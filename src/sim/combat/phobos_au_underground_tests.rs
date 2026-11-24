//! Phobos `[Projectile] AU` admission: the `TechnoClass::CanFire` arm
//! (`Ext/Techno/Hooks.Firing.cpp:535-558`) and the `0x6F37EB` weapon-selection
//! arm (`:167-194`), both against a burrowed target.
//!
//! The Tunnel locomotor is not ported, so the fixtures set the cached
//! `MovementLayer::Underground` on the target's own locomotor; that cache is
//! the layer answer the production readers consume
//! (`sim::world::entity_is_underground`).

use super::TargetKind;
use super::combat_weapon::{
    attacker_facts, get_weapon, techno_target_facts, what_weapon_should_i_use,
};
use super::fire_error::FireError;
use super::fire_error_world::FireSubject;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::world::Simulation;

const VERSES: &str = "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%";

/// Two projectiles that differ only in `AU`; `AA=yes` on both keeps T39's
/// foot arm (`0x006FC73C`) from answering before the AU arm under test.
fn rules(primary_au: &str, secondary_au: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=SCORER\n1=BURROWED\n[AircraftTypes]\n\
         [BuildingTypes]\n[ArmorTypes]\n[Warheads]\n0=PWH\n1=SWH\n\
         [SCORER]\nStrength=300\nArmor=none\nSpeed=6\nSight=8\nPrimary=PRIMARY\nSecondary=SECONDARY\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [BURROWED]\nStrength=300\nArmor=none\nSpeed=6\nSight=8\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [PRIMARY]\nDamage=50\nROF=30\nRange=6\nProjectile=SHOT\nWarhead=PWH\nOmniFire=yes\n\
         [SECONDARY]\nDamage=50\nROF=30\nRange=6\nProjectile=AUSHOT\nWarhead=SWH\nOmniFire=yes\n\
         [SHOT]\nInviso=yes\nAG=yes\nAA=yes\nAU={primary_au}\n\
         [AUSHOT]\nInviso=yes\nAG=yes\nAA=yes\nAU={secondary_au}\n\
         [PWH]\nCellSpread=0\nVersus={VERSES}\n\
         [SWH]\nCellSpread=0\nVersus={VERSES}\n"
    )))
    .expect("AU rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x93);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let firer = sim
        .spawn_object("SCORER", "Russians", 10, 10, 0, rules)
        .expect("firer");
    // A second ground Techno cannot Unlimbo into the firer's cell, so the
    // burrowed unit starts next door; its cached layer, not its cell, is what
    // separates it from a surface unit.
    let target = sim
        .spawn_object("BURROWED", "Americans", 11, 10, 0, rules)
        .expect("target");
    set_layer(&mut sim, target, MovementLayer::Underground);
    (sim, firer, target)
}

fn set_layer(sim: &mut Simulation, id: u64, layer: MovementLayer) {
    let entity = sim.substrate.entities.get_mut(id).expect("entity");
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Drive);
    locomotor.set_layer_for_test(layer);
    entity.locomotor = Some(locomotor);
}

fn fire_error(
    sim: &Simulation,
    rules: &RuleSet,
    firer: u64,
    target: u64,
    weapon_index: i32,
) -> FireError {
    FireSubject {
        world: sim,
        rules,
        fog: None,
        firer: sim.substrate.entities.get(firer).expect("firer"),
        obj: rules.object("SCORER").expect("SCORER"),
        target: Some(TargetKind::Entity(target)),
        weapon_index,
    }
    .fire_error(false)
}

fn selected_weapon(sim: &Simulation, rules: &RuleSet, firer: u64, target: u64) -> i32 {
    let actor = sim.substrate.entities.get(firer).expect("firer");
    let object = rules.object("SCORER").expect("SCORER");
    let target_facts = techno_target_facts(
        sim.substrate.entities.get(target).expect("target"),
        rules.object("BURROWED").expect("BURROWED"),
        sim.resolved_terrain.as_ref(),
        false,
        rules,
        &sim.interner,
    );
    what_weapon_should_i_use(
        rules,
        object,
        &attacker_facts(actor, object),
        Some(&target_facts),
        |slot| {
            get_weapon(
                actor,
                object,
                slot,
                &sim.substrate.entities,
                rules,
                &sim.interner,
            )
        },
    )
}

#[test]
fn a_weapon_without_au_refuses_an_underground_target() {
    // `Ext/Techno/Hooks.Firing.cpp:535-558`: layer Underground answers ILLEGAL
    // unless the projectile has `AU=yes`; the same scene with the key set
    // must pass every other arm.
    for (au, expected) in [("no", FireError::Illegal), ("yes", FireError::Ok)] {
        let rules = rules(au, "no");
        let (mut sim, firer, target) = scene(&rules);
        assert_eq!(
            fire_error(&sim, &rules, firer, target, 0),
            expected,
            "AU={au} decides the underground arm"
        );
        if expected == FireError::Illegal {
            // The same adjacent target on the surface passes every other arm,
            // so the layer is what the gate read.
            set_layer(&mut sim, target, MovementLayer::Ground);
            assert_eq!(
                fire_error(&sim, &rules, firer, target, 0),
                FireError::Ok,
                "a surfaced target needs no AU"
            );
        }
    }
}

#[test]
fn au_yes_makes_the_selection_prefer_the_underground_weapon() {
    // `Ext/Techno/Hooks.Firing.cpp:167-194` (`0x6F37EB`): a burrowed target
    // takes the secondary when only its projectile has `AU=yes`.
    let rules_au_secondary = rules("no", "yes");
    let (sim, firer, target) = scene(&rules_au_secondary);
    let index = selected_weapon(&sim, &rules_au_secondary, firer, target);
    assert_eq!(
        index, 1,
        "the AU=yes secondary replaces a primary without AU"
    );
    assert_eq!(
        fire_error(&sim, &rules_au_secondary, firer, target, index),
        FireError::Ok,
        "the selected AU=yes weapon passes the fire gate"
    );

    // Both projectiles `AU=no`: the ladder keeps the primary and the gate
    // refuses that weapon.
    let rules_no_au = rules("no", "no");
    let (sim, firer, target) = scene(&rules_no_au);
    let index = selected_weapon(&sim, &rules_no_au, firer, target);
    assert_eq!(index, 0, "no arm accepts the burrowed target");
    assert_eq!(
        fire_error(&sim, &rules_no_au, firer, target, index),
        FireError::Illegal,
        "the kept primary has no AU and is refused"
    );
}
