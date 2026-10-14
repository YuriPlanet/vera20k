//! A planted C4 charge's expiry (BuildingClass::Update's C4 tail) and the
//! player Sabotage order's refusals. The plant itself is `sim::c4_plant`'s.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::PendingC4Detonation;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::timer::CdTimer;

fn c4_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n0=GHOST\n1=TANY\n2=E1\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAPILE\n1=CAMISC01\n2=GAREFN\n\n\
         [GHOST]\nStrength=125\nArmor=flak\nSpeed=4\nC4=yes\nPrimary=Pistol\n\n\
         [TANY]\nStrength=125\nArmor=flak\nSpeed=4\nC4=yes\nPrimary=Pistol\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [GAPILE]\nStrength=600\nArmor=wood\n\n\
         [CAMISC01]\nStrength=600\nArmor=concrete\nCanC4=no\n\n\
         [GAREFN]\nStrength=1000\nArmor=wood\n\n\
         [Pistol]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [CombatDamage]\nC4Warhead=SA\n\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let art_ini: IniFile = IniFile::from_str(
        "[GAPILE]\nFoundation=2x2\n\
         [CAMISC01]\nFoundation=1x1\n\
         [GAREFN]\nFoundation=4x3\nAddOccupy1=-1,0\nAddOccupy2=-1,-1\nRemoveOccupy1=3,1\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini)
        .expect("c4 test rules should parse");
    let art = ArtRegistry::from_ini(&art_ini);
    rules.install_art_data(art);
    rules
}

fn c4_damage_state_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n0=GHOST\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAPILE\n\n\
         [GHOST]\nStrength=125\nArmor=flak\nSpeed=4\nC4=yes\nPrimary=Pistol\n\n\
         [GAPILE]\nStrength=600\nArmor=wood\n\n\
         [Pistol]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [CombatDamage]\nC4Warhead=SA\n\n\
         [SA]\nVerses=50%,50%,50%,50%,50%,50%,50%,50%,50%,0%,0%\n",
    );
    let art_ini = IniFile::from_str("[GAPILE]\nFoundation=2x2\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini)
        .expect("c4 damage-state rules should parse");
    let art = ArtRegistry::from_ini(&art_ini);
    rules.install_art_data(art);
    rules
}

