//! Building docking system — repair depot (`UnitRepair=yes`) admission,
//! waiting, service and release.
//!
//! Native shape (gamemd.exe, all read this task):
//!
//! - **Admission is contact-slot capacity, no stored queue.** The depot's
//!   `RadioClass` contact array is sized once at construction:
//!   `BuildingClass::Constructor` 0x0043BCBD reads `Type+0x1780`
//!   (`NumberOfDocks=`), clamps `< 1 ⇒ 1` and calls `Set_Contact_Count`
//!   (0x0043BCD0). Nothing in the binary keeps a waiting list.
//! - **The order admits one unit; the rest pend.** The player's Enter order
//!   runs `UnitClass::Set_Destination` 0x00741970 with the depot
//!   ([`Simulation::set_unit_destination`]): a free depot is sent DOCKING and HELLOs the unit
//!   into its slot, and the unit drives for the pad; a depot holding a contact
//!   becomes a damaged unit's pending entry (`Unit+0x500`, 0x00741D9F) with no
//!   destination. A pending unit's Enter dispatch parks it beside the depot
//!   (`0x0070D8F0`), and its `FootClass::AI` asks HELLO then CAN_LOAD every
//!   frame (`0x0070D7E0`, [`try_pending_entry`]): whichever pending unit's AI
//!   runs first after the slot frees takes it and drives for the pad. No
//!   unit without the slot is sent to the pad.
//! - **A contact re-probes on its Enter cadence.** `FootClass::Mission_Enter`
//!   0x004D9290 sends `0x0E` to `Contacts[0]` (or to the archive target
//!   `+0x218`, 0x004D929F) on every dispatch, then re-arms
//!   `ftol([current mission] Rate * 900) + RandomRanged(0, 2)` (0x004D946C..0x004D9497,
//!   Scenario stream).
//! - **`BuildingClass::Receive_Radio(0x0E)` 0x0043C2D0 for a UnitRepair
//!   building** (0x0043C7E9..): `+0x660` power flag off ⇒ 10; already linked
//!   AND `Transmit(0x22)` == 10 (0x0043C824..C842) ⇒ 10; the Hospital/Armory
//!   branch (`Type+0x16C1/+0x16C2`, 0x0043CB0C) is NOT taken, so a depot never
//!   evicts a repaired occupant on a probe; not linked and
//!   `Has_Free_Or_Own_Contact_Slot` 0x0065ADF0 ⇒ the building HELLOs the sender
//!   (0x0043C8B0..C8C3); then `0x13` and return 1 (0x0043C9F5..CA37). There
//!   is no `0x12` move assignment for depots — the unit's own NavCom (the
//!   depot, whose `+0x4C` is the pad) drives it.
//! - **`ObjectClass::Receive_Radio(0x22)` 0x005F5320**: health ratio ≥
//!   `Rules+0x16F8` ⇒ 10 else 1. `Rules+0x16F8` is not an INI key:
//!   `RulesClass::ReadAudioVisual` 0x0066B323/0x0066B32D stores the double 1.0
//!   unconditionally. Completion tests ordered ratio >= 1, including signed
//!   Strength and masked division by zero; unordered does not complete.
//! - **Reply 10 to the waiter**: `Mission_Enter` 0x004D92D0 sends BREAK and
//!   calls `Enter_Idle_Mode` (+0x484 = 0x00738970). BREAK preserves NavCom,
//!   so an installed destination selects Move before the harvester arm.
//!   Without NavCom, plain units select Guard; Harvester=yes units select
//!   Harvest unless a human owner stands off ore.
//! - **Release of a repaired occupant** is the building's repair mission
//!   (`BuildingClass::MissionRepairAndProduce` 0x0044B780, 0x0044C2AE..C4B0 and
//!   0x0044BD5E..). A first paid step, even one reaching full health, starts
//!   the independent service progress. Its next repair request releases on
//!   replies other than Roger or InsufficientFunds. The occupant gets
//!   `Queue_Mission(Move)`, `Set_Destination(exit cell)` and BREAK, and its
//!   pending entry is cleared. State1 clears ArchiveTarget after a valid
//!   release; state2 clears it only when returning an AI unit to its archive.
//!   The exit cell is the depot's own rally point (its
//!   ArchiveTarget, 0x0044C40D..C454) when set, else
//!   `BuildingClass::GetDockCellForObject` 0x0044EFB0: the foundation exit list
//!   (`Type+0xED4`, initializer 0x0045C300) walked in order until the unit can
//!   enter the cell. Scatter (`FootClass::Receive_Radio 0x17`) is only reached
//!   from the Hospital/Armory loop and is not part of the depot flow.
//!
//! Repair payment/heal is the canonical Techno radio receiver6F4AB0.
//! The Building mission44B780 owns status and its independent+620 progress.
//! Evidence and coverage: tools/spatial_oracle/building_repair.depot_service.md.
//! Marked waiter/near-stop controls: building_repair.depot_waiters.{json,md}.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/radio, sim/movement, sim/mission.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{SimSoundEvent, Simulation};

///Type+ED4: original45C300 startup's22 terminated rows at89D368+id*78.
///The row order is native data, including refinery/special foundations that
///are not rectangular rings. Both depot exit scanning and factory Unload use
///this owner. Native execution: anytown_damage/unit_unlimbo foundation rows.
pub fn foundation_exit_list(foundation: &str) -> Vec<(i32, i32)> {
    FOUNDATION_EXIT_ROWS[usize::from(crate::rules::foundation::foundation_def(foundation).id)]
        .iter()
        .copied()
        .take_while(|&pair| pair != (0x7FFF, 0x7FFF))
        .collect()
}

///Read an actual retained pair, including sentinel/padding. Factory Unload
///reads element10 directly rather than walking the terminated exit list.
pub(crate) fn foundation_exit_pair(foundation: &str, index: usize) -> Option<(i32, i32)> {
    FOUNDATION_EXIT_ROWS[usize::from(crate::rules::foundation::foundation_def(foundation).id)]
        .get(index)
        .copied()
}

