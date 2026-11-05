//! VERA online: what the online login page's info pane shows beside its
//! buttons (docs/lobby-protocol.md "Accounts"): the lobby's activity from
//! its public `GET /v1/stats`, and vera20k.org's newest news posts in the
//! game's language from `GET /news.json?lang=`. Both are read in the
//! background each time the page opens; the pane keeps the last news it
//! had meanwhile and drops a count it could not renew. gamemd's login page
//! has no such pane.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use anyhow::Result;

use crate::app::AppState;
use crate::util::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// The feed's newest posts the pane can show.
const NEWS_POSTS: usize = 5;
const TITLE_LIMIT: usize = 160;
const SUMMARY_LIMIT: usize = 400;

/// The lobby's `/v1/stats` counts the pane uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct LobbyStats {
    pub(crate) online: u32,
    pub(crate) games_24h: u32,
}

/// What the pane says of the lobby, as vera20k.org's front page does: the
/// players online, or while nobody is, the last day's games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Activity {
    Online(u32),
    RecentGames(u32),
}

/// One news post, in the game's language where the website has it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct NewsPost {
    pub(crate) date: String,
    pub(crate) title: String,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum News {
    #[default]
    Loading,
    Unavailable,
    Posts(Vec<NewsPost>),
}

#[derive(Default)]
pub(crate) struct LoginInfo {
    /// The page opened: the next poll reads both again.
    wanted: bool,
    stats_read: Option<Receiver<Result<LobbyStats>>>,
    news_read: Option<Receiver<Result<Vec<NewsPost>>>>,
    /// The last read's counts; None before it or when it failed.
    stats: Option<LobbyStats>,
    news: News,
}

impl LoginInfo {
    pub(crate) fn activity(&self) -> Option<Activity> {
        let stats = self.stats?;
        if stats.online > 0 {
            Some(Activity::Online(stats.online))
        } else {
            (stats.games_24h > 0).then_some(Activity::RecentGames(stats.games_24h))
        }
    }

    pub(crate) fn news(&self) -> &News {
        &self.news
    }

    /// The pane's contents as they arrive. Captures read nothing: they
    /// fill the pane here.
    pub(crate) fn set(&mut self, stats: Option<LobbyStats>, news: News) {
        self.stats = stats;
        self.news = news;
    }
}

/// The online login page opened: read the counts and the news again.
pub(super) fn refresh(state: &mut AppState) {
    state.frontend.login_info.wanted = true;
}

/// Starts the reads the page asked for and takes their answers.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let info = &mut state.frontend.login_info;
    if std::mem::take(&mut info.wanted) {
        let cancel = Arc::default;
        if info.stats_read.is_none() {
            info.stats_read =
                crate::app::map_library::spawn("lobby-stats", cancel(), |_| fetch::stats());
        }
        if info.news_read.is_none() {
            let language = crate::i18n::selected_language();
            info.news_read =
                crate::app::map_library::spawn("news", cancel(), move |_| fetch::news(language));
        }
        // No worker threads (the browser build): no counts and no news.
        if info.news_read.is_none() && info.news == News::Loading {
            info.news = News::Unavailable;
        }
    }
    let mut changed = false;
    if let Some(answer) = take(&mut info.stats_read) {
        info.stats = answer
            .inspect_err(|error| log::info!("The lobby's counts are unavailable: {error:#}"))
            .ok();
        changed = true;
    }
    if let Some(answer) = take(&mut info.news_read) {
        match answer {
            Ok(posts) => info.news = News::Posts(posts),
            Err(error) => {
                log::info!("The news is unavailable: {error:#}");
                if info.news == News::Loading {
                    info.news = News::Unavailable;
                }
            }
        }
        changed = true;
    }
    let reading = info.stats_read.is_some() || info.news_read.is_some();
    if changed {
        state.platform.window.request_redraw();
    }
    reading.then(|| Instant::now() + POLL_INTERVAL)
}

