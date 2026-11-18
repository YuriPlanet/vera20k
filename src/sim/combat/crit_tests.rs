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
        0,
        false,
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

/// Phobos `WarheadTypeExt::CanAffectTarget`'s trigger gates
/// (`Ext/WarheadType/Body.cpp:37-51`): a warhead whose own
/// `AffectsAbovePercent`/`AffectsBelowPercent`, `AffectsVeterancy` or
/// `AffectsInvokerOnly` refuses the target skips it before the crit pass.
#[test]
fn warhead_trigger_gates_skip_the_per_target_pass() {
    let crit_damage = |wh_ini: &str, set_health: Option<i32>, rank: Option<u16>| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        let (mut plain, plain_rules, plain_firer, plain_target) = scene("", "", "", "", "Russians");
        for (world, id) in [(&mut sim, target), (&mut plain, plain_target)] {
            if let Some(value) = set_health {
                world
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .health
                    .set_for_test(value);
            }
            if let Some(rank) = rank {
                world
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .veterancy
                    .set_rank_for_test(rank);
            }
        }
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        shoot(&mut plain, &plain_rules, "WH", plain_firer, plain_target, 0);
        health(&plain, plain_target) - health(&sim, target)
    };

    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsAbovePercent=0.9\n",
            Some(50),
            None
        ),
        0,
        "10% health fails the warhead's own threshold"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsAbovePercent=0.9\n",
            None,
            None
        ),
        50
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsBelowPercent=0.5\n",
            None,
            None
        ),
        0,
        "full health fails (0.0, 0.5]"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsBelowPercent=0.5\n",
            Some(100),
            None
        ),
        50,
        "20% health is inside (0.0, 0.5]"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsVeterancy=rookie\n",
            None,
            Some(100)
        ),
        0,
        "a veteran target fails the rookie-only mask"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsVeterancy=veteran\n",
            None,
            Some(100)
        ),
        50
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsInvokerOnly=yes\n",
            None,
            None
        ),
        0,
        "the firer is not the target"
    );
    assert_eq!(
        crit_damage(
            "Crit.Chance=1.0\nCrit.ExtraDamage=50\nAffectsInvokerOnly=yes\n\
             AffectsInvokerOnly.Reverse=yes\n",
            None,
            None
        ),
        50,
        "the reversed gate refuses only the invoker itself"
    );
}

/// Phobos `WarheadTypeExt::Detonate`'s `TransactMoney` block
/// (`Ext/WarheadType/Detonate.cpp:102-111`) through the bullet detonation:
/// a positive amount credits the firing house, a negative one debits it and a
/// zero amount moves nothing.
#[test]
fn transact_money_moves_the_firing_house_by_the_signed_amount() {
    // (firing house delta, target house delta) from `start` credits each.
    let run = |amount: i32, start: i32| {
        let (mut sim, rules, firer, target) =
            scene("", &format!("TransactMoney={amount}\n"), "", "", "Russians");
        let owner = sim.entities().get(firer).unwrap().owner();
        let target_owner = sim.entities().get(target).unwrap().owner();
        for house in [owner, target_owner] {
            sim.houses
                .get_mut(&house)
                .unwrap()
                .economy_mut_for_test()
                .set_credits_for_test(start);
        }
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        (
            sim.houses.get(&owner).unwrap().economy().credits() - start,
            sim.houses.get(&target_owner).unwrap().economy().credits() - start,
        )
    };
    assert_eq!(run(25, 0), (25, 0), "a positive value credits the firer");
    assert_eq!(run(-40, 100), (-40, 0), "a negative value debits the firer");
    assert_eq!(
        run(-40, 10),
        (-10, 0),
        "Spend_Money clamps at the balance it covers"
    );
    assert_eq!(run(0, 0), (0, 0), "zero moves nothing");
}

/// `TransactMoney.Display`'s label fields: `AtFirer` anchors on the firer's
/// `Location` instead of the impact, `Houses` and `Offset` travel with the
/// event, and the label has no source object (`pSource = nullptr`).
#[test]
fn transact_money_display_uses_at_firer_houses_and_offset() {
    use crate::rules::phobos_shields::AffectedHouse;
    use crate::sim::flying_text::BountyText;
    use crate::sim::movement::ground_pose::object_location;
    use crate::sim::world::FrameEffects;
    use std::cell::RefCell;

    let events = RefCell::new(Vec::new());
    let observe = |_: &Simulation, _: &RuleSet, text: BountyText| {
        events.borrow_mut().push(text);
    };
    let effects = FrameEffects::empty().with_bounty_text(&observe);
    let (mut sim, rules, firer, target) = scene(
        "",
        "TransactMoney=13\nTransactMoney.Display=yes\n\
         TransactMoney.Display.AtFirer=yes\nTransactMoney.Display.Houses=enemies\n\
         TransactMoney.Display.Offset=6,-4\n",
        "",
        "",
        "Russians",
    );
    let owner = sim.entities().get(firer).unwrap().owner();
    let firer_coord = object_location(
        sim.entities().get(firer).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    let shot = detonation(&mut sim, "WH", firer, target, 0);
    let mut run = ReceiverRun::default().with_effects(effects);
    commit_projectiles(&mut sim, &mut run, &[shot], &rules, None);
    run.finish();
    let events = events.borrow();
    assert_eq!(events.len(), 1, "Display=yes emits one label");
    assert_eq!(events[0].entity_id, None, "FlyingStrings source is nullptr");
    assert_eq!(events[0].amount, 13);
    assert_eq!(
        events[0].coord,
        ProjectileCoord::new(firer_coord.x, firer_coord.y, firer_coord.z),
        "AtFirer anchors on the firer's Location"
    );
    assert_eq!(events[0].display_to, Some(AffectedHouse::ENEMIES));
    assert_eq!(events[0].display_owner, Some(owner));
    assert_eq!(events[0].pixel_offset, [6, -4]);

    // The same warhead without Display emits no label.
    let hidden = RefCell::new(Vec::new());
    let observe_hidden = |_: &Simulation, _: &RuleSet, text: BountyText| {
        hidden.borrow_mut().push(text);
    };
    let hidden_effects = FrameEffects::empty().with_bounty_text(&observe_hidden);
    let (mut sim, rules, firer, target) = scene("", "TransactMoney=13\n", "", "", "Russians");
    let shot = detonation(&mut sim, "WH", firer, target, 0);
    let mut run = ReceiverRun::default().with_effects(hidden_effects);
    commit_projectiles(&mut sim, &mut run, &[shot], &rules, None);
    run.finish();
    assert!(hidden.borrow().is_empty(), "Display=no emits no label");

    // `if (this->TransactMoney)` wraps the Display block too: a zero amount
    // moves nothing and emits no label.
    let zero = RefCell::new(Vec::new());
    let observe_zero = |_: &Simulation, _: &RuleSet, text: BountyText| {
        zero.borrow_mut().push(text);
    };
    let zero_effects = FrameEffects::empty().with_bounty_text(&observe_zero);
    let (mut sim, rules, firer, target) = scene(
        "",
        "TransactMoney=0\nTransactMoney.Display=yes\n",
        "",
        "",
        "Russians",
    );
    let shot = detonation(&mut sim, "WH", firer, target, 0);
    let mut run = ReceiverRun::default().with_effects(zero_effects);
    commit_projectiles(&mut sim, &mut run, &[shot], &rules, None);
    run.finish();
    assert!(zero.borrow().is_empty(), "a zero amount emits no label");
}

/// Phobos `TechnoClass_CanFire @ 0x6FC339`
/// (`Ext/Techno/Hooks.Firing.cpp:325-328`): a negative `TransactMoney`
/// warhead refuses the shot while the firing house's `Available_Money` is
/// below `-amount`; an affordable or non-negative amount fires.
#[cfg(feature = "phobos")]
#[test]
fn transact_money_gates_the_shot_on_the_firing_house_wallet() {
    use crate::sim::combat::fire_error::FireError;
    let error = |amount: i32, credits: i32| {
        let (mut sim, rules, firer, target) =
            scene("", &format!("TransactMoney={amount}\n"), "", "", "Russians");
        let owner = sim.entities().get(firer).unwrap().owner();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut_for_test()
            .set_credits_for_test(credits);
        crate::sim::combat::fire_error_world::FireSubject {
            world: &sim,
            rules: &rules,
            overlay_registry: None,
            fog: None,
            firer: sim.entities().get(firer).unwrap(),
            obj: rules.object("FIRER").unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(target)),
            weapon_index: 0,
        }
        .fire_error(true)
    };
    assert_eq!(
        error(-100, 40),
        FireError::Cant,
        "40 credits cannot pay 100"
    );
    assert_ne!(error(-100, 100), FireError::Cant, "an equal balance pays");
    assert_ne!(error(-100, 250), FireError::Cant, "a surplus pays");
    assert_ne!(
        error(100, 0),
        FireError::Cant,
        "a positive amount never gates the shot"
    );
}

