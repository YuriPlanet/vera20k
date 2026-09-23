//! Tests for the mind-control owner. The `native_` tests compare against
//! corpora produced by running the original functions
//! (`tools/spatial_oracle/capture_*.py`); elsewhere expected draws are
//! replayed on a clone of the Scenario stream in the native order recorded on
//! each function (Rust regression only).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;

const RULES: &str = "\
[General]
AICaptureNormal=75,5,5,15
AICaptureWounded=15,40,40,5
AICaptureLowPower=15,5,75,5
AICaptureLowMoney=15,75,5,5
AICaptureLowMoneyMark=2000
AICaptureWoundedMark=.25
[CombatDamage]
OverloadCount=3,6,10,50
OverloadDamage=0,50,100,500
OverloadFrames=30,60,60,60
ControlledAnimationType=MINDANIM
C4Warhead=Super
[AudioVisual]
YuriMindControlSound=YuriMindControl
MindClearedSound=MindCleared
MasterMindOverloadDeathSound=MasterMindOverloadVoice
[InfantryTypes]
0=YURI
1=E1
2=DOG
[VehicleTypes]
0=MIND
1=HTNK
2=AMCV
3=TRNS
[AircraftTypes]
[BuildingTypes]
0=YAPSYT
1=GAPOWR
2=GACNST
3=BIOR
[Warheads]
0=Controller
1=Super
2=KILLWH
[YURI]
Strength=100
Speed=4
Sight=6
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=MindControl
ImmuneToPsionics=yes
[E1]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[DOG]
Strength=100
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
ImmuneToPsionics=yes
[MIND]
Strength=500
Speed=4
ROT=5
Sight=6
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
Primary=MultipleMindControlTank
ImmuneToPsionics=yes
[HTNK]
Strength=400
Speed=4
[YAPSYT]
Strength=1000
Foundation=2x2
Primary=MultipleMindControlTower
ImmuneToPsionics=yes
[GAPOWR]
Strength=750
Foundation=2x2
ImmuneToPsionics=no
[AMCV]
Strength=1000
Speed=4
DeploysInto=GACNST
[GACNST]
Strength=1000
Foundation=4x4
ConstructionYard=yes
UndeploysInto=AMCV
ImmuneToPsionics=no
[BIOR]
Strength=900
Foundation=2x2
Passengers=5
[TRNS]
Strength=300
Speed=4
Passengers=5
[MindControl]
Damage=1
ROF=200
Range=7
Speed=100
Projectile=PsychicControl
Warhead=Controller
[MultipleMindControlTank]
Damage=3
ROF=10
Range=6
Speed=100
InfiniteMindControl=yes
Projectile=PsychicControl
Warhead=Controller
[MultipleMindControlTower]
Damage=3
ROF=100
Range=7
Projectile=PsychicControl
Warhead=Controller
[PsychicControl]
Inviso=yes
[Controller]
MindControl=yes
Verses=100%,100%,100%,100%,100%,100%,0%,0%,0%,100%,100%
[Super]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[KILLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const ART: &str = "\
[MINDANIM]
LoopCount=-1
Rate=300
[GAPOWR]
Foundation=2x2
Height=3
";

fn rules() -> RuleSet {
    rules_from(RULES)
}

fn rules_from(text: &str) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(text)).expect("mind control rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(ART));
    art.bind_anim_frame_count_for_test("MINDANIM", 8);
    rules.art_registry = art;
    rules
}

/// YuriCountry (computer, side 2), Americans (human, side 0) and Russians
/// (computer, side 1).
fn sim(seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    for (name, side, human) in [
        ("YuriCountry", 2, false),
        ("Americans", 0, true),
        ("Russians", 1, false),
    ] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn owner(sim: &Simulation, id: u64) -> String {
    let entity = sim.substrate.entities.get(id).expect("entity");
    sim.interner.resolve(entity.owner()).to_string()
}

fn victims(sim: &Simulation, controller: u64) -> Vec<u64> {
    sim.substrate
        .entities
        .get(controller)
        .and_then(|entity| entity.capture_manager.as_ref())
        .map(|manager| manager.victims().collect())
        .unwrap_or_default()
}

fn voc_sounds(sim: &Simulation) -> Vec<String> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::VocAt { sound_id, .. } => Some(sound_id.clone()),
            _ => None,
        })
        .collect()
}

/// The `VocAt` sounds with the cell each plays at.
fn voc_cells(sim: &Simulation) -> Vec<(String, u16, u16)> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::VocAt {
                sound_id, rx, ry, ..
            } => Some((sound_id.clone(), *rx, *ry)),
            _ => None,
        })
        .collect()
}

fn queued_mission(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.substrate.entities.get(id)?.mission.queued().known()
}

/// One DecideUnitFate draw for a computer-owned result.
fn fate_draw(replay: &mut SimRng) {
    let _roll = replay.next_range_i32_inclusive(1, 100);
}

/// Weapon 0 decides the manager: Yuri's single link, the Mastermind's
/// infinite one, the Psychic Tower's three.
#[test]
fn init_managers_reads_weapon_zero() {
    let rules = rules();
    let limits = |kind: &str| {
        init_capture_manager(rules.object(kind).unwrap(), &rules)
            .map(|manager| (manager.max_control, manager.infinite))
    };
    assert_eq!(limits("YURI"), Some((1, false)));
    assert_eq!(limits("MIND"), Some((3, true)));
    assert_eq!(limits("YAPSYT"), Some((3, false)));
    assert_eq!(limits("E1"), None);
}

