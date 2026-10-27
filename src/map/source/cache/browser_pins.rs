//! Active-source pins for the browser's per-tab memory filesystem.
//!
//! Cache operations never yield; there is no second process sharing these
//! bytes. Weak entries are derived from the exact leases retained by sources,
//! not a separate cache or an independent file-lifetime decision.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[derive(Default)]
struct Pins(BTreeMap<PathBuf, Weak<PathBuf>>);

impl Pins {
    fn pin(&mut self, path: &Path) -> Arc<PathBuf> {
        self.0.retain(|_, lease| lease.strong_count() != 0);
        if let Some(lease) = self.0.get(path).and_then(Weak::upgrade) {
            return lease;
        }
        let lease = Arc::new(path.to_owned());
        self.0.insert(path.to_owned(), Arc::downgrade(&lease));
        lease
    }

    fn is_pinned(&self, path: &Path) -> bool {
        self.0
            .get(path)
            .is_some_and(|lease| lease.strong_count() != 0)
    }
}

fn pins() -> &'static Mutex<Pins> {
    static PINS: OnceLock<Mutex<Pins>> = OnceLock::new();
    PINS.get_or_init(Default::default)
}

pub(super) fn pin(path: &Path) -> Arc<PathBuf> {
    pins().lock().expect("browser cache pins").pin(path)
}

pub(super) fn is_pinned(path: &Path) -> bool {
    pins().lock().expect("browser cache pins").is_pinned(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_clones_and_another_reader_pin_until_the_final_lease_drops() {
        let path = Path::new("/browser-pin-test/active.map");
        let source = pin(path);
        let upload = Arc::clone(&source);
        let another_reader = pin(path);
        assert!(Arc::ptr_eq(&source, &another_reader));
        drop(source);
        drop(upload);
        assert!(is_pinned(path));
        drop(another_reader);
        assert!(!is_pinned(path));
    }

    #[test]
    fn unrelated_cache_paths_and_retired_entries_do_not_keep_a_lease_alive() {
        let mut registry = Pins::default();
        let a = Path::new("/browser-pin-a/shared.map");
        let b = Path::new("/browser-pin-b/shared.map");
        let first = registry.pin(a);
        let second = registry.pin(b);
        drop(first);
        assert!(!registry.is_pinned(a));
        assert!(registry.is_pinned(b));
        let third = registry.pin(Path::new("/browser-pin-c/another.map"));
        assert!(!registry.0.contains_key(a));
        drop((second, third));
        assert!(!registry.is_pinned(b));
    }
}
