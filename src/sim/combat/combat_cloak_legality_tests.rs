//! Acquisition and fire legality against cloaked and disguised candidates.
//!
//! Pins the two arms of `TechnoClass::Evaluate_Candidate @ 0x006F7CA0` that
//! Phase 6 rows GSI-12.05 / GSI-12.06 own — the cloak gate at `0x006F7DA9` and
//! the disguise gate at `0x006F84B1` — plus the target-side `GetFireError`
//! cloak exit at `0x006FC24D`.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::cloak_disguise::{
    CloakRuntime, DisguiseGateOutcome, cloak_rejects_candidate, disguise_rejects_candidate,
    is_disguised_to,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::vision::FogState;

fn legality_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nCloakingStages=9\n\
         [VehicleTypes]\n0=DEST\n1=SUB\n2=MGTK\n\
         [InfantryTypes]\n0=DOG\n1=E1\n\
         [DEST]\nStrength=600\nArmor=heavy\nSpeed=6\nSensors=yes\nSensorsSight=8\n\
         Primary=DestroyerGun\nSight=10\n\
         [SUB]\nStrength=600\nArmor=heavy\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\n\
         SensorsSight=8\nPrimary=DestroyerGun\nSight=10\n\
         [MGTK]\nStrength=400\nArmor=light\nSpeed=6\nDisguiseWhenStill=yes\nSight=10\n\
         [DOG]\nStrength=100\nArmor=none\nSpeed=6\nDetectDisguise=yes\nPrimary=DestroyerGun\n\
         Sight=10\n\
         [E1]\nStrength=100\nArmor=none\nSpeed=4\nPrimary=DestroyerGun\nSight=10\n\
         [DestroyerGun]\nDamage=60\nROF=50\nRange=8\nWarhead=PlainWH\n\
         [PlainWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("cloak legality fixture")
}

fn live(mut entity: GameEntity) -> GameEntity {
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entity.in_playfield = true;
    entity
}

fn fog() -> FogState {
    FogState {
        width: 64,
        height: 64,
        ..FogState::default()
    }
}

/// A destroyer at (10,10) and a hostile submarine one cell east.
fn destroyer_and_sub(sub_cloak_state: i32) -> (EntityStore, StringInterner, FogState) {
    let mut store = EntityStore::new();
    store.insert(live(GameEntity::test_default(
        1,
        "DEST",
        "Americans",
        10,
        10,
    )));
    let mut sub = live(GameEntity::test_default(2, "SUB", "Soviet", 11, 10));
    let mut cloak = CloakRuntime::new(0, 9);
    cloak.state = sub_cloak_state;
    sub.cloak = Some(cloak);
    store.insert(sub);
    let mut interner = test_interner();
    let americans = interner.intern("Americans");
    interner.intern("Soviet");
    let mut fog = fog();
    fog.mark_visible_for_owner(americans, 11, 10);
    (store, interner, fog)
}

fn acquire(
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker: u64,
    fog: &FogState,
) -> Option<u64> {
    acquire_best_target_for_entity(
        entities,
        &crate::sim::occupancy::OccupancyGrid::rebuild(entities),
        rules,
        interner,
        attacker,
        Some(fog),
        None,
        false,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
        None,
    )
}

#[test]
fn fully_cloaked_enemy_is_illegal_until_the_attacker_house_senses_its_cell() {
    let rules = legality_rules();
    let (entities, interner, mut fog) = destroyer_and_sub(2);
    let americans = interner.get("Americans").expect("interned attacker house");

    assert_eq!(
        acquire(&entities, &rules, &interner, 1, &fog),
        None,
        "Evaluate_Candidate 0x006F7DA9 rejects CloakState==2 with no sensor count"
    );

    // The destroyer's own `SensorsSight=` deposit is what makes it legal.
    fog.increment_sensor_at(americans, 11, 10);
    assert_eq!(
        acquire(&entities, &rules, &interner, 1, &fog),
        Some(2),
        "a positive sensor count on the candidate's cell restores legality"
    );
}

#[test]
fn cloaking_and_uncloaking_states_stay_legal() {
    let rules = legality_rules();
    for state in [0, 1, 3] {
        let (entities, interner, fog) = destroyer_and_sub(state);
        assert_eq!(
            acquire(&entities, &rules, &interner, 1, &fog),
            Some(2),
            "only state 2 is filtered; state {state} must stay legal"
        );
    }
}

#[test]
fn same_owner_is_exempt_but_an_ally_is_not() {
    let rules = legality_rules();

    // Same owner: native compares `candidate->pOwner != attacker->pOwner`, so a
    // house's own submerged sub stays a legal candidate. (It is dropped later by
    // the ordinary friendly filter, so acquisition still returns nothing — the
    // point is that the cloak arm itself does not reject it.)
    let mut store = EntityStore::new();
    store.insert(live(GameEntity::test_default(1, "DEST", "Soviet", 10, 10)));
    let mut own_sub = live(GameEntity::test_default(2, "SUB", "Soviet", 11, 10));
    let mut cloak = CloakRuntime::new(0, 9);
    cloak.state = 2;
    own_sub.cloak = Some(cloak);
    store.insert(own_sub);
    assert!(!cloak_rejects_candidate(true, false, true));

    // Alliance is NOT the test native applies.
    assert!(cloak_rejects_candidate(true, false, false));
    assert!(!cloak_rejects_candidate(true, true, false));
    assert!(!cloak_rejects_candidate(false, false, false));
    let _ = (&store, &rules);
}

