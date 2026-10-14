//! Original candidate geometry observations, not a second search implementation.
use super::*;
use serde_json::Value;

#[test]
fn original_infantry_and_unit_approach_radii_and_rays() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/cleg_approach_vectors.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    assert_eq!(native["infantry_identity"]["rtti"], 15);
    let cases = native["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 380);
    for row in cases {
        let range = row["weapon_range"].as_i64().unwrap() as i32;
        let adjusted = if range >= 512 { range - 128 } else { range };
        assert_eq!(i64::from(adjusted), row["adjusted"].as_i64().unwrap());
        let width = row["foundation"][0].as_i64().unwrap() as i32;
        let height = row["foundation"][1].as_i64().unwrap() as i32;
        let foundation = ((width, height) != (0, 0)).then_some((width, height));
        let category = if row["rtti"] == 15 {
            EntityCategory::Infantry
        } else {
            EntityCategory::Unit
        };
        assert_eq!(
            i64::from(approach_search_radius(adjusted, category, foundation)),
            row["radius"].as_i64().unwrap(),
            "{row}"
        );
    }
    let mut rays = 0;
    for pose in native["numeric"]["poses"].as_array().unwrap() {
        let target: [i32; 2] = serde_json::from_value(pose["target_xy"].clone()).unwrap();
        let source: [i32; 2] = serde_json::from_value(pose["source_xy"].clone()).unwrap();
        let facing = facing16_between(target, source);
        assert_eq!(u64::from(facing), pose["facing_word"].as_u64().unwrap());
        let base = ((u32::from(facing) + 128) >> 8) as u8;
        for row in pose["candidates"].as_array().unwrap() {
            let direction =
                (i16::from(base).wrapping_add(row["offset"].as_i64().unwrap() as i16) as u16) << 8;
            let radius = row["radius"].as_i64().unwrap() as i32;
            let expected: [i32; 3] = serde_json::from_value(row["unsnapped_xyz"].clone()).unwrap();
            assert_eq!(
                facing_step_world_xy(target, direction, radius),
                [expected[0], expected[1]]
            );
            rays += 1;
        }
    }
    assert_eq!(rays, 100);
}

/// Original non-CloseRange candidate block4D60FB..61A4 executes both map
/// queries. Ground at4D6181 is stored after PUSH: [esp+38] is the candidate's
/// Z, not a temporary. These controls cover flat, elevated, sloped and bridge
/// cells; they do not execute the whole approach or terrain admission.
#[test]
fn original_approach_candidate_heights() {
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/cleg_approach_vectors.json",
    ))
    .unwrap();
    let rows = native["candidate_heights"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    for row in rows {
        let point: [i32; 2] = serde_json::from_value(row["point_xy"].clone()).unwrap();
        let cell: [u16; 2] = serde_json::from_value(row["input"]["cell_xy"].clone()).unwrap();
        let width = cell[0] + 1;
        let height = cell[1] + 1;
        let mut terrain = ResolvedTerrainGrid::from_cells(
            width,
            height,
            (0..height)
                .flat_map(|ry| (0..width).map(move |rx| test_flat_cell(rx, ry)))
                .collect(),
        );
        let sampled =
            &mut terrain.cells[usize::from(cell[1]) * usize::from(width) + usize::from(cell[0])];
        sampled.level = row["level"].as_u64().unwrap().try_into().unwrap();
        sampled.slope_type = row["slope"].as_u64().unwrap().try_into().unwrap();
        sampled.bridge_facts.raw_flags = row["flags"].as_u64().unwrap().try_into().unwrap();
        let cells = NativeCellQuery::canonical(&terrain);
        assert_eq!(
            i64::from(
                crate::sim::movement::ground_pose::query_ground_height(
                    &cells,
                    DriveCoord {
                        x: point[0],
                        y: point[1],
                        z: 0
                    },
                )
                .unwrap()
            ),
            row["ground_z"].as_i64().unwrap(),
            "{row}"
        );
        assert_eq!(
            in_range::native_ground_target_z(point[0], point[1], &cells),
            row["candidate_z"].as_i64(),
            "{row}"
        );
    }
}

