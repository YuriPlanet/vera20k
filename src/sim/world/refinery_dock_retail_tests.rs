//! Bind the refinery continuation's original reader receipts to the production
//! RULESMD -> optional LANGRULE -> Battle -> XMP03T4 rules owner and fixed ART.
//! Native blocks, widths and supplied Image boundary: refinery_dock.{py,md,json}.
//! This tests selected data and its installed projection. Original cadence/RNG
//! and the supplied movement/world boundaries belong to the continuation replay.

use crate::rules::foundation::foundation_id;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::INCOME_PPM_SCALE;
use serde_json::Value;

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed32 receipt")).unwrap()
}

/// Decode saved native memory bytes, not INI text. Production readers remain
/// the sole parser of physical/processed retail values.
fn bytes<const N: usize>(value: &Value) -> [u8; N] {
    let encoded = value.as_str().expect("native memory hex");
    assert_eq!(encoded.len(), N * 2);
    std::array::from_fn(|i| u8::from_str_radix(&encoded[i * 2..i * 2 + 2], 16).unwrap())
}

fn double(value: &Value) -> f64 {
    f64::from_le_bytes(bytes(value))
}

/// The rules owner preconverts the native positive minute field into a frame
/// count. Check that representation against the saved native field's interval;
/// do not implement another ftol or turn a hand-computed cadence into a golden.
fn compare_frame_representation(actual: i32, native_minutes: f64, context: &str) {
    let native_frames = native_minutes * 900.0;
    assert!(
        native_frames >= 0.0,
        "{context}: bounded stock positive rate"
    );
    assert!(
        f64::from(actual) <= native_frames && native_frames < f64::from(actual) + 1.0,
        "{context}: {actual} frames does not represent native {native_minutes:?} minutes"
    );
}