/// Phobos `WarheadTypeExt::ApplyConvert`
/// (`Ext/WarheadType/Detonate.cpp:215,669-678`): the per-target pass converts
/// a Foot through the first applicable `Convert{i}` pair, with `All` as a
/// warhead's default source-house filter.
#[cfg(feature = "phobos")]
#[test]
fn warhead_convert_pairs_convert_the_target_foot() {
    let converted = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        sim.interner
            .resolve(sim.entities().get(target).unwrap().type_ref())
            .to_owned()
    };
    assert_eq!(
        converted("Convert0.From=TARGET\nConvert0.To=FIRER\n"),
        "FIRER"
    );
    assert_eq!(
        converted("Convert.From=TARGET\nConvert.To=FIRER\n"),
        "FIRER",
        "the unnumbered pair fills an empty list"
    );
    assert_eq!(
        converted("Convert0.From=TARGET\nConvert0.To=FIRER\nConvert0.AffectsHouse=owner\n"),
        "TARGET",
        "owner-only refuses the enemy target"
    );
    assert_eq!(
        converted("Convert0.From=TARGET\nConvert0.To=FIRER\nConvert0.AffectsHouse=enemies\n"),
        "FIRER",
        "enemies accepts the enemy target"
    );
    assert_eq!(
        converted("Convert0.From=FIRER\nConvert0.To=FIRER\n"),
        "TARGET",
        "a known non-matching From skips the pair"
    );
    assert_eq!(
        converted("Convert0.From=NOSUCH\nConvert0.To=FIRER\n"),
        "FIRER",
        "an all-unknown From is an empty list and converts any Foot"
    );
    assert_eq!(
        converted("Convert0.To=NOSUCH\nConvert1.To=FIRER\n"),
        "TARGET",
        "an unknown To ends the numbered list instead of skipping the row"
    );
    assert_eq!(
        converted(
            "Convert0.From=TARGET\nConvert0.To=FIRER\nConvert.From=TARGET\nConvert.To=TARGET\n"
        ),
        "TARGET",
        "the unnumbered pair replaces row 0, so no FIRER row applies"
    );
    assert_eq!(
        converted("Convert0.From=TARGET\nConvert0.To=FIRER\nConvert0.AffectedHouses=owner\n"),
        "TARGET",
        "the deprecated AffectedHouses key is read"
    );
    assert_eq!(
        converted(
            "Convert0.From=TARGET\nConvert0.To=FIRER\nConvert0.AffectedHouses=owner\nConvert0.AffectsHouse=enemies\n"
        ),
        "FIRER",
        "a set AffectsHouse overrides AffectedHouses"
    );
    assert_eq!(
        converted("Convert0.To=FIRER\nConvert1.From=TARGET\nConvert1.To=TARGET\n"),
        "FIRER",
        "the first converting row stops the list"
    );
    assert_eq!(converted(""), "TARGET", "no pairs, no conversion");
    let after = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .health
            .set_for_test(100);
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        health(&sim, target)
    };
    assert_eq!(
        after("Convert0.From=TARGET\nConvert0.To=TARGET\n"),
        after(""),
        "a same-type pair is a no-op, not a full-health reset"
    );
}

