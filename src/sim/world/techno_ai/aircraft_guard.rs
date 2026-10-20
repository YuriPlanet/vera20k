//! World host of the aircraft Mission_Guard and Mission_AreaGuard
//! (`aircraft::guard_mission`): the dock search, the computer house's
//! bridge-deck target search (`HouseClass @ 0x00500300`) and the Foot bodies
//! both tail into.

use super::ObjectAiCtx;
use super::mission_handlers;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::guard_mission::{self, GuardFacts, GuardHost};
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement::air_movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

/// The cell-spread band `0x00500300` walks: its literal 0x171 entries
/// (`0x0050033A`), the count table's radius 11.
const BRIDGE_TARGET_SPREAD_BAND: usize = 11;

/// GetHeight below which a vehicle is no bridge target (`0x0050040B`,
/// `-0x15 < height`).
const BRIDGE_TARGET_MIN_HEIGHT: i32 = -20;

impl Simulation {
    /// `AircraftClass::Mission_Guard @ 0x0041A5C0` on aircraft `id`, for
    /// Guard and Sticky alike.
    pub(crate) fn aircraft_mission_guard(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: ObjectAiCtx<'_>,
    ) -> i32 {
        let facts = self.aircraft_guard_facts(id, rules);
        guard_mission::guard_visit(
            &facts,
            &mut WorldGuard {
                sim: self,
                id,
                rules,
                ctx,
            },
        )
    }

    /// `AircraftClass::Mission_AreaGuard @ 0x0041A940` on aircraft `id`.
    pub(crate) fn aircraft_mission_area_guard(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: ObjectAiCtx<'_>,
    ) -> i32 {
        let facts = self.aircraft_guard_facts(id, rules);
        guard_mission::area_guard_visit(
            &facts,
            &mut WorldGuard {
                sim: self,
                id,
                rules,
                ctx,
            },
        )
    }

    fn aircraft_guard_facts(&self, id: u64, rules: &RuleSet) -> GuardFacts {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let object = self.interner.object_type(entity.type_ref(), rules);
        GuardFacts {
            height: air_movement::current_fly_height(entity, self.resolved_terrain.as_ref()),
            flight_level: air_movement::type_flight_level(entity, Some((rules, &self.interner))),
            team: self.team_script_vm.team_for_member(id).is_some(),
            nav_com: entity.navigation.nav_com().is_some(),
            weapon: object.is_some_and(|object| {
                crate::sim::combat::combat_weapon::primary_for_tier(object, entity.veterancy.rank())
                    .is_some()
            }),
            armed: object
                .is_some_and(|object| crate::sim::combat::combat_weapon::is_armed(entity, object)),
            ammo: entity.ammo().map_or(-1, |ammo| ammo.current()),
            type_ammo: object.map_or(0, |object| object.ammo),
            human: self.owner_is_human(entity.owner()),
        }
    }

    /// `HouseClass @ 0x00500300` (Ghidra's "Find_Nearest_Ally_Building" is
    /// wrong) for `house` at `coord`: over the cell-spread table's 369 cells
    /// around the cell of `coord` (`/256` toward zero, 16-bit sums), the
    /// first vehicle on each cell's bridge-deck list (`CellClass::
    /// FindFirstUnit(1) @ 0x0047EBA0`, `+0xE8`), out of limbo, owned by a
    /// house neither `house` nor its ally, outside every zone of its own
    /// house ([`Self::house_outside_zones`]), at GetHeight above -21 and not
    /// cloaked (`+0x220 != 2`). The nearest by `ftol(Sqrt_Approx)` of the 3D
    /// distance between `coord` and its GetCoords wins, a later one on a
    /// strictly smaller distance or while the best distance is still 0.
    pub(crate) fn house_nearest_bridge_unit(
        &self,
        house: InternedId,
        coord: [i32; 3],
    ) -> Option<u64> {
        let center = ((coord[0] / 256) as i16, (coord[1] / 256) as i16);
        let mut best = None;
        let mut best_distance = 0;
        for &(dx, dy) in crate::sim::combat::cell_spread::sweep(BRIDGE_TARGET_SPREAD_BAND) {
            let (x, y) = (center.0.wrapping_add(dx), center.1.wrapping_add(dy));
            let Some(unit_id) = self.substrate.occupancy.first_category_on_layer(
                x as u16,
                y as u16,
                MovementLayer::Bridge,
                EntityCategory::Unit,
                &self.substrate.entities,
            ) else {
                continue;
            };
            let unit = self.substrate.entities.get(unit_id).expect("listed unit");
            if unit.lifecycle.in_limbo() {
                continue;
            }
            let owner = unit.owner();
            if crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(self.house_alliances.as_map()),
                &self.interner,
                house,
                owner,
            ) {
                continue;
            }
            let there = crate::sim::movement::ground_pose::object_get_coords(
                unit,
                self.resolved_terrain.as_ref(),
            );
            let there = [there.x, there.y, there.z];
            if !self.house_outside_zones(owner, there)
                || air_movement::current_fly_height(unit, self.resolved_terrain.as_ref())
                    < BRIDGE_TARGET_MIN_HEIGHT
                || unit
                    .cloak
                    .as_ref()
                    .is_some_and(|cloak| cloak.is_fully_cloaked())
            {
                continue;
            }
            let distance = crate::util::native_x87::distance_3d_leptons(coord, there);
            if best_distance == 0 || distance < best_distance {
                best = Some(unit_id);
                best_distance = distance;
            }
        }
        best
    }
}

