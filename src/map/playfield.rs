//! Native MapClass playfield bounds and membership arithmetic.
//!
//! This is map-owned because both map preview/generation and deterministic sim
//! consume the same normalized `Size`/`LocalSize` fields. It depends only on
//! parsed map data and fixed packed-cell conventions, never on sim state.

use crate::map::cell_index::packed_cell_coord;
use crate::map::map_file::MapHeader;

/// The five final MapClass fields consumed by the isometric playfield query.
///
/// gamemd-derived: active YR `MapClass::Set_Clipped_LocalSize @ 0x00567230`
/// establishes these fields, and `MapClass::IsCellInPlayfield @ 0x00578460`
/// consumes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlayfieldBounds {
    /// Signed `[Map] Size=` width (`MapClass + 0xF4`).
    pub base: i32,
    /// Final normalized `[Map] LocalSize=` left (`MapClass + 0xFC`).
    pub off_fc: i32,
    /// Final normalized `[Map] LocalSize=` top (`MapClass + 0x100`).
    pub off_100: i32,
    /// Final normalized `[Map] LocalSize=` width (`MapClass + 0x104`).
    pub off_104: i32,
    /// Final normalized `[Map] LocalSize=` height (`MapClass + 0x108`).
    pub off_108: i32,
}

impl PlayfieldBounds {
    /// Construct the live MapClass playfield fields from the raw map header.
    ///
    /// `MapHeader` preserves signed INI values in `u32` bit patterns. This
    /// reproduces signed `ClipRect @ 0x00421B60` followed by
    /// `MapClass::Set_Clipped_LocalSize @ 0x00567230`, including wrapping
    /// arithmetic for malformed headers and no post-cap saturation.
    pub(crate) fn from_map_header(header: &MapHeader) -> Self {
        Self::from_raw_local_size(
            header.width as i32,
            header.height as i32,
            [
                header.local_left as i32,
                header.local_top as i32,
                header.local_width as i32,
                header.local_height as i32,
            ],
        )
    }

    /// Normalize one raw LocalSize writer against the immutable map Size.
    ///
    /// In addition to the load path above, active YR trigger action 0x28 writes
    /// four new signed dwords and calls the same normalization immediately
    /// (`TriggerAction__Execute @ 0x006DD8B0` -> `FUN_006E21E0`). Keeping this
    /// here prevents trigger runtime from copying MapClass's clip/margin math.
    pub(crate) fn from_raw_local_size(
        size_width: i32,
        size_height: i32,
        raw_local_size: [i32; 4],
    ) -> Self {
        let [clipped_left, clipped_top, clipped_width, clipped_height] =
            clip_local_size_to_map(size_width, size_height, raw_local_size);

        let left = clipped_left.max(2);
        let top = clipped_top.max(2);
        let width_cap = size_width.wrapping_sub(left).wrapping_sub(2);
        let height_cap = size_height.wrapping_sub(top).wrapping_sub(6);

        Self {
            base: size_width,
            off_fc: left,
            off_100: top,
            off_104: clipped_width.min(width_cap),
            off_108: clipped_height.min(height_cap),
        }
    }

    /// Construct from already-normalized final fields.
    ///
    /// RMG establishes synthetic LocalSize `(2,5,genW,genH)` and immediately
    /// runs `0x00567230`; those values are stable under its clip/margin rules.
    /// This seam exists for that post-normalization RMG state, not raw headers.
    pub const fn from_normalized_local_size(
        map_width: i32,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    ) -> Self {
        Self {
            base: map_width,
            off_fc: left,
            off_100: top,
            off_104: width,
            off_108: height,
        }
    }

    /// Mode-zero `MapClass::IsCellInPlayfield @ 0x00578460`.
    ///
    /// This is pure geometry: it does not perform a CellClass lookup and uses
    /// `h = 0`. Every add/subtract/double is explicit wrapping i32 arithmetic,
    /// matching the x86 instructions and signed branches.
    pub const fn contains_geometry_packed(self, x: i32, y: i32) -> bool {
        let (x, y) = packed_cell_coord(x, y);
        self.contains_with_height(x, y, 0)
    }

    /// Mode-one `MapClass::IsCellInPlayfield @ 0x00578460` after its caller
    /// supplies the selected CellClass's signed level and unsigned slope byte.
    pub const fn contains_height_aware_packed(self, x: i32, y: i32, level: i8, slope: u8) -> bool {
        let (x, y) = packed_cell_coord(x, y);
        let sum = x.wrapping_add(y);
        let mut height = level as i32;
        let slope_threshold = self
            .base
            .wrapping_add(4)
            .wrapping_add(self.off_100.wrapping_mul(2))
            .wrapping_add(height);
        if slope != 0 && sum < slope_threshold {
            height = height.wrapping_add(1);
        }
        self.contains_with_height(x, y, height)
    }

    const fn contains_with_height(self, x: i32, y: i32, height: i32) -> bool {
        let sum = x.wrapping_add(y);
        let difference = x.wrapping_sub(y);
        let low = self
            .base
            .wrapping_add(self.off_100.wrapping_mul(2))
            .wrapping_add(height);
        let high = self
            .base
            .wrapping_add(2)
            .wrapping_add(self.off_108.wrapping_add(self.off_100).wrapping_mul(2))
            .wrapping_add(height);
        let right = self
            .off_104
            .wrapping_add(self.off_fc)
            .wrapping_mul(2)
            .wrapping_sub(self.base);
        let left = self.base.wrapping_sub(self.off_fc.wrapping_mul(2));

        low < sum && sum <= high && difference < right && difference.wrapping_neg() < left
    }
}

