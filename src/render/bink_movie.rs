//! Bink movie surface for shell playback.

use std::sync::Arc;

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
    accumulator_secs: f64,
    texture: wgpu::Texture,
    batch_texture: BatchTexture,
    rgba: Vec<u8>,
    looping: bool,
    source_archive: String,
}

impl BinkMovieSurface {
    pub fn from_bytes(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        bytes: Arc<[u8]>,
        source_archive: String,
        looping: bool,
    ) -> Result<Self, crate::assets::error::AssetError> {
        let file = BinkFile::parse(bytes)?;
        let mut decoder = BinkDecoder::new(&file.header)?;
        let first_packet = file.video_packet(0)?;
        let frame = decoder.decode_frame(first_packet)?;
        let rgba = frame_to_rgba(frame);
        let (texture, batch_texture) =
            batch.create_updatable_texture(gpu, &rgba, file.header.width, file.header.height);
        Ok(Self {
            file,
            decoder,
            current_frame: 1,
            accumulator_secs: 0.0,
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

    pub fn step(
        &mut self,
        gpu: &GpuContext,
        elapsed_secs: f64,
    ) -> Result<BinkMovieStep, crate::assets::error::AssetError> {
        let fps = self.fps();
        if fps <= 0.0 {
            return Ok(BinkMovieStep::Unchanged);
        }
        let mut changed = false;
        for _ in 0..frames_due(&mut self.accumulator_secs, elapsed_secs, fps, 4) {
            if self.current_frame >= self.frame_count() {
                if self.looping {
                    self.restart_at_original_frame_one()?;
                    changed = true;
                    break;
                }
                return Ok(BinkMovieStep::Ended);
            }
            let pkt = self.file.video_packet(self.current_frame)?;
            let frame = self.decoder.decode_frame(pkt)?;
            self.rgba = frame_to_rgba(frame);
            self.current_frame += 1;
            changed = true;
        }
        if changed {
            self.upload_rgba(gpu);
            Ok(BinkMovieStep::FrameUploaded)
        } else {
            Ok(BinkMovieStep::Unchanged)
        }
    }

    fn upload_rgba(&self, gpu: &GpuContext) {
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
        let frame = self.decoder.decode_frame(pkt)?;
        self.rgba = frame_to_rgba(frame);
        self.current_frame = 1;
        self.accumulator_secs = 0.0;
        Ok(())
    }
}

pub fn frame_to_rgba(frame: &BinkFrame) -> Vec<u8> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    let mut out = vec![0u8; w * h * 4];

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
    out
}

fn frames_due(accumulator: &mut f64, elapsed: f64, fps: f64, max: usize) -> usize {
    if fps <= 0.0 {
        return 0;
    }
    *accumulator += elapsed.max(0.0);
    let frame_dt = 1.0 / fps;
    let mut count = 0;
    while *accumulator >= frame_dt && count < max {
        *accumulator -= frame_dt;
        count += 1;
    }
    count
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

#[cfg(test)]
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
    fn jpeg_mid_grey() {
        assert_eq!(yuv_to_rgb_jpeg(128, 128, 128), (128, 128, 128));
    }

    #[test]
    fn frame_clock_uses_fps_not_timer_interval() {
        let mut acc = 0.0;
        assert_eq!(frames_due(&mut acc, 0.034, 15.0, 4), 0);
        assert_eq!(frames_due(&mut acc, 0.033, 15.0, 4), 1);
    }
}