/// A finished read's answer; a worker that stopped without one failed.
fn take<T>(read: &mut Option<Receiver<Result<T>>>) -> Option<Result<T>> {
    let answer = match read.as_ref()?.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return None,
        Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("the read stopped")),
    };
    *read = None;
    Some(answer)
}

#[derive(serde::Deserialize)]
struct Feed {
    posts: Vec<NewsPost>,
}

/// The feed's posts, newest first as it lists them, at most the pane's.
fn read_feed(body: &[u8]) -> Result<Vec<NewsPost>> {
    let feed: Feed = serde_json::from_slice(body)?;
    let clip = |text: String, limit: usize| text.chars().take(limit).collect::<String>();
    Ok(feed
        .posts
        .into_iter()
        .take(NEWS_POSTS)
        .map(|post| NewsPost {
            date: clip(post.date, 10),
            title: clip(post.title, TITLE_LIMIT),
            summary: clip(post.summary, SUMMARY_LIMIT),
        })
        .collect())
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
mod fetch {
    //! The pane's HTTPS reads, on worker threads only.

    use std::io::Read;
    use std::time::Duration;

    use anyhow::{Context, Result, ensure};

    use super::{LobbyStats, NewsPost};

    const USER_AGENT: &str = concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " login");
    /// Either answer is well under a kilobyte or two.
    const MAX_BYTES: u64 = 64 * 1024;

    fn get(url: &str) -> Result<Vec<u8>> {
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
            .get(url)
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
        Ok(bytes)
    }

    pub(super) fn stats() -> Result<LobbyStats> {
        let base = crate::app::shell_wol::lobby_base_url()?;
        let url = format!("{}/v1/stats", base.trim_end_matches('/'));
        Ok(serde_json::from_slice(&get(&url)?)?)
    }

    /// The website answers in English for a language it lacks.
    pub(super) fn news(language: &str) -> Result<Vec<NewsPost>> {
        let url = format!("{}news.json?lang={language}", crate::app::web_link::WEBSITE);
        super::read_feed(&get(&url)?)
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
mod fetch {
    use anyhow::{Result, bail};

    use super::{LobbyStats, NewsPost};

    pub(super) fn stats() -> Result<LobbyStats> {
        bail!("the browser build reads no lobby counts")
    }

    pub(super) fn news(_language: &str) -> Result<Vec<NewsPost>> {
        bail!("the browser build reads no news")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pane_counts_players_and_else_the_last_days_games() {
        let mut info = LoginInfo::default();
        assert_eq!(info.activity(), None, "nothing read yet");
        let counts = |online, games_24h| Some(LobbyStats { online, games_24h });
        info.set(counts(3, 14), News::Loading);
        assert_eq!(info.activity(), Some(Activity::Online(3)));
        info.set(counts(0, 14), News::Loading);
        assert_eq!(info.activity(), Some(Activity::RecentGames(14)));
        info.set(counts(0, 0), News::Loading);
        assert_eq!(info.activity(), None);
    }

    #[test]
    fn the_feed_keeps_its_newest_posts_and_ignores_other_fields() {
        let post = |n: usize| {
            serde_json::json!({
                "date": "2026-11-03",
                "title": format!("Post {n}"),
                "summary": "x".repeat(SUMMARY_LIMIT + 9),
                "slug": "ignored",
            })
        };
        let body = serde_json::json!({ "posts": (0..7).map(post).collect::<Vec<_>>() });
        let posts = read_feed(body.to_string().as_bytes()).unwrap();
        assert_eq!(posts.len(), NEWS_POSTS);
        assert_eq!(posts[0].title, "Post 0");
        assert_eq!(posts[0].summary.len(), SUMMARY_LIMIT);
        assert!(read_feed(b"{\"news\": []}").is_err());
        let stats: LobbyStats = serde_json::from_str(
            r#"{"online":2,"in_game":0,"open_games":1,"games_24h":14,"games_total":120}"#,
        )
        .unwrap();
        assert_eq!(
            stats,
            LobbyStats {
                online: 2,
                games_24h: 14
            }
        );
    }
}