/// CaptureUnit: the target changes house, records the house it had, wears
/// the ring, and a computer-owned result takes one fate draw that sends it
/// hunting.
#[test]
fn capture_takes_the_target_and_records_its_house() {
    let rules = rules();
    let mut sim = sim(3);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let health = sim.substrate.entities.get(gi).unwrap().health.current;
    let mut replay = sim.scenario_rng.clone();
    assert!(sim.capture_unit(yuri, gi, &rules));
    fate_draw(&mut replay);

    assert_eq!(owner(&sim, gi), "YuriCountry");
    let entity = sim.substrate.entities.get(gi).unwrap();
    assert_eq!(entity.mind_control.controller(), Some(yuri));
    assert_eq!(entity.health.current, health);
    let manager = sim
        .substrate
        .entities
        .get(yuri)
        .unwrap()
        .capture_manager
        .as_ref()
        .unwrap();
    assert_eq!(manager.victims().collect::<Vec<_>>(), [gi]);
    assert_eq!(
        manager.original_owner(gi),
        Some(sim.interner.get("Americans").unwrap())
    );
    // The ring is MINDANIM, attached to the captive, 0x8C above it.
    let ring = entity.mind_control.ring_anim.expect("ring anim");
    let anim = sim.anim(ring).expect("live ring");
    assert_eq!(sim.interner.resolve(anim.type_id), "MINDANIM");
    assert_eq!(anim.owner_entity, Some(gi));
    let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    assert_eq!(sim.anim_absolute_coord(ring).unwrap().z, location.z + 0x8C);
    // Computer house: one RandomRanged(1, 100), and every stock choice hunts.
    assert_eq!(queued_mission(&sim, gi), Some(MissionType::Hunt));
    assert_eq!(sim.scenario_rng.state(), replay.state());
}

/// A captured building wears its ring at its GetCoords (`0x00447AC0`, the
/// foundation centre), `Height=` levels up at the level height the original
/// static initializer computes (`capture_ring_height.json`), sorted 1024
/// lower.
#[test]
fn a_captured_buildings_ring_sits_height_levels_above_its_centre() {
    let native: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/capture_ring_height.json"
    ))
    .unwrap();
    assert_eq!(
        native["level_height"],
        crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
    );
    let rules = rules();
    let mut sim = sim(25);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 14, 10);
    // Built: CanCapture refuses a building still in Construction.
    let now = sim.session.binary_frame;
    let _ = sim.mission_assign_exact(plant, MissionId::from_known(MissionType::Guard), now);
    assert!(sim.capture_unit(yuri, plant, &rules));
    let entity = sim.substrate.entities.get(plant).unwrap();
    let ring = entity.mind_control.ring_anim.expect("ring anim");
    let north_west = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    let centre = sim.anim_owner_coords(plant).unwrap();
    assert_eq!(
        (centre.x, centre.y),
        (north_west.x + 128, north_west.y + 128),
        "the 2x2 foundation centre"
    );
    let at = sim.anim_absolute_coord(ring).unwrap();
    assert_eq!((at.x, at.y, at.z), (centre.x, centre.y, centre.z + 3 * 104));
    assert_eq!(sim.anim(ring).unwrap().z_adjust, -1024);
}

/// A single-link controller releases its victim before taking the next one:
/// the first returns home (a human house: no fate draw), loses its ring and
/// plays MindCleared; the manager holds one node.
#[test]
fn a_single_link_controller_releases_its_victim_first() {
    let rules = rules();
    let mut sim = sim(5);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let first = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let second = spawn(&mut sim, &rules, "E1", "Americans", 12, 12);
    assert!(sim.capture_unit(yuri, first, &rules));
    let ring = sim
        .substrate
        .entities
        .get(first)
        .unwrap()
        .mind_control
        .ring_anim
        .unwrap();
    let mut replay = sim.scenario_rng.clone();
    sim.sound_events.clear();

    assert!(sim.capture_unit(yuri, second, &rules));
    fate_draw(&mut replay);

    assert_eq!(owner(&sim, first), "Americans");
    let released = sim.substrate.entities.get(first).unwrap();
    assert!(!released.mind_control.is_mind_controlled());
    assert!(released.mind_control.ring_anim.is_none());
    assert!(sim.anim(ring).is_none_or(|anim| anim.runtime.inactive));
    assert_eq!(voc_sounds(&sim), ["MindCleared"]);
    assert_eq!(victims(&sim, yuri), [second]);
    assert_eq!(owner(&sim, second), "YuriCountry");
    assert_eq!(sim.scenario_rng.state(), replay.state());
}

/// CanCapture refuses the controller's own house, psionic immunity, a unit
/// already controlled, an Iron-Curtained target, a Selling object and a full
/// finite manager; a single-link manager always admits (it replaces).
#[test]
fn can_capture_gates() {
    let rules = rules();
    let mut sim = sim(7);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let tower = spawn(&mut sim, &rules, "YAPSYT", "YuriCountry", 20, 20);
    let own = spawn(&mut sim, &rules, "E1", "YuriCountry", 11, 11);
    let dog = spawn(&mut sim, &rules, "DOG", "Americans", 12, 11);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 13, 11);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 14, 11);
    assert!(!sim.can_capture(yuri, own, &rules), "own house");
    assert!(!sim.can_capture(yuri, dog, &rules), "ImmuneToPsionics");
    assert!(sim.can_capture(yuri, gi, &rules));
    // An ally is not refused: only the controller's own house is.
    sim.house_alliances
        .entry("YURICOUNTRY".to_string())
        .or_default()
        .insert("RUSSIANS".to_string());
    sim.house_alliances
        .entry("RUSSIANS".to_string())
        .or_default()
        .insert("YURICOUNTRY".to_string());
    assert!(sim.can_capture(yuri, tank, &rules), "an allied tank");

    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(tank).unwrap(),
        sim.session.binary_frame,
        100,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    assert!(!sim.can_capture(yuri, tank, &rules), "Iron Curtain");

    assert!(sim.capture_unit(tower, gi, &rules));
    assert!(!sim.can_capture(yuri, gi, &rules), "already controlled");
    // A single-link Yuri with a victim still admits another.
    let other = spawn(&mut sim, &rules, "E1", "Americans", 15, 11);
    assert!(sim.capture_unit(yuri, other, &rules));
    let third = spawn(&mut sim, &rules, "E1", "Americans", 16, 11);
    assert!(sim.can_capture(yuri, third, &rules));

    // A finite manager at its limit is full.
    for x in 17..19 {
        let extra = spawn(&mut sim, &rules, "E1", "Americans", x, 11);
        assert!(sim.capture_unit(tower, extra, &rules));
    }
    let manager = sim
        .substrate
        .entities
        .get(tower)
        .unwrap()
        .capture_manager
        .as_ref()
        .unwrap();
    assert!(manager.is_full());
    assert!(!sim.can_capture(tower, third, &rules), "full");

    let seller = spawn(&mut sim, &rules, "E1", "Americans", 19, 12);
    let now = sim.session.binary_frame;
    let _ = sim.mission_assign_exact(seller, MissionId::from_known(MissionType::Selling), now);
    assert!(!sim.can_capture(yuri, seller, &rules), "Selling");
}

