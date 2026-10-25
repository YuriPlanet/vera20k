//! Infantry DrawIt518F90 light boundaries and production SHP forwarding.

use super::*;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{test_grid, test_smudge_cell};
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::Health;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::scenario_session::{ScenarioLightProfileUnits, ScenarioLightingState};
use crate::sim::superweapon::lightning_storm::LightningStorm;
use crate::sim::superweapon::psychic_dominator::PsychicDominatorState;

#[test]
fn infantry_body_light_respects_side_threshold_signed_height_and_weather() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nExtraInfantryLight=.2\n[InfantryTypes]\n0=E1\n",
    ))
    .unwrap();
    assert_eq!(rules.general.extra_infantry_light, 200);
    // Observed whole GI DrawIt outputs with top1000, Extra200, cell level0
    // and Normal/Ion/Dominator Level32/-12/77. The last control activates
    // all three weather effects; Ion wins over Dominator and Nuke fade-in.
    for (flags, z, on_bridge, all_weather, height, total) in [
        (0x10000, 208, false, false, 208, 700),  // side_equal208
        (0x10000, 209, false, false, 209, 1232), // side_above208
        (0x100, 416, true, false, 0, 1200),      // bridge_deck_height0
        (0x100, 209, true, false, -207, 1200),   // bridge_height_minus207
        (0x100, 208, true, false, -208, 1168),   // bridge_height_minus208
        (0x100, 832, true, true, 416, 1176),     // bridge_all_weather_ion_wins
    ] {
        let terrain = test_grid(32, 32, |x, y| {
            let mut cell = test_smudge_cell(x, y);
            if (x, y) == (10, 20) {
                cell.bridge_facts.raw_flags = flags;
            }
            cell
        });
        let mut sim = Simulation::new();
        let owner = sim.intern("OWNER");
        let kind = sim.intern("E1");
        let mut infantry = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            20,
            0,
            0,
            owner,
            Health::new(125),
            kind,
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        infantry.position.put_location(DriveCoord {
            x: 2688,
            y: 5248,
            z,
        });
        infantry.set_on_bridge(on_bridge);
        let mut normal = ScenarioLightProfileUnits::normal_default();
        normal.level_units = 32;
        let mut ion = ScenarioLightProfileUnits::ion_default();
        ion.level_units = -12;
        let mut dominator = ScenarioLightProfileUnits::dominator_default();
        dominator.level_units = 77;
        *sim.session_mut_for_test().lighting_mut() =
            ScenarioLightingState::new(normal, ion, dominator, 1, 1);
        if all_weather {
            sim.session_mut_for_test()
                .lighting_mut()
                .set_nuke_flash_for_test(1, 0, 30);
            *sim.psychic_dominator_mut_for_test() =
                PsychicDominatorState::for_test(5, (0, 0), None, None);
            *sim.lightning_storm_mut_for_test() = LightningStorm::raging_for_test(owner, (0, 0));
        }
        // Check live state/RNG/shared-Dummy nonmutation once, on the weather
        // control that exercises both height and profile selection.
        let before = all_weather.then(|| {
            (
                sim.state_hash(),
                sim.rng_state(),
                terrain.shared_cell_dummy().snapshot(),
            )
        });
        let cells = NativeCellQuery::isolated(&terrain);
        assert_eq!(
            crate::sim::movement::air_movement::queried_flight_height(&infantry, Some(&cells)),
            height,
            "z={z}, OnBridge={on_bridge}: deck-relative height",
        );
        let extra = body_extra_light(&infantry, &rules, &sim, Some(&cells));
        // An empty light grid explicitly supplies the native top1000 prior.
        assert_eq!(
            body_palette_light(&CellLightGrid::new(), (10, 20), extra).brightness(),
            total,
            "flags={flags:x}, z={z}, OnBridge={on_bridge}",
        );
        if let Some(before) = before {
            assert_eq!(body_extra_light(&infantry, &rules, &sim, None), 200);
            assert_eq!(
                (
                    sim.state_hash(),
                    sim.rng_state(),
                    terrain.shared_cell_dummy().snapshot()
                ),
                before,
            );
        }
    }
}

