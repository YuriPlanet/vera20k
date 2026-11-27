//! Phobos scenario-start briefing dialog admission.
//!
//! `DEFINE_HOOK(0x683E41, ScenarioClass_Start_ShowBriefing, 0x6)`
//! (`src/Misc/Hooks.UI.cpp:345-373`) arms `BriefingTemp::ShowBriefing` only
//! when all of these hold: `Phobos::Config::ShowBriefing` (`[Phobos]`),
//! `ScenarioExt::Global()->ShowBriefing` (`[Basic]` / the scenario's own
//! MISSIONMD section), the `showBriefing` argument of `ScenarioClass::Start`
//! and `SessionClass::IsCampaign()`. The dialog is then opened by
//! `MainGame_ShowBriefing @ 0x48CE85` / `AuxLoop_ShowBriefing @ 0x55D14F`.
//!
//! VERA installs the world in `install_world_presentation` and enters the
//! game loop at `Main_Game 0x0048CE7E`; that is where this admission runs.
//! The caller's `showBriefing` argument has no VERA equivalent (a loaded
//! save does not pass it), so it is treated as `true` — see the residuals in
//! `docs/phobos-show-briefing.md`. `BriefingTheme` queuing is likewise not
//! ported; `audio_runtime::bind_match_theme_context` owns the map theme.

use crate::app::AppState;
use crate::sim::scenario_session::ScenarioGameMode;

/// The three modelled gates of `0x683E41`. The scenario flag is an
/// `Option<bool>` because `Ext/Scenario/Body.cpp:167` reads with the retained
/// constructor value (`Body.h:62`, false) as its default: an absent key does
/// not arm the dialog. The fourth native gate (the caller's `showBriefing`)
/// has no VERA owner and is taken as admitted.
pub(crate) fn admits(
    client_show_briefing: bool,
    scenario_show_briefing: Option<bool>,
    mode: ScenarioGameMode,
) -> bool {
    client_show_briefing
        && scenario_show_briefing.unwrap_or(false)
        && mode == ScenarioGameMode::Campaign
}

/// Open the 0x10C briefing dialog for a scenario start and mark it as the
/// Phobos path, so the renderer uses the authored resume/status labels.
pub(crate) fn admit_scenario_start_briefing(state: &mut AppState) {
    let client = state.match_state.match_presentation.show_briefing;
    let scenario = state.match_state.map_basic.show_briefing;
    let mode = state.match_state.session_mode();
    if !admits(client, scenario, mode) {
        return;
    }
    // `restate::open` re-reads MISSIONMD.INI and opens nothing when the file
    // or the scenario section is missing, exactly as
    // `ScenarioClass::ShowMissionRestateBriefing @ 0x0065F520` does.
    crate::app::input::restate::open(
        &mut state.platform,
        &state.renderer,
        &mut state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &mut state.audio,
        &mut state.persistence,
    );
    if state.match_state.match_presentation.in_game_menu
        == crate::ui::pause_menu::InGameMenuState::MissionBriefing
    {
        // `open` clears this flag for the stock F2 / sidebar-gadget path;
        // re-arm it only when the dialog really opened.
        state.match_state.match_presentation.briefing_at_start = true;
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn the_modelled_gates_admit_only_a_campaign_with_both_switches_on() {
        assert!(admits(true, Some(true), ScenarioGameMode::Campaign));
        assert!(
            !admits(false, Some(true), ScenarioGameMode::Campaign),
            "the [Phobos] client switch is off"
        );
        assert!(
            !admits(true, None, ScenarioGameMode::Campaign),
            "an absent [Basic] key keeps the false constructor"
        );
        assert!(
            !admits(true, Some(false), ScenarioGameMode::Campaign),
            "an explicit [Basic] no"
        );
        assert!(
            !admits(true, Some(true), ScenarioGameMode::Skirmish),
            "IsCampaign is false"
        );
        assert!(!admits(true, Some(true), ScenarioGameMode::Lan));
        assert!(!admits(true, Some(true), ScenarioGameMode::Other));
    }
}
