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
//! CargoClass at Foot+0x114 holds a count and head at +0x118; cargo members
//! follow their Foot+0x30 link. This owner's Vec keeps that same head order.
//! Building garrisons use a separate append-order occupants vector.
//!
//! ## The way aboard: RNG draws, timer writes and detaches
//! An Enter order, team script action 14 and the Bio Reactor seek all take
//! this one route; each step lists what it writes.
//! - Order admission ([`can_enter_transport`]): the click's alliance test and
//!   the carrier's CAN_LOAD, sending nothing; no writes.
//! - The Infantry setter's Enter radio arm (`0x0051AC51..0x0051AD0F`): a
//!   busy Unit or Building becomes his pending entry (`+0x500`), his
//!   ArchiveTarget is cleared and Foot's setter takes NULL; a free Techno
//!   is sent HELLO (a contact both ways) and DOCKING, and any answer but
//!   ROGER or DOCKING BREAKs it (both ways).
//! - The Unit setter: a transport whose first contact is an infantryman
//!   BREAKs it when sent to a Techno it holds no contact with
//!   (`0x00741AD1..0x00741B22`); its Enter arm parks a Unit or `UnitAbsorb=`
//!   target in `+0x500` with the ArchiveTarget cleared (`0x00741C4F..
//!   0x00741CCC`, `0x00741CE5..0x00741D0D`).
//! - Every class setter call that takes a destination writes Foot's
//!   movement timers `+0x640` and `+0x668` and clears `+0x6B7`
//!   (`0x004D96C2..0x004D9707`).
//! - A transport's DOCKING (`0x007377D8..0x00737A95`): one with no contact
//!   HELLOs the sender; on Water or across zones it takes the sender as its
//!   destination (its setter's writes) and answers DOCKING; otherwise it asks
//!   NEED_TO_MOVE and sends MOVE_HERE with itself. The sender's Foot
//!   receiver (`0x004D9139`) answers ALREADY_THERE in the transport's cell;
//!   elsewhere it may queue Move (Guard with nothing queued), commences a
//!   queued Enter when ready, gives the transport to its own class setter
//!   (that setter's writes) and writes its UpdateTimer `{Frame, 0}`
//!   (`0x004D91F1`). Mission_Enter's dispatch epilogue overwrites that
//!   timer; at order time (the Infantry setter's DOCKING) it stands. A
//!   MOVE_HERE not answered ROGER makes the transport BREAK the sender
//!   (`0x00737A7F`), and it still answers ROGER.
//! - Mission_Enter (`0x004D9290`), each dispatch: DOCKING to Contacts[0] or
//!   the ArchiveTarget, and on any answer but ROGER, untethered, BREAK and
//!   idle (Guard or AreaGuard queued); with neither, the pending entry is
//!   walked to (its setter, Move queued; a dead one clears `+0x500`) or the
//!   Foot idles. Then the timer: `ftol(Rate * 900)` plus one Scenario
//!   `RandomRanged(0, 2)` (`0x004D946C..0x004D9497`).
//! - FootClass::AI's TryEnterTransport (`0x0070D7E0`) for a `+0x500` entry:
//!   HELLO; CAN_LOAD ROGER queues Enter, sets the destination and clears
//!   `+0x500`; any other answer queues NONE, sets NULL, clears `+0x500` and
//!   BREAKs.
//! - Arrival (`sim::movement::per_cell`): an infantryman's carrier's Tag
//!   hears event 1, whose triggers may act; the Infantry building arm sends
//!   DOCK_NOW (a depot queues its Repair and his Sleep, a refinery his
//!   Unload). A refusal sets NULL, queues Guard and Scatters from no
//!   coordinate: one Scenario `RandomRanged(0, 4)` once an infantryman's
//!   Scatter is admitted (`0x0051D2D9`), none for a Unit (`0x00743BE0`).
//! - [`board_passenger`]: the ArchiveTarget, OnBridge, `+0xC4` and the
//!   gattling spin cleared, a captive freed, then Limbo, which BREAKs every
//!   contact and conceals him; the arm's HELLO or house power
//!   invalidation, AddPassenger, an open-topped rider's Guard orders and
//!   logic entry, a gunner's weapon, and Stop_Moving after the building arm.
//! - Action 14 orders one member at a time; its next comes once his Limbo
//!   broke the transport's contact.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ (ObjectType), sim/game_entity, sim/entity_store.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

mod departure;
pub(crate) mod garrison_eject;
pub(crate) use departure::DepartureFailure;
pub use departure::{
    DepartureRoute, depart_cargo_head, exit_cell_position, reveal_unloaded_passenger,
};

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::world::{ConcealOutcome, FrameEffects, SimSoundEvent, Simulation, UninitContext};

/// The cargo hold of a type that holds passengers: CargoClass `Passengers`
/// (`TechnoClass+0x114`: the count, then the list head at `+0x118`, chained
/// through each passenger's `ObjectClass+0x30`), held in a
/// [`PassengerRole`].
///
/// Tracks which entities are currently inside this transport/garrison.
/// Passengers are stored as a Vec of stable_ids in native CargoClass list
/// order: index 0 is the list HEAD. `CargoClass::AddPassenger @ 0x004733A0`
/// PREPENDS (`passenger->next = head; head = passenger`, `0x004733FA`..
/// `0x00473400`) and `CargoClass::RemoveFirstPassenger @ 0x00473430` pops the
/// head, so every native consumer — transport unload, paradrop, sell eject —
/// releases the LAST boarded passenger first.
///
/// A garrisoned building's occupants natively live in a separate `Occupants`
/// vector instead (Items `+0x688`, Count `+0x694`), which
/// [`add_garrison_occupant`] APPENDS to ([`Self::add_occupant`]): index 0 is
/// the earliest admission, which fires first (`+0x69C`) and names the owner
/// a civilian building takes (`0x00458313`), while a sale removes them last
/// to first (`0x004580A9..0x0045819E`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PassengerCargo {
    /// Stable IDs of entities currently inside: a transport's head first
    /// (the most recently boarded at index 0, LIFO unload), a garrison's in
    /// admission order.
    passengers: Vec<u64>,
    /// `Size=` captured for each entry in `passengers`, at the same index.
    ///
    /// CargoClass can remove an individual expiring object without receiving
    /// its size as a separate argument. Keeping the admission-time size with
    /// the entry gives the Rust cargo list the same exact bookkeeping ability.
    #[serde(default)]
    passenger_sizes: Vec<f64>,
    /// Maximum passenger count (from Passengers= or MaxNumberOccupants= in rules.ini).
    capacity: u32,
    /// Maximum Size= of individual passenger allowed (from SizeLimit= in rules.ini).
    size_limit: f64,
    /// Native CargoTotalSize's signed low-32-bit FTOL result, recomputed in
    /// passenger-list order after every mutation.
    total_size: i32,
    /// Round-robin index for garrison fire — which occupant fires next.
    /// Matches gamemd BuildingClass+0x69C (CurrentFireIdx). Init 0, advanced
    /// by garrison combat after each shot: `(idx + 1) % occupant_count`.
    garrison_fire_index: u8,
    /// A transport's own passenger relation is independent of its cargo.
    /// Unit73A6EC..73A77C attaches it without clearing CargoClass+114;
    /// Steam gamemd.exe SHA3e81a617, original Unit739EC0 instructions.
    /// The passenger owner alone mutates this serialized relation.
    #[serde(default)]
    carrier_relation: CarrierRelation,
}

