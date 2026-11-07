//! Glyphs in GAME.FNT's style for the characters it lacks, so the Original
//! font option draws Cyrillic, Greek, Vietnamese, the rest of Latin and the
//! hanzi GAME.FNT has no drawing of as pixel letters beside its own, instead
//! of with the smooth bundled fonts. [`super::fnt_font`] maps them into
//! GAME.FNT's outline font, so they draw crisp at 1:1 and scale evenly.
//!
//! GAME.FNT's Latin letters are Arial Narrow Bold drawn at 13 px without
//! smoothing (49 of its 62 letters and digits pixel for pixel), with 2-pixel
//! stems; its hanzi are 1-pixel strokes in a 15x15 box on rows 1..15. It cuts
//! each glyph to its ink, from the first inked column and as wide as the ink,
//! and advances it one pixel more (each of its 312 ASCII, Latin-1 and Latin
//! Extended-A glyphs and 17,258 hanzi). It has no Cyrillic, Greek or
//! Vietnamese letters, points what it lacks of Latin Extended-B, IPA and
//! punctuation at blank placeholders, and 1,937 of GB2312's 6,763 simplified
//! hanzi at an empty box. In their place, cut the same way:
//!
//! - Cyrillic and Greek letters shaped like Latin ones take GAME.FNT's own
//!   Latin letter ([`LOOKALIKES`]); Arial draws each pair alike.
//! - Other characters of Latin size come from Noto Sans at weight 300 and
//!   width 80, drawn at 13 px without smoothing and thickened one pixel to
//!   the right, which gives them GAME.FNT's 2-pixel stems. Drawn that way it
//!   matched more of Arial Narrow Bold's own Latin, Cyrillic and Greek pixels
//!   at 13 px than the other free fonts tried (Arimo, Roboto, Archivo).
//! - Chinese, Japanese and Korean come from GNU Unifont's 1-pixel Song
//!   glyphs, trimmed to GAME.FNT's 15x15 box by `tools/unifont_cjk.py`
//!   (`src/render/fonts/README.md`).

use std::collections::HashSet;

use swash::scale::ScaleContext;
use swash::scale::outline::Outline;
use swash::text::{Category, Codepoint as _};
use swash::zeno::{Command, PathData as _};

use crate::assets::fnt_file::{FntFile, FntGlyph};

use super::fonts::{NOTO_SANS, TextClass};

/// Cyrillic and Greek letters, each with the Latin letter of the same shape
/// whose GAME.FNT glyph draws it.
const LOOKALIKES: [(u16, u16); 43] = [
    // Cyrillic А В Е К М Н О Р С Т Х Ѕ І Ј Ё Ї
    (0x0410, 0x41),
    (0x0412, 0x42),
    (0x0415, 0x45),
    (0x041A, 0x4B),
    (0x041C, 0x4D),
    (0x041D, 0x48),
    (0x041E, 0x4F),
    (0x0420, 0x50),
    (0x0421, 0x43),
    (0x0422, 0x54),
    (0x0425, 0x58),
    (0x0405, 0x53),
    (0x0406, 0x49),
    (0x0408, 0x4A),
    (0x0401, 0xCB),
    (0x0407, 0xCF),
    // Cyrillic а е о р с у х ѕ і ј ё ї
    (0x0430, 0x61),
    (0x0435, 0x65),
    (0x043E, 0x6F),
    (0x0440, 0x70),
    (0x0441, 0x63),
    (0x0443, 0x79),
    (0x0445, 0x78),
    (0x0455, 0x73),
    (0x0456, 0x69),
    (0x0458, 0x6A),
    (0x0451, 0xEB),
    (0x0457, 0xEF),
    // Greek Α Β Ε Ζ Η Ι Κ Μ Ν Ο Ρ Τ Υ Χ ο
    (0x0391, 0x41),
    (0x0392, 0x42),
    (0x0395, 0x45),
    (0x0396, 0x5A),
    (0x0397, 0x48),
    (0x0399, 0x49),
    (0x039A, 0x4B),
    (0x039C, 0x4D),
    (0x039D, 0x4E),
    (0x039F, 0x4F),
    (0x03A1, 0x50),
    (0x03A4, 0x54),
    (0x03A5, 0x59),
    (0x03A7, 0x58),
    (0x03BF, 0x6F),
];

