//! Phobos `ExtraRange.*` through the production `compute_in_range` gate:
//! `TechnoClass_InRange_WeaponRange @ 0x6F7248`
//! (`Ext/Techno/Hooks.WeaponRange.cpp:133..180`).

use super::extra_range_leptons;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::in_range::{compute_in_range, keep_range_overrides};
use crate::sim::components::DriveCoord;
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SIM_ONE;

const VERSES: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%";

/// A firer at (5,5) and a target at (11,5): 5.5 cells apart in leptons, so a
/// `Range=5` weapon reaches exactly 1536 only with one cell of ExtraRange.
fn scene(global: &str, weapon_keys: &str, firer_keys: &str) -> (Simulation, RuleSet, u64, u64) {
    scene_at(global, weapon_keys, firer_keys, 11)
}

fn scene_at(
    global: &str,
    weapon_keys: &str,
    firer_keys: &str,
    target_rx: u16,
) -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=6\nPrimary=GUN\nCanPassiveAquire=no\n{firer_keys}\
         [TARGET]\nStrength=1000\nArmor=heavy\nSpeed=6\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nDamage=5\nROF=100\nRange=5\nSpeed=100\n{weapon_keys}\
         Burst=2\nWarhead=HIT\nProjectile=INVIS\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\n{VERSES}\n\
         [General]\n{global}"
    )))
    .expect("extra range rules");
    let mut sim = Simulation::with_seed(3);
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(100);
    let firer = sim
        .spawn_object("FIRER", "Americans", 5, 5, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", target_rx, 5, 0, &rules)
        .expect("target");
    for id in [firer, target] {
        let entity = sim.entities_mut_for_test().get_mut(id).unwrap();
        entity.lifecycle.set_in_limbo_for_test(false);
    }
    (sim, rules, firer, target)
}

fn in_range(sim: &Simulation, rules: &RuleSet, firer: u64, target: u64) -> bool {
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let source = sim.entities().get(firer).unwrap();
    let z = i64::from(crate::sim::movement::ground_pose::object_world_z_leptons(
        source,
        Some(terrain),
    ));
    let coords = crate::sim::movement::ground_pose::object_get_coords(source, Some(terrain));
    compute_in_range(
        source,
        (i64::from(coords.x), i64::from(coords.y), z),
        &TargetKind::Entity(target),
        rules.weapon("GUN").unwrap(),
        rules,
        &sim.interner,
        sim,
        terrain,
        &Default::default(),
    )
}

fn make_moving(sim: &mut Simulation, id: u64) {
    let entity = sim.entities_mut_for_test().get_mut(id).unwrap();
    let here = crate::sim::movement::ground_pose::object_get_coords(entity, None);
    let head = DriveCoord {
        x: here.x.wrapping_add(256),
        y: here.y,
        z: here.z,
    };
    entity.foot_speed.set_speed_fraction(SIM_ONE);
    if !entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Drive
    }) {
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
    }
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_destination_for_test(Some(head))
                    .with_head_to_for_test(Some(head))
            ))
    );
}

/// The globals read through the native `[General]` keys with their
/// constructor defaults (`Ext/Rules/Body.cpp:511..515`, `Body.h:925..929`).
#[cfg(feature = "phobos")]
#[test]
fn the_extra_range_globals_read_with_native_defaults() {
    let rules = RuleSet::from_ini(&IniFile::from_str("[General]\n")).expect("rules");
    let globals = &rules.phobos_globals;
    assert_eq!(globals.extra_range_target_moving, 0);
    assert!(!globals.extra_range_target_moving_close_range_only);
    assert_eq!(globals.extra_range_firer_moving, 0);
    assert_eq!(globals.extra_range_prefiring, 0);
    assert!(globals.extra_range_prefiring_include_burst);

    // `Valueable<Leptons>`: the authored value is cells, parsed as cells × 256.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nExtraRange.TargetMoving=1\nExtraRange.TargetMoving.CloseRangeOnly=yes\n\
         ExtraRange.FirerMoving=2\nExtraRange.Prefiring=0.5\n\
         ExtraRange.Prefiring.IncludeBurst=no\n",
    ))
    .expect("rules");
    let globals = &rules.phobos_globals;
    assert_eq!(globals.extra_range_target_moving, 256);
    assert!(globals.extra_range_target_moving_close_range_only);
    assert_eq!(globals.extra_range_firer_moving, 512);
    assert_eq!(globals.extra_range_prefiring, 128);
    assert!(!globals.extra_range_prefiring_include_burst);
}

