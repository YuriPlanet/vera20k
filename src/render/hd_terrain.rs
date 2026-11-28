//! VERA's HD art terrain: 2x upscaled theater tiles, overlays, smudges and
//! terrain objects, sampled by the tile and overlay blitters while the camera
//! zooms in.
//!
//! VERA-only presentation extension, part of HD art (`[VERA] HdArt=`,
//! `render::hd_switch`, off by default), experimental
//! (`docs/zoom-one-pass.md`, phase 2). The tile atlas and the overlay atlas
//! each keep an [`RgbaTwin`]: a copy of their page at twice the resolution in
//! the same layout, each picture the pack's where it has one and the 1x one
//! doubled where not, with the 1x picture's coverage. While the camera zooms
//! in with HD art on (`render::hd_art::bind_hd_twins`), the atlas binds the
//! twin in place of its page, and the same draws read it
//! (`zdepth_shader.wgsl`, `sprite_source.wgsl`); light, Z and palette
//! handling stay the draw's own. Nothing reaches `sim/`, a save, a replay or
//! the network.
//!
//! The pictures live in the art pack's `terrain` folder
//! (`hd_art/pack/terrain` beside the executable, or `VERA20K_HD_ART_PACK`):
//! `<hash>.png` per tile, exactly 2x the tile's rectangle, RGB in the
//! theater palette. The hash ([`tile_hash`]) is the FNV-1a of the tile's size
//! and the RGBA the engine colours it with (`TmpFile::tile_to_rgba`), so a
//! tile finds its picture whatever map, set or variant uses it.
//! `VERA20K_HD_DUMP_TERRAIN=<folder>` writes `<hash>.rgba` (u16 width,
//! u16 height, RGBA) for every tile of the loaded theater the pack lacks, for
//! an offline upscaler.

use std::path::PathBuf;

use super::hd_art::{TwinSwap, compose_rgba_twin, create_twin_texture, write_twin};
use super::hd_loader::HdLoader;

/// Pictures asked of the pack readers (`render::hd_loader`) and not yet
/// back: the whole map's ground arrives over a few frames instead of
/// stalling one.
const MAX_IN_FLIGHT: usize = 512;

/// RGBA bytes of finished pictures put on the twin in one frame, so a burst
/// of arrivals spreads over a few frames.
const UPLOAD_BYTES_PER_FRAME: usize = 16 << 20;

/// FNV-1a 64 of a tile: width and height (u16 little-endian), then its RGBA.
pub(crate) fn tile_hash(width: u32, height: u32, rgba: &[u8]) -> u64 {
    super::hd_art::frame_hash(width, height, rgba)
}

/// The pack's terrain folder (`hd_art::pack_dir`).
fn pack_dir() -> Option<PathBuf> {
    super::hd_art::pack_subdir("terrain")
}

/// Write every tile of `images` the pack lacks as `<hash>.rgba` into the
/// `VERA20K_HD_DUMP_TERRAIN` folder. Nothing when it is unset.
pub(crate) fn dump_tiles<'a>(images: impl IntoIterator<Item = &'a crate::map::theater::TileImage>) {
    dump_rgba(
        images
            .into_iter()
            .map(|tile| (tile.width, tile.height, tile.rgba.as_slice())),
        "rgba",
    );
}

/// Write each `(width, height, rgba)` picture the pack lacks as
/// `<hash>.<extension>` into the `VERA20K_HD_DUMP_TERRAIN` folder: `rgba` for
/// tiles, which the upscaler pads with copies of themselves, `srgba` for
/// overlay and terrain object sprites, padded with their edge colours.
pub(crate) fn dump_rgba<'a>(
    pictures: impl IntoIterator<Item = (u32, u32, &'a [u8])>,
    extension: &str,
) {
    let Some(dump) = std::env::var("VERA20K_HD_DUMP_TERRAIN")
        .ok()
        .map(PathBuf::from)
    else {
        return;
    };
    let pack = pack_dir();
    let _ = std::fs::create_dir_all(&dump);
    let mut written = 0;
    for (width, height, rgba) in pictures {
        if width == 0 || height == 0 || rgba.len() != (width * height * 4) as usize {
            continue;
        }
        let name = format!("{:016x}", tile_hash(width, height, rgba));
        if pack
            .as_ref()
            .is_some_and(|pack| pack.join(format!("{name}.png")).exists())
        {
            continue;
        }
        let path = dump.join(format!("{name}.{extension}"));
        if path.exists() {
            continue;
        }
        let mut bytes = Vec::with_capacity(4 + rgba.len());
        bytes.extend_from_slice(&(width as u16).to_le_bytes());
        bytes.extend_from_slice(&(height as u16).to_le_bytes());
        bytes.extend_from_slice(rgba);
        if std::fs::write(path, bytes).is_ok() {
            written += 1;
        }
    }
    log::info!(
        "HD art terrain dump: {written} new .{extension} in {}",
        dump.display()
    );
}

