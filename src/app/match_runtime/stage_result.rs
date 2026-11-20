//! Presentation and transport of a completed authored campaign stage.
//! MatchSession owns the immutable options, vote, and handoff.

use crate::app::frontend::state::{ShellMessageBox, ShellMessageBoxPurpose};
use crate::app::shell_network_setup::campaign_room;
use crate::app::{App, AppState, GameScreen};
use crate::net::global::GlobalMessage;
use crate::net::lan_messages::extended_setup::campaign::{Signal, StageChoice};
use crate::sim::house_state::HouseOutcomeKind;
use crate::ui::main_menu_dialogs::ShellMessageBoxLabels;
use crate::ui::shell::modal::ModalResult;

use super::session::{CampaignResult, StageHandoff, StageProgress};

pub(crate) fn open(state: &mut AppState, outcome: HouseOutcomeKind) {
    if state.match_state.session.local_connection_watches() {
        App::return_to_main_menu(state);
        return;
    }
    let captured = capture(state, outcome)
        .and_then(|result| state.match_state.session.open_campaign_result(result));
    if let Err(error) = captured {
        log::warn!("Cannot open campaign stage result: {error:#}");
        abort(state, Some("VERA:CampaignStageUnavailable"));
        return;
    }
    super::sim_tick::flush_replay_log(state);
    App::destroy_current_shell_dialog(state);
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
    present(state, initial);
}

fn capture(state: &AppState, outcome: HouseOutcomeKind) -> anyhow::Result<CampaignResult> {
    use anyhow::Context;
    let session = &state.match_state.session;
    let room = session
        .campaign_room()
        .context("campaign room disappeared")?;
    let manifest = campaign_room::load_manifest(state, room)?;
    let sim = &state
        .match_state
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

fn present(state: &mut AppState, index: usize) {
    let Some(result) = state.match_state.session.campaign_result() else {
        return;
    };
    let Some(plan) = result.plans().get(index) else {
        return;
    };
    let title = super::scenario_exit::outcome_title(result.outcome());
    let tr = crate::i18n::tr;
    App::open_shell_message_box(
        state,
        ShellMessageBox {
            labels: ShellMessageBoxLabels {
                title: format!("{title}\n{}", plan.title),
                confirm: tr(match plan.choice {
                    StageChoice::Retry => "VERA:CampaignRetryStage",
                    StageChoice::Route(_) => "VERA:CampaignContinueStage",
                }),
                third: if result.plans().len() > 1 {
                    tr("VERA:CampaignOtherRoute")
                } else {
                    String::new()
                },
                cancel: tr("VERA:CampaignLeaveStage"),
            },
            purpose: ShellMessageBoxPurpose::AuthoredCampaignResult(Some(index)),
        },
    );
}

pub(crate) fn answer(state: &mut AppState, index: Option<usize>, response: ModalResult) {
    let Some(result) = state.match_state.session.campaign_result() else {
        return;
    };
    let tr = crate::i18n::tr;
    match (index, response) {
        (Some(index), ModalResult::MessageBox(2)) => {
            present(state, (index + 1) % result.plans().len());
        }
        (Some(index), ModalResult::MessageBox(0)) => {
            let Some(plan) = result.plans().get(index) else {
                return;
            };
            state
                .match_state
                .session
                .vote_for_campaign_stage(plan.choice);
            App::open_shell_message_box(
                state,
                ShellMessageBox {
                    labels: ShellMessageBoxLabels {
                        title: tr("VERA:CampaignWaitingForVotes"),
                        confirm: tr("VERA:CampaignLeaveStage"),
                        third: String::new(),
                        cancel: String::new(),
                    },
                    purpose: ShellMessageBoxPurpose::AuthoredCampaignResult(None),
                },
            );
        }
        _ => abort(state, None),
    }
}

pub(crate) fn receive(
    state: &mut AppState,
    source: usize,
    signal: Signal,
    receipt: Option<GlobalMessage>,
) {
    let from_host = crate::app::lan_service::campaign_host_source(state) == Some(source);
    state
        .match_state
        .session
        .receive_campaign_stage_signal(source, from_host, signal, receipt);
}

pub(crate) fn poll(state: &mut AppState, now_ms: u64) {
    if state.match_state.session.campaign_result().is_none() {
        return;
    }
    if state.match_state.live_network.as_ref().is_some_and(|live| {
        live.global().queued_send_count() == 0
            && live
                .service_report()
                .is_none_or(|report| !report.delivery_failed())
    }) {
        state.match_state.session.confirm_campaign_stage_delivery();
    }
    let session = &state.match_state.session;
    let Some(result) = session.campaign_result() else {
        return;
    };
    match result.progress(session.network_connected_source_indices(), now_ms) {
        StageProgress::Commit => {
            commit(state);
            return;
        }
        StageProgress::Abort => {
            abort(state, Some("VERA:CampaignStageVoteEnded"));
            return;
        }
        StageProgress::Waiting => {}
    }
    if let Some(plan) = result.plan_to_prepare().cloned() {
        let result =
            campaign_room::prepare_stage(state, &plan.stage, plan.carry).and_then(|room| {
                let start = campaign_room::prepare_current(state, &room)?;
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
    let peers = recipients(state, local);
    if !session.host() && peers.is_empty() {
        return;
    }
    if crate::app::lan_service::send_campaign_control(state, &peers, signal) {
        state
            .match_state
            .session
            .confirm_campaign_stage_send(signal, now_ms);
    }
}

fn recipients(state: &AppState, local: usize) -> Vec<usize> {
    let session = &state.match_state.session;
    if session.host() {
        session
            .network_connected_source_indices()
            .iter()
            .copied()
            .filter(|&source| source != local)
            .collect()
    } else {
        crate::app::lan_service::campaign_host_source(state)
            .into_iter()
            .collect()
    }
}

fn abort(state: &mut AppState, notice: Option<&str>) {
    if let Some(local) = super::network_match::local_source(&state.match_state.session) {
        let peers = recipients(state, local);
        crate::app::lan_service::send_campaign_control(state, &peers, Signal::StageAbort);
    }
    App::close_shell_message_box_from_controller(state);
    App::return_to_main_menu(state);
    if let Some(notice) = notice {
        App::open_shell_notice(state, crate::i18n::tr(notice));
    }
}

fn commit(state: &mut AppState) {
    if state.match_state.session.network_return_context()
        == Some(super::session::NetworkReturnContext::LanLobby)
    {
        let receipts = state.match_state.session.take_campaign_stage_receipts();
        if let Err(error) = crate::app::lan_service::advance_campaign_transport(state, receipts) {
            log::warn!("Cannot advance LAN campaign transport: {error:#}");
            abort(state, Some("VERA:CampaignStageUnavailable"));
            return;
        }
    }
    match state.match_state.session.commit_campaign_stage() {
        Ok(StageHandoff::Lan(startup)) => {
            App::close_shell_message_box_from_controller(state);
            App::retire_finished_scenario(state);
            crate::app::loading::pump::begin_loading(
                state,
                crate::app::loading::pump::LoadingRequest::network(startup),
            );
        }
        Ok(StageHandoff::Wol) => {
            App::close_shell_message_box_from_controller(state);
            App::retire_finished_scenario(state);
            App::resume_shell_after_match(state);
        }
        Err(error) => {
            log::warn!("Cannot complete campaign stage handoff: {error:#}");
            abort(state, Some("VERA:CampaignStageUnavailable"));
        }
    }
}
