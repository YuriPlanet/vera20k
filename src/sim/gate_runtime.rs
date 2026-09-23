//! Native `Gate=yes` building runtime.
//!
//! This module owns the small mission `0x18` state machine used by building
//! gates. Movement requests opening; the per-tick runtime advances the helper
//! phase, holds stable-open gates while live occupants remain in the footprint,
//! then closes them using parsed rules timings.

use crate::map::entities::EntityCategory;
use crate::map::houses::{HouseAllianceMap, are_houses_friendly};
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::{BuildingGateMissionState, BuildingGatePhase, BuildingGateRuntime};
use crate::sim::intern::StringInterner;
use crate::sim::mission::MissionTimer;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;

fn seed_hold_timer(gate: &mut BuildingGateRuntime, ticks: u32, binary_frame: u32) {
    // arm(now, n) == (start_frame=now, duration=n): the same pair the old
    // (hold_last_frame, hold_ticks_remaining) assignment held.
    gate.hold_timer.arm(binary_frame, ticks);
}

fn start_opening(gate: &mut BuildingGateRuntime, ticks: u32, binary_frame: u32) {
    if gate.phase == BuildingGatePhase::OpenStable {
        return;
    }
    if ticks == 0 {
        gate.phase = BuildingGatePhase::OpenStable;
        gate.transition_timer.defer(binary_frame, 0);
        gate.transition_total_ticks = 0;
        return;
    }
    gate.phase = BuildingGatePhase::Opening;
    gate.transition_timer.defer(binary_frame, ticks);
    gate.transition_total_ticks = ticks;
}

fn start_closing(gate: &mut BuildingGateRuntime, ticks: u32, binary_frame: u32) {
    if gate.phase == BuildingGatePhase::ClosedStable {
        return;
    }
    if ticks == 0 {
        gate.phase = BuildingGatePhase::ClosedStable;
        gate.transition_timer.defer(binary_frame, 0);
        gate.transition_total_ticks = 0;
        return;
    }
    gate.phase = BuildingGatePhase::Closing;
    gate.transition_timer.defer(binary_frame, ticks);
    gate.transition_total_ticks = ticks;
}

fn reverse_transition(gate: &mut BuildingGateRuntime, binary_frame: u32) {
    if !matches!(
        gate.phase,
        BuildingGatePhase::Opening | BuildingGatePhase::Closing
    ) {
        return;
    }
    // Recompute the reversed remaining from the nominal total, leaving the
    // native start-frame baseline untouched (same field math as before).
    let elapsed = gate.transition_timer.elapsed(binary_frame);
    let live_remaining = gate.transition_timer.duration.saturating_sub(elapsed);
    gate.transition_timer.duration = gate.transition_total_ticks.saturating_sub(live_remaining);
    gate.phase = match gate.phase {
        BuildingGatePhase::Opening => BuildingGatePhase::Closing,
        BuildingGatePhase::Closing => BuildingGatePhase::Opening,
        stable => stable,
    };
}

fn advance_hold(timer: &mut MissionTimer, binary_frame: u32) -> bool {
    // Re-anchor to now and saturating-decrement — the exact field math the old
    // advance_remaining performed on (hold_ticks_remaining, hold_last_frame).
    let elapsed = binary_frame.wrapping_sub(timer.start_frame);
    timer.start_frame = binary_frame;
    timer.duration = timer.duration.saturating_sub(elapsed);
    timer.duration == 0
}

fn advance_transition(gate: &mut BuildingGateRuntime, binary_frame: u32) {
    if !matches!(
        gate.phase,
        BuildingGatePhase::Opening | BuildingGatePhase::Closing
    ) {
        return;
    }
    // `due` is the exact complement of the old `elapsed < remaining` early-out.
    if !gate.transition_timer.due(binary_frame) {
        return;
    }
    gate.transition_timer.duration = 0;
    gate.phase = match gate.phase {
        BuildingGatePhase::Opening => BuildingGatePhase::OpenStable,
        BuildingGatePhase::Closing => BuildingGatePhase::ClosedStable,
        stable => stable,
    };
}

fn footprint_has_other_live_object(
    gate_id: u64,
    origin: (u16, u16),
    foundation: &str,
    occupancy: &OccupancyGrid,
) -> bool {
    crate::sim::production::building_base_foundation_cells(origin.0, origin.1, foundation)
        .into_iter()
        .any(|(rx, ry)| {
            occupancy.get(rx, ry).is_some_and(|occ| {
                occ.occupants
                    .iter()
                    .any(|occupant| occupant.entity_id != gate_id)
            })
        })
}

pub fn request_open(gate: &mut BuildingGateRuntime) {
    if gate.mission_18_active
        && matches!(
            gate.phase,
            BuildingGatePhase::Opening | BuildingGatePhase::OpenStable
        )
    {
        return;
    }
    gate.mission_18_active = true;
    gate.mission_state = BuildingGateMissionState::Setup;
}

