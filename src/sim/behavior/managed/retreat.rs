//! Retreat: take a hurt unit out of the fight, to the rally post when there is
//! one.
//!
//! The routine runs while the unit is below its health floor and hands it back
//! (`Done`) as soon as it is healthy again, so a caller can attach it once and
//! leave it. With a post it walks there and stops; without one it backs away
//! from the nearest threat and gives up when there is nothing left to run from.
//!
//! Not a native behaviour. Ammunition is **not** a retreat trigger in this
//! slice: [`ObjectView`](crate::sim::behavior::view::ObjectView) carries the
//! type's `Ammo=` limit, not a per-shot magazine, because no per-shot
//! ammunition counter is ported for units. That residual is recorded in
//! `docs/ai-layers.md`.

use crate::sim::behavior::goal::Goal;
use crate::sim::behavior::managed::{
    FallbackReason, MicroKind, MicroParams, MicroRoutine, MicroStep, Phase, Stage, step_away,
};
use crate::sim::behavior::view::{ObjectView, ThreatView};

/// The routine itself: no fields, no memory.
pub struct Retreat;

impl MicroRoutine for Retreat {
    fn kind(&self) -> MicroKind {
        MicroKind::Retreat
    }

    fn step(
        &self,
        me: &ObjectView,
        world: &ThreatView<'_>,
        phase: &mut Phase,
        params: &MicroParams,
    ) -> MicroStep {
        if me.health_permille() > params.health_floor {
            phase.stage = Stage::Engage;
            return MicroStep::Done;
        }
        if let Some(post) = phase.post {
            if me.cell == post {
                phase.stage = Stage::Regroup;
                return MicroStep::Done;
            }
            phase.stage = Stage::Regroup;
            return MicroStep::Goal(Goal::Move { cell: post });
        }
        let Some(threat) = world.nearest(me.cell, i32::from(params.radius_cells)) else {
            return MicroStep::Fallback(FallbackReason::HealthBelowFloor);
        };
        phase.target = Some(threat.entity_id);
        phase.stage = Stage::Withdraw;
        let cell = step_away(me.cell, threat.cell, i32::from(params.standoff_cells));
        if cell == me.cell {
            return MicroStep::Fallback(FallbackReason::NothingToDo);
        }
        MicroStep::Goal(Goal::Move { cell })
    }
}
