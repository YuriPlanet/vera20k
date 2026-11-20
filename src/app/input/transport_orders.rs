//! Self-click/cursor unload admission and the aircraft deploy-key route.
//!
//! Mobile transports use `Type+0x5E0 Passengers > 0` and the aircraft unload
//! slot `0x004151E0`; garrisons use GetOccupantCount; InfantryAbsorb buildings
//! use their native absorber flag. Buildings read Cargo and Occupants separately;
//! Building and Unit D instead use Event9; dispatch owns their separate admission.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;

/// Resolve self-click/cursor unload and the aircraft deploy key.
/// Unit transport D is handled by dispatch's separate Event9 route.
/// Structures unload their actual occupants or an InfantryAbsorb Cargo;
/// mobile transports retain their passenger-count gate.
/// The same projection authorizes the transport for the cursor and the key.
pub(crate) fn unload_command(
    sim: &crate::sim::world::Simulation,
    entity: &GameEntity,
    obj: Option<&ObjectType>,
    local_owner: Option<InternedId>,
) -> Option<Command> {
    unload_command_with(sim, &sim.control_projection(), entity, obj, local_owner)
}

pub(super) fn unload_command_with(
    sim: &crate::sim::world::Simulation,
    control: &crate::sim::world::ControlProjection<'_>,
    entity: &GameEntity,
    obj: Option<&ObjectType>,
    local_owner: Option<InternedId>,
) -> Option<Command> {
    let can_order = match local_owner {
        Some(owner) => control.can_order_from(entity.owner(), owner),
        None if sim.session().has_custom_control_grants() => control.can_order(entity.owner()),
        None => true,
    };
    if !can_order {
        return None;
    }
    let obj = obj?;
    let has_passengers = entity
        .passenger_role()
        .cargo()
        .is_some_and(|cargo| !cargo.is_empty());
    let can_unload = match entity.category() {
        // Building44731C..447358 reads independent Cargo+114 and Occupants+694.
        EntityCategory::Structure => {
            !entity.passenger_role().occupants().is_empty()
                || (obj.infantry_absorb && has_passengers)
        }
        EntityCategory::Unit => obj.passengers > 0 && has_passengers,
        // Aircraft417DCD..417DDA keeps Self while the hold (+0x114) holds
        // anyone, whatever Passengers=: a Carryall's cargo too.
        EntityCategory::Aircraft => has_passengers,
        EntityCategory::Infantry => false,
    };
    if !can_unload {
        return None;
    }
    Some(Command::UnloadPassengers {
        transport_id: entity.stable_id(),
    })
}
