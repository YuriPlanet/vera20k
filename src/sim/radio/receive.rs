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
//! - Building OVER_OUT's `Begin_Mode(IDLE)` (`0x00447780`) queues the
//!   building's IDLE BState (+0x538); VERA has no BState owner.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/radio + sim/world. sim/ NEVER depends on
//!   render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::docking::bunker_install::BunkerState;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
#[cfg(test)]
use crate::sim::world::LifecycleTestEvent;
use crate::sim::world::Simulation;

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
pub fn receive_radio(
    sim: &mut Simulation,
    target_sid: u64,
    sender_sid: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
) -> RadioResponse {
    let Some(category) = sim.substrate.entities.get(target_sid).map(|t| t.category) else {
        return RadioResponse::None;
    };
    match category {
        EntityCategory::Structure => {
            building_receive(sim, target_sid, sender_sid, msg, payload, rules)
        }
        EntityCategory::Unit => unit_receive(sim, target_sid, sender_sid, msg, payload, rules),
        EntityCategory::Infantry => foot_receive(sim, target_sid, sender_sid, msg, payload, rules),
        EntityCategory::Aircraft => {
            aircraft_receive(sim, target_sid, sender_sid, msg, payload, rules)
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
) -> RadioResponse {
    // VERA's bunker install adapter owns a Bunker= building's CAN_LOAD and
    // DOCK_NOW (native 0x0043C4F8 / 0x0043C75A arms).
    if is_bunker_building(sim, building)
        && matches!(msg, RadioMessage::CanEnter | RadioMessage::DockNow)
    {
        return bunker_receive(sim, building, sender, msg);
    }
    match msg {
        // 0x0043CD01: Begin_Mode(IDLE) (BState residual, module doc), then
        // the Techno receiver; the answer is always ROGER.
        RadioMessage::Break => {
            let _ = techno_receive(sim, building, sender, msg, payload, rules);
            RadioResponse::Roger
        }
        RadioMessage::CanDock => building_docking(sim, building, sender, rules),
        RadioMessage::DockNow => building_dock_now(sim, building, sender, payload, rules),
        // 8, 0xB, 0xC, 0xD and 0xF (0x0043C2F8; the refinery scan evaluates
        // CAN_LOAD directly) have no represented bus sender.
        RadioMessage::RequestClearance
        | RadioMessage::DockApproach
        | RadioMessage::DockArrived
        | RadioMessage::AnimStop
        | RadioMessage::CanEnter => RadioResponse::None,
        _ => techno_receive(sim, building, sender, msg, payload, rules),
    }
}

/// DOCKING, `BuildingClass::Receive_Radio` case 0x0E (`0x0043C7E9`): HELLO a
/// free sender back and ask it whether it is still moving (0x13). A
/// `DockUnload=`/`Weeder=` dock then sends it to the pad (MOVE_HERE 0x12)
/// and, once it answers that it is already there, tethers it (0x18) and has
/// it turn and report ready (0x16); any other building stops after 0x13.
/// The answer is ROGER unless the dock is offline.
fn building_docking(
    sim: &mut Simulation,
    building_id: u64,
    sender: Option<u64>,
    rules: Option<&RuleSet>,
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
    // The UnitRepair and Bunker arms (0x0043C814..0x0043C87F, and their
    // distance force at 0x0043C93F..0x0043C9F0) and the Helipad MOVE_HERE
    // (0x0043CA2F) belong to docks whose Enter probe VERA runs outside the bus
    // (`building_dock::mission_enter_dispatch`, `bunker_install`, the aircraft
    // landing flow), so no represented sender reaches them here. The
    // Hospital/Armory arm (0x0043C882..0x0043C89E) is dormant: retail
    // RULESMD.INI sets neither key.
    if object.unit_repair || object.bunker || object.helipad {
        return RadioResponse::None;
    }
    let pad_dock = object.dock_unload || object.weeder;
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
        );
    }
    // 0x0043C8D1..0x0043C93A: a contacted Foot whose NavCom is not the
    // GetDockCoord cell (NW+(2,1) for the stock refinery) forces MOVE_HERE.
    let contacted = sim
        .substrate
        .entities
        .get(building_id)
        .is_some_and(|building| building.radio_contacts.contains(from));
    let force = contacted
        && pad_dock
        && sim.substrate.entities.get(from).is_some_and(|foot| {
            foot.category != EntityCategory::Structure
                && foot.navigation.nav_com.is_some()
                && foot.navigation.nav_com
                    != crate::sim::movement::building_dock_cell(
                        &sim.substrate.entities,
                        building_id,
                        Some(from),
                        rules,
                        &sim.interner,
                    )
                    .map(|(x, y)| NavTargetRef::cell(x, y))
        });
    let moving = transmit(
        sim,
        building_id,
        from,
        RadioMessage::NeedToMove,
        RadioPayload::default(),
        Some(rules),
    );
    if moving != RadioResponse::Roger && !force {
        return RadioResponse::Roger;
    }
    // 0x0043CA13..0x0043CA37: only a DockUnload/Weeder dock walks the sender
    // onto its pad.
    if !pad_dock {
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
        RadioPayload { cell: Some(pad) },
        Some(rules),
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
    // 0x0043C732..0x0043C785: the repair docks and Bunker queue their own
    // mission; VERA's depot and bunker flows own those links.
    if object.unit_repair || object.bunker {
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
            let _ = foot_receive(sim, unit, sender, msg, payload, rules);
            RadioResponse::Roger
        }
        RadioMessage::PrepareToDock => unit_prepare_to_dock(sim, unit, sender, payload, rules),
        // 0x00737A98: a harvester mid-unload leaves it, then the Foot arm.
        RadioMessage::RunAway => {
            unit_run_away(sim, unit, rules);
            foot_receive(sim, unit, sender, msg, payload, rules)
        }
        // 7, 0xE, 0xF and 0x15: the transport/service arms have no
        // represented sender.
        RadioMessage::DockingComplete
        | RadioMessage::CanDock
        | RadioMessage::CanEnter
        | RadioMessage::DockNow => RadioResponse::None,
        _ => foot_receive(sim, unit, sender, msg, payload, rules),
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
) -> RadioResponse {
    // 0x007376BA: the Foot receiver first (Techno sends TETHER back).
    let _ = foot_receive(
        sim,
        unit,
        sender,
        RadioMessage::PrepareToDock,
        payload,
        rules,
    );
    let frame = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get_mut(unit) else {
        return RadioResponse::Roger;
    };
    if !entity.turret_rotation_latch {
        let current = entity
            .body_facing
            .as_ref()
            .map_or(u16::from(entity.facing) << 8, |body| body.current(frame));
        if current != DOCK_FACING {
            crate::sim::movement::drive_do_turn(entity, DOCK_FACING, frame);
            return RadioResponse::Roger;
        }
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
        );
    }
    RadioResponse::Roger
}

