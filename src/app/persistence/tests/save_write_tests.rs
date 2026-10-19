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
    simulation.session.map_name = "OVERWRITE.MAP".into();
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
