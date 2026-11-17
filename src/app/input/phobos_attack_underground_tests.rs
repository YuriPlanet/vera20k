//! Phobos `TechnoClass_MouseOverObject_AttackUnderGround` (`0x70023B`,
//! `Ext/Techno/Hooks.cpp:1497-1511`): the unforced What_Action attack rung
//! over an object that is not surfaced (`IsSurfaced()`) answers Attack only
//! when the selected weapon's projectile has `AU=yes`; otherwise it falls to
//! the tail (`0x70056C`), which over another House's object answers 0.
//!
//! The Tunnel locomotor is not ported, so the fixture sets the cached
//! `MovementLayer::Underground` on the target; that cache is the layer answer
//! the production readers consume (`sim::world::entity_is_underground`).

use super::*;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::world::Simulation;

const VERSES: &str = "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%";

/// Two projectiles that differ only in `AU`; `AA=yes` on both keeps every other
/// selection arm out of the answer so the gate under test is what decides.
fn rules(primary_au: &str, secondary_au: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=SCORER\n1=BURROWED\n[AircraftTypes]\n\
         [BuildingTypes]\n[ArmorTypes]\n[Warheads]\n0=PWH\n1=SWH\n\
         [SCORER]\nStrength=300\nArmor=none\nSpeed=6\nSight=8\nPrimary=PRIMARY\nSecondary=SECONDARY\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [BURROWED]\nStrength=300\nArmor=none\nSpeed=6\nSight=8\nLegalTarget=yes\n\
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

/// A human player's armed unit beside another House's object, the two cells
/// a ground Techno can occupy.
fn scene(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x23B);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let americans = sim.intern("Americans");
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    sim.fog_mut_for_test().reveal_all_for_owner(americans);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(americans));
    let actor = sim
        .spawn_object("SCORER", "Americans", 10, 10, 0, rules)
        .expect("actor");
    let target = sim
        .spawn_object("BURROWED", "Russians", 11, 10, 0, rules)
        .expect("target");
    (sim, actor, target)
}

fn set_layer(sim: &mut Simulation, id: u64, layer: MovementLayer) {
    let entity = sim.entities_mut_for_test().get_mut(id).expect("entity");
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Drive);
    locomotor.set_layer_for_test(layer);
    entity.locomotor = Some(locomotor);
}

fn action(sim: &Simulation, rules: &RuleSet, actor: u64, target: u64) -> AttackAction {
    attack_action(sim, rules, actor, &TargetKind::Entity(target), false, None)
}

#[test]
fn a_weapon_without_au_refuses_the_burrowed_cursor_and_the_surface_twin_admits() {
    let rules = rules("no", "no");
    let (mut sim, actor, target) = scene(&rules);
    set_layer(&mut sim, target, MovementLayer::Underground);
    assert_eq!(
        action(&sim, &rules, actor, target),
        AttackAction::NoMove,
        "a burrowed object with no AU on the selected weapon takes the tail"
    );
    // The same pair on the surface passes every other arm, so the layer is
    // what the gate read.
    set_layer(&mut sim, target, MovementLayer::Ground);
    assert_eq!(
        action(&sim, &rules, actor, target),
        AttackAction::Attack,
        "a surfaced object needs no AU"
    );
}

#[test]
fn au_yes_admits_the_burrowed_cursor() {
    let rules = rules("yes", "no");
    let (mut sim, actor, target) = scene(&rules);
    set_layer(&mut sim, target, MovementLayer::Underground);
    assert_eq!(
        action(&sim, &rules, actor, target),
        AttackAction::Attack,
        "the selected primary's AU admits the burrowed object"
    );
}

#[test]
fn the_gate_reads_the_selected_weapon_not_the_primary() {
    // `0x6F37EB` already answers the `AU=yes` secondary for a burrowed target,
    // so the gate must accept that answer rather than the primary's `AU=no`.
    let rules = rules("no", "yes");
    let (mut sim, actor, target) = scene(&rules);
    set_layer(&mut sim, target, MovementLayer::Underground);
    assert_eq!(
        action(&sim, &rules, actor, target),
        AttackAction::Attack,
        "the secondary the ladder selected carries AU"
    );
}
