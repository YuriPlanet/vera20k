//! Execute33/34 -> retained Super -> House refresh -> ordinary ClickFire.
//! Native Steam3e81a617:6DE925/6DE98E,6CB560,6CC080,50AF10,6CBB18.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::superweapon;
use serde_json::json;

fn scene(kind: i32) -> (Simulation, RuleSet, MapFile, InternedId, InternedId) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[Sides]\nAllied=Americans\nSoviet=Russians\n[Americans]\nSide=Allied\n[Russians]\nSide=Soviet\n[SuperWeaponTypes]\n0=UnusedNuke\n1=GrantedReveal\n\
         [UnusedNuke]\nType=MultiMissile\nRechargeTime=1\n\
         [GrantedReveal]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=yes\nShowTimer=yes\nDisableableFromShell=yes\n",
    )).unwrap();
    scene_with_rules(kind, rules, 1)
}

fn scene_with_rules(
    kind: i32,
    rules: RuleSet,
    index: usize,
) -> (Simulation, RuleSet, MapFile, InternedId, InternedId) {
    let map=MapFile::from_bytes(format!(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Triggers]\nGRANT=Americans,<none>,Grant,0,1,1,1,0\n[Events]\nGRANT=1,8,0,0\n\
         [Actions]\nGRANT=1,{kind},11,{index},0,0,0,0,A\n[Tags]\nGRANTTAG=0,Grant,GRANT\n"
    ).as_bytes()).unwrap();
    let mut sim = Simulation::with_seed(31);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.install_playfield_from_map_header(&map.header);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.binary_frame = 1000;
    let owner = sim.interner.intern("Americans");
    let other = sim.interner.intern("Russians");
    for id in [owner, other] {
        sim.houses
            .insert(id, HouseState::new(id, 0, Some(id), id == owner, 0, 10));
        sim.session.house_order.push(id);
        superweapon::construct_house_supers(&mut sim, &rules, id, false);
    }
    sim.session.current_house = Some(owner);
    sim.session.game_options.super_weapons = true;
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(Some(&rules));
    let id = sim.interner.get(&rules.super_weapon_order[index]).unwrap();
    (sim, rules, map, owner, id)
}

fn spring(sim: &mut Simulation, rules: &RuleSet) {
    sim.advance_triggers(Some(rules), None, FrameEffects::default());
}

#[test]
fn super_grant_actions_reach_retained_instances_and_immediate_one_time_readiness() {
    for kind in [33, 34] {
        let (mut sim, rules, _, owner, id) = scene(kind);
        let rng = sim.rng_state();
        spring(&mut sim, &rules);
        let instance = &sim.super_weapons[&owner][&id];
        assert!(
            instance.is_active,
            "Execute{kind} must grant the retained slot1"
        );
        assert_eq!(instance.is_ready, kind == 33);
        assert_eq!(instance.charge_start_tick, 1000);
        assert_eq!(instance.charge_duration, if kind == 33 { 0 } else { 900 });
        assert_eq!(instance.ready_tick, if kind == 33 { 1000 } else { -100 });
        assert!(!instance.is_suspended);
        let unused = sim.interner.get("UnusedNuke").unwrap();
        let other = sim.interner.get("Russians").unwrap();
        assert!(!sim.super_weapons[&owner][&unused].is_active);
        assert!(!sim.super_weapons[&other][&id].is_active);
        assert_eq!(sim.rng_state(), rng, "grant does not draw from any stream");
        assert_eq!(superweapon::super_timer_views(&sim, &rules).len(), 1);
        assert_eq!(
            superweapon::superweapon_views_for_owner(&sim, &rules, &owner).len(),
            1
        );
    }
}

