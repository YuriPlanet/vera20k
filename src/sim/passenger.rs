//! Passenger/transport system — boarding, unloading, and cargo tracking.
//!
//! Handles infantry entering transports (Passengers>0), building garrisons
//! (CanBeOccupied=yes), IFV weapon swapping (Gunner=yes), and passenger
//! death on transport destruction. Vehicle/aircraft unloading is the Unload
//! mission handler in `crate::sim::transport_unload`, and a garrison's is the
//! building's own Unload mission (`BuildingClass::Mission_Unload`). The private
//! departure module owns cargo-head removal and complete retry restoration for
//! vehicles, landed aircraft and paradrops.
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

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::{ConcealOutcome, SimSoundEvent, Simulation};

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

/// Passenger/transport role for an entity. Replaces three separate Option fields
/// with a single enum that makes invalid states unrepresentable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PassengerRole {
    /// Entity has no passenger/transport role. Most entities are this.
    None,
    /// Entity is a transport or garrisonable building that can hold passengers.
    Transport { cargo: PassengerCargo },
    /// Entity is approaching a transport to board it.
    Boarding { target_transport_id: u64 },
    /// Entity is inside a transport (hidden from map, not targetable).
    Inside {
        transport_id: u64,
        /// `TechnoClass+0x82` InOpenToppedTransport. Only
        /// `SetInOpenTransport @ 0x00710470` sets it, when an `OpenTopped=`
        /// transport takes the passenger aboard (Infantry `0x0051A45E`, Unit
        /// `0x0073A75D`). Its clears coincide with leaving: a successful unload
        /// (`ClearInOpenTransport`, `0x0073DB98`) and the dying transport's
        /// passenger block (`ClearAllInOpenTransport`, `0x00737F92`); a failed
        /// unload re-attaches the passenger with the flag still set.
        open_topped: bool,
    },
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
            Self::Inside { transport_id, .. } => Some(*transport_id),
            _ => Option::None,
        }
    }

    /// `TechnoClass+0x82` with its `+0x11C` Transporter: the `OpenTopped=`
    /// transport this entity rides in, if any. Read by weapon selection, the
    /// fire coordinate, InRange, GetFireError, FireAt's damage and muzzle
    /// anim, kill credit, navigation and the Temporal warp-distance check.
    pub fn open_transport_id(&self) -> Option<u64> {
        match self {
            Self::Inside {
                transport_id,
                open_topped: true,
            } => Some(*transport_id),
            _ => Option::None,
        }
    }

    /// `TechnoClass+0x82` InOpenToppedTransport.
    pub fn in_open_transport(&self) -> bool {
        self.open_transport_id().is_some()
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
    if building.building_up() || building.building_down() {
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
        sim.path_grid(),
    )
}

/// Maximum cell distance for a passenger to be considered "at" the transport.
/// Chebyshev distance in cells — 1 means same cell or adjacent.
const BOARD_DISTANCE: u32 = 1;

/// Advance the passenger boarding/unloading system each tick.
///
/// Phase A: For entities with `boarding_state`, check if they arrived at
/// the transport's cell. If so, execute boarding. If the transport is
/// destroyed or full, cancel boarding.
///
/// Phase B: each building reconciles its garrison owner on its own turn.
///
/// Nothing unloads here: a garrison's Unload is the building's mission
/// (`BuildingClass::Mission_Unload @ 0x0044D880`, `sim::world::techno_ai`)
/// and a vehicle or aircraft transport's is `crate::sim::transport_unload`.
///
/// Returns `true` if any entity's ownership changed this tick (garrison
/// transfer or revert), signalling that the sprite atlas needs a rebuild.
pub fn tick_passenger_system(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let order = sim.live_object_order_snapshot();
    tick_boarding_and_garrison_reconciliation_in_order(sim, rules, registry, &order)
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
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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
            process_boarding_passenger(sim, rules, entity_id, registry);
        }

        ownership_changed |=
            reconcile_civilian_garrison_owner_for_building(sim, rules, registry, entity_id);
    }
    ownership_changed
}

