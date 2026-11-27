//! The read surface for driving a house (RFC 0001 section 6i) on the retail
//! XMP03T4: object, cell and global reads, `Visible` and `Engage` for a
//! query's viewpoint, factories and superweapons.

use super::tests::{
    computer_tank, gi_duel, install, mem_i32, mem_i64, params_wat, prop, query_wat,
    scenario_with_computer,
};
use super::{ModuleSource, house_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::map::entities::EntityCategory;
use vera20k_wasm_ext::Status;

const UNSUPPORTED: i32 = Status::Unsupported as i32;

/// `get` calls of `(reference, property)` pairs, as WAT: status `i` at
/// `512 + 4i`, value `i` at `8i`.
fn get_calls(reads: &[(i64, &str)]) -> String {
    reads
        .iter()
        .enumerate()
        .map(|(i, (reference, name))| {
            format!(
                "(i32.store (i32.const {}) (call $get (i64.const {reference}) (i32.const {}) (i32.const {})))\n",
                512 + i * 4,
                prop(name),
                i * 8
            )
        })
        .collect()
}

/// `find` calls of the queries at `1024 + 256q` with `clauses[q]` clauses,
/// as WAT: status `q` at `256 + 4q`, the result at `64 + 16q`, references
/// at `4096 + 512q`.
fn find_calls(clauses: &[usize]) -> String {
    clauses
        .iter()
        .enumerate()
        .map(|(q, clauses)| {
            format!(
                "(i32.store (i32.const {}) (call $find (i32.const {}) (i32.const {}) (i32.const {}) (i32.const 64) (i32.const {})))\n",
                256 + q * 4,
                1024 + q * 256,
                64 + 16 * clauses,
                4096 + q * 512,
                64 + q * 16
            )
        })
        .collect()
}

/// A module that runs `body` once, with `get`, `set`, `find` and `action`.
fn once_module(body: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "set" (func $set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "find" (func $find (param i32 i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    {body}))"#
    ))
    .expect("test module parses")
}

fn found(scene: &HeadlessScenario, q: usize) -> Vec<i64> {
    let count = mem_i32(scene, 0, 64 + q * 16);
    (0..count as usize)
        .map(|i| mem_i64(scene, 0, 4096 + q * 512 + i * 8))
        .collect()
}

/// A computer tank ordered to move reads its NavCom as `Destination`, a
/// ready weapon, no deployment, build-up or parasite; `ControlGroup` is the
/// client's. Cells read the pathfinder's zone and their obstruction, the
/// global `AutomationAllowed` 0.
#[test]
fn a_module_reads_what_driving_a_house_needs() {
    use crate::rules::locomotor_type::MovementZone;
    let Some(mut scene) = scenario_with_computer() else {
        return;
    };
    let (house, tank, (x, y)) = computer_tank(&mut scene);
    let building = {
        let sim = scene.sim();
        sim.entities()
            .iter_sorted()
            .find(|(_, e)| {
                e.category() == EntityCategory::Structure
                    && e.lifecycle.object_alive()
                    && e.lifecycle.cell_marked()
                    && !e.lifecycle.in_limbo()
            })
            .map(|(_, e)| (e.position.rx(), e.position.ry()))
            .expect("the map has a building")
    };
    let goal = super::cell_ref(x + 6, y);
    let object = tank as i64;
    let here = super::cell_ref(x, y);
    let reads = [
        (object, "Destination"),
        (object, "WeaponReady"),
        (object, "Deployed"),
        (object, "UnderConstruction"),
        (object, "Parasite"),
        (object, "ControlGroup"),
        (here, "Zone.Normal"),
        (here, "Obstructed"),
        (super::cell_ref(building.0, building.1), "Obstructed"),
        (4i64 << 48, "AutomationAllowed"),
    ];
    // The Move order at 1024: kind 0, one unit, the goal, the tank at 1048.
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "order" (func $order (param i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (i32.store (i32.const 1024) (i32.const 0))
    (i32.store (i32.const 1028) (i32.const 1))
    (i64.store (i32.const 1032) (i64.const {goal}))
    (i64.store (i32.const 1048) (i64.const {object}))
    (i32.store (i32.const 2000) (call $order (i64.const {computer}) (i32.const 1024) (i32.const 32))))
  (func (export "on_frame")
    {calls}))"#,
        computer = house_ref(house),
        calls = get_calls(&reads)
    ))
    .expect("test module parses");
    install(&mut scene, ModuleSource::Map, &module);
    for _ in 0..10 {
        scene.tick();
    }
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(mem_i32(&scene, 0, 2000), 0, "the order is accepted");
    let sim = scene.sim();
    let destination = match sim.entities().get(tank).expect("tank").navigation.nav_com() {
        None => 0,
        Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => super::cell_ref(rx, ry),
        Some(crate::sim::components::NavTargetRef::Object { id }) => id as i64,
    };
    assert_ne!(destination, 0, "the tank is on its way");
    let terrain = sim.resolved_terrain().expect("terrain");
    let zone = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.get_zone_id_native(terrain, (x, y), MovementZone::Normal, false))
        .expect("the tank's cell has a zone");
    let values: Vec<i64> = (0..reads.len())
        .map(|i| mem_i64(&scene, 0, i * 8))
        .collect();
    let statuses: Vec<i32> = (0..reads.len())
        .map(|i| mem_i32(&scene, 0, 512 + i * 4))
        .collect();
    assert_eq!(
        values,
        [destination, 1, 0, 0, 0, 0, i64::from(zone), 0, 1, 0]
    );
    assert_eq!(statuses, [0, 0, 0, 0, 0, UNSUPPORTED, 0, 0, 0, 0]);
}

