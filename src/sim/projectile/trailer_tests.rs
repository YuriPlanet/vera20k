//! Native signed-frame producer and shared scalable-list controls.
use super::*;
use crate::rules::{ini_parser::IniFile, projectile_type::ProjectileType};
use serde_json::{Value, json};

#[test]
fn projectile_trailer_launch_placement_matches_original_bullet_fire() {
    use crate::map::resolved_terrain::{test_flat_cell, test_grid};
    use crate::sim::world::Simulation;
    use crate::util::native_x87::NativeF64Bits;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let kind = retail.rules.projectile("Torpedo").unwrap();
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/projectile_trailer.json",
    ))
    .unwrap();
    let point = |value: &Value| {
        ProjectileCoord::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
    };
    let world = |input: &Value| {
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(test_grid(25, 25, |x, y| {
            let mut cell = test_flat_cell(x, y);
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.slope_type = input["slope"].as_u64().unwrap() as u8;
            if input["live_bridge"].as_bool().unwrap() {
                cell.bridge_facts.raw_flags = 0x100;
            }
            cell
        }));
        sim
    };
    for row in corpus["placement_controls"]["fixup"].as_array().unwrap() {
        let input = &row["supplied"];
        let sim = world(input);
        assert_eq!(
            bullet_unlimbo_coord(
                sim.resolved_terrain.as_ref(),
                &sim.effective_shared_cell_dummy(),
                point(&input["origin"]),
            ),
            point(&row["adjusted"]),
            "{}",
            row["name"]
        );
    }
    let rows = corpus["placement_controls"]["fire"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["name"] == "surface_below_far"));
    for row in rows {
        let input = &row["supplied"];
        let mut sim = world(input);
        let target = &input["target_cell"];
        let mut shot = super::tests::spawn(ProjectileTarget::Cell {
            rx: target[0].as_u64().unwrap() as u16,
            ry: target[1].as_u64().unwrap() as u16,
        });
        shot.source_id = 0;
        shot.origin = point(&input["origin"]);
        shot.initial_target_position = point(&row["target_coord"]);
        shot.payload = ProjectilePayload::new(
            0,
            sim.interner.intern("AP"),
            Some(sim.interner.intern(&kind.id)),
            retail.rules.weapon("SubTorpedo").unwrap().bright,
        )
        .with_installed_weapon(Some(sim.interner.intern("SubTorpedo")));
        shot.flat = kind.flat;
        shot.arm_frames = kind.arm;
        // This test supplies the already-normalized Fire velocity, just as
        // the existing launch owner does. It compares placement and detector
        // setup against full original Fire468670, not the preceding FireAt.
        shot.velocity = ProjectileVelocity::from_native(std::array::from_fn(|axis| {
            NativeF64Bits::from_bits(
                u64::from_str_radix(row["velocity"]["bits"][axis].as_str().unwrap(), 16).unwrap(),
            )
        }));
        let id = sim.substrate.allocate_stable_id();
        sim.admit_projectile(id, shot);
        let bullet = sim.projectiles.get(id).unwrap();
        assert_eq!(bullet.position, point(&row["location"]), "{}", row["name"]);
        assert_eq!(bullet.launch_origin, point(&row["fire_coord"]));
        assert_eq!(bullet.launch_target, point(&row["target_coord"]));
        assert_eq!(
            bullet.previous_cell,
            (
                row["last_cell"][0].as_i64().unwrap() as i16,
                row["last_cell"][1].as_i64().unwrap() as i16,
            )
        );
        assert_eq!(
            bullet.velocity, shot.velocity,
            "placement preserves velocity"
        );
        let detector = &row["detector"];
        assert_eq!(bullet.last_target_position, point(&detector["reference"]));
        assert_eq!(
            i64::from(bullet.last_distance_half),
            detector["distance_watermark"].as_i64().unwrap(),
            "{}: detector starts at placed Location, not FireCoord",
            row["name"]
        );
        assert_eq!(
            i64::from(bullet.arm_timer.start_frame()),
            detector["arm_start"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(bullet.arm_timer.duration()),
            detector["arm_duration"].as_i64().unwrap()
        );
    }
}

