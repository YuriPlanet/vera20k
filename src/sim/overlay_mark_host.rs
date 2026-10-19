//! The live Simulation as the CellClass surface of the one
//! `OverlayClass::Mark @ 0x005FC570` port ([`crate::map::overlay_mark`]):
//! the runtime constructor callers (crate validation, meteor ore) and
//! Building's wall arm. Writes go through the OverlayGrid owner and project
//! into the terrain bridge cache; Recalc is the grid's runtime Recalc.
//!
//! The bridge rebuild and the batched ore placement host the same port on
//! their own borrows. The CellAnim constructor here serves every host that
//! reaches it.
//!
//! ## Dependency rules
//! Part of `sim/` — depends on `rules/`, `map/` and other `sim/` modules only.

use std::convert::Infallible;

use crate::map::bridge_facts::{BridgeFlagStamp, BridgeStampFamily, BridgeStampSlot};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::lighting::LightingProfileUnits;
use crate::map::overlay_mark::{OverlayCellAnim, OverlayMarkHost};
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::{AnimConstructorInput, AnimId, AnimSpawnError, AnimWorldCoord};
use crate::sim::cell_rect::{CellPassabilityArgs, canonical_cell_ref, check_cell_passability};
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::sim::world::{Simulation, SimulationWallRuntimeHost};
use crate::util::fixed_math::SimFixed;

/// [`OverlayMarkHost`] over the live Simulation. Callers admit a cell of the
/// CellClass array before revealing, so the terrain is present.
pub(crate) struct SimulationMarkHost<'a> {
    sim: &'a mut Simulation,
    /// `None` only for focused crate fixtures that bind CrateRules alone:
    /// they refuse walls and construct no CellAnim.
    rules: Option<&'a RuleSet>,
    registry: &'a OverlayTypeRegistry,
    lighting_profile: LightingProfileUnits,
}

impl<'a> SimulationMarkHost<'a> {
    pub(crate) fn new(
        sim: &'a mut Simulation,
        rules: Option<&'a RuleSet>,
        registry: &'a OverlayTypeRegistry,
        lighting_profile: LightingProfileUnits,
    ) -> Self {
        Self {
            sim,
            rules,
            registry,
            lighting_profile,
        }
    }

    fn terrain(&self) -> &ResolvedTerrainGrid {
        self.sim
            .resolved_terrain
            .as_ref()
            .expect("runtime OverlayClass::Mark reveals on the live CellClass array")
    }

    /// The OverlayGrid coordinate of a real cell.
    fn real_coord(&self, cell: NativeCellIdentity) -> Option<(u16, u16)> {
        match cell {
            NativeCellIdentity::Real(_) => {
                let (x, y) = self.terrain().native_cell_coord(cell);
                Some((x as u16, y as u16))
            }
            NativeCellIdentity::Dummy => None,
        }
    }

    /// The borrow-split wall observer over this Simulation's live owners.
    fn with_wall_host<R>(
        &mut self,
        apply: impl FnOnce(
            &mut crate::sim::overlay_grid::OverlayGrid,
            &mut ResolvedTerrainGrid,
            &mut SimulationWallRuntimeHost<'_>,
        ) -> R,
    ) -> Option<R> {
        let sim = &mut *self.sim;
        #[cfg(any(test, feature = "test-support"))]
        let mut detach_trace = Vec::new();
        let mut host = SimulationWallRuntimeHost {
            entities: &mut sim.substrate.entities,
            target_terrain_objects: &sim.production.terrain_objects,
            #[cfg(any(test, feature = "test-support"))]
            detach_trace: &mut detach_trace,
            radar_dirty_cells: &mut sim.radar_terrain_dirty_cells,
            radar_dirty_generation: &mut sim.radar_terrain_dirty_generation,
            tactical_dirty_cells: &mut sim.tactical_dirty_cells,
            zone_grid: &mut sim.zone_grid,
            path_grid: &mut sim.path_grid,
            bridge_state: sim.bridge_state.as_ref(),
            playfield_bounds: sim.playfield_bounds,
        };
        let (Some(grid), Some(terrain)) =
            (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
        else {
            return None;
        };
        Some(apply(grid, terrain, &mut host))
    }
}

impl OverlayMarkHost for SimulationMarkHost<'_> {
    type Error = Infallible;

    fn cell(&mut self, coord: (i16, i16)) -> NativeCellIdentity {
        self.terrain().native_cell_identity(coord)
    }

    fn coord(&self, cell: NativeCellIdentity) -> (i16, i16) {
        self.terrain().native_cell_coord(cell)
    }

