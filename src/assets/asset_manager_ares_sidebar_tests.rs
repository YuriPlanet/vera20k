//! Connected archive/cache lifecycle checks. Native selection and retirement
//! goldens live in tools/rules_oracle/ares_side_sidebar.json; fixture MIX bytes
//! are synthetic and exercise the production mount and CRC-cache owners.

use super::test_support::TestDirectory;
use super::{AssetManager, SIDEBAR_RETIRED_PALETTES, mix_hash};
use std::sync::Arc;

#[test]
fn ares_sidebar_group_replaces_resources_and_retires_only_ui_palette_winners() {
    let dir = TestDirectory::new("ares-sidebar-switch");
    dir.write_mix("sidec03md.mix", "SIDEBAR.PAL", b"third-md");
    dir.write_mix("sidec03.mix", "SIDEBAR.PAL", b"third-base");
    dir.write_mix("sidenc03.mix", "synthetic-nc", b"nc");
    dir.write_mix("sidec04.mix", "SIDEBAR.PAL", b"fourth-base");
    let mut assets = AssetManager::from_loose_root_for_test(dir.path());
    assert!(assets.activate_sidebar_archives(3).unwrap());
    let third = assets.load_file_from_mix("SIDEBAR.PAL").unwrap();
    assert_eq!(&*third.bytes, b"third-md");
    assert_eq!(
        assets.active_sidebar_archives,
        ["sidec03md.mix", "sidec03.mix", "sidenc03.mix"]
    );
    {
        let mut cache = assets.mix_file_cache.lock().unwrap();
        for name in SIDEBAR_RETIRED_PALETTES {
            cache.insert(mix_hash(name), third.clone());
        }
        cache.insert(mix_hash("SyntheticKeep.SHP"), third.clone());
    }
    assert!(assets.activate_sidebar_archives(4).unwrap());
    assert_eq!(assets.active_sidebar_archives, ["sidec04.mix"]);
    let fourth = assets.load_file_from_mix("SIDEBAR.PAL").unwrap();
    assert_eq!(&*fourth.bytes, b"fourth-base");
    assert!(!Arc::ptr_eq(&third.bytes, &fourth.bytes));
    assert!(Arc::ptr_eq(
        &assets
            .load_file_from_mix("SyntheticKeep.SHP")
            .unwrap()
            .bytes,
        &third.bytes
    ));
    let cache = assets.mix_file_cache.lock().unwrap();
    for name in ["YRII.PAL", "OBSERVER.PAL", "UIBKGD.PAL"] {
        assert!(!cache.contains_key(&mix_hash(name)), "{name}");
    }
    // Arc-owned old draw resources remain valid until the outgoing atlas dies.
    assert_eq!(&*third.bytes, b"third-md");
}

#[test]
fn ares_sidebar_failure_preserves_native_partial_mount_and_save_candidate_rollback() {
    let dir = TestDirectory::new("ares-sidebar-failure");
    dir.write_mix("sidec03.mix", "SIDEBAR.PAL", b"third");
    dir.write_mix("sidec05md.mix", "SIDEBAR.PAL", b"partial-md");
    let mut assets = AssetManager::from_loose_root_for_test(dir.path());
    assert!(assets.activate_sidebar_archives(3).unwrap());
    let prior = assets.load_file_from_mix("SIDEBAR.PAL").unwrap();
    assert!(!assets.activate_sidebar_archives(0).unwrap());
    assert_eq!(assets.active_sidebar_archives, ["sidec03.mix"]);
    let rejected: Result<(), ()> = assets.prepare_saved_load(|candidate| {
        assert!(!candidate.activate_sidebar_archives(5).unwrap());
        assert_eq!(candidate.active_sidebar_archives, ["sidec05md.mix"]);
        assert!(Arc::ptr_eq(
            &prior.bytes,
            &candidate.load_file_from_mix("SIDEBAR.PAL").unwrap().bytes
        ));
        Err(())
    });
    assert!(rejected.is_err());
    assert_eq!(assets.active_sidebar_archives, ["sidec03.mix"]);
    assert!(Arc::ptr_eq(
        &prior.bytes,
        &assets.load_file_from_mix("SIDEBAR.PAL").unwrap().bytes
    ));
    // Outside the explicit save transaction, failure is not rolled back.
    assert!(!assets.activate_sidebar_archives(5).unwrap());
    assert_eq!(assets.active_sidebar_archives, ["sidec05md.mix"]);
}

#[test]
fn ares_sidebar_signed_selector_preserves_native_int_min_wraparound() {
    let dir = TestDirectory::new("ares-sidebar-signed");
    let corpus = crate::test_fixture::json("tools/rules_oracle/ares_side_sidebar.json");
    let row = corpus["selector"]["rows"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(
        row["input"]["mix_file_index"].as_i64(),
        Some(i64::from(i32::MIN))
    );
    dir.write_mix(
        row["selection"]["mix_name"].as_str().unwrap(),
        "SIDEBAR.PAL",
        b"wrapped",
    );
    let mut assets = AssetManager::from_loose_root_for_test(dir.path());
    assert!(assets.activate_sidebar_archives(i32::MIN).unwrap());
    assert_eq!(
        &*assets.load_file_from_mix("SIDEBAR.PAL").unwrap().bytes,
        b"wrapped"
    );
    assert!(!assets.activate_sidebar_archives(-1).unwrap());
}
