//! Live whole-map picture stream for the read-only telemetry dashboard.
//!
//! While a dashboard viewer has the whole-map tab open, the frame loop becomes
//! a spectator: the simulation keeps ticking at its normal rate, the game
//! window shows only a text notice, and the whole battlefield is rendered
//! **in one frame** and pushed as one RGBA picture over the same authenticated
//! WebSocket that carries JSON snapshots.
//!
//! The engine normally shows a `battlefield`-sized window onto the world, so a
//! whole map used to need a round-robin scan of ~40 camera positions; the
//! viewer then saw a mosaic of 40 different moments. Instead, spectator mode
//! gives the match its own fixed 1920x1080 render resolution and a zoom that
//! fits the whole map into it (letterboxed, never stretched). The production
//! wheel-zoom pass renders the world at `resolution / zoom` source pixels (the
//! full map, 1:1) and resamples it down into that resolution. One render, one
//! readback, one coherent frame. Encoding and publishing happen on their own
//! thread, so the presentation loop keeps its own rate; the delivered rate then
//! follows the simulation tick (~57 pictures/s at 1080p on a 60 Hz match).
//!
//! The map's visible area is `[Map] LocalSize=`, the rectangle retail lets the
//! camera reach; the terrain beyond it is never shown in game. The framing is
//! therefore pinned to that area and the published picture is cropped to it,
//! so the stream shows the visible map rather than the map file's full extent.
//!
//! This is presentation-only. It reuses the production `render_game` exactly
//! once per frame, so instance buffers, shroud binding and every commit point
//! see one ordinary render. Closing the tab restores the camera, zoom and
//! visibility the player had before the stream started.

use std::collections::VecDeque;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

use super::{AppState, GameScreen};
use crate::app::map_picture::canvas::WorldRect;
use crate::app::telemetry_server::PictureSink;
use crate::render::frame_readback::{AsyncTextureReadback, PendingTextureReadback};

/// "VMAP" + full size + tile rectangle, all little-endian u32, then RGBA.
/// A whole-map frame is simply the tile rectangle covering the entire picture.
const MAGIC: &[u8; 4] = b"VMAP";
const HEADER_BYTES: usize = 28;
/// Fixed 1080p output canvas. The whole map is fitted into it without
/// distorting the isometric projection; any leftover band is centred and shows
/// the empty world beyond the map edge.
const OUTPUT_WIDTH: u32 = 1920;
const OUTPUT_HEIGHT: u32 = 1080;
/// Must match `telemetry_server::MAX_SCREEN_FRAME`. 1920x1080 RGBA plus the
/// header is 8,294,428 bytes, just inside this 8 MiB ceiling.
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// Submitted copies whose buffers are being mapped, oldest first. Two is
/// enough for a picture per frame and bounds staging memory (~17 MiB); a third
/// measured no faster (the loop, not the readback, is the limit).
const MAX_IN_FLIGHT: usize = 2;
/// Ceiling on the picture stream, in whole pictures per rolling second. A
/// rolling window rather than a fixed interval, because the event loop's frame
/// time is not an exact multiple of any interval. At most one picture starts
/// per game frame; the engine's own frame pacer (`Main::ThrottleFrame`) holds
/// that loop near the 60 Hz simulation tick, so at 1080p the delivered rate is
/// ~57 pictures/s and this ceiling does not bind. It exists to bound the work a
/// pathological loop could ask for. `VERA_SPECTATOR_FPS` overrides it without a
/// rebuild (1..=240).
const CAPTURE_WINDOW: Duration = Duration::from_secs(1);
const DEFAULT_CAPTURES_PER_WINDOW: u32 = 90;

fn capture_limit() -> u32 {
    use std::sync::OnceLock;
    static LIMIT: OnceLock<u32> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("VERA_SPECTATOR_FPS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| value.clamp(1, 240))
            .unwrap_or(DEFAULT_CAPTURES_PER_WINDOW)
    })
}
/// ASCII only: the window notice is drawn with the retail GAME.FNT bit font.
const NOTICE: &str = "SPECTATOR MODE - THE WHOLE MAP IS STREAMED TO THE DEBUG CONSOLE - CLOSE THAT TAB TO RETURN TO THE GAME VIEW";

