//! The fonts behind GAME.FNT in VERA's interface text, for what it and its
//! drawn-in letters lack, and how each script is sized against GAME.FNT.
//! Their glyphs draw in whole native pixels, as GAME.FNT's do
//! ([`super::pixels`]).
//!
//! All are Noto faces under the SIL Open Font License 1.1 (`fonts/OFL.txt`,
//! and `fonts/OFL-CJK.txt` for Noto Sans CJK), taken unmodified from
//! notofonts.github.io and github.com/notofonts/noto-cjk (`fonts/README.md`).
//! Registration order is fallback order: a character draws from the first
//! font that has it.
//!
//! Sizes are native (800x600) pixels; a draw at scale `s` scales each native
//! pixel to `s` window pixels. They were fitted to retail GAME.FNT by
//! measurement:
//! - Latin, Greek and Cyrillic: GAME.FNT's capitals are 9 rows tall ('H'
//!   rows 4..12) with 2-pixel stems, and a string advances by its glyph
//!   widths plus 1 pixel a character. Noto Sans at 12.6 px has a 9.0 px cap
//!   height; at weight 700 and width 82 it measures 0.99 of GAME.FNT's
//!   width over the 5,345 lines of retail `ra2md.csf`, so text fills the
//!   same boxes.
//! - Chinese, Japanese and Korean: GAME.FNT's hanzi are 1-pixel monoline
//!   strokes on a 14 + 1 pixel advance, which Noto Sans CJK SC Regular at
//!   15 px keeps.
//! - Other scripts have no GAME.FNT reference; they take a size that reads
//!   at the Latin text's height and weight 600.
//!
//! The browser build embeds none of them. It fetches Noto Sans, which
//! GAME.FNT's drawn-in letters need ([`super::fnt_fill`]), before the game
//! starts, and each of the others the first time text needs a character from
//! it ([`super::web_fonts`]): they would add some 30 MB to the engine every
//! player downloads, and kept apart a browser keeps them across its updates.
//!
//! An installer's build (`language_folder`, build.rs) embeds only Noto Sans
//! and reads each of the others from the languages folder the first time text
//! needs a character from it, so a font can be removed after the build. Other
//! builds embed them all.

/// One bundled font file.
pub(crate) struct BundledFont {
    /// Its name in `src/render/fonts`, and in the browser package's
    /// `pkg/fonts`.
    pub(crate) file: &'static str,
    /// Its bytes; `None` in a build that fetches it ([`super::web_fonts`]).
    pub(crate) data: Option<&'static [u8]>,
    /// Native pixels to move this font's glyphs down from the shared
    /// baseline, so its ink sits where GAME.FNT's does.
    pub(crate) baseline_shift: f32,
}

macro_rules! font {
    ($file:literal) => {
        font!($file, 0.0)
    };
    ($file:literal, $shift:expr) => {
        BundledFont {
            file: $file,
            #[cfg(not(any(
                language_folder,
                all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
            )))]
            data: Some(include_bytes!(concat!("../fonts/", $file))),
            #[cfg(any(
                language_folder,
                all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
            ))]
            data: None,
            baseline_shift: $shift,
        }
    };
}

/// Noto Sans's file name.
pub(crate) const NOTO_SANS_FILE: &str = "NotoSans-VF.ttf";

/// Noto Sans, first in [`bundled`]; [`super::fnt_fill`] also draws GAME.FNT's
/// missing Latin, Greek and Cyrillic letters from it. The browser build
/// fetches it before the game starts ([`super::web_fonts::preloaded`]).
pub(crate) fn noto_sans() -> &'static [u8] {
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    return include_bytes!("../fonts/NotoSans-VF.ttf");
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    super::web_fonts::preloaded(NOTO_SANS_FILE)
}

