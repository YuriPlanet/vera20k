//! Tank-bunker entry through the stock command and ordinary world turns.
//!
//! The original Unit destination setter (741DCC) sends DOCKING, whose
//! Building receiver (43C7E9) establishes contact. DOCK_NOW, and therefore
//! the bunker's Repair mission, belongs to the later arrival (73A31F).
//! The arena is prepared input; the continued installation below is an
//! integration regression, not a claim of complete native-frame parity.

use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::components::NavTargetRef;
use crate::sim::docking::bunker_install::BunkerState;
use crate::sim::docking::bunker_link::BunkerLink;
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;
use serde_json::{Value, json};

fn retail_bunker_rules() -> Option<RuleSet> {
    let mut rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let walls = [
        "NATBNK_A",
        "NATBNK_B",
        "NATBNK_AD",
        "NATBNK_BD",
        "NATBNK_A2",
        "NATBNK_B2",
        "NATBNK_A2D",
        "NATBNK_B2D",
    ]
    .map(str::to_owned);
    assert_eq!(
        rules.bind_anim_class_assets(&walls, &assets, "TEM", "TEMPERATE"),
        0
    );
    Some(rules)
}

fn advance(sim: &mut Simulation, rules: &RuleSet) {
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(&[], Some(rules), grid.as_deref(), None, 33);
}

#[test]
fn stock_tank_bunker_entry_contacts_before_arrival_then_installs() {
    let Some(rules) = retail_bunker_rules() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x43C7E9);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let bunker = sim
        .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object("LTNK", "Americans", 4, 10, 0, &rules)
        .unwrap();
    assert!(sim.apply_command(
        "Americans",
        &Command::clicked_mission(
            tank,
            MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(bunker),
        ),
        Some(&rules),
    ));

    // Observe the accepted command before movement or a Building mission
    // visit. The tank is still six cells away, so no arrival is supplied.
    let unit = sim.substrate.entities.get(tank).unwrap();
    let building = sim.substrate.entities.get(bunker).unwrap();
    assert_eq!((unit.position.rx(), unit.position.ry()), (4, 10));
    assert_eq!(unit.radio_contacts.slot(0), Some(bunker));
    assert_eq!(building.radio_contacts.slot(0), Some(tank));
    assert_eq!(
        unit.navigation.nav_com(),
        Some(NavTargetRef::object(bunker)),
    );
    assert_ne!(
        building.mission.effective().known(),
        Some(MissionType::Repair)
    );
    assert_eq!(building.bunker.occupant(), None);

    // The snapshot owns the real contact/NavCom state during approach.
    // Reload it before any arrival, then use ordinary turns to finish.
    let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    let path = sim.path_grid_snapshot();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "bunker approach", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    restored.install_fixture_path_grid(path.as_deref());
    sim = restored;
    assert_eq!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(bunker)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(bunker)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(tank)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::object(bunker))
    );

    // No second order, injected arrival, or direct install step: the same
    // admitted tank must complete the existing movement/install lifecycle.
    for _ in 0..900 {
        advance(&mut sim, &rules);
        if sim
            .substrate
            .entities
            .get(bunker)
            .unwrap()
            .bunker
            .occupant()
            == Some(tank)
        {
            break;
        }
    }
    let unit = sim.substrate.entities.get(tank).unwrap();
    let building = sim.substrate.entities.get(bunker).unwrap();
    assert_eq!(building.bunker.occupant(), Some(tank));
    assert_eq!(unit.bunker.link(), BunkerLink::Installed(bunker));
    assert_eq!(
        building.bunker_runtime.unwrap().state(),
        BunkerState::Occupied,
    );
    assert_eq!(unit.mission.effective().known(), Some(MissionType::Guard));
    assert!(!unit.selection.is_selected());
}