#[test]
fn projectile_trailer_header_matches_original_signed_cadence_and_wait_gates() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/projectile_trailer.json",
    ))
    .unwrap();
    let mut compared = 0;
    let mut excluded = Vec::new();
    for row in corpus["rows"].as_array().unwrap() {
        let supplied = &row["supplied"];
        // The store represents admitted/live Bullets; allocation failure and
        // a supplied already-dead header have no Rust receiver.
        if supplied["alive"] == false || supplied["allocation_failure"] == true {
            excluded.push(row["name"].as_str().unwrap());
            continue;
        }
        let mut art = String::from("[SUBT]\n");
        if supplied["trailer"] == true {
            art.push_str("Trailer=BBBLELRG\n");
        }
        if let Some(delay) = supplied["art_delay"].as_i64() {
            art.push_str(&format!("SpawnDelay={delay}\n"));
        }
        let art = IniFile::from_str(&art);
        let rules = IniFile::from_str("[Torpedo]\nImage=SUBT\n");
        let kind = ProjectileType::from_ini_section(
            "Torpedo",
            rules.section("Torpedo").unwrap(),
            art.section("SUBT"),
        );
        assert_eq!(
            i64::from(kind.spawn_delay),
            row["spawn_delay"].as_i64().unwrap()
        );
        let mut store = ProjectileStore::new();
        let mut spawn = super::tests::spawn(ProjectileTarget::Cell { rx: 16, ry: 20 });
        let type_id = InternedId::from_index(1);
        spawn.payload = ProjectilePayload::new(0, InternedId::from_index(2), Some(type_id), false);
        store.set_scaled_spawn_delay(type_id, supplied["scaled_delay"].as_i64().unwrap() as i32);
        let p = &supplied["position"];
        spawn.origin = ProjectileCoord::new(
            p[0].as_i64().unwrap() as i32,
            p[1].as_i64().unwrap() as i32,
            p[2].as_i64().unwrap() as i32,
        );
        spawn.native_unique_id = row["bullet_native_id"].as_i64().unwrap() as i32;
        let id = store.spawn(1, spawn);
        let waiting = supplied["waiting"].as_str().unwrap();
        let projectile = store.projectiles.get_mut(&id).unwrap();
        projectile.awaiting_anim = waiting != "no";
        projectile.awaited_anim = (waiting == "attached").then_some(99);
        let head = store.begin_ai_visit(id).unwrap();
        assert_eq!(
            store.get(id).unwrap().awaiting_anim,
            row["waiting_after"].as_bool().unwrap(),
            "{}",
            row["name"]
        );
        let result = std::panic::catch_unwind(|| {
            store.trailer(&head, &kind, supplied["frame"].as_i64().unwrap() as u32)
        });
        if row.get("divide_fault").is_some() {
            assert!(
                result.is_err(),
                "{}: native #DE has no silent cadence",
                row["name"]
            );
        } else {
            let actual: Vec<_> = result
                .unwrap()
                .into_iter()
                .map(|(name, p)| json!({"anim_type":name,"position":[p.x,p.y,p.z]}))
                .collect();
            let expected: Vec<_> = row["emissions"].as_array().unwrap().iter().map(|emission|
                json!({"anim_type":emission["anim_type"],"position":emission["position"]})).collect();
            assert_eq!(actual, expected, "{}", row["name"]);
        }
        compared += 1;
    }
    assert_eq!(excluded, ["dead_before_divide", "allocation_failure"]);
    assert_eq!(
        compared,
        corpus["rows"].as_array().unwrap().len() - excluded.len()
    );
}

fn scalable_kind(name: &str, base: i32) -> ProjectileType {
    let ini = IniFile::from_str(&format!("[{name}]\nImage=SUBT\nScalable=yes\n"));
    let art = IniFile::from_str(&format!("[SUBT]\nTrailer=BBBLELRG\nSpawnDelay={base}\n"));
    ProjectileType::from_ini_section(name, ini.section(name).unwrap(), art.section("SUBT"))
}

fn supplied_scalable_bullet(store: &mut ProjectileStore, id: u64, type_id: InternedId) {
    let mut spawn = super::tests::spawn(ProjectileTarget::Cell { rx: 16, ry: 20 });
    spawn.origin = ProjectileCoord::new(2688, 5248, 624);
    spawn.payload = ProjectilePayload::new(0, InternedId::from_index(0), Some(type_id), false);
    store.spawn(id, spawn);
}

