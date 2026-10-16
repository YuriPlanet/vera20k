//! Authored crate placement and the shared pickup-to-Tag/Logic boundary.
use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use serde_json::{Value, json};

fn fixture() -> (Simulation, RuleSet, OverlayTypeRegistry, InternedId, u64) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[Countries]\n0=Americans\n1=Russians\n[Americans]\nSide=Allied\n[Russians]\nSide=Soviet\n\
             [VehicleTypes]\n0=TNK\n[TNK]\nStrength=300\nSpeed=5\nROT=5\n\
             SpeedType=Track\nMovementZone=Normal\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [CrateRules]\nCrateImg=O0\nWoodCrateImg=O0\nWaterCrateImg=O0\n\
             SoloCrateMoney=5000\n[O0]\nCrate=yes\nCrateTrigger=yes\n\
             [Powerups]\nMoney=20,<none>,yes,2000\n",
    );
    sim.playfield_size_height = Some(16);
    sim.session.game_mode_nonzero = false;
    sim.session.game_options.crates = false;
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, Some(owner), true, 0, 10),
    );
    sim.session.house_order.push(owner);
    let actor = sim
        .spawn_object_at_height("TNK", "Americans", 15, 15, 0, 0, &rules)
        .unwrap();
    (sim, rules, registry, owner, actor)
}

fn corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_crate.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    native
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn supply_rng(sim: &mut Simulation, state: &Value) {
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(state["scenario"].as_str().unwrap());
    sim.main_rng = SimRng::from_native_state_hex_for_test(state["main"].as_str().unwrap()).into();
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(state["mapgen"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, state: &Value, name: &str) {
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        state["scenario"],
        "{name}: Scenario RNG"
    );
    assert_eq!(
        sim.main_rng.native_state_hex(),
        state["main"],
        "{name}: Main RNG"
    );
    assert_eq!(
        sim.mapgen_rng.native_state_hex(),
        state["mapgen"],
        "{name}: MapGen RNG"
    );
}

fn slot_words(slot: &crate::sim::crates::CrateSlot) -> Value {
    json!([
        slot.start_frame,
        slot.aux,
        slot.duration,
        slot.cell_x,
        slot.cell_y
    ])
}

fn zero_slot_priors(sim: &mut Simulation) {
    // The native fixture deliberately clears all256 physical slot records.
    // This prior differs from CrateAuthority's ordinary constructor timer -1.
    for index in 0..256 {
        *sim.crate_authority.slot_mut(index) = crate::sim::crates::CrateSlot {
            start_frame: 0,
            aux: 0,
            duration: 0,
            cell_x: 0,
            cell_y: 0,
        };
    }
}

fn install(sim: &mut Simulation, rules: &RuleSet, body: &str) -> crate::map::map_file::MapFile {
    let map = super::tests::install_event_ingress_map(sim, rules, body);
    sim.session.install_waypoints(&map.waypoints);
    map
}

fn poll(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    map: &crate::map::map_file::MapFile,
) {
    let _ = sim.advance_triggers(
        TriggerInputs {
            events: &map.events,
            actions: &map.actions,
            rules: Some(rules),
        },
        Some(registry),
        FrameEffects::default(),
    );
}

#[test]
fn authored_create_crate_action_reaches_existing_placement_owner() {
    let (mut sim, rules, registry, _, _) = fixture();
    let map = install(
        &mut sim,
        &rules,
        "[Triggers]\nCREATE=Americans,<none>,Crate,0,1,1,1,0\n\
         [Events]\nCREATE=1,13,0,0\n\
         [Actions]\nCREATE=1,108,0,0,0,0,0,0,AU\n\
         [Tags]\nCRATE=0,Crate,CREATE\n[Waypoints]\n46=15016\n",
    );
    poll(&mut sim, &rules, &registry, &map);
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(16, 15).overlay_id,
        registry.id_for_name("O0"),
        "the elapsed trigger must place its authored crate at waypoint AU"
    );
    assert!(
        sim.crate_authority
            .slots()
            .iter()
            .any(|slot| (slot.cell_x, slot.cell_y) == (16, 15))
    );
}

