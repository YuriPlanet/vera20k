//! Physical retail types through registered Scenario rules and real entry,
//! capture, firing and unload owners. The clear arena, human roster, visibility
//! and pre-Logic semantic delivery are supplied inputs, not raw-input goldens.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::{firing_occupant_id, is_occupied};
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;

struct Scene(SimRuntime);

impl Scene {
    fn new(patch: &str) -> Option<Self> {
        let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &IniFile::from_str(patch),
        )?;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
        retail
            .rules
            .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
        retail
            .rules
            .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x522970);
        sim.install_overlay_grid_for_load(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        for name in ["Americans", "Russians"] {
            let owner = sim.interner.get(name).unwrap();
            let house = sim.houses.get_mut(&owner).unwrap();
            house.initialize_scenario_name(name);
            house.set_is_human_for_test(true);
            house.set_player_control_for_test(true);
            house.set_current_iq_for_test(0);
        }
        sim.session
            .set_current_house_for_test(sim.interner.get("Americans"));
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        let options = sim.session.game_options_mut_for_test();
        options.multi_engineer = false;
        options.short_game = false;
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        for name in ["Americans", "Russians"] {
            sim.fog
                .reveal_all_for_owner(sim.interner.get(name).unwrap());
        }
        let terrain_template = sim.resolved_terrain().cloned();
        let mut scene = Self(SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template,
            },
        });
        scene.spawn("GAPOWR", "Americans", (4, 24));
        scene.spawn("NAPOWR", "Russians", (28, 28));
        scene.frame();
        Some(scene)
    }

    fn spawn(&mut self, kind: &str, owner: &str, cell: (u16, u16)) -> u64 {
        self.0
            .simulation
            .spawn_object_with_overlay_registry(
                kind,
                owner,
                cell.0,
                cell.1,
                0,
                &self.0.resources.rules,
                &self.0.resources.overlay_registry,
            )
            .expect("physical retail constructor and ordinary Unlimbo")
    }

    fn actor(&self, id: u64) -> &GameEntity {
        self.0.simulation.entities().get(id).expect("stored actor")
    }

    fn command(&mut self, owner: &str, command: Command) {
        assert!(self.0.apply_command_for_test(owner, &command));
    }

    fn frame(&mut self) -> SimFrameOutput {
        let commands = self.0.simulation.take_due_commands();
        let output = self
            .0
            .advance_frame(&commands, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        output
    }

    fn until(&mut self, done: impl Fn(&Self) -> bool) {
        for _ in 0..900 {
            if done(self) {
                return;
            }
            self.frame();
        }
        assert!(
            done(self),
            "ordinary movement/mission history did not finish"
        );
    }

    fn cargo(&self, id: u64) -> &[u64] {
        self.actor(id).passenger_role.cargo().unwrap().passengers()
    }

    fn enter(&mut self, kind: &str, owner: &str, building: u64, capture: bool) -> u64 {
        let id = self.spawn(kind, owner, (12, 14));
        self.command(
            owner,
            if capture {
                Command::CaptureBuilding {
                    engineer_id: id,
                    target_building_id: building,
                }
            } else {
                Command::clicked_mission(id, MissionType::Enter, TargetKind::Entity(building))
            },
        );
        self.until(|scene| scene.actor(id).passenger_role.inside_transport_id() == Some(building));
        assert!(self.actor(id).lifecycle.in_limbo());
        id
    }

    fn output(&self, owner: &str) -> i32 {
        let owner = self.0.simulation.interner.get(owner).unwrap();
        self.0.simulation.power_states()[&owner].total_output()
    }

    fn threat(&self, id: u64) -> i32 {
        let sim = &self.0.simulation;
        crate::sim::combat::live_threat_posed(
            self.actor(id),
            sim.interner
                .object_type(self.actor(id).type_ref(), &self.0.resources.rules),
            sim.entities(),
            &self.0.resources.rules,
            &sim.interner,
        )
    }

    fn save_and_restore(&mut self) {
        let live = &self.0.simulation;
        let resources = &self.0.resources;
        let bytes = GameSnapshot::save_validated(
            live,
            0,
            resources.rules.simulation_config_hash(),
            "separate building stores",
            0,
        );
        let mut loaded = GameSnapshot::load_validated(
            &bytes,
            0,
            resources.rules.simulation_config_hash(),
            live.session.map_name(),
        )
        .unwrap()
        .sim;
        assert_eq!(
            loaded.scenario_rng.logical_state(),
            crate::sim::rng::SimRng::new(0).logical_state()
        );
        loaded.retain_process_state_for_load(SnapshotProcessState::Live(live));
        loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
        loaded.reconstruct_cellclass_dummy_for_map_resize();
        loaded.restore_after_snapshot_load().unwrap();
        loaded.rebuild_caches_after_load(
            resources.terrain_template.clone().unwrap(),
            live.terrain_speed_config.clone(),
            &resources.rules,
        );
        loaded
            .restore_map_authority_after_snapshot_load(
                &resources.rules,
                &resources.overlay_registry,
                None,
            )
            .unwrap();
        loaded.resolve_type_handles(&resources.rules);
        loaded.rebuild_lighting_sources_after_load(&resources.rules);
        self.0.replace_simulation(loaded);
    }

    fn shot(&mut self, building: u64, target: u64, expected_weapon: &str) {
        for _ in 0..180 {
            let output = self.frame();
            if let Some(shot) = output
                .fire_events
                .iter()
                .find(|shot| shot.attacker_id == building)
            {
                assert_eq!(shot.target, TargetKind::Entity(target));
                assert_eq!(
                    self.0.simulation.interner.resolve(shot.weapon_id),
                    expected_weapon
                );
                return;
            }
        }
        panic!("occupied building did not fire through ordinary Attack/FireAt");
    }
}