/// Keep real Display admission and SHP emission. The minimal atlas covers the
/// authored six-frame Ready/Walk and retail GI's eight-facing Ready block.
fn assert_shp_light(
    runtime: &crate::sim::runtime::SimRuntime,
    id: u64,
    light: &CellLightGrid,
    expected: i32,
) {
    use crate::app::presentation::instances::{ShpInputs, build_shp_instances_from_inputs};
    use crate::app::presentation::render::draw_plan_lowering::{NativeDisplayOrder, ObjectTexture};
    use crate::render::sprite_atlas::{
        ShpPaletteContext, ShpSpriteEntry, ShpSpriteKey, SpriteAtlas,
    };
    use crate::rules::house_colors::HouseColorIndex;

    let sim = &runtime.simulation;
    let actor = sim.entities().get(id).unwrap();
    let owner_name = sim.resolve(actor.owner()).to_owned();
    let terrain = sim.resolved_terrain().unwrap();
    let before = (
        sim.state_hash(),
        sim.rng_state(),
        terrain.shared_cell_dummy().snapshot(),
    );
    let entry = ShpSpriteEntry {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [4.0; 2],
        offset_x: 0.0,
        offset_y: 0.0,
        canvas_rect: [0.0, 0.0, 4.0, 4.0],
        extended: true,
        page: 7,
        source_palette: 1,
    };
    let atlas = SpriteAtlas::from_test_entries(
        (0..8)
            .map(|frame| {
                (
                    ShpSpriteKey {
                        palette_context: ShpPaletteContext::Legacy,
                        type_id: "E1".to_owned(),
                        facing: 0,
                        frame,
                        house_color: HouseColorIndex(0),
                    },
                    entry,
                )
            })
            .collect(),
    );
    let interpolation = crate::app::presentation::interpolation::RenderInterpolation::new(false);
    let pitches = std::cell::RefCell::default();
    let colors =
        crate::map::houses::HouseColorMap::from([(owner_name.clone(), HouseColorIndex(0))]);
    let location = crate::sim::movement::ground_pose::object_location(actor, Some(terrain));
    let screen =
        crate::util::lepton::absolute_leptons_to_screen(location.x, location.y, location.z);
    let mut draws = Vec::new();
    build_shp_instances_from_inputs(
        &ShpInputs {
            runtime,
            atlas: Some(&atlas),
            overlay_atlas: None,
            unit_atlas: None,
            barrel_image_pitches: &pitches,
            interpolation: &interpolation,
            camera: [screen.0 - 400.0, screen.1 - 300.0],
            render_size: [800, 600],
            zoom: 1.0,
            axis: crate::render::batch::DepthAxis {
                origin_y: 0.0,
                world_height: 10_000.0,
            },
            local_owner: Some(&owner_name),
            ignore_visibility: true,
            lighting: light,
            house_colors: &colors,
            building_zshape: false,
        },
        &mut draws,
        &NativeDisplayOrder::from_display(sim.display_layers()),
    );
    let mut bodies = draws
        .iter()
        .filter(|draw| draw.parent.id == id)
        .flat_map(|draw| &draw.pieces)
        .filter(|piece| piece.target == ObjectTexture::ShpPage(7));
    let body = bodies.next().expect("ordinary Display emits the GI body");
    assert!(bodies.next().is_none(), "one GI body per display entry");
    assert_eq!(body.instance.palette_light.brightness(), expected);
    for channel in body.instance.tint {
        assert!((channel - expected as f32 / 1000.0).abs() < 0.000_001);
    }
    assert_eq!(
        (
            sim.state_hash(),
            sim.rng_state(),
            terrain.shared_cell_dummy().snapshot()
        ),
        before,
    );
}

