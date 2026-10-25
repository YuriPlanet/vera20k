//! Typed Event4C7128 -> Factory4C9C70 -> paid PLACE4FB0E0 -> Exit443C60.
//! Finished bases, a local retained strip and flat physical cells are scene
//! priors. No Factory progress, held identity, class or movement head is seeded.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, object_type::ObjectCategory, ruleset::RuleSet};
use crate::sim::command::{
    Command, CommandEnvelope, PlaceRecord, ProductionEvent, ProductionRecord,
};
use crate::sim::intern::InternedId;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, Simulation, TickLane};

fn rules(collision: bool) -> RuleSet {
    let vehicles = if collision {
        "0=E1\n1=HOST\n2=SLAV\n3=HORN"
    } else {
        "0=HOST"
    };
    let building_collision = if collision { "3=HORN" } else { "" };
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nBuildSpeed=.1\nAnimToInfantry=E1\n\
         [Countries]\n0=Americans\n1=Russians\n\
         [InfantryTypes]\n0=E1\n1=SLAV\n[VehicleTypes]\n{vehicles}\n\
         [AircraftTypes]\n0=HORN\n[BuildingTypes]\n0=GAPILE\n1=GAWEAP\n2=VAT\n{building_collision}\n\
         [Animations]\n0=CHANGE\n\
         [E1]\nLocomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\nStrength=125\nCost=200\nSpeed=4\nSight=5\nTechLevel=1\nOwner=Americans\nAmmo=3\n\
         [SLAV]\nLocomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\nStrength=80\nSpeed=4\nStorage=4\n\
         [HORN]\nLocomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\nStrength=75\nSpeed=14\nAmmo=1\nSpawned=yes\n\
         [HOST]\nLocomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\nStrength=500\nCost=600\nSpeed=4\nTechLevel=1\nOwner=Americans\n\
         Enslaves=SLAV\nSlavesNumber=1\nSpawns=HORN\nSpawnsNumber=1\n\
         SpawnRegenRate=600\nSlaveRegenRate=500\n\
         [GAPILE]\nStrength=1000\nFactory=InfantryType\nOwner=Americans,Russians\n\
         [GAWEAP]\nStrength=1000\nFactory=UnitType\nOwner=Americans\n\
         [VAT]\nStrength=1000\nCloning=yes\nOwner=Americans\n"
    ));
    let art = IniFile::from_str(
        "[E1]\nSequence=GI\n[SLAV]\nSequence=GI\n\
         [GI]\nReady=0,1,0\nWalk=1,1,1\n\
         [GAPILE]\nFoundation=1x1\n[GAWEAP]\nFoundation=1x1\n[VAT]\nFoundation=1x1\n\
         [CHANGE]\nStart=0\nEnd=1\nLoopEnd=1\nRate=900\nMakeInfantry=0\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("CHANGE", 2);
    rules.replace_art_registry_for_test(registry);
    // AnimType427F66 stores 900/Rate logic frames, so this declared two-frame
    // animation advances each tick through its ordinary Anim AI owner.
    assert_eq!(
        rules
            .art()
            .anim_runtime_config("CHANGE")
            .unwrap()
            .rate_logic_frames,
        1,
    );
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    rules
}

fn world(collision: bool, vat: bool) -> (SimRuntime, InternedId) {
    let rules = rules(collision);
    let mut sim = Simulation::with_seed(0x4c9c_7000);
    sim.intern_rule_type_ids(&rules);
    tests::install_infantry_delivery_fixture_map(&mut sim);
    let owner = sim.interner.intern("Americans");
    for (id, house, name, x, y) in [
        (1, "Americans", "GAPILE", 14, 14),
        (2, "Americans", "GAWEAP", 22, 14),
        (3, "Russians", "GAPILE", 14, 34),
    ] {
        tests::spawn_structure(&mut sim, &rules, id, house, name, x, y);
        assert!(sim.register_live_object(id));
    }
    if vat {
        tests::spawn_structure(&mut sim, &rules, 4, "Americans", "VAT", 30, 14);
        assert!(sim.register_live_object(4));
    }
    sim.session.set_current_house_for_test(Some(owner));
    sim.resolve_type_handles(&rules);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    (
        SimRuntime {
            simulation: sim,
            resources: SimResources::new(rules),
        },
        owner,
    )
}

