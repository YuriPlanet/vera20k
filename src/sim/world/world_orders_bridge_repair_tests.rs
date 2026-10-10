//! Integration tests for the engineer-bridge-repair flow + the C4-on-CABHUT
//! collapse path. Engineer entry repairs the bridge and consumes the
//! engineer; C4 on the hut leaves the hut at full HP and collapses the
//! bridge segment via the BridgeRepairHut branch in
//! `apply_c4_damage_to_building`.

use super::*;
use crate::map::bridge_facts::{
    BRIDGE_FLAG_ANCHOR_SELF, BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL,
    BridgeAnchorRelation, BridgeStampFamily, BridgeStampSlot,
};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rng_continuation::MapGenRngContinuation;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::command::Command;
use crate::sim::components::{Health, NavTargetRef, PendingC4Detonation};
use crate::sim::game_entity::GameEntity;
use crate::sim::timer::CdTimer;

/// Minimal 20x20 synthetic terrain for command admission and C4 fallback
/// controls. Live ordinary repair uses the shared resident entry fixture.
fn dummy_resolved_terrain() -> ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(20, 20, |rx, ry| ResolvedTerrainCell {
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    })
}

const BRIDGE_REPAIR_TEST_INI: &str = "[InfantryTypes]\n0=ENGI\n1=GHOST\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=CABHUT\n\n\
         [ENGI]\nStrength=75\nArmor=none\nSpeed=4\nPrimary=none\nEngineer=yes\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\n\
         [GHOST]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=none\nC4=yes\n\n\
         [CABHUT]\nStrength=200\nArmor=concrete\nFoundation=1x1\nBridgeRepairHut=yes\n\n\
         [AudioVisual]\nRepairBridgeSound=BridgeRepaired\n\n\
         [CombatDamage]\nC4Warhead=SA\nIvanWarhead=SA\nIvanDamage=1\nIvanTimedDelay=2\n\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";

fn bridge_repair_test_rules() -> RuleSet {
    let ini = IniFile::from_str(BRIDGE_REPAIR_TEST_INI);
    RuleSet::from_ini(&ini).expect("bridge-repair test rules should parse")
}

fn build_sim() -> (Simulation, RuleSet) {
    let mut sim = Simulation::new();
    let rules = bridge_repair_test_rules();
    sim.resolve_type_handles(&rules);
    sim.resolved_terrain = Some(dummy_resolved_terrain());
    (sim, rules)
}

fn build_ordinary_c4_sim(
    overlay: u8,
) -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    use crate::sim::house_state::HouseState;

    // The SEAL walks: a second [GHOST] section would not be read, since a
    // lookup finds the first section of a name.
    let ini = BRIDGE_REPAIR_TEST_INI
        .replace(
            "[GHOST]\n",
            "[GHOST]\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
        )
        .replace(
            "[BuildingTypes]\n0=CABHUT\n",
            "[BuildingTypes]\n0=CABHUT\n1=BIG\n[BIG]\nStrength=800\nArmor=concrete\nFoundation=3x3\n",
        );
    let (mut sim, rules, registry) = super::entry_test_fixture::fixture_with_rules(&ini);
    // A raw ordinary three-cell width, with resident TMP/navigation owners.
    // Overlay families do not imply structural/deck geometry.
    for y in [14, 15, 16] {
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(17, y)
            .unwrap()
            .bridge_facts
            .overlay_id = Some(overlay);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(17, y, overlay, 0);
    }
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain_with_map_size(
        sim.resolved_terrain.as_ref().unwrap(),
        true,
        300,
        (16, 16),
    ));
    for (side, name) in ["Americans", "Soviets"].into_iter().enumerate() {
        let house = sim.interner.intern(name);
        sim.houses.insert(
            house,
            HouseState::new(house, side as u8, None, false, 1000, 10),
        );
        sim.session.house_order.push(house);
    }
    (sim, rules, registry)
}

fn spawn_engineer(sim: &mut Simulation, rules: &RuleSet, rx: u16, ry: u16) -> u64 {
    // The command reaches Infantry51AA40; its receiver must have the actual
    // constructor/Unlimbo Walk and mission leaf, not only an Infantry tag.
    sim.spawn_object_at_height("ENGI", "Americans", rx, ry, 0, 0, rules)
        .expect("constructed Engineer is admitted on the resident clear terrain")
}

fn spawn_seal(sim: &mut Simulation, rx: u16, ry: u16) -> u64 {
    let owner = sim.interner.intern("Americans");
    let ty = sim.interner.intern("GHOST");
    let id = sim.substrate.next_stable_object_id;
    sim.substrate.next_stable_object_id += 1;
    let e = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        owner,
        Health { current: 125 },
        ty,
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(e);
    assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
    id
}