/// Phobos `WarheadTypeExt::ApplyReturnWarhead`
/// (`Ext/WarheadType/Detonate.cpp:610-636`): the per-target pass sends the
/// `ReturnWarhead=` warhead back at the techno that fired the bullet, gated
/// by the shared chance buffer, the source house and the victim filters.
#[cfg(feature = "phobos")]
#[test]
fn return_warhead_strikes_the_firer_back() {
    let outcome = |wh_ini: &str| {
        let (mut sim, rules, firer, target) =
            scene("1=RH\n", wh_ini, "", "[RH]\nCellSpread=0\n", "Russians");
        let mut expected = sim.scenario_rng.clone();
        // The per-detonation crit and ReturnWarhead buffers.
        expected.random_double();
        expected.random_double();
        let before = health(&sim, firer);
        shoot(&mut sim, &rules, "WH", firer, target, 10);
        (
            before - health(&sim, firer),
            sim.scenario_rng.logical_state() == expected.logical_state(),
        )
    };
    let (damage, _) = outcome("ReturnWarhead=RH\nReturnWarhead.Damage=30\n");
    assert_eq!(damage, 30, "the firer takes the return warhead");
    let (damage, one_draw) = outcome(
        "ReturnWarhead=RH\nReturnWarhead.Damage=30\nReturnWarhead.AffectsTarget=infantry\n",
    );
    assert_eq!(damage, 0, "the victim class filter refuses the vehicle");
    assert!(one_draw, "a refused return only draws the shared buffer");
    let (damage, one_draw) =
        outcome("ReturnWarhead=RH\nReturnWarhead.Damage=30\nReturnWarhead.AffectsHouse=owner\n");
    assert_eq!(damage, 0, "owner-only refuses the enemy source house");
    assert!(one_draw, "a refused return only draws the shared buffer");
    let (damage, _) =
        outcome("ReturnWarhead=RH\nReturnWarhead.Damage=30\nReturnWarhead.FullDetonation=no\n");
    assert_eq!(
        damage, 30,
        "the direct ReceiveDamage arm also hits the firer"
    );
}

/// Phobos `WarheadTypeExt::Detonate`'s Reveal block
/// (`Ext/WarheadType/Detonate.cpp:82-100`): a negative value reveals the
/// whole map for the firing house; the default 0 does nothing.
#[cfg(feature = "phobos")]
#[test]
fn reveal_warhead_reveals_the_map_for_its_owner() {
    let revealed = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        let house = sim.interner.intern("Americans");
        // Reveal stores the firing House's map-clear byte (`+0x241`) before
        // any client-map work, so no fog-viewer fixture is needed.
        sim.reset_shroud_for_owner(house, &rules);
        assert!(
            !sim.houses[&house].map_is_clear(),
            "the shroud starts closed"
        );
        detonate_once(&mut sim, &rules, firer, target, Some(target));
        sim.houses[&house].map_is_clear()
    };
    assert!(!revealed(""), "Reveal defaults to 0");
    assert!(
        revealed("Reveal=-1\n"),
        "a negative value reveals the whole map for the firing house"
    );
}

/// Phobos `WarheadTypeExt::Detonate`'s positive Reveal arm
/// (`Ext/WarheadType/Detonate.cpp:82-100`): the radius reaches the client
/// maps `RevealArea2`'s house gate admits — the warhead's house and, with
/// `AllyReveal=yes`, the house it names an ally — and `Detonate.cpp:88`
/// admits that arm only while the current player is the warhead's house or
/// its ally and is not defeated. The enemy's map keeps its shroud. Every unit
/// in this scene carries `Sight=0`, so only the warhead can reveal the probe
/// cells.
#[cfg(feature = "phobos")]
#[test]
fn a_positive_reveal_touches_only_the_owner_and_its_allies() {
    // `Koreans` owns the warhead and names `Americans`, the current player,
    // its ally; `IsAlliedWith @ 0x004F9A50` reads the asking house's own bits,
    // so only that one-way declaration admits the player's map. `Russians` is
    // the enemy and keeps a client map of its own.
    let reach = |defeated: bool| {
        let (mut sim, rules, _firer, target) = scene(
            "",
            "Reveal=4\n",
            "",
            "[AudioVisual]\nAllyReveal=yes\n",
            "Russians",
        );
        let americans = sim.interner.intern("Americans");
        let russians = sim.interner.intern("Russians");
        let koreans = sim.interner.intern("Koreans");
        // The arena scene carries no client map; `ResetShroud`/`RevealArea`
        // need a real grid, as every other reveal test gives its world.
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                koreans, 0, None, true, 0, 10,
            ));
        sim.houses
            .get_mut(&russians)
            .expect("russians")
            .set_is_human_for_test(true);
        sim.house_alliances_mut_for_test()
            .insert("KOREANS".into(), ["AMERICANS".into()].into());
        sim.reset_shroud_for_owner(koreans, &rules);
        sim.reset_shroud_for_owner(americans, &rules);
        sim.reset_shroud_for_owner(russians, &rules);
        // The enemy's own explored cell is its counterweight: "untouched" then
        // means the reveal wrote no enemy cell, not that it had no map.
        sim.fog.reveal_map_cell(
            russians,
            (22, 20),
            false,
            None,
            crate::sim::vision::MapRevealProjection::Transient,
        );
        sim.session.set_current_house_for_test(Some(americans));
        if defeated {
            sim.houses
                .get_mut(&americans)
                .expect("americans")
                .set_is_defeated_for_test(true);
        }
        let firer = sim
            .spawn_object("FIRER", "Koreans", 19, 20, 0, &rules)
            .expect("ally firer");
        detonate_once(&mut sim, &rules, firer, target, Some(target));
        (
            sim.fog.is_cell_revealed(koreans, 22, 20),
            sim.fog.is_cell_revealed(americans, 22, 20),
            sim.fog.is_cell_revealed(russians, 25, 20),
            sim.fog.is_cell_revealed(koreans, 25, 20),
            sim.fog.is_cell_revealed(koreans, 17, 20),
            sim.fog.is_cell_revealed(russians, 22, 20),
        )
    };
    assert_eq!(
        reach(false),
        (true, true, false, true, false, true),
        "a radius 4 reaches the owner and its ally, and stops at its edge"
    );
    assert_eq!(
        reach(true),
        (false, false, false, false, false, true),
        "a defeated current player admits no client map, the owner's included"
    );
}

