//! The House's building lists, retained base geometry and FactoryPlant factors.
//!
//! House+68 (buildings), House+110 (SecretLabs) and House+140 (FactoryPlants)
//! change membership at
//! Unlimbo, pointer expiry and ChangeOwner; constructor/destructor tracking
//! (`0x004FF700`/`0x004FF550`, `sim::house_tracking`) counts separately.
//! The absorbers list (House+0xB0) moves with House+68 under a type gate, so
//! it is read as a filtered view ([`Simulation::house_absorbers`]).
use super::*;
use crate::rules::ruleset::HouseCostFactors;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::houses::Houses;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::util::native_x87::NativeF32Bits;

type CostFactors = [NativeF32Bits; 5];
const UNIT_FACTORS: CostFactors = [NativeF32Bits::ONE; 5];

/// Immutable rule projections retained beside the House registration. This
/// lets Unlimbo, ChangeOwner and pointer expiry use the same inputs without a
/// borrowed RuleSet or per-tick string lookup. Registration order is not list
/// order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RegisteredBuilding {
    id: u64,
    /// The type's cost bonuses when it is a `FactoryPlant=` type.
    plant: Option<CostFactors>,
    /// Immutable BuildingType+16B0 projection for lifecycle list callbacks.
    secret_lab: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// House+110, appended441443/44908B, stable-removed448983/4FBB8E.
    secret_labs: Vec<u64>,
    /// Historical House+5498. Constructor4F5A53 stores zero; `4FD150`
    /// publishes it at Building lifecycle boundaries, not from a list on read.
    radius: i32,
}

impl std::hash::Hash for RegisteredBuilding {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.plant.hash(state);
        if self.secret_lab {
            b"secret-lab-type-v382".hash(state);
        }
    }
}

impl std::hash::Hash for HouseBaseState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.registrations.hash(state);
        self.buildings.hash(state);
        self.plants.hash(state);
        self.radius.hash(state);
        if !self.secret_labs.is_empty() {
            b"house-secret-labs-v382".hash(state);
            self.secret_labs.hash(state);
        }
    }
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
        let x = (sum_x / weight_total / 256) as i16;
        let y = (sum_y / weight_total / 256) as i16;
        sim.nearby_passable_cell(
            (i32::from(x), i32::from(y)),
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: SpeedType::Foot,
                    required_zone_id: None,
                    movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::SINGLE,
            ),
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
            secret_lab: object.secret_lab().enabled(),
        }
    }
}

impl HouseBaseState {
    /// The retained House4FD150 radius, also read by House500200/501AC0.
    pub(crate) const fn radius(&self) -> i32 {
        self.radius
    }

    /// `4FD150` publishes the radius at Building lifecycle boundaries.
    pub(crate) fn retain_radius(&mut self, radius: i32) {
        self.radius = radius;
    }

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

    /// House+110 in native vector order; no health/power filter at CanBuild.
    pub(crate) fn secret_labs(&self) -> &[u64] {
        &self.secret_labs
    }

    /// House+68 in vector order.
    pub(crate) fn buildings(&self) -> &[u64] {
        &self.buildings
    }

