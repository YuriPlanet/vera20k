//! Physical stock entry/fire/Temporal histories. Clear terrain, human roster,
//! visibility and semantic pre-Logic delivery are supplied boundaries. The
//! second history adds legal authored Tags, not an original played-map golden.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::house_state::{HouseState, MatchStatistics};
use crate::sim::mission::MissionType;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{LifecycleTestEvent, SimFrameOutput, SnapshotProcessState, TickLane};

struct Scene(SimRuntime);

impl Scene {
    fn new(short_game: bool, map: Option<&MapFile>) -> Option<Self> {
        let mut retail =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP29U2.MAP")?;
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

        let mut sim = Simulation::with_seed(0x71a9c4);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.install_overlay_grid_for_load(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        for name in ["Americans", "Russians"] {
            let owner = sim.interner.intern(name);
            let mut house = HouseState::new(
                owner,
                rules.country_side_index_raw(name).unwrap(),
                Some(owner),
                true,
                0,
                10,
            );
            house.initialize_scenario_name(name);
            house.set_current_iq_for_test(0);
            sim.houses.register(house);
        }
        sim.session
            .set_current_house_for_test(sim.interner.get("Americans"));
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.session
            .set_map_name_for_test("Temporal Cargo clear arena".into());
        sim.session.game_options_mut_for_test().short_game = short_game;
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        for name in ["Americans", "Russians"] {
            sim.fog
                .reveal_all_for_owner(sim.interner.get(name).unwrap());
        }
        if let Some(map) = map {
            let teams = TeamAiIniRegistry::load_retail(&assets, &map.ini, true).unwrap();
            sim.initialize_trigger_definitions(map, &teams, None, None);
            let terrain = sim.resolved_terrain().unwrap().clone();
            assert_eq!(
                sim.spawn_from_map_with_resolved_and_overlay_registry(
                    &map.entities,
                    &rules,
                    Some(&terrain),
                    Some(&registry)
                ),
                map.entities.len() as u32
            );
            sim.classify_map_triggers(&rules);
        }
        let terrain_template = sim.resolved_terrain().cloned();
        Some(Self(SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template,
            },
        }))
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
            .expect("stock constructor and real Unlimbo")
    }

    fn actor(&self, id: u64) -> &GameEntity {
        self.0.simulation.entities().get(id).expect("stored actor")
    }

    fn stats(&self, owner: &str) -> MatchStatistics {
        let sim = &self.0.simulation;
        *sim.houses[&sim.interner.get(owner).unwrap()].stats()
    }

    fn frame(&mut self) -> SimFrameOutput {
        let due = self.0.simulation.take_due_commands();
        let output = self
            .0
            .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(
            output.network_failure.is_none() && !output.network_waiting && !output.outcome_waiting
        );
        assert!(output.tick.frame_committed || output.tick.terminal_score_finalized);
        output
    }

    fn until(&mut self, frames: usize, done: impl Fn(&Self) -> bool) {
        for _ in 0..frames {
            if done(self) {
                return;
            }
            self.frame();
        }
        assert!(done(self), "ordinary history did not reach its boundary");
    }

    fn enter(&mut self, child: u64, reactor: u64) {
        assert!(self.0.apply_command_for_test(
            "Russians",
            &Command::clicked_mission(child, MissionType::Enter, TargetKind::Entity(reactor))
        ));
        self.until(900, |scene| {
            scene.actor(child).passenger_role.inside_transport_id() == Some(reactor)
        });
        let child = self.actor(child);
        assert!(child.lifecycle.in_limbo());
        assert!(!child.in_logic_vector.is_member());
    }

    fn cargo(&self, reactor: u64) -> &[u64] {
        self.actor(reactor)
            .passenger_role
            .cargo()
            .unwrap()
            .passengers()
    }

    fn tracked_initiates(&self) -> (i32, i32) {
        let sim = &self.0.simulation;
        let tracking = sim.houses[&sim.interner.get("Russians").unwrap()].tracking();
        (
            tracking.owned_count(EntityCategory::Infantry, sim.interner.get("INIT").unwrap()),
            tracking.infantry(),
        )
    }

    fn remaining(&self, cleg: u64) -> i32 {
        self.actor(cleg)
            .temporal
            .link
            .as_ref()
            .expect("actual Temporal owner")
            .warp_remaining
    }

    fn start_warp(&mut self, reactor: u64) -> u64 {
        let cleg = self.spawn("CLEG", "Americans", (12, 15));
        assert!(self.0.apply_command_for_test(
            "Americans",
            &Command::Attack {
                attacker_id: cleg,
                target_id: reactor
            }
        ));
        let mut saw_shot = false;
        for _ in 0..180 {
            let output = self.frame();
            for shot in output
                .fire_events
                .iter()
                .filter(|shot| shot.attacker_id == cleg)
            {
                assert_eq!(shot.target, TargetKind::Entity(reactor));
                assert_eq!(
                    self.0.simulation.interner.resolve(shot.weapon_id),
                    "NeutronRifle"
                );
                saw_shot = true;
            }
            if self.actor(reactor).temporal.chain_head() == Some(cleg) {
                assert!(saw_shot, "real FireAt must precede the active chain");
                assert_eq!(self.actor(cleg).temporal.warp_target(), Some(reactor));
                assert!(self.remaining(cleg) > 0);
                return cleg;
            }
        }
        panic!("stock CLEG did not fire and start a real warp");
    }

    fn erase(&mut self, reactor: u64) {
        self.until(1200, |scene| {
            !scene.0.simulation.entities().contains(reactor)
        });
    }

    fn scalar_order(&self, reactor: u64, children: &[u64]) {
        let trace = self.0.simulation.lifecycle_test_events_for_test();
        let parent_kill = trace.iter().position(|event| matches!(event, LifecycleTestEvent::RecordKillBookkeeping { stable_id } if *stable_id == reactor)).unwrap();
        let finalized: Vec<_> = trace
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match event {
                LifecycleTestEvent::FinalizedCommon { stable_id }
                    if children.contains(stable_id) =>
                {
                    Some((index, *stable_id))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            finalized.iter().map(|(_, id)| *id).collect::<Vec<_>>(),
            children
        );
        assert!(
            finalized.iter().all(|(index, _)| *index < parent_kill),
            "each Cargo scalar destructor finishes before parent RecordKill"
        );
        assert!(
            !trace.iter().any(|event| match event {
                LifecycleTestEvent::RecordKillBookkeeping { stable_id }
                | LifecycleTestEvent::UninitClassPre { stable_id }
                | LifecycleTestEvent::PendingDeleteQueued { stable_id } =>
                    children.contains(stable_id),
                _ => false,
            }),
            "scalar disposal must not invent child RecordKill/UnInit/deferred death"
        );
        for &child in children {
            assert!(!self.0.simulation.entities().contains(child));
        }
    }

    fn save_and_restore(&mut self) {
        let live = &self.0.simulation;
        let resources = &self.0.resources;
        let bytes = GameSnapshot::save_validated(
            live,
            0,
            resources.rules.simulation_config_hash(),
            "pending loaded Bio warp",
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
        // Keep the loader's Scenario reseed and preserve process-owned cursors.
        loaded.retain_process_state_for_load(SnapshotProcessState::Live(live));
        loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
        loaded.reconstruct_cellclass_dummy_for_map_resize();
        loaded.restore_after_snapshot_load().unwrap();
        loaded.rebuild_caches_after_load(
            resources.terrain_template.clone().unwrap(),
            live.terrain_speed_config().clone(),
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
}

#[test]
fn stock_bio_reactor_temporal_erase_does_not_book_absorbed_infantry_as_a_loss() {
    let Some(mut scene) = Scene::new(true, None) else {
        return;
    };
    scene.spawn("GAPOWR", "Americans", (4, 24));
    // Its last base remains alive throughout entry and the sustained warp.
    let reactor = scene.spawn("YAPOWR", "Russians", (16, 14));
    let child = scene.spawn("INIT", "Russians", (12, 14));
    scene.enter(child, reactor);
    assert_eq!(scene.cargo(reactor), [child]);
    assert_eq!(scene.tracked_initiates(), (1, 0));
    let before = scene.stats("Russians");
    let attacker_before = scene.stats("Americans");
    let cleg = scene.start_warp(reactor);
    scene.erase(reactor);
    // Temporal71A9C4/71A9D3 is physical scalar disposal; it never calls
    // the absorbed child's RecordKill, unlike ordinary parent UnInit.
    assert_eq!(scene.stats("Russians").units_lost(), before.units_lost());
    assert_eq!(
        scene.stats("Russians").buildings_lost(),
        before.buildings_lost() + 1
    );
    assert_eq!(
        scene.stats("Americans").buildings_killed(),
        attacker_before.buildings_killed() + 1
    );
    assert_eq!(
        scene.stats("Americans").units_killed(),
        attacker_before.units_killed()
    );
    assert_eq!(scene.tracked_initiates(), (0, 0));
    assert!(!scene.actor(cleg).temporal.is_warping_someone());
    scene.scalar_order(reactor, &[child]);

    // Actual House defeat/Savour/outcome and the master-frame finalizer own
    // this terminal row. No forced score, defeated flag or result callback.
    scene.until(300, |scene| {
        scene.0.simulation.terminal_score_snapshot().is_some()
    });
    let sim = &scene.0.simulation;
    let russian = sim.interner.get("Russians").unwrap();
    assert!(sim.houses[&russian].is_defeated());
    let row = sim
        .terminal_score_snapshot()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.owner == russian)
        .unwrap();
    assert_eq!(
        row.losses, 1,
        "the lost reactor is the sole terminal casualty"
    );
}

#[test]
fn pending_reactor_warp_restores_cargo_and_scalar_disposes_without_child_kill_tags() {
    let map = MapFile::from_bytes(b"[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n\
        [Basic]\nNewINIFormat=4\n\
        [IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n\
        [Infantry]\n0=Russians,INIT,256,12,14,0,Guard,0,F48,0\n1=Russians,INIT,256,12,16,0,Guard,0,S29,0\n\
        [Structures]\n0=Russians,YAPOWR,256,16,14,0,P29\n\
        [Triggers]\nFIRST=Russians,<none>,First kill,0,1,1,1,0\nSECOND=Russians,<none>,Second kill,0,1,1,1,0\nPARENT=Russians,<none>,Parent kill,0,1,1,1,0\n\
        [Tags]\nF48=0,First kill,FIRST\nS29=0,Second kill,SECOND\nP29=0,Parent kill,PARENT\n\
        [Events]\nFIRST=1,48,0,0\nSECOND=1,29,0,0\nPARENT=1,29,0,0\n\
        [Actions]\nFIRST=1,28,0,7,0,0,0,0,A\nSECOND=1,28,0,8,0,0,0,0,A\nPARENT=1,28,0,9,0,0,0,0,A\n").unwrap();
    let Some(mut scene) = Scene::new(false, Some(&map)) else {
        return;
    };
    // Only map sections/Tags are projected; the clear arena is supplied.
    let at = |kind: &str, cell: (u16, u16)| {
        scene
            .0
            .simulation
            .entities()
            .values()
            .find(|actor| {
                scene.0.simulation.interner.resolve(actor.type_ref()) == kind
                    && (actor.position.rx(), actor.position.ry()) == cell
            })
            .unwrap()
            .stable_id()
    };
    let (reactor, first, second) = (
        at("YAPOWR", (16, 14)),
        at("INIT", (12, 14)),
        at("INIT", (12, 16)),
    );
    scene.spawn("GAPOWR", "Americans", (4, 24));
    scene.spawn("NAPOWR", "Russians", (28, 28));
    for id in [reactor, first, second] {
        assert!(scene.0.simulation.object_has_trigger_tag(id));
    }
    assert_eq!(&scene.0.simulation.global_variables()[7..10], [false; 3]);
    scene.enter(first, reactor);
    scene.enter(second, reactor);
    assert_eq!(scene.cargo(reactor), [second, first]);
    assert_eq!(scene.tracked_initiates(), (2, 0));
    let cleg = scene.start_warp(reactor);
    let remaining = scene.remaining(cleg);
    scene.frame();
    assert!(remaining > scene.remaining(cleg) && scene.remaining(cleg) > 0);
    let saved_work = scene.actor(cleg).temporal.clone();
    let before = scene.stats("Russians");
    scene.save_and_restore();
    assert_eq!(scene.actor(cleg).temporal, saved_work);
    assert_eq!(scene.actor(reactor).temporal.chain_head(), Some(cleg));
    assert_eq!(scene.cargo(reactor), [second, first]);
    for id in [first, second] {
        assert_eq!(
            scene.actor(id).passenger_role.inside_transport_id(),
            Some(reactor)
        );
        assert!(scene.actor(id).lifecycle.in_limbo());
        assert!(scene.0.simulation.object_has_trigger_tag(id));
    }
    assert!(scene.0.simulation.object_has_trigger_tag(reactor));
    assert_eq!(&scene.0.simulation.global_variables()[7..10], [false; 3]);
    scene.erase(reactor);
    // The real parent29 proves operative definitions/attachments; absorbed
    // scalar children produce neither of their authored48/29 kill effects.
    assert_eq!(
        &scene.0.simulation.global_variables()[7..10],
        [false, false, true]
    );
    assert_eq!(scene.stats("Russians").units_lost(), before.units_lost());
    assert_eq!(
        scene.stats("Russians").buildings_lost(),
        before.buildings_lost() + 1
    );
    assert_eq!(scene.tracked_initiates(), (0, 0));
    scene.scalar_order(reactor, &[second, first]);
    assert!(!scene.actor(cleg).temporal.is_warping_someone());
}
