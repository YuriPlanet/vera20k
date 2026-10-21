//! Whole native588750 and Building440580 wall continuation, gamemd.
//! Consumes building_death_anims --wall-autofill's original-code controls.
//! The flat map/House are component priors; paid ingress has a separate retail
//! regression. No Factory, native world bootstrap or callback-PC equivalence.

use super::{FrameEffects, PlacementEvidence, Simulation, entry_test_fixture};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::power_system::PowerState;
use crate::sim::rng::SimRng;
use crate::sim::vision::OwnerVisibility;
use crate::sim::world::load_object_lifecycle::LoadOverlayHandle;
use crate::sim::world::substrate::DeferredDeleteTicket;
use serde_json::Value;

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native scalar")).unwrap()
}

fn xy(value: &Value) -> (u16, u16) {
    (
        int(&value[0]).try_into().unwrap(),
        int(&value[1]).try_into().unwrap(),
    )
}

fn stream<'a>(corpus: &'a Value, state: &Value, name: &str) -> &'a str {
    corpus["rng_streams"][state["rng"][name].as_u64().unwrap() as usize]
        .as_str()
        .unwrap()
}

fn restore_rng(sim: &mut Simulation, corpus: &Value, state: &Value) {
    sim.main_rng = SimRng::from_native_state_hex_for_test(stream(corpus, state, "main")).into();
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(stream(corpus, state, "scenario"));
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(stream(corpus, state, "mapgen"));
}

fn assert_rng(sim: &Simulation, corpus: &Value, state: &Value, label: &str) {
    for (name, actual) in [
        ("main", sim.main_rng.native_state_hex()),
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        assert!(
            actual == stream(corpus, state, name),
            "{label}: full {name}"
        );
    }
}

struct Scene {
    sim: Simulation,
    rules: RuleSet,
    registry: OverlayTypeRegistry,
    owner: InternedId,
    wall_type: InternedId,
}

