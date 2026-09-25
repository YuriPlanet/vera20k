//! Retail timing witness for entity-atlas refreshes during a match.
//!
//! Loads the stock Dustbowl skirmish map, builds the SHP sprite atlas and the
//! voxel unit atlas as map load does, then spawns a building type and a vehicle
//! type the map did not start with and times the refresh each one triggers,
//! plus the coverage check that runs on every spawn, death and Limbo. Timings
//! are printed for the record; the assertions only pin what a refresh must
//! leave drawable.
//!
//! Run (release, so timings resemble the shipped game):
//! RA2_DIR=<retail root> cargo test -p vera20k --lib --release \
//!     retail_atlas_refresh_costs -- --ignored --nocapture

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::assets::asset_manager::AssetManager;
use crate::map::houses::HouseColorMap;
use crate::render::batch::BatchRenderer;
use crate::render::sprite_atlas::{self, ShpPaletteContext, ShpSpriteKey};
use crate::render::unit_atlas::{self, UnitSpriteKey, VxlLayer};
use crate::render::vxl_compute::VxlComputeRenderer;
use crate::rules::house_colors::HouseColorIndex;

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
    let mut compute = VxlComputeRenderer::new(&device);

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
    let cell_drawers = std::collections::HashSet::new();

    let started = Instant::now();
    let extra = crate::app::frontend::skirmish::deployable_building_types(
        sim.entities(),
        Some(rules),
        Some(&sim.interner),
    );
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
        Some(&mut compute),
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

    // A building type and a vehicle type the map did not start with, placed
    // beside the first owned object.
    let anchor = sim
        .entities()
        .values()
        .find(|entity| sim.interner.resolve(entity.owner()) == owner)
        .map(|entity| (entity.position.rx, entity.position.ry))
        .expect("owned anchor");
    let heights = BTreeMap::new();
    let mut spawn_near = |type_id: &str| {
        for (dx, dy) in [(4, 4), (6, 0), (0, 6), (-6, 0), (0, -6), (8, 8), (-8, -8)] {
            let rx = (i32::from(anchor.0) + dx) as u16;
            let ry = (i32::from(anchor.1) + dy) as u16;
            if sim
                .spawn_object(type_id, &owner, rx, ry, 64, rules, &heights)
                .is_some()
            {
                return;
            }
        }
        panic!("could not place {type_id} near {anchor:?}");
    };
    spawn_near("GAPOWR");
    spawn_near("MTNK");

    // The check every spawn, death and Limbo runs before any rebuild.
    let rounds = 20;
    let started = Instant::now();
    let mut unit_rebuild = false;
    let mut sprite_rebuild = false;
    for _ in 0..rounds {
        let needed = unit_atlas::collect_needed_unit_keys(
            sim.entities(),
            &assets,
            Some(rules),
            Some(art),
            Some(&sim.interner),
        );
        unit_rebuild = !units.has_all_keys(&needed);
        let extra = crate::app::frontend::skirmish::deployable_building_types(
            sim.entities(),
            Some(rules),
            Some(&sim.interner),
        );
        let bases = sprite_atlas::collect_needed_base_keys(
            sim.entities(),
            &house_colors,
            &extra,
            Some(&sim.interner),
        );
        let remaps = sprite_atlas::collect_anim_remap_base_keys(sim);
        sprite_rebuild =
            !sprite_atlas::atlas_covers_base_keys(&sprites, &bases, ShpPaletteContext::Legacy)
                || !sprite_atlas::atlas_covers_base_keys(
                    &sprites,
                    &remaps,
                    ShpPaletteContext::SelectedScheme,
                );
    }
    eprintln!(
        "coverage check: {:.2} ms per event ({} entities)",
        ms(started.elapsed()) / f64::from(rounds),
        sim.entities().values().count()
    );
    assert!(
        sprite_rebuild,
        "a new building type must request a sprite refresh"
    );
    assert!(
        unit_rebuild,
        "a new vehicle type must request a unit refresh"
    );

    let started = Instant::now();
    let extra = crate::app::frontend::skirmish::deployable_building_types(
        sim.entities(),
        Some(rules),
        Some(&sim.interner),
    );
    let remaps = sprite_atlas::collect_anim_remap_base_keys(sim);
    sprites = sprite_atlas::build_sprite_atlas(
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
        Some(sprites),
        Some(&sim.interner),
    )
    .expect("refreshed sprite atlas");
    settle(&device, &queue);
    eprintln!(
        "sprite refresh (new building type): {:.0} ms, {} sprites on {} pages",
        ms(started.elapsed()),
        sprites.sprite_count(),
        sprites.page_count()
    );

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
        Some(&mut compute),
        Some(&sim.interner),
    )
    .expect("refreshed unit atlas");
    settle(&device, &queue);
    eprintln!(
        "unit refresh (new vehicle type): {:.0} ms, {} sprites on {} pages",
        ms(started.elapsed()),
        units.sprite_count(),
        units.page_count()
    );

    assert!(
        sprites
            .get(&ShpSpriteKey {
                palette_context: ShpPaletteContext::Legacy,
                type_id: "GAPOWR".to_string(),
                facing: 0,
                frame: 0,
                house_color: owner_color,
            })
            .is_some(),
        "the refreshed sprite atlas draws the new building"
    );
    assert!(
        units
            .get(&UnitSpriteKey {
                type_id: "MTNK".to_string(),
                facing: 0,
                layer: VxlLayer::Body,
                frame: 0,
                slope_type: 0,
            })
            .is_some(),
        "the refreshed unit atlas draws the new vehicle"
    );
}
