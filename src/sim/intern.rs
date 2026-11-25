//! String interning for zero-cost owner/type_ref clones in the sim layer.
//!
//! `InternedId` is a `Copy` newtype around `u32`. Cloning an entity's owner or
//! type_ref becomes a register copy instead of a heap allocation. The
//! `StringInterner` maps strings to IDs and back, using uppercase normalization
//! so case-insensitive comparisons of owners `intern` adds become plain `==`
//! on IDs.
//!
//! ## Determinism
//! The interner is part of `Simulation` — all peers intern the same strings
//! from the same rules.ini + map data in the same order, producing identical IDs.

mod type_handle_table;

use std::fmt;

use self::type_handle_table::TypeHandleTable;
use crate::rules::art_data::{AnimConfigHandle, AnimTypeRuntimeConfig};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::{RuleSet, TypeHandle};
use crate::util::forked::Forked;
use crate::util::lookup_hash::LookupMap;
use crate::util::native_string::with_ascii_uppercase;
use crate::util::synced::Synced;

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

/// A name a constructor is given: an id the simulation already holds, or
/// text from outside it (a map row, a tool, a test). The constructor interns
/// text only where it always did, after its type lookup succeeds, so an id
/// caller builds no `String` and a text caller adds ids in the same order.
#[derive(Clone, Copy, Debug)]
pub enum InternKey<'a> {
    Id(InternedId),
    Name(&'a str),
}

impl From<InternedId> for InternKey<'_> {
    fn from(id: InternedId) -> Self {
        Self::Id(id)
    }
}

impl<'a, T: AsRef<str> + ?Sized> From<&'a T> for InternKey<'a> {
    fn from(name: &'a T) -> Self {
        Self::Name(name.as_ref())
    }
}

/// Bidirectional string interner with uppercase-normalized lookup.
///
/// `intern("Americans")` and `intern("americans")` return the same ID, so two
/// distinct ids of names `intern` adds never name strings equal under ASCII
/// case folding. A damaged save can list such a pair; a load keeps both ids,
/// and lookups find the first.
/// The display string preserves the casing of the first call that created the entry.
#[derive(Debug)]
pub struct StringInterner {
    /// Uppercase-normalized key → ID. Only looked up, never walked, so it
    /// takes the lookup hasher.
    /// Synced into the presentation copy (docs/sim-thread.md);
    /// [`Self::intern`] writes only for a new string.
    to_id: Synced<LookupMap<String, InternedId>>,
    /// ID (index) → display string (first-seen casing).
    to_str: Synced<Vec<String>>,
    /// Derived: each id's object, projectile, warhead, weapon and AnimType
    /// config handles in the bound rules ([`Self::bind_types`], [`Self::cover_types`]). Never
    /// serialized; a load binds again.
    types: Synced<TypeHandleTable>,
    /// [`Self::serial`]; never serialized.
    serial: u64,
}

