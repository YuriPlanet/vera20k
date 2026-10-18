//! Game configuration and retail asset-root discovery.
//!
//! config.toml is machine-specific (contains the local RA2 install path)
//! and is gitignored. A config.toml.example template is provided in the repo.
//! The working-directory override takes precedence over a config beside the
//! executable. The latter lets packaged apps launch without a particular cwd.
//! Without either config, assets are resolved beside the executable.
//!
//! ## Dependency rules
//! - config.rs is part of util/ â€” no dependencies on game modules.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

/// Optional host override, in the working or executable directory.
const CONFIG_FILE_NAME: &str = "config.toml";

/// Top-level game configuration, read from config.toml.
///
/// Add new sections here as features are implemented (audio, game speed, etc.).
#[derive(Debug)]
pub struct GameConfig {
    /// File system paths (RA2 install directory).
    pub paths: PathsConfig,
    /// Graphics/window settings (all optional — sensible defaults provided).
    pub graphics: GraphicsConfig,
    /// Deterministic simulation settings.
    pub gameplay: GameplayConfig,
    /// Local player profile (name pre-filled into skirmish/multiplayer setup).
    pub profile: ProfileConfig,
}

/// Local player profile settings.
///
/// `[profile]` may be omitted entirely. `name` is the persistent player handle
/// the setup screen pre-fills into the name field; when unset the setup UI
/// falls back to its own default. This mirrors the original reading the player
/// name from a persistent profile source rather than a baked-in string.
#[derive(Debug, Default)]
pub struct ProfileConfig {
    /// Player name shown/edited in skirmish setup. `None` (or empty) means use
    /// the setup screen's built-in default.
    pub name: Option<String>,
}

impl ProfileConfig {
    /// The configured player name, trimmed; `None` when unset or blank so the
    /// caller can apply its own default.
    pub fn player_name(&self) -> Option<&str> {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }
}

/// Paths to external resources (the player's RA2 installation).
#[derive(Debug)]
pub struct PathsConfig {
    /// Path to the user's RA2 installation directory.
    /// MIX files (ra2.mix, language.mix, theme.mix) are loaded from here.
    /// Example: "C:/Program Files/EA Games/Command and Conquer Red Alert II"
    pub ra2_dir: PathBuf,
}

/// Graphics settings.
///
/// Every field has a sensible default so `[graphics]` can be omitted entirely.
/// Full screen, zoom and the original 4:3 shape are player choices on the
/// Options page, saved in RA2MD.INI, not here. The window always presents in
/// step with the display (`render/gpu.rs`), so an old config's `vsync` line
/// is ignored.
#[derive(Debug)]
pub struct GraphicsConfig {
    /// Enable cosmetic per-frame effects: water/ore sparkles. Also intended to
    /// gate future cosmetic effects (laser beam pulses, particle systems, line
    /// trails) per gamemd's "Extra Animations" option. Default ON to match
    /// gamemd's default.
    pub extra_animations: bool,
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            extra_animations: true,
        }
    }
}

/// Simulation pacing and command scheduling settings.
#[derive(Debug)]
pub struct GameplayConfig {
    /// Session MaxAhead, used by network scheduling and the local production
    /// prediction clock. Offline commands retain their issue-frame stamp.
    pub input_delay_ticks: u32,
    /// Freeze a running match while the window is not the foreground, as
    /// gamemd does when `WM_ACTIVATEAPP` clears. Off by default: the match
    /// keeps simulating and playing audio behind other windows, and only
    /// input stops. Local pacing policy; never read by the simulation.
    pub pause_on_focus_loss: bool,
}

impl Default for GameplayConfig {
    fn default() -> Self {
        Self {
            input_delay_ticks: default_input_delay_ticks(),
            pause_on_focus_loss: false,
        }
    }
}

/// Session constructor6975C0 stores15 through the CRT initializer
/// 8132F0 ->4E8035 ->6970A0 (active retail gamemd.exe). This is an initial
/// value, not a permanent offline invariant: configuration and negotiated
/// session updates retain authority over the current window.
pub const fn default_input_delay_ticks() -> u32 {
    15
}

impl GameConfig {
    /// Load the optional host override, otherwise use the retail module directory.
    ///
    /// Retail provenance: Executable-root path discovery — `WinMain` @ `0x006BB9A0`.
    /// Active `gamemd.exe` `WinMain @ 0x006BB9A0` calls
    /// `GetModuleFileNameA`, splits/rebuilds its drive and directory, and calls
    /// `SetCurrentDirectoryA` before opening `RA2MD.INI` or any MIX archive.
    /// Rust keeps the resolved directory explicit instead of mutating the
    /// process-wide current directory.
    pub fn load() -> Result<Self> {
        let working_dir =
            std::env::current_dir().context("Failed to locate the working directory")?;
        let executable = std::env::current_exe()
            .context("Failed to locate the running executable for retail asset discovery")?;
        Self::load_from(&working_dir, &executable)
    }