/// Phobos `WarheadTypeExt::Detonate`'s negative Reveal arm
/// (`Ext/WarheadType/Detonate.cpp:94-99`, `MapClass::Reveal(pHouse)`): the
/// firing house's whole map opens, including cells out of every unit's sight,
/// and no other house's map is written.
#[cfg(feature = "phobos")]
#[test]
fn a_negative_reveal_clears_the_owner_whole_map() {
    let (mut sim, rules, firer, target) = scene("", "Reveal=-1\n", "", "", "Russians");
    let americans = sim.interner.intern("Americans");
    let russians = sim.interner.intern("Russians");
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    sim.houses
        .get_mut(&russians)
        .expect("russians")
        .set_is_human_for_test(true);
    sim.session.set_current_house_for_test(Some(americans));
    detonate_once(&mut sim, &rules, firer, target, Some(target));
    for cell in [(22, 20), (10, 16), (6, 16), (28, 16)] {
        assert!(
            sim.fog.is_cell_revealed(americans, cell.0, cell.1),
            "the firing house's {cell:?} opens"
        );
        assert!(
            !sim.fog.is_cell_revealed(russians, cell.0, cell.1),
            "another house's {cell:?} keeps its shroud"
        );
    }
    assert!(
        !sim.fog.by_owner().contains_key(&russians),
        "another house's client map is never written"
    );
}

/// Phobos `WarheadTypeExt::Detonate`'s CreateGap block
/// (`Ext/WarheadType/Detonate.cpp:51-80`): a negative value reshrouds the
/// whole map for every non-allied house while the firing house keeps its
/// map; the default 0 does nothing.
#[cfg(feature = "phobos")]
#[test]
fn create_gap_reshrouds_the_map_for_non_allies() {
    let maps = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        let americans = sim.interner.intern("Americans");
        let russians = sim.interner.intern("Russians");
        // Reveal and Reshroud store each House's map-clear byte (`+0x241`)
        // before any client-map work, so no fog-viewer fixture is needed.
        sim.reveal_map_for_owner(americans, &rules);
        sim.reveal_map_for_owner(russians, &rules);
        assert!(sim.houses[&americans].map_is_clear());
        assert!(sim.houses[&russians].map_is_clear());
        detonate_once(&mut sim, &rules, firer, target, Some(target));
        (
            sim.houses[&americans].map_is_clear(),
            sim.houses[&russians].map_is_clear(),
        )
    };
    assert_eq!(maps(""), (true, true), "CreateGap defaults to 0");
    assert_eq!(
        maps("CreateGap=-1\n"),
        (true, false),
        "the non-allied house is reshrouded, the owner keeps its map"
    );
}

/// Phobos `DetonateOnOneUnit`'s `ApplyReverseEngineer`
/// (`Ext/WarheadType/Detonate.cpp:227-228,260-264`): with Ares present, the
/// firing house records each hit victim's type; the default skips.
#[cfg(all(feature = "phobos", feature = "ares"))]
#[test]
fn reverse_engineer_warhead_records_the_victim_type() {
    let recorded = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        let owner = sim.entities().get(firer).expect("firer").owner();
        detonate_once(&mut sim, &rules, firer, target, Some(target));
        sim.houses.get(&owner).is_some_and(|house| {
            house.has_ares_reverse_engineered(
                crate::rules::object_type::ObjectCategory::Vehicle,
                "TARGET",
            )
        })
    };
    assert!(!recorded(""), "ReverseEngineer defaults to no");
    assert!(
        recorded("ReverseEngineer=yes\n"),
        "the victim type is recorded for the firing house"
    );
}

/// Phobos `DetonateOnOneUnit`'s Taunt arm
/// (`Ext/WarheadType/Detonate.cpp:236-237`): every hit target is forced onto
/// `Mission::Attack` against the firing techno; the default is off.
#[cfg(feature = "phobos")]
#[test]
fn taunt_forces_the_hit_targets_mission_onto_the_firer() {
    use crate::sim::mission::{MissionId, MissionType};
    let mission = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        detonate_once(&mut sim, &rules, firer, target, Some(target));
        sim.substrate
            .entities
            .get(target)
            .unwrap()
            .mission
            .current()
    };
    assert_eq!(
        mission("Taunt=yes\n"),
        MissionId::from_known(MissionType::Attack),
        "Taunt overrides the target's mission"
    );
    assert_ne!(
        mission(""),
        MissionId::from_known(MissionType::Attack),
        "Taunt defaults to no"
    );
}

/// Phobos `WarheadTypeExt::ApplyAmmoModifier`
/// (`Ext/WarheadType/Detonate.cpp:221-222,923-932`): `Ammo` adds to the
/// target and clamps into `0..type->Ammo`; the default 0 leaves it alone.
#[cfg(feature = "phobos")]
#[test]
fn warhead_ammo_modifies_the_target_ammo_with_the_native_clamp() {
    let after = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "Ammo=5\n", "", "Russians");
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .ammo_mut()
            .unwrap()
            .set_current_for_test(2);
        detonate_once(&mut sim, &rules, firer, target, Some(target));
        sim.substrate
            .entities
            .get(target)
            .unwrap()
            .ammo()
            .unwrap()
            .current()
    };
    assert_eq!(after("Ammo=2\n"), 4);
    assert_eq!(after("Ammo=-1\n"), 1);
    assert_eq!(after("Ammo=-9\n"), 0, "negative clamps to zero");
    assert_eq!(after("Ammo=9\n"), 5, "clamped to the type's Ammo");
    assert_eq!(after(""), 2, "the default 0 is a no-op");
    // A type that leaves `Ammo=` unset keeps the constructor's -1; the native
    // upper clamp stores that -1 instead of asserting a 0..-1 range.
    let (mut sim, rules, firer, target) = scene("", "Ammo=2\n", "", "", "Russians");
    detonate_once(&mut sim, &rules, firer, target, Some(target));
    assert_eq!(
        sim.substrate
            .entities
            .get(target)
            .unwrap()
            .ammo()
            .unwrap()
            .current(),
        -1,
        "the unset Ammo=-1 upper clamp"
    );
}

/// Phobos `WarheadTypeExt::ApplyRemoveDisguise`
/// (`Ext/WarheadType/Detonate.cpp:224-225,466-475`): the per-target pass
/// strips a Unit's disguise even when the shot deals no damage; a Unit also
/// clears its recorded identity, and the flag defaults to off.
#[cfg(feature = "phobos")]
#[test]
fn remove_disguise_strips_a_target_on_impact() {
    let setup = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        let type_id = sim.interner.intern("TARGET");
        let house = sim.interner.intern("Russians");
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .disguise
            .get_or_insert_with(Default::default)
            .acquire(0, Some(type_id), Some(house));
        detonate_once(&mut sim, &rules, firer, target, Some(target));
        (sim, target)
    };
    let (sim, target) = setup("RemoveDisguise=yes\n");
    let disguise = sim
        .substrate
        .entities
        .get(target)
        .unwrap()
        .disguise
        .as_ref()
        .unwrap();
    assert!(!disguise.is_disguised(), "the warhead strips the disguise");
    assert_eq!(disguise.type_id(), None, "a Unit clears identity too");
    assert_eq!(disguise.house(), None);
    let (sim, target) = setup("");
    assert!(
        sim.substrate
            .entities
            .get(target)
            .unwrap()
            .disguise
            .as_ref()
            .unwrap()
            .is_disguised(),
        "RemoveDisguise defaults to no"
    );
}

