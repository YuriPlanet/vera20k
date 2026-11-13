//! FireAt's railgun arm (`0x006FF1FB..0x006FF26E`) through production.

use crate::rules::ini_parser::IniFile;
use crate::rules::particle_system_type::ParticleSystemBehavesLike;
use crate::rules::retail_ini_fixture::RetailBattleRules;
use crate::sim::combat::{DeathEffects, TargetKind};
use crate::sim::command::Command;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::vision::FogState;
use crate::sim::world::FrameEffects;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;
use std::collections::BTreeMap;

/// Retail sets `LtRail` on no type and lists no weapons, so a map that arms
/// a type with it allocates the weapon in its own pass and must carry the
/// section too. This layer copies retail `[LtRail]` and arms the Grizzly.
fn railgun_rules() -> Option<RetailBattleRules> {
    crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&IniFile::from_str(
        "[MTNK]\nPrimary=LtRail\n\
         [LtRail]\nDamage=0\nAmbientDamage=150\nROF=60\nRange=6\n\
         Projectile=InvisibleLow\nSpeed=100\nWarhead=RailShot2\nAnim=GUNFIRE\n\
         IsRailgun=true\nAttachedParticleSystem=SmallRailgunSys\nReport=BIGGGUN1\n",
    ))
}

/// One shot strikes the target and an (unarmed) MCV standing on the line,
/// spares one two cells off it, and links the Railgun system as `TechnoClass+0x314`,
/// which refuses the next railgun shot until its particles expire.
#[test]
fn a_railgun_shot_strikes_the_line_and_holds_its_particle_system() {
    let Some(patched) = railgun_rules() else {
        return;
    };
    let rules = &patched.rules;
    let weapon = rules.weapon("LtRail").unwrap();
    assert!(weapon.is_railgun);
    assert_eq!(
        (
            weapon.ambient_damage,
            rules.combat_damage.railgun_damage_radius
        ),
        (150, 0x80)
    );

    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x7A11_6A00);
    sim.fog = FogState::new(32, 32);
    let owner = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let source = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, rules)
        .unwrap();
    let target = sim
        .spawn_object("SMCV", "Russians", 20, 16, 192, rules)
        .unwrap();
    let on_line = sim
        .spawn_object("SMCV", "Russians", 18, 16, 192, rules)
        .unwrap();
    let off_line = sim
        .spawn_object("SMCV", "Russians", 18, 18, 192, rules)
        .unwrap();
    sim.fog.mark_visible_for_owner(owner, 20, 16);
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: source,
            target_id: target
        },
        Some(rules),
    ));
    let health = |sim: &crate::sim::world::Simulation, id| {
        sim.substrate.entities.get(id).unwrap().health.current()
    };
    let grid = sim.path_grid_snapshot().unwrap();
    let fired = (0..64).find(|_| {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        sim.fire_events
            .iter()
            .any(|shot| shot.attacker_id == source)
    });
    assert!(
        fired.is_some(),
        "the railgun Grizzly did not fire within 64 ticks"
    );

    // RailShot2 is 100% against every armour: 1000 - 150.
    assert_eq!(health(&sim, target), 850);
    assert_eq!(health(&sim, on_line), 850);
    assert_eq!(health(&sim, off_line), 1000);
    let system = sim
        .substrate
        .entities
        .get(source)
        .unwrap()
        .railgun_particle_system
        .system()
        .expect("FireAt links the Railgun system");
    let held = sim.particle_systems().get(system).unwrap();
    assert_eq!(
        rules.particle_system_type(held.type_id()).behaves_like,
        ParticleSystemBehavesLike::Railgun
    );
    let aim = super::world_fire::object_get_coords(&sim, target).unwrap();
    assert_eq!(held.target_coords().to_array(), [aim.x, aim.y, aim.z]);

    // The link holds while particles live (MaxEC 70 plus up to 9), so no
    // second shot lands; then it clears and the next one may.
    for _ in 0..70 {
        sim.fire_events.clear_for_test();
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        assert!(
            !sim.fire_events
                .iter()
                .any(|shot| shot.attacker_id == source),
            "a second shot while +0x314 is live"
        );
    }
    let cleared = (0..20).any(|_| {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        sim.substrate
            .entities
            .get(source)
            .unwrap()
            .railgun_particle_system
            .system()
            .is_none()
    });
    assert!(cleared, "the expired Railgun system clears +0x314");
}

