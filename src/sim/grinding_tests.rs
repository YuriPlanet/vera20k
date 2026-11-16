//! Regressions for the Phobos `Grinding.*` chain: the type gates on the seek
//! and CAN_LOAD admission, the die-sound switch, and `DoGrindingExtras`
//! (accumulated refund, self-fired weapon, replacement grinder sound).

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::FrameEffects;

fn fixture(grinder_lines: &str) -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nRefundPercent=50%\n\
         [Countries]\n0=Russians\n1=Americans\n\
         [InfantryTypes]\n0=CIV\n1=ENGINEER\n[VehicleTypes]\n0=TANK\n\
         [BuildingTypes]\n0=GRINDER\n1=OTHER\n\
         [Warheads]\n0=GWWH\n\
         [Weapons]\n0=GWPN\n\
         [CIV]\nImage=GI\nStrength=100\nCost=100\nSoylent=75\nSpeed=4\n\
         VoiceDie=VoiceA\nDieSound=DieA\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         [ENGINEER]\nImage=GI\nStrength=75\nEngineer=yes\nSpeed=4\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         [TANK]\nStrength=300\nCost=600\nSoylent=300\nSpeed=6\nSpeedType=Track\nPrimary=GWPN\n\
         Locomotor={{4A582741-9839-11D1-B709-00A024DDAFD1}}\n\
         [GRINDER]\nStrength=1000\nGrinding=yes\nDamageSelf=yes\nRepairable=yes\n{grinder_lines}\
         [OTHER]\nStrength=1000\nGrinding.Weapon=GWPN\n\
         Grinding.Weapon.RequiredCredits=100\nGrinding.Sound=OwnGrind\n\
         Grinding.PlayDieSound=no\n\
         [GWWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [GWPN]\nDamage=25\nROF=0\nRange=1\nWarhead=GWWH\n\
         [Eaten]\nRate=.016\n\
         [AudioVisual]\nEnterGrinderSound=GrinderGrinding\n"
    ));
    let art = IniFile::from_str(&format!(
        "{}\n[GRINDER]\nFoundation=1x1\nActiveAnim=ACTIVE\nSpecialAnim=GRIND\n\
         [OTHER]\nFoundation=1x1\n\
         [ACTIVE]\nLoopCount=-1\nRate=1\n[GRIND]\nLoopCount=1\nRate=1\n",
        crate::rules::retail_ini_fixture::GI_ART_EXCERPT,
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut art_registry = ArtRegistry::from_ini(&art);
    art_registry.bind_anim_frame_count_for_test("ACTIVE", 8);
    art_registry.bind_anim_frame_count_for_test("GRIND", 8);
    rules.install_art_data(art_registry);
    rules.bind_type_sound_references(
        &ini,
        &SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=GrinderGrinding\n1=OwnGrind\n2=VoiceA\n3=DieA\n",
        )),
    );
    let sim = crate::sim::arena_fixture::rival_arena(&rules, 60);
    (sim, rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, x: u16, y: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, x, y, 0, 0, rules)
        .unwrap()
}

fn mission(sim: &mut Simulation, id: u64) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Eaten),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
}

/// Put `foot` on a cell occupied by the grinder, with the grinder as its
/// TarCom target.
fn stand_on_grinder(sim: &mut Simulation, grinder: u64, foot: u64, rx: u16, ry: u16) {
    sim.substrate.occupancy.add(
        rx,
        ry,
        grinder,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::AppendBuilding,
    );
    mission(sim, foot);
    sim.substrate
        .entities
        .get_mut(foot)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(grinder)));
}

