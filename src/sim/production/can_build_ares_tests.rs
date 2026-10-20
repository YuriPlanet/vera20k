//! Ares3.0 HouseOwnsAll/Any10008110/100081E0 controls and live ownership.
//! The corpus compares the prerequisite components; the production checks
//! below cover their connection to VERA's existing CanBuild gate order.

use super::*;
use crate::sim::world::{FrameEffects, UninitContext};

#[test]
fn house_prerequisites_match_executed_controls() {
    let data = crate::test_fixture::json("tools/rules_oracle/ares_prerequisites.json");
    let entry = |code: i64| match code {
        code if code >= 0 => Prerequisite::Building(code as i32),
        -6..=-1 => Prerequisite::Group(PrerequisiteGroup::ALL[(-code - 1) as usize]),
        code => Prerequisite::GenericGroup((-code - 1) as u32),
    };
    for row in data["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row.get("building_counts").is_some())
    {
        let members = row["generic_custom_members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|code| ["A", "B", "COLLIDE"][code.as_u64().unwrap() as usize])
            .collect::<Vec<_>>();
        let alternate = names(&row["alternate_names"]);
        let upgrade = row["powers_up_host"]
            .as_str()
            .map_or_else(String::new, |host| {
                format!("[B]\nPowersUpBuilding={host}\n")
            });
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=C0\n\
             [BuildingTypes]\n0=A\n1=B\n2=COLLIDE\n\
             [VehicleTypes]\n0=TANK\n1=Q\n[InfantryTypes]\n0=INF\n\
             [GenericPrerequisites]\ncustom={}\n\
             [General]\nPrerequisiteCustomAlternate={}\n[Q]\nTechLevel=1\n{upgrade}",
            if members.is_empty() {
                ",,,".to_owned()
            } else {
                members.join(",")
            },
            alternate.join(",")
        )))
        .unwrap();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("H0");
        let country = sim.interner.intern("C0");
        sim.houses
            .register(HouseState::new(owner, 0, Some(country), true, 0, 10));
        for (index, building) in row["installed_buildings"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let id = index as u64 + 1;
            let name = ["A", "B", "COLLIDE"][int(&building["type_index"]) as usize];
            crate::sim::production::tests::spawn_structure(
                &mut sim,
                &rules,
                id,
                "H0",
                name,
                10 + index as u16 * 4,
                10,
            );
            let slots = std::array::from_fn(|slot| {
                building["upgrade_slots"][slot].as_u64().map(|index| {
                    sim.interner
                        .get(["A", "B", "COLLIDE"][index as usize])
                        .unwrap()
                })
            });
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .set_building_upgrades_for_test(
                    building["upgrade_count"].as_i64().unwrap_or(0) as i8,
                    slots,
                );
        }
        let mut tracking = HouseTracking::default();
        for (index, count) in row["building_counts"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let id = sim.interner.get(["A", "B", "COLLIDE"][index]).unwrap();
            tracking.set_active_for_test(EntityCategory::Structure, id, int(count));
        }
        for (id, count) in row["alternate_counts"].as_object().unwrap() {
            let ty = rules.object(id).unwrap();
            tracking.set_owned_for_test(
                EntityCategory::from(ty.category),
                sim.interner.get(id).unwrap(),
                int(count),
            );
        }
        for (id, count) in row["alternate_active_counts"]
            .as_object()
            .into_iter()
            .flatten()
        {
            let ty = rules.object(id).unwrap();
            tracking.set_active_for_test(
                EntityCategory::from(ty.category),
                sim.interner.get(id).unwrap(),
                int(count),
            );
        }
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_tracking_for_test(tracking);
        let entries = row["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|code| entry(code.as_i64().unwrap()))
            .collect::<Vec<_>>();
        let owns = |&entry| prerequisite_on_map(&sim, &rules, &sim.houses[&owner], entry);
        let any = row["name"]
            .as_str()
            .unwrap()
            .split('_')
            .any(|part| part == "any");
        let result = if any {
            entries.iter().any(owns)
        } else {
            entries.iter().all(owns)
        };
        assert_eq!(result, row["result"].as_bool().unwrap(), "{}", row["name"]);
        let mut query = rules.object("Q").unwrap().clone();
        if any {
            query.prerequisite_override = entries;
            sim.set_object_type_tech_level("Q", -1, &rules);
        } else {
            query.prerequisite = entries;
        }
        assert_eq!(
            can_build(&sim, &rules, owner, &query, false, false) == CanBuild::Yes,
            result,
            "{} production",
            row["name"]
        );
    }
}