#[test]
fn super_grant_actions_survive_provider_loss_power_and_shell_until_house_defeat() {
    for kind in [33, 34] {
        let (mut sim, rules, _, owner, id) = scene(kind);
        spring(&mut sim, &rules);
        sim.session.game_options.super_weapons = false;
        sim.power_states.entry(owner).or_default().is_low_power = true;
        let before = serde_json::to_value(&sim.super_weapons[&owner][&id]).unwrap();
        superweapon::update_owned_supers(&mut sim, &rules, owner);
        assert!(sim.super_weapons[&owner][&id].is_active);
        assert_eq!(
            serde_json::to_value(&sim.super_weapons[&owner][&id]).unwrap(),
            before
        );
        sim.houses.get_mut(&owner).unwrap().is_defeated = true;
        superweapon::update_owned_supers(&mut sim, &rules, owner);
        assert!(!sim.super_weapons[&owner][&id].is_active);
        assert!(!sim.super_weapons[&owner][&id].is_ready);
        assert!(superweapon::super_timer_views(&sim, &rules).is_empty());
    }
}

#[test]
fn super_grant_one_time_click_consumes_and_repeating_click_recharges() {
    for kind in [33, 34] {
        let (mut sim, rules, _, owner, id) = scene(kind);
        spring(&mut sim, &rules);
        if kind == 34 {
            sim.session.binary_frame = 1900;
            superweapon::tick_house_superweapon_instances(&mut sim, &rules, owner);
        }
        assert!(sim.super_weapons[&owner][&id].is_ready);
        assert!(sim.fire_super_weapon(
            &rules,
            owner,
            id,
            (10, 10),
            None,
            FrameEffects::default(),
            false
        ));
        let instance = &sim.super_weapons[&owner][&id];
        assert_eq!(instance.is_active, kind == 34);
        assert!(!instance.is_ready);
        assert_eq!(
            instance.charge_start_tick,
            if kind == 33 { 1000 } else { 1900 }
        );
        assert_eq!(instance.charge_duration, if kind == 33 { 0 } else { 900 });
        assert_eq!(
            superweapon::superweapon_views_for_owner(&sim, &rules, &owner).len(),
            usize::from(kind == 34)
        );
    }
}

fn execute(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    kind: i32,
    index: usize,
) -> bool {
    let entries = crate::map::actions::parse_actions(&IniFile::from_str(&format!(
        "[Actions]\nGRANT=1,{kind},11,{index},0,0,0,0,A\n"
    )));
    let action = RuntimeDefinitions::default().resolve_action(
        entries["GRANT"].entries()[0].clone(),
        None,
        None,
    );
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: Some(owner),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        None,
        FrameEffects::default(),
    )
}

#[test]
fn super_grant_retained_histories_preserve_original_flags_and_old_hold() {
    // Original full Read/Execute/Grant controls, including an already active
    // instance and a retained running timer with its hold set by prior play.
    for (
        kind,
        active,
        once,
        can_lose,
        held,
        expected_once,
        expected_lose,
        ready,
        start,
        duration,
    ) in [
        (33, true, false, true, false, false, true, false, 123, 7000),
        (34, true, false, true, false, false, false, false, 123, 7000),
        (33, true, true, true, false, true, true, false, 123, 7000),
        (34, true, true, true, false, true, false, false, 123, 7000),
        (33, false, false, false, true, true, false, true, 1000, 0),
        (
            34, false, false, false, true, false, false, false, 123, 7000,
        ),
    ] {
        let (mut sim, rules, _, owner, id) = scene(kind);
        let slot = sim
            .super_weapons
            .get_mut(&owner)
            .unwrap()
            .get_mut(&id)
            .unwrap();
        let mut value = serde_json::to_value(&*slot).unwrap();
        value["is_active"] = json!(active);
        value["one_time"] = json!(once);
        value["can_lose"] = json!(can_lose);
        value["is_suspended"] = json!(held);
        value["charge_start_tick"] = json!(123);
        value["charge_duration"] = json!(7000);
        *slot = serde_json::from_value(value).unwrap();
        assert!(execute(&mut sim, &rules, owner, kind, 1));
        let instance = &sim.super_weapons[&owner][&id];
        let value = serde_json::to_value(instance).unwrap();
        assert_eq!(value["one_time"], json!(expected_once));
        assert_eq!(value["can_lose"], json!(expected_lose));
        assert_eq!(
            (instance.is_active, instance.is_ready, instance.is_suspended),
            (true, ready, held)
        );
        assert_eq!(
            (instance.charge_start_tick, instance.charge_duration),
            (start, duration)
        );
    }
}