/// Actual46B280/4684E0 vectors and full-header results. The fixture supplies
/// constructed Bullets (not UnitFire) and explicit original callbacks; world
/// tests separately exercise the producer and common broadcast transaction.
#[test]
fn scalable_members_type_cadence_and_callbacks_match_original() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/projectile_trailer.json",
    ))
    .unwrap();
    let controls = &corpus["scalable_controls"];
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut interner = crate::sim::intern::StringInterner::new();
    // The original reads Torpedo against physical ART SpawnDelay3, then
    // changes the authored ART to7 before constructing/reading the B type.
    let kinds = [
        scalable_kind("Torpedo", 3),
        scalable_kind("ScalableTrailerB", 7),
        retail.rules.projectile("MedusaProjectile").unwrap().clone(),
    ];
    let ids: BTreeMap<_, _> = kinds
        .iter()
        .map(|kind| (kind.id.as_str(), interner.intern(&kind.id)))
        .collect();
    let state = |store: &ProjectileStore, native: &Value| {
        let types: serde_json::Map<_, _> = native["types"].as_object().unwrap().keys().map(|name| {
            let kind = kinds.iter().find(|kind| &kind.id == name).unwrap();
            (name.clone(), json!({"base_delay":kind.spawn_delay,"scaled_delay":store.scaled_spawn_delay(ids[name.as_str()]),
                "scalable":kind.scalable,"trailer":kind.trailer}))
        }).collect();
        json!({"members":store.scalable_members(),"count":store.scalable_members().len(),"types":types})
    };
    for rows in [
        controls["rows"].as_array().unwrap(),
        controls["sole_retirement"]["rows"].as_array().unwrap(),
    ] {
        let mut store = ProjectileStore::new();
        for row in rows {
            let id = row["bullet"].as_u64().unwrap();
            assert_eq!(
                state(&store, &row["before"]),
                row["before"],
                "{} before",
                row["name"]
            );
            match row["action"].as_str().unwrap() {
                "enroll" => {
                    let name = row["bullet_type"].as_str().unwrap();
                    supplied_scalable_bullet(&mut store, id, ids[name]);
                    let kind = kinds.iter().find(|kind| kind.id == name).unwrap();
                    store.init_scalable(id, kind);
                }
                "expire" | "uninit" | "drain" => {
                    for callback in row["callbacks"].as_array().unwrap() {
                        assert!(
                            store
                                .pointer_expired(
                                    callback["receiver"].as_u64().unwrap(),
                                    callback["expired"].as_u64().unwrap(),
                                    ProjectileTarget::None,
                                    callback["control"].as_bool().unwrap()
                                )
                                .is_some()
                        );
                    }
                    if row["action"] == "drain" {
                        assert!(store.remove(id).is_some());
                    }
                }
                "header" => {
                    let header = &row["header"];
                    let type_id = store.get(id).unwrap().payload.projectile_type().unwrap();
                    let kind = kinds
                        .iter()
                        .find(|kind| ids[kind.id.as_str()] == type_id)
                        .unwrap();
                    let head = store.begin_ai_visit(id).unwrap();
                    let actual: Vec<_> = store
                        .trailer(
                            &head,
                            kind,
                            header["supplied"]["frame"].as_i64().unwrap() as u32,
                        )
                        .into_iter()
                        .map(|(name, p)| json!({"anim_type":name,"position":[p.x,p.y,p.z]}))
                        .collect();
                    let expected: Vec<_> = header["emissions"].as_array().unwrap().iter()
                        .map(|emission| json!({"anim_type":emission["anim_type"],"position":emission["position"]})).collect();
                    assert_eq!(actual, expected, "{} header", row["name"]);
                }
                other => panic!("unhandled original operation {other}"),
            }
            assert_eq!(
                state(&store, &row["after"]),
                row["after"],
                "{} after",
                row["name"]
            );
            if row["action"] != "header" {
                assert_eq!(
                    row["before_rng"], row["after_rng"],
                    "native list operations draw no RNG"
                );
                assert_eq!(
                    row["before_native_id"], row["after_native_id"],
                    "native list operations allocate no IDs"
                );
            }
        }
    }
    for control in controls["overflow_controls"].as_array().unwrap() {
        let first = &control["states"][0]["types"]["Torpedo"];
        let kind = scalable_kind("Torpedo", first["base_delay"].as_i64().unwrap() as i32);
        let mut store = ProjectileStore::new();
        for (index, native) in control["states"].as_array().unwrap().iter().enumerate() {
            let id = index as u64 + 1;
            supplied_scalable_bullet(&mut store, id, ids["Torpedo"]);
            store.init_scalable(id, &kind);
            assert_eq!(
                store.scalable_members(),
                native["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                store.scaled_spawn_delay(ids["Torpedo"]),
                native["types"]["Torpedo"]["scaled_delay"].as_i64().unwrap() as i32
            );
        }
    }
}

