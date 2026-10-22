//! Accepted REPAIR21 keeps the original issuer across capture. The receiver
//! (4C6ED2) and payment (450630) use the building's current owner. Native paid
//! controls use Strength1000/Cost2500/RepairStep8/15%; this fixture supplies
//! those same type priors and joins the existing queue, capture and frame owners.

use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{
    COMMAND_RECORD_LEN, Command, CommandRecord, REPAIR_OPCODE, RepairRecord,
};
use crate::sim::command_queue::FrameInfo;
use crate::sim::estimated_health::EstimatedHealth;
use crate::sim::house_state::HouseState;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;

struct Scene {
    runtime: SimRuntime,
    building: u64,
    old: InternedId,
    new: InternedId,
}

impl Scene {
    fn new(new_money: i32) -> Self {
        let (sim, rules, registry) = entry_test_fixture::fixture_with_rules(
            "[General]\nRepairStep=8\nRepairPercent=15%\nRepairRate=.016\n\
             [IQ]\nRepairSell=1\n[AI]\nCreditReserve=100\n\
             [BuildingTypes]\n1=REPAIRTEST\n[REPAIRTEST]\nStrength=1000\nCost=2500\n\
             Foundation=2x2\nClickRepairable=yes\nRepairable=yes\nHasStupidGuardMode=no\n\
             [VehicleTypes]\n0=KEEPER\n[KEEPER]\nStrength=100\nSpeed=4\nSpeedType=Track\n\
             MovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
        );
        Self::with_parts(sim, rules, registry, "REPAIRTEST", ["KEEPER"; 2], new_money)
    }

    fn retail(new_money: i32) -> Option<Self> {
        let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let mut roots =
            crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
        roots.extend(
            retail
                .rules
                .general
                .damage_fire_types
                .iter()
                .map(|anim| anim.name.clone()),
        );
        retail
            .rules
            .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
        retail
            .rules
            .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        // The map/graph is an explicit flat-map prior. All type, repair and
        // fixed ART values below come through retail startup -> Battle -> Hills.
        let (mut sim, _, _) = entry_test_fixture::fixture();
        sim.intern_rule_type_ids(&retail.rules);
        sim.resolve_type_handles(&retail.rules);
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        terrain.set_land_speed_rules_for_test(&retail.rules.terrain_rules);
        let clear = retail
            .rules
            .terrain_rules
            .semantics_for_land_type(0)
            .unwrap();
        for cell in terrain.cells_mut_for_test() {
            cell.speed_costs = clear.speed_costs;
            cell.base_speed_costs = clear.speed_costs;
        }
        Some(Self::with_parts(
            sim,
            retail.rules,
            registry,
            "GAPOWR",
            ["MTNK", "HTNK"],
            new_money,
        ))
    }

    fn with_parts(
        mut sim: Simulation,
        rules: RuleSet,
        registry: OverlayTypeRegistry,
        building_type: &str,
        keepers: [&str; 2],
        new_money: i32,
    ) -> Self {
        let old = sim.intern("Americans");
        let new = sim.intern("Russians");
        for (owner, money) in [(old, 5000), (new, new_money)] {
            let mut house = HouseState::new(owner, 0, None, true, money, 10);
            house.set_current_iq_for_test(0);
            sim.houses.register(house);
        }
        sim.session.set_current_house_for_test(Some(new));
        let building = sim
            .spawn_object_with_overlay_registry(
                building_type,
                "Americans",
                15,
                15,
                0,
                &rules,
                &registry,
            )
            .unwrap();
        for ((owner, cell), kind) in [("Americans", (17, 14)), ("Russians", (17, 16))]
            .into_iter()
            .zip(keepers)
        {
            sim.spawn_object_with_overlay_registry(
                kind, owner, cell.0, cell.1, 0, &rules, &registry,
            )
            .unwrap();
        }
        let entity = sim.substrate.entities.get_mut(building).unwrap();
        entity.health.set_for_test(600);
        entity.estimated_health = EstimatedHealth::from_raw(600);
        sim.session.set_binary_frame_for_test(195);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        sim.hash_state_every(std::num::NonZeroU64::MIN);
        let terrain_template = sim.resolved_terrain.clone();
        Self {
            runtime: SimRuntime {
                simulation: sim,
                resources: SimResources {
                    rules,
                    overlay_registry: registry,
                    terrain_template,
                },
            },
            building,
            old,
            new,
        }
    }

