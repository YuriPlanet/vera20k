//! Retail one-shot screenshot completion.
//!
//! The app retains the last presented composition immediately before its
//! software-cursor pass, so a hotkey captures the client pixels already visible
//! when the input is dispatched without baking the cursor into the image. This
//! module queues that readback, expands the active RGB565 surface into direct-RGB
//! PCX bytes, probes the current working directory, and writes the first unused
//! `SCRN%04d.pcx`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};

pub(crate) const READBACK_TIMEOUT: Duration = Duration::from_secs(15);

struct PresentedFrameSlots<T> {
    presented: Option<T>,
    staging: Option<T>,
    presented_valid: bool,
    staging_valid: bool,
}

impl<T> Default for PresentedFrameSlots<T> {
    fn default() -> Self {
        Self {
            presented: None,
            staging: None,
            presented_valid: false,
            staging_valid: false,
        }
    }
}

impl<T> PresentedFrameSlots<T> {
    fn reset(&mut self, presented: T, staging: T) {
        self.presented = Some(presented);
        self.staging = Some(staging);
        self.presented_valid = false;
        self.staging_valid = false;
    }

    fn staging(&self) -> Option<&T> {
        self.staging.as_ref()
    }

    #[cfg(test)]
    fn staging_mut(&mut self) -> Option<&mut T> {
        self.staging.as_mut()
    }

    fn mark_staged(&mut self) {
        self.staging_valid = true;
    }

    fn presented(&self) -> Option<&T> {
        self.presented_valid
            .then_some(self.presented.as_ref())
            .flatten()
    }

    fn commit_presented(&mut self) {
        if !self.staging_valid {
            return;
        }
        std::mem::swap(&mut self.presented, &mut self.staging);
        self.presented_valid = true;
        self.staging_valid = false;
    }
}

/// Last cursor-free composition committed to successful presentation.
#[derive(Default)]
pub(crate) struct PresentedFrameCache {
    // Shell draw lists borrow their source atlases while this derived GPU cache
    // is staged. Keep this presentation-only mutation inside its single owner.
    state: RefCell<PresentedFrameCacheState>,
}

#[derive(Default)]
struct PresentedFrameCacheState {
    slots: PresentedFrameSlots<wgpu::Texture>,
    width: u32,
    height: u32,
    format: Option<wgpu::TextureFormat>,
    presentation_width: u32,
    presentation_height: u32,
    client_readback_texture: Option<wgpu::Texture>,
    client_width: u32,
    client_height: u32,
    client_format: Option<wgpu::TextureFormat>,
}

impl PresentedFrameCache {
    /// Copy the completed UI composition before the software cursor is drawn.
    /// The staged surface does not become screenshot-visible until the app
    /// confirms that this frame reached `present()`.
    ///
    /// gamemd.exe provenance: active YR `ScreenCaptureCommandClass::Execute`
    /// at 0x00537BC0 hides WWMouse while copying the presented client surface.
    pub(crate) fn stage_pre_cursor_composition(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        current: &wgpu::Texture,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        presentation_width: u32,
        presentation_height: u32,
    ) {
        let mut state = self.state.borrow_mut();
        state.ensure_composition_textures(
            device,
            format,
            width,
            height,
            presentation_width,
            presentation_height,
        );
        let staging = state
            .slots
            .staging()
            .expect("pre-cursor staging texture was just ensured");

        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: current,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: staging,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        state.slots.mark_staged();
    }

    /// Queue a readback from the previously presented cursor-free composition.
    /// When the game renders below client resolution, the retained source is
    /// passed through the same zoom pass into a temporary client-sized target
    /// before readback.
    pub(crate) fn capture_previous_if_requested(
        &self,
        requested: bool,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        client_format: wgpu::TextureFormat,
        client_width: u32,
        client_height: u32,
        upscale: Option<&crate::render::upscale_pass::UpscalePass>,
    ) -> Result<Option<crate::render::frame_readback::PendingBgra8Readback>> {
        let mut state = self.state.borrow_mut();
        if !requested || state.slots.presented().is_none() {
            return Ok(None);
        }

        if state.width == client_width && state.height == client_height {
            return crate::render::frame_readback::PendingBgra8Readback::encode(
                device,
                encoder,
                state
                    .slots
                    .presented()
                    .expect("presented source was checked above"),
                client_format,
                client_width,
                client_height,
            )
            .map(Some)
            .map_err(anyhow::Error::new);
        }

        state.ensure_client_readback_texture(device, client_format, client_width, client_height);
        let target = state
            .client_readback_texture
            .as_ref()
            .expect("client screenshot target was just ensured");
        state.encode_previous_composition(
            device,
            encoder,
            target,
            (client_width, client_height),
            upscale,
        )?;
        crate::render::frame_readback::PendingBgra8Readback::encode(
            device,
            encoder,
            target,
            client_format,
            client_width,
            client_height,
        )
        .map(Some)
        .map_err(anyhow::Error::new)
    }

    /// Commit the staged composition only after its cursor-bearing counterpart
    /// has been submitted and presented.
    pub(crate) fn commit_presented(&self) {
        self.state.borrow_mut().slots.commit_presented();
    }