fn next_interner_serial() -> u64 {
    static SERIALS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    SERIALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

impl StringInterner {
    /// Create an empty interner.
    pub fn new() -> Self {
        Self {
            to_id: Synced::default(),
            to_str: Synced::default(),
            types: Synced::default(),
            serial: next_interner_serial(),
        }
    }

    /// This interner's identity: distinct from every other interner the
    /// process makes, a clone or a load included (a clone's later ids can
    /// name other strings). It reaches no result; a cache keyed by interned
    /// ids compares it to know whose ids it holds.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// Resolve the strings interned so far to their objects in `rules`, so
    /// [`Self::object_type`] answers for them without hashing a name.
    pub fn bind_types(&mut self, rules: &RuleSet) {
        self.types = TypeHandleTable::build(rules, self).into();
    }

    /// Bind the strings interned since [`Self::bind_types`] as well: the
    /// object columns when `rules` is the set it bound, the AnimType columns
    /// for whatever set and ART layout `rules` holds. Nothing when none are
    /// new and the AnimType columns are current.
    pub fn cover_types(&mut self, rules: &RuleSet) {
        if !self.types.lags(self, rules) {
            return;
        }
        let mut types = std::mem::take(&mut self.types);
        types.cover(rules, self);
        self.types = types;
    }

    /// The projectile type `id` names in `rules`, as
    /// `rules.projectile(self.resolve(id))` answers, through the bound
    /// handles where they apply.
    #[inline]
    pub fn projectile_type<'r>(
        &self,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::projectile_type::ProjectileType> {
        self.types.projectile(self, id, rules)
    }

    /// The warhead `id` names in `rules`, as
    /// `rules.warhead(self.resolve(id))` answers, through the bound handles
    /// where they apply.
    #[inline]
    pub fn warhead_type<'r>(
        &self,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::warhead_type::WarheadType> {
        self.types.warhead(self, id, rules)
    }

    /// The weapon `id` names in `rules`, as
    /// `rules.weapon(self.resolve(id))` answers, through the bound handles
    /// where they apply.
    #[inline]
    pub fn weapon_type<'r>(
        &self,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::weapon_type::WeaponType> {
        self.types.weapon(self, id, rules)
    }

    /// The handle of the AnimType runtime config `id` names in
    /// `rules.art()`, as `rules.art().anim_config_handle(self.resolve(id))`
    /// answers, through the bound handles where they apply.
    #[inline]
    pub fn anim_type_config_handle(
        &self,
        id: InternedId,
        rules: &RuleSet,
    ) -> Option<AnimConfigHandle> {
        self.types.anim_config(self, id, rules)
    }

    /// The AnimType runtime config `id` names, as
    /// `rules.art().anim_runtime_config(self.resolve(id))` answers, the same
    /// way.
    #[inline]
    pub fn anim_type_config<'r>(
        &self,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r AnimTypeRuntimeConfig> {
        self.anim_type_config_handle(id, rules)
            .map(|handle| rules.art().anim_config(handle))
    }

    /// Whether the AnimType config `handle` is `[General] DropZoneAnim=`'s:
    /// as an ASCII case-insensitive compare of the two names answers for a
    /// type that has a config, the same way.
    #[inline]
    pub fn is_drop_zone_anim(&self, handle: AnimConfigHandle, rules: &RuleSet) -> bool {
        self.types.drop_zone_anim(rules) == Some(handle)
    }

    /// The object type `id` names in `rules`, as `rules.object(name)`
    /// answers for the name `self.try_resolve(id)` gives (none for an id
    /// this interner never made), through the bound handles where they
    /// apply.
    #[inline]
    pub fn object_type<'r>(&self, id: InternedId, rules: &'r RuleSet) -> Option<&'r ObjectType> {
        self.types.object(self, id, rules)
    }

    /// Resolve an already known concrete family. Native GetTechnoType returns
    /// the constructor's typed pointer (Infantry51FAF0), even when another
    /// class registry contains the same name. As
    /// `rules.object_in_category(category, self.try_resolve(id)?)` answers,
    /// through the bound handles where they apply.
    #[inline]
    pub fn object_type_in_category<'r>(
        &self,
        id: InternedId,
        category: crate::rules::object_type::ObjectCategory,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        self.types.object_in_category(self, id, category, rules)
    }

    /// The handle of the object type `id` names in `rules`, as
    /// `rules.type_handle(name)` answers for `self.try_resolve(id)`, the
    /// same way.
    #[inline]
    pub fn type_handle(&self, id: InternedId, rules: &RuleSet) -> Option<TypeHandle> {
        self.types.handle(self, id, rules)
    }

    pub fn type_handle_in_category(
        &self,
        id: InternedId,
        category: crate::rules::object_type::ObjectCategory,
        rules: &RuleSet,
    ) -> Option<TypeHandle> {
        self.types.handle_in_category(self, id, category, rules)
    }

    /// Native class methods hold the concrete Type pointer (Infantry51D707,
    /// 518DCF), not the broad name winner from another class's registry.
    pub fn animation_sequence_in_category<'r>(
        &self,
        id: InternedId,
        category: crate::rules::object_type::ObjectCategory,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::animation_sequence::SequenceSet> {
        rules.animation_sequence_by_handle(self.type_handle_in_category(id, category, rules)?)
    }

    /// The sequences of the object type `id` names in `rules`, as
    /// `rules.animation_sequence(self.resolve(id))` answers, the same way.
    #[inline]
    pub fn animation_sequence<'r>(
        &self,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::animation_sequence::SequenceSet> {
        rules.animation_sequence_by_handle(self.type_handle(id, rules)?)
    }

    /// The native array index (`+0xDF8`) of the type `id` names in
    /// `category`'s array, as `rules.type_array_index(category,
    /// self.resolve(id))` answers, the same way.
    #[inline]
    pub fn type_array_index(
        &self,
        id: InternedId,
        category: crate::rules::object_type::ObjectCategory,
        rules: &RuleSet,
    ) -> Option<i32> {
        self.types.array_index(self, id, category, rules)
    }

    /// Intern a string, returning its ID. If already interned (case-insensitive),
    /// returns the existing ID. Otherwise assigns the next sequential ID.
    pub fn intern(&mut self, s: &str) -> InternedId {
        if let Some(id) = self.get(s) {
            return id;
        }
        self.push(s.to_string())
    }

    /// `key`'s id: an id as it is, a name as [`Self::intern`] gives it.
    pub fn intern_key(&mut self, key: InternKey<'_>) -> InternedId {
        match key {
            InternKey::Id(id) => id,
            InternKey::Name(name) => self.intern(name),
        }
    }

    /// `key`'s id without adding one: an id as it is, a name as
    /// [`Self::get`] finds it.
    pub fn key_id(&self, key: InternKey<'_>) -> Option<InternedId> {
        match key {
            InternKey::Id(id) => Some(id),
            InternKey::Name(name) => self.get(name),
        }
    }

    /// `key`'s text: a name as given, an id as [`Self::resolve`] reads it.
    pub fn key_name<'a>(&'a self, key: InternKey<'a>) -> &'a str {
        match key {
            InternKey::Id(id) => self.resolve(id),
            InternKey::Name(name) => name,
        }
    }

    /// The object type `key` names in `rules`: [`Self::object_type`] for an
    /// id, `rules.object(name)` for a name, which adds no id.
    pub fn key_object_type<'r>(
        &self,
        key: InternKey<'_>,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        match key {
            InternKey::Id(id) => self.object_type(id, rules),
            InternKey::Name(name) => rules.object(name),
        }
    }

    /// Give `s` the next id. A name equal under ASCII case folding to one
    /// already held keeps looking up the earlier id: [`Self::intern`] never
    /// pushes such a name, and a load whose list holds one anyway (only a
    /// damaged save can) resolves it as `intern` would.
    fn push(&mut self, s: String) -> InternedId {
        let id = InternedId(self.to_str.len() as u32);
        self.to_id.entry(s.to_ascii_uppercase()).or_insert(id);
        self.to_str.push(s);
        id
    }

    /// Look up an ID without inserting. Returns `None` if not yet interned.
    /// The sidebar and the simulation ask many times a frame, so the key is
    /// folded on the stack.
    pub fn get(&self, s: &str) -> Option<InternedId> {
        with_ascii_uppercase(s, |key| self.to_id.get(key).copied())
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

    /// Folds every name in ID order, ignoring case, for the diagnostic that
    /// compares two clients' interners. Not part of the state hash.
    pub(crate) fn fold_names(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        for name in self.to_str.iter() {
            name.to_ascii_uppercase().hash(hasher);
        }
    }
}

