//! Precomputed `InternedId -> TypeHandle` map for one-hop entity->type resolution.
//!
//! An entity carries its type as an `InternedId` (`type_ref`). Resolving that to a
//! `&ObjectType` by name is two hops: `interner.resolve(id)` (an array index) then
//! `RuleSet::object(name)` (a hash lookup of the name). This table collapses the
//! second hop to a precomputed array index, so resolution is two array indexes
//! with no hashing. It keeps each id's native class-array indexes
//! (`RuleSet::type_array_index`), its projectile type (`RuleSet::projectile`,
//! which a bullet's AI reads every frame), its warhead (`RuleSet::warhead`,
//! which every hit reads) and its weapon (`RuleSet::weapon`, which a
//! detonation reads for the bullet's installed weapon) the same way.
//!
//! The `StringInterner` owns it: `StringInterner::bind_types` builds it from a
//! `RuleSet` and the strings interned so far (after
//! `Simulation::intern_rule_type_ids`, so every object type id is present),
//! `StringInterner::cover_types` adds the strings interned since (a
//! projectile's, warhead's or weapon's name is first interned when something
//! fires it), and `StringInterner::object_type`,
//! `StringInterner::object_type_in_category`,
//! `StringInterner::type_array_index`, `StringInterner::projectile_type`,
//! `StringInterner::warhead_type` and `StringInterner::weapon_type` read it. It answers exactly as the name
//! path does: a `RuleSet`'s name indexes never change after it is built, and
//! an id interned after the last build or cover, or a `RuleSet` other than
//! the one it was built from, takes the name path. Its AnimType column
//! follows the bound set's ART registry the same way, by that registry's
//! [`ArtRegistry::anim_configs_serial`](crate::rules::art_data::ArtRegistry::anim_configs_serial):
//! an AnimType that gains a config after a build (a map's root bound at
//! load) takes the name path until the next cover rebuilds the column.
//! Debug builds compare every
//! table answer with the name path
//! ([`live_read_check_enabled`](crate::sim::touch_log::live_read_check_enabled)).
//! A derived cache: never serialized or hashed; a load rebuilds it.
//!
//! ## Dependency rules
//! - Part of sim/; depends on rules/ (one-way) and `sim::intern`. NEVER on
//!   render/ui/audio/net.

use crate::rules::art_data::AnimConfigHandle;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::projectile_type::ProjectileType;
use crate::rules::ruleset::{ProjectileHandle, RuleSet, TypeHandle, WarheadHandle, WeaponHandle};
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::touch_log::live_read_check_enabled;

/// The class arrays, in the order [`TypeHandleTable`] keeps each id's
/// indexes and handles in them ([`class_slot`]).
const CLASS_ARRAYS: [ObjectCategory; 4] = [
    ObjectCategory::Infantry,
    ObjectCategory::Vehicle,
    ObjectCategory::Aircraft,
    ObjectCategory::Building,
];

/// `category`'s position in [`CLASS_ARRAYS`].
const fn class_slot(category: ObjectCategory) -> usize {
    match category {
        ObjectCategory::Infantry => 0,
        ObjectCategory::Vehicle => 1,
        ObjectCategory::Aircraft => 2,
        ObjectCategory::Building => 3,
    }
}

