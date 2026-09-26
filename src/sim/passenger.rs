//! Passenger/transport system — boarding, unloading, and cargo tracking.
//!
//! Handles infantry entering transports (Passengers>0), building garrisons
//! (CanBeOccupied=yes), IFV weapon swapping (Gunner=yes), and passenger
//! death on transport destruction. Vehicle/aircraft unloading is the Unload
//! mission handler in `crate::sim::transport_unload`; only garrison eviction
//! stays on the per-tick `OrderIntent::Unloading` path here. The private
//! departure module owns cargo-head removal and complete retry restoration for
//! garrisons, vehicles, landed aircraft and paradrops.
//!
//! ## Original engine reference
//! The original engine uses a linked-list at offsets +0x1D0/+0x1CC for passenger
//! storage; we use Vec<u64> for simplicity.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ (ObjectType), sim/game_entity, sim/entity_store.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

mod departure;
pub(crate) use departure::{
    DepartureFailure, DepartureRoute, depart_cargo_head, reveal_unloaded_passenger,
};

use std::collections::BTreeMap;

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::OrderIntent;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::movement;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::{ConcealOutcome, SimSoundEvent, Simulation};
use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

/// Passenger cargo state, attached as `Option<PassengerCargo>` on transport entities.
///
/// Tracks which entities are currently inside this transport/garrison.
/// Passengers are stored as a Vec of stable_ids in native CargoClass list
/// order: index 0 is the list HEAD. `CargoClass::AddPassenger @ 0x004733A0`
/// PREPENDS (`passenger->next = head; head = passenger`, `0x004733FA`..
/// `0x00473400`) and `CargoClass::RemoveFirstPassenger @ 0x00473430` pops the
/// head, so every native consumer — transport unload, paradrop, sell eject —
/// releases the LAST boarded passenger first.
///
/// RESIDUAL: a garrisoned building's occupants natively live in a separate
/// `Occupants` vector that APPENDS (`AddGarrisonOccupant 0x00522947..
/// 0x0052297C`), so its firing order (`+0x69C`, index 0 first) is entry
/// order, while a sale removes them last to first (`0x004580A9..
/// 0x0045819E`). VERA keeps garrisons in this cargo's prepend order, so a
/// garrison fires newest-first and the kill credit (`Record_The_Kill`'s
/// occupant arm) walks the same reversed line. Trigger: a garrison holding
/// two or more occupants. Effect: which occupant fires, and is paid, next.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PassengerCargo {
    /// Stable IDs of entities currently inside, head first: the most recently
    /// boarded passenger sits at index 0 (LIFO unload).
    pub passengers: Vec<u64>,
    /// `Size=` captured for each entry in `passengers`, at the same index.
    ///
    /// CargoClass can remove an individual expiring object without receiving
    /// its size as a separate argument. Keeping the admission-time size with
    /// the entry gives the Rust cargo list the same exact bookkeeping ability.
    #[serde(default)]
    pub(crate) passenger_sizes: Vec<u32>,
    /// Maximum passenger count (from Passengers= or MaxNumberOccupants= in rules.ini).
    pub capacity: u32,
    /// Maximum Size= of individual passenger allowed (from SizeLimit= in rules.ini).
    /// 0 means no size restriction.
    pub size_limit: u32,
    /// Total Size units currently occupied (sum of passenger Size= values).
    pub total_size: u32,
    /// Round-robin index for garrison fire — which occupant fires next.
    /// Matches gamemd BuildingClass+0x69C (CurrentFireIdx). Init 0, advanced
    /// by garrison combat after each shot: `(idx + 1) % occupant_count`.
    pub garrison_fire_index: u8,
}

impl PassengerCargo {
    pub fn new(capacity: u32, size_limit: u32) -> Self {
        Self {
            passengers: Vec::new(),
            passenger_sizes: Vec::new(),
            capacity,
            size_limit,
            total_size: 0,
            garrison_fire_index: 0,
        }
    }

    /// Number of passengers currently inside.
    pub fn count(&self) -> u32 {
        self.passengers.len() as u32
    }

    /// Whether the transport has room for a passenger of the given size.
    pub fn can_accept(&self, passenger_size: u32) -> bool {
        self.count() < self.capacity && (self.size_limit == 0 || passenger_size <= self.size_limit)
    }

    /// Add a passenger at the list head (`CargoClass::AddPassenger @
    /// 0x004733A0` prepends). Returns false if full or too large.
    pub fn board(&mut self, stable_id: u64, passenger_size: u32) -> bool {
        if !self.can_accept(passenger_size) {
            return false;
        }
        self.passengers.insert(0, stable_id);
        self.passenger_sizes.insert(0, passenger_size);
        self.total_size += passenger_size;
        true
    }

    /// Add a passenger without normal transport capacity/size gates.
    ///
    /// Standard paradrop superweapon loading uses CargoClass::AddPassenger on
    /// limbo-created infantry; it is driven by `*ParaDropNum`, not by PDPLANE's
    /// `Passengers=` or `SizeLimit=`.
    pub fn board_forced(&mut self, stable_id: u64, passenger_size: u32) {
        self.passengers.insert(0, stable_id);
        self.passenger_sizes.insert(0, passenger_size);
        self.total_size += passenger_size;
    }

    /// Remove a specific passenger. Returns true if found and removed.
    pub fn disembark(&mut self, stable_id: u64) -> bool {
        if let Some(pos) = self.passengers.iter().position(|&id| id == stable_id) {
            self.passengers.remove(pos);
            let passenger_size = self.passenger_sizes.remove(pos);
            self.total_size = self.total_size.saturating_sub(passenger_size);
            true
        } else {
            false
        }
    }

    /// Remove and return the list HEAD (the most recently boarded passenger)
    /// and its recorded size — `CargoClass::RemoveFirstPassenger @ 0x00473430`.
    pub(crate) fn unload_first(&mut self) -> Option<(u64, u32)> {
        if self.passengers.is_empty() {
            None
        } else {
            let id = self.passengers.remove(0);
            let passenger_size = self.passenger_sizes.remove(0);
            self.total_size = self.total_size.saturating_sub(passenger_size);
            Some((id, passenger_size))
        }
    }

    /// Restore a failed head-pop at the cargo head (the native failure path
    /// re-runs `AddPassenger`, which prepends).
    fn restore_front(&mut self, stable_id: u64, passenger_size: u32) {
        self.passengers.insert(0, stable_id);
        self.passenger_sizes.insert(0, passenger_size);
        self.total_size += passenger_size;
    }

    /// Is the cargo hold empty?
    pub fn is_empty(&self) -> bool {
        self.passengers.is_empty()
    }

    /// Clear the live CargoClass contents while preserving type configuration.
    pub(crate) fn clear_contents(&mut self) {
        self.passengers.clear();
        self.passenger_sizes.clear();
        self.total_size = 0;
        self.garrison_fire_index = 0;
    }

    /// Detach the complete cargo list for recursive ObjectClass::UnInit.
    ///
    /// Capacity and SizeLimit are type configuration and survive the transition;
    /// only the live CargoClass contents and its fire cursor are cleared.
    pub(crate) fn take_for_uninit(&mut self) -> Vec<u64> {
        let passengers = std::mem::take(&mut self.passengers);
        self.passenger_sizes.clear();
        self.total_size = 0;
        self.garrison_fire_index = 0;
        passengers
    }
}

/// Boarding intent phase — tracks a passenger's approach to a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BoardingPhase {
    /// Moving toward the transport cell.
    Approach,
    /// Adjacent to transport, entering this tick.
    Entering,
}

/// Passenger/transport role for an entity. Replaces three separate Option fields
/// with a single enum that makes invalid states unrepresentable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PassengerRole {
    /// Entity has no passenger/transport role. Most entities are this.
    None,
    /// Entity is a transport or garrisonable building that can hold passengers.
    Transport { cargo: PassengerCargo },
    /// Entity is approaching a transport to board it.
    Boarding {
        target_transport_id: u64,
        phase: BoardingPhase,
    },
    /// Entity is inside a transport (hidden from map, not targetable).
    Inside { transport_id: u64 },
}

impl PassengerRole {
    /// Returns the cargo hold if this entity is a transport.
    pub fn cargo(&self) -> Option<&PassengerCargo> {
        match self {
            Self::Transport { cargo } => Some(cargo),
            _ => Option::None,
        }
    }

    /// Returns a mutable reference to the cargo hold if this entity is a transport.
    pub fn cargo_mut(&mut self) -> Option<&mut PassengerCargo> {
        match self {
            Self::Transport { cargo } => Some(cargo),
            _ => Option::None,
        }
    }

    /// Returns the transport ID if this entity is inside one.
    pub fn inside_transport_id(&self) -> Option<u64> {
        match self {
            Self::Inside { transport_id } => Some(*transport_id),
            _ => Option::None,
        }
    }

    /// True if entity is inside a transport (hidden from map).
    pub fn is_inside_transport(&self) -> bool {
        matches!(self, Self::Inside { .. })
    }

    /// True if entity is a transport/garrison with a cargo hold.
    pub fn is_transport(&self) -> bool {
        matches!(self, Self::Transport { .. })
    }
}