#[allow(clippy::too_many_arguments)]
pub fn request_gate_open_for_cell(
    entities: &mut EntityStore,
    occupancy: &OccupancyGrid,
    cell: (u16, u16),
    object_list_layer: MovementLayer,
    mover_id: u64,
    mover_owner: &str,
    rules: &RuleSet,
    alliances: &HouseAllianceMap,
    interner: &StringInterner,
) -> bool {
    let Some(occ) = occupancy.get(cell.0, cell.1) else {
        return false;
    };
    let candidates: Vec<u64> = occ
        .iter_layer(object_list_layer)
        .filter_map(|occupant| (occupant.entity_id != mover_id).then_some(occupant.entity_id))
        .collect();
    let mut requested = false;
    for candidate_id in candidates {
        let Some(candidate) = entities.get(candidate_id) else {
            continue;
        };
        if candidate.category != EntityCategory::Structure {
            continue;
        }
        let Some(obj) = rules.object(interner.resolve(candidate.type_ref())) else {
            continue;
        };
        if !obj.gate
            || !are_houses_friendly(alliances, mover_owner, interner.resolve(candidate.owner()))
        {
            continue;
        }
        let Some(candidate) = entities.get_mut(candidate_id) else {
            continue;
        };
        request_open(candidate.building_gate.get_or_insert_with(Default::default));
        requested = true;
    }
    requested
}

pub fn tick_gate_runtimes(
    entities: &mut EntityStore,
    occupancy: &OccupancyGrid,
    rules: &RuleSet,
    interner: &StringInterner,
    binary_frame: u32,
) {
    // A warped gate's mission 0x18 does not run (`GameEntity::ai_frozen`): it
    // neither opens nor closes.
    let gate_ids: Vec<u64> = entities
        .values()
        .filter(|entity| entity.building_gate.is_some() && !entity.ai_frozen())
        .map(|entity| entity.stable_id())
        .collect();

    for gate_id in gate_ids {
        let Some((origin, type_ref)) = entities
            .get(gate_id)
            .map(|gate| ((gate.position.rx, gate.position.ry), gate.type_ref()))
        else {
            continue;
        };
        let Some(obj) = rules.object(interner.resolve(type_ref)) else {
            continue;
        };
        if !obj.gate {
            continue;
        }
        let obstructed =
            footprint_has_other_live_object(gate_id, origin, &obj.foundation, occupancy);
        let Some(gate) = entities
            .get_mut(gate_id)
            .and_then(|entity| entity.building_gate.as_mut())
        else {
            continue;
        };
        tick_gate(
            gate,
            obj.deploy_time_ticks,
            obj.gate_close_delay_ticks,
            obstructed,
            binary_frame,
        );
    }
}