#[test]
fn crate_tag_can_kill_collector_before_removal_and_notification() {
    let (mut sim, rules, registry, _, actor) = fixture();
    let _ = install(
        &mut sim,
        &rules,
        "[Triggers]\nKILL=Americans,<none>,Kill,0,1,1,1,0\n\
         [Events]\nKILL=1,49,0,0\n\
         [Actions]\nKILL=1,32,0,0,0,0,0,0,A\n\
         [Tags]\nKILLTAG=0,Kill,KILL\n",
    );
    sim.attach_object_trigger_tag(actor, Some("KILLTAG"));
    let overlay = registry.id_for_name("O0").unwrap();
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(16, 15, overlay, 0);
    assert!(!sim.pickup_crate_at(
        actor,
        (16, 15),
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    ));
    assert!(
        sim.substrate
            .entities
            .get(actor)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    );
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(16, 15).overlay_id,
        Some(overlay)
    );
    assert!(!sim.trigger_runtime.dirty[3]);
}

#[test]
fn untagged_crate_pickup_notifies_logic_event50_once() {
    let (mut sim, rules, registry, _, actor) = fixture();
    let map = install(
        &mut sim,
        &rules,
        "[Triggers]\nPICKED=Americans,<none>,Picked,0,1,1,1,0\n\
         [Events]\nPICKED=1,50,0,0\n\
         [Actions]\nPICKED=1,56,0,7,0,0,0,0,A\n\
         [Tags]\nPICKEDTAG=0,Picked,PICKED\n",
    );
    sim.overlay_grid.as_mut().unwrap().place_overlay(
        16,
        15,
        registry.id_for_name("O0").unwrap(),
        0,
    );
    assert!(sim.pickup_crate_at(
        actor,
        (16, 15),
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    ));
    assert!(sim.trigger_runtime.dirty[3]);
    assert!(!sim.trigger_runtime.locals_set.contains(&7));
    poll(&mut sim, &rules, &registry, &map);
    assert!(sim.trigger_runtime.locals_set.contains(&7));
    assert!(!sim.trigger_runtime.dirty[3]);
}

