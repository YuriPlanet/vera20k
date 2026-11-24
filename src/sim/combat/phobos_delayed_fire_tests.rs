//! Phobos `DelayedFire.*` (`src/Ext/WeaponType/Body.cpp:189-199`) through the
//! production `FireVisit::UnitTarget` path: the `0x6FDDC0` gate inside
//! `TechnoClass::Fire_At` (`emit_admitted_fire`), the `0x736F61` / `0x5209AF`
//! class-site step that runs `HandleDelayedFireWithPauseSequence` before their
//! frame test, the saved timer on `GameEntity`, the `0x736480` firing-frame
//! freeze and the `0x6FABC4` Stage skip, and `TechnoClass_SetTarget_After`-style
//! anim removal.
use super::world_fire;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::movement::FacingClass;
use crate::sim::world::{FrameEffects, Simulation};
use std::collections::BTreeSet;

/// A `FIRER` vehicle with the `GUN` weapon (`Burst=1`) aimed east at a rival
/// `DUMMY` one cell out. `weapon_extra` appends `DelayedFire.*` keys. A
/// constructor-only `DFANIM` AnimType is bound so the wind-up anim can spawn;
/// no retail INI/ART is involved.
fn arena(weapon_extra: &str) -> (Simulation, RuleSet, u64, u64) {
    arena_with_firer(weapon_extra, "")
}

/// [`arena`] with extra `[FIRER]` keys, for the type-level pause gate.
fn arena_with_firer(weapon_extra: &str, firer_extra: &str) -> (Simulation, RuleSet, u64, u64) {
    arena_with_art(weapon_extra, firer_extra, None)
}

/// [`arena_with_firer`] with a fixed ART record, for the keys the Unit ART
/// reader owns (`FiringFrames=`, `FireUp=`): the `0x736F61` gate only engages
/// when the processed pass sees them.
fn arena_with_art(
    weapon_extra: &str,
    firer_extra: &str,
    art: Option<&str>,
) -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Warheads]\n0=HIT\n[BuildingTypes]\n0=DUMMY\n\
         [VehicleTypes]\n0=FIRER\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nOmniFire=yes\nCanPassiveAquire=no\n{firer_extra}\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n{weapon_extra}\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Animations]\n0=DFANIM\n"
    ));
    let mut rules = match art {
        Some(art) => {
            let art = crate::rules::ini_parser::IniFile::from_str(art);
            let mut rules =
                RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("rules read");
            rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
            rules
        }
        None => {
            let mut rules = RuleSet::from_ini(&ini).expect("rules read");
            // The app loader's AnimType read receipt: constructor-only AnimList
            // types still create AnimClass objects
            // (`combat_tests::initialize_fixture_anim_types`).
            rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
            rules
        }
    };
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
            &mut FrameEffects::empty()
        )
        .is_ok(),
        "the firer holds the target as a mission would"
    );
    (sim, rules, firer, target)
}

/// One production `FireVisit::UnitTarget`, and every projectile it launched.
fn visit(sim: &mut Simulation, rules: &RuleSet, firer: u64) -> usize {
    let before: BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        world_fire::FireVisit::UnitTarget(firer),
        rules,
        None,
        &mut FrameEffects::empty(),
    );
    sim.projectiles
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .count()
}

fn anim_ids(sim: &Simulation) -> BTreeSet<u64> {
    sim.substrate.anims.iter().map(|(id, _)| *id).collect()
}

fn start_frame(sim: &Simulation, firer: u64) -> i32 {
    sim.entities()
        .get(firer)
        .expect("firer")
        .phobos_delayed_fire_start_frame()
}

