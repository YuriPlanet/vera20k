//! Experience, rank abilities and damage multipliers on the fire path: elite
//! ROF and firepower, bunker and garrison multipliers, kill credit and rearm
//! frames.

use super::*;

/// A Grizzly (`Cost=700`) killing rookie Rhinos (`Cost=900`) earns 900/(700*3)
/// per kill through the real damage path, so it wears a chevron on kill 3 and
/// goes elite on kill 5 — `TechnoClass::Record_The_Kill @ 0x00702D40` feeding
/// `VeterancyClass::Add @ 0x0074FF50`.
#[test]
fn gsi_08_12_a_grizzly_promotes_through_the_damage_path() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n[HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n[E1]\nStrength=125\nArmor=flak\nSpeed=4\nCost=200\nPrimary=M60\n[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n[SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[General]\nVeteranRatio=3.0\nVeteranCap=2\n",
    ))
    .expect("veterancy fixture parses");

    // Guard the fixture itself: the award divides by the killer's cost and
    // multiplies by the victim's, so a mistyped section silently yields zero.
    assert_eq!(rules.object("HTNK").map(|o| o.cost), Some(900));
    assert_eq!(rules.object("MTNK").map(|o| o.cost), Some(700));
    let mut ranks = Vec::new();
    let mut killer = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
    killer.set_owner_for_test(test_intern("Soviet"));
    let mut store = EntityStore::new();
    store.insert(killer);
    // Intern the victim type before snapshotting the thread-local test
    // interner, or the award cannot resolve its `Cost=`.
    let _ = test_intern("HTNK");
    let mut interner = test_interner();
    let mut scenario_rng = SimRng::new(9);

    for victim_id in 2..=6u64 {
        store.insert(make_entity_owned(victim_id, "HTNK", 8, 5, 1, "Americans"));
        install_entity_attack_target_for_test(&mut store, 1, victim_id);
        // Each victim is a fresh shot: clear the reload the last kill armed.
        if let Some(attacker) = store.get_mut(1) {
            attacker
                .seldom
                .rearm_timer
                .set_for_test(crate::sim::timer::CdTimer::default());
        }
        align_attackers_to_targets(&mut store);
        tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);
        store.remove(victim_id);
        ranks.push(store.get(1).expect("killer").veterancy.rank());
    }

    assert_eq!(ranks, vec![0, 0, 100, 100, 200], "ranks after kills 1..5");
}

/// An elite Grizzly with the stock `ROF,FIREPOWER` grants fires through the
/// production path with `ftol(65 * VeteranCombat 1.1) = 71` damage
/// (`Fire_At @ 0x006FE3C8`) and reloads in `ftol((50 + jitter) * VeteranROF
/// 0.6)` — 30 or 31 frames (`GetROF @ 0x006FD136`); a rookie of the same type
/// keeps 65 and 50..=52.
#[test]
fn gsi_08_05_elite_rof_and_firepower_abilities_reach_the_fire_path() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\nVeteranAbilities=STRONGER,FIREPOWER,ROF\nEliteAbilities=SELF_HEAL,FASTER\n\
         [HTNK]\nStrength=400\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\nVeteranCombat=1.1\nVeteranROF=0.6\n",
    ))
    .expect("elite-ability fixture parses");

    // (damage dealt, reload the shot armed)
    let fire_once = |elite: bool| -> (i32, u16) {
        let mut store = EntityStore::new();
        let mut firer = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
        if elite {
            firer.veterancy.set_elite();
        }
        store.insert(firer);
        let _ = test_intern("HTNK");
        store.insert(make_entity_owned(2, "HTNK", 8, 5, 400, "Americans"));
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 2);
        align_attackers_to_targets(&mut store);
        tick(
            &mut store,
            &rules,
            &mut interner,
            0,
            &mut SimRng::new(0x475A_5A4C),
        );
        let dealt = 400 - store.get(2).expect("target").health.current();
        let reload = store
            .get(1)
            .map(|attacker| attacker.seldom.rearm_timer.duration() as u16)
            .expect("the shot armed a reload");
        (dealt, reload)
    };

    let (rookie_damage, rookie_rof) = fire_once(false);
    assert_eq!(rookie_damage, 65);
    assert!(
        (50..=52).contains(&rookie_rof),
        "rookie reload {rookie_rof}"
    );
    let (elite_damage, elite_rof) = fire_once(true);
    assert_eq!(elite_damage, 71, "ftol(65 * 1.1)");
    assert!((30..=31).contains(&elite_rof), "elite reload {elite_rof}");
}

