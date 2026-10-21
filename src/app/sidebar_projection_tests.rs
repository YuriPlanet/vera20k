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
use crate::sim::world::{LifecycleOutput, Simulation};
use crate::ui::sidebar::cameo_order::{Cameo, CameoId, CameoReconcile};
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
use crate::ui::sidebar::{
    ArmedSidebarEntry, SidebarAction, SidebarChromeLayoutSpec, SidebarTab, SidebarView,
    build_sidebar_view_with_spec, hit_test_item,
};
use crate::util::save_codec;

impl SidebarProjectionState {
    /// Strip/projection fixtures below supply their admitted identities;
    /// real Building admission is exercised separately through production.
    pub(super) fn reconcile_fixture(
        &mut self,
        owner: InternedId,
        candidates: &[Cameo],
        rows: usize,
    ) -> CameoReconcile {
        let additions: Vec<_> = candidates
            .iter()
            .filter_map(|item| match item.id {
                CameoId::Object(id) if item.available => Some(id),
                _ => None,
            })
            .collect();
        for &type_id in &additions {
            self.apply_lifecycle_output(
                &LifecycleOutput::SidebarConstructionOption { owner, type_id },
                Some(owner),
            );
        }
        self.reconcile_cameos(owner, candidates, rows, &additions)
    }
}

fn world(rules: &RuleSet, factories: &[&str]) -> (Simulation, InternedId) {
    let mut sim = Simulation::with_seed(0x6A84_2000);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, true, 50_000, 10);
    house.project_country_mults(rules, sim.interner());
    sim.houses_mut_for_test().register(house);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    for (index, name) in factories.iter().enumerate() {
        let x = 4 + (index % 4) as u16 * 7;
        let y = 4 + (index / 4) as u16 * 7;
        sim.spawn_object(name, "Americans", x, y, 0, rules)
            .unwrap_or_else(|| panic!("place sidebar fixture factory {name}"));
    }
    (sim, owner)
}

#[cfg(feature = "ares")]
#[test]
fn superweapon_money_cameo_refresh_tracks_wallet_without_membership_rebuild() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n[SuperWeaponTypes]\n0=Paid\n[Paid]\nType=PsychicReveal\nMoney.Amount=-50001\n"
        ), &crate::rules::ini_parser::IniFile::empty(),
    ).unwrap();
    let (mut sim, owner) = world(&rules, &[]);
    let id = sim.interner().get("Paid").unwrap();
    let mut instance = crate::sim::superweapon::SuperWeaponInstance::new(id, owner, 0);
    instance.set_is_active_for_test(true);
    instance.set_is_ready_for_test(true);
    sim.super_weapons_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(id, instance);
    let supers = crate::sim::superweapon::superweapon_views_for_owner(&sim, &rules, &owner);
    let mut projection = SidebarProjectionState::default();
    let candidates = cameo_candidates(&sim, &rules, None, owner, &[], &[], &supers);
    projection.reconcile_fixture(owner, &candidates, 4);
    let mut view = build_sidebar_view_with_spec(
        SidebarChromeLayoutSpec::stock(),
        800.0,
        600.0,
        SidebarTab::Defense,
        50_000,
        0,
        0,
        None,
        &[],
        &[],
        &[],
        None,
        &[],
        Some(sim.interner()),
        &supers,
        &SidebarGadgetState::new(),
        None,
        None,
        None,
        None,
        [None; 2],
        projection.cameo_strips(),
    );
    assert_eq!(view.items.len(), 1);
    assert!(!view.items[0].enabled);
    let order = projection.cameo_strips().saved_identities();
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .economy_mut_for_test()
        .add_credits(1);
    let before = sim.state_hash();
    projection.apply_cameo_draw_state(&mut view, &sim, &rules, owner, None);
    assert!(view.items[0].enabled);
    assert!(view.items[0].is_ready);
    assert_eq!(sim.state_hash(), before);
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .economy_mut_for_test()
        .add_credits(-1);
    projection.apply_cameo_draw_state(&mut view, &sim, &rules, owner, None);
    assert!(!view.items[0].enabled);
    assert!(view.items[0].is_ready);
    assert_eq!(projection.cameo_strips().saved_identities(), order);
}

fn view(
    sim: &Simulation,
    options: &[BuildOption],
    projection: &SidebarProjectionState,
    tab: SidebarTab,
    height: f32,
) -> SidebarView {
    view_with_gadgets(
        sim,
        options,
        projection,
        tab,
        height,
        &SidebarGadgetState::new(),
    )
}

