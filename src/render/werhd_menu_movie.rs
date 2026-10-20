//! The menu loop's presentation source. Retail/capture keeps the original
//! Bink surface; interactive iOS/macOS uses the approved branded menu movie, losslessly
//! stored as indexed PNG keyframes and XOR deltas. This is deliberately not a
//! source for the campaign/fullscreen movie player.

use crate::assets::error::AssetError;
use crate::render::batch::BatchTexture;
use crate::render::bink_movie::{BinkMovieStep, BinkMovieSurface};
use crate::render::gpu::GpuContext;

pub(crate) enum MenuMovieSurface {
    Retail(BinkMovieSurface),
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    Branded(BrandedMenuSurface),
}

impl From<BinkMovieSurface> for MenuMovieSurface {
    fn from(value: BinkMovieSurface) -> Self {
        Self::Retail(value)
    }
}

impl MenuMovieSurface {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub(crate) fn branded(
        gpu: &GpuContext,
        batch: &crate::render::batch::BatchRenderer,
    ) -> Result<Self, AssetError> {
        BrandedMenuSurface::new(gpu, batch).map(Self::Branded)
    }

    pub(crate) fn batch_texture(&self) -> &BatchTexture {
        match self {
            Self::Retail(movie) => movie.batch_texture(),
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            Self::Branded(movie) => &movie.batch_texture,
        }
    }

    pub(crate) fn source_archive(&self) -> &str {
        match self {
            Self::Retail(movie) => movie.source_archive(),
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            Self::Branded(_) => "built-in WERHD menu movie",
        }
    }

