//! Four executed Ares histories: tools/rules_oracle/ares_super_anims.json.
//! The native fixture records slot calls; these checks also retain actual Rust
//! AnimClass members. No archive resource or screenshot is an expected value.
use super::*;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use serde_json::Value;

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/rules_oracle/ares_super_anims.json")
}

fn fixture(extra: &str) -> (Simulation, RuleSet, InternedId, u64) {
    let mut text = String::from("[Countries]\n");
    for index in 0..31 {
        text += &format!("{index}=C{index}\n");
    }
    text += "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
        [BuildingTypes]\n0=PROVIDER\n1=SUPPORT\n\
        [PROVIDER]\nStrength=100\nSuperWeapon=SW0\nChargedAnimTime=1\n\
        [SUPPORT]\nStrength=100\n[SuperWeaponTypes]\n0=SW0\n1=SW1\n2=SW2\n";
    for index in 0..3 {
        text += &format!("[SW{index}]\nType=LightningStorm\n");
    }
    text += "[Animations]\n0=SLOT14V0\n1=SLOT15V0\n2=SLOT16V0\n3=SLOT17V0\n";
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(&text);
    ini.merge(&IniFile::from_str(extra));
    let art = IniFile::from_str(
        "[PROVIDER]\nFoundation=1x1\nSuperAnim=SLOT14V0\nSuperAnimTwo=SLOT15V0\nSuperAnimThree=SLOT16V0\nSuperAnimFour=SLOT17V0\n[SUPPORT]\nFoundation=1x1\n[SLOT14V0]\nEnd=1\n[SLOT15V0]\nEnd=1\n[SLOT16V0]\nEnd=1\n[SLOT17V0]\nEnd=1\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    // The native comparison records PlayAnim calls rather than loading SHPs.
    // This Rust scene also allocates real AnimClass members, so provide their
    // asset bounds separately; parsing fixed ART does not perform binding.
    // Processing fixed ART resolves type fields; installing the registry above
    // supplies the runtime animation metadata, as the production loader does.
    for name in ["SLOT14V0", "SLOT15V0", "SLOT16V0", "SLOT17V0"] {
        rules.bind_anim_frame_count_for_test(name, 1);
    }
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("C0");
    sim.houses
        .register(HouseState::new(owner, 0, Some(owner), true, 0, 10));
    construct_house_supers(&mut sim, &rules, owner, false);
    sim.resolved_terrain = Some(test_grid(16, 16, test_flat_cell));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim.session.set_binary_frame_for_test(9000);
    let building = sim
        .spawn_object_at_height("PROVIDER", "C0", 5, 5, 0, 0, &rules)
        .unwrap();
    clear(&mut sim, building);
    (sim, rules, owner, building)
}

fn clear(sim: &mut Simulation, building: u64) {
    for slot in 14..=17 {
        sim.clear_building_anim_slot(building, slot);
    }
}

fn timers(sim: &mut Simulation, owner: InternedId, left: i32) {
    for instance in sim
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .values_mut()
    {
        instance.charge_start_tick = -1;
        instance.charge_duration = left;
    }
}

fn selected(sim: &Simulation, rules: &RuleSet, building: u64) -> Option<usize> {
    let instance =
        first_available_building_super(sim, rules, sim.entities().get(building).unwrap())?;
    let name = sim.interner.resolve(instance.type_id());
    rules.super_weapon_index(name)
}

fn assert_selection(sim: &Simulation, rules: &RuleSet, building: u64, native: &Value) {
    assert_eq!(
        selected(sim, rules, building),
        native["selected"].as_u64().map(|v| v as usize)
    );
}

fn slot_name(sim: &Simulation, building: u64, slot: u8) -> Option<String> {
    let id = sim.entities().get(building)?.building_art.anim_slots()[usize::from(slot)]?;
    Some(sim.interner.resolve(sim.anim(id)?.type_id()).to_string())
}

fn assert_play(sim: &Simulation, building: u64, native: &Value) {
    let play = native["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["action"] == "play")
        .unwrap();
    let slot = play["slot"].as_u64().unwrap() as u8;
    assert_eq!(
        slot_name(sim, building, slot).as_deref(),
        play["name"].as_str()
    );
}

#[test]
fn ares_super_launch_history_rejects_incomplete_restored_pair() {
    use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};

    let (mut sim, rules, owner, _) = fixture("");
    let instance = sim
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    // A same-layout damaged save can contain this pair despite the normal
    // writer always setting both fields. Exercise codec and restore admission.
    instance.ares_launch_count = 2;
    instance.ares_last_launch_frame = None;
    let bytes = GameSnapshot::save(&sim, 0, rules.simulation_config_hash(), "bad-history", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(
        restored.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::InvalidSuperWeaponLaunchHistory(
            "nonzero successful-launch count without a last frame"
        ))
    );
}

#[test]
fn ares_super_anim_queries_and_slot_calls_match_four_native_histories() {
    let native = oracle();
    assert_eq!(native["histories"].as_array().unwrap().len(), 3);
    let a = &native["histories"][0];
    let (mut sim, rules, owner, building) = fixture("");
    assert_selection(&sim, &rules, building, &a["selection"]);
    for phase in ["opening_charging", "opening_ready"] {
        clear(&mut sim, building);
        timers(&mut sim, owner, a[phase]["left"].as_i64().unwrap() as i32);
        sim.open_super_weapon_anims(building, false, false, &rules);
        assert_play(&sim, building, &a[phase]);
    }
    for (phase, occupied) in [("update_near", 14), ("update_far", 16)] {
        clear(&mut sim, building);
        sim.set_building_anim_slot(building, occupied, false, false, 0, &rules);
        timers(&mut sim, owner, a[phase]["left"].as_i64().unwrap() as i32);
        sim.update_super_weapon_anims(building, &rules);
        assert_eq!(slot_name(&sim, building, occupied), None);
        assert_play(&sim, building, &a[phase]);
    }
    // The closed native GetFirst/IsAvailable/timer call chain never reads
    // SuperType+B4: only the definition index and metadata are inputs. The
    // same executed timer rows therefore apply with an unimplemented Type;
    // displaying its body must not synthesize an effect implementation.
    let (mut sim, rules, owner, building) = fixture("[SW0]\nType=FUTURE_HANDLER\n");
    assert_eq!(
        rules.super_weapon("SW0").unwrap().kind,
        crate::rules::superweapon_type::SuperWeaponKind::NoEffect
    );
    assert_selection(&sim, &rules, building, &a["selection"]);
    for phase in ["opening_charging", "opening_ready"] {
        clear(&mut sim, building);
        timers(&mut sim, owner, a[phase]["left"].as_i64().unwrap() as i32);
        sim.open_super_weapon_anims(building, false, false, &rules);
        assert_play(&sim, building, &a[phase]);
    }
    let b = &native["histories"][1];
    let basic = "[PROVIDER]\nSuperWeapon2=SW1\nSuperWeapons=SW2\n[SW0]\nAuxBuilding=SUPPORT\n";
    let (mut sim, rules, owner, building) = fixture(basic);
    assert_selection(&sim, &rules, building, &b["secondary"]);
    timers(&mut sim, owner, 899);
    sim.open_super_weapon_anims(building, false, false, &rules);
    assert_play(&sim, building, &b["secondary_open"]);
    let (_, extra_rules, _, _) = fixture(&format!("{basic}[SW1]\nSW.AllowPlayer=no\n"));
    assert_selection(
        &sim,
        &extra_rules,
        building,
        &b["extra_after_secondary_reader_no"],
    );
    sim.spawn_object_at_height("SUPPORT", "C0", 8, 8, 0, 0, &rules)
        .unwrap();
    assert_selection(
        &sim,
        &extra_rules,
        building,
        &b["primary_recovers_first_in_order"],
    );
    let c = &native["histories"][2];
    let (mut sim, rules, owner, building) = fixture(&format!(
        "{basic}[SW1]\nSW.AllowPlayer=no\n[SW2]\nSW.AllowPlayer=no\n"
    ));
    assert_selection(&sim, &rules, building, &c["selection"]);
    timers(&mut sim, owner, 899);
    sim.open_super_weapon_anims(building, false, false, &rules);
    assert_play(&sim, building, &c["opening"]);
    let retained = slot_name(&sim, building, 14);
    timers(&mut sim, owner, 14);
    sim.update_super_weapon_anims(building, &rules);
    assert_eq!(slot_name(&sim, building, 14), retained);
    let d = native["availability_history"].as_array().unwrap();
    assert_eq!(d.len(), 9);
    // Exact native mask outputs are consumed, not recomputed expected bits.
    for (stage, tags, country, human) in [
        (1, "SW.RequiredHouses=<random>\n", "C0", true),
        (2, "SW.RequiredHouses=<random>\n", "C30", true),
        (
            3,
            "SW.RequiredHouses=<random>\nSW.ForbiddenHouses=<random>\n",
            "C30",
            true,
        ),
        (
            4,
            "SW.RequiredHouses=<random>\nSW.ForbiddenHouses=none\n",
            "C30",
            true,
        ),
        (
            5,
            "SW.RequiredHouses=<random>\nSW.AllowAI=no\n",
            "C30",
            false,
        ),
    ] {
        let (mut sim, rules, owner, building) = fixture(&format!("[SW0]\n{tags}"));
        let country = sim.interner.intern(country);
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_country_for_test(Some(country));
        house.set_is_human_for_test(human);
        house.set_player_control_for_test(human);
        assert_selection(&sim, &rules, building, &d[stage]["selected"]);
        if d[stage].get("opening").is_some() {
            sim.open_super_weapon_anims(building, false, false, &rules);
            assert_play(&sim, building, &d[stage]["opening"]);
        }
    }
}