fn lifecycle_world(query_section: &str) -> (Simulation, RuleSet, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=C1\n\
         [BuildingTypes]\n0=MEMBER\n1=MISSING\n\
         [VehicleTypes]\n0=ALT\n1=Q\n\
         [GenericPrerequisites]\ncustom=MEMBER\n\
         [General]\nPrerequisiteCustomAlternate=ALT\n\
         [MEMBER]\nStrength=100\nCapturable=yes\nNeedsEngineer=yes\n\
         [ALT]\nStrength=100\n\
         [Q]\nStrength=100\n{query_section}\n"
    )))
    .unwrap();
    let (sim, owner) = sim_for(&json!({}), &rules);
    (sim, rules, owner)
}

#[test]
fn generic_member_follows_capture_and_limbo_through_existing_owners() {
    let (mut sim, rules, owner) = lifecycle_world("TechLevel=1\nPrerequisite=custom");
    let other = sim.interner.get("H1").unwrap();
    let product = rules.object("Q").unwrap();
    let member = sim.spawn_object("MEMBER", "H1", 10, 10, 0, &rules).unwrap();
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::No
    );
    sim.change_owner_with_rules(member, owner, &rules, None, FrameEffects::default());
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::Yes
    );
    sim.change_owner_with_rules(member, other, &rules, None, FrameEffects::default());
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::No
    );
    sim.change_owner_with_rules(member, owner, &rules, None, FrameEffects::default());
    sim.techno_limbo_with_rules(member, &rules, None);
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::No
    );
    let type_id = sim.interner.get("MEMBER").unwrap();
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .owned_count(EntityCategory::Structure, type_id),
        1
    );
    sim.uninit_with_rules(member, &rules);
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .owned_count(EntityCategory::Structure, type_id),
        1,
        "UnInit leaves ctor-to-dtor tracking until physical destruction"
    );
    sim.process_pending_delete_with(
        Some(&rules),
        None,
        FrameEffects::default(),
        1,
        sim.session.started(),
    );
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .owned_count(EntityCategory::Structure, type_id),
        0
    );
}

#[test]
fn alternate_follows_owned_limbo_acquisition_and_destruction() {
    let (mut sim, rules, owner) = lifecycle_world("TechLevel=1\nPrerequisite=custom");
    let product = rules.object("Q").unwrap();
    let alternate = sim
        .construct_object_limbo_at_height("ALT", "H1", 0, 0, 0, 0, &rules, FrameEffects::default())
        .unwrap();
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::No
    );
    sim.change_owner_with_rules(alternate, owner, &rules, None, FrameEffects::default());
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::Yes
    );
    let type_id = sim.interner.get("ALT").unwrap();
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .active_count(EntityCategory::Unit, type_id),
        0
    );
    sim.discard_constructed_limbo(alternate, Some(&rules), FrameEffects::default());
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::No
    );
}

#[test]
fn generic_override_precedes_tech_and_prerequisites_then_checks_build_limit() {
    let (mut sim, rules, owner) = lifecycle_world(
        "TechLevel=-1\nPrerequisite=MISSING\nPrerequisiteOverride=custom\nBuildLimit=1",
    );
    let product = rules.object("Q").unwrap();
    let alternate = sim
        .construct_object_limbo_at_height("ALT", "H0", 0, 0, 0, 0, &rules, FrameEffects::default())
        .unwrap();
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::Yes
    );
    sim.construct_object_limbo_at_height("Q", "H0", 0, 0, 0, 0, &rules, FrameEffects::default())
        .unwrap();
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::AtLimit
    );
    sim.discard_constructed_limbo(alternate, Some(&rules), FrameEffects::default());
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, false),
        CanBuild::No
    );
}