fn spawn_cabhut(sim: &mut Simulation, rx: u16, ry: u16) -> u64 {
    let owner = sim.interner.intern("Soviets");
    let ty = sim.interner.intern("CABHUT");
    let id = sim.substrate.next_stable_object_id;
    sim.substrate.next_stable_object_id += 1;
    let e = GameEntity::new_at_frame_zero_for_test(
        id,
        rx,
        ry,
        0,
        0,
        owner,
        Health { current: 200 },
        ty,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(e);
    assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
    id
}

const BRIDGE_CELLS: &[(u16, u16)] = &[(10, 9), (10, 10), (10, 11), (10, 12), (10, 13)];

/// Collapsed ordinary high identities (CellClass+44) on the span cells.
fn seed_destroyed_bridge(sim: &mut Simulation) {
    let terrain = sim
        .resolved_terrain
        .as_mut()
        .expect("bridge tests require resolved terrain");
    for &(rx, ry) in BRIDGE_CELLS {
        terrain.cell_mut(rx, ry).unwrap().bridge_facts.overlay_id = Some(0xE7);
    }
    sim.bridge_state = Some(BridgeRuntimeState::default());
}

/// Make `cell` the fallback's ramp: a concrete BridgeMiddle1 tile at sub-tile
/// 4, which `MapClass::IsBridgeRampTile` (`0x005746C0`) accepts and
/// ApplyDamageToCell (`0x00587180`) reads to select the High state machine.
fn stamp_concrete_middle_tile(sim: &mut Simulation, cell: (u16, u16)) {
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
    terrain.test_set_high_bridge_rim_tiles(
        crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
            100,
            b"[General]\nBridgeMiddle1=7\nBridgeMiddle2=40\n",
        ),
    );
    let ramp = terrain.cell_mut(cell.0, cell.1).unwrap();
    ramp.final_tile_index = 100 + 7 - 1;
    ramp.final_sub_tile = 4;
}

/// The damaged EW anchor state (+11E = 15) the fallback must leave alone.
const FALLBACK_ANCHOR_STATE: u8 = 15;

fn fallback_anchor_state(sim: &Simulation) -> u8 {
    sim.resolved_terrain
        .as_ref()
        .unwrap()
        .cell(13, 10)
        .unwrap()
        .bridge_facts
        .state_byte
}

fn seed_hut_fallback_bridgehead_layout(sim: &mut Simulation) {
    sim.bridge_state = Some(BridgeRuntimeState::default());
    let terrain = sim
        .resolved_terrain
        .as_mut()
        .expect("bridge fallback tests require resolved terrain");
    let anchor_cell = terrain.native_cell_identity((13, 10));
    let starter = terrain.cell_mut(12, 10).unwrap();
    starter.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
    starter.bridge_facts.native_anchor = Some(anchor_cell);
    starter.bridge_facts.anchor = Some(BridgeAnchorRelation {
        anchor: (13, 10),
        slot: BridgeStampSlot::Forward1,
        family: BridgeStampFamily::Nesw,
        direction: 6,
    });
    let anchor = terrain.cell_mut(13, 10).unwrap();
    anchor.bridge_facts.raw_flags = 0;
    anchor.bridge_facts.state_byte = FALLBACK_ANCHOR_STATE;
    stamp_concrete_middle_tile(sim, (13, 10));
}

fn seed_terminal_overlay_with_fallback_trap(sim: &mut Simulation, overlay_byte: u8) {
    seed_hut_fallback_bridgehead_layout(sim);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(10, 10)
        .unwrap()
        .bridge_facts
        .overlay_id = Some(overlay_byte);
}

fn seed_hut_fallback_without_starter(sim: &mut Simulation) {
    seed_hut_fallback_bridgehead_layout(sim);
    let terrain = sim
        .resolved_terrain
        .as_mut()
        .expect("bridge fallback tests require resolved terrain");
    let starter = terrain.cell_mut(12, 10).unwrap();
    starter.bridge_facts.raw_flags = 0;
    starter.bridge_facts.anchor = None;
}

fn step(sim: &mut Simulation, rules: &RuleSet) -> TickResult {
    let due = sim.take_due_commands();
    sim.advance_tick(&due, Some(rules), None, None, 67)
}

