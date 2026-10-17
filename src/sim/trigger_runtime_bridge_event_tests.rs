//! ALL02's authored Bridge1_Broke chain through the live Event31 ingress.
//! Bridge575EE0 raises Tag6E53A0(31) on the retained CellTag; HasOccurred
//! admits it at 71EC9A/71F069 and the four authored actions run in order.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;

#[test]
fn retail_all02_bridge_collapse_event_starts_the_repair_chain_once() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = MapFile::from_bytes(assets.get_ref("ALL02UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("ALL02UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL02UMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        Some(&rules),
        Some(&map.ini),
    );
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);

    // Bridge1_Broke (Event31) and the two triggers it enables, with their
    // Tags. The authored CellTag 155112 is moved to arena cell (12,15): an
    // explicit geometry prior, not ALL02's terrain or bridge.
    let broke = "0E69E72C";
    let chain = [broke, "0BC61C6C", "0D10FEDC"];
    let mut body = String::new();
    for section in ["Triggers", "Events", "Actions", "Tags"] {
        body.push_str(&format!("[{section}]\n"));
        for (key, value) in map.ini.section(section).unwrap().raw_entries() {
            let selected = if section == "Tags" {
                value
                    .split(',')
                    .nth(2)
                    .is_some_and(|id| chain.contains(&id))
            } else {
                chain.contains(&key)
            };
            if selected {
                body.push_str(&format!("{key}={value}\n"));
            }
        }
    }
    let cell_tags = map.ini.section("CellTags").unwrap();
    assert_eq!(cell_tags.get_for_test("155112"), Some("0E69E38C"));
    body.push_str("[CellTags]\n15012=0E69E38C\n");
    super::tests::install_event_ingress_map(&mut sim, &rules, &body);
    // [VariableNames] 9=Bridge1_Fixed,1 supplies the authored initial value.
    sim.set_trigger_variable(false, 9, true);
    assert!(sim.trigger_runtime.locals_set.contains(&9));

    let live = |sim: &Simulation, id: &str| {
        sim.trigger_runtime
            .triggers
            .iter()
            .position(|trigger| {
                trigger.as_ref().is_some_and(|trigger| {
                    sim.trigger_runtime.definitions.trigger_types[trigger.type_index].id == id
                })
            })
            .unwrap()
    };
    // Bridge1_Fixed's Tag rides ALL02's CABHUT, which this crop omits, so
    // only Broke and the Logic-registered Fixer_Team have live instances.
    let enabled = |sim: &Simulation| {
        [broke, chain[2]].map(|id| sim.trigger_runtime.trigger(live(sim, id)).unwrap().enabled)
    };
    assert_eq!(enabled(&sim), [true, false]);

    // An ordinary Logic raise (no Event31) cannot satisfy the predicate.
    let identity = sim.trigger_cell_identity((12, 15));
    let tag = sim.trigger_runtime.cell_tags[&identity];
    let mut effects = Vec::new();
    sim.dispatch_live_tag_event(
        tag,
        TagEventInput {
            kind: EVENT_ANY,
            actor: None,
            source: None,
            cell: (0, 0),
            force: false,
        },
        &mut effects,
        Some(&rules),
        None,
        FrameEffects::default(),
    );
    assert_eq!(enabled(&sim), [true, false]);
    assert!(sim.trigger_runtime.locals_set.contains(&9));

    // 57,0,9 LocalClear; 53 Enable Bridge1_Fixed and Bridge1_Fixer_Team;
    // 54 Disable itself. Its repeating Tag stays attached to the cell.
    sim.dispatch_bridge_cell_tag_event(identity, &rules, None, FrameEffects::default());
    assert!(!sim.trigger_runtime.locals_set.contains(&9));
    assert_eq!(enabled(&sim), [false, true]);
    assert_eq!(sim.trigger_runtime.cell_tags.get(&identity), Some(&tag));

    // The disabled Broke trigger ignores a second collapse notification.
    sim.set_trigger_variable(false, 9, true);
    sim.dispatch_bridge_cell_tag_event(identity, &rules, None, FrameEffects::default());
    assert!(sim.trigger_runtime.locals_set.contains(&9));
    assert_eq!(enabled(&sim), [false, true]);
}