/// `Visible` reads the viewpoint house's sight: the computer sees its own
/// tank, the player, who has not scouted the computer's base, does not.
#[test]
fn visible_differs_between_the_owner_and_an_unscouted_house() {
    let Some(mut scene) = scenario_with_computer() else {
        return;
    };
    let (computer, _, (x, y)) = computer_tank(&mut scene);
    let human = {
        let sim = scene.sim();
        let human = sim
            .houses
            .order()
            .iter()
            .position(|id| sim.houses[id].is_human())
            .expect("the battle has a player");
        let viewer = sim.houses.order()[human];
        assert!(
            !sim.fog.is_cell_revealed(viewer, x, y),
            "the player has not scouted the computer's base"
        );
        human
    };
    let cell = Some(super::cell_ref(x, y));
    let queries = [
        query_wat(1024, 0, cell, 0, house_ref(computer), &[("Visible", 1)]),
        query_wat(1280, 0, cell, 0, house_ref(human), &[("Visible", 1)]),
        query_wat(1536, 0, cell, 0, house_ref(human), &[("Visible", 0)]),
    ];
    install(
        &mut scene,
        ModuleSource::Map,
        &once_module(&(queries.concat() + &find_calls(&[1, 1, 1]))),
    );
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let statuses: Vec<i32> = (0..3).map(|q| mem_i32(&scene, 0, 256 + q * 4)).collect();
    assert_eq!(statuses, [0; 3]);
    let counts: Vec<i32> = (0..3).map(|q| mem_i32(&scene, 0, 64 + q * 16)).collect();
    assert_eq!(counts, [1, 0, 1]);
}

