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
//! `retail_campaign_preload_covers_the_authored_match` is the GPU-free half: it
//! only compares voxel-model *demand* sets, so it runs without a wgpu adapter
//! and answers whether the load-time preload (world plus authored TaskForce
//! rosters) already holds every model a real campaign match later draws.
//!
//! Run (release, so timings resemble the shipped game):
//! RA2_DIR=<retail root> cargo test -p vera20k --lib --release \
//!     retail_atlas_refresh_costs -- --ignored --nocapture
//! Set `VERA20K_ATLAS_REFRESH_MAP` to use another installed skirmish map.
//!
//! RA2_DIR=<retail root> cargo test -p vera20k --lib --release \
//!     retail_campaign_preload_covers_the_authored_match -- --ignored --nocapture

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::houses::HouseColorMap;
use crate::render::batch::BatchRenderer;
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

/// One house colour per registered House, as the launch session assigns them.
fn house_colors_for(sim: &crate::sim::world::Simulation) -> HouseColorMap {
    sim.houses()
        .values()
        .enumerate()
        .map(|(index, house)| {
            (
                sim.interner().resolve(house.name()).to_string(),
                HouseColorIndex(u8::try_from(index).unwrap()),
            )
        })
        .collect()
}

/// The `(type, colour)` SHP pairs the load-time manifest preloads: placed
/// objects, every deploy target in every colour, and each participating
/// Country's own non-voxel types in the colour of the House that builds them.
fn load_time_shp_pairs(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    house_colors: &HouseColorMap,
) -> std::collections::BTreeSet<(String, u8)> {
    let extra = crate::app::frontend::skirmish::deployable_building_types(Some(rules));
    let mut pairs: std::collections::BTreeSet<(String, u8)> =
        sprite_atlas::object_pairs(sim.entities(), house_colors, &extra, Some(sim.interner()))
            .map(|(type_id, color)| (type_id.to_ascii_uppercase(), color.0))
            .collect();
    for country in crate::sim::ai_buildable::participating_countries(
        sim.houses(),
        rules,
        sim.interner(),
    ) {
        let colors: Vec<HouseColorIndex> = sim
            .houses()
            .values()
            .filter(|house| sim.interner().resolve(house.house_type_id()) == country.name)
            .filter_map(|house| {
                house_colors
                    .get(sim.interner().resolve(house.name()))
                    .copied()
            })
            .collect();
        for type_id in crate::sim::ai_buildable::producible_shp_types_for_country(
            country.masks,
            country.side_index,
            rules,
            sim.session().game_mode_nonzero(),
        ) {
            for &color in &colors {
                pairs.insert((type_id.to_ascii_uppercase(), color.0));
            }
        }
    }
    pairs
}

