//! Phobos `Burst.FireWithinSequence` (`Ext/WeaponType/Body.cpp:121`) at the
//! infantry fire-frame gate (`InfantryClass_FiringAI @ 0x5209AF`,
//! `Ext/Infantry/Hooks.Firing.cpp:83-137`): the expected sequence stage moves
//! to `fireUp + cumulativeDelay`, the loop draws one `RandomRanged(3, 5)` per
//! unset `Burst.Delays` index, and a projected frame past the sequence sets
//! `TechnoExt::ForceFullRearmDelay`, which GetROF's `0x6FD054` honours.
use super::world_fire;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::movement::FacingClass;
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, Simulation};

/// An infantryman with `FireUp=2` art and a six-frame `FireUp` action, aimed
/// east at a rival target one cell out. `weapon` adds keys to `GUN`. The
/// `[CombatDamage] ROF.RandomDelay=0` single value keeps the full-rearm arm
/// draw-free, so the tests can attribute every Scenario draw.
fn infantry_arena(weapon: &str) -> (Simulation, RuleSet, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n0=FIRER\n\
         [FIRER]\nStrength=1000\nArmor=light\nPrimary=GUN\nCanPassiveAquire=no\n\
         [Warheads]\n0=HIT\n\
         [BuildingTypes]\n0=DUMMY\n\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\n\
         Projectile=INVIS\nWarhead=HIT\nBurst=3\n{weapon}\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\nBuildSpeed=.75\n[CombatDamage]\nROF.RandomDelay=0\n"
    ));
    // `FireUp=` and the sequence records are ART keys; installing the registry
    // makes `infantry_fire_frame` and the overrun's frame count read them.
    let art = crate::rules::ini_parser::IniFile::from_str(
        "[FIRER]\nSequence=FIRERSequence\nFireUp=2\n\
         [FIRERSequence]\nFireUp=164,6,6\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("rules read");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session.set_binary_frame_for_test(100);
    let firer = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("DUMMY", "Russians", 12, 10, 0, &rules)
        .expect("target");
    let east = 0x4000;
    for id in [firer, target] {
        let entity = sim
            .entities_mut_for_test()
            .get_mut(id)
            .expect("placed object");
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    let owner = sim.entities().get(target).map(|entity| entity.owner());
    let victim = sim
        .entities()
        .get(target)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .expect("target");
    sim.fog
        .mark_visible_for_owner(owner.expect("owner"), victim.0, victim.1);
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.body_facing.snap(east, 100);
    actor.barrel_facing = Some(FacingClass::new(east, 0));
    assert!(
        sim.assign_target_represented(
            firer,
            Some(TargetKind::Entity(target)),
            &rules,
            FrameEffects::empty()
        )
        .is_ok(),
        "the firer holds the target as a mission would"
    );
    (sim, rules, firer)
}

/// Put the infantryman in the retained firing sequence at `stage` with the
/// `FireUp` action (4), whose sequence record is six frames long.
fn set_firing_frame(sim: &mut Simulation, firer: u64, stage: i32) {
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.mission_leaf.set_foot_firing_sequence(1);
    actor
        .mission_leaf
        .set_infantry_doing_verified(4)
        .expect("FireUp action");
    actor.set_native_stage_value(stage);
}

/// One infantry firing update, and every projectile it launched.
fn visit(sim: &mut Simulation, rules: &RuleSet, firer: u64) -> usize {
    let before: std::collections::BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        world_fire::FireVisit::InfantryTarget(firer),
        rules,
        None,
        FrameEffects::empty(),
    );
    sim.projectiles
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .count()
}

fn index(sim: &Simulation, firer: u64) -> i32 {
    sim.entities()
        .get(firer)
        .expect("firer")
        .weapon_burst
        .index()
}

fn rearm(sim: &Simulation, firer: u64) -> i32 {
    sim.entities()
        .get(firer)
        .expect("firer")
        .rearm_timer
        .duration()
}

