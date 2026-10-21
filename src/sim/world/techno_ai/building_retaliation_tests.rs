//! Native evidence for [`super`]: `tools/spatial_oracle/building_retaliation.json`
//! replayed through [`Simulation::building_hit_response`], and sourced hits
//! through the production receiver (`combat::world_receiver::commit_entities`).

use serde_json::Value;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::damage::DamageState;
use crate::sim::combat::world_receiver::{ReceiverRun, commit_entities};
use crate::sim::combat::{EntityDamageEvent, TargetKind};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::rng::SimRng;
use crate::sim::world::{SimSoundEvent, Simulation};

/// The oracle fixture's frame.
const FRAME: u32 = 200;
/// The building's cell; the source stands 3 cells east in range of `Gun`,
/// 10 out of it.
const BUILDING: (u16, u16) = (5, 5);
const IN_RANGE: (u16, u16) = (8, 5);
const OUT_OF_RANGE: (u16, u16) = (15, 5);

fn golden() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/building_retaliation.json")
}

/// `DEF` with the row's type fields, an enemy `TANK` and `JET`, and
/// `[CombatDamage] PlayerReturnFire=` (VERA's reader takes it only with a
/// non-empty `[General]`, which retail always has; its key here is the
/// constructor's value).
fn rules(input: &Value) -> RuleSet {
    let flag = |key: &str| input[key].as_bool().unwrap_or(false);
    let number = |key: &str, default: i64| input[key].as_i64().unwrap_or(default);
    let weapon = if !input["armed"].as_bool().unwrap_or(true) {
        ""
    } else if flag("anti_air") {
        "Primary=Flak\n"
    } else {
        "Primary=Gun\n"
    };
    // Foundation enum 0 is 1x1; the fixture's default 3 is 2x2.
    let foundation = if number("foundation", 3) == 0 {
        "1x1"
    } else {
        "2x2"
    };
    let undeploys = if flag("undeploys") {
        "UndeploysInto=TANK\n"
    } else {
        ""
    };
    // Not operational: Powered= and its House short of power.
    let powered = if input["operational"] == false {
        "Powered=yes\nPower=-50\n"
    } else {
        ""
    };
    let yes = |set: bool| if set { "yes" } else { "no" };
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[General]\nGuardAreaTargetingDelay=36\n[CombatDamage]\nPlayerReturnFire={}\n\
         [BuildingTypes]\n0=DEF\n\
         [VehicleTypes]\n0=TANK\n\
         [AircraftTypes]\n0=JET\n\
         [DEF]\nStrength=1000\nArmor=concrete\nROT={}\n\
         Insignificant={}\n{weapon}{undeploys}{powered}\
         [TANK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [JET]\nStrength=150\nArmor=light\nSpeed=12\n\
         [Gun]\nDamage=10\nROF=20\nRange=6\nProjectile=Shell\nWarhead=AP\n\
         [Flak]\nDamage=10\nROF=20\nRange=6\nProjectile=FlakShell\nWarhead=AP\n\
         [Shell]\nAG=yes\n\
         [FlakShell]\nAA=yes\nAG=no\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            yes(number("player_return_fire", 0) != 0),
            number("rot", 10),
            yes(number("insignificant", 0) != 0),
        )),
        &IniFile::from_str(&format!("[DEF]\nFoundation={foundation}\n")),
    )
    .unwrap()
}

/// A `Russians` object of `kind` standing at `cell`. The bare test map has no
/// land speeds for a vehicle's Unlimbo, and the block reads only the source's
/// class, house and position.
fn enemy(sim: &mut Simulation, kind: &str, category: EntityCategory, cell: (u16, u16)) -> u64 {
    let id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default(id, kind, "Russians", cell.0, cell.1);
    entity.set_owner_for_test(sim.intern("Russians"));
    entity.set_type_ref_for_test(sim.intern(kind));
    entity.set_category_for_test(category);
    sim.substrate.entities.insert(entity);
    id
}

