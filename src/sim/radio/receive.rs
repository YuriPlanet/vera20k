//! Per-class `Receive_Radio` handlers — the receiver half of the radio bus.
//!
//! Every receiver runs inline inside the sender's [`crate::sim::radio::transmit`]
//! (synchronous RPC, no queue) and walks the native class chain: Building
//! `0x0043C2D0`, Unit `0x00737430` → Foot `0x004D8FB0` → Techno `0x006F4AB0` →
//! Radio `0x0065A820` (Infantry enters at Foot, Aircraft `0x004190B0` at its
//! own arms then Foot). A level that does not handle a message returns what the
//! level below returns. A class arm VERA does not represent answers static (0)
//! instead of falling through, so a message whose native arm is unported has no
//! invented effect; each such arm names its native address below.
//!
//! Native evidence: tools/spatial_oracle/refinery_dock.json — the `radio` rows
//! (HELLO/OVER_OUT/TETHER on real Unit and Building receivers) and the
//! `can_dock` rows (the DOCKING handshake with every nested transmit).
//!
//! RESIDUALS (no represented reader or sender):
//! - `RadioClass+0xD4..+0xDC`, the last three distinct received messages
//!   (`0x0065A829`), is not kept.
//! - The bus carries no overlay registry, so RUN_AWAY's Scatter reaches an
//!   Infantry receiver without one, and its overlay reads fail on any cell
//!   holding an overlay (ore included); the error is logged. When
//!   Find_Nearby_Passable_Cell found a cell, the draw and the setter have
//!   already run: the man keeps that destination and walks at his next turn,
//!   but loses the immediate Process. When the neighbour fallback runs
//!   instead, he stays. Trigger: RUN_AWAY reaching an infantryman in radio
//!   contact. Frequency: rare, radio contacts are mostly docked Units. A Unit
//!   receiver reads no overlay.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/radio + sim/world. sim/ NEVER depends on
//!   render/, ui/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::docking::bunker_install::BunkerState;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ScatterFlags;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
#[cfg(test)]
use crate::sim::world::LifecycleTestEvent;
use crate::sim::world::{FrameEffects, Simulation};

/// The hull facing a Unit turns to on PREPARE_TO_DOCK (`0x007376E6`): east.
pub(crate) const DOCK_FACING: u16 = 0x4000;

/// The pad cell a DockUnload/Weeder dock sends in MOVE_HERE
/// (`0x0043CA71..0x0043CAB8`): its NW cell (`Get_Cell`, Location / 256) plus
/// (3, 1), CellStruct int16 adds. The stock refinery pad is this cell.
pub(crate) fn dock_pad_cell(rx: u16, ry: u16) -> (u16, u16) {
    (
        (rx as i16).wrapping_add(3) as u16,
        (ry as i16).wrapping_add(1) as u16,
    )
}

/// Receiver-side radio dispatch. `target_sid` is the receiver; `sender_sid` is
/// the RTTI-filtered sender (`None` when the sender failed the Techno filter).
/// Returns the receiver's response code.
pub(crate) fn receive_radio(
    sim: &mut Simulation,
    target_sid: u64,
    sender_sid: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let Some(category) = sim.substrate.entities.get(target_sid).map(|t| t.category) else {
        return RadioResponse::None;
    };
    match category {
        EntityCategory::Structure => {
            building_receive(sim, target_sid, sender_sid, msg, payload, rules, effects)
        }
        EntityCategory::Unit => {
            unit_receive(sim, target_sid, sender_sid, msg, payload, rules, effects)
        }
        EntityCategory::Infantry => {
            foot_receive(sim, target_sid, sender_sid, msg, payload, rules, effects)
        }
        EntityCategory::Aircraft => {
            aircraft_receive(sim, target_sid, sender_sid, msg, payload, rules, effects)
        }
    }
}

/// `BuildingClass::Receive_Radio @ 0x0043C2D0`.
fn building_receive(
    sim: &mut Simulation,
    building: u64,
    sender: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    // VERA's bunker install adapter owns a Bunker= building's CAN_LOAD and
    // DOCK_NOW (native 0x0043C4F8 / 0x0043C75A arms).
    if is_bunker_building(sim, building)
        && matches!(msg, RadioMessage::CanEnter | RadioMessage::DockNow)
    {
        return bunker_receive(sim, building, sender, msg);
    }
    match msg {
        // Original43CD01: Begin_Mode(IDLE) precedes the common teardown.
        // Human PLACE reaches this while the child's body is Construction0.
        RadioMessage::Break => {
            if let Some(entity) = sim.substrate.entities.get_mut(building) {
                entity.begin_building_body(
                    crate::sim::building_construction::BuildingBodyMode::Idle,
                    sim.session.binary_frame as i32,
                );
            }
            let _ = techno_receive(sim, building, sender, msg, payload, rules, effects);
            RadioResponse::Roger
        }
        // Original43CC39/43CCE0: the Construction status-zero first-contact
        // message queues Repair, then delegates to Techno and answers ROGER.
        RadioMessage::DockApproach => {
            if let Some(entity) = sim.substrate.entities.get_mut(building) {
                crate::sim::mission::authority::queue_entity_mission_deferred(
                    entity,
                    MissionId::from_known(MissionType::Repair),
                );
            }
            let _ = techno_receive(sim, building, sender, msg, payload, rules, effects);
            RadioResponse::Roger
        }
        // Original43CC4C..43CCE0: C queues Guard unless Selling. A yard
        // replaces PreProduction7/Idle18 with the live health's Production8.
        RadioMessage::DockArrived => {
            let selling = sim.substrate.entities.get(building).is_some_and(|entity| {
                entity.mission.effective().known() == Some(MissionType::Selling)
            });
            if !selling {
                if let Some(entity) = sim.substrate.entities.get_mut(building) {
                    crate::sim::mission::authority::queue_entity_mission_deferred(
                        entity,
                        MissionId::from_known(MissionType::Guard),
                    );
                }
                if let Some(rules) = rules
                    && let Some(entity) = sim.substrate.entities.get(building)
                    && let Some(object) = sim.object_type(entity.type_ref(), rules)
                    && object.construction_yard
                {
                    let damaged = crate::sim::building_art::requested_damage_state(
                        entity.health,
                        object.strength,
                        rules.general.condition_yellow,
                    );
                    sim.clear_building_anim_slot(building, 7);
                    sim.clear_building_anim_slot(building, 18);
                    sim.set_building_anim_slot(building, 8, damaged, false, 0, rules);
                }
            }
            let _ = techno_receive(sim, building, sender, msg, payload, rules, effects);
            RadioResponse::Roger
        }
        RadioMessage::CanDock => building_docking(sim, building, sender, rules, effects),
        RadioMessage::DockNow => building_dock_now(sim, building, sender, payload, rules, effects),
        RadioMessage::CanEnter => building_can_load(sim, building, sender, payload, rules, effects),
        RadioMessage::RequestClearance => {
            let Some(object) = rules.and_then(|rules| {
                sim.substrate
                    .entities
                    .get(building)
                    .and_then(|entity| sim.object_type(entity.type_ref(), rules))
            }) else {
                return RadioResponse::None;
            };
            // 0x0043CD2B reads UnitRepair (+0x16A9) and Bunker (+0x16AB): a
            // sender under 0x180 leptons from the dock (both GetCoords,
            // `(dx²+dz²)+dy²` through Sqrt_Approx and ftol) stays linked and
            // is answered ROGER (`0x0043CD4D..0x0043CDC8`).
            if (object.unit_repair || object.bunker)
                && sender
                    .and_then(|from| sim.substrate.entities.get(from))
                    .zip(sim.substrate.entities.get(building))
                    .is_some_and(|(from, dock)| {
                        let terrain = sim.resolved_terrain.as_ref();
                        let a = crate::sim::movement::ground_pose::object_get_coords(dock, terrain);
                        let b = crate::sim::movement::ground_pose::object_get_coords(from, terrain);
                        crate::util::native_x87::distance_3d_leptons(
                            [a.x, a.z, a.y],
                            [b.x, b.z, b.y],
                        ) < 0x180
                    })
            {
                return RadioResponse::Roger;
            }
            //43CDDD delegates before re-reading the live type at43CDE2.
            let _ = techno_receive(sim, building, sender, msg, payload, rules, effects);
            if rules.is_some_and(|rules| {
                sim.substrate
                    .entities
                    .get(building)
                    .and_then(|entity| sim.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| {
                        object.weapons_factory || object.unit_repair || object.bunker
                    })
            }) {
                RadioResponse::Queued //43CE18 returns literal0x17.
            } else {
                RadioResponse::Roger
            }
        }
        RadioMessage::AnimStop => {
            //43CE24..CE44: a WeaponsFactory answers1 before Object Mark2.
            //Other classes reach the existing shared refresh dispatcher.
            if rules.is_some_and(|rules| {
                sim.substrate
                    .entities
                    .get(building)
                    .and_then(|entity| sim.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| object.weapons_factory)
            }) {
                RadioResponse::Roger
            } else {
                techno_receive(sim, building, sender, msg, payload, rules, effects)
            }
        }
        _ => techno_receive(sim, building, sender, msg, payload, rules, effects),
    }
}