impl Scene {
    fn new(control: &Value) -> Option<Self> {
        let retail = if control["input"]["custom_middle_land"].is_null() {
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")?
        } else {
            // Exactly the declared native674000 control, through the normal
            // layered reader. This is not an ordinary stock-map occurrence.
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
                "Hills.mmx",
                &IniFile::from_str("[Rough]\nBuildable=yes\nTrack=0%\n"),
            )?
        };
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        // Reuse the existing flat-map/graph owner only. Actual behavior/type
        // values below come from retail startup -> Battle -> Hills and fixed ART.
        let (mut sim, _, _) = entry_test_fixture::fixture();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let bounds = &control["input"]["map"]["coordinate_bounds"];
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
                int(&control["input"]["map"]["size"][0]),
                int(&bounds[0]),
                int(&bounds[1]),
                int(&bounds[2]),
                int(&bounds[3]),
            ),
        );
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        terrain.set_land_speed_rules_for_test(&rules.terrain_rules);
        let clear = rules.terrain_rules.semantics_for_land_type(0).unwrap();
        for cell in terrain.cells_mut_for_test() {
            cell.speed_costs = clear.speed_costs;
            cell.base_speed_costs = clear.speed_costs;
        }
        // Native registered Cells0..31; the inherited graph has33x33 samples.
        let allocated: Vec<_> = (0..32).flat_map(|y| (0..32).map(move |x| (x, y))).collect();
        terrain.test_set_native_allocated_cells(&allocated);
        let owner = sim.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, Some(owner), true, 0, 10));
        sim.power_states
            .map_mut_for_test()
            .insert(owner, PowerState::default());
        sim.session.set_current_house_for_test(Some(owner));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                int(&control["input"]["game_mode"]) != 0,
            ),
        );
        sim.fog.set_width_for_test(sim.session.map_width());
        sim.fog.set_height_for_test(sim.session.map_height());
        // Native repair_target_prior supplies each registered Cell+12C=18
        // and +E4=0. Map5656D0 reads bit8 as visible. Exploration-only
        // reveal_all_for_owner does not publish current sight; this existing
        // fixture adapter publishes18/counter0 without creating a sight source.
        let mut visibility = OwnerVisibility::new(sim.fog.width(), sim.fog.height());
        for &(x, y) in &allocated {
            visibility.mark_visible(x, y);
        }
        sim.fog.by_owner_mut_for_test().insert(owner, visibility);
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(3));
        let wall_type = sim.interner.get("GAWALL").unwrap();
        Some(Self {
            sim,
            rules,
            registry,
            owner,
            wall_type,
        })
    }

    fn construct(&mut self, coord: (u16, u16)) -> u64 {
        self.sim
            .construct_object_limbo_at_height(
                "GAWALL",
                "Americans",
                coord.0,
                coord.1,
                0,
                0,
                &self.rules,
                FrameEffects::default(),
            )
            .unwrap()
    }

    fn reveal(&mut self, id: u64, coord: (u16, u16)) -> Option<u64> {
        self.sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                DriveCoord {
                    x: i32::from(coord.0) * 256 + 128,
                    y: i32::from(coord.1) * 256 + 128,
                    z: 0,
                },
                0,
                PlacementEvidence::EvaluateMark,
                &self.rules,
                Some(&self.registry),
                FrameEffects::default(),
            )
    }

    fn drain(&mut self) {
        self.sim.process_pending_delete_with(
            &self.rules,
            Some(&self.registry),
            FrameEffects::default(),
            1,
            true,
        );
    }

    fn assert_boundary(&self, corpus: &Value, control: &Value, state: &Value, label: &str) {
        assert_rng(&self.sim, corpus, state, label);
        assert_eq!(
            self.sim.native_identity_cursor(),
            Some(int(&state["cursor"]) as u32),
            "{label}: cursor"
        );
        let ids = &control["native_ids"];
        let expected_ids = |name: &str| -> Vec<i32> {
            state["registries"][name]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|value| ids.get(value.as_str().unwrap()))
                .map(int)
                .collect()
        };
        let actual_buildings: Vec<_> = self
            .sim
            .entities()
            .values()
            .filter(|entity| entity.type_ref() == self.wall_type)
            .collect();
        assert_eq!(
            actual_buildings
                .iter()
                .map(|e| e.native_unique_id())
                .collect::<Vec<_>>(),
            expected_ids("building"),
            "{label}: ordered Building registry"
        );
        let pending: Vec<_> = self
            .sim
            .substrate
            .pending_delete
            .retained_tickets()
            .map(|ticket| match ticket {
                DeferredDeleteTicket::Object(id) => {
                    self.sim.entities().get(id).unwrap().native_unique_id()
                }
                DeferredDeleteTicket::Overlay(id) => {
                    let actual = self
                        .sim
                        .load_objects
                        .snapshot(
                            LoadOverlayHandle::from_stable_id(id),
                            &self.sim.substrate.pending_delete,
                        )
                        .unwrap();
                    let native_id = actual.native_id.unwrap();
                    let native = state["objects"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|index| &corpus["object_states"][index.as_u64().unwrap() as usize])
                        .find(|object| int(&object["id"]) == native_id)
                        .unwrap();
                    assert_eq!(
                        actual.world,
                        [
                            int(&native["location"][0]),
                            int(&native["location"][1]),
                            int(&native["location"][2])
                        ]
                    );
                    assert_eq!(u8::from(actual.alive), int(&native["alive"]) as u8);
                    assert_eq!(u8::from(actual.limbo), int(&native["limbo"]) as u8);
                    assert!(actual.queued);
                    assert!(!actual.on_map);
                    native_id
                }
                other => panic!("unexpected component ticket {other:?}"),
            })
            .collect();
        assert_eq!(
            pending,
            expected_ids("pending"),
            "{label}: Overlay/Building enqueue order"
        );
        assert_eq!(
            self.sim.load_objects.object_count(),
            expected_ids("overlay").len(),
            "{label}: Overlay registry"
        );
        assert_eq!(
            self.sim.load_objects.registry_counts(),
            [expected_ids("overlay").len(); 5]
        );
        let logic: Vec<_> = self
            .sim
            .logic_order()
            .iter()
            .filter_map(|id| self.sim.entities().get(*id))
            .filter(|e| e.type_ref() == self.wall_type)
            .map(|e| e.native_unique_id())
            .collect();
        assert_eq!(
            logic,
            expected_ids("logic"),
            "{label}: no temporary Building Logic admission"
        );
        let origin_cursor = int(&control["transitions"]["origin_unlimbo"]["before"]["cursor"]);
        for entity in actual_buildings {
            let native = state["objects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| &corpus["object_states"][index.as_u64().unwrap() as usize])
                .find(|o| int(&o["id"]) == entity.native_unique_id())
                .unwrap();
            let pose = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            assert_eq!(
                [pose.x, pose.y, pose.z],
                [
                    int(&native["location"][0]),
                    int(&native["location"][1]),
                    int(&native["location"][2])
                ],
                "{label}: Location"
            );
            assert_eq!(
                entity.health.current(),
                int(&native["health"]),
                "{label}: health"
            );
            assert_eq!(
                entity.building_power_health_sample(),
                Some(int(&native["sampled_health"]))
            );
            assert_eq!(
                u8::from(entity.lifecycle.object_alive()),
                int(&native["alive"]) as u8
            );
            assert_eq!(
                u8::from(entity.lifecycle.in_limbo()),
                int(&native["limbo"]) as u8
            );
            assert_eq!(
                u8::from(entity.lifecycle.cell_marked()),
                int(&native["marked"]) as u8
            );
            assert_eq!(
                [
                    entity.passive_scan.timer().start_frame() as i32,
                    entity.passive_scan.timer().duration() as i32
                ],
                [
                    int(&native["passive_scan_timer"][0]),
                    int(&native["passive_scan_timer"][2])
                ]
            );
            assert_eq!(
                entity.radio_contacts.capacity(),
                int(&native["radio_capacity"]) as usize
            );
            assert!(entity.radio_contacts.is_empty());
            // Setup/origin ctor words precede the imported RNG boundary.
            // Only newly constructed filler words are numerical outputs here.
            if entity.native_unique_id() > origin_cursor {
                assert_eq!(
                    entity.techno_ctor_random_word(),
                    int(&native["constructor_ax"]) as u16,
                    "{label}: original constructor AX"
                );
            }
        }
        let grid = self.sim.overlay_grid.as_ref().unwrap();
        let terrain = self.sim.resolved_terrain.as_ref().unwrap();
        let field = |name: &str| {
            corpus["patch_fields"]
                .as_array()
                .unwrap()
                .iter()
                .position(|field| field == name)
                .unwrap()
        };
        for (coord, index) in control["patch_coordinates"]
            .as_array()
            .unwrap()
            .iter()
            .zip(state["patch"].as_array().unwrap())
        {
            let (x, y) = xy(coord);
            let row = &corpus["patch_values"][index.as_u64().unwrap() as usize];
            let expected = |name: &str| int(&row[field(name)]);
            let actual = grid.cell(x, y);
            assert_eq!(
                actual.overlay_id.map_or(-1, i32::from),
                expected("overlay"),
                "{label}: {coord} material"
            );
            assert_eq!(
                i32::from(actual.overlay_data),
                expected("data"),
                "{label}: {coord} data"
            );
            if actual.overlay_id.is_some() {
                assert_eq!(
                    actual.wall_owner,
                    Some(self.owner),
                    "{label}: native House0 owner"
                );
                assert_eq!(expected("owner"), 0);
            }
            assert_eq!(
                i32::from(
                    grid.retained_neighbor_counts()
                        [usize::from(y) * usize::from(grid.width()) + usize::from(x)]
                ),
                expected("retained_neighbor"),
                "{label}: {coord} retained122"
            );
            let cell = terrain.cell(x, y).unwrap();
            assert_eq!(
                (
                    cell.final_tile_index as i32,
                    i32::from(cell.level),
                    i32::from(cell.slope_type),
                    i32::from(cell.land_type)
                ),
                (
                    expected("tile"),
                    expected("level"),
                    expected("slope"),
                    expected("land")
                )
            );
            assert_eq!(
                i32::from(self.sim.substrate.raw_cell_occupation.ground_bits(x, y)),
                expected("ground_raw")
            );
            assert_eq!(
                i32::from(self.sim.substrate.raw_cell_occupation.deck_bits(x, y)),
                expected("deck_raw")
            );
            assert!(
                self.sim.fog.is_cell_visible(self.owner, x, y),
                "{label}: warm native sight remains visible"
            );
        }
        let house = &self.sim.houses[&self.owner];
        assert_eq!(house.economy().credits(), int(&state["house"]["credits"]));
        assert_eq!(
            house.tracking().buildings(),
            int(&state["house"]["building_count"])
        );
        assert_eq!(
            house
                .tracking()
                .owned_count(EntityCategory::Structure, self.wall_type),
            0
        );
        assert_eq!(
            house
                .tracking()
                .active_count(EntityCategory::Structure, self.wall_type),
            0
        );
        let power = &self.sim.power_states[&self.owner];
        assert_eq!(
            [
                u8::from(power.power_recheck_for_test()),
                u8::from(power.radar_recheck_for_test())
            ],
            [
                int(&state["house"]["dirty"][0]) as u8,
                int(&state["house"]["dirty"][1]) as u8
            ]
        );
    }
}