fn view_with_gadgets(
    sim: &Simulation,
    options: &[BuildOption],
    projection: &SidebarProjectionState,
    tab: SidebarTab,
    height: f32,
    gadgets: &SidebarGadgetState,
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
        options,
        &[],
        None,
        &[],
        Some(sim.interner()),
        &[],
        gadgets,
        None,
        None,
        None,
        None,
        [None; 2],
        projection.cameo_strips(),
    )
}

#[test]
fn retail_ready_buildings_flash_retained_tabs_and_stop_after_cancel() {
    use crate::sim::production::{self, ProductionCategory};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, owner) = world(rules, &["GACNST", "GAPOWR", "GAPILE"]);
    let options = all_build_options_for_owner(&sim, rules, "Americans");
    let candidates = cameo_candidates(&sim, rules, None, owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    projection.reconcile_fixture(owner, &candidates, 1);
    for kind in ["GAPOWR", "GAPILL"] {
        assert!(production::enqueue_by_type(
            &mut sim,
            rules,
            "Americans",
            kind,
            crate::sim::world::FrameEffects::default(),
            false
        ));
    }
    projection.reconcile_factory_links(&sim, owner);
    assert_eq!(projection.ready_building_tabs(&sim, owner), [false; 2]);
    for category in [ProductionCategory::Building, ProductionCategory::Defense] {
        assert!(
            sim.production_mut_for_test()
                .factories_mut_for_test()
                .test_arm_ready(owner, category)
        );
    }
    let ready = projection.ready_building_tabs(&sim, owner);
    assert_eq!(ready, [true; 2]);
    assert_eq!(
        SidebarProjectionState::default().ready_building_tabs(&sim, owner),
        [false; 2]
    );
    let foreign_owner = sim.intern("Russians");
    assert_eq!(
        projection.ready_building_tabs(&sim, foreign_owner),
        [false; 2]
    );

    let mut gadgets = SidebarGadgetState::default();
    gadgets.update_tab_flashes(10, 10, [ready[0], ready[1], false, false]);
    // Flashing belongs to every retained strip, including the tabs whose
    // cameos are absent from the current view. Both active and inactive
    // buttons consume the pulsing state through the production view builder.
    let other_tab = view_with_gadgets(
        &sim,
        &options,
        &projection,
        SidebarTab::Vehicle,
        480.0,
        &gadgets,
    );
    assert_eq!(other_tab.tabs[0].frame_index, 3);
    assert_eq!(other_tab.tabs[1].frame_index, 3);
    let building_tab = view_with_gadgets(
        &sim,
        &options,
        &projection,
        SidebarTab::Building,
        480.0,
        &gadgets,
    );
    assert_eq!(building_tab.tabs[0].frame_index, 4);
    gadgets.update_tab_flashes(29, 29, [true, true, false, false]);
    let normal = view_with_gadgets(
        &sim,
        &options,
        &projection,
        SidebarTab::Building,
        480.0,
        &gadgets,
    );
    assert_eq!(normal.tabs[0].frame_index, 1);
    assert_eq!(normal.tabs[1].frame_index, 0);

    for (kind, expected) in [("GAPOWR", [false, true]), ("GAPILL", [false, false])] {
        assert!(production::cancel_production_for_owner(
            &mut sim,
            rules,
            "Americans",
            production::ProductionCancellation::Type {
                type_id: kind,
                naval: false
            },
            false,
            crate::sim::world::FrameEffects::default()
        ));
        assert_eq!(projection.ready_building_tabs(&sim, owner), expected);
    }
    gadgets.update_tab_flashes(30, 30, [false; 4]);
    assert!(!gadgets.tab_flashes[0].is_active());
    assert!(!gadgets.tab_flashes[1].is_active());
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
fn trigger_cameo_flash_reaches_view_and_survives_projection_save_lifecycle() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/trigger_cameos.json",
    ))
    .unwrap();
    let (sim, rules, owner, csf) = native_context_world(&serde_json::json!({
        "current_side": 0, "types": native["catalog"]
    }));
    let options: Vec<_> = all_build_options_for_owner(&sim, &rules, "Americans")
        .into_iter()
        .filter(|item| sim.interner().resolve(item.type_id) != "UNLISTED")
        .collect();
    let candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    projection.reconcile_fixture(owner, &candidates, 3);
    let control = &native["flash_controls"][0];
    projection.flash_cameo(
        sim.interner().get("HTNK").unwrap(),
        control["duration"].as_i64().unwrap() as i32,
        control["frame"].as_i64().unwrap() as i32,
        sim.interner(),
        &rules,
    );
    projection.set_cameo_frame(control["frame"].as_i64().unwrap() as i32);
    // Projection key refresh must not clobber entry+88. Native's numeric-only
    // index collision flashes ORCA rather than the requested HTNK.
    projection.reconcile_fixture(owner, &candidates, 3);
    let vehicle_view = view(&sim, &options, &projection, SidebarTab::Vehicle, 600.);
    assert_eq!(
        vehicle_view
            .items
            .iter()
            .filter(|item| item.flash_darkened)
            .map(|item| item.type_id.as_str())
            .collect::<Vec<_>>(),
        ["ORCA"]
    );
    let saved = projection.saved_order().unwrap();
    let bytes = save_codec::serialize(&saved).unwrap();
    let saved: crate::sim::snapshot::SavedSidebarOrder = save_codec::deserialize(&bytes).unwrap();
    let restored =
        super::sidebar_projection::prepare_saved_order(Some(saved.clone()), &sim, &rules).unwrap();
    let mut loaded = SidebarProjectionState::default();
    loaded.restore_order(restored);
    loaded.set_cameo_frame(123);
    loaded.reconcile_fixture(owner, &candidates, 3);
    assert_eq!(loaded.saved_order(), Some(saved));
    assert!(
        view(&sim, &options, &loaded, SidebarTab::Vehicle, 600.)
            .items
            .iter()
            .any(|item| item.type_id == "ORCA" && item.flash_darkened)
    );
    let mut removed = candidates.clone();
    removed
        .iter_mut()
        .find(|item| item.id == CameoId::Object(sim.interner().get("ORCA").unwrap()))
        .unwrap()
        .available = false;
    loaded.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    loaded.reconcile_fixture(owner, &removed, 3);
    loaded.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    loaded.reconcile_fixture(owner, &candidates, 3);
    assert!(
        !view(&sim, &options, &loaded, SidebarTab::Vehicle, 600.)
            .items
            .iter()
            .any(|item| item.flash_darkened)
    );
    loaded.reset_cameo_seed();
    loaded.reconcile_fixture(owner, &candidates, 3);
    loaded.set_cameo_frame(123);
    assert!(
        !view(&sim, &options, &loaded, SidebarTab::Vehicle, 600.)
            .items
            .iter()
            .any(|item| item.flash_darkened)
    );
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
                .any(|name| name.as_str().unwrap() == sim.interner().resolve(item.type_id))
        })
        .collect();
    let mut candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    projection.reconcile_fixture(owner, &candidates, 3);
    projection.set_scroll_row(SidebarTab::Vehicle, 1);
    for candidate in &mut candidates {
        let CameoId::Object(id) = candidate.id else {
            unreachable!()
        };
        candidate.available = !case["remove_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name.as_str().unwrap() == sim.interner().resolve(id));
    }
    projection.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    projection.reconcile_fixture(owner, &candidates, 3);
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
                .interner()
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
        !projection
            .reconcile_fixture(owner, &candidates, 3)
            .build_inserted(),
        "scenario seeding is silent"
    );
    assert!(
        !projection
            .reconcile_fixture(owner, &candidates, 3)
            .build_inserted(),
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
                        at_build_limit: item.at_build_limit,
                    },
                );
                let option = options
                    .iter()
                    .find(|option| sim.interner().resolve(option.type_id) == item.type_id)
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
    let tank = CameoId::Object(sim.interner().get("tank").unwrap());
    initial
        .iter_mut()
        .find(|item| item.id == tank)
        .unwrap()
        .available = false;
    let mut projection = SidebarProjectionState::default();
    assert!(
        !projection
            .reconcile_fixture(owner, &initial, 3)
            .build_inserted()
    );
    assert!(
        !projection
            .reconcile_fixture(owner, &candidates, 3)
            .build_inserted(),
        "a refresh does not add ordinary entries without a House pass"
    );
    projection.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    assert!(
        projection
            .reconcile_fixture(owner, &candidates, 3)
            .build_inserted()
    );
    assert!(
        !projection
            .reconcile_fixture(owner, &candidates, 3)
            .build_inserted()
    );
    projection.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    assert!(
        !projection
            .reconcile_fixture(owner, &initial, 3)
            .build_inserted(),
        "removal is silent"
    );
    projection.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    assert!(
        projection
            .reconcile_fixture(owner, &candidates, 3)
            .build_inserted(),
        "a later accepted reinsertion announces again"
    );
    let other = sim.interner().get("TESTBUILD").unwrap();
    assert!(
        !projection
            .reconcile_fixture(other, &candidates, 3)
            .build_inserted(),
        "local-house replacement seeds silently"
    );
}

