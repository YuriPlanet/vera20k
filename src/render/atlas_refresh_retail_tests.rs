//! Retail timing witness for entity-atlas refreshes during a match.
//!
//! Loads the stock Dustbowl skirmish map, builds the SHP sprite atlas and the
//! voxel unit atlas as map load does, then spawns building types and a vehicle
//! type the map did not start with and times the refresh each one triggers,
//! plus the coverage check that runs on every spawn, death and Limbo. Timings
//! are printed for the record; the assertions pin what a refresh must leave
//! drawable, that it satisfies the coverage check, that a refresh with
//! nothing new renders nothing, and that the grown atlases hold exactly what
//! a map load of the same world builds.
//!
//! `VERA20K_ATLAS_PRELOAD=voxels` first places one of every vehicle and
//! aircraft type that fits beside the player, so the map-load unit atlas holds
//! nearly every stock voxel model (used to compare two builds' sprites).
//!
//! Run (release, so timings resemble the shipped game):
//! RA2_DIR=<retail root> cargo test -p vera20k --lib --release \
//!     retail_atlas_refresh_costs -- --ignored --nocapture

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::assets::asset_manager::AssetManager;
use crate::map::houses::HouseColorMap;
use crate::render::batch::BatchRenderer;
use crate::render::sprite_atlas::{self, ShpPaletteContext, ShpSpriteKey};
use crate::render::unit_atlas::{self, UnitSpriteKey, VxlLayer};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::object_type::ObjectCategory;

fn retail_root() -> PathBuf {
    std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        })
}

fn headless_device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("atlas refresh timing requires a wgpu adapter");
    let required_limits = wgpu::Limits {
        max_texture_dimension_2d: adapter
            .limits()
            .max_texture_dimension_2d
            .min(crate::render::gpu::MAX_USEFUL_TEXTURE_DIM),
        ..wgpu::Limits::default()
    };
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits,
        ..Default::default()
    }))
    .expect("headless device")
}