fn frame(
    runtime: &mut SimRuntime,
    owner: InternedId,
    commands: &[CommandEnvelope],
) -> Vec<CommandEnvelope> {
    // The app supplies these same retained Factory identities from its strip.
    let visits = runtime
        .simulation
        .production
        .factories()
        .strip_visits_for_test(owner);
    runtime
        .advance_frame(
            commands,
            67,
            TickLane::Ordinary,
            FrameEffects::default().with_production_strip(&visits, &|_| {}),
        )
        .unwrap()
        .take_admitted_commands()
}

fn produce(
    runtime: &SimRuntime,
    owner: InternedId,
    family: ObjectCategory,
    name: &str,
    alias: Option<i32>,
) -> CommandEnvelope {
    let rules = &runtime.resources.rules;
    let object = rules.object_in_category(family, name).unwrap();
    let mut selection = ProductionSelection::for_type(rules, object).unwrap();
    if let Some(rtti) = alias {
        selection.rtti = rtti;
        // Packet Naval is independent of the Infantry type's own false flag.
        selection.naval = rtti == 16;
    }
    let record = runtime
        .simulation
        .encode_selected_production_record(owner, ProductionEvent::Produce, selection)
        .unwrap();
    CommandEnvelope::synchronized(record, runtime.simulation.session.tick() + 1)
}

fn held(sim: &Simulation, owner: InternedId, category: ProductionCategory) -> Option<u64> {
    sim.production
        .factories()
        .view(owner, category)?
        .object?
        .entity_id
}

fn assert_type(runtime: &SimRuntime, id: u64, family: ObjectCategory, name: &str) {
    let rules = &runtime.resources.rules;
    let entity = runtime.simulation.entities().get(id).unwrap();
    let object = rules.object_in_category(family, name).unwrap();
    assert_eq!(entity.category(), EntityCategory::from(family));
    assert!(std::ptr::eq(
        runtime
            .simulation
            .entity_object_type(entity, rules)
            .unwrap(),
        object
    ));
    assert_eq!(entity.health.current(), object.strength);
    assert_eq!(entity.crushable_facts.crushable(), object.crushable);
    assert_eq!(entity.ammo().map(|ammo| ammo.current()), Some(object.ammo));
    assert_eq!(entity.type_facts.foundation(), &object.foundation);
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .map(|locomotor| locomotor.active_kind()),
        Some(object.locomotor)
    );
    if family == ObjectCategory::Infantry {
        assert!(entity.mission_leaf.as_infantry().is_some());
    }
    if family == ObjectCategory::Vehicle {
        assert!(entity.mission_leaf.as_unit().is_some());
    }
}

