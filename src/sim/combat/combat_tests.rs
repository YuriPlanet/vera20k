//! Tests for the combat system — weapon firing, damage, and entity death.
//!
//! Extracted from combat.rs to keep it under the 400-line limit.

use std::collections::BTreeMap;

use super::*;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::{Animation, SequenceKind};
use crate::sim::components::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, test_intern, test_interner};
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::power_system::PowerState;
use crate::sim::projectile::ProjectileDetonationReason;
use crate::sim::rng::SimRng;
use crate::sim::vision::FogState;

#[path = "infantry_fire_facing_tests.rs"]
mod infantry_fire_facing_tests;

/// Establish the supplied fixture's Cell lists through shared Mark, in
/// ascending fixture ID order. Acquisition no longer synthesizes these lists.
fn mark_fixture_entities(store: &mut EntityStore) -> OccupancyGrid {
    let mut sim = crate::sim::world::Simulation::new();
    sim.interner = test_interner();
    sim.substrate.entities = std::mem::take(store);
    let ids: Vec<_> = sim
        .substrate
        .entities
        .values()
        .map(GameEntity::stable_id)
        .collect();
    for id in ids {
        sim.add_entity_occupancy(id);
    }
    *store = sim.substrate.entities;
    sim.substrate.occupancy
}

/// Build a minimal RuleSet for combat testing.
fn test_rules() -> RuleSet {
    let ini_str: &str = "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\n\n\
[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
[SA]\nTiberium=yes\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
[AP]\nTiberium=yes\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n";
    let ini: IniFile = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("test rules should parse")
}

#[test]
fn sonic_active_wave_gate_precedes_target_resolution_and_all_shot_work() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n1=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nPrimary=SonicZap\n\n\
         [TARGET]\nStrength=100\nArmor=wood\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange=6\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [SonicWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("Sonic whole-shot gate fixture");
    let mut entities = EntityStore::new();
    let mut firer = GameEntity::test_default(1, "DLPH", "Americans", 4, 5);
    firer.attack_target = Some(AttackTarget::new(999));
    firer.current_weapon_index = 1;
    entities.insert(firer);
    let mut interner = test_interner();
    let snap = build_attacker_snapshot(
        entities.get(1).expect("Dolphin"),
        TargetKind::Entity(999),
        None,
        None,
        None,
    );
    let mut rng = SimRng::new(0x50_4e_49_43);
    let rng_before = rng.logical_state();
    let rearm_before = entities.get(1).unwrap().rearm_timer;
    let mut hooks: Option<&mut FixtureTrace> = None;
    let mut emit = CombatEmit::default();

    resolve_attacker_fire(
        &snap,
        &mut entities,
        &rules,
        &mut interner,
        None,
        None,
        &OccupancyGrid::new(),
        None,
        None,
        None,
        None,
        None,
        false,
        17,
        67,
        true,
        &mut rng,
        None,
        &mut hooks,
        &mut emit,
    );

    assert!(emit.fire_events.is_empty());
    assert!(emit.damage_events.is_empty());
    assert!(emit.projectile_spawns.is_empty());
    assert!(emit.current_weapon_updates.is_empty());
    assert_eq!(entities.get(1).unwrap().rearm_timer, rearm_before);
    assert!(emit.remove_attack.is_empty());
    assert_eq!(rng.logical_state(), rng_before);
    assert_eq!(entities.get(1).unwrap().current_weapon_index, 1);
}

#[test]
fn gsi_04_11_tiberium_prelude_gates_and_signed_large_quotient() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=TIBWH\n1=PLAINWH\n\
         [OverlayTypes]\n0=DEFAULTORE\n1=CHAINORE\n2=CHAINPLAIN\n\
         [TIBWH]\nTiberium=yes\n\
         [PLAINWH]\nTiberium=no\n\
         [DEFAULTORE]\nTiberium=yes\n\
         [CHAINORE]\nTiberium=yes\nChainReaction=yes\n\
         [CHAINPLAIN]\nTiberium=no\nChainReaction=yes\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("tiberium prelude gate rules");
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut overlay = OverlayGrid::new(3, 1);
    overlay.place_overlay(0, 0, registry.id_for_name("DEFAULTORE").unwrap(), 0);
    overlay.place_overlay(1, 0, registry.id_for_name("CHAINORE").unwrap(), 0);
    overlay.place_overlay(2, 0, registry.id_for_name("CHAINPLAIN").unwrap(), 0);

    assert!(!combat_aoe::tiberium_reduction_cell_admitted(
        Some(&overlay),
        Some(&registry),
        0,
        0
    ));
    assert!(combat_aoe::tiberium_reduction_cell_admitted(
        Some(&overlay),
        Some(&registry),
        1,
        0
    ));
    assert!(!combat_aoe::tiberium_reduction_cell_admitted(
        Some(&overlay),
        Some(&registry),
        2,
        0
    ));

    let tib = rules.warhead("TIBWH").unwrap();
    let plain = rules.warhead("PLAINWH").unwrap();
    assert_eq!(
        tiberium_reduction_amount(655_360, true, tib),
        Some(65_536),
        "native signed quotient must not wrap through u16"
    );
    assert_eq!(tiberium_reduction_amount(100, true, plain), None);
    assert_eq!(tiberium_reduction_amount(100, false, tib), None);
    assert_eq!(tiberium_reduction_amount(-100, true, tib), None);
}

fn building_damage_state_aoe_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "\
[InfantryTypes]\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[GAPOWR]\nStrength=100\nArmor=wood\n\n\
[105mm]\nDamage=20\nROF=50\nRange=6\nWarhead=AP\n\n\
[AP]\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\n\
[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
    );
    RuleSet::from_ini(&ini).expect("building damage state AoE rules should parse")
}

fn infantry_fire_frame_rules() -> RuleSet {
    let rules_ini: IniFile = IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nImage=GI\nPrimary=M60\nSecondary=Para\nDeployFire=yes\n\n\
[E2]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nReport=GIAttack\nOccupantAnim=UCFLASH\n\n\
[Para]\nDamage=40\nROF=15\nRange=5\nWarhead=AP\nReport=GIAttackDeployed\nOccupantAnim=UCFLASH\n\n\
[SA]\nVerses=100%,100%,100%,90%,70%,0%,100%,25%,25%,0%,0%\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    );
    let mut rules = RuleSet::from_ini(&rules_ini).expect("infantry rules should parse");
    let art_ini = IniFile::from_str(
        "[GI]\nCrawls=yes\nFireUp=2\nFireProne=3\nSecondaryFire=4\nSecondaryProne=5\n",
    );
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.merge_art_data(&art);
    rules
}

fn guardian_gi_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "\
[InfantryTypes]\n0=GGI\n1=E2\n2=ROCK\n\n\
[VehicleTypes]\n0=HTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[General]\nMissileROTVar=.25\n\n\
[GGI]\nStrength=100\nArmor=none\nSpeed=4\nPrimary=M60\nSecondary=MissileLauncher\nDeployFire=yes\n\n\
[E2]\nStrength=125\nArmor=none\nSpeed=4\n\n\
[ROCK]\nStrength=125\nArmor=none\nSpeed=8\nConsideredAircraft=yes\n\n\
[HTNK]\nStrength=400\nArmor=heavy\nSpeed=5\n\n\
[M60]\nDamage=15\nROF=20\nRange=4\nWarhead=SA\nReport=GGIAttack\n\n\
[MissileLauncher]\nDamage=40\nROF=40\nRange=8\nBurst=1\nProjectile=AAHeatSeeker2\nSpeed=30\nWarhead=GUARDWH\nReport=GuardianGIDeployedAttack\nMinimumRange=1\n\n\
[AAHeatSeeker2]\nArm=2\nShadow=no\nProximity=no\nRanged=yes\nAA=yes\nAG=yes\nImage=DRAGON\nROT=60\nSubjectToCliffs=no\nSubjectToElevation=no\nSubjectToWalls=no\n\n\
[SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n\n\
[GUARDWH]\nVerses=20%,20%,20%,100%,50%,100%,10%,10%,10%,100%,100%\n",
    );
    RuleSet::from_ini(&ini).expect("guardian GI rules should parse")
}

/// Point every fixture attacker at its own target.
///
/// `GameEntity::test_default` spawns facing north (0) with no turret, and the
/// fixture rules author no `Turret=`, so every fixture attacker is a TURRETLESS
/// vehicle. Under the native body gate — `UnitClass::GetFireError @ 0x00740FD0`
/// step 17, which compares the HULL `+0x388` when `Turret=` is unset — such a
/// unit spends its first ticks turning through `Fire_At_Target @ 0x00736DF0`
/// case 2 instead of shooting. Tests whose subject is damage, RNG consumption or
/// resolution ordering pre-align here so they still observe the shot on the
/// single tick they run; the turn-to-fire behaviour itself is covered by
/// `combat_turret_facing_tests`.
fn align_attackers_to_targets(
    store: &mut EntityStore,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) {
    for id in store.keys_sorted() {
        let desired = {
            let Some(entity) = store.get(id) else {
                continue;
            };
            let Some(attack) = entity.attack_target.as_ref() else {
                continue;
            };
            match crate::sim::movement::turret::facing_toward_target(
                entity,
                &attack.target,
                store,
                Some(rules),
                interner,
            ) {
                Some(desired) => desired,
                None => continue,
            }
        };
        if let Some(entity) = store.get_mut(id) {
            entity.facing = (desired >> 8) as u8;
            entity.body_facing = None;
            if let Some(ref mut barrel) = entity.barrel_facing {
                barrel.snap(desired, 0);
            }
        }
    }
}

fn make_entity(id: u64, type_ref: &str, rx: u16, ry: u16, hp: i32) -> GameEntity {
    let mut e = GameEntity::test_default(id, type_ref, "Test", rx, ry);
    e.health = Health { current: hp };
    e.lifecycle.in_limbo = false;
    e
}

fn make_entity_owned(
    id: u64,
    type_ref: &str,
    rx: u16,
    ry: u16,
    hp: i32,
    owner: &str,
) -> GameEntity {
    let mut e = GameEntity::test_default(id, type_ref, owner, rx, ry);
    e.health = Health { current: hp };
    e.lifecycle.in_limbo = false;
    e
}

fn gsi_04_05_attack_frame_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=SOURCE\n1=PROTECTED\n\
         [BuildingTypes]\n0=NORMAL\n1=MOD1X1\n2=STOCK2X2\n3=SELFNO\n4=SELFYES\n5=IMMUNE\n6=PROTECTEDBLDG\n\
         [Warheads]\n0=HITWH\n\
         [SOURCE]\nStrength=100\nArmor=heavy\n\
         [PROTECTED]\nStrength=100\nArmor=heavy\nToProtect=yes\n\
         [NORMAL]\nStrength=100\nArmor=wood\nFoundation=2x2\n\
         [MOD1X1]\nStrength=100\nArmor=wood\nUndeploysInto=MODUNIT\nFoundation=1x1\n\
         [STOCK2X2]\nStrength=100\nArmor=wood\nUndeploysInto=MODUNIT\nFoundation=2x2\n\
         [SELFNO]\nStrength=100\nArmor=wood\nFoundation=2x2\nDamageSelf=no\n\
         [SELFYES]\nStrength=100\nArmor=wood\nFoundation=2x2\nDamageSelf=yes\n\
         [IMMUNE]\nStrength=100\nArmor=wood\nFoundation=2x2\nImmune=yes\n\
         [PROTECTEDBLDG]\nStrength=100\nArmor=wood\nFoundation=2x2\nToProtect=yes\n\
         [HITWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("House attack-frame rules parse")
}

#[test]
fn gsi_04_05_building_attack_frame_prelude_obeys_object_and_type_gates() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    for (id, type_name) in [
        (2, "NORMAL"),
        (3, "MOD1X1"),
        (4, "STOCK2X2"),
        (5, "SELFNO"),
        (6, "SELFYES"),
    ] {
        let mut target = make_entity_owned(id, type_name, id as u16 + 3, 4, 100, "Victim");
        target.category = EntityCategory::Structure;
        entities.insert(target);
    }
    let mut unit_target = make_entity_owned(7, "SOURCE", 10, 4, 100, "Victim");
    unit_target.category = EntityCategory::Unit;
    entities.insert(unit_target);
    entities.insert(make_entity_owned(8, "SOURCE", 11, 4, 100, "Victim"));
    let mut dead_target = make_entity_owned(9, "NORMAL", 12, 4, 0, "Victim");
    dead_target.category = EntityCategory::Structure;
    entities.insert(dead_target);

    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = BTreeMap::from([(
        victim_owner,
        HouseState::new(victim_owner, 0, None, false, 0, 10),
    )]);
    let frame = |houses: &BTreeMap<InternedId, HouseState>| {
        houses[&victim_owner]
            .strategy_emergency
            .last_building_attack_frame()
    };

    let event = EntityDamageEvent::area(2, -7, 0, 1, Some(source_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(&event, &entities, &rules, &interner, &mut houses, 41),
        BuildingReceivePrelude::Respond
    );
    assert_eq!(
        frame(&houses),
        41,
        "negative damage still records the frame"
    );

    let event = EntityDamageEvent::area(2, 0, 0, 1, Some(victim_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(&event, &entities, &rules, &interner, &mut houses, 42),
        BuildingReceivePrelude::Respond
    );
    assert_eq!(frame(&houses), 42, "alliance and zero damage are not gates");

    let null_attacker = EntityDamageEvent::area(2, 10, 0, RAD_NO_ATTACKER, Some(source_owner), wh);
    apply_building_receive_prelude(
        &null_attacker,
        &entities,
        &rules,
        &interner,
        &mut houses,
        43,
    );
    assert_eq!(
        frame(&houses),
        42,
        "source House cannot replace a null object"
    );

    let removed_attacker = EntityDamageEvent::area(2, 10, 0, 999, None, wh);
    apply_building_receive_prelude(
        &removed_attacker,
        &entities,
        &rules,
        &interner,
        &mut houses,
        43,
    );
    assert_eq!(
        frame(&houses),
        43,
        "the retained non-null object argument does not require a live lookup"
    );

    let no_source_house = EntityDamageEvent::area(2, 10, 0, 1, None, wh);
    apply_building_receive_prelude(
        &no_source_house,
        &entities,
        &rules,
        &interner,
        &mut houses,
        44,
    );
    assert_eq!(
        frame(&houses),
        44,
        "source House is not required for the write"
    );

    let allied = EntityDamageEvent::area(2, 10, 0, 8, Some(victim_owner), wh);
    apply_building_receive_prelude(&allied, &entities, &rules, &interner, &mut houses, 45);
    assert_eq!(
        frame(&houses),
        45,
        "a distinct attacker owned by the victim House still writes"
    );

    let modded_skip = EntityDamageEvent::area(3, 10, 0, 1, Some(source_owner), wh);
    apply_building_receive_prelude(&modded_skip, &entities, &rules, &interner, &mut houses, 46);
    assert_eq!(
        frame(&houses),
        45,
        "a 1x1 undeployer takes vtable +0x80 skip"
    );

    let stock_shape = EntityDamageEvent::area(4, 10, 0, 1, Some(source_owner), wh);
    apply_building_receive_prelude(&stock_shape, &entities, &rules, &interner, &mut houses, 47);
    assert_eq!(
        frame(&houses),
        47,
        "larger retail-shaped undeployer records"
    );

    let already_dead = EntityDamageEvent::area(9, 10, 0, 1, Some(source_owner), wh);
    apply_building_receive_prelude(&already_dead, &entities, &rules, &interner, &mut houses, 48);
    assert_eq!(frame(&houses), 48, "Health zero is later than this prelude");

    let self_no = EntityDamageEvent::area(5, 10, 0, 5, Some(victim_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(&self_no, &entities, &rules, &interner, &mut houses, 49,),
        BuildingReceivePrelude::ReturnZero
    );
    assert_eq!(frame(&houses), 48);

    let self_yes = EntityDamageEvent::area(6, 10, 0, 6, Some(victim_owner), wh);
    assert_eq!(
        apply_building_receive_prelude(
            &self_yes,
            &entities,
            &rules,
            &interner,
            &mut houses,
            0x1_8000_0001,
        ),
        BuildingReceivePrelude::Respond
    );
    assert_eq!(
        frame(&houses),
        i32::MIN + 1,
        "native stores the raw low dword"
    );

    let unit = EntityDamageEvent::area(7, 10, 0, 1, Some(source_owner), wh);
    apply_building_receive_prelude(&unit, &entities, &rules, &interner, &mut houses, 47);
    assert_eq!(frame(&houses), i32::MIN + 1, "the wrapper is Building-only");
}

#[test]
fn gsi_04_05_building_attack_frame_precedes_immune_receiver_exit() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    let mut target = make_entity_owned(2, "IMMUNE", 5, 4, 100, "Victim");
    target.category = EntityCategory::Structure;
    entities.insert(target);
    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = BTreeMap::from([
        (
            victim_owner,
            HouseState::new(victim_owner, 0, None, false, 0, 10),
        ),
        (
            source_owner,
            HouseState::new(source_owner, 1, None, false, 0, 10),
        ),
    ]);
    let mut occupancy = OccupancyGrid::new();
    let mut main_rng = SimRng::new(5);
    let mut scenario_rng = SimRng::new(7);
    let mut handled_deaths = Vec::new();
    let mut trace_hook = FixtureTrace::default();
    let mut fatal_lifecycle: Option<&mut FixtureTrace> = Some(&mut trace_hook);
    let mut sound_sink = None;

    let _ = commit_damage_events(
        &[EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), wh)],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &[victim_owner, source_owner],
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        77,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );

    assert_eq!(entities.get(2).unwrap().health.current, 100);
    assert_eq!(
        trace_hook.entries,
        vec![BaseDefenseResponseTraceEntry {
            site: BaseDefenseResponseCallSite::BuildingPrelude,
            victim_id: 2,
            health: 100,
            last_attacker_house_index: 1,
        }],
        "Building response runs after the attacker-House index write and before immunity"
    );
    assert_eq!(
        houses[&victim_owner]
            .strategy_emergency
            .last_building_attack_frame(),
        77,
        "the House write happens before ObjectClass immunity rejects damage"
    );
}

#[test]
fn gsi_04_05_protected_techno_response_runs_after_object_health_commit() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "SOURCE", 4, 4, 100, "Enemy"));
    entities.insert(make_entity_owned(2, "PROTECTED", 5, 4, 100, "Victim"));
    let mut protected_building = make_entity_owned(3, "PROTECTEDBLDG", 6, 4, 100, "Victim");
    protected_building.category = EntityCategory::Structure;
    entities.insert(protected_building);
    entities.insert(make_entity_owned(4, "SOURCE", 7, 4, 100, "Victim"));

    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let source_owner = interner.intern("Enemy");
    let wh = interner.intern("HITWH");
    let mut houses = BTreeMap::from([
        (
            victim_owner,
            HouseState::new(victim_owner, 0, None, false, 0, 10),
        ),
        (
            source_owner,
            HouseState::new(source_owner, 1, None, false, 0, 10),
        ),
    ]);
    let mut occupancy = OccupancyGrid::new();
    let mut main_rng = SimRng::new(5);
    let mut scenario_rng = SimRng::new(7);
    let mut handled_deaths = Vec::new();
    let mut trace_hook = FixtureTrace::default();
    let mut inline_hooks: Option<&mut FixtureTrace> = Some(&mut trace_hook);
    let mut sound_sink = None;

    let _ = commit_damage_events(
        &[
            EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(3, 10, 0, 1, Some(source_owner), wh),
            EntityDamageEvent::area(4, 10, 0, 1, Some(source_owner), wh),
        ],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &[victim_owner, source_owner],
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        91,
        &mut inline_hooks,
        &mut sound_sink,
    );

    assert_eq!(
        trace_hook.entries,
        vec![
            BaseDefenseResponseTraceEntry {
                site: BaseDefenseResponseCallSite::ProtectedTechno,
                victim_id: 2,
                health: 90,
                last_attacker_house_index: -1,
            },
            BaseDefenseResponseTraceEntry {
                site: BaseDefenseResponseCallSite::BuildingPrelude,
                victim_id: 3,
                health: 100,
                last_attacker_house_index: 1,
            },
            BaseDefenseResponseTraceEntry {
                site: BaseDefenseResponseCallSite::ProtectedTechno,
                victim_id: 3,
                health: 90,
                last_attacker_house_index: 1,
            },
        ],
        "ToProtect calls once after Object health; Buildings first run their wrapper response"
    );
    assert_eq!(entities.get(2).unwrap().health.current, 90);
    assert_eq!(entities.get(3).unwrap().health.current, 90);
    assert_eq!(entities.get(4).unwrap().health.current, 90);
}

#[test]
fn gsi_04_05_building_self_damage_return_zero_stops_receiver_commit() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut entities = EntityStore::new();
    let mut target = make_entity_owned(5, "SELFNO", 5, 4, 100, "Victim");
    target.category = EntityCategory::Structure;
    entities.insert(target);
    let mut interner = test_interner();
    let victim_owner = interner.intern("Victim");
    let wh = interner.intern("HITWH");
    let mut houses = BTreeMap::from([(
        victim_owner,
        HouseState::new(victim_owner, 0, None, false, 0, 10),
    )]);
    let mut occupancy = OccupancyGrid::new();
    let mut main_rng = SimRng::new(5);
    let mut scenario_rng = SimRng::new(7);
    let mut handled_deaths = Vec::new();
    let mut fatal_lifecycle = None;
    let mut sound_sink = None;

    let _ = commit_damage_events(
        &[EntityDamageEvent::direct_receiver(
            5,
            10,
            0,
            5,
            Some(victim_owner),
            wh,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        )],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &[victim_owner],
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        88,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );

    assert_eq!(entities.get(5).unwrap().health.current, 100);
    assert_eq!(
        houses[&victim_owner]
            .strategy_emergency
            .last_building_attack_frame(),
        0
    );
}

#[test]
fn gsi_04_05_building_attack_frame_remains_live_after_world_receiver_dispatch() {
    let rules = gsi_04_05_attack_frame_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let source_id = sim
        .spawn_object("SOURCE", "Enemy", 4, 4, 0, &rules, &heights)
        .expect("source spawns");
    let target_id = sim
        .spawn_object("NORMAL", "Victim", 6, 4, 0, &rules, &heights)
        .expect("Building target spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner;
    let victim_owner = sim.substrate.entities.get(target_id).unwrap().owner;
    sim.houses.insert(
        source_owner,
        HouseState::new(source_owner, 0, None, false, 0, 10),
    );
    sim.houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 1, None, false, 0, 10),
    );
    sim.session.house_order = vec![source_owner, victim_owner];
    sim.session.binary_frame = 77;
    let warhead = sim.interner.intern("HITWH");

    let event = EntityDamageEvent::area(target_id, 10, 0, source_id, Some(source_owner), warhead);
    sim.commit_noncombat_aoe_hits(&rules, None, &[event, event]);

    assert_eq!(
        sim.houses[&victim_owner]
            .strategy_emergency
            .last_building_attack_frame(),
        77,
        "receiver dispatch writes the world's authoritative House timestamp"
    );
    assert_eq!(
        sim.houses[&victim_owner]
            .strategy_emergency
            .last_attacker_house_index(),
        0,
        "receiver dispatch retains the attacker-House index with the frame stamp"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(target_id)
            .unwrap()
            .health
            .current,
        80,
        "repeated qualifying receiver records in one frame remain admitted"
    );
}

fn make_infantry_entity(id: u64, type_ref: &str, rx: u16, ry: u16, hp: i32) -> GameEntity {
    let mut e = make_entity(id, type_ref, rx, ry, hp);
    e.category = EntityCategory::Infantry;
    e.mission_leaf =
        crate::sim::mission::leaf::MissionLeafState::for_entity_category(EntityCategory::Infantry);
    e.is_voxel = false;
    e.animation = Some(Animation::new(SequenceKind::Stand));
    e.infantry = Some(crate::sim::game_entity::InfantryRuntime::new());
    e
}

fn make_structure_entity(
    id: u64,
    type_ref: &str,
    rx: u16,
    ry: u16,
    current: i32,
    max: i32,
) -> GameEntity {
    let mut entity = make_entity(id, type_ref, rx, ry, max);
    entity.category = EntityCategory::Structure;
    entity.is_voxel = false;
    entity.health = Health { current };
    entity
}

fn run_combat_death_handoff(
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &mut crate::sim::intern::StringInterner,
    dead_entities: &[u64],
) -> DeathEffects {
    let mut occupancy = OccupancyGrid::new();
    let mut houses = BTreeMap::new();
    let mut main_rng = SimRng::new(0);
    let mut scenario_rng = SimRng::new(0);
    let mut handled_deaths = Vec::new();
    let handles = crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, interner);
    handle_entity_deaths(
        entities,
        &mut occupancy,
        rules,
        interner,
        Some(handles),
        &mut houses,
        &[],
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        dead_entities,
        &[],
        None,
        None,
        None,
        &mut None,
        false,
        0,
        &mut None,
        &mut None,
    )
}

#[test]
fn gsi_04_10_projectile_inert_suppresses_bridge_ore_and_collector_rng() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=WH\n\
         [WH]\nWall=yes\nCellSpread=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("inert projectile rules");
    let mut interner = test_interner();
    let warhead = interner.intern("WH");
    let weapon = interner.intern("MissingWeapon");
    let owner = interner.intern("Owner");
    let detonation = ProjectileDetonation {
        projectile_id: 1,
        source_id: 99,
        target: ProjectileTarget::Cell { rx: 8, ry: 5 },
        impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
        payload: ProjectilePayload {
            base_damage: 100,
            warhead,
            weapon,
        },
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut entities = EntityStore::new();
    let occupancy = OccupancyGrid::new();
    let mut scenario_rng = SimRng::new(77);
    let before_rng = scenario_rng.state();
    let mut emit = CombatEmit::default();
    let mut inline_hooks = None;

    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    emit_projectile_detonations(
        &[detonation],
        &mut entities,
        &occupancy,
        &rules,
        &mut interner,
        Some(handles),
        None,
        None,
        None,
        None,
        None,
        None,
        true,
        &HouseAllianceMap::new(),
        &mut scenario_rng,
        &mut inline_hooks,
        &mut emit,
    );

    assert!(emit.damage_events.is_empty());
    assert!(emit.effects.wall_mutations.is_empty());
    assert!(emit.effects.cell_target_detaches.is_empty());
    assert!(emit.effects.bridge_damage_events.is_empty());
    assert!(emit.effects.tiberium_reduction_requests.is_empty());
    assert_eq!(scenario_rng.state(), before_rng);
}

#[test]
fn lifecycle_authority_immediate_combat_death_reaches_uninit_without_precleanup() {
    let rules = test_rules();
    let mut store = EntityStore::new();

    let mut dead = make_entity(1, "MTNK", 5, 5, 0);
    dead.selected = true;
    dead.attack_target = Some(AttackTarget::new(2));
    dead.movement_target = Some(crate::sim::components::MovementTarget::default());
    dead.in_logic_vector = true;
    dead.lifecycle.in_limbo = false;
    dead.lifecycle.cell_marked = true;
    dead.radio_contacts.insert(2);
    store.insert(dead);

    let mut observer = make_entity(2, "MTNK", 6, 5, 300);
    observer.attack_target = Some(AttackTarget::new(1));
    store.insert(observer);

    let mut interner = test_interner();
    let result = run_combat_death_handoff(&mut store, &rules, &mut interner, &[1]);

    assert_eq!(result.immediate_uninit_ids, vec![1]);
    assert_eq!(result.despawned_ids, vec![1]);
    let dead = store
        .get(1)
        .expect("world must still be able to UnInit the victim");
    assert_eq!(dead.health.current, 0);
    assert!(
        !dead.dying,
        "immediate UnInit, not combat, owns the death gate"
    );
    assert!(dead.selected, "UnInit owns deselection");
    assert!(dead.attack_target.is_some(), "UnInit owns attack cleanup");
    assert!(
        dead.movement_target.is_some(),
        "UnInit owns movement cleanup"
    );
    assert!(dead.in_logic_vector, "UnInit owns LogicVector removal");
    assert!(dead.lifecycle.object_alive);
    assert!(!dead.lifecycle.in_limbo);
    assert!(dead.lifecycle.cell_marked);
    assert!(dead.radio_contacts.contains(2), "Techno Limbo owns BREAK");
    assert!(
        !dead.destruction_recorded,
        "world lifecycle owns count release"
    );
    assert!(
        store.get(2).unwrap().attack_target.is_some(),
        "combat must not bulk-clear other objects' targets"
    );
}

#[test]
fn lifecycle_authority_combat_leaves_transport_cargo_for_carrier_uninit() {
    let rules = test_rules();
    let mut store = EntityStore::new();

    let mut cargo = crate::sim::passenger::PassengerCargo::new(2, 1);
    assert!(cargo.board(2, 1));
    let mut carrier = make_entity(1, "MTNK", 5, 5, 0);
    carrier.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
    store.insert(carrier);

    let mut passenger = make_infantry_entity(2, "E1", 5, 5, 125);
    passenger.passenger_role = crate::sim::passenger::PassengerRole::Inside { transport_id: 1 };
    passenger.selected = true;
    passenger.attack_target = Some(AttackTarget::new(3));
    passenger.movement_target = Some(crate::sim::components::MovementTarget::default());
    store.insert(passenger);
    store.insert(make_entity(3, "MTNK", 6, 5, 300));

    let mut interner = test_interner();
    let result = run_combat_death_handoff(&mut store, &rules, &mut interner, &[1]);

    assert_eq!(result.immediate_uninit_ids, vec![1]);
    let carrier = store.get(1).unwrap();
    assert_eq!(carrier.passenger_role.cargo().unwrap().passengers, vec![2]);
    let passenger = store.get(2).unwrap();
    assert_eq!(passenger.health.current, 125);
    assert!(!passenger.dying);
    assert!(matches!(
        passenger.passenger_role,
        crate::sim::passenger::PassengerRole::Inside { transport_id: 1 }
    ));
    assert!(passenger.selected);
    assert!(passenger.attack_target.is_some());
    assert!(passenger.movement_target.is_some());
}

#[test]
fn lifecycle_authority_animated_combat_handoff_changes_only_dying_and_sequence() {
    let rules = test_rules();
    let mut store = EntityStore::new();

    let mut dead = make_infantry_entity(1, "E1", 5, 5, 0);
    dead.selected = true;
    dead.attack_target = Some(AttackTarget::new(2));
    dead.movement_target = Some(crate::sim::components::MovementTarget::default());
    dead.in_logic_vector = true;
    dead.lifecycle.in_limbo = false;
    dead.lifecycle.cell_marked = true;
    dead.radio_contacts.insert(2);
    store.insert(dead);

    let mut observer = make_entity(2, "MTNK", 6, 5, 300);
    observer.attack_target = Some(AttackTarget::new(1));
    store.insert(observer);

    let mut interner = test_interner();
    let result = run_combat_death_handoff(&mut store, &rules, &mut interner, &[1]);

    assert!(result.immediate_uninit_ids.is_empty());
    assert_eq!(result.despawned_ids, vec![1]);
    let dead = store.get(1).unwrap();
    assert_eq!(dead.health.current, 0);
    assert!(dead.dying);
    assert_eq!(
        dead.animation.as_ref().unwrap().sequence,
        SequenceKind::Die1
    );
    assert!(dead.selected);
    assert!(dead.attack_target.is_some());
    assert!(dead.movement_target.is_some());
    assert!(dead.in_logic_vector);
    assert!(dead.lifecycle.object_alive);
    assert!(!dead.lifecycle.in_limbo);
    assert!(dead.lifecycle.cell_marked);
    assert!(dead.radio_contacts.contains(2));
    assert!(!dead.destruction_recorded);
    assert!(
        store.get(2).unwrap().attack_target.is_some(),
        "selective removal listeners, not combat, own target invalidation"
    );
}

fn considered_aircraft_weapon_rules() -> RuleSet {
    let ini_str: &str = "\
[InfantryTypes]
0=ROCK
1=E1
[VehicleTypes]
0=IFV
[AircraftTypes]
[BuildingTypes]

[IFV]
Strength=200
Armor=light
Speed=8
Primary=GroundGun
Secondary=AirGun

[ROCK]
Strength=125
Armor=none
Speed=8
ConsideredAircraft=yes

[E1]
Strength=125
Armor=none
Speed=4

[GroundGun]
Damage=10
ROF=20
Range=7
Projectile=GroundProj
Warhead=TestWH

[AirGun]
Damage=10
ROF=20
Range=7
Projectile=AirProj
Warhead=TestWH

[GroundProj]
AG=yes
AA=no

[AirProj]
AG=no
AA=yes

[TestWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";
    let ini: IniFile = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("considered-aircraft combat rules should parse")
}

fn set_anim_frame(store: &mut EntityStore, id: u64, frame: u16) {
    store
        .get_mut(id)
        .unwrap()
        .animation
        .as_mut()
        .unwrap()
        .frame_index = frame;
}

#[test]
fn test_armor_index_lookup() {
    assert_eq!(armor_index("none"), 0);
    assert_eq!(armor_index("flak"), 1);
    assert_eq!(armor_index("heavy"), 5);
    assert_eq!(armor_index("wood"), 6);
    assert_eq!(armor_index("concrete"), 8);
    assert_eq!(armor_index("unknown"), 0);
}

#[test]
fn cell_center_coords_remains_ground_z_for_cell_targets() {
    let (rx, ry, sub_x, sub_y) = cell_center_coords(7, 9);
    assert_eq!((rx, ry), (7, 9));
    assert_eq!(sub_x.to_num::<i32>(), 128);
    assert_eq!(sub_y.to_num::<i32>(), 128);

    let entities = EntityStore::new();
    assert_eq!(
        attack_impact_z(TargetKind::Cell(7, 9), &entities, None),
        0,
        "with no loaded terrain there is no cell floor to read; the cell-centre \
         helper never invents one. The terrain-backed cases live in \
         `impact_height_tests`."
    );
}

#[test]
/// Re-baselined for GSI-08.02 (row 122). Weapon selection is altitude-driven,
/// not category-driven: `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3330`
/// arm V and the `GetFireError` AA gate both call `ObjectClass::IsHighFlying`
/// (`0x005F6B90`, vtable `+0x54`) = marked on the map and
/// `GetHeight() >= 2 * LeptonsPerLevel`. `ConsideredAircraft=` is not read
/// anywhere in that path. A Rocketeer standing on the ground is therefore an
/// ordinary ground target and draws the ground gun; it becomes an air target
/// only once it climbs past two levels.
#[allow(clippy::items_after_statements)]
fn considered_aircraft_infantry_is_air_only_while_high_flying() {
    // One fresh engagement per altitude — the attacker's ROF cooldown makes a
    // second shot in the same sim unobservable.
    fn fire_at_rocketeer(altitude_leptons: i64) -> (String, WeaponSlot) {
        let rules = considered_aircraft_weapon_rules();
        let mut sim = crate::sim::world::Simulation::new();
        let heights = BTreeMap::new();
        let attacker = sim
            .spawn_object("IFV", "Americans", 5, 5, 0, &rules, &heights)
            .expect("IFV should spawn");
        let target = sim
            .spawn_object("ROCK", "Soviet", 8, 5, 0, &rules, &heights)
            .expect("Rocketeer should spawn");

        let target_entity = sim
            .substrate
            .entities
            .get(target)
            .expect("target should exist");
        assert_eq!(target_entity.category, EntityCategory::Infantry);
        assert!(
            rules
                .object(sim.interner.resolve(target_entity.type_ref))
                .is_some_and(|obj| obj.considered_aircraft)
        );
        // `ConsideredAircraft` still drives the legacy category helper; the
        // selector no longer consults it.
        assert_eq!(
            combat_target_category(target_entity, &rules, &sim.interner),
            EntityCategory::Aircraft
        );

        if altitude_leptons > 0 {
            sim.substrate
                .entities
                .get_mut(target)
                .and_then(|entity| entity.locomotor.as_mut())
                .expect("Rocketeer carries a locomotor")
                .altitude = crate::util::fixed_math::SimFixed::from_num(altitude_leptons);
        }

        issue_attack_command(
            &mut sim.substrate.entities,
            attacker,
            target,
            None,
            &sim.interner,
        );
        let mut main_rng = SimRng::new(1);
        align_attackers_to_targets(&mut sim.substrate.entities, &rules, &sim.interner);
        let result = tick_combat(
            &mut sim.substrate.entities,
            &mut sim.substrate.occupancy,
            &rules,
            &mut sim.interner,
            0,
            100,
            0,
            &mut main_rng,
        );
        assert_eq!(result.consequences.fire_events().len(), 1);
        (
            sim.interner
                .resolve(result.consequences.fire_events()[0].weapon_id)
                .to_string(),
            result.consequences.fire_events()[0].weapon_slot,
        )
    }

    // Grounded: not high-flying, so the ladder falls through arm V to slot 0
    // and the AG-only GroundProj is legal.
    assert_eq!(
        fire_at_rocketeer(0),
        ("GroundGun".to_string(), WeaponSlot::Primary)
    );
    // Airborne past two levels: arm V picks the AA secondary.
    assert_eq!(
        fire_at_rocketeer(crate::util::lepton::HIGH_FLIGHT_THRESHOLD_LEPTONS),
        ("AirGun".to_string(), WeaponSlot::Secondary)
    );
}

#[test]
fn ordinary_infantry_remains_ground_for_projectile_legality() {
    let rules = considered_aircraft_weapon_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let attacker = sim
        .spawn_object("IFV", "Americans", 5, 5, 0, &rules, &heights)
        .expect("IFV should spawn");
    let target = sim
        .spawn_object("E1", "Soviet", 8, 5, 0, &rules, &heights)
        .expect("ordinary infantry should spawn");

    let target_entity = sim
        .substrate
        .entities
        .get(target)
        .expect("target should exist");
    assert_eq!(target_entity.category, EntityCategory::Infantry);
    assert_eq!(
        combat_target_category(target_entity, &rules, &sim.interner),
        EntityCategory::Infantry
    );

    issue_attack_command(
        &mut sim.substrate.entities,
        attacker,
        target,
        None,
        &sim.interner,
    );
    let mut main_rng = SimRng::new(1);
    align_attackers_to_targets(&mut sim.substrate.entities, &rules, &sim.interner);
    let result = tick_combat(
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        &rules,
        &mut sim.interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert_eq!(result.consequences.fire_events().len(), 1);
    assert_eq!(
        sim.interner
            .resolve(result.consequences.fire_events()[0].weapon_id),
        "GroundGun"
    );
    assert_eq!(
        result.consequences.fire_events()[0].weapon_slot,
        WeaponSlot::Primary
    );
}

#[test]
fn test_issue_attack_command() {
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    // Mid-reload: the reload is the object's (`TechnoClass+0x2EC`), and
    // `Assign_Target @ 0x006FCDB0` never touches it.
    let rearm = crate::sim::timer::CdTimer::started(0, 40);
    store.get_mut(1).unwrap().rearm_timer = rearm;

    let result: bool = issue_attack_command(&mut store, 1, 2, None, &test_interner());
    assert!(result, "Should succeed for valid entities");

    let attack = store.get(1).unwrap().attack_target.as_ref().unwrap();
    assert!(matches!(
        attack.target,
        crate::sim::combat::TargetKind::Entity(2)
    ));
    assert_eq!(
        store.get(1).unwrap().rearm_timer,
        rearm,
        "a new attack order does not reload the weapon"
    );
}

#[test]
fn test_attack_nonexistent_target() {
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));

    let result: bool = issue_attack_command(&mut store, 1, 99, None, &test_interner());
    assert!(!result, "Should fail for nonexistent target");
}

