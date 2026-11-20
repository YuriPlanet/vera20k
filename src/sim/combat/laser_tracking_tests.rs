//! Phobos `WeaponTypeExt::LaserPositionUpdate` / `LaserRT` tracking:
//! `SetLaserTrackingData` at birth and `LaserDrawClass_Update`'s per-frame
//! endpoint refresh, through the production FireAt laser path.

use super::TargetKind;
use super::fire_coord::{FireSource, fire_coordinate};
use super::laser::{self, LaserBirth};
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::flh::Flh;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
use crate::sim::world::{LifecycleOutput, Simulation};
use crate::util::fixed_math::SimFixed;

const ART: &str = "[UNITART]\nPrimaryFireFLH=0,0,0\n\
                   [CHASERART]\nPrimaryFireFLH=0,0,0\n\
                   [BUNKART]\nFoundation=1x1\nPrimaryFireFLH=0,0,0\n";

fn weapon_section(mode: Option<&str>, stop: Option<bool>) -> String {
    let mode = mode.map_or(String::new(), |mode| {
        format!("LaserPositionUpdate={mode}\n")
    });
    let stop = match stop {
        Some(value) => format!(
            "LaserPositionUpdate.StopOnFirerConvert={}\n",
            if value { "yes" } else { "no" }
        ),
        None => String::new(),
    };
    format!(
        "[GUN]\nIsLaser=yes\nIsHouseColor=yes\nLaserDuration=15\n\
         LaserInnerColor=100,120,140\nLaserOuterColor=10,20,30\n\
         LaserOuterSpread=0,0,0\n{mode}{stop}"
    )
}

fn build(rules_ini: &str) -> RuleSet {
    build_with_art(rules_ini, ART)
}

fn build_with_art(rules_ini: &str, art: &str) -> RuleSet {
    let art_ini = IniFile::from_str(art);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(rules_ini),
        &art_ini,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules
}

const BURST_ART: &str = "[UNITART]
PrimaryFireFLH=100,20,30
                        PrimaryFireFLH.Burst0=200,40,50
                        PrimaryFireFLH.Burst1=300,60,70
                        [CONTROLART]
PrimaryFireFLH=300,60,70
                        [BASEART]
PrimaryFireFLH=100,20,30
                        [CHASERART]
PrimaryFireFLH=0,0,0
";

fn burst_scene() -> (Simulation, RuleSet) {
    let rules = build_with_art(
        &format!(
            "[VehicleTypes]
0=UNIT
1=CHASER
2=CONTROL
3=BASE
             [UNIT]
Image=UNITART
Strength=600
Primary=GUN
             [CHASER]
Image=CHASERART
Strength=1000
             [CONTROL]
Image=CONTROLART
Strength=600
Primary=GUN
             [BASE]
Image=BASEART
Strength=600
Primary=GUN
{}",
            weapon_section(Some("Firer"), None)
        ),
        BURST_ART,
    );
    let mut sim = Simulation::new();
    place(&mut sim, 1, "UNIT", EntityCategory::Unit, [2560, 2560, 0]);
    place(&mut sim, 2, "CHASER", EntityCategory::Unit, [5120, 5120, 0]);
    (sim, rules)
}

fn coordinate_of(sim: &Simulation, rules: &RuleSet, kind: &str, burst: u8) -> ProjectileCoord {
    let source = sim.substrate.entities.get(1).unwrap();
    fire_coordinate(
        sim,
        rules,
        &FireSource::of_entity(source),
        rules.object(kind).unwrap(),
        0,
        burst,
        Flh::default(),
    )
    .coord
}

