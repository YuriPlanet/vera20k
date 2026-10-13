//! A vehicle's own parts of `UnitClass::ReceiveDamage`
//! (`combat::unit_damage_response`): the native rows of
//! `UnitClass::Should_Crush_It @ 0x007438F0`, the retail keys it reads, and
//! Rust regression tests of the response and the war-factory return through
//! the production receiver on retail rules.

use super::unit_damage_response::{CrushCharge, crush_distance};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::world_receiver::{ReceiverRun, commit_entities};
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseDifficulty;
use crate::sim::miner::{CargoBale, ResourceType};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;

/// `tools/unit_crush_oracle.json` `should_crush_it`: `0x007438F0` executed on
/// scratch objects, its distance through the native `Sqrt_Approx` and ftol.
/// Some exact squares root a hair short of their root and chop below it, so a
/// source 461 leptons away on an axis is still within `Crush=` 460, and
/// (326, 326, 0) is not; 460 itself roots to 460.
#[test]
fn should_crush_it_matches_native_execution() {
    let payload: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/unit_crush_oracle.json")).unwrap();
    let rows = payload["should_crush_it"].as_array().unwrap();
    assert!(rows.len() >= 20);
    for row in rows {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap();
        let int = |key: &str| input[key].as_i64().unwrap() as i32;
        let xyz = |key: &str| -> [i32; 3] {
            let v = &input[key];
            [0, 1, 2].map(|axis| v[axis].as_i64().unwrap() as i32)
        };
        let charge = CrushCharge {
            crusher: flag("crusher") || flag("ability"),
            crushable: flag("crushable"),
            distance: crush_distance(xyz("unit"), xyz("source")),
            reach: int("crush"),
            easy: int("difficulty") == HouseDifficulty::Easy as i32,
            iq: int("iq"),
            iq_auto_crush: int("iq_auto_crush"),
            disguised: flag("disguised"),
        };
        assert_eq!(
            charge.admits(),
            row["output"]["admits"].as_bool().unwrap(),
            "{}",
            input["name"]
        );
    }
}

fn retail_rules() -> Option<RuleSet> {
    let (ini, art) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    Some(RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("retail rules"))
}

/// Retail's reach and IQ, and its miners: of the `Harvester=` vehicles (none
/// is a `Weeder=`), the War and Chrono Miners count `Tiberium` pips over a
/// positive `Storage=`; their unloading classes count none.
#[test]
fn retail_harvesters_carry_tiberium_pips() {
    use crate::rules::object_type::PipScale::{None, Tiberium};
    let Some(rules) = retail_rules() else {
        return;
    };
    assert_eq!(rules.combat_damage.crush, 460);
    assert_eq!(rules.general.iq_auto_crush, 2);
    let miners: Vec<_> = rules
        .vehicle_ids
        .iter()
        .filter_map(|id| {
            let object = rules.object(id)?;
            (object.harvester || object.weeder).then_some((
                id.as_str(),
                object.weeder,
                object.pip_scale,
                object.storage,
            ))
        })
        .collect();
    assert_eq!(
        miners,
        [
            ("HARV", false, Tiberium, 40),
            ("HORV", false, None, 0),
            ("CMON", false, None, 0),
            ("CMIN", false, Tiberium, 20),
            ("SMON", false, None, 0),
        ]
    );
}

