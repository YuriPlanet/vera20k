//! Air split: rotate a flight's preferred target so its fire is spread.
//!
//! Each step picks the next hostile in distance order, so a flight of four
//! aircraft that would all aim at the nearest tank works through the four
//! nearest targets instead. It only sets *this wave's preference*: a unit still
//! retargets on its own when its target dies or leaves its weapon's reach, so
//! this is not an assignment and makes no claim to be one.
//!
//! Not a native behaviour: `gamemd`'s aircraft pick the nearest threat per
//! aircraft with no spread. See `behavior/managed/mod.rs`.

use crate::sim::behavior::goal::Goal;
use crate::sim::behavior::managed::{
    FallbackReason, MicroKind, MicroParams, MicroRoutine, MicroStep, Phase, Stage,
};
use crate::sim::behavior::view::{ObjectView, ThreatView};

/// The routine itself: no fields, no memory.
pub struct AirSplit;

impl MicroRoutine for AirSplit {
    fn kind(&self) -> MicroKind {
        MicroKind::AirSplit
    }

    fn step(
        &self,
        me: &ObjectView,
        world: &ThreatView<'_>,
        phase: &mut Phase,
        params: &MicroParams,
    ) -> MicroStep {
        if me.health_permille() <= params.health_floor {
            return MicroStep::Fallback(FallbackReason::HealthBelowFloor);
        }
        if !me.is_aircraft {
            return MicroStep::Fallback(FallbackReason::NothingToDo);
        }

        let mut threats: Vec<_> = world
            .within(me.cell, i32::from(params.radius_cells))
            .collect();
        if threats.is_empty() {
            phase.stage = Stage::Engage;
            phase.target = None;
            return MicroStep::Wait {
                frames: params.hold_frames,
            };
        }
        // Distance first, id second: a total order, so the rotation is
        // reproducible from the world alone.
        threats.sort_by_key(|threat| (threat.cells, threat.entity_id));

        let index = usize::try_from(phase.wave).unwrap_or(0) % threats.len();
        let target = threats[index];
        phase.wave = phase.wave.wrapping_add(1);
        phase.stage = Stage::Engage;
        phase.target = Some(target.entity_id);
        MicroStep::Goal(Goal::Attack {
            target: target.entity_id,
        })
    }
}
