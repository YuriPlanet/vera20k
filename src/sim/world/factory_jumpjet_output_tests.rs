//! Paid GAPILE -> two Rocketeers through the ordinary master-frame owner.
//!
//! A `JumpJet=` infantryman leaves ExitObject443C60 with no destination
//! (444CA3 skips the Move/exit arm) and only the HELLO/TETHER link. The idle
//! Scatter sends him up; while he climbs in contact with his barracks,
//! Jumpjet State1 (54BC59..54BCB3) raises his Notify(0x117B) (522A60): he
//! sends literal 8 through the contact, the barracks answers 25/3 and frees
//! its one radio slot for the next product, and he takes the rally or
//! scatters again. Without that release the barracks returns TryLater
//! (4440BC) for every later infantryman.
//!
//! Native behavior established by instruction reading of those bodies; the
//! radio goldens reuse the GI corpus, whose ExitObject link and building
//! literal-8 receiver are the same code. No native execution of a Rocketeer
//! build exists for this file.

use super::Simulation;
use super::factory_infantry_output_tests::{corpus, install_ground, pair_radio};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::movement::jumpjet_movement::jumpjet_flight::{STATE_ASCEND, STATE_HOLD};
use crate::sim::production::ProductionCategory;
use crate::sim::radio;
use serde_json::json;

const TICK_MS: u32 = 67;

#[derive(Default)]
struct Rocketeer {
    id: u64,
    placed_frame: Option<u32>,
    lift_off_frame: Option<u32>,
    release_frame: Option<u32>,
    hold_frame: Option<u32>,
    sound_starts: u32,
    sound_releases: u32,
}

#[test]
fn no_rally_two_paid_rocketeers_release_their_barracks_in_flight() {
    two_paid_rocketeers(None);
}

#[test]
fn rally_two_paid_rocketeers_fly_to_the_rally_after_release() {
    two_paid_rocketeers(Some((10, 18)));
}

