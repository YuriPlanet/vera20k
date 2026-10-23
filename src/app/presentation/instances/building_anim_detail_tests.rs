//! Physical slot construction and AI, followed by the actual CPU emitter and
//! retained Display lowering. Atlas entries retain physical frame geometry;
//! this presence regression does not claim atlas packing or pixel parity.

use super::*;
use crate::app::presentation::detail::DetailState;
use crate::app::presentation::instances::{DisplayEffectInputs, build_display_effect_instances};
use crate::app::presentation::render::draw_plan_lowering::lower_object_instances;
use crate::assets::shp_file::ShpFile;
use crate::headless_scenario::HeadlessScenario;
use crate::map::lighting::CellLightGrid;
use crate::render::sprite_atlas::{ShpSpriteEntry, SpriteAtlas};
use crate::sim::world::{FrameEffects, TickLane};
use crate::skirmish_launch::LaunchCountry;
use std::cell::RefCell;
use std::path::Path;

const OWNER: &str = "DetailPlant";

fn step(scene: &mut HeadlessScenario) {
    let output = scene
        .runtime
        .advance_frame(
            &[],
            crate::app::types::SIM_TICK_MS,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .expect("ordinary bound frame");
    assert!(
        output.tick.frame_committed,
        "the retail scene keeps running"
    );
}

fn slot(scene: &HeadlessScenario, plant: u64) -> u64 {
    scene
        .sim()
        .entities()
        .get(plant)
        .unwrap()
        .building_art
        .anim_slots()[3]
        .expect("ordinary GAPOWR has its ActiveAnim3")
}

fn scene(root: &Path) -> (HeadlessScenario, u64) {
    let mut launch = crate::headless_scenario::battle_session("XMP03T4.MAP", &[]);
    launch.options.shroud = false;
    launch.options.fog_of_war = false;
    launch.options.unit_count = 0;
    let mut scene = crate::headless_scenario::load_session(root, "XMP03T4.MAP", 0x422C_70, launch)
        .expect("physical XMP03T4 with bound Rules, ART and terrain");
    scene.add_human_house(OWNER, LaunchCountry::America);
    let candidates = {
        let sim = scene.sim();
        let terrain = sim.resolved_terrain().unwrap();
        let bounds = sim.playfield_bounds().unwrap();
        let offsets = scene
            .runtime
            .resources
            .rules
            .object("GAPOWR")
            .unwrap()
            .foundation
            .cell_offsets();
        terrain
            .cells()
            .iter()
            .filter(|cell| {
                offsets.iter().all(|&(dx, dy)| {
                    let x = i32::from(cell.rx) + i32::from(dx);
                    let y = i32::from(cell.ry) + i32::from(dy);
                    crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                        (x, y),
                        Some(bounds),
                        Some(terrain),
                    ) && terrain
                        .cell(x as u16, y as u16)
                        .is_some_and(|tile| tile.level == cell.level && tile.slope_type == 0)
                }) && sim.entities().values().all(|entity| {
                    i32::from(entity.position.rx()).abs_diff(i32::from(cell.rx)) > 5
                        || i32::from(entity.position.ry()).abs_diff(i32::from(cell.ry)) > 5
                })
            })
            .map(|cell| (cell.rx, cell.ry))
            .collect::<Vec<_>>()
    };
    // The placement is an explicit scene input; terrain, ART, slot state and
    // animation timers are never edited. Unlimbo still decides admission.
    let plant = candidates
        .into_iter()
        .find_map(|(x, y)| {
            scene.runtime.simulation.spawn_object_with_overlay_registry(
                "GAPOWR",
                OWNER,
                x,
                y,
                0,
                &scene.runtime.resources.rules,
                &scene.runtime.resources.overlay_registry,
            )
        })
        .expect("a clear physical foundation admits the stock power plant");
    let placed = slot(&scene, plant);
    // GrandOpening installs the slot; the first ordinary operational edge can
    // replace it through the existing ActiveAnimPoweredSpecial policy. Retain
    // the identity only after this real Building update.
    step(&mut scene);
    let settled = slot(&scene, plant);
    if placed != settled {
        assert!(scene.sim().anim(placed).is_none());
    }
    assert_eq!(
        scene
            .sim()
            .building_operational_state(plant, &scene.runtime.resources.rules),
        Some(true)
    );
    (scene, plant)
}