/// Phobos `TechnoClass_ReceiveDamage_Shield @ 0x701900`'s first block
/// (`Ext/Techno/Hooks.ReceiveDamage.cpp:26-34`) zeroes the whole packet when
/// the warhead's own triggers refuse the victim. Under Phobos' default
/// `Shrapnel.ObeyWarheadTriggerConditions=yes` the earlier
/// `BulletClass_Logics_NewChecks` gate (`0x4690D4`,
/// `Ext/Bullet/Hooks.DetonateLogics.cpp:22-29`) skips the whole body first, so
/// no packet reaches ReceiveDamage and its `0x7019D8` low-damage floor never
/// applies: a refused bullet detonation deals nothing.
#[test]
fn warhead_trigger_gates_zero_the_received_damage() {
    let damage = |wh_ini: &str, no_invoker: bool, set_health: Option<i32>| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        if let Some(value) = set_health {
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .health
                .set_for_test(value);
        }
        let source = if no_invoker { 0 } else { firer };
        let before = health(&sim, target);
        shoot(&mut sim, &rules, "WH", source, target, 100);
        before - health(&sim, target)
    };

    assert_eq!(damage("", false, None), 100, "control");
    assert_eq!(
        damage("AffectsAbovePercent=0.9\n", false, Some(50)),
        0,
        "10% health refuses the whole bullet body before any packet"
    );
    assert_eq!(
        damage("AffectsAbovePercent=0.9\n", false, None),
        100,
        "full health takes the damage"
    );
    assert_eq!(
        damage("AffectsInvokerOnly=yes\n", false, None),
        0,
        "the firer is not the victim, so the body is skipped"
    );
    assert_eq!(
        damage(
            "AffectsInvokerOnly=yes\nAffectsInvokerOnly.Reverse=yes\n",
            false,
            None
        ),
        100
    );
    assert_eq!(
        damage("AffectsInvokerOnly=yes\n", true, None),
        0,
        "no invoker and the default global true refuses"
    );
    assert_eq!(
        damage(
            "AffectsInvokerOnly=yes\nAffectsInvokerOnly.IgnoreInvokerState=no\n",
            true,
            None
        ),
        100,
        "IgnoreInvokerState=no admits an invoker-less packet"
    );
}

