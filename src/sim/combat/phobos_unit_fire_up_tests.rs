//! Phobos `UnitTypeExt::FireUp=` (`Ext/UnitType/Body.cpp:93`) through the
//! production `FireVisit::UnitTarget` path: `UnitClass_UpdateFiring_FireUp`
//! (`0x736F61`, `Ext/Unit/Hooks.Firing.cpp:7-102`) holds an SHP, non-turreted
//! Unit on the authored body firing frame, drops the sequence of a really
//! moving unit that does not have `OpportunityFire=`, and forces the full
//! rearm when `Burst.FireWithinSequence` projects past the sequence.
use super::world_fire;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::DriveCoord;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::{DriveLocomotionRuntime, SpeedRules, motion_query};
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::fixed_math::SIM_ONE;

const EAST: u16 = 0x4000;
/// `2 * 5 - 1`: a five-image ART firing sequence counts down from nine.
const FRAMES: i32 = 9;

/// An SHP, non-turreted `FIRER` unit aimed east at a rival `DUMMY` two cells
/// east, with ART `FiringFrames=5` and ART `FireUp=fire_up` (`-1` is the key's
/// constructor default). `firer_extra` appends `[FIRER]` keys, `weapon_extra`
/// appends `[GUN]` keys. `ROF.RandomDelay=0` keeps the rearm draw-free.
fn arena(firer_extra: &str, weapon_extra: &str, fire_up: i32) -> (Simulation, RuleSet, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Warheads]\n0=HIT\n[BuildingTypes]\n0=DUMMY\n\
         [VehicleTypes]\n0=FIRER\n\
         [FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nOmniFire=yes\n\
         CanPassiveAquire=no\n{firer_extra}\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=HIT\n\
         {weapon_extra}\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nROF.RandomDelay=0\n"
    ));
    // The Unit ART reader owns `FiringFrames`, so the processed pass has to see
    // the fixed art; `install_art_data` then projects `FireUp=` and `Voxel=`.
    let art = IniFile::from_str(&format!("[FIRER]\nFiringFrames=5\nFireUp={fire_up}\n"));
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
    let actor = sim.entities_mut_for_test().get_mut(firer).expect("firer");
    actor.body_facing.snap(EAST, 100);
    assert!(
        sim.assign_target_represented(
            firer,
            Some(TargetKind::Entity(target)),
            &rules,
            None,
            FrameEffects::empty(),
        )
        .is_ok(),
        "the firer holds the target as a mission would"
    );
    (sim, rules, firer)
}

/// Install a really moving Drive locomotor: a destination and a head point one
/// cell east of the parked body, with the Foot speed fraction a Move order
/// leaves. `Is_Really_Moving_Now` (ILocomotion+0xA8) answers through the Drive
/// readiness inputs (`ready_producer`), so the speed term needs a positive
/// owner speed.
fn start_moving(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let head = DriveCoord {
        x: 11 * 256 + 128,
        y: 10 * 256 + 128,
        z: 0,
    };
    let mut locomotor =
        LocomotorState::for_test_kind(crate::rules::locomotor_type::LocomotorKind::Drive);
    assert!(
        locomotor.install_drive_state_for_test(Some(
            DriveLocomotionRuntime::default()
                .with_destination_for_test(Some(head))
                .with_head_to_for_test(Some(head))
        )),
        "the fixture's locomotor has a Drive slot"
    );
    let actor = sim.entities_mut_for_test().get_mut(id).expect("firer");
    actor.foot_speed.set_speed_fraction(SIM_ONE);
    actor.locomotor = Some(locomotor);
    let entity = sim.entities().get(id).expect("firer");
    let speed = SpeedRules::new(rules, &sim.interner, &sim.houses);
    assert!(
        motion_query::is_really_moving_now(entity, speed, sim.session.binary_frame()),
        "the fixture reaches the native moving query"
    );
}

/// One production `FireVisit::UnitTarget`, and every projectile it launched.
fn visit(sim: &mut Simulation, rules: &RuleSet, firer: u64) -> usize {
    let before: std::collections::BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        world_fire::FireVisit::UnitTarget(firer),
        rules,
        None,
        FrameEffects::empty(),
    );
    sim.projectiles
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .count()
}

fn counter(sim: &Simulation, firer: u64) -> i32 {
    sim.entities()
        .get(firer)
        .expect("firer")
        .unit_firing_counter
        .get()
}

