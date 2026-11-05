//! Ordinary radar animation and its displayed pixel history.
//!
//! `RadarPresentation` owns the frame/timer and records, in order, what
//! native paints into the retained housing surface and when that surface is
//! shown; [`RadarAnimState`] replays the paints on the GPU. Internal minimap
//! updates remain independent.
//!
//! Native `652960` starts offline at frame 0 with a zero-duration timer.
//! `656BE0` changes direction without resetting that timer. `653100` advances
//! once per draw when four `timeGetTime() >> 4` buckets have elapsed, resets
//! from the current bucket, and clamps at frames 0/32 in that same draw.

use crate::render::batch::{BatchRenderer, BatchTexture, CameraUniform, SpriteInstance};
use crate::render::shroud_buffer::InstanceUpload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarAnimPhase {
    Offline,
    Opening,
    Online,
    Closing,
    /// Native housing phase 4: open frames 25..32 for a movie.
    MovieOpening,
    /// Native housing phase 5: restore the ordinary radar after a movie.
    MovieRestoring,
}

/// Native radar +14B8 prelude and mode 3/4. The app owns media resources;
/// this owner alone decides when they may occupy the displayed radar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MovieStage {
    Announce,
    WaitAnnouncement,
    WaitStreams,
    Playback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarMovieCommand {
    None,
    Announce,
    PrepareStreams,
    StartPlayback,
}

/// An admitted ordinary656BE0 direction change. The app resolves the
/// corresponding Rules Voc ID; this owner has no audio dependency.
/// Source: tools/input_oracle/radar_availability_audio.md and its consumed corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarAvailabilityCue {
    On,
    Off,
}

/// Ordinary tactical-radar housing state (movie/jammed modes are separate).
#[derive(Debug, Clone)]
struct RadarAnimation {
    pub phase: RadarAnimPhase,
    pub frame: usize,
    last_bucket: i32,
    duration: i32,
}

impl Default for RadarAnimation {
    fn default() -> Self {
        Self {
            phase: RadarAnimPhase::Offline,
            frame: 0,
            last_bucket: 0,
            duration: 0,
        }
    }
}

impl RadarAnimation {
    fn set_has_radar(&mut self, available: bool) -> Option<RadarAvailabilityCue> {
        match (available, self.phase) {
            (true, RadarAnimPhase::Offline | RadarAnimPhase::Closing) => {
                self.phase = RadarAnimPhase::Opening;
                Some(RadarAvailabilityCue::On)
            }
            //656C48 has no opening-style phase gate. Equality belongs to
            //656DF0 above this direction helper, not to the phase itself.
            (false, _) => {
                self.phase = RadarAnimPhase::Closing;
                Some(RadarAvailabilityCue::Off)
            }
            _ => None,
        }
    }

    pub fn advance_draw(&mut self, wall_ms: u64) -> bool {
        if matches!(self.phase, RadarAnimPhase::Offline | RadarAnimPhase::Online) {
            return false;
        }
        let bucket = ((wall_ms as u32) >> 4) as i32;
        // Native stopped timer sentinel keeps its remaining duration.
        let due = if self.last_bucket == -1 {
            self.duration == 0
        } else {
            bucket.wrapping_sub(self.last_bucket) >= self.duration
        };
        if !due {
            return false;
        }
        self.last_bucket = bucket;
        self.duration = 4;
        let old_frame = self.frame;
        match self.phase {
            RadarAnimPhase::Opening
            | RadarAnimPhase::MovieOpening
            | RadarAnimPhase::MovieRestoring => {
                self.frame += 1;
                if self.frame >= 32 {
                    self.frame = 32;
                    self.phase = RadarAnimPhase::Online;
                }
            }
            RadarAnimPhase::Closing => {
                self.frame = self.frame.saturating_sub(1);
                if self.frame == 0 {
                    self.phase = RadarAnimPhase::Offline;
                }
            }
            _ => {}
        }
        self.frame != old_frame
    }
}

/// One paint into the retained housing surface, the radar's rectangle of
/// `SidebarSurface` (allocated and cleared at `533FD0`), or the copy of it
/// the sidebar shows.
enum RadarPaint {
    /// A new surface: opaque black.
    Clear,
    /// Frame `n`'s pixels; `4912B0` skips source index 0, transparent here.
    Frame(usize),
    /// A movie frame along the top, centred in the surface's width.
    Movie {
        picture: BatchTexture,
        size: [u32; 2],
    },
    /// The online draw (`656EC0`) kept for a later loss: frame 32, then the
    /// minimap's picture through `map_quads` and the solid `solid_quads`, as
    /// the sidebar draws them from `origin`.
    Online {
        map_quads: Vec<SpriteInstance>,
        solid_quads: Vec<SpriteInstance>,
        origin: [f32; 2],
    },
    /// Show the surface as it now stands.
    Show,
}