#[derive(Default)]
pub(crate) struct MapViewSession {
    pub(crate) active: bool,
    saved: Option<SavedView>,
    /// The dedicated whole-map render resolution. `AppState::render_dimensions`
    /// and the tactical viewport both read this, so the match renders one
    /// picture of exactly this size instead of a window-sized battlefield.
    pub(crate) view_size: Option<(u32, u32)>,
    /// Top-left world pixel of the whole-map area, and the zoom that fits the
    /// whole map into `view_size` source pixels.
    camera: (f32, f32),
    zoom: f32,
    /// The copy is encoded into the frame's encoder but not yet submitted.
    /// `map_async` is illegal before that submit, so the map starts in
    /// `poll_and_publish`, which runs after `queue.submit` and `present`.
    staged: Option<PendingTextureReadback>,
    /// Submitted copies whose buffers are being mapped, oldest first.
    mapping: VecDeque<AsyncTextureReadback>,
    /// Rolling one-second window (start, pictures started) for the rate cap.
    capture_window: Option<(Instant, u32)>,
    /// The part of the rendered frame that is map rather than the engine's
    /// void margin around it, measured from the first picture. `None` means
    /// nothing has been published yet, so the whole frame is sent.
    crop: Option<Crop>,
    /// Pictures are encoded and pushed off the frame loop. See
    /// [`PictureWorker`]; `None` means the loop does it inline.
    worker: Option<PictureWorker>,
}

/// The visible area of one rendered frame, in picture pixels.
///
/// The camera rectangle the clamp hands out is wider and taller than the
/// terrain, so the engine clears a black band around the map -- the hidden
/// region retail never shows. Only the map itself is published, which also
/// saves the bandwidth of those rows.
#[derive(Clone, Copy)]
struct Crop {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

impl Crop {
    fn whole(width: u32, height: u32) -> Self {
        Self {
            left: 0,
            top: 0,
            width,
            height,
        }
    }
}

/// Encodes and publishes finished readbacks on its own thread.
///
/// A 1080p picture is 8.3 MB: the BGRA->RGBA swizzle plus the WebSocket frame
/// copy cost more than an entire game frame in a debug build, and they used to
/// run inside `poll_and_publish`. Handing the tight readback over keeps the
/// presentation loop at its own rate; a queue that is still full drops the
/// picture, which is the correct outcome for a live view.
struct PictureWorker {
    sender: Option<SyncSender<(Crop, u32, Vec<u8>)>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// Two queued pictures absorb a scheduling hiccup without letting a stalled
/// worker retain more than ~17 MB of stale frames.
const WORKER_QUEUE_DEPTH: usize = 2;

impl PictureWorker {
    fn spawn(sink: PictureSink) -> Self {
        let (sender, receiver) =
            std::sync::mpsc::sync_channel::<(Crop, u32, Vec<u8>)>(WORKER_QUEUE_DEPTH);
        let handle = std::thread::Builder::new()
            .name("vera-map-picture".to_owned())
            .spawn(move || {
                while let Ok((crop, source_width, bgra)) = receiver.recv() {
                    let frame = encode_frame(source_width, crop, &bgra);
                    if frame.len() <= MAX_FRAME_BYTES {
                        sink.publish_binary(&frame);
                    }
                }
            })
            .ok();
        Self {
            sender: Some(sender),
            handle,
        }
    }