/// `timer.Start` at frame 100 suppresses the shot and spawns the attached
/// wind-up anim; `PauseFiringSequence` freezes `UnitClass+6C0`; frames 101-103
/// stay suppressed; the `Completed()` frame (104) fires one shot and
/// `ResetDelayedFireTimer` force-destroys the anim (`RemoveAnimOnNoDelay`).
#[test]
fn a_delay_holds_the_shot_freezes_the_frame_then_fires_and_removes_the_anim() {
    let (mut sim, rules, firer, _) = arena(
        "DelayedFire.Duration=4\nDelayedFire.Animation=DFANIM\n\
         DelayedFire.RemoveAnimOnNoDelay=yes\nDelayedFire.PauseFiringSequence=yes\n",
    );
    // Seed a live firing-sequence counter so the `0x736480` freeze is observable.
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .unit_firing_counter
        .set_for_test(5);
    let before = anim_ids(&sim);

    assert_eq!(
        visit(&mut sim, &rules, firer),
        0,
        "frame 100 starts the timer"
    );
    let spawned: Vec<u64> = anim_ids(&sim).difference(&before).copied().collect();
    assert_eq!(spawned.len(), 1, "one attached wind-up anim is created");
    assert_eq!(start_frame(&sim, firer), 100, "the timer is running");
    assert!(
        sim.entities()
            .get(firer)
            .unwrap()
            .phobos_delayed_fire_sequence_paused(),
        "the firing sequence is paused during the delay"
    );
    crate::sim::animation::tick_unit_firing_counter(
        sim.entities_mut_for_test().get_mut(firer).unwrap(),
    );
    assert_eq!(
        sim.entities().get(firer).unwrap().unit_firing_counter.get(),
        5,
        "`UnitClass_AI @ 0x736480` holds the firing frame"
    );

    // `InProgress()` frames keep suppressing, with no extra anim.
    sim.session.set_binary_frame_for_test(102);
    assert_eq!(visit(&mut sim, &rules, firer), 0, "frame 102 is still held");
    assert_eq!(anim_ids(&sim).difference(&before).count(), 1);

    // `Completed()` → reset and fire; the tracked anim is force-destroyed.
    sim.session.set_binary_frame_for_test(104);
    assert_eq!(visit(&mut sim, &rules, firer), 1, "frame 104 discharges");
    assert_eq!(start_frame(&sim, firer), -1, "the timer was reset");
    assert!(
        !sim.entities()
            .get(firer)
            .unwrap()
            .phobos_delayed_fire_sequence_paused(),
        "the frame freeze is released"
    );
    assert!(
        sim.substrate.pending_delete.contains_object(&spawned[0]),
        "`RemoveAnimOnNoDelay` force-destroys the attached anim"
    );
    crate::sim::animation::tick_unit_firing_counter(
        sim.entities_mut_for_test().get_mut(firer).unwrap(),
    );
    assert_eq!(
        sim.entities().get(firer).unwrap().unit_firing_counter.get(),
        4,
        "the firing counter advances again once unpaused"
    );
}

/// `Attach=false` means `CurrentDelayedFireAnim` is never registered, so
/// `RemoveAnimOnNoDelay` is completely inert (`Body.cpp:691-695`): the anim is
/// spawned unowned and survives the timer reset.
#[test]
fn an_unattached_anim_ignores_remove_anim_on_no_delay() {
    let (mut sim, rules, firer, _) = arena(
        "DelayedFire.Duration=4\nDelayedFire.Animation=DFANIM\n\
         DelayedFire.AnimIsAttached=no\nDelayedFire.RemoveAnimOnNoDelay=yes\n",
    );
    let before = anim_ids(&sim);
    assert_eq!(visit(&mut sim, &rules, firer), 0, "frame 100 is held");
    let spawned: Vec<u64> = anim_ids(&sim).difference(&before).copied().collect();
    assert_eq!(spawned.len(), 1, "the unattached wind-up anim is created");

    sim.session.set_binary_frame_for_test(104);
    assert_eq!(visit(&mut sim, &rules, firer), 1, "frame 104 discharges");
    assert!(
        !sim.substrate.pending_delete.contains_object(&spawned[0]),
        "an unattached anim is not tracked and not force-destroyed"
    );
    assert!(
        anim_ids(&sim).contains(&spawned[0]),
        "the unattached anim keeps its natural lifetime"
    );
}

