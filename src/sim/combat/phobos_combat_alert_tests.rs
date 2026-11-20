//! Phobos `CombatAlert.*` behavior (`Ext/Techno/Hooks.ReceiveDamage.cpp:
//! 86-140`). Every case drives damage through the production receiver
//! (`world_receiver::commit_entities`) on retail-style rules text, then reads
//! the House timer and the emitted `SimSoundEvent::CombatAlert`.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{RAD_NO_ATTACKER, world_receiver};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{CombatAlertVoice, SimSoundEvent, Simulation};

const CELL: (u16, u16) = (10, 10);
const STRENGTH: i32 = 1000;
const VERSE: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%";

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    owner: InternedId,
    enemy: InternedId,
    target: u64,
}

/// `unit`, `building` and `warhead` append extra keys to their sections;
/// `audio` is the complete `[AudioVisual]` body beside the two warheads.
/// Type sound lists bind only through the process owner's fixed catalog
/// (`NativeRulesProcessOwner::bind_type_audio_references`), so the fixture
/// supplies a SOUNDMD `[SoundList]` for the two voices the cases name.
fn make_rules(unit: &str, building: &str, warhead: &str, audio: &str) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n1=BVEH\n2=CBVEH\n\
         [MTNK]\nStrength={STRENGTH}\nArmor=none\n{unit}\
         [GAPOWR]\nStrength={STRENGTH}\nArmor=none\nFoundation=2x2\n{building}\
         [BVEH]\nStrength={STRENGTH}\nArmor=none\nFoundation=1x1\nUndeploysInto=MTNK\n\
         [CBVEH]\nStrength={STRENGTH}\nArmor=none\nFoundation=1x1\nUndeploysInto=MTNK\n\
         ConsideredVehicle=no\n\
         [Warheads]\n0=WH\n1=SUPPRESSWH\n\
         [WH]\n{VERSE}\n{warhead}\
         [SUPPRESSWH]\n{VERSE}\nCombatAlert.Suppress=yes\n\
         [AudioVisual]\n{audio}"
    ));
    RuleSet::from_ini_with_sounds_for_test(&ini, &["FeedbackVoice", "AttackVoice"])
        .expect("combat alert test rules")
}

fn fixture(unit: &str, building: &str, warhead: &str, audio: &str) -> Fixture {
    fixture_for(true, unit, building, warhead, audio)
}

fn fixture_for(
    owner_human: bool,
    unit: &str,
    building: &str,
    warhead: &str,
    audio: &str,
) -> Fixture {
    let rules = make_rules(unit, building, warhead, audio);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.session
        .set_game_mode(ScenarioGameMode::from_legacy_nonzero(true));
    // The victim's name comes from `insert_target`; a computer victim
    // (`Russians`) needs a different attacker House.
    let enemy = sim
        .interner
        .intern(if owner_human { "Russians" } else { "Cubans" });
    let target = insert_target(&mut sim, "MTNK", EntityCategory::Unit, owner_human);
    // Every id the alert resolves — the victim House, the source House's
    // alliance test and the rules type — goes through `sim.interner`, so the
    // entity carries that interner's ids rather than `test_intern`'s.
    let owner = sim
        .substrate
        .entities
        .get(target)
        .expect("inserted target")
        .owner();
    let mut owner_house = HouseState::new(owner, 0, None, owner_human, 0, 10);
    owner_house.set_player_control_for_test(owner_human);
    sim.houses.register(owner_house);
    sim.houses
        .register(HouseState::new(enemy, 0, None, false, 0, 10));
    sim.session.set_current_house_for_test(Some(owner));
    Fixture {
        sim,
        rules,
        owner,
        enemy,
        target,
    }
}

