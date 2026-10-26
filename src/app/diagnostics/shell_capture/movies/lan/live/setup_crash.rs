//! Real editable-LAN process-loss proof. Only the external owned-process
//! runner kills a victim. Diagnostic control files acknowledge that boundary;
//! they never inject a service event or alter the real peer lease.

use super::*;
use crate::app::diagnostics::shell_capture::LanSetupCrash;
use std::path::{Path, PathBuf};
use crate::util::time::{Duration, Instant};

const FAULT_BOUNDARY_LIMIT: Duration = Duration::from_secs(5);

struct FaultBoundary {
    id: String,
    ready: Value,
    ready_at: Instant,
    killed: Option<Value>,
}

pub(super) struct SetupCrashCapture {
    victim: LanSetupCrash,
    prefix: Option<Value>,
    armed_at: Option<Instant>,
    boundary: Option<FaultBoundary>,
    recovered: Option<Value>,
    returned: Option<Value>,
}

fn sidecar(prefix: &Path, kind: &str) -> PathBuf {
    let mut name = prefix.as_os_str().to_owned();
    name.push(format!(".{kind}.json"));
    PathBuf::from(name)
}

use crate::app::diagnostics::shell_capture::fault_files::{read_control, write_atomic};

fn control_id<'a>(control: &'a Value, phase: &str) -> Result<&'a str> {
    let id = control["fault_id"]
        .as_str()
        .context("fault control identity")?;
    ensure!(
        control["schema"] == "vera20k.lan-setup-crash-control.v1"
            && control["phase"] == phase
            && id.len() == 32
            && id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "stale or out-of-order fault control"
    );
    Ok(id)
}

fn same_identity(first: &Value, next: &Value) -> bool {
    [
        "scope",
        "members",
        "local_endpoint",
        "peer_endpoint",
        "peer_name",
        "peer_source",
        "peer_raw_row",
        "map_admission",
        "lease_period",
    ]
    .iter()
    .all(|field| first[*field] == next[*field])
}

/// Project the exact admitted members using the map facts this client owns.
/// MPGameOptions::ParsePacket5E3D10's K arm5E4C3D..57 accepts peer map
/// statuses only on host screens C2/BC. A guest retains its own K1; it does
/// not acquire a positive K row for the host. Source: gamemd.exe instructions.
fn setup_members(
    session: &crate::app::match_runtime::session::MatchSession,
    transfer: bool,
) -> Option<Vec<Value>> {
    let launch = session.network_setup()?;
    if launch.humans.len() != 2 {
        return None;
    }
    launch
        .humans
        .iter()
        .enumerate()
        .map(|(source, human)| {
            let row = human.raw_slot_index?;
            if !transfer
                && (session.host() || source == launch.local_player_index)
                && session.network_map_status(source) != Some(1)
            {
                return None;
            }
            Some(json!({"name":human.name,"raw_row":row}))
        })
        .collect()
}

