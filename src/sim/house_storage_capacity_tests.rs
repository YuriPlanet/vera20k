//! Retained capacity, through existing typed construction/lifecycle owners.
//! Numeric transfer priors are component controls, not native map admission.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{
    FrameEffects, PlacementEvidence, RevealOutcome, RevealRequest, Simulation, UninitContext,
};

fn world(base: i32, payload: i32) -> (Simulation, RuleSet, InternedId, InternedId) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&format!(
            "[BuildingTypes]\n0=BASE\n1=CAP\n2=DONT\n3=INSIG\n\
             [VehicleTypes]\n0=CARGO\n\
             [BASE]\nStrength=500\nStorage={base}\n\
             [CAP]\nStrength=500\nStorage={payload}\n\
             [DONT]\nStrength=500\nStorage=7\nDontScore=yes\n\
             [INSIG]\nStrength=500\nStorage=11\nInsignificant=yes\n\
             [CARGO]\nStrength=100\nStorage=40\n"
        )),
        &IniFile::from_str(
            "[BASE]\nFoundation=2x2\n[CAP]\nFoundation=2x2\n\
             [DONT]\nFoundation=2x2\n[INSIG]\nFoundation=2x2\n",
        ),
    )
    .unwrap();
    let mut sim = Simulation::with_seed(3);
    let first = sim.interner.intern("Americans");
    let second = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(first, 0, None, true, 0, 10));
    sim.houses
        .register(HouseState::new(second, 1, None, false, 0, 10));
    (sim, rules, first, second)
}