/// VERA-only, experimental (`docs/zoom-one-pass.md`): an RGBA atlas page's
/// HD twin, the tile atlas's or the overlay atlas's. Made the first frame it
/// is asked for, from the 1x page (each picture doubled, nearest); then the
/// pack readers (`render::hd_loader`, at most [`MAX_IN_FLIGHT`] at a time)
/// read every picture's art, which takes its place on the twin when it is
/// back ([`UPLOAD_BYTES_PER_FRAME`] a frame). A twin the device cannot hold is
/// left out, and the page stays 1x.
pub(crate) struct RgbaTwin {
    dir: PathBuf,
    /// The 1x page's encoded RGBA, until every picture's art is back.
    rgba: Vec<u8>,
    size: [u32; 2],
    /// Each picture's 1x origin and size on the page.
    pictures: Vec<([u32; 2], [u32; 2])>,
    label: &'static str,
    /// The debug tint (`hd_art::debug_tint`).
    debug: [u8; 3],
    texture: Option<wgpu::Texture>,
    binding: Option<wgpu::BindGroup>,
    /// Pictures whose art was asked for, in `pictures` order.
    asked: usize,
    /// The pack readers while art is being read; a finished picture's key
    /// is its index in `pictures`.
    loader: Option<HdLoader>,
    /// Too big for the device: no twin.
    left_out: bool,
    swap: TwinSwap,
}

impl RgbaTwin {
    /// The twin of page `rgba` (`size`, encoded RGBA) holding `pictures`;
    /// None without the pack's terrain folder.
    pub(crate) fn new(
        label: &'static str,
        rgba: Vec<u8>,
        size: [u32; 2],
        pictures: Vec<([u32; 2], [u32; 2])>,
        debug: [u8; 3],
    ) -> Option<Self> {
        let dir = pack_dir().filter(|dir| dir.is_dir())?;
        (rgba.len() == (size[0] * size[1] * 4) as usize).then_some(Self {
            dir,
            rgba,
            size,
            pictures,
            label,
            debug,
            texture: None,
            binding: None,
            asked: 0,
            loader: None,
            left_out: false,
            swap: TwinSwap::default(),
        })
    }

