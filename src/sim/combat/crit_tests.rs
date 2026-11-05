//! Phobos warhead critical hits through the production detonation path.

use super::world_receiver::{ReceiverRun, commit_projectiles};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

/// Two rival houses on the flat arena; `sections` appends further type bodies
/// (`[CRITWH]`, ...) and `warheads` the extra `[Warheads]` registry rows.
fn scene(
    warheads: &str,
    wh_ini: &str,
    target_ini: &str,
    sections: &str,
    target_house: &str,
) -> (Simulation, RuleSet, u64, u64) {
    scene_with_firer(warheads, wh_ini, target_ini, sections, "", target_house)
}

#[allow(clippy::too_many_arguments)]
fn scene_with_firer(
    warheads: &str,
    wh_ini: &str,
    target_ini: &str,
    sections: &str,
    firer_ini: &str,
    target_house: &str,
) -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n{warheads}\
         [WH]\nCellSpread=0\n{wh_ini}\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n{firer_ini}\
         [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n{target_ini}\n\
         {sections}"
    )))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", target_house, 22, 20, 0, &rules)
        .expect("target");
    (sim, rules, firer, target)
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.entities().get(id).expect("entity").health.current()
}

fn detonation(
    sim: &mut Simulation,
    warhead: &str,
    source: u64,
    target: u64,
    damage: i32,
) -> ProjectileDetonation {
    let coords = {
        let target = sim.entities().get(target).expect("target");
        object_get_coords(target, sim.resolved_terrain.as_ref())
    };
    ProjectileDetonation {
        projectile_id: target,
        source_id: source,
        target: ProjectileTarget::Entity(target),
        impact: ProjectileCoord::new(coords.x, coords.y, coords.z),
        payload: ProjectilePayload::new(damage, sim.interner.intern(warhead), None, false),
        reason: ProjectileDetonationReason::ReachedTarget,
    }
}

fn shoot(
    sim: &mut Simulation,
    rules: &RuleSet,
    warhead: &str,
    source: u64,
    target: u64,
    damage: i32,
) -> super::LogicProjectileCommit {
    let detonation = detonation(sim, warhead, source, target, damage);
    let mut run = ReceiverRun::default();
    let commit = commit_projectiles(sim, &mut run, &[detonation], rules, None);
    run.finish();
    commit
}

/// One `WarheadTypeExt::Detonate` per-target pass of `WH` at `target`'s cell,
/// with `intended` as the bullet target and its own cell-spread collection.
fn detonate_once(
    sim: &mut Simulation,
    rules: &RuleSet,
    firer: u64,
    target: u64,
    intended: Option<u64>,
) {
    let warhead = rules.warhead("WH").expect("warhead");
    let warhead_ref = sim.interner.intern("WH");
    let source_house = sim.entities().get(firer).map(|entity| entity.owner());
    let impact = detonation(sim, "WH", firer, target, 0).impact;
    let mut run = ReceiverRun::default();
    let mut effects = super::effects::DeathEffects::default();
    super::crit::detonate(
        sim,
        &mut run,
        rules,
        None,
        warhead,
        warhead_ref,
        firer,
        source_house,
        impact,
        intended,
        &mut effects,
    );
    run.finish();
}

/// The Scenario stream of a plain scene after `doubles` `RandomDouble` draws.
fn after_doubles(doubles: usize) -> crate::sim::rng::SimRngLogicalState {
    let (sim, _rules, _firer, _target) = scene("", "", "", "", "Russians");
    let mut expected = sim.scenario_rng.clone();
    for _ in 0..doubles {
        expected.random_double();
    }
    expected.logical_state()
}

/// `ApplyCrit`'s plain arm calls the target's `ReceiveDamage` with
/// `Crit.ExtraDamage` before the native area damage; the same rules without a
/// crit chance leave the identical packet untouched.
#[test]
fn crit_extra_damage_lands_through_the_bullet_detonation() {
    let (mut crit_sim, crit_rules, firer, target) = scene(
        "",
        "Crit.Chance=1.0\nCrit.ExtraDamage=50\n",
        "",
        "",
        "Russians",
    );
    let (mut plain_sim, plain_rules, plain_firer, plain_target) = scene("", "", "", "", "Russians");
    shoot(&mut crit_sim, &crit_rules, "WH", firer, target, 0);
    shoot(
        &mut plain_sim,
        &plain_rules,
        "WH",
        plain_firer,
        plain_target,
        0,
    );
    assert_eq!(
        health(&plain_sim, plain_target) - health(&crit_sim, target),
        50,
        "the crit adds exactly its ExtraDamage"
    );
}

