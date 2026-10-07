//! Building Gate admission and Mission_Open (0x00452540, 0x0044E440).
//!
//! MissionCom owns Open24 and its handler cursor. Shared Techno Door+350
//! owns transitions and completes in the object's Techno AI turn. Only the
//! separate Building +604 hold clock belongs to the Gate owner.

use std::hash::{Hash, Hasher};

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::door::DoorPhase;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionTimer, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

/// Stable-open hold timer (+604), distinct from Door+350 and dispatch+C8.
/// Every Gate seed writes duration+60C and nominal+610 to the same value;
/// subsequent reads leave both untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingGateRuntime {
    hold_timer: MissionTimer,
}

impl Default for BuildingGateRuntime {
    fn default() -> Self {
        Self::at_frame(0)
    }
}

impl BuildingGateRuntime {
    /// Building constructor43B7CB..43B7DD anchors +604 at construction.
    pub(crate) fn at_frame(frame: u32) -> Self {
        Self {
            hold_timer: MissionTimer::armed(frame, 0),
        }
    }

    pub(crate) fn hash_state(self, hasher: &mut impl Hasher) {
        self.hold_timer.duration.hash(hasher);
        self.hold_timer.start_frame.hash(hasher);
    }
}

/// Map578AD0: walk Ground +E4 in list order, skipping the mover. The Gate
/// owner's directional alliance chooses Building452540 or4525F0. Only the
/// fresh Drive/Ship caller consumes false; blocked-step callers ignore it.
pub(crate) fn request_gate_open_for_cell(
    sim: &mut Simulation,
    cell: (u16, u16),
    mover_id: u64,
    rules: &RuleSet,
) -> bool {
    let Some(mover_owner) = sim.substrate.entities.get(mover_id).map(|m| m.owner()) else {
        return true;
    };
    let Some(occ) = sim.substrate.occupancy.get(cell.0, cell.1) else {
        return true;
    };
    let candidates: Vec<u64> = occ
        .iter_layer(MovementLayer::Ground)
        .filter_map(|occupant| (occupant.entity_id != mover_id).then_some(occupant.entity_id))
        .collect();
    for candidate_id in candidates {
        let Some(candidate) = sim.substrate.entities.get(candidate_id) else {
            continue;
        };
        if candidate.category != EntityCategory::Structure
            || !rules
                .object(sim.interner.resolve(candidate.type_ref()))
                .is_some_and(|obj| obj.gate)
        {
            continue;
        }
        if crate::map::houses::is_allied_with(
            &sim.house_alliances,
            sim.interner.resolve(candidate.owner()),
            sim.interner.resolve(mover_owner),
        ) {
            //4525B8..4525D6: Assign(-1), Queue(Open,false), Commence.
            //Read actual MissionCom and Door, never a Gate-local latch.
            if candidate.mission.effective().known() != Some(MissionType::Open)
                || matches!(
                    candidate.door_phase(),
                    DoorPhase::Closing | DoorPhase::ClosedStable
                )
            {
                let now = sim.session.binary_frame;
                let readiness = LiveReadyInputProvider { rules };
                let _ = sim.mission_assign_exact(candidate_id, MissionId::NONE, now);
                let _ = sim.mission_queue_exact(
                    candidate_id,
                    MissionId::from_known(MissionType::Open),
                    0,
                    now,
                    &readiness,
                );
                let _ = sim.mission_commence_exact(candidate_id, now);
                return false;
            }
            return candidate.is_open_gate();
        }
        if candidate.is_open_gate() {
            return true;
        }
    }
    true
}

///44E3A0: GetFoundation(false), then main Ground lists. Any pointer other
///than the Gate obstructs; bridge-layer occupants and Health are not queried.
fn footprint_has_other_object(sim: &Simulation, id: u64, foundation: &str) -> bool {
    let Some(gate) = sim.substrate.entities.get(id) else {
        return false;
    };
    crate::sim::production::building_base_foundation_cells(
        gate.position.rx,
        gate.position.ry,
        foundation,
    )
    .into_iter()
    .any(|(rx, ry)| {
        sim.substrate.occupancy.get(rx, ry).is_some_and(|occ| {
            occ.iter_layer(MovementLayer::Ground)
                .any(|occupant| occupant.entity_id != id)
        })
    })
}

