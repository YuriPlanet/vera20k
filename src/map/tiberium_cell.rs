//! Cell-level resource value and nonrandom germination.
//!
//! Owns the pure native cell operations shared by authored construction,
//! generated-map initialization, crates, and live ore placement. Callers own
//! cell writes and real-or-dummy lookup storage; this module owns classification,
//! native neighbor order, density table/modulo, and signed value arithmetic.
//! Depends on map overlay types, rules, and util direction; never on sim/.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::tiberium_type::TiberiumTypeRegistry;
use crate::util::direction::DIRECTION_DELTAS;

/// TerrainClass::Unlimbo @0x0071D000, original instructions71D0E2..71D12B:
/// clear only an existing Tiberium overlay's identity and data, without Recalc.
/// Live and generated cells commit this shared result through their owners;
/// their type lookup differs because RMG has only its literal ore/gem ranges.
pub(crate) fn terrain_unlimbo_overlay_fields(
    overlay_id: i32,
    overlay_data: u8,
    is_tiberium: impl FnOnce(i32) -> bool,
) -> (i32, u8) {
    if overlay_id != -1 && is_tiberium(overlay_id) {
        (-1, 0)
    } else {
        (overlay_id, overlay_data)
    }
}

/// `CellClass::Get_Tiberium_Value @ 0x00485020`: zero unless
/// `CellClass::OverlayToTiberiumIndex` resolves the overlay to a
/// TiberiumClass, otherwise `TiberiumClass+0xB8 (Value) * (OverlayData + 1)`
/// in native signed 32-bit arithmetic.
///
/// A resolved class index whose TiberiumClass slot is absent dereferences null
/// natively; VERA returns zero for that malformed registry state.
pub(crate) fn tiberium_value(
    overlay_id: Option<u8>,
    overlay_data: u8,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
) -> i32 {
    let Some(overlay_id) = overlay_id else {
        return 0;
    };
    let Some(type_id) = overlay_registry.tiberium_type_for_overlay(tiberium_types, overlay_id)
    else {
        return 0;
    };
    let Some(tiberium) = tiberium_types.get(type_id) else {
        return 0;
    };
    tiberium
        .value
        .wrapping_mul(i32::from(overlay_data).wrapping_add(1))
}

/// `g_OreDensityByNeighborCount @ 0x0081CD28` (twelve dwords, low bytes):
/// the stored `OverlayData` for a same-class neighbour count modulo
/// `TiberiumClass+0xE4 (MaxDensity)`.
pub(crate) const ORE_DENSITY_BY_NEIGHBOR_COUNT: [u8; 12] = [0, 1, 3, 4, 6, 7, 8, 10, 11, 7, 0, 1];

/// One `SpreadCellGerminate(0)` result for a resource receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GerminatedCell {
    /// New `CellClass+0x11E` (OverlayData) of the receiver.
    pub(crate) density: u8,
    /// Native return `(density + 1) * TiberiumClass+0xB8 (Value)`, signed
    /// wrapping 32-bit.
    pub(crate) value: i32,
}

/// `CellClass::SpreadCellGerminate @ 0x004818E0` with `randomizeType = 0`.
///
/// gamemd-derived (decompiled 2026-09-01): the helper returns 0 without any
/// write when the receiver's `OverlayTypeIndex` (`+0x44`) is -1 or
/// `CellClass::OverlayToTiberiumIndex @ 0x005FDD20` is -1. Otherwise it
/// captures `TiberiumClass+0xB8 (Value)`, resolves all eight
/// `g_DirectionOffsets @ 0x0089F688` neighbours (N, NE, E, SE, S, SW, W, NW;
/// `AND EDX,0x7` on a copy of the loop counter at `0x00481966..0x00481968`) through the stamping
/// `MapClass::Get_CellClass @ 0x005657A0` (`0x004819A6`; a miss stamps the
/// shared dummy's coordinate and the read continues on that dummy), counts
/// those whose `OverlayToTiberiumIndex` equals the receiver's, writes
/// `+0x11E = g_OreDensityByNeighborCount[count % MaxDensity]` (`IDIV` on
/// `TiberiumClass+0xE4` at `0x004819CA`), and returns `(data + 1) * Value`.
/// No RNG is drawn for argument 0.
///
/// The caller owns the receiver write and performs each neighbour lookup
/// through `read_neighbor_fields`, including its dummy stamp, so authored Mark,
/// crate Mark, generated initialization, and live ore placement use one port.
/// Connected original-execution goldens: tools/spatial_oracle/ore_queue.md.
pub(crate) fn spread_cell_germinate_without_randomization(
    tiberium_types: &TiberiumTypeRegistry,
    overlay_registry: &OverlayTypeRegistry,
    receiver_overlay_id: Option<u8>,
    cell: (i16, i16),
    mut read_neighbor_fields: impl FnMut((i16, i16)) -> (Option<u8>, u8),
) -> Option<GerminatedCell> {
    let overlay_id = receiver_overlay_id?;
    let type_id = overlay_registry.tiberium_type_for_overlay(tiberium_types, overlay_id)?;
    let tiberium_type = tiberium_types.get(type_id)?;
    // VERA-internal: the native `IDIV` faults on a zero MaxDensity; no retail
    // TiberiumType sets it to zero.
    if tiberium_type.max_density == 0 {
        return None;
    }
    let mut matching: i32 = 0;
    for (dx, dy) in DIRECTION_DELTAS {
        let neighbor = (
            cell.0.wrapping_add(dx as i16),
            cell.1.wrapping_add(dy as i16),
        );
        let (neighbor_id, _) = read_neighbor_fields(neighbor);
        if neighbor_id.and_then(|id| overlay_registry.tiberium_type_for_overlay(tiberium_types, id))
            == Some(type_id)
        {
            matching += 1;
        }
    }
    // At most eight neighbours, so the remainder never leaves the table.
    let index = matching % i32::from(tiberium_type.max_density);
    let density = ORE_DENSITY_BY_NEIGHBOR_COUNT[index as usize];
    Some(GerminatedCell {
        density,
        value: tiberium_value(Some(overlay_id), density, overlay_registry, tiberium_types),
    })
}