/// `Engage` for a viewpoint GI: the enemy GI two cells away is `Ok`, one
/// ten or more cells away `OutOfRange`; the house clause of the same query
/// reads for the GI's owner. With a house viewpoint, or none, `Engage` is
/// `Unsupported`.
#[test]
fn engage_answers_for_the_viewpoint_shooter() {
    let Some((mut scene, attacker, target)) = gi_duel() else {
        return;
    };
    let (near, far) = {
        let crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        } = &mut scene.runtime;
        let rules = &resources.rules;
        let near = sim
            .entities()
            .get(target)
            .map(|e| (e.position.rx(), e.position.ry()))
            .expect("target");
        let far = (near.0 + 10..near.0 + 60)
            .find_map(|fx| sim.spawn_object("E1", "Russians", fx, near.1, 0, rules))
            .expect("a far GI");
        sim.resolve_type_handles(rules);
        let far = sim
            .entities()
            .get(far)
            .map(|e| (e.position.rx(), e.position.ry()))
            .expect("far GI");
        (near, far)
    };
    let americans = {
        let sim = scene.sim();
        let id = sim.interner.get("Americans").expect("Americans");
        sim.houses.index_of(id).expect("Americans index")
    };
    let shooter = attacker as i64;
    let (near_cell, far_cell) = (
        Some(super::cell_ref(near.0, near.1)),
        Some(super::cell_ref(far.0, far.1)),
    );
    let queries = [
        query_wat(
            1024,
            0,
            near_cell,
            0,
            shooter,
            &[("Engage", 0), ("Relation", 2)],
        ),
        query_wat(1280, 0, far_cell, 0, shooter, &[("Engage", 2)]),
        query_wat(
            1536,
            0,
            near_cell,
            0,
            house_ref(americans),
            &[("Engage", 0)],
        ),
    ];
    let direct = get_calls(&[(target as i64, "Engage")]);
    install(
        &mut scene,
        ModuleSource::Map,
        &once_module(&(queries.concat() + &find_calls(&[2, 1, 1]) + &direct)),
    );
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(found(&scene, 0), [target as i64]);
    assert_eq!(found(&scene, 1).len(), 1, "the far GI is out of range");
    assert!(found(&scene, 2).is_empty());
    assert_eq!(
        mem_i32(&scene, 0, 512),
        UNSUPPORTED,
        "a plain get has no shooter"
    );
}

/// `Factory.<Kind>.*` follow the house's factory: two GIs queued show the
/// first in production and two entries, then the finished one ready.
#[test]
fn factory_properties_follow_a_queued_production() {
    use crate::sim::production::ProductionCategory;
    let Some((mut scene, _, _)) = gi_duel() else {
        return;
    };
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scene.runtime;
    let rules = &resources.rules;
    let americans = sim.interner.get("Americans").expect("Americans");
    let house = house_ref(sim.houses.index_of(americans).expect("index"));
    let e1 = sim.interner.intern("E1");
    let factories = sim.production.factories_mut();
    assert!(factories.test_enqueue_kernel(americans, ProductionCategory::Infantry, e1, 0, 200));
    factories.test_enqueue_kernel(americans, ProductionCategory::Infantry, e1, 1, 200);
    let gi = super::type_ref(
        vera20k_wasm_ext::NAME_TECHNO_TYPE,
        rules.type_handle("E1").expect("E1").0,
    );
    let read = |sim: &mut crate::sim::world::Simulation, name: &str| {
        sim.with_module_host(rules, ModuleSource::Map, |host| {
            host.reader().read(house, prop(name))
        })
        .expect("a valid read")
    };
    let names = [
        "Factory.Infantry.Type",
        "Factory.Infantry.Progress",
        "Factory.Infantry.Ready",
        "Factory.Infantry.Queued",
        "Factory.Vehicle.Type",
        "Factory.Vehicle.Queued",
    ];
    let queued: Vec<_> = names.iter().map(|name| read(sim, name)).collect();
    let ok = |value| (value, Status::Ok);
    assert_eq!(queued, [ok(gi), ok(0), ok(0), ok(2), ok(0), ok(0)]);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(americans, ProductionCategory::Infantry)
    );
    let ready: Vec<_> = names[..4].iter().map(|name| read(sim, name)).collect();
    assert_eq!(ready, [ok(gi), ok(100), ok(1), ok(2)]);
}