    fn record(&self) -> CommandRecord {
        self.runtime
            .simulation
            .encode_repair_record(self.old, self.building)
            .unwrap()
    }

    fn queue(&mut self, record: CommandRecord) {
        assert!(
            self.runtime
                .simulation
                .issue_local_command(CommandEnvelope::synchronized(record, u64::MAX))
        );
    }

    fn capture(&mut self) {
        self.runtime.simulation.change_owner_with_rules(
            self.building,
            self.new,
            &self.runtime.resources.rules,
            Some(&self.runtime.resources.overlay_registry),
            FrameEffects::default(),
        );
        let entity = self
            .runtime
            .simulation
            .substrate
            .entities
            .get(self.building)
            .unwrap();
        assert_eq!(entity.owner(), self.new);
        assert!(!entity.repairing.is_on());
    }

    fn frame(&mut self) -> SimFrameOutput {
        self.runtime
            .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
            .unwrap()
    }

    fn state(&self) -> (i32, bool, i32, i32) {
        let sim = &self.runtime.simulation;
        let entity = sim.substrate.entities.get(self.building).unwrap();
        (
            entity.health.current(),
            entity.repairing.is_on(),
            sim.houses[&self.old].economy().credits(),
            sim.houses[&self.new].economy().credits(),
        )
    }
}

#[test]
fn retail_power_plant_keeps_the_accepted_issuer_and_charges_its_captured_owner() {
    for (money, expected) in [(5000, (382, true, 5000, 4999)), (0, (374, false, 5000, 0))] {
        let Some(mut scene) = Scene::retail(money) else {
            return;
        };
        let rules = &scene.runtime.resources.rules;
        let plant = rules.object("GAPOWR").unwrap();
        assert_eq!((plant.strength, plant.cost), (750, 800));
        assert!(plant.capturable);
        assert_eq!(rules.general.repair_step, 8);
        assert_eq!(production::repair_step_cost(rules, plant), 1);
        // The native whole ChangeOwner -> EventExecute -> paid-update controls
        // supply these independent health authorities and already-running repair.
        let entity = scene
            .runtime
            .simulation
            .substrate
            .entities
            .get_mut(scene.building)
            .unwrap();
        entity.health.set_for_test(374);
        entity.estimated_health = EstimatedHealth::from_raw(611);
        entity.repairing.set_for_test(true);
        assert!(production::can_repair_building(
            &scene.runtime.simulation,
            rules,
            scene.building
        ));
        let record = scene.record();
        scene.queue(record);
        scene.capture();
        assert_eq!(scene.state(), (374, false, 5000, money));
        assert_eq!(
            scene
                .runtime
                .simulation
                .substrate
                .entities
                .get(scene.building)
                .unwrap()
                .estimated_health,
            EstimatedHealth::from_raw(611),
            "capture does not heal either health authority"
        );
        scene.frame();
        assert_eq!(scene.state(), (374, true, 5000, money));
        scene.frame();
        assert_eq!(scene.state(), expected);
        // Ordinary TechnoAI also owns estimated-health recovery. The native
        // direct450630 control's 619 estimate excludes those complete visits.
    }
}

#[test]
fn an_accepted_old_issuer_repairs_and_charges_the_new_owner_on_the_next_due_visit() {
    for (money, expected) in [
        (5000, (608, true, 5000, 4998)),
        (99, (608, true, 5000, 97)),
        (1, (600, false, 5000, 1)),
    ] {
        let mut scene = Scene::new(money);
        let record = scene.record();
        assert_eq!(record.house_id(), 0);
        scene.queue(record);
        scene.capture();
        assert_eq!(scene.state(), (600, false, 5000, money));
        let delivered = scene.frame();
        assert!(delivered.tick.frame_committed);
        assert_eq!(delivered.tick.executed_commands, 1);
        assert_eq!(
            scene.state(),
            (600, true, 5000, money),
            "delivery is after Logic195"
        );
        assert!(delivered.sound_events.iter().any(
            |event| matches!(event, SimSoundEvent::Repairing { owner } if *owner == scene.new)
        ));
        assert_eq!(scene.runtime.simulation.session.binary_frame(), 196);
        scene.frame();
        assert_eq!(scene.state(), expected, "new-owner money={money}");
        scene.frame();
        assert_eq!(
            scene.state(),
            expected,
            "no extra toggle or off-period payment"
        );
        assert!(
            scene
                .runtime
                .simulation
                .pending_command_snapshot()
                .is_empty()
        );
    }
}

