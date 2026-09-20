use super::*;
use crate::sim::intern::InternedId;
use crate::sim::production::{Factory, PRODUCTION_STEPS, ProductionCategory};

fn factory_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=SLAV\n1=E1\n\
         [VehicleTypes]\n0=MTNK\n[AircraftTypes]\n0=HORN\n\
         [BuildingTypes]\n0=PARENT\n1=OTHER\n2=DEFENSE\n\
         [PARENT]\nCost=1000\nStrength=2000\nFoundation=1x1\nTechLevel=1\n\
         Enslaves=SLAV\nSlavesNumber=2\nSlaveRegenRate=500\nSlaveReloadRate=25\n\
         [OTHER]\nCost=500\nStrength=1000\nFoundation=1x1\nTechLevel=1\nFactory=BuildingType\n\
         [DEFENSE]\nCost=500\nStrength=1000\nFoundation=1x1\nBuildCat=Combat\nTechLevel=1\n\
         [SLAV]\nStrength=125\nSpeed=4\nStorage=4\n\
         [E1]\nCost=200\nStrength=125\nSpeed=4\nTechLevel=1\n\
         [MTNK]\nCost=700\nStrength=300\nSpeed=6\nTechLevel=1\nSpawns=HORN\nSpawnsNumber=2\n\
         [HORN]\nStrength=75\nSpeed=14\n",
    ))
    .expect("factory restore fixture rules")
}

fn factory_fixture(
    type_name: &str,
    category: ProductionCategory,
) -> (Simulation, RuleSet, InternedId, u64) {
    let rules = factory_rules();
    let mut saved = load_fixture_simulation(true);
    saved.intern_rule_type_ids(&rules);
    saved.resolve_type_handles(&rules);
    let owner = saved.interner.intern("Americans");
    saved.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 50_000, 10),
    );
    let parent = saved
        .construct_object_limbo_at_height(type_name, "Americans", 0, 0, 0, 0, &rules)
        .expect("construct actual held graph");
    let type_id = saved.interner.get(type_name).unwrap();
    let cost = rules.object(type_name).unwrap().cost;
    assert!(
        saved
            .production
            .factory_shadow
            .test_enqueue_kernel(owner, category, type_id, 1, 100, cost,)
    );
    saved
        .production
        .factory_shadow
        .test_factory_mut(owner, category)
        .unwrap()
        .object
        .as_mut()
        .unwrap()
        .entity_id = Some(parent);
    saved.production.next_enqueue_order = 2;
    (saved, rules, owner, parent)
}

fn prepare_saved(
    saved: &Simulation,
    rules: RuleSet,
    label: &str,
) -> Result<PreparedLoad, PreparedLoadError> {
    let mut state = RunningMatchTestState::running(&rules);
    let directory = isolated_directory(label);
    let repository = SaveRepository::at(&directory);
    let path = repository
        .write_named("factory.bin", &snapshot_bytes(saved, &rules))
        .expect("write matching-header snapshot");
    state.runtime.resources.rules = rules;
    state.runtime.resources.overlay_registry = OverlayTypeRegistry::empty();
    state.runtime.resources.terrain_template = Some(load_fixture_terrain());
    let before = state.baseline();
    let result = PreparedLoad::from_repository(
        LoadPreparationView::from_runtime(
            &repository,
            Some(&state.runtime),
            Some(LOAD_FIXTURE_MAP_HASH),
        ),
        &path,
    );
    assert_eq!(
        state.baseline(),
        before,
        "preparation mutated running match: {label}"
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&directory).unwrap();
    result
}

