//! Retail movie subtitle tables (`subtitlemd.txt` / `subtitle.txt`).
//!
//! The files are UTF-16LE with a BOM. Each `BeginMovie` section names a BIK
//! stem; entries carry a `HH:MM:SS:CC` start and duration (the last field is
//! hundredths of a second, not a video frame number), plus position, RGB and
//! quoted text. A zero duration means "until the next entry", as in retail.
//!
//! The shipped Simplified Chinese tables contain two tolerated authoring
//! deviations: optional `/` prefixes on control lines and six entries missing
//! the comma between `Color` and `Text`. Malformed sections are skipped rather
//! than making movie playback fail.

use std::collections::HashMap;

/// One timed subtitle cue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleCue {
    pub start: SubtitleTime,
    /// Zero means the cue remains visible until the next cue.
    pub duration: SubtitleTime,
    pub position: SubtitlePosition,
    pub color: [u8; 3],
    pub text: String,
}

/// A retail subtitle timestamp. The last field is hundredths of a second;
/// `gamemd`'s parser at `0x006CA5A0` converts it as
/// `whole_seconds * 60 + floor(hundredths * 60 / 100)` in 60 Hz ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubtitleTime {
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub hundredths: u32,
}

impl SubtitleTime {
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }

    pub fn seconds(self) -> f64 {
        f64::from(self.hours) * 3600.0
            + f64::from(self.minutes) * 60.0
            + f64::from(self.seconds)
            + f64::from(self.hundredths) / 100.0
    }
}

/// Horizontal anchor in a retail cue's `Position=slot:anchor` value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubtitlePosition {
    Left,
    #[default]
    Center,
    Right,
}

/// Parsed subtitle table, indexed by normalized movie stem.
#[derive(Clone, Debug, Default)]
pub struct SubtitleTable {
    movies: HashMap<String, Vec<SubtitleCue>>,
}

impl SubtitleTable {
    /// Combine both retail tables, keeping YR's version of shared movie stems.
    /// A missing or malformed file does not discard the other game's subtitles.
    pub fn from_retail_files(yr: Option<&[u8]>, ra2: Option<&[u8]>) -> Option<Self> {
        let primary = yr.and_then(Self::parse_utf16le);
        let fallback = ra2.and_then(Self::parse_utf16le);
        match (primary, fallback) {
            (Some(mut primary), Some(fallback)) => {
                for (stem, cues) in fallback.movies {
                    primary.movies.entry(stem).or_insert(cues);
                }
                Some(primary)
            }
            (primary, fallback) => primary.or(fallback),
        }
    }

    /// Parse a UTF-16 subtitle file. Returns `None` for a missing BOM or an
    /// even-length byte stream that is not valid UTF-16LE.
    pub fn parse_utf16le(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 2 || bytes[0..2] != [0xFF, 0xFE] || bytes.len() % 2 != 0 {
            return None;
        }
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let text = String::from_utf16(&units).ok()?;
        Some(Self::parse_str(&text))
    }

    /// Parse already-decoded text. This is public within the crate so tests and
    /// future loose-file overrides use exactly the same rules.
    pub fn parse_str(text: &str) -> Self {
        let mut table = Self::default();
        let mut current: Option<(String, Vec<SubtitleCue>)> = None;
        for raw in text.lines() {
            let line = clean_line(raw);
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if let Some(name) = line
                .strip_prefix("BeginMovie")
                .or_else(|| line.strip_prefix("beginmovie"))
            {
                if let Some((stem, cues)) = current.take()
                    && !cues.is_empty()
                {
                    table.movies.insert(normalize_stem(&stem), cues);
                }
                let stem = name.trim().trim_matches('"');
                current = (!stem.is_empty()).then(|| (stem.to_owned(), Vec::new()));
                continue;
            }
            if line.eq_ignore_ascii_case("EndMovie") {
                if let Some((stem, cues)) = current.take()
                    && !cues.is_empty()
                {
                    table.movies.insert(normalize_stem(&stem), cues);
                }
                continue;
            }
            if line.starts_with("Font=") || line.to_ascii_lowercase().starts_with("font=") {
                continue;
            }
            let Some((_, cues)) = current.as_mut() else {
                continue;
            };
            if let Some(cue) = parse_cue(&line) {
                cues.push(cue);
            }
        }
        if let Some((stem, cues)) = current
            && !cues.is_empty()
        {
            table.movies.insert(normalize_stem(&stem), cues);
        }
        for cues in table.movies.values_mut() {
            cues.sort_by_key(|cue| cue.start);
        }
        table
    }

    /// Cue list for a movie stem such as `A00_F00E`, ignoring `.BIK` and case.
    pub fn cues_for(&self, movie_stem: &str) -> Option<&[SubtitleCue]> {
        self.movies
            .get(&normalize_stem(movie_stem))
            .map(Vec::as_slice)
    }

