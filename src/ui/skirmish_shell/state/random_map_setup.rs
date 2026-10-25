//! Random-map setup modal state (the Create Random Map dialog).
//!
//! Render-agnostic: owns the working option set, the enable/disable state
//! machine, and the accept/cancel outcome. Depends on the rmg options model and
//! the layout control enum only — no assets, no wgpu.

use crate::map::rmg::options::RmgOptions;
use crate::map::rmg::preview::PreviewImage;
use crate::map::rmg::randomize::{RandomRanged, derive_from_map_type, randomize};
use crate::map::rmg::settings::RmgSettings;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::static_reveal::DialogStatics;
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarPress, TrackbarRange};

use super::super::layout::RandomMapSetupControl;
use super::choose_map::ChooseMapSelection;

/// Process Main as the dialog's `RandomRanged` source. The app supplies process
/// Main (the installed simulation's continuation, or the frontend before the
/// first scenario). Map remains sim-independent (F05), so this trait binding
/// lives with the UI adapter.
pub struct MainRngRanged(pub crate::sim::rng::MainRngDraws);

impl RandomRanged for MainRngRanged {
    fn ranged(&mut self, min: i32, max: i32) -> i32 {
        self.0.ranged(min, max)
    }
}

/// Which combo is currently dropped open, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCombo {
    MapType,
    Time,
    Theater,
    Size,
    Resources,
}

/// The combos in layout row order. Row 5 of the option block is the players
/// trackbar, which is not a combo and is absent here.
pub const SETUP_COMBO_ROWS: [SetupCombo; 5] = [
    SetupCombo::MapType,
    SetupCombo::Time,
    SetupCombo::Theater,
    SetupCombo::Size,
    SetupCombo::Resources,
];

impl SetupCombo {
    /// The combo a control id addresses, if it is one.
    pub const fn from_control(control: RandomMapSetupControl) -> Option<Self> {
        match control {
            RandomMapSetupControl::MapType0x405 => Some(Self::MapType),
            RandomMapSetupControl::Time0x3ea => Some(Self::Time),
            RandomMapSetupControl::Theater0x407 => Some(Self::Theater),
            RandomMapSetupControl::Size0x406 => Some(Self::Size),
            RandomMapSetupControl::Resources0x408 => Some(Self::Resources),
            _ => None,
        }
    }

    /// The control id this combo is.
    pub const fn control(self) -> RandomMapSetupControl {
        match self {
            Self::MapType => RandomMapSetupControl::MapType0x405,
            Self::Time => RandomMapSetupControl::Time0x3ea,
            Self::Theater => RandomMapSetupControl::Theater0x407,
            Self::Size => RandomMapSetupControl::Size0x406,
            Self::Resources => RandomMapSetupControl::Resources0x408,
        }
    }

    /// Index into the layout's option rows.
    pub const fn row(self) -> usize {
        match self {
            Self::MapType => 0,
            Self::Time => 1,
            Self::Theater => 2,
            Self::Size => 3,
            Self::Resources => 4,
        }
    }
}

/// One combo entry: the label to resolve, an English fallback for a missing
/// string, and the value the entry carries into its option field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupComboItem {
    pub key: &'static str,
    pub fallback: &'static str,
    pub value: i32,
}

const fn item(key: &'static str, fallback: &'static str, value: i32) -> SetupComboItem {
    SetupComboItem {
        key,
        fallback,
        value,
    }
}

/// Landform entries. Note the values start at 1, not 0: archipelago (0) is
/// deliberately absent from the list, so a map type of 0 leaves the combo
/// showing nothing at all rather than falling back to the first entry.
const MAP_TYPE_ITEMS: [SetupComboItem; 4] = [
    item("TXT_MAP_CONTINENT", "Continent", 1),
    item("TXT_MAP_TEAM_CONTINENTS", "Team Continents", 2),
    item("TXT_MAP_INLAND", "Inland", 3),
    item("TXT_MAP_MOUNTAINOUS", "Mountainous", 4),
];