/// One read-only projection is reused for initial, requested-Ready and
/// post-kill observations. Admission/map/timer decisions remain app-owned.
fn snapshot(
    victim: LanSetupCrash,
    core: &LiveLanCapture,
    state: &AppState,
    rendered: PresentedShell,
    frame: u32,
    now: u32,
) -> Result<Option<Value>> {
    if rendered != PresentedShell::NetworkSetup || !setup_settled(state, core.host()) {
        return Ok(None);
    }
    let session = &state.match_state.session;
    let Some(launch) = session.network_setup() else {
        return Ok(None);
    };
    let Some(service) = state.match_state.lan_service.as_ref() else {
        return Ok(None);
    };
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .context("editable setup")?;
    let transfer = victim == LanSetupCrash::MapHost;
    let Some(members) = setup_members(session, transfer) else {
        return Ok(None);
    };
    if !service.setup_roster_complete(session)
        || !protocol.editable()
        || protocol.lan_go_send_pending()
        || (!transfer && protocol.blocking_map_transfer())
        || (!transfer && protocol.transfer_progress().is_some())
        || session.network_start_seed().is_none()
    {
        return Ok(None);
    }
    // Current admitted identity, not a momentarily empty socket FIFO, arms
    // the fault. Ordinary lossy presence and reliable download-window requests
    // may be queued on a healthy peer. Cleanup below still requires zero work.
    let (map_admission, map_download) = if transfer && !core.host() {
        let Some(metadata) = session.network_map_metadata() else {
            return Ok(None);
        };
        let Some((digest, completed, total)) = service.setup_map_download() else {
            return Ok(None);
        };
        if session.network_local_map_source().is_some()
            || session.network_local_map_effective_digest().is_some()
            || session.network_map_status(launch.local_player_index) == Some(1)
            || digest != metadata.source_sha256
            || total != usize::try_from(metadata.byte_length)?
            || protocol.transfer_progress()
                != Some((u32::try_from(completed)?, u32::try_from(total)?))
        {
            return Ok(None);
        }
        (
            json!({"file":launch.selected_map_file,"generated":false,"verified":false,"source":null,
                "source_sha256":metadata.source_sha256,"byte_length":metadata.byte_length,
                "effective_digest":metadata.effective_digest,"capacity":metadata.player_capacity}),
            Some(
                json!({"source_sha256":digest,"completed_bytes":completed,"total_bytes":total,
                "local_source_installed":false,"local_k1":session.network_map_status(launch.local_player_index)}),
            ),
        )
    } else {
        let Some(admission) = core
            .map_admission
            .as_ref()
            .filter(|admission| admission["file"].as_str() == launch.selected_map_file.as_deref())
        else {
            return Ok(None);
        };
        if session.network_map_status(launch.local_player_index) != Some(1) {
            return Ok(None);
        }
        (admission.clone(), None)
    };
    let (bind, peer_endpoint) = core
        .config
        .as_ref()
        .and_then(|config| config.transport.explicit_endpoints())
        .context("setup crash requires explicit LAN sockets")?;
    let Some(source) = session.network_endpoint_source(peer_endpoint) else {
        return Ok(None);
    };
    let Some((remaining, period)) = service
        .setup_peer_lease(peer_endpoint, now)
        .filter(|(remaining, _)| *remaining > 0)
    else {
        return Ok(None);
    };
    let peer = &launch.humans[source];
    ensure!(
        source != launch.local_player_index
            && peer.name == if core.host() { GUEST_NAME } else { HOST_NAME },
        "wrong admitted crash peer"
    );
    let scope = service.setup_scope().context("admitted setup scope")?;
    Ok(Some(
        json!({"schema":"vera20k.lan-setup-crash-prefix.v1", "victim":victim.as_str(),
        "account":if core.host() {HOST_NAME} else {GUEST_NAME},"host":HOST_NAME,"external_fault_pending":true,
        "setup":{"scope":scope,"members":members,"local_endpoint":bind.to_string(),
            "peer_endpoint":peer_endpoint.to_string(),"peer_name":peer.name,"peer_source":source,
            "peer_raw_row":peer.raw_slot_index,"map_admission":map_admission,"map_download":map_download,
            "queued_setup_count":service.queued_setup_count(),
            "editable":true,"go_pending":false,"loading":false,"gameplay_started":false,
            "observed_bucket":now,"lease_remaining":remaining,"lease_period":period,"render_frame":frame}}),
    ))
}

impl SetupCrashCapture {
    pub(super) fn new(victim: LanSetupCrash) -> Self {
        Self {
            victim,
            prefix: None,
            armed_at: None,
            boundary: None,
            recovered: None,
            returned: None,
        }
    }

    pub(super) fn ready(&self) -> bool {
        self.returned.is_some()
    }

