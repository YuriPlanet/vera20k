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
Primary=MultipleMindControlTower
ImmuneToPsionics=yes
[GAPOWR]
Strength=750
ImmuneToPsionics=no
[AMCV]
Strength=1000
Speed=4
DeploysInto=GACNST
[GACNST]
Strength=1000
ConstructionYard=yes
UndeploysInto=AMCV
ImmuneToPsionics=no
[BIOR]
Strength=900
Passengers=5
[TRNS]
Strength=300
Speed=4
Passengers=5
SizeLimit=2
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
[Clear]
Buildable=yes
";

const ART: &str = "\
[YAPSYT]
Foundation=2x2
[GACNST]
Foundation=4x4
[BIOR]
Foundation=2x2
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
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(text),
        &IniFile::from_str(ART),
    )
    .expect("mind control rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(ART));
    art.bind_anim_frame_count_for_test("MINDANIM", 8);
    rules.replace_art_registry_for_test(art);
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
            .register(HouseState::new(id, side, None, human, 0, 10));
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

/// Original Unit746A9C tail with an attached Anim buffer. The oracle does
/// not construct/capture that buffer; VERA uses the existing ring allocator
/// here to check the tail against a real retained Anim and its owning link.
#[test]
fn native_mirage_ring_tail_uses_current_house_or_campaign_control_flags() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .unwrap();
    let rules = rules();
    let rows = native["ring"].as_array().unwrap();
    for row in rows {
        let input = &row["input"];
        let context = row["name"].as_str().unwrap();
        let mut sim = sim(7);
        let target = spawn(&mut sim, &rules, "HTNK", "Americans", 12, 10);
        sim.attach_control_ring(target, "MINDANIM", &rules);
        let owner = sim.interner.get("Americans").unwrap();
        let viewer = if input["viewer"] == input["owner"] {
            owner
        } else {
            sim.interner.get("Russians").unwrap()
        };
        sim.session.set_current_house_for_test(Some(viewer));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(input["mode"] != 0),
        );
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_is_human_for_test(input["human"] == 1);
        house.set_player_control_for_test(input["player"] == 1);
        if input["owner_allied_with_viewer"] == true {
            let viewer_name = sim.interner.resolve(viewer).to_owned();
            sim.house_alliances_mut_for_test()
                .entry("Americans".into())
                .or_default()
                .insert(viewer_name);
        }
        let tree = sim.interner.intern("TREE01");
        let entity = sim.substrate.entities.get_mut(target).unwrap();
        let disguise = entity.disguise.as_mut().unwrap();
        if input["raw_disguised"] == 1 {
            disguise.acquire(1, Some(tree), None);
        } else {
            disguise.clear_unit();
        }
        let link = entity.mind_control;
        let ring = link.ring_anim.expect("existing owner attached the ring");
        let runtime = sim.anim(ring).unwrap().runtime().clone();
        let coord = sim.anim_absolute_coord(ring);
        let rng = sim.rng_state();
        // Both input values ensure this is a write, not an accidentally
        // correct constructor default. Native used a nonzero poison byte.
        for initial_hidden in [false, true] {
            sim.substrate.set_anim_hidden(ring, initial_hidden);
            sim.update_disguise_ring_visibility(target);
            assert_eq!(
                u8::from(sim.anim(ring).unwrap().draw_runtime().hidden),
                row["hidden_after"].as_u64().unwrap() as u8,
                "{context}"
            );
            assert_eq!(
                *sim.anim(ring).unwrap().runtime(),
                runtime,
                "{context}: no second animation clock"
            );
            assert_eq!(
                sim.anim_absolute_coord(ring),
                coord,
                "{context}: no relocation"
            );
            assert_eq!(
                sim.substrate.entities.get(target).unwrap().mind_control,
                link,
                "{context}: existing ring ownership"
            );
            assert_eq!(sim.rng_state(), rng, "{context}: no RNG draws");
        }
    }
    assert_eq!(rows.len(), 7);
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
    let health = sim.substrate.entities.get(gi).unwrap().health.current();
    let mut replay = sim.scenario_rng.clone();
    assert!(sim.capture_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    fate_draw(&mut replay);

    assert_eq!(owner(&sim, gi), "YuriCountry");
    let entity = sim.substrate.entities.get(gi).unwrap();
    assert_eq!(entity.mind_control.controller(), Some(yuri));
    assert_eq!(entity.health.current(), health);
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
    assert_eq!(sim.interner.resolve(anim.type_id()), "MINDANIM");
    assert_eq!(anim.owner_entity(), Some(gi));
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
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/capture_ring_height.json",
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
    let now = sim.session.binary_frame();
    let _ =
        sim.substrate
            .mission_assign_exact(plant, MissionId::from_known(MissionType::Guard), now);
    assert!(sim.capture_unit(
        yuri,
        plant,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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
    assert_eq!(sim.anim(ring).unwrap().z_adjust(), -1024);
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
    assert!(sim.capture_unit(
        yuri,
        first,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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

    assert!(sim.capture_unit(
        yuri,
        second,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    fate_draw(&mut replay);

    assert_eq!(owner(&sim, first), "Americans");
    let released = sim.substrate.entities.get(first).unwrap();
    assert!(!released.mind_control.is_mind_controlled());
    assert!(released.mind_control.ring_anim.is_none());
    assert!(sim.anim(ring).is_none_or(|anim| {
        !anim.in_logic_vector() && sim.substrate.pending_delete.contains_object(&ring)
    }));
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
    sim.house_alliances_mut_for_test()
        .entry("YURICOUNTRY".to_string())
        .or_default()
        .insert("RUSSIANS".to_string());
    sim.house_alliances_mut_for_test()
        .entry("RUSSIANS".to_string())
        .or_default()
        .insert("YURICOUNTRY".to_string());
    assert!(sim.can_capture(yuri, tank, &rules), "an allied tank");

    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(tank).unwrap(),
        sim.session.binary_frame(),
        100,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    assert!(!sim.can_capture(yuri, tank, &rules), "Iron Curtain");

    assert!(sim.capture_unit(
        tower,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(!sim.can_capture(yuri, gi, &rules), "already controlled");
    // A single-link Yuri with a victim still admits another.
    let other = spawn(&mut sim, &rules, "E1", "Americans", 15, 11);
    assert!(sim.capture_unit(
        yuri,
        other,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    let third = spawn(&mut sim, &rules, "E1", "Americans", 16, 11);
    assert!(sim.can_capture(yuri, third, &rules));

    // A finite manager at its limit is full.
    for x in 17..19 {
        let extra = spawn(&mut sim, &rules, "E1", "Americans", x, 11);
        assert!(sim.capture_unit(
            tower,
            extra,
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        ));
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
    let now = sim.session.binary_frame();
    let _ = sim.substrate.mission_assign_exact(
        seller,
        MissionId::from_known(MissionType::Selling),
        now,
    );
    assert!(!sim.can_capture(yuri, seller, &rules), "Selling");
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
    assert!(sim.capture_unit(
        mind,
        human,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(sim.capture_unit(
        mind,
        computer,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[hit],
        crate::sim::world::FrameEffects::default(),
    );
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
    assert!(sim.capture_unit(
        tower,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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
    assert!(sim.capture_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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

/// Original DecideUnitFate `0x004723B0` under Unicorn
/// (`tools/spatial_oracle/capture_decide_fate.py`): for every row, the
/// reason table the original read, the arm its choice took (Hunt, grinder,
/// absorber or none), whether Hunt was queued and the Scenario stream after
/// the call. The unit's own house is poor and unpowered, so reading it
/// instead of the controller's would change the reason.
#[test]
fn native_decide_unit_fate_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/capture_decide_fate.json",
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
        sim.substrate
            .entities
            .get_mut(unit)
            .unwrap()
            .health
            .set_for_test(int("health"));
        let controller_house = sim.interner.get("YuriCountry").unwrap();
        sim.houses
            .get_mut(&controller_house)
            .unwrap()
            .economy_mut()
            .set_credits_for_test(int("money"));
        let mut power_state = crate::sim::power_system::PowerState::default();
        power_state.set_total_output_for_test(int("produced"));
        power_state.set_total_drain_for_test(int("drained"));
        sim.power_states.insert(controller_house, power_state);
        let russians = sim.interner.get("Russians").unwrap();
        sim.houses
            .get_mut(&russians)
            .unwrap()
            .economy_mut()
            .set_credits_for_test(0);
        let mut power_state = crate::sim::power_system::PowerState::default();
        power_state.set_total_output_for_test(0);
        power_state.set_total_drain_for_test(100);
        sim.power_states.insert(russians, power_state);
        let seed = input["seed"].as_u64().unwrap();
        sim.scenario_rng = SimRng::new(seed);
        assert_ne!(queued_mission(&sim, unit), Some(MissionType::Hunt));

        sim.decide_unit_fate(
            yuri,
            unit,
            rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

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
        let health = crate::sim::combat::Health::new(int("health"));
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

/// DecideUnitFate's choice 3 (`0x0047261F..0x0047262E`): the computer
/// controller's new Foot seeks a Bio Reactor of its house
/// ([`Simulation::foot_enter_bio_reactor`]) and, one taking it, gets no Hunt.
/// The corpus above stubs the seek to find none.
#[test]
fn a_captive_sent_to_a_bio_reactor_does_not_hunt() {
    let mut rules = rules_from(&RULES.replace(
        "[BIOR]\nStrength=900\nPassengers=5",
        "[BIOR]\nStrength=900\nPassengers=5\nInfantryAbsorb=yes\nSizeLimit=15",
    ));
    rules.mind_control.ai_capture = std::array::from_fn(|_| vec![0, 0, 100, 0]);
    let mut sim = sim(39);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let reactor = spawn(&mut sim, &rules, "BIOR", "YuriCountry", 16, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let mut replay = sim.scenario_rng.clone();

    assert!(sim.capture_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    fate_draw(&mut replay);

    let entity = sim.substrate.entities.get(gi).unwrap();
    assert_eq!(queued_mission(&sim, gi), Some(MissionType::Enter));
    assert!(entity.building_entry.should_enter_absorber());
    assert_eq!(
        entity.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::object(reactor))
    );
    // The Infantry setter's Enter arm HELLOs the absorber (`0x0051ACD4`).
    assert_eq!(entity.radio_contacts.slot(0), Some(reactor));
    assert_eq!(sim.scenario_rng.state(), replay.state());
}

#[test]
fn ai_grinder_choice_preserves_the_seek_and_its_single_decision_draw() {
    for (kind, has_grinder) in [("E1", true), ("HTNK", true), ("E1", false)] {
        let mut rules = rules_from(&RULES.replace(
            "[BIOR]\nStrength=900\nPassengers=5",
            "[BIOR]\nStrength=900\nPassengers=5\nGrinding=yes",
        ));
        rules.mind_control.ai_capture = std::array::from_fn(|_| vec![0, 100]);
        let mut sim = sim(39);
        let controller = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
        let grinder = has_grinder.then(|| spawn(&mut sim, &rules, "BIOR", "YuriCountry", 16, 10));
        let captive = spawn(&mut sim, &rules, kind, "Americans", 12, 10);
        let mut expected_rng = sim.scenario_rng.clone();
        assert!(sim.capture_unit(controller, captive, &rules, None, FrameEffects::default()));
        fate_draw(&mut expected_rng);
        assert_eq!(sim.scenario_rng.state(), expected_rng.state());
        assert_eq!(
            queued_mission(&sim, captive),
            Some(if has_grinder {
                MissionType::Eaten
            } else {
                MissionType::Hunt
            }),
            "{kind}, grinder={has_grinder}"
        );
        assert_eq!(
            sim.entities().get(captive).unwrap().navigation.nav_com(),
            grinder.map(crate::sim::components::NavTargetRef::object)
        );
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
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/capture_overload_update.json",
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
                line_timer: Default::default(),
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
        if !infinite {
            assert_eq!(
                manager.overload_pip_state(),
                None,
                "{name}: a finite manager above capacity has no overload pip"
            );
        }
        assert_eq!(
            manager.overload_pip_countdown,
            constructed["flash"].as_i64().unwrap() as i32,
            "{name}: constructor pip countdown"
        );
        let entity = sim.substrate.entities.get_mut(mind).unwrap();
        entity.capture_manager = Some(manager);
        if input.get("kill").is_some() {
            // Dies to the first damaging row, as the fixture's owner does.
            entity.health.set_for_test(50);
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
            let health = sim.substrate.entities.get(mind).unwrap().health.current();
            let systems: Vec<u64> = sim.particle_systems().iter().map(|(&id, _)| id).collect();
            sim.sound_events.clear();

            sim.capture_manager_update(
                mind,
                rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );

            // Event checkpoints record the original manager after Update.
            // A retired Rust owner has no presentation state to compare;
            // the legacy killing callback does not establish its lifetime.
            if let (Some(native), Some(manager)) = (
                native_frames.get(&frame),
                sim.substrate
                    .entities
                    .get(mind)
                    .and_then(|entity| entity.capture_manager.as_ref()),
            ) {
                assert_eq!(
                    manager.overload_pip_countdown,
                    native["after"]["flash"].as_i64().unwrap() as i32,
                    "{name} frame {frame}: native pip countdown"
                );
            }

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
                    .map_or(0, |entity| entity.health.current());
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
                    assert_eq!(system.type_id(), spark_type);
                    assert_eq!(system.target_coords(), glam::IVec3::ZERO);
                    assert!(system.target_ref().is_none() && system.owner_entity().is_none());
                    assert!(system.owner_house().is_none());
                    let offset = system.coords() - location;
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
            .is_some_and(|entity| entity.lifecycle.object_alive());
        assert_eq!(alive, final_state["alive"] != 0, "{name}");
        if alive {
            // This legacy fixture executes only CaptureManager::Update:
            // no shared RockingUpdate integrates/damps the stored rate.
            // Its native damage/particle callbacks remain recorded stubs.
            let rate = sim
                .substrate
                .entities
                .get(mind)
                .unwrap()
                .rocking
                .map_or(0, |state| state.native_bits()[2]);
            assert_eq!(
                rate,
                (final_state["lean"].as_f64().unwrap() as f32).to_bits(),
                "{name}: final native sideways rate"
            );
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
            assert_eq!(
                manager.overload_pip_countdown,
                final_state["flash"].as_i64().unwrap() as i32,
                "{name}: final native pip countdown"
            );
        }
        assert_eq!(
            sim.scenario_rng.next_u32(),
            case["next_random"].as_u64().unwrap() as u32,
            "{name}"
        );
    }
}

/// Full native471A50 with physical stock MIND/Spark rules, actual nonlethal
/// Unit damage and five ParticleSystem constructors. Captive count alone is
/// an explicit supplied component prior on both sides. The additive Techno
/// bracket executes Mission/targeting before overload; other rows visit only
/// the manager. No real capture, fatal FreeAll or particle AI is claimed here.
/// The ordinary world witness owns legal shots/real nodes/next-frame lurch.
#[test]
fn native_joined_overload_preserves_constructor_rng_ids_and_rate_words() {
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use serde_json::{Value, json};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/capture_overload_joined.json",
    ))
    .unwrap();
    let techno_order: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/capture_overload_techno_order.json",
    ))
    .unwrap();
    let initial = &corpus["initial"];
    // Independent physical map copies can have different payload identities;
    // their selected type, overload and Spark reader outputs must agree.
    for key in ["type", "rules", "spark_types", "fixed_sources"] {
        assert_eq!(initial[key], techno_order["joined_inputs"][key]);
    }
    let object = rules.object("MIND").unwrap();
    let weapon = rules.weapon(object.primary().unwrap()).unwrap();
    assert_eq!(json!(object.strength), initial["type"]["strength"]);
    assert_eq!(json!(object.primary().unwrap()), initial["type"]["primary"]);
    assert_eq!(json!(weapon.damage), initial["type"]["primary_damage"]);
    assert_eq!(
        format!("{:016x}", object.weight.to_num::<f64>().to_bits()),
        initial["type"]["weight_bits"].as_str().unwrap()
    );
    assert_eq!(
        json!(rules.mind_control.overload_count),
        initial["rules"]["count"]
    );
    assert_eq!(
        json!(rules.mind_control.overload_damage),
        initial["rules"]["damage"]
    );
    assert_eq!(
        json!(rules.mind_control.overload_frames),
        initial["rules"]["frames"]
    );
    assert_eq!(
        json!(rules.bridge_warheads.c4_name),
        initial["rules"]["c4_warhead"]
    );
    assert_eq!(
        json!(rules.combat_damage.default_spark_system),
        initial["rules"]["default_spark"]
    );

    let rng_state = |world: &Simulation| {
        json!({
            "main": world.main_rng.native_state_hex(),
            "scenario": world.scenario_rng.native_state_hex(),
            "mapgen": world.mapgen_rng.native_state_hex(),
        })
    };
    let compare_actor = |world: &Simulation, id: u64, expected: &Value| {
        let actor = world.substrate.entities.get(id).unwrap();
        let manager = actor.capture_manager.as_ref().unwrap();
        let point = crate::sim::movement::ground_pose::object_location(
            actor,
            world.resolved_terrain.as_ref(),
        );
        assert_eq!(json!(actor.health.current()), expected["health"]);
        assert_eq!(
            json!(u8::from(actor.lifecycle.object_alive())),
            expected["alive"]
        );
        assert_eq!(
            json!(u8::from(actor.lifecycle.in_limbo())),
            expected["limbo"]
        );
        assert_eq!(json!([point.x, point.y, point.z]), expected["location"]);
        assert_eq!(
            json!(actor.rocking.map_or([0; 4], |state| state.native_bits())),
            expected["rocking_bits"]
        );
        assert_eq!(json!(manager.count()), expected["count"]);
        assert_eq!(json!(manager.overload_countdown), expected["countdown"]);
        assert_eq!(
            json!(u8::from(manager.overload_sound_played)),
            expected["latch"]
        );
        assert_eq!(json!(manager.overload_pip_countdown), expected["flash"]);
    };
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    let mut signs = std::collections::BTreeMap::<u64, std::collections::BTreeSet<u32>>::new();
    for row in rows.iter().chain(std::iter::once(&techno_order["joined"])) {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let count = input["captives"].as_u64().unwrap();
        let through_techno = input["techno_order"].as_bool() == Some(true);
        let mut world = crate::sim::arena_fixture::rival_arena_with_size(&rules, 1, 64);
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        let mind = world
            .spawn_object("MIND", "Americans", 8, 8, 128, &rules)
            .unwrap();
        let manager = world
            .substrate
            .entities
            .get_mut(mind)
            .unwrap()
            .capture_manager
            .as_mut()
            .unwrap();
        assert_eq!(json!(manager.max_control), row["context"]["manager_max"]);
        assert_eq!(
            json!(u8::from(manager.infinite)),
            row["context"]["manager_infinite"]
        );
        manager.nodes = (0..count)
            .map(|index| ControlNode {
                victim: 1_000_000 + index,
                original_owner: InternedId::default(),
                line_timer: Default::default(),
            })
            .collect();
        if through_techno {
            manager.overload_countdown = input["countdown"].as_i64().unwrap() as i32;
            world
                .session
                .set_binary_frame_for_test(input["frame"].as_u64().unwrap() as u32);
            let actor = world.substrate.entities.get(mind).unwrap();
            assert_eq!(
                json!(actor.mission.current().raw()),
                row["context"]["mission"]
            );
            assert_eq!(
                actor.passive_scan.timer(),
                crate::sim::mission::MissionTimer::armed(
                    row["context"]["passive_timer"][0].as_u64().unwrap() as u32,
                    row["context"]["passive_timer"][2].as_u64().unwrap() as u32,
                )
            );
        }
        world.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            row["native_cursor_before"].as_u64().unwrap() as u32,
        ));
        world.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        world.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        world.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        compare_actor(&world, mind, &row["before"]);
        assert_eq!(rng_state(&world), row["rng_before"]);
        let previous: std::collections::BTreeSet<_> =
            world.particle_systems().iter().map(|(&id, _)| id).collect();
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            if through_techno {
                world.object_ai_visit_one(
                    mind,
                    Some(&rules),
                    crate::sim::world::ObjectAiCtx::default(),
                );
            } else {
                for frame in 0..input["visits"].as_u64().unwrap() as u32 {
                    world.session.set_binary_frame_for_test(frame);
                    world.capture_manager_update(mind, &rules, None, FrameEffects::default());
                }
            }
        });
        let words: Vec<_> = draws.iter().map(|draw| draw["value"].clone()).collect();
        let expected_words: Vec<_> = row["rng_requests"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|request| request["raw_draws"].as_array().unwrap().iter().cloned())
            .collect();
        assert_eq!(
            words, expected_words,
            "{name}: every raw word in order, including rejection words"
        );
        assert_eq!(json!(draws.len()), row["raw_draw_count"]);
        assert_eq!(
            rng_state(&world),
            row["rng_after"],
            "{name}: complete three streams"
        );
        assert_eq!(
            json!(world.native_unique_ids.as_ref().unwrap().current_raw()),
            row["native_cursor_after"]
        );
        compare_actor(&world, mind, &row["after"]);
        if count > 3 {
            signs
                .entry(count)
                .or_default()
                .insert(row["after"]["rocking_bits"][2].as_u64().unwrap() as u32 >> 31);
        }
        let births: Vec<_> = world
            .substrate
            .logic
            .as_slice()
            .iter()
            .copied()
            .filter(|id| !previous.contains(id) && world.particle_systems().contains_key(*id))
            .collect();
        let expected = row["particle_systems"].as_array().unwrap();
        assert_eq!(
            births.len(),
            expected.len(),
            "{name}: actual ordered Spark births"
        );
        for (id, expected) in births.into_iter().zip(expected) {
            let system = world.particle_systems().get(id).unwrap();
            let state = &expected["state"];
            assert_eq!(
                json!(rules.particle_system_type(system.type_id()).name),
                expected["type"]
            );
            assert_eq!(json!(system.native_unique_id()), state["native_id"]);
            assert_eq!(json!(system.coords().to_array()), state["position"]);
            assert_eq!(json!(system.coords().to_array()), expected["position"]);
            assert_eq!(json!(system.target_coords().to_array()), state["target"]);
            assert_eq!(
                json!(system.target_coords().to_array()),
                expected["args"]["target"]
            );
            assert_eq!(json!(system.lifetime()), state["lifetime"]);
            assert_eq!(
                json!(system.spark_spawn_frames()),
                state["spark_spawn_frames"]
            );
            assert_eq!(json!(system.done_spawning()), state["time_to_die"]);
            assert_eq!(json!(system.in_logic_vector()), state["logic"]);
            assert_eq!(json!(system.particles().len()), state["particle_count"]);
            assert!(system.alive() && !system.in_limbo() && system.particles().is_empty());
            assert!(
                system.owner_entity().is_none()
                    && system.target_ref().is_none()
                    && system.owner_house().is_none()
            );
            for key in ["owner", "target_object", "house"] {
                assert_eq!(state[key], json!(0));
                assert_eq!(expected["args"][key], json!(0));
            }
            assert_eq!(
                expected["rng_request_before"], expected["rng_request_after"],
                "selected stock SparkSys ctor has no RNG draws at this seam"
            );
            assert_eq!(
                expected["native_cursor_after"].as_u64().unwrap(),
                expected["native_cursor_before"].as_u64().unwrap() + 1
            );
        }
    }
    assert_eq!(
        signs.get(&4).unwrap(),
        &std::collections::BTreeSet::from([0, 1])
    );
    assert_eq!(
        signs.get(&7).unwrap(),
        &std::collections::BTreeSet::from([0, 1])
    );
}

/// Declared retained-field controls for the original Infinite gate and the
/// positive-only decrement before both quiet and non-damaging returns.
#[test]
fn overload_pip_counter_obeys_finite_and_non_damaging_update_gates() {
    let rules = rules();
    for (kind, counter, countdown, expected_counter, expected_countdown) in [
        ("YURI", 3, 7, 3, 7),
        ("MIND", 3, 7, 2, 6),
        ("MIND", 3, 0, 2, 30),
        ("MIND", 0, 7, 0, 6),
        ("MIND", -1, 7, -1, 6),
    ] {
        let mut sim = sim(20);
        let controller = spawn(&mut sim, &rules, kind, "YuriCountry", 10, 10);
        let manager = sim
            .substrate
            .entities
            .get_mut(controller)
            .unwrap()
            .capture_manager
            .as_mut()
            .unwrap();
        manager.overload_pip_countdown = counter;
        manager.overload_countdown = countdown;
        let before = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        sim.capture_manager_update(
            controller,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let manager = sim
            .substrate
            .entities
            .get(controller)
            .unwrap()
            .capture_manager
            .as_ref()
            .unwrap();
        assert_eq!(
            manager.overload_pip_countdown, expected_counter,
            "{kind} {counter}/{countdown}"
        );
        assert_eq!(
            manager.overload_countdown, expected_countdown,
            "{kind} {counter}/{countdown}"
        );
        assert_eq!(manager.overload_pip_state(), None, "no captive overload");
        assert_eq!(
            (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex(),
            ),
            before
        );
    }
}

/// Nodes (with their original house), the pip countdown and the victim's
/// link survive a save, and each retained state participates in the hash.
#[test]
fn mind_control_state_round_trips_and_is_hashed() {
    let rules = rules();
    let mut sim = sim(21);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    assert!(sim.capture_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    {
        let manager = sim
            .substrate
            .entities
            .get_mut(yuri)
            .and_then(|entity| entity.capture_manager.as_mut())
            .unwrap();
        manager.overload_countdown = 7;
        manager.overload_sound_played = true;
        // Declared nonzero persistence input; the real pulse above owns
        // natural initialization and expiry of this field.
        manager.overload_pip_countdown = 5;
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
        (
            manager.overload_countdown,
            manager.overload_sound_played,
            manager.overload_pip_countdown,
        ),
        (7, true, 5)
    );
    let entity = restored.substrate.entities.get(gi).unwrap();
    assert_eq!(entity.mind_control.controller(), Some(yuri));
    assert!(entity.mind_control.ring_anim.is_some());

    let linked = restored.state_hash();
    restored
        .substrate
        .entities
        .get_mut(yuri)
        .unwrap()
        .capture_manager
        .as_mut()
        .unwrap()
        .overload_pip_countdown = 4;
    assert_ne!(restored.state_hash(), linked, "the pip countdown is hashed");
    restored
        .substrate
        .entities
        .get_mut(yuri)
        .unwrap()
        .capture_manager
        .as_mut()
        .unwrap()
        .overload_pip_countdown = 5;
    assert_eq!(restored.state_hash(), linked);
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
    let mut sim = sim(seed);
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
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
    let health = sim.substrate.entities.get(gi).unwrap().health.current();
    let owner_id = sim.interner.intern("YuriCountry");
    sim.queue_command(CommandEnvelope::new(
        owner_id,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: mind,
            target_id: gi,
        },
    ));
    for _ in 0..90 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
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
    assert_eq!(captive.health.current(), health, "no damage");
    assert!(
        sim.substrate
            .entities
            .get(mind)
            .unwrap()
            .attack_target
            .target()
            .is_none()
    );
    assert_eq!(victims(&sim, mind), [gi]);
}

/// Retail `rulesmd.ini` (the local `ini/`; skipped without it): the stock
/// controllers' managers and the mind-control globals bind.
#[test]
fn retail_rules_bind_the_stock_controllers() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
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
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.session.game_options_mut_for_test().mcv_redeploy = true;
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let mcv = spawn(&mut sim, &rules, "AMCV", "Americans", 12, 10);
    let free = spawn(&mut sim, &rules, "AMCV", "Americans", 12, 30);
    assert!(
        sim.deploy_mcv(
            free,
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        ),
        "an MCV deploys here"
    );
    assert!(sim.capture_unit(
        yuri,
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    sim.substrate
        .entities
        .get_mut(mcv)
        .unwrap()
        .mcv_deploy_pending
        .set_for_test(true);
    sim.sound_events.clear();
    assert!(!sim.deploy_mcv(
        mcv,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(
        !sim.substrate
            .entities
            .get(mcv)
            .unwrap()
            .mcv_deploy_pending
            .is_pending(),
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
        let now = sim.session.binary_frame();
        let _ = sim.substrate.mission_assign_exact(
            yard,
            MissionId::from_known(MissionType::Guard),
            now,
        );
    }
    assert!(sim.capture_unit(
        prime,
        stolen,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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
    let mut rules = rules();
    rules.set_buildup_control_for_test("YAPSYT", [0, 25, 2]);
    let mut sim = sim(29);
    let tower = spawn(&mut sim, &rules, "YAPSYT", "YuriCountry", 20, 20);
    // A frame's operational visit, which the off edge compares against.
    sim.visit_building_operational(
        tower,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(
        sim.substrate
            .entities
            .get(tower)
            .unwrap()
            .building_last_operational
            .is_operational()
    );
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    assert!(sim.capture_unit(
        tower,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    sim.sound_events.clear();

    assert!(crate::sim::production::sell_building_now_for_test(
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
    let now = sim.session.binary_frame();
    let _ =
        sim.substrate
            .mission_assign_exact(plant, MissionId::from_known(MissionType::Guard), now);
    sim.substrate
        .entities
        .get_mut(plant)
        .unwrap()
        .install_building_up(
            crate::sim::components::BuildingUp::completing_in_ticks(27, 0),
            0,
        );
    assert!(!sim.can_capture(yuri, plant, &rules));
    sim.substrate
        .entities
        .get_mut(plant)
        .unwrap()
        .finish_building_construction_for_test();
    assert!(sim.can_capture(yuri, plant, &rules));
}

/// ChangeOwner leaves an attack-move alone: only the MegaMission bodies,
/// the Foot constructor and PointerExpired write `Foot+0x5C4..+0x5D1`. Its
/// Enter_Idle_Mode asks RefreshMegaMission, which resumes the order with
/// Guard committed on the spot, but CaptureUnit then resets the captive's
/// orders to Guard (`vt+0x3D0` at `0x00471EA5`): it stands still with its
/// MegaMission kept. FreeUnit has no such reset, so the released object goes
/// on to the MegaDestination for its old owner.
#[test]
fn a_released_captive_resumes_its_attack_move() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::MissionType;
    let rules = rules();
    let mut sim = sim(33);
    // A human controller keeps its captive's orders (no DecideUnitFate roll).
    let yuri_house = sim.interner.intern("YuriCountry");
    sim.houses
        .get_mut(&yuri_house)
        .unwrap()
        .set_is_human_for_test(true);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let captive = sim.substrate.entities.get_mut(gi).unwrap();
    captive.mega_mission = crate::sim::mission::mega_mission::MegaMission::attack_move(Some(
        crate::sim::combat::TargetKind::Cell(30, 30),
    ));
    // Its FootClass::AI has cleared Foot+6B3 since the Unlimbo (`0x004DA54E`).
    captive.mission_leaf.set_foot_idle_entry_latch(0);
    assert!(sim.capture_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    let captive = sim.substrate.entities.get(gi).unwrap();
    assert!(captive.mega_mission.is_attack_move());
    assert_eq!(captive.navigation.nav_com(), None);
    assert_eq!(
        captive.mission.effective().known(),
        Some(MissionType::Guard)
    );

    // Frames pass before the release; FootClass::AI clears Foot+6B3 again.
    sim.substrate
        .entities
        .get_mut(gi)
        .unwrap()
        .mission_leaf
        .set_foot_idle_entry_latch(0);
    assert!(sim.free_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert_eq!(owner(&sim, gi), "Americans");
    let freed = sim.substrate.entities.get(gi).unwrap();
    assert!(freed.mega_mission.is_attack_move());
    assert_eq!(freed.navigation.nav_com(), Some(NavTargetRef::cell(30, 30)));
    assert_eq!(freed.mission.effective().known(), Some(MissionType::Move));
}

/// A captive entering an absorbing building is freed first (the
/// PerCellProcess sites): it goes in as its original house's unit.
#[test]
fn a_captive_boarding_an_absorber_is_freed_first() {
    let mut rules = rules_from(&RULES.replace(
        "[BIOR]\nStrength=900\nPassengers=5",
        "[BIOR]\nStrength=900\nPassengers=5\nInfantryAbsorb=yes\nSizeLimit=15",
    ));
    // DecideUnitFate's Bio Reactor choice queues the captive's Enter.
    rules.mind_control.ai_capture = std::array::from_fn(|_| vec![0, 0, 100, 0]);
    let mut sim = sim(35);
    let yuri = spawn(&mut sim, &rules, "YURI", "YuriCountry", 10, 10);
    let reactor = spawn(&mut sim, &rules, "BIOR", "YuriCountry", 14, 10);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 13, 10);
    assert!(sim.capture_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(gi)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(reactor)
    );
    sim.sound_events.clear();
    // PerCellProcess boards from the absorber's own foundation cell once
    // the queued Enter is current.
    let now = sim.session.binary_frame();
    let gi_entity = sim.substrate.entities.get_mut(gi).unwrap();
    assert!(crate::sim::mission::authority::commence_entity_mission(
        gi_entity, now
    ));
    gi_entity.position.set_rx_for_test(14);
    sim.per_cell_process(
        gi,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

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
        crate::sim::passenger::can_enter_transport(sim, &rules, pax, transport)
    };
    assert!(admits(&sim, own));
    assert!(admits(&sim, yuri));
    assert!(sim.capture_unit(
        yuri,
        gi,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
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
        sim.session.binary_frame(),
        1000,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    let health = sim.substrate.entities.get(gi).unwrap().health.current();
    let owner_id = sim.interner.intern("YuriCountry");
    sim.queue_command(CommandEnvelope::new(
        owner_id,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: yuri,
            target_id: gi,
        },
    ));
    let mut attacked = false;
    let mut dropped = false;
    for _ in 0..120 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        let targeting = sim
            .substrate
            .entities
            .get(yuri)
            .unwrap()
            .attack_target
            .target()
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
    assert_eq!(target.health.current(), health);
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
        assert!(sim.capture_unit(
            mind,
            gi,
            &rules,
            None,
            crate::sim::world::FrameEffects::default()
        ));
    }
    // Row one (50 damage) at the first check, on a 50-health Mastermind.
    sim.substrate
        .entities
        .get_mut(mind)
        .unwrap()
        .health
        .set_for_test(50);
    for _ in 0..30 {
        sim.capture_manager_update(
            mind,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
    }
    let mut replay = sim.scenario_rng.clone();
    sim.capture_manager_update(
        mind,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
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
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    );
}

/// Techno6FA6F5 checks a carried bomb before CaptureManager471A50. A
/// naturally due fatal Ivan fuse must free the captives before the manager
/// can emit another overload pulse. Stock rules, four actual capture shots and all
/// intervening ordinary object/particle turns use their production owners.
#[test]
fn a_due_ivan_bomb_precedes_the_same_turn_mastermind_overload() {
    use crate::sim::arena_fixture::mastermind::{MindScene, spark_ids};

    let Some(mut scene) = MindScene::retail() else {
        return;
    };
    scene.capture_four_through_real_attacks();
    let rules = scene.rules().clone();
    let mind = scene.mind();
    // The native timer uses Frame > end (Bomb438A70), while the original
    // manager's countdown returns after decrementing a positive value.
    // Pick an attach frame that makes these existing clocks expire together;
    // do not replace either retained clock or supply a damage/ownership result.
    let fuse_visits = rules.combat_damage.ivan_timed_delay + 1;
    let cycle = rules.mind_control.overload_frames[1] + 1;
    let attach_countdown = fuse_visits % cycle;
    for _ in 0..=cycle {
        let controller = scene.simulation().entities().get(mind).unwrap();
        if controller
            .capture_manager
            .as_ref()
            .unwrap()
            .overload_countdown
            == attach_countdown
        {
            break;
        }
        scene.step();
    }
    let controller = scene.simulation().entities().get(mind).unwrap();
    assert_eq!(
        controller
            .capture_manager
            .as_ref()
            .unwrap()
            .overload_countdown,
        attach_countdown
    );
    let ivan = scene
        .simulation_mut()
        .spawn_object("IVAN", "Russians", 48, 48, 0, &rules)
        .expect("retail Ivan supplied at the existing Attach entry");
    scene.simulation_mut().bomb_attach(ivan, Some(mind), &rules);
    assert!(
        scene
            .simulation()
            .entities()
            .get(mind)
            .unwrap()
            .bomb
            .is_some()
    );

    for _ in 0..=fuse_visits {
        let sim = scene.simulation();
        let controller = sim.entities().get(mind).unwrap();
        assert!(controller.lifecycle.object_alive());
        let expired = controller
            .bomb
            .unwrap()
            .expired(sim.session.binary_frame() as i32);
        if expired {
            assert_eq!(
                controller
                    .capture_manager
                    .as_ref()
                    .unwrap()
                    .overload_countdown,
                0
            );
            assert!(controller.health.current() <= rules.combat_damage.ivan_damage);
            let before = spark_ids(&scene);
            scene.step();
            assert!(
                scene
                    .simulation()
                    .entities()
                    .get(mind)
                    .is_none_or(|actor| !actor.lifecycle.object_alive()),
                "the real Ivan blast must kill the controller"
            );
            let original_owner = scene.simulation().interner.get("Russians").unwrap();
            for victim in scene.targets() {
                let actor = scene.simulation().entities().get(victim).unwrap();
                assert_eq!(
                    actor.owner(),
                    original_owner,
                    "death must restore each captive"
                );
                assert!(!actor.mind_control.is_mind_controlled());
            }
            assert!(!scene.simulation().bomb_carriers().contains(&mind));
            let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
                "tools/spatial_oracle/capture_overload_techno_order.json",
            ))
            .unwrap();
            let native = native["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == "killing_bomb_frees_captures_before_visit")
                .unwrap();
            let expected_sparks = native["manager_events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event.get("spark").is_some())
                .count();
            assert_eq!(
                spark_ids(&scene).difference(&before).count(),
                expected_sparks,
                "the fatal fuse must run before a due overload can create five SparkSys objects"
            );
            return;
        }
        scene.step();
    }
    panic!("the ordinary retained Ivan fuse never expired");
}

#[test]
fn control_link_timer_selection_ports_cleanup_and_snapshot_share_the_manager() {
    use crate::rules::art_data::ArtRegistry;
    use crate::sim::world::{FrameEffects, UninitContext};
    let input = IniFile::from_str(
        &RULES
            .replace(
                "[CombatDamage]",
                "[CombatDamage]\nMindControlAttackLineFrames=20",
            )
            .replace("[E1]", "[E1]\nLeptonMindControlOffset=90"),
    );
    let art = IniFile::from_str(
        "[MIND]\nPrimaryFireFLH=0,0,5\nAlternateFLH0=0,0,20\nAlternateFLH1=0,0,40\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&input, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let mut sim = sim(31);
    let house = sim.interner.get("YuriCountry").unwrap();
    sim.houses
        .get_mut(&house)
        .unwrap()
        .set_is_human_for_test(true);
    sim.session.set_binary_frame_for_test(100);
    let controller = spawn(&mut sim, &rules, "MIND", "YuriCountry", 10, 10);
    let first = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let second = spawn(&mut sim, &rules, "HTNK", "Russians", 14, 10);
    assert!(sim.capture_unit(controller, first, &rules, None, FrameEffects::default()));
    assert!(sim.capture_unit(controller, second, &rules, None, FrameEffects::default()));
    let rng = sim.rng_state();
    let lines = sim.capture_link_coordinates(controller, &rules);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        (
            lines[0].from.z,
            lines[0].to.z,
            lines[1].from.z,
            lines[1].to.z
        ),
        (40, 70, 20, 90)
    );
    assert!(lines.iter().all(|line| line.owner == house));
    assert_eq!(
        sim.rng_state(),
        rng,
        "read-only coordinates do not draw RNG"
    );
    sim.session.set_binary_frame_for_test(119);
    assert_eq!(sim.capture_link_coordinates(controller, &rules).len(), 2);
    let bytes = crate::sim::snapshot::GameSnapshot::save_validated(&sim, 0, 0, "control links", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(
        restored.capture_link_coordinates(controller, &rules),
        sim.capture_link_coordinates(controller, &rules)
    );
    sim.session.set_binary_frame_for_test(120);
    assert!(sim.capture_link_coordinates(controller, &rules).is_empty());
    sim.substrate
        .entities
        .get_mut(first)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    let selected = sim.capture_link_coordinates(controller, &rules);
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].from.z, 20,
        "filtering must retain the original node port"
    );
    sim.substrate
        .entities
        .get_mut(controller)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    assert_eq!(sim.capture_link_coordinates(controller, &rules).len(), 2);
    sim.free_unit(controller, first, &rules, None, FrameEffects::default());
    assert_eq!(sim.capture_link_coordinates(controller, &rules).len(), 1);
    sim.uninit_with_context(second, UninitContext::new(Some(&rules), None));
    assert!(sim.capture_link_coordinates(controller, &rules).is_empty());
}
