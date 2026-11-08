//! Optional Ares transport payloads and Hospital/Armory eviction
//! (`docs/ares-transports.md`).
//!
//! - `InitialPayload.Types`/`.Nums` (`rules::ares_techno_type`): an object
//!   fills its hold or garrison once, `TechnoExt::ExtData::CreateInitialPayload
//!   @ 10043C10`, when it is first put on the map: FootClass::Unlimbo for a
//!   Unit or Aircraft (FootClass_Put_InitialPayload `100491B0`, hook
//!   `0x4D718C`, in `world::lifecycle`) and BuildingClass::Place past the
//!   free unit (BuildingClass_Place_InitialPayload `10048EB0`, hook
//!   `0x446EE2`, in `building_art`). An object constructed inside the
//!   A8E7AC placement scope after frame 0 never creates one
//!   (TechnoClass_Init_1 `1004A530`, hook `0x6F407D`, in
//!   `world::world_spawn`).
//! - A `Hospital=` or `Armory=` building evicts its first passenger
//!   (`BuildingExt::KickOutHospitalArmory @ 10012920`) on Mission_Unload
//!   (`100145B0`, hook `0x44D8A1`), on every Sell_Back (`10016970`, hook
//!   `0x447113`) and on ChangeOwner (`100163B0`, hook `0x448277`).

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

/// TechnoExt `PayloadCreated` (`+0x9E`, false from the constructor
/// 10042CA0): this object's InitialPayload has been made, or it never gets
/// one. Only this module writes it; it survives a type conversion, as the
/// TechnoExt does.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct InitialPayloadState {
    created: bool,
}

impl InitialPayloadState {
    /// TechnoClass_Init_1 (`1004A530`, hook `0x6F407D`): an object
    /// constructed while `IKnowWhatImDoing` (A8E7AC) is raised after frame 0
    /// counts as having made its payload.
    pub(crate) fn mark_created(&mut self) {
        self.created = true;
    }

    /// CreateInitialPayload's guard: whether this call makes the payload;
    /// either way the object has made it afterwards.
    fn take(&mut self) -> bool {
        !std::mem::replace(&mut self.created, true)
    }

    /// Folded while still pending, so a placed object adds nothing. An
    /// Infantry's state is never read (FootClass_Put skips Infantry and a
    /// conversion keeps the class), so it is not folded.
    pub(crate) fn hash_state(&self, category: EntityCategory, hasher: &mut impl std::hash::Hasher) {
        if !self.created && category != EntityCategory::Infantry {
            std::hash::Hash::hash(b"ares-initial-payload-pending-v1", hasher);
        }
    }
}