#[test]
fn capacity_matches_original_signed_transfer_slices() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_storage_withdrawals.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in native["transfer_rows"].as_array().unwrap() {
        // The scalar capacity write precedes raw amounts. Only its result is
        // asserted; this does not port the fixture's nonempty House slots.
        if !row["input"]["amount_bits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bits| bits.as_str() == Some("00000000"))
        {
            continue;
        }
        let storage = i32::try_from(row["input"]["capacity"].as_i64().unwrap()).unwrap();
        let remove = row["input"]["source"] == "remove";
        let base = if remove {
            100_i32.wrapping_sub(storage)
        } else {
            100
        };
        let (mut sim, rules, owner, _) = world(base, storage);
        sim.spawn_object_at_height("BASE", "Americans", 10, 10, 0, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object_limbo_at_height("CAP", "Americans", 14, 10, 0, 0, &rules)
            .unwrap();
        assert_eq!(
            sim.houses[&owner].tracking().storage_capacity(),
            base,
            "construction is not admission"
        );
        assert!(matches!(
            sim.reveal_entity_with_rules(target, &rules),
            RevealOutcome::Revealed { .. }
        ));
        if remove {
            assert_eq!(sim.houses[&owner].tracking().storage_capacity(), 100);
            sim.techno_limbo_with_rules(target, &rules, None);
        }
        assert_eq!(
            sim.houses[&owner].tracking().storage_capacity(),
            i32::try_from(row["output"]["capacity"].as_i64().unwrap()).unwrap(),
            "{} Storage={storage}",
            row["input"]["source"]
        );
        compared += 1;
    }
    assert_eq!(compared, 10);
}

#[test]
fn capacity_follows_admission_transfer_and_first_limbo() {
    let (mut sim, rules, first, second) = world(0, 23);
    let target = sim
        .spawn_object_limbo_at_height("CAP", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let position = sim.entities().get(target).unwrap().position;
    // Supplied component failure, never claimed as native scene placement.
    let rejected = RevealRequest {
        position,
        placement: PlacementEvidence::MarkFailed,
        logic_eligible: true,
    };
    assert!(matches!(
        sim.try_reveal_entity_with_context(target, rejected, UninitContext::with_rules(&rules)),
        RevealOutcome::Failed(_)
    ));
    assert_eq!(sim.houses[&first].tracking().storage_capacity(), 0);
    assert!(matches!(
        sim.reveal_entity_with_rules(target, &rules),
        RevealOutcome::Revealed { .. }
    ));
    assert_eq!(
        sim.reveal_entity_with_rules(target, &rules),
        RevealOutcome::AlreadyRevealed
    );
    sim.spawn_object_at_height("CARGO", "Americans", 30, 10, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.houses[&first].tracking().storage_capacity(),
        23,
        "mobile cargo is not House capacity"
    );
    let dont = sim
        .spawn_object_at_height("DONT", "Americans", 18, 10, 0, 0, &rules)
        .unwrap();
    let insignificant = sim
        .spawn_object_at_height("INSIG", "Americans", 22, 10, 0, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.houses[&first].tracking().storage_capacity(),
        41,
        "score filters do not suppress capacity"
    );
    sim.change_owner_with_rules(target, second, &rules, None, FrameEffects::default());
    assert_eq!(
        (
            sim.houses[&first].tracking().storage_capacity(),
            sim.houses[&second].tracking().storage_capacity()
        ),
        (18, 23)
    );
    sim.techno_limbo_with_rules(target, &rules, None);
    sim.techno_limbo_with_rules(target, &rules, None);
    assert_eq!(sim.houses[&second].tracking().storage_capacity(), 0);
    sim.change_owner_with_rules(target, first, &rules, None, FrameEffects::default());
    assert_eq!(
        sim.houses[&first].tracking().storage_capacity(),
        18,
        "limbo transfer has no capacity effect"
    );
    sim.uninit_with_rules(target, &rules);
    sim.flush_pending_delete();
    assert_eq!(
        sim.houses[&first].tracking().storage_capacity(),
        18,
        "destructor does not subtract again"
    );
    sim.uninit_with_rules(dont, &rules);
    sim.uninit_with_rules(insignificant, &rules);
    assert_eq!(sim.houses[&first].tracking().storage_capacity(), 0);
    sim.flush_pending_delete();
    assert_eq!(sim.houses[&first].tracking().storage_capacity(), 0);
}

#[test]
fn capacity_and_future_type_input_survive_snapshot_and_affect_hash() {
    let (mut sim, rules, owner, _) = world(-40, 23);
    let base = sim
        .spawn_object_at_height("BASE", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let first = sim
        .spawn_object_at_height("CAP", "Americans", 14, 10, 0, 0, &rules)
        .unwrap();
    let future = sim
        .spawn_object_limbo_at_height("CAP", "Americans", 18, 10, 0, 0, &rules)
        .unwrap();
    assert_eq!(sim.houses[&owner].tracking().storage_capacity(), -17);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "retained-capacity", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.houses[&owner].tracking().storage_capacity(), -17);
    assert_eq!(restored.state_hash(), hash);
    // Owner-local component perturbations prove both retained bytes are hashed.
    restored
        .houses
        .get_mut(&owner)
        .unwrap()
        .tracking_mut()
        .storage_capacity = -16;
    assert_ne!(restored.state_hash(), hash);
    restored
        .houses
        .get_mut(&owner)
        .unwrap()
        .tracking_mut()
        .storage_capacity = -17;
    let facts = restored
        .substrate
        .entities
        .get(future)
        .unwrap()
        .tracking_facts;
    restored
        .substrate
        .entities
        .get_mut(future)
        .unwrap()
        .tracking_facts
        .storage_capacity = 0;
    assert_ne!(
        restored.state_hash(),
        hash,
        "a limbo object's future input matters"
    );
    restored
        .substrate
        .entities
        .get_mut(future)
        .unwrap()
        .tracking_facts = facts;
    assert_eq!(restored.state_hash(), hash);
    for candidate in [&mut sim, &mut restored] {
        assert!(matches!(
            candidate.reveal_entity_with_rules(future, &rules),
            RevealOutcome::Revealed { .. }
        ));
        assert_eq!(candidate.houses[&owner].tracking().storage_capacity(), 6);
        candidate.uninit_with_rules(first, &rules);
        assert_eq!(candidate.houses[&owner].tracking().storage_capacity(), -17);
        candidate.uninit_with_rules(base, &rules);
        assert_eq!(candidate.houses[&owner].tracking().storage_capacity(), 23);
        candidate.uninit_with_rules(future, &rules);
        candidate.flush_pending_delete();
        assert_eq!(candidate.houses[&owner].tracking().storage_capacity(), 0);
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
}
