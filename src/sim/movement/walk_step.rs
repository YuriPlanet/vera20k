//! Paid Walk75BFA9..75C0CB step, between admission/completion and placement.
//!
//! RESIDUAL: the gate in front of it is not ported. With the head still 17
//! leptons or more away (0x75BD70), an owner under EMP (vt+0x37C, Techno
//! 0x0070EFD0: +0x504 > 0) clears Walk +0x36 and Foot +0x68A and returns
//! without stepping (0x75BF85..0x75BFA6).
//! - Trigger: a walking infantryman under an EMP effect.
//! - Effect: it keeps stepping, and its Walk sequence, instead of freezing.
//! - Frequency: none with retail data: VERA has no EMP timer and no stock
//!   weapon mounts EMPuls (`warhead_type.rs`).
//! - Risk: modded EMP warheads.
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::game_entity::GameEntity;
use crate::sim::pathfinding::PathGrid;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};

/// Walk75BCE3..75BD16/75C1EA: with no head or destination, a Process visit
/// stops a positive Foot speed and clears +68A. Refusal and Stop owners do not
/// share this tail. Native controls: foot_scold_latch.json paid_tails.
pub(super) fn finish_idle(entity: &mut GameEntity) {
    if !entity.locomotor.as_ref().is_some_and(|loco| {
        loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk
            && loco.step_head().is_none()
            && loco.walk_destination().is_none()
    }) {
        return;
    }
    if entity.foot_speed.applied_fraction() > SIM_ZERO {
        entity.foot_speed.set_speed_fraction(SIM_ZERO);
    }
    entity.navigation.path_runtime.clear_scold_latch();
}

/// Publish Foot speed/facing and the provisional numeric step. The caller owns
/// the ordinary completion and boundary transactions; same-cell height is paid
/// here before that continuation. No new retained movement representation.
pub(super) fn advance(
    entity: &mut GameEntity,
    adjusted_speed: SimFixed,
    prone_crawls: Option<bool>,
    native_frame: u32,
    terrain: Option<&ResolvedTerrainGrid>,
    grid: Option<&PathGrid>,
) {
    let head = entity
        .locomotor
        .as_ref()
        .and_then(|loco| loco.step_head())
        .expect("paid Walk requires its admitted head");
    entity.foot_speed.set_speed_fraction(SIM_ONE); // owner +578 / Foot4D3710
    let speed = super::foot_speed::owner_current_speed_from_fraction(
        adjusted_speed,
        entity.foot_speed.applied_fraction(),
    );
    let speed = prone_crawls.map_or(speed, |crawls| {
        crate::sim::infantry::apply_prone_speed(SimFixed::from_num(speed), crawls).to_num::<i32>()
    });
    entity.navigation.path_runtime.path_blocked = false; //75BFD1; retain grace timer.
    let current = super::ground_pose::position_world_xy(&entity.position);
    let desired = crate::util::direction_tables::facing16_from_delta(
        head.x.wrapping_sub(current[0]),
        head.y.wrapping_sub(current[1]),
    );
    // Walk75AE00 snaps the body, but SI still supplies the desired direction to
    // the displacement even if snap's equality branch retains an old target.
    super::locomotor_turn::walk_do_turn(entity, desired, native_frame);
    let proposed = crate::util::native_trig::facing_step_world_xy(current, desired, speed);
    // A boundary step's staged sub-cell only feeds the crossing test, which
    // restores the old Location for walk_host's SetLocation (0x0075C12E). A
    // same-cell step's is its Location, which native sets through
    // FootClass::SetLocation at 0x0075C20F.
    // RESIDUAL: this direct write skips that call's OpenTopped rider tail.
    // Dormant: retail rulesmd.ini makes only the Drive BFRT OpenTopped=yes.
    entity.position.sub_x = SimFixed::from_num(proposed[0] - i32::from(entity.position.rx) * 256);
    entity.position.sub_y = SimFixed::from_num(proposed[1] - i32::from(entity.position.ry) * 256);
    if proposed[0] / 256 == i32::from(entity.position.rx)
        && proposed[1] / 256 == i32::from(entity.position.ry)
    {
        // Native clears +74 around this SetCoords/SetHeight pair (75C1FB..75C227).
        super::ground_pose::set_height(&mut entity.position, entity.on_bridge, 0, terrain, grid);
        //75C22F follows the same-cell SetCoords/SetHeight callbacks. A
        //boundary step retains the byte until its world placement completes.
        entity.navigation.path_runtime.clear_scold_latch();
    }
}

#[cfg(test)]
#[path = "walk_step_tests.rs"]
mod tests;