fn insert_target(
    sim: &mut Simulation,
    type_name: &str,
    category: EntityCategory,
    owner_human: bool,
) -> u64 {
    let owner_name = if owner_human { "Americans" } else { "Russians" };
    let mut target =
        GameEntity::test_default_of_category(1, type_name, owner_name, CELL.0, CELL.1, category);
    // `test_default_of_category` interns through the thread-local fixture
    // interner; re-stamp both ids so Houses, alliances and type lookups agree
    // with the simulation interner the alert reads.
    target.set_owner_for_test(sim.interner.intern(owner_name));
    target.set_type_ref_for_test(sim.interner.intern(type_name));
    target.set_category_for_test(category);
    target.health.set_for_test(STRENGTH);
    target.lifecycle.set_in_limbo_for_test(false);
    target.lifecycle.set_cell_marked_for_test(true);
    target.set_in_playfield(true);
    sim.substrate.entities.insert(target)
}

fn replace_target(f: &mut Fixture, type_name: &str, category: EntityCategory) {
    f.target = insert_target(&mut f.sim, type_name, category, true);
}

fn hit(f: &mut Fixture, damage: i32, source: Option<InternedId>, warhead: &str) {
    let warhead = f.sim.interner.intern(warhead);
    let _ = world_receiver::commit_entities(
        &mut f.sim,
        &mut world_receiver::ReceiverRun::default(),
        &[EntityDamageEvent::area(
            f.target,
            damage,
            0,
            RAD_NO_ATTACKER,
            source,
            warhead,
        )],
        None,
        &f.rules,
        None,
    );
}

fn alerts(f: &Fixture) -> Vec<(InternedId, RadarEventRequest, CombatAlertVoice)> {
    f.sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::CombatAlert {
                owner,
                radar,
                voice,
            } => Some((*owner, *radar, voice.clone())),
            _ => None,
        })
        .collect()
}

fn timer(f: &Fixture) -> crate::sim::timer::CdTimer {
    *f.sim.houses.get(&f.owner).unwrap().combat_alert_timer()
}

fn set_frame(f: &mut Fixture, frame: u32) {
    f.sim.session.set_binary_frame_for_test(frame);
}

fn voice_of(voice: &CombatAlertVoice) -> (&'static str, &str) {
    match voice {
        CombatAlertVoice::Feedback { sound_id } => ("feedback", sound_id),
        CombatAlertVoice::Attack { sound_id } => ("attack", sound_id),
        CombatAlertVoice::Eva { event } => ("eva", event),
        CombatAlertVoice::None => ("none", ""),
    }
}

/// Group 1: an admitted hit starts the House timer, raises one Combat radar
/// event at the object's cell with the configured voice, and a hit inside
/// `Interval` is throttled; the exact expiry boundary admits again.
#[test]
fn a_player_hit_raises_the_alert_once_per_interval() {
    let mut f = fixture(
        "VoiceFeedback=FeedbackVoice\nVoiceAttack=AttackVoice\n",
        "",
        "",
        "CombatAlert=yes\n",
    );
    set_frame(&mut f, 7);
    {
        let source = f.enemy;
        hit(&mut f, 50, Some(source), "WH");
    }
    let raised = alerts(&f);
    assert_eq!(raised.len(), 1);
    let (owner, radar, voice) = &raised[0];
    assert_eq!(*owner, f.owner);
    assert_eq!(radar.event_type, RadarEventType::Combat);
    assert_eq!((radar.rx, radar.ry), CELL);
    assert_eq!(voice_of(voice), ("feedback", "FeedbackVoice"));
    assert_eq!(timer(&f), crate::sim::timer::CdTimer::started(7, 150));

    // 149 frames on, `HasTimeLeft()` is still true (`:95`).
    set_frame(&mut f, 156);
    {
        let source = f.enemy;
        hit(&mut f, 50, Some(source), "WH");
    }
    assert_eq!(alerts(&f).len(), 1);
    assert_eq!(
        timer(&f),
        crate::sim::timer::CdTimer::started(7, 150),
        "a throttled hit does not restart the timer"
    );

    // The 150th frame expires it and a new hit restarts the timer.
    set_frame(&mut f, 157);
    {
        let source = f.enemy;
        hit(&mut f, 50, Some(source), "WH");
    }
    assert_eq!(alerts(&f).len(), 2);
    assert_eq!(timer(&f).start_frame(), 157);
}