#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
enum CarrierRelation {
    #[default]
    Outside,
    Inside {
        transport_id: u64,
        open_topped: bool,
    },
}

impl PassengerCargo {
    pub fn new(capacity: u32, size_limit: f64) -> Self {
        Self {
            passengers: Vec::new(),
            passenger_sizes: Vec::new(),
            capacity,
            size_limit,
            total_size: 0,
            garrison_fire_index: 0,
            carrier_relation: CarrierRelation::Outside,
        }
    }

    /// Number of passengers currently inside.
    pub fn count(&self) -> u32 {
        self.passengers.len() as u32
    }

    /// The passengers' stable ids, a transport's list head first and a
    /// garrison's earliest admission first.
    pub fn passengers(&self) -> &[u64] {
        &self.passengers
    }

    /// The `Size=` each passenger had when it boarded, by index.
    pub(crate) fn passenger_sizes(&self) -> &[f64] {
        &self.passenger_sizes
    }

    /// `Passengers=`, or a garrison's `MaxNumberOccupants=`.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// The transport's native double `SizeLimit=`.
    #[cfg(test)]
    pub fn size_limit(&self) -> f64 {
        self.size_limit
    }

    /// The passengers' total `Size=`.
    pub fn total_size(&self) -> i32 {
        self.total_size
    }

    /// CargoTotalSize473460: add each current passenger Size as binary64 to
    /// the signed accumulator, then apply native FTOL's signed-64 result
    /// truncated to its low 32 bits after every passenger. The passenger
    /// identity/type and session rules are immutable, so admission-time
    /// doubles remain the current Size values for this live cargo list.
    fn cargo_total_size(passenger_sizes: &[f64]) -> i32 {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};

        let mut total = 0_i32;
        for &size in passenger_sizes {
            let sum = X87::add(
                X87::load_i32(total),
                X87::load_f64(NativeF64Bits::from_bits(size.to_bits())),
            );
            total = X87::ftol_i32_low_masked(sum);
        }
        total
    }

    /// Recalculate through the same order-sensitive numeric owner used by
    /// the native CargoTotalSize consumer; subtraction cannot reproduce its
    /// per-passenger truncation for fractional or negative Sizes.
    fn recompute_total_size(&mut self) {
        self.total_size = Self::cargo_total_size(&self.passenger_sizes);
    }

    /// Re-evaluate CargoTotalSize for snapshot consistency checks.
    pub(crate) fn recomputed_total_size(&self) -> i32 {
        Self::cargo_total_size(&self.passenger_sizes)
    }

    /// Apply the native binary64 `Size <= SizeLimit` admission comparison.
    /// Zero is a real limit: it refuses a positive Size.
    pub(crate) fn size_within_limit(size: f64, limit: f64) -> bool {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};

        matches!(
            X87::compare(
                X87::load_f64(NativeF64Bits::from_bits(size.to_bits())),
                X87::load_f64(NativeF64Bits::from_bits(limit.to_bits())),
            ),
            MaskedX87Ordering::Less | MaskedX87Ordering::Equal
        )
    }

    /// Apply Unit Receive_Radio's `cargo total + passenger Size <= Passengers`
    /// comparison, preserving the signed integer cargo total and binary64
    /// passenger value until after the x87 comparison.
    pub(crate) fn size_fits_capacity(cargo_total: i32, size: f64, capacity: i32) -> bool {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};

        let occupied = X87::add(
            X87::load_i32(cargo_total),
            X87::load_f64(NativeF64Bits::from_bits(size.to_bits())),
        );
        matches!(
            X87::compare(occupied, X87::load_i32(capacity)),
            MaskedX87Ordering::Less | MaskedX87Ordering::Equal
        )
    }

    /// `BuildingClass+0x69C`, the garrison occupant that fires next.
    pub fn garrison_fire_index(&self) -> u8 {
        self.garrison_fire_index
    }

    /// `BulletClass::Fire`'s advance of an occupied building's firing
    /// occupant: `+0x69C = (+0x69C + 1) % occupants`.
    pub(crate) fn advance_garrison_fire_index(&mut self) {
        let count = self.count() as u8;
        if count > 0 {
            self.garrison_fire_index = (self.garrison_fire_index + 1) % count;
        }
    }

    /// Install a fixture's firing occupant.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_garrison_fire_index_for_test(&mut self, index: u8) {
        self.garrison_fire_index = index;
    }

    /// Install a fixture's passenger list, sizes untouched.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_passengers_for_test(&mut self, passengers: Vec<u64>) {
        self.passengers = passengers;
    }

    /// Install a fixture's admission-time sizes.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_passenger_sizes_for_test(&mut self, sizes: Vec<f64>) {
        self.passenger_sizes = sizes;
        self.recompute_total_size();
    }

    /// Whether the transport has room for a passenger of the given size.
    pub fn can_accept(&self, passenger_size: f64) -> bool {
        self.count() < self.capacity && Self::size_within_limit(passenger_size, self.size_limit)
    }

    /// Append a garrison occupant: `Occupants.AddItem` in
    /// `AddGarrisonOccupant` (`0x0052293B..0x0052297C`). The vector grows
    /// past `MaxNumberOccupants=`; `CanBeOccupiedBy`'s count test is the
    /// admission.
    pub(crate) fn add_occupant(&mut self, stable_id: u64, passenger_size: f64) {
        self.passengers.push(stable_id);
        self.passenger_sizes.push(passenger_size);
        self.recompute_total_size();
    }

    /// Cargo4733A0 attaches an existing Foot+30 chain ahead of the old head.
    /// Full reinforcement65DF88 supplies a whole reverse-construction chain;
    /// single radio/paradrop admissions use the same storage operation.
    /// AddPassenger tests neither count nor size: radio admission's total-size
    /// gate and paradrop's `*ParaDropNum` (not PDPLANE's `Passengers=` or
    /// `SizeLimit=`) belong to those callers.
    /// Class Limbo belongs to Simulation::add_cargo_passenger_chain before
    /// this splice. Known Foot IDs replace native raw link/AbstractFlags reads.
    pub fn board_forced_chain(&mut self, head_first: &[(u64, f64)]) {
        self.passengers
            .splice(0..0, head_first.iter().map(|&(id, _)| id));
        self.passenger_sizes
            .splice(0..0, head_first.iter().map(|&(_, size)| size));
        self.recompute_total_size();
    }

    /// Remove a specific passenger. Returns true if found and removed.
    pub fn disembark(&mut self, stable_id: u64) -> bool {
        if let Some(pos) = self.passengers.iter().position(|&id| id == stable_id) {
            self.passengers.remove(pos);
            self.passenger_sizes.remove(pos);
            self.recompute_total_size();
            true
        } else {
            false
        }
    }

    /// Remove and return the list HEAD (the most recently boarded passenger)
    /// and its recorded size — `CargoClass::RemoveFirstPassenger @ 0x00473430`.
    pub(crate) fn unload_first(&mut self) -> Option<(u64, f64)> {
        if self.passengers.is_empty() {
            None
        } else {
            let id = self.passengers.remove(0);
            let passenger_size = self.passenger_sizes.remove(0);
            self.recompute_total_size();
            Some((id, passenger_size))
        }
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

impl Simulation {
    /// CargoClass::AddPassenger4733A0: Limbo the supplied head, then attach
    /// its existing member chain without count/size admission. The caller
    /// owns transport/rider policy; this native leaf does not write +11C or
    /// OpenTopped. No member/Team removal or owner transfer is invented here.
    /// Steam gamemd3e81a617..08600; Full65DF88 and ordinary radio admission
    /// share PassengerCargo's prepend/recount owner.
    pub(crate) fn add_cargo_passenger_chain(
        &mut self,
        transport_id: u64,
        head_first: &[(u64, f64)],
        context: UninitContext<'_>,
    ) -> bool {
        let Some(&(head, _)) = head_first.first() else {
            return true;
        };
        let _ = self.techno_limbo_with_context(head, context);
        let Some(transport) = self.substrate.entities.get_mut(transport_id) else {
            return false;
        };
        // Every native Techno has CargoClass even when Type.Passengers is0.
        // Make its empty hold at the first raw Cargo attach; ordinary
        // capacity admission remains a caller policy.
        let cargo = transport.passenger_role.ensure_hold();
        // The existing size cache belongs to Cargo as well: ordinary callers
        // supply their admitted type size; retries retain the popped size.
        // Native quantity is the chain length, not its configured capacity.
        cargo.board_forced_chain(head_first);
        true
    }
}

/// An entity's passenger relation and, for a type that holds passengers, its
/// cargo hold. Natively these are separate Techno fields: the CargoClass
/// `Passengers` (`TechnoClass+0x114`, [`PassengerCargo`]), the Transporter
/// (`+0x11C`) and InOpenToppedTransport (`+0x82`). VERA creates a hold for
/// configured carriers or when native raw Cargo admission first uses it,
/// and keeps a transport's own relation to the
/// hull it rides in inside that hold. This module owns all of it: other
/// modules read it through these methods and change it through its boarding,
/// unload and teardown methods.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PassengerRole {
    role: Role,
}