    pub(crate) fn load_from(working_dir: &Path, executable: &Path) -> Result<Self> {
        let root = retail_asset_root_from_executable(executable)?;
        for directory in [working_dir, root.as_path()] {
            let path = directory.join(CONFIG_FILE_NAME);
            match std::fs::read_to_string(&path) {
                Ok(contents) => return Self::parse_from(&contents, &path),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("Failed to read config file: {}", path.display())
                    });
                }
            }
        }
        log::info!(
            "No {}; using retail executable directory: {}",
            CONFIG_FILE_NAME,
            root.display()
        );
        Ok(Self::from_retail_asset_root(root))
    }

    fn parse_from(contents: &str, path: &Path) -> Result<Self> {
        let mut config = Self::parse(contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        // A packaged config must mean the same thing after Finder changes cwd.
        if config.paths.ra2_dir.is_relative() {
            config.paths.ra2_dir = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&config.paths.ra2_dir);
        }

        log::info!("Loaded config from {}", path.display());
        log::info!("RA2 directory: {}", config.paths.ra2_dir.display());

        Ok(config)
    }

    /// Read config.toml's subset of TOML: `[section]` headers, `key = value`
    /// lines and `#` comments. The keys below take quoted strings, booleans or
    /// whole numbers. Other keys and sections are skipped without reading their
    /// values, so old configs with retired keys (`vsync`, `sim_tick_hz`) load.
    fn parse(contents: &str) -> Result<Self> {
        let mut ra2_dir = None;
        let mut graphics = GraphicsConfig::default();
        let mut gameplay = GameplayConfig::default();
        let mut profile = ProfileConfig::default();
        let mut section = "";
        let mut seen = std::collections::HashSet::new();
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
        for (index, line) in contents.lines().enumerate() {
            let line_number = index + 1;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                let header = line.split('#').next().unwrap_or_default().trim_end();
                section = header
                    .strip_prefix('[')
                    .and_then(|header| header.strip_suffix(']'))
                    .with_context(|| format!("line {line_number}: unterminated `[section]`"))?
                    .trim();
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .with_context(|| format!("line {line_number}: expected `key = value`"))?;
            let key = key.trim();
            ensure!(
                seen.insert((section, key)),
                "line {line_number}: duplicate key `{key}` in [{section}]"
            );
            let value = value.trim();
            let read = match (section, key) {
                ("paths", "ra2_dir") => {
                    parse_string(value).map(|path| ra2_dir = Some(PathBuf::from(path)))
                }
                ("graphics", "extra_animations") => {
                    parse_bool(value).map(|on| graphics.extra_animations = on)
                }
                ("gameplay", "input_delay_ticks") => {
                    parse_u32(value).map(|ticks| gameplay.input_delay_ticks = ticks)
                }
                ("gameplay", "pause_on_focus_loss") => {
                    parse_bool(value).map(|on| gameplay.pause_on_focus_loss = on)
                }
                ("profile", "name") => parse_string(value).map(|name| profile.name = Some(name)),
                _ => Ok(()),
            };
            read.with_context(|| format!("line {line_number}: [{section}] {key}"))?;
        }
        let ra2_dir = ra2_dir.context("missing `ra2_dir` in [paths]")?;
        Ok(Self {
            paths: PathsConfig { ra2_dir },
            graphics,
            gameplay,
            profile,
        })
    }

    fn from_retail_asset_root(ra2_dir: PathBuf) -> Self {
        Self {
            paths: PathsConfig { ra2_dir },
            graphics: GraphicsConfig::default(),
            gameplay: GameplayConfig::default(),
            profile: ProfileConfig::default(),
        }
    }
}

