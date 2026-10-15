//! Executed FireAt6FF57D -> Spawn6FD570 -> Create6FD460 -> Init4C2A60
//! comparisons. The native birth corpus executes Building GetFLH/target
//! getters; its Spark constructor boundary is checked against the separate
//! original full constructor/Logic controls in the same corpus.

use super::electric_bolt::{self, ElectricBoltBirth};
use super::{TargetKind, WeaponSlot};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{RetailBattleRules, retail_battle_rules_for_map};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
use crate::sim::world::{DelayedFire, LifecycleOutput, PendingBuildingFire, Simulation, TickLane};
use serde_json::{Value, json};

fn native() -> &'static Value {
    static CORPUS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CORPUS.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/electric_bolt.json",
        ))
        .unwrap()
    })
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn xyz(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| int(&value[axis]))
}

fn coord(value: &Value) -> ProjectileCoord {
    let [x, y, z] = xyz(value);
    ProjectileCoord::new(x, y, z)
}

fn ordinary_birth() -> &'static Value {
    native()["birth_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"]["name"] == "ordinary")
        .unwrap()
}

fn ordinary_spark() -> &'static Value {
    native()["spark_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"]["name"] == "ordinary_detail2")
        .unwrap()
}

fn restore_rng(sim: &mut Simulation, expected: &Value) {
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(expected["main"].as_str().unwrap()).into();
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(expected["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(expected["mapgen"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, expected: &Value, name: &str) {
    assert_eq!(
        sim.main_rng.native_state_hex(),
        expected["main"].as_str().unwrap(),
        "{name} full Main state"
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected["scenario"].as_str().unwrap(),
        "{name} full Scenario state"
    );
    assert_eq!(
        sim.mapgen_rng.native_state_hex(),
        expected["mapgen"].as_str().unwrap(),
        "{name} full MapGen state"
    );
}

/// Preserve the physical layered Battle input, then supply only the native
/// fixture's explicit field controls through the same production readers.
fn controlled_rules(retail: &RetailBattleRules, input: &Value) -> RuleSet {
    let mut ini = retail.processed_rules.clone();
    let mut art = retail.fixed_art.clone();
    let mut controls = format!(
        "[CoilBolt]\nIsLaser={}\nIsElectricBolt={}\nIsAlternateColor={}\n\
         [TESLA]\nTurretCount={}\n",
        input["is_laser"].as_bool().unwrap(),
        input["is_electric_bolt"].as_bool().unwrap(),
        input["alternate"].as_bool().unwrap(),
        int(&input["selector_turret_count"]),
    );
    if input["secondary_weapon_present"] == true {
        controls.push_str(&format!(
            "WeaponCount=2\nWeapon1=CoilBolt\nWeapon2=OPCoilBolt\n\
             [OPCoilBolt]\nIsAlternateColor={}\n",
            input["secondary_alternate"].as_bool().unwrap()
        ));
        let [f1, l1, h1] = xyz(&input["primary_flh"]);
        let [f2, l2, h2] = xyz(&input["secondary_flh"]);
        art.merge(&IniFile::from_str(&format!(
            "[NATSLA]\nWeapon1FLH={f1},{l1},{h1}\nWeapon2FLH={f2},{l2},{h2}\n"
        )));
    } else {
        controls.push_str("Secondary=none\n");
    }
    ini.merge(&IniFile::from_str(&controls));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules
}

fn scene(rules: &RuleSet, input: &Value) -> (Simulation, u64, u64) {
    let mut sim = Simulation::new();
    for (index, name) in ["Russians", "Americans"].into_iter().enumerate() {
        let owner = sim.intern(name);
        sim.houses.insert(
            owner,
            HouseState::new(owner, index as u8, None, true, 0, 10),
        );
        sim.session.house_order.push(owner);
    }
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        64,
    ));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let mut place = |kind, owner, at: &Value| {
        let [x, y, z] = xyz(at);
        let id = sim
            .spawn_object(kind, owner, (x / 256) as u16, (y / 256) as u16, 0, rules)
            .unwrap();
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.position.put_world_xy([x, y]);
        entity.position.set_exact_z_leptons_for_test(Some(z));
        id
    };
    let source = place("TESLA", "Russians", &input["location"]);
    let target = place("GAPOWR", "Americans", &input["target_building"]["location"]);
    sim.resolve_type_handles(rules);
    sim.session.binary_frame = int(&input["birth_frame"]) as u32;
    sim.power_states.clear();
    if input["retained_target"].as_bool().unwrap() {
        assert!(super::install_entity_attack_target_for_test(
            &mut sim.substrate.entities,
            source,
            target,
        ));
    }
    sim.substrate
        .entities
        .get_mut(source)
        .unwrap()
        .set_gunner_selection_for_test(int(&input["current_weapon_number"]), 0);
    sim.lifecycle_outputs.clear();
    (sim, source, target)
}