#[test]
#[ignore = "requires retail archives and a wgpu adapter; prints atlas refresh timings"]
fn retail_atlas_refresh_costs() {
    let root = retail_root();
    let map = std::env::var("VERA20K_ATLAS_REFRESH_MAP")
        .unwrap_or_else(|_| "Dustbowl.mmx".to_owned());
    let mut scenario = crate::headless_scenario::load(&root, &map, 0x0B21_D6E5)
        .unwrap_or_else(|error| panic!("{map} loads: {error}"));
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
    let extra = crate::app::frontend::skirmish::deployable_building_types(Some(rules));
    let remaps = sprite_atlas::collect_anim_remap_base_keys(sim, &house_colors);
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

    let started = Instant::now();
    let mut units = unit_atlas::build_unit_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        Some(rules),
        None,
        Some(sim.interner()),
    )
    .expect("initial unit atlas");
    settle(&device, &queue);
    let initial_unit_statistics = units.statistics().expect("initial texture descriptors");
    assert!(initial_unit_statistics.resident_sprite_count > 0);
    assert!(initial_unit_statistics.total_texel_payload_bytes > 0);
    eprintln!(
        "initial unit statistics: {}",
        serde_json::to_string(&initial_unit_statistics).unwrap()
    );
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

    let refresh_sprites =
        |sim: &crate::sim::world::Simulation, atlas: sprite_atlas::SpriteAtlas, label: &str| {
            let started = Instant::now();
            let extra = crate::app::frontend::skirmish::deployable_building_types(Some(rules));
            let remaps = sprite_atlas::collect_anim_remap_base_keys(sim, &house_colors);
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
                &Default::default(),
                Some(&theater.iso_palette),
                Some(atlas),
                Some(sim.interner()),
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
        Some(units),
        Some(sim.interner()),
    )
    .expect("refreshed unit atlas");
    settle(&device, &queue);
    let grown_unit_statistics = units.statistics().expect("grown texture descriptors");
    assert!(
        grown_unit_statistics.resident_sprite_count > initial_unit_statistics.resident_sprite_count
    );
    assert!(
        grown_unit_statistics
            .pages
            .starts_with(&initial_unit_statistics.pages),
        "growth preserves resident page resources"
    );
    assert!(
        grown_unit_statistics.total_texel_payload_bytes
            > initial_unit_statistics.total_texel_payload_bytes,
        "new model demand allocates a growth page"
    );
    eprintln!(
        "grown unit statistics: {}",
        serde_json::to_string(&grown_unit_statistics).unwrap()
    );
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
    units = unit_atlas::build_unit_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        Some(rules),
        Some(units),
        Some(sim.interner()),
    )
    .expect("unit atlas no-op refresh");
    assert_eq!(
        units.statistics().expect("no-op texture descriptors"),
        grown_unit_statistics,
        "a no-op retains pages, resident keys and the last actual build count"
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
                turret_index: 0,
                facing: 0,
                layer: VxlLayer::Body,
                frame: 0,
                slope_type: 0,
                barrel_pitch: 0,
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
    let loaded_units = unit_atlas::build_unit_atlas(
        &device,
        &queue,
        &batch,
        sim.entities(),
        &assets,
        Some(rules),
        None,
        Some(sim.interner()),
    )
    .expect("map-load unit atlas of the grown world");
    assert_eq!(units.sprite_count(), loaded_units.sprite_count());
    let loaded_unit_statistics = loaded_units
        .statistics()
        .expect("fresh texture descriptors");
    assert_eq!(
        grown_unit_statistics.resident_sprite_count,
        loaded_unit_statistics.resident_sprite_count
    );
    // A fresh pack and append-only growth have different packing histories;
    // neither texture allocation is an estimate of the other's resource use.
    eprintln!(
        "fresh unit statistics: {}",
        serde_json::to_string(&loaded_unit_statistics).unwrap()
    );
}