/// Maps an interned type id to its object handle. Dense by `InternedId` index;
/// `None` means the id was interned but names no object (an orphan `type_ref`,
/// e.g. a registry id listed without a `[section]`).
#[derive(Debug, Clone, Default)]
pub(super) struct TypeHandleTable {
    by_interned: Vec<Option<TypeHandle>>,
    /// Each id's native array index (`+0xDF8`) in each class array, in
    /// [`CLASS_ARRAYS`] order, as [`RuleSet::type_array_index`] answers for
    /// its name. Dense by `InternedId` index, like `by_interned`.
    array_indices: Vec<[Option<i32>; 4]>,
    /// Each id's handle in each class registry, in [`CLASS_ARRAYS`] order, as
    /// [`RuleSet::type_handle_in_category`] answers for its name. Dense by
    /// `InternedId` index, like `by_interned`.
    category_handles: Vec<[Option<TypeHandle>; 4]>,
    /// Each id's projectile type, as [`RuleSet::projectile`] answers for its
    /// name. Dense by `InternedId` index, like `by_interned`. gamemd's bullet
    /// holds its BulletType pointer (`+0xAC`, `BulletClass::Construct @
    /// 0x004664C0`); a bullet here holds the name's id, so this column stands
    /// in for that pointer.
    projectiles: Vec<Option<ProjectileHandle>>,
    /// Each id's warhead, as [`RuleSet::warhead`] answers for its name. Dense
    /// by `InternedId` index, like `by_interned`. gamemd's bullet keeps the
    /// WarheadTypeClass pointer `BulletClass::Construct @ 0x004664C0` is given,
    /// and its damage calls pass that pointer on; here they hold the name's
    /// id, so this column stands in for the pointer.
    warheads: Vec<Option<WarheadHandle>>,
    /// Each id's weapon, as [`RuleSet::weapon`] answers for its name. Dense
    /// by `InternedId` index, like `by_interned`. A payload holds its installed
    /// weapon as the name's id
    /// ([`ProjectilePayload::with_installed_weapon`](crate::sim::projectile::ProjectilePayload::with_installed_weapon)),
    /// where gamemd's bullet holds the pointer, so this column stands in for it.
    weapons: Vec<Option<WeaponHandle>>,
    /// [`RuleSet::serial`] of the set the handles came from; 0 while unbuilt,
    /// which no set has.
    rules_serial: u64,
    /// The AnimType columns, kept apart: ART can gain configs after a build.
    anims: AnimColumns,
}

/// Each id's AnimType runtime config in a set's ART registry, and that set's
/// `[General] DropZoneAnim=` config. gamemd's anim holds its AnimTypeClass
/// pointer; an anim here holds its type's name id, so these stand in for it.
#[derive(Debug, Clone, Default)]
struct AnimColumns {
    /// Each id's config, as `rules.art().anim_config_handle(name)` answers.
    /// Dense by `InternedId` index.
    configs: Vec<Option<AnimConfigHandle>>,
    /// `DropZoneAnim=`'s config, as `rules.art().anim_config_handle` answers
    /// for the name; `None` for an unset key or a name with no config.
    drop_zone: Option<AnimConfigHandle>,
    /// [`anim_stamp`] of the set the handles came from; `(0, 0)` while
    /// unbuilt, which no set has.
    stamp: (u64, u64),
}

/// What the AnimType columns depend on: the set (for `DropZoneAnim=`) and its
/// ART registry's handle layout.
fn anim_stamp(rules: &RuleSet) -> (u64, u64) {
    (rules.serial(), rules.art().anim_configs_serial())
}

impl TypeHandleTable {
    /// Build from every string currently in the interner. Each id that resolves
    /// to an object (case-insensitively) gets its handle; the rest stay `None`.
    pub(super) fn build(rules: &RuleSet, interner: &StringInterner) -> Self {
        let mut table = Self {
            rules_serial: rules.serial(),
            ..Self::default()
        };
        table.cover(rules, interner);
        table
    }

    /// Whether `interner` holds ids this table could add from `rules`
    /// ([`Self::cover`]).
    pub(super) fn lags(&self, interner: &StringInterner, rules: &RuleSet) -> bool {
        (self.by_interned.len() < interner.len() && self.rules_serial == rules.serial())
            || self.anims.lags(interner, rules)
    }

    /// Add the ids `interner` made since this table was built or last
    /// covered. The object columns follow only the set they were built from;
    /// the AnimType columns restart for whatever set and ART layout they are
    /// covered for.
    pub(super) fn cover(&mut self, rules: &RuleSet, interner: &StringInterner) {
        self.anims.cover(rules, interner);
        if self.rules_serial != rules.serial() {
            return;
        }
        for idx in self.by_interned.len() as u32..interner.len() as u32 {
            let name = interner.resolve(InternedId::from_index(idx));
            self.by_interned.push(rules.type_handle(name));
            self.array_indices
                .push(CLASS_ARRAYS.map(|category| rules.type_array_index(category, name)));
            self.category_handles
                .push(CLASS_ARRAYS.map(|category| rules.type_handle_in_category(category, name)));
            self.projectiles.push(rules.projectile_handle(name));
            self.warheads.push(rules.warhead_handle(name));
            self.weapons.push(rules.weapon_handle(name));
        }
    }

