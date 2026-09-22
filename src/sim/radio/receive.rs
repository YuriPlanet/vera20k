//! Per-category `receive_radio` handlers — the receiver half of the radio bus.
//!
//! Each handler runs inline inside the sender's [`crate::sim::radio::transmit`]
//! call (synchronous RPC, no queue) and commits its side effects in native
//! order. This slice implements the zero-link refinery inbound idiom: HELLO
//! admission, the CAN_DOCK accepted-cell reply, the ENTER/LEAVE dock-entered
//! flag, and BREAK teardown. BREAK's base receiver-contact clear is shared by
//! every represented Techno category; unrepresented class-specific handlers
//! remain explicit gaps rather than being guessed here.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/radio + sim/world. sim/ NEVER depends on
//!   render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::sim::docking::bunker_install::BunkerState;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse};
#[cfg(test)]
use crate::sim::world::LifecycleTestEvent;
use crate::sim::world::Simulation;

/// CAN_DOCK accepted-cell offset from the refinery NW anchor: anchor + (3, 1).
/// Verified: `BuildingClass::Receive_Radio` case 0x0E hardcodes this and does
/// not read art.ini `QueueingCell=`.
pub const REFINERY_ACCEPTED_DX: u16 = 3;
/// See [`REFINERY_ACCEPTED_DX`].
pub const REFINERY_ACCEPTED_DY: u16 = 1;

