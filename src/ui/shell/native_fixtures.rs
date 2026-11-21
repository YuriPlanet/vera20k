//! Native fixtures the shell's tests replay: gamemd's executed dialog
//! relayout and static timers, and the goldens the `vera20k-gui` widgets'
//! default metrics are checked against (`tools/storage_oracle/`). They live
//! here, not in the GUI crate, so that crate tests on its own.

use crate::test_fixture;
use crate::ui::shell::geom::RectPx;

/// Child `control` of `dialog` at `width`x`height` after the executed
/// WM_INITDIALOG relayout (`tools/storage_oracle/shell_relayout.py`).
pub fn executed_child_window(dialog: u16, control: u16, width: i32, height: i32) -> RectPx {
    executed_child_window_for_side(dialog, control, width, height, 0)
}

/// As [`executed_child_window`] for a case whose relayout depends on the
/// campaign side (`0x103`'s `0x72B` override).
pub fn executed_child_window_for_side(
    dialog: u16,
    control: u16,
    width: i32,
    height: i32,
    side: i32,
) -> RectPx {
    let fixture = test_fixture::json("tools/storage_oracle/shell_relayout.json");
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            case["dialog_id"] == dialog
                && case["width"] == width
                && case["height"] == height
                && case["side"] == side
        })
        .unwrap_or_else(|| panic!("no relayout of {dialog:#x} at {width}x{height} side {side}"));
    let child = case["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["id"] == control)
        .unwrap_or_else(|| panic!("{dialog:#x} has no child {control:#x}"));
    let rect: Vec<i32> = child["rect"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_i64().unwrap() as i32)
        .collect();
    RectPx::new(rect[0], rect[1], rect[2], rect[3])
}

/// Native getters executed under Unicorn by
/// `tools/storage_oracle/shell_static_timers.py`.
pub fn native_static_timer_cases() -> Vec<serde_json::Value> {
    let fixture = test_fixture::json("tools/storage_oracle/shell_static_timers.json");
    fixture["cases"].as_array().unwrap().clone()
}

/// The GUI crate's owner-draw button, list and trackbar arithmetic against
/// gamemd's executed instructions.
mod widget_goldens {
    use vera20k_gui::shell::button::owner_button_frame;
    use vera20k_gui::shell::list::{ListMetrics, thumb_height};
    use vera20k_gui::shell::trackbar::{
        THUMB_W, TrackbarRange, thumb_left, trackbar_position_from_x,
    };

    use crate::test_fixture;

    #[test]
    fn type2_type3_frame_selection_matches_original_owner_instructions() {
        let fixture = test_fixture::json("tools/storage_oracle/owner_button_frame.json");
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 4);
        for case in cases {
            assert_eq!(
                owner_button_frame(
                    case["pressed"].as_bool().unwrap(),
                    case["highlighted"].as_bool().unwrap()
                ),
                case["frame"].as_u64().unwrap() as usize
            );
        }
    }

    #[test]
    fn thumb_height_matches_native_sampled_geometry() {
        let vectors = test_fixture::json("tools/storage_oracle/saved_scrollbar.json");
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 450);
        for case in cases {
            assert_eq!(
                thumb_height(
                    case["height"].as_i64().unwrap() as i32,
                    case["range"].as_u64().unwrap() as usize,
                    ListMetrics::RETAIL,
                ),
                case["thumb"].as_i64().unwrap() as i32
            );
        }
    }

    #[test]
    fn launcher_trackbar_position_and_painted_thumb_match_original_instruction_goldens() {
        // Reproduced by tools/storage_oracle/launcher_trackbar.py from the
        // hash-checked retail executable. This compares the separate native
        // range+1 pointer partition and range-only retained paint calculation.
        let golden = test_fixture::json("tools/storage_oracle/launcher_trackbar.json");
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
        let golden = test_fixture::json("tools/storage_oracle/launcher_trackbar.json");
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
    fn owner_setups_match_original_instruction_goldens() {
        // Fresh windows through `0x406`, `0x405` and `0x400`
        // (tools/storage_oracle/launcher_trackbar.py): saved values outside
        // the range, the stepped read-back, a zero step and a minimum off
        // the step.
        let golden = test_fixture::json("tools/storage_oracle/launcher_trackbar.json");
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