/// GetFireError's MindControl gate in the weapon ladder: Yuri's only weapon is
/// illegal against an immune or own-house target and legal against an enemy.
#[test]
fn the_weapon_ladder_refuses_an_uncapturable_target() {
    let rules = rules();
    let mut sim = sim(9);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let dog = spawn(&mut sim, &rules, "DOG", "Americans", 12, 11);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 13, 11);
    let yuri_entity = sim.substrate.entities.get(yuri).unwrap();
    let yuri_obj = rules.object("YURI").unwrap();
    let facts = crate::sim::combat::combat_weapon::attacker_facts(yuri_entity, yuri_obj);
    let select = |target: u64| {
        crate::sim::combat::combat_weapon::select_weapon_against(
            &rules,
            yuri_obj,
            &facts,
            yuri_entity.owner(),
            &crate::sim::combat::TargetKind::Entity(target),
            &sim.substrate.entities,
            &sim.interner,
            None,
            None,
        )
        .is_some()
    };
    assert!(!select(dog));
    assert!(select(gi));
}

/// The controller's death frees every captive, newest node first, before its
/// own death sounds: a captive returning to a computer house draws its fate,
/// one returning to a human house does not.
#[test]
fn the_controllers_death_frees_its_captives_newest_first() {
    let rules = rules();
    let mut sim = sim(11);
    let mind = spawn(&mut sim, &rules, "MIND", "YuriCountry", 10, 10);
    let human = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let computer = spawn(&mut sim, &rules, "E1", "Russians", 12, 12);
    assert!(sim.capture_unit(mind, human, &rules));
    assert!(sim.capture_unit(mind, computer, &rules));
    let mut replay = sim.scenario_rng.clone();
    sim.sound_events.clear();

    let warhead = sim.interner.intern("KILLWH");
    let hit = EntityDamageEvent::direct_receiver(
        mind,
        100_000,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit]);
    // Newest first: the computer-house captive's fate draw, then none for the
    // human one.
    fate_draw(&mut replay);

    assert_eq!(owner(&sim, human), "Americans");
    assert_eq!(owner(&sim, computer), "Russians");
    assert!([human, computer].iter().all(|&id| {
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .mind_control
            .is_mind_controlled()
    }));
    // Newest node first: the second captive's MindCleared plays first, at
    // its own cell.
    assert_eq!(
        voc_cells(&sim),
        [
            ("MindCleared".to_string(), 12, 12),
            ("MindCleared".to_string(), 12, 10),
        ]
    );
    assert_eq!(sim.scenario_rng.state(), replay.state());
}

/// A captive's own death only drops its node: no owner change, no sound.
#[test]
fn a_captives_death_drops_its_node_silently() {
    let rules = rules();
    let mut sim = sim(13);
    let tower = spawn(&mut sim, &rules, "YAPSYT", "YuriCountry", 20, 20);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    assert!(sim.capture_unit(tower, gi, &rules));
    sim.sound_events.clear();
    sim.uninit_with_rules(gi, &rules);
    assert!(victims(&sim, tower).is_empty());
    assert!(voc_sounds(&sim).is_empty());
}

/// Foot UnInit (a crushed or removed controller) frees its captives first.
#[test]
fn a_removed_foot_controller_frees_its_captives() {
    let rules = rules();
    let mut sim = sim(15);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    assert!(sim.capture_unit(yuri, gi, &rules));
    sim.uninit_with_rules(yuri, &rules);
    assert_eq!(owner(&sim, gi), "Americans");
    assert!(
        !sim.substrate
            .entities
            .get(gi)
            .unwrap()
            .mind_control
            .is_mind_controlled()
    );
}

/// A Psychic Tower losing power (the operational off edge) frees its captives.
#[test]
fn a_tower_going_offline_frees_its_captives() {
    let rules = rules();
    let mut sim = sim(17);
    let tower = spawn(&mut sim, &rules, "YAPSYT", "YuriCountry", 20, 20);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    assert!(sim.capture_unit(tower, gi, &rules));
    sim.substrate
        .entities
        .get_mut(tower)
        .unwrap()
        .building_last_operational = true;
    sim.substrate
        .entities
        .get_mut(tower)
        .unwrap()
        .health
        .current = 0;
    sim.visit_building_operational(tower, &rules);
    assert_eq!(owner(&sim, gi), "Americans");
    assert!(victims(&sim, tower).is_empty());
}

/// The fate walk returns the first choice whose running sum reaches the roll,
/// at the stock Normal table's boundaries.
#[test]
fn the_fate_table_walk() {
    let normal = [75, 5, 5, 15];
    assert_eq!(capture_decision(&normal, 1), Some(1));
    assert_eq!(capture_decision(&normal, 75), Some(1));
    assert_eq!(capture_decision(&normal, 76), Some(2));
    assert_eq!(capture_decision(&normal, 85), Some(3));
    assert_eq!(capture_decision(&normal, 86), Some(4));
    assert_eq!(capture_decision(&normal, 100), Some(4));
    // A table that runs out, or a sixth choice, returns without an order.
    assert_eq!(capture_decision(&[10, 10], 50), None);
    assert_eq!(capture_decision(&[1, 1, 1, 1, 1, 1, 1], 7), None);
    assert_eq!(capture_decision(&[1, 1, 1, 1, 1, 1, 1], 6), Some(6));
    assert_eq!(capture_decision(&[0, 0, 0, 0, 50], 20), Some(5));
}