    /// The projectile type `id` names in `rules`, as
    /// `rules.projectile(interner.resolve(id))` answers: from the table when
    /// it was built from `rules` and covers `id`, else by name.
    #[inline]
    pub(super) fn projectile<'r>(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r ProjectileType> {
        match self.projectiles.get(id.index() as usize) {
            Some(&handle) if self.rules_serial == rules.serial() => {
                debug_assert!(
                    !live_read_check_enabled()
                        || handle == rules.projectile_handle(interner.resolve(id)),
                    "type table answers projectile {handle:?} for {id:?}, unlike its name"
                );
                handle.map(|handle| rules.projectile_by_handle(handle))
            }
            _ => rules.projectile(interner.resolve(id)),
        }
    }

    /// The warhead `id` names in `rules`, as
    /// `rules.warhead(interner.resolve(id))` answers: from the table when it
    /// was built from `rules` and covers `id`, else by name.
    #[inline]
    pub(super) fn warhead<'r>(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r WarheadType> {
        match self.warheads.get(id.index() as usize) {
            Some(&handle) if self.rules_serial == rules.serial() => {
                debug_assert!(
                    !live_read_check_enabled()
                        || handle == rules.warhead_handle(interner.resolve(id)),
                    "type table answers warhead {handle:?} for {id:?}, unlike its name"
                );
                handle.map(|handle| rules.warhead_by_handle(handle))
            }
            _ => rules.warhead(interner.resolve(id)),
        }
    }

    /// The weapon `id` names in `rules`, as
    /// `rules.weapon(interner.resolve(id))` answers: from the table when it
    /// was built from `rules` and covers `id`, else by name.
    #[inline]
    pub(super) fn weapon<'r>(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r WeaponType> {
        match self.weapons.get(id.index() as usize) {
            Some(&handle) if self.rules_serial == rules.serial() => {
                debug_assert!(
                    !live_read_check_enabled()
                        || handle == rules.weapon_handle(interner.resolve(id)),
                    "type table answers weapon {handle:?} for {id:?}, unlike its name"
                );
                handle.map(|handle| rules.weapon_by_handle(handle))
            }
            _ => rules.weapon(interner.resolve(id)),
        }
    }

    /// `id`'s native array index in `category`'s array, as
    /// `rules.type_array_index(category, interner.resolve(id))` answers: from
    /// the table when it was built from `rules` and covers `id`, else by name.
    #[inline]
    pub(super) fn array_index(
        &self,
        interner: &StringInterner,
        id: InternedId,
        category: ObjectCategory,
        rules: &RuleSet,
    ) -> Option<i32> {
        match self.array_indices.get(id.index() as usize) {
            Some(indices) if self.rules_serial == rules.serial() => {
                let index = indices[class_slot(category)];
                debug_assert!(
                    !live_read_check_enabled()
                        || index == rules.type_array_index(category, interner.resolve(id)),
                    "type table answers {category:?} index {index:?} for {id:?}, unlike its name"
                );
                index
            }
            _ => rules.type_array_index(category, interner.resolve(id)),
        }
    }

    /// The object `id` names in `category`'s registry of `rules`, as
    /// `rules.object_in_category(category, name)` answers for the name
    /// `interner.try_resolve(id)` gives (none for an id the interner never
    /// made): from the table when it was built from `rules` and covers `id`,
    /// else by name.
    #[inline]
    pub(super) fn object_in_category<'r>(
        &self,
        interner: &StringInterner,
        id: InternedId,
        category: ObjectCategory,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        self.handle_in_category(interner, id, category, rules)
            .map(|handle| rules.object_by_handle(handle))
    }

    /// The same cached concrete-class lookup, for consumers of type-owned banks.
    pub(super) fn handle_in_category(
        &self,
        interner: &StringInterner,
        id: InternedId,
        category: ObjectCategory,
        rules: &RuleSet,
    ) -> Option<TypeHandle> {
        match self.category_handles.get(id.index() as usize) {
            Some(handles) if self.rules_serial == rules.serial() => {
                let handle = handles[class_slot(category)];
                debug_assert!(
                    !live_read_check_enabled()
                        || handle == rules.type_handle_in_category(category, interner.resolve(id)),
                    "type table answers {category:?} {handle:?} for {id:?}, unlike its name"
                );
                handle
            }
            _ => rules.type_handle_in_category(category, interner.try_resolve(id)?),
        }
    }

    /// The object `id` names in `rules`, as `rules.object(name)` answers for
    /// the name `interner.try_resolve(id)` gives: from the table when it was
    /// built from `rules` and covers `id`, else by name.
    #[inline]
    pub(super) fn object<'r>(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        self.handle(interner, id, rules)
            .map(|handle| rules.object_by_handle(handle))
    }

    /// The handle of the object `id` names in `rules`, as
    /// `rules.type_handle(name)` answers for the name
    /// `interner.try_resolve(id)` gives (none for an id the interner never
    /// made), the same way.
    #[inline]
    pub(super) fn handle(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &RuleSet,
    ) -> Option<TypeHandle> {
        match self.by_interned.get(id.index() as usize) {
            Some(&handle) if self.rules_serial == rules.serial() => {
                debug_assert!(
                    !live_read_check_enabled() || handle == rules.type_handle(interner.resolve(id)),
                    "type table answers {handle:?} for {id:?}, unlike its name"
                );
                handle
            }
            _ => rules.type_handle(interner.try_resolve(id)?),
        }
    }

    /// The AnimType config `id` names in `rules.art()`, as
    /// `rules.art().anim_config_handle(interner.resolve(id))` answers: from
    /// the table when its AnimType columns came from `rules` as it is now and
    /// cover `id`, else by name.
    #[inline]
    pub(super) fn anim_config(
        &self,
        interner: &StringInterner,
        id: InternedId,
        rules: &RuleSet,
    ) -> Option<AnimConfigHandle> {
        match self.anims.configs.get(id.index() as usize) {
            Some(&handle) if self.anims.stamp == anim_stamp(rules) => {
                debug_assert!(
                    !live_read_check_enabled()
                        || handle == rules.art().anim_config_handle(interner.resolve(id)),
                    "type table answers anim config {handle:?} for {id:?}, unlike its name"
                );
                handle
            }
            _ => rules.art().anim_config_handle(interner.resolve(id)),
        }
    }

    /// `rules.general.drop_zone_anim`'s config, as
    /// `rules.art().anim_config_handle` answers for the name, the same way.
    #[inline]
    pub(super) fn drop_zone_anim(&self, rules: &RuleSet) -> Option<AnimConfigHandle> {
        let by_name = || {
            rules
                .general
                .drop_zone_anim
                .as_deref()
                .and_then(|name| rules.art().anim_config_handle(name))
        };
        if self.anims.stamp == anim_stamp(rules) {
            debug_assert!(
                !live_read_check_enabled() || self.anims.drop_zone == by_name(),
                "type table answers DropZoneAnim {:?}, unlike its name",
                self.anims.drop_zone
            );
            self.anims.drop_zone
        } else {
            by_name()
        }
    }

    /// The handle the table holds for `id`, if it covers it.
    #[cfg(test)]
    fn handle_for(&self, id: InternedId) -> Option<TypeHandle> {
        self.by_interned.get(id.index() as usize).copied().flatten()
    }

    /// Count of interned ids that did NOT resolve to an object (orphans).
    #[cfg(test)]
    fn orphan_count(&self) -> usize {
        self.by_interned.iter().filter(|h| h.is_none()).count()
    }
}

