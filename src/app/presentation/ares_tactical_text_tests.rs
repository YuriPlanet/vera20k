use super::*;
use crate::app::presentation::detail::DetailState;

/// Ares `1006BE80`: the release note ends on the battlefield's bottom edge in
/// red and the counter stacks on it in white, each over a black box at
/// x = 0 the size of its ComputeTextRect (DrawText `1006BAB0`). The counter
/// prints gamemd's published rate and the published total over the samples.
#[test]
fn release_note_sits_on_the_bottom_edge_and_the_counter_stacks_on_it() {
    let mut detail = DetailState::new();
    (0..30).for_each(|_| detail.record_logic_visit());
    detail.throttle_tail(0);
    (0..60).for_each(|_| detail.record_logic_visit());
    detail.throttle_tail(60 << 4);
    let fps = fps_text(detail.frame_rate(), detail.average_frame_rate());

    let lines = place_lines(Some("Beta 3"), Some(fps), 568, |text| {
        [text.len() as i32 * 7, 17]
    });

    assert_eq!(
        lines,
        vec![
            PlacedLine {
                text: "Beta 3".into(),
                rgb: [255, 0, 0],
                at: [0, 551],
                fill: [0, 551, 42, 17],
            },
            PlacedLine {
                text: "FPS: 60   Avg: 45.00".into(),
                rgb: [255, 255, 255],
                at: [0, 534],
                fill: [0, 534, 140, 17],
            },
        ]
    );
}

/// Ares' own `ares.csf` holds `TXT_RELEASE_NOTE` empty, and before the
/// first sample the average is 0: the counter alone takes the bottom line.
#[test]
fn an_empty_or_absent_release_note_leaves_the_bottom_line_to_the_counter() {
    let detail = DetailState::new();
    for note in [Some(""), None] {
        let fps = fps_text(detail.frame_rate(), detail.average_frame_rate());
        let lines = place_lines(note, Some(fps), 568, |_| [10, 17]);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            (lines[0].text.as_str(), lines[0].at),
            ("FPS: 0    Avg: 0.00", [0, 551])
        );
    }
}
