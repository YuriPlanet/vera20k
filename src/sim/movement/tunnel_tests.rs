//! Tunnel locomotor regressions through the Unit setter (`0x00741970`) and
//! Process (`0x00728E30`), on a mod-style type: no retail type selects the
//! class.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::movement::tunnel_movement::{TUNNEL_DEPTH_Z, TunnelState};
use crate::sim::world::{FrameEffects, Simulation};

const DIGGER: u64 = 1;

fn fixture() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTunnelSpeed=1\n[AudioVisual]\nDigSound=DigSnd\n\
         [VehicleTypes]\n0=DIGR\n[DIGR]\nStrength=300\nSpeed=6\nROT=8\n\
         MovementZone=Normal\nLocomotor={4A582743-9839-11D1-B709-00A024DDAFD1}\n",
    ))
    .expect("Tunnel fixture rules");
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("DIGR");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        DIGGER,
        10,
        10,
        0,
        0,
        owner,
        Health::new(300),
        type_id,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    entity.locomotor = Some(LocomotorState::from_object_type(
        rules.object("DIGR").expect("DIGR type"),
        0,
    ));
    entity.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(entity);
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    (sim, rules)
}

fn order(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) {
    sim.set_unit_destination(
        DIGGER,
        NavTargetRef::cell(cell.0, cell.1),
        rules,
        true,
        FrameEffects::default(),
    );
}

fn tunnel_state(sim: &Simulation) -> Option<TunnelState> {
    sim.substrate
        .entities
        .get(DIGGER)?
        .locomotor
        .as_ref()?
        .tunnel_runtime()
        .map(|r| r.state())
}

/// A trip of 12 or more cells keeps the Tunnel (`0x00746000`): the digger
/// turns and digs in, leaves its cell list at Z -256, travels below the
/// cells, and surfaces on the destination, back in its cell.
#[test]
fn long_trip_digs_travels_underground_and_surfaces() {
    let (mut sim, rules) = fixture();
    order(&mut sim, &rules, (30, 10));
    assert_eq!(tunnel_state(&sim), Some(TunnelState::PreDigIn));

    let mut underground_frames = 0;
    for frame in 1..3000 {
        sim.session.set_binary_frame_for_test(frame);
        sim.process_tunnel_locomotor(DIGGER, &rules, None, FrameEffects::default());
        let entity = sim.substrate.entities.get(DIGGER).unwrap();
        if entity.movement_layer_or_ground() == MovementLayer::Underground {
            underground_frames += 1;
            let location =
                super::ground_pose::object_location(entity, sim.resolved_terrain.as_ref());
            assert_eq!(location.z, TUNNEL_DEPTH_Z);
            assert!(
                !entity.lifecycle.cell_marked(),
                "underground leaves the cell"
            );
            assert_eq!(
                crate::sim::world::entity_layer(entity, sim.resolved_terrain.as_ref(), None),
                crate::sim::world::display_layers::DisplayLayer::UNDERGROUND
            );
        }
        if tunnel_state(&sim) == Some(TunnelState::Idle) {
            break;
        }
    }
    assert!(underground_frames > 0, "the trip ran below the cells");
    assert_eq!(tunnel_state(&sim), Some(TunnelState::Idle));
    let entity = sim.substrate.entities.get(DIGGER).unwrap();
    let location = super::ground_pose::object_location(entity, sim.resolved_terrain.as_ref());
    assert_eq!((location.x / 256, location.y / 256), (30, 10));
    assert_eq!(
        super::air_movement::current_fly_height(entity, sim.resolved_terrain.as_ref()),
        0
    );
    assert!(entity.lifecycle.cell_marked());
}

/// A destination that stops allowing burrowing while the digger travels:
/// ProcessDigging's own cell refuses it (Tunnel Can_Enter_Cell `0x0072A090`),
/// and its nearby-cell search keeps only burrowable cells (FNPC `0x0056DE5F`),
/// so it surfaces next to the destination instead of re-targeting it.
#[test]
fn blocked_destination_surfaces_on_a_burrowable_neighbour() {
    let (mut sim, rules) = fixture();
    order(&mut sim, &rules, (30, 10));
    let mut frame = 0;
    while tunnel_state(&sim) != Some(TunnelState::Digging) {
        frame += 1;
        sim.session.set_binary_frame_for_test(frame);
        sim.process_tunnel_locomotor(DIGGER, &rules, None, FrameEffects::default());
        assert!(frame < 500, "the digger reaches its underground state");
    }
    // A slope refuses burrowing (CellClass `0x00486FF0`).
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(30, 10)
        .unwrap()
        .slope_type = 1;
    while tunnel_state(&sim) != Some(TunnelState::Idle) {
        frame += 1;
        sim.session.set_binary_frame_for_test(frame);
        sim.process_tunnel_locomotor(DIGGER, &rules, None, FrameEffects::default());
        assert!(frame < 3000, "the digger surfaces");
    }
    let entity = sim.substrate.entities.get(DIGGER).unwrap();
    let location = super::ground_pose::object_location(entity, sim.resolved_terrain.as_ref());
    let cell = (location.x / 256, location.y / 256);
    assert_ne!(cell, (30, 10));
    assert!(
        (cell.0 - 30).abs() <= 1 && (cell.1 - 10).abs() <= 1,
        "{cell:?}"
    );
}