#[test]
fn captured_stock_bio_reactor_retains_absorber_cargo_without_publishing_occupant_threat() {
    let Some(mut scene) = Scene::new("") else {
        return;
    };
    let reactor = scene.spawn("YAPOWR", "Russians", (16, 14));
    let first = scene.enter("INIT", "Russians", reactor, false);
    let second = scene.enter("INIT", "Russians", reactor, false);
    assert_eq!(scene.cargo(reactor), [second, first]);
    scene.until(|scene| scene.output("Russians") == 500);
    let engineer = scene.spawn("ENGINEER", "Americans", (6, 14));
    assert_eq!(
        scene
            .0
            .simulation
            .engineer_building_action(engineer, reactor, &scene.0.resources.rules),
        Some(super::world_orders::EngineerBuildingAction::Capture)
    );
    scene.command(
        "Americans",
        Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: reactor,
        },
    );
    scene.frame();
    assert_eq!(
        scene
            .0
            .simulation
            .interner
            .resolve(scene.actor(reactor).owner()),
        "Russians"
    );
    assert!(
        matches!(scene.actor(engineer).navigation.nav_com(), Some(crate::sim::components::NavTargetRef::Object { id }) if id == reactor)
    );
    // Save the real pending approach, including the already loaded Cargo.
    // Canonical load reseeds Scenario RNG; this is a state/continuation check,
    // not live-v-loaded whole-RNG equivalence.
    scene.save_and_restore();
    scene.until(|scene| {
        scene
            .0
            .simulation
            .interner
            .resolve(scene.actor(reactor).owner())
            == "Americans"
    });
    assert!(
        scene
            .0
            .simulation
            .entities()
            .get(engineer)
            .is_none_or(|actor| !actor.is_object_alive())
    );
    assert_eq!(scene.cargo(reactor), [second, first]);
    scene.until(|scene| scene.output("Americans") == 550 && scene.output("Russians") == 150);
    // GetOccupantCount4581F0 and Threat708B40 read Building+694, not Cargo.
    // ChangeOwner701701..701780 publishes that value to the new enemy map.
    assert_eq!(scene.threat(reactor), 0, "absorber Cargo is not a garrison");
    assert_eq!(scene.actor(reactor).cached_spatial_threat(), Some(0));
    let sim = &scene.0.simulation;
    assert_eq!(
        sim.house_threat_at_cell(sim.interner.get("Russians").unwrap(), (16, 14))
            .unwrap(),
        0
    );
}

