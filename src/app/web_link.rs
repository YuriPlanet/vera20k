//! Web pages the game opens in the player's own browser: the updater's
//! download pages and, from the online lobby's player card, a player's
//! profile on vera20k.org.

/// VERA20k's website: the download page for a release that needs a newer
/// updater, the online login page's news, and the players' profiles.
pub(crate) const WEBSITE: &str = "https://vera20k.org/";

/// The player's ranked profile on the website, for a lobby player name
/// ([`crate::ui::wol_account::valid_name`], so the path needs no escapes).
pub(crate) fn profile_url(name: &str) -> Option<String> {
    crate::ui::wol_account::valid_name(name).then(|| format!("{WEBSITE}players/{name}"))
}

/// Open `url` in the player's browser: the system's opener on the desktop, a
/// new tab in the browser build.
pub(crate) fn open_in_browser(url: &str) {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        use std::process::{Command, Stdio};
        let opener = if cfg!(target_os = "windows") {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let opened = Command::new(opener)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match opened {
            Ok(mut child) => {
                log::info!("Opening {url}");
                // xdg-open may run the browser itself: reap it when it ends.
                let _ = std::thread::Builder::new()
                    .name("vera-web-link".into())
                    .spawn(move || child.wait());
            }
            Err(error) => log::warn!("Cannot open {url}: {error}"),
        }
    }
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    if web_sys::window()
        .and_then(|window| window.open_with_url_and_target(url, "_blank").ok())
        .is_none()
    {
        log::warn!("Cannot open {url}");
    }
    #[cfg(all(not(feature = "wasm"), target_arch = "wasm32", target_os = "unknown"))]
    log::warn!("Cannot open {url}");
}
