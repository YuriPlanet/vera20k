//! The underground (`0x006F7E11..0x006F7E1E`), campaign discovery
//! (`0x006F81BE..0x006F81F4`) and HarvesterImmune (`0x006F8009..0x006F8047`)
//! candidate gates, through the production scan owner against retail rules
//! and the retail `[General] HarvesterUnit=` list.

use super::*;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::world::Simulation;

/// A guarding MTNK on the rival arena with one enemy candidate of
/// `target_type` the player has discovered, through the production spawn and
/// scan owners.
fn guard_scene(rules: &RuleSet, target_type: &str) -> (Simulation, u64, u64) {
    let (mut sim, tank, target) = rival_pair(rules, "MTNK", target_type);
    crate::sim::arena_fixture::discover(&mut sim, target);
    (sim, tank, target)
}

/// An `Americans` (human) `scanner_type` two cells west of a `Russians`
/// (computer) `target_type` on the rival arena.
fn rival_pair(rules: &RuleSet, scanner_type: &str, target_type: &str) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let scanner = sim
        .spawn_object(scanner_type, "Americans", 16, 16, 64, rules)
        .expect("scanner");
    let target = sim
        .spawn_object(target_type, "Russians", 18, 16, 64, rules)
        .expect("candidate");
    (sim, scanner, target)
}

fn guard_pick(sim: &mut Simulation, rules: &RuleSet, scanner: u64) -> Option<u64> {
    sim.greatest_threat_represented(rules, None, scanner, ScanMission::Guard, None)
        .and_then(crate::sim::combat::TargetKind::object_id)
}

/// The campaign discovery gate (`0x006F81BE..0x006F81F4`): with GameMode 0 a
/// human player's tank passes over an enemy the current house has not
/// discovered (`+0x41B`) and takes it once discovered. A computer scanner and
/// a skirmish have no such gate.
#[test]
fn campaign_player_scan_skips_an_enemy_the_player_has_not_discovered() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, tank, enemy) = rival_pair(rules, "MTNK", "HTNK");
    assert!(
        !sim.session.game_mode_nonzero(),
        "the arena runs the campaign"
    );
    assert_eq!(
        guard_pick(&mut sim, rules, tank),
        None,
        "campaign: the player's tank refuses an undiscovered enemy"
    );
    assert_eq!(
        guard_pick(&mut sim, rules, enemy),
        Some(tank),
        "a computer house's scanner has no discovery gate"
    );
    crate::sim::arena_fixture::discover(&mut sim, enemy);
    assert_eq!(
        guard_pick(&mut sim, rules, tank),
        Some(enemy),
        "campaign: a discovered enemy is an ordinary candidate"
    );

    let (mut sim, tank, enemy) = rival_pair(rules, "MTNK", "HTNK");
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    assert_eq!(
        guard_pick(&mut sim, rules, tank),
        Some(enemy),
        "skirmish: no discovery gate"
    );
}

/// The production discovery the gate reads: with the campaign player
/// installed, Techno vt+0x4A0's campaign prelude (`0x0070D997..0x0070D9C6`,
/// [`Simulation::update_techno_discovery_from_shroud`]) leaves an enemy in
/// shrouded ground undiscovered, so the player's tank refuses it, and
/// discovers it once the player has revealed its ground.
#[test]
fn campaign_discovery_prelude_admits_an_enemy_once_its_ground_is_revealed() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, tank, enemy) = rival_pair(rules, "MTNK", "HTNK");
    let player = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(player));
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    let discovered = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(enemy)
            .unwrap()
            .discovery
            .discovered_by_current_house()
    };
    sim.update_techno_discovery_from_shroud(enemy);
    assert!(!discovered(&sim), "the enemy's ground is shrouded");
    assert_eq!(guard_pick(&mut sim, rules, tank), None);

    crate::sim::vision::reveal_radius(&mut sim.fog, player, 18, 16, 1);
    sim.update_techno_discovery_from_shroud(enemy);
    assert!(
        discovered(&sim),
        "the player has revealed the enemy's ground"
    );
    assert_eq!(guard_pick(&mut sim, rules, tank), Some(enemy));
}

