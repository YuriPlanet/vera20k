//! Ordinary AnimAI damage through the retained retail readers and Overlay Mark.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};

#[test]
fn ordinary_anim_damage_uses_optional_ares_warhead_and_owner() {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=1000\nArmor=light\n[Animations]\n0=PULSE\n\
         [CombatDamage]\nFlameDamage2=FALLBACK\n[Warheads]\n0=FALLBACK\n1=CUSTOM\n\
         [FALLBACK]\nCellSpread=.3\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CUSTOM]\nCellSpread=.3\n\
         Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\n",
    );
    let art = IniFile::from_str("[PULSE]\nRate=900\nEnd=4\nDamage=10\nWarhead=CUSTOM\n");
    let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("PULSE", 4);
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let invoker = sim.interner.get("Americans").unwrap();
    let mut targets = Vec::new();
    for (x, explicit_owner) in [(20, true), (30, false)] {
        let target = sim
            .spawn_object("TARGET", "Russians", x, 20, 0, &rules)
            .unwrap();
        let coord = crate::sim::movement::ground_pose::position_world_coord(
            &sim.entities().get(target).unwrap().position,
        );
        let world = AnimWorldCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        };
        let (rx, ry, sx, sy, z) = world.to_cell_sub_z();
        let pulse = sim.interner.intern("PULSE");
        let anim = sim
            .spawn_anim_at_world(
                &rules,
                AnimClassSpawnDescriptor::new(pulse, rx, ry, sx, sy, z),
                world,
            )
            .unwrap();
        sim.set_anim_owner_object(anim, Some(target), &rules);
        if explicit_owner {
            sim.substrate.set_anim_owner_house(anim, invoker);
        }
        targets.push(target);
    }
    let (_, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        for _ in 0..6 {
            sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
        }
    });
    assert!(
        !visits.is_empty(),
        "ordinary AnimAI must reach the real area receiver"
    );
    for target in targets {
        let health = sim.entities().get(target).unwrap().health.current();
        #[cfg(feature = "ares")]
        assert_eq!(
            health, 1000,
            "CUSTOM has zero effectiveness, unlike FALLBACK"
        );
        #[cfg(not(feature = "ares"))]
        assert!(
            health < 1000,
            "stock AnimAI keeps its effective FlameDamage2"
        );
    }
    for visit in visits {
        assert_eq!(
            visit["no_source"], true,
            "neither animation owner is an attacker"
        );
        #[cfg(feature = "ares")]
        {
            assert_eq!(visit["warhead"], "CUSTOM");
            assert_eq!(
                visit["no_source_house"], false,
                "explicit and attached owners both propagate"
            );
        }
        #[cfg(not(feature = "ares"))]
        {
            assert_eq!(visit["warhead"], "FALLBACK");
            assert_eq!(visit["no_source_house"], true);
        }
    }
}

struct Scene {
    sim: Simulation,
    rules: RuleSet,
    owner: NativeRulesProcessOwner,
    tree: u64,
    anim: u64,
}

fn retail_scene(extra: &str) -> Option<Scene> {
    let (_, assets) = retail_assets()?;
    let mut owner = retail_rules_owner(&assets);
    let mode = crate::rules::retail_sources::select_ini(&assets, "MPBattleMD.INI").unwrap();
    let mut map = IniFile::from_str(
        "[Animations]\n999=FIRE3\n[OverlayTypes]\n254=VCFIRE\n\
         [VCFIRE]\nLand=Clear\nCellAnim=FIRE3\n",
    );
    map.merge(&IniFile::from_str(extra));
    let (mut rules, _, art, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&mode.ini)),
            &map,
        )
        .unwrap()
        .into_parts();
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
    rules
        .bind_authored_load_anim("FIRE3", &assets, ".sno", "SNOW")
        .unwrap();
    let registry = rules.overlay_registry().clone();
    let mut sim = Simulation::with_seed(0x424647);
    sim.session.set_map_width_for_test(64);
    sim.session.set_map_height_for_test(64);
    sim.set_playfield_bounds_for_test(Some(
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(64, 2, 2, 60, 56),
    ));
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_retail_clear_grid(
        128, 128,
    ));
    *sim.overlay_grid_mut_for_test() = Some(crate::sim::overlay_grid::OverlayGrid::new(128, 128));
    let overlay = registry.id_for_name("VCFIRE").unwrap();
    sim.with_object_placement_scope(|sim| {
        sim.mark_overlay_for_test(&rules, &registry, (40, 86), overlay)
    });
    let anim = *sim.anims().next().unwrap().0;
    // Normal authored ordering: Overlay Mark first; Terrain before Logic AI.
    crate::sim::terrain_spawn::construct_terrain_objects(
        &mut sim,
        &[crate::map::overlay::TerrainObject {
            name: "TREE01".into(),
            rx: 42,
            ry: 88,
        }],
        &rules,
        true,
    );
    let tree = sim.terrain_objects().id_at((42, 88)).unwrap();
    crate::sim::terrain_object::mark_terrain_occupation(
        sim.terrain_objects.get(tree).unwrap(),
        sim.resolved_terrain.as_mut(),
    );
    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert_eq!(sim.live_object_order_snapshot(), [anim]);
    assert_eq!(
        sim.anim_absolute_coord(anim).unwrap(),
        AnimWorldCoord {
            x: 42 * 256,
            y: 88 * 256,
            z: 0
        }
    );
    Some(Scene {
        sim,
        rules,
        owner,
        tree,
        anim,
    })
}

fn advance(scene: &mut Scene, steps: usize) {
    let registry = scene.rules.overlay_registry();
    for _ in 0..steps {
        scene
            .sim
            .advance_tick(&[], Some(&scene.rules), None, Some(registry), 22);
    }
}

