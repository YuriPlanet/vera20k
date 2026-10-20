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

#[derive(Debug)]
pub enum AppLaunchMode {
    /// Ordinary launch, carrying whatever the retail switch table decided.
    Interactive(RetailStartupOptions),
    /// CnCNet's offline launch contract; configuration is read before creating a window.
    Spawn(RetailStartupOptions),
    /// A help switch: print usage and terminate before any window is created,
    /// exactly as native's switch parser makes `WinMain` bail.
    Usage,
    ShellCapture(ShellCaptureRequest),
    TacticalCapture(TacticalCaptureRequest),
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