/// A move order reaches the Unit setter through the class-setter dispatcher
/// (vt+0x480 = `UnitClass::Assign_Destination @ 0x00741970`), so a Tunnel
/// Unit starts digging instead of keeping a NavCom nothing drives toward.
#[test]
fn dispatched_order_reaches_the_unit_setter() {
    let (mut sim, rules) = fixture();
    sim.assign_destination_represented(
        DIGGER,
        Some(NavTargetRef::cell(30, 10)),
        &rules,
        None,
        FrameEffects::default(),
    )
    .expect("dispatched destination");
    assert_eq!(tunnel_state(&sim), Some(TunnelState::PreDigIn));
}

/// A short trip with a cheap ground route drives: the Unit setter's Tunnel
/// arm (`0x007427C0..0x00742C14`) puts a Drive over the Tunnel.
#[test]
fn short_trip_drives_over_the_tunnel() {
    let (mut sim, rules) = fixture();
    order(&mut sim, &rules, (13, 10));
    let locomotor = sim
        .substrate
        .entities
        .get(DIGGER)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    assert_eq!(locomotor.active_kind(), LocomotorKind::Drive);
    assert_eq!(locomotor.effective_kind(), LocomotorKind::Tunnel);
}

/// While it travels underground the digger answers the underground layer,
/// Ripple to the screen and Hidden to a sensor query
/// (`0x007291D0`), and its weapons answer FIRE_MOVING (`0x0072A1C0`)
/// from the first dig-in step.
#[test]
fn underground_digger_is_ripple_on_screen_and_hidden_to_sensors() {
    let (mut sim, rules) = fixture();
    order(&mut sim, &rules, (30, 10));
    let entity = sim.substrate.entities.get_mut(DIGGER).unwrap();
    let runtime = entity
        .locomotor
        .as_mut()
        .unwrap()
        .tunnel_runtime_mut()
        .unwrap();
    assert!(runtime.blocks_fire());
    runtime.set_state(TunnelState::Digging);
    let entity = sim.substrate.entities.get(DIGGER).unwrap();
    let screen = crate::sim::cloak_disguise::VisualCharacterQuery::screen(
        true, false, false, false, false, false,
    );
    let sensor = crate::sim::cloak_disguise::VisualCharacterQuery::sensor(true, false, false);
    assert_eq!(entity.visual_character(0, false, false, screen), 4);
    assert_eq!(entity.visual_character(0, false, false, sensor), 5);
    assert_eq!(
        entity.movement_layer_or_ground(),
        MovementLayer::Underground
    );
}

/// Ares `PlayDigEffects` (Ares.dll `0x100443E0`): a type's `DigIn` replaces
/// `[AudioVisual] Dig=` while digging in, `DigOut=none` spawns nothing while
/// digging out, and an unset `DigInSound`/`DigOutSound` keeps `DigSound`.
#[test]
fn ares_dig_keys_replace_the_audio_visual_effects() {
    use crate::sim::movement::tunnel_movement::DigEffects;
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nDigSound=DigSnd\nDig=DIGANIM\n[Animations]\n0=DIGANIM\n1=MYDIG\n\
         [VehicleTypes]\n0=DIGR\n[DIGR]\nDigIn=MYDIG\nDigOut=none\n",
    ))
    .expect("Ares dig fixture rules");
    let (sim, _) = fixture();
    let entity = sim.substrate.entities.get(DIGGER).unwrap();
    let names = |effects| sim.tunnel_dig_effect_types(entity, effects, &rules);
    let dig_sound = rules.general.dig_sound.clone();
    assert_eq!(
        names(DigEffects::DIG_IN),
        (dig_sound.clone(), Some("MYDIG".into()))
    );
    assert_eq!(names(DigEffects::DIG_OUT), (dig_sound, None));
}

/// Ares TunnelLocomotionClass_Turning (Ares.dll `0x1005D500`, hook
/// `0x0072920C`): PreDigIn with a null Destination ends the dig (DugOut)
/// instead of turning toward the map origin.
#[test]
fn ares_null_destination_ends_the_dig() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let (mut sim, rules) = fixture();
    sim.substrate
        .entities
        .get_mut(DIGGER)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .tunnel_runtime_mut()
        .unwrap()
        .set_state(TunnelState::PreDigIn);
    sim.process_tunnel_locomotor(DIGGER, &rules, None, FrameEffects::default());
    assert_eq!(tunnel_state(&sim), Some(TunnelState::DugOut));
}
