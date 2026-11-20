//! Native fixtures the shell substrate's tests (and the host's) replay.
//! Paths are relative to the repository root.

use crate::shell::geom::RectPx;

/// The fixture file at `path` (leaked: tests read a handful).
pub fn text(path: &'static str) -> &'static str {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path);
    let body = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("fixture {}: {e}", full.display()));
    Box::leak(body.into_boxed_str())
}

/// Child `control` of `dialog` at `width`x`height` after the executed
/// WM_INITDIALOG relayout (`tools/storage_oracle/shell_relayout.py`).
pub fn executed_child_window(
    dialog: u16,
    control: u16,
    width: i32,
    height: i32,
) -> RectPx {
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
    let fixture: serde_json::Value = serde_json::from_str(crate::fixtures::text(
        "tools/storage_oracle/shell_relayout.json",
    ))
    .unwrap();
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
        .unwrap_or_else(|| {
            panic!("no relayout of {dialog:#x} at {width}x{height} side {side}")
        });
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
    let fixture: serde_json::Value = serde_json::from_str(crate::fixtures::text(
        "tools/storage_oracle/shell_static_timers.json",
    ))
    .unwrap();
    fixture["cases"].as_array().unwrap().clone()
}