    /// Return false on every pre-kill frame, including the frame accepting
    /// the kill acknowledgement. Cleanup can only be observed on later frames.
    fn observe_boundary(
        &mut self,
        core: &LiveLanCapture,
        state: &AppState,
        rendered: PresentedShell,
        frame: u32,
        now: u32,
    ) -> Result<bool> {
        if self
            .boundary
            .as_ref()
            .is_some_and(|boundary| boundary.killed.is_some())
        {
            return Ok(true);
        }
        let mut current = snapshot(self.victim, core, state, rendered, frame, now)?
            .context("setup changed or peer cleanup began before owned kill acknowledgement")?;
        ensure!(
            same_identity(
                &self.prefix.as_ref().expect("armed prefix")["setup"],
                &current["setup"]
            ),
            "admitted setup changed before fault boundary"
        );
        let prefix_path = core
            .config
            .as_ref()
            .and_then(|config| config.failure_prefix_output.as_ref())
            .context("fault boundary path")?;
        if self.boundary.is_none() {
            ensure!(
                !sidecar(prefix_path, "killed").try_exists()?,
                "kill arrived before requested Ready"
            );
            let Some(control) = read_control(&sidecar(prefix_path, "prepare"), 4096)? else {
                return Ok(false);
            };
            let id = control_id(&control, "prepare")?.to_owned();
            ensure!(
                current["setup"]["lease_remaining"]
                    .as_u64()
                    .context("fresh lease")?
                    >= current["setup"]["lease_period"]
                        .as_u64()
                        .context("lease period")?
                        / 2,
                "fresh survivor lease is too near expiry for a bounded fault"
            );
            let ready =
                json!({"schema":"vera20k.lan-setup-crash-ready.v1","fault_id":id,"prefix":current});
            write_atomic(&sidecar(prefix_path, "ready"), &ready)?;
            self.boundary = Some(FaultBoundary {
                id,
                ready,
                ready_at: Instant::now(),
                killed: None,
            });
            return Ok(false);
        }
        let boundary = self.boundary.as_mut().expect("prepared boundary");
        ensure!(
            boundary.ready_at.elapsed() <= FAULT_BOUNDARY_LIMIT,
            "owned kill acknowledgement missed the fresh survivor boundary"
        );
        let original_prepare = read_control(&sidecar(prefix_path, "prepare"), 4096)?
            .context("fault preparation disappeared")?;
        ensure!(
            control_id(&original_prepare, "prepare")? == boundary.id,
            "fault preparation identity changed"
        );
        let Some(control) = read_control(&sidecar(prefix_path, "killed"), 4096)? else {
            return Ok(false);
        };
        ensure!(
            control_id(&control, "killed")? == boundary.id
                && control["owned_kill_succeeded"] == true
                && control["ready_render_frame"]
                    == boundary.ready["prefix"]["setup"]["render_frame"]
                && control["ready_observed_bucket"]
                    == boundary.ready["prefix"]["setup"]["observed_bucket"],
            "owned kill does not acknowledge this fresh Ready"
        );
        let request = control["request_after_launch_ms"]
            .as_u64()
            .context("fault request time")?;
        let received = control["ready_after_launch_ms"]
            .as_u64()
            .context("fresh Ready time")?;
        let started = control["kill_started_after_launch_ms"]
            .as_u64()
            .context("owned kill start")?;
        let completed = control["kill_succeeded_after_launch_ms"]
            .as_u64()
            .context("owned kill result")?;
        ensure!(
            request <= received
                && received <= started
                && started <= completed
                && completed - request <= FAULT_BOUNDARY_LIMIT.as_millis() as u64,
            "owned kill was outside the requested fresh boundary"
        );
        ensure!(
            u64::from(frame)
                > boundary.ready["prefix"]["setup"]["render_frame"]
                    .as_u64()
                    .context("Ready frame")?,
            "kill acknowledgement reused the Ready frame"
        );
        // The runner publishes this file only after the exact owned SIGKILL
        // succeeds. Actual admission and positive lease are still present NOW.
        current["external_fault_pending"] = json!(false);
        boundary.killed = Some(json!({"schema":"vera20k.lan-setup-crash-observed.v1",
            "fault_id":boundary.id,"control":control,"prefix":current,
            "elapsed_since_ready_ms":boundary.ready_at.elapsed().as_millis() as u64}));
        Ok(false)
    }