/// Stock CLEG weapon/type/ART readers, with supplied flat cells and a stationary
/// enemy. This checks the ordinary Attack command through live object turns;
/// it does not compare exact native first-shot or erase timing.
#[test]
fn retail_legionnaire_attacks_distant_object_after_teleport_recovery() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    for (target_type, target_x, stop_after_freeze) in [
        ("HTNK", 25, false),
        ("GAWEAP", 25, false),
        ("E1", 25, false),
        ("HTNK", 13, false),
        ("HTNK", 25, true),
    ] {
        let mut sim = Simulation::with_seed(11);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        for (name, side) in [("Americans", 0), ("Russians", 1)] {
            let owner = sim.intern(name);
            sim.houses
                .insert(owner, HouseState::new(owner, side, None, true, 0, 10));
            sim.session.house_order.push(owner);
        }
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let target = sim
            .spawn_object_at_height(target_type, "Russians", target_x, 10, 0, 0, &rules)
            .unwrap();
        // A supplied stationary target separates the approach under test from enemy
        // movement/fire. The factory has no attack weapon in stock rules.
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .mission
            .set_current_for_test(
                crate::sim::mission::MissionId::from_known(MissionType::Sleep),
                0,
            );
        let cleg = sim
            .spawn_object_at_height("CLEG", "Americans", 10, 10, 0, 0, &rules)
            .unwrap();
        let owner = sim.intern("Americans");
        let command = CommandEnvelope::new(
            owner,
            1,
            Command::Attack {
                attacker_id: cleg,
                target_id: target,
            },
        );
        let mut warped = false;
        let mut frozen = false;
        let mut completed = false;
        let mut stop = None;
        let mut stop_issued = false;
        let original_health = sim.substrate.entities.get(target).unwrap().health.current();
        for frame in 0..2200 {
            let pending = stop.take();
            let commands = if frame == 0 {
                std::slice::from_ref(&command)
            } else {
                pending.as_slice()
            };
            sim.advance_tick(commands, Some(&rules), Some(&grid), None, 33);
            let actor = sim.substrate.entities.get(cleg).unwrap();
            warped |= actor.is_warping_in();
            if actor.temporal.warp_target() == Some(target) {
                assert!(!actor.is_warping_in(), "cannot fire before recovery");
                assert_ne!(
                    (actor.position.rx, actor.position.ry),
                    (target_x, 10),
                    "approach must choose a firing position, not the occupied target cell"
                );
                frozen = true;
                if stop_after_freeze && !stop_issued {
                    stop = Some(CommandEnvelope::new(
                        owner,
                        sim.session.tick + 1,
                        Command::Stop { entity_id: cleg },
                    ));
                    stop_issued = true;
                }
            }
            if stop_issued && !actor.temporal.is_warping_someone() {
                let victim = sim.substrate.entities.get(target).unwrap();
                assert!(victim.lifecycle.object_alive());
                assert!(!victim.ai_frozen());
                assert_eq!(victim.health.current(), original_health);
                assert!(actor.attack_target.target().is_none());
                completed = true;
                break;
            }
            if sim
                .substrate
                .entities
                .get(target)
                .is_none_or(|t| !t.lifecycle.object_alive())
            {
                assert!(
                    !actor.temporal.is_warping_someone(),
                    "erase detaches the beam"
                );
                completed = true;
                break;
            }
        }
        assert_eq!(
            warped,
            target_x == 25,
            "{target_type}: only distant Attack needs teleport"
        );
        assert!(
            frozen,
            "{target_type}: Attack retains its target and freezes it after recovery"
        );
        assert!(
            completed,
            "{target_type}: erasure or Stop must clean up the beam"
        );
    }
}

#[test]
fn original_approach_candidate_geometry_and_order() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/approach_vectors.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let original_offsets = native["original_constants"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|row| row.get("signed_offsets"))
        .unwrap();
    assert_eq!(serde_json::json!(OFFSETS), *original_offsets);
    for (i, offset) in native["numeric"]["neighbor_offsets"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(serde_json::json!(cell_delta_unchecked(i as u8)), *offset);
    }
    let mut count = 0;
    for pose in native["numeric"]["poses"].as_array().unwrap() {
        let source: [i32; 2] = serde_json::from_value(pose["source_xy"].clone()).unwrap();
        let target: [i32; 2] = serde_json::from_value(pose["target_xy"].clone()).unwrap();
        let facing = facing16_between(target, source);
        assert_eq!(u64::from(facing), pose["facing_word"].as_u64().unwrap());
        let base = ((u32::from(facing) + 128) >> 8) as u8;
        assert_eq!(u64::from(base), pose["base_direction"].as_u64().unwrap());
        for row in pose["candidates"].as_array().unwrap() {
            let offset = row["offset"].as_i64().unwrap() as i16;
            let direction = (i16::from(base).wrapping_add(offset) as u16) << 8;
            let radius = row["radius"].as_i64().unwrap() as i32;
            let actual = facing_step_world_xy(target, direction, radius);
            let expected: [i32; 3] = serde_json::from_value(row["unsnapped_xyz"].clone()).unwrap();
            assert_eq!(
                actual,
                [expected[0], expected[1]],
                "source={source:?} target={target:?} offset={offset} radius={radius}"
            );
            count += 1;
        }
    }
    assert_eq!(count, 465);
}

/// Retail rulesmd.ini through the production reader: the Kirov (`ZEP`) and
/// the Floating Disc (`DISK`) are `BalloonHover=`; the Kirov's bombs, rookie
/// and elite, and the Disc's drain fall `Vertical=`, so the arm sends them
/// over their Target; the Disc's laser does not, so it goes on to the Foot
/// approach.
#[test]
fn retail_balloons_take_their_target_with_vertical_weapons() {
    use crate::sim::combat::combat_weapon::{primary_for_tier, secondary_for_tier};
    use crate::sim::combat::veterancy::RANK_ELITE_U16;
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    let zep = rules.object("ZEP").unwrap();
    let disk = rules.object("DISK").unwrap();
    assert!(zep.balloon_hover && disk.balloon_hover);
    for (weapon, vertical) in [
        (primary_for_tier(zep, 0), true),
        (primary_for_tier(zep, RANK_ELITE_U16), true),
        (primary_for_tier(disk, 0), false),
        (secondary_for_tier(disk, 0), true),
    ] {
        let weapon = rules.weapon(weapon.unwrap()).unwrap();
        assert_eq!(
            balloon_hover_destination(Some(weapon), TargetKind::Entity(7), &rules),
            vertical.then_some(NavTargetRef::object(7)),
            "{}",
            weapon.id
        );
    }
}

