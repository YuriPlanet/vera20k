//! The game's cursor on the window system's cursor plane.
//!
//! gamemd's mouse does not wait for a frame. `WinMain` (`0x006BD849`) starts
//! `WWMouseClass::StartThread` (`0x007B84F0`), whose time-critical `ThreadProc`
//! (`0x007B8610`) runs `ProcessMouse` (`0x007BA090`) every 16 ms: it reads
//! `GetCursorPos` and repaints mouse.shp on the primary surface, while the main
//! thread only picks the shape (`SetCursorShape` `0x007B8A00`, from
//! `MouseClass::SetCursor` `0x005BDC80` and `MouseClass::Action` `0x005BDDC0`).
//! VERA's match service and draw share one thread, so a sprite painted into
//! the frame moves only as often as frames finish and stalls with a long one.
//!
//! The window system's cursor plane keeps gamemd's split: it moves the image
//! at the pointer's own rate, however long a frame takes, and each draw only
//! says which shape it shows. A draw that would paint the cursor offers its
//! shape here instead ([`takes_cursor`]), and [`show`] puts it on the window
//! after the draw. The sprite stays where the OS pointer cannot stand in for
//! the cursor: frames that are read back, touch input, and a pointer in a
//! zoomed match's border, where the cursor clamps to the frame.
//!
//! Presentation-only: never serialized or read by the deterministic simulation.

use std::cell::Cell;
use std::collections::HashMap;

use winit::event_loop::ActiveEventLoop;

use crate::app::AppState;
use crate::app::types::CursorId;
use crate::render::cursor_atlas::OsCursorImage;

/// Android and the browser have no cursor plane this reaches.
const WINDOW_SYSTEM_DRAWS_CURSORS: bool =
    cfg!(not(any(target_os = "android", target_arch = "wasm32")));

#[cfg(target_os = "macos")]
type Image = super::mac_pointer::MacCursor;
#[cfg(not(target_os = "macos"))]
type Image = winit::window::CustomCursor;

/// One cursor frame at one size, as the window system holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Shape {
    id: CursorId,
    frame: usize,
    /// The mouse.sha load it came from (`SoftwareCursor::art_id`).
    art: u64,
    /// Bits of the window pixels per mouse.sha pixel.
    scale: u32,
    /// Bits of the screen's physical pixels per logical one.
    backing: u64,
}

impl Shape {
    fn same_art_and_size(self, other: Self) -> bool {
        (self.art, self.scale, self.backing) == (other.art, other.scale, other.backing)
    }
}

/// What the window system shows at the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shown {
    #[default]
    Hidden,
    /// The system arrow, where VERA draws no cursor of its own.
    Arrow,
    Game(Shape),
}

/// The owner of what the window system shows at the pointer.
#[derive(Default)]
pub(crate) struct HardwareCursor {
    /// This run reads its frames back, and they must hold the cursor.
    frames_read_back: bool,
    /// The shape this draw offered, until [`show`] takes it.
    offered: Cell<Option<(CursorId, usize)>>,
    shown: Shown,
    /// Images made so far: one mouse.sha load at one size. A new image of
    /// another load or size drops the rest.
    images: HashMap<Shape, Image>,
    /// A shape could not be made, so the sprite draws the cursor from now on.
    failed: bool,
}

impl HardwareCursor {
    /// Set before each pass: a capture, a hidden run or a replay capture
    /// keeps the cursor in the frames it reads back.
    pub(crate) fn set_frames_read_back(&mut self, read_back: bool) {
        self.frames_read_back = read_back;
    }

    /// Whether the window system draws what stands at the pointer, so moving
    /// it changes no pixel of the frame.
    pub(crate) fn on_cursor_plane(&self) -> bool {
        self.shown != Shown::Hidden
    }
}

/// Whether the window system shows this draw's cursor, `frame` of `id`, so
/// the draw paints no sprite for it. A true answer is remembered for [`show`].
pub(crate) fn takes_cursor(state: &AppState, id: CursorId, frame: usize) -> bool {
    let cursor = &state.platform.hardware_cursor;
    let takes = WINDOW_SYSTEM_DRAWS_CURSORS
        && !cursor.frames_read_back
        && !cursor.failed
        && !state.match_state.input.cursor_from_touch
        && state
            .match_state
            .input
            .pointer_in_window()
            .is_none_or(|pointer| {
                pointer_is_on_cursor(pointer, state.window_cursor_position(), zoom_scale(state))
            });
    if takes {
        cursor.offered.set(Some((id, frame)));
    }
    takes
}

/// Whether the OS pointer, in window pixels, stands on the game's cursor.
/// `window_to_source` clamps a window point to the frame's last source pixel,
/// which is `scale` window pixels across, so a pointer on it is still on the
/// cursor; a pointer in the zoom's border, or one the cursor lock's relative
/// motion left behind (`InputState::set_cursor_from_window`), is not.
fn pointer_is_on_cursor(pointer: (f32, f32), cursor: (f32, f32), scale: f32) -> bool {
    let slack = scale + 0.5;
    (pointer.0 - cursor.0).abs() <= slack && (pointer.1 - cursor.1).abs() <= slack
}

fn zoom_scale(state: &AppState) -> f32 {
    state.tactical_zoom().map_or(1.0, |zoom| zoom.scale())
}

/// After a draw, put what it settled on at the pointer: the shape it offered,
/// else the `arrow`, else nothing.
pub(crate) fn show(state: &mut AppState, event_loop: &ActiveEventLoop, arrow: bool) {
    let offered = state.platform.hardware_cursor.offered.take();
    let next = match offered
        .filter(|_| !arrow)
        .and_then(|(id, frame)| prepare_image(state, event_loop, id, frame))
    {
        Some(shape) => Shown::Game(shape),
        None if arrow => Shown::Arrow,
        None => Shown::Hidden,
    };
    apply(state, next);
}