/// `ApplyCrit`'s house, health-threshold and `ImmuneToCrit` gates each stop
/// the same always-crit warhead.
#[test]
fn crit_gates_follow_house_health_and_immunity() {
    let crit_damage =
        |wh_ini: &str, target_ini: &str, target_house: &str, set_health: Option<i32>| {
            let (mut sim, rules, firer, target) = scene("", wh_ini, target_ini, "", target_house);
            let (mut plain, plain_rules, plain_firer, plain_target) =
                scene("", "", target_ini, "", target_house);
            if let Some(value) = set_health {
                for (world, id) in [(&mut sim, target), (&mut plain, plain_target)] {
                    world
                        .substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .health
                        .set_for_test(value);
                }
            }
            shoot(&mut sim, &rules, "WH", firer, target, 0);
            shoot(&mut plain, &plain_rules, "WH", plain_firer, plain_target, 0);
            health(&plain, plain_target) - health(&sim, target)
        };

    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nCrit.AffectsHouse=enemies\n",
            "",
            "Americans",
            None,
        ),
        0,
        "an allied owner fails the enemies flag"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nCrit.AffectsAbovePercent=0.9\n",
            "",
            "Russians",
            Some(50),
        ),
        0,
        "10% health misses the (0.9, 1.0] interval"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\n",
            "ImmuneToCrit=yes\n",
            "Russians",
            None,
        ),
        0,
        "ImmuneToCrit refuses the crit"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nCrit.AffectsAbovePercent=0.9\n",
            "",
            "Russians",
            None,
        ),
        50,
        "full health satisfies the threshold"
    );
}

/// `Crit.Warhead` with `Crit.Warhead.FullDetonation` runs a real detonation of
/// the named warhead at the target, whose own `AnimList` shows in the tail.
#[test]
fn crit_warhead_detonates_at_the_target() {
    let sections = "[CRITWH]\nCellSpread=1\nAnimList=CRITANIM\n";
    let (mut sim, rules, firer, target) = scene(
        "1=CRITWH\n",
        "Crit.Chance=1.0\nCrit.ExtraDamage=30\nCrit.Warhead=CRITWH\n",
        "",
        sections,
        "Russians",
    );
    let (mut plain, plain_rules, plain_firer, plain_target) = scene(
        "1=CRITWH\n",
        "Crit.Warhead=CRITWH\n",
        "",
        sections,
        "Russians",
    );
    assert_eq!(
        rules.warhead("WH").unwrap().crit.warhead(),
        Some("CRITWH"),
        "the crit warhead key reads"
    );
    assert!(
        rules.warhead("CRITWH").is_some(),
        "the crit warhead resolves"
    );
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    shoot(&mut plain, &plain_rules, "WH", plain_firer, plain_target, 0);
    assert_eq!(
        health(&plain, plain_target) - health(&sim, target),
        30,
        "the crit detonated the crit warhead on the target"
    );
}

/// `SelectDamageAnimation_CritAnim`: an active crit replaces the detonation's
/// damage animation with the `Crit.AnimList` entry while
/// `Crit.AnimOnAffectedTargets` is off.
#[test]
fn a_crit_replaces_the_detonation_damage_animation() {
    let (mut sim, rules, _firer, _target) = scene(
        "",
        "Crit.Chance=1.0
Crit.AnimList=CRITICAL
AnimList=NORMAL
",
        "",
        "",
        "Russians",
    );
    let warhead = rules.warhead("WH").expect("warhead");
    let coordinate = ProjectileCoord::new(22 * 256 + 128, 20 * 256 + 128, 0);
    let selected = |sim: &mut Simulation, crit_active: bool| {
        super::detonation_anim::select(sim, &rules, warhead, 10, 0, coordinate, crit_active)
            .map(|id| sim.interner.resolve(id).to_owned())
    };
    assert_eq!(
        selected(&mut sim, false),
        Some("NORMAL".to_owned()),
        "no crit keeps the ordinary damage animation"
    );
    assert_eq!(
        selected(&mut sim, true),
        Some("CRITICAL".to_owned()),
        "Crit_Active swaps in the crit animation"
    );
}