fn step_with_overlay_registry(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> TickResult {
    let due = sim.take_due_commands();
    sim.advance_tick(&due, Some(rules), None, Some(registry), 67)
}

fn advance_pending_c4_to_detonation(sim: &mut Simulation, rules: &RuleSet) -> bool {
    let mut bridge_state_changed_seen = false;
    for _ in 0..(rules.c4_delay_ticks as u64 + 1) {
        let result = step(sim, rules);
        bridge_state_changed_seen |= result.bridge_state_changed;
    }
    bridge_state_changed_seen
}

fn advance_until_c4_claim(
    sim: &mut Simulation,
    rules: &RuleSet,
    target_id: u64,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> u64 {
    // SEAL/Tanya at Speed=4 covers ~10 lep/tick (gamemd-faithful), so a
    // one-cell enter (256 leptons) takes ~26 ticks; 32 leaves headroom.
    for _ in 0..32 {
        step_with_overlay_registry(sim, rules, registry);
        if let Some(pending) = sim
            .substrate
            .entities
            .get(target_id)
            .and_then(|b| b.pending_c4_detonation)
        {
            return pending.timer.start_frame() as u64;
        }
    }
    panic!("C4 plant was not claimed after entering the target building cell");
}

/// A PlantC4 order reaching a SEAL that is still walking elsewhere, or idle
/// several cells away, ends with the SEAL inside the building and the plant
/// claimed. The walk to the building NavCom is the only approach.
#[test]
fn c4_order_from_a_distance_reaches_the_building_while_moving_or_idle() {
    for (building, at, seal_at) in [("CABHUT", (15, 15), (11, 15)), ("BIG", (18, 19), (18, 26))] {
        for moving in [false, true] {
            let (mut sim, rules, registry) = build_ordinary_c4_sim(0xD4);
            let cabhut = sim
                .spawn_object_at_height(building, "Soviets", at.0, at.1, 0, 0, &rules)
                .unwrap();
            let seal = sim
                .spawn_object_at_height("GHOST", "Americans", seal_at.0, seal_at.1, 0, 0, &rules)
                .unwrap();
            if moving {
                assert!(
                    sim.set_infantry_destination(
                        seal,
                        NavTargetRef::cell(seal_at.0, seal_at.1 + 5),
                        &rules,
                        Some(&registry),
                        crate::sim::world::FrameEffects::default()
                    )
                    .unwrap()
                );
                for _ in 0..3 {
                    step_with_overlay_registry(&mut sim, &rules, &registry);
                }
                assert!(
                    sim.substrate
                        .entities
                        .get(seal)
                        .unwrap()
                        .movement_target
                        .is_some(),
                    "the SEAL is mid-walk when the order lands"
                );
            }
            let owner = sim.interner.intern("Americans");
            sim.queue_command(crate::sim::command::CommandEnvelope::new(
                owner,
                sim.session.tick + 1,
                Command::PlantC4 {
                    attacker_id: seal,
                    target_building_id: cabhut,
                },
            ));
            let claimed = (0..400).any(|_| {
                step_with_overlay_registry(&mut sim, &rules, &registry);
                sim.substrate
                    .entities
                    .get(cabhut)
                    .is_some_and(|b| b.pending_c4_detonation.is_some())
            });
            let e = sim.substrate.entities.get(seal).unwrap();
            assert!(
                claimed,
                "{building} moving={moving}: SEAL stopped at {:?} nav={:?} mission={:?}",
                (e.position.rx, e.position.ry),
                e.navigation.nav_com,
                e.mission.current()
            );
        }
    }
}

/// Native 51E551/51FA75 hut action ignores target-owner friendship; native
/// 51F190 -> 4D74E0 maps action29 to Capture with the hut as Destination.
/// The native rows are engineer_bridge_cursor_caller.json self/allied/hostile.
#[test]
fn capture_building_command_accepts_collapsed_noncapturable_hut_for_every_relation() {
    for relation in ["hostile", "allied", "self"] {
        let (mut sim, rules, registry) =
            super::entry_test_fixture::fixture_with_rules(BRIDGE_REPAIR_TEST_INI);
        let owner = if relation == "self" {
            "Americans"
        } else {
            "Soviets"
        };
        let cabhut = sim
            .spawn_object_at_height("CABHUT", owner, 9, 10, 0, 0, &rules)
            .unwrap();
        let engineer = spawn_engineer(&mut sim, &rules, 8, 10);
        if relation == "allied" {
            sim.house_alliances
                .entry("AMERICANS".into())
                .or_default()
                .insert("SOVIETS".into());
        }
        seed_destroyed_bridge(&mut sim);
        // Registered theater set starts keep flat tile0 out of the native
        // tile-first ladder. A collapsed overlay alone cannot override a tile.
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .test_set_high_bridge_set_starts(Some(100), Some(200));
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        assert!(
            sim.apply_command_with_overlays(
                "Americans",
                &Command::CaptureBuilding {
                    engineer_id: engineer,
                    target_building_id: cabhut,
                },
                Some(&rules),
                Some(&registry),
                crate::sim::world::FrameEffects::default(),
            ),
            "relation={relation}"
        );
        let actor = sim.substrate.entities.get(engineer).unwrap();
        assert_eq!(
            actor.navigation.nav_com,
            Some(NavTargetRef::building(cabhut)),
            "relation={relation}"
        );
        assert!(actor.attack_target.is_none(), "native queue target is null");
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            ),
            rng_before,
            "order admission makes no RNG draws: relation={relation}"
        );
    }
}

