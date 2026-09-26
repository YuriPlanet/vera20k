//! Shell trackbar rules shared by every `msctls_trackbar32` control the shell
//! subclasses: Skirmish, Generate Map, the launcher and in-game Options
//! sliders, the sound sliders and the campaign difficulty.
//! `ShellTrackbar__WndProc` `0x0061D950` owns the press, drag and paint
//! arithmetic below. It keeps the position relative to the range minimum
//! (`TBM_SETRANGE` `0x406`); `range` below is maximum minus minimum.

use super::geom::RectPx;

/// Width the value plaque takes at the window's right end (`0x32`), unless
/// the owner turns the plaque off with `0x4AC`.
pub const PLAQUE_RESERVE: i32 = 50;
/// The thumb's width in the paint and the press test (`+0xC`).
pub const THUMB_W: i32 = 12;

/// gamemd-derived: `ShellTrackbar__WndProc @ 0x0061D950` uses the literal
/// reserve/13/12/6 geometry and `(range + 1)` integer partition below.
pub fn trackbar_position_from_x(
    raw_mouse_x: i32,
    client_width: i32,
    plaque_reserve: i32,
    range: i32,
) -> i32 {
    let usable_span = (client_width - plaque_reserve - 13).max(1);
    let maximum_track_x = (client_width - plaque_reserve - 12).max(1);
    let track_x = (raw_mouse_x - 6).clamp(1, maximum_track_x);
    let relative = ((track_x - 1) * (range + 1)) / usable_span;
    relative.min(range)
}

/// What a left press does on a shell trackbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackbarPress {
    /// Off the window.
    Miss,
    /// On the window above the admitted strip: the press only takes the
    /// capture.
    Hold,
    /// On the thumb: capture it for dragging, without moving it.
    Drag,
    /// Beside the thumb: jump once to this position; later moves do not drag.
    Jump(i32),
}

impl TrackbarPress {
    /// The hold this press starts on trackbar `id`, unless it misses the
    /// window.
    pub fn hold<Id>(self, id: Id) -> Option<TrackbarHold<Id>> {
        match self {
            TrackbarPress::Miss => None,
            TrackbarPress::Drag => Some(TrackbarHold { id, dragging: true }),
            TrackbarPress::Hold | TrackbarPress::Jump(_) => Some(TrackbarHold {
                id,
                dragging: false,
            }),
        }
    }
}

/// A trackbar holding the mouse. Every press on the window takes the
/// capture (`SetCapture` at `0x0061E4DF`) until the button's release, or a
/// move without it (`ReleaseCapture` at `0x0061E44E`); until then neither the
/// dialog's status-help hit test (`WM_NCHITTEST`, `0x00622CCB`) nor another
/// control sees the pointer. Only a press on the thumb drags (`+0xEC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackbarHold<Id = ()> {
    pub id: Id,
    pub dragging: bool,
}

/// gamemd-derived: the `WM_LBUTTONDOWN` path of `ShellTrackbar__WndProc @
/// 0x0061D950` (`0x0061E4CE..0x0061E598`): it takes the capture, then admits
/// only `y > bottom - 18`; there a press on the thumb starts a drag without a
/// jump, and a press beside it jumps once.
pub fn trackbar_press(
    position: i32,
    local_x: i32,
    local_y: i32,
    width: i32,
    height: i32,
    reserve: i32,
    range: i32,
) -> TrackbarPress {
    if !(0..width).contains(&local_x) || !(0..height).contains(&local_y) {
        return TrackbarPress::Miss;
    }
    if !admits_press_y(local_y, height) {
        return TrackbarPress::Hold;
    }
    let left = thumb_left(position, width, reserve, range);
    if (left..left + THUMB_W).contains(&local_x) {
        return TrackbarPress::Drag;
    }
    TrackbarPress::Jump(trackbar_position_from_x(local_x, width, reserve, range))
}

/// Whether a press at window-relative `local_y` lands in the strip the
/// trackbar admits: `y > bottom - 18` (`0x0061E4F5..0x0061E512`).
pub const fn admits_press_y(local_y: i32, height: i32) -> bool {
    local_y > height - 18 && local_y < height
}

pub fn thumb_left(position: i32, client_width: i32, reserve: i32, range: i32) -> i32 {
    let usable_span = (client_width - reserve - 13).max(1);
    // Original 0x0061E486..0x0061E4A8 (TBM_SETPOS) and
    // 0x0061DC44..0x0061DC58 (drag) divide the painted offset by range.
    // Mouse quantization above deliberately uses range + 1 instead.
    1 + position * usable_span / range.max(1)
}