fn arena(rules: &RuleSet) -> Simulation {
    crate::sim::arena_fixture::rival_arena(rules, 0x7438F0)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, cell: (u16, u16)) -> u64 {
    sim.spawn_object(kind, owner, cell.0, cell.1, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// One `SA` hit of `damage` on `target` from `source`, through the receiver.
fn hit(sim: &mut Simulation, rules: &RuleSet, target: u64, source: u64, damage: i32) {
    hit_with(sim, rules, target, source, damage, false);
}

fn hit_with(
    sim: &mut Simulation,
    rules: &RuleSet,
    target: u64,
    source: u64,
    damage: i32,
    ignore_defenses: bool,
) {
    hit_by(sim, rules, "SA", target, source, damage, ignore_defenses);
}

fn hit_by(
    sim: &mut Simulation,
    rules: &RuleSet,
    warhead: &str,
    target: u64,
    source: u64,
    damage: i32,
    ignore_defenses: bool,
) {
    let owner = sim
        .substrate
        .entities
        .get(source)
        .map(|entity| entity.owner());
    let warhead = sim.interner.intern(warhead);
    let flags = ReceiverCallFlags {
        ignore_defenses,
        arg6: false,
    };
    let event =
        EntityDamageEvent::direct_receiver(target, damage, 0, source, owner, warhead, flags);
    commit_entities(
        sim,
        &mut ReceiverRun::default(),
        &[event],
        None,
        rules,
        None,
    );
}

fn queued(sim: &Simulation, id: u64) -> MissionId {
    sim.substrate.entities.get(id).unwrap().mission.queued()
}

fn nav_com(sim: &Simulation, id: u64) -> Option<NavTargetRef> {
    sim.substrate.entities.get(id).unwrap().navigation.nav_com
}

const MOVE: MissionId = MissionId::from_known(MissionType::Move);
const ENTER: MissionId = MissionId::from_known(MissionType::Enter);

/// A computer's Rhino shot by a GI a cell away drives at it
/// (`SetDestination(source, 1)`, `Queue_Mission(Move)`), also when the hit
/// does no damage (result 0). It does not for a human's Rhino, a team
/// member, a tethered Rhino, an allied, distant or uncrushable source, an
/// Easy house or one whose IQ is below `AutoCrush=`, nor for a hit with no
/// source or a killing one.
#[test]
fn computer_crusher_charges_a_crushable_attacker() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let charge = |damage: i32, setup: &dyn Fn(&mut Simulation, u64, u64) -> u64| {
        let mut sim = arena(&rules);
        let rhino = spawn(&mut sim, &rules, "HTNK", "Russians", (10, 10));
        let gi = spawn(&mut sim, &rules, "E1", "Americans", (11, 10));
        let source = setup(&mut sim, rhino, gi);
        let health = |sim: &Simulation| sim.substrate.entities.get(rhino).unwrap().health.current;
        let before = health(&sim);
        hit(&mut sim, &rules, rhino, source, damage);
        assert_eq!(damage == 0, health(&sim) == before);
        nav_com(&sim, rhino) == Some(NavTargetRef::Entity { id: source })
            && queued(&sim, rhino) == MOVE
    };
    let unchanged = |_: &mut Simulation, _: u64, gi: u64| gi;
    assert!(charge(10, &unchanged));
    assert!(
        charge(0, &unchanged),
        "a hit that does no damage reaches it"
    );
    assert!(!charge(10, &|sim, _, gi| {
        let owner = sim.interner.get("Russians").unwrap();
        sim.houses.get_mut(&owner).unwrap().is_human = true;
        gi
    }));
    assert!(!charge(10, &|sim, rhino, gi| {
        let owner = sim.interner.get("Russians").unwrap();
        let script = sim.interner.intern("Script");
        sim.team_script_vm
            .create_team(owner, script, vec![rhino], 0);
        gi
    }));
    assert!(!charge(10, &|sim, _, gi| {
        sim.house_alliances = crate::map::houses::HouseAllianceMap::from([(
            "RUSSIANS".to_string(),
            std::collections::BTreeSet::from(["AMERICANS".to_string()]),
        )]);
        gi
    }));
    assert!(!charge(10, &|sim, _, _| {
        spawn(sim, &rules, "E1", "Americans", (12, 10))
    }));
    assert!(!charge(10, &|sim, _, _| {
        spawn(sim, &rules, "MTNK", "Americans", (10, 11))
    }));
    assert!(!charge(10, &|sim, _, gi| {
        let owner = sim.interner.get("Russians").unwrap();
        sim.houses.get_mut(&owner).unwrap().difficulty = HouseDifficulty::Easy;
        gi
    }));
    assert!(!charge(10, &|sim, _, gi| {
        let owner = sim.interner.get("Russians").unwrap();
        sim.houses.get_mut(&owner).unwrap().current_iq = 1;
        gi
    }));
    assert!(!charge(10, &|sim, rhino, gi| {
        sim.substrate
            .entities
            .get_mut(rhino)
            .unwrap()
            .dock_entered_with = Some(gi);
        gi
    }));

    // A hit with no source (EBX null) and a killing hit (result 4, the death
    // arm) never reach it.
    let lone = |health: Option<i32>, source_present: bool| {
        let mut sim = arena(&rules);
        let rhino = spawn(&mut sim, &rules, "HTNK", "Russians", (10, 10));
        let gi = spawn(&mut sim, &rules, "E1", "Americans", (11, 10));
        if let Some(health) = health {
            sim.substrate
                .entities
                .get_mut(rhino)
                .unwrap()
                .health
                .current = health;
        }
        let source = if source_present {
            gi
        } else {
            super::receiver_call::RAD_NO_ATTACKER
        };
        hit(&mut sim, &rules, rhino, source, 10);
        sim.substrate
            .entities
            .get(rhino)
            .map(|rhino| (rhino.health.current, rhino.mission.queued()))
    };
    let (health, mission) = lone(None, false).unwrap();
    assert!(health < rules.object("HTNK").unwrap().strength);
    assert_ne!(mission, MOVE);
    assert!(lone(Some(1), true).is_none_or(|(health, mission)| health == 0 && mission != MOVE));
}

/// A computer's Battle Fortress (`OmniCrusher=yes`, not
/// `ImmuneToPsionics=`) gassed by a Chaos Drone a cell away: the Psychedelic
/// arm's result 1 (`0x00701DBA`) reaches the Unit receiver's response
/// (`0x007384B9`), whose charge (`SetDestination(drone)`, `Queue_Mission(Move)`)
/// replaces the berserk Hunt.
#[test]
fn gassed_computer_fortress_charges_the_chaos_drone() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let fortress = spawn(&mut sim, &rules, "BFRT", "Russians", (10, 10));
    let drone = spawn(&mut sim, &rules, "CAOS", "Americans", (11, 10));
    hit_by(
        &mut sim,
        &rules,
        "PsychGasCreate",
        fortress,
        drone,
        600,
        false,
    );
    let unit = sim.substrate.entities.get(fortress).unwrap();
    assert!(unit.berserk.active);
    assert_eq!(unit.health.current, rules.object("BFRT").unwrap().strength);
    assert_eq!(
        nav_com(&sim, fortress),
        Some(NavTargetRef::Entity { id: drone })
    );
    assert_eq!(queued(&sim, fortress), MOVE);
}

