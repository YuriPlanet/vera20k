//! Neutral top-level launch dispatch for interactive and capture modes.
//!
//! Capture vectors reach their sealed parsers unchanged. Interactive arguments
//! pass through the retail switch table before shell dispatch. This module
//! owns only mode routing and the strict tactical profile/contract/output
//! boundary.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::app::diagnostics::shell_capture::{
    self, AppLaunchMode as ShellAppLaunchMode, ShellCaptureRequest,
};
use crate::app::diagnostics::tactical_capture::profile::{
    CHECKPOINT_RADAR_ONLINE_V2, SealedJsonFile, TacticalCaptureContract, TacticalCaptureProfile,
    validate_new_output_directory,
};
use crate::app::frontend::startup_options::{RetailStartupOptions, consume_retail_switches};
use crate::skirmish_launch::SkirmishLaunchSession;

use crate::app::diagnostics::tactical_capture::session::MapCaptureProfile;

const TACTICAL_CAPTURE_FLAG: &str = "--tactical-capture";

/// Standalone replay verification runs before window, audio, profile or online
/// service startup. Retail bytes are opened read-only by the existing loader.
pub fn try_replay_verify<I>(args: I) -> Option<Result<i32>>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(OsStr::new("--replay-verify")) {
        return None;
    }
    Some((|| {
        let path = PathBuf::from(args.next().context("--replay-verify needs a replay file")?);
        ensure!(
            args.next().as_deref() == Some(OsStr::new("--ra2-dir")),
            "usage: vera20k --replay-verify <replay.json> --ra2-dir <retail-directory>"
        );
        let retail_dir = PathBuf::from(args.next().context("--ra2-dir needs a retail directory")?);
        ensure!(
            args.next().is_none(),
            "unexpected replay verification argument"
        );
        let replay = crate::sim::replay::ReplayLog::load(&path)?;
        let map_sha256 = replay.standalone_startup()?.map_sha256();
        let map_beside = map_beside_replay(&path, &replay.header.map_name, map_sha256)?;
        let receipt =
            crate::sim::replay::ReplayRunner::verify(&retail_dir, &replay, map_beside.as_deref())?;
        println!("{}", serde_json::to_string(&receipt)?);
        Ok(0)
    })())
}

/// The installer runs this before normal application startup. Keep its argument
/// boundary here with the other launch modes: game-folder values must bypass
/// native substring switches, and Unicode paths must remain OS strings.
pub fn try_game_data_check<I>(args: I) -> Option<Result<i32>>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(OsStr::new("--check-game-data")) {
        return None;
    }
    Some((|| {
        let game_dir = PathBuf::from(
            args.next()
                .context("missing game folder after --check-game-data")?,
        );
        let output_dir = PathBuf::from(args.next().context("missing game-data report directory")?);
        ensure!(
            args.next().is_none(),
            "--check-game-data takes a game folder and report directory"
        );
        crate::app::game_data_check::check_and_write_report(&game_dir, &output_dir)
    })())
}

/// `vera20k --probe [--ra2-dir <dir>] [--map <path>]` (docs/spawn.md): print
/// one JSON object describing what a spawn launch on this installation is
/// checked against, then exit. Headless: no logger, window or audio.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn try_probe<I>(args: I) -> Option<Result<i32>>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(OsStr::new("--probe")) {
        return None;
    }
    Some((|| {
        let (ra2_dir, map) = parse_probe_args(args)?;
        let report = crate::app::spawn_probe::run_desktop(ra2_dir.as_deref(), map.as_deref())?;
        println!("{}", serde_json::to_string(&report)?);
        Ok(0)
    })())
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn parse_probe_args(
    args: impl Iterator<Item = OsString>,
) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
    const USAGE: &str = "usage: vera20k --probe [--ra2-dir <retail-directory>] [--map <map-file>]";
    let mut args = args;
    let (mut ra2_dir, mut map) = (None, None);
    while let Some(flag) = args.next() {
        let slot = if flag == OsStr::new("--ra2-dir") {
            &mut ra2_dir
        } else if flag == OsStr::new("--map") {
            &mut map
        } else {
            bail!("unexpected probe argument {flag:?}; {USAGE}");
        };
        ensure!(slot.is_none(), "{flag:?} given twice; {USAGE}");
        *slot =
            Some(PathBuf::from(args.next().with_context(|| {
                format!("{flag:?} needs a value; {USAGE}")
            })?));
    }
    Ok((ra2_dir, map))
}

