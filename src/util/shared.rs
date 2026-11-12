//! Copy-on-write sharing for the presentation frame (docs/sim-thread.md,
//! phase 2).
//!
//! A tick that goes to the simulation thread publishes a copy of the live
//! world for drawing. [`Shared`] and [`ChunkedVec`] make that copy share
//! large state with the live world until the live world writes it:
//!
//! - [`Shared<T>`] shares a whole value. A write copies it only while a
//!   frame still shares it. It suits large state that rarely changes, such
//!   as the zone graph and the terrain's per-cell arrays.
//! - [`ChunkedVec<T>`] shares a vector in fixed chunks. A write copies only
//!   its own chunk. It suits large per-cell state that changes in small
//!   parts every tick, such as occupancy, fog planes and overlays.
//!
//! Reads go through `Deref`, and writes through `DerefMut` or `IndexMut`, so
//! owners keep their code. Neither type changes a value: equality, hashing
//! and serialization are those of the content.

use std::ops::{Deref, DerefMut, Index, IndexMut};
use std::sync::Arc;

/// A value shared copy-on-write with presentation copies.
#[derive(Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(value))
    }

    /// Whether `other` shares this value's storage.
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// The value, copied only if a frame still shares it.
    pub fn into_inner(self) -> T
    where
        T: Clone,
    {
        Arc::try_unwrap(self.0).unwrap_or_else(|shared| (*shared).clone())
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: serde::Serialize> serde::Serialize for Shared<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Shared<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::new)
    }
}

/// A derived cache a copy leaves empty: it is rebuilt from its sources on
/// the first read, so the presentation frame never copies it.
#[derive(Debug, Default)]
pub struct DerivedCache<T>(pub crate::util::sync::OnceLock<T>);

impl<T> Clone for DerivedCache<T> {
    fn clone(&self) -> Self {
        Self(crate::util::sync::OnceLock::new())
    }
}

impl<T> Deref for DerivedCache<T> {
    type Target = crate::util::sync::OnceLock<T>;

    fn deref(&self) -> &crate::util::sync::OnceLock<T> {
        &self.0
    }
}

impl<T> DerefMut for DerivedCache<T> {
    fn deref_mut(&mut self) -> &mut crate::util::sync::OnceLock<T> {
        &mut self.0
    }
}

/// Elements per chunk of a [`ChunkedVec`]: small enough that a scattered
/// write copies little, large enough that a copy of the whole vector shares
/// few pointers.
pub const CHUNK: usize = 256;

/// A vector shared copy-on-write with presentation copies in chunks of
/// [`CHUNK`] elements; a write copies only its chunk.
#[derive(Default)]
pub struct ChunkedVec<T> {
    chunks: Vec<Arc<Vec<T>>>,
    len: usize,
}

impl<T: Clone> ChunkedVec<T> {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        (index < self.len).then(|| &self.chunks[index / CHUNK][index % CHUNK])
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        (index < self.len)
            .then(|| &mut Arc::make_mut(&mut self.chunks[index / CHUNK])[index % CHUNK])
    }

    pub fn push(&mut self, value: T) {
        if self.len % CHUNK == 0 {
            self.chunks.push(Arc::new(Vec::with_capacity(CHUNK)));
        }
        Arc::make_mut(self.chunks.last_mut().expect("a chunk for the push")).push(value);
        self.len += 1;
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + '_ {
        self.chunks.iter().flat_map(|chunk| chunk.iter())
    }

    /// Every element mutably; copies every shared chunk.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> + '_ {
        self.chunks
            .iter_mut()
            .flat_map(|chunk| Arc::make_mut(chunk).iter_mut())
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.len = 0;
    }

    /// Set every element to `value`.
    pub fn fill(&mut self, value: T) {
        for chunk in &mut self.chunks {
            Arc::make_mut(chunk).fill(value.clone());
        }
    }

    pub fn to_vec(&self) -> Vec<T> {
        let mut out = Vec::with_capacity(self.len);
        for chunk in &self.chunks {
            out.extend_from_slice(chunk);
        }
        out
    }

    /// A vector of `len` copies of `value`.
    pub fn filled(value: T, len: usize) -> Self {
        let mut chunks = Vec::with_capacity(len.div_ceil(CHUNK));
        let mut left = len;
        while left > 0 {
            let take = left.min(CHUNK);
            chunks.push(Arc::new(vec![value.clone(); take]));
            left -= take;
        }
        Self { chunks, len }
    }

    /// Resize to `len`, filling new elements with `value`.
    pub fn resize(&mut self, len: usize, value: T) {
        if len <= self.len {
            let keep = len.div_ceil(CHUNK);
            self.chunks.truncate(keep);
            if let Some(last) = self.chunks.last_mut() {
                Arc::make_mut(last).truncate(len - (keep - 1) * CHUNK);
            }
            self.len = len;
        } else {
            while self.len < len {
                self.push(value.clone());
            }
        }
    }

    /// Copy `values` over the elements from `start`.
    pub fn copy_from(&mut self, start: usize, values: &[T]) {
        for (offset, value) in values.iter().enumerate() {
            self[start + offset] = value.clone();
        }
    }

    /// Whether `other` shares chunk `chunk` of this vector's storage.
    pub fn shares_chunk_with(&self, other: &Self, chunk: usize) -> bool {
        Arc::ptr_eq(&self.chunks[chunk], &other.chunks[chunk])
    }
}