/// A plaque trackbar's range as its owner sets it: `TBM_SETRANGE` (`0x406`)
/// and the step (`0x4AB`). Values are absolute; the control keeps the
/// position `value - min`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackbarRange {
    pub min: i32,
    pub max: i32,
    pub step: i32,
}

impl TrackbarRange {
    /// A zero step reads as 1: every message replaces it before any use
    /// (`0x0061DB94..0x0061DBAD`).
    pub const fn new(min: i32, max: i32, step: i32) -> Self {
        let step = if step == 0 { 1 } else { step };
        Self { min, max, step }
    }

    /// What the owner reads back from a window holding `value` (`TBM_GETPOS`,
    /// `0x0061E4AD`) and the value text shows (`0x0061E27A`): `value` rounded
    /// down to a multiple of the step.
    pub const fn stepped(self, value: i32) -> i32 {
        value / self.step * self.step
    }

    /// The value a fresh window holds once its owner's setup has sent
    /// `TBM_SETRANGE` and then `TBM_SETPOS value`. `TBM_SETPOS` ignores a
    /// value outside the range (`0x0061E48F`), which leaves what
    /// `TBM_SETRANGE` made of the fresh position 0: `min(pos, range)`, then
    /// `max(pos, min)` against the absolute minimum (`0x0061E5AE`).
    pub const fn set_up(self, value: i32) -> i32 {
        if value >= self.min && value <= self.max {
            return value;
        }
        let range = self.max - self.min;
        let position = if range < 0 { range } else { 0 };
        let position = if position < self.min {
            self.min
        } else {
            position
        };
        self.min + position
    }

    /// The thumb's left edge in a `width`-wide window showing `value`.
    pub fn thumb_left(self, value: i32, width: i32) -> i32 {
        thumb_left(value - self.min, width, PLAQUE_RESERVE, self.max - self.min)
    }

    /// The value a captured pointer at window-relative `x` selects.
    pub fn value_at(self, x: i32, width: i32) -> i32 {
        let position = trackbar_position_from_x(x, width, PLAQUE_RESERVE, self.max - self.min);
        self.stepped(self.min + position)
    }

    /// What a press at window-relative `(x, y)` does on a window showing
    /// `value`; a jump carries the new value.
    pub fn press(self, value: i32, x: i32, y: i32, width: i32, height: i32) -> TrackbarPress {
        let range = self.max - self.min;
        match trackbar_press(value - self.min, x, y, width, height, PLAQUE_RESERVE, range) {
            TrackbarPress::Jump(position) => TrackbarPress::Jump(self.stepped(self.min + position)),
            other => other,
        }
    }
}