/// Group 2: the master switch, the `damage > 1` boundary and a House the
/// current player does not control all silence the alert.
#[test]
fn the_master_switch_damage_threshold_and_uncontrolled_house_suppress_the_alert() {
    let mut off = fixture("VoiceFeedback=FeedbackVoice\n", "", "", "CombatAlert=no\n");
    {
        let source = off.enemy;
        hit(&mut off, 50, Some(source), "WH");
    }
    assert!(alerts(&off).is_empty(), "master switch off");
    assert_eq!(
        timer(&off),
        crate::sim::timer::CdTimer::default(),
        "no timer without an alert"
    );

    let mut boundary = fixture("VoiceFeedback=FeedbackVoice\n", "", "", "CombatAlert=yes\n");
    {
        let source = boundary.enemy;
        hit(&mut boundary, 1, Some(source), "WH");
    }
    assert!(alerts(&boundary).is_empty(), "damage 1 is not > 1");
    {
        let source = boundary.enemy;
        hit(&mut boundary, 2, Some(source), "WH");
    }
    assert_eq!(alerts(&boundary).len(), 1, "damage 2 is > 1");

    let mut computer = fixture_for(
        false,
        "VoiceFeedback=FeedbackVoice\n",
        "",
        "",
        "CombatAlert=yes\n",
    );
    {
        let source = computer.owner;
        hit(&mut computer, 50, Some(source), "WH");
    }
    assert!(
        alerts(&computer).is_empty(),
        "a computer House is not the current player"
    );
}

/// Group 3: `SuppressIfAllyDamage` with an allied source and a warhead whose
/// `CombatAlert.Suppress` is set both silence it; the same hits are admitted
/// when those two gates are off.
#[test]
fn ally_damage_and_a_suppressing_warhead_skip_the_alert() {
    let mut allied = fixture("VoiceFeedback=FeedbackVoice\n", "", "", "CombatAlert=yes\n");
    allied
        .sim
        .house_alliances_mut_for_test()
        .entry("AMERICANS".to_owned())
        .or_default()
        .insert("RUSSIANS".to_owned());
    allied
        .sim
        .house_alliances_mut_for_test()
        .entry("RUSSIANS".to_owned())
        .or_default()
        .insert("AMERICANS".to_owned());
    {
        let source = allied.enemy;
        hit(&mut allied, 50, Some(source), "WH");
    }
    assert!(alerts(&allied).is_empty(), "allied source suppressed");

    let mut allied_admitted = fixture(
        "VoiceFeedback=FeedbackVoice\n",
        "",
        "",
        "CombatAlert=yes\nCombatAlert.SuppressIfAllyDamage=no\n",
    );
    {
        let source = allied_admitted.enemy;
        hit(&mut allied_admitted, 50, Some(source), "WH");
    }
    assert_eq!(
        alerts(&allied_admitted).len(),
        1,
        "the ally gate is the configured one"
    );

    let mut warhead = fixture("VoiceFeedback=FeedbackVoice\n", "", "", "CombatAlert=yes\n");
    {
        let source = warhead.enemy;
        hit(&mut warhead, 50, Some(source), "SUPPRESSWH");
    }
    assert!(alerts(&warhead).is_empty(), "warhead suppresses");
    {
        let source = warhead.enemy;
        hit(&mut warhead, 50, Some(source), "WH");
    }
    assert_eq!(
        alerts(&warhead).len(),
        1,
        "an ordinary warhead is malicious by default"
    );
}