#[test]
fn super_grant_repeat_after_once_saves_both_flags_and_still_fires_once() {
    let (mut sim, rules, map, owner, id) = scene(33);
    spring(&mut sim, &rules);
    let first = sim.state_hash();
    assert!(execute(&mut sim, &rules, owner, 34, 1));
    let mixed = sim.state_hash();
    assert_ne!(
        first, mixed,
        "the retained +60 gate affects future admission"
    );
    assert!(execute(&mut sim, &rules, owner, 33, 1));
    assert_eq!(
        sim.state_hash(),
        mixed,
        "a refused repeat grant changes no flags"
    );
    let expected = serde_json::to_value(&sim.super_weapons[&owner][&id]).unwrap();
    assert_eq!(expected["one_time"], json!(true));
    assert_eq!(expected["can_lose"], json!(false));
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "GRANT.MAP", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        serde_json::to_value(&restored.super_weapons[&owner][&id]).unwrap(),
        expected
    );
    // Reinstall the declared map authority, as the production loader does;
    // snapshot decoding alone does not restore source terrain/navigation.
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    restored.install_playfield_from_map_header(&map.header);
    assert!(restored.fire_super_weapon(
        &rules,
        owner,
        id,
        (10, 10),
        None,
        FrameEffects::default(),
        false
    ));
    assert!(!restored.super_weapons[&owner][&id].is_active);
    assert_eq!(
        serde_json::to_value(&restored.super_weapons[&owner][&id]).unwrap()["one_time"],
        json!(false)
    );
    assert!(!restored.fire_super_weapon(
        &rules,
        owner,
        id,
        (10, 10),
        None,
        FrameEffects::default(),
        false,
    ));
}

#[test]
fn super_grant_one_time_nuke_uses_payload_speed_and_ground_warning_without_silo() {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, test_flat_cell, test_grid};
    use crate::sim::projectile::ProjectileTrajectory;
    // Three original Launch/43B5E0 controls: speed77, bright0, real-target
    // warning at floor even when the missile starts above a structural bridge.
    for (flags, level, origin_z, warning_z) in [
        (0, 0, 20000, 0),
        (0x100, 2, 20624, 208),
        (0x400, 2, 20208, 208),
    ] {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut rules = retail.rules;
        let all = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
        assert!(
            all.contains(&"PSIWARN".to_string()),
            "the production binder must include the literal root"
        );
        assert_eq!(
            rules.bind_anim_class_assets(
                &["PSIWARN".into(), "NUKEBALL".into()],
                &assets,
                "TEM",
                "TEMPERATE"
            ),
            0
        );
        // Full cold AudioVisual -> Rules -> mode -> Hills processing uses
        // Gravity6 at the speed postpass. This selected original history gives
        //235, unlike a fixture that starts its first Process at ctorGravity3.
        assert_eq!(rules.weapon("NukePayload").unwrap().speed, 235);
        rules.set_weapon_speed_for_test("NukePayload", 77);
        let index = rules.super_weapon_index("NukeSpecial").unwrap();
        let (mut sim, rules, _, owner, id) = scene_with_rules(33, rules, index);
        sim.install_resolved_terrain_for_new_map(test_grid(32, 32, |x, y| {
            let mut cell = ResolvedTerrainCell {
                level: if (x, y) == (10, 10) { level } else { 0 },
                ..test_flat_cell(x, y)
            };
            if (x, y) == (10, 10) {
                cell.bridge_facts.raw_flags = flags;
            }
            cell
        }));
        spring(&mut sim, &rules);
        assert!(sim.fire_super_weapon(
            &rules,
            owner,
            id,
            (10, 10),
            None,
            FrameEffects::default(),
            false
        ));
        assert!(!sim.super_weapons[&owner][&id].is_active);
        let (&bullet_id, bullet) = sim
            .projectiles
            .iter()
            .next()
            .expect("one-time launch has no silo dependency");
        assert_eq!(
            (
                bullet.launch_origin.x,
                bullet.launch_origin.y,
                bullet.launch_origin.z
            ),
            (2688, 2688, origin_z)
        );
        assert_eq!(
            bullet.velocity.native().map(|n| n.bits()),
            [0xb9be6aaf04000000, 0xbd0b936000000000, 0xc059000000000000]
        );
        assert_eq!(bullet.source_id, crate::sim::combat::RAD_NO_ATTACKER);
        assert_eq!(bullet.firer_house(), None);
        assert!(!bullet.payload.bright());
        assert!(matches!(
            bullet.trajectory,
            ProjectileTrajectory::Vertical { max_speed: 77, .. }
        ));
        assert_eq!(sim.houses[&owner].nuke_target(), (0, 0));
        let (&warning_id, warning) = sim
            .substrate
            .anims
            .iter()
            .find(|(_, a)| sim.interner.resolve(a.type_id) == "PSIWARN")
            .expect("bound warning constructs");
        assert_eq!(
            (
                warning.world_coord.x,
                warning.world_coord.y,
                warning.world_coord.z
            ),
            (2688, 2688, warning_z)
        );
        assert_eq!(warning.attached_bullet(), Some(bullet_id));
        assert_eq!(warning.owner_house(), Some(owner));
        assert!(warning.draw_runtime.hidden);
        assert_eq!(warning.draw_flags, 0x600);
        assert!(sim.retire_non_entity_object(
            bullet_id,
            crate::sim::world::UninitContext::with_rules(&rules)
        ));
        assert!(sim.substrate.pending_delete.contains_object(&warning_id));
        sim.process_pending_delete();
        assert!(sim.substrate.anims.get(warning_id).is_none());
    }
}