/// A Mission_Guard or Mission_AreaGuard visit's world.
struct WorldGuard<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    ctx: ObjectAiCtx<'a>,
}

impl WorldGuard<'_> {
    fn entity(&self) -> &crate::sim::game_entity::GameEntity {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .expect("aircraft dispatch")
    }

    /// The mission `+0xAC` that `MissionClass::Get_MissionControl @ 0x005B3A00`
    /// indexes for the Rate.
    fn current(&self) -> MissionType {
        self.entity()
            .mission
            .current()
            .known()
            .expect("a dispatched mission")
    }
}

impl GuardHost for WorldGuard<'_> {
    /// `+0x6D4 = 1`, `Mission_Guard @ 0x0041A5C0`'s stores (`0x0041A605`, `0x0041A69F`).
    fn set_transition_ready(&mut self) {
        let entity = self.sim.substrate.entities.get_mut(self.id).unwrap();
        entity.mission_leaf.set_aircraft_transition_ready(1);
    }

    fn queue(&mut self, mission: MissionType) {
        crate::sim::aircraft::queue_mission(&mut self.sim.substrate, self.id, mission);
    }

    fn rate(&mut self) -> i32 {
        self.rules.mission_control.rate_frames(self.current())
    }

    /// `MapClass::Get_CellClass_At_Coord @ 0x00565730` on the Location
    /// (`0x0041A666`), the shared dummy cell off the map.
    fn assign_own_cell(&mut self) {
        let coord =
            crate::sim::movement::ground_pose::position_world_coord(&self.entity().position);
        let (x, y) = self.sim.resolved_terrain.as_ref().map_or_else(
            || crate::sim::movement::coord_cell(coord),
            |terrain| {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                cells.coord(cells.lookup_world(coord.x, coord.y))
            },
        );
        self.sim.assign_aircraft_destination(
            self.id,
            Some(NavTargetRef::cell(x as u16, y as u16)),
            self.rules,
            self.ctx.effects,
        );
    }

    fn enter_idle_mode(&mut self) {
        crate::sim::aircraft::enter_idle_mode_for(
            self.sim,
            self.id,
            self.rules,
            self.ctx.overlay_registry,
            self.ctx.effects,
        );
    }

    fn in_radio_contact(&mut self) -> bool {
        !self.entity().radio_contacts.is_empty()
    }

    fn find_dock(&mut self) -> Option<u64> {
        self.sim
            .aircraft_find_docking_bay(self.id, self.rules, self.ctx.effects)
    }

    fn assign_dock(&mut self, dock: u64) {
        self.sim.assign_aircraft_destination(
            self.id,
            Some(NavTargetRef::object(dock)),
            self.rules,
            self.ctx.effects,
        );
    }

    fn clear_target(&mut self) {
        self.sim
            .assign_target_represented(
                self.id,
                None,
                Some(self.rules),
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("aircraft dispatch");
    }

    /// `Mission_Guard @ 0x0041A5C0`'s contact 0: a Building whose type is
    /// `UnitReload=` (`+0x16AA`, `0x0041A73F`).
    fn contact_reloads(&mut self) -> bool {
        let Some(contact) = self.entity().radio_contacts.slot(0) else {
            return false;
        };
        self.sim
            .substrate
            .entities
            .get(contact)
            .filter(|building| building.category() == EntityCategory::Structure)
            .and_then(|building| self.sim.object_type(building.type_ref(), self.rules))
            .is_some_and(|object| object.unit_reload)
    }

    /// TarCom `+0x2B4`, as `Mission_Guard @ 0x0041A5C0` reads it (`0x0041A822`).
    fn target(&mut self) -> bool {
        crate::sim::combat::target_present(self.entity().attack_target.target(), self.sim)
    }

    fn attack_bridge_unit(&mut self) {
        let entity = self.entity();
        let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let Some(unit) = self
            .sim
            .house_nearest_bridge_unit(entity.owner(), [coord.x, coord.y, coord.z])
        else {
            return;
        };
        self.sim
            .assign_target_represented(
                self.id,
                Some(TargetKind::Entity(unit)),
                Some(self.rules),
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("aircraft dispatch");
        self.queue(MissionType::Attack);
    }

    fn in_air(&mut self) -> bool {
        air_movement::is_high_flying(
            self.entity(),
            self.sim.resolved_terrain.as_ref(),
            Some((self.rules, &self.sim.interner)),
        )
    }

    fn jitter(&mut self) -> i32 {
        self.sim.scenario_rng.next_range_u32_inclusive(
            0,
            crate::sim::mission::authority::RATE_EPILOGUE_JITTER_MAX_FRAMES,
        ) as i32
    }

    fn foot_guard(&mut self) -> i32 {
        let mission = self.current();
        mission_handlers::evaluate_foot_guard_cadence(
            self.sim, self.rules, self.id, mission, self.ctx,
        )
        .delay()
    }

    fn foot_area_guard(&mut self) -> i32 {
        mission_handlers::evaluate_foot_area_guard(self.sim, self.id, self.rules, self.ctx).delay()
    }
}
