//! Large state the presentation copy of the world keeps in step with the live
//! world by copying only what changed (docs/sim-thread.md).
//!
//! Drawing reads its own copy of the world, never the live one: the live
//! world and the copy share no storage. A copy of the whole terrain each tick
//! would cost too much, so these containers record what a write changed, and
//! [`Synced::sync_into`] / [`ChunkedVec::sync_into`] bring the one copy last
//! synced from them up to date by copying only that:
//!
//! - [`Synced<T>`] for a value that rarely changes as a whole: the copy takes
//!   it again only after a write.
//! - [`ChunkedVec<T>`] for large per-cell state written in small parts: the
//!   copy takes only the chunks of [`CHUNK`] elements a write touched.
//!
//! A source knows the copy it last synced (by the copy's id); syncing into
//! any other copy copies everything. Reads go through `Deref`/`Index` and
//! writes through `DerefMut`/`IndexMut`, so owners keep their code. Neither
//! type changes a value: equality, hashing and serialization are those of the
//! content, and a clone is a full, independent value.

use std::ops::{Deref, DerefMut, Index, IndexMut};
use std::sync::atomic::{AtomicU64, Ordering};

/// A process-unique id for a synced container, so a source can tell the copy
/// it last synced from any other.
fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Which copy a source last synced. A container that keeps its own change
/// marks holds one (`sim::cell_map::CellMap`).
#[derive(Debug)]
pub struct SyncMark {
    /// This container's own id.
    id: u64,
    /// The id of the copy this source last synced into; 0 for none.
    synced_to: u64,
}

impl SyncMark {
    pub fn new() -> Self {
        Self {
            id: next_id(),
            synced_to: 0,
        }
    }

    /// Whether `mirror` is the copy this source last synced.
    pub fn knows(&self, mirror: &Self) -> bool {
        self.synced_to != 0 && self.synced_to == mirror.id
    }

    /// Note `mirror` as the copy this source last synced.
    pub fn synced(&mut self, mirror: &Self) {
        self.synced_to = mirror.id;
    }
}

impl Default for SyncMark {
    fn default() -> Self {
        Self::new()
    }
}

/// A clone is a new value: it has its own id and has synced nothing.
impl Clone for SyncMark {
    fn clone(&self) -> Self {
        Self::new()
    }
}

/// A value the presentation copy takes again only after a write.
#[derive(Default)]
pub struct Synced<T> {
    value: T,
    changed: bool,
    mark: SyncMark,
}

impl<T> Synced<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            changed: true,
            mark: SyncMark::new(),
        }
    }

    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: Clone> Synced<T> {
    /// Bring `mirror`, a presentation copy, up to this value.
    pub fn sync_into(&mut self, mirror: &mut Self) {
        if self.changed || !self.mark.knows(&mirror.mark) {
            mirror.value.clone_from(&self.value);
        }
        self.changed = false;
        self.mark.synced_to = mirror.mark.id;
    }
}

impl<T: Clone> Clone for Synced<T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<T> Deref for Synced<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Synced<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.changed = true;
        &mut self.value
    }
}

impl<T> From<T> for Synced<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: PartialEq> PartialEq for Synced<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Eq> Eq for Synced<T> {}

impl<T: std::hash::Hash> std::hash::Hash for Synced<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Synced<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}

impl<T: serde::Serialize> serde::Serialize for Synced<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(serializer)
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Synced<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::new)
    }
}

/// A derived cache a copy leaves empty: it is rebuilt from its sources on
/// the first read, so the presentation copy never copies it.
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
/// write costs the copy little, large enough that the change marks stay few.
pub const CHUNK: usize = 256;

/// A vector whose presentation copy takes only the chunks of [`CHUNK`]
/// elements a write touched.
#[derive(Default)]
pub struct ChunkedVec<T> {
    chunks: Vec<Vec<T>>,
    len: usize,
    /// One bit per chunk written since the last sync.
    changed: Vec<u64>,
    /// The chunk layout changed since the last sync (a push, a resize).
    reshaped: bool,
    mark: SyncMark,
}

impl<T: Clone> ChunkedVec<T> {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
            changed: Vec::new(),
            reshaped: true,
            mark: SyncMark::new(),
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