#[test]
fn wallet_survives_prepared_load_and_active_cancellation() {
    let (mut saved, rules, owner, _) = factory_fixture("PARENT", ProductionCategory::Building);
    // An on-map factory keeps the held build eligible after restore. Its
    // placement uses the same raw fixture boundary as production replay tests.
    let producer_id = saved.allocate_stable_id();
    let producer_type = saved.interner.get("OTHER").unwrap();
    let mut producer = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        producer_id,
        0,
        0,
        0,
        0,
        owner,
        crate::sim::components::Health { current: 1000 },
        producer_type,
        crate::map::entities::EntityCategory::Structure,
        0,
        5,
        false,
    );
    producer.lifecycle.in_limbo = false;
    producer.in_playfield = true;
    saved.substrate.entities.insert(producer);
    saved.add_entity_occupancy(producer_id);
    saved.houses.get_mut(&owner).unwrap().owned_building_count = 1;
    // Seed a partially paid held object, then exercise a different live credit
    // writer before saving. The old factory balance could be stale at this edge.
    for _ in 0..5 {
        let house = saved.houses.get_mut(&owner).unwrap();
        saved
            .production
            .factory_shadow
            .test_factory_mut(owner, ProductionCategory::Building)
            .unwrap()
            .advance_one_step(&mut house.economy);
    }
    crate::sim::credit_income::add_credits(&mut saved, owner, 123);
    saved
        .houses
        .get_mut(&owner)
        .unwrap()
        .economy
        .harvested_credits = 35;
    let expected = saved.houses[&owner].economy.clone();
    assert!(expected.spent_credits > 0);
    assert_eq!(expected.credits + expected.spent_credits, 50_123);

    let prepared = prepare_saved(&saved, rules, "wallet-authority").expect("prepare current save");
    let mut resources = crate::sim::runtime::SimResources::empty();
    resources.rules = factory_rules();
    let mut runtime = crate::sim::runtime::SimRuntime {
        simulation: Simulation::new(),
        resources,
    };
    prepared.commit_into(&mut runtime);
    assert_eq!(runtime.simulation.houses[&owner].economy, expected);
    let mut reference_resources = crate::sim::runtime::SimResources::empty();
    reference_resources.rules = factory_rules();
    let mut reference = crate::sim::runtime::SimRuntime {
        simulation: saved,
        resources: reference_resources,
    };
    for frame in 0..30 {
        runtime
            .advance_frame(&[], 67, crate::sim::world::TickLane::Ordinary)
            .expect("restored active frame");
        reference
            .advance_frame(&[], 67, crate::sim::world::TickLane::Ordinary)
            .expect("uninterrupted active frame");
        assert_eq!(
            runtime.simulation.houses[&owner].economy, reference.simulation.houses[&owner].economy,
            "wallet at resumed frame {frame}"
        );
        assert_eq!(
            runtime.simulation.production.factory_shadow,
            reference.simulation.production.factory_shadow,
            "factory at resumed frame {frame}"
        );
    }
    let resumed = runtime.simulation.houses[&owner].economy.clone();
    assert!(
        resumed.spent_credits > expected.spent_credits,
        "restored production must charge again"
    );
    assert_eq!(resumed.credits + resumed.spent_credits, 50_123);
    let factory = runtime
        .simulation
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Building)
        .expect("active factory retained");
    assert!(factory.progress > 5 && factory.progress < PRODUCTION_STEPS);
    assert!(crate::sim::production::cancel_by_type_for_owner(
        &mut runtime.simulation,
        &runtime.resources.rules,
        "Americans",
        "PARENT"
    ));
    assert_eq!(
        crate::sim::production::credits_for_owner(&runtime.simulation, "Americans"),
        50_123
    );
    runtime
        .advance_frame(&[], 67, crate::sim::world::TickLane::Ordinary)
        .expect("restored frame");
    let economy = &runtime.simulation.houses[&owner].economy;
    assert_eq!(economy.credits, 50_123);
    assert_eq!(economy.spent_credits, resumed.spent_credits);
    assert_eq!(economy.harvested_credits, 35);
}

#[test]
fn factory_restore_rejects_unrelated_ready_projection_before_match_commit() {
    let (mut saved, rules, owner, parent) = factory_fixture("PARENT", ProductionCategory::Building);
    assert_eq!(saved.production.slave_bindings[&parent].len(), 2);
    let missing = saved.interner.intern("REMOVED_TYPE");
    saved
        .production
        .ready_by_owner
        .insert(owner, [missing].into());
    let error = match prepare_saved(&saved, rules, "factory-invalid-ready") {
        Ok(_) => panic!("malformed ready projection was admitted"),
        Err(error) => error,
    };
    assert!(
        matches!(error, PreparedLoadError::FactoryState(ref error)
        if error.owner == owner && error.reason == "ready type is absent from bound rules"),
        "{error}"
    );
}