/// `Detonate`'s prefix (`Detonate.cpp:162..170`): a warhead that can crit or
/// carries an AttachEffect list draws the crit buffer (unless
/// `Crit.ApplyChancePerTarget`) and the `ReturnWarhead` buffer, even with no
/// target; `Crit.Chance` alone is not `CanCrit` and draws nothing.
#[test]
fn detonate_prefix_draws_both_buffers_for_crit_or_attach_effect_warheads() {
    let draws = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene(
            "",
            wh_ini,
            "",
            "[AttachEffectTypes]\n0=MARK\n[MARK]\nDuration=100\n",
            "Russians",
        );
        detonate_once(&mut sim, &rules, firer, target, None);
        sim.scenario_rng.logical_state()
    };
    assert_eq!(
        draws("Crit.Chance=1.0\nCrit.AnimList=X\n"),
        after_doubles(2),
        "crit buffer, then the ReturnWarhead buffer"
    );
    assert_eq!(
        draws("Crit.Chance=1.0\nCrit.AnimList=X\nCrit.ApplyChancePerTarget=yes\n"),
        after_doubles(1),
        "per-target dice leave only the ReturnWarhead buffer"
    );
    assert_eq!(
        draws("AttachEffect.AttachTypes=MARK\n"),
        after_doubles(2),
        "an AttachEffect list is PossibleCellSpreadDetonate"
    );
    assert_eq!(
        draws("Crit.Chance=1.0\n"),
        after_doubles(0),
        "a chance without damage, warhead or animation cannot crit"
    );
}

/// `CanCrit` does not read `Crit.Chance`: a firer's AttachEffect
/// `Crit.ExtraChance` crits a warhead that leaves the chance unset.
#[test]
fn an_attach_effect_extra_chance_crits_a_zero_chance_warhead() {
    let (mut sim, rules, firer, target) = scene_with_firer(
        "",
        "Crit.ExtraDamage=50\n",
        "",
        "[AttachEffectTypes]\n0=CRIT\n[CRIT]\nDuration=100\nCrit.ExtraChance=1.0\n",
        "AttachEffect.AttachTypes=CRIT\n",
        "Russians",
    );
    sim.update_phobos_attach_effects(firer, &rules);
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(health(&sim, target), 450, "the folded chance crits");
}

/// `CanAffectTarget`: under `EffectsRequireVerses` (default) a target whose
/// armor the warhead's Verses zero skips the whole per-target pass, so not
/// even a crit warhead with its own Verses reaches it.
#[test]
fn zero_verses_targets_skip_the_per_target_pass() {
    let crit_damage = |extra: &str| {
        let (mut sim, rules, firer, target) = scene(
            "1=CRITWH\n",
            &format!(
                "Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\nCrit.Chance=1.0\n\
                 Crit.ExtraDamage=30\nCrit.Warhead=CRITWH\n{extra}"
            ),
            "",
            "[CRITWH]\nCellSpread=1\n",
            "Russians",
        );
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        500 - health(&sim, target)
    };
    assert_eq!(crit_damage(""), 0, "0% Verses refuses the target");
    assert_eq!(
        crit_damage("EffectsRequireVerses=no\n"),
        30,
        "without the key the crit warhead lands"
    );
}

/// `BulletClass_Detonate @ 0x46920B` hooks the routine's entry, so a warhead
/// that claims the impact with a special arm (`IvanBomb=yes`) still runs the
/// per-target AttachEffect pass before its arm body, for both the intended
/// target and a cell-spread collection.
#[test]
fn special_arm_warheads_still_run_the_per_target_pass() {
    for cell_spread in ["CellSpread=0", "CellSpread=1"] {
        let (mut sim, rules, firer, target) = scene(
            "",
            &format!("IvanBomb=yes\n{cell_spread}\nAttachEffect.AttachTypes=MARK\n"),
            "",
            "[AttachEffectTypes]\n0=MARK\n[MARK]\nDuration=100\n",
            "Russians",
        );
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        let attached = sim
            .entities()
            .get(target)
            .expect("target")
            .phobos_attach_effects()
            .instances()
            .len();
        assert_eq!(
            attached, 1,
            "the special arm still visited the target ({cell_spread})"
        );
    }
}

/// `MapClass_DamageArea @ 0x489286`: a direct area damage runs the same
/// per-target pass, so a spreading warhead's AttachEffect list reaches its
/// receivers.
#[test]
fn direct_area_damage_applies_the_warhead_attach_effects() {
    let (mut sim, rules, firer, target) = scene(
        "1=AEWH\n",
        "",
        "",
        "[AEWH]\nCellSpread=1\nAttachEffect.AttachTypes=MARK\n\
         [AttachEffectTypes]\n0=MARK\n[MARK]\nDuration=100\n",
        "Russians",
    );
    let warhead = rules.warhead("AEWH").expect("warhead");
    let warhead_ref = sim.interner.intern("AEWH");
    let impact = detonation(&mut sim, "AEWH", firer, target, 0).impact;
    super::world_receiver::apply_area_damage(
        &mut sim,
        &rules,
        None,
        impact,
        10,
        warhead,
        false,
        (firer, None, warhead_ref),
        crate::sim::world::FrameEffects::empty(),
    );
    let attached = sim
        .entities()
        .get(target)
        .expect("target")
        .phobos_attach_effects()
        .instances()
        .len();
    assert_eq!(attached, 1, "the area's receiver got the effect");
}