/// Noto Sans as GAME.FNT's Latin letters were drawn (module notes): pixel
/// size, weight and width.
const NOTO_SIZE: f32 = 13.0;
const NOTO_WEIGHT: f32 = 300.0;
const NOTO_WIDTH_PERCENT: f32 = 80.0;
/// Straight segments each curve of an outline is split into.
const CURVE_STEPS: u8 = 8;

/// `tools/unifont_cjk.py`'s output: per glyph a little-endian code point,
/// then [`CJK_SIDE`] big-endian rows whose top bit is the left column.
const UNIFONT_CJK: &[u8] = include_bytes!("../fonts/Unifont-CJK-15.bin");
/// GAME.FNT's hanzi box, and Unifont's glyphs trimmed to it.
const CJK_SIDE: usize = 15;
const CJK_RECORD: usize = 2 + 2 * CJK_SIDE;
/// The hanzi box ends this many rows below the baseline: GAME.FNT fills rows
/// 1..15 around its row-13 baseline.
const CJK_DESCENT: i32 = 3;

/// GAME.FNT-style glyphs for the characters `fnt` lacks: `fnt.bitmap_rows`
/// tall like its own, with the baseline above row `baseline_row`.
pub(crate) fn glyphs(fnt: &FntFile, baseline_row: u32) -> Vec<(u16, FntGlyph)> {
    let rows = fnt.bitmap_rows as usize;
    let mut glyphs: Vec<(u16, FntGlyph)> = LOOKALIKES
        .iter()
        .filter(|&&(letter, latin)| !fnt.draws(letter) && fnt.draws(latin))
        .filter_map(|&(letter, latin)| {
            let glyph = fnt.glyph(latin)?;
            let rgba = glyph.rgba.clone();
            Some((
                letter,
                FntGlyph {
                    slot: 0,
                    width: glyph.width,
                    rgba,
                },
            ))
        })
        .collect();
    let top = baseline_row as i32 + CJK_DESCENT - CJK_SIDE as i32;
    for record in UNIFONT_CJK.chunks_exact(CJK_RECORD) {
        let codepoint = u16::from_le_bytes([record[0], record[1]]);
        if fnt.draws(codepoint) {
            continue;
        }
        let lit: HashSet<(i32, i32)> = (0..CJK_SIDE)
            .flat_map(|y| {
                let bits = u16::from_be_bytes([record[2 + 2 * y], record[3 + 2 * y]]);
                (0..CJK_SIDE)
                    .filter(move |x| bits & (0x8000 >> x) != 0)
                    .map(move |x| (x as i32, top + y as i32))
            })
            .collect();
        glyphs.push((codepoint, cropped(&lit, rows, CJK_SIDE)));
    }
    let taken: HashSet<u16> = glyphs.iter().map(|&(codepoint, _)| codepoint).collect();
    glyphs.extend(noto_glyphs(fnt, rows, baseline_row as f32, &taken));
    glyphs
}