/// `Burst.Delays=2,0` with the key on: shots admit at `fireUp + 0/2/3` (the
/// listed zero is clamped to one frame after index 0) and the loop draws no
/// Scenario value for a listed index.
#[test]
fn listed_delays_move_the_admission_frames_without_draws() {
    let (mut sim, rules, firer) =
        infantry_arena("Burst.Delays=2,0\nBurst.FireWithinSequence=yes\n");

    // The stock frame (2) still admits the first shot; the delays are zero
    // there.
    set_firing_frame(&mut sim, firer, 2);
    sim.scenario_rng = SimRng::new(9);
    let before = sim.scenario_rng.logical_state();
    assert_eq!(visit(&mut sim, &rules, firer), 1);
    assert_eq!(index(&sim, firer), 1);
    assert_eq!(rearm(&sim, firer), 2, "next index uses delays[0]");
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "a listed index draws nothing, in the loop or in GetROF"
    );

    // 2 + delays[0] = 4 admits the second shot, whose rearm is delays[1] = 0.
    // The two-frame gap has to elapse first (GetFireError's T45).
    sim.session.set_binary_frame_for_test(102);
    set_firing_frame(&mut sim, firer, 4);
    assert_eq!(visit(&mut sim, &rules, firer), 1);
    assert_eq!(index(&sim, firer), 2);
    assert_eq!(rearm(&sim, firer), 0);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "still no draw for the listed indices"
    );

    // 2 + 2 + max(0, 1) = 5 admits the last shot. Its GetROF joins the full
    // rearm arm, where Phobos' `0x6FD0B5` hook replaces the stock jitter
    // draw; `ROF.RandomDelay=0` is a single value, so it draws nothing and
    // the rearm is the plain 40.
    sim.session.set_binary_frame_for_test(103);
    set_firing_frame(&mut sim, firer, 5);
    assert_eq!(visit(&mut sim, &rules, firer), 1);
    assert_eq!(index(&sim, firer), 0, "the burst wrapped");
    assert_eq!(rearm(&sim, firer), 40);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "the hook replaces the stock draw; a single ROF.RandomDelay draws nothing"
    );
}

/// No `Burst.Delays` entries: the loop draws `RandomRanged(3, 5)` once per
/// index above 0, in index order, before the stage equality test. Index 0
/// always answers `GetBurstDelay(0)` = 0 without a draw.
#[test]
fn unset_entries_draw_one_value_per_index_in_order() {
    let (mut sim, rules, firer) = infantry_arena("Burst.FireWithinSequence=yes\n");

    // Index 0 draws nothing even when the frame does not match.
    set_firing_frame(&mut sim, firer, 0);
    sim.scenario_rng = SimRng::new(9);
    let seed = sim.scenario_rng.logical_state();
    assert_eq!(visit(&mut sim, &rules, firer), 0);
    assert_eq!(sim.scenario_rng.logical_state(), seed);

    // Index 2 draws twice before refusing a mismatched frame.
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.weapon_burst.set_index(2);
    set_firing_frame(&mut sim, firer, 0);
    sim.scenario_rng = SimRng::new(9);
    assert_eq!(visit(&mut sim, &rules, firer), 0);
    let mut expected = SimRng::new(9);
    expected.next_range_u32_inclusive(3, 5);
    expected.next_range_u32_inclusive(3, 5);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "one draw for index 1 and one for index 2"
    );

    // The next attempt is two draws into the seed; the values it draws next
    // accumulate into the frame that admits the shot.
    let mut expected = SimRng::new(9);
    expected.next_range_u32_inclusive(3, 5);
    expected.next_range_u32_inclusive(3, 5);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    let first = expected.next_range_u32_inclusive(3, 5) as i32;
    let second = expected.next_range_u32_inclusive(3, 5) as i32;
    let frame = 2 + first.max(1) + second.max(1);
    // The completed burst's GetROF joins the full arm, where the Phobos hook
    // replaces the stock jitter draw; a single-value ROF.RandomDelay draws
    // nothing, so the two unset entries stay the whole consumption.
    let after = expected.logical_state();
    set_firing_frame(&mut sim, firer, frame);
    assert_eq!(visit(&mut sim, &rules, firer), 1);
    assert_eq!(sim.scenario_rng.logical_state(), after);
    assert_eq!(index(&sim, firer), 0, "Burst=3 wraps from index 2");
    assert_eq!(rearm(&sim, firer), 40);
}

/// The overrun arm: `fireUp + projectedDelay` past the six-frame sequence sets
/// `ForceFullRearmDelay`, so GetROF's `0x6FD054` skips the mid-burst delay and
/// the shot's burst step restarts the index at 0.
#[test]
fn overrun_arms_the_full_rearm_and_restarts_the_burst() {
    let (mut sim, rules, firer) = infantry_arena("Burst.Delays=5\nBurst.FireWithinSequence=yes\n");
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.weapon_burst.set_index(1);
    // 2 + 5 = 7 admits the shot; 2 + (5 + 5) = 12 overruns the six frames.
    set_firing_frame(&mut sim, firer, 7);
    assert_eq!(visit(&mut sim, &rules, firer), 1);
    assert_eq!(index(&sim, firer), 0, "the forced rearm restarts the burst");
    assert_eq!(
        rearm(&sim, firer),
        40,
        "0x6FD054 leaves the mid-burst branch for the full ROF"
    );
    assert!(
        !sim.entities()
            .get(firer)
            .expect("firer")
            .weapon_burst
            .force_full_rearm(),
        "the shot's burst step clears the flag"
    );
}