/// A heal is never rank-scaled: `Fire_At`'s `JLE @ 0x006FE331` skips the
/// firepower fold and the FIREPOWER stage for damage <= 0, so an elite repairer
/// with FIREPOWER restores the bare 50, not `ftol(-50 * 1.1)`.
#[test]
fn gsi_08_05_a_heal_skips_the_firepower_rank_stage() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=Repair\nVeteranAbilities=STRONGER,FIREPOWER,ROF\n\
         [HTNK]\nStrength=400\nArmor=heavy\nSpeed=4\nCost=900\n\
         [Repair]\nDamage=-50\nROF=50\nRange=6\nWarhead=Mech\n\
         [Mech]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\nVeteranCombat=1.1\n",
    ))
    .expect("heal fixture parses");
    let heal_once = |elite: bool| -> i32 {
        let mut store = EntityStore::new();
        let mut firer = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
        if elite {
            firer.veterancy.set_elite();
        }
        store.insert(firer);
        let _ = test_intern("HTNK");
        store.insert(make_entity_owned(2, "HTNK", 8, 5, 200, "Soviet"));
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 2);
        align_attackers_to_targets(&mut store);
        tick(
            &mut store,
            &rules,
            &mut interner,
            0,
            &mut SimRng::new(0x475A_5A4C),
        );
        store.get(2).expect("patient").health.current() - 200
    };
    assert_eq!(heal_once(false), 50);
    assert_eq!(heal_once(true), 50, "not ftol(-50 * 1.1) = -55");
}

/// An `IsSonic=` weapon's shot carries no damage: `Fire_At` zeroes it
/// (`0x006FE306..0x006FE32A`) and the Sonic wave hurts instead (its
/// `AmbientDamage=`). Before, the Dolphin's shot also hit for its `Damage=4`.
#[test]
fn gsi_08_05_a_sonic_shot_carries_no_damage() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n1=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nPrimary=SonicZap\n\n\
         [TARGET]\nStrength=100\nArmor=wood\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange=6\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [SonicWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("Sonic fixture parses");
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "DLPH", 5, 5, 200, "Soviet"));
    let _ = test_intern("TARGET");
    store.insert(make_entity_owned(2, "TARGET", 8, 5, 100, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);
    let result = tick(
        &mut store,
        &rules,
        &mut interner,
        0,
        &mut SimRng::new(0x475A_5A4C),
    );
    assert_eq!(
        result.consequences.fire_events().len(),
        1,
        "the Dolphin fired"
    );
    assert_eq!(store.get(2).expect("target").health.current(), 100);
}

/// Ares `ApplyDamage=yes` (TechnoClass_Fire_AllowDamage 1004A370, hook
/// 0x6FE31C) keeps the Sonic shot's `Damage=`.
#[test]
fn ares_apply_damage_keeps_a_sonic_shots_damage() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n1=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nPrimary=SonicZap\n\n\
         [TARGET]\nStrength=100\nArmor=wood\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange=6\nWarhead=SonicWH\nIsSonic=yes\n\
         ApplyDamage=yes\n\n\
         [SonicWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("Sonic fixture parses");
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "DLPH", 5, 5, 200, "Soviet"));
    let _ = test_intern("TARGET");
    store.insert(make_entity_owned(2, "TARGET", 8, 5, 100, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);
    tick(
        &mut store,
        &rules,
        &mut interner,
        0,
        &mut SimRng::new(0x475A_5A4C),
    );
    assert_eq!(store.get(2).expect("target").health.current(), 96);
}

