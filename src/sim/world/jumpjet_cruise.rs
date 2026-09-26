//! Production host for the Jumpjet locomotor kernel ([`jumpjet_flight`]).
//!
//! Every Jumpjet runs `Process @ 0x0054AEC0` here: the Update gate
//! (`Is_Moving 0x0054AE50` or `Is_Moving_Now 0x0054D0D0`) and then the state at
//! `+0x50` — ground `0x0054B980`, ascend `0x0054BA30`, hold `0x0054BD30`,
//! translate `0x0054BFF0`, descend `0x0054C550`. That state field is the only
//! Jumpjet phase: readers take `JumpjetRuntime::phase`, and `AirMovePhase`
//! (Fly's phase) is neither written nor read for a Jumpjet. The locomotor
//! altitude is derived from the kernel's height. VERA's air adapter does not
//! drive a Jumpjet at all, which is what stops an idle one cycling takeoff and
//! landing forever.
//!
//! The cell `AltObject` air slot (`CellClass+0xE0`; `0x004135A0` queries it and
//! `0x00487D70` sets or clears it) lives in `ObjectSubstrate::air_slots`.
//! Claims and releases are collected as effects while the substrate stays
//! borrowed immutably, then applied once the frame's states have run.
//!
//! Orders reach the locomotor through `Move_To` and `Stop_Moving`
//! (`movement::jumpjet_movement`). VERA's movement adapter carries a Jumpjet's
//! orders in its goal cell, so each frame first applies the native entry the
//! goal stands for: a fresh goal is `Move_To` on the cell, and a goal dropped
//! in flight is Foot's null `Set_Destination`, whose `Stop_Moving` re-targets
//! the cell under the owner. State 4's refused landing runs `Stop_Moving` once
//! the frame is committed.
//!
//! A crashing owner (`FootClass+0x425`) takes no orders: the kill's Stun ran
//! `Stop_Moving` on it (`Simulation::jumpjet_stun_stop`), which keeps a moving
//! wreck flying and lifts a descending one into State 1, so `Process` latches
//! it into State 5 wherever the kill found it, and the object turn finishes
//! its impact (`Simulation::jumpjet_crash_impact`). A touchdown clears the
//! latch (`0x0054CA12`).
//!
//! Residuals: `Can_Enter_Cell`'s graded answer collapses to clear or refused;
//! the landing State 4 admits raises only the locomotor latch, not the owner's
//! cell occupation bit (a Unit's `0x007441B0` sets `0x20`, `0x00744210` clears
//! it), so a second Jumpjet can pick a cell another is landing in; `Process`'s
//! owner `+0x90` dispatch gate, `RulesClass+0x48`'s deploy facing, owner
//! `+0x134` and the `JumpJetTurn=` hold facing are unmodelled. A Health-0
//! wreck whose kill found no cell to re-target (its search failed at the map's
//! edge) can land without its crash and stay; native's handling of that owner
//! is untraced.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/, rules/ and sim/ only.

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::retail_trig::{AtanTable, TrigTable, required_atan_table, required_math_tables};
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::Position;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::movement::air_movement::AirMovementTickStats;
use crate::sim::movement::ground_pose::{ground_surface_z_at, position_world_xy};
use crate::sim::movement::jumpjet_flight::{
    self, FlightOwnerKind, JumpjetFlightHost, STATE_ASCEND, STATE_DESCEND, STATE_HOLD,
    STATE_TRANSLATE,
};
use crate::sim::movement::jumpjet_movement::JumpjetRuntime;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{AirSlotGrid, OccupancyGrid, RawCellKey, RawCellOccupationGrid};
use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::{GROUND_LEVEL_HEIGHT_LEPTONS, ground_height_leptons};

/// `(value + ((value >> 31) & 0xFF)) >> 8`, the signed lepton-to-cell step the
/// native cell lookups use.
fn native_cell(value: i32) -> i16 {
    (value.wrapping_add((value >> 31) & 0xFF) >> 8) as i16
}

struct CruiseHost<'a> {
    frame: u32,
    trig: &'a TrigTable,
    atan: &'a AtanTable,
    terrain: Option<&'a ResolvedTerrainGrid>,
    occupancy: &'a OccupancyGrid,
    entities: &'a EntityStore,
    rules: Option<&'a RuleSet>,
    interner: &'a StringInterner,
    kind: FlightOwnerKind,
    location: [i32; 3],
    on_bridge: bool,
    balloon_hover: bool,
    has_target: bool,
    piggyback_active: bool,
    simple_deployer: bool,
    deploy_to_land: bool,
    body_facing: Option<u16>,
    grounded_reset: bool,
    /// The owner, as the air slots identify it.
    stable_id: u64,
    /// `CellClass+0xE0` as it stood when the frame began; `slot_ops` overlays
    /// the writes this frame has made so far.
    air_slots: &'a AirSlotGrid,
    raw_occupation: &'a RawCellOccupationGrid,
    path_grid: Option<&'a crate::sim::pathfinding::PathGrid>,
    speed_type: crate::rules::locomotor_type::SpeedType,
    movement_zone: crate::rules::locomotor_type::MovementZone,
    /// `ScenarioClass+0x218`, drawn only when a scatter actually happens so the
    /// RNG cursor advances exactly where the original's does.
    rng: &'a mut SimRng,
    /// Deferred `+0xE0` writes: `None` releases the cell, `Some(owner)` claims
    /// it. Applied after the kernel returns, which keeps the substrate borrowed
    /// immutably while the states run.
    slot_ops: Vec<((u16, u16), Option<u64>)>,
    /// The neighbour `Set_Destination` (vtable `+0x480`) was handed.
    scatter_to: Option<(i16, i16)>,
    /// State 4 refused the landing and called `Stop_Moving`.
    stop_requested: bool,
    /// Locomotor `+0x90`, the landing-admitted latch, in and out.
    landing_latched: bool,
    /// Touchdown ran, so the owner must be put back on the ground.
    touched_down: bool,
    /// Owner `+0x425`.
    crashing: bool,
    /// Owner `+0xB4` or `GetCurrentMission` is Enter (7).
    mission_enter: bool,
    /// `MapClass+0xF4/+0xF8`; no map size admits no cell.
    map_size: Option<(i32, i32)>,
    /// State 5 moved the owner through its Mark/display transaction.
    crash_relocated: bool,
    /// State 5 reached the ground: the owner's impact notice is due.
    impact: bool,
    /// The last `SetSpeedFraction` (owner vtable `+0x544`) of the frame, as the
    /// native double's bits.
    speed_fraction: Option<u64>,
}