/// `MapClass::In_Bounds @ 0x00568300`: the diamond of the map `Size=` width
/// (`MapClass+0xF4`) and height (`+0xF8`), in wrapping signed arithmetic.
pub(crate) const fn size_diamond_contains(width: i32, height: i32, cell: (i16, i16)) -> bool {
    let x = cell.0 as i32;
    let y = cell.1 as i32;
    let sum = x.wrapping_add(y);
    width < sum
        && x.wrapping_sub(y) < width
        && y.wrapping_sub(x) < width
        && sum <= width.wrapping_add(height.wrapping_mul(2))
}

/// Convert one LocalSize-relative coordinate through active
/// `MapClass::LocalToCell @ 0x005654A0`.
///
/// All arithmetic wraps as signed i32, both results truncate through the
/// packed signed-i16 cell ABI, and the half-row terms use arithmetic shifts.
pub(crate) const fn local_to_packed_cell(
    bounds: PlayfieldBounds,
    local_u: i32,
    local_v: i32,
) -> (i32, i32) {
    let q = local_u.wrapping_add(bounds.off_fc);
    let r = local_v.wrapping_add(bounds.off_100);
    let rx = q.wrapping_add(r.wrapping_add(1) >> 1);
    let ry = bounds.base.wrapping_add(r >> 1).wrapping_sub(q);
    (rx as i16 as i32, ry as i16 as i32)
}

/// Packed corner coordinates for `MapClass::IsRectInPlayfield @ 0x00578390`.
/// Far-edge math wraps as i32 before each component truncates to signed i16;
/// zero and negative spans are intentionally not validated.
pub(crate) const fn rect_playfield_corners(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> [(i32, i32); 4] {
    let (near_x, near_y) = packed_cell_coord(x, y);
    let far_x = x.wrapping_add(width).wrapping_sub(1) as i16 as i32;
    let far_y = y.wrapping_add(height).wrapping_sub(1) as i16 as i32;
    [
        (near_x, near_y),
        (far_x, near_y),
        (near_x, far_y),
        (far_x, far_y),
    ]
}

/// Signed lepton component conversion used by the forced-height-aware wrapper
/// `MapClass::IsCoordInPlayfield @ 0x005785F0`.
pub(crate) const fn lepton_to_packed_cell_component(value: i32) -> i32 {
    (value / 256) as i16 as i32
}

/// Signed intersection of raw LocalSize with normalized Size, matching active
/// `ClipRect @ 0x00421B60` inside `0x00567230`.
/// Native passes LocalSize in EDX as the clipping bounds and Size on the stack
/// as the initial candidate (`0x00567233..0x00567251`). Reversing those roles
/// preserves ordinary intersections but changes signed-overflow branches.
/// Executable comparison: `tools/spatial_oracle/map_queries.py`.
fn clip_local_size_to_map(
    size_width: i32,
    size_height: i32,
    [clip_left, clip_top, clip_width, clip_height]: [i32; 4],
) -> [i32; 4] {
    if clip_width <= 0 || clip_height <= 0 || size_width <= 0 || size_height <= 0 {
        return [0; 4];
    }

    let (mut left, mut top, mut width, mut height) = (0i32, 0i32, size_width, size_height);
    if left < clip_left {
        width = width.wrapping_add(left.wrapping_sub(clip_left));
        left = clip_left;
    }
    if width <= 0 {
        return [0; 4];
    }

    if top < clip_top {
        height = height.wrapping_add(top.wrapping_sub(clip_top));
        top = clip_top;
    }
    if height <= 0 {
        return [0; 4];
    }

    if clip_left.wrapping_add(clip_width) < left.wrapping_add(width) {
        width = clip_left.wrapping_sub(left).wrapping_add(clip_width);
    }
    if width <= 0 {
        return [0; 4];
    }

    let clip_bottom = clip_top.wrapping_add(clip_height);
    if clip_bottom < top.wrapping_add(height) {
        height = clip_bottom.wrapping_sub(top);
    }
    if height <= 0 {
        return [0; 4];
    }

    [left, top, width, height]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(size: (i32, i32), local: [i32; 4]) -> MapHeader {
        MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width: size.0 as u32,
            height: size.1 as u32,
            local_left: local[0] as u32,
            local_top: local[1] as u32,
            local_width: local[2] as u32,
            local_height: local[3] as u32,
        }
    }

    #[test]
    fn playfield_bounds_from_map_header_matches_native_clipping_and_margins() {
        assert_eq!(
            PlayfieldBounds::from_map_header(&header((80, 80), [-5, -6, 100, 100])),
            PlayfieldBounds {
                base: 80,
                off_fc: 2,
                off_100: 2,
                off_104: 76,
                off_108: 72,
            }
        );
        assert_eq!(
            PlayfieldBounds::from_map_header(&header((0, 0), [0; 4])),
            PlayfieldBounds {
                base: 0,
                off_fc: 2,
                off_100: 2,
                off_104: -4,
                off_108: -8,
            }
        );
    }

    #[test]
    fn playfield_predicate_wraps_i32_like_gamemd() {
        let bounds = PlayfieldBounds {
            base: i32::MIN,
            off_fc: i32::MIN + 1,
            off_100: i32::MIN,
            off_104: i16::MIN.into(),
            off_108: i16::MIN.into(),
        };

        // This fixture crosses every bound-derived overflow site. The expected
        // verdict is the ordered wrapping-i32 formula from 0x005784AA..0x00578523.
        assert!(bounds.contains_geometry_packed(i16::MIN.into(), i16::MIN.into()));
        assert!(!bounds.contains_height_aware_packed(i16::MIN.into(), i16::MIN.into(), -128, 1,));
    }
}