/// The stored shape of a [`PassengerRole`]. A transport keeps its own cargo
/// while riding in another hull; other entities keep the Inside
/// representation.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
enum Role {
    /// Entity has no passenger/transport role. Most entities are this.
    #[default]
    None,
    /// Entity is a transport or garrisonable building that can hold passengers.
    Transport { cargo: PassengerCargo },
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
    /// The role of a type that holds passengers (`Passengers=`, or a
    /// garrison's `MaxNumberOccupants=`), with its empty hold.
    pub fn transport(cargo: PassengerCargo) -> Self {
        Self {
            role: Role::Transport { cargo },
        }
    }

    /// Transporter+11C and InOpenToppedTransport+82, independent of an entity's
    /// own CargoClass. Callers do not inspect the storage representation.
    pub fn inside_transport(&self) -> Option<(u64, bool)> {
        match &self.role {
            Role::Inside {
                transport_id,
                open_topped,
            } => Some((*transport_id, *open_topped)),
            Role::Transport { cargo } => match cargo.carrier_relation {
                CarrierRelation::Inside {
                    transport_id,
                    open_topped,
                } => Some((transport_id, open_topped)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Successful attachment or departure rollback. Cargo and its gunner
    /// selection remain owned by the vehicle throughout this transition.
    pub(crate) fn set_inside_transport(&mut self, transport_id: u64, open_topped: bool) {
        match &mut self.role {
            Role::Transport { cargo } => {
                cargo.carrier_relation = CarrierRelation::Inside {
                    transport_id,
                    open_topped,
                };
            }
            role => {
                *role = Role::Inside {
                    transport_id,
                    open_topped,
                }
            }
        }
    }

    /// Clear only this entity's carrier link. Its own cargo remains available
    /// for unloading, retry restoration and recursive UnInit.
    pub(crate) fn clear_carrier_relation(&mut self) {
        match &mut self.role {
            Role::Transport { cargo } => cargo.carrier_relation = CarrierRelation::Outside,
            role => *role = Role::None,
        }
    }

    /// Returns the cargo hold if this entity is a transport.
    pub fn cargo(&self) -> Option<&PassengerCargo> {
        match &self.role {
            Role::Transport { cargo } => Some(cargo),
            _ => Option::None,
        }
    }

    /// The passenger count (`+0x114`): the cargo's, none for a non-transport.
    pub fn passenger_count(&self) -> u32 {
        self.cargo().map_or(0, PassengerCargo::count)
    }

    /// The hold of a type without `Passengers=` that is loaded anyway, made
    /// on first use: every native Techno has its CargoClass (`+0x114`), and
    /// `HouseClass::SendParadropPlanes` fills the paradrop plane's through
    /// `CargoClass::AddPassenger` (`0x0065E7F5`). The entity's own carrier
    /// relation (`+0x11C`, `+0x82`) is a separate Techno field that
    /// `CargoClass::AddPassenger` (`0x004733A0`) does not write, so the hold
    /// keeps it.
    pub(crate) fn ensure_hold(&mut self) -> &mut PassengerCargo {
        if !self.is_transport() {
            let carrier_relation = match self.role {
                Role::Inside {
                    transport_id,
                    open_topped,
                } => CarrierRelation::Inside {
                    transport_id,
                    open_topped,
                },
                Role::None | Role::Transport { .. } => CarrierRelation::Outside,
            };
            let mut cargo = PassengerCargo::new(0, 0.0);
            cargo.carrier_relation = carrier_relation;
            self.role = Role::Transport { cargo };
        }
        match &mut self.role {
            Role::Transport { cargo } => cargo,
            _ => unreachable!("the hold was made above"),
        }
    }

    /// The cargo hold, for its own boarding, unload and teardown methods.
    pub fn cargo_mut(&mut self) -> Option<&mut PassengerCargo> {
        match &mut self.role {
            Role::Transport { cargo } => Some(cargo),
            _ => Option::None,
        }
    }

    /// Returns the transport ID if this entity is inside one.
    pub fn inside_transport_id(&self) -> Option<u64> {
        self.inside_transport().map(|(id, _)| id)
    }

    /// `TechnoClass+0x82` with its `+0x11C` Transporter: the `OpenTopped=`
    /// transport this entity rides in, if any. Read by weapon selection, the
    /// fire coordinate, InRange, GetFireError, FireAt's damage and muzzle
    /// anim, kill credit, navigation and the Temporal warp-distance check.
    pub fn open_transport_id(&self) -> Option<u64> {
        self.inside_transport()
            .and_then(|(id, open)| open.then_some(id))
    }

    /// `TechnoClass+0x82` InOpenToppedTransport.
    pub fn in_open_transport(&self) -> bool {
        self.open_transport_id().is_some()
    }

    /// True if entity is inside a transport (hidden from map).
    pub fn is_inside_transport(&self) -> bool {
        self.inside_transport().is_some()
    }

    /// True if entity is a transport/garrison with a cargo hold.
    pub fn is_transport(&self) -> bool {
        matches!(self.role, Role::Transport { .. })
    }

    /// No hold and no carrier relation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_none(&self) -> bool {
        matches!(self.role, Role::None)
    }

    /// A fixture's passenger inside `transport_id`, replacing any hold.
    #[cfg(any(test, feature = "test-support"))]
    pub fn inside_for_test(transport_id: u64, open_topped: bool) -> Self {
        Self {
            role: Role::Inside {
                transport_id,
                open_topped,
            },
        }
    }
}

/// The shared order/UI admission adapter, the click's ENTER
/// (`InfantryClass::What_Action_OnObject` `0x0051ED10..0x0051EE3B`): the
/// passenger's house allied with the carrier's, then the carrier's CAN_LOAD
/// ([`crate::sim::radio::receive::can_load_query`], each class receiver's
/// answer), whose ROGER is ENTER. Building garrison admission keeps its
/// existing CanBeOccupiedBy owner.
pub fn can_enter_transport(
    sim: &Simulation,
    rules: &RuleSet,
    passenger_id: u64,
    transport_id: u64,
) -> bool {
    let Some((passenger, transport, passenger_obj, transport_obj, cargo)) =
        garrison_parts(sim, rules, passenger_id, transport_id)
    else {
        return false;
    };
    // These are order/UI lifecycle admission, not extra receiver0F gates.
    if !passenger.is_alive()
        || passenger.death.is_dying()
        || passenger.passenger_role.is_inside_transport()
        || !transport.is_alive()
        || transport.death.is_dying()
    {
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
            sim,
        );
    }
    // Infantry51ED10 asks the actor's alliance before issuing the radio; the
    // receiver then asks its own (Building 0x0043C2D0, Unit 0x00737430).
    crate::sim::combat::combat_weapon::is_ally_by_object(
        Some(&sim.house_alliances),
        &sim.interner,
        passenger.owner(),
        transport.owner(),
    ) && crate::sim::radio::receive::can_load_query(sim, transport_id, passenger_id, rules)
        == crate::sim::radio::RadioResponse::Roger
}

/// `BuildingClass::CanBeOccupiedBy @ 0x00457CE0` for an infantryman: the
/// building's type is `CanBeOccupied=`, it is not under construction or
/// being sold, it stands in the playfield and is not warped out; the
/// infantryman is an `Occupier=` of the building's house (or the building's
/// house is `MultiplayPassive=`), the building is neither full nor at red
/// health, and the infantryman is not mind-controlled.
///
/// Map5785F0 tests the building's actual GetCoords through the shared
/// height-aware playfield owner, rather than the navigation rectangle.
/// RESIDUALS: the `Assaulter=` arm (`0x00457DAD..0x00457DD3`: an
/// assaulter may enter an occupied building of a house it is not allied
/// with) is not represented. No retail infantry is an assaulter.
pub fn can_dock_occupier_garrison(
    passenger: &GameEntity,
    building: &GameEntity,
    passenger_obj: &ObjectType,
    building_obj: &ObjectType,
    cargo: &PassengerCargo,
    rules: &RuleSet,
    sim: &Simulation,
) -> bool {
    if !building_obj.can_be_occupied {
        return false;
    }
    if building.building_up() || building.building_down() {
        return false;
    }
    let coord = crate::sim::movement::ground_pose::object_get_coords(
        building,
        sim.resolved_terrain.as_ref(),
    );
    if !crate::sim::cell_rect::cell_is_in_playfield_leptons(
        (coord.x, coord.y, coord.z),
        sim.playfield_bounds,
        sim.resolved_terrain.as_ref(),
    ) {
        return false;
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
    if !same_owner && !owner_is_multiplay_passive(building.owner(), &sim.houses) {
        return false;
    }
    if cargo.count() == building_obj.max_number_occupants {
        return false;
    }
    if is_at_or_below_red_hp(
        building.health.current(),
        building_obj.strength,
        rules.general.condition_red,
    ) {
        return false;
    }
    // `0x00457D98`: `TechnoClass::IsMindControlled`.
    !passenger.mind_control.is_mind_controlled()
}

/// [`can_dock_occupier_garrison`] by id, as `CanBeOccupiedBy`'s callers that
/// pass `this` when it is infantry and NULL otherwise (which it refuses):
/// `FootClass::EnterBattleBunker` and `InfantryClass::PerCellProcess`.
pub(crate) fn building_can_be_occupied_by(
    sim: &Simulation,
    rules: &RuleSet,
    building_id: u64,
    infantry_id: u64,
) -> bool {
    let Some((infantry, building, infantry_obj, building_obj, cargo)) =
        garrison_parts(sim, rules, infantry_id, building_id)
    else {
        return false;
    };
    if infantry.category() != EntityCategory::Infantry {
        return false;
    }
    can_dock_occupier_garrison(
        infantry,
        building,
        infantry_obj,
        building_obj,
        cargo,
        rules,
        sim,
    )
}

/// Read the target owner's `MultiplayPassive` house-type fact.
///
/// One source of truth: the flag is resolved from the country rules once, at
/// house creation, and stored on the house — see
/// `house_state::resolve_multiplay_passive`. An owner with no `HouseState` is
/// not passive.
fn owner_is_multiplay_passive(owner: InternedId, houses: &crate::sim::houses::Houses) -> bool {
    houses
        .get(&owner)
        .is_some_and(|house| house.multiplay_passive)
}

/// The passenger, building, their types and the building's cargo, by id:
/// what the garrison admission tests read.
fn garrison_parts<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    passenger_id: u64,
    building_id: u64,
) -> Option<(
    &'a GameEntity,
    &'a GameEntity,
    &'a ObjectType,
    &'a ObjectType,
    &'a PassengerCargo,
)> {
    let passenger = sim.substrate.entities.get(passenger_id)?;
    let building = sim.substrate.entities.get(building_id)?;
    Some((
        passenger,
        building,
        sim.object_type(passenger.type_ref(), rules)?,
        sim.object_type(building.type_ref(), rules)?,
        building.passenger_role.cargo()?,
    ))
}

/// The class `PerCellProcess` arm that takes a passenger aboard. Each runs
/// [`board_passenger`], which keeps their native differences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoardingArm {
    /// `InfantryClass::PerCellProcess`'s building arm, after the building
    /// answered DOCK_NOW (`0x0051A2AD..0x0051A39D`).
    InfantryBuilding,
    /// Its transport arm, after the transport answered CAN_LOAD
    /// (`0x0051A3FC..0x0051A485`).
    InfantryTransport,
    /// `UnitClass::PerCellProcess`'s `UnitAbsorb=` arm, after CAN_LOAD
    /// (`0x0073A28A..0x0073A31C`).
    UnitAbsorber,
    /// Its transport arm, after CAN_LOAD (`0x0073A6E8..0x0073A793`).
    UnitTransport,
}

impl BoardingArm {
    /// The building arms add through CargoClass alone; the transport arms
    /// through `TechnoClass::AddPassenger` (vt+0x394).
    fn building(self) -> bool {
        matches!(self, Self::InfantryBuilding | Self::UnitAbsorber)
    }
}

/// The admission an arm runs once its carrier said yes; its gates belong to
/// the class arrival (`sim::movement::per_cell`):
/// - every arm but the Infantry building one clears the ArchiveTarget
///   (`Set_ArchiveTarget(0)`) and OnBridge; all zero the mission tick count
///   and the gattling spin (`+0xC4`, `SetValue(0)`, `SetStage(0)`) and release
///   a captive (`CaptureManager::Free`);
/// - an `InfantryAbsorb=` building plays `EnterBioReactorSound=` at the
///   infantryman and sets `+0x439` before Limbo (`0x0051A2DF..0x0051A30F`),
///   whose house-total tail follows it (`0x0051A327..0x0051A348`);
/// - Limbo (vt+0xD4) BREAKs every contact;
/// - the Infantry building arm then HELLOs a building that does not absorb
///   infantry, or invalidates an `InfantryAbsorb=` one's house power when
///   its `ExtraPower=` is positive (`0x0051A34F..0x0051A37C`); the Unit
///   absorber arm invalidates it after the add (`0x0073A2F4..0x0073A30E`);
/// - the passenger joins the cargo, a transport's through AddPassenger,
///   which plays `EnterTransportSound=` and takes a `Gunner=` transport's
///   gunner; an `OpenTopped=` transport makes it a rider
///   (`SetInOpenTransport @ 0x00710470`);
/// - the Infantry building arm stops his movement (`0x004DF0D0`), and an
///   open-topped Unit's own riders let go of their targets
///   (`0x0073A76E..0x0073A77C`).
///
/// Every arm ends with the passenger's vt+0x11C, which repeats the
/// discovery clear its Limbo already made, so nothing follows it here.
/// Answers whether the passenger went in.
pub(crate) fn board_passenger(
    sim: &mut Simulation,
    rules: &RuleSet,
    pax_id: u64,
    carrier_id: u64,
    arm: BoardingArm,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(pax_size) = sim.substrate.entities.get(pax_id).map(|pax| {
        sim.object_type(pax.type_ref(), rules)
            .map_or(1.0, |o| o.size)
    }) else {
        return false;
    };
    let Some(carrier) = sim
        .substrate
        .entities
        .get(carrier_id)
        .and_then(|carrier| Some((carrier.owner(), sim.object_type(carrier.type_ref(), rules)?)))
        .map(|(owner, object)| {
            (
                owner,
                object.open_topped,
                object.gunner,
                object.infantry_absorb,
                object.extra_power > 0,
            )
        })
    else {
        return false;
    };
    let (carrier_owner, open_topped, gunner, infantry_absorb, extra_power) = carrier;
    // Infantry51A38A adds to every building that admitted DOCK_NOW, including
    // Hospital=yes with Passengers=0. The shared raw Cargo owner creates the
    // empty hold lazily; configured capacity remains an admission policy.
    let absorber_entry = arm == BoardingArm::InfantryBuilding && infantry_absorb;
    // `0x0051A2EF..0x0051A30A`: Foot's GetCoords (`+0x48`) before Limbo,
    // played with `VocClass::PlayAt @ 0x007509E0` once the add commits.
    let bio_reactor_entry_sound = absorber_entry
        .then(|| rules.general.enter_bio_reactor_sound.clone())
        .flatten()
        .and_then(|sound_id| {
            sim.substrate.entities.get(pax_id).map(|passenger| {
                (
                    sound_id,
                    crate::sim::movement::ground_pose::object_get_coords(
                        passenger,
                        sim.resolved_terrain.as_ref(),
                    ),
                )
            })
        });
    if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
        // `0x0051A3FF`, `0x0073A28E`, `0x0073A6EC`.
        if arm != BoardingArm::InfantryBuilding {
            pax.set_archive_target(None);
        }
        // Infantry `0x0051A2B0..0x0051A2BE` and `0x0051A40E..0x0051A41C`,
        // Unit `0x0073A29E..0x0073A2B1` and `0x0073A6FC..0x0073A70F`.
        pax.mission.clear_ai_counter();
        pax.gattling.set_value(0);
        pax.gattling.set_stage(0);
    }
    // `0x0051A2DA`, `0x0051A438`, `0x0073A2CD`, `0x0073A72B`.
    if let Some(controller) = sim
        .substrate
        .entities
        .get(pax_id)
        .and_then(|pax| pax.mind_control.controller())
    {
        sim.free_unit(controller, pax_id, rules, registry, effects);
    }
    // Add_Tracking skips its house-total arm while `+0x439` is set and keeps
    // the per-type count; the post-Limbo tail removes the old `+0x2F4`
    // contribution and clears `+0x438`.
    if absorber_entry {
        sim.set_infantry_absorber_entry(pax_id, true);
    }
    let context = UninitContext::new(Some(rules), registry).with_effects(effects);
    if sim.techno_limbo_with_context(pax_id, context) != ConcealOutcome::Concealed {
        if absorber_entry {
            sim.set_infantry_absorber_entry(pax_id, false);
        }
        return false;
    }
    if absorber_entry {
        sim.account_infantry_absorber_entry(pax_id);
    }
    match arm {
        BoardingArm::InfantryBuilding if !infantry_absorb => {
            crate::sim::radio::transmit(
                sim,
                pax_id,
                carrier_id,
                crate::sim::radio::RadioMessage::Hello,
                crate::sim::radio::RadioPayload::default(),
                Some(rules),
                effects,
            );
        }
        BoardingArm::InfantryBuilding if extra_power => {
            sim.invalidate_house_power(carrier_owner, false);
        }
        _ => {}
    }
    // RESIDUAL: native clears OnBridge before Limbo (`0x0051A407`,
    // `0x0073A297`, `0x0073A6F5`), whose Pick_Up unlinks from the list
    // OnBridge names (`0x005687F0` -> RemoveContent `0x0047EA90`), so a
    // passenger boarding from a cell's bridge list stays linked there. VERA
    // clears it after Limbo, which unlinks it. Trigger: boarding with
    // OnBridge set, from a bridge deck or as a trooper Paradrop refused over
    // a bridge. Effect: native leaves the passenger in that cell's bridge
    // list. Frequency: rare. Risk: a later pass over that list meets the
    // passenger.
    if arm != BoardingArm::InfantryBuilding
        && let Some(pax) = sim.substrate.entities.get_mut(pax_id)
    {
        pax.on_bridge.store(false);
    }
    //4733A0 repeats the head's virtual Limbo before its unconditional
    //prepend.
    let boarded = sim.add_cargo_passenger_chain(carrier_id, &[(pax_id, pax_size)], context);
    // The admitted carrier survives Limbo; Cargo creates its hold if needed.
    debug_assert!(boarded, "boarding admission changed during one transaction");
    if !boarded {
        // Recover the passenger rather than strand a concealed object if an
        // invariant is broken. Reveal does not re-run Add_Tracking, so undo
        // the entry's House+0x2F4 removal and +0x439 skip byte through the
        // owner a successful unload uses.
        if absorber_entry {
            sim.restore_absorbed_infantry_tracking(pax_id);
        }
        let _ = sim.reveal_entity_with_context(pax_id, context);
        return false;
    }
    if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
        // VERA's carrier relation stands for Transporter (`+0x11C`, written
        // by the transport arms at `0x0051A463` and `0x0073A768`) and for
        // membership of a building's cargo. Boarding keeps the passenger's
        // TarCom and passive flag: neither arm nor Limbo writes them.
        pax.passenger_role
            .set_inside_transport(carrier_id, open_topped && !arm.building());
        if arm == BoardingArm::InfantryBuilding {
            pax.navigation.stop_moving();
        }
    }
    if arm.building() {
        if arm == BoardingArm::UnitAbsorber && extra_power {
            sim.invalidate_house_power(carrier_owner, false);
        }
    } else {
        // `TechnoClass::AddPassenger @ 0x00710670` (Infantry `0x0051A46E`,
        // Unit `0x0073A740`): the transport type's `EnterTransportSound=` at
        // its Location (`VocClass::PlayAtCoord @ 0x00750E20`, flags 0).
        if let Some(transport) = sim.substrate.entities.get(carrier_id)
            && let Some(sound_id) = sim
                .object_type(transport.type_ref(), rules)
                .and_then(|object| object.enter_transport_sound.clone())
        {
            let coordinate =
                crate::sim::movement::ground_pose::position_world_coord(&transport.position);
            sim.sound_events.push(SimSoundEvent::VocAtCoord {
                sound_id,
                coordinate,
                flags: 0,
            });
            effects.flush_sound(sim, Some(rules));
        }
    }
    if let Some((sound_id, coordinate)) = bio_reactor_entry_sound {
        sim.sound_events.push(SimSoundEvent::voc_at_world_coord(
            sound_id,
            crate::sim::anim_class::AnimWorldCoord {
                x: coordinate.x,
                y: coordinate.y,
                z: coordinate.z,
            },
        ));
    }
    if open_topped && !arm.building() {
        // `SetInOpenTransport @ 0x00710470` (Infantry `0x0051A45E`, before
        // AddPassenger; Unit `0x0073A75D`, after): `+0x82` (the relation's
        // flag above), then `ResetOrdersToGuard` (vt+0x3D0) and the
        // LogicClass add, so the rider keeps an AI turn and guards from
        // inside. Infantry Limbo51DF10 keeps TarCom: 51A441 ->51A45E ->710484
        // reaches the class NULL setter with the former target intact.
        sim.reset_orders_to_guard(pax_id, rules, effects);
        let registered = sim.register_open_topped_passenger(pax_id);
        debug_assert!(
            registered,
            "accepted open-topped passenger must remain active"
        );
        // The rider stands where its walk into the transport ended, and
        // every later transport `SetLocation` copies the transport's
        // coordinate onto it (`FootClass::SetLocation 0x004DB810` ->
        // `0x007104F0`). Boarding itself preserves the arrival subcell.
    }
    if arm == BoardingArm::UnitTransport {
        sim.open_topped_passengers_take_target(pax_id, None, rules, registry, effects);
    }
    if gunner && !arm.building() {
        receive_gunner(sim, rules, carrier_id, pax_id);
    }
    true
}

