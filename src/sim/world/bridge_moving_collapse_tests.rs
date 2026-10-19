//! Production regression for the post-driver Cell487720 receiver.
//! Move and occupation are ordinary production behavior; damage starts at
//! Map587180, after the separate AreaDamage admission boundary. No supplied
//! head, raw occupation, bridge state, health, timer or RNG state.

use super::*;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::DriveCoord;
use crate::sim::movement::at_coord::AtCoordQuery;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellObjectMember, RawCellKey};
use crate::sim::world::TickLane;
use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

fn hills_battle() -> Option<crate::headless_scenario::HeadlessScenario> {
    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return None;
    };
    let mut launch = crate::headless_scenario::battle_session(
        "Hills.mmx",
        &[(LaunchCountry::America, AiDifficulty::Easy)],
    );
    launch.options.bases = false;
    launch.options.unit_count = 0;
    launch.options.short_game = false;
    Some(crate::headless_scenario::load_session(&retail, "Hills.mmx", 0x0B21_D6E5, launch).unwrap())
}

fn spawn_mtnk(scene: &mut crate::headless_scenario::HeadlessScenario, p: (u16, u16)) -> u64 {
    let owner = scene.sim().session.current_house().unwrap();
    let owner_name = scene.sim().resolve(owner).to_owned();
    let runtime = &mut scene.runtime;
    let id = runtime
        .simulation
        .spawn_object("MTNK", &owner_name, p.0, p.1, 0, &runtime.resources.rules)
        .expect("ordinary retail MTNK Unlimbo on physical Hills terrain");
    runtime
        .simulation
        .resolve_type_handles(&runtime.resources.rules);
    id
}