/// Original DecideUnitFate `0x004723B0` under Unicorn
/// (`tools/spatial_oracle/capture_decide_fate.py`): for every row, the
/// reason table the original read, the arm its choice took (Hunt, grinder,
/// absorber or none), whether Hunt was queued and the Scenario stream after
/// the call. The unit's own house is poor and unpowered, so reading it
/// instead of the controller's would change the reason.
#[test]
fn native_decide_unit_fate_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/capture_decide_fate.json"
    ))
    .unwrap();
    let rows = corpus.as_array().unwrap();
    assert_eq!(rows.len(), 156);
    let hunt = MissionId::from_known(MissionType::Hunt).raw();
    let mut by_strength = std::collections::BTreeMap::new();
    for row in rows {
        let input = &row["input"];
        let int = |key: &str| input[key].as_i64().unwrap() as i32;
        let strength = int("strength");
        let rules = by_strength.entry(strength).or_insert_with(|| {
            rules_from(&RULES.replace("[E1]\nStrength=125", &format!("[E1]\nStrength={strength}")))
        });
        let tables: Vec<Vec<i32>> = input["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|table| {
                table
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|weight| weight.as_i64().unwrap() as i32)
                    .collect()
            })
            .collect();
        rules.mind_control.ai_capture = tables.try_into().unwrap();
        rules.mind_control.ai_capture_low_money_mark = int("money_mark");
        rules.mind_control.ai_capture_wounded_mark = input["wounded_mark"].as_f64().unwrap() as f32;
        let rules: &RuleSet = rules;

        let mut sim = sim(1);
        let yuri = spawn(&mut sim, rules, "YURI", "YuriCountry", 10, 10);
        let human = int("unit_house_human") != 0;
        let unit_house = if human { "Americans" } else { "Russians" };
        let unit = spawn(&mut sim, rules, "E1", unit_house, 12, 10);
        sim.substrate.entities.get_mut(unit).unwrap().health.current = int("health");
        let controller_house = sim.interner.get("YuriCountry").unwrap();
        sim.houses
            .get_mut(&controller_house)
            .unwrap()
            .economy
            .credits = int("money");
        sim.power_states.insert(
            controller_house,
            crate::sim::power_system::PowerState {
                total_output: int("produced"),
                total_drain: int("drained"),
                ..Default::default()
            },
        );
        let russians = sim.interner.get("Russians").unwrap();
        sim.houses.get_mut(&russians).unwrap().economy.credits = 0;
        sim.power_states.insert(
            russians,
            crate::sim::power_system::PowerState {
                total_output: 0,
                total_drain: 100,
                ..Default::default()
            },
        );
        let seed = input["seed"].as_u64().unwrap();
        sim.scenario_rng = SimRng::new(seed);
        assert_ne!(queued_mission(&sim, unit), Some(MissionType::Hunt));

        sim.decide_unit_fate(yuri, unit, rules);

        let events = row["events"].as_array().unwrap();
        let has = |key: &str| events.iter().any(|event| event.get(key).is_some());
        assert_eq!(
            sim.scenario_rng.next_u32(),
            row["next_random"].as_u64().unwrap() as u32,
            "{input}"
        );
        assert_eq!(
            queued_mission(&sim, unit) == Some(MissionType::Hunt),
            has("queue_mission"),
            "{input}"
        );
        if let Some(queued) = events
            .iter()
            .find_map(|event| event["queue_mission"].as_array())
        {
            assert_eq!(queued, &[serde_json::json!(hunt), serde_json::json!(0)]);
        }
        if human {
            assert!(events.is_empty(), "{input}");
            continue;
        }
        let health = crate::sim::components::Health {
            current: int("health"),
        };
        let reason = sim.capture_reason(controller_house, health, strength, rules);
        if let Some(table) = events.iter().find_map(|event| event["table"].as_str()) {
            let native = match table {
                "low_money" => CaptureReason::LowMoney,
                "low_power" => CaptureReason::LowPower,
                "wounded" => CaptureReason::Wounded,
                "normal" => CaptureReason::Normal,
                other => panic!("reason {other}"),
            };
            assert_eq!(reason, native, "{input}");
        }
        let mut replay = SimRng::new(seed);
        let roll = replay.next_range_i32_inclusive(1, 100);
        let arm = match capture_decision(rules.mind_control.ai_capture_table(reason), roll) {
            Some(2) => "grinder",
            Some(3) => "absorber",
            Some(1 | 4 | 6) => "hunt",
            None | Some(5) => "none",
            Some(other) => panic!("choice {other}"),
        };
        let native_arm = if has("grinder") {
            "grinder"
        } else if has("absorber") {
            "absorber"
        } else if has("queue_mission") {
            "hunt"
        } else {
            "none"
        };
        assert_eq!(arm, native_arm, "{input}");
    }
}

