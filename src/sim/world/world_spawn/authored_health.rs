//! Authored health writes around class Unlimbo. Native arithmetic and class
//! differences: object_health.json/map_health (416 original-byte executions).
//! Unit7435DA, Aircraft41B3A5, Infantry51FE5C, Building44FB67.

use super::*;
use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X87};

pub(super) fn map_object_type<'a>(
    category: EntityCategory,
    name: &str,
    rules: &'a RuleSet,
) -> Option<&'a ObjectType> {
    let category = match category {
        EntityCategory::Unit => ObjectCategory::Vehicle,
        EntityCategory::Aircraft => ObjectCategory::Aircraft,
        EntityCategory::Infantry => ObjectCategory::Infantry,
        EntityCategory::Structure => ObjectCategory::Building,
    };
    rules.object_in_category(category, name)
}

pub(super) fn authored_health(category: EntityCategory, authored: i32, strength: i32) -> i32 {
    let authored = if category == EntityCategory::Structure {
        authored.min(256)
    } else {
        authored
    };
    let fraction = X87::load_f64(NativeF64Bits::from_bits(0x3f70_0000_0000_0000))
        .expect("exact binary fraction");
    let input = X87::load_i32(authored);
    let strength_value = X87::load_i32(strength);
    let product = match category {
        EntityCategory::Unit | EntityCategory::Aircraft => {
            X87::mul(X87::mul(input, strength_value), fraction)
        }
        EntityCategory::Infantry | EntityCategory::Structure => {
            X87::mul(X87::mul(input, fraction), strength_value)
        }
    };
    let mut actual = X87::ftol_i32_low_masked(product);
    if actual > strength.wrapping_sub(3) {
        actual = strength;
    }
    match category {
        EntityCategory::Unit | EntityCategory::Aircraft if actual == 0 => 1,
        EntityCategory::Infantry => actual.max(1),
        _ => actual,
    }
}

fn apply(entity: &mut GameEntity, authored: i32, strength: i32) {
    let actual = authored_health(entity.category, authored, strength);
    entity.health.current = actual;
    entity.estimated_health.reset(actual);
}

impl Simulation {
    /// Mobile loaders write authored HP only after successful Unlimbo and reload
    /// live type Strength then. Building44FB67..44FBF9 writes both fields first.
    /// Keeping this transaction separate also preserves constructor Strength for
    /// manager creation and admission callbacks of mobile classes.
    pub(super) fn unlimbo_authored_techno(
        &mut self,
        entity: GameEntity,
        authored: i32,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> (u64, RevealOutcome) {
        let building = entity.category == EntityCategory::Structure;
        // InitManagers belongs to construction, before the map loader's stores.
        let (id, position) = self.store_with_constructor_managers(entity, rules);
        if building && let Some(rules) = rules {
            let entity = self
                .substrate
                .entities
                .get(id)
                .expect("constructed map Techno");
            let strength = map_object_type(
                entity.category,
                self.interner.resolve(entity.type_ref()),
                rules,
            )
            .expect("map class type was resolved before construction")
            .strength;
            apply(
                self.substrate
                    .entities
                    .get_mut(id)
                    .expect("constructed map Techno"),
                authored,
                strength,
            );
        }
        let (id, outcome) = self.unlimbo_constructed_parent(id, position, rules, overlay_registry);
        if !building
            && matches!(outcome, RevealOutcome::Revealed { .. })
            && let Some(rules) = rules
        {
            let strength = self
                .substrate
                .entities
                .get(id)
                .and_then(|entity| {
                    map_object_type(
                        entity.category,
                        self.interner.resolve(entity.type_ref()),
                        rules,
                    )
                })
                .expect("admitted map Techno retains a resolved live type")
                .strength;
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                apply(entity, authored, strength);
            }
        }
        (id, outcome)
    }
}
