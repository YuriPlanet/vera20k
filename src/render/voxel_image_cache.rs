//! The original voxel image caches and their UseBuffer latch, replayed by the
//! draws that take them.
//!
//! `TechnoClass::Draw @ 0x00706640` keeps every body, turret and barrel image
//! a type draws in the type's caches (`ObjectType+0x244` body, `+0x258`
//! turret, `+0x280` barrel), and `TechnoClass::DrawVoxelShadow @ 0x00706BD0`
//! its shadow images (`+0x26C`), each under the draw's key. A hit blits the
//! stored image (`0x00707480`); a miss renders (`0x00706ED0`, `0x00707280`)
//! and stores what it rendered (`0x006C89E0`). A key of -1 renders every draw
//! and stores nothing: posed locomotor arms, a building gun's recoil, and
//! `DisableVoxelCache=` (`+0xDBE`, `0x00706806`) or `DisableShadowCache=`
//! (`+0xDBF`, `0x00706C33`).
//!
//! Every rendered part rasterizes in the style the flag `0xB43180` selects:
//! depth-tested while it is set. `0x00706ED0` sets it before a `UseBuffer=`
//! type's part draw and clears it after (`0x00706F12`, `0x0070724F`).
//! `0x00707280` sets it for such a type's shadow (`0x007072FC`, `0x0070744B`)
//! and never clears it; `Init_Game` clears it once (`0x0052BE04`). A Siege
//! Chopper's first shadow, or any Night Hawk Transport's (drawn uncached every
//! frame), therefore leaves it set: every other type's miss then stores a
//! depth-tested image, and uncached draws render in that style, until a
//! `UseBuffer=` type's part renders again.
//!
//! Stored images share one 4,000,000-byte pool (`0x00887460`, sized at
//! `0x0040F270`): a 12-byte header, then each row as a 2-byte length and
//! `0x00661530`'s encoding, one byte per nonzero pixel and two per run of up
//! to 255 zeros (`0x006C89E0`). A store that does not fit frees all four
//! caches of every type and rewinds the pool (`0x005F99E0`, from `0x00706AE0`
//! and `0x00706E38`), then stores again.
//!
//! Every render, cached or not, runs on the renderer's shared buffers
//! (`0x00753E00`): its first call (`0x008467E0`) clears the colour buffer
//! (`0xB2FF78`) and the depth map (`0xB1D5E0`) whole. Each later call
//! clears the colour buffer over the previous render's crop rect
//! (`0xB2FB60..6C`, written by `0x007542F0` and `0x00754510`; whole when the
//! rect leaves the buffer), and the depth map over the same rect, rows and
//! columns inclusive, only while 0xB43180 is set. A render's pixels lie
//! inside its own rect, so the colour buffer never shows an earlier render
//! and is not kept here. Depth does: a `UseBuffer=` part render clears the
//! flag after drawing, so the renders after it clear only colour until the
//! flag is set again, and that clear covers only the last render's rect. A
//! depth-tested render then keeps the earlier depth outside that rect, and
//! its voxels there draw only where nearer (#3794: on retail SCHP, 1-2% of
//! the next chopper image after a tank-sized render, 39% after a
//! bullet-sized one). A miss stores that image, which later hits blit.
//!
//! Residuals:
//! - Keys: a record stands for the native key through the atlas key drawn.
//!   The Unit Jumpjet shadow key also carries `(Shadow_Point.y - 16) & 31`
//!   (`0x0073C245..0x0073C287`), so a Siege Chopper rising or landing misses
//!   on more shadow keys natively; those misses set the latch it already
//!   holds unless a part miss cleared it in the same frame. HVA frames from 32
//!   alias natively (`& 0x1F`, `0x0073B605`). A barrel drawn without a turret
//!   is part of a Composite record. Rare; the latch outcome rarely differs.
//! - Pool sizes come from VERA's image of the record: the exact crop for a
//!   native-raster part, the composed mask or template for a shadow. A turret
//!   keyed by hull facing counts its atlas image each time. This moves a flush
//!   by a few stores at most.
//! - A posed body, and a barrel drawn without a turret, render their parts
//!   as one image: one clear and one rect where native clears and records
//!   one per part. Stock posed bodies have one part.
//! - A depth clear of a rect that leaves the 256-square map runs on past it
//!   natively (`0x00753EC5` does not clip); here it stops at the map's ends.
//!   No stock voxel spans 248 pixels.
//!

