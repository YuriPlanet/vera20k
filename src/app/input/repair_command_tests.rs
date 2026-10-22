//! EventCtor4C65E0 owns the accepted repair sender/frame and five-byte token.
use super::schedule_command_in_sim;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

#[test]
fn admitted_repair_enters_the_native_packet_queue_with_its_saved_sender() {
    let mut sim = Simulation::new();
    let other = sim.intern("Other");
    let sender = sim.intern("Sender");
    sim.houses_mut_for_test()
        .register(HouseState::new(other, 0, None, true, 0, 10));
    sim.houses_mut_for_test()
        .register(HouseState::new(sender, 0, None, true, 0, 10));
    sim.session_mut_for_test().set_tick_for_test(123);
    sim.session_mut_for_test().set_binary_frame_for_test(77);
    // The click's ownership/CanRepair admission precedes this scheduler boundary.
    assert_eq!(
        schedule_command_in_sim(&mut sim, "Sender", Command::ToggleRepair { entity_id: 42 }),
        Some(123)
    );
    let queues = sim.network_command_queues();
    let records = queues
        .local_records()
        .expect("accepted repair needs a native wire producer");
    assert_eq!(records.len(), 1);
    let record = records[0];
    assert_eq!(
        (record.opcode(), record.house_id(), record.frame_stamp()),
        (0x15, 1, 77)
    );
    assert_eq!(&record.payload()[..5], &[42, 0, 0, 0, 0x34]);
}