    /// The picture at `origin`, `size`, of the 1x page.
    fn picture(&self, [x, y]: [u32; 2], [width, height]: [u32; 2]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let start = ((row * self.size[0] + x) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + (width * 4) as usize]);
        }
        rgba
    }

    /// Where picture `index`'s art is in the pack, and its size there.
    fn art_path(&self, index: usize) -> (PathBuf, [u32; 2]) {
        let (origin, size) = self.pictures[index];
        let native = self.picture(origin, size);
        let hash = tile_hash(size[0], size[1], &native);
        (
            self.dir.join(format!("{hash:016x}.png")),
            size.map(|v| v * 2),
        )
    }

    /// Put `art`, the pack's picture of picture `index`, on the twin.
    fn put_art(&self, queue: &wgpu::Queue, texture: &wgpu::Texture, index: usize, art: &[u8]) {
        let Some(&(origin, size)) = self.pictures.get(index) else {
            return;
        };
        let native = self.picture(origin, size);
        let texels = compose_rgba_twin(&native, size, Some(art), self.debug);
        write_twin(
            queue,
            texture,
            origin.map(|v| v * 2),
            size.map(|v| v * 2),
            &texels,
        );
    }

    /// Put the twin's binding (`make` makes it from the twin's view) in
    /// `slot` when `on`, making the twin first and reading more of its art;
    /// otherwise the 1x binding back.
    pub(crate) fn bind(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut wgpu::BindGroup,
        on: bool,
        make: impl FnOnce(&wgpu::TextureView) -> wgpu::BindGroup,
    ) {
        if on && !self.left_out && self.texture.is_none() {
            let size = self.size.map(|side| side * 2);
            let max = device.limits().max_texture_dimension_2d;
            if size[0] > max || size[1] > max {
                log::warn!(
                    "HD art: the {} twin ({}x{}) is past this GPU's {max} px; it stays 1x",
                    self.label,
                    size[0],
                    size[1]
                );
                self.left_out = true;
                self.rgba = Vec::new();
            } else {
                let texture = create_twin_texture(device, self.label, size);
                for &(origin, size) in &self.pictures {
                    let native = self.picture(origin, size);
                    let texels = compose_rgba_twin(&native, size, None, self.debug);
                    write_twin(
                        queue,
                        &texture,
                        origin.map(|v| v * 2),
                        size.map(|v| v * 2),
                        &texels,
                    );
                }
                self.binding = Some(make(&texture.create_view(&Default::default())));
                self.texture = Some(texture);
                log::info!(
                    "HD art: {} twin {}x{}, {} pictures",
                    self.label,
                    size[0],
                    size[1],
                    self.pictures.len()
                );
            }
        }
        if let Some(texture) = &self.texture {
            let mut uploaded = 0;
            while uploaded < UPLOAD_BYTES_PER_FRAME {
                let Some(loaded) = self.loader.as_mut().and_then(HdLoader::take) else {
                    break;
                };
                if let Some(art) = loaded.rgba {
                    uploaded += art.len();
                    self.put_art(queue, texture, loaded.key as usize, &art);
                }
            }
            if on && self.asked < self.pictures.len() {
                if super::hd_art::read_at_once() {
                    for index in self.asked..self.pictures.len() {
                        let (path, size) = self.art_path(index);
                        if let Some(art) = super::hd_loader::read_picture(&path, size) {
                            self.put_art(queue, texture, index, &art);
                        }
                    }
                    self.asked = self.pictures.len();
                } else {
                    while self.asked < self.pictures.len()
                        && self
                            .loader
                            .as_ref()
                            .is_none_or(|loader| loader.in_flight() < MAX_IN_FLIGHT)
                    {
                        let index = self.asked;
                        let (path, size) = self.art_path(index);
                        self.loader
                            .get_or_insert_with(|| HdLoader::new("HD terrain reader"))
                            .request(index as u64, path, size);
                        self.asked += 1;
                    }
                }
            }
            if self.asked == self.pictures.len()
                && self
                    .loader
                    .as_ref()
                    .is_none_or(|loader| loader.in_flight() == 0)
            {
                // Every picture's art is in: the 1x copy and the readers go.
                self.rgba = Vec::new();
                self.loader = None;
            }
        }
        self.swap.set(slot, self.binding.as_ref(), on);
    }
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    /// A tile without art doubles into its twin with its own coverage; with
    /// art, the art's colour shows where the tile covers, and nowhere else.
    #[test]
    fn twin_pictures_keep_the_1x_coverage() {
        let native = [10, 20, 30, 255, 0, 0, 0, 0];
        let doubled = compose_rgba_twin(&native, [2, 1], None, [255, 0, 255]);
        assert_eq!(&doubled[..4], &[10, 20, 30, 255]);
        assert_eq!(&doubled[2 * 4..3 * 4], &[0; 4]);
        let art = [[90, 80, 70, 255]; 8].concat();
        let with_art = compose_rgba_twin(&native, [2, 1], Some(&art), [255, 0, 255]);
        if !crate::render::hd_switch::env_override().is_some_and(|(_, debug)| debug) {
            assert_eq!(&with_art[4..8], &[90, 80, 70, 255]);
        }
        assert_eq!(&with_art[3 * 4..4 * 4], &[0; 4]);
    }
}