/// Check whether a passenger entity can enter a specific transport.
///
/// Validates: alive, not already transported, owner compatibility,
/// size fits, transport has room.  For garrison buildings
/// (`CanBeOccupied=yes`) additional checks apply: `Occupier=yes`
/// required on the infantry, and the building must not be at red health.
pub fn can_enter_transport(
    passenger: &GameEntity,
    transport: &GameEntity,
    passenger_obj: &ObjectType,
    transport_obj: &ObjectType,
    cargo: &PassengerCargo,
    rules: &RuleSet,
    houses: &BTreeMap<InternedId, HouseState>,
    path_grid: Option<&PathGrid>,
) -> bool {
    // Must be alive, not dying, not already inside something
    if !passenger.is_alive() || passenger.dying {
        return false;
    }
    if passenger.passenger_role.is_inside_transport() {
        return false;
    }
    // Transport must be alive
    if !transport.is_alive() || transport.dying {
        return false;
    }
    if transport_obj.can_be_occupied {
        return can_dock_occupier_garrison(
            passenger,
            transport,
            passenger_obj,
            transport_obj,
            cargo,
            rules,
            houses,
            path_grid,
        );
    } else {
        // Vehicle transports: strict same-owner.
        if passenger.owner() != transport.owner() {
            return false;
        }
        // CanEnter (0x0F) is refused while the transport is warped: a Unit
        // tests `+0x270` (`0x007375BA..0x007375CD`), an absorbing building
        // its online latch before the absorber block (`0x0043C422`, answer
        // 10).
        use crate::map::entities::EntityCategory;
        match transport.category {
            EntityCategory::Unit if transport.is_warped_out() => return false,
            EntityCategory::Structure if !transport.building_online() => return false,
            _ => {}
        }
        // UnitClass::Receive_Radio 0x0F 0x007375F3..0x0073761D: a controlled
        // Foot, one a parasite is eating, or one controlling a captive may not
        // load. An absorbing building's Receive_Radio (`0x0043C4A0`) refuses
        // only the controller; a captive boarding it is released first.
        // AircraftClass's radio tests neither (no stock aircraft carries
        // passengers).
        if transport.category == EntityCategory::Unit && passenger.mind_control.is_mind_controlled()
        {
            return false;
        }
        if passenger.parasite_eating_me.is_some() {
            return false;
        }
        if transport.category != EntityCategory::Aircraft
            && passenger
                .capture_manager
                .as_ref()
                .is_some_and(|manager| manager.has_any())
        {
            return false;
        }
    }
    // Size check
    cargo.can_accept(passenger_obj.size)
}

#[allow(clippy::too_many_arguments)]
pub fn can_dock_occupier_garrison(
    passenger: &GameEntity,
    building: &GameEntity,
    passenger_obj: &ObjectType,
    building_obj: &ObjectType,
    cargo: &PassengerCargo,
    rules: &RuleSet,
    houses: &BTreeMap<InternedId, HouseState>,
    path_grid: Option<&PathGrid>,
) -> bool {
    if !building_obj.can_be_occupied {
        return false;
    }
    if building.building_up.is_some() || building.building_down.is_some() {
        return false;
    }
    if let Some(grid) = path_grid {
        if building.position.rx >= grid.width() || building.position.ry >= grid.height() {
            return false;
        }
    }
    // `BuildingClass::CanDock @ 0x00457D3E`: a building being warped out
    // admits nobody.
    if building.is_warped_out() {
        return false;
    }
    if !passenger_obj.occupier {
        return false;
    }
    let same_owner = passenger.owner() == building.owner();
    if !same_owner && !owner_is_multiplay_passive(building.owner(), houses) {
        return false;
    }
    if cargo.count() == building_obj.max_number_occupants {
        return false;
    }
    if is_at_or_below_red_hp(
        building.health.current,
        building_obj.strength,
        rules.general.condition_red,
    ) {
        return false;
    }
    // BuildingClass::CanDock `0x00457D98` and Receive_Radio `0x0043C5CB..
    // 0x0043C5EF` refuse an infantry that controls a captive or is itself
    // controlled.
    if passenger
        .capture_manager
        .as_ref()
        .is_some_and(|manager| manager.has_any())
        || passenger.mind_control.is_mind_controlled()
    {
        return false;
    }
    true
}

/// Read the target owner's `MultiplayPassive` house-type fact.
///
/// One source of truth: the flag is resolved from the country rules once, at
/// house creation, and stored on the house — see
/// `house_state::resolve_multiplay_passive`. An owner with no `HouseState` is
/// not passive.
fn owner_is_multiplay_passive(
    owner: InternedId,
    houses: &BTreeMap<InternedId, HouseState>,
) -> bool {
    houses
        .get(&owner)
        .is_some_and(|house| house.multiplay_passive)
}

pub fn can_entity_enter_garrison(
    sim: &Simulation,
    rules: &RuleSet,
    passenger_id: u64,
    building_id: u64,
    path_grid: Option<&PathGrid>,
) -> bool {
    let Some(passenger) = sim.substrate.entities.get(passenger_id) else {
        return false;
    };
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return false;
    };
    let Some(passenger_obj) = sim.object_type(passenger.type_ref(), rules) else {
        return false;
    };
    let Some(building_obj) = sim.object_type(building.type_ref(), rules) else {
        return false;
    };
    let Some(cargo) = building.passenger_role.cargo() else {
        return false;
    };
    can_enter_transport(
        passenger,
        building,
        passenger_obj,
        building_obj,
        cargo,
        rules,
        &sim.houses,
        path_grid,
    )
}

/// Maximum cell distance for a passenger to be considered "at" the transport.
/// Chebyshev distance in cells — 1 means same cell or adjacent.
const BOARD_DISTANCE: u32 = 1;

/// 8-directional neighbor offsets for finding unload exit cells.
const NEIGHBORS: [(i16, i16); 8] = [
    (0, -1),  // N
    (1, -1),  // NE
    (1, 0),   // E
    (1, 1),   // SE
    (0, 1),   // S
    (-1, 1),  // SW
    (-1, 0),  // W
    (-1, -1), // NW
];

/// Advance the passenger boarding/unloading system each tick.
///
/// Phase A: For entities with `boarding_state`, check if they arrived at
/// the transport's cell. If so, execute boarding. If the transport is
/// destroyed or full, cancel boarding.
///
/// Phase B: For `CanBeOccupied=` buildings with `OrderIntent::Unloading`,
/// eject one occupant per tick to an adjacent unoccupied cell. Clear the
/// order when all occupants are out.
///
/// Vehicle and aircraft transports do NOT unload here: their Unload is a
/// mission handler (`crate::sim::transport_unload`, the `Passengers > 0`
/// branch of `UnitClass::Mission_Unload @ 0x0073D630`) dispatched from the
/// per-object AI host on the `[Unload] Rate` cadence.
///
/// Returns `true` if any entity's ownership changed this tick (garrison
/// transfer or revert), signalling that the sprite atlas needs a rebuild.
pub fn tick_passenger_system(sim: &mut Simulation, rules: &RuleSet) -> bool {
    let order = sim.live_object_order_snapshot();
    tick_boarding_and_garrison_reconciliation_in_order(sim, rules, &order)
}

/// Local surrogate for gamemd's live object-vector walk for the garrison owner slice.
///
/// Production supplies `Simulation::live_object_order_snapshot`, while focused
/// tests pass explicit relative passenger/building order. The owned parity
/// contract here is that a building reconciles only when its own turn is reached
/// after a cargo mutation.
fn tick_boarding_and_garrison_reconciliation_in_order(
    sim: &mut Simulation,
    rules: &RuleSet,
    order: &[u64],
) -> bool {
    let mut ownership_changed = false;
    for &entity_id in order {
        if sim.substrate.entities.get(entity_id).is_none() {
            continue;
        }

        // Boarding is the passenger's own mission and unloading the
        // building's; a warped object runs neither (`GameEntity::ai_frozen`).
        let frozen = sim
            .substrate
            .entities
            .get(entity_id)
            .is_some_and(GameEntity::ai_frozen);
        if !frozen
            && sim
                .substrate
                .entities
                .get(entity_id)
                .is_some_and(|e| matches!(e.passenger_role, PassengerRole::Boarding { .. }))
        {
            process_boarding_passenger(sim, rules, entity_id);
        }

        if !frozen && is_can_be_occupied_unloading_transport(sim, rules, entity_id) {
            process_unloading_transport(sim, rules, entity_id);
        }

        ownership_changed |= reconcile_civilian_garrison_owner_for_building(sim, rules, entity_id);
    }
    ownership_changed
}