#[test]
fn a_warped_factory_keeps_old_infantry_cameos_but_cannot_add_a_new_one() {
    // Building4456D0 requires +660 for an AddCameo attempt; retained
    // Strip6AA600 deliberately asks FindFactory with require_online=false.
    // The production capture house-admission-before-run1 discriminates this
    // with stock NAHAND/CLEG and a CASLAB grant arriving during the warp.
    let (mut sim, rules, owner, csf, _) = native_history_world();
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    let candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    let new_infantry = CameoId::Object(sim.interner().get("engineer").unwrap());
    let initial: Vec<_> = candidates
        .iter()
        .filter(|entry| entry.id != new_infantry)
        .cloned()
        .collect();
    let mut projection = SidebarProjectionState::default();
    let seed: Vec<_> = initial
        .iter()
        .filter_map(|entry| match entry.id {
            CameoId::Object(id) if entry.available => Some(id),
            _ => None,
        })
        .collect();
    projection.reconcile_cameos(owner, &initial, 3, &seed);
    let factory = sim
        .entities()
        .iter_sorted()
        .find(|(_, entity)| {
            sim.object_type(entity.type_ref(), &rules)
                .is_some_and(|object| object.id == "TESTINF")
        })
        .unwrap()
        .0;
    // This strip fixture supplies an already discovered barracks; it does
    // not run the map's Cell PUT/discovery transaction.
    sim.entities_mut_for_test()
        .get_mut(factory)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(true);
    sim.entities_mut_for_test()
        .get_mut(factory)
        .unwrap()
        .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(
            factory,
        ));
    assert!(!sim.entities().get(factory).unwrap().building_online());
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    let candidates = cameo_candidates(&sim, &rules, Some(&csf), owner, &options, &[], &[]);
    for type_id in crate::sim::production::construction_options_for_owner(&sim, &rules, owner) {
        projection.apply_lifecycle_output(
            &LifecycleOutput::SidebarConstructionOption { owner, type_id },
            Some(owner),
        );
    }
    projection.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    projection.reconcile_cameos(owner, &candidates, 3, &[]);
    let infantry = projection.cameo_strips().items(SidebarTab::Infantry);
    assert!(
        infantry
            .iter()
            .any(|entry| entry.id == CameoId::Object(sim.interner().get("gi").unwrap()))
    );
    assert!(
        !infantry.iter().any(|entry| entry.id == new_infantry),
        "retention admission must not insert an infantry option while its adding factory is warped"
    );
    sim.entities_mut_for_test()
        .get_mut(factory)
        .unwrap()
        .set_temporal_for_test(Default::default());
    // Thawing after that House pass cannot retroactively add an option.
    projection.reconcile_cameos(owner, &candidates, 3, &[]);
    assert!(
        !projection
            .cameo_strips()
            .items(SidebarTab::Infantry)
            .iter()
            .any(|entry| entry.id == new_infantry)
    );
    for type_id in crate::sim::production::construction_options_for_owner(&sim, &rules, owner) {
        projection.apply_lifecycle_output(
            &LifecycleOutput::SidebarConstructionOption { owner, type_id },
            Some(owner),
        );
    }
    projection.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    assert!(
        projection
            .reconcile_cameos(owner, &candidates, 3, &[])
            .build_inserted()
    );
    assert!(
        projection
            .cameo_strips()
            .items(SidebarTab::Infantry)
            .iter()
            .any(|entry| entry.id == new_infantry)
    );
}

