//! Original repair-depot executable comparisons.
//!
//! Corpus: tools/spatial_oracle/building_repair.depot_service.{json,md}.
//! The selected retail strings pass through the production Rules/ART reader.
//! Unit737430→Foot4D8FB0→Techno6F4AB0 radio1C and Building MissionAI5B3060
//! →Guard4496B0/Repair44B780 are replayed through their canonical owners.
//! Coordinates, contacts and the stopped Drive are declared scene inputs;
//! these tests do not prove naturally occurring Guard arrival or animation.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::estimated_health::EstimatedHealth;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::radio::{self, RadioMessage, RadioPayload};
use crate::sim::rng::SimRng;
use crate::sim::stage::StageClass;
use crate::sim::timer::CdTimer;
use crate::sim::world::{ObjectAiCtx, Simulation};
use crate::util::fixed_math::SimFixed;

#[path = "building_repair_waiter_oracle_tests.rs"]
mod waiter_oracle_tests;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_repair.depot_service.json",
    ))
    .unwrap()
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn little_double(text: &str) -> u64 {
    assert_eq!(text.len(), 16);
    u64::from_le_bytes(std::array::from_fn(|n| {
        u8::from_str_radix(&text[n * 2..n * 2 + 2], 16).unwrap()
    }))
}

fn sections_text(sections: &Value) -> String {
    let mut text = String::new();
    for (section, keys) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, raw) in keys.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", raw.as_str().unwrap()));
        }
    }
    text
}

const REGISTRIES: &str = "[VehicleTypes]\n0=MTNK\n1=HTNK\n\
    [BuildingTypes]\n0=GADEPT\n1=NADEPT\n";

fn stock_rules(golden: &Value, case: &Value) -> RuleSet {
    let layers = golden["inputs"]["layers"].as_array().unwrap();
    let mut stack = RulesLayerStack::new(IniFile::from_str(&format!(
        "{REGISTRIES}{}\n",
        sections_text(&layers[0]["selected"]),
    )));
    for layer in &layers[1..] {
        if layer["absent"] == true {
            continue;
        }
        let kind = match layer["file"].as_str().unwrap() {
            "LANGRULE.INI" => RulesLayerKind::LangRule,
            "MPBattleMD.ini" => RulesLayerKind::GameMode,
            "XMP03T4.MAP" => RulesLayerKind::Scenario,
            other => panic!("unknown physical layer {other}"),
        };
        stack.push(kind, IniFile::from_str(&sections_text(&layer["selected"])));
    }
    let mut control = String::from("[General]\n");
    if let Some(step) = case["step"].as_i64() {
        control.push_str(&format!("RepairStep={step}\n"));
    }
    if let Some(bits) = case["percent_bits"].as_str() {
        // The executable controls supply only these exactly representable
        // native parser inputs. A decimal .15 would take a different %f path.
        let raw = match little_double(bits) {
            0x3fd0_0000_0000_0000 => "25%",
            0x3fc3_3333_3333_3333 => "15%",
            0xbfc3_3333_3333_3333 => "-15%",
            0 => "0",
            other => panic!("unrepresented percentage bits {other:016x}"),
        };
        control.push_str(&format!("RepairPercent={raw}\n"));
    }
    let unit = case["unit"].as_str().unwrap_or("HTNK");
    let building = case["building"].as_str().unwrap_or("NADEPT");
    // The native scene supplies Drive4AF540 independently of the selected
    // retail keys. Put that prerequisite in this reached unit section:
    // repeated sections in one physical INI use the first section only.
    // The production Foot constructor also installs Drive at Speed=0.
    control.push_str(&format!(
        "[{unit}]\nLocomotor={{4A582741-9839-11D1-B709-00A024DDAFD1}}\n"
    ));
    for (field, key) in [("cost", "Cost"), ("strength", "Strength")] {
        if let Some(value) = case[field].as_i64() {
            control.push_str(&format!("{key}={value}\n"));
        }
    }
    if let Some(value) = case["manual_reload"].as_bool() {
        control.push_str(&format!(
            "ManualReload={}\n",
            if value { "yes" } else { "no" }
        ));
    }
    control.push_str(&format!("[{building}]\n"));
    for (field, key) in [
        ("unit_repair", "UnitRepair"),
        ("unit_reload", "UnitReload"),
        ("bunker", "Bunker"),
        ("dock_unload", "DockUnload"),
    ] {
        if let Some(value) = case[field].as_bool() {
            control.push_str(&format!("{key}={}\n", if value { "yes" } else { "no" }));
        }
    }
    if let Some(value) = case["stupid_guard_mode"].as_bool() {
        control.push_str(&format!(
            "HasStupidGuardMode={}\n",
            if value { "yes" } else { "no" }
        ));
    }
    stack.push(RulesLayerKind::Scenario, IniFile::from_str(&control));
    let art = IniFile::from_str(&sections_text(&golden["inputs"]["art"]["selected"]));
    let mut rules =
        RuleSet::from_processed_rules(&stack.process_with_fixed_art(&art).unwrap()).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules
}

struct Scene {
    sim: Simulation,
    rules: RuleSet,
    tank: u64,
    depot: u64,
    other: Option<u64>,
}

