//! Observes real external peer disappearance or checksum corruption after the
//! shared 300-frame prefix. Quit/Stop enter the ordinary dialog input owner;
//! time, packets, commands and simulation advancement stay with the game.

use super::*;

mod startup;
use crate::app::match_runtime::network_match;
use crate::net::live_commands::{LiveWaitInfo, LiveWaitKind};
use crate::sim::command::{Command, CommandRecord, EXIT_OPCODE, REMOVE_PLAYER_OPCODE};
use crate::sim::rng::SimRngLogicalState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Timeout,
    Quit,
    Desync,
    Startup,
}

struct HeldWait {
    info: LiveWaitInfo,
    frame: u32,
    tick: u64,
    rng: SimRngLogicalState,
    dialog_period: u32,
    timeout_period: u32,
}

impl HeldWait {
    fn new(info: LiveWaitInfo, sim: &crate::sim::world::Simulation) -> Self {
        let (dialog_period, timeout_period) = network_match::live_wait_periods(info);
        Self {
            info,
            frame: sim.session().binary_frame(),
            tick: sim.session().tick(),
            rng: sim.clone_scenario_rng().logical_state(),
            dialog_period,
            timeout_period,
        }
    }
}

struct HeldChecksum {
    mismatch: crate::sim::command_queue::ChecksumMismatch,
    frame: u32,
    tick: u64,
    rng: SimRngLogicalState,
    first_render: u32,
    pressed_render: Option<u32>,
    released_render: Option<u32>,
}

pub(super) struct FailureCapture {
    mode: Mode,
    peer_house: Option<i32>,
    peer_source: Option<usize>,
    peer_name: String,
    sent_before: u64,
    replay_cursor: usize,
    remove_admissions: usize,
    wait: Option<HeldWait>,
    dialog: Option<Value>,
    removed: Option<Value>,
    removal_frame: Option<u32>,
    remove_player: Option<CommandRecord>,
    remove_player_frame: Option<u32>,
    death: Option<Value>,
    quit: Option<Value>,
    returned: Option<Value>,
    last_live: Option<Value>,
    checksum: Option<HeldChecksum>,
    startup: Option<startup::StartupCapture>,
}

impl FailureCapture {
    pub(super) fn new(mode: Mode) -> Self {
        Self {
            mode,
            peer_house: None,
            peer_source: None,
            peer_name: String::new(),
            sent_before: 0,
            replay_cursor: 0,
            remove_admissions: 0,
            wait: None,
            dialog: None,
            removed: None,
            removal_frame: None,
            remove_player: None,
            remove_player_frame: None,
            death: None,
            quit: None,
            returned: None,
            last_live: None,
            checksum: None,
            startup: None,
        }
    }

    pub(super) fn armed(&self) -> bool {
        self.peer_house.is_some()
    }

    pub(super) fn arm(&mut self, state: &AppState) -> Result<()> {
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("live failure transport")?;
        ensure!(live.peers().len() == 1, "failure prefix lost its peer");
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("failure prefix world")?
            .simulation;
        let house = i32::from(live.peers()[0].house_id());
        let source = network_match::house_source(&state.match_state.session, sim, house)
            .context("failure peer source")?;
        let owner = sim.houses().order()[house as usize];
        self.peer_name = crate::app::presentation::house_ui::house_ui_name(state, owner)
            .context("failure peer UI name")?;
        self.peer_house = Some(house);
        self.peer_source = Some(source);
        self.sent_before = live.sent_commands();
        self.replay_cursor = state
            .match_state
            .match_diagnostics
            .replay_log
            .as_ref()
            .context("failure capture requires production command recording")?
            .ticks
            .len();
        Ok(())
    }

    pub(super) fn expects_checksum(&self) -> bool {
        self.mode == Mode::Desync && self.armed()
    }

    pub(super) fn returned(&self) -> bool {
        self.returned.is_some()
    }