/// The CAN_DOCK accepted cell for a refinery at NW anchor `(rx, ry)`.
pub fn refinery_accepted_cell(rx: u16, ry: u16) -> (u16, u16) {
    (
        rx.saturating_add(REFINERY_ACCEPTED_DX),
        ry.saturating_add(REFINERY_ACCEPTED_DY),
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
) -> RadioResponse {
    let _ = payload;
    let category = match sim.substrate.entities.get(target_sid) {
        Some(target) => target.category,
        None => return RadioResponse::None,
    };
    let response = match category {
        EntityCategory::Structure => {
            // TODO(BLOCKED parity): BuildingClass runs GrandOpening before its
            // Techno/base BREAK tail. No exact represented animation authority
            // exists here yet, so do not approximate that effect.
            if is_bunker_building(sim, target_sid) {
                bunker_receive(sim, target_sid, sender_sid, msg)
            } else {
                refinery_receive(sim, target_sid, sender_sid, msg)
            }
        }
        // Unit/Infantry/Aircraft have no represented class-specific BREAK work
        // in this slice, but still reach the common RadioClass receiver tail.
        _ => RadioResponse::None,
    };

    if msg == RadioMessage::Break {
        if let Some(sender_sid) = sender_sid {
            #[cfg(test)]
            super::record_test_event(super::RadioTestEvent::ReceiverClassEffect {
                receiver_sid: target_sid,
                sender_sid,
            });
            #[cfg(test)]
            sim.trace_lifecycle_for_test(LifecycleTestEvent::BreakReceiverClassEffect {
                target: target_sid,
            });

            clear_receiver_break_contact(sim, target_sid, sender_sid);

            #[cfg(test)]
            super::record_test_event(super::RadioTestEvent::ReceiverCommonCleared {
                receiver_sid: target_sid,
                sender_sid,
            });
            #[cfg(test)]
            sim.trace_lifecycle_for_test(LifecycleTestEvent::BreakReceiverCleared {
                target: target_sid,
            });
        }
    }

    response
}

/// Common `RadioClass::Receive_Radio(BREAK)` tail. Class-specific receiver work
/// has already run; this only nulls the first matching receiver slot in place.
fn clear_receiver_break_contact(sim: &mut Simulation, receiver_sid: u64, sender_sid: u64) {
    if let Some(receiver) = sim.substrate.entities.get_mut(receiver_sid) {
        receiver.radio_contacts.remove(sender_sid);
    }
}

/// Zero-link refinery inbound dock handler (`unit+0x2E4 == 0` branch).
fn refinery_receive(
    sim: &mut Simulation,
    ref_sid: u64,
    sender_sid: Option<u64>,
    msg: RadioMessage,
) -> RadioResponse {
    let Some(miner_sid) = sender_sid else {
        return RadioResponse::None;
    };
    match msg {
        RadioMessage::Hello => refinery_hello(sim, ref_sid, miner_sid),
        RadioMessage::CanDock => {
            // The accepted cell is anchor + (3, 1); the caller reads it from
            // `refinery_accepted_cell`. The bus reply is the CELL_ACCEPTED ack.
            RadioResponse::CellAccepted
        }
        RadioMessage::EnterDock => {
            // 0x18 sets the dock-entered flag on the entering miner.
            if let Some(miner) = sim.substrate.entities.get_mut(miner_sid) {
                miner.dock_entered_with = Some(ref_sid);
            }
            RadioResponse::Roger
        }
        RadioMessage::LeaveDock => {
            // 0x19 clears it.
            if let Some(miner) = sim.substrate.entities.get_mut(miner_sid) {
                if miner.dock_entered_with == Some(ref_sid) {
                    miner.dock_entered_with = None;
                }
            }
            RadioResponse::Roger
        }
        RadioMessage::Break => {
            refinery_break_effect(sim, ref_sid, miner_sid);
            RadioResponse::None
        }
        RadioMessage::IsOccupied => {
            let occupied = sim
                .substrate
                .entities
                .get(ref_sid)
                .is_some_and(|r| !r.radio_contacts.is_empty());
            if occupied {
                RadioResponse::Roger
            } else {
                RadioResponse::None
            }
        }
        // MOVE_TO_CELL/TIMING_SYNC/DOCK_NOW are choreography the miner FSM still
        // drives via direct moves/facing in this slice; ack them so a future
        // caller can route them through the bus without changing behavior.
        RadioMessage::MoveToCell | RadioMessage::TimingSync | RadioMessage::DockNow => {
            RadioResponse::Roger
        }
        _ => RadioResponse::None,
    }
}

/// HELLO receiver-side admission (§5.2.3): alive gate → owner ally gate →
/// idempotent → first-free insert (no eviction) ⇒ ROGER, else NEGATORY.
fn refinery_hello(sim: &mut Simulation, ref_sid: u64, miner_sid: u64) -> RadioResponse {
    // Ally gate = owner equality (no ally graph in sim/ yet; stock skirmish docks
    // at the own-owner refinery only — swap for `is_ally()` when it lands).
    let miner_owner = match sim.substrate.entities.get(miner_sid) {
        Some(m) => m.owner(),
        None => return RadioResponse::None,
    };
    let Some(refinery) = sim.substrate.entities.get_mut(ref_sid) else {
        return RadioResponse::None;
    };
    if refinery.dying || refinery.health.current == 0 {
        return RadioResponse::None;
    }
    if refinery.owner() != miner_owner {
        return RadioResponse::Negatory;
    }
    // Idempotent (already linked ⇒ ROGER) is folded into `insert`. A saturated
    // receiver returns `None` and never evicts — the dock idiom (V3).
    match refinery.radio_contacts.insert(miner_sid) {
        Some(_) => RadioResponse::Roger,
        None => RadioResponse::Negatory,
    }
}

/// Represented refinery-specific BREAK work, before the common receiver clear.
///
/// This preserves the existing one-sided Rust dock-entered projection. It is
/// not the native conditional two-sided `0x19` transmit cascade, which remains
/// blocked until both `Techno+0x418` bytes have exact represented ownership.
fn refinery_break_effect(sim: &mut Simulation, ref_sid: u64, miner_sid: u64) {
    if let Some(miner) = sim.substrate.entities.get_mut(miner_sid) {
        if miner.dock_entered_with == Some(ref_sid) {
            miner.dock_entered_with = None;
        }
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
    use super::refinery_accepted_cell;
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
        )
    }

    fn can_enter(sim: &mut Simulation, sender: u64, target: u64) -> RadioResponse {
        transmit(
            sim,
            sender,
            target,
            RadioMessage::CanEnter,
            RadioPayload::default(),
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
            RadioMessage::EnterDock,
            RadioPayload::default(),
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().dock_entered_with,
            Some(2)
        );

        transmit(&mut sim, 1, 2, RadioMessage::Break, RadioPayload::default());
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
    fn accepted_cell_is_anchor_plus_three_one() {
        assert_eq!(refinery_accepted_cell(10, 10), (13, 11));
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
        broadcast_break(&mut sim, 1);
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
            RadioMessage::EnterDock,
            RadioPayload::default(),
        );

        clear_test_trace();
        transmit(&mut sim, 1, 2, RadioMessage::Break, RadioPayload::default());

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
            RadioMessage::EnterDock,
            RadioPayload::default(),
        );

        clear_test_trace();
        transmit(&mut sim, 1, 2, RadioMessage::Break, RadioPayload::default());
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

            transmit(&mut sim, 1, 2, RadioMessage::Break, RadioPayload::default());

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

        transmit(&mut sim, 1, 2, RadioMessage::Break, RadioPayload::default());
        transmit(&mut sim, 1, 2, RadioMessage::Break, RadioPayload::default());

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