#[derive(Debug)]
pub enum AppLaunchMode {
    /// Ordinary launch, carrying whatever the retail switch table decided.
    Interactive(RetailStartupOptions),
    /// CnCNet's offline launch contract; configuration is read before creating a window.
    Spawn(RetailStartupOptions),
    /// Watch a strictly reconstructed recording through the normal renderer.
    Replay(ReplayLaunchRequest),
    /// A help switch: print usage and terminate before any window is created,
    /// exactly as native's switch parser makes `WinMain` bail.
    Usage,
    ShellCapture(ShellCaptureRequest),
    TacticalCapture(TacticalCaptureRequest),
}

#[derive(Debug)]
pub struct ReplayLaunchRequest {
    pub(crate) replay: std::sync::Arc<crate::sim::replay::ReplayLog>,
    pub(crate) retail_dir: PathBuf,
    pub(crate) capture_output: Option<PathBuf>,
    pub(crate) replay_sha256: String,
    /// The recorded map beside the replay file: a -SPAWN game's map exists in
    /// no game folder. A file with other bytes is ignored.
    pub(crate) map_beside: Option<String>,
}

fn map_beside_replay(replay: &Path, map_name: &str, map_sha256: &str) -> Result<Option<String>> {
    Ok(replay
        .canonicalize()?
        .parent()
        // A rooted path's scenario name is only its file name, so a logical
        // name with a directory must keep resolving through the game folder.
        .filter(|_| !map_name.contains(['/', '\\']))
        .map(|directory| directory.join(map_name))
        .filter(|map| {
            std::fs::read(map)
                .is_ok_and(|bytes| crate::util::sha256::sha256_hex(&bytes) == map_sha256)
        })
        .and_then(|map| map.to_str().map(str::to_owned)))
}