/// Original CaptureManagerClass::Update `0x00471A50` after the original
/// constructor, frame by frame (`tools/spatial_oracle/capture_overload_update.py`):
/// which frames check, each check's C4 damage, voice and five spark systems
/// (offsets from the controller's Location), the countdown and voice latch
/// after the run, whether the controller survived, and the Scenario stream.
/// The native ReceiveDamage is recorded rather than run, so the Rust
/// controller is given the Strength to survive every row except in the
/// killing case, whose captives do not exist (no fate draws either side).
#[test]
fn native_overload_update_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/capture_overload_update.json"
    ))
    .unwrap();
    let cases = corpus.as_array().unwrap();
    assert_eq!(cases.len(), 22);
    let text = RULES
        .replace("[MIND]\nStrength=500", "[MIND]\nStrength=100000")
        .replace(
            "C4Warhead=Super\n",
            "C4Warhead=Super\nDefaultSparkSystem=SparkSys\n",
        )
        + "[ParticleSystems]\n0=SparkSys\n[SparkSys]\nBehavesLike=Spark\nLifetime=5\n";
    let mut rules = rules_from(&text);
    let spark_type = rules.ps_type_id_by_name("SparkSys").expect("spark system");
    let ints = |value: &serde_json::Value| -> Vec<i32> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_i64().unwrap() as i32)
            .collect()
    };
    let nodes = |count: i64| -> Vec<ControlNode> {
        (0..count as u64)
            .map(|index| ControlNode {
                victim: 1_000_000 + index,
                original_owner: InternedId::default(),
            })
            .collect()
    };
    for case in cases {
        let input = &case["input"];
        let name = input["name"].as_str().unwrap();
        let (count, damage, frames) = match input.get("tables") {
            Some(tables) => (
                ints(&tables["count"]),
                ints(&tables["damage"]),
                ints(&tables["frames"]),
            ),
            None => (
                vec![3, 6, 10, 50],
                vec![0, 50, 100, 500],
                vec![30, 60, 60, 60],
            ),
        };
        rules.mind_control.overload_count = count;
        rules.mind_control.overload_damage = damage;
        rules.mind_control.overload_frames = frames;
        let rules = &rules;

        let mut sim = sim(1);
        let mind = spawn(&mut sim, rules, "MIND", "YuriCountry", 10, 10);
        let infinite = input
            .get("infinite")
            .is_none_or(|flag| flag.as_bool().unwrap());
        let constructed = &case["constructed"];
        let max = constructed["max"].as_i64().unwrap() as i32;
        let mut manager = CaptureManagerState::with_victims_for_test(max, infinite, &[]);
        manager.nodes = nodes(input["captives"].as_i64().unwrap());
        assert_eq!(
            manager.overload_countdown,
            constructed["countdown"].as_i64().unwrap() as i32
        );
        assert_eq!(manager.overload_sound_played, constructed["latch"] != 0);
        assert_eq!(manager.infinite, constructed["infinite"] != 0);
        let entity = sim.substrate.entities.get_mut(mind).unwrap();
        entity.capture_manager = Some(manager);
        if input.get("kill").is_some() {
            // Dies to the first damaging row, as the fixture's owner does.
            entity.health.current = 50;
        }
        let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let location = glam::IVec3::new(location.x, location.y, location.z);
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
        let native_location = ints(
            &input
                .get("location")
                .cloned()
                .unwrap_or(serde_json::json!([12928, 25728, 480])),
        );
        let native_frames: std::collections::BTreeMap<i64, &serde_json::Value> = case["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|frame| (frame["frame"].as_i64().unwrap(), frame))
            .collect();

        for frame in 1..=input["calls"].as_i64().unwrap() {
            if let Some(count) = input
                .get("captives_at")
                .and_then(|changes| changes.get(frame.to_string()))
            {
                sim.substrate
                    .entities
                    .get_mut(mind)
                    .and_then(|entity| entity.capture_manager.as_mut())
                    .unwrap()
                    .nodes = nodes(count.as_i64().unwrap());
            }
            let health = sim.substrate.entities.get(mind).unwrap().health.current;
            let systems: Vec<u64> = sim.particle_systems().iter().map(|(&id, _)| id).collect();
            sim.sound_events.clear();

            sim.capture_manager_update(mind, rules, None);

            let events: Vec<&serde_json::Value> = native_frames
                .get(&frame)
                .map(|native| native["events"].as_array().unwrap().iter().collect())
                .unwrap_or_default();
            let native_damage = events
                .iter()
                .find_map(|event| event["damage"].as_i64())
                .unwrap_or(0) as i32;
            if let Some(event) = events.iter().find(|event| event.get("damage").is_some()) {
                assert_eq!(
                    event["args"],
                    serde_json::json!([0, 0x0C4C_4C40, 0, 0, 0, 0])
                );
            }
            let dealt = health
                - sim
                    .substrate
                    .entities
                    .get(mind)
                    .map_or(0, |entity| entity.health.current);
            assert_eq!(dealt, native_damage, "{name} frame {frame}");
            // The voice plays at the controller's Location on both sides.
            let voices: Vec<glam::IVec3> = sim
                .sound_events
                .iter()
                .filter_map(|event| match event {
                    SimSoundEvent::VocAt {
                        sound_id,
                        rx,
                        ry,
                        sub_x,
                        sub_y,
                        world_z_leptons,
                        ..
                    } if sound_id == "MasterMindOverloadVoice" => Some(glam::IVec3::new(
                        i32::from(*rx) * 256 + sub_x.to_num::<i32>(),
                        i32::from(*ry) * 256 + sub_y.to_num::<i32>(),
                        *world_z_leptons,
                    )),
                    _ => None,
                })
                .collect();
            let native_voices = events
                .iter()
                .filter(|event| event.get("sound").is_some())
                .inspect(|event| assert_eq!(ints(&event["at"]), native_location, "{name}"))
                .count();
            assert_eq!(voices.len(), native_voices, "{name} frame {frame}");
            assert!(
                voices.iter().all(|&at| at == location),
                "{name} frame {frame}"
            );
            let sparks: Vec<[i32; 3]> = sim
                .particle_systems()
                .iter()
                .filter(|(id, _)| !systems.contains(id))
                .map(|(_, system)| {
                    assert_eq!(system.type_id, spark_type);
                    assert_eq!(system.target_coords, glam::IVec3::ZERO);
                    assert!(system.attached_entity.is_none() && system.owner_entity.is_none());
                    assert!(system.owner_house.is_none());
                    let offset = system.coords - location;
                    [offset.x, offset.y, offset.z]
                })
                .collect();
            let native_sparks: Vec<[i32; 3]> = events
                .iter()
                .filter(|event| event.get("spark").is_some())
                .map(|event| {
                    assert_eq!(event["target_at"], serde_json::json!([0, 0, 0]));
                    let at = ints(&event["at"]);
                    [
                        at[0] - native_location[0],
                        at[1] - native_location[1],
                        at[2] - native_location[2],
                    ]
                })
                .collect();
            assert_eq!(sparks, native_sparks, "{name} frame {frame}");
        }

        let final_state = &case["final"];
        let alive = sim
            .substrate
            .entities
            .get(mind)
            .is_some_and(|entity| entity.lifecycle.object_alive);
        assert_eq!(alive, final_state["alive"] != 0, "{name}");
        if alive {
            let manager = sim
                .substrate
                .entities
                .get(mind)
                .and_then(|entity| entity.capture_manager.as_ref())
                .unwrap();
            assert_eq!(
                manager.overload_countdown,
                final_state["countdown"].as_i64().unwrap() as i32,
                "{name}"
            );
            assert_eq!(
                manager.overload_sound_played,
                final_state["latch"] != 0,
                "{name}"
            );
        }
        assert_eq!(
            sim.scenario_rng.next_u32(),
            case["next_random"].as_u64().unwrap() as u32,
            "{name}"
        );
    }
}