/// A tank with a 4-cell gun, and nothing else.
fn tank_rules() -> RuleSet {
    RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n\
         [TANK]\nStrength=300\nArmor=heavy\nSpeed=6\nSight=10\nPrimary=TankGun\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [TankGun]\nDamage=10\nROF=20\nRange=4\nProjectile=InvisibleAll\nWarhead=HITWH\n\
         [InvisibleAll]\nInviso=yes\nAA=yes\nAG=yes\n\
         [HITWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("tank rules")
}

/// Stamps the test catalog's DestroyableCliffs A tile (6x4, its four corners
/// without image, so no cell holds them) with its top-left cell at (8, 9).
fn stamp_destroyable_cliff(sim: &mut Simulation) {
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    terrain.test_install_destroyable_cliff_catalog(100);
    for row in 0..4u8 {
        for column in 0..6u8 {
            let sub_tile = row * 6 + column;
            if [0, 5, 18, 23].contains(&sub_tile) {
                continue;
            }
            let cell = terrain
                .cell_mut(8 + u16::from(column), 9 + u16::from(row))
                .unwrap();
            cell.final_tile_index = 100;
            cell.final_sub_tile = sub_tile;
        }
    }
}

/// `MapClass @ 0x00586FC0`: of the tile's imaged cells in the playfield, the
/// one nearest the mover's cell, the first row by row on a tie. From (4, 8)
/// the imageless corner (8, 9) would be nearest, so (8, 10) is taken; from
/// (14, 8), (12, 9) and (13, 10) tie and the upper row wins. A cell off any
/// destroyable tile answers itself.
#[test]
fn the_destroyable_cliff_redirect_takes_the_nearest_imaged_cell() {
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        32,
    ));
    sim.playfield_bounds = Some(crate::sim::arena_fixture::OPEN_PLAYFIELD);
    stamp_destroyable_cliff(&mut sim);

    for (target, own, nearest) in [
        ((10, 10), (4, 8), (8, 10)),
        ((12, 11), (14, 8), (12, 9)),
        ((20, 20), (4, 8), (20, 20)),
    ] {
        assert_eq!(
            sim.destroyable_cliff_nearest_cell(target, own),
            nearest,
            "{target:?} from {own:?}"
        );
    }
}

/// Approach's DestroyableCliffs arm (`0x004D5ABE..0x004D5B60`): a tank out of
/// range of a Cell on the tile takes the tile's cell nearest its own as its
/// target (vt+0x3C8) and searches for a firing cell against that.
#[test]
fn approach_retargets_a_destroyable_cliff_cell_to_its_nearest_imaged_cell() {
    let rules = tank_rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4D5ABE);
    stamp_destroyable_cliff(&mut sim);
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", 2, 8, 0, 0, &rules)
        .expect("tank spawns");
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .attack_target
        .set_for_test(Some(combat::TargetKind::Cell(10, 10)));

    sim.approach_target(tank, &rules, None, FrameEffects::default());

    let tank = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(tank.attack_target.target(), Some(TargetKind::Cell(8, 10)));
    assert!(
        matches!(tank.navigation.nav_com, Some(NavTargetRef::Cell { .. })),
        "{:?}",
        tank.navigation.nav_com
    );
}

/// Approach's NavQueue arm (`0x004D5A4D..0x004D5ABB`): out of range, the
/// first queued destination, an object as much as a Cell, is set (vt+0x480
/// with 0), leaves the queue and is the answer, ahead of any search.
#[test]
fn approach_takes_the_first_queued_destination_of_any_kind() {
    let rules = tank_rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4D5A4D);
    let spawn = |sim: &mut Simulation, owner: &str, rx: u16| {
        sim.spawn_object_at_height("TANK", owner, rx, 10, 0, 0, &rules)
            .expect("tank spawns")
    };
    let tank = spawn(&mut sim, "Americans", 4);
    let enemy = spawn(&mut sim, "Russians", 20);
    let escort = spawn(&mut sim, "Americans", 8);
    let queued = [NavTargetRef::object(escort), NavTargetRef::cell(5, 5)];
    let actor = sim.substrate.entities.get_mut(tank).unwrap();
    actor
        .attack_target
        .set_for_test(Some(combat::TargetKind::Entity(enemy)));
    actor.navigation.nav_queue = queued.to_vec();

    let answer = sim
        .approach_foot_target(tank, &rules, None, FrameEffects::default())
        .unwrap();

    let actor = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(answer, Some(queued[0]));
    assert_eq!(actor.navigation.nav_com, Some(queued[0]));
    assert_eq!(actor.navigation.nav_queue, queued[1..]);
}
