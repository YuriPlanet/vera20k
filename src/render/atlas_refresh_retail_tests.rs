//! Retail timing witness for entity-atlas refreshes during a match.
//!
//! Loads the stock Dustbowl skirmish map, builds the SHP sprite atlas and
//! admits the voxel models as map load does, then spawns building types and a
//! vehicle type the map did not start with and grows the SHP atlas through
//! [`EntityAtlasGrowth`] and admits the new voxel model, as a match does. It
//! prints the calling thread's share of each growth (start and publish) and
//! the wall time until it published, plus the coverage check that runs on
//! every spawn, death and Limbo. Timings are printed for the record; the
//! assertions pin what a growth must leave drawable, that it satisfies the
//! coverage check, that nothing new starts no growth, that a cancelled growth
//! publishes nothing, and that the grown atlases hold exactly what a map load
//! of the same world builds.
//!
//! `VERA20K_ATLAS_PRELOAD=voxels` first places one of every vehicle and
//! aircraft type that fits beside the player, so the map-load unit atlas
//! admits nearly every stock voxel model.
//!
//! Run (release, so timings resemble the shipped game):
//! RA2_DIR=<retail root> cargo test -p vera20k --lib --release \
//!     retail_atlas_refresh_costs -- --ignored --nocapture

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::houses::HouseColorMap;
use crate::render::batch::BatchRenderer;
use crate::render::entity_atlas_growth::{EntityAtlasGrowth, GrowthSource};
use crate::render::gpu::block_on;
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
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
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
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
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
    let mut assets =
        AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).expect("retail archives");
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
    let art = rules.art();

    // One remap colour per owner, as the launch session assigns them.
    let mut house_colors: HouseColorMap = HashMap::new();
    for entity in sim.entities().values() {
        let owner = sim.interner().resolve(entity.owner()).to_string();
        let next = HouseColorIndex(house_colors.len() as u8);
        house_colors.entry(owner).or_insert(next);
    }
    // The player's starting vehicle names the house that builds the new types.
    let owner = sim
        .entities()
        .values()
        .find(|entity| entity.is_voxel())
        .map(|entity| sim.interner().resolve(entity.owner()).to_string())
        .expect("the player starts with a vehicle");
    let owner_color = house_colors[&owner];
    // Terrain-attached animations draw from the theater ISO palette, as in the app.
    let cell_drawers: HashSet<String> = sim
        .resolved_terrain()
        .into_iter()
        .flat_map(|terrain| terrain.tile_animations())
        .map(|anim| anim.anim_name.to_ascii_uppercase())
        .collect();
    let anchor = sim
        .entities()
        .values()
        .find(|entity| sim.interner().resolve(entity.owner()) == owner)
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .expect("owned anchor");
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
                        .spawn_object(type_id, &owner, rx as u16, ry as u16, 64, rules)
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
    let extra = sprite_atlas::deployable_building_types(Some(rules));
    let remaps = sprite_atlas::collect_anim_remap_base_keys(sim, &house_colors);
    let sprites = sprite_atlas::build_sprite_atlas(
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
        &Default::default(),
        Some(&theater.iso_palette),
        None,
        Some(sim.interner()),
    )
    .expect("initial sprite atlas");
    settle(&device, &queue);
    eprintln!(
        "initial sprite atlas: {:.0} ms, {} sprites on {} pages",
        ms(started.elapsed()),
        sprites.sprite_count(),
        sprites.page_count()
    );

    let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;
    let admit = |sim: &crate::sim::world::Simulation, units: &mut Option<unit_atlas::UnitAtlas>| {
        unit_atlas::admit_world_models(
            units,
            sim.entities(),
            &assets,
            Some(rules),
            Some(sim.interner()),
            max_texture_dimension_2d,
        );
    };
    let started = Instant::now();
    let mut units = None;
    admit(sim, &mut units);
    let units = units.expect("initial unit atlas");
    let initial_unit_statistics = units.statistics().expect("initial texture descriptors");
    assert_eq!(
        (
            initial_unit_statistics.resident_sprite_count,
            initial_unit_statistics.last_build_rasterized_sprite_count,
            initial_unit_statistics.pages.len()
        ),
        (0, 0, 0),
        "map load draws no voxel picture"
    );
    eprintln!("initial unit admission: {:.1} ms", ms(started.elapsed()));

    // Building types and a vehicle type the map did not start with.
    let present: HashSet<String> = sim
        .entities()
        .values()
        .map(|entity| {
            sim.interner()
                .resolve(entity.type_ref())
                .to_ascii_uppercase()
        })
        .collect();
    let mut absent = ["GAPOWR", "NAPOWR", "GAPILE", "NAHAND", "GAREFN", "NAREFN"]
        .into_iter()
        .filter(|type_id| !present.contains(*type_id));
    let first_building = absent.next().expect("an absent building type");
    let second_building = absent.next().expect("a second absent building type");
    let third_building = absent.next().expect("a third absent building type");
    let new_vehicle = refresh_vehicles
        .into_iter()
        .find(|type_id| !present.contains(*type_id))
        .expect("an absent vehicle type");
    eprintln!("new types: {first_building}, {second_building}, {third_building}, {new_vehicle}");
    assert!(
        spawn_near(sim, first_building, 6),
        "{first_building} placed"
    );
    assert!(spawn_near(sim, new_vehicle, 6), "{new_vehicle} placed");

    // The check every spawn, death and Limbo runs before any refresh.
    let covers = |sim: &crate::sim::world::Simulation,
                  sprites: &sprite_atlas::SpriteAtlas,
                  units: &unit_atlas::UnitAtlas| {
        let extra = sprite_atlas::deployable_building_types(Some(rules));
        let remaps = sprite_atlas::collect_anim_remap_base_keys(sim, &house_colors);
        let sprites_covered = sprite_atlas::atlas_covers_world(
            Some(sprites),
            sim.entities(),
            &house_colors,
            &extra,
            &remaps,
            Some(sim.interner()),
        );
        let units_covered = unit_atlas::atlas_covers_world(
            Some(units),
            sim.entities(),
            Some(rules),
            Some(sim.interner()),
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

    // Grow the atlases as a match does: the SHP atlas prepared on this
    // thread, drawn on a worker and published by a later frame, and the new
    // voxel models admitted on this thread. This thread's share is the start
    // and the publish; the wall time runs until the last publish.
    let start = |growth: &mut EntityAtlasGrowth,
                 sim: &crate::sim::world::Simulation,
                 sprites: &Option<sprite_atlas::SpriteAtlas>,
                 units: &mut Option<unit_atlas::UnitAtlas>| {
        admit(sim, units);
        growth.start(
            &GrowthSource {
                sim,
                rules: Some(rules),
                asset_manager: &assets,
                house_colors: &house_colors,
                theater_name: &theater_name,
                theater_ext,
                max_texture_dimension_2d,
            },
            sprites.as_ref(),
        );
    };
    let grow = |growth: &mut EntityAtlasGrowth,
                sim: &crate::sim::world::Simulation,
                sprites: &mut Option<sprite_atlas::SpriteAtlas>,
                units: &mut Option<unit_atlas::UnitAtlas>,
                label: &str| {
        let began = Instant::now();
        start(growth, sim, sprites, units);
        let mut this_thread = began.elapsed();
        let started = growth.running();
        while growth.running() {
            let publishing = Instant::now();
            if growth.publish(&device, &queue, &batch, sprites, false) {
                this_thread += publishing.elapsed();
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let published = began.elapsed();
        settle(&device, &queue);
        let (sprites, units) = (sprites.as_ref().unwrap(), units.as_ref().unwrap());
        eprintln!(
            "growth ({label}): {:.1} ms on this thread, published after {:.1} ms; \
             {} SHP sprites on {} pages, {} unit sprites on {} pages",
            ms(this_thread),
            ms(published),
            sprites.sprite_count(),
            sprites.page_count(),
            units.sprite_count(),
            units.page_count()
        );
        started
    };
    let mut growth = EntityAtlasGrowth::default();
    let resident_pages = sprites.page_count();
    let before = sprites.sprite_count();
    let (mut sprites, mut units) = (Some(sprites), Some(units));
    assert!(grow(
        &mut growth,
        sim,
        &mut sprites,
        &mut units,
        "first new building and vehicle types"
    ));
    let grown_sprites = sprites.as_ref().unwrap();
    assert!(grown_sprites.sprite_count() > before);
    let growth_pages = grown_sprites.page_count() - resident_pages;
    assert!(growth_pages <= 1, "one growth page holds one refresh");
    assert_eq!(
        covers(sim, grown_sprites, units.as_ref().unwrap()),
        (true, true),
        "both growths satisfy the check"
    );
    let grown_unit_statistics = units
        .as_ref()
        .unwrap()
        .statistics()
        .expect("grown texture descriptors");
    assert_eq!(
        grown_unit_statistics, initial_unit_statistics,
        "admitting a new voxel model draws no picture"
    );

    let before = sprites.as_ref().unwrap().sprite_count();
    assert!(
        !grow(&mut growth, sim, &mut sprites, &mut units, "nothing new"),
        "resolved keys and models start no growth"
    );
    assert_eq!(sprites.as_ref().unwrap().sprite_count(), before);
    assert_eq!(
        units
            .as_ref()
            .unwrap()
            .statistics()
            .expect("no-op texture descriptors"),
        grown_unit_statistics,
        "a no-op retains pages, resident keys and the rasterized count"
    );

    assert!(
        spawn_near(sim, second_building, 6),
        "{second_building} placed"
    );
    assert!(grow(
        &mut growth,
        sim,
        &mut sprites,
        &mut units,
        "second new building type"
    ));

    // A growth cancelled before it publishes, as a map change does, leaves
    // its atlas as it was; the next check grows it again.
    assert!(
        spawn_near(sim, third_building, 6),
        "{third_building} placed"
    );
    start(&mut growth, sim, &sprites, &mut units);
    assert!(growth.running(), "a new building type starts a growth");
    growth.cancel();
    assert!(
        !growth.publish(&device, &queue, &batch, &mut sprites, true),
        "a cancelled growth never publishes"
    );
    assert!(!covers(sim, sprites.as_ref().unwrap(), units.as_ref().unwrap()).0);
    assert!(grow(
        &mut growth,
        sim,
        &mut sprites,
        &mut units,
        "third new building type, after a cancel"
    ));
    let (sprites, mut units) = (sprites.unwrap(), units.unwrap());

    for building in [first_building, second_building, third_building] {
        assert!(
            sprites
                .get(&ShpSpriteKey {
                    palette_context: ShpPaletteContext::Legacy,
                    type_id: building.into(),
                    facing: 0,
                    frame: 0,
                    house_color: owner_color,
                })
                .is_some(),
            "the refreshed sprite atlas draws {building}"
        );
    }
    let new_vehicle_body = UnitSpriteKey {
        type_id: new_vehicle.into(),
        turret_index: 0,
        facing: 0,
        layer: VxlLayer::Body,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let started = Instant::now();
    assert!(
        units.get(&new_vehicle_body).is_some(),
        "the refreshed unit atlas draws the new vehicle"
    );
    units.upload(&device, &queue, &batch);
    settle(&device, &queue);
    eprintln!(
        "first draw of a new vehicle's body: {:.1} ms",
        ms(started.elapsed())
    );
    let drawn_unit_statistics = units.statistics().expect("drawn texture descriptors");
    assert_eq!(drawn_unit_statistics.resident_sprite_count, 1);
    assert_eq!(drawn_unit_statistics.pages.len(), 1);

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
        &sprite_atlas::deployable_building_types(Some(rules)),
        &sprite_atlas::collect_anim_remap_base_keys(sim, &house_colors),
        &cell_drawers,
        &Default::default(),
        Some(&theater.iso_palette),
        None,
        Some(sim.interner()),
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
    // The atlas draws the Buildup the rules bound (`Buildup=` through the
    // theater lookup, `rules::buildup_asset_catalog`), with its frame count.
    for (make_key, &count) in &loaded_sprites.make_frame_counts {
        let type_id = make_key.strip_suffix("_MAKE").expect("a make key");
        assert!(rules.has_buildup(type_id), "{make_key}");
        if !rules.object(type_id).is_some_and(|object| object.gate) {
            assert_eq!(
                i32::from(count),
                rules.buildup_control(type_id)[1],
                "{make_key}"
            );
        }
    }
    let mut buildings = 0;
    for entity in sim.entities().values() {
        let type_id = sim.interner().resolve(entity.type_ref());
        if rules
            .object(type_id)
            .is_some_and(|object| object.category == ObjectCategory::Building)
        {
            buildings += 1;
            assert_eq!(
                loaded_sprites
                    .make_frame_counts
                    .contains_key(&format!("{type_id}_MAKE")),
                rules.has_buildup(type_id),
                "{type_id}"
            );
        }
    }
    eprintln!("buildings with their Buildup checked: {buildings}");
    let mut loaded_units = None;
    admit(sim, &mut loaded_units);
    let loaded_units = loaded_units.expect("map-load unit atlas of the grown world");
    assert!(
        units.admitted_keys() == loaded_units.admitted_keys(),
        "the admissions hold the keys a map load of the same world admits"
    );
    assert_eq!(units.frame_counts, loaded_units.frame_counts);
}