/// `find(SuperWeapons)` lists the granted Supers by house, then type; an
/// owner narrows it. A Super reads its type, owner and readiness, and a
/// `ChargePercent` write runs stock action 129.
#[test]
fn find_super_weapons_lists_the_houses_granted_supers() {
    let Some(mut scene) = scenario_with_computer() else {
        return;
    };
    let (a, b, nuke, storm) = {
        let rules = &scene.runtime.resources.rules;
        let sim = scene.sim();
        let mut houses = sim
            .houses
            .order()
            .iter()
            .enumerate()
            .filter(|(_, id)| {
                sim.super_weapons.get(id).is_some()
                    && sim.houses.get(id).is_some_and(|h| !h.multiplay_passive())
            })
            .map(|(index, _)| index);
        let a = houses.next().expect("a house with Supers");
        let b = houses.next().expect("a second house with Supers");
        (
            a,
            b,
            rules
                .super_weapon_index("NukeSpecial")
                .expect("NukeSpecial"),
            rules
                .super_weapon_index("LightningStormSpecial")
                .expect("LightningStormSpecial"),
        )
    };
    let all = query_wat(1024, 8, None, 0, 0, &[]);
    let of_b = query_wat(1280, 8, None, house_ref(b), 0, &[]);
    let first = "(i64.load (i32.const 4096))";
    let reads: String = ["Type", "Owner", "Ready", "ChargePercent"]
        .iter()
        .enumerate()
        .map(|(i, name)| {
            format!(
                "(i32.store (i32.const {}) (call $get {first} (i32.const {}) (i32.const {})))\n",
                600 + i * 4,
                prop(name),
                640 + i * 8
            )
        })
        .collect();
    let body = format!(
        "{grant_a} {grant_b}
    (i32.store (i32.const 0) (call $action (i32.const 34) (i64.const {house_a}) (i32.const 128) (i32.const 72) (i32.const 3000)))
    (i32.store (i32.const 4) (call $action (i32.const 34) (i64.const {house_b}) (i32.const 384) (i32.const 72) (i32.const 3008)))
    {all} {of_b} {finds} {reads}
    (i32.store (i32.const 8) (call $set {first} (i32.const {charge}) (i64.const 50)))
    (i32.store (i32.const 12) (call $get {first} (i32.const {charge}) (i32.const 680)))",
        grant_a = params_wat(128, nuke as i32, 0, 0, 0),
        grant_b = params_wat(384, storm as i32, 0, 0, 0),
        house_a = house_ref(a),
        house_b = house_ref(b),
        finds = find_calls(&[0, 0]),
        charge = prop("ChargePercent"),
    );
    install(&mut scene, ModuleSource::Map, &once_module(&body));
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let statuses: Vec<i32> = [0, 4, 256, 260, 600, 604, 608, 612, 8, 12]
        .iter()
        .map(|&at| mem_i32(&scene, 0, at))
        .collect();
    assert_eq!(statuses, [0; 10]);
    let (nuke_ref, storm_ref) = (super::super_ref(a, nuke), super::super_ref(b, storm));
    let granted: Vec<i64> = {
        let rules = &scene.runtime.resources.rules;
        let sim = scene.sim();
        (0..sim.houses.order().len())
            .flat_map(|house| (0..rules.super_weapon_order.len()).map(move |i| (house, i)))
            .filter(|&(house, i)| {
                let owner = sim.houses.order()[house];
                sim.interner
                    .get(&rules.super_weapon_order[i])
                    .and_then(|ty| sim.super_weapons.get(&owner)?.get(&ty))
                    .is_some_and(|instance| instance.is_active())
            })
            .map(|(house, i)| super::super_ref(house, i))
            .collect()
    };
    assert!(granted.contains(&nuke_ref) && granted.contains(&storm_ref));
    assert_eq!(found(&scene, 0), granted);
    assert_eq!(found(&scene, 1), [storm_ref]);
    let first = found(&scene, 0)[0];
    let (house, index) = ((first >> 16) as usize & 0xFFFF, first as usize & 0xFFFF);
    assert_eq!(
        mem_i64(&scene, 0, 640),
        super::type_ref(vera20k_wasm_ext::NAME_SUPER_WEAPON, index as u32)
    );
    assert_eq!(mem_i64(&scene, 0, 648), house_ref(house));
    assert!((0..=1).contains(&mem_i64(&scene, 0, 656)));
    assert!((0..=100).contains(&mem_i64(&scene, 0, 664)));
    // SetPercentage's f32 0.01 leaves 50% one frame short of half.
    assert!((49..=50).contains(&mem_i64(&scene, 0, 680)));
}

