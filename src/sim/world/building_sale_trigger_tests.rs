//! ALL01's authored SpySat_Boom through a physical building sale.
//!
//! This is a Rust composition regression: campaign loading, SELL delivery,
//! build-down, the live Tag callback and its map effects run their ordinary
//! owners. The explored-map prior is supplied through Map::Reveal; the earlier
//! playthrough that explored those cells and whole-match native RNG parity are
//! outside this test. No authored trigger or map actor is replaced.

use super::{DefinitionReference, TagAttachment};
use crate::headless_scenario::HeadlessScenario;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, SharedCellDummy};
use crate::map::tile_variant_selector::TileVariantSelectorCache;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::scenario_bootstrap::{
    FreshScenarioPrefix, NativeStartBounds, ScenarioBootstrapRng,
};
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::world::UninitContext;

const MAP: &str = "ALL01UMD.MAP";
const TAG: &str = "0BE0AB4C";
const TRIGGER: &str = "061FF03C";
const WAYPOINT: (u16, u16) = (53, 69);

fn campaign_scene(map_name: &str) -> HeadlessScenario {
    let (root, mut assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("run with RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1");
    let mut map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, map_name, &assets)
            .expect("physical campaign map")
            .map;
    let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let mut native_rules = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let named = assets
        .resolve_ref(&map_name.replace(".MAP", ".INI"))
        .map(|_| {
            crate::rules::retail_sources::select_ini(&assets, &map_name.replace(".MAP", ".INI"))
                .expect("optional campaign basename INI")
                .ini
        });
    let (mut rules, rules_ini, art_ini, native_rules_receipt) = native_rules
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .expect("campaign Rules process prefix")
        .into_parts();
    let teams = crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(&assets, &map.ini, false)
        .expect("campaign AI definitions");
    let theater =
        crate::map::theater::load_theater(&mut assets, &map.header.theater).expect("ALL01 theater");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let sequences = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini);
    let overlays = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art_ini));
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let lighting = crate::map::lighting::parse_lighting_profiles(&map.ini);
    let bounds = NativeStartBounds::from_map_header(&map.header).expect("ALL01 cell-array bounds");
    map.special_flags.resolve_campaign_defaults(None);
    let descriptor = ScenarioDescriptor {
        seed: 0x44A1_EF,
        map_name: map_name.into(),
        theater: map.header.theater.clone(),
        game_mode_nonzero: false,
        free_radar: map.basic.free_radar.unwrap_or(false),
        ignore_global_ai_triggers: map.basic.ignore_global_ai_triggers.unwrap_or(false),
        train_crate: map.basic.train_crate.unwrap_or(false),
        truck_crate: map.basic.truck_crate.unwrap_or(false),
        ice_growth_enabled: map.basic.ice_growth_enabled,
        vein_growth_enabled: map.basic.vein_growth_enabled,
        tiberium_grows_flag: map.special_flags.tiberium_grows.unwrap(),
        tiberium_spreads_flag: map.special_flags.tiberium_spreads.unwrap(),
        harvester_immune: map.special_flags.harvester_immune.unwrap(),
        map_width: bounds.min_rx + bounds.width,
        map_height: bounds.min_rx + bounds.width,
        lighting: crate::sim::scenario_session::ScenarioLightingState::from_map(&lighting),
        ..Default::default()
    };
    let (mut sim, projection) = ScenarioBootstrapRng::new(descriptor.seed)
        .into_fresh_staged_simulation(
            &descriptor,
            FreshScenarioPrefix::Campaign {
                native_rules_receipt,
                rules: &rules,
                map_data: &map,
                house_roster: &roster,
                difficulty: 1,
                mission_counter: 1,
                game_options: None,
                campaign_index: 0,
                restart_carry_over: crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
            },
        )
        .expect("actual campaign House and current-player construction");
    assert!(
        projection.is_none(),
        "campaign has no skirmish start projection"
    );
    let dummy = SharedCellDummy::fresh();
    dummy.reconstruct_for_map_resize();
    sim.bind_shared_cell_dummy(dummy.clone());
    sim.session.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(
        &map,
        &teams,
        Some(audio.eva()),
        Some(audio.sounds().as_ref()),
    );
    let (mut fill_rng, main_rng) = sim.terrain_load_draws();
    let mut fill_draw = |lo, hi| fill_rng.next_range_u32_inclusive(lo, hi);
    let mut variant_draw = move || main_rng.next_u32();
    let mut cache = TileVariantSelectorCache::default();
    let mut selector = cache.begin_load(&mut variant_draw);
    let terrain_fill =
        ResolvedTerrainGrid::build_pending_authored_with_variant_selector_and_shared_dummy(
            &map,
            Some(&theater),
            Some(&assets),
            Some(&rules.terrain_rules),
            Some(&overlays),
            true,
            rules.general.cliff_back_impassability,
            &mut fill_draw,
            &mut selector,
            dummy,
        );
    drop(variant_draw);
    let mut roots = crate::map::resolved_terrain::scheduler_anim_roots(&rules, &overlays, &[]);
    roots.extend(crate::map::actions::trigger_anim_class_roots(
        &rules,
        &map.actions,
    ));
    rules
        .bind_scheduler_anim_assets(&roots, &assets, theater.extension, &map.header.theater)
        .expect("physical scheduler animation assets");
    let tolerant = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(&tolerant, &assets, theater.extension, &map.header.theater);
    rules.populate_anim_frame_dims(&assets, theater.extension, &map.header.theater);
    rules.bind_effect_assets(&assets, theater.extension, &map.header.theater);
    rules.bind_building_buildup_assets(&assets, &map.header.theater);
    rules.bind_building_rubble_assets(&assets, theater.extension, &map.header.theater);
    rules.bind_terrain_spawner_assets(&rules_ini, &assets, theater.extension, &map.header.theater);
    rules.bind_animation_sequences(&sequences);
    let overlay_shps = crate::rules::effect_asset_catalog::resolved_overlay_shp_ids(
        &overlays,
        &rules_ini,
        rules.art(),
        &assets,
        theater.extension,
        &map.header.theater,
    );
    sim.construct_native_map_tubes(&map.ini)
        .expect("authored Tubes");
    let format = map.basic.new_ini_format.unwrap_or(0);
    let cliff = rules.general.cliff_back_impassability;
    let output = crate::sim::runtime::finalize_and_populate_staged_authored_scenario(
        &mut sim,
        &mut map,
        terrain_fill,
        &theater,
        &assets,
        &mut rules,
        &overlays,
        &overlay_shps,
        format,
        true,
        cliff,
        theater.extension,
        &descriptor.theater,
        crate::map::basic::BridgeDestroyabilityMode::CampaignOrEditor,
        &descriptor,
        |_, _| {}, // FreshScenarioPrefix already constructed the authored House roster.
    )
    .expect("physical terrain, objects and their authored Tags");
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&teams, &rules)
        .expect("resolved stock team AI");
    let post = crate::sim::runtime::finalize_constructed_scenario(
        &mut sim,
        &map,
        &rules,
        &overlays,
        output.overlay_grid,
        &roster,
        None,
    );
    assert!(post.navigation_published && sim.path_grid().is_some());
    sim.session.publish_started();
    sim.hash_state_every(std::num::NonZeroU64::new(10_000).unwrap());
    HeadlessScenario {
        runtime: crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources: crate::sim::runtime::SimResources {
                overlay_registry: overlays,
                terrain_template: None,
                rules,
            },
        },
        map,
    }
}

