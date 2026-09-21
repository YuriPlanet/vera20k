//! Bridge topology read service (Slice 3).
//!
//! Single owner of the gamemd-native bridge bit semantics and signed
//! effective-height math, plus the service-facing handle for the traversal gate.
//! Consumers (movement, combat-AoE, occupancy, pathfinding) construct a borrowed
//! `CellBridgeView` over the canonical cell store and read these predicates
//! instead of re-deriving each one at their own call site.
//!
//! Scope of THIS slice: the verified, hash-neutral predicate/offset
//! consolidation, plus the gamemd-correct SHADOW layer selectors — the
//! structural/bridgehead/anchor flag predicates, signed effective-height, the
//! concrete- and wood-bridge tileset windows (kept SEPARATE from the structural
//! `0x100` flag), the low-bridge/tube predicate, the list layer enum, a
//! delegating handle to the existing pathfinding traversal gate, and the verified
//! deck-height consts + AoE/occupancy layer selectors AS SHADOW.
//!
//! The shadow selectors (`aoe_object_layer`, `occupancy_bit_layer`) encode the
//! verified binary threshold (deck offset = `4 × per_level`). The authority-flip
//! is NOT done here: the authoritative AoE selector and occupancy storage keep
//! their current owners, while these predicates remain read-service mirrors.
//!
//! ## Dependency rules
//! - Depends on map/bridge_facts (flag bits), map/resolved_terrain, sim/pathfinding
//!   (the traversal gate it delegates to). All within sim/ + map/.
//! - NEVER depends on render/ — the render draw-offset lives behind a separate
//!   render-facing trait so this module stays render-free (invariant #1).
//! - All math is integer / `i8`-signed. No f32/f64 (the float boundary is INI
//!   parse only, which this module does not touch).

use crate::map::bridge_facts::BridgeFlags;
use crate::map::resolved_terrain::ResolvedTerrainCell;
#[cfg(test)]
use crate::map::resolved_terrain::YR_CELL_LAND_TUNNEL;
use crate::util::lepton::LEPTONS_PER_LEVEL;

/// Level-unit seed an anchor cell adds to its *effective height* (`GetEffectiveHeight`
/// = `Level + ((flags>>7)&1) * 4`). This is the discrete terrain-Level index seed
/// (1 ElevationIncrement) used for pathfinding/layer/occupancy decisions in the
/// `Can_Enter_Cell`/traversal family — it is NOT the coordinate-Z/AoE deck offset.
/// Verified value `4` (Level units). Keep this strictly separate from the lepton
/// deck offset below: the binary computes the two from different sources and never
/// mixes them in one comparison.
///
/// (Source: `GATE_BRIDGE_DECK_HEIGHT_RESOLUTION_GHIDRA_REPORT.md` §5 — the `+4`
/// Level-unit pathfinding seed, distinct from the coordinate-Z deck.)
pub const BRIDGE_EFFECTIVE_HEIGHT_ANCHOR_SEED_LEVELS: i32 = 4;

/// Verified coordinate-Z / AoE / occupancy deck offset a unit's Z gains when it is
/// on a HIGH structural bridge: `unit.Z = GetGroundHeight(coord) + DECK_OFFSET`.
///
/// The binary deck offset is the runtime global
/// `ftol_chop(per_level_bridge_height × 4 + 0.5)` **in LEPTONS**. With the
/// verified per-level step of 104 leptons this is `4 × 104 = 416` leptons,
/// exactly **4 levels**.
///
/// This is the deck height that the coordinate-Z snap, the AoE object-layer
/// selector, and the occupancy bit-layer threshold all share — distinct from the
/// Level-unit anchor seed above. The values are numerically equal in their
/// respective domains but come from separate native state and must stay named.
///
/// Active body: `0x005F37C0..0x005F3890` stores `DAT_00AC13BC`; the OnBridge
/// coordinate path at `0x005F5FA0` adds that value to CellClass ground height.
pub const BRIDGE_DECK_HEIGHT_LEPTONS: i32 = 4 * LEPTONS_PER_LEVEL as i32;
/// The same verified deck offset expressed in Level units (`416 leptons / 104 =
/// 4 levels`). Use this where the operand is already pre-divided to Level units
/// (the current Rust AoE/occupancy selectors operate on `cell.level`).
pub const BRIDGE_DECK_HEIGHT_LEVELS: i32 = BRIDGE_DECK_HEIGHT_LEPTONS / LEPTONS_PER_LEVEL as i32;

