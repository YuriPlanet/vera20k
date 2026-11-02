//! Process switches that only remove debug assertions. Kept in `util` so
//! every layer's derived caches can read them.

/// Whether debug builds compare each read of a kept product (the movement
/// pass's derived inputs, the kept Ground sort keys, the pointer-expiry
/// listeners, a path batch's entry facts, the sidebar's derived inputs, the
/// rules' projected house masks) with a fresh derivation.
/// Never in release. On in debug unless `VERA20K_SKIP_LIVE_READ_CHECK` is set,
/// which scale instruments use to time without the rebuilds the checks cost.
/// It only removes assertions, so it cannot change a result.
pub fn live_read_check_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(debug_assertions)
        && *ENABLED.get_or_init(|| std::env::var_os("VERA20K_SKIP_LIVE_READ_CHECK").is_none())
}
