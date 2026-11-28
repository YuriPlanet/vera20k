//! Player-supplied background music, indexed from the writable profile's
//! `musics/` folder.
//!
//! VERA extension, no native counterpart: `music.ini` in that folder decides
//! whether in-match random theme playback uses these files. Custom tracks are
//! appended to the same logical playlist the Theme AI draws from, so the
//! existing slot/sentinel/RNG state machine is reused unchanged. Named
//! lifecycle themes (INTRO / LOADING / SCORE / CREDITS) and the WOL lobby are
//! deliberately outside this path.
//!
//! Files are read through [`crate::util::file_system`] so the browser build's
//! in-memory VFS behaves like the native filesystem. An absent or broken
//! configuration leaves custom music disabled and the retail playlist intact.

use std::path::Path;
use std::sync::Arc;

use crate::assets::mix_archive::SharedBytes;
use crate::rules::ini_parser::IniFile;
use crate::rules::theme_ini::ThemeEntry;

/// Internal marker on a custom track's `Sound` stem. The character cannot
/// occur in a retail INI value, so a player file named like a retail song
/// never shadows that song when both are in the append playlist.
pub(crate) const CUSTOM_STEM_PREFIX: &str = "\u{1}vera:";

/// One loadable music file found in `musics/`.
#[derive(Clone)]
struct CustomTrack {
    /// The file's bytes, shared with the streaming decoder.
    bytes: SharedBytes,
    /// A `.opus` file (Ogg Opus) rather than WAV or AUD.
    opus: bool,
    /// A catalog-shaped view so the Theme runtime can reuse `entry(index)`,
    /// `is_allowed` and `entry_repeats` for these tracks.
    entry: ThemeEntry,
}

/// The indexed, immutable set of player-supplied tracks for one process.
#[derive(Clone)]
pub(crate) struct CustomMusicLibrary {
    exclusive: bool,
    tracks: Vec<CustomTrack>,
}

impl std::fmt::Debug for CustomMusicLibrary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CustomMusicLibrary")
            .field("exclusive", &self.exclusive)
            .field("tracks", &self.tracks.len())
            .finish()
    }
}

impl CustomMusicLibrary {
    /// `ExclusiveMode=yes`: only these tracks feed the in-match random draw.
    pub(crate) fn exclusive(&self) -> bool {
        self.exclusive
    }

    pub(crate) fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Catalog-style entry for a virtual index past the retail catalog.
    pub(crate) fn entry(&self, index: usize) -> Option<&ThemeEntry> {
        self.tracks.get(index).map(|track| &track.entry)
    }

    /// Bytes for a virtual custom index, and whether they are Ogg Opus, used
    /// by the Theme AI's prepare closure when a custom track was selected.
    pub(crate) fn file_at(&self, index: usize) -> Option<(&SharedBytes, bool)> {
        self.tracks
            .get(index)
            .map(|track| (&track.bytes, track.opus))
    }

    /// Build a library of empty, decodable-free entries for selection tests.
    /// The bytes are unused there: tests inject their own `prepare` closure.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(exclusive: bool, count: usize) -> Arc<Self> {
        let tracks = (0..count)
            .map(|index| CustomTrack {
                bytes: Arc::<[u8]>::from([]).into(),
                opus: false,
                entry: ThemeEntry {
                    key: format!("custom{index}"),
                    sound: format!("{CUSTOM_STEM_PREFIX}{index}"),
                    name_key: String::new(),
                    duration_seconds: 0,
                    scenario: 0,
                    normal: true,
                    repeat: false,
                    side_name: None,
                    available: true,
                },
            })
            .collect();
        Arc::new(Self { exclusive, tracks })
    }
}

