//! A world field as a copy-on-write value that knows whether it was written.
//!
//! The simulation's object pass is to run an object's turn on a private fork
//! of the world. Every field of the world is a [`Forked<T>`] for that: a fork
//! takes one `Arc` handle per field, a turn copies only the fields it writes,
//! and the fork can say which those were and drop them again.
//!
//! - In ordinary play a field is `Owned`: its value sits inline, reads go
//!   through `Deref` and writes through `DerefMut`, which also notes the
//!   write, so owners keep their code.
//! - [`Forked::share`] moves the value behind an `Arc` before the world is
//!   forked. [`Forked::fork`] of a shared field is another handle on the same
//!   value, without a copy.
//! - The first write through a handle makes it `Written`: the handle takes
//!   its own copy of the value and keeps the base it copied.
//!   [`Forked::restore_from`] drops the copy and reads the kept base again.
//!   Neither step touches the `Arc`'s count, which every worker thread
//!   would otherwise contend on.
//! - [`Forked::unshare`] takes the value back inline once the forks are gone.
//!
//! It never changes a value: `Debug`, serialization and the saved layout are
//! those of the content, and a clone is a full, independent value. A write
//! through interior mutability goes through `Deref`: it is not noted, and on
//! a shared value every fork sees it.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A world field as a copy-on-write value that knows whether it was written.
pub enum Forked<T> {
    /// The value, inline, and whether `DerefMut` handed it out since the flag
    /// was last cleared.
    Owned { value: T, written: bool },
    /// A value its forks read together, unwritten.
    Shared(Arc<T>),
    /// A shared value this handle has written: its own copy, and the base it
    /// copied, kept so that [`Forked::restore_from`] need not take it again.
    Written { base: Arc<T>, own: T },
    /// Inside a change from one of the forms above to another only, while the
    /// value or its `Arc` moves. No caller ever meets it.
    Moving,
}

/// A value was reached while it moved from one form to another.
#[cold]
#[inline(never)]
fn moving() -> ! {
    panic!("a Forked value was used while it changed form")
}

impl<T> Forked<T> {
    /// An owned, unwritten field holding `value`.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self::Owned {
            value,
            written: false,
        }
    }

    /// Move the value behind an `Arc`, unwritten, for a world about to be
    /// forked. A shared value stays as it is.
    pub fn share(&mut self) {
        *self = match std::mem::replace(self, Self::Moving) {
            Self::Owned { value, .. } => Self::Shared(Arc::new(value)),
            Self::Written { own, .. } => Self::Shared(Arc::new(own)),
            shared => shared,
        };
    }

    /// Take the value back inline once every fork of it is gone. A shared
    /// value comes back unwritten; one this handle wrote comes back written,
    /// leaving its base to the forks. An owned value stays as it is.
    ///
    /// # Panics
    /// When a fork still holds the shared value: the world was unshared too
    /// early.
    pub fn unshare(&mut self) {
        if matches!(self, Self::Owned { .. }) {
            return;
        }
        *self = match std::mem::replace(self, Self::Moving) {
            Self::Shared(shared) => match Arc::try_unwrap(shared) {
                Ok(value) => Self::new(value),
                Err(shared) => {
                    *self = Self::Shared(shared);
                    panic!("a fork still holds a shared {}", std::any::type_name::<T>());
                }
            },
            Self::Written { own, .. } => Self::Owned {
                value: own,
                written: true,
            },
            other => other,
        };
    }

    /// Whether `DerefMut` handed the value out since the flag was last
    /// cleared, or since the value was shared. A shared value is not written.
    #[inline]
    pub fn written(&self) -> bool {
        matches!(
            self,
            Self::Owned { written: true, .. } | Self::Written { .. }
        )
    }

    /// Forget that the value was written. A shared value this handle wrote
    /// becomes owned: it keeps the value and lets go of its base.
    pub fn clear_written(&mut self) {
        match self {
            Self::Owned { written, .. } => *written = false,
            Self::Shared(_) | Self::Moving => {}
            Self::Written { .. } => {
                if let Self::Written { own, .. } = std::mem::replace(self, Self::Moving) {
                    *self = Self::new(own);
                }
            }
        }
    }
}

impl<T: Clone> Forked<T> {
    /// A fork's handle on this field: a shared value gives another handle on
    /// the same value, without a copy; any other gives a full copy.
    pub fn fork(&self) -> Self {
        match self {
            Self::Shared(shared) => Self::Shared(Arc::clone(shared)),
            Self::Owned { .. } | Self::Written { .. } | Self::Moving => self.clone(),
        }
    }

    /// Become `base`'s fork again, dropping whatever this fork wrote. A fork
    /// of a shared `base` goes back to the handle it kept and leaves the
    /// `Arc`'s count alone.
    pub fn restore_from(&mut self, base: &Self) {
        let same_base = match (&*self, base) {
            (Self::Shared(kept) | Self::Written { base: kept, .. }, Self::Shared(shared)) => {
                Arc::ptr_eq(kept, shared)
            }
            _ => false,
        };
        if !same_base {
            *self = base.fork();
        } else if let Self::Written { .. } = self
            && let Self::Written { base: kept, .. } = std::mem::replace(self, Self::Moving)
        {
            *self = Self::Shared(kept);
        }
    }