fn mission(name: &str) -> MissionId {
    match name {
        "none" => MissionId::NONE,
        "guard" => MissionId::from_known(MissionType::Guard),
        "selling" => MissionId::from_known(MissionType::Selling),
        other => panic!("unexpected mission {other}"),
    }
}

/// The row's building (owned by `Americans`, human per the row) and source
/// (`Russians`), at the oracle's frame and Scenario seed.
fn fixture(rules: &RuleSet, input: &Value) -> (Simulation, u64, Option<u64>) {
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(24));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    for (house, human) in [
        ("Americans", input["human"].as_bool().unwrap_or(false)),
        // Original block fixture supplies the source House index3, leaving
        // two intervening registry slots. They own no actors or callbacks.
        ("Unused1", false),
        ("Unused2", false),
        ("Russians", false),
    ] {
        let id = sim.interner.intern(house);
        sim.houses
            .register(HouseState::new(id, 0, Some(id), human, 5000, 10));
    }
    if input["allied"].as_bool().unwrap_or(false) {
        sim.house_alliances_mut_for_test()
            .entry("AMERICANS".into())
            .or_default()
            .insert("RUSSIANS".into());
    }
    let building = sim
        .spawn_object("DEF", "Americans", BUILDING.0, BUILDING.1, 0, rules)
        .unwrap();
    let cell = if input["in_range"].as_bool().unwrap_or(true) {
        IN_RANGE
    } else {
        OUT_OF_RANGE
    };
    let source = match input.get("source") {
        Some(Value::Null) => None,
        Some(kind) if kind == "aircraft" => Some(("JET", EntityCategory::Aircraft)),
        _ => Some(("TANK", EntityCategory::Unit)),
    }
    .map(|(kind, category)| enemy(&mut sim, kind, category, cell));
    sim.session.set_binary_frame_for_test(FRAME);
    if input["operational"] == false {
        let owner = sim.interner.intern("Americans");
        let mut power_state = crate::sim::power_system::PowerState::default();
        power_state.set_total_drain_for_test(100);
        power_state.set_is_low_power_for_test(true);
        sim.power_states
            .map_mut_for_test()
            .insert(owner, power_state);
    }
    sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
    let entity = sim.substrate.entities.get_mut(building).unwrap();
    // The original block fixture supplies this retained +53C prior.
    entity.initialize_building_loss_index(0x7777);
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: mission(input["mission"].as_str().unwrap_or("guard")),
        suspended: MissionId::NONE,
        queued: mission(input["queued"].as_str().unwrap_or("none")),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: FRAME,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::from_raw(FRAME as i32, 0),
    });
    if input["target"].as_bool().unwrap_or(false) {
        entity
            .attack_target
            .set_for_test(source.map(TargetKind::Entity));
    }
    let facing = &mut entity.body_facing;
    if let Some(desired) = input["initial_desired"].as_u64() {
        // The oracle writes the desired word alone; a snap leaves the same
        // settled facing (the start word only matters while rotating).
        facing.snap(desired as u16, FRAME);
    }
    if input["rotating"].as_bool().unwrap_or(false) {
        facing.set(0x4000, FRAME);
        sim.session
            .set_binary_frame_for_test(FRAME + input["advance"].as_u64().unwrap_or(0) as u32);
    }
    (sim, building, source)
}

fn result(code: i64) -> DamageState {
    match code {
        0 => DamageState::Unaffected,
        1 => DamageState::Damaged,
        2 => DamageState::Yellow,
        3 => DamageState::Red,
        4 => DamageState::Dead,
        other => panic!("unexpected result {other}"),
    }
}

fn indices(rng: &SimRng) -> Value {
    let state = rng.logical_state();
    serde_json::json!([state.index_a, state.index_b])
}