impl<T: Clone> From<Vec<T>> for ChunkedVec<T> {
    fn from(values: Vec<T>) -> Self {
        let len = values.len();
        let mut chunks = Vec::with_capacity(len.div_ceil(CHUNK));
        let mut values = values.into_iter();
        loop {
            let chunk: Vec<T> = values.by_ref().take(CHUNK).collect();
            if chunk.is_empty() {
                break;
            }
            chunks.push(Arc::new(chunk));
        }
        Self { chunks, len }
    }
}

impl<T: Clone> FromIterator<T> for ChunkedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        iter.into_iter().collect::<Vec<T>>().into()
    }
}

impl<T> Clone for ChunkedVec<T> {
    fn clone(&self) -> Self {
        Self {
            chunks: self.chunks.clone(),
            len: self.len,
        }
    }
}

impl<T: Clone> Index<usize> for ChunkedVec<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        self.get(index).expect("chunked index in range")
    }
}

impl<T: Clone> IndexMut<usize> for ChunkedVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.get_mut(index).expect("chunked index in range")
    }
}

impl<T: Clone + PartialEq> PartialEq for ChunkedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().eq(other.iter())
    }
}

impl<T: Clone + Eq> Eq for ChunkedVec<T> {}

impl<T: Clone + PartialEq> PartialEq<[T]> for ChunkedVec<T> {
    fn eq(&self, other: &[T]) -> bool {
        self.len == other.len() && self.iter().eq(other.iter())
    }
}

impl<T: Clone + PartialEq> PartialEq<Vec<T>> for ChunkedVec<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        *self == other[..]
    }
}

impl<T: Clone + PartialEq, const N: usize> PartialEq<[T; N]> for ChunkedVec<T> {
    fn eq(&self, other: &[T; N]) -> bool {
        *self == other[..]
    }
}

impl<T: Clone + PartialEq> PartialEq<Vec<T>> for &ChunkedVec<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        **self == other[..]
    }
}

impl<T: Clone + PartialEq> PartialEq<[T]> for &ChunkedVec<T> {
    fn eq(&self, other: &[T]) -> bool {
        **self == *other
    }
}

/// Hashes exactly as the `Vec<T>` it stands for: the length, then one
/// `hash_slice` of every element in order.
impl<T: Clone + std::hash::Hash> std::hash::Hash for ChunkedVec<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.to_vec().hash(state);
    }
}

impl<T: Clone + std::fmt::Debug> std::fmt::Debug for ChunkedVec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Clone + serde::Serialize> serde::Serialize for ChunkedVec<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // A known length, as a Vec gives: bincode needs it up front.
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.len))?;
        for value in self.iter() {
            seq.serialize_element(value)?;
        }
        seq.end()
    }
}

impl<'de, T: Clone + serde::Deserialize<'de>> serde::Deserialize<'de> for ChunkedVec<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(Self::from)
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut ChunkedVec<T> {
    type Item = &'a mut T;
    type IntoIter = Box<dyn Iterator<Item = &'a mut T> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter_mut())
    }
}

impl<'a, T: Clone> IntoIterator for &'a ChunkedVec<T> {
    type Item = &'a T;
    type IntoIter = Box<dyn DoubleEndedIterator<Item = &'a T> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_copies_only_its_own_chunk() {
        let live: ChunkedVec<u32> = (0..(CHUNK as u32 * 3)).collect();
        let published = live.clone();
        let mut live = live;
        live[CHUNK + 5] = 99;
        assert!(live.shares_chunk_with(&published, 0));
        assert!(!live.shares_chunk_with(&published, 1));
        assert!(live.shares_chunk_with(&published, 2));
        assert_eq!(published[CHUNK + 5], CHUNK as u32 + 5);
        assert_eq!(live[CHUNK + 5], 99);
        assert_eq!(live.len(), CHUNK * 3);
    }

    #[test]
    fn a_shared_value_copies_on_its_first_write_only() {
        let mut live = Shared::new(vec![1, 2, 3]);
        let published = live.clone();
        assert!(live.shares_storage_with(&published));
        live.push(4);
        assert!(!live.shares_storage_with(&published));
        assert_eq!(*published, [1, 2, 3]);
        assert_eq!(*live, [1, 2, 3, 4]);
    }
}
