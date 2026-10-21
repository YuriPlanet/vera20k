//! Running-scenario exit audio handoff.
//!
//! Retail blocks its main thread while the audio master fades and queued voice
//! work drains. The app expresses the same order as a small per-frame state
//! machine so the window and renderer remain responsive while simulation is
//! frozen. The destination is committed only after audio teardown finishes.
//!
//! ## Dependency rules
//! - App layer; timing and destination data only. Audio effects are returned to
//!   the orchestrator, which owns the concrete players.

use crate::ui::score_shell::ScoreScreenModel;

/// `VolumeInterp` is constructed with a 1000-ms ramp in `0x00401000`.
const AUDIO_FADE_MS: u64 = 1_000;
/// Once `SavourDelay` expires, HouseClass waits at most 0x78 16-ms wall
/// buckets for the outcome EVA before raising the session-end global.
const OUTCOME_VOICE_WAIT_BUCKETS: u64 = 0x78;
/// The ordinary victory/defeat teardown waits at most 300 16-ms buckets after
/// the master-volume ramp completes (`0x00685670`, `0x00685DC0`).
const VOICE_WAIT_BUCKETS: u64 = 300;

fn wall_bucket(wall_ms: u64) -> u64 {
    wall_ms >> 4
}

pub(crate) fn outcome_title(kind: crate::sim::house_state::HouseOutcomeKind) -> &'static str {
    match kind {
        crate::sim::house_state::HouseOutcomeKind::Victory => "You are Victorious!",
        crate::sim::house_state::HouseOutcomeKind::Defeat => "You have Lost",
    }
}

pub(crate) fn outcome_detail(kind: crate::sim::house_state::HouseOutcomeKind) -> &'static str {
    match kind {
        crate::sim::house_state::HouseOutcomeKind::Victory => {
            "All enemy forces have been defeated."
        }
        crate::sim::house_state::HouseOutcomeKind::Defeat => "Your forces have been eliminated.",
    }
}

/// The termination bytes Main_Game samples once per loop: the local House's
/// ready victory/loss (`0x00A83D49`/`0x00A8ECD0`, one House holds at most
/// one), the campaign restart request (`0x008B41C0`) and an executed EXIT
/// (`0x00A83D48`). Main_Tick `0x0055DE4F..0x0055DE71` advances no frame while
/// any is set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TerminationRequests {
    pub outcome_ready: bool,
    pub restart: bool,
    pub exit: bool,
}

/// The one route `Main_Game__Route_Scenario_Termination 0x0055CFD0` takes.
/// Every taken route clears all four bytes, so a lower-priority request
/// raised in the same pass is dropped, not deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminationRoute {
    /// GameExit::Victory `0x00685670` or GameExit::Defeat `0x00685DC0`.
    Outcome,
    /// `0x006863E0`: Start_Scenario on the same map, no movie, prompt or score.
    Restart,
    /// GameExit::BattleControlTerminated `0x00686570`.
    Exit,
}

/// Priority victory > defeat > restart > exit, as `0x0055CFD0` tests them.
pub(crate) const fn route_scenario_termination(
    requests: TerminationRequests,
) -> Option<TerminationRoute> {
    if requests.outcome_ready {
        Some(TerminationRoute::Outcome)
    } else if requests.restart {
        Some(TerminationRoute::Restart)
    } else if requests.exit {
        Some(TerminationRoute::Exit)
    } else {
        None
    }
}

/// App-owned wall-clock Vox drain entered only after the serialized HouseClass
/// SavourDelay state reaches its terminal late-frame boundary.
#[derive(Debug, Clone)]
pub(crate) struct ScenarioOutcomeVoiceWait {
    kind: crate::sim::house_state::HouseOutcomeKind,
    started_bucket: u64,
}

impl ScenarioOutcomeVoiceWait {
    pub(crate) fn start(wall_ms: u64, kind: crate::sim::house_state::HouseOutcomeKind) -> Self {
        Self {
            kind,
            started_bucket: wall_bucket(wall_ms),
        }
    }

    pub(crate) fn kind(&self) -> crate::sim::house_state::HouseOutcomeKind {
        self.kind
    }