#[test]
fn native_whole_wall_autofill_constructs_retires_and_keeps_the_refused_middle() {
    let corpus =
        crate::test_fixture::json("tools/spatial_oracle/building_death_anims_wall_autofill.json");
    for control in corpus["controls"].as_array().unwrap() {
        assert!(control["observation_gaps"].as_array().unwrap().is_empty());
        assert_eq!(
            control["input"]["wall_owner_globals"]["0x008333BC"],
            u32::MAX
        );
        assert_eq!(control["input"]["scenario_init_priority"], 0);
        let Some(mut scene) = Scene::new(control) else {
            return;
        };
        let wall = scene.rules.object("GAWALL").unwrap();
        assert_eq!(wall.strength, int(&corpus["type"]["strength"]));
        assert_eq!(wall.sight, int(&corpus["type"]["sight"]));
        assert_eq!(wall.insignificant, corpus["type"]["insignificant"] != 0);
        assert_eq!(
            scene.registry.id_for_name("GAWALL").map(i32::from),
            Some(int(&corpus["type"]["overlay_index"]))
        );
        for endpoint in control["input"]["endpoints"].as_array().unwrap() {
            let coord = xy(endpoint);
            let id = scene.construct(coord);
            assert_eq!(scene.reveal(id, coord), Some(id));
        }
        scene.drain();
        let origin = xy(&control["input"]["origin"]);
        let id = scene.construct(origin);
        let native = &control["transitions"]["origin_unlimbo"];
        // Import the exact already-constructed input, never spend its draw twice.
        restore_rng(&mut scene.sim, &corpus, &native["before"]);
        scene.assert_boundary(&corpus, control, &native["before"], "origin before");
        assert_eq!(scene.reveal(id, origin), Some(id));
        scene.assert_boundary(&corpus, control, &native["after"], "origin after");
        if !control["input"]["custom_middle_land"].is_null() {
            let prior = &control["input"]["custom_middle_land"];
            let (x, y) = xy(&prior["coord"]);
            let land = int(&prior["land"]) as u8;
            let semantics = scene
                .rules
                .terrain_rules
                .semantics_for_land_type(land)
                .unwrap();
            let cell = scene
                .sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(x, y)
                .unwrap();
            cell.land_type = land;
            cell.speed_costs = semantics.speed_costs;
        }
        scene.assert_boundary(
            &corpus,
            control,
            &control["transitions"]["autofill"]["before"],
            "autofill before",
        );
        let object_type = scene.rules.object("GAWALL").unwrap();
        crate::sim::production::autofill_walls(
            &mut scene.sim,
            &scene.rules,
            &scene.registry,
            object_type,
            origin,
            scene.owner,
            FrameEffects::default(),
        );
        scene.assert_boundary(
            &corpus,
            control,
            &control["transitions"]["autofill"]["after"],
            "autofill after",
        );
        scene.drain();
        scene.assert_boundary(
            &corpus,
            control,
            &control["transitions"]["drain"]["after"],
            "drain after",
        );
    }
}

