//! Production rules/admission -> retained strip -> displayed/clicked identity.
//! Native ordering goldens are separate from the bounded admission fixture.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use serde_json::Value;

use super::sidebar_projection::{SidebarProjectionState, cameo_candidates};
use crate::assets::csf_file::{CsfFile, test_csf};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{BuildOption, all_build_options_for_owner};
use crate::sim::world::Simulation;
use crate::ui::sidebar::cameo_order::CameoId;
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
use crate::ui::sidebar::{
    SidebarAction, SidebarChromeLayoutSpec, SidebarTab, SidebarView, build_sidebar_view_with_spec,
    hit_test_item,
};

fn world(rules: &RuleSet, factories: &[&str]) -> (Simulation, InternedId) {
    let mut sim = Simulation::with_seed(0x6A84_2000);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, true, 50_000, 10);
    house.project_country_mults(rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    for (index, name) in factories.iter().enumerate() {
        let x = 4 + (index % 4) as u16 * 7;
        let y = 4 + (index / 4) as u16 * 7;
        sim.spawn_object(name, "Americans", x, y, 0, rules)
            .unwrap_or_else(|| panic!("place sidebar fixture factory {name}"));
    }
    (sim, owner)
}

fn view(
    sim: &Simulation,
    options: &[BuildOption],
    projection: &SidebarProjectionState,
    tab: SidebarTab,
    height: f32,
) -> SidebarView {
    build_sidebar_view_with_spec(
        SidebarChromeLayoutSpec::stock(),
        800.0,
        height,
        tab,
        50_000,
        1_000,
        0,
        None,
        &[],
        &[],
        options,
        &[],
        None,
        &[],
        Some(&sim.interner),
        &[],
        &SidebarGadgetState::new(),
        None,
        None,
        None,
        None,
        [None; 2],
        projection.cameo_strips(),
    )
}

fn native_context_world(context: &Value) -> (Simulation, RuleSet, InternedId, CsfFile) {
    let types: Vec<_> = context["types"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| !matches!(item["kind"].as_i64().unwrap(), 31 | 32 | 57))
        .collect();
    let factories = [
        ("TESTBUILD", "BuildingType"),
        ("TESTINF", "InfantryType"),
        ("TESTUNIT", "UnitType"),
        ("TESTAIR", "AircraftType"),
    ];
    let mut ini =
        String::from("[Countries]\n0=Americans\n[Sides]\nGDI=Americans\n[Americans]\nSide=GDI\n");
    for (kind, section) in [
        (7, "BuildingTypes"),
        (16, "InfantryTypes"),
        (40, "VehicleTypes"),
        (3, "AircraftTypes"),
    ] {
        writeln!(ini, "[{section}]").unwrap();
        for (index, item) in types.iter().filter(|item| item["kind"] == kind).enumerate() {
            writeln!(ini, "{index}={}", item["id"].as_str().unwrap()).unwrap();
        }
        if kind == 7 {
            for (index, (name, _)) in factories.iter().enumerate() {
                writeln!(ini, "{}={name}", index + 100).unwrap();
            }
        }
    }
    let mut labels = Vec::new();
    for item in &types {
        let id = item["id"].as_str().unwrap();
        let label = format!("CAMEO:{id}");
        labels.push((label.clone(), item["ui_name"].as_str().unwrap().to_string()));
        writeln!(
            ini,
            "[{id}]\nName=Internal {id}\nUIName={label}\nOwner=Americans\n\
             Strength=100\nCost={}\nTechLevel={}\nAIBasePlanningSide={}\n\
             ConsideredAircraft={}\nNaval={}\n",
            item["cost"],
            item["tech_level"],
            item["side"],
            item["considered_aircraft"],
            item["naval"],
        )
        .unwrap();
        if item["build_cat"] == 5 {
            writeln!(ini, "BuildCat=Combat").unwrap();
        }
    }
    for (name, kind) in factories {
        writeln!(
            ini,
            "[{name}]\nOwner=Americans\nFactory={kind}\nStrength=1000\nTechLevel=-1"
        )
        .unwrap();
    }
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&ini)).unwrap();
    let (sim, owner) = world(&rules, &factories.map(|(name, _)| name));
    let csf = test_csf(
        &labels
            .iter()
            .map(|(label, value)| (label.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
    );
    (sim, rules, owner, csf)
}

fn native_history_world() -> (Simulation, RuleSet, InternedId, CsfFile, Value) {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/cameo_order.json",
    ))
    .unwrap();
    let history = &corpus["histories"][0];
    let (sim, rules, owner, csf) = native_context_world(history);
    // The tenth accepted ordinary insertion is the native complete roster
    // before supers, repricing or removals. Expected order is never computed
    // from the Rust comparator or an independent test sorting function.
    let expected = history["steps"][9]["order"].clone();
    (sim, rules, owner, csf, expected)
}

