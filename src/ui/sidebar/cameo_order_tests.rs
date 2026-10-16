//! Replay original CompareItems/AddCameo/Recalculate outputs. The fixture
//! supplies type/House inputs; it does not establish full CanBuild timing.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use serde::Deserialize;
use serde_json::Value;

use super::SidebarTab;
use super::cameo_order::{Cameo, CameoId, CameoKey, CameoStrips};
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::{HouseCostFactors, RuleSet};
use crate::sim::intern::StringInterner;
use crate::util::native_x87::NativeF32Bits;

fn corpus() -> &'static Value {
    let corpus = crate::test_fixture::json("tools/sidebar_oracle/cameo_order.json");
    assert_eq!(corpus["schema"], 1);
    corpus
}

fn trigger_corpus() -> &'static Value {
    crate::test_fixture::json("tools/sidebar_oracle/trigger_cameos.json")
}

#[test]
fn original_trigger_tabs_and_cameo_flash_deadlines() {
    let native = trigger_corpus();
    let fixture = Fixture::new(&serde_json::json!({
        "current_side": 0, "types": native["catalog"]
    }));
    let candidates: Vec<_> = fixture
        .candidates()
        .into_iter()
        .filter(|item| fixture.name(item.id) != "UNLISTED")
        .collect();
    let mut initial = CameoStrips::default();
    initial.reconcile(&candidates, 3, true);
    fixture.assert_order(&initial, &native["order"], "114/115 initial order");
    for control in native["tab_controls"].as_array().unwrap() {
        let mut strips = initial.clone();
        let counts = control["counts"].as_array().unwrap();
        strips.retain(3, |id| {
            let item = candidates.iter().find(|item| item.id == id).unwrap();
            counts[item.tab.tab_index()].as_u64().unwrap() != 0
        });
        assert_eq!(
            strips
                .selectable_tab(control["index"].as_i64().unwrap() as i32)
                .map(|tab| tab.tab_index()),
            control["selected"]
                .as_array()
                .unwrap()
                .first()
                .map(|value| value.as_u64().unwrap() as usize),
            "114 native count admission: {control}"
        );
    }
    let index_of = |id| match id {
        CameoId::Object(id) => {
            let name = fixture.interner.resolve(id);
            let object = fixture.rules.object(name).unwrap();
            fixture.rules.type_array_index(object.category, name)
        }
        CameoId::SuperWeapon(_) => None,
    };
    for control in native["flash_controls"].as_array().unwrap() {
        let mut strips = initial.clone();
        if control["result"] == 1 {
            let name = control["type_name"].as_str().unwrap();
            let id = fixture.interner.get(name).unwrap();
            let object = fixture.rules.object(name).unwrap();
            strips.flash_type_index(
                SidebarTab::for_category(crate::sim::production::category_for_object(object)),
                index_of(CameoId::Object(id)).unwrap(),
                control["duration"].as_i64().unwrap() as i32,
                control["frame"].as_i64().unwrap() as i32,
                index_of,
            );
        }
        let (deadlines, redraw_until) = strips.saved_flash();
        let actual: serde_json::Map<String, Value> = SidebarTab::all()
            .into_iter()
            .flat_map(|tab| {
                strips
                    .items(tab)
                    .iter()
                    .zip(&deadlines[tab.tab_index()])
                    .map(|(item, deadline)| {
                        (fixture.name(item.id).to_owned(), Value::from(*deadline))
                    })
            })
            .collect();
        assert_eq!(
            Value::Object(actual),
            control["deadlines"],
            "{}",
            control["name"]
        );
        assert_eq!(
            redraw_until,
            control["redraw_until"].as_i64().unwrap() as i32
        );
        assert_eq!(control["rng_before"], control["rng_after"]);
    }
    let mut strips = initial;
    for control in native["overwrite"].as_array().unwrap() {
        strips.flash_type_index(
            SidebarTab::Infantry,
            0,
            control["duration"].as_i64().unwrap() as i32,
            control["frame"].as_i64().unwrap() as i32,
            index_of,
        );
        let (deadlines, redraw_until) = strips.saved_flash();
        assert_eq!(
            deadlines[2][0],
            control["deadline"].as_i64().unwrap() as i32
        );
        assert_eq!(
            redraw_until,
            control["redraw_until"].as_i64().unwrap() as i32
        );
    }
    assert!(native["rng_calls"].as_array().unwrap().is_empty());
}