fn parse_replay_args(args: Vec<OsString>) -> Result<AppLaunchMode> {
    let mut args = args.into_iter();
    ensure!(
        args.next().as_deref() == Some(OsStr::new("--replay")),
        "--replay must be first"
    );
    let path = PathBuf::from(args.next().context("--replay needs a replay file")?);
    ensure!(
        args.next().as_deref() == Some(OsStr::new("--ra2-dir")),
        "usage: vera20k --replay <replay.json> --ra2-dir <retail-directory> [--replay-capture <new.png>]"
    );
    let retail_dir = PathBuf::from(args.next().context("--ra2-dir needs a retail directory")?)
        .canonicalize()
        .context("replay retail directory")?;
    ensure!(retail_dir.is_dir(), "replay retail path is not a directory");
    let capture_output = match args.next() {
        None => None,
        Some(flag) => {
            ensure!(
                flag == OsStr::new("--replay-capture"),
                "unexpected replay argument"
            );
            let output = PathBuf::from(
                args.next()
                    .context("--replay-capture needs a new PNG path")?,
            );
            ensure!(
                output.is_absolute() && output.extension() == Some(OsStr::new("png")),
                "replay capture needs an absolute .png path"
            );
            let parent = output
                .parent()
                .context("replay capture parent")?
                .canonicalize()?;
            ensure!(
                !parent.ancestors().any(|dir| dir.join(".git").exists()),
                "replay capture output must be outside a source checkout"
            );
            let output = parent.join(output.file_name().context("replay capture filename")?);
            ensure!(
                !output.exists() && !output.with_extension("json").exists(),
                "replay capture output already exists"
            );
            Some(output)
        }
    };
    ensure!(args.next().is_none(), "unexpected replay argument");
    let (replay, replay_sha256) = crate::sim::replay::ReplayLog::load_bound(&path)?;
    let map_sha256 = replay.standalone_startup()?.map_sha256();
    let map_beside = map_beside_replay(&path, &replay.header.map_name, map_sha256)?;
    Ok(AppLaunchMode::Replay(ReplayLaunchRequest {
        map_beside,
        replay: std::sync::Arc::new(replay),
        retail_dir,
        capture_output,
        replay_sha256,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TacticalCaptureCheckpoint {
    RadarOnlineV2,
    MapObserveV1,
}

impl TacticalCaptureCheckpoint {
    fn parse(value: &str) -> Result<Self> {
        match value {
            CHECKPOINT_RADAR_ONLINE_V2 => Ok(Self::RadarOnlineV2),
            "map-observe-v1" => Ok(Self::MapObserveV1),
            _ => bail!("unsupported tactical-capture checkpoint {value:?}"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RadarOnlineV2 => CHECKPOINT_RADAR_ONLINE_V2,
            Self::MapObserveV1 => "map-observe-v1",
        }
    }
}

#[derive(Debug, Clone)]
enum CaptureProfile {
    Radar(Box<SealedJsonFile<TacticalCaptureProfile>>),
    Map(Box<SealedJsonFile<MapCaptureProfile>>),
}

#[derive(Debug, Clone)]
pub struct TacticalCaptureRequest {
    checkpoint: TacticalCaptureCheckpoint,
    profile: CaptureProfile,
    contract: SealedJsonFile<TacticalCaptureContract>,
    output_dir: PathBuf,
}

impl TacticalCaptureRequest {
    pub fn checkpoint(&self) -> TacticalCaptureCheckpoint {
        self.checkpoint
    }

    pub(crate) fn profile(&self) -> Result<&TacticalCaptureProfile> {
        Ok(&self.sealed_profile()?.value)
    }

    pub(crate) fn sealed_profile(&self) -> Result<&SealedJsonFile<TacticalCaptureProfile>> {
        match &self.profile {
            CaptureProfile::Radar(profile) => Ok(profile),
            CaptureProfile::Map(_) => bail!("radar profile requested for map observation"),
        }
    }

    pub(crate) fn map_profile(&self) -> Option<&SealedJsonFile<MapCaptureProfile>> {
        match &self.profile {
            CaptureProfile::Map(profile) => Some(profile),
            CaptureProfile::Radar(_) => None,
        }
    }

    pub(crate) fn seed(&self) -> u32 {
        match &self.profile {
            CaptureProfile::Radar(p) => p.value.launch.seed,
            CaptureProfile::Map(p) => p.value.seed,
        }
    }

    pub(crate) fn child_timeout_seconds(&self) -> u32 {
        match &self.profile {
            CaptureProfile::Radar(p) => p.value.budgets.child_timeout_seconds,
            CaptureProfile::Map(p) => p.value.timeout_seconds,
        }
    }

    pub(crate) fn post_l0_timeout_seconds(&self) -> u32 {
        match &self.profile {
            CaptureProfile::Radar(p) => p.value.budgets.post_l0_timeout_seconds,
            CaptureProfile::Map(p) => p.value.timeout_seconds,
        }
    }

    pub(crate) fn sealed_contract(&self) -> &SealedJsonFile<TacticalCaptureContract> {
        &self.contract
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub fn width(&self) -> u32 {
        match &self.profile {
            CaptureProfile::Radar(p) => p.value.capture.output_width,
            CaptureProfile::Map(p) => p.value.width,
        }
    }

    pub fn height(&self) -> u32 {
        match &self.profile {
            CaptureProfile::Radar(p) => p.value.capture.output_height,
            CaptureProfile::Map(p) => p.value.height,
        }
    }

    pub(crate) fn launch_session(&self) -> SkirmishLaunchSession {
        match &self.profile {
            CaptureProfile::Radar(p) => p.value.launch_session(),
            CaptureProfile::Map(p) => p.value.launch.clone(),
        }
    }

    pub fn validate_runtime_environment(&self) -> Result<()> {
        self.contract.value.validate_environment()
    }
}

/// Parse the neutral application launch boundary.
///
/// Capture option values bypass the retail switch table. Ordinary arguments
/// pass through that table, then the shell parser rejects anything left over.
/// Tactical capture requires its flag first; malformed vectors never silently
/// open an interactive window.
pub fn parse_launch_args<I>(args: I) -> Result<AppLaunchMode>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = args.into_iter().collect();
    if args
        .iter()
        .any(|arg| arg == OsStr::new("--replay") || arg == OsStr::new("--replay-capture"))
    {
        return parse_replay_args(args);
    }
    // Native's table scans every argument, which is safe there because native
    // has no option that takes a following value. VERA's capture modes do, and
    // the `-CD` search is a substring match, so an output path such as
    // `--output out\shell-cd-compare` would have its value silently eaten and
    // the capture parser would then fail pointing at the wrong argument. A
    // capture invocation therefore bypasses the table entirely and reaches its
    // sealed parser byte-for-byte.
    if is_capture_invocation(&args) {
        if args.first() == Some(&OsString::from(TACTICAL_CAPTURE_FLAG)) {
            return parse_tactical_args(args);
        }
        return match shell_capture::parse_launch_args(args)? {
            ShellAppLaunchMode::Interactive => {
                Ok(AppLaunchMode::Interactive(RetailStartupOptions::default()))
            }
            ShellAppLaunchMode::ShellCapture(request) => Ok(AppLaunchMode::ShellCapture(request)),
        };
    }
    // This is the only process-argument owner for retail startup decisions.
    let (retail_options, args) = consume_retail_switches(args);
    if retail_options.usage_requested {
        return Ok(AppLaunchMode::Usage);
    }
    let spawn_count = args
        .iter()
        .filter(|arg| {
            arg.to_str()
                .is_some_and(|arg| arg.eq_ignore_ascii_case("-SPAWN"))
        })
        .count();
    if spawn_count != 0 {
        ensure!(
            spawn_count == 1 && args.len() == 1,
            "-SPAWN cannot be combined with other launch modes or unknown arguments"
        );
        return Ok(AppLaunchMode::Spawn(retail_options));
    }
    match shell_capture::parse_launch_args(args)? {
        ShellAppLaunchMode::Interactive => Ok(AppLaunchMode::Interactive(retail_options)),
        ShellAppLaunchMode::ShellCapture(request) => Ok(AppLaunchMode::ShellCapture(request)),
    }
}

/// Whether this vector is driving one of the capture harnesses, in which case
/// its option values must reach the sealed parsers untouched.
fn is_capture_invocation(args: &[OsString]) -> bool {
    args.iter().any(|argument| {
        argument == OsStr::new(shell_capture::CAPTURE_FLAG)
            || argument == OsStr::new(TACTICAL_CAPTURE_FLAG)
    })
}

fn parse_tactical_args(args: Vec<OsString>) -> Result<AppLaunchMode> {
    let mut args = args.into_iter();
    let first = args.next().context("missing tactical-capture flag")?;
    ensure!(
        first == TACTICAL_CAPTURE_FLAG,
        "internal tactical dispatch mismatch"
    );
    let checkpoint_text = next_utf8(&mut args, "checkpoint after --tactical-capture")?;
    let checkpoint = TacticalCaptureCheckpoint::parse(&checkpoint_text)?;
    let mut profile_path = None;
    let mut contract_path = None;
    let mut output_dir = None;

    while let Some(flag) = args.next() {
        let flag = flag
            .into_string()
            .map_err(|_| anyhow::anyhow!("tactical-capture option name is not valid UTF-8"))?;
        match flag.as_str() {
            "--profile" => set_once(
                &mut profile_path,
                PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("missing value after --profile"))?,
                ),
                "--profile",
            )?,
            "--contract" => set_once(
                &mut contract_path,
                PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("missing value after --contract"))?,
                ),
                "--contract",
            )?,
            "--output" => set_once(
                &mut output_dir,
                PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("missing value after --output"))?,
                ),
                "--output",
            )?,
            _ => bail!("unknown tactical-capture option {flag:?}"),
        }
    }

    let profile_path = profile_path.context("missing required --profile")?;
    let contract_path = contract_path.context("missing required --contract")?;
    let output_dir = output_dir.context("missing required --output")?;
    let profile = match checkpoint {
        TacticalCaptureCheckpoint::RadarOnlineV2 => {
            let profile = TacticalCaptureProfile::load_strict(&profile_path)?;
            ensure!(
                profile.value.checkpoint == checkpoint.as_str(),
                "profile checkpoint differs"
            );
            CaptureProfile::Radar(Box::new(profile))
        }
        TacticalCaptureCheckpoint::MapObserveV1 => {
            CaptureProfile::Map(Box::new(MapCaptureProfile::load(&profile_path)?))
        }
    };
    let contract = TacticalCaptureContract::load_external(&contract_path)?;
    if let CaptureProfile::Radar(profile) = &profile {
        ensure!(
            profile.value.budgets.absolute_timeout_max_seconds
                == contract.value.absolute_max_child_timeout_seconds,
            "profile and contract absolute timeout maxima differ"
        );
    }
    validate_new_output_directory(&output_dir)?;

    Ok(AppLaunchMode::TacticalCapture(TacticalCaptureRequest {
        checkpoint,
        profile,
        contract,
        output_dir,
    }))
}