const TIME_ITEMS: [SetupComboItem; 4] = [
    item("TXT_TIME_MORNING", "Morning", 0),
    item("TXT_TIME_AFTERNOON", "Afternoon", 1),
    item("TXT_TIME_DUSK", "Dusk", 2),
    item("TXT_TIME_NIGHT", "Night", 3),
];

/// Only two theaters are offered, and their labels come from the theater table's
/// own name strings rather than the dialog's string list.
const THEATER_ITEMS: [SetupComboItem; 2] = [
    item("Name:Temperate", "Temperate", 0),
    item("Name:Snow", "Snow", 1),
];

const SIZE_ITEMS: [SetupComboItem; 4] = [
    item("TXT_MAPSIZE_SMALL", "Small", 0),
    item("TXT_MAPSIZE_MEDIUM", "Medium", 1),
    item("TXT_MAPSIZE_LARGE", "Large", 2),
    item("TXT_MAPSIZE_VERY_LARGE", "Very Large", 3),
];

const RESOURCE_ITEMS: [SetupComboItem; 4] = [
    item("TXT_RESOURCE_LOW", "Low", 0),
    item("TXT_RESOURCE_MODERATE", "Moderate", 1),
    item("TXT_RESOURCE_HIGH", "High", 2),
    item("TXT_RESOURCE_EXTREME", "Extreme", 3),
];

/// The entries a combo is filled with, in list order.
pub const fn setup_combo_items(combo: SetupCombo) -> &'static [SetupComboItem] {
    match combo {
        SetupCombo::MapType => &MAP_TYPE_ITEMS,
        SetupCombo::Time => &TIME_ITEMS,
        SetupCombo::Theater => &THEATER_ITEMS,
        SetupCombo::Size => &SIZE_ITEMS,
        SetupCombo::Resources => &RESOURCE_ITEMS,
    }
}

/// What closing the dialog should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptOutcome {
    /// Accepted: commit these options.
    Commit(Box<RmgOptions>),
    /// Rejected because nothing has been generated yet.
    NeedsGenerate,
}

/// The Players slider's range, 2..8 in steps of 1 (`TBM_SETRANGE` at
/// `0x0059722C`; no `0x4AB`).
pub const PLAYERS_RANGE: TrackbarRange = TrackbarRange::new(2, 8, 1);

/// The Create Random Map dialog.
#[derive(Debug, Clone)]
pub struct RandomMapSetupModalState {
    /// The working option set the controls edit.
    pub options: RmgOptions,
    /// True once Generate has run and no option has changed since.
    pub generated: bool,
    /// True while the synchronous generate block owns the dialog.
    pub generating: bool,
    /// Load/Delete enablement. Starts at saved-seed availability, but the
    /// generate action turns it on unconditionally, as the original does.
    pub saved_seed_buttons_enabled: bool,
    pub open_combo: Option<SetupCombo>,
    /// The Players slider holding the mouse since a press on it.
    pub players_hold: Option<TrackbarHold>,
    pub pressed_control: Option<RandomMapSetupControl>,
    /// The rasterised preview of the last generated map, shown in the preview
    /// box. `None` until a generate has produced one; any option edit clears it,
    /// because it no longer describes the configured map.
    pub generated_preview: Option<PreviewImage>,
    /// Bumped every time a generate produces a preview. The renderer keys its
    /// cached texture on this so it uploads once per generate rather than once
    /// per frame, and so a regenerated map of identical size still refreshes.
    pub preview_generation: u32,
    /// Restored verbatim if the player cancels.
    pub previous_selection: Option<ChooseMapSelection>,
    /// Heading `0x694` and status line `0x695`, alive while the seed browser
    /// hides the dialog.
    pub(crate) statics: DialogStatics,
}