impl Scene {
    fn name(&self, id: Option<u64>) -> Value {
        match id {
            None => Value::Null,
            Some(id) if id == self.tank => json!("miner"),
            Some(id) if id == self.depot => json!("refinery"),
            Some(id) if Some(id) == self.other => json!("other"),
            Some(id) => panic!("unrepresented pointer {id}"),
        }
    }

    fn nav(&self, nav: Option<NavTargetRef>) -> Value {
        match nav {
            None => Value::Null,
            Some(NavTargetRef::Cell { rx, ry }) => json!([rx, ry]),
            Some(
                NavTargetRef::Building { id }
                | NavTargetRef::Entity { id }
                | NavTargetRef::Object { id },
            ) => self.name(Some(id)),
        }
    }

    fn snapshot(&self) -> Value {
        let unit = self.sim.substrate.entities.get(self.tank).unwrap();
        let building = self.sim.substrate.entities.get(self.depot).unwrap();
        let house = &self.sim.houses[&unit.owner()];
        let stage = building
            .mission_leaf
            .as_building()
            .unwrap()
            .repair_progress();
        let raw_stage = serde_json::to_value(stage).unwrap();
        let rng = self.sim.scenario_rng.logical_view();
        let archive = unit.archive_target().map(|value| match value {
            TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
            TargetKind::Entity(id) => NavTargetRef::Entity { id },
        });
        let at = crate::sim::movement::ground_pose::object_get_coords(
            unit,
            self.sim.resolved_terrain.as_ref(),
        );
        let destination = unit
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .and_then(|drive| drive.destination())
            .map_or_else(|| json!([0, 0, 0]), |at| json!([at.x, at.y, at.z]));
        json!({
            "frame": self.sim.session.binary_frame,
            "unit_coordinate": [at.x,at.y,at.z],
            "health": unit.health.current,
            "estimate": unit.estimated_health.get(),
            "balance": house.economy.credits,
            "spent": house.economy.spent_credits,
            "unit_mission": unit.mission.current().raw(),
            "unit_queued": unit.mission.queued().raw(),
            "unit_nav": self.nav(unit.navigation.nav_com),
            "unit_nav_aux": self.nav(unit.navigation.nav_com_aux),
            "unit_archive": self.nav(archive),
            "pending_entry": self.name(unit.pending_entry()),
            "unit_contacts": (0..unit.radio_contacts.capacity()).map(|n| self.name(unit.radio_contacts.slot(n))).collect::<Vec<_>>(),
            "building_contacts": (0..building.radio_contacts.capacity()).map(|n| self.name(building.radio_contacts.slot(n))).collect::<Vec<_>>(),
            "unit_tether": u8::from(unit.dock_entered_with.is_some()),
            "building_tether": u8::from(building.dock_entered_with.is_some()),
            "building_mission": building.mission.current().raw(),
            "building_queued": building.mission.queued().raw(),
            "status": building.mission.handler_state(),
            "dispatch_words": [building.mission.dispatch_timer().start_frame(),building.mission.dispatch_timer().delay()],
            "stage": [json!(stage.value()),raw_stage["changed"].clone(),json!(stage.timer().start_frame()),json!(stage.timer().duration()),json!(stage.rate()),raw_stage["increment"].clone()],
            "repairing": building.building_ready_latch(),
            "locomotor_powered": u8::from(unit.locomotor.as_ref().unwrap().is_powered()),
            "locomotor_destination": destination,
            "scenario_rng": {"disabled":rng.disabled,"index_a":rng.index_a,"index_b":rng.index_b,"state":rng.words}
        })
    }

    fn compare(&self, native: &Value, context: &str) {
        for (field, value) in self.snapshot().as_object().unwrap() {
            assert_eq!(value, &native[field], "{context}: {field}");
        }
    }

    fn visit_depot(&mut self) {
        // This corpus executes MissionAI5B3060, excluding the Building AI
        // UpdateAnimation prefix. Use that same production dispatch seam.
        super::dispatch(
            &mut self.sim,
            self.depot,
            Some(&self.rules),
            ObjectAiCtx::default(),
        );
    }

    fn restore_snapshot_at_native_seed0_boundary(&mut self) {
        use crate::sim::snapshot::GameSnapshot;
        let progress = *self
            .sim
            .substrate
            .entities
            .get(self.depot)
            .unwrap()
            .mission_leaf
            .as_building()
            .unwrap()
            .repair_progress();
        let terrain = self.sim.resolved_terrain.as_ref().unwrap().clone();
        let path = self.sim.path_grid_snapshot();
        let bytes = GameSnapshot::save(&self.sim, 0, 0, "repair progress continuation", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &self.rules,
        );
        restored.install_fixture_path_grid(path.as_deref());
        assert_eq!(
            *restored
                .substrate
                .entities
                .get(self.depot)
                .unwrap()
                .mission_leaf
                .as_building()
                .unwrap()
                .repair_progress(),
            progress
        );
        assert_eq!(
            restored.scenario_rng.logical_state(),
            SimRng::new(0).logical_state(),
            "existing snapshot owner resets the Scenario stream"
        );
        self.sim = restored;
    }
}

