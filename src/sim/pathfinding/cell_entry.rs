//! Cell entry classification — unified Can_Enter_Cell result codes.
//!
//! The original RA2 engine returns 8 distinct codes when a unit
//! tries to enter a cell. Each code triggers a different movement response.
//! This module centralizes the classification logic that was previously
//! scattered as inline boolean checks in movement.rs.
//!
//! Two-phase design for borrow checker compatibility:
//! - Phase 1 (`check_terrain`): terrain + occupancy presence, no EntityStore needed
//! - Phase 2 (`classify_occupied_cell`): blocker friendship/crush, needs &EntityStore
//!
//! Bridge legality is now driven by A*'s `path_layers` (set per-step by `astar_search`
//! with the Ground→Bridge gates verified against the reference predicate), which
//! approximates the post-switch output of the original two-pass `Can_Enter_Cell`. See
//! docs/plans/2026-05-11-bridge-locomotor-layer-correctness-design.md §"Known Parity Boundary".
//!
//! ## The native shape, and what is not modelled
//!
//! `UnitClass::Can_Enter_Cell` @ `0x0073F0A0` and `InfantryClass::Can_Enter_Cell`
//! @ `0x0051BF90` are the `FootClass` `+0x1AC` slot (`0x007F5E1C` and
//! `0x007EB204`). Both are **accumulators**: a running code later occupants may
//! only raise, punctuated by hard `return 7` / `return 0` exits. `AStar_main_loop`
//! @ `0x00429A90` expands a neighbour iff the code is below 7 and
//! `AStar_compute_edge_cost` @ `0x00429830` indexes it into the float table at
//! `0x0081870C` — `[1.0, 1000.0, 1.0, 1.0, 60.0, 20.0, 8.0, 10000.0]`, whose only
//! reader is `0x00429848`. So codes 3, 4, 5 and 6 all still expand, at 1×, 60×,
//! 20× and 8× the base step.
//!
//! Pre-flight, in order, before any code accumulates: the bridge-deck select from
//! `Cell->Flags & 0x100`; the occupier/occupation snapshot; **Unit only** the
//! `MovementRestrictedTo=` gate; the direction-8 tube endpoint test; the
//! tube-direction consistency tests at this cell and at `(dir-4)&7`; **Infantry
//! only** an unconditional admit when `level - Cell->Level > 4`; the `+0x1B0`
//! slot (`CheckBridgeTraversal` @ `0x004D9C60`, FootClass-level — it is the same
//! entry in both vtables); the deck swap; the playfield gate; and **Unit only**
//! `FootClass::LocomotorPassabilityCheck` @ `0x004D9C10`, whose result **seeds**
//! the running code (Infantry seeds a literal 0).
//!
//! VERA reproduces the accumulate-worst-code shape and the crush latch. What it
//! does not, recorded rather than guessed and ordered by ordinary-skirmish
//! impact:
//!
//! - **`Gate=` buildings take the garrison arm.** [`classify_blocker`] maps a
//!   closed or opening `BuildingGateRuntime` to `ScatterRequired` (code 3), but
//!   code 3 in gamemd is `BuildingTypeClass+0x16B7` plus
//!   `!BuildingClass::CanGarrison()` — the garrison flag, read at `0x004525F9`.
//!   `Gate=` is a different field, `+0x16C0` (read by
//!   `BuildingClass::TogglePowerOrGate` @ `0x004471CB`), and its arm is its own
//!   branch, and it never reads the gate's open/closed state at all: the
//!   occupant is **skipped whole**, leaving the running code untouched, whenever
//!   `occupant->Owner+0x1FA` is clear, and is **`return 7`** when it is set. A
//!   gate never yields code 3. Trigger: any
//!   move order whose path crosses a friendly gate. Player effect: VERA's code 3
//!   routes into the scatter arm, which asks a *structure* to move out of the
//!   way; retail either walks through or treats it as solid. Frequency: common
//!   from mid-game on, in every walled base. Downstream risk: the test
//!   `friendly_closed_or_opening_gate_returns_code_3_not_code_6` pins the wrong
//!   mapping and must be re-baselined with the fix.
//! - **The wall arm produces the wrong code, not no code.** `cell_rect`'s
//!   `is_wall_overlay` / `WallBlocked` path is live and MovementZone-keyed, and
//!   its Destroyer-class escape set matches native's `{2, 3, 8, 0xC}` at
//!   `0x004835BB`. But it answers a **hard block** where native answers **4** for
//!   a friendly wall and **5** for an enemy one (Unit: `OverlayTypeClass+0x2A8`
//!   at `0x0073F420` with the `Crushable=` gate `+0x22D` at `0x0073F42E`;
//!   Infantry: `5 - isAlly`), and 4 and 5 both still expand in the A*. Retail
//!   therefore routes *through* a wall line at 60×/20× cost and stops at it.
//!   **Partly addressed by I9b (2026-09-16):** the runtime cell crossing now
//!   produces 4 and 5 and dispatches the wall-attack Override, so
//!   [`CellEntryResult::FriendlyWall`] has a producer there. The **A\*** still
//!   hard-blocks, which is what the rest of this paragraph describes and what
//!   bounds the fix: because order-time search never routes into a wall cell,
//!   the arm fires only where a wall appears across an already-moving mover's
//!   path and the following repath fails. Trigger: any expansion into a
//!   `Wall=yes` overlay cell. Player
//!   effect: a move order whose destination is enclosed by walls is refused
//!   outright instead of routing to the wall and stopping. Frequency: pre-placed
//!   civilian fences appear on most stock maps, so this fires many times a match
//!   even against players who never build walls. Downstream risk: codes 4 and 5
//!   feed the blocked-step Override arm, whose wall case targets a *cell* rather
//!   than an object and has no Restore path (see `movement_occupancy`); a
//!   producer must land together with that arm. **Do not add a second wall gate
//!   on top of the existing one.**
//! - **Crushable walls admit crushers and CrusherAll; every other mover is
//!   hard-blocked where native answers 4/5 or 7.** `OverlayTypeClass+0x22D` =
//!   `Crushable=` is parsed (`OverlayTypeFlags::crushable`) and
//!   `overlay_reduced_zone_type` reduces such an overlay to zone class
//!   `CRUSHABLE` (1), as `CellClass::RecalcZoneType` @ `0x00483CB5` does. The
//!   class arm below keys on that class: a `Crusher=` type (`+0xD28`; 29 stock
//!   types, the battle tanks among them) or a `MovementZone=CrusherAll` type
//!   (`[BFRT]`) enters, matching the crusher route of `UnitClass::
//!   Can_Enter_Cell` (`0x0073F42E..F46C`), and `UnitClass::PerCellProcess`
//!   then flattens the wall on arrival (I4, `apply_wall_crush_on_driveover`).
//!   Infantry (`0x0051BF90` has no crusher route) and non-crusher vehicles are
//!   refused, where native's weapon/warhead route answers 4 (allied) or 5
//!   (enemy) for a primary warhead with `Wall=yes` (or `Wood=yes` on a wooden
//!   overlay, Unit only) and 7 otherwise; an allied crushable wall answers 4
//!   even to a crusher; ability 0x11 also takes the crusher route. Those three
//!   are the wall-arm port (the previous entry). The four stock `Crushable=
//!   yes` overlays (`[GASAND]`, `[CAFNCB]`, `[CAFNCP]`, `[CAFNCW]`) are all
//!   `Wall=yes`; a modded crushable non-wall overlay would take this arm where
//!   native applies none. Trigger: a unit with a wall-capable warhead ordered
//!   across a sandbag or fence line, or a crusher crossing its own side's.
//!   Player effect: VERA routes such a unit around the line or refuses where
//!   retail routes through at wall cost and has it shoot; a crusher on an
//!   allied line pays nothing where retail pays 60x. Stock scope of that
//!   gap: the land non-crushers whose primary warhead carries `Wall=yes` are
//!   the IFV (`[FV]`, `HoverMissile` → `HE`) and the Brute (`[BRUTE]`,
//!   `Punch` → `Battering`); every infantry rifle (`M60` → `SA`) and the
//!   other non-crusher vehicles answer 7 natively, which this arm matches.
//!   Frequency: pre-placed fences and sandbags are map dressing on several
//!   stock maps.
//! - **The head-on deadlock exit** (Unit only; the decisive instructions are the
//!   octant compare and `0x0073FA10 CMP EAX,0x1FF / JG`).
//!   Before conceding code 2 to a moving ally, native compares both objects'
//!   `FacingClass::Current` octants — the second offset by `+0x7FFF`, so the test
//!   is literally "facing each other" — and the `Math::atan2` of the lepton
//!   delta, and returns **7** when they are closing on the same octant within
//!   `Sqrt_Approx(...) < 0x200` leptons (`0x0073FA10 CMP EAX,0x1FF / JG`).
//!   Trigger: two friendly vehicles meeting head-on inside two cells. Player
//!   effect: retail makes one treat the cell as impassable and re-path; VERA has
//!   both wait and shuffle. Frequency: continuous in any traffic. Downstream
//!   risk: code 2 is also what arms the ten-step blocker-prediction loop in
//!   `AStar_compute_edge_cost`, so the wrong code feeds the wrong cost branch.
//!   `InfantryClass::Can_Enter_Cell` has no equivalent — its ally-and-moving arm
//!   goes straight to code 2.
//! - **The unarmed-mover hard block.** [`classify_blocker`] returns
//!   `OccupiedEnemy` for every non-friendly blocker; native checks armament
//!   first — Infantry `GetWeaponRange(this, -1) < 1 && What_Am_I != 0x24` →
//!   **7**. The Unit side is the same idea reached differently: the arm opens on
//!   the crush gate `((TechnoTypeClass+0xD28 == 0 && !HasWeaponAbility()) ||
//!   !Is_Crushable_By())` — where `+0xD28` is **`Crusher=`** (stored by
//!   `TechnoTypeClass::ReadINI` @ `0x00714CE3` from the key at `0x0081BB58`),
//!   **not** an armament field and **not** itself a return-7 — and the hard
//!   block inside it is `GetWeapon(0)` (`TechnoClass::GetWeapon` @ `0x0070E140`,
//!   vtable `+0x3F8`) coming back with a NULL WeaponType, escaped only by having
//!   a weapon or by `IsTrain` (`+0xC94`). There is no owner escape.
//!   Trigger: an Engineer, Spy or other
//!   weaponless unit meeting an enemy on its path. Player effect: VERA pushes it
//!   into the code-5 blocked-step attack override instead of routing around a
//!   cell it can never clear. Frequency: a few times a match for any player who
//!   uses Engineers or Spies. Downstream risk: low; it is a predicate on the
//!   mover, not the cell.
//! - **The infantry sub-cell tail.** Native's tail is explicit: `code == 0 &&
//!   (OccupationFlags & 0x1C) == 0x1C` → **7**, and — behind a `code < 2` guard
//!   at `0x0051C821` — in the allied-occupier arm `counter == 3 ? 6 : 2`
//!   (`0x0051C826`-`0x0051C830`: `SUB / NEG / SBB / AND 0xFFFFFFFC / ADD 6`, so
//!   a counter of exactly 3 gives 6 and anything else gives 2), where `counter`
//!   counts the non-moving allied infantry found during the walk. VERA's
//!   `check_terrain` returns
//!   `NeedsBlockerCheck` with no counter and no `0x1C` test. Trigger: a fourth
//!   infantryman ordered into a full friendly cell. Player effect: VERA yields 6
//!   (scatter) where retail yields 2 (wait) or 7. Frequency: constant in
//!   infantry-heavy play. Downstream risk: the counter has to be threaded
//!   through the walk, which is the one structural change on this list.
//! - **Mixed crush/vehicle occupation uses the native tail.** The semantic
//!   `Crushable` payload now maps to code0: Unit73FB67 sets the latch without
//!   raising the accumulator, and73FD37 returns0. Admission leaves killing to
//!   PerCell. With the independent vehicle bit set,73FCF6..73FD23 instead
//!   queries GetUnit and IsCrushableBy, returning2 unless that vehicle is also
//!   crushable. The runtime wrapper reads the selected raw plane and queries
//!   the first GROUND-list Unit, including self/allied entries. The supplied
//!   post-walk continuation is compared in `cell_entry_crush_tail_tests.rs`.
//!   The preceding latch producer still uses `bump_crush`'s recorded capability,
//!   target-category and frame-independent eligibility approximations; this is
//!   not complete Unit CanEnter parity.
//!   Native code1 comes from the unrelated non-allied +220 branch. The Unit
//!   latch must not be generalized to Infantry, which has no such latch.
//! - **`MovementRestrictedTo=`** (`UnitTypeClass+0xDFC`, Unit only): when set,
//!   the cell's land type must equal it. LandType 10 (Tunnel) is exempt from the
//!   equality test but carries its own rule — `g_IsometricTileTypeClass_Array`
//!   entries with `(+0x2E4, +0x2E8)` of `(5,3)` or `(4,3)` are impassable unless
//!   `bIsoSubTileIndex == 2`, and `(3,4)` or `(3,5)` unless it is `6`. The
//!   overlay window `0xED..=0xEE` escapes the return-7 only when the mover's
//!   level does **not** match the cell's (`0x0073F1FD JNZ`). Trigger: stock
//!   `rulesmd.ini` sets the key on `[HYD]`, `[SQD]`, `[ASW]` and `[HORNET]`,
//!   always `=Water`. Player effect: none observed — the two naval types are
//!   already covered by the water-mover path and the two carrier aircraft use
//!   `AircraftClass`'s own predicate. Frequency: effectively zero for this
//!   owner. Downstream risk: none.
//! - **The tube gates** (`0x0073F211` to the `RET 0x14` at `0x0073F2C6`):
//!   direction 8 — the sentinel
//!   edge `AStar_main_loop` emits as its ninth neighbour — requires a tube at the
//!   cell and then **returns 0 immediately**, skipping the whole rest of the
//!   predicate; Unit tests `tube+0x28 == 0` while **Infantry tests
//!   `tube+0x28 == tube+0x24`**. Separately, any direction whose delta from the
//!   tube's own direction falls in `3..=5` is impassable, tested both at this
//!   cell and at the back-step cell `(dir-4)&7`. Trigger: pathing into or along a
//!   tube cell. Player effect: VERA admits tube-adjacent steps native refuses,
//!   and misses the direction-8 fast admit. Frequency: tube maps only.
//!   Downstream risk: none; it is a leaf predicate.
//! - **The AI-only overlay arm** (`0x0073F3EC`-`0x0073F41D`):
//!   `OverlayTypeClass+0x2AA != 0` and the mover's house not human-controlled
//!   and `g_GameMode == 0` → 7. Infantry has the same arm without the game-mode
//!   term. Trigger: an AI mover in campaign. Player effect: none in skirmish.
//!   Frequency: zero until an AI opponent exists. Downstream risk: none.
//! - **The end-of-list land-row test.** Native checks
//!   `LandTypeSpeedBuildabilityRows[Cell->LandType][speed] == 0.0` **after** the
//!   object walk and only on the ground list; VERA applies it in the terrain
//!   head. Trigger: a bridge-deck step over a zero-row ground cell — water under
//!   a bridge. Player effect: native can still return an object code there;
//!   VERA answers impassable from the head. Frequency: every bridge crossing
//!   over water. Downstream risk: the deck plane suppresses the test in native,
//!   which VERA's `is_elevated_bridge_cell` arm already approximates in
//!   `TerrainCostGrid`, so the observable outcome usually agrees.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/bump_crush, sim/entity_store, sim/locomotor,
//!   sim/pathfinding, map/entities, map/houses, rules/locomotor_type.

use std::collections::BTreeSet;

use super::PathGrid;
use super::terrain_cost::TerrainCostGrid;
use crate::map::entities::EntityCategory;
use crate::map::houses::{self, HouseAllianceMap};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellOccupationGrid, OccupancyGrid, RawCellOccupationGrid};

// ---------------------------------------------------------------------------
// Result enums
// ---------------------------------------------------------------------------