impl Default for StringInterner {
    fn default() -> Self {
        Self::new()
    }
}

/// The world's interner, a copy-on-write field (`util::forked`): a name it
/// already holds is looked up through a read, so only a name it adds writes
/// it, and a fork of the world copies the table only then.
impl Forked<StringInterner> {
    /// [`StringInterner::intern`], writing only for a new name.
    pub fn intern(&mut self, s: &str) -> InternedId {
        match self.get(s) {
            Some(id) => id,
            None => StringInterner::intern(self, s),
        }
    }

    /// [`StringInterner::intern_key`], writing only for a new name.
    pub fn intern_key(&mut self, key: InternKey<'_>) -> InternedId {
        match self.key_id(key) {
            Some(id) => id,
            None => StringInterner::intern_key(self, key),
        }
    }
}

impl Clone for StringInterner {
    /// The same strings and ids, under a serial of its own.
    fn clone(&self) -> Self {
        Self {
            to_id: self.to_id.clone(),
            to_str: self.to_str.clone(),
            types: self.types.clone(),
            serial: next_interner_serial(),
        }
    }
}

impl serde::Serialize for StringInterner {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_str.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for StringInterner {
    /// Each saved name keeps its id, its position in the list.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut interner = StringInterner::new();
        for s in Vec::<String>::deserialize(deserializer)? {
            interner.push(s);
        }
        Ok(interner)
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

impl StringInterner {
    /// Bring `mirror`, the presentation copy of the world's interner, up to
    /// this one (docs/sim-thread.md).
    pub(crate) fn sync_into(&mut self, mirror: &mut Self) {
        let Self {
            to_id,
            to_str,
            types,
            serial,
        } = self;
        to_id.sync_into(&mut mirror.to_id);
        to_str.sync_into(&mut mirror.to_str);
        types.sync_into(&mut mirror.types);
        mirror.serial = *serial;
    }
}

#[cfg(all(test, tests_sim_h_o))]
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
    fn the_worlds_interner_is_written_only_for_a_name_it_adds() {
        let mut sim = crate::sim::world::Simulation::with_seed(1);
        let held = sim.interner.intern("Americans");
        sim.clear_written_fields();
        assert_eq!(sim.interner.intern("AMERICANS"), held);
        assert_eq!(sim.interner.intern_key(InternKey::Name("americans")), held);
        assert_eq!(sim.written_fields(), [""; 0]);
        assert_ne!(sim.interner.intern("Soviet"), held);
        assert_eq!(sim.written_fields(), ["interner"]);
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

    #[test]
    fn load_keeps_the_first_id_of_a_case_variant_name() {
        // Only a damaged save lists a name twice; ids stay positions.
        let mut restored: StringInterner =
            serde_json::from_str(r#"["Americans","HTNK","AMERICANS"]"#).unwrap();

        assert_eq!(restored.intern("americans"), InternedId(0));
        assert_eq!(restored.len(), 3);
        assert_eq!(restored.resolve(InternedId(2)), "AMERICANS");
    }
}
