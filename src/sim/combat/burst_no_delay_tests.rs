//! Phobos `Burst.NoDelay` (`Ext/WeaponType/Body.cpp:122`) at the two sites its
//! hooks patch: `UnitClass_UpdateFiring_BurstNoDelay @ 0x736F67` and
//! `InfantryClass_UpdateFiring_BurstNoDelay @ 0x5209EE`, both inside the class
//! firing update. The loop repeats `TechnoClass::Fire` until the burst wraps,
//! zeroing `RearmTimer` between shots so `GetFireError`'s T45 admits the next,
//! then restores the last shot's own rearm value.
use super::world_fire;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::movement::FacingClass;
use crate::sim::world::{FrameEffects, Simulation};

/// A firer of `category` with a three-shot weapon, aimed east at a rival-house
/// target one cell out. `type_extra` adds keys to the firer's own type.
fn arena(category: &str, weapon: &str, type_extra: &str) -> (Simulation, RuleSet, u64, u64) {
    let (registry, firer) = match category {
        "VehicleTypes" => (
            "[VehicleTypes]\n0=FIRER\n",
            "[FIRER]\nStrength=1000\nArmor=heavy\nSpeed=4\nPrimary=GUN\nOmniFire=yes\n\
             CanPassiveAquire=no\n",
        ),
        _ => (
            "[InfantryTypes]\n0=FIRER\n",
            "[FIRER]\nStrength=1000\nArmor=light\nPrimary=GUN\nCanPassiveAquire=no\n",
        ),
    };
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "{registry}[Warheads]\n0=HIT\n[BuildingTypes]\n0=DUMMY\n{firer}{type_extra}\
         [DUMMY]\nStrength=10000\nArmor=heavy\nCanPassiveAquire=no\n\
         [WeaponTypes]\n0=GUN\n[GUN]\nDamage=10\nROF=40\nRange=9.5\nSpeed=100\n\
         Projectile=INVIS\nWarhead=HIT\nBurst=3\n{weapon}\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
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
    // Due east, so the facing the fixture snaps to is also the firing solution.
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
            None,
            FrameEffects::empty(),
        )
        .is_ok(),
        "the firer holds the target as a mission would"
    );
    (sim, rules, firer, target)
}

/// One class firing-update visit, and every projectile it launched.
fn visit(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64, category: &str) -> usize {
    let before: std::collections::BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        match category {
            "VehicleTypes" => world_fire::FireVisit::UnitTarget(firer),
            _ => world_fire::FireVisit::InfantryTarget(firer),
        },
        rules,
        None,
        FrameEffects::empty(),
    );
    sim.projectiles
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .count()
}

/// With the key, one update launches all three shots, the burst wraps back to 0
/// and `RearmTimer` ends on the last shot's full-ROF value rather than the
/// mid-burst gap.
#[test]
fn the_key_empties_the_whole_burst_in_one_frame() {
    for category in ["VehicleTypes", "InfantryTypes"] {
        let (mut sim, rules, firer, target) = arena(category, "Burst.NoDelay=yes\n", "");
        assert_eq!(
            visit(&mut sim, &rules, firer, target, category),
            3,
            "{category}: `Burst=3` with `Burst.NoDelay=yes` fires every shot now"
        );
        let entity = sim.entities().get(firer).expect("firer");
        assert_eq!(
            entity.weapon_burst.index(),
            0,
            "{category}: the burst wrapped, so the next frame starts a new one"
        );
        assert!(
            entity.rearm_timer.duration() >= 40,
            "{category}: the restored rearm is the full ROF, not a burst gap"
        );
    }
}

/// Without the key nothing changes: one shot per update, the burst steps by one
/// and the mid-burst gap stands.
#[test]
fn an_ordinary_burst_still_fires_one_shot_per_frame() {
    for category in ["VehicleTypes", "InfantryTypes"] {
        let (mut sim, rules, firer, target) = arena(category, "", "");
        assert_eq!(
            visit(&mut sim, &rules, firer, target, category),
            1,
            "{category}: the control still fires once"
        );
        let entity = sim.entities().get(firer).expect("firer");
        assert_eq!(entity.weapon_burst.index(), 1);
        assert!(
            entity.rearm_timer.duration() <= 5,
            "{category}: the mid-burst gap stands, not the full ROF"
        );
    }
}