#[test]
#[ignore = "physical retail ALL01 campaign, authored SpySat_Boom and complete sale"]
fn stock_all01_spy_satellite_sale_reshrouds_then_reveals_waypoint() {
    let mut scene = campaign_scene(MAP);
    let authored = scene
        .map
        .entities
        .iter()
        .find(|entity| entity.type_id == "GASPYSAT")
        .expect("ALL01's map-placed Spy Satellite");
    assert_eq!(authored.owner, "Americans");
    assert_eq!((authored.cell_x, authored.cell_y), (85, 110));
    assert_eq!(authored.trigger_tag.as_deref(), Some(TAG));
    let runtime = &mut scene.runtime;
    let sim = &mut runtime.simulation;
    let rules = &runtime.resources.rules;
    let player = sim.session.current_house().expect("campaign Player");
    assert_eq!(sim.interner.resolve(player), "Americans");
    assert!(!sim.session.game_mode_nonzero());
    assert!(sim.houses[&player].is_controlled_by_human(false));
    assert_eq!(
        sim.session.waypoint_cell(496),
        Some((WAYPOINT.0 as i16, WAYPOINT.1 as i16))
    );
    let id = sim
        .entities()
        .iter_sorted()
        .find_map(|(id, entity)| {
            (sim.interner.resolve(entity.type_ref()) == "GASPYSAT"
                && entity.owner() == player
                && (entity.position.rx(), entity.position.ry()) == (85, 110))
                .then_some(id)
        })
        .expect("constructed authored Spy Satellite");
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(id)];
    assert_eq!(
        sim.trigger_runtime.definitions.tag_types[sim.trigger_runtime.tag(tag).unwrap().type_index]
            .id,
        TAG
    );
    let live = sim
        .trigger_runtime
        .tag(tag)
        .unwrap()
        .triggers
        .iter()
        .copied()
        .find(|&live| {
            let trigger = sim.trigger_runtime.trigger(live).unwrap();
            sim.trigger_runtime.definitions.trigger_types[trigger.type_index].id == TRIGGER
        })
        .expect("actual SpySat_Boom live trigger");
    let trigger = sim.trigger_runtime.trigger(live).unwrap();
    assert!(trigger.enabled);
    let trigger_type = trigger.type_index;
    let definition = &sim.trigger_runtime.definitions.trigger_types[trigger_type];
    assert_eq!(definition.events.len(), 1);
    assert_eq!(definition.events[0].kind, 48);
    assert_eq!(
        definition
            .actions
            .iter()
            .map(|action| action.entry.kind)
            .collect::<Vec<_>>(),
        [51, 18, 54]
    );
    assert_eq!(definition.actions[1].value, Some(496));
    assert_eq!(
        definition.actions[2].reference,
        Some(DefinitionReference::Trigger(trigger_type))
    );
    let expected_actions: Vec<_> = definition
        .actions
        .iter()
        .map(|action| (action.entry.kind, action.value.unwrap_or(0)))
        .collect();
    let object = rules.object("GASPYSAT").unwrap();
    assert_eq!((rules.type_cost(object), object.soylent), (1500, 0));
    let factors = sim.houses[&player].cost_factors();
    let slot = object.factor_slot();
    assert_eq!(
        factors.country[slot],
        crate::util::native_x87::NativeF32Bits::ONE
    );
    assert_eq!(
        factors.factory_plant[slot],
        crate::util::native_x87::NativeF32Bits::ONE
    );
    assert_eq!(rules.general.refund_percent.bits(), 0.5_f64.to_bits());
    assert_eq!(
        crate::sim::production::techno_refund(sim, rules, id),
        Some(750)
    );
    assert!(crate::sim::production::can_sell_building(sim, rules, id));

    // Supply only prior exploration. Select outside current friendly sight and
    // well beyond action18's waypoint reveal, before Map::Reveal touches cells.
    let far = sim
        .resolved_terrain()
        .unwrap()
        .cells()
        .iter()
        .find_map(|cell| {
            let distance = (i32::from(cell.rx) - i32::from(WAYPOINT.0)).abs()
                + (i32::from(cell.ry) - i32::from(WAYPOINT.1)).abs();
            (distance > 60
                && sim.map_cell_in_bounds((cell.rx as i16, cell.ry as i16))
                && !sim.fog.is_cell_visible(player, cell.rx, cell.ry))
            .then_some((cell.rx, cell.ry))
        })
        .expect("allocated cell outside friendly sight and waypoint reveal");
    sim.reveal_entire_map(
        Some(player),
        UninitContext::new(rules, Some(&runtime.resources.overlay_registry)),
    );
    assert!(sim.fog.is_ground_unshrouded(player, far.0, far.1));
    assert!(sim.fog.is_ground_unshrouded(player, WAYPOINT.0, WAYPOINT.1));
    sim.trigger_runtime.action_trace.clear();
    sim.trigger_runtime.dispatch_trace.clear();
    let lost_before = sim.houses[&player].stats().buildings_lost();
    sim.queue_command(CommandEnvelope::new(
        player,
        sim.session.tick(),
        Command::SellBuilding { entity_id: id },
    ));
    let mut saw_selling = false;
    let mut saw_build_down = false;
    let mut sale_credit_delta = None;
    for _ in 0..600 {
        let before = scene.sim().houses[&player].economy().credits();
        let due = scene.runtime.simulation.take_due_commands();
        scene
            .runtime
            .advance_frame_for_tooling(&due, 22)
            .expect("ordinary campaign frame");
        let sim = scene.sim();
        let Some(entity) = sim.entities().get(id) else {
            sale_credit_delta = Some(sim.houses[&player].economy().credits() - before);
            break;
        };
        saw_selling |= entity.mission.current().known() == Some(MissionType::Selling);
        saw_build_down |= entity.building_down();
    }
    let sim = scene.sim();
    assert!(
        saw_selling && saw_build_down,
        "SELL must run ordinary Selling and physical buildup-down"
    );
    assert!(
        !sim.entities().contains(id),
        "physical buildup-down must finish within 600 frames"
    );
    assert_eq!(
        sale_credit_delta,
        Some(750),
        "completion frame pays the actual human refund"
    );
    assert_eq!(
        sim.houses[&player].stats().buildings_lost(),
        lost_before,
        "selling is not a combat loss"
    );
    assert!(
        sim.trigger_runtime.dispatch_trace.contains(&(tag, 48)),
        "sale must raise the actual object Tag's event48"
    );
    assert!(
        sim.trigger_runtime
            .action_trace
            .windows(3)
            .any(|window| window == expected_actions),
        "authored actions51/18/54 must execute in order: {:?}",
        sim.trigger_runtime.action_trace
    );
    assert!(
        !sim.trigger_runtime
            .trigger(live)
            .is_some_and(|trigger| trigger.enabled),
        "SpySat_Boom disables itself before the object attachment retires"
    );
    assert!(
        !sim.fog.is_ground_unshrouded(player, far.0, far.1),
        "action51 reshrouds explored cell {far:?}"
    );
    assert!(
        sim.fog.is_ground_unshrouded(player, WAYPOINT.0, WAYPOINT.1),
        "action18 then reveals authored waypoint496"
    );
}

