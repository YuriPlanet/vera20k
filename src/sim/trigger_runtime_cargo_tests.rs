//! Scenario cargo switches and Unit73838A's production fatal-receiver consumer.
//! Native control flow: Steam gamemd SHA2563e81a617..08600, Execute97
//! 6DD8EE, Execute87 6DDBD5, Basic68A217/68A235/68A5A7 and Unit73838A.
//! Existing crate/FNPC oracles own numeric values and RNG; these tests do
//! not substitute a hand-computed placement or timer golden.
use super::*;
use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry, ruleset::RuleSet};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::snapshot::GameSnapshot;

const CARGO_RULES: &str = "\
[Countries]\n0=Americans\n[Americans]\nSide=Allied\n\
[VehicleTypes]\n0=CARGOTRAIN\n1=CARGOTRUCK\n2=EMPTYTRAIN\n\
[CARGOTRAIN]\nStrength=100\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nIsTrain=yes\nCarriesCrate=yes\nCrewed=no\n\
[CARGOTRUCK]\nStrength=100\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nCarriesCrate=yes\nCrewed=no\n\
[EMPTYTRAIN]\nStrength=100\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nIsTrain=yes\nCrewed=no\n\
[CrateRules]\nCrateImg=O0\nWoodCrateImg=O0\nWaterCrateImg=O0\nSoloCrateMoney=5000\n\
[O0]\nCrate=yes\nCrateTrigger=yes\n[Powerups]\nMoney=20,<none>,yes,2000\n";

fn fixture() -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let (mut sim, rules, registry) =
        crate::sim::world::entry_test_fixture::fixture_with_rules(CARGO_RULES);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner,
            0,
            Some(owner),
            true,
            0,
            10,
        ));
    sim.session.game_options_mut_for_test().crates = true;
    (sim, rules, registry)
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

fn action(sim: &mut Simulation, kind: i32, value: i32) {
    let ini = IniFile::from_str(&format!("[Actions]\nA=1,{kind},0,{value},0,0,0,0,Z\n"));
    let entry = crate::map::actions::parse_actions(&ini)["A"].entries()[0].clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        None,
        None,
        FrameEffects::default(),
    ));
}

fn kill(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    kind: &str,
) -> [String; 3] {
    let id = sim
        .spawn_object_at_height(kind, "Americans", 15, 15, 0, 0, rules)
        .unwrap();
    let warhead = sim.interner.intern("SA");
    let previous_rng = rng(sim);
    sim.commit_direct_damage_receiver(
        rules,
        Some(registry),
        EntityDamageEvent::direct_receiver(
            id,
            1000,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: true,
            },
        ),
        FrameEffects::default(),
    );
    previous_rng
}

fn crate_count(sim: &Simulation) -> usize {
    sim.crate_authority
        .slots()
        .iter()
        .filter(|slot| !slot.is_empty())
        .count()
}

#[test]
fn train_toggle_ignores_operands_and_ice_stores_low_byte_without_running_ore() {
    let mut sim = Simulation::with_seed(97);
    sim.session.set_binary_frame_for_test(73);
    let original_hash = sim.state_hash();
    let previous_rng = rng(&sim);
    let ore = serde_json::to_value(&sim.ore_growth_state).unwrap();
    let grows = sim.ore_growth_config.grows();
    let spreads = sim.ore_growth_config.spreads();
    for value in [0, -1, 256, i32::MIN] {
        action(&mut sim, 97, value);
        assert!(sim.session.train_crate());
        assert!(!sim.session.truck_crate());
        action(&mut sim, 97, value);
        assert!(!sim.session.train_crate());
        assert_eq!(sim.state_hash(), original_hash);
    }
    for (value, byte) in [
        (0, 0),
        (2, 2),
        (255, 255),
        (256, 0),
        (257, 1),
        (-1, 255),
        (-256, 0),
    ] {
        action(&mut sim, 87, value);
        assert_eq!(
            serde_json::to_value(&sim.session).unwrap()["ice_growth"],
            byte
        );
        assert_eq!(sim.session.ice_growth_enabled(), byte != 0);
        assert_eq!(sim.ore_growth_config.grows(), grows);
        assert_eq!(sim.ore_growth_config.spreads(), spreads);
        assert_eq!(serde_json::to_value(&sim.ore_growth_state).unwrap(), ore);
        assert_eq!(rng(&sim), previous_rng);
        assert_eq!(sim.session.binary_frame(), 73);
    }
    action(&mut sim, 87, 1);
    assert_eq!(sim.state_hash(), original_hash);
}