    pub(crate) fn step(
        &mut self,
        gpu: &GpuContext,
        elapsed_secs: f64,
    ) -> Result<BinkMovieStep, AssetError> {
        match self {
            Self::Retail(movie) => movie.step(gpu, elapsed_secs),
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            Self::Branded(movie) => movie.step(gpu, elapsed_secs),
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
pub(crate) struct BrandedMenuSurface {
    frames: FrameStream<'static>,
    texture: wgpu::Texture,
    batch_texture: BatchTexture,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl BrandedMenuSurface {
    fn new(
        gpu: &GpuContext,
        batch: &crate::render::batch::BatchRenderer,
    ) -> Result<Self, AssetError> {
        let frames = FrameStream::new(include_bytes!("../../art/branding/werhd-menu.wmvframes"))?;
        let (texture, batch_texture) = batch.create_updatable_texture(
            &gpu.device,
            &gpu.queue,
            &frames.rgba,
            frames.archive.width,
            frames.archive.height,
            wgpu::FilterMode::Nearest,
        );
        Ok(Self {
            frames,
            texture,
            batch_texture,
        })
    }

    fn step(&mut self, gpu: &GpuContext, elapsed_secs: f64) -> Result<BinkMovieStep, AssetError> {
        if !self.frames.advance(elapsed_secs)? {
            return Ok(BinkMovieStep::Unchanged);
        }
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.frames.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.frames.archive.width * 4),
                rows_per_image: Some(self.frames.archive.height),
            },
            wgpu::Extent3d {
                width: self.frames.archive.width,
                height: self.frames.archive.height,
                depth_or_array_layers: 1,
            },
        );
        Ok(BinkMovieStep::FrameUploaded)
    }
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
struct FrameArchive<'a> {
    payload: &'a [u8],
    offsets: Vec<usize>,
    width: u32,
    height: u32,
    fps: f64,
    key_interval: usize,
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
impl<'a> FrameArchive<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, AssetError> {
        const HEADER_SIZE: usize = 8 + 6 * 4;
        if bytes.len() < HEADER_SIZE || &bytes[..8] != b"VWMV0001" {
            return Err(invalid("unrecognized or truncated header"));
        }
        let field = |index: usize| {
            let start = 8 + index * 4;
            u32::from_le_bytes(bytes[start..start + 4].try_into().expect("checked header"))
        };
        let (width, height, fps_num, fps_den, frame_count, key_interval) =
            (field(0), field(1), field(2), field(3), field(4), field(5));
        let pixel_count = width.checked_mul(height);
        if width == 0
            || height == 0
            || pixel_count.is_none_or(|count| count > 16_777_216)
            || fps_num == 0
            || fps_den == 0
            || frame_count == 0
            || key_interval == 0
        {
            return Err(invalid(
                "invalid dimensions, frame rate, frame count or key interval",
            ));
        }
        let count = usize::try_from(frame_count).map_err(|_| invalid("frame count overflows"))?;
        let index_end = count
            .checked_add(1)
            .and_then(|count| count.checked_mul(8))
            .and_then(|size| HEADER_SIZE.checked_add(size))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| invalid("truncated frame index"))?;
        let payload = &bytes[index_end..];
        let mut offsets = Vec::with_capacity(count + 1);
        for chunk in bytes[HEADER_SIZE..index_end].chunks_exact(8) {
            let offset = u64::from_le_bytes(chunk.try_into().expect("eight-byte index entry"));
            let offset = usize::try_from(offset).map_err(|_| invalid("frame offset overflows"))?;
            if offset > payload.len() || offsets.last().is_some_and(|previous| *previous >= offset)
            {
                return Err(invalid(
                    "frame offsets are unordered or outside the payload",
                ));
            }
            offsets.push(offset);
        }
        if offsets[0] != 0 || offsets[count] != payload.len() {
            return Err(invalid("frame index does not cover the payload"));
        }
        Ok(Self {
            payload,
            offsets,
            width,
            height,
            fps: f64::from(fps_num) / f64::from(fps_den),
            key_interval: key_interval as usize,
        })
    }

    fn frame_count(&self) -> usize {
        self.offsets.len() - 1
    }

    fn decode(&self, index: usize, previous: &mut Vec<u8>) -> Result<(), AssetError> {
        let Some((&start, &end)) = self.offsets.get(index).zip(self.offsets.get(index + 1)) else {
            return Err(invalid("frame index is out of range"));
        };
        let png =
            image::load_from_memory_with_format(&self.payload[start..end], image::ImageFormat::Png)
                .map_err(|error| invalid(format!("frame {index} PNG: {error}")))?
                .into_rgba8();
        if png.width() != self.width || png.height() != self.height {
            return Err(invalid(format!(
                "frame {index} dimensions do not match the archive"
            )));
        }
        let decoded = png.into_raw();
        if index.is_multiple_of(self.key_interval) {
            *previous = decoded;
        } else {
            if previous.len() != decoded.len() {
                return Err(invalid("delta frame has no matching previous RGBA frame"));
            }
            for (pixel, delta) in previous.iter_mut().zip(decoded) {
                *pixel ^= delta;
            }
        }
        Ok(())
    }
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
struct FrameStream<'a> {
    archive: FrameArchive<'a>,
    /// One restored frame; decoding temporarily holds one additional PNG RGBA
    /// frame, then drops it. The compressed archive is borrowed, never copied.
    rgba: Vec<u8>,
    next_frame: usize,
    accumulator_secs: f64,
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
impl<'a> FrameStream<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, AssetError> {
        let archive = FrameArchive::parse(bytes)?;
        let mut rgba = Vec::new();
        archive.decode(0, &mut rgba)?;
        Ok(Self {
            archive,
            rgba,
            next_frame: 1,
            accumulator_secs: 0.0,
        })
    }

    fn advance(&mut self, elapsed_secs: f64) -> Result<bool, AssetError> {
        if elapsed_secs.is_finite() {
            self.accumulator_secs += elapsed_secs.max(0.0);
        }
        let frame_time = 1.0 / self.archive.fps;
        let mut changed = false;
        // Match the existing menu player's bounded catch-up. Entry/exit slide
        // and suspension timers remain with the menu's existing clock owner.
        for _ in 0..4 {
            if self.accumulator_secs < frame_time {
                break;
            }
            self.accumulator_secs -= frame_time;
            if self.next_frame == self.archive.frame_count() {
                self.archive.decode(0, &mut self.rgba)?;
                self.next_frame = 1;
                self.accumulator_secs = 0.0;
                return Ok(true);
            }
            self.archive.decode(self.next_frame, &mut self.rgba)?;
            self.next_frame += 1;
            changed = true;
        }
        Ok(changed)
    }
}

