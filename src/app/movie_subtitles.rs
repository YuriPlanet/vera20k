//! Optional, locally supplied YR subtitles for the ordinary Apple client.
//!
//! The sidecar does not replace any movie bytes. Its clock is derived from
//! the displayed Bink frame, so pause, decoder catch-up and seek stay aligned.
//! Retail's time reader 6CA5A0 converts HH:MM:SS:centiseconds to 60 Hz ticks;
//! 6C9900/6CA680 give an omitted or zero duration a 1200-tick default.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_paint::TEXT_DEPTH;
use crate::render::shell_text::{ScissorRect, ShellTextDraw};

const DEFAULT_DURATION_TICKS: u64 = 1200;
const MAX_SIDECAR_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Alignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Debug)]
struct Cue {
    start: u64,
    duration: u64,
    color: [u8; 3],
    alignment: Alignment,
    text: String,
}

#[derive(Debug)]
pub(crate) struct MovieSubtitles {
    cues: Vec<Cue>,
}

fn time_ticks(value: &str) -> Result<u64, String> {
    let parts = value
        .split(':')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("invalid subtitle time {value:?}"))?;
    let [hours, minutes, seconds, centiseconds] = parts.as_slice() else {
        return Err(format!("subtitle time needs four fields: {value:?}"));
    };
    if *minutes >= 60 || *seconds >= 60 || *centiseconds >= 100 {
        return Err(format!("subtitle time out of range: {value:?}"));
    }
    hours
        .checked_mul(3600)
        .and_then(|n| n.checked_add(minutes * 60 + seconds))
        .and_then(|n| n.checked_mul(60))
        .and_then(|n| n.checked_add(centiseconds * 60 / 100))
        .ok_or_else(|| "subtitle time overflow".into())
}

/// Read one unquoted field independently: the original Chinese sidecar has
/// one Color value followed by Text without a comma. Retail tolerates it.
fn field<'a>(line: &'a str, key: &str) -> Result<&'a str, String> {
    let start = line
        .find(key)
        .ok_or_else(|| format!("missing subtitle field {key}"))?
        + key.len();
    Ok(line[start..]
        .trim_start()
        .split([',', ' ', '\t', '>'])
        .next()
        .unwrap_or_default())
}

fn parse_cue(line: &str) -> Result<Cue, String> {
    let start = time_ticks(field(line, "Time=")?)?;
    let duration = time_ticks(field(line, "Duration=")?)?;
    let colors = field(line, "Color=")?
        .split(':')
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid subtitle color".to_string())?;
    let [red, green, blue] = colors.as_slice() else {
        return Err("subtitle color needs three channels".into());
    };
    let position = field(line, "Position=")?;
    let alignment = match position
        .rsplit(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "left" => Alignment::Left,
        "right" => Alignment::Right,
        "center" => Alignment::Center,
        _ => return Err(format!("invalid subtitle position {position:?}")),
    };
    let text_start = line.find("Text=").ok_or("missing subtitle text")? + 5;
    let quoted = line[text_start..].trim_start();
    let text = quoted
        .strip_prefix('"')
        .and_then(|text| text.rsplit_once('"').map(|(text, _)| text))
        .ok_or("unclosed subtitle text")?
        .replace('\u{3000}', " ");
    Ok(Cue {
        start,
        duration: if duration == 0 {
            DEFAULT_DURATION_TICKS
        } else {
            duration
        },
        color: [*red, *green, *blue],
        alignment,
        text,
    })
}

fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err("truncated UTF-16 subtitle file".into());
        }
        let words: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        String::from_utf16(&words).map_err(|_| "invalid UTF-16 subtitle file".into())
    } else {
        std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map(str::to_owned)
            .map_err(|_| "subtitle file must be UTF-16LE or UTF-8".into())
    }
}

fn parse_movies(bytes: &[u8]) -> Result<BTreeMap<String, Vec<Cue>>, String> {
    let text = decode_text(bytes)?;
    let mut movies: BTreeMap<String, Vec<Cue>> = BTreeMap::new();
    let mut current = None;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with([';', '/']) {
            continue;
        }
        if let Some(name) = line.strip_prefix("BeginMovie ") {
            let name = name.trim().to_ascii_uppercase();
            movies.entry(name.clone()).or_default();
            current = Some(name);
        } else if line.eq_ignore_ascii_case("EndMovie") {
            current = None;
        } else if line.starts_with('<') {
            let name = current
                .as_ref()
                .ok_or_else(|| format!("subtitle cue outside movie on line {}", index + 1))?;
            let cue =
                parse_cue(line).map_err(|error| format!("subtitle line {}: {error}", index + 1))?;
            movies
                .get_mut(name)
                .expect("movie inserted above")
                .push(cue);
        }
    }
    for cues in movies.values_mut() {
        cues.sort_by_key(|cue| cue.start);
    }
    Ok(movies)
}

