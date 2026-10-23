//! String interning for zero-cost owner/type_ref clones in the sim layer.
//!
//! `InternedId` is a `Copy` newtype around `u32`. Cloning an entity's owner or
//! type_ref becomes a register copy instead of a heap allocation. The
//! `StringInterner` maps strings to IDs and back, using uppercase normalization
//! so case-insensitive owner comparisons become plain `==` on IDs.
//!
//! ## Determinism
//! The interner is part of `Simulation` — all peers intern the same strings
//! from the same rules.ini + map data in the same order, producing identical IDs.

use std::collections::BTreeMap;
use std::fmt;

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::{RuleSet, TypeHandle};
use crate::sim::type_handle_table::TypeHandleTable;

/// Interned string handle — `Copy`, `Eq`, `Ord`, `Hash`. Zero-cost clones.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct InternedId(u32);

impl InternedId {
    /// Raw index, mainly for state hashing / debug.
    #[inline]
    pub fn index(self) -> u32 {
        self.0
    }

    /// Reconstruct an id from a raw index. Only valid for indices this interner
    /// produced; used to iterate the interner by position (e.g. table builds).
    #[inline]
    pub fn from_index(idx: u32) -> Self {
        Self(idx)
    }
}

impl fmt::Debug for InternedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InternedId({})", self.0)
    }
}

impl fmt::Display for InternedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Bidirectional string interner with uppercase-normalized lookup.
///
/// `intern("Americans")` and `intern("americans")` return the same ID.
/// The display string preserves the casing of the first call that created the entry.
#[derive(Debug, Clone)]
pub struct StringInterner {
    /// Uppercase-normalized key → ID.
    to_id: BTreeMap<String, InternedId>,
    /// ID (index) → display string (first-seen casing).
    to_str: Vec<String>,
    /// Derived: each id's object handle in the bound rules
    /// ([`Self::bind_types`]). Never serialized; a load binds again.
    types: TypeHandleTable,
}

impl StringInterner {
    /// Create an empty interner.
    pub fn new() -> Self {
        Self {
            to_id: BTreeMap::new(),
            to_str: Vec::new(),
            types: TypeHandleTable::default(),
        }
    }

    /// Resolve the strings interned so far to their objects in `rules`, so
    /// [`Self::object_type`] answers for them without hashing a name.
    pub fn bind_types(&mut self, rules: &RuleSet) {
        self.types = TypeHandleTable::build(rules, self);
    }

    /// The object type `id` names in `rules`, as
    /// `rules.object(self.resolve(id))` answers, through the bound handles
    /// where they apply.
    #[inline]
    pub fn object_type<'r>(&self, id: InternedId, rules: &'r RuleSet) -> Option<&'r ObjectType> {
        self.types.object(self, id, rules)
    }

    /// Resolve an already known concrete family. Native GetTechnoType returns
    /// the constructor's typed pointer (Infantry51FAF0), even when another
    /// class registry contains the same name. The name-only cache stays broad.
    pub fn object_type_in_category<'r>(
        &self,
        id: InternedId,
        category: crate::rules::object_type::ObjectCategory,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        rules.object_in_category(category, self.try_resolve(id)?)
    }

    /// The handle of the object type `id` names in `rules`, as
    /// `rules.type_handle(self.resolve(id))` answers, the same way.
    #[inline]
    pub fn type_handle(&self, id: InternedId, rules: &RuleSet) -> Option<TypeHandle> {
        self.types.handle(self, id, rules)
    }

    /// Intern a string, returning its ID. If already interned (case-insensitive),
    /// returns the existing ID. Otherwise assigns the next sequential ID.
    pub fn intern(&mut self, s: &str) -> InternedId {
        let key = s.to_ascii_uppercase();
        if let Some(&id) = self.to_id.get(&key) {
            return id;
        }
        let id = InternedId(self.to_str.len() as u32);
        self.to_str.push(s.to_string());
        self.to_id.insert(key, id);
        id
    }

    /// Look up an ID without inserting. Returns `None` if not yet interned.
    pub fn get(&self, s: &str) -> Option<InternedId> {
        self.to_id.get(&s.to_ascii_uppercase()).copied()
    }

    /// Resolve an ID back to its display string (first-seen casing).
    ///
    /// # Panics
    /// Panics if `id` was not produced by this interner.
    #[inline]
    pub fn resolve(&self, id: InternedId) -> &str {
        &self.to_str[id.0 as usize]
    }

    /// `resolve` without the panic: `None` for an id this interner never
    /// produced (fixtures that build entities against a different interner).
    #[inline]
    pub fn try_resolve(&self, id: InternedId) -> Option<&str> {
        self.to_str.get(id.0 as usize).map(String::as_str)
    }

    /// Number of unique strings interned.
    pub fn len(&self) -> usize {
        self.to_str.len()
    }

    /// Returns `true` if no strings have been interned.
    pub fn is_empty(&self) -> bool {
        self.to_str.is_empty()
    }
}

impl Default for StringInterner {
    fn default() -> Self {
        Self::new()
    }
}

impl serde::Serialize for StringInterner {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_str.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for StringInterner {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let strings = Vec::<String>::deserialize(deserializer)?;
        let mut to_id = BTreeMap::new();
        for (i, s) in strings.iter().enumerate() {
            to_id.insert(s.to_ascii_uppercase(), InternedId(i as u32));
        }
        Ok(StringInterner {
            to_id,
            to_str: strings,
            types: TypeHandleTable::default(),
        })
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------
//
// Also compiled under `test-support`, because client-crate tests and core
// fixture code link against the lib compiled WITHOUT `cfg(test)`. The
// `RefCell` here is safe — single-threaded test use, never read from sim
// hashed state, never invoked from a sim tick. `#[doc(hidden)]` keeps these
// out of the public API surface.

#[cfg(any(test, feature = "test-support"))]
use std::cell::RefCell;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Thread-local interner used by `test_intern()` so that test entities
    /// created independently still share consistent IDs.
    static TEST_INTERNER: RefCell<StringInterner> = RefCell::new(StringInterner::new());
}

/// Intern a string using the thread-local test interner.
///
/// This lets `GameEntity::test_default()` and other test helpers create
/// entities with consistent `InternedId` values without requiring callers
/// to manage an explicit interner.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn test_intern(s: &str) -> InternedId {
    TEST_INTERNER.with(|cell| cell.borrow_mut().intern(s))
}

/// Get a copy of the thread-local test interner for use in test assertions
/// that need to resolve IDs back to strings.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn test_interner() -> StringInterner {
    TEST_INTERNER.with(|cell| cell.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_returns_same_id_case_insensitive() {
        let mut interner = StringInterner::new();
        let a = interner.intern("Americans");
        let b = interner.intern("americans");
        let c = interner.intern("AMERICANS");
        assert_eq!(a, b);
        assert_eq!(b, c);
        // Display string preserves first-seen casing
        assert_eq!(interner.resolve(a), "Americans");
    }

    #[test]
    fn serde_round_trip() {
        let mut interner = StringInterner::new();
        let a = interner.intern("Americans");
        let b = interner.intern("HTNK");
        let c = interner.intern("Soviet");

        let json = serde_json::to_string(&interner).unwrap();
        let restored: StringInterner = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.len(), 3);
        assert_eq!(restored.resolve(a), "Americans");
        assert_eq!(restored.resolve(b), "HTNK");
        assert_eq!(restored.resolve(c), "Soviet");
        // Case-insensitive lookup still works after round-trip
        assert_eq!(restored.get("americans"), Some(a));
        assert_eq!(restored.get("htnk"), Some(b));
    }
}