#[test]
fn test_tick_combat_applies_damage() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();

    // MTNK attacks another MTNK (heavy armor).
    // 105mm: damage=65, warhead=AP, AP verses[heavy(5)] = 75%.
    // Integer math: 65 * 75 / 100 = 48.
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    let target_health = store.get(2).expect("target alive").health.current;
    assert_eq!(
        target_health,
        300 - 48,
        "Should take 48 damage (65 * 75 / 100)"
    );
}

#[test]
fn combat_damage_crosses_live_type_condition_yellow() {
    let rules = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_structure_entity(2, "GAPOWR", 8, 5, 400, 750));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert!(
        store
            .get(2)
            .expect("building survives")
            .health
            .compare_ratio(
                rules.object("GAPOWR").unwrap().strength,
                rules.general.condition_yellow
            )
            == crate::util::native_x87::MaskedX87Ordering::Less
    );
}

#[test]
fn combat_damage_above_live_type_condition_yellow() {
    let rules = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_structure_entity(2, "GAPOWR", 8, 5, 750, 750));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert!(
        store
            .get(2)
            .expect("building survives")
            .health
            .compare_ratio(
                rules.object("GAPOWR").unwrap().strength,
                rules.general.condition_yellow
            )
            == crate::util::native_x87::MaskedX87Ordering::Greater
    );
}

#[test]
fn aoe_damage_crosses_live_type_condition_yellow() {
    let rules = building_damage_state_aoe_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_structure_entity(2, "GAPOWR", 8, 5, 60, 100));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert!(
        store
            .get(2)
            .expect("building survives")
            .health
            .compare_ratio(
                rules.object("GAPOWR").unwrap().strength,
                rules.general.condition_yellow
            )
            == crate::util::native_x87::MaskedX87Ordering::Less
    );
}

#[test]
fn combat_damage_landed_applies_infantry_fear() {
    let rules = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_infantry_entity(2, "E1", 8, 5, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert_eq!(
        store.get(2).unwrap().infantry.as_ref().unwrap().fear_level,
        100
    );
}

#[test]
fn ic_target_takes_zero_damage() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_infantry_entity(2, "E1", 8, 5, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    // Apply IronCurtain invulnerability to the target.
    if let Some(target) = store.get_mut(2) {
        target.invulnerability = Some(InvulnerabilityState {
            start_frame: 0,
            duration_frames: 1000,
            kind: InvulnKind::IronCurtain,
        });
    }
    let initial_hp = store.get(2).expect("target alive").health.current;
    let mut main_rng = SimRng::new(1);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        10u64,
        100,
        0u32,
        &mut main_rng,
    );
    assert_eq!(
        store.get(2).expect("target alive").health.current,
        initial_hp,
        "IC-invulnerable target must take zero damage"
    );
    assert!(
        store
            .get(2)
            .unwrap()
            .infantry
            .as_ref()
            .is_some_and(|inf| inf.fear_level == 0),
        "invulnerable targets should not gain fear because no damage lands"
    );
}

#[test]
fn test_tick_combat_only_emits_bridge_damage_for_wall_warheads() {
    let mut store = EntityStore::new();
    let rules_without_wall = test_rules();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);
    align_attackers_to_targets(&mut store, &rules_without_wall, &interner);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules_without_wall,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        None,
        0u64,
        100,
        0u32,
        &[],
        None,
        &mut main_rng,
    );
    assert!(
        result
            .consequences
            .effects()
            .bridge_damage_events
            .is_empty(),
        "non-wall warheads must not emit bridge damage"
    );
    assert!(
        result.consequences.effects().wall_mutations.is_empty(),
        "non-wall warheads must not emit wall damage"
    );

    let mut bridge_rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [AP]\nWall=yes\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    ))
    .expect("bridge combat rules should parse");
    // Combat reads IonCannonWarhead at the bridge-damage emit boundary; tests
    // that drive tick_combat must resolve before invoking it.
    let _handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&bridge_rules, &mut interner);
    let mut wall_store = EntityStore::new();
    wall_store.insert(make_entity(3, "MTNK", 5, 5, 300));
    wall_store.insert(make_entity(4, "MTNK", 8, 5, 300));
    issue_attack_command(&mut wall_store, 3, 4, None, &interner);
    align_attackers_to_targets(&mut wall_store, &bridge_rules, &interner);
    let wall_result = tick_combat_with_fog(
        &mut wall_store,
        &mut OccupancyGrid::new(),
        &bridge_rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        None,
        0u64,
        100,
        0u32,
        &[],
        None,
        &mut main_rng,
    );
    assert_eq!(
        wall_result.consequences.effects().bridge_damage_events,
        vec![BridgeDamageEvent {
            rx: 8,
            ry: 5,
            damage: 65,
            warhead_ref: interner
                .get("AP")
                .expect("AP warhead interned by tick_combat"),
            is_ion_cannon: false,
            impact_z: 0,
        }]
    );
    // Without an overlay grid+registry, the discriminator can't identify a wall
    // cell — events fall through to bridge_damage_events. Immediate wall
    // mutation requires both a grid lookup and Wall=yes in the registry.
    assert!(wall_result.consequences.effects().wall_mutations.is_empty());
}

#[test]
fn gsi_04_07_damage_wad_precedes_wall_and_wood_armor_routing() {
    fn fire(extra_warhead_flags: &str, overlay_armor: &str) -> (CombatTickResult, OverlayGrid) {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=MTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=KillWH\n1=WallWH\n2=NoWallWH\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Gun\n\
             [Gun]\nDamage=65\nROF=50\nRange=6\nWarhead=WH\n\
             [WH]\n{extra_warhead_flags}\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor={overlay_armor}\nStrength=1\n"
        ));
        let mut rules = RuleSet::from_ini(&ini).expect("wall route rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut entities = EntityStore::new();
        entities.insert(make_entity(1, "MTNK", 5, 5, 300));
        entities.insert(make_entity(2, "MTNK", 8, 5, 300));
        let mut interner = test_interner();
        let _handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        issue_attack_command(&mut entities, 1, 2, None, &interner);
        let mut overlays = OverlayGrid::new(12, 12);
        overlays.place_overlay(8, 5, 0, 0);
        let mut scenario_rng = SimRng::new(1);
        align_attackers_to_targets(&mut entities, &rules, &interner);
        let result = tick_combat_with_fog(
            &mut entities,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            None,
            &BTreeMap::new(),
            None,
            Some(&mut overlays),
            Some(&registry),
            None,
            0,
            100,
            0,
            &[],
            None,
            &mut scenario_rng,
        );
        (result, overlays)
    }

    let (absolute, absolute_grid) = fire("WallAbsoluteDestroyer=yes\nWall=yes", "concrete");
    assert_eq!(
        absolute.consequences.effects().wall_mutations,
        vec![crate::sim::overlay_grid::WallMutation {
            rx: 8,
            ry: 5,
            kind: crate::sim::overlay_grid::WallMutationKind::DirectRemoved,
        }],
        "WallAbsoluteDestroyer wins and commits forced removal inline"
    );
    assert_eq!(absolute_grid.cell(8, 5).overlay_id, None);
    assert!(
        absolute
            .consequences
            .effects()
            .bridge_damage_events
            .is_empty()
    );

    let (wood, wood_grid) = fire("Wood=yes", "wood");
    assert!(!wood.consequences.effects().wall_mutations.is_empty());
    assert_eq!(wood_grid.cell(8, 5).overlay_id, None);
    let (concrete, concrete_grid) = fire("Wood=yes", "concrete");
    assert!(concrete.consequences.effects().wall_mutations.is_empty());
    assert_eq!(concrete_grid.cell(8, 5).overlay_id, Some(0));
}

#[test]
fn gsi_04_07_damage_wall_dies_in_the_tail_after_both_attackers_fire() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [OverlayTypes]\n0=TESTWALL\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Gun\n\
         [Gun]\nDamage=1\nROF=50\nRange=8\nWarhead=WH\n\
         [WH]\nWallAbsoluteDestroyer=yes\nWall=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("live-order wall rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let mut entities = EntityStore::new();
    entities.insert(make_entity(10, "MTNK", 5, 5, 300));
    // Both tanks fire at the wall cell in the object pass. Their (Inviso)
    // bullets detonate in the same frame's Logic tail, where the first one
    // razes the wall and its detach restores the second tank's suspended
    // target (`0x0070D4A0`) for its next AI.
    let mut second = make_entity(20, "MTNK", 4, 5, 300);
    second.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Attack),
        suspended: MissionId::from_known(MissionType::Guard),
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    second.suspended_attack_target = Some(TargetKind::Entity(10));
    entities.insert(second);
    let mut interner = test_interner();
    let _handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    assert!(issue_attack_cell_command(
        &mut entities,
        10,
        8,
        5,
        Some(&rules),
        &interner,
    ));
    assert!(issue_attack_cell_command(
        &mut entities,
        20,
        8,
        5,
        Some(&rules),
        &interner,
    ));

    let mut overlays = OverlayGrid::new(16, 16);
    overlays.place_overlay(8, 5, 0, 0);
    let mut scenario_rng = SimRng::new(31);
    // Two attackers fire this tick, so the scenario stream advances by exactly
    // two `GetROF` reload jitters (`RandomRanged(0, 2)` @ `0x006FD0B0`) and
    // nothing else — in particular the WAD warhead still draws no Strength.
    let mut expected_rng = scenario_rng.clone();
    expected_rng.next_range_u32_inclusive(0, 2);
    expected_rng.next_range_u32_inclusive(0, 2);
    let before = expected_rng.state();
    align_attackers_to_targets(&mut entities, &rules, &interner);
    let result = tick_combat_with_fog(
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::new(),
        None,
        Some(&mut overlays),
        Some(&registry),
        None,
        0,
        100,
        0,
        &[10, 20],
        None,
        &mut scenario_rng,
    );

    assert_eq!(overlays.cell(8, 5).overlay_id, None);
    assert_eq!(
        scenario_rng.state(),
        before,
        "WAD consumes no Strength draw beyond the two reload jitters"
    );
    assert_eq!(
        result
            .consequences
            .fire_events()
            .iter()
            .map(|event| (event.attacker_id, event.target))
            .collect::<Vec<_>>(),
        vec![(10, TargetKind::Cell(8, 5)), (20, TargetKind::Cell(8, 5))],
        "both shots are Inviso bullets, so the wall stands until the tail"
    );
    assert_eq!(
        result
            .consequences
            .effects()
            .cell_target_detaches
            .iter()
            .map(|event| (event.listener_id, event.restored, event.cleared))
            .collect::<Vec<_>>(),
        vec![(10, false, true), (20, true, true)]
    );
}

#[test]
fn gsi_04_07_damage_prior_projectile_fatal_death_weapon_is_inline() {
    fn run(victim_hp: i32, explodes: bool) -> (CombatTickResult, OverlayGrid, EntityStore, u64) {
        let ini_text = format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=BOOMER\n1=SHOOTER\n2=TARGET\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [BOOMER]\nStrength=11\nArmor=heavy\nExplodes={}\nDeathWeapon=DeathBoom\n\
             [SHOOTER]\nStrength=100\nArmor=heavy\nPrimary=Gun\n\
             [TARGET]\nStrength=100\nArmor=heavy\n\
             [DeathBoom]\nDamage=214\nWarhead=WallWH\n\
             [Gun]\nDamage=0\nROF=50\nRange=8\nWarhead=NoWallWH\n\
             [WallWH]\nCellSpread=0\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [NoWallWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
            if explodes { "yes" } else { "no" },
        );
        let ini = IniFile::from_str(&ini_text);
        let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
        let mut rules = RuleSet::from_ini(&ini).expect("inline death-weapon rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
        assert_eq!(registry.flags(0).unwrap().strength, 400);
        assert_eq!(registry.flags(0).unwrap().damage_levels, 2);
        assert_eq!(
            rules.object("BOOMER").unwrap().death_weapon.as_deref(),
            Some("DeathBoom")
        );
        assert!(!rules.warhead("WallWH").unwrap().wall_absolute_destroyer);
        assert_eq!(rules.weapon("DeathBoom").unwrap().damage, 214);

        let mut entities = EntityStore::new();
        entities.insert(make_entity(10, "BOOMER", 8, 5, victim_hp));
        // West of both the wall cell and the target restored mid-tick, so one
        // hull heading serves both — see the note in the sibling live-order
        // test: a restored target BEHIND a turretless hull is refused for
        // facing by `UnitClass::GetFireError @ 0x00740FD0` step 17.
        let mut later_attacker = make_entity(20, "SHOOTER", 4, 5, 100);
        later_attacker
            .mission
            .apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Attack),
                suspended: MissionId::from_known(MissionType::Guard),
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
        later_attacker.suspended_attack_target = Some(TargetKind::Entity(30));
        entities.insert(later_attacker);
        entities.insert(make_entity(30, "TARGET", 5, 5, 100));
        let mut interner = test_interner();
        let _handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        assert!(issue_attack_cell_command(
            &mut entities,
            20,
            8,
            5,
            Some(&rules),
            &interner,
        ));

        let mut overlays = OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 5, 0, 0);
        let detonation = ProjectileDetonation {
            projectile_id: 1,
            source_id: 99,
            target: ProjectileTarget::Entity(10),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: ProjectilePayload {
                base_damage: 10,
                warhead: interner.intern("NoWallWH"),
                weapon: interner.intern("Gun"),
            },
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut scenario_rng = SimRng::new(1);
        let mut main_rng = SimRng::new(41);
        let mut houses = BTreeMap::new();
        let handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        align_attackers_to_targets(&mut entities, &rules, &interner);
        let result = tick_combat_with_fog_and_main_rng(
            &mut entities,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            Some(handles),
            None,
            &BTreeMap::new(),
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            None,
            Some(&mut overlays),
            Some(&registry),
            None,
            0,
            100,
            0,
            &[10, 20, 30],
            &[detonation],
            &[],
            None,
            &[],
            &mut scenario_rng,
            &mut main_rng,
            None,
        );
        (result, overlays, entities, scenario_rng.state())
    }

    let (fatal, fatal_grid, fatal_entities, fatal_rng) = run(10, true);
    assert_eq!(fatal_entities.get(10).unwrap().health.current, 0);
    assert_eq!(fatal.consequences.effects().immediate_uninit_ids, vec![10]);
    assert_eq!(fatal_grid.cell(8, 5).overlay_id, None);
    assert_eq!(
        fatal
            .consequences
            .fire_events()
            .iter()
            .map(|event| (event.attacker_id, event.target))
            .collect::<Vec<_>>(),
        vec![(20, TargetKind::Entity(30))],
        "later live-order attacker must observe nested target restoration"
    );
    assert!(
        fatal
            .consequences
            .effects()
            .cell_target_detaches
            .iter()
            .any(|detach| { detach.listener_id == 20 && detach.restored && detach.cleared })
    );
    assert_eq!(
        fatal_entities
            .get(20)
            .unwrap()
            .attack_target
            .as_ref()
            .unwrap()
            .target,
        TargetKind::Entity(30)
    );
    // Detonation processing runs before the fire phase, so the inline wall
    // draw lands first and the surviving attacker's `GetROF` reload jitter
    // (`RandomRanged(0, 2)` @ `0x006FD0B0`) follows it.
    let mut one_draw = SimRng::new(1);
    assert_eq!(one_draw.next_range_u32_inclusive(0, 400), 213);
    one_draw.next_range_u32_inclusive(0, 2);
    assert_eq!(fatal_rng, one_draw.state(), "nested wall draw is inline");

    let (survives, surviving_grid, surviving_entities, surviving_rng) = run(11, true);
    // No wall draw when the wall survives — only the attacker's reload jitter.
    assert_eq!(surviving_rng, {
        let mut expected = SimRng::new(1);
        expected.next_range_u32_inclusive(0, 2);
        expected.state()
    });
    assert_eq!(surviving_grid.cell(8, 5).overlay_id, Some(0));
    assert!(survives.consequences.effects().wall_mutations.is_empty());
    assert!(
        survives
            .consequences
            .effects()
            .cell_target_detaches
            .is_empty()
    );
    assert!(
        survives
            .consequences
            .effects()
            .immediate_uninit_ids
            .is_empty()
    );
    assert_eq!(surviving_entities.get(10).unwrap().health.current, 1);
    assert_eq!(
        survives
            .consequences
            .fire_events()
            .iter()
            .map(|event| (event.attacker_id, event.target))
            .collect::<Vec<_>>(),
        vec![(20, TargetKind::Cell(8, 5))],
        "HP=damage+1 must not expose death-wall or restored-target state"
    );

    let (ungated, ungated_grid, ungated_entities, ungated_rng) = run(10, false);
    assert_eq!(ungated_entities.get(10).unwrap().health.current, 0);
    assert_eq!(ungated_grid.cell(8, 5).overlay_id, Some(0));
    assert_eq!(ungated_rng, {
        let mut expected = SimRng::new(1);
        expected.next_range_u32_inclusive(0, 2);
        expected.state()
    });
    assert!(ungated.consequences.effects().wall_mutations.is_empty());
    assert!(
        ungated
            .consequences
            .effects()
            .cell_target_detaches
            .is_empty()
    );
}

/// One hit from `source` (an unarmed tank `source_cells` west of the victim)
/// on a VICTIM tank, through the production receiver. Returns whether the
/// victim turned on the source (`ShouldRetaliate 0x007087C0`, then
/// `ReceiveDamage`'s reach gate `0x00702A58..0x00702B2F`).
struct RetaliationCase {
    human: bool,
    mission: MissionType,
    source_cells: u16,
    /// The source's lepton offset inside its cell (128 is the centre).
    source_sub_x: i32,
    /// The source's health, of Strength=200.
    source_health: i32,
    range_cells: &'static str,
    sight: i32,
    damage: i32,
    verses: &'static str,
    player_return_fire: bool,
}

impl Default for RetaliationCase {
    fn default() -> Self {
        Self {
            human: true,
            mission: MissionType::Guard,
            source_cells: 2,
            source_sub_x: 128,
            source_health: 200,
            range_cells: "8",
            sight: 8,
            damage: 1,
            verses: "100%",
            player_return_fire: false,
        }
    }
}

fn retaliates(case: RetaliationCase) -> bool {
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=IncomingWH\n1=ReturnWH\n\
         [General]\nFixtureOnly=1\n\
         [CombatDamage]\nPlayerReturnFire={}\n\
         [SOURCE]\nStrength=200\nArmor=heavy\n\
         [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\nSight={}\nPrimary=ReturnGun\nCanRetaliate=yes\n\
         [ReturnGun]\nDamage={}\nROF=50\nRange={}\nWarhead=ReturnWH\n\
         [IncomingWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ReturnWH]\nCellSpread=0\nVerses={v},{v},{v},{v},{v},{v},{v},{v},{v},{v},{v}\n",
        if case.player_return_fire { "yes" } else { "no" },
        case.sight,
        case.damage,
        case.range_cells,
        v = case.verses,
    ));
    let rules = RuleSet::from_ini(&ini).expect("retaliation gate fixture");
    let mut interner = test_interner();
    let source_owner = interner.intern("SourceHouse");
    let victim_owner = interner.intern("VictimHouse");
    let incoming_wh = interner.intern("IncomingWH");
    let incoming_weapon = interner.intern("IncomingGun");
    let (source_rx, victim_rx) = (20 - case.source_cells, 20);

    let mut entities = EntityStore::new();
    let mut source = make_entity(1, "SOURCE", source_rx, 5, case.source_health);
    source.owner = source_owner;
    source.type_ref = interner.intern("SOURCE");
    source.position.sub_x = crate::util::fixed_math::SimFixed::from_num(case.source_sub_x);
    source.lifecycle.cell_marked = true;
    entities.insert(source);
    let mut victim = make_entity(2, "VICTIM", victim_rx, 5, 100);
    victim.owner = victim_owner;
    victim.type_ref = interner.intern("VICTIM");
    victim.lifecycle.cell_marked = true;
    victim.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(case.mission),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    victim.facing = 192;
    entities.insert(victim);

    let mut occupancy = OccupancyGrid::new();
    for (id, rx) in [(1, source_rx), (2, victim_rx)] {
        occupancy.add(
            rx,
            5,
            id,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
    }
    let detonation = ProjectileDetonation {
        projectile_id: 77,
        source_id: 1,
        target: ProjectileTarget::Entity(2),
        impact: ProjectileCoord::new(i32::from(victim_rx) * 256 + 128, 5 * 256 + 128, 0),
        payload: ProjectilePayload {
            base_damage: 10,
            warhead: incoming_wh,
            weapon: incoming_weapon,
        },
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut houses = BTreeMap::new();
    houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 0, None, case.human, 0, 10),
    );
    houses.insert(
        source_owner,
        HouseState::new(source_owner, 1, None, false, 0, 10),
    );
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    tick_combat_with_fog_and_main_rng(
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        &mut houses,
        &[],
        &HouseAllianceMap::new(),
        None,
        None,
        None,
        None,
        0,
        100,
        0,
        &[2],
        &[detonation],
        &[],
        None,
        &[],
        &mut SimRng::new(11),
        &mut SimRng::new(13),
        None,
    );
    entities
        .get(2)
        .and_then(|victim| victim.attack_target.as_ref())
        .is_some_and(|attack| attack.target == TargetKind::Entity(1))
}

/// `ShouldRetaliate 0x007089E8..0x00708A26`: unless `PlayerReturnFire=`, a
/// human's unit retaliates only on Guard, Area Guard or Patrol. A computer's
/// unit, or any unit under `PlayerReturnFire=yes`, retaliates on the move.
#[test]
fn gsi_04_07_a_human_unit_on_the_move_does_not_retaliate() {
    for mission in [
        MissionType::Guard,
        MissionType::AreaGuard,
        MissionType::Patrol,
    ] {
        assert!(
            retaliates(RetaliationCase {
                mission,
                ..Default::default()
            }),
            "{mission:?}"
        );
    }
    for mission in [MissionType::Move, MissionType::Attack, MissionType::Sticky] {
        assert!(
            !retaliates(RetaliationCase {
                mission,
                ..Default::default()
            }),
            "{mission:?}"
        );
    }
    assert!(retaliates(RetaliationCase {
        mission: MissionType::Move,
        human: false,
        ..Default::default()
    }));
    assert!(retaliates(RetaliationCase {
        mission: MissionType::Move,
        player_return_fire: true,
        ..Default::default()
    }));
}

/// `ReceiveDamage 0x00702A58..0x00702B2F`: a human's unit turns on a source
/// beyond its weapon's range only if the source is within `(Sight + 0.5) *
/// 256` leptons; a computer's always does. So a player's tank on Guard does
/// not charge an artillery piece it cannot see.
#[test]
fn gsi_04_07_a_human_unit_does_not_charge_an_unseen_shooter() {
    let far = |human, source_cells| RetaliationCase {
        human,
        source_cells,
        range_cells: "1",
        sight: 3,
        ..Default::default()
    };
    // Sight 3: 896 leptons; two cells (512) is seen, four (1024) is not.
    assert!(retaliates(far(true, 2)));
    assert!(!retaliates(far(true, 4)));
    assert!(retaliates(far(false, 4)));
    // The compare is inclusive and reads the ftol'd Sqrt_Approx distance: a
    // source whose distance comes out at exactly 896 is seen, 897 is not. Walk
    // raw offsets across the boundary from the victim's centre, x = 20 * 256
    // + 128; both sides must occur.
    let victim_x = 20 * 256 + 128;
    let mut distances = Vec::new();
    for offset in 896..=900 {
        let source_x = victim_x - offset;
        let distance = crate::util::native_x87::distance_3d_leptons(
            [source_x, 5 * 256 + 128, 0],
            [victim_x, 5 * 256 + 128, 0],
        );
        let case = RetaliationCase {
            source_sub_x: source_x % 256,
            ..far(true, (20 - source_x / 256) as u16)
        };
        assert_eq!(
            retaliates(case),
            distance <= 896,
            "offset {offset}, distance {distance}"
        );
        distances.push(distance);
    }
    assert!(
        distances.contains(&896) && distances.contains(&897),
        "{distances:?}"
    );
    // In range needs no sight.
    assert!(retaliates(RetaliationCase {
        source_cells: 4,
        range_cells: "5",
        sight: 1,
        ..Default::default()
    }));
}

/// `ShouldRetaliate 0x00708AF7..0x00708B09` refuses only Verses at or below
/// the single `0.01`: a 1% warhead still retaliates, a bare `0.005` (which
/// GetFireError's zero-Verses test lets through; `0.5%` would parse to 0) and
/// 0% do not. And a weapon whose
/// `Damage + AmbientDamage` is not positive never does (`0x007088A7`): zero
/// damage, or a healer even against a damaged source, which GetFireError's
/// heal test would allow.
#[test]
fn gsi_04_07_retaliation_verses_and_healer_gates() {
    assert!(retaliates(RetaliationCase {
        verses: "1%",
        ..Default::default()
    }));
    assert!(!retaliates(RetaliationCase {
        verses: "0.005",
        ..Default::default()
    }));
    assert!(!retaliates(RetaliationCase {
        verses: "0%",
        ..Default::default()
    }));
    assert!(!retaliates(RetaliationCase {
        damage: 0,
        ..Default::default()
    }));
    for source_health in [200, 100] {
        assert!(
            !retaliates(RetaliationCase {
                damage: -5,
                source_health,
                ..Default::default()
            }),
            "{source_health}"
        );
    }
}

const GATE_SOURCE: u64 = 1;
const GATE_VICTIM: u64 = 2;

/// A world for [`combat_targeting::should_retaliate`] alone: `source_type`
/// (house Source, a computer) two cells west of `victim_type` (house Victim,
/// on Guard).
fn retaliation_gate_world(
    victim_type: &str,
    victim_category: EntityCategory,
    source_type: &str,
    source_category: EntityCategory,
    human: bool,
) -> (crate::sim::world::Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=C4GUY\n1=E1\n\
         [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=SRCBLDG\n1=EMPBLDG\n2=CAGAS\n\
         [Warheads]\n0=ReturnWH\n\
         [General]\nFixtureOnly=1\n\
         [SOURCE]\nStrength=200\nArmor=heavy\n\
         [SRCBLDG]\nStrength=200\nArmor=concrete\n\
         [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\nSight=8\nPrimary=ReturnGun\n\
         [C4GUY]\nStrength=100\nArmor=none\nSpeed=4\nSight=8\nPrimary=ReturnGun\nC4=yes\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=ReturnGun\nOccupyWeapon=ReturnGun\n\
         [EMPBLDG]\nStrength=100\nArmor=concrete\nPrimary=ReturnGun\nEMPulseCannon=yes\n\
         [CAGAS]\nStrength=800\nArmor=wood\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\n\
         [ReturnGun]\nDamage=1\nROF=50\nRange=8\nWarhead=ReturnWH\n\
         [ReturnWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("retaliation gate fixture");
    let mut sim = crate::sim::world::Simulation::new();
    let victim_owner = sim.interner.intern("Victim");
    let source_owner = sim.interner.intern("Source");
    sim.houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 0, None, human, 0, 10),
    );
    sim.houses.insert(
        source_owner,
        HouseState::new(source_owner, 1, None, false, 0, 10),
    );
    for (id, type_name, category, owner, rx, health) in [
        (
            GATE_SOURCE,
            source_type,
            source_category,
            source_owner,
            18,
            200,
        ),
        (
            GATE_VICTIM,
            victim_type,
            victim_category,
            victim_owner,
            20,
            100,
        ),
    ] {
        let mut entity = GameEntity::test_default(id, type_name, "Test", rx, 5);
        entity.owner = owner;
        entity.type_ref = sim.interner.intern(type_name);
        entity.category = category;
        entity.health.current = health;
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        sim.substrate.entities.insert(entity);
    }
    (sim, rules)
}

/// `ShouldRetaliate @ 0x007087C0`'s refusals that read world state. Each case
/// changes one fact of a world whose baseline retaliates.
#[test]
fn gsi_04_07_should_retaliate_world_refusals() {
    use combat_targeting::should_retaliate;
    let tank = |human| {
        retaliation_gate_world(
            "VICTIM",
            EntityCategory::Unit,
            "SOURCE",
            EntityCategory::Unit,
            human,
        )
    };
    for human in [false, true] {
        let (sim, rules) = tank(human);
        assert!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            "baseline, human {human}"
        );
    }
    // `0x00708807..0x0070881F`: a draining object refuses only for a house
    // that is not human (`House+0x1EC`).
    for human in [false, true] {
        let (mut sim, rules) = tank(human);
        sim.substrate
            .entities
            .get_mut(GATE_VICTIM)
            .unwrap()
            .drain_target = Some(9);
        assert_eq!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            human,
            "draining, human {human}"
        );
    }
    // `0x007087EB`: a slave (SlaveOwner `+0x2DC`). GetFireError's T2
    // refuses a slave as well, so this outcome does not isolate the gate.
    let (mut sim, rules) = tank(false);
    sim.substrate
        .entities
        .get_mut(GATE_VICTIM)
        .unwrap()
        .slave_harvester = Some(crate::sim::slave_miner::SlaveHarvester::new(9, 4));
    assert!(!should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE));
    // `0x00708899`: the source is disguised to the victim's house as one of
    // its own (vt+0xC8).
    let (mut sim, rules) = tank(false);
    let victim_owner = sim.interner.intern("Victim");
    sim.substrate
        .entities
        .get_mut(GATE_SOURCE)
        .unwrap()
        .disguise = Some(crate::sim::cloak_disguise::DisguiseRuntime {
        disguised: true,
        disguised_as_house: Some(victim_owner),
        ..Default::default()
    });
    assert!(!should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE));
    // `0x00708905..0x007089A5`: a human's C4 infantryman leaves a building
    // alone; a computer's does not.
    for human in [false, true] {
        let (sim, rules) = retaliation_gate_world(
            "C4GUY",
            EntityCategory::Infantry,
            "SRCBLDG",
            EntityCategory::Structure,
            human,
        );
        assert_eq!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            !human,
            "C4 against a building, human {human}"
        );
    }
    // `0x00708A2C..0x00708A54`: a member of a `Suicide=` team.
    for suicide in [false, true] {
        let (mut sim, rules) = tank(false);
        crate::sim::team_script_vm::join_one_member_team_for_test(
            &mut sim,
            GATE_VICTIM,
            suicide,
            false,
        );
        assert_eq!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            !suicide,
            "Suicide={suicide}"
        );
    }
    // `0x007088FC`: GetFireError Cant, from an `EMPulseCannon=` building
    // (`BuildingClass::GetFireError` B3 `0x00447F54`).
    let (sim, rules) = retaliation_gate_world(
        "EMPBLDG",
        EntityCategory::Structure,
        "SOURCE",
        EntityCategory::Unit,
        false,
    );
    assert!(!should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE));
}

/// A garrisoned building fights back with its occupant's weapon: GetWeapon
/// (`BuildingClass::GetWeapon @ 0x004526F0`) answers the occupant's
/// `OccupyWeapon=` for every slot, so `GetWeaponDamageValue(-1)`, SelectWeapon,
/// GetFireError and the Verses test all read it. An empty civilian building
/// has no weapon and does not.
#[test]
fn gsi_04_07_a_garrison_retaliates_with_its_occupants_weapon() {
    let (mut sim, rules) = retaliation_gate_world(
        "CAGAS",
        EntityCategory::Structure,
        "SOURCE",
        EntityCategory::Unit,
        false,
    );
    assert!(!combat_targeting::should_retaliate(
        &sim,
        &rules,
        GATE_VICTIM,
        GATE_SOURCE
    ));
    let occupant_id = 3;
    let mut occupant = GameEntity::test_default(occupant_id, "E1", "Test", 20, 5);
    occupant.owner = sim.substrate.entities.get(GATE_VICTIM).unwrap().owner();
    occupant.type_ref = sim.interner.intern("E1");
    occupant.category = EntityCategory::Infantry;
    occupant.passenger_role = crate::sim::passenger::PassengerRole::Inside {
        transport_id: GATE_VICTIM,
    };
    sim.substrate.entities.insert(occupant);
    let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 1);
    assert!(cargo.board(occupant_id, 1));
    sim.substrate
        .entities
        .get_mut(GATE_VICTIM)
        .unwrap()
        .passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
    assert!(combat_targeting::should_retaliate(
        &sim,
        &rules,
        GATE_VICTIM,
        GATE_SOURCE
    ));
}