/// CAN_LOAD0F, Building43C2D0: common admission, then class/type arm.
/// The refinery scanner and pending depot entry use this same receiver.
/// Effective mission43C310, body43C34C, contacts43C35A, Naval/Balloon/online
/// gates precede UnitRepair43C61B and DockUnload43C64F. The wide scan's
/// A8E7AC context skips contact/passenger capacity, not the other gates.
fn building_can_load(
    sim: &mut Simulation,
    building_id: u64,
    sender: Option<u64>,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let (Some(from), Some(rules)) = (sender, rules) else {
        return RadioResponse::None;
    };
    let (Some(building), Some(unit)) = (
        sim.substrate.entities.get(building_id),
        sim.substrate.entities.get(from),
    ) else {
        return RadioResponse::None;
    };
    let (Some(building_type), Some(unit_type)) = (
        sim.object_type(building.type_ref(), rules),
        sim.object_type(unit.type_ref(), rules),
    ) else {
        return RadioResponse::None;
    };
    if !crate::sim::combat::combat_weapon::is_ally_by_object(
        Some(&sim.fog.alliances),
        &sim.interner,
        building.owner(),
        unit.owner(),
    ) {
        return RadioResponse::None;
    }
    let absorbs = building_type.unit_absorb || building_type.infantry_absorb;
    if matches!(
        building.mission.effective().known(),
        Some(MissionType::Construction | MissionType::Selling)
    ) || building.in_construction_bstate()
        || (!payload.ignore_dock_capacity && !absorbs && !building.radio_contacts.has_free_or(from))
        || (unit_type.movement_zone != crate::rules::locomotor_type::MovementZone::Amphibious
            && building_type.naval != unit_type.naval)
        || unit_type.balloon_hover
        || !building.building_online()
    {
        return RadioResponse::Negatory;
    }
    // 0x0043C43B..0x0043C4F2, an absorber (UnitAbsorb +0x16AE /
    // InfantryAbsorb +0x16AF): a Unit or an infantryman it does not absorb,
    // or a sender controlling something (0x004722C0), is refused. With a
    // free passenger slot (+0x114 against +0x5E0) and the sender's Size=
    // within SizeLimit= it answers ROGER; otherwise the later arms decide.
    // Size= and SizeLimit= are native doubles that VERA reads as integers
    // (equal on every retail value).
    if absorbs {
        let class_refused = match unit.category {
            EntityCategory::Unit => !building_type.unit_absorb,
            EntityCategory::Infantry => !building_type.infantry_absorb,
            _ => false,
        };
        if class_refused
            || unit
                .capture_manager
                .as_ref()
                .is_some_and(|manager| manager.has_any())
        {
            return RadioResponse::Negatory;
        }
        let passengers = building
            .passenger_role
            .cargo()
            .map_or(0, |cargo| cargo.count());
        if i64::from(passengers) < i64::from(building_type.passengers)
            && unit_type.size <= building_type.size_limit
        {
            return RadioResponse::Roger;
        }
    }
    // Other building mechanisms retain their existing owner/residual.
    if building_type.grinding || building_type.bunker {
        return RadioResponse::None;
    }
    if building_type.unit_repair {
        if !matches!(
            unit.category,
            EntityCategory::Unit | EntityCategory::Aircraft
        ) {
            return RadioResponse::Negatory;
        }
        return match transmit(
            sim,
            building_id,
            from,
            RadioMessage::IsOccupied,
            RadioPayload::default(),
            Some(rules),
            effects,
        ) {
            RadioResponse::Roger => RadioResponse::Negatory,
            _ => RadioResponse::Roger,
        };
    }
    // 0x0043C620: a Helipad takes an Aircraft and nothing else.
    if building_type.helipad {
        return if unit.category == EntityCategory::Aircraft {
            RadioResponse::Roger
        } else {
            RadioResponse::Negatory
        };
    }
    if unit.category == EntityCategory::Unit
        && (building_type.dock_unload && unit_type.harvester
            || building_type.weeder && unit_type.weeder)
    {
        // Native passenger-head+118 is never filled by a refinery unload.
        return RadioResponse::Roger;
    }
    RadioResponse::None
}

/// IsOccupied (0x23), `FootClass::Receive_Radio @ 0x004D8FB0`: ROGER when
/// the first building in the cell of the foot's coordinate is the sender,
/// else NEGATORY.
fn foot_is_occupied(sim: &Simulation, foot: u64, sender: Option<u64>) -> RadioResponse {
    let Some(entity) = sim.substrate.entities.get(foot) else {
        return RadioResponse::None;
    };
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    let building = u16::try_from(x / 256)
        .ok()
        .zip(u16::try_from(y / 256).ok())
        .and_then(|(rx, ry)| {
            sim.substrate.occupancy.first_building_on_layer(
                rx,
                ry,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            )
        });
    if sender.is_some() && building == sender {
        RadioResponse::Roger
    } else {
        RadioResponse::Negatory
    }
}

/// DOCKING, `BuildingClass::Receive_Radio` case 0x0E (`0x0043C7E9`): HELLO a
/// free sender back and ask it whether it is still moving (0x13). A
/// `DockUnload=`/`Weeder=` dock then sends it to the pad (MOVE_HERE 0x12)
/// and, once it answers that it is already there, tethers it (0x18) and has
/// it turn and report ready (0x16). A `Helipad=` sends it to itself (MOVE_HERE
/// with the dock as the parameter, `0x0043CA47`) and, answered ALREADY_THERE,
/// sends TETHER to its own first contact (`0x0043CA5C`). Any other building
/// stops after 0x13. The answer is ROGER unless the dock is offline.
fn building_docking(
    sim: &mut Simulation,
    building_id: u64,
    sender: Option<u64>,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let (Some(from), Some(rules)) = (sender, rules) else {
        return RadioResponse::None;
    };
    // 0x0043C7F6 runs the Techno receiver first; for 0x0E its only effect is
    // the unrepresented message history.
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return RadioResponse::None;
    };
    // 0x0043C7FB: the +0x660 online latch.
    if !building.building_online() {
        return RadioResponse::Negatory;
    }
    let Some(object) = sim.object_type(building.type_ref(), rules) else {
        return RadioResponse::None;
    };
    // UnitRepair43C814..849 asks the existing contact whether its
    // signed Health/Strength is full before any HELLO or movement probe.
    let unit_repair = object.unit_repair;
    if object.bunker {
        // This docking mechanism retains its existing adapter.
        return RadioResponse::None;
    }
    if unit_repair
        && building.radio_contacts.contains(from)
        && transmit(
            sim,
            building_id,
            from,
            RadioMessage::IsRepairing,
            RadioPayload::default(),
            Some(rules),
            effects,
        ) == RadioResponse::Negatory
    {
        return RadioResponse::Negatory;
    }
    let building = sim
        .substrate
        .entities
        .get(building_id)
        .expect("docking retains building");
    let object = sim
        .object_type(building.type_ref(), rules)
        .expect("docking retains type");
    let pad_dock = object.dock_unload || object.weeder;
    let helipad = object.helipad;
    let (rx, ry) = (building.position.rx, building.position.ry);
    // 0x0043C8A4..0x0043C8CC: a sender that is not a contact is HELLOed back
    // when a slot is free or already its own.
    if !building.radio_contacts.contains(from) && building.radio_contacts.has_free_or(from) {
        transmit(
            sim,
            building_id,
            from,
            RadioMessage::Hello,
            RadioPayload::default(),
            Some(rules),
            effects,
        );
    }
    // 0x0043C8D1..0x0043C93A: a contacted Foot whose NavCom is not the
    // GetDockCoord cell (NW+(2,1) for the stock refinery) forces MOVE_HERE.
    let contacted = sim
        .substrate
        .entities
        .get(building_id)
        .is_some_and(|building| building.radio_contacts.contains(from));
    let pad_force = contacted
        && pad_dock
        && sim.substrate.entities.get(from).is_some_and(|foot| {
            foot.category != EntityCategory::Structure
                && foot.navigation.nav_com.is_some()
                && foot.navigation.nav_com
                    != crate::sim::movement::building_dock_cell(
                        &sim.substrate.entities,
                        building_id,
                        Some(from),
                        sim.resolved_terrain.as_ref(),
                        rules,
                        &sim.interner,
                    )
                    .map(|(x, y)| NavTargetRef::cell(x, y))
        });
    // 43C93F..C9F0: a repair contact more than128 native leptons away
    // forces the same movement probe. Reuse the native deterministic distance
    // owner; the original sqrt approximation changes the129-lepton boundary.
    let repair_force = unit_repair
        && sim
            .substrate
            .entities
            .get(building_id)
            .and_then(|building| building.radio_contacts.slot(0).map(|id| (building, id)))
            .and_then(|(building, id)| sim.substrate.entities.get(id).map(|unit| (building, unit)))
            .is_some_and(|(building, unit)| {
                let a = crate::sim::movement::ground_pose::object_get_coords(
                    building,
                    sim.resolved_terrain.as_ref(),
                );
                let b = crate::sim::movement::ground_pose::object_get_coords(
                    unit,
                    sim.resolved_terrain.as_ref(),
                );
                crate::util::native_x87::distance_3d_leptons([a.x, a.y, a.z], [b.x, b.y, b.z]) > 128
            });
    let force = pad_force || repair_force;
    let moving = transmit(
        sim,
        building_id,
        from,
        RadioMessage::NeedToMove,
        RadioPayload::default(),
        Some(rules),
        effects,
    );
    if moving != RadioResponse::Roger && !force {
        return RadioResponse::Roger;
    }
    // 0x0043CA13..0x0043CA62: a DockUnload/Weeder dock walks the sender
    // onto its pad; a Helipad calls it to itself.
    if !pad_dock {
        if helipad
            && transmit(
                sim,
                building_id,
                from,
                RadioMessage::MoveToCell,
                RadioPayload {
                    target: Some(NavTargetRef::Building { id: building_id }),
                    ..RadioPayload::default()
                },
                Some(rules),
                effects,
            ) == RadioResponse::AlreadyThere
        {
            crate::sim::radio::transmit_to_contact(
                sim,
                building_id,
                RadioMessage::Tether,
                Some(rules),
                effects,
            );
        }
        return RadioResponse::Roger;
    }
    // 0x0043CA71..0x0043CAB8: the pad is Get_Cell() + (3, 1), CellStruct
    // int16 adds on the NW foundation cell.
    let pad = dock_pad_cell(rx, ry);
    let arrived = transmit(
        sim,
        building_id,
        from,
        RadioMessage::MoveToCell,
        RadioPayload {
            target: Some(NavTargetRef::cell(pad.0, pad.1)),
            ..RadioPayload::default()
        },
        Some(rules),
        effects,
    );
    if arrived != RadioResponse::AlreadyThere {
        return RadioResponse::Roger;
    }
    transmit(
        sim,
        building_id,
        from,
        RadioMessage::Tether,
        RadioPayload::default(),
        Some(rules),
        effects,
    );
    // 0x0043CAD4..0x0043CAF7: a non-ROGER answer would Scatter the sender;
    // every represented PREPARE_TO_DOCK receiver (Unit 0x007376AD, Techno
    // 0x006F4C6F) answers ROGER.
    let _ = transmit(
        sim,
        building_id,
        from,
        RadioMessage::PrepareToDock,
        RadioPayload::default(),
        Some(rules),
        effects,
    );
    RadioResponse::Roger
}

