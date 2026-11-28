//! Character-width classification shared by the text renderer and the
//! client's chat translation.

/// The characters GAME.FNT draws on its full-cell grid and Noto Sans CJK on
/// its ideographic em: CJK ideographs, kana, Hangul, Bopomofo, CJK symbols and
/// punctuation, and the fullwidth and halfwidth forms.
pub fn is_cjk_sized(cp: u32) -> bool {
    matches!(
        cp,
        0x1100..=0x11FF
            | 0x2E80..=0x2FDF
            | 0x2FF0..=0x31FF
            | 0x3200..=0x9FFF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7FF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
            | 0x20000..=0x3FFFF
    )
}
