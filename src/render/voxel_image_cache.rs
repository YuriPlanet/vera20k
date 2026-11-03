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
//! - Building guns, voxel debris and bullets are drawn after the unit pass and
//!   read the latch it left, not the latch at their display position. They
//!   differ only in a frame where a `UseBuffer=` draw changes it.
//! - The renderer's shared colour buffer, depth map and previous rect
//!   (`0x00753E00`) are not kept: a depth-tested draw after an ordinary one
//!   starts from a clear depth map, where native keeps stale depth outside the
//!   previous rect. On retail SCHP that drops 1-2% of the next chopper image
//!   after a tank-sized render, 39% after a bullet-sized one (#3794).

use std::collections::HashMap;

use super::unit_atlas::UnitSpriteKey;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedDraw {
    /// Rasterized with the flag set.
    pub depth_tested: bool,
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

#[derive(Default)]
pub struct VoxelImageCache {
    /// `0xB43180`.
    use_buffer: bool,
    /// Pool bytes the stored images take.
    pool_used: u32,
    images: HashMap<VoxelImageKey, CachedDraw>,
    /// Pool flushes so far; derived image stores keyed on it drop older fills.
    flushes: u32,
}

impl VoxelImageCache {
    /// A part draw through `0x00706640` keyed `image`: on a hit, the stored
    /// image. On a miss it renders in the current style with the drawn
    /// `pitch` and stores that; `bytes` is the image's pool size in that
    /// style.
    pub fn draw(
        &mut self,
        image: VoxelImageKey,
        use_buffer_type: bool,
        pitch: i8,
        bytes: impl FnOnce(bool) -> u32,
    ) -> CachedDraw {
        if let Some(stored) = self.images.get(&image) {
            return *stored;
        }
        let drawn = CachedDraw {
            depth_tested: self.render_part(use_buffer_type),
            barrel_pitch: pitch,
        };
        let size = bytes(drawn.depth_tested);
        self.store(image, drawn, size);
        drawn
    }

    /// The image stored under `image`, without drawing it.
    pub fn hit(&self, image: &VoxelImageKey) -> Option<CachedDraw> {
        self.images.get(image).copied()
    }

    /// A part render, uncached or on a miss (`0x00706ED0`): whether it is
    /// depth-tested.
    pub fn render_part(&mut self, use_buffer_type: bool) -> bool {
        if use_buffer_type {
            self.use_buffer = false;
            return true;
        }
        self.use_buffer
    }

    /// Whether a shadow draw keyed `image` blits a stored image.
    pub fn shadow_hit(&self, image: &VoxelImageKey) -> bool {
        self.images.contains_key(image)
    }

    /// A shadow render (`0x00707280`) after a miss, or uncached with `None`:
    /// stores `bytes` under `image`.
    pub fn render_shadow(
        &mut self,
        image: Option<VoxelImageKey>,
        use_buffer_type: bool,
        bytes: u32,
    ) {
        if use_buffer_type {
            self.use_buffer = true;
        }
        if let Some(image) = image {
            let shadow = CachedDraw {
                depth_tested: false,
                barrel_pitch: 0,
            };
            self.store(image, shadow, bytes);
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