#[test]
fn infantry_shp_top_light_follows_public_move_head_production() {
    use crate::sim::command::Command;
    use crate::sim::runtime::{SimResources, SimRuntime};

    // Deliberate component-scene rules/art, not a retail type-load claim.
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[AudioVisual]\nExtraInfantryLight=.2\n[InfantryTypes]\n0=E1\n\
             [E1]\nImage=GI\nStrength=125\nSpeed=4\nMovementZone=Infantry\n\
             Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n",
        ),
        &IniFile::from_str("[GI]\nSequence=GISequence\n[GISequence]\nReady=0,6,0\nWalk=0,6,0\n"),
    )
    .unwrap();
    assert_eq!(rules.general.extra_infantry_light, 200);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.set_input_delay_ticks(0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let id = sim
        .spawn_object("E1", "Americans", 10, 20, 0, &rules)
        .unwrap();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources::new(rules),
    };
    // Observed whole GI draws: null-head/destination-only700; a produced
    // head over top1000 gives1200, or700 when only the two tops are swapped.
    let sample = |runtime: &SimRuntime, current_top, head_top, expected| {
        let mut light = CellLightGrid::new();
        let profile = light.profiles().default_profile_id();
        for (cell, top) in [((10, 20), current_top), ((11, 20), head_top)] {
            light.insert_light(
                cell,
                crate::map::lighting::CellLight::compatibility(profile, [1000; 3], top),
            );
        }
        assert_shp_light(runtime, id, &light, expected);
    };
    sample(&runtime, 500, 1000, 700);
    assert!(runtime.simulation.apply_command(
        "Americans",
        &Command::Move {
            entity_id: id,
            target_rx: 11,
            target_ry: 20
        },
        Some(&runtime.resources.rules),
    ));
    let loco = runtime
        .simulation
        .entities()
        .get(id)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    assert_eq!(loco.walk_destination_cell(), Some((11, 20)));
    assert!(
        loco.step_head().is_none(),
        "a destination does not supply a head"
    );
    sample(&runtime, 500, 1000, 700);

    let mut produced = false;
    for _ in 0..128 {
        runtime
            .advance_frame_for_tooling(&[], 67)
            .expect("ordinary Move frame");
        let sim = &runtime.simulation;
        let actor = sim.entities().get(id).unwrap();
        let navigation = sim.foot_navigation_coordinate(id).unwrap();
        if (actor.position.rx(), actor.position.ry()) == (10, 20)
            && (navigation.x / 256, navigation.y / 256) == (11, 20)
        {
            sample(&runtime, 500, 1000, 1200);
            sample(&runtime, 1000, 500, 700);
            produced = true;
            break;
        }
    }
    assert!(produced, "public Move must produce physical10,20/head11,20");
}

#[test]
#[ignore = "slow; physical Hills loader and ordinary Infantry placement"]
fn retail_gi_body_light_on_the_physical_hills_bridge_side() {
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut launch = crate::headless_scenario::battle_session(
        "Hills.mmx",
        &[(LaunchCountry::America, AiDifficulty::Easy)],
    );
    launch.options.bases = false;
    launch.options.unit_count = 0;
    launch.options.short_game = false;
    let mut scene =
        crate::headless_scenario::load_session(&retail, "Hills.mmx", 0x0051_8F90, launch).unwrap();
    let owner = scene.sim().session().current_house().unwrap();
    let owner_name = scene.sim().resolve(owner).to_owned();
    let rt = &mut scene.runtime;
    let id = rt
        .simulation
        .spawn_object_with_overlay_registry(
            "E1",
            &owner_name,
            78,
            76,
            0,
            &rt.resources.rules,
            &rt.resources.overlay_registry,
        )
        .unwrap();
    let sim = scene.sim();
    let terrain = sim.resolved_terrain().unwrap();
    let actor = sim.entities().get(id).unwrap();
    let cell = terrain.cell(78, 76).unwrap();
    assert_eq!(cell.bridge_facts.raw_flags & 0x10B00, 0x10B00);
    assert_eq!(cell.level, 2);
    assert!(!actor.on_bridge());
    let cells = NativeCellQuery::isolated(terrain);
    assert_eq!(
        crate::sim::movement::air_movement::queried_flight_height(actor, Some(&cells)),
        0
    );
    assert_eq!(
        scene.runtime.resources.rules.general.extra_infantry_light,
        200
    );
    // Original GI draw projected these physical flags/level into sparse
    // cells: top1000 + extra(-300) = observed700. This is not a claim about
    // native Hills traversal or the physical map's lighting scalar.
    assert_eq!(
        body_extra_light(actor, &scene.runtime.resources.rules, sim, Some(&cells)),
        -300
    );
    assert_shp_light(&scene.runtime, id, &CellLightGrid::new(), 700);
}