/// Does the load-time preload hold every voxel model a real campaign match
/// later draws?
///
/// A device log can only show that a refresh happened; this shows *what* the
/// atlas could not have known at load. Demand is pure rules/entity data, so
/// this needs no GPU: it replays the same
/// `UnitAtlasDemand::of_world` + `of_authored_teams` union the app builds at
/// map load, then advances the scenario and reports every model that first
/// appears mid-match. Empty means the preloaded atlas never grows.
#[test]
#[ignore = "requires retail archives; reports campaign preload coverage without a GPU"]
fn retail_campaign_preload_covers_the_authored_match() {
    // ~5 minutes of play by default; `VERA20K_PRELOAD_TICKS` shortens a first pass.
    const DEFAULT_TICKS: u32 = 4_500;
    let ticks = std::env::var("VERA20K_PRELOAD_TICKS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_TICKS);
    let root = retail_root();
    let mut scenario = crate::headless_scenario::load(&root, "SOV01UMD.MAP", 339_173_046)
        .expect("retail SOV01 campaign scenario loads");
    // Destructure so the frame call and the rules reader borrow disjoint
    // fields of the same runtime.
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;

    // The two demand readers need the simulation's own interner to name types,
    // so each pass borrows the runtime, then releases it before the next frame.
    let (preload, world_at_load, teams_at_load, rosters_at_load) = {
        let mut preload = unit_atlas::UnitAtlasDemand::of_world(
            sim.entities(),
            Some(rules),
            Some(sim.interner()),
        );
        let world_at_load = preload.len();
        let teams = unit_atlas::UnitAtlasDemand::of_authored_teams(
            sim.team_script_vm(),
            rules,
            sim.interner(),
        );
        let rosters = unit_atlas::UnitAtlasDemand::of_participating_houses(
            sim.houses(),
            rules,
            sim.interner(),
            sim.session().game_mode_nonzero(),
        );
        let teams_at_load = teams.len();
        let rosters_at_load = rosters.len();
        preload.extend(teams);
        preload.extend(rosters);
        (preload, world_at_load, teams_at_load, rosters_at_load)
    };
    eprintln!(
        "load-time demand: {world_at_load} world + {teams_at_load} authored team + \
         {rosters_at_load} country buildable model(s) = {} unique",
        preload.len()
    );
    let house_colors = house_colors_for(sim);
    let mut shp_current = load_time_shp_pairs(sim, rules, &house_colors);
    let shp_at_load = shp_current.len();
    let mut shp_late: std::collections::BTreeMap<String, u32> = Default::default();
    let tick_ms = crate::headless_scenario::SIM_TICK_MS;
    let mut late: std::collections::BTreeMap<String, u32> = Default::default();
    let mut current = preload;
    let mut advanced = 0u32;
    // Teeth for the "covered" verdict: the world must actually grow during the
    // replay, or a match that never advanced would also report full coverage.
    let mut entities_at_load = 0usize;
    let mut models_at_load = 0usize;
    let tick_at_load = sim.session().tick();
    for tick in 1..=ticks {
        if sim.advance_app_frame(
            &[],
            rules,
            Some(&resources.overlay_registry),
            tick_ms,
            crate::sim::world::TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .is_err()
        {
            break;
        }
        advanced += 1;
        let now = unit_atlas::UnitAtlasDemand::of_world(
            sim.entities(),
            Some(rules),
            Some(sim.interner()),
        );
        if tick == 1 {
            entities_at_load = sim.entities().values().count();
            models_at_load = now.len();
        }
        let grown = now.without(&current);
        if !grown.is_empty() {
            for model in grown.type_ids() {
                late.entry(model.to_owned()).or_insert(tick);
            }
            current.extend(now);
        }
        let shp_now = load_time_shp_pairs(sim, rules, &house_colors);
        if !shp_now.is_subset(&shp_current) {
            for (type_id, color) in shp_now.difference(&shp_current) {
                shp_late
                    .entry(format!("{type_id}@{color}"))
                    .or_insert(tick);
            }
            shp_current = shp_now;
        }
    }
    eprintln!("advanced {advanced}/{ticks} tooling frames at {tick_ms} ms");
    eprintln!(
        "SHP object pairs: {shp_at_load} at load, {} after the replay; \
         {} first appear mid-match",
        shp_current.len(),
        shp_late.len()
    );
    for (pair, tick) in shp_late.iter().take(40) {
        eprintln!(
            "mid-match SHP pair: {pair} (first at tick {tick}, ~{} s)",
            u64::from(*tick) * u64::from(tick_ms) / 1000
        );
    }
    let models_end = unit_atlas::UnitAtlasDemand::of_world(
        sim.entities(),
        Some(rules),
        Some(sim.interner()),
    )
    .len();
    let entities_end = sim.entities().values().count();
    eprintln!(
        "world grew: {entities_at_load}->{entities_end} entities, \
         {models_at_load}->{models_end} voxel models in the world"
    );
    assert!(
        advanced == ticks && sim.session().tick() > tick_at_load,
        "the replay did not advance: {advanced}/{ticks} frames, tick {}->{}",
        tick_at_load,
        sim.session().tick()
    );
    if late.is_empty() {
        eprintln!(
            "preload covers the match: no voxel model first appears after load that the \
             world-at-load plus authored rosters do not already name"
        );
    } else {
        for (model, tick) in &late {
            eprintln!(
                "first appears at tick {tick} (~{} s): {model}",
                u64::from(*tick) * u64::from(tick_ms) / 1000
            );
        }
        eprintln!("{} model(s) the load-time preload does not cover", late.len());
    }
    assert!(
        late.is_empty(),
        "models the load-time atlas cannot hold: {:?}",
        late.keys().collect::<Vec<_>>()
    );
    assert!(
        shp_late.is_empty(),
        "SHP pairs the load-time atlas cannot hold: {:?}",
        shp_late.keys().collect::<Vec<_>>()
    );
}

/// How many sprites the load-time voxel bake holds before and after the
/// participating Houses' country rosters join the demand.
///
/// This is the load-time side of the trade: every model the preload admits is
/// rasterized on the loading screen instead of mid-match. CPU-side only, so it
/// answers on any machine and prints the size the Switch has to upload.
#[test]
#[ignore = "requires retail archives; reports the load-time voxel bake size without a GPU"]
fn retail_skirmish_preload_bake_size() {
    let root = retail_root();
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};
    let session = crate::headless_scenario::battle_session(
        "XMP03T4.MAP",
        &[
            (LaunchCountry::Russia, AiDifficulty::Hard),
            (LaunchCountry::France, AiDifficulty::Normal),
            (LaunchCountry::Korea, AiDifficulty::Easy),
        ],
    );
    let mut scenario =
        crate::headless_scenario::load_session(&root, "XMP03T4.MAP", 0x0B21_D6E5, session)
            .expect("retail YMP3T4 skirmish scenario loads");
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;
    let assets =
        AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).expect("retail archives");

    let mut world = unit_atlas::UnitAtlasDemand::of_world(
        sim.entities(),
        Some(rules),
        Some(sim.interner()),
    );
    let world_models = world.len();
    world.extend(unit_atlas::UnitAtlasDemand::of_authored_teams(
        sim.team_script_vm(),
        rules,
        sim.interner(),
    ));
    let teams_models = world.len();
    let (team_keys, _) = unit_atlas::needed_unit_keys(&world, &assets, Some(rules));
    world.extend(unit_atlas::UnitAtlasDemand::of_participating_houses(
        sim.houses(),
        rules,
        sim.interner(),
        sim.session().game_mode_nonzero(),
    ));
    let (roster_keys, _) = unit_atlas::needed_unit_keys(&world, &assets, Some(rules));
    eprintln!(
        "world + authored teams: {teams_models} model(s) ({world_models} world), \
         {} sprite key(s)",
        team_keys.len()
    );
    eprintln!(
        "full load preload: {} model(s), {} sprite key(s) (+{} keys, +{:.1}%)",
        world.len(),
        roster_keys.len(),
        roster_keys.len() - team_keys.len(),
        (roster_keys.len() - team_keys.len()) as f64 * 100.0 / team_keys.len() as f64
    );
}