const FOUNDATION_EXIT_ROWS: [[(i32, i32); 30]; 22] = [
    [
        (0, 1),
        (-1, 1),
        (1, 1),
        (-1, 0),
        (1, 0),
        (0, -1),
        (-1, -1),
        (1, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //0: 0x89d368
    [
        (0, 1),
        (1, 1),
        (-1, 1),
        (2, 1),
        (-1, 0),
        (2, 0),
        (0, -1),
        (1, -1),
        (-1, -1),
        (2, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //1: 0x89d3e0
    [
        (0, 2),
        (1, 2),
        (-1, 2),
        (-1, 1),
        (1, 1),
        (-1, 0),
        (1, 0),
        (0, -1),
        (-1, -1),
        (1, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //2: 0x89d458
    [
        (0, 2),
        (1, 2),
        (-1, 2),
        (2, 2),
        (-1, 1),
        (2, 1),
        (-1, 0),
        (2, 0),
        (0, -1),
        (1, -1),
        (-1, -1),
        (2, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //3: 0x89d4d0
    [
        (0, 3),
        (1, 3),
        (-1, 3),
        (2, 3),
        (-1, 2),
        (2, 2),
        (-1, 1),
        (2, 1),
        (-1, 0),
        (2, 0),
        (0, -1),
        (1, -1),
        (-1, -1),
        (2, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //4: 0x89d548
    [
        (0, 2),
        (1, 2),
        (2, 2),
        (-1, 2),
        (3, 2),
        (-1, 1),
        (3, 1),
        (-1, 0),
        (3, 0),
        (0, -1),
        (1, -1),
        (2, -1),
        (-1, -1),
        (3, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //5: 0x89d5c0
    [
        (0, 3),
        (1, 3),
        (2, 3),
        (-1, 3),
        (3, 3),
        (-1, 2),
        (3, 2),
        (-1, 1),
        (3, 1),
        (-1, 0),
        (3, 0),
        (0, -1),
        (1, -1),
        (2, -1),
        (-1, -1),
        (3, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //6: 0x89d638
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (2, -1),
        (3, -1),
        (-1, 0),
        (3, 0),
        (-1, 1),
        (3, 1),
        (-1, 2),
        (3, 2),
        (-1, 3),
        (3, 3),
        (-1, 4),
        (3, 4),
        (-1, 5),
        (0, 5),
        (1, 5),
        (2, 5),
        (3, 5),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //7: 0x89d6b0
    [
        (0, 2),
        (1, 2),
        (2, 2),
        (3, 2),
        (-1, 2),
        (4, 2),
        (-1, 1),
        (4, 1),
        (-1, 0),
        (4, 0),
        (0, -1),
        (1, -1),
        (2, -1),
        (3, -1),
        (-1, -1),
        (4, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //8: 0x89d728
    [
        (0, 0),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //9: 0x89d7a0
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (1, 1),
        (-1, 2),
        (1, 2),
        (-1, 3),
        (0, 3),
        (1, 3),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //10: 0x89d818
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (2, -1),
        (3, -1),
        (-1, 0),
        (3, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
        (2, 1),
        (3, 1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //11: 0x89d890
    [
        (0, 3),
        (1, 3),
        (2, 3),
        (-1, 3),
        (3, 3),
        (-1, 2),
        (3, 2),
        (-1, 1),
        (3, 1),
        (-1, 0),
        (3, 0),
        (0, -1),
        (1, -1),
        (2, -1),
        (-1, -1),
        (3, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //12: 0x89d908
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (1, 1),
        (-1, 2),
        (1, 2),
        (-1, 3),
        (1, 3),
        (-1, 4),
        (0, 4),
        (1, 4),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //13: 0x89d980
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (1, 1),
        (-1, 2),
        (1, 2),
        (-1, 3),
        (1, 3),
        (-1, 4),
        (1, 4),
        (-1, 5),
        (0, 5),
        (1, 5),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //14: 0x89d9f8
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (2, -1),
        (-1, 0),
        (2, 0),
        (-1, 1),
        (2, 1),
        (-1, 2),
        (2, 2),
        (-1, 3),
        (2, 3),
        (-1, 5),
        (2, 5),
        (-1, 6),
        (0, 6),
        (1, 6),
        (2, 6),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //15: 0x89da70
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (2, -1),
        (-1, 0),
        (2, 0),
        (-1, 1),
        (2, 1),
        (-1, 2),
        (2, 2),
        (-1, 3),
        (2, 3),
        (-1, 5),
        (0, 5),
        (1, 5),
        (2, 5),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //16: 0x89dae8
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (2, -1),
        (3, -1),
        (4, -1),
        (5, -1),
        (-1, 0),
        (5, 0),
        (-1, 1),
        (5, 1),
        (-1, 2),
        (5, 2),
        (-1, 3),
        (0, 3),
        (1, 3),
        (2, 3),
        (3, 3),
        (4, 3),
        (5, 3),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //17: 0x89db60
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (2, -1),
        (3, -1),
        (4, -1),
        (-1, 4),
        (0, 4),
        (1, 4),
        (2, 4),
        (3, 4),
        (4, 4),
        (-1, 0),
        (-1, 1),
        (-1, 2),
        (-1, 3),
        (-1, 4),
        (4, 0),
        (4, 1),
        (4, 2),
        (4, 3),
        (4, 4),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //18: 0x89dbd8
    [
        (0, 4),
        (1, 4),
        (2, 4),
        (-1, 4),
        (3, 4),
        (-1, 2),
        (3, 2),
        (-1, 1),
        (3, 1),
        (-1, 0),
        (3, 0),
        (0, -1),
        (1, -1),
        (2, -1),
        (-1, -1),
        (3, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //19: 0x89dc50
    [
        (2, -1),
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //20: 0x89dcc8
    [
        (32767, 32767),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ], //21: 0x89dd40
];

/// `BuildingClass::GetDockCellForObject @ 0x0044EFB0`: preferred flag cells,
/// naval dock cells, offered cell, then the saved foundation row or Hospital
/// scan. The actual producer and mover supply class facts; exact-zero class
/// CanEnter and native map bounds admit a result, including an owned footprint.
/// Native execution: basic-factory-exit-geometry-research, manifest
/// eb5727a3be64ab2a0b002e2516180432c1ef783b6fc1061a7c0101d3e81fe59f.
/// Parsed types have a saved row; native pre-ReadINI null-table objects have
/// no registered counterpart here. Hospital uses the original generic scan.
pub(crate) fn building_dock_cell(
    sim: &Simulation,
    producer_id: u64,
    mover_id: u64,
    offered: (i16, i16),
    rules: &RuleSet,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<(u16, u16)> {
    use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
    let producer = sim.substrate.entities.get(producer_id)?;
    let object = sim.object_type(producer.type_ref(), rules)?;
    let location =
        crate::sim::movement::ground_pose::object_location(producer, sim.resolved_terrain.as_ref());
    let nw = (
        crate::util::lepton::lepton_to_cell_packed(location.x),
        crate::util::lepton::lepton_to_cell_packed(location.y),
    );
    let relative = |dx: i16, dy: i16| (nw.0.wrapping_add(dx), nw.1.wrapping_add(dy));
    let admits = |cell, mode| {
        sim.map_cell_in_bounds(cell)
            && sim
                .mover_can_enter(
                    mover_id,
                    cell,
                    InfantryEntryArgs::REPAIR,
                    mode,
                    rules,
                    registry,
                )
                .is_ok_and(|code| code == 0)
    };
    let published = |cell: (i16, i16)| (cell.0 as u16, cell.1 as u16);

    //44EFD8/44F051/44F0CA: flags alone choose the preferred cell; there is
    //no Infantry RTTI condition. A refused preferred cell tries the next flag.
    for (flag, dx, dy) in [
        (object.gdi_barracks(), 1, 2),
        (object.nod_barracks(), 2, 2),
        (object.yuri_barracks(), 2, 1),
    ] {
        let cell = relative(dx, dy);
        if flag && admits(cell, EntryQueryMode::CheckLocomotor) {
            return Some(published(cell));
        }
    }
    //44F141..44F29F calls the existing Building447B20 coordinate owner.
    if object.naval
        && object.weapons_factory
        && let Some((x, y)) = crate::sim::movement::building_dock_cell(
            &sim.substrate.entities,
            producer_id,
            Some(mover_id),
            sim.resolved_terrain.as_ref(),
            rules,
            &sim.interner,
        )
    {
        let (x, y) = (x as i16, y as i16);
        for cell in [
            (x.wrapping_add(1), y.wrapping_add(1)),
            (x.wrapping_add(1), y),
            (x, y.wrapping_add(1)),
        ] {
            if admits(cell, EntryQueryMode::SkipLocomotor) {
                return Some(published(cell));
            }
        }
    }
    if offered != (0, 0) && admits(offered, EntryQueryMode::SkipLocomotor) {
        return Some(published(offered));
    }
    if !object.hospital {
        return foundation_exit_list(&object.foundation)
            .iter()
            .copied()
            .map(|(x, y)| relative(x as i16, y as i16))
            .find(|&cell| admits(cell, EntryQueryMode::SkipLocomotor))
            .map(published);
    }
    //44F3B7..44F599: south/north pairs left-to-right, then east/west
    //pairs top-to-bottom, including the corners. Both use fifthmode1.
    let (width, height) = crate::rules::foundation::foundation_dimensions(&object.foundation);
    let (width, height) = (width as i16, height as i16);
    for x in -1..=width {
        for cell in [relative(x, height), relative(x, -1)] {
            if admits(cell, EntryQueryMode::CheckLocomotor) {
                return Some(published(cell));
            }
        }
    }
    for y in -1..=height {
        for cell in [relative(width, y), relative(-1, y)] {
            if admits(cell, EntryQueryMode::CheckLocomotor) {
                return Some(published(cell));
            }
        }
    }
    None
}

/// The cell of the depot's ArchiveTarget, its rally point (read at
/// `0x0044C40D`: the target's vt+0x48 coordinate, `>> 8` toward zero). A
/// rally click archives a cell ([`GameEntity::rally_cell`]).
///
/// [`GameEntity::rally_cell`]: crate::sim::game_entity::GameEntity::rally_cell
fn depot_rally_cell(
    sim: &Simulation,
    depot: &crate::sim::game_entity::GameEntity,
) -> Option<(u16, u16)> {
    match depot.archive_target()? {
        TargetKind::Cell(x, y) => Some((x, y)),
        TargetKind::Entity(id) => {
            let [x, y] = crate::sim::movement::ground_pose::object_center_xy(
                sim.substrate.entities.get(id)?,
            );
            Some((u16::try_from(x / 256).ok()?, u16::try_from(y / 256).ok()?))
        }
    }
}

/// BREAK the unit↔depot link over the bus (both slots cleared). No-op when the
/// unit holds no contact with the depot.
fn break_depot_contact(sim: &mut Simulation, unit_id: u64, depot_id: u64) {
    let linked = sim
        .substrate
        .entities
        .get(unit_id)
        .is_some_and(|unit| unit.radio_contacts.contains(depot_id));
    if linked {
        let _ = radio::transmit(
            sim,
            unit_id,
            depot_id,
            RadioMessage::Break,
            RadioPayload::default(),
            None,
        );
    }
}

/// The native Unit+500 pending entry, retained by the existing depot owner.
/// Unit741D9F and Foot70D84F/70D889 are its admission/clear writers.
pub(crate) fn set_pending_entry(sim: &mut Simulation, id: u64, depot: Option<u64>) {
    if let Some(unit) = sim.substrate.entities.get_mut(id) {
        unit.set_pending_entry(depot);
    }
}

/// `FootClass::TryEnterTransport @ 0x0070D7E0` (the name is a lead; it serves
/// every `Unit+0x500` pending entry) for a pending depot, called from
/// `FootClass::AI` (0x004DAEDC) whenever vt+0x1D8 (the warp-in) is clear:
/// - the depot no longer alive (`+0x90`, 0x0070D7F2) drops the entry;
/// - HELLO (0x0070D815): NEGATORY while the depot's slot is held, and the
///   entry waits for the next frame (the Aircraft/Helipad re-probe arm
///   0x0070D894..D8CD is not a depot's);
/// - CAN_LOAD (0x0F, 0x0070D825): ROGER ⇒ `Queue_Mission(Enter, 1)`,
///   `Set_Destination(depot, 1)` and the entry cleared (0x0070D836..D84F):
///   the linked unit drives onto the pad; any other answer ⇒
///   `Queue_Mission(NONE)`, `Set_Destination(NULL, 1)`, the entry cleared and
///   BREAK to the depot (0x0070D85E..D889).
///
/// So whichever pending unit's `FootClass::AI` runs first after the slot
/// frees takes it, in live-object order.
pub(crate) fn try_pending_entry(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(depot) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|unit| unit.pending_entry())
    else {
        return;
    };
    if !sim
        .substrate
        .entities
        .get(depot)
        .is_some_and(|depot| depot.is_object_alive())
    {
        if let Some(unit) = sim.substrate.entities.get_mut(id) {
            clear_pending_entry(unit);
        }
        return;
    };
    if radio::transmit(
        sim,
        id,
        depot,
        RadioMessage::Hello,
        RadioPayload::default(),
        Some(rules),
    ) != RadioResponse::Roger
    {
        return;
    }
    let reply = radio::transmit(
        sim,
        id,
        depot,
        RadioMessage::CanEnter,
        RadioPayload::default(),
        Some(rules),
    );
    if reply == RadioResponse::Roger {
        // Foot70D83C calls Queue(Enter,1) before its class setter. Queued
        // Enter bypasses motion in Unit Ready, but its contact still needs
        // the live type/world inputs (Unit Ready744270). A rules-free
        // provider cannot complete this queue transaction.
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Enter),
            1,
            sim.session.binary_frame,
            &LiveReadyInputProvider { rules },
        );
        sim.set_unit_destination(
            id,
            crate::sim::components::NavTargetRef::Building { id: depot },
            rules,
            true,
        );
        set_pending_entry(sim, id, None);
    } else {
        let _ = sim.mission_queue_exact(
            id,
            MissionId::NONE,
            0,
            sim.session.binary_frame,
            &LiveReadyInputProvider { rules },
        );
        sim.assign_null_destination(id, Some(rules), None);
        if let Some(unit) = sim.substrate.entities.get_mut(id) {
            clear_pending_entry(unit);
        }
        break_depot_contact(sim, id, depot);
    }
}

/// Whether the unit's Enter dispatch is the depot's: its first contact is a
/// depot, or it holds a pending entry with no contact. `Mission_Enter` probes
/// `Contacts[0]` first (0x004D9294), so a harvester that keeps a pending
/// depot while it docks at its refinery enters the refinery.
pub(crate) fn depot_owns_enter(sim: &Simulation, rules: Option<&RuleSet>, id: u64) -> bool {
    let Some(unit) = sim.substrate.entities.get(id) else {
        return false;
    };
    if let Some(contact) = unit.radio_contacts.slot(0) {
        return rules
            .and_then(|rules| {
                sim.substrate
                    .entities
                    .get(contact)
                    .and_then(|building| sim.object_type(building.type_ref(), rules))
            })
            .is_some_and(|object| object.unit_repair);
    }
    unit.pending_entry().is_some()
}

/// `FootClass::TryEnterTransport` parking tail (`0x0070D8F0`), used by the
/// shared Enter mission when it has neither a contact nor an archive. The
/// Foot's native pending-entry pointer owns the pending depot; a valid nearby
/// cell queues Move and keeps that entry. The caller commences afterward.
pub(crate) fn park_pending_entry(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(depot) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|unit| unit.pending_entry())
    else {
        return false;
    };
    if !sim
        .substrate
        .entities
        .get(depot)
        .is_some_and(|depot| depot.is_object_alive())
    {
        if let Some(unit) = sim.substrate.entities.get_mut(id) {
            clear_pending_entry(unit);
        }
        return false;
    }
    let Some((x, y)) = sim
        .techno_nearby_location(id, Some(depot), rules)
        .and_then(|(x, y)| Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?)))
    else {
        return false;
    };
    sim.set_unit_destination(
        id,
        crate::sim::components::NavTargetRef::cell(x, y),
        rules,
        true,
    );
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Move),
        0,
        sim.session.binary_frame,
        &LiveReadyInputProvider { rules },
    );
    true
}

/// Building NowDead4425AA clears native+500 after RUN_AWAY returns.
pub(crate) fn clear_pending_entry(unit: &mut crate::sim::game_entity::GameEntity) {
    unit.set_pending_entry(None);
}

/// Techno707AE7's destructive expiry clears only the matching entry pointer.
pub(crate) fn expire_reference(unit: &mut crate::sim::game_entity::GameEntity, expired: u64) {
    if unit.pending_entry() == Some(expired) {
        unit.set_pending_entry(None);
    }
}

/// Building vt+4D8, original447E00: distance from the contact GetCoords(+48)
/// to GetDockCoord(+A8). The shared coordinate and native-distance owners
/// retain retail pad offsets and rounding; Guard uses the separate raw center.
fn distance_to_pad(sim: &Simulation, rules: &RuleSet, depot: u64, unit: u64) -> Option<i32> {
    use crate::sim::movement::ground_pose;
    sim.substrate.entities.get(depot)?;
    let contact = sim.substrate.entities.get(unit)?;
    let terrain = sim.resolved_terrain.as_ref();
    let coord = ground_pose::object_get_coords(contact, terrain);
    let pad = crate::sim::movement::building_dock_coordinate(
        &sim.substrate.entities,
        depot,
        Some(unit),
        terrain,
        rules,
        &sim.interner,
    )?;
    Some(crate::util::native_x87::distance_3d_leptons(
        [pad.x, pad.z, pad.y],
        [coord.x, coord.z, coord.y],
    ))
}

///44BC01/44BDCA/44BFEF: restore SpecialAnimThree and ActiveAnim through the
/// existing slot owner. These calls may construct animations and draw RNG.
fn idle_service_art(sim: &mut Simulation, rules: &RuleSet, depot: u64, clear_first: bool) {
    let damaged = sim.substrate.entities.get(depot).and_then(|e| {
        sim.object_type(e.type_ref(), rules).map(|o| {
            e.health
                .compare_ratio(o.strength, rules.general.condition_yellow)
        })
    }) != Some(crate::util::native_x87::MaskedX87Ordering::Greater);
    if clear_first {
        sim.clear_building_anim_slot(depot, 8);
        sim.clear_building_anim_slot(depot, 11);
    }
    sim.set_building_anim_slot(depot, 12, damaged, false, 0, rules);
    sim.set_building_anim_slot(depot, 3, damaged, false, 0, rules);
    if !clear_first {
        sim.clear_building_anim_slot(depot, 8);
        sim.clear_building_anim_slot(depot, 11);
    }
}

///44C397/44BE97: release through the Unit setter and Radio contact owners.
/// State1 clears ArchiveTarget unconditionally after a successful release;
/// state2 clears it only in the AI archive arm. An unavailable exit holds the
/// contact for a later dispatch. Foundation waiting #937 remains separate.
fn release_contact(
    sim: &mut Simulation,
    rules: &RuleSet,
    depot: u64,
    unit: u64,
    state1: bool,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    let Some(contact) = sim.substrate.entities.get(unit) else {
        return;
    };
    let archive = contact.archive_target();
    let ai_archive = archive.filter(|_| {
        sim.houses
            .get(&contact.owner())
            .is_some_and(|h| !h.is_controlled_by_human(sim.session.game_mode_nonzero))
    });
    if state1
        && let Some(contact) = sim.substrate.entities.get_mut(unit)
        && let Some(locomotor) = contact.locomotor.as_mut()
    {
        locomotor.power_on();
    }
    let destination = if let Some(archive) = ai_archive {
        match archive {
            TargetKind::Cell(x, y) => Some(crate::sim::components::NavTargetRef::cell(x, y)),
            TargetKind::Entity(id) => Some(crate::sim::components::NavTargetRef::Entity { id }),
        }
    } else {
        let Some(building) = sim.substrate.entities.get(depot) else {
            return;
        };
        // Original runs the exit search before a rally overrides its answer.
        // One GetDockCell44EFB0 port supplies depot and factory callers.
        let exit = building_dock_cell(sim, depot, unit, (0, 0), rules, registry);
        depot_rally_cell(sim, building)
            .or(exit)
            .map(|(x, y)| crate::sim::components::NavTargetRef::cell(x, y))
    };
    let Some(destination) = destination else {
        return;
    };
    let _ = sim.mission_queue_exact(
        unit,
        MissionId::from_known(MissionType::Move),
        0,
        sim.session.binary_frame,
        &LiveReadyInputProvider { rules },
    );
    sim.set_unit_destination(unit, destination, rules, true);
    if state1 || ai_archive.is_some() {
        if let Some(contact) = sim.substrate.entities.get_mut(unit) {
            contact.set_archive_target(None);
        }
    }
    // State1's AI archive branch clears +500 before BREAK44C3D6;
    // the other release branches clear it after the radio return.
    if state1
        && ai_archive.is_some()
        && let Some(contact) = sim.substrate.entities.get_mut(unit)
    {
        clear_pending_entry(contact);
    }
    radio::transmit_to_contact(sim, depot, RadioMessage::Break, Some(rules));
    if !(state1 && ai_archive.is_some())
        && let Some(contact) = sim.substrate.entities.get_mut(unit)
    {
        clear_pending_entry(contact);
    }
}

/// BuildingClass::MissionRepairAndProduce44B780's UnitRepair arm. MissionCom
/// owns status/dispatch; BuildingMissionLeaf owns the independent+620 progress;
/// Techno RepairTick owns heal/payment. Original corpus: building_repair.depot_service.
/// No unit service phase, wallet mutation, grace timeout or late sweep remains.
/// Other families of this shared native handler keep their existing owners.
pub(crate) fn mission_repair(
    sim: &mut Simulation,
    rules: &RuleSet,
    depot: u64,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<i32> {
    use crate::util::native_x87::{
        MaskedX87Chop53 as X87, MaskedX87Ordering as Ordering, NativeF64Bits,
    };
    let building = sim.substrate.entities.get(depot)?;
    let object = sim.object_type(building.type_ref(), rules)?;
    if !object.unit_repair {
        return None;
    }
    let status = building.mission.handler_state();
    let current = building
        .mission
        .current()
        .known()
        .unwrap_or(MissionType::Repair);
    let rate = rules.mission_control.rate_frames(current);
    let contact = building.radio_contacts.slot(0);
    let now = sim.session.binary_frame;
    let Some(unit) = contact else {
        // State1's no-contact arm44C0BD retains the current idle animations
        // when neither service slot exists. Recreating them would reset their
        // stages and consume additional constructor RNG on every dispatch.
        if status != 1
            || building.building_anim_slots[8].is_some()
            || building.building_anim_slots[11].is_some()
        {
            idle_service_art(sim, rules, depot, status != 1);
        }
        let building = sim.substrate.entities.get_mut(depot)?;
        if status == 2 {
            building.mission.set_handler_state(1);
        }
        if status == 0 || (status == 1 && building.building_anim_slots[18].is_none()) {
            // State0 Queue44C6EA preserves +6DD; only state1's no-contact
            // arm44C18E sets ready before asking Queue to commence Guard.
            if status == 1 {
                building.mission_leaf.set_building_ready_latch(1);
            }
            let _ = sim.mission_queue_exact(
                depot,
                MissionId::from_known(MissionType::Guard),
                1,
                now,
                &LiveReadyInputProvider { rules },
            );
        }
        return Some(1);
    };
    if status == 0 {
        sim.substrate
            .entities
            .get_mut(depot)?
            .mission_leaf
            .set_building_ready_latch(0);
        let hover_or_teleport = sim
            .substrate
            .entities
            .get(unit)
            .and_then(|e| e.locomotor.as_ref())
            .is_some_and(|l| {
                matches!(
                    l.kind,
                    crate::rules::locomotor_type::LocomotorKind::Hover
                        | crate::rules::locomotor_type::LocomotorKind::Teleport
                )
            });
        if radio::transmit_to_contact(sim, depot, RadioMessage::NeedToMove, Some(rules))
            == RadioResponse::Roger
            && distance_to_pad(sim, rules, depot, unit)
                .is_some_and(|d| d < if hover_or_teleport { 200 } else { 100 })
        {
            sim.substrate
                .entities
                .get_mut(depot)?
                .mission
                .set_handler_state(1);
            return Some(3);
        }
        // Original44C7E5..44C808: active YR House53A130 returns false,
        // then ILoco+58 restores power while admission is still pending.
        if let Some(l) = sim
            .substrate
            .entities
            .get_mut(unit)
            .and_then(|e| e.locomotor.as_mut())
        {
            l.power_on();
        }
        return Some(rate);
    }
    if status == 1 {
        // The ProductionAnim must finish before the first paid step.
        if sim.substrate.entities.get(depot)?.building_anim_slots[8].is_some() {
            return Some(1);
        }
        if distance_to_pad(sim, rules, depot, unit).is_some_and(|d| d < 200) {
            let contact = sim.substrate.entities.get(unit)?;
            let powered = contact.locomotor.as_ref().is_some_and(|l| l.is_powered());
            if powered {
                if crate::sim::movement::motion_query::is_moving(contact) == Some(false)
                    && let Some(l) = sim
                        .substrate
                        .entities
                        .get_mut(unit)
                        .and_then(|e| e.locomotor.as_mut())
                {
                    l.power_off();
                }
                return Some(1);
            }
            if let Some(contact) = sim.substrate.entities.get_mut(unit)
                && contact.navigation.nav_com.is_some()
            {
                crate::sim::movement::foot_stop_moving(contact);
            }
        }
        if radio::transmit_to_contact(sim, depot, RadioMessage::NeedToMove, Some(rules))
            != RadioResponse::Roger
        {
            // Original44C5AB..44C61B takes the same constant-false House
            // gate, queries ILoco+60 and powers on an unpowered contact.
            if let Some(l) = sim
                .substrate
                .entities
                .get_mut(unit)
                .and_then(|e| e.locomotor.as_mut())
                && !l.is_powered()
            {
                l.power_on();
            }
            return Some(rate);
        }
        let contact = sim.substrate.entities.get(unit)?;
        let typ = sim.object_type(contact.type_ref(), rules)?;
        let damaged = matches!(
            contact.health.compare_ratio(typ.strength, 1.0),
            Ordering::Less | Ordering::Unordered
        );
        let manual_reload = typ.manual_reload;
        let reply = radio::transmit_to_contact(sim, depot, RadioMessage::RepairTick, Some(rules));
        if (damaged || manual_reload)
            && matches!(reply, RadioResponse::Roger | RadioResponse::RepairComplete)
        {
            // Local+41A is resolved by the app, as for ToggleRepair4470B7.
            // Original44C507 announces before the service animation writes.
            let owner = sim.substrate.entities.get(depot)?.owner();
            sim.sound_events.push(SimSoundEvent::Repairing { owner });
            sim.substrate
                .entities
                .get_mut(depot)?
                .mission
                .set_handler_state(2);
            let damaged = sim.substrate.entities.get(depot).and_then(|e| {
                sim.object_type(e.type_ref(), rules).map(|o| {
                    e.health
                        .compare_ratio(o.strength, rules.general.condition_yellow)
                })
            }) != Some(Ordering::Greater);
            sim.set_building_anim_slot(depot, 10, damaged, false, 0, rules);
            sim.clear_building_anim_slot(depot, 3);
            sim.clear_building_anim_slot(depot, 18);
            let building = sim.substrate.entities.get_mut(depot)?;
            building.mission_leaf.set_building_ready_latch(0);
            building
                .mission_leaf
                .start_building_repair_progress(now as i32);
        } else {
            let contact = sim.substrate.entities.get(unit)?;
            let typ = sim.object_type(contact.type_ref(), rules)?;
            if matches!(
                contact.health.compare_ratio(typ.strength, 1.0),
                Ordering::Equal | Ordering::Unordered
            ) && contact.locomotor.as_ref().is_some_and(|l| !l.is_powered())
            {
                release_contact(sim, rules, depot, unit, true, registry);
            }
        }
        return Some(rate);
    }
    if status == 2 {
        let progress = sim
            .substrate
            .entities
            .get_mut(depot)?
            .mission_leaf
            .advance_building_repair_progress(now as i32);
        let threshold = X87::mul(
            X87::load_f64(NativeF64Bits::from_bits(
                rules.general.unit_repair_rate.to_bits(),
            )),
            X87::load_i32(900),
        );
        // FC0/C2/C3 test includes unordered, as the original FCOMPP.
        if matches!(
            X87::compare(threshold, X87::load_i32(progress)),
            Ordering::Greater
        ) {
            return Some(1);
        }
        if radio::transmit_to_contact(sim, depot, RadioMessage::NeedToMove, Some(rules))
            != RadioResponse::Roger
        {
            return Some(1);
        }
        let building = sim.substrate.entities.get_mut(depot)?;
        building.mission_leaf.set_building_ready_latch(0);
        building.mission_leaf.reset_building_repair_progress();
        match radio::transmit_to_contact(sim, depot, RadioMessage::RepairTick, Some(rules)) {
            RadioResponse::Roger => {}
            RadioResponse::InsufficientFunds => {
                // Original44BFD9 only announces an exactly empty wallet;
                // a nonzero shortfall remains silent. Client filters +41A.
                let owner = sim.substrate.entities.get(depot)?.owner();
                if crate::sim::credit_income::available_money(sim, owner) == 0 {
                    sim.sound_events.push(SimSoundEvent::HouseEva {
                        owner,
                        event: "EVA_InsufficientFunds",
                    });
                }
                idle_service_art(sim, rules, depot, true);
                sim.substrate
                    .entities
                    .get_mut(depot)?
                    .mission
                    .set_handler_state(1);
            }
            _ => {
                // Original44BDB2 gates the completion voice on the existing
                // client's type8 radar admission; simulation publishes the
                // request and the app resolves local ownership and admission.
                let building = sim.substrate.entities.get(depot)?;
                sim.sound_events.push(SimSoundEvent::UnitRepaired {
                    owner: building.owner(),
                    radar: crate::sim::radar::RadarEventRequest::new(
                        crate::sim::radar::RadarEventType::UnitRepaired,
                        building.position.rx,
                        building.position.ry,
                    ),
                });
                idle_service_art(sim, rules, depot, true);
                sim.substrate
                    .entities
                    .get_mut(depot)?
                    .mission
                    .set_handler_state(1);
                release_contact(sim, rules, depot, unit, false, registry);
            }
        }
        return Some(1);
    }
    Some(rate)
}

#[cfg(test)]
#[path = "building_dock_frame_tests.rs"]
mod frame_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::command::Command;
    use crate::sim::components::{Health, NavTargetRef};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellListInsertion;

    /// Every initialized45C300 row comes from saved original execution,
    /// including omitted/repeated entries and the empty0x0 row.
    #[test]
    fn foundation_exit_lists_match_native_rows() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "src/sim/docking/fixtures/building_exit_tables_native.json",
        ))
        .unwrap();
        let rows = native["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 22);
        for (foundation, row) in crate::rules::foundation::FOUNDATION_TABLE.iter().zip(rows) {
            assert_eq!(
                row["foundation_id"].as_u64(),
                Some(u64::from(foundation.id))
            );
            let expected: Vec<(i32, i32)> = row["cells"]
                .as_array()
                .unwrap()
                .iter()
                .map(|cell| {
                    (
                        cell[0].as_i64().unwrap() as i32,
                        cell[1].as_i64().unwrap() as i32,
                    )
                })
                .collect();
            assert_eq!(
                foundation_exit_list(foundation.name),
                expected,
                "{}",
                foundation.name
            );
        }
    }

    fn factory_exit_fixture(extra: &str) -> (Simulation, RuleSet) {
        let flags = if extra.is_empty() {
            "GDIBarracks=yes\n"
        } else {
            extra
        };
        let (mut sim, rules, _) =
            crate::sim::world::entry_test_fixture::fixture_with_rules_and_fixed_art(
                &format!(
                    "[InfantryTypes]\n2=E1\n\
                     [BuildingTypes]\n1=GAPILE\n2=GAPOWR\n\
                     [E1]\nStrength=125\nSpeed=4\nSpeedType=Foot\n\
                     Locomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\n\
                     [GAPILE]\nStrength=500\n{flags}\
                     [GAPOWR]\nStrength=750\n"
                ),
                &IniFile::from_str("[GAPILE]\nFoundation=3x2\n[GAPOWR]\nFoundation=2x2\n"),
            );
        sim.playfield_size_height = Some(16);
        spawn_exit_query_building(&mut sim, &rules, DEPOT, "GAPILE", 14, 14);
        spawn_entity(&mut sim, 1, "E1", EntityCategory::Infantry, 14, 14, 125);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .lifecycle
            .in_limbo = true;
        (sim, rules)
    }

    fn spawn_exit_query_building(
        sim: &mut Simulation,
        rules: &RuleSet,
        id: u64,
        name: &str,
        rx: u16,
        ry: u16,
    ) {
        let object = rules.object(name).unwrap();
        spawn_entity(
            sim,
            id,
            name,
            EntityCategory::Structure,
            rx,
            ry,
            object.strength,
        );
        sim.substrate.entities.get_mut(id).unwrap().foundation = object.foundation.clone();
        let (width, height) = crate::rules::foundation::foundation_dimensions(&object.foundation);
        for y in ry..ry + height {
            for x in rx..rx + width {
                sim.substrate.occupancy.add(
                    x,
                    y,
                    id,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
        }
    }

    /// Original geometry corpus reaches these selections with a held E1 and
    /// actual GAPOWR obstructions; the shared class owner decides them here.
    #[test]
    fn gdi_exit_preferred_fallback_and_refusal_match_native() {
        for (obstacles, expected) in [
            (vec![], Some((15, 16))),
            (vec![(15, 16)], Some((14, 16))),
            (
                vec![
                    (13, 16),
                    (15, 16),
                    (17, 16),
                    (13, 12),
                    (15, 12),
                    (17, 12),
                    (12, 14),
                    (17, 14),
                ],
                None,
            ),
        ] {
            let (mut sim, rules) = factory_exit_fixture("");
            for (i, (x, y)) in obstacles.iter().copied().enumerate() {
                spawn_exit_query_building(&mut sim, &rules, 600 + i as u64, "GAPOWR", x, y);
            }
            let before = (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            );
            assert_eq!(
                building_dock_cell(&sim, DEPOT, 1, (0, 0), &rules, None),
                expected
            );
            assert_eq!(
                before,
                (
                    sim.main_rng.logical_state(),
                    sim.scenario_rng.logical_state(),
                    sim.mapgen_rng.logical_state(),
                )
            );
        }
    }

    /// NOD/Yuri/Hospital contrasts supply flags on GAPILE, matching the
    /// native primitive coverage; they do not certify those stock objects.
    #[test]
    fn shared_exit_flags_offer_and_hospital_scan_match_native() {
        for (extra, offered, expected) in [
            ("GDIBarracks=no\nNODBarracks=yes\n", (0, 0), Some((16, 16))),
            ("GDIBarracks=no\nYuriBarracks=yes\n", (0, 0), Some((14, 16))),
            ("GDIBarracks=no\n", (0, 0), Some((14, 16))),
            ("GDIBarracks=no\n", (20, 20), Some((20, 20))),
            ("GDIBarracks=no\nHospital=yes\n", (0, 0), Some((13, 16))),
        ] {
            let (sim, rules) = factory_exit_fixture(extra);
            assert_eq!(
                building_dock_cell(&sim, DEPOT, 1, offered, &rules, None),
                expected
            );
        }
    }

    #[test]
    fn shared_exit_uses_native_size_diamond_before_class_lookup() {
        let (mut sim, rules) = factory_exit_fixture("GDIBarracks=no\n");
        sim.substrate.entities.get_mut(DEPOT).unwrap().position.rx = 0;
        sim.substrate.entities.get_mut(DEPOT).unwrap().position.ry = 0;
        assert_eq!(
            building_dock_cell(&sim, DEPOT, 1, (0, 0), &rules, None),
            None
        );
        assert_eq!(
            building_dock_cell(&sim, DEPOT, 1, (20, 20), &rules, None),
            Some((20, 20))
        );
    }

    /// The original depot ingress corpus applies the initialized3x3 row
    /// to NADEPT's4x3 foundation. Unit73F0A0 rejects five exterior raw
    /// reservations, then admits9,11 within its linked repair building.
    /// The former occupancy proxy rejected that native result indefinitely.
    #[test]
    fn linked_depot_exit_admits_native_inside_footprint_result() {
        use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};

        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules_and_fixed_art(
                "[VehicleTypes]\n0=MTNK\n\
                 [BuildingTypes]\n1=NADEPT\n\
                 [MTNK]\nStrength=300\nSpeed=6\nSpeedType=Track\n\
                 Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
                 [NADEPT]\nStrength=1200\nUnitRepair=yes\n\
                 NumberOfDocks=1\nNumberImpassableRows=1\n",
                &IniFile::from_str("[NADEPT]\nFoundation=4x3\n"),
            );
        sim.playfield_size_height = Some(16);
        spawn_exit_query_building(&mut sim, &rules, DEPOT, "NADEPT", 6, 9);
        spawn_tank(&mut sim, 1, 8, 10);
        sim.substrate
            .entities
            .get_mut(DEPOT)
            .unwrap()
            .mark_live_contact_with(1);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.mark_live_contact_with(DEPOT);
        unit.set_pending_entry(Some(DEPOT));

        for (x, y) in foundation_exit_list("4x3") {
            let cell = (6 + x, 9 + y);
            if !(6..10).contains(&cell.0) || !(9..12).contains(&cell.1) {
                sim.substrate
                    .raw_cell_occupation
                    .mark_ground(cell.0 as u16, cell.1 as u16, 0x20);
            }
        }
        let before = (
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        for cell in [(6, 12), (7, 12), (8, 12), (5, 12), (9, 12)] {
            assert_eq!(
                sim.mover_can_enter(
                    1,
                    cell,
                    InfantryEntryArgs::REPAIR,
                    EntryQueryMode::SkipLocomotor,
                    &rules,
                    Some(&registry),
                ),
                Ok(2),
                "native raw reservation at {cell:?}"
            );
        }
        assert_eq!(
            sim.mover_can_enter(
                1,
                (9, 11),
                InfantryEntryArgs::REPAIR,
                EntryQueryMode::SkipLocomotor,
                &rules,
                Some(&registry),
            ),
            Ok(0)
        );
        assert_eq!(
            building_dock_cell(&sim, DEPOT, 1, (0, 0), &rules, Some(&registry)),
            Some((9, 11))
        );
        assert_eq!(
            before,
            (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            )
        );

        // Supplied completed-service boundary; execute the shared original
        // release suffix, without resurrecting the removed depot FSM.
        release_contact(&mut sim, &rules, DEPOT, 1, false, Some(&registry));

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.navigation.nav_com, Some(NavTargetRef::cell(9, 11)));
        assert!(unit.pending_entry().is_none());
        assert!(!unit.has_live_contact_with(DEPOT));
        assert!(
            sim.substrate
                .entities
                .get(DEPOT)
                .unwrap()
                .radio_contacts
                .is_empty()
        );
    }

    /// Original Unit/Foot/Techno1C: full health replies10 without paying or
    /// resetting independent estimated health (building_repair depot controls).
    #[test]
    fn full_health_repair_request_does_not_pay_or_reset_estimated_health() {
        let (mut sim, rules) = setup(1);
        let owner = sim.substrate.entities.get(1).unwrap().owner();
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.health.current = 300;
        unit.estimated_health.reset(-20);
        let rng = sim.scenario_rng.state();
        assert_eq!(repair_request(&mut sim, &rules, 1), RadioResponse::Negatory);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.health.current, 300);
        assert_eq!(unit.estimated_health.get(), -20);
        assert_eq!(sim.houses[&owner].economy.credits, 10_000);
        assert_eq!(sim.houses[&owner].economy.spent_credits, 0);
        assert_eq!(sim.scenario_rng.state(), rng);
    }

    /// Foot4D900E replies10 while NavCom is nonnull, before any wallet/heal.
    #[test]
    fn moving_repair_request_stops_at_the_foot_navcom_interceptor() {
        let (mut sim, rules) = setup(1);
        let owner = sim.substrate.entities.get(1).unwrap().owner();
        assert!(sim.set_unit_destination(1, NavTargetRef::cell(15, 11), &rules, true));
        assert_eq!(repair_request(&mut sim, &rules, 1), RadioResponse::Negatory);
        assert_eq!(sim.substrate.entities.get(1).unwrap().health.current, 100);
        assert_eq!(sim.houses[&owner].economy.credits, 10_000);
        assert_eq!(sim.houses[&owner].economy.spent_credits, 0);
    }

    /// Techno6F4D35 replies32 without side effects; repeated requests do not
    /// accumulate a grace timeout (native long-no-funds service history).
    #[test]
    fn insufficient_money_never_accumulates_a_repair_grace_timeout() {
        let (mut sim, rules) = setup(1);
        let owner = sim.substrate.entities.get(1).unwrap().owner();
        sim.houses.get_mut(&owner).unwrap().economy.credits = 1;
        let rng = sim.scenario_rng.state();
        let estimate = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .estimated_health
            .get();
        for _ in 0..300 {
            assert_eq!(
                repair_request(&mut sim, &rules, 1),
                RadioResponse::InsufficientFunds
            );
        }
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.health.current, 100);
        assert_eq!(unit.estimated_health.get(), estimate);
        assert_eq!(sim.houses[&owner].economy.credits, 1);
        assert_eq!(sim.houses[&owner].economy.spent_credits, 0);
        assert_eq!(sim.scenario_rng.state(), rng);
    }

    // --- Admission / waiting / release through the production order path ---

    const DEPOT: u64 = 500;
    const DEPOT_RX: u16 = 10;
    const DEPOT_RY: u16 = 10;

    fn depot_rules() -> RuleSet {
        depot_rules_with_strength(300)
    }

    fn depot_rules_with_strength(strength: i32) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\n\
             RepairPercent=15%\n\
             RepairStep=8\n\
             URepairRate=.016\n\
             [Enter]\n\
             Rate=.016\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             1=HARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GADEPT\n\
             [MTNK]\n\
             Name=Grizzly\n\
             Cost=700\n\
             Strength={strength}\n\
             Speed=6\n\
             Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
             [HARV]\n\
             Name=War Miner\n\
             Cost=1400\n\
             Strength=600\n\
             Speed=4\n\
             Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
             Harvester=yes\n\
             [GADEPT]\n\
             Name=Depot\n\
             UnitRepair=yes\n\
             NumberImpassableRows=1\n\
             Strength=1000\n",
        ));
        RuleSet::from_ini_with_fixed_art_for_test(
            &ini,
            &IniFile::from_str("[GADEPT]\nFoundation=3x3\n"),
        )
        .expect("depot rules")
    }

    fn spawn_entity(
        sim: &mut Simulation,
        sid: u64,
        type_id: &str,
        category: EntityCategory,
        rx: u16,
        ry: u16,
        hp: i32,
    ) {
        let owner_id = sim.interner.intern("Americans");
        let type_name = type_id;
        let type_id = sim.interner.intern(type_id);
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            sid,
            rx,
            ry,
            0,
            0,
            owner_id,
            Health { current: hp },
            type_id,
            category,
            0,
            5,
            category == EntityCategory::Unit,
        );
        ge.lifecycle.in_limbo = false;
        if category == EntityCategory::Unit {
            // A live Drive mover: the depot's class setter and the Process
            // corridor need its locomotor.
            ge.locomotor = Some(
                crate::sim::movement::locomotor::LocomotorState::from_object_type(
                    depot_rules().object(type_name).expect("fixture unit type"),
                    0,
                ),
            );
            sim.substrate.occupancy.add(
                rx,
                ry,
                sid,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        sim.substrate.entities.insert(ge);
        if sim.substrate.next_stable_object_id <= sid {
            sim.substrate.next_stable_object_id = sid + 1;
        }
    }

    fn spawn_depot(sim: &mut Simulation) {
        spawn_entity(
            sim,
            DEPOT,
            "GADEPT",
            EntityCategory::Structure,
            DEPOT_RX,
            DEPOT_RY,
            1000,
        );
        sim.substrate.entities.get_mut(DEPOT).unwrap().foundation = "3x3".to_string();
        for y in DEPOT_RY..DEPOT_RY + 3 {
            for x in DEPOT_RX..DEPOT_RX + 3 {
                sim.substrate.occupancy.add(
                    x,
                    y,
                    DEPOT,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
        }
    }

    fn spawn_tank(sim: &mut Simulation, sid: u64, rx: u16, ry: u16) {
        spawn_entity(sim, sid, "MTNK", EntityCategory::Unit, rx, ry, 100);
    }

    fn setup(tank_count: u64) -> (Simulation, RuleSet) {
        let rules = depot_rules();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        {
            use crate::sim::house_state::HouseState;
            let owner_id = sim.interner.intern("Americans");
            let mut house = HouseState::new(owner_id, 0, None, false, 0, 10);
            house.economy.credits = 10_000;
            sim.houses.insert(owner_id, house);
        }
        spawn_depot(&mut sim);
        for i in 0..tank_count {
            spawn_tank(&mut sim, 1 + i, 14 + i as u16, 11);
        }
        (sim, rules)
    }

    /// Original Unit737430 -> Foot4D8FB0 -> Techno6F4CD7 repair request:
    /// the paid mutation uses House4F9790, including its spending statistic.
    /// Native inputs/results are retained in building_repair.depot_service.
    #[test]
    fn repair_request_uses_the_shared_house_spending_owner() {
        let (mut sim, rules) = setup(1);
        let owner = sim.substrate.entities.get(1).unwrap().owner();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .estimated_health
            .reset(100);
        let rng = sim.scenario_rng.state();

        let response = radio::transmit(
            &mut sim,
            DEPOT,
            1,
            RadioMessage::RepairTick,
            RadioPayload::default(),
            Some(&rules),
        );

        assert_eq!(response, RadioResponse::Roger);
        let tank = sim.substrate.entities.get(1).unwrap();
        assert_eq!(tank.health.current, 108);
        let house = sim.houses.get(&owner).unwrap();
        assert_eq!(house.economy.credits, 9_998);
        assert_eq!(house.economy.spent_credits, 2);
        assert_eq!(sim.scenario_rng.state(), rng);
    }

    /// Unit+0x500 survives an admitted visit and only its native clear writers
    /// remove it; service timers belong to the admitted depot, not the pointer.
    #[test]
    fn pending_entry_and_admitted_service_have_independent_lifecycles() {
        let (mut sim, rules) = setup(1);
        let pending = DEPOT + 1;
        link_for_service(&mut sim, 1);
        set_pending_entry(&mut sim, 1, Some(pending));
        sim.mission_assign_exact(DEPOT, MissionId::from_known(MissionType::Repair), 0)
            .unwrap();
        let building = sim.substrate.entities.get_mut(DEPOT).unwrap();
        building.mission.set_handler_state(2);
        building.mission_leaf.start_building_repair_progress(0);
        assert_eq!(mission_repair(&mut sim, &rules, DEPOT, None), Some(1));
        let before = *sim
            .substrate
            .entities
            .get(DEPOT)
            .unwrap()
            .mission_leaf
            .as_building()
            .unwrap()
            .repair_progress();

        set_pending_entry(&mut sim, 1, Some(pending + 1));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().pending_entry(),
            Some(pending + 1)
        );
        assert!(linked(&sim, 1));
        clear_pending_entry(sim.substrate.entities.get_mut(1).unwrap());
        assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
        assert!(linked(&sim, 1));
        assert_eq!(
            sim.substrate
                .entities
                .get(DEPOT)
                .unwrap()
                .mission
                .handler_state(),
            2
        );
        assert_eq!(
            *sim.substrate
                .entities
                .get(DEPOT)
                .unwrap()
                .mission_leaf
                .as_building()
                .unwrap()
                .repair_progress(),
            before
        );
    }

    #[test]
    fn pointer_expiry_clears_only_the_matching_pending_or_admitted_visit() {
        let (mut sim, rules) = setup(1);
        let pending = DEPOT + 1;
        link_for_service(&mut sim, 1);
        set_pending_entry(&mut sim, 1, Some(pending));
        expire_reference(sim.substrate.entities.get_mut(1).unwrap(), pending);
        assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
        assert!(
            linked(&sim, 1),
            "pending expiry does not tear down the admitted contact"
        );

        set_pending_entry(&mut sim, 1, Some(pending));
        sim.uninit_with_rules(DEPOT, &rules);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.pending_entry(), Some(pending));
        assert!(!unit.radio_contacts.contains(DEPOT));
        expire_reference(sim.substrate.entities.get_mut(1).unwrap(), pending);
        assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
    }

    /// Foot70D83C's Queue(Enter,1) uses the linked Building's type in Unit
    /// Ready even when the pending unit is moving to its parking cell. The
    /// live mission owner must promote Enter before the class pad setter.
    #[test]
    fn a_moving_pending_unit_commences_enter_before_its_pad_order() {
        let (mut sim, rules) = setup(1);
        sim.mission_assign_exact(1, MissionId::from_known(MissionType::Move), 0)
            .unwrap();
        assert!(sim.set_unit_destination(1, NavTargetRef::cell(15, 11), &rules, true));
        set_pending_entry(&mut sim, 1, Some(DEPOT));
        let rng = sim.scenario_rng.state();

        try_pending_entry(&mut sim, &rules, 1);

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.mission.current().known(), Some(MissionType::Enter));
        assert_eq!(unit.mission.queued(), MissionId::NONE);
        assert_eq!(
            unit.navigation.nav_com,
            Some(NavTargetRef::Building { id: DEPOT })
        );
        assert_eq!(unit.pending_entry(), None);
        assert!(linked(&sim, 1));
        assert_eq!(sim.scenario_rng.state(), rng);
    }

    #[test]
    fn depot_completion_preserves_signed_and_masked_ratio_domain() {
        for (hp, strength, complete) in [
            (70_000, 100_000, false),
            (100_000, 70_000, true),
            (-20, -10, true),
            (-5, -10, false),
            (1, 0, true),
            (-1, 0, false),
            (0, 0, false),
        ] {
            let (mut sim, _) = setup(1);
            let rules = depot_rules_with_strength(strength);
            sim.substrate.entities.get_mut(1).unwrap().health.current = hp;
            let response = radio::transmit(
                &mut sim,
                DEPOT,
                1,
                RadioMessage::IsRepairing,
                RadioPayload::default(),
                Some(&rules),
            );
            assert_eq!(
                response == RadioResponse::Negatory,
                complete,
                "{hp}/{strength}"
            );
        }
    }

    #[test]
    fn depot_service_wraps_both_independent_signed_health_values() {
        let (mut sim, _) = setup(1);
        let rules = depot_rules_with_strength(i32::MAX);
        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.health.current = i32::MAX - 1;
        unit.estimated_health =
            crate::sim::estimated_health::EstimatedHealth::from_raw(i32::MAX - 2);
        assert_eq!(repair_request(&mut sim, &rules, 1), RadioResponse::Roger);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.health.current, i32::MIN + 6);
        assert_eq!(unit.estimated_health.get(), i32::MIN + 5);
    }

    #[test]
    fn depot_service_adds_reservations_and_resets_them_on_completion() {
        let (mut sim, rules) = setup(1);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .estimated_health
            .reset(-20);
        assert_eq!(repair_request(&mut sim, &rules, 1), RadioResponse::Roger);
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.health.current, 108);
        assert_eq!(unit.estimated_health.get(), -12);

        let unit = sim.substrate.entities.get_mut(1).unwrap();
        unit.health.current = 299;
        unit.estimated_health.reset(-20);
        assert_eq!(
            repair_request(&mut sim, &rules, 1),
            RadioResponse::RepairComplete
        );
        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(unit.health.current, 300);
        assert_eq!(unit.estimated_health.get(), 300);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .estimated_health
            .reset(-20);
        assert_eq!(repair_request(&mut sim, &rules, 1), RadioResponse::Negatory);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .estimated_health
                .get(),
            -20
        );
    }

    fn repair_request(sim: &mut Simulation, rules: &RuleSet, unit: u64) -> RadioResponse {
        radio::transmit(
            sim,
            DEPOT,
            unit,
            RadioMessage::RepairTick,
            RadioPayload::default(),
            Some(rules),
        )
    }

    fn link_for_service(sim: &mut Simulation, unit: u64) {
        sim.substrate
            .entities
            .get_mut(unit)
            .unwrap()
            .mark_live_contact_with(DEPOT);
        sim.substrate
            .entities
            .get_mut(DEPOT)
            .unwrap()
            .mark_live_contact_with(unit);
    }

    /// Component fixture for unit-only Enter/RNG observations. Depot visits
    /// are explicit so its independent Guard cadence is outside those checks.
    fn tick_units(sim: &mut Simulation, rules: &RuleSet) {
        sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
        visit_units(sim, rules);
        process_units(sim, rules);
        pending_entries(sim, rules);
        sim.session.tick += 1;
    }

    fn visit_depot(sim: &mut Simulation, rules: &RuleSet) {
        sim.object_ai_visit_one(
            DEPOT,
            Some(rules),
            crate::sim::world::ObjectAiCtx::default(),
        );
    }

    fn order_repair(sim: &mut Simulation, rules: &RuleSet, tank: u64) -> bool {
        sim.apply_command(
            "Americans",
            &Command::RepairAtDepot {
                entity_id: tank,
                depot_id: DEPOT,
            },
            Some(rules),
        )
    }

    /// Component fixture: each unit's AI and movement, its pending-entry
    /// retry, then the Building's ordinary mission visit. Full-world ordering
    /// is exercised separately by the track-path continuation tests.
    fn tick(sim: &mut Simulation, rules: &RuleSet) {
        tick_units(sim, rules);
        visit_depot(sim, rules);
    }

    /// Component-only sorted-id retry sweep. Complete object turns and the
    /// live Logic order are exercised by `frame_tests`.
    fn pending_entries(sim: &mut Simulation, rules: &RuleSet) {
        for id in sim.substrate.entities.keys_sorted() {
            try_pending_entry(sim, rules, id);
        }
    }

    /// Component-only sorted-id Process sweep.
    fn process_units(sim: &mut Simulation, rules: &RuleSet) {
        for id in sim.substrate.entities.keys_sorted() {
            if sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| e.category == EntityCategory::Unit)
            {
                sim.process_ground_locomotor_one(id, Some(rules), None)
                    .expect("unit Process");
            }
        }
    }

    /// Component-only sorted-id Unit AI sweep. Storage order is not the
    /// production live-object order.
    fn visit_units(sim: &mut Simulation, rules: &RuleSet) {
        let units: Vec<u64> = sim
            .substrate
            .entities
            .keys_sorted()
            .into_iter()
            .filter(|&id| {
                sim.substrate
                    .entities
                    .get(id)
                    .is_some_and(|e| e.category == EntityCategory::Unit)
            })
            .collect();
        for id in units {
            sim.object_ai_visit_one(id, Some(rules), crate::sim::world::ObjectAiCtx::default());
        }
    }

    fn linked(sim: &Simulation, tank: u64) -> bool {
        sim.substrate
            .entities
            .get(tank)
            .is_some_and(|e| e.radio_contacts.contains(DEPOT))
            && sim
                .substrate
                .entities
                .get(DEPOT)
                .is_some_and(|d| d.radio_contacts.contains(tank))
    }

    fn servicing(sim: &Simulation, tank: u64) -> bool {
        linked(sim, tank)
            && sim.substrate.entities.get(DEPOT).is_some_and(|building| {
                building.mission.current().known() == Some(MissionType::Repair)
                    && building.mission.handler_state() == 2
            })
            && sim.substrate.entities.get(tank).is_some_and(|unit| {
                unit.navigation.nav_com.is_none()
                    && unit
                        .locomotor
                        .as_ref()
                        .is_some_and(|loco| !loco.is_powered())
            })
    }

    fn pos(sim: &Simulation, tank: u64) -> (u16, u16) {
        let e = sim.substrate.entities.get(tank).unwrap();
        (e.position.rx, e.position.ry)
    }

    /// Production path: the RepairAtDepot command queues Enter and runs the
    /// Unit setter with the depot. The first order finds the depot free: its
    /// DOCKING links it (`0x00741DD6`, `0x0043C8A4..C8C3`) and it drives for
    /// the pad. The later two find a contact held (`0x00741D22`): each takes
    /// the depot as its pending entry with no destination and no archive
    /// (`0x00741D9F`, `0x00742C52`). No stored queue.
    #[test]
    fn depot_order_links_a_free_depot_and_pends_the_rest() {
        let (mut sim, rules) = setup(3);
        for tank in 1..=3 {
            assert!(order_repair(&mut sim, &rules, tank));
            let e = sim.substrate.entities.get(tank).unwrap();
            assert_eq!(
                e.mission.queued().known(),
                Some(MissionType::Enter),
                "player depot order queues Enter (7) through the exact authority"
            );
        }
        assert!(linked(&sim, 1));
        assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().navigation.nav_com,
            Some(NavTargetRef::Building { id: DEPOT })
        );
        for tank in 2..=3 {
            let e = sim.substrate.entities.get(tank).unwrap();
            assert!(!linked(&sim, tank));
            assert_eq!(e.pending_entry(), Some(DEPOT));
            assert_eq!(e.navigation.nav_com, None);
            assert_eq!(e.archive_target(), None);
        }
        let depot = sim.substrate.entities.get(DEPOT).unwrap();
        assert_eq!(
            depot.radio_contacts.capacity(),
            1,
            "NumberOfDocks default 1"
        );
        assert_eq!(depot.radio_contacts.len(), 1);
        tick_units(&mut sim, &rules);
        for tank in 2..=3 {
            let pending = sim.substrate.entities.get(tank).unwrap().pending_entry();
            assert_eq!(pending, Some(DEPOT), "the slot stays held");
            let timer = sim
                .substrate
                .entities
                .get(tank)
                .unwrap()
                .mission
                .dispatch_timer();
            assert_eq!(timer.start_frame(), sim.session.binary_frame as i32);
            assert!((14..=16).contains(&timer.delay()));
        }
    }

    /// Each Enter dispatch draws exactly one `RandomRanged(0,2)` on the
    /// Scenario stream (the Mission_Enter epilogue), none between dispatches.
    #[test]
    fn waiter_probe_draws_one_scenario_random_per_dispatch() {
        let (mut sim, rules) = setup(2);
        for tank in 1..=2 {
            assert!(order_repair(&mut sim, &rules, tank));
        }
        // Frame 1: the contact probes and the pending unit dispatches (two
        // draws).
        let mut shadow = sim.clone_scenario_rng();
        tick_units(&mut sim, &rules);
        shadow.next_range_u32_inclusive(0, 2);
        shadow.next_range_u32_inclusive(0, 2);
        assert_eq!(
            sim.scenario_rng.next_range_u32_inclusive(0, 1000),
            shadow.next_range_u32_inclusive(0, 1000),
            "two dispatches ⇒ two RandomRanged(0,2) draws"
        );
    }

    /// Between two of the contact's probes nothing draws.
    #[test]
    fn contact_probe_draws_nothing_between_dispatches() {
        let (mut sim, rules) = setup(1);
        assert!(order_repair(&mut sim, &rules, 1));
        tick_units(&mut sim, &rules);
        let mut shadow = sim.clone_scenario_rng();
        let due = {
            let timer = sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer();
            (timer.start_frame() + timer.delay()) as u32
        };
        for _ in 0..64 {
            if sim.session.binary_frame + 1 >= due {
                break;
            }
            tick_units(&mut sim, &rules);
        }
        assert_eq!(
            sim.scenario_rng.next_range_u32_inclusive(0, 1000),
            shadow.next_range_u32_inclusive(0, 1000),
            "no draw between dispatches"
        );
    }

    /// The `Mission_Enter` epilogue draw sits in the unit's OWN object-AI
    /// slot, interleaved in live-object order with other objects' dispatch
    /// draws: visiting the contact 1 draws exactly one `RandomRanged(0,2)`,
    /// visiting the Guard tank 2 between them draws its own Guard cadence
    /// jitter, visiting the pending unit 3 draws one more `(0,2)`, and the
    /// the repair body draws nothing in its admitted service arm.
    #[test]
    fn waiter_probe_draw_sits_in_the_units_own_ai_slot_in_object_order() {
        let (mut sim, rules) = setup(3);
        assert!(order_repair(&mut sim, &rules, 1));
        assert!(order_repair(&mut sim, &rules, 3));
        // Tank 2 idles on Guard with a due dispatch timer.
        let now = sim.session.binary_frame;
        sim.mission_assign_exact(2, MissionId::from_known(MissionType::Guard), now)
            .expect("assign Guard");
        sim.session.binary_frame += 1;
        let ctx = crate::sim::world::ObjectAiCtx::default;

        let mut shadow = sim.clone_scenario_rng();
        sim.object_ai_visit_one(1, Some(&rules), ctx());
        shadow.next_range_u32_inclusive(0, 2);
        assert_eq!(
            sim.scenario_rng.state(),
            shadow.state(),
            "contact 1: exactly one (0,2) draw inside its own AI slot"
        );

        let before_guard = sim.scenario_rng.state();
        sim.object_ai_visit_one(2, Some(&rules), ctx());
        assert_ne!(
            sim.scenario_rng.state(),
            before_guard,
            "the Guard tank's dispatch draws between the two"
        );

        let mut shadow = sim.clone_scenario_rng();
        sim.object_ai_visit_one(3, Some(&rules), ctx());
        shadow.next_range_u32_inclusive(0, 2);
        assert_eq!(
            sim.scenario_rng.state(),
            shadow.state(),
            "pending 3: one (0,2) draw after the Guard tank's"
        );

        let after_ai = sim.scenario_rng.state();
        sim.mission_assign_exact(DEPOT, MissionId::from_known(MissionType::Repair), 1)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(DEPOT)
            .unwrap()
            .mission
            .set_handler_state(2);
        sim.substrate
            .entities
            .get_mut(DEPOT)
            .unwrap()
            .mission_leaf
            .start_building_repair_progress(1);
        assert_eq!(mission_repair(&mut sim, &rules, DEPOT, None), Some(1));
        assert_eq!(
            sim.scenario_rng.state(),
            after_ai,
            "the Building repair body draws nothing"
        );
        assert!(linked(&sim, 1), "the first order holds the slot");
        assert!(!linked(&sim, 3));
    }

    /// Original Enter/DOCKING never installs a depot MOVE_HERE; a Drive
    /// unit without NavCom keeps it unset. Hover's PerCell restoration is a
    /// distinct original GUID742 branch, pinned by the native control corpus.
    #[test]
    fn a_linked_drive_without_nav_com_is_not_given_a_second_destination() {
        let (mut sim, rules) = setup(1);
        assert!(order_repair(&mut sim, &rules, 1));
        tick_units(&mut sim, &rules);
        assert!(linked(&sim, 1));
        sim.assign_null_destination(1, Some(&rules), None);
        sim.substrate.entities.get_mut(1).unwrap().movement_target = None;
        for _ in 0..100 {
            tick_units(&mut sim, &rules);
            let unit = sim.substrate.entities.get(1).unwrap();
            assert_eq!(unit.navigation.nav_com, None);
            assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
        }
    }

    /// Release drives the repaired unit off the pad to the foundation exit
    /// list's first enterable cell — (0, 3) below the NW corner for a 3x3 —
    /// with BREAK on both ends and a queued Move mission.
    #[test]
    fn repaired_unit_is_released_to_the_native_exit_cell_and_pad_is_vacated() {
        let (mut sim, rules) = setup(1);
        assert!(order_repair(&mut sim, &rules, 1));
        let pad = crate::sim::movement::building_dock_cell(
            &sim.substrate.entities,
            DEPOT,
            Some(1),
            sim.resolved_terrain.as_ref(),
            &rules,
            &sim.interner,
        )
        .unwrap();
        let mut reached_pad = false;
        let mut released = false;
        for _ in 0..2000 {
            tick(&mut sim, &rules);
            if servicing(&sim, 1) {
                assert_eq!(pos(&sim, 1), pad);
                reached_pad = true;
                assert!(
                    !sim.substrate
                        .entities
                        .get(1)
                        .unwrap()
                        .locomotor
                        .as_ref()
                        .unwrap()
                        .powered
                );
            }
            if reached_pad && !linked(&sim, 1) {
                released = true;
                break;
            }
        }
        assert!(reached_pad && released);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(e.health.current, 300);
        assert!(!linked(&sim, 1));
        assert!(
            e.locomotor.as_ref().unwrap().powered,
            "release restores power before the departure order"
        );
        assert_eq!(e.mission.queued().known(), Some(MissionType::Move));
        // MissionRepairAndProduce 0x0044C496: the class setter's NavCom; the
        // route is the next Process's Find_Path.
        let exit = (DEPOT_RX, DEPOT_RY + 3);
        assert_eq!(
            e.navigation.nav_com,
            Some(NavTargetRef::cell(exit.0, exit.1))
        );
        for _ in 0..200 {
            tick(&mut sim, &rules);
        }
        assert_eq!(
            pos(&sim, 1),
            exit,
            "pad vacated, unit parked on the exit cell"
        );
        assert!(
            sim.substrate
                .entities
                .get(DEPOT)
                .unwrap()
                .radio_contacts
                .is_empty()
        );
    }

    /// A depot whose online latch a Temporal warp cleared answers the probe
    /// with 10 before any other test (`0x0043C7FB`): a damaged linked waiter
    /// BREAKs and goes idle on its next probe.
    #[test]
    fn a_warped_depot_turns_its_waiters_away() {
        let (mut sim, rules) = setup(1);
        assert!(order_repair(&mut sim, &rules, 1));
        tick(&mut sim, &rules);
        assert!(linked(&sim, 1));
        let nav = sim.substrate.entities.get(1).unwrap().navigation.nav_com;
        assert!(nav.is_some());
        sim.substrate.entities.get_mut(DEPOT).unwrap().temporal =
            crate::sim::temporal::TemporalState::warped_by_for_test(999);
        let due = {
            let timer = sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer();
            (timer.start_frame() + timer.delay()) as u32
        };
        for _ in 0..64 {
            if sim.session.binary_frame >= due {
                break;
            }
            tick(&mut sim, &rules);
        }
        assert!(!linked(&sim, 1));
        assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .queued()
                .known(),
            Some(MissionType::Move)
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().navigation.nav_com,
            nav
        );
    }

    /// A linked unit that is already at full health answers 0x22 with 10, so
    /// its own probe returns 10: BREAK + Enter_Idle_Mode retains NavCom and
    /// queues Move, with no scatter.
    #[test]
    fn linked_full_health_waiter_breaks_and_goes_idle_on_its_next_probe() {
        let (mut sim, rules) = setup(2);
        for tank in 1..=2 {
            assert!(order_repair(&mut sim, &rules, tank));
        }
        tick(&mut sim, &rules);
        assert!(linked(&sim, 1));
        let nav = sim.substrate.entities.get(1).unwrap().navigation.nav_com;
        assert!(nav.is_some());
        sim.substrate.entities.get_mut(1).unwrap().health.current = 300;
        let due = {
            let timer = sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer();
            (timer.start_frame() + timer.delay()) as u32
        };
        for _ in 0..64 {
            if sim.session.binary_frame >= due {
                break;
            }
            tick(&mut sim, &rules);
        }
        assert!(!linked(&sim, 1));
        assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .queued()
                .known(),
            Some(MissionType::Move)
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().navigation.nav_com,
            nav
        );
        // The slot is free for the next waiter's probe.
        let due2 = {
            let timer = sim
                .substrate
                .entities
                .get(2)
                .unwrap()
                .mission
                .dispatch_timer();
            (timer.start_frame() + timer.delay()) as u32
        };
        for _ in 0..64 {
            if sim.session.binary_frame >= due2 {
                break;
            }
            tick(&mut sim, &rules);
        }
        assert!(linked(&sim, 2));
    }

    // --- A harvester at the depot ---
    //
    // Native dispatches a miner on Enter(7) through `FootClass::Mission_Enter
    // @ 0x004D9290` like any Foot object (`UnitClass` vtable `0x007F5C70 +
    // 0x240` = `0x007F5EB0` holds `0x004D9290`; the Harvest handler is only
    // reached on selector 10). The Unit Enter arm therefore runs for a miner
    // linked to a depot, and the Harvest handler declines it.

    fn miner_cfg() -> crate::sim::miner::MinerConfig {
        crate::sim::miner::MinerConfig::default()
    }

    fn spawn_damaged_miner(sim: &mut Simulation, sid: u64, rx: u16, ry: u16) {
        spawn_entity(sim, sid, "HARV", EntityCategory::Unit, rx, ry, 100);
        let e = sim.substrate.entities.get_mut(sid).unwrap();
        e.miner = Some(crate::sim::miner::Miner::new(
            crate::sim::miner::MinerKind::War,
            &miner_cfg(),
            0,
        ));
    }

    /// The object-AI pass with the Harvest handler LIVE (`miner_config`
    /// supplied, as production does), in live-object order.
    fn visit_units_with_harvest(sim: &mut Simulation, rules: &RuleSet) {
        let units: Vec<u64> = sim
            .substrate
            .entities
            .keys_sorted()
            .into_iter()
            .filter(|&id| {
                sim.substrate
                    .entities
                    .get(id)
                    .is_some_and(|e| e.category == EntityCategory::Unit)
            })
            .collect();
        let cfg = miner_cfg();
        for id in units {
            sim.object_ai_visit_one(
                id,
                Some(rules),
                crate::sim::world::ObjectAiCtx {
                    overlay_registry: None,
                    terrain_spawner_cells: None,
                    miner_config: Some(&cfg),
                },
            );
        }
    }

    fn dispatch_timer(sim: &Simulation, id: u64) -> (i32, i32) {
        let t = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .mission
            .dispatch_timer();
        (t.start_frame(), t.delay())
    }

    /// Original Foot Enter4D9425..4D9497: no contact/archive/pending entry
    /// enters Unit idle, commences Guard, then samples Guard's Rate.
    /// Executed control: refinery_dock.json shared_enter_idle_plain.
    #[test]
    fn enter_without_a_contact_commences_idle_before_its_rate_draw() {
        let (mut sim, mut rules) = setup(1);
        rules.mission_control = crate::sim::mission::MissionControl::from_ini(&IniFile::from_str(
            "[Enter]\nRate=.1\n[Guard]\nRate=.03\n",
        ));
        sim.scenario_rng = crate::sim::rng::SimRng::new(1);
        sim.mission_assign_exact(1, MissionId::from_known(MissionType::Enter), 0)
            .unwrap();

        let delay = crate::sim::mission::enter::mission_enter(&mut sim, &rules, 1);

        let unit = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            unit.mission.current(),
            MissionId::from_known(MissionType::Guard)
        );
        assert_eq!(unit.mission.queued(), MissionId::NONE);
        assert_eq!(delay, 27, "native Guard base26 plus seed1 draw1");
    }

    fn harvest_cursor(sim: &Simulation, id: u64) -> u32 {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .mission
            .handler_state()
    }

    /// A damaged war miner ordered to the depot HELLOs on its Enter cadence
    /// (one Scenario `(0,2)` draw per dispatch, none between), links, is
    /// serviced to full and released with the Harvest cursor untouched. The
    /// dispatch timer has one writer: it moves only on frames where it was
    /// due at entry, and every write is the Enter epilogue (14..=16), never
    /// the Harvest handler's per-frame `1`.
    #[test]
    fn damaged_miner_at_depot_probes_links_is_serviced_and_released() {
        let (mut sim, rules) = setup(0);
        const MINER: u64 = 7;
        spawn_damaged_miner(&mut sim, MINER, 14, 11);
        assert!(order_repair(&mut sim, &rules, MINER));
        let cursor_at_order = harvest_cursor(&sim, MINER);

        let mut reached_pad = false;
        let mut released = false;
        let mut writes = 0u32;
        let mut prev_timer = dispatch_timer(&sim, MINER);
        for _ in 0..2000 {
            sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
            let now = sim.session.binary_frame;
            let due_at_entry = sim
                .substrate
                .entities
                .get(MINER)
                .unwrap()
                .mission
                .dispatch_timer()
                .due(now);
            let waiting = sim
                .substrate
                .entities
                .get(MINER)
                .unwrap()
                .mission
                .current()
                .known()
                == Some(MissionType::Enter);
            let mut shadow = sim.clone_scenario_rng();
            visit_units_with_harvest(&mut sim, &rules);
            let timer = dispatch_timer(&sim, MINER);
            let still_enter = sim
                .substrate
                .entities
                .get(MINER)
                .unwrap()
                .mission
                .current()
                .known()
                == Some(MissionType::Enter);
            if waiting && still_enter {
                if due_at_entry {
                    // Exactly one Enter dispatch: one (0,2) draw, one write.
                    shadow.next_range_u32_inclusive(0, 2);
                    assert_eq!(sim.scenario_rng.state(), shadow.state());
                    assert_eq!(timer.0, now as i32, "epilogue anchors at now");
                    assert!((14..=16).contains(&timer.1), "Enter cadence, got {timer:?}");
                    writes += 1;
                } else {
                    assert_eq!(sim.scenario_rng.state(), shadow.state(), "no draw");
                    assert_eq!(timer, prev_timer, "no writer on a pending frame");
                }
                assert_eq!(harvest_cursor(&sim, MINER), cursor_at_order);
            }
            prev_timer = timer;
            visit_depot(&mut sim, &rules);
            process_units(&mut sim, &rules);
            sim.session.tick += 1;
            if servicing(&sim, MINER) {
                reached_pad = true;
                assert!(linked(&sim, MINER));
            }
            if reached_pad && !linked(&sim, MINER) {
                released = true;
                break;
            }
        }
        assert!(reached_pad, "miner never reached the pad");
        assert!(released, "miner never released");
        assert!(writes >= 1, "at least the first HELLO dispatch");
        let e = sim.substrate.entities.get(MINER).unwrap();
        assert_eq!(e.health.current, 600);
        assert!(e.miner.is_some(), "miner component survives the depot stay");
        assert!(!linked(&sim, MINER));
        assert_eq!(e.mission.queued().known(), Some(MissionType::Move));
        assert_eq!(harvest_cursor(&sim, MINER), cursor_at_order);
    }

    /// The common selector routes a miner on Enter to the depot body, with
    /// one cadence draw and one timer write; no Harvest cursor step runs.
    #[test]
    fn miner_on_enter_with_a_depot_contact_has_one_dispatch_writer() {
        let (mut sim, rules) = setup(0);
        const MINER: u64 = 7;
        spawn_damaged_miner(&mut sim, MINER, 14, 11);
        assert!(order_repair(&mut sim, &rules, MINER));
        sim.session.binary_frame += 1;
        visit_units_with_harvest(&mut sim, &rules);
        assert!(linked(&sim, MINER));
        assert_eq!(
            sim.substrate
                .entities
                .get(MINER)
                .unwrap()
                .mission
                .current()
                .known(),
            Some(MissionType::Enter)
        );
        let (start, delay) = dispatch_timer(&sim, MINER);
        assert!((14..=16).contains(&delay));
        // Jump to the due frame.
        sim.session.binary_frame = (start + delay) as u32;
        let now = sim.session.binary_frame;
        let mut shadow = sim.clone_scenario_rng();
        visit_units_with_harvest(&mut sim, &rules);
        shadow.next_range_u32_inclusive(0, 2);
        assert_eq!(sim.scenario_rng.state(), shadow.state(), "one (0,2) draw");
        let after = dispatch_timer(&sim, MINER);
        assert_eq!(after.0, now as i32);
        assert!(
            (14..=16).contains(&after.1),
            "Enter epilogue, got {after:?}"
        );
    }

    /// The class entry owner, rather than a grid proxy, decides every saved row.
    #[test]
    fn exit_cell_skips_blocked_cells_in_list_order() {
        let (mut sim, rules) = setup(1);
        assert_eq!(
            building_dock_cell(&sim, DEPOT, 1, (0, 0), &rules, None),
            Some((DEPOT_RX, DEPOT_RY + 3))
        );
        spawn_tank(&mut sim, 7, DEPOT_RX, DEPOT_RY + 3);
        assert_eq!(
            building_dock_cell(&sim, DEPOT, 1, (0, 0), &rules, None),
            Some((DEPOT_RX + 1, DEPOT_RY + 3))
        );
    }

    /// Original44C0BD skips reconstruction when state1 has neither service
    /// animation. Exercise real Anim constructors with RandomRate so both
    /// animation identity and the shared Scenario stream expose regressions.
    #[test]
    fn no_contact_repair_preserves_idle_animation_identity() {
        use crate::rules::art_data::ArtRegistry;
        let (mut sim, _) = setup(0);
        let art_ini = IniFile::from_str(
            "[GADEPT]\nFoundation=3x3\nActiveAnim=ACTIVE\nSpecialAnimThree=IDLE\n\
             [ACTIVE]\nLoopCount=-1\nRandomRate=150,450\n\
             [IDLE]\nLoopCount=-1\nRandomRate=150,450\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[BuildingTypes]\n0=GADEPT\n[GADEPT]\nStrength=1000\nUnitRepair=yes\n\
                 [Animations]\n0=ACTIVE\n1=IDLE\n",
            ),
            &art_ini,
        )
        .unwrap();
        let mut art = ArtRegistry::from_ini(&art_ini);
        for name in ["ACTIVE", "IDLE"] {
            art.bind_anim_frame_count_for_test(name, 40);
        }
        rules.install_art_data(art);
        let active = sim
            .set_building_anim_slot(DEPOT, 3, false, false, 0, &rules)
            .unwrap();
        let idle = sim
            .set_building_anim_slot(DEPOT, 12, false, false, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(DEPOT)
            .unwrap()
            .mission
            .set_handler_state(1);
        let rng = sim.scenario_rng.state();
        assert_eq!(mission_repair(&mut sim, &rules, DEPOT, None), Some(1));
        let slots = &sim
            .substrate
            .entities
            .get(DEPOT)
            .unwrap()
            .building_anim_slots;
        assert_eq!((slots[3], slots[12]), (Some(active), Some(idle)));
        assert_eq!(sim.scenario_rng.state(), rng);
    }

    /// Logic55AFB0 finishes Building visits before Factory55B66A. Both
    /// requests can spend the supplied two credits individually; the depot
    /// must heal first and the factory must then hold, within advance_tick.
    #[test]
    fn depot_payment_precedes_the_same_frame_factory_charge() {
        use crate::sim::production::ProductionCategory;
        use crate::sim::timer::CdTimer;
        let (mut sim, _) = setup(1);
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[General]\nRepairPercent=15%\nRepairStep=8\nURepairRate=.016\n\
                 [VehicleTypes]\n0=MTNK\n1=PENDING\n\
                 [MTNK]\nCost=700\nStrength=300\nTechLevel=1\n\
                 [PENDING]\nCost=108\nStrength=100\nTechLevel=1\n\
                 [BuildingTypes]\n0=GADEPT\n1=FACTORY\n\
                 [GADEPT]\nStrength=1000\nUnitRepair=yes\nHasStupidGuardMode=no\n\
                 [FACTORY]\nStrength=500\nFactory=UnitType\n",
            ),
            &IniFile::from_str("[GADEPT]\nFoundation=3x3\n"),
        )
        .unwrap();
        let owner = sim.substrate.entities.get(1).unwrap().owner();
        sim.houses.get_mut(&owner).unwrap().economy.credits = 2;
        spawn_entity(
            &mut sim,
            501,
            "FACTORY",
            EntityCategory::Structure,
            18,
            11,
            500,
        );
        link_for_service(&mut sim, 1);
        sim.mission_assign_exact(DEPOT, MissionId::from_known(MissionType::Repair), 0)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(DEPOT)
            .unwrap()
            .mission
            .set_handler_state(1);
        sim.mission_assign_exact(1, MissionId::from_known(MissionType::Sleep), 0)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .power_off();
        let pending = sim.interner.intern("PENDING");
        assert!(sim.production.factory_shadow.test_enqueue_kernel(
            owner,
            ProductionCategory::Vehicle,
            pending,
            1,
            108,
        ));
        let factory = sim.production.factory_shadow.test_first_mut().unwrap();
        factory.step_rate_frames = 1;
        factory.step_timer = CdTimer::from_raw(0, 0);
        for id in [DEPOT, 1, 501] {
            assert!(sim.register_live_object(id));
        }
        sim.advance_tick(&[], Some(&rules), None, None, 66);
        assert_eq!(sim.substrate.entities.get(1).unwrap().health.current, 108);
        assert_eq!(sim.houses[&owner].economy.credits, 0);
        assert_eq!(sim.houses[&owner].economy.spent_credits, 2);
        let factory = sim.production.factory_shadow.test_first_mut().unwrap();
        assert_eq!(factory.progress, 0);
        assert_eq!(factory.balance, 108);
        assert!(factory.on_hold);
    }

    /// A repaired (full-HP) linked waiter that is a Harvester=yes unit takes
    /// the harvester arm of `Enter_Idle_Mode @ 0x00738970` after the BREAK
    /// (`FootClass::Mission_Enter` 0x004D92E2, args (0, 1)): Harvest for a
    /// non-human house regardless of land, Guard for a human house standing
    /// off ore. This fixture has no NavCom; an installed destination selects
    /// Move first for both plain units and harvesters.
    #[test]
    fn linked_full_health_miner_waiter_takes_the_harvester_idle_arm() {
        use crate::sim::miner::{Miner, MinerConfig, MinerKind};
        use crate::sim::mission::MissionId;

        fn build(human: bool) -> (Simulation, RuleSet) {
            let (mut sim, rules) = setup(0);
            if human {
                let owner_id = sim.interner.intern("Americans");
                sim.houses.get_mut(&owner_id).unwrap().is_human = true;
            }
            spawn_entity(&mut sim, 1, "HARV", EntityCategory::Unit, 14, 11, 600);
            {
                let unit = sim.substrate.entities.get_mut(1).unwrap();
                unit.miner = Some(Miner::new(MinerKind::War, &MinerConfig::default(), 0));
                unit.mark_live_contact_with(DEPOT);
            }
            sim.substrate
                .entities
                .get_mut(DEPOT)
                .unwrap()
                .mark_live_contact_with(1);
            sim.mission_assign_exact(1, MissionId::from_known(MissionType::Enter), 0)
                .expect("unit exists");
            assert!(linked(&sim, 1));
            (sim, rules)
        }

        let (mut sim, rules) = build(false);
        crate::sim::mission::enter::mission_enter(&mut sim, &rules, 1);
        assert!(!linked(&sim, 1));
        visit_depot(&mut sim, &rules);
        assert_eq!(sim.substrate.entities.get(1).unwrap().pending_entry(), None);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.queued(),
            MissionId::from_known(MissionType::Harvest),
            "a non-human house's miner always re-queues Harvest"
        );

        let (mut sim, rules) = build(true);
        crate::sim::mission::enter::mission_enter(&mut sim, &rules, 1);
        assert!(!linked(&sim, 1));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().mission.queued(),
            MissionId::from_known(MissionType::Guard),
            "a human house's miner standing off ore parks on Guard"
        );
    }
}
