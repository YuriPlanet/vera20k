//! Shell trackbar rules shared by every `TrackBar` control: the launcher and
//! in-game Options sliders, the sound sliders and the campaign difficulty.
//! `TrackBar_ProcessMouse` `0x0061D950` owns the press, drag and paint
//! arithmetic below.

/// gamemd-derived: `TrackBar_ProcessMouse @ 0x0061D950` uses the literal
/// reserve/13/12/6 geometry and `(range + 1)` integer partition below.
pub fn trackbar_position_from_x(
    raw_mouse_x: i32,
    client_width: i32,
    plaque_reserve: i32,
    maximum: u8,
) -> u8 {
    let usable_span = (client_width - plaque_reserve - 13).max(1);
    let maximum_track_x = (client_width - plaque_reserve - 12).max(1);
    let track_x = (raw_mouse_x - 6).clamp(1, maximum_track_x);
    let relative = ((track_x - 1) * (i32::from(maximum) + 1)) / usable_span;
    relative.min(i32::from(maximum)) as u8
}

/// What a left press does on a shell trackbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackbarPress {
    /// Outside the admitted strip: nothing happens.
    Ignored,
    /// On the thumb: capture it for dragging, without moving it.
    Capture,
    /// Beside the thumb: jump once to this position, without capture.
    Jump(u8),
}

/// gamemd-derived: the initial down row of `TrackBar_ProcessMouse @
/// 0x0061D950` requires `y > bottom - 18`; thumb-down captures without a
/// jump, while a rail down jumps once and does not capture.
pub fn trackbar_press(
    position: u8,
    local_x: i32,
    local_y: i32,
    width: i32,
    height: i32,
    reserve: i32,
    maximum: u8,
) -> TrackbarPress {
    if local_x < 0 || local_x >= width || local_y <= height - 18 || local_y >= height {
        return TrackbarPress::Ignored;
    }
    let left = thumb_left(position, width, reserve, maximum);
    if (left..left + 12).contains(&local_x) {
        return TrackbarPress::Capture;
    }
    TrackbarPress::Jump(trackbar_position_from_x(local_x, width, reserve, maximum))
}

pub fn thumb_left(position: u8, client_width: i32, reserve: i32, maximum: u8) -> i32 {
    let usable_span = (client_width - reserve - 13).max(1);
    // Original 0x0061E486..0x0061E4A8 (TBM_SETPOS) and
    // 0x0061DC44..0x0061DC58 (drag) divide the painted offset by range.
    // Mouse quantization above deliberately uses range + 1 instead.
    1 + i32::from(position) * usable_span / i32::from(maximum).max(1)
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
        let integer = |value: &serde_json::Value, key: &str| value[key].as_i64().unwrap() as i32;
        let mut position_count = 0;
        let mut pointer_count = 0;
        for geometry in geometries {
            let width = integer(geometry, "width");
            let reserve = integer(geometry, "reserve");
            let maximum = integer(geometry, "maximum") as u8;
            for sample in geometry["positions"].as_array().unwrap() {
                let position = integer(sample, "position") as u8;
                let left = thumb_left(position, width, reserve, maximum);
                assert_eq!(
                    left,
                    integer(sample, "thumb_left"),
                    "{geometry:?} {sample:?}"
                );
                assert_eq!(left + 12, integer(sample, "thumb_right"));
                position_count += 1;
            }
            for sample in geometry["pointers"].as_array().unwrap() {
                let x = integer(sample, "x");
                let position = trackbar_position_from_x(x, width, reserve, maximum);
                assert_eq!(
                    i32::from(position),
                    integer(sample, "position"),
                    "width={width} reserve={reserve} maximum={maximum} x={x}"
                );
                let left = thumb_left(position, width, reserve, maximum);
                assert_eq!(left, integer(sample, "thumb_left"));
                assert_eq!(left + 12, integer(sample, "thumb_right"));
                pointer_count += 1;
            }
        }
        assert_eq!(position_count, 92);
        assert_eq!(pointer_count, 2736);
    }
}