#[test]
fn grinding_type_gates_drive_the_seek_and_the_radio_answer() {
    use crate::sim::radio::{RadioResponse, receive::can_load_query};
    // AllowOwner=no refuses the same-house grinder to both the Script60 seek
    // (0x4DFABD) and CAN_LOAD (0x43C30A).
    let (mut sim, rules) = fixture("Grinding.AllowOwner=no\n");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    assert!(!rules.object("GRINDER").unwrap().phobos_grinding.allow_owner);
    assert!(!sim.can_grind_techno(grinder, civ, &rules));
    assert!(!sim.foot_enter_grinder(civ, &rules, None, FrameEffects::default()));
    assert_eq!(
        can_load_query(&sim, grinder, civ, &rules),
        RadioResponse::Negatory
    );

    // The type allow list only admits the types it names, and the disallow
    // list removes them again.
    let (mut sim, rules) = fixture("Grinding.AllowTypes=TANK\n");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Russians", 10, 12);
    assert!(sim.can_grind_techno(grinder, tank, &rules));
    assert!(!sim.can_grind_techno(grinder, civ, &rules));
    assert_eq!(
        can_load_query(&sim, grinder, civ, &rules),
        RadioResponse::Negatory
    );
    assert_eq!(
        can_load_query(&sim, grinder, tank, &rules),
        RadioResponse::Roger
    );

    let (mut sim, rules) = fixture("Grinding.DisallowTypes=CIV\n");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Russians", 10, 12);
    assert!(!sim.can_grind_techno(grinder, civ, &rules));
    assert!(sim.can_grind_techno(grinder, tank, &rules));

    // An allied grinder needs `Grinding.AllowAllies=yes`.
    let (mut sim, rules) = fixture("");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Americans", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    let alliances = sim.house_alliances_mut_for_test();
    alliances
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    alliances
        .entry("AMERICANS".into())
        .or_default()
        .insert("RUSSIANS".into());
    assert!(!sim.can_grind_techno(grinder, civ, &rules));
    assert_eq!(
        can_load_query(&sim, grinder, civ, &rules),
        RadioResponse::Negatory
    );
    let (mut sim, rules) = fixture("Grinding.AllowAllies=yes\n");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Americans", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    let alliances = sim.house_alliances_mut_for_test();
    alliances
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    alliances
        .entry("AMERICANS".into())
        .or_default()
        .insert("RUSSIANS".into());
    assert!(sim.can_grind_techno(grinder, civ, &rules));
    assert_eq!(
        can_load_query(&sim, grinder, civ, &rules),
        RadioResponse::Roger
    );
}