fn process_boarding_passenger(
    sim: &mut Simulation,
    rules: &RuleSet,
    pax_id: u64,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
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
            // `Set_ArchiveTarget(0)` opens both (`0x0051A3FF`, `0x0073A6EC`).
            pax.set_archive_target(None);
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
            sim.free_unit(controller, pax_id, rules, registry);
        }
        //Infantry51A37C/Unit73A30E invalidates positive ExtraPower
        //absorbers before AddPassenger. Ordinary mobile transports do not
        //write this House byte. The shared boarding owner handles both.
        if let Some(owner) = sim
            .substrate
            .entities
            .get(transport_id)
            .and_then(|transport| {
                rules
                    .object(sim.interner.resolve(transport.type_ref()))
                    .and_then(|object| {
                        (transport.category == EntityCategory::Structure
                            && object.extra_power > 0
                            && (object.infantry_absorb || object.unit_absorb))
                            .then_some(transport.owner())
                    })
            })
        {
            sim.invalidate_house_power(owner, false);
        }
        // CargoClass::AddPassenger conceals the passenger before splicing it
        // into the cargo chain. Techno Limbo owns BREAK, Mark removal, and
        // LogicVector removal in that order.
        if sim.techno_limbo_with_rules(pax_id, rules, registry) != ConcealOutcome::Concealed {
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
            let _ = sim.reveal_entity_with_rules(pax_id, rules);
            return;
        }

        //Building52298D refreshes its spatial threat after appending the
        //occupant, before the first-occupant Mark/discovery tail. Ordinary
        //transport cargo does not call Techno70F6E0 here.
        if transport_obj.is_some_and(|object| object.can_be_occupied) {
            sim.refresh_spatial_threat(transport_id, rules, None);
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
            pax.passenger_role = PassengerRole::Inside {
                transport_id,
                open_topped: transport_open_topped,
            };
            pax.movement_target = None;
            if !transport_open_topped {
                pax.attack_target = None;
                pax.passively_acquired_target = false;
            }
            pax.order_intent = None;
        }
        if transport_open_topped {
            // `SetInOpenTransport @ 0x00710470` (Infantry `0x0051A45E`, Unit
            // `0x0073A75D`): `+0x82` (the role's flag above), then
            // `ResetOrdersToGuard` (vt+0x3D0) and the LogicClass add, so the
            // rider keeps an AI turn and guards from inside.
            // Infantry Limbo51DF10 keeps TarCom: 51A441 ->51A45E ->710484
            // reaches the class NULL setter with the former target intact.
            // Clearing it above would bypass the changed-target effects.
            sim.reset_orders_to_guard(pax_id, rules);
            let registered = sim.register_open_topped_passenger(pax_id);
            debug_assert!(
                registered,
                "accepted open-topped passenger must remain active"
            );
            // The rider stands where its walk into the transport ended, and
            // every later transport `SetLocation` copies the transport's
            // coordinate onto it (`FootClass::SetLocation 0x004DB810` ->
            // `0x007104F0`). VERA boards from the adjacent cell, so the rider
            // takes the transport's coordinate here.
            //
            // RESIDUAL: native boards only once the passenger stands in the
            // transport's own cell (`0x0051A3A0..0x0051A3DF`), where the rider
            // keeps its own spot until the transport first moves; VERA boards
            // within `BOARD_DISTANCE`. Trigger: every boarding. Effect: the
            // passenger vanishes a cell early, and a parked open-topped
            // transport's riders measure range from its centre rather than
            // their spot, under half a cell apart. Frequency: every boarding.
            // Risk: a target at the edge of a parked rider's reach is in range
            // one frame early or late.
            //
            // RESIDUAL: native clears OnBridge (`+0x8C`, `0x0051A407`) on
            // every boarding; VERA keeps the boarding value because its unload
            // Reveal reads it to place the passenger on a bridge deck.
            if let Some(position) = sim
                .substrate
                .entities
                .get(transport_id)
                .map(|transport| transport.position)
                && let Some(pax) = sim.substrate.entities.get_mut(pax_id)
            {
                pax.position = position;
            }
        }
        // A boarding open-topped unit's own riders let go of their targets
        // (Unit `0x0073A76E..0x0073A77C`).
        sim.open_topped_passengers_take_target(pax_id, None, rules);

        if transport_gunner {
            receive_gunner(sim, rules, transport_id, pax_id);
        }
    } else if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
        pax.passenger_role = PassengerRole::None;
    }
}