#[cfg(any(target_os = "ios", target_os = "macos", test))]
fn invalid(detail: impl Into<String>) -> AssetError {
    AssetError::ParseError {
        format: "WERHD menu frames".to_string(),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    fn fixture(frames: &[Vec<u8>], width: u32, height: u32, interval: usize) -> Vec<u8> {
        let mut payload = Vec::new();
        let mut offsets = vec![0_u64];
        for (index, frame) in frames.iter().enumerate() {
            let pixels: Vec<_> = if index.is_multiple_of(interval) {
                frame.clone()
            } else {
                frame
                    .iter()
                    .zip(&frames[index - 1])
                    .map(|(a, b)| a ^ b)
                    .collect()
            };
            image::codecs::png::PngEncoder::new(&mut payload)
                .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
                .unwrap();
            offsets.push(payload.len() as u64);
        }
        let mut bytes = b"VWMV0001".to_vec();
        for field in [width, height, 15, 1, frames.len() as u32, interval as u32] {
            bytes.extend(field.to_le_bytes());
        }
        for offset in offsets {
            bytes.extend(offset.to_le_bytes());
        }
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn keyframes_and_xor_deltas_restore_every_rgba_byte_and_loop() {
        let frames = vec![
            vec![10, 20, 30, 255, 40, 50, 60, 255],
            vec![11, 25, 30, 255, 40, 80, 90, 255],
            vec![100, 110, 120, 255, 130, 140, 150, 255],
            vec![101, 111, 123, 255, 131, 144, 153, 255],
        ];
        let bytes = fixture(&frames, 2, 1, 2);
        let mut stream = FrameStream::new(&bytes).unwrap();
        assert_eq!(stream.rgba, frames[0]);
        assert!(!stream.advance(1.0 / 30.0).unwrap());
        assert!(stream.advance(1.0 / 30.0).unwrap());
        assert_eq!(stream.rgba, frames[1]);
        for frame in &frames[2..] {
            assert!(stream.advance(1.0 / 15.0).unwrap());
            assert_eq!(&stream.rgba, frame);
        }
        assert!(stream.advance(1.0 / 15.0).unwrap());
        assert_eq!(stream.rgba, frames[0]);
        assert_eq!(stream.next_frame, 1);
    }

    #[test]
    fn catch_up_is_bounded_and_nonfinite_elapsed_does_not_poison_clock() {
        let frames = (0..8)
            .map(|value| vec![value, value, value, 255])
            .collect::<Vec<_>>();
        let bytes = fixture(&frames, 1, 1, 3);
        let mut stream = FrameStream::new(&bytes).unwrap();
        assert!(!stream.advance(f64::INFINITY).unwrap());
        assert!(!stream.advance(f64::NAN).unwrap());
        assert!(stream.advance(1.0).unwrap());
        assert_eq!(stream.rgba, frames[4]);
        assert_eq!(stream.next_frame, 5);
    }

    #[test]
    fn malformed_index_and_mismatched_png_dimensions_are_rejected() {
        let frames = vec![vec![1, 2, 3, 255], vec![4, 5, 6, 255]];
        let bytes = fixture(&frames, 1, 1, 2);
        assert!(FrameArchive::parse(&bytes[..31]).is_err());
        let mut corrupt = bytes.clone();
        corrupt[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(FrameArchive::parse(&corrupt).is_err());
        let mut wrong_size = bytes;
        wrong_size[8..12].copy_from_slice(&2_u32.to_le_bytes());
        assert!(FrameStream::new(&wrong_size).is_err());
    }
}