fn build_sim_with_c4_rules() -> (Simulation, RuleSet) {
    let mut sim = Simulation::new();
    let rules = c4_test_rules();
    // Required: the expiry reads the C4Warhead handle, which panics
    // unless this resolver has run.
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn build_sim_with_c4_damage_state_rules() -> (Simulation, RuleSet) {
    let mut sim = Simulation::new();
    let rules = c4_damage_state_rules();
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn spawn_infantry(sim: &mut Simulation, type_str: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    let owner_id = sim.interner.intern(owner);
    let type_id = sim.interner.intern(type_str);
    let id = sim.substrate.next_stable_object_id;
    sim.substrate.next_stable_object_id += 1;
    let e = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health::new(125),
        type_id,
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(e);
    assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
    id
}

fn spawn_building(sim: &mut Simulation, type_str: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    let owner_id = sim.interner.intern(owner);
    let type_id = sim.interner.intern(type_str);
    let id = sim.substrate.next_stable_object_id;
    sim.substrate.next_stable_object_id += 1;
    let e = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health::new(600),
        type_id,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(e);
    assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
    id
}

/// Advance one tick, draining any pending commands first (mirrors the
/// production app sim-tick loop).
fn step(sim: &mut Simulation, rules: &RuleSet) {
    let due = sim.take_due_commands();
    sim.advance_tick(&due, Some(rules), None, None, 67);
}

/// `seal`'s C4 charge on `target`, planted this frame.
fn plant_c4(sim: &mut Simulation, rules: &RuleSet, target: u64, seal: u64) {
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .pending_c4_detonation = Some(PendingC4Detonation {
        timer: CdTimer::started(sim.session.binary_frame as i32, rules.c4_delay_frames()),
        source_entity_id: Some(seal),
    });
}

#[test]
fn c4_expiry_ignore_defenses_bypasses_verses_and_kills_building() {
    let (mut sim, rules) = build_sim_with_c4_damage_state_rules();
    let seal = spawn_infantry(&mut sim, "GHOST", "Americans", 10, 11);
    let bld = spawn_building(&mut sim, "GAPILE", "Soviets", 10, 10);
    plant_c4(&mut sim, &rules, bld, seal);

    let delay = rules.c4_delay_frames() as u64;
    for _ in 0..(delay + 2) {
        step(&mut sim, &rules);
        if sim.substrate.entities.get(bld).is_none() {
            break;
        }
    }

    assert!(
        sim.substrate
            .entities
            .get(bld)
            .is_none_or(|building| building.death.is_dying() || building.health.current() == 0),
        "the expiry receiver's ignore_defenses flag must bypass the C4Warhead's 50% Verses"
    );
}

// ---------- Test 2: attacker death mid-plant ----------

#[test]
fn c4_attacker_death_does_not_abort_detonation() {
    let (mut sim, rules) = build_sim_with_c4_rules();
    let seal = spawn_infantry(&mut sim, "GHOST", "Americans", 10, 11);
    let bld = spawn_building(&mut sim, "GAPILE", "Soviets", 10, 10);

    // Manually claim the plant (skip walk-up).
    plant_c4(&mut sim, &rules, bld, seal);

    // Mid-plant: kill the SEAL outright.
    sim.substrate
        .entities
        .get_mut(seal)
        .unwrap()
        .health
        .set_for_test(0);
    sim.substrate
        .entities
        .get_mut(seal)
        .unwrap()
        .death
        .set_dying_for_test(true);
    step(&mut sim, &rules);
    assert!(
        sim.substrate.entities.get(seal).is_none()
            || sim.substrate.entities.get(seal).unwrap().death.is_dying(),
        "SEAL must be despawned or dying after kill"
    );

    // Advance through C4Delay. Building MUST still die — the +0x6df marker
    // is never cleared in the C4 path, so detonation fires regardless.
    let delay = rules.c4_delay_frames() as u64;
    for _ in 0..(delay + 2) {
        step(&mut sim, &rules);
    }
    assert!(
        sim.substrate
            .entities
            .get(bld)
            .is_none_or(|b| b.death.is_dying() || b.health.current() == 0),
        "PARITY (OQ2): detonation must fire even after attacker death"
    );
}

// ---------- Test 3: Iron Curtain cancels pending C4 ----------

#[test]
fn c4_iron_curtain_application_cancels_pending_detonation() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
    let (mut sim, rules) = build_sim_with_c4_rules();
    let seal = spawn_infantry(&mut sim, "GHOST", "Americans", 10, 11);
    let bld = spawn_building(&mut sim, "GAPILE", "Soviets", 10, 10);

    // Claim the plant, then enter the real Building IronCurtain path. The
    // wrapper clears the shared C4/PostMortem latch before applying protection.
    plant_c4(&mut sim, &rules, bld, seal);
    apply_invulnerability(
        sim.substrate.entities.get_mut(bld).unwrap(),
        sim.session.binary_frame,
        40,
        InvulnKind::IronCurtain,
    );
    assert!(
        sim.substrate
            .entities
            .get(bld)
            .unwrap()
            .pending_c4_detonation
            .is_none(),
        "Building IronCurtain entry must cancel the shared C4/PostMortem latch"
    );

    // Advance beyond both C4Delay and the protection duration. Cancellation is
    // permanent; there is no expired-latch retry after Iron Curtain wears off.
    let delay = rules.c4_delay_frames() as u64;
    for _ in 0..(delay + 42) {
        step(&mut sim, &rules);
    }
    let building = sim
        .substrate
        .entities
        .get(bld)
        .expect("cancelled C4 must leave the building alive");
    assert!(!building.death.is_dying());
    assert_eq!(building.health.current(), 600);
}

// ---------- Test 6: a planted charge survives its planter's Stop ----------

#[test]
fn a_planted_charge_survives_its_planters_stop() {
    let (mut sim, rules) = build_sim_with_c4_rules();
    let seal = spawn_infantry(&mut sim, "GHOST", "Americans", 5, 5);
    let bld = spawn_building(&mut sim, "GAPILE", "Soviets", 10, 10);

    let owner = sim.interner.intern("Americans");

    // Stop on the planter does not clear the building's charge.
    let plant_start = sim.session.tick;
    sim.substrate
        .entities
        .get_mut(bld)
        .unwrap()
        .pending_c4_detonation = Some(PendingC4Detonation {
        timer: CdTimer::started(plant_start as i32, rules.c4_delay_frames()),
        source_entity_id: Some(seal),
    });
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Stop { entity_id: seal },
    ));
    step(&mut sim, &rules);
    assert!(
        sim.substrate
            .entities
            .get(bld)
            .unwrap()
            .pending_c4_detonation
            .is_some(),
        "PARITY: claimed plant survives Stop on attacker"
    );

    // And the building still detonates on schedule.
    let delay = rules.c4_delay_frames() as u64;
    for _ in 0..(delay + 2) {
        step(&mut sim, &rules);
    }
    assert!(
        sim.substrate
            .entities
            .get(bld)
            .is_none_or(|b| b.death.is_dying() || b.health.current() == 0),
        "claimed plant detonates on schedule even after Stop on attacker"
    );
}

// ---------- Test 7: CanC4=no building rejects PlantC4 ----------

#[test]
fn cannot_c4_building_rejects_plant_command() {
    let (mut sim, rules) = build_sim_with_c4_rules();
    let seal = spawn_infantry(&mut sim, "GHOST", "Americans", 5, 5);
    let oil = spawn_building(&mut sim, "CAMISC01", "Soviets", 10, 10);

    let owner = sim.interner.intern("Americans");
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::PlantC4 {
            attacker_id: seal,
            target_building_id: oil,
        },
    ));
    step(&mut sim, &rules);
    let seal = sim.substrate.entities.get(seal).unwrap();
    assert!(
        seal.mission.effective().known() != Some(MissionType::Sabotage)
            && seal.navigation.nav_com.is_none(),
        "PlantC4 must be silently rejected for CanC4=no buildings"
    );
    assert!(
        sim.substrate
            .entities
            .get(oil)
            .unwrap()
            .pending_c4_detonation
            .is_none(),
        "rejected PlantC4 must not set pending_c4_detonation on the target"
    );
}

// ---------- Test 8: non-C4 attacker rejects PlantC4 ----------

#[test]
fn non_c4_unit_rejects_plant_command() {
    let (mut sim, rules) = build_sim_with_c4_rules();
    let gi = spawn_infantry(&mut sim, "E1", "Americans", 5, 5);
    let bld = spawn_building(&mut sim, "GAPILE", "Soviets", 10, 10);

    let owner = sim.interner.intern("Americans");
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::PlantC4 {
            attacker_id: gi,
            target_building_id: bld,
        },
    ));
    step(&mut sim, &rules);
    let gi = sim.substrate.entities.get(gi).unwrap();
    assert!(
        gi.mission.effective().known() != Some(MissionType::Sabotage)
            && gi.navigation.nav_com.is_none(),
        "PlantC4 must be silently rejected for non-C4 attackers"
    );
    assert!(
        sim.substrate
            .entities
            .get(bld)
            .unwrap()
            .pending_c4_detonation
            .is_none(),
        "rejected PlantC4 must not set pending_c4_detonation on the target"
    );
}
