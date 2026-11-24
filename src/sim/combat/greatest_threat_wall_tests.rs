//! The ring walk's wall fallback (`TechnoClass::Cell_Threat_Fallback @
//! 0x006F8C10`) through the production scan owner on retail rules.

use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::overlay_grid::OverlayGrid;

/// What a computer MTNK on Guard picks with one GAWALL of `wall_owner`'s
/// three cells east and no object in reach, on retail rules under `patch`.
fn guard_pick(patch: &str, wall_owner: &str) -> Option<Option<TargetKind>> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str(patch),
    )?;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = &retail.rules;
    let weapon = rules.weapon("105mm").unwrap();
    assert!(
        rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .wall
    );
    assert!(
        rules
            .projectile(weapon.projectile.as_deref().unwrap())
            .unwrap()
            .ag
    );
    // The arena's computer house, Russians, plays Normal (`+0x184` = 1).
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    let tank = sim
        .spawn_object_with_overlay_registry("MTNK", "Russians", 16, 16, 64, rules, &registry)
        .expect("scanner");
    sim.spawn_object_with_overlay_registry("GAWALL", wall_owner, 19, 16, 0, rules, &registry)
        .expect("wall");
    Some(sim.greatest_threat_represented(rules, Some(&registry), tank, ScanMission::Guard, None))
}

#[test]
fn a_computer_tank_picks_an_enemy_wall_where_its_difficulty_destroys_walls() {
    // Retail RULESMD sets `DestroyWalls=no` on every difficulty row.
    let Some(retail) = guard_pick("", "Americans") else {
        return;
    };
    assert_eq!(retail, None);
    let destroy = "[Normal]\nDestroyWalls=yes\n";
    assert_eq!(
        guard_pick(destroy, "Americans").unwrap(),
        Some(TargetKind::Cell(19, 16)),
        "an enemy wall in range is the scan's cell target"
    );
    assert_eq!(
        guard_pick(destroy, "Russians").unwrap(),
        None,
        "the scanner's own wall is not"
    );
}
