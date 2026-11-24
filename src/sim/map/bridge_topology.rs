//! Bridge topology read service (Slice 3).
//!
//! Single owner of the gamemd-native bridge bit semantics and signed
//! effective-height math, plus the service-facing handle for the traversal gate.
//! Consumers (combat AoE, line of fire and InRange's elevation bonus)
//! construct a borrowed `CellBridgeView` over the canonical cell store or a
//! Map lookup's answer and read these predicates instead of re-deriving each
//! one at their own call site.
//!
//! It holds the structural/anchor flag predicates, signed effective height,
//! the list layer enum and the AoE object-layer selector.
//!
//! ## Dependency rules
//! - Depends on map/bridge_facts (flag bits) and map/resolved_terrain.
//! - NEVER depends on render/ — the render draw-offset lives behind a separate
//!   render-facing trait so this module stays render-free (invariant #1).
//! - All math is integer / `i8`-signed. No f32/f64 (the float boundary is INI
//!   parse only, which this module does not touch).

use crate::map::bridge_facts::BridgeFlags;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, BRIDGE_DECK_HEIGHT_LEVELS};

/// Which persistent cell list an object belongs to. The ground list and the
/// bridge-deck list are distinct so movers/projectiles on a high bridge do not
/// interact with whatever sits underneath the span.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ListLayer {
    #[default]
    Ground,
    Bridge,
}

/// Borrowed read view of one cell's bridge-relevant substrate fields.
///
/// This is NOT a new owned store — it is an adapter built from the canonical
/// post-map-load cell store (`ResolvedTerrainCell`) so the predicates read
/// the same data the rest of the engine stamps at map load. Fields are copied
/// out as the gamemd-native signed/typed forms (`level` is reinterpreted as
/// `i8`, mirroring `PathCell::signed_level`), so the predicates never re-do the
/// sign math.
#[derive(Debug, Clone, Copy)]
pub struct CellBridgeView {
    /// Signed cell height level. Reinterpreted `u8 -> i8` exactly like
    /// `PathCell::signed_level()` so a raw level > 127 reads as negative.
    pub level: i8,
    /// CellClass flag word as a typed handle, single-sourced from `bridge_facts`.
    pub flags: BridgeFlags,
}

impl CellBridgeView {
    /// The view of the cell a Map lookup answered through `cells`: a real cell
    /// or the shared dummy, whose level and flags are current state too.
    pub(crate) fn from_query(cells: &NativeCellQuery<'_>, cell: NativeCellIdentity) -> Self {
        Self::from_fields(cells.ground_fields(cell).0, cells.flags(cell))
    }

    /// Read a retained Cell pointer at this receiver boundary. In particular,
    /// an aliased Dummy may have changed since the lookup that returned it.
    pub(crate) fn from_cell_ref(cell: &crate::sim::cell_rect::CellRef<'_>) -> Self {
        Self::from_fields(cell.signed_level() as u8, cell.bridge_flags_0x1180())
    }

    /// The `u8 -> i8` cast on `level` is deliberate: it reproduces gamemd's
    /// signed reinterpretation of that byte (a raw level of `0xFE` is height
    /// `-2`, not `254`). This is the only place the cast lives.
    fn from_fields(level: u8, raw_flags: u32) -> Self {
        CellBridgeView {
            level: level as i8,
            flags: BridgeFlags(raw_flags),
        }
    }

    // --- Flag predicates (L1 / C1-C3) ----------------------------------------

    /// `0x100` — authoritative structural high-bridge cell.
    #[inline]
    pub fn is_bridge_cell(&self) -> bool {
        self.flags.structural()
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
    /// the level read is signed and an anchor adds the `+4` deck levels. It is
    /// intentionally NOT the layer-driven `effective_cell_z_for_layer` form
    /// (which keys off the mover's current layer instead of the anchor flag).
    #[inline]
    pub fn effective_height(&self) -> i32 {
        self.level as i32
            + if self.is_anchor() {
                BRIDGE_DECK_HEIGHT_LEVELS
            } else {
                0
            }
    }

    // --- AoE object-layer selector -------------------------------------------
    //
    // One AreaDamage invocation retains this layer across every scanned cell
    // and through receiver callbacks into the later rocking continuation.

    /// AreaDamage48955E..48958D selects one list before its cell callbacks.
    /// Map578080 supplies the ground-only floor at the exact impact XY; both
    /// operands are absolute leptons, including the one-lepton remainder.
    /// The original89E864 deck is416 leptons; equality at floor+208 stays Ground.
    #[inline]
    pub fn aoe_object_layer(&self, impact_z_leptons: i32, floor_z_leptons: i32) -> ListLayer {
        if self.is_bridge_cell()
            && impact_z_leptons > floor_z_leptons.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS / 2)
        {
            ListLayer::Bridge
        } else {
            ListLayer::Ground
        }
    }

    // GATE A2: the per-cell occupancy BITFIELD layer (ground vs bridge/deck) is
    // a SEPARATE selection from the object-LIST layer. The list layer keys off the
    // occupant's persistent `on_bridge` byte; the bit layer keys off the object's
    // Z height vs ground. The two are independent and may disagree at ramp
    // boundaries — a verified gamemd behavior, so this selector is list-only.
}

// --- Bridge traversal notes ---------------------------------------------------
//
// The traversal gate is Foot `0x004D9C60`, owned by
// `movement::infantry_entry::adjust_height_and_list` inside the class's
// Can_Enter_Cell.
//
// GATE A4 inventory (verified, `GATE_BRIDGE_TRAVERSAL_RESOLUTION_GHIDRA_REPORT.md`):
//   - In gamemd the ground-unit bridge-traversal validator is dispatched via the
//     Foot/Unit/Infantry vtable slot `+0x1B0` (Aircraft/Building override that
//     slot with non-bridge functions, so the dispatch is ground-unit-only).
//   - Warhead field `+0x144` is the `Wall=` boolean (INI key "Wall", default
//     false). It is the per-warhead half of the AoE bridge-destruction gate
//     (`DestroyableBridges && warhead.wall`) and also allows overlay-wall
//     destruction. NOT to be conflated with `WallAbsoluteDestroyer` (`+0x145`).
//     Recorded here for the topology inventory; the warhead parser/collapse wiring
//     that consumes it is out of this slice's scope.
//   - The `Level + 4` height seed the gate applies is the Level-unit pathfinding
//     seed (1 ElevationIncrement) — DISTINCT from the lepton coordinate-Z deck
//     offset (`BRIDGE_DECK_HEIGHT_LEPTONS`). The two are never mixed (A1 §5).