/// Group 4: the per-type `CombatAlert=no` overrides the admitting global
/// default, and `IgnoreBuilding`/`NotBuilding` split an ordinary defensive
/// building from a vehicle-like (1x1 `UndeploysInto=`) one.
#[test]
fn type_alert_global_default_and_building_gates() {
    let mut typed_off = fixture(
        "CombatAlert=no\nVoiceFeedback=FeedbackVoice\n",
        "",
        "",
        "CombatAlert=yes\nCombatAlert.Default=yes\n",
    );
    {
        let source = typed_off.enemy;
        hit(&mut typed_off, 50, Some(source), "WH");
    }
    assert!(
        alerts(&typed_off).is_empty(),
        "type CombatAlert=no overrides the global default"
    );

    let mut defensive = fixture("VoiceFeedback=FeedbackVoice\n", "", "", "CombatAlert=yes\n");
    replace_target(&mut defensive, "GAPOWR", EntityCategory::Structure);
    {
        let source = defensive.enemy;
        hit(&mut defensive, 50, Some(source), "WH");
    }
    assert!(
        alerts(&defensive).is_empty(),
        "IgnoreBuilding (default yes) skips a non-vehicle building"
    );

    let mut not_building = fixture(
        "VoiceFeedback=FeedbackVoice\n",
        "CombatAlert.NotBuilding=yes\n",
        "",
        "CombatAlert=yes\n",
    );
    replace_target(&mut not_building, "GAPOWR", EntityCategory::Structure);
    {
        let source = not_building.enemy;
        hit(&mut not_building, 50, Some(source), "WH");
    }
    assert_eq!(
        alerts(&not_building).len(),
        1,
        "CombatAlert.NotBuilding=yes admits the building"
    );

    let mut vehicle_like = fixture("VoiceFeedback=FeedbackVoice\n", "", "", "CombatAlert=yes\n");
    replace_target(&mut vehicle_like, "BVEH", EntityCategory::Structure);
    {
        let source = vehicle_like.enemy;
        hit(&mut vehicle_like, 50, Some(source), "WH");
    }
    assert_eq!(
        alerts(&vehicle_like).len(),
        1,
        "a 1x1 UndeploysInto building is vehicle-like"
    );
}

/// Phobos `ConsideredVehicle` (`Ext/BuildingType/Hooks.cpp:154-168`, hook
/// `0x465D40`) through the `CombatAlert.NotBuilding` fallback
/// (`Ext/Techno/Hooks.ReceiveDamage.cpp:106`): the key overrides the building
/// vt+0x80 both ways, so a 2x2 building that authors `yes` alerts like a
/// vehicle and a 1x1 `UndeploysInto=` building that authors `no` is skipped
/// like an ordinary base.
#[test]
fn considered_vehicle_flips_the_building_alert_gate() {
    let mut vehicle = fixture(
        "VoiceFeedback=FeedbackVoice\n",
        "ConsideredVehicle=yes\n",
        "",
        "CombatAlert=yes\n",
    );
    replace_target(&mut vehicle, "GAPOWR", EntityCategory::Structure);
    {
        let source = vehicle.enemy;
        hit(&mut vehicle, 50, Some(source), "WH");
    }
    assert_eq!(
        alerts(&vehicle).len(),
        1,
        "ConsideredVehicle=yes alerts as a vehicle"
    );

    let mut base = fixture("VoiceFeedback=FeedbackVoice\n", "", "", "CombatAlert=yes\n");
    replace_target(&mut base, "CBVEH", EntityCategory::Structure);
    {
        let source = base.enemy;
        hit(&mut base, 50, Some(source), "WH");
    }
    assert!(
        alerts(&base).is_empty(),
        "ConsideredVehicle=no suppresses the vehicle-like building"
    );
}

