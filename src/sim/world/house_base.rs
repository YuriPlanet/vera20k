//! Historical House base projection and its building lifecycle inputs.
//!
//! House4FD150 reads a list whose membership differs from EntityStore's owner
//! index during Unlimbo, Limbo, pointer expiry and ChangeOwner. Keep those
//! membership changes separate from constructor/destructor tracking4FF700/550.
use super::*;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::BuildCategory;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::sim::pathfinding::zone_map::ZoneId;
use crate::util::native_x87::{NativeF32Bits, NativeX87Error, X87Chop53};

// Consumer pending: House 0x4FD150 base centre / nonhuman failed-path
// relocation 0x500200 (AI-deferred); the projection is kept current so that
// owner starts from live inputs.
type CostFactors = [NativeF32Bits; 5];
const UNIT_FACTORS: CostFactors = [NativeF32Bits::ONE; 5];

/// Immutable rule projections retained beside the House registration. This
/// lets pointer expiry and finalization use the same inputs without a borrowed
/// RuleSet or per-tick string lookup. Registration order is not list order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct RegisteredBuilding {
    id: u64,
    tracks_base: bool,
    actual_cost: i32,
    free_unit_cost: Option<i32>,
    defense: bool,
    plant: Option<CostFactors>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct HouseBaseState {
    registrations: Vec<RegisteredBuilding>,
    /// Native House+68; append/remove at the actual callback boundary.
    buildings: Vec<u64>,
    /// Native House+140, whose order determines each f32 multiplication/store.
    plants: Vec<u64>,
    tracked_count: i32,
    factors: CostFactors,
    pub(crate) radius: i32,
}

impl Default for HouseBaseState {
    fn default() -> Self {
        Self {
            registrations: Vec::new(),
            buildings: Vec::new(),
            plants: Vec::new(),
            tracked_count: 0,
            factors: UNIT_FACTORS,
            radius: 0,
        }
    }
}

impl RegisteredBuilding {
    fn from_rules(id: u64, object: &ObjectType, rules: &RuleSet) -> Self {
        // Building+80=457620 ->465D40, then4FF76F tests the UNDEPLOY
        // TARGET's ResourceGatherer. This is not base-reservation eligibility.
        let undeploys_to_gatherer = object
            .undeploys_into
            .as_deref()
            .and_then(|name| rules.object(name))
            .is_some_and(|target| target.resource_gatherer);
        Self {
            id,
            tracks_base: !object.insignificant
                && !object.dont_score
                && !object.is_1x1_with_undeploy()
                && !undeploys_to_gatherer,
            actual_cost: rules.building_actual_cost(object),
            free_unit_cost: object
                .free_unit
                .as_deref()
                .and_then(|name| rules.object(name))
                .map(|free| free.cost),
            defense: object.build_cat == Some(BuildCategory::Combat),
            plant: object.factory_plant.then_some(object.cost_bonuses),
        }
    }

    #[allow(dead_code)]
    fn cost(&self, factors: CostFactors) -> i32 {
        // BuildingType45EDD0 calls shared711F00 for adjusted +AC, then adds
        // FreeUnit's +84 cost. Current retail country Cost*Mult values are1;
        // House factors are live50BF60 outputs, never raw Building Cost.
        let cost = scaled_cost(self.actual_cost, factors[if self.defense { 4 } else { 3 }]);
        //45EE47..55 clamps only the FreeUnit arm;45EE58 returns an
        // unbundled cost unchanged, including a negative adjusted cost.
        match self.free_unit_cost {
            Some(free) => cost.wrapping_add(scaled_cost(free, factors[1])).max(0),
            None => cost,
        }
    }
}