/// Original5FD270 passes the requested XYZ to Object5F4EC0;
/// OverlayType5FEA30 (vt7EF600+6C) copies all three words unchanged.
/// This is an instruction-established behavior check, not a native golden.
#[test]
fn wall_unlimbo_keeps_exact_overlay_request_and_constructor_building_location() {
    let corpus =
        crate::test_fixture::json("tools/spatial_oracle/building_death_anims_wall_autofill.json");
    let Some(mut scene) = Scene::new(&corpus["controls"][0]) else {
        return;
    };
    let origin = (20, 14);
    let id = scene.construct(origin);
    let requested = DriveCoord {
        x: 20 * 256 + 32,
        y: 14 * 256 + 200,
        z: 37,
    };
    assert_eq!(
        scene
            .sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                requested,
                0,
                PlacementEvidence::EvaluateMark,
                &scene.rules,
                Some(&scene.registry),
                FrameEffects::default(),
            ),
        Some(id)
    );
    let overlays: Vec<_> = scene.sim.load_objects.retained_overlay_ids().collect();
    assert_eq!(overlays.len(), 1);
    let overlay = scene
        .sim
        .load_objects
        .snapshot(
            LoadOverlayHandle::from_stable_id(overlays[0]),
            &scene.sim.substrate.pending_delete,
        )
        .unwrap();
    assert_eq!(overlay.world, [requested.x, requested.y, requested.z]);
    assert_eq!(overlay.cell, origin);
    assert!(overlay.queued && !overlay.alive && overlay.limbo);
    let building = scene.sim.entities().get(id).unwrap();
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&building.position),
        DriveCoord { x: 0, y: 0, z: 0 }
    );
    assert!(!building.lifecycle.object_alive() && building.lifecycle.in_limbo());
}