impl RandomMapSetupModalState {
    /// Open the dialog over the current selection.
    ///
    /// The app-layer Scenario owner initializes the process-persistent seed
    /// before opening this presentation state. Accept starts disabled: the
    /// player must generate first.
    pub fn open(
        mut options: RmgOptions,
        previous_selection: Option<ChooseMapSelection>,
        saved_seeds_available: bool,
    ) -> Self {
        assert_ne!(
            options.seed, -1,
            "random-map options must be initialized by the app Scenario owner"
        );
        options.normalize();
        Self {
            options,
            generated: false,
            generating: false,
            saved_seed_buttons_enabled: saved_seeds_available,
            open_combo: None,
            players_hold: None,
            pressed_control: None,
            generated_preview: None,
            preview_generation: 0,
            previous_selection,
            statics: DialogStatics::default(),
        }
    }

    /// Whether a control is currently interactive. Every control is inert
    /// during the synchronous generate block, including Cancel.
    pub fn is_enabled(&self, control: RandomMapSetupControl) -> bool {
        use RandomMapSetupControl as C;
        if self.generating {
            return false;
        }
        match control {
            // Accept and save both require a generated result.
            C::Ok0x6c5 | C::Save0x6c3 => self.generated,
            C::Load0x6c2 | C::Delete0x6c4 => self.saved_seed_buttons_enabled,
            _ => true,
        }
    }

    /// Apply an option edit. Any change invalidates the generated result, so
    /// accept is disabled until the next generate.
    pub fn set_map_type(&mut self, value: i32) {
        self.options.map_type = value;
        self.on_option_changed();
    }

    pub fn set_time(&mut self, value: i32) {
        self.options.time = value;
        self.on_option_changed();
    }

    pub fn set_theater(&mut self, value: i32) {
        self.options.theater = value;
        self.on_option_changed();
    }

    /// One size selection drives both axes.
    pub fn set_size(&mut self, value: i32) {
        self.options.width = value;
        self.options.height = value;
        self.on_option_changed();
    }

    pub fn set_resources(&mut self, value: i32) {
        self.options.resources = value;
        self.on_option_changed();
    }

    pub fn set_num_players(&mut self, value: i32) {
        self.options.num_players = value;
        self.on_option_changed();
    }

    /// A press at device `(x, y)` on the Players slider window. Returns whether
    /// the value changed; the slider then clicks (GenericClick, `0x0061E6DD`),
    /// since `0x105` never turns its cue off (`0x4AE`).
    pub fn press_players(&mut self, x: i32, y: i32, window: RectPx, scale: ShellScale) -> bool {
        let press = PLAYERS_RANGE.press_in(self.options.num_players, x, y, window, scale);
        self.players_hold = press.hold(());
        match press {
            TrackbarPress::Jump(value) => self.change_num_players(value),
            _ => false,
        }
    }

    /// A move while the Players slider holds the mouse: a held thumb follows
    /// device `x`. Returns whether the value changed.
    pub fn drag_players(&mut self, x: i32, window: RectPx, scale: ShellScale) -> bool {
        self.players_hold.is_some_and(|hold| hold.dragging)
            && self.change_num_players(PLAYERS_RANGE.value_at_in(x, window, scale))
    }

    /// The slider notifies (`WM_HSCROLL`, case `0x00596B04`) only when its
    /// position changes (`0x0061E609`).
    fn change_num_players(&mut self, value: i32) -> bool {
        let changed = self.options.num_players != value;
        if changed {
            self.set_num_players(value);
        }
        changed
    }

    /// The option field a combo reflects. Size reads the width axis, which is
    /// the one the original resyncs the combo from.
    pub const fn combo_value(&self, combo: SetupCombo) -> i32 {
        match combo {
            SetupCombo::MapType => self.options.map_type,
            SetupCombo::Time => self.options.time,
            SetupCombo::Theater => self.options.theater,
            SetupCombo::Size => self.options.width,
            SetupCombo::Resources => self.options.resources,
        }
    }