pub fn tick_gate(
    gate: &mut BuildingGateRuntime,
    deploy_ticks: u32,
    close_delay_ticks: u32,
    obstructed: bool,
    binary_frame: u32,
) {
    advance_transition(gate, binary_frame);
    if !gate.mission_18_active {
        return;
    }

    match gate.mission_state {
        BuildingGateMissionState::Setup => {
            if gate.phase == BuildingGatePhase::OpenStable {
                gate.mission_state = BuildingGateMissionState::OpenHold;
            } else {
                if gate.phase == BuildingGatePhase::Closing {
                    reverse_transition(gate, binary_frame);
                } else if gate.phase != BuildingGatePhase::Opening {
                    start_opening(gate, deploy_ticks, binary_frame);
                }
                gate.mission_state = BuildingGateMissionState::OpeningWait;
            }
            seed_hold_timer(gate, close_delay_ticks, binary_frame);
        }
        BuildingGateMissionState::OpeningWait => {
            if gate.phase == BuildingGatePhase::OpenStable {
                gate.mission_state = BuildingGateMissionState::OpenHold;
            }
        }
        BuildingGateMissionState::OpenHold => {
            if obstructed {
                seed_hold_timer(gate, close_delay_ticks, binary_frame);
            } else if advance_hold(&mut gate.hold_timer, binary_frame) {
                gate.mission_state = BuildingGateMissionState::BeginClose;
            }
        }
        BuildingGateMissionState::BeginClose => {
            start_closing(gate, deploy_ticks, binary_frame);
            gate.mission_state = BuildingGateMissionState::ClosingWait;
        }
        BuildingGateMissionState::ClosingWait => {
            if gate.phase == BuildingGatePhase::ClosedStable {
                gate.mission_state = BuildingGateMissionState::PostClose;
            }
        }
        BuildingGateMissionState::PostClose => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_and_closing_use_deploy_ticks_and_stable_open_is_passable() {
        let mut gate = BuildingGateRuntime::default();
        request_open(&mut gate);
        tick_gate(&mut gate, 3, 10, false, 0);
        assert_eq!(gate.phase, BuildingGatePhase::Opening);
        assert!(!gate.can_garrison_passable());

        tick_gate(&mut gate, 3, 10, false, 2);
        assert_eq!(gate.phase, BuildingGatePhase::Opening);
        tick_gate(&mut gate, 3, 10, false, 3);
        assert_eq!(gate.phase, BuildingGatePhase::OpenStable);
        assert!(gate.can_garrison_passable());
    }

    #[test]
    fn hold_reseeds_while_obstructed_then_closes_after_clear_delay() {
        let mut gate = BuildingGateRuntime::default();
        request_open(&mut gate);
        tick_gate(&mut gate, 1, 4, false, 0);
        tick_gate(&mut gate, 1, 4, false, 1);
        assert_eq!(gate.phase, BuildingGatePhase::OpenStable);

        tick_gate(&mut gate, 1, 4, true, 4);
        assert_eq!(gate.hold_timer.duration, 4);
        tick_gate(&mut gate, 1, 4, false, 7);
        assert_eq!(gate.mission_state, BuildingGateMissionState::OpenHold);
        tick_gate(&mut gate, 1, 4, false, 8);
        assert_eq!(gate.mission_state, BuildingGateMissionState::BeginClose);
        tick_gate(&mut gate, 1, 4, false, 9);
        assert_eq!(gate.phase, BuildingGatePhase::Closing);
        assert!(!gate.can_garrison_passable());
        tick_gate(&mut gate, 1, 4, false, 10);
        assert_eq!(gate.phase, BuildingGatePhase::ClosedStable);
    }

    #[test]
    fn closed_or_closing_request_restarts_mission_setup() {
        let mut gate = BuildingGateRuntime {
            mission_18_active: true,
            phase: BuildingGatePhase::Closing,
            mission_state: BuildingGateMissionState::ClosingWait,
            transition_timer: MissionTimer::armed(20, 2),
            transition_total_ticks: 4,
            ..Default::default()
        };
        request_open(&mut gate);
        assert_eq!(gate.mission_state, BuildingGateMissionState::Setup);
        tick_gate(&mut gate, 4, 10, false, 20);
        assert_eq!(gate.phase, BuildingGatePhase::Opening);
        assert_eq!(gate.transition_timer.duration, 2);
        assert_eq!(gate.transition_timer.start_frame, 20);
    }

    #[test]
    fn closing_rerequest_preserves_native_start_frame_baseline() {
        let mut gate = BuildingGateRuntime {
            mission_18_active: true,
            phase: BuildingGatePhase::Closing,
            mission_state: BuildingGateMissionState::ClosingWait,
            transition_timer: MissionTimer::armed(100, 39),
            transition_total_ticks: 39,
            ..Default::default()
        };

        request_open(&mut gate);
        assert_eq!(gate.mission_state, BuildingGateMissionState::Setup);

        tick_gate(&mut gate, 39, 180, false, 110);
        assert_eq!(gate.phase, BuildingGatePhase::Opening);
        assert_eq!(gate.transition_timer.duration, 10);
        assert_eq!(gate.transition_timer.start_frame, 100);
        assert!(!gate.can_garrison_passable());

        tick_gate(&mut gate, 39, 180, false, 111);
        assert_eq!(gate.phase, BuildingGatePhase::OpenStable);
        assert!(gate.can_garrison_passable());
    }

    #[test]
    fn friendly_gate_cell_request_assigns_open_mission() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};

        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAGATE_A\n\
             [MTNK]\nName=Tank\nSpeed=4\n\
             [GAGATE_A]\nName=Allied Gate\nFoundation=3x1\nGate=yes\nDeployTime=.044\nGateCloseDelay=.2\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("gate rules");
        let alliances = Default::default();

        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(1, "MTNK", "Americans", 8, 10);
        mover.category = EntityCategory::Unit;
        entities.insert(mover);
        let mut gate = GameEntity::test_default(100, "GAGATE_A", "Americans", 10, 10);
        gate.category = EntityCategory::Structure;
        gate.building_gate = Some(BuildingGateRuntime::default());
        entities.insert(gate);

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            10,
            10,
            100,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let interner = crate::sim::intern::test_interner();

        assert!(request_gate_open_for_cell(
            &mut entities,
            &occupancy,
            (10, 10),
            MovementLayer::Ground,
            1,
            "Americans",
            &rules,
            &alliances,
            &interner,
        ));

        let gate = entities.get(100).unwrap().building_gate.unwrap();
        assert!(gate.mission_18_active);
        assert_eq!(gate.phase, BuildingGatePhase::ClosedStable);
        assert_eq!(gate.mission_state, BuildingGateMissionState::Setup);
    }
}