/// Every row through [`Simulation::building_hit_response`]: its ping, TarCom,
/// mission, Scenario draws (values and generator indices), `+0x388` and
/// retained `+0x53C` (`who_last`). RESIDUAL (dormant
/// with retail data): `ai_artillary_out_of_range`'s sale, BuildingClass::
/// SetTarget's TickTank/Artillary arm (`building_missions` module doc).
#[test]
fn the_block_matches_the_original() {
    let golden = golden();
    let rows = golden["retaliation"].as_array().unwrap();
    assert_eq!(rows.len(), 44);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = rules(input);
        let (mut sim, building, source) = fixture(&rules, input);
        assert_eq!(
            sim.session.binary_frame(),
            row["frame"].as_u64().unwrap() as u32
        );
        let before = sim.substrate.entities.get(building).unwrap().clone();
        let facing_before = before.body_facing;
        let mut expected_rng = sim.scenario_rng.clone();
        assert_eq!(indices(&sim.scenario_rng), row["rng"][0], "{name} indices");

        let ping = sim.building_hit_response(
            building,
            source,
            result(input["result"].as_i64().unwrap_or(1)),
            &rules,
            None,
        );

        let after = sim.substrate.entities.get(building).unwrap();
        assert_eq!(
            i64::from(after.building_loss_index()),
            row["who_last"].as_i64().unwrap(),
            "{name} source-house loss index"
        );
        // NotifyUnderAttack's cell is GetCoords': a 2x2's foundation centre
        // lies 128 leptons past its anchor cell's centre on both axes, in the
        // next cell (native GetCoords rows in building_fire_facing.json).
        let centre = u16::from(input["foundation"].as_i64().unwrap_or(3) != 0);
        assert_eq!(
            ping.map(|event| (event.victim_type, event.owner, event.rx, event.ry)),
            (row["ping"] == serde_json::json!([true])).then_some((
                before.type_ref(),
                before.owner(),
                BUILDING.0 + centre,
                BUILDING.1 + centre
            )),
            "{name} ping"
        );
        assert_eq!(
            after.attack_target.target(),
            row["target"]
                .as_str()
                .map(|_| TargetKind::Entity(source.unwrap())),
            "{name} target"
        );
        if name != "ai_artillary_out_of_range" {
            assert_eq!(
                (after.mission.current(), after.mission.queued()),
                (before.mission.current(), before.mission.queued()),
                "{name} mission"
            );
        }
        for (stream, drawn) in row["draws"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["drawn"].as_array().unwrap())
        {
            assert_eq!(stream, "scenario");
            assert_eq!(
                u64::from(expected_rng.next_u32()),
                drawn.as_u64().unwrap(),
                "{name} drawn"
            );
        }
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "{name} draws"
        );
        assert_eq!(indices(&sim.scenario_rng), row["rng"][1], "{name} indices");

        let facing = after.body_facing;
        let native = &row["facing_after"];
        if *native == row["facing_before"] {
            assert_eq!(facing, facing_before, "{name} +0x388 untouched");
            continue;
        }
        let word = |key: &str| native[key].as_u64().unwrap() as u16;
        assert_eq!(facing.destination(), word("desired"), "{name} desired");
        assert_eq!(facing.start_word(), word("start"), "{name} start");
        assert_eq!(facing.rot_per_frame(), word("rot"), "{name} ROT");
        assert_eq!(
            facing.timer_duration(),
            word("timer_left"),
            "{name} time left"
        );
        if (word("rot") as i16) > 0 {
            assert_eq!(
                facing.timer_start_frame(),
                Some(native["timer_start"].as_u64().unwrap() as u32),
                "{name} timer start"
            );
        }
    }
}

/// `DEF` as a turreted defence (`Turret=yes`, `ROT=10`), `SHACK` as a
/// turretless `Insignificant=` one, and the enemy `GUN` that hits them (a
/// building, which the bare test map can place).
fn production_rules(extra: &str) -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nGuardAreaTargetingDelay=36\n[BuildingTypes]\n0=DEF\n1=SHACK\n2=GUN\n3=MINER\n\
         [DEF]\nStrength=1000\nArmor=concrete\nPrimary=Gun\nTurret=yes\nROT=10\n\
         [SHACK]\nStrength=1000\nArmor=concrete\nPrimary=Gun\nROT=10\n\
         Insignificant=yes\n\
         [GUN]\nStrength=1000\nArmor=concrete\nPrimary=Gun\n\
         [MINER]\nStrength=1000\nArmor=concrete\nUndeploysInto=GUN\n\
         ResourceGatherer=yes\n\
         [Gun]\nDamage=10\nROF=20\nRange=6\nProjectile=Shell\nWarhead=AP\n\
         [Shell]\nAG=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{extra}"
    )), &IniFile::from_str("[DEF]\nFoundation=2x2\n[SHACK]\nFoundation=2x2\n[GUN]\nFoundation=1x1\n[MINER]\nFoundation=2x2\n"))
    .unwrap()
}