/// Width of a tileset window: a concrete- or wood-bridge tileset occupies the
/// first 16 tiles `[base, base + 0x10)` of its theater set. Gated on base != -1.
#[cfg(test)]
const BRIDGE_TILESET_WINDOW: i32 = 0x10;

/// Which persistent cell list an object belongs to. The ground list and the
/// bridge-deck list are distinct so movers/projectiles on a high bridge do not
/// interact with whatever sits underneath the span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListLayer {
    Ground,
    Bridge,
}

/// Borrowed read view of one cell's bridge-relevant substrate fields.
///
/// This is NOT a new owned store — it is an adapter built from the canonical
/// post-map-load cell store (`ResolvedTerrainCell`) so the seven predicates read
/// the same data the rest of the engine stamps at map load. All fields are copied
/// out as the gamemd-native signed/typed forms (e.g. `level` is reinterpreted as
/// `i8`, mirroring `PathCell::signed_level`), so the predicates never re-do the
/// sign or window math.
#[derive(Debug, Clone, Copy)]
pub struct CellBridgeView {
    /// Signed cell height level. Reinterpreted `u8 -> i8` exactly like
    /// `PathCell::signed_level()` so a raw level > 127 reads as negative.
    pub level: i8,
    /// CellClass flag word as a typed handle, single-sourced from `bridge_facts`.
    pub flags: BridgeFlags,
    /// Slope/ramp passability byte (CellClass+0x11C / `PathCell.slope_type`),
    /// reinterpreted signed for the diff-1 traversal sub-branch the gate uses.
    pub ramp_byte: i8,
    /// Final resolved isometric tile id (`ResolvedTerrainCell.final_tile_index`).
    /// Window-compared against the theater tileset bases for the tileset
    /// predicates — NOT the structural flag.
    pub iso_tile_index: i32,
    /// Low-bridge tube index (CellClass+0x116), `None` when not a tube cell.
    pub tube_index: Option<i16>,
    /// Final CellClass LandType (`yr_cell_land_type`), used by the low-bridge
    /// predicate (`== YR_CELL_LAND_TUNNEL`).
    pub land_type: u8,
    /// Bridge state byte (0 = NS base, 9 = EW base, etc.). Carried for the
    /// render draw-offset trait; not used by the sim predicates.
    pub state_byte: u8,
}

impl CellBridgeView {
    /// Build a view from the canonical resolved-terrain cell.
    ///
    /// The `u8 -> i8` casts on `level` and `slope_type` are deliberate: they
    /// reproduce gamemd's signed reinterpretation of those bytes (a raw level of
    /// `0xFE` is height `-2`, not `254`). This is the only place the cast lives.
    pub fn from_resolved(cell: &ResolvedTerrainCell) -> Self {
        CellBridgeView {
            level: cell.level as i8,
            flags: BridgeFlags(cell.bridge_facts.raw_flags),
            ramp_byte: cell.slope_type as i8,
            iso_tile_index: cell.final_tile_index,
            tube_index: cell.tube_index.map(|t| t.0 as i16),
            land_type: cell.yr_cell_land_type,
            state_byte: cell.bridge_facts.state_byte,
        }
    }

    // --- Flag predicates (L1 / C1-C3) ----------------------------------------

    /// `0x100` — authoritative structural high-bridge cell.
    #[inline]
    pub fn is_bridge_cell(&self) -> bool {
        self.flags.structural()
    }

    /// `0x200` — bridgehead/transition (on/off-ramp boundary) cell.
    #[cfg(test)]
    #[inline]
    pub fn is_bridgehead(&self) -> bool {
        self.flags.bridgehead()
    }

    /// `0x80` — anchor cell of the stamp.
    #[inline]
    pub fn is_anchor(&self) -> bool {
        self.flags.anchor()
    }

    // --- Effective height (L2 / C4) ------------------------------------------