fn sidecar_paths(executable: &Path) -> Vec<PathBuf> {
    let Some(directory) = executable.parent() else {
        return Vec::new();
    };
    // iOS: application root. macOS: Contents/Resources next to MacOS.
    let mut paths = vec![directory.join("subtitlemd.txt")];
    if let Some(contents) = directory.parent() {
        paths.push(contents.join("Resources/subtitlemd.txt"));
    }
    paths
}

impl MovieSubtitles {
    pub(crate) fn load(name: &str, ordinary_client: bool) -> Option<Self> {
        if !ordinary_client || !cfg!(any(target_os = "ios", target_os = "macos")) {
            return None;
        }
        let executable = std::env::current_exe().ok()?;
        let path = sidecar_paths(&executable)
            .into_iter()
            .find(|path| path.is_file())?;
        let result = (|| {
            let size = std::fs::metadata(&path)
                .map_err(|error| error.to_string())?
                .len();
            if size > MAX_SIDECAR_BYTES {
                return Err("subtitle sidecar is too large".to_string());
            }
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            let mut movies = parse_movies(&bytes)?;
            Ok(movies
                .remove(&name.to_ascii_uppercase())
                .map(|cues| Self { cues }))
        })();
        match result {
            Ok(Some(track)) => {
                log::info!(
                    "Movie subtitles: {name}, {} cues, {}",
                    track.cues.len(),
                    path.display()
                );
                Some(track)
            }
            Ok(None) => None,
            Err(error) => {
                log::warn!("Movie subtitles unavailable ({}): {error}", path.display());
                None
            }
        }
    }

    fn cue_at_frame(&self, frame: usize, fps_num: u32, fps_den: u32) -> Option<&Cue> {
        if fps_num == 0 || fps_den == 0 {
            return None;
        }
        let ticks = ((frame as u128 * u128::from(fps_den) * 60) / u128::from(fps_num))
            .min(u128::from(u64::MAX)) as u64;
        let index = self
            .cues
            .partition_point(|cue| cue.start <= ticks)
            .checked_sub(1)?;
        let cue = &self.cues[index];
        (ticks < cue.start.saturating_add(cue.duration)).then_some(cue)
    }

