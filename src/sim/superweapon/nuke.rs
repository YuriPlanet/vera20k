//! The nuclear missile's launch: `SuperClass::Launch @ 0x006CC390` case 0
//! (`Type=MultiMissile`, `0x006CDA67`) for a Super that is not one-time.
//! The silo's flight (`BuildingClass::Mission_Missile @ 0x0044C980`) and
//! the falling warhead (`NukeMaker=`, `0x0046B310`) belong to the building
//! missions and the projectile detonation.
//!
//! RESIDUALS:
//! - The one-time arm (`0x006CDA88..0x006CDCB3`: a NukePayload bullet
//!   dropped straight on the cell): VERA grants no one-time Super.
//! - Presentation for the player's own launch (`0x006CDE06..0x006CDE27`):
//!   the targeting cursor's type (`0x008809A0`) is cleared and a queued
//!   `EVA_NuclearMissileReady` line removed.
//! - `HouseClass+0x1FC` (`0x006CDE2F`) asks the owner's next House update
//!   for its grant and revoke passes, which change nothing after a launch
//!   (the silo still stands); VERA refreshes grants at building events.
//! - The launch-mute global `0x00A8B538` that skips the EVA line
//!   (`0x006CDDEE`) is not modeled: the line always plays.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::world::{SimSoundEvent, Simulation};

/// Launch case 0 for `owner`'s Super of type `sw` at `cell`: the first
/// BuildingType in BuildingTypeClass::Array order with `NukeSilo=`
/// (`+0x16BA`) whose `SuperWeapon=` (`+0x16F0`) or `SuperWeapon2=`
/// (`+0x16F4`) is the type's array index (`+0x98`) names the silo type
/// (`0x006CDCF0..0x006CDD30`); the owner's first building of it
/// ([`find_building_of_type`]) queues and commences Missile, the owner's
/// NukeTarget (`+0x5784`) becomes the cell and the silo's firing type
/// (`+0x5F8`) the type's `Type=` (`0x006CDDA3..0x006CDDD7`). The app plays
/// `DigSound=` at the cell and `EVA_NuclearMissileLaunched`
/// (`0x006CDDE9`, `0x006CDE01`) for the launch event. Without such a
/// building nothing happens.
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) -> bool {
    let Some(index) = rules.super_weapon_index(&sw.id) else {
        return false;
    };
    let fires = |object: &ObjectType| {
        object.nuke_silo
            && [&object.super_weapon, &object.super_weapon2]
                .into_iter()
                .any(|name| {
                    name.as_deref()
                        .and_then(|name| rules.super_weapon_index(name))
                        == Some(index)
                })
    };
    let ids = rules.type_array_ids(ObjectCategory::Building);
    let Some(silo_type) = (0..ids.len())
        .find(|&slot| rules.building_type_at(slot as i32).is_some_and(fires))
        .and_then(|slot| sim.interner.get(&ids[slot]))
    else {
        return false;
    };
    let Some(silo) = find_building_of_type(sim, owner, silo_type) else {
        return false;
    };
    crate::sim::world::queue_and_commence(sim, silo, MissionType::Missile, rules);
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.set_nuke_target(cell);
    }
    if let Some(entity) = sim.substrate.entities.get_mut(silo) {
        entity
            .mission_leaf
            .set_building_firing_super_weapon(sw.kind.native_index());
    }
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type: sw_type_id,
        rx: cell.0,
        ry: cell.1,
    });
    true
}

/// `HouseClass::Find_Building_Of_Type @ 0x004FD060` with no edge (`-1`):
/// when the house's owned-type counter (`+0x5500`, `0x0049FAE0`) holds the
/// type, its first building (`+0x68` order) of that type outside Limbo.
fn find_building_of_type(sim: &Simulation, owner: InternedId, type_id: InternedId) -> Option<u64> {
    let house = sim.houses.get(&owner)?;
    if house
        .tracking
        .owned_count(EntityCategory::Structure, type_id)
        <= 0
    {
        return None;
    }
    house
        .base_projection
        .buildings()
        .iter()
        .copied()
        .find(|&id| {
            sim.substrate.entities.get(id).is_some_and(|building| {
                !building.lifecycle.in_limbo && building.type_ref() == type_id
            })
        })
}