#[test]
fn gsi_04_07_damage_retaliation_is_receiver_synchronous_and_uses_mission_override() {
    #[derive(Debug)]
    struct Outcome {
        health: i32,
        current: MissionId,
        suspended: MissionId,
        target: Option<TargetKind>,
        nav_com: Option<crate::sim::components::NavTargetRef>,
        suspended_nav_com: Option<crate::sim::components::NavTargetRef>,
        has_movement: bool,
        fired: Vec<(u64, TargetKind)>,
        immediate_uninit: Vec<u64>,
    }

    fn run(
        mission: MissionType,
        source_present: bool,
        allied: bool,
        victim_health: i32,
    ) -> Outcome {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=IncomingWH\n1=ReturnWH\n\
             [SOURCE]\nStrength=200\nArmor=heavy\n\
             [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\nPrimary=ReturnGun\nCanRetaliate=yes\n\
             [IncomingGun]\nDamage=10\nRange=8\nWarhead=IncomingWH\n\
             [ReturnGun]\nDamage=1\nROF=50\nRange=8\nWarhead=ReturnWH\n\
             [IncomingWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [ReturnWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [Harvest]\nRetaliate=no\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("retaliation receiver fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type = interner.intern("VICTIM");
        let incoming_wh = interner.intern("IncomingWH");
        let incoming_weapon = interner.intern("IncomingGun");

        let mut entities = EntityStore::new();
        let mut source = make_entity(1, "SOURCE", 6, 5, 200);
        source.owner = source_owner;
        source.type_ref = source_type;
        source.lifecycle.in_limbo = false;
        source.lifecycle.cell_marked = true;
        entities.insert(source);

        let mut victim = make_entity(2, "VICTIM", 8, 5, 100);
        victim.owner = victim_owner;
        victim.type_ref = victim_type;
        victim.health.current = victim_health;
        victim.lifecycle.in_limbo = false;
        victim.lifecycle.cell_marked = true;
        victim.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(mission),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        // The victim retaliates WEST at the source; give it that hull heading up
        // front so the turretless body gate (`UnitClass::GetFireError @
        // 0x00740FD0` step 17) does not spend this pass turning. The subject
        // here is the inline mission Override, not turn-to-fire.
        victim.facing = 192;
        victim.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(9, 5));
        victim.movement_target = Some(crate::sim::components::MovementTarget::default());
        entities.insert(victim);

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            6,
            5,
            1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        occupancy.add(
            8,
            5,
            2,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        let detonation = ProjectileDetonation {
            projectile_id: 77,
            source_id: if source_present { 1 } else { RAD_NO_ATTACKER },
            target: ProjectileTarget::Entity(2),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: ProjectilePayload {
                base_damage: 10,
                warhead: incoming_wh,
                weapon: incoming_weapon,
            },
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut alliances = HouseAllianceMap::new();
        if allied {
            alliances
                .entry("VICTIMHOUSE".to_string())
                .or_default()
                .insert("SOURCEHOUSE".to_string());
            alliances
                .entry("SOURCEHOUSE".to_string())
                .or_default()
                .insert("VICTIMHOUSE".to_string());
        }
        let mut scenario_rng = SimRng::new(11);
        let mut main_rng = SimRng::new(13);
        let mut houses = BTreeMap::new();
        let handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
        align_attackers_to_targets(&mut entities, &rules, &interner);
        let result = tick_combat_with_fog_and_main_rng(
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            Some(handles),
            None,
            &BTreeMap::new(),
            &mut houses,
            &[],
            &alliances,
            None,
            None,
            None,
            None,
            0,
            100,
            0,
            &[2],
            &[detonation],
            &[],
            None,
            &[],
            &mut scenario_rng,
            &mut main_rng,
            None,
        );
        let victim = entities.get(2).expect("deferred storage keeps victim");
        Outcome {
            health: victim.health.current,
            current: victim.mission.current(),
            suspended: victim.mission.suspended(),
            target: victim.attack_target.as_ref().map(|target| target.target),
            nav_com: victim.navigation.nav_com,
            suspended_nav_com: victim.navigation.suspended_nav_com,
            has_movement: victim.movement_target.is_some(),
            fired: result
                .consequences
                .fire_events()
                .iter()
                .map(|event| (event.attacker_id, event.target))
                .collect(),
            immediate_uninit: result.consequences.effects().immediate_uninit_ids.clone(),
        }
    }

    let live = run(MissionType::Guard, true, false, 100);
    assert_eq!(live.health, 90);
    assert_eq!(live.current, MissionId::from_known(MissionType::Attack));
    assert_eq!(live.suspended, MissionId::from_known(MissionType::Guard));
    assert_eq!(live.target, Some(TargetKind::Entity(1)));
    assert_eq!(live.nav_com, None);
    assert_eq!(
        live.suspended_nav_com,
        Some(crate::sim::components::NavTargetRef::cell(9, 5))
    );
    assert!(
        !live.has_movement,
        "NULL destination stops the represented path"
    );
    assert_eq!(
        live.fired,
        vec![(2, TargetKind::Entity(1))],
        "the later live slot reads the inline Override and fires this pass"
    );

    for (blocked, expected_mission) in [
        (
            run(MissionType::Harvest, true, false, 100),
            MissionType::Harvest,
        ),
        (
            run(MissionType::Guard, false, false, 100),
            MissionType::Guard,
        ),
        (run(MissionType::Guard, true, true, 100), MissionType::Guard),
    ] {
        assert_eq!(blocked.current, MissionId::from_known(expected_mission));
        assert_eq!(blocked.health, 90);
        assert!(blocked.target.is_none());
        assert!(blocked.fired.is_empty());
        assert!(blocked.nav_com.is_some());
        assert!(blocked.has_movement);
    }
    let fatal = run(MissionType::Guard, true, false, 10);
    assert_eq!(fatal.health, 0);
    assert_eq!(fatal.target, None);
    assert!(fatal.fired.is_empty());
    assert_eq!(fatal.immediate_uninit, vec![2]);
}

#[test]
fn gsi_04_07_damage_retaliation_peek_rejects_limbo_attacker() {
    fn run(attacker_in_limbo: bool) -> (i32, bool, MissionId, MissionId, Option<TargetKind>) {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=HitWH\n1=ReturnWH\n\
             [SOURCE]\nStrength=200\nArmor=heavy\n\
             [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\nPrimary=ReturnGun\nCanRetaliate=yes\n\
             [ReturnGun]\nDamage=1\nRange=8\nWarhead=ReturnWH\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [ReturnWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("retaliation peek fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type = interner.intern("VICTIM");
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let mut source = make_entity(1, "SOURCE", 6, 5, 200);
        source.owner = source_owner;
        source.type_ref = source_type;
        source.lifecycle.in_limbo = attacker_in_limbo;
        source.lifecycle.cell_marked = !attacker_in_limbo;
        entities.insert(source);

        let mut victim = make_entity(2, "VICTIM", 8, 5, 100);
        victim.owner = victim_owner;
        victim.type_ref = victim_type;
        victim.lifecycle.in_limbo = false;
        victim.lifecycle.cell_marked = true;
        victim.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        entities.insert(victim);

        let event = EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), hit_wh);
        let mut occupancy = OccupancyGrid::new();
        let mut main_rng = SimRng::new(5);
        let mut scenario_rng = SimRng::new(7);
        let mut handled_deaths = Vec::new();
        let mut houses = BTreeMap::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let _ = commit_damage_events(
            &[event],
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        let victim = entities.get(2).expect("victim survives");
        (
            victim.health.current,
            victim.was_attacked_by_enemy,
            victim.mission.current(),
            victim.mission.suspended(),
            victim.attack_target.as_ref().map(|target| target.target),
        )
    }

    let illegal = run(true);
    assert_eq!(illegal.0, 90, "ReceiveDamage still commits before the peek");
    assert!(illegal.1, "the surviving hostile-hit postlude still runs");
    assert_eq!(illegal.2, MissionId::from_known(MissionType::Guard));
    assert_eq!(illegal.3, MissionId::NONE);
    assert_eq!(illegal.4, None, "FIRE_ILLEGAL suppresses Override");

    let legal = run(false);
    assert_eq!(legal.0, 90);
    assert!(legal.1);
    assert_eq!(legal.2, MissionId::from_known(MissionType::Attack));
    assert_eq!(legal.3, MissionId::from_known(MissionType::Guard));
    assert_eq!(legal.4, Some(TargetKind::Entity(1)));
}

#[test]
fn gsi_04_07_damage_invulnerability_impact_precedes_warping_and_postlude() {
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=HitWH\n\
         [SOURCE]\nStrength=100\nArmor=heavy\nSpeed=6\n\
         [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\n\
         [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("invulnerability impact fixture");
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let source_id = sim
        .spawn_object("SOURCE", "SourceHouse", 6, 5, 0, &rules, &heights)
        .expect("source spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner;
    let protected = [
        (8, InvulnKind::IronCurtain, false),
        (9, InvulnKind::ForceShield, false),
        (10, InvulnKind::IronCurtain, true),
    ]
    .map(|(rx, kind, warping)| {
        let id = sim
            .spawn_object("VICTIM", "VictimHouse", rx, 5, 0, &rules, &heights)
            .expect("protected victim spawns");
        let victim = sim.substrate.entities.get_mut(id).unwrap();
        victim.invulnerability = Some(InvulnerabilityState {
            start_frame: 0,
            duration_frames: 100,
            kind,
        });
        if warping {
            victim.teleport_state = Some(TeleportState {
                phase: TeleportPhase::Relocate,
                target_rx: 20,
                target_ry: 20,
                being_warped_ticks: 1,
            });
        }
        id
    });
    let healing_id = sim
        .spawn_object("VICTIM", "VictimHouse", 11, 5, 0, &rules, &heights)
        .expect("healing control spawns");
    let ignored_id = sim
        .spawn_object("VICTIM", "VictimHouse", 12, 5, 0, &rules, &heights)
        .expect("ignore-defenses control spawns");
    for id in [healing_id, ignored_id] {
        sim.substrate.entities.get_mut(id).unwrap().invulnerability = Some(InvulnerabilityState {
            start_frame: 0,
            duration_frames: 100,
            kind: InvulnKind::IronCurtain,
        });
    }
    sim.substrate
        .entities
        .get_mut(healing_id)
        .unwrap()
        .health
        .current = 90;
    let hit_wh = sim.interner.intern("HitWH");
    let hits = vec![
        EntityDamageEvent::area(protected[0], 10, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::area(protected[1], 20, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::area(protected[2], 30, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::area(healing_id, -10, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::direct_receiver(
            ignored_id,
            10,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
    ];

    sim.commit_noncombat_aoe_hits(&rules, None, &hits);

    for id in protected {
        let victim = sim.substrate.entities.get(id).unwrap();
        assert_eq!(victim.health.current, 100);
        assert!(!victim.was_attacked_by_enemy);
        assert_eq!(victim.damage_smoke_system_id, None);
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(healing_id)
            .unwrap()
            .health
            .current,
        100,
        "negative healing bypasses IC without an impact"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(ignored_id)
            .unwrap()
            .health
            .current,
        90,
        "ignoreDefenses bypasses IC without an impact"
    );

    let effects = &sim.combat_light_requests;
    assert_eq!(effects.len(), 3);
    assert_eq!(
        effects
            .iter()
            .filter_map(|effect| effect.target_id)
            .collect::<Vec<_>>(),
        protected,
        "the dedicated combat-light handoff preserves receiver order"
    );
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.damage)
            .collect::<Vec<_>>(),
        vec![20, 40, 60]
    );
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.flags)
            .collect::<Vec<_>>(),
        vec![1, 6, 1],
        "native selector flags distinguish IC from ForceShield"
    );
    for (index, effect) in effects.iter().enumerate() {
        assert_eq!(effect.warhead_ref, hit_wh);
        assert!(effect.force_create);
        assert_eq!(
            effect.coord,
            ProjectileCoord::new((8 + index as i32) * 256 + 128, 5 * 256 + 128, 0),
            "the helper receives the protected target coordinate, not source/impact"
        );
    }
}

#[test]
fn gsi_04_07_damage_receiver_smoke_creation_precedes_retaliation() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n1=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=HitWH\n1=ReturnWH\n\
         [ParticleSystems]\n0=SparkSys\n1=SmallGreySSys\n\
         [SOURCE]\nStrength=200\nArmor=heavy\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=ReturnGun\nCanRetaliate=yes\nDamageParticleSystems=SparkSys,SmallGreySSys\n\
         [ReturnGun]\nDamage=1\nROF=50\nRange=8\nWarhead=ReturnWH\n\
         [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ReturnWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [SparkSys]\nBehavesLike=Spark\nLifetime=5\n\
         [SmallGreySSys]\nBehavesLike=Smoke\nLifetime=-1\nSpawns=no\n\
         [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
    ))
    .expect("damage-Smoke receiver fixture");
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let source_id = sim
        .spawn_object("SOURCE", "SourceHouse", 6, 5, 0, &rules, &heights)
        .expect("source spawns");
    let victim_id = sim
        .spawn_object("MTNK", "VictimHouse", 8, 5, 0, &rules, &heights)
        .expect("Grizzly spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner;
    let victim_owner = sim.substrate.entities.get(victim_id).unwrap().owner;
    sim.houses.insert(
        source_owner,
        HouseState::new(source_owner, 0, None, false, 0, 10),
    );
    sim.houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 1, None, false, 0, 10),
    );
    {
        let victim = sim.substrate.entities.get_mut(victim_id).unwrap();
        victim.health = Health { current: 180 };
        victim.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
    }
    let hit_wh = sim.interner.intern("HitWH");
    let before_order = sim.live_object_order_snapshot();
    let before_rng = sim.scenario_rng.logical_state();

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            victim_id,
            60,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
        )],
    );

    let victim = sim.substrate.entities.get(victim_id).unwrap();
    assert_eq!(victim.health.current, 120);
    assert_eq!(
        victim.mission.current(),
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        victim.attack_target.as_ref().map(|target| target.target),
        Some(TargetKind::Entity(source_id)),
        "retaliation sees the receiver after synchronous smoke creation"
    );
    let system_id = victim
        .damage_smoke_system_id
        .expect("yellow crossing attaches a Smoke system");
    let system = sim.particle_systems().get(system_id).unwrap();
    assert_eq!(
        rules.particle_system_type(system.type_id).name,
        "SmallGreySSys"
    );
    assert_eq!(system.owner_entity, Some(victim_id));
    assert_eq!(system.attached_entity, None);
    assert_eq!(system.owner_house, None);
    assert_eq!(
        system.coords,
        glam::IVec3::new(8 * 256 + 128, 5 * 256 + 128, 0)
    );
    let mut expected_order = before_order;
    expected_order.push(system_id);
    assert_eq!(sim.live_object_order_snapshot(), expected_order);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before_rng,
        "reverse filtering leaves one Smoke choice, so RandomRanged(0,0) draws nothing"
    );

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            victim_id,
            1,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
        )],
    );
    assert_eq!(
        sim.particle_systems().len(),
        1,
        "live +0x310 suppresses duplicates"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .damage_smoke_system_id,
        Some(system_id)
    );

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            victim_id,
            -61,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
        )],
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .health
            .current,
        180
    );
    assert!(
        sim.particle_systems().get(system_id).unwrap().done_spawning,
        "recovery above ConditionYellow invokes mark-only ParticleSystem Destroy"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .damage_smoke_system_id,
        Some(system_id),
        "the owner pointer remains until physical pointer expiry"
    );
    sim.retire_particle_system(system_id);
    sim.process_pending_delete();
    assert!(sim.particle_systems().get(system_id).is_none());
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .damage_smoke_system_id,
        None,
        "physical finalization clears the +0x310 pointer"
    );
}

#[test]
fn gsi_04_07_damage_ai_retaliation_keeps_higher_scored_current_target() {
    #[derive(Debug)]
    struct Outcome {
        current_score: i64,
        attacker_score: i64,
        mission: MissionId,
        suspended: MissionId,
        target: TargetKind,
        nav_com: Option<crate::sim::components::NavTargetRef>,
        suspended_nav_com: Option<crate::sim::components::NavTargetRef>,
    }

    fn run(current_id: u64, attacker_id: u64) -> Outcome {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=TANY\n1=E1\n\
             [VehicleTypes]\n0=HTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=HitWH\n1=AP\n2=HollowPoint2\n3=SA\n\
             [General]\nMyEffectivenessCoefficientDefault=200\nTargetEffectivenessCoefficientDefault=-200\nTargetSpecialThreatCoefficientDefault=200\nTargetStrengthCoefficientDefault=-200\nTargetDistanceCoefficientDefault=-10\n\
             [HTNK]\nStrength=400\nArmor=heavy\nSpeed=6\nPrimary=120mm\nCanRetaliate=yes\n\
             [TANY]\nStrength=200\nArmor=flak\nPrimary=DoublePistols\nSpecialThreatValue=1\n\
             [E1]\nStrength=125\nArmor=none\nPrimary=M60\n\
             [120mm]\nDamage=90\nRange=5.75\nWarhead=AP\n\
             [DoublePistols]\nDamage=125\nRange=6\nWarhead=HollowPoint2\n\
             [M60]\nDamage=15\nRange=4\nWarhead=SA\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [AP]\nVerses=25%,25%,15%,75%,100%,100%,65%,45%,60%,60%,100%\n\
             [HollowPoint2]\nVerses=100%,100%,100%,0%,0%,0%,1%,1%,1%,1%,100%\n\
             [SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("stock threat-score fixture");
        let mut interner = test_interner();
        let ai_owner = interner.intern("AI");
        let tanya_owner = interner.intern("TanyaHouse");
        let gi_owner = interner.intern("GiHouse");
        let htnk_type = interner.intern("HTNK");
        let tanya_type = interner.intern("TANY");
        let gi_type = interner.intern("E1");
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let mut victim = make_entity(10, "HTNK", 8, 5, 400);
        victim.owner = ai_owner;
        victim.type_ref = htnk_type;
        victim.lifecycle.in_limbo = false;
        victim.lifecycle.cell_marked = true;
        victim.attack_target = Some(AttackTarget::new(current_id));
        victim.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(9, 5));
        victim.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        entities.insert(victim);

        let mut tanya = make_entity(20, "TANY", 6, 5, 200);
        tanya.owner = tanya_owner;
        tanya.type_ref = tanya_type;
        tanya.category = EntityCategory::Infantry;
        tanya.lifecycle.in_limbo = false;
        tanya.lifecycle.cell_marked = true;
        entities.insert(tanya);

        let mut gi = make_entity(30, "E1", 10, 5, 125);
        gi.owner = gi_owner;
        gi.type_ref = gi_type;
        gi.category = EntityCategory::Infantry;
        gi.lifecycle.in_limbo = false;
        gi.lifecycle.cell_marked = true;
        entities.insert(gi);

        let current_score = crate::sim::combat::combat_targeting::calculate_ai_threat_score(
            &entities, 10, current_id, &rules, &interner, None, None,
        )
        .map(|score| {
            i64::from(crate::util::native_x87::MaskedX87Chop53::ftol_i32_low_masked(score))
        })
        .expect("current target score");
        let attacker_score = crate::sim::combat::combat_targeting::calculate_ai_threat_score(
            &entities,
            10,
            attacker_id,
            &rules,
            &interner,
            None,
            None,
        )
        .map(|score| {
            i64::from(crate::util::native_x87::MaskedX87Chop53::ftol_i32_low_masked(score))
        })
        .expect("attacker score");

        let mut houses = BTreeMap::new();
        houses.insert(ai_owner, HouseState::new(ai_owner, 0, None, false, 0, 10));
        let source_house = entities.get(attacker_id).expect("attacker").owner;
        let event = EntityDamageEvent::area(10, 1, 0, attacker_id, Some(source_house), hit_wh);
        let mut occupancy = OccupancyGrid::new();
        let mut main_rng = SimRng::new(5);
        let mut scenario_rng = SimRng::new(7);
        let mut handled_deaths = Vec::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let _ = commit_damage_events(
            &[event],
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        let victim = entities.get(10).expect("victim retained");
        Outcome {
            current_score,
            attacker_score,
            mission: victim.mission.current(),
            suspended: victim.mission.suspended(),
            target: victim
                .attack_target
                .as_ref()
                .expect("target retained")
                .target,
            nav_com: victim.navigation.nav_com,
            suspended_nav_com: victim.navigation.suspended_nav_com,
        }
    }

    // Scored on the per-type coefficient set (200 / -200 / 200 / -200 / -10),
    // which `HouseClass+0x1FB` selects for every skirmish house from creation.
    // Tanya: base 100000, C = 200*SpecialThreatValue 1 = +200, A = 200 * AP-vs-
    // flak 25% = +50, D = -200 * (200/200) = -200, B absent (HollowPoint2 is 0%
    // against heavy, so no weapon selects), E = 0 (2 cells, range 5) → 100050.
    // GI: B = -200 * SA-vs-heavy 25% = -50, C = 0, A = 200 * AP-vs-none 25% =
    // +50, D = -200, E = 0 → 99800.
    let keep_tanya = run(20, 30);
    assert_eq!(
        (keep_tanya.current_score, keep_tanya.attacker_score),
        (100_050, 99_800)
    );
    assert_eq!(
        keep_tanya.mission,
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(keep_tanya.suspended, MissionId::NONE);
    assert_eq!(keep_tanya.target, TargetKind::Entity(20));
    assert_eq!(
        keep_tanya.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(9, 5))
    );
    assert_eq!(keep_tanya.suspended_nav_com, None);

    let switch_to_tanya = run(30, 20);
    assert_eq!(
        (
            switch_to_tanya.current_score,
            switch_to_tanya.attacker_score
        ),
        (99_800, 100_050)
    );
    assert_eq!(
        switch_to_tanya.mission,
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        switch_to_tanya.suspended,
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(switch_to_tanya.target, TargetKind::Entity(20));
    assert_eq!(switch_to_tanya.nav_com, None);
    assert_eq!(
        switch_to_tanya.suspended_nav_com,
        Some(crate::sim::components::NavTargetRef::cell(9, 5))
    );
}

#[test]
fn gsi_04_07_damage_spawn_and_slave_managers_block_retaliation() {
    fn run(slave_manager_shaped: bool) -> (MissionId, MissionId, Option<TargetKind>, bool) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=SLAV\n\
             [VehicleTypes]\n0=SOURCE\n1=CARRIER\n2=SLAVEMASTER\n\
             [AircraftTypes]\n0=HORNET\n\
             [BuildingTypes]\n\
             [Warheads]\n0=HitWH\n1=ReturnWH\n\
             [SOURCE]\nStrength=100\nArmor=heavy\n\
             [CARRIER]\nStrength=100\nArmor=heavy\nPrimary=ReturnGun\nSpawns=HORNET\nSpawnsNumber=1\n\
             [SLAVEMASTER]\nStrength=100\nArmor=heavy\nPrimary=ReturnGun\nEnslaves=SLAV\nSlavesNumber=1\n\
             [HORNET]\nStrength=75\nArmor=light\n\
             [SLAV]\nStrength=100\nArmor=none\n\
             [ReturnGun]\nDamage=1\nRange=8\nWarhead=ReturnWH\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [ReturnWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("manager identity fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type_name = if slave_manager_shaped {
            "SLAVEMASTER"
        } else {
            "CARRIER"
        };
        let victim_type = interner.intern(victim_type_name);
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let mut source = make_entity(1, "SOURCE", 6, 5, 100);
        source.owner = source_owner;
        source.type_ref = source_type;
        source.lifecycle.in_limbo = false;
        source.lifecycle.cell_marked = true;
        entities.insert(source);

        let mut victim = make_entity(2, victim_type_name, 8, 5, 100);
        victim.owner = victim_owner;
        victim.type_ref = victim_type;
        victim.lifecycle.in_limbo = false;
        victim.lifecycle.cell_marked = true;
        victim.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        if !slave_manager_shaped {
            victim.spawn_manager = crate::sim::spawn_manager::init_spawn_manager(
                rules.object("CARRIER").expect("carrier type"),
                &rules,
                &mut interner,
                0,
            );
            assert!(victim.spawn_manager.is_some(), "live SpawnManager fixture");
        } else {
            assert!(
                rules
                    .object("SLAVEMASTER")
                    .and_then(|object| object.enslaves.as_deref())
                    .is_some_and(|slave| rules.object_case_insensitive(slave).is_some()),
                "resolved Enslaves profile creates native SlaveManager"
            );
        }
        entities.insert(victim);

        let event = EntityDamageEvent::area(2, 1, 0, 1, Some(source_owner), hit_wh);
        let mut occupancy = OccupancyGrid::new();
        let mut main_rng = SimRng::new(5);
        let mut scenario_rng = SimRng::new(7);
        let mut handled_deaths = Vec::new();
        let mut houses = BTreeMap::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let _ = commit_damage_events(
            &[event],
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );
        let victim = entities.get(2).expect("victim survives");
        assert_eq!(victim.health.current, 99);
        (
            victim.mission.current(),
            victim.mission.suspended(),
            victim.attack_target.as_ref().map(|target| target.target),
            victim.spawn_manager.is_some(),
        )
    }

    let carrier = run(false);
    assert_eq!(carrier.0, MissionId::from_known(MissionType::Guard));
    assert_eq!(carrier.1, MissionId::NONE, "no override archives Guard");
    assert_eq!(carrier.2, None);
    assert!(carrier.3, "rejection preserves the live SpawnManager");

    let slave_master = run(true);
    assert_eq!(slave_master.0, MissionId::from_known(MissionType::Guard));
    assert_eq!(
        slave_master.1,
        MissionId::NONE,
        "no override archives Guard"
    );
    assert_eq!(slave_master.2, None);
    assert!(!slave_master.3);
}

#[test]
fn gsi_04_07_damage_full_capture_manager_blocks_retaliation() {
    struct Outcome {
        health: i32,
        mission: MissionId,
        suspended: MissionId,
        target: Option<TargetKind>,
        links: Vec<u64>,
    }

    fn run(link_count: usize) -> Outcome {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=SOURCE\n1=LINK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=YAPSYT\n\
             [Warheads]\n0=HitWH\n1=Controller\n\
             [SOURCE]\nStrength=100\nArmor=heavy\n\
             [LINK]\nStrength=100\nArmor=heavy\n\
             [YAPSYT]\nStrength=100\nArmor=heavy\nPrimary=MultipleMindControlTower\n\
             [MultipleMindControlTower]\nDamage=3\nRange=7\nWarhead=Controller\n\
             [Controller]\nMindControl=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Psychic Tower manager fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type = interner.intern("YAPSYT");
        let link_type = interner.intern("LINK");
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let mut source = make_entity(1, "SOURCE", 6, 5, 100);
        source.owner = source_owner;
        source.type_ref = source_type;
        source.lifecycle.in_limbo = false;
        source.lifecycle.cell_marked = true;
        entities.insert(source);

        let mut victim = make_structure_entity(2, "YAPSYT", 8, 5, 100, 100);
        victim.owner = victim_owner;
        victim.type_ref = victim_type;
        victim.lifecycle.in_limbo = false;
        victim.lifecycle.cell_marked = true;
        victim.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        let links: Vec<u64> = (0..link_count).map(|offset| 10 + offset as u64).collect();
        for (offset, &id) in links.iter().enumerate() {
            let mut controlled = make_entity(id, "LINK", 10 + offset as u16, 5, 100);
            controlled.owner = victim_owner;
            controlled.type_ref = link_type;
            controlled.mind_control =
                crate::sim::capture_manager::MindControlLink::controlled_by_for_test(2);
            controlled.lifecycle.in_limbo = false;
            controlled.lifecycle.cell_marked = true;
            entities.insert(controlled);
        }
        // The stock tower's link limit (`[MultipleMindControlTower] Damage=3`).
        victim.capture_manager = Some(
            crate::sim::capture_manager::CaptureManagerState::with_victims_for_test(
                3, false, &links,
            ),
        );
        entities.insert(victim);

        let event = EntityDamageEvent::area(2, 1, 0, 1, Some(source_owner), hit_wh);
        let mut occupancy = OccupancyGrid::new();
        let mut main_rng = SimRng::new(5);
        let mut scenario_rng = SimRng::new(7);
        let mut handled_deaths = Vec::new();
        let mut houses = BTreeMap::new();
        let mut fatal_lifecycle = None;
        let mut sound_sink = None;
        let _ = commit_damage_events(
            &[event],
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &[],
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            0,
            &mut fatal_lifecycle,
            &mut sound_sink,
        );

        let victim = entities.get(2).expect("tower survives");
        Outcome {
            health: victim.health.current,
            mission: victim.mission.current(),
            suspended: victim.mission.suspended(),
            target: victim.attack_target.as_ref().map(|target| target.target),
            links: victim
                .capture_manager
                .as_ref()
                .expect("manager retained")
                .victims()
                .collect(),
        }
    }

    let full = run(3);
    assert_eq!(full.health, 99, "receiver still commits the hostile hit");
    assert_eq!(full.mission, MissionId::from_known(MissionType::Guard));
    assert_eq!(full.suspended, MissionId::NONE);
    assert_eq!(full.target, None, "full manager rejects Override");
    assert_eq!(full.links, vec![10, 11, 12]);

    let below_capacity = run(2);
    assert_eq!(below_capacity.health, 99);
    assert_eq!(
        below_capacity.mission,
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        below_capacity.suspended,
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(below_capacity.target, Some(TargetKind::Entity(1)));
    assert_eq!(below_capacity.links, vec![10, 11]);
}

#[test]
fn gsi_04_07_damage_repair_bullet_cellspread_zero_keeps_signed_area_record() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=REPAIRER\n1=TARGET\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [REPAIRER]\nStrength=100\nArmor=light\nPrimary=RepairBullet\n\
         [TARGET]\nStrength=180\nArmor=heavy\n\
         [RepairBullet]\nDamage=-50\nROF=80\nRange=1.8\nProjectile=Invisible\nSpeed=100\nWarhead=Mechanical\n\
         [Mechanical]\nVerses=0%,0%,0%,100%,100%,100%,0%,0%,0%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("RepairBullet/Mechanical rules");
    let weapon = rules.weapon("RepairBullet").expect("RepairBullet");
    let warhead = rules.warhead("Mechanical").expect("Mechanical");
    assert_eq!(weapon.damage, -50);
    assert_eq!(warhead.cell_spread_f64, 0.0);

    let mut entities = EntityStore::new();
    let mut target = make_entity(10, "TARGET", 8, 5, 180);
    target.health.current = 100;
    entities.insert(target);
    let mut occupancy = OccupancyGrid::new();
    let mut interner = test_interner();
    let warhead_ref = interner.intern("Mechanical");
    let weapon_ref = interner.intern("RepairBullet");
    let detonation = ProjectileDetonation {
        projectile_id: 1,
        source_id: 77,
        target: ProjectileTarget::Entity(10),
        impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
        payload: ProjectilePayload {
            base_damage: weapon.damage,
            warhead: warhead_ref,
            weapon: weapon_ref,
        },
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut scenario_rng = SimRng::new(9);
    let mut emitted = CombatEmit::default();
    let mut inline_hooks = None;
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    emit_projectile_detonations(
        &[detonation],
        &mut entities,
        &occupancy,
        &rules,
        &mut interner,
        Some(handles),
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        &HouseAllianceMap::new(),
        &mut scenario_rng,
        &mut inline_hooks,
        &mut emitted,
    );
    // Firer 77 is gone, so the bullet has no Owner and DamageArea takes no
    // source house (`0x00469A69..0x00469A75`).
    let mut expected = EntityDamageEvent::area(10, -50, 0, 77, None, warhead_ref);
    expected.near_center_ic_isolation_eligible = true;
    assert_eq!(
        emitted.damage_events,
        vec![combat_aoe::AreaDamageReceiver::Entity(expected)],
        "CellSpread=0 still enters the center receiver scan with raw signed damage"
    );

    // The detonation commits its receivers inline: one heal of 50.
    assert_eq!(entities.get(10).unwrap().health.current, 150);
}

/// `Apply_area_damage`'s dispatch skips an `InvisibleInGame=` building
/// (BuildingType `+0x1701`, `0x00489A1B..0x00489A29`), such as the invisible
/// light posts, while a visible building in the same blast takes its record.
#[test]
fn gsi_04_07_area_dispatch_skips_an_invisible_in_game_building() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=LAMP\n1=SHED\n\
         [LAMP]\nStrength=6000\nArmor=wood\nInvisibleInGame=yes\n\
         [SHED]\nStrength=300\nArmor=wood\n\
         [Warheads]\n0=HE\n\
         [HE]\nCellSpread=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("invisible-building fixture parses");
    let mut entities = EntityStore::new();
    entities.insert(make_structure_entity(1, "LAMP", 5, 5, 6000, 6000));
    entities.insert(make_structure_entity(2, "SHED", 6, 5, 300, 300));
    let mut interner = test_interner();
    let warhead = interner.intern("HE");
    let house = Some(interner.intern("Test"));
    let receivers = [1, 2].map(|id| {
        combat_aoe::AreaDamageReceiver::Entity(EntityDamageEvent::area(
            id, 100, 0, 77, house, warhead,
        ))
    });
    let mut main_rng = SimRng::new(3);
    let mut scenario_rng = SimRng::new(4);
    let mut handled_deaths = Vec::new();
    let mut houses = BTreeMap::new();
    let mut fatal_lifecycle = None;
    let mut sound_sink = None;
    commit_area_damage_receivers(
        &receivers,
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        &mut houses,
        &[],
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert_eq!(entities.get(1).unwrap().health.current, 6000);
    assert_eq!(entities.get(2).unwrap().health.current, 200);
}

#[test]
fn gsi_04_07_damage_receiver_updates_grudge_before_retaliation() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nConditionRed=25%\n\
         [VehicleTypes]\n0=TARGET\n1=ZERO\n2=SOURCE\n\
         [Warheads]\n0=HitWH\n1=ZeroWH\n2=ReturnWH\n\
         [TARGET]\nStrength=1000\nCost=700\nArmor=heavy\nPrimary=ReturnGun\nCanRetaliate=yes\n\
         [ZERO]\nStrength=1000\nCost=700\nArmor=heavy\nPrimary=ReturnGun\nCanRetaliate=yes\n\
         [SOURCE]\nStrength=1000\nCost=300\nArmor=heavy\n\
         [HitWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ZeroWH]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n\
         [ReturnGun]\nDamage=1\nROF=1\nRange=8\nWarhead=ReturnWH\n\
         [ReturnWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Guard]\nRetaliate=yes\n",
    ))
    .expect("anger feedback rules parse");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "SOURCE", 7, 5, 1000, "B");
    source.lifecycle.in_limbo = false;
    entities.insert(source);
    let mut damaged = make_entity_owned(2, "TARGET", 5, 5, 1000, "A");
    damaged.lifecycle.in_limbo = false;
    entities.insert(damaged);
    let mut zero = make_entity_owned(3, "ZERO", 6, 5, 1000, "A");
    zero.lifecycle.in_limbo = false;
    entities.insert(zero);

    // `GameEntity::test_default` interns through the thread-local test
    // registry. Clone it only after constructing the fixture so every entity
    // type/owner ID resolves through the receiver's local registry too.
    let mut interner = test_interner();
    let victim_house = interner.intern("A");
    let source_house = interner.intern("B");
    let hit_wh = interner.intern("HitWH");
    let zero_wh = interner.intern("ZeroWH");

    let mut houses = BTreeMap::from([
        (
            victim_house,
            HouseState::new(victim_house, 0, None, false, 0, 10),
        ),
        (
            source_house,
            HouseState::new(source_house, 1, None, false, 0, 10),
        ),
    ]);
    let house_order = [victim_house, source_house];
    let threat_persistence = |houses: &BTreeMap<InternedId, HouseState>| {
        let victim = &houses[&victim_house];
        let snapshot = bincode::serialize(&(victim.grudge_scores.clone(), victim.enemy_house))
            .expect("threat state serializes");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&victim.grudge_scores.len(), &mut hasher);
        for (other, score) in &victim.grudge_scores {
            std::hash::Hash::hash(other, &mut hasher);
            std::hash::Hash::hash(score, &mut hasher);
        }
        std::hash::Hash::hash(&victim.enemy_house, &mut hasher);
        (snapshot, std::hash::Hasher::finish(&hasher))
    };
    let mut occupancy = OccupancyGrid::new();
    let mut main_rng = SimRng::new(5);
    let mut scenario_rng = SimRng::new(7);
    let mut handled_deaths = Vec::new();
    let mut fatal_lifecycle = None;
    let mut sound_sink = None;
    let threat_before_zero = threat_persistence(&houses);
    let (zero_death, _) = commit_damage_events(
        &[EntityDamageEvent::area(
            3,
            500,
            0,
            1,
            Some(source_house),
            zero_wh,
        )],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &house_order,
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert_eq!(entities.get(3).unwrap().health.current, 1000);
    assert_eq!(threat_persistence(&houses), threat_before_zero);
    let victim = &houses[&victim_house];
    assert!(!victim.grudge_scores.contains_key(&source_house));
    assert_eq!(victim.enemy_house, None);
    assert_eq!(
        zero_death.receiver_stage_trace,
        [
            ReceiverStageTrace::HouseThreat {
                target_id: 3,
                delta: 0,
            },
            ReceiverStageTrace::ShouldRetaliate { target_id: 3 },
        ],
        "fresh zero feedback rescans before retaliation without materializing a sparse node"
    );

    let (death, _) = commit_damage_events(
        &[EntityDamageEvent::area(
            2,
            500,
            0,
            1,
            Some(source_house),
            hit_wh,
        )],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &house_order,
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert_eq!(entities.get(2).unwrap().health.current, 500);
    let victim = &houses[&victim_house];
    assert_eq!(victim.grudge_scores.get(&source_house), Some(&350));
    assert_eq!(victim.enemy_house, Some(source_house));
    assert_eq!(
        death.receiver_stage_trace,
        [
            ReceiverStageTrace::HouseThreat {
                target_id: 2,
                delta: 350,
            },
            ReceiverStageTrace::ShouldRetaliate { target_id: 2 },
        ],
        "the first nonzero feedback materializes its sparse node before retaliation"
    );
}

#[test]
fn gsi_04_07_damage_postmortem_stock_barrel_delay_and_nested_order() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=CAMISC02\n\
         [Warheads]\n0=OilExplosionWH\n1=Super\n2=BarrelWallWH\n\
         [OverlayTypes]\n0=TESTWALL\n\
         [CombatDamage]\nC4Warhead=Super\n\
         [CAMISC02]\nStrength=5\nArmor=concrete\nCanC4=no\nExplodes=yes\n\
         EligibleForDelayKill=yes\nDeathWeapon=BarrelExplosion\n\
         [OilExplosionWH]\nCellSpread=4\nPercentAtMax=.5\nCausesDelayKill=yes\n\
         DelayKillFrames=5\nDelayKillAtMax=7.0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Super]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [BarrelExplosion]\nDamage=200\nWarhead=BarrelWallWH\n\
         [BarrelWallWH]\nCellSpread=0\nWall=yes\nWallAbsoluteDestroyer=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("stock-shaped PostMortem rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let heights = BTreeMap::new();
    let center = sim
        .spawn_object("CAMISC02", "Neutral", 8, 5, 0, &rules, &heights)
        .expect("center barrel");
    let middle = sim
        .spawn_object("CAMISC02", "Neutral", 10, 5, 0, &rules, &heights)
        .expect("middle barrel");
    let edge = sim
        .spawn_object("CAMISC02", "Neutral", 12, 5, 0, &rules, &heights)
        .expect("edge barrel");
    sim.substrate
        .entities
        .get_mut(edge)
        .unwrap()
        .pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
        start_frame: 0,
        duration_frames: 40,
        source_entity_id: Some(center),
    });
    sim.overlay_grid = Some(OverlayGrid::new(20, 12));
    sim.overlay_grid.as_mut().unwrap().place_overlay(8, 5, 0, 0);
    let oil_wh = sim.interner.intern("OilExplosionWH");

    sim.commit_noncombat_aoe_hits(
        &rules,
        Some(&registry),
        &[
            EntityDamageEvent::area(center, 10, 0, RAD_NO_ATTACKER, None, oil_wh),
            EntityDamageEvent::area(middle, 10, 512, RAD_NO_ATTACKER, None, oil_wh),
            EntityDamageEvent::area(edge, 10, 1024, RAD_NO_ATTACKER, None, oil_wh),
        ],
    );

    let pending = |sim: &crate::sim::world::Simulation, id| {
        sim.substrate
            .entities
            .get(id)
            .and_then(|entity| entity.pending_c4_detonation)
            .expect("qualifying fatal barrel becomes PostMortem")
    };
    assert_eq!(pending(&sim, center).duration_frames, 5);
    assert_eq!(pending(&sim, middle).duration_frames, 20);
    assert_eq!(pending(&sim, edge).duration_frames, 35);
    assert_eq!(
        pending(&sim, edge).source_entity_id,
        None,
        "center's exact-zero Destroy notification expires the retained source pointer"
    );
    for id in [center, middle, edge] {
        let barrel = sim.substrate.entities.get(id).unwrap();
        assert_eq!(barrel.health.current, 1);
        assert!(barrel.lifecycle.object_alive && !barrel.dying);
    }
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        Some(0)
    );

    // IC/FS uses the Building wrapper and cancels the shared timer outright.
    crate::sim::superweapon::invulnerability::apply_invulnerability(
        sim.substrate.entities.get_mut(middle).unwrap(),
        0,
        30,
        crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
    );
    assert!(
        sim.substrate
            .entities
            .get(middle)
            .unwrap()
            .pending_c4_detonation
            .is_none()
    );

    sim.session.binary_frame = 4;
    sim.tick_pending_building_detonation(center, &rules, Some(&registry));
    assert!(sim.substrate.entities.get(center).is_some());
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        Some(0)
    );

    sim.session.binary_frame = 5;
    sim.tick_pending_building_detonation(center, &rules, Some(&registry));
    let expired = sim
        .substrate
        .entities
        .get(center)
        .expect("UnInit keeps physical storage until the pending-delete drain");
    assert_eq!(expired.health.current, 0);
    assert!(expired.dying && !expired.lifecycle.object_alive);
    assert!(!expired.in_logic_vector);
    assert!(!sim.substrate.occupancy.contains_entity(8, 5, center));
    assert!(sim.substrate.pending_delete.contains(&center));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        None,
        "the barrel DeathWeapon removes the wall in the same expiry transaction"
    );
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(center).is_none());
}