impl Simulation {
    /// `TechnoExt::ExtData::CreateInitialPayload @ 10043C10`, once per object:
    /// - the free slots are a `CanBeOccupied=` building's MaxNumberOccupants
    ///   less its occupants, else the type's `Passengers=` less the passenger
    ///   count (not their sizes);
    /// - each `InitialPayload.Types` entry, in order, skips a BuildingType or
    ///   AircraftType, and on a building anything but an InfantryType; its
    ///   count is the same `InitialPayload.Nums` entry, past the list's end
    ///   its last one, and 1 without any, capped at the free slots (signed:
    ///   a negative count frees slots and makes nothing);
    /// - each one is CreateObject for the owner (a constructor draw), then
    ///   - an occupiable building appends it to its occupants and refreshes
    ///     its cell threat (no EVA, raid or garrison art: AddGarrisonOccupant
    ///     does not run);
    ///   - another building puts it in its cargo: an `InfantryAbsorb=` one
    ///     marks it absorbed (Absorbed `+0x439`, the counted-total latch
    ///     `+0x438` lowering the house total), its house power dirty when
    ///     `ExtraPower=` is positive; any other first hears its HELLO
    ///     (RequestLink); then CargoClass AddPassenger and AbortMotion;
    ///   - a Foot takes the transport's Location (SetLocation vt+0x1B4), an
    ///     `OpenTopped=` transport makes it a rider (EnteredOpenTopped
    ///     `0x00710470`), Transporter is the transport, and AddPassenger
    ///     (vt+0x394, a `Gunner=` transport's gunner included) runs with
    ///     VocClass::VoicesEnabled cleared, so no EnterTransportSound;
    /// - after each entry an occupied building refreshes its cell threat
    ///   again (its UpdatePlacement(Redraw) is presentation).
    pub(crate) fn create_ares_initial_payload(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // Native always has the type; a call without its rules (a fixture's
        // empty set) leaves the payload pending rather than spend it.
        let Some(object) = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| self.entity_object_type(entity, rules))
        else {
            return;
        };
        if !self
            .substrate
            .entities
            .get_mut(id)
            .is_some_and(|entity| entity.ares_initial_payload.take())
        {
            return;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let (types, nums) = object.techno_combat().initial_payload();
        if types.is_empty() {
            return;
        }
        let building = entity.category() == EntityCategory::Structure;
        let occupiable = building && object.can_be_occupied;
        let mut free_slots = if occupiable {
            (object.max_number_occupants as i32)
                .wrapping_sub(entity.passenger_role.occupants().count())
        } else {
            object
                .passengers
                .wrapping_sub(entity.passenger_role.passenger_count() as i32)
        };
        let infantry_absorb = object.infantry_absorb;
        let extra_power = object.extra_power.output() > 0;
        let open_topped = object.open_topped;
        let owner = entity.owner();
        let owner_name = self.interner.resolve(owner).to_owned();
        let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let context = UninitContext::new(rules, registry).with_effects(effects);
        for (index, reference) in types.iter().enumerate() {
            if matches!(
                reference.category,
                ObjectCategory::Building | ObjectCategory::Aircraft
            ) || (building && reference.category != ObjectCategory::Infantry)
            {
                continue;
            }
            let Some(payload_type) = rules.object_in_category(reference.category, &reference.id)
            else {
                continue;
            };
            let num = nums.get(index).or(nums.last()).copied().unwrap_or(1);
            let count = num.min(free_slots);
            free_slots = free_slots.wrapping_sub(count);
            for _ in 0..count {
                let Some(payload) = self.construct_resolved_object_limbo_at_height(
                    payload_type,
                    &owner_name,
                    0,
                    0,
                    0,
                    0,
                    rules,
                    effects,
                ) else {
                    continue;
                };
                // Remove (vt+0xD4) finds the new object in limbo already.
                if occupiable {
                    if let Some(holder) = self.substrate.entities.get_mut(id) {
                        holder.passenger_role.occupants_mut().add(payload);
                    }
                    if let Some(occupant) = self.substrate.entities.get_mut(payload) {
                        // VERA's occupant relation, as AddGarrisonOccupant
                        // gives it.
                        occupant.passenger_role.set_inside_transport(id, false);
                    }
                    self.refresh_spatial_threat(id, rules, None);
                } else if building {
                    if infantry_absorb {
                        self.substrate.set_infantry_absorber_entry(payload, true);
                        Simulation::account_infantry_absorber_entry(
                            &mut self.substrate,
                            &mut self.houses,
                            payload,
                        );
                        if extra_power {
                            Simulation::invalidate_house_power(
                                &self.houses,
                                &mut self.power_states,
                                owner,
                                false,
                            );
                        }
                    } else {
                        let _ = crate::sim::radio::transmit(
                            self,
                            payload,
                            id,
                            crate::sim::radio::RadioMessage::Hello,
                            crate::sim::radio::RadioPayload::default(),
                            rules,
                            effects,
                        );
                    }
                    let _ = self.add_cargo_passenger_chain(
                        id,
                        &[(payload, payload_type.size)],
                        context,
                    );
                    if let Some(passenger) = self.substrate.entities.get_mut(payload) {
                        // A building's cargo shares VERA's carrier relation.
                        passenger.passenger_role.set_inside_transport(id, false);
                        // AbortMotion (`0x004DF0D0`).
                        passenger.navigation.stop_moving();
                    }
                } else {
                    crate::sim::movement::ground_pose::foot_set_location(
                        &mut self.substrate.entities,
                        payload,
                        location,
                        rules,
                        &self.interner,
                    );
                    let _ = self.ares_abduct_into_hold(
                        payload,
                        id,
                        payload_type.size,
                        open_topped,
                        false,
                        context,
                    );
                }
            }
            if building
                && self
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|holder| !holder.passenger_role.occupants().is_empty())
            {
                self.refresh_spatial_threat(id, rules, None);
            }
        }
    }

    /// `BuildingExt::KickOutHospitalArmory @ 10012920`: a `Hospital=` or
    /// `Armory=` building pops its first passenger and kicks it out through
    /// ExitObject (vt+0x100, `0x00443C60`) at the empty cell, ignoring the
    /// answer, as the Hospital's own release does (`docking::hospital`).
    /// Mission_Unload (hook `0x0044D8A1`), Sell_Back (`0x00447113`) and
    /// ChangeOwner (`0x00448277`) call it.
    pub(crate) fn ares_kick_out_hospital_armory(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if !self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.hospital || object.armory())
        {
            return;
        }
        let _ = crate::sim::passenger::depart_cargo_head(
            self,
            rules,
            registry,
            id,
            crate::sim::passenger::DepartureRoute::Hospital,
            |sim, patient| {
                let _ = crate::sim::production::exit_produced_object(
                    sim,
                    rules,
                    id,
                    patient,
                    (0, 0),
                    registry,
                    effects,
                );
                Ok(())
            },
            effects,
        );
    }
}

#[cfg(test)]
#[path = "ares_transports_tests.rs"]
mod tests;