fn unit_scene(mode: Option<&str>, stop: Option<bool>, global_stop: bool) -> (Simulation, RuleSet) {
    let global = if global_stop {
        "[AudioVisual]\nLaserPositionUpdate.StopOnFirerConvert=yes\n"
    } else {
        ""
    };
    let weapon = weapon_section(mode, stop);
    let rules = build(&format!(
        "[VehicleTypes]\n0=UNIT\n1=CHASER\n\
         [UNIT]\nImage=UNITART\nStrength=600\nPrimary=GUN\n\
         [CHASER]\nImage=CHASERART\nStrength=1000\n{global}{weapon}"
    ));
    let mut sim = Simulation::new();
    place(&mut sim, 1, "UNIT", EntityCategory::Unit, [2560, 2560, 0]);
    place(&mut sim, 2, "CHASER", EntityCategory::Unit, [5120, 5120, 0]);
    (sim, rules)
}

fn building_scene(occupants: i32) -> (Simulation, RuleSet) {
    let rules = build(&format!(
        "[VehicleTypes]\n0=CHASER\n\
         [BuildingTypes]\n0=BUNKER\n\
         [CHASER]\nImage=CHASERART\nStrength=1000\n\
         [BUNKER]\nImage=BUNKART\nStrength=600\nPrimary=GUN\n\
         MaxNumberOccupants={occupants}\n{}",
        weapon_section(Some("Firer"), None)
    ));
    let mut sim = Simulation::new();
    place(
        &mut sim,
        1,
        "BUNKER",
        EntityCategory::Structure,
        [2560, 2560, 0],
    );
    place(&mut sim, 2, "CHASER", EntityCategory::Unit, [5120, 5120, 0]);
    (sim, rules)
}

fn place(sim: &mut Simulation, id: u64, kind: &str, category: EntityCategory, at: [i32; 3]) {
    let mut entity = GameEntity::test_default(
        id,
        kind,
        "Americans",
        (at[0] / 256) as u16,
        (at[1] / 256) as u16,
    );
    entity.set_type_ref_for_test(sim.intern(kind));
    entity.set_owner_for_test(sim.intern("Americans"));
    entity.set_category_for_test(category);
    entity
        .position
        .set_sub_x_for_test(SimFixed::from_num(at[0] % 256));
    entity
        .position
        .set_sub_y_for_test(SimFixed::from_num(at[1] % 256));
    entity.position.set_exact_z_leptons_for_test(Some(at[2]));
    entity.type_facts.set_foundation_for_test("1x1".into());
    sim.substrate.entities.insert(entity);
}

fn fire(sim: &mut Simulation, rules: &RuleSet) -> LaserBirth {
    laser::fired(
        sim,
        rules,
        1,
        TargetKind::Entity(2),
        0,
        rules.weapon("GUN").unwrap(),
        0,
    );
    *sim.lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::LaserCreated(birth) => Some(birth),
            _ => None,
        })
        .last()
        .expect("laser birth")
}

fn move_to(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
    let position = &mut sim.substrate.entities.get_mut(id).unwrap().position;
    position.set_rx_for_test(rx);
    position.set_ry_for_test(ry);
}

#[test]
fn all_follows_the_firer_and_the_target() {
    let (mut sim, rules) = unit_scene(Some("All"), None, false);
    let mut birth = fire(&mut sim, &rules);
    assert!(birth.track.is_some());
    assert_eq!(birth.from, ProjectileCoord::new(2560, 2560, 0));
    assert_eq!(birth.to, ProjectileCoord::new(5120, 5120, 0));

    move_to(&mut sim, 1, 11, 10);
    move_to(&mut sim, 2, 20, 21);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, ProjectileCoord::new(2816, 2560, 0));
    assert_eq!(birth.to, ProjectileCoord::new(5120, 5376, 0));
}

