//! Web pages the game opens in the player's own browser: the updater's
//! download pages and, from the online lobby's player card, a player's
//! profile on vera20k.org. Also a folder in the system's file manager: the
//! Mods page's Open Mods Folder.

/// VERA20k's website: the download page for a release that needs a newer
/// updater, the online login page's news, and the players' profiles.
pub(crate) const WEBSITE: &str = "https://vera20k.org/";

/// The player's ranked profile on the website, for a lobby player name
/// ([`crate::ui::wol_account::valid_name`], so the path needs no escapes),
/// in the game's language ([`crate::i18n::selected_language`]).
pub(crate) fn profile_url(name: &str) -> Option<String> {
    profile_url_in(&crate::i18n::selected_language(), name)
}

/// vera20k.org's languages besides English, by VERA's language tag, with the
/// prefix their pages live under (the website's `LOCALES`, site-locales.mjs).
const WEBSITE_LANGUAGES: [(&str, &str); 9] = [
    ("zh-CN", "zh/"),
    ("de", "de/"),
    ("es", "es/"),
    ("pt-BR", "pt-br/"),
    ("sv", "sv/"),
    ("tr", "tr/"),
    ("ru", "ru/"),
    ("ar", "ar/"),
    ("th", "th/"),
];

/// The profile in `language`: under the website's prefix for it, and in
/// English, at the root, for a language the website lacks.
fn profile_url_in(language: &str, name: &str) -> Option<String> {
    let prefix = WEBSITE_LANGUAGES
        .iter()
        .find(|(tag, _)| tag.eq_ignore_ascii_case(language))
        .map_or("", |(_, prefix)| prefix);
    crate::ui::wol_account::valid_name(name).then(|| format!("{WEBSITE}{prefix}players/{name}"))
}

/// Show `folder` in the system's file manager. False where none could be
/// started: the browser build has none.
pub(crate) fn open_folder(folder: &std::path::Path) -> bool {
    let folder = folder.to_string_lossy();
    // Explorer takes a path with `/` in it for no path and shows Documents.
    #[cfg(target_os = "windows")]
    let folder = folder.replace('/', "\\");
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    return open_with_system(&folder);
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        log::warn!("Cannot open {folder}");
        false
    }
}

/// Hand `target`, a web address or a folder, to the system's opener. False
/// when the opener could not be started.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn open_with_system(target: &str) -> bool {
    use std::process::{Command, Stdio};
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let opened = Command::new(opener)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match opened {
        Ok(mut child) => {
            log::info!("Opening {target}");
            // xdg-open may run the browser itself: reap it when it ends.
            let _ = std::thread::Builder::new()
                .name("vera-web-link".into())
                .spawn(move || child.wait());
            true
        }
        Err(error) => {
            log::warn!("Cannot open {target}: {error}");
            false
        }
    }
}

/// Open `url` in the player's browser: the system's opener on the desktop, a
/// new tab in the browser build.
pub(crate) fn open_in_browser(url: &str) {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    open_with_system(url);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_opens_the_profile_in_its_language_where_the_website_has_it() {
        assert_eq!(
            profile_url_in("zh-CN", "Tanya").as_deref(),
            Some("https://vera20k.org/zh/players/Tanya")
        );
        assert_eq!(
            profile_url_in("pt-BR", "Tanya").as_deref(),
            Some("https://vera20k.org/pt-br/players/Tanya")
        );
        assert_eq!(
            profile_url_in("fr", "Tanya").as_deref(),
            Some("https://vera20k.org/players/Tanya")
        );
    }
}