/// The Mastermind overload: at three captives or fewer a check every 30
/// frames costs nothing; a fourth brings 50 C4 damage every 60 frames, five
/// spark systems (ten draws), the lean's sign (one draw) and the voice once
/// per episode.
#[test]
fn a_mastermind_overloads_above_three_captives() {
    let rules = rules();
    let mut sim = sim(19);
    let mind = spawn(&mut sim, &rules, "MIND", "YuriCountry", 10, 10);
    let mut captives = Vec::new();
    for x in 0..4 {
        let gi = spawn(&mut sim, &rules, "E1", "Americans", 12 + x, 10);
        captives.push(gi);
    }
    for &gi in &captives[..3] {
        assert!(sim.capture_unit(mind, gi, &rules));
    }
    // 30 quiet frames, then a free check at three captives.
    let health = sim.substrate.entities.get(mind).unwrap().health.current;
    let before = sim.scenario_rng.state();
    for _ in 0..31 {
        sim.capture_manager_update(mind, &rules, None);
    }
    assert_eq!(sim.scenario_rng.state(), before);
    assert_eq!(
        sim.substrate.entities.get(mind).unwrap().health.current,
        health
    );

    assert!(sim.capture_unit(mind, captives[3], &rules));
    for _ in 0..30 {
        sim.capture_manager_update(mind, &rules, None);
    }
    sim.sound_events.clear();
    let mut replay = sim.scenario_rng.clone();
    sim.capture_manager_update(mind, &rules, None);
    for _ in 0..10 {
        let _ = replay.next_range_i32_inclusive(-200, 200);
    }
    let _ = replay.next_range_i32_inclusive(0, 100);
    assert_eq!(sim.scenario_rng.state(), replay.state());
    assert_eq!(
        sim.substrate.entities.get(mind).unwrap().health.current,
        health - 50
    );
    assert_eq!(voc_sounds(&sim), ["MasterMindOverloadVoice"]);
    // The next check is 60 frames on, and the voice does not repeat.
    let quiet = sim.scenario_rng.state();
    for _ in 0..60 {
        sim.capture_manager_update(mind, &rules, None);
    }
    assert_eq!(sim.scenario_rng.state(), quiet);
    sim.sound_events.clear();
    sim.capture_manager_update(mind, &rules, None);
    assert_eq!(
        sim.substrate.entities.get(mind).unwrap().health.current,
        health - 100
    );
    assert!(voc_sounds(&sim).is_empty());
}

/// Nodes (with their original house) and the victim's link survive a save,
/// and both are hashed.
#[test]
fn mind_control_state_round_trips_and_is_hashed() {
    let rules = rules();
    let mut sim = sim(21);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    assert!(sim.capture_unit(yuri, gi, &rules));
    {
        let manager = sim
            .substrate
            .entities
            .get_mut(yuri)
            .and_then(|entity| entity.capture_manager.as_mut())
            .unwrap();
        manager.overload_countdown = 7;
        manager.overload_sound_played = true;
    }

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "mind_control", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("mind-control links resolve");
    assert_eq!(victims(&restored, yuri), [gi]);
    let americans = restored.interner.get("Americans").unwrap();
    let manager = restored
        .substrate
        .entities
        .get(yuri)
        .unwrap()
        .capture_manager
        .as_ref()
        .unwrap();
    assert_eq!(manager.original_owner(gi), Some(americans));
    assert_eq!(
        (manager.overload_countdown, manager.overload_sound_played),
        (7, true)
    );
    let entity = restored.substrate.entities.get(gi).unwrap();
    assert_eq!(entity.mind_control.controller(), Some(yuri));
    assert!(entity.mind_control.ring_anim.is_some());

    let linked = restored.state_hash();
    restored
        .substrate
        .entities
        .get_mut(gi)
        .unwrap()
        .mind_control = MindControlLink::default();
    assert_ne!(restored.state_hash(), linked, "the victim link is hashed");
    restored
        .substrate
        .entities
        .get_mut(gi)
        .unwrap()
        .mind_control = MindControlLink::controlled_by_for_test(yuri);
    let without_ring = restored.state_hash();
    restored
        .substrate
        .entities
        .get_mut(yuri)
        .unwrap()
        .capture_manager
        .as_mut()
        .unwrap()
        .pointer_expired(gi);
    assert_ne!(restored.state_hash(), without_ring, "the nodes are hashed");
}

/// A flat 32x32 clear map with its playfield, zones and path grid.
fn arena(seed: u64, rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    const SIZE: u16 = 32;
    let mut sim = sim(seed);
    sim.input_delay_ticks = 0;
    sim.session.map_width = SIZE;
    sim.session.map_height = SIZE;
    let clear = crate::rules::terrain_rules::SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: None,
        amphibious: Some(80),
        float_beach: None,
        hover: Some(50),
    };
    let cell = |x, y| {
        let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
        cell.speed_costs = clear;
        cell.base_speed_costs = clear;
        cell
    };
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            SIZE,
            SIZE,
            (0..SIZE)
                .flat_map(|y| (0..SIZE).map(move |x| cell(x, y)))
                .collect(),
        ),
    );
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 20,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    });
    sim.playfield_size_height = Some(20);
    assert!(sim.rebuild_dynamic_navigation(rules));
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .expect("navigation grid");
    (sim, grid)
}

/// Through the production fire path: a Yuri ordered to attack captures its
/// target through the Inviso delivery, with no damage, and the capture's
/// owner change drops the Yuri's own target (the detach sweep `0x0070D4A0`
/// runs for every Techno aiming at the victim).
#[test]
fn an_attack_order_captures_without_damage() {
    use crate::sim::command::{Command, CommandEnvelope};
    let rules = rules();
    let (mut sim, grid) = arena(23, &rules);
    let mind = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let health = sim.substrate.entities.get(gi).unwrap().health.current;
    let owner_id = sim.interner.intern("YuriCountry");
    sim.queue_command(CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: mind,
            target_id: gi,
        },
    ));
    for _ in 0..90 {
        let commands = sim.take_due_commands();
        sim.advance_tick(
            &commands,
            Some(&rules),
            &std::collections::BTreeMap::new(),
            Some(&grid),
            None,
            33,
        );
        if sim
            .substrate
            .entities
            .get(gi)
            .is_some_and(|entity| entity.mind_control.is_mind_controlled())
        {
            break;
        }
    }
    let captive = sim.substrate.entities.get(gi).unwrap();
    assert_eq!(captive.mind_control.controller(), Some(mind), "captured");
    assert_eq!(owner(&sim, gi), "YuriCountry");
    assert_eq!(captive.health.current, health, "no damage");
    assert!(
        sim.substrate
            .entities
            .get(mind)
            .unwrap()
            .attack_target
            .is_none()
    );
    assert_eq!(victims(&sim, mind), [gi]);
}