/// With the bullet's `Shrapnel.ObeyWarheadTriggerConditions=no` the whole-body
/// skip does not run, so a trigger-refusing target still reaches
/// `TechnoClass::ReceiveDamage`'s zeroing gate and its `0x7019D8` one-point
/// floor.
#[test]
fn the_receive_damage_gate_floors_a_key_off_bullet() {
    let (mut sim, rules, firer, target) = scene(
        "",
        "AffectsAbovePercent=0.9\n",
        "Secondary=GUNB\n",
        "[GUNB]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\nProjectile=SHELL\n\
         [SHELL]\nShrapnel.ObeyWarheadTriggerConditions=no\n",
        "Russians",
    );
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .health
        .set_for_test(50);
    assert!(
        rules.projectile("SHELL").is_some(),
        "GUNB's Projectile allocates the bullet type"
    );
    let bullet = sim.interner.intern("SHELL");
    let mut detonation = detonation(&mut sim, "WH", firer, target, 100);
    detonation.payload =
        ProjectilePayload::new(100, sim.interner.intern("WH"), Some(bullet), false);
    let before = health(&sim, target);
    let mut run = ReceiverRun::default();
    commit_projectiles(&mut sim, &mut run, &[detonation], &rules, None);
    run.finish();
    assert_eq!(
        before - health(&sim, target),
        1,
        "the receive gate zeroes the packet and the floor leaves one point"
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

/// `PossibleCellSpreadDetonate` lists `IvanBomb.Detonate`
/// (`Ext/WarheadType/Body.cpp`'s "List all Warheads here that respect
/// CellSpread", read by `WarheadTypeExt::Detonate`): a warhead whose only
/// member is the fused-bomb detonation still starts the per-target pass and
/// draws both buffers. A warhead with every member off draws nothing, so the
/// member is what keeps the scenario stream aligned.
#[test]
fn ivan_bomb_detonate_is_a_possible_cell_spread_member() {
    let draws = |wh_ini: &str| {
        let (mut sim, rules, firer, target) = scene("", wh_ini, "", "", "Russians");
        detonate_once(&mut sim, &rules, firer, target, None);
        sim.scenario_rng.logical_state()
    };
    assert_eq!(
        draws("IvanBomb.Detonate=yes\n"),
        after_doubles(2),
        "the crit buffer and the ReturnWarhead buffer"
    );
    assert_eq!(
        draws(""),
        after_doubles(0),
        "no member and no chance, no draws"
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

/// Phobos `ApplyPenetratesTransport` (`Ext/WarheadType/Detonate.cpp:780-930`):
/// a warhead level above the transport's reaches its passengers with the
/// bullet's Health; `DamageAll=yes` hits both, `no` picks one, and a level at
/// or below the transport's skips the arm.
fn penetrates_scene(
    transport_level: i32,
    damage_all: bool,
) -> (Simulation, RuleSet, u64, u64, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=APC\n\
         [InfantryTypes]\n0=GI\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=0\nPenetratesTransport.Level=15\n\
         PenetratesTransport.DamageAll={}\n\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
         [APC]\nStrength=500\nArmor=none\nSpeed=6\nPassengers=2\n\
         PenetratesTransport.Level={transport_level}\n\
         [GI]\nStrength=100\nArmor=none\nSpeed=4\n",
        if damage_all { "yes" } else { "no" }
    )))
    .expect("penetrates rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let apc = sim
        .spawn_object("APC", "Russians", 22, 20, 0, &rules)
        .expect("transport");
    let gi1 = sim
        .spawn_object("GI", "Russians", 22, 21, 0, &rules)
        .expect("passenger");
    let gi2 = sim
        .spawn_object("GI", "Russians", 22, 19, 0, &rules)
        .expect("passenger");
    sim.substrate
        .entities
        .get_mut(apc)
        .expect("transport")
        .passenger_role
        .cargo_mut()
        .expect("transport cargo")
        .set_passengers_for_test(vec![gi1, gi2]);
    (sim, rules, firer, apc, gi1, gi2)
}

#[test]
fn penetrates_transport_damages_the_selected_passengers() {
    let (mut sim, rules, firer, apc, gi1, gi2) = penetrates_scene(1, true);
    shoot(&mut sim, &rules, "WH", firer, apc, 20);
    assert_eq!(health(&sim, gi1), 80, "DamageAll damages every passenger");
    assert_eq!(health(&sim, gi2), 80);

    let (mut sim, rules, firer, apc, gi1, gi2) = penetrates_scene(1, false);
    shoot(&mut sim, &rules, "WH", firer, apc, 20);
    let hits = usize::from(health(&sim, gi1) == 80) + usize::from(health(&sim, gi2) == 80);
    assert_eq!(hits, 1, "DamageAll=no picks exactly one passenger");

    let (mut sim, rules, firer, apc, gi1, gi2) = penetrates_scene(15, true);
    shoot(&mut sim, &rules, "WH", firer, apc, 20);
    assert_eq!(
        (health(&sim, gi1), health(&sim, gi2)),
        (100, 100),
        "a transport at the warhead's level is not penetrated"
    );
}

/// `Detonate.cpp:813,845,865,916`: every damaged passenger takes the
/// transport's Location before `ReceiveDamage`, so a lethal passenger's death
/// effects leave from the transport rather than its own stored cell.
#[test]
fn penetrates_transport_moves_damaged_passengers_to_the_transport() {
    let (mut sim, rules, firer, apc, gi1, gi2) = penetrates_scene(1, true);
    // A staging target (`TarCom+0x2B4`) refuses ReceiveDamage's Scatter gate
    // (`0x00702B47..0x00702CFC`), as a passenger firing from the transport
    // holds one; otherwise the damage scatters the passenger off the transport.
    for id in [gi1, gi2] {
        assert!(
            crate::sim::combat::attack_target::install_entity_attack_target_for_test(
                &mut sim.substrate.entities,
                id,
                firer
            )
        );
    }
    let transport = object_get_coords(
        sim.entities().get(apc).expect("transport"),
        sim.resolved_terrain.as_ref(),
    );
    assert_ne!(
        object_get_coords(
            sim.entities().get(gi1).expect("passenger"),
            sim.resolved_terrain.as_ref()
        ),
        transport,
        "the fixture starts the first passenger off the transport"
    );
    shoot(&mut sim, &rules, "WH", firer, apc, 20);
    for id in [gi1, gi2] {
        assert_eq!(
            object_get_coords(
                sim.entities().get(id).expect("passenger"),
                sim.resolved_terrain.as_ref()
            ),
            transport,
            "the damaged passenger now sits at the transport"
        );
    }
}

/// `Detonate.cpp:887-913`: the first iterated passenger's `NowDead` removes
/// the gunner, then the cargo head after the loop takes the slot back through
/// `ReceiveGunner`, so the transport leaves mode 0.
#[test]
fn penetrates_transport_hands_the_gunner_slot_to_the_cargo_head() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=GUNV\n\
         [InfantryTypes]\n0=GI1\n1=GI2\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=0\nPenetratesTransport.Level=15\n\
         PenetratesTransport.DamageAll=yes\nPenetratesTransport.DamageMultiplier=6.0\n\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\n\
         [GUNV]\nStrength=500\nArmor=none\nSpeed=6\nPassengers=2\nGunner=yes\nWeaponCount=20\n\
         PenetratesTransport.Level=1\n\
         [GI1]\nStrength=100\nArmor=none\nSpeed=4\nIFVMode=3\n\
         [GI2]\nStrength=300\nArmor=none\nSpeed=4\nIFVMode=7\n",
    ))
    .expect("gunner rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_21);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let apc = sim
        .spawn_object("GUNV", "Russians", 22, 20, 0, &rules)
        .expect("transport");
    let gi1 = sim
        .spawn_object("GI1", "Russians", 22, 21, 0, &rules)
        .expect("passenger");
    let gi2 = sim
        .spawn_object("GI2", "Russians", 22, 19, 0, &rules)
        .expect("passenger");
    {
        let transport = sim.substrate.entities.get_mut(apc).expect("transport");
        transport
            .passenger_role
            .cargo_mut()
            .expect("transport cargo")
            .set_passengers_for_test(vec![gi1, gi2]);
        transport
            .passenger_role
            .cargo_mut()
            .expect("transport cargo")
            .set_passenger_sizes_for_test(vec![1.0, 1.0]);
        transport.set_gunner_selection_for_test(3, -1);
    }
    assert_eq!(
        sim.entities().get(apc).unwrap().current_weapon_number(),
        3,
        "the head passenger's IFVMode is the gunner slot before the shot"
    );
    shoot(&mut sim, &rules, "WH", firer, apc, 20);
    assert!(
        sim.entities().get(gi1).unwrap().death.is_dying(),
        "the head passenger dies"
    );
    assert!(
        !sim.entities().get(gi2).unwrap().death.is_dying(),
        "the second passenger survives the adjusted damage"
    );
    let head = sim
        .entities()
        .get(apc)
        .and_then(|transport| transport.passenger_role.cargo())
        .and_then(|cargo| cargo.passengers().first().copied());
    let head_mode = head
        .and_then(|id| sim.entities().get(id))
        .and_then(|passenger| sim.interner.object_type(passenger.type_ref(), &rules))
        .map_or(0, |object| object.ifv_mode);
    assert_eq!(head, Some(gi2), "the surviving passenger is the cargo head");
    assert_eq!(
        sim.entities().get(apc).unwrap().current_weapon_number(),
        head_mode,
        "the tail re-seats the cargo head instead of leaving mode 0"
    );
}

/// Phobos `ChangeOwner` (`Ext/WarheadType/Detonate.cpp:207-211,488-522`):
/// the firing House takes the target.
#[cfg(feature = "phobos")]
#[test]
fn change_owner_moves_the_target_to_the_firing_house() {
    let (mut sim, rules, firer, target) = scene("", "ChangeOwner=yes\n", "", "", "Russians");
    let americans = sim.interner.get("Americans").expect("house");
    let before = sim.entities().get(target).expect("target").owner();
    assert_ne!(before, americans, "the target starts Russian");
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(
        sim.entities().get(target).expect("target").owner(),
        americans,
        "the firing House owns it"
    );
}

