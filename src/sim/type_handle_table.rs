//! Precomputed `InternedId -> TypeHandle` map for one-hop entity->type resolution.
//!
//! An entity carries its type as an `InternedId` (`type_ref`). Resolving that to a
//! `&ObjectType` the naive way is two hops: `interner.resolve(id)` (an array index)
//! then `RuleSet::object(name)` (an uppercase `String` allocation + hash lookup).
//! This table collapses the second hop to a precomputed array index, so resolution
//! is two array indexes with no allocation and no hashing.
//!
//! Built once at sim init from a `RuleSet` plus the populated `StringInterner`
//! (after `Simulation::intern_rule_type_ids`). It is a derived cache — read-only during
//! ticks, never serialized, rebuilt on load.
//!
//! ## Dependency rules
//! - Part of sim/; depends on rules/ (one-way) and `sim::intern`. NEVER on
//!   render/ui/sidebar/audio/net.

use crate::rules::ruleset::{RuleSet, TypeHandle};
use crate::sim::intern::{InternedId, StringInterner};

/// Maps an interned type id to its object handle. Dense by `InternedId` index;
/// `None` means the id was interned but names no object (an orphan `type_ref`,
/// e.g. a registry id listed without a `[section]`).
#[derive(Debug, Clone, Default)]
pub struct TypeHandleTable {
    by_interned: Vec<Option<TypeHandle>>,
}

impl TypeHandleTable {
    /// Build from every string currently in the interner. Each id that resolves
    /// to an object (case-insensitively) gets its handle; the rest stay `None`.
    /// Call after `Simulation::intern_rule_type_ids` so every type id is present.
    pub fn build(rules: &RuleSet, interner: &StringInterner) -> Self {
        let mut by_interned = Vec::with_capacity(interner.len());
        for idx in 0..interner.len() as u32 {
            // idx < interner.len(), so resolve() is in bounds.
            let name = interner.resolve(InternedId::from_index(idx));
            by_interned.push(rules.type_handle(name));
        }
        Self { by_interned }
    }

    /// Resolve an interned id to its handle, if it names an object.
    #[inline]
    pub fn handle_for(&self, id: InternedId) -> Option<TypeHandle> {
        self.by_interned.get(id.index() as usize).copied().flatten()
    }

    /// True if no entries were built (e.g. the table has not been resolved yet).
    pub fn is_empty(&self) -> bool {
        self.by_interned.is_empty()
    }

    /// Resolve a type in one precomputed hop, falling back to the name path
    /// only while the table is unbuilt (fixtures that skip resolution).
    #[inline]
    pub fn object<'r>(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::object_type::ObjectType> {
        match self.handle_for(id) {
            Some(handle) => Some(rules.object_by_handle(handle)),
            None if self.is_empty() => rules.object(interner.resolve(id)),
            None => None,
        }
    }

    /// Count of interned ids that did NOT resolve to an object (orphans).
    #[cfg(test)]
    pub fn orphan_count(&self) -> usize {
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

    /// Minimal ruleset with a single infantry object `E1`.
    fn tiny_rules() -> RuleSet {
        let ini = IniFile::from_str("[InfantryTypes]\n0=E1\n\n[E1]\nStrength=100\n");
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

    #[test]
    fn empty_table_reports_empty() {
        assert!(TypeHandleTable::default().is_empty());
        assert_eq!(
            TypeHandleTable::default().handle_for(InternedId::from_index(0)),
            None
        );
    }
}
