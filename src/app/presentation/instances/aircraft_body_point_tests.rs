use super::*;
use crate::sim::command::{Command, CommandEnvelope};

/// Original Fly4CF830, composed by Aircraft414613 and Foot4DAF6D.
#[test]
fn retail_harrier_move_bobs_only_its_body_at_the_committed_draw_frame() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 17, 128);
    let owner = sim.intern("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 64,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    }));
    sim.set_playfield_size_height_for_test(Some(64));
    sim.set_input_delay_ticks(0);
    for (kind, house, cell) in [
        ("GAPOWR", "Americans", (40, 48)),
        ("NAPOWR", "Russians", (90, 80)),
    ] {
        sim.spawn_object(kind, house, cell.0, cell.1, 0, &rules)
            .unwrap();
    }
    let plane = sim
        .spawn_object("ORCA", "Americans", 50, 64, 0, &rules)
        .unwrap();
    assert!(!rules.object("ORCA").unwrap().is_dropship());
    let mut commands = vec![CommandEnvelope::new(
        owner,
        sim.session().tick(),
        Command::Move {
            entity_id: plane,
            target_rx: 100,
            target_ry: 64,
        },
    )];
    // Executed original Draw_Point values; the body adds each integer twice.
    let native_y = [0, 0, 1, 1, 1, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0, -1, 0, 0, 0, 0];
    let mut saw_low_bob = false;
    let mut phases = [false; 3];
    for _ in 0..160 {
        let grid = sim.path_grid_snapshot();
        sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
        commands = sim.take_due_commands();
        let entity = sim
            .entities()
            .get(plane)
            .expect("the moving Harrier stays alive");
        let (taking_off, landing) = entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_phase_callbacks()
            .unwrap();
        let cells = sim
            .resolved_terrain()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        let height =
            crate::sim::movement::air_movement::queried_flight_height(entity, cells.as_ref());
        assert_eq!(
            crate::sim::movement::air_movement::aircraft_landing_base(
                entity,
                sim.entities(),
                Some((&rules, sim.interner())),
            ),
            0
        );
        let phase =
            display_binary_frame_for_committed_session(sim.session().binary_frame()) as usize % 20;
        let expected = if !taking_off && !landing && height > 0 {
            2 * native_y[phase]
        } else {
            0
        };
        let (x, y) = crate::render::locomotor_visual::screen_position(entity);
        let anchor = [x + 0.25, y - 0.5];
        let hash = sim.state_hash();
        // Synthetic atlas metadata isolates the production CPU submission;
        // both page routes must place the body, its bounds and depth together.
        let entry = UnitSpriteEntry {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [16.0, 12.0],
            offset_x: -5.0,
            offset_y: -7.0,
            page: 3,
            native_draw_bounds: Some([-5, -7, 16, 12]),
        };
        let axis = crate::render::batch::DepthAxis {
            origin_y: -100.0,
            world_height: 5000.0,
        };
        for (source, depth_row_offset) in [
            (UnitTextureSource::Stable(3), 0.0),
            (UnitTextureSource::Pose(3), 0.0),
            (UnitTextureSource::Stable(3), TILE_HEIGHT),
        ] {
            let piece = aircraft_body_piece(
                &sim,
                entity,
                Some(&rules),
                cells.as_ref(),
                entry,
                source,
                anchor,
                axis,
                depth_row_offset,
                entity.position.z(),
                [1.0; 3],
                crate::render::palette_light::PaletteLight::default(),
                DrawState::default(),
                0.0,
            );
            let top = anchor[1] + expected as f32 + entry.offset_y;
            assert_eq!(
                piece.instance.position,
                [anchor[0] + entry.offset_x, top],
                "phase{phase}, height{height}"
            );
            assert_eq!(piece.instance.zshape_origin, [top, entry.pixel_size[1]]);
            assert_eq!(
                piece.instance.depth,
                super::super::helpers::compute_sprite_depth_params(
                    axis.origin_y,
                    axis.world_height,
                    ground_sort_row(entity, top + entry.pixel_size[1] + depth_row_offset),
                    entity.position.z(),
                )
            );
            assert_eq!(
                piece.target,
                match source {
                    UnitTextureSource::Stable(page) => ObjectTexture::UnitAtlasPage(page),
                    UnitTextureSource::Pose(page) => ObjectTexture::UnitPosePage(page),
                }
            );
        }
        assert_eq!(
            crate::render::locomotor_visual::screen_position(entity),
            (x, y)
        );
        assert_eq!(
            sim.state_hash(),
            hash,
            "body placement leaves retained simulation state unchanged"
        );
        if expected != 0 {
            saw_low_bob |= height < 208;
            for (seen, wanted) in phases.iter_mut().zip([2, 5, 15]) {
                *seen |= phase == wanted;
            }
        }
        if saw_low_bob && phases.into_iter().all(|seen| seen) {
            return;
        }
    }
    panic!("ordinary Move must reach low-flight bob and the positive/negative native body phases");
}