/// Fallback order. Noto Sans first, so Latin, Greek, Cyrillic, digits and
/// common punctuation inside any script take its forms.
pub(crate) fn bundled() -> [BundledFont; 21] {
    [
        BundledFont {
            file: NOTO_SANS_FILE,
            data: Some(noto_sans()),
            baseline_shift: 0.0,
        },
        // GAME.FNT's hanzi fill rows 1..15 around its row-13 baseline, centred
        // 4.5 px above it; Noto's ideographic box at 15 px centres 5.9 px up.
        font!("NotoSansCJKsc-Regular.otf", 1.4),
        font!("NotoSansArabicUI-VF.ttf"),
        font!("NotoSansHebrew-VF.ttf"),
        font!("NotoSansDevanagari-VF.ttf"),
        font!("NotoSansBengali-VF.ttf"),
        font!("NotoSansThai-VF.ttf"),
        font!("NotoSansTamil-VF.ttf"),
        font!("NotoSansTelugu-VF.ttf"),
        font!("NotoSansKannada-VF.ttf"),
        font!("NotoSansMalayalam-VF.ttf"),
        font!("NotoSansGujarati-VF.ttf"),
        font!("NotoSansGurmukhi-VF.ttf"),
        font!("NotoSansOriya-VF.ttf"),
        font!("NotoSansSinhala-VF.ttf"),
        font!("NotoSansKhmer-VF.ttf"),
        font!("NotoSansLao-VF.ttf"),
        font!("NotoSansMyanmar-VF.ttf"),
        font!("NotoSansArmenian-VF.ttf"),
        font!("NotoSansGeorgian-VF.ttf"),
        font!("NotoSansEthiopic-VF.ttf"),
    ]
}

/// The baseline, in native pixels below a line's top: GAME.FNT's capitals
/// end on row 12.
pub(crate) const BASELINE: f32 = 13.0;
/// The height of Latin capitals in native pixels: GAME.FNT's 'H' fills rows
/// 4..12, and Noto Sans at [`LATIN_SIZE`] stands as tall.
pub(crate) const CAP_HEIGHT: f32 = 9.0;
/// Latin, Greek and Cyrillic text size and face settings (see the module
/// notes).
pub(crate) const LATIN_SIZE: f32 = 12.6;
pub(crate) const LATIN_WEIGHT: f32 = 700.0;
pub(crate) const LATIN_WIDTH_PERCENT: f32 = 82.0;
/// Chinese, Japanese and Korean text size and weight.
pub(crate) const CJK_SIZE: f32 = 15.0;
pub(crate) const CJK_WEIGHT: f32 = 400.0;
/// Every other script's weight.
pub(crate) const OTHER_WEIGHT: f32 = 600.0;

/// How a character is sized: GAME.FNT draws two sizes, Latin-height text and
/// full-cell CJK, and other scripts take their own reading size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TextClass {
    Latin,
    Cjk,
    Other(f32),
}

impl TextClass {
    pub(crate) fn of(c: char) -> Self {
        let cp = c as u32;
        if crate::i18n::is_cjk_sized(cp) {
            return Self::Cjk;
        }
        match cp {
            // Arabic, Syriac, Thaana, N'Ko and their extensions and
            // presentation forms read at Latin height only somewhat larger.
            0x0600..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Self::Other(14.0),
            // Hebrew and its presentation forms.
            0x0590..=0x05FF | 0xFB1D..=0xFB4F => Self::Other(13.4),
            // Brahmic scripts hang their letters from a headline and stack
            // marks above and below: Devanagari through Sinhala, Thai, Lao,
            // Tibetan, Myanmar and Khmer.
            0x0900..=0x0DFF | 0x0E00..=0x0FFF | 0x1000..=0x109F | 0x1780..=0x17FF => {
                Self::Other(13.4)
            }
            // Armenian, Georgian and Ethiopic.
            0x0530..=0x058F
            | 0x10A0..=0x10FF
            | 0x1200..=0x139F
            | 0x1C90..=0x1CBF
            | 0x2D00..=0x2D2F
            | 0x2D80..=0x2DDF
            | 0xAB00..=0xAB2F => Self::Other(13.0),
            _ => Self::Latin,
        }
    }

    /// Native pixel size.
    pub(crate) fn size(self) -> f32 {
        match self {
            Self::Latin => LATIN_SIZE,
            Self::Cjk => CJK_SIZE,
            Self::Other(size) => size,
        }
    }
}