#[test]
fn super_grant_retail_one_time_nuke_reaches_wait_damage_radiation_and_cleanup() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = retail.rules;
    assert_eq!(
        rules.bind_anim_class_assets(
            &["PSIWARN".into(), "NUKEBALL".into()],
            &assets,
            "TEM",
            "TEMPERATE"
        ),
        0
    );
    let index = rules.super_weapon_index("NukeSpecial").unwrap();
    let (rules, mut sim, owner) = superweapon::chronosphere_tests::world_with(rules, 64, &[]);
    let target = (40, 40);
    let victim = sim
        .spawn_object_at_height("MTNK", "Americans", target.0, target.1, 0, 0, &rules)
        .unwrap();
    let health = sim.entities().get(victim).unwrap().health.current();
    let id = sim.interner.get("NukeSpecial").unwrap();
    assert!(execute(&mut sim, &rules, owner, 33, index));
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        id,
        target,
        None,
        FrameEffects::default(),
        false
    ));
    assert!(!sim.super_weapons[&owner][&id].is_active);
    let bullet_id = *sim.projectiles.iter().next().unwrap().0;
    let warning_id = sim
        .substrate
        .anims
        .iter()
        .find(|(_, a)| a.attached_bullet() == Some(bullet_id))
        .map(|(id, _)| *id)
        .unwrap();
    let mut waited = false;
    for _ in 0..1200 {
        superweapon::chronosphere_tests::step(&mut sim, &rules);
        match sim.projectiles.get(bullet_id) {
            Some(bullet) => waited |= bullet.awaiting_anim(),
            None => break,
        }
    }
    assert!(waited, "the physical NUKEBALL owns impact delay");
    assert!(sim.projectiles.get(bullet_id).is_none());
    assert!(sim.substrate.anims.get(warning_id).is_none());
    assert!(
        sim.entities()
            .get(victim)
            .is_none_or(|e| e.health.current() < health)
    );
    assert!(sim.radiation.site_at(target).is_some());
    assert!(
        !sim.super_weapons[&owner][&id].is_active,
        "ordinary House passes must not regrant a consumed one-time instance without a provider"
    );
}