    fn submit(&self, crop: Crop, source_width: u32, bgra: Vec<u8>) {
        let Some(sender) = self.sender.as_ref() else {
            return;
        };
        // Never wait: an old picture is worth less than the current frame.
        match sender.try_send((crop, source_width, bgra)) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

impl Drop for PictureWorker {
    fn drop(&mut self) {
        // Closing the queue ends the loop after the picture in flight, so the
        // join below is bounded by one swizzle.
        self.sender = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct SavedView {
    camera_x: f32,
    camera_y: f32,
    zoom_level: f32,
    zoom_target: f32,
    full_visibility: bool,
    map_picture: bool,
}

/// Enter, leave or advance spectator mode. Called once per frame, before any
/// world instance is built, so the camera this frame uses is already settled.
pub(crate) fn sync(state: &mut AppState) {
    let wanted = state
        .diag
        .telemetry_server
        .as_ref()
        .is_some_and(|server| server.has_screen_clients())
        && state.frontend.screen == GameScreen::InGame
        // The Options zoom composes through a second target; keep the
        // spectator on the plain one-target route for now.
        && state.renderer.upscale_pass.is_none()
        && state.match_state.sim_runtime.is_some();
    if !wanted {
        if state.diag.map_view.active {
            stop(state);
        }
        return;
    }
    if !state.diag.map_view.active {
        start(state);
    }
    arm_view(state);
}

/// Whether this loop must draw although the window is minimised. A subscribed
/// viewer reads its picture from the frame the match draws, not from the
/// window, so a minimised match that keeps streaming must keep drawing. The
/// GPU context renders that frame offscreen (no surface to acquire), and the
/// picture reaches the dashboard as usual.
pub(crate) fn wants_frames_while_hidden(state: &AppState) -> bool {
    state.diag.map_view.active
        && state.diag.map_view.view_size.is_some()
        && state.frontend.screen == GameScreen::InGame
}

fn start(state: &mut AppState) {
    let input = &state.match_state.input;
    state.diag.map_view.saved = Some(SavedView {
        camera_x: input.camera_x,
        camera_y: input.camera_y,
        zoom_level: input.zoom_level,
        zoom_target: input.zoom_target,
        full_visibility: state.match_state.sandbox_full_visibility,
        map_picture: state.platform.capture_presentation.map_picture,
    });
    // `sandbox_full_visibility` is the per-frame switch every visibility
    // consumer already reads; setting it here removes the shroud and fog for
    // the streamed picture without touching simulation state.
    state.match_state.sandbox_full_visibility = true;
    // A map picture's camera may pass the playable area's edges, which is
    // exactly what framing the whole map needs. It also suppresses the
    // screen-fixed UI layers (they would be repeated over the picture).
    state.platform.capture_presentation.map_picture = true;
    state.diag.map_view.active = true;
    // The stream leaves the surface's present mode alone. Asking for an
    // unsynchronised mode means asking for vsync back afterwards, and on Vulkan
    // that second `configure` loses the device when it follows a match ending:
    // every later acquisition then answers `Validation` and the window stops
    // drawing for the rest of the session. The stream gains nothing by it — the
    // loop is paced by the engine's frame pacer and the picture ceiling
    // (`DEFAULT_CAPTURES_PER_WINDOW`) — so the surface stays on vsync.
    if let Some(view) = build_view(state) {
        state.diag.map_view.view_size = Some(view.size);
        state.diag.map_view.camera = view.camera;
        state.diag.map_view.zoom = view.zoom;
        state.diag.map_view.crop = Some(view.crop);
        log::info!(
            "Whole-map spectator stream enabled: visible area {}x{} of a {}x{} frame, zoom {:.4}, camera ({:.0},{:.0})",
            view.crop.width,
            view.crop.height,
            view.size.0,
            view.size.1,
            view.zoom,
            view.camera.0,
            view.camera.1,
        );
    } else {
        // Without a playable area there is nothing to frame. The stream stays
        // idle rather than rendering a bogus camera position.
        state.diag.map_view.view_size = None;
        state.diag.map_view.crop = None;
        log::warn!("Whole-map stream has no playable area yet");
    }
    state.diag.map_view.staged = None;
    state.diag.map_view.mapping.clear();
    state.diag.map_view.capture_window = None;
    let sink = state
        .diag
        .telemetry_server
        .as_ref()
        .map(|server| server.picture_sink());
    state.diag.map_view.worker = sink.map(PictureWorker::spawn);
}

fn stop(state: &mut AppState) {
    if let Some(saved) = state.diag.map_view.saved.take() {
        let input = &mut state.match_state.input;
        input.camera_x = saved.camera_x;
        input.camera_y = saved.camera_y;
        input.zoom_level = saved.zoom_level;
        input.zoom_target = saved.zoom_target;
        state.match_state.sandbox_full_visibility = saved.full_visibility;
        state.platform.capture_presentation.map_picture = saved.map_picture;
    }
    let session = &mut state.diag.map_view;
    session.active = false;
    session.view_size = None;
    session.staged = None;
    session.mapping.clear();
    session.capture_window = None;
    session.crop = None;
    // Dropping the worker closes its queue and joins it, so no picture thread
    // outlives the stream.
    session.worker = None;
    log::info!("Whole-map spectator stream disabled");
}

struct View {
    size: (u32, u32),
    camera: (f32, f32),
    zoom: f32,
    crop: Crop,
}

/// One picture, one render, at the fixed 1080p size. The zoom fits the map's
/// visible area (`[Map] LocalSize=`, which is what retail lets the camera
/// reach; see `input::camera::tactical_view_area`) into that frame while
/// preserving the isometric aspect. The area is pinned to the frame's top-left
/// corner rather than centred, so the pixels outside it are one clean band that
/// `crop` drops from the published picture -- that band is the map the game
/// hides, never part of the visible area. The production wheel-zoom pass
/// renders `size / zoom` source pixels -- the area at native world scale -- and
/// resamples them into `size` in one pass.
fn build_view(state: &AppState) -> Option<View> {
    let view = crate::app::input::camera::tactical_view_area(state)?;
    let area = WorldRect::of_view_area(view)?;
    if area.w == 0 || area.h == 0 {
        return None;
    }
    let size = (OUTPUT_WIDTH, OUTPUT_HEIGHT);
    let zoom = (size.0 as f32 / area.w as f32).min(size.1 as f32 / area.h as f32);
    if !zoom.is_finite() || zoom <= 0.0 {
        return None;
    }
    // One axis fits exactly; the other is rounded down so the crop never
    // reaches past the visible area into the margin.
    let crop = Crop {
        left: 0,
        top: 0,
        width: ((area.w as f32 * zoom).round() as u32).clamp(1, size.0),
        height: ((area.h as f32 * zoom).round() as u32).clamp(1, size.1),
    };
    Some(View {
        size,
        camera: (area.x as f32, area.y as f32),
        zoom,
        crop,
    })
}

/// Keep the camera and zoom owned by the spectator. `set_camera_position`
/// clears pending scroll, and `map_picture` keeps the later clamp from moving
/// the camera outside the playable area.
fn arm_view(state: &mut AppState) {
    let (camera, zoom) = (state.diag.map_view.camera, state.diag.map_view.zoom);
    if state.diag.map_view.view_size.is_none() || zoom <= 0.0 {
        return;
    }
    crate::app::input::camera::set_camera_position(&mut state.match_state.input, camera);
    let input = &mut state.match_state.input;
    input.zoom_level = zoom;
    input.zoom_target = zoom;
    // Restated every frame: the shroud is what the picture must not carry, and
    // several later passes write the flag for the player's own window.
    state.match_state.sandbox_full_visibility = true;
}

/// Called immediately after `render_game`, before the frame is presented.
/// Returns whether the spectator owns the window this frame; when it does the
/// caller draws the notice instead of copying the battlefield to the surface.
pub(crate) fn capture(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
) -> anyhow::Result<bool> {
    let Some((width, height)) = state.diag.map_view.view_size else {
        return Ok(state.diag.map_view.active);
    };
    if !state.diag.map_view.active {
        return Ok(false);
    }
    // One picture per frame at most, capped by the rolling one-second budget.
    // The loop is paced near the 60 Hz simulation tick, so the budget rarely
    // binds; the in-flight guard below is what actually spaces pictures out.
    let now = Instant::now();
    let (window_start, captures) = match state.diag.map_view.capture_window {
        Some((start, count)) if now.duration_since(start) < CAPTURE_WINDOW => (start, count),
        _ => (now, 0),
    };
    if captures >= capture_limit()
        || state.diag.map_view.staged.is_some()
        || state.diag.map_view.mapping.len() >= MAX_IN_FLIGHT
    {
        state.diag.map_view.capture_window = Some((window_start, captures));
        return Ok(true);
    }
    // The wheel-zoom pass resampled the whole map into its frame-sized color
    // texture. Reading that one small texture gives one coherent picture; the
    // old scan read a battlefield-sized tile from a different moment each time.
    let Some(texture) = state
        .renderer
        .world_zoom_pass
        .as_ref()
        .map(|zoom| zoom.color_texture())
    else {
        return Ok(true);
    };
    let (copy_w, copy_h) = (width.min(texture.width()), height.min(texture.height()));
    if copy_w != width || copy_h != height {
        return Ok(true);
    }
    let gpu = &state.renderer.gpu;
    let readback = match PendingTextureReadback::encode(
        &gpu.device,
        encoder,
        texture,
        gpu.config.format,
        copy_w,
        copy_h,
    ) {
        Ok(readback) => readback,
        Err(error) => {
            // A surface format this readback cannot sample must never take
            // down the match: the stream simply stays empty.
            log::warn!("Whole-map stream readback unavailable: {error}");
            return Ok(true);
        }
    };
    let session = &mut state.diag.map_view;
    // Do not map yet: this frame's encoder still carries the copy and has not
    // been submitted. `poll_and_publish` starts the map after the submit.
    session.staged = Some(readback);
    session.capture_window = Some((window_start, captures + 1));
    Ok(true)
}

/// The window's whole content in spectator mode: a clear and one line of text.
pub(crate) fn draw_notice(
    state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    destination: &wgpu::Texture,
) {
    crate::app::loading::transitions::clear_screen(encoder, view);
    state.renderer.performance_overlay.encode_status(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        encoder,
        destination,
        &state.renderer.bit_font,
        NOTICE,
    );
}

/// Pump completed readbacks and publish the finished picture. Called at the
/// end of the frame, after the submission this copy belongs to was presented.
pub(crate) fn poll_and_publish(state: &mut AppState) {
    // The encoder carrying this copy was submitted above. Mapping the buffer
    // before that submit makes wgpu reject Queue::submit with "buffer is still
    // mapped"; start it here, after the submission and presentation.
    if let Some(readback) = state.diag.map_view.staged.take() {
        state
            .diag
            .map_view
            .mapping
            .push_back(readback.begin());
    }
    // map_async callbacks are dispatched on poll; Poll never waits for GPU
    // completion, so the presentation thread keeps its frame budget.
    let _ = state.renderer.gpu.device.poll(wgpu::PollType::Poll);
    let mut finished = Vec::new();
    {
        let queue = &mut state.diag.map_view.mapping;
        let mut remaining = VecDeque::with_capacity(queue.len());
        while let Some(mut mapping) = queue.pop_front() {
            match mapping.try_finish() {
                Some(result) => finished.push(result),
                None => remaining.push_back(mapping),
            }
        }
        *queue = remaining;
    }
    let Some((width, height)) = state.diag.map_view.view_size else {
        return;
    };
    // Only the map's visible area is published: the rest of the frame is the
    // world beyond it, which retail never shows.
    let crop = state
        .diag
        .map_view
        .crop
        .unwrap_or_else(|| Crop::whole(width, height));
    for result in finished {
        let bgra = match result {
            Ok(pixels) => pixels,
            Err(error) => {
                log::warn!("Whole-map stream picture dropped: {error}");
                continue;
            }
        };
        if let Some(worker) = state.diag.map_view.worker.as_ref() {
            worker.submit(crop, width, bgra);
            continue;
        }
        // No worker (no server yet): encode inline rather than drop the frame.
        let Some(server) = state.diag.telemetry_server.as_ref() else {
            continue;
        };
        let frame = encode_frame(width, crop, &bgra);
        if frame.len() > MAX_FRAME_BYTES {
            continue;
        }
        server.publish_binary(&frame);
    }
}

/// Convert the map part of the tight BGRA readback into one RGBA VMAP frame
/// whose tile rectangle is the entire picture. The browser needs no tiling at
/// all: one `putImageData` replaces the whole canvas with one coherent moment.
/// `source_width` is the rendered frame's width, which `crop` selects from.
fn encode_frame(source_width: u32, crop: Crop, bgra: &[u8]) -> Vec<u8> {
    let pixels = crop.width as usize * crop.height as usize;
    let row_bytes = crop.width as usize * 4;
    // One allocation, then straight indexed stores. A per-pixel `get` slice
    // plus four `push` calls costs several milliseconds per picture in a debug
    // build and was the largest CPU cost of the high-resolution stream.
    let mut frame = vec![0_u8; HEADER_BYTES + pixels * 4];
    frame[..4].copy_from_slice(MAGIC);
    frame[4..8].copy_from_slice(&crop.width.to_le_bytes());
    frame[8..12].copy_from_slice(&crop.height.to_le_bytes());
    frame[12..16].copy_from_slice(&0_u32.to_le_bytes());
    frame[16..20].copy_from_slice(&0_u32.to_le_bytes());
    frame[20..24].copy_from_slice(&crop.width.to_le_bytes());
    frame[24..28].copy_from_slice(&crop.height.to_le_bytes());
    let out = &mut frame[HEADER_BYTES..];
    // A truncated readback keeps its zeroed tail, so the frame always
    // advertises a complete rectangle and the browser never sees a malformed
    // payload.
    for row in 0..crop.height as usize {
        let from = ((crop.top as usize + row) * source_width as usize + crop.left as usize) * 4;
        let Some(source) = bgra.get(from..from + row_bytes) else {
            break;
        };
        let to = row * row_bytes;
        for (pixel, target) in source
            .chunks_exact(4)
            .zip(out[to..to + row_bytes].chunks_exact_mut(4))
        {
            target[0] = pixel[2];
            target[1] = pixel[1];
            target[2] = pixel[0];
            target[3] = 255;
        }
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_picture_frame_header_covers_the_entire_canvas() {
        // Two BGRA rows of two pixels: red then blue, black then green.
        let bgra = [0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 0, 255, 0, 255, 0, 255];
        let frame = encode_frame(2, Crop::whole(2, 2), &bgra);
        assert_eq!(&frame[..4], MAGIC);
        assert_eq!(&frame[4..8], &2_u32.to_le_bytes());
        assert_eq!(&frame[8..12], &2_u32.to_le_bytes());
        assert_eq!(&frame[12..16], &0_u32.to_le_bytes());
        assert_eq!(&frame[16..20], &0_u32.to_le_bytes());
        assert_eq!(&frame[20..24], &2_u32.to_le_bytes());
        assert_eq!(&frame[24..28], &2_u32.to_le_bytes());
        assert_eq!(frame.len(), HEADER_BYTES + 2 * 2 * 4);
        assert_eq!(&frame[HEADER_BYTES..HEADER_BYTES + 4], &[255, 0, 0, 255]);
        assert_eq!(&frame[HEADER_BYTES + 4..HEADER_BYTES + 8], &[0, 0, 255, 255]);
        assert_eq!(&frame[HEADER_BYTES + 12..HEADER_BYTES + 16], &[0, 255, 0, 255]);
    }

    #[test]
    fn frame_carries_only_the_visible_area_of_a_bigger_render() {
        // A 3x3 render whose visible area is the middle column pair, top two
        // rows: the published picture must be that rectangle, row by row,
        // with the dropped band left out entirely.
        let mut bgra = Vec::new();
        for y in 0..3_u32 {
            for x in 0..3_u32 {
                let value = (y * 3 + x) as u8 + 1;
                bgra.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let crop = Crop {
            left: 1,
            top: 0,
            width: 2,
            height: 2,
        };
        let frame = encode_frame(3, crop, &bgra);
        assert_eq!(&frame[4..8], &2_u32.to_le_bytes());
        assert_eq!(&frame[8..12], &2_u32.to_le_bytes());
        assert_eq!(frame.len(), HEADER_BYTES + 2 * 2 * 4);
        let reds: Vec<u8> = frame[HEADER_BYTES..].chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(reds, vec![2, 3, 5, 6]);
    }
}
