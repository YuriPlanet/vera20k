//! Producer-boundary copies through real Overlay construction and the shared
//! lighting transaction. These exercise field/order/lifetime, not native RGB.
use super::*;
use crate::map::lighting::PointLight;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
use crate::sim::light_sources::LightingEvent;
use crate::sim::snapshot::GameSnapshot;

fn cell_anim() -> (
    Simulation,
    RuleSet,
    ResolvedTerrainGrid,
    Vec<LightingEvent>,
    u64,
    (u16, u16),
) {
    let ini = IniFile::from_str(
        "[Colors]\nGold=43,239,255\nNeonGreen=104,241,195\n\
         [Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nColor=NeonGreen\n\
         [Animations]\n0=SPARK\n[OverlayTypes]\n0=ANIMBOX\n\
         [ANIMBOX]\nTiberium=yes\nCellAnim=SPARK\n\
         [CrateRules]\nCrateMinimum=1\nCrateMaximum=1\nWoodCrateImg=ANIMBOX\n\
         [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=100%\n\
         Hover=100%\nAmphibious=100%\nFloatBeach=100%\n",
    );
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[SPARK]\nRate=1\nEnd=20\nAltPalette=yes\n",
    ));
    art.bind_anim_frame_count_for_test("SPARK", 20);
    rules.replace_art_registry_for_test(art);
    let mut terrain = crate::map::resolved_terrain::test_grid(40, 40, |x, y| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            allows_tiberium: true,
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_flat_cell(x, y)
        }
    });
    terrain
        .set_land_speed_rules_for_test(&crate::rules::terrain_rules::TerrainRules::from_ini(&ini));
    let mut sim = Simulation::with_seed(31);
    sim.session_mut_for_test().set_map_width_for_test(40);
    sim.session_mut_for_test().set_map_height_for_test(40);
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .crates = true;
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 20,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    }));
    sim.set_playfield_size_height_for_test(Some(20));
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.install_overlay_grid_for_load(crate::sim::overlay_grid::OverlayGrid::new(40, 40));
    assert_eq!(
        crate::sim::crates::place_scenario_start_crates(&mut sim, &rules, &registry, 1).visible,
        1
    );
    let events = sim.lighting_sources().pending().to_vec();
    let (id, cell) = events
        .iter()
        .find_map(|event| match event {
            LightingEvent::RetainAnimIntensity {
                anim,
                cell: Some(cell),
            } => Some((*anim, *cell)),
            _ => None,
        })
        .expect("the actual OverlayMark publishes its original cell read");
    assert_eq!(sim.anim(id).unwrap().z_adjust(), 0);
    let world = sim.anim_absolute_coord(id).unwrap();
    assert_ne!(
        (world.x / 256, world.y / 256),
        (i32::from(cell.0), i32::from(cell.1))
    );
    (sim, rules, terrain, events, id, cell)
}

#[test]
fn overlay_sample_flushes_prior_sources_and_survives_later_relight() {
    let (sim, rules, terrain, mut events, id, cell) = cell_anim();
    let source = LightingEvent::Building {
        id: 900,
        source: Some(PointLight {
            rx: cell.0,
            ry: cell.1,
            center_x: i32::from(cell.0) * 256 + 128,
            center_y: i32::from(cell.1) * 256 + 128,
            radius_leptons: 512,
            intensity: 300,
            tint: [0; 3],
            active: true,
            detail: true,
        }),
    };
    let mut control = MatchLighting::default();
    control.install(
        CellLightGrid::new(),
        LightingConfig::default(),
        2,
        Some((&terrain, &sim)),
        None,
    );
    control.apply_events(&terrain, std::slice::from_ref(&source));
    let expected = control.grid().cell_light_at(cell).unwrap().top_scalar;
    events.insert(0, source);
    events.push(LightingEvent::Building {
        id: 900,
        source: None,
    });
    let mut lights = MatchLighting::default();
    lights.install(
        CellLightGrid::new(),
        LightingConfig::default(),
        2,
        Some((&terrain, &sim)),
        Some(&events),
    );
    assert_eq!(lights.anim_intensities()[&id], expected);
    assert_ne!(
        lights.grid().cell_light_at(cell).unwrap().top_scalar,
        expected
    );
    lights.refresh(&terrain, &sim, 0);
    assert_eq!(
        lights.anim_intensities()[&id],
        expected,
        "detail rebuild never resamples FC"
    );
    let anim = sim.anim(id).unwrap();
    let light = anim_palette_light(
        lights.grid(),
        cell,
        rules.art().anim_runtime_config("SPARK"),
        false,
        anim.remap().as_ref(),
        expected,
    );
    assert_eq!(
        (light.rows(), light.brightness()),
        (53, expected),
        "instance Convert precedes AltPalette"
    );
}

#[test]
fn save_projects_pending_anim_reads_without_consuming_live_history() {
    let (mut sim, _rules, terrain, events, id, cell) = cell_anim();
    let mut lights = MatchLighting::default();
    lights.install(
        CellLightGrid::new(),
        LightingConfig::default(),
        2,
        Some((&terrain, &sim)),
        None,
    );
    // A pending producer at the pre-Logic boundary: the save sees its prefix,
    // while ordinary publication still owns the live transaction afterward.
    let before = (sim.state_hash(), sim.rng_state());
    let expected = lights.grid().cell_light_at(cell).unwrap().top_scalar;
    let (_, saved) = lights.clone().saved_at(&sim);
    assert_eq!(saved, [(id, expected)]);
    assert!(lights.anim_intensities().is_empty());
    assert_eq!(sim.lighting_sources().pending().len(), events.len());
    assert_eq!((sim.state_hash(), sim.rng_state()), before);
    let bytes = GameSnapshot::save_validated_with_presentation(
        &sim,
        1,
        2,
        "anim light",
        3,
        &[],
        &saved,
        None,
        None,
        None,
        None,
    );
    let mut loaded = GameSnapshot::load_validated(&bytes, 1, 2, sim.session().map_name()).unwrap();
    lights.apply_events(&terrain, &sim.take_lighting_events());
    loaded
        .sim
        .session_mut_for_test()
        .lighting_mut()
        .set_current_ambient_for_test(20);
    lights.install(
        CellLightGrid::new(),
        LightingConfig::default(),
        2,
        Some((&terrain, &loaded.sim)),
        None,
    );
    assert!(
        lights.anim_intensities().is_empty(),
        "world replacement clears outgoing samples"
    );
    assert_ne!(
        lights.grid().cell_light_at(cell).unwrap().top_scalar,
        expected
    );
    lights.restore_anim_intensities(loaded.anim_intensities);
    assert_eq!(lights.anim_intensities()[&id], expected);
    lights.refresh(&terrain, &Simulation::new(), 2);
    assert!(
        lights.anim_intensities().is_empty(),
        "physical disposal retires the sample"
    );
}
