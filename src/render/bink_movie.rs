//! Bink movie surface for shell playback.

use std::sync::Arc;
use std::time::Duration;

use crate::assets::bink_decode::{BinkDecoder, BinkFrame, ColorRange};
use crate::assets::bink_file::BinkFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;

pub enum BinkMovieStep {
    Unchanged,
    FrameUploaded,
    Ended,
}

pub struct BinkMovieSurface {
    file: BinkFile,
    decoder: BinkDecoder,
    current_frame: usize,
    clock: BinkFrameClock,
    texture: wgpu::Texture,
    batch_texture: BatchTexture,
    rgba: Vec<u8>,
    looping: bool,
    source_archive: String,
}

impl BinkMovieSurface {
    /// `filter` samples the frame texture when a draw scales it.
    pub fn from_bytes(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        bytes: Arc<[u8]>,
        source_archive: String,
        looping: bool,
        filter: wgpu::FilterMode,
    ) -> Result<Self, crate::assets::error::AssetError> {
        let file = BinkFile::parse(bytes)?;
        let mut decoder = BinkDecoder::new(&file.header)?;
        let first_packet = file.video_packet(0)?;
        let frame = decoder.decode_frame(first_packet)?;
        let rgba = frame_to_rgba(frame);
        let (texture, batch_texture) = batch.create_updatable_texture(
            &gpu.device,
            &gpu.queue,
            &rgba,
            file.header.width,
            file.header.height,
            filter,
        );
        Ok(Self {
            file,
            decoder,
            current_frame: 1,
            clock: BinkFrameClock::default(),
            texture,
            batch_texture,
            rgba,
            looping,
            source_archive,
        })
    }

    pub fn batch_texture(&self) -> &BatchTexture {
        &self.batch_texture
    }

    pub fn width(&self) -> u32 {
        self.file.header.width
    }

    pub fn height(&self) -> u32 {
        self.file.header.height
    }

    pub fn fps(&self) -> f64 {
        self.file.header.fps()
    }

    pub fn frame_count(&self) -> usize {
        self.file.frame_index.len()
    }

    pub fn source_archive(&self) -> &str {
        &self.source_archive
    }

    /// Index of the next frame `step` will decode (frame 0 is decoded on
    /// construction).
    pub fn next_frame(&self) -> usize {
        self.current_frame
    }

    pub fn file(&self) -> &BinkFile {
        &self.file
    }

    /// BinkPause(0) at 0x1000A5C5 invalidates the clock; the next BinkWait
    /// starts timing from the frame already on screen.
    pub(crate) fn resume_clock(&mut self) {
        self.clock.reanchor(self.current_frame);
    }

    pub fn step(
        &mut self,
        gpu: &GpuContext,
        elapsed_secs: f64,
    ) -> Result<BinkMovieStep, crate::assets::error::AssetError> {
        self.step_with_sound(gpu, elapsed_secs, MovieSoundTiming::Silent)
    }

    pub(crate) fn step_with_sound(
        &mut self,
        gpu: &GpuContext,
        elapsed_secs: f64,
        sound: MovieSoundTiming,
    ) -> Result<BinkMovieStep, crate::assets::error::AssetError> {
        let elapsed = Duration::try_from_secs_f64(elapsed_secs).unwrap_or_default();
        let due = self.clock.frames_due(
            elapsed,
            self.current_frame,
            self.file.header.fps_num,
            self.file.header.fps_den,
            sound,
        );
        self.advance_frames(gpu, due)
    }

