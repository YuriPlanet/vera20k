//! Diagnostic input/observations for one externally killed, uncertified startup.
//! This reuses the real Initial wait, EA row input, ConnectionFailed CE and
//! FailureCapture return. No progress, vote, time, membership or tick is supplied.

use super::*;
use crate::app::diagnostics::shell_capture::fault_files::{read_control, write_atomic};
use crate::app::diagnostics::shell_capture::startup_host_loss::{
    StartupVoteControl, read_killed, read_vote, sidecar,
};
use crate::app::diagnostics::shell_capture::{LiveOnlineConfig, network_startup_receipt};
use crate::app::match_runtime::network_failure;
use crate::sim::world::{NetworkInterruption, Simulation};

pub(super) struct StartupCapture {
    config: LiveOnlineConfig,
    prefix: Option<Value>,
    control: Option<Value>,
    dialog_ready: Option<Value>,
    vote_control: Option<StartupVoteControl>,
    vote: Option<Value>,
    modal: Option<Value>,
    failure: Option<Value>,
    ack: Option<Value>,
    lobby_return: Option<Value>,
    pressed_render: Option<u32>,
    released_render: Option<u32>,
}

impl StartupCapture {
    fn new(config: LiveOnlineConfig) -> Self {
        Self {
            config,
            prefix: None,
            control: None,
            dialog_ready: None,
            vote_control: None,
            vote: None,
            modal: None,
            failure: None,
            ack: None,
            lobby_return: None,
            pressed_render: None,
            released_render: None,
        }
    }

    pub(super) fn receipt(&self) -> Value {
        let mut modal = self.modal.clone();
        if let Some(modal) = modal.as_mut() {
            modal["pressed_render"] = json!(self.pressed_render);
            modal["released_render"] = json!(self.released_render);
        }
        json!({"process_id":std::process::id(),"prefix":self.prefix,"control":self.control,
            "dialog_ready":self.dialog_ready,"vote_control":self.vote_control,"vote":self.vote,
            "modal":modal,"failure":self.failure,"ack":self.ack,"lobby_return":self.lobby_return})
    }
}

fn private_peers(live: &crate::net::live_commands::LiveCommandSession) -> Value {
    let peers = live.connections().peers().collect::<Vec<_>>();
    json!(
        peers
            .into_iter()
            .map(|(house, endpoint)| { json!({"house":house,"endpoint":endpoint.to_string()}) })
            .collect::<Vec<_>>()
    )
}

fn no_to_die(state: &AppState, sim: &Simulation) -> Result<bool> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .context("frozen startup roster")?;
    ensure!(
        launch.humans.len() == 3,
        "startup lost its original admission"
    );
    for source in 0..launch.humans.len() {
        let house = network_match::source_house(&state.match_state.session, sim, source)
            .context("startup source House")?;
        let owner = *sim
            .houses()
            .order()
            .get(usize::try_from(house)?)
            .context("startup House")?;
        if sim.houses()[&owner].is_to_die() {
            return Ok(false);
        }
    }
    Ok(true)
}

impl FailureCapture {
    pub(in super::super) fn startup(config: LiveOnlineConfig) -> Self {
        let mut capture = Self::new(Mode::Startup);
        capture.startup = Some(StartupCapture::new(config));
        capture
    }

    pub(in super::super) fn is_startup(&self) -> bool {
        self.mode == Mode::Startup
    }