/// A host read of `name` on `reference`, without a viewpoint.
fn host_read(scene: &mut HeadlessScenario, reference: i64, name: &str) -> (i64, Status) {
    let runtime = &mut scene.runtime;
    runtime
        .simulation
        .with_module_host(&runtime.resources.rules, ModuleSource::Map, |host| {
            host.reader().read(reference, prop(name))
        })
        .expect("a valid read")
}

/// `NearbyEnemyCount` counts the enemy GIs within the GI's `Sight=`, not a
/// friend beside it or an enemy far off. `TargetInRange` and
/// `CanFireAtTarget` read 0 without a target and against the far enemy;
/// against the near one InRange holds and `CanFireAtTarget` is
/// GetFireError's Ok. A GI is not cloaked and has no door or working body
/// mode.
#[test]
fn a_gi_reads_its_nearby_enemies_and_its_target() {
    use crate::sim::combat::TargetKind;
    use crate::sim::combat::fire_error::FireError;
    let Some((mut scene, attacker, target)) = gi_duel() else {
        return;
    };
    let far = {
        let crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        } = &mut scene.runtime;
        let rules = &resources.rules;
        let (x, y) = sim
            .entities()
            .get(attacker)
            .map(|e| (e.position.rx(), e.position.ry()))
            .expect("attacker");
        sim.spawn_object("E1", "Russians", x + 1, y + 1, 0, rules)
            .expect("a second near enemy");
        sim.spawn_object("E1", "Americans", x, y + 1, 0, rules)
            .expect("a friend");
        let far = (x + 30..x + 90)
            .find_map(|fx| sim.spawn_object("E1", "Russians", fx, y, 0, rules))
            .expect("a far enemy");
        sim.resolve_type_handles(rules);
        far
    };
    let ok = |value| (value, Status::Ok);
    let me = attacker as i64;
    assert_eq!(host_read(&mut scene, me, "NearbyEnemyCount"), ok(2));
    assert_eq!(host_read(&mut scene, me, "TargetInRange"), ok(0));
    assert_eq!(host_read(&mut scene, me, "CanFireAtTarget"), ok(0));
    let aim = |scene: &mut HeadlessScenario, at: u64| {
        scene
            .runtime
            .simulation
            .substrate
            .entities
            .get_mut(attacker)
            .expect("attacker")
            .attack_target
            .set_for_test(Some(TargetKind::Entity(at)));
    };
    aim(&mut scene, far);
    assert_eq!(host_read(&mut scene, me, "TargetInRange"), ok(0));
    assert_eq!(host_read(&mut scene, me, "CanFireAtTarget"), ok(0));
    aim(&mut scene, target);
    let error = {
        let runtime = &scene.runtime;
        runtime.simulation.selected_weapon_fire_error(
            &runtime.resources.rules,
            attacker,
            TargetKind::Entity(target),
            true,
        )
    };
    assert_ne!(error, FireError::Range, "two cells off is in range");
    assert_eq!(host_read(&mut scene, me, "TargetInRange"), ok(1));
    assert_eq!(
        host_read(&mut scene, me, "CanFireAtTarget"),
        ok(i64::from(error == FireError::Ok))
    );
    assert_eq!(host_read(&mut scene, me, "Cloaked"), ok(0));
    let unsupported = (0, Status::Unsupported);
    assert_eq!(host_read(&mut scene, me, "DoorOpen"), unsupported);
    assert_eq!(host_read(&mut scene, me, "Animating"), unsupported);
}

