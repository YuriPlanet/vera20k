//! Original519630/701410 ordinary Engineer building-entry comparisons.
//!
//! engineer_repair_joined executes the original command delivery and Walk
//! arrival on the pinned active-retail image. Like that witness, these controls
//! supply an admitted Building, settled House/RNG state, and an already-paid
//! Walk head. They exercise the production command, Walk, PerCell, repair,
//! ownership and deferred-destruction owners; full travel/loader/audio-device
//! composition is covered separately by context_order_retail_engineer_tests.

use super::{EngineerBuildingAction, SimSoundEvent, Simulation, entry_test_fixture};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::command::Command;
use crate::sim::components::{DriveCoord, Health, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::{HouseState, MatchStatistics};
use crate::sim::intern::InternedId;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::power_system::PowerState;
use crate::sim::rng::{SimRng, trace_draws};
use serde_json::{Value, json};

fn corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_repair_joined.json",
    ))
    .unwrap();
    assert_eq!(native["source"], "unicorn/gamemd.exe");
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed scalar")).unwrap()
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    }
}

fn install_rng(sim: &mut Simulation, native: &Value) {
    sim.main_rng = SimRng::from_native_state_hex_for_test(native["main"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(native["mapgen"].as_str().unwrap());
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(native["scenario"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, native: &Value, boundary: &str) {
    for (name, rng) in [
        ("main", &sim.main_rng),
        ("mapgen", &sim.mapgen_rng),
        ("scenario", &sim.scenario_rng),
    ] {
        // Do not dump the 250-word buffers on a failed comparison.
        assert!(
            rng.native_state_hex() == native[name].as_str().unwrap(),
            "{boundary}: full {name} logical state differs"
        );
    }
}

fn install_mission(entity: &mut GameEntity, native: &Value) {
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(int(&native["current_mission"])),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(int(&native["queued_mission"])),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        // CdTimer's middle storage dword is not its duration.
        dispatch_timer: MissionDispatchTimer::from_raw(
            int(&native["mission_timer"][0]),
            int(&native["mission_timer"][2]),
        ),
    });
    entity
        .mission_leaf
        .set_infantry_doing_verified(int(&native["doing"]))
        .unwrap();
}

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    registry: OverlayTypeRegistry,
    building: u64,
    native_building: i32,
    engineers: Vec<u64>,
    houses: Vec<(i32, InternedId)>,
}

impl Fixture {
    fn new(route: &Value, native: &Value, threshold: &str) -> Self {
        // Exact stock ART input subset, read by the production ART reader.
        // Foundation is deliberately absent from the rules layer.
        let art = IniFile::from_str(
            "[GAPOWR]\nFoundation=2x2\nActiveAnim=GAPOWR_A\n\
             ActiveAnimDamaged=GAPOWR_AD\nActiveAnimZAdjust=-32\n\
             ActiveAnimYSort=362\nActiveAnimPoweredSpecial=true\n\
             ActiveAnimPowered=false\n[GAPOWR_A]\nImage=GAPOWR_A\n\
             Normalized=yes\nNewTheater=yes\nLayer=ground\nStart=0\n\
             LoopStart=0\nLoopEnd=8\nLoopCount=-1\nRate=220\nDetailLevel=2\n\
             [GAPOWR_AD]\nImage=GAPOWR_A\nNormalized=yes\nLayer=ground\n\
             Start=8\nLoopStart=8\nLoopEnd=16\nLoopCount=-1\nRate=220\n\
             DetailLevel=2\n[ENGINEER]\nSequence=EngineerSequence\n\
             Crawls=yes\nRemapable=yes\nFireUp=2\n[EngineerSequence]\n\
             Ready=0,1,1\nGuard=0,1,1\nWalk=8,6,6\n",
        );
        let name = route["name"].as_str().unwrap();
        // The noncapturable row supplies a changed type flag after command
        // delivery. Installing it before the friendly repair command is
        // equivalent at that boundary: friendly repair never reads Capturable.
        let capture_scatter = native["routes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|route| route["name"] == "repair_order_enemy_owner_race")
            .unwrap()["arrivals"][0]["trace"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "scatter_original_mission_control")
            .unwrap()["scatter"]
            .clone();
        // MissionControl's constructor default is Scatter=yes. This original
        // captured control is a required read input, not a constructor default.
        let engineer_sight = native["engineer_layers"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find_map(|layer| layer["physical_keys"]["ENGINEER"]["Sight"].as_str())
            .unwrap();
        // Infantry51E0EF clears discovery41B for exactly Sight=0. Adopt the
        // recorded retail key so this fixture retains the original Engineer's
        // prior discovery through its paid Walk head.
        let extra = format!(
            "[General]\nEngineerCaptureLevel={threshold}\n\
             [Capture]\nScatter={}\n\
             [ENGINEER]\nSight={engineer_sight}\n\
             [AudioVisual]\nConditionYellow=50%\nBuildingRepairedSound=BuildingRepaired\n\
             [Animations]\n82=GAPOWR_A\n83=GAPOWR_AD\n\
             [BuildingTypes]\n0=GAPOWR\n1=CABHUT\n[GAPOWR]\n\
             Strength=750\nCost=800\nPower=200\nCapturable={}\nRepairable=yes\n",
            if capture_scatter == 1 { "yes" } else { "no" },
            if name == "enemy_noncapturable_consumes" {
                "no"
            } else {
                "yes"
            }
        );
        let (mut sim, mut rules, registry) =
            entry_test_fixture::fixture_with_rules_and_fixed_art(&extra, &art);
        // RuleSet's standalone ART fixture retains invalid sound IDs until
        // the process sound-binding owner runs. Supply the native fixture's
        // selected Voc catalog identities through that same production reader;
        // audio-device/sample progression is outside this comparison.
        let mut sounds = String::from("[SoundList]\n");
        for sound in native["selected_sound_inputs"].as_array().unwrap() {
            sounds.push_str(&format!(
                "{}={}\n",
                int(&sound["fixture_index"]),
                sound["name"].as_str().unwrap()
            ));
        }
        rules.bind_type_sound_references(
            &IniFile::from_str(&extra),
            &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(&sounds)),
        );
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        // The native packet also carries constructor inputs for other
        // buildings and debris. This bounded fixture installs only GAPOWR's
        // normal/damaged ART, so bind its native SHP counts alone.
        let mut bound_animations = 0;
        for animation in native["native_anim_inputs"].as_array().unwrap() {
            let name = animation["name"].as_str().unwrap();
            if art.section(name).is_none() {
                continue;
            }
            rules.bind_anim_frame_count_for_test(name, int(&animation["raw_shp_frame_count"]));
            bound_animations += 1;
        }
        assert_eq!(bound_animations, 2);
        let owner = sim.interner.intern("Americans");
        let mut houses = Vec::new();
        for (index, house) in route["arrivals"][0]["before"]["houses"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let id = if index == 0 {
                owner
            } else {
                sim.interner.intern("Second")
            };
            sim.houses
                .insert(id, HouseState::new(id, 0, None, index == 0, 5000, 10));
            sim.session.house_order.push(id);
            houses.push((int(&house["pointer"]), id));
        }
        sim.session.current_house = Some(owner);
        // The reused native fixture supplies its game mode explicitly;
        // this is a caller prior, not a retail startup-default assertion.
        sim.session.game_mode_nonzero = int(&route["arrivals"][0]["before"]["game_mode"]) != 0;
        if name == "allied_damaged" {
            sim.house_alliances
                .insert("AMERICANS".into(), ["SECOND".into()].into_iter().collect());
            sim.house_alliances
                .insert("SECOND".into(), ["AMERICANS".into()].into_iter().collect());
        }
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(999));
        let building = sim
            .spawn_object("GAPOWR", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let before = &route["arrivals"][0]["before"]["building"];
        {
            let entity = sim.substrate.entities.get_mut(building).unwrap();
            entity.finish_building_construction_for_test();
            ground_pose::put_location(&mut entity.position, coord(&before["position"]));
            entity.health.current = int(&before["actual_hp"]);
            entity.estimated_health.reset(int(&before["estimated_hp"]));
            entity.repairing = before["paid_repair"] == 1;
            entity.sample_building_health_for_power();
        }
        sim.set_building_anim_slot(building, 3, true, false, 0, &rules)
            .unwrap();
        sim.set_building_damage_state(building, true, &rules);
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1));
        let mut engineers = Vec::new();
        for (index, command) in route["commands"].as_array().unwrap().iter().enumerate() {
            let id = sim
                .spawn_object("ENGINEER", "Americans", 9, 10 + index as u16, 0, &rules)
                .unwrap();
            assert_eq!(
                command["action"],
                native["original_action_controls"]["own_damaged_action"]
            );
            assert_eq!(
                sim.engineer_building_action(id, building, &rules),
                Some(EngineerBuildingAction::Repair(true))
            );
            assert!(sim.apply_command_with_overlays(
                "Americans",
                &Command::CaptureBuilding {
                    engineer_id: id,
                    target_building_id: building,
                },
                Some(&rules),
                Some(&registry)
            ));
            // The native command witness follows Event4C6CB0 with the
            // ordinary Mission5B3570 Commence receiver before its snapshot.
            sim.mission_commence_exact(id, 0).unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            assert_eq!(
                entity.mission.effective().raw(),
                int(&command["delivered_mission"])
            );
            assert_eq!(
                entity.navigation.nav_com,
                Some(NavTargetRef::Building { id: building })
            );
            install_mission(entity, &route["before_arrivals"][index]["engineer"]);
            engineers.push(id);
        }
        let new_owner = houses
            .iter()
            .find(|(pointer, _)| *pointer == int(&before["owner"]))
            .unwrap()
            .1;
        if new_owner != owner {
            // Supply the native owner-race prior using the shared owner to
            // install coherent House/occupancy indexes, then restore the
            // witness's pre-arrival scalar statistics and paid repair state.
            sim.change_owner_with_rules(building, new_owner, &rules, Some(&registry));
            let entity = sim.substrate.entities.get_mut(building).unwrap();
            entity.repairing = before["paid_repair"] == 1;
            entity.has_been_captured = false;
            // The preceding fixture transfer is supplied setup, not this
            // arrival. Consume its identical idle request through the owner.
            entity.apply_queued_building_body(0, &sim.session.game_options);
        }
        if name == "enemy_selling_refusal" {
            let entity = sim.substrate.entities.get_mut(building).unwrap();
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Selling),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
        } else if name == "enemy_warped_refusal" {
            sim.substrate.entities.get_mut(building).unwrap().temporal =
                crate::sim::temporal::TemporalState::warped_by_for_test(0xf001);
        } else if name == "missing_physical_building" {
            for y in 10..12 {
                for x in 10..12 {
                    sim.substrate.occupancy.remove(x, y, building);
                }
            }
        } else if name == "different_first_physical_building" {
            // Native supplies a second physical Building outside its ordinary
            // class/UID registration. Give the cell list a real stable identity,
            // while excluding this prior-only object from the compared Houses.
            let other_owner = sim.interner.intern("UnregisteredPrior");
            let type_ref = sim.interner.intern("GAPOWR");
            let other = 90_000;
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                other,
                10,
                10,
                0,
                0,
                other_owner,
                Health { current: 374 },
                type_ref,
                EntityCategory::Structure,
                0,
                5,
                false,
            );
            entity.lifecycle.object_alive = true;
            entity.lifecycle.in_limbo = false;
            sim.substrate.entities.insert(entity);
            sim.substrate.occupancy.remove(10, 10, building);
            for id in [other, building] {
                sim.substrate.occupancy.add(
                    10,
                    10,
                    id,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
        }
        for (prior, (_, id)) in route["arrivals"][0]["before"]["houses"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&houses)
        {
            sim.houses.get_mut(id).unwrap().stats =
                MatchStatistics::from_totals_for_test(0, 0, 0, 0, 0, 0);
            // Restore only the supplied pre-arrival House1F4 through the
            // existing persistence representation; arrival writes stay with
            // the shared discovery owner and are compared below.
            let mut house = serde_json::to_value(&sim.houses[id]).unwrap();
            house["discovered_by_current_house"] =
                json!(prior["first_current_viewer_discovery_1f4"] == 1);
            sim.houses
                .insert(*id, serde_json::from_value(house).unwrap());
            let mut state = serde_json::to_value(PowerState::default()).unwrap();
            state["total_output"] = prior["power"].clone();
            state["total_drain"] = prior["drain"].clone();
            state["power_dirty"] = json!(prior["dirty"][0] == 1);
            state["radar_dirty"] = json!(prior["dirty"][1] == 1);
            sim.power_states
                .insert(*id, serde_json::from_value(state).unwrap());
        }
        // The native witness supplies constructor-cleared discovery history
        // for its admitted Building. Restore that prior after installing the
        // coherent ownership indexes, never after the compared arrival.
        let history = &before["discovery"];
        let discovery = &mut sim.substrate.entities.get_mut(building).unwrap().discovery;
        discovery.owned_by_current_house = history["owned_by_current_house"] == 1;
        discovery.discovered_by_current_house = history["discovered_by_current_house"] == 1;
        discovery.discovered_by_other_house = history["discovered_by_other_house"] == 1;
        sim.sound_events.clear();
        Self {
            sim,
            rules,
            registry,
            building,
            native_building: int(&route["commands"][0]["delivered_destination"]),
            engineers,
            houses,
        }
    }

    fn assert_building(&self, native: &Value, boundary: &str) {
        let entity = self.sim.substrate.entities.get(self.building).unwrap();
        assert_eq!(
            entity.health.current,
            int(&native["actual_hp"]),
            "{boundary}: actual HP"
        );
        assert_eq!(
            entity.estimated_health.get(),
            int(&native["estimated_hp"]),
            "{boundary}: estimated HP"
        );
        assert_eq!(
            entity.building_power_health_sample(),
            Some(int(&native["sampled_hp"])),
            "{boundary}: retained HP sample"
        );
        assert_eq!(
            entity.repairing,
            native["paid_repair"] == 1,
            "{boundary}: paid repair"
        );
        assert_eq!(
            entity.building_damage_state_active,
            native["damaged_mode"] == 1,
            "{boundary}: damage mode"
        );
        assert_eq!(
            entity.owner(),
            self.houses
                .iter()
                .find(|(pointer, _)| *pointer == int(&native["owner"]))
                .unwrap()
                .1,
            "{boundary}: owner"
        );
        let history = &native["discovery"];
        assert_eq!(
            entity.discovery.owned_by_current_house,
            history["owned_by_current_house"] == 1,
            "{boundary}: native41A owner classification"
        );
        assert_eq!(
            entity.discovery.discovered_by_current_house,
            history["discovered_by_current_house"] == 1,
            "{boundary}: native41B current-viewer history"
        );
        assert_eq!(
            entity.discovery.discovered_by_other_house,
            history["discovered_by_other_house"] == 1,
            "{boundary}: native41C other-viewer history"
        );
        let object = self.rules.object("GAPOWR").unwrap();
        let dimensions = crate::sim::production::foundation_dimensions(&object.foundation);
        assert_eq!(
            [i32::from(dimensions.0), i32::from(dimensions.1)],
            [
                int(&native["foundation_dimensions"][0]),
                int(&native["foundation_dimensions"][1])
            ],
            "{boundary}: ART foundation"
        );
        assert_eq!(
            ground_pose::object_get_coords(entity, self.sim.resolved_terrain.as_ref()),
            coord(&native["get_coords"]),
            "{boundary}: Building GetCoords"
        );
        for slot in native["slots"].as_array().unwrap() {
            let anim = self
                .sim
                .anim(entity.building_anim_slots[3].unwrap())
                .unwrap();
            assert_eq!(
                self.sim.resolve(anim.type_id),
                slot["type_name"].as_str().unwrap(),
                "{boundary}: active/damaged ART consumer"
            );
        }
    }

    fn assert_houses(&self, native: &Value, boundary: &str) {
        let type_ref = self.sim.interner.get("GAPOWR").unwrap();
        for expected in native.as_array().unwrap() {
            let owner = self
                .houses
                .iter()
                .find(|(pointer, _)| *pointer == int(&expected["pointer"]))
                .unwrap()
                .1;
            let house = &self.sim.houses[&owner];
            // Existing total projection, not a claim of per-opponent kill-table parity.
            let kills: u32 = expected["building_kills_by_house"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| int(n) as u32)
                .sum();
            assert_eq!(
                house.stats.buildings_killed(),
                kills,
                "{boundary}: capture kills"
            );
            assert_eq!(
                house.stats.buildings_lost(),
                int(&expected["building_losses"]) as u32,
                "{boundary}: capture loss"
            );
            assert_eq!(
                house.stats.score_points(),
                int(&expected["score"]),
                "{boundary}: shared score"
            );
            assert_eq!(
                house.building_capture_notified(),
                expected["captured_latch"] == 1,
                "{boundary}: trigger latch"
            );
            assert_eq!(
                house.discovered_by_current_house(),
                expected["first_current_viewer_discovery_1f4"] == 1,
                "{boundary}: nativeHouse1F4 current-viewer discovery"
            );
            assert_eq!(
                house
                    .tracking
                    .owned_count(EntityCategory::Structure, type_ref),
                int(&expected["owned_building_count"]),
                "{boundary}: owned count"
            );
            assert_eq!(
                house
                    .tracking
                    .active_count(EntityCategory::Structure, type_ref),
                int(&expected["live_building_count"]),
                "{boundary}: live count"
            );
            assert_eq!(
                house.tracking.buildings(),
                int(&expected["tracked_ordinary_buildings"]),
                "{boundary}: tracked buildings"
            );
            let list: Vec<u64> = expected["buildings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|_| self.building)
                .collect();
            assert_eq!(
                house.base_projection.buildings(),
                list,
                "{boundary}: ordered House building list"
            );
            let power = &self.sim.power_states[&owner];
            assert_eq!(
                power.total_output,
                int(&expected["power"]),
                "{boundary}: cached power"
            );
            assert_eq!(
                power.total_drain,
                int(&expected["drain"]),
                "{boundary}: cached drain"
            );
            let serialized = serde_json::to_value(power).unwrap();
            assert_eq!(
                serialized["power_dirty"],
                expected["dirty"][0] == 1,
                "{boundary}: power dirty"
            );
            assert_eq!(
                serialized["radar_dirty"],
                expected["dirty"][1] == 1,
                "{boundary}: radar dirty"
            );
        }
    }

    fn prepare_paid_arrival(&mut self, index: usize, native: &Value) -> DriveCoord {
        let id = self.engineers[index];
        // Same supplied head and <17-lepton pre-arrival distance as original
        // walk_arrival. This does not purport to compare a whole route search.
        let head = DriveCoord::cell(10, 10 + index as u16, 0);
        self.sim.run_walk_boundary(
            id,
            DriveCoord {
                x: head.x - 16,
                ..head
            },
            Some(&self.rules),
            Some(&self.registry),
        );
        let entity = self.sim.substrate.entities.get_mut(id).unwrap();
        install_mission(entity, &native["engineer"]);
        let locomotor = entity.locomotor.as_mut().unwrap();
        assert_eq!(
            locomotor.walk_is_moving(),
            Some(native["engineer"]["locomotor"]["moving"] == 1)
        );
        locomotor.set_step_head(Some(head));
        install_rng(&mut self.sim, &native["rng"]);
        head
    }

    fn assert_arrival(&self, index: usize, native: &Value, boundary: &str) {
        self.assert_building(&native["building"], boundary);
        self.assert_houses(&native["houses"], boundary);
        let id = self.engineers[index];
        let entity = self.sim.substrate.entities.get(id).unwrap();
        let expected = &native["engineer"];
        assert_eq!(
            entity.health.current,
            int(&expected["actual_hp"]),
            "{boundary}: Engineer HP"
        );
        assert_eq!(
            entity.lifecycle.object_alive,
            expected["alive"] == 1,
            "{boundary}: Engineer alive"
        );
        assert_eq!(
            entity.lifecycle.in_limbo,
            expected["limbo"] == 1,
            "{boundary}: Engineer limbo"
        );
        assert_eq!(
            entity.lifecycle.cell_marked,
            expected["marked"] == 1,
            "{boundary}: cell mark"
        );
        assert_eq!(
            self.sim.logic_order().contains(&id),
            expected["logic_registered"] == 1,
            "{boundary}: Logic membership"
        );
        assert_eq!(
            entity.native_unique_id,
            int(&expected["uid"]),
            "{boundary}: retained UID"
        );
        assert_eq!(
            ground_pose::position_world_coord(&entity.position),
            coord(&expected["position"]),
            "{boundary}: physical position"
        );
        assert_eq!(
            entity.mission.current().raw(),
            int(&expected["current_mission"]),
            "{boundary}: current mission"
        );
        assert_eq!(
            entity.mission.queued().raw(),
            int(&expected["queued_mission"]),
            "{boundary}: queued mission"
        );
        if int(&expected["destination"]) == self.native_building {
            assert_eq!(
                entity.navigation.nav_com,
                Some(NavTargetRef::Building { id: self.building }),
                "{boundary}: retained object NavCom"
            );
        } else {
            // The native refusal arms resolve an FNPC Cell destination.
            // The shared Scatter goldens cover its cell choice; this joined
            // comparison checks the object-to-cell navigation transition.
            assert!(
                matches!(entity.navigation.nav_com, Some(NavTargetRef::Cell { .. })),
                "{boundary}: Scatter replaces object NavCom with Cell"
            );
        }
        assert_eq!(
            entity.mission.dispatch_timer().start_frame(),
            int(&expected["mission_timer"][0]),
            "{boundary}: mission timer anchor"
        );
        assert_eq!(
            entity.mission.dispatch_timer().delay(),
            int(&expected["mission_timer"][2]),
            "{boundary}: mission timer delay"
        );
        assert_eq!(
            entity.mission_leaf.as_infantry().unwrap().doing(),
            int(&expected["doing"]),
            "{boundary}: Doing"
        );
        let locomotor = entity.locomotor.as_ref().unwrap();
        assert_eq!(
            locomotor.walk_is_moving(),
            Some(expected["locomotor"]["moving"] == 1),
            "{boundary}: moving"
        );
        assert_eq!(locomotor.step_head(), None, "{boundary}: retired paid head");
        let class_count = (0..)
            .take_while(|index| {
                self.sim
                    .substrate
                    .entities
                    .infantry_registry_at(*index)
                    .is_some()
            })
            .count();
        assert_eq!(
            class_count,
            int(&native["infantry_registry_count"]) as usize,
            "{boundary}: retained class registry"
        );
        assert_eq!(
            self.sim.substrate.pending_delete.len(),
            int(&native["pending_delete_count"]) as usize,
            "{boundary}: deferred queue"
        );
        assert_rng(&self.sim, &native["rng"], boundary);
    }
}

#[test]
fn original_half_threshold_and_friendly_repair_object_actions() {
    let native = corpus();
    let route = &native["routes"][0];
    let controls = &native["original_action_controls"];
    let mut fixture = Fixture::new(route, &native, controls["raw_threshold"].as_str().unwrap());
    let id = fixture.engineers[0];
    let enemy = fixture.sim.interner.intern("EnemyControl");
    fixture
        .sim
        .houses
        .insert(enemy, HouseState::new(enemy, 0, None, false, 5000, 10));
    fixture.sim.change_owner_with_rules(
        fixture.building,
        enemy,
        &fixture.rules,
        Some(&fixture.registry),
    );
    for row in controls["enemy_rows"].as_array().unwrap() {
        fixture
            .sim
            .substrate
            .entities
            .get_mut(fixture.building)
            .unwrap()
            .health
            .current = int(&row["actual_hp"]);
        let before = fixture.sim.state_hash();
        let expected = match int(&row["action"]) {
            9 => EngineerBuildingAction::Capture,
            28 => EngineerBuildingAction::Damage,
            action => panic!("unexpected native action {action}"),
        };
        assert_eq!(
            fixture
                .sim
                .engineer_building_action(id, fixture.building, &fixture.rules),
            Some(expected),
            "native HP {}",
            row["actual_hp"]
        );
        assert!(row["rng_unchanged"].as_bool().unwrap());
        assert_eq!(
            fixture.sim.state_hash(),
            before,
            "object action is a read-only decision"
        );
    }
}

#[test]
fn original_nine_routes_ten_walk_arrivals_repair_capture_and_deferred_cleanup() {
    let native = corpus();
    assert_eq!(native["routes"].as_array().unwrap().len(), 9);
    let mut arrivals = 0;
    for route in native["routes"].as_array().unwrap() {
        let name = route["name"].as_str().unwrap();
        let mut fixture = Fixture::new(route, &native, "1.0");
        for (index, arrival) in route["arrivals"].as_array().unwrap().iter().enumerate() {
            let boundary = format!("{name}/{index}");
            fixture.assert_building(&arrival["before"]["building"], &boundary);
            fixture.assert_houses(&arrival["before"]["houses"], &boundary);
            let head = fixture.prepare_paid_arrival(index, &arrival["before"]);
            let sound_start = fixture.sim.sound_events.len();
            let id = fixture.engineers[index];
            if name == "repair_order_enemy_owner_race" {
                assert_eq!(
                    fixture
                        .sim
                        .substrate
                        .entities
                        .get(fixture.building)
                        .unwrap()
                        .queued_building_body_state(),
                    Some(-1),
                    "the supplied capture prior has no pending body request"
                );
            }
            let (changed, draws) = trace_draws(|| {
                fixture
                    .sim
                    .run_completed_walk_step(
                        id,
                        head,
                        Some(&fixture.rules),
                        Some(&fixture.registry),
                    )
                    .unwrap()
            });
            assert!(
                !changed,
                "{boundary}: ordinary building entry is not bridge publication"
            );
            fixture.assert_arrival(index, &arrival["after"], &boundary);
            if name == "repair_order_enemy_owner_race" {
                // Whole7014A0 calls BuildingIdle44D6E0, whose447780(1)
                // publishes queued+538 before Guard. Existing native body
                // comparisons cover43FFB4 clearing an identical request.
                assert_eq!(
                    fixture
                        .sim
                        .substrate
                        .entities
                        .get(fixture.building)
                        .unwrap()
                        .queued_building_body_state(),
                    Some(1),
                    "capture requests the existing idle body owner"
                );
            }
            assert_eq!(
                draws.len(),
                arrival["raw_advances"].as_array().unwrap().len(),
                "{boundary}: original raw RNG advance count"
            );
            for (draw, expected) in draws
                .iter()
                .zip(arrival["raw_advances"].as_array().unwrap())
            {
                assert_eq!(
                    draw["value"], expected["raw"],
                    "{boundary}: raw RNG word/order"
                );
            }
            let played = fixture.sim.sound_events[sound_start..].iter().filter(|event| matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "BuildingRepaired")).count();
            let expected = arrival["lifecycle"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| {
                    event["event"] == "sound_play"
                        && event["this"] == native["rules"]["repair_sound"]
                })
                .count();
            assert_eq!(
                played, expected,
                "{boundary}: resolved BuildingRepaired request"
            );
            arrivals += 1;
        }
        fixture
            .sim
            .process_pending_delete_with(Some(&fixture.rules), Some(&fixture.registry));
        for (index, after) in route["cleanup"]["after"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let uid = int(&after["engineer"]["uid"]);
            let survives_uid = after["object_uid_registry"]
                .as_array()
                .unwrap()
                .iter()
                .any(|pair| int(&pair[0]) == uid);
            let survives = fixture
                .sim
                .substrate
                .entities
                .contains(fixture.engineers[index]);
            assert_eq!(
                survives, survives_uid,
                "{name}/{index}: Engineer UID/store retirement"
            );
            assert_eq!(
                fixture
                    .sim
                    .substrate
                    .entities
                    .values()
                    .any(|entity| entity.native_unique_id == uid),
                survives_uid,
                "{name}/{index}: implicit UID roster"
            );
            let class_count = (0..)
                .take_while(|index| {
                    fixture
                        .sim
                        .substrate
                        .entities
                        .infantry_registry_at(*index)
                        .is_some()
                })
                .count();
            assert_eq!(
                class_count,
                int(&after["infantry_registry_count"]) as usize,
                "{name}: class compaction"
            );
            assert_eq!(
                fixture.sim.substrate.pending_delete.len(),
                int(&after["pending_delete_count"]) as usize,
                "{name}: deferred queue drained"
            );
        }
        // Repair leaves +544 unchanged; its later Building visit is the only
        // consumer that samples restored HP and invalidates House assessment.
        fixture
            .sim
            .sample_building_health_for_house_update(fixture.building);
        fixture.assert_building(&route["power_consumers"]["after_sample"]["building"], name);
    }
    assert_eq!(arrivals, 10);
}

#[test]
fn original_engineer_receiver_terminal_branches_return_before_foot_tail() {
    let native = corpus();
    for route in native["routes"].as_array().unwrap() {
        let name = route["name"].as_str().unwrap();
        let mut fixture = Fixture::new(route, &native, "1.0");
        for (index, arrival) in route["arrivals"].as_array().unwrap().iter().enumerate() {
            let head = fixture.prepare_paid_arrival(index, &arrival["before"]);
            let id = fixture.engineers[index];
            fixture
                .sim
                .run_walk_boundary(id, head, Some(&fixture.rules), Some(&fixture.registry));
            fixture
                .sim
                .substrate
                .entities
                .get_mut(id)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap()
                .set_step_head(None);
            let result = fixture
                .sim
                .infantry_per_cell_engineer_entry(id, &fixture.rules, Some(&fixture.registry))
                .unwrap();
            let foot_tail = arrival["trace"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"] == "foot_per_cell_tail");
            assert_eq!(
                result.return_before_foot, !foot_tail,
                "{name}/{index}: original branch before Foot4D85D0"
            );
            assert!(!result.bridge_state_changed, "{name}/{index}");
        }
    }
}
