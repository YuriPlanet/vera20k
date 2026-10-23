//! The crush loop of `UnitClass::Crush_Or_Prescatter_Cell @ 0x007416A0`,
//! run through `Simulation::per_cell_process` on retail rules.
use super::*;
use crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art;
use crate::sim::occupancy::OccupancyGrid;

/// Stock Attack and Move orders reach capture and crushing. The authored map
/// shares a recurring destroyed Tag between the controller and its captive:
/// Action14 gives both to Russians. Retail releases captures in victim +170,
/// before RecordKill raises event7, so that action takes the released captive
/// back. A release deferred until UnInit instead leaves it American.
#[test]
fn retail_crush_releases_captives_before_destroyed_tag_changes_house() {
    use crate::sim::command::{Command, CommandEnvelope};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let mut rules = retail.rules;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    let mut sim = crate::sim::world::Simulation::with_seed(0x7418E5);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    sim.set_input_delay_ticks(0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let original_owner = sim.interner.intern("Americans");
    let controller_owner = sim.interner.intern("Russians");
    // Two human skirmish players issue their own ordinary orders.
    for owner in [original_owner, controller_owner] {
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner,
                0,
                Some(owner),
                true,
                0,
                10,
            ));
    }
    let controller = sim
        .spawn_object("YURI", "Russians", 14, 16, 0, &rules)
        .unwrap();
    let captive = sim
        .spawn_object("MTNK", "Americans", 14, 12, 0, &rules)
        .unwrap();
    // Keep the player's House active while its tank is controlled.
    sim.spawn_object("E1", "Americans", 2, 2, 0, &rules)
        .unwrap();
    sim.queue_command(CommandEnvelope::new(
        controller_owner,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: controller,
            target_id: captive,
        },
    ));
    for _ in 0..180 {
        let commands = sim.take_due_commands();
        let grid = sim.path_grid_snapshot().unwrap();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if sim
            .substrate
            .entities
            .get(captive)
            .unwrap()
            .mind_control
            .controller()
            == Some(controller)
        {
            break;
        }
    }
    let captured = sim.substrate.entities.get(captive).unwrap();
    assert_eq!(
        captured.mind_control.controller(),
        Some(controller),
        "ordinary weapon captures; controller={:?}, captive={:?}",
        sim.substrate.entities.get(controller).map(|entity| (
            entity.lifecycle.object_alive(),
            entity.health.current(),
            entity.mission.current(),
            entity.attack_target.target(),
            entity.position
        )),
        sim.substrate.entities.get(captive).map(|entity| (
            entity.lifecycle.object_alive(),
            entity.health.current(),
            entity.mission.current(),
            entity.position
        ))
    );
    assert_eq!(captured.owner(), controller_owner);
    let manager = sim
        .substrate
        .entities
        .get(controller)
        .unwrap()
        .capture_manager
        .as_ref()
        .unwrap();
    assert_eq!(manager.victims().collect::<Vec<_>>(), [captive]);
    assert_eq!(manager.original_owner(captive), Some(original_owner));
    let ring = sim
        .substrate
        .anims
        .iter()
        .find_map(|(id, anim)| {
            (anim.owner_entity() == Some(captive)
                && sim.interner.resolve(anim.type_id())
                    == rules.mind_control.controlled_anim.as_deref().unwrap())
            .then_some(*id)
        })
        .expect("ordinary capture attaches its controlled animation");

    // Spawn after capture so an idle fortress cannot shoot the controller
    // during setup; its first frame receives the Move through the controller.
    let crusher = sim
        .spawn_object("BFRT", "Americans", 10, 16, 64, &rules)
        .unwrap();

    // This is an explicit map control over physical retail actors/rules, not
    // a claim that a stock map already authors this Tag association.
    let country = rules.trigger_house_type_index("Russians").unwrap().0;
    assert_eq!(
        sim.house_by_trigger_selector(i32::from(country), &rules),
        Some(controller_owner)
    );
    let map = crate::map::map_file::MapFile::from_bytes(
        format!(
            "[Map]\nSize=0,0,32,32\nLocalSize=0,0,32,32\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Triggers]\nKILLED=Russians,<none>,Killed,0,1,1,1,0\n\
          [Tags]\nTAG=2,Killed,KILLED\n\
          [Events]\nKILLED=1,7,0,0\n\
          [Actions]\nKILLED=1,14,0,{country},0,0,0,0,A\n",
        )
        .as_bytes(),
    )
    .unwrap();
    sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
    sim.classify_map_triggers(&rules);
    sim.attach_object_trigger_tag(controller, Some("TAG"));
    sim.attach_object_trigger_tag(captive, Some("TAG"));
    assert!(sim.object_has_trigger_tag(controller));
    assert!(sim.object_has_trigger_tag(captive));
    sim.queue_command(CommandEnvelope::new(
        original_owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: crusher,
            target_rx: 18,
            target_ry: 16,
        },
    ));
    for _ in 0..1200 {
        let commands = sim.take_due_commands();
        let grid = sim.path_grid_snapshot().unwrap();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if sim
            .substrate
            .entities
            .get(controller)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
        {
            assert!(sim.sound_events().iter().any(|event| matches!(event,
                crate::sim::world::SimSoundEvent::EntityCrushed { crush_sound_id, .. }
                    if sim.interner.resolve(*crush_sound_id) == rules.object("YURI").unwrap().crush_sound.as_deref().unwrap())),
                "the controller dies in the live crush loop, rather than gunfire");
            assert!(
                sim.substrate
                    .entities
                    .get(crusher)
                    .unwrap()
                    .veterancy
                    .raw()
                    .bits()
                    > 0,
                "the fortress receives kill credit"
            );
            if let Some(controller) = sim.substrate.entities.get(controller) {
                assert!(
                    controller.health.current() > 0,
                    "crushing retires without a fatal HP store"
                );
                assert_eq!(controller.capture_manager.as_ref().unwrap().count(), 0);
            }
            assert!(
                sim.anim(ring).is_none(),
                "the controlled animation was released"
            );
            let released = sim.substrate.entities.get(captive).unwrap();
            assert_eq!(released.mind_control.controller(), None);
            assert_eq!(
                released.owner(),
                controller_owner,
                "destroyed Tag changes the already released captive back to Russians"
            );
            return;
        }
    }
    panic!(
        "ordinary Move must crush the hostile controller: crusher={:?}, controller={:?}",
        sim.substrate.entities.get(crusher).map(|entity| (
            &entity.position,
            entity.owner(),
            entity.mission.current()
        )),
        sim.substrate.entities.get(controller).map(|entity| (
            &entity.position,
            entity.owner(),
            entity.mission.current()
        ))
    );
}