/// Steam3e81a617 Execute6DF69B ->68BCC0 ->56BEC0. The native
/// corpus supplies a flat interior scene; the fixture's padding row/column is
/// immaterial to these eight choices. No retail geometry is substituted as a
/// golden: selected cells, AL, five slot words and all RNG bytes are native.
#[test]
fn authored_placement_matches_eight_original_controls_and_all_rng_streams() {
    let native = corpus();
    let cases = native["placement_controls"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let input = &case["input"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, mut rules, registry, _, _) = fixture();
        // Explicit oracle prior; Rules' constructor default is10, whereas
        // this bounded native matrix supplies Rules+1678=3.0.
        rules.crate_rules.regen = crate::util::native_x87::NativeF64Bits::from_bits(
            input["regen_minutes"].as_f64().unwrap_or(3.0).to_bits(),
        );
        sim.session.binary_frame = 100;
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(1000));
        zero_slot_priors(&mut sim);
        if input["slots_full"] == true {
            for index in 0..256 {
                *sim.crate_authority.slot_mut(index) = crate::sim::crates::CrateSlot {
                    start_frame: 50,
                    aux: 777,
                    duration: 100,
                    cell_x: 5,
                    cell_y: 5,
                };
            }
        }
        if input["water"] == true {
            use crate::rules::terrain_rules::{LandType, TerrainRules};
            // make_source_fixture supplies the actual global speed table:
            // land0's rows are1.0, every other retained land row is0.0.
            // None means an unavailable profile in Rust, not native zero.
            let mut text = String::new();
            for land in LandType::ALL {
                text.push_str(&format!("[{}]\n", land.section_name()));
                for key in [
                    "Foot",
                    "Track",
                    "Wheel",
                    "Float",
                    "Amphibious",
                    "FloatBeach",
                    "Hover",
                ] {
                    text.push_str(&format!(
                        "{key}={}%\n",
                        if land == LandType::Clear { 100 } else { 0 }
                    ));
                }
            }
            let speed_rules =
                TerrainRules::from_ini(&crate::rules::ini_parser::IniFile::from_str(&text));
            let costs = speed_rules.semantics_for_land_type(2).unwrap().speed_costs;
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            terrain.set_land_speed_rules_for_test(&speed_rules);
            for y in 0..33 {
                for x in 0..33 {
                    let c = terrain.cell_mut(x, y).unwrap();
                    c.land_type = 2;
                    c.yr_cell_land_type = 2;
                    c.base_land_type = 2;
                    c.base_yr_cell_land_type = 2;
                    c.is_water = true;
                    c.speed_costs = costs;
                    c.base_speed_costs = costs;
                }
            }
        }
        if input["all_occupied"] == true {
            for y in 0..33 {
                for x in 0..33 {
                    sim.overlay_grid.as_mut().unwrap().place_overlay(x, y, 0, 0);
                }
            }
        }
        let waypoint = if input["missing_waypoint"] == true {
            String::new()
        } else {
            let xy = &case["native_waypoint"];
            format!(
                "[Waypoints]\n0={}\n",
                integer(&xy[1]) * 1000 + integer(&xy[0])
            )
        };
        let map = install(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nCREATE=Americans,<none>,Create,0,1,1,1,0\n\
             [Events]\nCREATE=1,13,0,0\n[Tags]\nCREATE_TAG=0,Create,CREATE\n\
             [Actions]\nCREATE=1,{}\n{waypoint}",
                case["native_read"]["raw"].as_str().unwrap()
            ),
        );
        let typ = sim
            .trigger_runtime
            .definitions
            .trigger_type("CREATE")
            .unwrap();
        let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
        assert_eq!(
            action.value.unwrap(),
            integer(&case["native_read"]["value"])
        );
        assert_eq!(
            action.entry.waypoint_index.unwrap() as i32,
            integer(&case["native_read"]["waypoint"])
        );
        assert_eq!(
            sim.session.waypoint_cell(0),
            Some((
                integer(&case["native_waypoint"][0]) as i16,
                integer(&case["native_waypoint"][1]) as i16
            ))
        );
        supply_rng(&mut sim, &case["rng_before"]);
        let registries_before = sim.load_objects.registry_counts();
        let result = sim.apply_live_trigger_action(
            &action,
            sim.trigger_action_context(typ, None, TagEventInput::default(), Some(&rules)),
            &mut Vec::new(),
            Some(&rules),
            Some(&registry),
            FrameEffects::default(),
        );
        assert_eq!(
            i32::from(result),
            integer(&case["output"]["returned"]),
            "{name}: active108 AL"
        );
        assert_eq!(
            slot_words(&sim.crate_authority.slots()[0]),
            case["output"]["first_slot"],
            "{name}"
        );
        assert_eq!(
            slot_words(&sim.crate_authority.slots()[1]),
            case["output"]["second_slot"],
            "{name}"
        );
        assert_eq!(
            sim.crate_authority
                .slots()
                .iter()
                .filter(|s| !s.is_empty())
                .count() as u64,
            case["output"]["occupied_slots"].as_u64().unwrap(),
            "{name}"
        );
        if input["all_occupied"] != true {
            let mut cells = Vec::new();
            let grid = sim.overlay_grid.as_ref().unwrap();
            for y in 0..grid.height() {
                for x in 0..grid.width() {
                    let c = grid.cell(x, y);
                    if let Some(id) = c.overlay_id {
                        cells.push(json!({"xy":[x,y],"overlay":id,"data":c.overlay_data}));
                    }
                }
            }
            assert_eq!(
                json!(cells),
                case["output"]["cells"],
                "{name}: overlay/data"
            );
        }
        assert_rng(&sim, &case["rng_after"], name);
        assert_eq!(
            json!(sim.native_identity_cursor().unwrap()),
            case["output"]["scenario_id_cursor"],
            "{name}: constructor identity continuation"
        );
        assert_eq!(map.actions["CREATE"].entries[0].kind, 108);
        if let Some(lifetime) = case.get("lifecycle") {
            // Native AllAbstract already contains the supplied Action, whereas
            // this owner records only Overlay objects. Compare registry deltas
            // against that measured prior rather than claiming equal worlds.
            let compare_lifetime = |sim: &Simulation, phase: &str| {
                for (index, address) in ["00A8E360", "00B0F720", "00B0F670", "00B0F618", "00A8EC50"]
                    .into_iter()
                    .enumerate()
                {
                    assert_eq!(
                        sim.load_objects.registry_counts()[index] - registries_before[index],
                        (integer(&lifetime[phase]["registry_counts"][address])
                            - integer(&lifetime["before"]["registry_counts"][address]))
                            as usize,
                        "{name}: {phase} registry {address}"
                    );
                }
                assert_eq!(
                    sim.substrate.pending_delete.overlay_count(),
                    lifetime[phase]["pending_native_ids"]
                        .as_array()
                        .unwrap()
                        .len(),
                    "{name}: {phase} retained Overlay tickets"
                );
                assert_eq!(
                    json!(sim.native_identity_cursor().unwrap()),
                    lifetime[phase]["cursor"],
                    "{name}: {phase} identity continuation"
                );
            };
            compare_lifetime(&sim, "after_placement");
            assert_rng(&sim, &lifetime["rng_before_drain"], name);
            // Exercise the existing admitted frame tail, which owns the same
            // deferred drain as original725C70. Rules/object AI are omitted;
            // this compares Overlay lifetime, not whole-frame parity.
            sim.advance_tick(&[], None, None, None, 67);
            compare_lifetime(&sim, "after_drain");
            assert_rng(&sim, &lifetime["rng_after_drain"], name);
        }
    }
}

