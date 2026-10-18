//! Original Building4456D0 attempts, joined to the existing admission owner.
//! Native pause/resume produces the supplied online input; the connected app
//! capture separately exercises the Temporal trigger and release.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::rng::trace_draws;
use crate::sim::temporal::TemporalState;
use std::fmt::Write;

/// One Allied yard, a type with a prerequisite, and a type at `BuildLimit=1`.
fn draw_state_world() -> (Simulation, RuleSet, InternedId, u64) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=C0\n[C0]\nSide=GDI\n\
         [BuildingTypes]\n0=Yard\n1=Limit\n2=Req\n3=Needs\n4=Plain\n\
         [Yard]\nStrength=1000\nTechLevel=-1\nFactory=BuildingType\nOwner=C0\n\
         [Limit]\nStrength=100\nTechLevel=1\nBuildLimit=1\nOwner=C0\n\
         [Req]\nStrength=100\nTechLevel=1\nOwner=C0\n\
         [Needs]\nStrength=100\nTechLevel=1\nPrerequisite=Req\nOwner=C0\n\
         [Plain]\nStrength=100\nTechLevel=1\nOwner=C0\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let country = sim.interner.intern("C0");
    let owner = sim.interner.intern("H");
    sim.houses.insert(
        owner,
        HouseState::new(owner, 0, Some(country), true, 1000, 10),
    );
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    let yard = sim.spawn_object("Yard", "H", 8, 8, 0, &rules).unwrap();
    (sim, rules, owner, yard)
}

/// `StripClass::Draw`'s two queries are the drawn result; `Recalculate`'s
/// retention test is a separate pass and a separate answer.
#[test]
fn cameo_draw_state_darkens_where_draw_does_not_decide_retention() {
    let (mut sim, rules, owner, yard) = draw_state_world();
    let needs = rules.object("Needs").unwrap();
    let limit = rules.object("Limit").unwrap();
    let plain = rules.object("Plain").unwrap();

    // A missing `Prerequisite=` is Recalculate's refusal, not Draw's darkening:
    // the entry keeps its brightness until the membership pass drops it.
    assert_eq!(
        strip_refusal(&sim, &rules, owner, needs),
        Some(BuildDisabledReason::CannotBuild)
    );
    assert!(cameo_draw_state(&sim, &rules, owner, needs).enabled);

    assert!(cameo_draw_state(&sim, &rules, owner, limit).enabled);
    assert!(cameo_draw_state(&sim, &rules, owner, plain).enabled);

    // An offline factory keeps the entry and darkens it: Draw's
    // `FindFactory(1, 1, 1)` requires `+660`, `Recalculate`'s does not.
    sim.entities_mut()
        .get_mut(yard)
        .unwrap()
        .set_temporal_for_test(TemporalState::warped_by_for_test(yard));
    assert!(!sim.entities().get(yard).unwrap().building_online());
    assert!(strip_refusal(&sim, &rules, owner, plain).is_none());
    let offline = cameo_draw_state(&sim, &rules, owner, plain);
    assert!(!offline.enabled);
    assert!(!offline.at_build_limit);
    // The same live query clears the overlay when the yard is back online.
    sim.entities_mut()
        .get_mut(yard)
        .unwrap()
        .set_temporal_for_test(TemporalState::default());
    assert!(cameo_draw_state(&sim, &rules, owner, plain).enabled);

    // `BuildLimit=1` reached: still retained, drawn dark, and the press gate
    // reports the limit independently.
    sim.spawn_object("Limit", "H", 20, 8, 0, &rules).unwrap();
    assert!(strip_refusal(&sim, &rules, owner, limit).is_none());
    let at_limit = cameo_draw_state(&sim, &rules, owner, limit);
    assert!(!at_limit.enabled);
    assert!(at_limit.at_build_limit);
    // The ready-factory query asks the limit itself (`CanBuild(type, 1, 1)`),
    // so its null answer comes first; both arms draw the same overlay.
    assert_eq!(
        sidebar_state(&sim, &rules, owner, limit),
        Some(BuildDisabledReason::NoReadyFactory)
    );
}