/// A Battle Fortress driven onto a Grizzly crushes it, and the victim's
/// vtable `+0x170` (`0x007418E5` -> `0x00746D60`) runs Death_Explosion
/// (`0x00738680`): one Scenario pick from `Explosion=`, its anim at the
/// tank's Location (stock vehicles author no `DestroyAnim=`), before the
/// kill is recorded and the tank is UnInit. Having crushed a Unit, the
/// level fortress then tilts forwards.
#[test]
fn a_battle_fortress_crushing_a_tank_plays_its_death_explosion() {
    let Some(mut rules) = retail_unlayered_rules_with_art() else {
        return;
    };
    // Asset binding gives each anim type its SHP frame count; the count is
    // not under test.
    for name in rules.object("MTNK").unwrap().explosion_anims.clone() {
        rules.bind_anim_frame_count_for_test(&name, 20);
    }
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7418E5);
    let fortress = sim
        .spawn_object("BFRT", "Russians", 10, 10, 0, &rules)
        .expect("BFRT spawns");
    let tank = sim
        .spawn_object("MTNK", "Americans", 12, 10, 0, &rules)
        .expect("MTNK spawns");
    // The tank stands a little off the fortress's centre, inside the crush
    // distance, so its Location is not the crusher's.
    let mut position = sim.substrate.entities.get(fortress).unwrap().position;
    let centre = position_world_coord(&position);
    position.put_world_xy([centre.x + 40, centre.y - 24]);
    sim.substrate.entities.get_mut(tank).unwrap().position = position;
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    let location = position_world_coord(&position);
    let object = rules.object("MTNK").unwrap();
    let mut replay = sim.scenario_rng.clone();
    assert!(object.destroy_anims.is_empty());
    let explosion = &object.explosion_anims;
    let expected = explosion[(replay.next_u32() % explosion.len() as u32) as usize].clone();
    assert!(sim.substrate.anims.iter().next().is_none());
    sim.per_cell_process(
        fortress,
        PerCellReason::Arrival,
        &rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .is_none_or(|tank| !tank.lifecycle.object_alive()),
        "crushed"
    );
    let played: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| {
            let coord = anim.world_coord();
            (
                sim.interner.resolve(anim.type_id()).to_string(),
                (coord.x, coord.y, coord.z),
            )
        })
        .collect();
    assert_eq!(played, [(expected, (location.x, location.y, location.z))]);
    assert_eq!(sim.scenario_rng.state(), replay.state());
    // The crush tail (`0x00741939..0x00741954`): the level fortress tilts.
    let rocking = sim
        .substrate
        .entities
        .get(fortress)
        .unwrap()
        .rocking
        .unwrap();
    assert_eq!(rocking.native_bits()[3], 0xbd4c_cccd);
}
