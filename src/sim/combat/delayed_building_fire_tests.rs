//! Retail generic Building delayed-fire contract tests (GSI-05.10).

use std::collections::BTreeMap;

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;

fn delayed_building_rules_with_delay(delay: i32) -> RuleSet {
    let rules_ini = IniFile::from_str(
        "[General]\nPrismType=ATESLA\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=GROUND\n\
         [AircraftTypes]\n0=AIR\n\
         [BuildingTypes]\n0=TESLA\n1=NODELAY\n2=ATESLA\n\
         [TESLA]\nImage=NATSLA\nStrength=600\nArmor=steel\nPrimary=GroundGun\nSecondary=OmniGun\n\
         [NODELAY]\nImage=NODELAY\nStrength=600\nArmor=steel\nPrimary=GroundGun\n\
         [ATESLA]\nImage=GAPRIS\nStrength=600\nArmor=steel\nPrimary=GroundGun\n\
         [GROUND]\nStrength=100\nArmor=light\nSpeed=6\n\
         [AIR]\nStrength=100\nArmor=light\nSpeed=8\n\
         [GroundGun]\nDamage=10\nROF=30\nRange=10\nProjectile=GroundProjectile\nWarhead=TESTWH\n\
         [OmniGun]\nDamage=30\nROF=30\nRange=10\nProjectile=OmniProjectile\nWarhead=TESTWH\n\
         [GroundProjectile]\nInviso=yes\nAG=yes\nAA=no\n\
         [OmniProjectile]\nInviso=yes\nAG=yes\nAA=yes\n\
         [TESTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut rules = RuleSet::from_ini(&rules_ini).expect("delayed Building rules");
    let art_text = format!(
        "[NATSLA]\nIsAnimDelayedFire=yes\nDelayedFireDelay={delay}\n\
         [NODELAY]\nHeight=1\n\
         [GAPRIS]\nIsAnimDelayedFire=yes\nDelayedFireDelay=28\n"
    );
    let art = ArtRegistry::from_ini(&IniFile::from_str(&art_text));
    rules.merge_art_data(&art);
    rules
}

fn delayed_building_rules() -> RuleSet {
    delayed_building_rules_with_delay(28)
}

fn spawn(
    sim: &mut Simulation,
    rules: &RuleSet,
    type_id: &str,
    owner: &str,
    rx: u16,
    ry: u16,
) -> u64 {
    sim.spawn_object(type_id, owner, rx, ry, 0, rules, &BTreeMap::new())
        .unwrap_or_else(|| panic!("spawn {type_id}"))
}

fn combat_visit(
    sim: &mut Simulation,
    rules: &RuleSet,
    main_rng: &mut SimRng,
    frame: u32,
) -> CombatTickResult {
    tick_combat(
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        rules,
        &mut sim.interner,
        u64::from(frame),
        100,
        frame,
        main_rng,
    )
}

#[test]
fn gsi_05_10_tesla_arms_without_emission_and_fires_on_visit_28() {
    let rules = delayed_building_rules();
    let tesla = rules.object("TESLA").expect("stock Tesla Coil rules type");
    assert_eq!(tesla.image, "NATSLA");
    assert!(
        rules.art_registry.get("TESLA").is_none(),
        "delayed metadata must come through TESLA Image=NATSLA"
    );
    let mut sim = Simulation::new();
    let tower = spawn(&mut sim, &rules, "TESLA", "Soviet", 5, 5);
    let target = spawn(&mut sim, &rules, "GROUND", "Allies", 7, 5);
    assert!(issue_attack_command(
        &mut sim.substrate.entities,
        tower,
        target,
        Some(&rules),
        &sim.interner,
    ));
    let mut main_rng = SimRng::new(1);

    let first = combat_visit(&mut sim, &rules, &mut main_rng, 1);
    assert!(first.consequences.fire_events().is_empty());
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current,
        100
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(tower)
            .unwrap()
            .pending_building_fire,
        Some(PendingBuildingFire {
            remaining_ticks: 27,
            weapon_slot: WeaponSlot::Primary,
        })
    );

    for visit in 2..=27 {
        let result = combat_visit(&mut sim, &rules, &mut main_rng, visit);
        assert!(
            result.consequences.fire_events().is_empty(),
            "early fire on visit {visit}"
        );
    }
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current,
        100
    );

    let expiry = combat_visit(&mut sim, &rules, &mut main_rng, 28);
    assert_eq!(expiry.consequences.fire_events().len(), 1);
    assert_eq!(
        expiry.consequences.fire_events()[0].weapon_slot,
        WeaponSlot::Primary
    );
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current,
        90
    );
    assert!(
        sim.substrate
            .entities
            .get(tower)
            .unwrap()
            .pending_building_fire
            .is_none()
    );
}

