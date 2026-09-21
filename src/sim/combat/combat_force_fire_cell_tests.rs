//! Force-fire-on-cell unit tests for `issue_attack_cell_command`.
//!
//! Verifies the sim-side entry point for `Command::ForceAttackCell` —
//! Ctrl + left-click on empty terrain.

use super::{AttackTarget, TargetKind, issue_attack_cell_command};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{Health, MovementTarget};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;

/// Minimal RuleSet: armed MTNK (Primary=105mm) and unarmed ENGI (no Primary).
fn ff_rules() -> RuleSet {
    let ini_str: &str = "\
[VehicleTypes]\n0=MTNK\n\n\
[InfantryTypes]\n0=ENGI\n\n\
[BuildingTypes]\n\n\
[AircraftTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[ENGI]\nStrength=75\nArmor=none\nSpeed=4\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n";
    let ini: IniFile = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("ff_rules should parse")
}

fn make_unit(id: u64, type_ref: &str, rx: u16, ry: u16, hp: i32) -> GameEntity {
    let mut e = GameEntity::test_default(id, type_ref, "Americans", rx, ry);
    e.health = Health { current: hp };
    e
}

#[test]
fn issue_attack_cell_sets_cell_target_for_armed_unit() {
    let mut store = EntityStore::new();
    store.insert(make_unit(1, "MTNK", 5, 5, 300));
    let interner = test_interner();
    let rules = ff_rules();

    let ok = issue_attack_cell_command(&mut store, 1, 50, 50, Some(&rules), &interner);

    assert!(
        ok,
        "issue_attack_cell_command should succeed for armed unit"
    );
    let attack = store.get(1).unwrap().attack_target.as_ref().unwrap();
    assert!(matches!(attack.target, TargetKind::Cell(50, 50)));
    assert_eq!(attack.cooldown_ticks, 0);
    assert_eq!(attack.burst_remaining, 0);
}

#[test]
fn issue_attack_cell_rejects_unarmed_attacker() {
    let mut store = EntityStore::new();
    store.insert(make_unit(1, "ENGI", 5, 5, 75));
    let interner = test_interner();
    let rules = ff_rules();

    let ok = issue_attack_cell_command(&mut store, 1, 50, 50, Some(&rules), &interner);

    assert!(!ok, "ForceAttackCell on unarmed unit must return false");
    assert!(store.get(1).unwrap().attack_target.is_none());
}

#[test]
fn issue_attack_cell_clears_movement_target() {
    let mut store = EntityStore::new();
    store.insert(make_unit(1, "MTNK", 5, 5, 300));
    store.get_mut(1).unwrap().movement_target = Some(MovementTarget::default());
    let interner = test_interner();
    let rules = ff_rules();

    let ok = issue_attack_cell_command(&mut store, 1, 50, 50, Some(&rules), &interner);

    assert!(ok);
    assert!(store.get(1).unwrap().movement_target.is_none());
}

#[test]
fn issue_attack_cell_returns_false_for_missing_attacker() {
    let mut store = EntityStore::new();
    let interner = test_interner();
    let rules = ff_rules();

    let ok = issue_attack_cell_command(&mut store, 999, 50, 50, Some(&rules), &interner);

    assert!(
        !ok,
        "Should return false when attacker entity does not exist"
    );
}

#[test]
fn for_cell_constructor_creates_cell_variant() {
    let at = AttackTarget::for_cell(42, 17);
    assert!(matches!(at.target, TargetKind::Cell(42, 17)));
    assert_eq!(at.cooldown_ticks, 0);
    assert_eq!(at.burst_remaining, 0);
    assert_eq!(at.burst_delay_ticks, 0);
}

#[test]
fn new_constructor_creates_entity_variant() {
    let at = AttackTarget::new(123);
    assert!(matches!(at.target, TargetKind::Entity(123)));
}