fn next_utf8<I>(args: &mut I, what: &str) -> Result<String>
where
    I: Iterator<Item = OsString>,
{
    args.next()
        .ok_or_else(|| anyhow::anyhow!("missing {what}"))?
        .into_string()
        .map_err(|_| anyhow::anyhow!("{what} is not valid UTF-8"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<()> {
    ensure!(slot.is_none(), "duplicate tactical-capture option {flag}");
    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn probe_dispatch_is_strict_and_only_first() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(try_probe(args(&["-WIN", "--probe"])).is_none());
        assert!(try_probe(args(&["-SPAWN"])).is_none());
        for bad in [
            &["--bogus"][..],
            &["--map"],
            &["--ra2-dir"],
            &["--map", "a", "--map", "b"],
            &["--ra2-dir", "a", "-WIN"],
        ] {
            assert!(parse_probe_args(args(bad).into_iter()).is_err(), "{bad:?}");
        }
        assert_eq!(
            parse_probe_args(args(&["--map", "m.ini", "--ra2-dir", "r"]).into_iter()).unwrap(),
            (Some(PathBuf::from("r")), Some(PathBuf::from("m.ini")))
        );
        assert_eq!(parse_probe_args(std::iter::empty()).unwrap(), (None, None));
        // The interactive parser does not treat --probe as a launch mode.
        assert!(!matches!(
            parse_launch_args(args(&["--probe"])),
            Ok(AppLaunchMode::Spawn(_))
        ));
    }

    #[test]
    fn standalone_replay_dispatch_is_strict_and_does_not_enter_interactive_startup() {
        assert!(try_replay_verify([OsString::from("-WIN")]).is_none());
        assert!(
            try_replay_verify([OsString::from("--replay-verify")])
                .unwrap()
                .is_err()
        );
        let bad = ["--replay-verify", "record.json", "--password", "unused"];
        assert!(
            try_replay_verify(bad.into_iter().map(OsString::from))
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn rendered_replay_routes_exact_values_and_requires_standalone_startup_and_new_output() {
        let root = test_directory("replay-cd-values");
        let path = root.join("record-cd.json");
        let log = crate::sim::replay::ReplayLog::new(crate::sim::replay::ReplayHeader {
            version: 2,
            tick_hz: 45,
            seed: 7,
            map_name: "XMP03T4.MAP".into(),
            rules_hash: 0,
            pixel_conversion_bounds: Default::default(),
        });
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        let mut args = vec![
            OsString::from("--replay"),
            path.into_os_string(),
            OsString::from("--ra2-dir"),
            root.clone().into_os_string(),
        ];
        assert!(
            parse_launch_args(args.clone())
                .unwrap_err()
                .to_string()
                .contains("no standalone startup")
        );
        let output = root.join("capture.png");
        std::fs::write(&output, b"preserve").unwrap();
        args.extend([
            OsString::from("--replay-capture"),
            output.clone().into_os_string(),
        ]);
        assert!(
            parse_launch_args(args)
                .unwrap_err()
                .to_string()
                .contains("already exists")
        );
        assert_eq!(std::fs::read(output).unwrap(), b"preserve");
        assert!(parse_launch_args([OsString::from("--replay-capture")]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_replay_uses_the_map_beside_it_only_when_present() {
        let root = test_directory("replay-map-beside");
        let replay = root.join("replay.json");
        std::fs::write(&replay, b"{}").unwrap();
        let sha256 = crate::util::sha256::sha256_hex(b"map");
        assert_eq!(
            map_beside_replay(&replay, "spawnmap.ini", &sha256).unwrap(),
            None
        );
        std::fs::create_dir(root.join("Maps")).unwrap();
        std::fs::write(root.join("Maps/spawnmap.ini"), b"map").unwrap();
        assert_eq!(
            map_beside_replay(&replay, "Maps/spawnmap.ini", &sha256).unwrap(),
            None
        );
        let map = root.join("spawnmap.ini");
        std::fs::write(&map, b"other").unwrap();
        assert_eq!(
            map_beside_replay(&replay, "spawnmap.ini", &sha256).unwrap(),
            None
        );
        std::fs::write(&map, b"map").unwrap();
        let found = map_beside_replay(&replay, "spawnmap.ini", &sha256)
            .unwrap()
            .unwrap();
        assert_eq!(PathBuf::from(found), map.canonicalize().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn test_directory(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "vera20k-tactical-launch-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).expect("create test directory");
        // macOS keeps the temp dir under `/var`, a symlink to `/private/var`,
        // which the tactical output guard rightly refuses; test the resolved path.
        #[cfg(unix)]
        let directory = std::fs::canonicalize(&directory).expect("canonical test directory");
        directory
    }

    fn tactical_args(output: &Path) -> Vec<OsString> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        vec![
            TACTICAL_CAPTURE_FLAG.into(),
            CHECKPOINT_RADAR_ONLINE_V2.into(),
            "--profile".into(),
            root.join("tools/tactical_certification/profiles/soviet-radar-online-v3.json")
                .into_os_string(),
            "--contract".into(),
            root.join("src/app/diagnostics/tactical_capture/contract.v2.json")
                .into_os_string(),
            "--output".into(),
            output.as_os_str().to_owned(),
        ]
    }

    #[test]
    fn map_observation_routes_through_tactical_capture_and_refuses_radar_profile() {
        let parent = test_directory("map-observe");
        let output = parent.join("capture");
        let mut args = tactical_args(&output);
        args[1] = "map-observe-v1".into();
        assert!(parse_launch_args(args.clone()).is_err());
        args[3] = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tools/map_observation.example.json")
            .into_os_string();
        let AppLaunchMode::TacticalCapture(request) = parse_launch_args(args.clone()).unwrap()
        else {
            panic!("wrong capture lifecycle");
        };
        assert_eq!(
            request.checkpoint(),
            TacticalCaptureCheckpoint::MapObserveV1
        );
        assert_eq!(request.map_profile().unwrap().value.ticks, 30);
        assert!(request.profile().is_err());
        std::fs::create_dir(&output).unwrap();
        assert!(parse_launch_args(args).is_err());
        std::fs::remove_dir(&output).unwrap();
        std::fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn cncnet_spawn_rejects_duplicate_and_conflicting_modes() {
        for args in [
            vec!["-SPAWN", "-spawn"],
            vec!["-SPAWN", "--typo"],
            vec!["-SPAWN", "--shell-capture"],
            vec!["-SPAWN", "--tactical-capture"],
        ] {
            assert!(parse_launch_args(args.into_iter().map(OsString::from)).is_err());
        }
    }

    #[test]
    fn a_capture_option_value_containing_cd_is_not_stripped() {
        // The retail table matches `-CD` as a substring, so a capture value
        // like this would lose its `--output` argument if the table ran over
        // capture vectors. The tactical parser must still see the path.
        let parent = test_directory("cd-compare");
        let output = parent.join("shell-cd-compare");
        let launch = parse_launch_args(tactical_args(&output)).expect("parse");
        let AppLaunchMode::TacticalCapture(request) = launch else {
            panic!("a -CD-containing output path must stay a tactical capture");
        };
        assert_eq!(request.output_dir(), output);
        let app = crate::app::App::new_tactical_capture(request);
        assert_eq!(
            app.startup_options.media_archive_mode,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        );
        std::fs::remove_dir(parent).expect("remove test directory");
    }

    #[test]
    fn tactical_parser_loads_exact_profile_contract_and_new_output() {
        let parent = test_directory("valid");
        let output = parent.join("capture");
        let launch = parse_launch_args(tactical_args(&output)).expect("parse");
        let AppLaunchMode::TacticalCapture(request) = launch else {
            panic!("wrong launch mode");
        };
        assert_eq!(
            request.checkpoint(),
            TacticalCaptureCheckpoint::RadarOnlineV2
        );
        assert_eq!(
            request.profile().unwrap().profile_id,
            "soviet-radar-online-v3"
        );
        assert_eq!(request.width(), 800);
        assert_eq!(request.height(), 600);
        assert_eq!(request.output_dir(), output);
        assert_eq!(
            request.launch_session().selected_map_file.as_deref(),
            Some("Fight.MAP")
        );
        std::fs::remove_dir(parent).expect("remove test directory");
    }

    #[test]
    fn tactical_parser_rejects_duplicates_mixed_flags_and_relative_output() {
        let parent = test_directory("invalid");
        let output = parent.join("capture");
        let mut duplicate = tactical_args(&output);
        duplicate.extend(["--output".into(), parent.join("other").into_os_string()]);
        assert!(
            parse_launch_args(duplicate)
                .expect_err("duplicate")
                .to_string()
                .contains("duplicate tactical-capture option --output")
        );

        let mut mixed = tactical_args(&output);
        mixed.extend(["--shell-capture".into(), "main-menu-0xe2-steady".into()]);
        assert!(
            parse_launch_args(mixed)
                .expect_err("mixed")
                .to_string()
                .contains("unknown tactical-capture option")
        );

        let mut relative = tactical_args(&output);
        let index = relative
            .iter()
            .position(|value| value == &OsString::from("--output"))
            .expect("output flag");
        relative[index + 1] = OsString::from("relative-capture");
        assert!(
            parse_launch_args(relative)
                .expect_err("relative")
                .to_string()
                .contains("--output must be an absolute path")
        );
        std::fs::remove_dir(parent).expect("remove test directory");
    }
}