impl CruiseHost<'_> {
    /// The pending-aware view of a cell's air slot.
    fn slot_holder(&self, cell: (i16, i16)) -> Option<u64> {
        if cell.0 < 0 || cell.1 < 0 {
            return None;
        }
        let key = (cell.0 as u16, cell.1 as u16);
        match self.slot_ops.iter().rev().find(|(at, _)| *at == key) {
            Some((_, pending)) => *pending,
            None => self.air_slots.holder(key.0, key.1),
        }
    }

    /// Level and slope of the cell holding `xy`, through the shared dummy cell
    /// for lookups outside the map (`MapClass::Get_CellClass_At_Coord @ 0x00565730`).
    fn cell_terrain(&self, xy: [i32; 2]) -> (u8, u8) {
        let Some(terrain) = self.terrain else {
            return (0, 0);
        };
        match terrain.native_cell_identity((native_cell(xy[0]), native_cell(xy[1]))) {
            NativeCellIdentity::Real(index) => {
                let cell = &terrain.cells()[index];
                (cell.level, cell.slope_type)
            }
            NativeCellIdentity::Dummy => {
                let dummy = terrain.shared_cell_dummy().snapshot();
                (dummy.level as u8, dummy.slope_type)
            }
        }
    }
}

impl JumpjetFlightHost for CruiseHost<'_> {
    fn binary_frame(&self) -> u32 {
        self.frame
    }

    fn trig(&self) -> &TrigTable {
        self.trig
    }

    fn atan(&self) -> &AtanTable {
        self.atan
    }

    fn owner_kind(&self) -> FlightOwnerKind {
        self.kind
    }

    fn location(&self) -> [i32; 3] {
        self.location
    }

    fn set_location(&mut self, coord: [i32; 3]) {
        self.location = coord;
    }

    fn set_z(&mut self, z: i32) {
        self.location[2] = z;
    }

    fn height_above_ground(&self) -> i32 {
        let xy = [self.location[0], self.location[1]];
        self.location[2]
            .wrapping_sub(ground_surface_z_at(xy, self.on_bridge, self.terrain, None).unwrap_or(0))
    }

    fn on_bridge(&self) -> bool {
        self.on_bridge
    }

    fn grounded_reset(&mut self) {
        self.grounded_reset = true;
        self.on_bridge = false;
    }

    fn floor_height(&self, xy: [i32; 2]) -> i32 {
        ground_surface_z_at(xy, false, self.terrain, None).unwrap_or(0)
    }

    fn cell_high_bridge(&self, xy: [i32; 2]) -> bool {
        self.terrain.is_some_and(|terrain| {
            let cell = terrain.native_cell_identity((native_cell(xy[0]), native_cell(xy[1])));
            terrain.native_cell_flags(cell) & 0x100 != 0
        })
    }

    fn cell_top_height(&self, xy: [i32; 2]) -> i32 {
        let (level, slope) = self.cell_terrain(xy);
        let centre = ground_height_leptons(level, slope, 128, 128).unwrap_or(0);
        let (cx, cy) = (native_cell(xy[0]), native_cell(xy[1]));
        if cx < 0 || cy < 0 {
            return jumpjet_flight::cell_top_height(centre, None, false);
        }
        let (rx, ry) = (cx as u16, cy as u16);
        // `BuildingTypeClass::Dimension2 @ 0x00464AF0`: art `Height=` (`+0xEF4`,
        // written at `0x00461101` from the ART key at `0x0081A7A8`) times
        // `g_HeightFactor` (`0x0089DDB8`). The startup chain `0x0045AFA0..
        // 0x0045B070` sets it to 104 under the native control word
        // (`tools/spatial_oracle/height_factor.json`), the same value as the
        // cell level height.
        let building_height = self
            .occupancy
            .first_building_on_layer(rx, ry, MovementLayer::Ground)
            .map(|id| {
                self.entities
                    .get(id)
                    .zip(self.rules)
                    .and_then(|(building, rules)| {
                        rules
                            .object(self.interner.resolve(building.type_ref()))
                            .map(|object| rules.building_launch_height(object))
                    })
                    .unwrap_or(0)
                    .wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS)
            });
        let any_techno = self
            .occupancy
            .get(rx, ry)
            .is_some_and(|cell| !cell.is_empty_on(MovementLayer::Ground));
        jumpjet_flight::cell_top_height(centre, building_height, any_techno)
    }

    fn cell_land_type(&self, xy: [i32; 2]) -> u8 {
        self.terrain.map_or(0, |terrain| {
            match terrain.native_cell_identity((native_cell(xy[0]), native_cell(xy[1]))) {
                NativeCellIdentity::Real(index) => terrain.cells()[index].yr_cell_land_type,
                NativeCellIdentity::Dummy => 0,
            }
        })
    }

    fn balloon_hover(&self) -> bool {
        self.balloon_hover
    }

    fn has_target(&self) -> bool {
        self.has_target
    }

    fn piggyback_active(&self) -> bool {
        self.piggyback_active
    }

    fn piggyback_arrival(&mut self) {}

    fn simple_deployer(&self) -> bool {
        self.simple_deployer
    }

    fn type_flag_6ad(&self) -> bool {
        self.deploy_to_land
    }

    fn set_speed_fraction(&mut self, fraction_bits: u64) {
        self.speed_fraction = Some(fraction_bits);
    }

    fn arrival_notify(&mut self) {}

    fn snap_body_facing(&mut self, facing: u16) {
        self.body_facing = Some(facing);
    }

    fn hold_target_facing(&self) -> Option<u16> {
        None
    }

    fn cell_of(&self, xy: [i32; 2]) -> (i16, i16) {
        (native_cell(xy[0]), native_cell(xy[1]))
    }

    fn body_facing(&self) -> u16 {
        self.body_facing.unwrap_or(0)
    }

    fn random_direction(&mut self) -> u32 {
        self.rng.next_range_i32_inclusive(0, 7) as u32
    }

    fn air_slot_taken_at(&mut self, cell: (i16, i16)) -> bool {
        self.slot_holder(cell)
            .is_some_and(|held| held != self.stable_id)
    }

    fn holds_air_slot_at(&self, cell: (i16, i16)) -> bool {
        self.slot_holder(cell) == Some(self.stable_id)
    }

    fn air_slot_empty_at(&self, cell: (i16, i16)) -> bool {
        self.slot_holder(cell).is_none()
    }

    fn release_owner_air_slots(&mut self) {
        // Stands in for the cached-cell (`+0x560`) release: drop every cell
        // this owner still holds, so a claim cannot orphan when it drifts.
        let held: Vec<(u16, u16)> = self
            .air_slots
            .entries()
            .filter(|(_, _, owner)| *owner == self.stable_id)
            .map(|(rx, ry, _)| (rx, ry))
            .collect();
        for cell in held {
            self.slot_ops.push((cell, None));
        }
    }

    fn claim_air_slot_at(&mut self, cell: (i16, i16)) {
        if cell.0 >= 0 && cell.1 >= 0 {
            self.slot_ops
                .push(((cell.0 as u16, cell.1 as u16), Some(self.stable_id)));
        }
    }

    fn release_air_slot_at(&mut self, cell: (i16, i16)) {
        if cell.0 >= 0 && cell.1 >= 0 {
            self.slot_ops.push(((cell.0 as u16, cell.1 as u16), None));
        }
    }

    fn set_destination_cell(&mut self, cell: (i16, i16)) {
        self.scatter_to = Some(cell);
    }

    fn can_enter_cell(&self, cell: (i16, i16)) -> i32 {
        // Native answers a graded value (0 clear, 2 conditional, above 2
        // refused); VERA's passability predicate is binary, so this collapses
        // to clear or refused. Residual recorded in the acceptance ledger.
        if cell.0 < 0 || cell.1 < 0 {
            return 3;
        }
        let Some(grid) = self.path_grid else {
            return 0;
        };
        let clear = crate::sim::pathfinding::is_cell_passable_for_mover_on_layer_with_speed(
            grid,
            cell.0 as u16,
            cell.1 as u16,
            MovementLayer::Ground,
            Some(self.movement_zone),
            Some(self.speed_type),
            self.terrain,
            None,
            false,
            crate::sim::pathfinding::cell_entry::TerrainEntryMode::AStarNeighbor,
        );
        if clear { 0 } else { 3 }
    }

    fn sub_cell_free(&self, cell: (i16, i16), sub_cell: i32, bridge: bool) -> bool {
        if cell.0 < 0 || cell.1 < 0 || !(0..8).contains(&sub_cell) {
            return true;
        }
        let key = RawCellKey::Real(cell.0 as u16, cell.1 as u16);
        let layer = if bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        self.raw_occupation.bits_at(key, layer) & (1u8 << sub_cell) == 0
    }

    fn mission_is_seven(&self) -> bool {
        self.mission_enter
    }

    fn stop_moving(&mut self) -> i32 {
        // `Stop_Moving` is State 4's last act and needs the world's search, so
        // it runs once the frame is committed
        // (`Simulation::jumpjet_stop_moving`), lifting the descent into State 1
        // when it re-targets.
        self.stop_requested = true;
        STATE_DESCEND
    }

    fn cell_high_bridge_at(&self, cell: (i16, i16)) -> bool {
        self.terrain.is_some_and(|terrain| {
            terrain.native_cell_flags(terrain.native_cell_identity((cell.0, cell.1))) & 0x100 != 0
        })
    }

    fn landing_latched(&self) -> bool {
        self.landing_latched
    }

    fn begin_landing(&mut self, _destination: [i32; 3]) {
        self.landing_latched = true;
    }

    fn deploy_latched(&self) -> bool {
        // Owner `+0x134` has no VERA equivalent yet.
        false
    }

    fn deploy_facing(&self) -> Option<u16> {
        // `RulesClass+0x48` is not read yet, so a simple deployer keeps its
        // heading instead of turning to the deploy facing.
        None
    }

    fn touchdown(&mut self) {
        self.touched_down = true;
        let here = self.cell_of([self.location[0], self.location[1]]);
        if self.holds_air_slot_at(here) {
            self.release_air_slot_at(here);
        }
    }

    fn crashing(&self) -> bool {
        self.crashing
    }

    fn in_bounds(&self, cell: (i16, i16)) -> bool {
        self.map_size.is_some_and(|(width, height)| {
            crate::map::playfield::size_diamond_contains(width, height, cell)
        })
    }

    fn crash_relocate(&mut self, coord: [i32; 3]) {
        // The Mark and display calls run on the committed move below.
        self.location = coord;
        self.crash_relocated = true;
    }

    fn crash_impact(&mut self) {
        // The tracker removal and the notice are the owner's; the object turn
        // runs them once the frame's states are committed.
        self.impact = true;
    }
}