/// Time the load-time voxel bake with and without the participating Houses'
/// buildable rosters. The difference is what the loading screen pays once, in
/// exchange for the growth pages a match no longer makes mid-game.
#[test]
#[ignore = "requires retail archives and a wgpu adapter; times the load-time voxel bake"]
fn retail_skirmish_preload_bake_cost() {
    let root = retail_root();
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};
    let session = crate::headless_scenario::battle_session(
        "XMP03T4.MAP",
        &[
            (LaunchCountry::Russia, AiDifficulty::Hard),
            (LaunchCountry::France, AiDifficulty::Normal),
            (LaunchCountry::Korea, AiDifficulty::Easy),
        ],
    );
    let mut scenario =
        crate::headless_scenario::load_session(&root, "XMP03T4.MAP", 0x0B21_D6E5, session)
            .expect("retail YMP3T4 skirmish scenario loads");
    let mut assets =
        AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).expect("retail archives");
    let theater_name = scenario.map.header.theater.clone();
    crate::map::theater::load_theater(&mut assets, &theater_name).expect("theater loads");
    let (device, queue) = headless_device();
    let batch =
        BatchRenderer::new_with_device(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);

    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;

    let teams_demand = || {
        let mut demand = unit_atlas::UnitAtlasDemand::of_world(
            sim.entities(),
            Some(rules),
            Some(sim.interner()),
        );
        demand.extend(unit_atlas::UnitAtlasDemand::of_authored_teams(
            sim.team_script_vm(),
            rules,
            sim.interner(),
        ));
        demand
    };
    let rosters = unit_atlas::UnitAtlasDemand::of_participating_houses(
        sim.houses(),
        rules,
        sim.interner(),
        sim.session().game_mode_nonzero(),
    );
    let roster_models = rosters.len();
    let teams_models = teams_demand().len();
    let full = {
        let mut demand = teams_demand();
        demand.extend(unit_atlas::UnitAtlasDemand::of_participating_houses(
            sim.houses(),
            rules,
            sim.interner(),
            sim.session().game_mode_nonzero(),
        ));
        demand
    };

    for (label, demand) in [
        ("world + authored teams", teams_demand()),
        ("full load preload", full),
    ] {
        let started = Instant::now();
        let atlas = unit_atlas::build_unit_atlas_with_demand(
            &device,
            &queue,
            &batch,
            sim.entities(),
            &assets,
            Some(rules),
            None,
            Some(sim.interner()),
            Some(demand),
        )
        .expect("load-time unit atlas");
        settle(&device, &queue);
        let statistics = atlas.statistics().expect("texture descriptors");
        eprintln!(
            "{label}: {:.0} ms, {} sprites on {} pages, {} texel bytes",
            ms(started.elapsed()),
            atlas.sprite_count(),
            atlas.page_count(),
            statistics.total_texel_payload_bytes,
        );
    }
    eprintln!("({teams_models} authored-team models + {roster_models} roster models)");
}