#[test]
fn factory_restore_preserves_supported_held_states_and_constructor_graphs() {
    for (label, type_name, category) in [
        ("active-tail", "PARENT", ProductionCategory::Building),
        (
            "unpublished-complete",
            "PARENT",
            ProductionCategory::Building,
        ),
        ("ready-building", "PARENT", ProductionCategory::Building),
        ("ready-defense", "DEFENSE", ProductionCategory::Defense),
        ("mobile-retry", "MTNK", ProductionCategory::Vehicle),
        ("absent-house", "E1", ProductionCategory::Infantry),
        ("retained-playfield", "E1", ProductionCategory::Infantry),
        ("unrelated-limbo", "PARENT", ProductionCategory::Building),
        ("empty-registry", "PARENT", ProductionCategory::Building),
    ] {
        let (mut saved, rules, owner, parent) = factory_fixture(type_name, category);
        match label {
            "active-tail" => {
                let type_id = saved.interner.get(type_name).unwrap();
                assert!(
                    !saved
                        .production
                        .factory_shadow
                        .test_enqueue_kernel(owner, category, type_id, 2, 100, 1000)
                );
                saved.production.next_enqueue_order = 3;
            }
            "unpublished-complete" | "ready-building" | "ready-defense" | "mobile-retry" => {
                assert!(
                    saved
                        .production
                        .factory_shadow
                        .test_arm_ready(owner, category)
                );
                if label != "unpublished-complete" {
                    assert!(!crate::sim::production::tick_production(
                        &mut saved,
                        &rules,
                        &Default::default(),
                        None
                    ));
                    assert!(
                        saved
                            .production
                            .factory_shadow
                            .view(owner, category)
                            .unwrap()
                            .object
                            .unwrap()
                            .completion_accounted
                    );
                }
            }
            "retained-playfield" => {
                saved
                    .substrate
                    .entities
                    .get_mut(parent)
                    .unwrap()
                    .in_playfield = true;
            }
            "absent-house" => {
                saved.houses.remove(&owner);
            }
            "unrelated-limbo" => {
                saved
                    .construct_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, &rules)
                    .unwrap();
            }
            "empty-registry" => {
                saved.production.factory_shadow = Default::default();
            }
            _ => unreachable!(),
        }
        let factories: Vec<Factory> = saved
            .production
            .factory_shadow
            .iter_insertion_ordered()
            .into_iter()
            .cloned()
            .collect();
        let ready = saved.production.ready_by_owner.clone();
        // MatchStatistics is skipped by the existing snapshot format; this
        // admission change preserves the serialized counts and wallet only.
        let counts = saved.houses.get(&owner).map(|house| {
            (
                house.owned_building_count,
                house.owned_unit_count,
                house.economy.credits,
            )
        });
        let identities: Vec<_> = saved
            .substrate
            .entities
            .values()
            .map(|entity| {
                (
                    entity.stable_id(),
                    entity.type_ref,
                    entity.owner(),
                    entity.health.current,
                    entity.dying,
                    entity.lifecycle,
                    entity.in_playfield,
                    entity.techno_ctor_random_word,
                    entity.spawn_owner_id,
                    entity.spawn_manager.as_ref().map(|manager| {
                        manager
                            .slots
                            .iter()
                            .map(|slot| slot.spawn)
                            .collect::<Vec<_>>()
                    }),
                    entity.slave_harvester.as_ref().map(|slave| slave.master_id),
                )
            })
            .collect();
        let prepared =
            prepare_saved(&saved, rules, label).unwrap_or_else(|error| panic!("{label}: {error}"));
        let restored = &prepared.simulation;
        assert_eq!(
            restored
                .production
                .factory_shadow
                .iter_insertion_ordered()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            factories,
            "{label}"
        );
        assert_eq!(restored.production.ready_by_owner, ready, "{label}");
        assert_eq!(
            restored.houses.get(&owner).map(|house| (
                house.owned_building_count,
                house.owned_unit_count,
                house.economy.credits,
            )),
            counts,
            "{label}"
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .values()
                .map(|entity| (
                    entity.stable_id(),
                    entity.type_ref,
                    entity.owner(),
                    entity.health.current,
                    entity.dying,
                    entity.lifecycle,
                    entity.in_playfield,
                    entity.techno_ctor_random_word,
                    entity.spawn_owner_id,
                    entity.spawn_manager.as_ref().map(|manager| manager
                        .slots
                        .iter()
                        .map(|slot| slot.spawn)
                        .collect::<Vec<_>>()),
                    entity.slave_harvester.as_ref().map(|slave| slave.master_id),
                ))
                .collect::<Vec<_>>(),
            identities,
            "{label}"
        );
        assert_eq!(
            restored.production.slave_bindings, saved.production.slave_bindings,
            "{label}"
        );
    }
}

