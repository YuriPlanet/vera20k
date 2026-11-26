//! Unit-level AI decisions on a retail map (control-point list MIS-S2..S6,
//! MIS-S8): `IdleMission` replaces the mission a stopped GI idles into;
//! `MissionStep` sends a guarding GI to the module's cell and gives an
//! attacking or hunting GI the module's target; `HarvestCell` sends a
//! harvester to the module's cell; and a computer house's claimed `Forces`
//! leaves its infantry's stance to the units' own rule.

use vera20k_wasm_ext::decide::{EVENT_HARVEST_CELL, EVENT_IDLE_MISSION, EVENT_MISSION_STEP};

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

/// A `MissionStep` decider bound to `target` that copies its record to 512
/// and the result area as it came to 704, counts requests at 696 and
/// answers by writing `answer` at byte `at` of the area (0 the target, 8
/// the destination).
fn step_module(target: i64, at: u32, answer: i64) -> Vec<u8> {
    let def = decide_def(EVENT_MISSION_STEP);
    let bytes = def.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\00\04\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{bytes}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (local $area i32)
    (memory.copy (i32.const 512) (local.get $ptr) (i32.const 160))
    (i32.store (i32.const 696) (i32.add (i32.load (i32.const 696)) (i32.const 1)))
    (local.set $area (i32.load offset=148 (local.get $ptr)))
    (memory.copy (i32.const 704) (local.get $area) (i32.const 24))
    (i64.store offset={at} (local.get $area) (i64.const {answer}))
    (i32.store offset=144 (local.get $ptr) (i32.const 1))))"#,
        len = def.len(),
    ))
    .expect("test module parses")
}

fn install(scene: &mut HeadlessScenario, kind: i32, target: i64, answer: i64) {
    install_module(scene, &module(kind, target, answer));
}

fn install_module(scene: &mut HeadlessScenario, module: &[u8]) {
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, module)
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

