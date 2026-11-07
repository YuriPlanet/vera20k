//! Parsed elapsed Trigger -> Action41 -> constructor-time Anim.Start mineral
//! removal, using stock Battle rules/ART and real scheduler asset bounds.

use super::*;
use crate::map::{
    map_file::MapFile,
    overlay::{OverlayDataPack, OverlayEntry},
};
use crate::sim::{
    overlay_grid::OverlayGrid,
    scenario_session::ScenarioGameMode,
    world::{FrameEffects, TickLane},
};

fn scene(resource_name: &str, map_patch: &str) -> Option<(Simulation, RuleSet)> {
    use crate::rules::retail_ini_fixture::{
        retail_assets, retail_battle_rules_for_map, retail_battle_rules_for_map_with_patch,
    };

    let retail = retail_battle_rules_for_map("XMP03T4.MAP")?;
    let ordinal = retail
        .rules
        .anim_type_art_read_states()
        .iter()
        .position(|(name, _)| name == "TWLT070T")
        .expect("stock AnimType constructor-order catalog contains TWLT070T");
    // Parsed trigger inputs and explicit map-layer overrides only. Geometry
    // is the shared flat arena; fixed retail ART is never modified.
    let map = MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,32,32\nLocalSize=0,0,32,32\n\
             [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
             [Basic]\nNewINIFormat=4\n\
             [Waypoints]\n0=16016\n\
             [Triggers]\nMINERAL=<none>,<none>,Mineral start,0,1,1,1,0\n\
             [Events]\nMINERAL=1,13,0,0\n\
             [Actions]\nMINERAL=1,41,0,{ordinal},0,0,0,0,A\n\
             [Tags]\nMINERAL_TAG=0,Mineral start,MINERAL\n{map_patch}"
        )
        .as_bytes(),
    )
    .unwrap();
    let retail = retail_battle_rules_for_map_with_patch("XMP03T4.MAP", &map.ini).unwrap();
    let mut rules = retail.rules;
    assert_eq!(
        rules.anim_type_name_by_index(ordinal as i32),
        Some("TWLT070T")
    );
    let (_, mut assets) = retail_assets().unwrap();
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::map::actions::trigger_anim_class_roots(&rules, &map.actions);
    rules
        .bind_scheduler_anim_assets(&roots, &assets, "TEM", "TEMPERATE")
        .unwrap();
    let effect_roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(&effect_roots, &assets, "TEM", "TEMPERATE");
    let config = rules.art().anim_runtime_config("TWLT070T").unwrap();
    assert!(config.tiberium_chain_reaction);
    assert!(config.raw_shp_frame_count.is_some_and(|frames| frames > 0));
    let overlays = rules.overlay_registry().clone();
    let ore = overlays.id_for_name(resource_name).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.install_session_mode(ScenarioGameMode::Skirmish);
    sim.publish_scenario_started();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let grid = OverlayGrid::from_generated_overlays(
        &[OverlayEntry {
            rx: 16,
            ry: 16,
            overlay_id: ore,
            frame: 11,
        }],
        &OverlayDataPack::from_cells([(16, 16, 11)]),
        sim.resolved_terrain.as_mut().unwrap(),
        &overlays,
    );
    let view =
        crate::sim::tiberium::tiberium_cell_view(&grid, &overlays, &rules.tiberium_types, (16, 16))
            .unwrap();
    assert_eq!(view.overlay_data, 11);
    crate::sim::runtime::initialize_native_tiberium_queues(
        &mut sim,
        &map.basic,
        &map.special_flags,
        &rules,
        &overlays,
        Some(&grid),
        (32, 32),
    );
    sim.install_overlay_grid_for_load(grid);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    sim.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(&map, &Default::default(), None, None, None);
    sim.install_authored_cell_trigger_tags(&map);
    sim.classify_map_triggers(&rules);
    assert_eq!(sim.anims().count(), 0);
    Some((sim, rules))
}

