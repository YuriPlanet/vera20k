//! VERA's Game Options box on the skirmish page (`0x102`). OK, Enter or Esc
//! closes the box: VERA's own settings go under `[VERA]`, and the session
//! options gamemd keeps but YR's page never shows (Tech Level, Bases, Fog of
//! War, Multi-Engineer and Harvester Truce) back to the page, which keeps
//! them with its other options. Superweapon Limit and Terrain Deformation
//! also seed a host's network room. Allow cheat codes applies only to typed
//! commands during offline skirmish.

use vera20k_gui::{NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::rules::personal_rules::{SUPERWEAPON_LIMIT_MAX, admitted_superweapon_limit};
use crate::skirmish_launch::SessionOptions;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale};
use crate::ui::shell::page::{self, PageCue, PageEvent};
use vera20k_gui::shell::trackbar::TrackbarRange;

use super::SkirmishShellLayout;

/// The page's button: the room's button text.
pub const GAME_OPTIONS_LABEL: &str = "GUI:GameOptions";
pub const GAME_OPTIONS_HELP: &str = "VERA:SkirmishGameOptionsHelp";
/// `docs/terrain-deformation.md`.
pub const TERRAIN_DEFORMATION_LABEL: &str = "VERA:TerrainDeformation";
pub const TERRAIN_DEFORMATION_HELP: &str = "VERA:TerrainDeformationHelp";
pub const SKIRMISH_CHEATS_LABEL: &str = "VERA:SkirmishCheats";
pub const SKIRMISH_CHEATS_HELP: &str = "VERA:SkirmishCheatsHelp";
/// `docs/superweapon-limit.md`.
pub const SUPERWEAPON_LIMIT_LABEL: &str = "VERA:SuperweaponLimit";
pub const SUPERWEAPON_LIMIT_HELP: &str = "VERA:SuperweaponLimitHelp";
/// The slider's plaque at its left end.
pub const SUPERWEAPON_LIMIT_OFF: &str = "VERA:SuperweaponLimitOff";
/// Match options gamemd keeps but YR's skirmish and room dialogs never show
/// (`[MultiplayerDialogSettings]`, `GameOptions::from_multiplayer_dialog_settings`);
/// this box and the room's Game Options show them.
pub const TECH_LEVEL_LABEL: &str = "VERA:TechLevel";
pub const TECH_LEVEL_HELP: &str = "VERA:TechLevelHelp";
pub const BASES_LABEL: &str = "VERA:Bases";
pub const BASES_HELP: &str = "VERA:BasesHelp";
pub const FOG_OF_WAR_LABEL: &str = "VERA:FogOfWar";
pub const FOG_OF_WAR_HELP: &str = "VERA:FogOfWarHelp";
pub const MULTI_ENGINEER_LABEL: &str = "VERA:MultiEngineer";
pub const MULTI_ENGINEER_HELP: &str = "VERA:MultiEngineerHelp";
pub const HARVESTER_TRUCE_LABEL: &str = "VERA:HarvesterTruce";
pub const HARVESTER_TRUCE_HELP: &str = "VERA:HarvesterTruceHelp";

/// The Tech Level slider of this box and of the room's Game Options, over
/// the levels `[MultiplayerDialogSettings] TechLevel` holds in retail (10)
/// and below.
pub const TECH_LEVEL_RANGE: TrackbarRange = TrackbarRange::new(1, 10, 1);

/// A stored Tech Level the slider can hold; `None` leaves the rules' or the
/// page's own value, as the page's other remembered values do.
pub fn admitted_tech_level(value: i32) -> Option<i32> {
    (TECH_LEVEL_RANGE.min..=TECH_LEVEL_RANGE.max)
        .contains(&value)
        .then_some(value)
}

