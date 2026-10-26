//! Lossless BGRA8 readback for an already-composited GPU frame.
//!
//! This module never renders a replacement frame. The app gives it the actual
//! swapchain texture after every production draw has been encoded, and it adds
//! one copy into a CPU-mappable buffer. Readback is synchronous by design and
//! is used only by the explicit one-shot shell-capture mode.

use thiserror::Error;

const BGRA8_BYTES_PER_PIXEL: u32 = 4;

#[derive(Debug, Error)]
pub enum FrameReadbackError {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    #[error("synchronous GPU capture is unavailable in the browser")]
    BrowserCaptureUnsupported,
    #[error("frame readback requires non-zero dimensions, got {width}x{height}")]
    EmptyExtent { width: u32, height: u32 },
    #[error("frame readback only supports BGRA8 surfaces, got {0:?}")]
    UnsupportedFormat(wgpu::TextureFormat),
    #[error("frame readback row-size arithmetic overflowed for width {0}")]
    RowSizeOverflow(u32),
    #[error("frame readback buffer-size arithmetic overflowed for {width}x{height}")]
    BufferSizeOverflow { width: u32, height: u32 },
    #[error("frame readback map callback was dropped: {0}")]
    MapCallbackDropped(#[from] std::sync::mpsc::RecvError),
    #[error("frame readback map failed: {0}")]
    MapFailed(#[from] wgpu::BufferAsyncError),
    #[error("frame readback device poll failed: {0}")]
    PollFailed(#[from] wgpu::PollError),
    #[error("mapped frame length {actual} is smaller than required padded length {required}")]
    ShortMappedBuffer { actual: usize, required: usize },
}

/// One encoded texture-to-buffer copy awaiting queue submission and mapping.
pub struct PendingBgra8Readback {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    tight_bytes_per_row: u32,
    padded_bytes_per_row: u32,
}

impl PendingBgra8Readback {
    /// Encode a copy from the final production texture into a mappable buffer.
    ///
    /// The caller must submit `encoder` before calling [`Self::finish`].
    pub fn encode(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<Self, FrameReadbackError> {
        if width == 0 || height == 0 {
            return Err(FrameReadbackError::EmptyExtent { width, height });
        }
        if !matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(FrameReadbackError::UnsupportedFormat(format));
        }

        let tight_bytes_per_row = width
            .checked_mul(BGRA8_BYTES_PER_PIXEL)
            .ok_or(FrameReadbackError::RowSizeOverflow(width))?;
        let padded_bytes_per_row =
            align_up(tight_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                .ok_or(FrameReadbackError::RowSizeOverflow(width))?;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(FrameReadbackError::BufferSizeOverflow { width, height })?;

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shell Capture BGRA8 Readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        Ok(Self {
            buffer,
            width,
            height,
            tight_bytes_per_row,
            padded_bytes_per_row,
        })
    }

    /// Wait for the submitted copy, remove GPU row padding, and return tight
    /// top-left BGRA8 pixels.
    pub fn finish(
        self,
        device: &wgpu::Device,
        submission_index: wgpu::SubmissionIndex,
        timeout: std::time::Duration,
    ) -> Result<Vec<u8>, FrameReadbackError> {
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        {
            let _ = (device, submission_index, timeout);
            Err(FrameReadbackError::BrowserCaptureUnsupported)
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        {
            let slice = self.buffer.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
            device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission_index),
                timeout: Some(timeout),
            })?;
            rx.recv()??;

            let mapped = slice.get_mapped_range();
            let tight = strip_row_padding(
                &mapped,
                self.width,
                self.height,
                self.tight_bytes_per_row,
                self.padded_bytes_per_row,
            )?;
            drop(mapped);
            self.buffer.unmap();
            Ok(tight)
        }
    }
}

fn align_up(value: u32, alignment: u32) -> Option<u32> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment - 1)
        .map(|sum| sum & !(alignment - 1))
}

fn strip_row_padding(
    mapped: &[u8],
    width: u32,
    height: u32,
    tight_bytes_per_row: u32,
    padded_bytes_per_row: u32,
) -> Result<Vec<u8>, FrameReadbackError> {
    let required = usize::try_from(
        u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(FrameReadbackError::BufferSizeOverflow { width, height })?,
    )
    .map_err(|_| FrameReadbackError::BufferSizeOverflow { width, height })?;
    if mapped.len() < required {
        return Err(FrameReadbackError::ShortMappedBuffer {
            actual: mapped.len(),
            required,
        });
    }

    let tight_len = usize::try_from(
        u64::from(tight_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(FrameReadbackError::BufferSizeOverflow { width, height })?,
    )
    .map_err(|_| FrameReadbackError::BufferSizeOverflow { width, height })?;
    let tight_row = tight_bytes_per_row as usize;
    let padded_row = padded_bytes_per_row as usize;
    let mut out = Vec::with_capacity(tight_len);
    for row in 0..height as usize {
        let start = row * padded_row;
        out.extend_from_slice(&mapped[start..start + tight_row]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_between_rows_is_discarded() {
        let mut mapped = vec![0xEE; 16];
        mapped[0..4].copy_from_slice(&[1, 2, 3, 4]);
        mapped[8..12].copy_from_slice(&[5, 6, 7, 8]);
        let tight = strip_row_padding(&mapped, 1, 2, 4, 8).expect("tight rows");
        assert_eq!(tight, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn short_mapping_fails_closed() {
        let err = strip_row_padding(&[0; 7], 1, 2, 4, 4).expect_err("must reject");
        assert!(matches!(
            err,
            FrameReadbackError::ShortMappedBuffer {
                actual: 7,
                required: 8
            }
        ));
    }
}