/// Full retail FullInit/FillInData composition, including authored terrain and
/// structures. The outside-LocalSize player Flak has no initial discovery;
/// Building440D25/440E5C therefore never admits GrandOpening445F80. Its turret
/// also blocks UpdateAnimation451145's independent ready write.
#[test]
#[ignore = "physical retail ALL02 campaign load and discovery"]
fn stock_all02_hidden_player_flak_waits_for_discovery() {
    let mut scene = campaign_scene("ALL02UMD.MAP");
    let player = scene.sim().session.current_house().unwrap();
    let flak_type = scene.runtime.resources.rules.object("NAFLAK").unwrap();
    assert!(flak_type.has_turret);
    assert_eq!(flak_type.sight, 5);
    let find = |scene: &HeadlessScenario, kind: &str, cell: (u16, u16)| {
        scene
            .sim()
            .entities()
            .iter_sorted()
            .find_map(|(id, entity)| {
                (entity.owner() == player
                    && scene.sim().interner.resolve(entity.type_ref()) == kind
                    && (entity.position.rx(), entity.position.ry()) == cell)
                    .then_some(id)
            })
            .expect("authored player building")
    };
    let flak = find(&scene, "NAFLAK", (159, 165));
    let barricade = find(&scene, "CABARR02", (137, 103));
    let entity = scene.sim().entities().get(flak).unwrap();
    assert!(!entity.in_playfield());
    assert!(!entity.discovery.discovered_by_current_house());
    assert!(
        !entity.building_art.actually_placed(),
        "unseen player turret must not GrandOpen"
    );
    assert_eq!(entity.building_ready_latch(), 0);
    assert_eq!(entity.mission.current().raw(), -1);
    assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
    for _ in 0..2 {
        scene
            .runtime
            .advance_frame_for_tooling(&[], 22)
            .expect("ordinary campaign frame");
    }
    let entity = scene.sim().entities().get(flak).unwrap();
    assert!(!entity.building_art.actually_placed());
    assert_eq!(entity.building_ready_latch(), 0);
    assert_eq!(entity.mission.current().raw(), -1);
    assert_eq!(entity.mission.queued().known(), Some(MissionType::Guard));
    // A non-turret reaches the separate UpdateAnimation ready write, even
    // before discovery. Do not hold all hidden human buildings in None.
    assert_eq!(
        scene
            .sim()
            .entities()
            .get(barricade)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Guard)
    );
    let runtime = &mut scene.runtime;
    runtime.simulation.reveal_entire_map(
        Some(player),
        UninitContext::new(
            &runtime.resources.rules,
            Some(&runtime.resources.overlay_registry),
        ),
    );
    let entity = runtime.simulation.entities().get(flak).unwrap();
    assert!(entity.discovery.discovered_by_current_house());
    assert!(
        entity.building_art.actually_placed(),
        "real discovery must run the shared opening"
    );
    assert_ne!(entity.building_ready_latch(), 0);
    runtime
        .advance_frame_for_tooling(&[], 22)
        .expect("first revealed campaign frame");
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(flak)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Guard)
    );
}
