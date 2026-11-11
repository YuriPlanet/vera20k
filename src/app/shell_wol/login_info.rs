//! VERA online: the lobby's activity the online login page shows under its
//! username field (docs/lobby-protocol.md "Accounts"), from the lobby's
//! public `GET /v1/stats`. It is read in the background each time the page
//! opens, and a count it could not renew is dropped. gamemd's login page
//! shows no such counts.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use anyhow::Result;

use crate::app::AppState;
use crate::util::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The lobby's `/v1/stats` counts the page uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct LobbyStats {
    pub(crate) online: u32,
    pub(crate) games_24h: u32,
}

#[derive(Default)]
pub(crate) struct LoginInfo {
    /// The page opened: the next poll reads the counts again.
    wanted: bool,
    read: Option<Receiver<Result<LobbyStats>>>,
    /// The last read's counts; None before it or when it failed.
    stats: Option<LobbyStats>,
}

impl LoginInfo {
    /// The page's count rows, top first: the players online, then the last
    /// day's games, each by its text key. A count of none is left out, as
    /// vera20k.org's front page leaves it.
    pub(crate) fn counts(&self) -> impl Iterator<Item = (&'static str, u32)> {
        let stats = self.stats;
        [
            ("VERA:PlayersOnline", stats.map(|stats| stats.online)),
            ("VERA:RecentGames", stats.map(|stats| stats.games_24h)),
        ]
        .into_iter()
        .filter_map(|(key, count)| Some((key, count.filter(|count| *count > 0)?)))
    }

    /// The counts as they arrive. Captures read nothing: they set them here.
    pub(crate) fn set(&mut self, stats: Option<LobbyStats>) {
        self.stats = stats;
    }
}

/// The online login page opened: read the counts again.
pub(super) fn refresh(state: &mut AppState) {
    state.frontend.login_info.wanted = true;
}

/// Starts the read the page asked for and takes its answer.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let info = &mut state.frontend.login_info;
    // No worker threads (the browser build): no counts.
    if std::mem::take(&mut info.wanted) && info.read.is_none() {
        info.read =
            crate::app::map_library::spawn("lobby-stats", Arc::default(), |_| fetch::stats());
    }
    let answer = match info.read.as_ref()?.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return Some(Instant::now() + POLL_INTERVAL),
        Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("the read stopped")),
    };
    info.read = None;
    info.stats = answer
        .inspect_err(|error| log::info!("The lobby's counts are unavailable: {error:#}"))
        .ok();
    state.platform.window.request_redraw();
    None
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
mod fetch {
    //! The page's HTTPS read, on a worker thread only.

    use std::io::Read;
    use std::time::Duration;

    use anyhow::{Context, Result, ensure};

    use super::LobbyStats;

    const USER_AGENT: &str = concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " login");
    /// The answer is well under a kilobyte.
    const MAX_BYTES: u64 = 64 * 1024;

    pub(super) fn stats() -> Result<LobbyStats> {
        let base = crate::app::shell_wol::lobby_base_url()?;
        let url = format!("{}/v1/stats", base.trim_end_matches('/'));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(15)))
            .timeout_per_read(Some(Duration::from_secs(15)))
            .build()
            .into();
        let mut response = agent
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .with_context(|| format!("cannot reach {url}"))?;
        let status = response.status().as_u16();
        ensure!(status == 200, "{url} answered {status}");
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("cannot read {url}"))?;
        ensure!(bytes.len() as u64 <= MAX_BYTES, "{url} answered too much");
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
mod fetch {
    use anyhow::{Result, bail};

    use super::LobbyStats;

    pub(super) fn stats() -> Result<LobbyStats> {
        bail!("the browser build reads no lobby counts")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_counts_players_and_the_last_days_games_but_never_none() {
        let mut info = LoginInfo::default();
        let rows = |info: &LoginInfo| info.counts().collect::<Vec<_>>();
        assert_eq!(rows(&info), [], "nothing read yet");
        let counts = |online, games_24h| Some(LobbyStats { online, games_24h });
        info.set(counts(3, 14));
        assert_eq!(
            rows(&info),
            [("VERA:PlayersOnline", 3), ("VERA:RecentGames", 14)]
        );
        info.set(counts(0, 14));
        assert_eq!(rows(&info), [("VERA:RecentGames", 14)]);
        info.set(counts(0, 0));
        assert_eq!(rows(&info), []);
    }
}