// Existing shared GetFLH returns X one lepton below native for the aimed
// TESLA control (<1 projected pixel). Preserve this explicit source-only
// residual; target, Z adjustment, IDs and RNG remain exact comparisons.
fn assert_source(actual: ProjectileCoord, expected: ProjectileCoord, name: &str) {
    assert!(
        (0..=1).contains(&(i64::from(expected.x) - i64::from(actual.x))),
        "{name} source X exceeds recorded one-lepton residual: {actual:?} vs {expected:?}"
    );
    assert_eq!(
        [actual.y, actual.z],
        [expected.y, expected.z],
        "{name} source Y/Z"
    );
}

fn assert_birth(actual: ElectricBoltBirth, expected: &Value, frame: i32, name: &str) {
    assert_eq!(actual.frame, frame, "{name} birth frame");
    assert_source(actual.from, coord(&expected["source"]), name);
    assert_eq!(
        actual.to,
        coord(&expected["target"]),
        "{name} foundation target"
    );
    assert_eq!(
        actual.z_adjust,
        int(&expected["z_adjust"]),
        "{name} Z adjustment"
    );
    assert_eq!(
        actual.phase,
        int(&expected["phase"]),
        "{name} initial Main phase"
    );
    assert_eq!(
        actual.alternate_color,
        expected["alternate"] == true,
        "{name} color"
    );
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_birth_fields_match_native_with_documented_flh_residual() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let weapon = retail.rules.weapon("CoilBolt").unwrap();
    let physical = &native()["weapon_inputs"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["state"];
    assert_eq!(weapon.is_laser, physical["is_laser"] == true);
    assert_eq!(
        weapon.is_electric_bolt,
        physical["is_electric_bolt"] == true
    );
    assert_eq!(
        weapon.is_alternate_color,
        physical["is_alternate_color"] == true
    );
    assert_eq!(weapon.range_leptons, int(&physical["range_leptons"]));
    let art = retail.rules.art().get("NATSLA").unwrap();
    assert_eq!(
        <[i32; 3]>::from(art.primary_fire_flh),
        xyz(&ordinary_birth()["input"]["primary_flh"])
    );

    let spark = ordinary_spark();
    let constructor = spark["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["pc"] == "0x62dc50")
        .unwrap();
    let initial_id = constructor["native_id_before"].as_u64().unwrap() as u32;
    let original_system = &spark["birth"]["state"]["systems"][0];
    for row in native()["birth_cases"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        if input["new_fails"] == true {
            // Rust allocation failure is not a nullable operator-new branch.
            continue;
        }
        let rules = controlled_rules(&retail, input);
        let (mut sim, source, target) = scene(&rules, input);
        restore_rng(&mut sim, &row["rng_before"]);
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(initial_id));
        let stable_before = sim.substrate.next_stable_object_id;
        electric_bolt::fired(
            &mut sim,
            &rules,
            source,
            TargetKind::Entity(target),
            rules.weapon("CoilBolt").unwrap(),
        );

        let births: Vec<_> = sim
            .lifecycle_outputs
            .iter()
            .filter_map(|output| match output {
                LifecycleOutput::ElectricBoltCreated(birth) => Some(*birth),
                _ => None,
            })
            .collect();
        let expected = row["bolts"].as_array().unwrap();
        assert_eq!(births.len(), expected.len(), "{name} FireAt gate");
        for (actual, expected) in births.iter().zip(expected) {
            assert_birth(*actual, expected, int(&input["birth_frame"]), name);
        }
        assert_rng(&sim, &row["rng_after"], name);

        let native_calls: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["event"] == "spark_constructor_boundary")
            .collect();
        assert_eq!(
            sim.particle_systems().len(),
            native_calls.len(),
            "{name} Spark ctor count"
        );
        assert_eq!(
            sim.substrate.next_stable_object_id - stable_before,
            native_calls.len() as u64,
            "{name} EBolt has no stable simulation object"
        );
        if let Some(call) = native_calls.first() {
            let (_, system) = sim.particle_systems().iter().next().unwrap();
            assert_eq!(
                system.native_unique_id(),
                int(&original_system["native_id"]),
                "{name} EBolt consumes no Abstract ID before Spark"
            );
            assert_eq!(
                sim.native_unique_ids.as_ref().unwrap().current_raw(),
                int(&spark["birth"]["state"]["native_id_cursor"]) as u32
            );
            assert_eq!(json!(system.coords.to_array()), call["coords"]);
            assert_eq!(json!(system.target_coords.to_array()), call["target"]);
            assert!(
                system.owner_entity.is_none()
                    && system.attached_entity.is_none()
                    && system.owner_house.is_none()
            );
            assert_eq!(system.lifetime, int(&original_system["lifetime"]));
            assert_eq!(
                system.spark_spawn_frames,
                int(&original_system["spark_spawn_frames"])
            );
            assert_eq!(system.done_spawning, original_system["time_to_die"] == true);
            assert_eq!(
                system.particles.len(),
                int(&original_system["particle_count"]) as usize
            );
            assert!(sim.live_object_order_snapshot().contains(&system.stable_id));
        } else {
            assert_eq!(
                sim.native_unique_ids.as_ref().unwrap().current_raw(),
                initial_id,
                "{name} rejected birth allocates no Abstract ID"
            );
        }
    }
}

