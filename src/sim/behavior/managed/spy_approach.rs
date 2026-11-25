//! Spy approach: wait out a patrol, then walk in and take the building.
//!
//! The capture click already walks the infantryman in
//! (`Command::CaptureBuilding` → the Capture mission), so the routine only has
//! to decide *when* to spend him: while a hostile stands close enough to shoot
//! the approach, it waits instead. A player does the same by holding the spy
//! behind a corner until the dog passes.
//!
//! Not a native behaviour and deliberately coarse: it reads proximity, not the
//! vision, disguise or dog-detection rules, so it avoids anything hostile
//! rather than anything that can actually see through the disguise. See
//! `behavior/managed/mod.rs`.

use crate::sim::behavior::goal::Goal;
use crate::sim::behavior::managed::{
    FallbackReason, MicroKind, MicroParams, MicroRoutine, MicroStep, Phase, Stage,
};
use crate::sim::behavior::view::{ObjectView, ThreatView};

/// The routine itself: no fields, no memory.
pub struct SpyApproach;

impl MicroRoutine for SpyApproach {
    fn kind(&self) -> MicroKind {
        MicroKind::SpyApproach
    }

    fn step(
        &self,
        me: &ObjectView,
        world: &ThreatView<'_>,
        phase: &mut Phase,
        params: &MicroParams,
    ) -> MicroStep {
        let Some(target) = phase.target else {
            return MicroStep::Fallback(FallbackReason::NoTarget);
        };
        if me.is_structure || me.is_aircraft {
            return MicroStep::Fallback(FallbackReason::NothingToDo);
        }
        // A hostile within the standoff may be watching the way in: hold.
        if world.count_within(me.cell, i32::from(params.standoff_cells)) > 0 {
            phase.stage = Stage::Avoid;
            return MicroStep::Wait {
                frames: params.hold_frames,
            };
        }
        phase.stage = Stage::Engage;
        MicroStep::Goal(Goal::Capture { target })
    }
}