/// `InfantryClass::AddGarrisonOccupant @ 0x00522910`, the one way into a
/// garrison: `InfantryClass::PerCellProcess`'s occupier arm
/// (`sim::movement::per_cell`), shared by player and computer Capture orders. An
/// `Occupier=` infantryman leaves the map (Limbo, vt+0xD4) and is appended
/// to the building's occupants ([`PassengerCargo::add_occupant`]); the
/// building's spatial threat is refreshed (`0x0052297F..0x0052298D`). The
/// first occupant marks the building for redraw (Mark(2), `0x0052299B`) and
/// plays `EVA_StructureGarrisoned` and `BuildingGarrisonedSound=` at the
/// infantryman (`0x005229A8..0x005229E0`, the app plays them for the
/// player's own house). A human house's occupant drops its occupy seeks
/// (`+0x690`, `+0x691`; `0x005229E5..0x005229FA`); a computer's keeps them,
/// so once ejected its Guard head seeks a bunker again. Answers whether the
/// infantryman went in.
///
/// RESIDUALS: the `Assaulter=` arm (`0x00522A09..0x00522A45`: the
/// building's occupants are killed, and the assaulter drops its destination
/// and scatters) is not represented; no retail infantry is an assaulter.
/// VERA's passenger also drops its movement and order state.
pub fn add_garrison_occupant(
    sim: &mut Simulation,
    rules: &RuleSet,
    pax_id: u64,
    building_id: u64,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(pax) = sim.substrate.entities.get(pax_id) else {
        return false;
    };
    let owner = pax.owner();
    let (rx, ry) = (pax.position.rx(), pax.position.ry());
    let Some(pax_size) = sim
        .object_type(pax.type_ref(), rules)
        .filter(|object| object.occupier)
        .map(|object| object.size)
    else {
        return false;
    };
    if !sim
        .substrate
        .entities
        .get(building_id)
        .is_some_and(|building| building.passenger_role.cargo().is_some())
    {
        return false;
    }
    let context = UninitContext::new(Some(rules), registry).with_effects(effects);
    if sim.techno_limbo_with_context(pax_id, context) != ConcealOutcome::Concealed {
        return false;
    }
    let Some(count) = sim
        .substrate
        .entities
        .get_mut(building_id)
        .and_then(|building| building.passenger_role.cargo_mut())
        .map(|cargo| {
            cargo.add_occupant(pax_id, pax_size);
            cargo.count()
        })
    else {
        return false;
    };
    if let Some(pax) = sim.substrate.entities.get_mut(pax_id) {
        // Admission and Limbo keep the occupant's TarCom and passive flag.
        pax.passenger_role.set_inside_transport(building_id, false);
    }
    sim.refresh_spatial_threat(building_id, rules, None);
    if count == 1 {
        let _ = sim.mark_entity_refresh(building_id);
        sim.sound_events
            .push(SimSoundEvent::StructureGarrisoned { owner });
        sim.sound_events
            .push(SimSoundEvent::BuildingGarrisonedSfx { owner, rx, ry });
    }
    if sim.houses.get(&owner).is_some_and(|house| house.is_human)
        && let Some(pax) = sim.substrate.entities.get_mut(pax_id)
    {
        pax.building_entry.clear_occupy_seeks();
    }
    true
}

