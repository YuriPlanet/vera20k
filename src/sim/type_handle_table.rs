//! Precomputed `InternedId -> TypeHandle` map for one-hop entity->type resolution.
//!
//! An entity carries its type as an `InternedId` (`type_ref`). Resolving that to a
//! `&ObjectType` by name is two hops: `interner.resolve(id)` (an array index) then
//! `RuleSet::object(name)` (a hash lookup of the name). This table collapses the
//! second hop to a precomputed array index, so resolution is two array indexes
//! with no hashing.
//!
//! The `StringInterner` owns it: `StringInterner::bind_types` builds it from a
//! `RuleSet` and the strings interned so far (after
//! `Simulation::intern_rule_type_ids`, so every type id is present), and
//! `StringInterner::object_type` reads it. It answers exactly as the name path
//! does: an id interned after the build, or a `RuleSet` other than the one it
//! was built from, takes the name path. A derived cache: never serialized or
//! hashed; a load rebuilds it.
//!
//! ## Dependency rules
//! - Part of sim/; depends on rules/ (one-way) and `sim::intern`. NEVER on
//!   render/ui/audio/net.

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::{RuleSet, TypeHandle};
use crate::sim::intern::{InternedId, StringInterner};

/// Maps an interned type id to its object handle. Dense by `InternedId` index;
/// `None` means the id was interned but names no object (an orphan `type_ref`,
/// e.g. a registry id listed without a `[section]`).
#[derive(Debug, Clone, Default)]
pub struct TypeHandleTable {
    by_interned: Vec<Option<TypeHandle>>,
    /// [`RuleSet::serial`] of the set the handles came from; 0 while unbuilt,
    /// which no set has.
    rules_serial: u64,
}

impl TypeHandleTable {
    /// Build from every string currently in the interner. Each id that resolves
    /// to an object (case-insensitively) gets its handle; the rest stay `None`.
    pub fn build(rules: &RuleSet, interner: &StringInterner) -> Self {
        Self {
            by_interned: (0..interner.len() as u32)
                .map(|idx| rules.type_handle(interner.resolve(InternedId::from_index(idx))))
                .collect(),
            rules_serial: rules.serial(),
        }
    }

    /// The object `id` names in `rules`, as `rules.object(interner.resolve(id))`
    /// answers: from the table when it was built from `rules` and covers `id`,
    /// else by name.
    #[inline]
    pub fn object<'r>(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        self.handle(interner, id, rules)
            .map(|handle| rules.object_by_handle(handle))
    }

    /// The handle of the object `id` names in `rules`, as
    /// `rules.type_handle(interner.resolve(id))` answers, the same way.
    #[inline]
    pub fn handle(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &RuleSet,
    ) -> Option<TypeHandle> {
        match self.by_interned.get(id.index() as usize) {
            Some(&handle) if self.rules_serial == rules.serial() => handle,
            _ => rules.type_handle(interner.resolve(id)),
        }
    }

    /// The handle the table holds for `id`, if it covers it.
    #[cfg(test)]
    pub(crate) fn handle_for(&self, id: InternedId) -> Option<TypeHandle> {
        self.by_interned.get(id.index() as usize).copied().flatten()
    }

    /// Count of interned ids that did NOT resolve to an object (orphans).
    #[cfg(test)]
    pub(crate) fn orphan_count(&self) -> usize {
        self.by_interned.iter().filter(|h| h.is_none()).count()
    }
}

/// Sim-owned pre-resolved rule handles (F04): the `[CombatDamage]` bridge
/// warhead identities combat compares per detonation. `RuleSet` keeps only the
/// canonical names (`rules.bridge_warheads`); the interned ids live here,
/// built beside `TypeHandleTable` after interning and rebuilt from rules on
/// load. Derived cache — never serialized, never hashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRuleHandles {
    /// Resolved `IonCannonWarhead=` (retail default `IonCannonWH`).
    pub ion_cannon: crate::sim::intern::InternedId,
    /// Resolved `C4Warhead=` (retail default `Super`).
    pub c4: crate::sim::intern::InternedId,
    /// Resolved `CrushWarhead=` (retail default `Crush`).
    pub crush: crate::sim::intern::InternedId,
}

impl ResolvedRuleHandles {
    /// Intern the canonical warhead names and pin their ids. Idempotent for an
    /// interner that already carries the names (init and every load path), so
    /// re-resolution cannot grow or reorder serialized interner state.
    pub fn resolve(
        rules: &crate::rules::ruleset::RuleSet,
        interner: &mut crate::sim::intern::StringInterner,
    ) -> Self {
        Self {
            ion_cannon: interner.intern(&rules.bridge_warheads.ion_cannon_name),
            c4: interner.intern(&rules.bridge_warheads.c4_name),
            crush: interner.intern(&rules.bridge_warheads.crush_name),
        }
    }

    /// Whether an interned warhead is the resolved `CrushWarhead=`.
    pub fn is_crush(self, warhead_id: crate::sim::intern::InternedId) -> bool {
        self.crush == warhead_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// Minimal ruleset with two infantry objects, `E1` and `E2`.
    fn tiny_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=E2\n\n[E1]\nStrength=100\n\n[E2]\nStrength=125\n",
        );
        RuleSet::from_ini(&ini).expect("minimal ruleset should parse")
    }

    #[test]
    fn handle_for_resolves_case_insensitively() {
        let rules = tiny_rules();
        let mut interner = StringInterner::new();
        // Intern the lowercased reference; the section header is `E1`.
        let id = interner.intern("e1");
        let table = TypeHandleTable::build(&rules, &interner);
        assert_eq!(table.handle_for(id), rules.type_handle("E1"));
        assert!(table.handle_for(id).is_some());
        assert_eq!(table.orphan_count(), 0);
    }

    #[test]
    fn unknown_interned_id_is_orphan() {
        let rules = tiny_rules();
        let mut interner = StringInterner::new();
        let bogus = interner.intern("NOSUCHTYPE");
        let table = TypeHandleTable::build(&rules, &interner);
        assert_eq!(table.handle_for(bogus), None);
        assert_eq!(table.orphan_count(), 1);
    }

    /// The table answers as the name path does: from its handles for the
    /// set it was built from, by name for another set or an id interned after
    /// the build.
    #[test]
    fn answers_as_the_name_path() {
        let rules = tiny_rules();
        let other = tiny_rules();
        assert_ne!(rules.serial(), other.serial());
        let mut interner = StringInterner::new();
        let e1 = interner.intern("E1");
        let table = TypeHandleTable::build(&rules, &interner);
        let later = interner.intern("e2");
        let name_path =
            |id, rules: &RuleSet| rules.object(interner.resolve(id)).map(std::ptr::from_ref);
        for set in [&rules, &other] {
            for id in [e1, later] {
                assert_eq!(
                    table.object(&interner, id, set).map(std::ptr::from_ref),
                    name_path(id, set)
                );
            }
        }
        assert!(table.object(&interner, e1, &other).is_some());
        assert!(table.object(&interner, later, &rules).is_some());
        assert_eq!(
            TypeHandleTable::default()
                .object(&interner, e1, &rules)
                .map(std::ptr::from_ref),
            name_path(e1, &rules)
        );
    }
}
