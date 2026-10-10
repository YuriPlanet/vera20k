//! Original Country/Side reader and retained-registry comparisons.
//!
//! tools/input_oracle/campaign_start.py --houses executes the original
//! 5117D0/4767C0/672440/4756F0/511850 kernels. The corpus and meta bound
//! these histories to those kernels, rather than complete Rules Process.
//! TriggerType727240 controls reach its owner write7272CB for valid indexes;
//! negative indexes stop before the pointer access7272BB.

use super::*;
use crate::rules::ruleset::{CountryIdx, RuleSet, SideIdx};
use serde_json::{Value, json};

fn native_controls() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/campaign_start_houses.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native["side_controls"].clone()
}

fn ordered_ini(sections: &Value) -> IniFile {
    // JSON objects are sorted by serde_json's default map. These ordered
    // arrays record the actual native cache inputs, including stored-empty
    // values and raw >24-byte Side keys, without changing their order.
    IniFile::from_sections_for_test(sections.as_array().unwrap().iter().map(|input| {
        let mut section = IniSection::new(input["name"].as_str().unwrap().to_owned());
        for entry in input["entries"].as_array().unwrap() {
            section.set(entry[0].as_str().unwrap(), entry[1].as_str().unwrap());
        }
        section
    }))
}

fn reader_ini(row: &Value) -> IniFile {
    row["physical_ini"]
        .as_str()
        .map_or_else(|| ordered_ini(&row["ordered_sections"]), IniFile::from_str)
}

fn assert_registry(
    families: &HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    expected: &Value,
    context: &str,
) {
    let actual_countries: Vec<_> = families
        .get(&RulesTypeFamily::Country)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, member)| {
            json!({
                "index": index,
                "id": member.native_stored_id,
                "name": member.body.read_name("Name", 0x31)
                    .unwrap_or(&member.native_stored_id),
                "color": member.country_color_scheme,
                "side": member.country_side_index,
            })
        })
        .collect();
    let expected_countries: Vec<_> = expected["countries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| {
            json!({
                "index": member["index"],
                "id": member["id"],
                "name": member["name"],
                "color": member["color"],
                "side": member["side"],
            })
        })
        .collect();
    assert_eq!(actual_countries, expected_countries, "{context}: Countries");
    let actual_sides: Vec<_> = families
        .get(&RulesTypeFamily::Side)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, member)| {
            json!({
                "index": index,
                "id": member.native_stored_id,
                "members": member.side_country_members,
            })
        })
        .collect();
    let expected_sides: Vec<_> = expected["sides"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| {
            json!({
                "index": member["index"],
                "id": member["id"],
                "members": member["members"],
            })
        })
        .collect();
    assert_eq!(
        actual_sides, expected_sides,
        "{context}: ordered Side vectors"
    );
}

fn assert_constructors(events: &[NativeTypeConstructionEvent], row: &Value, context: &str) {
    let expected: Vec<_> = row["native_calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|call| {
            let (family, registry) = match call["family"].as_str()? {
                "Country" => (NativeTypeConstructorFamily::HouseType, "countries"),
                "Side" => (NativeTypeConstructorFamily::Side, "sides"),
                other => panic!("unexpected native constructor family {other}"),
            };
            // Compare the observed stored ID on the constructed native object,
            // including duplicate stored IDs from distinct raw long keys.
            let native_id = call["id_before"].as_u64().unwrap() + 1;
            let stored_id = row["after"][registry]
                .as_array()
                .unwrap()
                .iter()
                .find(|member| member["native_id"].as_u64() == Some(native_id))
                .unwrap()["id"]
                .as_str()
                .unwrap();
            Some((family, stored_id))
        })
        .collect();
    assert_eq!(
        events
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        expected,
        "{context}: original constructor order/stored identities"
    );
    let id_delta =
        row["after"]["id_cursor"].as_u64().unwrap() - row["before"]["id_cursor"].as_u64().unwrap();
    assert_eq!(
        events.len() as u64,
        id_delta,
        "{context}: bounded native ID delta"
    );
}