impl HouseBaseState {
    #[allow(dead_code)]
    fn weighted_center(
        &self,
        entities: &crate::sim::entity_store::EntityStore,
    ) -> (i32, (i16, i16)) {
        let (mut weight, mut x, mut y) = (0_i32, 0_i32, 0_i32);
        for &id in &self.buildings {
            let Some(entity) = entities.get(id) else {
                continue;
            };
            if entity.lifecycle.in_limbo || entity.health.current == 0 {
                continue;
            }
            let Some(entry) = self.registration(id) else {
                continue;
            };
            let count = (entry.cost(self.factors) / 1000).wrapping_add(1);
            if count <= 0 {
                continue;
            }
            let center = crate::sim::movement::ground_pose::object_center_coord_with_foundation(
                entity,
                &entity.foundation,
            );
            //4FD203..22D repeats a pure447AC0 getter and wrapping addition.
            // Multiplication preserves that modular sum without O(Cost) work.
            weight = weight.wrapping_add(count);
            x = x.wrapping_add(center.x.wrapping_mul(count));
            y = y.wrapping_add(center.y.wrapping_mul(count));
        }
        let seed = if weight > 0 {
            (((x / weight) / 256) as i16, ((y / weight) / 256) as i16)
        } else {
            (0, 0)
        };
        (weight, seed)
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
            Err(index) => {
                self.tracked_count = self
                    .tracked_count
                    .wrapping_add(i32::from(entry.tracks_base));
                self.registrations.insert(index, entry);
            }
        }
    }

    fn unregister(&mut self, id: u64) -> Option<RegisteredBuilding> {
        let index = self
            .registrations
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()?;
        let entry = self.registrations.remove(index);
        self.tracked_count = self
            .tracked_count
            .wrapping_sub(i32::from(entry.tracks_base));
        Some(entry)
    }

    fn remove_membership(&mut self, id: u64) {
        //4FB9B0 stable-removes one matching pointer, then50BF60 recomputes.
        remove_first(&mut self.plants, id);
        remove_first(&mut self.buildings, id);
        self.refresh_factors();
    }

    #[allow(dead_code)]
    fn append_membership(&mut self, id: u64) {
        //441545/44154E precede441594. These vectors have independent lives.
        if self
            .registration(id)
            .is_some_and(|entry| entry.plant.is_some())
        {
            self.plants.push(id);
            self.refresh_factors();
        }
        self.buildings.push(id);
    }

    fn refresh_factors(&mut self) {
        let mut factors = UNIT_FACTORS;
        for &id in &self.plants {
            if let Some(bonuses) = self.registration(id).and_then(|entry| entry.plant) {
                for (factor, bonus) in factors.iter_mut().zip(bonuses) {
                    *factor = multiply_factor(*factor, bonus);
                }
            }
        }
        self.factors = factors;
    }
}

fn remove_first(ids: &mut Vec<u64>, id: u64) {
    if let Some(index) = ids.iter().position(|&candidate| candidate == id) {
        ids.remove(index);
    }
}

///50BF60 multiplies two f32 operands exactly in x87, then stores toward zero.
/// The shared arithmetic owner includes gradual underflow after many stock
/// NAINDP plants. This receiver retains its masked-overflow result policy.
fn multiply_factor(lhs: NativeF32Bits, rhs: NativeF32Bits) -> NativeF32Bits {
    let product = X87Chop53::mul(
        X87Chop53::load_f32(lhs).expect("retail cost factors are finite"),
        X87Chop53::load_f32(rhs).expect("retail cost bonuses are finite"),
    );
    match X87Chop53::store_f32(product) {
        Ok(bits) => bits,
        // House50BF60 stores with exceptions masked and rounding toward zero.
        // Keep that receiver policy explicit; other users retain checked stores.
        // Native comparison: tools/spatial_oracle/factory_plant_factors.
        Err(NativeX87Error::StoreOverflow { format: "f32" }) => {
            NativeF32Bits::from_bits(((lhs.bits() ^ rhs.bits()) & 0x8000_0000) | 0x7f7f_ffff)
        }
        Err(error) => panic!("finite cost factor store failed: {error}"),
    }
}

#[allow(dead_code)]
fn scaled_cost(cost: i32, factor: NativeF32Bits) -> i32 {
    let product = X87Chop53::mul(
        X87Chop53::load_i32(cost),
        X87Chop53::load_f32(factor).expect("retail cost factors are finite"),
    );
    X87Chop53::ftol_i64(product).expect("retail cost products fit native ftol") as i32
}

