//! The production save envelope and candidate transaction retain strip history.
//! Native lifecycle: Mouse Save5BE6D0 / Load5BDF70 / no-init5BE9B0.

use super::*;
use crate::app::sidebar_projection::SidebarProjectionState;
use crate::sim::intern::InternedId;
use crate::sim::snapshot::{SavedCameoId, SavedSidebarOrder};
use crate::ui::sidebar::SidebarTab;
use crate::ui::sidebar::cameo_order::{Cameo, CameoKey, CameoRequestState};
use std::fmt::Write;

fn fixture() -> (Simulation, RuleSet, InternedId, [InternedId; 2]) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[Americans]\nSide=GDI\n\
         [BuildingTypes]\n0=FIRST\n1=LATER\n\
         [FIRST]\nCost=400\nStrength=1000\nFoundation=1x1\nTechLevel=1\nOwner=Americans\n\
         [LATER]\nCost=200\nStrength=1000\nFoundation=1x1\nTechLevel=1\nOwner=Americans\n",
    ))
    .unwrap();
    let (sim, owner) = simulation(&rules);
    let types = [
        sim.interner().get("FIRST").unwrap(),
        sim.interner().get("LATER").unwrap(),
    ];
    (sim, rules, owner, types)
}

fn simulation(rules: &RuleSet) -> (Simulation, InternedId) {
    let mut sim = load_fixture_simulation(true);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 50_000, 10,
        ));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    *sim.scenario_rng_mut_for_test() = crate::sim::rng::SimRng::new(0);
    (sim, owner)
}

const SAVED_SCROLL_ROWS: [usize; 4] = [1, 2, 3, 4];