/// The Superweapon Limit slider of this box and of the room's Game Options:
/// 1 is Off, then the limit itself up to [`SUPERWEAPON_LIMIT_MAX`].
pub const SUPERWEAPON_LIMIT_RANGE: TrackbarRange =
    TrackbarRange::new(1, SUPERWEAPON_LIMIT_MAX as i32, 1);

/// What the slider's plaque reads for a limit as the profile stores it (0
/// while off).
pub fn superweapon_limit_text(limit: u8) -> String {
    match limit {
        0 => crate::i18n::tr(SUPERWEAPON_LIMIT_OFF),
        limit => limit.to_string(),
    }
}

/// The rules the box shows: VERA's own, as the profile stores them, and the
/// page's session options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchRules {
    /// 0 while off, else 2 up ([`admitted_superweapon_limit`]).
    pub superweapon_limit: u8,
    pub terrain_deformation: bool,
    pub skirmish_cheats: bool,
    /// `SkirmishShellState::session`.
    pub session: SessionOptions,
    /// The page's Short Game, which unticking Bases unticks.
    pub short_game: bool,
}

impl MatchRules {
    /// The value checkbox `control` holds; `None` for the sliders and OK.
    fn check_mut(&mut self, control: GameOptionsControl) -> Option<&mut bool> {
        match control {
            GameOptionsControl::Bases => Some(&mut self.session.bases),
            GameOptionsControl::FogOfWar => Some(&mut self.session.fog_of_war),
            GameOptionsControl::MultiEngineer => Some(&mut self.session.multi_engineer),
            GameOptionsControl::HarvesterTruce => Some(&mut self.session.harvester_truce),
            GameOptionsControl::TerrainDeformation => Some(&mut self.terrain_deformation),
            GameOptionsControl::SkirmishCheats => Some(&mut self.skirmish_cheats),
            GameOptionsControl::Slider(_) | GameOptionsControl::Ok => None,
        }
    }

    /// Whether checkbox `control` is ticked.
    pub fn checked(mut self, control: GameOptionsControl) -> bool {
        self.check_mut(control).is_some_and(|checked| *checked)
    }
}

/// A slider of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOptionsSlider {
    SuperweaponLimit,
    TechLevel,
}

impl GameOptionsSlider {
    pub const ALL: [Self; 2] = [Self::SuperweaponLimit, Self::TechLevel];

    pub fn range(self) -> TrackbarRange {
        match self {
            Self::SuperweaponLimit => SUPERWEAPON_LIMIT_RANGE,
            Self::TechLevel => TECH_LEVEL_RANGE,
        }
    }
}

/// A control of the open box, by its id in [`DIALOG`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOptionsControl {
    Slider(GameOptionsSlider),
    Bases,
    FogOfWar,
    MultiEngineer,
    HarvesterTruce,
    TerrainDeformation,
    SkirmishCheats,
    Ok,
}

/// Every control in paint order: the sliders, the checkboxes, then OK.
const CONTROLS: [GameOptionsControl; 9] = [
    GameOptionsControl::Slider(GameOptionsSlider::SuperweaponLimit),
    GameOptionsControl::Slider(GameOptionsSlider::TechLevel),
    GameOptionsControl::Bases,
    GameOptionsControl::FogOfWar,
    GameOptionsControl::MultiEngineer,
    GameOptionsControl::HarvesterTruce,
    GameOptionsControl::TerrainDeformation,
    GameOptionsControl::SkirmishCheats,
    GameOptionsControl::Ok,
];

/// The sliders' caption statics, which show their help too.
const fn slider_label_id(slider: GameOptionsSlider) -> i32 {
    match slider {
        GameOptionsSlider::SuperweaponLimit => 5,
        GameOptionsSlider::TechLevel => 11,
    }
}