/// `SelectDamageAnimation_CritAnim` (`0x48A5B3`) runs before the `AnimList`
/// empty test, so a warhead with only `Crit.AnimList` still shows it; the
/// `0x48A4F3` hook selects a negative packet by its magnitude.
#[test]
fn crit_anim_list_shows_without_an_anim_list() {
    let (mut sim, rules, _firer, _target) = scene(
        "",
        "Crit.Chance=1.0\nCrit.AnimList=LOW,HIGH\n",
        "",
        "",
        "Russians",
    );
    let warhead = rules.warhead("WH").expect("warhead");
    let coordinate = ProjectileCoord::new(22 * 256 + 128, 20 * 256 + 128, 0);
    let mut selected = |damage: i32| {
        super::detonation_anim::select(&mut sim, &rules, warhead, damage, 0, coordinate, true)
            .map(|id| sim.interner.resolve(id).to_owned())
    };
    assert_eq!(
        selected(30),
        Some("HIGH".to_owned()),
        "30 is the second band"
    );
    assert_eq!(
        selected(-10),
        Some("LOW".to_owned()),
        "-10 selects as 10, not as a huge unsigned value"
    );
}

/// `Crit.Warhead.FullDetonation=no`: `DamageAreaWithTarget` runs the crit
/// warhead's area damage without a bullet, so the target takes its damage and
/// no detonation animation is selected.
#[test]
fn crit_warhead_area_arm_damages_the_target() {
    let sections = "[CRITWH]\nCellSpread=1\n";
    let (mut sim, rules, firer, target) = scene(
        "1=CRITWH\n",
        "Crit.Chance=1.0\nCrit.ExtraDamage=30\nCrit.Warhead=CRITWH\n\
         Crit.Warhead.FullDetonation=no\n",
        "",
        sections,
        "Russians",
    );
    let (mut plain, plain_rules, plain_firer, plain_target) = scene(
        "1=CRITWH\n",
        "Crit.Warhead=CRITWH\nCrit.Warhead.FullDetonation=no\n",
        "",
        sections,
        "Russians",
    );
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    shoot(&mut plain, &plain_rules, "WH", plain_firer, plain_target, 0);
    assert_eq!(
        health(&plain, plain_target) - health(&sim, target),
        30,
        "the crit warhead's area damage reached the target"
    );
}

/// The `CRITIMMUNE` Phobos rank ability and a `Crit.AffectsTarget` that does
/// not list the target's class each stop the same always-crit warhead.
#[test]
fn crit_immune_ability_and_target_filter_stop_the_crit() {
    let crit_damage = |wh_ini: &str, target_ini: &str, veteran: bool| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, target_ini, "", "Russians");
        let (mut plain, plain_rules, plain_firer, plain_target) =
            scene("", "", target_ini, "", "Russians");
        if veteran {
            for (world, id) in [(&mut sim, target), (&mut plain, plain_target)] {
                world
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .veterancy
                    .set_veteran();
            }
        }
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        shoot(&mut plain, &plain_rules, "WH", plain_firer, plain_target, 0);
        health(&plain, plain_target) - health(&sim, target)
    };

    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\n",
            "VeteranAbilities=CRITIMMUNE\n",
            true,
        ),
        0,
        "a veteran with CRITIMMUNE refuses the crit"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nCrit.AffectsTarget=aircraft\n",
            "",
            false,
        ),
        0,
        "an aircraft-only filter skips a ground unit"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\n",
            "VeteranAbilities=CRITIMMUNE\n",
            false,
        ),
        50,
        "a rookie with the same list is not immune"
    );
}

/// `Crit.ActiveChanceAnims` picks one entry with a `RandomRanged(0, len-1)`
/// draw after the crit and `ReturnWarhead` buffers.
#[test]
fn crit_active_chance_anims_draw_after_the_buffers() {
    let (mut sim, rules, firer, target) = scene(
        "",
        "Crit.Chance=1.0\nCrit.ExtraDamage=0\nCrit.ActiveChanceAnims=ONE,TWO\n",
        "",
        "",
        "Russians",
    );
    detonate_once(&mut sim, &rules, firer, target, None);
    let mut expected = {
        let (sim, _rules, _firer, _target) = scene("", "", "", "", "Russians");
        sim.scenario_rng.clone()
    };
    expected.random_double();
    expected.random_double();
    expected.next_range_i32_inclusive(0, 1);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "two buffers, then one anim pick"
    );
}

