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
        // The class array indexes answer as the name lookup does, too.
        for set in [&rules, &other] {
            for id in [e1, later] {
                for category in CLASS_ARRAYS {
                    assert_eq!(
                        table.array_index(&interner, id, category, set),
                        set.type_array_index(category, interner.resolve(id))
                    );
                }
            }
        }
        assert_eq!(
            table.array_index(&interner, e1, ObjectCategory::Infantry, &rules),
            Some(0)
        );
        assert_eq!(
            table.array_index(&interner, later, ObjectCategory::Infantry, &rules),
            Some(1)
        );
        assert_eq!(
            TypeHandleTable::default()
                .object(&interner, e1, &rules)
                .map(std::ptr::from_ref),
            name_path(e1, &rules)
        );
    }

    /// The per-registry lookup answers as `RuleSet::object_in_category` does
    /// for the id's name: a name two registries list keeps both types, from
    /// the table for the set it was bound to and by name for another set or
    /// an id interned after binding; an id the interner never made names
    /// nothing.
    #[test]
    fn category_lookup_answers_as_the_name_path() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=TWIN\n[VehicleTypes]\n0=TWIN\n[AircraftTypes]\n0=SOLO\n\
             [TWIN]\nStrength=100\n[SOLO]\nStrength=50\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let other = RuleSet::from_ini(&ini).unwrap();
        let mut interner = StringInterner::new();
        let twin = interner.intern("twin");
        interner.bind_types(&rules);
        let later = interner.intern("SOLO");
        for set in [&rules, &other] {
            let typed = |id, category| {
                interner
                    .object_type_in_category(id, category, set)
                    .map(|object| (std::ptr::from_ref(object), object.category))
            };
            for id in [twin, later] {
                for category in CLASS_ARRAYS {
                    assert_eq!(
                        typed(id, category).map(|(object, _)| object),
                        set.object_in_category(category, interner.resolve(id))
                            .map(std::ptr::from_ref)
                    );
                }
            }
            let category = |category| typed(twin, category).map(|(_, category)| category);
            assert_eq!(
                category(ObjectCategory::Infantry),
                Some(ObjectCategory::Infantry)
            );
            assert_eq!(
                category(ObjectCategory::Vehicle),
                Some(ObjectCategory::Vehicle)
            );
            assert!(typed(later, ObjectCategory::Aircraft).is_some());
            assert!(typed(InternedId::from_index(99), ObjectCategory::Vehicle).is_none());
        }
    }

    /// An id this interner never produced — the shape of a fixture entity built
    /// against another interner — names no type. `handle` used to reach
    /// `StringInterner::resolve`, which panics for a foreign id, so the whole
    /// caller (the hover-shutdown landing arm's per-cell pass) died instead of
    /// answering "unknown type".
    #[test]
    fn a_foreign_id_names_no_type_instead_of_panicking() {
        let rules = tiny_rules();
        let mut interner = StringInterner::new();
        let e1 = interner.intern("E1");
        let table = TypeHandleTable::build(&rules, &interner);
        // An empty interner asked for index 1: the exact shape that panicked
        // ("the len is 0 but the index is 1") on the fall-through past the
        // table's own row.
        let foreign = StringInterner::new();
        let foreign_id = InternedId::from_index(1);
        assert_eq!(table.handle(&foreign, foreign_id, &rules), None);
        assert!(table.object(&foreign, foreign_id, &rules).is_none());
        // The table's own set still answers for the id it holds.
        assert_eq!(table.handle(&interner, e1, &rules), rules.type_handle("E1"));
    }
}