#[test]
fn gsi_04_07_damage_postmortem_exact_zero_callbacks_precede_restore() {
    use crate::sim::world::LifecycleTestEvent;

    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=BARREL\n\
         [Warheads]\n0=DelayWH\n\
         [SOURCE]\nStrength=100\nArmor=heavy\nCost=100\n\
         [BARREL]\nStrength=5\nArmor=concrete\nCost=700\nCanC4=yes\n\
         EligibleForDelayKill=yes\n\
         [DelayWH]\nCellSpread=1\nPercentAtMax=1\nCausesDelayKill=yes\n\
         DelayKillFrames=5\nDelayKillAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("PostMortem callback rules");
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let source_id = sim
        .spawn_object("SOURCE", "SourceHouse", 6, 5, 0, &rules, &heights)
        .expect("source spawns");
    let target_id = sim
        .spawn_object("BARREL", "VictimHouse", 8, 5, 0, &rules, &heights)
        .expect("eligible target spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner;
    let victim_owner = sim.substrate.entities.get(target_id).unwrap().owner;
    sim.houses.insert(
        source_owner,
        HouseState::new(source_owner, 0, None, false, 0, 10),
    );
    sim.houses.insert(
        victim_owner,
        HouseState::new(victim_owner, 1, None, false, 0, 10),
    );
    sim.session.house_order.extend([source_owner, victim_owner]);
    let arm_source_target = |sim: &mut crate::sim::world::Simulation| {
        let source = sim.substrate.entities.get_mut(source_id).unwrap();
        source.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::from_known(MissionType::Guard),
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        source.attack_target = Some(AttackTarget::new(target_id));
    };
    arm_source_target(&mut sim);
    sim.substrate.entities.get_mut(target_id).unwrap().selected = true;
    let delay_wh = sim.interner.intern("DelayWH");

    let hit = |sim: &mut crate::sim::world::Simulation| {
        sim.commit_noncombat_aoe_hits(
            &rules,
            None,
            &[EntityDamageEvent::area(
                target_id,
                10,
                0,
                source_id,
                Some(source_owner),
                delay_wh,
            )],
        );
    };
    hit(&mut sim);

    let pending = sim
        .substrate
        .entities
        .get(target_id)
        .unwrap()
        .pending_c4_detonation
        .expect("PostMortem arms the shared timer after callbacks");
    assert_eq!(pending.start_frame, 0);
    assert_eq!(pending.duration_frames, 5);
    assert_eq!(pending.source_entity_id, None);
    let target = sim.substrate.entities.get(target_id).unwrap();
    assert_eq!(target.health.current, 1);
    assert!(target.lifecycle.object_alive && !target.lifecycle.in_limbo);
    assert!(target.in_logic_vector && target.lifecycle.cell_marked);
    assert!(
        !target.selected,
        "ObjectClass::Detach_All(1) deselects before detach"
    );
    assert!(sim.substrate.occupancy.contains_entity(8, 5, target_id));
    assert!(!sim.substrate.pending_delete.contains(&target_id));
    assert_eq!(
        target.killed_by, None,
        "the synchronous callback consumes attribution before HP1 restore"
    );
    let source = sim.substrate.entities.get(source_id).unwrap();
    assert_eq!(source.mission.current().known(), Some(MissionType::Guard));
    assert!(source.attack_target.is_none());
    assert_eq!(sim.houses[&victim_owner].stats.buildings_lost, 1);
    assert_eq!(sim.houses[&source_owner].stats.buildings_killed, 1);
    assert_eq!(sim.houses[&source_owner].stats.score_points, 700);

    let events = sim.lifecycle_test_events_for_test();
    let kill_index = events
        .iter()
        .position(|event| {
            *event
                == (LifecycleTestEvent::PostMortemKillBookkeeping {
                    stable_id: target_id,
                })
        })
        .expect("kill callback was traced");
    let destroy_index = events
        .iter()
        .position(|event| {
            *event
                == LifecycleTestEvent::DestroyNotifyBoundary {
                    stable_id: target_id,
                }
        })
        .expect("Destroy notify boundary was traced");
    let radio_index = events
        .iter()
        .position(|event| {
            *event
                == (LifecycleTestEvent::DestroyRadioBreakCompleted {
                    stable_id: target_id,
                })
        })
        .expect("Building Destroy broadcasts BREAK");
    let deselect_index = events
        .iter()
        .position(|event| {
            *event
                == (LifecycleTestEvent::DestroyDeselected {
                    stable_id: target_id,
                })
        })
        .expect("Object Destroy deselects");
    let source_detach_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id,
                    listener_id,
                    target_alive: true,
                    target_in_limbo: false,
                } if *expired_id == target_id && *listener_id == source_id
            )
        })
        .expect("represented source receives pointer expiry");
    assert!(
        kill_index < radio_index
            && radio_index < deselect_index
            && deselect_index < destroy_index
            && destroy_index < source_detach_index
    );
    assert!(!events.iter().any(|event| {
        matches!(
            event,
            LifecycleTestEvent::UninitClassPre { stable_id }
                | LifecycleTestEvent::UninitAliveCleared { stable_id }
                | LifecycleTestEvent::PendingDeleteQueued { stable_id }
                if *stable_id == target_id
        )
    }));

    // A later equal candidate is longer than the four frames remaining. Native
    // keeps the original timer bytes, but reruns Object's exact-zero callbacks.
    let original_pending = pending;
    sim.session.binary_frame = 1;
    arm_source_target(&mut sim);
    sim.clear_lifecycle_test_events_for_test();
    hit(&mut sim);
    let target = sim.substrate.entities.get(target_id).unwrap();
    assert_eq!(target.health.current, 1);
    assert_eq!(target.pending_c4_detonation, Some(original_pending));
    assert!(target.lifecycle.object_alive && target.in_logic_vector);
    assert!(sim.substrate.occupancy.contains_entity(8, 5, target_id));
    assert!(!sim.substrate.pending_delete.contains(&target_id));
    assert_eq!(sim.houses[&victim_owner].stats.buildings_lost, 2);
    assert_eq!(sim.houses[&source_owner].stats.buildings_killed, 2);
    assert_eq!(sim.houses[&source_owner].stats.score_points, 1_400);
    assert!(sim.lifecycle_test_events_for_test().iter().any(|event| {
        *event
            == (LifecycleTestEvent::PostMortemKillBookkeeping {
                stable_id: target_id,
            })
    }));
    assert!(
        sim.substrate
            .entities
            .get(source_id)
            .unwrap()
            .attack_target
            .is_none()
    );
}

#[test]
fn gsi_04_07_damage_postmortem_fresh_null_expiry_does_not_recredit_initial_killer() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCEA\n1=SOURCEB\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=BARREL\n\
         [Warheads]\n0=DelayWH\n1=OrdinaryWH\n2=Super\n\
         [CombatDamage]\nC4Warhead=Super\n\
         [SOURCEA]\nStrength=100\nArmor=heavy\n\
         [SOURCEB]\nStrength=100\nArmor=heavy\n\
         [BARREL]\nStrength=5\nArmor=concrete\nCost=700\nCanC4=yes\n\
         EligibleForDelayKill=yes\n\
         [DelayWH]\nCellSpread=1\nPercentAtMax=1\nCausesDelayKill=yes\n\
         DelayKillFrames=5\nDelayKillAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [OrdinaryWH]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Super]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("fresh PostMortem attribution rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let heights = BTreeMap::new();
    let source_a = sim
        .spawn_object("SOURCEA", "HouseA", 4, 5, 0, &rules, &heights)
        .expect("initial source spawns");
    let source_b = sim
        .spawn_object("SOURCEB", "HouseB", 5, 5, 0, &rules, &heights)
        .expect("later source spawns");
    let expiry_target = sim
        .spawn_object("BARREL", "VictimHouse", 8, 5, 0, &rules, &heights)
        .expect("expiry target spawns");
    let later_target = sim
        .spawn_object("BARREL", "VictimHouse", 10, 5, 0, &rules, &heights)
        .expect("later ordinary target spawns");
    let owner_a = sim.substrate.entities.get(source_a).unwrap().owner;
    let owner_b = sim.substrate.entities.get(source_b).unwrap().owner;
    let victim_owner = sim.substrate.entities.get(expiry_target).unwrap().owner;
    for (index, owner) in [owner_a, owner_b, victim_owner].into_iter().enumerate() {
        sim.houses.insert(
            owner,
            HouseState::new(owner, index as u8, None, false, 0, 10),
        );
        sim.session.house_order.push(owner);
    }
    let delay_wh = sim.interner.intern("DelayWH");
    let ordinary_wh = sim.interner.intern("OrdinaryWH");

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[
            EntityDamageEvent::area(expiry_target, 10, 0, source_a, Some(owner_a), delay_wh),
            EntityDamageEvent::area(later_target, 10, 0, source_a, Some(owner_a), delay_wh),
        ],
    );
    assert_eq!(sim.houses[&owner_a].stats.buildings_killed, 2);
    assert_eq!(sim.houses[&owner_a].stats.score_points, 1_400);
    for target_id in [expiry_target, later_target] {
        let target = sim.substrate.entities.get(target_id).unwrap();
        assert_eq!(target.health.current, 1);
        assert_eq!(target.killed_by, None);
        assert_eq!(target.kill_award_points, 0);
    }

    // A different ordinary fatal transaction after restoration must not be
    // blocked by the consumed initial callback attribution.
    sim.session.binary_frame = 1;
    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            later_target,
            10,
            0,
            source_b,
            Some(owner_b),
            ordinary_wh,
        )],
    );
    let later = sim.substrate.entities.get(later_target).unwrap();
    assert_eq!(later.health.current, 0);
    assert_eq!(later.killed_by, Some(owner_b));
    assert_eq!(sim.houses[&owner_b].stats.buildings_killed, 1);
    assert_eq!(sim.houses[&owner_b].stats.score_points, 700);

    let initial_killer_before_expiry = (
        sim.houses[&owner_a].stats.buildings_killed,
        sim.houses[&owner_a].stats.score_points,
    );
    sim.session.binary_frame = 5;
    sim.tick_pending_building_detonation(expiry_target, &rules, None);
    let expired = sim.substrate.entities.get(expiry_target).unwrap();
    assert_eq!(expired.health.current, 0);
    assert_eq!(expired.killed_by, None);
    assert!(sim.substrate.pending_delete.contains(&expiry_target));
    assert_eq!(
        (
            sim.houses[&owner_a].stats.buildings_killed,
            sim.houses[&owner_a].stats.score_points,
        ),
        initial_killer_before_expiry,
        "fresh PostMortem expiry packet is sourceless and cannot recredit HouseA"
    );
}

#[test]
fn gsi_04_07_damage_death_weapon_gate_selection_and_native_damage() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=FV\n1=NANRCT\n2=SLOTGATE\n3=DEFAULTED\n4=CURRENT\n5=EARLYDEFAULT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [FV]\nStrength=200\nArmor=light\nPrimary=HoverMissile\nDeathWeapon=CRNuke\n\
         [NANRCT]\nStrength=1000\nArmor=concrete\nExplodes=yes\nDeathWeapon=NukePayload\nDeathWeaponDamageModifier=.5\n\
         [SLOTGATE]\nStrength=100\nArmor=light\nPrimary=Ordinary\nSecondary=SuicideGun\nDeathWeapon=SlotBoom\n\
         [DEFAULTED]\nStrength=601\nArmor=light\nExplodes=yes\n\
         [CURRENT]\nStrength=100\nArmor=light\nExplodes=yes\nPrimary=Ordinary\nDeathWeaponDamageModifier=.5\n\
         [EARLYDEFAULT]\nStrength=1\nSecondary=DefaultDeath\n\
         [HoverMissile]\nDamage=25\nWarhead=OrdinaryWH\n\
         [CRNuke]\nDamage=999\nWarhead=NukeWH\n\
         [NukePayload]\nDamage=600\nWarhead=NukeWH\n\
         [Ordinary]\nDamage=40\nWarhead=OrdinaryWH\n\
         [SuicideGun]\nDamage=40\nWarhead=OrdinaryWH\nSuicide=yes\n\
         [SlotBoom]\nDamage=225\nWarhead=SlotWH\n\
         [DefaultDeath]\nDamage=999\nWarhead=DefaultWH\n\
         [CombatDamage]\nDeathWeapon=DefaultDeath\n\
         [OrdinaryWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [NukeWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [SlotWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [DefaultWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("death-weapon producer rules");
    let mut interner = test_interner();

    assert_eq!(
        death_weapon_aoe(
            &rules,
            rules.object("FV").unwrap(),
            0,
            0,
            None,
            &mut interner,
        ),
        None,
        "DeathWeapon without Explodes/current Suicide is inert (stock FV shape)"
    );

    let (nanrct_damage, nanrct_wh, nanrct_weapon) = death_weapon_aoe(
        &rules,
        rules.object("NANRCT").unwrap(),
        0,
        0,
        None,
        &mut interner,
    )
    .unwrap();
    assert_eq!(nanrct_damage, 300, "ftol(600 * 0.5f) must be 300");
    assert_eq!(interner.resolve(nanrct_wh), "NukeWH");
    assert_eq!(interner.resolve(nanrct_weapon), "NukePayload");

    assert_eq!(
        death_weapon_aoe(
            &rules,
            rules.object("SLOTGATE").unwrap(),
            0,
            0,
            None,
            &mut interner,
        ),
        None,
        "ordinary current Primary does not admit the helper"
    );
    let selected_suicide = interner.intern("SuicideGun");
    let (suicide_damage, suicide_wh, suicide_weapon) = death_weapon_aoe(
        &rules,
        rules.object("SLOTGATE").unwrap(),
        0,
        0,
        Some(selected_suicide),
        &mut interner,
    )
    .unwrap();
    assert_eq!(suicide_damage, 225);
    assert_eq!(interner.resolve(suicide_wh), "SlotWH");
    assert_eq!(interner.resolve(suicide_weapon), "SlotBoom");

    let (current_damage, current_wh, current_weapon) = death_weapon_aoe(
        &rules,
        rules.object("CURRENT").unwrap(),
        0,
        0,
        None,
        &mut interner,
    )
    .unwrap();
    assert_eq!(current_damage, 20);
    assert_eq!(interner.resolve(current_wh), "OrdinaryWH");
    assert_eq!(interner.resolve(current_weapon), "Ordinary");

    let (default_damage, default_wh, default_weapon) = death_weapon_aoe(
        &rules,
        rules.object("DEFAULTED").unwrap(),
        0,
        0,
        None,
        &mut interner,
    )
    .unwrap();
    assert_eq!(
        default_damage, 300,
        "Rules fallback is ftol(Strength * 0.5)"
    );
    assert_eq!(interner.resolve(default_wh), "DefaultWH");
    assert_eq!(interner.resolve(default_weapon), "DefaultDeath");
}

#[test]
fn gsi_08_05_tick_combat_respects_the_jittered_cooldown() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    // One call per frame: the rearm countdown is frame-anchored.
    let mut fire_at =
        |frame: u32, store: &mut EntityStore, interner: &mut StringInterner, rng: &mut SimRng| {
            align_attackers_to_targets(store, &rules, interner);
            tick_combat(
                store,
                &mut OccupancyGrid::new(),
                &rules,
                interner,
                u64::from(frame),
                100,
                frame,
                rng,
            );
        };

    // First shot fires immediately (no reload running).
    fire_at(0, &mut store, &mut interner, &mut main_rng);
    let h1: i32 = store.get(2).unwrap().health.current;
    assert!(h1 < 300, "the first shot lands at once");

    // `TechnoClass::GetROF @ 0x006FCFA0` returns `ROF + RandomRanged(0, 2)`,
    // so a `ROF=50` weapon reloads in 50, 51 or 52 frames — the exact value is
    // drawn, which is why this test reads the armed countdown instead of
    // hardcoding a frame number.
    let cooldown = store.get(1).unwrap().rearm_timer.duration() as u32;
    assert!(
        (50..=52).contains(&cooldown),
        "ROF=50 must reload in 50..=52 frames, got {cooldown}"
    );

    // Every frame before the countdown ends leaves the target untouched.
    for frame in 1..cooldown {
        fire_at(frame, &mut store, &mut interner, &mut main_rng);
    }
    assert_eq!(
        store.get(2).unwrap().health.current,
        h1,
        "no second shot before the countdown reaches zero"
    );
    assert_eq!(
        store
            .get(1)
            .unwrap()
            .rearm_timer
            .remaining(cooldown as i32 - 1),
        1
    );

    // The countdown's last frame fires.
    fire_at(cooldown, &mut store, &mut interner, &mut main_rng);
    assert!(
        store.get(2).unwrap().health.current < h1,
        "the shot lands on the frame the countdown clears"
    );
}

fn selected_death_sounds_for(
    rules: &RuleSet,
    owner_is_human: bool,
    main_rng: &mut SimRng,
) -> Vec<String> {
    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(2, "E1", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    let owner = test_intern("Americans");
    let mut houses = BTreeMap::from([(
        owner,
        HouseState::new(owner, 0, Some(owner), owner_is_human, 5_000, 10),
    )]);
    let mut scenario_rng = SimRng::new(0);
    let mut handled_deaths = Vec::new();
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    let effects = handle_entity_deaths(
        &mut entities,
        &mut OccupancyGrid::new(),
        rules,
        &mut interner,
        Some(handles),
        &mut houses,
        &[owner],
        &HouseAllianceMap::new(),
        main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        &[2],
        &[],
        None,
        None,
        None,
        &mut None,
        false,
        0,
        &mut None,
        &mut None,
    );

    effects
        .death_sounds
        .into_iter()
        .map(|(sound, _, _)| interner.resolve(sound).to_string())
        .collect()
}

/// `BuildingClass::DestructionEffects`: a dying building whose type has no
/// `DieSound=` of its own falls back to `[AudioVisual] BuildingDieSound`
/// (`0x0044173F` reads the type's `DieSound` vector count at `+0x520`,
/// `0x0044174A CMP ECX,EBX ; JNZ` skips the global when it is non-zero, and
/// `0x00441773`/`0x00441779` play `Rules+0x6E8` at the building's coordinate).
/// The fallback is not owner-gated and draws no RNG.
#[test]
fn a_building_with_no_die_sound_falls_back_to_the_global_building_die_sound() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[General]\nFlightLevel=500\n\n\
[AudioVisual]\nBuildingDieSound=BuildingGenericDie\n\n\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n1=GATECH\n\n\
[E1]\nStrength=1\nArmor=none\nDieSound=DieA\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\n\n\
[GATECH]\nStrength=750\nArmor=wood\nDieSound=OwnCrumble\n",
    ))
    .expect("building die-sound rules parse");

    let selected = |type_name: &str, category: EntityCategory| -> Vec<String> {
        let mut interner = test_interner();
        let mut rng = SimRng::new(11);
        let rng_before = rng.state();
        let mut sounds = Vec::new();
        super::append_selected_death_sounds(
            rules.object(type_name).expect("type"),
            category,
            rules.general.building_die_sound.as_deref(),
            true,
            &mut rng,
            &mut interner,
            4,
            7,
            &mut sounds,
        );
        assert_eq!(
            rng.state() == rng_before,
            rules.object(type_name).expect("type").die_sounds.is_empty(),
            "the global cue must not draw; only a non-empty DieSound= list does",
        );
        sounds
            .into_iter()
            .map(|(id, rx, ry)| {
                assert_eq!((rx, ry), (4, 7), "played at the building's own cell");
                interner.resolve(id).to_string()
            })
            .collect()
    };

    assert_eq!(
        selected("GAPOWR", EntityCategory::Structure),
        ["BuildingGenericDie"],
        "no DieSound= on the type, so the global crumble cue plays"
    );
    assert_eq!(
        selected("GATECH", EntityCategory::Structure),
        ["OwnCrumble"],
        "0x0044174A skips the global when the type has its own list"
    );
    assert_eq!(
        selected("E1", EntityCategory::Infantry),
        ["DieA"],
        "the fallback is BuildingClass-only"
    );

    // A rules file with no BuildingDieSound= leaves the building silent
    // rather than inventing a cue.
    let bare = RuleSet::from_ini(&IniFile::from_str(
        "\
[General]\nFlightLevel=500\n\n\
[InfantryTypes]\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n[GAPOWR]\nStrength=750\nArmor=wood\n",
    ))
    .expect("bare rules parse");
    let mut interner = test_interner();
    let mut rng = SimRng::new(11);
    let mut sounds = Vec::new();
    super::append_selected_death_sounds(
        bare.object("GAPOWR").expect("type"),
        EntityCategory::Structure,
        bare.general.building_die_sound.as_deref(),
        true,
        &mut rng,
        &mut interner,
        4,
        7,
        &mut sounds,
    );
    assert!(sounds.is_empty());
}

/// `BuildingClass::ReceiveDamage @ 0x00442230`: the global
/// `[AudioVisual] BuildingDamageSound` (`Rules+0x714`) is a **damage-state**
/// cue, not a per-hit one.
///
/// `0x00442476 JMP [EAX*4 + 0x00442C18]` indexes the four-entry table
/// `{0x004426AC, 0x004426C8, 0x004424A2, 0x0044247D}` with `result - 2`;
/// entry 0 (result 2, the `Strength >> 1` crossing) falls through into entry 1
/// (result 3, the `Strength * Rules+0x1708` crossing) and both reach
/// `0x004426D2 CMP [type+0x538],-1`, so a type with its own `DamageSound=`
/// takes `JNZ 0x0044270B` and the global never plays. Results 1 (no crossing)
/// and 4 (dead) leave the arm unvisited, and `0x0044242C MOV AL,[ESI+0x90]`
/// (`ObjectClass::IsAlive`) skips the whole dispatch for a corpse.
#[test]
fn a_struck_building_sounds_the_global_damage_cue_only_on_a_state_crossing() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[General]\nFlightLevel=500\n\n\
[AudioVisual]\nBuildingDamageSound=BuildingDamaged\nConditionRed=25%\n\n\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n1=GAWEAP\n\n\
[E1]\nStrength=100\nArmor=none\n\n\
[MTNK]\nStrength=300\nArmor=none\nSpeed=6\nPrimary=Plain\n\n\
[GAPOWR]\nStrength=100\nArmor=none\n\n\
[GAWEAP]\nStrength=100\nArmor=none\nDamageSound=BuildingMetalDamaged\n\n\
[Plain]\nDamage=10\nROF=20\nRange=6\nWarhead=PlainWH\n\n\
[PlainWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("building damage-sound rules parse");

    // One `commit_damage_events` transaction: entity 1 (MTNK) hits entity 2
    // for `damage`. `victim_type`/`victim_category` shape the receiver.
    let hit = |victim_type: &str,
               victim_category: EntityCategory,
               damage: i32|
     -> (Vec<SimSoundEvent>, i32) {
        // `GameEntity::test_default` interns through the thread-local test
        // interner, and `test_interner()` snapshots it — so the entities must
        // exist before the snapshot or their type ids resolve to whatever the
        // clone happens to hold at that index.
        let mut entities = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        attacker.lifecycle.in_limbo = false;
        attacker.in_playfield = true;
        entities.insert(attacker);
        let mut victim = GameEntity::test_default(2, victim_type, "Soviet", 4, 7);
        victim.category = victim_category;
        victim.health = Health { current: 100 };
        victim.lifecycle.in_limbo = false;
        victim.in_playfield = true;
        entities.insert(victim);

        let mut interner = test_interner();
        let allies = interner.intern("Americans");
        let soviet = interner.intern("Soviet");
        let warhead_ref = interner.intern("PlainWH");

        let mut houses = BTreeMap::from([
            (soviet, HouseState::new(soviet, 0, None, false, 0, 10)),
            (allies, HouseState::new(allies, 1, None, false, 0, 10)),
        ]);
        let house_order = [soviet, allies];
        let mut occupancy = OccupancyGrid::new();
        let mut main_rng = SimRng::new(11);
        let mut scenario_rng = SimRng::new(13);
        let mut handled_deaths = Vec::new();
        let mut hooks = None;
        let mut collected: Vec<SimSoundEvent> = Vec::new();
        let mut sound_sink: Option<&mut Vec<SimSoundEvent>> = Some(&mut collected);

        let records = [EntityDamageEvent::area(
            2,
            damage,
            0,
            1,
            Some(allies),
            warhead_ref,
        )];
        let _ = commit_damage_events(
            &records,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &house_order,
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            100,
            &mut hooks,
            &mut sound_sink,
        );
        let hp = entities.get(2).map_or(0, |victim| victim.health.current);
        (collected, hp)
    };

    let damage_cues = |sounds: &[SimSoundEvent]| -> Vec<(u16, u16)> {
        sounds
            .iter()
            .filter_map(|sound| match sound {
                SimSoundEvent::BuildingDamagedSfx { rx, ry } => Some((*rx, *ry)),
                _ => None,
            })
            .collect()
    };

    // 100 -> 40 crosses `Strength >> 1` = 50: result 2, entry 0 of the table,
    // and the cue plays at the building's own cell.
    let (sounds, hp) = hit("GAPOWR", EntityCategory::Structure, 60);
    assert_eq!(hp, 40);
    assert_eq!(damage_cues(&sounds), [(4, 7)]);

    // 100 -> 80 crosses nothing: result 1, which the `CMP EAX,3 ; JA` bound at
    // `0x0044246D` rejects, so the damage-sound arm is never entered.
    let (sounds, hp) = hit("GAPOWR", EntityCategory::Structure, 20);
    assert_eq!(hp, 80);
    assert!(damage_cues(&sounds).is_empty());

    // 100 -> 0 is result 4: entry 2 of the table (`0x004424A2`), which never
    // reaches `0x004426D2`. A dying building gets its crumble cue instead.
    let (sounds, _) = hit("GAPOWR", EntityCategory::Structure, 100);
    assert!(damage_cues(&sounds).is_empty());

    // `[GAWEAP] DamageSound=` is the `0x004426D2` gate: the type's own cue
    // wins and the global is skipped.
    let (sounds, hp) = hit("GAWEAP", EntityCategory::Structure, 60);
    assert_eq!(hp, 40);
    assert!(damage_cues(&sounds).is_empty());

    // The arm is BuildingClass-only — an infantryman crossing the same
    // threshold returns through `TechnoClass::ReceiveDamage`, never
    // `0x00442476`.
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 60);
    assert_eq!(hp, 40);
    assert!(damage_cues(&sounds).is_empty());
}

/// `TechnoClass::ReceiveDamage @ 0x00701900` arm `0x00702695` — index 2 of the
/// switch table at `0x00702D24`, i.e. damage result 2 only. Unlike the
/// BuildingClass cue this is a Techno-level arm, so every category reaches it;
/// and unlike it, result 3 does NOT (index 3 is `0x007027F7`, the tail).
#[test]
fn a_techno_speaks_its_voice_feedback_only_on_the_half_strength_crossing() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[General]\nFlightLevel=500\n\n\
[AudioVisual]\nBuildingDamageSound=BuildingDamaged\nConditionRed=25%\n\n\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[E1]\nStrength=100\nArmor=none\nVoiceFeedback=GIFear\n\n\
[E2]\nStrength=100\nArmor=none\n\n\
[MTNK]\nStrength=300\nArmor=none\nSpeed=6\nPrimary=Plain\n\n\
[GAPOWR]\nStrength=100\nArmor=none\nVoiceFeedback=StructureFear\n\n\
[Plain]\nDamage=10\nROF=20\nRange=6\nWarhead=PlainWH\n\n\
[PlainWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("voice-feedback rules parse");

    let hit = |victim_type: &str,
               victim_category: EntityCategory,
               damage: i32|
     -> (Vec<SimSoundEvent>, i32) {
        let mut entities = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        attacker.lifecycle.in_limbo = false;
        attacker.in_playfield = true;
        entities.insert(attacker);
        let mut victim = GameEntity::test_default(2, victim_type, "Soviet", 4, 7);
        victim.category = victim_category;
        victim.health = Health { current: 100 };
        victim.lifecycle.in_limbo = false;
        victim.in_playfield = true;
        entities.insert(victim);

        let mut interner = test_interner();
        let allies = interner.intern("Americans");
        let soviet = interner.intern("Soviet");
        let warhead_ref = interner.intern("PlainWH");

        let mut houses = BTreeMap::from([
            (soviet, HouseState::new(soviet, 0, None, false, 0, 10)),
            (allies, HouseState::new(allies, 1, None, false, 0, 10)),
        ]);
        let house_order = [soviet, allies];
        let mut occupancy = OccupancyGrid::new();
        let mut main_rng = SimRng::new(11);
        let mut scenario_rng = SimRng::new(13);
        let mut handled_deaths = Vec::new();
        let mut hooks = None;
        let mut collected: Vec<SimSoundEvent> = Vec::new();
        let mut sound_sink: Option<&mut Vec<SimSoundEvent>> = Some(&mut collected);

        let records = [EntityDamageEvent::area(
            2,
            damage,
            0,
            1,
            Some(allies),
            warhead_ref,
        )];
        let scenario_before = scenario_rng.state();
        let main_before = main_rng.state();
        let _ = commit_damage_events(
            &records,
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut houses,
            &house_order,
            &HouseAllianceMap::new(),
            &mut main_rng,
            &mut scenario_rng,
            &mut handled_deaths,
            None,
            None,
            None,
            100,
            &mut hooks,
            &mut sound_sink,
        );
        // Both native draws are on `g_MainRng @ 0x00886B88`, which per-frame
        // draw paths also consume, so the cue must cost `sim/` nothing.
        assert_eq!(
            scenario_rng.state(),
            scenario_before,
            "the damage voice must not spend a scenario draw"
        );
        assert_eq!(
            main_rng.state(),
            main_before,
            "the damage voice must not spend a sim main draw"
        );
        let hp = entities.get(2).map_or(0, |victim| victim.health.current);
        (collected, hp)
    };

    let voices = |sounds: &[SimSoundEvent]| -> Vec<(u16, u16)> {
        sounds
            .iter()
            .filter_map(|sound| match sound {
                SimSoundEvent::VoiceFeedback { rx, ry, .. } => Some((*rx, *ry)),
                _ => None,
            })
            .collect()
    };

    // 100 -> 40 crosses `Strength >> 1` = 50 without reaching ConditionRed
    // (25): result 2, index 2 of `0x00702D24`, spoken at the object's own cell
    // (`0x00702702 CALL [EDX+0x48]`).
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 60);
    assert_eq!(hp, 40);
    assert_eq!(voices(&sounds), [(4, 7)]);

    // 100 -> 80 crosses nothing: result 1, index 1 — the per-type
    // `DamageSound=` arm, never the voice.
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 20);
    assert_eq!(hp, 80);
    assert!(voices(&sounds).is_empty());

    // 100 -> 20 crosses BOTH thresholds, and `ObjectClass::ReceiveDamage @
    // 0x005F5390` lets 3 override 2. Index 3 is `0x007027F7`, the shared tail,
    // so a hit that drops a unit straight into the red says nothing.
    let (sounds, hp) = hit("E1", EntityCategory::Infantry, 80);
    assert_eq!(hp, 20);
    assert!(voices(&sounds).is_empty());

    // 100 -> 0 is forced to index 4 by `0x00702035 MOV EDI,0x4`.
    let (sounds, _) = hit("E1", EntityCategory::Infantry, 100);
    assert!(voices(&sounds).is_empty());

    // `0x007026A1 MOV EAX,[EDI+0x4E8]` / `0x007026A9 JLE`: an empty list
    // returns before the roll, so a type with no `VoiceFeedback=` is silent.
    let (sounds, hp) = hit("E2", EntityCategory::Infantry, 60);
    assert_eq!(hp, 40);
    assert!(voices(&sounds).is_empty());

    // The arm is TechnoClass-level, so a building speaks too — and the voice
    // is emitted first, because native runs it inside
    // `TechnoClass::ReceiveDamage`, which `BuildingClass::ReceiveDamage` only
    // resumes after at `0x00442425`.
    let (sounds, hp) = hit("GAPOWR", EntityCategory::Structure, 60);
    assert_eq!(hp, 40);
    assert_eq!(voices(&sounds), [(4, 7)]);
    let order: Vec<&str> = sounds
        .iter()
        .filter_map(|sound| match sound {
            SimSoundEvent::VoiceFeedback { .. } => Some("voice"),
            SimSoundEvent::BuildingDamagedSfx { .. } => Some("building"),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["voice", "building"]);
}

#[test]
fn fatal_sound_selection_uses_human_voice_then_die_sound_main_draws() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=1\nArmor=none\nVoiceDie=VoiceA,VoiceB,VoiceC\nDieSound=DieA,DieB\n\n\
[MTNK]\nStrength=100\nArmor=heavy\nPrimary=Gun\n\n\
[Gun]\nDamage=10\nROF=1\nRange=5\nWarhead=WH\n\n\
[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("death-sound rules parse");

    let mut entities = EntityStore::new();
    entities.insert(make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet"));
    entities.insert(make_entity_owned(2, "E1", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut entities, 1, 2, None, &interner);
    let owner = test_intern("Americans");
    let mut houses = BTreeMap::from([(
        owner,
        HouseState::new(owner, 0, Some(owner), true, 5_000, 10),
    )]);
    let mut sounds = Vec::new();
    let mut scenario_rng = SimRng::new(73);
    // The one attacker fires this tick, so the scenario stream advances by its
    // `GetROF` reload jitter (`RandomRanged(0, 2)` @ `0x006FD0B0`) and nothing
    // else — the death-sound choices themselves still draw only on the human
    // and main streams.
    let scenario_before = {
        let mut expected = scenario_rng.clone();
        expected.next_range_u32_inclusive(0, 2);
        expected.state()
    };
    let mut human_rng = SimRng::new(1);
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    align_attackers_to_targets(&mut entities, &rules, &interner);
    tick_combat_with_fog_and_main_rng(
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        &mut houses,
        &[owner],
        &HouseAllianceMap::new(),
        Some(&mut sounds),
        None,
        None,
        None,
        0,
        100,
        0,
        &[1, 2],
        &[],
        &[],
        None,
        &[],
        &mut scenario_rng,
        &mut human_rng,
        None,
    );
    assert_eq!(
        sounds
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::EntityDied { die_sound_id, .. } => {
                    Some(interner.resolve(*die_sound_id))
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["VoiceC", "DieA"]
    );
    assert_eq!(
        scenario_rng.state(),
        scenario_before,
        "death-sound choices must not consume Scenario RNG beyond the shot's own draws"
    );
    let mut two_draw_reference = SimRng::new(1);
    two_draw_reference.next_u32();
    two_draw_reference.next_u32();
    assert_eq!(human_rng.state(), two_draw_reference.state());

    let mut ai_rng = SimRng::new(1);
    assert_eq!(
        selected_death_sounds_for(&rules, false, &mut ai_rng),
        ["DieB"]
    );
    let mut one_draw_reference = SimRng::new(1);
    one_draw_reference.next_u32();
    assert_eq!(ai_rng.state(), one_draw_reference.state());
}

#[test]
fn fatal_sound_empty_lists_skip_draws_but_single_choices_still_draw() {
    let empty_rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=1\nArmor=none\nVoiceDie= , \nDieSound=\n",
    ))
    .expect("empty death-sound rules parse");
    let mut empty_rng = SimRng::new(1);
    let empty_before = empty_rng.state();
    assert!(selected_death_sounds_for(&empty_rules, true, &mut empty_rng).is_empty());
    assert_eq!(empty_rng.state(), empty_before);

    let single_rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[E1]\nStrength=1\nArmor=none\nVoiceDie=OnlyVoice\nDieSound=OnlyDie\n",
    ))
    .expect("single death-sound rules parse");
    let mut single_rng = SimRng::new(1);
    assert_eq!(
        selected_death_sounds_for(&single_rules, true, &mut single_rng),
        ["OnlyVoice", "OnlyDie"]
    );
    let mut two_draw_reference = SimRng::new(1);
    two_draw_reference.next_u32();
    two_draw_reference.next_u32();
    assert_eq!(single_rng.state(), two_draw_reference.state());
}