/// A building of `kind` owned by `Americans` (human or not) and an enemy
/// `GUN` at `cell`, at the oracle's frame and Scenario seed 1.
fn production_fixture(
    rules: &RuleSet,
    kind: &str,
    human: bool,
    cell: (u16, u16),
) -> (Simulation, u64, u64) {
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(24));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    for (house, human) in [("Americans", human), ("Russians", false)] {
        let id = sim.interner.intern(house);
        sim.houses
            .register(HouseState::new(id, 0, Some(id), human, 5000, 10));
    }
    let building = sim
        .spawn_object(kind, "Americans", BUILDING.0, BUILDING.1, 0, rules)
        .unwrap();
    let source = sim
        .spawn_object("GUN", "Russians", cell.0, cell.1, 0, rules)
        .unwrap();
    sim.session.set_binary_frame_for_test(FRAME);
    sim.scenario_rng = SimRng::new(1);
    (sim, building, source)
}

/// One `AP` hit of 10 on `building` from `source`, through the production
/// receiver; returns its pings.
fn hit(sim: &mut Simulation, rules: &RuleSet, building: u64, source: u64) -> usize {
    let house = sim.substrate.entities.get(source).unwrap().owner();
    let warhead = sim.interner.intern("AP");
    let event = EntityDamageEvent::area(building, 10, 0, source, Some(house), warhead);
    let (_, pings) = commit_entities(
        sim,
        &mut ReceiverRun::default(),
        &[event],
        None,
        rules,
        None,
    );
    pings.len()
}

/// A 2x2 building that undeploys and gathers (the deployed Slave Miner)
/// pings on NotifyUnderAttack's ore-miner line (`0x004F9491..0x004F94A3`).
#[test]
fn a_deployed_slave_miner_pings_the_ore_miner_line() {
    let rules = production_rules("");
    let (mut sim, building, source) = production_fixture(&rules, "MINER", true, OUT_OF_RANGE);
    let house = sim.substrate.entities.get(source).unwrap().owner();
    let warhead = sim.interner.intern("AP");
    let event = EntityDamageEvent::area(building, 10, 0, source, Some(house), warhead);
    let (_, pings) = commit_entities(
        &mut sim,
        &mut ReceiverRun::default(),
        &[event],
        None,
        &rules,
        None,
    );
    Simulation::dispatch_under_attack_events(
        &sim.interner,
        &sim.session,
        &sim.house_alliances,
        &mut sim.sound_events,
        &sim.houses,
        &pings,
        &rules,
    );
    assert_eq!(
        sim.sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::UnderAttack { miner, .. } => Some(*miner),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [true]
    );
}

/// A computer's defence whose target has left its range drops it when that
/// target hits it: BuildingClass::SetTarget hands the base setter NULL
/// (`0x00443BFE`), whose same-target test then compares NULL, both in
/// ReceiveDamage's retaliation Override (`0x00702B41`) and in the block's
/// SetTarget (the `ai_target_out_of_range` row).
#[test]
fn a_computer_defence_drops_its_out_of_range_target_when_hit_by_it() {
    let rules = production_rules("");
    let (mut sim, building, source) = production_fixture(&rules, "DEF", false, OUT_OF_RANGE);
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(source)));
    assert_eq!(hit(&mut sim, &rules, building, source), 1);
    let entity = sim.substrate.entities.get(building).unwrap();
    assert!(entity.attack_target.target().is_none());
    assert_eq!(
        entity.suspended_attack_target.target(),
        Some(TargetKind::Entity(source)),
        "the Override archived it"
    );
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Attack)
    );
}