/// `IsMovingFire` (`:164..168`): the bonus applies only while the firer's
/// locomotor really moves.
#[cfg(feature = "phobos")]
#[test]
fn a_moving_firer_receives_the_firer_bonus() {
    let (mut sim, rules, firer, target) = scene("ExtraRange.FirerMoving=1\n", "", "");
    assert!(
        !in_range(&sim, &rules, firer, target),
        "5.5 cells vs 5 cells"
    );
    make_moving(&mut sim, firer);
    assert!(
        in_range(&sim, &rules, firer, target),
        "the moving bonus reaches 5.5 cells"
    );
}

/// `IsChasing` (`:157..162`): the bonus follows the target's locomotor state.
/// Phobos's per-weapon `ExtraRange_TargetMoving` is read from `[General]`
/// (`Ext/WeaponType/Body.cpp:200`), so an authored global makes `isset()`
/// true and the `CloseRangeOnly` arm never runs: a plain firer still takes
/// the bonus.
#[cfg(feature = "phobos")]
#[test]
fn a_moving_target_receives_the_target_bonus() {
    let (mut sim, rules, firer, target) = scene("ExtraRange.TargetMoving=1\n", "", "");
    make_moving(&mut sim, target);
    assert!(in_range(&sim, &rules, firer, target));
    let (mut sim, rules, firer, target) = scene(
        "ExtraRange.TargetMoving=1\nExtraRange.TargetMoving.CloseRangeOnly=yes\n",
        "",
        "",
    );
    make_moving(&mut sim, target);
    assert!(
        in_range(&sim, &rules, firer, target),
        "the [General]-read nullable bypasses CloseRangeOnly, as in Phobos"
    );
    let (mut sim, rules, firer, target) = scene(
        "ExtraRange.TargetMoving=1\nExtraRange.TargetMoving.CloseRangeOnly=yes\n",
        "",
        "CloseRange=yes\n",
    );
    make_moving(&mut sim, target);
    assert!(in_range(&sim, &rules, firer, target));
}

/// `IsPrefiring` (`:79..80`, `:152..155`): a nonzero burst index counts as
/// pre-firing while `ExtraRange.Prefiring.IncludeBurst` is on.
#[cfg(feature = "phobos")]
#[test]
fn the_prefiring_bonus_follows_the_burst_index() {
    let (mut sim, rules, firer, target) = scene("ExtraRange.Prefiring=1\n", "", "");
    assert!(!in_range(&sim, &rules, firer, target));
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .weapon_burst
        .advance();
    assert!(
        in_range(&sim, &rules, firer, target),
        "the second burst counts as pre-firing"
    );

    let (mut sim, rules, firer, target) = scene(
        "ExtraRange.Prefiring=1\nExtraRange.Prefiring.IncludeBurst=no\n",
        "",
        "",
    );
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .weapon_burst
        .advance();
    assert!(
        !in_range(&sim, &rules, firer, target),
        "IncludeBurst=no drops the burst arm"
    );
}

/// The additions sit before every other InRange term but after the -512
/// sentinel and the live KeepRange replacement (`:145..148`).
#[cfg(feature = "phobos")]
#[test]
fn the_sentinel_and_keep_range_bypass_the_bonuses() {
    let (mut sim, rules, firer, target) = scene_at(
        "ExtraRange.FirerMoving=1\n",
        "KeepRange=-5\nKeepRange.AllowAI=yes\n",
        "",
        10,
    );
    make_moving(&mut sim, firer);
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .rearm_timer
        .set_for_test(crate::sim::timer::CdTimer::started(100, 100));
    let weapon = rules.weapon("GUN").unwrap();
    let entity = sim.entities().get(firer).unwrap();
    assert!(
        keep_range_overrides(&sim, &rules, entity, weapon, Some(target))
            .1
            .is_some(),
        "the live KeepRange replaces the maximum"
    );
    assert!(
        !in_range(&sim, &rules, firer, target),
        "a live KeepRange skips the ExtraRange folds"
    );

    let (mut sim, rules, firer, target) = scene("ExtraRange.FirerMoving=1\n", "Range=-2\n", "");
    make_moving(&mut sim, firer);
    assert!(
        in_range(&sim, &rules, firer, target),
        "the -512 sentinel stays always-in-range"
    );
}