impl GameOptionsControl {
    /// Its widget's id in [`DIALOG`].
    pub const fn id(self) -> i32 {
        match self {
            Self::Slider(GameOptionsSlider::SuperweaponLimit) => 1,
            Self::TerrainDeformation => 2,
            Self::SkirmishCheats => 3,
            Self::Ok => 4,
            Self::Slider(GameOptionsSlider::TechLevel) => 6,
            Self::Bases => 7,
            Self::FogOfWar => 8,
            Self::MultiEngineer => 9,
            Self::HarvesterTruce => 10,
        }
    }

    /// The control widget `id` is, or whose caption it is.
    fn of_id(id: i32) -> Option<Self> {
        GameOptionsSlider::ALL
            .into_iter()
            .find(|slider| slider_label_id(*slider) == id)
            .map(Self::Slider)
            .or_else(|| CONTROLS.into_iter().find(|control| control.id() == id))
    }

    /// The checkboxes, in the order the box lays them out.
    pub const CHECKBOXES: [Self; 6] = [
        Self::Bases,
        Self::FogOfWar,
        Self::MultiEngineer,
        Self::HarvesterTruce,
        Self::TerrainDeformation,
        Self::SkirmishCheats,
    ];

    /// The text beside it; OK draws its own.
    pub fn label_key(self) -> Option<&'static str> {
        Some(match self {
            Self::Slider(GameOptionsSlider::SuperweaponLimit) => SUPERWEAPON_LIMIT_LABEL,
            Self::Slider(GameOptionsSlider::TechLevel) => TECH_LEVEL_LABEL,
            Self::Bases => BASES_LABEL,
            Self::FogOfWar => FOG_OF_WAR_LABEL,
            Self::MultiEngineer => MULTI_ENGINEER_LABEL,
            Self::HarvesterTruce => HARVESTER_TRUCE_LABEL,
            Self::TerrainDeformation => TERRAIN_DEFORMATION_LABEL,
            Self::SkirmishCheats => SKIRMISH_CHEATS_LABEL,
            Self::Ok => return None,
        })
    }

    /// Its status help: VERA's own lines; OK has none, as the shell's
    /// message boxes give theirs none.
    pub fn status_help_key(self) -> Option<&'static str> {
        Some(match self {
            Self::Slider(GameOptionsSlider::SuperweaponLimit) => SUPERWEAPON_LIMIT_HELP,
            Self::Slider(GameOptionsSlider::TechLevel) => TECH_LEVEL_HELP,
            Self::Bases => BASES_HELP,
            Self::FogOfWar => FOG_OF_WAR_HELP,
            Self::MultiEngineer => MULTI_ENGINEER_HELP,
            Self::HarvesterTruce => HARVESTER_TRUCE_HELP,
            Self::TerrainDeformation => TERRAIN_DEFORMATION_HELP,
            Self::SkirmishCheats => SKIRMISH_CHEATS_HELP,
            Self::Ok => return None,
        })
    }
}

/// The open box over the page in the shell controller.
pub const DIALOG: DialogResourceId = DialogResourceId(0x7103);

/// The open box: the rules it shows. Its widgets hold the press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOptionsBox {
    pub rules: MatchRules,
}

impl GameOptionsBox {
    pub fn new(rules: MatchRules) -> Self {
        Self { rules }
    }

    /// The slider's position: Superweapon Limit's is 1 while off, else the
    /// limit; a rules Tech Level past the slider's ends sits at the end.
    pub fn slider_value(&self, slider: GameOptionsSlider) -> i32 {
        match slider {
            GameOptionsSlider::SuperweaponLimit => i32::from(self.rules.superweapon_limit.max(1)),
            GameOptionsSlider::TechLevel => self
                .rules
                .session
                .tech_level
                .clamp(TECH_LEVEL_RANGE.min, TECH_LEVEL_RANGE.max),
        }
    }

    /// The slider's plaque.
    pub fn slider_text(&self, slider: GameOptionsSlider) -> String {
        match slider {
            GameOptionsSlider::SuperweaponLimit => {
                superweapon_limit_text(self.rules.superweapon_limit)
            }
            GameOptionsSlider::TechLevel => self.rules.session.tech_level.to_string(),
        }
    }