/// Native51E49E..51E637 returns repair action29 for ordinary friendly damaged
/// buildings; Infantry51F190 delivers it through Capture mission8. This
/// remains admitted with BridgeRepairHut=false, independently of hut repair.
/// Native execution: engineer_repair_joined own_damaged/allied_damaged.
#[test]
fn ordinary_friendly_repair_uses_capture_event_without_hut_exception() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_repair_joined.json",
    ))
    .unwrap();
    let strength = native["native_building_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|building| building["name"] == "GAPOWR")
        .unwrap()["strength"]
        .as_i64()
        .unwrap();
    let ini = BRIDGE_REPAIR_TEST_INI
        .replace("Strength=200", &format!("Strength={strength}"))
        .replace("BridgeRepairHut=yes", "BridgeRepairHut=no\nCapturable=yes");
    for target_owner in ["Americans", "Soviets"] {
        let (mut sim, rules, registry) = super::entry_test_fixture::fixture_with_rules(&ini);
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        let target = sim
            .spawn_object_at_height("CABHUT", target_owner, 9, 10, 0, 0, &rules)
            .unwrap();
        let engineer = spawn_engineer(&mut sim, &rules, 8, 10);
        sim.house_alliances
            .entry("AMERICANS".into())
            .or_default()
            .insert("SOVIETS".into());
        let row = native["routes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|route| {
                route["name"]
                    == if target_owner == "Americans" {
                        "own_damaged"
                    } else {
                        "allied_damaged"
                    }
            })
            .unwrap();
        // Supply the already-damaged prior from the native witness. Only its
        // ordinary building flags/HP and action/event admission are compared.
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .health
            .current = row["arrivals"][0]["before"]["building"]["actual_hp"]
            .as_i64()
            .unwrap() as i32;
        assert!(!rules.object("CABHUT").unwrap().bridge_repair_hut);
        let rng = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let before = sim.state_hash();
        assert_eq!(row["commands"][0]["action"], 29);
        assert_eq!(
            sim.engineer_building_action(engineer, target, &rules),
            Some(EngineerBuildingAction::Repair(true)),
            "target_owner={target_owner}"
        );
        assert_eq!(sim.state_hash(), before, "object-action query is read-only");
        assert!(
            sim.apply_command_with_overlays(
                "Americans",
                &Command::CaptureBuilding {
                    engineer_id: engineer,
                    target_building_id: target,
                },
                Some(&rules),
                Some(&registry),
                crate::sim::world::FrameEffects::default(),
            ),
            "target_owner={target_owner}"
        );
        let actor = sim.substrate.entities.get(engineer).unwrap();
        assert_eq!(
            actor.navigation.nav_com,
            Some(NavTargetRef::building(target)),
            "target_owner={target_owner}"
        );
        assert_eq!(
            i64::from(actor.mission.queued().raw()),
            row["commands"][0]["delivered_mission"].as_i64().unwrap(),
            "native event requests Capture"
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng
        );
    }
}

/// The native query belongs to the mouse producer, before Event4C7467.
/// An already queued Capture must survive a span becoming intact before the
/// event executes. Re-querying at this receiver would alter delayed inputs.
#[test]
fn queued_hut_capture_survives_repair_before_event_execution() {
    for relation in ["hostile", "allied", "self"] {
        let (mut sim, rules, registry) =
            super::entry_test_fixture::fixture_with_rules(BRIDGE_REPAIR_TEST_INI);
        let owner = if relation == "self" {
            "Americans"
        } else {
            "Soviets"
        };
        let hut = sim
            .spawn_object_at_height("CABHUT", owner, 9, 10, 0, 0, &rules)
            .unwrap();
        let engineer = spawn_engineer(&mut sim, &rules, 8, 10);
        if relation == "allied" {
            sim.house_alliances
                .entry("AMERICANS".into())
                .or_default()
                .insert("SOVIETS".into());
        }
        seed_destroyed_bridge(&mut sim);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .test_set_high_bridge_set_starts(Some(100), Some(200));
        assert!(super::bridge_orchestrator::bridge_hut_can_repair(
            &sim,
            (9, 10)
        ));
        let actor_owner = sim.interner.intern("Americans");
        sim.queue_command(crate::sim::command::CommandEnvelope::new(
            actor_owner,
            sim.session.tick + 1,
            Command::CaptureBuilding {
                engineer_id: engineer,
                target_building_id: hut,
            },
        ));
        for &(rx, ry) in BRIDGE_CELLS {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(rx, ry)
                .unwrap()
                .bridge_facts
                .overlay_id = Some(0xD4);
        }
        assert!(!super::bridge_orchestrator::bridge_hut_can_repair(
            &sim,
            (9, 10)
        ));
        let due = sim.take_due_commands();
        assert_eq!(due.len(), 1);
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        assert!(
            sim.apply_command_with_overlays(
                "Americans",
                &due[0].payload,
                Some(&rules),
                Some(&registry),
                crate::sim::world::FrameEffects::default(),
            ),
            "relation={relation}"
        );
        let actor = sim.substrate.entities.get(engineer).unwrap();
        assert_eq!(actor.navigation.nav_com, Some(NavTargetRef::building(hut)));
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng_before
        );
    }
}