/// Production proof for the combat-explosion animation route: a real shot,
/// through `Simulation::advance_tick`, must leave an `AnimClass` instance in
/// `AnimStore` carrying the constructor row
/// `BulletClass::DetonateAtCoord 0x00469C93` pushes, and must emit the art
/// type's `Report=`. Before this route existed the same shot pushed a legacy
/// `WorldEffect` with translucency forced on and no sound at all.
#[test]
fn force_fire_detonation_builds_an_anim_instance_and_plays_its_report() {
    use crate::rules::art_data::ArtRegistry;
    use crate::sim::anim_class::{COMBAT_EXPLOSION_DRAW_FLAGS, COMBAT_EXPLOSION_Z_ADJUST};
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::world::{SimSoundEvent, Simulation};
    use std::collections::BTreeMap;

    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n\
         [InfantryTypes]\n\n[BuildingTypes]\n\n[AircraftTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n\
         AnimList=TWLT036\n",
    ))
    .expect("explosion rules parse");
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[TWLT036]\nTranslucent=yes\nReport=Explosion06\nEnd=8\n",
    ));
    art.bind_anim_frame_count_for_test("TWLT036", 8);
    rules.art_registry = art;

    let mut sim = Simulation::new();
    sim.input_delay_ticks = 0;
    sim.substrate
        .entities
        .insert(make_unit(1, "MTNK", 5, 5, 300));
    sim.interner = crate::sim::intern::test_interner();
    let owner_id = sim.interner.intern("Americans");
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let grid = PathGrid::test_all_passable(64, 64);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    // In range from the start, so the shot lands without a pursuit walk.
    sim.queue_command(CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::ForceAttackCell {
            attacker_id: 1,
            target_rx: 8,
            target_ry: 5,
        },
    ));

    let explosion_type = sim.interner.intern("TWLT036");
    let mut explosion = None;
    for _ in 0..60 {
        let pending = sim.take_due_commands();
        sim.advance_tick(&pending, Some(&rules), &height_map, Some(&grid), None, 100);
        if let Some((&id, _)) = sim.anims().find(|(_, anim)| anim.type_id == explosion_type) {
            explosion = Some(id);
            break;
        }
    }

    let id = explosion.expect("the detonation must construct a TWLT036 AnimClass");
    let anim = sim.anim(id).expect("registered explosion anim");
    assert_eq!(anim.draw_flags, COMBAT_EXPLOSION_DRAW_FLAGS);
    assert_eq!(anim.z_adjust, COMBAT_EXPLOSION_Z_ADJUST);
    assert!(anim.start_sound_active);

    let report = sim.interner.intern("EXPLOSION06");
    assert!(
        sim.sound_events.iter().any(|event| matches!(
            event,
            SimSoundEvent::AnimationStarted { anim_id, sound_id, .. }
                if *anim_id == id && *sound_id == report
        )),
        "the explosion must play its art `Report=`"
    );
}

#[test]
fn force_fire_cell_pursuit_then_fire_integration() {
    // Full pipeline: ctrl-click on a far cell → ForceAttackCell command →
    // attack_target=Cell set → out of range → pursuit issues movement →
    // (many ticks of walking) → in range → combat fires.
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::world::Simulation;
    use std::collections::BTreeMap;

    let rules = ff_rules();
    let mut sim = Simulation::new();
    sim.input_delay_ticks = 0;
    sim.substrate
        .entities
        .insert(make_unit(1, "MTNK", 5, 5, 300));
    // Replace sim interner with the test interner so type_ref/owner IDs from
    // GameEntity::test_default resolve correctly.
    sim.interner = crate::sim::intern::test_interner();
    let owner_id = sim.interner.intern("Americans");
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let grid = PathGrid::test_all_passable(64, 64);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    sim.queue_command(CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::ForceAttackCell {
            attacker_id: 1,
            target_rx: 15,
            target_ry: 5,
        },
    ));

    // Tick 1: EventClass applies the command at the native Main_Tick tail,
    // after this frame's pursuit/object walk has already completed.
    let pending = sim.take_due_commands();
    sim.advance_tick(&pending, Some(&rules), &height_map, Some(&grid), None, 100);

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.attack_target.is_some(),
        "the command tail sets attack_target"
    );
    assert!(
        entity.movement_target.is_none(),
        "the completed object walk cannot observe a tail-dispatched command"
    );

    // Tick 2: the next object walk observes the target and starts pursuit.
    sim.advance_tick(&[], Some(&rules), &height_map, Some(&grid), None, 100);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .is_some_and(|e| e.movement_target.is_some()),
        "the following frame starts out-of-range pursuit"
    );

    // Tick many times until the unit walks into range and fires.
    let mut fired = false;
    for _ in 0..400 {
        let pending = sim.take_due_commands();
        sim.advance_tick(&pending, Some(&rules), &height_map, Some(&grid), None, 100);
        if !sim.fire_events.is_empty() {
            fired = true;
            break;
        }
        assert!(
            sim.substrate
                .entities
                .get(1)
                .is_some_and(|e| e.attack_target.is_some()),
            "attack_target dropped mid-pursuit (parity bug)"
        );
    }

    assert!(
        fired,
        "unit should walk into range and fire within 400 ticks"
    );
}