    /// Stores the slider's new position; whether the rule changed.
    fn set_slider_value(&mut self, slider: GameOptionsSlider, value: i32) -> bool {
        match slider {
            GameOptionsSlider::SuperweaponLimit => {
                let limit = admitted_superweapon_limit(value);
                std::mem::replace(&mut self.rules.superweapon_limit, limit) != limit
            }
            GameOptionsSlider::TechLevel => {
                let level = TECH_LEVEL_RANGE.stepped(value);
                std::mem::replace(&mut self.rules.session.tech_level, level) != level
            }
        }
    }

    /// Checkbox `control` toggled to `on`: unticking Bases unticks the
    /// page's Short Game, as C2's Bases click does (`0x0078FAAA`).
    fn toggle(&mut self, control: GameOptionsControl, on: bool) {
        if control == GameOptionsControl::Bases {
            if self.rules.session.bases != on {
                crate::skirmish_launch::click_bases(
                    &mut self.rules.session.bases,
                    &mut self.rules.short_game,
                );
            }
        } else if let Some(checked) = self.rules.check_mut(control) {
            *checked = on;
        }
    }
}

/// The box's widgets, pushed over the page when they are not on top, on
/// `layout` and showing `open`; their root.
pub fn sync(
    controller: &mut DialogController,
    open: &GameOptionsBox,
    layout: &GameOptionsLayout,
) -> Option<NodeId> {
    if controller.top_id() != Some(DIALOG) {
        controller.push(DIALOG);
    }
    controller.set_scale(layout.scale);
    let root = controller.root_in(DIALOG)?;
    let gui = controller.gui_mut();
    page::ensure_built(gui, root, || {
        let labels = GameOptionsSlider::ALL.map(|slider| {
            let control = GameOptionsControl::Slider(slider);
            NodeSpec::new(NodeKind::Text)
                .id(slider_label_id(slider))
                .tooltip(control.status_help_key().unwrap_or_default())
                .with(Prop::Align, Value::Int(0))
                .with(
                    Prop::Text,
                    Value::Text(control.label_key().map(crate::i18n::tr).unwrap_or_default()),
                )
        });
        let controls = CONTROLS.map(|control| {
            let spec = match control {
                GameOptionsControl::Slider(slider) => {
                    let range = slider.range();
                    NodeSpec::new(NodeKind::Trackbar)
                        .with(Prop::RehoverOnRelease, Value::Bool(true))
                        .with(Prop::Min, Value::Int(range.min))
                        .with(Prop::Max, Value::Int(range.max))
                        .with(Prop::Step, Value::Int(range.step))
                }
                // VERA's box closes on Enter or Escape as on OK, as the
                // room's does.
                GameOptionsControl::Ok => NodeSpec::new(NodeKind::Button)
                    .with(Prop::Default, Value::Bool(true))
                    .with(Prop::Cancel, Value::Bool(true)),
                _ => NodeSpec::new(NodeKind::Checkbox).with(
                    Prop::Text,
                    Value::Text(control.label_key().map(crate::i18n::tr).unwrap_or_default()),
                ),
            };
            let spec = spec.id(control.id());
            match control.status_help_key() {
                Some(key) => spec.tooltip(key),
                None => spec,
            }
        });
        NodeSpec::new(NodeKind::Dialog)
            .children(labels)
            .children(controls)
    });
    page::sync_rects(gui, root, |id| {
        let id = i32::from(id);
        if let Some(slider) = GameOptionsSlider::ALL
            .into_iter()
            .find(|slider| slider_label_id(*slider) == id)
        {
            return Some(layout.slider_label(slider));
        }
        match CONTROLS.into_iter().find(|control| control.id() == id)? {
            GameOptionsControl::Slider(slider) => Some(layout.slider(slider)),
            GameOptionsControl::Ok => Some(layout.ok),
            control => layout
                .checkboxes()
                .find(|(each, _)| *each == control)
                .map(|(_, rect)| rect),
        }
    });
    for slider in GameOptionsSlider::ALL {
        let id = GameOptionsControl::Slider(slider).id() as u16;
        page::set(
            gui,
            root,
            id,
            Prop::Pos,
            Value::Int(open.slider_value(slider)),
        );
        page::set(
            gui,
            root,
            id,
            Prop::Text,
            Value::Text(open.slider_text(slider)),
        );
    }
    for control in GameOptionsControl::CHECKBOXES {
        let checked = Value::Bool(open.rules.checked(control));
        page::set(gui, root, control.id() as u16, Prop::Checked, checked);
    }
    Some(root)
}