use crate::util::lookup_hash::LookupMap;

use super::unit_atlas::{UnitSpriteEntry, UnitSpriteKey};
use super::vxl_raster::DepthMap;

/// `0x0040F270`'s pool size.
pub const POOL_BYTES: u32 = 4_000_000;

/// One of an ObjectType's four voxel image caches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoxelCache {
    /// `+0x244`: the main voxel, or the NoSpawnAlt body (`0x0073B5F9..`).
    Body,
    /// `+0x258`.
    Turret,
    /// `+0x26C`.
    Shadow,
    /// `+0x280`, under the turret's key.
    Barrel,
}

/// One stored image's identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoxelImageKey {
    pub cache: VoxelCache,
    /// The atlas key standing for the native key; a gun's is its turret's.
    pub sprite: UnitSpriteKey,
    /// Native key bits the atlas key lacks: the hull's facing for a gun of a
    /// type with a `TurretOffset=` (`0x0073B79F`), or the Fly normal arm's
    /// pitch (4) and roll (1, 2) bits (`0x004CF610`).
    pub extra: u32,
}

impl VoxelImageKey {
    pub fn new(cache: VoxelCache, sprite: UnitSpriteKey) -> Self {
        Self {
            cache,
            sprite,
            extra: 0,
        }
    }
}

/// What a part draw blits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CachedDraw {
    /// Rasterized with the flag set.
    pub depth_tested: bool,
    /// The image rasterized over stale depth, which differs from a clean
    /// render of its key; None when it matches one.
    pub stale: Option<UnitSpriteEntry>,
    /// The barrel elevation step the image holds. A gun's key (the turret's
    /// step, the hull's when the type has a `TurretOffset=`, the locomotor's
    /// key and the turret frame, `0x0073B748..0x0073B79F`; a building's
    /// facing and frame low byte, `0x0043E2FA`) has no elevation, and with
    /// both images cached `UnitClass::DrawVoxelBody` skips the gun matrices
    /// (`0x0073BA12..0x0073BA47`). So a key's barrel keeps the elevation of
    /// the first unit drawn under it: level while a new unit's still rounds
    /// level, the first two frames after Unlimbo at the default `FireAngle=`.
    pub barrel_pitch: i8,
}

/// How one part render rasterizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartRender {
    /// Depth-tested: 0xB43180 is set.
    pub depth_tested: bool,
    /// The render must rasterize against [`VoxelImageCache::depth_map`]:
    /// its rect holds stale depth, or it is a `UseBuffer=` part, whose depth
    /// a later render can read.
    pub live: bool,
    /// Stale depth lies in its rect, so its image can differ from a clean
    /// render's.
    pub stale: bool,
}

pub struct VoxelImageCache {
    /// `0xB43180`.
    use_buffer: bool,
    /// Pool bytes the stored images take.
    pool_used: u32,
    images: LookupMap<VoxelImageKey, CachedDraw>,
    /// Pool flushes so far; derived image stores keyed on it drop older fills.
    flushes: u32,
    /// `0xB1D5E0`.
    depth: Box<DepthMap>,
    /// `0xB2FB60..6C`: the last render's crop x, y, width and height in the
    /// 256-square buffers. `0x00753AF0` starts it at (0, 0, 255, 255).
    previous: [i32; 4],
    /// `0x008467E0`: the next clear is the first.
    first_clear: bool,
}

impl Default for VoxelImageCache {
    fn default() -> Self {
        Self {
            use_buffer: false,
            pool_used: 0,
            images: Default::default(),
            flushes: 0,
            depth: vec![0; 65536]
                .into_boxed_slice()
                .try_into()
                .expect("64 KiB depth map"),
            previous: [0, 0, 255, 255],
            first_clear: true,
        }
    }
}

/// The crop rect `0x00754510` and `0x007542F0` give an image `size`
/// (width, height) pixels: each extent less 8, centred at 124 - extent / 2.
fn crop_rect([width, height]: [i32; 2]) -> [i32; 4] {
    let start = |size: i32| 124 - (size - 8) / 2;
    [start(width), start(height), width, height]
}