#[test]
fn original_post_removal_scroll_reaches_view_and_hit_test() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/cameo_order.json",
    ))
    .unwrap();
    let native = &corpus["recalculate_scroll"];
    let case = native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "twenty_six_slots_row1_remove19")
        .unwrap();
    let context = serde_json::json!({"current_side": 0, "types": native["types"]});
    let (sim, rules, owner, csf) = native_context_world(&context);
    let options: Vec<_> = all_build_options_for_owner(&sim, &rules, "Americans")
        .into_iter()
        .filter(|item| {
            case["initial_order"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name.as_str().unwrap() == sim.interner.resolve(item.type_id))
        })
        .collect();
    let mut candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    projection.reconcile_cameos(owner, &candidates, 3);
    projection.set_scroll_row(SidebarTab::Vehicle, 1);
    for candidate in &mut candidates {
        let CameoId::Object(id) = candidate.id else {
            unreachable!()
        };
        candidate.available = !case["remove_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name.as_str().unwrap() == sim.interner.resolve(id));
    }
    projection.reconcile_cameos(owner, &candidates, 3);
    let view = view(&sim, &options, &projection, SidebarTab::Vehicle, 430.);
    assert_eq!(view.layout.side2_tile_count, 3);
    assert_eq!(
        view.scroll_rows,
        case["top_row_after"].as_u64().unwrap() as usize
    );
    let expected_first = case["order"][view.scroll_rows * 2].as_str().unwrap();
    assert_eq!(view.items[0].type_id, expected_first);
    assert!(matches!(
        hit_test_item(&view.items[0], false, false),
        SidebarAction::CameoPress { type_id, .. } if type_id == expected_first
    ));
}

#[test]
fn production_projection_uses_rule_side_category_flags_and_csf_utf16_inputs() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/cameo_order.json",
    ))
    .unwrap();
    let context = &corpus["comparisons"];
    let (sim, rules, owner, csf) = native_context_world(context);
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    let candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    let keys: HashMap<_, _> = candidates
        .iter()
        .map(|item| {
            let CameoId::Object(id) = item.id else {
                panic!("ordinary type fixture")
            };
            (id, &item.key)
        })
        .collect();
    // Rules may have interned an ID with different display casing already.
    // Compare the native names through the identity owner, and require every
    // ordinary fixture type before excluding the seven superweapon types.
    let ordinary: HashMap<_, _> = context["types"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| !matches!(item["kind"].as_i64().unwrap(), 31 | 32 | 57))
        .map(|item| {
            let name = item["id"].as_str().unwrap();
            let id = sim
                .interner
                .get(name)
                .unwrap_or_else(|| panic!("native ordinary type was not interned: {name}"));
            let key = keys
                .get(&id)
                .unwrap_or_else(|| panic!("native ordinary type was not projected: {name}"));
            (name, *key)
        })
        .collect();
    assert_eq!(ordinary.len(), 26);
    let mut compared = 0;
    for case in context["cases"].as_array().unwrap() {
        let left = case["left"].as_str().unwrap();
        let right = case["right"].as_str().unwrap();
        let (Some(left_key), Some(right_key)) = (ordinary.get(left), ordinary.get(right)) else {
            continue; // Supers are separately covered by the native key replay.
        };
        assert_eq!(
            left_key.inserts_before(right_key),
            case["before_or_equal"].as_bool().unwrap(),
            "projected native inputs: {left} versus {right}"
        );
        compared += 1;
    }
    assert_eq!(compared, 26 * 26);
}

