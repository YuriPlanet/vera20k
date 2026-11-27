//! Active Sound B8: original6B6230/6B6300, resourceBEFE4C.
//! Values here are control projections; the process Options/Theme owners retain
//! audio values, playback and playlist identity.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use super::controller::DialogController;
use super::descriptor::DialogResourceId;
use super::geom::{self, RectPx};
use super::in_game_shell::{self, InGameShellLayout};

use crate::ui::shell::geom::ShellAlign;

/// Active B8 (resource `0xBEFE4C`).
pub const DIALOG: DialogResourceId = DialogResourceId(0xB8);

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
/// B8's children with status help, in the order the pointer finds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundControl {
    Button(SoundButton),
    Slider(SoundSlider),
    Shuffle,
    Repeat,
    List,
}

impl SoundControl {
    pub const ALL: [Self; 9] = [
        Self::Button(SoundButton::Back),
        Self::Button(SoundButton::Play),
        Self::Button(SoundButton::Stop),
        Self::Slider(SoundSlider::Music),
        Self::Slider(SoundSlider::Sound),
        Self::Slider(SoundSlider::Voice),
        Self::Shuffle,
        Self::Repeat,
        Self::List,
    ];

    /// The resource's control ids.
    pub const fn id(self) -> u16 {
        match self {
            Self::Button(SoundButton::Back) => 0x686,
            Self::Button(SoundButton::Play) => 0x531,
            Self::Button(SoundButton::Stop) => 0x535,
            Self::Slider(slider) => SHELL_SOUND_SLIDERS[slider as usize],
            Self::Shuffle => 0x533,
            Self::Repeat => 0x534,
            Self::List => 0x530,
        }
    }

    pub fn from_id(id: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|control| control.id() == id)
    }

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

/// B8's track rows and its controls, which hold the sliders' positions and
/// the list's selection and top.
#[derive(Debug)]
pub struct SoundState {
    pub rows: Vec<SoundTrackRow>,
    pub ui: DialogController,
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
        let row = |row: Option<usize>| Value::Int(row.map_or(-1, |row| row as i32));
        let controls = SoundControl::ALL.map(|control| {
            let node = match control {
                SoundControl::Button(_) => NodeSpec::new(NodeKind::Button),
                SoundControl::Slider(slider) => trackbar(slider, positions, true),
                // The proc toggles the box on the press of its icon.
                SoundControl::Shuffle | SoundControl::Repeat => NodeSpec::new(NodeKind::Checkbox),
                // Every press on a row selects it (no double-click case);
                // the rows draw unaligned, 2 px in.
                SoundControl::List => NodeSpec::new(NodeKind::List)
                    .with(
                        Prop::Items,
                        Value::Items(rows.iter().map(|row| row.text.clone()).collect()),
                    )
                    .with(Prop::Align, Value::Int(i32::from(ShellAlign::NONE.0)))
                    .with(Prop::Selected, row(selected))
                    .with(Prop::Top, row(Some(top))),
            };
            node.id(control.id().into()).tooltip(control.help().0)
        });
        // The slider captions, title and status line are statics with no
        // help.
        let labels = [0, 1, 2].map(slider_label);
        let mut ui = DialogController::default();
        ui.reset_to(DIALOG);
        if let Some(root) = ui.root_in(DIALOG) {
            super::page::ensure_built(ui.gui_mut(), root, || {
                NodeSpec::new(NodeKind::Dialog)
                    .children(controls)
                    .children(labels)
                    .children(in_game_shell::panel_statics())
            });
        }
        Self { rows, ui }
    }

    fn root(&self) -> Option<NodeId> {
        self.ui.root_in(DIALOG)
    }

    /// The sliders' positions, which Back re-applies.
    pub fn positions(&self) -> [u8; 3] {
        self.root()
            .map_or([0; 3], |root| shell_sound_positions(self.ui.gui(), root))
    }

    pub fn selected_theme(&self) -> Option<i32> {
        let root = self.root()?;
        let row = super::page::int(self.ui.gui(), root, SoundControl::List.id(), Prop::Selected)?;
        self.rows
            .get(usize::try_from(row).ok()?)
            .map(|row| row.theme_index)
    }
}

/// B8's controls in paint order (`gui_paint`): the trackbars, the boxes,
/// the list, Play and Stop, then the slider captions.
pub const PAINT_ORDER: [u16; 11] = [
    SHELL_SOUND_SLIDERS[0],
    SHELL_SOUND_SLIDERS[1],
    SHELL_SOUND_SLIDERS[2],
    SoundControl::Shuffle.id(),
    SoundControl::Repeat.id(),
    SoundControl::List.id(),
    SoundControl::Button(SoundButton::Play).id(),
    SoundControl::Button(SoundButton::Stop).id(),
    SHELL_SOUND_LABELS[0],
    SHELL_SOUND_LABELS[1],
    SHELL_SOUND_LABELS[2],
];

