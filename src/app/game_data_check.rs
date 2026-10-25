//! Read-only installer check using the same archive and resource readers as play.
//! This runs before logging, a window, audio output or the single-instance gate.

use std::path::Path;

use anyhow::{Context, Result};

use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::assets::csf_file::CsfFile;
use crate::rules::retail_sources::select_ini;

#[derive(Debug, Default)]
struct InstallReport {
    required: Vec<String>,
    optional: Vec<String>,
    /// Language from the same successfully parsed string table used by play.
    language: Option<u32>,
}

fn inspect(game_dir: &Path) -> InstallReport {
    let mut report = InstallReport::default();
    let (mut assets, issues) =
        match AssetManager::inspect_install(game_dir, MediaArchiveMode::STOCK_DIGITAL) {
            Ok(result) => result,
            Err(error) => {
                report
                    .required
                    .push(format!("{}: {error}", game_dir.display()));
                return report;
            }
        };
    for issue in issues {
        let destination = if issue.required {
            &mut report.required
        } else {
            &mut report.optional
        };
        destination.push(format!("{}: {}", issue.name, issue.detail));
    }

    match CsfFile::load_active(|name| assets.get_ref(name)) {
        Ok((csf, _)) => report.language = Some(csf.language),
        Err(error) => report.required.push(format!("ra2md.csf: {error}")),
    }
    // Read the startup roots through the production selector, including loose
    // overrides and nested archives. Optional LANGRULE is not a prerequisite.
    let roots = assets.ini_roots();
    for name in [roots.rules(), roots.art(), roots.ai()] {
        if let Err(error) = select_ini(&assets, name) {
            report.required.push(format!("{name}: {error}"));
        }
    }
    // The application admits neutral shell archives before it builds the menu.
    // Their absence is harmless when the mandatory buttons resolve elsewhere.
    if let Err(error) = assets.register_neutral_archives() {
        report
            .optional
            .push(format!("neutral shell archives: {error}"));
    }
    report
        .required
        .extend(crate::render::main_menu_shell_chrome::inspect_main_menu_button_art(&assets));
    report
        .optional
        .extend(crate::audio::theme::missing_install_music(&assets));
    // Sound effects are optional at runtime too: the game logs a missing or
    // malformed index and continues without its sound bank.
    match assets.load_audio_index() {
        Ok(Some(_)) => {}
        Ok(None) => report
            .optional
            .push("audio.idx / audio.bag: sound bank unavailable".into()),
        Err(error) => report
            .optional
            .push(format!("audio.idx / audio.bag: {error}")),
    }
    report
}