    #[inline]
    fn note(&mut self, chunk: usize) {
        let word = chunk / 64;
        if word >= self.changed.len() {
            self.changed.resize(word + 1, 0);
        }
        self.changed[word] |= 1 << (chunk % 64);
    }

    fn note_all(&mut self) {
        self.reshaped = true;
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        self.note(index / CHUNK);
        Some(&mut self.chunks[index / CHUNK][index % CHUNK])
    }

    pub fn push(&mut self, value: T) {
        if self.len % CHUNK == 0 {
            self.chunks.push(Vec::with_capacity(CHUNK));
        }
        self.chunks
            .last_mut()
            .expect("a chunk for the push")
            .push(value);
        self.len += 1;
        self.note_all();
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + '_ {
        self.chunks.iter().flat_map(|chunk| chunk.iter())
    }

    /// Every element mutably; the copy takes every chunk again.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> + '_ {
        self.note_all();
        self.chunks.iter_mut().flat_map(|chunk| chunk.iter_mut())
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.len = 0;
        self.note_all();
    }

    /// Set every element to `value`.
    pub fn fill(&mut self, value: T) {
        for chunk in &mut self.chunks {
            chunk.fill(value.clone());
        }
        self.note_all();
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
            chunks.push(vec![value.clone(); take]);
            left -= take;
        }
        Self {
            chunks,
            len,
            ..Self::new()
        }
    }

    /// Resize to `len`, filling new elements with `value`.
    pub fn resize(&mut self, len: usize, value: T) {
        if len <= self.len {
            let keep = len.div_ceil(CHUNK);
            self.chunks.truncate(keep);
            if let Some(last) = self.chunks.last_mut() {
                last.truncate(len - (keep - 1) * CHUNK);
            }
            self.len = len;
            self.note_all();
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

    /// Bring `mirror`, a presentation copy, up to this vector: only the
    /// chunks written since the last sync into it, or all of it when it is
    /// another copy or the layout changed.
    pub fn sync_into(&mut self, mirror: &mut Self) {
        if self.reshaped || !self.mark.knows(&mirror.mark) {
            mirror.chunks.clone_from(&self.chunks);
            mirror.len = self.len;
        } else {
            for (word, bits) in self.changed.iter().enumerate() {
                let mut bits = *bits;
                while bits != 0 {
                    let chunk = word * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    mirror.chunks[chunk].clone_from(&self.chunks[chunk]);
                }
            }
        }
        self.changed.iter_mut().for_each(|word| *word = 0);
        self.reshaped = false;
        self.mark.synced_to = mirror.mark.id;
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
            chunks.push(chunk);
        }
        Self {
            chunks,
            len,
            ..Self::new()
        }
    }
}

impl<T: Clone> FromIterator<T> for ChunkedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        iter.into_iter().collect::<Vec<T>>().into()
    }
}

/// A clone is a full, independent vector that has synced nothing.
impl<T: Clone> Clone for ChunkedVec<T> {
    fn clone(&self) -> Self {
        Self {
            chunks: self.chunks.clone(),
            len: self.len,
            ..Self::new()
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
    fn a_sync_copies_only_the_chunks_written_since_the_last() {
        let mut live: ChunkedVec<u32> = (0..(CHUNK as u32 * 3)).collect();
        let mut copy = ChunkedVec::new();
        live.sync_into(&mut copy);
        assert_eq!(copy, live);
        live[CHUNK + 5] = 99;
        // A write the copy has not taken yet does not reach it.
        copy[5] = 7;
        live.sync_into(&mut copy);
        assert_eq!(copy[CHUNK + 5], 99);
        assert_eq!(copy[5], 7, "an untouched chunk is not copied again");
        // Another copy takes everything.
        let mut other = ChunkedVec::new();
        live.sync_into(&mut other);
        assert_eq!(other, live);
    }

    #[test]
    fn a_synced_value_is_copied_again_only_after_a_write() {
        let mut live = Synced::new(vec![1, 2, 3]);
        let mut copy = Synced::new(Vec::new());
        live.sync_into(&mut copy);
        assert_eq!(*copy, [1, 2, 3]);
        copy.push(9);
        live.sync_into(&mut copy);
        assert_eq!(*copy, [1, 2, 3, 9], "an unwritten value is not copied");
        live.push(4);
        live.sync_into(&mut copy);
        assert_eq!(*copy, [1, 2, 3, 4]);
    }
}