    /// An oracle row's House+68, in its order; an id no object holds stands
    /// for a null entry.
    #[cfg(any(test, feature = "test-support"))]
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
        remove_first(&mut self.secret_labs, id);
        remove_first(&mut self.plants, id);
        remove_first(&mut self.buildings, id);
    }

    /// The appends Unlimbo (`0x004414F1..0x00441594`) and ChangeOwner's new
    /// House (`0x00449139..0x004491D2`) make: a `FactoryPlant=` type joins
    /// House+140 and the factors are recomputed (`0x0050BF60`), then the
    /// building joins the House+68 tail. The two vectors have independent
    /// lives.
    fn append_membership(&mut self, id: u64) {
        if self.registration(id).is_some_and(|entry| entry.secret_lab) {
            self.secret_labs.push(id);
        }
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

impl Simulation {
    /// Original Execute30 at 0x006DE206..0x006DE2A5. The acquired BuildConst vector
    /// supplies its first raw Location, then an empty plan asks 0x0050C210 to
    /// choose again from the reverse general Building list. Those two orders
    /// can select different yards. Off changes only the existing +0x1F3 latch.
    pub(crate) fn trigger_auto_base_building(
        &mut self,
        owner: InternedId,
        value: i32,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(house) = self.houses.get_mut(&owner) else {
            return false;
        };
        if !house.set_auto_base_building_from_trigger(value) {
            return true;
        }
        let first = house.build_const_order().first().copied();
        if let Some(cell) = first.and_then(|id| self.raw_building_base_cell(id)) {
            let house = self.houses.get_mut(&owner).expect("retained owner");
            house.set_base_center(cell);
            house.anchor_base_plan(cell);
        }
        if self.houses[&owner].base_plan().nodes().is_empty() {
            return self.initialize_house_base_from_build_const(owner, rules, registry, effects);
        }
        true
    }

    /// House 0x0050C210..0x0050C332: the last non-limbo owned Building whose type
    /// belongs to Rules.BuildConst becomes the primary and node-zero anchor.
    /// No Alive/Health gate belongs to this list read. TypeFacts retains the
    /// existing Rules BuildConst membership used by all lifecycle writers.
    fn initialize_house_base_from_build_const(
        &mut self,
        owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let candidate = self.houses[&owner]
            .base_projection()
            .buildings()
            .iter()
            .rev()
            .find_map(|&id| {
                let building = self.substrate.entities.get(id)?;
                (!building.lifecycle.in_limbo() && building.type_facts.build_const_eligible())
                    .then_some(id)
            });
        let Some(cell) = candidate.and_then(|id| self.raw_building_base_cell(id)) else {
            return true;
        };
        self.houses
            .get_mut(&owner)
            .expect("retained owner")
            .set_base_center(cell);
        if self.session.game_mode_nonzero() && !self.houses[&owner].is_controlled_by_human(true) {
            self.computer_paranoid(rules, registry, effects);
        }
        // 0x0050C2F5 rereads the plan after the diplomacy callbacks. Keep Recalc's
        // existing owner and its Scenario RNG, clearing A8E7AC only around it.
        if self.houses[&owner].base_plan().nodes().is_empty() {
            let house = &self.houses[&owner];
            let Some(country) = house
                .country()
                .map(|id| self.interner.resolve(id).to_owned())
            else {
                return false;
            };
            let (side, difficulty, tech_level) =
                (house.side_index(), house.difficulty(), house.tech_level());
            if crate::sim::base_plan_generation::preflight_recalc(rules, &country, side, difficulty)
                .is_err()
            {
                return false;
            }
            self.without_object_placement_scope(|sim| {
                let house = sim.houses.get_mut(&owner).expect("retained owner");
                crate::sim::base_plan_generation::recalc_base_plan(
                    house.base_plan_mut(),
                    rules,
                    &sim.type_tech_levels,
                    &country,
                    side,
                    difficulty,
                    tech_level,
                    sim.session.game_options().super_weapons,
                    &mut sim.scenario_rng,
                );
            });
        }
        let house = self.houses.get_mut(&owner).expect("retained owner");
        house
            .base_plan_mut()
            .anchor_first_node(crate::sim::base_plan::pack_base_plan_cell(
                i32::from(cell.0 as i16),
                i32::from(cell.1 as i16),
            ));
        house.anchor_base_plan(cell);
        true
    }

    /// Both native callers read Object+0x9C directly, before signed /256 and
    /// short packing. Building GetCoords' foundation centre is a different read.
    fn raw_building_base_cell(&self, id: u64) -> Option<(u16, u16)> {
        let building = self.substrate.entities.get(id)?;
        let coord = crate::sim::movement::ground_pose::position_world_coord(&building.position);
        let (x, y) = crate::sim::movement::coord_cell(coord);
        Some((x as u16, y as u16))
    }
}

impl Simulation {
    /// `HouseClass::MoveGroundForcesToBase @ 0x0050C920`, which a computer
    /// House's Construction Yard deploy calls (`UnitClass::Deploy` at
    /// `0x00739926`, after the House's AI latches and before the MCV's slave
    /// transfer and UnInit). One Foot 4x4 nearby-cell search from the base
    /// origin, moved one cell south-east, becomes the destination of every
    /// eligible Techno; the class setters and locomotors own the travel.
    pub(super) fn move_ground_forces_to_base(
        &mut self,
        owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        // 0x0050C924 reads TechnoClass::Array's count before the search; the
        // walk re-reads each slot (0x0050C9D5).
        let count = self.substrate.entities.techno_registry_len();
        let Some(origin) = self.houses.get(&owner).map(HouseState::base_origin) else {
            return;
        };
        // 0x0050C966..0x0050C999: Foot, no zone, Normal, 4x4, bridges
        // allowed, no target cell. An empty result ends the call
        // (0x0050C9A7..0x0050C9BC); the offset (0x0050C9C2, 0x0050C9C7)
        // follows that test.
        let Some((x, y)) = self
            .nearby_passable_cell(
                (i32::from(origin.0), i32::from(origin.1)),
                NearbyArgs::plain(
                    PassabilityArgs {
                        speed_type: SpeedType::Foot,
                        required_zone_id: None,
                        movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
                        bridge_aware_zone: false,
                    },
                    NearbyFootprint::new(4, 4),
                ),
            )
            .filter(|&cell| cell != (0, 0))
        else {
            return;
        };
        let destination = NavTargetRef::cell(x.wrapping_add(1), y.wrapping_add(1));
        let move_mission = MissionId::from_known(MissionType::Move);
        for index in 0..count {
            let Some(id) = self.substrate.entities.techno_registry_at(index) else {
                continue;
            };
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            // 0x0050C9E6..0x0050CA3E: the House's own Technos, except
            // Buildings, Aircraft and types that deploy into a Construction
            // Yard. There is no health, limbo, Team, naval or mission filter.
            if entity.owner() != owner
                || matches!(
                    entity.category(),
                    EntityCategory::Structure | EntityCategory::Aircraft
                )
                || crate::sim::mcv_deploy::deploys_into(self, entity, rules)
                    .is_some_and(|target| target.construction_yard)
            {
                continue;
            }
            // 0x0050CA40..0x0050CA7B re-reads the base origin for each one.
            if self
                .houses
                .get(&owner)
                .is_none_or(|house| house.base_origin() == (0, 0))
            {
                continue;
            }
            // Assign_Destination(cell, 1) (vt+0x480, 0x0050CA93), a plain
            // archive clear (0x0070C610 at 0x0050CA9D), then
            // Queue_Mission(Move, 0) (vt+0x1E8, 0x0050CAAA) without commencing.
            let _ = self.assign_destination_represented(
                id,
                Some(destination),
                rules,
                registry,
                frame_effects,
            );
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.set_archive_target(None);
            }
            let _ = self.mission_queue_exact(
                id,
                move_mission,
                0,
                self.session.binary_frame(),
                &LiveReadyInputProvider { rules },
            );
        }
    }

    /// House50DF30: the existing alternate/primary base-cell owner, followed
    /// by Map5657A0 and CellGetCoords486840. An empty base is CoordEmpty.
    /// WhichZone4FFB20 inlines the same query; Home731773 calls it directly.
    pub(crate) fn house_base_coordinate(
        &self,
        owner: InternedId,
        cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    ) -> [i32; 3] {
        let origin = self
            .houses
            .get(&owner)
            .map_or((0, 0), HouseState::base_origin);
        if origin == (0, 0) {
            return [0, 0, 0];
        }
        // A fresh Map5657A0 lookup, which stamps the Dummy on a miss; not the
        // retained-Cell GetCoords that target_get_coords dispatches.
        let coord = crate::sim::movement::navcom::target_cell_coord(origin.0, origin.1, cells);
        [coord.x, coord.y, coord.z]
    }

    /// `HouseClass::Which_Zone @ 0x004FFB20` answering -1 for `owner`: `coord`
    /// is the empty coordinate (`0x00A8EFF8`, all zero) or lies more than four
    /// base radii (`+0x5498`) from the base cell's GetCoords (`0x00486840` on
    /// [`HouseState::base_origin`]; an empty base is the empty coordinate), by
    /// `CoordStruct::Distance3D @ 0x0041C380`. Its other answers (0 within one
    /// radius, 1..4 by direction from the base) are not ported: no caller
    /// reads them.
    ///
    /// [`HouseState::base_origin`]: crate::sim::house_state::HouseState::base_origin
    pub(crate) fn house_outside_zones(&self, owner: InternedId, coord: [i32; 3]) -> bool {
        if coord == [0, 0, 0] {
            return true;
        }
        let Some(house) = self.houses.get(&owner) else {
            return true;
        };
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let base = self.house_base_coordinate(owner, cells.as_ref());
        let distance = crate::util::native_x87::distance_3d_leptons(base, coord);
        let radius = house.base_radius();
        distance > radius && radius.wrapping_shl(2) < distance
    }

    /// Ordinary Unit/Infantry arm of House500200 (Find_Passable_Cell_Near_Unit)
    /// on `house` for `actor_id`. Callers: Foot Mission_Rescue (`0x004DE143`),
    /// Find_Path's failure arm (`0x004D4189`) and Unit Per_Cell_Process's
    /// war-factory post (`0x0073AA19`) ask the actor's own House;
    /// ExitObject's generic arm (`0x00444924`) and its
    /// Infantry post (`0x00444D51`) ask the producer's. Their original
    /// virtual+2D4/+2D8/+2DC bodies (41BF00/10/20) return zero, selecting
    /// House501AC0 variant0 without the alternative-selector draw. Other
    /// classes/variants are not covered here.
    /// Native comparison: spatial_oracle/house_base_return, all28 home rows.
    ///
    /// Returns None for native packed-zero FNPC or unavailable actor/House/map
    /// authority. Missing terrain/Size/LocalSize/zones consume no RNG and do
    /// not supply a substitute map. This is a query: no mission or radius write.
    pub(crate) fn house_return_cell(
        &mut self,
        house: InternedId,
        actor_id: u64,
    ) -> Option<(u16, u16)> {
        let actor = self.substrate.entities.get(actor_id)?;
        if !matches!(
            actor.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        ) {
            return None;
        }
        //50025F..5002C9 reads physical Object+9C/+A0, not Should_Be/NavCom or
        //a virtual bridge-height coordinate. OnBridge does not change its zone.
        let current = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
        let house = self.houses.get(&house)?;
        let radius = house.base_radius().clamp(768, 2048);
        let origin = house.base_origin();
        let terrain = self.resolved_terrain.as_ref()?;
        let bounds = self.playfield_bounds?;
        self.map_size_diamond()?;
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
        self.nearby_passable_cell(
            seed,
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: SpeedType::Track,
                    required_zone_id: Some(zone),
                    movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::SINGLE,
            ),
        )
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
        let mut buildings = Vec::with_capacity(house.base_projection().buildings.len());
        if house.tracking().buildings() > 0 {
            for &id in house.base_projection().buildings() {
                let Some(entity) = self.substrate.entities.get(id) else {
                    continue;
                };
                if entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
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
            house.tracking().buildings(),
            &buildings,
            house.alternate_base_center(),
        );
        self.houses
            .get_mut(&owner)
            .expect("projection House remains registered")
            .retain_base_geometry(primary, radius);
        true
    }

    /// `HouseClass::DetermineEdge @ 0x0050DB00`: the map edge `owner`'s
    /// aircraft come from and leave by (House+0x577C,
    /// [`HouseState::waypoint_edge`]). Its callers, SelectPrimaryFactory
    /// (`0x0044813D`) and TogglePrimaryFactory (`0x00448241`), pass a
    /// `Factory=BuildingType` Building of `owner`.
    ///
    /// - That Building, when it is primary (`+0x3D3`) and its type's Factory
    ///   is BuildingType (`0x0050DB23..0x0050DB6E`), is the anchor.
    /// - Otherwise, when House+0x2F0 is nonzero, the House+68 walk
    ///   (`0x0050DB73..0x0050DC34`) takes each non-limbo `ConstructionYard=`
    ///   Building in turn and stops at a primary one; without one, the first
    ///   other non-limbo Building. With neither, the edge stays (`0x0050DC86`).
    ///
    /// The anchor is the Building's GetCoords (`0x00447AC0`, its foundation
    /// centre); Map[coord] (`0x00565730`) gives the cell the edge choice
    /// ([`determine_waypoint_edge`]) measures from. Native marks "no anchor"
    /// with the zero CoordStruct (`0x00A8EFF8`), which a placed Building's
    /// GetCoords never equals (its X is at least 128).
    ///
    /// RESIDUAL: VERA's Unlimbo and ChangeOwner append the Building to
    /// House+68 before this call; native appends after it (`0x00441594`,
    /// `0x004491D2`). The walk runs only when another primary
    /// `Factory=BuildingType` Building kept the flag. Every retail one is a
    /// `ConstructionYard=` type, where the walk stops before that tail.
    ///
    /// A mapless fixture without PlayfieldBounds keeps its edge.
    ///
    /// [`determine_waypoint_edge`]: crate::sim::house_state::determine_waypoint_edge
    pub(crate) fn determine_house_edge(
        &mut self,
        owner: InternedId,
        building: u64,
        rules: &RuleSet,
    ) {
        let Some(bounds) = self.playfield_bounds else {
            return;
        };
        let primary_yard_factory = self.production.is_primary_factory(building)
            && self
                .substrate
                .entities
                .get(building)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .is_some_and(|object| {
                    object.factory_type()
                        == Some(crate::rules::object_type::FactoryType::BuildingType)
                });
        let anchor = if primary_yard_factory {
            Some(building)
        } else {
            let Some(house) = self.houses.get(&owner) else {
                return;
            };
            let mut yard = None;
            let mut other = None;
            if house.tracking().buildings() != 0 {
                for &id in house.base_projection().buildings() {
                    let Some(entity) = self.substrate.entities.get(id) else {
                        continue;
                    };
                    if entity.lifecycle.in_limbo() {
                        continue;
                    }
                    if self
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.construction_yard)
                    {
                        yard = Some(id);
                        if self.production.is_primary_factory(id) {
                            break;
                        }
                    } else if other.is_none() {
                        other = Some(id);
                    }
                }
            }
            yard.or(other)
        };
        let Some(cell) = anchor
            .and_then(|id| self.substrate.entities.get(id))
            .and_then(super::lifecycle::object_get_coords_cell)
        else {
            return;
        };
        let edge = crate::sim::house_state::determine_waypoint_edge(cell, bounds);
        if let Some(house) = self.houses.get_mut(&owner) {
            house.assign_waypoint_edge(edge);
        }
    }

    /// [`HouseState::cost_factors`] for `owner`. `None` without a House, which
    /// takes Cost_Of's null-House arm (`0x00711F4E`, the raw cost).
    pub(crate) fn house_cost_factors(
        houses: &Houses,
        owner: InternedId,
    ) -> Option<HouseCostFactors> {
        Some(houses.get(&owner)?.cost_factors())
    }

    /// TechnoType virtual `+0x84` for `owner`'s House ([`RuleSet::cost_of`]).
    pub fn cost_of(&self, owner: InternedId, object: &ObjectType, rules: &RuleSet) -> i32 {
        debug_assert!(
            self.houses.get(&owner).is_none_or(|house| {
                house.country_cost_mults().0
                    == rules.country_cost_mults(self.interner.resolve(house.house_type_id()))
            }),
            "a House's `Cost*Mult=` copy differs from its type's: project it at creation"
        );
        rules.cost_of(
            object,
            Self::house_cost_factors(&self.houses, owner).as_ref(),
        )
    }

    /// House+0x98, the grinders, in registration order. Grinding+16AD
    /// appends at441283..4412D1 and448ECB..448F19; ChangeOwner4487E6..
    /// 44882E and expiry4FBA99..4FBABB stable-remove. Like the absorber
    /// list, this is the existing House+68 registry filtered by type.
    pub(crate) fn house_grinders(&self, owner: InternedId, rules: &RuleSet) -> Vec<u64> {
        self.house_buildings_where(owner, rules, |object| object.grinding)
    }

    /// Rated input to PowerClass63F850, from the canonical House+68 list.
    /// Original63F889..63F8AB sums Type+EE0/+EE4 without live-state or dirty
    /// assessment gates. BuildingType461060..46109A splits signed `Power`
    /// into these fields; wrapping abs/add also preserve NEG MIN and ADD32.
    /// Executed list controls: tools/sidebar_oracle/power_state.json.
    /// Missing Rust records/types retain the other House projections' safe
    /// fallback for incomplete worlds; native assumes valid listed pointers.
    pub fn theoretical_power_for_owner(&self, rules: &RuleSet, owner: &str) -> i32 {
        let Some(house) = self.interner.get(owner).and_then(|id| self.houses.get(&id)) else {
            return 0;
        };
        house
            .base_projection()
            .buildings()
            .iter()
            .filter_map(|&id| {
                self.substrate
                    .entities
                    .get(id)
                    .and_then(|entity| self.object_type(entity.type_ref(), rules))
            })
            .fold(0_i32, |total, object| {
                total.wrapping_add(object.power.wrapping_abs())
            })
    }

    /// House+0x80, the UnitRepair stations (Items `+0x84`, Count `+0x90`), in
    /// vector order. Unlimbo appends a `UnitRepair=` building (Type
    /// `+0x16A9`, `0x0044122C..0x0044127A`) ahead of its House+0xB0 and
    /// House+68 appends; ChangeOwner removes it from the old House
    /// (`0x00448790..0x004487E0`) and appends it to the new one
    /// (`0x00448E6E..0x00448EC5`) under the same gate, ahead of the same
    /// lists; pointer expiry stable-removes it (`0x004FBA59..0x004FBA99`). As
    /// with [`Self::house_absorbers`], it is House+68 filtered by the gate,
    /// in order.
    pub(crate) fn house_unit_repair_stations(
        &self,
        owner: InternedId,
        rules: &RuleSet,
    ) -> Vec<u64> {
        self.house_buildings_where(owner, rules, |object| object.unit_repair)
    }

    /// House+0xB0, the absorbers (Items `+0xB4`, Count `+0xC0`), in vector
    /// order. Unlimbo appends a `UnitAbsorb=`/`InfantryAbsorb=` building
    /// (Type `+0x16AE`/`+0x16AF`, `0x004412D4..0x00441335`) ahead of its
    /// House+68 append; ChangeOwner removes it from the old House ahead of
    /// House+68 (`0x00448836..0x0044885A`) and appends it to the new one
    /// ahead of House+68 (`0x00448F22..0x00448F42`), under the same gate;
    /// pointer expiry stable-removes it from both (`0x004FBAC0..0x004FBAFE`,
    /// `0x004FBC1C`). Each list therefore sees the same appends and
    /// removals, and House+0xB0 is House+68 filtered by the gate, in order.
    pub(crate) fn house_absorbers(&self, owner: InternedId, rules: &RuleSet) -> Vec<u64> {
        self.house_buildings_where(owner, rules, |object| object.absorbs_passengers())
    }

    /// House+0xC8, the tank bunkers (Items `+0xCC`, Count `+0xD8`), in
    /// vector order. Unlimbo appends a `Bunker=` building (Type `+0x16AB`,
    /// `0x00441335..0x0044138C`) between its House+0xB0 and House+0xE0
    /// appends; ChangeOwner moves it under the same gate between those two
    /// lists' moves (`0x0044888A..0x004488DA` from the old House,
    /// `0x00448F7D..0x00448FD4` into the new); pointer expiry stable-removes
    /// it (`0x004FBB00..0x004FBB27`). As with [`Self::house_absorbers`], it
    /// is House+68 filtered by the gate, in order.
    pub(crate) fn house_bunkers(&self, owner: InternedId, rules: &RuleSet) -> Vec<u64> {
        self.house_buildings_where(owner, rules, |object| object.bunker)
    }

    /// House+0xE0, the occupiable buildings (Items `+0xE4`, Count `+0xF0`),
    /// in vector order. Unlimbo appends a `CanBeOccupied=` building whose
    /// `TechLevel=` is above -1 (Type `+0x157B`, `+0x634`;
    /// `0x0044138C..0x004413EC`) between its House+0xB0 and House+68
    /// appends; ChangeOwner moves it under the same gate between those two
    /// lists' moves (`0x004488E0..0x00448933`, `0x00448FDA..0x00449030`);
    /// pointer expiry stable-removes it (`0x004FBB27..0x004FBB67`). As with
    /// [`Self::house_absorbers`], it is House+68 filtered by the gate, in
    /// order. On retail rules only the Battle Bunker (`NABNKR`) passes.
    pub(crate) fn house_occupiables(&self, owner: InternedId, rules: &RuleSet) -> Vec<u64> {
        // RESIDUAL: native retains membership at Unlimbo/ChangeOwner/expiry;
        // 106 does not rebuild it. Keep the existing initial-type filter here
        // rather than introducing a new live recomputation after106. A map
        // changing an occupiable BuildingType needs the separate list-lifecycle
        // port; the ALL05 aircraft production chain does not visit this list.
        self.house_buildings_where(owner, rules, |object| {
            object.can_be_occupied && object.tech_level > -1
        })
    }

    /// The house's buildings (House+68, in order) whose type passes `gate`.
    fn house_buildings_where(
        &self,
        owner: InternedId,
        rules: &RuleSet,
        gate: impl Fn(&ObjectType) -> bool,
    ) -> Vec<u64> {
        let Some(house) = self.houses.get(&owner) else {
            return Vec::new();
        };
        house
            .base_projection()
            .buildings()
            .iter()
            .copied()
            .filter(|&id| {
                self.substrate
                    .entities
                    .get(id)
                    .and_then(|entity| self.object_type(entity.type_ref(), rules))
                    .is_some_and(&gate)
            })
            .collect()
    }

    pub(super) fn register_house_base_building(
        interner: &StringInterner,
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        id: u64,
        rules: &RuleSet,
    ) {
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure {
            return;
        }
        let Some(object) =
            interner.object_type_in_category(entity.type_ref(), entity.category().into(), rules)
        else {
            return;
        };
        let entry = RegisteredBuilding::new(id, object);
        let owner = entity.owner();
        if let Some(house) = houses.get_mut(&owner) {
            house.base_projection_mut().register(entry);
        }
    }

    /// `BuildingClass::Unlimbo`'s House+68 append (`0x00441553..0x00441594`),
    /// reached on the same path as its BuildConst append
    /// ([`Simulation::append_live_build_const`]).
    pub(super) fn append_house_base_building(
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        id: u64,
    ) {
        let Some(owner) = substrate
            .entities
            .get(id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(|entity| entity.owner())
        else {
            return;
        };
        if let Some(house) = houses.get_mut(&owner) {
            house.base_projection_mut().append_membership(id);
        }
    }

    /// For a fixture that stores a building directly: the House+68 append its
    /// Unlimbo would make ([`Self::append_house_base_building`]).
    #[cfg(any(test, feature = "test-support"))]
    pub fn append_house_base_building_for_test(&mut self, id: u64) {
        Self::append_house_base_building(&self.substrate, &mut self.houses, id);
    }

    pub(super) fn join_house_base_lists(
        &mut self,
        id: u64,
        old_owner: InternedId,
        owner: InternedId,
        rules: &RuleSet,
    ) {
        // Building448C76/448C81: old list already removed and Techno tracking
        // moved, but neither new FactoryPlant nor House+68 append has run.
        self.recalculate_house_base_geometry(old_owner, rules);
        self.recalculate_house_base_geometry(owner, rules);
        if let Some(house) = self.houses.get_mut(&owner) {
            house.base_projection_mut().append_membership(id);
        }
    }

    pub(super) fn remove_house_base_membership(
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        id: u64,
    ) {
        if !substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            return;
        }
        // Broadcast visits every House, including historical non-owner lists.
        for house in houses.values_mut() {
            house.base_projection_mut().remove_membership(id);
        }
    }

    pub(super) fn release_house_base_tracking(
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        id: u64,
    ) {
        if let Some(owner) = substrate.entities.get(id).map(|entity| entity.owner())
            && let Some(house) = houses.get_mut(&owner)
        {
            //43BF34 changes tracking AFTER the earlier expiry/Limbo. No recalc.
            house.base_projection_mut().unregister(id);
        }
    }
}