/// Write UTF-8 issue lists and the decimal CSF language ID consumed by Inno
/// Setup (no JSON or MIX parser there). A missing or invalid string table leaves
/// the language file empty.
/// Exit 0 allows installation even with optional warnings; exit 1 blocks it.
/// Failure to write a report is returned separately, so it cannot look valid.
pub(super) fn check_and_write_report(game_dir: &Path, output_dir: &Path) -> Result<i32> {
    let report = inspect(game_dir);
    std::fs::create_dir_all(output_dir).context("create game-data report directory")?;
    for (name, issues) in [
        ("required.txt", &report.required),
        ("optional.txt", &report.optional),
    ] {
        std::fs::write(output_dir.join(name), issues.join("\r\n"))
            .with_context(|| format!("write {name}"))?;
    }
    let language = report.language.map(|id| id.to_string()).unwrap_or_default();
    std::fs::write(output_dir.join("language.txt"), language).context("write language.txt")?;
    Ok(i32::from(!report.required.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::asset_manager::test_support::TestDirectory;

    fn fixture() -> TestDirectory {
        let dir = TestDirectory::new("installer-report-尤里");
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/release/windows/test-game");
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() != "README.md" {
                std::fs::copy(entry.path(), dir.path().join(entry.file_name())).unwrap();
            }
        }
        dir
    }

    #[test]
    fn packed_required_data_allows_install_and_lists_missing_music_only_as_optional() {
        let dir = fixture();
        // The gameplay playlist requires WAV availability even though the
        // low-level playback decoder can also read an AUD file.
        std::fs::write(dir.path().join("test-song.aud"), b"present").unwrap();
        let report_dir = TestDirectory::new("installer-output");
        let result = crate::app::frontend::launch::try_game_data_check([
            "--check-game-data".into(),
            dir.path().as_os_str().to_owned(),
            report_dir.path().as_os_str().to_owned(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(result, 0);
        let required = std::fs::read_to_string(report_dir.path().join("required.txt")).unwrap();
        let optional = std::fs::read_to_string(report_dir.path().join("optional.txt")).unwrap();
        assert!(required.is_empty(), "{required}");
        assert!(optional.contains("test-song.wav"), "{optional}");
        assert!(optional.contains("audio.idx / audio.bag"), "{optional}");
        assert_eq!(
            std::fs::read_to_string(report_dir.path().join("language.txt")).unwrap(),
            "0"
        );
    }

    #[test]
    fn all_required_failures_are_reported_and_retry_replaces_old_results() {
        let dir = fixture();
        let output = TestDirectory::new("installer-retry");
        std::fs::write(output.path().join("language.txt"), "9").unwrap();
        std::fs::remove_file(dir.path().join("ra2md.mix")).unwrap();
        std::fs::remove_file(dir.path().join("ra2.mix")).unwrap();
        assert_eq!(
            check_and_write_report(dir.path(), output.path()).unwrap(),
            1
        );
        let report = std::fs::read_to_string(output.path().join("required.txt")).unwrap();
        for name in [
            "ra2md.mix",
            "ra2.mix",
            "ra2md.csf",
            "rulesmd.ini",
            "artmd.ini",
            "aimd.ini",
        ] {
            assert!(report.contains(name), "missing {name} in {report}");
        }
        assert!(
            std::fs::read(output.path().join("language.txt"))
                .unwrap()
                .is_empty()
        );
        let valid = fixture();
        std::fs::write(
            valid.path().join("RA2MD.CSF"),
            include_bytes!("../../.github/release/windows/test-language-german.csf"),
        )
        .unwrap();
        assert_eq!(
            check_and_write_report(valid.path(), output.path()).unwrap(),
            0
        );
        assert!(
            std::fs::read(output.path().join("required.txt"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read_to_string(output.path().join("language.txt")).unwrap(),
            "2"
        );
        assert!(
            crate::app::frontend::launch::try_game_data_check([
                "--check-game-data".into(),
                dir.path().as_os_str().to_owned()
            ])
            .unwrap()
            .is_err()
        );
    }

    #[test]
    fn corrupt_localization_is_required_but_corrupt_music_is_optional() {
        let dir = fixture();
        std::fs::write(dir.path().join("ra2md.csf"), b"bad CSF").unwrap();
        std::fs::write(dir.path().join("thememd.mix"), b"bad MIX").unwrap();
        std::fs::remove_file(dir.path().join("SHELL2.PAL")).unwrap();
        std::fs::write(dir.path().join("SDBTNANM.SHP"), b"bad SHP").unwrap();
        let report = inspect(dir.path());
        assert_eq!(report.language, None);
        assert!(
            report
                .required
                .iter()
                .any(|issue| issue.contains("ra2md.csf") && issue.contains("CSF"))
        );
        for name in ["SDBTNANM.PAL", "SHELL2.PAL", "SDBTNANM.SHP"] {
            assert!(
                report.required.iter().any(|issue| issue.contains(name)),
                "{report:?}"
            );
        }
        assert!(
            report
                .optional
                .iter()
                .any(|issue| issue.contains("thememd.mix"))
        );
        assert!(
            !report
                .required
                .iter()
                .any(|issue| issue.contains("thememd.mix"))
        );
    }
}