#[test]
fn firer_and_target_modes_follow_only_their_own_end() {
    let (mut sim, rules) = unit_scene(Some("Firer"), None, false);
    let mut birth = fire(&mut sim, &rules);
    move_to(&mut sim, 1, 11, 10);
    move_to(&mut sim, 2, 20, 21);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, ProjectileCoord::new(2816, 2560, 0));
    assert_eq!(
        birth.to,
        ProjectileCoord::new(5120, 5120, 0),
        "Firer leaves the target endpoint copied"
    );

    let (mut sim, rules) = unit_scene(Some("Target"), None, false);
    let mut birth = fire(&mut sim, &rules);
    move_to(&mut sim, 1, 11, 10);
    move_to(&mut sim, 2, 20, 21);
    birth.update_tracking(&sim, &rules);
    assert_eq!(
        birth.from,
        ProjectileCoord::new(2560, 2560, 0),
        "Target leaves the source endpoint copied"
    );
    assert_eq!(birth.to, ProjectileCoord::new(5120, 5376, 0));
}

#[test]
fn a_weapon_without_the_key_keeps_both_endpoints() {
    let (mut sim, rules) = unit_scene(None, None, false);
    let mut birth = fire(&mut sim, &rules);
    assert!(birth.track.is_none());
    move_to(&mut sim, 1, 11, 10);
    move_to(&mut sim, 2, 20, 21);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, ProjectileCoord::new(2560, 2560, 0));
    assert_eq!(birth.to, ProjectileCoord::new(5120, 5120, 0));
}

#[test]
fn a_transport_building_does_not_follow_its_firer() {
    let (mut sim, rules) = building_scene(5);
    let birth = fire(&mut sim, &rules);
    assert!(
        birth.track.is_none(),
        "MaxNumberOccupants gates the Firer bit"
    );

    let (mut sim, rules) = building_scene(0);
    let mut birth = fire(&mut sim, &rules);
    assert!(birth.track.is_some());
    let initial = birth.from;
    move_to(&mut sim, 1, 11, 10);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from.x, initial.x + 256);
}

#[test]
fn stop_on_firer_convert_drops_the_follow_after_a_type_change() {
    let (mut sim, rules) = unit_scene(Some("Firer"), Some(true), false);
    let mut birth = fire(&mut sim, &rules);
    move_to(&mut sim, 1, 11, 10);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, ProjectileCoord::new(2816, 2560, 0));

    let chaser = sim.intern("CHASER");
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_type_ref_for_test(chaser);
    move_to(&mut sim, 1, 12, 10);
    birth.update_tracking(&sim, &rules);
    assert_eq!(
        birth.from,
        ProjectileCoord::new(2816, 2560, 0),
        "the converted firer is no longer followed"
    );
}

#[test]
fn without_the_latch_a_converted_firer_is_still_followed() {
    let (mut sim, rules) = unit_scene(Some("Firer"), None, false);
    let mut birth = fire(&mut sim, &rules);
    let chaser = sim.intern("CHASER");
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_type_ref_for_test(chaser);
    move_to(&mut sim, 1, 11, 10);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, ProjectileCoord::new(2816, 2560, 0));
}

#[test]
fn the_audio_visual_default_supplies_the_latch() {
    let (mut sim, rules) = unit_scene(Some("Firer"), None, true);
    let mut birth = fire(&mut sim, &rules);
    let chaser = sim.intern("CHASER");
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_type_ref_for_test(chaser);
    move_to(&mut sim, 1, 11, 10);
    birth.update_tracking(&sim, &rules);
    assert_eq!(
        birth.from,
        ProjectileCoord::new(2560, 2560, 0),
        "the global default resolves the nullable"
    );
}

#[test]
fn an_ignoring_caller_keeps_the_source_but_still_tracks_the_target() {
    let (mut sim, rules) = unit_scene(Some("All"), None, false);
    let from = ProjectileCoord::new(2000, 2000, 0);
    let mut birth = *laser::create(
        &mut sim,
        &rules,
        1,
        ProjectileTarget::Entity(2),
        0,
        rules.weapon("GUN").unwrap(),
        Some(from),
        true,
    )
    .unwrap();
    assert!(birth.track.is_some());
    move_to(&mut sim, 1, 11, 10);
    move_to(&mut sim, 2, 20, 21);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, from, "shrapnel's CreateLaser ignores the firer");
    assert_eq!(birth.to, ProjectileCoord::new(5120, 5376, 0));
}