#[test]
fn a_house_recheck_stays_pending_until_the_projection_runs_it() {
    // House4F92E2 is event-driven, so the strip pass it requested must run in
    // the frame that produced it rather than wait for the next player input.
    let mut projection = SidebarProjectionState::default();
    let owner = InternedId::default();
    assert!(!projection.cameo_recheck_pending());
    projection.apply_lifecycle_output(&LifecycleOutput::SidebarRecalculated { owner }, Some(owner));
    assert!(projection.cameo_recheck_pending());
    projection.reconcile_cameos(owner, &[], 3, &[]);
    assert!(!projection.cameo_recheck_pending());
}

#[test]
fn building_production_darkens_other_cameos_until_cancelled() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        .map(|retail| retail.rules)
    else {
        return;
    };
    let (mut sim, owner) = world(&rules, &["GACNST", "GAPOWR", "GAPILE"]);
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    let candidates = cameo_candidates(&sim, &rules, None, owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    projection.reconcile_fixture(owner, &candidates, 12);
    let drawn = view(&sim, &options, &projection, SidebarTab::Building, 1200.0);
    projection.replace_view(Some(drawn));
    assert!(crate::sim::production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GAPOWR",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    projection.reconcile_factory_links(&sim, owner);
    projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
    let items = &projection.view().unwrap().items;
    assert!(
        items
            .iter()
            .find(|item| item.type_id == "GAPOWR")
            .unwrap()
            .enabled
    );
    assert!(
        !items
            .iter()
            .find(|item| item.type_id == "GAPILE")
            .unwrap()
            .enabled,
        "StripDraw6A95C8..6A95E9 sees an active request anywhere in the strip"
    );
    assert!(sim.suspend_production_for_test(
        "Americans",
        crate::sim::production::ProductionCategory::Building,
    ));
    projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
    assert!(
        projection
            .view()
            .unwrap()
            .items
            .iter()
            .find(|item| item.type_id == "GAPOWR")
            .unwrap()
            .is_on_hold
    );
    assert!(
        !projection
            .view()
            .unwrap()
            .items
            .iter()
            .find(|item| item.type_id == "GAPILE")
            .unwrap()
            .enabled
    );
    assert!(crate::sim::production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GAPOWR",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    for _ in 0..4096 {
        let frame = sim
            .advance_app_frame(
                &[],
                &rules,
                None,
                67,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        for output in &frame.lifecycle_outputs {
            projection.apply_lifecycle_output(output, Some(owner));
        }
        projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
        if projection
            .view()
            .unwrap()
            .items
            .iter()
            .any(|item| item.type_id == "GAPOWR" && item.is_ready)
        {
            break;
        }
    }
    let items = &projection.view().unwrap().items;
    let power = items.iter().find(|item| item.type_id == "GAPOWR").unwrap();
    assert!(power.enabled && power.is_ready && power.progress == 1.0);
    assert!(
        !items
            .iter()
            .find(|item| item.type_id == "GAPILE")
            .unwrap()
            .enabled,
        "ready-to-place still owns the strip"
    );
    assert!(crate::sim::production::cancel_production_for_owner(
        &mut sim,
        &rules,
        "Americans",
        crate::sim::production::ProductionCancellation::Type {
            type_id: "GAPOWR",
            naval: false
        },
        false,
        crate::sim::world::FrameEffects::default(),
    ));
    projection.reconcile_factory_links(&sim, owner);
    projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
    assert!(
        projection
            .view()
            .unwrap()
            .items
            .iter()
            .find(|item| item.type_id == "GAPILE")
            .unwrap()
            .enabled,
        "cancellation releases the strip without another input"
    );
    let power = projection
        .view()
        .unwrap()
        .items
        .iter()
        .find(|item| item.type_id == "GAPOWR")
        .unwrap();
    assert!(!power.is_ready && !power.is_on_hold && power.progress == 0.0);
}

#[test]
fn busy_strip_draw_matches_native_pending_and_linked_controls() {
    use crate::sim::command::{Command, CommandEnvelope};
    let Some(rules) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        .map(|retail| retail.rules)
    else {
        return;
    };
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/draw_availability.json",
    ))
    .unwrap();
    for control in native["controls"].as_array().unwrap() {
        let state = control["state"].as_str().unwrap();
        // State3 is a synthetic draw control with no established writer;
        // Ready has its own production lifecycle test above.
        if !matches!(state, "idle" | "pending" | "pending_hold" | "building") {
            continue;
        }
        let (mut sim, owner) = world(
            &rules,
            &[
                "GACNST", "GAPOWR", "GAPILE", "GAREFN", "GAWEAP", "GAAIRC", "GATECH",
            ],
        );
        let options = all_build_options_for_owner(&sim, &rules, "Americans");
        let candidates = cameo_candidates(&sim, &rules, None, owner, &options, &[], &[]);
        let mut projection = SidebarProjectionState::default();
        projection.reconcile_fixture(owner, &candidates, 12);
        let source = control["source"].as_str().unwrap();
        if state == "idle" {
            // A scripted future order must not lock today's strip. The
            // scheduler admits the next ordinary frame, not every queued tick.
            sim.queue_command(CommandEnvelope::new(
                owner,
                sim.session().tick() + 100,
                Command::QueueProduction {
                    type_id: sim.interner().get(source).unwrap(),
                },
            ));
            // A different player's due orders must not hold this local strip.
            let foreign = sim.intern("Russians");
            sim.queue_commands([
                CommandEnvelope::new(
                    foreign,
                    sim.session().tick() + 1,
                    Command::QueueProduction {
                        type_id: sim.interner().get(source).unwrap(),
                    },
                ),
                CommandEnvelope::new(
                    foreign,
                    sim.session().tick() + 1,
                    Command::SuspendProduction {
                        category: crate::sim::production::category_for_object(
                            rules.object(source).unwrap(),
                        ),
                    },
                ),
            ]);
        }
        if matches!(state, "pending" | "pending_hold") {
            let category =
                crate::sim::production::category_for_object(rules.object(source).unwrap());
            let type_id = sim.interner().get(source).unwrap();
            let click = crate::app::input::sidebar_eva::cameo_click(
                &[],
                category,
                Some(type_id),
                None,
                projection.cameo_request(type_id),
                crate::app::input::sidebar_eva::CameoPress::Left,
                false,
            );
            projection.record_cameo_click(&sim, &rules, owner, type_id, click);
            if state == "pending_hold" {
                let click = crate::app::input::sidebar_eva::cameo_click(
                    &[],
                    category,
                    Some(type_id),
                    None,
                    projection.cameo_request(type_id),
                    crate::app::input::sidebar_eva::CameoPress::Right { shift: false },
                    false,
                );
                projection.record_cameo_click(&sim, &rules, owner, type_id, click);
            }
        } else if state == "building" {
            assert!(
                crate::sim::production::enqueue_by_type(
                    &mut sim,
                    &rules,
                    "Americans",
                    source,
                    crate::sim::world::FrameEffects::default(),
                    false,
                ),
                "{control}"
            );
        }
        projection.reconcile_factory_links(&sim, owner);
        let tab = SidebarTab::all()[control["tab"].as_u64().unwrap() as usize];
        let drawn = view(&sim, &options, &projection, tab, 2400.0);
        projection.replace_view(Some(drawn));
        projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
        for expected in control["entries"].as_array().unwrap() {
            let name = expected["type"].as_str().unwrap();
            let item = projection
                .view()
                .unwrap()
                .items
                .iter()
                .find(|item| item.type_id == name)
                .unwrap();
            assert_eq!(
                !item.enabled,
                expected["unavailable"].as_bool().unwrap(),
                "{control}: {name}"
            );
        }
        // Building and defense are different retained strips.
        if tab == SidebarTab::Building && state != "idle" {
            let drawn = view(&sim, &options, &projection, SidebarTab::Defense, 2400.0);
            projection.replace_view(Some(drawn));
            projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
            assert!(
                projection
                    .view()
                    .unwrap()
                    .items
                    .iter()
                    .find(|item| item.type_id == "GAPILL")
                    .unwrap()
                    .enabled
            );
        }
    }
}