    /// The cue visible at `elapsed_seconds` for a movie at `fps`.
    ///
    /// A zero-duration cue ends at the next authored cue, or stays visible to
    /// EOF when it is last. Cues at the same start time keep file order.
    pub fn cue_at(&self, movie_stem: &str, elapsed_seconds: f64) -> Option<&SubtitleCue> {
        let cues = self.cues_for(movie_stem)?;
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            return None;
        }
        cues.iter().enumerate().rev().find_map(|(index, cue)| {
            let start = cue.start.seconds();
            if elapsed_seconds + f64::EPSILON < start {
                return None;
            }
            let end = if cue.duration.is_zero() {
                cues.get(index + 1)
                    .map_or(f64::INFINITY, |next| next.start.seconds())
            } else {
                start + cue.duration.seconds()
            };
            (elapsed_seconds < end).then_some(cue)
        })
    }
}

fn clean_line(raw: &str) -> &str {
    let line = raw.trim().trim_start_matches('\u{feff}').trim_start();
    line.strip_prefix('/').unwrap_or(line).trim()
}

fn normalize_stem(stem: &str) -> String {
    stem.split('.')
        .next()
        .unwrap_or(stem)
        .trim()
        .to_ascii_uppercase()
}

fn parse_cue(line: &str) -> Option<SubtitleCue> {
    let body = line.strip_prefix('<')?.strip_suffix('>')?;
    let time = value(body, "Time")?;
    let duration = value(body, "Duration")?;
    let position = value(body, "Position")?;
    let (color, text) = color_and_text(body)?;
    let (_, anchor) = position.rsplit_once(':')?;
    Some(SubtitleCue {
        start: parse_time(time)?,
        duration: parse_time(duration)?,
        position: parse_position(anchor)?,
        color: parse_color(color)?,
        text: parse_text(text)?,
    })
}

fn value<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let (_, tail) = body.split_once(&format!("{key}="))?;
    let end = tail.find(',').unwrap_or(tail.len());
    Some(tail[..end].trim())
}

fn color_and_text(body: &str) -> Option<(&str, &str)> {
    let (_, tail) = body.split_once("Color=")?;
    let text_start = tail.find("Text=")?;
    let before = tail[..text_start].trim().trim_end_matches(',').trim();
    let text = tail[text_start + "Text=".len()..].trim();
    Some((before, text))
}

fn parse_time(value: &str) -> Option<SubtitleTime> {
    let mut parts = value.split(':');
    let hours = parts.next()?.trim().parse().ok()?;
    let minutes = parts.next()?.trim().parse().ok()?;
    let seconds = parts.next()?.trim().parse().ok()?;
    let hundredths = parts.next()?.trim().parse().ok()?;
    (parts.next().is_none() && minutes < 60 && seconds < 60 && hundredths < 100).then_some(
        SubtitleTime {
            hours,
            minutes,
            seconds,
            hundredths,
        },
    )
}

fn parse_position(value: &str) -> Option<SubtitlePosition> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => Some(SubtitlePosition::Left),
        "center" | "centre" => Some(SubtitlePosition::Center),
        "right" => Some(SubtitlePosition::Right),
        _ => None,
    }
}

fn parse_color(value: &str) -> Option<[u8; 3]> {
    let mut channels = value.split(':').map(|part| part.trim().parse::<u8>());
    let color = [
        channels.next()?.ok()?,
        channels.next()?.ok()?,
        channels.next()?.ok()?,
    ];
    channels.next().is_none().then_some(color)
}