/// The pointer at `x`, `y` over the open box, which takes every press, move
/// and release over the page: the cues to play, and true when OK came up
/// over its own press and the box closes.
pub fn pointer(
    controller: &mut DialogController,
    state: &mut super::SkirmishShellState,
    layout: &GameOptionsLayout,
    at: super::page::Pointer,
    x: i32,
    y: i32,
) -> (Vec<PageCue>, bool) {
    let (mut cues, mut close) = (Vec::new(), false);
    let Some(open) = state.game_options.as_mut() else {
        return (cues, close);
    };
    if sync(controller, open, layout).is_none() {
        return (cues, close);
    }
    for event in super::page::route(controller, at, x, y) {
        match event {
            PageEvent::Cue(cue) => cues.push(cue),
            PageEvent::Toggled(id, on) => {
                if let Some(control) = GameOptionsControl::of_id(id.into()) {
                    open.toggle(control, on);
                }
            }
            // A pointer-driven change clicks, as the page's sliders do
            // (`0x0061E6DD`).
            PageEvent::Slid {
                id,
                value,
                final_: false,
            } => {
                if let Some(GameOptionsControl::Slider(slider)) =
                    GameOptionsControl::of_id(id.into())
                    && open.set_slider_value(slider, value)
                {
                    cues.push(PageCue::ListClick);
                }
            }
            PageEvent::Activated(id) if i32::from(id) == GameOptionsControl::Ok.id() => {
                close = true
            }
            _ => {}
        }
    }
    (cues, close)
}

/// A key over the open box, which takes every key: true when it activated
/// OK (Enter or Escape) and the box closes.
pub fn key(
    controller: &mut DialogController,
    state: &mut super::SkirmishShellState,
    layout: &GameOptionsLayout,
    key: vera20k_gui::Key,
) -> bool {
    let Some(open) = state.game_options.as_ref() else {
        return false;
    };
    if sync(controller, open, layout).is_none() {
        return false;
    }
    controller.on_input(vera20k_gui::InputEvent::Key(key));
    let raw = controller.drain_events();
    page::page_events(controller.gui(), raw)
        .contains(&PageEvent::Activated(GameOptionsControl::Ok.id() as u16))
}

/// The box's control under the pointer, for its status help.
pub fn hovered(gui: &vera20k_gui::Gui) -> Option<GameOptionsControl> {
    GameOptionsControl::of_id(gui.id_of(gui.hovered()?)?)
}

/// The box's rects on screen, in the page's mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOptionsLayout {
    pub dialog: RectPx,
    pub superweapon_limit_label: RectPx,
    pub superweapon_limit: RectPx,
    pub tech_level_label: RectPx,
    pub tech_level: RectPx,
    /// [`GameOptionsControl::CHECKBOXES`]' rects, in that order.
    pub checkboxes: [RectPx; 6],
    pub ok: RectPx,
    pub scale: ShellScale,
}