/// SEAL with `c4_plant` set, adjacent to a healthy CABHUT, must:
///   - not claim from the adjacent cell,
///   - claim after entering the CABHUT cell,
///   - leave the hut at full HP across the entire C4Delay window,
///   - on timer expiry, route through the BridgeRepairHut branch in
///     `apply_c4_damage_to_building` so the bridge collapses while the
///     hut survives,
///   - propagate `bridge_state_changed` to TickResult so the app rebuilds
///     PathGrid.
///
/// This exercises the ordinary concrete hut sweep
/// (`574000 -> 5749C0 -> 575BA0`) and its raw-overlay publication.
/// Resident damage and complete navigation results are covered by the
/// separate concrete bridge integration tests.
#[test]
fn c4_on_cabhut_collapses_bridge_and_hut_survives() {
    let (mut sim, rules, registry) = build_ordinary_c4_sim(0xD4);
    let cabhut = sim
        .spawn_object_at_height("CABHUT", "Soviets", 15, 15, 0, 0, &rules)
        .expect("hut must be constructed and placed beside the concrete strip");
    let cabhut_max_hp = sim.substrate.entities.get(cabhut).unwrap().health.current;
    let seal = sim
        .spawn_object_at_height("GHOST", "Americans", 16, 15, 0, 0, &rules)
        .expect("SEAL must be constructed with a Walk locomotor in the adjacent cell");
    let owner = sim.interner.intern("Americans");
    sim.queue_command(crate::sim::command::CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::PlantC4 {
            attacker_id: seal,
            target_building_id: cabhut,
        },
    ));

    // First tick: the order starts the walk to the CABHUT NavCom. It must not
    // claim the marker until the SEAL's current cell resolves to the CABHUT.
    step_with_overlay_registry(&mut sim, &rules, &registry);
    assert!(
        sim.substrate
            .entities
            .get(cabhut)
            .and_then(|b| b.pending_c4_detonation)
            .is_none(),
        "adjacent SEAL must not claim C4 before entering CABHUT"
    );
    let plant_start = advance_until_c4_claim(&mut sim, &rules, cabhut, &registry);

    // Throughout the C4Delay window: hut HP must stay at max — the
    // BridgeRepairHut branch never damages the hut, even before the timer
    // fires. The damaged strip stays unchanged until detonation.
    let delay = rules.c4_delay_ticks as u64;
    let mut bridge_state_changed_seen = false;
    for _ in 0..(delay + 1) {
        if (sim.session.binary_frame as u64) < plant_start + delay {
            for y in [14, 15, 16] {
                assert_eq!(
                    sim.resolved_terrain
                        .as_ref()
                        .unwrap()
                        .cell(17, y)
                        .unwrap()
                        .bridge_facts
                        .overlay_id,
                    Some(0xD4),
                    "damaged concrete strip must remain unchanged before C4Delay expires"
                );
            }
        }
        let result = step_with_overlay_registry(&mut sim, &rules, &registry);
        bridge_state_changed_seen |= result.bridge_state_changed;
        // Hut HP invariant — hold across every tick of the window.
        let cur = sim.substrate.entities.get(cabhut).unwrap().health.current;
        assert_eq!(
            cur, cabhut_max_hp,
            "hut HP must stay at max during C4Delay (plant_start={plant_start}, sim.session.tick={})",
            sim.session.tick
        );
    }

    // After detonation: hut alive, bridge segment Destroyed,
    // bridge_state_changed propagated at least once.
    let hut = sim
        .substrate
        .entities
        .get(cabhut)
        .expect("hut entity must survive the explosion");
    assert_eq!(
        hut.health.current, cabhut_max_hp,
        "hut HP unchanged: BridgeRepairHut branch must skip damage"
    );
    assert!(!hut.dying, "hut must not be marked dying");
    assert!(
        hut.pending_c4_detonation.is_none(),
        "CABHUT pending C4 marker must clear after bridge dispatch"
    );

    // MapClass's concrete hut sweep (574000 -> 5749C0 -> 575BA0)
    // changes the raw overlay authority, without structural runtime cells.
    for y in [14, 15, 16] {
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(17, y)
                .unwrap()
                .bridge_facts
                .overlay_id,
            Some(0xE7),
            "CABHUT C4 must collapse concrete cell (17, {y})"
        );
    }

    // BR-16: the collapse must feed the minimap radar-dirty channel end-to-end
    // (the same channel the engineer-repair path uses).
    assert!(
        !sim.radar_terrain_dirty_cells.is_empty(),
        "bridge collapse must dirty minimap terrain cells"
    );
    assert!(
        sim.radar_terrain_dirty_cells.contains(&(17, 15)),
        "the collapsed concrete cell (17,15) must be radar-dirty"
    );

    assert!(
        bridge_state_changed_seen,
        "TickResult.bridge_state_changed must fire at least once so the app rebuilds PathGrid"
    );
}

/// A bomb fused on a CABHUT drops its bridge after the blast
/// (`BombClass::Detonate` `0x0043896A`, from the hut's own AI at
/// `0x006FA712`), and the frame reports the change as the C4 path does.
#[test]
fn a_fused_bomb_on_a_cabhut_collapses_the_bridge_and_reports_it() {
    let (mut sim, rules, registry) = build_ordinary_c4_sim(0xD4);
    let cabhut = sim
        .spawn_object_at_height("CABHUT", "Soviets", 15, 15, 0, 0, &rules)
        .expect("hut must be constructed and placed beside the concrete strip");
    let planter = sim
        .spawn_object_at_height("GHOST", "Americans", 16, 15, 0, 0, &rules)
        .expect("an Infantry planter");
    sim.bomb_attach(planter, Some(cabhut), &rules);
    assert!(sim.bomb_carriers().contains(&cabhut));

    let mut bridge_state_changed_seen = false;
    for _ in 0..rules.combat_damage.ivan_timed_delay + 2 {
        let result = step_with_overlay_registry(&mut sim, &rules, &registry);
        bridge_state_changed_seen |= result.bridge_state_changed;
    }
    assert!(sim.bomb_carriers().is_empty(), "the bomb went off");
    for y in [14, 15, 16] {
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(17, y)
                .unwrap()
                .bridge_facts
                .overlay_id,
            Some(0xE7),
            "the bombed hut collapses concrete cell (17, {y})"
        );
    }
    assert!(
        bridge_state_changed_seen,
        "TickResult.bridge_state_changed must report the bomb's collapse"
    );
}

/// `seal`'s C4 charge on `target`, planted this frame.
fn plant_c4(sim: &mut Simulation, rules: &RuleSet, target: u64, seal: u64) {
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .pending_c4_detonation = Some(PendingC4Detonation {
        timer: CdTimer::started(sim.session.binary_frame as i32, rules.c4_delay_ticks as i32),
        source_entity_id: Some(seal),
    });
}

