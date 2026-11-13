//! Out-of-line storage for state most objects never write.
//!
//! [`Sparse<T>`] stands for a `T` that most of its holders leave at its
//! default. It keeps nothing until the value is first written, and then
//! keeps it in its own allocation. A record with 20,000 instances carries a
//! pointer in each instead of the whole value, and a copy of a record that
//! never wrote it copies none of it.
//!
//! Reads go through `Deref`, where an unwritten value reads as
//! [`SparseDefault::sparse_default`], and writes through `DerefMut`, so
//! owners keep their code. It never changes a value: `Debug`, serialization
//! and the saved layout are those of the content, an unwritten value saves
//! as the default, and a loaded default takes no storage again.

use std::ops::{Deref, DerefMut};

/// A type a [`Sparse`] can hold: its `Default` is what an unwritten value
/// reads as.
pub trait SparseDefault: Default + PartialEq + 'static {
    /// `Self::default()`, built once.
    fn sparse_default() -> &'static Self;
}

/// A `T` kept out of line, and not kept at all until it is first written.
pub struct Sparse<T>(Option<Box<T>>);

impl<T> Sparse<T> {
    /// Whether the value has storage of its own: it was written, or loaded
    /// as something other than the default. A value written back to the
    /// default keeps its storage.
    pub fn is_stored(&self) -> bool {
        self.0.is_some()
    }
}

impl<T> Default for Sparse<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: Clone> Clone for Sparse<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: SparseDefault> Deref for Sparse<T> {
    type Target = T;

    fn deref(&self) -> &T {
        match &self.0 {
            Some(value) => value,
            None => T::sparse_default(),
        }
    }
}

impl<T: SparseDefault> DerefMut for Sparse<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0.get_or_insert_with(Box::default)
    }
}

impl<T: SparseDefault + std::fmt::Debug> std::fmt::Debug for Sparse<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: SparseDefault + serde::Serialize> serde::Serialize for Sparse<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

impl<'de, T: SparseDefault + serde::Deserialize<'de>> serde::Deserialize<'de> for Sparse<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = T::deserialize(deserializer)?;
        Ok(Self(
            (value != *T::sparse_default()).then(|| Box::new(value)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::save_codec;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Slots {
        variant: i32,
        ids: [Option<u64>; 3],
    }

    const UNWRITTEN: Slots = Slots {
        variant: -1,
        ids: [None; 3],
    };

    impl Default for Slots {
        fn default() -> Self {
            UNWRITTEN
        }
    }

    impl SparseDefault for Slots {
        fn sparse_default() -> &'static Self {
            &UNWRITTEN
        }
    }

    #[test]
    fn an_unwritten_value_reads_as_the_default_and_stores_nothing_until_written() {
        let mut value = Sparse::<Slots>::default();
        assert_eq!(*value, UNWRITTEN);
        assert!(!value.is_stored());
        let copy = value.clone();
        value.ids[1] = Some(7);
        assert!(value.is_stored());
        assert_eq!(value.ids, [None, Some(7), None]);
        assert_eq!(value.clone().ids, value.ids);
        assert_eq!(*copy, UNWRITTEN);
        assert!(!copy.is_stored());
    }

    #[test]
    fn it_saves_as_its_content_and_a_loaded_default_stores_nothing() {
        let mut value = Sparse::<Slots>::default();
        assert_eq!(
            save_codec::serialize(&value).unwrap(),
            save_codec::serialize(&UNWRITTEN).unwrap()
        );
        value.variant = 4;
        let written = Slots {
            variant: 4,
            ..UNWRITTEN
        };
        let bytes = save_codec::serialize(&value).unwrap();
        assert_eq!(bytes, save_codec::serialize(&written).unwrap());
        let loaded: Sparse<Slots> = save_codec::deserialize(&bytes).unwrap();
        assert!(loaded.is_stored());
        assert_eq!(*loaded, written);
        // Written back to the default it still saves as the default, which
        // loads without storage.
        value.variant = -1;
        let bytes = save_codec::serialize(&value).unwrap();
        let loaded: Sparse<Slots> = save_codec::deserialize(&bytes).unwrap();
        assert!(!loaded.is_stored());
        assert_eq!(
            save_codec::layout::describe::<Sparse<Slots>>().unwrap(),
            save_codec::layout::describe::<Slots>().unwrap()
        );
    }
}