#[test]
fn test_tick_combat_out_of_range() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    // 105mm range = 6 cells. Target at distance 10.
    store.insert(make_entity(1, "MTNK", 0, 0, 300));
    store.insert(make_entity(2, "MTNK", 10, 0, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    let target_health = store.get(2).unwrap().health.current;
    assert_eq!(
        target_health, 300,
        "Out-of-range target should not take damage"
    );
    // Range failure preserves attack_target; pursuit (run from advance_tick,
    // not from tick_combat in isolation) walks the unit into range.
    assert!(
        store.get(1).unwrap().attack_target.is_some(),
        "AttackTarget preserved when out of range — pursuit closes the gap"
    );
}

#[test]
fn undeployed_guardian_gi_vs_infantry_uses_m60() {
    let rules = guardian_gi_rules();
    let mut store = EntityStore::new();
    store.insert(make_infantry_entity(1, "GGI", 0, 0, 100));
    store.insert(make_infantry_entity(2, "E2", 3, 0, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "M60");
    assert_eq!(ev.weapon_slot, WeaponSlot::Primary);
    assert_eq!(store.get(2).unwrap().health.current, 110);
}

/// `TechnoClass::FireAt 0x006FE5E2..0x006FE622`: every launched shot whose
/// BulletType is not `Inaccurate=` takes `EstimateDamage` off its TarCom's
/// retained estimate (`+0x70`), apart from the bullet's own damage.
#[test]
fn a_shot_debits_its_targets_estimate_unless_inaccurate() {
    for inaccurate in [false, true] {
        let ini = format!(
            "[InfantryTypes]\n0=GGI\n1=E2\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
             [GGI]\nStrength=100\nArmor=none\nSpeed=4\nPrimary=M60\n\n\
             [E2]\nStrength=125\nArmor=none\nSpeed=4\n\n\
             [M60]\nDamage=15\nROF=20\nRange=4\nProjectile=Shot\nWarhead=SA\n\n\
             [Shot]\nInviso=yes\nInaccurate={}\n\n\
             [SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n",
            if inaccurate { "yes" } else { "no" }
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut store = EntityStore::new();
        store.insert(make_infantry_entity(1, "GGI", 0, 0, 100));
        store.insert(make_infantry_entity(2, "E2", 3, 0, 125));
        let before = store.get(2).unwrap().estimated_health.get();
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut SimRng::new(1),
        );
        let target = store.get(2).unwrap();
        assert_eq!(target.health.current, 110, "the bullet lands either way");
        assert_eq!(
            target.estimated_health.get(),
            if inaccurate { before } else { before - 15 },
            "Inaccurate={inaccurate}"
        );
    }
}

/// `BulletClass::DetonateAtCoord 0x00469BD6..0x00469C41`: a `Bright=` weapon's
/// bullet lights its detonation with its damage, force 1 and the warhead's
/// CLDisable channels; a dim one lights nothing.
#[test]
fn a_bright_shot_lights_its_detonation() {
    for bright in [true, false] {
        let ini = format!(
            "[InfantryTypes]\n\n[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
             [SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n\
             [TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\n\
             [GUN]\nDamage=90\nROF=20\nRange=10\nProjectile=Shot\nWarhead=WH\nBright={}\n\n\
             [Shot]\nInviso=yes\n\n\
             [WH]\nCLDisableGreen=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            if bright { "yes" } else { "no" }
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut store = EntityStore::new();
        store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
        store.insert(make_entity(2, "TARGET", 8, 5, 500));
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        align_attackers_to_targets(&mut store, &rules, &interner);
        let result = tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut SimRng::new(1),
        );
        assert_eq!(result.consequences.fire_events().len(), 1);
        let lights = &result.consequences.effects().combat_light_requests;
        if bright {
            assert_eq!(lights.len(), 1);
            assert_eq!(lights[0].damage, 90);
            assert_eq!(lights[0].flags, 4, "CLDisableGreen");
            assert!(lights[0].force_create);
            assert_eq!(lights[0].target_id, None);
        } else {
            assert!(lights.is_empty());
        }
    }
}

#[test]
fn deployed_guardian_gi_vs_rhino_at_six_cells_uses_missilelauncher() {
    let rules = guardian_gi_rules();
    let mut store = EntityStore::new();
    let mut ggi = make_infantry_entity(1, "GGI", 0, 0, 100);
    ggi.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
    ggi.animation = Some(Animation::new(SequenceKind::Deployed));
    store.insert(ggi);
    store.insert(make_entity(2, "HTNK", 6, 0, 400));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "MissileLauncher");
    assert_eq!(ev.weapon_slot, WeaponSlot::Secondary);
    assert_eq!(store.get(2).unwrap().health.current, 400);
    assert_eq!(result.projectile_spawns.len(), 1);
}

#[test]
fn deployed_guardian_gi_vs_rocketeer_uses_missilelauncher() {
    let rules = guardian_gi_rules();
    let mut store = EntityStore::new();
    let mut ggi = make_infantry_entity(1, "GGI", 0, 0, 100);
    ggi.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
    ggi.animation = Some(Animation::new(SequenceKind::Deployed));
    store.insert(ggi);
    store.insert(make_infantry_entity(2, "ROCK", 6, 0, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "MissileLauncher");
    assert_eq!(ev.weapon_slot, WeaponSlot::Secondary);
}

#[test]
fn test_infantry_vs_heavy_armor() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    // E1 (M60) attacks MTNK (heavy armor).
    // M60: damage=25, warhead=SA, SA verses[heavy(5)] = 25%.
    // Integer math: 25 * 25 / 100 = 6.
    store.insert(make_entity(1, "E1", 5, 5, 125));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    let h: i32 = store.get(2).unwrap().health.current;
    assert_eq!(
        h,
        300 - 6,
        "Infantry vs heavy armor should do 6 damage (25 * 25 / 100)"
    );
}

#[test]
fn infantry_standing_fire_waits_for_fire_frame() {
    let rules = infantry_fire_frame_rules();
    let mut store = EntityStore::new();
    store.insert(make_infantry_entity(1, "E1", 5, 5, 125));
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );

    assert_eq!(store.get(2).unwrap().health.current, 125);
    assert!(result.consequences.fire_events().is_empty());
    let attack = store.get(1).unwrap().attack_target.as_ref().unwrap();
    assert_eq!(
        attack.pending_infantry_fire.unwrap(),
        PendingInfantryFire {
            sequence: SequenceKind::Attack,
            fire_frame: 2
        }
    );
    assert_eq!(
        store.get(1).unwrap().animation.as_ref().unwrap().sequence,
        SequenceKind::Attack
    );

    set_anim_frame(&mut store, 1, 1);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        1,
        100,
        0,
        &mut main_rng,
    );
    assert_eq!(store.get(2).unwrap().health.current, 125);
    assert!(result.consequences.fire_events().is_empty());

    set_anim_frame(&mut store, 1, 2);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        2,
        100,
        0,
        &mut main_rng,
    );
    assert_eq!(store.get(2).unwrap().health.current, 100);
    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "M60");
    assert_eq!(ev.weapon_slot, WeaponSlot::Primary);
    assert_eq!(
        ev.report_sound_id.map(|id| interner.resolve(id)),
        Some("GIAttack")
    );
    assert!(!ev.occupied_building);
    assert!(
        store
            .get(1)
            .unwrap()
            .attack_target
            .as_ref()
            .unwrap()
            .pending_infantry_fire
            .is_none()
    );
}

#[test]
fn prone_infantry_uses_prone_fire_sequence_and_frame() {
    let rules = infantry_fire_frame_rules();
    let mut store = EntityStore::new();
    let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
    attacker.infantry.as_mut().unwrap().is_prone = true;
    attacker.animation = Some(Animation::new(SequenceKind::Prone));
    store.insert(attacker);
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );
    let attack = store.get(1).unwrap().attack_target.as_ref().unwrap();
    assert!(result.consequences.fire_events().is_empty());
    assert_eq!(
        attack.pending_infantry_fire.unwrap(),
        PendingInfantryFire {
            sequence: SequenceKind::FireProne,
            fire_frame: 3
        }
    );
    assert_eq!(store.get(2).unwrap().health.current, 125);

    set_anim_frame(&mut store, 1, 2);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        1,
        100,
        0,
        &mut main_rng,
    );
    assert_eq!(store.get(2).unwrap().health.current, 125);
    assert!(result.consequences.fire_events().is_empty());

    set_anim_frame(&mut store, 1, 3);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        2,
        100,
        0,
        &mut main_rng,
    );
    assert_eq!(store.get(2).unwrap().health.current, 100);
    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "M60");
    assert_eq!(ev.weapon_slot, WeaponSlot::Primary);
    assert_eq!(
        ev.report_sound_id.map(|id| interner.resolve(id)),
        Some("GIAttack")
    );
}

#[test]
fn deployed_gi_uses_deployed_fire_visual_with_deploy_fire_weapon() {
    let rules = infantry_fire_frame_rules();
    let mut store = EntityStore::new();
    let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
    attacker.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
    attacker.animation = Some(Animation::new(SequenceKind::Deployed));
    store.insert(attacker);
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );
    let attack = store.get(1).unwrap().attack_target.as_ref().unwrap();
    assert!(result.consequences.fire_events().is_empty());
    assert_eq!(
        attack.pending_infantry_fire.unwrap(),
        PendingInfantryFire {
            sequence: SequenceKind::DeployedFire,
            fire_frame: 5
        }
    );
    assert_eq!(store.get(2).unwrap().health.current, 300);

    set_anim_frame(&mut store, 1, 5);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        1,
        100,
        0,
        &mut main_rng,
    );
    assert_eq!(
        store.get(2).unwrap().health.current,
        260,
        "deployed-fire should use the DeployFireWeapon secondary slot"
    );
    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert_eq!(interner.resolve(ev.weapon_id), "Para");
    assert_eq!(ev.weapon_slot, WeaponSlot::Secondary);
    assert_eq!(
        ev.report_sound_id.map(|id| interner.resolve(id)),
        Some("GIAttackDeployed")
    );
}

#[test]
fn garrison_fire_keeps_occupant_anim_and_sound_path() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]\n0=E1\n1=E2\n\n\
[VehicleTypes]\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=CAGAS\n\n\
[CAGAS]\nStrength=800\nArmor=wood\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\n\n\
[E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\nOccupyWeapon=M60\n\n\
[E2]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
[M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\nReport=GIAttack\nOccupantAnim=UCFLASH\n\n\
[SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n",
    ))
    .expect("garrison rules parse");
    let mut store = EntityStore::new();
    let mut building = make_entity(10, "CAGAS", 5, 5, 800);
    building.category = EntityCategory::Structure;
    let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 1);
    assert!(cargo.board(1, 1));
    building.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
    store.insert(building);
    let mut occupant = make_infantry_entity(1, "E1", 5, 5, 125);
    occupant.passenger_role = crate::sim::passenger::PassengerRole::Inside { transport_id: 10 };
    store.insert(occupant);
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));

    let mut interner = test_interner();
    issue_attack_command(&mut store, 10, 2, None, &interner);
    let mut sounds = Vec::new();
    let mut main_rng = SimRng::new(1);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        Some(&mut sounds),
        None,
        None,
        None,
        0,
        100,
        0,
        &[],
        None,
        &mut main_rng,
    );

    assert_eq!(result.consequences.fire_events().len(), 1);
    let ev = &result.consequences.fire_events()[0];
    assert!(ev.occupied_building);
    assert_eq!(
        ev.muzzle_anim.map(|id| interner.resolve(id)),
        Some("UCFLASH"),
        "an occupied building's shot constructs the weapon's OccupantAnim="
    );
    assert_eq!(
        ev.report_sound_id.map(|id| interner.resolve(id)),
        Some("GIAttack")
    );
    assert!(sounds.is_empty());
}

#[test]
fn delayed_infantry_fire_cancels_when_target_dies_before_fire_frame() {
    let rules = infantry_fire_frame_rules();
    let mut store = EntityStore::new();
    store.insert(make_infantry_entity(1, "E1", 5, 5, 125));
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut main_rng,
    );
    store.get_mut(2).unwrap().health.current = 0;
    set_anim_frame(&mut store, 1, 2);

    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        1,
        100,
        0,
        &mut main_rng,
    );

    assert_eq!(store.get(2).unwrap().health.current, 0);
    assert!(result.consequences.fire_events().is_empty());
    assert!(
        store.get(1).unwrap().attack_target.is_none(),
        "dead target should cancel delayed shot instead of spawning stale damage"
    );
}

#[test]
fn test_prone_infantry_takes_scaled_direct_damage() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n1=E2\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [E2]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=100\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\nProneDamage=50%\n",
    ))
    .expect("prone combat rules should parse");

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    let mut target = make_infantry_entity(2, "E2", 8, 5, 125);
    target.infantry.as_mut().unwrap().is_prone = true;
    store.insert(target);

    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    let target_health = store.get(2).expect("target alive").health.current;
    assert_eq!(
        target_health, 75,
        "100 damage with ProneDamage=50% should deal 50"
    );
}

#[test]
fn test_prone_infantry_takes_scaled_aoe_damage() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n1=E2\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [E2]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=100\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\nProneDamage=50%\n",
    ))
    .expect("prone aoe combat rules should parse");

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    let mut target = make_infantry_entity(2, "E2", 8, 5, 125);
    target.infantry.as_mut().unwrap().is_prone = true;
    store.insert(target);

    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    let target_health = store.get(2).expect("target alive").health.current;
    assert_eq!(
        target_health, 75,
        "AoE center hit should also respect ProneDamage=50%"
    );
}

#[test]
fn test_cell_distance() {
    assert!((cell_distance(0, 0, 3, 4) - 5.0).abs() < 0.01);
    assert!((cell_distance(5, 5, 5, 5) - 0.0).abs() < f32::EPSILON);
    assert!((cell_distance(0, 0, 1, 0) - 1.0).abs() < f32::EPSILON);
}

/// No fire path reads shroud or fog: GetFireError `0x006FC0B0`, the class
/// fire routines and Greatest_Threat never call `IsShrouded @ 0x00586360`,
/// and `IsFogged @ 0x005865E0` is a constant false. A target on a cell its
/// attacker's house cannot see is shot like any other.
#[test]
fn an_unseen_target_is_fired_at() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Americans"));
    store.insert(make_entity_owned(2, "MTNK", 8, 5, 300, "Soviet"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);

    let fog = FogState::default();
    assert!(!fog.is_cell_visible(test_intern("Americans"), 8, 5));
    let mut occupancy = mark_fixture_entities(&mut store);
    let mut main_rng = SimRng::new(1);
    let result = tick_combat_with_fog(
        &mut store,
        &mut occupancy,
        &rules,
        &mut interner,
        Some(&fog),
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        None,
        0u64,
        100,
        0u32,
        &[],
        None,
        &mut main_rng,
    );

    assert!(!result.consequences.fire_events().is_empty());
    assert!(store.get(2).expect("target alive").health.current < 300);
}

/// Two identical enemies share one cell. `TechnoClass::Scan_Cell_For_Target @
/// 0x006F8960` walks ONE object list and stops at the first hostile entry, so
/// the cell offers exactly one candidate and the other is never evaluated —
/// there is no stable-id tie-break anywhere in the native scan.
///
/// The list head is the most recently added non-building
/// (`CellListInsertion::PrependNonBuilding`). This fixture Marks in ascending
/// ID order, so20 is actually linked ahead of3 and the scan picks20.
#[test]
fn gsi_08_01_a_shared_cell_offers_only_its_list_head() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(10, "MTNK", 5, 5, 300, "Americans"));
    store.insert(make_entity_owned(99, "MTNK", 6, 5, 0, "Soviet")); // dead
    store.insert(make_entity_owned(20, "MTNK", 7, 5, 300, "Soviet"));
    store.insert(make_entity_owned(3, "MTNK", 7, 5, 300, "Soviet"));
    let interner = test_interner();

    let mut fog = FogState::default();
    fog.mark_visible_for_owner(test_intern("Americans"), 7, 5);
    let occupancy = mark_fixture_entities(&mut store);
    // `TechnoClass::Greatest_Threat @ 0x006F8DF0` with the passive mask.
    let pick = acquire_best_target_for_entity(
        &store,
        &occupancy,
        &rules,
        &interner,
        10,
        Some(&fog),
        None,
        false,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            overlay_grid: None,
            overlay_registry: None,
            alliances: Some(&fog.alliances),
        },
        None,
    );
    assert!(
        pick == Some(20),
        "the cell's list head is the candidate, not the lower stable id"
    );
}

#[test]
fn gsi_08_01_unarmed_building_loses_to_a_tank_at_equal_distance() {
    let rules: RuleSet = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity_owned(10, "MTNK", 5, 5, 300, "Americans"));
    store.insert(make_entity_owned(99, "MTNK", 6, 5, 0, "Soviet")); // dead
    let mut building = make_entity_owned(1, "GAPOWR", 7, 5, 750, "Soviet");
    building.category = crate::map::entities::EntityCategory::Structure;
    store.insert(building);
    store.insert(make_entity_owned(200, "MTNK", 7, 5, 300, "Soviet"));
    let interner = test_interner();

    let mut fog = FogState::default();
    fog.mark_visible_for_owner(test_intern("Americans"), 7, 5);
    let occupancy = mark_fixture_entities(&mut store);
    // `TechnoClass::Greatest_Threat @ 0x006F8DF0` with the passive mask.
    let pick = acquire_best_target_for_entity(
        &store,
        &occupancy,
        &rules,
        &interner,
        10,
        Some(&fog),
        None,
        false,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            overlay_grid: None,
            overlay_registry: None,
            alliances: Some(&fog.alliances),
        },
        None,
    );
    // Not a "threat class" tie-break — gamemd has none. `[GAPOWR]` carries no
    // weapon and `ThreatPosed=0`, so the human-attacker building gate at
    // `TechnoClass::Evaluate_Candidate @ 0x006F85AB` refuses it outright and
    // the tank is the only candidate the cell can offer that survives.
    assert!(
        pick == Some(200),
        "an unarmed enemy building is not a legal passive target for a human unit"
    );
}

// --- Ore destruction integration tests ---

/// Build a RuleSet with a CellSpread=2 AoE weapon for ore destruction testing.
fn test_rules_with_spread() -> RuleSet {
    let ini_str: &str = "\
[InfantryTypes]\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=120mm\n\n\
[120mm]\nDamage=120\nROF=50\nRange=6\nWarhead=HE\n\n\
[HE]\nCellSpread=2\nTiberium=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let ini = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("test rules should parse")
}

#[test]
fn test_weapon_fire_destroys_ore_in_spread() {
    let rules = test_rules_with_spread();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    // Place ore at the target cell and a neighbor within CellSpread=2.
    // 6 density levels of ore at target (8,5): remaining = 6 * 120 = 720.
    // 3 density levels at (9,5): remaining = 3 * 120 = 360.

    let ore_ini =
        IniFile::from_str("[OverlayTypes]\n0=ORE\n[ORE]\nTiberium=yes\nChainReaction=yes\n");
    let ore_registry = OverlayTypeRegistry::from_ini(&ore_ini, None);
    let mut overlays = OverlayGrid::new(16, 16);
    overlays.place_overlay(8, 5, 0, 5);
    overlays.place_overlay(9, 5, 0, 2);

    let mut main_rng = SimRng::new(1);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        Some(&mut overlays),
        Some(&ore_registry),
        None,
        0u64,
        100,
        0u32,
        &[],
        None,
        &mut main_rng,
    );

    // Combat emits TiberiumReductionRequests (applied later by World via the
    // shared cell reducer).
    // Damage=120 → ore_damage = 120/10 = 12 density levels at each cell within
    // CellSpread=2. Both ore cells (8,5) and (9,5) get a reduction request.
    let req_amount = |rx: u16, ry: u16| {
        result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .iter()
            .find(|r| r.rx == rx && r.ry == ry)
            .map(|r| r.amount)
    };
    assert_eq!(
        req_amount(8, 5),
        Some(12),
        "target cell should get a 12-level reduction request"
    );
    assert_eq!(
        req_amount(9, 5),
        Some(12),
        "neighbor cell within CellSpread=2 should get a 12-level reduction request"
    );
}

#[test]
fn test_direct_hit_weapon_destroys_center_ore() {
    let rules = test_rules(); // AP warhead has CellSpread=0.
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    // Ore at adjacent cell (9,5) should NOT be affected (CellSpread=0 = center only).

    let ore_ini =
        IniFile::from_str("[OverlayTypes]\n0=ORE\n[ORE]\nTiberium=yes\nChainReaction=yes\n");
    let ore_registry = OverlayTypeRegistry::from_ini(&ore_ini, None);
    let mut overlays = OverlayGrid::new(16, 16);
    overlays.place_overlay(8, 5, 0, 5);
    overlays.place_overlay(9, 5, 0, 5);

    let mut main_rng = SimRng::new(1);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        Some(&mut overlays),
        Some(&ore_registry),
        None,
        0u64,
        100,
        0u32,
        &[],
        None,
        &mut main_rng,
    );

    // Combat emits a TiberiumReductionRequest (applied later by World). 105mm
    // damage=65 → ore_damage = 65/10 = 6 density levels. CellSpread=0 → only the
    // impact cell (8,5) gets a request; the adjacent cell (9,5) gets none.
    let center = result
        .consequences
        .effects()
        .tiberium_reduction_requests
        .iter()
        .find(|r| r.rx == 8 && r.ry == 5);
    assert_eq!(
        center.map(|r| r.amount),
        Some(6),
        "center cell should get a 6-level reduction request"
    );
    assert!(
        !result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .iter()
            .any(|r| r.rx == 9 && r.ry == 5),
        "adjacent cell should get no request with CellSpread=0"
    );
}

#[test]
fn test_weak_weapon_partial_ore_reduction() {
    let rules = test_rules(); // M60 damage=25.
    let mut store = EntityStore::new();
    // E1 attacks MTNK — E1's primary is M60 (damage=25, SA warhead, CellSpread=0).
    store.insert(make_entity(1, "E1", 5, 5, 125));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    // 10 density levels of ore: remaining = 10 * 120 = 1200.

    let ore_ini =
        IniFile::from_str("[OverlayTypes]\n0=ORE\n[ORE]\nTiberium=yes\nChainReaction=yes\n");
    let ore_registry = OverlayTypeRegistry::from_ini(&ore_ini, None);
    let mut overlays = OverlayGrid::new(16, 16);
    overlays.place_overlay(8, 5, 0, 9);

    let mut main_rng = SimRng::new(1);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        Some(&mut overlays),
        Some(&ore_registry),
        None,
        0u64,
        100,
        0u32,
        &[],
        None,
        &mut main_rng,
    );

    // Combat emits a TiberiumReductionRequest (applied later by World). M60
    // damage=25 → ore_damage = 25/10 = 2 density levels at the impact cell.
    let req = result
        .consequences
        .effects()
        .tiberium_reduction_requests
        .iter()
        .find(|r| r.rx == 8 && r.ry == 5);
    assert_eq!(
        req.map(|r| r.amount),
        Some(2),
        "should emit a 2-density-level reduction request (25/10=2)"
    );
}

// ---- Wall damage integration tests ----------------------------------------

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::sim::overlay_grid::{OverlayGrid, WallDamageEvent};
use crate::sim::world::Simulation;

/// INI containing GAWALL as both a [BuildingTypes] entry (so it has an
/// ObjectType with Wall=yes) and an [OverlayTypes] entry (so the overlay
/// registry knows it as a wall overlay). Strength=400, DamageLevels=4 are
/// representative of the real GAWALL.
fn wall_test_ini() -> &'static str {
    "[InfantryTypes]\n\
     [VehicleTypes]\n\
     [AircraftTypes]\n\
     [BuildingTypes]\n0=GAWALL\n\
     [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
     [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\n\
     [GASAND]\nWall=yes\nStrength=400\n\
     [CYCL]\nWall=yes\nStrength=400\n"
}

/// Build a Simulation with an ephemeral GAWALL overlay at `(rx, ry)`.
fn build_minimal_sim_with_gawall(rx: u16, ry: u16) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let ini = IniFile::from_str(wall_test_ini());
    let rules = RuleSet::from_ini(&ini).expect("wall rules parse");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    let mut sim = Simulation::new();
    let mut grid = OverlayGrid::new(10, 10);
    // Place GAWALL (overlay_id=2). Initial frame = 0 (isolated, stage 0).
    grid.place_overlay(rx, ry, 2, 0);
    sim.overlay_grid = Some(grid);

    (sim, rules, registry)
}

#[test]
fn wall_warhead_damages_and_destroys_wall_overlay() {
    let (mut sim, rules, registry) = build_minimal_sim_with_gawall(5, 5);

    let initial_wall_entities = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(_, e)| {
            rules
                .object(sim.interner.resolve(e.type_ref))
                .is_some_and(|o| o.wall)
        })
        .count();
    assert_eq!(initial_wall_entities, 0, "wall state is cell-owned");

    // Forced destruction (literal -1 bypasses the probabilistic gate).
    let events = [WallDamageEvent {
        rx: 5,
        ry: 5,
        damage: -1,
    }];
    sim.apply_wall_damage_events(&events, &registry);
    // Overlay cleared.
    let grid = sim
        .overlay_grid
        .as_ref()
        .expect("grid should still be present");
    assert!(
        grid.cell(5, 5).overlay_id.is_none(),
        "overlay should be cleared"
    );
    assert_eq!(
        sim.radar_terrain_dirty_cells,
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (4, 4),
            (5, 4),
            (4, 6),
            (3, 5),
            (4, 5),
            (6, 6),
            (5, 7),
            (5, 6),
            (7, 5),
            (6, 5),
        ],
        "direct wall damage uses the terminal DestroyOverlay visit stencil",
    );
    assert_eq!(sim.radar_terrain_dirty_generation, 13);
    assert_eq!(
        sim.tactical_dirty_cells,
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (5, 5),
            (4, 4),
            (5, 4),
            (4, 4),
            (5, 5),
            (4, 6),
            (3, 5),
            (4, 5),
            (5, 5),
            (6, 6),
            (5, 7),
            (4, 6),
            (5, 6),
            (6, 4),
            (7, 5),
            (6, 6),
            (5, 5),
            (6, 5),
        ]
    );

    // No persistent wall entity is created or removed.
    let remaining = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(_, e)| {
            rules
                .object(sim.interner.resolve(e.type_ref))
                .is_some_and(|o| o.wall)
        })
        .count();
    assert_eq!(remaining, 0);
}

#[test]
fn crusher_driveover_destroys_wall_but_noncrusher_does_not() {
    // A `Crusher=yes` drive vehicle standing on a wall cell after ground movement
    // flattens the wall (gamemd movement-side PerCellProcess crush), taking no
    // damage itself; a non-crusher on the same cell leaves the wall intact.
    // GAWALL is both a BuildingType (Wall=yes ObjectType) and an OverlayType;
    // BFRT is a Crusher drive vehicle, MTNK a plain drive vehicle.
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=BFRT\n1=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWALL\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
         [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\nDamageLevels=4\n\
         [BFRT]\nCrusher=yes\nMovementZone=CrusherAll\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [MTNK]\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules parse");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    // Build a sim with a GAWALL overlay at (5,5) and one
    // vehicle of `veh_type` placed on that same cell, its crusher flag +
    // locomotor derived from the real ObjectType.
    let build = |veh_type: &str| -> Simulation {
        let mut sim = Simulation::new();
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(5, 5, 2, 0); // GAWALL overlay_id=2
        sim.overlay_grid = Some(grid);

        let owner_id = sim.interner.intern("Test");
        let obj = rules.object(veh_type).expect("veh object");
        let veh_type_id = sim.interner.intern(veh_type);
        let mut veh = GameEntity::test_default(2, veh_type, "Test", 5, 5);
        veh.owner = owner_id;
        veh.type_ref = veh_type_id;
        veh.regular_crusher = obj.crusher;
        veh.omni_crusher = obj.omni_crusher;
        veh.locomotor =
            Some(crate::sim::movement::locomotor::LocomotorState::from_object_type(obj, 0));
        veh.health = Health { current: 300 };
        sim.substrate.entities.insert(veh);
        sim.substrate.entities.rebuild_owner_index();
        sim
    };

    let wall_present = |sim: &Simulation| -> bool {
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .overlay_id
            .is_some()
    };

    // Crusher (BFRT): wall destroyed and crusher unharmed.
    let mut sim = build("BFRT");
    sim.apply_wall_crush_on_driveover(Some(&rules), Some(&registry));
    sim.flush_pending_delete();
    assert!(
        !wall_present(&sim),
        "crusher drive-over must remove the wall overlay"
    );
    assert_eq!(
        sim.radar_terrain_dirty_cells.len(),
        13,
        "crusher uses the terminal DestroyOverlay radar-dirty stencil",
    );
    assert_eq!(sim.radar_terrain_dirty_cells[0], (5, 5));
    assert_eq!(sim.radar_terrain_dirty_generation, 13);
    assert_eq!(sim.tactical_dirty_cells.len(), 21);
    assert!(
        sim.substrate
            .entities
            .get(2)
            .is_some_and(|e| e.health.current == 300),
        "crusher takes no damage from crushing the wall"
    );
    let walls_left = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(_, e)| {
            rules
                .object(sim.interner.resolve(e.type_ref))
                .is_some_and(|o| o.wall)
        })
        .count();
    assert_eq!(walls_left, 0, "walls never create persistent entities");

    // Non-crusher (MTNK): wall stays intact.
    let mut sim = build("MTNK");
    sim.apply_wall_crush_on_driveover(Some(&rules), Some(&registry));
    sim.flush_pending_delete();
    assert!(
        wall_present(&sim),
        "a non-crusher drive vehicle must not remove the wall"
    );
}

/// `UnitClass::PerCellProcess @ 0x0073B013..B034`: a `Crushable=yes` overlay
/// (fences, sandbags) falls to any crusher whatever its locomotor, a plain
/// `Wall=yes` overlay only to a `MovementZone=CrusherAll` one; the overlay
/// `CrushSound=` is queued at the crusher (`0x0073B045..B04D`).
#[test]
fn crushable_fence_falls_to_any_crusher_and_plays_its_crush_sound() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=ROBO\n1=BFRT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWALL\n1=CAFNCB\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n3=CAFNCB\n\
         [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\nDamageLevels=4\n\
         [CAFNCB]\nStrength=100\nArmor=wood\nWall=yes\nCrushable=yes\nCrushSound=WallCrushBlack\n\
         [ROBO]\nCrusher=yes\nLocomotor={4A582742-9839-11D1-B709-00A024DDAFD1}\n\
         [BFRT]\nCrusher=yes\nMovementZone=CrusherAll\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules parse");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    assert_eq!(
        registry.flags(3).and_then(|f| f.crush_sound.as_deref()),
        Some("WallCrushBlack")
    );

    let build = |veh_type: &str, overlay_id: u8| -> Simulation {
        let mut sim = Simulation::new();
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(5, 5, overlay_id, 0);
        sim.overlay_grid = Some(grid);
        let owner_id = sim.interner.intern("Test");
        let obj = rules.object(veh_type).expect("veh object");
        let veh_type_id = sim.interner.intern(veh_type);
        let mut veh = GameEntity::test_default(2, veh_type, "Test", 5, 5);
        veh.owner = owner_id;
        veh.type_ref = veh_type_id;
        veh.regular_crusher = obj.crusher;
        veh.locomotor =
            Some(crate::sim::movement::locomotor::LocomotorState::from_object_type(obj, 0));
        sim.substrate.entities.insert(veh);
        sim.substrate.entities.rebuild_owner_index();
        sim
    };
    let wall_present = |sim: &Simulation| -> bool {
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .overlay_id
            .is_some()
    };
    let crush_sounds = |sim: &Simulation| -> Vec<String> {
        sim.sound_events
            .iter()
            .filter_map(|event| match event {
                crate::sim::world::SimSoundEvent::WallCrushed {
                    sound_id, rx, ry, ..
                } => {
                    assert_eq!((*rx, *ry), (5, 5));
                    Some(sound_id.clone())
                }
                _ => None,
            })
            .collect()
    };

    // Hover crusher over a crushable fence: crushed, cue queued.
    let mut sim = build("ROBO", 3);
    sim.apply_wall_crush_on_driveover(Some(&rules), Some(&registry));
    sim.flush_pending_delete();
    assert!(
        !wall_present(&sim),
        "a crushable fence falls to a hover crusher"
    );
    assert_eq!(crush_sounds(&sim), vec!["WallCrushBlack".to_string()]);

    // Crusher over a concrete wall WITHOUT MovementZone=CrusherAll: survives.
    let mut sim = build("ROBO", 2);
    sim.apply_wall_crush_on_driveover(Some(&rules), Some(&registry));
    sim.flush_pending_delete();
    assert!(
        wall_present(&sim),
        "a plain wall needs MovementZone=CrusherAll, not merely Crusher=yes"
    );
    assert!(crush_sounds(&sim).is_empty());

    // CrusherAll vehicle over a concrete wall without a CrushSound: crushed,
    // silent. Exactly one stock vehicle qualifies - BFRT, the Battle Fortress.
    let mut sim = build("BFRT", 2);
    sim.apply_wall_crush_on_driveover(Some(&rules), Some(&registry));
    sim.flush_pending_delete();
    assert!(!wall_present(&sim));
    assert!(
        crush_sounds(&sim).is_empty(),
        "no CrushSound= means silence"
    );
}

/// Build a Simulation with a row of GAWALL at `(rx_range, ry)`. Each cell gets
/// both an OverlayCell entry and a matching wall GameEntity.
fn build_minimal_sim_with_gawall_row(
    ry: u16,
    rx_range: std::ops::Range<u16>,
) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let ini = IniFile::from_str(wall_test_ini());
    let rules = RuleSet::from_ini(&ini).expect("wall rules parse");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    let mut sim = Simulation::new();
    let mut grid = OverlayGrid::new(10, 10);
    let owner_id = sim.interner.intern("Test");
    let type_id = sim.interner.intern("GAWALL");
    let mut next_id: u64 = 1;
    for rx in rx_range {
        grid.place_overlay(rx, ry, 2, 0);
        let mut entity = GameEntity::test_default(next_id, "GAWALL", "Test", rx, ry);
        entity.owner = owner_id;
        entity.type_ref = type_id;
        entity.health = Health { current: 400 };
        sim.substrate.entities.insert(entity);
        next_id += 1;
    }
    sim.overlay_grid = Some(grid);
    sim.substrate.entities.rebuild_owner_index();

    (sim, rules, registry)
}

#[test]
fn concrete_wall_chain_reaction_runs_without_panic() {
    // Row of 4 GAWALL at (4..8, 5).
    let (mut sim, _rules, registry) = build_minimal_sim_with_gawall_row(5, 4..8);
    // Pre-set (5,5) to stage 2 with E+W connectivity so a single damage event
    // pushes it through the penultimate-stage chain trigger (stage 3 of
    // DamageLevels=4). Connectivity nibble 0b1010 = E+W = 0xA, byte = 0x2A.
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .set_overlay_data(5, 5, 0x2A);

    // damage = Strength (400) — gate `damage < strength` is false, so the
    // probabilistic check is skipped and the damage applies. Stage advances
    // to 3 → chain triggers 200-damage events on pristine same-type cardinal
    // neighbors. Outcome of those events depends on RNG roll vs strength=400.
    let events = [WallDamageEvent {
        rx: 5,
        ry: 5,
        damage: 400,
    }];
    sim.apply_wall_damage_events(&events, &registry);

    // The chain code path ran (no panic). Assert (5,5) is at stage ≥ 3 or
    // gone — either outcome is consistent with the binary's behavior at the
    // penultimate damage level.
    let grid = sim.overlay_grid.as_ref().unwrap();
    let cell = grid.cell(5, 5);
    if let Some(id) = cell.overlay_id {
        assert_eq!(id, 2, "if not destroyed, must still be GAWALL");
        assert!(
            cell.overlay_data >> 4 >= 3,
            "stage should have advanced to ≥3 after applied damage"
        );
    }
    // No assertion about pristine neighbors — their fate depends on RNG.
}

/// Seeded variant of `build_minimal_sim_with_gawall` — used for determinism
/// replay tests where two sims must produce byte-identical state given the
/// same input event sequence.
fn build_minimal_sim_with_gawall_seeded(
    rx: u16,
    ry: u16,
    seed: u64,
) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let (mut sim, rules, registry) = build_minimal_sim_with_gawall(rx, ry);
    sim.reseed_scenario_and_main(seed);
    (sim, rules, registry)
}

#[test]
fn wall_damage_deterministic_across_replays() {
    let seed: u64 = 0x1234_5678;
    let events = [
        WallDamageEvent {
            rx: 5,
            ry: 5,
            damage: 100,
        },
        WallDamageEvent {
            rx: 5,
            ry: 5,
            damage: 100,
        },
        WallDamageEvent {
            rx: 5,
            ry: 5,
            damage: 100,
        },
        WallDamageEvent {
            rx: 5,
            ry: 5,
            damage: 100,
        },
        WallDamageEvent {
            rx: 5,
            ry: 5,
            damage: 100,
        },
    ];

    let snapshot_a: (Option<u8>, u8) = {
        let (mut sim, _rules, registry) = build_minimal_sim_with_gawall_seeded(5, 5, seed);
        sim.apply_wall_damage_events(&events, &registry);
        let cell = sim.overlay_grid.as_ref().unwrap().cell(5, 5);
        (cell.overlay_id, cell.overlay_data)
    };
    let snapshot_b: (Option<u8>, u8) = {
        let (mut sim, _rules, registry) = build_minimal_sim_with_gawall_seeded(5, 5, seed);
        sim.apply_wall_damage_events(&events, &registry);
        let cell = sim.overlay_grid.as_ref().unwrap().cell(5, 5);
        (cell.overlay_id, cell.overlay_data)
    };

    assert_eq!(
        snapshot_a, snapshot_b,
        "wall damage must be RNG-deterministic"
    );
}

#[test]
fn pursuit_weapon_range_for_entity_target() {
    use crate::sim::combat::{TargetKind, pursuit_selected_weapon};
    let rules = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 0, 0, 300));
    store.insert(make_entity(2, "MTNK", 5, 0, 300));
    let interner = test_interner();

    let attacker = store.get(1).unwrap();
    let range = pursuit_selected_weapon(
        attacker,
        &TargetKind::Entity(2),
        &store,
        &rules,
        &interner,
        None,
        None,
    )
    .map(|w| w.range);
    // 105mm Range=6.
    assert_eq!(range, Some(crate::util::fixed_math::SimFixed::from_num(6)));
}

#[test]
fn pursuit_weapon_range_for_cell_target() {
    use crate::sim::combat::{TargetKind, pursuit_selected_weapon};
    let rules = test_rules();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 0, 0, 300));
    let interner = test_interner();

    let attacker = store.get(1).unwrap();
    let range = pursuit_selected_weapon(
        attacker,
        &TargetKind::Cell(50, 50),
        &store,
        &rules,
        &interner,
        None,
        None,
    )
    .map(|w| w.range);
    // Cell target: the ladder returns slot 0 and the GetFireError cell subset
    // clears it — the 105mm Cannon projectile is AG, and MTNK is not
    // LandTargeting=1. Range = 6.
    assert_eq!(range, Some(crate::util::fixed_math::SimFixed::from_num(6)));
}

#[test]
fn pursuit_weapon_range_none_for_unarmed_attacker() {
    use crate::sim::combat::{TargetKind, pursuit_selected_weapon};
    let rules_str = "[InfantryTypes]\n0=ENGI\n\n\
                     [VehicleTypes]\n\n[BuildingTypes]\n\n[AircraftTypes]\n\n\
                     [ENGI]\nStrength=75\nArmor=none\nSpeed=4\n";
    let ini = IniFile::from_str(rules_str);
    let rules = RuleSet::from_ini(&ini).expect("parse");
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "ENGI", 0, 0, 75));
    let interner = test_interner();

    let attacker = store.get(1).unwrap();
    let range = pursuit_selected_weapon(
        attacker,
        &TargetKind::Cell(50, 50),
        &store,
        &rules,
        &interner,
        None,
        None,
    )
    .map(|w| w.range);
    assert_eq!(range, None);
}