/// Group 5: feedback wins over attack wins over EVA; `MakeAVoice=no` keeps
/// only the radar event, and a `CombatAlert.EVA=none` read silences the EVA
/// arm.
#[test]
fn voice_precedence_and_make_a_voice() {
    let mut both = fixture(
        "VoiceFeedback=FeedbackVoice\nVoiceAttack=AttackVoice\n",
        "",
        "",
        "CombatAlert=yes\n",
    );
    {
        let source = both.enemy;
        hit(&mut both, 50, Some(source), "WH");
    }
    assert_eq!(voice_of(&alerts(&both)[0].2), ("feedback", "FeedbackVoice"));

    let mut attack_only = fixture("VoiceAttack=AttackVoice\n", "", "", "CombatAlert=yes\n");
    {
        let source = attack_only.enemy;
        hit(&mut attack_only, 50, Some(source), "WH");
    }
    assert_eq!(
        voice_of(&alerts(&attack_only)[0].2),
        ("attack", "AttackVoice")
    );

    let mut eva_only = fixture("", "", "", "CombatAlert=yes\n");
    {
        let source = eva_only.enemy;
        hit(&mut eva_only, 50, Some(source), "WH");
    }
    assert_eq!(
        voice_of(&alerts(&eva_only)[0].2),
        ("eva", "EVA_UnitsInCombat")
    );

    let mut silent = fixture("", "", "", "CombatAlert=yes\nCombatAlert.MakeAVoice=no\n");
    {
        let source = silent.enemy;
        hit(&mut silent, 50, Some(source), "WH");
    }
    let raised = alerts(&silent);
    assert_eq!(raised.len(), 1, "the radar ping survives MakeAVoice=no");
    assert_eq!(raised[0].1.event_type, RadarEventType::Combat);
    assert_eq!(raised[0].2, CombatAlertVoice::None);

    let mut eva_none = fixture("CombatAlert.EVA=none\n", "", "", "CombatAlert=yes\n");
    {
        let source = eva_none.enemy;
        hit(&mut eva_none, 50, Some(source), "WH");
    }
    assert_eq!(
        alerts(&eva_none)[0].2,
        CombatAlertVoice::None,
        "a set -1 EVA index plays nothing"
    );
}

/// Group 6: each unset nullable voice/EVA switch inherits its `[AudioVisual]`
/// global, and a type-level switch overrides that global.
#[test]
fn nullable_voice_and_eva_switches_fall_back_to_the_globals() {
    let mut global_eva = fixture(
        "VoiceFeedback=FeedbackVoice\nVoiceAttack=AttackVoice\n",
        "",
        "",
        "CombatAlert=yes\nCombatAlert.UseFeedbackVoice=no\nCombatAlert.UseAttackVoice=no\n",
    );
    {
        let source = global_eva.enemy;
        hit(&mut global_eva, 50, Some(source), "WH");
    }
    assert_eq!(
        voice_of(&alerts(&global_eva)[0].2),
        ("eva", "EVA_UnitsInCombat")
    );

    let mut global_silent = fixture(
        "VoiceFeedback=FeedbackVoice\nVoiceAttack=AttackVoice\n",
        "",
        "",
        "CombatAlert=yes\nCombatAlert.UseFeedbackVoice=no\nCombatAlert.UseAttackVoice=no\nCombatAlert.UseEVA=no\n",
    );
    {
        let source = global_silent.enemy;
        hit(&mut global_silent, 50, Some(source), "WH");
    }
    assert_eq!(alerts(&global_silent)[0].2, CombatAlertVoice::None);

    let mut type_override = fixture(
        "CombatAlert.UseFeedbackVoice=yes\nVoiceFeedback=FeedbackVoice\n",
        "",
        "",
        "CombatAlert=yes\nCombatAlert.UseFeedbackVoice=no\n",
    );
    {
        let source = type_override.enemy;
        hit(&mut type_override, 50, Some(source), "WH");
    }
    assert_eq!(
        voice_of(&alerts(&type_override)[0].2),
        ("feedback", "FeedbackVoice"),
        "the type key overrides the global fallback"
    );
}