/// Whole49 actions32/14 are existing production consumers. Their fixture has
/// no death animations or ownership side effects requiring RNG. Native rows
/// use explicit callback sinks, so only their retained caller observations are
/// compared, not an invented original Tag-action/world replay.
#[test]
fn money_pickup_matches_original_reentrant_owner_alive_and_notification_controls() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;
    use crate::util::native_x87::NativeF64Bits;
    let native = corpus();
    let mut compared = 0;
    let mut source_only = Vec::new();
    for case in native["pickup_controls"].as_array().unwrap() {
        let input = &case["input"];
        let name = input["name"].as_str().unwrap();
        if matches!(
            input["mutation"].as_str(),
            Some("selection10" | "selection0")
        ) {
            // No existing gameplay action performs this exact supplied data
            // mutation. Keep these actual native prefix/caller witnesses;
            // do not add a test hook or another pickup implementation.
            source_only.push(name);
            continue;
        }
        let (mut sim, mut rules, mut registry, owner, actor) = fixture();
        rules.crate_rules.solo_crate_money = integer(&input["solo_money"]);
        rules.powerups.magnitudes[0] = NativeF64Bits::from_bits(500.0_f64.to_bits());
        if input["crate_trigger"] == false {
            registry = OverlayTypeRegistry::from_ini(
                &IniFile::from_str("[OverlayTypes]\n0=O0\n[O0]\nCrate=yes\nCrateTrigger=no\n"),
                None,
            );
        }
        sim.houses.get_mut(&owner).unwrap().player_control = false;
        sim.houses.get_mut(&owner).unwrap().economy.add_credits(100);
        let other = sim.interner.intern("Russians");
        sim.houses.insert(
            other,
            HouseState::new(other, 1, Some(other), false, 200, 10),
        );
        sim.session.house_order.push(other);
        let mutation = input["mutation"].as_str().unwrap_or("none");
        let action = match mutation {
            "kill" => "32,0,0,0,0,0,0,A",
            "owner1" => "14,0,1,0,0,0,0,A",
            _ => "28,0,7,0,0,0,0,A",
        };
        let _ = install(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nPICK=Americans,<none>,Pick,0,1,1,1,0\n\
             [Events]\nPICK=1,49,0,0\n[Tags]\nPICKTAG=0,Pick,PICK\n\
             [Actions]\nPICK=1,{action}\n"
            ),
        );
        if input["tag"] != false {
            sim.attach_object_trigger_tag(actor, Some("PICKTAG"));
        }
        if input["alive"] == false {
            sim.substrate
                .entities
                .get_mut(actor)
                .unwrap()
                .lifecycle
                .set_object_alive_for_test(false);
        }
        zero_slot_priors(&mut sim);
        *sim.crate_authority.slot_mut(0) = crate::sim::crates::CrateSlot {
            start_frame: 50,
            aux: 777,
            duration: 100,
            cell_x: 10,
            cell_y: 10,
        };
        sim.overlay_grid.as_mut().unwrap().place_overlay(
            10,
            10,
            0,
            integer(&input["selection"]) as u8,
        );
        sim.session.binary_frame = 100;
        supply_rng(&mut sim, &case["rng_full_before"]);
        let result = sim.pickup_crate_at(
            actor,
            (10, 10),
            Some(&rules),
            Some(&registry),
            FrameEffects::default(),
        );
        assert_eq!(i32::from(result), integer(&case["returned"]), "{name}");
        let entity = sim.substrate.entities.get(actor);
        assert_eq!(
            entity.is_some_and(|e| e.lifecycle.object_alive()),
            case["alive"].as_bool().unwrap(),
            "{name}: Alive"
        );
        if let Some(entity) = entity {
            assert_eq!(
                entity.owner(),
                [owner, other][case["owner"].as_u64().unwrap() as usize],
                "{name}: live House"
            );
        }
        assert_eq!(
            json!([
                sim.houses[&owner].economy.credits(),
                sim.houses[&other].economy.credits()
            ]),
            case["balances"],
            "{name}: real AddCredits"
        );
        assert_eq!(
            i32::from(sim.trigger_runtime.dirty[3]),
            integer(&case["dirty34be"]),
            "{name}: notification"
        );
        let cell = sim.overlay_grid.as_ref().unwrap().cell(10, 10);
        assert_eq!(
            cell.overlay_id.map_or(-1, i32::from),
            integer(&case["overlay"]),
            "{name}: removal"
        );
        assert_eq!(
            i32::from(cell.overlay_data),
            integer(&case["selection"]),
            "{name}: data"
        );
        assert_eq!(
            slot_words(&sim.crate_authority.slots()[0]),
            case["slot"],
            "{name}: solo retains slot"
        );
        assert_eq!(
            sim.trigger_runtime
                .dispatch_trace
                .iter()
                .filter(|(_, kind)| *kind == 49)
                .count(),
            case["tag_calls"].as_array().unwrap().len(),
            "{name}: synchronous Tag ingress"
        );
        assert_rng(&sim, &case["rng_full_after"], name);
        compared += 1;
    }
    assert_eq!(compared, 7);
    assert_eq!(source_only, ["callback_data10", "callback_data0"]);
}