#[test]
fn grinding_extras_accumulate_the_refund_and_fire_the_type_weapon() {
    let (mut sim, rules) = fixture(
        "Grinding.PlayDieSound=no\nGrinding.Sound=OwnGrind\n\
         Grinding.Weapon=GWPN\nGrinding.Weapon.RequiredCredits=100\n",
    );
    assert_eq!(
        rules
            .object("GRINDER")
            .unwrap()
            .phobos_grinding
            .weapon
            .as_deref(),
        Some("GWPN")
    );
    let weapon = rules.weapon("GWPN").expect("grinding weapon");
    assert_eq!(weapon.damage, 25);
    assert_eq!(weapon.warhead.as_deref(), Some("GWWH"));
    assert!(rules.warhead("GWWH").is_some());
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let first = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    stand_on_grinder(&mut sim, grinder, first, 10, 10);
    let health = sim.entities().get(grinder).unwrap().health.current();
    sim.sound_events.clear_for_test();
    assert!(sim.foot_per_cell_eaten(first, &rules, None, FrameEffects::default()));
    // 75 refunded is below the 100 required: the accumulator keeps it and the
    // weapon stays silent.
    assert_eq!(
        sim.entities()
            .get(grinder)
            .unwrap()
            .phobos_grinding_weapon_accumulated_credits(),
        75
    );
    assert_eq!(
        sim.entities().get(grinder).unwrap().health.current(),
        health
    );
    // `Grinding.PlayDieSound=no` suppresses the victim's die sounds, and the
    // type sound replaces the stock EnterGrinderSound.
    let played: Vec<&str> = sim
        .sound_events()
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::SimSoundEvent::VocAt { sound_id, .. } => Some(sound_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(played, ["OwnGrind"]);

    let second = spawn(&mut sim, &rules, "CIV", "Russians", 11, 10);
    stand_on_grinder(&mut sim, grinder, second, 11, 10);
    sim.sound_events.clear_for_test();
    assert!(sim.foot_per_cell_eaten(second, &rules, None, FrameEffects::default()));
    // The second 75 reaches 150: the weapon fires at the grinder and the
    // accumulator clears.
    assert_eq!(
        sim.entities()
            .get(grinder)
            .unwrap()
            .phobos_grinding_weapon_accumulated_credits(),
        0
    );
    assert_eq!(
        sim.entities().get(grinder).unwrap().health.current(),
        health - 25
    );
}

/// Phobos `UnitClass_WhatAction_Grinding`/`InfantryClass_WhatAction_Grinding`:
/// a refused grinder answers NoEnter instead of falling back to Select, and
/// an allied grinder answers the infantry row through the type gates.
#[test]
fn grinding_click_rows_answer_no_enter_for_a_refused_grinder() {
    use crate::sim::world::{EntryBaseAction, EntryClickAction};
    let (mut sim, rules) = fixture("Grinding.AllowOwner=no\n");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Russians", 10, 10);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(Some(sim.substrate.entities.get(tank).unwrap().owner()));
    assert_eq!(
        sim.entry_click_action(tank, grinder, EntryBaseAction::Select, false, &rules),
        Some(EntryClickAction::NoEnter)
    );

    // A same-house grinder that admits the victim still answers Grind; the
    // force-fire key with an armed Unit skips the building row entirely.
    let (mut sim, rules) = fixture("");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(Some(sim.substrate.entities.get(civ).unwrap().owner()));
    assert_eq!(
        sim.entry_click_action(civ, grinder, EntryBaseAction::Select, false, &rules),
        Some(EntryClickAction::Grind)
    );
    let tank = spawn(&mut sim, &rules, "TANK", "Russians", 10, 13);
    assert_eq!(
        sim.entry_click_action(tank, grinder, EntryBaseAction::Select, false, &rules),
        Some(EntryClickAction::Grind)
    );
    assert_eq!(
        sim.entry_click_action(tank, grinder, EntryBaseAction::Select, true, &rules),
        None,
        "the force-fire key skips the building row"
    );
}

/// Phobos runs `DoGrindingExtras` inside the Grinding arm (`0x5198B3`,
/// after the Ares hook at `0x5198AD`), so a non-`Grinding=` building keeps
/// none of the extras even when its type authors them.
#[test]
fn a_non_grinding_building_never_runs_the_grinding_extras() {
    let (mut sim, rules) = fixture("");
    let other = spawn(&mut sim, &rules, "OTHER", "Russians", 18, 10);
    let first = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    stand_on_grinder(&mut sim, other, first, 10, 10);
    let health = sim.entities().get(other).unwrap().health.current();
    sim.sound_events.clear_for_test();
    assert!(sim.foot_per_cell_eaten(first, &rules, None, FrameEffects::default()));
    let entity = sim.entities().get(other).unwrap();
    assert_eq!(
        entity.phobos_grinding_weapon_accumulated_credits(),
        0,
        "no refund reaches the weapon accumulator"
    );
    assert_eq!(entity.health.current(), health, "the weapon never fires");
    let played: Vec<&str> = sim
        .sound_events()
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::SimSoundEvent::VocAt { sound_id, .. } => Some(sound_id.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        played.is_empty(),
        "no type sound and no stock grinder sound: {played:?}"
    );
}

/// Phobos `FootClass_Mission_Eaten_Grinding` (`0x4D4CD3`): a destination
/// grinder that refuses the foot loses the destination instead of being
/// walked into.
#[test]
fn mission_eaten_clears_a_destination_the_grinder_type_refuses() {
    use crate::sim::components::NavTargetRef;
    let (mut sim, rules) = fixture("Grinding.AllowOwner=no\n");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    mission(&mut sim, civ);
    let _ = sim.assign_destination_represented(
        civ,
        Some(NavTargetRef::object(grinder)),
        &rules,
        None,
        FrameEffects::default(),
    );
    assert!(
        sim.entities()
            .get(civ)
            .unwrap()
            .navigation
            .nav_com()
            .is_some()
    );
    crate::sim::mission::eaten::mission_eaten(&mut sim, &rules, None, civ, FrameEffects::default());
    assert_eq!(
        sim.entities().get(civ).unwrap().navigation.nav_com(),
        None,
        "the refused grinder is dropped"
    );
}

/// Phobos `InfantryClass_WhatAction_Grinding_Engineer` (`0x51E63A`): the
/// engineer's grinder action asks the type gates and answers NoGRepair
/// (`Repair(false)`) when they refuse.
#[test]
fn grinding_engineer_row_uses_the_type_gates() {
    use crate::sim::world::EngineerBuildingAction;
    let run = |allow_owner: bool| {
        let (mut sim, rules) = fixture(if allow_owner {
            ""
        } else {
            "Grinding.AllowOwner=no\n"
        });
        let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
        let engineer = spawn(&mut sim, &rules, "ENGINEER", "Russians", 10, 10);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session.set_current_house_for_test(Some(
            sim.substrate.entities.get(engineer).unwrap().owner(),
        ));
        sim.engineer_building_action(engineer, grinder, &rules)
    };
    assert_eq!(run(true), Some(EngineerBuildingAction::EnterGrinder));
    assert_eq!(run(false), Some(EngineerBuildingAction::Repair(false)));
}

/// Phobos `DoGrindingExtras` also feeds the grinder's `AccumulatedIncome`
/// (`Ext/Building/Body.cpp:266-271`), the value the periodic `DisplayIncome`
/// label prints.
#[test]
fn grinding_refund_feeds_the_display_income_accumulator() {
    let (mut sim, rules) = fixture("");
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Americans", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    stand_on_grinder(&mut sim, grinder, civ, 10, 10);
    let owner = sim.interner().get("Americans").unwrap();
    let before = crate::sim::credit_income::available_money(&sim.houses, owner);
    assert!(sim.foot_per_cell_eaten(civ, &rules, None, FrameEffects::default()));
    let after = crate::sim::credit_income::available_money(&sim.houses, owner);
    assert_eq!(
        sim.substrate
            .entities
            .get(grinder)
            .unwrap()
            .phobos_accumulated_income(),
        after.wrapping_sub(before),
        "the accumulator takes the refund the owner was credited"
    );
}