/// The depth map addresses `0x00753E00` clears for `rect`: its rows and
/// columns inclusive, row by row from (x, y), within the map.
fn rect_addresses([x, y, width, height]: [i32; 4]) -> impl Iterator<Item = std::ops::Range<usize>> {
    (0..=height.max(-1)).map(move |row| {
        let start = (y + row) * 256 + x;
        let clamp = |address: i32| address.clamp(0, 65536) as usize;
        clamp(start)..clamp(start + width + 1)
    })
}

impl VoxelImageCache {
    /// A part draw through `0x00706640` keyed `image`: on a hit, the stored
    /// image. On a miss it renders an image `size` (width, height) pixels
    /// in the current style with the drawn `pitch` and stores what `render`
    /// returns: the stale image it rasterized, if any, and its pool bytes.
    pub fn draw(
        &mut self,
        image: VoxelImageKey,
        use_buffer_type: bool,
        pitch: i8,
        size: [i32; 2],
        render: impl FnOnce(PartRender, &mut DepthMap) -> (Option<UnitSpriteEntry>, u32),
    ) -> CachedDraw {
        if let Some(stored) = self.images.get(&image) {
            return *stored;
        }
        let part = self.render_part(use_buffer_type, size);
        let (stale, bytes) = render(part, &mut self.depth);
        let drawn = CachedDraw {
            depth_tested: part.depth_tested,
            stale,
            barrel_pitch: pitch,
        };
        self.store(image, drawn, bytes);
        drawn
    }

    /// The image stored under `image`, without drawing it.
    pub fn hit(&self, image: &VoxelImageKey) -> Option<CachedDraw> {
        self.images.get(image).copied()
    }

    /// Whether a part render now would be depth-tested.
    pub fn part_style(&self, use_buffer_type: bool) -> bool {
        use_buffer_type || self.use_buffer
    }

    /// A part render of an image `size` (width, height) pixels, uncached or
    /// on a miss: `0x00706ED0`, Bullet `0x0046B0C0` and `VoxelAnimClass`
    /// `0x00749B70`'s body. A `UseBuffer=` type's render sets the flag before
    /// its clear and clears it after (`0x00706F12`, `0x0070724F`). Any other
    /// render that is not stale need not touch the map: its depth lies in its
    /// own rect, which the next render's clear erases while the flag stays
    /// set.
    pub fn render_part(&mut self, use_buffer_type: bool, size: [i32; 2]) -> PartRender {
        if use_buffer_type {
            self.use_buffer = true;
        }
        self.clear();
        let rect = crop_rect(size);
        let depth_tested = self.use_buffer;
        let stale = depth_tested
            && rect_addresses(rect).any(|addresses| self.depth[addresses].iter().any(|&z| z != 0));
        self.previous = rect;
        if use_buffer_type {
            self.use_buffer = false;
        }
        PartRender {
            depth_tested,
            live: stale || (depth_tested && use_buffer_type),
            stale,
        }
    }

    /// The depth map a live render rasterizes against.
    pub fn depth_map(&mut self) -> &mut DepthMap {
        &mut self.depth
    }

    /// Whether a shadow draw keyed `image` blits a stored image.
    pub fn shadow_hit(&self, image: &VoxelImageKey) -> bool {
        self.images.contains_key(image)
    }

    /// A shadow render of an image `size` (width, height) pixels: `0x00707280`
    /// after a miss, or uncached with `None`, which stores `bytes` under
    /// `image`; or `0x00749B70`'s. A `UseBuffer=` type's sets the flag before
    /// its clear (`0x007072FC`) and leaves it set (`0x0070744B`). The stencil
    /// ignores the flag and writes no depth.
    pub fn render_shadow(
        &mut self,
        image: Option<VoxelImageKey>,
        use_buffer_type: bool,
        bytes: u32,
        size: [i32; 2],
    ) {
        if use_buffer_type {
            self.use_buffer = true;
        }
        self.clear();
        self.previous = crop_rect(size);
        if let Some(image) = image {
            let shadow = CachedDraw {
                depth_tested: false,
                stale: None,
                barrel_pitch: 0,
            };
            self.store(image, shadow, bytes);
        }
    }