#[test]
fn a_burst_art_offset_replaces_the_slot_flh_without_a_lateral_shift() {
    let (mut sim, rules) = burst_scene();
    // Odd burst index: the authored offset is absolute and keeps its lateral.
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .weapon_burst
        .advance();
    let birth = fire(&mut sim, &rules);
    let burst = coordinate_of(&sim, &rules, "UNIT", 1);
    assert_eq!(birth.from, burst);
    assert_eq!(
        birth.from,
        coordinate_of(&sim, &rules, "CONTROL", 0),
        "an absolute burst offset takes no odd-burst lateral shift"
    );
}

#[test]
fn a_tracked_laser_keeps_the_frozen_burst_offset() {
    let (mut sim, rules) = burst_scene();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .weapon_burst
        .advance();
    let mut birth = fire(&mut sim, &rules);
    assert_eq!(
        birth.track.map(|track| track.local_flh()),
        Some(Flh::from([300, 60, 70])),
        "GetBurstFLH freezes the authored burst offset"
    );
    let initial = birth.from;
    birth.update_tracking(&sim, &rules);
    assert_eq!(
        birth.from, initial,
        "the tracked GetFLH sample keeps the same burst offset"
    );
    move_to(&mut sim, 1, 11, 10);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from.x, initial.x + 256);
    assert_eq!(birth.from.y, initial.y);
}

#[test]
fn a_burst_index_past_the_run_falls_back_to_the_slot_offset() {
    let (mut sim, rules) = burst_scene();
    let burst = &mut sim.substrate.entities.get_mut(1).unwrap().weapon_burst;
    burst.advance();
    burst.advance();
    let birth = fire(&mut sim, &rules);
    assert_eq!(
        birth.track.map(|track| track.local_flh()),
        Some(Flh::from([100, 20, 30])),
        "past the run GetBurstFLH falls back to the slot FLH"
    );
    assert_eq!(birth.from, coordinate_of(&sim, &rules, "UNIT", 2));
    assert_eq!(
        birth.from,
        coordinate_of(&sim, &rules, "BASE", 0),
        "the authored run stops at index 2"
    );
}

#[test]
fn a_dead_firer_stops_the_follow_without_moving_the_source() {
    let (mut sim, rules) = unit_scene(Some("Firer"), None, false);
    let mut birth = fire(&mut sim, &rules);
    sim.substrate.entities.remove(1);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, ProjectileCoord::new(2560, 2560, 0));
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, ProjectileCoord::new(2560, 2560, 0));
}

#[test]
fn an_expired_firer_still_tracks_the_target() {
    let (mut sim, rules) = unit_scene(Some("All"), None, false);
    let house = sim.intern("Americans");
    let (from, to) = (
        ProjectileCoord::new(2000, 2000, 0),
        ProjectileCoord::new(5120, 5120, 0),
    );
    sim.substrate.entities.remove(1);
    let track = super::laser::build_track(
        &sim,
        &rules,
        1,
        ProjectileTarget::Entity(2),
        0,
        crate::rules::phobos_laser_tracking::PositionFollow::All,
        false,
        from,
        true,
    );
    assert!(
        track.is_some(),
        "SimulatedFiringLaser tracks the target after the owner expired"
    );
    let mut birth = *super::laser::create_effect(
        &mut sim,
        Some(house),
        from,
        to,
        rules.weapon("GUN").unwrap(),
    );
    birth.track = track;
    move_to(&mut sim, 2, 20, 21);
    birth.update_tracking(&sim, &rules);
    assert_eq!(birth.from, from);
    assert_eq!(birth.to, ProjectileCoord::new(5120, 5376, 0));
}
