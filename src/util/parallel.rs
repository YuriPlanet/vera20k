//! Worker counts for CPU-side rasterization.

/// Upper bound for the `sysconf` fallback. Horizon reports the application's
/// core count; anything larger than this is a host oddity, not a target.
const MAX_FALLBACK_WORKERS: usize = 4;

/// Threads one CPU-bound rasterization pass may use.
///
/// `std::thread::available_parallelism` answers `Unsupported` on Horizon, which
/// would silently pin every sprite bake to one core. The Switch runtime answers
/// `sysconf(_SC_NPROCESSORS_ONLN)` with the cores the application owns (3 for a
/// homebrew NSP), so that is the fallback rather than a hard-coded count.
pub fn worker_count() -> usize {
    if let Ok(count) = std::thread::available_parallelism() {
        return count.get();
    }
    #[cfg(unix)]
    {
        // SAFETY: sysconf takes only the name constant and returns a long.
        let count = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        if count > 0 {
            return (count as usize).clamp(1, MAX_FALLBACK_WORKERS);
        }
    }
    1
}

#[cfg(all(test, tests_util))]
mod tests {
    #[test]
    fn worker_count_is_at_least_one_and_bounded() {
        let workers = super::worker_count();
        assert!(workers >= 1);
        assert!(workers <= 1024);
    }
}