    fn in_bounds(&mut self, coord: (i16, i16)) -> Result<bool, Infallible> {
        Ok(self.sim.map_cell_in_bounds(coord))
    }

    fn slope(&self, cell: NativeCellIdentity) -> u8 {
        self.terrain().native_cell_ground_fields(cell).1
    }

    fn identity(&self, cell: NativeCellIdentity) -> i32 {
        match self.real_coord(cell) {
            Some((x, y)) => self
                .sim
                .overlay_grid
                .as_ref()
                .and_then(|grid| grid.cell(x, y).overlay_id)
                .map_or(crate::map::overlay_mark::NO_OVERLAY, i32::from),
            None => {
                self.terrain()
                    .shared_cell_dummy()
                    .overlay_identity_state()
                    .0
            }
        }
    }

    fn data(&self, cell: NativeCellIdentity) -> u8 {
        match self.real_coord(cell) {
            Some((x, y)) => self
                .sim
                .overlay_grid
                .as_ref()
                .map_or(0, |grid| grid.cell(x, y).overlay_data),
            None => {
                self.terrain()
                    .shared_cell_dummy()
                    .overlay_identity_state()
                    .1
            }
        }
    }

    fn write_identity(
        &mut self,
        cell: NativeCellIdentity,
        overlay_id: u8,
    ) -> Result<(), Infallible> {
        match self.real_coord(cell) {
            Some((x, y)) => {
                if let (Some(grid), Some(terrain)) = (
                    self.sim.overlay_grid.as_mut(),
                    self.sim.resolved_terrain.as_mut(),
                ) {
                    grid.write_mark_identity(Some(terrain), self.registry, x, y, overlay_id);
                }
            }
            None => self
                .terrain()
                .shared_cell_dummy()
                .write_overlay_identity(i32::from(overlay_id)),
        }
        Ok(())
    }

    fn write_data(&mut self, cell: NativeCellIdentity, data: u8) -> Result<(), Infallible> {
        match self.real_coord(cell) {
            Some((x, y)) => {
                if let Some(grid) = self.sim.overlay_grid.as_mut() {
                    grid.write_cell_data(self.sim.resolved_terrain.as_mut(), x, y, data);
                }
            }
            None => self.terrain().shared_cell_dummy().write_overlay_state(data),
        }
        Ok(())
    }

    fn scenario_init(&self) -> bool {
        self.sim.object_placement_scope_active()
    }

    fn next_scenario_raw(&mut self) -> u32 {
        self.sim.scenario_rng.next_u32()
    }

    fn passable(
        &mut self,
        cell: NativeCellIdentity,
        speed: SpeedType,
        ignore_occupants: bool,
    ) -> Result<bool, Infallible> {
        Ok(mark_cell_passable(self.sim, cell, speed, ignore_occupants))
    }

    fn set_bridge_direction(
        &mut self,
        anchor: NativeCellIdentity,
        family: BridgeStampFamily,
        direction: u8,
    ) -> Result<(), Infallible> {
        let (x, y) = self.coord(anchor);
        let stamp = BridgeFlagStamp::new((x as u16, y as u16), direction, true);
        let Some(slots) = stamp.slots() else {
            return Ok(());
        };
        // `0x0047E040`/`0x0047E470` store the data byte on these four visits,
        // with its radar-dirty event, before the flag transaction; a miss
        // writes the shared dummy, which ends on the setter's final visit.
        let data = if direction == 0 { 0 } else { 9 };
        for (slot, coord) in slots {
            if !matches!(
                slot,
                BridgeStampSlot::Anchor
                    | BridgeStampSlot::Forward1
                    | BridgeStampSlot::Forward2
                    | BridgeStampSlot::Opposite
            ) {
                continue;
            }
            let Some((x, y)) = coord else {
                continue;
            };
            let target = self.cell((x as i16, y as i16));
            match self.real_coord(target) {
                Some((rx, ry)) => {
                    if let (Some(grid), Some(terrain)) = (
                        self.sim.overlay_grid.as_mut(),
                        self.sim.resolved_terrain.as_mut(),
                    ) {
                        grid.write_crate_mark_data_field(terrain, rx, ry, data);
                    }
                }
                None => self.terrain().shared_cell_dummy().write_overlay_state(data),
            }
        }
        self.sim.apply_runtime_bridge_mark_stamp(stamp, family);
        Ok(())
    }

