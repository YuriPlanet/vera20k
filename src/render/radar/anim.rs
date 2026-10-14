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
    pub fn set_has_radar(&mut self, available: bool) {
        match (available, self.phase) {
            (true, RadarAnimPhase::Offline | RadarAnimPhase::Closing) => {
                self.phase = RadarAnimPhase::Opening
            }
            (
                false,
                RadarAnimPhase::Online
                | RadarAnimPhase::Opening
                | RadarAnimPhase::MovieOpening
                | RadarAnimPhase::MovieRestoring,
            ) => self.phase = RadarAnimPhase::Closing,
            _ => {}
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
    pub(in crate::render) fn set_has_radar(&mut self, has_radar: bool) {
        self.pending_radar = has_radar;
        // SetMode656CB0 defers ordinary changes into +14B4 during modes3/4.
        if self.movie.is_some() {
            return;
        }
        // 656DF0/656BE0 do not clear/dirty the native surface. A loss before
        // the next due draw must expose the last displayed map, not frame32.
        self.needs_upload |= !has_radar && self.animation.phase == RadarAnimPhase::Online;
        self.animation.set_has_radar(has_radar);
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
        let (texture_raw, texture) =
            batch.create_updatable_texture(gpu, &presentation.surface.rgba, width, height);
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
            let (raw, texture) =
                batch.create_updatable_texture(gpu, &self.presentation.surface.rgba, width, height);
            self.texture_raw = raw;
            self.texture = texture;
            self.width = width;
            self.height = height;
        }
        true
    }
    pub fn set_has_radar(&mut self, has_radar: bool) {
        self.presentation.set_has_radar(has_radar);
    }
    /// Native 653100 samples wall time once per draw, independent of game speed.
    pub fn tick(&mut self, gpu: &GpuContext, wall_ms: u64) {
        if self.presentation.advance_draw(wall_ms) {
            self.upload_frame(gpu);
        }
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
        radar.set_has_radar(true);
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
        radar.set_has_radar(false);
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
        radar.set_has_radar(true);
        for frame in 1..=32 {
            assert!(radar.advance_draw((frame - 1) * 64));
            assert_eq!(radar.surface.rgba, frames[frame as usize]);
        }
        assert_eq!(radar.phase(), RadarAnimPhase::Online);
        online_snapshot(&mut radar, [20, 40, 60]);
        let displayed = radar.surface.rgba.clone();
        radar.set_has_radar(false);
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
        radar.set_has_radar(true);
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
        radar.set_has_radar(false);
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
        radar.set_has_radar(true);
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
        radar.set_has_radar(false);
        assert!(!radar.advance_draw(1072));
        assert!(radar.advance_draw(1120));
        assert_eq!(
            (radar.phase(), radar.animation.frame),
            (RadarAnimPhase::Closing, 1)
        );
    }

    #[test]
    #[ignore = "timing probe; one fixed-size UI copy, independent of entity count"]
    fn retained_radar_online_copy_timing() {
        use std::{hint::black_box, time::Instant};
        let width = 168;
        let height = 110;
        let mut radar = RadarPresentation::new(
            vec![vec![255; width * height * 4]; 33],
            width as u32,
            height as u32,
        )
        .unwrap();
        radar.set_has_radar(true);
        for i in 0..32 {
            radar.advance_draw(i * 64);
        }
        let source = [192, 168, 144, 255].repeat(140 * 108);
        let mut map = SpriteInstance {
            position: [648.25, 49.75],
            size: [140., 108.],
            uv_size: [1.; 2],
            tint: [1.; 3],
            alpha: 1.,
            ..Default::default()
        };
        let line = SpriteInstance {
            position: [650.25, 54.75],
            size: [120., 1.],
            tint: crate::render::sidebar_text::native_radar_outline_color(
                crate::render::sidebar_chrome::SidebarTheme::Allied,
            ),
            uv_size: [1.; 2],
            alpha: 1.,
            ..Default::default()
        };
        for _ in 0..100 {
            radar.retain_online_content(
                &source,
                [140, 108],
                &[map],
                [line].into_iter(),
                [632.25, 48.75],
            );
        }
        let mut samples = Vec::with_capacity(2000);
        for i in 0..2000 {
            map.position[0] = 648.25 + (i % 3) as f32;
            let start = Instant::now();
            radar.retain_online_content(
                black_box(&source),
                [140, 108],
                &[map],
                [line].into_iter(),
                [632.25, 48.75],
            );
            black_box(&radar.surface.rgba);
            samples.push(start.elapsed().as_secs_f64() * 1e6);
        }
        samples.sort_by(f64::total_cmp);
        eprintln!(
            "Radar online retained copy: 2000 calls, mean {:.3}us, p50 {:.3}us, p95 {:.3}us, max {:.3}us; fixed168x110 once per UI frame, no per-unit calls",
            samples.iter().sum::<f64>() / samples.len() as f64,
            samples[1000],
            samples[1900],
            samples[1999]
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
        a.set_has_radar(true);
        assert!(a.advance_draw(1000));
        assert_eq!(a.frame, 1);
        a.set_has_radar(false);
        assert!(!a.advance_draw(1016));
        assert!(a.advance_draw(1056));
        assert_eq!((a.frame, a.phase), (0, RadarAnimPhase::Offline));
        a.set_has_radar(true);
        assert!(!a.advance_draw(1072));
        assert!(a.advance_draw(1_000_000));
        assert_eq!(a.frame, 1);
    }
}