#[test]
fn v3_non_killing_aoe_emits_one_detonation_anim() {
    // V3-style splash hits a heavy-armor target with HP > splash damage.
    // The target survives; the shot still starts one AnimList anim, whose
    // own Middle marks the ground.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=MTNK\n1=V3\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=V3W\n\n\
         [V3]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=V3W\n\n\
         [V3W]\nDamage=100\nROF=20\nRange=10\nWarhead=V3WH\n\n\
         [V3WH]\nCellSpread=1\nPercentAtMax=1\nAnimList=V3EXP\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("v3 test rules should parse");

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300)); // full HP — won't die
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    assert!(
        store.get(2).map(|e| e.health.current > 0).unwrap_or(false),
        "target must survive (test setup invariant)"
    );
    let v3exp = interner.intern("V3EXP");
    let anims: Vec<_> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| effect.shp_name)
        .collect();
    assert_eq!(
        anims,
        vec![v3exp],
        "one detonation starts one AnimList anim"
    );
}

#[test]
fn v3_killing_aoe_emits_exactly_one_detonation_anim() {
    // V3 splash kills a low-HP target with no Explosion= list. Only ONE
    // detonation occurred, so ONE AnimList anim starts; the kill adds none.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=MTNK\n1=WEAK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=V3W\n\n\
         [WEAK]\nStrength=10\nArmor=heavy\nSpeed=6\n\n\
         [V3W]\nDamage=200\nROF=20\nRange=10\nWarhead=V3WH\n\n\
         [V3WH]\nCellSpread=1\nPercentAtMax=1\nAnimList=V3EXP\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("v3 kill test rules should parse");

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "WEAK", 8, 5, 10)); // dies in one hit
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    assert_eq!(
        result.consequences.effects().despawned_ids.len(),
        1,
        "target must die (test setup invariant)"
    );
    assert_eq!(
        result.consequences.effects().explosion_effects.len(),
        1,
        "kill must start exactly one anim — no double from the kill handler"
    );
}

#[test]
fn gsi_04_11_death_weapon_anim_precedes_outer_detonation_anim() {
    // A Demo-Truck-style entity (Explodes=yes, primary warhead with its own
    // AnimList) is killed by a tank with a different warhead and AnimList.
    // ReceiveDamage synchronously completes the demo's UCEXPLOD death weapon;
    // only then does the outer Bullet detonation start TANKEXP.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=TNK\n1=DEMO\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [TNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=TANKW\n\n\
         [DEMO]\nStrength=100\nArmor=light\nSpeed=6\nPrimary=DEMOW\nExplodes=yes\n\n\
         [TANKW]\nDamage=100\nROF=20\nRange=10\nWarhead=TANKHIT\n\n\
         [DEMOW]\nDamage=200\nROF=50\nRange=4\nWarhead=DEMOWH\n\n\
         [TANKHIT]\nCellSpread=0\nPercentAtMax=1\nAnimList=TANKEXP\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
         [DEMOWH]\nCellSpread=2\nPercentAtMax=0.5\nAnimList=UCEXPLOD\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("demo-truck test rules should parse");

    let mut store = EntityStore::new();
    store.insert(make_entity(1, "TNK", 5, 5, 300));
    store.insert(make_entity(2, "DEMO", 8, 5, 100));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let mut main_rng = SimRng::new(1);

    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0u64,
        100,
        0u32,
        &mut main_rng,
    );

    let tankexp = interner.intern("TANKEXP");
    let ucexplod = interner.intern("UCEXPLOD");
    assert_eq!(
        result
            .consequences
            .effects()
            .explosion_effects
            .iter()
            .map(|effect| effect.shp_name)
            .collect::<Vec<_>>(),
        vec![ucexplod, tankexp]
    );
}

fn inviso_weapon_rules(inviso: bool, with_anim: bool) -> RuleSet {
    let anim_list = if with_anim { "AnimList=PIFF\n" } else { "" };
    let ini = IniFile::from_str(&format!(
        "\
[InfantryTypes]\n\n\
[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n\
[TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\n\
[GUN]\nDamage=10\nROF=20\nRange=10\nProjectile=TESTPROJ\nWarhead=TESTWH\n\n\
[TESTPROJ]\nInviso={}\n\n\
[TESTWH]\n{}\
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        if inviso { "yes" } else { "no" },
        anim_list,
    ));
    RuleSet::from_ini(&ini).expect("Inviso test rules should parse")
}

fn persistent_projectile_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\n[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n[SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n[TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\n[GUN]\nDamage=10\nROF=20\nRange=10\nSpeed=128\nProjectile=TESTPROJ\nWarhead=TESTWH\n\n[TESTPROJ]\nInviso=no\nImage=TESTBULLET\n\n[TESTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("persistent projectile rules should parse")
}

#[test]
fn fire_admission_preserves_flat_projectile_layer_through_save_and_retirement() {
    use crate::sim::world::display_layers::DisplayLayer;
    for flat in [false, true] {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\
                [SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\
                [TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\
                [GUN]\nDamage=10\nROF=20\nRange=10\nSpeed=128\nProjectile=TESTPROJ\nWarhead=TESTWH\n\
                [TESTPROJ]\nImage=SHOT\nROT=0\nArcing=yes\n\
                [TESTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"),
            &IniFile::from_str(&format!("[SHOT]\nFlat={}\n", if flat { "yes" } else { "no" })),
        ).unwrap();
        assert_eq!(rules.projectile("TESTPROJ").unwrap().flat, flat);
        let mut entities = EntityStore::new();
        entities.insert(make_entity(1, "SHOOTER", 5, 5, 300));
        entities.insert(make_entity(2, "TARGET", 8, 5, 500));
        let mut interner = test_interner();
        issue_attack_command(&mut entities, 1, 2, None, &interner);
        align_attackers_to_targets(&mut entities, &rules, &interner);
        let fire = tick_combat(
            &mut entities,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut SimRng::new(1),
        );
        assert_eq!(fire.projectile_spawns.len(), 1);
        assert_eq!(fire.projectile_spawns[0].flat, flat);
        let mut sim = crate::sim::world::Simulation::new();
        sim.interner = interner;
        // Preserve the real firer and target identities across the save.
        assert_eq!(sim.allocate_stable_id(), 1);
        assert_eq!(sim.allocate_stable_id(), 2);
        sim.substrate.entities = entities;
        let id = sim.allocate_stable_id();
        sim.admit_projectile(id, fire.projectile_spawns[0]);
        let layer = if flat {
            DisplayLayer::SURFACE
        } else {
            DisplayLayer::AIR
        };
        assert_eq!(sim.substrate.display.members(layer), [id]);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "flat-shot", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.substrate.display.members(layer), [id]);
        assert!(restored.retire_non_entity_object(id));
        assert_eq!(restored.substrate.display.layer_of(id), None);
        assert!(
            restored.projectiles.get(id).is_some(),
            "display removal precedes deferred free"
        );
        restored.process_pending_delete();
        assert!(restored.projectiles.get(id).is_none());
    }
}

#[test]
fn gsi_04_11_persistent_projectile_keeps_exact_lepton_z() {
    let rules = persistent_projectile_rules();
    let mut entities = EntityStore::new();
    let mut shooter = make_entity(1, "SHOOTER", 5, 5, 300);
    shooter.position.z = 7;
    shooter.position.exact_z_leptons = Some(733);
    entities.insert(shooter);
    let mut target = make_entity(2, "TARGET", 8, 5, 500);
    target.position.z = 11;
    target.position.exact_z_leptons = Some(1_177);
    entities.insert(target);
    let mut interner = test_interner();
    issue_attack_command(&mut entities, 1, 2, None, &interner);

    align_attackers_to_targets(&mut entities, &rules, &interner);
    let result = tick_combat(
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(1),
    );

    assert_eq!(result.projectile_spawns.len(), 1);
    assert_eq!(result.projectile_spawns[0].origin.z, 733);
    assert_eq!(result.projectile_spawns[0].initial_target_position.z, 1_177);
}

#[test]
fn persistent_projectile_delays_damage_across_save_load_continuation() {
    let rules = persistent_projectile_rules();
    assert!(matches!(
        classify_projectile_delivery(rules.weapon("GUN").unwrap(), &rules),
        ProjectileDelivery { .. }
    ));
    let mut entities = EntityStore::new();
    entities.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    entities.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    issue_attack_command(&mut entities, 1, 2, None, &interner);

    let mut scenario_rng = SimRng::new(1);
    align_attackers_to_targets(&mut entities, &rules, &interner);
    let fire = tick_combat(
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut scenario_rng,
    );
    assert_eq!(entities.get(2).unwrap().health.current, 500);
    assert_eq!(fire.projectile_spawns.len(), 1);

    let mut sim = crate::sim::world::Simulation::new();
    let projectile_id = sim.allocate_stable_id();
    sim.admit_projectile(projectile_id, fire.projectile_spawns[0]);
    let target_positions =
        BTreeMap::from([(2, ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0))]);
    let shared_cell_dummy = sim.effective_shared_cell_dummy();
    assert!(
        sim.projectiles
            .advance(0, &target_positions, None, &shared_cell_dummy, |_, _| None,)
            .detonations
            .is_empty()
    );

    let snapshot = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "projectile-flight", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&snapshot)
        .expect("pending projectile snapshot should load")
        .sim;
    let restored_shared_cell_dummy = restored.effective_shared_cell_dummy();
    let mut detonations = Vec::new();
    for _ in 0..8 {
        detonations = restored
            .projectiles
            .advance(
                0,
                &target_positions,
                None,
                &restored_shared_cell_dummy,
                |_, _| {
                    Some(
                        crate::sim::projectile::ProjectileCollisionResponse::TargetZClamp(
                            target_positions[&2],
                        ),
                    )
                },
            )
            .detonations;
        if !detonations.is_empty() {
            break;
        }
    }
    assert_eq!(
        detonations.len(),
        1,
        "resumed projectile must deliver the supplied world admission"
    );

    entities.remove(1);
    let mut main_rng = SimRng::new(1);
    let mut houses = BTreeMap::new();
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    align_attackers_to_targets(&mut entities, &rules, &interner);
    tick_combat_with_fog_and_main_rng(
        &mut entities,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        &mut houses,
        &[],
        &HouseAllianceMap::new(),
        None,
        None,
        None,
        None,
        1,
        100,
        1,
        &[2],
        &detonations,
        &[],
        None,
        &[],
        &mut scenario_rng,
        &mut main_rng,
        None,
    );
    assert_eq!(entities.get(2).unwrap().health.current, 490);
}

/// One Inviso bullet's detonation draws in the frame's tail: the anim
/// scatter (one raw draw), then the cluster successor
/// (`RandomRanged(0x100, 0x200)` and one raw draw, `0x00469057`).
fn inviso_detonation_draws(
    rng: &mut SimRng,
    coord: (u16, u16, SimFixed, SimFixed),
) -> (u16, u16, SimFixed, SimFixed) {
    let effect =
        inviso_scatter::scatter_inviso_effect_coord(rng, coord.0, coord.1, coord.2, coord.3);
    let _ =
        crate::sim::projectile::projectile_next_cluster_coord(ProjectileCoord::new(0, 0, 0), rng);
    effect
}

fn explosion_coord(effect: &ExplosionEffect) -> (u16, u16, SimFixed, SimFixed) {
    (effect.rx, effect.ry, effect.sub_x, effect.sub_y)
}

#[test]
fn inviso_scatter_uses_scenario_rng_only_for_effect_and_paired_smudge() {
    let rules = inviso_weapon_rules(true, true);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let target_coord = (
        store.get(2).unwrap().position.rx,
        store.get(2).unwrap().position.ry,
        store.get(2).unwrap().position.sub_x,
        store.get(2).unwrap().position.sub_y,
    );

    let mut scenario_rng = SimRng::new(1);
    let mut expected_rng = scenario_rng.clone();
    // FireAt's `GetROF @ 0x006FCFA0` jitter, then the bullet's detonation in
    // the same frame's tail.
    expected_rng.next_range_u32_inclusive(0, 2);
    let expected_effect = inviso_detonation_draws(&mut expected_rng, target_coord);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut scenario_rng,
    );

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert_eq!(store.get(2).unwrap().health.current, 490);
    assert_eq!(result.consequences.effects().explosion_effects.len(), 1);
    assert_eq!(
        explosion_coord(&result.consequences.effects().explosion_effects[0]),
        expected_effect
    );
    assert_ne!(expected_effect, target_coord);
    assert!(
        result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .is_empty(),
        "a non-Tiberium warhead without authoritative overlay context must not reduce ore"
    );
}

#[test]
fn inviso_empty_animlist_still_consumes_one_draw() {
    let rules = inviso_weapon_rules(true, false);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    let mut scenario_rng = SimRng::new(1);
    let mut expected_rng = scenario_rng.clone();
    // GetROF in FireAt, then the tail's scatter and cluster draws.
    expected_rng.next_range_u32_inclusive(0, 2);
    let _ = inviso_detonation_draws(
        &mut expected_rng,
        (8, 5, SimFixed::from_num(128), SimFixed::from_num(128)),
    );
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut scenario_rng,
    );

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert!(result.consequences.effects().explosion_effects.is_empty());
    assert!(
        result
            .consequences
            .effects()
            .smudge_spawn_requests
            .is_empty()
    );
}

#[test]
fn gsi_08_05_non_inviso_projectile_advances_scenario_rng_by_the_reload_jitter() {
    let rules = inviso_weapon_rules(false, true);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    let target_coord = (
        store.get(2).unwrap().position.rx,
        store.get(2).unwrap().position.ry,
        store.get(2).unwrap().position.sub_x,
        store.get(2).unwrap().position.sub_y,
    );

    let mut scenario_rng = SimRng::new(1);
    // A non-inviso shot takes no scatter draw, but it still reloads, and
    // `TechnoClass::GetROF @ 0x006FCFA0` draws `RandomRanged(0, 2)` for the
    // reload unconditionally. Exactly one draw, on the scenario instance.
    let mut expected_rng = scenario_rng.clone();
    expected_rng.next_range_u32_inclusive(0, 2);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut scenario_rng,
    );

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert!(
        result.consequences.effects().explosion_effects.is_empty(),
        "non-Inviso Speed=0 still creates a persistent native shot"
    );
    assert_eq!(result.projectile_spawns.len(), 1);
    let target = result.projectile_spawns[0].initial_target_position;
    assert_eq!(
        (target.x, target.y),
        (
            i32::from(target_coord.0) * 256 + target_coord.2.to_num::<i32>(),
            i32::from(target_coord.1) * 256 + target_coord.3.to_num::<i32>()
        )
    );
}

/// Both shots' FireAt draws come first, in live order; then the tail visits
/// the bullets in the order FireAt appended them. The first bullet's
/// detonation removes it from the Logic vector, which shifts the second into
/// its slot, and the cursor moves past it (`0x0055B613`): the second bullet
/// detonates next frame.
#[test]
fn two_inviso_attackers_fire_in_live_order_and_the_second_bullet_waits_a_frame() {
    let rules = inviso_weapon_rules(true, true);
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "SHOOTER", 6, 5, 300));
    store.insert(make_entity(3, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 3, None, &interner);
    issue_attack_command(&mut store, 2, 3, None, &interner);
    let target = store.get(3).unwrap();
    let target_coord = (
        target.position.rx,
        target.position.ry,
        target.position.sub_x,
        target.position.sub_y,
    );

    let mut scenario_rng = SimRng::new(1);
    let mut expected_rng = scenario_rng.clone();
    expected_rng.next_range_u32_inclusive(0, 2);
    expected_rng.next_range_u32_inclusive(0, 2);
    let expected = inviso_detonation_draws(&mut expected_rng, target_coord);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat_with_fog(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        None,
        &BTreeMap::<InternedId, PowerState>::new(),
        None,
        None,
        None,
        None,
        0,
        100,
        0,
        &[2, 1],
        None,
        &mut scenario_rng,
    );

    assert_eq!(
        result
            .consequences
            .fire_events()
            .iter()
            .map(|event| event.attacker_id)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert_eq!(result.consequences.effects().explosion_effects.len(), 1);
    assert_eq!(
        explosion_coord(&result.consequences.effects().explosion_effects[0]),
        expected
    );
    assert_eq!(
        result.projectile_spawns.len(),
        1,
        "attacker 1's bullet was skipped"
    );
    assert_eq!(result.projectile_spawns[0].source_id, 1);
}

/// `inviso_weapon_rules(true, true)` with one special warhead key added.
fn inviso_special_rules(special_key: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "\
[InfantryTypes]\n\n\
[VehicleTypes]\n0=SHOOTER\n1=TARGET\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[SHOOTER]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=GUN\n\n\
[TARGET]\nStrength=500\nArmor=heavy\nSpeed=6\n\n\
[GUN]\nDamage=10\nROF=20\nRange=10\nProjectile=TESTPROJ\nWarhead=TESTWH\n\n\
[TESTPROJ]\nInviso=yes\n\n\
[TESTWH]\nAnimList=PIFF\n{special_key}\n\
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("special Inviso test rules should parse")
}

/// `BulletClass::DetonateAtCoord @ 0x004690B0`: every special arm, its own
/// refusals included, leaves by `JMP 0x00469AA4`, and `Apply_area_damage`
/// (`0x00469A83`) is reachable only from the final else at `0x00469A3F`. An
/// Inviso shot is that same detonation, so a warhead selecting an arm whose
/// body VERA has not ported deals no damage and still runs the shared tail:
/// the Inviso re-scatter draw (`0x00469AD7`) and the `AnimList=` anim. Before,
/// the immediate path took ordinary area damage for these arms: the
/// Magnetron's `[MagneticBeam]` dealt 5000 to the vehicle it should lift.
#[test]
fn inviso_special_arms_claim_the_impact_and_keep_the_shared_tail() {
    // DirectRocker is the chain's one conditional arm; TARGET is a UnitClass.
    for special_key in [
        "ElectricAssault=yes",
        "IsLocomotor=yes",
        "Airstrike=yes",
        "DirectRocker=yes",
        "MakesDisguise=yes",
        "NukeMaker=yes",
    ] {
        let rules = inviso_special_rules(special_key);
        let mut store = EntityStore::new();
        store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
        store.insert(make_entity(2, "TARGET", 8, 5, 500));
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        let target = store.get(2).unwrap();
        let target_coord = (
            target.position.rx,
            target.position.ry,
            target.position.sub_x,
            target.position.sub_y,
        );
        let mut scenario_rng = SimRng::new(1);
        let mut expected_rng = scenario_rng.clone();
        expected_rng.next_range_u32_inclusive(0, 2);
        let expected_effect = inviso_detonation_draws(&mut expected_rng, target_coord);
        align_attackers_to_targets(&mut store, &rules, &interner);
        let result = tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut scenario_rng,
        );

        assert_eq!(
            store.get(2).unwrap().health.current,
            500,
            "{special_key}: the arm claims the impact, so no area damage"
        );
        assert_eq!(
            result.consequences.fire_events().len(),
            1,
            "{special_key}: the shot itself is fired"
        );
        assert_eq!(
            scenario_rng.logical_state(),
            expected_rng.logical_state(),
            "{special_key}: the reload jitter, then the tail's scatter and cluster draws"
        );
        let effects = result.consequences.effects();
        assert_eq!(effects.explosion_effects.len(), 1, "{special_key}");
        assert_eq!(
            explosion_coord(&effects.explosion_effects[0]),
            expected_effect,
            "{special_key}: the shared tail places the AnimList anim"
        );
    }
}

/// The Giant Squid's `[SquidGrab]` is an Inviso Parasite shot. Its arm
/// (`0x004693D3`) claims the impact; the grapple itself is `parasite`'s
/// recorded residual, so the attach is refused and the ship takes nothing.
#[test]
fn inviso_parasite_grapple_claims_the_impact() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=SQUID\n1=SHIP\n\
         [SQUID]\nStrength=300\nArmor=heavy\nSpeed=6\nNaval=yes\nOrganic=yes\nPrimary=GRAB\n\
         [SHIP]\nStrength=500\nArmor=heavy\nSpeed=6\nNaval=yes\n\
         [GRAB]\nDamage=40\nROF=99\nRange=2\nProjectile=TESTPROJ\nWarhead=TESTWH\n\
         [TESTPROJ]\nInviso=yes\n\
         [TESTWH]\nParasite=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SQUID", 5, 5, 300));
    store.insert(make_entity(2, "SHIP", 6, 5, 500));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(1),
    );

    assert_eq!(result.consequences.fire_events().len(), 1);
    assert_eq!(store.get(2).unwrap().health.current, 500);
}

/// The conditional DirectRocker arm (`0x00469796..0x004697B2`) claims the
/// impact only for a UnitClass target; at a cell it falls through to the
/// ordinary arm, so the ground shot still damages what stands there.
#[test]
fn inviso_direct_rocker_at_a_cell_keeps_ordinary_damage() {
    let rules = inviso_special_rules("DirectRocker=yes\nCellSpread=1");
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "SHOOTER", 5, 5, 300));
    store.insert(make_entity(2, "TARGET", 8, 5, 500));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    store.get_mut(1).unwrap().attack_target = Some(AttackTarget::for_cell(8, 5));
    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(1),
    );

    assert!(
        store.get(2).unwrap().health.current < 500,
        "a cell target is never a UnitClass, so the ordinary arm runs"
    );
}

/// Retail data: every stock weapon whose warhead selects an unported special
/// arm is an Inviso shot, so all of them reach this path.
#[test]
fn retail_special_inviso_weapons_claim_their_impact() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    for (weapon_id, expected) in [
        ("AssaultBolt", SpecialDetonationAction::ElectricAssault),
        ("MagneticBeam", SpecialDetonationAction::Locomotor),
        ("MagneticBeamE", SpecialDetonationAction::Locomotor),
        ("Flare", SpecialDetonationAction::Airstrike),
        ("MakeupKit", SpecialDetonationAction::MakesDisguise),
    ] {
        let weapon = rules.weapon(weapon_id).unwrap();
        let warhead = rules.warhead(weapon.warhead.as_deref().unwrap()).unwrap();
        let action = projectile_special_detonation_action(
            SpecialDetonationFlags::of(warhead),
            SpecialDetonationTarget { is_unit: true },
        );
        assert_eq!(action, expected, "{weapon_id}");
        assert!(action.suppresses_ordinary_damage(), "{weapon_id}");
        assert!(
            classify_projectile_delivery(weapon, &rules).inviso,
            "{weapon_id} is an Inviso shot"
        );
    }
}

// --- emit_warhead_detonation_effects helper tests ---------------------------

fn emit_helper_test_warhead(animlist: &[&str]) -> crate::rules::warhead_type::WarheadType {
    let animlist_csv = animlist.join(",");
    let ini_text = format!("[WH]\nFixtureOnly=1\nAnimList={}\n", animlist_csv);
    let ini = IniFile::from_str(&ini_text);
    let section = ini.section("WH").expect("section parses");
    crate::rules::warhead_type::WarheadType::from_ini_section("WH", section)
}

#[test]
fn emit_warhead_detonation_effects_empty_animlist_emits_nothing() {
    let mut interner = crate::sim::intern::StringInterner::new();
    let wh = emit_helper_test_warhead(&[]);
    let mut explosions: Vec<ExplosionEffect> = Vec::new();
    emit_warhead_detonation_effects(
        &wh,
        100,
        5,
        5,
        crate::util::lepton::CELL_CENTER_LEPTON,
        crate::util::lepton::CELL_CENTER_LEPTON,
        0,
        0,
        &mut interner,
        &mut explosions,
    );
    assert!(explosions.is_empty());
}

#[test]
fn emit_warhead_detonation_effects_single_animlist_entry_emits_one_anim() {
    let mut interner = crate::sim::intern::StringInterner::new();
    let wh = emit_helper_test_warhead(&["EXPLOSION1"]);
    let mut explosions: Vec<ExplosionEffect> = Vec::new();
    emit_warhead_detonation_effects(
        &wh,
        100,
        5,
        5,
        SimFixed::from_num(160),
        SimFixed::from_num(96),
        0,
        731,
        &mut interner,
        &mut explosions,
    );
    assert_eq!(explosions.len(), 1);
    let expected_id = interner.intern("EXPLOSION1");
    assert_eq!(explosions[0].shp_name, expected_id);
    assert_eq!(explosions[0].rx, 5);
    assert_eq!(explosions[0].ry, 5);
    assert_eq!(explosions[0].sub_x.to_num::<i32>(), 160);
    assert_eq!(explosions[0].sub_y.to_num::<i32>(), 96);
    assert_eq!(explosions[0].z, 0);
    assert_eq!(explosions[0].world_z, 731);
}

#[test]
fn emit_warhead_detonation_effects_animlist_index_is_damage_div_25_clamped() {
    let mut interner = crate::sim::intern::StringInterner::new();
    let wh = emit_helper_test_warhead(&["EXP1", "EXP2", "EXP3"]);

    // damage=0 → idx=0 → EXP1.
    let mut explosions: Vec<ExplosionEffect> = Vec::new();
    emit_warhead_detonation_effects(
        &wh,
        0,
        0,
        0,
        crate::util::lepton::CELL_CENTER_LEPTON,
        crate::util::lepton::CELL_CENTER_LEPTON,
        0,
        0,
        &mut interner,
        &mut explosions,
    );
    assert_eq!(explosions[0].shp_name, interner.intern("EXP1"));

    // damage=50 → idx=2 (50/25) → EXP3.
    let mut explosions: Vec<ExplosionEffect> = Vec::new();
    emit_warhead_detonation_effects(
        &wh,
        50,
        0,
        0,
        crate::util::lepton::CELL_CENTER_LEPTON,
        crate::util::lepton::CELL_CENTER_LEPTON,
        0,
        0,
        &mut interner,
        &mut explosions,
    );
    assert_eq!(explosions[0].shp_name, interner.intern("EXP3"));

    // damage=10000 → idx clamped to len-1 (2) → EXP3.
    let mut explosions: Vec<ExplosionEffect> = Vec::new();
    emit_warhead_detonation_effects(
        &wh,
        10000,
        0,
        0,
        crate::util::lepton::CELL_CENTER_LEPTON,
        crate::util::lepton::CELL_CENTER_LEPTON,
        0,
        0,
        &mut interner,
        &mut explosions,
    );
    assert_eq!(explosions[0].shp_name, interner.intern("EXP3"));
}

#[test]
fn combat_resolves_in_live_object_order_not_stable_id() {
    // Two attackers A (stable_id 1) and B (stable_id 2), same owner, both fire
    // at a shared enemy target T (stable_id 3) this tick. Each 105mm shot deals
    // 48 (65 * 75% AP-vs-heavy) to T's 50 HP. The first-resolved attacker's
    // bullet leads the Logic tail and lands this frame; the second is skipped
    // past when the first leaves the vector, so T survives on 2.
    //
    // Phase 4 of tick_combat_with_fog applies damage_events in resolution order;
    // damage_events is built in Phase 2 by walking the snapshots in their sorted
    // order. fire_events is pushed in that same order, so fire_events[0].attacker_id
    // is exactly the first-resolved attacker. The new sort keys on live_order
    // position (stable_id tiebreak), so passing live_order = [2, 1] must make B
    // resolve first, and live_order = &[] must fall back to stable-id order (A first).
    fn build() -> (EntityStore, RuleSet, StringInterner) {
        let rules = test_rules();
        let mut store = EntityStore::new();
        // A and B are co-located is irrelevant; both are in range (<= 6) of T.
        store.insert(make_entity(1, "MTNK", 5, 5, 300)); // attacker A
        store.insert(make_entity(2, "MTNK", 6, 5, 300)); // attacker B
        store.insert(make_entity(3, "MTNK", 5, 6, 50)); // shared target T
        let interner = test_interner();
        issue_attack_command(&mut store, 1, 3, None, &interner);
        issue_attack_command(&mut store, 2, 3, None, &interner);
        (store, rules, interner)
    }
    let mut main_rng = SimRng::new(1);

    // Run 1: live order [B(2), A(1)] (reversed vs stable-id). B resolves first:
    // it fires first and lands the lethal shot.
    {
        let (mut store, rules, mut interner) = build();
        align_attackers_to_targets(&mut store, &rules, &interner);
        let result = tick_combat_with_fog(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            None,
            &BTreeMap::<InternedId, PowerState>::new(),
            None,
            None,
            None,
            None,
            0u64,
            100,
            0u32,
            &[2, 1],
            None,
            &mut main_rng,
        );
        assert_eq!(
            result.consequences.fire_events()[0].attacker_id,
            2,
            "live order [2,1]: B (live-order-first) must fire first"
        );
        // B's bullet leads the Logic tail and lands; A's is skipped this frame.
        assert_eq!(store.get(3).unwrap().health.current, 2);
        assert_eq!(result.projectile_spawns.len(), 1);
        assert_eq!(result.projectile_spawns[0].source_id, 1);
    }

    // Run 2: empty live order falls back to stable-id order [A(1), B(2)]. A now
    // resolves first and fires first - the OPPOSITE of run 1 - proving live_order
    // controls the resolution sequence and that &[] reproduces the prior order.
    {
        let (mut store, rules, mut interner) = build();
        align_attackers_to_targets(&mut store, &rules, &interner);
        let result = tick_combat_with_fog(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            None,
            &BTreeMap::<InternedId, PowerState>::new(),
            None,
            None,
            None,
            None,
            0u64,
            100,
            0u32,
            &[],
            None,
            &mut main_rng,
        );
        assert_eq!(
            result.consequences.fire_events()[0].attacker_id,
            1,
            "empty live order: stable-id fallback fires A first"
        );
        assert_eq!(store.get(3).unwrap().health.current, 2);
        assert_eq!(result.projectile_spawns.len(), 1);
        assert_eq!(result.projectile_spawns[0].source_id, 2);
    }
}

// ---------------------------------------------------------------------------
// Radiation field (substrate Slice 7): periodic foot-unit damage through the
// [Radiation] RadSiteWarhead, building exemption, and the deployed
// self-irradiator (Desolator) re-fire loop.
// ---------------------------------------------------------------------------

/// Desolator-shaped rules: a deployable radiation infantry, a soft infantry
/// victim, a heavy-armor vehicle victim, and a building.
fn radiation_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "\
[General]\nVeteranArmor=1.5\n\n\
[InfantryTypes]\n0=DESO\n1=E2\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[DESO]\nStrength=200\nArmor=plate\nSpeed=4\nPrimary=RadBeamWeapon\nSecondary=RadEruptionWeapon\nDeployer=yes\nDeployFire=yes\nImmuneToRadiation=yes\n\n\
[E2]\nStrength=300\nArmor=none\nSpeed=4\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nVeteranAbilities=STRONGER\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\n\n\
[RadBeamWeapon]\nDamage=25\nROF=70\nRange=6\nWarhead=RadSite\n\n\
[RadEruptionWeapon]\nDamage=1\nROF=60\nRange=4\nAreaFire=yes\nWarhead=RadEruptionWarhead\nRadLevel=500\nReport=DesolatorDeploy\n\n\
[RadEruptionWarhead]\nVerses=100%,100%,100%,20%,10%,10%,0%,0%,0%,100%,100%\nInfDeath=7\nRadiation=yes\nCellSpread=10\n\n\
[RadSite]\nVerses=100%,100%,100%,50%,10%,10%,0%,0%,0%,100%,100%\nInfDeath=7\nRadiation=yes\n\n\
[Radiation]\nRadDurationMultiple=1\nRadApplicationDelay=16\nRadLevelMax=500\nRadLevelDelay=90\nRadLevelFactor=.2\nRadSiteWarhead=RadSite\n",
    );
    RuleSet::from_ini(&ini).expect("radiation rules should parse")
}

/// One combat tick with the radiation field threaded through.
fn rad_combat_tick(
    sim: &mut crate::sim::world::Simulation,
    rules: &RuleSet,
    binary_frame: u32,
) -> CombatTickResult {
    let mut radiation = std::mem::take(&mut sim.radiation);
    let result = tick_combat_with_fog(
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        rules,
        &mut sim.interner,
        None,
        &BTreeMap::new(),
        None,
        None,
        None,
        None,
        0,
        100,
        binary_frame,
        &[],
        Some(&mut radiation),
        &mut sim.scenario_rng,
    );
    sim.radiation = radiation;
    result
}

/// Radiation damage applies only on `frame % RadApplicationDelay == 0`
/// boundaries, scaled by the RadSiteWarhead Verses per armor class:
/// trunc(min(500, 500) × 0.2) = 100 base → 100 vs none, 10 vs heavy.
#[test]
fn rad_damage_fires_on_application_delay_boundary_only() {
    let rules = radiation_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let inf = sim
        .spawn_object("E2", "Americans", 5, 5, 0, &rules, &heights)
        .expect("infantry spawns");
    let tank = sim
        .spawn_object("MTNK", "Americans", 6, 5, 0, &rules, &heights)
        .expect("tank spawns");
    sim.radiation.apply_detonation(
        crate::sim::radiation::RadDetonation {
            rx: 5,
            ry: 5,
            rad_level: 500,
            spread: 2,
        },
        0,
        &rules.radiation,
        None,
    );

    // Frame 15: not an application boundary — nobody takes damage.
    rad_combat_tick(&mut sim, &rules, 15);
    assert_eq!(sim.substrate.entities.get(inf).unwrap().health.current, 300);
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().health.current,
        300
    );

    // Frame 16: boundary. E2 stands on the center cell (level 500, clamped
    // 500): trunc(500 × 0.2) = 100, Verses none = 100% → 100 damage. The
    // tank is one cell out (falloff (640−256)/640 × 500 = 300): trunc(300 ×
    // 0.2) = 60, Verses heavy = 10% → 6 damage.
    rad_combat_tick(&mut sim, &rules, 16);
    let inf_hp = sim.substrate.entities.get(inf).unwrap().health.current;
    let tank_hp = sim.substrate.entities.get(tank).unwrap().health.current;
    assert_eq!(inf_hp, 200, "100 rad damage vs armor none");
    assert_eq!(tank_hp, 294, "6 rad damage vs heavy armor (10% Verses)");
    // Sourceless damage must not arm retaliation.
    assert!(
        sim.substrate
            .entities
            .get(inf)
            .unwrap()
            .attack_target
            .is_none()
    );

    // Frame 17: off-boundary again.
    rad_combat_tick(&mut sim, &rules, 17);
    assert_eq!(sim.substrate.entities.get(inf).unwrap().health.current, 200);
}

#[test]
fn gsi_04_07_damage_periodic_radiation_enters_direct_receiver_once() {
    let rules = radiation_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let tank = sim
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules, &heights)
        .expect("veteran heavy target spawns");
    sim.substrate.entities.get_mut(tank).unwrap().veterancy = 100;
    sim.radiation.apply_detonation(
        crate::sim::radiation::RadDetonation {
            rx: 5,
            ry: 5,
            rad_level: 500,
            spread: 2,
        },
        0,
        &rules.radiation,
        None,
    );

    let result = rad_combat_tick(&mut sim, &rules, 16);
    let target = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(
        target.health.current, 294,
        "raw 100 / VeteranArmor 1.5 = 66; ftol(66 x heavy 10%) = 6 once"
    );
    assert!(
        target.attack_target.is_none(),
        "periodic radiation cannot arm retaliation"
    );
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "null source house cannot emit an enemy under-attack event"
    );
}