/// Read `musics/music.ini` and index every loadable WAV/AUD/Opus file.
///
/// Returns `None` when the config is absent, `enable` is off, no file is
/// loadable, or the browser VFS has nothing indexed. Any such failure keeps
/// the retail playlist in force.
pub(crate) fn load(dir: &Path) -> Option<Arc<CustomMusicLibrary>> {
    let config = crate::util::file_system::read_to_string(dir.join("music.ini")).ok()?;
    let ini = IniFile::from_str(&config);
    if !read_switch(&ini, "enable", false) {
        return None;
    }
    let exclusive = read_switch(&ini, "ExclusiveMode", false);

    let mut found: Vec<(String, SharedBytes, bool)> = Vec::new();
    let listing = crate::util::file_system::read_dir(dir).ok()?;
    for entry in listing.flatten() {
        let path = entry.path();
        let Some((stem, opus)) = music_stem(&path) else {
            continue;
        };
        let Ok(data) = crate::util::file_system::read(&path) else {
            continue;
        };
        let bytes: SharedBytes = Arc::<[u8]>::from(data).into();
        if !crate::audio::theme::track_bytes_loadable(&bytes, opus) {
            continue;
        }
        found.push((stem, bytes, opus));
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found.dedup_by(|left, right| left.0.eq_ignore_ascii_case(&right.0));
    if found.is_empty() {
        return None;
    }
    let tracks = found
        .into_iter()
        .enumerate()
        .map(|(index, (stem, bytes, opus))| CustomTrack {
            bytes,
            opus,
            entry: ThemeEntry {
                key: stem.clone(),
                sound: format!("{CUSTOM_STEM_PREFIX}{index}"),
                name_key: String::new(),
                duration_seconds: 0,
                scenario: 0,
                normal: true,
                repeat: false,
                side_name: None,
                available: true,
            },
        })
        .collect();
    Some(Arc::new(CustomMusicLibrary { exclusive, tracks }))
}

/// A VERA-owned switch; scan sections so the player may place the keys under
/// any heading (a heading is required by the native INI reader). `read_bool`
/// keeps the same `1/T/Y` / `0/F/N` first-byte decision gamemd uses.
fn read_switch(ini: &IniFile, key: &str, default: bool) -> bool {
    for name in ini.section_names() {
        if let Some(value) = ini
            .section(name)
            .and_then(|section| section.read_bool_value(key))
        {
            return value;
        }
    }
    default
}

/// `.wav` and `.aud`, the engine's own encodings, and VERA's `.opus` (Ogg
/// Opus, decoded by `assets::opus_file`); the flag says which kind.
fn music_stem(path: &Path) -> Option<(String, bool)> {
    let extension = path.extension()?.to_str()?;
    let opus = extension.eq_ignore_ascii_case("opus");
    if !opus && !extension.eq_ignore_ascii_case("wav") && !extension.eq_ignore_ascii_case("aud") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    (!stem.is_empty()).then(|| (stem.to_string(), opus))
}

#[cfg(all(test, tests_audio))]
mod tests {
    use super::*;

    #[test]
    fn switch_reads_native_bool_decision_and_defaults_off() {
        let ini = IniFile::from_str("[Music]\nenable=yes\nExclusiveMode=no\n");
        assert!(read_switch(&ini, "enable", false));
        assert!(!read_switch(&ini, "ExclusiveMode", false));
        assert!(!read_switch(&ini, "Absent", false));

        let ini = IniFile::from_str("[Anything]\nenable=1\n");
        assert!(read_switch(&ini, "enable", false));

        let ini = IniFile::from_str("[Music]\nenable=on\n");
        assert!(!read_switch(&ini, "enable", false));
    }

    #[test]
    fn music_stem_accepts_engine_formats_only() {
        let stem = |name: &str| music_stem(Path::new(name));
        assert_eq!(stem("Rock.WAV"), Some(("Rock".to_string(), false)));
        assert_eq!(stem("Theme.aud"), Some(("Theme".to_string(), false)));
        assert_eq!(stem("Calm.Opus"), Some(("Calm".to_string(), true)));
        assert_eq!(music_stem(Path::new("notes.mp3")), None);
        assert_eq!(music_stem(Path::new("noextension")), None);
    }

    fn pcm_wav() -> Vec<u8> {
        let samples: Vec<i16> = (0..512).map(|i| (i % 64) as i16).collect();
        let mut file = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
        file.extend(1u16.to_le_bytes()); // PCM
        file.extend(1u16.to_le_bytes()); // mono
        file.extend(22_050u32.to_le_bytes()); // sample rate
        file.extend(44_100u32.to_le_bytes()); // byte rate
        file.extend(2u16.to_le_bytes()); // block align
        file.extend(16u16.to_le_bytes()); // bits per sample
        file.extend(b"data");
        file.extend(((samples.len() * 2) as u32).to_le_bytes());
        for sample in samples {
            file.extend(sample.to_le_bytes());
        }
        file
    }

    #[test]
    fn load_indexes_only_enabled_playable_files() {
        let dir = std::env::temp_dir().join(format!("vera20k-musics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test musics dir");
        std::fs::write(dir.join("Track.WAV"), pcm_wav()).expect("write wav");
        std::fs::write(dir.join("broken.wav"), b"not a wav").expect("write broken");
        std::fs::write(dir.join("notes.txt"), b"ignore me").expect("write notes");

        // Disabled: no library even though files are present.
        std::fs::write(dir.join("music.ini"), "[Music]\nenable=no\n").expect("write ini");
        assert!(
            load(&dir).is_none(),
            "disabled config keeps retail playlist"
        );

        // Enabled: only the playable WAV is indexed, with an encoded stem.
        std::fs::write(
            dir.join("music.ini"),
            "[Music]\nenable=yes\nExclusiveMode=yes\n",
        )
        .expect("write ini");
        let library = load(&dir).expect("indexed library");
        assert_eq!(library.len(), 1, "broken and non-audio files skipped");
        assert!(library.exclusive(), "ExclusiveMode read from ini");
        let entry = library.entry(0).expect("first entry");
        assert_eq!(entry.key, "Track");
        assert_eq!(entry.sound, format!("{CUSTOM_STEM_PREFIX}0"));
        assert!(library.file_at(0).is_some(), "bytes retained for streaming");
        assert!(library.file_at(1).is_none(), "past the end");

        // The retained bytes decode through the same path the AI prepare
        // closure uses, so a selected custom track yields real PCM.
        let (bytes, opus) = library.file_at(0).expect("custom bytes");
        assert!(crate::audio::theme::track_bytes_loadable(bytes, opus));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