#[test]
fn house_pass_carries_stored_building_order_before_recalculation() {
    // House4F9280 scans House+6C; startup684C58 instead scans the global
    // constructor array. Returning an earlier building to a house makes the
    // two orders differ and exercises the actual House lifecycle handoff.
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=C0\n[C0]\nSide=GDI\n\
         [BuildingTypes]\n0=InfFactory\n1=BuildFactory\n2=Build\n\
         [InfantryTypes]\n0=Inf\n\
         [InfFactory]\nStrength=1000\nTechLevel=-1\nFactory=InfantryType\nOwner=C0\n\
         [BuildFactory]\nStrength=1000\nTechLevel=-1\nFactory=BuildingType\nOwner=C0\n\
         [Build]\nStrength=100\nTechLevel=1\nOwner=C0\n\
         [Inf]\nStrength=100\nTechLevel=1\nOwner=C0\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let country = sim.interner.intern("C0");
    let owner = sim.interner.intern("H");
    let other = sim.interner.intern("Other");
    for id in [owner, other] {
        sim.houses
            .insert(id, HouseState::new(id, 0, Some(country), true, 1000, 10));
        sim.session.house_order.push(id);
    }
    sim.session.current_house = Some(owner);
    let earlier = sim
        .spawn_object("InfFactory", "H", 8, 8, 0, &rules)
        .unwrap();
    let later = sim
        .spawn_object("BuildFactory", "H", 12, 12, 0, &rules)
        .unwrap();
    for id in [earlier, later] {
        sim.discover_techno(
            id,
            owner,
            crate::sim::world::UninitContext::new(Some(&rules), None),
        );
    }
    sim.change_owner_with_rules(earlier, other, &rules, None, Default::default());
    sim.change_owner_with_rules(earlier, owner, &rules, None, Default::default());
    let inf = sim.interner.get("Inf").unwrap();
    let build = sim.interner.get("Build").unwrap();
    assert_eq!(initial_construction_options(&sim, &rules), [inf, build]);
    assert_eq!(
        construction_options_for_owner(&sim, &rules, owner),
        [build, inf]
    );
    let result = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            None,
            22,
            crate::sim::world::TickLane::Ordinary,
            None,
            Default::default(),
        )
        .unwrap();
    let reached: Vec<_> = result
        .lifecycle_outputs
        .iter()
        .filter_map(|output| match *output {
            crate::sim::world::LifecycleOutput::SidebarConstructionOption {
                owner: reached,
                type_id,
            } if reached == owner => Some(Some(type_id)),
            crate::sim::world::LifecycleOutput::SidebarRecalculated { owner: reached }
                if reached == owner =>
            {
                Some(None)
            }
            _ => None,
        })
        .collect();
    assert_eq!(reached, [Some(build), Some(inf), None]);
}

