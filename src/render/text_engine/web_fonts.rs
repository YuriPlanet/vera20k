//! The bundled fonts the browser build leaves out of its binary
//! ([`super::fonts`]). Each is fetched from the package's `pkg/fonts/`, next
//! to the engine's own files, the first time text needs a character that it
//! is the first bundled font to draw and GAME.FNT lacks; every
//! [`super::TextEngine`] then registers it in its place in the fallback order
//! and lays its text out again, so the text ends up as the desktop build
//! draws it. Until then such a character draws as the desktop build would
//! draw it without that font, usually GAME.FNT's missing-glyph box.
//!
//! Which font draws a character comes from the fonts' character maps, which
//! build.rs reads (`font_coverage.rs`): fallback picks the first font in the
//! order that maps it.
//!
//! Noto Sans and Unifont's CJK table draw GAME.FNT's missing letters, which
//! are made when its font loads, so the browser build fetches those two
//! before the game starts instead ([`preload`]): the host awaits
//! `fonts_ready()` (`crate::web`) first.
//!
//! Other builds embed every font, so nothing is fetched there.
#![cfg_attr(
    not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")),
    allow(dead_code)
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use parley::fontique::Blob;

use super::fonts;

mod coverage {
    include!(concat!(env!("OUT_DIR"), "/font_coverage.rs"));
}

/// Each bundled font's URL version and code point ranges, in fallback order.
static BUNDLED: LazyLock<Vec<(&str, &[(u32, u32)])>> = LazyLock::new(|| {
    fonts::bundled()
        .iter()
        .map(|font| {
            coverage::FONTS
                .iter()
                .find(|(file, ..)| *file == font.file)
                .map(|&(_, version, ranges)| (version, ranges))
                .expect("build.rs reads every bundled font")
        })
        .collect()
});

/// The place in [`fonts::bundled`] of the first font that maps `c`.
pub(super) fn owner(c: char) -> Option<usize> {
    let cp = c as u32;
    BUNDLED.iter().position(|(_, ranges)| {
        ranges
            .binary_search_by(|&(start, end)| {
                if end < cp {
                    std::cmp::Ordering::Less
                } else if start > cp {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    })
}

enum Fetch {
    Pending,
    Fetched(Blob<u8>),
    Failed,
}

/// Every font asked for, by its place in [`fonts::bundled`]. A font is asked
/// for once a session; one that fails stays out.
static FETCHES: Mutex<Vec<(usize, Fetch)>> = Mutex::new(Vec::new());
/// Fonts fetched so far: an engine that saw fewer looks for new ones.
static ARRIVALS: AtomicUsize = AtomicUsize::new(0);

pub(super) fn arrivals() -> usize {
    ARRIVALS.load(Ordering::Acquire)
}

/// Bundled font `place`, once fetched.
pub(super) fn fetched(place: usize) -> Option<Blob<u8>> {
    let fetches = FETCHES.lock().unwrap_or_else(PoisonError::into_inner);
    fetches.iter().find_map(|(known, fetch)| match fetch {
        Fetch::Fetched(blob) if *known == place => Some(blob.clone()),
        _ => None,
    })
}

/// Fetch bundled font `place`, unless it was asked for already.
pub(super) fn fetch(place: usize) {
    {
        let mut fetches = FETCHES.lock().unwrap_or_else(PoisonError::into_inner);
        if fetches.iter().any(|&(known, _)| known == place) {
            return;
        }
        fetches.push((place, Fetch::Pending));
    }
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    browser::fetch(place, fonts::bundled()[place].file, BUNDLED[place].0);
}

/// Bundled font `place`'s bytes have come, or `None` if fetching it failed.
fn arrive(place: usize, file: &str, bytes: Option<Vec<u8>>) {
    let fetch = match bytes {
        Some(bytes) if swash::FontRef::from_index(&bytes, 0).is_some() => {
            log::info!("Fetched the font {file} ({} bytes)", bytes.len());
            Fetch::Fetched(Blob::new(Arc::new(bytes)))
        }
        Some(_) => {
            log::warn!("The fetched {file} is not a font; its characters stay undrawn");
            Fetch::Failed
        }
        None => Fetch::Failed,
    };
    let fetched = matches!(fetch, Fetch::Fetched(_));
    let mut fetches = FETCHES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, known)) = fetches.iter_mut().find(|(known, _)| *known == place) {
        *known = fetch;
    }
    if fetched {
        ARRIVALS.fetch_add(1, Ordering::Release);
    }
}

/// The files fetched before the game starts, with their bytes once fetched.
static PRELOADS: [(&str, OnceLock<&'static [u8]>); 2] = [
    (fonts::NOTO_SANS_FILE, OnceLock::new()),
    (super::fnt_fill::UNIFONT_CJK_FILE, OnceLock::new()),
];

/// A file [`preload`] fetched. The game starts only once both have come
/// ([`preloads_arrived`]), so the text drawn in it always finds them.
pub(super) fn preloaded(file: &str) -> &'static [u8] {
    PRELOADS
        .iter()
        .find(|(known, _)| *known == file)
        .and_then(|(_, bytes)| bytes.get().copied())
        .unwrap_or_else(|| panic!("{file} is drawn from before fonts_ready() resolved"))
}

/// Whether every file [`preload`] fetches has come.
pub(crate) fn preloads_arrived() -> bool {
    PRELOADS.iter().all(|(_, bytes)| bytes.get().is_some())
}

/// The URL version of `file`: the start of its SHA-256, from build.rs.
fn version(file: &str) -> &'static str {
    coverage::FONTS
        .iter()
        .map(|&(known, version, _)| (known, version))
        .chain(coverage::TABLES.iter().copied())
        .find(|(known, _)| *known == file)
        .map(|(_, version)| version)
        .expect("build.rs versions every file in src/render/fonts")
}

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use browser::preload;

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
mod browser {
    use wasm_bindgen::prelude::*;

