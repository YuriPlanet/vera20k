//! The build description text drawn at the top right of the tactical view.
//!
//! Native identity: Phobos hooks `GScreenClass::DrawText @ 0x4F4583` to print
//! `Phobos::VersionDescription` (`src/Phobos.cpp`) on the Composite surface:
//! a black box starting `marginX` (10) pixels in from the view's right edge
//! at `y = 0`, and the text in red five pixels inside it. Retail's own
//! `GScreenClass::DrawText` body draws the optional `ShowGameTime` clock,
//! which VERA20K does not implement, so this module is that site's first
//! VERA writer and carries the version description instead.
//!
//! The text is the native product name, version and build warning from
//! [`crate::app::phobos_version`], drawn only in Phobos builds; every other
//! build keeps the retail screen, which prints nothing here.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::phobos_version;
use crate::render::batch::SpriteInstance;
use crate::ui::game_screen::GameScreen;

/// Native `marginX` with `MessageDisplayInCenter` off (`src/Phobos.cpp`);
/// VERA20K has no center-message mode, so the small margin is the only one.
const MARGIN_X: i32 = 10;
/// The black box's padding past the text on the right and bottom
/// (`wanted.Width + 10`, `wanted.Height + 10`).
const BOX_PAD: i32 = 10;
/// The text's inset inside the box (`rect.X + 5`, y `5`).
const TEXT_INSET: i32 = 5;
/// Native `COLOR_RED`.
const TEXT_RGB: [u8; 3] = [255, 0, 0];

/// The build description a Phobos build stamps into the tactical view:
/// `Phobos <version>. <build warning>`.
pub(crate) fn description() -> String {
    phobos_version::version_description()
}

/// Where the description prints: its top left, and the black box behind it
/// (`x, y, width, height`), in the tactical view's pixels.
pub(crate) fn place(view_width: i32, text_width: i32, text_height: i32) -> ([i32; 2], [i32; 4]) {
    let box_x = view_width - text_width - MARGIN_X;
    (
        [box_x + TEXT_INSET, TEXT_INSET],
        [box_x, 0, text_width + BOX_PAD, text_height + BOX_PAD],
    )
}

/// The description's fill box and text instances, or empty pairs outside a
/// Phobos build's in-game screen.
pub(crate) fn build_instances(state: &AppState) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    if !crate::rules::PHOBOS_ENABLED
        || !phobos_version::SHOWS_DESCRIPTION
        || state.frontend.screen != GameScreen::InGame
    {
        return (Vec::new(), Vec::new());
    }
    let (view_width, _) = crate::app::input::camera::tactical_viewport_size(state);
    // Screen-space lanes add back the world camera the UI shader subtracts.
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let font = &state.renderer.bit_font;
    let text = description();
    let text_width = font.text_width(&text) as i32;
    let text_height = font.cell_height() as i32;
    let (at, fill_rect) = place(view_width as i32, text_width, text_height);
    let fill = SpriteInstance {
        position: [
            fill_rect[0] as f32 + camera[0],
            fill_rect[1] as f32 + camera[1],
        ],
        size: [fill_rect[2] as f32, fill_rect[3] as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        tint: [0.0; 3],
        alpha: 1.0,
        ..Default::default()
    };
    let tint: [f32; 3] = TEXT_RGB.map(|channel| f32::from(channel) / 255.0);
    let texts = font.build_text(&text, at[0] as f32, at[1] as f32, 1.0, 0.0, tint, camera);
    (vec![fill], texts)
}

#[cfg(test)]
#[path = "build_text_tests.rs"]
mod tests;