/// `DelayedFire.SkipInTransport` (`Body.cpp:190`) makes `0x6FDDC0` skip the
/// gate for an open-topped passenger. The only change between the two cases is
/// the key: without it the same rider is delayed and spawns the wind-up anim.
#[test]
fn skip_in_transport_bypasses_the_delay_only_for_an_open_topped_rider() {
    for (extra, fire_now, anim_now) in [
        (
            "DelayedFire.Duration=4\nDelayedFire.Animation=DFANIM\nDelayedFire.SkipInTransport=yes\n",
            1,
            false,
        ),
        (
            "DelayedFire.Duration=4\nDelayedFire.Animation=DFANIM\n",
            0,
            true,
        ),
    ] {
        let (mut sim, rules, firer, _) = arena(extra);
        // `Role::Inside { open_topped: true }`: the `FireVisit::UnitTarget`
        // gate admits exactly this rider to fire (`world_fire.rs:2973-2974`).
        sim.entities_mut_for_test()
            .get_mut(firer)
            .unwrap()
            .passenger_role
            .set_inside_transport(0xDEAD_BEEF, true);
        let before = anim_ids(&sim);
        assert_eq!(
            visit(&mut sim, &rules, firer),
            fire_now,
            "extra={extra:?}: SkipInTransport controls the first frame"
        );
        assert_eq!(
            anim_ids(&sim).difference(&before).count() == 1,
            anim_now,
            "extra={extra:?}: the wind-up anim appears only when the delay runs"
        );
        assert_eq!(
            start_frame(&sim, firer) != -1,
            fire_now == 0,
            "extra={extra:?}: the timer starts only when the delay runs"
        );
    }
}

/// A class fire routine refuses the shot before `TechnoClass::Fire_At`
/// reaches `0x6FDDC0` while the weapon is still rearming, so no timer starts
/// and no wind-up anim appears; the first admitted shot then starts the
/// delay from its own frame.
#[test]
fn a_refused_shot_never_starts_the_delay() {
    let (mut sim, rules, firer, _) =
        arena("DelayedFire.Duration=4\nDelayedFire.Animation=DFANIM\n");
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .seldom.rearm_timer
        .start(100, 10);
    let before = anim_ids(&sim);
    assert_eq!(visit(&mut sim, &rules, firer), 0);
    assert_eq!(
        start_frame(&sim, firer),
        -1,
        "a rearming shot starts no timer"
    );
    assert_eq!(
        anim_ids(&sim).difference(&before).count(),
        0,
        "no wind-up anim before Fire_At is entered"
    );
    sim.session.set_binary_frame_for_test(105);
    assert_eq!(visit(&mut sim, &rules, firer), 0);
    assert_eq!(start_frame(&sim, firer), -1, "still rearming at frame 105");
    sim.session.set_binary_frame_for_test(111);
    assert_eq!(visit(&mut sim, &rules, firer), 0);
    assert_eq!(
        start_frame(&sim, firer),
        111,
        "the delay starts on the first admitted shot"
    );
    assert_eq!(anim_ids(&sim).difference(&before).count(), 1);
}

/// `Hooks.Firing.cpp:658-662` routes `PauseFiringSequence` only through
/// Infantry and turretless non-Voxel SHP Units. A turreted or Voxel Unit still
/// gets its shot delayed, but `0x736480` / `0x6FABC4` must not freeze its
/// counter or Stage.
#[test]
fn a_turreted_or_voxel_unit_delays_without_freezing_its_counter() {
    for (firer_extra, art) in [("Turret=yes\n", ""), ("", "[FIRER]\nVoxel=yes\n")] {
        let (mut sim, mut rules, firer, _) = arena_with_firer(
            "DelayedFire.Duration=4\nDelayedFire.Animation=DFANIM\n\
             DelayedFire.PauseFiringSequence=yes\n",
            firer_extra,
        );
        if !art.is_empty() {
            rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
                &crate::rules::ini_parser::IniFile::from_str(art),
            ));
        }
        sim.entities_mut_for_test()
            .get_mut(firer)
            .unwrap()
            .unit_firing_counter
            .set_for_test(5);
        let before = anim_ids(&sim);
        assert_eq!(visit(&mut sim, &rules, firer), 0, "the shot is delayed");
        assert_eq!(start_frame(&sim, firer), 100, "the timer starts");
        assert_eq!(
            anim_ids(&sim).difference(&before).count(),
            1,
            "the wind-up anim still spawns"
        );
        assert!(
            !sim.entities()
                .get(firer)
                .unwrap()
                .phobos_delayed_fire_sequence_paused(),
            "the pause hooks do not apply to this type"
        );
        crate::sim::animation::tick_unit_firing_counter(
            sim.entities_mut_for_test().get_mut(firer).unwrap(),
        );
        assert_eq!(
            sim.entities().get(firer).unwrap().unit_firing_counter.get(),
            4,
            "the stock counter cadence continues"
        );
    }
}