    // A snippet sits in `pkg/snippets/<crate>-<hash>/`, so its own URL leads
    // to `pkg/fonts/` wherever the page loads the package from.
    #[wasm_bindgen(inline_js = "export function veraFetchFont(path) {
        return fetch(new URL(`../../fonts/${path}`, import.meta.url)).then((response) => {
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            return response.arrayBuffer();
        }).then((buffer) => new Uint8Array(buffer));
    }")]
    extern "C" {
        #[wasm_bindgen(js_name = veraFetchFont)]
        fn fetch_font(path: &str) -> js_sys::Promise;
    }

    thread_local! {
        /// [`preload`]'s promise, made once.
        static PRELOAD: std::cell::OnceCell<js_sys::Promise> = const { std::cell::OnceCell::new() };
    }

    /// Fetch the files the game needs before it starts ([`super::PRELOADS`]),
    /// once: the promise resolves when both have come, and rejects if one
    /// cannot be fetched or is not what it should be.
    pub(crate) fn preload() -> js_sys::Promise {
        PRELOAD.with(|promise| {
            promise
                .get_or_init(|| {
                    let requests: Vec<_> = super::PRELOADS
                        .iter()
                        .map(|(file, _)| {
                            (
                                *file,
                                fetch_font(&format!("{file}?v={}", super::version(file))),
                            )
                        })
                        .collect();
                    wasm_bindgen_futures::future_to_promise(async move {
                        for (file, request) in requests {
                            let bytes = wasm_bindgen_futures::JsFuture::from(request)
                                .await
                                .map_err(|error| {
                                    JsValue::from_str(&format!("Could not fetch {file}: {error:?}"))
                                })?;
                            let bytes = js_sys::Uint8Array::new(&bytes).to_vec();
                            if file == super::fonts::NOTO_SANS_FILE
                                && swash::FontRef::from_index(&bytes, 0).is_none()
                            {
                                return Err(JsValue::from_str(&format!(
                                    "The fetched {file} is not a font"
                                )));
                            }
                            log::info!("Fetched {file} ({} bytes)", bytes.len());
                            let slot = super::PRELOADS
                                .iter()
                                .find(|(known, _)| *known == file)
                                .map(|(_, slot)| slot)
                                .expect("a preloaded file");
                            let _ = slot.set(Vec::leak(bytes));
                        }
                        Ok(JsValue::UNDEFINED)
                    })
                })
                .clone()
        })
    }

    /// Fetch `file`, versioned so that a browser keeps it until it changes.
    pub(super) fn fetch(place: usize, file: &'static str, version: &str) {
        let request = fetch_font(&format!("{file}?v={version}"));
        wasm_bindgen_futures::spawn_local(async move {
            let bytes = match wasm_bindgen_futures::JsFuture::from(request).await {
                Ok(bytes) => Some(js_sys::Uint8Array::new(&bytes).to_vec()),
                Err(error) => {
                    log::warn!("Could not fetch the font {file}: {error:?}");
                    None
                }
            };
            super::arrive(place, file, bytes);
        });
    }
}

#[cfg(test)]
pub(super) fn arrive_for_test(place: usize, bytes: Vec<u8>) {
    arrive(place, fonts::bundled()[place].file, Some(bytes));
}

#[cfg(test)]
pub(super) fn asked_for(place: usize) -> bool {
    let fetches = FETCHES.lock().unwrap_or_else(PoisonError::into_inner);
    fetches.iter().any(|&(known, _)| known == place)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// build.rs reads each bundled font's character map as swash, which draws
    /// the glyphs, does.
    #[test]
    fn coverage_matches_each_fonts_character_map() {
        for (font, (_, ranges)) in fonts::bundled().iter().zip(BUNDLED.iter()) {
            let data = font.data.expect("native builds embed every font");
            let font_ref = swash::FontRef::from_index(data, 0).expect("bundled font parses");
            let mut mapped = BTreeSet::new();
            font_ref.charmap().enumerate(|cp, glyph| {
                if glyph != 0 {
                    mapped.insert(cp);
                }
            });
            let read: BTreeSet<u32> = ranges
                .iter()
                .flat_map(|&(start, end)| start..=end)
                .collect();
            assert_eq!(read, mapped, "{}", font.file);
        }
    }

    /// The browser build fetches each preloaded file by its URL version;
    /// build.rs gives the glyph table one beside the fonts'.
    #[test]
    fn every_preloaded_file_has_a_url_version() {
        for (file, _) in &PRELOADS {
            assert_eq!(version(file).len(), 16, "{file}");
        }
    }
}
