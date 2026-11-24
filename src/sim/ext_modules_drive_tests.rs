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
            host.read(house, prop(name))
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