/// DOCK_NOW, `BuildingClass::Receive_Radio` case 0x15 (`0x0043C6F2`): a
/// building being sold refuses; a `DockUnload=` dock queues Unload on the
/// sender (no contact, tether or position test).
fn building_dock_now(
    sim: &mut Simulation,
    building_id: u64,
    sender: Option<u64>,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let Some(from) = sender else {
        return RadioResponse::None;
    };
    let Some(building) = sim.substrate.entities.get(building_id) else {
        return RadioResponse::None;
    };
    // 0x0043C6F6: Get_Mission == Selling.
    if building.mission.effective() == MissionId::from_known(MissionType::Selling) {
        return RadioResponse::Negatory;
    }
    let Some(rules) = rules else {
        return RadioResponse::None;
    };
    let Some(object) = sim.object_type(building.type_ref(), rules) else {
        return RadioResponse::None;
    };
    // 0x0043C710..0x0043C72C: UnitAbsorb/InfantryAbsorb answer ROGER.
    if object.unit_absorb || object.infantry_absorb {
        return RadioResponse::Roger;
    }
    // 0x0043C732..0x0043C785: the repair and reload docks and Bunker queue
    // their own mission; VERA's depot and bunker flows own those links.
    // RESIDUAL: Hospital= and Armory= share this arm natively; neither has a
    // represented DOCK_NOW sender.
    if object.unit_repair || object.unit_reload {
        // Building43C7B5..C7DC: service request and the occupant's Sleep
        // queue. The Building mission owns the admitted pad contact.
        let now = sim.session.binary_frame;
        let _ = sim.mission_queue_exact(
            building_id,
            MissionId::from_known(MissionType::Repair),
            0,
            now,
            &EntityReadyInputProvider,
        );
        let _ = sim.mission_queue_exact(
            from,
            MissionId::from_known(MissionType::Sleep),
            0,
            now,
            &EntityReadyInputProvider,
        );
        if let Some(building) = sim.substrate.entities.get_mut(building_id) {
            building.mission_leaf.set_building_ready_latch(1);
        }
        return RadioResponse::Roger;
    }
    if object.bunker {
        return RadioResponse::None;
    }
    // 0x0043C788..0x0043C7B2: Queue_Mission(Unload, 0) on the sender.
    if object.dock_unload {
        let _ = sim.mission_queue_exact(
            from,
            MissionId::from_known(MissionType::Unload),
            0,
            sim.session.binary_frame,
            &EntityReadyInputProvider,
        );
        return RadioResponse::Roger;
    }
    techno_receive(
        sim,
        building_id,
        sender,
        RadioMessage::DockNow,
        payload,
        Some(rules),
        effects,
    )
}

/// `UnitClass::Receive_Radio @ 0x00737430`.
fn unit_receive(
    sim: &mut Simulation,
    unit: u64,
    sender: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    match msg {
        // 0x00737B14: a unit on Return queues Guard, then the Foot receiver;
        // the answer is always ROGER.
        RadioMessage::Break => {
            if sim.substrate.entities.get(unit).is_some_and(|e| {
                e.mission.effective() == MissionId::from_known(MissionType::Return)
            }) {
                let _ = sim.mission_queue_exact(
                    unit,
                    MissionId::from_known(MissionType::Guard),
                    0,
                    sim.session.binary_frame,
                    &EntityReadyInputProvider,
                );
            }
            let _ = foot_receive(sim, unit, sender, msg, payload, rules, effects);
            RadioResponse::Roger
        }
        RadioMessage::PrepareToDock => {
            unit_prepare_to_dock(sim, unit, sender, payload, rules, effects)
        }
        // 0x00737A98: a harvester mid-unload leaves it, then the Foot arm.
        RadioMessage::RunAway => {
            unit_run_away(sim, unit, rules, effects);
            foot_receive(sim, unit, sender, msg, payload, rules, effects)
        }
        RadioMessage::CanEnter => rules.map_or(RadioResponse::None, |rules| {
            unit_can_enter_radio(sim, unit, sender, rules)
        }),
        // 7, 0xE and 0x15: the transport/service arms have no
        // represented sender.
        RadioMessage::DockingComplete | RadioMessage::CanDock | RadioMessage::DockNow => {
            RadioResponse::None
        }
        _ => foot_receive(sim, unit, sender, msg, payload, rules, effects),
    }
}

/// Unit737430 message0F, 73758A..7376AA: the mobile transport's admission
/// query. Both the UI adapter and the real arrival radio use this one owner.
/// Native controls: unit_transport_can_enter.py. It reads the receiver's
/// one-way alliance, the ground Cell building list, and total cargo Size,
/// not the passenger count. Size/SizeLimit are native doubles represented as
/// integers here; the saved controls cover integer retail values only.
pub(crate) fn unit_can_enter_radio(
    sim: &Simulation,
    unit: u64,
    sender: Option<u64>,
    rules: &RuleSet,
) -> RadioResponse {
    let Some((transport, passenger)) = sender.and_then(|sender| {
        sim.substrate
            .entities
            .get(unit)
            .zip(sim.substrate.entities.get(sender))
    }) else {
        return RadioResponse::None;
    };
    let Some((transport_type, passenger_type)) = sim
        .object_type(transport.type_ref(), rules)
        .zip(sim.object_type(passenger.type_ref(), rules))
    else {
        return RadioResponse::None;
    };
    if transport_type.passengers == 0
        || !crate::sim::combat::combat_weapon::is_ally_by_object(
            Some(&sim.fog.alliances),
            &sim.interner,
            transport.owner(),
            passenger.owner(),
        )
        || transport.is_warped_out()
    {
        return RadioResponse::None;
    }
    // GetCell+1BC -> ground CellClass::Look_up_building_in_cell47C520.
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(transport);
    if u16::try_from(x / 256)
        .ok()
        .zip(u16::try_from(y / 256).ok())
        .and_then(|(rx, ry)| {
            sim.substrate.occupancy.first_building_on_layer(
                rx,
                ry,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            )
        })
        .is_some()
        || passenger.mind_control.is_mind_controlled()
        || passenger.parasite_eating_me.is_some()
        || passenger
            .capture_manager
            .as_ref()
            .is_some_and(|manager| manager.has_any())
    {
        return RadioResponse::None;
    }
    let occupied = transport
        .passenger_role
        .cargo()
        .map_or(0, |cargo| cargo.total_size);
    if i64::from(occupied) + i64::from(passenger_type.size) > i64::from(transport_type.passengers)
        || passenger_type.size > transport_type.size_limit
    {
        RadioResponse::Negatory
    } else {
        RadioResponse::Roger
    }
}

/// PREPARE_TO_DOCK, `UnitClass::Receive_Radio` case 0x16 (`0x007376AD`):
/// turn the hull to exactly [`DOCK_FACING`] unless the turret is mid-swing
/// (`+0x6AF`); once facing and stopped, a tethered unit on Enter whose first
/// contact is a building sends it DOCK_NOW.
fn unit_prepare_to_dock(
    sim: &mut Simulation,
    unit: u64,
    sender: Option<u64>,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    // 0x007376BA: the Foot receiver first (Techno sends TETHER back).
    let _ = foot_receive(
        sim,
        unit,
        sender,
        RadioMessage::PrepareToDock,
        payload,
        rules,
        effects,
    );
    let frame = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get_mut(unit) else {
        return RadioResponse::Roger;
    };
    if !entity.turret_rotation_latch && entity.body_facing_current(frame) != DOCK_FACING {
        crate::sim::movement::drive_do_turn(entity, DOCK_FACING, frame);
        return RadioResponse::Roger;
    }
    if crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false) {
        return RadioResponse::Roger;
    }
    let dock = entity
        .radio_contacts
        .slot(0)
        .filter(|_| entity.dock_entered_with.is_some())
        .filter(|_| entity.mission.effective() == MissionId::from_known(MissionType::Enter));
    let dock = dock.filter(|&dock| {
        sim.substrate
            .entities
            .get(dock)
            .is_some_and(|contact| contact.category == EntityCategory::Structure)
    });
    if let Some(dock) = dock {
        transmit(
            sim,
            unit,
            dock,
            RadioMessage::DockNow,
            RadioPayload::default(),
            rules,
            effects,
        );
    }
    RadioResponse::Roger
}

/// RUN_AWAY, `UnitClass::Receive_Radio` case 0x17 (`0x00737A98..0x00737AF6`):
/// a `Harvester=`/`Weeder=` unit with its unload latch up drops it, scatters
/// (forced, not no-kidding — the Unit Scatter refuses while Unload is still
/// current), queues Harvest and commences it when ready.
fn unit_run_away(
    sim: &mut Simulation,
    unit: u64,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) {
    let Some(rules) = rules else {
        return;
    };
    let latched_harvester = sim.substrate.entities.get(unit).is_some_and(|entity| {
        entity
            .miner
            .as_ref()
            .is_some_and(|miner| miner.unload_active)
            && sim
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.harvester || object.weeder)
    });
    if !latched_harvester {
        return;
    }
    crate::sim::miner::clear_unload_latch(sim, unit);
    if let Err(cause) = sim.scatter(
        unit,
        None,
        ScatterFlags::new(true, false),
        rules,
        None,
        effects,
    ) {
        log::debug!("RUN_AWAY unit {unit} did not scatter: {cause}");
    }
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        unit,
        MissionId::from_known(MissionType::Harvest),
        0,
        now,
        &EntityReadyInputProvider,
    );
    sim.mission_host_promote(unit, now, rules);
}

/// What the aircraft receiver's transport gates read: `Carryall=`
/// (`+0xDFC`), `Passengers=` (`+0x5E0`) and the cargo count (`+0x114`).
struct AircraftHold {
    carryall: bool,
    passengers: i32,
    cargo: i32,
}

