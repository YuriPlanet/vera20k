//! Presentation and transport of a completed authored campaign stage.
//! MatchSession owns the immutable options, vote, and handoff.

use crate::app::frontend::state::ShellMessageBoxPurpose;
use crate::app::lan_service::CampaignControlSend;
use crate::app::shell_network_setup::campaign_room;
use crate::app::{App, AppState, GameScreen};
use crate::net::global::GlobalMessage;
use crate::net::lan_messages::extended_setup::campaign::{Signal, StageChoice};
use crate::net::live_commands::LiveCommandSession;
use crate::sim::house_state::HouseOutcomeKind;
use crate::ui::shell::modal::ShellBox;

use super::session::{CampaignResult, NetworkReturnContext, StageHandoff, StageProgress};

pub(crate) fn open(state: &mut AppState, outcome: HouseOutcomeKind) {
    if state.match_state.session.local_connection_watches() {
        App::return_to_main_menu(state);
        return;
    }
    let captured = capture(
        state.platform.game_config.as_ref(),
        &state.match_state,
        outcome,
    )
    .and_then(|result| state.match_state.session.open_campaign_result(result));
    if let Err(error) = captured {
        log::warn!("Cannot open campaign stage result: {error:#}");
        abort(state, Some("VERA:CampaignStageUnavailable"));
        return;
    }
    super::sim_tick::flush_replay_log(state);
    App::destroy_current_shell_dialog(&mut state.frontend);
    state.frontend.screen = GameScreen::MainMenu;
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
    let initial = usize::from(
        outcome == HouseOutcomeKind::Victory
            && state
                .match_state
                .session
                .campaign_result()
                .is_some_and(|result| result.plans().len() > 1),
    );
    present(
        &state.platform.window,
        &state.renderer,
        &mut state.frontend,
        &state.match_state.session,
        &state.process_assets,
        &state.persistence.options_profile,
        initial,
    );
}

fn capture(
    game_config: Option<&crate::util::config::GameConfig>,
    match_state: &crate::app::match_runtime::state::MatchState,
    outcome: HouseOutcomeKind,
) -> anyhow::Result<CampaignResult> {
    use anyhow::Context;
    let session = &match_state.session;
    let room = session
        .campaign_room()
        .context("campaign room disappeared")?;
    let manifest = campaign_room::load_manifest(game_config, room)?;
    let sim = &match_state
        .sim_runtime
        .runtime()
        .context("finished campaign world disappeared")?
        .simulation;
    CampaignResult::capture(
        &manifest,
        room,
        sim,
        outcome,
        super::frame_pacer::wall_clock_ms(),
        session.network_connected_source_indices(),
    )
}

fn present(
    window: &winit::window::Window,
    renderer: &crate::app::renderer_state::RendererState,
    frontend: &mut crate::app::frontend::state::FrontendState,
    session: &super::session::MatchSession,
    process_assets: &crate::app::process_assets::ProcessAssets,
    options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    index: usize,
) {
    let Some(result) = session.campaign_result() else {
        return;
    };
    let Some(plan) = result.plans().get(index) else {
        return;
    };
    let title = super::scenario_exit::outcome_title(result.outcome());
    let tr = crate::i18n::tr;
    let shell_box = ShellBox::message(
        format!("{title}\n{}", plan.title),
        tr(match plan.choice {
            StageChoice::Retry => "VERA:CampaignRetryStage",
            StageChoice::Route(_) => "VERA:CampaignContinueStage",
        }),
        tr("VERA:CampaignLeaveStage"),
        if result.plans().len() > 1 {
            tr("VERA:CampaignOtherRoute")
        } else {
            String::new()
        },
    );
    App::open_shell_message_box(
        window,
        renderer,
        frontend,
        process_assets,
        options_profile,
        shell_box,
        ShellMessageBoxPurpose::AuthoredCampaignResult(Some(index)),
    );
}