/// Result of checking whether a unit can enter a target cell.
///
/// Maps to the original engine's Can_Enter_Cell return codes (0–7). Each variant
/// carries enough context for the movement tick to dispatch the correct
/// response without re-querying the EntityStore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellEntryResult {
    /// Code 0: Cell is passable. Enter freely.
    Clear,
    /// Code 0 with crushable occupants. Admission does not kill them; the
    /// Unit PerCell receiver owns crushing after movement reaches the cell.
    Crushable { victims: Vec<u64> },
    /// Code 2: Blocked by a moving friendly unit. Wait, then repath.
    TemporaryBlock { blocker_id: u64 },
    /// Code 2: the independent Unit occupation bit is set, but the destination
    /// object list has no blocker identity. Wait/repath without scattering.
    TemporaryOccupation,
    /// Code 3: Allied building/scatter-required soft block.
    ScatterRequired { blocker_id: Option<u64> },
    /// Code 4: Friendly wall/overlay soft block, targeting the queried cell.
    FriendlyWall,
    /// Code 5: Enemy or unowned wall, targeting the queried cell rather than
    /// inventing an object-list blocker identity.
    EnemyWall,
    /// Code 5: Enemy unit occupying. Attack blocker while waiting.
    OccupiedEnemy { blocker_id: u64 },
    /// Code 6: Friendly stationary non-building occupant.
    FriendlyStationary { blocker_id: u64 },
    /// Code 7: Terrain impassable (water, building footprint, etc.). Abort.
    Impassable,
}

impl CellEntryResult {
    pub fn yr_code(&self) -> u8 {
        match self {
            Self::Clear => 0,
            // Unit73FB67 sets the crush latch without raising the code;
            // its clear tail73FD37 returns0. Native code1 is unrelated.
            Self::Crushable { .. } => 0,
            Self::TemporaryBlock { .. } | Self::TemporaryOccupation => 2,
            Self::ScatterRequired { .. } => 3,
            Self::FriendlyWall => 4,
            Self::EnemyWall | Self::OccupiedEnemy { .. } => 5,
            Self::FriendlyStationary { .. } => 6,
            Self::Impassable => 7,
        }
    }
}

/// Phase 1 result — terrain and basic occupancy check (no EntityStore needed).
///
/// Computed inside the mutable entity borrow where we cannot also access
/// EntityStore for blocker lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainCheckResult {
    /// Cell is passable (terrain OK, occupancy clear or sub-cell available).
    Clear,
    /// Terrain impassable for this unit type.
    Impassable,
    /// Cell has occupants — needs Phase 2 EntityStore lookup to classify.
    NeedsBlockerCheck,
}

/// Terrain-only result for native-shaped cell-entry checks above `PathGrid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanEnterCellResult {
    Clear,
    HardBlocked,
    /// The wall arm answered 4 or 5: the mover cannot step here, but the search
    /// may still expand through it at the class's cost multiplier.
    ///
    /// gamemd-derived: `UnitClass::Can_Enter_Cell @ 0x0073F0A0` accumulates
    /// `max(code, 4)` for an allied wall (`0x0073F4EB`) and `max(code, 5)` for a
    /// non-allied one (`0x0073F50E`), after `HouseClass::Is_Ally_ByIndex
    /// @ 0x004F9A10` on the wall owner at `cell+0x50`. `InfantryClass::
    /// Can_Enter_Cell @ 0x0051BF90` computes the same pair as `5 - is_ally`.
    /// `AStar_compute_edge_cost @ 0x00429830` then prices them at 60x and 20x
    /// from the class table at `0x0081870C`.
    WallBlocked {
        cost_class: u8,
    },
}

/// Search-time interpretation of the YR `FootClass` cell predicate result.
///
/// This is deliberately not a terrain-speed percentage. `TerrainCostGrid` remains
/// responsible for SpeedType movement rates; this value is the small native
/// classification `AStar_compute_edge_cost` @ `0x00429830` consumes during A*
/// expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchCellCostDecision {
    /// The raw value returned by the per-Foot predicate.
    pub raw_cost_class: u8,
    /// The class supplied to the neighbor-cost routine when expansion continues.
    pub effective_cost_class: Option<u8>,
    /// Whether this neighbor may be expanded.
    pub expands: bool,
    /// Whether the normal edge-cost path is reachable.
    pub should_call_edge_cost: bool,
}

/// Apply the search-only cost-class gate used after YR's `FootClass` +0x1AC call.
///
/// Original: `AStar_main_loop` @ `0x00429A90`, immediately after the `+0x1AC`
/// call — `if (gate && class < 7) class = 0;` then reject `class >= 7`.
///
/// The gate is neither bridge nor coercion: it is `TechnoTypeClass+0xC94`, read
/// at `0x00429B64` and `0x00429C79`, which `TechnoTypeClass::ReadINI` binds at
/// `0x00712284` to the key string at `0x008444BC` = **`IsTrain`**. No stock
/// `rulesmd.ini` entry sets it, so this arm is a correctly-shaped model of a
/// mechanism nothing in stock YR enables — latent, not live.
pub fn search_cell_cost_decision(
    raw_cost_class: u8,
    coerce_to_zero_gate: bool,
) -> SearchCellCostDecision {
    let effective_cost_class = if coerce_to_zero_gate && raw_cost_class < 7 {
        0
    } else {
        raw_cost_class
    };
    let expands = effective_cost_class < 7;

    SearchCellCostDecision {
        raw_cost_class,
        effective_cost_class: expands.then_some(effective_cost_class),
        expands,
        should_call_edge_cost: expands,
    }
}

impl CanEnterCellResult {
    pub fn is_clear(self) -> bool {
        matches!(self, Self::Clear)
    }
}

/// Caller flavor for the terrain-entry slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainEntryMode {
    AStarNeighbor,
    RuntimeTransition,
    Smoothing,
    Scatter,
    SpawnLike,
}

/// Native-shaped known-input context for the terrain/layer portion of cell entry.
///
/// This deliberately stops before the unresolved search-only cost class and the
/// runtime-only blocker response. The original evaluates those with different
/// caller state; only the shared terrain/layer admission belongs here.
#[derive(Debug, Clone, Copy)]
pub struct CanEnterCellContext<'a> {
    pub target: (u16, u16),
    pub terrain_layer: MovementLayer,
    pub movement_zone: Option<MovementZone>,
    pub speed_type: Option<SpeedType>,
    pub path_grid: Option<&'a PathGrid>,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub terrain_costs: Option<&'a TerrainCostGrid>,
    pub bypass_grid: bool,
    pub mode: TerrainEntryMode,
    /// Selects the infantry view of terrain-object occupation. Retail terrain
    /// objects occupy sub-cells, and only the infantry entry gate reads that
    /// mask; vehicles stay blocked by the whole cell.
    pub is_infantry: bool,
    /// The mover's `Crusher=` type flag (`UnitTypeClass+0xD28`), the key of
    /// the crusher route of the Unit wall arm (`0x0073F438`). Infantry never
    /// takes that route; pass `false` where the mover is unknown.
    pub mover_is_crusher: bool,
    /// Wall-arm inputs. `None` keeps the coarse pre-I9b answer (a wall is a
    /// hard block), which is what every caller without a resolved mover wants.
    pub wall: Option<WallArmContext<'a>>,
}

/// The map-global tables the wall arm needs, carried on the pathfinding context.
///
/// Split from [`WallArmContext`] deliberately, and the split is the whole point
/// of the seam. These three are map-global — identical for every mover — so they
/// ride on `PathfindingContext` and no caller ever supplies them. The *mover*
/// facts (`owner`, `is_armed`, the two warhead bools) have a different lifetime,
/// one per mover, and are resolved by exactly two authorities: `snapshot_mover`
/// on the tick path and `resolve_move_info` on the order path.
///
/// Ledger row I9c is the counter-example this shape exists to avoid: a mover
/// fact (`mover_is_crusher`) was derived independently at each call site, and the
/// sites lacking context silently passed `false`, so the same unit behaved
/// differently depending on which function issued its move. Tables that no
/// caller passes cannot diverge that way.
///
/// The interner is deliberately **not** here. [`WallArmContext`] is built per
/// mover at the point of use, where an immutable reborrow is already in scope;
/// holding `&StringInterner` on a context that outlives the whole pass would
/// collide with the `&mut` the pass still needs (movement_tick.rs:4023, :4049,
/// :4114, :3855).
#[derive(Clone, Copy)]
pub struct WallArmTables<'a> {
    pub overlay_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    pub overlay_registry: Option<&'a crate::map::overlay_types::OverlayTypeRegistry>,
    pub alliances: Option<&'a crate::map::houses::HouseAllianceMap>,
    /// Map-global like the other three: the ally test resolves the wall's owner
    /// name through it before `HouseClass::Is_Ally_ByIndex @ 0x004F9A10`. It
    /// used to be supplied separately at the one runtime site that built a
    /// `WallArmContext`, which left the search site unable to build one at all
    /// without threading a second value; it belongs with the tables.
    pub interner: Option<&'a crate::sim::intern::StringInterner>,
}

/// Everything the wall arm of `Can_Enter_Cell` reads that terrain alone cannot
/// supply: the overlay at the target cell, the house that owns it, and whether
/// the mover can shoot a wall at all.
///
/// gamemd-derived: `UnitClass::Can_Enter_Cell @ 0x0073F0A0` wall arm
/// (`0x0073F3D0..0x0073F51F`) and `InfantryClass::Can_Enter_Cell @ 0x0051BF90`.
#[derive(Clone, Copy)]
pub struct WallArmContext<'a> {
    pub overlay_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    pub overlay_registry: Option<&'a crate::map::overlay_types::OverlayTypeRegistry>,
    pub alliances: Option<&'a crate::map::houses::HouseAllianceMap>,
    pub interner: Option<&'a crate::sim::intern::StringInterner>,
    /// The mover's owning house, compared with the wall's owner through
    /// `HouseClass::Is_Ally_ByIndex @ 0x004F9A10`.
    pub mover_owner: Option<crate::sim::intern::InternedId>,
    /// `TechnoClass::Is_Armed @ 0x00701120` (vtable `+0x2AC`, primary weapon
    /// slot non-null). False answers 7 at `0x0073F48F`.
    pub is_armed: bool,
    /// Primary warhead `Wall=` (`WarheadTypeClass+0x144`, tested at
    /// `0x0073F4A9`).
    pub warhead_wall: bool,
    /// Primary warhead `Wood=` (`+0x147`, tested at `0x0073F4B3`), which only
    /// admits an overlay whose `Armor=` is wood (`0x0073F4BD` compares 6).
    pub warhead_wood: bool,
}

// `OverlayTypeRegistry` carries no `Debug`, and adding one there would touch a
// rules type for a pathfinding convenience. `CanEnterCellContext` derives
// `Debug`, so this prints the mover-side facts and elides the borrowed tables.
impl std::fmt::Debug for WallArmContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WallArmContext")
            .field("mover_owner", &self.mover_owner)
            .field("is_armed", &self.is_armed)
            .field("warhead_wall", &self.warhead_wall)
            .field("warhead_wood", &self.warhead_wood)
            .field("overlay_grid", &self.overlay_grid.is_some())
            .field("overlay_registry", &self.overlay_registry.is_some())
            .field("alliances", &self.alliances.is_some())
            .finish()
    }
}

impl WallArmContext<'_> {
    /// The wall overlay at `cell`, as `(is_wall, armor_is_wood, owner)`.
    fn overlay_at(
        &self,
        cell: (u16, u16),
    ) -> Option<(bool, bool, Option<crate::sim::intern::InternedId>)> {
        let grid = self.overlay_grid?;
        let registry = self.overlay_registry?;
        let overlay = grid.cell(cell.0, cell.1);
        let flags = overlay.overlay_id.and_then(|id| registry.flags(id))?;
        Some((flags.wall, flags.armor_is_wood, overlay.wall_owner))
    }

    /// `HouseClass::Is_Ally_ByIndex @ 0x004F9A10`: true for the mover's own
    /// house, false for an unowned wall (index -1), else the ally bitfield.
    fn owner_is_ally(&self, wall_owner: Option<crate::sim::intern::InternedId>) -> bool {
        let (Some(alliances), Some(interner), Some(mover), Some(wall)) =
            (self.alliances, self.interner, self.mover_owner, wall_owner)
        else {
            return false;
        };
        // RESIDUAL (ledger I9b): `is_allied_with` normalizes both names, i.e. two
        // String allocations per query, and this runs once per refused neighbour
        // inside the A* loop. Pre-existing, not introduced here. An id-keyed
        // lookup is the right fix but must be built once per *frame* and cached
        // (`MovementPassCache`) — a first attempt rebuilt it per object per frame,
        // which is strictly worse, and silently dropped alliance rows for house
        // names carrying whitespace because the interner does not trim.
        crate::map::houses::is_allied_with(
            alliances,
            interner.resolve(mover),
            interner.resolve(wall),
        )
    }

    /// The accumulated wall code, or `None` when the arm answers 7.
    ///
    /// `0x0073F483..0x0073F51F`: an unarmed mover exits through the shared
    /// epilogue at `0x0073FCD0` (branch `JZ` at `0x0073F48F`); one whose primary
    /// warhead is neither `Wall=` nor `Wood=`-against-wood returns 7 at its own
    /// exit, `0x0073F4C9`. Otherwise the ally test picks `max(code, 4)` or
    /// `max(code, 5)`.
    ///
    /// `is_infantry` gates the `Wood=` clause, which is **Unit-only**. The
    /// infantry arm (`InfantryClass::Can_Enter_Cell 0x0051BF90`) resolves its
    /// warhead through `FUN_00772AC0`, whose whole body is one test of
    /// `+0x144` (`Wall=`) — no `+0x147`, no `Armor == 6` compare — and then
    /// takes `5 - Is_Ally_ByIndex`. Stock-reachable: `[SHK]` (Primary
    /// `ElectricBolt` -> warhead `[Shock]`, `Wood=yes` with no `Wall=`) against
    /// `[CAKRMW]` (`Armor=wood`, `Crushable=no`).
    fn weapon_route_code(
        &self,
        armor_is_wood: bool,
        wall_owner: Option<crate::sim::intern::InternedId>,
        is_infantry: bool,
    ) -> Option<u8> {
        if !self.is_armed {
            return None;
        }
        let wood_route = !is_infantry && self.warhead_wood && armor_is_wood;
        if !(self.warhead_wall || wood_route) {
            return None;
        }
        // Fail closed on a wiring gap rather than guessing "enemy". Native
        // always has a house to ask, so absent alliance tables here are a VERA
        // wiring mistake, not a game state — and answering 5 would be
        // indistinguishable from a genuinely unowned wall while quietly pricing
        // it at 20x. Declining keeps the pre-I9b hard block, which is the same
        // "absent context => pre-I9b behaviour" rule the rest of the arm
        // follows, and it matters because the producer has to reach many call
        // sites: a site wired without tables then refuses walls instead of
        // silently mis-pricing them. An unowned wall (`wall_owner: None` with
        // the tables present) still takes 5 — native's index -1.
        if self.alliances.is_none() || self.interner.is_none() || self.mover_owner.is_none() {
            return None;
        }
        Some(if self.owner_is_ally(wall_owner) { 4 } else { 5 })
    }
}

/// Prices a candidate cell for A* expansion through the wall arm.
///
/// gamemd-derived: `AStar_main_loop @ 0x00429A90` calls the Foot `+0x1AC` slot
/// for each neighbour and hands the returned code to
/// `AStar_compute_edge_cost @ 0x00429830`, which indexes the class base table
/// at `0x0081870C` — `[1.0, 1000.0, 1.0, 1.0, 60.0, 20.0, 8.0, 10000.0]`, so an
/// allied wall (4) expands at 60x and an enemy one (5) at 20x. A code of 7 stops
/// the expansion in `search_cell_cost_decision`.
///
/// This is the production producer for the `search_cost_classifier` seam: it
/// carries the mover facts the pure pathfinding layer cannot resolve on its own.
pub struct WallSearchCostClassifier<'a> {
    pub wall: WallArmContext<'a>,
    pub path_grid: Option<&'a PathGrid>,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub terrain_costs: Option<&'a TerrainCostGrid>,
    pub movement_zone: Option<MovementZone>,
    pub speed_type: Option<SpeedType>,
    pub is_infantry: bool,
    pub mover_is_crusher: bool,
}