    /// Which entry is highlighted, or `None` when the current value has no entry
    /// — the original resolves the selection by matching the entry, so an
    /// unlisted value simply clears the selection instead of clamping.
    pub fn selected_item_index(&self, combo: SetupCombo) -> Option<usize> {
        let value = self.combo_value(combo);
        setup_combo_items(combo)
            .iter()
            .position(|entry| entry.value == value)
    }

    /// Commit a picked entry and close the dropdown.
    pub fn set_combo_value(&mut self, combo: SetupCombo, value: i32) {
        match combo {
            SetupCombo::MapType => self.set_map_type(value),
            SetupCombo::Time => self.set_time(value),
            SetupCombo::Theater => self.set_theater(value),
            SetupCombo::Size => self.set_size(value),
            SetupCombo::Resources => self.set_resources(value),
        }
        self.open_combo = None;
    }

    /// A mouse press on the combos, and the cue it plays; `None` when the
    /// press is not theirs. `OwnerDraw_ComboBox @ 0x00617250` plays the open
    /// cue on any press of an enabled face (`0x006184A2..0x006184BA`) and
    /// opens the list only from its arrow zone, `x > right - 0x14`
    /// (`0x006184BF..0x006184D7`; the shared hit rect is 1 px wider). The open list's drop window (`0x0060D540`) captures the
    /// mouse: a pick, or a press anywhere else, closes it with the close cue
    /// (`0x0060E4E9..0x0060E500`) and goes no further.
    pub fn press_combos(
        &mut self,
        layout: &super::super::layout::RandomMapSetupLayout,
        x: i32,
        y: i32,
    ) -> Option<super::SkirmishShellUiSound> {
        if let Some(combo) = self.open_combo {
            let items = setup_combo_items(combo);
            if let Some(index) = super::super::layout::random_map_setup_dropdown_row_at(
                layout,
                combo.row(),
                items.len(),
                x,
                y,
            ) {
                self.set_combo_value(combo, items[index].value);
            }
            self.open_combo = None;
            return Some(super::SkirmishShellUiSound::GuiComboCloseSound);
        }
        let combo = super::super::layout::random_map_setup_control_at(layout, x, y)
            .and_then(SetupCombo::from_control)
            .filter(|combo| self.is_enabled(combo.control()))?;
        let face = layout.control_rects[combo.row()];
        if super::super::layout::combo_arrow_hit_rect(face, layout.scale).contains(x, y) {
            self.open_combo = Some(combo);
        }
        Some(super::SkirmishShellUiSound::GuiComboOpenSound)
    }

    fn on_option_changed(&mut self) {
        self.options.normalize();
        self.generated = false;
        self.generated_preview = None;
    }

    /// Surprise Me: randomize the option subset and invalidate the result.
    pub fn randomize_options(
        &mut self,
        settings: &RmgSettings,
        rng: &mut impl RandomRanged,
        description: &str,
    ) {
        randomize(&mut self.options, settings, rng, description);
        self.generated = false;
        self.generated_preview = None;
        self.open_combo = None;
    }

    /// Refresh the map-type-derived fields immediately before Generate.
    /// The caller supplies the process Main stream that also seeded this dialog.
    pub fn reroll_derived_for_generate(
        &mut self,
        settings: &RmgSettings,
        rng: &mut impl RandomRanged,
    ) {
        derive_from_map_type(&mut self.options, settings, rng);
    }

    /// Begin the synchronous generate block: every control goes inert.
    pub fn begin_generate(&mut self) {
        self.generating = true;
        self.open_combo = None;
        self.players_hold = None;
    }

    /// Release a failed worker without presenting an old/partial map as a
    /// successful result. Only finish_generate may enable Save and Accept.
    pub fn fail_generate(&mut self) {
        self.generating = false;
        self.generated = false;
        self.generated_preview = None;
    }