#[test]
fn stock_tank_bunker_stop_releases_the_approach_for_another_tank() {
    let Some(rules) = retail_bunker_rules() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4C75E0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let bunker = sim
        .spawn_object("NATBNK", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let first = sim
        .spawn_object("LTNK", "Americans", 4, 10, 0, &rules)
        .unwrap();
    let second = sim
        .spawn_object("LTNK", "Americans", 4, 12, 0, &rules)
        .unwrap();
    assert!(sim.apply_command(
        "Americans",
        &Command::clicked_mission(
            first,
            MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(bunker),
        ),
        Some(&rules)
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(bunker)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(first)
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::Stop { entity_id: first },
        Some(&rules)
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(first)
            .unwrap()
            .radio_contacts
            .slot(0),
        None
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(first)
            .unwrap()
            .navigation
            .nav_com(),
        None
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(bunker)
            .unwrap()
            .radio_contacts
            .slot(0),
        None
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::clicked_mission(
            second,
            MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(bunker),
        ),
        Some(&rules)
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(second)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(bunker)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(bunker)
            .unwrap()
            .radio_contacts
            .slot(0),
        Some(second)
    );
    for _ in 0..900 {
        advance(&mut sim, &rules);
        if sim
            .substrate
            .entities
            .get(bunker)
            .unwrap()
            .bunker
            .occupant()
            == Some(second)
        {
            break;
        }
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(bunker)
            .unwrap()
            .bunker
            .occupant(),
        Some(second)
    );
    assert_eq!(
        sim.substrate.entities.get(first).unwrap().bunker.link(),
        BunkerLink::None
    );
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn coord(value: &Value) -> crate::sim::components::DriveCoord {
    crate::sim::components::DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    }
}

/// Supplied types/map and live objects for the bounded original histories.
/// These synthetic 150/900-strength types are independent of the retail
/// command tests above. No original constructor, travel or match is claimed.
struct NativeScene {
    sim: Simulation,
    rules: RuleSet,
    unit: u64,
    bunker: u64,
    other: u64,
    second: u64,
}

impl NativeScene {
    fn new(inputs: &Value, row: &Value) -> Self {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::mission::{MissionDispatchTimer, MissionId};
        let unarmed = row["input"]["unarmed"].as_bool().unwrap();
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[VehicleTypes]\n0=PreparedTank\n[BuildingTypes]\n0=PreparedBunker\n\
             [PreparedTank]\nStrength=150\nSpeed=0\nSpeedType=Track\nMovementZone=Normal\n\
             Turret=yes\nBunkerable=yes\nROT=5\nLocomotor={{4A582741-9839-11D1-B709-00A024DDAFD1}}\n{}\
             [PreparedBunker]\nStrength=900\nBunker=yes\nNumberOfDocks=1\n\
             [PreparedWeapon]\nDamage=1\nRange=1\nProjectile=PreparedBullet\nWarhead=PreparedWarhead\n\
             [PreparedBullet]\n[PreparedWarhead]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [Enter]\nRate=.016\n[Guard]\nRate=.030\n",
            if unarmed {
                ""
            } else {
                "Primary=PreparedWeapon\n"
            },
        ));
        let art = IniFile::from_str("[PreparedBunker]\nFoundation=2x2\n");
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let tank_type = rules.object("PreparedTank").unwrap();
        assert_eq!(tank_type.strength, int(&inputs["unit"]["strength"]));
        assert_eq!(
            tank_type.has_turret,
            inputs["unit"]["turret"].as_bool().unwrap()
        );
        assert_eq!(
            tank_type.bunkerable,
            inputs["unit"]["bunkerable"].as_bool().unwrap()
        );
        assert_eq!(
            tank_type.speed_type as i32,
            int(&inputs["unit"]["speed_type"])
        );
        assert_eq!(
            tank_type.movement_zone as i32,
            int(&inputs["unit"]["movement_zone"])
        );
        let bunker_type = rules.object("PreparedBunker").unwrap();
        assert_eq!(bunker_type.strength, int(&inputs["bunker"]["strength"]));
        assert_eq!(
            bunker_type.bunker,
            inputs["bunker"]["bunker"].as_bool().unwrap()
        );
        assert_eq!(bunker_type.number_of_docks, int(&inputs["bunker"]["docks"]));
        assert_eq!(bunker_type.foundation, "2x2");
        assert_eq!(inputs["bunker"]["dimensions"], json!([2, 2]));
        // Only Enter's rate executes in these histories. Both the supplied
        // original double and the INI reader project it to14 frames.
        for (mission, name) in [(MissionType::Enter, "enter"), (MissionType::Guard, "guard")] {
            assert_eq!(
                rules.mission_control.entry(mission).unwrap().rate_frames,
                int(&inputs["mission_rate_conversions"][name]["frames"])
            );
        }

        let mut sim = Simulation::new();
        sim.session
            .set_binary_frame_for_test(int(&inputs["scenario"]["frame"]) as u32);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_game_mode(int(
                &inputs["scenario"]["game_mode"],
            )),
        );
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let map = &inputs["map"];
        assert_eq!(map["allocated"], json!([32, 32]));
        assert_eq!(map["level"], 0);
        assert_eq!(map["clear_speed_rows"], json!(vec![1.0; 9]));
        assert_eq!(map["water_speed_rows"], json!(vec![0.0; 9]));
        assert_eq!(map["units_in_cell_lists"], false);
        let local = &map["local"];
        let width = int(&map["width"]);
        let bounds = crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
            width,
            int(&local[0]),
            int(&local[1]),
            int(&local[2]),
            int(&local[3]),
        );
        sim.playfield_bounds = Some(bounds);
        sim.playfield_size_height = Some(width);
        sim.session.set_map_width_for_test(width as u16);
        sim.session.set_map_height_for_test(width as u16);
        let terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
        let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
        sim.zone_grid = Some(
            crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
                &path,
                &terrain,
                &[],
                Some((width, width)),
                Some(bounds),
            ),
        );
        sim.path_grid = Some(std::sync::Arc::new(path));
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
        sim.install_resolved_terrain_for_new_map(terrain);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner,
                0,
                None,
                row["input"]["human"].as_bool().unwrap(),
                0,
                0,
            ));
        let bunker = sim
            .spawn_object("PreparedBunker", "Americans", 8, 8, 0, &rules)
            .unwrap();
        let other = sim
            .spawn_object("PreparedBunker", "Americans", 20, 20, 0, &rules)
            .unwrap();
        let at = &row["input"]["unit_cell"];
        let unit = sim
            .spawn_object(
                "PreparedTank",
                "Americans",
                int(&at[0]) as u16,
                int(&at[1]) as u16,
                0,
                &rules,
            )
            .unwrap();
        let second = sim
            .spawn_object("PreparedTank", "Americans", 15, 14, 0, &rules)
            .unwrap();
        // Original fixture supplies only the buildings' footprint lists.
        // The units' prepared XYZ must not add an unobserved map member.
        sim.remove_entity_occupancy(unit);
        sim.remove_entity_occupancy(second);
        for id in [unit, second, bunker, other] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity
                .mission
                .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                    current: MissionId::from_raw(if id == unit || id == second {
                        int(&row["input"]["mission"])
                    } else {
                        5
                    }),
                    suspended: MissionId::NONE,
                    queued: MissionId::NONE,
                    movement_bypass_latch: 0,
                    handler_state: 0,
                    mission_start_frame: 0,
                    ai_counter: 0,
                    dispatch_timer: MissionDispatchTimer::from_raw(0, 0),
                });
            entity.radio_contacts.clear_all();
        }
        for id in [unit, second] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            assert_eq!(
                crate::sim::combat::combat_weapon::is_armed(
                    entity,
                    rules.object("PreparedTank").unwrap()
                ),
                !unarmed
            );
            entity.body_facing = crate::sim::movement::facing_class::FacingClass::new(
                int(&inputs["unit"]["facing"]) as u16,
                int(&inputs["unit"]["rot"]),
            );
            entity.navigation.set_nav_com_for_test(None);
            entity.navigation.set_nav_com_aux_for_test(None);
            entity
                .navigation
                .path_replay_mut()
                .set_directions_for_test(vec![u8::MAX, 0, 0, 0]);
            assert!(!entity.mega_mission.is_attack_move());
            assert_eq!(inputs["unit"]["mega_mission"], -1);
            assert!(
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .ensure_installed_track_state()
            );
        }
        sim.scenario_rng =
            crate::sim::rng::SimRng::new(inputs["scenario"]["seed"].as_u64().unwrap());
        Self {
            sim,
            rules,
            unit,
            bunker,
            other,
            second,
        }
    }

    fn name(&self, id: Option<u64>) -> Value {
        match id {
            None => Value::Null,
            Some(id) if id == self.unit => json!("unit"),
            Some(id) if id == self.bunker => json!("bunker"),
            Some(id) if id == self.other => json!("other"),
            Some(id) if id == self.second => json!("second_unit"),
            Some(id) => panic!("unrepresented native pointer {id}"),
        }
    }

    fn nav(&self, nav: Option<NavTargetRef>) -> Value {
        match nav {
            None => Value::Null,
            Some(NavTargetRef::Object { id }) => self.name(Some(id)),
            Some(NavTargetRef::Cell { rx, ry }) => json!([rx, ry]),
        }
    }

    fn snapshot(&self) -> Value {
        let unit = self.sim.substrate.entities.get(self.unit).unwrap();
        let bunker = self.sim.substrate.entities.get(self.bunker).unwrap();
        let other = self.sim.substrate.entities.get(self.other).unwrap();
        let second = self.sim.substrate.entities.get(self.second).unwrap();
        let drive = unit
            .locomotor
            .as_ref()
            .unwrap()
            .selected_drive_runtime()
            .unwrap()
            .retained()
            .unwrap();
        let xyz = |at: Option<crate::sim::components::DriveCoord>| {
            at.map_or(json!([0, 0, 0]), |at| json!([at.x, at.y, at.z]))
        };
        let position = crate::sim::movement::ground_pose::object_get_coords(
            unit,
            self.sim.resolved_terrain.as_ref(),
        );
        let rng = self.sim.scenario_rng.logical_view();
        let path = unit.navigation.path_replay();
        assert_eq!(path.cursor(), 0);
        let path: Vec<i32> = path
            .directions()
            .iter()
            .take(4)
            .map(|&x| if x == u8::MAX { -1 } else { i32::from(x) })
            .collect();
        json!({
            "unit_mission":unit.mission.current().raw(), "unit_queued":unit.mission.queued().raw(),
            "unit_nav":self.nav(unit.navigation.nav_com()), "unit_contact":self.name(unit.radio_contacts.slot(0)),
            "bunker_contact":self.name(bunker.radio_contacts.slot(0)), "other_contact":self.name(other.radio_contacts.slot(0)),
            "bunker_mission":bunker.mission.current().raw(), "bunker_queued":bunker.mission.queued().raw(),
            "bunker_state":bunker.bunker_runtime.unwrap().state() as u8,
            "unit_coord":[position.x,position.y,position.z],
            "drive_destination":xyz(drive.destination()), "drive_head":xyz(drive.head_to()),
            "drive_power":u8::from(unit.locomotor.as_ref().unwrap().is_powered()),
            "drive_track":drive.track().turn_index(), "drive_cursor":drive.track().cursor(),
            "mega_mission":if unit.mega_mission.is_attack_move() {29} else {-1},
            "unit_tether":u8::from(unit.tether.partner().is_some()), "bunker_tether":u8::from(bunker.tether.partner().is_some()),
            "archive":self.nav(unit.archive_target().map(Into::into)), "pending_entry":self.name(unit.pending_entry()),
            "second_nav":self.nav(second.navigation.nav_com()), "second_contact":self.name(second.radio_contacts.slot(0)),
            "unit_bunker":self.name(unit.bunker.link().installed_in()), "bunker_occupant":self.name(bunker.bunker.occupant()),
            "nav_aux":self.nav(unit.navigation.nav_com_aux()), "path":path,
            "rng_indices":[rng.index_a,rng.index_b],
        })
    }

    fn compare(&self, native: &Value, context: &str) {
        let actual = self.snapshot();
        assert_eq!(
            actual.as_object().unwrap().len(),
            native.as_object().unwrap().len(),
            "{context}: complete state projection"
        );
        for (key, value) in native.as_object().unwrap() {
            assert_eq!(&actual[key], value, "{context}: {key}");
        }
    }

    fn supply_position(&mut self, at: crate::sim::components::DriveCoord) {
        let unit = self.sim.substrate.entities.get_mut(self.unit).unwrap();
        unit.position.set_rx_for_test((at.x / 256) as u16);
        unit.position.set_ry_for_test((at.y / 256) as u16);
        unit.position
            .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(at.x % 256));
        unit.position
            .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(at.y % 256));
        unit.position.set_exact_z_leptons_for_test(Some(at.z));
    }

    fn step(&mut self, step: &Value) {
        use crate::sim::world::FrameEffects;
        let effects = FrameEffects::default();
        let mut answer = None;
        match step["op"].as_str().unwrap() {
            "set_destination" | "other_set_destination" | "replace_destination" => {
                let actor = if step["op"] == "other_set_destination" {
                    self.second
                } else {
                    self.unit
                };
                let target = if step["op"] == "replace_destination" {
                    self.other
                } else {
                    self.bunker
                };
                self.sim.set_unit_destination(
                    actor,
                    NavTargetRef::object(target),
                    &self.rules,
                    true,
                    effects,
                );
            }
            "enter" => {
                answer = Some(crate::sim::mission::enter::mission_enter(
                    &mut self.sim,
                    &self.rules,
                    None,
                    self.unit,
                    effects,
                ))
            }
            "radio_docking" => {
                answer = Some(
                    crate::sim::radio::transmit(
                        &mut self.sim,
                        self.unit,
                        self.bunker,
                        crate::sim::radio::RadioMessage::CanDock,
                        crate::sim::radio::RadioPayload::default(),
                        Some(&self.rules),
                        effects,
                    )
                    .code() as i32,
                )
            }
            "dock_coordinate" => {
                let at = crate::sim::movement::building_dock_coordinate(
                    &self.sim.substrate.entities,
                    self.bunker,
                    Some(self.unit),
                    self.sim.resolved_terrain.as_ref(),
                    &self.rules,
                    &self.sim.interner,
                )
                .unwrap();
                assert_eq!(at, coord(&step["coordinate"]));
            }
            "supply_arrival" => self.supply_position(coord(&step["coordinate"])),
            "per_cell" => {
                self.sim
                    .unit_dock_now(self.unit, &self.rules, None, effects);
            }
            "cancel_destination" => {
                self.sim
                    .set_unit_null_destination(self.unit, Some(&self.rules), None, effects);
            }
            "idle_event_calls" => {
                crate::sim::radio::broadcast_break(
                    &mut self.sim,
                    self.unit,
                    Some(&self.rules),
                    effects,
                );
                self.sim
                    .set_unit_null_destination(self.unit, Some(&self.rules), None, effects);
                self.sim
                    .assign_target_represented(self.unit, None, Some(&self.rules), None, effects)
                    .unwrap();
            }
            "update_bunker" => crate::sim::docking::bunker_install::update_bunker(
                &mut self.sim,
                &self.rules,
                None,
                self.bunker,
                effects,
            ),
            "seek_bunker" => {
                answer = Some(i32::from(self.sim.foot_enter_tank_bunker(
                    self.unit,
                    &self.rules,
                    None,
                    effects,
                )))
            }
            "prepare_state3_destination" => {
                let at = coord(&step["coordinate"]);
                self.supply_position(at);
                self.sim
                    .substrate
                    .entities
                    .get_mut(self.bunker)
                    .unwrap()
                    .bunker_runtime
                    .as_mut()
                    .unwrap()
                    .set_state_for_test(BunkerState::TrackStep);
                assert!(
                    self.sim
                        .substrate
                        .entities
                        .get_mut(self.unit)
                        .unwrap()
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .store_track_destination(
                            crate::sim::movement::track_process::TrackFamily::Drive,
                            Some(at)
                        )
                );
            }
            other => panic!("unrepresented native operation {other}"),
        }
        if let Some(expected) = step["return_value"].as_i64() {
            assert_eq!(answer, Some(expected as i32), "{} return", step["op"]);
        }
    }
}