    /// The value itself, moved out of a world that is being taken apart. A
    /// shared value is copied unless this is its last handle.
    pub fn into_inner(self) -> T {
        match self {
            Self::Owned { value, .. } => value,
            Self::Shared(shared) => Arc::unwrap_or_clone(shared),
            Self::Written { own, .. } => own,
            Self::Moving => moving(),
        }
    }

    /// The first write through a shared handle: the handle takes its own
    /// copy of the value and keeps the base.
    #[cold]
    #[inline(never)]
    fn own_for_write(&mut self) -> &mut T {
        let Self::Shared(shared) = &*self else {
            moving()
        };
        let own = T::clone(shared);
        if let Self::Shared(base) = std::mem::replace(self, Self::Moving) {
            *self = Self::Written { base, own };
        }
        match self {
            Self::Written { own, .. } => own,
            Self::Owned { .. } | Self::Shared(_) | Self::Moving => moving(),
        }
    }
}

impl<T> Deref for Forked<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        match self {
            Self::Owned { value, .. } => value,
            Self::Shared(shared) => shared,
            Self::Written { own, .. } => own,
            Self::Moving => moving(),
        }
    }
}

impl<T: Clone> DerefMut for Forked<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        match self {
            Self::Owned { value, written } => {
                *written = true;
                value
            }
            Self::Written { own, .. } => own,
            Self::Shared(_) | Self::Moving => self.own_for_write(),
        }
    }
}

impl<T> From<T> for Forked<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Default> Default for Forked<T> {
    #[inline]
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// A clone is a full, independent value, owned and unwritten.
impl<T: Clone> Clone for Forked<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self::new(T::clone(self))
    }
}

impl<T: fmt::Debug> fmt::Debug for Forked<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: serde::Serialize> serde::Serialize for Forked<T> {
    #[inline]
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        T::serialize(self, serializer)
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Forked<T> {
    #[inline]
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::new)
    }
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;
    use crate::util::save_codec;

    #[test]
    fn a_write_through_a_fork_copies_the_value_and_leaves_the_base_alone() {
        let mut base = Forked::new(vec![1, 2]);
        base.share();
        let mut fork = base.fork();
        let Forked::Shared(shared) = &base else {
            panic!("a shared base")
        };
        assert!(matches!(&fork, Forked::Shared(handle) if Arc::ptr_eq(handle, shared)));
        fork.push(3);
        assert_eq!(*fork, [1, 2, 3]);
        assert!(fork.written());
        assert_eq!(*base, [1, 2]);
        assert!(!base.written());
        // The fork kept its base: going back to it takes no new handle.
        assert_eq!(Arc::strong_count(shared), 2);
        fork.restore_from(&base);
        assert!(matches!(&fork, Forked::Shared(handle) if Arc::ptr_eq(handle, shared)));
        assert_eq!(Arc::strong_count(shared), 2);
        assert!(!fork.written());
    }

    #[test]
    fn deref_mut_notes_a_write_and_deref_does_not() {
        let mut value = Forked::new(7_u32);
        assert_eq!(*value, 7);
        assert!(!value.written());
        *value += 1;
        assert!(value.written());
        // A clone is the value alone.
        let copy = value.clone();
        assert_eq!(*copy, 8);
        assert!(!copy.written());
        value.clear_written();
        assert!(!value.written());
        assert_eq!(*value, 8);
    }

    #[test]
    fn a_shared_value_forks_without_a_copy_and_comes_back_owned() {
        let mut base = Forked::new(String::from("a"));
        base.push('b');
        base.share();
        assert!(!base.written());
        let fork = base.fork();
        assert_eq!(*fork, "ab");
        drop(fork);
        base.unshare();
        assert!(matches!(base, Forked::Owned { written: false, .. }));
        assert_eq!(*base, "ab");
        // An owned value forks as a full copy, and so restores as one.
        let mut copy = base.fork();
        copy.push('c');
        assert_eq!(*base, "ab");
        copy.restore_from(&base);
        assert_eq!(*copy, "ab");
        assert!(!copy.written());
    }

    #[test]
    #[should_panic(expected = "a fork still holds a shared u8")]
    fn unsharing_a_value_a_fork_still_holds_panics() {
        let mut base = Forked::new(1_u8);
        base.share();
        let mut fork = base.fork();
        *fork = 2;
        base.unshare();
    }

    #[test]
    fn it_saves_as_its_value_and_loads_owned_and_unwritten() {
        let mut value = Forked::new((3_u16, String::from("x")));
        value.0 = 4;
        let bytes = save_codec::serialize(&value).unwrap();
        assert_eq!(bytes, save_codec::serialize(&(4_u16, "x")).unwrap());
        value.share();
        assert_eq!(save_codec::serialize(&value).unwrap(), bytes);
        let loaded: Forked<(u16, String)> = save_codec::deserialize(&bytes).unwrap();
        assert!(matches!(loaded, Forked::Owned { written: false, .. }));
        assert_eq!(*loaded, (4, String::from("x")));
        assert_eq!(
            save_codec::layout::describe::<Forked<(u16, String)>>().unwrap(),
            save_codec::layout::describe::<(u16, String)>().unwrap()
        );
    }
}
