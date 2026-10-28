//! Movie subtitles from `subtitlemd.txt`.
//!
//! BinkMovie_Open `0x00432750` builds a subtitle owner for every Bink movie it
//! opens (`0x006C99D0`): the movie file's name without directory or extension
//! is looked up in `subtitlemd.txt` (`0x006C9B70`), and only when that movie
//! has cues does it allocate the 640x255 subtitle surface. After each
//! BinkDoFrame, `0x00432E40` passes `FrameNum * (60 / fps)` to `0x006C9C60`,
//! which shows every cue whose time has come (`0x006C9D40` draws it) and hides
//! the shown one when its duration has passed; `0x00433270` copies the drawn
//! text's rectangle onto the movie with colour 0 transparent.
//!
//! The file is UTF-16 after a two-byte mark (`0x006C9EC0` reads the mark and
//! only logs when it is not `FEFF`). A movie's cues sit between
//! `BeginMovie <name>` and `EndMovie`; empty lines and lines starting with `;`
//! are skipped. A cue is one `<...>` line of `Key=Value` fields, a value
//! ending at `,` or a space unless it is quoted:
//!
//! ```text
//! BeginMovie ALLIES1
//! <Time=00:00:02:50, Duration=00:00:04:00, Position=14:Center, Color=255:255:0, Text="...">
//! EndMovie
//! ```
//!
//! The English retail file holds only its mark; localized releases fill it.

use crate::util::native_string::{
    crt_wcstol, crt_wcstoul, wcs_eq_ignore_case, wcs_starts_with_ignore_case, wcstrim,
};

/// The file `Subtitle_LookupByKey` `0x006C9B70` opens through the loose-file
/// and MIX search.
pub(crate) const SUBTITLE_FILE: &str = "subtitlemd.txt";

/// The subtitle surface `0x006C99D0` allocates (`BSurface(640, 255, 2)`).
pub(crate) const BAND_WIDTH: i32 = 640;
pub(crate) const BAND_HEIGHT: i32 = 255;

/// `0x006CA940` (and its inline copy at `0x006CA00D`) store at most this
/// many units per line.
const LINE_UNITS: usize = 0x400;

const SPACE: u16 = b' ' as u16;
const SEMICOLON: u16 = b';' as u16;
const EQUALS: u16 = b'=' as u16;
const QUOTE: u16 = b'"' as u16;
const COMMA: u16 = b',' as u16;
const COLON: u16 = b':' as u16;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubtitleAlign {
    Left,
    Right,
    Center,
}

/// One `SubTitle` (0x18 bytes) with its constructor's defaults
/// (`0x006C9900`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Subtitle {
    /// Shown from this movie time, in 1/60 s (+0x0).
    pub(crate) time: u32,
    /// Shown for this long, in 1/60 s (+0x4).
    pub(crate) duration: u32,
    /// `0xRRGGBB` (+0x8).
    pub(crate) color: u32,
    /// One-based text line from the top of the band (+0xC).
    pub(crate) line: i32,
    /// (+0x10)
    pub(crate) align: SubtitleAlign,
    /// (+0x14); `None` when the cue has no `Text=`.
    pub(crate) text: Option<String>,
}

impl Default for Subtitle {
    fn default() -> Self {
        Self {
            time: 0,
            duration: 1200,
            color: 0x00FF_FFFF,
            line: 15,
            align: SubtitleAlign::Center,
            text: None,
        }
    }
}

impl Subtitle {
    /// The cue's colour as bytes.
    pub(crate) fn rgb(&self) -> [u8; 3] {
        [
            (self.color >> 16) as u8,
            (self.color >> 8) as u8,
            self.color as u8,
        ]
    }
}