#[test]
fn crate_predicates_match_all_original_controls_and_registration_classes() {
    use super::tests::{event_ingress_fixture, install_event_ingress_map};
    let native = corpus();
    let cases = native["event_controls"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        let _ = install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nCRATE=Country0,<none>,Crate,0,1,1,1,0\n\
             [Events]\nCRATE={}\n[Tags]\nTAG=2,Crate,CRATE\n",
                case["raw"].as_str().unwrap()
            ),
        );
        let tag_type = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(tag_type);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house =
            Some(houses[2]);
        sim.trigger_runtime.triggers[live].as_mut().unwrap().timer = CdTimer::from_raw(
            integer(&case["timer_before"][0]),
            integer(&case["timer_before"][2]),
        );
        let mut persistence = integer(&case["inputs"]["initial_persistence"]) != 0;
        supply_rng(&mut sim, &native["predicate_rng_seed31"]);
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["inputs"]["raised"]),
                    ..Default::default()
                },
                None,
                &mut persistence,
                Some(&rules),
                live,
            )
        };
        let result = if case["inputs"]["scenario_init"] == true {
            sim.with_buildup_bypass(predicate)
        } else {
            predicate(&mut sim)
        };
        assert_eq!(i32::from(result), integer(&case["result"]), "{case}");
        assert_eq!(
            i32::from(persistence),
            integer(&case["persistence"]),
            "{case}"
        );
        assert_eq!(
            sim.trigger_runtime.trigger(live).unwrap().timer,
            CdTimer::from_raw(
                integer(&case["timer_after"][0]),
                integer(&case["timer_after"][2])
            )
        );
        assert_eq!(case["timer_after"][1], 0);
        assert_eq!(case["event54"], case["event54_before"]);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house,
            Some(houses[2])
        );
        assert_rng(
            &sim,
            &native["predicate_rng_seed31"],
            case["name"].as_str().unwrap(),
        );
    }
    for case in native["classification"].as_array().unwrap() {
        assert_eq!(case["state_a"], 1);
        assert_eq!(case["state_b"], 1);
        assert_eq!(
            event_classification(integer(&case["kind"])),
            if case["kind"] == 49 { 2 } else { 16 }
        );
    }
}