/// The Latin-size characters `fnt` lacks and Noto Sans draws, other than
/// those `taken`, as GAME.FNT pixels (module notes); a space keeps its
/// advance at [`NOTO_SIZE`] as its width.
fn noto_glyphs(
    fnt: &FntFile,
    rows: usize,
    baseline: f32,
    taken: &HashSet<u16>,
) -> Vec<(u16, FntGlyph)> {
    let Some(font) = swash::FontRef::from_index(NOTO_SANS, 0) else {
        return Vec::new();
    };
    let coords: Vec<_> = font
        .variations()
        .normalized_coords([("wght", NOTO_WEIGHT), ("wdth", NOTO_WIDTH_PERCENT)])
        .collect();
    let advances = font.glyph_metrics(&coords).scale(NOTO_SIZE);
    let mut context = ScaleContext::new();
    let mut scaler = context
        .builder(font)
        .size(NOTO_SIZE)
        .normalized_coords(&coords)
        .build();
    let mut wanted = Vec::new();
    font.charmap().enumerate(|codepoint, id| {
        if let (Ok(code), Some(c)) = (u16::try_from(codepoint), char::from_u32(codepoint))
            && !fnt.draws(code)
            && !taken.contains(&code)
            && TextClass::of(c) == TextClass::Latin
            && stands_alone(c)
        {
            wanted.push((code, id));
        }
    });
    wanted
        .into_iter()
        .map(|(codepoint, id)| {
            let mut lit = scaler
                .scale_outline(id)
                .map(|outline| pixels(&segments(&outline), rows, baseline))
                .unwrap_or_default();
            let thickened: Vec<(i32, i32)> = lit.iter().map(|&(x, row)| (x + 1, row)).collect();
            lit.extend(thickened);
            let blank = (advances.advance_width(id).round() as usize).max(1);
            (codepoint, cropped(&lit, rows, blank))
        })
        .collect()
}

/// Whether `c` draws as a glyph of its own: not a control, format, private,
/// unassigned or line-separating code point, and not a mark on the character
/// before it (those fall back together with their letter).
fn stands_alone(c: char) -> bool {
    !matches!(
        c.category(),
        Category::Other
            | Category::Control
            | Category::Format
            | Category::Unassigned
            | Category::PrivateUse
            | Category::Surrogate
            | Category::Mark
            | Category::SpacingMark
            | Category::EnclosingMark
            | Category::NonspacingMark
            | Category::LineSeparator
            | Category::ParagraphSeparator
    )
}

/// A scaled outline (pixels, y up, the pen at the origin) as straight
/// segments `[x0, y0, x1, y1]`, each curve split into [`CURVE_STEPS`].
fn segments(outline: &Outline) -> Vec<[f32; 4]> {
    let steps = || (1..=CURVE_STEPS).map(|step| f32::from(step) / f32::from(CURVE_STEPS));
    let mut segments = Vec::new();
    let (mut start, mut pen) = ([0.0; 2], [0.0; 2]);
    for command in outline.path().commands() {
        let ends: Vec<[f32; 2]> = match command {
            Command::MoveTo(p) => {
                close_contour(&mut segments, pen, start);
                (start, pen) = ([p.x, p.y], [p.x, p.y]);
                continue;
            }
            Command::Close => {
                close_contour(&mut segments, pen, start);
                pen = start;
                continue;
            }
            Command::LineTo(p) => vec![[p.x, p.y]],
            Command::QuadTo(c, p) => steps()
                .map(|t| {
                    let u = 1.0 - t;
                    let at = |a: f32, b: f32, d: f32| u * u * a + 2.0 * u * t * b + t * t * d;
                    [at(pen[0], c.x, p.x), at(pen[1], c.y, p.y)]
                })
                .collect(),
            Command::CurveTo(c1, c2, p) => steps()
                .map(|t| {
                    let u = 1.0 - t;
                    let at = |a: f32, b: f32, c: f32, d: f32| {
                        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                    };
                    [at(pen[0], c1.x, c2.x, p.x), at(pen[1], c1.y, c2.y, p.y)]
                })
                .collect(),
        };
        for end in ends {
            segments.push([pen[0], pen[1], end[0], end[1]]);
            pen = end;
        }
    }
    close_contour(&mut segments, pen, start);
    segments
}

/// The segment from `pen` back to its contour's `start`, unless it is there.
fn close_contour(segments: &mut Vec<[f32; 4]>, pen: [f32; 2], start: [f32; 2]) {
    if pen != start {
        segments.push([pen[0], pen[1], start[0], start[1]]);
    }
}