#[test]
fn original_flash_signed_frame_phase() {
    let native = trigger_corpus();
    let fixture = Fixture::new(&serde_json::json!({
        "current_side": 0, "types": native["catalog"]
    }));
    let infantry = fixture
        .candidates()
        .into_iter()
        .find(|item| fixture.name(item.id) == "E2")
        .unwrap();
    let mut strips = CameoStrips::default();
    strips.insert(&infantry);
    for control in native["draw_controls"].as_array().unwrap() {
        let deadline = control[0].as_i64().unwrap() as i32;
        strips.restore_flash([vec![], vec![], vec![deadline], vec![]], 0);
        strips.set_frame(control[1].as_i64().unwrap() as i32);
        assert_eq!(
            strips.flash_darkened(infantry.id),
            control[2].as_bool().unwrap(),
            "original6A9B59 deadline/frame: {control}"
        );
    }
}

#[derive(Deserialize)]
struct NativeType {
    id: String,
    kind: i32,
    side: i32,
    tech_level: i32,
    cost: i32,
    ui_name: String,
    considered_aircraft: bool,
    naval: bool,
    build_cat: i32,
    recharge_frames: i32,
    free_unit: Option<String>,
}

struct Fixture {
    types: Vec<NativeType>,
    rules: RuleSet,
    interner: StringInterner,
    side: i32,
    factors: HouseCostFactors,
}

fn factor_bits(values: &Value) -> [NativeF32Bits; 5] {
    let values = values.as_array().unwrap();
    assert_eq!(values.len(), 5);
    std::array::from_fn(|slot| {
        NativeF32Bits::from_bits((values[slot].as_f64().unwrap() as f32).to_bits())
    })
}

impl Fixture {
    fn new(context: &Value) -> Self {
        let types: Vec<NativeType> = serde_json::from_value(context["types"].clone()).unwrap();
        // Feed raw Cost/FreeUnit/BuildCat inputs to the production reader and
        // Cost_Of owner, including the BuildingType override. No cost formula
        // or ordering implementation is reproduced by this adapter.
        let mut ini = String::from("[General]\nSeparateAircraft=yes\n");
        for (kind, section) in [
            (7, "BuildingTypes"),
            (16, "InfantryTypes"),
            (40, "VehicleTypes"),
            (3, "AircraftTypes"),
        ] {
            writeln!(ini, "[{section}]").unwrap();
            for (index, item) in types.iter().filter(|item| item.kind == kind).enumerate() {
                writeln!(ini, "{index}={}", item.id).unwrap();
            }
        }
        for item in types
            .iter()
            .filter(|item| !matches!(item.kind, 31 | 32 | 57))
        {
            writeln!(ini, "[{}]\nCost={}", item.id, item.cost).unwrap();
            if item.build_cat == 5 {
                writeln!(ini, "BuildCat=Combat").unwrap();
            }
            if let Some(free) = &item.free_unit {
                writeln!(ini, "FreeUnit={free}").unwrap();
            }
        }
        let mut interner = StringInterner::new();
        for item in &types {
            interner.intern(&item.id);
        }
        Self {
            types,
            rules: RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap(),
            interner,
            side: context["current_side"].as_i64().unwrap() as i32,
            factors: HouseCostFactors {
                country: context
                    .get("country")
                    .map_or([NativeF32Bits::ONE; 5], factor_bits),
                factory_plant: context
                    .get("plant")
                    .map_or([NativeF32Bits::ONE; 5], factor_bits),
            },
        }
    }

    fn candidates(&self) -> Vec<Cameo> {
        self.types
            .iter()
            .map(|item| {
                let id = self.interner.get(&item.id).unwrap();
                if matches!(item.kind, 31 | 32 | 57) {
                    Cameo {
                        id: CameoId::SuperWeapon(id),
                        tab: SidebarTab::Defense,
                        available: true,
                        key: CameoKey::SuperWeapon {
                            recharge_frames: item.recharge_frames,
                            name: item.ui_name.clone(),
                        },
                    }
                } else {
                    let vehicle = matches!(item.kind, 3 | 40);
                    Cameo {
                        id: CameoId::Object(id),
                        tab: match item.kind {
                            7 if item.build_cat == 5 => SidebarTab::Defense,
                            7 => SidebarTab::Building,
                            16 => SidebarTab::Infantry,
                            3 | 40 => SidebarTab::Vehicle,
                            kind => panic!("uncovered native kind {kind}"),
                        },
                        available: true,
                        key: CameoKey::Techno {
                            own_side: item.side == self.side,
                            considered_aircraft: vehicle && item.considered_aircraft,
                            naval: vehicle && item.naval,
                            tech_level: item.tech_level,
                            cost: self
                                .rules
                                .cost_of(self.rules.object(&item.id).unwrap(), Some(&self.factors)),
                            name: item.ui_name.clone(),
                        },
                    }
                }
            })
            .collect()
    }

