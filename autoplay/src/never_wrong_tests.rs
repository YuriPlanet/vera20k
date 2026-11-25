//! Things the army must never do, asserted directly.
//!
//! The wish list's play tests report three behaviours that should be impossible
//! rather than merely undesirable:
//!
//! 1. shooting a building the house already owns, after a capture changed hands;
//! 2. shooting something no weapon of ours can hurt — the engine's own `Illegal`
//!    answer, which the policy filters on and which therefore needs a test that
//!    fails if the filter ever stops being consulted;
//! 3. shooting the map's own furniture — a neutral house's structures, which the
//!    projection is supposed to hold apart from enemies on purpose.
//!
//! Each test below builds the smallest arena that exhibits the case and asks the
//! projection, not the policy's opinion of it.

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::world::Simulation;

/// The two-house arena with a third, passive house standing for the map's own
/// structures.
///
/// `multiplay_passive` is the flag retail uses for the neutral and civilian
/// houses, and `Observation::enemy_entities` excludes it by design: the comment
/// on `neutral_structures` says the computer AI does not go to war with the map.
fn arena() -> Option<(Simulation, RuleSet, InternedId)> {
    let retail = vera20k_core::rules::retail_ini_fixture::retail_battle_rules()?;
    let rules = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner: InternedId = sim.intern("Americans");
    Some((sim, rules, owner))
}

/// Register a passive house and stand one structure on the map for it.
fn add_passive_house(sim: &mut Simulation, name: &str) -> InternedId {
    let id = sim.intern(name);
    let mut house = vera20k_core::sim::house_state::HouseState::new(id, 0, None, false, 0, 10);
    house.set_difficulty_for_test(vera20k_core::sim::house_state::HouseDifficulty::Normal);
    house.set_multiplay_passive_for_test(true);
    sim.register_house(house);
    id
}

#[test]
fn a_passive_house_structure_is_not_an_enemy_target() {
    let Some((mut sim, rules, owner)) = arena() else {
        return;
    };
    let map_house = add_passive_house(&mut sim, "Neutral");

    // One civilian structure standing in plain sight of our own rifleman: the
    // case a play test reports as "it spends its time knocking down the map".
    let furniture = sim
        .spawn_object("CAOILD", "Neutral", 22, 20, 0, &rules)
        .expect("a neutral derrick");
    assert!(furniture > 0, "the passive house's structure exists");
    sim.resolve_type_handles(&rules);

    let ours = sim
        .spawn_object("E1", "Americans", 20, 20, 0, &rules)
        .expect("our rifleman");
    assert!(ours > 0);

    let obs = Observation::new(&sim, owner);
    let enemies: Vec<u64> = obs
        .visible_enemy_entities()
        .map(|enemy| enemy.entity_id)
        .collect();

    assert!(
        !enemies.contains(&furniture),
        "a passive house's structure is not an enemy: {enemies:?} should not hold {furniture}"
    );
    // And it is offered separately, which is what lets a capture pass decide
    // about it without the army deciding to shoot it.
    let neutral: Vec<u64> = obs
        .neutral_structures()
        .map(|neutral| neutral.entity_id)
        .collect();
    assert!(
        neutral.contains(&furniture),
        "the same structure is reachable as a neutral: {neutral:?}"
    );
    assert_ne!(map_house, owner);
}