/// The predicate itself adds the three bonuses independently and sums them.
#[cfg(feature = "phobos")]
#[test]
fn the_predicate_sums_every_applicable_bonus() {
    let (mut sim, rules, firer, target) = scene(
        "ExtraRange.FirerMoving=1\nExtraRange.TargetMoving=0.5\nExtraRange.Prefiring=0.25\n",
        "",
        "",
    );
    make_moving(&mut sim, firer);
    make_moving(&mut sim, target);
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .weapon_burst
        .advance();
    let attacker = sim.entities().get(firer).unwrap();
    let victim = sim.entities().get(target).unwrap();
    let weapon = rules.weapon("GUN").unwrap();
    assert_eq!(
        extra_range_leptons(
            &sim,
            &rules,
            attacker,
            super::RangeTarget::Techno(victim),
            weapon
        ),
        256 + 128 + 64
    );
}

/// `IsChasing` starts with a Foot-class check on the firer (`:53..54`), so a
/// Building never takes the target-moving bonus even against a moving target.
#[cfg(feature = "phobos")]
#[test]
fn a_building_firer_takes_no_target_moving_bonus() {
    let (mut sim, rules, firer, target) = scene("ExtraRange.TargetMoving=1\n", "", "");
    make_moving(&mut sim, target);
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .set_category_for_test(crate::map::entities::EntityCategory::Structure);
    let attacker = sim.entities().get(firer).unwrap();
    let victim = sim.entities().get(target).unwrap();
    let weapon = rules.weapon("GUN").unwrap();
    assert_eq!(
        extra_range_leptons(
            &sim,
            &rules,
            attacker,
            super::RangeTarget::Techno(victim),
            weapon
        ),
        0
    );
}

/// A `Burst=0` weapon never divides by zero, and an index that is a multiple
/// of the burst does not count as pre-firing either (`:79..80`).
#[cfg(feature = "phobos")]
#[test]
fn the_burst_arm_needs_a_positive_burst_and_a_remainder() {
    let (mut sim, rules, firer, target) = scene("ExtraRange.Prefiring=1\n", "Burst=0\n", "");
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .weapon_burst
        .advance();
    assert!(!in_range(&sim, &rules, firer, target));

    let (mut sim, rules, firer, target) = scene("ExtraRange.Prefiring=1\n", "", "");
    {
        let burst = &mut sim
            .entities_mut_for_test()
            .get_mut(firer)
            .unwrap()
            .weapon_burst;
        burst.advance();
        burst.advance();
    }
    assert!(
        !in_range(&sim, &rules, firer, target),
        "index 2 % Burst 2 == 0 is not pre-firing"
    );
}

/// Phobos hooks `InRange` itself, so a Cell (attack-ground) target also takes
/// the movement bonuses; the terrain gate must thread the world handle.
#[cfg(feature = "phobos")]
#[test]
fn a_cell_target_takes_the_moving_firer_bonus() {
    let (mut sim, rules, firer, _) = scene("ExtraRange.FirerMoving=1\n", "", "");
    make_moving(&mut sim, firer);
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let cell = terrain.native_cell_identity((11, 5));
    let los = crate::sim::combat::line_of_fire::LineOfFireInputs::terrain_only();
    let attacker = sim.entities().get(firer).unwrap();
    let weapon = rules.weapon("GUN").unwrap();
    let run = |world: Option<&Simulation>| {
        crate::sim::combat::in_range::cell_target_in_range(
            attacker,
            cell,
            weapon,
            &rules,
            &sim.interner,
            terrain,
            &los,
            Some(&sim.substrate.entities),
            world,
        )
    };
    assert_eq!(
        run(None),
        Some(false),
        "without the world there are no bonuses"
    );
    assert_eq!(
        run(Some(&sim)),
        Some(true),
        "the moving bonus reaches the cell"
    );
}
