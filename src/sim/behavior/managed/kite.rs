//! Kite: engage, pull back while the weapon reloads, engage again.
//!
//! The micro a player does with a fast unit against a slower gun: fire, step
//! out of its reach for the reload, step back in. Out of range it closes with
//! an attack-move rather than a plain move, so it starts shooting the moment it
//! arrives.
//!
//! Not a native behaviour: `gamemd`'s computer houses do not kite, and no
//! retail mechanism is reproduced here. See `behavior/managed/mod.rs`.

use crate::sim::behavior::goal::Goal;
use crate::sim::behavior::managed::{
    FallbackReason, MicroKind, MicroParams, MicroRoutine, MicroStep, Phase, Stage, step_away,
};
use crate::sim::behavior::view::{ObjectView, ThreatView};

/// The routine itself: no fields, no memory.
pub struct Kite;

impl MicroRoutine for Kite {
    fn kind(&self) -> MicroKind {
        MicroKind::Kite
    }

    fn step(
        &self,
        me: &ObjectView,
        world: &ThreatView<'_>,
        phase: &mut Phase,
        params: &MicroParams,
    ) -> MicroStep {
        let radius = i32::from(params.radius_cells);
        let standoff = i32::from(params.standoff_cells);
        let threat = world.nearest(me.cell, radius);

        // Hurt: leave, and let the brain decide what happens next once there is
        // nothing left to run from.
        if me.health_permille() <= params.health_floor {
            let Some(threat) = threat else {
                return MicroStep::Fallback(FallbackReason::HealthBelowFloor);
            };
            phase.stage = Stage::Withdraw;
            phase.target = Some(threat.entity_id);
            let cell = step_away(me.cell, threat.cell, standoff);
            if cell == me.cell {
                return MicroStep::Wait {
                    frames: params.hold_frames,
                };
            }
            return MicroStep::Goal(Goal::Move { cell });
        }

        let Some(threat) = threat else {
            phase.stage = Stage::Engage;
            phase.target = None;
            return MicroStep::Wait {
                frames: params.hold_frames,
            };
        };
        phase.target = Some(threat.entity_id);

        let range = me.weapon_range_cells;
        if range <= 0 {
            // Nothing to shoot with: not this routine's business.
            return MicroStep::Fallback(FallbackReason::NothingToDo);
        }
        if threat.cells > range {
            phase.stage = Stage::Engage;
            return MicroStep::Goal(Goal::AttackMove { cell: threat.cell });
        }
        if me.weapon_ready() {
            phase.stage = Stage::Engage;
            return MicroStep::Goal(Goal::Attack {
                target: threat.entity_id,
            });
        }
        // Reloading: step out of its reach rather than standing in it.
        phase.stage = Stage::Reload;
        let cell = step_away(me.cell, threat.cell, standoff);
        if cell == me.cell {
            return MicroStep::Wait {
                frames: params.hold_frames,
            };
        }
        MicroStep::Goal(Goal::Move { cell })
    }
}