    /// Signed level plus the Level-unit anchor seed for anchor cells.
    ///
    /// This is the `(i8)level + ((flags >> 7) & 1) * 4` form (`GetEffectiveHeight`):
    /// the level read is signed and an anchor adds exactly the `+4` Level-unit
    /// pathfinding seed. It is intentionally NOT the layer-driven
    /// `effective_cell_z_for_layer` form (which keys off the mover's current layer
    /// instead of the anchor flag), and it is NOT the coordinate-Z deck offset
    /// (`BRIDGE_DECK_HEIGHT_LEPTONS` / `_LEVELS` = 4 levels). They are separate
    /// native quantities despite sharing the numeric value four.
    #[inline]
    pub fn effective_height(&self) -> i32 {
        self.level as i32
            + if self.is_anchor() {
                BRIDGE_EFFECTIVE_HEIGHT_ANCHOR_SEED_LEVELS
            } else {
                0
            }
    }

    // --- Tileset windows (L3 / L4 / C5) --------------------------------------
    //
    // These are tile-id range checks, completely DISTINCT from the structural
    // `0x100` flag. Conflating a tileset window with structural is DRIFT #6, so
    // the windows are their own predicates and never alias `is_bridge_cell`.

    /// Concrete-bridge tileset window `[base, base + 0x10)`, gated on `base >= 0`.
    /// `base` is the theater-loaded `g_BridgeSet_TileSetBase` equivalent (passed
    /// in because it is theater state, not cell-local). Returns `false` when no
    /// concrete-bridge set is loaded (`base == None` / `< 0`).
    /// `CellClass::IsBridge` 0x00486750: `base != -1 && base <= IsoTileTypeIndex
    /// < base + 0x10`. Leaf, no calls, no RNG. `base` is
    /// `g_BridgeSet_TileSetBase` 0x00AA0E28, written per theater by
    /// `Read_Theater_TileSets_INI` at 0x00545A80 / 0x00545DDB / 0x00546CB3.
    #[cfg(test)]
    #[inline]
    pub fn is_bridge_tileset(&self, base: Option<i32>) -> bool {
        base.is_some_and(|b| {
            b >= 0 && (b..b + BRIDGE_TILESET_WINDOW).contains(&self.iso_tile_index)
        })
    }

    /// Wood-bridge tileset window `[wood_base, wood_base + 0x10)`, gated on
    /// `wood_base >= 0`. Distinct from the concrete window AND from structural.
    /// `wood_base` is the theater-loaded `g_WoodBridgeSet_TileSetBase` equivalent.
    ///
    /// NOTE: the canonical store already precomputes the wood-window membership
    /// for `final_tile_index` as `ResolvedTerrainCell.is_wood_bridge_repair_tile`
    /// (the CABHUT repair-dispatch predicate). When a `CellBridgeView` is built
    /// from a resolved cell, prefer routing through that precompute rather than
    /// re-deriving the window with a re-passed base (single-source). This method
    /// exists for callers that hold only the tile id + base.
    /// `CellClass::IsWoodBridge` 0x00486770 — the structural twin of
    /// 0x00486750 with `g_WoodBridgeSet_TileSetBase` 0x00ABAD1C substituted,
    /// written at 0x00545A86 / 0x00545DEA / 0x00546CB9.
    #[cfg(test)]
    #[inline]
    pub fn is_wood_bridge_tileset(&self, wood_base: Option<i32>) -> bool {
        wood_base.is_some_and(|b| {
            b >= 0 && (b..b + BRIDGE_TILESET_WINDOW).contains(&self.iso_tile_index)
        })
    }

    // --- Low bridge / tube (L5 / C6) -----------------------------------------

    /// Low-bridge/tube cell: BOTH a valid tube index in `[0, tube_count)` AND a
    /// LandType of `YR_CELL_LAND_TUNNEL` (10). Both conditions are required —
    /// either alone is not a tube cell. This is the low-bridge tube, NOT
    /// subterranean/tunnel (TS-legacy, not modelled).
    #[cfg(test)]
    #[inline]
    pub fn is_low_bridge_cell(&self, tube_count: usize) -> bool {
        self.tube_index
            .is_some_and(|t| t >= 0 && (t as usize) < tube_count)
            && self.land_type == YR_CELL_LAND_TUNNEL
    }

    // --- AoE object-layer selector (SHADOW; not yet authoritative) -----------
    //
    // GATE A2/A1: which per-cell object list an AoE detonation damages, by
    // comparing the detonation Z against the deck mid-height.