    fn name(&self, id: CameoId) -> &str {
        match id {
            CameoId::Object(id) | CameoId::SuperWeapon(id) => self.interner.resolve(id),
        }
    }

    fn assert_order(&self, strips: &CameoStrips, expected: &Value, label: &str) {
        for tab in SidebarTab::all() {
            let actual: Vec<_> = strips
                .items(tab)
                .iter()
                .map(|item| self.name(item.id))
                .collect();
            assert_eq!(
                serde_json::json!(actual),
                expected[tab.tab_index()],
                "{label}, {tab:?}"
            );
        }
    }

    fn assert_costs(&self, expected: &Value) {
        for (name, value) in expected.as_object().unwrap() {
            assert_eq!(
                self.rules
                    .cost_of(self.rules.object(name).unwrap(), Some(&self.factors)),
                value.as_i64().unwrap() as i32,
                "original Cost_Of for {name} with {:?}",
                self.factors,
            );
        }
    }

    fn assert_comparisons(&self, cases: &Value) {
        let candidates = self.candidates();
        let by_name: HashMap<_, _> = candidates
            .iter()
            .map(|item| (self.name(item.id), &item.key))
            .collect();
        for case in cases.as_array().unwrap() {
            let left = case["left"].as_str().unwrap();
            let right = case["right"].as_str().unwrap();
            assert_eq!(
                by_name[left].inserts_before(by_name[right]),
                case["before_or_equal"].as_bool().unwrap(),
                "original CompareItems: {left} versus {right}",
            );
        }
    }
}

#[test]
fn original_pair_matrix_covers_utf16_ties_flag_overlap_and_key_hierarchy() {
    let corpus = corpus();
    let context = &corpus["comparisons"];
    assert_eq!(context["types"].as_array().unwrap().len(), 33);
    assert_eq!(context["cases"].as_array().unwrap().len(), 1089);
    Fixture::new(context).assert_comparisons(&context["cases"]);
}

#[test]
fn original_cost_contexts_use_country_plant_and_free_unit_owners() {
    let corpus = corpus();
    let contexts = corpus["cost_contexts"].as_array().unwrap();
    assert_eq!(contexts.len(), 4);
    for context in contexts {
        let fixture = Fixture::new(context);
        fixture.assert_costs(&context["native_costs"]);
        fixture.assert_comparisons(&context["cases"]);
        assert_eq!(context["cases"].as_array().unwrap().len(), 36);
    }
}

