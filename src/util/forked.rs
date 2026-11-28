//! A world field as a copy-on-write value that knows whether it was written.
//!
//! The simulation's object pass is to run an object's turn on a private fork
//! of the world. Every field of the world is a [`Forked<T>`] for that: a fork
//! takes one `Arc` handle per field, a turn copies only the fields it writes,
//! and the fork can say which those were and drop them again.
//!
//! - In ordinary play a field is `Inline`: its value sits in the field, reads
//!   go through `Deref` and writes through `DerefMut`, which also notes the
//!   write, so owners keep their code. A read tests one byte first.
//! - [`Forked::share`] moves the value behind an `Arc` before the world is
//!   forked. [`Forked::fork`] of a shared field is another handle on the same
//!   value, without a copy.
//! - The first write through a handle gives it its own copy of the value,
//!   inline again, and keeps the base it copied. [`Forked::restore_from`]
//!   drops the copy and reads the kept base again. Neither step touches the
//!   `Arc`'s count, which every worker thread would otherwise contend on.
//! - [`Forked::unshare`] takes the value back inline once the forks are gone.
//!
//! It never changes a value: `Debug`, serialization and the saved layout are
//! those of the content, and a clone is a full, independent value. A write
//! through interior mutability goes through `Deref`: it is not noted, and on
//! a shared value every fork sees it.
//!
//! Its own operations are associated functions, as `Arc`'s are
//! (`Forked::share(&mut world.field)`): a method would hide the value's
//! method of the same name, and `world.field.fork()` must stay the value's.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A world field as a copy-on-write value that knows whether it was written.
///
/// `repr(u8)` keeps the form in the first byte, with `Inline` as zero: a read
/// of an inline field is one byte's test before the value, wherever it is
/// inlined. Without it the form may hide in a niche of `T` that takes more
/// instructions to tell apart.
#[repr(u8)]
pub enum Forked<T> {
    /// The value, in the field, and whether `DerefMut` handed it out since
    /// the flag was last cleared. A world's own field has no `base`. A
    /// handle that wrote a shared value holds its own copy here and keeps
    /// the `base` it copied, so that [`Forked::restore_from`] need not take
    /// it again.
    Inline {
        written: bool,
        base: Option<Arc<T>>,
        value: T,
    } = 0,
    /// A value its forks read together, unwritten.
    Shared(Arc<T>) = 1,
    /// Inside a change from one of the forms above to the other only, while
    /// the value or its `Arc` moves. No caller ever meets it.
    Moving = 2,
}

/// A value was reached while it moved from one form to the other.
#[cold]
#[inline(never)]
fn moving() -> ! {
    panic!("a Forked value was used while it changed form")
}

impl<T> Forked<T> {
    /// An inline, unwritten field holding `value`.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self::Inline {
            written: false,
            base: None,
            value,
        }
    }

    /// Move the value behind an `Arc`, unwritten, for a world about to be
    /// forked. A shared value stays as it is.
    pub fn share(this: &mut Self) {
        *this = match std::mem::replace(this, Self::Moving) {
            Self::Inline { value, .. } => Self::Shared(Arc::new(value)),
            shared => shared,
        };
    }

    /// Take a shared value back inline, unwritten, once every fork of it is
    /// gone. An inline value stays as it is and lets go of any base it kept.
    ///
    /// # Panics
    /// When a fork still holds the shared value: the world was unshared too
    /// early.
    pub fn unshare(this: &mut Self) {
        match this {
            Self::Inline { base, .. } => *base = None,
            Self::Moving => {}
            Self::Shared(_) => {
                if let Self::Shared(shared) = std::mem::replace(this, Self::Moving) {
                    match Arc::try_unwrap(shared) {
                        Ok(value) => *this = Self::new(value),
                        Err(shared) => {
                            *this = Self::Shared(shared);
                            panic!("a fork still holds a shared {}", std::any::type_name::<T>());
                        }
                    }
                }
            }
        }
    }

    /// Whether `DerefMut` handed the value out since the flag was last
    /// cleared, or since the value was shared. A shared value is not written.
    #[inline]
    pub fn written(this: &Self) -> bool {
        matches!(this, Self::Inline { written: true, .. })
    }

    /// Forget that the value was written.
    #[inline]
    pub fn clear_written(this: &mut Self) {
        if let Self::Inline { written, .. } = this {
            *written = false;
        }
    }

    /// A read that found the field shared: the value its forks read
    /// together. Out of line, so that an inline field's read stays one test
    /// wherever it is inlined; a world's fields are all inline unless it is
    /// forked.
    #[cold]
    #[inline(never)]
    fn shared_ref(&self) -> &T {
        match self {
            Self::Inline { value, .. } => value,
            Self::Shared(shared) => shared,
            Self::Moving => moving(),
        }
    }
}

impl<T: Clone> Forked<T> {
    /// A fork's handle on this field: a shared value gives another handle on
    /// the same value, without a copy; an inline one gives a full copy.
    pub fn fork(this: &Self) -> Self {
        match this {
            Self::Shared(shared) => Self::Shared(Arc::clone(shared)),
            Self::Inline { .. } | Self::Moving => this.clone(),
        }
    }

