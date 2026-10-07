//! The House's building lists, retained base geometry and FactoryPlant factors.
//!
//! House+68 (buildings) and House+140 (FactoryPlants) change membership at
//! Unlimbo, pointer expiry and ChangeOwner; constructor/destructor tracking
//! (`0x004FF700`/`0x004FF550`, `sim::house_tracking`) counts separately.
use super::*;
use crate::rules::ruleset::HouseCostFactors;
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::util::native_x87::NativeF32Bits;

type CostFactors = [NativeF32Bits; 5];
const UNIT_FACTORS: CostFactors = [NativeF32Bits::ONE; 5];

/// Immutable rule projections retained beside the House registration. This
/// lets Unlimbo, ChangeOwner and pointer expiry use the same inputs without a
/// borrowed RuleSet or per-tick string lookup. Registration order is not list
/// order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct RegisteredBuilding {
    id: u64,
    /// The type's cost bonuses when it is a `FactoryPlant=` type.
    plant: Option<CostFactors>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct HouseBaseState {
    registrations: Vec<RegisteredBuilding>,
    /// Native House+68 (DynamicVectorClass, Items `+0x6C`, Count `+0x78`):
    /// Unlimbo appends (`0x00441553..0x00441594`), pointer expiry and
    /// ChangeOwner stable-remove (`0x004FBC1C`, `0x00448A78..0x00448AB0`) and
    /// ChangeOwner appends to the new House's tail (`0x00449197..0x004491D2`).
    /// The Prism walk reads it in this order ([`Self::buildings`]).
    buildings: Vec<u64>,
    /// Native House+140, whose order determines each f32 multiplication/store.
    plants: Vec<u64>,
    /// Historical House+5498. Constructor4F5A53 stores zero; `4FD150`
    /// publishes it at Building lifecycle boundaries, not from a list on read.
    radius: i32,
}

/// Admitted House+68 entries, with the shared Cost_Of and Building447AC0
/// projections already resolved. Keeping signed leptons here also lets native
/// overflow rows exercise the same arithmetic without narrowing into Position.
struct ProjectionBuilding {
    cost: i32,
    coord: DriveCoord,
}

/// `HouseClass::RecalculateBase @ 0x004FD150`, through its primary/radius
/// publication (`0x004FD2CB`, `0x004FD4E2..0x004FD4EE`). Sector aggregates are
/// a separate consumer. Native execution: spatial_oracle/house_base_projection
/// and its selected house_base_return projection corpus.
///
/// Building447AC0 is pure, so its repeated weighted coordinate reads can be
/// batched with wrapping multiplication. Distance uses the existing native
/// Sqrt_Approx owner: one unweighted distance per admitted entry, divided by
/// the positive cost-weight sum, with wrapped dword accumulation.
fn project_geometry(
    sim: &Simulation,
    tracked_buildings: i32,
    buildings: &[ProjectionBuilding],
    alternate: (u16, u16),
) -> (Option<(u16, u16)>, i32) {
    if tracked_buildings <= 0 {
        return (None, 0);
    }
    let mut weight_total = 0_i32;
    let mut sum_x = 0_i32;
    let mut sum_y = 0_i32;
    for building in buildings {
        let weight = (building.cost / 1000).wrapping_add(1);
        if weight > 0 {
            weight_total = weight_total.wrapping_add(weight);
            sum_x = sum_x.wrapping_add(building.coord.x.wrapping_mul(weight));
            sum_y = sum_y.wrapping_add(building.coord.y.wrapping_mul(weight));
        }
    }
    let primary = if weight_total > 0 {
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("map projection terrain");
        let native_cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let (size_width, size_height) = sim.map_size_diamond().expect("map projection Size");
        let query = NearbyQuery {
            native_cells: Some(&native_cells),
            raw_occupation: Some(&sim.substrate.raw_cell_occupation),
            passability: PassabilityArgs {
                speed_type: SpeedType::Foot,
                required_zone_id: None,
                movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
                bridge_aware_zone: false,
            },
            footprint: NearbyFootprint::SINGLE,
            anchor_gate: NearbyAnchorGate::NativeHeightAware,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: false,
            radius_cap: map_owned_radius_cap(size_width, size_height),
            target_cell: None,
            path_grid: None,
            resolved_terrain: Some(terrain),
            overlay_grid: sim.overlay_grid.as_ref(),
            occupancy: Some(&sim.substrate.occupancy),
            entities: Some(&sim.substrate.entities),
            zone_grid: sim.zone_grid.as_ref(),
            playfield_bounds: sim.playfield_bounds,
        };
        let x = (sum_x / weight_total / 256) as i16;
        let y = (sum_y / weight_total / 256) as i16;
        find_nearby_passable_cell(
            (i32::from(x), i32::from(y)),
            &query,
            sim.session.binary_frame,
        )
        .filter(|&cell| cell != (0, 0))
    } else {
        None
    };
    if weight_total <= 1 {
        return (primary, 512);
    }
    let origin = if alternate != (0, 0) {
        alternate
    } else {
        primary.unwrap_or((0, 0))
    };
    let mut radius_sum = 0_i32;
    for building in buildings {
        // 50DEF0 -> Map5657A0 -> Cell486840 runs for each entry. A null
        // packed cell instead reads NullCoord, not cell (0,0)'s centre.
        let origin_coord = if origin == (0, 0) {
            DriveCoord { x: 0, y: 0, z: 0 }
        } else {
            crate::sim::movement::target_cell_coord(
                origin.0,
                origin.1,
                sim.resolved_terrain
                    .as_ref()
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            )
        };
        let distance = crate::util::native_x87::object_distance(
            [building.coord.x, building.coord.y, building.coord.z],
            [origin_coord.x, origin_coord.y, origin_coord.z],
            None,
        );
        radius_sum = radius_sum.wrapping_add(distance);
    }
    (primary, (radius_sum / weight_total).max(512))
}

impl RegisteredBuilding {
    fn new(id: u64, object: &ObjectType) -> Self {
        Self {
            id,
            plant: object.factory_plant.then_some(object.cost_bonuses),
        }
    }
}