/// `ChangeOwner.SetAsMindControl` refuses a psionically immune target and an
/// already mind-controlled one; a plain `ChangeOwner` ignores both.
#[cfg(feature = "phobos")]
#[test]
fn change_owner_mind_control_respects_immunity() {
    let (mut sim, rules, firer, target) = scene(
        "",
        "ChangeOwner=yes\nChangeOwner.SetAsMindControl=yes\n",
        "ImmuneToPsionics=yes\n",
        "",
        "Russians",
    );
    let russians = sim.interner.get("Russians").expect("house");
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(
        sim.entities().get(target).expect("target").owner(),
        russians,
        "a mind-control capture refuses psionic immunity"
    );

    let (mut sim, rules, firer, target) = scene(
        "",
        "ChangeOwner=yes\n",
        "ImmuneToPsionics=yes\n",
        "",
        "Russians",
    );
    let americans = sim.interner.get("Americans").expect("house");
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    assert_eq!(
        sim.entities().get(target).expect("target").owner(),
        americans,
        "a plain owner change ignores psionic immunity"
    );
}

/// A `ChangeOwner.SetAsMindControl` capture writes `MindControlledByAUnit`
/// (Techno+0x2C4, VERA's `MindControlLink::permanent`).
#[cfg(feature = "phobos")]
#[test]
fn change_owner_mind_control_marks_the_capture() {
    let (mut sim, rules, firer, target) = scene(
        "",
        "ChangeOwner=yes\nChangeOwner.SetAsMindControl=yes\n",
        "",
        "",
        "Russians",
    );
    let americans = sim.interner.get("Americans").expect("house");
    shoot(&mut sim, &rules, "WH", firer, target, 0);
    let entity = sim.entities().get(target).expect("target");
    assert_eq!(entity.owner(), americans, "the firing House owns it");
    assert!(
        entity.mind_control.is_mind_controlled(),
        "the capture counts as mind-controlled"
    );
    assert!(
        entity.mind_control.permanent(),
        "MindControlledByAUnit is +0x2C4"
    );
}

/// `ApplyCrit`'s `IsTechnoEligible` gate (`Detonate.cpp:562`,
/// `EnumFunctions.cpp:59-94` with `considerAircraftSeparately=false`): a
/// spreading always-crit warhead crits only the classes its
/// `Crit.AffectsTarget` lists; `all` and the unset default admit every class.
#[test]
fn crit_affects_target_filter_admits_only_the_listed_classes() {
    // (unit, infantry, building) crit damage of one detonation at the unit.
    fn victims(wh_ini: &str) -> [i32; 3] {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(&format!(
                "[Countries]\n0=Americans\n1=Russians\n\
                 [VehicleTypes]\n0=FIRER\n1=TANK\n\
                 [InfantryTypes]\n0=GI\n\
                 [BuildingTypes]\n0=BLDG\n\
                 [GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
                 [Warheads]\n0=WH\n\
                 [WH]\nCellSpread=3\nCrit.Chance=1.0\nCrit.ExtraDamage=50\n{wh_ini}\
                 [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
                 [TANK]\nStrength=500\nArmor=none\nSpeed=6\n\
                 [GI]\nStrength=500\nArmor=none\nSpeed=4\n\
                 [BLDG]\nStrength=500\nArmor=none\n"
            )),
            &IniFile::from_str("[BLDG]\nFoundation=1x1\n"),
        )
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let firer = sim
            .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
            .expect("firer");
        let tank = sim
            .spawn_object("TANK", "Russians", 22, 20, 0, &rules)
            .expect("unit");
        let gi = sim
            .spawn_object("GI", "Russians", 23, 20, 0, &rules)
            .expect("infantry");
        let bldg = sim
            .spawn_object("BLDG", "Russians", 24, 20, 0, &rules)
            .expect("building");
        shoot(&mut sim, &rules, "WH", firer, tank, 0);
        [
            500 - health(&sim, tank),
            500 - health(&sim, gi),
            500 - health(&sim, bldg),
        ]
    }
    assert_eq!(
        victims("Crit.AffectsTarget=units\n"),
        [50, 0, 0],
        "units admits the vehicle and refuses infantry and buildings"
    );
    assert_eq!(victims("Crit.AffectsTarget=infantry\n"), [0, 50, 0]);
    assert_eq!(victims("Crit.AffectsTarget=buildings\n"), [0, 0, 50]);
    assert_eq!(
        victims("Crit.AffectsTarget=all\n"),
        [50, 50, 50],
        "all admits every class"
    );
    assert_eq!(victims(""), [50, 50, 50], "the unset default is All");
}

/// `ApplyCrit`'s `CanTargetHouse` gate (`Detonate.cpp:556`,
/// `EnumFunctions.cpp:5-16`): the owner, ally and enemy buckets, and the
/// deprecated `Crit.AffectsHouses`/`Crit.Affects` aliases, which the current
/// keys overwrite in reader order regardless of INI order
/// (`WarheadType/Body.cpp:200-211`).
#[test]
fn crit_affects_house_buckets_and_alias_precedence() {
    let crit_damage = |wh_ini: &str, target_house: &str, allied: bool| {
        let (mut sim, rules, firer, target) = scene(
            "",
            &format!("Crit.Chance=1.0\nCrit.ExtraDamage=50\n{wh_ini}"),
            "",
            "",
            target_house,
        );
        if allied {
            for (house, ally) in [("AMERICANS", "RUSSIANS"), ("RUSSIANS", "AMERICANS")] {
                sim.house_alliances_mut_for_test()
                    .entry(house.to_owned())
                    .or_default()
                    .insert(ally.to_owned());
            }
        }
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        500 - health(&sim, target)
    };
    assert_eq!(
        crit_damage("Crit.AffectsHouse=enemies\n", "Russians", false),
        50,
        "enemies admits the enemy target"
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouse=enemies\n", "Americans", false),
        0,
        "enemies refuses the firing house's own unit"
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouse=owner\n", "Americans", false),
        50
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouse=owner\n", "Russians", false),
        0
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouse=enemies\n", "Russians", true),
        0,
        "an allied target leaves the enemy bucket"
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouse=allies\n", "Russians", true),
        50,
        "allies admits the allied target"
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouse=allies\n", "Russians", false),
        0,
        "without an alliance the target stays an enemy"
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouses=enemies\n", "Russians", false),
        50,
        "the deprecated alias alone maps to the same field"
    );
    assert_eq!(
        crit_damage("Crit.AffectsHouses=owner\n", "Russians", false),
        0
    );
    assert_eq!(
        crit_damage(
            "Crit.AffectsHouses=owner\nCrit.AffectsHouse=enemies\n",
            "Russians",
            false
        ),
        50,
        "the current key overwrites the alias"
    );
    assert_eq!(
        crit_damage(
            "Crit.AffectsHouse=enemies\nCrit.AffectsHouses=owner\n",
            "Russians",
            false
        ),
        50,
        "reader order wins even when the alias comes later in the INI"
    );
    assert_eq!(
        crit_damage("Crit.Affects=infantry\n", "Russians", false),
        0,
        "the target alias alone refuses the vehicle"
    );
    assert_eq!(
        crit_damage(
            "Crit.Affects=infantry\nCrit.AffectsTarget=units\n",
            "Russians",
            false
        ),
        50,
        "Crit.AffectsTarget overwrites Crit.Affects"
    );
}