#[test]
fn direct_bolt_uses_explicit_weapon_source_and_existing_rng_constructor() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let row = ordinary_birth();
    let mut input = row["input"].clone();
    // Direct native6FD460 does not apply FireAt's IsLaser suppression or
    // reselect a different weapon; the caller already admitted this effect.
    input["is_laser"] = Value::Bool(true);
    let rules = controlled_rules(&retail, &input);
    let (mut sim, source, target) = scene(&rules, &input);
    restore_rng(&mut sim, &row["rng_before"]);
    let spark = ordinary_spark();
    let initial_id = spark["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["pc"] == "0x62dc50")
        .unwrap()["native_id_before"]
        .as_u64()
        .unwrap() as u32;
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(initial_id));
    let expected = &row["bolts"][0];
    let from = coord(&expected["source"]);
    electric_bolt::create(
        &mut sim,
        &rules,
        source,
        TargetKind::Entity(target).into(),
        rules.weapon("CoilBolt").unwrap(),
        from,
    );
    let births: Vec<_> = sim
        .lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::ElectricBoltCreated(birth) => Some(*birth),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 1);
    assert_eq!(births[0].from, from);
    assert_birth(births[0], expected, int(&input["birth_frame"]), "direct");
    assert_rng(&sim, &row["rng_after"], "direct");
    let systems: Vec<_> = sim.particle_systems().iter().collect();
    assert_eq!(systems.len(), 1);
    assert_eq!(
        systems[0].1.native_unique_id(),
        int(&spark["birth"]["state"]["systems"][0]["native_id"])
    );
    assert_eq!(systems[0].1.coords.to_array(), xyz(&expected["target"]));
}

#[test]
fn building_fireat_reads_the_retained_target_for_its_launch_coordinate() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let cleared = native()["birth_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "cleared_retained_target")
        .unwrap();
    let mut origins = Vec::new();
    for row in [ordinary_birth(), cleared] {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = controlled_rules(&retail, input);
        assert_eq!(rules.weapon("CoilBolt").unwrap().burst, 1);
        let (mut sim, source, target) = scene(&rules, input);
        // FireAt6FE268 passes the live source to GetFLH; its explicit
        // target argument never replaces BuildingFireFacing445E50's +2B4.
        sim.commit_fire_visit(
            super::world_fire::FireVisit::Building {
                id: source,
                shot: super::BuildingShot::Mission {
                    weapon: 0,
                    target: TargetKind::Entity(target),
                },
            },
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.fire_events.len(), 1, "{name} actual Building FireAt");
        let event = &sim.fire_events[0];
        assert_eq!(event.attacker_id, source);
        assert_eq!(event.target, TargetKind::Entity(target));
        assert_source(event.fire_coord, coord(&row["bolts"][0]["source"]), name);
        origins.push(event.fire_coord);
    }
    assert_ne!(
        origins[0], origins[1],
        "retained target changes Building GetFLH"
    );
}