    /// Show the map as it stands part-way through the generate block.
    ///
    /// The original draws its preview repeatedly while generating, so the player
    /// watches the map build up rather than waiting on an empty box. The block
    /// is still running: nothing here unlocks a control or marks a result
    /// available.
    pub fn show_progress_preview(&mut self, preview: PreviewImage) {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.generated_preview = Some(preview);
    }

    /// End the generate block, marking a result available so accept unlocks.
    ///
    /// This also switches Load/Delete on unconditionally: the original
    /// re-enables the whole control set afterwards without re-testing whether
    /// any saved seed actually exists.
    pub fn finish_generate(&mut self, preview: Option<PreviewImage>) {
        self.generating = false;
        self.generated = true;
        self.saved_seed_buttons_enabled = true;
        if preview.is_some() {
            self.preview_generation = self.preview_generation.wrapping_add(1);
        }
        self.generated_preview = preview;
    }

    /// Accept. The original generates first when nothing has been generated
    /// yet, so a caller receiving `NeedsGenerate` must generate then retry.
    pub fn accept(&self) -> AcceptOutcome {
        if !self.generated {
            return AcceptOutcome::NeedsGenerate;
        }
        let mut committed = self.options.clone();
        committed.normalize();
        AcceptOutcome::Commit(Box::new(committed))
    }

    /// Cancel. Returns the selection to restore; the caller performs no other
    /// side effects.
    pub const fn cancel(&self) -> Option<ChooseMapSelection> {
        self.previous_selection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Always returns `max`, so draws are identifiable.
    struct MaxRng;
    impl RandomRanged for MaxRng {
        fn ranged(&mut self, _min: i32, max: i32) -> i32 {
            max
        }
    }

    fn opened() -> RandomMapSetupModalState {
        RandomMapSetupModalState::open(
            RmgOptions {
                seed: 0x1234,
                ..Default::default()
            },
            None,
            false,
        )
    }

    #[test]
    fn gsi_04_02_dialog_open_is_rng_pure_and_generate_reroll_uses_process_main_only() {
        let process_main = crate::sim::rng::MainRng::new(0);
        let reference_main = crate::sim::rng::MainRng::new(0);
        let scenario = crate::sim::rng::SimRng::new(0x1234);
        let scenario_before = scenario.state();
        let mut mapgen = crate::map::rmg::RmgRng::new(0x5678);
        let mut mapgen_reference = mapgen.clone();

        let mut modal = RandomMapSetupModalState::open(
            RmgOptions {
                seed: 0x2468,
                ..Default::default()
            },
            None,
            false,
        );
        assert_eq!(process_main.logical_state(), reference_main.logical_state());

        let mut expected_options = modal.options.clone();
        derive_from_map_type(
            &mut expected_options,
            &RmgSettings::default(),
            &mut MainRngRanged(reference_main.draws()),
        );
        modal.reroll_derived_for_generate(
            &RmgSettings::default(),
            &mut MainRngRanged(process_main.draws()),
        );

        assert_eq!(modal.options, expected_options);
        assert_eq!(process_main.logical_state(), reference_main.logical_state());
        assert_eq!(scenario.state(), scenario_before);
        assert_eq!(mapgen.next_u32(), mapgen_reference.next_u32());
    }

    #[test]
    fn ok_and_save_start_disabled() {
        let state = opened();
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
        assert!(!state.is_enabled(RandomMapSetupControl::Save0x6c3));
    }

    #[test]
    fn load_and_delete_follow_saved_seed_availability_at_open() {
        let none = opened();
        assert!(!none.is_enabled(RandomMapSetupControl::Load0x6c2));
        assert!(!none.is_enabled(RandomMapSetupControl::Delete0x6c4));

        let some = RandomMapSetupModalState::open(
            RmgOptions {
                seed: 1,
                ..Default::default()
            },
            None,
            true,
        );
        assert!(some.is_enabled(RandomMapSetupControl::Load0x6c2));
        assert!(some.is_enabled(RandomMapSetupControl::Delete0x6c4));
    }

    #[test]
    fn changing_an_option_disables_accept_again() {
        let mut state = opened();
        state.generated = true;
        state.set_resources(2);
        assert!(!state.generated, "an edit invalidates the generated result");
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
    }

    #[test]
    fn randomize_invalidates_the_generated_result() {
        let mut state = opened();
        state.generated = true;
        state.randomize_options(&RmgSettings::default(), &mut MaxRng, "Random Map");
        assert!(!state.generated);
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
        assert_eq!(state.options.description, "Random Map");
    }

    #[test]
    fn every_control_is_inert_during_generate_including_cancel() {
        let mut state = opened();
        state.begin_generate();
        for control in [
            RandomMapSetupControl::MapType0x405,
            RandomMapSetupControl::Theater0x407,
            RandomMapSetupControl::Players0x3eb,
            RandomMapSetupControl::Randomize0x621,
            RandomMapSetupControl::Generate0x620,
            RandomMapSetupControl::Ok0x6c5,
            RandomMapSetupControl::Cancel0x5c0,
        ] {
            assert!(!state.is_enabled(control), "{control:?} must be inert");
        }
    }

    fn one_pixel_preview() -> PreviewImage {
        PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        }
    }

