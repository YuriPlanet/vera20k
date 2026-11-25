//! Turn streams: in the live object pass each Logic slot's turn draws from a
//! random stream of its own instead of the shared Scenario stream (VERA-only,
//! sim-scale; docs/scale-benchmark.md).
//!
//! gamemd keeps one Scenario stream (`*(0x00A8B230)+0x218`; `Random::Next @
//! 0x0065C780`, `Random::RandomRanged @ 0x0065C7E0`), and `LogicClass::AI @
//! 0x0055AFB0` walks the Logic vector with every object's AI drawing from it
//! in turn: the values a turn draws depend on how many every turn before it
//! drew. A turn that draws so writes state that every later turn reads, and
//! no turn that draws can run apart from the others.
//!
//! With the switch on ([`Simulation::separate_turn_streams`]: a match from its
//! start or from a load, its replay, and sim-bench unless `--shared-stream`),
//! the pass takes one raw draw from the Scenario stream as it begins, on every
//! frame, whether or not any turn then draws, and each Logic slot's visit,
//! with everything it calls, work on behalf of other objects too, draws from
//! a generator of its own, started from that draw, the frame and the slot's
//! object's stable id (`sim::rng`'s turn part). No turn's values depend on
//! the turns before it or on their order. The range reductions
//! (`RandomRanged`'s mask and rejection, the raw remainders) are unchanged:
//! only the raw words come from elsewhere. Draws outside the turns, the house
//! AI's unit choice above all, stay on the Scenario stream, but take other
//! words than they would: the pass's draw comes before every later one, and
//! the turns' draws no longer do. The Main stream is not split: its draws
//! choose only sounds and pictures.
//!
//! A player sees the same random behavior with the same spread (mission and
//! targeting delays, ROF jitter, scatter, gas drift), not the values gamemd
//! would draw for the same seed.
//!
//! Nothing new is saved. The pass sets and clears the turn part within its
//! frame (`master_frame`), and a save falls between frames. Nor is the
//! setting saved: a load turns it on again (`PreparedLoad::into_committed`).
//! The switch is its own, not the target scan batch's or another sim-scale
//! switch's: the others keep their order with this one off, which is how
//! `sim-bench --shared-stream` reproduces a build without it.

use super::Simulation;

impl Simulation {
    /// Draw each Logic slot's turn from its own stream from now on, or every
    /// turn from the Scenario stream in Logic order as gamemd does. Real
    /// matches, their loads and replays, and sim-bench turn it on; results
    /// depend on it.
    pub fn separate_turn_streams(&mut self, on: bool) {
        *self.turn_streams = on;
    }

    /// Whether turns draw from their own streams
    /// ([`Self::separate_turn_streams`]).
    pub fn turn_streams_separate(&self) -> bool {
        *self.turn_streams
    }

    /// The live object pass begins: with the switch on, its one draw from the
    /// Scenario stream becomes its key, on every frame alike.
    pub(super) fn begin_turn_streams(&mut self) {
        if *self.turn_streams {
            let frame = self.session.binary_frame();
            self.scenario_rng.begin_turn_streams(frame);
        }
    }

    /// The pass is over, or failed: every draw takes the Scenario stream.
    pub(super) fn end_turn_streams(&mut self) {
        if *self.turn_streams {
            self.scenario_rng.end_turn_streams();
        }
    }

    /// `id`'s Logic slot begins: its draws come from its own stream.
    pub(super) fn enter_turn_stream(&mut self, id: u64) {
        if *self.turn_streams {
            self.scenario_rng.enter_turn(id);
        }
    }

    /// `id`'s Logic slot is over. Debug builds panic when its turn changed
    /// the Scenario stream beyond its own generator.
    pub(super) fn leave_turn_stream(&mut self, id: u64) {
        if *self.turn_streams {
            debug_assert!(
                self.scenario_rng.has_turn_key(),
                "object {id}'s turn replaced the Scenario stream"
            );
            self.scenario_rng.leave_turn(id);
        }
    }
}