#[test]
fn retail_docking_inputs_match_original_readers_and_installed_art() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        // Existing fixture requires assets when VERA20K_REQUIRE_RETAIL_INI=1;
        // asset-free CI retains the repository's usual optional retail gate.
        return;
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/refinery_dock.json",
    ))
    .unwrap();
    let input = &corpus["retail_inputs"];
    let stock = &input["after"];
    let harv_receipt = &input["harv"]["after"];
    let harv = retail.rules.object("HARV").expect("stock HARV");

    assert_eq!(harv.strength, int(&stock["strength"]), "HARV Strength");
    assert_eq!(harv.storage, int(&harv_receipt["storage"]), "HARV Storage");
    assert_eq!(harv.harvester, int(&harv_receipt["harvester"]) != 0);
    assert_eq!(harv.weeder, int(&harv_receipt["weeder"]) != 0);
    assert_eq!(
        harv.movement_zone as i32,
        int(&harv_receipt["movement_zone"])
    );
    let native_dock: Vec<_> = harv_receipt["dock"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(harv.dock, native_dock, "native ordered HARV Dock names");

    let general = &retail.rules.general;
    assert_eq!(general.harvester_load_rate, int(&harv_receipt["load_rate"]));
    assert_eq!(
        general.tiberium_short_scan,
        int(&harv_receipt["short_scan"])
    );
    assert_eq!(general.tiberium_long_scan, int(&harv_receipt["long_scan"]));
    assert_eq!(
        general.harvester_too_far_distance,
        int(&stock["too_far"][0])
    );
    assert_eq!(
        general.chrono_harv_too_far_distance,
        int(&stock["too_far"][1])
    );

    // ReadDouble670CD4 retains constructor/prior-pass binary64. The typed
    // owner stores the integer dump gate; its expected gate comes from the
    // saved original dispatch that first admits payment, not ceil duplicated
    // here. The companion stage14 native row refuses the same deposit.
    let section = retail.processed_rules.section("General").unwrap();
    let dump_rate = section.read_double(
        "HarvesterDumpRate",
        double(&input["constructor"]["dump_rate_bits"]),
    );
    assert_eq!(
        dump_rate.to_bits(),
        double(&stock["dump_rate_bits"]).to_bits()
    );
    let dump_gate = corpus["mission_unload"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "unload_gate_ore")
        .expect("original admitted stock dump gate");
    assert_eq!(
        i32::from(general.harvester_dump_frames),
        int(&dump_gate["input"]["stage"][0]),
        "native dump gate -> immutable frame threshold"
    );

    // ReadDouble66FC6B narrows through FSTP float. Check both the native stored
    // width and the immutable ppm representation, without another INI parser.
    let purifier = f32::from_le_bytes(bytes(&stock["purifier_bonus_bits"]));
    let constructor_purifier =
        f32::from_le_bytes(bytes(&input["constructor"]["purifier_bonus_bits"]));
    assert_eq!(
        section
            .read_float("PurifierBonus", constructor_purifier)
            .to_bits(),
        purifier.to_bits()
    );
    assert_eq!(
        general.purifier_bonus_ppm as f64 / INCOME_PPM_SCALE as f64,
        f64::from(purifier),
        "stock native float -> ppm representation"
    );

    for (name, native) in stock["buildings"].as_object().unwrap() {
        let object = retail.rules.object(name).expect("native stock dock type");
        assert_eq!(
            object.strength,
            int(&native["strength"]),
            "{name}: Strength"
        );
        assert_eq!(
            object.dock_unload,
            int(&native["dock_unload"]) != 0,
            "{name}: DockUnload"
        );
        assert_eq!(
            object.refinery,
            int(&native["refinery"]) != 0,
            "{name}: Refinery"
        );
        assert_eq!(
            object.unit_repair,
            int(&native["unit_repair"]) != 0,
            "{name}: UnitRepair"
        );
        assert_eq!(
            object.number_of_docks,
            int(&native["docks"]),
            "{name}: NumberOfDocks"
        );
        assert_eq!(
            i32::from(foundation_id(&object.foundation)),
            int(&native["foundation"]),
            "{name}: processed Building Foundation"
        );
        let native_queue = [
            int(&native["queueing_cell"][0]),
            int(&native["queueing_cell"][1]),
        ];
        assert_eq!(
            object.queueing_cell, native_queue,
            "{name}: installed QueueingCell"
        );
        // The native ART control supplies Image==typeID. Assert production's
        // selected stock identity before comparing its fixed registry entry.
        assert_eq!(
            object.image, *name,
            "{name}: native supplied Image boundary"
        );
        let art = retail
            .rules
            .art()
            .get(&object.image)
            .expect("installed stock ART entry");
        assert_eq!(
            art.foundation.map(i32::from),
            Some(int(&native["foundation"])),
            "{name}: ART Foundation"
        );
        assert_eq!(
            art.queueing_cell, native_queue,
            "{name}: fixed ART QueueingCell"
        );
    }

    // The independent original46499D..464A47 ART loop reads NADEPT's
    // DockingOffset0 over its original constructor's one zeroed slot.
    // Its stock pad differs from the historical continuation's supplied
    // NULL-items/page0 input; do not project that sparse input into retail.
    let native_pads = &input["art"]["docking_offsets"]["NADEPT"];
    let native_offset = &native_pads["offsets"][0];
    let expected_offset = (
        int(&native_offset[0]),
        int(&native_offset[1]),
        int(&native_offset[2]),
    );
    let depot = retail.rules.object("NADEPT").expect("stock NADEPT");
    let art_depot = retail.rules.art().get(&depot.image).unwrap();
    assert_eq!(
        art_depot.pads.len(),
        native_pads["offsets"].as_array().unwrap().len(),
        "NADEPT: fixed ART DockingOffset count"
    );
    assert_eq!(
        art_depot.pads[0].lepton_offset, expected_offset,
        "NADEPT: original ART DockingOffset0"
    );
    assert_eq!(
        depot.pads.len(),
        int(&native_pads["native_number_of_docks"]) as usize,
        "NADEPT: installed NumberOfDocks pad projection"
    );
    assert_eq!(
        depot.pads[0].lepton_offset, expected_offset,
        "NADEPT: installed stock DockingOffset0"
    );

    for (name, native_value) in stock["tiberium_values"].as_object().unwrap() {
        let id = retail
            .rules
            .tiberium_types
            .id_by_name(name)
            .expect("native Tiberium type");
        assert_eq!(
            retail.rules.tiberium_types.get(id).unwrap().value,
            int(native_value),
            "{name}: original721B0C Value store"
        );
    }

    // All selected controls, including shared idle/Stop consumers. Six bytes
    // are native flags at+4..+9; the double fields are Rate+10/AARate+18.
    for (name, native) in harv_receipt["mission_controls"].as_object().unwrap() {
        let record: [u8; 32] = bytes(native);
        let mission = MissionType::from_id(record[0]).expect("native mission selector");
        let entry = retail.rules.mission_control.entry(mission).unwrap();
        assert_eq!(mission.ini_section().to_ascii_lowercase(), *name);
        assert_eq!(entry.no_threat, record[4] != 0, "{name}: NoThreat");
        assert_eq!(entry.zombie, record[5] != 0, "{name}: Zombie");
        assert_eq!(entry.recruitable, record[6] != 0, "{name}: Recruitable");
        assert_eq!(entry.paralyzed, record[7] != 0, "{name}: Paralyzed");
        assert_eq!(entry.retaliate, record[8] != 0, "{name}: Retaliate");
        assert_eq!(entry.scatter, record[9] != 0, "{name}: Scatter");
        let native_rate = f64::from_le_bytes(record[0x10..0x18].try_into().unwrap());
        let native_aa = f64::from_le_bytes(record[0x18..0x20].try_into().unwrap());
        let constructed: [u8; 32] = bytes(&input["harv"]["constructor"]["mission_controls"][name]);
        let constructed_rate = f64::from_le_bytes(constructed[0x10..0x18].try_into().unwrap());
        let section = retail.processed_rules.section(mission.ini_section());
        let rate = section.map_or(constructed_rate, |section| {
            section.read_double("Rate", constructed_rate)
        });
        assert_eq!(
            rate.to_bits(),
            native_rate.to_bits(),
            "{name}: native Rate bits"
        );
        // Original5B38DD reads AARate over0 and copies Rate when zero. Using
        // already observed Rate as the absent-key fallback expresses that
        // final-field relationship; an explicit zero has the same fallback.
        let aa = match section.map_or(rate, |section| section.read_double("AARate", rate)) {
            0.0 => rate,
            value => value,
        };
        assert_eq!(
            aa.to_bits(),
            native_aa.to_bits(),
            "{name}: native AARate bits"
        );
        compare_frame_representation(entry.rate_frames, native_rate, &format!("{name}: Rate"));
        compare_frame_representation(entry.aa_rate_frames, native_aa, &format!("{name}: AARate"));
    }
}