#[test]
fn c4_on_cabhut_without_bridge_clears_pending_marker() {
    let (mut sim, rules) = build_sim();
    let cabhut = spawn_cabhut(&mut sim, 9, 10);
    let seal = spawn_seal(&mut sim, 10, 10);
    let cabhut_max_hp = sim.substrate.entities.get(cabhut).unwrap().health.current;
    sim.bridge_state = Some(BridgeRuntimeState::default());
    plant_c4(&mut sim, &rules, cabhut, seal);

    let mut bridge_state_changed_seen = false;
    for _ in 0..(rules.c4_delay_ticks as u64 + 1) {
        let result = step(&mut sim, &rules);
        bridge_state_changed_seen |= result.bridge_state_changed;
    }

    let hut = sim.substrate.entities.get(cabhut).unwrap();
    assert_eq!(hut.health.current, cabhut_max_hp);
    assert!(!hut.dying);
    assert!(hut.pending_c4_detonation.is_none());
    assert!(
        !bridge_state_changed_seen,
        "no bridge evidence means no bridge-state change"
    );
}

#[test]
fn c4_on_invulnerable_cabhut_still_dispatches_bridge_and_clears_pending() {
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

    let (mut sim, rules, registry) = build_ordinary_c4_sim(0xD4);
    let cabhut = sim
        .spawn_object_at_height("CABHUT", "Soviets", 15, 15, 0, 0, &rules)
        .expect("hut must be constructed and placed beside the concrete strip");
    let seal = sim
        .spawn_object_at_height("GHOST", "Americans", 16, 15, 0, 0, &rules)
        .expect("SEAL must be constructed with a Walk locomotor in the adjacent cell");
    let cabhut_max_hp = sim.substrate.entities.get(cabhut).unwrap().health.current;
    plant_c4(&mut sim, &rules, cabhut, seal);
    sim.substrate
        .entities
        .get_mut(cabhut)
        .unwrap()
        .invulnerability = Some(InvulnerabilityState::new(
        crate::sim::timer::CdTimer::started(
            sim.session.tick as i32,
            rules.c4_delay_ticks as i32 + 20,
        ),
        InvulnKind::IronCurtain,
    ));

    let mut bridge_state_changed_seen = false;
    for _ in 0..(rules.c4_delay_ticks as u64 + 1) {
        let result = step_with_overlay_registry(&mut sim, &rules, &registry);
        bridge_state_changed_seen |= result.bridge_state_changed;
    }

    let hut = sim.substrate.entities.get(cabhut).unwrap();
    assert_eq!(hut.health.current, cabhut_max_hp);
    assert!(!hut.dying);
    assert!(hut.pending_c4_detonation.is_none());
    assert!(bridge_state_changed_seen);
    for y in [14, 15, 16] {
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(17, y)
                .unwrap()
                .bridge_facts
                .overlay_id,
            Some(0xE7)
        );
    }
}

#[test]
fn c4_on_cabhut_fallback_rejects_anchor_or_direction_flags_alone() {
    let (mut sim, rules) = build_sim();
    let cabhut = spawn_cabhut(&mut sim, 9, 10);
    let seal = spawn_seal(&mut sim, 9, 10);
    seed_hut_fallback_bridgehead_layout(&mut sim);
    let starter = sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(12, 10)
        .unwrap();
    starter.bridge_facts.raw_flags = BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_DIRECTION_ZERO;
    starter.bridge_facts.anchor = None;
    plant_c4(&mut sim, &rules, cabhut, seal);

    let bridge_state_changed_seen = advance_pending_c4_to_detonation(&mut sim, &rules);

    assert!(
        !bridge_state_changed_seen,
        "0x80/0x800 alone must not trigger CABHUT no-overlay fallback"
    );
    assert_eq!(fallback_anchor_state(&sim), FALLBACK_ANCHOR_STATE);
}

#[test]
fn c4_on_cabhut_without_fallback_starter_is_noop() {
    let (mut sim, rules) = build_sim();
    let cabhut = spawn_cabhut(&mut sim, 9, 10);
    let seal = spawn_seal(&mut sim, 9, 10);
    seed_hut_fallback_without_starter(&mut sim);
    plant_c4(&mut sim, &rules, cabhut, seal);

    let bridge_state_changed_seen = advance_pending_c4_to_detonation(&mut sim, &rules);

    assert!(!bridge_state_changed_seen);
    assert_eq!(fallback_anchor_state(&sim), FALLBACK_ANCHOR_STATE);
}

#[test]
fn c4_on_cabhut_low_overlay_collapses_low_bridge() {
    // Original574C20 ->574780 ->575220 reaches the same live ordinary owner
    // as the physical hut corpus. The retired fixture populated only a runtime
    // cache, leaving CellClass overlays empty and providing no Recalc inputs.
    let (mut sim, rules, registry) = build_ordinary_c4_sim(0x4A);
    let cabhut = sim
        .spawn_object_at_height("CABHUT", "Soviets", 15, 15, 0, 0, &rules)
        .expect("hut beside the wooden strip");
    let seal = sim
        .spawn_object_at_height("GHOST", "Americans", 16, 15, 0, 0, &rules)
        .expect("SEAL beside the hut");
    let hut_hp = sim.substrate.entities.get(cabhut).unwrap().health.current;
    plant_c4(&mut sim, &rules, cabhut, seal);

    let mut changed = false;
    for _ in 0..=rules.c4_delay_ticks {
        changed |= step_with_overlay_registry(&mut sim, &rules, &registry).bridge_state_changed;
    }
    let hut = sim.substrate.entities.get(cabhut).unwrap();
    assert_eq!(hut.health.current, hut_hp);
    assert!(!hut.dying);
    assert!(hut.pending_c4_detonation.is_none());
    assert!(changed);
    for y in [14, 15, 16] {
        let cell = sim.resolved_terrain.as_ref().unwrap().cell(17, y).unwrap();
        assert_eq!(cell.bridge_facts.overlay_id, Some(100));
        assert!(!cell.bridge_facts.has_structural_bridge());
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(17, y).overlay_id,
            Some(100)
        );
    }
}