/// B8's captions by control id: Play, Stop, the boxes and the slider
/// captions (key and fallback). The rest have none.
pub fn caption(id: u16) -> Option<(&'static str, &'static str)> {
    const FALLBACKS: [&str; 3] = ["Music Volume", "Sound Volume", "Voice Volume"];
    if let Some(index) = SHELL_SOUND_LABELS.iter().position(|label| *label == id) {
        return Some((SHELL_SOUND_LABEL_KEYS[index], FALLBACKS[index]));
    }
    Some(match SoundControl::from_id(id)? {
        SoundControl::Button(SoundButton::Play) => ("GUI:Play", "Play"),
        SoundControl::Button(SoundButton::Stop) => ("GUI:Stop", "Stop"),
        SoundControl::Shuffle => ("GUI:Shuffle", "Shuffle"),
        SoundControl::Repeat => ("GUI:Repeat", "Repeat"),
        _ => return None,
    })
}

/// Slider `index`'s right-aligned caption static.
fn slider_label(index: usize) -> NodeSpec {
    let align = i32::from(ShellAlign::H_RIGHT.0);
    NodeSpec::new(NodeKind::Text)
        .id(SHELL_SOUND_LABELS[index].into())
        .label(SHELL_SOUND_LABEL_KEYS[index])
        .with(Prop::Align, Value::Int(align))
}

/// The horizontal volume row for online options.
pub(crate) fn launcher_volume_rects(slider: SoundSlider) -> (RectPx, RectPx) {
    let x = 124 + slider as i32 * 154;
    (RectPx::new(x, 482, 129, 17), RectPx::new(x, 503, 129, 22))
}

/// Slider `slider` at `positions`, range 0..10 with its value plaque.
pub(crate) fn trackbar(slider: SoundSlider, positions: [u8; 3], enabled: bool) -> NodeSpec {
    let index = slider as usize;
    NodeSpec::new(NodeKind::Trackbar)
        .id(SHELL_SOUND_SLIDERS[index].into())
        .with(Prop::Min, Value::Int(0))
        .with(Prop::Max, Value::Int(10))
        .with(Prop::Pos, Value::Int(positions[index].into()))
        .with(Prop::Enabled, Value::Bool(enabled))
}

