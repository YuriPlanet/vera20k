//! `AircraftClass::Find_Nearest_Friendly_Airfield @ 0x0041A160`: the cell an
//! aircraft heads for when it has nowhere to land.
//!
//! Evidence: `tools/spatial_oracle/fly_stop.py` runs it natively under Fly's
//! `Stop_Moving`; `world::fly_stop_tests` replays those rows.

use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::movement::ground_pose;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::distance_3d_leptons;

/// The building distance (leptons) under which the aircraft is taken to be
/// at the airfield already and searches around its own cell (`0x0041A30C`).
const AT_AIRFIELD_LEPTONS: i32 = 0x100;

impl Simulation {
    /// `AircraftClass::Find_Nearest_Friendly_Airfield @ 0x0041A160`: the cell
    /// (`MapClass::GetCellAt`) aircraft `id` should fly to.
    ///
    /// With a `Dock=` list, each building of its House in vector order
    /// (`HouseClass+0x6C`) outside Limbo is measured from the aircraft
    /// (`vt+0x48` coordinates, `CoordStruct__Distance3D @ 0x0041C380`); one
    /// whose type is the first `Dock=` entry counts a quarter of that. The
    /// first strictly nearer than the best so far is searched around: a 3x3
    /// passable spot at its cell (`vt+0x1B8`) in that cell's zone
    /// ([`Self::airfield_search`]); a miss leaves the best as it was. The best
    /// spot is the answer, unless that building lies under 0x100 leptons
    /// away, when it is a 1x1 spot at the aircraft's own cell (NullCell on a
    /// miss).
    ///
    /// Without such a building, every other techno of its House outside Limbo
    /// (`ObjectClass::Array @ 0x00A8E364`, cast by `0x0040DD70`) is measured
    /// the same way, unweighted, and the nearest 1x1 spot at a techno's cell
    /// wins; failing all, the cell of the aircraft's own Location.
    ///
    /// RESIDUAL: ObjectClass::Array holds objects in construction order; VERA
    /// walks stable ids, which is creation order. A factory's product is
    /// constructed natively when its production starts and in VERA when it
    /// completes, so two technos at exactly the same truncated distance can
    /// resolve the other way. Trigger: a House with no building, at a tie.
    pub(crate) fn aircraft_find_nearest_friendly_airfield(
        &self,
        id: u64,
        rules: &RuleSet,
    ) -> (i16, i16) {
        let terrain = self.resolved_terrain.as_ref();
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("Find_Nearest_Friendly_Airfield owner");
        let owner = entity.owner();
        let own = ground_pose::object_get_coords(entity, terrain);
        let own_cell = map_coords(entity);
        let distance = |other: &crate::sim::game_entity::GameEntity| {
            let coords = ground_pose::object_get_coords(other, terrain);
            distance_3d_leptons([own.x, own.y, own.z], [coords.x, coords.y, coords.z])
        };
        let docks = self
            .interner
            .object_type(entity.type_ref(), rules)
            .map_or(&[][..], |object| object.dock.as_slice());
        let mut best = None;
        if let Some(first) = docks.first() {
            let buildings = self
                .houses
                .get(&owner)
                .map_or(&[][..], |house| house.base_projection().buildings());
            for &building in buildings {
                let Some(building) = self.substrate.entities.get(building) else {
                    continue;
                };
                if building.lifecycle.in_limbo() {
                    continue;
                }
                let mut d = distance(building);
                if self
                    .interner
                    .resolve(building.type_ref())
                    .eq_ignore_ascii_case(first)
                {
                    d /= 4;
                }
                if best.is_some_and(|(best, _)| d >= best) {
                    continue;
                }
                if let Some(cell) = self.airfield_search(map_coords(building), 3) {
                    best = Some((d, cell));
                }
            }
            match best {
                Some((d, _)) if d < AT_AIRFIELD_LEPTONS => {
                    return self.airfield_search(own_cell, 1).unwrap_or((0, 0));
                }
                Some((_, cell)) => return cell,
                None => {}
            }
        }
        for other in self.substrate.entities.values() {
            if other.stable_id() == id || other.lifecycle.in_limbo() || other.owner() != owner {
                continue;
            }
            let d = distance(other);
            if best.is_some_and(|(best, _)| d >= best) {
                continue;
            }
            if let Some(cell) = self.airfield_search(map_coords(other), 1) {
                best = Some((d, cell));
            }
        }
        best.map_or(own_cell, |(_, cell)| cell)
    }

