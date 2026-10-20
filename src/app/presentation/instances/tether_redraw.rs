//! A building that unloads draws its tethered contacts in its own Display
//! slot, and a Unit tethered to it draws only there.

use crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance;
use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

/// Mark the planned parents of this frame's tether draws.
///
/// `BuildingClass` vt+0x4E4 (`0x0043DA80`) opens with a redraw
/// (`0x0043DAA0..0x0043DC0D`). A building whose Get_Mission (vt+0x184) is
/// Unload and whose tether byte (`+0x418`) is set draws each radio contact
/// (`0x0065AD40`, slot order) again with its own DrawIt (vt+0x114), at the
/// point its ordinary draw uses (vt+0xAC `0x0041BE00` -> `0x006D2140`). Empty
/// slots, contacts in Limbo (`+0x81`) and Buildings are skipped. Its fog skip
/// asks `0x005865E0`, which is `xor al, al` in YR, so it never skips.
///
/// `UnitClass::DrawIfVisible @ 0x0073B0B0` refuses a Unit whose tether byte is
/// set and whose contact 0 is a Building with Get_Mission or queued mission
/// (`+0xB4`) Unload. Infantry and Aircraft reach `0x005F4B10`, which has no
/// such refusal. Both also ask the door at `+0x350` whether it is opening,
/// closing, open or closed (`0x004A5110`, `0x004A5130`, `0x004A51B0`,
/// `0x004A51D0`); every door is in one of those four phases
/// (`sim::door::DoorPhase`), so that read never refuses.
///
/// The redraw repeats the pieces the contact's own builder planned. A contact
/// the tactical admission did not plan is not drawn again, where native's
/// DrawIt would still run. Its health bar and pips stay in its own slot,
/// because `0x006D9789` draws Extras for every Display parent.
pub(crate) fn assign_tether_redraws(sim: &Simulation, planned: &mut [PlannedObjectInstance]) {
    let entities = sim.entities();
    for object in planned.iter_mut() {
        let Some(entity) = entities.get(object.parent.id) else {
            continue;
        };
        if entity.tether.partner().is_none() {
            continue;
        }
        match entity.category() {
            EntityCategory::Structure if gets_unload(entity) => {
                object.tether_redraws = entity
                    .radio_contacts
                    .iter_live()
                    .filter(|&contact| {
                        entities.get(contact).is_some_and(|contact| {
                            !contact.lifecycle.in_limbo()
                                && contact.category() != EntityCategory::Structure
                        })
                    })
                    .collect();
            }
            EntityCategory::Unit => {
                object.drawn_by_tether = entity
                    .radio_contacts
                    .slot(0)
                    .and_then(|contact| entities.get(contact))
                    .is_some_and(|building| {
                        building.category() == EntityCategory::Structure
                            && (gets_unload(building)
                                || building.mission.queued().known() == Some(MissionType::Unload))
                    });
            }
            _ => {}
        }
    }
}

/// Get_Mission (vt+0x184) is Unload.
fn gets_unload(entity: &GameEntity) -> bool {
    entity.mission.effective().known() == Some(MissionType::Unload)
}