    /// Restore the last presented cursor-free backing for a retained modal
    /// painting a `frame`-sized top-left corner of `destination`.
    /// The saved-load mode-2 paint (621E90) copies existing AlternateSurface;
    /// this shares screenshot retention and its ordinary upscale projection.
    pub(crate) fn encode_previous_composition(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
        frame: (u32, u32),
        upscale: Option<&crate::render::upscale_pass::UpscalePass>,
    ) -> Result<bool> {
        self.state.borrow().encode_previous_composition(
            device,
            encoder,
            destination,
            frame,
            upscale,
        )
    }
}

impl PresentedFrameCacheState {
    /// A zoomed match frame keeps its source size here: the retained source
    /// fills the frame's corner, and the frame's own zoom pass scales both.
    fn encode_previous_composition(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
        frame: (u32, u32),
        upscale: Option<&crate::render::upscale_pass::UpscalePass>,
    ) -> Result<bool> {
        let Some(source) = self.slots.presented() else {
            return Ok(false);
        };
        ensure!(
            self.presentation_width == destination.width()
                && self.presentation_height == destination.height()
                && self.format == Some(destination.format()),
            "cursor-free backing no longer matches the presentation target"
        );
        if (self.width, self.height) == frame {
            encoder.copy_texture_to_texture(
                source.as_image_copy(),
                destination.as_image_copy(),
                source.size(),
            );
        } else {
            ensure!(
                frame == (destination.width(), destination.height()),
                "cursor-free cache is {}x{}, the frame is {}x{}",
                self.width,
                self.height,
                frame.0,
                frame.1
            );
            let upscale = upscale
                .context("cursor-free source differs from client size without a zoom pass")?;
            ensure!(
                upscale.src_width() == self.width && upscale.src_height() == self.height,
                "cursor-free cache is {}x{}, zoom source is {}x{}",
                self.width,
                self.height,
                upscale.src_width(),
                upscale.src_height()
            );
            upscale.draw_texture(
                device,
                encoder,
                source,
                &destination.create_view(&Default::default()),
            );
        }
        Ok(true)
    }

    fn ensure_composition_textures(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        presentation_width: u32,
        presentation_height: u32,
    ) {
        if self.slots.staging().is_some()
            && self.width == width
            && self.height == height
            && self.format == Some(format)
            && self.presentation_width == presentation_width
            && self.presentation_height == presentation_height
        {
            return;
        }

        let create = |label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        self.slots.reset(
            create("Presented cursor-free composition"),
            create("Staged cursor-free composition"),
        );
        self.width = width;
        self.height = height;
        self.format = Some(format);
        self.presentation_width = presentation_width;
        self.presentation_height = presentation_height;
        self.client_readback_texture = None;
        self.client_width = 0;
        self.client_height = 0;
        self.client_format = None;
    }

    fn ensure_client_readback_texture(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) {
        if self.client_readback_texture.is_some()
            && self.client_width == width
            && self.client_height == height
            && self.client_format == Some(format)
        {
            return;
        }
        self.client_readback_texture = Some(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Cursor-free client screenshot target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }));
        self.client_width = width;
        self.client_height = height;
        self.client_format = Some(format);
    }
}

/// Write one final client-frame readback using the active retail naming and PCX
/// contract. `pixels` must be tightly packed, top-left-origin BGRA8.
pub(crate) fn write_retail_screenshot(
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    pixels: &[u8],
) -> Result<PathBuf> {
    ensure!(
        matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ),
        "retail screenshot requires a BGRA8 client surface, got {format:?}"
    );

    let pixel_count = width
        .checked_mul(height)
        .context("screenshot pixel count overflowed")?;
    let expected_len = usize::try_from(
        pixel_count
            .checked_mul(4)
            .context("screenshot byte count overflowed")?,
    )
    .context("screenshot byte count does not fit this platform")?;
    ensure!(
        pixels.len() == expected_len,
        "screenshot readback has {} bytes, expected {expected_len}",
        pixels.len()
    );

    let pcx_width = u16::try_from(width).context("screenshot width does not fit PCX")?;
    let pcx_height = u16::try_from(height).context("screenshot height does not fit PCX")?;
    let rgb = rgb565_pixels_from_bgra(pixels, pixel_count)?;

    let encoded = crate::assets::pcx_file::encode_direct_rgb(pcx_width, pcx_height, &rgb)
        .map_err(anyhow::Error::new)
        .context("could not encode retail screenshot PCX")?;
    let path = first_unused_screenshot_path(Path::new("."))
        .context("the SCRN screenshot filename space is exhausted")?;
    std::fs::write(&path, encoded)
        .with_context(|| format!("could not write screenshot {}", path.display()))?;
    Ok(path)
}

fn rgb565_pixels_from_bgra(pixels: &[u8], pixel_count: u32) -> Result<Vec<u8>> {
    let mut rgb = Vec::with_capacity(
        usize::try_from(
            pixel_count
                .checked_mul(3)
                .context("screenshot RGB byte count overflowed")?,
        )
        .context("screenshot RGB byte count does not fit this platform")?,
    );
    for bgra in pixels.as_chunks::<4>().0 {
        rgb.extend_from_slice(&active_retail_rgb565_expansion(bgra));
    }
    Ok(rgb)
}