#[test]
fn crate_logic_histories_reuse_native_lifecycle_driver_and_serialization() {
    let native = corpus();
    let rows = native["logic_histories"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        super::native_tests::replay_native_histories(rows.iter().cloned()),
        2
    );
}

/// Actual ALL07 source/Rules readers, cropped to its one elapsed20 route.
/// Waypoint identities/retail coordinates are checked before deliberately
/// translating AU/AV to nearby flat cells. This is production integration,
/// not a whole-map, original-world or gameplay-animation parity claim.
#[test]
fn retail_all07_elapsed_crates_and_money_survive_save_and_notify_logic() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    use crate::sim::snapshot::GameSnapshot;
    let native = corpus();
    let retail = &native["retail"];
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw_map = assets.get_ref(retail["map"].as_str().unwrap()).unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw_map),
        retail["map_sha256"]
    );
    let stock_map = crate::map::map_file::MapFile::from_bytes(raw_map).unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(assets.get_ref("RULESMD.INI").unwrap()),
        retail["rules_sha256"]
    );
    let named = assets.resolve_ref("ALL07SMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL07SMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, merged, art, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &stock_map.ini,
        )
        .unwrap()
        .into_parts();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let registry = OverlayTypeRegistry::from_ini(&merged, Some(&art));
    let roster =
        crate::map::houses::parse_house_roster(&stock_map.ini, &rules.color_schemes, Some(&rules));
    let (mut sim, _, _) = crate::sim::world::entry_test_fixture::fixture();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.game_mode_nonzero = false;
    sim.session.initialize_campaign_startup(1, 7).unwrap();
    sim.session.game_options.crates = false;
    sim.session.binary_frame = 0;
    sim.playfield_size_height = Some(16);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        Some(&rules),
        Some(&stock_map.ini),
    );
    let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(&merged);
    let clear = terrain_rules.semantics_for_land_type(0).unwrap();
    for y in 0..33 {
        for x in 0..33 {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap();
            cell.speed_costs = clear.speed_costs;
            cell.base_speed_costs = clear.speed_costs;
        }
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    // Terrain/path caches are deliberately not serialized. The app reattaches
    // its immutable map grid after every load; retain this explicitly cropped
    // flat template and exercise the same map-authority restore owners below.
    let map_terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    let id = retail["trigger_id"].as_str().unwrap();
    let mut body = String::new();
    // The notification consumer is an authored test observer, not an extra
    // retail ALL07 route. Put its records in the same physical section: a
    // second nonempty section is retained separately by the native INI reader.
    for (section, key, observer) in [
        (
            "Triggers",
            "trigger",
            "PICKED=Americans,<none>,Picked,0,1,1,1,0\n",
        ),
        ("Events", "events", "PICKED=1,50,0,0\n"),
        ("Actions", "actions", "PICKED=1,28,0,7,0,0,0,0,A\n"),
    ] {
        let raw = retail[key].as_str().unwrap();
        assert_eq!(
            stock_map
                .ini
                .section(section)
                .unwrap()
                .read_string(id, "", 512),
            raw
        );
        body.push_str(&format!("[{section}]\n{id}={raw}\n"));
        body.push_str(observer);
    }
    body.push_str("[Tags]\n");
    for (tag, raw) in retail["tags"].as_object().unwrap() {
        let raw = raw.as_str().unwrap();
        assert_eq!(
            stock_map
                .ini
                .section("Tags")
                .unwrap()
                .read_string(tag, "", 512),
            raw
        );
        body.push_str(&format!("{tag}={raw}\n"));
    }
    body.push_str("PICKEDTAG=0,Picked,PICKED\n");
    body.push_str("[Waypoints]\n");
    for action in retail["native_actions"].as_array().unwrap() {
        let wp = integer(&action["waypoint"]) as u32;
        let original = stock_map.waypoints[&wp];
        assert_eq!(json!([original.rx, original.ry]), action["native_waypoint"]);
        body.push_str(&format!(
            "{wp}={}\n",
            action["raw_waypoint"].as_str().unwrap()
        ));
    }
    let map = install(&mut sim, &rules, &body);
    let observer_type = sim
        .trigger_runtime
        .definitions
        .trigger_type("PICKED")
        .unwrap();
    let observer = &sim.trigger_runtime.definitions.trigger_types[observer_type];
    assert_eq!(observer.events.len(), 1);
    assert_eq!(observer.events[0].kind, 50);
    assert_eq!(observer.actions.len(), 1);
    assert_eq!(observer.actions[0].entry.kind, 28);
    assert_eq!(observer.actions[0].value, Some(7));
    let observer_tag_type = sim
        .trigger_runtime
        .definitions
        .tag_type("PICKEDTAG")
        .unwrap();
    assert!(sim.trigger_runtime.logic_tags.iter().any(|&tag| {
        sim.trigger_runtime
            .tag(tag)
            .is_some_and(|live| live.type_index == observer_tag_type)
    }));
    let typ = sim.trigger_runtime.definitions.trigger_type(id).unwrap();
    let definition = &sim.trigger_runtime.definitions.trigger_types[typ];
    assert_eq!(
        definition.events[0].kind,
        integer(&retail["native_events"]["kind"])
    );
    assert_eq!(
        definition.events[0].value,
        integer(&retail["native_events"]["value"])
    );
    assert_eq!(definition.actions.len(), 2);
    for (action, read) in definition
        .actions
        .iter()
        .zip(retail["native_actions"].as_array().unwrap())
    {
        assert_eq!(action.entry.kind, integer(&read["kind"]));
        assert_eq!(action.value.unwrap(), integer(&read["value"]));
        assert_eq!(
            action.entry.waypoint_index.unwrap() as i32,
            integer(&read["waypoint"])
        );
    }
    let mut translated = map.waypoints.clone();
    for (read, (x, y)) in retail["native_actions"]
        .as_array()
        .unwrap()
        .iter()
        .zip([(16, 15), (17, 15)])
    {
        let wp = integer(&read["waypoint"]) as u32;
        translated.insert(
            wp,
            crate::map::waypoints::Waypoint {
                index: wp,
                rx: x,
                ry: y,
            },
        );
    }
    sim.session.install_waypoints(&translated);
    let actor = sim
        .spawn_object_at_height("E1", "Americans", 20, 20, 0, 0, &rules)
        .unwrap();
    let house = sim.substrate.entities.get(actor).unwrap().owner();
    sim.houses.get_mut(&house).unwrap().player_control = false;
    supply_rng(&mut sim, &native["placement_controls"][0]["rng_before"]);
    let live = *sim
        .trigger_runtime
        .trigger_order
        .iter()
        .find(|&&live| sim.trigger_runtime.trigger(live).unwrap().type_index == typ)
        .unwrap();
    let deadline = sim.trigger_runtime.trigger(live).unwrap().timer.duration() as u32;
    sim.session.binary_frame = deadline - 1;
    poll(&mut sim, &rules, &registry, &map);
    assert_eq!(
        sim.crate_authority
            .slots()
            .iter()
            .filter(|slot| !slot.is_empty())
            .count(),
        0
    );
    let saved = GameSnapshot::save(&sim, 0, 0, "ALL07SMD.MAP", 0);
    let mut sim = GameSnapshot::load(&saved).unwrap().sim;
    sim.restore_after_snapshot_load().unwrap();
    sim.rebuild_caches_after_load(
        map_terrain.clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    sim.restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.session.binary_frame = deadline;
    poll(&mut sim, &rules, &registry, &map);
    assert_eq!(
        sim.crate_authority
            .slots()
            .iter()
            .filter(|slot| !slot.is_empty())
            .count(),
        2
    );
    let wood = registry
        .id_for_name(rules.crate_rules.wood_crate_img.as_deref().unwrap())
        .unwrap();
    for (x, y) in [(16, 15), (17, 15)] {
        let cell = sim.overlay_grid.as_ref().unwrap().cell(x, y);
        assert_eq!(cell.overlay_id, Some(wood));
        assert_eq!(cell.overlay_data, 0);
    }
    let before = sim.houses[&house].economy.credits();
    assert!(sim.pickup_crate_at(
        actor,
        (16, 15),
        Some(&rules),
        Some(&registry),
        FrameEffects::default()
    ));
    let ordinary = native["pickup_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "ordinary_money")
        .unwrap();
    assert_eq!(
        sim.houses[&house].economy.credits().wrapping_sub(before),
        integer(&ordinary["credit_calls"][0]["amount"])
    );
    assert!(sim.trigger_runtime.dirty[3]);
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(16, 15).overlay_id,
        None
    );
    // Save the notification between pickup and Logic, preserving the owner
    // rather than rebuilding a second scenario flag in the test.
    let authority = sim.crate_authority.clone();
    let saved = GameSnapshot::save(&sim, 0, 0, "ALL07SMD.MAP", 0);
    let mut sim = GameSnapshot::load(&saved).unwrap().sim;
    sim.restore_after_snapshot_load().unwrap();
    sim.rebuild_caches_after_load(
        map_terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    sim.restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .unwrap();
    sim.resolve_type_handles(&rules);
    assert_eq!(sim.crate_authority, authority);
    assert!(sim.trigger_runtime.dirty[3]);
    poll(&mut sim, &rules, &registry, &map);
    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert!(!sim.trigger_runtime.dirty[3]);
    assert_eq!(
        sim.crate_authority
            .slots()
            .iter()
            .filter(|slot| !slot.is_empty())
            .count(),
        2,
        "one-shot elapsed route must not place again"
    );
    let count = sim
        .trigger_runtime
        .action_trace
        .iter()
        .filter(|(kind, _)| *kind == 28)
        .count();
    poll(&mut sim, &rules, &registry, &map);
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .filter(|(kind, _)| *kind == 28)
            .count(),
        count,
        "dirty flag is consumed once"
    );
}