/// Building Mission_Open44E440, called by the existing Health/cadence
/// dispatcher inside this object's Building AI. Shared TechnoAI has already
/// completed a due Door. Executed controls: building_guard_attack oracle.
pub(crate) fn mission_open(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 0;
    };
    let Some(object) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return 0;
    };
    let now = sim.session.binary_frame;
    if !object.gate {
        //44E77C..44E794: non-Gate Open queues Guard without commencing.
        let readiness = LiveReadyInputProvider { rules };
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Guard),
            0,
            now,
            &readiness,
        );
        return 1;
    }
    let status = entity.mission.handler_state();
    let phase = entity.door_phase();
    let deploy_ticks = object.deploy_time_ticks;
    let hold_ticks = object.gate_close_delay_ticks;
    match status {
        0 => {
            //44E46D..44E5CE: open enters hold; otherwise open, retain an
            //opening transition, or reverse a closing one.
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            if phase == DoorPhase::OpenStable {
                entity.mission.set_handler_state(2);
                entity
                    .building_gate
                    .get_or_insert_with(Default::default)
                    .hold_timer
                    .arm(now, hold_ticks);
                return sim.mission_rate_epilogue_for(rules, id, MissionType::Open);
            }
            if phase == DoorPhase::Closing {
                entity.reverse_door(now);
            } else if phase != DoorPhase::Opening {
                entity.open_door(deploy_ticks, now);
            }
            entity.mission.set_handler_state(1);
            entity
                .building_gate
                .get_or_insert_with(Default::default)
                .hold_timer
                .arm(now, hold_ticks);
            0
        }
        1 | 4 => {
            //Jump table44E798: state1→44E6DA, state4→44E6F3; only the
            //opening wait observes stable-open and enters state2.
            if status == 1
                && phase == DoorPhase::OpenStable
                && let Some(entity) = sim.substrate.entities.get_mut(id)
            {
                entity.mission.set_handler_state(2);
            }
            if phase == DoorPhase::ClosedStable {
                //44E70C: canonical Building44D6A0(0,1), then state5.
                //Guard stays queued until the host's Ready check.
                sim.building_enter_idle_mode(id, false, Some(rules));
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    entity.mission.set_handler_state(5);
                }
            }
            //GateStage+703, dirty+80, sound and repaint remain the existing
            //presentation residual; there is no second Door state.
            0
        }
        2 => {
            let obstructed = footprint_has_other_object(sim, id, &object.foundation);
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            let gate = entity.building_gate.get_or_insert_with(Default::default);
            if obstructed {
                //44E69F: reseed; never decrement/reanchor on a read.
                gate.hold_timer.arm(now, hold_ticks);
            } else if gate.hold_timer.due(now) {
                entity.mission.set_handler_state(3);
            }
            sim.mission_rate_epilogue_for(rules, id, MissionType::Open)
        }
        3 => {
            //44E5DA: Close, state4, no cadence RNG.
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.close_door(deploy_ticks, now);
            entity.mission.set_handler_state(4);
            0
        }
        //44E464: states above4 share the cadence exit44E4BA.
        _ => sim.mission_rate_epilogue_for(rules, id, MissionType::Open),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::test_flat_ground_grid;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_interner;
    use crate::sim::mission::MissionDispatchTimer;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::occupancy::CellListInsertion;
    use crate::sim::rng::SimRng;
    use serde_json::{Value, json};

    const GATE: u64 = 100;
    const MOVER: u64 = 1;

    fn native_corpus() -> Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_guard_attack_gate.json",
        ))
        .unwrap()
    }

    fn native_mission(value: &Value) -> MissionId {
        match value.as_str() {
            Some("none") => MissionId::NONE,
            Some("guard") => MissionId::from_known(MissionType::Guard),
            Some(name) => panic!("unknown native mission {name}"),
            None => MissionId::from_raw(value.as_i64().unwrap() as i32),
        }
    }

    /// The supplied original 2x2 Gate at12,12, outside complete class/type
    /// constructors and retail reader closure. Controls bind through the real
    /// Rust INI reader; retained phase/clock/mission inputs are explicit.
    fn native_fixture(row: &Value) -> (Simulation, RuleSet) {
        let input = &row["input"];
        let frame = input["frame"].as_u64().unwrap_or(100) as u32;
        let seed = input["seed"].as_u64().unwrap_or(1);
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Open]\nRate={}\n[Guard]\nRate=.030\nAARate=.016\n\
             [VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GATE\n\
             [MTNK]\nStrength=1000\nSpeed=4\n\
             [GATE]\nStrength=1000\nFoundation=2x2\nGate={}\n\
             DeployTime={}\nGateCloseDelay={}\nHasStupidGuardMode=no\n",
            input["rate"].as_str().unwrap_or(".016"),
            if input["gate"].as_bool().unwrap_or(true) {
                "yes"
            } else {
                "no"
            },
            input["deploy"].as_str().unwrap_or(".044"),
            input["close_delay"].as_str().unwrap_or(".2"),
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(seed);
        sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(24));
        sim.session.binary_frame = frame;
        let owner = sim.interner.intern("Americans");
        let ty = sim.interner.intern("GATE");
        let mut gate = GameEntity::new_at_frame_for_test(
            GATE,
            12,
            12,
            0,
            0,
            owner,
            Health { current: 1000 },
            ty,
            EntityCategory::Structure,
            0,
            0,
            false,
            frame,
        );
        let object = rules.object("GATE").unwrap();
        let phase = input["phase"].as_str().unwrap_or("closed");
        if matches!(phase, "open" | "closing") {
            gate.open_door(0, frame);
            gate.advance_door(frame);
        }
        let door_start = input["door_start"].as_u64().unwrap_or(u64::from(frame)) as u32;
        match phase {
            "opening" => gate.open_door(object.deploy_time_ticks, door_start),
            "closing" => gate.close_door(object.deploy_time_ticks, door_start),
            "closed" | "open" => {}
            other => panic!("unknown native phase {other}"),
        }
        let hold = &row["initial"]["hold"];
        gate.building_gate = Some(BuildingGateRuntime {
            hold_timer: MissionTimer::armed(
                hold[0].as_i64().unwrap() as u32,
                hold[1].as_i64().unwrap() as u32,
            ),
        });
        gate.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(input["current"].as_i64().unwrap_or(24) as i32),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(input["queue"].as_i64().unwrap_or(-1) as i32),
            movement_bypass_latch: 0,
            handler_state: input["status"].as_u64().unwrap() as u32,
            mission_start_frame: frame,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::from_raw(frame as i32, 0),
        });
        gate.mission_leaf
            .set_building_ready_latch(input["ready"].as_u64().unwrap_or(0) as u8);
        // The native fixture supplies an already admitted cell-list object;
        // class construction/reveal is outside this comparison boundary.
        gate.lifecycle.in_limbo = false;
        gate.lifecycle.cell_marked = true;
        gate.in_playfield = true;
        sim.substrate.entities.insert(gate);
        let mover_type = sim.interner.intern("MTNK");
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_for_test(
                MOVER,
                8,
                8,
                0,
                0,
                owner,
                Health { current: 1000 },
                mover_type,
                EntityCategory::Unit,
                0,
                0,
                false,
                frame,
            ));
        // Gate's own pointer appears on all original foundation Ground lists.
        for y in 12..14 {
            for x in 12..14 {
                sim.substrate.occupancy.add(
                    x,
                    y,
                    GATE,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
        }
        if let Some(blocker) = input["blocker"].as_str() {
            sim.substrate
                .entities
                .get_mut(MOVER)
                .unwrap()
                .health
                .current = input["blocker_health"].as_i64().unwrap_or(1000) as i32;
            sim.substrate.occupancy.add(
                12,
                12,
                MOVER,
                if blocker == "ground" {
                    MovementLayer::Ground
                } else {
                    MovementLayer::Bridge
                },
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        sim.main_rng = SimRng::new(seed);
        sim.scenario_rng = SimRng::new(seed);
        sim.mapgen_rng = SimRng::new(seed);
        (sim, rules)
    }

    fn assert_native_state(sim: &Simulation, native: &Value, context: &str) {
        let entity = sim.substrate.entities.get(GATE).unwrap();
        assert_eq!(
            entity.mission.current(),
            native_mission(&native["mission"]),
            "{context}: current"
        );
        assert_eq!(
            entity.mission.queued(),
            native_mission(&native["queued"]),
            "{context}: queue"
        );
        assert_eq!(
            entity.mission.handler_state(),
            native["status"].as_i64().unwrap() as u32,
            "{context}: handler cursor"
        );
        assert_eq!(
            entity.mission.mission_start_frame(),
            native["mission_start"].as_u64().unwrap() as u32,
            "{context}: mission start"
        );
        let phase = match entity.door_phase() {
            DoorPhase::ClosedStable => [0, 0],
            DoorPhase::Opening => [1, 1],
            DoorPhase::OpenStable => [0, 1],
            DoorPhase::Closing => [1, 0],
        };
        assert_eq!(json!(phase), native["phase"], "{context}: Door bytes");
        let door = entity.door_timer_fields();
        assert_eq!(
            json!([door.0, door.1, door.2]),
            native["door"],
            "{context}: Door timer"
        );
        let hold = entity.building_gate.unwrap().hold_timer;
        // Native+610 is a separate nominal value, but every Gate writer
        // seeds it identically with60C and no retained read mutates either.
        assert_eq!(
            json!([
                hold.start_frame as i32,
                hold.duration as i32,
                hold.duration as i32
            ]),
            native["hold"],
            "{context}: hold clock and nominal value"
        );
        let timer = entity.mission.dispatch_timer();
        assert_eq!(
            json!([timer.start_frame(), timer.delay()]),
            native["timer"],
            "{context}: dispatch timer"
        );
        assert_eq!(
            entity.mission.ai_counter(),
            native["counter"].as_u64().unwrap() as u32,
            "{context}: mission counter"
        );
        assert_eq!(
            u64::from(entity.building_ready_latch()),
            native["ready"].as_u64().unwrap(),
            "{context}: ready latch"
        );
    }

    fn assert_native_rng(sim: &Simulation, native: &Value, context: &str) {
        for (name, rng) in [
            ("main", &sim.main_rng),
            ("scenario", &sim.scenario_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert_eq!(
                rng.native_state_hex(),
                native[name],
                "{context}: full {name} state"
            );
        }
    }

    #[test]
    fn original_gate_controls_match_shared_owners_and_complete_rng_states() {
        let corpus = native_corpus();
        let controls = corpus["controls"].as_array().unwrap();
        assert_eq!(controls.len(), 37);
        for row in controls {
            let name = row["input"]["name"].as_str().unwrap();
            let (mut sim, rules) = native_fixture(row);
            assert_native_state(&sim, &row["initial"], name);
            assert_native_rng(&sim, &row["rng_before"], name);
            let result = if row["input"]["operation"] == "admission" {
                u32::from(request_gate_open_for_cell(
                    &mut sim,
                    (12, 12),
                    MOVER,
                    &rules,
                ))
            } else {
                mission_open(&mut sim, GATE, &rules) as u32
            };
            assert_eq!(
                u64::from(result),
                row["returns"].as_u64().unwrap(),
                "{name}: return"
            );
            assert_native_state(&sim, &row["after"], name);
            assert_native_rng(&sim, &row["rng_after"], name);
            assert_eq!(
                sim.substrate
                    .entities
                    .get(GATE)
                    .unwrap()
                    .building_body_state(),
                Some(row["after"]["bstate"].as_i64().unwrap() as i32),
                "{name}: canonical idle body"
            );
        }
    }

    #[test]
    fn original_gate_cadence_matches_the_live_object_turn() {
        let corpus = native_corpus();
        let row = &corpus["cadence"];
        // The cadence's starting hold clock is the fixture's retained input.
        let setup = json!({"input": row["input"], "initial": {"hold": [100,180,180]}});
        let (mut sim, rules) = native_fixture(&setup);
        sim.building_enter_idle_mode(GATE, false, Some(&rules));
        sim.mission_assign_exact(GATE, MissionId::from_known(MissionType::Guard), 100)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(GATE)
            .unwrap()
            .mission_leaf
            .set_building_ready_latch(1);
        sim.set_logic_order_for_test(vec![GATE]);
        assert_eq!(
            u64::from(request_gate_open_for_cell(
                &mut sim,
                (12, 12),
                MOVER,
                &rules
            )),
            row["admission"].as_u64().unwrap()
        );
        assert_native_rng(&sim, &row["rng_before"], "cadence admission");
        sim.session.binary_frame = 101;
        let frames = row["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 350);
        for native in frames {
            let frame = native["frame"].as_u64().unwrap() as u32;
            assert_eq!(sim.session.binary_frame, frame, "frame owner before visit");
            sim.advance_tick(&[], Some(&rules), None, None, 67);
            let context = format!("Gate object turn{frame}");
            assert_native_state(&sim, native, &context);
            let entity = sim.substrate.entities.get(GATE).unwrap();
            assert_eq!(
                i64::from(entity.building_body_state().unwrap()),
                native["bstate"].as_i64().unwrap(),
                "{context}: body"
            );
            assert_eq!(
                i64::from(entity.queued_building_body_state().unwrap()),
                native["queued_bstate"].as_i64().unwrap(),
                "{context}: queued body"
            );
        }
        assert_native_rng(&sim, &row["rng_after"], "cadence final");
    }

    #[test]
    fn gate_persistence_retains_only_shared_door_mission_and_hold_authority() {
        let corpus = native_corpus();
        for name in ["setup_closing_reverses", "hold_expired_seed9"] {
            let row = corpus["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap();
            let (mut sim, rules) = native_fixture(row);
            mission_open(&mut sim, GATE, &rules);
            let entity = sim.substrate.entities.get(GATE).unwrap();
            // GameSnapshot serializes this same entity. This focused wire
            // check deliberately excludes load's process RNG reset/caches.
            let bytes = bincode::serialize(entity).unwrap();
            let restored: GameEntity = bincode::deserialize(&bytes).unwrap();
            assert_eq!(restored.mission, entity.mission, "{name}: MissionCom");
            assert_eq!(
                restored.door_phase(),
                entity.door_phase(),
                "{name}: Door phase"
            );
            assert_eq!(
                restored.door_timer_fields(),
                entity.door_timer_fields(),
                "{name}: Door clock"
            );
            assert_eq!(restored.building_gate, entity.building_gate, "{name}: hold");
            let initial_hold = entity.building_gate;
            let serialized = serde_json::to_value(entity).unwrap();
            assert_eq!(
                serialized["building_gate"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                vec!["hold_timer"],
                "{name}: no copied Door or mission fields"
            );
            let hash = sim.state_hash();
            let timer = &mut sim
                .substrate
                .entities
                .get_mut(GATE)
                .unwrap()
                .building_gate
                .as_mut()
                .unwrap()
                .hold_timer;
            timer.start_frame = timer.start_frame.wrapping_add(1);
            assert_ne!(sim.state_hash(), hash, "{name}: hold anchor in world hash");
            sim.substrate.entities.get_mut(GATE).unwrap().building_gate = initial_hold;
            assert_eq!(sim.state_hash(), hash, "{name}: restored hold");
            let frame = sim.session.binary_frame;
            sim.substrate
                .entities
                .get_mut(GATE)
                .unwrap()
                .close_door(1, frame);
            assert_ne!(sim.state_hash(), hash, "{name}: shared Door in world hash");
        }
    }

    fn fixture() -> (Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAGATE_A\n\
             [MTNK]\nSpeed=4\n\
             [GAGATE_A]\nFoundation=3x1\nGate=yes\nDeployTime=.044\nGateCloseDelay=.2\n",
        ))
        .unwrap();
        let mut sim = Simulation::with_seed(17);
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 8, 10));
        let mut gate = GameEntity::test_default_of_category(
            100,
            "GAGATE_A",
            "Americans",
            10,
            10,
            EntityCategory::Structure,
        );
        gate.building_gate = Some(BuildingGateRuntime::default());
        sim.substrate.entities.insert(gate);
        sim.interner = test_interner();
        sim.substrate.occupancy.add(
            10,
            10,
            100,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        (sim, rules)
    }

    #[test]
    fn allied_gate_request_uses_real_mission_and_read_only_passability() {
        let (mut sim, rules) = fixture();
        let rng = sim.scenario_rng.logical_state();
        assert!(!request_gate_open_for_cell(&mut sim, (10, 10), 1, &rules));
        let gate = sim.substrate.entities.get(100).unwrap();
        assert_eq!(gate.mission.current().known(), Some(MissionType::Open));
        assert_eq!(gate.mission.queued(), MissionId::NONE);
        assert_eq!(gate.mission.handler_state(), 0);
        assert_eq!(gate.door_phase(), DoorPhase::ClosedStable);
        assert!(!gate.is_open_gate());
        assert_eq!(sim.scenario_rng.logical_state(), rng);

        assert_eq!(mission_open(&mut sim, 100, &rules), 0);
        let gate = sim.substrate.entities.get(100).unwrap();
        assert_eq!(gate.mission.handler_state(), 1);
        assert_eq!(gate.door_phase(), DoorPhase::Opening);
        assert_eq!(sim.scenario_rng.logical_state(), rng);
        sim.session.binary_frame = 39;
        sim.substrate
            .entities
            .get_mut(100)
            .unwrap()
            .advance_door(39);
        assert!(request_gate_open_for_cell(&mut sim, (10, 10), 1, &rules));
        assert_eq!(
            sim.substrate
                .entities
                .get(100)
                .unwrap()
                .mission
                .handler_state(),
            1
        );
        assert_eq!(mission_open(&mut sim, 100, &rules), 0);
        assert_eq!(
            sim.substrate
                .entities
                .get(100)
                .unwrap()
                .mission
                .handler_state(),
            2
        );
    }

    #[test]
    fn hold_reads_only_ground_lists_and_preserves_its_anchor() {
        let (mut sim, rules) = fixture();
        request_gate_open_for_cell(&mut sim, (10, 10), 1, &rules);
        let gate = sim.substrate.entities.get_mut(100).unwrap();
        gate.open_door(0, 0);
        gate.advance_door(0);
        mission_open(&mut sim, 100, &rules);
        sim.substrate.occupancy.add(
            11,
            10,
            1,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        sim.session.binary_frame = 179;
        mission_open(&mut sim, 100, &rules);
        let gate = sim.substrate.entities.get(100).unwrap();
        assert_eq!(gate.mission.handler_state(), 2);
        assert_eq!(
            gate.building_gate.unwrap().hold_timer,
            MissionTimer::armed(0, 180)
        );
        sim.session.binary_frame = 180;
        mission_open(&mut sim, 100, &rules);
        let gate = sim.substrate.entities.get(100).unwrap();
        assert_eq!(gate.mission.handler_state(), 3);
        assert_eq!(
            gate.building_gate.unwrap().hold_timer,
            MissionTimer::armed(0, 180)
        );
    }
}