    /// Decode in order and upload the last of the requested frames.
    pub(crate) fn advance_frames(
        &mut self,
        gpu: &GpuContext,
        due: usize,
    ) -> Result<BinkMovieStep, crate::assets::error::AssetError> {
        let mut changed = false;
        for i in 0..due {
            if self.current_frame >= self.frame_count() {
                if self.looping {
                    self.restart_at_original_frame_one()?;
                    changed = true;
                    break;
                }
                // Upload the final decoded frame before reporting completion
                // on a later step. Catch-up must not discard its last store.
                if i == 0 {
                    return Ok(BinkMovieStep::Ended);
                }
                break;
            }
            let pkt = self.file.video_packet(self.current_frame)?;
            match self.decoder.decode_frame(pkt) {
                Ok(_) => changed = true,
                // Two retail clips contain a single rejected picture. The decoder
                // keeps its last good reference; audio and subtitles keep this slot.
                Err(err) => log::warn!("Skipping Bink video frame {}: {err}", self.current_frame),
            }
            self.current_frame += 1;
        }
        if changed {
            self.upload_current_frame(gpu);
            Ok(BinkMovieStep::FrameUploaded)
        } else {
            Ok(BinkMovieStep::Unchanged)
        }
    }

    fn upload_current_frame(&mut self, gpu: &GpuContext) {
        // Intervening frames supply prediction data. Only the displayed frame
        // needs RGB pixels, even when several frames were due on this redraw.
        write_frame_rgba(&self.decoder.cur, &mut self.rgba);
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width() * 4),
                rows_per_image: Some(self.height()),
            },
            wgpu::Extent3d {
                width: self.width(),
                height: self.height(),
                depth_or_array_layers: 1,
            },
        );
    }

    fn restart_at_original_frame_one(&mut self) -> Result<(), crate::assets::error::AssetError> {
        self.decoder.flush();
        let pkt = self.file.video_packet(0)?;
        self.decoder.decode_frame(pkt)?;
        self.current_frame = 1;
        self.clock = BinkFrameClock::default();
        Ok(())
    }
}

pub fn frame_to_rgba(frame: &BinkFrame) -> Vec<u8> {
    let mut out = vec![0u8; frame.width as usize * frame.height as usize * 4];
    write_frame_rgba(frame, &mut out);
    out
}

fn write_frame_rgba(frame: &BinkFrame, out: &mut [u8]) {
    let w = frame.width as usize;
    let h = frame.height as usize;

    for y in 0..h {
        for x in 0..w {
            let yv = frame.y[y * frame.stride_y + x] as i32;
            let uv_off = (y / 2) * frame.stride_uv + (x / 2);
            let u = frame.u[uv_off] as i32;
            let v = frame.v[uv_off] as i32;
            let (r, g, b) = match frame.color_range {
                ColorRange::Mpeg => yuv_to_rgb_mpeg(yv, u, v),
                ColorRange::Jpeg => yuv_to_rgb_jpeg(yv, u, v),
            };
            let base = (y * w + x) * 4;
            out[base] = r;
            out[base + 1] = g;
            out[base + 2] = b;
            out[base + 3] = 255;
        }
    }
}

pub(crate) enum MovieSoundTiming {
    Silent,
    Playing,
    /// Real PCM delivered before the mixer started inserting silence.
    Underrun(Duration),
}

/// BINKW32 BinkWait (0x1000A3C0) compares integer millisecond deadlines
/// measured from one clock origin. Frame zero was decoded when the movie opened.
#[derive(Default)]
pub(crate) struct BinkFrameClock {
    elapsed: Duration,
    baseline_frame: usize,
}

impl BinkFrameClock {
    fn reanchor(&mut self, decoded_frames: usize) {
        self.elapsed = Duration::ZERO;
        self.baseline_frame = decoded_frames - 1;
    }

    pub(crate) fn frames_due(
        &mut self,
        elapsed: Duration,
        decoded_frames: usize,
        fps_num: u32,
        fps_den: u32,
        sound: MovieSoundTiming,
    ) -> usize {
        if let MovieSoundTiming::Underrun(position) = sound {
            self.elapsed = position;
            self.baseline_frame = 0;
            return self.ready_frames(decoded_frames, fps_num, fps_den);
        }
        self.elapsed = self.elapsed.saturating_add(elapsed);
        let frame = decoded_frames - self.baseline_frame;
        let period_numerator = 1000 * u128::from(fps_den);
        let deadline = |n: usize| n as u128 * period_numerator / u128::from(fps_num);
        if matches!(sound, MovieSoundTiming::Silent)
            && self.elapsed.as_millis().saturating_sub(deadline(frame)) > deadline(2)
        {
            // BinkWait 0x1000A4B5..0x1000A4DD: if a silent movie is over two
            // frames late, show the next frame and reanchor at decoded_count - 1.
            self.reanchor(decoded_frames);
            return 1;
        }
        self.ready_frames(decoded_frames, fps_num, fps_den).min(4)
    }

