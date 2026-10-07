//! Original ready MTNK shot through the live object loop and fatal cleanup.
//!
//! building_death_anims --joined executes an actual MTNK shot and a repeated
//! direct AP control. The shot comparison executes FireAt, Bullet and the
//! dynamic Building/Anim/crew loop from the supplied ready-shooter prior.
//! The separate null-source receiver control checks successive damage and
//! power sampling. Acquisition and previous volleys remain outside coverage.

use super::{Simulation, entry_test_fixture};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind};
use crate::sim::components::DriveCoord;
use crate::sim::house_state::{HouseState, MatchStatistics};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::ground_pose;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::power_system::{self, PowerState};
use crate::sim::rng::SimRng;
use serde_json::{Value, json};

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native scalar")).unwrap()
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    }
}

fn append_sections(text: &mut String, sections: &Value) {
    for (section, keys) in sections.as_object().expect("retained physical sections") {
        let Some(keys) = keys.as_object() else {
            continue;
        };
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in keys {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
}

fn assert_rng(sim: &Simulation, native: &Value, boundary: &str) {
    for (name, rng) in [
        ("scenario", &sim.scenario_rng),
        ("main", &sim.main_rng),
        ("mapgen", &sim.mapgen_rng),
    ] {
        assert!(
            rng.native_state_hex() == native[name].as_str().unwrap(),
            "{boundary}: full {name} state"
        );
    }
}

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    registry: OverlayTypeRegistry,
    building: u64,
    unit: u64,
    houses: [InternedId; 2],
    receiver_birth_floor: i32,
}

impl Fixture {
    fn new(corpus: &Value, prior: &Value) -> Self {
        let corpus = if prior.get("attacker").is_none() {
            corpus
                .get("direct_receiver_input_context")
                .unwrap_or(corpus)
        } else {
            corpus
        };
        // Catalog indices are fixture transport. Every behavior key comes
        // from the retained physical native inputs, in scenario layer order.
        let mut text = String::from(
            "[InfantryTypes]\n2=E1\n[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAPOWR\n[Warheads]\n0=AP\n1=Super\n2=SA\n3=HE\n",
        );
        let mut input = IniFile::from_str(&text);
        for layer in corpus["death_layers"].as_array().unwrap() {
            if layer["absent"] == true {
                continue;
            }
            let mut layer_text = String::new();
            append_sections(&mut layer_text, &layer["physical_input_sections"]);
            if let Some(sections) = layer.get("declared_e1_weapon_sections") {
                append_sections(&mut layer_text, sections);
            }
            input.merge(&IniFile::from_str(&layer_text));
        }
        if let Some(stock) = corpus.get("native_stock_smudge_inputs") {
            for layer in stock["layers"].as_array().unwrap() {
                if layer["absent"] == true {
                    continue;
                }
                let mut sections = layer["physical_input_sections"].clone();
                sections.as_object_mut().unwrap().remove("SmudgeTypes");
                let mut layer_text = String::new();
                append_sections(&mut layer_text, &sections);
                // Registry order is physical declaration order, not JSON's
                // lexical key order (1, 10, 11, ...).
                let entries = layer["physical_smudge_registry_entries"]
                    .as_array()
                    .unwrap();
                if !entries.is_empty() {
                    layer_text.push_str("[SmudgeTypes]\n");
                    for entry in entries {
                        layer_text.push_str(&format!(
                            "{}={}\n",
                            entry[0].as_str().unwrap(),
                            entry[1].as_str().unwrap()
                        ));
                    }
                }
                input.merge(&IniFile::from_str(&layer_text));
            }
        }
        let mut art_text = String::new();
        append_sections(&mut art_text, &corpus["physical_art_input_sections"]);
        if let Some(stock) = corpus.get("native_stock_smudge_inputs") {
            append_sections(&mut art_text, &stock["physical_art_input_sections"]);
        }
        let mut anim_names: Vec<&str> = corpus["native_fatal_anim_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|anim| anim["name"].as_str().unwrap())
            .collect();
        // The inherited native reader also registers ART-less gtpowexp.
        // Omitting it would shorten GAPOWR's six-entry Explosion list.
        anim_names.extend(["GAPOWR_A", "GAPOWR_AD", "gtpowexp"]);
        let mut catalog = String::from("[Animations]\n");
        for (index, name) in anim_names.iter().enumerate() {
            catalog.push_str(&format!("{index}={name}\n"));
        }
        input.merge(&IniFile::from_str(&catalog));
        // The shared fixture accepts text. Serialize the production INI
        // overlay once; concatenating duplicate sections would retain the
        // first physical occurrence instead of applying scenario layers.
        text.clear();
        for name in input.section_names() {
            text.push_str(&format!("[{name}]\n"));
            for (key, value) in input.section(name).unwrap().raw_entries() {
                text.push_str(&format!("{key}={value}\n"));
            }
        }
        let art = IniFile::from_str(&art_text);
        let (mut sim, mut rules, registry) =
            entry_test_fixture::fixture_with_rules_and_fixed_art(&text, &art);
        // The original joined fixture explicitly writes GameOptions+A8EB60
        // to 3 before any Anim ctor (building_construction::JoinedFixture).
        // Use the same supplied prior through the existing cadence owner.
        sim.session.game_options.game_speed = prior.get("game_speed").map_or(3, int);
        // Original DestructionEffects441819 rolls the center mark even
        // with no loaded SmudgeType candidates. Keep the admitted map
        // owner's empty cells; None is the mapless dispatch boundary.
        // The shared fixture layers retail SmudgeTypes; this joined oracle
        // deliberately supplies the original empty registry constructor.
        if let Some(stock) = corpus.get("native_stock_smudge_inputs") {
            rules.smudge_types =
                crate::rules::smudge_type::SmudgeTypeRegistry::from_rules_ini(&input);
            let expected = stock["layers"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find_map(|layer| layer["registry"].as_array())
                .unwrap();
            assert_eq!(rules.smudge_types.len(), expected.len());
            for ((index, def), native) in rules.smudge_types.iter_with_id().zip(expected) {
                assert_eq!(i32::from(index), int(&native["index"]));
                assert_eq!(def.name, native["name"]);
                assert_eq!(def.burn, native["burn"] == 1);
                assert_eq!(def.crater, native["crater"] == 1);
                assert_eq!(i32::from(def.width), int(&native["width"]));
                assert_eq!(i32::from(def.height), int(&native["height"]));
            }
            let mut theater_text = String::new();
            append_sections(
                &mut theater_text,
                &corpus["native_stock_theater_inputs"]["physical_input_sections"],
            );
            let lookup =
                crate::map::theater::parse_tileset_ini(theater_text.as_bytes(), "tem").unwrap();
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            for y in 0..terrain.height() {
                for x in 0..terrain.width() {
                    let cell = terrain.cell_mut(x, y).unwrap();
                    cell.final_tile_index = 0;
                    cell.accepts_smudge =
                        crate::map::resolved_terrain::current_tile_permissions(&lookup, 0).0;
                }
            }
            terrain.test_set_dummy_accepts_smudge(
                crate::map::resolved_terrain::current_tile_permissions(&lookup, 0xFFFF).0,
            );
        } else {
            rules.smudge_types = crate::rules::smudge_type::SmudgeTypeRegistry::default();
            assert_eq!(rules.smudge_types.iter_with_id().count(), 0);
        }
        sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(33, 33));
        let mut sound_text = String::from("[SoundList]\n");
        for sound in corpus["selected_sounds"].as_array().unwrap() {
            sound_text.push_str(&format!(
                "{}={}\n",
                int(&sound["fixture_index"]),
                sound["name"].as_str().unwrap()
            ));
        }
        rules.bind_type_sound_references(
            &IniFile::from_str(&text),
            &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(&sound_text)),
        );
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        for anim in corpus["native_fatal_anim_inputs"].as_array().unwrap() {
            rules.bind_anim_frame_count_for_test(
                anim["name"].as_str().unwrap(),
                int(&anim["raw_shp_frame_count"]),
            );
        }
        // The inherited original reader packet holds the same physical
        // Building ART/SHP inputs; no bounds are calculated from Rust.
        let inherited: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_repair_joined.json",
        ))
        .unwrap();
        for anim in inherited["native_anim_inputs"].as_array().unwrap() {
            let name = anim["name"].as_str().unwrap();
            if ["GAPOWR_A", "GAPOWR_AD", "gtpowexp"].contains(&name) {
                let raw_count = int(&anim["raw_shp_frame_count"]);
                if raw_count > 0 {
                    rules.bind_anim_frame_count_for_test(name, raw_count);
                } else {
                    // Registered gtpowexp never reached ReadINI/image loading.
                    // Its original constructor End0 survives asset binding.
                    let config = rules.art().anim_runtime_config(name).unwrap();
                    assert!(!config.art_body_read);
                    assert_eq!(config.end, 0);
                }
            }
        }
        let houses = [sim.intern("Americans"), sim.intern("Second")];
        for house in houses {
            sim.houses
                .insert(house, HouseState::new(house, 0, None, true, 5000, 10));
            sim.session.house_order.push(house);
        }
        sim.session.current_house = Some(houses[0]);
        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: 16,
            off_fc: 0,
            off_100: 0,
            off_104: 32,
            off_108: 32,
        });
        assert!(sim.rebuild_dynamic_navigation(&rules));
        // Both native routes retain the real full-constructor ID1.
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            int(&prior["id"]).wrapping_sub(1) as u32,
        ));
        // Reuse the held-object constructor/reveal boundary, then restore
        // damage before the admitted opening. Map import would open first
        // at full health, producing an extra normal ART birth.
        let building = sim
            .construct_object_limbo_at_height("GAPOWR", "Second", 10, 10, 0, 0, &rules)
            .unwrap();
        sim.reveal_constructed_object_at_height(
            building,
            10,
            10,
            0,
            0,
            super::PlacementEvidence::MarkSucceeded,
            &rules,
        )
        .unwrap();
        if prior.get("attacker").is_none() {
            // The historical direct isolation supplies foundation lists
            // without normal native Mark admission, leaving raw+124 zero.
            // Supply that declared input only here. Main retains real Mark.
            let entity = sim.substrate.entities.get(building).unwrap();
            for (rx, ry) in crate::sim::crew_survival::foundation_cells(
                entity.position.rx,
                entity.position.ry,
                &entity.foundation,
            ) {
                sim.substrate.raw_cell_occupation.clear_ground(
                    rx,
                    ry,
                    crate::sim::occupancy::BUILDING_OCCUPATION_BIT,
                );
            }
        }
        {
            let entity = sim.substrate.entities.get_mut(building).unwrap();
            entity.finish_building_construction_for_test();
            // Native supplied BState1 receives BeginMode(1) before opening;
            // the equal queued request survives until its first AI visit.
            entity
                .begin_building_body(crate::sim::building_construction::BuildingBodyMode::Idle, 0);
            entity
                .mission_leaf
                .set_building_ready_latch(int(&prior["building"]["building"]["done"]) as u8);
            entity.health.current = int(&prior["health"]);
            entity.estimated_health.reset(int(&prior["health"]));
            entity.sample_building_health_for_power();
        }
        assert_eq!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .native_unique_id,
            int(&prior["id"])
        );
        let slot_pointer = prior["building"]["slots"][3].as_u64().unwrap();
        let prior_anim = prior["building"]["anims"]
            .as_array()
            .unwrap()
            .iter()
            .find(|anim| anim["pointer"].as_u64() == Some(slot_pointer))
            .unwrap();
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            int(&prior_anim["native_id"]).wrapping_sub(1) as u32,
        ));
        sim.set_building_damage_state(building, true, &rules);
        sim.grand_opening(building, false, false, &rules, Some(&registry));
        assert!(
            sim.substrate
                .entities
                .get(building)
                .unwrap()
                .building_anim_slots[3]
                .is_some()
        );
        let unit = sim
            .spawn_object("MTNK", "Americans", 11, 8, 128, &rules)
            .unwrap();
        if let Some(attacker) = prior.get("attacker") {
            sim.assign_target_represented(unit, Some(TargetKind::Entity(building)), Some(&rules))
                .unwrap();
            let actor = sim.substrate.entities.get_mut(unit).unwrap();
            actor.body_facing.snap(
                int(&attacker["body_facing_words"][0]) as u16,
                int(&prior["frame"]) as u32,
            );
            actor.barrel_facing.as_mut().unwrap().snap(
                int(&attacker["turret_facing_words"][0]) as u16,
                int(&prior["frame"]) as u32,
            );
            actor
                .mission
                .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                    current: MissionId::from_raw(int(&attacker["current_mission"])),
                    suspended: MissionId::NONE,
                    queued: MissionId::from_raw(int(&attacker["queued_mission"])),
                    movement_bypass_latch: 0,
                    handler_state: attacker["mission_status"].as_u64().unwrap() as u32,
                    mission_start_frame: 0,
                    ai_counter: attacker["mission_visit_count"].as_u64().unwrap() as u32,
                    dispatch_timer: MissionDispatchTimer::from_raw(
                        int(&attacker["mission_timer"][0]),
                        int(&attacker["mission_timer"][2]),
                    ),
                });
            actor.rearm_timer = crate::sim::timer::CdTimer::from_raw(
                int(&attacker["rearm_timer"][0]),
                int(&attacker["rearm_timer"][2]),
            );
            actor.last_fire_frame = i64::from(int(&attacker["last_fire_frame"]));
        }
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(int(
            &prior["unique_id_cursor"],
        )
            as u32));
        sim.session.binary_frame = int(&prior["frame"]) as u32;
        for (house, native) in houses.iter().zip(prior["houses"].as_array().unwrap()) {
            sim.houses.get_mut(house).unwrap().stats =
                MatchStatistics::from_totals_for_test(0, 0, 0, 0, 0, 0);
            let mut power = serde_json::to_value(PowerState::default()).unwrap();
            power["total_output"] = native["power"].clone();
            power["total_drain"] = native["drain"].clone();
            power["power_dirty"] = json!(native["dirty"][0] == 1);
            power["radar_dirty"] = json!(native["dirty"][1] == 1);
            sim.power_states
                .insert(*house, serde_json::from_value(power).unwrap());
        }
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(prior["rng"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(prior["rng"]["main"].as_str().unwrap());
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(prior["rng"]["mapgen"].as_str().unwrap());
        Self {
            sim,
            rules,
            registry,
            building,
            unit,
            houses,
            receiver_birth_floor: int(&prior["unique_id_cursor"]).wrapping_add(1),
        }
    }

    fn hit(&mut self) {
        let wh = self.sim.intern("AP");
        let damage = self.rules.weapon("105mm").unwrap().damage;
        let hit = EntityDamageEvent::direct_receiver(
            self.building,
            damage,
            0,
            RAD_NO_ATTACKER,
            None,
            wh,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        self.sim
            .commit_noncombat_aoe_hits(&self.rules, Some(&self.registry), &[hit]);
    }

    fn power_prefix(&mut self) {
        for house in self.houses {
            power_system::assess_house_power(
                self.sim.power_states.get_mut(&house).unwrap(),
                &self.sim.substrate.entities,
                &self.rules,
                house,
                &self.sim.interner,
                self.sim.session.binary_frame,
            );
        }
    }

    fn assert_live_frame(&self, route: &Value, native: &Value, boundary: &str) {
        self.assert_state(native, boundary);
        let building = self.sim.substrate.entities.get(self.building).unwrap();
        let stage = building.native_stage();
        assert_eq!(
            json!({
                "bstate": building.building_body_state().unwrap(),
                "done": building.building_ready_latch(),
                "mission": building.mission.current().raw(),
                "queued": building.mission.queued().raw(),
                "rate": stage.rate(),
                "stage": stage.value(),
                "status": building.mission.handler_state(),
                "timer": [stage.timer().start_frame(), stage.timer().duration()],
            }),
            native["building"]["building"],
            "{boundary}: Building body and mission",
        );
        assert_eq!(
            building.queued_building_body_state().unwrap(),
            int(&native["building"]["queued_bstate"]),
            "{boundary}: queued Building body",
        );
        assert_eq!(
            building.building_last_operational,
            native["building"]["old_operational"] == 1,
            "{boundary}: retained operational state",
        );
        assert_eq!(
            building.building_actually_placed,
            native["building"]["placed"] == 1,
            "{boundary}: first-opening state",
        );
        let dispatch = building.mission.dispatch_timer();
        assert_eq!(
            [dispatch.start_frame(), dispatch.delay()],
            [
                int(&native["building"]["mission_timer"][0]),
                int(&native["building"]["mission_timer"][2])
            ],
            "{boundary}: Building dispatch timer",
        );
        assert_eq!(
            building.damage_fire_state_active,
            native["damage_fire_active_5e8"] == 1,
            "{boundary}: damage-fire state"
        );

        self.assert_graph(route, native, boundary);
        self.assert_attacker(route, native, boundary);
        self.assert_occupation(native, boundary);
    }

    fn assert_graph(&self, route: &Value, native: &Value, boundary: &str) {
        // Native pointers and Rust stable IDs are transport identities. Map
        // both to the real constructor-assigned shared native IDs.
        let mut pointers = std::collections::BTreeMap::from([
            (
                route["before"]["attacker"]["target"].as_u64().unwrap(),
                int(&native["id"]),
            ),
            (
                route["before"]["attacker"]["pointer"].as_u64().unwrap(),
                int(&native["attacker"]["id"]),
            ),
            (
                route["shot"]["bullet"].as_u64().unwrap(),
                int(&route["shot"]["bullet_id"]),
            ),
        ]);
        for actor in native["building"]["anims"].as_array().unwrap() {
            pointers.insert(actor["pointer"].as_u64().unwrap(), int(&actor["native_id"]));
        }
        for actor in native["crew"].as_array().unwrap() {
            pointers.insert(actor["pointer"].as_u64().unwrap(), int(&actor["id"]));
        }
        if let Some(stock) = route.get("stock_runtime") {
            for call in stock["calls"].as_array().unwrap() {
                if call["kind"] == "smudge_constructor" {
                    let actor = &call["constructed_object"];
                    pointers.insert(actor["pointer"].as_u64().unwrap(), int(&actor["id"]));
                }
            }
        }
        let native_id = |id| self.native_id(id);
        let expected_logic: Vec<_> = native["building"]["logic"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pointer| pointers[&pointer.as_u64().unwrap()])
            .collect();
        let actual_logic: Vec<_> = self
            .sim
            .logic_order()
            .iter()
            .copied()
            .map(native_id)
            .collect();
        assert_eq!(
            actual_logic, expected_logic,
            "{boundary}: dynamic Logic order"
        );
        let expected_pending: Vec<_> = native["pending"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pointer| pointers[&pointer.as_u64().unwrap()])
            .collect();
        let actual_pending: Vec<_> = self
            .sim
            .substrate
            .pending_delete
            .iter()
            .copied()
            .map(native_id)
            .collect();
        assert_eq!(
            actual_pending, expected_pending,
            "{boundary}: deferred retirement order"
        );
        if let Some(building) = self.sim.substrate.entities.get(self.building) {
            for (slots, key) in [
                (&building.building_anim_slots[..], "slots"),
                (&building.damage_fire_anim_ids[..], "damage_fire_slots"),
            ] {
                let expected = if key == "slots" {
                    &native["building"][key]
                } else {
                    &native[key]
                };
                let expected: Vec<_> = expected
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pointer| {
                        let pointer = pointer.as_u64().unwrap();
                        (pointer != 0).then(|| pointers[&pointer])
                    })
                    .collect();
                let actual: Vec<_> = slots.iter().map(|id| id.map(native_id)).collect();
                assert_eq!(actual, expected, "{boundary}: {key}");
            }
        }
        assert_eq!(
            self.sim.substrate.anims.iter().count(),
            native["building"]["anims"].as_array().unwrap().len(),
            "{boundary}: retained Anim count"
        );
        for anim in native["building"]["anims"].as_array().unwrap() {
            let actual = self
                .sim
                .substrate
                .anims
                .iter()
                .map(|(_, a)| a)
                .find(|a| a.native_unique_id == int(&anim["native_id"]))
                .unwrap();
            let mut runtime = anim["runtime"].clone();
            for key in [
                "first_ai_guard",
                "constructor_reverse",
                "inactive",
                "paused",
            ] {
                runtime[key] = json!(int(&runtime[key]) != 0);
            }
            runtime["frame_timer"] = json!({"start_frame": int(&runtime["frame_timer"][0]),
                "duration": int(&runtime["frame_timer"][1])});
            assert_eq!(
                serde_json::to_value(&actual.runtime).unwrap(),
                runtime,
                "{boundary}: Anim{} runtime",
                actual.native_unique_id
            );
            let owner = anim["owner_object"].as_u64().unwrap();
            let expected_owner = (owner != 0).then(|| pointers[&owner]);
            assert_eq!(
                actual.owner_entity.map(native_id),
                expected_owner,
                "{boundary}: Anim{} owner",
                actual.native_unique_id
            );
            assert_eq!(
                actual.z_adjust,
                int(&anim["z_adjust"]),
                "{boundary}: Anim{} depth",
                actual.native_unique_id
            );
        }
    }

    fn assert_attacker(&self, route: &Value, native: &Value, boundary: &str) {
        let attacker = self.sim.substrate.entities.get(self.unit).unwrap();
        let expected = &native["attacker"];
        assert_eq!(
            [
                attacker.mission.current().raw(),
                attacker.mission.queued().raw(),
                attacker.mission.handler_state() as i32,
                attacker.mission.ai_counter() as i32
            ],
            [
                int(&expected["current_mission"]),
                int(&expected["queued_mission"]),
                int(&expected["mission_status"]),
                int(&expected["mission_visit_count"])
            ],
            "{boundary}: firer mission and visit count",
        );
        let dispatch = attacker.mission.dispatch_timer();
        assert_eq!(
            [dispatch.start_frame(), dispatch.delay()],
            [
                int(&expected["mission_timer"][0]),
                int(&expected["mission_timer"][2])
            ],
            "{boundary}: firer dispatch timer",
        );
        let target = expected["target"].as_u64().unwrap();
        if target != 0 {
            assert_eq!(
                target,
                route["before"]["attacker"]["target"].as_u64().unwrap()
            );
        }
        assert_eq!(
            attacker.attack_target.as_ref().map(|target| target.target),
            (target != 0).then_some(TargetKind::Entity(self.building)),
            "{boundary}: firer target expiry",
        );
        assert_eq!(
            [
                attacker.passive_scan_timer.start_frame as i32,
                attacker.passive_scan_timer.duration as i32
            ],
            [
                int(&expected["passive_scan_timer"][0]),
                int(&expected["passive_scan_timer"][2])
            ],
            "{boundary}: firer passive targeting timer",
        );
        assert_eq!(
            attacker.body_facing.destination(),
            int(&expected["body_facing_words"][0]) as u16,
            "{boundary}: body facing"
        );
        assert_eq!(
            attacker.barrel_facing.unwrap().destination(),
            int(&expected["turret_facing_words"][0]) as u16,
            "{boundary}: turret facing"
        );
        assert_eq!(
            [
                attacker.rearm_timer.start_frame(),
                attacker.rearm_timer.duration()
            ],
            [
                int(&expected["rearm_timer"][0]),
                int(&expected["rearm_timer"][2])
            ],
            "{boundary}: rearm timer"
        );
        assert_eq!(
            attacker.last_fire_frame,
            i64::from(int(&expected["last_fire_frame"])),
            "{boundary}: firing timestamp"
        );
        assert_eq!(
            attacker.weapon_burst.index(),
            int(&expected["burst_index"]),
            "{boundary}: retained burst"
        );
    }

    fn assert_occupation(&self, native: &Value, boundary: &str) {
        for (cell, expected) in native["raw_foot_occupation"].as_object().unwrap() {
            let (x, y) = cell.split_once(',').unwrap();
            let (x, y) = (x.parse::<u16>().unwrap(), y.parse::<u16>().unwrap());
            assert_eq!(
                i32::from(self.sim.substrate.raw_cell_occupation.ground_bits(x, y)),
                int(&expected["ground"]),
                "{boundary}: {cell} ground occupation"
            );
            assert_eq!(
                i32::from(self.sim.substrate.raw_cell_occupation.deck_bits(x, y)),
                int(&expected["deck"]),
                "{boundary}: {cell} deck occupation"
            );
        }
    }

    fn assert_state(&self, native: &Value, boundary: &str) {
        let entity = self.sim.substrate.entities.get(self.building).unwrap();
        assert_eq!(
            entity.health.current,
            int(&native["health"]),
            "{boundary}: actual health"
        );
        assert_eq!(
            entity.building_power_health_sample(),
            Some(int(&native["sampled_health"])),
            "{boundary}: sampled health"
        );
        assert_eq!(
            entity.lifecycle.object_alive,
            native["alive"] == 1,
            "{boundary}: alive"
        );
        assert_eq!(
            entity.lifecycle.in_limbo,
            native["limbo"] == 1,
            "{boundary}: limbo"
        );
        assert_eq!(
            entity.lifecycle.cell_marked,
            native["marked"] == 1,
            "{boundary}: cell mark"
        );
        assert_eq!(
            self.sim.substrate.pending_delete.contains(&self.building),
            !native["pending"].as_array().unwrap().is_empty(),
            "{boundary}: pending retirement"
        );
        assert!(
            !self.sim.object_placement_scope_active(),
            "{boundary}: escape bracket cleared"
        );
        assert_eq!(
            self.sim.native_unique_ids.as_ref().unwrap().current_raw(),
            int(&native["unique_id_cursor"]) as u32,
            "{boundary}: shared identity cursor"
        );
        let expected = native["building"]["anims"].as_array().unwrap();
        // Earlier muzzle/Bullet are supplied native priors outside this
        // receiver comparison. Every constructor from the receiver is real.
        let floor = self.receiver_birth_floor;
        let actual: Vec<_> = self
            .sim
            .substrate
            .anims
            .iter()
            .map(|(_, a)| a)
            .filter(|a| a.native_unique_id >= floor)
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .filter(|a| int(&a["native_id"]) >= floor)
            .collect();
        assert_eq!(
            actual.len(),
            expected.len(),
            "{boundary}: frame{} receiver Anims; actual{:?}, native{:?}",
            int(&native["frame"]),
            actual
                .iter()
                .map(|anim| (
                    anim.native_unique_id,
                    self.sim.interner.resolve(anim.type_id)
                ))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|anim| (int(&anim["native_id"]), anim["type_name"].as_str().unwrap()))
                .collect::<Vec<_>>(),
        );
        for (actual, native) in actual.iter().zip(expected) {
            assert_eq!(
                actual.native_unique_id,
                int(&native["native_id"]),
                "{boundary}: Anim ID"
            );
            assert_eq!(
                self.sim.interner.resolve(actual.type_id),
                native["type_name"].as_str().unwrap(),
                "{boundary}: Anim type"
            );
            assert_eq!(
                [
                    actual.world_coord.x,
                    actual.world_coord.y,
                    actual.world_coord.z
                ],
                [
                    int(&native["location"][0]),
                    int(&native["location"][1]),
                    int(&native["location"][2])
                ],
                "{boundary}: Anim coordinate"
            );
            assert_eq!(
                i32::from(actual.runtime.delay_remaining),
                int(&native["runtime"]["delay_remaining"]),
                "{boundary}: Anim delay"
            );
            assert_eq!(
                actual.draw_flags,
                int(&native["draw_flags"]) as u32,
                "{boundary}: Anim flags"
            );
            if let Some(bounce) = &actual.bounce {
                assert_eq!(
                    bounce.position.map(|v| v.bits()),
                    std::array::from_fn(
                        |i| native["bounce"]["position_bits"][i].as_u64().unwrap() as u32
                    ),
                    "{boundary}: Bounce position"
                );
                assert_eq!(
                    bounce.velocity.map(|v| v.bits()),
                    std::array::from_fn(
                        |i| native["bounce"]["velocity_bits"][i].as_u64().unwrap() as u32
                    ),
                    "{boundary}: Bounce velocity"
                );
            }
        }
        assert_rng(&self.sim, &native["rng"], boundary);
        let crew: Vec<_> = self
            .sim
            .substrate
            .entities
            .values()
            .filter(|e| e.category == EntityCategory::Infantry)
            .collect();
        assert_eq!(
            crew.len(),
            native["crew"].as_array().unwrap().len(),
            "{boundary}: crew count"
        );
        for (actual, native) in crew.iter().zip(native["crew"].as_array().unwrap()) {
            assert_eq!(
                actual.native_unique_id,
                int(&native["id"]),
                "{boundary}: crew constructor order"
            );
            assert_eq!(
                actual.health.current,
                int(&native["health"]),
                "{boundary}: crew health"
            );
            assert_eq!(
                ground_pose::position_world_coord(&actual.position),
                coord(&native["position"]),
                "{boundary}: crew physical location"
            );
            assert_eq!(
                actual.mission.current().raw(),
                int(&native["current_mission"]),
                "{boundary}: crew current mission"
            );
            assert_eq!(
                actual.mission.queued().raw(),
                int(&native["queued_mission"]),
                "{boundary}: crew queued mission"
            );
            let dispatch = actual.mission.dispatch_timer();
            assert_eq!(
                [dispatch.start_frame(), dispatch.delay()],
                [
                    int(&native["mission_timer"][0]),
                    int(&native["mission_timer"][2])
                ],
                "{boundary}: crew dispatch timer",
            );
            assert_eq!(
                actual.mission_leaf.as_infantry().unwrap().doing(),
                int(&native["doing"]),
                "{boundary}: crew Doing"
            );
            assert_eq!(
                actual.locomotor.as_ref().unwrap().step_head(),
                Some(coord(&native["locomotor"]["head"])),
                "{boundary}: immediate Walk head"
            );
            let native_path: Vec<u8> = native["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|word| int(word) as u8)
                .take_while(|word| *word != u8::MAX)
                .collect();
            assert_eq!(
                actual.navigation.path_replay.remaining_directions(),
                native_path,
                "{boundary}: retained native path head"
            );
            let logic = self.sim.logic_order();
            let crew_slot = logic
                .iter()
                .position(|id| *id == actual.stable_id())
                .unwrap();
            assert!(
                self.sim
                    .substrate
                    .anims
                    .iter()
                    .filter(|(_, a)| a.native_unique_id >= floor
                        && a.native_unique_id < actual.native_unique_id
                        && a.in_logic_vector)
                    .all(|(id, _)| logic.iter().position(|live| live == id).unwrap() < crew_slot),
                "{boundary}: death Anims admitted before crew"
            );
        }
        self.assert_houses(native, boundary);
    }

    fn assert_houses(&self, native: &Value, boundary: &str) {
        let type_ref = self.sim.interner.get("GAPOWR").unwrap();
        for (owner, native) in self.houses.iter().zip(native["houses"].as_array().unwrap()) {
            let house = &self.sim.houses[owner];
            assert_eq!(
                house
                    .tracking
                    .active_count(EntityCategory::Structure, type_ref),
                int(&native["live_buildings"]),
                "{boundary}: live presence"
            );
            assert_eq!(
                house
                    .tracking
                    .owned_count(EntityCategory::Structure, type_ref),
                int(&native["owned_buildings"]),
                "{boundary}: retained ownership"
            );
            assert_eq!(
                house.tracking.buildings(),
                int(&native["tracked_buildings"]),
                "{boundary}: tracking"
            );
            assert_eq!(
                house.stats.buildings_lost(),
                int(&native["losses"]) as u32,
                "{boundary}: losses"
            );
            assert_eq!(
                house.stats.score_points(),
                int(&native["score"]),
                "{boundary}: credit"
            );
            let power = &self.sim.power_states[owner];
            assert_eq!(
                power.total_output,
                int(&native["power"]),
                "{boundary}: cached power"
            );
            let flags = serde_json::to_value(power).unwrap();
            assert_eq!(
                flags["power_dirty"],
                native["dirty"][0] == 1,
                "{boundary}: power dirty"
            );
            assert_eq!(
                flags["radar_dirty"],
                native["dirty"][1] == 1,
                "{boundary}: radar dirty"
            );
        }
    }

    fn native_id(&self, id: u64) -> i32 {
        self.sim
            .substrate
            .entities
            .get(id)
            .map(|e| e.native_unique_id)
            .or_else(|| self.sim.substrate.anims.get(id).map(|a| a.native_unique_id))
            .or_else(|| self.sim.projectiles.get(id).map(|p| p.native_unique_id))
            .or_else(|| {
                self.sim
                    .smudge_grid
                    .as_ref()?
                    .object(id)
                    .map(|s| s.native_unique_id())
            })
            .expect("retained observed native identity")
    }

    fn identities(&self) -> std::collections::BTreeMap<u64, i32> {
        let mut ids = std::collections::BTreeMap::new();
        for (id, _) in self.sim.substrate.entities.iter_sorted() {
            ids.insert(id, self.native_id(id));
        }
        for (&id, _) in self.sim.substrate.anims.iter() {
            ids.insert(id, self.native_id(id));
        }
        for (&id, _) in self.sim.projectiles.iter() {
            ids.insert(id, self.native_id(id));
        }
        for (id, _) in self.sim.smudge_grid.as_ref().unwrap().objects() {
            ids.insert(id, self.native_id(id));
        }
        ids
    }

    fn assert_stock_marks(&self, native: &Value) {
        let grid = self.sim.smudge_grid.as_ref().unwrap();
        let mut actual: Vec<_> = grid
            .iter_occupied()
            .map(|(x, y, cell)| {
                (
                    y,
                    x,
                    i32::from(cell.type_id.unwrap()),
                    i32::from(cell.frame_offset),
                )
            })
            .collect();
        actual.sort();
        let mut expected: Vec<_> = native["smudge_cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                (
                    int(&cell["cell"][1]) as u16,
                    int(&cell["cell"][0]) as u16,
                    int(&cell["type_index"]),
                    int(&cell["data"]),
                )
            })
            .collect();
        expected.sort();
        assert_eq!(actual, expected, "native persistent footprint");
    }

    fn assert_stock_pending_snapshot(&mut self, route: &Value) {
        use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
        use std::hash::Hasher;
        let Some(stock) = route.get("stock_runtime") else {
            return;
        };
        let constructors: Vec<_> = stock["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["kind"] == "smudge_constructor")
            .collect();
        // Snapshot285 joins main's deferred Smudge identities with the
        // branch's shared Techno Door. Exercise both in the same full save/load,
        // using a surviving actor so deferred cleanup must retain its Door.
        let actor = self.sim.substrate.entities.get_mut(self.unit).unwrap();
        let original_actor = actor.clone();
        actor.open_door(7, self.sim.session.binary_frame);
        let door_phase = actor.door_phase();
        assert_eq!(door_phase, crate::sim::door::DoorPhase::Opening);
        let door_timer = actor.door_timer_fields();
        let mission = actor.mission;
        let grid = self.sim.smudge_grid.as_ref().unwrap();
        assert_eq!(grid.object_count(), constructors.len());
        for ((id, object), native) in grid.objects().zip(constructors) {
            let expected = &native["constructed_object"];
            assert_eq!(object.native_unique_id(), int(&expected["id"]));
            assert_eq!(object.health(), int(&expected["health"]));
            assert_eq!(object.object_alive(), expected["alive"] == 1);
            assert_eq!(object.in_limbo(), expected["limbo"] == 1);
            let location = object.location();
            assert_eq!(
                [location.x, location.y, location.z],
                [
                    int(&expected["location"][0]),
                    int(&expected["location"][1]),
                    int(&expected["location"][2])
                ]
            );
            assert_eq!(
                self.rules.smudge_types.get(object.type_id()).unwrap().name,
                native["type_name"]
            );
            assert!(!self.sim.logic_order().contains(&id));
            assert!(self.sim.substrate.pending_delete.contains(&id));
        }
        self.assert_stock_marks(&stock["final"]);

        let bytes = GameSnapshot::save(&self.sim, 0, 0, "native-stock-smudge-pending", 0);
        // Keep the native route's live continuation at its original input.
        *self.sim.substrate.entities.get_mut(self.unit).unwrap() = original_actor;
        let mut restored = GameSnapshot::load_unchecked(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        let actor = restored.substrate.entities.get(self.unit).unwrap();
        assert_eq!(actor.door_phase(), door_phase, "shared Door phase");
        assert_eq!(actor.door_timer_fields(), door_timer, "shared Door clock");
        assert_eq!(actor.mission, mission, "shared MissionCom");
        // Native load resets Scenario RNG and Bullet timers. Check the new
        // lifetime authority and its hash contribution without overriding that
        // existing load behavior or claiming unchanged whole-world execution.
        assert_eq!(
            bincode::serialize(restored.smudge_grid.as_ref().unwrap()).unwrap(),
            bincode::serialize(grid).unwrap()
        );
        assert_eq!(
            restored.substrate.pending_delete,
            self.sim.substrate.pending_delete
        );
        assert_eq!(
            restored.native_unique_ids.as_ref().unwrap().current_raw(),
            self.sim.native_unique_ids.as_ref().unwrap().current_raw()
        );
        let mut original_hash = std::collections::hash_map::DefaultHasher::new();
        grid.fold_objects(&mut original_hash);
        let mut restored_hash = std::collections::hash_map::DefaultHasher::new();
        restored
            .smudge_grid
            .as_ref()
            .unwrap()
            .fold_objects(&mut restored_hash);
        assert_eq!(restored_hash.finish(), original_hash.finish());
        let first = grid.objects().next().unwrap().0;
        let mut without_lifetime = grid.clone();
        without_lifetime.finalize_remove(first);
        let mut removed_hash = std::collections::hash_map::DefaultHasher::new();
        without_lifetime.fold_objects(&mut removed_hash);
        assert_ne!(removed_hash.finish(), original_hash.finish());

        let mut missing_queue = GameSnapshot::load_unchecked(&bytes).unwrap().sim;
        missing_queue
            .substrate
            .pending_delete
            .retain(|id| *id != first);
        assert_eq!(
            missing_queue.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                registry: "SmudgeGrid",
                object_id: first
            })
        );
        let mut in_logic = GameSnapshot::load_unchecked(&bytes).unwrap().sim;
        in_logic.substrate.logic.try_push(first).unwrap();
        assert_eq!(
            in_logic.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::InactiveLogicIdentity {
                registry: "SmudgeGrid",
                object_id: first
            })
        );

        restored.process_pending_delete_with(Some(&self.rules), Some(&self.registry));
        let actor = restored.substrate.entities.get(self.unit).unwrap();
        assert_eq!(
            actor.door_phase(),
            door_phase,
            "Door survives Smudge cleanup"
        );
        assert_eq!(
            actor.door_timer_fields(),
            door_timer,
            "Door clock survives cleanup"
        );
        assert_eq!(restored.smudge_grid.as_ref().unwrap().object_count(), 0);
        assert!(restored.substrate.pending_delete.is_empty());
        assert_eq!(
            restored
                .smudge_grid
                .as_ref()
                .unwrap()
                .iter_occupied()
                .count(),
            stock["final"]["smudge_cells"].as_array().unwrap().len()
        );
    }

    fn assert_stock_expiries(
        &self,
        route: &Value,
        identities: &std::collections::BTreeMap<u64, i32>,
    ) {
        use super::lifecycle::LifecycleTestEvent;
        let Some(stock) = route.get("stock_runtime") else {
            return;
        };
        let expected: Vec<_> = stock["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["kind"] == "expiry" && call["object"]["vtable"] == "0x007F32FC")
            .collect();
        let observed: Vec<_> = self
            .sim
            .lifecycle_test_events_for_test()
            .iter()
            .filter(|event| matches!(event, LifecycleTestEvent::SmudgeExpiryBoundary { .. }))
            .collect();
        assert_eq!(observed.len(), expected.len(), "native Smudge expiry count");
        for (actual, native) in observed.into_iter().zip(expected) {
            let LifecycleTestEvent::SmudgeExpiryBoundary {
                stable_id,
                native_id,
                native_cursor,
                object_alive,
                in_limbo,
                health,
                location,
                pending,
                generic_objects,
                rng,
            } = actual
            else {
                unreachable!()
            };
            assert_eq!(identities[stable_id], *native_id);
            assert_eq!(*native_id, int(&native["object"]["id"]));
            assert_eq!(
                *native_cursor,
                int(&native["before"]["unique_id_cursor"]) as u32
            );
            assert_eq!(*object_alive, native["object"]["alive"] == 1);
            assert_eq!(*in_limbo, native["object"]["limbo"] == 1);
            assert_eq!(*health, int(&native["object"]["health"]));
            assert_eq!(
                *location,
                [
                    int(&native["object"]["location"][0]),
                    int(&native["object"]["location"][1]),
                    int(&native["object"]["location"][2])
                ]
            );
            for (actual, key) in [
                (pending, "pending_members"),
                (generic_objects, "generic_object_members"),
            ] {
                let actual: Vec<_> = actual.iter().map(|id| identities[id]).collect();
                let expected: Vec<_> = native["before"][key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|object| int(&object["id"]))
                    .collect();
                assert_eq!(actual, expected, "native expiry {key}");
            }
            for (actual, key) in rng.iter().zip(["scenario", "main", "mapgen"]) {
                assert_eq!(
                    actual,
                    native["before"]["rng"][key].as_str().unwrap(),
                    "expiry full {key}"
                );
            }
        }
        let grid = self.sim.smudge_grid.as_ref().unwrap();
        assert_eq!(
            grid.object_count(),
            int(&stock["final"]["smudge_object_count"]) as usize
        );
        self.assert_stock_marks(&stock["final"]);
    }
}