#[test]
fn native_order_reaches_localized_views_scrolling_and_click_identity() {
    let (sim, rules, owner, csf, expected) = native_history_world();
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    assert_eq!(
        options
            .iter()
            .filter(|item| item.visible_in_sidebar())
            .count(),
        10
    );
    let candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    assert!(
        !projection.reconcile_cameos(owner, &candidates, 3),
        "scenario seeding is silent"
    );
    assert!(
        !projection.reconcile_cameos(owner, &candidates, 3),
        "retained identities are not insertions"
    );

    for tab in SidebarTab::all() {
        let expected = expected[tab.tab_index()].as_array().unwrap();
        projection.set_scroll_row(tab, 0);
        let first = view(&sim, &options, &projection, tab, 360.0);
        for scroll in 0..=first.max_scroll_rows {
            projection.set_scroll_row(tab, scroll);
            let view = view(&sim, &options, &projection, tab, 360.0);
            let names: Vec<_> = view
                .items
                .iter()
                .map(|item| item.type_id.as_str())
                .collect();
            let expected_names: Vec<_> = expected
                .iter()
                .skip(scroll * 2)
                .take(view.layout.side2_tile_count * 2)
                .map(|name| name.as_str().unwrap())
                .collect();
            assert_eq!(names, expected_names, "{tab:?}, row {scroll}");
            for item in &view.items {
                let object = rules.object(&item.type_id).unwrap();
                assert_eq!(
                    item.display_name,
                    csf.get(object.ui_name.as_deref().unwrap()).unwrap()
                );
                assert_ne!(item.display_name, object.name.as_deref().unwrap());
                assert_eq!(
                    hit_test_item(item, false, false),
                    SidebarAction::CameoPress {
                        type_id: item.type_id.clone(),
                        right: false,
                        shift: false,
                        at_build_limit: !item.enabled,
                    },
                );
                let option = options
                    .iter()
                    .find(|option| sim.interner.resolve(option.type_id) == item.type_id)
                    .unwrap();
                assert_eq!(item.cost, Some(option.cost));
                assert_eq!(item.queue_category, option.queue_category);
            }
        }
    }
    projection.set_scroll_row(SidebarTab::Vehicle, 1);
    let scrolled = view(&sim, &options, &projection, SidebarTab::Vehicle, 360.0);
    assert_eq!(
        scrolled.scroll_rows, 1,
        "fixture must exercise an actual second page"
    );
}

#[test]
fn only_an_accepted_new_object_in_the_same_house_announces_construction_options() {
    let (sim, rules, owner, csf, _) = native_history_world();
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    let candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    let mut initial = candidates.clone();
    let tank = CameoId::Object(sim.interner.get("tank").unwrap());
    initial
        .iter_mut()
        .find(|item| item.id == tank)
        .unwrap()
        .available = false;
    let mut projection = SidebarProjectionState::default();
    assert!(!projection.reconcile_cameos(owner, &initial, 3));
    assert!(projection.reconcile_cameos(owner, &candidates, 3));
    assert!(!projection.reconcile_cameos(owner, &candidates, 3));
    assert!(
        !projection.reconcile_cameos(owner, &initial, 3),
        "removal is silent"
    );
    assert!(
        projection.reconcile_cameos(owner, &candidates, 3),
        "a later accepted reinsertion announces again"
    );
    let other = sim.interner.get("TESTBUILD").unwrap();
    assert!(
        !projection.reconcile_cameos(other, &candidates, 3),
        "local-house replacement seeds silently"
    );
}

#[test]
fn retail_rules_admission_and_csf_names_keep_the_same_identity_in_every_view() {
    let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    // Extracted RULESMD/ARTMD are an explicit base-rules fixture, not a claim
    // about all mode/map layers. The separate native replay supplies order.
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let (sim, owner) = world(
        &rules,
        &[
            "GACNST", "GAPOWR", "GAPILE", "GAREFN", "GAWEAP", "GAAIRC", "GATECH",
        ],
    );
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    let csf = crate::rules::retail_ini_fixture::retail_assets().map(|(_, assets)| {
        let selected = assets
            .load_file_from_mix("ra2md.csf")
            .expect("retail active-YR string table");
        CsfFile::from_bytes(&selected.bytes).unwrap()
    });
    let candidates = cameo_candidates(&sim, &rules, csf.as_ref(), owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    assert!(!projection.reconcile_cameos(owner, &candidates, 3));
    let visible: HashSet<_> = options
        .iter()
        .filter(|item| item.visible_in_sidebar())
        .map(|item| item.type_id)
        .collect();
    assert!(
        visible.contains(&sim.interner.get("E1").unwrap()),
        "placed Allied barracks exposes the GI"
    );
    assert!(
        !visible.contains(&sim.interner.get("E2").unwrap()),
        "foreign infantry is not admitted by the Allied factory"
    );
    let mut observed = HashSet::new();
    for tab in SidebarTab::all() {
        let view = view(&sim, &options, &projection, tab, 2400.0);
        for item in &view.items {
            let id = sim.interner.get(&item.type_id).unwrap();
            assert!(
                observed.insert(id),
                "no duplicate projected identity: {}",
                item.type_id
            );
            assert!(visible.contains(&id));
            let kind = rules.object(&item.type_id).unwrap();
            let label = kind.ui_name.as_deref().unwrap();
            let expected_name = csf
                .as_ref()
                .map_or_else(|| label.to_string(), |csf| csf.text(label).into_owned());
            assert_eq!(item.display_name, expected_name);
            assert_eq!(item.cost, Some(sim.cost_of(owner, kind, &rules)));
            assert!(
                matches!(hit_test_item(item, false, false), SidebarAction::CameoPress { type_id, .. } if type_id == item.type_id)
            );
        }
    }
    assert_eq!(observed, visible);
}