#[test]
fn elapsed_trigger_anim_removes_ore_before_action41_sets_no_effects() {
    let Some((mut sim, rules)) = scene("TIB01", "") else {
        return;
    };
    let overlays = rules.overlay_registry();
    assert_eq!(rules.combat_damage.tiberium_explosion_damage, 0);
    sim.advance_app_frame(
        &[],
        &rules,
        Some(overlays),
        67,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .unwrap();

    let parent = sim
        .anims()
        .map(|(_, anim)| anim)
        .find(|anim| sim.interner.resolve(anim.type_id()) == "TWLT070T")
        .expect("elapsed Event13 springs the parsed Action41");
    assert!(
        parent.no_effects(),
        "Action41 installs its late NoEffects byte"
    );
    // Start424DB8..424DC0 removes the raw density+1 inside ctor421EA0;
    // Action41's NoEffects store6E2368 occurs only after that ctor returns.
    let cell = sim.overlay_grid().unwrap().cell(16, 16);
    assert_eq!(
        (cell.overlay_id, cell.overlay_data),
        (None, 0),
        "constructor-time Start must remove ore despite Action41's later NoEffects"
    );
}

#[test]
fn gem_child_retains_mineral_conversion_and_nonzero_area_preserves_nearby_ore() {
    // An Ares build draws RandomRanged(0, 99) < Debris.Chance instead; the
    // map's Cruentus ReadINI makes that certain.
    let patch = "[CombatDamage]\nTiberiumExplosionDamage=40\nC4Warhead=HE\n[HE]\nCellSpread=2.5\n";
    #[cfg(feature = "ares")]
    let patch = format!("{patch}[Tiberiums]\n1=Cruentus\n[Cruentus]\nDebris.Chance=100\n");
    let Some((mut sim, rules)) = scene("GEM01", &patch) else {
        return;
    };
    let overlays = rules.overlay_registry();
    let mineral = rules
        .tiberium_types
        .types()
        .iter()
        .find(|ty| ty.section == "Cruentus")
        .unwrap();
    assert_eq!(
        mineral.debris(),
        ["CRYSTAL1", "CRYSTAL2", "CRYSTAL3", "CRYSTAL4"]
    );
    for name in mineral.debris() {
        assert!(
            rules
                .anim_type_art_read_states()
                .iter()
                .any(|(id, read)| id == name && *read)
        );
        let config = rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(
            config.raw_shp_frame_count,
            Some(30),
            "stock CRYSTAL shapes reside in the activated theater archive"
        );
    }
    let ore = overlays.id_for_name("TIB01").unwrap();
    sim.with_object_placement_scope(|sim| {
        sim.mark_overlay_for_test(&rules, overlays, (18, 16), ore)
    });
    let before = sim.overlay_grid().unwrap().cell(18, 16).overlay_data;
    // Static Terrain construction does not consume a Scenario draw before
    // Start's native signed gate (unlike an ordinary Unit constructor).
    crate::sim::terrain_spawn::construct_terrain_objects(
        &mut sim,
        &[crate::map::overlay::TerrainObject {
            name: "TREE01".into(),
            rx: 16,
            ry: 16,
        }],
        &rules,
        false,
    );
    let target = sim.terrain_objects().id_at((16, 16)).unwrap();
    let health = sim.terrain_objects().get(target).unwrap().health();
    let (output, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        sim.advance_app_frame(
            &[],
            &rules,
            Some(overlays),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap()
    });
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0]["damage"], 40);
    assert_eq!(visits[0]["warhead"], "HE");
    assert_eq!(visits[0]["affects_tiberium"], false);
    assert_eq!(visits[0]["no_source"], true);
    assert_eq!(visits[0]["no_source_house"], true);
    assert!(
        sim.terrain_objects().get(target).unwrap().health() < health,
        "false only suppresses resource reduction, not real AreaDamage receivers"
    );
    let cell = sim.overlay_grid().unwrap().cell(18, 16);
    assert_eq!((cell.overlay_id, cell.overlay_data), (Some(ore), before));
    assert!(
        sim.overlay_grid()
            .unwrap()
            .cell(16, 16)
            .overlay_id
            .is_none()
    );
    let (child, anim) = sim
        .anims()
        .find(|(_, anim)| {
            mineral
                .debris()
                .iter()
                .any(|name| name == sim.interner.resolve(anim.type_id()))
        })
        .expect("the admitted native mineral gate constructs real stock Debris");
    assert!(
        !anim.no_effects(),
        "parent Action41 NoEffects is not inherited"
    );
    assert_eq!(
        anim.remap(),
        Some(AnimRemap::ColorScheme(
            crate::rules::house_colors::HouseColorIndex((mineral.color_scheme() / 2) as u8)
        ))
    );
    assert!(output.lighting_events.iter().any(|event| matches!(event,
        crate::sim::light_sources::LightingEvent::RetainAnimIntensity { anim, cell: Some((16, 16)) } if anim == child)));
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "mineral-child", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.anim(*child),
        Some(anim),
        "the saved Anim retains conversion, physics and timers"
    );
    let child = *child;
    let mut landed = false;
    for _ in 0..160 {
        sim.advance_app_frame(
            &[],
            &rules,
            Some(overlays),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
        landed |= sim
            .anims()
            .any(|(_, anim)| sim.interner.resolve(anim.type_id()) == "TWLT026");
    }
    assert!(
        landed,
        "the ordinary stock child reaches its ExpireAnim landing"
    );
    assert!(
        sim.anim(child).is_none(),
        "landed child retires through the shared Anim lifecycle"
    );
}

/// Ares 3.0 10050D50 (hook 0x424DD3) and 10050DA0 (hook 0x424EC5): a zero
/// `Debris.Chance` never throws Debris, and the area damage takes the
/// mineral's `ExplosionDamage`/`ExplosionWarhead` over the Rules pair.
#[cfg(feature = "ares")]
#[test]
fn ares_mineral_start_uses_debris_chance_and_explosion_keys() {
    let Some((mut sim, rules)) = scene(
        "GEM01",
        "[CombatDamage]\nTiberiumExplosionDamage=40\nC4Warhead=Super\n[HE]\nCellSpread=2.5\n\
         [Tiberiums]\n1=Cruentus\n[Cruentus]\nDebris.Chance=0\nExplosionDamage=25\n\
         ExplosionWarhead=HE\n",
    ) else {
        return;
    };
    let overlays = rules.overlay_registry();
    let (_, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        sim.advance_app_frame(
            &[],
            &rules,
            Some(overlays),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap()
    });
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0]["damage"], 25);
    assert_eq!(visits[0]["warhead"], "HE");
    let mineral = rules
        .tiberium_types
        .types()
        .iter()
        .find(|ty| ty.section == "Cruentus")
        .unwrap();
    assert!(
        !sim.anims().any(|(_, anim)| mineral
            .debris()
            .iter()
            .any(|name| name == sim.interner.resolve(anim.type_id()))),
        "Debris.Chance=0 throws no Debris"
    );
}