/// `CRITCHANCE` + `VeteranCritChance` scale the warhead chance; the same
/// rules without the rank ability keep the base chance.
#[test]
fn crit_chance_rank_ability_scales_the_warhead_chance() {
    let scene_for = |firer_ini: &str, veteran: bool| {
        let (mut sim, rules, firer, target) = scene_with_firer(
            "",
            "Crit.Chance=0.05
Crit.ExtraDamage=50
",
            "",
            "",
            firer_ini,
            "Russians",
        );
        if veteran {
            sim.substrate
                .entities
                .get_mut(firer)
                .unwrap()
                .veterancy
                .set_veteran();
        }
        // The shot's dice must miss the base 0.05 chance.
        let mut probe = sim.scenario_rng.clone();
        assert!(probe.random_double() > 0.05);
        (sim, rules, firer, target)
    };
    let control_health = |firer_ini: &str, veteran: bool| {
        let (mut sim, rules, firer, target) = scene_for(firer_ini, veteran);
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        health(&sim, target)
    };

    let control = control_health("", false);
    let (mut sim, rules, firer, target) = scene_for(
        "VeteranAbilities=CRITCHANCE
VeteranCritChance=20
",
        true,
    );
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(
        control - health(&sim, target),
        50,
        "0.05 x 20 reaches a certain crit"
    );

    let (mut sim, rules, firer, target) = scene_for(
        "VeteranCritChance=20
",
        true,
    );
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(
        control - health(&sim, target),
        0,
        "without the ability the chance stays at 0.05"
    );
}

/// `Crit.ExtraDamage.ApplyFirepowerMult` scales the extra damage by the
/// firer's current firepower multiplier (here its FIREPOWER rank stage).
#[test]
fn crit_extra_damage_can_apply_the_firepower_multiplier() {
    let (mut sim, rules, firer, target) = scene_with_firer(
        "",
        "Crit.Chance=1.0\nCrit.ExtraDamage=50\nCrit.ExtraDamage.ApplyFirepowerMult=yes\n",
        "",
        "[General]\nVeteranCombat=2\n",
        "VeteranAbilities=FIREPOWER\n",
        "Russians",
    );
    sim.substrate
        .entities
        .get_mut(firer)
        .unwrap()
        .veterancy
        .set_veteran();
    let (mut plain, plain_rules, plain_firer, plain_target) = scene("", "", "", "", "Russians");
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    shoot(&mut plain, &plain_rules, "WH", plain_firer, plain_target, 0);
    assert_eq!(
        health(&plain, plain_target) - health(&sim, target),
        100,
        "50 extra damage times the veteran FIREPOWER 2"
    );
}

/// `Crit.AnimOnAffectedTargets` keeps the ordinary damage animation even when
/// the crit applies its own animation at the target.
#[test]
fn crit_affected_target_anims_keep_the_ordinary_damage_animation() {
    let (mut sim, rules, _firer, _target) = scene(
        "",
        "Crit.Chance=1.0\nCrit.AnimList=CRITICAL\nAnimList=NORMAL\n\
         Crit.AnimOnAffectedTargets=yes\n",
        "",
        "",
        "Russians",
    );
    let warhead = rules.warhead("WH").expect("warhead");
    let coordinate = ProjectileCoord::new(22 * 256 + 128, 20 * 256 + 128, 0);
    assert_eq!(
        super::detonation_anim::select(&mut sim, &rules, warhead, 10, 0, coordinate, true)
            .map(|id| sim.interner.resolve(id).to_owned()),
        Some("NORMAL".to_owned()),
        "the crit list belongs to the affected targets"
    );
}

/// `Crit.Warhead` is read with `Read<true>`: a warhead only the key names is
/// still allocated and detonates.
#[test]
fn crit_warhead_allocates_an_unlisted_type() {
    let sections = "[CRITWH]\nCellSpread=1\n";
    let (mut sim, rules, firer, target) = scene(
        "",
        "Crit.Chance=1.0\nCrit.ExtraDamage=30\nCrit.Warhead=CRITWH\n",
        "",
        sections,
        "Russians",
    );
    assert!(
        rules.warhead("CRITWH").is_some(),
        "the key allocates the type"
    );
    let (mut plain, plain_rules, plain_firer, plain_target) = scene("", "", "", "", "Russians");
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    shoot(&mut plain, &plain_rules, "WH", plain_firer, plain_target, 0);
    assert_eq!(
        health(&plain, plain_target) - health(&sim, target),
        30,
        "the allocated crit warhead detonated"
    );
}
