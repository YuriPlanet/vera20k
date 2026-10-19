//! Engineer candidate arm6F860C..8667. Stock scanners carry DefuseKit and
//! VirtualScanner. AreaGuard acquisition and Hunt's explicit Capture
//! transition4D53BA..53F4/4D548F..54B9 are distinct production paths.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;

fn scene() -> Option<(
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    u64,
    u64,
)> {
    scene_with("ENGINEER", "", "Americans")
}

fn scene_with(
    engineer_type: &str,
    patch: &str,
    building_owner: &str,
) -> Option<(
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    u64,
    u64,
)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str(patch),
    )?;
    let mut rules = retail.rules;
    let (mut sim, _, registry) = crate::sim::world::entry_test_fixture::fixture();
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::map::resolved_terrain::scheduler_anim_roots(&rules, &registry, &[]);
    rules
        .bind_scheduler_anim_assets(&roots, &assets, theater.extension, "TEMPERATE")
        .unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for name in ["Americans", building_owner] {
        let owner = sim.intern(name);
        sim.houses
            .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, false, 0, 10));
    }
    let object = rules.object(engineer_type).unwrap();
    assert!(object.engineer);
    assert_eq!(object.primary(), Some("DefuseKit"));
    assert_eq!(object.secondary(), Some("VirtualScanner"));
    let building = sim
        .spawn_object("GAPOWR", building_owner, 17, 15, 0, &rules)
        .unwrap();
    let engineer = sim
        .spawn_object(engineer_type, "Americans", 14, 15, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(1);
    let frame = sim.session.binary_frame();
    sim.mission_assign_exact(
        engineer,
        MissionId::from_known(MissionType::AreaGuard),
        frame,
    )
    .unwrap();
    let actor = sim.substrate.entities.get(engineer).unwrap();
    assert!(is_armed(actor, object));
    Some((sim, rules, registry, engineer, building))
}

#[test]
fn retail_ai_engineer_takes_a_critically_damaged_own_building() {
    let Some((mut sim, rules, registry, engineer, building)) = scene() else {
        return;
    };
    let pick = sim.greatest_threat_represented(
        &rules,
        Some(&registry),
        engineer,
        super::super::ScanMission::AreaGuard,
        None,
    );
    assert_eq!(pick, Some(building));
}

#[test]
fn retail_ai_engineer_refuses_a_friendly_building_above_condition_red() {
    let Some((mut sim, rules, registry, engineer, building)) = scene() else {
        return;
    };
    let strength = rules.object("GAPOWR").unwrap().strength;
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(strength / 2);
    assert!(rules.general.condition_red < 0.5);
    let pick = sim.greatest_threat_represented(
        &rules,
        Some(&registry),
        engineer,
        super::super::ScanMission::AreaGuard,
        None,
    );
    assert_eq!(pick, None);
}

#[test]
fn all_retail_engineers_obey_the_condition_red_boundary() {
    for engineer_type in ["ENGINEER", "SENGINEER", "YENGINEER"] {
        let Some((mut sim, rules, registry, engineer, building)) =
            scene_with(engineer_type, "[GAPOWR]\nStrength=400", "Americans")
        else {
            return;
        };
        assert_eq!(rules.general.condition_red, 0.25);
        for health in [99, 100, 101, 200, 400] {
            sim.substrate
                .entities
                .get_mut(building)
                .unwrap()
                .health
                .set_for_test(health);
            assert_eq!(
                sim.greatest_threat_represented(
                    &rules,
                    Some(&registry),
                    engineer,
                    super::super::ScanMission::AreaGuard,
                    None,
                ),
                (health <= 100).then_some(building),
                "{engineer_type}: health {health}"
            );
        }
    }
}

#[test]
fn engineer_allied_cost_gate_is_nonzero_not_positive() {
    // Cell walk6F8B3C tests >0, but its exhausted list can still offer the
    // held object. Evaluate_Candidate6F8667 then tests !=0.
    for cost in [0, -1, 800] {
        let Some((mut sim, rules, registry, engineer, building)) =
            scene_with("ENGINEER", &format!("[GAPOWR]\nCost={cost}"), "Americans")
        else {
            return;
        };
        let candidate = sim.entities().get(building).unwrap();
        assert_eq!(
            sim.cost_of(candidate.owner(), rules.object("GAPOWR").unwrap(), &rules),
            cost
        );
        assert_eq!(
            sim.greatest_threat_represented(
                &rules,
                Some(&registry),
                engineer,
                super::super::ScanMission::AreaGuard,
                None,
            ),
            (cost != 0).then_some(building),
            "cost {cost}"
        );
    }
}

#[test]
fn enemy_buildings_bypass_engineer_allied_health_and_cost_gates() {
    let Some((mut sim, rules, registry, engineer, building)) =
        scene_with("ENGINEER", "[GAPOWR]\nCost=0", "Russians")
    else {
        return;
    };
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(rules.object("GAPOWR").unwrap().strength);
    assert_eq!(
        sim.greatest_threat_represented(
            &rules,
            Some(&registry),
            engineer,
            super::super::ScanMission::AreaGuard,
            None,
        ),
        Some(building)
    );
}

#[test]
fn engineer_gate_applies_to_other_allied_houses_too() {
    let Some((mut sim, rules, registry, engineer, building)) =
        scene_with("ENGINEER", "[GAPOWR]\nStrength=400", "French")
    else {
        return;
    };
    sim.house_alliances
        .entry("AMERICANS".into())
        .or_default()
        .insert("FRENCH".into());
    for health in [100, 101] {
        sim.substrate
            .entities
            .get_mut(building)
            .unwrap()
            .health
            .set_for_test(health);
        assert_eq!(
            sim.greatest_threat_represented(
                &rules,
                Some(&registry),
                engineer,
                super::super::ScanMission::AreaGuard,
                None,
            ),
            (health <= 100).then_some(building)
        );
    }
}

#[test]
fn retail_ai_engineer_hunt_acquires_enters_repairs_and_is_consumed() {
    let Some((mut sim, rules, registry, engineer, building)) = scene() else {
        return;
    };
    let path = sim.path_grid.clone().unwrap();
    let full = rules.object("GAPOWR").unwrap().strength;
    // Original Hunt4D53BA..53F4/4D548F..54B9 assigns the building as destination and
    // queues Capture. AreaGuard4D6F32 instead calls ordinary Approach;
    // do not invent Hunt's transition in that separate handler.
    sim.mission_assign_exact(
        engineer,
        MissionId::from_known(MissionType::Hunt),
        sim.session.binary_frame(),
    )
    .unwrap();
    for _ in 0..600 {
        sim.advance_tick(&[], Some(&rules), Some(&path), Some(&registry), 66);
        if sim
            .substrate
            .entities
            .get(building)
            .unwrap()
            .health
            .current()
            == full
        {
            assert!(
                sim.substrate
                    .entities
                    .get(engineer)
                    .is_none_or(|e| !e.lifecycle.object_alive())
            );
            return;
        }
    }
    let actor = sim.substrate.entities.get(engineer).unwrap();
    panic!(
        "Engineer did not repair: mission={:?}, target={:?}, nav={:?}, position=({}, {})",
        actor.mission.current(),
        actor.attack_target.target(),
        actor.navigation.nav_com(),
        actor.position.rx(),
        actor.position.ry()
    );
}