#[test]
fn c4_on_cabhut_low_terminal_overlay_0x65_uses_overlay_first_scan() {
    let (mut sim, rules) = build_sim();
    let cabhut = spawn_cabhut(&mut sim, 9, 10);
    let seal = spawn_seal(&mut sim, 10, 10);
    seed_terminal_overlay_with_fallback_trap(&mut sim, 0x65);
    plant_c4(&mut sim, &rules, cabhut, seal);

    let bridge_state_changed_seen = advance_pending_c4_to_detonation(&mut sim, &rules);

    assert!(
        !bridge_state_changed_seen,
        "terminal overlay scan hit must not fall through to fallback trap"
    );
    assert_eq!(fallback_anchor_state(&sim), FALLBACK_ANCHOR_STATE);
}

#[test]
fn c4_on_cabhut_high_terminal_overlay_0xe8_uses_overlay_first_scan() {
    let (mut sim, rules) = build_sim();
    let cabhut = spawn_cabhut(&mut sim, 9, 10);
    let seal = spawn_seal(&mut sim, 10, 10);
    seed_terminal_overlay_with_fallback_trap(&mut sim, 0xE8);
    plant_c4(&mut sim, &rules, cabhut, seal);

    let bridge_state_changed_seen = advance_pending_c4_to_detonation(&mut sim, &rules);

    assert!(
        !bridge_state_changed_seen,
        "terminal overlay scan hit must not fall through to fallback trap"
    );
    assert_eq!(fallback_anchor_state(&sim), FALLBACK_ANCHOR_STATE);
}

// ---- G4 damaged-variant lifecycle integration tests ------------------------

#[test]
fn ordinary_engineer_overlay_repair_preserves_pavement_damage() {
    use super::bridge_orchestrator::{ready_engineer, ready_repair_fixture, repair_frame};
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);
    // Admit damaged-data tiles so an accidental pavement clear would
    // affect this fixture; native ordinary overlay repair must preserve it.
    for &(rx, ry) in LIVE_REPAIR_STRIP {
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(rx, ry)
            .unwrap();
        cell.has_damaged_data = true;
        cell.bridge_facts.raw_flags |= crate::map::bridge_pavement::DAMAGED_PAVEMENT;
    }

    assert!(repair_frame(&mut sim, &rules, &registry).bridge_state_changed);
    assert!(sim.substrate.entities.get(engineer).is_none());

    let terrain = sim.resolved_terrain.as_ref().unwrap();
    for &(rx, ry) in LIVE_REPAIR_STRIP {
        assert_eq!(
            terrain.cell(rx, ry).unwrap().bridge_facts.overlay_id,
            Some(0xce)
        );
        assert!(
            terrain.pavement_damaged_at(rx, ry),
            "cell ({rx},{ry}) pavement damage must survive native ordinary overlay repair"
        );
    }
}

#[test]
fn ordinary_engineer_overlay_repair_does_not_clear_neighbor_pavement() {
    use super::bridge_orchestrator::{ready_engineer, ready_repair_fixture, repair_frame};
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);

    // The neighbor shares the span's tile identity and damaged-data gate.
    // It lies outside the repaired strip, adjacent to (17, 16), so it is NOT
    // visited by the ordinary overlay repair walk. An erroneous connected
    // pavement clear would reach it from the adjacent span cell.
    for &(rx, ry) in LIVE_REPAIR_STRIP.iter().chain(&[(17, 17)]) {
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(rx, ry)
            .unwrap();
        cell.has_damaged_data = true;
        cell.bridge_facts.raw_flags |= crate::map::bridge_pavement::DAMAGED_PAVEMENT;
    }

    assert!(repair_frame(&mut sim, &rules, &registry).bridge_state_changed);
    assert!(sim.substrate.entities.get(engineer).is_none());

    let terrain = sim.resolved_terrain.as_ref().unwrap();
    for &(rx, ry) in LIVE_REPAIR_STRIP {
        assert_eq!(
            terrain.cell(rx, ry).unwrap().bridge_facts.overlay_id,
            Some(0xce)
        );
    }
    assert!(
        terrain.pavement_damaged_at(17, 17),
        "ordinary overlay repair must not flood-clear off-span pavement"
    );
}

// Live519C07 ->573540 ->57F440 ->5800D0 publication owns stream routing.
// Transition/callback arithmetic is checked by bridge_ordinary_repair.json;
// SimRng's mapgen_range.json regression checks the retained-state range helper.
// These integration tests enter the already-admitted production receiver with
// resident TMP/overlay inputs and all navigation owners installed.
const LIVE_REPAIR_STRIP: &[(u16, u16)] = &[(17, 14), (17, 15), (17, 16)];