fn scene(golden: &Value, row: &Value) -> Scene {
    let case = &row["input"];
    let before = &row["before"];
    let rules = stock_rules(golden, case);
    assert_eq!(
        rules
            .object(row["unit"].as_str().unwrap())
            .unwrap()
            .locomotor,
        LocomotorKind::Drive,
        "the bounded native scene supplies a stopped Drive"
    );
    let mut sim = Simulation::new();
    sim.session.binary_frame = int(&before["frame"]) as u32;
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    // The shared native source fixture supplies Map+F4/F8=16 and final
    // LocalSize [-16,-16,64,64] over its 32x32 allocated CellClass grid.
    // Unit's production Unlimbo requires this mode-one playfield authority.
    let local = case["map_local_size"]
        .as_array()
        .map_or([-16, -16, 64, 64], |v| std::array::from_fn(|i| int(&v[i])));
    let bounds = crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
        16, local[0], local[1], local[2], local[3],
    );
    sim.playfield_bounds = Some(bounds);
    sim.playfield_size_height = Some(16);
    sim.session.map_width = 16;
    sim.session.map_height = 16;
    let mut terrain = test_flat_ground_grid(32);
    if let Some(level) = case["terrain_level"].as_u64() {
        for y in 0..32 {
            for x in 0..32 {
                terrain.cell_mut(x, y).unwrap().level = level as u8;
            }
        }
    }
    if let Some(cell) = row["raw_current_cell"].as_array() {
        terrain
            .cell_mut(int(&cell[0]) as u16, int(&cell[1]) as u16)
            .unwrap()
            .bridge_facts
            .raw_flags = row["raw_current_flags"].as_u64().unwrap() as u32;
    }
    let path = PathGrid::from_resolved_terrain(&terrain);
    sim.zone_grid = Some(
        crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
            &path,
            &terrain,
            &[],
            Some((16, 16)),
            Some(bounds),
        ),
    );
    sim.path_grid = Some(Arc::new(path));
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.terrain_costs = crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(
        sim.resolved_terrain.as_ref().unwrap(),
    );
    // Native scenes supply an existing human House and empty storage before
    // either Techno is constructed. Install that same membership prerequisite.
    let owner = sim.interner.intern("Russians");
    let mut house = crate::sim::house_state::HouseState::new(owner, 1, None, true, 0, 0);
    house.difficulty = crate::sim::house_state::HouseDifficulty::Hard;
    sim.houses.insert(owner, house);
    let depot = sim
        .spawn_object(
            row["building"].as_str().unwrap(),
            "Russians",
            6,
            9,
            0,
            &rules,
        )
        .expect("native depot type and supplied map admit the depot");
    let tank = sim
        .spawn_object(row["unit"].as_str().unwrap(), "Russians", 10, 10, 0, &rules)
        .expect("native unit type and supplied map admit the tank");
    let other = (before["unit_nav_aux"] == "other"
        || before["pending_entry"] == "other"
        || before["unit_contacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "other")
        || before["building_contacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "other"))
    .then(|| {
        sim.spawn_object(
            row["building"].as_str().unwrap(),
            "Russians",
            20,
            20,
            0,
            &rules,
        )
        .expect("native auxiliary target has an admitted supplied cell")
    });
    if case["spatial_startup"] == true {
        // The native waiter scene has admitted fields but only the depot
        // is CellPUT-marked. Remove constructor-added tank/placeholder lists
        // through Mark's existing owner before supplying their exact poses.
        sim.remove_entity_occupancy(tank);
        if let Some(other) = other {
            sim.remove_entity_occupancy(other);
            if before["building_contacts"][0] == "other" {
                sim.substrate
                    .entities
                    .get_mut(other)
                    .unwrap()
                    .mark_live_contact_with(depot);
            }
        }
    }
    for (id, current, queued, status) in [
        (tank, "unit_mission", "unit_queued", 0),
        (
            depot,
            "building_mission",
            "building_queued",
            int(&before["status"]),
        ),
    ] {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(int(&before[current])),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(int(&before[queued])),
            movement_bypass_latch: 0,
            handler_state: status as u32,
            mission_start_frame: int(&before["frame"]) as u32,
            ai_counter: 0,
            dispatch_timer: if id == tank && row["unit_dispatch_before"].is_array() {
                MissionDispatchTimer::from_raw(
                    int(&row["unit_dispatch_before"][0]),
                    int(&row["unit_dispatch_before"][1]),
                )
            } else {
                MissionDispatchTimer::from_raw(-1, 0)
            },
        });
    }
    for (id, peer, contacts, tether) in [
        (tank, depot, "unit_contacts", "unit_tether"),
        (depot, tank, "building_contacts", "building_tether"),
    ] {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.radio_contacts.clear_all();
        entity
            .radio_contacts
            .set_capacity(before[contacts].as_array().unwrap().len());
        for value in before[contacts].as_array().unwrap() {
            let target = match value.as_str() {
                Some("miner") => Some(tank),
                Some("refinery") => Some(depot),
                Some("other") => other,
                None => None,
                Some(value) => panic!("unrepresented contact {value}"),
            };
            if let Some(target) = target {
                entity.radio_contacts.insert(target);
            }
        }
        entity.dock_entered_with = (int(&before[tether]) != 0).then_some(peer);
    }
    {
        let unit = sim.substrate.entities.get_mut(tank).unwrap();
        if let Some(in_playfield) = case["unit_in_playfield"].as_bool() {
            assert_eq!(
                unit.in_playfield, in_playfield,
                "admitted native membership"
            );
        }
        if row["foot_before"].is_object() {
            let foot = &row["foot_before"];
            let path = &mut unit.navigation.path_runtime;
            path.movement_timer = CdTimer::from_raw(
                int(&foot["movement_timer"][0]),
                int(&foot["movement_timer"][1]),
            );
            path.blocked_timer = CdTimer::from_raw(
                int(&foot["blocked_timer"][0]),
                int(&foot["blocked_timer"][1]),
            );
            path.retries_left = foot["retries_left"].as_u64().unwrap() as u32;
            path.path_blocked = foot["path_blocked"].as_bool().unwrap();
            assert_eq!(unit.in_playfield, foot["in_playfield"].as_bool().unwrap());
            assert_eq!(
                unit.is_mission_only(),
                foot["mission_only"].as_bool().unwrap()
            );
            assert_eq!(
                unit.navigation.nav_queue.len(),
                int(&foot["nav_queue_count"]) as usize
            );
        }
        unit.health.current = int(&before["health"]);
        unit.estimated_health = EstimatedHealth::from_raw(int(&before["estimate"]));
        let at = &before["unit_coordinate"];
        unit.position.rx = (int(&at[0]) / 256) as u16;
        unit.position.ry = (int(&at[1]) / 256) as u16;
        unit.position.sub_x = SimFixed::from_num(int(&at[0]) % 256);
        unit.position.sub_y = SimFixed::from_num(int(&at[1]) % 256);
        unit.position.exact_z_leptons = Some(int(&at[2]));
        unit.navigation.nav_com = if let Some(cell) = before["unit_nav"].as_array() {
            Some(NavTargetRef::cell(
                int(&cell[0]) as u16,
                int(&cell[1]) as u16,
            ))
        } else {
            before["unit_nav"]
                .is_string()
                .then_some(NavTargetRef::Building { id: depot })
        };
        unit.navigation.nav_com_aux = (before["unit_nav_aux"] == "other")
            .then(|| NavTargetRef::Building { id: other.unwrap() });
        unit.set_pending_entry(match before["pending_entry"].as_str() {
            Some("other") => other,
            Some("refinery") => Some(depot),
            None => None,
            Some(value) => panic!("unrepresented pending entry {value}"),
        });
        let destination = &before["locomotor_destination"];
        let at = DriveCoord {
            x: int(&destination[0]),
            y: int(&destination[1]),
            z: int(&destination[2]),
        };
        {
            let loco = unit.locomotor.as_mut().unwrap();
            assert!(loco.ensure_installed_track_state());
            assert!(loco.store_track_destination(
                crate::sim::movement::track_process::TrackFamily::Drive,
                (at != DriveCoord { x: 0, y: 0, z: 0 }).then_some(at)
            ));
        };
        if int(&before["locomotor_powered"]) == 0 {
            unit.locomotor.as_mut().unwrap().power_off();
        } else {
            unit.locomotor.as_mut().unwrap().power_on();
        }
    }
    {
        let building = sim.substrate.entities.get_mut(depot).unwrap();
        building
            .mission_leaf
            .set_building_ready_latch(int(&before["repairing"]) as u8);
        let stage = &before["stage"];
        building
            .mission_leaf
            .install_building_repair_progress_fixture(StageClass::from_native_fixture(
                int(&stage[0]),
                int(&stage[1]) as u8,
                CdTimer::from_raw(int(&stage[2]), int(&stage[3])),
                int(&stage[4]),
                int(&stage[5]),
            ));
        if let Some(rally) = case["rally"].as_array() {
            building.set_archive_target(Some(TargetKind::Cell(
                int(&rally[0]) as u16,
                int(&rally[1]) as u16,
            )));
        }
    }
    let owner = sim.substrate.entities.get(tank).unwrap().owner();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.economy.credits = int(&before["balance"]);
    house.economy.spent_credits = int(&before["spent"]);
    sim.scenario_rng = if let Some(hex) = row["rng_before"]["scenario"].as_str() {
        SimRng::from_native_state_hex_for_test(hex)
    } else {
        SimRng::new(case["seed"].as_u64().unwrap_or(1))
    };
    Scene {
        sim,
        rules,
        tank,
        depot,
        other,
    }
}