#[test]
fn retail_cell_anim_damage_reaches_terrain_only_after_the_due_timer_and_receiver_gates() {
    // Native AnimAI4244B7..424654: due advance, then positive Damage and
    // shared AreaDamage. Kernel489220 truncates falloff before armor: a
    // raw1 pulse at this corner still becomes0 with stock PercentAtMax.5.
    for (extra, lethal) in [
        ("[TREE01]\nStrength=1\n[Fire2]\nCellSpread=1\n", false),
        (
            "[TREE01]\nStrength=1\n[Fire2]\nCellSpread=1\nPercentAtMax=1\n",
            true,
        ),
        (
            "[TREE01]\nStrength=1\n[Fire2]\nCellSpread=1\nPercentAtMax=1\n[CombatDamage]\nFlameDamage2=none\n",
            false,
        ),
    ] {
        let Some(mut scene) = retail_scene(extra) else {
            return;
        };
        advance(&mut scene, 2);
        assert!(
            scene
                .sim
                .terrain_objects()
                .get(scene.tree)
                .unwrap()
                .object_alive()
        );
        advance(&mut scene, 1);
        let alive = scene
            .sim
            .terrain_objects()
            .get(scene.tree)
            .is_some_and(|tree| tree.object_alive());
        assert_eq!(alive, !lethal, "{extra}");
        if lethal {
            assert_eq!(scene.sim.terrain_objects().id_at((42, 88)), None);
            assert!(scene.sim.path_grid().unwrap().is_walkable(42, 88));
        }
        advance(&mut scene, 10);
        assert!(
            scene.sim.anim(scene.anim).is_none(),
            "normal Anim cleanup must still run"
        );
    }
}

#[test]
fn saved_cell_anim_keeps_its_debited_fraction_timer_and_type_reference() {
    let Some(mut scene) = retail_scene("[Fire2]\nCellSpread=1\nPercentAtMax=1\n") else {
        return;
    };
    advance(&mut scene, 3);
    let health = scene
        .sim
        .terrain_objects()
        .get(scene.tree)
        .unwrap()
        .health();
    assert!(
        health
            < scene
                .sim
                .terrain_objects()
                .get(scene.tree)
                .unwrap()
                .max_health()
    );
    let anim = scene.sim.anim(scene.anim).unwrap().clone();
    let grid = scene.sim.resolved_terrain().unwrap().clone();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&scene.sim, 0, 0, "cell-anim", 0);
    let processed =
        crate::util::save_codec::serialize(scene.owner.saved_scenario_rules().unwrap()).unwrap();
    let processed = crate::util::save_codec::deserialize(&processed).unwrap();
    let mut rules = scene
        .owner
        .project_saved_scenario_rules(&processed)
        .unwrap();
    rules.install_art_data(scene.rules.art().clone());
    assert_eq!(
        rules.combat_damage.flame_damage2(),
        scene.rules.combat_damage.flame_damage2()
    );
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(grid, Default::default(), &rules);
    assert_eq!(restored.anim(scene.anim), Some(&anim));
    // Native AnimLoad425280 preserves +188; constructor's1 must not be replayed.
    for _ in 0..10 {
        advance(&mut scene, 1);
        restored.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
        assert_eq!(
            restored.terrain_objects().get(scene.tree).unwrap().health(),
            health
        );
    }
    assert_eq!(
        scene
            .sim
            .terrain_objects()
            .get(scene.tree)
            .unwrap()
            .health(),
        health
    );
    assert!(restored.anim(scene.anim).is_none() && scene.sim.anim(scene.anim).is_none());
}

#[test]
fn no_effects_accumulates_and_next_clears_the_same_retained_owner() {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let Some(mut scene) = retail_scene("[Fire2]\nCellSpread=1\nPercentAtMax=1\n") else {
        return;
    };
    let health = scene
        .sim
        .terrain_objects()
        .get(scene.tree)
        .unwrap()
        .health();
    scene.sim.substrate.set_anim_no_effects(scene.anim, true);
    advance(&mut scene, 3);
    let anim = scene.sim.anim(scene.anim).unwrap();
    assert!(f64::from_bits(anim.damage_accumulator.bits()) > 1.0);
    assert_eq!(
        scene
            .sim
            .terrain_objects()
            .get(scene.tree)
            .unwrap()
            .health(),
        health
    );
    let hash = |anim: &AnimObject| {
        let mut hash = DefaultHasher::new();
        anim.hash(&mut hash);
        hash.finish()
    };
    let mut other_fraction = anim.clone();
    other_fraction.damage_accumulator = NativeF64Bits::ONE;
    assert_ne!(hash(anim), hash(&other_fraction));
    // Exercise the existing Next owner: identity/NoEffects survive; +188 resets.
    scene
        .sim
        .switch_anim_type(scene.anim, "FIRE3", &scene.rules, None);
    let anim = scene.sim.anim(scene.anim).unwrap();
    assert_eq!(anim.damage_accumulator, NativeF64Bits::POSITIVE_ZERO);
    assert!(anim.no_effects());
    advance(&mut scene, 3);
    let carried = f64::from_bits(
        scene
            .sim
            .anim(scene.anim)
            .unwrap()
            .damage_accumulator
            .bits(),
    );
    assert!(carried > 0.0 && carried < 1.0);
    scene.sim.substrate.set_anim_no_effects(scene.anim, false);
    advance(&mut scene, 2);
    assert_eq!(
        scene
            .sim
            .terrain_objects()
            .get(scene.tree)
            .unwrap()
            .health(),
        health
    );
}