fn setup_processor(controls: &Value, row: &Value) -> RulesPassProcessor {
    let mut processor = RulesPassProcessor::default();
    let art = IniFile::empty();
    processor
        .apply_pass(&ordered_ini(&controls["ordered_palette_sections"]), &art)
        .unwrap();
    processor
        .apply_pass(&ordered_ini(&row["ordered_setup_sections"]), &art)
        .unwrap();
    assert_registry(&processor.families, &row["before"], "direct reader setup");
    assert_eq!(
        processor.native_type_construction_events.len() as u64,
        row["before"]["id_cursor"].as_u64().unwrap() - 1_000_000,
        "the supplied empty initial registries reached the observed constructor count"
    );
    processor
}

#[test]
fn ordered_country_side_histories_match_original_and_survive_process_handoff() {
    let controls = native_controls();
    let histories = controls["histories"].as_array().unwrap();
    assert_eq!(histories.len(), 6);
    assert_eq!(
        histories
            .iter()
            .map(|history| history["passes"].as_array().unwrap().len())
            .sum::<usize>(),
        19
    );
    for history in histories {
        let palette = RulesLayerStack::new(ordered_ini(&controls["ordered_palette_sections"]))
            .process()
            .unwrap();
        assert_eq!(palette.native_type_construction_trace().event_count(), 0);
        let mut registry = palette
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events();
        assert_registry(
            &registry.families,
            &history["initial"],
            "empty initial registries",
        );
        for (index, row) in history["passes"].as_array().unwrap().iter().enumerate() {
            let context = format!("{} pass {index}", history["name"].as_str().unwrap());
            assert_registry(&registry.families, &row["before"], &context);
            let processed = RulesLayerStack::new(ordered_ini(&row["ordered_sections"]))
                .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
                .unwrap();
            let trace = processed.native_type_construction_trace();
            assert_registry(&trace.registry_state().families, &row["after"], &context);
            assert_constructors(trace.events(), row, &context);

            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            for country in row["after"]["countries"].as_array().unwrap() {
                let id = country["id"].as_str().unwrap();
                let index = country["index"].as_u64().unwrap();
                assert_eq!(rules.country_name(CountryIdx(index as u16)), Some(id));
                assert_eq!(
                    rules.country_color_scheme(id),
                    country["color"].as_i64().unwrap() as i32
                );
                let side = country["side"].as_i64().unwrap();
                assert_eq!(
                    rules.country_side_index(id),
                    (side >= 0).then(|| SideIdx(side as u8)),
                    "{context}: projected Country binding"
                );
            }
            for side in row["after"]["sides"].as_array().unwrap() {
                assert_eq!(
                    rules.side_name(SideIdx(side["index"].as_u64().unwrap() as u8)),
                    side["id"].as_str(),
                    "{context}: ordered Side projection"
                );
            }
            registry = processed
                .into_ini_and_native_type_construction_trace()
                .1
                .into_registry_state_discarding_events();
        }
    }
}

#[test]
fn ordered_country_side_name_lookup_matches_original_signed_controls() {
    let controls = native_controls();
    let rows = controls["lookup_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 22);
    for row in rows {
        let processor = setup_processor(&controls, row);
        let token = row["token"].as_str().unwrap();
        let expected = row["result"].as_i64().unwrap() as i32;
        assert_eq!(processor.country_index_of_name(token), expected, "{row}");
        let (_, trace, _, _, _, _) = processor.finish();
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.scenario_country_name(Some(token)),
            (expected >= 0).then(|| rules.country_name(CountryIdx(expected as u16)).unwrap()),
            "{row}: registered Scenario identity projection; factory routes are not admitted"
        );
    }
}