/// Retail `rulesmd.ini` (the local `ini/`; skipped without it): the stock
/// controllers' managers and the mind-control globals bind.
#[test]
fn retail_rules_bind_the_stock_controllers() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    let limits = |kind: &str| {
        init_capture_manager(rules.object(kind).unwrap(), &rules)
            .map(|manager| (manager.max_control, manager.infinite))
    };
    assert_eq!(limits("YURI"), Some((1, false)));
    assert_eq!(limits("YURIPR"), Some((1, false)));
    // The rookie `Primary=MindControl`, not `ElitePrimary=MindControlE`
    // (Damage=10): the manager is built before any veterancy is set.
    assert_eq!(limits("PTROOP"), Some((1, false)));
    assert_eq!(limits("MIND"), Some((3, true)));
    assert_eq!(limits("YAPSYT"), Some((3, false)));
    assert_eq!(limits("E1"), None);

    let mind_control = &rules.mind_control;
    assert_eq!(mind_control.controlled_anim.as_deref(), Some("MINDANIM"));
    assert_eq!(mind_control.overload_count, [3, 6, 10, 50]);
    assert_eq!(mind_control.overload_damage, [0, 50, 100, 500]);
    assert_eq!(mind_control.overload_frames, [30, 60, 60, 60]);
    assert_eq!(
        mind_control.ai_capture_table(CaptureReason::Normal),
        [75, 5, 5, 15]
    );
    assert_eq!(mind_control.ai_capture_low_money_mark, 2000);
    assert_eq!(mind_control.ai_capture_wounded_mark, 0.25);
    assert!(mind_control.mind_cleared_sound.is_some());
    assert!(mind_control.mind_control_sound.is_some());
    assert!(mind_control.overload_sound.is_some());
    // Miners are immune; a GI and a power plant are not.
    for (kind, immune) in [
        ("HARV", true),
        ("CMIN", true),
        ("SMIN", true),
        ("E1", false),
        ("GAPOWR", false),
    ] {
        assert_eq!(
            rules.object(kind).unwrap().immune_to_psionics,
            immune,
            "{kind}"
        );
    }
}

/// UnitClass::Deploy's CanDeploySlashUnload refuses a mind-controlled unit
/// whose DeploysInto is a Construction Yard (`0x00700EC6..0x00700ED8`) and
/// the refusal clears its pending deploy; CanUndeployMCV refuses a
/// mind-controlled Construction Yard (`0x00449C15`).
#[test]
fn a_controlled_mcv_cannot_deploy_and_a_controlled_yard_cannot_repack() {
    let rules = rules();
    let mut sim = sim(27);
    sim.session.game_options.mcv_redeploy = true;
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let mcv = spawn(&mut sim, &rules, "AMCV", "Americans", 12, 10);
    let free = spawn(&mut sim, &rules, "AMCV", "Americans", 12, 30);
    let heights = std::collections::BTreeMap::new();
    assert!(
        sim.deploy_mcv(free, &rules, &heights),
        "an MCV deploys here"
    );
    assert!(sim.capture_unit(yuri, mcv, &rules));
    sim.substrate
        .entities
        .get_mut(mcv)
        .unwrap()
        .mcv_deploy_pending = true;
    sim.sound_events.clear();
    assert!(!sim.deploy_mcv(mcv, &rules, &heights));
    assert!(
        !sim.substrate.entities.get(mcv).unwrap().mcv_deploy_pending,
        "the refusal clears the pending deploy"
    );
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::CannotDeployHere { .. })),
        "refused, not blocked"
    );
    assert_eq!(owner(&sim, mcv), "YuriCountry");

    // A human controller's stolen yard cannot repack; its own yard can.
    let prime = spawn(&mut sim, &rules, "YURI", "Americans", 20, 20);
    let stolen = spawn(&mut sim, &rules, "GACNST", "Russians", 22, 20);
    let own = spawn(&mut sim, &rules, "GACNST", "Americans", 22, 26);
    for yard in [stolen, own] {
        let now = sim.session.binary_frame;
        let _ = sim.mission_assign_exact(yard, MissionId::from_known(MissionType::Guard), now);
    }
    assert!(sim.capture_unit(prime, stolen, &rules));
    assert_eq!(owner(&sim, stolen), "Americans");
    assert!(!sim.can_undeploy_building_runtime(stolen, &rules));
    assert!(sim.can_undeploy_building_runtime(own, &rules));
}

/// Selling a Psychic Tower frees its captives first: native Selling ends
/// Is_Operational and the off edge frees them (`0x00454B47`) before the
/// tower goes, so no captive points at a removed controller and the game
/// still saves and loads.
#[test]
fn selling_a_psychic_tower_frees_its_captives() {
    let rules = rules();
    let mut sim = sim(29);
    let tower = spawn(&mut sim, &rules, "YAPSYT", "YuriCountry", 20, 20);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    assert!(sim.capture_unit(tower, gi, &rules));
    sim.sound_events.clear();

    assert!(crate::sim::production::sell_building(
        &mut sim, &rules, tower
    ));
    assert_eq!(owner(&sim, gi), "Americans");
    assert!(
        !sim.substrate
            .entities
            .get(gi)
            .unwrap()
            .mind_control
            .is_mind_controlled()
    );
    assert_eq!(voc_sounds(&sim), ["MindCleared"]);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "sold_tower", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("no captive keeps a removed controller");
}

/// CanCapture's gate 9 refuses a building still building up: VERA's
/// `building_up` is the Construction mission native runs meanwhile.
#[test]
fn a_building_mid_construction_cannot_be_captured() {
    let rules = rules();
    let mut sim = sim(31);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 14, 10);
    let now = sim.session.binary_frame;
    let _ = sim.mission_assign_exact(plant, MissionId::from_known(MissionType::Guard), now);
    sim.substrate.entities.get_mut(plant).unwrap().building_up =
        Some(crate::sim::components::BuildingUp {
            elapsed_ticks: 3,
            total_ticks: 30,
        });
    assert!(!sim.can_capture(yuri, plant, &rules));
    sim.substrate.entities.get_mut(plant).unwrap().building_up = None;
    assert!(sim.can_capture(yuri, plant, &rules));
}