/// Gunner caller dispatch to UnitClass +4D4 (746420). Temporal ownership
/// moves first; its mode7 step precedes the final passenger IFVMode selector.
fn receive_gunner(sim: &mut Simulation, rules: &RuleSet, transport_id: u64, pax_id: u64) {
    let Some(transport) = sim.substrate.entities.get(transport_id) else {
        return;
    };
    let Some(object) = sim.interner.object_type(transport.type_ref(), rules) else {
        return;
    };
    // Aircraft/Foot bind this virtual slot to the 4DE750 stub.
    if transport.category() != crate::map::entities::EntityCategory::Unit || !object.gunner {
        return;
    }
    let Some(passenger) = sim.substrate.entities.get(pax_id) else {
        return;
    };
    // 74643A..746460 samples the donor before moving its manager. Native
    // tests nonzero, including a paused signed duration, rather than >0.
    let donor_rearming = passenger
        .rearm_timer
        .remaining(sim.session.binary_frame() as i32)
        != 0;
    if sim.temporal_receive_gunner(transport_id, pax_id) {
        if let Some(transport) = sim.substrate.entities.get_mut(transport_id) {
            transport.set_gunner_weapon(7, object);
        }
        if donor_rearming {
            rearm_transferred_gunner(sim, rules, transport_id, 7);
        }
    }
    let mode = sim
        .substrate
        .entities
        .get(pax_id)
        .and_then(|passenger| sim.interner.object_type(passenger.type_ref(), rules))
        .map_or(0, |passenger| passenger.ifv_mode);
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
    let Some(object) = sim.interner.object_type(transport.type_ref(), rules) else {
        return;
    };
    // Aircraft/Foot bind this virtual slot to the 4DE760 stub.
    if transport.category() != crate::map::entities::EntityCategory::Unit || !object.gunner {
        return;
    }
    // 746502..746528 samples the IFV before transfer/LetGo. The receiving
    // passenger's current burst/rank/House are read after LetGo below.
    let donor_rearming = transport
        .rearm_timer
        .remaining(sim.session.binary_frame() as i32)
        != 0;
    if let Some(pax_id) = pax_id
        && sim.temporal_remove_gunner(transport_id, pax_id)
        && donor_rearming
    {
        rearm_transferred_gunner(sim, rules, pax_id, 0);
    }
    if let Some(transport) = sim.substrate.entities.get_mut(transport_id) {
        transport.set_gunner_weapon(0, object);
    }
}