fn live_repair_fixture() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    u64,
) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 16, 15, 0, &rules)
        .unwrap();
    let owner = sim.substrate.entities.get(engineer).unwrap().owner();
    for (index, &(x, y)) in LIVE_REPAIR_STRIP.iter().enumerate() {
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(x, y)
            .unwrap()
            .bridge_facts
            .overlay_id = Some(231);
        let overlays = sim.overlay_grid.as_mut().unwrap();
        overlays.place_overlay(x, y, 231, 0xA0 + index as u8);
        overlays.cell_mut(x, y).wall_owner = Some(owner);
    }
    (sim, rules, registry, engineer)
}

fn assert_live_repair_strip(sim: &Simulation, engineer: u64, expected_overlay: u8) {
    let owner = sim.substrate.entities.get(engineer).unwrap().owner();
    for (index, &(x, y)) in LIVE_REPAIR_STRIP.iter().enumerate() {
        let terrain = sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
        let overlay = sim.overlay_grid.as_ref().unwrap().cell(x, y);
        assert_eq!(terrain.bridge_facts.overlay_id, Some(expected_overlay));
        assert_eq!(overlay.overlay_id, Some(expected_overlay));
        assert_eq!(overlay.overlay_data, 0xA0 + index as u8);
        assert_eq!(overlay.wall_owner, Some(owner));
    }
}

#[test]
fn ordinary_engineer_repair_consumes_mapgen_only_and_preserves_overlay_metadata() {
    let (mut sim, rules, registry, engineer) = live_repair_fixture();
    let scenario_before = sim.scenario_rng.logical_state();
    let main_before = sim.main_rng.logical_state();
    let mut expected_mapgen = crate::sim::rng::SimRng::new(0);
    assert_eq!(expected_mapgen.next_high_two_bits(), 1);

    assert!(
        crate::sim::world::bridge_orchestrator::repair_from_engineer(
            &mut sim,
            &rules,
            Some(&registry),
            engineer,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("live repair must complete its Recalc and navigation callbacks")
    );

    assert_eq!(sim.scenario_rng.logical_state(), scenario_before);
    assert_eq!(sim.main_rng.logical_state(), main_before);
    assert_eq!(
        sim.mapgen_rng.logical_state(),
        expected_mapgen.logical_state()
    );
    assert_live_repair_strip(&sim, engineer, 0xCE);
}

/// An installed post-generation cursor feeds the live owner's next repair draw.
#[test]
fn generated_map_bridge_repair_continues_post_rmg_mapgen_stream() {
    let mut generated = crate::sim::rng::SimRng::new(0xBEEF);
    for _ in 0..353 {
        let _ = generated.next_u32();
    }
    let generated = generated.logical_state();
    let continuation = || {
        MapGenRngContinuation::from_native_parts(
            generated.disabled,
            generated.words,
            usize::try_from(generated.index_a).expect("test MapGen cursor A is non-negative"),
            usize::try_from(generated.index_b).expect("test MapGen cursor B is non-negative"),
        )
    };
    let mut expected = crate::sim::rng::SimRng::from_mapgen_continuation(continuation());
    let expected_variant = expected.next_high_two_bits();
    let (mut sim, rules, registry, engineer) = live_repair_fixture();
    sim.mapgen_rng = crate::sim::rng::SimRng::from_mapgen_continuation(continuation());
    let scenario_before = sim.scenario_rng.logical_state();
    let main_before = sim.main_rng.logical_state();

    assert!(
        crate::sim::world::bridge_orchestrator::repair_from_engineer(
            &mut sim,
            &rules,
            Some(&registry),
            engineer,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("live repair must retain the installed MapGen continuation")
    );

    assert_eq!(sim.scenario_rng.logical_state(), scenario_before);
    assert_eq!(sim.main_rng.logical_state(), main_before);
    assert_eq!(sim.mapgen_rng.logical_state(), expected.logical_state());
    assert_live_repair_strip(&sim, engineer, 0xCD + expected_variant);
}

#[test]
fn two_identical_sims_repair_with_identical_hash_and_streams() {
    let (mut sim_a, rules_a, registry_a, engineer_a) = live_repair_fixture();
    let (mut sim_b, rules_b, registry_b, engineer_b) = live_repair_fixture();
    for (sim, rules, registry, engineer) in [
        (&mut sim_a, &rules_a, &registry_a, engineer_a),
        (&mut sim_b, &rules_b, &registry_b, engineer_b),
    ] {
        assert!(
            crate::sim::world::bridge_orchestrator::repair_from_engineer(
                sim,
                rules,
                Some(registry),
                engineer,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("live repair must complete")
        );
        assert_live_repair_strip(sim, engineer, 0xCE);
    }
    assert_eq!(sim_a.state_hash(), sim_b.state_hash());
    assert_eq!(
        sim_a.scenario_rng.logical_state(),
        sim_b.scenario_rng.logical_state()
    );
    assert_eq!(
        sim_a.main_rng.logical_state(),
        sim_b.main_rng.logical_state()
    );
    assert_eq!(
        sim_a.mapgen_rng.logical_state(),
        sim_b.mapgen_rng.logical_state()
    );
}