#[test]
fn a_retained_cameo_darkens_on_the_frame_its_restriction_appears() {
    // `StripClass::Draw @ 0x006A9540` re-asks `FindFactory(1, 1, 1)`,
    // `CanBuild(type, 0, 0)` and `CheckBuildLimit` for every entry it draws.
    // A BuildLimit reached between two House passes must darken the retained
    // cameo without another projection rebuild or an unrelated input.
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let (mut sim, owner) = world(
        &rules,
        &[
            "GACNST", "GAPOWR", "GAPILE", "GAREFN", "GAWEAP", "GAAIRC", "GATECH",
        ],
    );
    let options = all_build_options_for_owner(&sim, &rules, "Americans");
    let candidates = cameo_candidates(&sim, &rules, None, owner, &options, &[], &[]);
    let mut projection = SidebarProjectionState::default();
    projection.reconcile_fixture(owner, &candidates, 12);
    // Retail `BuildLimit=1` types on both factory kinds: the Chronosphere on
    // the Buildings factory and Tanya on the house's Infantry factory.
    let drawn_state = |projection: &SidebarProjectionState, type_id: &str| {
        let item = projection
            .view()
            .expect("reviewed projection")
            .items
            .iter()
            .find(|item| item.type_id == type_id)
            .unwrap_or_else(|| panic!("the base admits the {type_id} cameo"));
        (item.enabled, item.at_build_limit)
    };
    let drawn = view(&sim, &options, &projection, SidebarTab::Defense, 2400.0);
    projection.replace_view(Some(drawn));
    projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
    assert_eq!(drawn_state(&projection, "GACSPH"), (true, false));

    // The house builds the one Chronosphere its `BuildLimit=1` allows.
    sim.spawn_object("GACSPH", "Americans", 30, 4, 0, &rules)
        .expect("place the Chronosphere");
    projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
    assert_eq!(
        drawn_state(&projection, "GACSPH"),
        (false, true),
        "the retained cameo draws the unavailable overlay and gates the press"
    );

    let drawn = view(&sim, &options, &projection, SidebarTab::Infantry, 2400.0);
    projection.replace_view(Some(drawn));
    projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
    assert_eq!(drawn_state(&projection, "TANY"), (true, false));
    sim.spawn_object("TANY", "Americans", 4, 20, 0, &rules)
        .expect("place Tanya");
    projection.refresh_cameo_draw_state(&sim, &rules, owner, None);
    assert_eq!(
        drawn_state(&projection, "TANY"),
        (false, true),
        "the same Draw queries cover a house factory's retained infantry cameo"
    );
}