/// A vehicle installed in a Tank Bunker fires for `ftol(damage * f32
/// BunkerDamageMultiplier)` (`0x006FE40B..0x006FE437`, a `+0x2E4` link on a
/// non-building): the retail 1.3 turns a 90-damage gun into 116 (the single
/// 1.3 sits just below 1.3, so 117 is never reached).
#[test]
fn gsi_08_05_a_bunkered_vehicle_takes_the_bunker_damage_multiplier() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HTNK\n1=MTNK\n\
         [HTNK]\nStrength=300\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=RhinoGun\n\
         [MTNK]\nStrength=400\nArmor=heavy\nSpeed=6\nCost=700\n\
         [RhinoGun]\nDamage=90\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nBunkerDamageMultiplier=1.3\n",
    ))
    .expect("bunker fixture parses");
    let fire_once = |bunkered: bool| -> i32 {
        let mut store = EntityStore::new();
        let mut firer = make_entity_owned(1, "HTNK", 5, 5, 300, "Soviet");
        if bunkered {
            firer.bunker.set_bunker_link_for_test(
                crate::sim::docking::bunker_link::BunkerLink::Installed(99),
            );
        }
        store.insert(firer);
        let _ = test_intern("MTNK");
        store.insert(make_entity_owned(2, "MTNK", 8, 5, 400, "Americans"));
        let mut interner = test_interner();
        install_entity_attack_target_for_test(&mut store, 1, 2);
        align_attackers_to_targets(&mut store);
        tick(
            &mut store,
            &rules,
            &mut interner,
            0,
            &mut SimRng::new(0x475A_5A4C),
        );
        400 - store.get(2).expect("target").health.current()
    };
    assert_eq!(fire_once(false), 90);
    assert_eq!(fire_once(true), 116, "ftol(90 * 1.3f)");
}

/// A garrison's shot is multiplied by the f32 `OccupyDamageMultiplier=` on the
/// x87 (`FILD; FMUL dword Rules+0xF40; ftol`, `0x006FE3F1`): a 30-damage
/// weapon under the retail 1.2 carries 36. VERA's former fixed-point 1.2
/// (`78643/65536`, below the single) truncated it to 35. Native value:
/// `tools/spatial_oracle/damage_build.py` (fire rows, damage 30, occupied).
#[test]
fn gsi_08_05_a_garrison_shot_takes_the_f32_occupy_multiplier() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=CAGAS\n\n\
[CombatDamage]\nOccupyDamageMultiplier=1.2\n\n\
[CAGAS]\nStrength=800\nArmor=wood\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=UCPara\nOccupyWeapon=UCPara\n\n\
[E2]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
[UCPara]\nDamage=30\nROF=20\nRange=5\nWarhead=SA\n\n\
[SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n",
    ))
    .expect("garrison rules parse");
    let mut store = EntityStore::new();
    let mut building = make_entity(10, "CAGAS", 5, 5, 800);
    building.set_category_for_test(EntityCategory::Structure);
    building
        .passenger_role_mut_for_test()
        .occupants_mut()
        .add_for_test(1);
    store.insert(building);
    let mut occupant = make_infantry_entity(1, "E1", 5, 5, 125);
    occupant.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::inside_for_test(
        10, false,
    ));
    store.insert(occupant);
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 10, 2);
    let result = tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));
    assert_eq!(
        result.consequences.fire_events().len(),
        1,
        "the garrison fired"
    );
    assert_eq!(store.get(2).expect("target").health.current(), 125 - 36);
}

/// `Record_The_Kill @ 0x00702D40` reads the VICTIM type's `DontScore=` byte
/// (`+0xC9F`) at 0x00702E4E and returns before the multiplier, before the
/// accumulator and before the score add. Stock marks the V3, Dreadnought and
/// Boomer missiles that way, and they are shot down in most matches — without
/// the gate every interception promotes the interceptor.
#[test]
fn gsi_08_12_a_dont_score_victim_pays_no_experience() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n[HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\nDontScore=yes\n[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[General]\nVeteranRatio=3.0\nVeteranCap=2\n",
    ))
    .expect("dont-score fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    let mut interner = test_interner();
    let mut scenario_rng = SimRng::new(11);

    for victim_id in 2..=6u64 {
        let mut victim = make_entity_owned(victim_id, "HTNK", 8, 5, 1, "Americans");
        victim
            .tracking_facts_mut_for_test()
            .set_dont_score_for_test(true);
        store.insert(victim);
        install_entity_attack_target_for_test(&mut store, 1, victim_id);
        // Each victim is a fresh shot: clear the reload the last kill armed.
        if let Some(attacker) = store.get_mut(1) {
            attacker
                .seldom
                .rearm_timer
                .set_for_test(crate::sim::timer::CdTimer::default());
        }
        tick(&mut store, &rules, &mut interner, 0, &mut scenario_rng);
        store.remove(victim_id);
    }

    let killer = store.get(1).expect("killer");
    assert_eq!(
        killer.veterancy.rank(),
        0,
        "five DontScore kills earn nothing"
    );
    assert_eq!(killer.veterancy.raw().bits(), 0);
}