#[test]
fn only_a_detect_disguise_attacker_auto_acquires_a_disguised_mirage() {
    let rules = legality_rules();
    let mut store = EntityStore::new();
    store.insert(live(GameEntity::test_default(1, "E1", "Americans", 10, 10)));
    store.insert(live(GameEntity::test_default(
        3,
        "DOG",
        "Americans",
        10,
        11,
    )));
    let mut mirage = live(GameEntity::test_default(2, "MGTK", "Soviet", 11, 10));
    let mut disguise = crate::sim::cloak_disguise::DisguiseRuntime::default();
    disguise.acquire(0, None, None);
    mirage.disguise = Some(disguise);
    store.insert(mirage);
    let mut interner = test_interner();
    let americans = interner.intern("Americans");
    interner.intern("Soviet");
    let mut fog = fog();
    fog.mark_visible_for_owner(americans, 11, 10);

    assert_eq!(
        acquire(&store, &rules, &interner, 1, &fog),
        None,
        "Evaluate_Candidate 0x006F84D4: an attacker type without DetectDisguise= is rejected"
    );
    assert_eq!(
        acquire(&store, &rules, &interner, 3, &fog),
        Some(2),
        "a DetectDisguise= attacker (dog/Yuri/PTROOP) bypasses the whole arm"
    );
}

#[test]
fn a_detect_disguise_building_covering_the_cell_restores_legality_for_everyone() {
    let rules = legality_rules();
    let mut store = EntityStore::new();
    store.insert(live(GameEntity::test_default(1, "E1", "Americans", 10, 10)));
    let mut spy = live(GameEntity::test_default(2, "MGTK", "Soviet", 11, 10));
    let mut disguise = crate::sim::cloak_disguise::DisguiseRuntime::default();
    disguise.acquire(0, None, None);
    spy.disguise = Some(disguise);
    store.insert(spy);
    let mut interner = test_interner();
    let americans = interner.intern("Americans");
    interner.intern("Soviet");
    let mut fog = fog();
    fog.mark_visible_for_owner(americans, 11, 10);

    assert_eq!(acquire(&store, &rules, &interner, 1, &fog), None);
    // `BuildingClass::AddDetectDisguiseAt @ 0x00455A80` stamps
    // `CellClass+0xAC[house]`; `IsDisguisedTo` reads it through `FUN_004870F0`.
    fog.disguise_detect_add_at(americans, (11, 10), 3);
    assert_eq!(
        acquire(&store, &rules, &interner, 1, &fog),
        Some(2),
        "a NAPSIS-shaped detect circle makes the disguise transparent to that house"
    );
}

#[test]
fn is_disguised_to_matches_the_native_clause_order() {
    // `UnitClass::IsDisguisedTo @ 0x00746750`.
    assert!(is_disguised_to(true, false, false, false, false));
    assert!(!is_disguised_to(false, false, false, false, false));
    assert!(
        !is_disguised_to(true, true, false, false, false),
        "an allied owner is never disguised to the observer"
    );
    assert!(
        !is_disguised_to(true, false, true, false, false),
        "a detect-disguise counter on the cell sees through it"
    );
    // The fake-house clause: hidden only when the fake house IS the observer or
    // one of its allies; a disguise as some third enemy is seen through.
    assert!(is_disguised_to(true, false, false, true, true));
    assert!(!is_disguised_to(true, false, false, false, true));
}

#[test]
fn the_disguise_gate_collapses_to_a_plain_reject_without_a_blink_window() {
    // Blink timer not running: native's `if (rem == 0) reject`.
    assert_eq!(
        disguise_rejects_candidate(true, false, 0, false),
        DisguiseGateOutcome::Reject
    );
    // Blink running but the attacking house is human: the very next line.
    assert_eq!(
        disguise_rejects_candidate(true, false, 5, true),
        DisguiseGateOutcome::Reject
    );
    // Blink running and the house is AI: native reaches its one Scenario draw.
    // VERA has no AI opponent, so no production caller reaches this variant.
    assert_eq!(
        disguise_rejects_candidate(true, false, 5, false),
        DisguiseGateOutcome::AiDetectionRoll
    );
    // Not disguised, or the attacker detects disguises.
    assert_eq!(
        disguise_rejects_candidate(false, false, 0, true),
        DisguiseGateOutcome::Accept
    );
    assert_eq!(
        disguise_rejects_candidate(true, true, 0, true),
        DisguiseGateOutcome::Accept
    );
}
