//! Interned identifiers for the simulation’s configured bridge warheads.

/// Sim-owned pre-resolved rule handles (F04): the `[CombatDamage]` bridge
/// warhead identities combat compares per detonation. `RuleSet` keeps only the
/// canonical names (`rules.bridge_warheads`); the interned ids live here,
/// built after interning and rebuilt from rules on
/// load. Derived cache — never serialized, never hashed. An unset key keeps
/// the RulesClass constructor's null, the empty name, which no warhead has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRuleHandles {
    /// Resolved `IonCannonWarhead=` (retail `IonCannonWH`).
    pub ion_cannon: crate::sim::intern::InternedId,
    /// Resolved `C4Warhead=` (retail `Super`).
    pub c4: crate::sim::intern::InternedId,
    /// Resolved `CrushWarhead=` (retail `Crush`).
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