impl crate::sim::pathfinding::SearchCellCostClassifier for WallSearchCostClassifier<'_> {
    fn classify(&self, _from: (u16, u16), candidate: (u16, u16), bridge: bool) -> u8 {
        let terrain_layer = if bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        match evaluate_can_enter_cell(CanEnterCellContext {
            wall: Some(self.wall),
            target: candidate,
            terrain_layer,
            movement_zone: self.movement_zone,
            speed_type: self.speed_type,
            path_grid: self.path_grid,
            resolved_terrain: self.resolved_terrain,
            terrain_costs: self.terrain_costs,
            bypass_grid: false,
            mode: TerrainEntryMode::AStarNeighbor,
            is_infantry: self.is_infantry,
            mover_is_crusher: self.mover_is_crusher,
        }) {
            // `Clear` is NOT class 0 here. `astar_search` consults this
            // classifier only after its own `neighbor_passable` has already
            // refused the cell, and that check carries terms this cell-scoped
            // evaluation cannot see — the ground/bridge layer split, and the
            // `neighbor_cell.transition` (`0x200`) gate a ground->bridge entry
            // must pass. Answering 0 would re-admit a refused neighbour at 1x:
            // a passable deck whose `transition` is clear would be entered
            // anyway. Only a wall this mover may shoot changes the outcome.
            CanEnterCellResult::Clear => 7,
            CanEnterCellResult::WallBlocked { cost_class } => cost_class,
            CanEnterCellResult::HardBlocked => 7,
        }
    }
}

/// Evaluate the shared terrain/layer slice of Can_Enter_Cell.
///
/// `PathGrid` is a coarse structural filter. Final terrain legality must also
/// consult the mover's SpeedType against the resolved target LandType/speed row
/// so a PathGrid-walkable water cell is still illegal for ordinary ground movers.
// Original: the `FootClass` `+0x1AC` slot — `UnitClass::Can_Enter_Cell` @
// `0x0073F0A0` (`0x007F5E1C`) and `InfantryClass::Can_Enter_Cell` @
// `0x0051BF90` (`0x007EB204`). There is no `EvaluateCellEnterabilityOrCost`
// symbol in this program; the earlier name here was invented.
pub fn evaluate_can_enter_cell(ctx: CanEnterCellContext<'_>) -> CanEnterCellResult {
    match ctx.terrain_layer {
        MovementLayer::Ground => evaluate_ground_cell_entry(ctx),
        MovementLayer::Bridge => {
            let bridge_walkable = ctx.path_grid.is_some_and(|grid| {
                grid.is_walkable_on_layer(ctx.target.0, ctx.target.1, MovementLayer::Bridge)
            });
            // The deck branch never reaches the land row: `0x0073FA92` tests the
            // deck flag and jumps past the read (`JNZ 0x0073FC24`), so the row
            // cannot refuse a bridge-layer entry.
            evaluate_shared_cell_leaf(ctx, bridge_walkable, true)
        }
        // Air and underground locomotors are admitted by their dedicated
        // locomotion state machines, not this ground/bridge terrain slice.
        MovementLayer::Air | MovementLayer::Underground => CanEnterCellResult::Clear,
    }
}

fn evaluate_ground_cell_entry(ctx: CanEnterCellContext<'_>) -> CanEnterCellResult {
    let (x, y) = ctx.target;

    if let Some(movement_zone) = ctx.movement_zone.filter(|zone| zone.is_water_mover()) {
        let land_passable = ctx
            .resolved_terrain
            .and_then(|terrain| terrain.cell(x, y))
            .is_some_and(|cell| is_water_surface_cell_passable(cell, movement_zone));
        // A water mover's surface test above already stands in for the row on
        // this VERA-internal branch; native has one path and would read the row
        // here too. UNCHECKED, and inert in practice (walls are not placed on
        // open water), so the row is not made to refuse anything extra.
        return evaluate_shared_cell_leaf(ctx, land_passable, true);
    }

    let grid_ok = ctx.path_grid.map_or(true, |grid| {
        ctx.bypass_grid
            || if ctx.is_infantry {
                grid.is_walkable_for_infantry(x, y)
            } else {
                grid.is_walkable(x, y)
            }
    });
    let speed_type = ctx.speed_type.or_else(|| {
        if ctx.terrain_costs.is_some() && !ctx.is_infantry {
            None
        } else {
            // InfantryClass::Can_Enter_Cell @ 0x0051C750 reads the ground
            // LandType/SpeedType row even beneath an intact bridge. The coarse
            // cost grid also represents the deck and can contain 100 over a
            // ground Foot row of zero, so it cannot replace this input.
            ctx.movement_zone.map(|zone| zone.speed_type())
        }
    });
    let speed_passable = speed_type.is_none_or(|speed_type| {
        ctx.resolved_terrain
            .and_then(|terrain| terrain.cell(x, y))
            .is_none_or(|cell| speed_type_allows_cell(cell, speed_type))
    });
    // A mover on the ground plane of a stamped cell reads the land row of the
    // terrain itself, not the deck's override: `UnitClass::Can_Enter_Cell`
    // @ `0x0073F0A0` clears its deck flag for a path height within one of the
    // cell's signed level (`0x0073F0B7..F0E8`), walks the ground list `+0xE4`
    // (`0x0073F51A`) and then tests the row at `0x0073FAB5` only on that
    // branch (`0x0073FA92`); Infantry does the same at `0x0051C750`. The planner's cost
    // grid carries the deck answer on those cells, so consult its ground row.
    let terrain_cost_passable = match ctx.terrain_costs {
        Some(costs) if target_has_structural_bridge(ctx) => costs.ground_cost_at(x, y) != 0,
        Some(costs) => costs.cost_at(x, y) != 0,
        None => true,
    };

    evaluate_shared_cell_leaf(
        ctx,
        grid_ok && speed_passable && terrain_cost_passable,
        speed_passable,
    )
}

/// Whether the target carries the native `CellClass+0x140 & 0x100` stamp.
fn target_has_structural_bridge(ctx: CanEnterCellContext<'_>) -> bool {
    ctx.path_grid
        .and_then(|grid| grid.cell(ctx.target.0, ctx.target.1))
        .is_some_and(|cell| cell.has_structural_bridge())
        || ctx
            .resolved_terrain
            .and_then(|terrain| terrain.cell(ctx.target.0, ctx.target.1))
            .is_some_and(|cell| cell.bridge_facts.has_structural_bridge())
}

/// The shared tail of both `Can_Enter_Cell` implementations.
///
/// `land_passable` is VERA's coarse "may this mover stand here at all" answer:
/// the path grid, the speed row and the cost grid folded together.
///
/// `land_row_passable` is the cell's **land row** — `speed_type_allows_cell`,
/// the analogue of native's `FLD [ECX*4 + 0x89EA40]` / `FCOMP 0.0` at
/// `0x0073FAB5`.
///
/// It is a separate parameter because the wall arm needs the row without the
/// two *blocking* terms the coarse answer folds in: `grid_ok` and the cost grid
/// both go false on `overlay_blocks`, which `ResolvedTerrainGrid` sets for every
/// `zone_class::WALL` overlay, so gating the wall classes on `land_passable`
/// would refuse the very cells the arm exists to price.
///
/// The row itself is *not* overlay-free, and the distinction matters:
/// `apply_overlay_land` writes `cell.speed_costs` from the overlay's own `Land=`
/// row, exactly as native does — `CellClass::RecalcAttributes @ 0x0047D2B0`
/// opens with `this->LandType = ot->Land` (`+0x298`) and early-returns on
/// `Land == 4`/`9` or `NoUseTileLandType` (`+0x2AC`), which
/// `uses_early_recalc_land_branch` ports. So reading the post-overlay row is the
/// faithful analogue, not an accident.
///
/// In stock data this gate never fires: no `Wall=yes` overlay declares `Land=`,
/// so each inherits `LandType::Clear` and its passable row rather than the
/// all-zero `[Wall]` row. It bites only where an overlay declares a land whose
/// row is zero for the mover's SpeedType.
fn evaluate_shared_cell_leaf(
    ctx: CanEnterCellContext<'_>,
    land_passable: bool,
    land_row_passable: bool,
) -> CanEnterCellResult {
    let structural_bridge = target_has_structural_bridge(ctx);
    let bridge_transition = ctx
        .path_grid
        .and_then(|grid| grid.cell(ctx.target.0, ctx.target.1))
        .is_some_and(|cell| cell.is_bridge_transition_cell())
        || ctx
            .resolved_terrain
            .and_then(|terrain| terrain.cell(ctx.target.0, ctx.target.1))
            .is_some_and(|cell| cell.is_bridge_transition_cell());
    if bridge_transition || (ctx.terrain_layer == MovementLayer::Bridge && !structural_bridge) {
        // Native `IsClearToMove` receives an integer level, not the engine's
        // path-layer enum. A bridgehead can carry Ground while an already-on-
        // bridge mover remains at deck height, so guessing base/base+4 here
        // rejects the proved Body->Ramp->Ground transition. Until +0x1AC
        // threads its numeric path height, retain the prior structural gate.
        return if land_passable {
            CanEnterCellResult::Clear
        } else {
            CanEnterCellResult::HardBlocked
        };
    }

    let Some(speed_type) = ctx
        .speed_type
        .or_else(|| ctx.movement_zone.map(|zone| zone.speed_type()))
    else {
        return if land_passable {
            CanEnterCellResult::Clear
        } else {
            CanEnterCellResult::HardBlocked
        };
    };
    let movement_zone = ctx.movement_zone.unwrap_or(MovementZone::Normal);
    if ctx.terrain_layer == MovementLayer::Ground && speed_type != SpeedType::Winged {
        // Neither Foot +0x1AC implementation calls Cell::CheckCellPassability
        // @ 0x004834A0 — its callers are CellRect::CheckPassability, threat
        // scans, placement, paradrop, overlay Mark and Jumpjet touchdown, none of
        // them a Foot entry test. Infantry +0x1AC @ 0x0051BF90 and Unit +0x1AC
        // @ 0x0073F0A0 (UnitClass vtable 0x007F5C70 + 0x1AC = 0x007F5E1C) both
        // defer numeric height legality to the shared +0x1B0 traversal @
        // 0x004D9C60, whose equal-level arm admits base-height ground beneath a
        // span; ground near the candidate's level then selects the ground list
        // (+0xE4/+0x124) even when the cell carries the 0x100 stamp. A* @
        // 0x00429F54, Walk @ 0x0075B690 and the Drive crossing admission reach
        // this class contract through the virtual slot. Keep the existing
        // coarse wall result here (the native 4/5 wall accumulator is a separate
        // recorded gap), without importing the unrelated Cell leaf's level
        // rejection. See RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md, under-span admission.
        let terrain_cell = ctx
            .resolved_terrain
            .and_then(|terrain| terrain.cell(ctx.target.0, ctx.target.1));
        if terrain_cell.is_none()
            && ctx
                .path_grid
                .and_then(|grid| grid.cell(ctx.target.0, ctx.target.1))
                .is_none()
        {
            // Retain the prior live-adapter missing-target rejection, including
            // bypass_grid callers. This does not model native dummy-cell access.
            return CanEnterCellResult::HardBlocked;
        }
        let wall = terrain_cell.is_some_and(|cell| cell.zone_type == zone_class::WALL);
        let wall_cleared = wall
            && matches!(
                movement_zone,
                MovementZone::Destroyer
                    | MovementZone::AmphibiousDestroyer
                    | MovementZone::InfantryDestroyer
                    | MovementZone::CrusherAll
            );
        // A `Crushable=` overlay reduces to zone class 1 in `RecalcZoneType`
        // (`overlay_reduced_zone_type`), never to `WALL`, so the wall test
        // above cannot see a sandbag or fence. The four stock `Crushable=yes`
        // overlays (`[GASAND]`, `[CAFNCB]`, `[CAFNCP]`, `[CAFNCW]`) are all
        // `Wall=yes`, so class 1 stands for "crushable wall" here; a modded
        // crushable non-wall overlay would take this arm where native applies
        // none. Native `UnitClass::Can_Enter_Cell` wall arm `0x0073F42E..F46E`:
        // `Crushable=` (+0x22D) with the type's `Crusher=` (+0xD28) or ability
        // 0x11 enters (code unchanged when the wall is not allied, 4 when it
        // is); `0x0073F455..F46C`: `MovementZone=CrusherAll` (+0x5B4 == 0xC)
        // enters any `Wall=`; everything else, and every infantryman
        // (`0x0051BF90` has no crusher route), takes the weapon/warhead route
        // that answers 4/5 or 7. The runtime crossing produces 4/5 since
        // 2026-09-16; the A* search still does not (its classifier has no
        // production construction site), so that
        // route is the hard block below; the allied-wall 4 and ability 0x11
        // are likewise unmodelled.
        let crushable_wall =
            terrain_cell.is_some_and(|cell| cell.zone_type == zone_class::CRUSHABLE);
        let crushable_wall_admitted =
            !ctx.is_infantry && (ctx.mover_is_crusher || movement_zone == MovementZone::CrusherAll);

        // The wall arm's weapon route (`0x0073F483..0x0073F51F`): an armed mover
        // whose primary warhead is `Wall=`, or `Wood=` against an `Armor=wood`
        // overlay (Unit only — the infantry arm's `FUN_00772AC0` tests `Wall=`
        // alone), accumulates 4 against an allied wall and 5 against any other
        // — an unowned wall is index -1, which `Is_Ally_ByIndex` rejects, so it
        // takes 5. An unarmed mover leaves through the shared epilogue at
        // `0x0073FCD0`; a warhead miss returns 7 at `0x0073F4C9`.
        //
        // With `ctx.wall` absent this stays `None` and the coarse pre-I9b hard
        // block is returned unchanged, which is what every caller that has not
        // resolved a mover wants.
        //
        // Residual: the crusher route's allied-wall `max(code, 4)` (`0x0073F481`
        // jumps into the same accumulator at `0x0073F4EB`) is not modelled, so a
        // crusher still enters a crushable wall freely whoever owns it. Changing
        // that also moves I4's wall-crush admission, so it is recorded rather
        // than folded in here.
        let wall_attack_code: Option<u8> = ctx.wall.and_then(|wall_ctx| {
            let (is_wall, armor_is_wood, owner) = wall_ctx.overlay_at(ctx.target)?;
            if !is_wall {
                return None;
            }
            wall_ctx.weapon_route_code(armor_is_wood, owner, ctx.is_infantry)
        });

        // The wall arm does NOT return in native. It accumulates 4/5 into the
        // running code, falls through the occupant walk, and then reads the
        // ground land row at `0x0073FAB5` (`FLD [ECX*4 + 0x89EA40]`, `FCOMP
        // 0.0`); a zero row returns 7 at `0x0073FAD0` whatever the arm
        // accumulated, and `InfantryClass` does the same at `0x0051C7D0`. So a
        // wall overlay on terrain whose speed row refuses this mover answers 7,
        // not 4/5 - the classes survive only where the LAND ROW admits, which
        // is not the same as the terrain beneath. Review corrected this:
        // `OverlayTypeFlags::no_use_tile_land_type` defaults **true**, so a
        // stock wall overlay writes its own `Clear` row into the cell and a
        // wall over water or rock reads as passable here. Native reads the
        // same stored LandType at `0x0073FAB5`, so this is plausibly parity
        // rather than a defect, and stock maps do not place walls on water -
        // but what the code checks is the stored row, not the ground.
        //
        // `land_row_passable` is that row alone, deliberately not
        // `land_passable`: see this function's doc for why the wider term would
        // refuse every wall and leave the arm dead.
        if crushable_wall && !crushable_wall_admitted {
            return match wall_attack_code.filter(|_| land_row_passable) {
                Some(cost_class) => CanEnterCellResult::WallBlocked { cost_class },
                None => CanEnterCellResult::HardBlocked,
            };
        }
        return if !wall_cleared && (wall || !land_passable) {
            match wall_attack_code.filter(|_| wall && land_row_passable) {
                Some(cost_class) => CanEnterCellResult::WallBlocked { cost_class },
                None => CanEnterCellResult::HardBlocked,
            }
        } else {
            CanEnterCellResult::Clear
        };
    }
    let result = evaluate_live_cell_passability(LiveCellPassabilityQuery {
        target: ctx.target,
        speed_type,
        movement_zone,
        // FootClass +0x1AC owns zone calculation outside the shared Cell leaf.
        requested_zone: None,
        actual_zone: 0,
        requested_layer: Some(ctx.terrain_layer),
        ignore_infantry: false,
        ignore_vehicles: false,
        land_passable,
        path_grid: ctx.path_grid,
        resolved_terrain: ctx.resolved_terrain,
        // Object-list and raw occupation classification remain the later
        // class-specific +0x1AC arms and must not be collapsed into terrain.
        raw_occupation: None,
    });
    if matches!(
        result,
        IsClearToMoveResult::Clear { .. } | IsClearToMoveResult::ClearWinged
    ) {
        CanEnterCellResult::Clear
    } else {
        CanEnterCellResult::HardBlocked
    }
}