    /// [`Self::aircraft_find_nearest_friendly_airfield`] as the destination
    /// its callers hand the class setter: the `CellClass` the cell names
    /// (`MapClass::GetCellAt`), a cell off the map being the shared dummy.
    pub(crate) fn aircraft_nearest_friendly_airfield_cell(
        &self,
        id: u64,
        rules: &RuleSet,
    ) -> crate::sim::components::NavTargetRef {
        let cell = self.aircraft_find_nearest_friendly_airfield(id, rules);
        let (x, y) = self
            .resolved_terrain
            .as_ref()
            .map_or(cell, |t| t.native_cell_coord(t.native_cell_identity(cell)));
        crate::sim::components::NavTargetRef::cell(x as u16, y as u16)
    }

    /// `AircraftClass::Find_Docking_Bay @ 0x0041BBD0` (vt+0x528) with the
    /// type's `Dock=` list and `(0, 0)`, as the idle mode (`0x004179A4`,
    /// `0x00417B0C`), Mission_Guard (`0x0041A7E0`) and the Enter order
    /// (`0x0041AB3C`) ask it: an AirportBound aircraft keeps its dock
    /// (`+0x6CC`) while that dock answers its CAN_LOAD ROGER; otherwise the
    /// dock is cleared and set to what `FootClass::Find_Docking_Bay(list, 0,
    /// 0)` answers ([`find_docking_bay`]).
    ///
    /// [`find_docking_bay`]: crate::sim::miner::miner_system::find_docking_bay
    pub(crate) fn aircraft_find_docking_bay(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> Option<u64> {
        let entity = self.substrate.entities.get(id)?;
        let airport_bound = self
            .interner
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.airport_bound);
        let cached = entity.ammo().and_then(|ammo| ammo.dock());
        if airport_bound && let Some(cached) = cached {
            if crate::sim::radio::transmit(
                self,
                id,
                cached,
                crate::sim::radio::RadioMessage::CanEnter,
                crate::sim::radio::RadioPayload::default(),
                rules,
                effects,
            ) == crate::sim::radio::RadioResponse::Roger
            {
                return Some(cached);
            }
            self.substrate.set_aircraft_dock(id, None);
        }
        let dock = crate::sim::miner::miner_system::find_docking_bay(
            self, rules, id, false, false, effects,
        );
        self.substrate.set_aircraft_dock(id, dock);
        dock
    }

    /// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` as `0x0041A160`
    /// asks it (`0x0041A279..0x0041A2C0`, `0x0041A313..0x0041A36B`,
    /// `0x0041A48E..0x0041A4D4`): SpeedType Foot, MovementZone Normal, the
    /// seed's zone (`MapClass::GetZoneID @ 0x0056D230`, Normal, no bridge
    /// resolution), `alt` clear, a `size` x `size` footprint, bridges
    /// allowed and no target cell. `None` is NullCell; a world without a
    /// zone grid (headless fixtures) asks no zone.
    fn airfield_search(&self, seed: (i16, i16), size: i32) -> Option<(i16, i16)> {
        let terrain = self.resolved_terrain.as_ref()?;
        let zone = self.zone_grid.as_ref().and_then(|zones| {
            zones.get_zone_id_native(
                terrain,
                (seed.0 as u16, seed.1 as u16),
                MovementZone::Normal,
                false,
            )
        });
        self.nearby_passable_cell(
            (i32::from(seed.0), i32::from(seed.1)),
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: SpeedType::Foot,
                    required_zone_id: zone,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::new(size, size),
            ),
        )
        .filter(|cell| *cell != (0, 0))
        .map(|(x, y)| (x as i16, y as i16))
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// Aircraft `+0x6CC`'s writers ([`TechnoAmmo::set_dock`]): Mission_Enter
    /// (`0x00419CE5`, `0x00419D57`), Find_Docking_Bay (`0x0041BBFF`,
    /// `0x0041BC1C`) and the factory exit (`0x0044404D`).
    ///
    /// [`TechnoAmmo::set_dock`]: crate::sim::ammo::TechnoAmmo::set_dock
    pub(crate) fn set_aircraft_dock(&mut self, id: u64, dock: Option<u64>) {
        if let Some(ammo) = self
            .entities
            .get_mut(id)
            .and_then(|entity| entity.ammo_mut())
        {
            ammo.set_dock(dock);
        }
    }
}

/// `vt+0x1B8` (`0x0041BEA0`, every class this search meets): the cell of the
/// Location.
fn map_coords(entity: &crate::sim::game_entity::GameEntity) -> (i16, i16) {
    crate::sim::movement::coord_cell(ground_pose::position_world_coord(&entity.position))
}
