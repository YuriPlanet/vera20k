//! Active Sound B8: original6B6230/6B6300, resourceBEFE4C.
//! Values here are control projections; the process Options/Theme owners retain
//! audio values, playback and playlist identity.

use vera20k_gui::{Gui, NodeId, NodeKind, Prop, UiEvent, Value};

use super::button::ShellButtonInteraction;
use super::geom::{self, RectPx};
use super::in_game_shell::{self, InGameShellLayout};
use crate::ui::shell::trackbar::{thumb_left, trackbar_position_from_x};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum SoundSlider {
    Music,
    Sound,
    Voice,
}
impl SoundSlider {
    pub const ALL: [Self; 3] = [Self::Music, Self::Sound, Self::Voice];
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundButton {
    Back,
    Play,
    Stop,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundControl {
    Slider(SoundSlider),
    Button(SoundButton),
    Shuffle,
    Repeat,
    List,
}

impl SoundControl {
    pub fn help(self) -> (&'static str, &'static str) {
        match self {
            Self::Slider(SoundSlider::Music) => (
                "STT:SoundOptSliderMusic",
                "Controls the music volume level.",
            ),
            Self::Slider(SoundSlider::Sound) => (
                "STT:SoundOptSliderSound",
                "Controls the sound volume level.",
            ),
            Self::Slider(SoundSlider::Voice) => (
                "STT:SoundOptSliderVoice",
                "Controls the voice volume level.",
            ),
            Self::Button(SoundButton::Back) => {
                ("STT:SoundOptButtonBack", "Return to the previous screen.")
            }
            Self::Button(SoundButton::Play) => ("STT:SoundOptButtonPlay", "Play a music track."),
            Self::Button(SoundButton::Stop) => ("STT:SoundOptButtonStop", "Stop a music track."),
            Self::Shuffle => (
                "STT:SoundOptCBoxShuffle",
                "Shuffle the order of the music tracks that play.",
            ),
            Self::Repeat => ("STT:SoundOptCBoxRepeat", "Repeat a single music track."),
            Self::List => ("STT:SoundOptListScore", "Lists the available music tracks."),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SoundTrackRow {
    pub theme_index: i32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SoundState {
    pub positions: [u8; 3],
    pub rows: Vec<SoundTrackRow>,
    pub selected: Option<usize>,
    pub top: usize,
    pub buttons: ShellButtonInteraction<SoundButton>,
    pub dragging: Option<SoundSlider>,
    pub hovered: Option<SoundControl>,
    pub scroll: super::list::ListScrollInteraction,
}
impl SoundState {
    pub fn new(positions: [u8; 3], rows: Vec<SoundTrackRow>, current: i32) -> Self {
        // 6B65CE..660B: retained-or-pending identity, otherwise initial row0;
        // LB_SETCURSEL followed by LB_SETTOPINDEX. The list owns top clamping.
        let selected = rows
            .iter()
            .position(|row| row.theme_index == current)
            .or_else(|| (!rows.is_empty()).then_some(0));
        let top = selected.unwrap_or(0).min(rows.len().saturating_sub(8));
        Self {
            positions,
            rows,
            selected,
            top,
            buttons: Default::default(),
            dragging: None,
            hovered: None,
            scroll: Default::default(),
        }
    }
    pub fn selected_theme(&self) -> Option<i32> {
        self.selected
            .and_then(|i| self.rows.get(i))
            .map(|row| row.theme_index)
    }
    pub fn reset_interaction(&mut self) {
        self.buttons = Default::default();
        self.dragging = None;
        self.hovered = None;
        self.scroll.cancel();
    }
    pub fn thumb_left(&self, id: SoundSlider, rect: RectPx) -> i32 {
        thumb_left(i32::from(self.positions[id as usize]), rect.w, 50, 10)
    }
    pub fn set_from_pointer(&mut self, id: SoundSlider, rect: RectPx, x: i32) -> Option<f32> {
        // The range is 10, so the position fits a byte.
        let pos = trackbar_position_from_x(x - rect.x, rect.w, 50, 10) as u8;
        let stored = &mut self.positions[id as usize];
        if *stored == pos {
            return None;
        }
        *stored = pos;
        // Original HSCROLL uses double0.1 then stores f32 through each setter.
        Some((f64::from(pos) * 0.1) as f32)
    }
    /// A press on a slider (`0x61E505..0x61E594`): only the strip's bottom
    /// rows admit it; the thumb starts a drag, the rail jumps once. Returns
    /// the changed volume.
    pub fn press_slider(&mut self, id: SoundSlider, rect: RectPx, x: i32, y: i32) -> Option<f32> {
        if !crate::ui::shell::trackbar::admits_press_y(y - rect.y, rect.h) {
            return None;
        }
        let left = rect.x + self.thumb_left(id, rect);
        if (left..left + 12).contains(&x) {
            self.dragging = Some(id);
            None
        } else {
            self.set_from_pointer(id, rect, x)
        }
    }
}

/// The shell's Sound Options `0xD6`: `0x6B6230` creates it instead of B8 while
/// `g_GameActive [0xA8E9A0]` is 0, as WOL Options `0x2BC` sets it. A family
/// page (`0x0060C540`) with the three volume sliders and Back only; proc
/// `0x6B6300` skips the Shuffle/Repeat/list init there (`0x6B6485`), and the
/// executed `0x006040B0` table has no help rows for it.
pub const SHELL_SOUND_PAGE: super::menu_page::MenuPageSpec = super::menu_page::MenuPageSpec {
    dialog: super::descriptor::DialogId(0x00D6),
    title_key: "GUI:SoundOptions",
    stacked: &[],
    back: super::menu_page::MenuPageButtonSpec {
        id: 0x0686,
        dlu_top: 257,
        csf_key: "GUI:Back",
        tooltip_key: "GUI:Blank",
        result: Some(1),
    },
};

/// The slider captions `GUI:MusicVolume`, `GUI:SoundVolume`, `GUI:VoiceVolume`.
pub const SHELL_SOUND_LABEL_KEYS: [&str; 3] =
    ["GUI:MusicVolume", "GUI:SoundVolume", "GUI:VoiceVolume"];

/// What one of `0xD6`'s widget events asks its host to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShellSoundInput {
    /// Back took the press: GUIMainButtonSound (`0x0061376B`). The rail
    /// press plays nothing (`0x006B6382`/`0x006B63EA`/`0x006B6444`).
    BackPressed,
    /// Back released over itself: the positions it re-applies.
    Back([u8; 3]),
    /// A trackbar moved (`WM_HSCROLL`): the volume it applies live.
    Volume(SoundSlider, f32),
}

/// The open `0xD6`: the three positions Back re-applies, and its widgets in
/// the dialog's GUI tree.
#[derive(Debug, Clone)]
pub struct ShellSoundState {
    pub positions: [u8; 3],
    /// `0x6B6300`'s init enables the three trackbars only with an output
    /// device (`Audio__HasOutputDevice @ 0x00407000`, `0x006B635F..0x006B6483`);
    /// a disabled window takes no press. B8 never opens without one (BBB
    /// disables its Sound button, `0x004E2201..0x004E2223`), so only `0xD6`
    /// clears it.
    pub sliders_enabled: bool,
    nodes: Option<ShellSoundNodes>,
}

#[derive(Debug, Clone, Copy)]
struct ShellSoundNodes {
    back: NodeId,
    sliders: [NodeId; 3],
    labels: [NodeId; 3],
}

impl ShellSoundState {
    pub fn new(positions: [u8; 3], sliders_enabled: bool) -> Self {
        Self {
            positions,
            sliders_enabled,
            nodes: None,
        }
    }

    /// Puts the widgets under `root` (again, for a new dialog instance) and
    /// mirrors the layout and positions into them: Back, then each slider
    /// (range 0..10, the plaque on) under its right-aligned caption.
    pub fn sync_gui(&mut self, gui: &mut Gui, root: NodeId, layout: &ShellSoundLayout) {
        let built = self
            .nodes
            .is_some_and(|nodes| gui.tree().parent(nodes.back) == Ok(Some(root)));
        if !built {
            self.nodes = Self::build(gui, root);
        }
        let Some(nodes) = self.nodes else {
            return;
        };
        let rect = |rect: RectPx| Value::Rect(rect);
        let _ = gui.set(nodes.back, Prop::Rect, rect(layout.back()));
        for id in SoundSlider::ALL {
            let index = id as usize;
            let slider = nodes.sliders[index];
            let _ = gui.set(slider, Prop::Rect, rect(layout.sliders[index]));
            let _ = gui.set(slider, Prop::Pos, Value::Int(self.positions[index].into()));
            let _ = gui.set(slider, Prop::Enabled, Value::Bool(self.sliders_enabled));
            let _ = gui.set(nodes.labels[index], Prop::Rect, rect(layout.labels[index]));
        }
    }

    fn build(gui: &mut Gui, root: NodeId) -> Option<ShellSoundNodes> {
        let back = gui.create(NodeKind::Button, Some(root)).ok()?;
        let mut sliders = [back; 3];
        let mut labels = [back; 3];
        for index in 0..3 {
            let slider = gui.create(NodeKind::Trackbar, Some(root)).ok()?;
            let _ = gui.set(slider, Prop::Min, Value::Int(0));
            let _ = gui.set(slider, Prop::Max, Value::Int(10));
            let label = gui.create(NodeKind::Text, Some(root)).ok()?;
            let key = SHELL_SOUND_LABEL_KEYS[index].to_owned();
            let _ = gui.set(label, Prop::Label, Value::Text(key));
            let align = crate::render::shell_text::ShellAlign::H_RIGHT.0;
            let _ = gui.set(label, Prop::Align, Value::Int(align.into()));
            (sliders[index], labels[index]) = (slider, label);
        }
        Some(ShellSoundNodes {
            back,
            sliders,
            labels,
        })
    }

    /// Back's widget.
    pub fn back(&self) -> Option<NodeId> {
        self.nodes.map(|nodes| nodes.back)
    }

    pub fn on_event(&mut self, event: &UiEvent) -> Option<ShellSoundInput> {
        let nodes = self.nodes?;
        match *event {
            UiEvent::Pressed { node } if node == nodes.back => Some(ShellSoundInput::BackPressed),
            UiEvent::Activated { node, .. } if node == nodes.back => {
                Some(ShellSoundInput::Back(self.positions))
            }
            // The release repeats the last position; only a move applies.
            UiEvent::Changed {
                node,
                value: Value::Int(pos),
                final_: false,
            } => {
                let index = nodes.sliders.iter().position(|n| *n == node)?;
                // The range is 10, so the position fits a byte.
                let pos = pos.clamp(0, 10) as u8;
                self.positions[index] = pos;
                // Original HSCROLL uses double 0.1 then stores f32 through
                // each setter.
                let volume = (f64::from(pos) * 0.1) as f32;
                Some(ShellSoundInput::Volume(SoundSlider::ALL[index], volume))
            }
            _ => None,
        }
    }
}

/// `0xD6` at the current screen. The sliders and labels keep their executed
/// places at every size (`0x0060C0C0`); the right panel follows the family.
#[derive(Debug, Clone)]
pub struct ShellSoundLayout {
    pub page: super::menu_page::MenuPageLayout,
    /// Status line `0x695`: the template is 303 DLU wide here.
    pub status_help: RectPx,
    pub sliders: [RectPx; 3],
    pub labels: [RectPx; 3],
}
impl ShellSoundLayout {
    pub fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, geom::ShellScale::RETAIL)
    }
    pub fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, geom::ShellScale::from_screen(width, height))
    }
    fn build(width: i32, height: i32, scale: geom::ShellScale) -> Self {
        use super::menu_page;
        let status_template = RectPx::new(2, 355, 303, 12);
        let (page, status_help) = if scale.is_retail() {
            (
                menu_page::compute_layout(&SHELL_SOUND_PAGE, width as u32, height as u32),
                super::layout::status_line_rect(status_template, width, height),
            )
        } else {
            (
                menu_page::compute_responsive_layout(
                    &SHELL_SOUND_PAGE,
                    width as u32,
                    height as u32,
                ),
                scale.scale_rect(super::layout::status_line_rect(
                    status_template,
                    geom::SHELL_BASE_W,
                    geom::SHELL_BASE_H,
                )),
            )
        };
        let px = |x, y, w, h| scale.scale_rect(geom::child_window(x, y, w, h));
        Self {
            page,
            status_help,
            sliders: [
                px(178, 141, 173, 13),
                px(178, 164, 173, 13),
                px(178, 186, 173, 13),
            ],
            labels: [
                px(84, 140, 87, 15),
                px(84, 163, 87, 15),
                px(84, 185, 87, 15),
            ],
        }
    }
    pub fn back(&self) -> RectPx {
        self.page.buttons[0].rect
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SoundLayout {
    pub title: RectPx,
    pub back: RectPx,
    pub footer: RectPx,
    pub sliders: [RectPx; 3],
    pub labels: [RectPx; 3],
    pub list: RectPx,
    pub play: RectPx,
    pub stop: RectPx,
    pub shuffle: RectPx,
    pub repeat: RectPx,
}
impl SoundLayout {
    pub fn new(width: i32, height: i32, shell: InGameShellLayout, button_size: [i32; 2]) -> Self {
        let dlu = |x, y, w, h| {
            in_game_shell::ordinary_child_rect(geom::dlu_rect(x, y, w, h), width, height)
        };
        Self {
            title: in_game_shell::right_panel_title_rect(width),
            back: shell.lower_button_rect(width, button_size),
            footer: in_game_shell::status_line_rect(height),
            sliders: [
                dlu(157, 60, 175, 13),
                dlu(157, 82, 175, 13),
                dlu(157, 104, 175, 13),
            ],
            labels: [
                dlu(61, 59, 90, 15),
                dlu(61, 81, 90, 15),
                dlu(61, 103, 90, 15),
            ],
            list: dlu(157, 129, 175, 99),
            play: dlu(81, 246, 83, 15),
            stop: dlu(249, 246, 83, 15),
            shuffle: dlu(81, 187, 70, 14),
            repeat: dlu(81, 214, 70, 14),
        }
    }
    pub fn button(self, id: SoundButton) -> RectPx {
        match id {
            SoundButton::Back => self.back,
            SoundButton::Play => self.play,
            SoundButton::Stop => self.stop,
        }
    }
    pub fn control_at(self, x: i32, y: i32) -> Option<SoundControl> {
        for id in [SoundButton::Back, SoundButton::Play, SoundButton::Stop] {
            if self.button(id).contains(x, y) {
                return Some(SoundControl::Button(id));
            }
        }
        for id in SoundSlider::ALL {
            if self.sliders[id as usize].contains(x, y) {
                return Some(SoundControl::Slider(id));
            }
        }
        for (rect, id) in [
            (self.shuffle, SoundControl::Shuffle),
            (self.repeat, SoundControl::Repeat),
            (self.list, SoundControl::List),
        ] {
            if rect.contains(x, y) {
                return Some(id);
            }
        }
        None
    }
}
#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    #[test]
    fn shell_0xd6_page_matches_the_executed_wol_relayout() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let rect = |v: &serde_json::Value| {
            let a: Vec<i32> = serde_json::from_value(v.clone()).unwrap();
            RectPx::new(a[0], a[1], a[2], a[3])
        };
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0xD6"]["sizes"].as_object().unwrap() {
            let (w, h) = size.split_once('x').unwrap();
            let layout = ShellSoundLayout::new(w.parse().unwrap(), h.parse().unwrap());
            for (key, child) in case["controls"].as_object().unwrap() {
                let actual = match key.as_str() {
                    "0x52F" => layout.sliders[0],
                    "0x532" => layout.sliders[1],
                    "0x536" => layout.sliders[2],
                    "0xFFFF" => layout.labels[0],
                    "0xFFFF#1" => layout.labels[1],
                    "0xFFFF#2" => layout.labels[2],
                    "0x686" => layout.back(),
                    "0x694" => layout.page.title,
                    "0x695" => layout.status_help,
                    "0x71C" => layout.page.warning_monitor,
                    other => panic!("{other}"),
                };
                assert_eq!(actual, rect(&child["rect"]), "{size} {key}");
                checked += 1;
            }
        }
        assert_eq!(checked, 30);
        // No 0x6040B0 rows for 0xD6.
        assert!(
            corpus["help"]["0xD6"]
                .as_object()
                .unwrap()
                .values()
                .all(serde_json::Value::is_null)
        );
    }
    #[test]
    fn numeric_263px_rails_match_original_pointer_and_thumb_projection() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/launcher_trackbar.json",
        ))
        .unwrap();
        let native = fixture["geometries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["width"] == 263)
            .unwrap();
        let rect = RectPx::new(236, 98, 263, 21);
        let mut state = SoundState::new([0; 3], vec![], -1);
        for sample in native["positions"].as_array().unwrap() {
            state.positions[0] = sample["position"].as_u64().unwrap() as u8;
            assert_eq!(
                state.thumb_left(SoundSlider::Music, rect),
                sample["thumb_left"].as_i64().unwrap() as i32
            );
        }
        for sample in native["pointers"].as_array().unwrap() {
            state.set_from_pointer(
                SoundSlider::Music,
                rect,
                rect.x + sample["x"].as_i64().unwrap() as i32,
            );
            assert_eq!(
                state.positions[0],
                sample["position"].as_u64().unwrap() as u8
            );
            assert_eq!(
                state.thumb_left(SoundSlider::Music, rect),
                sample["thumb_left"].as_i64().unwrap() as i32
            );
        }
    }
    #[test]
    fn original_b8_resource_children_match_executed_placement() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/sound_shell_layout.json",
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let child =
                &fixture["resource"]["controls"][case["resource_index"].as_u64().unwrap() as usize];
            let r: Vec<i32> = child["dlu_rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let actual = in_game_shell::ordinary_child_rect(
                geom::dlu_rect(r[0], r[1], r[2], r[3]),
                case["width"].as_i64().unwrap() as i32,
                case["height"].as_i64().unwrap() as i32,
            );
            assert_eq!(
                serde_json::json!([actual.x, actual.y, actual.w, actual.h]),
                case["rect"]
            );
        }
    }
    #[test]
    fn song_selection_keeps_catalog_identity_and_changed_slider_notification() {
        let rows = vec![
            SoundTrackRow {
                theme_index: 4,
                text: "first".into(),
            },
            SoundTrackRow {
                theme_index: 9,
                text: "second".into(),
            },
        ];
        let mut s = SoundState::new([5, 5, 5], rows, 9);
        assert_eq!(s.selected_theme(), Some(9));
        let r = RectPx::new(236, 98, 263, 21);
        assert_eq!(s.set_from_pointer(SoundSlider::Music, r, 1000), Some(1.0));
        assert_eq!(s.set_from_pointer(SoundSlider::Music, r, 1000), None);
        assert_eq!(s.positions, [10, 5, 5]);
    }
    #[test]
    fn shell_0xd6_disabled_sliders_take_no_press_and_a_rail_press_applies_once() {
        use vera20k_gui::{DialogKind, InputEvent, MouseButton};
        let layout = ShellSoundLayout::new(800, 600);
        let mut gui = Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        gui.push_dialog(root, DialogKind::Page, None, None).unwrap();
        let mut s = ShellSoundState::new([5, 5, 5], false);
        s.sync_gui(&mut gui, root, &layout);
        let r = layout.sliders[1];
        let press = |gui: &mut Gui, s: &mut ShellSoundState, x: i32| {
            let (y, button) = (r.y + r.h - 2, MouseButton::Left);
            gui.input(InputEvent::PointerDown { x, y, button }, 0);
            gui.input(InputEvent::PointerUp { x, y, button }, 0);
            let events = gui.drain_events();
            events
                .iter()
                .filter_map(|e| s.on_event(e))
                .collect::<Vec<_>>()
        };
        let thumb = r.x + crate::ui::shell::trackbar::thumb_left(5, r.w, 50, 10);
        for x in [r.x + 1, thumb] {
            assert_eq!(press(&mut gui, &mut s, x), []);
        }
        assert_eq!(s.positions, [5, 5, 5]);
        s.sliders_enabled = true;
        s.sync_gui(&mut gui, root, &layout);
        assert_eq!(
            press(&mut gui, &mut s, r.x + 1),
            [ShellSoundInput::Volume(SoundSlider::Sound, 0.0)]
        );
        assert_eq!(s.positions, [5, 0, 5]);
    }
}
