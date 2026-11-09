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
//! Other builds embed every font, so nothing is fetched there.
#![cfg_attr(
    not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")),
    allow(dead_code)
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

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
}