/// End to end through the production tick: a force-fired shot constructs the
/// weapon's `Anim=` as an `AnimClass` in the store, at the shot's fire
/// coordinate, owned by the firer (`TechnoClass::Fire_At`, `0x006FF3C2` and
/// `0x006FF43A`).
#[test]
fn a_fired_shot_constructs_its_muzzle_anim_in_the_store() {
    use crate::rules::art_data::ArtRegistry;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::world::Simulation;
    use std::collections::BTreeMap;

    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\n\
         [Animations]\n0=GUNFIRE\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\nAnim=GUNFIRE\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    ))
    .expect("rules");
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[MTNK]\nPrimaryFireFLH=150,0,100\n[GUNFIRE]\nRate=900\n",
    ));
    art.bind_anim_frame_count_for_test("GUNFIRE", 6);
    rules.merge_art_data(&art);
    rules.art_registry = art;

    let mut sim = Simulation::new();
    sim.input_delay_ticks = 0;
    // The tank takes its id from the shared counter, so the anim's id is free.
    let tank = sim.allocate_stable_id();
    assert_eq!(tank, 1);
    sim.substrate
        .entities
        .insert(make_unit(tank, "MTNK", 5, 5, 300));
    sim.interner = crate::sim::intern::test_interner();
    let owner_id = sim.interner.intern("Americans");
    sim.reveal(tank);
    let grid = PathGrid::test_all_passable(64, 64);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    sim.queue_command(CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::ForceAttackCell {
            attacker_id: tank,
            target_rx: 8,
            target_ry: 5,
        },
    ));

    let mut shot = None;
    for _ in 0..200 {
        let pending = sim.take_due_commands();
        sim.advance_tick(&pending, Some(&rules), &height_map, Some(&grid), None, 100);
        if let Some(event) = sim.fire_events.first() {
            shot = Some(event.clone());
            break;
        }
    }
    let shot = shot.expect("the tank fires at a cell three cells away");
    assert_eq!(
        shot.muzzle_anim
            .map(|id| sim.interner.resolve(id).to_string()),
        Some("GUNFIRE".to_string())
    );

    let muzzle: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "GUNFIRE")
        .map(|(id, anim)| (*id, anim.owner_entity, anim.draw_flags))
        .collect();
    assert_eq!(muzzle.len(), 1, "one shot, one muzzle anim");
    let (anim_id, owner, draw_flags) = muzzle[0];
    assert_eq!((owner, draw_flags), (Some(1), 0x600));
    let absolute = sim.anim_absolute_coord(anim_id).expect("anim coordinate");
    assert_eq!(
        (absolute.x, absolute.y, absolute.z),
        (shot.fire_coord.x, shot.fire_coord.y, shot.fire_coord.z),
        "the flash sits on the coordinate the shot left from"
    );
    // The FLH put the muzzle off the hull centre and 100 leptons up.
    let centre = (5 * 256 + 128, 5 * 256 + 128);
    assert_ne!((shot.fire_coord.x, shot.fire_coord.y), centre);
    assert_eq!(shot.fire_coord.z, 100);
}