/// Ship movement must use the reduced ZoneType matrix rather than PathGrid's
/// ground walkability, with the confirmed coastal-water compatibility fallback.
pub(crate) fn is_water_surface_cell_passable(
    cell: &ResolvedTerrainCell,
    movement_zone: MovementZone,
) -> bool {
    let matrix_ok = super::passability::is_passable_for_zone(cell.zone_type, movement_zone);
    if matrix_ok || cell.is_water {
        return true;
    }
    movement_zone == MovementZone::WaterBeach && cell.zone_type == zone_class::BEACH
}

fn speed_type_allows_cell(cell: &ResolvedTerrainCell, speed_type: SpeedType) -> bool {
    cell.speed_costs
        .cost_for_speed_type(speed_type)
        .is_none_or(|cost| cost > 0)
}

/// Layer selections used by Can_Enter_Cell-style checks.
///
/// The common case uses one layer for all phases. Bridge traversal may select
/// the bridge object list while the post-traversal occupancy bits remain ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanEnterLayerContext {
    pub terrain_layer: MovementLayer,
    pub object_list_layer: MovementLayer,
    pub occupancy_bits_layer: MovementLayer,
}

impl CanEnterLayerContext {
    pub fn single(layer: MovementLayer) -> Self {
        Self {
            terrain_layer: layer,
            object_list_layer: layer,
            occupancy_bits_layer: layer,
        }
    }
}

/// Read-only cell-entry oracle row preserving gamemd's split layer decisions.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct CellEntryOracleRow {
    pub target: (u16, u16),
    pub terrain_layer: MovementLayer,
    pub object_list_layer: MovementLayer,
    pub occupancy_bits_layer: MovementLayer,
    pub terrain_result: String,
    pub yr_code: Option<u8>,
    pub occupancy_ground_present: bool,
    pub occupancy_bridge_present: bool,
}

impl CellEntryOracleRow {
    pub fn from_terrain_result(
        target: (u16, u16),
        layers: CanEnterLayerContext,
        result: TerrainCheckResult,
        occupancy: &OccupancyGrid,
    ) -> Self {
        let occ = occupancy.get(target.0, target.1);
        Self {
            target,
            terrain_layer: layers.terrain_layer,
            object_list_layer: layers.object_list_layer,
            occupancy_bits_layer: layers.occupancy_bits_layer,
            terrain_result: format!("{:?}", result),
            yr_code: match result {
                TerrainCheckResult::Clear => Some(CellEntryResult::Clear.yr_code()),
                TerrainCheckResult::Impassable => Some(CellEntryResult::Impassable.yr_code()),
                TerrainCheckResult::NeedsBlockerCheck => None,
            },
            occupancy_ground_present: occ.is_some_and(|o| !o.is_empty_on(MovementLayer::Ground)),
            occupancy_bridge_present: occ.is_some_and(|o| !o.is_empty_on(MovementLayer::Bridge)),
        }
    }
}

/// Opt-in diagnostic wrapper for Phase-1 cell entry checks.
pub fn check_terrain_with_layers_oracle(
    target: (u16, u16),
    layers: CanEnterLayerContext,
    mover_category: EntityCategory,
    path_grid: Option<&PathGrid>,
    cost_grid: Option<&TerrainCostGrid>,
    occupancy: &OccupancyGrid,
) -> (TerrainCheckResult, CellEntryOracleRow) {
    let result = check_terrain_with_layers(
        target,
        layers,
        mover_category,
        path_grid,
        cost_grid,
        occupancy,
    );
    let row = CellEntryOracleRow::from_terrain_result(target, layers, result, occupancy);
    (result, row)
}

/// Vehicle-only building entry branch that may reach the live row helper.
///
/// InfantryClass::Can_Enter_Cell does not use the radio/contact or
/// UnitRepair/Bunker NumberImpassableRows branches, so callers must not use this
/// as a shared infantry rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VehicleBuildingEntryBranch {
    /// Contact-vector branch. The caller must supply whether this mover has
    /// RadioClass contact with the checked building.
    RadioContact { mover_has_contact: bool },
    /// UnitRepair/Bunker branch. This branch is gated by the checked building's
    /// type flags, not by RadioClass contact.
    UnitRepairOrBunker,
}

/// Decision for a checked building occupant in UnitClass-style cell entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingOccupantEntryDecision {
    /// Keep the checked building in the ordinary blocker classification path.
    KeepBlocker,
    /// Skip this building occupant and continue scanning later occupants in the
    /// cell's object list.
    SkipBlocker,
}

/// Explicit live facts needed by the UnitClass building row-helper decision.
///
/// Caller responsibilities:
/// - `candidate_building_id` must be the result of a live
///   Look_up_building_in_cell-style lookup for the candidate cell.
/// - `checked_building_id` and type/runtime flags must describe the building
///   occupant currently being inspected.
/// - `mover_category` must be the mover's semantic category; only UnitClass-style
///   vehicle movers use these exceptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveVehicleBuildingEntry {
    pub mover_category: EntityCategory,
    pub branch: VehicleBuildingEntryBranch,
    pub checked_building_id: u64,
    pub candidate_building_id: Option<u64>,
    pub candidate_x: u16,
    pub building_origin_x: u16,
    pub number_impassable_rows: i32,
    pub is_unit_repair: bool,
    pub is_bunker: bool,
    pub bunker_occupied: bool,
}

/// Decide whether UnitClass-style movement should skip a building occupant.
///
/// This models `FUN_00458A00` at its two UnitClass::Can_Enter_Cell callsites:
/// radio/contact and UnitRepair/Bunker. A `KeepBlocker` result means the caller
/// should continue with the existing Can_Enter_Cell return-code classification;
/// `SkipBlocker` means only this building occupant is ignored.
pub fn decide_live_vehicle_building_entry(
    input: LiveVehicleBuildingEntry,
) -> BuildingOccupantEntryDecision {
    if input.mover_category != EntityCategory::Unit {
        return BuildingOccupantEntryDecision::KeepBlocker;
    }

    let branch_active = match input.branch {
        VehicleBuildingEntryBranch::RadioContact { mover_has_contact } => mover_has_contact,
        VehicleBuildingEntryBranch::UnitRepairOrBunker => input.is_unit_repair || input.is_bunker,
    };
    if !branch_active {
        return BuildingOccupantEntryDecision::KeepBlocker;
    }

    if input.candidate_building_id != Some(input.checked_building_id) {
        //458A00 returns false on identity mismatch. The radio caller73F5A2
        //skips then; UnitRepair/Bunker73F761 has a separate equality gate.
        return match input.branch {
            VehicleBuildingEntryBranch::RadioContact { .. } => {
                BuildingOccupantEntryDecision::SkipBlocker
            }
            VehicleBuildingEntryBranch::UnitRepairOrBunker => {
                BuildingOccupantEntryDecision::KeepBlocker
            }
        };
    }
    if input.number_impassable_rows == -1 {
        return BuildingOccupantEntryDecision::KeepBlocker;
    }
    if input.is_bunker && input.bunker_occupied {
        return BuildingOccupantEntryDecision::KeepBlocker;
    }

    let first_clear_x = i32::from(input.building_origin_x) + input.number_impassable_rows;
    if i32::from(input.candidate_x) >= first_clear_x {
        BuildingOccupantEntryDecision::SkipBlocker
    } else {
        BuildingOccupantEntryDecision::KeepBlocker
    }
}

// ---------------------------------------------------------------------------
// Phase 1: terrain + occupancy presence
// ---------------------------------------------------------------------------

/// Check terrain walkability and basic occupancy for a target cell.
///
/// This is Phase 1 of the two-phase cell entry check. It does NOT access
/// EntityStore, so it can run inside a mutable entity borrow.
///
/// For infantry movers, also checks sub-cell availability.
pub fn check_terrain(
    target: (u16, u16),
    target_layer: MovementLayer,
    mover_category: EntityCategory,
    path_grid: Option<&PathGrid>,
    cost_grid: Option<&TerrainCostGrid>,
    occupancy: &OccupancyGrid,
) -> TerrainCheckResult {
    check_terrain_with_layers(
        target,
        CanEnterLayerContext::single(target_layer),
        mover_category,
        path_grid,
        cost_grid,
        occupancy,
    )
}

/// Check terrain and occupancy using explicit CanEnter layer selections.
pub fn check_terrain_with_layers(
    target: (u16, u16),
    layers: CanEnterLayerContext,
    mover_category: EntityCategory,
    path_grid: Option<&PathGrid>,
    cost_grid: Option<&TerrainCostGrid>,
    occupancy: &OccupancyGrid,
) -> TerrainCheckResult {
    let (nx, ny) = target;

    // --- Terrain walkability ---
    let terrain_walkable = evaluate_can_enter_cell(CanEnterCellContext {
        wall: None,
        target,
        terrain_layer: layers.terrain_layer,
        movement_zone: None,
        speed_type: None,
        path_grid,
        resolved_terrain: None,
        terrain_costs: cost_grid,
        bypass_grid: false,
        mode: TerrainEntryMode::RuntimeTransition,
        is_infantry: mover_category == EntityCategory::Infantry,
        // No resolved terrain is supplied here, so the wall arm never runs.
        mover_is_crusher: false,
    })
    .is_clear();
    if !terrain_walkable {
        return TerrainCheckResult::Impassable;
    }

    // --- Occupancy ---
    let occ = occupancy.get(nx, ny);

    if mover_category == EntityCategory::Infantry {
        let selected_list_blocked =
            occ.is_some_and(|o| o.has_blockers_on(layers.object_list_layer));
        let sub =
            bump_crush::allocate_sub_cell_with_reserved(occ, layers.occupancy_bits_layer, None);
        if sub.is_some() && !selected_list_blocked {
            return TerrainCheckResult::Clear;
        }
        // No sub-cell available — needs blocker classification.
        return TerrainCheckResult::NeedsBlockerCheck;
    }

    // Vehicle/aircraft/structure: cell must be unoccupied on this layer.
    match occ {
        None => TerrainCheckResult::Clear,
        Some(o)
            if o.is_empty_on(layers.object_list_layer)
                && o.is_empty_on(layers.occupancy_bits_layer) =>
        {
            TerrainCheckResult::Clear
        }
        Some(_) => TerrainCheckResult::NeedsBlockerCheck,
    }
}

// ---------------------------------------------------------------------------
// Phase 2: blocker classification (needs EntityStore)
// ---------------------------------------------------------------------------

/// Classify an occupied cell's blockers to determine the Can_Enter_Cell code.
///
/// This is Phase 2 — runs outside the mutable entity borrow so it can read
/// blocker properties from EntityStore.
///
/// Check order, mirroring `UnitClass::Can_Enter_Cell` @ `0x0073F0A0`'s object
/// walk:
/// 1. Crush: crushability is a latch consulted after the walk, not an early exit
/// 2. Blocker friendship: enemy → OccupiedEnemy, friendly → moving/stationary
/// 3. JumpJet override: codes < 7 treated as Clear
///
/// The terrain and layer half of the native predicate runs before this, in
/// [`evaluate_can_enter_cell`]. The arms of the native walk this phase does not
/// produce — the wall/overlay codes and the head-on facing test — are recorded
/// in the module header rather than approximated here.
pub fn classify_occupied_cell(
    target: (u16, u16),
    target_layer: MovementLayer,
    mover_id: u64,
    crush_capability: bump_crush::CrushCapability,
    mover_owner: &str,
    mover_locomotor: LocomotorKind,
    mover_bypass_grid: bool,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    alliances: &HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
) -> CellEntryResult {
    classify_occupied_cell_with_layers(
        target,
        CanEnterLayerContext::single(target_layer),
        mover_id,
        crush_capability,
        mover_owner,
        mover_locomotor,
        mover_bypass_grid,
        occupancy,
        entities,
        alliances,
        interner,
    )
}

/// Classify an occupied cell using explicit CanEnter layer selections.
#[allow(clippy::too_many_arguments)]
pub fn classify_occupied_cell_with_layers(
    target: (u16, u16),
    layers: CanEnterLayerContext,
    mover_id: u64,
    crush_capability: bump_crush::CrushCapability,
    mover_owner: &str,
    mover_locomotor: LocomotorKind,
    mover_bypass_grid: bool,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    alliances: &HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
) -> CellEntryResult {
    classify_occupied_cell_with_layers_and_ignored(
        target,
        layers,
        mover_id,
        crush_capability,
        mover_owner,
        mover_locomotor,
        mover_bypass_grid,
        None,
        occupancy,
        entities,
        alliances,
        interner,
    )
}

/// Classify an occupied cell while ignoring a caller-supplied subset of live
/// object-list occupants. This is the runtime UnitClass path used by refinery
/// pads and repair/bunker rows where gamemd skips only the checked building
/// occupant, then continues scanning the same cell list.
#[allow(clippy::too_many_arguments)]
pub fn classify_occupied_cell_with_layers_and_ignored(
    target: (u16, u16),
    layers: CanEnterLayerContext,
    mover_id: u64,
    crush_capability: bump_crush::CrushCapability,
    mover_owner: &str,
    mover_locomotor: LocomotorKind,
    mover_bypass_grid: bool,
    ignored_blockers: Option<&BTreeSet<u64>>,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    alliances: &HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
) -> CellEntryResult {
    classify_occupied_cell_with_slave_query(
        target,
        layers,
        mover_id,
        crush_capability,
        mover_owner,
        mover_locomotor,
        mover_bypass_grid,
        ignored_blockers,
        occupancy,
        entities,
        alliances,
        interner,
        None,
        &mut false,
        false,
    )
}