/// The file's units after the mark, a line at a time (`0x006CA940`): up to
/// `\n` or 0x400 stored units, `\r` dropped.
struct Lines<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Lines<'_> {
    /// The next line, and whether the file ended while reading it.
    fn next_line(&mut self) -> (Vec<u16>, bool) {
        let mut line = Vec::new();
        while line.len() < LINE_UNITS {
            let Some(pair) = self.bytes.get(self.pos..self.pos + 2) else {
                return (line, true);
            };
            self.pos += 2;
            let unit = u16::from_le_bytes([pair[0], pair[1]]);
            if unit == u16::from(b'\n') {
                break;
            }
            if unit != u16::from(b'\r') {
                line.push(unit);
            }
        }
        (line, false)
    }
}

/// The cues `0x006C9F10` collects for `movie` (the movie file's name without
/// directory or extension, compared case-insensitively); `None` when the file
/// names no such movie or the movie has no cue.
pub(crate) fn movie_subtitles(file: &[u8], movie: &str) -> Option<Vec<Subtitle>> {
    // mbstowcs(name, fname, 0x20)
    let movie: Vec<u16> = movie.encode_utf16().take(0x20).collect();
    let (begin, end) = (wide("BeginMovie"), wide("EndMovie"));
    let mut lines = Lines {
        bytes: file,
        pos: 2,
    };
    loop {
        let (line, ended) = lines.next_line();
        let line = wcstrim(&line);
        if line.is_empty() || line[0] == SEMICOLON {
            if ended {
                return None;
            }
            continue;
        }
        if !wcs_starts_with_ignore_case(line, &begin) {
            continue;
        }
        let Some(space) = line.iter().position(|unit| *unit == SPACE) else {
            continue;
        };
        if wcs_eq_ignore_case(wcstrim(&line[space..]), &movie) {
            break;
        }
    }
    let mut cues = Vec::new();
    loop {
        let (line, ended) = lines.next_line();
        let line = wcstrim(&line);
        if !line.is_empty() && line[0] != SEMICOLON {
            if line[0] == u16::from(b'<') && line.last() == Some(&u16::from(b'>')) {
                let fields = wcstrim(&line[1..line.len() - 1]);
                let mut cue = Subtitle::default();
                if !fields.is_empty() && parse_fields(fields, &mut cue) {
                    cues.push(cue);
                }
            } else if wcs_starts_with_ignore_case(line, &end) {
                break;
            }
        }
        // RESIDUAL: the native cue loop (0x006CA00D) never tests the end of
        // the file, so a movie without EndMovie hangs gamemd; VERA ends the
        // movie's cues there.
        if ended {
            break;
        }
    }
    (!cues.is_empty()).then_some(cues)
}

/// `0x006CA310`: the `Key=Value` fields of one cue. `false` (the cue is
/// dropped) for a field without `=`, an unclosed quote or an empty key;
/// unknown keys are ignored.
fn parse_fields(fields: &[u16], cue: &mut Subtitle) -> bool {
    let mut rest = fields;
    loop {
        let Some(equals) = rest.iter().position(|unit| *unit == EQUALS) else {
            return false;
        };
        let key = wcstrim(&rest[..equals]);
        let after = wcstrim(&rest[equals + 1..]);
        let (value, next) = if after.first() == Some(&QUOTE) {
            let Some(close) = after[1..].iter().position(|unit| *unit == QUOTE) else {
                return false;
            };
            let tail = wcstrim(&after[close + 2..]);
            (
                &after[1..close + 1],
                tail.strip_prefix(&[COMMA]).unwrap_or(tail),
            )
        } else {
            match after.iter().position(|unit| matches!(*unit, COMMA | SPACE)) {
                Some(end) => (&after[..end], &after[end + 1..]),
                None => (after, &[][..]),
            }
        };
        if key.is_empty() {
            return false;
        }
        apply_field(key, value, cue);
        rest = wcstrim(next);
        if rest.is_empty() {
            return true;
        }
    }
}

