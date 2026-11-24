//! Ares 3.0's lines at the bottom left of the battlefield:
//! TacticalClass_Draw_TheDarkSideOfTheMoon (Ares.dll `1006BE80`, hook
//! `0x006D4B25`, `Misc/Surfaces.cpp`), which runs after
//! `TacticalClass::Draw`'s timer lines and before its outcome banner
//! (`0x006D4E20`). From the bottom up: the stability warning, which 3.0
//! never has (`Ares::GetStabilityWarning`, `10003650`, returns null), the
//! string table's `TXT_RELEASE_NOTE` in red, and, while the FPSCounter
//! command has it on, the frame counter in white.
//!
//! Each line (DrawText, `1006BAB0`) is measured by ComputeTextRect
//! (`0x004A59E0`) with no margins, filled black at x = 0 and printed there
//! (Simple_Text_Print_Wide, flags `0x16`). The first line ends 32 pixels
//! above the Composite surface's bottom: that surface is as tall as the
//! screen and the bottom strip below the battlefield is 32 pixels
//! (Change_Video_Mode `0x00560ED2`), so it ends on the battlefield's bottom
//! edge, and each further line stacks on the one below. The colours are the
//! surface's packed red and white.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::rules::ruleset::RuleSet;
use crate::ui::game_screen::GameScreen;

/// The release note's label (the CSFText at Ares.dll `100C2160`, label
/// `100A8C48`). Ares' own `ares.csf` holds it empty; a mod's string table
/// fills it.
const RELEASE_NOTE_LABEL: &str = "TXT_RELEASE_NOTE";
const RED: [u8; 3] = [255, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

/// One line's text, colour, and where it prints in the battlefield's pixels:
/// its top left and the black box behind it (`x, y, width, height`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedLine {
    pub(crate) text: String,
    pub(crate) rgb: [u8; 3],
    pub(crate) at: [i32; 2],
    pub(crate) fill: [i32; 4],
}

/// `FPSCounterCommandClass::Execute` (Ares.dll `100022F0`) flips the
/// counter. The command exists only in a game Ares plays.
pub(crate) fn execute_fps_counter(state: &mut AppState) {
    if state.rules().is_some_and(RuleSet::ares) {
        let presentation = &mut state.match_state.match_presentation;
        presentation.ares_fps_counter = !presentation.ares_fps_counter;
    }
}

/// The counter's line: `FPS: %-4u Avg: %.2lf` (`100A8C5C`) with gamemd's
/// published frame rate (ABCD44) and its average.
pub(crate) fn fps_text(frame_rate: u32, average: f64) -> String {
    format!("FPS: {frame_rate:<4} Avg: {average:.2}")
}

/// The lines from the bottom up, placed on a battlefield `view_height`
/// tall; `measure` is a text's ComputeTextRect width and height. The
/// release note shows when the string table gives it text: Ares
/// draws whatever its lookup returns, and `ares.csf`, which every Ares
/// install loads, holds the label, so a game that lacks it reads as Ares'
/// own empty note rather than as a missing label.
pub(crate) fn place_lines(
    release_note: Option<&str>,
    fps: Option<String>,
    view_height: i32,
    measure: impl Fn(&str) -> [i32; 2],
) -> Vec<PlacedLine> {
    let note = release_note
        .filter(|note| !note.is_empty())
        .map(|note| (note.to_owned(), RED));
    let fps = fps.map(|text| (text, WHITE));
    let mut bottom = view_height;
    note.into_iter()
        .chain(fps)
        .map(|(text, rgb)| {
            let [width, height] = measure(&text);
            let y = bottom - height;
            bottom = y;
            PlacedLine {
                text,
                rgb,
                at: [0, y],
                fill: [0, y, width, height],
            }
        })
        .collect()
}

/// The lines' fill boxes and text instances, or empty pairs outside a game
/// Ares plays, outside the in-game screen and in a map picture.
pub(crate) fn build_instances(state: &AppState) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    if !state.rules().is_some_and(RuleSet::ares)
        || state.frontend.screen != GameScreen::InGame
        || (state.platform.capture_client_size.is_some()
            && state.platform.capture_presentation.map_picture)
    {
        return (Vec::new(), Vec::new());
    }
    let presentation = &state.match_state.match_presentation;
    let fps = presentation.ares_fps_counter.then(|| {
        let detail = presentation.detail.borrow();
        fps_text(detail.frame_rate(), detail.average_frame_rate())
    });
    let release_note = state
        .process_assets
        .csf
        .as_ref()
        .and_then(|csf| csf.get(RELEASE_NOTE_LABEL));
    let font = &state.renderer.bit_font;
    let (_, view_height) = crate::app::input::camera::tactical_viewport_size(state);
    let lines = place_lines(release_note, fps, view_height as i32, |text| {
        let [_, _, width, height] = font.compute_text_rect(text, [0, 0], 0, [0, 0]);
        [width, height]
    });
    // Screen-space lanes add back the world camera the UI shader subtracts,
    // and the battlefield's corner: the lines are in the view's own pixels.
    let camera = crate::app::input::camera::battlefield_lane_camera(state);
    let mut fills = Vec::with_capacity(lines.len());
    let mut texts = Vec::new();
    for line in &lines {
        let [x, y, w, h] = line.fill;
        fills.push(SpriteInstance {
            position: [x as f32 + camera[0], y as f32 + camera[1]],
            size: [w as f32, h as f32],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            tint: [0.0; 3],
            alpha: 1.0,
            ..Default::default()
        });
        texts.extend(font.build_text(
            &line.text,
            line.at[0] as f32,
            line.at[1] as f32,
            1.0,
            0.0,
            line.rgb.map(|channel| f32::from(channel) / 255.0),
            camera,
        ));
    }
    (fills, texts)
}

#[cfg(all(test, tests_app_presentation))]
#[path = "ares_tactical_text_tests.rs"]
mod tests;
