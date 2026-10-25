//! VERA repository write transactions; these are filesystem regressions, not
//! comparisons with the native SAV format.

use super::*;

fn fixture(label: &str) -> SaveRepository {
    SaveRepository::at(std::env::temp_dir().join(format!(
        "vera20k-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    )))
}

fn snapshot(description: &str, timestamp: u64) -> Vec<u8> {
    let mut simulation = Simulation::new();
    simulation
        .session_mut_for_test()
        .set_map_name_for_test("OVERWRITE.MAP".into());
    GameSnapshot::save_validated(&simulation, 1, 2, description, timestamp)
}

#[test]
fn explicit_overwrite_replaces_selected_save_and_refreshes_exact_description() {
    let repository = fixture("explicit-overwrite");
    let original = snapshot("original", 1);
    let path = repository.write_named("selected.bin", &original).unwrap();
    let other = repository.write_named("other.bin", &original).unwrap();
    let exact_description = "  保存 / ridge  ";
    let replacement = snapshot(exact_description, 2);

    repository.overwrite_existing(&path, &replacement).unwrap();
    assert_eq!(repository.read(&path).unwrap(), replacement);
    assert_eq!(repository.read(&other).unwrap(), original);
    let entries = repository.browser_entries();
    assert_eq!(entries.len(), 2);
    let (selected, _) = entries
        .iter()
        .find(|(entry, _)| entry.path == path)
        .unwrap();
    assert_eq!(selected.header.description, exact_description);
    assert_eq!(
        std::fs::read_dir(repository.directory()).unwrap().count(),
        2
    );
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[test]
fn creation_collision_and_missing_overwrite_leave_previous_save() {
    let repository = fixture("create-collision");
    let original = snapshot("original", 1);
    let path = repository
        .write_named("same-timestamp.bin", &original)
        .unwrap();
    let replacement = snapshot("replacement", 2);

    let collision = repository
        .write_named("same-timestamp.bin", &replacement)
        .unwrap_err();
    assert_eq!(collision.source.kind(), std::io::ErrorKind::AlreadyExists);
    let missing = repository.directory().join("removed-row.bin");
    assert!(
        repository
            .overwrite_existing(&missing, &replacement)
            .is_err()
    );
    assert!(!missing.exists());
    assert_eq!(repository.read(&path).unwrap(), original);
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[cfg(windows)]
#[test]
fn failed_overwrite_publish_preserves_original_and_removes_staged_file() {
    use std::os::windows::fs::OpenOptionsExt;

    let repository = fixture("overwrite-publish-failure");
    let original = snapshot("original", 1);
    let path = repository.write_named("selected.bin", &original).unwrap();
    // Permit the repository's existing-header read but deny replacement. This
    // fails the real final rename after the full staged snapshot is written.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(
        repository
            .overwrite_existing(&path, &snapshot("replacement", 2))
            .is_err()
    );
    assert_eq!(repository.read(&path).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(repository.directory()).unwrap().count(),
        1
    );
    drop(held);
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[test]
fn initial_campaign_slot_is_visible_and_replaced_without_touching_user_saves() {
    let repository = fixture("initial-campaign");
    let user = repository
        .write_named("user.bin", &snapshot("user", 1))
        .unwrap();
    let initial = repository
        .write_initial_campaign("ALL01UMD.sav", &snapshot("[Allied 01 - Time Lapse]", 2))
        .unwrap();
    assert_eq!(repository.browser_entries().len(), 2);
    repository
        .write_initial_campaign("ALL01UMD.sav", &snapshot("[new attempt]", 3))
        .unwrap();
    assert_eq!(repository.browser_entries().len(), 2);
    assert_eq!(
        GameSnapshot::read_header(&repository.read(&initial).unwrap())
            .unwrap()
            .description,
        "[new attempt]"
    );
    assert_eq!(
        GameSnapshot::read_header(&repository.read(&user).unwrap())
            .unwrap()
            .description,
        "user"
    );
    repository
        .overwrite_existing(&initial, &snapshot("user renamed initial save", 4))
        .unwrap();
    assert_eq!(
        GameSnapshot::read_header(&repository.read(&initial).unwrap())
            .unwrap()
            .description,
        "user renamed initial save"
    );
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[test]
fn initial_campaign_attempt_is_process_owned_and_rearmed_only_by_its_callers() {
    let mut state = PersistenceState::new(Default::default());
    assert!(!state.take_initial_campaign_save(false));
    assert!(state.take_initial_campaign_save(true));
    // A failed IO attempt and a same-mission restart do not clear the latch.
    assert!(!state.take_initial_campaign_save(true));
    state.arm_initial_campaign_save();
    assert!(state.take_initial_campaign_save(true));
    state.arm_initial_campaign_save();
    state.loaded_campaign();
    assert!(!state.take_initial_campaign_save(true));
}

#[test]
fn initial_campaign_attempt_admission_matches_original_main_tick() {
    let native = crate::test_fixture::json("tools/input_oracle/campaign_mission_titles.json");
    for row in native["initial_save_rows"].as_array().unwrap() {
        let mut state = PersistenceState::new(Default::default());
        state.initial_campaign_save_attempted = row["incoming_latch"].as_u64().unwrap() != 0;
        let admitted = state.take_initial_campaign_save(row["game_mode"].as_u64().unwrap() == 0);
        assert_eq!(admitted, row["filename"].is_string(), "{}", row["name"]);
        assert_eq!(
            u64::from(state.initial_campaign_save_attempted),
            row["outgoing_latch"].as_u64().unwrap(),
            "{}",
            row["name"]
        );
    }
}

#[test]
fn initial_campaign_paths_preserve_parent_and_fail_without_creating_it() {
    let repository = fixture("initial-campaign-paths");
    let bytes = snapshot("[initial]", 1);
    assert!(
        repository
            .write_initial_campaign("missing/ALL01UMD.sav", &bytes)
            .is_err()
    );
    assert!(!repository.directory().join("missing").exists());
    std::fs::create_dir(repository.directory().join("missions")).unwrap();
    let relative = repository
        .write_initial_campaign("missions/ALL01UMD.sav", &bytes)
        .unwrap();
    assert_eq!(
        relative,
        repository.directory().join("missions/ALL01UMD.sav")
    );
    let absolute = repository.directory().join("absolute.sav");
    std::fs::write(&absolute, snapshot("older automatic slot", 0)).unwrap();
    assert_eq!(
        repository
            .write_initial_campaign(absolute.to_str().unwrap(), &bytes)
            .unwrap(),
        absolute
    );
    assert_eq!(repository.read(&absolute).unwrap(), bytes);
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[test]
fn initial_campaign_preserves_foreign_saved_game_bytes() {
    let repository = fixture("foreign-initial-save");
    std::fs::create_dir_all(repository.directory()).unwrap();
    let path = repository.directory().join("ALL01UMD.sav");
    let foreign = b"original native or other foreign save";
    std::fs::write(&path, foreign).unwrap();
    assert!(
        repository
            .write_initial_campaign("ALL01UMD.sav", &snapshot("new", 1))
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), foreign);
    std::fs::remove_dir_all(repository.directory()).unwrap();
}