/// A TOML basic string (`"…"`, with backslash escapes) or literal string
/// (`'…'`, taken as written). Windows paths need `'C:\RA2'` or `"C:\\RA2"`.
fn parse_string(value: &str) -> Result<String> {
    if let Some(body) = value.strip_prefix('\'') {
        let (text, rest) = body.split_once('\'').context("unterminated string")?;
        expect_value_end(rest)?;
        return Ok(text.to_owned());
    }
    let body = value
        .strip_prefix('"')
        .context("expected a quoted string")?;
    let mut text = String::new();
    let mut chars = body.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => {
                expect_value_end(&body[index + 1..])?;
                return Ok(text);
            }
            '\\' => {
                let escaped = match chars.next().map(|(_, c)| c) {
                    Some('"') => '"',
                    Some('\\') => '\\',
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some('r') => '\r',
                    Some('b') => '\u{8}',
                    Some('f') => '\u{c}',
                    Some(kind @ ('u' | 'U')) => {
                        let digits = if kind == 'u' { 4 } else { 8 };
                        let hex: String = chars.by_ref().take(digits).map(|(_, c)| c).collect();
                        u32::from_str_radix(&hex, 16)
                            .ok()
                            .filter(|_| hex.len() == digits)
                            .and_then(char::from_u32)
                            .with_context(|| format!("invalid escape `\\{kind}{hex}`"))?
                    }
                    Some(other) => bail!("invalid escape `\\{other}`"),
                    None => bail!("unterminated string"),
                };
                text.push(escaped);
            }
            c => text.push(c),
        }
    }
    bail!("unterminated string")
}

fn parse_bool(value: &str) -> Result<bool> {
    match bare_value(value) {
        "true" => Ok(true),
        "false" => Ok(false),
        other => bail!("expected true or false, found `{other}`"),
    }
}

fn parse_u32(value: &str) -> Result<u32> {
    let number = bare_value(value);
    // TOML allows `_` between digits.
    number.replace('_', "").parse().with_context(|| {
        format!(
            "expected a whole number from 0 to {}, found `{number}`",
            u32::MAX
        )
    })
}

/// An unquoted value: everything before a `#` comment.
fn bare_value(value: &str) -> &str {
    value.split('#').next().unwrap_or_default().trim_end()
}

/// Only whitespace and a `#` comment may follow a quoted value.
fn expect_value_end(rest: &str) -> Result<()> {
    let rest = rest.trim_start();
    ensure!(
        rest.is_empty() || rest.starts_with('#'),
        "unexpected `{rest}` after the value"
    );
    Ok(())
}