/// The stage box's answer: 0 its choice, 2 the other route, 1 (Leave, Enter
/// or Escape) leaves the campaign; the waiting box's only button leaves too.
pub(crate) fn answer(state: &mut AppState, index: Option<usize>, response: usize) {
    let Some(result) = state.match_state.session.campaign_result() else {
        return;
    };
    let tr = crate::i18n::tr;
    match (index, response) {
        (Some(index), 2) => {
            present(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.match_state.session,
                &state.process_assets,
                &state.persistence.options_profile,
                (index + 1) % result.plans().len(),
            );
        }
        (Some(index), 0) => {
            let Some(plan) = result.plans().get(index) else {
                return;
            };
            state
                .match_state
                .session
                .vote_for_campaign_stage(plan.choice);
            let shell_box = ShellBox::message(
                tr("VERA:CampaignWaitingForVotes"),
                tr("VERA:CampaignLeaveStage"),
                String::new(),
                String::new(),
            );
            App::open_shell_message_box(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                shell_box,
                ShellMessageBoxPurpose::AuthoredCampaignResult(None),
            );
        }
        _ => abort(state, None),
    }
}

pub(crate) fn receive(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    source: usize,
    signal: Signal,
    receipt: Option<GlobalMessage>,
) {
    let from_host = crate::app::lan_service::campaign_host_source(match_state) == Some(source);
    let relay_retry_delta = relay_retry_delta(
        match_state.session.network_return_context(),
        match_state.live_network.as_ref(),
        signal,
    );
    match_state.session.receive_campaign_stage_signal(
        source,
        from_host,
        signal,
        receipt,
        super::frame_pacer::wall_clock_ms(),
        relay_retry_delta,
    );
}

fn relay_retry_delta(
    context: Option<NetworkReturnContext>,
    live: Option<&LiveCommandSession>,
    signal: Signal,
) -> Option<u32> {
    if context == Some(NetworkReturnContext::Wol)
        && matches!(signal, Signal::StageBegin(_) | Signal::StageBegun(_))
    {
        live.map(|live| live.global().retry_delta())
    } else {
        None
    }
}

pub(crate) fn poll(state: &mut AppState, now_ms: u64) {
    if state.match_state.session.campaign_result().is_none() {
        return;
    }
    if let Some(live) = state.match_state.live_network.as_ref() {
        state
            .match_state
            .session
            .confirm_campaign_stage_delivery(|batch| live.global().batch_delivered(batch));
    }
    let session = &state.match_state.session;
    let Some(result) = session.campaign_result() else {
        return;
    };
    match result.progress(session.network_connected_source_indices(), now_ms) {
        StageProgress::Commit => {
            commit(state, now_ms);
            return;
        }
        StageProgress::Abort => {
            abort(state, Some("VERA:CampaignStageVoteEnded"));
            return;
        }
        StageProgress::Waiting => {}
    }
    if let Some(plan) = result.plan_to_prepare().cloned() {
        let result = campaign_room::prepare_stage(
            state.platform.game_config.as_ref(),
            &state.match_state.session,
            &state.process_assets,
            &plan.stage,
            plan.carry,
        )
        .and_then(|room| {
            let start = campaign_room::prepare_current(
                state.platform.game_config.as_ref(),
                &state.match_state.session,
                &state.process_assets,
                &room,
            )?;
            state
                .match_state
                .session
                .stage_prepared(room, start, now_ms)
        });
        if let Err(error) = result {
            log::warn!("Cannot admit the next campaign stage: {error:#}");
            abort(state, Some("VERA:CampaignStageUnavailable"));
            return;
        }
    }
    let session = &state.match_state.session;
    let Some(local) = super::network_match::local_source(session) else {
        return;
    };
    let Some(signal) = session
        .campaign_result()
        .and_then(|result| result.outgoing(session.host(), local, now_ms))
    else {
        return;
    };
    let peers = recipients(&state.match_state, local);
    if !session.host() && peers.is_empty() {
        return;
    }
    if let Some(CampaignControlSend::Queued(batch)) =
        crate::app::lan_service::send_campaign_control(&mut state.match_state, &peers, signal)
    {
        state
            .match_state
            .session
            .confirm_campaign_stage_send(signal, now_ms, batch);
    }
}