    #[test]
    fn progress_previews_show_without_ending_the_generate_block() {
        let mut state = opened();
        state.begin_generate();
        state.show_progress_preview(one_pixel_preview());

        assert!(state.generated_preview.is_some(), "the map is on screen");
        // Still generating: nothing has unlocked, and accept must not become
        // available off a half-built map.
        assert!(state.generating);
        assert!(!state.generated);
        assert!(!state.is_enabled(RandomMapSetupControl::Cancel0x5c0));
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
    }

    #[test]
    fn each_progress_preview_bumps_the_texture_key() {
        // The renderer caches its texture on this counter, so a snapshot that
        // did not bump it would never reach the screen.
        let mut state = opened();
        state.begin_generate();
        let before = state.preview_generation;
        state.show_progress_preview(one_pixel_preview());
        state.show_progress_preview(one_pixel_preview());
        assert_eq!(state.preview_generation, before.wrapping_add(2));
    }

    #[test]
    fn failed_generation_cannot_accept_or_save_old_or_partial_results() {
        for had_prior_result in [false, true] {
            let mut state = opened();
            if had_prior_result {
                state.finish_generate(Some(one_pixel_preview()));
            }
            state.begin_generate();
            state.show_progress_preview(one_pixel_preview());
            state.fail_generate();
            assert!(!state.generating);
            assert!(!state.generated);
            assert!(state.generated_preview.is_none());
            assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
            assert!(!state.is_enabled(RandomMapSetupControl::Save0x6c3));
            assert!(state.is_enabled(RandomMapSetupControl::Generate0x620));
            assert!(state.is_enabled(RandomMapSetupControl::Cancel0x5c0));
            assert_eq!(state.accept(), AcceptOutcome::NeedsGenerate);
            // Retrying and succeeding is the only path back to acceptance.
            state.begin_generate();
            state.finish_generate(Some(one_pixel_preview()));
            assert!(state.is_enabled(RandomMapSetupControl::Ok0x6c5));
            assert!(state.is_enabled(RandomMapSetupControl::Save0x6c3));
        }
    }

    #[test]
    fn generate_enables_load_and_delete_even_with_no_saved_seeds() {
        // The original re-enables the whole control set after generating
        // without re-testing saved-seed availability.
        let mut state = opened();
        assert!(!state.is_enabled(RandomMapSetupControl::Load0x6c2));
        state.begin_generate();
        state.finish_generate(None);
        assert!(state.is_enabled(RandomMapSetupControl::Load0x6c2));
        assert!(state.is_enabled(RandomMapSetupControl::Delete0x6c4));
    }