/// Whether `frames` are housing frames for a `width`x`height` surface: the
/// ordinary endpoints are 0 and 32.
fn valid_frames(frames: &[Vec<u8>], width: u32, height: u32) -> bool {
    let Some(byte_count) = width.checked_mul(height).and_then(|n| n.checked_mul(4)) else {
        return false;
    };
    frames.len() >= 33
        && byte_count != 0
        && frames
            .iter()
            .all(|frame| frame.len() == byte_count as usize)
}

/// One owner for the displayed radar history, independent of GPU allocation.
/// Native 653100 paints into SidebarSurface; 656EC0 writes online content into
/// the same backing and suppresses those stores during transitions.
pub(in crate::render) struct RadarPresentation {
    animation: RadarAnimation,
    /// The surface's size, which the frames share.
    size: [u32; 2],
    /// Paints since the last replay, in order.
    paints: Vec<RadarPaint>,
    /// Show the surface at the next draw, even if no frame is due.
    needs_show: bool,
    movie: Option<MovieStage>,
    pending_radar: bool,
    movie_pause_latch: bool,
}
impl RadarPresentation {
    pub(in crate::render) fn new(frames: &[Vec<u8>], width: u32, height: u32) -> Option<Self> {
        valid_frames(frames, width, height).then(|| Self {
            animation: RadarAnimation::default(),
            size: [width, height],
            // A new surface shows frame 0 from the start.
            paints: vec![RadarPaint::Clear, RadarPaint::Frame(0), RadarPaint::Show],
            needs_show: false,
            movie: None,
            pending_radar: false,
            movie_pause_latch: false,
        })
    }
    /// Source installation is separate from animation timing (652E90).
    /// Paint the selected current frame as a forced source redraw; source0
    /// skips retain prior pixels, exactly as any other 4912B0 frame store.
    pub(super) fn replace_frames(&mut self, frames: &[Vec<u8>], width: u32, height: u32) -> bool {
        if !valid_frames(frames, width, height) {
            return false;
        }
        let frame = RadarPaint::Frame(self.animation.frame);
        if self.size != [width, height] {
            // VERA fallback for non-stock dimensional changes; no native
            // cross-size retained-history equivalence is asserted. The new
            // surface is shown as soon as it exists.
            self.size = [width, height];
            self.paints
                .extend([RadarPaint::Clear, frame, RadarPaint::Show]);
        } else {
            self.paints.push(frame);
        }
        self.needs_show = true;
        true
    }
    pub(in crate::render) fn set_has_radar(
        &mut self,
        has_radar: bool,
    ) -> Option<RadarAvailabilityCue> {
        //656DF8..656E02 compares/stores the availability byte before any
        //direction or mode request. Idle renders cannot replay a cue.
        if self.pending_radar == has_radar {
            return None;
        }
        self.pending_radar = has_radar;
        // SetMode656CB0 defers ordinary changes into +14B4 during modes3/4.
        if self.movie.is_some() {
            return None;
        }
        // 656DF0/656BE0 do not clear/dirty the native surface. A loss before
        // the next due draw must expose the last displayed map, not frame32.
        self.needs_show |= !has_radar && self.animation.phase == RadarAnimPhase::Online;
        self.animation.set_has_radar(has_radar)
    }
    /// Advance the housing, painting the frame it reaches; returns whether
    /// the surface is shown.
    pub(in crate::render) fn advance_draw(&mut self, wall_ms: u64) -> bool {
        let old_phase = self.animation.phase;
        let changed = self.animation.advance_draw(wall_ms);
        // A closing->opening reversal at frame32 can finish at the same frame.
        // Reconstruct our continuously composed Online base before map/outline
        // draws. Native instead raises14DA and only repaints frame32 if Update's
        // dirty/event gates pass (65715C..6571C9,6575F3). If none pass, its final
        // online image is unchanged; this base is not a native store event.
        if changed || old_phase != self.animation.phase {
            self.paints.push(RadarPaint::Frame(self.animation.frame));
            self.needs_show = true;
        }
        let show = std::mem::take(&mut self.needs_show);
        if show {
            self.paints.push(RadarPaint::Show);
        }
        show
    }
    pub(in crate::render) fn phase(&self) -> RadarAnimPhase {
        self.animation.phase
    }
    /// Native states 2..5: the housing is opening or closing.
    fn housing_moving(&self) -> bool {
        !matches!(
            self.animation.phase,
            RadarAnimPhase::Offline | RadarAnimPhase::Online
        )
    }
    /// One `653100` draw: whether the housing finished opening or closing in
    /// it, where native ends the radar's sound loop (`0x00653267` at frame 0,
    /// `0x0065328C` at frame 32).
    fn draw(&mut self, wall_ms: u64) -> bool {
        let moving = self.housing_moving();
        self.advance_draw(wall_ms);
        moving && !self.housing_moving()
    }
    fn admit_movie(&mut self) -> bool {
        if self.movie.is_some() {
            return false;
        }
        self.movie = Some(MovieStage::Announce);
        // SetMode4 at656DB2 changes only mode; the housing is preserved while
        // IncomingTransmission plays. SetMode3 below starts its transition.
        true
    }
    fn movie_step(&mut self, selected_done: bool, streams_ready: bool) -> RadarMovieCommand {
        match self.movie {
            Some(MovieStage::Announce) => {
                self.movie = Some(MovieStage::WaitAnnouncement);
                RadarMovieCommand::Announce
            }
            Some(MovieStage::WaitAnnouncement) if selected_done => {
                self.movie = Some(MovieStage::WaitStreams);
                RadarMovieCommand::PrepareStreams
            }
            Some(MovieStage::WaitStreams) if streams_ready => {
                self.movie = Some(MovieStage::Playback);
                if matches!(
                    self.animation.phase,
                    RadarAnimPhase::Online | RadarAnimPhase::MovieRestoring
                ) {
                    //656D69:phase1/5 starts at25; other phases reverse/open
                    //through the same656BE0 owner without resetting its timer.
                    self.animation.phase = RadarAnimPhase::MovieOpening;
                    self.animation.frame = 25;
                } else {
                    self.animation.phase = RadarAnimPhase::Opening;
                }
                RadarMovieCommand::StartPlayback
            }
            _ => RadarMovieCommand::None,
        }
    }
    fn finish_movie(&mut self, has_next: bool) {
        if has_next {
            //657BEE writes phase4/frame25; no second IncomingTransmission.
            self.animation.phase = RadarAnimPhase::MovieOpening;
            self.animation.frame = 25;
        } else {
            self.movie = None;
            self.movie_pause_latch = false;
            if self.pending_radar {
                self.animation.phase = RadarAnimPhase::MovieOpening;
                self.animation.frame = 25;
            } else {
                self.animation.phase = RadarAnimPhase::Closing;
            }
        }
    }
    /// Native movie copies into retained SidebarSurface. Use the stock 140x110
    /// aperture centered in its168x110 backing. Full native origin/final-paint
    /// driver equivalence is not established; no battlefield shader involved.
    fn paint_movie(&mut self, picture: &BatchTexture, size: [u32; 2]) {
        // Painting the same picture again stores the same pixels.
        if let [
            ..,
            RadarPaint::Movie {
                picture: last,
                size: last_size,
            },
            RadarPaint::Show,
        ] = self.paints.as_slice()
            && last.view == picture.view
            && *last_size == size
        {
            return;
        }
        self.paints.push(RadarPaint::Movie {
            picture: picture.clone(),
            size,
        });
        self.paints.push(RadarPaint::Show);
    }
    /// Save the actual online content before the next availability change.
    /// Internal minimap updates continue while closed; they cannot replace this
    /// displayed backing until online again (`656EF2..6F0D`, `657578`).
    /// Returns whether it was kept.
    pub fn retain_online_content(
        &mut self,
        map_quads: &[SpriteInstance],
        solid_quads: impl Iterator<Item = SpriteInstance>,
        origin: [f32; 2],
    ) -> bool {
        if self.animation.phase != RadarAnimPhase::Online || self.movie.is_some() {
            return false;
        }
        // Snapshot the currently displayed online draw, whose stock housing
        // endpoint is opaque. Do not accumulate trails from old viewport quads.
        self.paints.push(RadarPaint::Online {
            map_quads: map_quads.to_vec(),
            solid_quads: solid_quads.collect(),
            origin,
        });
        true
    }
}