    /// Become `base`'s fork again, dropping whatever this fork wrote. A fork
    /// of a shared `base` goes back to the handle it kept and leaves the
    /// `Arc`'s count alone.
    pub fn restore_from(this: &mut Self, base: &Self) {
        let same_base = match (&*this, base) {
            (
                Self::Shared(kept)
                | Self::Inline {
                    base: Some(kept), ..
                },
                Self::Shared(shared),
            ) => Arc::ptr_eq(kept, shared),
            _ => false,
        };
        if !same_base {
            *this = Self::fork(base);
        } else if let Self::Inline { .. } = this
            && let Self::Inline {
                base: Some(kept), ..
            } = std::mem::replace(this, Self::Moving)
        {
            *this = Self::Shared(kept);
        }
    }

    /// The value itself, moved out of a world that is being taken apart. A
    /// shared value is copied unless this is its last handle. A method, for
    /// the fixtures that take a world apart field by field; no world value
    /// has an `into_inner` of its own.
    pub fn into_inner(self) -> T {
        match self {
            Self::Inline { value, .. } => value,
            Self::Shared(shared) => Arc::unwrap_or_clone(shared),
            Self::Moving => moving(),
        }
    }

    /// A write that found the field shared: the handle takes its own copy of
    /// the value, written, and keeps the base. Out of line for the reason
    /// [`Self::shared_ref`] is.
    #[cold]
    #[inline(never)]
    fn write_shared(&mut self) -> &mut T {
        if let Self::Shared(shared) = &*self {
            let value = T::clone(shared);
            if let Self::Shared(base) = std::mem::replace(self, Self::Moving) {
                *self = Self::Inline {
                    written: true,
                    base: Some(base),
                    value,
                };
            }
        }
        match self {
            Self::Inline { written, value, .. } => {
                *written = true;
                value
            }
            Self::Shared(_) | Self::Moving => moving(),
        }
    }
}

impl<T> Deref for Forked<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        match self {
            Self::Inline { value, .. } => value,
            Self::Shared(_) | Self::Moving => self.shared_ref(),
        }
    }
}

impl<T: Clone> DerefMut for Forked<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        match self {
            Self::Inline { written, value, .. } => {
                *written = true;
                value
            }
            Self::Shared(_) | Self::Moving => self.write_shared(),
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

/// A clone is a full, independent value, inline and unwritten.
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
        Forked::share(&mut base);
        let mut fork = Forked::fork(&base);
        let Forked::Shared(shared) = &base else {
            panic!("a shared base")
        };
        assert!(matches!(&fork, Forked::Shared(handle) if Arc::ptr_eq(handle, shared)));
        fork.push(3);
        assert_eq!(*fork, [1, 2, 3]);
        assert!(Forked::written(&fork));
        assert_eq!(*base, [1, 2]);
        assert!(!Forked::written(&base));
        // The fork kept its base: going back to it takes no new handle.
        assert_eq!(Arc::strong_count(shared), 2);
        Forked::restore_from(&mut fork, &base);
        assert!(matches!(&fork, Forked::Shared(handle) if Arc::ptr_eq(handle, shared)));
        assert_eq!(Arc::strong_count(shared), 2);
        assert!(!Forked::written(&fork));
    }

    #[test]
    fn deref_mut_notes_a_write_and_deref_does_not() {
        let mut value = Forked::new(7_u32);
        assert_eq!(*value, 7);
        assert!(!Forked::written(&value));
        *value += 1;
        assert!(Forked::written(&value));
        // A clone is the value alone.
        let copy = value.clone();
        assert_eq!(*copy, 8);
        assert!(!Forked::written(&copy));
        Forked::clear_written(&mut value);
        assert!(!Forked::written(&value));
        assert_eq!(*value, 8);
    }

    #[test]
    fn a_shared_value_forks_without_a_copy_and_comes_back_inline() {
        let mut base = Forked::new(String::from("a"));
        base.push('b');
        Forked::share(&mut base);
        assert!(!Forked::written(&base));
        let fork = Forked::fork(&base);
        assert_eq!(*fork, "ab");
        drop(fork);
        Forked::unshare(&mut base);
        assert!(matches!(
            base,
            Forked::Inline {
                written: false,
                base: None,
                ..
            }
        ));
        assert_eq!(*base, "ab");
        // An inline value forks as a full copy, and so restores as one.
        let mut copy = Forked::fork(&base);
        copy.push('c');
        assert_eq!(*base, "ab");
        Forked::restore_from(&mut copy, &base);
        assert_eq!(*copy, "ab");
        assert!(!Forked::written(&copy));
    }

    #[test]
    #[should_panic(expected = "a fork still holds a shared u8")]
    fn unsharing_a_value_a_fork_still_holds_panics() {
        let mut base = Forked::new(1_u8);
        Forked::share(&mut base);
        let mut fork = Forked::fork(&base);
        *fork = 2;
        Forked::unshare(&mut base);
    }

    #[test]
    fn it_saves_as_its_value_and_loads_inline_and_unwritten() {
        let mut value = Forked::new((3_u16, String::from("x")));
        value.0 = 4;
        let bytes = save_codec::serialize(&value).unwrap();
        assert_eq!(bytes, save_codec::serialize(&(4_u16, "x")).unwrap());
        Forked::share(&mut value);
        assert_eq!(save_codec::serialize(&value).unwrap(), bytes);
        let loaded: Forked<(u16, String)> = save_codec::deserialize(&bytes).unwrap();
        assert!(matches!(
            loaded,
            Forked::Inline {
                written: false,
                base: None,
                ..
            }
        ));
        assert_eq!(*loaded, (4, String::from("x")));
        assert_eq!(
            save_codec::layout::describe::<Forked<(u16, String)>>().unwrap(),
            save_codec::layout::describe::<(u16, String)>().unwrap()
        );
    }
}
