//! Graceful main-menu quit cascade (presentation/teardown only).
//!
//! A non-blocking per-frame state machine, modelled on
//! [`crate::app::frontend::shell_transition::ShellFrameWave`]: it owns timing only and
//! returns the effects the app applies each frame (lower music volume, hard-stop
//! music, exit). The original runs this teardown as a blocking thread spin; the
//! port reproduces the same observable timing across frames so the winit event
//! loop keeps turning. Reproduces: music fade-out (concurrent with a bounded wait
//! for trailing voices) → music hard-stop → exit. The screen fade-to-black phase
//! is added in sub-step 4b-ii-b.
//!
//! ## Dependency rules
//! - App layer; depends only on std. No render/sim/audio type deps (the app maps
//!   directives onto MusicPlayer/SfxPlayer), so it stays headless-testable.

use std::time::Instant;

/// Music volume-fade rate: full scale (1.0) over 1000 ms, matching the original's
/// theme stop-with-fade (a full-scale volume interpolator over a 1000 ms divisor).
/// A fade from volume `v` therefore reaches silence in `v * 1000` ms.
const MUSIC_FADE_PER_MS: f64 = 1.0 / 1000.0;

/// Safety ceiling on the fade + trailing-voice wait. The original bounds its vox
/// pump-wait at `0xBB8` timer ticks (3000 × 16 ms). Effectively dominated by the
/// ≤1 s music fade, so this is reached only if audio never reports done.
const WAIT_CEILING_MS: u64 = 0xBB8 * 16; // 48_000 ms

/// Screen fade-to-black duration. The original fades the palette over `0x1E`=30
/// timer ticks × ~16 ms/tick ≈ 480 ms, linear to a black palette.
const SCREEN_FADE_MS: u64 = 480;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuitPhase {
    /// Music fading out while any trailing EVA/menu voice plays. Ends the instant
    /// the music fade completes OR voices finish OR the ceiling is hit.
    FadeMusicAndWaitVoices,
    /// Full-screen fade-to-black over [`SCREEN_FADE_MS`], after the hard music stop.
    FadeToBlack,
    /// Terminal — the app exits the event loop.
    Done,
}

/// Per-frame effects the app applies after ticking the cascade.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct QuitCascadeTick {
    /// New music volume to apply this frame (None once the fade is over).
    pub music_volume: Option<f64>,
    /// Hard-stop the music this frame (one-shot, on the fade→teardown edge).
    pub stop_music: bool,
    /// The cascade has finished — exit the event loop.
    pub finished: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct QuitCascade {
    phase: QuitPhase,
    phase_started_at: Instant,
    /// Live music volume captured when the cascade began (the fade start point).
    start_music_volume: f64,
    /// Latest fade-to-black alpha (0.0..=1.0), updated each FadeToBlack tick;
    /// read by the renderer for the black overlay.
    overlay_alpha: f32,
}

impl QuitCascade {
    /// Begin the cascade. `start_music_volume` is the live music volume at quit
    /// time (the caller has already persisted it before calling this).
    pub(crate) fn start(now: Instant, start_music_volume: f64) -> Self {
        Self {
            phase: QuitPhase::FadeMusicAndWaitVoices,
            phase_started_at: now,
            start_music_volume: start_music_volume.clamp(0.0, 1.0),
            overlay_alpha: 0.0,
        }
    }

    /// Advance the cascade and return the effects to apply this frame.
    /// `voices_active` is the live "any EVA/voice still playing" poll.
    pub(crate) fn tick(&mut self, now: Instant, voices_active: bool) -> QuitCascadeTick {
        let elapsed_ms = now.duration_since(self.phase_started_at).as_millis() as u64;
        match self.phase {
            QuitPhase::FadeMusicAndWaitVoices => {
                let faded =
                    (self.start_music_volume - elapsed_ms as f64 * MUSIC_FADE_PER_MS).max(0.0);
                let music_done = faded <= 0.0;
                if music_done || !voices_active || elapsed_ms >= WAIT_CEILING_MS {
                    self.enter(QuitPhase::FadeToBlack, now);
                    // Hard-stop music as the visual fade-to-black begins.
                    return QuitCascadeTick {
                        stop_music: true,
                        ..Default::default()
                    };
                }
                QuitCascadeTick {
                    music_volume: Some(faded),
                    ..Default::default()
                }
            }
            QuitPhase::FadeToBlack => {
                self.overlay_alpha = (elapsed_ms as f32 / SCREEN_FADE_MS as f32).min(1.0);
                if elapsed_ms >= SCREEN_FADE_MS {
                    self.enter(QuitPhase::Done, now);
                }
                // Finish on the NEXT tick so the fully-black frame is presented.
                QuitCascadeTick::default()
            }
            QuitPhase::Done => QuitCascadeTick {
                finished: true,
                ..Default::default()
            },
        }
    }

    fn enter(&mut self, phase: QuitPhase, now: Instant) {
        self.phase = phase;
        self.phase_started_at = now;
    }

    /// Current fade-to-black overlay alpha (0.0 = none, 1.0 = full black).
    pub(crate) fn overlay_alpha(&self) -> f32 {
        self.overlay_alpha
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// When the fade completes, the cascade hard-stops music, runs the ~480 ms
    /// fade-to-black ramp, then finishes.
    #[test]
    fn fade_completion_enters_screen_fade_then_finishes() {
        let t0 = Instant::now();
        let mut c = QuitCascade::start(t0, 1.0);
        // Music fade completes at 1000 ms → hard-stop + enter screen fade. The
        // fade clock starts at this `now` (t0+1000) since `enter` resets it.
        let edge = c.tick(at(t0, 1000), true);
        assert!(edge.stop_music && !edge.finished);
        // Alpha ramps linearly over the next 480 ms.
        let mid = c.tick(at(t0, 1000 + 240), true);
        assert!((c.overlay_alpha() - 0.5).abs() < 0.01);
        assert!(!mid.finished);
        // At 480 ms the all-black frame is presented (alpha 1.0, not yet finished)…
        let full = c.tick(at(t0, 1000 + 480), true);
        assert!((c.overlay_alpha() - 1.0).abs() < 1e-6);
        assert!(!full.finished);
        // …and the cascade finishes on the next tick.
        assert!(c.tick(at(t0, 1000 + 500), true).finished);
    }
}
