//! Phobos `FootExt::UpdateTypeData`'s in-air arm
//! (`Ext/Foot/Body.cpp:722-793`), reached through the production conversion
//! owner (`Simulation::convert_techno_type`, `Ext/Techno/Body.cpp:455`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::DeliveryType;
use crate::sim::house_state::HouseState;
use crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_HOLD;
use crate::sim::world::{FrameEffects, Simulation};

fn rules(a_extra: &str, b_extra: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=A\n1=B\n\
         [A]\nStrength=100\nSpeed=4\nLocomotor=jumpjet\n{a_extra}\
         [B]\nStrength=100\nSpeed=4\nLocomotor=jumpjet\n{b_extra}"
    )))
    .expect("type-change rules")
}

fn world(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    let owner = sim.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    let id = sim
        .spawn_object("A", "Americans", 10, 10, 0, rules)
        .expect("A spawns");
    (sim, id)
}

fn airborne(sim: &mut Simulation, id: u64, z: i32) {
    sim.entities_mut_for_test()
        .get_mut(id)
        .expect("spawned body")
        .position
        .set_exact_z_leptons_for_test(Some(z));
}

fn convert(sim: &mut Simulation, id: u64, name: &str, rules: &RuleSet) {
    sim.convert_techno_type(
        id,
        &DeliveryType {
            category: crate::rules::object_type::ObjectCategory::Vehicle,
            id: name.to_owned(),
        },
        rules,
        FrameEffects::default(),
    );
}

/// `:733-746`: an airborne Jumpjet keeps running and takes the new type's
/// `Jumpjet*` block; `JumpjetTurnRate` clamps to 127 (`:733`).
#[test]
fn an_airborne_jumpjet_takes_the_new_types_flight_block() {
    let rules = rules(
        "JumpjetSpeed=5\nJumpjetHeight=500\n",
        "JumpjetSpeed=9\nJumpjetHeight=800\nJumpjetTurnRate=200\n",
    );
    let (mut sim, id) = world(&rules);
    airborne(&mut sim, id, 600);
    convert(&mut sim, id, "B", &rules);
    let runtime = sim
        .entities()
        .get(id)
        .expect("converted body")
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.jumpjet_runtime())
        .expect("the conversion keeps the Jumpjet locomotor");
    assert_eq!(runtime.params().speed(), 9, "the new JumpjetSpeed=");
    assert_eq!(runtime.params().height(), 800, "the new JumpjetHeight=");
    assert_eq!(
        runtime.params().turn_rate(),
        127,
        "the hook clamps JumpjetTurnRate to 127"
    );
}

/// `:752-759`: an airborne `BalloonHover` type is parked in `State::Hovering`
/// with the moving byte raised, without a destination request.
#[test]
fn an_airborne_balloon_hover_jumpjet_is_parked_in_place() {
    let rules = rules(
        "JumpjetSpeed=5\n",
        "JumpjetSpeed=9\nBalloonHover=yes\nDeployToLand=yes\n",
    );
    let (mut sim, id) = world(&rules);
    airborne(&mut sim, id, 600);
    convert(&mut sim, id, "B", &rules);
    let runtime = sim
        .entities()
        .get(id)
        .expect("converted body")
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.jumpjet_runtime())
        .expect("the conversion keeps the Jumpjet locomotor");
    assert_eq!(runtime.phase(), STATE_HOLD, "State::Hovering");
    assert!(runtime.moving(), "the emitting byte is raised");
}

/// `:767-785`: an airborne body whose new type has no Jumpjet is let go, and a
/// clear cell below turns the fall into a parachute landing rather than a
/// crash death.
#[test]
fn an_airborne_body_without_a_jumpjet_falls_with_on_parachuted() {
    // A must be airborne before the conversion, so both types run Drive.
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=A\n1=B\n\
         [A]\nStrength=100\nSpeed=4\nLocomotor=drive\n\
         [B]\nStrength=100\nSpeed=4\nLocomotor=drive\n",
    ))
    .expect("drive rules");
    let (mut sim, id) = world(&rules);
    airborne(&mut sim, id, 600);
    convert(&mut sim, id, "B", &rules);
    let entity = sim.entities().get(id).expect("converted body");
    assert!(entity.is_falling_down(), "the body is let go");
    assert!(
        entity.on_parachuted(),
        "a clear cell below lands it as a parachute"
    );
    assert!(!entity.was_dropped_as_bomb());
}

/// `:774-779`: a landing cell the new type's `MovementZone` cannot pass turns
/// the fall into a crash death.
#[test]
fn a_blocked_landing_cell_turns_the_fall_into_a_crash_death() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=A\n1=B\n\
         [A]\nStrength=100\nSpeed=4\nLocomotor=drive\n\
         [B]\nStrength=100\nSpeed=4\nLocomotor=drive\n",
    ))
    .expect("drive rules");
    let (mut sim, id) = world(&rules);
    airborne(&mut sim, id, 600);
    // Reduced ZoneType 4 is the Water column, which a Normal mover never
    // passes, so `IsClearToMove` refuses the cell.
    sim.resolved_terrain
        .as_mut()
        .expect("scene grid")
        .cell_mut_for_test(10, 10)
        .expect("the spawn cell")
        .zone_type = 4;
    convert(&mut sim, id, "B", &rules);
    let entity = sim.entities().get(id).expect("converted body");
    assert!(entity.is_falling_down(), "the body is let go");
    assert!(
        entity.was_dropped_as_bomb(),
        "a refused landing cell grants the crash death"
    );
}

/// The arm is gated on `IsInAir`: a grounded body converting types stays
/// grounded and does not fall.
#[test]
fn a_grounded_body_converting_types_does_not_fall() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=A\n1=B\n\
         [A]\nStrength=100\nSpeed=4\nLocomotor=drive\n\
         [B]\nStrength=100\nSpeed=4\nLocomotor=drive\n",
    ))
    .expect("drive rules");
    let (mut sim, id) = world(&rules);
    convert(&mut sim, id, "B", &rules);
    let entity = sim.entities().get(id).expect("converted body");
    assert!(!entity.is_falling_down());
}