/// RUN_AWAY, `UnitClass::Receive_Radio` case 0x17 (`0x00737A98..0x00737AF6`):
/// a `Harvester=`/`Weeder=` unit with its unload latch up drops it, scatters
/// (forced, not no-kidding — the Unit Scatter refuses while Unload is still
/// current), queues Harvest and commences it when ready.
fn unit_run_away(sim: &mut Simulation, unit: u64, rules: Option<&RuleSet>) {
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
    scatter(sim, unit, rules);
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

/// `TechnoClass::Scatter` (vt+0x174) with a null source and the force byte,
/// through the shared blocked-cell adapter (its displacement and RNG
/// residuals apply).
fn scatter(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(layer) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.occupancy_list_layer())
    else {
        return;
    };
    let grid = sim.path_grid_snapshot();
    crate::sim::movement::bump_crush::scatter_blocker(
        &mut sim.substrate.entities,
        id,
        grid.as_deref(),
        sim.resolved_terrain.as_ref(),
        &sim.substrate.occupancy,
        layer,
        &mut sim.scenario_rng,
        Some(rules),
        &sim.interner,
        crate::sim::movement::DestinationTiming::from_rules(sim.session.binary_frame, rules.into()),
    );
}

/// `AircraftClass::Receive_Radio @ 0x004190B0`: its own arms for 8, 0xE,
/// 0xF, 0x12, 0x13, 0x15, 0x17, 0x1D, 0x1F and 0x21 are not represented; the
/// rest (TETHER/UNTETHER included, table `0x0041957C`) take the Foot path.
fn aircraft_receive(
    sim: &mut Simulation,
    aircraft: u64,
    sender: Option<u64>,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
) -> RadioResponse {
    match msg {
        RadioMessage::RequestClearance
        | RadioMessage::CanDock
        | RadioMessage::CanEnter
        | RadioMessage::MoveToCell
        | RadioMessage::NeedToMove
        | RadioMessage::DockNow
        | RadioMessage::RunAway
        | RadioMessage::HelipadReserveAck
        | RadioMessage::LinkPassenger => RadioResponse::None,
        _ => foot_receive(sim, aircraft, sender, msg, payload, rules),
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
) -> RadioResponse {
    match msg {
        RadioMessage::MoveToCell => foot_move_here(sim, foot, payload, rules),
        RadioMessage::NeedToMove => foot_need_to_move(sim, foot),
        RadioMessage::RunAway => {
            foot_run_away(sim, foot, rules);
            techno_receive(sim, foot, sender, msg, payload, rules)
        }
        // 0x11, 0x1C and 0x23 have no represented sender.
        RadioMessage::IsUnitLinked | RadioMessage::RepairTick | RadioMessage::IsOccupied => {
            RadioResponse::None
        }
        _ => techno_receive(sim, foot, sender, msg, payload, rules),
    }
}

/// MOVE_HERE, Foot case 0x12 (`0x004D9139`): already in the payload cell →
/// ALREADY_THERE. Otherwise a unit on Guard with nothing queued queues Move,
/// a queued Enter commences when ready, the class setter takes the cell and
/// the mission timer restarts; ROGER.
fn foot_move_here(
    sim: &mut Simulation,
    foot: u64,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
) -> RadioResponse {
    let Some(entity) = sim.substrate.entities.get(foot) else {
        return RadioResponse::None;
    };
    if payload.cell == Some((entity.position.rx, entity.position.ry)) {
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
    match payload.cell {
        Some(cell) => {
            sim.set_unit_cell_destination(foot, cell, rules);
        }
        None => {
            sim.assign_null_destination(foot, Some(rules));
        }
    }
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
fn foot_run_away(sim: &mut Simulation, foot: u64, rules: Option<&RuleSet>) {
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
        sim.assign_null_destination(foot, Some(rules));
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
    {
        scatter(sim, foot, rules);
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
) -> RadioResponse {
    match msg {
        RadioMessage::Break => techno_over_out(sim, techno, sender, payload, rules),
        // 0x006F4C6F (7, 9, 0x16): TETHER back to the sender, then the Radio
        // receiver; ROGER.
        RadioMessage::DockingComplete | RadioMessage::PrepareToDock => {
            if let Some(from) = sender {
                transmit(
                    sim,
                    techno,
                    from,
                    RadioMessage::Tether,
                    RadioPayload::default(),
                    rules,
                );
            }
            let _ = radio_receive(sim, techno, sender, msg);
            RadioResponse::Roger
        }
        RadioMessage::Tether => techno_tether(sim, techno, sender, rules),
        RadioMessage::Untether => techno_untether(sim, techno, sender, rules),
        // 8, 0x1A..0x1C, 0x1E and 0x1F have no represented sender.
        RadioMessage::RequestClearance
        | RadioMessage::SecondaryLockSet
        | RadioMessage::SecondaryLockClear
        | RadioMessage::RepairTick
        | RadioMessage::DeploySetNav
        | RadioMessage::LinkPassenger => RadioResponse::None,
        _ => radio_receive(sim, techno, sender, msg),
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
    );
    RadioResponse::Roger
}

/// UNTETHER, Techno case 0x19 (`0x006F4B8D`): the mirror of TETHER.
fn techno_untether(
    sim: &mut Simulation,
    techno: u64,
    sender: Option<u64>,
    rules: Option<&RuleSet>,
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
        );
    }
    RadioResponse::Roger
}

/// `RadioClass::Receive_Radio @ 0x0065A820` (HELLO and OVER_OUT); every other
/// message reaches `ObjectClass::Receive_Radio @ 0x005F5320`, which answers 0
/// for everything VERA sends over the bus.
fn radio_receive(
    sim: &mut Simulation,
    receiver: u64,
    sender: Option<u64>,
    msg: RadioMessage,
) -> RadioResponse {
    match msg {
        RadioMessage::Hello => radio_hello(sim, receiver, sender),
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
            if let Some(b) = sim.substrate.entities.get_mut(bld) {
                if let Some(rt) = b.bunker_runtime.as_mut() {
                    if rt.state == BunkerState::Idle {
                        rt.state = BunkerState::ArriveWait;
                        rt.installing_unit = Some(unit);
                    }
                }
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
/// The rules-gated Bunkerable+weapon check runs at command time (EnterBunker).
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
        )
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

        crate::sim::radio::broadcast(&mut sim, 1, RadioMessage::Tether, Some(&rules));

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
        broadcast_break(&mut sim, 1, None);
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
        );

        clear_test_trace();
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
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
        );

        clear_test_trace();
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
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
        );
        transmit(
            &mut sim,
            1,
            2,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
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
}
