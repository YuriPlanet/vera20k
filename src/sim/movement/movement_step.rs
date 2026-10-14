//! Movement step helpers — vehicle rotation, Walk head completion and Walk
//! cell boundary crossing detection.

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::Position;
use crate::sim::movement::locomotor::LocomotorState;

/// Result of vehicle rotation — tells the caller whether to skip this tick.
pub(super) enum RotationResult {
    /// Still rotating in place — caller should `continue` (skip lepton advancement).
    StillRotating,
    /// Rotation complete or not needed — proceed with movement.
    ReadyToMove,
}

/// Handle vehicle in-place rotation before movement begins.
///
/// A vehicle holds position while its hull turns: gamemd's Drive/Ship Process
/// returns through its tail while `FacingClass::Is_Rotating` answers true on
/// the body (`0x004B0788`), whoever started the turn. The fresh arm issues
/// Do_Turn through the shared locomotor slot owner before this query.
/// Infantry are excluded by the caller.
pub(super) fn handle_vehicle_rotation(
    body_facing: &super::facing_class::FacingClass,
    native_frame: u32,
) -> RotationResult {
    if !body_facing.is_rotating(native_frame) {
        return RotationResult::ReadyToMove;
    }
    // Still rotating in place — the hull turns but the mover does not advance.
    RotationResult::StillRotating
}

#[cfg(test)]
#[path = "movement_step_tests.rs"]
mod tests;

/// Walk75BD70 completes a retained subcell head within 17 world leptons: the
/// owner's XY less the head's, `dx² + dy²` through `Sqrt_Approx` and
/// `Math::ftol` (`0x0075BD33..0x0075BD70`).
pub(super) fn completed_walk_head(
    position: &Position,
    locomotor: &Option<LocomotorState>,
) -> Option<crate::sim::components::DriveCoord> {
    let loco = locomotor
        .as_ref()
        .filter(|l| l.kind == LocomotorKind::Walk)?;
    let head = loco.step_head()?;
    let [x, y] = super::ground_pose::position_world_xy(position);
    (crate::util::native_x87::sqrt_approx_length([x.wrapping_sub(head.x), y.wrapping_sub(head.y)])
        < 17)
        .then_some(head)
}

/// Walk Process's paid-step boundary (0x0075C0F3..0x0075C117): after a paid
/// step, both actual cell coordinates are compared with the cell the mover
/// stood in. A changed cell suspends the step at the provisional coordinate;
/// the caller restores the old XYZ and runs the boundary corridor
/// (`walk_host`), which needs the whole store for Mark(REMOVE), SetCoords,
/// SetHeight and Mark(PUT). A diagonal step can enter a side cell before its
/// other axis crosses.
pub(super) fn walk_boundary_crossing(
    position: &Position,
) -> Option<crate::sim::components::DriveCoord> {
    let [x, y] = super::ground_pose::position_world_xy(position);
    (((x / 256) as u16, (y / 256) as u16) != (position.rx, position.ry))
        .then(|| super::ground_pose::position_world_coord(position))
}
