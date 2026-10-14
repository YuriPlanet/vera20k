//! Cell entry: the reduced terrain arm and shared Can_Enter_Cell leaves.
//!
//! The original RA2 engine returns 8 distinct codes when a unit
//! tries to enter a cell. Each code triggers a different movement response.
//! The classes' own `Can_Enter_Cell` ports are `Simulation::foot_can_enter`
//! and `aircraft_can_enter` (`world/object_entry.rs`). This module keeps the
//! terrain-only `evaluate_can_enter_cell` for callers without a live receiver,
//! and the building-entry and crush leaves those ports call.
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
//! What VERA does not reproduce, recorded rather than guessed and ordered by
//! ordinary-skirmish impact:
//!
//! - **Gate identity corrected (2026-09-22).** `BuildingType+16B7` is `Gate=`:
//!   ReadINI4609EA..460A13 reads literal81AA8C. `4525F0` is gate passability,
//!   requiring Open mission24 and stable-open animation state. Unit73F6E8
//!   skips an open gate; a closed allied gate raises3, armed enemy raises5,
//!   unarmed enemy returns7. The old garrison interpretation was false.
//!   `+16C0` is the separate FirestormWall flag. Complete Unit-call evidence:
//!   `tools/spatial_oracle/unit_entry`. This establishes classification, not
//!   the downstream gate-opening/scatter response for every caller.
//! - **Walls are a hard block here.** The class wall arm (Infantry
//!   `0x0051C17C..0x0051C223`, Unit `0x0073F3D7..0x0073F51F`) is ported in the
//!   live receiver (`world/object_entry.rs`), which Find_Path's search and the
//!   movement steps ask. This reduced arm, for callers without a live
//!   receiver, refuses a `Wall=` cell except to the Destroyer movement zones
//!   and `MovementZone=CrusherAll`, and admits a `Crushable=` one only to a
//!   crusher or CrusherAll. Native answers 4 or 5 to a mover whose primary
//!   warhead can shoot the wall, and 4 to a crusher on an allied crushable
//!   wall. No route search reads this arm: every order takes its class setter,
//!   and Find_Path asks the live receiver. Its remaining caller is the move
//!   cursor's per-cell probe (`app::input::cursor`), which shows No-Move for
//!   any answer above 1, so a shootable wall reads the same. Trigger: a
//!   crusher hovering an allied crushable wall. Effect: the cursor shows Move
//!   where native shows No-Move. Frequency: rare. Risk: the cursor only.
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
//! - Part of sim/ — depends on sim/bump_crush, sim/locomotor, sim/pathfinding,
//!   map/entities, map/houses, rules/locomotor_type.

use super::PathGrid;
use super::terrain_cost::TerrainCostGrid;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;

// ---------------------------------------------------------------------------
// Result enums
// ---------------------------------------------------------------------------

/// Terrain-only result for native-shaped cell-entry checks above `PathGrid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanEnterCellResult {
    Clear,
    HardBlocked,
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
    mover_is_train: bool,
) -> SearchCellCostDecision {
    let effective_cost_class = if mover_is_train && raw_cost_class < 7 {
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
    /// Selects the infantry view of terrain-object occupation. Retail terrain
    /// objects occupy sub-cells, and only the infantry entry gate reads that
    /// mask; vehicles stay blocked by the whole cell.
    pub is_infantry: bool,
    /// The mover's `Crusher=` type flag (`UnitTypeClass+0xD28`), the key of
    /// the crusher route of the Unit wall arm (`0x0073F438`). Infantry never
    /// takes that route; pass `false` where the mover is unknown.
    pub mover_is_crusher: bool,
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
            evaluate_shared_cell_leaf(ctx, bridge_walkable)
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
        return evaluate_shared_cell_leaf(ctx, land_passable);
    }

    let grid_ok = ctx.path_grid.is_none_or(|grid| {
        if ctx.is_infantry {
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

    evaluate_shared_cell_leaf(ctx, grid_ok && speed_passable && terrain_cost_passable)
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
fn evaluate_shared_cell_leaf(
    ctx: CanEnterCellContext<'_>,
    land_passable: bool,
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
        // this class contract through the virtual slot. Keep the coarse wall
        // result here (the class wall arm is the live receiver's; see the
        // module residual), without importing the unrelated Cell leaf's level
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
            // Retain the prior live-adapter missing-target rejection. This does
            // not model native dummy-cell access.
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
        // none. It admits the movers native's crusher route
        // (`0x0073F42E..F46E`) lets enter; every other mover, and every
        // infantryman (`0x0051BF90` has no crusher route), is refused.
        let crushable_wall =
            terrain_cell.is_some_and(|cell| cell.zone_type == zone_class::CRUSHABLE);
        let crushable_wall_admitted =
            !ctx.is_infantry && (ctx.mover_is_crusher || movement_zone == MovementZone::CrusherAll);
        if crushable_wall && !crushable_wall_admitted {
            return CanEnterCellResult::HardBlocked;
        }
        return if !wall_cleared && (wall || !land_passable) {
            CanEnterCellResult::HardBlocked
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

/// The cell's land row (`Ground[LandType]`, `0x0089EA40`) for `speed_type`:
/// false where it gives no speed (`FCOMP 0.0`); an unbound row admits.
pub(crate) fn speed_type_allows_cell(cell: &ResolvedTerrainCell, speed_type: SpeedType) -> bool {
    cell.speed_costs
        .cost_for_speed_type(speed_type)
        .is_none_or(|cost| cost > 0)
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

/// The Unit-target subset of Object::IsCrushableBy5F6CD0, reached by the
/// post-latch GetUnit(false) exception at73FD17. The caller has already supplied
/// a latch; this leaf does not retest Crusher/ability or apply kills. Both native
/// arms test mover-house alliance and the target's +160 invulnerability slot.
/// A rejected Omni arm falls through to ordinary Crushable, which does not
/// read OmniCrushResistant or the mover's regular-crusher flag.
pub(crate) fn unit_tail_is_crushable_by(
    unit: &GameEntity,
    capability: bump_crush::CrushCapability,
    mover_considers_target_allied: bool,
    current_frame: u32,
) -> bool {
    debug_assert_eq!(unit.category(), EntityCategory::Unit);
    let target = bump_crush::CrushTarget::from_entity(unit, current_frame);
    // Live Unit entities carry Techno abstract flag1. Deploy crush immunity
    // is written only by Infantry deploy; it is always clear on this subset.
    !mover_considers_target_allied
        && bump_crush::object_is_crushable_by(capability.omni_crusher, target)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
            target: (1, 1),
            terrain_layer: MovementLayer::Ground,
            movement_zone: Some(movement_zone),
            speed_type: None,
            path_grid: Some(grid),
            resolved_terrain: Some(terrain),
            terrain_costs: None,
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
            target: (0, 0),
            terrain_layer: MovementLayer::Ground,
            movement_zone: Some(MovementZone::Normal),
            speed_type: None,
            path_grid: Some(&grid),
            resolved_terrain: Some(&terrain),
            terrain_costs: None,
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
                Some(MovementZone::Normal),
                Some(&terrain),
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
}