/// The window went to the background and may draw no frame that would give
/// the arrow back.
pub(crate) fn give_back_arrow(state: &mut AppState) {
    apply(state, Shown::Arrow);
}

/// A frame presented outside the ordinary draw, which paints no cursor (the
/// saved-game load's progress dialog): take the game's image off the screen.
pub(crate) fn hide_game_cursor(state: &mut AppState) {
    if matches!(state.platform.hardware_cursor.shown, Shown::Game(_)) {
        apply(state, Shown::Hidden);
    }
}

/// The image for `frame` of `id` at the window's current size, made on its
/// first use. `None` leaves this draw without a cursor and the next ones with
/// the sprite.
fn prepare_image(
    state: &mut AppState,
    event_loop: &ActiveEventLoop,
    id: CursorId,
    frame: usize,
) -> Option<Shape> {
    let scale = crate::app::frontend::shell_layout::menu_cursor_scale(state) * zoom_scale(state);
    let backing = state.platform.window.scale_factor();
    let art = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()?;
    let shape = Shape {
        id,
        frame,
        art: art.art_id(),
        scale: scale.to_bits(),
        backing: backing.to_bits(),
    };
    let cursor = &mut state.platform.hardware_cursor;
    if cursor.images.contains_key(&shape) {
        return Some(shape);
    }
    let Some(image) = art
        .os_image(id, frame, scale)
        .and_then(|image| make_image(event_loop, &image, backing))
    else {
        log::warn!("No window-system cursor image for {id:?}; drawing the cursor in the frame");
        cursor.failed = true;
        return None;
    };
    cursor
        .images
        .retain(|made, _| made.same_art_and_size(shape));
    cursor.images.insert(shape, image);
    Some(shape)
}

#[cfg(target_os = "macos")]
fn make_image(_event_loop: &ActiveEventLoop, image: &OsCursorImage, backing: f64) -> Option<Image> {
    Image::new(image, backing)
}

#[cfg(not(target_os = "macos"))]
fn make_image(event_loop: &ActiveEventLoop, image: &OsCursorImage, _backing: f64) -> Option<Image> {
    let source = Image::from_rgba(
        image.rgba.clone(),
        image.width,
        image.height,
        image.hotspot.0,
        image.hotspot.1,
    )
    .ok()?;
    Some(event_loop.create_custom_cursor(source))
}

fn apply(state: &mut AppState, next: Shown) {
    let cursor = &mut state.platform.hardware_cursor;
    let previous = std::mem::replace(&mut cursor.shown, next);
    #[cfg(target_os = "macos")]
    {
        let game = matches!(next, Shown::Game(_));
        if game != matches!(previous, Shown::Game(_)) {
            super::mac_pointer::set_cursor_rects_enabled(&state.platform.window, !game);
        }
        match next {
            // Set on every draw: winit gives a window's cursor rectangle only
            // its own images, and AppKit applies a rectangle only after the
            // pointer moves, so this is what runs an animation under a still
            // pointer and takes back the pointer from whatever else put its
            // cursor up.
            Shown::Game(shape) => cursor.images[&shape].set(),
            Shown::Arrow if previous != next => super::mac_pointer::set_system_arrow(),
            _ => {}
        }
    }
    #[cfg(not(target_os = "macos"))]
    match next {
        Shown::Game(shape) if previous != next => {
            state
                .platform
                .window
                .set_cursor(cursor.images[&shape].clone());
        }
        Shown::Arrow | Shown::Hidden if matches!(previous, Shown::Game(_)) => {
            state
                .platform
                .window
                .set_cursor(winit::window::CursorIcon::Default);
        }
        _ => {}
    }
    // On macOS the window's cursor rectangle stays blank under the game's
    // image and AppKit does not apply it (above); releasing the app-wide hide
    // below is what lets the image show.
    let window_visible = if cfg!(target_os = "macos") {
        next == Shown::Arrow
    } else {
        next != Shown::Hidden
    };
    // Asking winit is not free on Windows: `set_cursor_visible` runs
    // `refresh_os_cursor`, which clears any OS cursor clip winit does not own
    // itself, so repeating the same answer every frame would tear
    // `cursor_confined` down once per frame and let the pointer slip out of
    // the window between two re-asserts. Hand over only a transition, then
    // put the clip straight back.
    if state.platform.os_cursor_visible != Some(window_visible) {
        crate::app::input::cursor::set_os_cursor_visible(state, window_visible);
        crate::app::input::cursor::reassert_cursor_confinement(state);
    }
    #[cfg(target_os = "macos")]
    super::mac_pointer::set_arrow_hidden(next == Shown::Hidden);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::upscale_pass::{FrameShape, ZoomGeometry};

    /// A 200% 4:3 frame in a 1600x900 window has side bars. The frame's last
    /// column, which the zoom clamps a pixel short of the pointer, keeps the
    /// OS image; a pointer out in the bar leaves the cursor on the frame's
    /// edge, so the sprite must draw it.
    #[test]
    fn a_pointer_in_the_zoom_border_is_off_the_cursor() {
        let zoom = ZoomGeometry::new(1600, 900, 200, FrameShape::Original);
        let on = |x: f32| {
            let source = zoom.window_to_source(x, 450.0);
            pointer_is_on_cursor((x, 450.0), zoom.source_to_window(source.0, source.1), 2.0)
        };
        let (frame_width, _) = zoom.frame_size();
        let last_column = (zoom.origin.0 + frame_width - 1) as f32;
        assert!(on(800.0));
        assert!(
            on(last_column),
            "the frame's last column keeps the OS image"
        );
        assert!(!on(last_column + 20.0), "the side bar is off the cursor");
    }
}