impl crate::sim::houses::Houses {
    /// `BuildingClass::ChangeOwner`'s moves between the two Houses' lists:
    /// before the Techno owner swap it stable-removes the building from the
    /// old House's FactoryPlant list, recomputes and removes it from House+68
    /// (`0x00448A21..0x00448AB0`); after it, it appends to the new House's
    /// lists (`0x00449155..0x004491D2`).
    pub(super) fn leave_house_base_lists(&mut self, id: u64, owner: InternedId) {
        if let Some(house) = self.get_mut(&owner) {
            house.base_projection_mut().remove_membership(id);
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
            .get_mut(&old_owner)
            .and_then(|house| house.base_projection_mut().unregister(id))
        else {
            return;
        };
        if let Some(house) = self.get_mut(&new_owner) {
            house.base_projection_mut().register(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::Health;
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
        sim.session.set_map_width_for_test(17);
        sim.session.set_map_height_for_test(17);
        sim.session.set_binary_frame_for_test(100);
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        sim
    }

    fn add_house(sim: &mut Simulation, name: &str, rules: &RuleSet) -> InternedId {
        let owner = sim.interner.intern(name);
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
        house.project_country_mults(rules, &sim.interner);
        sim.houses.register(house);
        owner
    }

    fn geometry(sim: &Simulation, owner: InternedId) -> (Option<(u16, u16)>, i32) {
        let house = &sim.houses[&owner];
        (house.base_center(), house.base_radius())
    }

    fn ordinary_rules() -> RuleSet {
        RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n1=Russians\n\
             [BuildingTypes]\n0=FIRST\n1=SECOND\n\
             [FIRST]\nCost=2000\nStrength=1000\nFoundation=1x1\n\
             [SECOND]\nCost=1000\nStrength=1000\nFoundation=1x1\n",
        ))
        .unwrap()
    }

    #[test]
    fn theoretical_power_reads_native_house_list_without_assessment() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/power_state.json",
        ))
        .unwrap();
        // Original63F850 observes Type+EE0/+EE4 through House+68. These
        // are supplied registered-state fixtures, not native frame timing.
        for row in corpus["rated_lists"].as_array().unwrap() {
            let powers = row["powers"].as_array().unwrap();
            let power = |index: usize| powers.get(index).and_then(|p| p.as_i64()).unwrap_or(0);
            let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
                "[Countries]\n0=Americans\n1=Russians\n\
                 [BuildingTypes]\n0=FIRST\n1=SECOND\n\
                 [FIRST]\nCost=2000\nStrength=1000\nFoundation=1x1\nPower={}\n\
                 [SECOND]\nCost=1000\nStrength=1000\nFoundation=1x1\nPower={}\n",
                power(0),
                power(1),
            )))
            .unwrap();
            let mut sim = native_map_fixture();
            let owner = add_house(&mut sim, "Americans", &rules);
            for (index, kind) in ["FIRST", "SECOND"].iter().take(powers.len()).enumerate() {
                sim.spawn_object(kind, "Americans", 5 + index as u16 * 4, 5, 0, &rules)
                    .unwrap();
            }
            assert_eq!(
                sim.houses[&owner].base_projection().buildings().len(),
                powers.len()
            );
            let before = sim.state_hash();
            assert_eq!(
                sim.theoretical_power_for_owner(&rules, "Americans"),
                row["total"].as_i64().unwrap() as i32,
                "original House list sum: {}",
                row["name"],
            );
            assert_eq!(sim.state_hash(), before, "read-only query: {}", row["name"]);
            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "rated-power", 0);
            let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            assert_eq!(
                restored.theoretical_power_for_owner(&rules, "Americans"),
                row["total"].as_i64().unwrap() as i32,
                "restored canonical membership: {}",
                row["name"],
            );
            if row["name"] == "stock-mixed" {
                let native_total = |name| {
                    corpus["rated_lists"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|case| case["name"] == name)
                        .unwrap()["total"]
                        .as_i64()
                        .unwrap() as i32
                };
                let ids = sim.houses[&owner].base_projection().buildings().to_vec();
                let other = add_house(&mut sim, "Russians", &rules);
                sim.change_owner_with_rules(ids[0], other, &rules, None, FrameEffects::default());
                assert_eq!(
                    sim.theoretical_power_for_owner(&rules, "Americans"),
                    native_total("stock-drain")
                );
                assert_eq!(
                    sim.theoretical_power_for_owner(&rules, "Russians"),
                    native_total("stock-output")
                );
                assert_eq!(
                    sim.techno_limbo_with_rules(ids[1], &rules, None),
                    lifecycle::ConcealOutcome::Concealed
                );
                assert_eq!(
                    sim.theoretical_power_for_owner(&rules, "Americans"),
                    native_total("empty")
                );
                sim.uninit_with_rules(ids[0], &rules);
                assert_eq!(
                    sim.theoretical_power_for_owner(&rules, "Russians"),
                    native_total("empty")
                );
            }
        }
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
                let target = terrain.cell_mut_for_test(x, y).unwrap();
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
                    target.land_type = land as u8;
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
        let base = zones.base_topology_mut_for_test();
        base.zone_ids_mut_for_test().fill(0);
        base.movement_classes_mut_for_test().fill(0);
        let row = crate::rules::locomotor_type::MovementZone::Normal
            .matrix_row()
            .unwrap();
        // Supplied native query premises use one shared Map+4C label count
        // for all13 Map+18 rows. Only Normal's values are exercised here, but
        // the unused rows must still form a coherent saved navigation owner.
        let labels = overrides.map_or(0, Vec::len) + 1;
        for raw_row in base.raw_zone_ids_by_row_mut_for_test() {
            raw_row.resize(labels, 1);
        }
        base.raw_zone_ids_by_row_mut_for_test()[row].fill(1);
        if let Some(overrides) = overrides {
            for (index, zone) in overrides.iter().enumerate() {
                let x = zone["xy"][0].as_u64().unwrap() as usize;
                let y = zone["xy"][1].as_u64().unwrap() as usize;
                base.zone_ids_mut_for_test()[y * 17 + x] = index as u16 + 1;
                base.raw_zone_ids_by_row_mut_for_test()[row][index + 1] =
                    zone["group"].as_u64().unwrap() as u16;
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
        house.set_alternate_base_center_for_test(cell("alternate", (0, 0)));
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
            Health::new(100),
            type_ref,
            category,
            0,
            0,
            false,
        );
        // The signed-current stress row uses retained negative subcoordinates
        // at unsigned cell0; position_world_coord reconstructs its raw leptons.
        // This is an explicit nonretail physical input, not placement evidence.
        actor
            .position
            .set_sub_x_for_test(SimFixed::from_num(current[0] - i32::from(rx) * 256));
        actor
            .position
            .set_sub_y_for_test(SimFixed::from_num(current[1] - i32::from(ry) * 256));
        actor
            .position
            .set_exact_z_leptons_for_test(Some(current[2]));
        actor.set_on_bridge(input["on_bridge"].as_bool().unwrap_or(false));
        sim.substrate.entities.insert(actor);
        sim.scenario_rng = crate::sim::rng::SimRng::new(input["seed"].as_u64().unwrap());
        (sim, id, owner)
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
            let (answer, trace) = crate::sim::rng::trace_draws(|| sim.house_return_cell(owner, id));
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
            house.tracking_mut().set_buildings_for_test(tracked);
            house.set_alternate_base_center_for_test(alternate);
            house.retain_base_geometry(Some((14, 14)), 9999);
            if let Some(bonus) = input["unit_bonus"].as_f64() {
                let mut factors = UNIT_FACTORS;
                factors[1] = NativeF32Bits::from_bits((bonus as f32).to_bits());
                // The oracle supplies House+5394 directly. This list recreates
                // that input through the existing factor owner, without adding
                // a building to the oracle's separate House+68 premise.
                house.base_projection_mut().register(RegisteredBuilding {
                    id: u64::MAX,
                    plant: Some(factors),
                    secret_lab: false,
                });
                house.base_projection_mut().plants.push(u64::MAX);
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
                    Health::new(health),
                    type_ref,
                    EntityCategory::Structure,
                    0,
                    0,
                    false,
                );
                entity.lifecycle.set_in_limbo_for_test(limbo);
                entity
                    .position
                    .set_sub_x_for_test(SimFixed::from_num(xyz[0] % 256));
                entity
                    .position
                    .set_sub_y_for_test(SimFixed::from_num(xyz[1] % 256));
                entity.position.set_exact_z_leptons_for_test(Some(xyz[2]));
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
                    .base_projection_mut()
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
        assert_eq!(sim.houses[&owner].base_projection().buildings(), [first]);
        let second = sim
            .spawn_object("SECOND", "Americans", 13, 9, 0, &rules)
            .unwrap();
        assert_eq!(
            sim.houses[&owner].base_projection().buildings(),
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
        assert_eq!(sim.houses[&owner].base_projection().buildings(), [first]);
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
        assert_eq!(sim.houses[&owner].tracking().buildings(), 0);
        assert_eq!(
            geometry(&sim, owner),
            (None, 512),
            "destructor does not recalculate"
        );
    }

    #[test]
    fn failed_mark_and_dead_unlimbo_do_not_publish_geometry() {
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
                entity.lifecycle.set_object_alive_for_test(false);
            }
            let result = sim.try_reveal_entity_with_context(
                id,
                RevealRequest {
                    position: RevealPosition::new(
                        cell.0,
                        cell.1,
                        0,
                        None,
                        SimFixed::from_num(128),
                        SimFixed::from_num(128),
                    ),
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
            assert!(sim.houses[&owner].base_projection().buildings().is_empty());
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
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_alternate_base_center_for_test((1, 1));
        assert!(sim.recalculate_house_base_geometry(owner, &rules));
        // Saved native alternate-origin projection row.
        assert_eq!(geometry(&sim, owner), (Some((8, 7)), 1028));
        let native_hash = sim.state_hash();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .base_projection_mut()
            .radius = 1029;
        assert_ne!(sim.state_hash(), native_hash);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .base_projection_mut()
            .radius = 1028;
        assert_eq!(sim.state_hash(), native_hash);
        // A primary writer retains radius; it also leaves the alternate and
        // BasePlan centre untouched. Historical geometry need not equal lists.
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_base_center((14, 14));
        house.set_base_plan_center_for_test((3, 4));
        // Native load constructs the Scenario RNG with seed0. This test pins
        // geometry/list persistence, not a different RNG restore contract.
        // The native-map fixture also supplies navigation; publish and restore
        // that owner through the production map seam, which finishes the
        // deliberately pending post-deserialization hierarchy.
        let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(
            terrain.width(),
            terrain.height(),
        ));
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        sim.restore_map_authority_after_snapshot_load(&rules, &overlays, None)
            .expect("publish fixture map authority");
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let before = sim.state_hash();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "house-base-geometry", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &rules,
        );
        restored
            .restore_map_authority_after_snapshot_load(&rules, &overlays, None)
            .expect("restore fixture map authority and navigation");
        assert_eq!(
            restored.state_hash(),
            before,
            "saved owners {:?}; restored owners {:?}",
            sim.hash_owner_diagnostics_for_test(),
            restored.hash_owner_diagnostics_for_test(),
        );
        assert_eq!(geometry(&restored, owner), (Some((14, 14)), 1028));
        assert_eq!(restored.houses[&owner].alternate_base_center(), (1, 1));
        assert_eq!(restored.houses[&owner].base_plan_center(), (3, 4));
        assert_eq!(
            restored.houses[&owner].base_projection().buildings(),
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
                    secret_lab: false,
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
        use crate::sim::house_state::HouseState;

        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
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
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        let [allies, soviets] = ["Americans", "Russians"].map(|name| sim.interner.intern(name));
        for house in [allies, soviets] {
            let mut state = HouseState::new(house, 0, None, false, 0, 10);
            state.project_country_mults(&rules, &sim.interner);
            sim.houses.register(state);
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
                .any(|house| !house.base_projection().factory_plants().is_empty())
        };
        assert_eq!(costs(&sim), (900, 450));
        assert!(!plant_fold(&sim));
        let plant = sim
            .spawn_object("NAINDP", "Russians", 5, 5, 0, &rules)
            .unwrap();
        // ftol(900 * 0.75 * 0.5) = ftol(337.5)
        assert_eq!(costs(&sim), (900, 337));
        assert_eq!(
            sim.houses[&soviets].base_projection().factory_plants(),
            [plant]
        );
        assert!(plant_fold(&sim));
        sim.change_owner_with_rules(
            plant,
            allies,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(costs(&sim), (675, 450));
        assert!(
            sim.houses[&soviets]
                .base_projection()
                .factory_plants()
                .is_empty()
        );
        assert_eq!(
            sim.houses[&allies].base_projection().factory_plants(),
            [plant]
        );
        sim.uninit_with_rules(plant, &rules);
        assert_eq!(costs(&sim), (900, 450));
        assert!(!plant_fold(&sim));
    }

    /// `HouseClass::MoveGroundForcesToBase @ 0x0050C920` on its own. Water
    /// inside the base origin's 4x4 block makes the search's footprint
    /// matter. The House's Unit and Infantry share one destination at the
    /// found cell +1/+1, lose their archive target and only queue Move. Its
    /// Building, Aircraft and Construction Yard maker, and another computer
    /// House's Unit, keep their orders.
    #[test]
    fn moving_ground_forces_to_base_queues_one_shared_move() {
        use crate::rules::locomotor_type::{MovementZone, SpeedType};
        use crate::sim::combat::TargetKind;

        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n1=Russians\n\
             [InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n1=AMCV\n\
             [AircraftTypes]\n0=ORCA\n[BuildingTypes]\n0=YARD\n\
             [MTNK]\nStrength=400\nSpeed=6\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [AMCV]\nStrength=1000\nSpeed=5\nDeploysInto=YARD\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [E1]\nStrength=125\nSpeed=4\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [ORCA]\nStrength=150\nSpeed=8\n\
             Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n\
             [YARD]\nStrength=1000\nConstructionYard=yes\nFoundation=1x1\n\
             [Water]\nFoot=0%\nTrack=0%\nWheel=0%\nFloat=100%\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        crate::sim::arena_fixture::flood(&mut sim, &rules, 32, [(13, 13)]);
        let owner = add_house(&mut sim, "Americans", &rules);
        add_house(&mut sim, "Russians", &rules);
        let spawn = |sim: &mut Simulation, kind, house, x, y| {
            let id = sim.spawn_object(kind, house, x, y, 0, &rules).unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .set_archive_target(Some(TargetKind::Cell(x, y)));
            id
        };
        let tank = spawn(&mut sim, "MTNK", "Americans", 4, 20);
        let infantry = spawn(&mut sim, "E1", "Americans", 6, 22);
        let kept = [
            spawn(&mut sim, "AMCV", "Americans", 8, 24),
            spawn(&mut sim, "ORCA", "Americans", 10, 26),
            spawn(&mut sim, "YARD", "Americans", 22, 4),
            spawn(&mut sim, "MTNK", "Russians", 26, 20),
        ];
        let all = [tank, infantry, kept[0], kept[1], kept[2], kept[3]];
        let snapshot = |sim: &Simulation| {
            all.map(|id| {
                let entity = sim.substrate.entities.get(id).unwrap();
                (
                    entity.archive_target(),
                    entity.navigation.nav_com(),
                    entity.mission.current(),
                    entity.mission.queued(),
                )
            })
        };
        let before = snapshot(&sim);

        // An empty base origin ends the call before the search.
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_base_center_for_test(None);
        house.set_alternate_base_center_for_test((0, 0));
        sim.move_ground_forces_to_base(owner, &rules, None, FrameEffects::default());
        assert_eq!(snapshot(&sim), before);

        let origin = (12, 12);
        sim.houses.get_mut(&owner).unwrap().set_base_center(origin);
        let search = |seed: (u16, u16), (width, height)| {
            sim.nearby_passable_cell(
                (i32::from(seed.0), i32::from(seed.1)),
                NearbyArgs::plain(
                    PassabilityArgs {
                        speed_type: SpeedType::Foot,
                        required_zone_id: None,
                        movement_zone: MovementZone::Normal,
                        bridge_aware_zone: false,
                    },
                    NearbyFootprint::new(width, height),
                ),
            )
        };
        let found = search(origin, (4, 4)).unwrap();
        assert_eq!(search(origin, (1, 1)), Some(origin), "1x1 differs");
        assert_ne!(found, origin, "the water blocks the origin's 4x4 block");
        let destination = Some(NavTargetRef::cell(found.0 + 1, found.1 + 1));
        sim.move_ground_forces_to_base(owner, &rules, None, FrameEffects::default());

        let after = snapshot(&sim);
        let move_mission = MissionId::from_known(MissionType::Move);
        for index in 0..2 {
            let current = before[index].2;
            assert_eq!(after[index], (None, destination, current, move_mission));
            assert_ne!(current, move_mission, "Move is queued, not commenced");
        }
        assert_eq!(after[2..], before[2..]);
    }
}

#[cfg(test)]
#[path = "house_edge_tests.rs"]
mod edge_tests;