/// Time the load-time SHP bake with and without the participating Houses'
/// buildable non-voxel types. The roster list is crossed with every House
/// colour, which is the conservative superset the atlas already uses for
/// deploy targets.
#[test]
#[ignore = "requires retail archives and a wgpu adapter; times the load-time SHP bake"]
fn retail_skirmish_shp_preload_bake_cost() {
    let root = retail_root();
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};
    let session = crate::headless_scenario::battle_session(
        "XMP03T4.MAP",
        &[
            (LaunchCountry::Russia, AiDifficulty::Hard),
            (LaunchCountry::France, AiDifficulty::Normal),
            (LaunchCountry::Korea, AiDifficulty::Easy),
        ],
    );
    let mut scenario =
        crate::headless_scenario::load_session(&root, "XMP03T4.MAP", 0x0B21_D6E5, session)
            .expect("retail YMP3T4 skirmish scenario loads");
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
    let mut house_colors: HouseColorMap = HashMap::new();
    for house in sim.houses().values() {
        let next = HouseColorIndex(house_colors.len() as u8);
        house_colors
            .entry(sim.interner().resolve(house.name()).to_string())
            .or_insert(next);
    }
    let cell_drawers: HashSet<String> = sim
        .resolved_terrain()
        .into_iter()
        .flat_map(|terrain| terrain.tile_animations())
        .map(|anim| anim.anim_name.to_ascii_uppercase())
        .collect();
    let remaps = sprite_atlas::collect_anim_remap_base_keys(sim, &house_colors);
    let frozen = sprite_atlas::collect_fogged_sprite_keys(sim, rules, &house_colors);
    let deployable = crate::app::frontend::skirmish::deployable_building_types(Some(rules));
    let mut country_pairs: Vec<(String, HouseColorIndex)> = Vec::new();
    for country in crate::sim::ai_buildable::participating_countries(
        sim.houses(),
        rules,
        sim.interner(),
    ) {
        let colors: Vec<HouseColorIndex> = sim
            .houses()
            .values()
            .filter(|house| sim.interner().resolve(house.house_type_id()) == country.name)
            .filter_map(|house| {
                house_colors
                    .get(sim.interner().resolve(house.name()))
                    .copied()
            })
            .collect();
        for type_id in crate::sim::ai_buildable::producible_shp_types_for_country(
            country.masks,
            country.side_index,
            rules,
            sim.session().game_mode_nonzero(),
        ) {
            for &color in &colors {
                country_pairs.push((type_id.to_owned(), color));
            }
        }
    }
    country_pairs.sort_unstable_by(|a, b| (&a.0, a.1.0).cmp(&(&b.0, b.1.0)));
    country_pairs.dedup();

    for (label, pairs) in [
        ("deploy targets only", Vec::new()),
        ("country roster pairs", country_pairs),
    ] {
        let started = Instant::now();
        let atlas = sprite_atlas::build_sprite_atlas_with_pairs(
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
            &deployable,
            &pairs,
            &remaps,
            &cell_drawers,
            &frozen,
            Some(&theater.iso_palette),
            None,
            Some(sim.interner()),
        )
        .expect("load-time sprite atlas");
        settle(&device, &queue);
        eprintln!(
            "{label}: {:.0} ms, {} sprites on {} pages ({} extra pairs)",
            ms(started.elapsed()),
            atlas.sprite_count(),
            atlas.page_count(),
            pairs.len(),
        );
    }
}