    /// Arm only after the actual first frame has reached the installed full3
    /// Initial wait. The ordinary simulation/transport owners produce this view.
    pub(in super::super) fn arm_startup(&mut self, state: &AppState, render: u32) -> Result<bool> {
        if self.armed() {
            return Ok(true);
        }
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("startup private owner")?;
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("startup world")?
            .simulation;
        let Some(info) = live.wait_info() else {
            ensure!(
                sim.session().binary_frame() == 0,
                "startup advanced before Initial observation"
            );
            return Ok(false);
        };
        ensure!(
            info.kind == LiveWaitKind::Initial
                && live.awaiting_initial_barrier()
                && live.startup_pending()
                && sim.pending_network_wait(),
            "startup observation missed the initial barrier"
        );
        let startup = self.startup.as_mut().context("startup observer config")?;
        let output = startup
            .config
            .loading_output
            .as_ref()
            .context("startup output")?;
        let install = read_control(&sidecar(output, "install"), 131072)?
            .context("startup install guard absent")?;
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("startup launch")?;
        let original = network_startup_receipt(state, launch, Some(sim))?;
        let connected = state.match_state.session.network_connected_source_indices();
        ensure!(
            launch.humans.len() == 3
                && connected.len() == 3
                && live.peers().len() == 2
                && live.connections().peers().count() == 2
                && launch.humans[launch.local_player_index].name == startup.config.account
                && startup.config.account != startup.config.host
                && install["schema"] == "vera20k.startup-host-install.v1"
                && install["account"] == startup.config.account
                && install["process_id"] == std::process::id()
                && install["checkpoint_id"] == json!(startup.config.startup_host_loss)
                && install["startup"] == original
                && !original["match_id"].is_null()
                && sim.session().binary_frame() == 0
                && sim.session().tick() == 0
                && sim.pending_local_command_count() == 0
                && live.sent_commands() == 0
                && sim.pending_network_interruption().is_none()
                && no_to_die(state, sim)?,
            "startup prefix is not the installed original full three-player admission"
        );
        let certificate = serde_json::to_value(live.startup_agreement())?;
        ensure!(
            certificate["houses"]
                .as_array()
                .is_some_and(|houses| houses.len() == 3)
                && certificate["active"]
                    .as_u64()
                    .is_some_and(|mask| mask.count_ones() == 3),
            "startup certificate was narrowed before the external loss"
        );
        let host = launch
            .humans
            .iter()
            .position(|human| human.name == startup.config.host)
            .context("startup victim source")?;
        let host_house = network_match::source_house(&state.match_state.session, sim, host)
            .context("startup victim House")?;
        self.peer_source = Some(host);
        self.peer_house = Some(host_house);
        self.peer_name.clone_from(&startup.config.host);
        self.wait = Some(HeldWait::new(info, sim));
        let held = self.wait.as_ref().expect("Initial wait retained");
        let observed = (crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32) >> 4;
        ensure!(
            observed.wrapping_sub(info.entered_at) < held.timeout_period,
            "startup prefix was observed after the native initial deadline"
        );
        let mut prefix = install.clone();
        prefix["schema"] = json!("vera20k.startup-host-prefix.v1");
        prefix["render_frame"] = json!(render);
        prefix["uptime_ms"] = json!(crate::app::match_runtime::frame_pacer::wall_clock_ms());
        prefix["install"] = install;
        prefix["certificate"] = certificate;
        prefix["native_frame"] = json!(0);
        prefix["tick"] = json!(0);
        prefix["current_world_hash"] = json!(sim.state_hash());
        prefix["startup_pending"] = json!(true);
        prefix["awaiting_initial_barrier"] = json!(true);
        prefix["connected_sources"] = json!(connected);
        prefix["private_peers"] = private_peers(live);
        prefix["progress_peers"] = json!(live.peers().iter().map(|peer|json!({
            "house":peer.house_id(),"frame":peer.frame(),"advertised_commands":peer.advertised_commands(),
            "received_commands":peer.received_commands()})).collect::<Vec<_>>());
        prefix["local_commands"] = json!(0);
        prefix["sent_commands"] = json!(0);
        prefix["no_to_die"] = json!(true);
        prefix["wait"] = json!({"mode":format!("{:?}",info.mode),"kind":"Initial",
            "entered_at":info.entered_at,"response_time_at_entry":info.response_time_at_entry,
            "observed_bucket":observed,"dialog_period":held.dialog_period,"timeout_period":held.timeout_period});
        write_atomic(&sidecar(output, "prefix"), &prefix)?;
        startup.prefix = Some(prefix);
        Ok(true)
    }