/// `ApplyCrit`'s `TechnoExt::IsHealthInThreshold` gate
/// (`Detonate.cpp:553`, `Ext/Techno/Body.cpp:752-759`): the band is
/// `(above, below]` — strict above, inclusive below — and the deprecated
/// percent aliases map to the same bounds with the same overwrite rule
/// (`WarheadType/Body.cpp:217-228`).
#[test]
fn crit_health_band_boundaries_are_open_above_closed_below() {
    let crit_damage = |wh_ini: &str, hp: i32| {
        let (mut sim, rules, firer, target) = scene(
            "",
            &format!("Crit.Chance=1.0\nCrit.ExtraDamage=50\n{wh_ini}"),
            "",
            "",
            "Russians",
        );
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .health
            .set_for_test(hp);
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        hp - health(&sim, target)
    };
    assert_eq!(
        crit_damage("Crit.AffectsAbovePercent=0.5\n", 250),
        0,
        "exactly half health misses the strict above bound"
    );
    assert_eq!(crit_damage("Crit.AffectsAbovePercent=0.5\n", 251), 50);
    assert_eq!(
        crit_damage("Crit.AffectsBelowPercent=0.5\n", 250),
        50,
        "exactly half health satisfies the inclusive below bound"
    );
    assert_eq!(crit_damage("Crit.AffectsBelowPercent=0.5\n", 251), 0);
    assert_eq!(
        crit_damage(
            "Crit.AffectsAbovePercent=0.25\nCrit.AffectsBelowPercent=0.5\n",
            250
        ),
        50,
        "half health sits inside (0.25, 0.5]"
    );
    assert_eq!(
        crit_damage("Crit.AffectAbovePercent=0.5\n", 250),
        0,
        "the deprecated above alias reads the same bound"
    );
    assert_eq!(crit_damage("Crit.AffectBelowPercent=0.5\n", 250), 50);
    assert_eq!(
        crit_damage(
            "Crit.AffectBelowPercent=0.1\nCrit.AffectsBelowPercent=0.5\n",
            250
        ),
        50,
        "the current percent key overwrites the alias"
    );
    assert_eq!(
        crit_damage("", 100),
        50,
        "the unset (0.0, 1.0] band admits every live target"
    );
}

/// A refused target costs no extra Scenario draw and plays no per-target
/// crit animation: the default shared dice is drawn once in the prefix
/// (`Detonate.cpp:161-168`), `ApplyCrit` reads the buffer before its filters
/// (`537-563`), and `Crit.ActiveChanceAnims` plays at the detonation point
/// even when every target is refused, while `Crit.AnimList` under
/// `Crit.AnimOnAffectedTargets` plays only on admitted targets (`567-587`).
#[test]
fn refused_crit_targets_draw_no_dice_and_play_no_target_anim() {
    let outcome = |wh_ini: &str| {
        let (mut sim, mut rules, firer, target) = scene(
            "",
            &format!(
                "Crit.Chance=1.0\nCrit.ExtraDamage=50\n\
                 Crit.ActiveChanceAnims=ONE,TWO\nCrit.AnimList=CRITFX\n\
                 Crit.AnimOnAffectedTargets=yes\n{wh_ini}"
            ),
            "",
            "[Animations]\n0=ONE\n1=TWO\n2=CRITFX\n",
            "Russians",
        );
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[ONE]\nRate=900\n[TWO]\nRate=900\n[CRITFX]\nRate=900\n",
        ));
        for name in ["ONE", "TWO", "CRITFX"] {
            art.bind_anim_frame_count_for_test(name, 6);
        }
        rules.install_art_fixture(art);
        // The two prefix buffers, then the ActiveChanceAnims pick.
        let mut expected = sim.scenario_rng.clone();
        expected.random_double();
        expected.random_double();
        expected.next_range_i32_inclusive(0, 1);
        shoot(&mut sim, &rules, "WH", firer, target, 0);
        let anims: Vec<String> = sim
            .anims()
            .map(|(_, anim)| sim.resolve(anim.type_id()).to_owned())
            .collect();
        (
            500 - health(&sim, target),
            sim.scenario_rng.logical_state() == expected.logical_state(),
            anims,
        )
    };
    let (damage, stream_unchanged, anims) = outcome("Crit.AffectsTarget=infantry\n");
    assert_eq!(damage, 0, "the class filter refuses the vehicle");
    assert!(
        stream_unchanged,
        "a refused target reads the shared buffer without drawing"
    );
    assert!(
        !anims.iter().any(|name| name == "CRITFX"),
        "no per-target crit animation on a refused target"
    );
    assert_eq!(
        anims
            .iter()
            .filter(|name| *name == "ONE" || *name == "TWO")
            .count(),
        1,
        "the detonation-point chance anim plays regardless of admission"
    );
    let (damage, stream_unchanged, anims) = outcome("");
    assert_eq!(damage, 50, "an admitted target keeps the crit behavior");
    assert!(stream_unchanged, "the shared dice draws no per-target roll");
    assert!(
        anims.iter().any(|name| name == "CRITFX"),
        "the admitted target plays the crit animation"
    );
}