#[test]
fn bunker_entry_histories_match_the_original_contact_and_arrival_owners() {
    let golden = crate::test_fixture::json("tools/spatial_oracle/bunker_entry.json");
    assert_eq!(golden["cases"].as_array().unwrap().len(), 8);
    for row in golden["cases"].as_array().unwrap() {
        let name = row["input"]["name"].as_str().unwrap();
        let mut scene = NativeScene::new(&golden["inputs"], row);
        scene.compare(&row["before"], &format!("{name}: admitted prestate"));
        for (index, step) in row["steps"].as_array().unwrap().iter().enumerate() {
            let context = format!("{name}/{index}/{}", step["op"]);
            scene.compare(&step["before"], &format!("{context} before"));
            crate::sim::radio::take_transmit_log();
            scene.step(step);
            scene.compare(&step["after"], &format!("{context} after"));
            let radio: Vec<Value> = crate::sim::radio::take_transmit_log()
                .iter()
                .map(|record| {
                    json!({
                        "sender":scene.name(Some(record.sender_sid)),
                        "recipient":scene.name(Some(record.target_sid)),
                        "message":record.msg, "reply":record.reply.unwrap(),
                    })
                })
                .collect();
            assert_eq!(
                json!(radio),
                step["radio"],
                "{context}: ordered original radio history"
            );
        }
    }
}