/// The token table at `0x008421A8`: Time `0x006CA670`, Duration
/// `0x006CA680`, Position `0x006CA6A0`, Color `0x006CA730`, Text
/// `0x006CA7B0`; the first name equal to `key` ignoring case wins.
fn apply_field(key: &[u16], value: &[u16], cue: &mut Subtitle) {
    let is = |name: &str| wcs_eq_ignore_case(key, &wide(name));
    if is("Time") {
        cue.time = clock(value);
    } else if is("Duration") {
        let duration = clock(value);
        if duration > 0 {
            cue.duration = duration;
        }
    } else if is("Position") {
        // `line:align`; without the colon the whole value is the alignment.
        let mut align = value;
        if let Some(colon) = value.iter().position(|unit| *unit == COLON) {
            cue.line = crt_wcstol(&value[..colon], 0);
            align = &value[colon + 1..];
        }
        // Table 0x008421D8: Left 0, Right 1, Center 2; anything else is 2.
        cue.align = if wcs_eq_ignore_case(align, &wide("Left")) {
            SubtitleAlign::Left
        } else if wcs_eq_ignore_case(align, &wide("Right")) {
            SubtitleAlign::Right
        } else {
            SubtitleAlign::Center
        };
    } else if is("Color") {
        // `R:G:B`, each `wcstoul(.., 10)` kept as a byte (0x006C9940).
        let mut parts = colon_fields(value);
        let [r, g, b] = [(); 3].map(|_| parts.next().map_or(0, |part| crt_wcstoul(part, 10)) as u8);
        cue.color = u32::from_be_bytes([0, r, g, b]);
    } else if is("Text") {
        cue.text = Some(String::from_utf16_lossy(value));
    }
}

/// Splits on `:` as the native readers do with successive `wcschr`.
/// RESIDUAL: a missing `:` makes gamemd write through NULL; VERA reads the
/// missing fields as 0.
fn colon_fields(value: &[u16]) -> impl Iterator<Item = &[u16]> {
    value.split(|unit| *unit == COLON)
}

/// `0x006CA5A0`: `HH:MM:SS:CC` (CC in hundredths) from the value's first 11
/// units, in 1/60 s: `CC * 60 / 100 + ((HH * 60 + MM) * 60 + SS) * 60`.
fn clock(value: &[u16]) -> u32 {
    let mut parts = colon_fields(&value[..value.len().min(11)]);
    let [hours, minutes, seconds, hundredths] =
        [(); 4].map(|_| parts.next().map_or(0, |part| crt_wcstoul(part, 10)));
    let whole = hours
        .wrapping_mul(60)
        .wrapping_add(minutes)
        .wrapping_mul(60)
        .wrapping_add(seconds)
        .wrapping_mul(60);
    (hundredths.wrapping_mul(60) / 100).wrapping_add(whole)
}

/// One movie's cues and the one on screen: `0x006C99D0`'s owner (list +0x0,
/// next index +0x4, shown cue +0x8).
pub(crate) struct SubtitleTrack {
    cues: Vec<Subtitle>,
    next: usize,
    shown: Option<usize>,
    /// BinkMovie_Open's `60 / (FrameRate / FrameRateDiv)` (+0x24).
    ticks_per_frame: u32,
}

impl SubtitleTrack {
    /// RESIDUAL: a movie slower than one frame per second makes gamemd divide
    /// by zero there; VERA shows no subtitles for it.
    pub(crate) fn new(cues: Vec<Subtitle>, fps_num: u32, fps_den: u32) -> Option<Self> {
        let fps = fps_num.checked_div(fps_den)?;
        Some(Self {
            cues,
            next: 0,
            shown: None,
            ticks_per_frame: 60u32.checked_div(fps)?,
        })
    }