    #[test]
    fn accept_after_generate_commits_normalized_options() {
        let mut state = opened();
        state.finish_generate(None);
        match state.accept() {
            AcceptOutcome::Commit(options) => {
                assert!(options.tiberium >= 1, "committed options are normalized");
                assert_eq!(options.seed, state.options.seed);
            }
            other => panic!("expected a commit, got {other:?}"),
        }
    }

    #[test]
    fn map_type_entries_start_at_one_and_omit_archipelago() {
        let items = setup_combo_items(SetupCombo::MapType);
        assert_eq!(items.len(), 4);
        assert_eq!(
            items.iter().map(|entry| entry.value).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn an_unlisted_map_type_clears_the_selection() {
        let mut state = opened();
        state.set_map_type(0);
        assert_eq!(
            state.selected_item_index(SetupCombo::MapType),
            None,
            "archipelago has no entry, so nothing is shown as selected"
        );
        state.set_map_type(3);
        assert_eq!(state.selected_item_index(SetupCombo::MapType), Some(2));
    }

    #[test]
    fn size_selection_reflects_the_width_axis() {
        let mut state = opened();
        state.set_combo_value(SetupCombo::Size, 2);
        assert_eq!((state.options.width, state.options.height), (2, 2));
        assert_eq!(state.selected_item_index(SetupCombo::Size), Some(2));
    }

    #[test]
    fn picking_an_entry_closes_the_dropdown_and_invalidates_the_result() {
        let mut state = opened();
        state.open_combo = Some(SetupCombo::Resources);
        state.generated = true;
        state.set_combo_value(SetupCombo::Resources, 3);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.options.resources, 3);
        assert!(!state.generated);
    }

    /// A face press plays the open cue but only the arrow opens the list. With
    /// a list open, a press on a row picks it and one anywhere else only
    /// closes it; both play the close cue.
    #[test]
    fn combo_presses_open_from_the_arrow_and_close_from_anywhere() {
        use super::super::SkirmishShellUiSound::{GuiComboCloseSound, GuiComboOpenSound};
        let layout = super::super::super::layout::compute_random_map_setup_layout(800, 600);
        let mut state = opened();
        let time = layout.control_rects[SetupCombo::Time.row()];
        let (text, arrow) = ((time.x + 4, time.y + 4), (time.x + time.w - 4, time.y + 4));
        assert_eq!(
            state.press_combos(&layout, text.0, text.1),
            Some(GuiComboOpenSound)
        );
        assert_eq!(state.open_combo, None);
        assert_eq!(
            state.press_combos(&layout, arrow.0, arrow.1),
            Some(GuiComboOpenSound)
        );
        assert_eq!(state.open_combo, Some(SetupCombo::Time));
        let list = super::super::super::layout::random_map_setup_dropdown_rect(
            &layout,
            SetupCombo::Time.row(),
            setup_combo_items(SetupCombo::Time).len(),
        );
        let dusk_row = list.y + list.h * 5 / 8;
        assert_eq!(
            state.press_combos(&layout, list.x + 4, dusk_row),
            Some(GuiComboCloseSound)
        );
        assert_eq!(
            (
                state.open_combo,
                state.selected_item_index(SetupCombo::Time)
            ),
            (None, Some(2))
        );
        assert_eq!(
            state.press_combos(&layout, arrow.0, arrow.1),
            Some(GuiComboOpenSound)
        );
        let outside = (layout.randomize.x + 1, layout.randomize.y + 1);
        assert_eq!(
            state.press_combos(&layout, outside.0, outside.1),
            Some(GuiComboCloseSound)
        );
        assert_eq!(
            (
                state.open_combo,
                state.selected_item_index(SetupCombo::Time)
            ),
            (None, Some(2))
        );
        assert_eq!(state.press_combos(&layout, outside.0, outside.1), None);
    }
}