/// Return the directory that retail `WinMain` makes its file-search root.
fn retail_asset_root_from_executable(executable: &Path) -> Result<PathBuf> {
    executable
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "Running executable has no containing directory: {}",
                executable.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConfigFixture(PathBuf);

    impl ConfigFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("vera20k-config-{}-{id}", std::process::id()));
            std::fs::create_dir(&root).expect("unique config fixture");
            std::fs::create_dir(root.join("launch")).unwrap();
            std::fs::create_dir(root.join("app")).unwrap();
            Self(root)
        }

        fn load(&self) -> Result<GameConfig> {
            GameConfig::load_from(&self.0.join("launch"), &self.0.join("app/vera20k"))
        }
    }

    impl Drop for ConfigFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn packaged_config_loads_from_an_unrelated_working_directory() {
        let fixture = ConfigFixture::new();
        std::fs::write(
            fixture.0.join("app/config.toml"),
            "[paths]\nra2_dir = 'retail files'\n[profile]\nname = 'Packaged player'\n",
        )
        .unwrap();
        let config = fixture.load().unwrap();
        assert_eq!(config.paths.ra2_dir, fixture.0.join("app/retail files"));
        assert_eq!(config.profile.player_name(), Some("Packaged player"));
    }

    #[test]
    fn working_directory_config_overrides_packaged_config() {
        let fixture = ConfigFixture::new();
        std::fs::write(fixture.0.join("app/config.toml"), "invalid package config").unwrap();
        std::fs::write(
            fixture.0.join("launch/config.toml"),
            "[paths]\nra2_dir = 'local retail'\n",
        )
        .unwrap();
        assert_eq!(
            fixture.load().unwrap().paths.ra2_dir,
            fixture.0.join("launch/local retail")
        );
    }

    #[test]
    fn invalid_override_is_reported_instead_of_silently_using_packaged_assets() {
        let fixture = ConfigFixture::new();
        let override_path = fixture.0.join("launch").join("config.toml");
        std::fs::write(&override_path, "[broken").unwrap();
        std::fs::write(
            fixture.0.join("app/config.toml"),
            "[paths]\nra2_dir = '.'\n",
        )
        .unwrap();
        let message = format!("{:#}", fixture.load().unwrap_err());
        assert!(message.contains(override_path.to_str().unwrap()));
        assert!(message.contains("Failed to parse config"));
    }

    #[test]
    fn missing_configs_keep_retail_executable_directory_discovery() {
        let fixture = ConfigFixture::new();
        assert_eq!(fixture.load().unwrap().paths.ra2_dir, fixture.0.join("app"));
    }

    #[test]
    fn test_minimal_config() {
        let toml_str = r#"
[paths]
ra2_dir = "C:/Westwood/RA2"
"#;
        let config = GameConfig::parse(toml_str).expect("Failed to parse test config");
        assert!(config.graphics.extra_animations);
        assert_eq!(config.gameplay.input_delay_ticks, 15);
        assert!(!config.gameplay.pause_on_focus_loss);
        // No [profile] section -> no pre-filled player name.
        assert_eq!(config.profile.player_name(), None);
    }

    #[test]
    fn configured_ahead_windows_preserve_zero_and_custom_values() {
        for ahead in [0, 2, 30] {
            let text = format!("[paths]\nra2_dir='.'\n[gameplay]\ninput_delay_ticks={ahead}\n");
            let config = GameConfig::parse(&text).unwrap();
            assert_eq!(config.gameplay.input_delay_ticks, ahead);
        }
    }

    #[test]
    fn an_old_vsync_line_still_loads() {
        let toml_str = r#"
[paths]
ra2_dir = "C:/Westwood/RA2"

[graphics]
vsync = false
extra_animations = false
"#;
        let config = GameConfig::parse(toml_str).expect("old configs keep loading");
        assert!(!config.graphics.extra_animations);
    }

    #[test]
    fn test_profile_player_name_trims_and_blank_is_none() {
        let toml_str = r#"
[paths]
ra2_dir = "C:/Westwood/RA2"

[profile]
name = "  Commander  "
"#;
        let config = GameConfig::parse(toml_str).expect("Failed to parse test config");
        assert_eq!(config.profile.player_name(), Some("Commander"));

        let blank = r#"
[paths]
ra2_dir = "C:/Westwood/RA2"

[profile]
name = "   "
"#;
        let config = GameConfig::parse(blank).expect("Failed to parse test config");
        assert_eq!(config.profile.player_name(), None);
    }

    #[test]
    fn reads_comments_both_string_kinds_and_skips_unknown_keys() {
        let text = "\u{feff}# comment\n[paths] # header comment\nra2_dir = 'C:\\RA2' # literal\n\n\
                    [graphics]\nvsync = [1, 2]\nextra_animations = false # off\n\
                    [gameplay]\ninput_delay_ticks = 1_0\n\
                    [profile]\nname = \"Tab\\there \\u00E9 \\\"q\\\" # not a comment\"\n\
                    [[unknown]]\nanything = { goes = 1 }\n";
        let config = GameConfig::parse(text).unwrap();
        assert_eq!(config.paths.ra2_dir, PathBuf::from("C:\\RA2"));
        assert!(!config.graphics.extra_animations);
        assert_eq!(config.gameplay.input_delay_ticks, 10);
        assert_eq!(
            config.profile.name.as_deref(),
            Some("Tab\there é \"q\" # not a comment")
        );
    }

    #[test]
    fn rejects_values_toml_rejects() {
        for text in [
            "[paths]\nra2_dir = \"C:\\RA2\"\n",
            "[paths]\nra2_dir = 'a'\nra2_dir = 'b'\n",
            "[paths]\nra2_dir = 'a' b\n",
            "[paths]\nra2_dir = \"a\n",
            "[paths]\nra2_dir = a\n",
            "[paths]\nra2_dir\n",
            "[paths]\nra2_dir = 'a'\n[gameplay]\ninput_delay_ticks = -1\n",
            "[paths]\nra2_dir = 'a'\n[graphics]\nextra_animations = yes\n",
            "[graphics]\nextra_animations = true\n",
        ] {
            assert!(GameConfig::parse(text).is_err(), "{text:?}");
        }
    }

    /// Host-native paths: `Path` only splits on the host's separators, so a
    /// Windows literal has no parent on Linux or macOS.
    fn host_path(windows: &str, unix: &str) -> PathBuf {
        PathBuf::from(if cfg!(windows) { windows } else { unix })
    }

    #[test]
    fn retail_asset_root_is_the_executable_directory() {
        let directory = host_path(r"C:\Westwood\RA2", "/opt/westwood/ra2");
        let executable = directory.join("gamemd.exe");
        assert_eq!(
            retail_asset_root_from_executable(&executable).expect("module directory"),
            directory
        );
    }

    #[test]
    fn retail_asset_root_preserves_a_volume_root_boundary() {
        let root = host_path(r"C:\", "/");
        let executable = root.join("gamemd.exe");
        assert_eq!(
            retail_asset_root_from_executable(&executable).expect("volume root"),
            root
        );
    }

    #[test]
    fn discovered_config_keeps_host_defaults() {
        let config = GameConfig::from_retail_asset_root(PathBuf::from(r"D:\Games\RA2"));
        assert_eq!(config.paths.ra2_dir, PathBuf::from(r"D:\Games\RA2"));
        assert!(config.graphics.extra_animations);
        assert_eq!(config.profile.player_name(), None);
    }
}