    pub(super) fn step(
        &mut self,
        state: &mut AppState,
        render: u32,
        route: &mut Vec<Value>,
    ) -> Result<bool> {
        if self.mode == Mode::Startup {
            return self.observe_startup(state, render, route);
        }
        if self.mode == Mode::Desync {
            return self.observe_checksum(state, render, route);
        }
        let now = (crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32) >> 4;
        if self.quit.is_some() {
            return self.observe_return(state, render, route);
        }
        self.observe_commands(state)?;
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("failure world")?
            .simulation;
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("failure live transport")?;
        let frame = sim.session().binary_frame();
        let private_peers = live.connections().peers().collect::<Vec<_>>();
        let (global_sends, global_receives) = {
            let global = live.global();
            (global.queued_send_count(), global.queued_receive_count())
        };
        self.last_live = Some(json!({"frame":frame,"sent_commands":live.sent_commands(),
            "private_peers":private_peers.into_iter().map(|(id,endpoint)|json!({"house":id,"endpoint":endpoint.to_string()})).collect::<Vec<_>>(),
            "progress_peers":live.peers().len(),"global_sends":global_sends,
            "global_receives":global_receives}));

        if live.peers().is_empty() {
            return self.observe_recovery(state, render, now, route);
        }
        ensure!(
            state.frontend.screen == GameScreen::InGame,
            "failure survivor left gameplay before peer removal"
        );
        ensure!(
            live.peers().len() == 1 && self.removed.is_none(),
            "failure peer membership changed twice"
        );
        let Some(info) = live.wait_info() else {
            if self.wait.take().is_some() {
                ensure!(
                    self.dialog.is_none(),
                    "peer resumed after its failure dialog opened"
                );
                route.push(json!({"render_frame":render,"native_frame":frame,"action":"transient wait finished before peer disappearance"}));
            }
            return Ok(false);
        };
        ensure!(
            info.kind == LiveWaitKind::Ordinary,
            "failure run returned to initial barrier"
        );
        if self
            .wait
            .as_ref()
            .is_none_or(|wait| wait.info.entered_at != info.entered_at)
        {
            ensure!(self.dialog.is_none(), "confirmed wait entry changed");
            // Borrow the live wait owner's LAN/WOL row and sampled response.
            // This observer never supplies time or changes either deadline.
            let wait = HeldWait::new(info, sim);
            let (period, timeout_period) = (wait.dialog_period, wait.timeout_period);
            self.wait = Some(wait);
            route.push(json!({"render_frame":render,"native_frame":frame,"action":"ordinary wait reached",
                "entry_bucket":info.entered_at,"observed_bucket":now,"response":info.response_time_at_entry,
                "dialog_period":period,"timeout_period":timeout_period,"mode":format!("{:?}",info.mode)}));
        }
        let wait = self.wait.as_ref().context("observed wait")?;
        ensure!(
            sim.pending_network_wait(),
            "live wait has no held simulation continuation"
        );
        ensure!(
            frame == wait.frame && sim.session().tick() == wait.tick,
            "held frame committed while waiting"
        );
        ensure!(
            sim.clone_scenario_rng().logical_state() == wait.rng,
            "held frame reran simulation RNG"
        );
        ensure!(
            state
                .match_state
                .session
                .network_connected_source_indices()
                .contains(&self.peer_source.context("peer source")?),
            "peer removed before live progress"
        );
        let elapsed = now.wrapping_sub(info.entered_at);
        if state.match_state.network_wait.page().is_some() && self.dialog.is_none() {
            ensure!(
                elapsed >= wait.dialog_period,
                "EA dialog opened before native deadline"
            );
            let page = state.match_state.network_wait.page().context("EA")?;
            self.dialog = Some(json!({"entry_bucket":info.entered_at,"observed_bucket":now,
                "elapsed_buckets":elapsed,"held_frame":frame,"rows":page.rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
                "row_sources":page.rows.iter().map(|row| row.source_index).collect::<Vec<_>>(),"countdown":page.countdown}));
            route.push(json!({"render_frame":render,"native_frame":frame,"action":"real EA dialog opened","elapsed_buckets":elapsed}));
            if self.mode == Mode::Quit {
                let layout =
                    crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
                        state,
                    )
                    .map(|shell| {
                        crate::ui::shell::connection::connection_layout(
                            shell.width,
                            shell.height,
                            shell.layout,
                            shell.button_size,
                        )
                    })
                    .context("EA Quit hit geometry")?;
                let point = set_window_pointer_to_rect_center(state, layout.quit);
                crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                for edge in [
                    winit::event::ElementState::Pressed,
                    winit::event::ElementState::Released,
                ] {
                    crate::app::input::dispatch::handle_mouse_input(state, MouseButton::Left, edge);
                }
                self.quit = Some(json!({"held_frame":frame,"observed_bucket":now,"point":point}));
                route.push(json!({"render_frame":render,"native_frame":frame,"action":"real EA Quit Game clicked","point":point}));
            }
        } else if elapsed < wait.dialog_period {
            ensure!(
                state.match_state.network_wait.page().is_none(),
                "EA visible before dialog period"
            );
        }
        Ok(false)
    }

    fn observe_commands(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(log) = state.match_state.match_diagnostics.replay_log.as_ref() {
            ensure!(
                log.ticks.len() >= self.replay_cursor,
                "failure replay prefix moved backwards"
            );
            for tick in &log.ticks[self.replay_cursor..] {
                for event in &tick.commands {
                    if let Command::SynchronizedRecord { record } = &event.payload {
                        ensure!(
                            record.opcode() != EXIT_OPCODE,
                            "failure route admitted EXIT"
                        );
                        if record.opcode() == REMOVE_PLAYER_OPCODE {
                            let target =
                                i32::from_le_bytes(record.payload()[..4].try_into().unwrap());
                            ensure!(
                                self.mode == Mode::Timeout && Some(target) == self.peer_house,
                                "failure route admitted an unexpected REMOVEPLAYER"
                            );
                            self.remove_admissions += 1;
                            ensure!(
                                self.remove_admissions == 1,
                                "failure route admitted duplicate REMOVEPLAYER"
                            );
                        }
                    }
                }
            }
            self.replay_cursor = log.ticks.len();
        } else {
            ensure!(
                self.remove_admissions == 1,
                "failure capture lost command recording before REMOVEPLAYER"
            );
        }
        let sim = &mut state
            .match_state
            .sim_runtime
            .as_mut()
            .context("observed world retained")?
            .simulation;
        ensure!(!sim.quit_requested(), "failure route executed EXIT");
        // This diagnostic snapshot decodes native records. Use it only to
        // inspect the pending effect, never to identify its wire carrier.
        for event in sim.pending_command_snapshot() {
            match event.payload {
                Command::ExitMatch => anyhow::bail!("failure route queued semantic EXIT"),
                Command::RemovePlayer { house_index } => {
                    ensure!(self.mode == Mode::Timeout, "EA Quit queued REMOVEPLAYER");
                    ensure!(
                        Some(house_index) == self.peer_house,
                        "REMOVEPLAYER selected another House"
                    );
                }
                Command::SynchronizedRecord { record } if record.opcode() == EXIT_OPCODE => {
                    anyhow::bail!("failure route queued raw EXIT")
                }
                _ => {}
            }
        }
        // Inspect the actual OutList prefix before its next send boundary.
        // Taking this scoped view does not drain or mutate either queue.
        let frame = sim.session().binary_frame();
        let queues = sim.network_command_queues();
        let mut remove_count = 0;
        for record in queues.local_records()? {
            ensure!(
                record.opcode() != EXIT_OPCODE,
                "failure route queued raw EXIT"
            );
            if record.opcode() == REMOVE_PLAYER_OPCODE {
                ensure!(self.mode == Mode::Timeout, "EA Quit queued REMOVEPLAYER");
                let house = i32::from_le_bytes(record.payload()[..4].try_into().unwrap());
                ensure!(
                    Some(house) == self.peer_house,
                    "REMOVEPLAYER selected another House"
                );
                remove_count += 1;
                if self.remove_player.is_none() {
                    self.remove_player = Some(record.clone());
                    self.remove_player_frame = Some(frame);
                }
            }
        }
        ensure!(remove_count <= 1, "duplicate raw REMOVEPLAYER pending");
        Ok(())
    }

    fn observe_recovery(
        &mut self,
        state: &AppState,
        render: u32,
        now: u32,
        route: &mut Vec<Value>,
    ) -> Result<bool> {
        ensure!(
            self.mode == Mode::Timeout,
            "Quit target lost its peer before input"
        );
        let wait = self
            .wait
            .as_ref()
            .context("peer vanished without an observed ordinary wait")?;
        ensure!(self.dialog.is_some(), "timeout removed peer without EA");
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("survivor world")?
            .simulation;
        let frame = sim.session().binary_frame();
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("survivor transport")?;
        ensure!(
            !state
                .match_state
                .session
                .network_connected_source_indices()
                .contains(&self.peer_source.context("peer source")?),
            "removed peer remains connected"
        );
        ensure!(
            live.connections().peers().len() == 0,
            "removed peer retains private connection"
        );
        let lost_line = crate::assets::csf_file::format_csf(
            &App::csf_label(state, "TXT_CONNECTION_LOST", "TXT_CONNECTION_LOST"),
            &[crate::assets::csf_file::CsfArg::Str(&self.peer_name)],
        );
        let lost_count = state
            .match_state
            .match_presentation
            .message_list
            .messages()
            .iter()
            .filter(|message| message.text == lost_line)
            .count();
        if self.removed.is_none() {
            ensure!(
                now.wrapping_sub(wait.info.entered_at) >= wait.timeout_period,
                "peer removed before native final deadline"
            );
            ensure!(
                lost_count == 1,
                "timeout did not post exactly one Lost line"
            );
            self.removal_frame = Some(frame);
            self.removed = Some(
                json!({"observed_bucket":now,"elapsed_buckets":now.wrapping_sub(wait.info.entered_at),
                "native_frame":frame,"message":lost_line,"connected_sources":state.match_state.session.network_connected_source_indices()}),
            );
            route.push(json!({"render_frame":render,"native_frame":frame,"action":"real timeout removed peer","message":lost_line}));
        } else {
            ensure!(lost_count <= 1, "timeout posted a duplicate Lost line");
        }
        let owner = sim.houses().order()[self.peer_house.context("peer House")? as usize];
        let house = &sim.houses()[&owner];
        let alive = sim
            .entities()
            .values_sorted()
            .filter(|entity| entity.owner() == owner && entity.is_alive())
            .count();
        if self.death.is_none() && house.is_defeated() && alive == 0 {
            ensure!(
                self.remove_player.is_some(),
                "House died without observed raw REMOVEPLAYER"
            );
            ensure!(
                frame
                    > self
                        .remove_player_frame
                        .context("REMOVEPLAYER observation frame")?,
                "House died without delayed REMOVEPLAYER execution"
            );
            self.death = Some(
                json!({"native_frame":frame,"defeated":true,"alive_objects":alive,"to_die":house.is_to_die()}),
            );
            route.push(json!({"render_frame":render,"native_frame":frame,"action":"ordinary REMOVEPLAYER House cleanup completed"}));
        }
        let recovered = frame.saturating_sub(self.removal_frame.context("removal frame")?);
        let natural_result = state.frontend.score_page.is_some()
            && matches!(state.frontend.screen, GameScreen::MissionResult { .. });
        if self.death.is_some() && (recovered >= 50 || natural_result) {
            ensure!(
                !state.match_state.network_wait.waiting() && !sim.pending_network_wait(),
                "recovery retained a held frame"
            );
            ensure!(
                self.remove_admissions == 1,
                "timeout did not admit exactly one raw REMOVEPLAYER"
            );
            ensure!(
                live.sent_commands() > self.sent_before,
                "timeout never sent its observed REMOVEPLAYER"
            );
            route.push(json!({"render_frame":render,"native_frame":frame,"action":"ordinary post-removal gameplay recovered","frames":recovered,"natural_result":natural_result}));
            return Ok(true);
        }
        Ok(false)
    }

    fn observe_checksum(
        &mut self,
        state: &mut AppState,
        render: u32,
        route: &mut Vec<Value>,
    ) -> Result<bool> {
        use crate::app::match_runtime::network_failure;
        let (frame, tick, rng, mismatch) = {
            let sim = &state
                .match_state
                .sim_runtime
                .as_ref()
                .context("desync world")?
                .simulation;
            ensure!(
                sim.pending_network_interruption().is_none(),
                "desync test reached another network error"
            );
            (
                sim.session().binary_frame(),
                sim.session().tick(),
                sim.clone_scenario_rng().logical_state(),
                sim.pending_network_failure(),
            )
        };
        if let Some(mismatch) = mismatch {
            ensure!(
                i32::from(mismatch.house_id) == self.peer_house.context("desync peer House")?
                    && mismatch.local_checksum != mismatch.remote_checksum
                    && mismatch.event_frame > REQUIRED_FRAMES as i32,
                "checksum modal is not the expected admitted peer mismatch"
            );
            ensure!(
                state.match_state.network_failure.is_some()
                    && state.frontend.shell_controller.top_id() == Some(network_failure::DIALOG),
                "real checksum CE dialog is absent"
            );
            let labels = network_failure::labels(state);
            ensure!(
                labels.0.0 == "TXT_OUT_OF_SYNC" && labels.1.0 == "TXT_STOP",
                "another error dialog replaced the checksum modal"
            );
            let held = self.checksum.get_or_insert_with(|| HeldChecksum {
                mismatch,
                frame,
                tick,
                rng: rng.clone(),
                first_render: render,
                pressed_render: None,
                released_render: None,
            });
            ensure!(
                held.mismatch == mismatch
                    && held.frame == frame
                    && held.tick == tick
                    && held.rng == rng,
                "checksum modal reentered Logic or consumed simulation RNG"
            );
            if render == held.first_render {
                return Ok(false);
            }
            let rect = crate::ui::shell::modal::body_ok_layout(
                state.render_width() as i32,
                state.render_height() as i32,
            )
            .ok;
            set_window_pointer_to_rect_center(state, rect);
            network_failure::cursor_moved(state);
            if held.pressed_render.is_none() {
                network_failure::mouse(state, MouseButton::Left, true);
                held.pressed_render = Some(render);
                route.push(json!({"render_frame":render,"native_frame":frame,"action":"actual checksum CE Stop pressed; frame and RNG held"}));
            } else if held.released_render.is_none() {
                network_failure::mouse(state, MouseButton::Left, false);
                held.released_render = Some(render);
                route.push(json!({"render_frame":render,"native_frame":frame,"action":"actual checksum CE Stop released through normal input"}));
            }
            MoviesCapture::restore_neutral_pointer(state);
            return Ok(false);
        }
        let Some(held) = &self.checksum else {
            return Ok(false);
        };
        ensure!(
            held.released_render.is_some(),
            "checksum continuation cleared without Stop acknowledgement"
        );
        if state.frontend.screen != GameScreen::MainMenu || !lobby_settled(state) {
            return Ok(false);
        }
        ensure!(
            frame == held.frame.wrapping_add(1) && tick == held.tick.wrapping_add(1),
            "checksum Stop did not commit exactly one native aborted tail"
        );
        ensure!(
            state.match_state.live_network.is_none()
                && state.match_state.lan_service.is_some()
                && !state.match_state.network_wait.waiting()
                && state.match_state.network_failure.is_none()
                && state.frontend.shell_controller.top_id() != Some(network_failure::DIALOG)
                && state.match_state.session.network_setup().is_none()
                && state.frontend.loading_session.is_none()
                && state.match_state.scenario_exit.is_none()
                && !state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("returned desync world")?
                    .simulation
                    .pending_network_wait(),
            "checksum abort retained gameplay, modal, setup or loading owners"
        );
        self.returned = Some(
            json!({"native_frame":frame,"held_frame":held.frame,"held_tick":held.tick,"tick":tick,
            "held_tail_commits":frame.wrapping_sub(held.frame),"live_owner_retired":true,"lan_service_retained":true,
            "network_wait_cleared":true,"checksum_cleared":true,"modal_cleared":true,"pending_exit":false}),
        );
        route.push(json!({"render_frame":render,"native_frame":frame,"action":"checksum Stop returned to settled LAN lobby after exactly one held tail"}));
        Ok(true)
    }

    fn observe_return(
        &mut self,
        state: &AppState,
        render: u32,
        route: &mut Vec<Value>,
    ) -> Result<bool> {
        ensure!(
            state.match_state.scenario_exit.is_none(),
            "EA Quit started EXIT teardown"
        );
        ensure!(
            matches!(
                state.frontend.screen,
                GameScreen::InGame | GameScreen::MainMenu
            ),
            "unexpected Quit destination"
        );
        let online = self.wait.as_ref().context("Quit held frame")?.info.mode
            == crate::net::live_commands::LiveCommandMode::Wol;
        let settled = if online {
            super::online::online_lobby_settled(state)
        } else {
            state.frontend.lan.is_some() && lobby_settled(state)
        };
        if state.frontend.screen != GameScreen::MainMenu || !settled {
            return Ok(false);
        }
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("returned world retained")?
            .simulation;
        let held = self.wait.as_ref().context("Quit held frame")?;
        ensure!(
            sim.session().binary_frame() == held.frame.wrapping_add(1),
            "EA Quit did not commit exactly one held tail"
        );
        ensure!(
            !sim.pending_network_wait() && !state.match_state.network_wait.waiting(),
            "EA Quit retained wait state"
        );
        ensure!(
            state.match_state.live_network.is_none(),
            "EA Quit retained live command owner"
        );
        ensure!(
            state.match_state.lan_service.is_some(),
            "EA Quit dropped the portable LAN service"
        );
        ensure!(
            state.match_state.session.network_setup().is_none()
                && state.frontend.loading_session.is_none(),
            "EA Quit retained setup/loading"
        );
        if self.mode == Mode::Startup {
            ensure!(
                sim.session().tick() == held.tick.wrapping_add(1)
                    && state.match_state.network_failure.is_none()
                    && sim.pending_network_interruption().is_none(),
                "startup OK did not retire its modal after exactly one aborted tick"
            );
        }
        self.returned = Some(
            json!({"native_frame":sim.session().binary_frame(),"tick":sim.session().tick(),"held_frame":held.frame,
            "held_tail_commits":sim.session().binary_frame().wrapping_sub(held.frame),"live_owner_retired":true,"lan_service_retained":true,
            "network_wait_cleared":true,"pending_exit":false}),
        );
        route.push(json!({"render_frame":render,"native_frame":sim.session().binary_frame(),
            "action":if self.mode == Mode::Startup { "startup ConnectionFailed OK returned to settled lobby after one held-tail commit" } else { "EA Quit returned to settled lobby after one held-tail commit" },"online":online}));
        Ok(true)
    }

    pub(super) fn receipt(&self) -> Value {
        json!({"expected_network_failure":if self.mode == Mode::Startup {"externally killed original host before full startup certification; real EA votes and ConnectionFailed OK"} else if self.mode == Mode::Desync {"external real FRAMEINFO checksum corruption after a shared prefix"} else {"external peer disappearance after a real shared prefix"},
            "mode":format!("{:?}",self.mode),"peer_house":self.peer_house,"peer_source":self.peer_source,
            "sent_commands_before":self.sent_before,"remove_player_admissions":self.remove_admissions,"dialog":self.dialog,"removed":self.removed,
            "remove_player":self.remove_player,"remove_player_seen_frame":self.remove_player_frame,"death":self.death,"quit":self.quit,"returned":self.returned,
            "last_live":self.last_live,"startup":self.startup.as_ref().map(startup::StartupCapture::receipt),
            "checksum":self.checksum.as_ref().map(|held|json!({"house":held.mismatch.house_id,"event_frame":held.mismatch.event_frame,
                "delay":held.mismatch.delay,"history_index":held.mismatch.history_index,"local_checksum":held.mismatch.local_checksum,
                "remote_checksum":held.mismatch.remote_checksum,"held_frame":held.frame,"held_tick":held.tick,
                "first_render":held.first_render,"pressed_render":held.pressed_render,"released_render":held.released_render,
                "rng_held":true,"body_key":"TXT_OUT_OF_SYNC","button_key":"TXT_STOP"}))})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::live_commands::LiveCommandMode;

    #[test]
    fn held_failure_wait_uses_live_transport_mode_deadlines() {
        let sim = crate::sim::world::Simulation::new();
        let info = LiveWaitInfo {
            mode: LiveCommandMode::Wol,
            kind: LiveWaitKind::Ordinary,
            entered_at: 50,
            response_time_at_entry: 0,
        };
        let online = HeldWait::new(info, &sim);
        assert_eq!((online.dialog_period, online.timeout_period), (720, 7200));
        let measured = HeldWait::new(
            LiveWaitInfo {
                response_time_at_entry: 241,
                ..info
            },
            &sim,
        );
        assert_eq!(
            (measured.dialog_period, measured.timeout_period),
            (723, 7200)
        );
        let lan = HeldWait::new(
            LiveWaitInfo {
                mode: LiveCommandMode::Lan,
                ..info
            },
            &sim,
        );
        assert_eq!((lan.dialog_period, lan.timeout_period), (420, 2400));
    }
}
