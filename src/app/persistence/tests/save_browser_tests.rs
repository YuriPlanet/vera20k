use super::*;
use crate::util::save_codec;

#[test]
fn browser_admission_rejects_other_build_capability() {
    let directory = crate::map::source::test_support::TestDirectory::new("save-browser-capability");
    let repository = SaveRepository::at(directory.path());
    let bytes = GameSnapshot::save_validated(&Simulation::new(), 0, 0, "other build", 0);
    let header = GameSnapshot::read_header(&bytes).unwrap();
    // Any other capability combination must be refused; flip every bit so this
    // stays a foreign mode as capabilities are added.
    let other_capability = crate::rules::compatibility::RulesCompatibility::from_capabilities(
        !crate::rules::ARES_ENABLED,
        !crate::rules::PHOBOS_ENABLED,
        !crate::rules::NPATCH_ENABLED,
        !crate::rules::VERAX_ENABLED,
    );
    let mut header = serde_json::to_value(header).unwrap();
    header["compatibility"] = serde_json::to_value(other_capability).unwrap();
    let header: GameSnapshotHeader = serde_json::from_value(header).unwrap();
    repository
        .write_named("other-build.bin", &save_codec::serialize(&header).unwrap())
        .unwrap();
    let entries = repository.browser_entries();
    assert_eq!(
        entries.len(),
        1,
        "keep readable metadata in browser ordering"
    );
    assert!(!entries[0].0.header.is_compatible());
    assert!(!repository.has_browser_entry());
}

#[test]
fn browser_admission_keeps_empty_descriptions_and_uses_file_time() {
    let directory = std::env::temp_dir().join(format!(
        "vera-browser-metadata-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = SaveRepository::at(&directory);
    let mut sim = Simulation::new();
    sim.session_mut_for_test()
        .set_map_name_for_test("BROWSER.MAP".into());
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let bytes = GameSnapshot::save_validated(&sim, 1, 2, "", 3);
    let path = repository.write_named("valid.bin", &bytes).unwrap();
    let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let mut header = GameSnapshot::read_header(&bytes).unwrap();
    header.layout = 0;
    repository
        .write_named("incompatible.bin", &save_codec::serialize(&header).unwrap())
        .unwrap();
    let entries = repository.browser_entries();
    // Native559ED0 reads metadata and records internal-version mismatch;
    // 5596A0 sorts every readable row before5599DF hides incompatible rows.
    // Filtering here changes the raw index used for initial selection.
    assert_eq!(
        entries.len(),
        2,
        "readable incompatible metadata is retained"
    );
    let valid = entries
        .iter()
        .find(|(entry, _)| entry.path == path)
        .unwrap();
    assert!(valid.0.header.description.is_empty());
    assert!(repository.has_browser_entry());
    assert_eq!(
        valid.1,
        crate::map::rmg::saved_seeds::system_time_to_file_time(modified)
    );
    repository.delete(&path).unwrap();
    assert!(!repository.has_browser_entry());
    assert_eq!(repository.browser_entries().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}