/// `AircraftClass::Receive_Radio @ 0x004190B0`. An aircraft on Retreat or a
/// paradrop or Spy Plane mission without an Airstrike (`+0x294`) answers
/// every message 0 (`0x004190B6..0x004190E3`). Its own arms (table
/// `0x0041957C`) are the reload dock's queries (0x1D, 0x1F), MOVE_HERE (0x12),
/// NEED_TO_MOVE (0x13) and RUN_AWAY (0x17). Message 8 takes the Foot path
/// unless the type is `Carryall=` (`+0xDFC`, `0x004194C5`), and 0xE unless
/// the type has `Passengers=` (`+0x5E0`) above its cargo (`+0x114`,
/// `0x00419397`); the rest take the Foot path.
///
/// RESIDUAL: the transport arms are not represented, and no retail aircraft
/// type is a Carryall or has `Passengers=`
/// (`retail_fly_aircraft_take_no_radio_passengers`):
/// - 8 for a Carryall on Move holding a passenger (`0x004194C5..`);
/// - 0xE's admission into a hold with room (`0x004193B7..`);
/// - 0xF (`0x0041946B`), which answers 0 without `Passengers=`, as here;
/// - 0x15 (`0x00419300`) answers 5 for any type. At capacity (every
///   non-transport) it first closes the door (`0x004A5240`), and an Infantry
///   sender of a type with `+0xEC1` and without `+0x6D9` makes the aircraft
///   drop its Target and destination and queue Retreat. No retail sender
///   reaches an aircraft with DOCK_NOW, so VERA answers it 0;
/// - 0x21 (`0x0041918C`), which nothing sends.
fn aircraft_receive(
    sim: &mut Simulation,
    aircraft: u64,
    sender: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let Some(entity) = sim.substrate.entities.get(aircraft) else {
        return RadioResponse::None;
    };
    let airstrike = entity
        .mission_leaf
        .as_aircraft()
        .is_some_and(|leaf| leaf.airstrike_manager_present());
    if !airstrike
        && matches!(
            entity.mission.current().known(),
            Some(
                MissionType::Retreat
                    | MissionType::ParadropApproach
                    | MissionType::ParadropOverfly
                    | MissionType::SpyplaneApproach
                    | MissionType::SpyplaneOverfly
            )
        )
    {
        return RadioResponse::None;
    }
    let ammo = entity
        .aircraft_ammo
        .as_ref()
        .map_or(-1, |ammo| ammo.current);
    let object = rules.and_then(|rules| sim.object_type(entity.type_ref(), rules));
    let type_ammo = object.map_or(-1, |object| object.ammo);
    let transport = AircraftHold {
        carryall: object.is_some_and(|object| object.carryall),
        passengers: object.map_or(0, |object| object.passengers),
        cargo: entity
            .passenger_role
            .cargo()
            .map_or(0, |cargo| cargo.count() as i32),
    };
    let target = entity.attack_target.is_some();
    match msg {
        // 0x00419109: half its type's Ammo or more and a Target, it does not
        // need the round; otherwise Techno takes it (`0x006F4C9C`).
        RadioMessage::Reload if ammo >= type_ammo / 2 && target => RadioResponse::Roger,
        // 0x00419153: ready to leave the dock with no Target and full Ammo.
        RadioMessage::QueryReloaded => {
            if !target && ammo == type_ammo {
                RadioResponse::Roger
            } else {
                RadioResponse::Negatory
            }
        }
        RadioMessage::MoveToCell => {
            aircraft_move_here(sim, aircraft, sender, payload, rules, effects)
        }
        RadioMessage::NeedToMove => {
            // 0x00419274: Foot first, its answer dropped.
            let _ = foot_receive(sim, aircraft, sender, msg, payload, rules, effects);
            aircraft_need_to_move(sim, aircraft, rules)
        }
        RadioMessage::RunAway => {
            // 0x004191AF: Queue(Move), off to the nearest friendly airfield,
            // OVER_OUT to the first contact, then the Foot arm.
            if let Some(rules) = rules {
                let _ = sim.mission_queue_exact(
                    aircraft,
                    MissionId::from_known(MissionType::Move),
                    0,
                    sim.session.binary_frame,
                    &EntityReadyInputProvider,
                );
                let airfield = sim.aircraft_nearest_friendly_airfield_cell(aircraft, rules);
                sim.assign_aircraft_destination(aircraft, Some(airfield), rules, effects);
            }
            crate::sim::radio::transmit_to_contact(
                sim,
                aircraft,
                RadioMessage::Break,
                rules,
                effects,
            );
            foot_receive(sim, aircraft, sender, msg, payload, rules, effects)
        }
        RadioMessage::RequestClearance if !transport.carryall => {
            foot_receive(sim, aircraft, sender, msg, payload, rules, effects)
        }
        RadioMessage::CanDock if transport.passengers <= transport.cargo => {
            foot_receive(sim, aircraft, sender, msg, payload, rules, effects)
        }
        RadioMessage::RequestClearance
        | RadioMessage::CanDock
        | RadioMessage::CanEnter
        | RadioMessage::DockNow => RadioResponse::None,
        _ => foot_receive(sim, aircraft, sender, msg, payload, rules, effects),
    }
}

/// MOVE_HERE, Aircraft case 0x12 (`0x004191F2`): the Foot arm first, its
/// answer dropped; then a dock parameter that does not answer this aircraft's
/// CAN_LOAD ROGER is refused (NEGATORY), one that does queues Enter, and any
/// other parameter queues Move; the class setter takes the parameter and the
/// mission commences (`vt+0x1EC`); ROGER.
fn aircraft_move_here(
    sim: &mut Simulation,
    aircraft: u64,
    sender: Option<u64>,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let _ = foot_receive(
        sim,
        aircraft,
        sender,
        RadioMessage::MoveToCell,
        payload,
        rules,
        effects,
    );
    let Some(rules) = rules else {
        return RadioResponse::None;
    };
    let dock = payload.target.and_then(|target| match target {
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => sim
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category == EntityCategory::Structure)
            .map(|_| id),
        NavTargetRef::Cell { .. } => None,
    });
    let mission = match dock {
        Some(dock) => {
            if transmit(
                sim,
                aircraft,
                dock,
                RadioMessage::CanEnter,
                RadioPayload::default(),
                Some(rules),
                effects,
            ) != RadioResponse::Roger
            {
                return RadioResponse::Negatory;
            }
            MissionType::Enter
        }
        None => MissionType::Move,
    };
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        aircraft,
        MissionId::from_known(mission),
        0,
        now,
        &EntityReadyInputProvider,
    );
    sim.assign_aircraft_destination(aircraft, payload.target, rules, effects);
    let _ = sim.mission_commence_exact(aircraft, now);
    RadioResponse::Roger
}

/// NEED_TO_MOVE, Aircraft case 0x13 after the Foot arm (`0x00419286`): a
/// Fly that is not moving (`Is_Moving`), or one with no NavCom, answers
/// ROGER; one moving toward a NavCom answers NEGATORY unless it is
/// AirportBound and that NavCom is the cell it is in.
fn aircraft_need_to_move(
    sim: &Simulation,
    aircraft: u64,
    rules: Option<&RuleSet>,
) -> RadioResponse {
    let Some(entity) = sim.substrate.entities.get(aircraft) else {
        return RadioResponse::None;
    };
    if !crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false) {
        return RadioResponse::Roger;
    }
    let Some(nav_com) = entity.navigation.nav_com else {
        return RadioResponse::Roger;
    };
    let airport_bound = rules
        .and_then(|rules| sim.object_type(entity.type_ref(), rules))
        .is_some_and(|object| object.airport_bound);
    let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    let own_cell = (location.x / 256, location.y / 256);
    match nav_com {
        NavTargetRef::Cell { rx, ry }
            if airport_bound && (i32::from(rx), i32::from(ry)) == own_cell =>
        {
            RadioResponse::Roger
        }
        _ => RadioResponse::Negatory,
    }
}

/// `FootClass::Receive_Radio @ 0x004D8FB0`.
fn foot_receive(
    sim: &mut Simulation,
    foot: u64,
    sender: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    match msg {
        RadioMessage::MoveToCell => foot_move_here(sim, foot, payload, rules, effects),
        RadioMessage::NeedToMove => foot_need_to_move(sim, foot),
        RadioMessage::RunAway => {
            foot_run_away(sim, foot, rules, effects);
            techno_receive(sim, foot, sender, msg, payload, rules, effects)
        }
        RadioMessage::IsOccupied => foot_is_occupied(sim, foot, sender),
        // Foot4D900E..4D9028: a live NavCom blocks the repair request before
        // the shared Techno cost/heal/payment receiver is reached.
        RadioMessage::RepairTick => {
            if sim
                .substrate
                .entities
                .get(foot)
                .is_some_and(|entity| entity.navigation.nav_com.is_some())
            {
                RadioResponse::Negatory
            } else {
                techno_receive(sim, foot, sender, msg, payload, rules, effects)
            }
        }
        // 0x11 has no represented sender.
        RadioMessage::IsUnitLinked => RadioResponse::None,
        _ => techno_receive(sim, foot, sender, msg, payload, rules, effects),
    }
}

/// MOVE_HERE, Foot case 0x12 (`0x004D9139`): a receiver whose cell is the
/// cell of the parameter's GetCoords (`vt+0x48`) answers ALREADY_THERE.
/// Otherwise a unit on Guard with nothing queued queues Move, a queued Enter
/// commences when ready, the class setter takes the parameter and the mission
/// timer restarts; ROGER.
fn foot_move_here(
    sim: &mut Simulation,
    foot: u64,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let Some(entity) = sim.substrate.entities.get(foot) else {
        return RadioResponse::None;
    };
    // 0x004D913D..0x004D9189: both cells are coordinate / 256, truncated
    // toward zero; the receiver's is its Location's (`vt+0x1B8`).
    let terrain = sim.resolved_terrain.as_ref();
    let target_cell = payload.target.and_then(|target| match target {
        NavTargetRef::Cell { rx, ry } => Some((i32::from(rx), i32::from(ry))),
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => {
            let coords = crate::sim::movement::ground_pose::object_get_coords(
                sim.substrate.entities.get(id)?,
                terrain,
            );
            Some((coords.x / 256, coords.y / 256))
        }
    });
    let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    if target_cell == Some((location.x / 256, location.y / 256)) {
        return RadioResponse::AlreadyThere;
    }
    let Some(rules) = rules else {
        return RadioResponse::None;
    };
    let now = sim.session.binary_frame;
    // 0x004D919A..0x004D91BA.
    if entity.mission.effective() == MissionId::from_known(MissionType::Guard)
        && entity.mission.queued() == MissionId::NONE
    {
        let _ = sim.mission_queue_exact(
            foot,
            MissionId::from_known(MissionType::Move),
            0,
            now,
            &EntityReadyInputProvider,
        );
    }
    // 0x004D91C0..0x004D91DB: queued Enter && Ready_To_Commence → Commence.
    if sim
        .substrate
        .entities
        .get(foot)
        .is_some_and(|e| e.mission.queued() == MissionId::from_known(MissionType::Enter))
    {
        sim.mission_host_promote(foot, now, rules);
    }
    // 0x004D91E1..0x004D91EB: the class setter vt+0x480(*P, 1).
    let _ = sim.assign_destination_represented(foot, payload.target, Some(rules), None, effects);
    // 0x004D91F1..0x004D920D: UpdateTimer (+0xC8) = {Frame, -, 0}. Inside a
    // mission dispatch the MissionClass::AI epilogue overwrites it.
    if let Some(entity) = sim.substrate.entities.get_mut(foot) {
        entity.mission.write_dispatch_epilogue(now as i32, 0);
    }
    RadioResponse::Roger
}

