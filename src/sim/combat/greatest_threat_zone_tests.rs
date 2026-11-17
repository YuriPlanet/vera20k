//! Phobos' `TargetZoneScanType` ([`zone_admits`]) in a hunt across water,
//! through the production scan on retail rules and a map that sets the key: a
//! 48-cell rival arena whose columns from 24 on are Water, so a tank on the
//! land and a Destroyer on the water are in different zones.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{RetailBattleRules, retail_battle_rules_with_map_patch};
use crate::sim::combat::threat_range::ScanMission;

/// Retail rules with a map that gives the Rhino `any` and the Grizzly
/// `inrange`, which turns Phobos on.
fn phobos_rules() -> Option<RetailBattleRules> {
    let rules = retail_battle_rules_with_map_patch(&IniFile::from_str(
        "[HTNK]\nTargetZoneScanType=any\n[MTNK]\nTargetZoneScanType=InRange\n\
         [APOC]\nTargetZoneScanType=nowhere\n",
    ))?;
    assert!(rules.rules.phobos());
    Some(rules)
}

/// Whether a Russian `hunter` on the land at (20, 24) picks the player's
/// Destroyer at (`ship_x`, 24) when it hunts.
fn hunt_takes_ship(rules: &RuleSet, hunter: &str, ship_x: u16) -> bool {
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 1, 48);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        48,
        (0..48).flat_map(|y| (24..48).map(move |x| (x, y))),
    );
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let hunter = sim
        .spawn_object(hunter, "Russians", 20, 24, 64, rules)
        .expect("tank on land");
    let ship = sim
        .spawn_object("DEST", "Americans", ship_x, 24, 64, rules)
        .expect("destroyer on water");
    sim.greatest_threat_represented(rules, None, hunter, ScanMission::Hunt, None)
        .and_then(crate::sim::combat::TargetKind::object_id)
        == Some(ship)
}

/// The original refuses a ship in another zone; a map's `any` keeps it, and a
/// value Phobos does not know keeps the default `same`.
#[test]
fn phobos_any_hunts_a_ship_in_another_zone() {
    let Some(stock) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some(phobos) = phobos_rules() else {
        return;
    };
    assert!(!hunt_takes_ship(&stock.rules, "HTNK", 40));
    assert!(hunt_takes_ship(&phobos.rules, "HTNK", 40));
    let apocalypse = phobos.rules.object("APOC").unwrap();
    assert_eq!(apocalypse.target_zone_scan_type, TargetZoneScanType::Same);
    assert!(!hunt_takes_ship(&phobos.rules, "APOC", 40));
}

/// `inrange`: the Grizzly's 105mm (`Range=5.75`) reaches a ship two cells off
/// the shore from the land cell nearest it, but not one 16 cells off.
#[test]
fn phobos_inrange_hunts_a_ship_its_gun_reaches_from_the_shore() {
    let Some(phobos) = phobos_rules() else {
        return;
    };
    assert!(hunt_takes_ship(&phobos.rules, "MTNK", 26));
    assert!(!hunt_takes_ship(&phobos.rules, "MTNK", 40));
}