/// Receiving half of Unit746486..7464B8 /746561..74658F. These callbacks
/// INC the receiver's burst without FireAt's remainder, ask its actual weapon
/// slot through the shared GetROF context, then start its own timer afresh.
/// Neither copies the donor's remainder nor writes FireAt's charge delay+2F8.
/// Original body/RNG controls: `tools/spatial_oracle/ifv_turret_switching.py
/// --temporal-gunner`, consumed as `ifv_temporal_gunner.json`.
fn rearm_transferred_gunner(sim: &mut Simulation, rules: &RuleSet, receiver: u64, slot: i32) {
    let Some(actor) = sim.substrate.entities.get_mut(receiver) else {
        return;
    };
    let burst_index = actor.weapon_burst.advance();
    let actor = sim.substrate.entities.get(receiver).unwrap();
    let Some(object) = sim.interner.object_type(actor.type_ref(), rules) else {
        return;
    };
    let weapon = crate::sim::combat::combat_weapon::get_weapon(
        actor,
        object,
        slot,
        &sim.substrate.entities,
        rules,
        &sim.interner,
    );
    // GetROF reads the firer's context, never its target. Keep the existing
    // snapshot adapter used by FireAt/C4; an untargeted actor uses its cell.
    let target = actor
        .attack_target
        .target()
        .unwrap_or(crate::sim::combat::TargetKind::Cell(
            actor.position.rx(),
            actor.position.ry(),
        ));
    let snapshot = crate::sim::combat::build_attacker_snapshot(actor, target, None);
    let live = crate::sim::combat::rof::LiveParticles {
        spark: actor.spark_particle_system.system().is_some(),
        // Retained Fire/Railgun links remain unported, as in the C4 caller.
        // None of the selected stock FV/CLEG slots has these flags.
        ..Default::default()
    };
    let rof = crate::sim::combat::world_fire::snapshot_get_rof(
        sim,
        rules,
        &snapshot,
        object,
        weapon,
        live,
        burst_index,
    );
    let frame = sim.session.binary_frame() as i32;
    if let Some(actor) = sim.substrate.entities.get_mut(receiver) {
        actor.rearm_timer.start(frame, rof);
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
    if !interner
        .object_type(transport.type_ref(), rules)
        .is_some_and(|object| object.open_topped)
    {
        return;
    }
    let (riders, position) = (cargo.passengers.clone(), transport.position);
    for rider in riders {
        if let Some(entity) = entities.get_mut(rider) {
            entity.position.copy_from(position);
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
    /// its own (`0x0073A77C`, NULL). Techno Limbo's first-live OpenTopped
    /// loop (`0x006F6BA0`) shares the same virtual target setter dispatch.
    pub(crate) fn open_topped_passengers_take_target(
        &mut self,
        transport_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
            let _ = self.assign_target_represented(
                passenger,
                target,
                Some(rules),
                overlay_registry,
                frame_effects,
            );
        }
    }
}

// Object5F5CD0, reached from CanDock457D8F and ejection45821A, tests
// ratio with AH41 then separately requires signed actual HP>0.
fn is_at_or_below_red_hp(current: i32, strength: i32, condition_red: f64) -> bool {
    use crate::util::native_x87::MaskedX87Ordering::{Equal, Less, Unordered};
    current > 0
        && matches!(
            crate::sim::combat::Health::new(current).compare_ratio(strength, condition_red),
            Less | Equal | Unordered
        )
}

/// Building AI4401A3 and CheckAutoSellOrCivilian458200. Only map garrisons
/// (CanBeOccupied and TechLevel=-1) take this path. The existing house-array
/// owner resolves SideClass::FindIndex("Civilian"); no house name is special.
/// Native decision/callback evidence: tools/garrison_oracle/allegiance.py.
pub(crate) fn reconcile_civilian_garrison_owner_for_building(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    building_id: u64,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return false;
    };
    let Some(object) = sim.object_type(building.type_ref(), rules) else {
        return false;
    };
    if building.category() != EntityCategory::Structure
        || !object.can_be_occupied
        || sim.object_tech_level(object) != -1
    {
        return false;
    }
    if is_at_or_below_red_hp(
        building.health.current(),
        object.strength,
        rules.general.condition_red,
    ) {
        garrison_eject::sell_building_occupants(
            sim,
            rules,
            registry,
            building_id,
            garrison_eject::GarrisonEjectMode::Release,
            effects,
        );
    }
    //458265/4582F2 re-read the live vector after ejection.
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return false;
    };
    // Zero-capacity types have no allocated Rust cargo. Native458272 tests
    // the empty occupant vector without requiring MaxNumberOccupants > 0.
    let cargo = building.passenger_role.cargo();
    let current_owner = building.owner();
    let cargo_empty = cargo.is_none_or(PassengerCargo::is_empty);
    //458313 reads Occupants[0], the earliest admission.
    let first_passenger = cargo.and_then(|cargo| cargo.passengers.first().copied());

    let Some(civilian_owner) = sim.civilian_side_house(rules) else {
        // A malformed roster can leave the native empty arm passing NULL to
        //ChangeOwner. Preserve the real owner rather than fabricate a house.
        return false;
    };
    if !cargo_empty && current_owner == civilian_owner {
        let Some(new_owner) = first_passenger
            .and_then(|passenger_id| sim.substrate.entities.get(passenger_id))
            .map(|passenger| passenger.owner())
        else {
            return false;
        };
        sim.refresh_garrison_anims(building_id, rules);
        sim.change_owner_with_rules(building_id, new_owner, rules, registry, effects);
        return new_owner != current_owner;
    }

    if cargo_empty && current_owner != civilian_owner {
        if sim.house_is_human_player(current_owner) {
            //4582A9: centred Voc750920, pan0x2000/volume1/no handle. The
            //configured sound plays even if the following radar event fails.
            if let Some(sound_id) = rules.general.building_abandoned_sound.clone() {
                sim.sound_events
                    .push(SimSoundEvent::VocCentered { sound_id });
            }
            let building = sim.substrate.entities.get(building_id).unwrap();
            sim.sound_events.push(SimSoundEvent::StructureAbandoned {
                owner: current_owner,
                radar: crate::sim::radar::RadarEventRequest::new(
                    crate::sim::radar::RadarEventType::StructureAbandoned,
                    building.position.rx(),
                    building.position.ry(),
                ),
            });
        }
        sim.refresh_garrison_anims(building_id, rules);
        sim.change_owner_with_rules(building_id, civilian_owner, rules, registry, effects);
        return true;
    }

    false
}

