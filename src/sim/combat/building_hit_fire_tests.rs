//! Building44270B hit fires through retail Rules, the receiver and ordinary Anim.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::snapshot::GameSnapshot;

fn scene(sparky: bool, on_fire: &str) -> Option<(Simulation, RuleSet, u64)> {
    let patch = IniFile::from_str(&format!(
        "[AudioVisual]\nOnFire={on_fire}\n[AP]\nSparky={sparky}\n"
    ));
    let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &patch,
    )?
    .rules;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    let mut world = crate::sim::arena_fixture::rival_arena(&rules, 31);
    world.intern_rule_type_ids(&rules);
    let building = world
        .spawn_object("GAPOWR", "Russians", 16, 16, 0, &rules)
        .unwrap();
    world.resolve_type_handles(&rules);
    // Make the footprint's floor distinct from the already placed building.
    // This is an authored terrain change, not an expected native output.
    for (dx, dy) in rules.object("GAPOWR").unwrap().foundation.cell_offsets() {
        world
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test((16 + dx) as u16, (16 + dy) as u16)
            .unwrap()
            .level = 2;
    }
    Some((world, rules, building))
}

fn hit(world: &mut Simulation, rules: &RuleSet, building: u64, amount: i32) {
    let warhead = world.interner.intern("AP");
    let event = EntityDamageEvent::direct_receiver(
        building,
        amount,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    commit_entities(
        world,
        &mut ReceiverRun::default(),
        &[event],
        None,
        rules,
        Some(rules.overlay_registry()),
    );
}

fn cross_yellow(world: &mut Simulation, rules: &RuleSet, building: u64) {
    let strength = world.entities().get(building).unwrap().health.current();
    hit(world, rules, building, strength - strength / 2 + 1);
}

fn fire_ids(world: &Simulation, rules: &RuleSet) -> Vec<u64> {
    world
        .anims()
        .filter(|(_, anim)| {
            rules
                .general
                .on_fire
                .iter()
                .any(|name| name.eq_ignore_ascii_case(world.interner.resolve(anim.type_id())))
        })
        .map(|(&id, _)| id)
        .collect()
}

#[test]
fn sparky_hit_uses_the_foundation_floor_and_does_not_repeat_without_a_crossing() {
    let Some((mut world, rules, building)) = scene(true, "FIRE01,FIRE02,FIRE03") else {
        return;
    };
    cross_yellow(&mut world, &rules, building);
    let fires = fire_ids(&world, &rules);
    assert!(
        !fires.is_empty(),
        "the admitted Sparky hit must create fire"
    );
    for id in &fires {
        let fire = world.anim(*id).unwrap();
        assert!(["FIRE01", "FIRE02", "FIRE03"].contains(&world.interner.resolve(fire.type_id())));
        assert_eq!(fire.owner_entity(), Some(building));
        assert_eq!(fire.draw_flags(), 0x600);
        assert_eq!(fire.z_adjust(), 0);
        assert_eq!(fire.damage_fire_slot(), None);
        assert_eq!(world.anim_absolute_coord(*id).unwrap().z, 208);
    }
    let rng = world.scenario_rng.logical_state();
    hit(&mut world, &rules, building, 1);
    assert_eq!(fire_ids(&world, &rules), fires);
    assert_eq!(world.scenario_rng.logical_state(), rng);
    let health = world.entities().get(building).unwrap().health.current();
    hit(&mut world, &rules, building, health - 1);
    assert!(
        fire_ids(&world, &rules).len() > fires.len(),
        "the red crossing also creates fire"
    );

    let (mut plain, rules, building) = scene(false, "FIRE01,FIRE02,FIRE03").unwrap();
    let rng = plain.scenario_rng.logical_state();
    cross_yellow(&mut plain, &rules, building);
    assert!(fire_ids(&plain, &rules).is_empty());
    assert_eq!(plain.scenario_rng.logical_state(), rng);

    // Native would read past an empty OnFire vector. Recovery still spends
    // the preceding selection/loop/direction draws, without a fallback fire.
    let (mut empty, rules, building) = scene(true, "none").unwrap();
    let rng = empty.scenario_rng.logical_state();
    cross_yellow(&mut empty, &rules, building);
    assert!(empty.anims().all(|(_, anim)| {
        anim.owner_entity() != Some(building) || anim.building_slot().is_some()
    }));
    assert_ne!(empty.scenario_rng.logical_state(), rng);
}

#[test]
fn sparky_attached_fire_survives_save_and_healing_then_uses_owner_expiry() {
    let Some((mut world, rules, building)) = scene(true, "FIRE01,FIRE02,FIRE03") else {
        return;
    };
    cross_yellow(&mut world, &rules, building);
    let fires = fire_ids(&world, &rules);
    assert!(
        !fires.is_empty(),
        "the receiver must create an attached fire"
    );
    let bytes = GameSnapshot::save(&world, 0, rules.simulation_config_hash(), "Sparky", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    // ScenarioLoad6894C5 calls683560, whose first call65C6D0 seeds0.
    assert_eq!(
        restored.scenario_rng.logical_state(),
        crate::sim::rng::SimRng::new(0).logical_state(),
        "the existing native load owner reseeds Scenario, without replaying the hit"
    );
    for id in &fires {
        assert_eq!(
            restored.anim(*id).unwrap().runtime(),
            world.anim(*id).unwrap().runtime()
        );
        assert_eq!(
            restored.anim_absolute_coord(*id),
            world.anim_absolute_coord(*id)
        );
    }
    hit(&mut restored, &rules, building, -1000);
    for id in &fires {
        assert_eq!(restored.anim(*id).unwrap().owner_entity(), Some(building));
        assert_eq!(restored.anim(*id).unwrap().damage_fire_slot(), None);
    }
    restored.uninit_with_rules(building, &rules);
    let rng = restored.scenario_rng.logical_state();
    for id in fires {
        let fire = restored
            .anim(id)
            .expect("owner expiry defers Anim retirement");
        assert_eq!(fire.owner_entity(), None);
        assert!(fire.runtime().inactive);
        restored
            .visit_anim(id, &rules, None, FrameEffects::default())
            .unwrap();
        restored.process_pending_delete();
        assert!(restored.anim(id).is_none());
        assert_eq!(restored.display_layers().layer_of(id), None);
    }
    assert_eq!(restored.scenario_rng.logical_state(), rng);
}
