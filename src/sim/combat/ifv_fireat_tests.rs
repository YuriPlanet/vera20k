//! Bounded production FireAt dispatch for an empty FV's two-shot burst.
//! Goldens: original GetFLH6F3AD0 + FireAt6FE4F2..6FF43F in
//! tools/projectile_oracle/ifv_fire_coord.{py,json,md}. Source pose, Scenario
//! prefix/RNG and dispatch frames are supplied on both sides. This does not
//! execute the native Unit scheduler or intervening Bullet/world AI.

use super::{CombatEmit, TargetKind, build_attacker_snapshot, world_receiver};
use crate::rules::{art_data::ArtRegistry, ruleset::RuleSet};
use crate::sim::movement::FacingClass;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

fn coord(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

#[test]
fn empty_fv_two_shot_fireat_matches_native_muzzles_ids_rearm_and_rng() {
    let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_fire_coord.json",
    ))
    .unwrap();
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let initial = &rows[0]["initial"];
    let fv_type = rules.object("FV").unwrap();
    assert_eq!(fv_type.weapon_list[0].as_deref(), Some("HoverMissile"));
    let missile = rules.weapon("HoverMissile").unwrap();
    assert_eq!(missile.burst, 2);
    assert_eq!(missile.projectile.as_deref(), Some("AAHeatSeeker2"));
    assert_eq!(missile.warhead.as_deref(), Some("HE"));
    assert_eq!(
        missile.speed,
        initial["layers"][0]["speed"].as_i64().unwrap() as i32
    );
    for (index, delay) in fv_type.burst_delays.iter().enumerate() {
        assert_eq!(
            *delay,
            initial["type_layers"][0]["burst_delays"][index]
                .as_i64()
                .unwrap() as i32
        );
    }
    assert!(missile.anim.is_empty());
    assert!(!missile.use_fire_particles && !missile.use_spark_particles && !missile.is_railgun);

    let mut world = Simulation::new();
    let owner = world.interner.intern("Americans");
    world.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
    );
    world.session.house_order.push(owner);
    crate::sim::arena_fixture::flat_arena(&mut world, &rules);
    // guided_step.create supplies flat level6 cells and no bridge flags.
    let terrain = world.resolved_terrain.as_mut().unwrap();
    for y in 16..25 {
        for x in 6..25 {
            terrain.cell_mut(x, y).unwrap().level = 6;
        }
    }
    let firer_id = world
        .spawn_object("FV", "Americans", 10, 20, 0, &rules)
        .unwrap();
    world.resolve_type_handles(&rules);
    let target_cell = &rows[0]["supplied"]["target_cell"];
    let target = TargetKind::Cell(
        target_cell[0].as_u64().unwrap() as u16,
        target_cell[1].as_u64().unwrap() as u16,
    );
    assert!(super::install_cell_attack_target_for_test(
        &mut world.substrate.entities,
        firer_id,
        target_cell[0].as_u64().unwrap() as u16,
        target_cell[1].as_u64().unwrap() as u16,
        Some(&rules),
        &world.interner,
    ));
    {
        let input = &rows[0]["supplied"];
        let origin = coord(&input["source_origin"]);
        let firer = world.substrate.entities.get_mut(firer_id).unwrap();
        firer.position.rx = (origin.x / 256) as u16;
        firer.position.ry = (origin.y / 256) as u16;
        firer.position.sub_x = SimFixed::from_num(origin.x % 256);
        firer.position.sub_y = SimFixed::from_num(origin.y % 256);
        firer.position.exact_z_leptons = Some(origin.z);
        firer.body_facing = FacingClass::new(input["body_heading"].as_u64().unwrap() as u16, 0);
        firer.barrel_facing = Some(FacingClass::new(
            input["source_heading"].as_u64().unwrap() as u16,
            0,
        ));
        firer.rearm_timer = crate::sim::timer::CdTimer::started(0, 0);
    }
    // Adopt the oracle's declared boundary after source setup, not a claim
    // that the fixture's synthetic world constructed the native full prefix.
    world.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
        rows[0]["launch"]["scenario_id_before"].as_u64().unwrap() as u32,
    ));
    world.scenario_rng = SimRng::new(31);
    let main_before = world.main_rng.logical_state();
    let anim_count = world.substrate.anims.len();
    let mut previous_projectiles = std::collections::BTreeSet::new();

    for row in rows {
        let frame = row["supplied"]["binary_frame"].as_u64().unwrap() as u32;
        world.session.binary_frame = frame;
        let firer = world.substrate.entities.get(firer_id).unwrap();
        assert_eq!(
            firer.weapon_burst.index(),
            row["launch"]["burst_before"].as_i64().unwrap() as i32
        );
        assert!(firer.rearm_timer.expired(frame as i32));
        let snapshot = build_attacker_snapshot(firer, target, None);
        assert_eq!(
            world.scenario_rng.native_state_hex(),
            row["rng_before"].as_str().unwrap(),
            "RNG before dispatch at {frame}"
        );
        assert_eq!(
            world.native_unique_ids.as_ref().unwrap().current_raw(),
            row["launch"]["scenario_id_before"].as_u64().unwrap() as u32
        );
        let mut emitted = CombatEmit::default();
        // Native starts at6FE4F2, after admission. Its supplied six-cell
        // target is outside retail HoverMissile's range; do not bypass that
        // real gate by changing rules or claim this fixture proves admission.
        let selected = super::combat_weapon::resolve_selected_weapon(
            &rules,
            fv_type,
            &super::combat_weapon::attacker_facts(
                world.substrate.entities.get(firer_id).unwrap(),
                fv_type,
            ),
            Some(&super::combat_weapon::cell_target_facts(
                target_cell[0].as_u64().unwrap() as u16,
                target_cell[1].as_u64().unwrap() as u16,
                world.resolved_terrain.as_ref(),
            )),
        )
        .unwrap();
        let shot = world_receiver::AdmittedFire {
            target_type_ref: snapshot.type_id,
            snap: snapshot,
            obj: fv_type,
            selected,
            target_coords: (
                target_cell[0].as_u64().unwrap() as u16,
                target_cell[1].as_u64().unwrap() as u16,
                SimFixed::from_num(128),
                SimFixed::from_num(128),
            ),
            is_garrison: false,
        };
        world_receiver::emit_admitted_fire(&mut world, &rules, shot, frame, &mut emitted, None);
        let new_bullets: Vec<_> = world
            .projectiles
            .iter()
            .filter(|(id, _)| !previous_projectiles.contains(*id))
            .collect();
        assert_eq!(new_bullets.len(), 1, "one constructor at {frame}");
        let (&stable_id, bullet) = new_bullets[0];
        assert_eq!(
            bullet.native_unique_id,
            row["launch"]["unique_id"].as_i64().unwrap() as i32
        );
        assert_ne!(stable_id, bullet.native_unique_id as u64);
        assert_eq!(bullet.launch_origin, coord(&row["launch"]["position"]));
        assert_eq!(
            bullet.position, bullet.launch_origin,
            "no intervening Bullet AI"
        );
        assert_eq!(bullet.launch_target, coord(&row["launch"]["target"]));
        assert_eq!(emitted.fire_events.len(), 1);
        assert_eq!(emitted.fire_events[0].fire_coord, bullet.launch_origin);
        assert_eq!(
            world.substrate.anims.len(),
            anim_count,
            "no muzzle Anim constructor"
        );
        assert_eq!(
            world.native_unique_ids.as_ref().unwrap().current_raw(),
            row["launch"]["scenario_id_after"].as_u64().unwrap() as u32,
            "the Bullet receipt is the only native ID in this shot"
        );
        let firer = world.substrate.entities.get(firer_id).unwrap();
        assert_eq!(
            [
                firer.rearm_timer.start_frame(),
                firer.rearm_timer.duration()
            ],
            [
                row["launch"]["rearm"][0].as_i64().unwrap() as i32,
                row["launch"]["rearm"][1].as_i64().unwrap() as i32,
            ]
        );
        assert_eq!(
            firer.weapon_burst.index(),
            row["launch"]["next_burst"].as_i64().unwrap() as i32
        );
        assert_eq!(
            world.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "full RNG state includes native rejection draws at {frame}"
        );
        assert_eq!(world.main_rng.logical_state(), main_before);
        previous_projectiles.insert(stable_id);
    }
}
