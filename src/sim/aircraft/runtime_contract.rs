//! Evidence-bounded YR aircraft runtime contracts.

use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};

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