/// Capture and release leave no order behind: VERA's legacy `order_intent`
/// goes with the TarCom and NavCom ChangeOwner drops.
#[test]
fn capture_and_release_drop_the_previous_order() {
    use crate::sim::components::OrderIntent;
    let rules = rules();
    let mut sim = sim(33);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    sim.substrate.entities.get_mut(gi).unwrap().order_intent = Some(OrderIntent::AttackMove {
        goal_rx: 30,
        goal_ry: 30,
    });
    assert!(sim.capture_unit(yuri, gi, &rules));
    assert!(
        sim.substrate
            .entities
            .get(gi)
            .unwrap()
            .order_intent
            .is_none()
    );

    sim.substrate.entities.get_mut(gi).unwrap().order_intent = Some(OrderIntent::Guard {
        anchor_rx: 11,
        anchor_ry: 10,
    });
    assert!(sim.free_unit(yuri, gi, &rules));
    assert_eq!(owner(&sim, gi), "Americans");
    assert!(
        sim.substrate
            .entities
            .get(gi)
            .unwrap()
            .order_intent
            .is_none()
    );
}

/// A captive entering an absorbing building is freed first (the
/// PerCellProcess sites): it goes in as its original house's unit.
#[test]
fn a_captive_boarding_an_absorber_is_freed_first() {
    use crate::sim::passenger::{BoardingPhase, PassengerRole};
    let rules = rules();
    let mut sim = sim(35);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let reactor = spawn(&mut sim, &rules, "BIOR", "YuriCountry", 14, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 13, 10);
    assert!(sim.capture_unit(yuri, gi, &rules));
    sim.sound_events.clear();
    sim.substrate.entities.get_mut(gi).unwrap().passenger_role = PassengerRole::Boarding {
        target_transport_id: reactor,
        phase: BoardingPhase::Entering,
    };

    crate::sim::passenger::tick_passenger_system(&mut sim, &rules);

    let entity = sim.substrate.entities.get(gi).unwrap();
    assert!(!entity.mind_control.is_mind_controlled());
    assert_eq!(owner(&sim, gi), "Americans");
    assert!(entity.passenger_role.is_inside_transport());
    assert!(victims(&sim, yuri).is_empty());
    assert_eq!(voc_sounds(&sim), ["MindCleared"]);
}

/// UnitClass::Receive_Radio refuses a controlled passenger (`0x007375F3`)
/// and a controller holding a captive (`0x00737616`); others load.
#[test]
fn a_unit_transport_refuses_captives_and_their_controllers() {
    let rules = rules();
    let mut sim = sim(37);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let transport = spawn(&mut sim, &rules, "TRNS", "YuriCountry", 11, 11);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let own = spawn(&mut sim, &rules, "E1", "YuriCountry", 12, 12);
    let admits = |sim: &Simulation, pax: u64| {
        let passenger = sim.substrate.entities.get(pax).unwrap();
        let carrier = sim.substrate.entities.get(transport).unwrap();
        crate::sim::passenger::can_enter_transport(
            passenger,
            carrier,
            rules
                .object(sim.interner.resolve(passenger.type_ref()))
                .unwrap(),
            rules.object("TRNS").unwrap(),
            carrier.passenger_role.cargo().unwrap(),
            &rules,
            &sim.houses,
            None,
        )
    };
    assert!(admits(&sim, own));
    assert!(admits(&sim, yuri));
    assert!(sim.capture_unit(yuri, gi, &rules));
    assert!(!admits(&sim, gi), "a captive");
    assert!(!admits(&sim, yuri), "its controller");
}

/// Fire admission runs CanCapture live (GetFireError `0x006FCB24`, with the
/// Iron Curtain gate the frame-free weapon ladder lacks): a Yuri ordered onto
/// an Iron-Curtained target drops the attack without capturing or damaging
/// it.
#[test]
fn an_iron_curtained_target_refuses_the_capture_at_fire_time() {
    use crate::sim::command::{Command, CommandEnvelope};
    let rules = rules();
    let (mut sim, grid) = arena(39, &rules);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(gi).unwrap(),
        sim.session.binary_frame,
        1000,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    let health = sim.substrate.entities.get(gi).unwrap().health.current;
    let owner_id = sim.interner.intern("YuriCountry");
    sim.queue_command(CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: yuri,
            target_id: gi,
        },
    ));
    let mut attacked = false;
    let mut dropped = false;
    for _ in 0..120 {
        let commands = sim.take_due_commands();
        sim.advance_tick(
            &commands,
            Some(&rules),
            &std::collections::BTreeMap::new(),
            Some(&grid),
            None,
            33,
        );
        let targeting = sim
            .substrate
            .entities
            .get(yuri)
            .unwrap()
            .attack_target
            .is_some();
        attacked |= targeting;
        if attacked && !targeting {
            dropped = true;
            break;
        }
    }
    assert!(dropped, "the Yuri took the order and then dropped it");
    let target = sim.substrate.entities.get(gi).unwrap();
    assert!(!target.mind_control.is_mind_controlled());
    assert_eq!(owner(&sim, gi), "Americans");
    assert_eq!(target.health.current, health);
}

/// An overload that kills the Mastermind frees its captives inside the
/// damage call (the death arm: newest first, one fate draw per computer
/// captive), then throws the five spark systems (ten draws) and takes no
/// lean draw. Rust regression: the native corpus stubs ReceiveDamage.
#[test]
fn a_killing_overload_frees_the_captives_before_the_sparks() {
    let rules = rules();
    let mut sim = sim(41);
    let mind = spawn(&mut sim, &rules, "MIND", "YuriCountry", 10, 10);
    let captives: Vec<u64> = (0..4)
        .map(|x| spawn(&mut sim, &rules, "E1", "Russians", 12 + x, 10))
        .collect();
    for &gi in &captives {
        assert!(sim.capture_unit(mind, gi, &rules));
    }
    // Row one (50 damage) at the first check, on a 50-health Mastermind.
    sim.substrate.entities.get_mut(mind).unwrap().health.current = 50;
    for _ in 0..30 {
        sim.capture_manager_update(mind, &rules, None);
    }
    let mut replay = sim.scenario_rng.clone();
    sim.capture_manager_update(mind, &rules, None);
    for _ in &captives {
        fate_draw(&mut replay);
    }
    for _ in 0..10 {
        let _ = replay.next_range_i32_inclusive(-200, 200);
    }
    assert_eq!(sim.scenario_rng.state(), replay.state());
    assert!(captives.iter().all(|&gi| owner(&sim, gi) == "Russians"));
    assert!(
        sim.substrate
            .entities
            .get(mind)
            .is_none_or(|entity| !entity.lifecycle.object_alive)
    );
}
