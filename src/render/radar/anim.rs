//! Ordinary radar animation and its displayed pixel history.
//!
//! `RadarPresentation` owns the frame/timer and retained pixels; the GPU
//! adapter uploads its result. Internal minimap updates remain independent.
//!
//! Native `652960` starts offline at frame 0 with a zero-duration timer.
//! `656BE0` changes direction without resetting that timer. `653100` advances
//! once per draw when four `timeGetTime() >> 4` buckets have elapsed, resets
//! from the current bucket, and clamps at frames 0/32 in that same draw.

use super::surface::RadarSurface;
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;

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

/// One owner for the displayed radar history, independent of GPU allocation.
/// Native 653100 paints into SidebarSurface; 656EC0 writes online content into
/// the same backing and suppresses those stores during transitions.
pub(in crate::render) struct RadarPresentation {
    animation: RadarAnimation,
    pub(in crate::render) surface: RadarSurface,
    needs_upload: bool,
    frames_rgba: Vec<Vec<u8>>,
    movie: Option<MovieStage>,
    pending_radar: bool,
    movie_pause_latch: bool,
}
impl RadarPresentation {
    pub(in crate::render) fn new(
        frames_rgba: Vec<Vec<u8>>,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let byte_count = width.checked_mul(height)?.checked_mul(4)? as usize;
        if frames_rgba.len() < 33
            || byte_count == 0
            || frames_rgba.iter().any(|frame| frame.len() != byte_count)
        {
            return None;
        }
        let mut surface = RadarSurface::new(width, height);
        surface.paint_frame(&frames_rgba[0]);
        Some(Self {
            animation: RadarAnimation::default(),
            surface,
            needs_upload: false,
            frames_rgba,
            movie: None,
            pending_radar: false,
            movie_pause_latch: false,
        })
    }
    /// Source installation is separate from animation timing (652E90).
    /// Paint the selected current frame as a forced source redraw; source0
    /// skips retain prior pixels, exactly as any other 4912B0 frame store.
    pub(super) fn replace_frames(&mut self, frames: Vec<Vec<u8>>, width: u32, height: u32) -> bool {
        let Some(byte_count) = width.checked_mul(height).and_then(|n| n.checked_mul(4)) else {
            return false;
        };
        if frames.len() < 33
            || byte_count == 0
            || frames.iter().any(|f| f.len() != byte_count as usize)
        {
            return false;
        }
        if self.surface.dimensions() != [width, height] {
            // VERA fallback for non-stock dimensional changes; no native
            // cross-size retained-history equivalence is asserted.
            self.surface = RadarSurface::new(width, height);
        }
        self.frames_rgba = frames;
        self.surface
            .paint_frame(&self.frames_rgba[self.animation.frame]);
        self.needs_upload = true;
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
        self.needs_upload |= !has_radar && self.animation.phase == RadarAnimPhase::Online;
        self.animation.set_has_radar(has_radar)
    }
    pub(in crate::render) fn advance_draw(&mut self, wall_ms: u64) -> bool {
        let old_phase = self.animation.phase;
        let changed = self.animation.advance_draw(wall_ms);
        // A closing->opening reversal at frame32 can finish at the same frame.
        // Reconstruct our continuously composed Online base before map/outline
        // draws. Native instead raises14DA and only repaints frame32 if Update's
        // dirty/event gates pass (65715C..6571C9,6575F3). If none pass, its final
        // online image is unchanged; this base is not a native store event.
        if changed || old_phase != self.animation.phase {
            self.surface
                .paint_frame(&self.frames_rgba[self.animation.frame]);
            self.needs_upload = true;
        }
        std::mem::take(&mut self.needs_upload)
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
    /// One `653100` draw: whether to upload, and whether the housing
    /// finished opening or closing in it, where native ends the radar's
    /// sound loop (`0x00653267` at frame 0, `0x0065328C` at frame 32).
    fn draw(&mut self, wall_ms: u64) -> (bool, bool) {
        let moving = self.housing_moving();
        let upload = self.advance_draw(wall_ms);
        (upload, moving && !self.housing_moving())
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
    /// Save the actual online content before the next availability change.
    /// Internal minimap updates continue while closed; they cannot replace this
    /// displayed backing until online again (`656EF2..6F0D`, `657578`).
    pub fn retain_online_content(
        &mut self,
        source: &[u8],
        source_size: [u32; 2],
        map_quads: &[SpriteInstance],
        solid_quads: impl Iterator<Item = SpriteInstance>,
        origin: [f32; 2],
    ) {
        if self.animation.phase != RadarAnimPhase::Online || self.movie.is_some() {
            return;
        }
        // Snapshot the currently displayed online draw, whose stock housing
        // endpoint is opaque. Do not accumulate trails from old viewport quads.
        self.surface.paint_frame(&self.frames_rgba[32]);
        for quad in map_quads {
            self.surface.paint_quad(quad, origin, source, source_size);
        }
        for quad in solid_quads {
            self.surface.paint_solid(&quad, origin);
        }
    }
}

/// GPU adapter for the one retained radar presentation owner.
pub struct RadarAnimState {
    presentation: RadarPresentation,
    texture: BatchTexture,
    texture_raw: wgpu::Texture,
    pub width: u32,
    pub height: u32,
}
impl RadarAnimState {
    /// Literal ordinary native endpoints are 0 and 32; malformed frames are
    /// rejected before constructing the texture.
    pub fn new(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        frames_rgba: Vec<Vec<u8>>,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let presentation = RadarPresentation::new(frames_rgba, width, height)?;
        let (texture_raw, texture) = batch.create_updatable_texture(
            &gpu.device,
            &gpu.queue,
            &presentation.surface.rgba,
            width,
            height,
            wgpu::FilterMode::Nearest,
        );
        Some(Self {
            presentation,
            texture,
            texture_raw,
            width,
            height,
        })
    }
    pub fn replace_frames(
        &mut self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        frames: Vec<Vec<u8>>,
        width: u32,
        height: u32,
    ) -> bool {
        if !self.presentation.replace_frames(frames, width, height) {
            return false;
        }
        if [width, height] != [self.width, self.height] {
            let (raw, texture) = batch.create_updatable_texture(
                &gpu.device,
                &gpu.queue,
                &self.presentation.surface.rgba,
                width,
                height,
                wgpu::FilterMode::Nearest,
            );
            self.texture_raw = raw;
            self.texture = texture;
            self.width = width;
            self.height = height;
        }
        true
    }
    pub fn set_has_radar(&mut self, has_radar: bool) -> Option<RadarAvailabilityCue> {
        self.presentation.set_has_radar(has_radar)
    }
    /// Native 653100 samples wall time once per draw, independent of game speed.
    /// Returns whether the housing finished opening or closing in this draw.
    pub fn tick(&mut self, gpu: &GpuContext, wall_ms: u64) -> bool {
        let (upload, settled) = self.presentation.draw(wall_ms);
        if upload {
            self.upload_frame(gpu);
        }
        settled
    }
    pub fn retain_online_content(
        &mut self,
        source: &[u8],
        source_size: [u32; 2],
        map_quads: &[SpriteInstance],
        solid_quads: impl Iterator<Item = SpriteInstance>,
        origin: [f32; 2],
    ) {
        self.presentation.retain_online_content(
            source,
            source_size,
            map_quads,
            solid_quads,
            origin,
        );
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
    /// Native movie copies into retained SidebarSurface. Use the stock 140x110
    /// aperture centered in its168x110 backing. Full native origin/final-paint
    /// driver equivalence is not established; no battlefield shader involved.
    pub fn paint_movie(&mut self, gpu: &GpuContext, frame: &[u8], size: [u32; 2]) {
        self.presentation.surface.paint_movie(frame, size);
        self.upload_frame(gpu);
    }
    pub fn phase(&self) -> RadarAnimPhase {
        self.presentation.phase()
    }
    pub fn texture(&self) -> &BatchTexture {
        &self.texture
    }
    /// Upload the current frame's RGBA data to the GPU texture.
    fn upload_frame(&self, gpu: &GpuContext) {
        let rgba: &[u8] = &self.presentation.surface.rgba;
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture_raw,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }
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
        let mut radar = RadarPresentation::new(housing(), 2, 1).unwrap();
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
            let mut radar = RadarPresentation::new(housing(), 2, 1).unwrap();
            radar.animation = RadarAnimation {
                phase,
                frame,
                last_bucket: 100,
                duration: 4,
            };
            assert!(!radar.draw(1_663).1, "not due");
            assert!(radar.draw(1_664).1, "{phase:?} settles");
            assert!(!radar.draw(1_728).1, "settled housing stays");
        }
    }
    /// Native656DF0 suppresses equal availability before direction admission;
    ///656BE0 preserves the housing timer during both reversal arms.
    #[test]
    fn ordinary_availability_emits_only_admitted_direction_cues() {
        use RadarAvailabilityCue::{Off, On};
        let mut radar = RadarPresentation::new(housing(), 2, 1).unwrap();
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
            let mut radar = RadarPresentation::new(housing(), 2, 1).unwrap();
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

    fn online_snapshot(radar: &mut RadarPresentation, rgb: [u8; 3]) {
        let q = SpriteInstance {
            position: [0.; 2],
            size: [2., 1.],
            uv_size: [1.; 2],
            tint: [1.; 3],
            alpha: 1.,
            ..Default::default()
        };
        radar.retain_online_content(
            &[rgb[0], rgb[1], rgb[2], 255].repeat(2),
            [2, 1],
            &[q],
            std::iter::empty(),
            [0.; 2],
        );
    }
    #[test]
    fn production_radar_owner_keeps_predue_map_and_rebuilds_online_base() {
        let frames = housing();
        let mut radar = RadarPresentation::new(frames.clone(), 2, 1).unwrap();
        let _ = radar.set_has_radar(true);
        for frame in 1..=32 {
            assert!(radar.advance_draw((frame - 1) * 64));
            assert_eq!(radar.surface.rgba, frames[frame as usize]);
        }
        assert_eq!(radar.phase(), RadarAnimPhase::Online);
        online_snapshot(&mut radar, [20, 40, 60]);
        let displayed = radar.surface.rgba.clone();
        let _ = radar.set_has_radar(false);
        assert!(
            radar.advance_draw(2032),
            "upload the retained map before timer is due"
        );
        assert_eq!(radar.surface.rgba, displayed);
        online_snapshot(&mut radar, [80, 90, 100]);
        assert_eq!(
            radar.surface.rgba, displayed,
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
            radar.surface.rgba, frames[32],
            "old viewport lines cannot survive behind a new online draw"
        );
        online_snapshot(&mut radar, [100, 120, 140]);
        let _ = radar.set_has_radar(false);
        assert!(radar.advance_draw(2112));
        assert_eq!(radar.surface.rgba, frames[31]);
        assert_eq!(radar.phase(), RadarAnimPhase::Closing);
        for frame in (0..31).rev() {
            assert!(radar.advance_draw(2112 + (31 - frame) as u64 * 64));
            assert_eq!(radar.surface.rgba, frames[frame]);
        }
        assert_eq!(radar.phase(), RadarAnimPhase::Offline);
    }
    #[test]
    fn source_replacement_preserves_current_frame_timer_and_skipped_pixels() {
        // Source installation at 652E90 does not reset the animation fields.
        // The development owner switch forces a source redraw; this regression
        // exercises that VERA interaction, not an invented native owner switch.
        let mut radar = RadarPresentation::new(housing(), 2, 1).unwrap();
        let _ = radar.set_has_radar(true);
        assert!(radar.advance_draw(1000));
        assert_eq!(radar.animation.frame, 1);
        let previous = radar.surface.rgba.clone();
        let mut replacement = vec![[60, 120, 180, 255].repeat(2); 33];
        replacement[1][3] = 0;
        assert!(radar.replace_frames(replacement.clone(), 2, 1));
        assert_eq!(radar.phase(), RadarAnimPhase::Opening);
        assert_eq!(radar.animation.frame, 1);
        assert_eq!(&radar.surface.rgba[..4], &previous[..4]);
        assert_eq!(&radar.surface.rgba[4..], &replacement[1][4..]);
        assert!(
            radar.advance_draw(1016),
            "upload the replaced source before timer is due"
        );
        assert_eq!(radar.animation.frame, 1);
        assert!(!radar.advance_draw(1040));
        assert!(
            radar.advance_draw(1056),
            "retain original 16ms bucket alignment"
        );
        assert_eq!(radar.animation.frame, 2);
        assert_eq!(radar.surface.rgba, replacement[2]);
        let before_invalid = radar.surface.rgba.clone();
        assert!(!radar.replace_frames(vec![vec![0; 8]; 32], 2, 1));
        assert_eq!(radar.surface.rgba, before_invalid);
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