/// RUN_AWAY, Foot case 0x17 (`0x004D902B..0x004D90C6`): a NavCom aimed at
/// the first contact is dropped; a unit asleep queues Guard and commences it
/// when ready, one on Enter queues Guard; one with no NavCom and no turret
/// swing (`+0x6AF`) scatters (forced, no-kidding). The Techno receiver follows.
fn foot_run_away(
    sim: &mut Simulation,
    foot: u64,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) {
    let Some(rules) = rules else {
        return;
    };
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(foot) else {
        return;
    };
    // 0x004D902D..0x004D904F: In_Radio_Contact and NavCom == Contact(0).
    let aimed_at_contact = match (entity.navigation.nav_com, entity.radio_contacts.slot(0)) {
        (
            Some(
                NavTargetRef::Entity { id }
                | NavTargetRef::Object { id }
                | NavTargetRef::Building { id },
            ),
            Some(contact),
        ) => id == contact,
        _ => false,
    };
    if aimed_at_contact {
        sim.assign_null_destination(foot, Some(rules), None, effects);
    }
    let mission = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(foot)
            .map_or(MissionId::NONE, |entity| entity.mission.effective())
    };
    // 0x004D9055..0x004D9082: Sleep → Queue(Guard), Ready → Commence.
    if mission(sim) == MissionId::from_known(MissionType::Sleep) {
        let _ = sim.mission_queue_exact(
            foot,
            MissionId::from_known(MissionType::Guard),
            0,
            now,
            &EntityReadyInputProvider,
        );
        sim.mission_host_promote(foot, now, rules);
    }
    // 0x004D9088..0x004D909F: Enter → Queue(Guard).
    if mission(sim) == MissionId::from_known(MissionType::Enter) {
        let _ = sim.mission_queue_exact(
            foot,
            MissionId::from_known(MissionType::Guard),
            0,
            now,
            &EntityReadyInputProvider,
        );
    }
    // 0x004D90A5..0x004D90C6.
    if sim
        .substrate
        .entities
        .get(foot)
        .is_some_and(|entity| !entity.turret_rotation_latch && entity.navigation.nav_com.is_none())
        && let Err(cause) = sim.scatter(
            foot,
            None,
            ScatterFlags::new(true, true),
            rules,
            None,
            effects,
        )
    {
        log::debug!("RUN_AWAY foot {foot} did not scatter: {cause}");
    }
}

/// NEED_TO_MOVE, Foot case 0x13 (`0x004D90E8`): ROGER with no NavCom or a
/// stopped locomotor, NEGATORY while moving to one. The `*P = NavCom` write
/// has no represented reader (the DOCKING caller overwrites it).
fn foot_need_to_move(sim: &Simulation, foot: u64) -> RadioResponse {
    let Some(entity) = sim.substrate.entities.get(foot) else {
        return RadioResponse::None;
    };
    if entity.navigation.nav_com.is_some()
        && crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false)
    {
        RadioResponse::Negatory
    } else {
        RadioResponse::Roger
    }
}

/// `TechnoClass::Receive_Radio @ 0x006F4AB0`.
fn techno_receive(
    sim: &mut Simulation,
    techno: u64,
    sender: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    match msg {
        RadioMessage::Break => techno_over_out(sim, techno, sender, payload, rules, effects),
        // 0x006F4C6F (7, 9, 0x16): TETHER back to the sender, then the Radio
        // receiver; ROGER.
        RadioMessage::DockingComplete | RadioMessage::TetherBack | RadioMessage::PrepareToDock => {
            if let Some(from) = sender {
                transmit(
                    sim,
                    techno,
                    from,
                    RadioMessage::Tether,
                    RadioPayload::default(),
                    rules,
                    effects,
                );
            }
            let _ = radio_receive(sim, techno, sender, msg);
            RadioResponse::Roger
        }
        // Object5F5339..5382: inherited health query used by depot DOCKING.
        RadioMessage::IsRepairing => {
            let Some((entity, object)) =
                sim.substrate
                    .entities
                    .get(techno)
                    .zip(rules)
                    .and_then(|(entity, rules)| {
                        sim.object_type(entity.type_ref(), rules)
                            .map(|object| (entity, object))
                    })
            else {
                return RadioResponse::None;
            };
            if entity.health.is_full(object.strength) {
                RadioResponse::Negatory
            } else {
                RadioResponse::Roger
            }
        }
        RadioMessage::Tether => techno_tether(sim, techno, sender, rules, effects),
        RadioMessage::Untether => techno_untether(sim, techno, sender, rules, effects),
        // Techno6F4C29: the RECEIVER sends0x19 then0x03 to the original sender.
        // Radio65A970/Techno's existing nested receivers own both endpoints.
        // Source: anytown_damage/unit_unlimbo factory exit radio controls.
        RadioMessage::RequestClearance => {
            if let Some(from) = sender {
                transmit(
                    sim,
                    techno,
                    from,
                    RadioMessage::Untether,
                    RadioPayload::default(),
                    rules,
                    effects,
                );
                // Original6F4C47..6F4C4D returns the nested Break reply.
                transmit(
                    sim,
                    techno,
                    from,
                    RadioMessage::Break,
                    RadioPayload::default(),
                    rules,
                    effects,
                )
            } else {
                // Radio65A970 resolves a null explicit target through slot0
                // on EACH send. Re-read after the nested Untether callback.
                crate::sim::radio::transmit_to_contact(
                    sim,
                    techno,
                    RadioMessage::Untether,
                    rules,
                    effects,
                );
                crate::sim::radio::transmit_to_contact(
                    sim,
                    techno,
                    RadioMessage::Break,
                    rules,
                    effects,
                )
            }
        }
        RadioMessage::RepairTick => techno_repair_tick(sim, techno, rules, effects),
        RadioMessage::Reload => techno_reload(sim, techno, rules),
        // 0x1A, 0x1B and 0x1E have no represented sender; neither has 0x1D
        // to anything but an aircraft, which answers it itself.
        RadioMessage::SecondaryLockSet
        | RadioMessage::SecondaryLockClear
        | RadioMessage::DeploySetNav
        | RadioMessage::QueryReloaded => RadioResponse::None,
        _ => radio_receive(sim, techno, sender, msg),
    }
}

/// RELOAD, Techno case 0x1F (`0x006F4C9C`): Ammo at the type's `Ammo=`
/// answers NEGATORY; otherwise one round more and ROGER.
fn techno_reload(sim: &mut Simulation, techno: u64, rules: Option<&RuleSet>) -> RadioResponse {
    let Some(type_ammo) = rules.and_then(|rules| {
        sim.substrate
            .entities
            .get(techno)
            .and_then(|entity| sim.object_type(entity.type_ref(), rules))
            .map(|object| object.ammo)
    }) else {
        return RadioResponse::None;
    };
    let Some(ammo) = sim
        .substrate
        .entities
        .get_mut(techno)
        .and_then(|entity| entity.aircraft_ammo.as_mut())
    else {
        return RadioResponse::None;
    };
    if ammo.current == type_ammo {
        return RadioResponse::Negatory;
    }
    ammo.current = ammo.current.wrapping_add(1);
    RadioResponse::Roger
}

/// REPAIR_TICK1C, Techno6F4CD7..6F4E3B. The type's shared repair-cost
/// getter7120D0 owns its virtual GetCost; default712120 and Unit747F20 heal
/// getters both read RepairStep. Payment precedes the actual/estimated adds,
/// forced parasite release and damage-smoke retirement, then full-health clamp.
/// Original execution: building_repair.depot_service.json receiver/history rows.
fn techno_repair_tick(
    sim: &mut Simulation,
    techno: u64,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let Some(rules) = rules else {
        return RadioResponse::None;
    };
    let Some(entity) = sim.substrate.entities.get(techno) else {
        return RadioResponse::None;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return RadioResponse::None;
    };
    if entity.health.is_full(object.strength) {
        return RadioResponse::Negatory;
    }
    // InfantryType5247A0/524790 overrides cost0 and Rules+16D8 IRepairStep.
    // That Hospital-family dependency is outside the selected Unit service
    // chain; do not give it the inherited Unit/default getters.
    if entity.category == EntityCategory::Infantry {
        return RadioResponse::None;
    }
    let strength = object.strength;
    let cost = crate::sim::production::repair_step_cost(rules, object);
    let heal = rules.general.repair_step.max(1);
    let owner = entity.owner();
    if crate::sim::credit_income::available_money(sim, owner) < cost {
        return RadioResponse::InsufficientFunds;
    }
    if cost != 0 {
        crate::sim::credit_income::spend_money(sim, owner, cost);
    }
    let Some(entity) = sim.substrate.entities.get_mut(techno) else {
        return RadioResponse::None;
    };
    entity.health.current = entity.health.current.wrapping_add(heal);
    entity.estimated_health.add_repair(heal);
    let eater = entity.parasite_eating_me;
    if let Some(eater) = eater {
        sim.parasite_force_release(
            eater,
            crate::sim::combat::parasite::FORCED_RELEASE_SUPPRESSION_FRAMES,
            rules,
            effects,
        );
    }
    // Radio6F4DAB..6F4DE5 uses the same two-clause health/height gate as
    // SelfHeal6FA75A. Its existing owner retires the retained smoke system.
    sim.retire_damage_smoke_after_self_heal(techno, rules);
    let Some(entity) = sim.substrate.entities.get_mut(techno) else {
        return RadioResponse::None;
    };
    if entity.health.is_full(strength) {
        entity.health.current = strength;
        entity.estimated_health.reset(strength);
        RadioResponse::RepairComplete
    } else {
        RadioResponse::Roger
    }
}

/// OVER_OUT, Techno case 3 (`0x006F4C50`): with both ends tethered it sends
/// UNTETHER to the sender, then the Radio receiver drops the contact; ROGER.
fn techno_over_out(
    sim: &mut Simulation,
    techno: u64,
    sender: Option<u64>,
    _payload: RadioPayload,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    if let Some(from) = sender {
        #[cfg(test)]
        super::record_test_event(super::RadioTestEvent::ReceiverClassEffect {
            receiver_sid: techno,
            sender_sid: from,
        });
        #[cfg(test)]
        sim.trace_lifecycle_for_test(LifecycleTestEvent::BreakReceiverClassEffect {
            target: techno,
        });
        let tethered = |id: u64| {
            sim.substrate
                .entities
                .get(id)
                .is_some_and(|e| e.dock_entered_with.is_some())
        };
        if tethered(techno) && tethered(from) {
            transmit(
                sim,
                techno,
                from,
                RadioMessage::Untether,
                RadioPayload::default(),
                rules,
                effects,
            );
        }
    }
    let _ = radio_receive(sim, techno, sender, RadioMessage::Break);
    RadioResponse::Roger
}

