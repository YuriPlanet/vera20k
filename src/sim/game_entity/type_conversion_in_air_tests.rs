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
        &mut FrameEffects::default(),
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

/// `:774-785`: the arm passes `pCell->GetLevel()` as the level, and YRpp folds
/// the deck into it (`Level + (ContainsBridge() ? BridgeLevels : 0)`). Native
/// `0x00483529..0x0048353C` sends both `-1` and `base + 4` to the deck arm,
/// which reads `AltOccupationFlags(+0x128)` and skips the land half
/// (`0x004835D5`), so a conversion over a bridge deck above water lands on the
/// deck instead of taking the crash death the water ground would grant.
#[test]
fn an_airborne_conversion_lands_on_a_bridge_deck_over_water() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=A\n1=B\n\
         [A]\nStrength=100\nSpeed=4\nLocomotor=drive\n\
         [B]\nStrength=100\nSpeed=4\nLocomotor=drive\n",
    ))
    .expect("drive rules");
    for occupied_deck in [false, true] {
        let (mut sim, id) = world(&rules);
        airborne(&mut sim, id, 600);
        {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .expect("scene grid")
                .cell_mut_for_test(10, 10)
                .expect("the spawn cell");
            // Reduced ZoneType 4 is the Water column: the ground plane alone
            // would refuse the landing, the deck plane admits it.
            cell.zone_type = 4;
            cell.bridge_facts.raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        }
        if occupied_deck {
            // A structure is the one bit both ignore masks leave.
            sim.substrate.raw_cell_occupation.mark_deck(10, 10, 0x40);
        }
        convert(&mut sim, id, "B", &rules);
        let entity = sim.entities().get(id).expect("converted body");
        assert!(entity.is_falling_down(), "the body is let go");
        if occupied_deck {
            assert!(
                entity.was_dropped_as_bomb(),
                "an occupied deck refuses the landing"
            );
        } else {
            assert!(
                !entity.was_dropped_as_bomb(),
                "the deck plane admits the landing over the water ground"
            );
            assert!(entity.on_parachuted());
            assert!(
                entity.on_bridge(),
                "OnBridge copies the cell's ContainsBridge()"
            );
        }
    }
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

/// `:787-791`: an Infantry that changed to parachute status plays
/// `Sequence::Paradrop` through the class's own `Do_Action` (`0x0051D6F0`)
/// with `force` set and a deterministic first stage, so both the man's Doing
/// and the pose the SHP pass reads move to the Paradrop record.
#[test]
fn an_airborne_infantry_conversion_force_arms_the_paradrop_sequence() {
    let (mut sim, rules) = infantry_world(true);
    let id = sim
        .spawn_object("ENGINEER", "Americans", 10, 10, 0, &rules)
        .expect("ENGINEER spawns");
    airborne(&mut sim, id, 600);
    assert_eq!(
        sim.entities()
            .get(id)
            .expect("spawned man")
            .infantry_sprite_pose(),
        Some((INFANTRY_SEQUENCE_SENTINEL, 0)),
        "the spawned man keeps the constructor's -1 sequence"
    );
    convert_infantry(&mut sim, id, "PARA", &rules);
    let entity = sim.entities().get(id).expect("converted body");
    assert!(entity.is_falling_down(), "the body is let go");
    assert!(
        entity.on_parachuted(),
        "a clear cell lands it as a parachute"
    );
    assert!(
        !entity.was_dropped_as_bomb(),
        "the Paradrop call runs after both landing arms and does not depend on them"
    );
    assert_eq!(
        entity.infantry_sprite_pose(),
        Some((crate::sim::movement::infantry_action::DO_PARADROP, 0)),
        "the Doing write 0x51D9D2 lands on the Paradrop record the SHP pass reads"
    );
}

/// `0x0051D70F`: the same call refuses when the new type's Paradrop record
/// carries no frames, so the man keeps the Doing he had.
#[test]
fn a_new_type_without_a_paradrop_record_keeps_its_doing() {
    let (mut sim, rules) = infantry_world(false);
    let id = sim
        .spawn_object("ENGINEER", "Americans", 10, 10, 0, &rules)
        .expect("ENGINEER spawns");
    airborne(&mut sim, id, 600);
    convert_infantry(&mut sim, id, "PARA", &rules);
    let entity = sim.entities().get(id).expect("converted body");
    assert!(entity.is_falling_down(), "the body is still let go");
    assert!(entity.on_parachuted());
    assert_eq!(
        entity.infantry_sprite_pose(),
        Some((INFANTRY_SEQUENCE_SENTINEL, 0)),
        "a zero-length Paradrop record refuses at 0x0051D70F"
    );
}

/// `InfantryClass`'s constructor leaves `SequenceAnim` (`+0x6C4`) at -1.
const INFANTRY_SEQUENCE_SENTINEL: i32 = -1;

/// The entry fixture's Infantry pair, `PARA` differing only in its compiled
/// Paradrop record (`Artmd.ini` numbers the action like any other).
fn infantry_world(with_paradrop: bool) -> (Simulation, RuleSet) {
    let paradrop = if with_paradrop {
        "Paradrop=300,6,6\n"
    } else {
        ""
    };
    let art = IniFile::from_str(&format!(
        "[ENGINEER]\nSequence=EngineerSequence\n[EngineerSequence]\nReady=100,6,6\n\
         [PARA]\nSequence=ParaSequence\n[ParaSequence]\nReady=100,6,6\n{paradrop}"
    ));
    let extra = "[InfantryTypes]\n2=PARA\n\
         [PARA]\nStrength=75\nSpeed=4\nSpeedType=Foot\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n";
    let (mut sim, rules, _) =
        crate::sim::world::entry_test_fixture::fixture_with_rules_and_fixed_art(extra, &art);
    let owner = sim.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    (sim, rules)
}

fn convert_infantry(sim: &mut Simulation, id: u64, name: &str, rules: &RuleSet) {
    sim.convert_techno_type(
        id,
        &DeliveryType {
            category: crate::rules::object_type::ObjectCategory::Infantry,
            id: name.to_owned(),
        },
        rules,
        &mut FrameEffects::default(),
    );
}
