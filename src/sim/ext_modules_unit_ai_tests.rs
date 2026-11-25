//! Unit-level AI decisions on a retail map (control-point list MIS-S2,
//! MIS-S8): `IdleMission` replaces the mission a stopped GI idles into;
//! `HarvestCell` sends a harvester to the module's cell; and a computer
//! house's claimed `Forces` leaves its infantry's stance to the units' own
//! rule.

use vera20k_wasm_ext::decide::{EVENT_HARVEST_CELL, EVENT_IDLE_MISSION};

use super::ModuleSource;
use crate::headless_scenario::HeadlessScenario;

/// A `TriggerDef`: house 0, repeating, every difficulty, decision handler
/// key 1, one event of `kind`, no actions.
fn decide_def(kind: i32) -> Vec<u8> {
    let mut out = 0i64.to_le_bytes().to_vec();
    for value in [2, 7, 0, 1, 1, 0, kind, 0, 0, -1] {
        out.extend(value.to_le_bytes());
    }
    out.extend(0i64.to_le_bytes());
    out.extend([0; 8]);
    out
}

/// A decider for `kind` bound to `target` that answers `answer` and copies
/// its record to 512, counting requests at 696.
fn module(kind: i32, target: i64, answer: i64) -> Vec<u8> {
    let def = decide_def(kind);
    let bytes = def.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\a0\00\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{bytes}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (memory.copy (i32.const 512) (local.get $ptr) (i32.const 160))
    (i32.store (i32.const 696) (i32.add (i32.load (i32.const 696)) (i32.const 1)))
    (i64.store offset=136 (local.get $ptr) (i64.const {answer}))
    (i32.store offset=144 (local.get $ptr) (i32.const 1))))"#,
        len = def.len(),
    ))
    .expect("test module parses")
}

fn install(scene: &mut HeadlessScenario, kind: i32, target: i64, answer: i64) {
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &module(kind, target, answer),
        )
        .expect("module installs");
    scene.tick();
    let memory = scene.sim().ext_modules.slots[0].module.memory().to_vec();
    assert_eq!(&memory[72..80], &[0; 8], "create and bind answer Ok");
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory().to_vec()
}

fn advance(scene: &mut HeadlessScenario, orders: Vec<crate::sim::command::CommandEnvelope>) {
    let mut orders = orders;
    for _ in 0..10 {
        scene
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("frame advances");
    }
}

/// A GI that arrives from a move idles into Sleep (mission 0) when the
/// module answers so, where it would queue Guard or Area Guard.
#[test]
fn idle_mission_replaces_what_an_arriving_unit_queues() {
    use crate::sim::command::{Command, CommandEnvelope};
    let Some((mut scene, attacker, _)) = super::tests::gi_duel() else {
        return;
    };
    install(&mut scene, EVENT_IDLE_MISSION, attacker as i64, 0);
    let (x, y) = {
        let at = &scene.sim().entities().get(attacker).expect("GI").position;
        (at.rx(), at.ry())
    };
    let walk = CommandEnvelope::new(
        scene.sim().interner.get("Americans").expect("Americans house"),
        scene.sim().session.tick() + 1,
        Command::Move {
            entity_id: attacker,
            target_rx: x,
            target_ry: y.saturating_sub(2),
        },
    );
    advance(&mut scene, vec![walk]);
    for _ in 0..12 {
        advance(&mut scene, Vec::new());
    }
    let memory = memory(&scene);
    let count = i32::from_le_bytes(memory[696..700].try_into().unwrap());
    assert!(count >= 1, "the GI idled and asked");
    let kind = i32::from_le_bytes(memory[516..520].try_into().unwrap());
    assert_eq!(kind, EVENT_IDLE_MISSION);
    let native = i64::from_le_bytes(memory[632..640].try_into().unwrap());
    assert!(native == 5 || native == 11, "Guard or Area Guard natively, {native}");
    let mission = scene
        .sim()
        .entities()
        .get(attacker)
        .expect("GI")
        .mission
        .current()
        .raw();
    assert_eq!(mission, 0, "Sleep");
}

/// A harvester's search heads for the module's cell, three cells east
/// (`HarvestCell` is asked in the scan the search runs).
#[test]
fn harvest_cell_sends_the_harvester_to_the_module_cell() {
    let Some((mut scene, attacker, _)) = super::tests::gi_duel() else {
        return;
    };
    let (x, y) = {
        let at = &scene.sim().entities().get(attacker).expect("GI").position;
        (at.rx(), at.ry() + 4)
    };
    let harvester = {
        let runtime = &mut scene.runtime;
        runtime
            .simulation
            .spawn_object("HARV", "Americans", x, y, 0, &runtime.resources.rules)
    };
    let Some(harvester) = harvester else {
        return;
    };
    let cell = super::cell_ref(x + 3, y);
    install(&mut scene, EVENT_HARVEST_CELL, harvester as i64, cell);
    let found = {
        let runtime = &mut scene.runtime;
        crate::sim::miner::ore_scan::search_for_tiberium_and_move(
            &mut runtime.simulation,
            &runtime.resources.rules,
            Some(runtime.resources.rules.overlay_registry()),
            harvester,
            8,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("the scan runs")
    };
    assert!(!found, "not its own cell");
    let memory = memory(&scene);
    assert_eq!(
        i32::from_le_bytes(memory[516..520].try_into().unwrap()),
        EVENT_HARVEST_CELL
    );
    assert!(
        scene
            .sim()
            .entities()
            .get(harvester)
            .expect("harvester")
            .navigation
            .nav_com()
            .is_some(),
        "the module's cell became its destination"
    );
}

/// A computer house whose `Forces` a module claims: its idle infantry take
/// the units' own stance rule, Guard for a stock GI (no `GuardArea`
/// ability), where the computer's pick would be Area Guard for an armed one.
#[test]
fn claimed_forces_leave_the_idle_stance_to_the_unit() {
    let Some(mut scene) = super::tests::scenario_with_computer() else {
        return;
    };
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scene.runtime;
    let rules = &resources.rules;
    let Some(owner) = sim
        .houses
        .order()
        .iter()
        .copied()
        .find(|owner| sim.houses.get(owner).is_some_and(|house| !house.is_human()))
    else {
        return;
    };
    let name = sim.interner.resolve(owner).to_owned();
    assert_eq!(
        sim.claim_for_test(&name, vera20k_wasm_ext::DOMAIN_FORCES as u8, rules),
        vera20k_wasm_ext::Status::Ok
    );
    let Some(gi) = (20..120_u16)
        .flat_map(|y| (20..120_u16).map(move |x| (x, y)))
        .find_map(|(x, y)| sim.spawn_object("E1", name.as_str(), x, y, 0, rules))
    else {
        return;
    };
    assert_eq!(
        sim.infantry_idle_guard_pick_for_test(gi, rules),
        crate::sim::mission::MissionType::Guard
    );
}