/// The charge outlives the frame's pursuit pass. The Rhino still holds the GI
/// it retaliates against, in range and on Attack, but Unit AI commences the
/// queued Move (`0x00736473`) before Mission_Attack's approach or the range
/// stop could drop the destination.
#[test]
fn queued_charge_keeps_its_destination_through_pursuit() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let rhino = spawn(&mut sim, &rules, "HTNK", "Russians", (10, 10));
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (11, 10));
    hit(&mut sim, &rules, rhino, gi, 10);
    let entity = sim.substrate.entities.get(rhino).unwrap();
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        entity.attack_target.as_ref().map(|attack| attack.target),
        Some(TargetKind::Entity(gi))
    );
    assert!(entity.movement_target.is_some());
    sim.tick_attack_pursuit(&rules);
    assert_eq!(nav_com(&sim, rhino), Some(NavTargetRef::Entity { id: gi }));
    assert_eq!(queued(&sim, rhino), MOVE);
}

/// A computer's War Miner carrying ore and at `ConditionYellow` (half its
/// strength, equal counts), hit by a tank, sends its refinery HELLO and
/// queues Enter. An empty miner, one above yellow and one already in contact
/// with the refinery do not. The first `Dock=` type with a bay wins
/// (vt+0x530 per type), so a nearer GAREFN loses to the NAREFN.
#[test]
fn damaged_computer_miner_heads_for_its_refinery() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let strength = rules.object("HARV").unwrap().strength;
    let retreat = |bales: usize, health: i32, setup: &dyn Fn(&mut Simulation, u64, u64)| {
        let mut sim = arena(&rules);
        let refinery = spawn(&mut sim, &rules, "NAREFN", "Russians", (14, 4));
        let miner = spawn(&mut sim, &rules, "HARV", "Russians", (12, 12));
        let tank = spawn(&mut sim, &rules, "MTNK", "Americans", (12, 14));
        let entity = sim.substrate.entities.get_mut(miner).unwrap();
        entity.health.current = health;
        entity.miner.as_mut().unwrap().cargo = vec![
            CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            };
            bales
        ];
        setup(&mut sim, miner, refinery);
        hit(&mut sim, &rules, miner, tank, 1);
        let contact = sim
            .substrate
            .entities
            .get(miner)
            .unwrap()
            .radio_contacts
            .slot(0);
        (queued(&sim, miner) == ENTER, contact == Some(refinery))
    };
    let alone = |_: &mut Simulation, _: u64, _: u64| {};
    assert_eq!(retreat(10, strength / 2, &alone), (true, true));
    assert_eq!(retreat(0, strength / 2, &alone), (false, false));
    assert_eq!(retreat(10, strength - 10, &alone), (false, false));
    let in_contact = |sim: &mut Simulation, miner: u64, refinery: u64| {
        let entity = sim.substrate.entities.get_mut(miner).unwrap();
        entity.radio_contacts.insert(refinery).unwrap();
    };
    assert_eq!(retreat(10, strength / 2, &in_contact), (false, true));
    let nearer_garefn = |sim: &mut Simulation, _: u64, _: u64| {
        spawn(sim, &rules, "GAREFN", "Russians", (6, 12));
    };
    assert_eq!(retreat(10, strength / 2, &nearer_garefn), (true, true));
}

/// A vehicle on a cell of the War Factory it is in radio contact with takes
/// no hit unless the hit ignores defences.
#[test]
fn vehicle_inside_its_war_factory_takes_no_hit() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let health_after = |ignore_defenses: bool| {
        let mut sim = arena(&rules);
        let factory = spawn(&mut sim, &rules, "NAWEAP", "Russians", (14, 14));
        let rhino = spawn(&mut sim, &rules, "HTNK", "Russians", (10, 10));
        let gi = spawn(&mut sim, &rules, "E1", "Americans", (10, 12));
        let inside = (15, 15);
        assert_eq!(
            sim.substrate.occupancy.first_building_on_layer(
                inside.0,
                inside.1,
                crate::sim::movement::locomotor::MovementLayer::Ground
            ),
            Some(factory)
        );
        let entity = sim.substrate.entities.get_mut(rhino).unwrap();
        entity.position.rx = inside.0;
        entity.position.ry = inside.1;
        entity.radio_contacts.insert(factory).unwrap();
        let before = entity.health.current;
        hit_with(&mut sim, &rules, rhino, gi, 50, ignore_defenses);
        before - sim.substrate.entities.get(rhino).unwrap().health.current
    };
    assert_eq!(health_after(false), 0);
    assert!(health_after(true) > 0);
}