fn two_paid_rocketeers(rally: Option<(u16, u16)>) {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let native = corpus();
    let rules = retail.rules;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let terrain_rules = TerrainRules::from_ini(&retail.processed_rules);
    let mut sim = Simulation::with_seed(2);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    install_ground(&mut sim, &rules, &terrain_rules, None);
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(owner), true, 10_000, 10);
    house.difficulty = HouseDifficulty::Hard;
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    sim.session.game_mode_nonzero = true;
    sim.session.game_options.game_speed = 3;
    let spawn = |sim: &mut Simulation, name: &str, rx: u16, ry: u16| {
        sim.spawn_object_at_height_with_overlay_registry(
            name,
            "Americans",
            rx,
            ry,
            0,
            0,
            &rules,
            &registry,
        )
        .unwrap_or_else(|| panic!("retail {name} enters through its shared constructor"))
    };
    let producer = spawn(&mut sim, "GAPILE", 14, 14);
    // Power for full-speed production and the RADAR prerequisite of JUMPJET.
    spawn(&mut sim, "GAPOWR", 14, 11);
    spawn(&mut sim, "GAAIRC", 11, 14);
    sim.advance_tick(&[], Some(&rules), None, Some(&registry), TICK_MS);
    if let Some((rx, ry)) = rally {
        let command = CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::SetRally {
                rx,
                ry,
                producer_ids: vec![producer],
            },
        );
        sim.advance_tick(&[command], Some(&rules), None, Some(&registry), TICK_MS);
    }
    let jumpjet = sim.interner.get("JUMPJET").unwrap();
    let commands = [
        CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::QueueProduction { type_id: jumpjet },
        ),
        CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::QueueProduction { type_id: jumpjet },
        ),
    ];
    radio::take_transmit_log();
    sim.advance_tick(&commands, Some(&rules), None, Some(&registry), TICK_MS);
    let first = sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry)
        .and_then(|view| view.object)
        .and_then(|object| object.entity_id)
        .expect("the first Rocketeer is under construction");
    let mut products = vec![Rocketeer {
        id: first,
        ..Default::default()
    }];
    let golden = &native["products"][0];

    for _ in 0..3_000 {
        let due = sim.take_due_commands();
        let output = sim
            .advance_app_frame(
                &due,
                Some(&rules),
                Some(&registry),
                TICK_MS,
                super::TickLane::Ordinary,
                None,
            )
            .expect("the ordinary app frame completes");
        let frame = sim.session.binary_frame;
        let log = radio::take_transmit_log();
        if let Some(held) = sim
            .production
            .factories
            .view(owner, ProductionCategory::Infantry)
            .and_then(|view| view.object)
            .and_then(|object| object.entity_id)
            && !products.iter().any(|p| p.id == held)
        {
            assert_eq!(products.len(), 1, "exactly two identities are constructed");
            products.push(Rocketeer {
                id: held,
                ..Default::default()
            });
        }
        for event in &output.sound_events {
            match event {
                super::SimSoundEvent::AnimationStarted {
                    anim_id, sound_id, ..
                } if sound_id == "RocketeerMoveLoop" => {
                    if let Some(p) = products.iter_mut().find(|p| p.id == *anim_id) {
                        p.sound_starts += 1;
                    }
                }
                _ => {}
            }
        }
        for (index, product) in products.iter_mut().enumerate() {
            let entity = sim.substrate.entities.get(product.id).unwrap();
            if entity.lifecycle.in_limbo {
                continue;
            }
            let transmissions = pair_radio(&log, producer, product.id);
            let runtime = entity
                .locomotor
                .as_ref()
                .and_then(|l| l.jumpjet_runtime())
                .expect("a Rocketeer flies with the Jumpjet locomotor");
            let context = format!("Rocketeer[{}] frame={frame}", product.id);
            if product.placed_frame.is_none() {
                product.placed_frame = Some(frame);
                assert_eq!(json!(transmissions), golden["delivery_radio"], "{context}");
                assert!(entity.dock_entered_with.is_some(), "{context}: tethered");
                assert_eq!(entity.radio_contacts.slot(0), Some(producer));
                // 444CA3 skips the exit move and the Archive restore, so the
                // Unlimbo idle handoff (Foot4D8472..852A) leaves a rally as
                // the NavCom the Jumpjet already flies to; without one there is
                // no destination at all.
                assert!(
                    entity.archive_target().is_none(),
                    "{context}: Archive consumed or absent"
                );
                assert_eq!(
                    entity.navigation.nav_com,
                    rally.map(|(rx, ry)| NavTargetRef::cell(rx, ry)),
                    "{context}"
                );
                assert_eq!(
                    runtime.moving(),
                    rally.is_some(),
                    "{context}: Move_To with the rally"
                );
                continue;
            }
            if product.lift_off_frame.is_none() && runtime.phase() == STATE_ASCEND {
                product.lift_off_frame = Some(frame);
                assert!(
                    entity.dock_entered_with.is_some(),
                    "{context}: still tethered when he leaves the ground"
                );
            }
            if let Some(start) = transmissions.iter().position(|row| row[2] == 8) {
                assert!(product.release_frame.is_none(), "{context}: one release");
                assert!(
                    product.lift_off_frame.is_some_and(|lift| frame >= lift),
                    "{context}: the release happens in flight"
                );
                assert_eq!(runtime.phase(), STATE_ASCEND, "{context}: during State1");
                assert_eq!(
                    json!(&transmissions[start..]),
                    golden["arrival_radio"],
                    "{context}"
                );
                assert!(entity.radio_contacts.is_empty() && entity.dock_entered_with.is_none());
                assert!(
                    sim.substrate
                        .entities
                        .get(producer)
                        .unwrap()
                        .radio_contacts
                        .is_empty(),
                    "{context}: the barracks slot is free again"
                );
                // No Archive remains, so 4DF0D0 clears the NavCom and the
                // forced null Scatter is refused for a moving, non-Fraidycat
                // owner (51D172); the locomotor keeps its destination.
                assert!(entity.archive_target().is_none(), "{context}");
                assert!(
                    entity.navigation.nav_com.is_none(),
                    "{context}: NavCom cleared"
                );
                assert!(runtime.moving(), "{context}: still flying to his cell");
                product.release_frame = Some(frame);
            }
            // State1 holds for one frame at cruise height before the
            // translate; the hover that counts is over his own destination.
            let destination = runtime.destination();
            let at_destination = (i32::from(entity.position.rx), i32::from(entity.position.ry))
                == (destination.x / 256, destination.y / 256);
            if product.hold_frame.is_none()
                && product.release_frame.is_some()
                && runtime.phase() == STATE_HOLD
                && at_destination
            {
                if let Some((rx, ry)) = rally {
                    // The first takes the rally's air slot; the next finds it
                    // taken and steps to a random neighbour (claim_or_scatter).
                    let here = (entity.position.rx, entity.position.ry);
                    if index == 0 {
                        assert_eq!(here, (rx, ry), "{context}");
                    } else {
                        assert!(
                            here.0.abs_diff(rx) <= 1 && here.1.abs_diff(ry) <= 1,
                            "{context}: beside the taken rally slot, at {here:?}"
                        );
                    }
                }
                product.hold_frame = Some(frame);
            }
        }
        for event in &output.sound_events {
            if let super::SimSoundEvent::ObjectSoundReleased { owner } = event
                && let Some(p) = products.iter_mut().find(|p| p.id == *owner)
                && p.placed_frame.is_some()
            {
                p.sound_releases += 1;
            }
        }
        // The MoveSound countdown lapses three frames into the hover.
        if products.len() == 2
            && products
                .iter()
                .all(|p| p.hold_frame.is_some_and(|hold| frame >= hold + 8))
        {
            break;
        }
    }
    assert_eq!(products.len(), 2);
    for product in &products {
        assert!(
            product.placed_frame.is_some()
                && product.lift_off_frame.is_some()
                && product.release_frame.is_some()
                && product.hold_frame.is_some(),
            "Rocketeer[{}] must be placed, lift off, release and hover: {:?}",
            product.id,
            (
                product.placed_frame,
                product.lift_off_frame,
                product.release_frame,
                product.hold_frame
            )
        );
        assert_eq!(
            (product.sound_starts, product.sound_releases),
            (1, 1),
            "Rocketeer[{}]: one MoveSound draw for the flight, released once he hovers",
            product.id
        );
    }
    assert!(
        products[0].release_frame.unwrap() < products[1].placed_frame.unwrap(),
        "the second Rocketeer exits only once the first freed the barracks"
    );
    let factory = sim
        .production
        .factories
        .view(owner, ProductionCategory::Infantry);
    assert!(factory.is_none_or(|view| view.object.is_none() && view.queue.is_empty()));
}