fn replay_history(context: &Value) {
    let mut fixture = Fixture::new(context);
    let mut candidates = fixture.candidates();
    let mut strips = CameoStrips::default();
    let mut unavailable = HashSet::new();
    if let Some(costs) = context.get("initial_native_costs") {
        fixture.assert_costs(costs);
    }
    if let Some(initial) = context["initial_adds"].as_array() {
        for name in initial {
            let item = candidates
                .iter()
                .find(|item| fixture.name(item.id) == name.as_str().unwrap())
                .unwrap();
            assert!(strips.insert(item));
        }
        fixture.assert_order(
            &strips,
            &context["initial_order"],
            "initial capacity admissions",
        );
    }
    for (index, step) in context["steps"].as_array().unwrap().iter().enumerate() {
        let action = step["action"].as_str().unwrap();
        let label = format!("{} step {index}: {action}", context["id"]);
        match action {
            "add" => {
                let name = step["id"].as_str().unwrap();
                let item = candidates
                    .iter()
                    .find(|item| fixture.name(item.id) == name)
                    .unwrap();
                assert_eq!(
                    strips.insert(item),
                    step["accepted"].as_bool().unwrap(),
                    "{label}, {name}"
                );
            }
            "plant_factors" => {
                fixture.factors.factory_plant = factor_bits(&step["values"]);
                fixture.assert_costs(&step["native_costs"]);
                candidates = fixture.candidates();
                strips.refresh_keys(&candidates);
            }
            "factory_available" | "can_build" | "super_granted" => {
                let name = step["id"].as_str().unwrap();
                let id = candidates
                    .iter()
                    .find(|item| fixture.name(item.id) == name)
                    .unwrap()
                    .id;
                let available = step["value"]
                    .as_bool()
                    .unwrap_or_else(|| step["value"].as_i64().unwrap() != 0);
                if available {
                    unavailable.remove(&id);
                } else {
                    unavailable.insert(id);
                }
            }
            "recalculate" => {
                let tab = step["tab"].as_u64().unwrap() as usize;
                strips.retain(3, |id| {
                    let item = candidates.iter().find(|item| item.id == id).unwrap();
                    item.tab.tab_index() != tab || !unavailable.contains(&id)
                });
            }
            "init_clear" => strips = CameoStrips::default(),
            other => panic!("uncovered native history action {other}"),
        }
        if let Some(order) = step.get("order") {
            fixture.assert_order(&strips, order, &label);
        }
    }
}

#[test]
fn original_histories_preserve_survivors_and_reinsert_using_current_costs() {
    let corpus = corpus();
    let histories = corpus["histories"].as_array().unwrap();
    assert_eq!(histories.len(), 3);
    for history in histories {
        replay_history(history);
    }
}

#[test]
fn original_capacity_gate_and_duplicate_rejection_do_not_move_entries() {
    replay_history(&corpus()["capacity_history"]);
}