#[test]
fn factory_restore_rejects_inconsistent_roots_and_ready_relationships() {
    for label in [
        "key-owner",
        "idle-owner-index",
        "ready-owner-index",
        "tail-unknown-type",
        "tail-type-index",
        "tail-category",
        "active-category",
        "duplicate-roots",
        "revealed-root",
        "marked-root",
        "logic-root",
        "missing-entity",
        "key-category",
        "no-identity",
        "unknown-active-type",
        "wrong-type",
        "wrong-owner",
        "concrete-category",
        "manager-owned-root",
        "child-root-alias",
        "tail-without-head",
        "ready-no-factory",
        "ready-no-head",
        "ready-mismatch",
        "ready-nonbuilding",
        "duplicate-ready",
        "early-ready",
        "unaccounted-ready",
        "missing-ready",
        "early-accounting",
    ] {
        let (mut saved, rules, owner, parent) =
            factory_fixture("PARENT", ProductionCategory::Building);
        let category = ProductionCategory::Building;
        let parent_type = saved.interner.get("PARENT").unwrap();
        let other = saved.interner.get("OTHER").unwrap();
        let foreign = saved.interner.intern("Russians");
        let missing = saved.interner.intern("REMOVED_TYPE");
        if (label.starts_with("ready-") && label != "ready-owner-index")
            || matches!(
                label,
                "duplicate-ready" | "early-ready" | "unaccounted-ready"
            )
        {
            assert!(
                saved
                    .production
                    .factory_shadow
                    .test_arm_ready(owner, category)
            );
            assert!(!crate::sim::production::tick_production(
                &mut saved,
                &rules,
                &Default::default(),
                None
            ));
        }
        match label {
            "idle-owner-index" => {
                let invalid_owner = InternedId::from_index(u32::MAX);
                saved.production.factory_shadow.test_enqueue_kernel(
                    invalid_owner,
                    category,
                    parent_type,
                    3,
                    100,
                    1000,
                );
                saved
                    .production
                    .factory_shadow
                    .test_factory_mut(invalid_owner, category)
                    .unwrap()
                    .object = None;
            }
            "ready-owner-index" => {
                saved.production.ready_by_owner.clear();
                saved
                    .production
                    .ready_by_owner
                    .insert(InternedId::from_index(u32::MAX), Default::default());
            }
            "tail-unknown-type" | "tail-type-index" | "tail-category" => {
                let queued_type = match label {
                    "tail-unknown-type" => missing,
                    "tail-type-index" => InternedId::from_index(u32::MAX),
                    _ => saved.interner.get("E1").unwrap(),
                };
                assert!(!saved.production.factory_shadow.test_enqueue_kernel(
                    owner,
                    category,
                    queued_type,
                    2,
                    100,
                    200
                ));
            }
            "active-category" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object
                    .as_mut()
                    .unwrap()
                    .type_id = saved.interner.get("E1").unwrap()
            }
            "duplicate-roots" => {
                saved.production.factory_shadow.test_enqueue_kernel(
                    foreign,
                    category,
                    parent_type,
                    3,
                    100,
                    1000,
                );
                saved
                    .production
                    .factory_shadow
                    .test_factory_mut(foreign, category)
                    .unwrap()
                    .object
                    .as_mut()
                    .unwrap()
                    .entity_id = Some(parent);
            }
            "revealed-root" => {
                saved
                    .substrate
                    .entities
                    .get_mut(parent)
                    .unwrap()
                    .lifecycle
                    .in_limbo = false
            }
            "marked-root" => {
                let order = saved.substrate.next_occupancy_enter_order.next();
                let entity = saved.substrate.entities.get_mut(parent).unwrap();
                entity.occupancy_enter_order = order;
                entity.lifecycle.cell_marked = true;
            }
            "logic-root" => saved.set_logic_order_for_test(vec![parent]),
            "missing-entity" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object
                    .as_mut()
                    .unwrap()
                    .entity_id = Some(u64::MAX)
            }
            "key-owner" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .owner = foreign
            }
            "key-category" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .category = ProductionCategory::Infantry
            }
            "no-identity" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object
                    .as_mut()
                    .unwrap()
                    .entity_id = None
            }
            "unknown-active-type" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object
                    .as_mut()
                    .unwrap()
                    .type_id = missing
            }
            "wrong-type" => saved.substrate.entities.get_mut(parent).unwrap().type_ref = other,
            "wrong-owner" => saved.substrate.entities.get_mut(parent).unwrap().owner = foreign,
            "concrete-category" => {
                saved.substrate.entities.get_mut(parent).unwrap().category =
                    crate::map::entities::EntityCategory::Infantry
            }
            "manager-owned-root" => {
                saved
                    .substrate
                    .entities
                    .get_mut(parent)
                    .unwrap()
                    .spawn_owner_id = Some(parent)
            }
            "child-root-alias" => {
                saved.production.slave_bindings.get_mut(&parent).unwrap()[0] = parent
            }
            "tail-without-head" => {
                saved.production.factory_shadow.test_enqueue_kernel(
                    owner,
                    category,
                    parent_type,
                    2,
                    100,
                    1000,
                );
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object = None;
            }
            "ready-no-factory" => saved.production.factory_shadow = Default::default(),
            "ready-no-head" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object = None
            }
            "ready-mismatch" => {
                *saved
                    .production
                    .ready_by_owner
                    .get_mut(&owner)
                    .unwrap()
                    .front_mut()
                    .unwrap() = other
            }
            "ready-nonbuilding" => {
                *saved
                    .production
                    .ready_by_owner
                    .get_mut(&owner)
                    .unwrap()
                    .front_mut()
                    .unwrap() = saved.interner.get("E1").unwrap()
            }
            "duplicate-ready" => saved
                .production
                .ready_by_owner
                .get_mut(&owner)
                .unwrap()
                .push_back(parent_type),
            "early-ready" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .progress = 1
            }
            "unaccounted-ready" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object
                    .as_mut()
                    .unwrap()
                    .completion_accounted = false
            }
            "missing-ready" => {
                let factory = saved.production.factory_shadow.test_first_mut().unwrap();
                factory.progress = PRODUCTION_STEPS;
                factory.object.as_mut().unwrap().completion_accounted = true;
            }
            "early-accounting" => {
                saved
                    .production
                    .factory_shadow
                    .test_first_mut()
                    .unwrap()
                    .object
                    .as_mut()
                    .unwrap()
                    .completion_accounted = true
            }
            _ => unreachable!(),
        }
        match prepare_saved(&saved, rules, label) {
            Err(PreparedLoadError::FactoryState(_)) => {}
            Err(PreparedLoadError::Restore(SnapshotRestoreError::UnresolvedObjectReference {
                source_registry: "FactoryRegistry",
                ..
            })) if label == "missing-entity" => {}
            Err(error) => panic!("{label}: unexpected earlier failure {error}"),
            Ok(_) => panic!("{label}: inconsistent factory was admitted"),
        }
    }
}