/// Native comparison: `tools/procedural_drawing_oracle/railgun.json`
/// `line_cases` run the whole `0x0070C690` over objects just inside and
/// outside `RailgunDamageRadius`, a building far off the line, a step below
/// the ground and a target in no cell list. Each ReceiveDamage native records
/// carries AmbientDamage, distance 0, the weapon's Warhead, the firer and no
/// House, as `line_damage` passes them.
#[test]
fn railgun_line_damage_matches_native_hits_and_ground_stop() {
    let Some(patched) = railgun_rules() else {
        return;
    };
    let rules = &patched.rules;
    let weapon = rules.weapon("LtRail").unwrap();
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/railgun.json",
    ))
    .unwrap();
    let xyz = |value: &Value| -> [i32; 3] {
        std::array::from_fn(|axis| value[axis].as_i64().unwrap() as i32)
    };
    for case in corpus["line_cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        assert_eq!(
            case["radius"].as_i64().unwrap() as i32,
            rules.combat_damage.railgun_damage_radius
        );
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 1, 64);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let source = xyz(&case["source"]);
        // Native's firer is in no cell list; the source is passed explicitly.
        let firer = sim
            .spawn_object("MTNK", "Americans", 2, 2, 64, rules)
            .unwrap();
        let mut ids = BTreeMap::new();
        for object in case["objects"].as_array().unwrap() {
            let [x, y, z] = xyz(&object["location"]);
            let (rx, ry) = ((x / 256) as u16, (y / 256) as u16);
            let id = if object["kind"] == "building" {
                sim.spawn_object("GAPILL", "Russians", rx, ry, 0, rules)
                    .unwrap()
            } else if object.get("placed") == Some(&Value::Bool(false)) {
                // Natively in no cell list; production holds it in none.
                let id = sim.substrate.allocate_stable_id();
                let mut entity = crate::sim::game_entity::GameEntity::test_default(
                    id, "MTNK", "Russians", rx, ry,
                );
                entity.set_owner_for_test(sim.interner.intern("Russians"));
                entity.set_type_ref_for_test(sim.interner.intern("MTNK"));
                entity.health.set_for_test(300);
                sim.substrate.entities.insert(entity);
                id
            } else {
                sim.spawn_object("MTNK", "Russians", rx, ry, 0, rules)
                    .unwrap()
            };
            if object["kind"] != "building" {
                let position = &mut sim.substrate.entities.get_mut(id).unwrap().position;
                position.set_sub_x_for_test(SimFixed::from_num(x % 256));
                position.set_sub_y_for_test(SimFixed::from_num(y % 256));
                position.set_exact_z_leptons_for_test(Some(z));
            }
            ids.insert(object["name"].as_str().unwrap().to_owned(), id);
        }
        let health: BTreeMap<_, _> = ids
            .iter()
            .map(|(name, &id)| {
                (
                    name.clone(),
                    sim.substrate.entities.get(id).unwrap().health.current(),
                )
            })
            .collect();
        let mut deaths = DeathEffects::default();
        let rng_before = sim.scenario_rng.native_state_hex();
        let returned = super::railgun::line_damage(
            &mut sim,
            rules,
            None,
            FrameEffects::default(),
            &mut deaths,
            firer,
            ProjectileCoord::new(source[0], source[1], source[2]),
            TargetKind::Entity(ids[case["target"].as_str().unwrap()]),
            weapon,
        );
        assert_eq!(
            [returned.x, returned.y, returned.z],
            xyz(&case["returned"]),
            "{name}: returned coordinate"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex() == rng_before,
            case["rng_unchanged"].as_bool().unwrap(),
            "{name}: Scenario draws"
        );
        let mut struck: Vec<_> = ids
            .iter()
            .filter(|(name, id)| {
                sim.substrate.entities.get(**id).unwrap().health.current() == health[*name] - 150
            })
            .map(|(name, _)| name.as_str())
            .collect();
        let mut expected: Vec<_> = case["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| {
                assert_eq!(
                    hit["damage"].as_i64().unwrap() as i32,
                    weapon.ambient_damage
                );
                assert_eq!(
                    (&hit["distance"], &hit["weapon_warhead"], &hit["firer"]),
                    (&Value::from(0), &Value::Bool(true), &Value::from("firer"))
                );
                assert_eq!(
                    (&hit["ignore_defenses"], &hit["arg6"], &hit["house"]),
                    (&Value::from(0), &Value::from(0), &Value::from(0))
                );
                hit["object"].as_str().unwrap()
            })
            .collect();
        struck.sort_unstable();
        expected.sort_unstable();
        assert_eq!(struck, expected, "{name}: struck objects");
    }
}

/// Ares TechnoClass_Fire_DetachedRailgun/2 (Ares.dll 1004D0C0/1004D0F0,
/// hooks 0x6FF1FB/0x6FF26E): an IsDetachedRailgun weapon strikes the line
/// and builds its system without linking +0x314, so the next shot follows
/// its ROF instead of waiting for the particles.
#[test]
fn a_detached_railgun_never_holds_its_particle_system() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let Some(patched) =
        crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&IniFile::from_str(
            "[MTNK]\nPrimary=LtRail\n\
             [LtRail]\nDamage=0\nAmbientDamage=150\nROF=20\nRange=6\n\
             Projectile=InvisibleLow\nSpeed=100\nWarhead=RailShot2\n\
             IsRailgun=true\nIsDetachedRailgun=yes\nAttachedParticleSystem=SmallRailgunSys\n",
        ))
    else {
        return;
    };
    let rules = &patched.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x7A11_6A01);
    sim.fog = FogState::new(32, 32);
    let owner = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let source = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, rules)
        .unwrap();
    let target = sim
        .spawn_object("SMCV", "Russians", 20, 16, 192, rules)
        .unwrap();
    sim.fog.mark_visible_for_owner(owner, 20, 16);
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: source,
            target_id: target
        },
        Some(rules),
    ));
    let grid = sim.path_grid_snapshot().unwrap();
    let mut shots = 0;
    for _ in 0..64 {
        sim.fire_events.clear_for_test();
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        if sim
            .fire_events
            .iter()
            .any(|shot| shot.attacker_id == source)
        {
            shots += 1;
            let firer = sim.substrate.entities.get(source).unwrap();
            assert!(firer.railgun_particle_system.system().is_none());
            assert!(!sim.particle_systems().is_empty());
        }
    }
    assert!(
        shots >= 2,
        "only {shots} detached railgun shots in 64 ticks"
    );
}
