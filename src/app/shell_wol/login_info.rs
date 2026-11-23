//! Public lobby activity for the VERA Online welcome dialog and account form.
//! Refresh on entry and once a minute while the chooser is open. A failed
//! read clears the count, so an unavailable lobby never appears empty.

use std::sync::mpsc::{Receiver, TryRecvError};

use anyhow::Result;

use crate::app::AppState;
use crate::util::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
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
    refresh_at: Option<Instant>,
    read: Option<Receiver<Result<LobbyStats>>>,
    /// The last read's counts; None before it or when it failed.
    stats: Option<LobbyStats>,
}

impl LoginInfo {
    pub(crate) fn players_online(&self) -> Option<u32> {
        self.stats.map(|stats| stats.online)
    }

    /// The page's count rows, top first: the players online, then the last
    /// day's games, each by its text key. A count of none is left out, as
    /// vera20k.org's front page leaves it.
    pub(crate) fn counts(&self) -> impl Iterator<Item = (&'static str, u32)> {
        let stats = self.stats;
        [
            ("VERA:PlayersOnline", self.players_online()),
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
    let info = &mut state.frontend.login_info;
    info.stats = None;
    info.wanted = true;
}

/// Starts the read the page asked for and takes its answer.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let choosing = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.login())
        .is_some_and(|login| login.choosing_entry());
    let info = &mut state.frontend.login_info;
    let now = Instant::now();
    let due = choosing && info.refresh_at.is_some_and(|deadline| now >= deadline);
    if (std::mem::take(&mut info.wanted) || due) && info.read.is_none() {
        info.read = start_read();
        info.refresh_at = info.read.as_ref().map(|_| now + REFRESH_INTERVAL);
    }
    let next = info.refresh_at.filter(|_| choosing);
    let Some(read) = &info.read else {
        return next;
    };
    let answer = match read.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return Some(Instant::now() + POLL_INTERVAL),
        Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("the read stopped")),
    };
    info.read = None;
    info.stats = answer
        .inspect_err(|error| log::info!("The lobby's counts are unavailable: {error:#}"))
        .ok();
    next
}

pub(super) const STATS_PATH: &str = "/v1/stats";
/// The answer is well under a kilobyte.
pub(super) const MAX_STATS_BYTES: u32 = 64 * 1024;

#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
fn start_read() -> Option<Receiver<Result<LobbyStats>>> {
    let (output, input) = std::sync::mpsc::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = output.send(super::live_service::browser_stats().await);
    });
    Some(input)
}

#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
fn start_read() -> Option<Receiver<Result<LobbyStats>>> {
    use anyhow::{Context, ensure};
    use std::io::Read;

    crate::app::worker::spawn("lobby-stats", Default::default(), |_| {
        let answer = super::lobby_get(STATS_PATH, None)?;
        ensure!(answer.status == 200, "/v1/stats answered {}", answer.status);
        let mut bytes = Vec::new();
        answer
            .body
            .take(u64::from(MAX_STATS_BYTES) + 1)
            .read_to_end(&mut bytes)
            .context("cannot read /v1/stats")?;
        ensure!(
            bytes.len() <= MAX_STATS_BYTES as usize,
            "/v1/stats answered too much"
        );
        Ok(serde_json::from_slice(&bytes)?)
    })
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn chooser_shows_zero_only_when_the_lobby_reports_zero() {
        let mut info = LoginInfo::default();
        assert_eq!(info.players_online(), None);
        info.set(Some(LobbyStats {
            online: 0,
            games_24h: 8,
        }));
        assert_eq!(info.players_online(), Some(0));
        info.set(Some(LobbyStats {
            online: 3,
            games_24h: 8,
        }));
        assert_eq!(info.players_online(), Some(3));
        info.set(None);
        assert_eq!(info.players_online(), None);
    }

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