impl HouseBaseState {
    /// The FactoryPlant products (House `+0x5390..+0x53A0`, in
    /// [`ObjectType::factor_slot`] order) that `CalculateCostMultipliers @
    /// 0x0050BF60` stores; `HouseClass::GetAccumulatedBonus @ 0x0050BEB0`
    /// returns one slot. Every House+140 change is followed by that recompute,
    /// so VERA folds the list when read instead of keeping the products.
    pub(crate) fn factory_plant_factors(&self) -> CostFactors {
        let mut factors = UNIT_FACTORS;
        for &id in &self.plants {
            if let Some(bonuses) = self.registration(id).and_then(|entry| entry.plant) {
                for (factor, bonus) in factors.iter_mut().zip(bonuses) {
                    *factor = multiply_factor(*factor, bonus);
                }
            }
        }
        factors
    }

    /// House+140 in vector order.
    pub(crate) fn factory_plants(&self) -> &[u64] {
        &self.plants
    }

    /// House+68 in vector order.
    pub(crate) fn buildings(&self) -> &[u64] {
        &self.buildings
    }

    /// An oracle row's House+68, in its order; an id no object holds stands
    /// for a null entry.
    #[cfg(test)]
    pub(crate) fn replace_buildings_for_test(&mut self, buildings: Vec<u64>) {
        self.buildings = buildings;
    }

    fn registration(&self, id: u64) -> Option<&RegisteredBuilding> {
        self.registrations
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()
            .map(|index| &self.registrations[index])
    }

    fn register(&mut self, entry: RegisteredBuilding) {
        match self
            .registrations
            .binary_search_by_key(&entry.id, |entry| entry.id)
        {
            Ok(_) => {}
            Err(index) => self.registrations.insert(index, entry),
        }
    }

    fn unregister(&mut self, id: u64) -> Option<RegisteredBuilding> {
        let index = self
            .registrations
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()?;
        Some(self.registrations.remove(index))
    }

    fn remove_membership(&mut self, id: u64) {
        //4FB9B0 stable-removes one matching pointer, then50BF60 recomputes.
        remove_first(&mut self.plants, id);
        remove_first(&mut self.buildings, id);
    }

    /// The appends Unlimbo (`0x004414F1..0x00441594`) and ChangeOwner's new
    /// House (`0x00449139..0x004491D2`) make: a `FactoryPlant=` type joins
    /// House+140 and the factors are recomputed (`0x0050BF60`), then the
    /// building joins the House+68 tail. The two vectors have independent
    /// lives.
    fn append_membership(&mut self, id: u64) {
        if self
            .registration(id)
            .is_some_and(|entry| entry.plant.is_some())
        {
            self.plants.push(id);
        }
        self.buildings.push(id);
    }
}

fn remove_first(ids: &mut Vec<u64>, id: u64) {
    if let Some(index) = ids.iter().position(|&candidate| candidate == id) {
        ids.remove(index);
    }
}

/// `0x0050BF60` multiplies two floats in x87 and stores the product with
/// exceptions masked under the chop control word: gradual underflow, signed
/// zero and saturation at the largest finite float.
/// Native comparison: tools/spatial_oracle/factory_plant_factors.
fn multiply_factor(lhs: NativeF32Bits, rhs: NativeF32Bits) -> NativeF32Bits {
    use crate::util::native_x87::MaskedX87Chop53 as X87;
    X87::store_f32_masked_chop(X87::mul(X87::load_f32(lhs), X87::load_f32(rhs)))
}

impl crate::sim::house_state::HouseState {
    /// The retained House4FD150 radius, also read by House500200/501AC0.
    pub(crate) fn base_radius(&self) -> i32 {
        self.base_projection.radius
    }

    /// House50E000's explicit primary write, called at Unit7398CE and scenario
    /// launch. It does not recalculate radius or replace alternate/BasePlan cells.
    pub(crate) fn set_base_center(&mut self, cell: (u16, u16)) {
        self.base_center = Some(cell);
    }

    fn retain_base_geometry(&mut self, primary: Option<(u16, u16)>, radius: i32) {
        self.base_center = primary;
        self.base_projection.radius = radius;
    }

    /// Supplied physical House+5490/+5498 inputs in native query corpora.
    #[cfg(test)]
    pub(crate) fn set_base_geometry_for_test(&mut self, primary: Option<(u16, u16)>, radius: i32) {
        self.retain_base_geometry(primary, radius);
    }

    /// The House factors TechnoType `Cost_Of` (`0x00711F00`) reads: its
    /// country's `Cost*Mult=` and its FactoryPlant products.
    pub(crate) fn cost_factors(&self) -> HouseCostFactors {
        HouseCostFactors {
            country: self.country_cost_mults.0,
            factory_plant: self.base_projection.factory_plant_factors(),
        }
    }
}

impl Simulation {
    /// Ordinary Unit/Infantry arm of House500200 (Find_Passable_Cell_Near_Unit),
    /// used by Foot Mission_Rescue. Their original virtual+2D4/+2D8/+2DC bodies
    /// (41BF00/10/20) return zero, selecting House501AC0 variant0 without the
    /// alternative-selector draw. Other classes/variants are not covered here.
    /// Native comparison: spatial_oracle/house_base_return, all28 home rows.
    ///
    /// Returns None for native packed-zero FNPC or unavailable actor/House/map
    /// authority. Missing terrain/Size/LocalSize/zones consume no RNG and do
    /// not supply a substitute map. This is a query: no mission or radius write.
    pub(crate) fn house_return_cell(&mut self, actor_id: u64) -> Option<(u16, u16)> {
        let actor = self.substrate.entities.get(actor_id)?;
        if !matches!(
            actor.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) {
            return None;
        }
        //50025F..5002C9 reads physical Object+9C/+A0, not Should_Be/NavCom or
        //a virtual bridge-height coordinate. OnBridge does not change its zone.
        let current = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
        let house = self.houses.get(&actor.owner())?;
        let radius = house.base_radius().clamp(768, 2048);
        let origin = house.base_origin();
        let terrain = self.resolved_terrain.as_ref()?;
        let bounds = self.playfield_bounds?;
        let (size_width, size_height) = self.map_size_diamond()?;
        let zones = self.zone_grid.as_ref()?;
        let native_cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);

