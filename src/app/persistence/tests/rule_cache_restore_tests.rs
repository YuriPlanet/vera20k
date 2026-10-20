use super::*;

fn rules_with_unspawned_types() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nBridgeExplosions=EXP_A,EXP_B\nMetallicDebris=DEBRIS_A,DEBRIS_B\n\
         [InfantryTypes]\n0=UNSPAWNED_INF\n[UNSPAWNED_INF]\nStrength=100\n\
         [VehicleTypes]\n0=UNSPAWNED_UNIT\n[UNSPAWNED_UNIT]\nStrength=200\n\
         [AircraftTypes]\n0=UNSPAWNED_AIR\n[UNSPAWNED_AIR]\nStrength=300\n\
         [BuildingTypes]\n0=UNSPAWNED_BUILDING\n[UNSPAWNED_BUILDING]\nStrength=400\n",
    ))
    .expect("parsed rule-name and bridge-list fixture")
}

/// Use the real repository transaction, retaining a different live interner.
/// These are same-content worlds: names and numeric IDs may differ even when
/// map/rules validation succeeds (ordinary app versus headless construction).
fn restore_into_differently_interned_world(label: &str) -> crate::sim::runtime::SimRuntime {
    let rules = rules_with_unspawned_types();
    let mut saved = load_fixture_simulation(true);
    let player = saved.intern("Player");
    assert_eq!(player.index(), 0);
    // Deliberately omit unspawned types, as the headless loader formerly did.
    for name in ["DEBRIS_B", "EXP_B", "SavedOnly", "EXP_A", "DEBRIS_A"] {
        saved.intern(name);
    }
    let saved_strings: Vec<_> = (0..saved.interner().len())
        .map(|index| {
            saved
                .interner()
                .resolve(crate::sim::intern::InternedId::from_index(index as u32))
                .to_owned()
        })
        .collect();
    let mut current = load_fixture_simulation(true);
    current.intern("OutgoingOnly");
    current.intern_rule_type_ids(&rules);
    *current.bridge_explosions_mut_for_test() = rules
        .bridge_rules
        .explosions
        .iter()
        .map(|name| current.intern(name))
        .collect();
    *current.metallic_debris_mut_for_test() = rules
        .general
        .metallic_debris
        .iter()
        .map(|name| current.intern(name))
        .collect();
    assert_ne!(
        current.interner().get("EXP_A"),
        saved.interner().get("EXP_A")
    );
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(current);
    runtime.resources.rules = rules;
    runtime.resources.overlay_registry = OverlayTypeRegistry::empty();
    runtime.resources.terrain_template = Some(load_fixture_terrain());
    let directory = isolated_directory(label);
    let repository = SaveRepository::at(&directory);
    let path = repository
        .write_named(
            "rules.bin",
            &snapshot_bytes(&saved, &runtime.resources.rules),
        )
        .expect("write saved-interner fixture");
    let outgoing_hash = runtime.simulation.state_hash();
    let outgoing_rng = runtime.simulation.rng_state();
    let prepared = PreparedLoad::from_repository(
        LoadPreparationView::from_runtime(&repository, Some(&runtime), Some(LOAD_FIXTURE_MAP_HASH)),
        &path,
    )
    .expect("same content admits an independently interned snapshot");
    assert_eq!(runtime.simulation.state_hash(), outgoing_hash);
    assert_eq!(runtime.simulation.rng_state(), outgoing_rng);
    prepared.commit_into(&mut runtime);
    for (index, name) in saved_strings.iter().enumerate() {
        assert_eq!(
            runtime
                .simulation
                .interner()
                .resolve(crate::sim::intern::InternedId::from_index(index as u32)),
            name,
            "cache rebinding must preserve every saved identity"
        );
    }
    assert!(runtime.simulation.interner().get("OutgoingOnly").is_none());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
    runtime
}

#[test]
fn prepared_load_resolves_bridge_lists_against_saved_interner() {
    let runtime = restore_into_differently_interned_world("bridge-list-cache-restore");
    let sim = &runtime.simulation;
    let names = |ids: &[crate::sim::intern::InternedId]| {
        ids.iter()
            .map(|id| sim.interner().try_resolve(*id))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(sim.bridge_explosions()),
        [Some("EXP_A"), Some("EXP_B")]
    );
    assert_eq!(
        names(sim.metallic_debris()),
        [Some("DEBRIS_A"), Some("DEBRIS_B")]
    );
}

#[test]
fn prepared_load_interns_unspawned_rule_types_before_building_handles() {
    let runtime = restore_into_differently_interned_world("unspawned-type-cache-restore");
    let sim = &runtime.simulation;
    let player = sim.interner().get("Player").unwrap();
    for name in [
        "UNSPAWNED_INF",
        "UNSPAWNED_UNIT",
        "UNSPAWNED_AIR",
        "UNSPAWNED_BUILDING",
    ] {
        let id = sim
            .interner()
            .get(name)
            .expect("unspawned rule type is interned");
        assert_ne!(id, player, "build options must not alias fallback ID zero");
        assert_eq!(sim.interner().resolve(id), name);
        assert_eq!(
            sim.object_type(id, &runtime.resources.rules).unwrap().id,
            name
        );
    }
}