/// Write a world-lepton coordinate back into the cell/sub-cell position and
/// the exact Z the renderer and range checks read.
fn commit_world_location(position: &mut Position, location: [i32; 3]) {
    let cell_x = i32::from(native_cell(location[0])).max(0);
    let cell_y = i32::from(native_cell(location[1])).max(0);
    position.rx = cell_x as u16;
    position.ry = cell_y as u16;
    position.sub_x = SimFixed::from_num((location[0] - cell_x * 256).clamp(0, 255));
    position.sub_y = SimFixed::from_num((location[1] - cell_y * 256).clamp(0, 255));
    position.exact_z_leptons = Some(location[2]);
}

/// Every Jumpjet the native locomotor owns - all of them, airborne or landed.
/// `Process 0x0054AEC0` itself decides whether a frame does anything, so an
/// idle landed or idle holding owner is advanced by nothing.
fn jumpjet_locomotor(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.kind == LocomotorKind::Jumpjet && locomotor.jumpjet_runtime().is_some()
    })
}

/// What the kernel asked the world to do, collected while the substrate was
/// still borrowed immutably.
struct HostEffects {
    moving: bool,
    /// The destination the frame flew toward, which is what the locomotor keeps
    /// at `+0x40` — not the owner's new position.
    destination: [i32; 3],
    /// The state the frame began in, so the caller can see a cruise end.
    entry_state: i32,
    body_facing: Option<u16>,
    grounded_reset: bool,
    height: i32,
    slot_ops: Vec<((u16, u16), Option<u64>)>,
    scatter_to: Option<(i16, i16)>,
    stop_requested: bool,
    landing_latched: bool,
    touched_down: bool,
    crash_relocated: bool,
    impact: bool,
    speed_fraction: Option<u64>,
}