#[test]
fn map_basic_defaults_exact_keys_and_current_default_inputs_are_preserved() {
    use crate::map::basic::parse_basic_section;
    for text in [
        "",
        "[Basic]\nName=Empty\n",
        "[Basic]\nTrainCrate=\nTruckCrate=?\nIceGrowthEnabled=\n",
        "[Basic]\ntraincrate=yes\ntruckcrate=yes\nicegrowthenabled=no\n",
    ] {
        let basic = parse_basic_section(&IniFile::from_str(text));
        assert_eq!(
            (
                basic.train_crate,
                basic.truck_crate,
                basic.ice_growth_enabled
            ),
            (None, None, None)
        );
    }
    let basic = parse_basic_section(&IniFile::from_str(
        "[Basic]\nTrainCrate=yes\nTruckCrate=true\nIceGrowthEnabled=no\n",
    ));
    let sim = Simulation::from_descriptor(&ScenarioDescriptor {
        train_crate: basic.train_crate.unwrap_or(false),
        truck_crate: basic.truck_crate.unwrap_or(false),
        ice_growth_enabled: basic.ice_growth_enabled,
        ..Default::default()
    });
    assert!(sim.session.train_crate() && sim.session.truck_crate());
    assert!(!sim.session.ice_growth_enabled());
    let fresh = Simulation::new();
    assert!(!fresh.session.train_crate() && !fresh.session.truck_crate());
    assert!(fresh.session.ice_growth_enabled());
}

#[test]
fn cargo_switches_and_noncanonical_ice_byte_survive_snapshot_and_change_hash() {
    let descriptor = ScenarioDescriptor {
        truck_crate: true,
        ..Default::default()
    };
    let mut sim = Simulation::from_descriptor(&descriptor);
    let before = sim.state_hash();
    action(&mut sim, 97, 0);
    action(&mut sim, 87, -1);
    assert_ne!(sim.state_hash(), before);
    let saved = GameSnapshot::save(&sim, 0, 0, "Cargo flags", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    assert!(restored.session.train_crate() && restored.session.truck_crate());
    assert_eq!(
        serde_json::to_value(&restored.session).unwrap()["ice_growth"],
        255
    );
    assert_eq!(sim.state_hash(), restored.state_hash());
    action(&mut restored, 97, 0);
    action(&mut restored, 87, 1);
    assert_eq!(restored.state_hash(), before);
}

#[test]
fn live_house_trigger_enables_train_cargo_before_fatal_receiver_and_pickup() {
    let (mut sim, rules, registry) = fixture();
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nCARGO=Americans,<none>,Cargo,0,1,1,1,0\n\
         [Events]\nCARGO=1,8,0,0\n\
         [Actions]\nCARGO=3,97,0,-1,0,0,0,0,Z,87,0,257,0,0,0,0,Z,56,0,9,0,0,0,0,A\n\
         [Tags]\nCARGOTAG=0,Cargo,CARGO\n",
    );
    let owner = sim.interner.get("Americans").unwrap();
    sim.session.set_binary_frame_for_test(16);
    let previous_rng = rng(&sim);
    sim.raise_house_tags(
        owner,
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(sim.session.train_crate());
    assert!(sim.trigger_runtime.locals_set.contains(&9));
    assert_eq!(rng(&sim), previous_rng);
    let death_rng = kill(&mut sim, &rules, &registry, "CARGOTRAIN");
    assert_eq!(crate_count(&sim), 1);
    assert_ne!(
        rng(&sim)[1],
        death_rng[1],
        "accepted crate timer draws Scenario RNG"
    );
    assert_eq!(rng(&sim)[0], death_rng[0]);
    assert_eq!(rng(&sim)[2], death_rng[2]);
    let slot = *sim
        .crate_authority
        .slots()
        .iter()
        .find(|slot| !slot.is_empty())
        .unwrap();
    let cell = (slot.cell_x as u16, slot.cell_y as u16);
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(cell.0, cell.1)
            .overlay_data,
        20
    );
    let collector = sim
        .spawn_object_at_height("CARGOTRUCK", "Americans", 16, 15, 0, 0, &rules)
        .unwrap();
    let money = crate::sim::credit_income::available_money(&sim, owner);
    assert!(sim.pickup_crate_at(
        collector,
        (slot.cell_x, slot.cell_y),
        Some(&rules),
        Some(&registry),
        FrameEffects::default()
    ));
    // Native56C020's solo arm removes the overlay without clearing slots;
    // retain the already-established crate owner and its timer semantics.
    assert_eq!(crate_count(&sim), 1);
    assert!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(cell.0, cell.1)
            .overlay_id
            .is_none()
    );
    assert!(crate::sim::credit_income::available_money(&sim, owner) > money);
    assert!(
        sim.trigger_runtime.dirty[3],
        "native CrateTrigger still feeds event50"
    );
}

