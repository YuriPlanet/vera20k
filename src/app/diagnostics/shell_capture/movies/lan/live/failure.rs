//! Observes real peer disappearance after the shared 300-frame live prefix.
//! Only Quit Game uses input. Time, peer packets, commands and simulation
//! advancement remain owned by the ordinary application path.

use super::*;
use crate::app::match_runtime::network_match;
use crate::net::live_commands::{LiveWaitInfo, LiveWaitKind};
use crate::sim::command::{Command, CommandRecord, EXIT_OPCODE, REMOVE_PLAYER_OPCODE};
use crate::sim::rng::SimRngLogicalState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Timeout,
    Quit,
}

struct HeldWait {
    info: LiveWaitInfo,
    frame: u32,
    tick: u64,
    rng: SimRngLogicalState,
    dialog_period: u32,
}

pub(super) struct FailureCapture {
    mode: Mode,
    peer_house: Option<i32>,
    peer_source: Option<usize>,
    peer_name: String,
    sent_before: u16,
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
}

impl FailureCapture {
    pub(super) fn new(mode: Mode) -> Self {
        Self {
            mode,
            peer_house: None,
            peer_source: None,
            peer_name: String::new(),
            sent_before: 0,
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
        Ok(())
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
        let now = (crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32) >> 4;
        self.observe_commands(state)?;
        if self.quit.is_some() {
            return self.observe_return(state, render, route);
        }
        ensure!(
            state.frontend.screen == GameScreen::InGame,
            "failure survivor left gameplay early"
        );
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
        self.last_live = Some(json!({"frame":frame,"sent_commands":live.sent_commands(),
            "private_peers":live.connections().peers().map(|(id,endpoint)|json!({"house":id,"endpoint":endpoint.to_string()})).collect::<Vec<_>>(),
            "progress_peers":live.peers().len(),"global_sends":live.global().queued_send_count(),
            "global_receives":live.global().queued_receive_count()}));

        if live.peers().is_empty() {
            return self.observe_recovery(state, render, now, route);
        }
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
            // The expected deadline comes from native648710's documented
            // signed floor; this observer never supplies the application's time.
            let period = (info.response_time_at_entry.wrapping_mul(3) as i32).max(420) as u32;
            self.wait = Some(HeldWait {
                info,
                frame,
                tick: sim.session().tick(),
                rng: sim.clone_scenario_rng().logical_state(),
                dialog_period: period,
            });
            route.push(json!({"render_frame":render,"native_frame":frame,"action":"ordinary wait reached",
                "entry_bucket":info.entered_at,"observed_bucket":now,"response":info.response_time_at_entry,
                "dialog_period":period}));
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
                "elapsed_buckets":elapsed,"held_frame":frame,"rows":page.rows,"countdown":page.countdown}));
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
                now.wrapping_sub(wait.info.entered_at) >= 2400,
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
        if self.death.is_some() && recovered >= 50 {
            ensure!(
                !state.match_state.network_wait.waiting() && !sim.pending_network_wait(),
                "recovery retained a held frame"
            );
            ensure!(
                live.sent_commands() == self.sent_before.wrapping_add(1),
                "timeout sent duplicate or unrelated commands"
            );
            route.push(json!({"render_frame":render,"native_frame":frame,"action":"50 post-recovery frames committed","frames":recovered}));
            return Ok(true);
        }
        Ok(false)
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
        if state.frontend.screen != GameScreen::MainMenu
            || state.frontend.lan.is_none()
            || !lobby_settled(state)
        {
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
        self.returned = Some(
            json!({"native_frame":sim.session().binary_frame(),"held_frame":held.frame,
            "held_tail_commits":sim.session().binary_frame().wrapping_sub(held.frame),"live_owner_retired":true,"lan_service_retained":true,
            "network_wait_cleared":true,"pending_exit":false}),
        );
        route.push(json!({"render_frame":render,"native_frame":sim.session().binary_frame(),"action":"EA Quit returned to settled BB after one held-tail commit"}));
        Ok(true)
    }

    pub(super) fn receipt(&self) -> Value {
        json!({"mode":format!("{:?}",self.mode),"peer_house":self.peer_house,"peer_source":self.peer_source,
            "sent_commands_before":self.sent_before,"dialog":self.dialog,"removed":self.removed,
            "remove_player":self.remove_player,"remove_player_seen_frame":self.remove_player_frame,"death":self.death,"quit":self.quit,"returned":self.returned,
            "last_live":self.last_live})
    }
}