#[test]
fn gsi_04_07_damage_hostile_building_hit_latches_was_attacked_for_ai_repair() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\n\
         FixtureOnly=1\n\
         [AI]\nCreditReserve=100\n\
         [IQ]\nMaxIQLevels=5\nRepairSell=2\nSellBack=2\n\
         [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n\
         [Warheads]\n0=HITWH\n\
         [MTNK]\nStrength=300\nArmor=heavy\n\
         [GAPOWR]\nStrength=1000\nArmor=wood\nCost=800\nCrewed=no\n\
         [HITWH]\nCellSpread=0\nPercentAtMax=1\nAffectsAllies=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("hostile-hit rules");
    let mut sim = crate::sim::world::Simulation::new();
    let ai_owner = sim.interner.intern("AI");
    let enemy_owner = sim.interner.intern("ENEMY");
    let ally_owner = sim.interner.intern("ALLY");
    let scenario_ini = IniFile::from_str("[Houses]\n0=AI\n[AI]\nIQ=1\n");
    let scenario_houses =
        crate::map::houses::parse_house_roster(&scenario_ini, &rules.color_schemes, Some(&rules));
    let mut ai_house = HouseState::new(ai_owner, 0, None, false, 0, 51);
    ai_house.current_iq =
        scenario_houses.houses[0].scenario_current_iq(rules.general.max_iq_levels);
    sim.houses.insert(ai_owner, ai_house);
    let heights = BTreeMap::new();
    let hostile_target = sim
        .spawn_object("GAPOWR", "AI", 5, 5, 0, &rules, &heights)
        .expect("hostile target");
    let allied_target = sim
        .spawn_object("GAPOWR", "AI", 7, 5, 0, &rules, &heights)
        .expect("allied target");
    let null_target = sim
        .spawn_object("GAPOWR", "AI", 9, 5, 0, &rules, &heights)
        .expect("null-source target");
    let hostile_source = sim
        .spawn_object("MTNK", "ENEMY", 5, 6, 0, &rules, &heights)
        .expect("hostile source");
    let allied_source = sim
        .spawn_object("MTNK", "ALLY", 7, 6, 0, &rules, &heights)
        .expect("allied source");
    for target_id in [hostile_target, allied_target, null_target] {
        sim.substrate
            .entities
            .get_mut(target_id)
            .unwrap()
            .health
            .current = 200;
    }
    let warhead_ref = sim.interner.intern("HITWH");
    let events = [
        EntityDamageEvent::area(
            hostile_target,
            10,
            0,
            hostile_source,
            Some(enemy_owner),
            warhead_ref,
        ),
        EntityDamageEvent::area(
            allied_target,
            10,
            0,
            allied_source,
            Some(ally_owner),
            warhead_ref,
        ),
        EntityDamageEvent::area(null_target, 10, 0, RAD_NO_ATTACKER, None, warhead_ref),
    ];
    let mut alliances = HouseAllianceMap::new();
    alliances
        .entry("AI".to_string())
        .or_default()
        .insert("ALLY".to_string());
    let mut main_rng = SimRng::new(3);
    let mut handled_deaths = Vec::new();
    let mut fatal_lifecycle = None;
    let mut sound_sink = None;
    let _ = commit_damage_events(
        &events,
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        &rules,
        &mut sim.interner,
        &mut sim.houses,
        &sim.session.house_order,
        &alliances,
        &mut main_rng,
        &mut sim.scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert!(
        sim.substrate
            .entities
            .get(hostile_target)
            .unwrap()
            .was_attacked_by_enemy,
        "surviving hostile source sets the persistent Techno tail byte"
    );
    assert!(
        !sim.substrate
            .entities
            .get(allied_target)
            .unwrap()
            .was_attacked_by_enemy,
        "target-owner alliance suppresses the hostile latch"
    );
    assert!(
        !sim.substrate
            .entities
            .get(null_target)
            .unwrap()
            .was_attacked_by_enemy,
        "null radiation/environment source cannot set it"
    );
    let latched_hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(hostile_target)
        .unwrap()
        .was_attacked_by_enemy = false;
    assert_ne!(
        sim.state_hash(),
        latched_hash,
        "the persistent byte is hashed"
    );
    sim.substrate
        .entities
        .get_mut(hostile_target)
        .unwrap()
        .was_attacked_by_enemy = true;

    let low_iq_rng = sim.scenario_rng.logical_state();
    crate::sim::production::tick_repairs(&mut sim, &rules);
    assert!(
        sim.substrate
            .entities
            .get(hostile_target)
            .unwrap()
            .lifecycle
            .object_alive,
        "scenario CurrentIQ 1 stays below RepairSell/SellBack 2"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        low_iq_rng,
        "an IQ-gated-out building draws no low-credit sale RNG"
    );

    sim.houses.get_mut(&ai_owner).unwrap().current_iq = 2;
    let mut expected_rng = sim.scenario_rng.clone();
    assert!(
        expected_rng.next_range_u32_inclusive(0, 0x32) < 51,
        "TechLevel 51 makes every inclusive native roll win"
    );
    crate::sim::production::tick_repairs(&mut sim, &rules);
    let sold = sim.substrate.entities.get(hostile_target).unwrap();
    assert!(!sold.lifecycle.object_alive && sold.lifecycle.in_limbo);
    assert!(
        sim.substrate
            .entities
            .get(allied_target)
            .unwrap()
            .lifecycle
            .object_alive
    );
    assert!(
        sim.substrate
            .entities
            .get(null_target)
            .unwrap()
            .lifecycle
            .object_alive
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state(),
        "the single qualifying building consumes exactly one inclusive roll"
    );
}

/// Buildings never take radiation damage; an ImmuneToRadiation unit on the
/// same cell is also exempt.
#[test]
fn buildings_take_no_rad_damage() {
    let rules = radiation_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let building = sim
        .spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules, &heights)
        .expect("building spawns");
    let deso = sim
        .spawn_object("DESO", "Americans", 6, 5, 0, &rules, &heights)
        .expect("desolator spawns");
    sim.radiation.apply_detonation(
        crate::sim::radiation::RadDetonation {
            rx: 5,
            ry: 5,
            rad_level: 500,
            spread: 2,
        },
        0,
        &rules.radiation,
        None,
    );

    rad_combat_tick(&mut sim, &rules, 16);
    assert_eq!(
        sim.substrate.entities.get(building).unwrap().health.current,
        750,
        "buildings are exempt from radiation damage"
    );
    assert_eq!(
        sim.substrate.entities.get(deso).unwrap().health.current,
        200,
        "ImmuneToRadiation units are exempt"
    );
}

/// A deployed Desolator force-fires its deploy weapon at its own cell when no
/// site exists there, arming a full-level site; while the site's effective
/// level stays at or above a third of the weapon's RadLevel the gate is
/// closed and it does not fire again.
#[test]
fn deployed_desolator_self_irradiates_and_refires_below_third() {
    let rules = radiation_rules();
    let mut sim = crate::sim::world::Simulation::new();
    let heights = BTreeMap::new();
    let deso = sim
        .spawn_object("DESO", "Americans", 10, 10, 0, &rules, &heights)
        .expect("desolator spawns");
    sim.substrate.entities.get_mut(deso).unwrap().deploy_state =
        Some(crate::sim::deploy::DeployPhase::Deployed);

    // Tick 1: gate open (no site) → self-targeted deploy-weapon shot.
    let result = rad_combat_tick(&mut sim, &rules, 1);
    assert_eq!(
        result.consequences.fire_events().len(),
        1,
        "deployed self-irradiate fires"
    );
    assert_eq!(
        sim.interner
            .resolve(result.consequences.fire_events()[0].weapon_id),
        "RadEruptionWeapon"
    );
    assert_eq!(
        result.consequences.fire_events()[0].target,
        TargetKind::Cell(10, 10)
    );
    let site = sim
        .radiation
        .site_at((10, 10))
        .expect("detonation armed a site at the desolator's cell");
    assert_eq!(site.level, 500);
    assert_eq!(sim.radiation.cell_level((10, 10)), 500.0);

    // Tick 2: gate closed (effective 500 ≥ 500/3) → no fire, self-target
    // cleared.
    let result = rad_combat_tick(&mut sim, &rules, 2);
    assert_eq!(
        result.consequences.fire_events().len(),
        0,
        "gate closed after re-arm"
    );
    assert!(
        sim.substrate
            .entities
            .get(deso)
            .unwrap()
            .attack_target
            .is_none(),
        "synthesized self-target is cleared once the gate closes"
    );

    // Decay the site below RadLevel/3 (= 166): effective = remaining×500/500
    // drops below 166 once remaining < 167.
    for frame in 3..=340 {
        sim.radiation.tick_decay(frame, &rules.radiation, None);
    }
    let site = sim.radiation.site_at((10, 10)).expect("site still alive");
    assert!(crate::sim::radiation::RadiationState::current_site_level(site) < 500 / 3);

    // Gate reopens → fires again and merges the site back up.
    let result = rad_combat_tick(&mut sim, &rules, 341);
    assert_eq!(
        result.consequences.fire_events().len(),
        1,
        "gate reopens below one third"
    );
    let site = sim.radiation.site_at((10, 10)).expect("merged site");
    assert!(site.level > 500, "re-detonation merged effective + added");
}

#[test]
fn under_attack_events_fire_for_sourced_structures_and_harvester_types() {
    // The damage-apply producer, per `BuildingClass::ReceiveDamage @
    // 0x00442230` (sourced, non-zero result, `Insignificant=` clear — no
    // attacker-house test) and `UnitClass::ReceiveDamage 0x007384B9`
    // (`Harvester=` type flag, any source).
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=HARV\n1=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=CAGAS\n1=CATREE\n2=YAREFN\n\n\
         [CAGAS]\nStrength=800\nArmor=wood\n\n\
         [CATREE]\nStrength=800\nArmor=wood\nInsignificant=yes\n\n\
         [YAREFN]\nStrength=800\nArmor=wood\nUndeploysInto=HARV\nResourceGatherer=yes\n\n\
         [HARV]\nStrength=1000\nArmor=heavy\nSpeed=4\nHarvester=yes\n\n\
         [MTNK]\nStrength=1000\nArmor=heavy\nSpeed=4\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n",
    ))
    .expect("rules parse");

    let mut main_rng = SimRng::new(1);
    let mut run_attack = |victim: GameEntity| -> CombatTickResult {
        let mut store = EntityStore::new();
        store.insert(victim);
        let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
        attacker.owner = test_intern("Attacker");
        store.insert(attacker);
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 10, None, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut main_rng,
        )
    };

    // Enemy-owned Structure → one base ping for the VICTIM's owner.
    let mut building = make_entity_owned(10, "CAGAS", 8, 5, 800, "Defender");
    building.category = EntityCategory::Structure;
    let result = run_attack(building);
    assert_eq!(
        result.consequences.effects().under_attack_events.len(),
        1,
        "structure hit pings"
    );
    let ev = &result.consequences.effects().under_attack_events[0];
    assert!(!ev.miner);
    assert!(ev.structure);
    assert_eq!(ev.owner, test_intern("Defender"));
    assert_eq!((ev.rx, ev.ry), (8, 5));

    // `Harvester=` vehicle → miner ping (the type flag, `UnitType+0xE0E`,
    // not the Rust miner component).
    let harv = make_entity_owned(10, "HARV", 8, 5, 1000, "Defender");
    let result = run_attack(harv);
    assert_eq!(
        result.consequences.effects().under_attack_events.len(),
        1,
        "harvester hit pings"
    );
    assert!(result.consequences.effects().under_attack_events[0].miner);
    assert!(!result.consequences.effects().under_attack_events[0].structure);

    // SAME-owner structure damage → still pings: `BuildingClass::
    // ReceiveDamage` never compares the source's house before
    // `NotifyUnderAttack` (force-fire on an own building announces).
    let mut friendly = make_entity_owned(10, "CAGAS", 8, 5, 800, "Attacker");
    friendly.category = EntityCategory::Structure;
    let result = run_attack(friendly);
    assert_eq!(
        result.consequences.effects().under_attack_events.len(),
        1,
        "own-fire pings"
    );
    assert_eq!(
        result.consequences.effects().under_attack_events[0].owner,
        test_intern("Attacker")
    );

    // `Insignificant=yes` building → `BuildingType+0x232` skip.
    let mut prop = make_entity_owned(10, "CATREE", 8, 5, 800, "Defender");
    prop.category = EntityCategory::Structure;
    let result = run_attack(prop);
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "insignificant buildings never ping"
    );

    // Deployed slave miner (`UndeploysInto=` + `ResourceGatherer=yes`
    // building) → the ore-miner line through the building path.
    let mut slave = make_entity_owned(10, "YAREFN", 8, 5, 800, "Defender");
    slave.category = EntityCategory::Structure;
    let result = run_attack(slave);
    assert_eq!(result.consequences.effects().under_attack_events.len(), 1);
    assert!(result.consequences.effects().under_attack_events[0].miner);
    assert!(result.consequences.effects().under_attack_events[0].structure);

    // Plain vehicle (not `Harvester=`, not a structure) → no ping.
    let plain = make_entity_owned(10, "MTNK", 8, 5, 1000, "Defender");
    let result = run_attack(plain);
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "plain unit hits do not ping"
    );
}

/// `TechnoClass::Death_Announcement @ 0x004D98C0` inputs: a damage kill of a
/// non-building emits one `UnitLostEvent`; a `Spawned=` type (`0x004D98DD`)
/// and a building (no `+0x3B8` caller in `BuildingClass`) emit none.
#[test]
fn unit_lost_events_come_from_damage_kills_of_unspawned_non_buildings() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n0=HORNET\n\n\
         [BuildingTypes]\n0=CAGAS\n\n\
         [CAGAS]\nStrength=10\nArmor=wood\n\n\
         [MTNK]\nStrength=10\nArmor=heavy\nSpeed=4\n\n\
         [HORNET]\nStrength=10\nArmor=light\nSpeed=4\nSpawned=yes\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [M60]\nDamage=250\nROF=20\nRange=5\nWarhead=SA\n\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("rules parse");

    let mut main_rng = SimRng::new(1);
    let mut run_kill = |victim: GameEntity| -> CombatTickResult {
        let mut store = EntityStore::new();
        store.insert(victim);
        let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
        attacker.owner = test_intern("Attacker");
        store.insert(attacker);
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 10, None, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut main_rng,
        )
    };

    let tank = make_entity_owned(10, "MTNK", 8, 5, 10, "Defender");
    let result = run_kill(tank);
    assert_eq!(
        result.consequences.effects().unit_lost_events.len(),
        1,
        "vehicle kill announces"
    );
    assert_eq!(
        result.consequences.effects().unit_lost_events[0].owner,
        test_intern("Defender")
    );
    assert_eq!(
        (
            result.consequences.effects().unit_lost_events[0].rx,
            result.consequences.effects().unit_lost_events[0].ry
        ),
        (8, 5)
    );

    let mut hornet = make_entity_owned(10, "HORNET", 8, 5, 10, "Defender");
    hornet.category = EntityCategory::Aircraft;
    let result = run_kill(hornet);
    assert!(
        result.consequences.effects().unit_lost_events.is_empty(),
        "Spawned= types are silent"
    );

    let mut shack = make_entity_owned(10, "CAGAS", 8, 5, 10, "Defender");
    shack.category = EntityCategory::Structure;
    let result = run_kill(shack);
    assert!(
        result.consequences.effects().unit_lost_events.is_empty(),
        "buildings have no Death_Announcement caller"
    );
}

/// `UnitClass::ReceiveDamage @ 0x00737C90`: `0x00737D69 CMP EAX,4` splits the
/// result. The `Harvester=` ping (`0x007384B9..0x00738530`) lives only in the
/// `result != 4` arm; a killing blow takes the death arm, where only
/// `Death_Announcement` (`+0x3B8`) speaks. So a one-shot miner kill is "Unit
/// lost" alone, never "Ore miner under attack" as well.
#[test]
fn harvester_killing_blow_announces_unit_lost_without_the_miner_ping() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=HARV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [HARV]\nStrength=1000\nArmor=heavy\nSpeed=4\nHarvester=yes\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [M60]\nDamage=250\nROF=20\nRange=5\nWarhead=SA\n\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("rules parse");

    let mut main_rng = SimRng::new(1);
    let mut run_attack = |victim: GameEntity| -> CombatTickResult {
        let mut store = EntityStore::new();
        store.insert(victim);
        let mut attacker = make_infantry_entity(1, "E1", 5, 5, 125);
        attacker.owner = test_intern("Attacker");
        store.insert(attacker);
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 10, None, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut main_rng,
        )
    };

    // Non-lethal hit: the `result != 4` arm pings and nothing dies.
    let result = run_attack(make_entity_owned(10, "HARV", 8, 5, 1000, "Defender"));
    assert_eq!(
        result.consequences.effects().under_attack_events.len(),
        1,
        "survivor hit pings"
    );
    assert!(result.consequences.effects().under_attack_events[0].miner);
    assert!(
        result.consequences.effects().unit_lost_events.is_empty(),
        "nothing died"
    );

    // One-shot kill: result 4 skips the ping; only the death arm speaks.
    let result = run_attack(make_entity_owned(10, "HARV", 8, 5, 10, "Defender"));
    assert!(
        result.consequences.effects().under_attack_events.is_empty(),
        "a killing blow never reaches the miner ping"
    );
    assert_eq!(
        result.consequences.effects().unit_lost_events.len(),
        1,
        "the kill announces once"
    );
    assert_eq!(
        result.consequences.effects().unit_lost_events[0].owner,
        test_intern("Defender")
    );
}

/// Build one ObjectType straight from an INI body, so the `Cost=` parse feeding
/// the score award is exercised rather than a hand-set field.
fn object_with_body(body: &str) -> ObjectType {
    let ini = IniFile::from_str(&format!(
        "[TEST]
{body}"
    ));
    ObjectType::from_ini_section(
        "TEST",
        ini.section("TEST").expect("test section"),
        crate::rules::object_type::ObjectCategory::Vehicle,
    )
}

#[test]
fn score_award_is_the_victim_cost_scaled_by_veterancy() {
    // gamemd's kill-record step values the victim at its `Cost=`, doubled at
    // veteran and tripled at elite. Anchored on stock Rhino (HTNK) Cost=900.
    let obj = object_with_body(
        "Cost=900
",
    );
    assert_eq!(obj.cost, 900, "Cost= parsed off the section");
    assert_eq!(score_award_for_victim(Some(&obj), 0), 900);
    assert_eq!(score_award_for_victim(Some(&obj), 99), 900);
    assert_eq!(score_award_for_victim(Some(&obj), 100), 1_800);
    assert_eq!(score_award_for_victim(Some(&obj), 199), 1_800);
    assert_eq!(score_award_for_victim(Some(&obj), 200), 2_700);
}

#[test]
fn score_award_ignores_the_dormant_points_key() {
    // `Points=` parses into a type field the binary never reads back — dormant
    // TS legacy in YR — so this engine does not parse it and a section carrying
    // only `Points=` is worth nothing. Stock GI (E1) is Cost=200 / Points=10; the
    // award must follow the cost, not the points.
    let points_only = object_with_body(
        "Points=10
",
    );
    assert_eq!(score_award_for_victim(Some(&points_only), 0), 0);

    let gi = object_with_body(
        "Cost=200
Points=10
",
    );
    assert_eq!(score_award_for_victim(Some(&gi), 0), 200);
}

#[test]
fn score_award_is_zero_without_a_cost_or_a_resolvable_type() {
    let obj = object_with_body(
        "Strength=100
",
    );
    assert_eq!(score_award_for_victim(Some(&obj), 200), 0);
    assert_eq!(score_award_for_victim(None, 200), 0);
}

#[test]
fn projectile_shrapnel_targets_hostile_head_before_random_cell_child() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n[MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n[PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n[PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=2\n\n[CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n[CHILDPROJ]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("shrapnel rules");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet");
    source.lifecycle.cell_marked = true;
    entities.insert(source);
    // First ring-table entry is (+1,-1).
    let mut target = make_entity_owned(2, "MTNK", 6, 4, 100, "Americans");
    target.lifecycle.cell_marked = true;
    entities.insert(target);
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact: crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: crate::sim::projectile::ProjectilePayload {
            base_damage: 20,
            warhead: interner.intern("WH"),
            weapon: interner.intern("PARENT"),
        },
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let mut scenario_rng = SimRng::new(0x46_a310);
    let mut expected_rng = scenario_rng.clone();
    let _ = expected_rng.next_range_u32_inclusive(0, 4);
    let _ = expected_rng.next_range_u32_inclusive(0, 4);
    let mut main_rng = SimRng::new(1);
    let mut houses = BTreeMap::new();

    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    align_attackers_to_targets(&mut entities, &rules, &interner);
    let result = tick_combat_with_fog_and_main_rng(
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        &mut houses,
        &[],
        &crate::map::houses::HouseAllianceMap::default(),
        None,
        None,
        None,
        None,
        1,
        100,
        1,
        &[1, 2],
        &[detonation],
        &[],
        None,
        &[],
        &mut scenario_rng,
        &mut main_rng,
        None,
    );

    assert_eq!(result.projectile_spawns.len(), 2);
    assert_eq!(
        result.projectile_spawns[0].target,
        crate::sim::projectile::ProjectileTarget::Entity(2)
    );
    assert!(matches!(
        result.projectile_spawns[1].target,
        crate::sim::projectile::ProjectileTarget::Cell { .. }
    ));
    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
}

/// SpawnShrapnel's object branch aims at the hostile object's GetCoords
/// (vt+0x48 at `0x0046A614`): for a building its foundation center
/// (`0x00447AC0`), not its north-west cell.
#[test]
fn projectile_shrapnel_aims_at_a_building_foundation_center() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n[BuildingTypes]\n0=HQ\n\n[HQ]\nStrength=100\nFoundation=2x2\n\n[MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n[PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n[PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=1\n\n[CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n[CHILDPROJ]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("shrapnel rules");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet");
    source.lifecycle.cell_marked = true;
    entities.insert(source);
    // First ring-table entry is (+1,-1): the HQ's north-west cell.
    let mut target = make_entity_owned(2, "HQ", 6, 4, 100, "Americans");
    target.category = EntityCategory::Structure;
    target.lifecycle.cell_marked = true;
    entities.insert(target);
    let mut occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let impact = crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0);
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact,
        payload: crate::sim::projectile::ProjectilePayload {
            base_damage: 20,
            warhead: interner.intern("WH"),
            weapon: interner.intern("PARENT"),
        },
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };
    let mut scenario_rng = SimRng::new(0x46_a310);
    let mut main_rng = SimRng::new(1);
    let mut houses = BTreeMap::new();
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    align_attackers_to_targets(&mut entities, &rules, &interner);
    let result = tick_combat_with_fog_and_main_rng(
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        Some(handles),
        None,
        &BTreeMap::new(),
        &mut houses,
        &[],
        &crate::map::houses::HouseAllianceMap::default(),
        None,
        None,
        None,
        None,
        1,
        100,
        1,
        &[1, 2],
        &[detonation],
        &[],
        None,
        &[],
        &mut scenario_rng,
        &mut main_rng,
        None,
    );

    assert_eq!(result.projectile_spawns.len(), 1);
    let child = &result.projectile_spawns[0];
    assert_eq!(
        child.target,
        crate::sim::projectile::ProjectileTarget::Entity(2)
    );
    // 0x00447AC0: Location + ((w - 1) * 128, (h - 1) * 128) for a 2x2.
    let hq = entities.get(2).unwrap();
    let center = crate::sim::projectile::ProjectileCoord::new(
        i32::from(hq.position.rx) * 256 + hq.position.sub_x.to_num::<i32>() + 128,
        i32::from(hq.position.ry) * 256 + hq.position.sub_y.to_num::<i32>() + 128,
        0,
    );
    assert_eq!(child.initial_target_position, center);
    assert_eq!(
        child.velocity,
        crate::sim::projectile::launch::shrapnel_launch_velocity(impact, center, 40, false)
    );
}

#[test]
fn gsi_04_01_projectile_shrapnel_captures_each_shared_dummy_lookup() {
    use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeStampSlot};
    use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
    use crate::sim::projectile::{
        ProjectileCoord, ProjectileTarget, dummy_cell_target_coord, projectile_random_shrapnel_cell,
    };
    use crate::util::lepton::{BRIDGE_HEIGHT_DELTA_LEPTONS, ground_height_leptons};

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n[MTNK]\nStrength=100\nArmor=heavy\nPrimary=PARENT\nSecondary=CHILD\n\n[PARENT]\nDamage=20\nROF=10\nRange=6\nSpeed=30\nProjectile=PARENTPROJ\nWarhead=WH\n\n[PARENTPROJ]\nAirburst=yes\nShrapnelWeapon=CHILD\nShrapnelCount=3\n\n[CHILD]\nDamage=5\nROF=10\nRange=3\nSpeed=40\nProjectile=CHILDPROJ\nWarhead=WH\n\n[CHILDPROJ]\nSubjectToWalls=yes\n\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("shrapnel rules");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "MTNK", 5, 5, 100, "Soviet");
    source.lifecycle.cell_marked = true;
    entities.insert(source);
    let occupancy = OccupancyGrid::rebuild(&entities);
    let mut interner = test_interner();
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 7,
        source_id: 1,
        target: ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact: ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: crate::sim::projectile::ProjectilePayload {
            base_damage: 20,
            warhead: interner.intern("WH"),
            weapon: interner.intern("PARENT"),
        },
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };

    let mut scenario_rng = SimRng::new(0x46_a310);
    let mut expected_rng = scenario_rng.clone();
    let expected_cells = [
        projectile_random_shrapnel_cell(5, 5, &mut expected_rng),
        projectile_random_shrapnel_cell(5, 5, &mut expected_rng),
        projectile_random_shrapnel_cell(5, 5, &mut expected_rng),
    ];
    assert_ne!(expected_cells[0], expected_cells[1]);
    assert_ne!(expected_cells[1], expected_cells[2]);

    // The declared map has storage for every request, but only the first
    // random coordinate has a native CellClass pointer. The next two lookups
    // both return and restamp one shared dummy identity.
    let mut terrain = super::impact_height_tests::terrain_at_level(2);
    terrain.test_set_native_allocated_cells(&[(
        expected_cells[0].0 as u16,
        expected_cells[0].1 as u16,
    )]);
    terrain.test_set_dummy_cell_level_slope(2, 0);
    let dummy = terrain.shared_cell_dummy();
    dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);

    let expected_target = |(rx, ry): (i32, i32), structural: bool| {
        let x = rx * 256 + 128;
        let y = ry * 256 + 128;
        let z = ground_height_leptons(2, 0, x, y).expect("flat CellClass surface is supported")
            + if structural {
                BRIDGE_HEIGHT_DELTA_LEPTONS as i32
            } else {
                0
            };
        ProjectileCoord::new(x, y, z)
    };
    let expected_positions = [
        expected_target(expected_cells[0], false),
        expected_target(expected_cells[1], true),
        expected_target(expected_cells[2], true),
    ];
    let mut out = CombatEmit::default();
    emit_projectile_shrapnel(
        &detonation,
        &entities,
        &occupancy,
        &rules,
        &mut interner,
        Some(&terrain),
        &HouseAllianceMap::default(),
        &mut scenario_rng,
        &mut out,
    );

    assert_eq!(scenario_rng.logical_state(), expected_rng.logical_state());
    assert_eq!(out.projectile_spawns.len(), 3);
    assert_eq!(
        out.projectile_spawns[0].target,
        ProjectileTarget::Cell {
            rx: expected_cells[0].0 as u16,
            ry: expected_cells[0].1 as u16,
        }
    );
    assert_eq!(out.projectile_spawns[1].target, ProjectileTarget::DummyCell);
    assert_eq!(out.projectile_spawns[2].target, ProjectileTarget::DummyCell);
    for (index, spawn) in out.projectile_spawns.iter().enumerate() {
        assert_eq!(spawn.initial_target_position, expected_positions[index]);
        assert_eq!(
            spawn.velocity,
            crate::sim::projectile::launch::shrapnel_launch_velocity(
                detonation.impact,
                expected_positions[index],
                40,
                true
            )
        );
    }
    assert_ne!(
        out.projectile_spawns[1].initial_target_position,
        out.projectile_spawns[2].initial_target_position
    );

    let final_snapshot = dummy.snapshot();
    assert_eq!(final_snapshot.coord, expected_cells[2]);
    assert_ne!(
        final_snapshot.bridge_flags_0x1180 & BRIDGE_FLAG_STRUCTURAL,
        0
    );
    assert!(dummy.same_identity(&terrain.shared_cell_dummy()));

    // A later miss restamps the retained pointer for BulletClass::AI without
    // rewriting either child's constructor-time launch coordinate.
    let later = get_cellclass_fallback(Some(&terrain), 9, 10);
    let CellRef::Dummy { cell: later_dummy } = later else {
        panic!("native-unallocated later lookup must retain the shared dummy");
    };
    assert!(dummy.same_identity(&later_dummy));
    assert_eq!(dummy.snapshot().coord, (9, 10));
    let later_target = dummy_cell_target_coord(&later_dummy);
    assert_eq!(later_target.x, 9 * 256 + 128);
    assert_eq!(later_target.y, 10 * 256 + 128);
    assert_eq!(later_target.z, 2 * 104 + BRIDGE_HEIGHT_DELTA_LEPTONS as i32);
    assert_ne!(
        later_target,
        out.projectile_spawns[2].initial_target_position
    );
    assert_eq!(
        out.projectile_spawns
            .iter()
            .map(|spawn| spawn.initial_target_position)
            .collect::<Vec<_>>(),
        expected_positions
    );
}

#[test]
fn gsi_04_10_near_center_iron_curtain_isolates_earlier_terrain_receiver() {
    use crate::sim::combat::combat_aoe::AreaDamageReceiver;
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};

    fn run(kind: InvulnKind, techno_distance: i32) -> i32 {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nTreeStrength=100\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n\
             [Warheads]\n0=WOODWH\n\
             [VICTIM]\nStrength=100\nArmor=wood\n\
             [TREE01]\nStrength=100\nArmor=wood\nImmune=no\n\
             [WOODWH]\nWood=yes\nCellSpread=.5\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
        ))
        .expect("Terrain isolation rules");
        let mut sim = crate::sim::world::Simulation::new();
        sim.resolve_type_handles(&rules);
        let victim_id = sim
            .spawn_object("VICTIM", "VictimHouse", 5, 5, 0, &rules, &BTreeMap::new())
            .expect("protected Techno spawns");
        sim.substrate
            .entities
            .get_mut(victim_id)
            .unwrap()
            .invulnerability = Some(InvulnerabilityState {
            start_frame: 0,
            duration_frames: 100,
            kind,
        });

        let terrain_id = 700;
        let terrain_ref = sim.interner.intern("TREE01");
        sim.production.terrain_objects.insert(
            terrain_id,
            TerrainObjectState {
                stable_id: terrain_id,
                native_unique_id: None,
                in_logic_vector: false,
                type_ref: terrain_ref,
                rx: 5,
                ry: 5,
                health: 100,
                max_health: 100,
                occupation_bits: 4,
                lifecycle: TerrainObjectLifecycle::Live,
            },
        );
        sim.production
            .terrain_object_cells
            .insert((5, 5), terrain_id);

        let warhead_ref = sim.interner.intern("WOODWH");
        let mut entity_event = EntityDamageEvent::area(
            victim_id,
            10,
            techno_distance,
            RAD_NO_ATTACKER,
            None,
            warhead_ref,
        );
        entity_event.near_center_ic_isolation_eligible = true;
        let receivers = [
            AreaDamageReceiver::Terrain(TerrainDamageEvent {
                stable_id: terrain_id,
                rx: 5,
                ry: 5,
                damage: 10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: true,
            }),
            AreaDamageReceiver::Entity(entity_event),
        ];
        sim.commit_noncombat_aoe_receivers(&rules, None, &receivers);
        sim.production.terrain_objects[&terrain_id].health
    }

    assert_eq!(
        run(InvulnKind::IronCurtain, 84),
        100,
        "the later active IC record arms isolation for an earlier Terrain record"
    );
    assert_eq!(
        run(InvulnKind::IronCurtain, 85),
        90,
        "the native distance boundary is strict less-than 85"
    );
    assert_eq!(
        run(InvulnKind::ForceShield, 84),
        90,
        "Force Shield receives through isolation but never arms it"
    );
}

#[test]
fn gsi_04_10_entity_fatal_hook_and_later_terrain_share_raw_occupation() {
    use crate::sim::combat::combat_aoe::AreaDamageReceiver;
    use crate::sim::terrain_object::{
        TerrainObjectLifecycle, TerrainObjectState, mark_terrain_raw_occupation,
    };

    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTreeStrength=10\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n0=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TerrainTypes]\n0=TREE01\n\
         [Warheads]\n0=WOODWH\n\
         [VICTIM]\nStrength=10\nArmor=wood\nSpeed=6\n\
         [TREE01]\nStrength=10\nArmor=wood\nImmune=no\n\
         [WOODWH]\nWood=yes\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("shared raw-occupation rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolve_type_handles(&rules);
    let entity_id = sim
        .spawn_object("VICTIM", "VictimHouse", 4, 5, 0, &rules, &BTreeMap::new())
        .expect("fatal vehicle spawns");
    let terrain_id = 701;
    let terrain_ref = sim.interner.intern("TREE01");
    sim.production.terrain_objects.insert(
        terrain_id,
        TerrainObjectState {
            stable_id: terrain_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: terrain_ref,
            rx: 5,
            ry: 5,
            health: 10,
            max_health: 10,
            occupation_bits: 4,
            lifecycle: TerrainObjectLifecycle::Live,
        },
    );
    sim.production
        .terrain_object_cells
        .insert((5, 5), terrain_id);
    mark_terrain_raw_occupation(&mut sim.substrate.raw_cell_occupation, (5, 5), 4);
    assert_ne!(sim.substrate.raw_cell_occupation.ground_bits(4, 5), 0);
    assert_ne!(sim.substrate.raw_cell_occupation.ground_bits(5, 5), 0);

    let warhead_ref = sim.interner.intern("WOODWH");
    let receivers = [
        AreaDamageReceiver::Entity(EntityDamageEvent::area(
            entity_id,
            10,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead_ref,
        )),
        AreaDamageReceiver::Terrain(TerrainDamageEvent {
            stable_id: terrain_id,
            rx: 5,
            ry: 5,
            damage: 10,
            distance_leptons: 0,
            warhead_ref,
            near_center_ic_isolation_eligible: false,
        }),
    ];
    sim.commit_noncombat_aoe_receivers(&rules, None, &receivers);

    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(4, 5),
        0,
        "World UnInit clears the Techno bit through the lent authoritative raw grid"
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(5, 5),
        0,
        "the later Terrain finalize observes and mutates that same grid"
    );
    assert_eq!(
        sim.production.terrain_objects[&terrain_id].lifecycle,
        TerrainObjectLifecycle::Destroyed
    );
}

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
    killer.owner = test_intern("Soviet");
    let mut store = EntityStore::new();
    store.insert(killer);
    // Intern the victim type before snapshotting the thread-local test
    // interner, or the award cannot resolve its `Cost=`.
    let _ = test_intern("HTNK");
    let mut interner = test_interner();
    let mut scenario_rng = SimRng::new(9);

    for victim_id in 2..=6u64 {
        store.insert(make_entity_owned(victim_id, "HTNK", 8, 5, 1, "Americans"));
        issue_attack_command(&mut store, 1, victim_id, None, &interner);
        // Each victim is a fresh shot: clear the reload the last kill armed.
        if let Some(attacker) = store.get_mut(1) {
            attacker.rearm_timer = crate::sim::timer::CdTimer::default();
        }
        align_attackers_to_targets(&mut store, &rules, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut scenario_rng,
        );
        store.remove(victim_id);
        ranks.push(store.get(1).expect("killer").veterancy);
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
            crate::sim::combat::veterancy::set_elite(&mut firer);
        }
        store.insert(firer);
        let _ = test_intern("HTNK");
        store.insert(make_entity_owned(2, "HTNK", 8, 5, 400, "Americans"));
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        align_attackers_to_targets(&mut store, &rules, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut SimRng::new(0x475A_5A4C),
        );
        let dealt = 400 - store.get(2).expect("target").health.current;
        let reload = store
            .get(1)
            .map(|attacker| attacker.rearm_timer.duration() as u16)
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
            crate::sim::combat::veterancy::set_elite(&mut firer);
        }
        store.insert(firer);
        let _ = test_intern("HTNK");
        store.insert(make_entity_owned(2, "HTNK", 8, 5, 200, "Soviet"));
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        align_attackers_to_targets(&mut store, &rules, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut SimRng::new(0x475A_5A4C),
        );
        store.get(2).expect("patient").health.current - 200
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
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(0x475A_5A4C),
    );
    assert_eq!(
        result.consequences.fire_events().len(),
        1,
        "the Dolphin fired"
    );
    assert_eq!(store.get(2).expect("target").health.current, 100);
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
            firer.bunker_link = crate::sim::game_entity::BunkerLink::Installed(99);
        }
        store.insert(firer);
        let _ = test_intern("MTNK");
        store.insert(make_entity_owned(2, "MTNK", 8, 5, 400, "Americans"));
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        align_attackers_to_targets(&mut store, &rules, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut SimRng::new(0x475A_5A4C),
        );
        400 - store.get(2).expect("target").health.current
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
    building.category = EntityCategory::Structure;
    let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 1);
    assert!(cargo.board(1, 1));
    building.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
    store.insert(building);
    let mut occupant = make_infantry_entity(1, "E1", 5, 5, 125);
    occupant.passenger_role = crate::sim::passenger::PassengerRole::Inside { transport_id: 10 };
    store.insert(occupant);
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 10, 2, None, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(1),
    );
    assert_eq!(
        result.consequences.fire_events().len(),
        1,
        "the garrison fired"
    );
    assert_eq!(store.get(2).expect("target").health.current, 125 - 36);
}

/// `UnitClass::Death_Explosion @ 0x00738680` plays one anim from the dying
/// type's own `Explosion=` list and then one from `DestroyAnim=`, at its own
/// coordinate, one `Random__Next()` draw each. Before this the type's list had
/// no reader at all and every vehicle died with the warhead's puff.
#[test]
fn gsi_08_11_unit_death_plays_type_explosion_then_destroy_anim() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n[HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\nExplosion=TWLT070,TWLT120\nDestroyAnim=SMOKEY\n[105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("death-explosion fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    align_attackers_to_targets(&mut store, &rules, &interner);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(4),
    );

    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    let explosion_index = names
        .iter()
        .position(|name| *name == "TWLT070" || *name == "TWLT120")
        .expect("the type's own Explosion= anim");
    let destroy_index = names
        .iter()
        .position(|name| *name == "SMOKEY")
        .expect("the type's DestroyAnim=");
    assert!(
        explosion_index < destroy_index,
        "Explosion= precedes DestroyAnim=: {names:?}"
    );
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
        victim.dont_score = true;
        store.insert(victim);
        issue_attack_command(&mut store, 1, victim_id, None, &interner);
        // Each victim is a fresh shot: clear the reload the last kill armed.
        if let Some(attacker) = store.get_mut(1) {
            attacker.rearm_timer = crate::sim::timer::CdTimer::default();
        }
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut scenario_rng,
        );
        store.remove(victim_id);
    }

    let killer = store.get(1).expect("killer");
    assert_eq!(killer.veterancy, 0, "five DontScore kills earn nothing");
    assert_eq!(killer.veterancy_raw.bits(), 0);
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
    building.category = EntityCategory::Structure;
    let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 1);
    assert!(cargo.board(1, 1));
    assert!(cargo.board(3, 1));
    building.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
    store.insert(building);
    for id in [1, 3] {
        let mut occupant = make_infantry_entity(id, "E1", 5, 5, 125);
        occupant.owner = test_intern("Soviet");
        occupant.passenger_role = crate::sim::passenger::PassengerRole::Inside { transport_id: 10 };
        store.insert(occupant);
    }
    let mut victim = make_infantry_entity(2, "E2", 8, 5, 1);
    victim.owner = test_intern("Americans");
    store.insert(victim);
    let mut interner = test_interner();
    issue_attack_command(&mut store, 10, 2, None, &interner);
    // The occupant at fire index 0 shoots; the one after it is next in line.
    let (shooter, next) = {
        let cargo = store.get(10).unwrap().passenger_role.cargo().unwrap();
        assert_eq!(cargo.garrison_fire_index, 0);
        (cargo.passengers[0], cargo.passengers[1])
    };
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(1),
    );

    assert_eq!(store.get(2).map_or(0, |victim| victim.health.current), 0);
    let raw = |id: u64| store.get(id).unwrap().veterancy_raw.bits();
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
    building.category = EntityCategory::Structure;
    let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 1);
    assert!(cargo.board(1, 1));
    assert!(cargo.board(3, 1));
    building.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
    store.insert(building);
    // Whichever occupant sits at index 0 shoots with the quick gun; the one
    // after it carries the slow gun.
    let (shooter, next) = {
        let cargo = store.get(10).unwrap().passenger_role.cargo().unwrap();
        (cargo.passengers[0], cargo.passengers[1])
    };
    for (id, kind) in [(shooter, "E1"), (next, "E2")] {
        let mut occupant = make_infantry_entity(id, kind, 5, 5, 125);
        occupant.owner = test_intern("Soviet");
        occupant.passenger_role = crate::sim::passenger::PassengerRole::Inside { transport_id: 10 };
        store.insert(occupant);
    }
    let mut victim = make_infantry_entity(2, "E3", 8, 5, 400);
    victim.owner = test_intern("Americans");
    store.insert(victim);
    let mut interner = test_interner();
    issue_attack_command(&mut store, 10, 2, None, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(1),
    );

    assert!(
        store.get(2).unwrap().health.current < 400,
        "the quick gun fired"
    );
    let building = store.get(10).unwrap();
    assert_eq!(
        building.passenger_role.cargo().unwrap().garrison_fire_index,
        1,
        "the turn passed to the next occupant"
    );
    // SlowGun's 120 over two occupants is about 60 frames; FastGun's 10 would
    // be about 5.
    let rearm = building.rearm_timer.duration();
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
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);
    tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(1),
    );
    assert_eq!(store.get(2).map_or(0, |victim| victim.health.current), 0);
    assert_eq!(store.get(1).unwrap().veterancy_raw.bits(), 0);
}