#[test]
fn original_ready_mtnk_shot_runs_building_fires_crew_and_deferred_cleanup() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_death_anims_joined.json",
    ))
    .unwrap();
    assert_ready_fatal_routes(&corpus);
}

#[test]
fn original_stock_smudges_join_ready_shot_crew_and_deferred_cleanup() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_death_anims_joined_stock_smudges.json",
    ))
    .unwrap();
    assert_ready_fatal_routes(&corpus);
}

fn assert_ready_fatal_routes(corpus: &Value) {
    let mut escaped = false;
    let mut refused = false;
    for route in corpus["routes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|route| route["input"]["route"] == "mtnk_105mm_ap_fatal")
    {
        let label = format!("original seed{}", int(&route["input"]["seed"]));
        let mut fixture = Fixture::new(&corpus, &route["before"]);
        fixture.receiver_birth_floor = i32::MIN;
        fixture.assert_live_frame(
            route,
            &route["before"],
            &format!("{label} ready native prior"),
        );
        fixture.sim.commit_fire_visit(
            crate::sim::combat::world_receiver::FireVisit::Direct {
                id: fixture.unit,
                target: TargetKind::Entity(fixture.building),
                weapon_index: int(&route["shot"]["slot"]),
            },
            &fixture.rules,
            Some(&fixture.registry),
        );
        fixture.assert_live_frame(
            route,
            &route["after_launch"],
            &format!("{label} MTNK launch"),
        );
        let bullet = fixture
            .sim
            .projectiles
            .iter()
            .map(|(_, p)| p)
            .next()
            .unwrap();
        assert_eq!(bullet.native_unique_id, int(&route["shot"]["bullet_id"]));
        assert_eq!(
            [
                bullet.launch_origin.x,
                bullet.launch_origin.y,
                bullet.launch_origin.z
            ],
            [
                int(&route["shot"]["launch_position"][0]),
                int(&route["shot"]["launch_position"][1]),
                int(&route["shot"]["launch_position"][2])
            ]
        );
        for frame in route["frames"].as_array().unwrap() {
            fixture
                .sim
                .advance_live_object_pass(Some(&fixture.rules), Some(&fixture.registry))
                .unwrap();
            fixture.assert_live_frame(
                route,
                &frame["after_objects"],
                &format!("{label} dynamic Logic pass"),
            );
            fixture.power_prefix();
            fixture.assert_live_frame(
                route,
                &frame["after_power"],
                &format!("{label} House power prefix"),
            );
            if frame["after_objects"]["alive"] == 0 {
                break;
            }
            fixture.sim.session.binary_frame = fixture.sim.session.binary_frame.wrapping_add(1);
            fixture
                .sim
                .process_pending_delete_with(Some(&fixture.rules), Some(&fixture.registry));
        }
        let retained_slot = fixture
            .sim
            .substrate
            .entities
            .get(fixture.building)
            .unwrap()
            .building_anim_slots[3]
            .unwrap();
        fixture.assert_stock_pending_snapshot(route);
        let identities = fixture.identities();
        fixture.sim.session.binary_frame = int(&route["after_frame_increment"]["frame"]) as u32;
        fixture
            .sim
            .process_pending_delete_with(Some(&fixture.rules), Some(&fixture.registry));
        assert!(
            fixture
                .sim
                .substrate
                .entities
                .get(fixture.building)
                .is_none()
        );
        assert!(fixture.sim.substrate.anims.get(retained_slot).is_none());
        assert!(fixture.sim.substrate.pending_delete.is_empty());
        fixture.assert_graph(
            route,
            &route["after_drain"],
            &format!("{label} deferred destructor"),
        );
        fixture.assert_attacker(
            route,
            &route["after_drain"],
            &format!("{label} deferred destructor"),
        );
        fixture.assert_occupation(
            &route["after_drain"],
            &format!("{label} deferred destructor"),
        );
        fixture.assert_houses(
            &route["after_drain"],
            &format!("{label} deferred destructor"),
        );
        assert_rng(
            &fixture.sim,
            &route["after_drain"]["rng"],
            &format!("{label} deferred destructor"),
        );
        fixture.assert_stock_expiries(route, &identities);
        if route["after_drain"]["crew"].as_array().unwrap().is_empty() {
            refused = true;
        } else {
            escaped = true;
        }
    }
    assert!(
        escaped && refused,
        "normal native Mark routes must cover crew escape and refusal"
    );
}

#[test]
fn original_direct_ap_receivers_join_debris_crew_uninit_power_and_deferred_destructor() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_death_anims_joined.json",
    ))
    .unwrap();
    assert_eq!(
        corpus["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let route = &corpus["routes"][1];
    assert_eq!(route["input"]["route"], "direct_ap_multi_hit_control");
    let mut fixture = Fixture::new(&corpus, &route["before"]);
    fixture.assert_state(&route["before"], "direct prior");
    for (index, visit) in route["receiver_visits"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        fixture.hit();
        fixture.assert_state(
            &visit["after_receiver"],
            &format!("direct receiver {index}"),
        );
        fixture
            .sim
            .sample_building_health_for_house_update(fixture.building);
        fixture.power_prefix();
        fixture.assert_state(
            &visit["after_sample_and_power"],
            &format!("direct sample/power {index}"),
        );
    }
    let slots = fixture
        .sim
        .substrate
        .entities
        .get(fixture.building)
        .unwrap()
        .building_anim_slots;
    assert!(slots[3].is_some(), "UnInit retains ordinary attached ART");
    fixture.sim.session.binary_frame = int(&route["after_frame_increment"]["frame"]) as u32;
    fixture
        .sim
        .process_pending_delete_with(Some(&fixture.rules), Some(&fixture.registry));
    assert!(
        fixture
            .sim
            .substrate
            .entities
            .get(fixture.building)
            .is_none()
    );
    assert!(
        fixture.sim.substrate.anims.get(slots[3].unwrap()).is_none(),
        "destructor removes retained ART"
    );
    assert!(fixture.sim.substrate.pending_delete.is_empty());
    assert_rng(
        &fixture.sim,
        &route["after_drain"]["rng"],
        "direct deferred destructor",
    );
    fixture.assert_houses(&route["after_drain"], "direct deferred destructor");
}