// The ordinary movement adapter keeps its existing soft-code classification.
// Native51C2BC's shared slave arm runs only when this ordered list reaches
// the master; its true result clears a local vehicle latch and continues.
#[allow(clippy::too_many_arguments)]
fn classify_occupied_cell_with_slave_query(
    target: (u16, u16),
    layers: CanEnterLayerContext,
    mover_id: u64,
    crush_capability: bump_crush::CrushCapability,
    mover_owner: &str,
    mover_locomotor: LocomotorKind,
    mover_bypass_grid: bool,
    ignored_blockers: Option<&BTreeSet<u64>>,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    alliances: &HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
    slave_query: Option<&crate::sim::slave_deposit::SlaveDepositQuery<'_>>,
    slave_cleared_vehicle: &mut bool,
    native_unit_tail: bool,
) -> CellEntryResult {
    let _ = mover_bypass_grid;
    // --- Crush candidates ---
    // Crushability is a latch, not an early exit: gamemd sets it while walking
    // the cell list and consults it only after the walk, when nothing else
    // raised the running code above 0. An occupant the mover can crush does not
    // contribute a code; one it cannot crush raises the code like any blocker.
    let ally_gate = bump_crush::CrushAllyGate::new(mover_owner, alliances, interner);
    // Infantry's native predicate has no Unit crush latch. The missing-mover
    // allowance is for the older frame-independent planning API; live runtime
    // callers supply their mover and the raw occupation wrapper below.
    let victims = if entities
        .get(mover_id)
        .is_none_or(|mover| mover.category == EntityCategory::Unit)
    {
        bump_crush::collect_crush_victims(
            target,
            occupancy,
            layers.object_list_layer,
            crush_capability,
            entities,
            ally_gate,
        )
    } else {
        Vec::new()
    };
    let crushable: BTreeSet<u64> = victims.iter().copied().collect();

    // --- Walk the WHOLE selected cell list, worst occupant wins ---
    // gamemd keeps a running code across every occupant of the selected list
    // (`if (code < N) code = N;`) and only the hard cases return early. Taking
    // the first occupant instead loses to whichever entity happens to be at the
    // list head, which is wrong in any mixed-occupancy cell.
    let mut worst = CellEntryResult::Clear;
    let mut saw_candidate = false;
    if let Some(occ) = occupancy.get(target.0, target.1) {
        for occupant in occ.iter_layer(layers.object_list_layer) {
            if occupant.entity_id == mover_id {
                continue;
            }
            if let Some(query) = slave_query
                && query.master(mover_id) == Some(occupant.entity_id)
            {
                // The selected Cell identity is already the admission input.
                // Resolve its slot without adding a second map lookup here.
                let selected = query
                    .terrain
                    .native_fixed_cell_index(target.0 as i16, target.1 as i16)
                    .map(crate::map::cell_index::NativeCellIdentity::Real)
                    .unwrap_or(crate::map::cell_index::NativeCellIdentity::Dummy);
                if query.admits(mover_id, occupant.entity_id, selected) {
                    *slave_cleared_vehicle = true;
                    saw_candidate = true;
                    continue;
                }
            }
            if ignored_blockers.is_some_and(|ids| ids.contains(&occupant.entity_id)) {
                continue;
            }
            saw_candidate = true;
            if crushable.contains(&occupant.entity_id) {
                continue;
            }
            let candidate = classify_blocker(
                occupant.entity_id,
                entities.get(mover_id),
                mover_owner,
                entities,
                alliances,
                interner,
            );
            // **VERA-internal generalisation, gamemd equivalent UNCHECKED.** A
            // running code of 7 aborts the walk here. Native has no such
            // threshold: the Unit walk alone carries about nine distinct
            // `return 7` sites — the gate `Owner+0x1FA`, mission 0xB on the
            // archive target, the unarmed mover, blocker `+0x16B6`, three inside
            // the `What_Am_I == 0x24` arm, the allied building, the head-on
            // exit, and garrison-can't-attack — and otherwise only ever raises.
            //
            // Trigger: any occupant classified 7 while further occupants remain
            // in the list. Player effect: none observable — 7 is terminal for
            // the caller either way, and nothing later in the walk can lower it.
            // Frequency: every mixed-occupancy cell containing a hard blocker,
            // which is common, with no divergence today. Downstream risk: a
            // future classifier that produced 7 where native raises instead
            // would silently inherit the early exit and skip the rest of the
            // list.
            if candidate.yr_code() >= CellEntryResult::Impassable.yr_code() {
                return apply_overrides(CellEntryResult::Impassable, mover_locomotor);
            }
            if candidate.yr_code() > worst.yr_code() {
                worst = candidate;
            }
        }
    }

    if !saw_candidate {
        // **VERA-internal, gamemd has no equivalent.** Exhausting the object
        // list in either `Can_Enter_Cell` simply falls through to the tail with
        // the running code unchanged — a walk that found nothing returns 0.
        // This arm fabricates a hard block instead, on the reasoning that Phase
        // 1 would have answered Clear for a genuinely empty cell.
        //
        // Trigger: the occupancy grid says a cell has occupants but none of them
        // resolves to a live `EntityStore` entity the walk will look at.
        // Player effect: the mover is told the cell is impassable and the order
        // is refused or re-pathed; retail would have entered. Frequency: zero
        // while occupancy and `EntityStore` agree — it is a consistency
        // backstop, not a modelled rule. Downstream risk: it converts any future
        // occupancy desync from a silent inconsistency into a visible refused
        // order, which is arguably the safer failure but is not parity.
        if native_unit_tail || ignored_blockers.is_some() {
            return apply_overrides(CellEntryResult::Clear, mover_locomotor);
        }
        return apply_overrides(CellEntryResult::Impassable, mover_locomotor);
    }

    if worst == CellEntryResult::Clear
        && !victims.is_empty()
        && (native_unit_tail
            || bump_crush::cell_passable_after_crush(
                target,
                occupancy,
                layers.occupancy_bits_layer,
                crush_capability,
                entities,
                ally_gate,
            ))
    {
        return apply_overrides(CellEntryResult::Crushable { victims }, mover_locomotor);
    }

    apply_overrides(worst, mover_locomotor)
}