#[test]
fn native_pending_entry_and_radio_lifecycle_use_shared_command_owners() {
    use crate::sim::command::Command;

    let golden = corpus();
    let rows = golden["pending_entry_lifecycle"].as_array().unwrap();
    assert_eq!(rows.len(), 30);
    let mut compared = 0;
    for row in rows {
        let case = &row["input"];
        let entry = case["entry"].as_str().unwrap();
        // These two native controls enter DEPLOY after its type admission.
        // Stock HTNK cannot enter that production command. Their evidence
        // establishes the independent Event9 suffix; it does not invent a
        // stock tank deployment producer for this depot chain.
        if entry == "deploy_event_calls" {
            continue;
        }
        let name = case["name"].as_str().unwrap();
        let mut scene = scene(&golden, row);
        {
            let unit = scene.sim.substrate.entities.get_mut(scene.tank).unwrap();
            unit.setter_force_reassign = case["force"] == true;
        }
        {
            let contact = scene.sim.substrate.entities.get_mut(scene.depot).unwrap();
            contact.lifecycle.object_alive = case["contact_alive"] != false;
            if let Some(health) = case["contact_health"].as_u64() {
                contact.health.current = health as i32;
            }
            if case["contact_is_unit"] == true {
                // Explicit native original-Unit-vtable RTTI prestate. The
                // shared gate must reject it before reading Building flags.
                contact.category = crate::map::entities::EntityCategory::Unit;
            }
        }
        if case["two_contacts"] == true {
            scene
                .sim
                .substrate
                .entities
                .get_mut(scene.other.unwrap())
                .unwrap()
                .mark_live_contact_with(scene.tank);
        }
        scene.compare(&row["before"], name);
        match entry {
            "unit_null" => {
                scene
                    .sim
                    .set_unit_null_destination(scene.tank, Some(&scene.rules), None);
            }
            "unit_cell" => {
                scene.sim.set_unit_destination(
                    scene.tank,
                    NavTargetRef::cell(13, 13),
                    &scene.rules,
                    true,
                );
            }
            "unit_idle" => {
                let _ = scene
                    .sim
                    .unit_enter_idle_mode(scene.tank, Some(&scene.rules), false);
            }
            "megamission_clear" => {
                // The native packet stops before mission translation/Queue.
                scene.sim.begin_megamission_retask(
                    scene.tank,
                    MissionType::Move,
                    Some(&scene.rules),
                );
            }
            "idle_event_calls"
            | "idle_event_foot_prefix"
            | "idle_event_after_actor_admission"
            | "idle_tether_gate"
            | "idle_mission_gate" => {
                assert!(
                    scene.sim.apply_command(
                        "Russians",
                        &Command::Stop {
                            entity_id: scene.tank
                        },
                        Some(&scene.rules)
                    ),
                    "{name}"
                );
            }
            other => panic!("unrepresented native command seam {other}"),
        }
        scene.compare(&row["after"], name);
        if case["two_contacts"] == true {
            let other = scene
                .sim
                .substrate
                .entities
                .get(scene.other.unwrap())
                .unwrap();
            let native_cleared = row["lifecycle_trace"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["field"] == "other_contact_slot0" && event["value"] == 0);
            assert_eq!(
                other.radio_contacts.is_empty(),
                native_cleared,
                "{name}: second contact reciprocal cleanup"
            );
        }
        compared += 1;
    }
    assert_eq!(compared, 28);
}