impl Simulation {
    /// One `Process @ 0x0054AEC0` frame for a Jumpjet: the Update gate, then
    /// the state at `+0x50`. Answers `None` for any other locomotor, so the air
    /// adapter keeps every other flier.
    pub(crate) fn tick_jumpjet_cruise_one(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
    ) -> Option<AirMovementTickStats> {
        let frame = self.session.binary_frame;
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(jumpjet_locomotor)
        {
            return None;
        }
        self.apply_jumpjet_adapter_order(stable_id, rules);
        // Taken before the substrate borrows so the host can hold the rest.
        let path_grid = self.path_grid_snapshot();
        let map_size = self.map_size_diamond();
        let terrain = self.resolved_terrain.as_ref();

        let (state, flight, location, effects) = {
            let entity = self.substrate.entities.get(stable_id)?;
            let locomotor = entity.locomotor.as_ref()?;
            let runtime = locomotor.jumpjet_runtime()?;
            let moving = runtime.moving;
            let state = runtime.phase;
            let destination = [
                runtime.destination.x,
                runtime.destination.y,
                runtime.destination.z,
            ];

            let xy = position_world_xy(&entity.position);
            let z = entity.position.exact_z_leptons.unwrap_or_else(|| {
                ground_surface_z_at(xy, entity.on_bridge, terrain, None)
                    .unwrap_or(0)
                    .wrapping_add(locomotor.altitude.to_num::<i32>())
            });
            let object =
                rules.and_then(|rules| rules.object(self.interner.resolve(entity.type_ref())));
            let (trig, _) = required_math_tables();
            let body = entity
                .body_facing
                .as_ref()
                .map_or(u16::from(entity.facing) << 8, |body| body.current(frame));
            let mut host = CruiseHost {
                frame,
                trig,
                atan: required_atan_table(),
                terrain,
                occupancy: &self.substrate.occupancy,
                entities: &self.substrate.entities,
                rules,
                interner: &self.interner,
                kind: match entity.category {
                    EntityCategory::Unit => FlightOwnerKind::Unit,
                    EntityCategory::Infantry => FlightOwnerKind::Infantry,
                    _ => FlightOwnerKind::Other,
                },
                location: [xy[0], xy[1], z],
                on_bridge: entity.on_bridge,
                balloon_hover: locomotor.balloon_hover,
                has_target: entity.attack_target.is_some(),
                piggyback_active: entity.foot_locomotor_swap_active,
                simple_deployer: object.is_some_and(|object| object.is_simple_deployer),
                deploy_to_land: object.is_some_and(|object| object.deploy_to_land),
                body_facing: Some(body),
                grounded_reset: false,
                stable_id,
                air_slots: &self.substrate.air_slots,
                raw_occupation: &self.substrate.raw_cell_occupation,
                path_grid: path_grid.as_deref(),
                speed_type: locomotor.speed_type,
                movement_zone: locomotor.movement_zone,
                rng: &mut self.scenario_rng,
                slot_ops: Vec::new(),
                scatter_to: None,
                stop_requested: false,
                landing_latched: runtime.landing_latched,
                touched_down: false,
                crashing: entity.crashing,
                mission_enter: entity.mission.queued().raw() == 7
                    || entity.mission.effective().raw() == 7,
                map_size,
                crash_relocated: false,
                impact: false,
                speed_fraction: None,
            };

            let params = runtime.params;
            let mut flight = runtime.flight;
            let entry_state = state;
            let state = jumpjet_flight::process(
                moving,
                state,
                destination,
                &params,
                &mut flight,
                &mut host,
            );
            let effects = HostEffects {
                moving,
                destination,
                entry_state,
                body_facing: host.body_facing,
                grounded_reset: host.grounded_reset,
                height: host.height_above_ground(),
                slot_ops: host.slot_ops,
                scatter_to: host.scatter_to,
                stop_requested: host.stop_requested,
                landing_latched: host.landing_latched,
                touched_down: host.touched_down,
                crash_relocated: host.crash_relocated,
                impact: host.impact,
                speed_fraction: host.speed_fraction,
            };
            (state, flight, host.location, effects)
        };

        for (cell, owner) in &effects.slot_ops {
            match owner {
                Some(owner) => {
                    self.substrate.air_slots.claim(cell.0, cell.1, *owner);
                }
                None => self.substrate.air_slots.release(cell.0, cell.1),
            }
        }

        if effects.crash_relocated {
            // State 5's Mark(REMOVE) before its SetLocation. Mark leaves the
            // AircraftTracker alone; the impact removes the wreck from it.
            self.unmark_entity_remove_keeping_air_tracker(stable_id);
        }
        let entity = self.substrate.entities.get_mut(stable_id)?;
        commit_world_location(&mut entity.position, location);
        // `FootClass::SetSpeedFraction @ 0x004D3710` on the owner, which the
        // Infantry fire error (`0x0051C9B8`), its locomotion action AI and
        // GetCurrentSpeed's shot lead read.
        if let Some(bits) = effects.speed_fraction {
            entity.foot_speed.set_speed_fraction_native_bits(bits);
        }
        if effects.grounded_reset {
            entity.on_bridge = false;
        }
        if let Some(facing) = effects.body_facing {
            entity.facing = (facing >> 8) as u8;
            if let Some(body) = entity.body_facing.as_mut() {
                body.snap(facing, frame);
            }
        }
        if effects.crash_relocated {
            // Mark(PUT) and the display resubmission after it.
            self.add_entity_occupancy(stable_id);
            self.submit_entity_display(stable_id, rules, None);
        }

        // Jumpjet54C8F0 calls PerCell(2) only at accepted touchdown, before
        // clearing the destination. Cruise coordinate changes do not do this.
        if effects.touched_down {
            self.foot_neighbors_at_per_cell(stable_id);
        }
        let entity = self.substrate.entities.get_mut(stable_id)?;
        let mut moving = effects.moving;
        // `Set_Destination` after a scatter re-aims the owner at the neighbour.
        if let Some(cell) = effects.scatter_to
            && cell.0 >= 0
            && cell.1 >= 0
        {
            let target = (cell.0 as u16, cell.1 as u16);
            entity.movement_target = Some(crate::sim::components::MovementTarget {
                path: vec![target],
                path_layers: vec![MovementLayer::Air],
                next_index: 0,
                final_goal: Some(target),
                ..Default::default()
            });
            moving = true;
        }
        // A cruise that leaves State 3 for the hold or the descent has reached
        // the ordered cell: the order is done, and what follows needs no goal.
        let ended_cruise =
            effects.entry_state == STATE_TRANSLATE && matches!(state, STATE_HOLD | STATE_DESCEND);
        if effects.touched_down || ended_cruise {
            entity.movement_target = None;
        }
        if effects.touched_down {
            moving = false;
            // `0x0054CA12`: touchdown ends a crash the latch never caught.
            entity.crashing = false;
        }
        let arrived = effects.touched_down || ended_cruise;

        let locomotor = entity.locomotor.as_mut()?;
        locomotor.altitude = SimFixed::from_num(effects.height.max(0));
        if let Some(runtime) = locomotor.jumpjet_runtime_mut() {
            runtime.flight = flight;
            runtime.phase = state;
            runtime.moving = moving;
            runtime.landing_latched = effects.landing_latched && !effects.touched_down;
            runtime.destination = if moving {
                crate::sim::components::DriveCoord {
                    x: effects.destination[0],
                    y: effects.destination[1],
                    z: effects.destination[2],
                }
            } else {
                JumpjetRuntime::NULL
            };
        }
        if effects.stop_requested {
            // State 4 refused the landing and called `Stop_Moving` as its last
            // act, so it runs on the committed frame. Its re-target is the
            // order the adapter follows from here.
            let speed = self.jumpjet_order_speed(stable_id, rules);
            self.jumpjet_stop_moving(stable_id, rules, None);
            self.publish_jumpjet_destination(stable_id, speed);
        }
        Some(AirMovementTickStats {
            air_movers: 1,
            arrivals: u32::from(arrived),
            impact: effects.impact,
        })
    }