    fn wall_clear_to_build(&mut self, anchor: NativeCellIdentity) -> Result<bool, Infallible> {
        let Some(rules) = self.rules else {
            return Ok(false);
        };
        let cells = NativeCellQuery::canonical(self.terrain());
        Ok(crate::sim::build_site::is_clear_to_build(
            self.sim,
            rules,
            Some(self.registry),
            anchor,
            Some(SpeedType::Track),
            None,
            None,
            Some(&cells),
        ))
    }

    fn wall_cleanup(&mut self, anchor: NativeCellIdentity) -> Result<(), Infallible> {
        let (x, y) = self.coord(anchor);
        let registry = self.registry;
        self.with_wall_host(|grid, terrain, host| {
            crate::sim::overlay_grid::refresh_wall_connectivity_after_placement_with_host(
                grid,
                registry,
                Some(terrain),
                x as u16,
                y as u16,
                Some(host as &mut dyn crate::sim::overlay_grid::WallDamageTransactionHost),
            );
        });
        Ok(())
    }

    fn merge_wall_zones(&mut self, anchor: NativeCellIdentity) -> Result<(), Infallible> {
        let (x, y) = self.coord(anchor);
        self.with_wall_host(|_, terrain, host| {
            use crate::sim::overlay_grid::WallDamageTransactionHost;
            host.navigation_step(
                terrain,
                (x as u16, y as u16),
                false,
                ZoneRepairKind::MergeAdjacent,
            );
        });
        Ok(())
    }

    fn increment_wall_neighbor_count(
        &mut self,
        cell: NativeCellIdentity,
    ) -> Result<(), Infallible> {
        if let (Some(grid), Some(terrain)) = (
            self.sim.overlay_grid.as_mut(),
            self.sim.resolved_terrain.as_ref(),
        ) {
            grid.increment_retained_wall_neighbor_count(terrain, cell);
        }
        Ok(())
    }

    fn spawn_cell_anim(&mut self, request: OverlayCellAnim<'_>) -> Result<(), Infallible> {
        let Some(rules) = self.rules else {
            return Ok(());
        };
        let ground = OverlayCellAnimGround::read(self.terrain(), &request);
        if let Err(error) = self.sim.construct_overlay_cell_anim(
            rules,
            request,
            ground,
            self.lighting_profile,
            AnimConstructorInput::Runtime,
        ) {
            panic!(
                "Overlay CellAnim [{}] must be bound before OverlayClass::Mark: {error}",
                request.anim
            );
        }
        Ok(())
    }

    fn recalc(&mut self, cell: NativeCellIdentity) -> Result<(), Infallible> {
        // The shared dummy's derived attributes are not modeled.
        if let Some((x, y)) = self.real_coord(cell)
            && let (Some(grid), Some(terrain)) = (
                self.sim.overlay_grid.as_mut(),
                self.sim.resolved_terrain.as_mut(),
            )
        {
            grid.recalculate_runtime_cell(terrain, self.registry, (x, y));
        }
        Ok(())
    }
}

/// `OverlayClass::Mark`'s `CellClass::CheckCellPassability @ 0x004834A0`
/// on a live Simulation cell.
pub(crate) fn mark_cell_passable(
    sim: &Simulation,
    cell: NativeCellIdentity,
    speed: SpeedType,
    ignore_occupants: bool,
) -> bool {
    let terrain = sim
        .resolved_terrain
        .as_ref()
        .expect("runtime OverlayClass::Mark reveals on the live CellClass array");
    check_cell_passability(
        &canonical_cell_ref(terrain, cell),
        mark_passability_args(speed, ignore_occupants),
        &sim.cell_passability_map(),
    )
    .admits()
}

/// `OverlayClass::Mark`'s arguments to `CellClass::CheckCellPassability @
/// 0x004834A0`: `(speed, ignore, ignore, -1, 0, -1, 1)`. The water crate pushes
/// them at `0x005FD001..0x005FD008` and ordinary admission at
/// `0x005FD03D..0x005FD049`; both share the `CALL` at `0x005FD00C` (`JNZ` at
/// `0x005FD04D`), except TIB01, whose own `CALL` at `0x005FD04F` precedes the
/// slope recheck. The other crate images call at `0x005FD070`.
pub(crate) fn mark_passability_args(
    speed: SpeedType,
    ignore_occupants: bool,
) -> CellPassabilityArgs {
    CellPassabilityArgs {
        speed_type: speed,
        ignore_infantry: ignore_occupants,
        ignore_vehicles: ignore_occupants,
        zone: None,
        movement_zone: MovementZone::Normal,
        level: None,
        is_bridge: true,
    }
}