/// The key belongs to the two classes that have the hook: a mission's own FireAt
/// call is one shot, wherever the key stands.
#[test]
fn a_raw_fireat_call_still_launches_one_shot() {
    let (mut sim, rules, firer, target) = arena("VehicleTypes", "Burst.NoDelay=yes\n", "");
    let before: std::collections::BTreeSet<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
    sim.commit_fire_visit(
        world_fire::FireVisit::Direct {
            id: firer,
            target: TargetKind::Entity(target),
            weapon_index: 0,
        },
        &rules,
        None,
        FrameEffects::empty(),
    );
    assert_eq!(
        sim.projectiles
            .iter()
            .filter(|(id, _)| !before.contains(id))
            .count(),
        1,
        "`FireVisit::Direct` is a mission's FireAt, which native leaves alone"
    );
}

/// The run also ends on a *refused* attempt, which is what the write-back after
/// the loop exists for: the first shot debits the firer's only round
/// (`DecreaseAmmo70D670` inside `emit_admitted_fire`), `GetFireError`'s ammo arm
/// then refuses the repeat, and the timer the refused attempt had just zeroed has
/// to be restored from the last successful shot rather than left at zero.
#[test]
fn a_refused_repeat_restores_the_last_shots_rearm() {
    for category in ["VehicleTypes", "InfantryTypes"] {
        let (mut sim, rules, firer, target) = arena(category, "Burst.NoDelay=yes\n", "Ammo=1\n");
        assert_eq!(
            visit(&mut sim, &rules, firer, target, category),
            1,
            "{category}: the repeat is refused once the only round is spent"
        );
        let entity = sim.entities().get(firer).expect("firer");
        assert_eq!(
            entity.weapon_burst.index(),
            1,
            "{category}: the burst stopped mid-run instead of wrapping"
        );
        let duration = entity.rearm_timer.duration();
        assert!(
            (1..=5).contains(&duration),
            "{category}: the restore must write the first shot's mid-burst gap back, not the zero the refused attempt armed and not the full ROF"
        );
        assert_eq!(
            entity.ammo().map(|ammo| ammo.current()),
            Some(0),
            "{category}: the repeat was refused by the spent round, not by another gate"
        );
    }
}

/// The hooks gate the repeat on the delayed-fire keys as well:
/// `!Duration.isset() || OnlyOnInitialBurst`. A weapon that authors
/// `DelayedFire.Duration=` without the second key keeps native's one shot per
/// frame even with `Burst.NoDelay=yes`, and authoring both opens the repeat
/// again - so each key is tested against a case the other one fails.
#[test]
fn a_delayed_fire_weapon_keeps_its_burst_cadence() {
    for category in ["VehicleTypes", "InfantryTypes"] {
        let (mut sim, rules, firer, target) =
            arena(category, "Burst.NoDelay=yes\nDelayedFire.Duration=5\n", "");
        assert_eq!(
            visit(&mut sim, &rules, firer, target, category),
            1,
            "{category}: a set Duration with OnlyOnInitialBurst unset refuses the repeat"
        );
        assert_eq!(
            sim.entities()
                .get(firer)
                .expect("firer")
                .weapon_burst
                .index(),
            1,
            "{category}: the burst stepped once, as an ordinary burst does"
        );

        let (mut sim, rules, firer, target) = arena(
            category,
            "Burst.NoDelay=yes\nDelayedFire.Duration=5\nDelayedFire.OnlyOnInitialBurst=yes\n",
            "",
        );
        assert_eq!(
            visit(&mut sim, &rules, firer, target, category),
            3,
            "{category}: OnlyOnInitialBurst opens the gate again"
        );
    }
}