#[test]
fn retail_hills_ordinary_move_reserves_a_collapsing_deck_cell() {
    let Some(mut scene) = hills_battle() else {
        return;
    };
    let owner = scene.sim().session.current_house().unwrap();
    let id = spawn_mtnk(&mut scene, (75, 75));
    let runtime = &mut scene.runtime;
    let target = (76, 75);
    let cell = runtime
        .simulation
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(target.0, target.1)
        .unwrap();
    assert!(cell.bridge_facts.has_structural_bridge());
    let ground = crate::sim::cell_rect::CellRef::Real(cell)
        .center_coordinate()
        .unwrap();
    let probe = DriveCoord {
        x: ground.x,
        y: ground.y,
        z: ground.z + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
    };
    let mut admitted = None;
    for step in 0..256 {
        let commands = if step == 0 {
            vec![CommandEnvelope::new(
                owner,
                scene.sim().session.tick(),
                Command::Move {
                    entity_id: id,
                    target_rx: 80,
                    target_ry: 75,
                },
            )]
        } else {
            Vec::new()
        };
        scene
            .runtime
            .advance_frame(
                &commands,
                crate::headless_scenario::SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let sim = scene.sim();
        let actor = sim.entities().get(id).unwrap();
        let raw = sim
            .substrate
            .raw_cell_occupation
            .bits_at(RawCellKey::Real(target.0, target.1), MovementLayer::Bridge);
        let list = sim.cell_object_list_location(CellObjectMember::Entity(id));
        let matches = AtCoordQuery::from_entity(actor).is_some_and(|q| q.matches(probe));
        if step < 8 || raw != 0 {
            eprintln!(
                "step{} pos{:?} list{list:?} raw{raw:x} matches{matches} head{:?}",
                step + 1,
                (actor.position.rx(), actor.position.ry()),
                AtCoordQuery::from_entity(actor).map(|q| q.cells())
            );
        }
        if raw != 0 && matches && list.is_some_and(|(cell, _)| cell != target) {
            admitted = Some(step + 1);
            break;
        }
    }
    let step = admitted
        .expect("ordinary Move must produce neighboring-list/deck-claim/IsAtCoord admission");
    eprintln!("admitted moving collapse boundary at step{step}, actor{id}, probe{probe:?}");
    let runtime = &mut scene.runtime;
    for hit in 0..8 {
        let mut drivers = BridgeDamageDrivers::new(
            &mut runtime.simulation,
            &runtime.resources.rules,
            Some(&runtime.resources.overlay_registry),
            FrameEffects::default(),
        );
        let returned = drivers.apply_damage_to_cell((target.0 as i16, target.1 as i16));
        let sim = &runtime.simulation;
        let structural = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(target.0, target.1)
            .unwrap()
            .bridge_facts
            .has_structural_bridge();
        let raw = sim
            .substrate
            .raw_cell_occupation
            .bits_at(RawCellKey::Real(target.0, target.1), MovementLayer::Bridge);
        let actor = sim.entities().get(id);
        let matches = actor
            .and_then(AtCoordQuery::from_entity)
            .is_some_and(|q| q.matches(probe));
        eprintln!(
            "hit{hit} returned{returned} structural{structural} raw{raw:x} matches{matches} actor{:?}",
            actor.map(|e| (e.health.current(), e.lifecycle.object_alive()))
        );
        if !structural {
            if actor.is_some_and(|e| e.lifecycle.object_alive() && e.health.current() > 0) {
                assert!(
                    raw != 0 && matches,
                    "surviving actor must still meet post-driver487720 admission before claiming a missing kill"
                );
            }
            assert!(
                actor.is_none_or(|e| !e.lifecycle.object_alive() || e.health.current() <= 0),
                "native487720 must send currentHP/C4/ignore-defenses through the neighboring mover's damage receiver"
            );
            assert_eq!(
                raw, 0,
                "fatal receiver clears the committed deck reservation"
            );
            assert!(
                sim.cell_object_list_location(CellObjectMember::Entity(id))
                    .is_none()
            );
            assert_eq!(
                sim.pending_bridge_collapses.at(0),
                None,
                "Map587180 final clear"
            );
            for _ in 0..64 {
                if scene.sim().entities().get(id).is_none() {
                    break;
                }
                scene.tick();
            }
            assert!(
                scene.sim().entities().get(id).is_none(),
                "ordinary deferred deletion retires the victim"
            );
            return;
        }
    }
    panic!("physical bridge did not collapse through Map587180");
}

#[test]
fn retail_hills_empty_claim_nonmatching_head_and_stale_pending_controls() {
    let Some(mut scene) = hills_battle() else {
        return;
    };
    let deck = spawn_mtnk(&mut scene, (75, 75));
    let runtime = &mut scene.runtime;
    let before = runtime.simulation.state_hash();
    moving_collapse::apply(
        &mut runtime.simulation,
        &runtime.resources.rules,
        Some(&runtime.resources.overlay_registry),
        (76, 75),
        FrameEffects::default(),
    );
    assert_eq!(
        runtime.simulation.state_hash(),
        before,
        "empty deck byte returns without effects"
    );

    // The public spawn helper uses ground-height admission, which cannot
    //place this tank directly on the deck. Reach it through ordinary Move.
    let owner = scene.sim().session.current_house().unwrap();
    let command = CommandEnvelope::new(
        owner,
        scene.sim().session.tick(),
        Command::Move {
            entity_id: deck,
            target_rx: 76,
            target_ry: 75,
        },
    );
    scene
        .runtime
        .advance_frame(
            &[command],
            crate::headless_scenario::SIM_TICK_MS,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    let mut arrived = false;
    for _ in 0..256 {
        let e = scene.sim().entities().get(deck).unwrap();
        if (e.position.rx(), e.position.ry()) == (76, 75)
            && e.on_bridge()
            && crate::sim::movement::motion_query::is_moving(e) == Some(false)
        {
            arrived = true;
            break;
        }
        scene.tick();
    }
    assert!(
        arrived,
        "ordinary Move must admit the selected-cell control"
    );
    let bank = spawn_mtnk(&mut scene, (75, 75));
    let runtime = &mut scene.runtime;
    assert_ne!(
        runtime
            .simulation
            .substrate
            .raw_cell_occupation
            .bits_at(RawCellKey::Real(76, 75), MovementLayer::Bridge),
        0
    );
    let before = runtime.simulation.state_hash();
    moving_collapse::apply(
        &mut runtime.simulation,
        &runtime.resources.rules,
        Some(&runtime.resources.overlay_registry),
        (76, 75),
        FrameEffects::default(),
    );
    assert_eq!(
        runtime.simulation.state_hash(),
        before,
        "selected-cell members are skipped; neighboring stationary Foot does not target the deck"
    );
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(bank)
            .unwrap()
            .health
            .current(),
        300
    );
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(deck)
            .unwrap()
            .health
            .current(),
        300
    );

    // A native-save pending vector is independent of the next requested cell.
    // Even no-driver587180 clears it before considering these coordinates.
    runtime.simulation.pending_bridge_collapses.append((76, 75));
    runtime.simulation.pending_bridge_collapses.append((76, 75));
    let mut driver = BridgeDamageDrivers::new(
        &mut runtime.simulation,
        &runtime.resources.rules,
        Some(&runtime.resources.overlay_registry),
        FrameEffects::default(),
    );
    let no_driver = (90, 44);
    let cell = driver
        .sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity(no_driver);
    assert!(damage_dispatch::select_driver(&mut LiveCells::new(driver.sim), cell).is_none());
    assert!(!driver.apply_damage_to_cell(no_driver));
    assert_eq!(runtime.simulation.pending_bridge_collapses.at(0), None);
    assert_eq!(
        runtime.simulation.state_hash(),
        before,
        "no-driver clear does not dispatch stale entries"
    );
}