/// The shot leaves the BARREL, not the hull centre.
///
/// `TechnoClass::Fire_At` launches from `GetFLH @ 0x006F3AD0`, and the stock
/// MTNK fixture (`PrimaryFireFLH=190,25,120`, body north, turret east) puts that
/// muzzle at `+189, -25, +120` leptons from the object coordinate — 189, not
/// 190, because retail composes two table rotations whose residual truncates the
/// X term down a lepton.
#[test]
fn gsi_08_04_projectile_spawns_at_the_muzzle_not_the_hull_centre() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\nTurret=yes\n[HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n[105mm]\nDamage=65\nROF=50\nRange=6\nSpeed=40\nProjectile=Cannon\nWarhead=AP\n[Cannon]\nArcing=true\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("fire-origin fixture parses");
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[MTNK]\nPrimaryFireFLH=190,25,120\n"),
    ));

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
    // Body facing north; the turret is aimed east at the target.
    shooter.facing = 0;
    shooter.barrel_facing = Some(crate::sim::movement::facing_class::FacingClass::new(
        0x4000, 0,
    ));
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 2000, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(3),
    );

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the shot creates a tracked projectile");
    let hull_x = i32::from(store.get(1).unwrap().position.rx) * 256
        + store.get(1).unwrap().position.sub_x.to_num::<i32>();
    let hull_y = i32::from(store.get(1).unwrap().position.ry) * 256
        + store.get(1).unwrap().position.sub_y.to_num::<i32>();
    assert_eq!(
        (spawn.origin.x - hull_x, spawn.origin.y - hull_y),
        (189, -25),
        "muzzle offset in leptons"
    );
}

/// A `Dropping=` shell (BulletType `+0x29C`) is the exception: FireAt swaps
/// its launch source for the firer's GetCoords (`0x006FE2D8..0x006FE2FE`)
/// before the launch distance and the delta, so it leaves the hull centre.
/// Same fixture as `gsi_08_04`, whose `Arcing=` shell keeps the muzzle.
#[test]
fn gsi_08_04_a_dropping_shell_leaves_the_hull_centre() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\nTurret=yes\n[HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n[105mm]\nDamage=65\nROF=50\nRange=6\nSpeed=40\nProjectile=Bomb\nWarhead=AP\n[Bomb]\nDropping=yes\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("dropping fixture parses");
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[MTNK]\nPrimaryFireFLH=190,25,120\n"),
    ));

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
    shooter.facing = 0;
    shooter.barrel_facing = Some(crate::sim::movement::facing_class::FacingClass::new(
        0x4000, 0,
    ));
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 2000, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(3),
    );

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the shot creates a tracked projectile");
    let shooter = store.get(1).unwrap();
    let hull = (
        i32::from(shooter.position.rx) * 256 + shooter.position.sub_x.to_num::<i32>(),
        i32::from(shooter.position.ry) * 256 + shooter.position.sub_y.to_num::<i32>(),
    );
    assert_eq!((spawn.origin.x, spawn.origin.y), hull);
}

/// `TechnoClass::FireAt 0x006FEA36`..`0x006FEA4C`: a `ROT > 0` shot leaves the
/// tube at ONE lepton per frame and the weapon's `Speed=` is stored as
/// `Bullet+0x110` instead, which `BulletTypeClass::Acceleration` (`+0x2D0`)
/// then ramps toward. The launch direction is the aim facing, not the bearing
/// to the target.
#[test]
fn gsi_08_06_homing_launch_uses_one_lepton_and_stores_speed_as_the_ceiling() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=LNCHR\n1=HTNK\n\
         [LNCHR]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=Rocket\nTurret=yes\n\
         [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=Rocket\n\
         [Rocket]\nDamage=65\nROF=50\nRange=6\nSpeed=30\nProjectile=Seeker\nWarhead=AP\n\
         [Seeker]\nROT=60\nArm=2\nRanged=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("homing launch fixture parses");

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "LNCHR", 5, 5, 300, "Soviet");
    shooter.facing = 0;
    shooter.barrel_facing = Some(crate::sim::movement::facing_class::FacingClass::new(
        0x4000, 0,
    ));
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 2000, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(3),
    );

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the missile is a tracked projectile");
    assert_eq!(
        spawn.speed_leptons_per_frame, 1,
        "every ROT > 0 launch starts at one lepton per frame"
    );
    let guidance = spawn.guidance.expect("a ROT > 0 shot carries guidance");
    assert_eq!(guidance.max_speed, 30, "weapon Speed= is only the ceiling");
    assert_eq!(guidance.acceleration, 3, "BulletTypeClass ctor default");
    assert_eq!(
        guidance.heading_bam, 0,
        "the turret faces east (0x4000), which is heading BAM 0"
    );
    assert_eq!(
        guidance.fuse_reference, spawn.initial_target_position,
        "the ProximityDetector reference is frozen on the launch-time target"
    );
    assert_eq!(spawn.velocity, ProjectileVelocity::new(1, 0, 0));
}

/// A launch with no ballistic solution skips the rest of the shot. Under
/// `Gravity=200` an `Arcing=` shell one cell out has none: the speed FireAt
/// clamps to half the distance (128) is far short of an arc. The bullet is
/// deleted (`0x006FF000` -> `0x006FF93C`) and FireAt resumes at `0x006FF749`,
/// past the burst step, GetROF, the rearm and the `+0x120` store, so the tank
/// may try again next frame. The same shell with `Arcing=no` launches.
#[test]
fn gsi_08_06_a_failed_arc_launch_skips_the_rest_of_the_shot() {
    let shoot = |arcing: &str| {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nFixtureOnly=1\n[AudioVisual]\nGravity=200\n\
             [VehicleTypes]\n0=ARTY\n1=HTNK\n\
             [ARTY]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=Shell\n\
             [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\n\
             [Shell]\nDamage=65\nROF=50\nBurst=2\nRange=6\nSpeed=200\nProjectile=Lob\nWarhead=AP\n\
             [Lob]\nArcing={arcing}\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        )))
        .expect("failed launch fixture parses");
        let mut store = EntityStore::new();
        let mut shooter = make_entity_owned(1, "ARTY", 5, 5, 300, "Soviet");
        shooter.facing = 64;
        store.insert(shooter);
        let _ = test_intern("HTNK");
        store.insert(make_entity_owned(2, "HTNK", 6, 5, 2000, "Americans"));
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        let before = store.get(1).unwrap().clone();
        let mut rng = SimRng::new(3);
        let rng_before = rng.logical_state();
        let result = tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut rng,
        );
        let shooter = store.get(1).unwrap();
        (
            result.projectile_spawns.len(),
            result.consequences.fire_events().len(),
            rng.logical_state() != rng_before,
            shooter.rearm_timer != before.rearm_timer,
            shooter.weapon_burst.index() != before.weapon_burst.index(),
            shooter.last_fire_frame != before.last_fire_frame,
        )
    };
    assert_eq!(
        shoot("no"),
        (1, 1, true, true, true, true),
        "the control launches"
    );
    assert_eq!(
        shoot("yes"),
        (0, 0, false, false, false, false),
        "no bullet, no report or anim, no GetROF draw, no rearm, no burst step, no \
         last-fire store"
    );
}

/// `TechnoClass::FireAt 0x006FF28F..0x006FF2BE`: the duration stored from
/// every GetROF value the oracle ran (`tools/spatial_oracle/rearm_timer.py`,
/// `fire` rows, int32 extremes included) is `fireat_rearm_frames` of it,
/// halved toward zero for a berserk firer. (The rows also record the start
/// frame and the `+0x2F8` copy, which VERA does not keep.)
#[test]
fn gsi_08_05_rearm_frames_match_the_original() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/rearm_timer.json"
    ))
    .unwrap();
    let rows = vectors["fire"].as_array().unwrap();
    assert_eq!(rows.len(), 89);
    for row in rows {
        let field = |name: &str| row["input"][name].as_i64().unwrap() as i32;
        assert_eq!(
            i64::from(super::world_receiver::fireat_rearm_frames(
                field("rof"),
                field("berserk") != 0
            )),
            row["duration"].as_i64().unwrap(),
            "{row}"
        );
    }
}

/// `TechnoClass::FireAt 0x006FF28F..0x006FF29C`: a berserk firer (`+0x298`,
/// set by a `Psychedelic=` warhead) re-arms with half of GetROF's value,
/// signed and toward zero.
#[test]
fn gsi_08_05_a_berserk_firer_rearms_at_half_its_rof() {
    let rearm = |berserk: bool| {
        let rules = test_rules();
        let mut store = EntityStore::new();
        let mut tank = make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet");
        tank.berserk.active = berserk;
        store.insert(tank);
        store.insert(make_entity_owned(2, "MTNK", 7, 5, 300, "Americans"));
        let mut interner = test_interner();
        issue_attack_command(&mut store, 1, 2, None, &interner);
        align_attackers_to_targets(&mut store, &rules, &interner);
        tick_combat(
            &mut store,
            &mut OccupancyGrid::new(),
            &rules,
            &mut interner,
            0,
            100,
            0,
            &mut SimRng::new(7),
        );
        store.get(1).unwrap().rearm_timer.duration()
    };
    let (normal, berserk) = (rearm(false), rearm(true));
    assert!(normal > 1, "precondition: the tank fired ({normal})");
    assert_eq!(berserk, normal / 2);
}

/// `TechnoClass::FireAt 0x006FE9FE`: EVERY launch speed is clamped to half the
/// straight-line distance to the target before the homing/vertical override.
/// For a `ROT=0` shell the speed clamped is `WeaponTypeClass::GetSpeed`'s
/// (`ftol(Sqrt_Approx(d * Gravity * 1.2))`, `0x00773070`), not `Speed=`;
/// under `[AudioVisual] Gravity=200` one cell out that is 247, above the
/// clamp's 128. The shell is straight (not `Arcing=`), so no arc solution is
/// needed at that gravity.
#[test]
fn gsi_08_06_point_blank_shot_clamps_the_launch_speed_to_half_the_distance() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nFixtureOnly=1\n[AudioVisual]\nGravity=200\n\
         [VehicleTypes]\n0=ARTY\n1=HTNK\n\
         [ARTY]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=Shell\n\
         [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=Shell\n\
         [Shell]\nDamage=65\nROF=50\nRange=6\nSpeed=200\nProjectile=Lob\nWarhead=AP\n\
         [Lob]\nArcing=no\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("clamp fixture parses");

    let mut store = EntityStore::new();
    let mut shooter = make_entity_owned(1, "ARTY", 5, 5, 300, "Soviet");
    // Turretless: the hull facing is what `GetFireError` gates on, so aim it
    // east at the target before the shot.
    shooter.facing = 64;
    store.insert(shooter);
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 6, 5, 2000, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(3),
    );

    let spawn = result
        .projectile_spawns
        .first()
        .expect("the shell is a tracked projectile");
    let dx = spawn.initial_target_position.x - spawn.origin.x;
    let dy = spawn.initial_target_position.y - spawn.origin.y;
    let dz = spawn.initial_target_position.z - spawn.origin.z;
    let distance = (f64::from(dx * dx + dy * dy + dz * dz)).sqrt().trunc() as i32;
    let get_speed = crate::sim::projectile::launch::weapon_launch_speed(
        200,
        Some(crate::sim::projectile::launch::LaunchSpeedProjectile {
            rot: 0,
            floater: false,
        }),
        200,
        crate::sim::projectile::launch::fireat_launch_distance(
            spawn.origin,
            spawn.initial_target_position,
        ),
    );
    assert!(
        distance / 2 < get_speed,
        "the fixture must be closer than twice GetSpeed's launch speed ({get_speed})"
    );
    assert_eq!(
        i32::from(spawn.speed_leptons_per_frame),
        distance / 2,
        "the launch speed is clamped to dist/2"
    );
}

/// A flat level-0 grid, so the `Vertical` arm's floor probe has a ground
/// surface to reach.
fn flat_level_zero_terrain(
    width: u16,
    height: u16,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};

    let speed_costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for ry in 0..height {
        for rx in 0..width {
            cells.push(ResolvedTerrainCell {
                rx,
                ry,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: true,
                tileset_index: Some(0),
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs,
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: TerrainClass::Clear,
                base_speed_costs: speed_costs,
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

/// GSI-08.08 end to end: the Kirov bomb has to FALL and explode.
///
/// `[BlimpBombP]` is `Vertical=yes` with `Acceleration=1`, no `ROT=` and no
/// `Ranged=`, so it carries no fuse and no steering — its only terminations are
/// the `Vertical` arm's own probes at `BulletClass::AI 0x00467334`ff.
/// (`DetonationAltitude`, then `GetAltitude() < 0`, then the bridge deck).
/// `TechnoClass::FireAt` gives a `Vertical` launch a pitch only when the
/// muzzle-to-target separation exceeds 200 leptons, so if the firer's hover
/// altitude failed to reach the launch coordinate the bomb would leave level,
/// never cross `DetonationAltitude=20000`, never drop below the floor, and
/// drift off the map unexploded. This pins the whole chain instead: the Jumpjet
/// firer's `JumpjetHeight=` becomes the locomotor's hover target, that hover
/// altitude reaches `object_world_z_leptons`, the launch pitch points down, and
/// the flight ends in a detonation.
///
/// The fixture is stock-faithful on the jumpjet keys: like retail `[ZEP]` it
/// omits `JumpJet=` and takes its Jumpjet locomotor from the GUID alone, and
/// `JumpjetHeight=750` still lands, because `TechnoTypeClass::ReadINI @
/// 0x00715151` stores it into `TechnoType+0xD80` unconditionally (over the
/// constructor default 500 at `0x007115D3`). A native Kirov really does hover
/// at 750.
#[test]
fn gsi_08_08_kirov_vertical_bomb_falls_and_detonates() {
    use crate::map::resolved_terrain::SharedCellDummy;
    use crate::sim::projectile::{ProjectileStore, ProjectileTrajectory};

    // Stock `[BlimpBombP]`, `[BlimpBomb]` and the Kirov's `JumpjetHeight=750`,
    // with one deliberate change: `Range=` is widened from the stock 1.5 so the
    // shot clears the fire-range gate while the firer hovers 750 leptons up
    // (nothing on the `Vertical` arm reads `Range=`). `JumpJet=` stays absent,
    // as in stock. See the doc comment.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=ZEP\n1=HTNK\n\
         [ZEP]\nStrength=2000\nArmor=medium\nSpeed=5\nPrimary=BlimpBomb\n\
         BalloonHover=yes\nJumpjetHeight=750\nConsideredAircraft=yes\n\
         MovementZone=Fly\nSpeedType=Hover\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n\
         [HTNK]\nStrength=2000\nArmor=heavy\nSpeed=4\n\
         [BlimpBomb]\nDamage=250\nBurst=1\nROF=50\nRange=6\nSpeed=20\n\
         Projectile=BlimpBombP\nWarhead=BlimpHE\nOmniFire=yes\n\
         [BlimpBombP]\nArm=10\nAcceleration=1\nVertical=yes\nDetonationAltitude=20000\n\
         [BlimpHE]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("Kirov bomb fixture parses");

    let mut store = EntityStore::new();
    let mut kirov = make_entity_owned(1, "ZEP", 5, 5, 2000, "Soviet");
    kirov.category = EntityCategory::Aircraft;
    let zep_object = rules.object("ZEP").expect("ZEP object type");
    let mut locomotor =
        crate::sim::movement::locomotor::LocomotorState::from_object_type(zep_object, 0);
    // The hover altitude is NOT hand-set: `JumpjetHeight=750` has to arrive
    // through the native Jumpjet Link_To_Object copy as the hover target, and
    // the airship is then placed at the top of its climb. Assert the rules hop
    // at its source so a broken parse fails here rather than downstream.
    assert_eq!(
        locomotor.jumpjet_runtime().unwrap().params.height,
        750,
        "`JumpjetHeight=750` must reach the locomotor's hover target; a 500 here \
         means the rules->locomotor hop is broken, not the flight model"
    );
    locomotor.altitude = crate::util::fixed_math::SimFixed::from_num(
        locomotor.jumpjet_runtime().unwrap().params.height,
    );
    kirov.locomotor = Some(locomotor);
    store.insert(kirov);
    let _ = test_intern("HTNK");
    // Directly beneath the airship, the way a Kirov bombs.
    store.insert(make_entity_owned(2, "HTNK", 5, 5, 2000, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(7),
    );

    let spawn = *result
        .projectile_spawns
        .first()
        .expect("the Kirov bomb is a tracked projectile");
    assert!(
        matches!(spawn.trajectory, ProjectileTrajectory::Vertical { .. }),
        "Vertical=yes takes the third arm"
    );
    assert!(
        spawn.origin.z >= 750,
        "the Jumpjet hover altitude must reach the launch coordinate, else the \
         bomb leaves level and never terminates; got {}",
        spawn.origin.z
    );
    assert!(
        f64::from_bits(spawn.velocity.z.bits()) < 0.0,
        "the launch pitch has to point the bomb DOWN; got {:?}",
        spawn.velocity
    );

    let terrain = flat_level_zero_terrain(10, 10);
    let ground = ProjectileCoord::new(5 * 256, 5 * 256, 0);
    let mut projectiles = ProjectileStore::new();
    let id = projectiles.spawn(1, spawn);
    let dummy = SharedCellDummy::fresh();
    for frame in 1..600u32 {
        let step = projectiles
            .advance_one(
                id,
                frame,
                |_| Some(ground),
                Some(&terrain),
                &dummy,
                rules.general.gravity,
                false,
                |_, _, _| None,
            )
            .expect("the bomb is still in flight");
        assert!(
            step.expired.is_empty(),
            "the bomb must not vanish unexploded at frame {frame}"
        );
        if let Some(detonation) = step.detonations.first() {
            assert_eq!(detonation.projectile_id, id);
            // The descending bomb admits at the floor probe: 467350..467368.
            // The common admitted-impact clamp at 467BF0..467C06 then moves
            // its final coordinate to the floor before the damage handoff.
            assert_eq!(
                detonation.impact.z, 0,
                "the below-floor admission hands damage off at the level-0 floor"
            );
            return;
        }
    }
    panic!("the Kirov bomb never detonated");
}

/// A destroyed vehicle with `MaxDebris=` alone scatters SHP chunks from
/// `[General] MetallicDebris=`.
///
/// gamemd-derived: `TechnoClass::ReceiveDamage @ 0x00701900`. With
/// `DebrisAnims.Count == 0` (`0x007023FC`) and `DebrisTypes.Count == 0`
/// (`0x007024D2`) the whole budget goes to the arm at `0x007024E0`, one
/// `RandomRanged(0, RulesClass+0x14C - 1)` per chunk at `0x0070253A`. 155 stock
/// TechnoTypes take exactly this path in gamemd — `MTNK` (Grizzly), `ZEP` (Kirov
/// Airship), `NAPSYA` (Psychic Amplifier), and 11 of the 12 `[AircraftTypes]`;
/// the twelfth, `APACHE`, has no `[APACHE]` section in `rulesmd.ini` at all, so
/// it keeps the `TechnoTypeClass` constructor default of 0 (`0x00710B00` ->
/// `0x00710FB7`) and takes no draw. Before this landed, none of them threw
/// anything.
///
/// 155, not 168: `ObjectType::from_ini_section` reads `MaxDebris=` case-exactly,
/// the way gamemd's `INIClass` does — it CRCs the raw key bytes and folds no
/// case anywhere on the path. 17 stock `[VehicleTypes]` spell the key
/// `Maxdebris=` — 14 buildable (Rhino, Apocalypse, Lasher, Tesla, Gattling,
/// Prism, V3, Magnetron, Master Mind, Battle Fortress, IFV, Flak Track, Landing
/// Craft, Armored Transport) and 3 at `TechLevel=-1` (`CMON`, `HORV`, `UTNK`) —
/// and every one of them takes the constructor default 0 and throws no debris
/// at all. 13 of the 17 would have sat on this metallic arm, which is exactly
/// the 168 - 155 gap; the other 4 (`CMON`, `FV`, `HORV`, `HTK`) author
/// `DebrisTypes=TIRE` and would have sat on the voxel arm. All three exemplars
/// named above spell `MaxDebris` exactly, so they are unaffected either way.
#[test]
fn gsi_05_14_a_dying_vehicle_scatters_metallic_debris() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\nMaxDebris=4\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("metallic-debris fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(9),
    );

    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    assert!(
        names.iter().any(|name| name.starts_with("DBRIS")),
        "the death should scatter MetallicDebris chunks, got {names:?}"
    );
    assert!(
        result.consequences.effects().voxel_debris.is_empty(),
        "a type with no DebrisTypes= throws no VoxelAnims"
    );
}

/// `DebrisAnims=` on the section wins over `[General] MetallicDebris=`.
///
/// gamemd-derived: `0x007023EF` reads `TechnoType+0x5D4` first and only a zero
/// count falls through to the Rules list at `0x007024BB`. All 166 stock
/// `DebrisAnims=` lines sit on `[BuildingTypes]` sections, so only a structure
/// ever takes this arm — but not every structure does: of the 292 registered
/// `[BuildingTypes]` that throw, 126 carry `MaxDebris=` alone and fall through
/// to `[General] MetallicDebris=`. All three figures are basis-independent: the
/// 17 sections that spell the key `Maxdebris=` are every one of them a
/// `[VehicleTypes]`, so the case-exact read moves no building count.
#[test]
fn gsi_05_14_a_dying_building_uses_its_own_debris_anims() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n\
         MinDebris=3\nMaxDebris=4\nDebrisAnims=DBRI-WM1,DBRI-WM2\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("debris-anims fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(21),
    );

    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    let own: usize = names.iter().filter(|n| n.starts_with("DBRI-WM")).count();
    // `MinDebris=3`, `MaxDebris=4` pins the budget at 3 with no draw.
    assert_eq!(
        own, 3,
        "the whole budget comes from DebrisAnims=: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("DBRIS")),
        "the Rules MetallicDebris arm must not also fire: {names:?}"
    );
}

/// A destroyed harvester throws `[VoxelAnims]` tyres, and its budget never
/// reaches the SHP arms.
///
/// gamemd-derived: with `DebrisTypes.Count > 0` the loop at `0x007022FE`
/// drains the budget to zero before either SHP arm is tested, so a type that
/// names `DebrisTypes=` throws VXL debris and nothing else. All 36 stock
/// `DebrisTypes=` lines name `TIRE`, and 32 of the 36 reach the block in gamemd
/// — `CMON`, `FV`, `HORV` and `HTK` spell `Maxdebris=` and so get the default 0.
/// `[HARV]` spells `MaxDebris=6` exactly, with `DebrisMaximums=4`.
#[test]
fn gsi_05_14_a_dying_harvester_throws_voxel_tires_and_no_shp_debris() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VoxelAnims]\n1=TIRE\n\
         [TIRE]\nElasticity=0.8\nMinAngularVelocity=12.0\nMaxAngularVelocity=24.0\n\
         MinZVel=28.0\nMaxZVel=32.0\nMaxXYVel=10.0\nDuration=150\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n\
         MinDebris=5\nMaxDebris=6\nDebrisTypes=TIRE\nDebrisMaximums=4\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("voxel-debris fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);

    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut SimRng::new(5),
    );

    // MinDebris == MaxDebris - 1 pins the budget at 5 with no draw, and the
    // loop drains exactly that many.
    assert_eq!(result.consequences.effects().voxel_debris.len(), 5);
    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    assert!(
        !names.iter().any(|n| n.starts_with("DBRI")),
        "the voxel loop spends the whole budget: {names:?}"
    );
    // Every piece launches from the wreck's own coordinate, lifted 10 leptons.
    for piece in &result.consequences.effects().voxel_debris {
        assert_eq!(piece.object.duration, 150);
        assert_eq!(piece.object.world_coord().z, 10);
    }
}

/// A type with `MaxDebris=0` costs the shared stream nothing.
///
/// `TEST ECX,ECX / JLE 0x00702572` at `0x00702291` skips the whole block above
/// the budget draw. No `[InfantryTypes]` section authors `MaxDebris=` at all —
/// 0 of the 65 — so this is the common case, and consuming a draw here would
/// shift the cursor for every later consumer in the tick.
#[test]
fn gsi_05_14_a_type_without_maxdebris_takes_no_draw() {
    let ini = "[General]\nMetallicDebris=DBRIS1LG,DBRIS2LG,DBRIS3LG\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nCost=700\nPrimary=105mm\n\
         [HTNK]\nStrength=1\nArmor=heavy\nSpeed=4\nCost=900\nPrimary=105mm\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let rules = RuleSet::from_ini(&IniFile::from_str(ini)).expect("no-debris fixture parses");

    let mut store = EntityStore::new();
    store.insert(make_entity_owned(1, "MTNK", 5, 5, 300, "Soviet"));
    let _ = test_intern("HTNK");
    store.insert(make_entity_owned(2, "HTNK", 8, 5, 1, "Americans"));
    let mut interner = test_interner();
    issue_attack_command(&mut store, 1, 2, None, &interner);
    align_attackers_to_targets(&mut store, &rules, &interner);

    let mut rng = SimRng::new(64);
    let result = tick_combat(
        &mut store,
        &mut OccupancyGrid::new(),
        &rules,
        &mut interner,
        0,
        100,
        0,
        &mut rng,
    );
    assert!(result.consequences.effects().voxel_debris.is_empty());
    let names: Vec<&str> = result
        .consequences
        .effects()
        .explosion_effects
        .iter()
        .map(|effect| interner.resolve(effect.shp_name))
        .collect();
    assert!(
        !names.iter().any(|n| n.starts_with("DBRI")),
        "no MaxDebris= means no debris at all: {names:?}"
    );
}

/// GSI-08.08 — a special detonation arm suppresses shrapnel and area damage,
/// but NOT the shared tail.
///
/// `BulletClass::DetonateAtCoord @ 0x004690b0`: only the final else at
/// `0x00469a3f` runs `BulletClass::SpawnShrapnel @ 0x0046a310` and
/// `Apply_area_damage @ 0x00489280`, so any arm shadows both. Every arm then
/// leaves through `JMP LAB_00469AA4`, which is the explosion-anim /
/// scorch-crater / debris / `Airburst` tail — `Warhead::SelectExplosionAnim`
/// is called from inside it at `0x00469bcf`. Stock `[Controller]` carries
/// `AnimList=YURICNTL`, so a Yuri beam impact must still draw.
#[test]
fn gsi_08_08_special_arm_suppresses_damage_but_keeps_the_detonation_tail() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=TARGET\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TARGET]\nStrength=200\nArmor=heavy\n\
         [Warheads]\n0=Controller\n1=Plain\n\
         [Controller]\nMindControl=yes\nAnimList=YURICNTL\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Plain]\nAnimList=YURICNTL\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("Controller/Plain warhead rules");

    struct TailOutcome {
        damage_events: usize,
        anims: Vec<(String, u16, u16, u8)>,
    }

    fn run(rules: &RuleSet, warhead_name: &str) -> TailOutcome {
        let mut interner = test_interner();
        let mut entities = EntityStore::new();
        entities.insert(make_entity(10, "TARGET", 8, 5, 200));
        let occupancy = OccupancyGrid::new();
        let warhead_ref = interner.intern(warhead_name);
        let detonation = ProjectileDetonation {
            projectile_id: 1,
            source_id: 77,
            target: ProjectileTarget::Entity(10),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: ProjectilePayload {
                base_damage: 50,
                warhead: warhead_ref,
                weapon: interner.intern("MissingWeapon"),
            },
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut scenario_rng = SimRng::new(9);
        let mut emitted = CombatEmit::default();
        let mut inline_hooks = None;
        let handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(rules, &mut interner);
        emit_projectile_detonations(
            &[detonation],
            &mut entities,
            &occupancy,
            rules,
            &mut interner,
            Some(handles),
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &HouseAllianceMap::new(),
            &mut scenario_rng,
            &mut inline_hooks,
            &mut emitted,
        );
        let anims: Vec<(String, u16, u16, u8)> = emitted
            .effects
            .explosion_effects
            .iter()
            .map(|effect| {
                (
                    interner.resolve(effect.shp_name).to_string(),
                    effect.rx,
                    effect.ry,
                    effect.z,
                )
            })
            .collect();
        TailOutcome {
            damage_events: emitted.damage_events.len(),
            anims,
        }
    }

    let mind_control = run(&rules, "Controller");
    assert_eq!(
        mind_control.damage_events, 0,
        "the MindControl arm at 0x00469211 shadows Apply_area_damage"
    );
    assert_eq!(
        mind_control.anims,
        vec![("YURICNTL".to_string(), 8, 5, 0)],
        "LAB_00469AA4 still selects and starts the AnimList explosion"
    );

    let plain = run(&rules, "Plain");
    assert_eq!(
        plain.damage_events, 1,
        "the ordinary arm at 0x00469a3f is unchanged"
    );
    assert_eq!(
        plain.anims, mind_control.anims,
        "the tail selects the same anim at the same place whichever arm reached it"
    );
}

/// `BulletClass::ResolveImpactCoordAndDetonate @ 0x00468D80`: a `Cluster=5`
/// shot detonates at its impact, then four times 256..512 leptons around that
/// impact (`0x0049F420` is handed the copy made at `0x00469008`), never around
/// the previous cluster.
#[test]
fn clusters_scatter_around_the_impact() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=LAUNCHER\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [LAUNCHER]\nStrength=100\nPrimary=ClusterGun\n\
         [ClusterGun]\nDamage=10\nProjectile=ClusterP\nWarhead=Blast\n\
         [ClusterP]\nCluster=5\n\
         [Warheads]\n0=Blast\n\
         [Blast]\nAnimList=EXPLOSML\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("cluster rules");
    let mut interner = test_interner();
    let mut entities = EntityStore::new();
    let occupancy = OccupancyGrid::new();
    let impact = ProjectileCoord::new(40 * 256 + 64, 50 * 256 + 200, 0);
    let detonation = ProjectileDetonation {
        projectile_id: 1,
        source_id: 77,
        target: ProjectileTarget::Cell { rx: 40, ry: 50 },
        impact,
        payload: ProjectilePayload {
            base_damage: 10,
            warhead: interner.intern("Blast"),
            weapon: interner.intern("ClusterGun"),
        },
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut scenario_rng = SimRng::new(11);
    let mut emitted = CombatEmit::default();
    let mut inline_hooks = None;
    let handles =
        crate::sim::type_handle_table::ResolvedRuleHandles::resolve(&rules, &mut interner);
    emit_projectile_detonations(
        &[detonation],
        &mut entities,
        &occupancy,
        &rules,
        &mut interner,
        Some(handles),
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        &HouseAllianceMap::new(),
        &mut scenario_rng,
        &mut inline_hooks,
        &mut emitted,
    );
    let points: Vec<(i32, i32)> = emitted
        .effects
        .explosion_effects
        .iter()
        .map(|effect| {
            (
                i32::from(effect.rx) * 256 + effect.sub_x.to_num::<i32>(),
                i32::from(effect.ry) * 256 + effect.sub_y.to_num::<i32>(),
            )
        })
        .collect();
    assert_eq!(points.len(), 5, "one anim per cluster");
    assert_eq!(points[0], (impact.x, impact.y), "the first at the impact");
    for &(x, y) in &points[1..] {
        let (dx, dy) = (f64::from(x - impact.x), f64::from(y - impact.y));
        let distance = dx.hypot(dy);
        assert!(
            (255.0..=513.0).contains(&distance),
            "each later cluster lies 256..512 leptons from the impact, got {distance}"
        );
    }
}

/// GSI-08.33 — `DirectRocker` is the chain's only conditional arm.
///
/// `BulletClass::DetonateAtCoord @ 0x0046978e` tests the flag, then
/// `Target != 0` (`0x004697a4`) and `Target->What_Am_I() == 1`
/// (`0x004697b2`, `UnitClass`). All three failures jump to `0x004699c4`, the
/// *next* test, so a `DirectRocker=yes` warhead that hits infantry still runs
/// ordinary damage. Dead in stock — `rulesmd.ini` carries no live
/// `DirectRocker=` line — but kept correct.
#[test]
fn gsi_08_33_direct_rocker_only_claims_a_vehicle_target() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=FOOT\n\
         [VehicleTypes]\n0=TARGET\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TARGET]\nStrength=200\nArmor=heavy\n\
         [FOOT]\nStrength=200\nArmor=none\n\
         [Warheads]\n0=Rocker\n\
         [Rocker]\nDirectRocker=yes\nAnimList=YURICNTL\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("DirectRocker warhead rules");
    assert!(
        rules.warhead("Rocker").expect("Rocker").direct_rocker,
        "DirectRocker= must reach the parsed field, not a dead raw byte"
    );

    fn run(rules: &RuleSet, category: EntityCategory, type_ref: &str) -> usize {
        let mut interner = test_interner();
        let mut entities = EntityStore::new();
        let mut target = make_entity(10, type_ref, 8, 5, 200);
        target.category = category;
        entities.insert(target);
        let occupancy = OccupancyGrid::new();
        let detonation = ProjectileDetonation {
            projectile_id: 1,
            source_id: 77,
            target: ProjectileTarget::Entity(10),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: ProjectilePayload {
                base_damage: 50,
                warhead: interner.intern("Rocker"),
                weapon: interner.intern("MissingWeapon"),
            },
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut scenario_rng = SimRng::new(9);
        let mut emitted = CombatEmit::default();
        let mut inline_hooks = None;
        let handles =
            crate::sim::type_handle_table::ResolvedRuleHandles::resolve(rules, &mut interner);
        emit_projectile_detonations(
            &[detonation],
            &mut entities,
            &occupancy,
            rules,
            &mut interner,
            Some(handles),
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &HouseAllianceMap::new(),
            &mut scenario_rng,
            &mut inline_hooks,
            &mut emitted,
        );
        assert_eq!(
            emitted.effects.explosion_effects.len(),
            1,
            "both branches reach LAB_00469AA4"
        );
        emitted.damage_events.len()
    }

    assert_eq!(
        run(&rules, EntityCategory::Unit, "TARGET"),
        0,
        "a vehicle target enters the arm and shadows Apply_area_damage"
    );
    assert_eq!(
        run(&rules, EntityCategory::Infantry, "FOOT"),
        1,
        "infantry falls through 0x004697b2 to the next test, so damage runs"
    );
}

/// A Drive `Crusher=yes` vehicle that is not `CrusherAll` leaves a plain wall
/// standing.
///
/// This is the case the change is about, and the one the sibling tests cannot
/// isolate: BFRT is Drive *and* CrusherAll, so it crushed under the old
/// locomotor gate too, and ROBO is Hover *and* not CrusherAll, so it differs on
/// both axes. HTNK differs on exactly one - Drive, `Crusher=yes`, and NOT
/// `CrusherAll` - so reverting the gate to `LocomotorKind::Drive` makes this
/// fail and nothing else in the suite notices.
///
/// Native: `0x0073B027` loads the type from `+0x6C4`, `0x0073B02D` compares
/// `TechnoTypeClass+0x5B4` (MovementZone) against `0xC` (CrusherAll), and
/// `0x0073B034 JNZ` leaves the wall alone.
#[test]
fn a_drive_crusher_without_crusherall_leaves_a_plain_wall_standing() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=HTNK\n1=BFRT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWALL\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
         [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\nDamageLevels=4\n\
         [HTNK]\nCrusher=yes\nMovementZone=Destroyer\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [BFRT]\nCrusher=yes\nMovementZone=CrusherAll\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("wall crush rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    // Same construction the sibling wall-crush tests use.
    let build = |veh_type: &str| -> Simulation {
        let mut sim = Simulation::new();
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(5, 5, 2, 0);
        sim.overlay_grid = Some(grid);
        let owner_id = sim.interner.intern("Test");
        let obj = rules.object(veh_type).expect("veh object");
        let veh_type_id = sim.interner.intern(veh_type);
        let mut veh = GameEntity::test_default(2, veh_type, "Test", 5, 5);
        veh.owner = owner_id;
        veh.type_ref = veh_type_id;
        veh.regular_crusher = obj.crusher;
        veh.locomotor =
            Some(crate::sim::movement::locomotor::LocomotorState::from_object_type(obj, 0));
        sim.substrate.entities.insert(veh);
        sim.substrate.entities.rebuild_owner_index();
        sim
    };

    let wall_present = |sim: &Simulation| -> bool {
        sim.overlay_grid
            .as_ref()
            .map(|g| g.cell(5, 5).overlay_id == Some(2))
            .unwrap_or(false)
    };

    let mut tank = build("HTNK");
    tank.apply_wall_crush_on_driveover(Some(&rules), Some(&registry));
    tank.flush_pending_delete();
    assert!(
        wall_present(&tank),
        "a Rhino is Crusher=yes and Drive, but MovementZone=Destroyer: the wall stands"
    );

    let mut fortress = build("BFRT");
    fortress.apply_wall_crush_on_driveover(Some(&rules), Some(&registry));
    fortress.flush_pending_delete();
    assert!(
        !wall_present(&fortress),
        "the Battle Fortress is CrusherAll, so the same wall falls to it"
    );
}