    /// Select the AoE damage object list for a detonation over this cell.
    ///
    /// gamemd compares the impact Z against `ground_z + half_deck`, where the
    /// half-deck term is two levels (`DECK / 2 = 2`). The compare is
    /// STRICT `>` (impact exactly at the mid-height stays on the ground list).
    /// `ground_z` is the `GetGroundHeight`-equivalent operand in the SAME domain
    /// as `impact_z` (both Level units here).
    ///
    /// (Source: `GATE_BRIDGE_ONBRIDGE_OCCUPANCY_RESOLUTION_GHIDRA_REPORT.md` +
    /// `GATE_BRIDGE_DECK_HEIGHT_RESOLUTION_GHIDRA_REPORT.md` §3/§4.)
    #[inline]
    pub fn aoe_object_layer(&self, impact_z: i32, ground_z: i32) -> ListLayer {
        if self.is_bridge_cell() && impact_z > ground_z + BRIDGE_DECK_HEIGHT_LEVELS / 2 {
            ListLayer::Bridge
        } else {
            ListLayer::Ground
        }
    }

    // --- Occupancy BIT-layer selector (SHADOW; not yet authoritative) --------
    //
    // GATE A2: the per-cell occupancy BITFIELD layer (ground vs bridge/deck) is
    // a SEPARATE selection from the object-LIST layer. The list layer keys off the
    // occupant's persistent `on_bridge` byte; the bit layer keys off the object's
    // Z height vs ground. The two are independent and may disagree at ramp
    // boundaries — a verified gamemd behavior, kept separate here.

    /// Select the occupancy BIT layer for an object at `obj_z` over this cell.
    ///
    /// Bridge bit layer iff the object sits at/above the full deck height
    /// (`ground_z + DECK <= obj_z`, threshold inclusive `<=`) AND — for the MARK
    /// path only — the cell is structural (`Flags & 0x100`). The CLEAR path passes
    /// `require_structural = false`: it clears by the Z threshold ALONE and does
    /// NOT re-check the structural flag, so collapse cleanup still finds the deck
    /// bit after the bridge flag is gone. This Mark/Clear asymmetry is load-bearing.
    ///
    /// SHADOW: `OccupancyGrid` authoritative storage (`src/sim/occupancy.rs`) is
    /// NOT changed by this pass; this selector is only consumed by shadow tests and
    /// the not-yet-wired bit-layer repr.
    ///
    /// (Source: `GATE_BRIDGE_ONBRIDGE_OCCUPANCY_RESOLUTION_GHIDRA_REPORT.md` §b —
    /// Mark `0x007441B0` gates on `Flags&0x100`, Clear `0x00744210` does not.)
    #[cfg(test)]
    #[inline]
    pub fn occupancy_bit_layer(
        &self,
        obj_z: i32,
        ground_z: i32,
        require_structural: bool,
    ) -> ListLayer {
        let z_on_deck = ground_z + BRIDGE_DECK_HEIGHT_LEVELS <= obj_z;
        let structural_ok = !require_structural || self.is_bridge_cell();
        if z_on_deck && structural_ok {
            ListLayer::Bridge
        } else {
            ListLayer::Ground
        }
    }
}