/// One step of a replay: a draw of `count` quads from `first`, or a surface
/// operation.
enum ReplayStep {
    Clear,
    Show,
    Draw {
        texture: BatchTexture,
        /// Index into the replay's cameras: 0 is the surface's own.
        camera: usize,
        first: u32,
        count: u32,
    },
}

/// A quad of `size` pixels at `position`, sampling its whole picture.
fn picture_quad(position: [f32; 2], size: [f32; 2]) -> SpriteInstance {
    SpriteInstance {
        position,
        size,
        uv_size: [1.0; 2],
        tint: [1.0; 3],
        alpha: 1.0,
        ..Default::default()
    }
}

/// The GPU side of the one retained radar presentation owner: the housing
/// frames, the surface its paints are replayed into, and the copy of that
/// surface the sidebar draws.
pub struct RadarAnimState {
    presentation: RadarPresentation,
    frames: Vec<BatchTexture>,
    surface: BatchTexture,
    displayed: BatchTexture,
    depth: wgpu::TextureView,
    /// The surface's own camera, then one per online paint in a replay.
    cameras: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    quads: InstanceUpload,
    /// The minimap's picture and white texture, which the online paints draw.
    online: Option<(BatchTexture, BatchTexture)>,
    pub width: u32,
    pub height: u32,
}
impl RadarAnimState {
    /// Literal ordinary native endpoints are 0 and 32; malformed frames are
    /// rejected before constructing the textures.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        frames_rgba: Vec<Vec<u8>>,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let presentation = RadarPresentation::new(&frames_rgba, width, height)?;
        let (surface, displayed, depth) = housing_targets(device, batch, width, height);
        Some(Self {
            presentation,
            frames: frame_textures(device, queue, batch, &frames_rgba, width, height),
            surface,
            displayed,
            depth,
            cameras: Vec::new(),
            quads: InstanceUpload::default(),
            online: None,
            width,
            height,
        })
    }
    pub fn replace_frames(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        frames: Vec<Vec<u8>>,
        width: u32,
        height: u32,
    ) -> bool {
        if !self.presentation.replace_frames(&frames, width, height) {
            return false;
        }
        self.frames = frame_textures(device, queue, batch, &frames, width, height);
        if [width, height] != [self.width, self.height] {
            (self.surface, self.displayed, self.depth) =
                housing_targets(device, batch, width, height);
            self.width = width;
            self.height = height;
        }
        true
    }
    pub fn set_has_radar(&mut self, has_radar: bool) -> Option<RadarAvailabilityCue> {
        self.presentation.set_has_radar(has_radar)
    }
    /// Whether the housing is opening or closing: `653100` steps it on the
    /// first draw after each four wall-clock buckets.
    pub fn housing_moving(&self) -> bool {
        self.presentation.housing_moving()
    }
    /// Native 653100 samples wall time once per draw, independent of game speed.
    /// Returns whether the housing finished opening or closing in this draw.
    pub fn tick(&mut self, wall_ms: u64) -> bool {
        self.presentation.draw(wall_ms)
    }
    /// Keep the online draw: the minimap's `map` picture through `map_quads`,
    /// then `white` tinted through the solid quads, as drawn from `origin`.
    pub fn retain_online_content(
        &mut self,
        map: &BatchTexture,
        white: &BatchTexture,
        map_quads: &[SpriteInstance],
        solid_quads: impl Iterator<Item = SpriteInstance>,
        origin: [f32; 2],
    ) {
        if self
            .presentation
            .retain_online_content(map_quads, solid_quads, origin)
        {
            self.online = Some((map.clone(), white.clone()));
        }
    }
    pub fn is_minimap_visible(&self) -> bool {
        self.phase() == RadarAnimPhase::Online && self.presentation.movie.is_none()
    }
    ///653963->656CB0(4),6578F0: one admission/prelude authority.
    pub fn admit_movie(&mut self) -> bool {
        self.presentation.admit_movie()
    }
    pub fn movie_command(&mut self, selected_done: bool, streams_ready: bool) -> RadarMovieCommand {
        self.presentation.movie_step(selected_done, streams_ready)
    }
    pub fn movie_ready_to_draw(&self) -> bool {
        self.presentation.movie == Some(MovieStage::Playback)
            && self.phase() == RadarAnimPhase::Online
    }
    pub fn movie_pause(&self) -> bool {
        self.presentation.movie_pause_latch
    }
    ///657C24 compares head+4 with14BC once at actual playback, including next
    ///head after retirement; enqueuing never acquires this pause.
    pub fn set_movie_pause(&mut self, paused: bool) {
        self.presentation.movie_pause_latch = paused;
    }
    pub fn finish_movie(&mut self, has_next: bool) {
        self.presentation.finish_movie(has_next);
    }
    pub fn retire_movies(&mut self) -> bool {
        let suspended = matches!(
            self.presentation.movie,
            Some(MovieStage::WaitAnnouncement | MovieStage::WaitStreams)
        );
        if self.presentation.movie.is_some() {
            self.presentation.finish_movie(false);
        }
        self.presentation.movie_pause_latch = false;
        suspended
    }
    /// Paint the movie's current `picture`, `size` pixels, and show it.
    pub fn paint_movie(&mut self, picture: &BatchTexture, size: [u32; 2]) {
        self.presentation.paint_movie(picture, size);
    }
    pub fn phase(&self) -> RadarAnimPhase {
        self.presentation.phase()
    }
    /// The surface as last shown.
    pub fn texture(&self) -> &BatchTexture {
        &self.displayed
    }
    /// Replay the paints since the last replay into the surface, copying it
    /// to the shown texture at each show. Encode it before the sidebar draws
    /// [`Self::texture`], and after the minimap's composition, which the
    /// online paints sample.
    pub fn encode_paints(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let paints = std::mem::take(&mut self.presentation.paints);
        if paints.is_empty() {
            return;
        }
        // Every draw's quads go up together; each draw reads its own range.
        let size = [self.width as f32, self.height as f32];
        let mut quads = Vec::new();
        let mut steps = Vec::new();
        let mut origins = vec![[0.0; 2]];
        let mut draw = |steps: &mut Vec<ReplayStep>,
                        texture: &BatchTexture,
                        camera: usize,
                        new: &[SpriteInstance]| {
            if !new.is_empty() {
                steps.push(ReplayStep::Draw {
                    texture: texture.clone(),
                    camera,
                    first: quads.len() as u32,
                    count: new.len() as u32,
                });
                quads.extend_from_slice(new);
            }
        };
        for paint in paints {
            match paint {
                RadarPaint::Clear => steps.push(ReplayStep::Clear),
                RadarPaint::Show => steps.push(ReplayStep::Show),
                RadarPaint::Frame(frame) => {
                    draw(
                        &mut steps,
                        &self.frames[frame],
                        0,
                        &[picture_quad([0.0; 2], size)],
                    );
                }
                RadarPaint::Movie {
                    picture,
                    size: movie,
                } => {
                    let left = self.width.saturating_sub(movie[0]) / 2;
                    let quad = picture_quad([left as f32, 0.0], movie.map(|v| v as f32));
                    draw(&mut steps, &picture, 0, &[quad]);
                }
                RadarPaint::Online {
                    map_quads,
                    solid_quads,
                    origin,
                } => {
                    draw(
                        &mut steps,
                        &self.frames[32],
                        0,
                        &[picture_quad([0.0; 2], size)],
                    );
                    if let Some((map, white)) = &self.online {
                        origins.push(origin);
                        let camera = origins.len() - 1;
                        draw(&mut steps, map, camera, &map_quads);
                        draw(&mut steps, white, camera, &solid_quads);
                    }
                }
            }
        }
        while self.cameras.len() < origins.len() {
            self.cameras.push(batch.create_ui_camera(device));
        }
        let ui = batch.ui_camera_uniform();
        for ((buffer, _), origin) in self.cameras.iter().zip(&origins) {
            let camera = CameraUniform {
                screen_size: size,
                camera_pos: *origin,
                zoom: 1.0,
                ..ui
            };
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(&camera));
        }
        if !quads.is_empty() {
            self.quads.write(
                device,
                queue,
                "Radar housing quads",
                bytemuck::cast_slice(&quads),
            );
        }
        let mut segments = steps
            .split(|step| matches!(step, ReplayStep::Show))
            .peekable();
        while let Some(segment) = segments.next() {
            self.encode_segment(batch, encoder, segment);
            if segments.peek().is_some() {
                encoder.copy_texture_to_texture(
                    self.surface.view.texture().as_image_copy(),
                    self.displayed.view.texture().as_image_copy(),
                    wgpu::Extent3d {
                        width: self.width,
                        height: self.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }
    /// One render pass over the surface for steps between shows. A clear
    /// discards whatever came before it.
    fn encode_segment(
        &self,
        batch: &BatchRenderer,
        encoder: &mut wgpu::CommandEncoder,
        segment: &[ReplayStep],
    ) {
        let (load, steps) = match segment
            .iter()
            .rposition(|step| matches!(step, ReplayStep::Clear))
        {
            Some(clear) => (
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                &segment[clear + 1..],
            ),
            None if segment.is_empty() => return,
            None => (wgpu::LoadOp::Load, segment),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Radar housing"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.surface.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            // The direct stores neither test nor write it.
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(crate::render::native_z::STORED_DEPTH_CLEAR),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some(quads) = self.quads.buffer() else {
            return;
        };
        for step in steps {
            if let ReplayStep::Draw {
                texture,
                camera,
                first,
                count,
            } = step
            {
                batch.draw_passthrough_range_bound(
                    &mut pass,
                    texture,
                    quads,
                    &self.cameras[*camera].1,
                    *first,
                    *count,
                );
            }
        }
    }
}

/// The housing frames' textures; only frames 0..=32 are drawn.
fn frame_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
) -> Vec<BatchTexture> {
    frames
        .iter()
        .take(33)
        .map(|frame| batch.create_texture_on_device(device, queue, frame, width, height, None))
        .collect()
}

/// The surface, the texture shown from it, and the depth the batch pipelines
/// need beside them.
fn housing_targets(
    device: &wgpu::Device,
    batch: &BatchRenderer,
    width: u32,
    height: u32,
) -> (BatchTexture, BatchTexture, wgpu::TextureView) {
    let format = batch.target_format();
    let (_, depth) =
        crate::render::scaled_copy::create_depth_target(device, "Radar housing", (width, height));
    (
        batch.create_render_target_texture(device, width, height, format),
        batch.create_render_target_texture(device, width, height, format),
        depth,
    )
}

#[cfg(test)]
#[path = "availability_tests.rs"]
mod availability_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    fn housing() -> Vec<Vec<u8>> {
        (0..33).map(|i| [i * 5, 12, 88, 255].repeat(2)).collect()
    }
    /// Executed6531DB controls for native phase4/5: at100+3 buckets no
    ///movement; at100+4,25->26 and31->32/phase1. Synthetic colors here only
    ///exercise the owner, never certify retail shape/pixel equivalence.
    #[test]
    fn native_movie_housing_controls_and_pending_restore() {
        for phase in [RadarAnimPhase::MovieOpening, RadarAnimPhase::MovieRestoring] {
            let mut a = RadarAnimation {
                phase,
                frame: 25,
                last_bucket: 100,
                duration: 4,
            };
            assert!(!a.advance_draw(1_663));
            assert_eq!(a.frame, 25);
            assert!(a.advance_draw(1_664));
            assert_eq!((a.frame, a.last_bucket, a.duration), (26, 104, 4));
            a.frame = 31;
            assert!(a.advance_draw(1_728));
            assert_eq!((a.frame, a.phase), (32, RadarAnimPhase::Online));
        }
        let mut radar = RadarPresentation::new(&housing(), 2, 1).unwrap();
        let _ = radar.set_has_radar(true);
        radar.animation.phase = RadarAnimPhase::Online;
        radar.animation.frame = 32;
        assert!(radar.admit_movie());
        assert!(!radar.admit_movie());
        assert_eq!(radar.movie_step(true, true), RadarMovieCommand::Announce);
        assert_eq!(radar.movie_step(false, true), RadarMovieCommand::None);
        assert_eq!(
            radar.movie_step(true, false),
            RadarMovieCommand::PrepareStreams
        );
        assert_eq!(radar.movie_step(true, false), RadarMovieCommand::None);
        assert_eq!(
            radar.movie_step(true, true),
            RadarMovieCommand::StartPlayback
        );
        let _ = radar.set_has_radar(false);
        assert_eq!(radar.movie, Some(MovieStage::Playback));
        radar.movie_pause_latch = true;
        radar.finish_movie(true);
        assert!(
            radar.movie_pause_latch,
            "retiring117 keeps latch until the next head draw"
        );
        assert_eq!(
            (radar.animation.phase, radar.animation.frame),
            (RadarAnimPhase::MovieOpening, 25)
        );
        radar.finish_movie(false);
        assert!(!radar.movie_pause_latch);
        assert_eq!(radar.animation.phase, RadarAnimPhase::Closing);
    }
    /// `653100` ends the radar's sound loop only on the draw that finishes
    /// the housing: MovieOpening reaching 32 (`0x0065328C`) and Closing
    /// reaching 0 (`0x00653267`), each once; a draw that is not due reports
    /// nothing.
    #[test]
    fn the_housing_settles_once_on_its_final_frame() {
        for (phase, frame) in [
            (RadarAnimPhase::MovieOpening, 31),
            (RadarAnimPhase::Closing, 1),
        ] {
            let mut radar = RadarPresentation::new(&housing(), 2, 1).unwrap();
            radar.animation = RadarAnimation {
                phase,
                frame,
                last_bucket: 100,
                duration: 4,
            };
            assert!(!radar.draw(1_663), "not due");
            assert!(radar.draw(1_664), "{phase:?} settles");
            assert!(!radar.draw(1_728), "settled housing stays");
        }
    }
    /// Native656DF0 suppresses equal availability before direction admission;
    ///656BE0 preserves the housing timer during both reversal arms.
    #[test]
    fn ordinary_availability_emits_only_admitted_direction_cues() {
        use RadarAvailabilityCue::{Off, On};
        let mut radar = RadarPresentation::new(&housing(), 2, 1).unwrap();
        assert_eq!(radar.set_has_radar(false), None);
        assert_eq!(radar.set_has_radar(true), Some(On));
        assert_eq!(radar.set_has_radar(true), None);
        assert!(radar.advance_draw(1600));
        let timer = (radar.animation.last_bucket, radar.animation.duration);
        let frame = radar.animation.frame;
        assert_eq!(radar.set_has_radar(false), Some(Off));
        assert_eq!(radar.set_has_radar(false), None);
        assert_eq!(radar.set_has_radar(true), Some(On));
        assert_eq!(radar.animation.phase, RadarAnimPhase::Opening);
        assert_eq!(radar.animation.frame, frame);
        assert_eq!(
            (radar.animation.last_bucket, radar.animation.duration),
            timer
        );

        assert!(radar.admit_movie());
        assert_eq!(radar.set_has_radar(false), None, "mode3/4 defers silently");
        radar.finish_movie(false);
        assert_eq!(radar.set_has_radar(false), None);
        assert_eq!(radar.animation.phase, RadarAnimPhase::Closing);
    }

    /// Original House508DF0 reaches the sole setters on these native local
    /// edges. House provider admission is covered by its existing core oracle;
    /// this comparison consumes native setter inputs, including BL-only bools.
    #[test]
    fn original_house_radar_edges_match_admitted_cues_and_retained_timer() {
        let native = crate::test_fixture::json("tools/input_oracle/radar_availability_audio.json");
        let mut compared = 0;
        for row in native["rows"].as_array().unwrap() {
            let initial = &row["initial"]["radar"];
            let mut radar = RadarPresentation::new(&housing(), 2, 1).unwrap();
            radar.pending_radar = initial["available"].as_bool().unwrap();
            radar.animation.phase = phase(initial["phase"].as_i64().unwrap());
            radar.animation.frame = initial["frame"].as_u64().unwrap() as usize;
            radar.animation.last_bucket = initial["timer"][0].as_i64().unwrap() as i32;
            radar.animation.duration = initial["timer"][2].as_i64().unwrap() as i32;
            for boundary in row["phases"].as_array().unwrap() {
                let calls = boundary["calls"].as_array().unwrap();
                let actual: Vec<_> = calls
                    .iter()
                    .filter(|call| call["pc"] == 0x656df0_u64)
                    .filter_map(|call| {
                        // Native656DF8 reads BL; the caller's high bits are
                        // retained register contents even for a false byte.
                        radar.set_has_radar(call["args"][0].as_u64().unwrap() & 0xff != 0)
                    })
                    .collect();
                let expected: Vec<_> = calls
                    .iter()
                    .filter(|call| call["pc"] == 0x750920_u64)
                    .map(|call| match call["return_pc"].as_u64().unwrap() {
                        0x656c37 => RadarAvailabilityCue::On,
                        0x656c82 => RadarAvailabilityCue::Off,
                        pc => panic!("unexpected original radar request return {pc:x}"),
                    })
                    .collect();
                assert_eq!(actual, expected, "{} {}", row["name"], boundary["label"]);
                let after = &boundary["after"]["radar"];
                assert_eq!(radar.pending_radar, after["available"].as_bool().unwrap());
                assert_eq!(
                    radar.animation.phase,
                    phase(after["phase"].as_i64().unwrap())
                );
                assert_eq!(
                    radar.animation.frame,
                    after["frame"].as_u64().unwrap() as usize
                );
                assert_eq!(
                    radar.animation.last_bucket,
                    after["timer"][0].as_i64().unwrap() as i32
                );
                assert_eq!(
                    radar.animation.duration,
                    after["timer"][2].as_i64().unwrap() as i32
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 20);
    }

    /// The paints since the last look, by kind.
    #[derive(Debug, PartialEq)]
    enum Painted {
        Clear,
        Frame(usize),
        Online,
        Show,
    }
    fn painted(radar: &mut RadarPresentation) -> Vec<Painted> {
        std::mem::take(&mut radar.paints)
            .into_iter()
            .map(|paint| match paint {
                RadarPaint::Clear => Painted::Clear,
                RadarPaint::Frame(frame) => Painted::Frame(frame),
                RadarPaint::Online { .. } => Painted::Online,
                RadarPaint::Show => Painted::Show,
                RadarPaint::Movie { .. } => panic!("no movie plays here"),
            })
            .collect()
    }
    fn online_snapshot(radar: &mut RadarPresentation) {
        let map = SpriteInstance {
            size: [2., 1.],
            uv_size: [1.; 2],
            tint: [1.; 3],
            alpha: 1.,
            ..Default::default()
        };
        radar.retain_online_content(&[map], std::iter::empty(), [0.; 2]);
    }
    /// The GPU replay's pixels: `sidebar_gpu_tests`.
    #[test]
    fn production_radar_owner_keeps_predue_map_and_rebuilds_online_base() {
        use Painted::{Frame, Online, Show};
        let mut radar = RadarPresentation::new(&housing(), 2, 1).unwrap();
        assert_eq!(painted(&mut radar), [Painted::Clear, Frame(0), Show]);
        let _ = radar.set_has_radar(true);
        for frame in 1..=32 {
            assert!(radar.advance_draw((frame - 1) * 64));
            assert_eq!(painted(&mut radar), [Frame(frame as usize), Show]);
        }
        assert_eq!(radar.phase(), RadarAnimPhase::Online);
        online_snapshot(&mut radar);
        let _ = radar.set_has_radar(false);
        assert!(
            radar.advance_draw(2032),
            "show the retained map before timer is due"
        );
        assert_eq!(painted(&mut radar), [Online, Show]);
        online_snapshot(&mut radar);
        assert_eq!(
            painted(&mut radar),
            [],
            "internal map changes cannot overwrite closing backing"
        );
        let _ = radar.set_has_radar(true);
        assert!(!radar.advance_draw(2032));
        assert!(
            radar.advance_draw(2048),
            "reconstruct Online base when the due transition completes"
        );
        assert_eq!(radar.phase(), RadarAnimPhase::Online);
        assert_eq!(
            painted(&mut radar),
            [Frame(32), Show],
            "old viewport lines cannot survive behind a new online draw"
        );
        online_snapshot(&mut radar);
        let _ = radar.set_has_radar(false);
        assert!(radar.advance_draw(2112));
        assert_eq!(painted(&mut radar), [Online, Frame(31), Show]);
        assert_eq!(radar.phase(), RadarAnimPhase::Closing);
        for frame in (0..31).rev() {
            assert!(radar.advance_draw(2112 + (31 - frame) as u64 * 64));
            assert_eq!(painted(&mut radar), [Frame(frame), Show]);
        }
        assert_eq!(radar.phase(), RadarAnimPhase::Offline);
    }
    #[test]
    fn source_replacement_preserves_current_frame_timer_and_skipped_pixels() {
        // Source installation at 652E90 does not reset the animation fields.
        // The development owner switch forces a source redraw; this regression
        // exercises that VERA interaction, not an invented native owner switch.
        let mut radar = RadarPresentation::new(&housing(), 2, 1).unwrap();
        let _ = radar.set_has_radar(true);
        assert!(radar.advance_draw(1000));
        assert_eq!(radar.animation.frame, 1);
        painted(&mut radar);
        let replacement = vec![[60, 120, 180, 255].repeat(2); 33];
        assert!(radar.replace_frames(&replacement, 2, 1));
        assert_eq!(radar.phase(), RadarAnimPhase::Opening);
        assert_eq!(radar.animation.frame, 1);
        // Painted over the old surface, whose pixels its skips keep.
        assert_eq!(painted(&mut radar), [Painted::Frame(1)]);
        assert!(
            radar.advance_draw(1016),
            "show the replaced source before timer is due"
        );
        assert_eq!(painted(&mut radar), [Painted::Show]);
        assert_eq!(radar.animation.frame, 1);
        assert!(!radar.advance_draw(1040));
        assert!(
            radar.advance_draw(1056),
            "retain original 16ms bucket alignment"
        );
        assert_eq!(radar.animation.frame, 2);
        assert_eq!(painted(&mut radar), [Painted::Frame(2), Painted::Show]);
        assert!(!radar.replace_frames(&vec![vec![0; 8]; 32], 2, 1));
        assert_eq!(painted(&mut radar), []);
        assert_eq!(radar.animation.frame, 2);
        let _ = radar.set_has_radar(false);
        assert!(!radar.advance_draw(1072));
        assert!(radar.advance_draw(1120));
        assert_eq!(
            (radar.phase(), radar.animation.frame),
            (RadarAnimPhase::Closing, 1)
        );
    }

    #[derive(Deserialize)]
    struct Packet {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        input: [i64; 5],
        output: [i64; 5],
    }
    fn phase(n: i64) -> RadarAnimPhase {
        match n {
            0 => RadarAnimPhase::Offline,
            1 => RadarAnimPhase::Online,
            2 => RadarAnimPhase::Closing,
            3 => RadarAnimPhase::Opening,
            _ => panic!("bad phase"),
        }
    }
    #[test]
    fn original_600_draw_timer_states_match() {
        let packet: Packet = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/radar_timer.json",
        ))
        .unwrap();
        assert_eq!(packet.cases.len(), 600);
        for c in packet.cases {
            let mut a = RadarAnimation {
                phase: phase(c.input[0]),
                frame: c.input[1] as usize,
                last_bucket: c.input[2] as i32,
                duration: c.input[3] as i32,
            };
            a.advance_draw(c.input[4] as u64);
            assert_eq!(
                (a.phase, a.frame, a.last_bucket, a.duration),
                (
                    phase(c.output[0]),
                    c.output[1] as usize,
                    c.output[2] as i32,
                    c.output[3] as i32
                ),
                "{:?}",
                c.input
            );
        }
    }
    #[test]
    fn availability_reversal_keeps_draw_timer_and_stalls_do_not_catch_up() {
        let mut a = RadarAnimation::default();
        let _ = a.set_has_radar(true);
        assert!(a.advance_draw(1000));
        assert_eq!(a.frame, 1);
        let _ = a.set_has_radar(false);
        assert!(!a.advance_draw(1016));
        assert!(a.advance_draw(1056));
        assert_eq!((a.frame, a.phase), (0, RadarAnimPhase::Offline));
        let _ = a.set_has_radar(true);
        assert!(!a.advance_draw(1072));
        assert!(a.advance_draw(1_000_000));
        assert_eq!(a.frame, 1);
    }
}