/// Test driver for one supplied cell arrival through the class PerCell
/// owner, for every infantryman on Enter or Capture, without running the
/// separate Building AI ownership-reconciliation phase.
#[cfg(any(test, feature = "test-support"))]
fn tick_entry_arrival(sim: &mut Simulation, rules: &RuleSet) {
    use crate::sim::mission::MissionType;
    let arriving: Vec<u64> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .filter(|&id| {
            sim.substrate.entities.get(id).is_some_and(|entity| {
                matches!(
                    entity.mission.effective().known(),
                    Some(MissionType::Enter | MissionType::Capture)
                )
            })
        })
        .collect();
    for id in arriving {
        sim.per_cell_process(
            id,
            crate::sim::movement::PerCellReason::Arrival,
            Some(rules),
            None,
            FrameEffects::default(),
        )
        .expect("supplied infantry arrival");
    }
}

#[cfg(test)]
mod gunner_tests;

#[cfg(test)]
mod allegiance_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    use crate::sim::world::RevealOutcome;

    /// Replays the original CargoTotalSize execution controls. Size is added
    /// to the signed accumulator in passenger-list order and FTOL's low dword
    /// is retained after each entry, so neither a Rust float sum nor a final
    /// cast is an equivalent implementation.
    #[test]
    fn cargo_total_size_matches_original_ordered_ftol_controls() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/cargo_total_size.json",
        ))
        .unwrap();

        for row in native["rows"].as_array().unwrap() {
            let head_first: Vec<(u64, f64)> = row["input"]["size_f64_bits"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, bits)| {
                    let bits = u64::from_str_radix(bits.as_str().unwrap(), 16)
                        .expect("native Size bits are hexadecimal u64");
                    (index as u64 + 1, f64::from_bits(bits))
                })
                .collect();
            let expected = row["total_size_i32"].as_i64().unwrap() as i32;
            let mut cargo = PassengerCargo::new(head_first.len() as u32, 0.0);
            cargo.board_forced_chain(&head_first);
            assert_eq!(
                cargo.total_size(),
                expected,
                "{}",
                row["input"]["name"].as_str().unwrap_or("unnamed control")
            );
            assert_eq!(
                expected as u32,
                u32::from_str_radix(row["eax_hex"].as_str().unwrap(), 16).unwrap(),
                "native EAX encoding for {}",
                row["input"]["name"].as_str().unwrap_or("unnamed control")
            );
        }
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
        entity.set_owner_for_test(owner_id);
        entity.set_type_ref_for_test(type_id);
        entity.set_category_for_test(EntityCategory::Unit);
        entity.passenger_role = PassengerRole::transport(PassengerCargo::new(
            obj.passengers.max(0) as u32,
            obj.size_limit,
        ));
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

    /// Supply an infantry arrival: Capture for a garrison, Enter for other
    /// carriers, using the same production PerCell entry owner as the walk.
    fn spawn_entering_infantry(
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
        ge.set_owner_for_test(owner_id);
        ge.set_type_ref_for_test(type_id);
        ge.set_category_for_test(EntityCategory::Infantry);
        ge.mission_leaf =
            crate::sim::mission::leaf::MissionLeafState::for_entity_category(ge.category());
        let capture = sim
            .substrate
            .entities
            .get(transport_id)
            .and_then(|target| sim.object_type(target.type_ref(), rules))
            .is_some_and(|object| object.can_be_occupied);
        sim.substrate.entities.insert(ge);
        assert!(matches!(
            sim.reveal_entity_with_rules(stable_id, rules),
            RevealOutcome::Revealed { .. }
        ));
        // Unlimbo commits idle before this supplied already-entering state.
        // This component fixture starts after that lifecycle, not before it.
        let ge = sim.substrate.entities.get_mut(stable_id).unwrap();
        ge.navigation
            .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(
                transport_id,
            )));
        ge.mission.set_current_for_test(
            crate::sim::mission::MissionId::from_known(if capture {
                crate::sim::mission::MissionType::Capture
            } else {
                crate::sim::mission::MissionType::Enter
            }),
            0,
        );
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

    // --- Contract #2: lifecycle → active-order membership (conceal/reveal) ---

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
            spawn_entering_infantry(&mut sim, &rules, "E1", "Americans", transport, 10, 10);
        let actor = sim.substrate.entities.get_mut(passenger).unwrap();
        actor
            .health
            .set_for_test(row["input"]["health"].as_i64().unwrap() as i32);
        actor
            .attack_target
            .set_for_test(Some(crate::sim::combat::TargetKind::Entity(transport)));
        actor
            .mission_leaf
            .set_foot_firing_sequence(row["before"]["firing_latch"].as_u64().unwrap() as u8);

        tick_entry_arrival(&mut sim, &rules);

        let actor = sim.substrate.entities.get(passenger).unwrap();
        assert_eq!(
            actor.passenger_role.inside_transport(),
            Some((transport, true))
        );
        assert!(actor.attack_target.target().is_none());
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

    /// Retail depots and refineries have no configured passenger capacity.
    /// Their absence of ordinary transport admission does not constrain the
    /// raw Cargo leaf used by an already-admitted building entry.
    #[test]
    fn retail_no_service_building_carries_passengers() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let mut services = 0;
        for name in &rules.building_ids {
            let Some(object) = rules.object(name) else {
                continue;
            };
            if object.unit_repair || object.unit_reload || object.dock_unload {
                services += 1;
                assert!(object.passengers <= 0, "{name}");
            }
        }
        assert!(services > 0, "retail has depots and refineries");
    }
}