fn parse_text(value: &str) -> Option<String> {
    let value = value.trim();
    let text = if let Some(inner) = value.strip_prefix('"') {
        inner.strip_suffix('"')?
    } else {
        value
    };
    (!text.is_empty()).then(|| text.replace("\\n", "\n"))
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    #[test]
    fn parses_retail_prefixes_missing_comma_and_zero_duration() {
        let text = "\u{feff}; comment\n/BeginMovie titlmvie\n/Font=batang.ttc\n\
            /<Time=00:00:00:00, Duration=00:00:10:00, Position=14:Center, Color=0:255:0, Text=\"测试\">\n\
            <Time=00:00:12:00, Duration=00:00:00:00, Position=14:Center, Color=255:255:128 Text=\"第二句\">\n\
            <Time=00:00:14:00, Duration=00:00:00:00, Position=14:Right, Color=255:0:0, Text=\"最后一句\">\n\
            /EndMovie\n";
        let table = SubtitleTable::parse_str(text);
        let cues = table.cues_for("TITLMVIE.BIK").unwrap();
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].text, "测试");
        assert_eq!(cues[1].color, [255, 255, 128]);
        assert_eq!(cues[2].position, SubtitlePosition::Right);
        assert_eq!(table.cue_at("titlmvie", 13.0).unwrap().text, "第二句");
        assert_eq!(table.cue_at("titlmvie", 16.0).unwrap().text, "最后一句");
        assert!(table.cue_at("titlmvie", 11.0).is_none());
    }

    #[test]
    fn explicit_duration_ends_before_the_next_cue() {
        let text = "BeginMovie A00_F00E\n\
            <Time=00:00:00:00, Duration=00:00:01:00, Position=14:Left, Color=1:2:3, Text=\"短\">\n\
            <Time=00:00:05:00, Duration=00:00:02:00, Position=14:Center, Color=4:5:6, Text=\"长\">\n\
            EndMovie\n";
        let table = SubtitleTable::parse_str(text);
        assert!(table.cue_at("A00_F00E", 0.5).is_some());
        assert!(table.cue_at("A00_F00E", 4.0).is_none());
        assert_eq!(table.cue_at("A00_F00E", 5.5).unwrap().text, "长");
        assert!(table.cue_at("A00_F00E", 7.0).is_none());
    }

    #[test]
    fn malformed_entries_and_movies_do_not_break_the_table() {
        let text = "BeginMovie GOOD\n\
            <Time=00:00:01:00, Duration=00:00:00:00, Position=10:Right, Color=0:0:255, Text=\"ok\">\n\
            <Time=bad, Duration=00:00:00:00, Position=14:Center, Color=0:0:0, Text=\"bad\">\n\
            EndMovie\n\
            BeginMovie EMPTY\n\
            EndMovie\n";
        let table = SubtitleTable::parse_str(text);
        assert_eq!(table.cues_for("good").unwrap().len(), 1);
        assert!(table.cues_for("empty").is_none());
    }

    #[test]
    fn utf16_requires_a_bom_and_preserves_chinese_text() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "BeginMovie A\r\n<Time=00:00:00:00, Duration=00:00:01:00, Position=14:Center, Color=255:255:255, Text=\"中文\">\r\nEndMovie\r\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let table = SubtitleTable::parse_utf16le(&bytes).unwrap();
        assert_eq!(table.cues_for("a").unwrap()[0].text, "中文");
        assert!(SubtitleTable::parse_utf16le(&bytes[2..]).is_none());
    }

    #[test]
    fn combines_both_games_without_replacing_yr_cues() {
        let encode = |text: &str| {
            std::iter::once(0xFEFF_u16)
                .chain(text.encode_utf16())
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        let entry =
            "<Time=00:00:00:00, Duration=00:00:00:00, Position=14:Center, Color=255:255:255, Text=";
        let yr = encode(&format!("BeginMovie SHARED\n{entry}\"YR\">\nEndMovie\n"));
        let ra2 = encode(&format!(
            "BeginMovie shared\n{entry}\"RA2\">\nEndMovie\n\
             BeginMovie RA2_ONLY\n{entry}\"base movie\">\nEndMovie\n"
        ));
        let table = SubtitleTable::from_retail_files(Some(&yr), Some(&ra2)).unwrap();
        assert_eq!(table.cues_for("shared.bik").unwrap()[0].text, "YR");
        assert_eq!(table.cues_for("ra2_only").unwrap()[0].text, "base movie");
        let fallback = SubtitleTable::from_retail_files(Some(b"invalid"), Some(&ra2)).unwrap();
        assert_eq!(fallback.cues_for("shared").unwrap()[0].text, "RA2");
        assert!(SubtitleTable::from_retail_files(None, None).is_none());
    }

    #[test]
    fn retail_chinese_tables_parse_when_configured() {
        let Some(root) = std::env::var_os("RA2_DIR").map(std::path::PathBuf::from) else {
            return;
        };
        let manager = crate::assets::asset_manager::AssetManager::new(
            &root,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .expect("open retail asset root");
        // The retail language archive is intentionally not part of the global
        // gameplay lookup chain; read it through its registered archive.
        let bytes = manager
            .archive("language.mix")
            .and_then(|archive| archive.get_by_name("subtitlemd.txt"))
            .expect("RA2_DIR must provide language.mix/subtitlemd.txt");
        let table = SubtitleTable::parse_utf16le(bytes).expect("parse subtitlemd.txt");
        assert!(
            table
                .cues_for("A00_F00E")
                .is_some_and(|cues| cues.len() > 20)
        );
        let first = table
            .cues_for("A00_F00E")
            .and_then(|cues| cues.first())
            .expect("A00_F00E has subtitles");
        assert_eq!(
            first.start,
            SubtitleTime {
                hours: 0,
                minutes: 0,
                seconds: 4,
                hundredths: 30
            }
        );
        assert_eq!(first.start.seconds(), 4.3);
        assert_eq!(
            table
                .cue_at("a00_f00e", first.start.seconds() + 0.5)
                .unwrap()
                .text,
            "我们目前正处于第二级防卫状态。"
        );
    }
}