/// Without an overrun the flag stays clear and the mid-burst delay stands.
#[test]
fn a_frame_inside_the_sequence_keeps_the_mid_burst_delay() {
    let (mut sim, rules, firer) = infantry_arena("Burst.Delays=2\nBurst.FireWithinSequence=yes\n");
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.weapon_burst.set_index(1);
    // 2 + 2 = 4 admits; 2 + (2 + 2) = 6 does not pass the six frames.
    set_firing_frame(&mut sim, firer, 4);
    assert_eq!(visit(&mut sim, &rules, firer), 1);
    assert_eq!(index(&sim, firer), 2);
    assert_eq!(rearm(&sim, firer), 2, "the listed delay, not the full ROF");
    assert!(
        !sim.entities()
            .get(firer)
            .expect("firer")
            .weapon_burst
            .force_full_rearm()
    );
}

/// Key off: the stock stage admits the shot and the listed delays are not part
/// of the animation gate, so the key-on frame refuses and no draw happens.
#[test]
fn the_key_off_keeps_the_stock_stage() {
    let (mut sim, rules, firer) = infantry_arena("Burst.Delays=2,0\n");
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.weapon_burst.set_index(1);
    set_firing_frame(&mut sim, firer, 4);
    sim.scenario_rng = SimRng::new(9);
    let before = sim.scenario_rng.logical_state();
    assert_eq!(
        visit(&mut sim, &rules, firer),
        0,
        "the key-on frame is not the stock frame"
    );
    assert_eq!(sim.scenario_rng.logical_state(), before);

    set_firing_frame(&mut sim, firer, 2);
    assert_eq!(visit(&mut sim, &rules, firer), 1);
    assert_eq!(index(&sim, firer), 2);
}

/// A Unit whose art leaves `FireUp=` unset stays stock: `UnitTypeExt::FireUp`
/// composes -1, so the `0x736F61` arm (`phobos_unit_fire_up_tests`) holds
/// nothing and the first shot lands on frame 0.
#[test]
fn a_unit_without_the_art_key_stays_stock() {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=FIRER\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nOmniFire=yes\n\
         CanPassiveAquire=no\n\
         [Warheads]\n0=HIT\n\
         [BuildingTypes]\n0=DUMMY\n\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\n\
         Projectile=INVIS\nWarhead=HIT\nBurst=3\n\
         Burst.Delays=2,0\nBurst.FireWithinSequence=yes\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nROF.RandomDelay=0\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules read");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session.set_binary_frame_for_test(100);
    let firer = sim
        .spawn_object("FIRER", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("DUMMY", "Russians", 12, 10, 0, &rules)
        .expect("target");
    for id in [firer, target] {
        sim.entities_mut_for_test()
            .get_mut(id)
            .expect("placed object")
            .lifecycle
            .set_in_limbo_for_test(false);
    }
    let owner = sim.entities().get(target).map(|entity| entity.owner());
    let victim = sim
        .entities()
        .get(target)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .expect("target");
    sim.fog
        .mark_visible_for_owner(owner.expect("owner"), victim.0, victim.1);
    let east = 0x4000;
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.body_facing.snap(east, 100);
    actor.barrel_facing = Some(FacingClass::new(east, 0));
    assert!(
        sim.assign_target_represented(
            firer,
            Some(TargetKind::Entity(target)),
            &rules,
            FrameEffects::empty()
        )
        .is_ok()
    );
    sim.scenario_rng = SimRng::new(9);
    let before = sim.scenario_rng.logical_state();
    let before_ids: std::collections::BTreeSet<_> =
        sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        world_fire::FireVisit::UnitTarget(firer),
        &rules,
        None,
        FrameEffects::empty(),
    );
    let shots = sim
        .projectiles
        .iter()
        .filter(|(id, _)| !before_ids.contains(id))
        .count();
    assert_eq!(shots, 1, "no `FireUp=` holds nothing");
    assert_eq!(index(&sim, firer), 1);
    assert_eq!(rearm(&sim, firer), 2, "Burst.Delays still arms the rearm");
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "no admission draws on the dormant unit arm"
    );
}