#[test]
fn native_retail_inputs_reach_production_rules_and_art() {
    let golden = corpus();
    let rules = stock_rules(&golden, &json!({}));
    let inputs = &golden["inputs"]["after"];
    assert_eq!(rules.general.repair_step, int(&inputs["repair_step"]));
    for (value, field) in [
        (rules.general.repair_percent, "repair_percent_bits"),
        (rules.general.unit_repair_rate, "unit_repair_rate_bits"),
        (rules.general.condition_yellow, "condition_yellow_bits"),
    ] {
        assert_eq!(
            value.to_bits(),
            little_double(inputs[field].as_str().unwrap()),
            "{field}"
        );
    }
    for (name, expected) in inputs["units"].as_object().unwrap() {
        let unit = rules.object(name).unwrap();
        assert_eq!(unit.strength, int(&expected["strength"]));
        assert_eq!(unit.cost, int(&expected["cost"]));
        assert_eq!(unit.manual_reload, expected["manual_reload"] == 1);
        assert_eq!(unit.harvester, expected["harvester"] == 1);
        assert_eq!(unit.weeder, expected["weeder"] == 1);
        assert_eq!(unit.movement_zone as i32, int(&expected["movement_zone"]));
    }
    for (name, expected) in inputs["buildings"].as_object().unwrap() {
        let building = rules.object(name).unwrap();
        assert_eq!(building.strength, int(&expected["strength"]));
        assert_eq!(building.cost, int(&expected["cost"]));
        assert_eq!(building.unit_repair, expected["unit_repair"] == 1);
        assert_eq!(
            building.has_stupid_guard_mode,
            expected["stupid_guard_mode"] == 1
        );
        assert_eq!(building.number_of_docks, int(&expected["number_of_docks"]));
        if let Some(offsets) =
            golden["inputs"]["art"]["docking_offsets"][name]["offsets"].as_array()
        {
            for (slot, expected) in offsets.iter().enumerate() {
                // ART installation omits an entirely unauthored zero array.
                // The dock-coordinate owner reads that representation as the
                // native constructor's zero offset (GADEPT), not a new pad.
                assert_eq!(
                    building
                        .pads
                        .get(slot)
                        .map_or((0, 0, 0), |pad| pad.lepton_offset),
                    (int(&expected[0]), int(&expected[1]), int(&expected[2])),
                    "{name}: installed docking offset"
                );
            }
        }
    }
    for row in golden["mission"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["name"] == "state1_paid")
    {
        assert_eq!(
            rules.mission_control.rate_frames(MissionType::Repair),
            int(&row["steps"][0]["returned_eax"])
        );
    }
}

