//! Focused extended-IFV production reader and selection regressions.
use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{NativeRulesRegistryState, RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;

fn art() -> IniFile {
    IniFile::from_str(
        "[CARRIER]\nVoxel=yes\nWeapon1FLH=0,0,11\nWeapon19FLH=$0 ,0,49h\nEliteWeapon19FLH=0,0,99\nWeapon20FLH=0,0,80\n[NEW]\nVoxel=yes\n",
    )
}

fn fixture(patches: &[&str]) -> (RuleSet, NativeRulesRegistryState) {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=CARRIER\n[CARRIER]\nGunner=yes\nTurret=yes\nTurretCount=20\nWeaponCount=20\n\
         Weapon1=BASE\nWeapon19=EXTENDED\nEliteWeapon19=ELITE\nWeapon20=TAIL\n\
         RepairTurretWeapon=18\nRepairTurretIndex=3\nWeaponTurretIndex19=18\nWeaponTurretIndex20=19\n\
         WeaponUIName1=Name:First\nWeaponUIName19=Name:Extended\nWeaponUIName20=Name:Last\n\
         [BASE]\nDamage=1\n[EXTENDED]\nDamage=2\n[ELITE]\nDamage=3\n[TAIL]\nDamage=4\n",
    ));
    for patch in patches {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(patch));
    }
    let processed = layers.process_with_fixed_art(&art()).unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art()));
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    (rules, trace.into_registry_state_discarding_events())
}

#[cfg(feature = "ares")]
#[test]
fn extended_ifv_selects_weapon_model_and_normal_elite_muzzle() {
    // Ares10040020/1003DBC0 -> SwitchGunner10042480 -> GetFLH6F3AD0.
    let (rules, _) = fixture(&[]);
    let object = rules.object("CARRIER").unwrap();
    assert!(object.gunner);
    let mut actor =
        crate::sim::game_entity::GameEntity::test_default(1, "CARRIER", "Americans", 10, 10);
    actor.set_gunner_weapon(18, object);
    assert_eq!(
        (actor.current_weapon_number(), actor.current_turret_index()),
        (18, 18)
    );
    let world = crate::sim::world::Simulation::new();
    let mut source = crate::sim::combat::fire_coord::FireSource::of_entity(&actor);
    for (rank, name, height) in [(0, "EXTENDED", 73), (200, "ELITE", 99)] {
        source.veterancy = rank;
        assert_eq!(
            crate::sim::combat::combat_weapon::weapon_for_index(object, rank, 18)
                .unwrap()
                .0,
            name
        );
        let fire = crate::sim::combat::fire_coord::fire_coordinate(
            &world,
            &rules,
            &source,
            object,
            18,
            0,
            Default::default(),
        );
        assert_eq!(fire.coord.z - fire.source_z, height);
    }
    assert_eq!(
        object.gunner_turrets.weapon_ui_name(18),
        Some("Name:Extended")
    );
    actor.set_gunner_weapon(20, object);
    assert_eq!(actor.current_weapon_number(), 0);
}

#[cfg(feature = "ares")]
#[test]
fn extended_ifv_resize_retains_base_and_discards_extra_tail_and_labels() {
    // Ares resize10040AF0 and gunner reader1003FE10 have distinct pointer,
    // mapping and label lifetimes. Missing Gunner must retain its earlier byte.
    let (rules, state) = fixture(&[
        "[CARRIER]\nImage=NEW\nWeaponCount=19\n",
        "[CARRIER]\nWeaponCount=20\n",
    ]);
    let object = rules.object("CARRIER").unwrap();
    assert!(object.gunner);
    assert_eq!(object.weapon_at(18), Some("EXTENDED"));
    assert_eq!(object.weapon_at(19), None);
    assert_eq!(object.weapon_flh(19, false), Default::default());
    assert_eq!(object.gunner_turrets.turret_for_weapon(18), 18);
    assert_eq!(object.gunner_turrets.turret_for_weapon(19), -1);
    assert_eq!(object.gunner_turrets.weapon_ui_name(0), Some("Name:First"));
    assert_eq!(object.gunner_turrets.weapon_ui_name(18), None);
    let saved = crate::util::save_codec::serialize(&state).unwrap();
    let restored: NativeRulesRegistryState = crate::util::save_codec::deserialize(&saved).unwrap();
    let processed = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&art(), restored)
        .unwrap();
    let restored = RuleSet::from_processed_rules(&processed).unwrap();
    let restored = restored.object("CARRIER").unwrap();
    assert_eq!(restored.gunner_turrets, object.gunner_turrets);
    assert_eq!(restored.weapon_list, object.weapon_list);
    assert_eq!(restored.weapon_flh(18, false), object.weapon_flh(18, false));
}

#[cfg(not(feature = "ares"))]
#[test]
fn original_build_keeps_extension_keys_and_slots_inactive() {
    let (rules, _) = fixture(&[]);
    let object = rules.object("CARRIER").unwrap();
    assert!(object.gunner);
    assert_eq!(object.weapon_record_count(), WEAPON_SLOT_COUNT);
    assert_eq!(object.weapon_at(18), None);
    assert_eq!(object.gunner_turrets.turret_for_weapon(18), -1);
    let mut actor =
        crate::sim::game_entity::GameEntity::test_default(1, "CARRIER", "Americans", 10, 10);
    actor.set_gunner_weapon(18, object);
    assert_eq!(actor.current_weapon_number(), 0);
}
