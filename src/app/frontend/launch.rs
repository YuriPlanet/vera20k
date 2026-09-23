//! Neutral top-level launch dispatch for interactive and capture modes.
//!
//! The retail switch table is consumed first, then every remaining non-tactical
//! argument is delegated byte-for-byte to the sealed shell parser. This module
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

const TACTICAL_CAPTURE_FLAG: &str = "--tactical-capture";

#[derive(Debug)]
pub enum AppLaunchMode {
    /// Ordinary launch, carrying whatever the retail switch table decided.
    Interactive(RetailStartupOptions),
    /// A help switch: print usage and terminate before any window is created,
    /// exactly as native's switch parser makes `WinMain` bail.
    Usage,
    ShellCapture(ShellCaptureRequest),
    TacticalCapture(TacticalCaptureRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TacticalCaptureCheckpoint {
    RadarOnlineV2,
}

impl TacticalCaptureCheckpoint {
    fn parse(value: &str) -> Result<Self> {
        match value {
            CHECKPOINT_RADAR_ONLINE_V2 => Ok(Self::RadarOnlineV2),
            _ => bail!("unsupported tactical-capture checkpoint {value:?}"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RadarOnlineV2 => CHECKPOINT_RADAR_ONLINE_V2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TacticalCaptureRequest {
    checkpoint: TacticalCaptureCheckpoint,
    profile: SealedJsonFile<TacticalCaptureProfile>,
    contract: SealedJsonFile<TacticalCaptureContract>,
    output_dir: PathBuf,
}

impl TacticalCaptureRequest {
    pub fn checkpoint(&self) -> TacticalCaptureCheckpoint {
        self.checkpoint
    }

    pub(crate) fn profile(&self) -> &TacticalCaptureProfile {
        &self.profile.value
    }

    pub(crate) fn sealed_profile(&self) -> &SealedJsonFile<TacticalCaptureProfile> {
        &self.profile
    }

    pub(crate) fn sealed_contract(&self) -> &SealedJsonFile<TacticalCaptureContract> {
        &self.contract
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub fn width(&self) -> u32 {
        self.profile.value.capture.output_width
    }

    pub fn height(&self) -> u32 {
        self.profile.value.capture.output_height
    }

    pub(crate) fn launch_session(&self) -> SkirmishLaunchSession {
        self.profile.value.launch_session()
    }

    pub fn validate_runtime_environment(&self) -> Result<()> {
        self.contract.value.validate_environment()
    }
}

/// Parse the neutral application launch boundary.
///
/// Native's global switch table runs over every argument before launch
/// dispatch, so it is consumed here first: without that, a player typing the
/// one switch the community actually uses (`-WIN`) gets no window at all. Only
/// the arguments native does not recognise reach the mode parsers, so the
/// sealed capture contract — a misspelled automation flag must never open an
/// interactive window — is untouched.
///
/// Tactical mode is then recognized only when its flag is the first remaining
/// argument. Every other vector, including an empty vector and malformed or
/// non-UTF-8 shell vectors, is passed unchanged to the existing shell parser.
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
    // `AssetManager` reads the original process argv for `-CD` itself, using
    // native's substring match; consuming the switch with the same predicate
    // here keeps the two matchers from disagreeing about what `-CD` means.
    let (retail_options, args) = consume_retail_switches(args);
    if retail_options.usage_requested {
        return Ok(AppLaunchMode::Usage);
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
        first == OsString::from(TACTICAL_CAPTURE_FLAG),
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
    let profile = TacticalCaptureProfile::load_strict(&profile_path)?;
    ensure!(
        profile.value.checkpoint == checkpoint.as_str(),
        "profile checkpoint {:?} differs from requested checkpoint {:?}",
        profile.value.checkpoint,
        checkpoint.as_str()
    );
    let contract = TacticalCaptureContract::load_external(&contract_path)?;
    ensure!(
        profile.value.budgets.absolute_timeout_max_seconds
            == contract.value.absolute_max_child_timeout_seconds,
        "profile and contract absolute timeout maxima differ"
    );
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
            root.join("tools/tactical_certification/profiles/soviet-radar-online-v2.json")
                .into_os_string(),
            "--contract".into(),
            root.join("src/app/diagnostics/tactical_capture/contract.v2.json")
                .into_os_string(),
            "--output".into(),
            output.as_os_str().to_owned(),
        ]
    }

    #[test]
    fn no_args_still_delegate_to_interactive_shell_launch() {
        assert!(matches!(
            parse_launch_args(Vec::<OsString>::new()).expect("parse"),
            AppLaunchMode::Interactive(_)
        ));
    }

    #[test]
    fn retail_cd_switch_is_a_global_interactive_launch_option() {
        assert!(matches!(
            parse_launch_args([OsString::from("-cD")]).expect("parse"),
            AppLaunchMode::Interactive(_)
        ));
    }

    #[test]
    fn a_cd_superstring_launches_instead_of_aborting() {
        // The asset layer already selects the wildcard media branch for any
        // argument *containing* `-CD`, so the launch parser must accept the
        // same set or an argument like `-CDROM` refuses to start the game.
        let AppLaunchMode::Interactive(options) =
            parse_launch_args([OsString::from("-CDROM")]).expect("parse")
        else {
            panic!("a -CD superstring must stay an interactive launch");
        };
        assert!(options.cd_media);
    }

    #[test]
    fn the_windowed_switch_launches_instead_of_aborting() {
        let AppLaunchMode::Interactive(options) =
            parse_launch_args([OsString::from("-win")]).expect("parse")
        else {
            panic!("-WIN must stay an interactive launch");
        };
        assert!(options.windowed);
    }

    #[test]
    fn help_switches_route_to_usage_without_a_window() {
        for form in ["/?", "-?", "-h", "/h"] {
            assert!(
                matches!(
                    parse_launch_args([OsString::from(form)]).expect("parse"),
                    AppLaunchMode::Usage
                ),
                "{form} must print usage instead of launching"
            );
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
        std::fs::remove_dir(parent).expect("remove test directory");
    }

    #[test]
    fn non_tactical_unknown_args_retain_shell_error() {
        let args = [OsString::from("--not-a-real-option")];
        let shell_error = shell_capture::parse_launch_args(args.clone())
            .expect_err("sealed shell parser must reject");
        let error = parse_launch_args(args).expect_err("sealed shell parser must reject");
        assert_eq!(error.to_string(), shell_error.to_string());
    }

    #[test]
    fn shell_capture_request_delegates_without_semantic_changes() {
        let parent = test_directory("shell");
        let output = parent.join("capture");
        let args: Vec<OsString> = [
            "--shell-capture",
            "main-menu-0xe2-steady",
            "--width",
            "800",
            "--height",
            "600",
            "--cursor-x",
            "400",
            "--cursor-y",
            "300",
            "--output",
        ]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(output.as_os_str().to_owned()))
        .collect();
        let ShellAppLaunchMode::ShellCapture(expected) =
            shell_capture::parse_launch_args(args.clone()).expect("sealed parse")
        else {
            panic!("wrong sealed launch mode");
        };
        let AppLaunchMode::ShellCapture(actual) = parse_launch_args(args).expect("neutral parse")
        else {
            panic!("wrong neutral launch mode");
        };
        assert_eq!(actual.checkpoint(), expected.checkpoint());
        assert_eq!(
            (actual.width(), actual.height()),
            (expected.width(), expected.height())
        );
        assert_eq!(
            (actual.cursor_x(), actual.cursor_y()),
            (expected.cursor_x(), expected.cursor_y())
        );
        assert_eq!(actual.output_dir(), expected.output_dir());
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
        assert_eq!(request.profile().profile_id, "soviet-radar-online-v2");
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