    /// `0x00753E00`'s depth clear.
    fn clear(&mut self) {
        if std::mem::take(&mut self.first_clear) {
            self.depth.fill(0);
        } else if self.use_buffer {
            for addresses in rect_addresses(self.previous) {
                self.depth[addresses].fill(0);
            }
        }
    }

    /// Pool flushes so far.
    pub fn flushes(&self) -> u32 {
        self.flushes
    }

    /// `0x006C89E0` fails when the image does not fit; the caller then frees
    /// every cache (`0x005F99E0`) and stores again (`0x00706AD5..0x00706B0A`).
    fn store(&mut self, image: VoxelImageKey, drawn: CachedDraw, bytes: u32) {
        if self.pool_used.saturating_add(bytes) > POOL_BYTES {
            self.images.clear();
            self.pool_used = 0;
            self.flushes += 1;
        }
        // An image larger than the whole pool is stored as null and never
        // drawn again natively; a VERA image never approaches that size.
        self.pool_used = self.pool_used.saturating_add(bytes).min(POOL_BYTES);
        self.images.insert(image, drawn);
    }

    /// A new scenario reads its types again, and `TechnoTypeClass::ReadINI`
    /// reloads a `Voxel=` type's voxels (`0x0071609C`), which frees all four
    /// of its caches (`0x005F8BF0..0x005F8C95`): `reloaded` names those
    /// types. The rest, such as a building's gun caches, keep their images:
    /// `Clear_Scene` (`0x006851F0`) deletes objects, not types. The pool
    /// cursor (rewound only by `0x005F99E0`) and `0xB43180` (cleared only by
    /// `Init_Game`, `0x0052BE04`, once a process) carry over too.
    pub fn start_scenario(&mut self, reloaded: impl Fn(&str) -> bool) {
        self.images
            .retain(|image, _| !reloaded(&image.sprite.type_id));
    }

    /// A loaded game: each type's Load frees its four caches
    /// (`ObjectTypeClass::Load`, `0x005F9731..0x005F9772`). The pool cursor
    /// and `0xB43180` carry over as for a new scenario.
    pub fn load_game(&mut self) {
        self.images.clear();
    }
}

/// The pool bytes of a sprite drawn at `offset` from its anchor: its native
/// crop `bounds` (x, y, width, height from the anchor) within the padded
/// `size` pixels, or the whole sprite without bounds.
pub fn sprite_pool_bytes(
    pixels: &[u8],
    size: [u32; 2],
    offset: [f32; 2],
    bounds: Option<[i32; 4]>,
) -> u32 {
    let whole = [0, 0, size[0], size[1]];
    let crop = bounds.map_or(whole, |[x, y, width, height]| {
        let x = (x - offset[0] as i32).clamp(0, size[0] as i32) as u32;
        let y = (y - offset[1] as i32).clamp(0, size[1] as i32) as u32;
        [
            x,
            y,
            (width.max(0) as u32).min(size[0] - x),
            (height.max(0) as u32).min(size[1] - y),
        ]
    });
    pool_bytes(pixels, size[0], crop)
}

/// The pool bytes `0x006C89E0` takes for the `width` x `height` crop of
/// `pixels` (rows `stride` long) at `[x, y]`.
pub fn pool_bytes(pixels: &[u8], stride: u32, [x, y, width, height]: [u32; 4]) -> u32 {
    let mut bytes = 12;
    for row in y..y + height {
        let start = (row * stride + x) as usize;
        let Some(row) = pixels.get(start..start + width as usize) else {
            break;
        };
        bytes += 2 + row_bytes(row);
    }
    bytes
}

/// `0x00661530` without output: one byte per nonzero pixel, two per run of
/// up to 255 zeros. Its zero count can read one byte past the row; that only
/// ends the row's last run, whose bytes it already counted.
fn row_bytes(row: &[u8]) -> u32 {
    let mut bytes = 0;
    let mut zeros = 0u32;
    for &pixel in row {
        if pixel == 0 {
            if zeros % 255 == 0 {
                bytes += 2;
            }
            zeros += 1;
        } else {
            zeros = 0;
            bytes += 1;
        }
    }
    bytes
}

#[cfg(test)]
#[path = "voxel_image_cache_tests.rs"]
mod tests;