#[test]
fn a_delivered_repair_does_not_repeat_can_repair_after_acceptance() {
    let mut scene = Scene::new(5000);
    let record = scene.record();
    scene
        .runtime
        .simulation
        .substrate
        .entities
        .get_mut(scene.building)
        .unwrap()
        .health
        .set_for_test(1000);
    assert!(!production::can_repair_building(
        &scene.runtime.simulation,
        &scene.runtime.resources.rules,
        scene.building
    ));
    scene.queue(record);
    scene.frame();
    assert_eq!(scene.state(), (1000, true, 5000, 5000));
    scene.frame();
    assert_eq!(
        scene.state(),
        (1000, false, 4998, 5000),
        "the next due step pays before noticing full Strength, as native"
    );
}

#[test]
fn pending_repair_survives_both_local_and_synchronized_snapshot_stages() {
    for in_dolist in [false, true] {
        let mut scene = Scene::new(5000);
        let mut record = scene.record();
        // A retained future native frame, shared with received/retimed events.
        record.set_frame_stamp(197);
        scene.queue(record);
        if in_dolist {
            scene.runtime.simulation.transfer_local_commands();
        }
        scene.capture();
        let pending = scene.runtime.simulation.pending_command_snapshot();
        let map_name = scene.runtime.simulation.session.map_name().to_owned();
        let hash = scene.runtime.resources.rules.simulation_config_hash();
        let bytes = GameSnapshot::save_validated(&scene.runtime.simulation, 17, hash, "repair", 0);
        let mut loaded = GameSnapshot::load_validated(&bytes, 17, hash, &map_name)
            .unwrap()
            .sim;
        loaded.restore_after_snapshot_load().unwrap();
        loaded.rebuild_caches_after_load(
            scene.runtime.resources.terrain_template.clone().unwrap(),
            scene.runtime.simulation.terrain_speed_config.clone(),
            &scene.runtime.resources.rules,
        );
        loaded
            .restore_map_authority_after_snapshot_load(
                &scene.runtime.resources.rules,
                &scene.runtime.resources.overlay_registry,
                None,
            )
            .unwrap();
        assert_eq!(loaded.pending_command_snapshot(), pending);
        assert_eq!(
            loaded
                .substrate
                .entities
                .get(scene.building)
                .unwrap()
                .owner(),
            scene.new
        );
        let mut restored = Scene {
            runtime: SimRuntime {
                simulation: loaded,
                resources: SimResources {
                    rules: scene.runtime.resources.rules.clone(),
                    overlay_registry: scene.runtime.resources.overlay_registry.clone(),
                    terrain_template: scene.runtime.resources.terrain_template.clone(),
                },
            },
            building: scene.building,
            old: scene.old,
            new: scene.new,
        };
        for _ in 0..17 {
            scene.frame();
            restored.frame();
        }
        assert_eq!(scene.state(), (608, true, 5000, 4998));
        assert_eq!(restored.state(), scene.state(), "in_dolist={in_dolist}");
        assert!(
            restored
                .runtime
                .simulation
                .pending_command_snapshot()
                .is_empty()
        );
    }
}

