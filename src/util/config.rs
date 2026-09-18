//! Game configuration and retail asset-root discovery.
//!
//! config.toml is machine-specific (contains the local RA2 install path)
//! and is gitignored. A config.toml.example template is provided in the repo.
//! When it is absent, the retail executable contract applies: assets are
//! resolved relative to the directory containing the running executable.
//!
//! ## Dependency rules
//! - config.rs is part of util/ â€” no dependencies on game modules.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Optional host override â€” looked up in the current working directory.
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

/// Deterministic simulation and command scheduling settings.
#[derive(Debug, Deserialize)]
pub struct GameplayConfig {
    /// Fixed simulation tick rate (Hz).
    #[serde(default = "default_sim_tick_hz")]
    pub sim_tick_hz: u32,
    /// Input delay in ticks for lockstep-style command execution.
    #[serde(default = "default_input_delay_ticks")]
    pub input_delay_ticks: u32,
}

impl Default for GameplayConfig {
    fn default() -> Self {
        Self {
            sim_tick_hz: default_sim_tick_hz(),
            input_delay_ticks: default_input_delay_ticks(),
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
        let path = Path::new(CONFIG_FILE_NAME);
        match std::fs::read_to_string(path) {
            Ok(contents) => Self::parse_from(&contents, path),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let executable = std::env::current_exe().context(
                    "Failed to locate the running executable for retail asset discovery",
                )?;
                let root = retail_asset_root_from_executable(&executable)?;
                log::info!(
                    "No {}; using retail executable directory: {}",
                    CONFIG_FILE_NAME,
                    root.display()
                );
                Ok(Self::from_retail_asset_root(root))
            }
            Err(err) => {
                Err(err).with_context(|| format!("Failed to read config file: {}", path.display()))
            }
        }
    }

    fn parse_from(contents: &str, path: &Path) -> Result<Self> {
        let config: GameConfig = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

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

    #[test]
    fn retail_asset_root_is_the_executable_directory() {
        let executable = Path::new(r"C:\Westwood\RA2\gamemd.exe");
        assert_eq!(
            retail_asset_root_from_executable(executable).expect("module directory"),
            PathBuf::from(r"C:\Westwood\RA2")
        );
    }

    #[test]
    fn retail_asset_root_preserves_a_volume_root_boundary() {
        let executable = Path::new(r"C:\gamemd.exe");
        assert_eq!(
            retail_asset_root_from_executable(executable).expect("volume root"),
            PathBuf::from(r"C:\")
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