/// TETHER, Techno case 0x18 (`0x006F4B1F`): an `AirportBound=` aircraft
/// (AircraftType+0xE0D) and an already tethered receiver take the default
/// path; otherwise set Techno+0x418 and send TETHER back; ROGER.
fn techno_tether(
    sim: &mut Simulation,
    techno: u64,
    sender: Option<u64>,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let Some(from) = sender else {
        return RadioResponse::None;
    };
    let Some(entity) = sim.substrate.entities.get(techno) else {
        return RadioResponse::None;
    };
    let airport_bound = entity.category == EntityCategory::Aircraft
        && rules
            .and_then(|rules| sim.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.airport_bound);
    if airport_bound || entity.dock_entered_with.is_some() {
        return radio_receive(sim, techno, sender, RadioMessage::Tether);
    }
    if let Some(entity) = sim.substrate.entities.get_mut(techno) {
        entity.dock_entered_with = Some(from);
    }
    transmit(
        sim,
        techno,
        from,
        RadioMessage::Tether,
        RadioPayload::default(),
        rules,
        effects,
    );
    RadioResponse::Roger
}

/// UNTETHER, Techno case 0x19 (`0x006F4B8D`): the mirror of TETHER.
fn techno_untether(
    sim: &mut Simulation,
    techno: u64,
    sender: Option<u64>,
    rules: Option<&RuleSet>,
    effects: FrameEffects<'_>,
) -> RadioResponse {
    let Some(entity) = sim.substrate.entities.get_mut(techno) else {
        return RadioResponse::None;
    };
    if entity.dock_entered_with.is_none() {
        return radio_receive(sim, techno, sender, RadioMessage::Untether);
    }
    entity.dock_entered_with = None;
    if let Some(from) = sender {
        transmit(
            sim,
            techno,
            from,
            RadioMessage::Untether,
            RadioPayload::default(),
            rules,
            effects,
        );
    }
    RadioResponse::Roger
}

/// `RadioClass::Receive_Radio @ 0x0065A820` (HELLO and OVER_OUT); every other
/// message reaches `ObjectClass::Receive_Radio @ 0x005F5320`. Literal13
/// invokes the shared Mark(2) owner and always answers ROGER (`0x005F5370`).
fn radio_receive(
    sim: &mut Simulation,
    receiver: u64,
    sender: Option<u64>,
    msg: RadioMessage,
) -> RadioResponse {
    match msg {
        RadioMessage::Hello => radio_hello(sim, receiver, sender),
        RadioMessage::AnimStop => {
            #[cfg(test)]
            let sender_state = sender
                .and_then(|id| sim.substrate.entities.get(id))
                .map(|entity| {
                    (
                        entity.lifecycle.cell_marked,
                        sim.substrate.occupancy.contains_entity(
                            entity.position.rx,
                            entity.position.ry,
                            entity.stable_id(),
                        ),
                    )
                });
            let accepted = sim.mark_entity_refresh(receiver);
            #[cfg(test)]
            super::record_test_event(super::RadioTestEvent::ObjectMarkRefresh {
                receiver_sid: receiver,
                sender_sid: sender,
                sender_cell_marked: sender_state.map(|state| state.0),
                sender_cell_listed: sender_state.map(|state| state.1),
                accepted,
            });
            let _ = accepted;
            RadioResponse::Roger
        }
        // 0x0065A854..0x0065A8AA: null the first slot holding the sender.
        RadioMessage::Break => {
            let Some(from) = sender else {
                return RadioResponse::None;
            };
            let removed = sim
                .substrate
                .entities
                .get_mut(receiver)
                .and_then(|entity| entity.radio_contacts.remove(from))
                .is_some();
            #[cfg(test)]
            super::record_test_event(super::RadioTestEvent::ReceiverCommonCleared {
                receiver_sid: receiver,
                sender_sid: from,
            });
            #[cfg(test)]
            sim.trace_lifecycle_for_test(LifecycleTestEvent::BreakReceiverCleared {
                target: receiver,
            });
            if removed {
                RadioResponse::Roger
            } else {
                RadioResponse::None
            }
        }
        _ => RadioResponse::None,
    }
}

/// HELLO admission, `0x0065A8AD..0x0065A966`: a dead receiver answers 0; each
/// side must count the other as an ally (`HouseClass::Is_Ally @ 0x004F9A90`,
/// both directions); an existing link answers ROGER; otherwise the first null
/// slot takes the sender, and a saturated receiver answers NEGATORY without
/// evicting.
fn radio_hello(sim: &mut Simulation, receiver: u64, sender: Option<u64>) -> RadioResponse {
    let Some(from) = sender else {
        return RadioResponse::None;
    };
    let (Some(this), Some(other)) = (
        sim.substrate.entities.get(receiver),
        sim.substrate.entities.get(from),
    ) else {
        return RadioResponse::None;
    };
    if this.health.current == 0 {
        return RadioResponse::None;
    }
    let ally = |asker, other| {
        crate::sim::combat::combat_weapon::is_ally_by_object(
            Some(&sim.fog.alliances),
            &sim.interner,
            asker,
            other,
        )
    };
    if !ally(other.owner(), this.owner()) || !ally(this.owner(), other.owner()) {
        return RadioResponse::Negatory;
    }
    let Some(this) = sim.substrate.entities.get_mut(receiver) else {
        return RadioResponse::None;
    };
    match this.radio_contacts.insert(from) {
        Some(_) => RadioResponse::Roger,
        None => RadioResponse::Negatory,
    }
}

/// A tank bunker is any structure seeded with a `bunker_runtime` (Bunker=yes at
/// spawn). Routing on this lets the bus stay rules-free (it has no `RuleSet`).
fn is_bunker_building(sim: &Simulation, sid: u64) -> bool {
    sim.substrate
        .entities
        .get(sid)
        .is_some_and(|b| b.bunker_runtime.is_some())
}

/// Tank-bunker inbound admission + commit. Mirrors the refinery handshake shape:
/// CAN_ENTER is the eligibility query; DOCK_NOW commits the install machine.
fn bunker_receive(
    sim: &mut Simulation,
    bld: u64,
    sender: Option<u64>,
    msg: RadioMessage,
) -> RadioResponse {
    let Some(unit) = sender else {
        return RadioResponse::None;
    };
    match msg {
        RadioMessage::CanEnter => {
            if bunker_admits(sim, bld, unit) {
                RadioResponse::Roger
            } else {
                RadioResponse::Negatory
            }
        }
        RadioMessage::DockNow => {
            // Commit: start the install machine if the bunker is idle.
            if let Some(b) = sim.substrate.entities.get_mut(bld)
                && let Some(rt) = b.bunker_runtime.as_mut()
                && rt.state == BunkerState::Idle
            {
                rt.state = BunkerState::ArriveWait;
                rt.installing_unit = Some(unit);
            }
            RadioResponse::Roger
        }
        RadioMessage::Break => {
            // Bunker reciprocal-link teardown is owned by bunker_link's three
            // verified trigger-specific helpers. Radio BREAK adds no guessed
            // bunker mutation here; the shared receiver tail clears Contacts.
            RadioResponse::None
        }
        _ => RadioResponse::None,
    }
}

/// Sim-state admission gate (no rules): own-owner, alive, not occupied, idle.
/// The rules-gated half (`bunker_link::can_auto_deploy_here`, `0x0070FB50`)
/// runs before the adapter asks (the EnterBunker order and the tank bunker
/// seek).
fn bunker_admits(sim: &Simulation, bld: u64, unit: u64) -> bool {
    let Some(unit) = sim.substrate.entities.get(unit) else {
        return false;
    };
    // CanEnterBunker 0x0070FBAF..0x0070FBC3, called from this receiver at
    // 0x0043C512: a unit infected on its way in is refused at the door.
    if unit.parasite_eating_me.is_some() {
        return false;
    }
    let unit_owner = unit.owner();
    let Some(b) = sim.substrate.entities.get(bld) else {
        return false;
    };
    if b.dying || b.health.current == 0 {
        return false;
    }
    if b.owner() != unit_owner {
        return false;
    }
    if b.bunker_occupant.is_some() {
        return false;
    }
    matches!(b.bunker_runtime.map(|rt| rt.state), Some(BunkerState::Idle))
}

#[cfg(test)]
mod tests {
    use super::dock_pad_cell;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::radio::{
        RadioMessage, RadioPayload, RadioResponse, RadioTestEvent, broadcast_break,
        clear_test_trace, take_test_trace, transmit,
    };
    use crate::sim::world::Simulation;