#[test]
fn recorded_repair_admission_reconciles_pending_copies_and_replays_once() {
    let mut live = Scene::new(5000);
    let mut replay = Scene::new(5000);
    for scene in [&mut live, &mut replay] {
        let record = scene.record();
        scene.queue(record);
        scene.capture();
    }
    assert_eq!(
        live.runtime.simulation.state_hash(),
        replay.runtime.simulation.state_hash()
    );
    let mut log = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: live.runtime.simulation.session.pixel_conversion_bounds(),
        version: 1,
        tick_hz: 15,
        seed: live.runtime.simulation.session.seed(),
        map_name: "repair event".to_owned(),
        rules_hash: live.runtime.resources.rules.simulation_config_hash(),
    });
    let mut expected = Vec::new();
    for _ in 0..4 {
        let mut output = live.frame();
        expected.push(output.tick.state_hash);
        let commands = output.take_admitted_commands();
        log.record_tick(output.tick.tick, commands, output.tick.state_hash);
    }
    assert_eq!(
        log.ticks
            .iter()
            .map(|tick| tick.commands.len())
            .sum::<usize>(),
        1
    );
    let hashes = ReplayRunner::run_runtime(&mut replay.runtime, &log, 67).unwrap();
    assert_eq!(hashes, expected);
    assert_eq!(replay.state(), live.state());
    assert_eq!(live.state(), (608, true, 5000, 4998));
    assert!(
        replay
            .runtime
            .simulation
            .pending_command_snapshot()
            .is_empty()
    );
}

#[test]
fn repair_uses_the_existing_packet_and_mirror_owners_with_a_five_byte_payload() {
    let mut scene = Scene::new(5000);
    let record = scene.record();
    scene.queue(record.clone());
    scene.capture();
    let mut codec = crate::net::event_packet::EventPacketCodec::default();
    let packet = {
        let queues = scene.runtime.simulation.network_command_queues();
        let records = queues.local_records().unwrap();
        codec
            .encode_packet(
                &records,
                FrameInfo {
                    house_id: 0,
                    event_frame: 197,
                    checksum: 0,
                    timing_word: 0,
                    delay: 0,
                },
                195,
                crate::net::event_packet::NATIVE_EVENT_PACKET_CAPACITY,
            )
            .unwrap()
    };
    assert_eq!(
        packet.bytes.len(),
        crate::net::event_packet::EVENT_PACKET_HEADER_LEN + 1 + 5
    );
    assert_eq!(packet.mirrors.len(), 1);
    scene
        .runtime
        .simulation
        .network_command_queues()
        .commit_local_mirrors(&packet.mirrors)
        .unwrap();
    assert_eq!(
        scene
            .runtime
            .simulation
            .network_command_queues()
            .local_len(),
        0
    );
    let mut decoded = Vec::new();
    codec
        .decode_packet(&packet.bytes, |record| {
            decoded.push(record);
            true
        })
        .unwrap();
    let repair = decoded
        .iter()
        .find(|record| record.opcode() == REPAIR_OPCODE)
        .unwrap();
    assert_eq!(&repair.payload()[..5], &record.payload()[..5]);
    assert_eq!(repair.house_id(), 0, "saved sender, not captured owner");
    assert_eq!(repair.frame_stamp(), 197);
    for _ in 0..17 {
        scene.frame();
    }
    assert_eq!(scene.state(), (608, true, 5000, 4998));
}

#[test]
fn receiver_uses_native_header_house_instead_of_diagnostic_carrier_metadata() {
    let mut scene = Scene::new(5000);
    let mut carrier = CommandEnvelope::synchronized(scene.record(), u64::MAX);
    carrier.owner = InternedId::from_index(u32::MAX);
    assert!(scene.runtime.simulation.issue_local_command(carrier));
    scene.capture();
    scene.frame();
    scene.frame();
    assert_eq!(scene.state(), (608, true, 5000, 4998));
}

#[test]
fn typed_repair_decode_keeps_missing_objects_until_the_receiver_resolves_them() {
    let scene = Scene::new(5000);
    let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).unwrap();
    RepairRecord {
        house_id: 0,
        frame: 195,
        entity_id: 900_000,
    }
    .write_into(&mut record);
    let decoded = scene
        .runtime
        .simulation
        .decode_native_command_record(&record, 123)
        .unwrap();
    assert!(matches!(
        decoded.payload,
        Command::ToggleRepair { entity_id: 900_000 }
    ));
    assert_eq!(decoded.owner, scene.old);
    assert_eq!(decoded.execute_tick, 123);
}