#[test]
fn native_reader_defaults_exact_case_and_missing_key_retention() {
    let golden = corpus();
    for row in golden["input_controls"].as_array().unwrap() {
        let mut stack = RulesLayerStack::new(IniFile::from_str(&format!(
            "{REGISTRIES}{}",
            row["source"].as_str().unwrap(),
        )));
        for layer in &row["layers"].as_array().unwrap()[1..] {
            stack.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&sections_text(&layer["selected"])),
            );
        }
        let rules = RuleSet::from_rules_layers(&stack).unwrap();
        let expected = &row["after"];
        assert_eq!(
            rules.general.repair_step,
            int(&expected["repair_step"]),
            "{} step",
            row["name"]
        );
        for (value, field) in [
            (rules.general.repair_percent, "repair_percent_bits"),
            (rules.general.unit_repair_rate, "unit_repair_rate_bits"),
        ] {
            assert_eq!(
                value.to_bits(),
                little_double(expected[field].as_str().unwrap()),
                "{} {field}",
                row["name"]
            );
        }
        let unit = rules.object("HTNK").unwrap();
        assert_eq!(unit.cost, int(&expected["units"]["HTNK"]["cost"]));
        assert_eq!(unit.strength, int(&expected["units"]["HTNK"]["strength"]));
        assert_eq!(
            unit.manual_reload,
            expected["units"]["HTNK"]["manual_reload"] == 1
        );
        let depot = rules.object("NADEPT").unwrap();
        assert_eq!(
            depot.unit_repair,
            expected["buildings"]["NADEPT"]["unit_repair"] == 1
        );
        assert_eq!(
            depot.has_stupid_guard_mode,
            expected["buildings"]["NADEPT"]["stupid_guard_mode"] == 1
        );
    }
}

#[test]
fn native_unit_type_repair_getters_match_signed_controls() {
    let golden = corpus();
    for row in golden["unit_cost"].as_array().unwrap() {
        let rules = stock_rules(&golden, &row["input"]);
        let object = rules.object(row["unit"].as_str().unwrap()).unwrap();
        let steps = row["steps"].as_array().unwrap();
        assert_eq!(
            rules.type_cost(object) as u32,
            steps[0]["returned_eax"].as_u64().unwrap() as u32,
            "{} base cost",
            row["input"]["name"]
        );
        assert_eq!(
            crate::sim::production::repair_step_cost(&rules, object) as u32,
            steps[1]["returned_eax"].as_u64().unwrap() as u32,
            "{} repair cost",
            row["input"]["name"]
        );
        assert_eq!(
            rules.general.repair_step as u32,
            steps[2]["returned_eax"].as_u64().unwrap() as u32,
            "{} heal",
            row["input"]["name"]
        );
    }
}

