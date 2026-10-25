//! Build description placement and content tests.

use super::*;

#[cfg(feature = "phobos")]
#[test]
fn description_is_the_native_version_description() {
    assert_eq!(
        description(),
        "Phobos v0.5.0.0-alpha1. Please test the build before shipping.",
        "the native product name, version and build warning"
    );
}

#[test]
fn place_puts_the_box_at_the_top_right_with_the_native_padding() {
    // Native: box X = view width - text width - 10, Y = 0, size
    // text + 10 on both axes; the text sits five pixels inside.
    let (at, fill) = place(800, 100, 12);
    assert_eq!(at, [695, 5]);
    assert_eq!(fill, [690, 0, 110, 22]);
    // The box's right edge reaches the view's right edge.
    assert_eq!(fill[0] + fill[2], 800);
}

#[test]
fn place_tracks_the_text_width() {
    let (_, narrow) = place(800, 40, 12);
    let (_, wide) = place(800, 120, 12);
    assert_eq!(narrow[0], 750);
    assert_eq!(wide[0], 670, "a wider text moves the box further left");
    assert_eq!(wide[0] + wide[2], 800);
}
