//! Stock Magnetron regressions through the normal firing and lift owners.

use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::projectile::ProjectileCoord;

#[test]
fn retail_magnetic_wave_tracks_the_lifted_unit_then_the_live_firer_flh() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = fixture.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x1269);
    sim.input_delay_ticks = 0;
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let grid = sim.path_grid_snapshot().unwrap();
    let source = sim
        .spawn_object("TELE", "Americans", 10, 16, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object("MTNK", "Russians", 16, 16, 0, &rules)
        .unwrap();
    let owner = sim.interner.get("Americans").unwrap();
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: source,
            target_id: target,
        },
    ));
    let mut retained = None;
    let mut observed_lifted_geometry = false;
    for _ in 0..240 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        let Some((&id, wave)) = sim
            .waves
            .iter()
            .find(|(_, wave)| wave.owner_id == Some(source))
        else {
            continue;
        };
        assert_eq!(
            *retained.get_or_insert(id),
            id,
            "one retained Wave per firer"
        );
        assert_eq!(wave.wave_type, 3);
        let held = sim.substrate.entities.get(target).unwrap();
        assert!(
            held.lift.swap_active(),
            "the ordinary shot installs the lift"
        );
        let target_coord = crate::sim::movement::ground_pose::object_get_coords(
            held,
            sim.resolved_terrain.as_ref(),
        );
        // Update_Wave762AF0 reverses the Magnetic arguments for
        // What_Am_I==Unit: its source is the held vehicle, its target the FLH.
        assert_eq!(
            wave.source,
            ProjectileCoord::new(target_coord.x, target_coord.y, target_coord.z),
            "the stock Unit arm reverses the magnetic beam's endpoints",
        );
        let firer = sim.substrate.entities.get(source).unwrap();
        let object = sim.interner.object_type(firer.type_ref(), &rules).unwrap();
        let fire = crate::sim::combat::fire_coord::fire_coordinate(
            &sim,
            &rules,
            &crate::sim::combat::fire_coord::FireSource::of_entity(firer),
            object,
            0,
            0,
            crate::rules::flh::Flh::default(),
        );
        assert_eq!(
            wave.target,
            ProjectileCoord::new(fire.coord.x, fire.coord.y, fire.coord.z + 50),
            "live geometry consumes the canonical GetFLH and the native Z adjustment",
        );
        observed_lifted_geometry |= target_coord.z > 0;
        if wave.type3_cycle_count > 0 {
            assert!(
                observed_lifted_geometry,
                "live geometry follows the lift's height"
            );
            break;
        }
    }
    let id = retained.expect("the stock TELE shot admits its WaveClass");
    assert!(
        sim.waves.get(id).unwrap().type3_cycle_count > 0,
        "active magnetic geometry survives the native phase reset"
    );
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: source,
            target_rx: 10,
            target_ry: 12,
        },
    ));
    let mut saw_decay = false;
    for _ in 0..120 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        let Some(wave) = sim.waves.get(id) else {
            assert!(
                saw_decay,
                "the ordinary Move starts the retained wave's fade"
            );
            assert_eq!(sim.active_wave_id(source), None);
            assert!(
                !sim.display_layers()
                    .members(crate::sim::world::display_layers::DisplayLayer::AIR)
                    .contains(&id)
            );
            assert_eq!(
                sim.substrate.entities.get(source).unwrap().lift.target(),
                None
            );
            return;
        };
        saw_decay |= !wave.active_geometry && wave.decaying;
    }
    panic!("the Move-released Wave did not retire");
}