/// Block until every queued upload has executed, so a timing includes it.
fn settle(device: &wgpu::Device, queue: &wgpu::Queue) {
    queue.submit(std::iter::empty());
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[test]
#[ignore = "requires retail archives and a wgpu adapter; prints atlas refresh timings"]
fn retail_atlas_refresh_costs() {
    let root = retail_root();
    let mut scenario =
        crate::headless_scenario::load(&root, "Dustbowl.mmx", 0x0B21_D6E5).expect("Dustbowl loads");
    let mut assets = AssetManager::new(&root).expect("retail archives");
    let theater_name = scenario.map.header.theater.clone();
    let theater =
        crate::map::theater::load_theater(&mut assets, &theater_name).expect("theater loads");
    let theater_ext = theater.extension;
    let (device, queue) = headless_device();
    let batch =
        BatchRenderer::new_with_device(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);

    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;
    let art = &rules.art_registry;

    // One remap colour per owner, as the launch session assigns them.
    let mut house_colors: HouseColorMap = HashMap::new();
    for entity in sim.entities().values() {
        let owner = sim.interner.resolve(entity.owner()).to_string();
        let next = HouseColorIndex(house_colors.len() as u8);
        house_colors.entry(owner).or_insert(next);
    }
    // The player's starting vehicle names the house that builds the new types.
    let owner = sim
        .entities()
        .values()
        .find(|entity| entity.is_voxel)
        .map(|entity| sim.interner.resolve(entity.owner()).to_string())
        .expect("the player starts with a vehicle");
    let owner_color = house_colors[&owner];
    // Terrain-attached animations draw from the theater ISO palette, as in the app.
    let cell_drawers: HashSet<String> = sim
        .resolved_terrain
        .as_ref()
        .into_iter()
        .flat_map(|terrain| terrain.tile_animations())
        .map(|anim| anim.anim_name.to_ascii_uppercase())
        .collect();
    let anchor = sim
        .entities()
        .values()
        .find(|entity| sim.interner.resolve(entity.owner()) == owner)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .expect("owned anchor");
    let heights = BTreeMap::new();
    // Place `type_id` on the first free cell of growing rings around the
    // player's first object, two cells apart.
    let spawn_near = |sim: &mut crate::sim::world::Simulation, type_id: &str, reach: i32| {
        (1..=reach)
            .flat_map(|ring| {
                (-ring..=ring).flat_map(move |d| [(d, -ring), (d, ring), (-ring, d), (ring, d)])
            })
            .any(|(dx, dy)| {
                let rx = i32::from(anchor.0) + dx * 2;
                let ry = i32::from(anchor.1) + dy * 2;
                rx > 0
                    && ry > 0
                    && sim
                        .spawn_object(type_id, &owner, rx as u16, ry as u16, 64, rules, &heights)
                        .is_some()
            })
    };
    let refresh_vehicles = ["HTNK", "MTNK", "APOC", "LTNK", "FV"];
    if std::env::var("VERA20K_ATLAS_PRELOAD").is_ok_and(|value| value == "voxels") {
        let voxel_types: Vec<String> = rules
            .all_objects()
            .filter(|obj| {
                matches!(
                    obj.category,
                    ObjectCategory::Vehicle | ObjectCategory::Aircraft
                )
            })
            .map(|obj| obj.id.clone())
            .filter(|type_id| !refresh_vehicles.contains(&type_id.as_str()))
            .collect();
        let placed = voxel_types
            .iter()
            .filter(|type_id| spawn_near(sim, type_id, 12))
            .count();
        eprintln!(
            "preloaded {placed} of {} vehicle and aircraft types",
            voxel_types.len()
        );
    }

    let started = Instant::now();
    let extra = crate::app::frontend::skirmish::deployable_building_types(Some(rules));
    let remaps = sprite_atlas::collect_anim_remap_base_keys(sim);
    let mut sprites = sprite_atlas::build_sprite_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        &theater.unit_palette,
        theater_ext,
        &theater_name,
        Some(rules),
        Some(art),
        &house_colors,
        &extra,
        &remaps,
        &cell_drawers,
        Some(&theater.iso_palette),
        None,
        Some(&sim.interner),
    )
    .expect("initial sprite atlas");
    settle(&device, &queue);
    eprintln!(
        "initial sprite atlas: {:.0} ms, {} sprites on {} pages",
        ms(started.elapsed()),
        sprites.sprite_count(),
        sprites.page_count()
    );

    let started = Instant::now();
    let mut units = unit_atlas::build_unit_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        Some(rules),
        Some(art),
        None,
        Some(&sim.interner),
    )
    .expect("initial unit atlas");
    settle(&device, &queue);
    eprintln!(
        "initial unit atlas: {:.0} ms, {} sprites on {} pages",
        ms(started.elapsed()),
        units.sprite_count(),
        units.page_count()
    );

    // Building types and a vehicle type the map did not start with.
    let present: HashSet<String> = sim
        .entities()
        .values()
        .map(|entity| sim.interner.resolve(entity.type_ref()).to_ascii_uppercase())
        .collect();
    let mut absent = ["GAPOWR", "NAPOWR", "GAPILE", "NAHAND", "GAREFN", "NAREFN"]
        .into_iter()
        .filter(|type_id| !present.contains(*type_id));
    let first_building = absent.next().expect("an absent building type");
    let second_building = absent.next().expect("a second absent building type");
    let new_vehicle = refresh_vehicles
        .into_iter()
        .find(|type_id| !present.contains(*type_id))
        .expect("an absent vehicle type");
    eprintln!("new types: {first_building}, {second_building}, {new_vehicle}");
    assert!(
        spawn_near(sim, first_building, 6),
        "{first_building} placed"
    );
    assert!(spawn_near(sim, new_vehicle, 6), "{new_vehicle} placed");

    // The check every spawn, death and Limbo runs before any refresh.
    let covers = |sim: &crate::sim::world::Simulation,
                  sprites: &sprite_atlas::SpriteAtlas,
                  units: &unit_atlas::UnitAtlas| {
        let extra = crate::app::frontend::skirmish::deployable_building_types(Some(rules));
        let remaps = sprite_atlas::collect_anim_remap_base_keys(sim);
        let sprites_covered = sprite_atlas::atlas_covers_world(
            Some(sprites),
            sim.entities(),
            &house_colors,
            &extra,
            &remaps,
            Some(&sim.interner),
        );
        let units_covered = unit_atlas::atlas_covers_world(
            Some(units),
            sim.entities(),
            Some(rules),
            Some(&sim.interner),
        );
        (sprites_covered, units_covered)
    };
    let rounds = 20;
    let started = Instant::now();
    let mut covered = (true, true);
    for _ in 0..rounds {
        covered = covers(sim, &sprites, &units);
    }
    eprintln!(
        "coverage check: {:.3} ms per event ({} entities)",
        ms(started.elapsed()) / f64::from(rounds),
        sim.entities().values().count()
    );
    assert!(
        !covered.0,
        "a new building type must request a sprite refresh"
    );
    assert!(!covered.1, "a new vehicle type must request a unit refresh");

    let refresh_sprites =
        |sim: &crate::sim::world::Simulation, atlas: sprite_atlas::SpriteAtlas, label: &str| {
            let started = Instant::now();
            let extra = crate::app::frontend::skirmish::deployable_building_types(Some(rules));
            let remaps = sprite_atlas::collect_anim_remap_base_keys(sim);
            let atlas = sprite_atlas::build_sprite_atlas(
                &device,
                &queue,
                &batch,
                sim.entities(),
                &assets,
                &theater.unit_palette,
                theater_ext,
                &theater_name,
                Some(rules),
                Some(art),
                &house_colors,
                &extra,
                &remaps,
                &cell_drawers,
                Some(&theater.iso_palette),
                Some(atlas),
                Some(&sim.interner),
            )
            .expect("refreshed sprite atlas");
            settle(&device, &queue);
            eprintln!(
                "sprite refresh ({label}): {:.1} ms, {} sprites on {} pages",
                ms(started.elapsed()),
                atlas.sprite_count(),
                atlas.page_count()
            );
            atlas
        };
    let resident_pages = sprites.page_count();
    let before = sprites.sprite_count();
    sprites = refresh_sprites(sim, sprites, "first new building type");
    assert!(sprites.sprite_count() > before);
    let growth_pages = sprites.page_count() - resident_pages;
    assert!(growth_pages <= 1, "one growth page holds one refresh");
    assert!(
        covers(sim, &sprites, &units).0,
        "a refresh satisfies the check"
    );
    let before = sprites.sprite_count();
    sprites = refresh_sprites(sim, sprites, "nothing new");
    assert_eq!(
        sprites.sprite_count(),
        before,
        "resolved keys are never retried"
    );
    assert!(
        spawn_near(sim, second_building, 6),
        "{second_building} placed"
    );
    sprites = refresh_sprites(sim, sprites, "second new building type");

    let started = Instant::now();
    units = unit_atlas::build_unit_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        Some(rules),
        Some(art),
        Some(units),
        Some(&sim.interner),
    )
    .expect("refreshed unit atlas");
    settle(&device, &queue);
    eprintln!(
        "unit refresh (new vehicle type): {:.1} ms, {} sprites on {} pages",
        ms(started.elapsed()),
        units.sprite_count(),
        units.page_count()
    );
    assert_eq!(
        covers(sim, &sprites, &units),
        (true, true),
        "both refreshes satisfy the check"
    );

    for building in [first_building, second_building] {
        assert!(
            sprites
                .get(&ShpSpriteKey {
                    palette_context: ShpPaletteContext::Legacy,
                    type_id: building.to_string(),
                    facing: 0,
                    frame: 0,
                    house_color: owner_color,
                })
                .is_some(),
            "the refreshed sprite atlas draws {building}"
        );
    }
    assert!(
        units
            .get(&UnitSpriteKey {
                type_id: new_vehicle.to_string(),
                facing: 0,
                layer: VxlLayer::Body,
                frame: 0,
                slope_type: 0,
            })
            .is_some(),
        "the refreshed unit atlas draws the new vehicle"
    );

    // The grown atlases must hold exactly what a map load of the same world
    // builds: the same drawable keys and the same frame counts.
    let started = Instant::now();
    let loaded_sprites = sprite_atlas::build_sprite_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        &theater.unit_palette,
        theater_ext,
        &theater_name,
        Some(rules),
        Some(art),
        &house_colors,
        &crate::app::frontend::skirmish::deployable_building_types(Some(rules)),
        &sprite_atlas::collect_anim_remap_base_keys(sim),
        &cell_drawers,
        Some(&theater.iso_palette),
        None,
        Some(&sim.interner),
    )
    .expect("map-load sprite atlas of the grown world");
    settle(&device, &queue);
    eprintln!(
        "map-load sprite atlas of the grown world: {:.0} ms",
        ms(started.elapsed())
    );
    let keys = |atlas: &sprite_atlas::SpriteAtlas| -> HashSet<ShpSpriteKey> {
        atlas.entries_iter().map(|(key, _)| key.clone()).collect()
    };
    let (grown, loaded) = (keys(&sprites), keys(&loaded_sprites));
    assert!(
        grown == loaded,
        "keys only the refreshes drew: {:?}; keys only map load drew: {:?}",
        grown.difference(&loaded).take(8).collect::<Vec<_>>(),
        loaded.difference(&grown).take(8).collect::<Vec<_>>(),
    );
    assert_eq!(
        sprites.active_anim_frame_counts,
        loaded_sprites.active_anim_frame_counts
    );
    assert_eq!(sprites.make_frame_counts, loaded_sprites.make_frame_counts);
    let loaded_units = unit_atlas::build_unit_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        Some(rules),
        Some(art),
        None,
        Some(&sim.interner),
    )
    .expect("map-load unit atlas of the grown world");
    assert_eq!(units.sprite_count(), loaded_units.sprite_count());
}