    /// `0x006C9C60` after BinkDoFrame of the one-based Bink `frame`.
    pub(crate) fn advance(&mut self, frame: u32) {
        let now = frame.wrapping_mul(self.ticks_per_frame);
        while self.cues.get(self.next).is_some_and(|cue| cue.time <= now) {
            self.shown = Some(self.next);
            self.next += 1;
        }
        if self
            .shown()
            .is_some_and(|cue| cue.time.wrapping_add(cue.duration) <= now)
        {
            self.shown = None;
        }
    }

    pub(crate) fn shown(&self) -> Option<&Subtitle> {
        self.shown.map(|index| &self.cues[index])
    }
}

/// Where `0x006C9D40` draws a cue whose text measures `text_width` wide, and
/// `0x00433270` copies it: relative to the movie's top-left, with the band
/// centred on the movie and bottom-aligned to it.
pub(crate) fn text_origin(
    cue: &Subtitle,
    text_width: i32,
    line_height: i32,
    movie_w: i32,
    movie_h: i32,
) -> [i32; 2] {
    let x = match cue.align {
        SubtitleAlign::Left => 0,
        SubtitleAlign::Right => BAND_WIDTH - text_width,
        SubtitleAlign::Center => (BAND_WIDTH - text_width) / 2,
    };
    [
        (movie_w - BAND_WIDTH) / 2 + x,
        movie_h - BAND_HEIGHT + cue.line.wrapping_sub(1).wrapping_mul(line_height),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    #[test]
    fn a_movie_section_yields_its_cues_with_native_defaults() {
        let text = "; comment\r\nBeginMovie OTHER\r\n<Text=\"no\">\r\nEndMovie\r\n\
                    beginmovie  allies1 \r\n\
                    <Time=00:01:02:50, Duration=00:00:04:00, Position=14:Left, Color=255:128:0, Text=\"Hold, then go\">\r\n\
                    <Text=\"defaults\">\r\n\
                    <Time 00:00:01:00>\r\n\
                    ENDMOVIE\r\n<Text=\"after\">\r\n";
        let cues = movie_subtitles(&file(text), "ALLIES1").unwrap();
        assert_eq!(cues.len(), 2, "the field without '=' drops its cue");
        assert_eq!(
            cues[0],
            Subtitle {
                time: (62 * 60) + 30,
                duration: 240,
                color: 0xFF8000,
                line: 14,
                align: SubtitleAlign::Left,
                text: Some("Hold, then go".into()),
            }
        );
        assert_eq!(cues[1].text.as_deref(), Some("defaults"));
        assert_eq!((cues[1].duration, cues[1].line), (1200, 15));
        assert_eq!(cues[1].align, SubtitleAlign::Center);
        assert!(movie_subtitles(&file(text), "SOV01").is_none());
    }

    #[test]
    fn the_shown_cue_follows_bink_frame_time() {
        let cue = |time, duration| Subtitle {
            time,
            duration,
            ..Subtitle::default()
        };
        // 15 fps: four ticks per frame.
        let mut track =
            SubtitleTrack::new(vec![cue(8, 8), cue(8, 100), cue(40, 4)], 15, 1).unwrap();
        track.advance(1);
        assert!(track.shown().is_none());
        // Both cues due at tick 8 are drawn in turn; the later one stays.
        track.advance(2);
        assert_eq!(track.shown().unwrap().duration, 100);
        track.advance(10);
        assert_eq!(track.shown().unwrap().time, 40);
        // Hidden once time + duration has passed, before any later cue.
        track.advance(11);
        assert!(track.shown().is_none());
    }

    #[test]
    fn text_sits_on_its_line_in_the_band_at_the_movie_bottom() {
        let cue = Subtitle {
            line: 14,
            align: SubtitleAlign::Right,
            ..Subtitle::default()
        };
        // 800x600 movie: band at (80, 345); line 14 of 17 px rows.
        assert_eq!(text_origin(&cue, 100, 17, 800, 600), [80 + 540, 345 + 221]);
        let centred = Subtitle::default();
        assert_eq!(text_origin(&centred, 101, 17, 640, 480), [269, 225 + 238]);
    }
}
