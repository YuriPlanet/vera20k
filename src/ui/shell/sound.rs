//! Active Sound B8: original6B6230/6B6300, resourceBEFE4C.
//! Values here are control projections; the process Options/Theme owners retain
//! audio values, playback and playlist identity.

use super::button::ShellButtonInteraction;
use super::geom::{self, RectPx};
use super::in_game_shell::InGameShellLayout;
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
        let dlu = |x, y, w, h| ordinary(geom::dlu_rect(x, y, w, h), width, height);
        Self {
            title: RectPx::new(width - 165, 2, 162, 16),
            back: RectPx::new(
                width - 147,
                shell.side3.y - button_size[1],
                button_size[0],
                button_size[1],
            ),
            footer: RectPx::new(10, height - 21, 455, 20),
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
fn ordinary(raw: RectPx, width: i32, height: i32) -> RectPx {
    RectPx::new(
        (raw.x + (width - 800) / 2).max(0),
        (raw.y + (height - 600) / 2).max(0),
        raw.w,
        raw.h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_263px_rails_match_original_pointer_and_thumb_projection() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/launcher_trackbar.json"
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
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/sound_shell_layout.json"
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
            let actual = ordinary(
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
}
