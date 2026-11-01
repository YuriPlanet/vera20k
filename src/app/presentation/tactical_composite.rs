//! One tactical composite per Main_Tick.
//!
//! Offline, `Main_Tick @ 0x0055D360` draws the tactical view once per tick:
//! `RenderFrame_main @ 0x004F4480` runs at `0x0055D8F2` on an ordinary tick
//! and at `0x0055D84F` on a scenario-pause tick (campaign and skirmish), and
//! `Main__ThrottleFrame @ 0x0055E160` then sleeps without drawing. While the
//! chrono screen holds (`0x0053BAE0`), `0x004F4480` skips the tactical draw.
//! So the effects that step inside the composite (laser jitter, electric bolts,
//! airstrike flare shades, line trails, radiation beams, the superweapon range
//! pulse and Ares flying credits) and the lines that read `timeGetTime` while
//! drawing (mind-control links, psychic lines and rings, the airstrike wobble,
//! planning dashes) move once per tick, however fast the display refreshes.
//!
//! VERA draws on display refresh. This owner counts the Main_Ticks and tells
//! each draw how many composites it owes; a draw that owes none shows the last
//! composite's output again. Online, `0x0055E23A..0x0055E271` also renders
//! while ThrottleFrame waits, so network matches composite on every draw, as
//! do the capture drivers, whose recorded frames are built that way.

/// How often a draw composites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompositeCadence {
    /// Offline play: once per Main_Tick.
    PerMainTick,
    /// Network matches and capture drivers: once per draw.
    PerDraw,
}

/// The most composites one draw runs: one service catches up at most eight
/// Main_Ticks, so a longer backlog builds only while nothing draws (a hidden
/// window, an outcome wait).
const MAX_STEPS_PER_DRAW: u64 = 8;

/// The Main_Tick count and the composites already drawn for it.
#[derive(Debug, Default)]
pub(crate) struct TacticalComposite {
    ticks: u64,
    shown: u64,
    clock_ms: u64,
}

impl TacticalComposite {
    /// A Main_Tick ran: an admitted Logic pass or a paced scenario-pause pass.
    pub(crate) fn note_main_tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    /// The composites this draw runs, at most eight; none while the chrono
    /// screen holds, whose ticks are dropped. `wall_ms` becomes the
    /// composite's clock ([`Self::clock_ms`]) when the draw composites.
    pub(crate) fn begin_draw(
        &mut self,
        cadence: CompositeCadence,
        wall_ms: u64,
        chrono_holds: bool,
    ) -> u32 {
        let owed = self.ticks.wrapping_sub(self.shown);
        self.shown = self.ticks;
        let steps = match cadence {
            CompositeCadence::PerDraw => 1,
            CompositeCadence::PerMainTick if chrono_holds => 0,
            CompositeCadence::PerMainTick => owed.min(MAX_STEPS_PER_DRAW),
        };
        if steps > 0 {
            self.clock_ms = wall_ms;
        }
        steps as u32
    }

    /// The Main_Ticks counted so far: a picture drawn before the latest one
    /// owes a composite.
    pub(crate) fn ticks(&self) -> u64 {
        self.ticks
    }

    /// The `timeGetTime` of the last composite, read by the lines that move
    /// with the clock.
    pub(crate) fn clock_ms(&self) -> u64 {
        self.clock_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Owed ticks step at most eight composites; a second draw of the same
    /// tick steps none and keeps the clock; the chrono screen drops what is
    /// owed; per-draw cadence steps and samples every draw.
    #[test]
    fn begin_draw_steps_owed_ticks_up_to_the_catch_up_limit() {
        let mut composite = TacticalComposite::default();
        for _ in 0..9 {
            composite.note_main_tick();
        }
        assert_eq!(
            composite.begin_draw(CompositeCadence::PerMainTick, 100, false),
            8
        );
        assert_eq!(
            composite.begin_draw(CompositeCadence::PerMainTick, 117, false),
            0
        );
        assert_eq!(composite.clock_ms(), 100);
        composite.note_main_tick();
        assert_eq!(
            composite.begin_draw(CompositeCadence::PerMainTick, 133, true),
            0
        );
        assert_eq!(
            composite.begin_draw(CompositeCadence::PerMainTick, 150, false),
            0,
            "the chrono screen dropped its tick"
        );
        assert_eq!(
            composite.begin_draw(CompositeCadence::PerDraw, 166, true),
            1
        );
        assert_eq!(composite.clock_ms(), 166);
        assert_eq!(composite.ticks(), 10);
    }
}
