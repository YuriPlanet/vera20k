//! Ares 3.0 moves the destroyable-cliff roll from the railgun's ground stop
//! to DamageArea (`ares-src/src/Misc/Bugfixes.Buffer.cpp`).

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{DeathEffects, TargetKind};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, Simulation};

/// CollapseChance=0: a destroyable cliff spends its draw but never collapses.
fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nCollapseChance=0\n\
         [VehicleTypes]\n0=FIRER\n[FIRER]\nStrength=100\nPrimary=RAIL\n\
         [RAIL]\nDamage=0\nAmbientDamage=10\nRange=8\nWarhead=WH\nIsRailgun=yes\n\
         [WH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("cliff rules")
}

/// A level-1 8x8 map whose cell (1,1) is a destroyable cliff tile.
fn world_with_cliff() -> Simulation {
    let cells = (0..8u16)
        .flat_map(|ry| (0..8u16).map(move |rx| (rx, ry)))
        .map(|(rx, ry)| {
            let mut cell = crate::sim::world::common_raw_test_terrain_cell(rx, ry, 1, false);
            if (rx, ry) == (1, 1) {
                cell.final_tile_index = 100;
            }
            cell
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(8, 8, cells);
    terrain.test_install_destroyable_cliff_catalog(100);
    assert!(terrain.is_destroyable_cliff(1, 1));
    let mut sim = Simulation::with_seed(0x70CA);
    sim.resolved_terrain = Some(terrain);
    sim
}

/// TechnoClass_DealParticleDamage_DontDestroyCliff (1005BF80, hook
/// 0x70CAD8): a railgun line that meets the ground on a destroyable cliff
/// stops there without the CollapseChance draw.
#[test]
fn ares_railgun_ground_stop_spends_no_cliff_draw() {
    let rules = rules();
    let mut sim = world_with_cliff();
    let before = sim.scenario_rng.logical_state();
    // From below the ground of cell (0,1), the first new cell is the cliff.
    let returned = super::railgun::line_damage(
        &mut sim,
        &rules,
        None,
        FrameEffects::default(),
        &mut DeathEffects::default(),
        0,
        ProjectileCoord::new(128, 256 + 128, 0),
        TargetKind::Cell(5, 1),
        rules.weapon("RAIL").unwrap(),
    );
    assert_eq!(returned.x / 256, 1, "the line stops on the cliff cell");
    assert_eq!(sim.scenario_rng.logical_state(), before);
}

/// DamageArea_DestroyCliff (1005BE20, hook 0x489562): an area detonation
/// on a destroyable cliff spends one RandomRanged(0, 99) draw against
/// CollapseChance.
#[test]
fn ares_area_damage_rolls_the_impact_cliff() {
    let rules = rules();
    let mut sim = world_with_cliff();
    let mut expected = sim.scenario_rng.clone();
    let _cliff_roll = expected.next_range_u32_inclusive(0, 99);
    let warhead_ref = sim.interner.intern("WH");
    super::world_receiver::collect_area(
        &mut sim,
        &rules,
        None,
        (1, 1),
        100,
        rules.warhead("WH").unwrap(),
        true,
        (super::RAD_NO_ATTACKER, None, warhead_ref),
        None,
        1,
        FrameEffects::default(),
    );
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}