/// The same coverage question for a skirmish, where the demand source is AI
/// *production* rather than authored TaskForce rosters.
///
/// Reports — without failing — every voxel model a computer builds after load,
/// because those are exactly the models the load-time atlas can only learn
/// about by guessing. The result decides whether skirmish load must pre-bake a
/// country's buildable voxel roster in addition to the authored teams.
#[test]
#[ignore = "requires retail archives; reports skirmish preload coverage without a GPU"]
fn retail_skirmish_preload_coverage() {
    const DEFAULT_TICKS: u32 = 6_000;
    let ticks = std::env::var("VERA20K_PRELOAD_TICKS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_TICKS);
    let root = retail_root();
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};
    let session = crate::headless_scenario::battle_session(
        "XMP03T4.MAP",
        &[
            (LaunchCountry::Russia, AiDifficulty::Hard),
            (LaunchCountry::France, AiDifficulty::Normal),
            (LaunchCountry::Korea, AiDifficulty::Easy),
        ],
    );
    let mut scenario =
        crate::headless_scenario::load_session(&root, "XMP03T4.MAP", 0x0B21_D6E5, session)
            .expect("retail YMP3T4 skirmish scenario loads");
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;

    // The load-time demand the app builds: world plus authored TaskForces plus
    // the participating Houses' buildable country rosters.
    let (preload, world_size, teams_size, roster_size) = {
        let mut preload = unit_atlas::UnitAtlasDemand::of_world(
            sim.entities(),
            Some(rules),
            Some(sim.interner()),
        );
        let world_size = preload.len();
        let teams = unit_atlas::UnitAtlasDemand::of_authored_teams(
            sim.team_script_vm(),
            rules,
            sim.interner(),
        );
        let rosters = unit_atlas::UnitAtlasDemand::of_participating_houses(
            sim.houses(),
            rules,
            sim.interner(),
            sim.session().game_mode_nonzero(),
        );
        let teams_size = teams.len();
        let roster_size = rosters.len();
        preload.extend(teams);
        preload.extend(rosters);
        (preload, world_size, teams_size, roster_size)
    };
    eprintln!(
        "skirmish load-time demand: {world_size} world + {teams_size} authored team + \
         {roster_size} country buildable model(s) = {} unique",
        preload.len()
    );

    // The SHP half of the same question: every (type, colour) pair the sprite
    // atlas must hold. A pair that first appears after load is another
    // mid-match growth page.
    let house_colors = house_colors_for(sim);
    let mut shp_current = load_time_shp_pairs(sim, rules, &house_colors);
    let shp_at_load = shp_current.len();
    let mut shp_late: std::collections::BTreeMap<String, u32> = Default::default();

    let tick_ms = crate::headless_scenario::SIM_TICK_MS;
    let mut late: std::collections::BTreeMap<String, u32> = Default::default();
    let mut current = preload;
    let mut advanced = 0u32;
    for tick in 1..=ticks {
        if sim.advance_app_frame(
            &[],
            rules,
            Some(&resources.overlay_registry),
            tick_ms,
            crate::sim::world::TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .is_err()
        {
            break;
        }
        advanced += 1;
        let now = unit_atlas::UnitAtlasDemand::of_world(
            sim.entities(),
            Some(rules),
            Some(sim.interner()),
        );
        let grown = now.without(&current);
        if !grown.is_empty() {
            for model in grown.type_ids() {
                late.entry(model.to_owned()).or_insert(tick);
            }
            current.extend(now);
        }
        let shp_now = load_time_shp_pairs(sim, rules, &house_colors);
        if !shp_now.is_subset(&shp_current) {
            for (type_id, color) in shp_now.difference(&shp_current) {
                shp_late
                    .entry(format!("{type_id}@{color}"))
                    .or_insert(tick);
            }
            shp_current = shp_now;
        }
    }
    eprintln!(
        "advanced {advanced}/{ticks} frames at {tick_ms} ms; {} entities",
        sim.entities().values().count()
    );
    eprintln!(
        "SHP object pairs: {shp_at_load} at load, {} after the replay; \
         {} first appear mid-match",
        shp_current.len(),
        shp_late.len()
    );
    for (pair, tick) in shp_late.iter().take(40) {
        eprintln!(
            "mid-match SHP pair: {pair} (first at tick {tick}, ~{} s)",
            u64::from(*tick) * u64::from(tick_ms) / 1000
        );
    }
    for (model, tick) in &late {
        eprintln!(
            "mid-match voxel model: {model} (first at tick {tick}, ~{} s)",
            u64::from(*tick) * u64::from(tick_ms) / 1000
        );
    }
    eprintln!(
        "{} voxel model(s) first appear after load: {}",
        late.len(),
        late.keys().cloned().collect::<Vec<_>>().join(",")
    );
    assert!(
        late.is_empty(),
        "models the load-time atlas cannot hold: {:?}",
        late.keys().collect::<Vec<_>>()
    );
    assert!(
        shp_late.is_empty(),
        "SHP pairs the load-time atlas cannot hold: {:?}",
        shp_late.keys().collect::<Vec<_>>()
    );
}