#[test]
fn retail_hills_tanya_charge_collapses_under_an_ordinary_incoming_tank() {
    let Some(mut scene) = hills_battle() else {
        return;
    };
    let owner = scene.sim().session.current_house().unwrap();
    let owner_name = scene.sim().resolve(owner).to_owned();
    let victim = spawn_mtnk(&mut scene, (75, 75));
    let runtime = &mut scene.runtime;
    assert!(runtime.resources.rules.object("TANY").unwrap().c4);
    assert_eq!(runtime.resources.rules.c4_delay_frames(), 26);
    let tanya = runtime
        .simulation
        .spawn_object("TANY", &owner_name, 74, 75, 0, &runtime.resources.rules)
        .expect("ordinary TANY Unlimbo on the physical bank");
    runtime
        .simulation
        .resolve_type_handles(&runtime.resources.rules);
    let hut = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| {
            (runtime.simulation.resolve(e.type_ref()) == "CABHUT"
                && (e.position.rx(), e.position.ry()) == (73, 73))
                .then_some(id)
        })
        .expect("actual physical Hills CABHUT73,73");
    let (anchor, _) = ramp_repair::hut_anchor(&mut LiveCells::new(&runtime.simulation), (73, 73))
        .unwrap()
        .expect("native hut starter admission");
    assert_eq!(anchor, (76, 75));
    let health = runtime
        .simulation
        .entities()
        .get(hut)
        .unwrap()
        .health
        .current();
    let cell = runtime
        .simulation
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(76, 75)
        .unwrap();
    let ground = crate::sim::cell_rect::CellRef::Real(cell)
        .center_coordinate()
        .unwrap();
    let probe = DriveCoord {
        x: ground.x,
        y: ground.y,
        z: ground.z + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
    };
    let mut stopped = false;
    let mut ordered = None;
    let mut observed_claim = None;
    for step in 0..512 {
        let mut commands = Vec::new();
        let tick = scene.sim().session.tick();
        if step == 0 {
            commands.push(CommandEnvelope::new(
                owner,
                tick,
                Command::PlantC4 {
                    attacker_id: tanya,
                    target_building_id: hut,
                },
            ));
        }
        let pending = scene
            .sim()
            .entities()
            .get(hut)
            .unwrap()
            .pending_c4_detonation
            .pending();
        if let Some(charge) = pending {
            let remaining = charge
                .timer()
                .remaining(scene.sim().session.binary_frame() as i32);
            if !stopped {
                eprintln!("TANY actual plant atstep{step}, remaining{remaining}");
                commands.push(CommandEnvelope::new(
                    owner,
                    tick,
                    Command::Stop { entity_id: tanya },
                ));
                stopped = true;
            }
            // Ordinary input timed against the observed real fuse. The tank
            //turns naturally; no heading, head, timer or raw plane is assigned.
            if ordered.is_none() && remaining <= 15 {
                commands.push(CommandEnvelope::new(
                    owner,
                    tick,
                    Command::Move {
                        entity_id: victim,
                        target_rx: 80,
                        target_ry: 75,
                    },
                ));
                ordered = Some(step);
                eprintln!("ordinary victimMove step{step} remaining{remaining}");
            }
        }
        scene
            .runtime
            .advance_frame(
                &commands,
                crate::headless_scenario::SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let sim = scene.sim();
        let structural = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(76, 75)
            .unwrap()
            .bridge_facts
            .has_structural_bridge();
        let raw = sim
            .substrate
            .raw_cell_occupation
            .bits_at(RawCellKey::Real(76, 75), MovementLayer::Bridge);
        let actor = sim.entities().get(victim);
        let at = actor
            .and_then(AtCoordQuery::from_entity)
            .is_some_and(|q| q.matches(probe));
        let list = sim.cell_object_list_location(CellObjectMember::Entity(victim));
        if raw != 0 && at && list.is_some_and(|(p, _)| p != (76, 75)) {
            observed_claim = Some(step + 1);
            eprintln!("C4chain claimstep{} raw{raw:x} list{list:?}", step + 1);
        }
        if !structural {
            eprintln!(
                "C4collapse step{} victim{:?} ordered{ordered:?} claim{observed_claim:?}",
                step + 1,
                actor.map(|e| (e.health.current(), e.lifecycle.object_alive()))
            );
            assert!(
                stopped && ordered.is_some() && observed_claim.is_some(),
                "ordinary C4/Move must admit a neighboring live deck claimant before the collapse"
            );
            assert!(actor.is_none_or(|e| e.health.current() <= 0 && !e.lifecycle.object_alive()));
            let hut = sim.entities().get(hut).unwrap();
            assert!(hut.lifecycle.object_alive());
            assert_eq!(
                hut.health.current(),
                health,
                "hut C4 expiry preserves the hut"
            );
            assert!(hut.pending_c4_detonation.pending().is_none());
            assert_eq!(sim.pending_bridge_collapses.at(0), None);
            return;
        }
    }
    panic!("ordinary TANY C4 did not reach the selected bridge collapse");
}

#[test]
fn pending_bridge_cells_retain_order_duplicates_and_snapshot_state() {
    let mut sim = Simulation::with_seed(0);
    let empty_hash = sim.state_hash();
    sim.pending_bridge_collapses.append((76, 75));
    let single_hash = sim.state_hash();
    assert_ne!(single_hash, empty_hash);
    sim.pending_bridge_collapses.append((76, 75));
    sim.pending_bridge_collapses.append((-1, 12));
    assert_ne!(sim.state_hash(), single_hash);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "bridge", 0);
    let saved = crate::sim::snapshot::GameSnapshot::load(&bytes).unwrap();
    assert_eq!(saved.sim.pending_bridge_collapses.at(0), Some((76, 75)));
    assert_eq!(saved.sim.pending_bridge_collapses.at(1), Some((76, 75)));
    assert_eq!(saved.sim.pending_bridge_collapses.at(2), Some((-1, 12)));
    assert_eq!(saved.sim.pending_bridge_collapses.at(3), None);
    sim.pending_bridge_collapses.clear();
    assert_eq!(sim.state_hash(), empty_hash);
}