#[test]
fn scalable_expiry_removes_one_duplicate_per_present_bullet_callback() {
    let kind = scalable_kind("Torpedo", 3);
    let type_id = InternedId::from_index(1);
    let mut store = ProjectileStore::new();
    supplied_scalable_bullet(&mut store, 1, type_id);
    supplied_scalable_bullet(&mut store, 2, type_id);
    assert!(
        store.scalable_members().is_empty(),
        "generic construction does not enroll"
    );
    for _ in 0..3 {
        store.init_scalable(1, &kind);
    }
    assert_eq!(store.scalable_members(), [1, 1, 1]);
    assert_eq!(
        store
            .pointer_expired(999, 1, ProjectileTarget::None, true)
            .map(|(_, remaining)| remaining),
        None
    );
    assert_eq!(
        store.scalable_members(),
        [1, 1, 1],
        "no subscriber means no callback"
    );
    assert_eq!(
        store
            .pointer_expired(1, 1, ProjectileTarget::None, false)
            .map(|(_, remaining)| remaining),
        Some(false)
    );
    assert_eq!(store.scalable_members(), [1, 1, 1]);
    assert_eq!(
        store
            .pointer_expired(1, 1, ProjectileTarget::None, true)
            .map(|(_, remaining)| remaining),
        Some(true)
    );
    assert_eq!(store.scalable_members(), [1, 1]);
    assert_eq!(
        store
            .pointer_expired(2, 1, ProjectileTarget::None, true)
            .map(|(_, remaining)| remaining),
        Some(true)
    );
    assert_eq!(store.scalable_members(), [1]);
    assert_eq!(
        store.scaled_spawn_delay(type_id),
        3,
        "expiry does not recompute type cadence"
    );
    let mut no_longer_scalable = kind.clone();
    no_longer_scalable.scalable = false;
    let head = store.begin_ai_visit(1).unwrap();
    assert!(
        store.trailer(&head, &no_longer_scalable, 3).is_some(),
        "header reads retained override without a Scalable flag gate"
    );
}

/// A receipt after the last match permits the remaining native subscribers
/// to repair their own refs without repeating a known-missing vector search.
/// This checks the permission's semantics, not elapsed time or search internals.
#[test]
fn scalable_expiry_last_match_receipt_keeps_later_subscriber_repairs() {
    let kind = scalable_kind("Torpedo", 3);
    let type_id = InternedId::from_index(1);
    let mut store = ProjectileStore::new();
    supplied_scalable_bullet(&mut store, 1, type_id);
    store.init_scalable(1, &kind);
    for id in 2..=64 {
        supplied_scalable_bullet(&mut store, id, type_id);
        let bullet = store.get_mut(id).unwrap();
        bullet.source_id = 1;
        bullet.target = ProjectileTarget::Entity(1);
    }
    let mut pending = true;
    for receiver in 1..=64 {
        (_, pending) = store
            .pointer_expired(
                receiver,
                1,
                ProjectileTarget::Cell { rx: 10, ry: 20 },
                pending,
            )
            .unwrap();
        assert!(
            !pending,
            "the sole match was removed at the first actual subscriber"
        );
        if receiver != 1 {
            let bullet = store.get(receiver).unwrap();
            assert_eq!(bullet.source_id, crate::sim::combat::RAD_NO_ATTACKER);
            assert_eq!(bullet.target, ProjectileTarget::Cell { rx: 10, ry: 20 });
        }
    }
    assert!(store.scalable_members().is_empty());
    assert_eq!(store.scaled_spawn_delay(type_id), 3);
}
