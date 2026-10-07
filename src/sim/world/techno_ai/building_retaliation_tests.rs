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
use crate::sim::combat::{AttackTarget, EntityDamageEvent, TargetKind};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;

/// The oracle fixture's frame.
const FRAME: u32 = 200;
/// The building's cell; the source stands 3 cells east in range of `Gun`,
/// 10 out of it.
const BUILDING: (u16, u16) = (5, 5);
const IN_RANGE: (u16, u16) = (8, 5);
const OUT_OF_RANGE: (u16, u16) = (15, 5);

fn golden() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_retaliation.json",
    ))
    .unwrap()
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
        &IniFile::from_str(&format!(
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
    let id = sim.allocate_stable_id();
    let mut entity = GameEntity::test_default(id, kind, "Russians", cell.0, cell.1);
    entity.owner = sim.intern("Russians");
    entity.type_ref = sim.intern(kind);
    entity.category = category;
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
    for (house, human) in [
        ("Americans", input["human"].as_bool().unwrap_or(false)),
        ("Russians", false),
    ] {
        let id = sim.interner.intern(house);
        sim.houses
            .insert(id, HouseState::new(id, 0, Some(id), human, 5000, 10));
    }
    if input["allied"].as_bool().unwrap_or(false) {
        sim.house_alliances
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
    sim.session.binary_frame = FRAME;
    if input["operational"] == false {
        let owner = sim.interner.intern("Americans");
        let mut power_state = crate::sim::power_system::PowerState::default();
        power_state.total_drain = 100;
        power_state.is_low_power = true;
        sim.power_states.insert(owner, power_state);
    }
    sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
    let entity = sim.substrate.entities.get_mut(building).unwrap();
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
        entity.attack_target = source.map(AttackTarget::new);
    }
    let facing = &mut entity.body_facing;
    if let Some(desired) = input["initial_desired"].as_u64() {
        // The oracle writes the desired word alone; a snap leaves the same
        // settled facing (the start word only matters while rotating).
        facing.snap(desired as u16, FRAME);
    }
    if input["rotating"].as_bool().unwrap_or(false) {
        facing.set(0x4000, FRAME);
        sim.session.binary_frame = FRAME + input["advance"].as_u64().unwrap_or(0) as u32;
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
/// mission, Scenario draws (values and generator indices) and `+0x388`.
/// `+0x53C` (`who_last`) has no VERA state (module doc). RESIDUAL (dormant
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
            sim.session.binary_frame,
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
        // NotifyUnderAttack's cell is GetCoords': a 2x2's foundation centre
        // lies 128 leptons past its anchor cell's centre on both axes, in the
        // next cell (native GetCoords rows in building_fire_facing.json).
        let centre = u16::from(input["foundation"].as_i64().unwrap_or(3) != 0);
        assert_eq!(
            ping.map(|event| (event.structure, event.owner, event.rx, event.ry)),
            (row["ping"] == serde_json::json!([true])).then_some((
                true,
                before.owner(),
                BUILDING.0 + centre,
                BUILDING.1 + centre
            )),
            "{name} ping"
        );
        assert_eq!(
            after.attack_target.as_ref().map(|attack| attack.target),
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
    RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&format!(
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
    for (house, human) in [("Americans", human), ("Russians", false)] {
        let id = sim.interner.intern(house);
        sim.houses
            .insert(id, HouseState::new(id, 0, Some(id), human, 5000, 10));
    }
    let building = sim
        .spawn_object(kind, "Americans", BUILDING.0, BUILDING.1, 0, rules)
        .unwrap();
    let source = sim
        .spawn_object("GUN", "Russians", cell.0, cell.1, 0, rules)
        .unwrap();
    sim.session.binary_frame = FRAME;
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
    pings.iter().filter(|ping| ping.structure).count()
}

/// A human's turreted defence hit from out of range pings and turns its
/// turret to the low byte of one Scenario draw; hit again while the turret
/// still turns, it pings without a draw. The first draw of seed 1 is the
/// oracle's `human_seed1` (0x78B76ED5, so 0xD500 in 4 frames at ROT 10).
#[test]
fn a_human_defence_hit_from_out_of_range_turns_its_turret_once() {
    let rules = production_rules("");
    let (mut sim, building, source) = production_fixture(&rules, "DEF", true, OUT_OF_RANGE);
    let mut expected = sim.scenario_rng.clone();
    let drawn = expected.next_u32();
    assert_eq!(drawn, 0x78B7_6ED5);

    assert_eq!(hit(&mut sim, &rules, building, source), 1);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "one Scenario draw"
    );
    let entity = sim.substrate.entities.get(building).unwrap();
    assert_eq!(entity.health.current, 990);
    assert!(entity.attack_target.is_none());
    let turret = entity.body_facing;
    assert_eq!(turret.destination(), 0xD500);
    assert_eq!(turret.timer_duration(), 4);
    assert!(turret.is_rotating(FRAME));

    assert_eq!(hit(&mut sim, &rules, building, source), 1);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "no draw while the turret turns"
    );
    assert_eq!(
        sim.substrate.entities.get(building).unwrap().body_facing,
        turret
    );
}

/// `Insignificant=` gates only the ping: a human's turretless insignificant
/// building still turns its body (`+0x388`, which every building carries).
#[test]
fn an_insignificant_building_turns_without_a_ping() {
    let rules = production_rules("");
    let (mut sim, building, source) = production_fixture(&rules, "SHACK", true, OUT_OF_RANGE);
    assert_eq!(hit(&mut sim, &rules, building, source), 0);
    let body = sim.substrate.entities.get(building).unwrap().body_facing;
    assert_eq!(body.destination(), 0xD500);
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
    assert_eq!(
        pings
            .iter()
            .map(|ping| (ping.structure, ping.miner))
            .collect::<Vec<_>>(),
        [(true, true)]
    );
}

/// A computer's defence hit from in range takes the source; with
/// `PlayerReturnFire=` a human's does too, and neither draws.
#[test]
fn a_returned_hit_takes_the_source_without_a_draw() {
    for (human, extra) in [
        (false, ""),
        (true, "[CombatDamage]\nPlayerReturnFire=yes\n"),
    ] {
        let rules = production_rules(extra);
        let (mut sim, building, source) = production_fixture(&rules, "DEF", human, IN_RANGE);
        let rng = sim.scenario_rng.logical_state();
        assert_eq!(hit(&mut sim, &rules, building, source), 1);
        assert_eq!(sim.scenario_rng.logical_state(), rng, "human {human}");
        let entity = sim.substrate.entities.get(building).unwrap();
        assert_eq!(
            entity.attack_target.as_ref().map(|attack| attack.target),
            Some(TargetKind::Entity(source)),
            "human {human}"
        );
        assert_eq!(entity.body_facing.destination(), 0, "human {human}");
    }
}

/// A hit without a source object neither pings nor turns (`0x00442942`).
#[test]
fn a_hit_without_a_source_does_nothing() {
    let rules = production_rules("");
    let (mut sim, building, _) = production_fixture(&rules, "DEF", true, OUT_OF_RANGE);
    let rng = sim.scenario_rng.logical_state();
    let warhead = sim.interner.intern("AP");
    let event = EntityDamageEvent::area(
        building,
        10,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
    );
    let (_, pings) = commit_entities(
        &mut sim,
        &mut ReceiverRun::default(),
        &[event],
        None,
        &rules,
        None,
    );
    assert!(pings.is_empty());
    assert_eq!(sim.scenario_rng.logical_state(), rng);
    assert_eq!(
        sim.substrate.entities.get(building).unwrap().health.current,
        990
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
        .attack_target = Some(AttackTarget::new(source));
    assert_eq!(hit(&mut sim, &rules, building, source), 1);
    let entity = sim.substrate.entities.get(building).unwrap();
    assert!(entity.attack_target.is_none());
    assert_eq!(
        entity.suspended_attack_target,
        Some(TargetKind::Entity(source)),
        "the Override archived it"
    );
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Attack)
    );
}
