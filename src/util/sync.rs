//! Locks and once-cells that a tick and the draw can both reach: the Main
//! RNG, and caches inside state the presentation frame shares with the live
//! world. In the threaded browser build a tick runs on a Web Worker beside
//! the draw (docs/sim-thread.md, "The browser"), and the browser's main
//! thread traps when it waits on another thread. There the mutex spins on the
//! main thread (`wasm_sync`) and the once-cell never waits
//! ([`NeverWaitingOnceLock`]); everywhere else they are the standard
//! library's.

#[cfg(all(
    feature = "wasm-threads",
    target_arch = "wasm32",
    target_os = "unknown"
))]
pub use NeverWaitingOnceLock as OnceLock;
#[cfg(all(
    feature = "wasm-threads",
    target_arch = "wasm32",
    target_os = "unknown"
))]
pub use wasm_sync::Mutex;

#[cfg(not(all(
    feature = "wasm-threads",
    target_arch = "wasm32",
    target_os = "unknown"
)))]
pub use std::sync::{Mutex, OnceLock};

use std::marker::PhantomData;
use std::sync::atomic::{AtomicPtr, Ordering};

/// A once-cell whose [`Self::get_or_init`] never waits for another thread:
/// on an empty cell it runs its closure, and if another thread filled the
/// cell meanwhile, keeps that value and drops its own. Two threads may thus
/// both run the closure; every cell here holds a value that depends only on
/// state they share, so either value is the same. (`wasm_sync`'s once-cell
/// reads the standard library's private state and spins forever on the
/// nightly the threaded build uses.)
pub struct NeverWaitingOnceLock<T> {
    value: AtomicPtr<T>,
    _owns: PhantomData<Box<T>>,
}

// SAFETY: the cell owns its value as a `Box<T>` would; shared access hands
// out `&T` and fills the cell with a `T` moved from the calling thread.
unsafe impl<T: Send + Sync> Sync for NeverWaitingOnceLock<T> {}
unsafe impl<T: Send> Send for NeverWaitingOnceLock<T> {}

impl<T> NeverWaitingOnceLock<T> {
    pub const fn new() -> Self {
        Self {
            value: AtomicPtr::new(std::ptr::null_mut()),
            _owns: PhantomData,
        }
    }

    pub fn get(&self) -> Option<&T> {
        // SAFETY: a non-null pointer came from `Box::into_raw` in `set`, and
        // the value lives until `take` or drop, which need `&mut self`.
        unsafe { self.value.load(Ordering::Acquire).as_ref() }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        // SAFETY: as in `get`, with exclusive access.
        unsafe { self.value.get_mut().as_mut() }
    }

    /// Fill an empty cell; a full one gives `value` back.
    pub fn set(&self, value: T) -> Result<(), T> {
        let new = Box::into_raw(Box::new(value));
        match self.value.compare_exchange(
            std::ptr::null_mut(),
            new,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            // SAFETY: `new` was never published.
            Err(_) => Err(*unsafe { Box::from_raw(new) }),
        }
    }

    pub fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
        if let Some(value) = self.get() {
            return value;
        }
        let _ = self.set(init());
        self.get().expect("the cell was just filled")
    }

    pub fn take(&mut self) -> Option<T> {
        let value = std::mem::replace(self.value.get_mut(), std::ptr::null_mut());
        // SAFETY: as in `get`; the cell no longer points at it.
        (!value.is_null()).then(|| *unsafe { Box::from_raw(value) })
    }
}

impl<T> Drop for NeverWaitingOnceLock<T> {
    fn drop(&mut self) {
        self.take();
    }
}

impl<T> Default for NeverWaitingOnceLock<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for NeverWaitingOnceLock<T> {
    fn clone(&self) -> Self {
        let copy = Self::new();
        if let Some(value) = self.get() {
            let _ = copy.set(value.clone());
        }
        copy
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for NeverWaitingOnceLock<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OnceLock").field(&self.get()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::NeverWaitingOnceLock;

    #[test]
    fn racing_fills_keep_one_value_and_drop_the_other() {
        let cell = NeverWaitingOnceLock::new();
        let values: Vec<std::sync::Arc<u32>> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| cell.get_or_init(|| std::sync::Arc::new(7)).clone()))
                .collect();
            threads.into_iter().map(|t| t.join().unwrap()).collect()
        });
        // Every caller got the one value kept; the losers' copies are gone.
        let kept = cell.get().unwrap();
        assert!(
            values
                .iter()
                .all(|value| std::sync::Arc::ptr_eq(value, kept))
        );
        assert_eq!(std::sync::Arc::strong_count(kept), 1 + values.len());
        let mut cell = cell;
        assert_eq!(*cell.take().unwrap(), 7);
        assert!(cell.get().is_none());
    }
}