fn process_boarding_passenger(sim: &mut Simulation, rules: &RuleSet, pax_id: u64) {
    let transport_id = match sim
        .substrate
        .entities
        .get(pax_id)
        .map(|e| &e.passenger_role)
    {
        Some(PassengerRole::Boarding {
            target_transport_id,
            ..
        }) => *target_transport_id,
        _ => return,
    };

    let transport_alive = sim
        .substrate
        .entities
        .get(transport_id)
        .is_some_and(|t| t.is_alive() && !t.dying);
    if !transport_alive {
        if let Some(e) = sim.substrate.entities.get_mut(pax_id) {
            e.passenger_role = PassengerRole::None;
        }
        return;
    }

    let (pax_rx, pax_ry) = match sim.substrate.entities.get(pax_id) {
        Some(e) => (e.position.rx, e.position.ry),
        None => return,
    };
    let (trx, try_) = match sim.substrate.entities.get(transport_id) {
        Some(e) => (e.position.rx, e.position.ry),
        None => return,
    };

    let dx = (pax_rx as i32 - trx as i32).unsigned_abs();
    let dy = (pax_ry as i32 - try_ as i32).unsigned_abs();
    if dx.max(dy) > BOARD_DISTANCE {
        return;
    }

    let pax_type_str = sim
        .substrate
        .entities
        .get(pax_id)
        .map(|e| sim.interner.resolve(e.type_ref()).to_string())
        .unwrap_or_default();
    let transport_type_str = sim
        .substrate
        .entities
        .get(transport_id)
        .map(|e| sim.interner.resolve(e.type_ref()).to_string())
        .unwrap_or_default();

    let pax_size = rules.object(&pax_type_str).map(|obj| obj.size).unwrap_or(1);
    let transport_obj = rules.object(&transport_type_str);
    let transport_gunner = transport_obj.map(|obj| obj.gunner).unwrap_or(false);
    let transport_open_topped = transport_obj.map(|obj| obj.open_topped).unwrap_or(false);

    let pax_obj = rules.object(&pax_type_str);
    let pax_ifv_mode = pax_obj.map(|obj| obj.ifv_mode).unwrap_or(0);
    let pax_open_transport_weapon = pax_obj.map(|obj| obj.open_transport_weapon).unwrap_or(-1);
    let entering_owner = sim.substrate.entities.get(pax_id).map(|pax| pax.owner());

    let can_board = sim
        .substrate
        .entities
        .get(transport_id)
        .and_then(|t| t.passenger_role.cargo())
        .is_some_and(|cargo| cargo.can_accept(pax_size));

    if can_board {
        // PerCellProcess zeroes the mission tick count and the gattling spin
        // first (`+0xC4 = 0`, `SetValue(0)`, `SetStage(0)`: Unit
        // `0x0073A6FC..0x0073A70F` and `0x0073A29E..0x0073A2B1`, Infantry
        // `0x0051A40E..0x0051A41C` and `0x0051A2B0..0x0051A2BE`).
        if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
            pax.mission.clear_ai_counter();
            pax.gattling.set_value(0);
            pax.gattling.set_stage(0);
        }
        // PerCellProcess releases a captive before it enters (`0x0051A2DA`,
        // `0x0051A438`, `0x0073A2CD`, `0x0073A72B`); the transport radio gates
        // leave only absorbing buildings to reach this.
        if let Some(controller) = sim
            .substrate
            .entities
            .get(pax_id)
            .and_then(|pax| pax.mind_control.controller())
        {
            sim.free_unit(controller, pax_id, rules);
        }
        // CargoClass::AddPassenger conceals the passenger before splicing it
        // into the cargo chain. Techno Limbo owns BREAK, Mark removal, and
        // LogicVector removal in that order.
        if sim.techno_limbo_with_rules(pax_id, rules) != ConcealOutcome::Concealed {
            return;
        }
        let boarded = sim
            .substrate
            .entities
            .get_mut(transport_id)
            .and_then(|t| t.passenger_role.cargo_mut())
            .is_some_and(|cargo| cargo.board(pax_id, pax_size));
        debug_assert!(boarded, "boarding admission changed during one transaction");
        if !boarded {
            // No mutation can race the single-threaded transaction, but recover
            // the passenger rather than stranding a concealed object if an
            // invariant is broken.
            let _ = sim.reveal(pax_id);
            return;
        }

        let first_occupant = sim
            .substrate
            .entities
            .get(transport_id)
            .and_then(|t| t.passenger_role.cargo())
            .map_or(false, |c| c.count() == 1);
        if first_occupant
            && rules
                .object(&transport_type_str)
                .map_or(false, |o| o.can_be_occupied)
        {
            if let (Some(owner), Some(t)) =
                (entering_owner, sim.substrate.entities.get(transport_id))
            {
                let rx = t.position.rx;
                let ry = t.position.ry;
                sim.sound_events
                    .push(SimSoundEvent::StructureGarrisoned { owner });
                sim.sound_events
                    .push(SimSoundEvent::BuildingGarrisonedSfx { owner, rx, ry });
            }
        }

        if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
            pax.passenger_role = PassengerRole::Inside { transport_id };
            pax.movement_target = None;
            pax.attack_target = None;
            pax.passively_acquired_target = false;
            pax.order_intent = None;
        }
        if transport_open_topped {
            let registered = sim.register_open_topped_passenger(pax_id);
            debug_assert!(
                registered,
                "accepted open-topped passenger must remain active"
            );
        }

        let new_override = if transport_gunner {
            Some(crate::sim::combat::combat_weapon::WeaponOverride::IfvSlot(
                pax_ifv_mode,
            ))
        } else if transport_open_topped && pax_open_transport_weapon >= 0 {
            Some(
                crate::sim::combat::combat_weapon::WeaponOverride::OpenTransport(
                    pax_open_transport_weapon as u32,
                ),
            )
        } else {
            None
        };
        if new_override.is_some() {
            if let Some(t) = sim.substrate.entities.get_mut(transport_id) {
                t.weapon_override = new_override;
            }
        }
        if transport_gunner {
            // UnitClass +0x4D4 (`0x00746420`): the gunner's TemporalClass
            // moves to the IFV.
            sim.temporal_receive_gunner(transport_id, pax_id);
        }
    } else if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
        pax.passenger_role = PassengerRole::None;
    }
}

/// `TechnoClass+0x82` InOpenTransport with its `+0x11C` Transporter: the
/// `OpenTopped=` transport `entity` rides in, if any. `PerCellProcess`
/// (`0x0051A463`/`0x0073A768`) writes `+0x11C` for every transport and, for an
/// open-topped one, `SetInOpenTransport @ 0x00710470` sets `+0x82`. Read by
/// FireAt's damage build, kill credit, GetFireError, navigation and the
/// Temporal warp-distance check.
pub(crate) fn open_topped_transport(
    entities: &crate::sim::entity_store::EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    entity: &GameEntity,
) -> Option<u64> {
    let transport_id = entity.passenger_role.inside_transport_id()?;
    let transport = entities.get(transport_id)?;
    rules
        .object(interner.resolve(transport.type_ref()))?
        .open_topped
        .then_some(transport_id)
}

fn is_civilian_garrison_owner(interner: &StringInterner, owner: InternedId) -> bool {
    let owner = interner.resolve(owner);
    owner.eq_ignore_ascii_case("neutral") || owner.eq_ignore_ascii_case("special")
}

fn resolved_civilian_garrison_owner(sim: &mut Simulation) -> InternedId {
    let neutral = sim.interner.intern("Neutral");
    if sim.houses.is_empty() || sim.houses.contains_key(&neutral) {
        return neutral;
    }

    let special = sim.interner.intern("Special");
    if sim.houses.contains_key(&special) {
        return special;
    }

    neutral
}

// Object5F5CD0, reached from CanDock457D8F and ejection45821A, tests
// ratio with AH41 then separately requires signed actual HP>0.
fn is_at_or_below_red_hp(current: i32, strength: i32, condition_red: f64) -> bool {
    use crate::util::native_x87::MaskedX87Ordering::{Equal, Less, Unordered};
    current > 0
        && matches!(
            crate::sim::components::Health { current }.compare_ratio(strength, condition_red),
            Less | Equal | Unordered
        )
}

fn reconcile_civilian_garrison_owner_for_building(
    sim: &mut Simulation,
    rules: &RuleSet,
    building_id: u64,
) -> bool {
    let Some((type_ref, mut current_owner, mut first_passenger, mut cargo_empty, red_hp_occupied)) =
        sim.substrate
            .entities
            .get(building_id)
            .and_then(|building| {
                let cargo = building.passenger_role.cargo()?;
                Some((
                    building.type_ref(),
                    building.owner(),
                    // The FIRST occupant to enter: the cargo list is head-first
                    // (`AddPassenger` prepends), so the earliest entry is the
                    // tail. VERA-internal ownership rule, gamemd equivalent
                    // UNCHECKED; preserved unchanged across the LIFO change.
                    cargo.passengers.last().copied(),
                    cargo.is_empty(),
                    !cargo.is_empty()
                        && is_at_or_below_red_hp(
                            building.health.current,
                            sim.object_type(building.type_ref(), rules)?.strength,
                            rules.general.condition_red,
                        ),
                ))
            })
    else {
        return false;
    };

    let type_name = sim.interner.resolve(type_ref);
    if !rules
        .object(type_name)
        .is_some_and(|obj| obj.can_be_occupied)
    {
        return false;
    }

    if red_hp_occupied {
        crate::sim::production::eject_red_hp_garrison(sim, rules, building_id);
        let Some((owner_after_eject, first_after_eject, empty_after_eject)) = sim
            .substrate
            .entities
            .get(building_id)
            .and_then(|building| {
                let cargo = building.passenger_role.cargo()?;
                Some((
                    building.owner(),
                    cargo.passengers.last().copied(),
                    cargo.is_empty(),
                ))
            })
        else {
            return false;
        };
        current_owner = owner_after_eject;
        first_passenger = first_after_eject;
        cargo_empty = empty_after_eject;
    }

    if !cargo_empty && is_civilian_garrison_owner(&sim.interner, current_owner) {
        let Some(new_owner) = first_passenger
            .and_then(|passenger_id| sim.substrate.entities.get(passenger_id))
            .map(|passenger| passenger.owner())
        else {
            return false;
        };
        if new_owner == current_owner {
            return false;
        }
        sim.change_owner_with_rules(building_id, new_owner, rules);
        return true;
    }

    if cargo_empty && !is_civilian_garrison_owner(&sim.interner, current_owner) {
        let civilian_owner = resolved_civilian_garrison_owner(sim);
        sim.sound_events.push(SimSoundEvent::StructureAbandoned {
            owner: current_owner,
        });
        sim.change_owner_with_rules(building_id, civilian_owner, rules);
        return current_owner != civilian_owner;
    }

    false
}