impl GameOptionsLayout {
    /// The box over the skirmish page `page`, placed as the room places its
    /// Game Options (`MultiplayerSetupLayout::move_options_into_popup`): the
    /// shell's 451x326 menu box on the page's content, the sliders' labels
    /// and sliders in the room's two columns, gamemd's session checkboxes
    /// under them on the left, VERA's own on the right, and OK in the room's
    /// place.
    pub fn over(page: &SkirmishShellLayout) -> Self {
        let scale = page.scale;
        // The retail page sits in its own window; a fitted one is the
        // 800x600 page, scaled.
        let menu = if scale.is_unscaled() {
            crate::ui::shell::modal::menu_box(page.screen.w, page.screen.h)
                .translate(page.screen.x, page.screen.y)
        } else {
            crate::ui::shell::modal::menu_box(SHELL_BASE_W, SHELL_BASE_H)
        };
        let popup = |x: i32, y: i32, w: i32, h: i32| {
            scale.scale_rect(RectPx::new(menu.x + x, menu.y + y, w, h))
        };
        let left = |row: i32| popup(24, 92 + 22 * row, 185, 17);
        let right = |row: i32| popup(215, 92 + 22 * row, 226, 17);
        Self {
            dialog: popup(0, 0, menu.w, menu.h),
            superweapon_limit_label: popup(24, 32, 185, 17),
            superweapon_limit: popup(215, 30, 129, 21),
            tech_level_label: popup(24, 62, 185, 17),
            tech_level: popup(215, 60, 129, 21),
            checkboxes: [left(0), left(1), left(2), left(3), right(0), right(1)],
            ok: popup(276, 292, 124, 24),
            scale,
        }
    }

    /// Each checkbox with its rect.
    pub fn checkboxes(&self) -> impl Iterator<Item = (GameOptionsControl, RectPx)> {
        GameOptionsControl::CHECKBOXES
            .into_iter()
            .zip(self.checkboxes)
    }

    /// The slider's track.
    pub fn slider(&self, slider: GameOptionsSlider) -> RectPx {
        match slider {
            GameOptionsSlider::SuperweaponLimit => self.superweapon_limit,
            GameOptionsSlider::TechLevel => self.tech_level,
        }
    }