    fn ready_frames(&self, decoded_frames: usize, fps_num: u32, fps_den: u32) -> usize {
        // BinkWait rounds each deadline down to whole milliseconds. The first
        // frame with a deadline after now is ceil((now_ms + 1) * fps / 1000).
        let next = ((self.elapsed.as_millis() + 1) * u128::from(fps_num))
            .div_ceil(1000 * u128::from(fps_den));
        let ready = next.saturating_sub((decoded_frames - self.baseline_frame) as u128);
        usize::try_from(ready).unwrap_or(usize::MAX)
    }
}

#[inline]
fn clip(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

#[inline]
fn yuv_to_rgb_mpeg(y: i32, u: i32, v: i32) -> (u8, u8, u8) {
    let c = (y - 16) * 298;
    let d = u - 128;
    let e = v - 128;
    (
        clip((c + 409 * e + 128) >> 8),
        clip((c - 100 * d - 208 * e + 128) >> 8),
        clip((c + 516 * d + 128) >> 8),
    )
}

#[inline]
fn yuv_to_rgb_jpeg(y: i32, u: i32, v: i32) -> (u8, u8, u8) {
    let d = u - 128;
    let e = v - 128;
    (
        clip(y + ((359 * e + 128) >> 8)),
        clip(y + ((-88 * d - 183 * e + 128) >> 8)),
        clip(y + ((454 * d + 128) >> 8)),
    )
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    #[test]
    fn mpeg_black_and_white() {
        assert_eq!(yuv_to_rgb_mpeg(16, 128, 128), (0, 0, 0));
        assert_eq!(yuv_to_rgb_mpeg(235, 128, 128), (255, 255, 255));
    }

    #[test]
    fn jpeg_black_and_white() {
        assert_eq!(yuv_to_rgb_jpeg(0, 128, 128), (0, 0, 0));
        assert_eq!(yuv_to_rgb_jpeg(255, 128, 128), (255, 255, 255));
    }

    #[test]
    fn frame_clock_uses_bink_wait_millisecond_deadlines() {
        let mut clock = BinkFrameClock::default();
        let ms = Duration::from_millis;
        assert_eq!(
            clock.frames_due(ms(65), 1, 15, 1, MovieSoundTiming::Silent),
            0
        );
        assert_eq!(
            clock.frames_due(ms(1), 1, 15, 1, MovieSoundTiming::Silent),
            1
        );
    }

    #[test]
    fn silent_movie_reanchors_only_beyond_bink_wait_lateness_allowance() {
        // BINKW32 BinkWait at 15 fps: the first deadline is 66 ms, and
        // 133 ms of lateness is still allowed. The next millisecond rebases.
        let ms = Duration::from_millis;
        let mut clock = BinkFrameClock::default();
        assert_eq!(
            clock.frames_due(ms(199), 1, 15, 1, MovieSoundTiming::Silent),
            2
        );
        let mut late = BinkFrameClock::default();
        assert_eq!(
            late.frames_due(ms(200), 1, 15, 1, MovieSoundTiming::Silent),
            1
        );
        assert_eq!(
            late.frames_due(ms(132), 2, 15, 1, MovieSoundTiming::Silent),
            0
        );
        assert_eq!(
            late.frames_due(ms(1), 2, 15, 1, MovieSoundTiming::Silent),
            1
        );
        let mut voiced = BinkFrameClock::default();
        assert_eq!(
            voiced.frames_due(ms(200), 1, 15, 1, MovieSoundTiming::Playing),
            3
        );
    }
}