/// Map586E50: correct the diagonal first, sample its Cell once, then walk
/// along the other diagonal until578460(mode1) admits the packed coordinate.
/// The loop's predicate owns subsequent Cell/Dummy lookups independently.
#[allow(dead_code)]
fn clamp_house_cell(
    input: (i16, i16),
    cells: &NativeCellQuery<'_>,
    bounds: crate::sim::cell_rect::PlayfieldBounds,
) -> Result<(i16, i16), String> {
    let (mut x, mut y) = (i32::from(input.0), i32::from(input.1));
    let right = bounds
        .off_fc
        .wrapping_add(bounds.off_104)
        .wrapping_mul(2)
        .wrapping_sub(bounds.base);
    let left = bounds.base.wrapping_sub(bounds.off_fc.wrapping_mul(2));
    if x.wrapping_sub(y) >= right {
        let delta = x.wrapping_sub(y).wrapping_sub(right).wrapping_add(2) / 2;
        x = x.wrapping_sub(delta);
        y = y.wrapping_add(delta);
    } else if y.wrapping_sub(x) >= left {
        let delta = y.wrapping_sub(x).wrapping_sub(left).wrapping_add(2) / 2;
        y = y.wrapping_sub(delta);
        x = x.wrapping_add(delta);
    }
    let cell = cells.lookup((x as i16, y as i16));
    let (raw_level, slope) = cells.ground_fields(cell);
    let mut level = i32::from(raw_level as i8);
    let top = bounds.base.wrapping_add(bounds.off_100.wrapping_mul(2));
    let sum = x.wrapping_add(y);
    if slope != 0 && sum < top.wrapping_add(4).wrapping_add(level) {
        level = level.wrapping_add(1);
    }
    let bottom = bounds
        .base
        .wrapping_add(bounds.off_100.wrapping_add(bounds.off_108).wrapping_mul(2))
        .wrapping_add(2)
        .wrapping_add(level);
    let step = if sum <= top.wrapping_add(level) {
        1
    } else if sum > bottom {
        -1
    } else {
        return Ok((x as i16, y as i16));
    };
    // Packed words repeat after65536 steps. A complete cycle with no admitted
    // Cell is malformed map authority; never invent a substitute destination.
    for _ in 0..=u16::MAX {
        x = x.wrapping_add(step);
        y = y.wrapping_add(step);
        if crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
            (x, y),
            Some(bounds),
            Some(cells.terrain()),
            Some(cells),
        ) {
            return Ok((x as i16, y as i16));
        }
    }
    Err("House map clamp has no admitted packed cell on its diagonal".into())
}

impl Simulation {
    /// Shared56DC20 argument shape used by4FD2C0 and5002E5. Their speed
    /// and required zone differ; neither requests occupancy or height filtering.
    #[allow(dead_code)]
    fn house_nearby_cell(
        &self,
        seed: (i32, i32),
        speed_type: SpeedType,
        required_zone_id: Option<ZoneId>,
        terrain: &ResolvedTerrainGrid,
    ) -> Option<(u16, u16)> {
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())?;
        let cells = NativeCellQuery::canonical(terrain);
        let grid = self.path_grid_snapshot();
        find_nearby_passable_cell(
            seed,
            &NearbyQuery {
                native_cells: Some(&cells),
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type,
                    required_zone_id,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: false,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: Some(terrain),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        )
        .filter(|cell| *cell != (0, 0))
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
        let entry = RegisteredBuilding::from_rules(id, object, rules);
        let owner = entity.owner();
        if let Some(house) = self.houses.get_mut(&owner) {
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
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn original_factory_plant_f32_fold_including_gradual_underflow() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/factory_plant_factors.json"
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
                    tracks_base: true,
                    actual_cost: 2500,
                    free_unit_cost: None,
                    defense: false,
                    plant: Some(bonus),
                });
                state.append_membership(id);
            }
            // Exercise the membership receiver used by production Limbo/expiry:
            // removing one plant recomputes the remaining ordered native fold.
            state.remove_membership(count);
            let expected: Vec<u32> = row["factor_bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|bits| bits.as_u64().unwrap() as u32)
                .collect();
            assert_eq!(
                state.factors.map(NativeF32Bits::bits).as_slice(),
                expected,
                "native FactoryPlant count{count}"
            );
        }
    }

    #[test]
    fn original_building_weight_cost_uses_free_unit_and_live_house_factors() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/building_weight_cost.json"
        ))
        .unwrap();
        for row in rows.as_array().unwrap() {
            let input = &row["input"];
            let free = input["free"].as_i64().unwrap();
            let ini = format!(
                "[BuildingTypes]\n0=BUILDING\n[VehicleTypes]\n0=FREE\n\
                [BUILDING]\nCost={}\n{}\n[FREE]\nCost={free}\n",
                input["cost"].as_i64().unwrap(),
                if free == 0 { "" } else { "FreeUnit=FREE" }
            );
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            let building =
                RegisteredBuilding::from_rules(1, rules.object("BUILDING").unwrap(), &rules);
            let mut factors = UNIT_FACTORS;
            factors[1] =
                NativeF32Bits::from_bits((input["unit"].as_f64().unwrap() as f32).to_bits());
            factors[3] =
                NativeF32Bits::from_bits((input["building"].as_f64().unwrap() as f32).to_bits());
            assert_eq!(
                building.cost(factors),
                row["cost84"].as_i64().unwrap() as i32,
                "native cost row{input}"
            );
        }
    }
}