    fn spawn_refinery(sim: &mut Simulation, sid: u64, owner: &str, capacity: usize) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern("GAREFN");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            10,
            10,
            0,
            0,
            owner_id,
            Health { current: 900 },
            type_id,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.radio_contacts.set_capacity(capacity);
        sim.substrate.entities.insert(ge);
    }

    fn spawn_miner(sim: &mut Simulation, sid: u64, owner: &str) {
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern("HARV");
        let ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            12,
            12,
            0,
            0,
            owner_id,
            Health { current: 200 },
            type_id,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(ge);
    }

    fn spawn_bunker(sim: &mut Simulation, sid: u64, owner: &str) {
        use crate::sim::docking::bunker_install::BunkerRuntime;
        let owner_id = sim.interner.intern(owner);
        let type_id = sim.interner.intern("NATBNK");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            10,
            10,
            0,
            0,
            owner_id,
            Health { current: 1000 },
            type_id,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.bunker_runtime = Some(BunkerRuntime::idle());
        sim.substrate.entities.insert(ge);
    }

    fn hello(sim: &mut Simulation, sender: u64, target: u64) -> RadioResponse {
        transmit(
            sim,
            sender,
            target,
            RadioMessage::Hello,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        )
    }

    fn can_enter(sim: &mut Simulation, sender: u64, target: u64) -> RadioResponse {
        transmit(
            sim,
            sender,
            target,
            RadioMessage::CanEnter,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        )
    }

    fn infantry_output_link(weapons_factory: bool) -> (Simulation, crate::rules::ruleset::RuleSet) {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=125\n\
             [BuildingTypes]\n0=GAPILE\n[GAPILE]\nStrength=500\n\
             WeaponsFactory={weapons_factory}\nHospital=no\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(31);
        spawn_refinery(&mut sim, 2, "Americans", 3);
        let type_ref = sim.intern("GAPILE");
        let building = sim.substrate.entities.get_mut(2).unwrap();
        building.type_ref = type_ref;
        building.lifecycle.in_limbo = false;
        building.lifecycle.cell_marked = true;
        let owner = sim.intern("Americans");
        let type_ref = sim.intern("E1");
        let mut infantry = GameEntity::new_at_frame_zero_for_test(
            1,
            15,
            16,
            0,
            128,
            owner,
            Health { current: 125 },
            type_ref,
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        infantry.lifecycle.in_limbo = false;
        infantry.lifecycle.cell_marked = true;
        infantry.radio_contacts.set_capacity(3);
        sim.substrate.entities.insert(infantry);
        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
        assert_eq!(
            transmit(
                &mut sim,
                1,
                2,
                RadioMessage::Tether,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            ),
            RadioResponse::Roger
        );
        crate::sim::radio::take_transmit_log();
        clear_test_trace();
        (sim, rules)
    }

    /// Original P2 output frames268/485: ExitObject444DC3/444DD9 sends
    /// HELLO2 then9; Foot4D90D9 reaches Techno6F4C6F, then three nested24
    /// sends end with the innermost Radio/Object default0. The jump table
    /// at6F4E88 maps9 to the same arm as7/0x16. This compares the radio
    /// sequence, contacts/tether and unchanged RNG, not the other Exit work.
    /// Native packet: basic-factory-output-prerequisites-research,
    /// rally-consumer-handoff-primary-2.json, SHA00632f4d2610f63a4afe53513bd7441d834d51d8c46ee67480c7fbb17ad893e6.
    #[test]
    fn barracks_output_hello_then_nine_replays_native_reciprocal_tether() {
        let (mut sim, rules) = infantry_output_link(false);
        // Use the same canonical bus to return the reusable linked fixture to
        // the supplied pre-output state: no contacts and no tether.
        assert_eq!(
            transmit(
                &mut sim,
                2,
                1,
                RadioMessage::Break,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default()
            ),
            RadioResponse::Roger
        );
        for id in [1, 2] {
            let entity = sim.substrate.entities.get(id).unwrap();
            assert!(entity.radio_contacts.is_empty());
            assert_eq!(entity.dock_entered_with, None);
        }
        crate::sim::radio::take_transmit_log();
        clear_test_trace();
        let rng = sim.rng_state();
        assert_eq!(
            transmit(
                &mut sim,
                2,
                1,
                RadioMessage::Hello,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default()
            ),
            RadioResponse::Roger
        );
        assert_eq!(
            transmit(
                &mut sim,
                2,
                1,
                RadioMessage::TetherBack,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default()
            ),
            RadioResponse::Roger
        );
        let actual = crate::sim::radio::take_transmit_log()
            .into_iter()
            .map(|row| (row.sender_sid, row.msg, row.target_sid, row.reply))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                (2, 2, 1, Some(1)),
                (2, 9, 1, Some(1)),
                (1, 24, 2, Some(1)),
                (2, 24, 1, Some(1)),
                (1, 24, 2, Some(0)),
            ]
        );
        for (id, peer) in [(1, 2), (2, 1)] {
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(entity.radio_contacts.len(), 1);
            assert!(entity.has_live_contact_with(peer));
            assert_eq!(entity.dock_entered_with, Some(peer));
        }
        assert_eq!(sim.rng_state(), rng);
    }

    /// Original P2 human GAPILE output:51A80C→43CDDD→6F4C29; all three
    /// native RNG objects remain unchanged by the five nested transactions.
    /// Saved executable witness: basic-factory-output-prerequisites-research,
    /// manifest3b42b8512826dc477250a2b894585724db4cb35264770618729c8fd47a083bf5.
    /// Extra sparse contacts check the existing bus's bounded teardown, not a
    /// forced factory clear. The class/type priors and radio replies are native.
    #[test]
    fn infantry_output_clearance_replays_native_nested_release_and_preserves_other_links() {
        let (mut sim, rules) = infantry_output_link(false);
        for id in [1, 2] {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .radio_contacts
                .set_slot(2, 99);
        }
        let rng_before = sim.rng_state();

        assert_eq!(
            transmit(
                &mut sim,
                1,
                2,
                RadioMessage::RequestClearance,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            ),
            RadioResponse::Roger
        );

        let actual = crate::sim::radio::take_transmit_log()
            .into_iter()
            .map(|event| (event.sender_sid, event.msg, event.target_sid, event.reply))
            .collect::<Vec<_>>();
        // Original entry order; nested replies are filled on return.
        assert_eq!(
            actual,
            vec![
                (1, 8, 2, Some(1)),
                (2, 25, 1, Some(1)),
                (1, 25, 2, Some(1)),
                (2, 25, 1, Some(0)),
                (2, 3, 1, Some(1)),
            ]
        );
        for id in [1, 2] {
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(entity.dock_entered_with, None);
            assert_eq!(entity.radio_contacts.slot(0), None);
            assert_eq!(entity.radio_contacts.slot(1), None);
            assert_eq!(entity.radio_contacts.slot(2), Some(99));
        }
        assert_eq!(sim.rng_state(), rng_before);
    }

    /// Building43CDE2 re-reads its type after the common release. Native
    /// WeaponsFactory43CE15 returns0x17; stock GAPILE returns1 instead.
    #[test]
    fn a_weapons_factory_clearance_releases_before_its_literal_0x17_reply() {
        let (mut sim, rules) = infantry_output_link(true);
        assert_eq!(
            transmit(
                &mut sim,
                1,
                2,
                RadioMessage::RequestClearance,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            ),
            RadioResponse::Queued
        );
        let log = crate::sim::radio::take_transmit_log();
        assert_eq!(log[0].reply, Some(0x17));
        assert_eq!(log.len(), 5);
        for id in [1, 2] {
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!(entity.dock_entered_with, None);
            assert!(entity.radio_contacts.is_empty());
        }
    }

    /// Original dirty GAPILE Mark2 witness270a1321...391127 returnsAL0 but
    /// Object5F537A returns radio1, with no lifecycle, raw/list or RNG effects.
    /// Object+80 itself remains unrepresented; this is the bounded dirty arm.
    #[test]
    fn dirty_gapile_mark_refresh_acknowledges_without_a_second_presence_transaction() {
        let (mut sim, rules) = infantry_output_link(false);
        let before = serde_json::to_value(sim.substrate.entities.get(2).unwrap()).unwrap();
        let rng_before = sim.rng_state();
        let lifecycle_before = sim.lifecycle_test_events_for_test().to_vec();
        assert!(!sim.mark_entity_refresh(2));
        assert_eq!(
            transmit(
                &mut sim,
                1,
                2,
                RadioMessage::AnimStop,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            ),
            RadioResponse::Roger
        );
        assert_eq!(
            serde_json::to_value(sim.substrate.entities.get(2).unwrap()).unwrap(),
            before
        );
        assert_eq!(sim.rng_state(), rng_before);
        assert_eq!(sim.lifecycle_test_events_for_test(), lifecycle_before);
        assert_eq!(
            take_test_trace(),
            vec![RadioTestEvent::ObjectMarkRefresh {
                receiver_sid: 2,
                sender_sid: Some(1),
                sender_cell_marked: Some(true),
                sender_cell_listed: Some(false),
                accepted: false,
            }]
        );
        let log = crate::sim::radio::take_transmit_log();
        assert_eq!(log.len(), 1);
        assert_eq!((log[0].msg, log[0].reply), (13, Some(1)));
    }

    ///43CE24's WeaponsFactory gate acknowledges before Object5F5320.
    #[test]
    fn weapons_factory_mark_refresh_bypasses_the_object_receiver() {
        let (mut sim, rules) = infantry_output_link(true);
        assert_eq!(
            transmit(
                &mut sim,
                1,
                2,
                RadioMessage::AnimStop,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            ),
            RadioResponse::Roger
        );
        assert!(take_test_trace().is_empty());
    }

    #[test]
    fn bunker_bus_routes_and_admits_by_sim_state() {
        use crate::sim::docking::bunker_install::BunkerState;
        let mut sim = Simulation::new();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_miner(&mut sim, 1, "Americans"); // own-owner vehicle
        spawn_miner(&mut sim, 3, "Soviets"); // enemy

        // Own-owner, idle, empty → admitted.
        assert_eq!(can_enter(&mut sim, 1, 2), RadioResponse::Roger);
        // Enemy owner → rejected.
        assert_eq!(can_enter(&mut sim, 3, 2), RadioResponse::Negatory);

        // Occupied → rejected.
        sim.substrate.entities.get_mut(2).unwrap().bunker_occupant = Some(99);
        assert_eq!(can_enter(&mut sim, 1, 2), RadioResponse::Negatory);
        sim.substrate.entities.get_mut(2).unwrap().bunker_occupant = None;

        // Installing (non-Idle) → rejected.
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .bunker_runtime
            .as_mut()
            .unwrap()
            .state = BunkerState::ArriveWait;
        assert_eq!(can_enter(&mut sim, 1, 2), RadioResponse::Negatory);
    }

    #[test]
    fn bunker_dock_now_starts_install_machine() {
        use crate::sim::docking::bunker_install::BunkerState;
        let mut sim = Simulation::new();
        spawn_bunker(&mut sim, 2, "Americans");
        spawn_miner(&mut sim, 1, "Americans");
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::DockNow,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let rt = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .bunker_runtime
            .unwrap();
        assert_eq!(rt.state, BunkerState::ArriveWait);
        assert_eq!(rt.installing_unit, Some(1));
    }

    #[test]
    fn refinery_unchanged_when_not_a_bunker() {
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_miner(&mut sim, 1, "Americans");
        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
    }

    #[test]
    fn refinery_full_second_hello_is_negatory_no_evict() {
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_miner(&mut sim, 1, "Americans");
        spawn_miner(&mut sim, 3, "Americans");

        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
        // Capacity-1 receiver denies the second HELLO without evicting Contacts[0].
        assert_eq!(hello(&mut sim, 3, 2), RadioResponse::Negatory);

        let refinery = sim.substrate.entities.get(2).expect("refinery");
        assert!(refinery.radio_contacts.contains(1));
        assert!(!refinery.radio_contacts.contains(3));
    }

    #[test]
    fn enemy_hello_is_negatory() {
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_miner(&mut sim, 1, "Soviets");

        // Owner-mismatch ally gate denies the cross-owner HELLO.
        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Negatory);
        assert!(
            !sim.substrate
                .entities
                .get(2)
                .unwrap()
                .radio_contacts
                .contains(1)
        );
    }

    #[test]
    fn idempotent_hello_re_confirms_roger() {
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_miner(&mut sim, 1, "Americans");

        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().radio_contacts.len(),
            1
        );
    }

    #[test]
    fn enter_dock_sets_flag_and_break_clears_both_sides() {
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_miner(&mut sim, 1, "Americans");

        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Tether,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().dock_entered_with,
            Some(2)
        );

        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().dock_entered_with,
            None
        );
        assert!(
            !sim.substrate
                .entities
                .get(2)
                .unwrap()
                .radio_contacts
                .contains(1)
        );
        assert!(
            !sim.substrate
                .entities
                .get(1)
                .unwrap()
                .radio_contacts
                .contains(2)
        );
    }

    #[test]
    fn dock_pad_is_anchor_plus_three_one() {
        assert_eq!(dock_pad_cell(10, 10), (13, 11));
    }

    /// TETHER on an `AirportBound=` aircraft takes the default path
    /// (`0x006F4B1F`, AircraftType+0xE0D): the landing broadcast leaves its
    /// airfield tethered to it and the aircraft itself untethered.
    #[test]
    fn airport_bound_aircraft_stays_untethered_when_it_lands() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[AircraftTypes]\n0=ORCA\n[BuildingTypes]\n0=GAAIRC\n\
             [ORCA]\nAirportBound=yes\n[GAAIRC]\nHelipad=yes\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        sim.substrate.entities.get_mut(2).unwrap().type_ref = sim.interner.intern("GAAIRC");
        spawn_miner(&mut sim, 1, "Americans");
        {
            let plane = sim.substrate.entities.get_mut(1).unwrap();
            plane.category = EntityCategory::Aircraft;
            plane.type_ref = sim.interner.intern("ORCA");
        }
        for (id, partner) in [(1, 2), (2, 1)] {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .radio_contacts
                .set_slot(0, partner);
        }

        crate::sim::radio::broadcast(
            &mut sim,
            1,
            RadioMessage::Tether,
            Some(&rules),
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(
            sim.substrate.entities.get(1).unwrap().dock_entered_with,
            None
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().dock_entered_with,
            Some(1)
        );
    }

    #[test]
    fn lifecycle_authority_limbo_break_uses_sparse_slot_order() {
        let mut sim = Simulation::new();
        spawn_miner(&mut sim, 1, "Americans");
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_refinery(&mut sim, 3, "Americans", 1);
        spawn_refinery(&mut sim, 4, "Americans", 1);

        let sender = sim.substrate.entities.get_mut(1).unwrap();
        sender.radio_contacts.set_capacity(4);
        assert_eq!(sender.radio_contacts.insert(2), Some(0));
        assert_eq!(sender.radio_contacts.insert(3), Some(1));
        assert_eq!(sender.radio_contacts.insert(4), Some(2));
        assert_eq!(sender.radio_contacts.remove(3), Some(1));
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .radio_contacts
            .insert(1);
        sim.substrate
            .entities
            .get_mut(4)
            .unwrap()
            .radio_contacts
            .insert(1);

        clear_test_trace();
        broadcast_break(
            &mut sim,
            1,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let trace = take_test_trace();

        let reads = trace
            .iter()
            .filter_map(|event| match event {
                RadioTestEvent::BroadcastSlotRead {
                    slot, target_sid, ..
                } => Some((*slot, *target_sid)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reads,
            vec![(0, Some(2)), (1, None), (2, Some(4)), (3, None)]
        );

        let targets = trace
            .iter()
            .filter_map(|event| match event {
                RadioTestEvent::SenderBreakCleared { target_sid, .. } => Some(*target_sid),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(targets, vec![2, 4]);
        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .radio_contacts
                .is_empty()
        );
    }

    #[test]
    fn lifecycle_authority_break_sender_is_clear_before_receiver_effect() {
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_miner(&mut sim, 1, "Americans");
        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Tether,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );

        clear_test_trace();
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(
            take_test_trace(),
            vec![
                RadioTestEvent::SenderBreakCleared {
                    sender_sid: 1,
                    target_sid: 2,
                },
                RadioTestEvent::ReceiverClassEffect {
                    receiver_sid: 2,
                    sender_sid: 1,
                },
                RadioTestEvent::ReceiverCommonCleared {
                    receiver_sid: 2,
                    sender_sid: 1,
                },
            ]
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().dock_entered_with,
            None
        );
        assert!(
            !sim.substrate
                .entities
                .get(2)
                .unwrap()
                .radio_contacts
                .contains(1)
        );
    }

    #[test]
    fn lifecycle_authority_break_receiver_effect_precedes_common_clear() {
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, 2, "Americans", 1);
        spawn_miner(&mut sim, 1, "Americans");
        assert_eq!(hello(&mut sim, 1, 2), RadioResponse::Roger);
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Tether,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );

        clear_test_trace();
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let trace = take_test_trace();

        let effect_index = trace
            .iter()
            .position(|event| matches!(event, RadioTestEvent::ReceiverClassEffect { .. }))
            .expect("represented receiver effect boundary");
        let common_index = trace
            .iter()
            .position(|event| matches!(event, RadioTestEvent::ReceiverCommonCleared { .. }))
            .expect("common receiver clear boundary");
        assert!(effect_index < common_index);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().dock_entered_with,
            None
        );
        assert!(
            !sim.substrate
                .entities
                .get(2)
                .unwrap()
                .radio_contacts
                .contains(1)
        );
    }

    #[test]
    fn lifecycle_authority_break_clears_non_structure_receiver_contact() {
        for category in [
            EntityCategory::Unit,
            EntityCategory::Infantry,
            EntityCategory::Aircraft,
        ] {
            let mut sim = Simulation::new();
            spawn_miner(&mut sim, 1, "Americans");
            spawn_miner(&mut sim, 2, "Americans");
            sim.substrate.entities.get_mut(2).unwrap().category = category;
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .radio_contacts
                .insert(2);
            sim.substrate
                .entities
                .get_mut(2)
                .unwrap()
                .radio_contacts
                .insert(1);

            transmit(
                &mut sim,
                1,
                2,
                RadioMessage::Break,
                RadioPayload::default(),
                None,
                crate::sim::world::FrameEffects::default(),
            );

            assert!(
                !sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .radio_contacts
                    .contains(2)
            );
            assert!(
                !sim.substrate
                    .entities
                    .get(2)
                    .unwrap()
                    .radio_contacts
                    .contains(1)
            );
        }
    }

    #[test]
    fn lifecycle_authority_stale_break_contact_is_idempotent() {
        let mut sim = Simulation::new();
        spawn_miner(&mut sim, 1, "Americans");
        spawn_miner(&mut sim, 2, "Americans");
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .radio_contacts
            .insert(2);

        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .radio_contacts
                .is_empty()
        );
        assert!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .radio_contacts
                .is_empty()
        );
    }

    /// Replays original Unit737430 message0F with the same supplied integer
    /// Size/cargo/alliance and refusal states. Original query execution, not
    /// full construction/boarding or fractional Size parity.
    #[test]
    fn unit_transport_can_enter_matches_original_radio_controls() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::capture_manager::{CaptureManagerState, MindControlLink};
        use crate::sim::passenger::{PassengerCargo, PassengerRole};
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/unit_transport_can_enter.json",
        ))
        .unwrap();
        for row in corpus["rows"].as_array().unwrap() {
            let input = &row["input"];
            let capacity = input["capacity"].as_i64().unwrap();
            let limit = input["size_limit"].as_u64().unwrap();
            let size = input["passenger_size"].as_u64().unwrap();
            let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
                "[VehicleTypes]\n0=TRAN\n[TRAN]\nStrength=100\nPassengers={capacity}\nSizeLimit={limit}\n\
                 [InfantryTypes]\n0=E1\n[E1]\nStrength=100\nSize={size}\n"
            )))
            .unwrap();
            let mut sim = Simulation::with_seed(31);
            let own = sim.intern("Americans");
            let other = sim.intern("Russians");
            if input["allied"].as_bool().unwrap_or(false) {
                sim.fog
                    .alliances
                    .insert("AMERICANS".into(), ["RUSSIANS".into()].into());
            }
            if input["reverse_allied"].as_bool().unwrap_or(false) {
                sim.fog
                    .alliances
                    .insert("RUSSIANS".into(), ["AMERICANS".into()].into());
            }
            let carrier_type = sim.intern("TRAN");
            let mut carrier = GameEntity::new_at_frame_zero_for_test(
                1,
                10,
                10,
                128,
                128,
                own,
                Health { current: 100 },
                carrier_type,
                EntityCategory::Unit,
                0,
                5,
                false,
            );
            let mut cargo = PassengerCargo::new(capacity.max(0) as u32, limit as u32);
            for (index, entry) in input["cargo_sizes"].as_array().unwrap().iter().enumerate() {
                cargo.board_forced(10 + index as u64, entry.as_u64().unwrap() as u32);
            }
            carrier.passenger_role = PassengerRole::Transport { cargo };
            if input["warped"].as_bool().unwrap_or(false) {
                carrier.temporal = crate::sim::temporal::TemporalState::warped_by_for_test(99);
            }
            sim.substrate.entities.insert(carrier);
            let passenger_type = sim.intern("E1");
            let mut passenger = GameEntity::new_at_frame_zero_for_test(
                2,
                10,
                10,
                128,
                128,
                if input["other_house"].as_bool().unwrap_or(false) {
                    other
                } else {
                    own
                },
                Health { current: 100 },
                passenger_type,
                EntityCategory::Infantry,
                0,
                5,
                false,
            );
            if input["controlled"].as_bool().unwrap_or(false) {
                passenger.mind_control = MindControlLink::controlled_by_for_test(99);
            }
            if input["parasite"].as_bool().unwrap_or(false) {
                passenger.parasite_eating_me = Some(99);
            }
            if input["controlling"].as_bool().unwrap_or(false)
                || input["empty_controller"].as_bool().unwrap_or(false)
            {
                passenger.capture_manager = Some(CaptureManagerState::with_victims_for_test(
                    1,
                    false,
                    if input["controlling"].as_bool().unwrap_or(false) {
                        &[99]
                    } else {
                        &[]
                    },
                ));
            }
            sim.substrate.entities.insert(passenger);
            if input["building"].as_bool().unwrap_or(false) {
                sim.substrate.occupancy.add(
                    10,
                    10,
                    99,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    None,
                    crate::sim::occupancy::CellListInsertion::AppendBuilding,
                );
            }
            let before = sim.rng_state();
            let sender = (!input["no_sender"].as_bool().unwrap_or(false)).then_some(2);
            let response = super::receive_radio(
                &mut sim,
                1,
                sender,
                RadioMessage::CanEnter,
                RadioPayload::default(),
                Some(&rules),
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                u64::from(response.code()),
                row["reply"].as_u64().unwrap(),
                "{}",
                input["name"]
            );
            assert_eq!(sim.rng_state(), before, "{}", input["name"]);
        }
    }

    /// The aircraft receiver's transport arms need `Carryall=` (`+0xDFC`) or
    /// `Passengers=` (`+0x5E0`); no retail aircraft type sets either, so 8
    /// and 0xE take the Foot path and the others stay residuals
    /// (`aircraft_receive`).
    #[test]
    fn retail_fly_aircraft_take_no_radio_passengers() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        assert!(!rules.aircraft_ids.is_empty());
        for name in &rules.aircraft_ids {
            let object = rules.object(name).unwrap();
            assert!(!object.carryall, "{name}");
            assert_eq!(object.passengers, 0, "{name}");
        }
    }
}