#[test]
fn native_category_getter_keeps_unit_cost_free_of_building_deductions() {
    let golden = corpus();
    for row in golden["category_cost"].as_array().unwrap() {
        let case = &row["input"];
        let text = format!(
            "[VehicleTypes]\n0=FREE\n[BuildingTypes]\n0=TYPE\n\
             [AircraftTypes]\n0=PAD0\n1=PAD1\n\
             [General]\nRepairStep=8\nRepairPercent=15%\nPadAircraft=PAD0,PAD1\nSeparateAircraft={}\n\
             [TYPE]\nCost={}\nStrength={}\n{}\
             [FREE]\nCost={}\n[PAD0]\nCost=1000\nDock={}\n[PAD1]\nCost=1200\n",
            if case["separate_aircraft"] == false {
                "no"
            } else {
                "yes"
            },
            int(&case["cost"]),
            int(&case["strength"]),
            if case["free_unit"].is_number() {
                "FreeUnit=FREE\n"
            } else {
                ""
            },
            case["free_unit"].as_i64().unwrap_or(0),
            if case["pad_dock"] == true { "TYPE" } else { "" },
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
        let building = rules.object("TYPE").unwrap();
        assert_eq!(rules.type_cost(building), int(&row["building_cost"]));
        assert_eq!(
            crate::sim::production::repair_step_cost(&rules, building),
            int(&row["building_repair_cost"])
        );
        // Same supplied type fields, separately supplied actual category;
        // this is the native control's changed instance vtable prerequisite.
        let mut unit = building.clone();
        unit.category = crate::rules::object_type::ObjectCategory::Vehicle;
        assert_eq!(rules.type_cost(&unit), int(&row["unit_cost"]));
        assert_eq!(
            crate::sim::production::repair_step_cost(&rules, &unit),
            int(&row["unit_repair_cost"])
        );
    }
}

#[test]
fn native_unit_radio_repair_payment_heal_and_responses() {
    let golden = corpus();
    for row in golden["radio"].as_array().unwrap() {
        let mut scene = scene(&golden, row);
        scene.compare(&row["before"], row["input"]["name"].as_str().unwrap());
        let reply = radio::receive_radio(
            &mut scene.sim,
            scene.tank,
            Some(scene.depot),
            RadioMessage::RepairTick,
            RadioPayload::default(),
            Some(&scene.rules),
        );
        let step = &row["steps"][0];
        assert_eq!(
            u64::from(reply.code()),
            step["returned_eax"].as_u64().unwrap(),
            "{} response",
            row["input"]["name"]
        );
        scene.compare(&step["after"], row["input"]["name"].as_str().unwrap());
    }
}

#[test]
fn native_unit_destination_power_tail_uses_raw_cell_and_shared_setters() {
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellListInsertion;

    let golden = corpus();
    for row in golden["destination"].as_array().unwrap() {
        let mut scene = scene(&golden, row);
        let name = row["input"]["name"].as_str().unwrap();
        let flags = &row["setter_flags_before"];
        {
            let unit = scene.sim.substrate.entities.get_mut(scene.tank).unwrap();
            unit.setter_force_reassign = int(&flags["force_reassign"]) != 0;
            unit.mission_leaf
                .set_unit_deployed(int(&flags["deployed"]) as u8);
        }
        let cell = &row["raw_current_cell"];
        let (x, y) = (int(&cell[0]) as u16, int(&cell[1]) as u16);
        // The native controls explicitly supply the raw E4 list at the
        // current coordinate. No placement/Move Process producer is claimed.
        let existing = scene
            .sim
            .substrate
            .occupancy
            .get(x, y)
            .map(|cell| {
                cell.iter_layer(MovementLayer::Ground)
                    .map(|item| item.entity_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for id in existing {
            scene
                .sim
                .substrate
                .occupancy
                .remove_on_layer(x, y, id, MovementLayer::Ground);
        }
        for item in row["current_list"].as_array().unwrap() {
            let id = match item.as_str().unwrap() {
                "miner" => scene.tank,
                "refinery" => scene.depot,
                other => panic!("unrepresented current-list fixture {other}"),
            };
            let category = scene.sim.substrate.entities.get(id).unwrap().category;
            scene.sim.substrate.occupancy.add(
                x,
                y,
                id,
                MovementLayer::Ground,
                None,
                CellListInsertion::from_category(category),
            );
        }
        scene.compare(&row["before"], name);
        // Native741970 is a void setter; EAX is retained for reproducibility
        // but the Rust acceptance Boolean is not its native return contract.
        if row["input"]["null"] == true {
            scene
                .sim
                .set_unit_null_destination(scene.tank, Some(&scene.rules), None);
        } else {
            let destination = if row["input"]["target"] == "depot" {
                NavTargetRef::Building { id: scene.depot }
            } else {
                NavTargetRef::cell(13, 13)
            };
            scene.sim.set_unit_destination(
                scene.tank,
                destination,
                &scene.rules,
                row["input"]["flag"] != 0,
            );
        }
        scene.compare(&row["steps"][0]["after"], name);
        let unit = scene.sim.substrate.entities.get(scene.tank).unwrap();
        assert_eq!(
            u8::from(unit.setter_force_reassign),
            int(&row["setter_flags_after"]["force_reassign"]) as u8,
            "{name}: force-reassign byte"
        );
        assert_eq!(
            unit.mission_leaf.as_unit().unwrap().deployed(),
            int(&row["setter_flags_after"]["deployed"]) as u8,
            "{name}: deployment byte"
        );
    }
}

#[test]
fn native_guard_and_repair_controls_use_production_mission_dispatch() {
    let golden = corpus();
    for bucket in ["guard", "mission"] {
        for row in golden[bucket].as_array().unwrap() {
            let mut scene = scene(&golden, row);
            scene.visit_depot();
            scene.compare(
                &row["steps"][0]["after"],
                row["input"]["name"].as_str().unwrap(),
            );
        }
    }
}

#[test]
fn native_repair_power_fallbacks_use_the_original_power_owner() {
    let golden = corpus();
    let rows = golden["repair_fallback"].as_array().unwrap();
    let mut compared = 0;
    for row in rows.iter().filter(|row| row["input"]["linked"] != false) {
        let name = row["input"]["name"].as_str().unwrap();
        let mut scene = scene(&golden, row);
        scene.compare(&row["before"], name);
        scene.visit_depot();
        scene.compare(&row["steps"][0]["after"], name);
        compared += 1;
    }
    // Original MissionAI/Repair controls include both fallback calls at
    // 44C61B/44C808 and their admitted/near/powered boundary controls.
    assert_eq!(compared, 18);
}

#[test]
fn native_repair_no_contact_uses_the_original_readiness_gate() {
    let golden = corpus();
    let rows = golden["repair_fallback"].as_array().unwrap();
    let mut compared = 0;
    for row in rows.iter().filter(|row| row["input"]["linked"] == false) {
        let name = row["input"]["name"].as_str().unwrap();
        let mut scene = scene(&golden, row);
        scene.compare(&row["before"], name);
        scene.visit_depot();
        scene.compare(&row["steps"][0]["after"], name);
        compared += 1;
    }
    // State0 Queue44C6EA preserves +6DD; state1 explicitly writes it at
    // 44C18E. Each executes with both supplied ready-byte states.
    assert_eq!(compared, 4);
}

#[test]
fn native_ready_prerequisites_use_the_building_commence_owner() {
    let golden = corpus();
    for row in golden["prerequisites"].as_array().unwrap() {
        let mut scene = scene(&golden, row);
        let name = row["input"]["name"].as_str().unwrap();
        scene.compare(&row["before"], name);
        for step in row["steps"].as_array().unwrap() {
            match step["entry"].as_str().unwrap() {
                "0x5b3060" => scene.visit_depot(),
                "0x43ff91" => super::ready_commence(&mut scene.sim, scene.depot, false),
                "0x43c2d0" => {
                    let reply = radio::receive_radio(
                        &mut scene.sim,
                        scene.depot,
                        Some(scene.tank),
                        RadioMessage::DockNow,
                        RadioPayload::default(),
                        Some(&scene.rules),
                    );
                    assert_eq!(
                        u64::from(reply.code()),
                        step["returned_eax"].as_u64().unwrap(),
                        "{name}: DockNow response"
                    );
                }
                other => panic!("unrepresented readiness entry {other}"),
            }
            scene.compare(&step["after"], name);
        }
    }
}

#[test]
fn native_paid_retry_full_release_histories_run_mission_dispatch_every_frame() {
    let golden = corpus();
    for row in golden["histories"].as_array().unwrap() {
        let mut scene = scene(&golden, row);
        let mut steps = row["steps"].as_array().unwrap().iter().peekable();
        let name = row["input"]["name"].as_str().unwrap();
        if row["input"]["dock_now"] == true {
            let reply = radio::receive_radio(
                &mut scene.sim,
                scene.depot,
                Some(scene.tank),
                RadioMessage::DockNow,
                RadioPayload::default(),
                Some(&scene.rules),
            );
            let native = steps.next().unwrap();
            assert_eq!(
                u64::from(reply.code()),
                native["returned_eax"].as_u64().unwrap()
            );
            scene.compare(&native["after"], name);
        }
        for frame in int(&row["before"]["frame"]) as u32..=int(&row["input"]["to_frame"]) as u32 {
            scene.sim.session.binary_frame = frame;
            if let Some(step) = steps.peek()
                && step["entry"] == "0x65c6d0"
                && int(&step["after"]["frame"]) as u32 == frame
            {
                let native = steps.next().unwrap();
                scene.restore_snapshot_at_native_seed0_boundary();
                scene.compare(&native["after"], &format!("{name} restored frame{frame}"));
            }
            if let Some(step) = steps.peek()
                && step["entry"] == "0x4f9950"
                && int(&step["after"]["frame"]) as u32 == frame
            {
                let native = steps.next().unwrap();
                let owner = scene
                    .sim
                    .substrate
                    .entities
                    .get(scene.tank)
                    .unwrap()
                    .owner();
                crate::sim::credit_income::add_credits(
                    &mut scene.sim,
                    owner,
                    int(&row["input"]["deposit"]),
                );
                scene.compare(&native["after"], name);
            }
            scene.visit_depot();
            if let Some(step) = steps.peek()
                && int(&step["after"]["frame"]) as u32 == frame
            {
                let native = steps.next().unwrap();
                assert_eq!(native["entry"], "0x5b3060");
                scene.compare(&native["after"], &format!("{name} frame{frame}"));
            }
        }
        assert!(
            steps.next().is_none(),
            "{name}: every native retained step consumed"
        );
    }
}

#[test]
fn independent_repair_progress_contributes_to_state_hash() {
    let golden = corpus();
    let row = golden["histories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "paid_complete")
        .unwrap();
    let mut scene = scene(&golden, row);
    for frame in 200..=279 {
        scene.sim.session.binary_frame = frame;
        scene.visit_depot();
    }
    let progress = *scene
        .sim
        .substrate
        .entities
        .get(scene.depot)
        .unwrap()
        .mission_leaf
        .as_building()
        .unwrap()
        .repair_progress();
    let raw = serde_json::to_value(progress).unwrap();
    let original_hash = scene.sim.state_hash();
    scene
        .sim
        .substrate
        .entities
        .get_mut(scene.depot)
        .unwrap()
        .mission_leaf
        .install_building_repair_progress_fixture(StageClass::from_native_fixture(
            progress.value().wrapping_add(1),
            int(&raw["changed"]) as u8,
            progress.timer(),
            progress.rate(),
            int(&raw["increment"]),
        ));
    assert_ne!(
        scene.sim.state_hash(),
        original_hash,
        "only repair progress changes this future repair boundary"
    );
    scene
        .sim
        .substrate
        .entities
        .get_mut(scene.depot)
        .unwrap()
        .mission_leaf
        .install_building_repair_progress_fixture(progress);
    assert_eq!(
        scene.sim.state_hash(),
        original_hash,
        "restoring the sole changed field restores the hash"
    );
}