    /// HouseClass::Update @ 0x004F8440 pumps the current Vox channel until it
    /// becomes idle or 0x78 absolute 16-ms timer buckets have elapsed.
    pub(crate) fn tick(&self, wall_ms: u64, voices_active: bool) -> bool {
        !voices_active
            || wall_bucket(wall_ms).saturating_sub(self.started_bucket)
                >= OUTCOME_VOICE_WAIT_BUCKETS
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ScenarioExitDestination {
    /// Skirmish/multiplayer score dialog `0x108`.
    Score {
        title: String,
        detail: String,
        model: ScoreScreenModel,
    },
    /// Campaign victory's distinct retail dialog `0x103`.
    CampaignScore {
        title: String,
        detail: String,
        model: crate::ui::campaign_score_shell::CampaignScoreScreenModel,
    },
    /// Campaign victory with `[Basic] SkipScore=yes` continues directly.
    CampaignVictoryContinue,
    /// Campaign defeat bypasses `0x108`: GameExit::Defeat plays the Lose
    /// movie and asks `TXT_TO_REPLAY` instead (`0x00686060..0x006860F6`).
    CampaignDefeat,
    /// A network Cooperative mission: GameExit's mode vt+8 arm draws the
    /// empty backdrop instead of `0x108` (`0x00685851..0x0068586A`), then
    /// runs `+0xA8`, the result dialog BC6.
    CooperativeResult {
        victory: bool,
    },
    MainMenu,
}

/// Campaign outcome paths around GameExit's score/prompt calls. This decision
/// uses the live session mode, not an installed-campaign token: restored
/// Campaign saves still own the campaign result presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampaignOutcomeRoute {
    VictoryScore,
    VictoryContinue,
    DefeatPrompt,
}

pub(crate) fn campaign_outcome_route(
    mode: super::session::SessionMode,
    outcome: crate::sim::house_state::HouseOutcomeKind,
    skip_score: bool,
) -> Option<CampaignOutcomeRoute> {
    if mode != super::session::SessionMode::Campaign {
        return None;
    }
    Some(match outcome {
        crate::sim::house_state::HouseOutcomeKind::Victory if skip_score => {
            CampaignOutcomeRoute::VictoryContinue
        }
        crate::sim::house_state::HouseOutcomeKind::Victory => CampaignOutcomeRoute::VictoryScore,
        crate::sim::house_state::HouseOutcomeKind::Defeat => CampaignOutcomeRoute::DefeatPrompt,
    })
}

/// Commit the route selected for a terminal outcome. Model builders are lazy:
/// campaign SkipScore and defeat paths do not construct either score model,
/// and a campaign result never evaluates the multiplayer score owner.
pub(crate) fn scenario_exit_destination(
    mode: super::session::SessionMode,
    outcome: crate::sim::house_state::HouseOutcomeKind,
    skip_score: bool,
    cooperative: bool,
    title: String,
    detail: String,
    campaign_score: impl FnOnce() -> crate::ui::campaign_score_shell::CampaignScoreScreenModel,
    skirmish_score: impl FnOnce() -> ScoreScreenModel,
) -> ScenarioExitDestination {
    match campaign_outcome_route(mode, outcome, skip_score) {
        Some(CampaignOutcomeRoute::VictoryScore) => ScenarioExitDestination::CampaignScore {
            title,
            detail,
            model: campaign_score(),
        },
        Some(CampaignOutcomeRoute::VictoryContinue) => {
            ScenarioExitDestination::CampaignVictoryContinue
        }
        Some(CampaignOutcomeRoute::DefeatPrompt) => ScenarioExitDestination::CampaignDefeat,
        None if cooperative => ScenarioExitDestination::CooperativeResult {
            victory: outcome == crate::sim::house_state::HouseOutcomeKind::Victory,
        },
        None => ScenarioExitDestination::Score {
            title,
            detail,
            model: skirmish_score(),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenarioExitPhase {
    FadeAudio,
    WaitForVoices,
    Done,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct ScenarioExitTick {
    /// The audio master's multiplier over live music, SFX and voices. On abort
    /// the music also carries Theme's own fade, which the Theme owner runs.
    pub master_output_scale: Option<f64>,
    /// One-shot speech/SFX stop and queue clear; Theme keeps its own lifecycle.
    pub clear_sounds: bool,
    /// One-shot audio action after speech/SFX cleanup and master restoration.
    pub after_cleanup: Option<ScenarioExitAudioAction>,
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioExitAudioAction {
    PlayTheme(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioExitVoiceAction {
    InterruptBattleControlTerminated,
}

#[derive(Debug, Clone)]
pub(crate) struct ScenarioExitCascade {
    phase: ScenarioExitPhase,
    started_at_ms: u64,
    voice_wait_started_bucket: Option<u64>,
    start_voice_action: Option<ScenarioExitVoiceAction>,
    destination: Option<ScenarioExitDestination>,
}

impl ScenarioExitCascade {
    pub(crate) fn start(wall_ms: u64, destination: ScenarioExitDestination) -> Self {
        let start_voice_action = matches!(&destination, ScenarioExitDestination::MainMenu)
            .then_some(ScenarioExitVoiceAction::InterruptBattleControlTerminated);
        Self {
            phase: ScenarioExitPhase::FadeAudio,
            started_at_ms: wall_ms,
            voice_wait_started_bucket: None,
            start_voice_action,
            destination: Some(destination),
        }
    }

    /// One-shot voice-channel action owned by the route entry. Native abort
    /// queues this INTERRUPT cue before waiting for the master fade.
    pub(crate) fn take_start_voice_action(&mut self) -> Option<ScenarioExitVoiceAction> {
        self.start_voice_action.take()
    }

    /// Native does not start the explicit EVA queue pump until the master fade
    /// has completed. Audio already playing continues naturally during it.
    pub(crate) fn needs_voice_poll(&self, wall_ms: u64) -> bool {
        self.phase == ScenarioExitPhase::WaitForVoices
            || (self.phase == ScenarioExitPhase::FadeAudio
                && wall_ms.saturating_sub(self.started_at_ms) >= AUDIO_FADE_MS)
    }

    /// gamemd provenance: ordinary offline scenario exit; verified
    /// `0x00685670` and `0x00685DC0` both run master target-zero + wait, then
    /// pump voices for at most 300 16-ms buckets, then clear speech/SFX before the
    /// score dialog. `0x00686570` first starts Theme's own fade
    /// (`ThemeClass::Stop(1)` @ `0x006865FA`, which the route entry hands to
    /// the Theme owner) and then this independent master fade
    /// (`[0x0087E758]` @ `0x00686605`), so abort music receives the product
    /// of both.
    pub(crate) fn tick(&mut self, wall_ms: u64, voices_active: bool) -> ScenarioExitTick {
        let fade_elapsed_ms = wall_ms.saturating_sub(self.started_at_ms);
        match self.phase {
            ScenarioExitPhase::FadeAudio if fade_elapsed_ms < AUDIO_FADE_MS => ScenarioExitTick {
                master_output_scale: Some(
                    (1.0 - fade_elapsed_ms as f64 / AUDIO_FADE_MS as f64).max(0.0),
                ),
                ..Default::default()
            },
            ScenarioExitPhase::FadeAudio => {
                self.phase = ScenarioExitPhase::WaitForVoices;
                // Native takes a fresh timer reading only after its blocking
                // master-fade wait returns. Anchor the 300-bucket ceiling to
                // the frame where this state machine observes that edge.
                self.voice_wait_started_bucket = Some(wall_bucket(wall_ms));
                self.finish_voice_wait_if_due(wall_ms, voices_active)
            }
            ScenarioExitPhase::WaitForVoices => {
                self.finish_voice_wait_if_due(wall_ms, voices_active)
            }
            ScenarioExitPhase::Done => ScenarioExitTick {
                finished: true,
                ..Default::default()
            },
        }
    }

    fn finish_voice_wait_if_due(&mut self, wall_ms: u64, voices_active: bool) -> ScenarioExitTick {
        let voice_wait_started_bucket = self
            .voice_wait_started_bucket
            .expect("voice wait phase has a fresh start timestamp");
        let elapsed_buckets = wall_bucket(wall_ms).saturating_sub(voice_wait_started_bucket);
        if !voices_active || elapsed_buckets >= VOICE_WAIT_BUCKETS {
            self.phase = ScenarioExitPhase::Done;
            ScenarioExitTick {
                master_output_scale: Some(0.0),
                clear_sounds: true,
                after_cleanup: matches!(
                    self.destination.as_ref(),
                    Some(
                        ScenarioExitDestination::Score { .. }
                            | ScenarioExitDestination::CampaignScore { .. }
                    )
                )
                .then_some(ScenarioExitAudioAction::PlayTheme("SCORE")),
                finished: true,
            }
        } else {
            ScenarioExitTick {
                master_output_scale: Some(0.0),
                ..Default::default()
            }
        }
    }

    pub(crate) fn take_destination(&mut self) -> Option<ScenarioExitDestination> {
        (self.phase == ScenarioExitPhase::Done)
            .then(|| self.destination.take())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsi_01_04_outcome_voice_wait_uses_absolute_120_bucket_boundary() {
        let wait =
            ScenarioOutcomeVoiceWait::start(15, crate::sim::house_state::HouseOutcomeKind::Victory);
        assert!(!wait.tick(1_919, true));
        // Native compares absolute timeGetTime()>>4 buckets: bucket 120 starts
        // at 1920ms even though only 1905ms elapsed from the 15ms anchor.
        assert!(wait.tick(1_920, true));
        assert!(wait.tick(16, false));
    }

    fn score_destination() -> ScenarioExitDestination {
        ScenarioExitDestination::Score {
            title: "won".to_string(),
            detail: "done".to_string(),
            model: ScoreScreenModel::default(),
        }
    }

    #[test]
    fn production_destination_keeps_campaign_and_skirmish_models_on_their_owners() {
        use crate::app::match_runtime::session::SessionMode;
        use crate::sim::house_state::HouseOutcomeKind::{Defeat, Victory};

        let campaign_model = crate::ui::campaign_score_shell::CampaignScoreScreenModel {
            side: 0,
            par_time: "00:00:10".into(),
            your_time: "00:00:09".into(),
            enemies_destroyed: "8".into(),
            rank: "fast".into(),
            commendation: "well done".into(),
        };
        let victory = scenario_exit_destination(
            SessionMode::Campaign,
            Victory,
            false,
            false,
            "victory".into(),
            "detail".into(),
            || campaign_model.clone(),
            || panic!("campaign victory must not build multiplayer score"),
        );
        assert!(matches!(
            victory,
            ScenarioExitDestination::CampaignScore { model, .. }
                if model == campaign_model
        ));

        let skipped = scenario_exit_destination(
            SessionMode::Campaign,
            Victory,
            true,
            false,
            "victory".into(),
            "detail".into(),
            || panic!("SkipScore must continue without a score model"),
            || panic!("SkipScore must not build multiplayer score"),
        );
        assert!(matches!(
            skipped,
            ScenarioExitDestination::CampaignVictoryContinue
        ));

        let defeat = scenario_exit_destination(
            SessionMode::Campaign,
            Defeat,
            false,
            false,
            "defeat".into(),
            "detail".into(),
            || panic!("campaign defeat does not build a score model"),
            || panic!("campaign defeat must not build multiplayer score"),
        );
        assert!(matches!(defeat, ScenarioExitDestination::CampaignDefeat));

        let skirmish = scenario_exit_destination(
            SessionMode::Skirmish,
            Victory,
            false,
            false,
            "victory".into(),
            "detail".into(),
            || panic!("skirmish must not build campaign score"),
            || ScoreScreenModel {
                side: 3,
                ..ScoreScreenModel::default()
            },
        );
        assert!(matches!(
            skirmish,
            ScenarioExitDestination::Score { model, .. } if model.side == 3
        ));

        let cooperative = scenario_exit_destination(
            SessionMode::Lan,
            Defeat,
            false,
            true,
            "defeat".into(),
            "detail".into(),
            || panic!("co-op must not build campaign score"),
            || panic!("co-op skips the multiplayer score dialog"),
        );
        assert!(matches!(
            cooperative,
            ScenarioExitDestination::CooperativeResult { victory: false }
        ));
    }

    /// `0x0055CFD0` routes victory/defeat before restart and restart before
    /// EXIT; the taken route clears the others.
    #[test]
    fn termination_router_takes_outcome_then_restart_then_exit() {
        let route = |outcome_ready, restart, exit| {
            route_scenario_termination(TerminationRequests {
                outcome_ready,
                restart,
                exit,
            })
        };
        assert_eq!(route(false, false, false), None);
        assert_eq!(route(true, true, true), Some(TerminationRoute::Outcome));
        assert_eq!(route(true, false, true), Some(TerminationRoute::Outcome));
        assert_eq!(route(false, true, true), Some(TerminationRoute::Restart));
        assert_eq!(route(false, true, false), Some(TerminationRoute::Restart));
        assert_eq!(route(false, false, true), Some(TerminationRoute::Exit));
    }

    #[test]
    fn gsi_01_04_outcome_fade_voice_wait_then_score_theme() {
        let mut exit = ScenarioExitCascade::start(15, score_destination());
        assert_eq!(exit.take_start_voice_action(), None);

        let half = exit.tick(515, false);
        assert_eq!(half.master_output_scale, Some(0.5));
        assert!(!half.clear_sounds && !half.finished);
        assert!(exit.take_destination().is_none());

        let fade_edge = exit.tick(1_015, true);
        assert_eq!(fade_edge.master_output_scale, Some(0.0));
        assert!(!fade_edge.clear_sounds && !fade_edge.finished);
        assert!(exit.take_destination().is_none());

        let voice_done = exit.tick(1_016, false);
        assert!(voice_done.clear_sounds && voice_done.finished);
        assert_eq!(
            voice_done.after_cleanup,
            Some(ScenarioExitAudioAction::PlayTheme("SCORE"))
        );
        assert!(matches!(
            exit.take_destination(),
            Some(ScenarioExitDestination::Score { .. })
        ));
    }

    #[test]
    fn gsi_01_04_trailing_voice_wait_is_bounded_to_300_buckets() {
        let mut exit = ScenarioExitCascade::start(15, ScenarioExitDestination::MainMenu);
        assert_eq!(
            exit.take_start_voice_action(),
            Some(ScenarioExitVoiceAction::InterruptBattleControlTerminated)
        );
        assert_eq!(exit.take_start_voice_action(), None);

        // Theme's own fade is the Theme owner's; the master fade is the
        // same as an outcome's.
        let half = exit.tick(515, true);
        assert_eq!(half.master_output_scale, Some(0.5));
        assert!(!exit.needs_voice_poll(1_014));
        assert!(exit.needs_voice_poll(1_015));

        // Fade completion is observed in absolute bucket 63.
        let fade_edge = exit.tick(1_015, true);
        assert!(!fade_edge.clear_sounds && !fade_edge.finished);

        let before_ceiling = exit.tick(5_807, true); // bucket 362, delta 299
        assert!(!before_ceiling.clear_sounds && !before_ceiling.finished);

        let ceiling = exit.tick(5_808, true); // bucket 363, delta 300
        assert!(ceiling.clear_sounds && ceiling.finished);
        assert_eq!(ceiling.after_cleanup, None);
        assert!(matches!(
            exit.take_destination(),
            Some(ScenarioExitDestination::MainMenu)
        ));
    }

    #[test]
    fn gsi_01_04_voice_ceiling_starts_when_delayed_fade_wait_finishes() {
        let mut exit = ScenarioExitCascade::start(15, score_destination());

        // Delayed observation anchors in absolute bucket 188, not at the
        // nominal 1015-ms fade boundary.
        let late_fade_completion = exit.tick(3_015, true);
        assert!(!late_fade_completion.finished);

        let old_nominal_ceiling = exit.tick(5_815, true);
        assert!(!old_nominal_ceiling.finished);

        let before_fresh_ceiling = exit.tick(7_807, true); // bucket 487
        assert!(!before_fresh_ceiling.finished);

        let fresh_ceiling = exit.tick(7_808, true); // bucket 488
        assert!(fresh_ceiling.clear_sounds && fresh_ceiling.finished);
    }
}