/// A garrison's kill promotes an occupant, never the building. A BuildingType
/// is untrainable by default (constructor `0x0045E42E`), so `Record_The_Kill`
/// takes its occupied-building arm (`0x00702F98..0x00702FEA`) and pays the
/// occupant at the building's fire index. FireAt has already moved that
/// index past the shooter (`0x006FF031..0x006FF085`) when the shot kills, so
/// with two GIs inside the one next in line is paid for the shooter's kill.
#[test]
fn gsi_08_12_a_garrison_kill_pays_the_occupant_next_in_line() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=CAGAS\n\n\
[General]\nVeteranRatio=3.0\nVeteranCap=2\n\n\
[CAGAS]\nStrength=800\nArmor=wood\nCost=500\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nCost=200\nPrimary=M60\nOccupyWeapon=M60\n\n\
[E2]\nStrength=1\nArmor=flak\nSpeed=4\nCost=300\n\n\
[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
[SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n",
    ))
    .expect("garrison kill fixture parses");
    assert!(!rules.object("CAGAS").unwrap().trainable);
    let mut store = EntityStore::new();
    let mut building = make_entity_owned(10, "CAGAS", 5, 5, 800, "Soviet");
    building.set_category_for_test(EntityCategory::Structure);
    // Preserve the former supplied Cargo prepend order as occupant input.
    building
        .passenger_role_mut_for_test()
        .occupants_mut()
        .set_ids_for_test(vec![3, 1]);
    store.insert(building);
    for id in [1, 3] {
        let mut occupant = make_infantry_entity(id, "E1", 5, 5, 125);
        occupant.set_owner_for_test(test_intern("Soviet"));
        occupant.set_passenger_role_for_test(
            crate::sim::passenger::PassengerRole::inside_for_test(10, false),
        );
        store.insert(occupant);
    }
    let mut victim = make_infantry_entity(2, "E2", 8, 5, 1);
    victim.set_owner_for_test(test_intern("Americans"));
    store.insert(victim);
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 10, 2);
    // The occupant at fire index 0 shoots; the one after it is next in line.
    let (shooter, next) = {
        let occupants = store.get(10).unwrap().passenger_role().occupants();
        assert_eq!(occupants.fire_index(), 0);
        (occupants.ids()[0], occupants.ids()[1])
    };
    tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert_eq!(store.get(2).map_or(0, |victim| victim.health.current()), 0);
    let raw = |id: u64| store.get(id).unwrap().veterancy.raw().bits();
    assert_eq!(raw(10), 0, "the building never ranks");
    assert_eq!(raw(shooter), 0, "the shooter is not the one paid");
    assert_ne!(raw(next), 0, "the occupant next in line is");
}

/// A garrison's rearm is the NEXT occupant's: FireAt advances the firing
/// occupant (`0x006FF031..0x006FF085`) before `GetROF` (`0x006FF289`), which
/// takes the building's weapon through `BuildingClass::GetWeapon @
/// 0x004526F0` at the new index. A quick shooter followed by a slow one
/// therefore reloads slowly.
#[test]
fn gsi_08_05_a_mixed_garrison_rearms_with_the_next_occupants_weapon() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n1=E2\n2=E3\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=CAGAS\n\n\
[CAGAS]\nStrength=800\nArmor=wood\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=FastGun\nOccupyWeapon=FastGun\n\n\
[E2]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=SlowGun\nOccupyWeapon=SlowGun\n\n\
[E3]\nStrength=400\nArmor=flak\nSpeed=4\n\n\
[FastGun]\nDamage=5\nROF=10\nRange=5\nWarhead=SA\n\n\
[SlowGun]\nDamage=5\nROF=120\nRange=5\nWarhead=SA\n\n\
[SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("mixed garrison fixture parses");
    let mut store = EntityStore::new();
    let mut building = make_entity_owned(10, "CAGAS", 5, 5, 800, "Soviet");
    building.set_category_for_test(EntityCategory::Structure);
    building
        .passenger_role_mut_for_test()
        .occupants_mut()
        .set_ids_for_test(vec![3, 1]);
    store.insert(building);
    // Whichever occupant sits at index 0 shoots with the quick gun; the one
    // after it carries the slow gun.
    let (shooter, next) = {
        let occupants = store.get(10).unwrap().passenger_role().occupants();
        (occupants.ids()[0], occupants.ids()[1])
    };
    for (id, kind) in [(shooter, "E1"), (next, "E2")] {
        let mut occupant = make_infantry_entity(id, kind, 5, 5, 125);
        occupant.set_owner_for_test(test_intern("Soviet"));
        occupant.set_passenger_role_for_test(
            crate::sim::passenger::PassengerRole::inside_for_test(10, false),
        );
        store.insert(occupant);
    }
    let mut victim = make_infantry_entity(2, "E3", 8, 5, 400);
    victim.set_owner_for_test(test_intern("Americans"));
    store.insert(victim);
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 10, 2);
    tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));

    assert!(
        store.get(2).unwrap().health.current() < 400,
        "the quick gun fired"
    );
    let building = store.get(10).unwrap();
    assert_eq!(
        building.passenger_role().occupants().fire_index(),
        1,
        "the turn passed to the next occupant"
    );
    // SlowGun's 120 over two occupants is about 60 frames; FastGun's 10 would
    // be about 5.
    let rearm = building.seldom.rearm_timer.duration();
    assert!(rearm > 40, "rearm {rearm} follows the slow gun");
}

