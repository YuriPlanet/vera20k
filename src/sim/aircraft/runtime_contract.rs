//! Evidence-bounded YR aircraft runtime contracts.

#[cfg(test)]
use std::collections::HashSet;

use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireLocationSearch {
    pub aircraft_leptons: (i32, i32),
    pub reference_leptons: Option<(i32, i32)>,
    pub range_leptons: i32,
    pub playable_cells: (u16, u16),
    /// Scenario RNG result already reduced to the native inclusive 0..99 range.
    pub random_0_to_99: u8,
}

/// Search the first usable sixteen-angle ring for an aircraft firing cell.
///
/// Named location: `AircraftClass::FindFireLocation`.
#[cfg(test)]
pub fn find_fire_location(
    input: FireLocationSearch,
    blocked_cells: &HashSet<(u16, u16)>,
) -> Option<(u16, u16)> {
    let mut radius = input.range_leptons - 0x100;
    if radius <= 0x100 {
        return None;
    }
    let reference = input.reference_leptons.unwrap_or(input.aircraft_leptons);

    while radius > 0x100 {
        let mut best: Option<(u64, (u16, u16))> = None;
        let mut second: Option<(u64, (u16, u16))> = None;
        for angle in (0..=0xf0).step_by(0x10) {
            let angle_units = ((angle as i32) << 8) - 0x3fff;
            let radians = angle_units as f64 * -std::f64::consts::PI / 32768.0;
            let candidate_x =
                (input.aircraft_leptons.0 as f64 + radians.sin() * radius as f64) as i32;
            let candidate_y =
                (input.aircraft_leptons.1 as f64 - radians.cos() * radius as f64) as i32;
            let cell_x = candidate_x / 0x100;
            let cell_y = candidate_y / 0x100;
            if cell_x < 0
                || cell_y < 0
                || cell_x >= input.playable_cells.0 as i32
                || cell_y >= input.playable_cells.1 as i32
            {
                continue;
            }
            let cell = (cell_x as u16, cell_y as u16);
            if blocked_cells.contains(&cell) {
                continue;
            }
            let dx = i64::from(reference.0 - candidate_x);
            let dy = i64::from(reference.1 - candidate_y);
            let distance = ((dx * dx + dy * dy) as f64).sqrt() as u64;
            let ranked = (distance, cell);
            if best.is_none_or(|current| ranked.0 < current.0) {
                second = best;
                best = Some(ranked);
            } else if second.is_none_or(|current| ranked.0 < current.0) {
                second = Some(ranked);
            }
        }
        if let Some(best) = best {
            return Some(if input.random_0_to_99 < 0x32 {
                best.1
            } else {
                second.unwrap_or(best).1
            });
        }
        radius -= 0x100;
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AircraftUpdateStep {
    MissionFlagHousekeeping,
    SecondaryLocomotor,
    MovementSmoke,
    FiringLocomotorProcess,
    OccupyMissionResolve,
    MindControlDisconnect,
    BaseFootUpdate,
    CrashWobble,
    ContrailAnim,
    OffMapLimbo,
    OnMapPassengerSync,
}

/// Return the proved slot-23 update sequence without assigning semantics to its predicates.
/// Named location: `AircraftClass::Update` (`yr_1001` 0x414bb0).
#[cfg(test)]
pub fn aircraft_update_steps(
    alive_after_firing: bool,
    firing_or_landing: bool,
    alive_after_foot: bool,
    off_map: bool,
) -> Vec<AircraftUpdateStep> {
    use AircraftUpdateStep::*;
    let mut steps = vec![
        MissionFlagHousekeeping,
        SecondaryLocomotor,
        MovementSmoke,
        FiringLocomotorProcess,
    ];
    if !alive_after_firing {
        return steps;
    }
    if firing_or_landing {
        steps.push(OccupyMissionResolve);
        return steps;
    }
    steps.extend([MindControlDisconnect, BaseFootUpdate]);
    if !alive_after_foot {
        return steps;
    }
    steps.extend([CrashWobble, ContrailAnim]);
    if off_map {
        steps.push(OffMapLimbo);
    } else {
        steps.push(OnMapPassengerSync);
    }
    steps
}

/// Convert the statically proved default paradrop edge source into launch facing.
/// Named location: YR linked-aircraft paradrop launch dispatch.
pub fn paradrop_edge_facing_word(default_edge: i32, alternate_type_state: bool) -> u16 {
    let edge = if default_edge == -1 { 0 } else { default_edge };
    let doubled = edge.wrapping_mul(2);
    let base = doubled.wrapping_shl(13);
    if alternate_type_state {
        base.wrapping_sub(0x6001) as u16 & 0xe000
    } else {
        base as u16
    }
}

/// AircraftClass's shared Cell leaf for landing probes.
///
/// Winged returns before map, zone, occupation, wall, or land reads. The live
/// caller must still apply pad/occupant ownership and shroud/state rules.
// Native: AircraftClass::IsCellOccupied wrapper -> CellClass::IsClearToMove.
pub fn aircraft_landing_cell_leaf_clear() -> bool {
    matches!(
        evaluate_live_cell_passability(LiveCellPassabilityQuery {
            target: (0, 0),
            speed_type: SpeedType::Winged,
            movement_zone: MovementZone::Normal,
            requested_zone: None,
            actual_zone: 0,
            requested_layer: None,
            ignore_infantry: false,
            ignore_vehicles: false,
            land_passable: false,
            path_grid: None,
            resolved_terrain: None,
            raw_occupation: None,
        }),
        IsClearToMoveResult::ClearWinged
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_search_rejects_short_ranges() {
        let input = FireLocationSearch {
            aircraft_leptons: (10 * 256, 10 * 256),
            reference_leptons: None,
            range_leptons: 512,
            playable_cells: (20, 20),
            random_0_to_99: 0,
        };
        assert_eq!(find_fire_location(input, &HashSet::new()), None);
    }

    #[test]
    fn radial_search_uses_first_ring_and_rng_rank() {
        let base = FireLocationSearch {
            aircraft_leptons: (10 * 256 + 128, 10 * 256 + 128),
            reference_leptons: Some((10 * 256 + 128, 4 * 256 + 128)),
            range_leptons: 5 * 256,
            playable_cells: (30, 30),
            random_0_to_99: 0,
        };
        let first = find_fire_location(base, &HashSet::new()).unwrap();
        let second = find_fire_location(
            FireLocationSearch {
                random_0_to_99: 99,
                ..base
            },
            &HashSet::new(),
        )
        .unwrap();
        assert_ne!(first, second);
        assert!((first.0 as i32 - 10).abs().max((first.1 as i32 - 10).abs()) >= 3);
    }

    #[test]
    fn firing_branch_skips_base_update() {
        let steps = aircraft_update_steps(true, true, true, false);
        assert_eq!(
            steps.last(),
            Some(&AircraftUpdateStep::OccupyMissionResolve)
        );
        assert!(!steps.contains(&AircraftUpdateStep::BaseFootUpdate));
    }

    #[test]
    fn normal_branch_preserves_tail_order() {
        let steps = aircraft_update_steps(true, false, true, false);
        assert_eq!(steps[4], AircraftUpdateStep::MindControlDisconnect);
        assert_eq!(steps[5], AircraftUpdateStep::BaseFootUpdate);
        assert_eq!(steps.last(), Some(&AircraftUpdateStep::OnMapPassengerSync));
    }

    #[test]
    fn paradrop_edge_normalization_and_facing_are_exact() {
        assert_eq!(paradrop_edge_facing_word(-1, false), 0);
        assert_eq!(paradrop_edge_facing_word(1, false), 0x4000);
        assert_eq!(paradrop_edge_facing_word(2, false), 0x8000);
        assert_eq!(paradrop_edge_facing_word(3, false), 0xc000);
        assert_eq!(paradrop_edge_facing_word(1, true), 0xc000);
    }

    #[test]
    fn aircraft_landing_cell_leaf_preserves_winged_early_return() {
        assert!(aircraft_landing_cell_leaf_clear());
    }
}