    pub(super) fn after_present(
        &mut self,
        core: &mut LiveLanCapture,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<bool> {
        ensure!(
            !matches!(
                state.frontend.screen,
                GameScreen::Loading | GameScreen::InGame
            ) && state.match_state.live_network.is_none()
                && state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .is_none_or(|runtime| !runtime.simulation.session().started()),
            "editable LAN crash checkpoint reached loading/gameplay"
        );
        if self.returned.is_some() {
            return Ok(true);
        }
        if self.prefix.is_none() && !matches!(core.phase, Phase::HostReady | Phase::GuestSetup) {
            return Ok(false);
        }
        let now = (crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32) >> 4;
        if self.prefix.is_none() {
            let Some(prefix) = snapshot(self.victim, core, state, rendered, frame, now)? else {
                return Ok(true);
            };
            let path = core
                .config
                .as_ref()
                .and_then(|config| config.failure_prefix_output.as_ref())
                .context("setup crash prefix output")?;
            for kind in ["prepare", "ready", "killed"] {
                ensure!(
                    !sidecar(path, kind).try_exists()?,
                    "stale setup fault control exists"
                );
            }
            write_atomic(path, &prefix)?;
            self.prefix = Some(prefix);
            self.armed_at = Some(Instant::now());
            core.event(frame, "Real editable setup prefix saved; hold normal inputs for external prepare/kill boundary");
            return Ok(true);
        }
        ensure!(
            self.armed_at
                .is_some_and(|armed| armed.elapsed() < Duration::from_secs(120)),
            "editable LAN crash did not recover through its ordinary lease"
        );
        if core.host() == self.victim.host() {
            return Ok(true);
        }
        if !self.observe_boundary(core, state, rendered, frame, now)? {
            return Ok(true);
        }
        let boundary = self.boundary.as_ref().expect("requested boundary");
        let killed = boundary.killed.as_ref().expect("observed owned kill");
        let setup = &killed["prefix"]["setup"];
        ensure!(
            u64::from(frame) > setup["render_frame"].as_u64().context("post-kill frame")?,
            "cleanup was not observed strictly after the owned kill"
        );
        let elapsed = now.wrapping_sub(
            setup["observed_bucket"]
                .as_u64()
                .context("kill observation bucket")? as u32,
        );
        let (_, peer_endpoint) = core
            .config
            .as_ref()
            .and_then(|config| config.transport.explicit_endpoints())
            .context("setup crash requires explicit LAN sockets")?;
        let peer_retired = state
            .match_state
            .session
            .network_endpoint_source(peer_endpoint)
            .is_none()
            && state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| {
                    !service.setup_peer_cleanup_pending(peer_endpoint)
                        && service.queued_setup_count() == 0
                });
        if self.recovered.is_none()
            && core.host()
            && rendered == PresentedShell::NetworkSetup
            && setup_settled(state, true)
            && peer_retired
            && state
                .frontend
                .network_setup_protocol
                .as_ref()
                .is_some_and(|protocol| protocol.editable())
            && state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| {
                    launch.humans.len() == 1
                        && launch.local_player().name == HOST_NAME
                        && !launch.humans.iter().any(|human| human.name == GUEST_NAME)
                })
        {
            ensure!(
                u64::from(elapsed)
                    >= setup["lease_remaining"]
                        .as_u64()
                        .context("post-kill lease")?,
                "guest was removed before its actual post-kill lease elapsed"
            );
            let row = u8::try_from(setup["peer_raw_row"].as_u64().context("peer raw row")?)?;
            ensure!(
                state.match_state.session.network_vacancy(row)
                    == Some(crate::app::match_runtime::session::NetworkVacancy::Open),
                "expired guest retained its occupied setup row"
            );
            self.recovered = Some(
                json!({"fault_id":boundary.id,"kind":"guest removed; host remained editable","render_frame":frame,
                "observed_bucket":now,"elapsed_buckets":elapsed,"peer_endpoint_retired":true,
                "peer_admission_retired":true,"map_recipient_retired":true,"reliable_setup_queue_empty":true,
                "peer_raw_row":row,"peer_row_open":true,"remaining_names":[HOST_NAME]}),
            );
            setup_button(state, crate::ui::multiplayer_setup::BACK)?;
            core.event(
                frame,
                "Post-kill guest lease expired; actual host Back clicked from still-editable setup",
            );
            return Ok(true);
        }
        if rendered == PresentedShell::LanLobby
            && lobby_settled(state)
            && state.frontend.shell_exit.is_none()
            && peer_retired
            && state.frontend.network_setup_protocol.is_none()
            && state.frontend.multiplayer_setup.is_none()
            && state.match_state.session.network_setup().is_none()
            && state.match_state.session.network_map_metadata().is_none()
            && state
                .match_state
                .session
                .network_local_map_source()
                .is_none()
            && state
                .match_state
                .lan_service
                .as_ref()
                .is_some_and(|service| {
                    service.setup_scope().is_none() && !service.setup_map_work_active()
                })
        {
            ensure!(
                !core.host() || self.recovered.is_some(),
                "host skipped editable peer-loss recovery"
            );
            ensure!(
                u64::from(elapsed)
                    >= setup["lease_remaining"]
                        .as_u64()
                        .context("post-kill lease")?,
                "host was removed before its actual post-kill lease elapsed"
            );
            if self.recovered.is_none() {
                self.recovered = Some(
                    json!({"fault_id":boundary.id,"kind":"host lost; guest returned through HostLeft","render_frame":frame,
                    "observed_bucket":now,"elapsed_buckets":elapsed,"peer_endpoint_retired":true,
                    "peer_admission_retired":true,"map_recipient_retired":true,"reliable_setup_queue_empty":true}),
                );
            }
            self.returned = Some(
                json!({"fault_id":boundary.id,"render_frame":frame,"settled_lan_lobby":true,"setup_retired":true,
                "session_setup_retired":true,"map_source_retired":true,"map_transfer_retired":true,
                "setup_scope_retired":true,"live_owner_retired":true,"lan_service_retained":true,
                "loading":false,"gameplay_started":false}),
            );
            core.event(frame, "Settled actual BB after externally acknowledged kill; old setup/map/peer owners retired");
        }
        Ok(true)
    }

    pub(super) fn receipt(&self) -> Result<Value> {
        ensure!(
            self.ready(),
            "editable LAN crash checkpoint has not returned"
        );
        let boundary = self.boundary.as_ref().context("fault boundary")?;
        Ok(
            json!({"schema":"vera20k.lan-setup-crash.v1","victim":self.victim.as_str(),
            "scope":"real editable LAN setup; freshly requested admitted lease, exact owned process termination, strictly later ordinary lease cleanup and actual BB; no gameplay proof",
            "prefix":self.prefix,"fault_id":boundary.id,"ready":boundary.ready,
            "kill_observed":boundary.killed,"recovered":self.recovered,"returned":self.returned}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};

    #[test]
    fn setup_member_projection_respects_host_only_peer_map_status() {
        let profile: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.humans[0].name = HOST_NAME.into();
        let mut guest = launch.humans[0].clone();
        guest.name = GUEST_NAME.into();
        guest.raw_slot_index = Some(1);
        launch.humans.push(guest);
        launch.local_player_index = 1;
        let expected = vec![
            json!({"name":HOST_NAME,"raw_row":0}),
            json!({"name":GUEST_NAME,"raw_row":1}),
        ];

        let mut guest = MatchSession::default();
        guest.enter_network(NetworkReturnContext::LanLobby);
        guest
            .accept_network_setup(launch.clone(), false, None)
            .unwrap();
        guest.invalidate_network_agreement().unwrap();
        assert_eq!(guest.network_map_status(0), Some(0));
        assert_eq!(guest.network_map_status(1), Some(1));
        assert_eq!(setup_members(&guest, false), Some(expected.clone()));

        launch.local_player_index = 0;
        let mut host = MatchSession::default();
        host.enter_network(NetworkReturnContext::LanLobby);
        host.accept_network_setup(launch, true, None).unwrap();
        assert!(setup_members(&host, false).is_none());
        host.set_network_map_status(1, 1).unwrap();
        assert_eq!(setup_members(&host, false), Some(expected.clone()));

        guest.set_network_map_status(1, 0).unwrap();
        assert!(setup_members(&guest, false).is_none());
        assert_eq!(setup_members(&guest, true), Some(expected));
        assert_eq!(guest.network_map_status(1), Some(0));
    }
}