#[test]
fn combined_absorber_and_garrison_save_firing_order_then_unload_separate_lists() {
    let Some(mut scene) = Scene::new(
        "[YAPOWR]\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\nCanPassiveAquire=no\n",
    ) else {
        return;
    };
    // Legal registered-map flags deliberately overlap the two native stores;
    // no shipped type supplies this combined profile. Passive acquisition is
    // disabled so the explicit Attack below owns the first firing history.
    let reactor = scene.spawn("YAPOWR", "Americans", (16, 14));
    let first = scene.enter("INIT", "Americans", reactor, false);
    let second = scene.enter("INIT", "Americans", reactor, false);
    let gi = scene.enter("E1", "Americans", reactor, true);
    let conscript = scene.enter("E2", "Americans", reactor, true);
    let object = scene.0.resources.rules.object("YAPOWR").unwrap();
    assert!(is_occupied(scene.actor(reactor), object));
    assert_eq!(
        firing_occupant_id(scene.actor(reactor), object),
        Some(gi),
        "Cargo prepend order must not replace occupant admission order"
    );
    assert_eq!(scene.cargo(reactor), [second, first]);
    assert_eq!(scene.threat(reactor), 20);
    assert_eq!(
        crate::sim::building_art::current_body_frame(
            scene.actor(reactor),
            object,
            &scene.0.resources.rules,
            scene.0.simulation.object_tech_level(object),
        ),
        2,
        "GetCurrentFrame uses occupied body art with both stores populated"
    );
    scene.until(|scene| scene.output("Americans") == 550);
    let gi_weapon = scene
        .0
        .resources
        .rules
        .object("E1")
        .unwrap()
        .occupy_weapon
        .clone()
        .unwrap();
    let conscript_weapon = scene
        .0
        .resources
        .rules
        .object("E2")
        .unwrap()
        .occupy_weapon
        .clone()
        .unwrap();
    assert_ne!(gi_weapon, conscript_weapon);
    let target = scene.spawn("SMCV", "Russians", (21, 14));
    scene.command(
        "Americans",
        Command::Attack {
            attacker_id: reactor,
            target_id: target,
        },
    );
    scene.shot(reactor, target, &gi_weapon);
    assert_eq!(
        firing_occupant_id(
            scene.actor(reactor),
            scene.0.resources.rules.object("YAPOWR").unwrap()
        ),
        Some(conscript)
    );
    // Both lists are nonempty and a real FireAt advanced the saved cursor.
    scene.save_and_restore();
    assert_eq!(scene.cargo(reactor), [second, first]);
    assert_eq!(scene.threat(reactor), 20);
    assert_eq!(
        firing_occupant_id(
            scene.actor(reactor),
            scene.0.resources.rules.object("YAPOWR").unwrap()
        ),
        Some(conscript)
    );
    scene.shot(reactor, target, &conscript_weapon);
    scene.command(
        "Americans",
        Command::UnloadPassengers {
            transport_id: reactor,
        },
    );
    scene.until(|scene| scene.cargo(reactor).len() < 2);
    // Mission_Unload44D89C releases the garrison, then the absorber owner
    // pops exactly one Cargo head. D-key admission is a different route.
    assert_eq!(scene.cargo(reactor), [first]);
    let object = scene.0.resources.rules.object("YAPOWR").unwrap();
    assert_eq!(
        crate::sim::building_art::current_body_frame(
            scene.actor(reactor),
            object,
            &scene.0.resources.rules,
            scene.0.simulation.object_tech_level(object),
        ),
        0,
        "GetCurrentFrame must not draw a Cargo-only reactor as garrisoned"
    );
    assert!(!is_occupied(
        scene.actor(reactor),
        scene.0.resources.rules.object("YAPOWR").unwrap()
    ));
    for id in [gi, conscript, second] {
        assert!(!scene.actor(id).lifecycle.in_limbo());
        assert_eq!(scene.actor(id).passenger_role.inside_transport_id(), None);
    }
    assert_eq!(
        scene.actor(first).passenger_role.inside_transport_id(),
        Some(reactor)
    );
    assert_eq!(scene.threat(reactor), 0);
    assert_eq!(scene.output("Americans"), 450);
    scene.until(|scene| scene.cargo(reactor).is_empty());
    assert!(!scene.actor(first).lifecycle.in_limbo());
    assert_eq!(scene.output("Americans"), 350);
}