/// Gunner caller dispatch to UnitClass +4D4 (746420). Temporal ownership
/// moves first; its mode7 step precedes the final passenger IFVMode selector.
/// The existing temporal rearm handover residual is recorded in temporal.rs.
fn receive_gunner(sim: &mut Simulation, rules: &RuleSet, transport_id: u64, pax_id: u64) {
    let Some(transport) = sim.substrate.entities.get(transport_id) else {
        return;
    };
    let Some(object) = rules.object(sim.interner.resolve(transport.type_ref())) else {
        return;
    };
    // Aircraft/Foot bind this virtual slot to the 4DE750 stub.
    if transport.category != crate::map::entities::EntityCategory::Unit || !object.gunner {
        return;
    }
    let Some(passenger) = sim.substrate.entities.get(pax_id) else {
        return;
    };
    let mode = rules
        .object(sim.interner.resolve(passenger.type_ref()))
        .map_or(0, |p| p.ifv_mode);
    if sim.temporal_receive_gunner(transport_id, pax_id)
        && let Some(transport) = sim.substrate.entities.get_mut(transport_id)
    {
        transport.set_gunner_weapon(7, object);
    }
    if let Some(transport) = sim.substrate.entities.get_mut(transport_id) {
        transport.set_gunner_weapon(mode, object);
    }
}

/// Gunner caller dispatch to UnitClass +4D8 (7464E0). RemoveGunner always
/// selects mode0 after optional temporal return/LetGo, even with no gunner.
fn remove_gunner(sim: &mut Simulation, rules: &RuleSet, transport_id: u64, pax_id: Option<u64>) {
    let Some(transport) = sim.substrate.entities.get(transport_id) else {
        return;
    };
    let Some(object) = rules.object(sim.interner.resolve(transport.type_ref())) else {
        return;
    };
    // Aircraft/Foot bind this virtual slot to the 4DE760 stub.
    if transport.category != crate::map::entities::EntityCategory::Unit || !object.gunner {
        return;
    }
    if let Some(pax_id) = pax_id {
        sim.temporal_remove_gunner(transport_id, pax_id);
    }
    if let Some(transport) = sim.substrate.entities.get_mut(transport_id) {
        transport.set_gunner_weapon(0, object);
    }
}

/// `FootClass::SetLocation @ 0x004DB810`'s `OpenTopped=` tail (`0x004DB88A`
/// -> `0x007104F0`): each rider of an open-topped transport takes the
/// transport's coordinate, walked from the cargo head. Its one caller is that
/// port, [`foot_set_location`], after a changed Location.
///
/// [`foot_set_location`]: crate::sim::movement::ground_pose::foot_set_location
pub(crate) fn open_topped_riders_follow(
    entities: &mut crate::sim::entity_store::EntityStore,
    transport_id: u64,
    rules: &RuleSet,
    interner: &StringInterner,
) {
    let Some(transport) = entities.get(transport_id) else {
        return;
    };
    // Cargo first: every changed SetLocation asks, and the type lookup costs
    // an allocation.
    let Some(cargo) = transport
        .passenger_role
        .cargo()
        .filter(|cargo| !cargo.passengers.is_empty())
    else {
        return;
    };
    if !rules
        .object(interner.resolve(transport.type_ref()))
        .is_some_and(|object| object.open_topped)
    {
        return;
    }
    let (riders, position) = (cargo.passengers.clone(), transport.position);
    for rider in riders {
        if let Some(entity) = entities.get_mut(rider) {
            entity.position = position;
        }
    }
}

impl Simulation {
    /// `TechnoClass::SetTargetForPassengers @ 0x00710550` behind its callers'
    /// `OpenTopped=` test (`+0x5E4`): every passenger of an open-topped
    /// `transport_id`, from the cargo head, takes `target` through its own
    /// Assign_Target (vt+0x3C8). Callers: an order event with a target
    /// (MEGAMISSION `0x004C749D`), the Stop event (`0x004C7650`, NULL),
    /// DecideUnitFate (`0x0047240F`, NULL) and a unit boarding with riders of
    /// its own (`0x0073A77C`, NULL).
    pub(crate) fn open_topped_passengers_take_target(
        &mut self,
        transport_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
        rules: &RuleSet,
    ) {
        let Some(transport) = self.substrate.entities.get(transport_id) else {
            return;
        };
        if !self
            .object_type(transport.type_ref(), rules)
            .is_some_and(|object| object.open_topped)
        {
            return;
        }
        let passengers: Vec<u64> = transport
            .passenger_role
            .cargo()
            .map(|cargo| cargo.passengers.clone())
            .unwrap_or_default();
        for passenger in passengers {
            // Original71056C dispatches each receiver's virtual+3C8, including
            // Infantry51B1F0's synchronous action/path/latch effects.
            let _ = self.assign_target_represented(passenger, target, Some(rules));
        }
    }
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
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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
        crate::sim::production::sell_building_occupants(sim, rules, registry, building_id);
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
        sim.change_owner_with_rules(building_id, new_owner, rules, registry);
        return true;
    }

    if cargo_empty && !is_civilian_garrison_owner(&sim.interner, current_owner) {
        let civilian_owner = resolved_civilian_garrison_owner(sim);
        sim.sound_events.push(SimSoundEvent::StructureAbandoned {
            owner: current_owner,
        });
        sim.change_owner_with_rules(building_id, civilian_owner, rules, registry);
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
        // This direct boarding fixture declares no overlay context.
        process_boarding_passenger(sim, rules, id, None);
    }
    false
}