/// Where the line `y = at` (`row`) or `x = at` runs inside `segments`, by the
/// nonzero winding rule: `[start, end]` along the line, in order.
fn inside(segments: &[[f32; 4]], at: f32, row: bool) -> Vec<[f32; 2]> {
    let mut crossings: Vec<(f32, i32)> = segments
        .iter()
        .filter_map(|&[x0, y0, x1, y1]| {
            // `a` runs across the line, `b` along it.
            let ([a0, b0], [a1, b1]) = if row {
                ([y0, x0], [y1, x1])
            } else {
                ([x0, y0], [x1, y1])
            };
            let crosses = (a0 <= at && at < a1) || (a1 <= at && at < a0);
            crosses.then(|| {
                let b = b0 + (at - a0) * (b1 - b0) / (a1 - a0);
                (b, if a1 > a0 { 1 } else { -1 })
            })
        })
        .collect();
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut stretches = Vec::new();
    let (mut winding, mut start) = (0, 0.0);
    for (b, direction) in crossings {
        if winding == 0 {
            start = b;
        }
        winding += direction;
        if winding == 0 {
            stretches.push([start, b]);
        }
    }
    stretches
}

/// The `(column, row)` pixels an outline's `segments` light in a cell `rows`
/// tall whose baseline lies `baseline` rows down, columns counted from the
/// pen: each pixel whose centre the outline covers, as an unsmoothed
/// rasterizer draws, and, where a stroke crosses a row's or column's centre
/// line between two pixel centres, the pixel at its middle (dropout
/// control), so strokes thinner than a pixel stay unbroken.
fn pixels(segments: &[[f32; 4]], rows: usize, baseline: f32) -> HashSet<(i32, i32)> {
    let mut lit = HashSet::new();
    for row in 0..rows as i32 {
        let centre = baseline - row as f32 - 0.5;
        for [start, end] in inside(segments, centre, true) {
            let (first, last) = ((start - 0.5).ceil() as i32, (end - 0.5).floor() as i32);
            if first <= last {
                lit.extend((first..=last).map(|column| (column, row)));
            } else {
                lit.insert((((start + end) / 2.0).floor() as i32, row));
            }
        }
    }
    let xs = segments.iter().flat_map(|s| [s[0], s[2]]);
    let (left, right) = xs.fold((f32::MAX, f32::MIN), |(l, r), x| (l.min(x), r.max(x)));
    for column in left.floor() as i32..right.ceil() as i32 {
        for [start, end] in inside(segments, column as f32 + 0.5, false) {
            let (first, last) = (
                (baseline - 0.5 - end).ceil() as i32,
                (baseline - 0.5 - start).floor() as i32,
            );
            let row = (baseline - (start + end) / 2.0).floor() as i32;
            if first > last && (0..rows as i32).contains(&row) {
                lit.insert((column, row));
            }
        }
    }
    lit
}