/// Phobos `TechnoClass_DetachAnim @ 0x710415` (`Body.cpp:1256-1267`) clears
/// `CurrentDelayedFireAnim` when the wind-up anim is detached or expires; the
/// pointer is non-owning, so the timer's later reset must not destroy it a
/// second time.
#[test]
fn an_expired_wind_up_anim_clears_the_tracked_pointer() {
    let (mut sim, rules, firer, _) = arena(
        "DelayedFire.Duration=4\nDelayedFire.Animation=DFANIM\n\
         DelayedFire.AnimIsAttached=yes\nDelayedFire.RemoveAnimOnNoDelay=yes\n",
    );
    let before = anim_ids(&sim);
    assert_eq!(
        visit(&mut sim, &rules, firer),
        0,
        "frame 100 starts the timer"
    );
    let spawned: Vec<u64> = anim_ids(&sim).difference(&before).copied().collect();
    assert_eq!(spawned.len(), 1);
    assert_eq!(
        sim.entities()
            .get(firer)
            .unwrap()
            .phobos_delayed_fire_anim(),
        Some(spawned[0]),
        "the attached anim is tracked"
    );
    // Natural expiry/detach runs the shared destroy owner, as today.
    sim.destroy_anim(spawned[0], &rules);
    assert_eq!(
        sim.entities()
            .get(firer)
            .unwrap()
            .phobos_delayed_fire_anim(),
        None,
        "0x710415 clears the tracked pointer without destroying again"
    );
    sim.session.set_binary_frame_for_test(104);
    assert_eq!(
        visit(&mut sim, &rules, firer),
        1,
        "the timer still discharges"
    );
    assert_eq!(start_frame(&sim, firer), -1, "reset clears the timer");
    assert!(sim.substrate.pending_delete.contains_object(&spawned[0]));
}

/// `UnitClass_UpdateFiring_FireUp @ 0x736F61` calls
/// `HandleDelayedFireWithPauseSequence` with this frame's `frame` and
/// `firingFrame` **before** its `frame != firingFrame` test
/// (`Ext/Unit/Hooks.Firing.cpp:89-95`; the shared body is
/// `Ext/Techno/Body.cpp:699-750`). With `FiringFrames=3` the seeded counter
/// decodes to sequence frame 0, one short of `FireUp=1`, so:
/// - the first held update already starts the timer (native's `!HasStarted()`
///   arm at `Body.cpp:718-736`), and
/// - the completed arm resets it before that frame's test can skip the shot
///   (`Body.cpp:742-743` runs ahead of `Hooks.Firing.cpp:92-95`).
#[test]
fn delayed_fire_pause_resets_before_the_frame_test() {
    let (mut sim, rules, firer, _) = arena_with_art(
        "DelayedFire.Duration=4\nDelayedFire.PauseFiringSequence=yes\n",
        "",
        Some("[FIRER]\nFiringFrames=3\nFireUp=1\n"),
    );
    // `2 * 3 - 1 = 5` is the seed a first update takes: `(5 - 5) / 2` is frame 0.
    sim.entities_mut_for_test()
        .get_mut(firer)
        .unwrap()
        .unit_firing_counter
        .set_for_test(5);

    assert_eq!(
        visit(&mut sim, &rules, firer),
        0,
        "the held frame launches nothing"
    );
    assert_eq!(
        start_frame(&sim, firer),
        100,
        "the delayed step ran before the frame test and started the timer"
    );

    sim.session.set_binary_frame_for_test(104);
    assert_eq!(
        visit(&mut sim, &rules, firer),
        0,
        "the sequence still stands one frame short of `FireUp=1`"
    );
    assert_eq!(
        start_frame(&sim, firer),
        -1,
        "the completed arm reset the timer before the frame test"
    );
}