#[cfg(test)]
mod gunner_tests;

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
    use crate::sim::command::Command;
    use crate::sim::mission::{MissionId, MissionType};
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
        let art = IniFile::from_str("[CAGAS01]\nFoundation=2x2\n");
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("parse garrison test rules")
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
        let mut ge = GameEntity::test_default_of_category(
            stable_id,
            type_ref,
            owner_str,
            rx,
            ry,
            EntityCategory::Structure,
        );
        ge.mission_leaf =
            crate::sim::mission::leaf::MissionLeafState::for_entity_category(ge.category);
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
            sim.reveal_entity_with_rules(stable_id, rules),
            RevealOutcome::Revealed { .. }
        ));
        if sim.resolved_terrain.is_some() {
            assert!(
                sim.substrate
                    .entities
                    .get(stable_id)
                    .unwrap()
                    .cached_spatial_threat()
                    .is_some(),
                "admitted fixture Unlimbo publishes Techno+508"
            );
        }
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
            sim.reveal_entity_with_rules(stable_id, rules),
            RevealOutcome::Revealed { .. }
        ));
        if sim.resolved_terrain.is_some() {
            assert!(
                sim.substrate
                    .entities
                    .get(stable_id)
                    .unwrap()
                    .cached_spatial_threat()
                    .is_some(),
                "admitted fixture Unlimbo publishes Techno+508"
            );
        }
        stable_id
    }

    /// Spawn an Occupier infantry entity at (rx, ry) in `Boarding::Entering` state
    /// targeting `transport_id`.
    fn spawn_boarding_occupier(
        sim: &mut Simulation,
        rules: &RuleSet,
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
        ge.category = EntityCategory::Infantry;
        ge.mission_leaf =
            crate::sim::mission::leaf::MissionLeafState::for_entity_category(ge.category);
        ge.passenger_role = PassengerRole::Boarding {
            target_transport_id: transport_id,
        };
        sim.substrate.entities.insert(ge);
        assert!(matches!(
            sim.reveal_entity_with_rules(stable_id, rules),
            RevealOutcome::Revealed { .. }
        ));
        if sim.resolved_terrain.is_some() {
            assert!(
                sim.substrate
                    .entities
                    .get(stable_id)
                    .unwrap()
                    .cached_spatial_threat()
                    .is_some(),
                "admitted fixture Unlimbo publishes Techno+508"
            );
        }
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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

        let changed = tick_boarding(&mut sim, &rules);

        assert!(!changed, "boarding must not report ownership transfer");
        assert_eq!(owner_name(&sim, bldg), "Neutral");
        assert!(matches!(
            sim.substrate.entities.get(pax).unwrap().passenger_role,
            PassengerRole::Inside { transport_id, .. } if transport_id == bldg
        ));
    }

    #[test]
    fn garrison_owner_transfers_same_frame_when_building_update_after_entry() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

        let changed = tick_boarding_and_garrison_reconciliation_in_order(
            &mut sim,
            &rules,
            None,
            &[pax, bldg],
        );

        assert!(changed);
        assert_eq!(owner_name(&sim, bldg), "Americans");
    }

    #[test]
    fn production_garrison_owner_order_uses_live_object_order_not_stable_id() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
        assert!(
            bldg < pax,
            "fixture keeps stable-id order opposite native order"
        );
        sim.set_logic_order_for_test(vec![pax, bldg]);

        let changed = tick_passenger_system(&mut sim, &rules, None);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

        let changed = tick_boarding_and_garrison_reconciliation_in_order(
            &mut sim,
            &rules,
            None,
            &[bldg, pax],
        );

        assert!(!changed);
        assert_eq!(owner_name(&sim, bldg), "Neutral");

        let changed = tick_boarding_and_garrison_reconciliation_in_order(
            &mut sim,
            &rules,
            None,
            &[bldg, pax],
        );

        assert!(changed);
        assert_eq!(owner_name(&sim, bldg), "Americans");
    }

    #[test]
    fn garrison_reconciliation_uses_first_occupant_owner() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let first = spawn_boarding_occupier(&mut sim, &rules, "E1", "Russians", bldg, 10, 11);
        let second = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 11, 10);

        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            let cargo = building.passenger_role.cargo_mut().expect("cargo exists");
            assert!(cargo.board(first, 1));
            assert!(cargo.board(second, 1));
        }

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, None, bldg);

        assert!(changed);
        assert_eq!(owner_name(&sim, bldg), "Russians");
    }

    #[test]
    fn empty_captured_garrison_reverts_to_civilian_house_not_original_owner() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, None, bldg);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
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
            &mut sim, &rules, None, bldg
        ));
        assert_eq!(sim.houses[&neutral].tracking.buildings(), 0);
        assert_eq!(sim.houses[&americans].tracking.buildings(), 1);

        assert!(
            sim.substrate
                .entities
                .get_mut(bldg)
                .and_then(|building| building.passenger_role.cargo_mut())
                .expect("garrison cargo")
                .disembark(pax)
        );
        assert!(reconcile_civilian_garrison_owner_for_building(
            &mut sim, &rules, None, bldg
        ));
        assert_eq!(sim.houses[&neutral].tracking.buildings(), 1);
        assert_eq!(sim.houses[&americans].tracking.buildings(), 0);
    }

    #[test]
    fn red_hp_captured_garrison_ejects_and_reverts_in_same_reconciliation() {
        let rules = garrison_test_rules();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = place_inside_garrison(&mut sim, &rules, bldg, "E1", "Americans");

        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            building.health.current = 100;
            if let Some(cargo) = building.passenger_role.cargo_mut() {
                cargo.garrison_fire_index = 3;
            }
        }

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, None, bldg);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
        assert!(sim.live_object_order_snapshot().contains(&pax));

        tick_boarding_and_garrison_reconciliation_in_order(&mut sim, &rules, None, &[pax, bldg]);

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
        let passenger =
            spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", transport, 10, 11);

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

        tick_passenger_system(&mut sim, &rules, None);

        let passenger_entity = sim
            .substrate
            .entities
            .get(passenger)
            .expect("passenger survives boarding");
        assert!(matches!(
            passenger_entity.passenger_role,
            PassengerRole::Inside { transport_id, .. } if transport_id == transport
        ));
        assert!(passenger_entity.lifecycle.in_limbo);
        assert!(!passenger_entity.lifecycle.cell_marked);
        assert!(passenger_entity.in_logic_vector);
        assert_eq!(
            sim.live_object_order_snapshot(),
            vec![transport, tail, passenger]
        );

        tick_passenger_system(&mut sim, &rules, None);
        assert_eq!(
            sim.live_object_order_snapshot(),
            vec![transport, tail, passenger],
            "an already-active contained passenger must not append twice"
        );
        sim.debug_assert_logic_membership_consistent();
    }

    /// Ejecting a garrison occupant reveals it: it re-enters the active order.
    #[test]
    fn garrison_eject_reveals_passenger_into_active_order() {
        let rules = garrison_test_rules();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
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

        let changed = reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, None, bldg);
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
        let rules = garrison_test_rules();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Neutral", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

        tick_boarding_and_garrison_reconciliation_in_order(&mut sim, &rules, None, &[pax, bldg]);
        assert!(!sim.live_object_order_snapshot().contains(&pax));

        if let Some(building) = sim.substrate.entities.get_mut(bldg) {
            building.health.current = 100;
            if let Some(cargo) = building.passenger_role.cargo_mut() {
                cargo.garrison_fire_index = 3;
            }
        }
        reconcile_civilian_garrison_owner_for_building(&mut sim, &rules, None, bldg);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
            let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
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
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
        let grid = crate::sim::pathfinding::PathGrid::new(5, 5);
        sim.install_fixture_path_grid(Some(&grid));

        assert!(
            !can_entity_enter_garrison(&sim, &rules, pax, bldg),
            "CanDock rejects buildings outside the playfield bounds"
        );
    }

    #[test]
    fn test_can_enter_garrison_rejects_building_up_or_down() {
        let rules = garrison_test_rules();

        {
            let mut sim = Simulation::new();
            let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
            let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
            let building = sim
                .substrate
                .entities
                .get_mut(bldg)
                .expect("building exists");
            building.install_building_up(
                crate::sim::components::BuildingUp::completing_in_ticks(30, 0),
                0,
            );

            assert!(
                !can_enter_garrison_fixture(&sim, &rules, pax, bldg),
                "CanDock rejects buildings still playing build-up"
            );
        }

        {
            let mut sim = Simulation::new();
            let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
            let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);
            let building = sim
                .substrate
                .entities
                .get_mut(bldg)
                .expect("building exists");
            building.bind_building_construction_control([0, 30, 1]);
            crate::sim::mission::authority::queue_entity_mission_deferred(
                building,
                crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Selling,
                ),
            );
            crate::sim::mission::authority::commence_entity_mission(building, 0);
            building
                .install_building_down(crate::sim::components::BuildingDown::commenced(0, false));

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
        let _pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
    fn open_topped_boarding_preserves_old_target_until_class_reset() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/base_defense_response.json",
        ))
        .unwrap();
        let row = native["infantry_assignment"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "target_clear")
            .unwrap();
        let rules = open_topped_test_rules();
        let mut sim = Simulation::new();
        let transport = spawn_transport(&mut sim, &rules, "BFRT", "Americans", 10, 10);
        let passenger =
            spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", transport, 10, 11);
        let actor = sim.substrate.entities.get_mut(passenger).unwrap();
        actor.health.current = row["input"]["health"].as_i64().unwrap() as i32;
        actor.attack_target = Some(crate::sim::combat::AttackTarget::new(transport));
        actor
            .mission_leaf
            .set_foot_firing_sequence(row["before"]["firing_latch"].as_u64().unwrap() as u8);

        tick_boarding(&mut sim, &rules);

        let actor = sim.substrate.entities.get(passenger).unwrap();
        assert!(matches!(
            actor.passenger_role,
            PassengerRole::Inside { transport_id, open_topped: true }
                if transport_id == transport
        ));
        assert!(actor.attack_target.is_none());
        // Original51B20E clears68D on a changed live target before any Doing
        // admission. Limbo51DF10 has already forced Ready during boarding;
        // therefore only this independent native latch output is transported
        // from target_clear, not that row's action/Stage result. The synthetic
        // rules intentionally lack ART; no full boarding parity is claimed.
        assert_eq!(
            actor.mission_leaf.foot_firing_sequence_latch(),
            row["after"]["firing_latch"].as_u64().unwrap() as u8,
            "SetInOpenTransport710484 must observe the former TarCom",
        );
    }

    #[test]
    fn test_boarding_inside_transition_clears_live_radio_contacts() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
            PassengerRole::Inside { transport_id, .. } if transport_id == bldg
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
        let _pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg, 10, 11);

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
        let rules = garrison_test_rules();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let pax = place_inside_garrison(&mut sim, &rules, bldg, "E1", "Americans");
        sim.substrate.entities.get_mut(pax).unwrap().locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ),
        );

        // An occupied building is armed (`0x00458DB0`), so its Guard dispatch
        // has raised the ready latch (`+0x6DD`, `0x00449701`).
        {
            let building = sim.substrate.entities.get_mut(bldg).unwrap();
            building.mission_leaf =
                crate::sim::mission::leaf::MissionLeafState::for_entity_category(
                    EntityCategory::Structure,
                );
            building.mission_leaf.set_building_ready_latch(1);
        }
        // The player's Unload queues the building's Unload mission; its next
        // ready check commences it and the dispatch ejects every occupant
        // (`SellBuilding(0, 0)`) and queues Guard.
        assert!(sim.apply_command(
            "Americans",
            &Command::UnloadPassengers { transport_id: bldg },
            Some(&rules),
        ));
        assert_eq!(
            sim.substrate.entities.get(bldg).unwrap().mission.queued(),
            MissionId::from_known(MissionType::Unload)
        );
        sim.session.binary_frame += 1;
        sim.object_ai_visit_one(
            bldg,
            Some(&rules),
            crate::sim::world::ObjectAiCtx::default(),
        );
        let building = sim.substrate.entities.get(bldg).unwrap();
        assert!(building.passenger_role.cargo().unwrap().is_empty());
        assert_eq!(
            building.mission.current(),
            MissionId::from_known(MissionType::Unload)
        );
        assert_eq!(
            building.mission.queued(),
            MissionId::from_known(MissionType::Guard)
        );
        assert_eq!(building.mission.dispatch_timer().delay(), 1);

        // The same frame's passenger pass reverts the emptied garrison.
        let changed = tick_passenger_system(&mut sim, &rules, None);
        assert!(
            changed,
            "last-occupant normal unload should report ownership change in the same tick"
        );
        let unloaded = sim.substrate.entities.get(pax).unwrap();
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
        let changed_again = tick_passenger_system(&mut sim, &rules, None);
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
        let _pax = spawn_boarding_occupier(&mut sim, &rules, "E1", "Americans", bldg_id, 10, 11);

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
        ge.category = EntityCategory::Infantry;
        ge.mission_leaf =
            crate::sim::mission::leaf::MissionLeafState::for_entity_category(ge.category);
        ge.passenger_role = PassengerRole::Inside {
            transport_id: building_id,
            open_topped: false,
        };
        sim.substrate.entities.insert(ge);
        // Add to building's cargo.
        if let Some(bldg) = sim.substrate.entities.get_mut(building_id) {
            if let Some(cargo) = bldg.passenger_role.cargo_mut() {
                let obj = rules.object(type_ref).expect("type exists");
                cargo.board(stable_id, obj.size.max(1));
            }
        }
        // Supplied first-board ownership precedes the native AddOccupant's
        // threat-refresh tail. The building helper already constructs the
        // Structure mission receiver; direct-cargo fixtures keep that class.
        if let Some(bldg) = sim.substrate.entities.get_mut(building_id) {
            bldg.owner = owner_id;
            bldg.category = crate::map::entities::EntityCategory::Structure;
        }
        sim.refresh_spatial_threat(building_id, rules, None);
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
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
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
    /// A failed unload re-attaches an open-topped rider at the head it left,
    /// with its recorded size and `+0x82` still set: only a successful unload
    /// clears the flag (`ClearInOpenTransport`, `0x0073DB98`), whatever the
    /// attempt did to the rider's role before it failed.
    #[test]
    fn a_failed_departure_keeps_the_riders_open_topped_flag() {
        let rules = garrison_test_rules();
        let mut sim = Simulation::new();
        let transport_id = sim.allocate_stable_id();
        let (front, back) = (sim.allocate_stable_id(), sim.allocate_stable_id());
        let mut cargo = PassengerCargo::new(5, 2);
        cargo.board_forced(back, 1);
        cargo.board_forced(front, 2);
        let mut transport = GameEntity::test_default(transport_id, "BFRT", "Americans", 10, 10);
        transport.passenger_role = PassengerRole::Transport { cargo };
        sim.substrate.entities.insert(transport);
        for id in [front, back] {
            let mut rider = GameEntity::test_default(id, "E1", "Americans", 10, 10);
            rider.passenger_role = PassengerRole::Inside {
                transport_id,
                open_topped: true,
            };
            sim.substrate.entities.insert(rider);
        }
        // `test_default` interns its names in the shared test interner.
        sim.interner = crate::sim::intern::test_interner();
        let held = serde_json::to_value(
            sim.substrate
                .entities
                .get(transport_id)
                .unwrap()
                .passenger_role
                .cargo(),
        )
        .unwrap();
        let result = departure::depart_cargo_head(
            &mut sim,
            &rules,
            None,
            transport_id,
            departure::DepartureRoute::Vehicle,
            |sim, id| {
                assert_eq!(id, front, "the head departs");
                sim.substrate.entities.get_mut(id).unwrap().passenger_role = PassengerRole::None;
                Err(departure::DepartureFailure::Placement)
            },
        );
        assert_eq!(result, Err(departure::DepartureFailure::Placement));
        let transport = sim.substrate.entities.get(transport_id).unwrap();
        assert_eq!(
            serde_json::to_value(transport.passenger_role.cargo()).unwrap(),
            held
        );
        for id in [front, back] {
            let rider = sim.substrate.entities.get(id).unwrap();
            assert_eq!(rider.passenger_role.open_transport_id(), Some(transport_id));
        }
    }
}