#[test]
fn ordered_country_side_trigger_owner_matches_original_caller_prefix() {
    let controls = native_controls();
    let rows = controls["trigger_type_reader_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 25);
    let mut outcomes = [0; 3];
    for row in rows {
        let context = row["name"].as_str().unwrap();
        let palette = RulesLayerStack::new(ordered_ini(&controls["ordered_palette_sections"]))
            .process()
            .unwrap();
        let mut registry = palette
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events();
        for pass in row["setup_passes"].as_array().unwrap() {
            assert_registry(&registry.families, &pass["before"], context);
            let processed = RulesLayerStack::new(ordered_ini(&pass["ordered_sections"]))
                .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
                .unwrap();
            let trace = processed.native_type_construction_trace();
            assert_registry(&trace.registry_state().families, &pass["after"], context);
            assert_constructors(trace.events(), pass, context);
            registry = processed
                .into_ini_and_native_type_construction_trace()
                .1
                .into_registry_state_discarding_events();
        }
        assert_registry(&registry.families, &row["before"], context);
        let processed = RulesLayerStack::new(IniFile::empty())
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();

        // Original ReadString512 and strtok select the first owner token.
        // 727292..7272AA handles literal <none> before5117D0.
        let input = reader_ini(row);
        let token = input
            .section_or_empty("Triggers")
            .read_list(row["trigger_id"].as_str().unwrap(), 0x200)
            .and_then(|tokens| tokens.first().copied());
        assert_eq!(
            token,
            row["first_token"].as_str(),
            "{context}: copied first token"
        );
        let Some(token) = token else {
            // Count0 never invokes the identity accessor. The native fixture
            // observes prior +A4 retention; this projection comparison covers
            // the omitted query, rather than a Rust Trigger pointer lifecycle.
            assert_eq!(row["outcome"], "missing_entry");
            outcomes[0] += 1;
            continue;
        };
        let expected = row["resolved_index"].as_u64().map(|index| {
            CountryIdx(u16::try_from(index).expect("native registered Country index"))
        });
        assert_eq!(
            rules.trigger_house_type_index(token),
            expected,
            "{context}: caller owner binding"
        );
        if row["binding_executed"].as_bool().unwrap() {
            assert_eq!(row["outcome"], "bound_country");
            outcomes[1] += 1;
        } else {
            // The original stopped before Country[-1/-2] pointer access.
            // None is the existing Rust registered-only admission policy,
            // not an assertion that native safely completed this binding.
            assert_eq!(row["outcome"], "excluded_negative_index");
            assert!(matches!(row["lookup_result"].as_i64(), Some(-1 | -2)));
            outcomes[2] += 1;
        }
    }
    assert_eq!(outcomes, [6, 13, 6]);
}

#[test]
fn ordered_country_side_list_reader_matches_original_signed_and_default_controls() {
    let controls = native_controls();
    let rows = controls["list_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 22);
    for row in rows {
        let processor = setup_processor(&controls, row);
        let input = reader_ini(row);
        let default: Vec<_> = row["default"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| index.as_i64().unwrap() as i32)
            .collect();
        let actual =
            processor.read_side_members(input.section_or_empty("Control"), "Members", &default);
        assert_eq!(json!(actual), row["result"], "{row}");
        assert_registry(
            &processor.families,
            &row["before"],
            "lookup-only list leaves registries intact",
        );
    }
}

#[test]
fn ordered_country_side_scalar_reader_matches_original_factory_controls() {
    let controls = native_controls();
    let rows = controls["side_reader_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    for row in rows {
        let mut processor = setup_processor(&controls, row);
        let constructor_start = processor.native_type_construction_events.len();
        let input = reader_ini(row);
        let actual = processor.read_side_index(
            input.section_or_empty("Control"),
            "Value",
            row["current"].as_i64().unwrap() as i32,
        );
        assert_eq!(json!(actual), row["result"], "{row}");
        assert_registry(
            &processor.families,
            &row["after"],
            row["name"].as_str().unwrap(),
        );
        assert_constructors(
            &processor.native_type_construction_events[constructor_start..],
            row,
            row["name"].as_str().unwrap(),
        );
    }
}