    /// VERA's movement adapter hands a Jumpjet its orders through a goal cell
    /// rather than through Foot's `Set_Destination`, so before `Process` the
    /// two native entries are applied from it:
    /// - a goal the locomotor is not flying to is a fresh
    ///   `Set_Destination(cell)`: Foot stores the NavCom and hands `Move_To`
    ///   the cell's `GetCoords` (`0x004D94B0`), and the goal then names the
    ///   cell `Move_To` chose;
    /// - a goal dropped while the owner climbs or cruises with its NavCom
    ///   still set (an Attack order and the attack approach drop only the
    ///   goal) is a null `Set_Destination`, whose Foot arm runs `Stop_Moving`
    ///   ([`Simulation::jumpjet_null_destination`]) and clears the NavCom, so
    ///   it is applied once.
    ///
    /// A wreck takes no orders.
    fn apply_jumpjet_adapter_order(&mut self, id: u64, rules: Option<&RuleSet>) {
        enum Order {
            MoveTo((u16, u16)),
            Stop,
        }
        let order = {
            let Some(entity) = self.substrate.entities.get(id) else {
                return;
            };
            let Some(runtime) = entity
                .locomotor
                .as_ref()
                .and_then(|locomotor| locomotor.jumpjet_runtime())
            else {
                return;
            };
            if entity.crashing {
                return;
            }
            match entity.movement_target.as_ref().and_then(|t| t.final_goal) {
                Some(goal) => {
                    let flying_to = (
                        native_cell(runtime.destination.x),
                        native_cell(runtime.destination.y),
                    );
                    if runtime.moving
                        && runtime.destination != JumpjetRuntime::NULL
                        && flying_to == (goal.0 as i16, goal.1 as i16)
                    {
                        return;
                    }
                    Order::MoveTo(goal)
                }
                None if runtime.moving
                    && matches!(runtime.phase, STATE_ASCEND | STATE_TRANSLATE)
                    && entity.navigation.nav_com.is_some() =>
                {
                    Order::Stop
                }
                None => return,
            }
        };
        match order {
            Order::MoveTo(goal) => {
                let speed = self.jumpjet_order_speed(id, rules);
                let request = crate::sim::movement::target_cell_coord(
                    goal.0,
                    goal.1,
                    self.resolved_terrain.as_ref(),
                );
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.navigation.nav_com =
                        Some(crate::sim::components::NavTargetRef::cell(goal.0, goal.1));
                    entity.navigation.nav_com_aux = None;
                }
                self.jumpjet_move_to(id, request, rules);
                let moving = self.substrate.entities.get(id).is_some_and(|entity| {
                    entity
                        .locomotor
                        .as_ref()
                        .and_then(|locomotor| locomotor.jumpjet_runtime())
                        .is_some_and(|runtime| runtime.moving)
                });
                if moving {
                    self.publish_jumpjet_destination(id, speed);
                } else if let Some(entity) = self.substrate.entities.get_mut(id) {
                    // A refused Move_To leaves the owner where it was; the goal
                    // retires instead of being retried every frame.
                    entity.movement_target = None;
                }
            }
            Order::Stop => {
                self.jumpjet_null_destination(id, rules, None);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    crate::sim::movement::DestinationTiming::from_rules(
                        self.session.binary_frame,
                        rules,
                    )
                    .accept(entity);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::jumpjet_params::JumpjetParams;
    use crate::sim::components::MovementTarget;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::ground_pose::position_world_coord;
    use crate::sim::movement::locomotor::LocomotorState;
    use serde_json::Value;

    fn retail_tables_present() -> bool {
        let (trig, _) = required_math_tables();
        if !trig.matches_retail() || !required_atan_table().matches_retail() {
            // With RA2_DIR set, a mismatched table is a failure, not a skip.
            assert!(
                std::env::var_os("RA2_DIR").is_none(),
                "RA2_DIR is set but the retail sine or atan table does not match"
            );
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return false;
        }
        true
    }

    fn native_row(name: &str) -> Value {
        let rows: Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/jumpjet_flight.json"
        ))
        .expect("corpus parses");
        rows.as_array()
            .expect("rows")
            .iter()
            .find(|row| row["name"] == name)
            .cloned()
            .expect("named row")
    }