/// Phase-2 classification including the independent Unit occupation plane.
/// A bit-only reservation has no destination-list blocker to attack or scatter,
/// so it keeps native code 2 without inventing a `CellOccupant`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn classify_occupied_cell_with_layers_and_ignored_and_occupation(
    target: (u16, u16),
    layers: CanEnterLayerContext,
    mover_id: u64,
    crush_capability: bump_crush::CrushCapability,
    mover_owner: &str,
    mover_locomotor: LocomotorKind,
    mover_bypass_grid: bool,
    ignored_blockers: Option<&BTreeSet<u64>>,
    occupancy: &OccupancyGrid,
    cell_occupation: &CellOccupationGrid,
    raw_cell_occupation: &RawCellOccupationGrid,
    current_frame: u32,
    entities: &EntityStore,
    alliances: &HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
) -> CellEntryResult {
    classify_occupied_cell_with_occupation_and_slave_query(
        target,
        layers,
        mover_id,
        crush_capability,
        mover_owner,
        mover_locomotor,
        mover_bypass_grid,
        ignored_blockers,
        occupancy,
        cell_occupation,
        raw_cell_occupation,
        current_frame,
        entities,
        alliances,
        interner,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn classify_occupied_cell_with_occupation_and_slave_query(
    target: (u16, u16),
    layers: CanEnterLayerContext,
    mover_id: u64,
    crush_capability: bump_crush::CrushCapability,
    mover_owner: &str,
    mover_locomotor: LocomotorKind,
    mover_bypass_grid: bool,
    ignored_blockers: Option<&BTreeSet<u64>>,
    occupancy: &OccupancyGrid,
    cell_occupation: &CellOccupationGrid,
    raw_cell_occupation: &RawCellOccupationGrid,
    current_frame: u32,
    entities: &EntityStore,
    alliances: &HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
    slave_query: Option<&crate::sim::slave_deposit::SlaveDepositQuery<'_>>,
) -> CellEntryResult {
    let mut slave_cleared_vehicle = false;
    let native_unit_tail = entities
        .get(mover_id)
        .is_some_and(|mover| mover.category == EntityCategory::Unit);
    let result = classify_occupied_cell_with_slave_query(
        target,
        layers,
        mover_id,
        crush_capability,
        mover_owner,
        mover_locomotor,
        mover_bypass_grid,
        ignored_blockers,
        occupancy,
        entities,
        alliances,
        interner,
        slave_query,
        &mut slave_cleared_vehicle,
        native_unit_tail,
    );
    if native_unit_tail {
        // Unit73FC24 preserves every nonzero accumulated code. With a crush
        // latch,73FCF6 only consults vehicle bit5, then GetUnit(false) on the
        // GROUND list. There is no self/ignored-ID filter in that lookup and
        // no owner attribution in the raw byte. Object walk and bit plane are
        // intentionally independent. See tools/spatial_oracle/cell_entry_crush_tail.
        if result.yr_code() != 0 || slave_cleared_vehicle {
            return result;
        }
        let raw = match layers.occupancy_bits_layer {
            MovementLayer::Ground => raw_cell_occupation.ground_bits(target.0, target.1),
            MovementLayer::Bridge => raw_cell_occupation.deck_bits(target.0, target.1),
            _ => 0,
        };
        if raw & 0x20 == 0 {
            return result;
        }
        if matches!(result, CellEntryResult::Crushable { .. })
            && occupancy.get(target.0, target.1).is_some_and(|cell| {
                cell.iter_layer(MovementLayer::Ground)
                    .filter_map(|occupant| entities.get(occupant.entity_id))
                    .find(|entity| entity.category == EntityCategory::Unit)
                    .is_some_and(|unit| {
                        unit_tail_is_crushable_by(
                            unit,
                            crush_capability,
                            houses::is_allied_with(
                                alliances,
                                mover_owner,
                                interner.resolve(unit.owner()),
                            ),
                            current_frame,
                        )
                    })
            })
        {
            return result;
        }
        return apply_overrides(CellEntryResult::TemporaryOccupation, mover_locomotor);
    }
    if !slave_cleared_vehicle
        && matches!(result, CellEntryResult::Clear | CellEntryResult::Impassable)
        && cell_occupation.occupied_by_other(
            target.0,
            target.1,
            layers.occupancy_bits_layer,
            mover_id,
        )
    {
        apply_overrides(CellEntryResult::TemporaryOccupation, mover_locomotor)
    } else {
        result
    }
}

/// The Unit-target subset of Object::IsCrushableBy5F6CD0, reached by the
/// post-latch GetUnit(false) exception at73FD17. The caller has already supplied
/// a latch; this leaf does not retest Crusher/ability or apply kills. Both native
/// arms test mover-house alliance and the target's +160 invulnerability slot.
/// A rejected Omni arm falls through to ordinary Crushable, which does not
/// read OmniCrushResistant or the mover's regular-crusher flag.
fn unit_tail_is_crushable_by(
    unit: &GameEntity,
    capability: bump_crush::CrushCapability,
    mover_considers_target_allied: bool,
    current_frame: u32,
) -> bool {
    debug_assert_eq!(unit.category, EntityCategory::Unit);
    let target = bump_crush::CrushTarget::from_entity(unit, current_frame);
    // Live Unit entities carry Techno abstract flag1. Deploy crush immunity
    // is written only by Infantry deploy; it is always clear on this subset.
    ((capability.omni_crusher && !target.omni_crush_resistant)
        || (target.crushable && !target.deploy_crush_immune))
        && !mover_considers_target_allied
        && !target.iron_curtained
}

#[cfg(test)]
#[path = "cell_entry_crush_tail_tests.rs"]
mod crush_tail_tests;

/// First blocker entity in the selected layer's cell list.
///
/// The production classifier no longer stops at this entity — it walks the whole
/// list and keeps the worst code, matching the native predicate. This helper is
/// retained only to pin the list ordering and the ignore/self-skip rules.
///
/// Live building exceptions are supplied through `ignored_blockers`; bypassing
/// the static path grid does not suppress structure occupants by itself.
#[cfg(test)]
fn find_primary_blocker(
    target: (u16, u16),
    layer: MovementLayer,
    mover_id: u64,
    _mover_bypass_grid: bool,
    ignored_blockers: Option<&BTreeSet<u64>>,
    occupancy: &OccupancyGrid,
    _entities: &EntityStore,
) -> Option<u64> {
    let occ = occupancy.get(target.0, target.1)?;
    for occupant in occ.iter_layer(layer) {
        if occupant.entity_id == mover_id {
            continue;
        }
        if ignored_blockers.is_some_and(|ids| ids.contains(&occupant.entity_id)) {
            continue;
        }
        return Some(occupant.entity_id);
    }
    None
}

/// The head-on exit of `UnitClass::Can_Enter_Cell`, `0x0073F8D4..FA26`.
///
/// Taken for an allied occupant that is moving. Octants are the native
/// `((facing16 >> 12) + 1 >> 1) & 7`; the byte facing is the high byte of that
/// word, so `((facing8 >> 4) + 1 >> 1) & 7` is the same value. The occupant's
/// facing is reversed by adding `0x7FFF` in the 16-bit word (`0x0073F914`)
/// before its octant is taken. When the mover's octant equals that reversed
/// octant — the two are facing each other — the 3-D lepton distance
/// `Sqrt_Approx(dx² + dz² + dy²)` (`0x0073F9DA..FA03`) goes through `ftol`
/// and anything above `0x1FF` (`0x0073FA10 CMP EAX,0x1FF / JG`) escapes;
/// otherwise the direction word from mover to occupant
/// (`Math::atan2(mover.y − occ.y, occ.x − mover.x)` centred and scaled at
/// `0x0073F97B..F98A`) must land in the mover's own octant for the exit to
/// fire (`0x0073FA24 CMP ECX,EBP / JZ 0x0073FCD0`, return 7).
fn head_on_with_moving_ally(mover: &GameEntity, occupant: &GameEntity) -> bool {
    head_on_exit(
        mover.facing,
        entity_world_leptons(mover),
        occupant.facing,
        entity_world_leptons(occupant),
    )
}

/// An object's native coordinate triple in leptons: cell origin plus sub-cell
/// offset, and the exact Z when the mover retains one, else the level height.
pub(crate) fn entity_world_leptons(entity: &GameEntity) -> [i32; 3] {
    use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
    let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
    let z = entity
        .position
        .exact_z_leptons
        .unwrap_or_else(|| i32::from(entity.position.z as i8) * GROUND_LEVEL_HEIGHT_LEPTONS);
    [x, y, z]
}

/// The head-on exit over raw inputs; see [`head_on_with_moving_ally`] for the
/// native trace. Shared with the Drive selection lane, which evaluates it
/// against its owner snapshot of moving allies.
pub(crate) fn head_on_exit(
    mover_facing: u8,
    mover_world: [i32; 3],
    occupant_facing: u8,
    occupant_world: [i32; 3],
) -> bool {
    use crate::util::direction_tables::facing16_from_delta;
    use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

    let octant16 = |word: u32| ((word >> 12).wrapping_add(1) >> 1) & 7;
    let mover_octant = octant16(u32::from(mover_facing) << 8);
    let occupant_reversed = (u32::from(occupant_facing) << 8).wrapping_add(0x7FFF) & 0xFFFF;
    if mover_octant != octant16(occupant_reversed) {
        return false;
    }

    let [mx, my, mz] = mover_world;
    let [ox, oy, oz] = occupant_world;
    let (dx, dy, dz) = (
        mx.wrapping_sub(ox),
        my.wrapping_sub(oy),
        mz.wrapping_sub(oz),
    );
    let square = |value: i32| {
        let loaded = X87Chop53::load_i32(value);
        X87Chop53::mul(loaded, loaded)
    };
    let sum = X87Chop53::add(X87Chop53::add(square(dx), square(dz)), square(dy));
    let distance = sqrt_approx_f32(sum)
        .ok()
        .and_then(|bits| X87Chop53::load_f32(bits).ok())
        .and_then(|value| X87Chop53::ftol_i64(value).ok());
    let Some(distance) = distance else {
        return false;
    };
    if distance > 0x1FF {
        return false;
    }
    let direction = facing16_from_delta(ox.wrapping_sub(mx), oy.wrapping_sub(my));
    octant16(u32::from(direction)) == mover_octant
}

/// Classify a single blocker as enemy, friendly-moving, or friendly-stationary.
fn classify_blocker(
    blocker_id: u64,
    mover: Option<&GameEntity>,
    mover_owner: &str,
    entities: &EntityStore,
    alliances: &HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
) -> CellEntryResult {
    let Some(blocker) = entities.get(blocker_id) else {
        return CellEntryResult::Impassable;
    };
    let is_friendly =
        houses::are_houses_friendly(alliances, mover_owner, interner.resolve(blocker.owner()));
    if !is_friendly {
        return CellEntryResult::OccupiedEnemy { blocker_id };
    }
    if blocker
        .building_gate
        .is_some_and(|gate| !gate.can_garrison_passable())
    {
        return CellEntryResult::ScatterRequired {
            blocker_id: Some(blocker_id),
        };
    }
    // Friendly and not moving. A BuildingClass occupant is a HARD block here,
    // not code 6: the ally/not-moving arm of the native predicate tests the
    // blocker's class first and returns impassable for a building. Code 6 would
    // otherwise send the mover down the scatter-and-wait path, which asks a
    // *structure* to move out of the way and hands out a grace period the
    // native code-7 path never gives. The runtime consumer treats a Structure
    // blocker as a hard block to match; do not add a second gate on top of it.
    if blocker.category == EntityCategory::Structure {
        return CellEntryResult::Impassable;
    }
    // Friendly: moving -> temporary block, stationary -> code 6.
    //
    // The moving arm of `UnitClass::Can_Enter_Cell` (`0x0073FA2C..FA7C`) does
    // not raise unconditionally. It reads the occupant's `Foot+0x6B6` at
    // `0x0073FA30`; when that is zero (the occupant is in transit — Drive
    // clears it at `0x004B161A` and re-sets it at `0x004B1FEF`, Ship at
    // `0x006A0CDA`/`0x006A1632`, Hover at `0x005147D5`/`0x0051451E`) or the
    // occupant is an InfantryClass (`vtable+0x2C == 0xF`, `0x0073FA41`), it asks
    // the occupant's locomotor slot `+0xA4` (`0x0073FA63`); a false answer
    // skips the occupant entirely (`0x0073FA6B JZ 0x0073FA7C`, which reloads
    // the running code unchanged) and only a true one reaches the running-max raise to 2 at
    // `0x0073FA6D..FA74`. Drive/Ship answer `Can_Use_Track`; every other
    // class answers false. The cell then blocks through the occupation mask
    // arm, not the object list. "Moving" itself stays VERA's
    // `movement_target` test; native's NavCom/rotating/`Is_Moving` triple
    // (`0x0073F865..F8C0`) is a recorded gap, unchanged here. The head-on exit
    // that precedes this arm is `head_on_with_moving_ally` above.
    if blocker.movement_target.is_some() {
        // The head-on exit precedes the locomotor question and exists only in
        // the Unit implementation (`0x0073F8D4`); Infantry `+0x1AC` has none.
        if mover.is_some_and(|mover| {
            mover.category == EntityCategory::Unit && head_on_with_moving_ally(mover, blocker)
        }) {
            return CellEntryResult::Impassable;
        }
        let in_transit = !blocker.foot_occupation_enabled;
        let infantry = blocker.category == EntityCategory::Infantry;
        if (in_transit || infantry)
            && !crate::sim::movement::drive_track::occupant_slot_a4_answers_true(blocker)
        {
            return CellEntryResult::Clear;
        }
        CellEntryResult::TemporaryBlock { blocker_id }
    } else {
        CellEntryResult::FriendlyStationary { blocker_id }
    }
}

/// Apply locomotor-specific overrides to a cell entry result.
///
/// **VERA-internal, gamemd equivalent UNCHECKED.** JumpJet: every code except
/// Impassable is lowered to Clear. The previous citation here — "deep_113 line
/// 861" — is not an address or a named research doc and does not meet the
/// provenance form; it is dropped rather than dressed up.
///
/// The nearest native mechanism runs the other way round.
/// `FootClass::LocomotorPassabilityCheck` @ `0x004D9C10` dispatches the mover's
/// locomotor vtable `+0x1C` and **seeds** the running code before the occupant
/// walk, is Unit-only, is gated on a caller flag byte, and can only be raised
/// afterwards — nothing in either `Can_Enter_Cell` lowers an accumulated code at
/// the end. What `JumpjetLocomotionClass+0x1C` returns is UNCHECKED. Trigger:
/// any jumpjet mover meeting an occupied or soft-blocked cell. Player effect:
/// jumpjets ignore ground traffic, which is the retail feel; whether they ignore
/// it by this route is unverified. Frequency: every Rocketeer and Floating Disc
/// order. Downstream risk: replacing this with the native seed changes where the
/// locomotor hook sits relative to the walk, so it is a restructure rather than
/// a swap.
fn apply_overrides(result: CellEntryResult, locomotor: LocomotorKind) -> CellEntryResult {
    if locomotor == LocomotorKind::Jumpjet && !matches!(result, CellEntryResult::Impassable) {
        return CellEntryResult::Clear;
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::occupancy::CellListInsertion;

    /// `0x0073F8D4..FA26` over raw inputs: facing each other inside `0x1FF`
    /// leptons with the occupant in the mover's octant fires; every one of the
    /// three gates alone releases it.
    #[test]
    fn head_on_exit_requires_opposed_facings_range_and_bearing() {
        // Mover faces east (64 → octant 2), occupant faces west (192 → reversed
        // octant 2), occupant 256 leptons due east, same height.
        let mover = [10 * 256 + 128, 10 * 256 + 128, 0];
        let east_256 = [11 * 256 + 128, 10 * 256 + 128, 0];
        assert!(head_on_exit(64, mover, 192, east_256));
        // Distance gate: 511 fires, 512 does not (`CMP EAX,0x1FF / JG`).
        assert!(head_on_exit(64, mover, 192, [mover[0] + 511, mover[1], 0]));
        assert!(!head_on_exit(64, mover, 192, [mover[0] + 512, mover[1], 0]));
        // Height enters the 3-D distance.
        assert!(!head_on_exit(
            64,
            mover,
            192,
            [mover[0] + 500, mover[1], 120]
        ));
        // Occupant facing the same way (a column) is not head-on.
        assert!(!head_on_exit(64, mover, 64, east_256));
        // Occupant behind the mover, still facing it: bearing is west, not east.
        assert!(!head_on_exit(
            64,
            mover,
            192,
            [9 * 256 + 128, 10 * 256 + 128, 0]
        ));
        // Occupant off to the side (south-east) leaves the mover's octant.
        assert!(!head_on_exit(
            64,
            mover,
            192,
            [11 * 256 + 128, 11 * 256 + 128, 0]
        ));
        // Octant rounding: facing 48..79 all read as octant 2.
        assert!(head_on_exit(48, mover, 208, east_256));
        assert!(head_on_exit(79, mover, 177, east_256));
        assert!(!head_on_exit(80, mover, 192, east_256));
    }

    fn crushable_wall_grid() -> ResolvedTerrainGrid {
        let mut cells = Vec::with_capacity(9);
        for ry in 0..3u16 {
            for rx in 0..3u16 {
                let mut cell = ResolvedTerrainCell::clear_for_test(rx, ry);
                if (rx, ry) == (1, 1) {
                    // `[GASAND]`: Wall=yes + Crushable=yes -> RecalcZoneType class 1.
                    cell.overlay_zone_type = Some(zone_class::CRUSHABLE);
                    cell.zone_type = zone_class::CRUSHABLE;
                }
                cells.push(cell);
            }
        }
        ResolvedTerrainGrid::from_cells(3, 3, cells)
    }

    fn crushable_wall_entry(
        terrain: &ResolvedTerrainGrid,
        grid: &PathGrid,
        movement_zone: MovementZone,
        is_infantry: bool,
        mover_is_crusher: bool,
    ) -> CanEnterCellResult {
        evaluate_can_enter_cell(CanEnterCellContext {
            wall: None,
            target: (1, 1),
            terrain_layer: MovementLayer::Ground,
            movement_zone: Some(movement_zone),
            speed_type: None,
            path_grid: Some(grid),
            resolved_terrain: Some(terrain),
            terrain_costs: None,
            bypass_grid: false,
            mode: TerrainEntryMode::RuntimeTransition,
            is_infantry,
            mover_is_crusher,
        })
    }

    /// `UnitClass::Can_Enter_Cell 0x0073F42E..F46C`: a `Crushable=` wall admits a
    /// `Crusher=` type or `MovementZone=CrusherAll`; `InfantryClass 0x0051BF90`
    /// has no crusher route. Every other mover is refused here where native's
    /// weapon route answers 4/5/7 (recorded gap). Native established from the
    /// bodies; this is a Rust regression check over the zone-class-1 fixture.
    #[test]
    fn crushable_wall_admits_only_crushers_and_crusher_all() {
        let terrain = crushable_wall_grid();
        let grid = PathGrid::from_resolved_terrain(&terrain);
        assert!(
            grid.is_walkable(1, 1),
            "class 1 does not block the path grid"
        );
        let cases = [
            (
                MovementZone::Normal,
                false,
                false,
                CanEnterCellResult::HardBlocked,
            ),
            (MovementZone::Normal, false, true, CanEnterCellResult::Clear),
            (
                MovementZone::Crusher,
                false,
                true,
                CanEnterCellResult::Clear,
            ),
            (
                MovementZone::CrusherAll,
                false,
                false,
                CanEnterCellResult::Clear,
            ),
            (
                MovementZone::Infantry,
                true,
                false,
                CanEnterCellResult::HardBlocked,
            ),
            // An infantryman never takes the crusher route, whatever its type says.
            (
                MovementZone::Infantry,
                true,
                true,
                CanEnterCellResult::HardBlocked,
            ),
            // Nor does an infantryman take the CrusherAll route.
            (
                MovementZone::CrusherAll,
                true,
                false,
                CanEnterCellResult::HardBlocked,
            ),
            (
                MovementZone::Destroyer,
                false,
                false,
                CanEnterCellResult::HardBlocked,
            ),
        ];
        for (zone, infantry, crusher, expected) in cases {
            assert_eq!(
                crushable_wall_entry(&terrain, &grid, zone, infantry, crusher),
                expected,
                "zone {zone:?} infantry {infantry} crusher {crusher}"
            );
        }
        // The clear neighbour is untouched by the arm.
        let clear = evaluate_can_enter_cell(CanEnterCellContext {
            wall: None,
            target: (0, 0),
            terrain_layer: MovementLayer::Ground,
            movement_zone: Some(MovementZone::Normal),
            speed_type: None,
            path_grid: Some(&grid),
            resolved_terrain: Some(&terrain),
            terrain_costs: None,
            bypass_grid: false,
            mode: TerrainEntryMode::RuntimeTransition,
            is_infantry: false,
            mover_is_crusher: false,
        });
        assert_eq!(clear, CanEnterCellResult::Clear);
    }

    /// The search consumer of the arm: over a sandbag line with one gap, a
    /// non-crusher's route takes the gap and a crusher's route goes straight
    /// through the wall cell (I4's drive-over crush then flattens it).
    #[test]
    fn astar_routes_non_crushers_around_a_sandbag_line_and_crushers_through_it() {
        // Seven by five: sandbags down column 3 with the only gap at (3, 0),
        // so the crusher's straight run along row 4 (6 steps) is strictly
        // cheaper than the non-crusher's detour through the gap (8 steps) and
        // the outcome rests on the arm, not on the direction tiebreak.
        let (w, h) = (7u16, 5u16);
        let mut cells = Vec::with_capacity((w * h) as usize);
        for ry in 0..h {
            for rx in 0..w {
                let mut cell = ResolvedTerrainCell::clear_for_test(rx, ry);
                if rx == 3 && ry != 0 {
                    cell.overlay_zone_type = Some(zone_class::CRUSHABLE);
                    cell.zone_type = zone_class::CRUSHABLE;
                }
                cells.push(cell);
            }
        }
        let terrain = ResolvedTerrainGrid::from_cells(w, h, cells);
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let route = |mover_is_crusher: bool| {
            crate::sim::pathfinding::find_path_with_costs(
                &grid,
                (0, 4),
                (6, 4),
                None,
                None,
                Some(MovementZone::Normal),
                Some(&terrain),
                None,
                0,
                mover_is_crusher,
                false,
            )
            .expect("a route exists on both sides of the arm")
        };
        let detour = route(false);
        assert!(
            detour.contains(&(3, 0)),
            "non-crusher takes the gap: {detour:?}"
        );
        assert!(
            !detour.iter().any(|&(x, y)| x == 3 && y != 0),
            "non-crusher never enters a sandbag cell: {detour:?}"
        );
        let straight = route(true);
        assert!(
            straight.iter().any(|&(x, y)| x == 3 && y != 0),
            "crusher drives through the line: {straight:?}"
        );
        assert!(
            straight.len() < detour.len(),
            "the arm, not the tiebreak, decides: {straight:?} vs {detour:?}"
        );
    }

    fn moving_ally(id: u64, rx: u16, ry: u16, facing: u8, in_transit: bool) -> GameEntity {
        let mut ally = GameEntity::test_default(id, "MTNK", "Americans", rx, ry);
        ally.category = EntityCategory::Unit;
        ally.facing = facing;
        ally.foot_occupation_enabled = !in_transit;
        ally.movement_target = Some(crate::sim::components::MovementTarget {
            path: vec![(rx, ry), (rx.wrapping_sub(1), ry)],
            path_layers: vec![MovementLayer::Ground, MovementLayer::Ground],
            next_index: 1,
            ..Default::default()
        });
        ally
    }

    /// The hover consumer of the arm: `HoverLocomotionClass` inherits the base
    /// `+0xA4` (`0x004B6640`, always false), so a hover ally is skipped exactly
    /// while its Foot occupation enable is clear (in transit, `0x005147D5`) and
    /// raises code 2 once the arrival arm (`0x0051451E`) or a refused step has
    /// set it again.
    #[test]
    fn classify_blocker_skips_a_hover_ally_only_while_it_is_in_transit() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::LocomotorState;
        let mut entities = EntityStore::new();
        for (id, in_transit) in [(100u64, true), (101, false)] {
            let mut hover = moving_ally(id, 11 + (id - 100) as u16, 10, 192, in_transit);
            hover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Hover));
            entities.insert(hover);
        }
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();
        assert_eq!(
            classify_blocker(100, None, "Americans", &entities, &alliances, &interner),
            CellEntryResult::Clear,
            "hover in transit: skipped"
        );
        assert_eq!(
            classify_blocker(101, None, "Americans", &entities, &alliances, &interner),
            CellEntryResult::TemporaryBlock { blocker_id: 101 },
            "hover with its enable set: code 2"
        );
    }

    /// `0x0073FA2C..FA7C`: an in-transit ally whose locomotor answers false on
    /// slot `+0xA4` is skipped (no code); a standing moving ally still raises
    /// code 2; an infantryman takes the locomotor question whatever its transit
    /// state and Walk always answers false.
    #[test]
    fn classify_blocker_skips_in_transit_allies_that_cannot_use_a_track() {
        let mut entities = EntityStore::new();
        entities.insert(moving_ally(100, 11, 10, 192, true));
        entities.insert(moving_ally(101, 12, 10, 192, false));
        let mut infantry = moving_ally(102, 13, 10, 192, false);
        infantry.category = EntityCategory::Infantry;
        entities.insert(infantry);
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        assert_eq!(
            classify_blocker(100, None, "Americans", &entities, &alliances, &interner),
            CellEntryResult::Clear,
            "in transit, no retained track: skipped"
        );
        assert_eq!(
            classify_blocker(101, None, "Americans", &entities, &alliances, &interner),
            CellEntryResult::TemporaryBlock { blocker_id: 101 },
            "standing moving ally still raises 2"
        );
        assert_eq!(
            classify_blocker(102, None, "Americans", &entities, &alliances, &interner),
            CellEntryResult::Clear,
            "moving infantry: Walk slot answers false"
        );
    }

    /// The head-on exit runs before the locomotor question and only for a
    /// Unit mover: the same in-transit ally that is skipped above blocks a tank
    /// facing it head-on inside two cells, and does not block an infantryman.
    #[test]
    fn classify_blocker_head_on_exit_precedes_the_transit_skip_for_unit_movers() {
        let mut entities = EntityStore::new();
        entities.insert(moving_ally(100, 11, 10, 192, true));
        let mut tank = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        tank.category = EntityCategory::Unit;
        tank.facing = 64;
        let mut soldier = GameEntity::test_default(2, "E1", "Americans", 10, 10);
        soldier.category = EntityCategory::Infantry;
        soldier.facing = 64;
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        assert_eq!(
            classify_blocker(
                100,
                Some(&tank),
                "Americans",
                &entities,
                &alliances,
                &interner
            ),
            CellEntryResult::Impassable
        );
        assert_eq!(
            classify_blocker(
                100,
                Some(&soldier),
                "Americans",
                &entities,
                &alliances,
                &interner
            ),
            CellEntryResult::Clear
        );
        tank.facing = 192;
        assert_eq!(
            classify_blocker(
                100,
                Some(&tank),
                "Americans",
                &entities,
                &alliances,
                &interner
            ),
            CellEntryResult::Clear,
            "a tank facing away is not head-on"
        );
    }

    fn empty_occ() -> OccupancyGrid {
        OccupancyGrid::new()
    }

    #[test]
    fn test_clear_empty_cell() {
        let result = check_terrain(
            (5, 5),
            MovementLayer::Ground,
            EntityCategory::Unit,
            None,
            None,
            &empty_occ(),
        );
        assert_eq!(result, TerrainCheckResult::Clear);
    }

    #[test]
    fn test_impassable_blocked_grid() {
        use crate::sim::pathfinding::PathGrid;
        let mut grid = PathGrid::new(10, 10);
        grid.set_blocked(5, 5, true);
        let result = check_terrain(
            (5, 5),
            MovementLayer::Ground,
            EntityCategory::Unit,
            Some(&grid),
            None,
            &empty_occ(),
        );
        assert_eq!(result, TerrainCheckResult::Impassable);
    }

    #[test]
    fn test_vehicle_occupied_needs_check() {
        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            42,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let result = check_terrain(
            (5, 5),
            MovementLayer::Ground,
            EntityCategory::Unit,
            None,
            None,
            &occ,
        );
        assert_eq!(result, TerrainCheckResult::NeedsBlockerCheck);
    }

    #[test]
    fn test_infantry_subcell_available() {
        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        let result = check_terrain(
            (5, 5),
            MovementLayer::Ground,
            EntityCategory::Infantry,
            None,
            None,
            &occ,
        );
        assert_eq!(result, TerrainCheckResult::Clear);
    }

    #[test]
    fn test_infantry_cell_full() {
        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        occ.add(
            5,
            5,
            11,
            MovementLayer::Ground,
            Some(3),
            CellListInsertion::PrependNonBuilding,
        );
        occ.add(
            5,
            5,
            12,
            MovementLayer::Ground,
            Some(4),
            CellListInsertion::PrependNonBuilding,
        );
        let result = check_terrain(
            (5, 5),
            MovementLayer::Ground,
            EntityCategory::Infantry,
            None,
            None,
            &occ,
        );
        assert_eq!(result, TerrainCheckResult::NeedsBlockerCheck);
    }

    #[test]
    fn test_jumpjet_override_clears_non_impassable() {
        let result = apply_overrides(
            CellEntryResult::OccupiedEnemy { blocker_id: 1 },
            LocomotorKind::Jumpjet,
        );
        assert_eq!(result, CellEntryResult::Clear);
    }

    #[test]
    fn test_jumpjet_keeps_impassable() {
        let result = apply_overrides(CellEntryResult::Impassable, LocomotorKind::Jumpjet);
        assert_eq!(result, CellEntryResult::Impassable);
    }

    #[test]
    fn test_non_jumpjet_no_override() {
        let result = apply_overrides(
            CellEntryResult::OccupiedEnemy { blocker_id: 1 },
            LocomotorKind::Drive,
        );
        assert_eq!(result, CellEntryResult::OccupiedEnemy { blocker_id: 1 });
    }

    #[test]
    fn cell_entry_result_yr_codes_match_verified_table() {
        assert_eq!(CellEntryResult::Clear.yr_code(), 0);
        assert_eq!(CellEntryResult::Crushable { victims: vec![1] }.yr_code(), 0);
        assert_eq!(
            CellEntryResult::TemporaryBlock { blocker_id: 1 }.yr_code(),
            2
        );
        assert_eq!(
            CellEntryResult::ScatterRequired {
                blocker_id: Some(1),
            }
            .yr_code(),
            3
        );
        assert_eq!(CellEntryResult::FriendlyWall.yr_code(), 4);
        assert_eq!(
            CellEntryResult::OccupiedEnemy { blocker_id: 1 }.yr_code(),
            5
        );
        assert_eq!(
            CellEntryResult::FriendlyStationary { blocker_id: 1 }.yr_code(),
            6
        );
        assert_eq!(CellEntryResult::Impassable.yr_code(), 7);
    }

    #[test]
    fn friendly_closed_or_opening_gate_returns_code_3_not_code_6() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::{BuildingGatePhase, BuildingGateRuntime, GameEntity};

        let mut entities = EntityStore::new();
        let mut gate = GameEntity::test_default(100, "GAGATE_A", "Americans", 5, 5);
        gate.category = EntityCategory::Structure;
        gate.building_gate = Some(BuildingGateRuntime::default());
        entities.insert(gate);
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        let result = classify_blocker(100, None, "Americans", &entities, &alliances, &interner);
        assert_eq!(
            result,
            CellEntryResult::ScatterRequired {
                blocker_id: Some(100)
            }
        );
        assert_eq!(result.yr_code(), 3);

        entities.get_mut(100).unwrap().building_gate = Some(BuildingGateRuntime {
            mission_18_active: true,
            phase: BuildingGatePhase::Opening,
            ..Default::default()
        });
        let result = classify_blocker(100, None, "Americans", &entities, &alliances, &interner);
        assert_eq!(
            result,
            CellEntryResult::ScatterRequired {
                blocker_id: Some(100)
            }
        );
        assert_eq!(result.yr_code(), 3);
    }

    #[test]
    fn allied_stationary_building_is_hard_blocked_not_code_6() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        let mut refinery = GameEntity::test_default(200, "GAREFN", "Americans", 5, 5);
        refinery.category = EntityCategory::Structure;
        entities.insert(refinery);
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        let result = classify_blocker(200, None, "Americans", &entities, &alliances, &interner);
        assert_eq!(result, CellEntryResult::Impassable);
        assert_eq!(result.yr_code(), 7);
    }

    #[test]
    fn allied_stationary_unit_still_returns_code_6() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        let mut tank = GameEntity::test_default(201, "GTNK", "Americans", 5, 5);
        tank.category = EntityCategory::Unit;
        entities.insert(tank);
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        let result = classify_blocker(201, None, "Americans", &entities, &alliances, &interner);
        assert_eq!(
            result,
            CellEntryResult::FriendlyStationary { blocker_id: 201 }
        );
        assert_eq!(result.yr_code(), 6);
    }

    /// The player-visible half of A7 D1: a crusher classifies its own infantry
    /// as a friendly blocker, not as something to crush.
    ///
    /// `UnitClass::Can_Enter_Cell` asks the alliance question at
    /// `0x0073F8C0..0x0073F8CE` - `MOV ECX,[EBX+0x21c]` (the occupant's house),
    /// `PUSH ESI`, `CALL 0x004F9A90`, `TEST AL,AL`, `JZ 0x0073FAFF` - *before*
    /// the crush latch at `0x0073FB2A`, and only the not-ally fall-through
    /// reaches it. A stationary ally then answers **6** at `0x0073FAF4`.
    ///
    /// The unit tests below pin `collect_crush_victims` directly; this one pins
    /// the answer a mover actually acts on, which is what stalled: admission
    /// said `Crushable`, the kill site refused, and the tank sat on its own GI.
    #[test]
    fn a_crusher_classifies_its_own_infantry_as_friendly_not_crushable() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::new();
        let mut gi = GameEntity::test_default(7, "E1", "Americans", 5, 5);
        gi.category = EntityCategory::Infantry;
        gi.crushable = true;
        gi.sub_cell = Some(2);
        entities.insert(gi);

        let occupancy = {
            let mut grid = OccupancyGrid::new();
            grid.add(
                5,
                5,
                7,
                MovementLayer::Ground,
                Some(2),
                CellListInsertion::PrependNonBuilding,
            );
            grid
        };
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        let own = classify_occupied_cell(
            (5, 5),
            MovementLayer::Ground,
            1,
            bump_crush::CrushCapability::new(true, false),
            "Americans",
            LocomotorKind::Drive,
            false,
            &occupancy,
            &entities,
            &alliances,
            &interner,
        );
        assert_eq!(
            own,
            CellEntryResult::FriendlyStationary { blocker_id: 7 },
            "a crusher must read its own infantry as a friendly blocker"
        );
        assert_eq!(own.yr_code(), 6, "which is native's code 6");

        // The same cell, an enemy crusher: still crushable, so the refusal is
        // about alliance and not about something incidental to the fixture.
        let enemy = classify_occupied_cell(
            (5, 5),
            MovementLayer::Ground,
            1,
            bump_crush::CrushCapability::new(true, false),
            "Soviets",
            LocomotorKind::Drive,
            false,
            &occupancy,
            &entities,
            &alliances,
            &interner,
        );
        assert!(
            matches!(enemy, CellEntryResult::Crushable { .. }),
            "an enemy crusher still crushes it, got {enemy:?}"
        );
    }

    #[test]
    fn whole_cell_list_is_classified_and_the_worst_occupant_wins() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        // Two occupants: a friendly stationary unit at the list head (code 6)
        // and an enemy behind it. gamemd walks the whole list and raises the
        // running code, so the enemy must not be lost to list order — but code 6
        // outranks code 5, so the friendly still wins here. The reverse ordering
        // must give the same answer.
        let mut entities = EntityStore::new();
        let mut ally = GameEntity::test_default(300, "GTNK", "Americans", 5, 5);
        ally.category = EntityCategory::Unit;
        entities.insert(ally);
        let mut foe = GameEntity::test_default(301, "HTNK", "Soviets", 5, 5);
        foe.category = EntityCategory::Unit;
        entities.insert(foe);
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        for order in [[300u64, 301u64], [301, 300]] {
            let mut occ = OccupancyGrid::new();
            for id in order {
                occ.add(
                    5,
                    5,
                    id,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
            let result = classify_occupied_cell(
                (5, 5),
                MovementLayer::Ground,
                999,
                bump_crush::CrushCapability::new(false, false),
                "Americans",
                LocomotorKind::Drive,
                false,
                &occ,
                &entities,
                &alliances,
                &interner,
            );
            assert_eq!(
                result,
                CellEntryResult::FriendlyStationary { blocker_id: 300 },
                "order={order:?}"
            );
        }
    }

    #[test]
    fn a_structure_behind_a_unit_still_hard_blocks_the_cell() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        // Taking only the list head would report code 6 and send the mover into
        // the scatter/grace path; the whole-list walk finds the structure.
        let mut entities = EntityStore::new();
        let mut ally = GameEntity::test_default(310, "GTNK", "Americans", 5, 5);
        ally.category = EntityCategory::Unit;
        entities.insert(ally);
        let mut refinery = GameEntity::test_default(311, "GAREFN", "Americans", 5, 5);
        refinery.category = EntityCategory::Structure;
        entities.insert(refinery);
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            310,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        occ.add(
            5,
            5,
            311,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );

        let result = classify_occupied_cell(
            (5, 5),
            MovementLayer::Ground,
            999,
            bump_crush::CrushCapability::new(false, false),
            "Americans",
            LocomotorKind::Drive,
            false,
            &occ,
            &entities,
            &alliances,
            &interner,
        );
        assert_eq!(result, CellEntryResult::Impassable);
    }

    #[test]
    fn enemy_closed_gate_keeps_enemy_result_code() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::{BuildingGateRuntime, GameEntity};

        let mut entities = EntityStore::new();
        let mut gate = GameEntity::test_default(100, "GAGATE_A", "Soviets", 5, 5);
        gate.category = EntityCategory::Structure;
        gate.building_gate = Some(BuildingGateRuntime::default());
        entities.insert(gate);
        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();

        let result = classify_blocker(100, None, "Americans", &entities, &alliances, &interner);
        assert_eq!(result, CellEntryResult::OccupiedEnemy { blocker_id: 100 });
        assert_eq!(result.yr_code(), 5);
    }

    fn row_entry_input(
        mover_category: EntityCategory,
        branch: VehicleBuildingEntryBranch,
        candidate_x: u16,
    ) -> LiveVehicleBuildingEntry {
        LiveVehicleBuildingEntry {
            mover_category,
            branch,
            checked_building_id: 100,
            candidate_building_id: Some(100),
            candidate_x,
            building_origin_x: 10,
            number_impassable_rows: 1,
            is_unit_repair: false,
            is_bunker: false,
            bunker_occupied: false,
        }
    }

    #[test]
    fn infantry_does_not_use_vehicle_row_contact_skip() {
        let input = row_entry_input(
            EntityCategory::Infantry,
            VehicleBuildingEntryBranch::RadioContact {
                mover_has_contact: true,
            },
            11,
        );

        assert_eq!(
            decide_live_vehicle_building_entry(input),
            BuildingOccupantEntryDecision::KeepBlocker
        );
    }

    #[test]
    fn contacted_vehicle_row_skip_opens_east_columns_but_keeps_west() {
        let contacted = VehicleBuildingEntryBranch::RadioContact {
            mover_has_contact: true,
        };
        assert_eq!(
            decide_live_vehicle_building_entry(row_entry_input(
                EntityCategory::Unit,
                contacted,
                10,
            )),
            BuildingOccupantEntryDecision::KeepBlocker
        );
        assert_eq!(
            decide_live_vehicle_building_entry(row_entry_input(
                EntityCategory::Unit,
                contacted,
                11,
            )),
            BuildingOccupantEntryDecision::SkipBlocker
        );
        assert_eq!(
            decide_live_vehicle_building_entry(row_entry_input(
                EntityCategory::Unit,
                VehicleBuildingEntryBranch::RadioContact {
                    mover_has_contact: false,
                },
                11,
            )),
            BuildingOccupantEntryDecision::KeepBlocker
        );
    }

    #[test]
    fn empty_vs_occupied_bunker_uses_explicit_runtime_occupant_arg() {
        let mut empty = row_entry_input(
            EntityCategory::Unit,
            VehicleBuildingEntryBranch::UnitRepairOrBunker,
            10,
        );
        empty.number_impassable_rows = 0;
        empty.is_bunker = true;

        assert_eq!(
            decide_live_vehicle_building_entry(empty),
            BuildingOccupantEntryDecision::SkipBlocker
        );

        let occupied = LiveVehicleBuildingEntry {
            bunker_occupied: true,
            ..empty
        };
        assert_eq!(
            decide_live_vehicle_building_entry(occupied),
            BuildingOccupantEntryDecision::KeepBlocker
        );
    }

    #[test]
    fn row_helper_requires_same_candidate_building_and_rows_value() {
        let mut other_building = row_entry_input(
            EntityCategory::Unit,
            VehicleBuildingEntryBranch::UnitRepairOrBunker,
            11,
        );
        other_building.is_unit_repair = true;
        other_building.candidate_building_id = Some(200);
        assert_eq!(
            decide_live_vehicle_building_entry(other_building),
            BuildingOccupantEntryDecision::KeepBlocker
        );

        assert_eq!(
            decide_live_vehicle_building_entry(LiveVehicleBuildingEntry {
                branch: VehicleBuildingEntryBranch::RadioContact {
                    mover_has_contact: true
                },
                ..other_building
            }),
            BuildingOccupantEntryDecision::SkipBlocker,
            "73F5A2 consumes false458A00 as skip even for a different first Building"
        );
        let no_rows = LiveVehicleBuildingEntry {
            candidate_building_id: Some(100),
            number_impassable_rows: -1,
            ..other_building
        };
        assert_eq!(
            decide_live_vehicle_building_entry(no_rows),
            BuildingOccupantEntryDecision::KeepBlocker
        );
    }

    #[test]
    fn find_primary_blocker_does_not_use_bypass_grid_as_structure_skip() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        // Cell occupancy: a Structure (refinery) at (5, 5).
        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            100,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );

        // EntityStore with the structure entity.
        let mut entities = EntityStore::new();
        let mut refinery = GameEntity::test_default(100, "GAREFN", "Allies", 5, 5);
        refinery.category = EntityCategory::Structure;
        entities.insert(refinery);

        // With bypass_grid=true: structure is filtered, no other occupants → None.
        let result = find_primary_blocker(
            (5, 5),
            MovementLayer::Ground,
            42,   // mover_id
            true, // mover_bypass_grid
            None,
            &occ,
            &entities,
        );
        assert_eq!(
            result,
            Some(100),
            "bypass_grid must not erase live structure blockers"
        );

        // With bypass_grid=false: structure is the primary blocker → Some(100).
        let result = find_primary_blocker(
            (5, 5),
            MovementLayer::Ground,
            42,
            false, // mover_bypass_grid
            None,
            &occ,
            &entities,
        );
        assert_eq!(
            result,
            Some(100),
            "with bypass_grid=false, Structure must still be picked as blocker (regression)"
        );
    }

    #[test]
    fn find_primary_blocker_follows_layer_order() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        occ.add(
            5,
            5,
            20,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );

        let mut entities = EntityStore::new();
        let mut blocker = GameEntity::test_default(10, "HTNK", "Allies", 5, 5);
        blocker.category = EntityCategory::Unit;
        entities.insert(blocker);
        let mut infantry = GameEntity::test_default(20, "E1", "Allies", 5, 5);
        infantry.category = EntityCategory::Infantry;
        entities.insert(infantry);

        let result = find_primary_blocker(
            (5, 5),
            MovementLayer::Ground,
            42,
            false,
            None,
            &occ,
            &entities,
        );
        assert_eq!(result, Some(20));
    }

    #[test]
    fn find_primary_blocker_skips_caller_ignored_ids() {
        use crate::sim::entity_store::EntityStore;

        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        occ.add(
            5,
            5,
            20,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let ignored = std::collections::BTreeSet::from([10]);
        let entities = EntityStore::new();
        let result = find_primary_blocker(
            (5, 5),
            MovementLayer::Ground,
            42,
            false,
            Some(&ignored),
            &occ,
            &entities,
        );

        assert_eq!(result, Some(20));
    }

    #[test]
    fn split_context_uses_occupancy_bits_layer_for_presence() {
        use crate::sim::pathfinding::PathGrid;

        let mut grid = PathGrid::new(10, 10);
        grid.set_cell_for_test(5, 5, 0, true, true);
        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let result = check_terrain_with_layers(
            (5, 5),
            CanEnterLayerContext {
                terrain_layer: MovementLayer::Bridge,
                object_list_layer: MovementLayer::Bridge,
                occupancy_bits_layer: MovementLayer::Ground,
            },
            EntityCategory::Unit,
            Some(&grid),
            None,
            &occ,
        );

        assert_eq!(result, TerrainCheckResult::NeedsBlockerCheck);
    }

    #[test]
    fn infantry_under_span_occupation_uses_ground_subcells_and_blockers() {
        let mut grid = PathGrid::new(1, 1);
        grid.set_cell_for_test(0, 0, 2, true, true);
        let check = |occupation: &OccupancyGrid| {
            check_terrain_with_layers(
                (0, 0),
                CanEnterLayerContext::single(MovementLayer::Ground),
                EntityCategory::Infantry,
                Some(&grid),
                None,
                occupation,
            )
        };
        let mut occupation = OccupancyGrid::new();
        occupation.add(
            0,
            0,
            10,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        assert_eq!(
            check(&occupation),
            TerrainCheckResult::Clear,
            "a deck vehicle must not block the ground"
        );
        occupation.add(
            0,
            0,
            11,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        assert_eq!(
            check(&occupation),
            TerrainCheckResult::NeedsBlockerCheck,
            "ground vehicles still require classification"
        );
        occupation.remove_on_layer(0, 0, 11, MovementLayer::Ground);
        for (index, slot) in bump_crush::FUNCTIONAL_SUB_CELLS.into_iter().enumerate() {
            occupation.add(
                0,
                0,
                20 + index as u64,
                MovementLayer::Ground,
                Some(slot),
                CellListInsertion::PrependNonBuilding,
            );
        }
        assert_eq!(
            check(&occupation),
            TerrainCheckResult::NeedsBlockerCheck,
            "three ground infantry fill the functional subcells"
        );
        occupation.remove_on_layer(0, 0, 20, MovementLayer::Ground);
        assert_eq!(
            check(&occupation),
            TerrainCheckResult::Clear,
            "the freed ground subcell must admit infantry"
        );
    }

    #[test]
    fn oracle_wrapper_preserves_split_layers_and_yr_code() {
        use crate::sim::pathfinding::PathGrid;

        let mut grid = PathGrid::new(10, 10);
        grid.set_cell_for_test(5, 5, 0, true, true);
        let layers = CanEnterLayerContext {
            terrain_layer: MovementLayer::Bridge,
            object_list_layer: MovementLayer::Bridge,
            occupancy_bits_layer: MovementLayer::Ground,
        };
        let (result, row) = check_terrain_with_layers_oracle(
            (5, 5),
            layers,
            EntityCategory::Unit,
            Some(&grid),
            None,
            &empty_occ(),
        );

        assert_eq!(result, TerrainCheckResult::Clear);
        assert_eq!(row.terrain_layer, MovementLayer::Bridge);
        assert_eq!(row.object_list_layer, MovementLayer::Bridge);
        assert_eq!(row.occupancy_bits_layer, MovementLayer::Ground);
        assert_eq!(row.yr_code, Some(0));
    }

    #[test]
    fn split_context_uses_object_list_layer_for_selected_blockers() {
        use crate::sim::pathfinding::PathGrid;

        let mut grid = PathGrid::new(10, 10);
        grid.set_cell_for_test(5, 5, 0, true, true);
        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            10,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let result = check_terrain_with_layers(
            (5, 5),
            CanEnterLayerContext {
                terrain_layer: MovementLayer::Bridge,
                object_list_layer: MovementLayer::Bridge,
                occupancy_bits_layer: MovementLayer::Ground,
            },
            EntityCategory::Unit,
            Some(&grid),
            None,
            &occ,
        );

        assert_eq!(result, TerrainCheckResult::NeedsBlockerCheck);
    }

    #[test]
    fn split_context_scans_object_list_layer_for_primary_blocker() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let mut occ = OccupancyGrid::new();
        occ.add(
            5,
            5,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        occ.add(
            5,
            5,
            20,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let mut entities = EntityStore::new();
        let mut ground = GameEntity::test_default(10, "HTNK", "Allies", 5, 5);
        ground.category = EntityCategory::Unit;
        entities.insert(ground);
        let mut bridge = GameEntity::test_default(20, "HTNK", "Soviets", 5, 5);
        bridge.category = EntityCategory::Unit;
        entities.insert(bridge);

        let alliances = HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();
        let result = classify_occupied_cell_with_layers(
            (5, 5),
            CanEnterLayerContext {
                terrain_layer: MovementLayer::Bridge,
                object_list_layer: MovementLayer::Bridge,
                occupancy_bits_layer: MovementLayer::Ground,
            },
            42,
            bump_crush::CrushCapability::new(false, false),
            "Allies",
            LocomotorKind::Drive,
            false,
            &occ,
            &entities,
            &alliances,
            &interner,
        );

        assert_eq!(result, CellEntryResult::OccupiedEnemy { blocker_id: 20 });
    }

    #[test]
    fn yr_search_cost_class_rejects_seven_without_neighbor_cost() {
        assert_eq!(
            search_cell_cost_decision(7, false),
            SearchCellCostDecision {
                raw_cost_class: 7,
                effective_cost_class: None,
                expands: false,
                should_call_edge_cost: false,
            }
        );
    }

    #[test]
    fn yr_search_cost_class_preserves_accepted_class_when_gate_is_off() {
        let decision = search_cell_cost_decision(4, false);
        assert_eq!(decision.effective_cost_class, Some(4));
        assert!(decision.expands);
        assert!(decision.should_call_edge_cost);
    }

    #[test]
    fn yr_search_cost_class_coerces_accepted_class_when_gate_is_on() {
        let decision = search_cell_cost_decision(6, true);
        assert_eq!(decision.effective_cost_class, Some(0));
        assert!(decision.expands);
        assert!(decision.should_call_edge_cost);
    }

    #[test]
    fn yr_search_cost_class_two_remains_an_accepted_special_case_input() {
        let decision = search_cell_cost_decision(2, false);
        assert_eq!(decision.effective_cost_class, Some(2));
        assert!(decision.expands);
        assert!(decision.should_call_edge_cost);
    }

    /// `tables` carries the alliance context the arm needs to answer the ally
    /// test; `None` models a caller that has not wired it, which the arm treats
    /// as a reason to decline rather than to guess.
    type AllianceTables<'a> = (
        &'a HouseAllianceMap,
        &'a crate::sim::intern::StringInterner,
        crate::sim::intern::InternedId,
    );

    fn wall_arm<'a>(
        tables: Option<AllianceTables<'a>>,
        is_armed: bool,
        warhead_wall: bool,
        warhead_wood: bool,
    ) -> WallArmContext<'a> {
        WallArmContext {
            overlay_grid: None,
            overlay_registry: None,
            alliances: tables.map(|(alliances, _, _)| alliances),
            interner: tables.map(|(_, interner, _)| interner),
            mover_owner: tables.map(|(_, _, owner)| owner),
            is_armed,
            warhead_wall,
            warhead_wood,
        }
    }

    /// The `Wood=` clause of the wall arm is **Unit-only**.
    ///
    /// `UnitClass::Can_Enter_Cell 0x0073F0A0` tests `Wall=` (`+0x144`) at
    /// `0x0073F4A9` and then `Wood=` (`+0x147`) at `0x0073F4B3` gated on the
    /// overlay's `Armor` (`+0x9C == 6`) at `0x0073F4BD`. The infantry arm
    /// `InfantryClass::Can_Enter_Cell 0x0051BF90` instead calls `FUN_00772AC0`,
    /// whose entire body is `warhead != 0 && *(warhead + 0x144) != 0` — `Wall=`
    /// only, no `Wood=` and no armor compare (decompiled 2026-09-16).
    ///
    /// Stock-reachable: `[SHK]` Shock Trooper (`Category=Soldier`,
    /// `Primary=ElectricBolt` -> warhead `[Shock]`, which declares `Wood=yes`
    /// and no `Wall=`) against `[CAKRMW]` (`Armor=wood`, `Crushable=no`) — a
    /// non-crushable wooden wall gamemd refuses it.
    #[test]
    fn the_wall_arm_wood_route_is_unit_only() {
        // Intern before cloning the thread-local: the clone must already carry
        // the id, or `resolve` in the ally test finds nothing.
        let mover = crate::sim::intern::test_intern("Americans");
        let interner = crate::sim::intern::test_interner();
        let alliances = HouseAllianceMap::new();
        let tables = Some((&alliances, &interner, mover));
        let arm = |armed, wall, wood| wall_arm(tables, armed, wall, wood);

        // Wood= against a wooden wall: the vehicle routes, the infantryman does not.
        assert_eq!(
            arm(true, false, true).weapon_route_code(true, None, false),
            Some(5)
        );
        assert_eq!(
            arm(true, false, true).weapon_route_code(true, None, true),
            None
        );
        // Wall= routes for both classes.
        assert_eq!(
            arm(true, true, false).weapon_route_code(false, None, false),
            Some(5)
        );
        assert_eq!(
            arm(true, true, false).weapon_route_code(false, None, true),
            Some(5)
        );
        // Wood= against a non-wood overlay never routes, for either class.
        assert_eq!(
            arm(true, false, true).weapon_route_code(false, None, false),
            None
        );
        // An unarmed mover leaves at 0x0073F48F before any warhead is read.
        assert_eq!(
            arm(false, true, true).weapon_route_code(true, None, false),
            None
        );
        // Wiring gap: with no alliance tables the arm declines instead of
        // guessing "enemy" and quietly pricing the wall at 20x. An unowned wall
        // with the tables present still takes 5 — that is the `None` wall_owner
        // in the cases above.
        assert_eq!(
            wall_arm(None, true, true, false).weapon_route_code(false, None, false),
            None
        );
    }

    /// `astar_search` consults this classifier ONLY after its own
    /// `neighbor_passable` has refused the neighbour, so a `Clear` answer is not
    /// new information and must never re-admit the cell.
    ///
    /// Mapping `Clear` to class 0 would drop the terms the classifier cannot
    /// see — the ground/bridge layer split and the `neighbor_cell.transition`
    /// (`0x200`) gate a ground->bridge entry must pass — and silently expand a
    /// refused neighbour at 1x.
    #[test]
    fn the_wall_classifier_never_upgrades_a_refusal_to_passable() {
        use crate::sim::pathfinding::SearchCellCostClassifier as _;
        let terrain = crushable_wall_grid();
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let classifier = WallSearchCostClassifier {
            wall: wall_arm(None, true, true, true),
            path_grid: Some(&grid),
            resolved_terrain: Some(&terrain),
            terrain_costs: None,
            movement_zone: Some(MovementZone::Normal),
            speed_type: None,
            is_infantry: false,
            mover_is_crusher: false,
        };
        // (0, 0) is ordinary clear ground: the arm answers Clear, which must
        // still read as "keep the refusal", never as class 0.
        assert_eq!(classifier.classify((0, 0), (0, 0), false), 7);
        // (1, 1) is the fixture's crushable wall. This arm carries no overlay
        // grid, so `overlay_at` yields nothing and the weapon route never runs;
        // the refusal stands at 7. The overlay-backed 4/5 case is covered by
        // `the_wall_arm_answers_seven_where_the_land_row_refuses` below.
        assert_eq!(classifier.classify((0, 0), (1, 1), false), 7);
    }

    /// A 3x3 board whose centre carries a non-crushable `Wall=yes` overlay.
    ///
    /// `track_row` is the centre cell's `Track` land row: `Some(0)` is a row
    /// that refuses the mover, `Some(100)` one that admits it.
    fn wall_row_fixture(track_row: Option<u8>) -> ResolvedTerrainGrid {
        let mut cells = Vec::with_capacity(9);
        for ry in 0..3u16 {
            for rx in 0..3u16 {
                let mut cell = ResolvedTerrainCell::clear_for_test(rx, ry);
                cell.speed_costs.track = Some(100);
                if (rx, ry) == (1, 1) {
                    // `RecalcZoneType` reduces a non-crushable `Wall=` overlay to
                    // class 2, and `ResolvedTerrainGrid` marks it `overlay_blocks`
                    // — which is exactly why the wall arm cannot key on the wider
                    // `land_passable`.
                    cell.zone_type = zone_class::WALL;
                    cell.overlay_zone_type = Some(zone_class::WALL);
                    cell.overlay_blocks = true;
                    cell.speed_costs.track = track_row;
                }
                cells.push(cell);
            }
        }
        ResolvedTerrainGrid::from_cells(3, 3, cells)
    }

    /// Native's wall arm does **not** return. It accumulates 4/5 into the
    /// running code, falls through the occupant walk, and then reads the ground
    /// land row at `0x0073FAB5` (`FLD [ECX*4 + 0x89EA40]` / `FCOMP 0.0`); a zero
    /// row returns 7 at `0x0073FAD0`, and `InfantryClass` does the same at
    /// `0x0051C7D0`, whatever the arm accumulated. So a wall standing on terrain
    /// this mover's speed row refuses answers 7, not 4/5.
    ///
    /// This is also the first exercise of the overlay-backed producer path:
    /// `WallArmContext::overlay_at` -> `weapon_route_code` with a live
    /// `OverlayGrid` and `OverlayTypeRegistry`.
    #[test]
    fn the_wall_arm_answers_seven_where_the_land_row_refuses() {
        use crate::rules::ini_parser::IniFile;
        let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(
            &IniFile::from_str("[OverlayTypes]\n0=GAWALL\n\n[GAWALL]\nWall=yes\n"),
            None,
        );
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
        // Unowned: `Is_Ally_ByIndex` rejects index -1, so the route takes 5.
        overlays.cell_mut(1, 1).overlay_id = Some(0);
        // Intern before cloning the thread-local interner.
        let mover = crate::sim::intern::test_intern("Americans");
        let interner = crate::sim::intern::test_interner();
        let alliances = HouseAllianceMap::new();

        let entry = |track_row: Option<u8>| {
            let terrain = wall_row_fixture(track_row);
            let grid = PathGrid::from_resolved_terrain(&terrain);
            evaluate_can_enter_cell(CanEnterCellContext {
                wall: Some(WallArmContext {
                    overlay_grid: Some(&overlays),
                    overlay_registry: Some(&registry),
                    alliances: Some(&alliances),
                    interner: Some(&interner),
                    mover_owner: Some(mover),
                    is_armed: true,
                    warhead_wall: true,
                    warhead_wood: false,
                }),
                target: (1, 1),
                terrain_layer: MovementLayer::Ground,
                movement_zone: Some(MovementZone::Normal),
                speed_type: Some(SpeedType::Track),
                path_grid: Some(&grid),
                resolved_terrain: Some(&terrain),
                terrain_costs: None,
                bypass_grid: false,
                mode: TerrainEntryMode::AStarNeighbor,
                is_infantry: false,
                mover_is_crusher: false,
            })
        };

        // Row admits the mover: the weapon route survives as the enemy class 5.
        assert_eq!(
            entry(Some(100)),
            CanEnterCellResult::WallBlocked { cost_class: 5 }
        );
        // Row refuses it: native's post-arm read returns 7 regardless of the
        // accumulated 5, so the wall class must not escape.
        assert_eq!(entry(Some(0)), CanEnterCellResult::HardBlocked);
    }
}