fn active_retail_rgb565_expansion(bgra: &[u8]) -> [u8; 3] {
    [bgra[2] & 0xF8, bgra[1] & 0xFC, bgra[0] & 0xF8]
}

fn first_unused_screenshot_path(directory: &Path) -> Option<PathBuf> {
    first_unused_screenshot_path_with(directory, Path::exists)
}

fn first_unused_screenshot_path_with(
    directory: &Path,
    mut exists: impl FnMut(&Path) -> bool,
) -> Option<PathBuf> {
    (0..=i32::MAX)
        .map(|index| directory.join(format!("SCRN{index:04}.pcx")))
        .find(|candidate| !exists(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an explicit GPU readback run"]
    fn retained_backing_excludes_later_cursor_and_unpresented_frames() {
        let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let size = [2, 2];
        let source = gpu.target(size, format);
        let destination = gpu.target(size, format);
        let original = [32_u8, 48, 64, 255].repeat(4);
        gpu.queue.write_texture(
            source.as_image_copy(),
            &original,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8),
                rows_per_image: Some(2),
            },
            source.size(),
        );
        let cache = PresentedFrameCache::default();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        cache.stage_pre_cursor_composition(&gpu.device, &mut encoder, &source, format, 2, 2, 2, 2);
        assert!(
            !cache
                .encode_previous_composition(&gpu.device, &mut encoder, &destination, (2, 2), None)
                .unwrap()
        );
        // Stand in for the later cursor draw: it changes the live source only
        // after the shared retention boundary has copied the completed frame.
        let view = source.create_view(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Cursor overwrite after backing retention"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 1.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        gpu.queue.submit([encoder.finish()]);
        cache.commit_presented();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        // A staged successor is not the visible frame until present commits it.
        cache.stage_pre_cursor_composition(&gpu.device, &mut encoder, &source, format, 2, 2, 2, 2);
        assert!(
            cache
                .encode_previous_composition(&gpu.device, &mut encoder, &destination, (2, 2), None)
                .unwrap()
        );
        let read = gpu.read(&mut encoder, &destination);
        assert_eq!(gpu.finish(encoder, &[read], size)[0], original);
        cache.commit_presented();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        assert!(
            cache
                .encode_previous_composition(&gpu.device, &mut encoder, &destination, (2, 2), None)
                .unwrap()
        );
        let read = gpu.read(&mut encoder, &destination);
        assert_eq!(
            gpu.finish(encoder, &[read], size)[0],
            [0_u8, 255, 0, 255].repeat(4)
        );
    }

    #[test]
    fn filename_probe_starts_at_zero_and_uses_first_gap() {
        let occupied = ["SCRN0000.pcx", "SCRN0001.pcx", "SCRN0003.pcx"];
        let path = first_unused_screenshot_path_with(Path::new("."), |candidate| {
            candidate
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| occupied.contains(&name))
        })
        .expect("unused screenshot path");

        assert_eq!(path.file_name().unwrap(), "SCRN0002.pcx");
    }

    #[test]
    fn filename_width_is_a_minimum_not_a_four_digit_cap() {
        let path = first_unused_screenshot_path_with(Path::new("."), |candidate| {
            candidate.file_name().unwrap() != "SCRN10000.pcx"
        })
        .expect("unused screenshot path");
        assert_eq!(path.file_name().unwrap(), "SCRN10000.pcx");
    }

    #[test]
    fn active_surface_expansion_preserves_only_rgb565_channel_bits() {
        assert_eq!(
            active_retail_rgb565_expansion(&[0xFF, 0xAB, 0x7F, 0x42]),
            [0x78, 0xA8, 0xF8]
        );
    }

    #[test]
    fn gsi_17_09_screenshot_uses_prior_pre_cursor_pixels() {
        let underlying_a = vec![0x18, 0x94, 0xE7, 0xFF];
        let underlying_b = vec![0x71, 0x42, 0x23, 0xFF];
        let cursor_pixel = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let mut slots = PresentedFrameSlots::default();
        slots.reset(Vec::<u8>::new(), Vec::<u8>::new());

        *slots.staging_mut().expect("staging slot") = underlying_a.clone();
        slots.mark_staged();
        slots.commit_presented();
        *slots.staging_mut().expect("staging slot") = underlying_b;
        slots.mark_staged();

        // The just-rendered display receives the cursor, while a hotkey at this
        // boundary still sees the last frame committed by presentation.
        assert_ne!(cursor_pixel, underlying_a);
        let captured = slots.presented().expect("prior presented frame");
        assert_eq!(captured, &underlying_a);
        let rgb = rgb565_pixels_from_bgra(captured, 1).expect("RGB565 expansion");
        let encoded = crate::assets::pcx_file::encode_direct_rgb(1, 1, &rgb).expect("encode PCX");
        let decoded = crate::assets::pcx_file::PcxFile::from_bytes(&encoded).expect("decode PCX");
        assert_eq!(decoded.pixels, vec![0xE0, 0x94, 0x18]);
    }
}