/// One frame with `orders`.
fn frame(scene: &mut HeadlessScenario, orders: Vec<crate::sim::command::CommandEnvelope>) {
    scene
        .runtime
        .advance_frame(
            &orders,
            crate::headless_scenario::SIM_TICK_MS,
            crate::sim::world::TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("frame advances");
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn i64_at(bytes: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

/// Queues and commences `mission` on `id`, as a script would.
fn queue_mission(scene: &mut HeadlessScenario, id: u64, mission: crate::sim::mission::MissionType) {
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scene.runtime;
    let now = sim.session.binary_frame();
    sim.mission_queue_exact(
        id,
        crate::sim::mission::MissionId::from_known(mission),
        1,
        now,
        &crate::sim::mission::authority::LiveReadyInputProvider {
            rules: &resources.rules,
        },
    )
    .expect("the GI takes the mission");
}

/// A second Soviet GI beside the first (`gi_duel`'s flat, walkable block).
fn second_enemy(scene: &mut HeadlessScenario, enemy: u64) -> Option<u64> {
    let (x, y) = {
        let at = &scene.sim().entities().get(enemy).expect("enemy").position;
        (at.rx(), at.ry())
    };
    let runtime = &mut scene.runtime;
    runtime
        .simulation
        .spawn_object("E1", "Russians", x, y + 1, 0, &runtime.resources.rules)
}

/// A guarding GI walks to the module's cell: `MissionStep` answers a
/// destination one cell south-west where the Guard body leaves none.
#[test]
fn mission_step_sends_a_guarding_gi_to_the_module_cell() {
    let Some((mut scene, attacker, _)) = super::tests::gi_duel() else {
        return;
    };
    let cell = {
        let at = &scene.sim().entities().get(attacker).expect("GI").position;
        (at.rx() - 1, at.ry() + 1)
    };
    install_module(
        &mut scene,
        &step_module(attacker as i64, 8, super::cell_ref(cell.0, cell.1)),
    );
    queue_mission(
        &mut scene,
        attacker,
        crate::sim::mission::MissionType::Guard,
    );
    let wanted = crate::sim::components::NavTargetRef::cell(cell.0, cell.1);
    let mut sent = false;
    for _ in 0..200 {
        frame(&mut scene, Vec::new());
        let gi = scene.sim().entities().get(attacker).expect("GI");
        if gi.navigation.nav_com() == Some(wanted) || (gi.position.rx(), gi.position.ry()) == cell {
            sent = true;
            break;
        }
    }
    let memory = memory(&scene);
    assert!(i32_at(&memory, 696) >= 1, "the Guard body asked");
    assert_eq!(i32_at(&memory, 516), EVENT_MISSION_STEP);
    let mission = i64_at(&memory, 584);
    assert!(
        matches!(mission, 5 | 6 | 11),
        "a Guard-family body, {mission}"
    );
    assert!(sent, "the module's cell became the GI's destination");
}

/// A GI ordered to attack one Soviet GI fights the module's pick, a second
/// one: `MissionStep` answers the target after the Attack body.
#[test]
fn mission_step_picks_the_target_of_an_attacking_gi() {
    use crate::sim::command::{Command, CommandEnvelope};
    let Some((mut scene, attacker, enemy)) = super::tests::gi_duel() else {
        return;
    };
    let Some(second) = second_enemy(&mut scene, enemy) else {
        return;
    };
    install_module(&mut scene, &step_module(attacker as i64, 0, second as i64));
    let order = {
        let sim = scene.sim();
        CommandEnvelope::new(
            sim.interner.get("Americans").expect("Americans house"),
            sim.session.tick() + 1,
            Command::clicked_mission(
                attacker,
                crate::sim::mission::MissionType::Attack,
                crate::sim::combat::TargetKind::Entity(enemy),
            ),
        )
    };
    let mut orders = vec![order];
    let mut picked = false;
    for _ in 0..200 {
        frame(&mut scene, std::mem::take(&mut orders));
        let asked = i32_at(&memory(&scene), 696);
        let gi = scene.sim().entities().get(attacker).expect("GI");
        // A Guard body before the order may ask too; the order then resets
        // the target, so only a later Attack body's answer passes here.
        if asked >= 1
            && gi.mission.current().known() == Some(crate::sim::mission::MissionType::Attack)
            && gi.attack_target.target() == Some(crate::sim::combat::TargetKind::Entity(second))
        {
            picked = true;
            break;
        }
    }
    let memory = memory(&scene);
    assert!(picked, "the module's pick became the GI's target");
    assert_eq!(i32_at(&memory, 516), EVENT_MISSION_STEP);
    assert_eq!(i64_at(&memory, 584), 1, "the Attack body asked last");
    // The area comes holding the body's target: the order's, the module's
    // pick, or none on a frame the body has just cleared it.
    let native = i64_at(&memory, 704);
    assert!(
        [0, enemy as i64, second as i64].contains(&native),
        "the area came holding the body's target, {native}"
    );
}

/// A hunting GI takes the module's pick over its own Greatest_Threat scan.
#[test]
fn mission_step_picks_the_target_of_a_hunting_gi() {
    let Some((mut scene, attacker, enemy)) = super::tests::gi_duel() else {
        return;
    };
    let Some(second) = second_enemy(&mut scene, enemy) else {
        return;
    };
    install_module(&mut scene, &step_module(attacker as i64, 0, second as i64));
    queue_mission(&mut scene, attacker, crate::sim::mission::MissionType::Hunt);
    let mut picked = false;
    for _ in 0..200 {
        frame(&mut scene, Vec::new());
        let asked = i32_at(&memory(&scene), 696);
        let gi = scene.sim().entities().get(attacker).expect("GI");
        let hunting = matches!(
            gi.mission.current().known(),
            Some(crate::sim::mission::MissionType::Hunt | crate::sim::mission::MissionType::Attack)
        );
        if asked >= 1
            && hunting
            && gi.attack_target.target() == Some(crate::sim::combat::TargetKind::Entity(second))
        {
            picked = true;
            break;
        }
    }
    let memory = memory(&scene);
    assert!(i32_at(&memory, 696) >= 1, "the Hunt body asked");
    assert_eq!(i32_at(&memory, 516), EVENT_MISSION_STEP);
    let mission = i64_at(&memory, 584);
    assert!(
        matches!(mission, 1 | 15),
        "Hunt, or Attack after it, {mission}"
    );
    assert!(picked, "the module's pick became the GI's target");
}