        //501B2B..501BB9: radius draw FIRST, then retained origin -> ground
        //Cell486840 (no deck rise), then one raw direction draw with snap1.
        let magnitude = self.scenario_rng.next_range_i32_inclusive(0, radius);
        let base = if origin == (0, 0) {
            DriveCoord { x: 0, y: 0, z: 0 }
        } else {
            crate::sim::movement::target_cell_coord(origin.0, origin.1, Some(&native_cells))
        };
        let (x, y) = crate::sim::combat::inviso_scatter::random_direction_coord(
            &mut self.scenario_rng,
            base.x,
            base.y,
            magnitude,
            crate::sim::combat::inviso_scatter::RandomDirectionSnap::CellCenter,
        );
        //Cell-centre snap1 cannot yield NullCoord. 413A30 divides toward zero
        //before packing: a sampled -128 lepton component becomes cell0.
        let mut seed = (
            i32::from(crate::util::lepton::lepton_to_cell_packed(x)),
            i32::from(crate::util::lepton::lepton_to_cell_packed(y)),
        );
        //50209B..5020BF: mode1 membership and conditional correction precede
        //5002CF's ground scalar zone query. Preserve their shared Dummy order.
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
            seed,
            Some(bounds),
            Some(terrain),
            Some(&native_cells),
        ) {
            seed = crate::sim::cell_rect::clamp_cell_to_playfield(
                seed,
                bounds,
                Some(terrain),
                Some(&native_cells),
            );
        }
        let current_cell = (
            i32::from(crate::util::lepton::lepton_to_cell_packed(current.x)),
            i32::from(crate::util::lepton::lepton_to_cell_packed(current.y)),
        );
        let zone = zones.get_zone_id_native_in_query(
            terrain,
            (current_cell.0 as u16, current_cell.1 as u16),
            crate::rules::locomotor_type::MovementZone::Normal,
            false,
            Some(&native_cells),
        )?;
        //5002E5 packed arguments [Track1,zone,Normal0,bridge0,1,1,0,0,0,1,0,0].
        //GetZone has NO SpeedType parameter; the following FNPC always Track.
        let query = NearbyQuery {
            native_cells: Some(&native_cells),
            raw_occupation: Some(&self.substrate.raw_cell_occupation),
            passability: PassabilityArgs {
                speed_type: SpeedType::Track,
                required_zone_id: Some(zone),
                movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
                bridge_aware_zone: false,
            },
            footprint: NearbyFootprint::SINGLE,
            anchor_gate: NearbyAnchorGate::NativeHeightAware,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: false,
            radius_cap: map_owned_radius_cap(size_width, size_height),
            target_cell: None,
            path_grid: None,
            resolved_terrain: Some(terrain),
            overlay_grid: self.overlay_grid.as_ref(),
            occupancy: Some(&self.substrate.occupancy),
            entities: Some(&self.substrate.entities),
            zone_grid: Some(zones),
            playfield_bounds: Some(bounds),
        };
        find_nearby_passable_cell(seed, &query, self.session.binary_frame)
            .filter(|&cell| cell != (0, 0))
    }

    /// Execute the retained House4FD150 projection with this map's native
    /// authority. Returns false without House/terrain/Size/LocalSize: mapless
    /// component fixtures cannot execute MapClass prerequisites and keep their
    /// supplied historical geometry. Real map loaders install all four inputs.
    pub(crate) fn recalculate_house_base_geometry(
        &mut self,
        owner: InternedId,
        rules: &RuleSet,
    ) -> bool {
        let Some(house) = self.houses.get(&owner) else {
            return false;
        };
        if self.resolved_terrain.is_none()
            || self.playfield_bounds.is_none()
            || self.map_size_diamond().is_none()
        {
            return false;
        }
        let mut buildings = Vec::with_capacity(house.base_projection.buildings.len());
        if house.tracking.buildings() > 0 {
            for &id in house.base_projection.buildings() {
                let Some(entity) = self.substrate.entities.get(id) else {
                    continue;
                };
                if entity.lifecycle.in_limbo || entity.health.current <= 0 {
                    continue;
                }
                let Some(object) = self.object_type(entity.type_ref(), rules) else {
                    continue;
                };
                buildings.push(ProjectionBuilding {
                    cost: self.cost_of(owner, object, rules),
                    coord: crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        self.resolved_terrain.as_ref(),
                    ),
                });
            }
        }
        let (primary, radius) = project_geometry(
            self,
            house.tracking.buildings(),
            &buildings,
            house.alternate_base_center,
        );
        self.houses
            .get_mut(&owner)
            .expect("projection House remains registered")
            .retain_base_geometry(primary, radius);
        true
    }

    /// [`HouseState::cost_factors`] for `owner`. `None` without a House, which
    /// takes Cost_Of's null-House arm (`0x00711F4E`, the raw cost).
    pub(crate) fn house_cost_factors(&self, owner: InternedId) -> Option<HouseCostFactors> {
        Some(self.houses.get(&owner)?.cost_factors())
    }

    /// TechnoType virtual `+0x84` for `owner`'s House ([`RuleSet::cost_of`]).
    pub(crate) fn cost_of(&self, owner: InternedId, object: &ObjectType, rules: &RuleSet) -> i32 {
        debug_assert!(
            self.houses.get(&owner).is_none_or(|house| {
                house.country_cost_mults.0
                    == rules.country_cost_mults(self.interner.resolve(house.house_type_id()))
            }),
            "a House's `Cost*Mult=` copy differs from its type's: project it at creation"
        );
        rules.cost_of(object, self.house_cost_factors(owner).as_ref())
    }

    pub(super) fn register_house_base_building(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category != EntityCategory::Structure {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let entry = RegisteredBuilding::new(id, object);
        let owner = entity.owner();
        if let Some(house) = self.houses.get_mut(&owner) {
            house.base_projection.register(entry);
        }
    }

    /// `BuildingClass::Unlimbo`'s House+68 append (`0x00441553..0x00441594`),
    /// reached on the same path as its BuildConst append
    /// ([`Simulation::append_live_build_const`]).
    pub(super) fn append_house_base_building(&mut self, id: u64) {
        let Some(owner) = self
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category == EntityCategory::Structure)
            .map(|entity| entity.owner())
        else {
            return;
        };
        if let Some(house) = self.houses.get_mut(&owner) {
            house.base_projection.append_membership(id);
        }
    }

    /// For a fixture that stores a building directly: the House+68 append its
    /// Unlimbo would make ([`Self::append_house_base_building`]).
    #[cfg(test)]
    pub(crate) fn append_house_base_building_for_test(&mut self, id: u64) {
        self.append_house_base_building(id);
    }

    /// `BuildingClass::ChangeOwner`'s moves between the two Houses' lists:
    /// before the Techno owner swap it stable-removes the building from the
    /// old House's FactoryPlant list, recomputes and removes it from House+68
    /// (`0x00448A21..0x00448AB0`); after it, it appends to the new House's
    /// lists (`0x00449155..0x004491D2`).
    pub(super) fn leave_house_base_lists(&mut self, id: u64, owner: InternedId) {
        if let Some(house) = self.houses.get_mut(&owner) {
            house.base_projection.remove_membership(id);
        }
    }

    pub(super) fn join_house_base_lists(
        &mut self,
        id: u64,
        old_owner: InternedId,
        owner: InternedId,
        rules: Option<&RuleSet>,
    ) {
        // Building448C76/448C81: old list already removed and Techno tracking
        // moved, but neither new FactoryPlant nor House+68 append has run.
        if let Some(rules) = rules {
            self.recalculate_house_base_geometry(old_owner, rules);
            self.recalculate_house_base_geometry(owner, rules);
        }
        if let Some(house) = self.houses.get_mut(&owner) {
            house.base_projection.append_membership(id);
        }
    }

    /// `TechnoClass::ChangeOwner`'s Remove_Tracking on the old House and
    /// Add_Tracking on the new (`0x007015DE`, `0x007015E6`) move the
    /// building's registration with it.
    pub(super) fn move_house_base_tracking(
        &mut self,
        id: u64,
        old_owner: InternedId,
        new_owner: InternedId,
    ) {
        let Some(entry) = self
            .houses
            .get_mut(&old_owner)
            .and_then(|house| house.base_projection.unregister(id))
        else {
            return;
        };
        if let Some(house) = self.houses.get_mut(&new_owner) {
            house.base_projection.register(entry);
        }
    }

    pub(super) fn remove_house_base_membership(&mut self, id: u64) {
        if !self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category == EntityCategory::Structure)
        {
            return;
        }
        // Broadcast visits every House, including historical non-owner lists.
        for house in self.houses.values_mut() {
            house.base_projection.remove_membership(id);
        }
    }

    pub(super) fn release_house_base_tracking(&mut self, id: u64) {
        if let Some(owner) = self.substrate.entities.get(id).map(|entity| entity.owner())
            && let Some(house) = self.houses.get_mut(&owner)
        {
            //43BF34 changes tracking AFTER the earlier expiry/Limbo. No recalc.
            house.base_projection.unregister(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;

    /// The oracle's supplied physical MapClass: Size8x8, LocalSize0,0,8,8,
    /// flat allocated cells0..16, native frame100 and an empty occupation plane.
    fn native_map_fixture() -> Simulation {
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(17),
        );
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 8,
            off_fc: 0,
            off_100: 0,
            off_104: 8,
            off_108: 8,
        });
        sim.playfield_size_height = Some(8);
        sim.session.map_width = 17;
        sim.session.map_height = 17;
        sim.session.binary_frame = 100;
        sim
    }

    fn add_house(sim: &mut Simulation, name: &str, rules: &RuleSet) -> InternedId {
        let owner = sim.interner.intern(name);
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
        house.project_country_mults(rules, &sim.interner);
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        owner
    }

    fn geometry(sim: &Simulation, owner: InternedId) -> (Option<(u16, u16)>, i32) {
        let house = &sim.houses[&owner];
        (house.base_center, house.base_radius())
    }

    fn ordinary_rules() -> RuleSet {
        RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n\
             [BuildingTypes]\n0=FIRST\n1=SECOND\n\
             [FIRST]\nCost=2000\nStrength=1000\nFoundation=1x1\n\
             [SECOND]\nCost=1000\nStrength=1000\nFoundation=1x1\n",
        ))
        .unwrap()
    }

    /// Supplied postconstruction physical inputs shared with the native
    /// House500200 corpus. The direct raw Normal-zone labels are oracle
    /// premises, not a claim about flood-fill or retail type construction.
    fn native_home_fixture(input: &serde_json::Value) -> (Simulation, u64, InternedId) {
        let mut sim = native_map_fixture();
        if let Some(bounds) = input["bounds"].as_array() {
            sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
                base: bounds[0].as_i64().unwrap() as i32,
                off_fc: bounds[2].as_i64().unwrap() as i32,
                off_100: bounds[3].as_i64().unwrap() as i32,
                off_104: bounds[4].as_i64().unwrap() as i32,
                off_108: bounds[5].as_i64().unwrap() as i32,
            });
            sim.playfield_size_height = Some(bounds[1].as_i64().unwrap() as i32);
        }
        let mut terrain = sim.resolved_terrain.take().unwrap();
        if let Some(cells) = input["cells"].as_array() {
            for cell in cells {
                let x = cell["xy"][0].as_u64().unwrap() as u16;
                let y = cell["xy"][1].as_u64().unwrap() as u16;
                let target = terrain.cell_mut(x, y).unwrap();
                if let Some(level) = cell["level"].as_i64() {
                    target.level = level as u8;
                }
                if let Some(slope) = cell["slope"].as_u64() {
                    target.slope_type = slope as u8;
                }
                if let Some(flags) = cell["flags"].as_u64() {
                    target.bridge_facts.raw_flags = flags as u32;
                }
                if let Some(land) = cell["land"].as_i64() {
                    target.yr_cell_land_type = land as u8;
                }
            }
        }
        sim.install_resolved_terrain_for_new_map(terrain);
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let path_grid = PathGrid::from_resolved_terrain(terrain);
        let mut zones = ZoneGrid::build_with_native_map_context(
            &path_grid,
            terrain,
            &[],
            sim.map_size_diamond(),
            sim.playfield_bounds,
        );
        let overrides = input["zone_overrides"].as_array();
        let base = zones.base_topology_mut();
        base.zone_ids.fill(0);
        base.movement_classes.fill(0);
        let row = crate::rules::locomotor_type::MovementZone::Normal
            .matrix_row()
            .unwrap();
        // Supplied native query premises use one shared Map+4C label count
        // for all13 Map+18 rows. Only Normal's values are exercised here, but
        // the unused rows must still form a coherent saved navigation owner.
        let labels = overrides.map_or(0, Vec::len) + 1;
        for raw_row in &mut base.raw_zone_ids_by_row {
            raw_row.resize(labels, 1);
        }
        base.raw_zone_ids_by_row[row].fill(1);
        if let Some(overrides) = overrides {
            for (index, zone) in overrides.iter().enumerate() {
                let x = zone["xy"][0].as_u64().unwrap() as usize;
                let y = zone["xy"][1].as_u64().unwrap() as usize;
                base.zone_ids[y * 17 + x] = index as u16 + 1;
                base.raw_zone_ids_by_row[row][index + 1] = zone["group"].as_u64().unwrap() as u16;
            }
        }
        sim.zone_grid = Some(zones);
        // Zone construction can stamp the same Dummy. Native replay starts
        // its ordinary-return chain AFTER that constructor, with these fields.
        terrain.stamp_dummy_cell_requested_coord(-7, 1);
        terrain.test_set_dummy_cell_level_slope(0, 0);

        let owner = add_house(&mut sim, "Americans", &ordinary_rules());
        let cell = |key: &str, default| {
            input[key].as_array().map_or(default, |cell| {
                (
                    cell[0].as_i64().unwrap() as u16,
                    cell[1].as_i64().unwrap() as u16,
                )
            })
        };
        let primary = cell("primary", (8, 8));
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_base_geometry_for_test(
            (primary != (0, 0)).then_some(primary),
            input["radius"].as_i64().unwrap() as i32,
        );
        house.alternate_base_center = cell("alternate", (0, 0));
        let current = input["current"]
            .as_array()
            .map_or([2688, 2688, 0], |coord| {
                std::array::from_fn(|axis| coord[axis].as_i64().unwrap() as i32)
            });
        let family = input["family"].as_str().unwrap();
        let category = match family {
            "MTNK" => EntityCategory::Unit,
            "E1" => EntityCategory::Infantry,
            _ => panic!("uncovered native concrete class {family}"),
        };
        let type_ref = sim.interner.intern(family);
        let id = 1;
        let rx = (current[0] / 256).max(0) as u16;
        let ry = (current[1] / 256).max(0) as u16;
        let mut actor = GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
            0,
            owner,
            Health { current: 100 },
            type_ref,
            category,
            0,
            0,
            false,
        );
        // The signed-current stress row uses retained negative subcoordinates
        // at unsigned cell0; position_world_coord reconstructs its raw leptons.
        // This is an explicit nonretail physical input, not placement evidence.
        actor.position.sub_x = SimFixed::from_num(current[0] - i32::from(rx) * 256);
        actor.position.sub_y = SimFixed::from_num(current[1] - i32::from(ry) * 256);
        actor.position.exact_z_leptons = Some(current[2]);
        actor.on_bridge = input["on_bridge"].as_bool().unwrap_or(false);
        sim.substrate.entities.insert(actor);
        sim.scenario_rng = crate::sim::rng::SimRng::new(input["seed"].as_u64().unwrap());
        (sim, id, owner)
    }

    #[test]
    fn native_home_inputs_keep_retained_base_geometry_across_snapshot_load() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_base_return.json",
        ))
        .unwrap();
        for row in corpus["home_return"].as_array().unwrap() {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let (sim, _, owner) = native_home_fixture(input);
            let retained = geometry(&sim, owner);
            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, name, 0);
            let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            // These native query premises deliberately have no registered
            // buildings. Restore must retain the saved House5490/5498, not
            // replace them with an empty-membership projection.
            assert_eq!(geometry(&loaded, owner), retained, "{name}");
            let retained_hash = loaded.state_hash();
            loaded
                .houses
                .get_mut(&owner)
                .unwrap()
                .retain_base_geometry(retained.0, retained.1.wrapping_add(1));
            assert_ne!(loaded.state_hash(), retained_hash, "radius hash: {name}");
        }
    }

    #[test]
    fn original_house_500200_ordinary_return_rows_through_production_owner() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_base_return.json",
        ))
        .unwrap();
        let meta: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_base_return.meta.json",
        ))
        .unwrap();
        assert_eq!(
            meta["native_sha256"].as_str(),
            Some("1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c")
        );
        assert_eq!(meta["entry_points"]["ordinary_return"], "0x00500200");
        let rows = corpus["home_return"].as_array().unwrap();
        assert_eq!(rows.len(), 28);
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            assert_eq!(row["original_code_and_vtables_unchanged"], true, "{name}");
            let (mut sim, id, owner) = native_home_fixture(input);
            let before = sim.rng_state();
            let retained = geometry(&sim, owner);
            let physical = crate::sim::movement::ground_pose::position_world_coord(
                &sim.substrate.entities.get(id).unwrap().position,
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_before"].as_str().unwrap(),
                "initial full Scenario RNG: {name}"
            );
            let (answer, trace) = crate::sim::rng::trace_draws(|| sim.house_return_cell(id));
            let cell = (
                row["output"][0].as_i64().unwrap() as u16,
                row["output"][1].as_i64().unwrap() as u16,
            );
            assert_eq!(answer, (cell != (0, 0)).then_some(cell), "{name}");
            assert_eq!(
                trace.len(),
                row["raw_draw_count"].as_u64().unwrap() as usize,
                "{name}"
            );
            let raw_words: Vec<u32> = trace
                .iter()
                .map(|draw| draw["value"].as_u64().unwrap() as u32)
                .collect();
            let native_words: Vec<u32> = row["raw_draws"]
                .as_array()
                .unwrap()
                .iter()
                .map(|draw| draw.as_u64().unwrap() as u32)
                .collect();
            assert_eq!(raw_words, native_words, "raw draw order: {name}");
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "final full Scenario RNG: {name}"
            );
            let after = sim.rng_state();
            assert_eq!(after.main, before.main, "Main untouched: {name}");
            assert_eq!(after.mapgen, before.mapgen, "MapGen untouched: {name}");
            assert_eq!(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .dummy_cell_requested_coord(),
                (
                    row["final_dummy"][0].as_i64().unwrap() as i32,
                    row["final_dummy"][1].as_i64().unwrap() as i32,
                ),
                "final shared Dummy after direction/bounds/zone/FNPC: {name}"
            );
            assert_eq!(
                geometry(&sim, owner),
                retained,
                "query retains House: {name}"
            );
            assert_eq!(
                crate::sim::movement::ground_pose::position_world_coord(
                    &sim.substrate.entities.get(id).unwrap().position,
                ),
                physical,
                "query retains actor: {name}"
            );
        }
    }

    #[test]
    fn ordinary_home_query_reports_unavailable_before_rng_without_map_or_class_authority() {
        let input = serde_json::json!({
            "family": "MTNK", "radius": 0, "seed": 31,
        });
        for missing in 0..7 {
            let (mut sim, id, owner) = native_home_fixture(&input);
            let held_terrain = sim.resolved_terrain.as_ref().unwrap().clone();
            match missing {
                0 => sim.resolved_terrain = None,
                1 => sim.playfield_bounds = None,
                2 => sim.playfield_size_height = None,
                3 => sim.zone_grid = None,
                4 => {
                    sim.houses.remove(&owner);
                }
                5 => {
                    sim.substrate.entities.remove(id);
                }
                _ => {
                    sim.substrate.entities.get_mut(id).unwrap().category = EntityCategory::Structure
                }
            }
            let before = sim.rng_state();
            let hash_before = sim.state_hash();
            assert_eq!(sim.house_return_cell(id), None, "missing premise {missing}");
            assert_eq!(sim.rng_state(), before, "missing premise {missing}");
            assert_eq!(sim.state_hash(), hash_before, "missing premise {missing}");
            assert_eq!(held_terrain.dummy_cell_requested_coord(), (-7, 1));
        }
    }

    #[test]
    fn original_house_4fd150_projection_rows_and_shared_cost_queries() {
        let default: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_base_projection.json",
        ))
        .unwrap();
        let selected: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_base_return.json",
        ))
        .unwrap();
        let mut production_rows = 0;
        for row in default
            .as_array()
            .unwrap()
            .iter()
            .chain(selected["projection"].as_array().unwrap())
        {
            let input = &row["input"];
            let native_buildings = input["buildings"].as_array().unwrap();
            let mut ini = String::from(
                "[General]\nSeparateAircraft=yes\n[Countries]\n0=Americans\n\
                 [VehicleTypes]\n0=FREE\n[FREE]\nCost=1400\n[BuildingTypes]\n",
            );
            for index in 0..native_buildings.len() {
                ini.push_str(&format!("{index}=B{index}\n"));
            }
            for (index, building) in native_buildings.iter().enumerate() {
                ini.push_str(&format!(
                    "[B{index}]\nCost={}\nStrength=100\nFoundation=1x1\n{}",
                    building["cost"].as_i64().unwrap(),
                    if building["free"].as_bool() == Some(true) {
                        "FreeUnit=FREE\n"
                    } else {
                        ""
                    },
                ));
            }
            let rules =
                RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&ini)).unwrap();
            let mut sim = native_map_fixture();
            let owner = add_house(&mut sim, "Americans", &rules);
            let tracked = input["tracked_count"].as_i64().unwrap() as i32;
            let alternate = input["alternate"].as_array().map_or((0, 0), |cell| {
                (
                    cell[0].as_i64().unwrap() as u16,
                    cell[1].as_i64().unwrap() as u16,
                )
            });
            let house = sim.houses.get_mut(&owner).unwrap();
            house.tracking.set_buildings_for_test(tracked);
            house.alternate_base_center = alternate;
            house.retain_base_geometry(Some((14, 14)), 9999);
            if let Some(bonus) = input["unit_bonus"].as_f64() {
                let mut factors = UNIT_FACTORS;
                factors[1] = NativeF32Bits::from_bits((bonus as f32).to_bits());
                // The oracle supplies House+5394 directly. This list recreates
                // that input through the existing factor owner, without adding
                // a building to the oracle's separate House+68 premise.
                house.base_projection.register(RegisteredBuilding {
                    id: u64::MAX,
                    plant: Some(factors),
                });
                house.base_projection.plants.push(u64::MAX);
            }
            let mut admitted = Vec::new();
            let mut representable = true;
            let mut ids = Vec::new();
            for (index, building) in native_buildings.iter().enumerate() {
                let xyz = std::array::from_fn::<_, 3, _>(|axis| {
                    building["xyz"][axis].as_i64().unwrap() as i32
                });
                let limbo = building["limbo"].as_bool().unwrap_or(false);
                let health = building["health"].as_i64().unwrap_or(100) as i32;
                let name = format!("B{index}");
                let object = rules.object(&name).unwrap();
                if !limbo && health > 0 {
                    admitted.push(ProjectionBuilding {
                        cost: sim.cost_of(owner, object, &rules),
                        coord: DriveCoord {
                            x: xyz[0],
                            y: xyz[1],
                            z: xyz[2],
                        },
                    });
                }
                // The three raw signed/overflow rows cannot be represented by
                // Position's unsigned cell and I16F16 subcell storage. They
                // still replay the SAME arithmetic/FNPC/ground owners above.
                if xyz[0] < 0
                    || xyz[1] < 0
                    || xyz[0] / 256 > i32::from(u16::MAX)
                    || xyz[1] / 256 > i32::from(u16::MAX)
                {
                    representable = false;
                    continue;
                }
                let id = index as u64 + 1;
                let type_ref = sim.interner.intern(&name);
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    id,
                    (xyz[0] / 256) as u16,
                    (xyz[1] / 256) as u16,
                    0,
                    0,
                    owner,
                    Health { current: health },
                    type_ref,
                    EntityCategory::Structure,
                    0,
                    0,
                    false,
                );
                entity.lifecycle.in_limbo = limbo;
                entity.position.sub_x = SimFixed::from_num(xyz[0] % 256);
                entity.position.sub_y = SimFixed::from_num(xyz[1] % 256);
                entity.position.exact_z_leptons = Some(xyz[2]);
                sim.substrate.entities.insert(entity);
                ids.push(id);
            }
            let cell = (
                row["primary"][0].as_i64().unwrap() as u16,
                row["primary"][1].as_i64().unwrap() as u16,
            );
            let expected = (
                (cell != (0, 0)).then_some(cell),
                row["radius"].as_i64().unwrap() as i32,
            );
            let rng_before = sim.rng_state();
            assert_eq!(
                project_geometry(&sim, tracked, &admitted, alternate),
                expected,
                "{input}"
            );
            assert_eq!(
                sim.rng_state(),
                rng_before,
                "4FD150/FNPC draw no RNG: {input}"
            );
            if representable {
                production_rows += 1;
                sim.houses
                    .get_mut(&owner)
                    .unwrap()
                    .base_projection
                    .replace_buildings_for_test(ids);
                assert!(sim.recalculate_house_base_geometry(owner, &rules));
                assert_eq!(
                    geometry(&sim, owner),
                    expected,
                    "production resolver: {input}"
                );
            }
        }
        assert_eq!(production_rows, 16);
    }

    #[test]
    fn unlimbo_preappend_limbo_and_uninit_retain_native_historical_geometry() {
        let rules = ordinary_rules();
        let mut sim = native_map_fixture();
        let owner = add_house(&mut sim, "Americans", &rules);
        let first = sim
            .spawn_object("FIRST", "Americans", 5, 5, 0, &rules)
            .unwrap();
        // Original tracked>0/empty-House+68 row. The first append follows it.
        assert_eq!(geometry(&sim, owner), (None, 512));
        assert_eq!(sim.houses[&owner].base_projection.buildings(), [first]);
        let second = sim
            .spawn_object("SECOND", "Americans", 13, 9, 0, &rules)
            .unwrap();
        assert_eq!(
            sim.houses[&owner].base_projection.buildings(),
            [first, second]
        );
        let (primary, radius) = geometry(&sim, owner);
        let primary = primary.expect("first building has a nearby Foot cell");
        assert!(
            primary.0.abs_diff(5) <= 1 && primary.1.abs_diff(5) <= 1,
            "preappend query sees first building, including its occupied cell: {primary:?}"
        );
        assert_eq!(radius, 512);

        assert_eq!(
            sim.techno_limbo_with_rules(second, &rules, None),
            lifecycle::ConcealOutcome::Concealed
        );
        // Both entries are alive/on-map at Building445DA6. The unoccupied
        // centroid agrees with the native two-building row before conceal.
        assert_eq!(geometry(&sim, owner), (Some((8, 7)), 512));
        assert_eq!(sim.houses[&owner].base_projection.buildings(), [first]);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_base_center((14, 14));
        assert_eq!(
            sim.techno_limbo_with_rules(second, &rules, None),
            lifecycle::ConcealOutcome::AlreadyConcealed
        );
        assert_eq!(geometry(&sim, owner), (Some((14, 14)), 512));

        // UnInit removes the pointer BEFORE Building Limbo: empty House+68,
        // while constructor tracking still counts the deferred objects.
        sim.uninit_with_rules(first, &rules);
        assert_eq!(geometry(&sim, owner), (None, 512));
        sim.uninit_with_rules(second, &rules);
        sim.process_pending_delete();
        assert_eq!(sim.houses[&owner].tracking.buildings(), 0);
        assert_eq!(
            geometry(&sim, owner),
            (None, 512),
            "destructor does not recalculate"
        );
    }

    #[test]
    fn capture_recalculates_after_old_removal_and_before_new_append() {
        let rules = ordinary_rules();
        let mut sim = native_map_fixture();
        let old = add_house(&mut sim, "Americans", &rules);
        let new = add_house(&mut sim, "Russians", &rules);
        let first = sim
            .spawn_object("FIRST", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let second = sim
            .spawn_object("SECOND", "Americans", 13, 9, 0, &rules)
            .unwrap();
        sim.change_owner_with_rules(first, new, &rules, None);
        assert_eq!(sim.houses[&old].tracking.buildings(), 1);
        assert_eq!(sim.houses[&new].tracking.buildings(), 1);
        assert_eq!(sim.houses[&old].base_projection.buildings(), [second]);
        assert_eq!(sim.houses[&new].base_projection.buildings(), [first]);
        let old_primary = sim.houses[&old].base_center.unwrap();
        assert!(old_primary.0.abs_diff(13) <= 1 && old_primary.1.abs_diff(9) <= 1);
        // Original tracked>0/empty row on the NEW House. If its append ran
        // first, the geometry would already be near the captured building.
        assert_eq!(geometry(&sim, new), (None, 512));
    }

    #[test]
    fn failed_mark_dead_and_attached_upgrade_unlimbo_do_not_publish_geometry() {
        use crate::sim::world::lifecycle::{
            PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, UninitContext,
        };
        let rules = ordinary_rules();
        let mut sim = native_map_fixture();
        let owner = add_house(&mut sim, "Americans", &rules);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_base_geometry_for_test(Some((14, 14)), 9999);
        for (index, placement) in [
            PlacementEvidence::MarkFailed,
            PlacementEvidence::MarkSucceeded,
            PlacementEvidence::AttachedUpgrade,
        ]
        .into_iter()
        .enumerate()
        {
            let cell = (5 + index as u16, 5);
            let id = sim
                .spawn_object_limbo_at_height("FIRST", "Americans", cell.0, cell.1, 0, 0, &rules)
                .unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            if index == 1 {
                entity.lifecycle.object_alive = false;
            } else if index == 2 {
                entity.structure_upgrade_link =
                    Some(crate::sim::game_entity::StructureUpgradeLink {
                        parent_stable_id: id - 1,
                        slot: 0,
                    });
            }
            let result = sim.try_reveal_entity_with_context(
                id,
                RevealRequest {
                    position: RevealPosition {
                        exact_z_leptons: None,
                        rx: cell.0,
                        ry: cell.1,
                        z: 0,
                        sub_x: SimFixed::from_num(128),
                        sub_y: SimFixed::from_num(128),
                    },
                    placement,
                    logic_eligible: true,
                },
                UninitContext::with_rules(&rules),
            );
            if index == 0 {
                assert!(matches!(result, RevealOutcome::Failed(_)));
            } else {
                assert!(matches!(result, RevealOutcome::Revealed { .. }));
            }
            assert_eq!(geometry(&sim, owner), (Some((14, 14)), 9999));
            assert!(sim.houses[&owner].base_projection.buildings().is_empty());
        }
    }

    #[test]
    fn missing_map_context_reports_unavailable_without_replacing_retained_geometry() {
        let rules = ordinary_rules();
        for missing in 0..3 {
            let mut sim = native_map_fixture();
            let owner = add_house(&mut sim, "Americans", &rules);
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .retain_base_geometry(Some((14, 14)), 9999);
            match missing {
                0 => sim.resolved_terrain = None,
                1 => sim.playfield_bounds = None,
                _ => sim.playfield_size_height = None,
            }
            let hash_before = sim.state_hash();
            assert!(!sim.recalculate_house_base_geometry(owner, &rules));
            assert_eq!(geometry(&sim, owner), (Some((14, 14)), 9999));
            assert_eq!(sim.state_hash(), hash_before);
        }
    }

    #[test]
    fn retained_radius_and_list_order_survive_restore_without_recalculation() {
        let rules = ordinary_rules();
        let mut sim = native_map_fixture();
        let owner = add_house(&mut sim, "Americans", &rules);
        let first = sim
            .spawn_object("FIRST", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let second = sim
            .spawn_object("SECOND", "Americans", 13, 9, 0, &rules)
            .unwrap();
        sim.houses.get_mut(&owner).unwrap().alternate_base_center = (1, 1);
        assert!(sim.recalculate_house_base_geometry(owner, &rules));
        // Saved native alternate-origin projection row.
        assert_eq!(geometry(&sim, owner), (Some((8, 7)), 1028));
        let native_hash = sim.state_hash();
        sim.houses.get_mut(&owner).unwrap().base_projection.radius = 1029;
        assert_ne!(sim.state_hash(), native_hash);
        sim.houses.get_mut(&owner).unwrap().base_projection.radius = 1028;
        assert_eq!(sim.state_hash(), native_hash);
        // A primary writer retains radius; it also leaves the alternate and
        // BasePlan centre untouched. Historical geometry need not equal lists.
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_base_center((14, 14));
        house.base_plan_center = (3, 4);
        // Native load constructs the Scenario RNG with seed0. This test pins
        // geometry/list persistence, not a different RNG restore contract.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let before = sim.state_hash();
        let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "house-base-geometry", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &rules,
        );
        assert_eq!(restored.state_hash(), before);
        assert_eq!(geometry(&restored, owner), (Some((14, 14)), 1028));
        assert_eq!(restored.houses[&owner].alternate_base_center, (1, 1));
        assert_eq!(restored.houses[&owner].base_plan_center, (3, 4));
        assert_eq!(
            restored.houses[&owner].base_projection.buildings(),
            [first, second]
        );
        // Restore leaves geometry historical. The NEXT native boundary must
        // produce the same publication and list cleanup on either continuation.
        sim.techno_limbo_with_rules(second, &rules, None);
        restored.techno_limbo_with_rules(second, &rules, None);
        assert_eq!(geometry(&restored, owner), geometry(&sim, owner));
        assert_eq!(restored.state_hash(), sim.state_hash());
    }

    #[test]
    fn original_factory_plant_f32_fold_including_gradual_underflow() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/factory_plant_factors.json",
        ))
        .unwrap();
        for row in rows.as_array().unwrap() {
            let count = row["count"].as_u64().unwrap();
            let mut state = HouseBaseState::default();
            let mut bonus = UNIT_FACTORS;
            bonus[1] = NativeF32Bits::from_bits(0.75_f32.to_bits());
            if let Some(bits) = row["bonus_bits"].as_array() {
                for (factor, bits) in bonus.iter_mut().zip(bits) {
                    *factor = NativeF32Bits::from_bits(bits.as_u64().unwrap() as u32);
                }
            }
            for id in 0..=count {
                state.register(RegisteredBuilding {
                    id,
                    plant: Some(bonus),
                });
                state.append_membership(id);
            }
            // Exercise the membership receiver used by production Limbo/expiry:
            // the product is the remaining list's ordered native fold.
            state.remove_membership(count);
            let expected: Vec<u32> = row["factor_bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|bits| bits.as_u64().unwrap() as u32)
                .collect();
            assert_eq!(
                state
                    .factory_plant_factors()
                    .map(NativeF32Bits::bits)
                    .as_slice(),
                expected,
                "native FactoryPlant count{count}"
            );
        }
    }

    /// An Industrial Plant discounts its House from Unlimbo on, the discount
    /// moves with a capture and ends at the plant's pointer expiry; the
    /// country's `CostUnitsMult=` multiplies in throughout. Schema 228 folds
    /// House+140 only while it is non-empty.
    #[test]
    fn factory_plant_discount_follows_unlimbo_capture_and_expiry() {
        use crate::map::resolved_terrain::test_flat_ground_grid;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::house_state::HouseState;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n\
             [Russians]\nCostUnitsMult=.5\n\
             [BuildingTypes]\n0=NAINDP\n[VehicleTypes]\n0=HTNK\n\
             [NAINDP]\nStrength=1000\nFoundation=1x1\nFactoryPlant=yes\n\
             UnitsCostBonus=0.75\n\
             [HTNK]\nCost=900\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(32));
        let [allies, soviets] = ["Americans", "Russians"].map(|name| sim.interner.intern(name));
        for house in [allies, soviets] {
            let mut state = HouseState::new(house, 0, None, false, 0, 10);
            state.project_country_mults(&rules, &sim.interner);
            sim.houses.insert(house, state);
        }
        let tank = rules.object("HTNK").unwrap();
        let costs = |sim: &Simulation| {
            (
                sim.cost_of(allies, tank, &rules),
                sim.cost_of(soviets, tank, &rules),
            )
        };
        // The hash folds House+140 only while it is non-empty.
        let plant_fold = |sim: &Simulation| {
            sim.houses
                .values()
                .any(|house| !house.base_projection.factory_plants().is_empty())
        };
        assert_eq!(costs(&sim), (900, 450));
        assert!(!plant_fold(&sim));
        let plant = sim
            .spawn_object("NAINDP", "Russians", 5, 5, 0, &rules)
            .unwrap();
        // ftol(900 * 0.75 * 0.5) = ftol(337.5)
        assert_eq!(costs(&sim), (900, 337));
        assert_eq!(
            sim.houses[&soviets].base_projection.factory_plants(),
            [plant]
        );
        assert!(plant_fold(&sim));
        sim.change_owner_with_rules(plant, allies, &rules, None);
        assert_eq!(costs(&sim), (675, 450));
        assert!(
            sim.houses[&soviets]
                .base_projection
                .factory_plants()
                .is_empty()
        );
        assert_eq!(
            sim.houses[&allies].base_projection.factory_plants(),
            [plant]
        );
        sim.uninit_with_rules(plant, &rules);
        assert_eq!(costs(&sim), (900, 450));
        assert!(!plant_fold(&sim));
    }
}