#[test]
fn gsi_05_10_expiry_reads_live_target_but_keeps_saved_weapon_slot() {
    let rules = delayed_building_rules();
    let mut sim = Simulation::new();
    let tower = spawn(&mut sim, &rules, "TESLA", "Soviet", 5, 5);
    let air = spawn(&mut sim, &rules, "AIR", "Allies", 7, 5);
    let ground = spawn(&mut sim, &rules, "GROUND", "Allies", 8, 5);
    // The tower must arm against a genuinely airborne target for the AA
    // secondary to be the saved slot. `TechnoClass::What_Weapon_Should_I_Use`
    // arm V (`0x006F37E7`) reads `ObjectClass::IsHighFlying @ 0x005F6B90` —
    // altitude, not `AircraftTypes` membership — so a parked aircraft would
    // arm the ground primary instead.
    sim.substrate
        .entities
        .get_mut(air)
        .and_then(|entity| entity.locomotor.as_mut())
        .expect("AIR carries a locomotor")
        .altitude = crate::util::fixed_math::SimFixed::from_num(
        crate::util::lepton::HIGH_FLIGHT_THRESHOLD_LEPTONS,
    );
    assert!(issue_attack_command(
        &mut sim.substrate.entities,
        tower,
        air,
        Some(&rules),
        &sim.interner,
    ));
    let mut main_rng = SimRng::new(2);
    let arm = combat_visit(&mut sim, &rules, &mut main_rng, 1);
    assert!(arm.consequences.fire_events().is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(tower)
            .unwrap()
            .pending_building_fire
            .unwrap()
            .weapon_slot,
        WeaponSlot::Secondary
    );

    let building = sim.substrate.entities.get_mut(tower).unwrap();
    building.attack_target.as_mut().unwrap().target = TargetKind::Entity(ground);
    building
        .pending_building_fire
        .as_mut()
        .unwrap()
        .remaining_ticks = 1;
    let expiry = combat_visit(&mut sim, &rules, &mut main_rng, 2);

    assert_eq!(expiry.consequences.fire_events().len(), 1);
    assert_eq!(
        expiry.consequences.fire_events()[0].weapon_slot,
        WeaponSlot::Secondary
    );
    assert_eq!(sim.substrate.entities.get(air).unwrap().health.current, 100);
    assert_eq!(
        sim.substrate.entities.get(ground).unwrap().health.current,
        70
    );
}

#[test]
fn gsi_05_10_expiry_error_clears_without_retarget_or_shot() {
    let rules = delayed_building_rules();
    let mut sim = Simulation::new();
    let tower = spawn(&mut sim, &rules, "TESLA", "Soviet", 5, 5);
    let target = spawn(&mut sim, &rules, "GROUND", "Allies", 7, 5);
    let alternative = spawn(&mut sim, &rules, "GROUND", "Allies", 6, 5);
    assert!(issue_attack_command(
        &mut sim.substrate.entities,
        tower,
        target,
        Some(&rules),
        &sim.interner,
    ));
    let mut main_rng = SimRng::new(3);
    let _ = combat_visit(&mut sim, &rules, &mut main_rng, 1);

    let building = sim.substrate.entities.get_mut(tower).unwrap();
    building.attack_target.as_mut().unwrap().target = TargetKind::Entity(99_999);
    building
        .pending_building_fire
        .as_mut()
        .unwrap()
        .remaining_ticks = 1;
    let expiry = combat_visit(&mut sim, &rules, &mut main_rng, 2);

    assert!(expiry.consequences.fire_events().is_empty());
    let building = sim.substrate.entities.get(tower).unwrap();
    assert!(building.pending_building_fire.is_none());
    assert_eq!(
        building.attack_target.as_ref().unwrap().target,
        TargetKind::Entity(99_999),
        "expiry must neither reacquire the nearby alternative nor drop the live target field"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(alternative)
            .unwrap()
            .health
            .current,
        100
    );
}

#[test]
fn gsi_05_10_non_delayed_and_prism_type_bypass_fire_immediately() {
    let rules = delayed_building_rules();
    assert_eq!(rules.general.prism_type.as_deref(), Some("ATESLA"));
    assert!(
        rules
            .art_registry
            .get("GAPRIS")
            .is_some_and(|art| art.is_anim_delayed_fire)
    );

    let mut sim = Simulation::new();
    let ordinary = spawn(&mut sim, &rules, "NODELAY", "Soviet", 5, 5);
    let prism = spawn(&mut sim, &rules, "ATESLA", "Allies", 5, 8);
    let ordinary_target = spawn(&mut sim, &rules, "GROUND", "Allies", 7, 5);
    let prism_target = spawn(&mut sim, &rules, "GROUND", "Soviet", 7, 8);
    assert!(issue_attack_command(
        &mut sim.substrate.entities,
        ordinary,
        ordinary_target,
        Some(&rules),
        &sim.interner,
    ));
    assert!(issue_attack_command(
        &mut sim.substrate.entities,
        prism,
        prism_target,
        Some(&rules),
        &sim.interner,
    ));
    let result = combat_visit(&mut sim, &rules, &mut SimRng::new(4), 1);

    assert_eq!(result.consequences.fire_events().len(), 2);
    assert!(
        sim.substrate
            .entities
            .get(ordinary)
            .unwrap()
            .pending_building_fire
            .is_none()
    );
    assert!(
        sim.substrate
            .entities
            .get(prism)
            .unwrap()
            .pending_building_fire
            .is_none()
    );
}

#[test]
fn gsi_05_10_delays_at_or_below_one_expire_on_the_arming_visit() {
    for delay in [1, 0, -7] {
        let rules = delayed_building_rules_with_delay(delay);
        let mut sim = Simulation::new();
        let tower = spawn(&mut sim, &rules, "TESLA", "Soviet", 5, 5);
        let target = spawn(&mut sim, &rules, "GROUND", "Allies", 7, 5);
        assert!(issue_attack_command(
            &mut sim.substrate.entities,
            tower,
            target,
            Some(&rules),
            &sim.interner,
        ));

        let result = combat_visit(&mut sim, &rules, &mut SimRng::new(5), 1);
        assert_eq!(result.consequences.fire_events().len(), 1, "delay {delay}");
        assert!(
            sim.substrate
                .entities
                .get(tower)
                .unwrap()
                .pending_building_fire
                .is_none(),
            "delay {delay} clamps to zero and clears on the arming visit"
        );
    }
}