#[test]
fn retail_rules_admission_and_csf_names_keep_the_same_identity_in_every_view() {
    // Extracted RULESMD/ARTMD are an explicit base-rules fixture, not a claim
    // about all mode/map layers. The separate native replay supplies order.
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
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
    assert!(
        !projection
            .reconcile_fixture(owner, &candidates, 3)
            .build_inserted()
    );
    let visible: HashSet<_> = options
        .iter()
        .filter(|item| item.visible_in_sidebar())
        .map(|item| item.type_id)
        .collect();
    assert!(
        visible.contains(&sim.interner().get("E1").unwrap()),
        "placed Allied barracks exposes the GI"
    );
    assert!(
        !visible.contains(&sim.interner().get("E2").unwrap()),
        "foreign infantry is not admitted by the Allied factory"
    );
    let mut observed = HashSet::new();
    for tab in SidebarTab::all() {
        let view = view(&sim, &options, &projection, tab, 2400.0);
        for item in &view.items {
            let id = sim.interner().get(&item.type_id).unwrap();
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

/// Supplied charge/hold flags isolate retained membership from the live click
/// decision. Native CanFire/Select controls and real retail power transitions
/// are covered separately by the superweapon cameo tests.
#[test]
fn superweapon_retained_cameo_keeps_identity_through_hold_and_recovery() {
    use crate::app::input::state::MatchInputState;
    use crate::app::input::superweapon_cameo::{self, SuperWeaponClick};
    use crate::app::types::TargetingMode;
    use crate::sim::superweapon::{SuperWeaponInstance, superweapon_views_for_owner};

    fn sw_cameo(
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        projection: &mut SidebarProjectionState,
        armed: Option<&ArmedSidebarEntry>,
    ) -> (crate::ui::sidebar::SidebarItem, CameoReconcile) {
        let sw_views = superweapon_views_for_owner(sim, rules, &owner);
        let candidates = cameo_candidates(sim, rules, None, owner, &[], &[], &sw_views);
        let change = projection.reconcile_cameos(owner, &candidates, 3, &[]);
        let ids: Vec<_> = projection
            .cameo_strips()
            .items(SidebarTab::Defense)
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids, vec![CameoId::SuperWeapon(sw_views[0].type_id)]);
        let sidebar = build_sidebar_view_with_spec(
            SidebarChromeLayoutSpec::stock(),
            800.0,
            430.0,
            SidebarTab::Defense,
            50_000,
            1_000,
            0,
            None,
            &[],
            &[],
            &[],
            armed,
            &[],
            Some(sim.interner()),
            &sw_views,
            &SidebarGadgetState::new(),
            None,
            None,
            None,
            None,
            [None; 2],
            projection.cameo_strips(),
        );
        assert_eq!(
            sidebar.items.len(),
            1,
            "one retained Super, without duplicates"
        );
        (sidebar.items[0].clone(), change)
    }

    fn click(
        sim: &Simulation,
        rules: &RuleSet,
        owner: InternedId,
        item: &crate::ui::sidebar::SidebarItem,
        right: bool,
        input: &mut MatchInputState,
        gadgets: &mut SidebarGadgetState,
    ) -> SuperWeaponClick {
        let SidebarAction::SuperWeaponPress {
            section,
            right: routed_right,
        } = hit_test_item(item, right, false)
        else {
            panic!("held and charging cameos must still forward their press");
        };
        assert_eq!(section, "NukeSpecial");
        assert_eq!(routed_right, right);
        let views = superweapon_views_for_owner(sim, rules, &owner);
        superweapon_cameo::press(
            input,
            gadgets,
            views.iter().find(|view| view.display_name == section),
            rules.super_weapon(&section).unwrap().action.as_deref(),
            right,
        )
    }

    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [SuperWeaponTypes]\n0=NukeSpecial\n\
         [NukeSpecial]\nType=MultiMissile\nAction=Nuke\nRechargeTime=10\nIsPowered=yes\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    let owner = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let sw_type = sim.intern("NukeSpecial");
    let mut instance = SuperWeaponInstance::new(sw_type, owner, 0);
    instance.set_is_active_for_test(true);
    instance.set_is_ready_for_test(true);
    sim.super_weapons_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(sw_type, instance);
    let mut projection = SidebarProjectionState::default();
    let (online, initial) = sw_cameo(&sim, &rules, owner, &mut projection, None);
    assert!(online.enabled && online.is_ready);
    assert_eq!(
        initial
            .transitions()
            .tab_button_disabled(SidebarTab::Defense),
        Some(false)
    );

    sim.super_weapons_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw_type)
        .unwrap()
        .set_is_suspended_for_test(true);
    let (held, change) = sw_cameo(&sim, &rules, owner, &mut projection, None);
    assert_eq!(
        change,
        CameoReconcile::default(),
        "hold keeps the retained entry"
    );
    assert!(!held.enabled && held.is_ready);
    let mut input = MatchInputState::new(Default::default());
    let mut gadgets = SidebarGadgetState::new();
    input.begin_building_placement("BUILDING", 0, owner);
    gadgets.repair_mode_on = true;
    // Cached online draw flags cannot admit the newly held live Super.
    assert_eq!(
        click(
            &sim,
            &rules,
            owner,
            &online,
            false,
            &mut input,
            &mut gadgets
        ),
        SuperWeaponClick::Refused
    );
    assert!(
        input
            .targeting_mode
            .as_ref()
            .unwrap()
            .as_building_placement()
            .is_some()
    );
    assert!(gadgets.repair_mode_on);

    input.targeting_mode = Some(TargetingMode::SuperWeapon("NukeSpecial".into()));
    let (armed_held, change) = sw_cameo(
        &sim,
        &rules,
        owner,
        &mut projection,
        Some(&ArmedSidebarEntry::SuperWeapon("NukeSpecial".into())),
    );
    assert_eq!(change, CameoReconcile::default());
    assert!(armed_held.is_armed);
    assert_eq!(
        click(
            &sim,
            &rules,
            owner,
            &armed_held,
            false,
            &mut input,
            &mut gadgets
        ),
        SuperWeaponClick::Refused
    );
    assert_eq!(
        input.targeting_mode.as_ref().unwrap().as_super_weapon(),
        Some("NukeSpecial")
    );
    assert_eq!(
        click(&sim, &rules, owner, &held, true, &mut input, &mut gadgets),
        SuperWeaponClick::None
    );
    assert!(
        input.targeting_mode.is_none(),
        "held right-click cancels targeting"
    );

    sim.super_weapons_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw_type)
        .unwrap()
        .set_is_suspended_for_test(false);
    let (recovered, change) = sw_cameo(&sim, &rules, owner, &mut projection, None);
    assert_eq!(
        change,
        CameoReconcile::default(),
        "recovery does not reinsert the cameo"
    );
    assert!(recovered.enabled && recovered.is_ready);
    // Cached offline draw flags cannot veto the now-admitted live Super.
    assert_eq!(
        click(&sim, &rules, owner, &held, false, &mut input, &mut gadgets),
        SuperWeaponClick::Target
    );
    assert!(!gadgets.repair_mode_on);
    assert_eq!(
        input.targeting_mode.as_ref().unwrap().as_super_weapon(),
        Some("NukeSpecial")
    );

    sim.super_weapons_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw_type)
        .unwrap()
        .set_is_ready_for_test(false);
    let (charging, change) = sw_cameo(&sim, &rules, owner, &mut projection, None);
    assert_eq!(
        change,
        CameoReconcile::default(),
        "charging keeps membership too"
    );
    assert!(!charging.is_ready);
    assert_eq!(
        click(
            &sim,
            &rules,
            owner,
            &charging,
            false,
            &mut input,
            &mut gadgets
        ),
        SuperWeaponClick::Refused
    );
}