    pub(in super::super) fn record_startup_lobby(
        &mut self,
        state: &AppState,
        render: u32,
    ) -> Result<()> {
        let startup = self.startup.as_mut().context("startup return observer")?;
        ensure!(
            self.returned.is_some()
                && super::super::online::online_lobby_settled(state)
                && state
                    .frontend
                    .wol
                    .as_ref()
                    .is_some_and(|visit| visit.authenticated())
                && state.match_state.live_network.is_none()
                && state.match_state.session.network_setup().is_none()
                && state.frontend.network_setup_protocol.is_none()
                && state.frontend.loading_session.is_none()
                && state.match_state.network_failure.is_none()
                && !state.match_state.network_wait.waiting(),
            "startup return retained an unauthenticated lobby or gameplay owner"
        );
        startup.lobby_return = Some(
            json!({"render_frame":render,"authenticated":true,"presented":true,
            "live_owner_retired":true,"setup_retired":true,"loading_retired":true,"modal_retired":true}),
        );
        Ok(())
    }

    pub(in super::super) fn observe_startup(
        &mut self,
        state: &mut AppState,
        render: u32,
        route: &mut Vec<Value>,
    ) -> Result<bool> {
        if self
            .startup
            .as_ref()
            .is_some_and(|startup| startup.released_render.is_some())
        {
            return self.observe_return(state, render, route);
        }
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("held startup world")?
            .simulation;
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("held startup transport")?;
        let held = self.wait.as_ref().context("held Initial wait")?;
        let startup = self.startup.as_mut().context("startup observer")?;
        let prefix = startup.prefix.as_ref().context("startup full3 prefix")?;
        ensure!(
            sim.session().binary_frame() == held.frame
                && held.frame == 0
                && sim.session().tick() == held.tick
                && held.tick == 0
                && sim.clone_scenario_rng().logical_state() == held.rng
                && json!(sim.state_hash()) == prefix["current_world_hash"]
                && no_to_die(state, sim)?
                && !sim.quit_requested()
                && sim.pending_local_command_count() == 0
                && live.sent_commands() == 0
                && live.startup_pending(),
            "uncertified startup advanced world/RNG, flagged a House or admitted commands"
        );
        ensure!(
            sim.pending_command_snapshot().is_empty(),
            "uncertified startup queued a semantic record"
        );
        let connected = state.match_state.session.network_connected_source_indices();
        let peer_source = self.peer_source.context("startup victim source")?;
        let now = (crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32) >> 4;
        let elapsed = now.wrapping_sub(held.info.entered_at);
        if let Some(control) = read_killed(&startup.config, &prefix["startup"]["match_id"])? {
            ensure!(
                startup.control.as_ref().is_none_or(|old| old == &control),
                "startup kill control changed"
            );
            startup.control = Some(control);
        }
        // Removal or a modal before the acknowledged owned kill is a failed
        // attempt, never proof inferred from a later controller file.
        if startup.control.is_none() {
            ensure!(
                connected.len() == 3
                    && live.peers().len() == 2
                    && sim.pending_network_interruption().is_none(),
                "startup failed or lost a source before the owned kill acknowledgement"
            );
            return Ok(false);
        }
        ensure!(
            startup.failure.is_some() || elapsed < held.timeout_period,
            "startup reached initial expiry instead of the real row-vote failure"
        );
        let failure = sim.pending_network_interruption();
        if failure.is_none() {
            ensure!(
                connected.len() == 3
                    && live.peers().len() == 2
                    && live.awaiting_initial_barrier()
                    && sim.pending_network_wait(),
                "startup membership changed without its ConnectionFailed modal"
            );
            // This observer runs after presentation. The renderer uses this
            // same predicate before drawing the complete native EA shell.
            if crate::app::frontend::skirmish_shell_render::connection_shell_drawable(state) {
                let page = state
                    .match_state
                    .network_wait
                    .page()
                    .context("rendered EA")?;
                ensure!(
                    elapsed >= held.dialog_period
                        && page.rows.len() == 3
                        && page.visible_range() == (0..3)
                        && page.countdown.is_some(),
                    "startup EA does not display the complete native Initial wait"
                );
                if startup.dialog_ready.is_none() {
                    // Wait_For_Players648DC0..648DEC clears early proposals
                    // when each EA opens. Wait for both observed dialogs rather
                    // than assuming the two native waits opened together.
                    // `opened_bucket` is our first post-presentation observation;
                    // painted_elapsed belongs to the native page's last repaint.
                    self.dialog = Some(
                        json!({"entry_bucket":held.info.entered_at,"opened_bucket":now,"observed_bucket":now,
                        "native_painted_elapsed":page.painted_elapsed,
                        "native_painted_bucket":held.info.entered_at.wrapping_add(u32::try_from(page.painted_elapsed)?),
                        "dialog_period":held.dialog_period,"timeout_period":held.timeout_period,
                        "held_frame":held.frame,"rows":page.rows.iter().map(|row|json!({
                            "source":row.source_index,"name":row.name})).collect::<Vec<_>>()}),
                    );
                    let mut ready = prefix.clone();
                    for key in ["install", "loading", "progress_peers", "certificate"] {
                        ready
                            .as_object_mut()
                            .context("startup prefix object")?
                            .remove(key);
                    }
                    ready["schema"] = json!("vera20k.startup-host-dialog.v1");
                    ready["prefix_render_frame"] = prefix["render_frame"].clone();
                    ready["render_frame"] = json!(render);
                    ready["uptime_ms"] =
                        json!(crate::app::match_runtime::frame_pacer::wall_clock_ms());
                    ready["world_held"] = json!(true);
                    ready["rng_held"] = json!(true);
                    ready["control"] = startup.control.clone().context("owned kill control")?;
                    ready["dialog"] = self.dialog.clone().context("EA observation")?;
                    write_atomic(
                        &sidecar(
                            startup
                                .config
                                .loading_output
                                .as_ref()
                                .context("startup output")?,
                            "dialog",
                        ),
                        &ready,
                    )?;
                    startup.dialog_ready = Some(ready);
                    route.push(json!({"render_frame":render,"native_frame":0,
                        "action":"complete Initial EA presented; awaiting both survivor dialog proofs"}));
                    return Ok(false);
                }
                if startup.vote.is_none() {
                    let ready = startup
                        .dialog_ready
                        .as_ref()
                        .context("presented EA proof")?;
                    let rows = json!(
                        page.rows
                            .iter()
                            .map(|row| json!({
                        "source":row.source_index,"name":row.name}))
                            .collect::<Vec<_>>()
                    );
                    ensure!(
                        rows == ready["dialog"]["rows"],
                        "startup EA rows changed after its ready proof"
                    );
                    let launch = state
                        .match_state
                        .session
                        .network_setup()
                        .context("startup original roster")?;
                    let survivors: Vec<_> = launch
                        .humans
                        .iter()
                        .filter(|human| human.name != startup.config.host)
                        .map(|human| human.name.as_str())
                        .collect();
                    let Some(control) = read_vote(&startup.config, ready, &survivors)? else {
                        return Ok(false);
                    };
                    ensure!(
                        ready["render_frame"]
                            .as_u64()
                            .is_some_and(|ready| u64::from(render) > ready)
                            && startup
                                .vote_control
                                .as_ref()
                                .is_none_or(|old| old == &control),
                        "startup vote grant changed or preceded its presented dialog proof"
                    );
                    startup.vote_control = Some(control);
                    let row = page
                        .rows
                        .iter()
                        .position(|row| {
                            row.source_index == peer_source && row.name == startup.config.host
                        })
                        .context("original host EA row")?;
                    let visible = page.visible_range();
                    ensure!(visible.contains(&row), "startup host EA row is not visible");
                    let physical = row - visible.start;
                    let shell =
                        crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
                            state,
                        )
                        .context("startup EA geometry")?;
                    let layout = crate::ui::shell::connection::connection_layout(
                        shell.width,
                        shell.height,
                        shell.layout,
                        shell.button_size,
                    );
                    let point = set_window_pointer_to_rect_center(state, layout.players[physical]);
                    crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                    for edge in [
                        winit::event::ElementState::Pressed,
                        winit::event::ElementState::Released,
                    ] {
                        crate::app::input::dispatch::handle_mouse_input(
                            state,
                            MouseButton::Left,
                            edge,
                        );
                    }
                    startup.vote = Some(json!({"render_frame":render,"observed_bucket":now,
                        "source_index":peer_source,"physical_row":physical,"point":point}));
                    MoviesCapture::restore_neutral_pointer(state);
                    route.push(json!({"render_frame":render,"native_frame":0,"action":"actual Initial EA original-host row clicked after both survivor dialog proofs"}));
                }
            }
            return Ok(false);
        }
        ensure!(
            failure == Some(NetworkInterruption::ConnectionFailed)
                && self.dialog.is_some()
                && startup.dialog_ready.is_some()
                && startup.vote_control.is_some()
                && startup.vote.is_some()
                && !connected.contains(&peer_source)
                && !live.awaiting_initial_barrier()
                && state.match_state.network_failure.is_some()
                && state.frontend.shell_controller.top_id() == Some(network_failure::DIALOG),
            "startup loss did not fail closed after only the real victim-row vote"
        );
        let labels = network_failure::labels(state);
        ensure!(
            labels.0.0 == "TXT_SYSTEM_NOT_RESPONDING" && labels.1.0 == "TXT_OK",
            "another modal replaced startup ConnectionFailed"
        );
        if startup.modal.is_none() {
            // The immutable failure proof precedes either CE acknowledgement.
            // After both proofs exist, the other survivor may finish ordinary
            // cleanup while this one's external ACK is still in flight.
            ensure!(
                connected.len() == 2
                    && live.peers().len() == 1
                    && live.connections().peers().count() == 1,
                "startup proof did not retain the other live survivor"
            );
            startup.modal =
                Some(json!({"first_render":render,"body_key":labels.0.0,"button_key":labels.1.0}));
            let mut snapshot = prefix.clone();
            // Historical loading/initial peer progress stays in the linked
            // prefix, never presented as the current failed transport state.
            for key in [
                "install",
                "loading",
                "progress_peers",
                "wait",
                "certificate",
            ] {
                snapshot.as_object_mut().expect("prefix object").remove(key);
            }
            snapshot["schema"] = json!("vera20k.startup-host-failure.v1");
            snapshot["render_frame"] = json!(render);
            snapshot["uptime_ms"] = json!(crate::app::match_runtime::frame_pacer::wall_clock_ms());
            snapshot["prefix_render_frame"] = prefix["render_frame"].clone();
            snapshot["control"] = startup.control.clone().context("owned kill control")?;
            snapshot["victim_source"] = json!(peer_source);
            snapshot["victim_house"] = json!(self.peer_house);
            snapshot["removed_sources"] = json!([peer_source]);
            snapshot["connected_sources"] = json!(connected);
            snapshot["private_peers"] = private_peers(live);
            snapshot["awaiting_initial_barrier"] = json!(false);
            snapshot["world_held"] = json!(true);
            snapshot["rng_held"] = json!(true);
            snapshot["victim_to_die"] = json!(false);
            snapshot["dialog"] = self.dialog.clone().context("EA observation")?;
            snapshot["dialog_ready"] = startup.dialog_ready.clone().context("EA ready proof")?;
            snapshot["vote_control"] =
                serde_json::to_value(startup.vote_control.as_ref().context("EA vote grant")?)?;
            snapshot["vote"] = startup.vote.clone().context("EA vote")?;
            snapshot["modal"] = startup.modal.clone().context("ConnectionFailed modal")?;
            write_atomic(
                &sidecar(
                    startup
                        .config
                        .loading_output
                        .as_ref()
                        .context("startup output")?,
                    "failure",
                ),
                &snapshot,
            )?;
            self.removed = Some(json!({"native_frame":0,"tick":0,"peer_source":peer_source,
                "peer_house":self.peer_house,"private_peers":snapshot["private_peers"],"world_held":true,"rng_held":true}));
            startup.failure = Some(snapshot);
        }
        if startup
            .modal
            .as_ref()
            .is_some_and(|modal| modal["first_render"] == render)
        {
            return Ok(false);
        }
        let output = startup
            .config
            .loading_output
            .as_ref()
            .context("startup output")?;
        let Some(ack) = read_control(&sidecar(output, "ack"), 4096)? else {
            return Ok(false);
        };
        let failed = startup
            .failure
            .as_ref()
            .context("startup failure receipt")?;
        let proofs = ack["failure_processes"]
            .as_array()
            .context("both startup failures")?;
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("startup original roster")?;
        let mut expected: Vec<_> = launch
            .humans
            .iter()
            .filter(|human| human.name != startup.config.host)
            .map(|human| human.name.as_str())
            .collect();
        expected.sort_unstable();
        let mut actual: Vec<_> = proofs
            .iter()
            .filter_map(|proof| proof["account"].as_str())
            .collect();
        actual.sort_unstable();
        ensure!(
            ack["schema"] == "vera20k.startup-host-ack.v1"
                && ack["phase"] == "startup-ack"
                && ack["checkpoint_id"] == json!(startup.config.startup_host_loss)
                && ack["match_id"] == prefix["startup"]["match_id"]
                && ack["account"] == startup.config.account
                && ack["process_id"] == std::process::id()
                && ack["ready_render_frame"] == failed["render_frame"]
                && ack["victim_account"] == startup.config.host
                && ack["all_failures_observed"] == true
                && proofs.len() == 2
                && actual == expected
                && proofs.iter().all(|proof| proof["process_id"]
                    .as_u64()
                    .is_some_and(|pid| pid > 0)
                    && proof["render_frame"].as_u64().is_some())
                && proofs
                    .iter()
                    .any(|proof| proof["account"] == startup.config.account
                        && proof["process_id"] == std::process::id()
                        && proof["render_frame"] == failed["render_frame"])
                && startup.ack.as_ref().is_none_or(|old| old == &ack),
            "startup OK control is not bound to both fresh survivor failures in this match"
        );
        startup.ack = Some(ack);
        let rect = crate::ui::shell::modal::body_ok_layout(
            state.render_width() as i32,
            state.render_height() as i32,
        )
        .ok;
        set_window_pointer_to_rect_center(state, rect);
        network_failure::cursor_moved(state);
        if startup.pressed_render.is_none() {
            network_failure::mouse(state, MouseButton::Left, true);
            startup.pressed_render = Some(render);
            route.push(json!({"render_frame":render,"native_frame":0,"action":"actual startup ConnectionFailed CE OK pressed after both failure proofs"}));
        } else if startup.released_render.is_none() {
            network_failure::mouse(state, MouseButton::Left, false);
            startup.released_render = Some(render);
            route.push(json!({"render_frame":render,"native_frame":0,"action":"actual startup ConnectionFailed CE OK released through normal input"}));
        }
        MoviesCapture::restore_neutral_pointer(state);
        Ok(false)
    }
}