/// `DoorOpen` reads a war factory's door: closed as placed, open once it
/// opens; a power plant has no door. Neither building is in its working
/// body mode (`Animating` 0).
#[test]
fn a_war_factory_reads_its_door() {
    let Some((mut scene, attacker, _)) = gi_duel() else {
        return;
    };
    let (factory, plant) = {
        let crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        } = &mut scene.runtime;
        let rules = &resources.rules;
        let americans = sim.interner.get("Americans").expect("Americans");
        let plant = sim
            .entities()
            .iter_sorted()
            .find(|(_, e)| e.category() == EntityCategory::Structure && e.owner() == americans)
            .map(|(id, _)| id)
            .expect("the American power plant");
        let (x, y) = sim
            .entities()
            .get(attacker)
            .map(|e| (e.position.rx(), e.position.ry()))
            .expect("attacker");
        let factory = (20..120_u16)
            .flat_map(|fy| (20..120_u16).map(move |fx| (fx, fy)))
            .filter(|&(fx, fy)| fx.abs_diff(x).max(fy.abs_diff(y)) >= 8)
            .find_map(|(fx, fy)| sim.spawn_object("GAWEAP", "Americans", fx, fy, 0, rules))
            .expect("a war factory");
        sim.resolve_type_handles(rules);
        (factory, plant)
    };
    let ok = |value| (value, Status::Ok);
    let unsupported = (0, Status::Unsupported);
    assert_eq!(host_read(&mut scene, factory as i64, "DoorOpen"), ok(0));
    assert_eq!(host_read(&mut scene, plant as i64, "DoorOpen"), unsupported);
    assert_eq!(host_read(&mut scene, factory as i64, "Animating"), ok(0));
    assert_eq!(host_read(&mut scene, plant as i64, "Animating"), ok(0));
    let frame = scene.sim().session.binary_frame();
    scene
        .runtime
        .simulation
        .substrate
        .entities
        .get_mut(factory)
        .expect("factory")
        .open_door(10, frame);
    assert_eq!(host_read(&mut scene, factory as i64, "DoorOpen"), ok(1));
    // A write holds how the door is drawn and leaves the door alone; -1
    // lets it go. A power plant has no door to hold.
    let write = |scene: &mut HeadlessScenario, reference: u64, value: i64| {
        let runtime = &mut scene.runtime;
        runtime
            .simulation
            .with_module_host(&runtime.resources.rules, ModuleSource::Map, |host| {
                vera20k_wasm_ext::SimHost::set(host, reference as i64, prop("DoorOpen"), value)
            })
            .expect("a valid write")
    };
    assert_eq!(write(&mut scene, factory, 0), Status::Ok);
    assert_eq!(scene.sim().ext_module_door(factory), Some(false));
    assert_eq!(host_read(&mut scene, factory as i64, "DoorOpen"), ok(1), "the door runs on");
    assert_eq!(write(&mut scene, plant, 1), Status::Unsupported);
    assert_eq!(write(&mut scene, factory, -1), Status::Ok);
    assert_eq!(scene.sim().ext_module_door(factory), None);
}