    /// The text left of the slider.
    pub fn slider_label(&self, slider: GameOptionsSlider) -> RectPx {
        match slider {
            GameOptionsSlider::SuperweaponLimit => self.superweapon_limit_label,
            GameOptionsSlider::TechLevel => self.tech_level_label,
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::page::Pointer;
    use crate::ui::skirmish_shell::{SkirmishShellState, checkbox_icon_rect};

    fn layout() -> GameOptionsLayout {
        GameOptionsLayout::over(&crate::ui::skirmish_shell::compute_layout(800, 600))
    }

    /// The box open over a page, driven as the app drives it.
    struct Open(DialogController, SkirmishShellState);

    impl Open {
        fn new(rules: MatchRules) -> Self {
            let state = SkirmishShellState {
                game_options: Some(GameOptionsBox::new(rules)),
                ..Default::default()
            };
            Open(DialogController::default(), state)
        }
        fn at(&mut self, at: Pointer, x: i32, y: i32) -> (Vec<PageCue>, bool) {
            pointer(&mut self.0, &mut self.1, &layout(), at, x, y)
        }
        fn mouse_down(&mut self, x: i32, y: i32) -> Vec<PageCue> {
            self.at(Pointer::Move, x, y);
            self.at(Pointer::Down, x, y).0
        }
        fn mouse_up(&mut self, x: i32, y: i32) -> bool {
            self.at(Pointer::Move, x, y);
            self.at(Pointer::Up, x, y).1
        }
        fn rules(&self) -> MatchRules {
            self.1.game_options.unwrap().rules
        }
    }

    /// The slider stores what the profile and the room read: off at its
    /// left end, then the limit itself; Terrain Deformation's box toggles.
    #[test]
    fn the_box_stores_off_at_the_left_end_and_the_limit_beyond() {
        let layout = layout();
        let rect = |control| {
            layout
                .checkboxes()
                .find(|(each, _)| *each == control)
                .map(|(_, rect)| rect)
                .unwrap()
        };
        let bar = layout.superweapon_limit;
        let mut open = Open::new(MatchRules {
            superweapon_limit: 6,
            ..Default::default()
        });
        let row = bar.y + bar.h - 4;
        assert_eq!(open.mouse_down(bar.x + 1, row), [PageCue::ListClick]);
        assert_eq!(open.rules().superweapon_limit, 0);
        open.mouse_up(bar.x + 1, row);
        // The plaque's 50 pixels stay off the track: its far end is the
        // most the limit allows.
        open.mouse_down(bar.x + bar.w - 51, row);
        assert_eq!(open.rules().superweapon_limit, SUPERWEAPON_LIMIT_MAX);
        open.mouse_up(bar.x + bar.w - 51, row);

        let check = checkbox_icon_rect(rect(GameOptionsControl::TerrainDeformation), layout.scale);
        assert_eq!(
            open.mouse_down(check.x + 2, check.y + 2),
            [PageCue::Checkbox]
        );
        assert!(open.rules().terrain_deformation);

        let check = checkbox_icon_rect(rect(GameOptionsControl::SkirmishCheats), layout.scale);
        let (x, y) = (check.x + 2, check.y + 2);
        open.at(Pointer::Move, x, y);
        assert_eq!(
            hovered(open.0.gui()).and_then(GameOptionsControl::status_help_key),
            Some(SKIRMISH_CHEATS_HELP),
        );
        assert!(!open.rules().skirmish_cheats);
        open.mouse_down(x, y);
        assert!(open.rules().skirmish_cheats);
    }

    /// Tech Level's slider runs from 1 to 10 below Superweapon Limit's, and
    /// the session checkboxes tick the page's session options; unticking
    /// Bases unticks the page's Short Game, as C2 does.
    #[test]
    fn the_box_sets_the_session_options() {
        let layout = layout();
        let mut open = Open::new(MatchRules {
            short_game: true,
            ..Default::default()
        });
        assert_eq!(open.rules().session.tech_level, 10);
        let bar = layout.tech_level;
        let row = bar.y + bar.h - 4;
        open.at(Pointer::Move, bar.x + 1, row);
        assert_eq!(
            hovered(open.0.gui()),
            Some(GameOptionsControl::Slider(GameOptionsSlider::TechLevel))
        );
        open.mouse_down(bar.x + 1, row);
        open.mouse_up(bar.x + 1, row);
        assert_eq!(open.rules().session.tech_level, 1);
        assert_eq!(open.rules().superweapon_limit, 0);
        for (control, rect) in layout.checkboxes().take(4) {
            let check = checkbox_icon_rect(rect, layout.scale);
            let before = open.rules().checked(control);
            open.mouse_down(check.x + 2, check.y + 2);
            open.mouse_up(check.x + 2, check.y + 2);
            assert_ne!(open.rules().checked(control), before, "{control:?}");
        }
        let session = open.rules().session;
        assert!(!session.bases && session.fog_of_war && session.multi_engineer);
        assert!(session.harvester_truce);
        assert!(!open.rules().short_game);
    }

    /// OK closes the box only when the press and the release are both on it.
    #[test]
    fn ok_closes_only_on_a_release_over_its_own_press() {
        let ok = layout().ok;
        let mut open = Open::new(MatchRules::default());
        assert_eq!(open.mouse_down(ok.x + 2, ok.y + 2), [PageCue::Button]);
        assert!(!open.mouse_up(ok.x - 2, ok.y + 2));
        open.mouse_down(ok.x - 2, ok.y + 2);
        assert!(!open.mouse_up(ok.x + 2, ok.y + 2));
        open.mouse_down(ok.x + 2, ok.y + 2);
        assert!(open.mouse_up(ok.x + 2, ok.y + 2));
    }
}