/// The value text's rect on a window with the plaque: its last `0x31`
/// columns, full height (`0x0061E2B6..0x0061E2DB`).
pub const fn value_text_rect(window: RectPx) -> RectPx {
    RectPx::new(window.x + window.w - 0x31, window.y, 0x31, window.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trackbar_formula_matches_all_canonical_d5_thresholds() {
        let cases = [
            (180, 0, 1, vec![(90, 0), (91, 1)]),
            (180, 0, 2, vec![(62, 0), (63, 1), (118, 1), (119, 2)]),
            (
                180,
                0,
                6,
                vec![
                    (30, 0),
                    (31, 1),
                    (55, 2),
                    (79, 3),
                    (103, 4),
                    (127, 5),
                    (151, 6),
                ],
            ),
            (
                128,
                50,
                10,
                vec![
                    (12, 0),
                    (13, 1),
                    (19, 2),
                    (25, 3),
                    (31, 4),
                    (37, 5),
                    (43, 6),
                    (49, 7),
                    (55, 8),
                    (61, 9),
                    (67, 10),
                ],
            ),
        ];
        for (width, reserve, maximum, samples) in cases {
            for (x, expected) in samples {
                assert_eq!(
                    trackbar_position_from_x(x, width, reserve, maximum),
                    expected,
                    "width={width} reserve={reserve} x={x}"
                );
            }
        }
    }

    #[test]
    fn launcher_trackbar_position_and_painted_thumb_match_original_instruction_goldens() {
        // Reproduced by tools/storage_oracle/launcher_trackbar.py from the
        // hash-checked retail executable. This compares the separate native
        // range+1 pointer partition and range-only retained paint calculation.
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/launcher_trackbar.json"
        ))
        .unwrap();
        // BBB/B8 add geometries to the shared oracle; retain original D5 coverage.
        let geometries: Vec<_> = golden["geometries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|geometry| matches!(geometry["width"].as_i64(), Some(128 | 180)))
            .collect();
        assert_eq!(geometries.len(), 16);
        let (positions, pointers) = compare_with_goldens(geometries);
        assert_eq!(positions, 92);
        assert_eq!(pointers, 2736);
    }

    #[test]
    fn runtime_shell_windows_match_original_instruction_goldens() {
        // The runtime windows retail paints (Skirmish and the D5 audio
        // sliders 129, Generate Map 226, the D5 plain sliders 181) with their
        // owners' ranges; Credits runs 5000..10000 in steps of 100 (retail
        // Rules).
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/launcher_trackbar.json"
        ))
        .unwrap();
        let geometries: Vec<_> = golden["geometries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|geometry| matches!(geometry["width"].as_i64(), Some(129 | 181 | 226)))
            .collect();
        assert_eq!(geometries.len(), 7);
        let (positions, pointers) = compare_with_goldens(geometries);
        assert_eq!(positions, 93);
        assert_eq!(pointers, 1275);
    }

    #[test]
    fn every_press_on_the_window_holds_and_only_the_strip_moves() {
        // A 129x22 Skirmish window at position 0 of 50: the thumb spans 1..13.
        let press = |x, y| trackbar_press(0, x, y, 129, 22, PLAQUE_RESERVE, 50);
        for (x, y) in [(-1, 10), (129, 10), (40, -1), (40, 22)] {
            assert_eq!(press(x, y), TrackbarPress::Miss, "({x}, {y})");
        }
        for y in 0..=4 {
            assert_eq!(press(40, y), TrackbarPress::Hold, "y={y}");
        }
        assert_eq!(press(1, 5), TrackbarPress::Drag);
        assert_eq!(press(12, 21), TrackbarPress::Drag);
        assert_eq!(press(0, 5), TrackbarPress::Jump(0));
        assert_eq!(press(13, 5), TrackbarPress::Jump(4));
        assert_eq!(TrackbarPress::Miss.hold(7), None);
        for (kind, dragging) in [
            (TrackbarPress::Hold, false),
            (TrackbarPress::Drag, true),
            (TrackbarPress::Jump(3), false),
        ] {
            assert_eq!(kind.hold(7), Some(TrackbarHold { id: 7, dragging }));
        }
    }

    #[test]
    fn owner_setups_match_original_instruction_goldens() {
        // Fresh windows through `0x406`, `0x405` and `0x400`
        // (tools/storage_oracle/launcher_trackbar.py): saved values outside
        // the range, the stepped read-back, a zero step and a minimum off
        // the step.
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/launcher_trackbar.json"
        ))
        .unwrap();
        let setups = golden["setups"].as_array().unwrap();
        assert_eq!(setups.len(), 77);
        for setup in setups {
            let integer = |key: &str| setup[key].as_i64().unwrap() as i32;
            let range = TrackbarRange::new(integer("minimum"), integer("maximum"), integer("step"));
            let value = range.set_up(integer("value"));
            assert_eq!(value, range.min + integer("position"), "{setup}");
            assert_eq!(range.stepped(value), integer("read_back"), "{setup}");
        }
    }

    /// Compares the thumb projection and the pointer partition with the
    /// original-instruction samples; returns how many of each it compared.
    fn compare_with_goldens(geometries: Vec<&serde_json::Value>) -> (usize, usize) {
        let integer = |value: &serde_json::Value, key: &str| value[key].as_i64().unwrap() as i32;
        let mut position_count = 0;
        let mut pointer_count = 0;
        for geometry in geometries {
            let width = integer(geometry, "width");
            let reserve = integer(geometry, "reserve");
            let range = integer(geometry, "maximum");
            let minimum = geometry["minimum"].as_i64().unwrap_or(0) as i32;
            let step = geometry["step"].as_i64().unwrap_or(1) as i32;
            for sample in geometry["positions"].as_array().unwrap() {
                let position = integer(sample, "position");
                let left = thumb_left(position, width, reserve, range);
                assert_eq!(
                    left,
                    integer(sample, "thumb_left"),
                    "{geometry:?} {sample:?}"
                );
                assert_eq!(left + THUMB_W, integer(sample, "thumb_right"));
                position_count += 1;
            }
            for sample in geometry["pointers"].as_array().unwrap() {
                let x = integer(sample, "x");
                let raw = trackbar_position_from_x(x, width, reserve, range);
                let steps = TrackbarRange::new(minimum, minimum + range, step);
                let position = steps.stepped(minimum + raw) - minimum;
                assert_eq!(
                    position,
                    integer(sample, "position"),
                    "width={width} reserve={reserve} range={range} x={x}"
                );
                let left = thumb_left(position, width, reserve, range);
                assert_eq!(left, integer(sample, "thumb_left"));
                assert_eq!(left + THUMB_W, integer(sample, "thumb_right"));
                pointer_count += 1;
            }
        }
        (position_count, pointer_count)
    }
}