    pub(crate) fn draw(
        &self,
        font: &BitFont,
        frame: usize,
        fps: [u32; 2],
        movie: [i32; 4],
        screen: [u32; 2],
    ) -> Option<ShellTextDraw> {
        let cue = self.cue_at_frame(frame, fps[0], fps[1])?;
        let [movie_x, movie_y, movie_w, movie_h] = movie;
        let left = movie_x.max(0);
        let top = movie_y.max(0);
        let right = (movie_x + movie_w).min(screen[0] as i32);
        let bottom = (movie_y + movie_h).min(screen[1] as i32);
        if right <= left || bottom <= top {
            return None;
        }
        let margin = ((right - left) as f32 * 0.04).round().max(8.0);
        let width = ((right - left) as f32 - 2.0 * margin).max(1.0);
        let glyph_height = (movie_h as f32 * 24.0 / 600.0).clamp(16.0, 36.0);
        let scale = glyph_height / font.glyph_height().max(1.0);
        let layout = font.wrap_layout(&cue.text, (width / scale).floor().max(1.0) as u32);
        let line_height = font.cell_height() * scale;
        let bottom_margin = (movie_h as f32 * 0.03).max(10.0);
        let mut y = (bottom as f32 - bottom_margin - layout.lines.len() as f32 * line_height)
            .max(top as f32);
        let mut instances = Vec::<SpriteInstance>::new();
        let outline = (glyph_height / 18.0).max(1.0);
        for line in layout.lines {
            let line_text = &cue.text[line.start_byte..line.end_byte];
            let line_width = line.width as f32 * scale;
            let offset = match cue.alignment {
                Alignment::Left => 0.0,
                Alignment::Center => (width - line_width) * 0.5,
                Alignment::Right => width - line_width,
            };
            let x = left as f32 + margin + offset.max(0.0);
            for [dx, dy] in [
                [-1.0, -1.0],
                [0.0, -1.0],
                [1.0, -1.0],
                [-1.0, 0.0],
                [1.0, 0.0],
                [-1.0, 1.0],
                [0.0, 1.0],
                [1.0, 1.0],
            ] {
                instances.extend(font.build_text(
                    line_text,
                    x + dx * outline,
                    y + dy * outline,
                    scale,
                    TEXT_DEPTH + 0.00001,
                    [0.0; 3],
                    [0.0; 2],
                ));
            }
            instances.extend(font.build_text(
                line_text,
                x,
                y,
                scale,
                TEXT_DEPTH,
                cue.color.map(|c| f32::from(c) / 255.0),
                [0.0; 2],
            ));
            y += line_height;
        }
        Some(ShellTextDraw {
            instances,
            scissor: ScissorRect {
                x: left as u32,
                y: top as u32,
                w: (right - left) as u32,
                h: (bottom - top) as u32,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BeginMovie A00_F00E\n<Time=00:00:04:30, Duration=00:00:02:00, Position=14:Center, Color=255:255:0, Text=\"我们目前正处于第二级防卫状态。\">\n<Time=00:00:09:00, Duration=00:00:00:00, Position=14:center, Color=0:255:0 Text=\"中尉，这个恶魔到底想做什么？\">\nEndMovie\n";

    fn track() -> MovieSubtitles {
        let mut movies = parse_movies(SAMPLE.as_bytes()).unwrap();
        MovieSubtitles {
            cues: movies.remove("A00_F00E").unwrap(),
        }
    }

    #[test]
    fn native_time_uses_centiseconds_quantized_to_sixty_hz() {
        assert_eq!(time_ticks("00:00:04:30").unwrap(), 258);
        assert_eq!(time_ticks("01:02:03:99").unwrap(), 223439);
        assert_eq!(time_ticks("00:00:00:01").unwrap(), 0);
        assert!(time_ticks("00:60:00:00").is_err());
    }

    #[test]
    fn default_duration_and_real_sidecar_missing_comma_are_supported() {
        let track = track();
        assert_eq!(track.cues[0].duration, 120);
        assert_eq!(track.cues[1].duration, 1200);
        assert_eq!(track.cues[1].color, [0, 255, 0]);
    }

    #[test]
    fn displayed_frame_controls_boundaries_and_expired_gaps() {
        let track = track();
        assert!(track.cue_at_frame(64, 15, 1).is_none());
        assert_eq!(track.cue_at_frame(65, 15, 1).unwrap().start, 258);
        assert!(track.cue_at_frame(95, 15, 1).is_none());
        assert_eq!(track.cue_at_frame(135, 15, 1).unwrap().start, 540);
        assert!(track.cue_at_frame(435, 15, 1).is_none());
        // A held/paused frame selects the same cue; a backwards seek can
        // reselect an earlier one without a second running subtitle clock.
        assert_eq!(track.cue_at_frame(65, 15, 1).unwrap().start, 258);
    }

    #[test]
    fn later_cue_replaces_an_overlapping_long_default() {
        let mut track = track();
        track.cues[0].duration = DEFAULT_DURATION_TICKS;
        assert_eq!(track.cue_at_frame(135, 15, 1).unwrap().start, 540);
        assert!(track.cue_at_frame(1, 0, 1).is_none());
    }

    #[test]
    fn utf16_comments_and_movie_case_are_supported() {
        let text = format!("; comment\n/BeginMovie TEST\n{SAMPLE}");
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let movies = parse_movies(&bytes).unwrap();
        assert_eq!(movies.len(), 1);
        assert!(movies.contains_key("A00_F00E"));
        assert!(decode_text(&[0xff, 0xfe, 1]).is_err());
    }

    #[test]
    fn app_bundle_candidates_cover_mac_and_ios() {
        assert_eq!(
            sidecar_paths(Path::new(
                "/Applications/王二火大.app/Contents/MacOS/vera20k"
            ))[1],
            PathBuf::from("/Applications/王二火大.app/Contents/Resources/subtitlemd.txt")
        );
        assert_eq!(
            sidecar_paths(Path::new("/var/containers/王二火大.app/vera20k"))[0],
            PathBuf::from("/var/containers/王二火大.app/subtitlemd.txt")
        );
    }

    #[test]
    fn subtitle_wrap_and_outline_stay_inside_the_movie() {
        let chars: Vec<_> = SAMPLE
            .chars()
            .filter(|c| !c.is_ascii())
            .map(|c| (c as u16, 16))
            .collect();
        let font = crate::render::bit_font::tests::make_test_font(&chars, 8);
        let track = track();
        let draw = track
            .draw(&font, 65, [15, 1], [240, 0, 1440, 1080], [1920, 1080])
            .unwrap();
        assert_eq!(draw.scissor.x, 240);
        assert_eq!(draw.scissor.w, 1440);
        assert!(!draw.instances.is_empty());
        assert!(
            draw.instances
                .iter()
                .all(|glyph| glyph.position[0] >= 240.0 && glyph.position[1] > 800.0)
        );
    }

    #[test]
    #[ignore = "set VERA20K_SUBTITLE_FILE to the optional, locally supplied subtitlemd.txt"]
    fn local_yuri_sidecar_parses_all_fifty_six_movies() {
        let path = std::env::var_os("VERA20K_SUBTITLE_FILE").expect("VERA20K_SUBTITLE_FILE");
        let movies = parse_movies(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(movies.len(), 56);
        assert_eq!(movies.values().map(Vec::len).sum::<usize>(), 495);
        assert_eq!(movies["A00_F00E"].len(), 59);
        assert_eq!(movies["A00_F00E"][0].start, 258);
    }
}