/// Aircraft (WhatAmI 2) pass the campaign discovery gate: the player's IFV
/// (HoverMissile, AA and AG) still takes an undiscovered paradrop plane.
/// Retail's Kirov (`ZEP`) is a VehicleType, so it takes no exemption.
#[test]
fn campaign_player_scan_takes_an_undiscovered_aircraft() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, plane) = rival_pair(rules, "FV", "PDPLANE");
    let actor = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(actor.category(), EntityCategory::Aircraft);
    assert!(!actor.discovery.discovered_by_current_house());
    assert_eq!(guard_pick(&mut sim, rules, ifv), Some(plane));
}

#[test]
fn guard_scan_honors_the_maps_techno_legal_target_override() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(None),
            &IniFile::from_str("[HTNK]\nLegalTarget=no\n"),
        )
        .unwrap()
        .into_parts();
    let (mut sim, tank, _) = guard_scene(&rules, "HTNK");
    assert_eq!(
        sim.greatest_threat_represented(&rules, None, tank, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id),
        None,
        "the map's LegalTarget=no must exclude an otherwise valid enemy"
    );

    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(None),
            &IniFile::from_str("[HTNK]\nLegalTarget=yes\n"),
        )
        .unwrap()
        .into_parts();
    let (mut sim, tank, target) = guard_scene(&rules, "HTNK");
    assert_eq!(
        sim.greatest_threat_represented(&rules, None, tank, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id),
        Some(target),
        "an explicit yes retains the normal target admission"
    );
}

/// Scenario bit `0x800` (`0x006F8009..0x006F8047`): while set, a guarding
/// tank refuses an enemy harvester — retail `[General] HarvesterUnit=HARV`
/// is the list the production reader resolves — and still takes an enemy
/// tank; with the bit clear the harvester is an ordinary candidate again.
#[test]
fn retail_guard_scan_skips_a_harvester_while_scenario_bit_800_is_set() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert!(
        rules
            .harvester_unit_types
            .iter()
            .any(|type_id| type_id.eq_ignore_ascii_case("HARV")),
        "retail [General] HarvesterUnit= lists HARV"
    );

    let (mut sim, tank, harv) = guard_scene(rules, "HARV");
    let pick = |sim: &mut Simulation| {
        sim.greatest_threat_represented(rules, None, tank, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id)
    };
    assert_eq!(
        pick(&mut sim),
        Some(harv),
        "bit clear: the harvester is an ordinary candidate"
    );
    sim.session.set_harvester_immune_for_test(true);
    assert_eq!(
        pick(&mut sim),
        None,
        "bit set: the listed HarvesterUnit type is refused"
    );

    // A candidate outside the list stays admissible while the bit is set.
    let (mut sim, tank, rhino) = guard_scene(rules, "HTNK");
    sim.session.set_harvester_immune_for_test(true);
    assert_eq!(
        sim.greatest_threat_represented(rules, None, tank, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id),
        Some(rhino),
        "bit set: a non-harvester tank is not refused"
    );
}

/// The underground gate (`0x006F7E11..0x006F7E1E`): a candidate whose
/// GetHeight (vt+0x1C8, `0x005F5F40`) is strictly below -20 leptons — the
/// exact Z set through the owning Position field, flat arena ground — is
/// refused; one at exactly -20 is not. VERA writes no such height today (no
/// Tunnel locomotor), so the boundary is what the port preserves.
#[test]
fn guard_scan_refuses_a_candidate_strictly_below_minus_twenty_leptons() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, tank, target) = guard_scene(rules, "HTNK");
    let pick = |sim: &mut Simulation| {
        sim.greatest_threat_represented(rules, None, tank, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id)
    };
    let set_height = |sim: &mut Simulation, z: i32| {
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(z));
    };
    set_height(&mut sim, -20);
    assert_eq!(
        pick(&mut sim),
        Some(target),
        "GetHeight -20 is not below the -20 cutoff"
    );
    set_height(&mut sim, -21);
    assert_eq!(
        pick(&mut sim),
        None,
        "GetHeight -21 is below the -20 cutoff: the candidate is underground"
    );
    set_height(&mut sim, -20);
    assert_eq!(
        pick(&mut sim),
        Some(target),
        "the gate reads the current height each scan"
    );
}