#[test]
fn reconciliation_adds_before_pruning_and_retries_on_the_next_recheck() {
    let corpus = corpus();
    let context = &corpus["capacity_history"];
    let fixture = Fixture::new(context);
    let mut candidates = fixture.candidates();
    let initial: HashSet<_> = context["initial_adds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect();
    for item in &mut candidates {
        item.available = initial.contains(fixture.name(item.id));
    }
    let mut strips = CameoStrips::default();
    assert!(strips.reconcile(&candidates, 3, true).build_inserted());
    fixture.assert_order(&strips, &context["initial_order"], "initial reconciliation");

    let steps = context["steps"].as_array().unwrap();
    for item in &mut candidates {
        item.available = fixture.name(item.id) != steps[0]["id"].as_str().unwrap();
    }
    assert!(
        !strips.reconcile(&candidates, 3, true).build_inserted(),
        "full strip rejects the new item before removing the old one"
    );
    fixture.assert_order(&strips, &steps[2]["order"], "first reconciliation");
    assert!(strips.reconcile(&candidates, 3, true).build_inserted());
    fixture.assert_order(&strips, &steps[3]["order"], "second reconciliation");
}

/// Native flips the tab buttons and moves the active tab only on the two
/// AddCameo/Recalculate transitions, never from a strip's current contents.
///
/// AddCameo 6A6472 enables the added strip's button and, when no strip held an
/// entry before the insertion, hands ActiveTab to the first populated strip.
/// Recalculate 6AAAE7 disables the button of the strip it just emptied and
/// re-checks ActiveTab the same way. A strip that is empty without such a
/// transition keeps its enabled button, so the executable controls
/// (tools/sidebar_oracle/cameo_order.json) and the four command gates
/// (`StripClass::HasEntries` 6ABAC0) agree.
#[test]
fn native_tab_transitions_follow_add_cameo_and_recalculate() {
    let mut interner = StringInterner::new();
    let unit = interner.intern("TABVEHICLE");
    let building = interner.intern("TABSTRUCTURE");
    let cameo = |id, tab| Cameo {
        id: CameoId::Object(id),
        tab,
        available: true,
        key: CameoKey::Techno {
            own_side: true,
            considered_aircraft: false,
            naval: false,
            tech_level: 1,
            cost: 100,
            name: "Test Type".to_string(),
        },
    };

    // The first entry of an empty sidebar selects its own strip.
    let mut strips = CameoStrips::default();
    let first = strips.reconcile(&[cameo(unit, SidebarTab::Vehicle)], 3, true);
    assert_eq!(
        first.transitions().tab_button_disabled(SidebarTab::Vehicle),
        Some(false)
    );
    assert_eq!(
        first.transitions().active_tab_destination(),
        Some(SidebarTab::Vehicle)
    );

    // A later addition into a populated sidebar moves nothing.
    let populated = [
        cameo(unit, SidebarTab::Vehicle),
        cameo(building, SidebarTab::Building),
    ];
    let second = strips.reconcile(&populated, 3, true);
    assert_eq!(
        second
            .transitions()
            .tab_button_disabled(SidebarTab::Building),
        Some(false)
    );
    assert_eq!(second.transitions().active_tab_destination(), None);

    // Recalculate dropping the last vehicle entry disables that strip's own
    // button and re-checks the active tab against the first populated strip.
    let mut pruned = populated;
    pruned[0].available = false;
    let third = strips.reconcile(&pruned, 3, true);
    assert_eq!(
        third.transitions().tab_button_disabled(SidebarTab::Vehicle),
        Some(true)
    );
    assert_eq!(
        third
            .transitions()
            .tab_button_disabled(SidebarTab::Building),
        None
    );
    assert_eq!(
        third.transitions().active_tab_destination(),
        Some(SidebarTab::Building)
    );
    assert_eq!(strips.first_populated_tab(), Some(SidebarTab::Building));

    // Re-reconciling the now empty vehicle strip reports no transition: it was
    // already empty, so native never disabled its button on this pass.
    let fourth = strips.reconcile(&[cameo(building, SidebarTab::Building)], 3, true);
    assert_eq!(
        fourth
            .transitions()
            .tab_button_disabled(SidebarTab::Vehicle),
        None
    );
    assert_eq!(fourth.transitions().active_tab_destination(), None);
}

#[test]
fn original_super_recharge_input_keeps_reader_history_and_native_rounding() {
    let corpus = corpus();
    let reader = &corpus["recharge_reader"];
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[SuperWeaponTypes]\n0=TESTSW\n[TESTSW]\nType=LightningStorm\n",
    ));
    let read_frames = |layers: &RulesLayerStack| {
        RuleSet::from_rules_layers(layers)
            .unwrap()
            .super_weapon("TESTSW")
            .unwrap()
            .recharge_time_frames
    };
    assert_eq!(
        i64::from(read_frames(&layers)),
        reader["constructor_frames"]
    );
    for case in reader["cases"].as_array().unwrap() {
        assert_eq!(i64::from(read_frames(&layers)), case["before"]);
        let text = case["raw"].as_str().map_or_else(
            || "[TESTSW]\n".to_string(),
            |raw| format!("[TESTSW]\nRechargeTime={raw}\n"),
        );
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
        assert_eq!(
            i64::from(read_frames(&layers)),
            case["recharge_frames"],
            "original retained RechargeTime reader: {case}",
        );
    }
}

#[test]
fn original_complete_recalculate_preserves_order_and_adjusts_each_strip_scroll() {
    let corpus = corpus();
    let native = &corpus["recalculate_scroll"];
    let fixture = Fixture::new(&serde_json::json!({
        "current_side": 0, "types": native["types"],
    }));
    let candidates = fixture.candidates();
    let cases = native["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 430);
    for case in cases {
        let initial: Vec<_> = case["initial_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                candidates
                    .iter()
                    .find(|item| fixture.name(item.id) == name.as_str().unwrap())
                    .unwrap()
                    .clone()
            })
            .collect();
        let mut strips = CameoStrips::from_retained(
            [Vec::new(), Vec::new(), Vec::new(), initial],
            [0, 0, 0, case["top_row_before"].as_u64().unwrap() as usize],
        );
        let removed: HashSet<_> = case["remove_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        strips.retain(case["visible_slots"].as_u64().unwrap() as usize / 2, |id| {
            !removed.contains(fixture.name(id))
        });
        assert_eq!(
            strips.scroll_row(SidebarTab::Vehicle),
            case["top_row_after"].as_u64().unwrap() as usize,
            "{}",
            case["id"]
        );
        let actual: Vec<_> = strips
            .items(SidebarTab::Vehicle)
            .iter()
            .map(|item| fixture.name(item.id))
            .collect();
        assert_eq!(serde_json::json!(actual), case["order"], "{}", case["id"]);
        assert_eq!(&strips.scroll_rows()[..3], &[0; 3]);
    }
}