#[test]
fn train_and_truck_flags_are_independent_and_empty_types_drop_nothing() {
    for (kind, train, truck, crates, expected) in [
        ("CARGOTRAIN", false, false, true, 0),
        ("CARGOTRAIN", true, false, true, 1),
        ("CARGOTRAIN", false, true, true, 0),
        ("CARGOTRUCK", true, false, true, 0),
        ("CARGOTRUCK", false, true, true, 1),
        ("EMPTYTRAIN", true, true, true, 0),
        ("CARGOTRAIN", true, true, false, 0),
    ] {
        let (mut sim, rules, registry) = fixture();
        // Use the same descriptor handoff as both map-loading roots.
        sim.session =
            crate::sim::scenario_session::ScenarioSession::from_descriptor(&ScenarioDescriptor {
                train_crate: train,
                truck_crate: truck,
                map_width: 33,
                map_height: 33,
                ..Default::default()
            });
        sim.session.game_options_mut_for_test().crates = crates;
        kill(&mut sim, &rules, &registry, kind);
        assert_eq!(
            crate_count(&sim),
            expected,
            "{kind}: train={train}, truck={truck}, crates={crates}"
        );
    }
}

#[test]
fn carries_crate_uses_unit_defaults_layers_and_retained_process_state() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=CAR\n[InfantryTypes]\n0=MAN\n[CAR]\nName=Car\n[MAN]\nCarriesCrate=yes\n",
    ));
    let defaults = RuleSet::from_rules_layers(&layers).unwrap();
    assert!(!defaults.object("CAR").unwrap().carries_crate());
    assert!(
        !defaults.object("MAN").unwrap().carries_crate(),
        "Unit-only reader"
    );
    for (text, expected) in [
        ("CarriesCrate=yes", true),
        ("CarriesCrate=?", true),
        ("CarriesCrate=", true),
        ("carriescrate=no", true),
        ("CarriesCrate=no", false),
    ] {
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!("[CAR]\n{text}\n")),
        );
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("CAR")
                .unwrap()
                .carries_crate(),
            expected,
            "{text}"
        );
    }
    let continued = |value| {
        let initial = RulesLayerStack::new(IniFile::from_str(&format!(
            "[VehicleTypes]\n0=CAR\n[CAR]\nCarriesCrate={value}\n"
        )))
        .process()
        .unwrap();
        let (_, trace) = initial.into_ini_and_native_type_construction_trace();
        let state = trace.into_registry_state_discarding_events();
        let state = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&state).unwrap(),
        )
        .unwrap();
        let next = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=CAR\n[CAR]\nName=Continued car\n",
        ))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
        .unwrap();
        RuleSet::from_processed_rules(&next).unwrap()
    };
    let carrying = continued("yes");
    let empty = continued("no");
    assert!(carrying.object("CAR").unwrap().carries_crate());
    assert!(!empty.object("CAR").unwrap().carries_crate());
    assert_ne!(
        carrying.simulation_config_hash(),
        empty.simulation_config_hash()
    );
}

#[test]
fn retail_truck_b_retains_cargo_and_map_override_reaches_the_same_reader() {
    use crate::rules::retail_ini_fixture;
    let assert_retail = |rules: &RuleSet| {
        let carriers: Vec<_> = rules
            .all_objects()
            .filter(|object| object.carries_crate())
            .map(|object| object.id.as_str())
            .collect();
        assert_eq!(carriers, ["TRUCKB"]);
        assert!(!rules.object("MTNK").unwrap().carries_crate());
    };
    let Some(root) = retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_retail(&root);
    for map in ["Hills.mmx", "XMP03T4.MAP"] {
        let Some(retail) = retail_ini_fixture::retail_battle_rules_for_map(map) else {
            return;
        };
        assert_retail(&retail.rules);
    }
    let Some(patched) = retail_ini_fixture::retail_battle_rules_with_map_patch(&IniFile::from_str(
        "[TRUCKB]\nCarriesCrate=no\n[MTNK]\nCarriesCrate=yes\n",
    )) else {
        return;
    };
    assert!(!patched.rules.object("TRUCKB").unwrap().carries_crate());
    assert!(patched.rules.object("MTNK").unwrap().carries_crate());
}