/// Test driver for the production boarding operation. Snapshot admission IDs
/// without running the separate building ownership reconciliation phase.
#[cfg(test)]
fn tick_boarding(sim: &mut Simulation, rules: &RuleSet) -> bool {
    let boarding_ids: Vec<u64> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .filter(|&id| {
            sim.substrate.entities.get(id).is_some_and(|entity| {
                matches!(entity.passenger_role, PassengerRole::Boarding { .. })
            })
        })
        .collect();
    for id in boarding_ids {
        process_boarding_passenger(sim, rules, id);
    }
    false
}

/// Process transports with `OrderIntent::Unloading` — eject one passenger per tick.
fn is_can_be_occupied_unloading_transport(
    sim: &Simulation,
    rules: &RuleSet,
    transport_id: u64,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(transport_id) else {
        return false;
    };
    if !matches!(entity.order_intent, Some(OrderIntent::Unloading)) {
        return false;
    }
    sim.object_type(entity.type_ref(), rules)
        .is_some_and(|obj| obj.can_be_occupied)
}

/// `FootClass::GetCurrentSpeed @ 0x004DB1A0` for an ejected passenger.
///
/// The passenger entity is already unlimboed when this runs, so its rank and
/// locomotor are readable; a rank the entity does not carry yet (or a missing
/// entity) falls back to the plain type speed.
fn scatter_speed_for_passenger(
    sim: &Simulation,
    rules: &RuleSet,
    pax_id: u64,
    pax_type_str: &str,
) -> crate::util::fixed_math::SimFixed {
    let obj = rules.object(pax_type_str);
    let raw = obj.map_or(4, |o| o.speed);
    match sim.substrate.entities.get(pax_id) {
        Some(pax) => crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
            pax,
            obj,
            raw,
            rules.general.veteran_speed,
        ),
        None => ra2_speed_to_leptons_per_second(raw),
    }
}