fn scrolling_fixture() -> (Simulation, RuleSet, InternedId, Vec<Cameo>) {
    // Different odd counts exercise the last partial row on every strip.
    // These are authored save inputs; native removal arithmetic is covered
    // by the original-executable corpus at the shared strip owner.
    let names: [Vec<String>; 4] = std::array::from_fn(|tab| {
        (0..[3, 5, 7, 9][tab])
            .map(|index| format!("SCROLL{tab}_{index}"))
            .collect()
    });
    let mut ini =
        String::from("[Countries]\n0=Americans\n[Sides]\nGDI=Americans\n[Americans]\nSide=GDI\n");
    for (registry, tabs) in [
        ("BuildingTypes", &[0, 1][..]),
        ("InfantryTypes", &[2][..]),
        ("VehicleTypes", &[3][..]),
    ] {
        writeln!(ini, "[{registry}]").unwrap();
        for (index, name) in tabs.iter().flat_map(|&tab| &names[tab]).enumerate() {
            writeln!(ini, "{index}={name}").unwrap();
        }
    }
    for (tab, names) in names.iter().enumerate() {
        for (index, name) in names.iter().enumerate() {
            writeln!(
                ini,
                "[{name}]\nCost={}\nStrength=1000\nTechLevel=1\n\
                 Owner=Americans\nAIBasePlanningSide=0\nUIName=NAME:{name}",
                100 * (index + 1),
            )
            .unwrap();
            if tab == SidebarTab::Defense.tab_index() {
                writeln!(ini, "BuildCat=Combat").unwrap();
            }
        }
    }
    let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    let (sim, owner) = simulation(&rules);
    let candidates = SidebarTab::all()
        .into_iter()
        .flat_map(|tab| {
            names[tab.tab_index()]
                .iter()
                .map(|name| {
                    let kind = rules.object(name).unwrap();
                    Cameo {
                        id: SavedCameoId::Object(sim.interner().get(name).unwrap()),
                        tab,
                        available: true,
                        key: CameoKey::Techno {
                            own_side: true,
                            considered_aircraft: false,
                            naval: false,
                            tech_level: kind.tech_level,
                            cost: sim.cost_of(owner, kind, &rules),
                            name: kind.ui_name.clone().unwrap(),
                        },
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    (sim, rules, owner, candidates)
}

fn scrolled_projection(owner: InternedId, candidates: &[Cameo]) -> SidebarProjectionState {
    let mut projection = SidebarProjectionState::default();
    assert!(
        !projection
            .reconcile_fixture(owner, candidates, 1)
            .build_inserted()
    );
    for (tab, row) in SidebarTab::all().into_iter().zip(SAVED_SCROLL_ROWS) {
        projection.set_scroll_row(tab, row);
    }
    projection
}

fn candidates(types: [InternedId; 2], first_cost: i32) -> Vec<Cameo> {
    types
        .into_iter()
        .zip([first_cost, 200])
        .map(|(id, cost)| Cameo {
            id: SavedCameoId::Object(id),
            tab: SidebarTab::Building,
            available: true,
            key: CameoKey::Techno {
                own_side: true,
                considered_aircraft: false,
                naval: false,
                tech_level: 1,
                cost,
                name: String::new(),
            },
        })
        .collect()
}

fn retained_history(owner: InternedId, types: [InternedId; 2]) -> SidebarProjectionState {
    let mut projection = SidebarProjectionState::default();
    assert!(
        !projection
            .reconcile_fixture(owner, &candidates(types, 100), 3)
            .build_inserted()
    );
    // The current keys would sort LATER first, but native Recalculate retains
    // survivors. This records history through the actual shared strip owner.
    assert!(
        !projection
            .reconcile_fixture(owner, &candidates(types, 400), 3)
            .build_inserted()
    );
    assert_eq!(
        projection.saved_order().unwrap().strips()[0],
        types.map(SavedCameoId::Object)
    );
    projection
}

fn save(sim: &Simulation, rules: &RuleSet, order: Option<&SavedSidebarOrder>) -> Vec<u8> {
    GameSnapshot::save_validated_with_presentation(
        sim,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        "retained sidebar",
        1,
        &[(7, -12)],
        &[],
        order,
        None,
        None,
        None,
    )
}

fn prepare(
    bytes: &[u8],
    sim: &Simulation,
    rules: &RuleSet,
) -> Result<PreparedLoad, PreparedLoadError> {
    PreparedLoad::prepare_candidate(
        bytes,
        Some(sim),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(rules),
        Some(&load_fixture_terrain()),
        Some(&OverlayTypeRegistry::empty()),
    )
}

#[cfg(feature = "phobos")]
#[test]
fn sw_sidebar_expanded_flag_survives_the_snapshot_envelope() {
    let (sim, rules, owner, types) = fixture();
    let mut saved = retained_history(owner, types).saved_order().unwrap();
    saved.set_sw_sidebar_expanded(false);
    let bytes = save(&sim, &rules, Some(&saved));
    let loaded = GameSnapshot::load_validated(
        &bytes,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        LOAD_FIXTURE_MAP_NAME,
    )
    .unwrap();
    assert!(!loaded.sidebar_order().unwrap().sw_sidebar_expanded());
    let prepared = prepare(&bytes, &sim, &rules).unwrap();
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    let committed = prepared.commit_into(&mut runtime);
    assert!(
        !committed
            .sidebar_order
            .expect("retained sidebar")
            .sw_sidebar_expanded()
    );
}

#[test]
fn sidebar_order_envelope_roundtrips_without_changing_simulation_hash() {
    let (sim, rules, owner, types) = fixture();
    let saved = retained_history(owner, types).saved_order().unwrap();
    let hash = sim.state_hash();
    let bytes = save(&sim, &rules, Some(&saved));
    let loaded = GameSnapshot::load_validated(
        &bytes,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        LOAD_FIXTURE_MAP_NAME,
    )
    .unwrap();
    assert_eq!(loaded.sidebar_order(), Some(&saved));
    assert_eq!(loaded.sidebar_order().unwrap().owner(), owner);
    assert_eq!(loaded.sinking_waterlines, [(7, -12)]);
    assert_eq!(loaded.sim.state_hash(), hash);
    let bytes = GameSnapshot::save_validated(
        &sim,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        "headless",
        1,
    );
    let loaded = GameSnapshot::load_validated(
        &bytes,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        LOAD_FIXTURE_MAP_NAME,
    )
    .unwrap();
    assert!(loaded.sidebar_order().is_none());
    assert!(loaded.sinking_waterlines.is_empty());
    assert_eq!(loaded.sim.state_hash(), hash);
}

#[test]
fn sidebar_order_prepares_and_commits_unsorted_history_without_reinserting_survivors() {
    let (sim, rules, owner, types) = fixture();
    let saved = retained_history(owner, types).saved_order().unwrap();
    let bytes = save(&sim, &rules, Some(&saved));
    let mut outgoing = SidebarProjectionState::default();
    assert!(
        !outgoing
            .reconcile_fixture(owner, &candidates(types, 400), 3)
            .build_inserted()
    );
    let before = outgoing.saved_order().unwrap();
    assert_ne!(before, saved);
    let prepared = prepare(&bytes, &sim, &rules).unwrap();
    assert_eq!(outgoing.saved_order(), Some(before));
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    let committed = prepared.commit_into(&mut runtime);
    outgoing.restore_order(committed.sidebar_order);
    let entries = outgoing.cameo_strips().items(SidebarTab::Building);
    assert_eq!(
        entries.iter().map(|item| item.id).collect::<Vec<_>>(),
        saved.strips()[0]
    );
    assert!(matches!(entries[0].key, CameoKey::Techno { cost: 400, .. }));
    assert!(matches!(entries[1].key, CameoKey::Techno { cost: 200, .. }));
    assert_eq!(outgoing.saved_order(), Some(saved.clone()));
    assert!(
        !outgoing
            .reconcile_fixture(owner, &candidates(types, 400), 3)
            .build_inserted()
    );
    assert_eq!(outgoing.saved_order(), Some(saved));

    // A headless save has no retained UI history: replacing with it resets
    // the outgoing order, and the next normal seed remains silent.
    let headless = save(&runtime.simulation, &rules, None);
    let prepared = prepare(&headless, &runtime.simulation, &rules).unwrap();
    let committed = prepared.commit_into(&mut runtime);
    outgoing.restore_order(committed.sidebar_order);
    assert!(outgoing.saved_order().is_none());
    assert!(
        !outgoing
            .reconcile_fixture(owner, &candidates(types, 400), 3)
            .build_inserted()
    );
}

#[test]
fn nonzero_rows_on_every_strip_roundtrip_prepare_commit_and_refresh() {
    let (sim, rules, owner, candidates) = scrolling_fixture();
    let saved = scrolled_projection(owner, &candidates)
        .saved_order()
        .unwrap();
    assert_eq!(saved.clone().into_parts().2, SAVED_SCROLL_ROWS);
    let hash = sim.state_hash();
    let bytes = save(&sim, &rules, Some(&saved));
    let loaded = GameSnapshot::load_validated(
        &bytes,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        LOAD_FIXTURE_MAP_NAME,
    )
    .unwrap();
    assert_eq!(loaded.sidebar_order(), Some(&saved));
    assert_eq!(loaded.sim.state_hash(), hash);

    let mut outgoing = SidebarProjectionState::default();
    assert!(
        !outgoing
            .reconcile_fixture(owner, &candidates, 1)
            .build_inserted()
    );
    let before = outgoing.saved_order();
    assert_eq!(outgoing.cameo_strips().scroll_rows(), [0; 4]);
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime
        .simulation
        .houses_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .economy_mut_for_test()
        .add_credits(17);
    let outgoing_hash = runtime.simulation.state_hash();
    let outgoing_rng = runtime.simulation.rng_state();
    let prepared = prepare(&bytes, &runtime.simulation, &rules).unwrap();
    assert_eq!(
        outgoing.saved_order(),
        before,
        "prepare cannot install presentation"
    );
    assert_eq!(runtime.simulation.state_hash(), outgoing_hash);
    assert_eq!(runtime.simulation.rng_state(), outgoing_rng);
    let committed = prepared.commit_into(&mut runtime);
    outgoing.restore_order(committed.sidebar_order);
    let strips = outgoing.cameo_strips();
    assert_eq!(strips.scroll_rows(), SAVED_SCROLL_ROWS);
    assert_eq!(strips.saved_identities(), *saved.strips());
    assert_eq!(
        runtime.simulation.houses()[&owner].economy().credits(),
        50_000
    );
    assert_eq!(outgoing.saved_order(), Some(saved.clone()));
    assert!(
        !outgoing
            .reconcile_fixture(owner, &candidates, 1)
            .build_inserted()
    );
    assert_eq!(outgoing.saved_order(), Some(saved));
    for (tab, row) in SidebarTab::all().into_iter().zip(SAVED_SCROLL_ROWS) {
        assert_eq!(outgoing.cameo_strips().scroll_row(tab), row);
    }
}

#[test]
fn out_of_range_saved_rows_preserve_outgoing_scroll_and_running_world() {
    let (sim, rules, owner, candidates) = scrolling_fixture();
    let outgoing = scrolled_projection(owner, &candidates);
    let before = outgoing.saved_order();
    let valid = before.clone().unwrap();
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    for tab in SidebarTab::all() {
        for invalid_row in [SAVED_SCROLL_ROWS[tab.tab_index()] + 1, usize::MAX] {
            let mut rows = SAVED_SCROLL_ROWS;
            rows[tab.tab_index()] = invalid_row;
            let invalid = SavedSidebarOrder::new(owner, valid.strips().clone(), rows);
            let result = prepare(&save(&sim, &rules, Some(&invalid)), &sim, &rules);
            let Err(PreparedLoadError::SidebarOrder(reason)) = result else {
                panic!("out-of-range row {invalid_row} for {tab:?} must reject preparation");
            };
            assert!(reason.contains("invalid scroll row"), "{reason}");
            assert_eq!(outgoing.saved_order(), before);
            assert_eq!(outgoing.cameo_strips().scroll_rows(), SAVED_SCROLL_ROWS);
            assert_eq!(sim.state_hash(), hash);
            assert_eq!(sim.rng_state(), rng);
        }
    }
}

#[test]
fn invalid_sidebar_payload_preserves_outgoing_projection_and_running_world() {
    let (sim, rules, owner, types) = fixture();
    let outgoing = retained_history(owner, types);
    let before = outgoing.saved_order();
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let valid = before.clone().unwrap();
    let mut duplicate = valid.strips().clone();
    let first = duplicate[0][0];
    duplicate[0].push(first);
    let mut wrong_strip = std::array::from_fn(|_| Vec::new());
    wrong_strip[3].push(SavedCameoId::Object(types[0]));
    let mut unknown = valid.strips().clone();
    unknown[0][0] = SavedCameoId::Object(InternedId::from_index(u32::MAX));
    let mut wrong_kind = valid.strips().clone();
    wrong_kind[0][0] = SavedCameoId::SuperWeapon(types[0]);
    let mut over_capacity = std::array::from_fn(|_| Vec::new());
    over_capacity[0] = vec![SavedCameoId::Object(types[0]); 77];
    let invalid = [
        SavedSidebarOrder::new(
            InternedId::from_index(u32::MAX),
            valid.strips().clone(),
            [0; 4],
        ),
        SavedSidebarOrder::new(owner, duplicate, [0; 4]),
        SavedSidebarOrder::new(owner, wrong_strip, [0; 4]),
        SavedSidebarOrder::new(owner, unknown, [0; 4]),
        SavedSidebarOrder::new(owner, wrong_kind, [0; 4]),
        SavedSidebarOrder::new(owner, over_capacity, [0; 4]),
    ];
    for order in invalid {
        assert!(matches!(
            prepare(&save(&sim, &rules, Some(&order)), &sim, &rules),
            Err(PreparedLoadError::SidebarOrder(_))
        ));
        assert_eq!(outgoing.saved_order(), before);
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(sim.rng_state(), rng);
    }
}

#[test]
fn pending_requests_survive_the_save_transaction_with_reset_prediction() {
    let (sim, rules, owner, types) = fixture();
    let mut source = retained_history(owner, types);
    source.record_cameo_click(
        &sim,
        &rules,
        owner,
        types[0],
        crate::app::input::sidebar_eva::CameoClick {
            order: Some(crate::app::input::sidebar_eva::CameoOrder::Produce),
            ..Default::default()
        },
    );
    let order = source.saved_order().unwrap();
    assert!(!order.prediction_categories().is_empty());
    let mut requests = order.requests().clone();
    requests[0] = vec![1, 2];
    let order = order.with_requests(requests);
    let hash = sim.state_hash();
    let bytes = save(&sim, &rules, Some(&order));
    let prepared = prepare(&bytes, &sim, &rules).expect("valid retained requests");
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(Simulation::new());
    let committed = prepared.commit_into(&mut runtime);
    let mut restored = SidebarProjectionState::default();
    restored.restore_order(committed.sidebar_order);
    assert_eq!(
        restored.cameo_request(types[0]),
        Some(CameoRequestState::Started)
    );
    assert_eq!(
        restored.cameo_request(types[1]),
        Some(CameoRequestState::Held)
    );
    for type_id in types {
        assert_eq!(
            restored
                .cameo_strips()
                .request_stage(SavedCameoId::Object(type_id)),
            Some(0)
        );
    }
    assert_eq!(restored.saved_order(), Some(order));
    assert_eq!(
        sim.state_hash(),
        hash,
        "presentation transport never writes the running world"
    );
}

#[test]
fn invalid_pending_request_transport_rejects_before_world_commit() {
    let (sim, rules, owner, types) = fixture();
    let outgoing = retained_history(owner, types);
    let before = outgoing.saved_order();
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let invalid = before.clone().unwrap().with_prediction_categories(vec![
        crate::sim::production::ProductionCategory::Infantry,
        crate::sim::production::ProductionCategory::Infantry,
    ]);
    assert!(matches!(
        prepare(&save(&sim, &rules, Some(&invalid)), &sim, &rules),
        Err(PreparedLoadError::SidebarOrder(_))
    ));
    for states in [vec![1], vec![1, 2, 0], vec![1, 3], vec![u8::MAX, 2]] {
        let valid = before.clone().unwrap();
        let mut requests = valid.requests().clone();
        requests[0] = states;
        let invalid = valid.with_requests(requests);
        assert!(matches!(
            prepare(&save(&sim, &rules, Some(&invalid)), &sim, &rules),
            Err(PreparedLoadError::SidebarOrder(_))
        ));
        assert_eq!(outgoing.saved_order(), before);
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(sim.rng_state(), rng);
    }
}