    /// A Unit Jumpjet with the corpus BASE type block, fresh from `link`
    /// (state 0, locomotor facing `0x4000`), hovering at 500 at cell (10,10)
    /// with a move to cell (16,10). The terrain is the oracle fixture: level 0
    /// and flat, except cell (9,10) at level 2 with slope 1.
    fn hovering_jumpjet(body_facing: u8) -> Simulation {
        let mut sim = Simulation::new();
        super::super::lifecycle_tests::install_common_raw_terrain(&mut sim, 24, 16, 0, None);
        // A playfield holding the corpus corridor (x 6..20, y 9..11), for the
        // orders' searches and `In_Bounds`.
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 12,
            off_fc: 0,
            off_100: 0,
            off_104: 16,
            off_108: 16,
        });
        sim.playfield_size_height = Some(16);
        {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .and_then(|terrain| terrain.cell_mut(9, 10))
                .expect("fixture cell");
            cell.level = 2;
            cell.slope_type = 1;
        }
        let mut entity = GameEntity::test_default(1, "JUMPJETUNIT", "Americans", 10, 10);
        entity.category = EntityCategory::Unit;
        entity.facing = body_facing;
        entity.position.sub_x = SimFixed::from_num(128);
        entity.position.sub_y = SimFixed::from_num(128);
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
        locomotor
            .jumpjet_runtime_mut()
            .expect("jumpjet runtime")
            .link(&JumpjetParams {
                turn_rate: 4,
                speed: SimFixed::from_num(14),
                climb: 5.0,
                crash: 5.0,
                height: 500,
                accel: 2.0,
                wobbles: 0.15,
                deviation: 40,
                no_wobbles: false,
            });
        {
            let runtime = locomotor.jumpjet_runtime_mut().expect("jumpjet runtime");
            // The cruise rows begin mid-flight, which is what `Move_To` leaves
            // behind: State 3 with the moving byte set, and State 0's seed
            // already applied (locomotor facing snapped to the body, both speed
            // doubles and the bob zero, `+0x80` at `JumpjetHeight=`).
            runtime.phase = STATE_TRANSLATE;
            runtime.moving = true;
            runtime.flight.facing.snap(u16::from(body_facing) << 8, 0);
            runtime.flight.target_height = 500;
        }
        locomotor.altitude = SimFixed::from_num(500);

        entity.locomotor = Some(locomotor);
        entity.movement_target = Some(MovementTarget {
            path: vec![(16, 10)],
            path_layers: vec![MovementLayer::Air],
            next_index: 0,
            speed: SimFixed::from_num(14),
            final_goal: Some((16, 10)),
            ..Default::default()
        });
        sim.substrate.entities.insert(entity);
        sim
    }

    /// Park a Jumpjet with no order in a given native state and run the tick.
    fn idle_for(state: i32, balloon_hover: bool, frames: u32) -> Simulation {
        let mut sim = hovering_jumpjet(0x40);
        {
            let entity = sim.substrate.entities.get_mut(1).expect("jumpjet");
            entity.movement_target = None;
            let locomotor = entity.locomotor.as_mut().expect("locomotor");
            locomotor.balloon_hover = balloon_hover;
            locomotor.altitude = SimFixed::from_num(if state == jumpjet_flight::STATE_GROUND {
                0
            } else {
                500
            });
            let runtime = locomotor.jumpjet_runtime_mut().expect("runtime");
            runtime.phase = state;
            runtime.moving = false;
            runtime.destination = JumpjetRuntime::NULL;
        }
        for frame in 0..frames {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(1, None);
        }
        sim
    }

    /// Before the locomotor owned this tick, VERA's air adapter cycled an idle
    /// non-balloon Jumpjet Ascending -> Hovering -> Descending -> Landed every
    /// 102 frames. `Process 0x0054AEC0` runs Update only while the moving byte
    /// is set or the state is not ground/hold, and State 0 leaves the ground
    /// only when moving, so an idle landed owner does nothing at all.
    #[test]
    fn an_idle_landed_jumpjet_never_leaves_the_ground() {
        let sim = idle_for(jumpjet_flight::STATE_GROUND, false, 400);
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        let locomotor = entity.locomotor.as_ref().expect("locomotor");
        assert_eq!(locomotor.altitude, SimFixed::from_num(0));
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase),
            Some(jumpjet_flight::STATE_GROUND)
        );
        assert_eq!((entity.position.rx, entity.position.ry), (10, 10));
    }

    /// Re-opening a cruise must not leave a claim behind on the cell the owner
    /// drifted out of.
    ///
    /// Native State 2 releases the owner's cached cell (`+0x560`) at
    /// `0x0054BF75..0x0054BFA8` before releasing the current one. The drift is
    /// real: State 1 sets the target speed before promoting to the hold, so the
    /// owner leaves the cell it claimed — the native corpus shows a claim on
    /// one cell and a release on another a frame later. VERA keeps no cached
    /// cell, so it drops every slot the owner still holds. Without this the
    /// orphaned claim would block that cell against every later hoverer and
    /// grow the hashed, snapshotted grid without bound.
    ///
    /// The corpus cannot witness this: its declared substitution pins `+0x560`,
    /// so the original skips the cached-cell release there. Hence a regression
    /// test rather than a parity row.
    #[test]
    fn re_opening_a_cruise_drops_a_drifted_slot() {
        let mut sim = hovering_jumpjet(0x40);
        // The owner holds (13,10) but sits at (10,10) — the drift case.
        assert!(sim.substrate.air_slots.claim(13, 10, 1));
        {
            let locomotor = sim
                .substrate
                .entities
                .get_mut(1)
                .and_then(|entity| entity.locomotor.as_mut())
                .expect("locomotor");
            let runtime = locomotor.jumpjet_runtime_mut().expect("runtime");
            runtime.phase = STATE_HOLD;
            runtime.moving = true;
        }
        sim.session.binary_frame = 1001;
        sim.tick_air_movement_with_cell_lists_one(1, None);

        assert_eq!(
            sim.substrate.air_slots.holder(13, 10),
            None,
            "the claim on the cell the owner drifted out of must not leak"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .and_then(|entity| entity.locomotor.as_ref())
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .map(|runtime| runtime.phase),
            Some(STATE_TRANSLATE),
            "a destination elsewhere re-opens the cruise"
        );
    }

    /// `Is_Moving_Now 0x0054D0D0` answers false for the hold too, so a parked
    /// balloon with no order neither bobs nor drifts.
    #[test]
    fn an_idle_hold_without_a_move_is_inert() {
        let sim = idle_for(STATE_HOLD, true, 400);
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        let locomotor = entity.locomotor.as_ref().expect("locomotor");
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase),
            Some(STATE_HOLD)
        );
        assert_eq!((entity.position.rx, entity.position.ry), (10, 10));
    }

    /// Fly a native row through the production air tick, comparing the owner
    /// position and exact Z every frame, until arrival hands it to descent.
    fn fly_native_row(name: &str, body_facing: u8) {
        let row = native_row(name);
        let frames = row["output"]["frames"].as_array().expect("frames");
        let mut sim = hovering_jumpjet(body_facing);
        for (index, expected) in frames.iter().enumerate() {
            sim.session.binary_frame = 1001 + index as u32;
            let stats = sim.tick_air_movement_with_cell_lists_one(1, None);
            let entity = sim.substrate.entities.get(1).expect("jumpjet");
            let coord = position_world_coord(&entity.position);
            let native: Vec<i64> = expected["coord"]
                .as_array()
                .expect("coord")
                .iter()
                .map(|value| value.as_i64().expect("int"))
                .collect();
            assert_eq!(
                vec![i64::from(coord.x), i64::from(coord.y), i64::from(coord.z)],
                native,
                "{name}: frame {index}"
            );
            assert_eq!(
                u32::from(entity.facing),
                (expected["body_facing"].as_u64().expect("body facing") >> 8) as u32,
                "{name}: body facing, frame {index}"
            );
            let last = index + 1 == frames.len();
            assert_eq!(stats.arrivals, u32::from(last), "{name}: frame {index}");
        }
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert!(entity.movement_target.is_none());
        let locomotor = entity.locomotor.as_ref().expect("locomotor");
        assert_eq!(
            locomotor.jumpjet_runtime().map(|runtime| runtime.phase),
            Some(STATE_DESCEND)
        );
    }

    /// Production parity for the native `east_cruise` row, owner already facing
    /// east.
    #[test]
    fn production_cruise_flies_the_native_east_cruise_frames() {
        if retail_tables_present() {
            fly_native_row("east_cruise", 0x40);
        }
    }

    /// Production parity for `east_from_west_facing`: the takeoff seed snaps the
    /// locomotor facing to the body facing (west), so the cruise turns around
    /// from there instead of starting from the linked `0x4000`.
    #[test]
    fn production_cruise_turns_from_the_body_facing() {
        if retail_tables_present() {
            fly_native_row("east_from_west_facing", 0xC0);
        }
    }

    /// An order dropped mid-cruise (an Attack order, the attack approach at
    /// range) is Foot's null `Set_Destination`: `Stop_Moving` re-targets the
    /// cell under the owner and keeps the moving byte, so it flies on to that
    /// cell's centre and lands there, and the NavCom its `Move_To` wrote is
    /// cleared again. It is applied once: the next frame finds no NavCom.
    #[test]
    fn a_dropped_order_stops_at_the_cell_under_the_owner() {
        let mut sim = hovering_jumpjet(0x40);
        for frame in 0..20 {
            sim.session.binary_frame = 1001 + frame;
            sim.tick_air_movement_with_cell_lists_one(1, None);
        }
        let runtime = |sim: &Simulation| {
            sim.substrate
                .entities
                .get(1)
                .and_then(|entity| entity.locomotor.as_ref())
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .cloned()
                .expect("runtime")
        };
        let flying = runtime(&sim);
        assert_eq!(flying.phase, STATE_TRANSLATE);
        assert_eq!(
            flying.destination,
            crate::sim::components::DriveCoord {
                x: 16 * 256 + 128,
                y: 10 * 256 + 128,
                z: 0
            },
            "the order's Move_To placed the owner at the ordered cell's floor"
        );
        let entity = sim.substrate.entities.get_mut(1).expect("jumpjet");
        assert!(entity.navigation.nav_com.is_some());
        let here = (entity.position.rx, entity.position.ry);
        assert!(here.0 < 16, "still on its way");
        entity.movement_target = None;

        sim.session.binary_frame = 1021;
        sim.tick_air_movement_with_cell_lists_one(1, None);
        let stopped = runtime(&sim);
        assert!(stopped.moving, "Stop_Moving keeps the moving byte");
        assert_eq!(
            stopped.destination,
            crate::sim::components::DriveCoord {
                x: i32::from(here.0) * 256 + 128,
                y: i32::from(here.1) * 256 + 128,
                z: 0
            }
        );
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert!(entity.navigation.nav_com.is_none());
        assert!(entity.movement_target.is_none());

        for frame in 0..400 {
            sim.session.binary_frame = 1022 + frame;
            sim.tick_air_movement_with_cell_lists_one(1, None);
        }
        let landed = runtime(&sim);
        assert_eq!(landed.phase, jumpjet_flight::STATE_GROUND);
        assert!(!landed.moving);
        let entity = sim.substrate.entities.get(1).expect("jumpjet");
        assert_eq!((entity.position.rx, entity.position.ry), here);
        assert_eq!(entity.position.exact_z_leptons, Some(0));
    }

    /// `g_HeightFactor`, read from the native startup chain, is the multiplier
    /// the cell top height applies to a building's art `Height=`.
    #[test]
    fn building_height_factor_matches_the_native_startup_value() {
        let native: Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/height_factor.json"
        ))
        .expect("height factor parses");
        assert_eq!(
            native["height_factor"].as_i64(),
            Some(i64::from(GROUND_LEVEL_HEIGHT_LEPTONS))
        );
    }
}