/// The shell's Sound Options `0xD6`: `0x6B6230` creates it instead of B8 while
/// `g_GameActive [0xA8E9A0]` is 0, as WOL Options `0x2BC` sets it. A family
/// page (`0x0060C540`) with the three volume sliders and Back only; proc
/// `0x6B6300` skips the Shuffle/Repeat/list init there (`0x6B6485`), and the
/// executed `0x006040B0` table has no help rows for it.
pub const SHELL_SOUND_PAGE: super::menu_page::MenuPageSpec = super::menu_page::MenuPageSpec {
    dialog: super::descriptor::DialogResourceId(0x00D6),
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
/// `0xD6`'s trackbars `0x52F`, `0x532` and `0x536`, in [`SoundSlider::ALL`]'s
/// order.
pub const SHELL_SOUND_SLIDERS: [u16; 3] = [0x052F, 0x0532, 0x0536];
/// The captions are statics with no control id (`0xFFFF`); these stand for
/// them in the page's tree.
pub(crate) const SHELL_SOUND_LABELS: [u16; 3] = [0x7E50, 0x7E51, 0x7E52];

/// The open `0xD6`'s widgets: Back, then each slider (range 0..10, the plaque
/// on) at `positions` under its right-aligned caption. `0x6B6300`'s init
/// enables the three trackbars only with an output device
/// (`Audio__HasOutputDevice @ 0x00407000`, `0x006B635F..0x006B6483`); a
/// disabled window takes no press. B8 never opens without one (BBB disables
/// its Sound button, `0x004E2201..0x004E2223`), so only `0xD6` clears it.
pub fn shell_sound_spec(positions: [u8; 3], sliders_enabled: bool) -> NodeSpec {
    let rows = SoundSlider::ALL.map(|id| {
        [
            trackbar(id, positions, sliders_enabled),
            slider_label(id as usize),
        ]
    });
    NodeSpec::new(NodeKind::Dialog)
        .child(NodeSpec::new(NodeKind::Button).id(SHELL_SOUND_PAGE.back.id.into()))
        .children(rows.into_iter().flatten())
}

/// The three positions the trackbars stand at, which Back re-applies.
pub fn shell_sound_positions(gui: &Gui, root: NodeId) -> [u8; 3] {
    // The range is 10, so a position fits a byte.
    SHELL_SOUND_SLIDERS.map(|id| {
        super::page::int(gui, root, id, Prop::Pos)
            .unwrap_or(0)
            .clamp(0, 10) as u8
    })
}

/// A trackbar's move (`WM_HSCROLL`): the volume it applies live. The release
/// repeats the last position; only a move applies.
pub fn shell_sound_volume(event: &super::page::PageEvent) -> Option<(SoundSlider, f32)> {
    let super::page::PageEvent::Slid {
        id,
        value,
        final_: false,
    } = *event
    else {
        return None;
    };
    let index = SHELL_SOUND_SLIDERS
        .iter()
        .position(|slider| *slider == id)?;
    // Original HSCROLL uses double 0.1 then stores f32 through each setter.
    let volume = (f64::from(value.clamp(0, 10)) * 0.1) as f32;
    Some((SoundSlider::ALL[index], volume))
}

/// `0xD6` at the current screen. The sliders and labels keep their executed
/// places at every size (`0x0060C0C0`); the right panel follows the family.
#[derive(Debug, Clone)]
pub struct ShellSoundLayout {
    pub page: super::menu_page::MenuPageLayout,
    pub sliders: [RectPx; 3],
    pub labels: [RectPx; 3],
}
impl ShellSoundLayout {
    pub fn new(width: i32, height: i32, scale: geom::ShellScale) -> Self {
        let (page, rects) =
            super::layout_data::menu_page(&SHELL_SOUND_PAGE, "0xD6", scale, width, height);
        let rect = |id| rects.get(id).unwrap_or_default();
        Self {
            page,
            sliders: SHELL_SOUND_SLIDERS.map(rect),
            labels: SHELL_SOUND_LABELS.map(rect),
        }
    }
    pub fn back(&self) -> RectPx {
        self.page.buttons[0].rect
    }

    /// Control `id`'s rect.
    pub fn rect(&self, id: u16) -> Option<RectPx> {
        if id == SHELL_SOUND_PAGE.back.id {
            return Some(self.back());
        }
        let slider = SHELL_SOUND_SLIDERS.iter().position(|slider| *slider == id);
        let label = SHELL_SOUND_LABELS.iter().position(|label| *label == id);
        slider
            .map(|index| self.sliders[index])
            .or_else(|| label.map(|index| self.labels[index]))
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
        let rects = super::layout_data::in_game("0xB8", &shell, width, height, button_size);
        let rect = |control: SoundControl| rects.get(control.id()).unwrap_or_default();
        Self {
            title: in_game_shell::right_panel_title_rect(width),
            back: rect(SoundControl::Button(SoundButton::Back)),
            footer: in_game_shell::status_line_rect(height),
            sliders: SoundSlider::ALL.map(|slider| rect(SoundControl::Slider(slider))),
            labels: SHELL_SOUND_LABELS.map(|id| rects.get(id).unwrap_or_default()),
            list: rect(SoundControl::List),
            play: rect(SoundControl::Button(SoundButton::Play)),
            stop: rect(SoundControl::Button(SoundButton::Stop)),
            shuffle: rect(SoundControl::Shuffle),
            repeat: rect(SoundControl::Repeat),
        }
    }
    pub fn button(self, id: SoundButton) -> RectPx {
        match id {
            SoundButton::Back => self.back,
            SoundButton::Play => self.play,
            SoundButton::Stop => self.stop,
        }
    }

    /// Control `id`'s rect, a slider caption's, the title's and the status
    /// line's included.
    pub fn rect(self, id: u16) -> Option<RectPx> {
        if let Some(index) = SHELL_SOUND_LABELS.iter().position(|label| *label == id) {
            return Some(self.labels[index]);
        }
        match id {
            in_game_shell::TITLE => return Some(self.title),
            in_game_shell::FOOTER => return Some(self.footer),
            _ => {}
        }
        Some(match SoundControl::from_id(id)? {
            SoundControl::Button(button) => self.button(button),
            SoundControl::Slider(slider) => self.sliders[slider as usize],
            SoundControl::Shuffle => self.shuffle,
            SoundControl::Repeat => self.repeat,
            SoundControl::List => self.list,
        })
    }
}
#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    /// Slider `id`'s painted thumb offset in `rect`.
    fn thumb_left(state: &SoundState, id: SoundSlider, rect: RectPx) -> i32 {
        let position = i32::from(state.positions()[id as usize]);
        vera20k_gui::shell::trackbar::TrackbarRange::new(0, 10, 1).thumb_left(position, rect.w)
    }
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
            let layout =
                ShellSoundLayout::new(w.parse().unwrap(), h.parse().unwrap(), geom::RETAIL_SCALE);
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
                    "0x695" => layout.page.status_help,
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
        let root = state.root().unwrap();
        let set = |state: &mut SoundState, position: i32| {
            let id = SHELL_SOUND_SLIDERS[0];
            super::super::page::set(
                state.ui.gui_mut(),
                root,
                id,
                Prop::Pos,
                Value::Int(position),
            );
        };
        for sample in native["positions"].as_array().unwrap() {
            set(&mut state, sample["position"].as_i64().unwrap() as i32);
            assert_eq!(
                thumb_left(&state, SoundSlider::Music, rect),
                sample["thumb_left"].as_i64().unwrap() as i32
            );
        }
        // The trackbar's drag (`0x0061DC44`) takes the pointer's position.
        let range = vera20k_gui::shell::trackbar::TrackbarRange::new(0, 10, 1);
        for sample in native["pointers"].as_array().unwrap() {
            let x = rect.x + sample["x"].as_i64().unwrap() as i32;
            set(&mut state, range.value_at_in(x, rect, geom::RETAIL_SCALE));
            assert_eq!(
                state.positions()[0],
                sample["position"].as_u64().unwrap() as u8
            );
            assert_eq!(
                thumb_left(&state, SoundSlider::Music, rect),
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
        let root = s.root().unwrap();
        super::super::page::sync_rects(s.ui.gui_mut(), root, |id| {
            (id == SHELL_SOUND_SLIDERS[0]).then_some(r)
        });
        let y = r.y + r.h - 2;
        let send = |s: &mut SoundState, event| {
            s.ui.on_input(event);
            let events = s.ui.drain_events();
            super::super::page::page_events(s.ui.gui(), events)
                .iter()
                .filter_map(shell_sound_volume)
                .collect::<Vec<_>>()
        };
        use vera20k_gui::{InputEvent, MouseButton};
        let thumb = r.x + thumb_left(&s, SoundSlider::Music, r) + 1;
        let button = MouseButton::Left;
        assert_eq!(
            send(
                &mut s,
                InputEvent::PointerDown {
                    x: thumb,
                    y,
                    button
                }
            ),
            []
        );
        let drag = InputEvent::PointerMove { x: 1000, y };
        assert_eq!(send(&mut s, drag.clone()), [(SoundSlider::Music, 1.0)]);
        assert_eq!(send(&mut s, drag), []);
        assert_eq!(s.positions(), [10, 5, 5]);
    }
    #[test]
    fn shell_0xd6_disabled_sliders_take_no_press_and_a_rail_press_applies_once() {
        use super::super::page;
        use vera20k_gui::{DialogKind, InputEvent, MouseButton};
        let layout = ShellSoundLayout::new(800, 600, geom::RETAIL_SCALE);
        let mut gui = Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        gui.push_dialog(root, DialogKind::Page).unwrap();
        page::ensure_built(&mut gui, root, || shell_sound_spec([5, 5, 5], false));
        page::sync_rects(&mut gui, root, |id| layout.rect(id));
        let r = layout.sliders[1];
        let press = |gui: &mut Gui, x: i32| {
            let (y, button) = (r.y + r.h - 2, MouseButton::Left);
            gui.input(InputEvent::PointerDown { x, y, button }, 0);
            gui.input(InputEvent::PointerUp { x, y, button }, 0);
            let events = gui.drain_events();
            page::page_events(gui, events)
                .iter()
                .filter_map(shell_sound_volume)
                .collect::<Vec<_>>()
        };
        let thumb = r.x + vera20k_gui::shell::trackbar::TrackbarRange::new(0, 10, 1).thumb_left(5, r.w);
        for x in [r.x + 1, thumb] {
            assert_eq!(press(&mut gui, x), []);
        }
        assert_eq!(shell_sound_positions(&gui, root), [5, 5, 5]);
        for id in SHELL_SOUND_SLIDERS {
            page::set(&mut gui, root, id, Prop::Enabled, Value::Bool(true));
        }
        assert_eq!(press(&mut gui, r.x + 1), [(SoundSlider::Sound, 0.0)]);
        assert_eq!(shell_sound_positions(&gui, root), [5, 0, 5]);
    }
}
