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
fn migration_backs_up_original_before_decoding_the_old_world() {
    use super::save_migration::SaveMigrationError;

    let mut header = GameSnapshot::read_header(&snapshot("old", 1)).unwrap();
    header.layout = 0xFD30_A463;
    if !SaveRepository::can_migrate(&header) {
        return;
    }
    // The header qualifies, but there is no body to convert.
    let original = crate::util::save_codec::serialize(&header).unwrap();
    let repository = fixture("migration-backup");
    let source = repository.write_named("old.sav", &original).unwrap();
    let backups = repository.directory().join("backups");

    std::fs::write(&backups, b"obstruction").unwrap();
    assert!(matches!(
        repository.migrate_to_slot(&source, "new", None),
        Err(SaveMigrationError::Backup(_))
    ));
    std::fs::remove_file(&backups).unwrap();

    assert!(matches!(
        repository.migrate_to_slot(&source, "new", None),
        Err(SaveMigrationError::Convert(_))
    ));
    let mut copies = std::fs::read_dir(&backups).unwrap();
    let copy = copies.next().unwrap().unwrap().path();
    assert_eq!(std::fs::read(copy).unwrap(), original);
    assert!(copies.next().is_none());
    assert_eq!(repository.read(&source).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(repository.directory()).unwrap().count(),
        2
    );
    assert_eq!(repository.browser_entries().len(), 1);
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[test]
#[cfg(not(any(feature = "phobos", feature = "verax", feature = "npbug")))]
fn conversion_overwrites_only_the_selected_slot_and_keeps_its_path() {
    use super::save_migration::SaveMigrationError;

    let repository = fixture("migration-overwrite");
    let original = crate::test_fixture::bytes("tools/save_compatibility/fixtures/v0.3.9.sav");
    let source = repository.write_named("old.sav", original).unwrap();
    let alias = repository.directory().join("old-link.bin");
    std::fs::hard_link(&source, &alias).unwrap();
    let other = repository.write_named("other.sav", original).unwrap();
    let target = repository
        .write_named("selected.bin", &snapshot("current", 7))
        .unwrap();
    let description = " 战役 / 续玩? ";
    for path in [&source, &alias] {
        assert!(matches!(
            repository.migrate_to_slot(&source, description, Some(path)),
            Err(SaveMigrationError::SourceTarget)
        ));
    }
    assert!(matches!(
        repository.migrate_to_slot(&source, "  ", Some(&target)),
        Err(SaveMigrationError::InvalidName)
    ));
    assert!(!repository.directory().join("backups").exists());
    // The selected path no longer holds the current save displayed by the dialog.
    for changed in [original, b"corrupt" as &[u8]] {
        std::fs::write(&target, changed).unwrap();
        let error = repository
            .migrate_to_slot(&source, description, Some(&target))
            .unwrap_err();
        assert_eq!(error.message_key(), "VERA:SaveConversionSlotHelp");
        assert!(repository.read(&target).unwrap() == changed);
    }
    std::fs::write(&target, snapshot("current", 7)).unwrap();
    let written = repository
        .migrate_to_slot(&source, description, Some(&target))
        .unwrap();
    assert_eq!(written, target);
    assert_eq!(
        GameSnapshot::read_header(&repository.read(&written).unwrap())
            .unwrap()
            .description,
        description
    );
    for path in [&source, &alias, &other] {
        assert!(repository.read(path).unwrap() == original);
    }
    let backup = std::fs::read_dir(repository.directory().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_file())
        .unwrap();
    assert!(repository.read(&backup).unwrap() == original);
    // New has no overwrite target even when another slot shows the same description.
    let new_slot = repository
        .migrate_to_slot(&source, description, None)
        .unwrap();
    assert_ne!(new_slot, written);
    assert_eq!(
        repository
            .browser_entries()
            .iter()
            .filter(|(entry, _)| entry.header.description == description)
            .count(),
        2
    );
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[test]
#[cfg(not(any(feature = "phobos", feature = "verax", feature = "npbug")))]
fn conversion_preserves_the_replaced_slot_and_takes_the_current_rules() {
    use super::save_migration::SaveMigrationError;
    use crate::sim::snapshot::migration;

    let released = crate::test_fixture::bytes("tools/save_compatibility/fixtures/v0.3.9.sav");
    let target_bytes = migration::migrate_named(released.to_vec(), "Known good").unwrap();
    let mut header = GameSnapshot::read_metadata(released).unwrap();
    let expected_rules = header.rules_hash;
    header.rules_hash ^= 1;
    let prefix = crate::util::save_codec::serialize(&header).unwrap();
    let mut wrong_rules = released.to_vec();
    wrong_rules[..prefix.len()].copy_from_slice(&prefix);

    let repository = fixture("conversion-replaced-slot");
    let source = repository.write_named("old.sav", &wrong_rules).unwrap();
    let target = repository.write_named("chosen.bin", &target_bytes).unwrap();
    let backups = repository.directory().join("backups");
    let replaced = backups.join("replaced");
    std::fs::create_dir(&backups).unwrap();
    std::fs::write(&replaced, b"obstruct destination backup").unwrap();
    assert!(matches!(
        repository.migrate_to_slot(&source, "Converted", Some(&target)),
        Err(SaveMigrationError::Backup(_))
    ));
    assert!(repository.read(&target).unwrap() == target_bytes);
    std::fs::remove_file(&replaced).unwrap();

    let written = repository
        .migrate_to_slot(&source, "Converted", Some(&target))
        .unwrap();
    assert_eq!(written, target);
    let converted = repository.read(&target).unwrap();
    // Whatever digest the old writer recorded, the converted save defers to
    // the rules the game has when it loads.
    assert_ne!(header.rules_hash, expected_rules);
    assert_eq!(
        GameSnapshot::read_metadata(&converted).unwrap().rules_hash,
        migration::CONVERTED_RULES_HASH
    );
    let attempt = std::fs::read_dir(&replaced)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert!(repository.read(&attempt.path().join("chosen.bin")).unwrap() == target_bytes);
    assert!(
        repository
            .read(&backups.join(attempt.file_name()).with_extension("sav"))
            .unwrap()
            == wrong_rules
    );
    assert!(repository.read(&source).unwrap() == wrong_rules);
    std::fs::remove_dir_all(repository.directory()).unwrap();
}

#[test]
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn conversion_keeps_a_concurrent_writer_and_the_displaced_slot() {
    use crate::util::file_system;
    let repository = fixture("concurrent-conversion");
    let previous = snapshot("Previous", 1);
    let competing = snapshot("Another writer", 2);
    let target = repository.write_named("chosen.bin", &previous).unwrap();
    let replaced = repository.directory().join("backups/replaced");
    let result =
        file_system::write_staged(repository.directory(), &snapshot("Converted", 3), |stage| {
            SaveRepository::replace_converted_slot(&target, &replaced, "this-attempt", || {
                // This writer acquires the old slot after the conversion has moved it.
                file_system::write_new(&target, &competing)?;
                file_system::persist_noclobber(stage, &target)
            })
        });
    assert!(matches!(
        result,
        Err(super::save_migration::SaveMigrationError::NameExists)
    ));
    assert!(repository.read(&target).unwrap() == competing);
    assert!(
        repository
            .read(&replaced.join("this-attempt/chosen.bin"))
            .unwrap()
            == previous
    );
    assert_eq!(
        std::fs::read_dir(repository.directory()).unwrap().count(),
        2
    );
    std::fs::remove_dir_all(repository.directory()).unwrap();
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
        .write_player_slot(None, Some("ridge"), 42, 7, &original)
        .unwrap();
    assert_eq!(path, repository.directory().join("save_ridge_tick42_7.bin"));
    let replacement = snapshot("replacement", 2);

    let collision = repository
        .write_player_slot(None, Some("ridge"), 42, 7, &replacement)
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
fn initial_campaign_replaces_its_hidden_previous_layout_slot() {
    let repository = fixture("initial-campaign-old-layout");
    let mut old_header = GameSnapshot::read_header(&snapshot("old first mission", 1)).unwrap();
    old_header.layout ^= 1;
    let old_bytes = crate::util::save_codec::serialize(&old_header).unwrap();
    assert!(GameSnapshot::read_header(&old_bytes).is_err());
    let first = repository.write_named("ALL01UMD.sav", &old_bytes).unwrap();
    let manual_bytes = snapshot("manual", 2);
    let manual = repository.write_named("user.bin", &manual_bytes).unwrap();
    assert!(
        !repository
            .browser_entries()
            .iter()
            .find(|(entry, _)| entry.path == first)
            .unwrap()
            .0
            .header
            .is_compatible()
    );

    let replacement = snapshot("[Allied 01 - Time Lapse]", 3);
    repository
        .write_initial_campaign("ALL01UMD.sav", &replacement)
        .unwrap();
    assert_eq!(repository.read(&first).unwrap(), replacement);
    assert!(
        repository
            .browser_entries()
            .iter()
            .find(|(entry, _)| entry.path == first)
            .unwrap()
            .0
            .header
            .is_compatible()
    );
    assert_eq!(repository.read(&manual).unwrap(), manual_bytes);
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
    // VERA: a mission name that would leave the save folder writes nothing,
    // where native 67CF13 creates the file wherever the name points.
    let outside = repository
        .directory()
        .with_file_name(format!("outside-{}.sav", std::process::id()));
    let sibling = format!("../{}", outside.file_name().unwrap().to_str().unwrap());
    for escaping in [sibling.as_str(), outside.to_str().unwrap()] {
        let refused = repository.write_initial_campaign(escaping, &bytes).is_err();
        let written = std::fs::remove_file(&outside).is_ok();
        assert!(refused && !written, "{escaping}");
    }
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