#[test]
fn typed_duplicate_paid_start_place_and_promotion_preserve_constructor_type() {
    for collision in [false, true] {
        let (mut runtime, owner) = world(collision, false);
        // Object/type RTTI15/16 aliases both select the Infantry array, even
        // when the old name-only API resolves this name to a UnitType.
        let commands = [
            produce(&runtime, owner, ObjectCategory::Infantry, "E1", Some(16)),
            produce(&runtime, owner, ObjectCategory::Infantry, "E1", Some(15)),
        ];
        frame(&mut runtime, owner, &commands);
        let first = held(&runtime.simulation, owner, ProductionCategory::Infantry).unwrap();
        assert_type(&runtime, first, ObjectCategory::Infantry, "E1");
        assert!(
            runtime
                .simulation
                .entities()
                .get(first)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
        assert_eq!(
            runtime
                .simulation
                .production
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .unwrap()
                .queue
                .len(),
            1
        );
        let mut place = false;
        for _ in 0..2048 {
            for event in frame(&mut runtime, owner, &[]) {
                if let Command::SynchronizedRecord { record } = event.payload
                    && let Some(record) = PlaceRecord::decode(&record)
                {
                    assert_eq!(
                        (
                            record.rtti,
                            record.type_index,
                            record.naval,
                            record.x,
                            record.y
                        ),
                        (15, -1, 0, 0, 0)
                    );
                    place = true;
                }
            }
            if !runtime
                .simulation
                .entities()
                .get(first)
                .unwrap()
                .lifecycle
                .in_limbo()
            {
                break;
            }
        }
        assert!(place, "ordinary strip issued native mobile PLACE");
        assert!(
            !runtime
                .simulation
                .entities()
                .get(first)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
        assert_type(&runtime, first, ObjectCategory::Infantry, "E1");
        let next = held(&runtime.simulation, owner, ProductionCategory::Infantry).unwrap();
        assert_ne!(next, first);
        assert_type(&runtime, next, ObjectCategory::Infantry, "E1");
        assert_eq!(
            runtime.simulation.houses[&owner].economy().spent_credits(),
            200
        );
        assert!(
            runtime
                .simulation
                .production
                .factories()
                .view(owner, ProductionCategory::Vehicle)
                .is_none()
        );
    }
    // BuildingDetach44EEAC/44EEB6 retains held index/producer Factory RTTI,
    // with literal Naval0. A broad-name cancellation could hit the Unit queue.
    for backup in [false, true] {
        let (mut runtime, owner) = world(true, false);
        if backup {
            tests::spawn_structure(
                &mut runtime.simulation,
                &runtime.resources.rules,
                4,
                "Americans",
                "GAPILE",
                14,
                22,
            );
            assert!(runtime.simulation.register_live_object(4));
            runtime
                .simulation
                .resolve_type_handles(&runtime.resources.rules);
            assert!(
                runtime
                    .simulation
                    .rebuild_dynamic_navigation(&runtime.resources.rules)
            );
        }
        let commands = [
            produce(&runtime, owner, ObjectCategory::Infantry, "E1", None),
            produce(&runtime, owner, ObjectCategory::Vehicle, "E1", None),
            produce(&runtime, owner, ObjectCategory::Vehicle, "E1", None),
        ];
        frame(&mut runtime, owner, &commands);
        for _ in 0..12 {
            frame(&mut runtime, owner, &[]);
        }
        let infantry = held(&runtime.simulation, owner, ProductionCategory::Infantry).unwrap();
        let vehicle = held(&runtime.simulation, owner, ProductionCategory::Vehicle).unwrap();
        assert_type(&runtime, infantry, ObjectCategory::Infantry, "E1");
        assert_type(&runtime, vehicle, ObjectCategory::Vehicle, "E1");
        let infantry_factory = runtime
            .simulation
            .production
            .factories()
            .factory(FactoryHolder::House(owner, ProductionCategory::Infantry))
            .unwrap();
        assert!(infantry_factory.progress() > 0 && infantry_factory.progress() < PRODUCTION_STEPS);
        let object = runtime
            .resources
            .rules
            .object_in_category(ObjectCategory::Infantry, "E1")
            .unwrap();
        let refund = runtime
            .simulation
            .cost_of(owner, object, &runtime.resources.rules)
            - infantry_factory.balance();
        assert!(refund > 0);
        let vehicle_factory = runtime
            .simulation
            .production
            .factories()
            .factory(FactoryHolder::House(owner, ProductionCategory::Vehicle))
            .unwrap()
            .clone();
        assert_eq!(vehicle_factory.queue().len(), 1);
        let credits = runtime.simulation.houses[&owner].economy().credits();
        runtime.simulation.uninit_with_context(
            1,
            crate::sim::world::UninitContext::with_rules(&runtime.resources.rules),
        );
        if backup {
            assert_eq!(
                held(&runtime.simulation, owner, ProductionCategory::Infantry),
                Some(infantry)
            );
            assert_eq!(
                runtime.simulation.houses[&owner].economy().credits(),
                credits
            );
            runtime.simulation.uninit_with_context(
                4,
                crate::sim::world::UninitContext::with_rules(&runtime.resources.rules),
            );
        }
        assert!(!runtime.simulation.entities().contains(infantry));
        assert!(held(&runtime.simulation, owner, ProductionCategory::Infantry).is_none());
        assert_eq!(
            runtime.simulation.houses[&owner].economy().credits(),
            credits + refund
        );
        assert_eq!(
            runtime
                .simulation
                .production
                .factories()
                .factory(FactoryHolder::House(owner, ProductionCategory::Vehicle)),
            Some(&vehicle_factory)
        );
        assert_eq!(
            held(&runtime.simulation, owner, ProductionCategory::Vehicle),
            Some(vehicle)
        );
        assert_type(&runtime, vehicle, ObjectCategory::Vehicle, "E1");
        let type_id = runtime.simulation.interner.get("E1").unwrap();
        assert_eq!(
            runtime.simulation.houses[&owner]
                .tracking()
                .owned_count(EntityCategory::Infantry, type_id),
            0
        );
        assert_eq!(
            runtime.simulation.houses[&owner]
                .tracking()
                .owned_count(EntityCategory::Unit, type_id),
            1
        );
        validate_restored_factory_state(&runtime.simulation, &runtime.resources.rules).unwrap();
    }
}

#[test]
fn typed_duplicate_snapshot_rebind_continues_paid_queue_and_rejects_wrong_identity() {
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::SnapshotProcessState;
    for (family, category) in [
        (ObjectCategory::Infantry, ProductionCategory::Infantry),
        (ObjectCategory::Vehicle, ProductionCategory::Ship),
    ] {
        let mismatched_naval = category == ProductionCategory::Ship;
        let (mut live, owner) = world(true, false);
        let mut commands = [
            produce(&live, owner, family, "E1", None),
            produce(&live, owner, family, "E1", None),
        ];
        if mismatched_naval {
            assert!(
                !live
                    .resources
                    .rules
                    .object_in_category(family, "E1")
                    .unwrap()
                    .naval
            );
            for command in &mut commands {
                let Command::SynchronizedRecord { record } = &mut command.payload else {
                    unreachable!();
                };
                let mut packet = ProductionRecord::decode(record).unwrap();
                // Event4C711B tests the full DWORD; HouseBegin independently
                // puts this nonnaval Unit in its Ship slot4FA573.
                packet.naval = 256;
                packet.write_into(record);
            }
        }
        frame(&mut live, owner, &commands);
        for _ in 0..12 {
            frame(&mut live, owner, &[]);
        }
        let product = held(&live.simulation, owner, category).unwrap();
        assert_type(&live, product, family, "E1");
        let factory = live
            .simulation
            .production
            .factories()
            .view(owner, category)
            .unwrap();
        assert!(factory.progress > 0 && factory.progress < PRODUCTION_STEPS);
        assert_eq!(factory.queue.len(), 1);
        assert!(live.simulation.houses[&owner].economy().spent_credits() > 0);
        // Match native player-load resets of Scenario RNG and the unsaved
        // planner before comparing whole state and continuing both copies.
        live.simulation.scenario_rng = crate::sim::rng::SimRng::new(0);
        live.simulation.reset_planning_after_player_load();
        let rules = &live.resources.rules;
        // Validate player-load admission against the supplied synthetic map
        // hash and this scene's rules/session metadata. Resource-backed app
        // loading remains outside this fixture.
        let bytes = GameSnapshot::save_validated(
            &live.simulation,
            0,
            rules.simulation_config_hash(),
            "typed-build",
            0,
        );
        let mut restored = GameSnapshot::load_validated(
            &bytes,
            0,
            rules.simulation_config_hash(),
            live.simulation.session.map_name(),
        )
        .unwrap()
        .sim;
        restored.retain_process_state_for_load(SnapshotProcessState::Live(&live.simulation));
        restored.restore_after_snapshot_load().unwrap();
        restored.resolve_type_handles(rules);
        restored.rebuild_caches_after_load(
            live.simulation.resolved_terrain.as_ref().unwrap().clone(),
            live.simulation.terrain_speed_config().clone(),
            rules,
        );
        assert!(restored.rebuild_dynamic_navigation(rules));
        validate_restored_factory_state(&restored, rules).unwrap();
        assert_eq!(restored.state_hash(), live.simulation.state_hash());
        // Only malformed decoder inputs are mutated. Loosening Unit's Naval
        // relation must not loosen interned-owner/type/concrete-class/limbo integrity.
        // A different valid House may own a Factory-held limbo product.
        for corruption in 0..4 {
            let mut bad = GameSnapshot::load_validated(
                &bytes,
                0,
                rules.simulation_config_hash(),
                live.simulation.session.map_name(),
            )
            .unwrap()
            .sim;
            bad.resolve_type_handles(rules);
            let invalid_owner = InternedId::from_index(u32::MAX);
            let wrong_type = bad.interner.get("HOST").unwrap();
            let entity = bad.substrate.entities.get_mut(product).unwrap();
            match corruption {
                0 => entity.set_owner_for_test(invalid_owner),
                1 => entity.set_type_ref_for_test(wrong_type),
                2 => entity.set_category_for_test(if family == ObjectCategory::Infantry {
                    EntityCategory::Unit
                } else {
                    EntityCategory::Infantry
                }),
                _ => entity.lifecycle.set_in_limbo_for_test(false),
            }
            assert!(validate_restored_factory_state(&bad, rules).is_err());
        }
        let mut restored = SimRuntime {
            simulation: restored,
            resources: SimResources::new(rules.clone()),
        };
        let mut finished = false;
        for _ in 0..2048 {
            frame(&mut live, owner, &[]);
            frame(&mut restored, owner, &[]);
            assert_eq!(
                live.simulation.state_hash(),
                restored.simulation.state_hash()
            );
            assert_eq!(
                live.simulation.scenario_rng.logical_state(),
                restored.simulation.scenario_rng.logical_state()
            );
            assert_eq!(
                live.simulation.main_rng.logical_state(),
                restored.simulation.main_rng.logical_state()
            );
            assert_eq!(
                live.simulation.mapgen_rng.logical_state(),
                restored.simulation.mapgen_rng.logical_state()
            );
            let complete = if mismatched_naval {
                live.simulation
                    .production
                    .factories()
                    .view(owner, category)
                    .unwrap()
                    .complete_object()
                    .is_some()
            } else {
                !live
                    .simulation
                    .entities()
                    .get(product)
                    .unwrap()
                    .lifecycle
                    .in_limbo()
            };
            if complete {
                finished = true;
                break;
            }
        }
        assert!(finished);
        if mismatched_naval {
            for runtime in [&mut live, &mut restored] {
                // Strip6A8ECB emits the actual object's own Naval=false,
                // while HousePlace4FB140 selects that event's Vehicle slot.
                let record = runtime
                    .simulation
                    .encode_place_mobile_record(owner, category, &runtime.resources.rules)
                    .unwrap();
                let packet = PlaceRecord::decode(&record).unwrap();
                assert_eq!(
                    (
                        packet.rtti,
                        packet.type_index,
                        packet.naval,
                        packet.x,
                        packet.y
                    ),
                    (1, -1, 0, 0, 0)
                );
                let command =
                    CommandEnvelope::synchronized(record, runtime.simulation.session.tick() + 1);
                frame(runtime, owner, &[command]);
                assert_eq!(held(&runtime.simulation, owner, category), Some(product));
                assert!(held(&runtime.simulation, owner, ProductionCategory::Vehicle).is_none());
                assert!(
                    runtime
                        .simulation
                        .entities()
                        .get(product)
                        .unwrap()
                        .lifecycle
                        .in_limbo()
                );
                assert_type(runtime, product, family, "E1");
                // A raw matching-slot wildcard cancels the head, rather than
                // the indexed ABANDON arm that would remove a queued copy.
                let record = runtime
                    .simulation
                    .encode_selected_production_record(
                        owner,
                        ProductionEvent::Abandon,
                        ProductionSelection {
                            rtti: 1,
                            type_index: -1,
                            naval: true,
                        },
                    )
                    .unwrap();
                let command =
                    CommandEnvelope::synchronized(record, runtime.simulation.session.tick() + 1);
                frame(runtime, owner, &[command]);
                assert!(!runtime.simulation.entities().contains(product));
                assert!(held(&runtime.simulation, owner, category).is_none());
                // StartNext4CA5F5 reissues the popped type's own Naval=false;
                // its successor therefore starts in Vehicle, not the old slot.
                let successor =
                    held(&runtime.simulation, owner, ProductionCategory::Vehicle).unwrap();
                assert_type(runtime, successor, family, "E1");
            }
        }
        let successor_category = if mismatched_naval {
            ProductionCategory::Vehicle
        } else {
            category
        };
        let successor = held(&live.simulation, owner, successor_category).unwrap();
        assert_ne!(successor, product);
        assert_eq!(
            held(&restored.simulation, owner, successor_category),
            Some(successor)
        );
        for runtime in [&live, &restored] {
            assert_type(runtime, successor, family, "E1");
            validate_restored_factory_state(&runtime.simulation, &runtime.resources.rules).unwrap();
        }
        for runtime in [&mut live, &mut restored] {
            let type_id = runtime.simulation.interner.get("E1").unwrap();
            let object = runtime
                .resources
                .rules
                .object_in_category(family, "E1")
                .unwrap();
            let selection =
                ProductionSelection::for_type(&runtime.resources.rules, object).unwrap();
            let record = runtime
                .simulation
                .encode_selected_production_record(owner, ProductionEvent::Abandon, selection)
                .unwrap();
            let command =
                CommandEnvelope::synchronized(record, runtime.simulation.session.tick() + 1);
            frame(runtime, owner, &[command]);
            assert!(!runtime.simulation.entities().contains(successor));
            assert!(held(&runtime.simulation, owner, successor_category).is_none());
            assert_eq!(
                runtime.simulation.houses[&owner]
                    .tracking()
                    .owned_count(EntityCategory::from(family), type_id),
                if mismatched_naval { 0 } else { 1 }
            );
            validate_restored_factory_state(&runtime.simulation, &runtime.resources.rules).unwrap();
        }
        assert_eq!(
            live.simulation.state_hash(),
            restored.simulation.state_hash()
        );
    }
}

#[test]
fn name_only_compatibility_and_shared_creators_keep_their_native_type_families() {
    let (mut runtime, owner) = world(true, true);
    let rules = &runtime.resources.rules;
    assert_eq!(
        rules.object("E1").unwrap().category,
        ObjectCategory::Vehicle
    );
    let type_id = runtime.simulation.interner.get("E1").unwrap();
    let record = runtime
        .simulation
        .encode_production_record(owner, ProductionEvent::Produce, type_id, rules)
        .unwrap();
    let decoded = crate::sim::command::ProductionRecord::decode(&record).unwrap();
    assert_eq!(decoded.rtti, 40);
    // Semantic QueueProduction remains deliberately name-only.
    let command = CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick() + 1,
        Command::QueueProduction { type_id },
    );
    frame(&mut runtime, owner, &[command]);
    let vehicle = held(&runtime.simulation, owner, ProductionCategory::Vehicle).unwrap();
    assert_type(&runtime, vehicle, ObjectCategory::Vehicle, "E1");
    // Independent typed Infantry production must clone the actual product's
    // type at444A2B, despite the same-name UnitType in the broad registry.
    let command = produce(&runtime, owner, ObjectCategory::Infantry, "E1", None);
    frame(&mut runtime, owner, &[command]);
    let infantry = held(&runtime.simulation, owner, ProductionCategory::Infantry).unwrap();
    for _ in 0..2048 {
        frame(&mut runtime, owner, &[]);
        if !runtime
            .simulation
            .entities()
            .get(infantry)
            .unwrap()
            .lifecycle
            .in_limbo()
        {
            break;
        }
    }
    let copies: Vec<_> = runtime
        .simulation
        .entities()
        .values()
        .filter(|entity| {
            entity.type_ref() == type_id && entity.category() == EntityCategory::Infantry
        })
        .map(|entity| entity.stable_id())
        .collect();
    assert_eq!(copies.len(), 2, "source and ordinary vat copy");
    for id in copies {
        assert_type(&runtime, id, ObjectCategory::Infantry, "E1");
        assert!(
            !runtime
                .simulation
                .entities()
                .get(id)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
    }
    // An ordinary Anim constructor plus its producer's SetOwnerHouse prior,
    // then normal frame AI; no direct MakeInfantry leaf call or completed flag.
    let anim_type = runtime.simulation.interner.intern("CHANGE");
    let anim = runtime
        .simulation
        .spawn_anim_object(
            &runtime.resources.rules,
            crate::sim::components::AnimClassSpawnDescriptor::new(
                anim_type,
                26,
                26,
                crate::util::fixed_math::SIM_HALF,
                crate::util::fixed_math::SIM_HALF,
                0,
            ),
        )
        .unwrap();
    assert!(
        runtime
            .simulation
            .substrate
            .set_anim_owner_house(anim, owner)
    );
    for _ in 0..64 {
        frame(&mut runtime, owner, &[]);
        if runtime.simulation.anim(anim).is_none() {
            break;
        }
    }
    assert!(runtime.simulation.anim(anim).is_none());
    let converted: Vec<_> = runtime
        .simulation
        .entities()
        .values()
        .filter(|entity| {
            entity.type_ref() == type_id && entity.category() == EntityCategory::Infantry
        })
        .map(|entity| entity.stable_id())
        .collect();
    assert_eq!(
        converted.len(),
        3,
        "AnimToInfantry preserved InfantryType creator"
    );
    for id in converted {
        assert_type(&runtime, id, ObjectCategory::Infantry, "E1");
        assert!(
            !runtime
                .simulation
                .entities()
                .get(id)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
    }
    // Known-family constructor children: Enslaves714DD7/67BAC0 and
    // Spawns714E9E/67BD30, both with names also registered as UnitTypes.
    let command = produce(&runtime, owner, ObjectCategory::Vehicle, "HOST", None);
    frame(&mut runtime, owner, &[command]);
    // The preceding name-only vehicle may already have left; HOST can either
    // start immediately or be promoted by its ordinary paid predecessor.
    for _ in 0..2048 {
        let active = held(&runtime.simulation, owner, ProductionCategory::Vehicle).unwrap();
        if runtime.simulation.interner.resolve(
            runtime
                .simulation
                .entities()
                .get(active)
                .unwrap()
                .type_ref(),
        ) == "HOST"
        {
            break;
        }
        frame(&mut runtime, owner, &[]);
    }
    let host = held(&runtime.simulation, owner, ProductionCategory::Vehicle).unwrap();
    assert_type(&runtime, host, ObjectCategory::Vehicle, "HOST");
    let entity = runtime.simulation.entities().get(host).unwrap();
    let slaves: Vec<_> = entity.slave_manager.as_ref().unwrap().slaves().collect();
    let spawns: Vec<_> = entity
        .spawn_manager
        .as_ref()
        .unwrap()
        .slots()
        .iter()
        .filter_map(|slot| slot.spawn())
        .collect();
    assert_eq!((slaves.len(), spawns.len()), (1, 1));
    assert_type(&runtime, slaves[0], ObjectCategory::Infantry, "SLAV");
    assert_type(&runtime, spawns[0], ObjectCategory::Aircraft, "HORN");
    assert!(
        runtime
            .simulation
            .entities()
            .get(spawns[0])
            .unwrap()
            .type_facts
            .spawned_non_missile_aircraft()
    );
}