/// A GAME.FNT glyph `rows` tall, white at the `(column, row)` pixels `lit`,
/// cut as GAME.FNT cuts its own (module notes); without ink, a blank
/// `blank_width` wide.
fn cropped(lit: &HashSet<(i32, i32)>, rows: usize, blank_width: usize) -> FntGlyph {
    let inked: Vec<(i32, usize)> = lit
        .iter()
        .filter_map(|&(x, row)| Some((x, usize::try_from(row).ok().filter(|&row| row < rows)?)))
        .collect();
    let left = inked.iter().map(|&(x, _)| x).min().unwrap_or(0);
    let width = inked
        .iter()
        .map(|&(x, _)| (x - left) as usize + 1)
        .max()
        .unwrap_or(blank_width);
    let mut rgba = vec![0; width * rows * 4];
    for (x, row) in inked {
        rgba[(row * width + (x - left) as usize) * 4..][..4].fill(u8::MAX);
    }
    FntGlyph {
        slot: 0,
        width: width as u32,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use super::*;

    fn lit(glyph: &FntGlyph, x: usize, row: usize) -> bool {
        glyph.rgba[(row * glyph.width as usize + x) * 4 + 3] != 0
    }

    fn ink_rows(glyph: &FntGlyph) -> BTreeSet<usize> {
        let rows = glyph.rgba.len() / 4 / glyph.width as usize;
        (0..rows)
            .filter(|&row| (0..glyph.width as usize).any(|x| lit(glyph, x, row)))
            .collect()
    }

    /// Retail GAME.FNT's gaps fill in its style: Cyrillic 'А' is its 'A',
    /// 'П' stands on the rows of its 'H' with 2-pixel stems, the hanzi it
    /// lacks fill the box its own hanzi fill, and every character of the
    /// shipped languages in scripts it draws (all but Arabic and Thai) is
    /// its own or a fill-in.
    #[test]
    fn retail_game_fnt_gaps_fill_in_its_style() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let fnt = FntFile::from_bytes(assets.get_ref("GAME.FNT").expect("retail GAME.FNT"))
            .expect("parse retail GAME.FNT");
        let fill_ins: HashMap<u16, FntGlyph> = glyphs(&fnt, 13).into_iter().collect();

        let own = |cp: u8| fnt.glyph(u16::from(cp)).expect("GAME.FNT Latin");
        assert_eq!(fill_ins[&0x0410].rgba, own(b'A').rgba);
        let pe = &fill_ins[&0x041F];
        assert_eq!(ink_rows(pe), ink_rows(own(b'H')));
        for row in 5..=12 {
            let pixels: String = (0..pe.width as usize)
                .map(|x| if lit(pe, x, row) { '#' } else { '.' })
                .collect();
            let runs: Vec<usize> = pixels
                .split('.')
                .filter(|run| !run.is_empty())
                .map(str::len)
                .collect();
            assert_eq!(runs, [2, 2], "row {row}: {pixels}");
        }

        // Its hanzi start their ink in the first column and end it in the
        // last; those it lacks are cut the same, within the rows its '中'
        // fills.
        let hanzi = |cp: &u16| (0x4E00..=0x9FFF).contains(cp);
        let cut = |glyph: &FntGlyph| {
            let width = glyph.width as usize;
            let rows = glyph.rgba.len() / 4 / width;
            [0, width - 1]
                .iter()
                .all(|&x| (0..rows).any(|row| lit(glyph, x, row)))
        };
        let own: Vec<&FntGlyph> = fnt
            .drawn_codepoints()
            .filter(|(cp, _)| hanzi(cp))
            .map(|(_, g)| g)
            .collect();
        let filled: Vec<&FntGlyph> = fill_ins
            .iter()
            .filter(|(cp, _)| hanzi(cp))
            .map(|(_, g)| g)
            .collect();
        assert!(own.iter().chain(&filled).all(|glyph| cut(glyph)));
        let rows: BTreeSet<usize> = filled.iter().flat_map(|glyph| ink_rows(glyph)).collect();
        assert_eq!(rows, ink_rows(fnt.glyph(0x4E2D).expect("中")));
        assert!(!fnt.draws(0x4EEC) && fill_ins.contains_key(&0x4EEC), "们");

        let undrawn: BTreeSet<char> = crate::i18n::shipped_texts(|_| true)
            .into_iter()
            .filter(|(tag, _, _)| !matches!(tag.as_str(), "ar" | "th"))
            .flat_map(|(_, _, text)| text.chars().collect::<Vec<_>>())
            .filter(|&c| !c.is_control())
            .filter(|&c| {
                u16::try_from(u32::from(c)).map_or(true, |code| {
                    !fnt.draws(code) && !fill_ins.contains_key(&code)
                })
            })
            .collect();
        assert!(undrawn.is_empty(), "{undrawn:?}");
    }
}