/// The CellClass inputs of an Overlay's CellAnim, read from the CellClass
/// array its Mark ran on: `MapClass @ 0x00578080`'s ground height at the
/// Overlay's Location and the Mark cell's level, from which its `+0x10A`
/// ground adjustment derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverlayCellAnimGround {
    z: i32,
    level: u8,
}

impl OverlayCellAnimGround {
    pub(crate) fn read(terrain: &ResolvedTerrainGrid, request: &OverlayCellAnim<'_>) -> Self {
        let z = crate::sim::movement::ground_pose::query_ground_height(
            &NativeCellQuery::canonical(terrain),
            crate::sim::components::DriveCoord {
                x: request.location[0],
                y: request.location[1],
                z: request.location[2],
            },
        )
        .unwrap_or(0);
        Self {
            z,
            level: terrain.native_cell_ground_fields(request.cell).0,
        }
    }
}

impl Simulation {
    /// `OverlayClass::Mark @ 0x005FD124..0x005FD1F4`: construct the CellAnim
    /// at the Overlay's Location plus `0x180` on the ground height
    /// `MapClass @ 0x00578080` reports at the Location, with `(0, 1, 0x600,
    /// 0, 0)`. Only a cell that resolved a TiberiumClass enters the tint
    /// block: that class's Color scheme into `+0xD4` and the cell's `+0x10A`
    /// ground adjustment into `+0xFC`. Otherwise the constructor's values stay.
    pub(crate) fn construct_overlay_cell_anim(
        &mut self,
        rules: &RuleSet,
        request: OverlayCellAnim<'_>,
        ground: OverlayCellAnimGround,
        lighting_profile: LightingProfileUnits,
        constructor: AnimConstructorInput,
    ) -> Result<AnimId, AnimSpawnError> {
        let [x, y] = request.anim_xy;
        let ground_z = ground.z;
        let type_name = self.interner.intern(request.anim);
        let mut descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
            type_name,
            (x / 256) as u16,
            (y / 256) as u16,
            SimFixed::from_num(0),
            SimFixed::from_num(0),
            0,
        );
        descriptor.delay = 0;
        descriptor.loop_count = 1;
        descriptor.draw_flags = 0x600;
        descriptor.z_adjust = 0;
        descriptor.reverse = false;
        let anim_id = self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            AnimWorldCoord { x, y, z: ground_z },
            constructor,
        )?;
        let Some(tiberium) = request
            .tiberium
            .and_then(|type_id| rules.tiberium_types.get(type_id))
        else {
            return Ok(anim_id);
        };
        let remap_color = tiberium
            .color
            .as_deref()
            .and_then(|color| {
                crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, color)
            })
            .and_then(|entry| u8::try_from(entry).ok())
            .map(crate::rules::house_colors::HouseColorIndex);
        let z_adjust = crate::map::lighting::cell_ground_z_adjust(lighting_profile, ground.level);
        assert!(
            self.set_cell_anim_draw_authority(anim_id, remap_color, z_adjust),
            "Overlay CellAnim {anim_id} disappeared before OverlayClass post-constructor writes"
        );
        Ok(anim_id)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Simulation {
    /// The Mark body alone at a cell's center, outside any constructor or
    /// Unlimbo: fixtures that stamp an Overlay through the production owner.
    pub(crate) fn mark_overlay_for_test(
        &mut self,
        rules: &RuleSet,
        registry: &OverlayTypeRegistry,
        cell: (u16, u16),
        overlay_id: u8,
    ) -> crate::map::overlay_mark::OverlayMarkOutcome {
        use crate::map::overlay_mark::{MarkCrateImages, OverlayMarkRequest, mark_overlay};
        let request = OverlayMarkRequest {
            overlay_id,
            flags: registry
                .flags(overlay_id)
                .expect("a registered Overlay type"),
            cell: (cell.0 as i16, cell.1 as i16),
            location: [
                i32::from(cell.0).wrapping_mul(256).wrapping_add(128),
                i32::from(cell.1).wrapping_mul(256).wrapping_add(128),
                0,
            ],
            registry,
            tiberium_types: Some(&rules.tiberium_types),
            crate_images: MarkCrateImages::resolve(&rules.crate_rules, registry),
        };
        let lighting = self
            .session
            .lighting
            .profile(crate::sim::scenario_session::ScenarioLightingProfile::Normal)
            .into();
        match mark_overlay(
            &mut SimulationMarkHost::new(self, Some(rules), registry, lighting),
            &request,
        ) {
            Ok(outcome) => outcome,
            Err(never) => match never {},
        }
    }
}