#[test]
fn installed_upgrade_direct_and_group_requirements_follow_host_lifecycle() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=C1\n\
         [BuildingTypes]\n0=HOST\n1=UPGRADE\n2=OTHER\n\
         [VehicleTypes]\n0=DIRECT\n1=GROUP\n\
         [GenericPrerequisites]\ninstalled=UPGRADE\n\
         [HOST]\nStrength=100\nUpgrades=3\nCapturable=yes\nNeedsEngineer=yes\n\
         [UPGRADE]\nStrength=100\nPowersUpBuilding=HOST\n\
         [OTHER]\nStrength=100\n\
         [DIRECT]\nTechLevel=1\nPrerequisite=UPGRADE\n\
         [GROUP]\nTechLevel=1\nPrerequisite=installed\n",
    ))
    .unwrap();
    let (mut sim, owner) = sim_for(&json!({}), &rules);
    let other = sim.interner.get("H1").unwrap();
    let host = sim.spawn_object("HOST", "H1", 10, 10, 0, &rules).unwrap();
    sim.spawn_object("OTHER", "H0", 14, 10, 0, &rules).unwrap();
    let admission = |sim: &Simulation, expected| {
        for name in ["DIRECT", "GROUP"] {
            assert_eq!(
                can_build(
                    sim,
                    &rules,
                    owner,
                    rules.object(name).unwrap(),
                    false,
                    false
                ),
                expected,
                "{name}"
            );
        }
    };
    admission(&sim, CanBuild::No);
    let temporary = sim
        .construct_object_limbo_at_height(
            "UPGRADE",
            "H1",
            10,
            10,
            0,
            0,
            &rules,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(sim.try_install_building_upgrade(
        temporary,
        host,
        &rules,
        UninitContext::with_rules(&rules),
    ));
    assert!(
        !sim.entities()
            .get(temporary)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    sim.process_pending_delete_with(
        Some(&rules),
        None,
        FrameEffects::default(),
        1,
        sim.session.started(),
    );
    assert!(sim.entities().get(temporary).is_none());
    admission(&sim, CanBuild::No);
    sim.change_owner_with_rules(host, owner, &rules, None, FrameEffects::default());
    admission(&sim, CanBuild::Yes);
    let upgrade = sim.interner.get("UPGRADE").unwrap();
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .active_count(EntityCategory::Structure, upgrade),
        0
    );
    sim.change_owner_with_rules(host, other, &rules, None, FrameEffects::default());
    admission(&sim, CanBuild::No);
    sim.change_owner_with_rules(host, owner, &rules, None, FrameEffects::default());
    admission(&sim, CanBuild::Yes);
    assert_eq!(sim.power_down_building_upgrade(host, &rules), Some(upgrade));
    admission(&sim, CanBuild::No);
}

#[test]
fn installed_upgrade_scan_does_not_filter_online_health_or_selling() {
    use crate::sim::mission::MissionDispatchTimer;
    use crate::sim::mission::state::MissionTestFixture;

    // Ares1000830F..835F reads the matched host's Type and fixed3 slots;
    // no health, limbo, online, mission or upgrade-count field is read.
    let (mut sim, rules, owner) = upgrade_prerequisite_world();
    let host = sim.substrate.entities.get_mut(1).unwrap();
    host.health.set_for_test(0);
    host.set_building_online_for_test(false);
    let selling = MissionId::from_known(MissionType::Selling);
    host.mission.apply_test_fixture(MissionTestFixture {
        current: selling,
        suspended: MissionId::NONE,
        queued: selling,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    assert_eq!(
        can_build(
            &sim,
            &rules,
            owner,
            rules.object("Q").unwrap(),
            false,
            false
        ),
        CanBuild::Yes,
        "the installed host's health, online and Selling state are unread"
    );
}