fn emit(
    scene: &HeadlessScenario,
    plant: u64,
    atlas: &SpriteAtlas,
    detail: &RefCell<DetailState>,
    detail_level: u32,
    camera_displacement: Option<[f32; 2]>,
) -> bool {
    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    let id = slot(scene, plant);
    let before = (sim.state_hash(), sim.rng_state());
    let before_runtime = serde_json::to_value(sim.anim(id).unwrap().runtime()).unwrap();
    let building = sim.entities().get(plant).unwrap();
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let light = CellLightGrid::new();
    let cell = (building.position.rx(), building.position.ry());
    let coord = sim.anim_absolute_coord(id).unwrap();
    let center = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    let displacement = camera_displacement.unwrap_or([0.0; 2]);
    let camera = [
        center.0 - 400.0 + displacement[0],
        center.1 - 300.0 + displacement[1],
    ];
    let viewport = [camera[0], camera[1], 800.0, 600.0];
    let local_owner = sim.session().current_house();
    let viewer = local_owner.map(|owner| sim.interner().resolve(owner));
    let mut observer = super::super::helpers::observer_draw_context(
        sim,
        building,
        viewer,
        local_owner,
        Some(rules),
    );
    observer.drawn_voxel = Some(false);
    let material = DrawState::for_building_slot_anims(
        building,
        sim.session().binary_frame(),
        super::super::units::house_color_to_remap_row(HouseColorIndex(0)),
        observer,
    );
    assert!(material.visible);
    let mut house_colors = crate::map::houses::HouseColorMap::new();
    house_colors.insert(OWNER.to_owned(), HouseColorIndex(0));
    let hidden = Default::default();
    let mut prepared = Vec::new();
    // Use the application's binding/material preparation and one shared Display
    // admission walk. Preparing a slot neither consumes the FPS latch nor makes
    // the prepared piece visible before its actual retained encounter.
    emit_building_anims(
        &mut prepared,
        &order,
        atlas,
        rules.art(),
        Some(rules),
        "GAPOWR",
        HouseColorIndex(0),
        light.building_body_tint_at(cell),
        crate::app::presentation::lighting::body_palette_light(&light, cell, 0),
        &light,
        cell,
        sim,
        building,
        &|_| 0,
        crate::render::batch::DepthAxis::NONE.origin_y,
        crate::render::batch::DepthAxis::NONE.world_height,
        viewport,
        material.state,
        detail_level as i32,
    );
    let prepared = prepared
        .into_iter()
        .map(|parent| (parent.parent.id, parent))
        .collect();
    let mut parents = Vec::new();
    build_display_effect_instances(
        &DisplayEffectInputs {
            interpolation: None,
            sim,
            rules,
            camera,
            render_size: [800, 600],
            tactical: [0, 0, 800, 600],
            zoom: 1.0,
            axis: crate::render::batch::DepthAxis::NONE,
            atlas: Some(atlas),
            detail,
            detail_level,
            lighting: &light,
            retained_anim_intensities: None,
            house_colors: &house_colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner,
            observe_particles: false,
        },
        &mut parents,
        &order,
        prepared,
    );
    let count = lower_object_instances(parents)
        .iter()
        .flat_map(|layer| &layer.owners)
        .filter(|&&owner| owner == id)
        .count();
    assert!(count <= 1, "one retained slot draws at most one body");
    assert_eq!((sim.state_hash(), sim.rng_state()), before);
    assert_eq!(slot(scene, plant), id);
    assert_eq!(
        serde_json::to_value(sim.anim(id).unwrap().runtime()).unwrap(),
        before_runtime
    );
    count == 1
}

fn check_performance(detail: &RefCell<DetailState>, native: &serde_json::Value, context: &str) {
    // observation() is a read, unlike minimum_frame_rate(): comparing the
    // latter here would itself consume another original55AF60 query.
    let actual = serde_json::to_value(detail.borrow().observation()).unwrap();
    for (rust, original) in [
        ("frame_rate", "current_fps"),
        ("logic_visits", "logic_count"),
        ("minimum", "minimum"),
        ("buffer", "buffer"),
        ("reduced", "reduced"),
        ("initialized", "timer_initialized"),
    ] {
        assert_eq!(actual[rust], native[original], "{context}: {rust}");
    }
    assert_eq!(
        actual["sample_start"], native["timer_words"][0],
        "{context}"
    );
    assert_eq!(
        actual["sample_duration"], native["timer_words"][2],
        "{context}"
    );
}

fn sampled_detail(fps: u32) -> RefCell<DetailState> {
    let mut detail = DetailState::new();
    for _ in 0..fps {
        detail.record_logic_visit();
    }
    detail.throttle_tail(0);
    assert_eq!(detail.frame_rate(), fps);
    RefCell::new(detail)
}

