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

use anyhow::{Context, Result};
use serde::Deserialize;

/// Optional host override, in the working or executable directory.
const CONFIG_FILE_NAME: &str = "config.toml";

/// Top-level game configuration, deserialized from config.toml.
///
/// Add new sections here as features are implemented (audio, game speed, etc.).
#[derive(Debug, Deserialize)]
pub struct GameConfig {
    /// File system paths (RA2 install directory).
    pub paths: PathsConfig,
    /// Graphics/window settings (all optional — sensible defaults provided).
    #[serde(default)]
    pub graphics: GraphicsConfig,
    /// Deterministic simulation settings.
    #[serde(default)]
    pub gameplay: GameplayConfig,
    /// Local player profile (name pre-filled into skirmish/multiplayer setup).
    #[serde(default)]
    pub profile: ProfileConfig,
}

/// Local player profile settings.
///
/// `[profile]` may be omitted entirely. `name` is the persistent player handle
/// the setup screen pre-fills into the name field; when unset the setup UI
/// falls back to its own default. This mirrors the original reading the player
/// name from a persistent profile source rather than a baked-in string.
#[derive(Debug, Deserialize, Default)]
pub struct ProfileConfig {
    /// Player name shown/edited in skirmish setup. `None` (or empty) means use
    /// the setup screen's built-in default.
    #[serde(default)]
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
#[derive(Debug, Deserialize)]
pub struct PathsConfig {
    /// Path to the user's RA2 installation directory.
    /// MIX files (ra2.mix, language.mix, theme.mix) are loaded from here.
    /// Example: "C:/Program Files/EA Games/Command and Conquer Red Alert II"
    pub ra2_dir: PathBuf,
}

/// Graphics and window settings.
///
/// Every field has a sensible default so `[graphics]` can be omitted entirely.
#[derive(Debug, Deserialize)]
pub struct GraphicsConfig {
    /// Window width in pixels.
    #[serde(default = "default_width")]
    pub width: u32,
    /// Window height in pixels.
    #[serde(default = "default_height")]
    pub height: u32,
    /// Whether to enable vertical sync (reduces tearing, caps framerate).
    #[serde(default = "default_true")]
    pub vsync: bool,
    /// Enable Catmull-Rom bicubic upscaling (renders at half resolution, upscales to window).
    #[serde(default)]
    pub upscale: bool,
    /// Enable cosmetic per-frame effects: water/ore sparkles. Also intended to
    /// gate future cosmetic effects (laser beam pulses, particle systems, line
    /// trails) per gamemd's "Extra Animations" option. Default ON to match
    /// gamemd's default.
    #[serde(default = "default_true")]
    pub extra_animations: bool,
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            width: default_width(),
            height: default_height(),
            vsync: true,
            upscale: false,
            extra_animations: true,
        }
    }
}

impl GraphicsConfig {
    /// Render width: half of window width when upscaling, otherwise full window width.
    pub fn render_width(&self) -> u32 {
        if self.upscale {
            self.width / 2
        } else {
            self.width
        }
    }

    /// Render height: half of window height when upscaling, otherwise full window height.
    pub fn render_height(&self) -> u32 {
        if self.upscale {
            self.height / 2
        } else {
            self.height
        }
    }
}

/// Simulation pacing and command scheduling settings.
#[derive(Debug, Deserialize)]
pub struct GameplayConfig {
    /// Fixed simulation tick rate (Hz).
    #[serde(default = "default_sim_tick_hz")]
    pub sim_tick_hz: u32,
    /// Input delay in ticks for lockstep-style command execution.
    #[serde(default = "default_input_delay_ticks")]
    pub input_delay_ticks: u32,
    /// Freeze a running match while the window is not the foreground, as
    /// gamemd does when `WM_ACTIVATEAPP` clears. Off by default: the match
    /// keeps simulating and playing audio behind other windows, and only
    /// input stops. Local pacing policy; never read by the simulation.
    #[serde(default)]
    pub pause_on_focus_loss: bool,
}

impl Default for GameplayConfig {
    fn default() -> Self {
        Self {
            sim_tick_hz: default_sim_tick_hz(),
            input_delay_ticks: default_input_delay_ticks(),
            pause_on_focus_loss: false,
        }
    }
}

fn default_width() -> u32 {
    1024
}

fn default_height() -> u32 {
    768
}

fn default_true() -> bool {
    true
}

fn default_sim_tick_hz() -> u32 {
    15
}

fn default_input_delay_ticks() -> u32 {
    2
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

    fn load_from(working_dir: &Path, executable: &Path) -> Result<Self> {
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
        let mut config: GameConfig = toml::from_str(contents)
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

    fn from_retail_asset_root(ra2_dir: PathBuf) -> Self {
        Self {
            paths: PathsConfig { ra2_dir },
            graphics: GraphicsConfig::default(),
            gameplay: GameplayConfig::default(),
            profile: ProfileConfig::default(),
        }
    }
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
        let config: GameConfig = toml::from_str(toml_str).expect("Failed to parse test config");
        assert_eq!(config.graphics.width, 1024);
        assert_eq!(config.graphics.height, 768);
        assert!(config.graphics.vsync);
        assert!(!config.graphics.upscale);
        assert_eq!(config.gameplay.sim_tick_hz, 15);
        assert_eq!(config.gameplay.input_delay_ticks, 2);
        assert!(!config.gameplay.pause_on_focus_loss);
        // No [profile] section -> no pre-filled player name.
        assert_eq!(config.profile.player_name(), None);
    }

    #[test]
    fn test_profile_player_name_trims_and_blank_is_none() {
        let toml_str = r#"
[paths]
ra2_dir = "C:/Westwood/RA2"

[profile]
name = "  Commander  "
"#;
        let config: GameConfig = toml::from_str(toml_str).expect("Failed to parse test config");
        assert_eq!(config.profile.player_name(), Some("Commander"));

        let blank = r#"
[paths]
ra2_dir = "C:/Westwood/RA2"

[profile]
name = "   "
"#;
        let config: GameConfig = toml::from_str(blank).expect("Failed to parse test config");
        assert_eq!(config.profile.player_name(), None);
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
        assert_eq!(config.graphics.width, 1024);
        assert_eq!(config.graphics.height, 768);
        assert_eq!(config.gameplay.sim_tick_hz, 15);
        assert_eq!(config.profile.player_name(), None);
    }
}
