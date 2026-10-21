//! Bind the refinery continuation's original reader receipts to the production
//! RULESMD -> optional LANGRULE -> Battle -> XMP03T4 rules owner and fixed ART.
//! Native blocks, widths and supplied Image boundary: refinery_dock.{py,md,json}.
//! This tests selected data and its installed projection. Original cadence/RNG
//! and the supplied movement/world boundaries belong to the continuation replay.

use crate::rules::foundation::foundation_id;
use crate::rules::mission_data::MissionType;
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

/// Stock XMP17T6 overlay identities, reduced by the production harvest owner
/// and carried to NAREFN's production Unload owner. The flat scene and pad/
/// stage placement are explicit test boundaries, not a full map playthrough.
#[test]
fn retail_riparius_and_vinifera_pay_on_separate_unload_gates() {
    use crate::rules::tiberium_type::TiberiumTypeId;
    use crate::sim::mission::{MissionId, MissionType};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP17T6.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(
        &crate::rules::retail_ini_fixture::retail_archive_bytes("XMP17T6.MAP").unwrap(),
    )
    .unwrap();
    let rules = retail.rules;
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let bounds = crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
        map.header.width as i32,
        map.header.local_left as i32,
        map.header.local_top as i32,
        map.header.local_width as i32,
        map.header.local_height as i32,
    );
    // Harvest in reverse slot order: unloading selects native slot order,
    // independently of the order in which the bales entered the hold.
    let sources: Vec<_> = [TiberiumTypeId(2), TiberiumTypeId(0)]
        .into_iter()
        .map(|kind| {
            let ty = rules.tiberium_types.get(kind).unwrap();
            assert_eq!(ty.value, 25);
            let source = map
                .overlays
                .iter()
                .find(|entry| {
                    bounds.contains_geometry_packed(i32::from(entry.rx), i32::from(entry.ry))
                        && overlays
                            .tiberium_type_for_overlay(&rules.tiberium_types, entry.overlay_id)
                            == Some(kind)
                })
                .expect("stock map contains this native type inside LocalSize");
            source.overlay_id
        })
        .collect();
    let mut sim =
        super::refinery_dock_oracle_tests::world_with(&rules, &retail.processed_rules, |_| {});
    let owner = sim.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner,
            0,
            Some(owner),
            true,
            0,
            10,
        ));
    let refinery = sim
        .spawn_object("NAREFN", "Americans", 6, 9, 0, &rules)
        .unwrap();
    let miner = sim
        .spawn_object("HARV", "Americans", 15, 15, 0, &rules)
        .unwrap();
    for overlay in sources {
        // Five ordinary whole-density harvests; keep a level on the cell so
        // this regression isolates cargo identity from ore exhaustion queues.
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(15, 15, overlay, 5);
        sim.overlay_grid.as_mut().unwrap().recalculate_runtime_cell(
            sim.resolved_terrain.as_mut().unwrap(),
            &overlays,
            (15, 15),
        );
        for _ in 0..5 {
            assert!(crate::sim::miner::miner_system::harvest_ore_tick_for_test(
                &mut sim,
                &rules,
                Some(&overlays),
                miner,
            ));
        }
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(miner)
            .unwrap()
            .miner
            .as_ref()
            .unwrap()
            .cargo()
            .len(),
        10
    );
    let source_cargo = sim
        .substrate
        .entities
        .get(miner)
        .unwrap()
        .miner
        .as_ref()
        .unwrap()
        .cargo()
        .to_vec();
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "mixed native cargo", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    // Deserialization alone intentionally leaves cell-list indexes empty.
    // Run the existing production restoration owners before gameplay resumes.
    restored.retain_process_state_for_load(super::SnapshotProcessState::Live(&sim));
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(&rules, &overlays, None)
        .unwrap();
    sim = restored;
    assert_eq!(
        sim.substrate
            .entities
            .get(miner)
            .unwrap()
            .miner
            .as_ref()
            .unwrap()
            .cargo(),
        source_cargo,
        "mixed hold survives save/load"
    );
    sim.resolve_type_handles(&rules);
    let now = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(miner, MissionId::from_known(MissionType::Unload), now)
        .unwrap();
    let entity = sim.substrate.entities.get_mut(miner).unwrap();
    entity.position.set_rx_for_test(9);
    entity.position.set_ry_for_test(10);
    entity.body_facing = crate::sim::movement::FacingClass::new(0x4000, 5);
    entity.radio_contacts.set_slot(0, refinery);
    entity.mission.set_handler_state(3);
    entity
        .miner
        .as_mut()
        .unwrap()
        .set_unload_active_for_test(true);
    sim.substrate
        .entities
        .get_mut(refinery)
        .unwrap()
        .radio_contacts
        .set_slot(0, miner);
    for (credits, remaining, score) in [(125, 5, 25), (250, 0, 50)] {
        sim.substrate
            .entities
            .get_mut(miner)
            .unwrap()
            .install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
                i32::from(rules.general.harvester_dump_frames),
                0,
                crate::sim::timer::CdTimer::from_raw(-1, 0),
                1,
                1,
            ));
        crate::sim::miner::mission_unload(&mut sim, &rules, miner, super::FrameEffects::default());
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            credits,
            "one native type per gate"
        );
        assert_eq!(sim.houses[&owner].economy().score(), score);
        assert_eq!(
            sim.substrate
                .entities
                .get(miner)
                .unwrap()
                .miner
                .as_ref()
                .unwrap()
                .cargo()
                .len(),
            remaining
        );
        assert!(
            sim.substrate
                .entities
                .get(miner)
                .unwrap()
                .miner
                .as_ref()
                .unwrap()
                .cargo()
                .iter()
                .all(|bale| bale.tiberium_type == TiberiumTypeId(2))
        );
    }
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

    // ReadDouble66FC70 narrows through FSTP66FC75. Check both the native stored
    // width and the immutable raw field, without another INI parser.
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
        general.purifier_bonus.bits(),
        purifier.to_bits(),
        "stock native float retained unchanged"
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