fn recipients(
    match_state: &crate::app::match_runtime::state::MatchState,
    local: usize,
) -> Vec<usize> {
    let session = &match_state.session;
    if session.host() {
        session
            .network_connected_source_indices()
            .iter()
            .copied()
            .filter(|&source| source != local)
            .collect()
    } else {
        crate::app::lan_service::campaign_host_source(match_state)
            .into_iter()
            .collect()
    }
}

fn abort(state: &mut AppState, notice: Option<&str>) {
    if let Some(local) = super::network_match::local_source(&state.match_state.session) {
        let peers = recipients(&state.match_state, local);
        let _ = crate::app::lan_service::send_campaign_control(
            &mut state.match_state,
            &peers,
            Signal::StageAbort,
        );
    }
    App::close_shell_message_box_from_controller(&mut state.frontend);
    App::return_to_main_menu(state);
    if let Some(notice) = notice {
        App::open_shell_notice(
            &state.platform.window,
            &state.renderer,
            &mut state.frontend,
            &state.process_assets,
            &state.persistence.options_profile,
            crate::i18n::tr(notice),
        );
    }
}

fn commit(state: &mut AppState, now_ms: u64) {
    if state.match_state.session.network_return_context() == Some(NetworkReturnContext::LanLobby) {
        let receipts = state
            .match_state
            .session
            .take_campaign_stage_receipts(now_ms);
        if let Err(error) =
            crate::app::lan_service::advance_campaign_transport(&mut state.match_state, receipts)
        {
            log::warn!("Cannot advance LAN campaign transport: {error:#}");
            abort(state, Some("VERA:CampaignStageUnavailable"));
            return;
        }
    }
    match state.match_state.session.commit_campaign_stage(now_ms) {
        Ok(StageHandoff::Lan(startup)) => {
            App::close_shell_message_box_from_controller(&mut state.frontend);
            App::retire_finished_scenario(state);
            crate::app::loading::pump::begin_loading(
                state,
                crate::app::loading::pump::LoadingRequest::network(startup),
            );
        }
        Ok(StageHandoff::Wol) => {
            App::close_shell_message_box_from_controller(&mut state.frontend);
            App::retire_finished_scenario(state);
            App::resume_shell_after_match(state);
        }
        Err(error) => {
            log::warn!("Cannot complete campaign stage handoff: {error:#}");
            abort(state, Some("VERA:CampaignStageUnavailable"));
        }
    }
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;
    use crate::net::live_commands::{LiveCommandMode, StartupAgreement};
    use crate::net::reliable::{LanConnectionConfig, LanConnections, RetryTiming};
    use crate::net::udp::{LanUdpTransport, UdpBindConfig};
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::num::NonZeroU32;

    #[test]
    fn unsequenced_wol_stage_receipts_still_have_relay_retry_timing() {
        let timing = RetryTiming::LAN_GUEST_COMPLETED_GLOBAL;
        let bind = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let transport = LanUdpTransport::bind(
            UdpBindConfig {
                primary: bind,
                alternate: bind,
            },
            LanConnections::new(LanConnectionConfig::default(), timing),
        )
        .unwrap();
        let rate = NonZeroU32::new(1).unwrap();
        let agreement =
            StartupAgreement::new([(0, 0)], [0], 1, 10, 30, rate, 0, 0, "", "").unwrap();
        let live =
            LiveCommandSession::new(agreement, transport, 0, 1, rate, LiveCommandMode::Wol, None)
                .unwrap();
        assert!(!live.uses_sequenced_lockstep());

        for signal in [
            Signal::StageBegin(StageChoice::Retry),
            Signal::StageBegun(StageChoice::Retry),
        ] {
            assert_eq!(
                relay_retry_delta(Some(NetworkReturnContext::Wol), Some(&live), signal),
                Some(timing.retry_delta)
            );
            assert_eq!(
                relay_retry_delta(Some(NetworkReturnContext::LanLobby), Some(&live), signal),
                None
            );
        }
    }
}