/// A type's `Name` is a string ID of its rules ID, which `get` adds and
/// every later read repeats, and a filter read finds only once held;
/// TechnoType `Points` is the rules' value. The global `GameMode` names the
/// session's multiplayer mode class. `find(Types)` lists every TechnoType
/// for a wildcard, one for a specific type, and with a `Buildable` clause
/// the types `buildable_by_player` allows the viewpoint house; a plain
/// `Buildable` read has no viewpoint (`Unsupported`), and a Types query
/// without a type is invalid.
#[test]
fn types_read_their_names_points_and_buildability() {
    use vera20k_wasm_ext::{
        Area, Clause, ClauseSubject, MAX_SCAN, NAME_STRING, NAME_TECHNO_TYPE, Op, Query,
        SCOPE_TYPES, SimHost,
    };
    let Some(mut scene) = scenario_with_computer() else {
        return;
    };
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scene.runtime;
    let rules = &resources.rules;
    let techno = |name: &str| {
        super::type_ref(
            NAME_TECHNO_TYPE,
            rules.type_handle(name).expect("a TechnoType").0,
        )
    };
    let (mtnk, htnk) = (techno("MTNK"), techno("HTNK"));
    let points = rules.object("MTNK").expect("MTNK").points;
    let mode = sim
        .session
        .multiplayer_mode_class()
        .map_or("", crate::skirmish_modes::MpModeClass::section)
        .to_owned();
    let total = rules.all_objects().count();
    let shared: &crate::sim::world::Simulation = sim;
    let expected: Vec<Vec<i64>> = shared
        .houses
        .order()
        .iter()
        .map(|&house| {
            rules
                .all_objects()
                .enumerate()
                .filter(|(_, object)| {
                    crate::sim::production::buildable_by_player(shared, rules, house, object)
                })
                .map(|(id, _)| super::type_ref(NAME_TECHNO_TYPE, id as u32))
                .collect()
        })
        .collect();
    let query = |viewpoint: i64, type_ref: i64, buildable: Option<i64>| Query {
        scope: SCOPE_TYPES,
        area: Area::Map,
        type_ref,
        owner: 0,
        viewpoint,
        after: 0,
        clauses: buildable
            .map(|value| Clause {
                subject: ClauseSubject::Property(prop("Buildable")),
                op: Op::Eq,
                value,
            })
            .into_iter()
            .collect(),
        max_scan: MAX_SCAN,
    };
    let refs = |page: &vera20k_wasm_ext::FindPage| -> Vec<i64> {
        page.found.iter().map(|&(_, reference)| reference).collect()
    };
    let any = super::type_ref(NAME_TECHNO_TYPE, u32::MAX);
    sim.with_module_host(rules, ModuleSource::Map, |host| {
        let name = prop("Name");
        assert_eq!(
            host.reader().read(htnk, name).expect("read"),
            (0, Status::NotFound),
            "no get has held HTNK"
        );
        let (id, status) = host.get(mtnk, name).expect("get");
        assert_eq!(status, Status::Ok);
        assert!(id > 0, "a string ID");
        assert_eq!(
            host.name_of(NAME_STRING, id as i32).expect("name_of"),
            ("MTNK".to_owned(), Status::Ok)
        );
        assert_eq!(host.get(mtnk, name).expect("get"), (id, Status::Ok));
        assert_eq!(host.reader().read(mtnk, name).expect("read"), (id, Status::Ok));
        assert_eq!(
            host.get(mtnk, prop("Points")).expect("get"),
            (i64::from(points), Status::Ok)
        );
        let (mode_id, status) = host.get(4 << 48, prop("GameMode")).expect("get");
        let text = if mode_id == 0 {
            String::new()
        } else {
            host.name_of(NAME_STRING, mode_id as i32)
                .expect("name_of")
                .0
        };
        assert_eq!((text, status), (mode, Status::Ok));
        let (all, status) = host.find(&query(0, any, None), 4096).expect("find");
        assert_eq!((all.found.len(), status), (total, Status::Ok));
        let (one, _) = host.find(&query(0, mtnk, None), 16).expect("find");
        assert_eq!(refs(&one), [mtnk]);
        for (index, expected) in expected.iter().enumerate() {
            let view = house_ref(index);
            let (yes, _) = host.find(&query(view, any, Some(1)), 4096).expect("find");
            let (no, _) = host.find(&query(view, any, Some(0)), 4096).expect("find");
            assert_eq!(&refs(&yes), expected, "house {index}");
            assert_eq!(yes.found.len() + no.found.len(), total);
        }
        assert_eq!(
            host.reader().read(mtnk, prop("Buildable")).expect("read"),
            (0, Status::Unsupported)
        );
        assert!(host.find(&query(0, 0, None), 16).is_err());
    });
}

/// `Points=` parses into TechnoType `Points`, 0 when absent.
#[test]
fn techno_type_points_read_the_rules_key() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use vera20k_wasm_ext::NAME_TECHNO_TYPE;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=SCORED\n1=PLAIN\n[SCORED]\nPoints=37\n[PLAIN]\nStrength=100\n",
    ))
    .expect("rules parse");
    let points = |name: &str| {
        let id = rules.type_handle(name).expect("a TechnoType").0;
        super::type_property(&rules, NAME_TECHNO_TYPE, id, "Points").expect("a valid read")
    };
    assert_eq!(points("SCORED"), (37, Status::Ok));
    assert_eq!(points("PLAIN"), (0, Status::Ok));
}
