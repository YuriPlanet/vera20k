//! Shared construction routing against original class rank gates. The native
//! corpus bounds class-tail behavior; it does not certify whole DLL startup.

use super::*;
use crate::sim::house_state::HouseState;
use crate::util::native_x87::NativeF32Bits;

fn country_rules(kind: &str, matched: bool, trainable: bool, naval: bool) -> RuleSet {
    let (registry, key) = match kind {
        "infantry" => ("InfantryTypes", "VeteranInfantry"),
        "unit_ctor" | "unit_init" => ("VehicleTypes", "VeteranUnits"),
        "aircraft_init" => ("AircraftTypes", "VeteranAircraft"),
        _ => panic!("unknown native rank caller {kind}"),
    };
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=SYNTHCOUNTRY\n[SYNTHCOUNTRY]\n{key}={}\n\
         [{registry}]\n0=TARGET\n[TARGET]\nStrength=200\nSpeed=4\nTrainable={}\nNaval={}\n",
        if matched { "TARGET" } else { ",,," },
        if trainable { "yes" } else { "no" },
        if naval { "yes" } else { "no" },
    )))
    .unwrap()
}

fn install_country_house(sim: &mut Simulation, infiltrated: bool) -> InternedId {
    let owner = sim.interner.intern("FIELD_HOUSE");
    let country = sim.interner.intern("SYNTHCOUNTRY");
    let mut house = HouseState::new(owner, 0, Some(country), true, 0, 10);
    if infiltrated {
        house.grant_veteran_infantry();
        house.grant_veteran_units();
    }
    sim.houses.register(house);
    owner
}

#[test]
fn country_and_spy_starting_veterancy_follow_original_class_gates() {
    let corpus = crate::test_fixture::json("tools/rules_oracle/country_veterancy.json");
    let rows = corpus["rank_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 28);
    for row in rows {
        let kind = row["kind"].as_str().unwrap();
        let rules = country_rules(
            kind,
            row["country_match"].as_bool().unwrap(),
            row["trainable"].as_bool().unwrap(),
            row["naval"].as_bool().unwrap(),
        );
        let object = rules.object("TARGET").unwrap();
        let mut sim = Simulation::with_seed(0x51_7CC0);
        let owner = install_country_house(&mut sim, row["infiltrated"].as_bool().unwrap());
        let type_id = sim.interner.intern("TARGET");
        let mut actor = GameEntity::new_at_frame_zero_for_test(
            1,
            6,
            5,
            0,
            0,
            owner,
            Health::new(200),
            type_id,
            EntityCategory::from(object.category),
            0,
            8,
            false,
        );
        actor.veterancy.set_raw_for_test(NativeF32Bits::from_bits(
            row["raw_before"].as_u64().unwrap() as u32,
        ));
        Simulation::install_techno_components(
            &mut sim.interner,
            &sim.session,
            &sim.houses,
            &mut actor,
            Some(object),
            &rules,
            construction::ComponentOrigin::Runtime,
        );
        assert_eq!(
            u64::from(actor.veterancy.raw().bits()),
            row["raw_after"].as_u64().unwrap(),
            "{}",
            row["id"]
        );
        assert_eq!(
            actor.veterancy.rank_cache(),
            crate::sim::combat::veterancy::LEVEL_UNSAMPLED,
            "constructor country/spy setters do not announce a promotion: {}",
            row["id"]
        );
    }
}

#[test]
fn country_starting_rank_reaches_held_construction_and_map_percent_overrides() {
    for (kind, category) in [
        ("infantry", EntityCategory::Infantry),
        ("unit_ctor", EntityCategory::Unit),
        ("aircraft_init", EntityCategory::Aircraft),
    ] {
        // Untrainable membership reaches actual creation too; the original
        // class-tail controls above separately cover the Naval distinction.
        let rules = country_rules(kind, true, false, false);
        let mut held = Simulation::with_seed(0x51_7CC0);
        install_country_house(&mut held, false);
        let id = held
            .construct_object_limbo_at_height(
                "TARGET",
                "FIELD_HOUSE",
                6,
                5,
                0,
                0,
                &rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        assert_eq!(
            held.substrate
                .entities
                .get(id)
                .unwrap()
                .veterancy
                .raw()
                .bits(),
            0x3f80_0000,
            "{kind}"
        );
        for (percent, expected) in [(None, 0x3f80_0000), (Some(0), 0), (Some(200), 0x4000_0000)] {
            let mut map = Simulation::with_seed(0x51_7CC0);
            install_country_house(&mut map, false);
            let mut placement = map_entity("TARGET", category, (6, 5));
            placement.owner = "FIELD_HOUSE".into();
            placement.veterancy = percent;
            assert_eq!(
                map.spawn_from_map(&[placement], Some(&rules)),
                1,
                "{kind}/{percent:?}"
            );
            assert_eq!(
                map.substrate
                    .entities
                    .values()
                    .next()
                    .unwrap()
                    .veterancy
                    .raw()
                    .bits(),
                expected,
                "map percent applies after the country constructor: {kind}/{percent:?}"
            );
        }
    }
}

#[cfg(feature = "ares")]
#[test]
fn ares_country_building_rank_matches_native_init_manager_hook() {
    let corpus = crate::test_fixture::json("tools/rules_oracle/ares_country_veterancy.json");
    for row in corpus["building_creation"].as_array().unwrap() {
        let members = row["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(",");
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=SYNTHCOUNTRY\n[SYNTHCOUNTRY]\nVeteranBuildings={members}\n\
             [BuildingTypes]\n0=MATCH\n1=OTHER\n[MATCH]\nStrength=200\nTrainable={}\n[OTHER]\nStrength=200\n",
            if row["trainable"].as_bool().unwrap() { "yes" } else { "no" },
        ))).unwrap();
        let mut sim = Simulation::with_seed(0x51_7CC0);
        let owner = if row["owner"].as_bool().unwrap() {
            install_country_house(&mut sim, false)
        } else {
            sim.interner.intern("MISSING_HOUSE")
        };
        let type_id = sim.interner.intern("MATCH");
        let mut actor = GameEntity::new_at_frame_zero_for_test(
            1,
            6,
            5,
            0,
            0,
            owner,
            Health::new(200),
            type_id,
            EntityCategory::Structure,
            0,
            8,
            false,
        );
        actor.veterancy.set_raw_for_test(NativeF32Bits::from_bits(
            row["initial_bits"].as_u64().unwrap() as u32,
        ));
        Simulation::install_techno_components(
            &mut sim.interner,
            &sim.session,
            &sim.houses,
            &mut actor,
            rules.object("MATCH"),
            &rules,
            construction::ComponentOrigin::Runtime,
        );
        assert_eq!(
            u64::from(actor.veterancy.raw().bits()),
            row["veterancy_bits"].as_u64().unwrap(),
            "{}",
            row["name"]
        );
        assert_eq!(
            actor.veterancy.rank_cache(),
            crate::sim::combat::veterancy::LEVEL_UNSAMPLED
        );
    }
}