#[test]
fn construction_options_match_original_gates_type_order_limit_and_warp_grant() {
    let native = crate::test_fixture::json("tools/can_build_construction_options.json");
    assert_eq!(native["rows"].as_array().unwrap().len(), 13);
    let families = [
        ("building", "BuildingTypes"),
        ("unit", "VehicleTypes"),
        ("infantry", "InfantryTypes"),
        ("aircraft", "AircraftTypes"),
    ];
    for row in native["rows"].as_array().unwrap() {
        let mut text = String::from("[Countries]\n0=C0\n[C0]\nSide=GDI\n");
        let factory_kind = match row["factory_rtti"].as_i64().unwrap() {
            7 => "BuildingType",
            40 => "UnitType",
            16 => "InfantryType",
            3 => "AircraftType",
            _ => "None",
        };
        writeln!(text, "[Factory]\nFactory={factory_kind}\nStrength=1000\nTechLevel=-1\nOwner=C0\n[Lab]\nStrength=1000\nTechLevel=-1\nSecretLab=yes").unwrap();
        for (family, section) in families {
            writeln!(text, "[{section}]").unwrap();
            if family == "building" {
                writeln!(text, "900=Factory\n901=Lab").unwrap();
            }
            let Some(catalog) = row["catalogs"][family].as_array() else {
                continue;
            };
            for (index, spec) in catalog.iter().enumerate() {
                writeln!(text, "{index}={}", spec["id"].as_str().unwrap()).unwrap();
            }
            for spec in catalog {
                writeln!(
                    text,
                    "[{}]\nStrength=100\nTechLevel={}\nBuildLimit={}\nNaval={}\nOwner=C0",
                    spec["id"].as_str().unwrap(),
                    spec["tech"].as_i64().unwrap_or(1),
                    spec["limit"].as_i64().unwrap_or(i32::MAX as i64),
                    if spec["naval"].as_bool().unwrap_or(false) {
                        "yes"
                    } else {
                        "no"
                    }
                )
                .unwrap();
            }
        }
        let mut ini = IniFile::from_str("[Clear]\nBuildable=yes\n");
        ini.merge(&IniFile::from_str(&text));
        let rules = RuleSet::from_ini(&ini).unwrap();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let country = sim.interner.intern("C0");
        let owner = sim.interner.intern("H");
        let other = sim.interner.intern("Other");
        for (id, human) in [(owner, true), (other, false)] {
            sim.houses
                .insert(id, HouseState::new(id, 0, Some(country), human, 1000, 10));
            sim.session.house_order.push(id);
        }
        sim.session.current_house = Some(owner);
        let factory_owner = if row["owner"] == "other" {
            "Other"
        } else {
            "H"
        };
        let factory = sim
            .spawn_object("Factory", factory_owner, 8, 8, 0, &rules)
            .unwrap();
        let lab = sim.spawn_object("Lab", "Other", 12, 12, 0, &rules).unwrap();
        for (family, _) in families {
            for spec in row["catalogs"][family].as_array().into_iter().flatten() {
                for n in 0..spec["owned"].as_u64().unwrap_or(0) {
                    sim.spawn_object(
                        spec["id"].as_str().unwrap(),
                        "H",
                        20 + n as u16,
                        20,
                        0,
                        &rules,
                    )
                    .unwrap();
                }
            }
        }
        if row["limbo"].as_bool() == Some(true) {
            sim.techno_limbo_with_rules(factory, &rules, None);
        }
        sim.entities_mut()
            .get_mut(factory)
            .unwrap()
            .discovery
            .set_discovered_by_current_house_for_test(row["discovered"].as_bool().unwrap_or(true));
        for phase in row["results"].as_array().unwrap() {
            sim.entities_mut()
                .get_mut(factory)
                .unwrap()
                .set_temporal_for_test(if phase["online"].as_bool().unwrap() {
                    TemporalState::default()
                } else {
                    TemporalState::warped_by_for_test(factory)
                });
            if let Some(grant) = phase["grant"].as_str() {
                let id = sim.interner.get(grant).unwrap();
                sim.entities_mut()
                    .get_mut(lab)
                    .unwrap()
                    .assign_secret_lab_fallback((ObjectCategory::Infantry, id));
                sim.change_owner_with_rules(lab, owner, &rules, None, Default::default());
            }
            let label = format!("{} / {}", row["name"], phase["name"]);
            for call in phase["can_build"].as_array().unwrap() {
                let object = rules
                    .object(call["type_id"].as_str().unwrap())
                    .unwrap_or_else(|| panic!("{label}: missing {}", call["type_id"]));
                let answer = can_build(&sim, &rules, owner, object, false, true);
                let expected = match call["answer"].as_i64().unwrap() {
                    1 => CanBuild::Yes,
                    -1 => CanBuild::AtLimit,
                    0 => CanBuild::No,
                    value => panic!("unknown native CanBuild {value}"),
                };
                assert_eq!(answer, expected, "{label}: {}", call["type_id"]);
            }
            let mut attempts = Vec::new();
            let (_, draws) =
                trace_draws(|| append_construction_options(&sim, &rules, factory, &mut attempts));
            let actual: Vec<_> = attempts
                .iter()
                .map(|id| sim.interner.resolve(*id))
                .collect();
            let expected: Vec<_> = phase["attempted_additions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["type_id"].as_str().unwrap())
                .collect();
            assert_eq!(actual, expected, "{label}");
            assert!(draws.is_empty(), "{label}: admission spends no RNG");
            assert_eq!(
                row["rng_unchanged"],
                serde_json::json!({"main":true,"mapgen":true,"scenario":true})
            );
        }
    }
}