/// A base defence never ranks: a BuildingType is untrainable unless its
/// section says `Trainable=yes` (constructor `0x0045E42E`), so its kills pay
/// nobody (`Record_The_Kill`, `0x00702EF5..0x00702FF5`).
#[test]
fn gsi_08_12_a_base_defence_kill_pays_nobody() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=PILL\n[VehicleTypes]\n0=HTNK\n\
         [PILL]\nStrength=400\nArmor=concrete\nCost=500\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\nVeteranRatio=3.0\nVeteranCap=2\n",
    ))
    .expect("defence fixture parses");
    assert!(!rules.object("PILL").unwrap().trainable);
    let mut store = EntityStore::new();
    store.insert(make_structure_entity(1, "PILL", 5, 5, 400, 400));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);
    align_attackers_to_targets(&mut store);
    tick(&mut store, &rules, &mut interner, 0, &mut SimRng::new(1));
    assert_eq!(store.get(2).map_or(0, |victim| victim.health.current()), 0);
    assert_eq!(store.get(1).unwrap().veterancy.raw().bits(), 0);
}

/// `TechnoClass::FireAt 0x006FF28F..0x006FF2BE`: the duration stored from
/// every GetROF value the oracle ran (`tools/spatial_oracle/rearm_timer.py`,
/// `fire` rows, int32 extremes included) is `fireat_rearm_frames` of it,
/// halved toward zero for a berserk firer. (The rows also record the start
/// frame and the `+0x2F8` copy, which VERA does not keep.)
#[test]
fn gsi_08_05_rearm_frames_match_the_original() {
    let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/rearm_timer.json",
    ))
    .unwrap();
    let rows = vectors["fire"].as_array().unwrap();
    assert_eq!(rows.len(), 89);
    for row in rows {
        let field = |name: &str| row["input"][name].as_i64().unwrap() as i32;
        assert_eq!(
            i64::from(super::world_fire::fireat_rearm_frames(
                field("rof"),
                field("berserk") != 0
            )),
            row["duration"].as_i64().unwrap(),
            "{row}"
        );
    }
}

/// Ares TechnoClass_Fire_BerserkROFMultiplier (Ares.dll 1004A3D0, hook
/// 0x6FF28F): a berserk firer's rearm is its GetROF value times the type's
/// `Berserk.ROFMultiplier`, else `[CombatDamage] BerserkROFMultiplier`,
/// truncated toward zero.
#[test]
fn ares_berserk_rearm_takes_the_type_or_rules_multiplier() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nBerserkROFMultiplier=0.75\n[VehicleTypes]\n0=OWN\n1=RULES\n\
         [OWN]\nBerserk.ROFMultiplier=0.25\n[RULES]\nStrength=1\n",
    ))
    .unwrap();
    let global = rules.ares_techno_combat().berserk_rof_multiplier();
    let rearm = |name: &str, rof: i32| {
        let multiplier = rules
            .object(name)
            .unwrap()
            .techno_combat()
            .combat()
            .berserk_rof_multiplier(global);
        super::world_fire::ares_berserk_rearm_frames(rof, Some(multiplier))
    };
    assert_eq!(
        (rearm("OWN", 101), rearm("RULES", 101), rearm("RULES", -101)),
        (25, 75, -75)
    );
    assert_eq!(super::world_fire::ares_berserk_rearm_frames(101, None), 101);
}