impl AnimColumns {
    /// Whether [`Self::cover`] would change the columns for `rules`: they
    /// came from another set or ART layout, or `interner` holds ids since.
    fn lags(&self, interner: &StringInterner, rules: &RuleSet) -> bool {
        self.stamp != anim_stamp(rules) || self.configs.len() < interner.len()
    }

    /// Answer for `rules` as it is now: start again when the columns came
    /// from another set or ART layout, then add the ids `interner` made
    /// since.
    fn cover(&mut self, rules: &RuleSet, interner: &StringInterner) {
        let art = rules.art();
        let stamp = anim_stamp(rules);
        if self.stamp != stamp {
            self.configs.clear();
            self.drop_zone = rules
                .general
                .drop_zone_anim
                .as_deref()
                .and_then(|name| art.anim_config_handle(name));
            self.stamp = stamp;
        }
        for idx in self.configs.len() as u32..interner.len() as u32 {
            let name = interner.resolve(InternedId::from_index(idx));
            self.configs.push(art.anim_config_handle(name));
        }
    }
}

#[cfg(all(test, tests_sim_h_o))]
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

    /// A projectile and a warhead named after the build are covered by
    /// `cover`, and the table answers as `RuleSet::projectile` and
    /// `RuleSet::warhead` do for their names, in any case, for the set it was
    /// built from and for another.
    #[test]
    fn types_interned_after_the_build_answer_as_their_names() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nPrimary=M60\n\
             [M60]\nDamage=20\nProjectile=InvisibleLow\nWarhead=SA\n\
             [InvisibleLow]\nAG=yes\n[SA]\nCellSpread=0\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let other = RuleSet::from_ini(&ini).unwrap();
        let mut interner = StringInterner::new();
        interner.intern("E1");
        let mut table = TypeHandleTable::build(&rules, &interner);
        let bullet = interner.intern("invisiblelow");
        let warhead = interner.intern("sa");
        table.cover(&other, &interner);
        assert!(table.lags(&interner, &rules));
        table.cover(&rules, &interner);
        assert!(!table.lags(&interner, &rules));
        assert!(table.projectiles[bullet.index() as usize].is_some());
        assert!(table.warheads[warhead.index() as usize].is_some());
        for set in [&rules, &other] {
            for id in [bullet, warhead] {
                let name = interner.resolve(id);
                assert_eq!(
                    table.projectile(&interner, id, set).map(std::ptr::from_ref),
                    set.projectile(name).map(std::ptr::from_ref)
                );
                assert_eq!(
                    table.warhead(&interner, id, set).map(std::ptr::from_ref),
                    set.warhead(name).map(std::ptr::from_ref)
                );
            }
            assert!(table.projectile(&interner, bullet, set).is_some());
            assert!(table.warhead(&interner, warhead, set).is_some());
        }
    }

    /// A weapon id answers the WeaponType `RuleSet::weapon` answers for its
    /// name, in any case: one the base rules have and one a later layer
    /// adds, interned after the build and covered, from the table for the
    /// set it was built from and by name for another; an id naming no weapon
    /// answers none.
    #[test]
    fn weapons_answer_as_their_names() {
        let mut ini = IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nPrimary=M60\n\
             [M60]\nDamage=20\nWarhead=SA\n[SA]\nCellSpread=0\n",
        );
        ini.merge(&IniFile::from_str(
            "[E1]\nSecondary=NewGun\n[NewGun]\nDamage=5\nWarhead=SA\n",
        ));
        let rules = RuleSet::from_ini(&ini).unwrap();
        let other = RuleSet::from_ini(&ini).unwrap();
        let mut interner = StringInterner::new();
        let base = interner.intern("m60");
        let unarmed = interner.intern("E1");
        let mut table = TypeHandleTable::build(&rules, &interner);
        let layered = interner.intern("NEWGUN");
        table.cover(&rules, &interner);
        assert!(table.weapons[base.index() as usize].is_some());
        assert!(table.weapons[layered.index() as usize].is_some());
        for set in [&rules, &other] {
            for id in [base, layered, unarmed] {
                assert_eq!(
                    table.weapon(&interner, id, set).map(std::ptr::from_ref),
                    set.weapon(interner.resolve(id)).map(std::ptr::from_ref)
                );
            }
            let damage = |id| table.weapon(&interner, id, set).map(|weapon| weapon.damage);
            assert_eq!(damage(base), Some(20));
            assert_eq!(damage(layered), Some(5));
            assert_eq!(damage(unarmed), None);
        }
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
}