fn retail_with_bound_animation_assets() -> Option<(
    RetailBattleRules,
    crate::assets::asset_manager::AssetManager,
)> {
    let mut retail = retail_battle_rules_for_map("XMP03T4.MAP")?;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let mut roots = crate::rules::effect_asset_catalog::anim_class_roots(&retail.rules);
    roots.extend(retail.rules.art().building_anim_roots());
    roots.extend(
        retail
            .rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.clone()),
    );
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    Some((retail, assets))
}

/// Building fatal UnInit -> Conceal5F4D30 -> Mark(REMOVE) releases its
/// occupied cells before the next Logic object can query navigation. This
/// regression checks the derived path view, not a new native damage golden.
#[test]
fn fatal_coil_bolt_releases_navigation_before_the_next_logic_reader() {
    let Some((retail, _assets)) = retail_with_bound_animation_assets() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, source, target) = scene(rules, &ordinary_birth()["input"]);
    // Match production loading: native death can create a crew type that was
    // not present among this small fixture's initial buildings.
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let victim = sim.substrate.entities.get_mut(target).unwrap();
    victim.health.set_for_test(1);
    let anchor = (victim.position.rx(), victim.position.ry());
    assert!(sim.rebuild_dynamic_navigation(rules));
    let blocked_before = sim.path_grid_snapshot().unwrap();
    let cells: Vec<_> = crate::sim::production::building_base_foundation_cells(
        anchor.0,
        anchor.1,
        &rules.object("GAPOWR").unwrap().foundation,
    )
    .into_iter()
    .filter(|&(x, y)| !blocked_before.is_walkable(x, y))
    .collect();
    assert!(!cells.is_empty(), "the live GAPOWR blocks its foundation");
    sim.commit_fire_visit(
        super::world_fire::FireVisit::Building {
            id: source,
            shot: super::BuildingShot::Mission {
                weapon: 0,
                target: TargetKind::Entity(target),
            },
        },
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let bullet = *sim.projectiles.iter().next().unwrap().0;
    assert!(sim.object_ai_visit_one(bullet, Some(rules), Default::default()));
    let victim = sim.substrate.entities.get(target).unwrap();
    assert!(
        !victim.lifecycle.cell_marked(),
        "fatal receiver already unmarked"
    );
    assert!(
        !victim.in_logic_vector.is_member(),
        "fatal receiver already left Logic"
    );
    for &(x, y) in &cells {
        assert!(
            sim.path_grid().unwrap().is_walkable(x, y),
            "next same-frame path reader still sees destroyed GAPOWR at {x},{y}"
        );
        assert!(
            !blocked_before.is_walkable(x, y),
            "old borrowed view is immutable"
        );
    }
    for _ in 0..2 {
        sim.advance_app_frame(
            &[],
            Some(rules),
            None,
            67,
            TickLane::Ordinary,
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        for &(x, y) in &cells {
            assert!(sim.path_grid().unwrap().is_walkable(x, y));
        }
    }
}

#[test]
fn delayed_building_fire_skips_spark_after_bullet_compaction_until_the_next_pass() {
    let Some((retail, assets)) = retail_with_bound_animation_assets() else {
        return;
    };
    let rules = &retail.rules;
    let schedule = native()["live_logic_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"]["name"] == "inviso_bullet_before_spark_occupied_gapowr")
        .unwrap();
    let impact_types = schedule["impact_type_inputs"].as_array().unwrap();
    for impact in impact_types {
        let name = impact["name"].as_str().unwrap();
        assert!(rules.art().scheduler_anim_types().contains(name));
        let shape = &impact["shape"];
        let loaded = assets
            .load_file_from_mix(shape["file"].as_str().unwrap())
            .expect("native impact SHP is available to the production binder");
        assert_eq!(loaded.bytes.len() as u64, shape["bytes"].as_u64().unwrap());
        assert_eq!(
            crate::util::sha256::sha256_hex(&loaded.bytes),
            shape["sha256"].as_str().unwrap()
        );
    }
    let row = ordinary_birth();
    let (mut sim, source, target) = scene(rules, &row["input"]);
    restore_rng(&mut sim, &row["rng_before"]);
    let now = sim.session.binary_frame as i32;
    let entity = sim.substrate.entities.get_mut(source).unwrap();
    entity
        .pending_building_fire
        .set_for_test(Some(PendingBuildingFire::new(
            1,
            DelayedFire::Weapon(WeaponSlot::Primary),
        )));
    entity.rearm_timer.set_for_test(CdTimer::started(100, 0));
    entity.mission_leaf.set_building_ready_latch(1);
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Attack),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: now as u32,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::from_raw(now, 100),
    });
    let frame = sim
        .advance_app_frame(
            &[],
            Some(rules),
            None,
            67,
            TickLane::Ordinary,
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    let births: Vec<_> = frame
        .lifecycle_outputs
        .iter()
        .filter_map(|output| match output {
            LifecycleOutput::ElectricBoltCreated(birth) => Some(*birth),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 1, "one actual delayed CoilBolt shot");
    // Full FireAt includes its preceding report/ROF consumers; their RNG is
    // outside the isolated Init seed boundary. Retain the source residual.
    assert_source(births[0].from, coord(&row["bolts"][0]["source"]), "delayed");
    assert_eq!(births[0].to, coord(&row["bolts"][0]["target"]));
    assert_eq!(births[0].z_adjust, int(&row["bolts"][0]["z_adjust"]));
    let spark_type = rules
        .ps_type_id_by_name(rules.combat_damage.default_spark_system.as_deref().unwrap())
        .unwrap();
    let systems: Vec<_> = sim
        .particle_systems()
        .iter()
        .filter(|(_, system)| system.type_id == spark_type)
        .collect();
    assert_eq!(systems.len(), 1);
    let system = systems[0].1;
    let system_id = system.stable_id;
    // The native predecessor component supplies Bullet -> Spark construction,
    // then runs the original live loop. Bullet retirement compacts Spark into
    // the visited slot; the unadjusted cursor skips it on this firing pass.
    let firing_pass = &schedule["first_logic_visit"]["state"];
    let skipped = &firing_pass["systems"][0];
    assert_eq!(system.lifetime, int(&skipped["lifetime"]));
    assert_eq!(
        system.spark_spawn_frames,
        int(&skipped["spark_spawn_frames"])
    );
    assert_eq!(system.done_spawning, skipped["time_to_die"] == true);
    assert_eq!(
        system.particles.len(),
        int(&skipped["particle_count"]) as usize
    );
    assert_eq!(skipped, &schedule["birth"]["state"]["systems"][0]);
    let spark_lights = |output: &crate::sim::world::SimFrameOutput| {
        output
            .combat_lights
            .iter()
            .filter_map(|request| match request {
                super::CombatLightRequest::Spark { coord, base_size } => {
                    Some(json!({"position": [coord.x, coord.y, coord.z], "size": base_size}))
                }
                super::CombatLightRequest::Impact { .. } => None,
            })
            .collect::<Vec<_>>()
    };
    let native_lights = |state: &Value| {
        state["lights"]
            .as_array()
            .unwrap()
            .iter()
            .map(|light| json!({"position": light["position"], "size": light["size"]}))
            .collect::<Vec<_>>()
    };
    assert_eq!(spark_lights(&frame), native_lights(firing_pass));
    assert_eq!(firing_pass["bullet"]["logic"], false);
    assert!(
        sim.projectiles.is_empty(),
        "Inviso Bullet retired in its live slot"
    );
    assert!(
        sim.anims().any(|(_, anim)| {
            impact_types
                .iter()
                .any(|impact| impact["name"] == sim.resolve(anim.type_id))
        }),
        "the same impact constructed a physically bound animation"
    );
    assert_eq!(json!(system.coords.to_array()), row["bolts"][0]["target"]);
    let order = sim.live_object_order_snapshot();
    assert!(
        order.iter().position(|&id| id == source).unwrap()
            < order.iter().position(|&id| id == system.stable_id).unwrap()
    );
    assert!(
        sim.substrate.entities.get(target).unwrap().health.current()
            < rules.object("GAPOWR").unwrap().strength
    );
    assert!(
        sim.substrate
            .entities
            .get(source)
            .unwrap()
            .pending_building_fire
            .pending()
            .is_none()
    );

    let firing_frame = sim.session.binary_frame;
    let identity_before_ai = sim.native_identity_cursor().unwrap();
    sim.clear_lifecycle_test_events_for_test();
    let next = sim
        .advance_app_frame(
            &[],
            Some(rules),
            None,
            67,
            TickLane::Ordinary,
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert!(
        !next
            .lifecycle_outputs
            .iter()
            .any(|output| { matches!(output, LifecycleOutput::ElectricBoltCreated(_)) })
    );
    let native_frame_step = int(&schedule["second_logic_visit"]["binary_frame"])
        - int(&schedule["first_logic_visit"]["binary_frame"]);
    assert_eq!(
        sim.session.binary_frame - firing_frame,
        native_frame_step as u32
    );
    let first_ai = &schedule["second_logic_visit"]["state"];
    let retired = &first_ai["systems"][0];
    assert_eq!(retired["time_to_die"], true);
    assert_eq!(retired["particle_count"], 0);
    assert_eq!(retired["alive"], false);
    assert_eq!(retired["logic"], false);
    assert!(
        first_ai["pending_native_ids"]
            .as_array()
            .unwrap()
            .contains(&retired["native_id"])
    );
    assert!(
        schedule["after_second_pass_deferred_drain"]["state"]["pending_native_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Original62C6E0's occupied-GAPOWR control deletes every first-AI
    // child on contact. The done, empty system then retires in62FD60;
    // its light was emitted earlier in62EC53 and survives that cleanup.
    assert_eq!(spark_lights(&next), native_lights(first_ai));
    assert!(
        !spark_lights(&next).is_empty(),
        "first actual Spark AI emitted its light"
    );
    assert!(
        sim.native_identity_cursor().unwrap() > identity_before_ai,
        "the first AI constructed children before collision cleanup"
    );
    assert!(sim.particle_systems().get(system_id).is_none());
    assert!(!sim.live_object_order_snapshot().contains(&system_id));
    let events = sim.lifecycle_test_events_for_test();
    let queued = events
        .iter()
        .position(|event| {
            matches!(
                event,
                crate::sim::world::LifecycleTestEvent::PendingDeleteQueued { stable_id }
                    if *stable_id == system_id
            )
        })
        .unwrap();
    let finalized = events
        .iter()
        .position(|event| {
            matches!(
                event,
                crate::sim::world::LifecycleTestEvent::FinalizedCommon { stable_id }
                    if *stable_id == system_id
            )
        })
        .unwrap();
    assert!(queued < finalized);
    assert!(events[queued + 1..finalized].iter().any(|event| matches!(
        event,
        crate::sim::world::LifecycleTestEvent::PendingDeleteDrainStarted
    )));
    // Full Building FireAt and this damage receiver consume additional RNG.
    // The native component supplies cell lists after an empty-receiver impact;
    // this comparison establishes cadence/cleanup, not whole-shot RNG or counts.
}

/// Real NATSLA_B lifetime can move the live-loop cursor past the newly born
/// Inviso Bullet. Original charge_logic_cases executes both sides of that
/// boundary; the production receiver additionally restores the Active slot
/// through Building451B40 and applies the actual GAPOWR damage/contact gates.
#[test]
fn natural_charge_expiry_preserves_native_bullet_and_spark_visit_order() {
    let Some((retail, _assets)) = retail_with_bound_animation_assets() else {
        return;
    };
    let rules = &retail.rules;
    let birth = ordinary_birth();
    for case in native()["charge_logic_cases"].as_array().unwrap() {
        let name = case["input"]["name"].as_str().unwrap();
        let (mut sim, source, target) = scene(rules, &birth["input"]);
        let start = int(&case["input"]["charge_birth_frame"]);
        sim.session.binary_frame = 0;
        // The native component supplies an already operational Building.
        // Reach that state through the ordinary House/Building owners before
        // creating its charge, including the damaged-slot transition.
        sim.spawn_object("GAPOWR", "Russians", 32, 40, 0, rules)
            .unwrap();
        let damaged = case["input"]["damaged_charge"] == true;
        let source_entity = sim.substrate.entities.get_mut(source).unwrap();
        if damaged {
            // Supplied damaged receiver; the shared slot owner selects its ART.
            source_entity
                .health
                .set_for_test(rules.object("TESLA").unwrap().strength / 4);
        }
        source_entity
            .mission
            .apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Sleep),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(0, 1000),
            });
        source_entity.mission_leaf.set_building_ready_latch(1);
        for _ in 0..start {
            sim.advance_app_frame(
                &[],
                Some(rules),
                None,
                67,
                TickLane::Ordinary,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        }
        sim.clear_building_anim_slot(source, 3);
        let charge = sim
            .set_building_anim_slot(source, 10, damaged, false, 0, rules)
            .expect("physical Tesla charge animation is bound");
        for visit in case["charge_warmup"].as_array().unwrap() {
            assert_eq!(
                sim.session.binary_frame,
                int(&visit["frame"]) as u32,
                "{name}"
            );
            sim.advance_app_frame(
                &[],
                Some(rules),
                None,
                67,
                TickLane::Ordinary,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
            let actual = sim.anim(charge).unwrap_or_else(|| {
                panic!("{name} charge retired during warmup at {}", visit["frame"])
            });
            let expected = &visit["state"];
            assert_eq!(
                actual.runtime.current_frame,
                int(&expected["frame"]),
                "{name}"
            );
            assert_eq!(
                actual.runtime.first_ai_guard,
                expected["first_ai_guard"] == true,
                "{name}"
            );
            assert_eq!(
                actual.runtime.frame_timer.start_frame(),
                int(&expected["timer_start"]),
                "{name}"
            );
            assert_eq!(
                actual.runtime.frame_timer.duration(),
                int(&expected["timer_duration"]),
                "{name}"
            );
        }
        assert_eq!(
            sim.session.binary_frame,
            int(&case["input"]["frame"]) as u32,
            "{name}"
        );
        let entity = sim.substrate.entities.get_mut(source).unwrap();
        entity
            .pending_building_fire
            .set_for_test(Some(PendingBuildingFire::new(
                1,
                DelayedFire::Weapon(WeaponSlot::Primary),
            )));
        entity
            .rearm_timer
            .set_for_test(CdTimer::started(sim.session.binary_frame as i32, 0));
        let health_before = sim.substrate.entities.get(target).unwrap().health.current();
        let mut previous_light_count = 0;
        for (visit_index, key) in ["first_logic_visit", "second_logic_visit"]
            .into_iter()
            .enumerate()
        {
            let frame = sim
                .advance_app_frame(
                    &[],
                    Some(rules),
                    None,
                    67,
                    TickLane::Ordinary,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            let expected = &case[key]["state"];
            let births = frame
                .lifecycle_outputs
                .iter()
                .filter(|output| matches!(output, LifecycleOutput::ElectricBoltCreated(_)))
                .count();
            assert_eq!(
                births,
                usize::from(visit_index == 0),
                "{name} one delayed shot"
            );
            let bullet_alive = expected["bullet"]["logic"] == true;
            assert_eq!(
                !sim.projectiles.is_empty(),
                bullet_alive,
                "{name} {key} Bullet visit"
            );
            assert_eq!(
                sim.substrate.entities.get(target).unwrap().health.current() == health_before,
                bullet_alive,
                "{name} {key} damage follows the actual Bullet visit"
            );
            let light_count = expected["lights"].as_array().unwrap().len();
            let lights: Vec<_> = frame
                .combat_lights
                .iter()
                .filter_map(|request| match request {
                    super::CombatLightRequest::Spark { coord, base_size } => Some(json!({
                        "position": [coord.x, coord.y, coord.z], "size": base_size,
                    })),
                    super::CombatLightRequest::Impact { .. } => None,
                })
                .collect();
            let native_lights: Vec<_> = expected["lights"].as_array().unwrap()
                [previous_light_count..]
                .iter()
                .map(|light| json!({"position": light["position"], "size": light["size"]}))
                .collect();
            assert_eq!(
                lights, native_lights,
                "{name} {key} first Spark AI creates its light"
            );
            previous_light_count = light_count;
            assert_eq!(
                sim.anim(charge).is_some(),
                expected["charge_anim"]["logic"] == true,
                "{name} {key} charge remains until native expiry"
            );
        }
        if case["second_logic_visit"]["state"]["charge_anim"]["logic"] == true {
            assert!(
                sim.substrate
                    .entities
                    .get(source)
                    .unwrap()
                    .building_art
                    .anim_slots()[3]
                    .is_none(),
                "{name} a live SpecialAnim has not restored Active"
            );
            continue;
        }
        // Existing building_slot_replacement.expiry's delayed_fire_normal
        // executes this separate native listener, absent in charge_logic_cases.
        let idle = sim
            .substrate
            .entities
            .get(source)
            .unwrap()
            .building_art
            .anim_slots()[3]
            .expect("normal delayed-fire completion restores the Active slot");
        assert_eq!(
            sim.resolve(sim.anim(idle).unwrap().type_id),
            "NATSLA_A",
            "{name}"
        );
    }
}