// --- P2: service-facing traversal-gate handle --------------------------------
//
// The binary-shaped traversal gate already exists and is correct in
// `pathfinding::core`. This slice does NOT relocate it (that flip is hash-relevant
// and out of scope here); it only re-exports the existing owner at crate
// visibility under a service-facing alias so combat/occupancy could reach the
// same gate the same way A*/runtime do. The pathfinding owner stays authoritative;
// this is a delegating handle that proves the seam is identical (see the shadow
// test below), not a second implementation.
//
// Visibility note: the gate and its input/result types are `pub(crate)` in
// pathfinding, so this handle is `pub(crate)` too — it cannot be made more public
// than its owner, and making it so would be the authority-flip this slice avoids.
//
// GATE A4 inventory (verified, `GATE_BRIDGE_TRAVERSAL_RESOLUTION_GHIDRA_REPORT.md`):
//   - The ground-unit bridge-traversal validator is the function this handle
//     delegates to; in gamemd it is dispatched via the Foot/Unit/Infantry vtable
//     slot `+0x1B0` (Aircraft/Building override that slot with non-bridge
//     functions, so the dispatch is ground-unit-only). Our handle mirrors that:
//     it is reached only from the ground-unit cell-entry path, never aircraft.
//   - Warhead field `+0x144` is the `Wall=` boolean (INI key "Wall", default
//     false). It is the per-warhead half of the AoE bridge-destruction gate
//     (`DestroyableBridges && warhead.wall`) and also allows overlay-wall
//     destruction. NOT to be conflated with `WallAbsoluteDestroyer` (`+0x145`).
//     Recorded here for the topology inventory; the warhead parser/collapse wiring
//     that consumes it is out of this slice's scope.
//   - The `Level + 4` height seed the gate applies is the Level-unit pathfinding
//     seed (1 ElevationIncrement) — DISTINCT from the lepton coordinate-Z deck
//     offset (`BRIDGE_DECK_HEIGHT_LEPTONS`). The two are never mixed (A1 §5).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::{
        BRIDGE_FLAG_ANCHOR_SELF, BRIDGE_FLAG_STRUCTURAL, BRIDGE_FLAG_TRANSITION,
    };
    use crate::sim::pathfinding::{
        BridgeTraversalInput, BridgeTraversalResult, PathGrid,
        check_bridge_traversal as bridge_traversal_gate,
    };

    /// Build a view directly from raw fields (bypasses `from_resolved` so a test
    /// can pin a precise flag/level/tile combination without a full terrain cell).
    fn view(level: i8, raw_flags: u32, iso_tile_index: i32) -> CellBridgeView {
        CellBridgeView {
            level,
            flags: BridgeFlags(raw_flags),
            ramp_byte: 0,
            iso_tile_index,
            tube_index: None,
            land_type: 0,
            state_byte: 0,
        }
    }

    #[test]
    fn bridge_topology_predicates_match_pathcell() {
        // Shadow assert-equal: for a battery of raw-flag fixtures, the view's
        // flag predicates must agree with the canonical `BridgeFlags` consts the
        // pathfinding/load views also read.
        for raw in [
            0u32,
            BRIDGE_FLAG_STRUCTURAL,
            BRIDGE_FLAG_TRANSITION,
            BRIDGE_FLAG_ANCHOR_SELF,
            BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_TRANSITION | BRIDGE_FLAG_ANCHOR_SELF,
        ] {
            let v = view(2, raw, 0);
            assert_eq!(v.is_bridge_cell(), raw & BRIDGE_FLAG_STRUCTURAL != 0);
            assert_eq!(v.is_bridgehead(), raw & BRIDGE_FLAG_TRANSITION != 0);
            assert_eq!(v.is_anchor(), raw & BRIDGE_FLAG_ANCHOR_SELF != 0);
        }
    }

    #[test]
    fn effective_height_anchor_plus4_signed_level() {
        // L2: signed level + exactly the deck height for an anchor; NOT the layer
        // form. Feed a raw level byte > 127 (0xFE) to prove the i8 reinterpret.
        let raw_level = 0xFEu8 as i8; // -2
        assert_eq!(raw_level, -2, "0xFE must reinterpret as -2");

        let anchor = view(raw_level, BRIDGE_FLAG_ANCHOR_SELF, 0);
        assert_eq!(
            anchor.effective_height(),
            -2 + 4,
            "anchor adds the deck height"
        );

        let non_anchor = view(raw_level, 0, 0);
        assert_eq!(
            non_anchor.effective_height(),
            -2,
            "non-anchor is the bare level"
        );
    }

    #[test]
    fn is_bridge_tileset_distinct_from_structural_flag() {
        // DRIFT #6: a cell whose tile id falls in the bridge window but which has
        // NO structural flag is a tileset hit, NOT a structural-bridge cell. The
        // two predicates must never alias.
        let base = Some(100);
        let v = view(0, 0, 105); // in [100, 116), structural flag clear
        assert!(v.is_bridge_tileset(base));
        assert!(!v.is_bridge_cell());

        // Boundary: base + 0x10 is exclusive.
        assert!(view(0, 0, 100).is_bridge_tileset(base)); // lower bound inclusive
        assert!(view(0, 0, 115).is_bridge_tileset(base)); // last in-window tile
        assert!(!view(0, 0, 116).is_bridge_tileset(base)); // upper bound exclusive
        assert!(!view(0, 0, 99).is_bridge_tileset(base)); // below window

        // No set loaded -> never a tileset bridge.
        assert!(!view(0, 0, 105).is_bridge_tileset(None));
        assert!(!view(0, 0, 105).is_bridge_tileset(Some(-1)));
    }

    #[test]
    fn is_wood_bridge_tileset_distinct_from_concrete_and_structural() {
        // L4: the wood window is distinct from the concrete window AND from
        // structural. A tile in the wood window but outside the concrete window
        // and without the structural flag is wood-only.
        let concrete_base = Some(100);
        let wood_base = Some(200);
        let v = view(0, 0, 205); // in wood window, outside concrete window
        assert!(v.is_wood_bridge_tileset(wood_base));
        assert!(!v.is_bridge_tileset(concrete_base));
        assert!(!v.is_bridge_cell());
    }

    #[test]
    fn is_low_bridge_requires_landtype10_and_tube_in_range() {
        // L5: BOTH conditions required.
        let tube_count = 4;
        let with = |tube: Option<i16>, land: u8| CellBridgeView {
            level: 0,
            flags: BridgeFlags(0),
            ramp_byte: 0,
            iso_tile_index: 0,
            tube_index: tube,
            land_type: land,
            state_byte: 0,
        };

        // tube in range + land 10 -> true
        assert!(with(Some(0), YR_CELL_LAND_TUNNEL).is_low_bridge_cell(tube_count));
        assert!(with(Some(3), YR_CELL_LAND_TUNNEL).is_low_bridge_cell(tube_count));
        // tube in range + wrong land -> false
        assert!(!with(Some(2), 9).is_low_bridge_cell(tube_count));
        // tube out of range + land 10 -> false
        assert!(!with(Some(4), YR_CELL_LAND_TUNNEL).is_low_bridge_cell(tube_count));
        // negative tube index (signed compare) + land 10 -> false
        assert!(!with(Some(-1), YR_CELL_LAND_TUNNEL).is_low_bridge_cell(tube_count));
        // no tube + land 10 -> false
        assert!(!with(None, YR_CELL_LAND_TUNNEL).is_low_bridge_cell(tube_count));
    }

    #[test]
    fn gsi_04_03b_deck_height_consts_resolve_to_verified_values() {
        // The coordinate-Z/AoE/occupancy deck offset is 4 × per_level.
        // The anchor effective-height seed is a separately sourced Level-unit +4.
        assert_eq!(
            BRIDGE_DECK_HEIGHT_LEPTONS, 416,
            "deck offset = 4 × 104 leptons"
        );
        assert_eq!(BRIDGE_DECK_HEIGHT_LEVELS, 4, "416 leptons / 104 = 4 levels");
        assert_eq!(
            BRIDGE_EFFECTIVE_HEIGHT_ANCHOR_SEED_LEVELS, 4,
            "GetEffectiveHeight anchor seed is separately sourced"
        );
    }

    #[test]
    fn aoe_object_layer_strict_gt_half_deck() {
        // GATE A2/A1: STRICT `>` against ground + half_deck (half = DECK/2 = 2).
        // base = 100; structural cell.
        let bridge = view(100, BRIDGE_FLAG_STRUCTURAL, 0);
        let ground_z = 100;
        let half = BRIDGE_DECK_HEIGHT_LEVELS / 2; // = 2
        assert_eq!(half, 2, "half-deck is 2 levels");

        // Exactly at the mid-height -> Ground (strict `>` excludes equality).
        assert_eq!(
            bridge.aoe_object_layer(ground_z + half, ground_z),
            ListLayer::Ground
        );
        // One above the mid-height -> Bridge.
        assert_eq!(
            bridge.aoe_object_layer(ground_z + half + 1, ground_z),
            ListLayer::Bridge
        );
        // Below -> Ground.
        assert_eq!(
            bridge.aoe_object_layer(ground_z, ground_z),
            ListLayer::Ground
        );

        // Non-structural cell is always Ground regardless of Z.
        let non_bridge = view(100, 0, 0);
        assert_eq!(
            non_bridge.aoe_object_layer(ground_z + 999, ground_z),
            ListLayer::Ground
        );
    }

    #[test]
    fn occupancy_bit_layer_inclusive_full_deck_and_clear_asymmetry() {
        // GATE A2 §b: bit layer is bridge iff (ground + full_deck <= obj_z) AND,
        // for MARK only, the cell is structural. Threshold is INCLUSIVE `<=` and
        // uses the FULL deck (not the half-deck AoE term).
        let deck = BRIDGE_DECK_HEIGHT_LEVELS; // = 4 (full deck)
        let ground_z = 50;
        let structural = view(50, BRIDGE_FLAG_STRUCTURAL, 0);
        let non_structural = view(50, 0, 0);

        // MARK (require_structural = true):
        //   exactly at full deck on a structural cell -> Bridge (inclusive).
        assert_eq!(
            structural.occupancy_bit_layer(ground_z + deck, ground_z, true),
            ListLayer::Bridge
        );
        //   one below full deck -> Ground.
        assert_eq!(
            structural.occupancy_bit_layer(ground_z + deck - 1, ground_z, true),
            ListLayer::Ground
        );
        //   at full deck but NON-structural -> Ground (Mark gates on Flags&0x100).
        assert_eq!(
            non_structural.occupancy_bit_layer(ground_z + deck, ground_z, true),
            ListLayer::Ground
        );

        // CLEAR (require_structural = false): same Z but NO structural re-check ->
        // a non-structural cell still resolves to Bridge by Z alone. This is the
        // load-bearing collapse-cleanup asymmetry (the bridge flag may be gone).
        assert_eq!(
            non_structural.occupancy_bit_layer(ground_z + deck, ground_z, false),
            ListLayer::Bridge
        );
    }

    #[test]
    fn clear_occupation_no_structural_flag_required() {
        // GATE A2 §b / P5 (L14): Mark gates the bridge bit layer on Flags&0x100,
        // but Clear does NOT — it resolves by the Z threshold alone. So a
        // non-structural cell (the bridge flag already cleared by a collapse) at
        // full deck height resolves to Bridge under Clear (require_structural=false)
        // but Ground under Mark (require_structural=true). This asymmetry lets
        // collapse cleanup still find the deck bit after the structural flag is gone.
        let deck = BRIDGE_DECK_HEIGHT_LEVELS; // full deck = 4 levels
        let ground_z = 0;
        let non_structural = view(0, 0, 0);

        // Mark on a non-structural cell at full deck -> Ground (flag re-checked).
        assert_eq!(
            non_structural.occupancy_bit_layer(ground_z + deck, ground_z, /* mark */ true),
            ListLayer::Ground
        );
        // Clear on the SAME cell/Z -> Bridge (no structural re-check).
        assert_eq!(
            non_structural.occupancy_bit_layer(ground_z + deck, ground_z, /* clear */ false),
            ListLayer::Bridge
        );
    }

    #[test]
    fn service_gate_handle_is_bit_identical_to_pathfinding_gate() {
        // P2 shadow: the service-facing handle must produce the exact same
        // `BridgeTraversalResult` as calling the pathfinding owner directly, for a
        // direction == -1 candidate-only seed over a 1x1 structural fixture. This
        // proves the delegating seam is identical without relocating the gate.
        use crate::sim::pathfinding::PathCell;

        let candidate = PathCell {
            ground_walkable: true,
            bridge_walkable: true,
            bridge_structural: true,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 2,
            bridge_deck_level: 6,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        };
        let grid: PathGrid = PathGrid::from_cells(vec![candidate], 1, 1);

        let input = BridgeTraversalInput {
            candidate: grid.cell(0, 0).unwrap(),
            candidate_coord: (0, 0),
            direction: -1,
            path_height: -1,
            parent: None,
        };

        let via_service: BridgeTraversalResult = bridge_traversal_gate(&grid, input);
        let via_owner = crate::sim::pathfinding::check_bridge_traversal(&grid, input);

        assert_eq!(
            via_service, via_owner,
            "service handle must equal the owner"
        );
        assert!(via_service.allowed);
        assert_eq!(via_service.path_height, 6); // signed_level(2) + 4
        assert!(!via_service.force_bridge_list);
    }
}