fn process_unloading_transport(sim: &mut Simulation, rules: &RuleSet, transport_id: u64) {
    let (trx, try_, tz) = match sim.substrate.entities.get(transport_id) {
        Some(e) => (e.position.rx, e.position.ry, e.position.z),
        None => return,
    };

    let occupied_cells: Vec<(u16, u16)> = {
        let all_keys: Vec<u64> = sim.substrate.entities.keys_sorted();
        all_keys
            .iter()
            .filter_map(|&eid| {
                let e = sim.substrate.entities.get(eid)?;
                if !e.passenger_role.is_inside_transport() && !e.dying && e.is_alive() {
                    Some((e.position.rx, e.position.ry))
                } else {
                    None
                }
            })
            .collect()
    };

    let exit_cell = NEIGHBORS.iter().find_map(|&(dx, dy)| {
        let nx = trx as i16 + dx;
        let ny = try_ as i16 + dy;
        if nx < 0 || ny < 0 {
            return None;
        }
        let (nx, ny) = (nx as u16, ny as u16);
        let occupied = occupied_cells.iter().any(|&(ox, oy)| ox == nx && oy == ny);
        if occupied { None } else { Some((nx, ny)) }
    });

    let Some((exit_rx, exit_ry)) = exit_cell else {
        return;
    };

    let result = depart_cargo_head(
        sim,
        rules,
        transport_id,
        DepartureRoute::Garrison,
        |sim, pax_id| {
            let pax_type_str = sim
                .substrate
                .entities
                .get(pax_id)
                .map(|e| sim.interner.resolve(e.type_ref()).to_string())
                .unwrap_or_default();
            reveal_unloaded_passenger(sim, transport_id, pax_id, exit_rx, exit_ry, tz)?;

            // `FootClass::GetCurrentSpeed @ 0x004DB1A0`: a veteran passenger scatters at
            // its FASTER speed like any other ordered move.
            let scatter_speed = scatter_speed_for_passenger(sim, rules, pax_id, &pax_type_str);
            let start_dir = sim.scatter_rng().next_u32() as usize % 8;
            for i in 0..8 {
                let (dx, dy) = NEIGHBORS[(start_dir + i) % 8];
                let sx = exit_rx as i32 + dx as i32;
                let sy = exit_ry as i32 + dy as i32;
                if sx >= 0 && sy >= 0 {
                    let dest = (sx as u16, sy as u16);
                    let occupied = occupied_cells
                        .iter()
                        .any(|&(ox, oy)| ox == dest.0 && oy == dest.1);
                    if !occupied {
                        movement::issue_direct_move(
                            &mut sim.substrate.entities,
                            pax_id,
                            dest,
                            scatter_speed,
                            movement::DestinationTiming::from_rules(
                                sim.session.binary_frame,
                                rules.into(),
                            ),
                        );
                        break;
                    }
                }
            }

            let cargo_empty = sim
                .substrate
                .entities
                .get(transport_id)
                .and_then(|t| t.passenger_role.cargo())
                .is_some_and(|c| c.is_empty());
            if cargo_empty {
                if let Some(t) = sim.substrate.entities.get_mut(transport_id) {
                    t.weapon_override = None;
                    t.order_intent = None;
                }
            }
            Ok(())
        },
    );
    if result == Err(DepartureFailure::NoCargo) {
        if let Some(transport) = sim.substrate.entities.get_mut(transport_id) {
            transport.order_intent = None;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_health_ratio_corpus_matches_garrison_red_predicate() {
        for row in crate::sim::health_ratio_fixture::rows() {
            assert_eq!(
                super::is_at_or_below_red_hp(
                    row.input.current,
                    row.input.strength,
                    row.input.red()
                ),
                row.output.garrison_red,
                "{row:?}"
            );
        }
    }
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::world::RevealOutcome;
    use crate::util::lepton;

    fn garrison_test_rules() -> RuleSet {
        let ini_str = "\
[InfantryTypes]
0=E1
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]
0=CAGAS01
[Countries]
0=Americans
1=Russians
2=Neutral
3=Special

[E1]
Name=Conscript
Cost=100
Strength=125
Armor=none
Speed=4
Occupier=yes

[CAGAS01]
Name=GasStation
Cost=0
Strength=400
Armor=wood
Foundation=2x2
CanBeOccupied=yes
CanOccupyFire=yes
MaxNumberOccupants=5

[Neutral]
MultiplayPassive=true

[Special]
MultiplayPassive=true

[General]
FixtureOnly=1
[AudioVisual]
BuildingGarrisonedSound=BuildingGarrisoned
ConditionRed=25%
ConditionYellow=50%
";
        let ini = IniFile::from_str(ini_str);
        RuleSet::from_ini(&ini).expect("parse garrison test rules")
    }

    fn open_topped_test_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "\
[InfantryTypes]
0=E1
[VehicleTypes]
0=BFRT
[AircraftTypes]
[BuildingTypes]

[E1]
Strength=125
Armor=none
Speed=4
Size=1
OpenTransportWeapon=0

[BFRT]
Strength=600
Armor=heavy
Speed=4
Passengers=5
SizeLimit=2
OpenTopped=yes

[General]
FixtureOnly=1
[AudioVisual]
ConditionRed=25%
ConditionYellow=50%
",
        );
        RuleSet::from_ini(&ini).expect("parse open-topped test rules")
    }

    /// Spawn a CanBeOccupied building entity at (rx, ry) owned by `owner_str`.
    fn spawn_garrison_building(
        sim: &mut Simulation,
        rules: &RuleSet,
        type_ref: &str,
        owner_str: &str,
        rx: u16,
        ry: u16,
    ) -> u64 {
        let stable_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern(owner_str);
        let type_id = sim.interner.intern(type_ref);
        let mut ge = GameEntity::test_default(stable_id, type_ref, owner_str, rx, ry);
        ge.owner = owner_id;
        ge.type_ref = type_id;
        let obj = rules.object(type_ref).expect("type exists");
        ge.health.current = obj.strength;
        ge.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(obj.strength);
        ge.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(obj.max_number_occupants, 1),
        };
        sim.substrate.entities.insert(ge);
        assert!(matches!(
            sim.reveal(stable_id),
            RevealOutcome::Revealed { .. }
        ));
        stable_id
    }

    fn spawn_transport(
        sim: &mut Simulation,
        rules: &RuleSet,
        type_ref: &str,
        owner_str: &str,
        rx: u16,
        ry: u16,
    ) -> u64 {
        let stable_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern(owner_str);
        let type_id = sim.interner.intern(type_ref);
        let obj = rules.object(type_ref).expect("transport type exists");
        let mut entity = GameEntity::test_default(stable_id, type_ref, owner_str, rx, ry);
        entity.owner = owner_id;
        entity.type_ref = type_id;
        entity.category = EntityCategory::Unit;
        entity.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(obj.passengers.max(0) as u32, obj.size_limit),
        };
        sim.substrate.entities.insert(entity);
        assert!(matches!(
            sim.reveal(stable_id),
            RevealOutcome::Revealed { .. }
        ));
        stable_id
    }

    /// Spawn an Occupier infantry entity at (rx, ry) in `Boarding::Entering` state
    /// targeting `transport_id`.
    fn spawn_boarding_occupier(
        sim: &mut Simulation,
        type_ref: &str,
        owner_str: &str,
        transport_id: u64,
        rx: u16,
        ry: u16,
    ) -> u64 {
        let stable_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern(owner_str);
        let type_id = sim.interner.intern(type_ref);
        let mut ge = GameEntity::test_default(stable_id, type_ref, owner_str, rx, ry);
        ge.owner = owner_id;
        ge.type_ref = type_id;
        ge.passenger_role = PassengerRole::Boarding {
            target_transport_id: transport_id,
            phase: BoardingPhase::Entering,
        };
        sim.substrate.entities.insert(ge);
        assert!(matches!(
            sim.reveal(stable_id),
            RevealOutcome::Revealed { .. }
        ));
        stable_id
    }

    fn can_enter_garrison_fixture(
        sim: &Simulation,
        rules: &RuleSet,
        passenger_id: u64,
        building_id: u64,
    ) -> bool {
        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("passenger exists");
        let transport = sim
            .substrate
            .entities
            .get(building_id)
            .expect("building exists");
        let passenger_obj = rules
            .object(sim.interner.resolve(passenger.type_ref))
            .expect("passenger type exists");
        let transport_obj = rules
            .object(sim.interner.resolve(transport.type_ref))
            .expect("transport type exists");
        let cargo = transport.passenger_role.cargo().expect("cargo exists");
        can_enter_transport(
            passenger,
            transport,
            passenger_obj,
            transport_obj,
            cargo,
            rules,
            &sim.houses,
            None,
        )
    }

    fn owner_name(sim: &Simulation, entity_id: u64) -> String {
        sim.substrate
            .entities
            .get(entity_id)
            .map(|entity| sim.interner.resolve(entity.owner).to_string())
            .expect("entity exists")
    }

    #[test]
    fn test_cargo_new() {
        let cargo = PassengerCargo::new(5, 2);
        assert_eq!(cargo.capacity, 5);
        assert_eq!(cargo.size_limit, 2);
        assert_eq!(cargo.count(), 0);
        assert!(cargo.is_empty());
        assert!(cargo.passenger_sizes.is_empty());
        assert_eq!(cargo.total_size, 0);
    }

    #[test]
    fn test_board_and_count() {
        let mut cargo = PassengerCargo::new(3, 0);
        assert!(cargo.board(100, 1));
        assert!(cargo.board(101, 1));
        assert!(cargo.board(102, 1));
        assert_eq!(cargo.count(), 3);
        assert!(!cargo.is_empty());
        assert_eq!(cargo.total_size, 3);
        // Full — cannot board more
        assert!(!cargo.board(103, 1));
        assert_eq!(cargo.count(), 3);
    }

    #[test]
    fn test_size_limit_rejection() {
        let mut cargo = PassengerCargo::new(5, 2);
        // Size 1 fits
        assert!(cargo.can_accept(1));
        assert!(cargo.board(100, 1));
        // Size 2 fits
        assert!(cargo.can_accept(2));
        assert!(cargo.board(101, 2));
        // Size 3 rejected by SizeLimit=2
        assert!(!cargo.can_accept(3));
        assert!(!cargo.board(102, 3));
        assert_eq!(cargo.count(), 2);
        assert_eq!(cargo.total_size, 3);
    }

    #[test]
    fn test_size_limit_zero_means_no_restriction() {
        let mut cargo = PassengerCargo::new(5, 0);
        assert!(cargo.can_accept(100)); // Any size fits
        assert!(cargo.board(1, 50));
        assert_eq!(cargo.total_size, 50);
    }

    #[test]
    fn test_disembark() {
        let mut cargo = PassengerCargo::new(5, 0);
        cargo.board(100, 1);
        cargo.board(101, 2);
        cargo.board(102, 1);

        assert!(cargo.disembark(101));
        assert_eq!(cargo.count(), 2);
        assert_eq!(cargo.total_size, 2);
        // Head-first list order: the last boarded sits at index 0.
        assert_eq!(cargo.passengers, vec![102, 100]);
        assert_eq!(cargo.passenger_sizes, vec![1, 1]);

        // Disembarking non-existent ID returns false
        assert!(!cargo.disembark(999));
    }

    /// `CargoClass::AddPassenger @ 0x004733A0` prepends and
    /// `RemoveFirstPassenger @ 0x00473430` pops the head: the last boarded
    /// passenger leaves first.
    #[test]
    fn test_unload_first_is_lifo_head_pop() {
        let mut cargo = PassengerCargo::new(5, 0);
        cargo.board(100, 1);
        cargo.board(101, 2);
        cargo.board(102, 3);

        assert_eq!(cargo.passengers, vec![102, 101, 100]);
        assert_eq!(cargo.unload_first(), Some((102, 3)));
        assert_eq!(cargo.total_size, 3);
        assert_eq!(cargo.unload_first(), Some((101, 2)));
        assert_eq!(cargo.total_size, 1);
        assert_eq!(cargo.unload_first(), Some((100, 1)));
        assert_eq!(cargo.total_size, 0);
        assert_eq!(cargo.unload_first(), None);
        assert!(cargo.is_empty());
        assert!(cargo.passenger_sizes.is_empty());
    }

    #[test]
    fn test_can_accept_when_full() {
        let mut cargo = PassengerCargo::new(1, 1);
        assert!(cargo.can_accept(1));
        cargo.board(100, 1);
        assert!(!cargo.can_accept(1));
    }

    #[test]
    fn garrison_owner_not_changed_during_boarding_call() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        let changed = tick_boarding(&mut sim, &rules);

        assert!(!changed, "boarding must not report ownership transfer");
        assert_eq!(owner_name(&sim, bldg), "Neutral");
        assert!(matches!(
            sim.substrate.entities.get(pax).unwrap().passenger_role,
            PassengerRole::Inside { transport_id } if transport_id == bldg
        ));
        assert_eq!(
            sim.substrate
                .entities
                .get(bldg)
                .unwrap()
                .garrison_original_owner,
            None,
            "civilian garrison boarding must not save a per-building original owner"
        );
    }

    #[test]
    fn garrison_owner_transfers_same_frame_when_building_update_after_entry() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        let changed =
            tick_boarding_and_garrison_reconciliation_in_order(&mut sim, &rules, &[pax, bldg]);

        assert!(changed);
        assert_eq!(owner_name(&sim, bldg), "Americans");
    }

    #[test]
    fn production_garrison_owner_order_uses_live_object_order_not_stable_id() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
        assert!(
            bldg < pax,
            "fixture keeps stable-id order opposite native order"
        );
        sim.set_logic_order_for_test(vec![pax, bldg]);

        let changed = tick_passenger_system(&mut sim, &rules);

        assert!(
            changed,
            "production passenger tick should use live object order for same-frame transfer"
        );
        assert_eq!(owner_name(&sim, bldg), "Americans");
    }

    #[test]
    fn garrison_owner_waits_next_frame_when_building_update_before_entry() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        let changed =
            tick_boarding_and_garrison_reconciliation_in_order(&mut sim, &rules, &[bldg, pax]);

        assert!(!changed);
        assert_eq!(owner_name(&sim, bldg), "Neutral");

        let changed =
            tick_boarding_and_garrison_reconciliation_in_order(&mut sim, &rules, &[bldg, pax]);

        assert!(changed);
        assert_eq!(owner_name(&sim, bldg), "Americans");
    }

    #[test]
    fn garrison_reconciliation_uses_first_occupant_owner() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let first = spawn_boarding_occupier(&mut sim, "E1", "Russians", bldg, 10, 11);
        let second = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 11, 10);

        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            let cargo = building.passenger_role.cargo_mut().expect("cargo exists");
            assert!(cargo.board(first, 1));
            assert!(cargo.board(second, 1));
        }

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, bldg);

        assert!(changed);
        assert_eq!(owner_name(&sim, bldg), "Russians");
    }

    #[test]
    fn empty_captured_garrison_reverts_to_civilian_house_not_original_owner() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let special_id = sim.interner.intern("Special");
        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            building.garrison_original_owner = Some(special_id);
        }

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, bldg);

        assert!(changed);
        assert_eq!(owner_name(&sim, bldg), "Neutral");
        assert!(sim.sound_events.iter().any(|event| {
            matches!(
                event,
                SimSoundEvent::StructureAbandoned { owner }
                    if sim.interner.resolve(*owner) == "Americans"
            )
        }));
    }

    #[test]
    fn gsi_05_16_garrison_capture_and_reversion_move_building_counts() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
        let neutral = sim.interner.intern("Neutral");
        let americans = sim.interner.intern("Americans");

        sim.substrate
            .entities
            .get_mut(bldg)
            .expect("garrison exists")
            .category = EntityCategory::Structure;
        let mut neutral_house = HouseState::new(neutral, 0, None, false, 0, 10);
        neutral_house.multiplay_passive = true;
        neutral_house.tracking.set_buildings_for_test(1);
        sim.houses.insert(neutral, neutral_house);
        sim.houses
            .insert(americans, HouseState::new(americans, 1, None, true, 0, 10));
        assert!(
            sim.substrate
                .entities
                .get_mut(bldg)
                .and_then(|building| building.passenger_role.cargo_mut())
                .expect("garrison cargo")
                .board(pax, 1)
        );

        assert!(reconcile_civilian_garrison_owner_for_building(
            &mut sim, &rules, bldg
        ));
        assert_eq!(sim.houses[&neutral].tracking.buildings_for_test(), 0);
        assert_eq!(sim.houses[&americans].tracking.buildings_for_test(), 1);

        assert!(
            sim.substrate
                .entities
                .get_mut(bldg)
                .and_then(|building| building.passenger_role.cargo_mut())
                .expect("garrison cargo")
                .disembark(pax)
        );
        assert!(reconcile_civilian_garrison_owner_for_building(
            &mut sim, &rules, bldg
        ));
        assert_eq!(sim.houses[&neutral].tracking.buildings_for_test(), 1);
        assert_eq!(sim.houses[&americans].tracking.buildings_for_test(), 0);
    }

    #[test]
    fn red_hp_captured_garrison_ejects_and_reverts_in_same_reconciliation() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = place_inside_garrison(&mut sim, &rules, bldg, "E1", "Americans");

        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            building.health.current = 100;
            if let Some(cargo) = building.passenger_role.cargo_mut() {
                cargo.garrison_fire_index = 3;
            }
        }

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, bldg);

        assert!(
            changed,
            "red-HP ejection should let the same reconciliation call revert owner"
        );
        assert_eq!(owner_name(&sim, bldg), "Neutral");

        let building = sim
            .substrate
            .entities
            .get(bldg)
            .expect("building should remain alive");
        let cargo = building
            .passenger_role
            .cargo()
            .expect("cargo should remain");
        assert!(
            cargo.is_empty(),
            "red-HP SellBuilding path clears occupants"
        );
        assert_eq!(cargo.garrison_fire_index, 0);

        let passenger = sim
            .substrate
            .entities
            .get(pax)
            .expect("occupant should still exist");
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.dying);
        assert_eq!((passenger.position.rx, passenger.position.ry), (12, 12));

        assert!(sim.sound_events.iter().any(|event| {
            matches!(
                event,
                SimSoundEvent::StructureAbandoned { owner }
                    if sim.interner.resolve(*owner) == "Americans"
            )
        }));
    }

    // --- Contract #2: lifecycle → active-order membership (conceal/reveal) ---

    /// Boarding conceals the passenger: it leaves the active object order and
    /// stops receiving per-tick AI, while the transport/building stays.
    #[test]
    fn boarding_conceals_passenger_from_active_order() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
        assert!(sim.live_object_order_snapshot().contains(&pax));

        tick_boarding_and_garrison_reconciliation_in_order(&mut sim, &rules, &[pax, bldg]);

        assert!(
            matches!(
                sim.substrate.entities.get(pax).unwrap().passenger_role,
                PassengerRole::Inside { .. }
            ),
            "passenger should have boarded"
        );
        assert!(
            !sim.live_object_order_snapshot().contains(&pax),
            "boarded passenger leaves the active order"
        );
        let passenger = sim.substrate.entities.get(pax).unwrap();
        assert!(passenger.lifecycle.object_alive);
        assert!(passenger.lifecycle.in_limbo);
        assert!(!passenger.lifecycle.cell_marked);
        assert!(!passenger.in_logic_vector);
        assert!(
            sim.live_object_order_snapshot().contains(&bldg),
            "the garrisoned building stays in the active order"
        );
    }

    #[test]
    fn open_topped_passenger_reappends_live_after_hidden_boarding() {
        let mut sim = Simulation::new();
        let rules = open_topped_test_rules();
        let transport = spawn_transport(&mut sim, &rules, "BFRT", "Americans", 10, 10);
        let passenger = spawn_boarding_occupier(&mut sim, "E1", "Americans", transport, 10, 11);

        let tail = sim.allocate_stable_id();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("E1");
        let mut tail_entity = GameEntity::test_default(tail, "E1", "Americans", 20, 20);
        tail_entity.owner = owner;
        tail_entity.type_ref = type_ref;
        tail_entity.category = EntityCategory::Infantry;
        sim.substrate.entities.insert(tail_entity);
        assert!(matches!(sim.reveal(tail), RevealOutcome::Revealed { .. }));
        assert_eq!(
            sim.live_object_order_snapshot(),
            vec![transport, passenger, tail]
        );

        tick_passenger_system(&mut sim, &rules);

        let passenger_entity = sim
            .substrate
            .entities
            .get(passenger)
            .expect("passenger survives boarding");
        assert!(matches!(
            passenger_entity.passenger_role,
            PassengerRole::Inside { transport_id } if transport_id == transport
        ));
        assert!(passenger_entity.lifecycle.in_limbo);
        assert!(!passenger_entity.lifecycle.cell_marked);
        assert!(passenger_entity.in_logic_vector);
        assert_eq!(
            sim.live_object_order_snapshot(),
            vec![transport, tail, passenger]
        );

        tick_passenger_system(&mut sim, &rules);
        assert_eq!(
            sim.live_object_order_snapshot(),
            vec![transport, tail, passenger],
            "an already-active contained passenger must not append twice"
        );
        #[cfg(debug_assertions)]
        sim.debug_assert_logic_membership_consistent();
    }

    /// Ejecting a garrison occupant reveals it: it re-enters the active order.
    #[test]
    fn garrison_eject_reveals_passenger_into_active_order() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = place_inside_garrison(&mut sim, &rules, bldg, "E1", "Americans");
        assert!(
            !sim.live_object_order_snapshot().contains(&pax),
            "an inside occupant is not in the active order"
        );

        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            building.health.current = 100;
            if let Some(cargo) = building.passenger_role.cargo_mut() {
                cargo.garrison_fire_index = 3;
            }
        }

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, bldg);
        assert!(changed);
        assert!(matches!(
            sim.substrate.entities.get(pax).unwrap().passenger_role,
            PassengerRole::None
        ));
        assert!(
            sim.live_object_order_snapshot().contains(&pax),
            "ejected occupant is re-appended to the active order"
        );
    }

    /// Board-then-eject leaves exactly one membership entry, re-appended at the
    /// tail (idempotent, order-preserving — not sorted).
    #[test]
    fn board_then_eject_round_trip_reappends_once_at_tail() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        tick_boarding_and_garrison_reconciliation_in_order(&mut sim, &rules, &[pax, bldg]);
        assert!(!sim.live_object_order_snapshot().contains(&pax));

        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            building.health.current = 100;
            if let Some(cargo) = building.passenger_role.cargo_mut() {
                cargo.garrison_fire_index = 3;
            }
        }
        reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, bldg);

        let order = sim.live_object_order_snapshot();
        assert_eq!(
            order.iter().filter(|&&x| x == pax).count(),
            1,
            "exactly one membership entry after a board/eject round trip"
        );
        assert_eq!(
            *order.last().expect("order non-empty"),
            pax,
            "re-appended at the tail, not in sorted id position"
        );
    }

    #[test]
    fn test_can_enter_garrison_rejects_red_health_building() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        {
            let building = sim
                .substrate
                .entities
                .get_mut(bldg)
                .expect("building exists");
            building.health.current = 100;
        }

        assert!(
            !can_enter_garrison_fixture(&sim, &rules, pax, bldg),
            "CanDock rejects garrison entry at ConditionRed or below"
        );
    }

    #[test]
    fn test_can_enter_garrison_rejects_non_occupier() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
        let passenger = sim.substrate.entities.get(pax).expect("passenger exists");
        let transport = sim.substrate.entities.get(bldg).expect("building exists");
        let mut passenger_obj = rules.object("E1").expect("E1 exists").clone();
        let transport_obj = rules.object("CAGAS01").expect("CAGAS01 exists");
        let cargo = transport.passenger_role.cargo().expect("cargo exists");

        passenger_obj.occupier = false;

        assert!(
            !can_enter_transport(
                passenger,
                transport,
                &passenger_obj,
                transport_obj,
                cargo,
                &rules,
                &sim.houses,
                None,
            ),
            "CanDock requires Occupier=yes infantry"
        );
    }

    #[test]
    fn test_can_enter_garrison_rejects_full_building_at_capacity() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        {
            let building = sim
                .substrate
                .entities
                .get_mut(bldg)
                .expect("building exists");
            let cargo = building.passenger_role.cargo_mut().expect("cargo exists");
            for occupant_id in 1000..1005 {
                assert!(cargo.board(occupant_id, 1));
            }
            assert_eq!(cargo.count(), cargo.capacity);
        }

        assert!(
            !can_enter_garrison_fixture(&sim, &rules, pax, bldg),
            "CanDock rejects exactly full garrisons"
        );
    }

    #[test]
    fn test_can_enter_garrison_rejects_non_friendly_non_civilian_building() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Russians", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        assert!(
            !can_enter_garrison_fixture(&sim, &rules, pax, bldg),
            "CanDock rejects non-friendly occupied-owner buildings"
        );
    }

    #[test]
    fn test_can_enter_garrison_allows_neutral_and_special_buildings() {
        for owner in ["Neutral", "Special"] {
            let mut sim = Simulation::new();
            let rules = garrison_test_rules();
            insert_stamped_house(&mut sim, &rules, owner, owner);
            let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", owner, 10, 10);
            let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

            assert!(
                can_enter_garrison_fixture(&sim, &rules, pax, bldg),
                "CanDock allows {owner} civilian garrison buildings"
            );
        }
    }

    /// Insert a house the way production does — the `MultiplayPassive` flag is
    /// resolved from the country rules once, at creation, and stamped onto the
    /// house for every later reader.
    fn insert_stamped_house(
        sim: &mut Simulation,
        rules: &RuleSet,
        house_name: &str,
        country_name: &str,
    ) -> InternedId {
        let name_id = sim.interner.intern(house_name);
        let country_id = sim.interner.intern(country_name);
        let mut house =
            crate::sim::house_state::HouseState::new(name_id, 0, Some(country_id), false, 0, 10);
        house.multiplay_passive =
            crate::sim::house_state::resolve_multiplay_passive(Some(rules), Some(country_name));
        sim.houses.insert(name_id, house);
        name_id
    }

    #[test]
    fn test_can_enter_garrison_uses_owner_country_multiplay_passive() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        // House name and country deliberately differ: the passive fact must come
        // from `Country=Neutral`, not from the house's own name.
        insert_stamped_house(&mut sim, &rules, "CivHouse", "Neutral");
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "CivHouse", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        assert!(
            can_enter_garrison_fixture(&sim, &rules, pax, bldg),
            "CanDock allows non-owner entry through target owner country MultiplayPassive"
        );
    }

    /// BuildingClass::CanDock tests the entering infantry (`0x00457D98`), not
    /// the building: a controlled occupier is refused, a controlled building
    /// is not.
    #[test]
    fn test_can_enter_garrison_rejects_a_mind_controlled_occupier() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
        sim.substrate.entities.get_mut(bldg).unwrap().mind_control =
            crate::sim::capture_manager::MindControlLink::controlled_by_for_test(999);
        assert!(
            can_enter_garrison_fixture(&sim, &rules, pax, bldg),
            "a controlled building does not refuse by itself"
        );
        sim.substrate.entities.get_mut(pax).unwrap().mind_control =
            crate::sim::capture_manager::MindControlLink::controlled_by_for_test(999);
        assert!(
            !can_enter_garrison_fixture(&sim, &rules, pax, bldg),
            "CanDock rejects an occupier whose IsMindControlled is true"
        );
    }

    #[test]
    fn test_can_enter_garrison_rejects_out_of_bounds_target_when_grid_available() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
        let grid = crate::sim::pathfinding::PathGrid::new(5, 5);

        assert!(
            !can_entity_enter_garrison(&sim, &rules, pax, bldg, Some(&grid)),
            "CanDock rejects buildings outside the playfield bounds"
        );
    }

    #[test]
    fn test_can_enter_garrison_rejects_building_up_or_down() {
        let rules = garrison_test_rules();

        {
            let mut sim = Simulation::new();
            let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
            let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
            let building = sim
                .substrate
                .entities
                .get_mut(bldg)
                .expect("building exists");
            building.building_up = Some(crate::sim::components::BuildingUp::completing_in_ticks(
                30, 0,
            ));

            assert!(
                !can_enter_garrison_fixture(&sim, &rules, pax, bldg),
                "CanDock rejects buildings still playing build-up"
            );
        }

        {
            let mut sim = Simulation::new();
            let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
            let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);
            let building = sim
                .substrate
                .entities
                .get_mut(bldg)
                .expect("building exists");
            building.building_down = Some(crate::sim::components::BuildingDown::commenced(
                [0, 30, 1],
                0,
                false,
            ));

            assert!(
                !can_enter_garrison_fixture(&sim, &rules, pax, bldg),
                "CanDock rejects buildings in reverse build-down"
            );
        }
    }

    #[test]
    fn test_first_occupant_emits_garrisoned_event() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let _pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        tick_boarding(&mut sim, &rules);

        let mut found_eva = false;
        let mut found_sfx = false;
        for evt in &sim.sound_events {
            match evt {
                SimSoundEvent::StructureGarrisoned { owner } => {
                    assert_eq!(
                        sim.interner.resolve(*owner),
                        "Americans",
                        "EVA owner should be the garrisoning player"
                    );
                    found_eva = true;
                }
                SimSoundEvent::BuildingGarrisonedSfx { owner, rx, ry } => {
                    assert_eq!(sim.interner.resolve(*owner), "Americans");
                    assert_eq!((*rx, *ry), (10, 10));
                    found_sfx = true;
                }
                _ => {}
            }
        }
        assert!(found_eva, "expected StructureGarrisoned event");
        assert!(found_sfx, "expected BuildingGarrisonedSfx event");
    }

    #[test]
    fn test_boarding_inside_transition_clears_live_radio_contacts() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        sim.substrate
            .entities
            .get_mut(pax)
            .unwrap()
            .mark_live_contact_with(bldg);
        sim.substrate
            .entities
            .get_mut(bldg)
            .unwrap()
            .mark_live_contact_with(pax);

        tick_boarding(&mut sim, &rules);

        assert!(matches!(
            sim.substrate.entities.get(pax).unwrap().passenger_role,
            PassengerRole::Inside { transport_id } if transport_id == bldg
        ));
        assert!(
            sim.substrate
                .entities
                .get(pax)
                .unwrap()
                .radio_contacts
                .is_empty()
        );
        assert!(
            !sim.substrate
                .entities
                .get(bldg)
                .unwrap()
                .has_live_contact_with(pax),
            "boarding hide should clear peer radio contacts to the passenger"
        );
    }

    #[test]
    fn test_second_occupant_emits_no_garrison_event() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);

        // Pre-populate with one occupant (simulating a previous successful board).
        if let Some(t) = sim.substrate.entities.get_mut(bldg) {
            if let Some(cargo) = t.passenger_role.cargo_mut() {
                cargo.board(9999, 1);
            }
        }
        let _pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        tick_boarding(&mut sim, &rules);

        for evt in &sim.sound_events {
            match evt {
                SimSoundEvent::StructureGarrisoned { .. }
                | SimSoundEvent::BuildingGarrisonedSfx { .. } => {
                    panic!(
                        "garrison event should NOT emit on non-first occupant: {:?}",
                        evt
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_last_occupant_emits_abandoned_event_with_pre_revert_owner() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        // Spawn a CanBeOccupied building owned by Americans (post-garrison state),
        // with garrison_original_owner = Neutral (pre-garrison state).
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let neutral_id = sim.interner.intern("Neutral");
        // Set up the "1 occupant inside, original owner = Neutral" state.
        if let Some(t) = sim.substrate.entities.get_mut(bldg) {
            t.garrison_original_owner = Some(neutral_id);
            if let Some(cargo) = t.passenger_role.cargo_mut() {
                // Pretend a passenger entity 12345 was inside.
                cargo.board(12345, 1);
            }
            t.order_intent = Some(OrderIntent::Unloading);
        }
        // Spawn a placeholder passenger entity so unload_first finds it.
        let pax_owner = sim.interner.intern("Americans");
        let pax_type = sim.interner.intern("E1");
        let mut pax = GameEntity::test_default(12345, "E1", "Americans", 9, 10);
        pax.owner = pax_owner;
        pax.type_ref = pax_type;
        pax.passenger_role = PassengerRole::Inside { transport_id: bldg };
        sim.substrate.entities.insert(pax);

        // Tick unloading — should pop the one passenger and trigger empty branch.
        let changed = tick_passenger_system(&mut sim, &rules);
        assert!(
            changed,
            "last-occupant normal unload should report ownership change in the same tick"
        );
        let unloaded = sim.substrate.entities.get(12345).unwrap();
        assert!(unloaded.lifecycle.object_alive);
        assert!(!unloaded.lifecycle.in_limbo);
        assert!(unloaded.lifecycle.cell_marked);
        assert!(unloaded.in_logic_vector);

        // Assert StructureAbandoned was emitted with the PRE-revert owner (Americans).
        let mut found = false;
        for evt in &sim.sound_events {
            if let SimSoundEvent::StructureAbandoned { owner } = evt {
                assert_eq!(
                    sim.interner.resolve(*owner),
                    "Americans",
                    "StructureAbandoned should carry pre-revert owner, not post-revert civilian"
                );
                found = true;
            }
        }
        assert!(
            found,
            "expected StructureAbandoned event after last occupant left"
        );

        // Confirm the revert actually happened (post-revert owner = Neutral).
        let bldg_owner_str = sim
            .substrate
            .entities
            .get(bldg)
            .map(|t| sim.interner.resolve(t.owner).to_string())
            .expect("building exists");
        assert_eq!(
            bldg_owner_str, "Neutral",
            "owner should have reverted to Neutral"
        );

        let abandoned_events = sim
            .sound_events
            .iter()
            .filter(|evt| matches!(evt, SimSoundEvent::StructureAbandoned { .. }))
            .count();
        let changed_again = tick_passenger_system(&mut sim, &rules);
        assert!(
            !changed_again,
            "empty revert must not be delayed into the next passenger pass"
        );
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|evt| matches!(evt, SimSoundEvent::StructureAbandoned { .. }))
                .count(),
            abandoned_events,
            "StructureAbandoned should emit on the unload/reconcile tick only"
        );
    }

    #[test]
    fn test_non_garrison_transport_emits_no_garrison_events() {
        // Passengers=5 IFV-style transport (not CanBeOccupied) — no garrison events.
        let ini_str = "\
[InfantryTypes]
0=E1
[VehicleTypes]
0=IFV
[AircraftTypes]
[BuildingTypes]

[E1]
Name=Conscript
Cost=100
Strength=125
Armor=none
Speed=4
Occupier=yes

[IFV]
Name=IFV
Cost=600
Strength=200
Armor=light
Speed=8
Passengers=5

[General]
FixtureOnly=1
[AudioVisual]
ConditionRed=25%
ConditionYellow=50%
";
        let ini = IniFile::from_str(ini_str);
        let rules = RuleSet::from_ini(&ini).expect("parse");
        let mut sim = Simulation::new();
        let bldg_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("IFV");
        let mut bldg = GameEntity::test_default(bldg_id, "IFV", "Americans", 10, 10);
        bldg.owner = owner_id;
        bldg.type_ref = type_id;
        bldg.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(5, 0),
        };
        sim.substrate.entities.insert(bldg);
        let _pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg_id, 10, 11);

        tick_boarding(&mut sim, &rules);

        for evt in &sim.sound_events {
            match evt {
                SimSoundEvent::StructureGarrisoned { .. }
                | SimSoundEvent::BuildingGarrisonedSfx { .. } => {
                    panic!(
                        "non-garrison transport should not emit garrison events: {:?}",
                        evt
                    );
                }
                _ => {}
            }
        }
    }

    /// Helper: insert an Occupier infantry directly into a garrison building's
    /// cargo (skipping the boarding flow). Used by destruction-eject tests.
    fn place_inside_garrison(
        sim: &mut Simulation,
        rules: &RuleSet,
        building_id: u64,
        type_ref: &str,
        owner_str: &str,
    ) -> u64 {
        let stable_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern(owner_str);
        let type_id = sim.interner.intern(type_ref);
        let mut ge = GameEntity::test_default(stable_id, type_ref, owner_str, 0, 0);
        ge.owner = owner_id;
        ge.type_ref = type_id;
        ge.passenger_role = PassengerRole::Inside {
            transport_id: building_id,
        };
        sim.substrate.entities.insert(ge);
        // Add to building's cargo.
        if let Some(bldg) = sim.substrate.entities.get_mut(building_id) {
            if let Some(cargo) = bldg.passenger_role.cargo_mut() {
                let obj = rules.object(type_ref).expect("type exists");
                cargo.board(stable_id, obj.size.max(1));
            }
        }
        // Building inherits garrisoning player's ownership (sim does this on
        // first board). For destruction tests we set it explicitly here, and
        // also set category=Structure since GameEntity::test_default leaves it
        // as Unit — the death-loop branch keys on Structure.
        if let Some(bldg) = sim.substrate.entities.get_mut(building_id) {
            if bldg.garrison_original_owner.is_none() {
                bldg.garrison_original_owner = Some(bldg.owner);
            }
            bldg.owner = owner_id;
            bldg.category = crate::map::entities::EntityCategory::Structure;
        }
        stable_id
    }

    /// Construct a `DestroyedGarrisonBuilding` event from a still-alive
    /// building's state, eject/detach cargo while the building is still alive,
    /// then route the building through UnInit and the test drain. This tests the
    /// helper end-to-end without needing a full combat tick + damage events.
    fn eject_via_event(sim: &mut Simulation, rules: &RuleSet, building_id: u64) -> Vec<u64> {
        let event = {
            let bldg = sim
                .substrate
                .entities
                .get(building_id)
                .expect("building present");
            let cargo = bldg.passenger_role.cargo().expect("cargo present");
            let obj = rules
                .object(sim.interner.resolve(bldg.type_ref))
                .expect("type exists");
            let (foundation_w, foundation_h) =
                crate::sim::production::foundation_dimensions(&obj.foundation);
            crate::sim::combat::DestroyedGarrisonBuilding {
                building_id,
                type_id: bldg.type_ref,
                owner: bldg.owner,
                rx: bldg.position.rx,
                ry: bldg.position.ry,
                z: bldg.position.z,
                foundation_w,
                foundation_h,
                passenger_ids: cargo.passengers.clone(),
            }
        };
        let survivor_ids = event.passenger_ids.clone();
        crate::sim::production::eject_destruction_garrison(sim, rules, &event);
        sim.uninit(building_id);
        sim.process_pending_delete();
        survivor_ids
    }

    #[test]
    fn test_garrison_eject_on_destruction_happy_path() {
        let rules = garrison_test_rules();
        let mut sim = Simulation::new();
        let building_id = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Allied", 10, 10);
        let pax1 = place_inside_garrison(&mut sim, &rules, building_id, "E1", "Allied");
        let pax2 = place_inside_garrison(&mut sim, &rules, building_id, "E1", "Allied");
        let pax3 = place_inside_garrison(&mut sim, &rules, building_id, "E1", "Allied");

        let survivor_ids = eject_via_event(&mut sim, &rules, building_id);
        assert_eq!(survivor_ids.len(), 3, "all 3 occupants captured");

        // Building gone.
        assert!(
            sim.substrate.entities.get(building_id).is_none(),
            "building despawned"
        );

        let expected_positions = [(12, 12), (12, 12), (12, 12)];
        for (pid, expected_position) in [
            (pax1, expected_positions[0]),
            (pax2, expected_positions[1]),
            (pax3, expected_positions[2]),
        ] {
            let pax = sim.substrate.entities.get(pid).expect("survivor present");
            assert!(pax.is_alive(), "occupant {pid} should be alive");
            assert!(!pax.dying, "occupant {pid} should not be dying");
            assert!(matches!(pax.passenger_role, PassengerRole::None));
            assert_eq!(
                sim.interner.resolve(pax.owner),
                "Allied",
                "occupant {pid} should retain garrisoning owner"
            );
            assert_eq!(
                (pax.position.rx, pax.position.ry),
                expected_position,
                "occupant {pid} should reuse the one SellBuilding exit coordinate"
            );
        }
    }

    #[test]
    fn test_garrison_eject_blocked_edge_cells_kills_occupants() {
        let rules = garrison_test_rules();
        let mut sim = Simulation::new();
        let building_id = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Allied", 10, 10);
        let pax = place_inside_garrison(&mut sim, &rules, building_id, "E1", "Allied");

        // Block all sell-style edge ejection cells around the 2x2 building.
        let owner_id = sim.interner.intern("Allied");
        for (bx, by) in [
            (12, 11),
            (11, 12),
            (12, 10),
            (10, 12),
            (12, 12),
            (11, 9),
            (9, 11),
            (10, 9),
            (12, 9),
            (9, 10),
            (9, 12),
            (9, 9),
        ] {
            let blocker_id = sim.allocate_stable_id();
            let mut blocker = GameEntity::test_default(blocker_id, "E1", "Allied", bx, by);
            blocker.category = EntityCategory::Infantry;
            blocker.owner = owner_id;
            blocker.type_ref = sim.interner.intern("E1");
            blocker.sub_cell = Some(2);
            (blocker.position.sub_x, blocker.position.sub_y) =
                lepton::subcell_lepton_offset(Some(2));
            sim.substrate.entities.insert(blocker);
            assert!(matches!(
                sim.reveal(blocker_id),
                RevealOutcome::Revealed { .. }
            ));
        }

        eject_via_event(&mut sim, &rules, building_id);

        assert!(
            !sim.substrate.entities.contains(pax),
            "blocked occupant must reach UnInit and the shared test drain"
        );
        assert!(!sim.substrate.pending_delete.contains(&pax));
    }
    #[test]
    fn cargo_departure_garrison_reveal_failure_retains_order_and_recorded_size() {
        use crate::sim::combat::combat_weapon::WeaponOverride;
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let building = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let passenger = spawn_boarding_occupier(&mut sim, "E1", "Americans", building, 10, 11);
        process_boarding_passenger(&mut sim, &rules, passenger);
        {
            let carrier = sim.substrate.entities.get_mut(building).unwrap();
            carrier.weapon_override = Some(WeaponOverride::IfvSlot(99));
            carrier.order_intent = Some(OrderIntent::Unloading);
            let cargo = carrier.passenger_role.cargo_mut().unwrap();
            cargo.passenger_sizes[0] = 7;
            cargo.total_size = 7;
        }
        sim.substrate
            .entities
            .get_mut(passenger)
            .unwrap()
            .lifecycle
            .cell_marked = true;
        let held = serde_json::to_value(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .passenger_role
                .cargo(),
        )
        .unwrap();
        let rng_before = sim.scenario_rng.state();
        sim.sound_events.clear();
        process_unloading_transport(&mut sim, &rules, building);
        let carrier = sim.substrate.entities.get(building).unwrap();
        assert_eq!(
            serde_json::to_value(carrier.passenger_role.cargo()).unwrap(),
            held
        );
        assert_eq!(carrier.weapon_override, Some(WeaponOverride::IfvSlot(99)));
        assert!(matches!(carrier.order_intent, Some(OrderIntent::Unloading)));
        let pax = sim.substrate.entities.get(passenger).unwrap();
        assert_eq!(pax.passenger_role.inside_transport_id(), Some(building));
        assert!(pax.lifecycle.in_limbo && pax.lifecycle.cell_marked);
        assert!(!pax.in_logic_vector);
        assert_eq!(sim.scenario_rng.state(), rng_before);
        assert!(sim.sound_events.is_empty());
        println!(
            "CARGO_TRACE garrison {:?} {:?}",
            sim.scenario_rng.state(),
            held
        );
        sim.substrate
            .entities
            .get_mut(passenger)
            .unwrap()
            .lifecycle
            .cell_marked = false;
        process_unloading_transport(&mut sim, &rules, building);
        let carrier = sim.substrate.entities.get(building).unwrap();
        assert!(carrier.passenger_role.cargo().unwrap().is_empty());
        assert_eq!(carrier.passenger_role.cargo().unwrap().total_size, 0);
        assert!(carrier.weapon_override.is_none() && carrier.order_intent.is_none());
        let pax = sim.substrate.entities.get(passenger).unwrap();
        assert!(!pax.passenger_role.is_inside_transport());
        assert!(pax.lifecycle.cell_marked && pax.in_logic_vector);
    }
}