#[test]
fn a_captured_structure_leaves_the_enemy_list() {
    let Some((mut sim, rules, owner)) = arena() else {
        return;
    };
    // Our own rifleman standing beside the target: sight comes from units, so a
    // lone structure with no friendly unit nearby is never visible and the
    // premise of this test would never be met.
    let ours = sim
        .spawn_object("E1", "Americans", 20, 20, 0, &rules)
        .expect("our rifleman");
    let theirs = sim
        .spawn_object("GAPOWR", "Russians", 22, 20, 0, &rules)
        .expect("their power plant");
    assert!(ours > 0 && theirs > 0);
    sim.resolve_type_handles(&rules);

    let mut before = Vec::new();
    for _ in 0..120 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let obs = Observation::new(&sim, owner);
        before = obs
            .visible_enemy_entities()
            .map(|enemy| enemy.entity_id)
            .collect();
        if before.contains(&theirs) {
            break;
        }
    }
    assert!(
        before.contains(&theirs),
        "the power plant starts as an enemy, which is the premise: {before:?}; \
         omniscient list: {:?}",
        Observation::new(&sim, owner)
            .enemy_entities()
            .map(|e| e.entity_id)
            .collect::<Vec<_>>()
    );

    // The capture path ends in `change_owner`; that is the only thing tested
    // here, so the assertion is about the projection reading the new owner
    // rather than about how the capture was performed.
    let americans: InternedId = sim.intern("Americans");
    sim.change_owner(theirs, americans);

    let after: Vec<u64> = Observation::new(&sim, owner)
        .visible_enemy_entities()
        .map(|enemy| enemy.entity_id)
        .collect();
    assert!(
        !after.contains(&theirs),
        "a structure the house owns is not an enemy: {after:?}"
    );
}

#[test]
fn a_zero_damage_target_is_not_reported_as_unattackable() {
    let Some((mut sim, rules, owner)) = arena() else {
        return;
    };
    // The wish list's 狗咬坦克, and the fact this test exists to pin down is the
    // one that made the policy's old filter useless: `ParasiteDog` carries
    // `Verses=100%,100%,100%,0%,0%,...`, so a dog *may* fire at a structure and
    // simply does no damage. The engine's `Illegal` answer is about whether the
    // order is legal, not whether it is worth giving, and the two are not the
    // same question.
    //
    // So this asserts the negative: `engage_error` says the dog can attack, and
    // any rule that is meant to stop 狗咬坦克 has to come from the damage table
    // rather than from this accessor.
    let dog = sim
        .spawn_object("ADOG", "Americans", 20, 20, 0, &rules)
        .expect("our attack dog");
    let building = sim
        .spawn_object("GAPOWR", "Russians", 22, 20, 0, &rules)
        .expect("their power plant");
    assert!(dog > 0 && building > 0);
    sim.resolve_type_handles(&rules);

    let obs = Observation::new(&sim, owner);
    assert_eq!(
        obs.engage_error(&rules, dog, building),
        None,
        "a dog is allowed to bite a structure; the damage is zero, which is a \
         different question and the reason an `Illegal`-only filter never stopped \
         狗咬坦克"
    );
}

#[test]
fn a_dog_against_concrete_is_legal_and_worthless() {
    let Some((mut sim, rules, owner)) = arena() else {
        return;
    };
    // The case this session spent three rounds on: the army held position on one
    // Soviet Iron Curtain device for a whole match and its health did not move.
    // The report it printed was
    //
    //     ineffective target=2211 type=NAIRON health=750 after 450 frames
    //
    // and the reading that explains it is not a bug. `ADOG` carries
    // `Primary=GoodTeeth` (`Warhead=ParasiteDog`, `Verses=100%,100%,100%,0%,0%,…` —
    // zero against everything but infantry) *and* `Secondary=VirtualScanner`
    // (`Damage=1`, `Warhead=SA`), and `answer` walks all eight weapon slots and
    // takes the first that answers `Ok`. The scanner does answer `Ok`, so the dog
    // is allowed to attack the curtain and does one point at a time.
    //
    // So the engine's `Illegal` gate is right and cannot be the thing that stops
    // this. What is missing is a rule about whether the order is *worth giving*, and
    // that belongs to the policy, measured in damage rather than in legality.
    let dog = sim
        .spawn_object("ADOG", "Americans", 20, 20, 0, &rules)
        .expect("our attack dog");
    let curtain = sim
        .spawn_object("NAIRON", "Russians", 22, 20, 0, &rules)
        .expect("their iron curtain");
    assert!(dog > 0 && curtain > 0);
    sim.resolve_type_handles(&rules);

    let obs = Observation::new(&sim, owner);
    assert_eq!(
        obs.engage_error(&rules, dog, curtain),
        None,
        "the dog's secondary weapon answers Ok, so the order is legal; the engine is \
         not wrong here and no assertion on legality can stop this"
    );
}