/// Install the retained `CurrentFiringFrame` prior (`UnitClass+6C0`).
fn set_counter(sim: &mut Simulation, firer: u64, value: i32) {
    sim.entities_mut_for_test()
        .get_mut(firer)
        .expect("firer")
        .unit_firing_counter
        .set_for_test(value);
}

fn rearm(sim: &Simulation, firer: u64) -> i32 {
    sim.entities()
        .get(firer)
        .expect("firer")
        .rearm_timer
        .duration()
}

/// A `-1` counter is first seeded to `2 * FiringFrames - 1` — native's
/// `CurrentFiringFrame == -1` arm in the hook — so the first update stands on
/// frame 0 and holds; only the counter that decodes to `FireUp` launches.
#[test]
fn fire_up_holds_an_shp_unit_until_the_authored_frame() {
    let (mut sim, rules, firer) = arena("", "", 2);

    assert_eq!(visit(&mut sim, &rules, firer), 0, "frame 0 holds the shot");
    assert_eq!(
        counter(&sim, firer),
        FRAMES,
        "the hook seeded the sequence at frame 0"
    );

    set_counter(&mut sim, firer, 7);
    assert_eq!(
        visit(&mut sim, &rules, firer),
        0,
        "`(9 - 7) / 2` is frame 1, not `FireUp=2`"
    );
    assert_eq!(counter(&sim, firer), 7, "a held frame keeps its counter");

    set_counter(&mut sim, firer, 5);
    assert_eq!(
        visit(&mut sim, &rules, firer),
        1,
        "`(9 - 5) / 2` is the authored frame 2"
    );
}

/// A really moving unit without `OpportunityFire=` drops the sequence and
/// fires nothing; with it, the sequence stands and the authored frame launches
/// while the hull still moves.
#[test]
fn moving_clears_the_fire_up_sequence_unless_opportunity_fire() {
    let (mut sim, rules, firer) = arena("OpportunityFire=no\n", "", 2);
    set_counter(&mut sim, firer, 5);
    start_moving(&mut sim, &rules, firer);
    assert_eq!(visit(&mut sim, &rules, firer), 0, "moving holds the shot");
    assert_eq!(counter(&sim, firer), -1, "the sequence is cleared");

    let (mut sim, rules, firer) = arena("OpportunityFire=yes\n", "", 2);
    set_counter(&mut sim, firer, 5);
    start_moving(&mut sim, &rules, firer);
    assert_eq!(
        visit(&mut sim, &rules, firer),
        1,
        "`OpportunityFire` keeps the authored frame while moving"
    );
}

/// `Burst.Delays=4,3` at burst index 1 gives cumulative 4 and projected 8:
/// native `GetBurstDelay(0)` is 0 by definition, and index 1 reads the first
/// authored delay 4. A counter that decodes to `FireUp(3) + 4 = 7` accepts the
/// shot, and `3 + 8 = 11` passes the nine-frame sequence, so the shot takes the
/// full rearm instead of the index's delay.
///
/// The counter is a fixture prior below the sequence floor: a live counter
/// stops at -1 and every frame it decodes is at most half the sequence, which
/// keeps `fireUp + projected` inside `frames`. The arm is ported because it
/// exists in native; it is dormant for live counters.
#[test]
fn burst_overrun_forces_a_full_rearm() {
    let (mut sim, rules, firer) = arena(
        "",
        "Burst=2\nBurst.Delays=4,3\nBurst.FireWithinSequence=yes\n",
        3,
    );
    sim.entities_mut_for_test()
        .get_mut(firer)
        .expect("firer")
        .weapon_burst
        .set_index(1);
    set_counter(&mut sim, firer, -5);

    assert_eq!(
        visit(&mut sim, &rules, firer),
        1,
        "`(9 + 5) / 2` is frame 7, `FireUp(3) + cumulative(4)`"
    );
    assert_eq!(
        rearm(&sim, firer),
        40,
        "the overrun takes the full ROF, not the index's three-frame delay"
    );
}

/// The key's -1 constructor value keeps the stock unit cadence: the first
/// update launches on frame 0 without a sequence gate.
#[test]
fn the_key_off_keeps_the_stock_unit_cadence() {
    let (mut sim, rules, firer) = arena("", "", -1);
    assert_eq!(
        visit(&mut sim, &rules, firer),
        1,
        "no `FireUp=` holds nothing"
    );
}