#[test]
#[ignore = "requires physical XMP03T4/Rules/ART; ordinary GAPOWR slot through CPU emitter and Display lowering"]
fn retail_power_plant_slot_obeys_detail_without_pausing_its_ai() {
    let Some((root, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let (mut scene, plant) = scene(&root);
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let rules = &scene.runtime.resources.rules;
    let config = rules.art().anim_runtime_config("GAPOWR_A").unwrap();
    assert!(config.art_body_read && !config.shadow);
    assert_eq!(
        config.detail_level, 2,
        "physical stock ART, not a test override"
    );
    assert_eq!(
        (config.start, config.loop_end, config.loop_count),
        (0, 8, -1)
    );
    assert!(!config.translucent && config.translucency == 0);
    let image = rules.art().resolve_anim_image_id("GAPOWR_A");
    let physical = crate::rules::art_data::effect_shp_candidates(
        crate::rules::art_data::EffectImageKind::Animation,
        Some(rules.art()),
        "GAPOWR_A",
        &image,
        theater.extension,
        "TEMPERATE",
    )
    .into_iter()
    .find_map(|name| {
        assets
            .get_ref(&name)
            .map(|bytes| ShpFile::from_bytes(bytes).unwrap())
    })
    .expect("physical bound GAPOWR_A SHP");
    assert_eq!(
        config.raw_shp_frame_count,
        Some(physical.frames.len() as i32)
    );
    let context = crate::render::sprite_atlas::attached_anim_palette_context(Some(config));
    let entries = physical
        .frames
        .iter()
        .enumerate()
        .map(|(frame, image)| {
            assert!(image.frame_width > 0 && image.frame_height > 0);
            (
                ShpSpriteKey {
                    palette_context: context,
                    type_id: "GAPOWR_A".to_owned(),
                    facing: 0,
                    frame: frame as u16,
                    house_color: HouseColorIndex(0),
                },
                ShpSpriteEntry {
                    uv_origin: [0.0; 2],
                    uv_size: [1.0; 2],
                    pixel_size: [f32::from(image.frame_width), f32::from(image.frame_height)],
                    offset_x: f32::from(image.frame_x) - f32::from(physical.width) / 2.0,
                    offset_y: f32::from(image.frame_y) - f32::from(physical.height) / 2.0,
                    canvas_rect: [
                        0.0,
                        0.0,
                        f32::from(physical.width),
                        f32::from(physical.height),
                    ],
                    extended: image.format & 2 != 0,
                    page: 0,
                    source_palette: 1,
                },
            )
        })
        .collect();
    let atlas = SpriteAtlas::from_test_entries(entries);
    let retained = slot(&scene, plant);
    let native = crate::test_fixture::json("tools/spatial_oracle/building_anim_detail.json");
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    assert_eq!(native["anim_type_reader"]["native_detail_level"], 2);
    // This real retail map has a different world position from the native flat
    // component scene. Apply the measured camera displacement to our centered
    // view; do not copy its unrelated absolute origin onto the retail plant.
    let normal_camera = &native["input"]["normal_camera"];
    assert_eq!(
        normal_camera,
        &rows
            .iter()
            .find(|row| row["name"] == "restored_camera_fps14_options2")
            .expect("original restored-view control")["supplied_camera"]
    );
    let mut differences = Vec::new();
    let detail = sampled_detail(0);
    detail.borrow_mut().configure_normal(&rules.general.detail);
    let mut published_bucket = 0;
    // Whole original422C70 executes every row against one retained55AF60 latch.
    // Replay its real counter/Throttle samples, preserving latch history across
    // Options changes and recovery thresholds. Expected presence comes from
    // original Surface calls, not a Rust copy of the visibility predicate.
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let before = &row["performance_before"];
        let fps = before["current_fps"].as_u64().unwrap() as u32;
        let detail_level = before["options_detail"].as_u64().unwrap() as u32;
        let bucket = before["timer_words"][0].as_u64().unwrap();
        if bucket != published_bucket {
            let mut state = detail.borrow_mut();
            for _ in 0..fps {
                state.record_logic_visit();
            }
            state.throttle_tail(bucket * 16);
            published_bucket = bucket;
        }
        check_performance(&detail, before, &format!("{name} before"));
        let camera = row["supplied_camera"].as_array().map(|camera| {
            [
                (camera[0].as_f64().unwrap() - normal_camera[0].as_f64().unwrap()) as f32,
                (camera[1].as_f64().unwrap() - normal_camera[1].as_f64().unwrap()) as f32,
            ]
        });
        let calls = row["observed_surface_call_count"].as_u64().unwrap();
        assert!(calls <= 1, "one native body call for this no-Shadow type");
        let expected = calls == 1;
        let actual = emit(&scene, plant, &atlas, &detail, detail_level, camera);
        check_performance(&detail, &row["performance_after"], &format!("{name} after"));
        eprintln!("GAPOWR_A {name}: presence {actual}, original {expected}");
        if actual != expected {
            differences.push(format!("{name}: {actual} != {expected}"));
        }
    }
    let initial_frame = scene.sim().anim(retained).unwrap().runtime().current_frame;
    let mut progressed = false;
    for _ in 0..120 {
        if emit(&scene, plant, &atlas, &sampled_detail(0), 2, None) {
            differences.push("low-FPS body appeared while ordinary AI was running".to_owned());
        }
        step(&mut scene);
        assert_eq!(
            slot(&scene, plant),
            retained,
            "draw suppression retains the slot"
        );
        if scene.sim().anim(retained).unwrap().runtime().current_frame != initial_frame {
            progressed = true;
            break;
        }
    }
    assert!(
        progressed,
        "ordinary Anim AI progresses while detail hides its body"
    );
    assert!(
        emit(&scene, plant, &atlas, &sampled_detail(60), 2, None),
        "the same live slot remains drawable"
    );
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
