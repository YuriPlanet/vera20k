//! The read-only per-frame view Lua mods observe.
//!
//! Scripts never borrow engine state: the app builds this snapshot once per
//! committed frame and the API closures read it. That keeps `mlua` out of the
//! simulation, gives every callback the same consistent world, and bounds the
//! cost to one pass over the entity store regardless of how many queries a mod
//! issues.

use std::collections::{HashMap, HashSet};

use crate::map::entities::EntityCategory;
use crate::sim::intern::InternedId;
use crate::sim::runtime::SimRuntime;

/// One techno as scripts see it, in map-cell units.
#[derive(Debug, Clone)]
pub(crate) struct EntitySnapshot {
    pub(crate) id: u64,
    pub(crate) owner: InternedId,
    pub(crate) type_id: InternedId,
    pub(crate) type_name: String,
    pub(crate) category: EntityCategory,
    pub(crate) health: i32,
    pub(crate) max_health: i32,
    /// Cell X with the sub-cell lepton offset, i.e. `rx + sub_x / 256`.
    pub(crate) x: f64,
    /// Cell Y with the sub-cell lepton offset.
    pub(crate) y: f64,
    /// Elevation level (0 = ground).
    pub(crate) z: f64,
}

impl EntitySnapshot {
    /// `FootClass`-derived objects: the only ones movement missions apply to.
    pub(crate) const fn is_mobile(&self) -> bool {
        matches!(
            self.category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        )
    }

    /// The WolfCTOS `GetKind()` spelling.
    pub(crate) const fn kind(&self) -> &'static str {
        match self.category {
            EntityCategory::Unit => "unit",
            EntityCategory::Infantry => "infantry",
            EntityCategory::Structure => "building",
            EntityCategory::Aircraft => "aircraft",
        }
    }
}

/// One house as scripts see it.
#[derive(Debug, Clone)]
pub(crate) struct HouseSnapshot {
    pub(crate) display_name: String,
    pub(crate) credits: i32,
    pub(crate) is_human: bool,
    /// `"hard"`, `"normal"` or `"easy"`, the WolfCTOS `GetAIDifficulty` set.
    pub(crate) difficulty: &'static str,
}

/// Everything a mod may read this frame.
#[derive(Debug, Default)]
pub(crate) struct FrameSnapshot {
    pub(crate) entities: HashMap<u64, EntitySnapshot>,
    /// Entity ids in logic order, so repeated queries return a stable order.
    pub(crate) entity_order: Vec<u64>,
    pub(crate) houses: HashMap<InternedId, HouseSnapshot>,
    /// House ids in native registration order (`HouseClass::Array`).
    pub(crate) house_order: Vec<InternedId>,
    /// `(asker, other)` pairs the asker considers allied.
    pub(crate) alliances: HashSet<(InternedId, InternedId)>,
    pub(crate) local_house: Option<InternedId>,
    pub(crate) selected: Vec<u64>,
    pub(crate) tick: u64,
}

impl FrameSnapshot {
    pub(crate) fn build(runtime: &SimRuntime, selected: &[u64]) -> Self {
        let view = runtime.view();
        let rules = &runtime.resources.rules;
        let interner = view.interner();
        let mut snapshot = Self {
            selected: selected.to_vec(),
            tick: view.session().tick(),
            ..Self::default()
        };

        for house in view.houses().in_order() {
            let name = house.name();
            snapshot.house_order.push(name);
            snapshot.houses.insert(
                name,
                HouseSnapshot {
                    display_name: interner.try_resolve(name).unwrap_or_default().to_string(),
                    credits: house.economy().credits(),
                    is_human: house.is_human(),
                    difficulty: difficulty_name(house.difficulty()),
                },
            );
        }
        snapshot.local_house = view.session().current_house();

        let alliances = view.house_alliances();
        for asker in &snapshot.house_order {
            // `house_alliances` is keyed by normalized name, so resolve first.
            let Some(asker_name) = interner.try_resolve(*asker) else {
                continue;
            };
            for other in &snapshot.house_order {
                let Some(other_name) = interner.try_resolve(*other) else {
                    continue;
                };
                if crate::map::houses::is_allied_with(
                    alliances.as_map(),
                    asker_name,
                    other_name,
                ) {
                    snapshot.alliances.insert((*asker, *other));
                }
            }
        }

        for entity in view.entities().values() {
            // WolfCTOS validates liveness on every query; dead objects never
            // appear in the lists.
            if !entity.is_alive() {
                continue;
            }
            let type_id = entity.type_ref();
            let Some(type_name) = interner.try_resolve(type_id) else {
                continue;
            };
            let max_health = interner
                .type_handle(type_id, rules)
                .map(|handle| rules.object_by_handle(handle).strength)
                .unwrap_or_default();
            let position = entity.position;
            let id = entity.stable_id();
            snapshot.entity_order.push(id);
            snapshot.entities.insert(
                id,
                EntitySnapshot {
                    id,
                    owner: entity.owner(),
                    type_id,
                    type_name: type_name.to_string(),
                    category: entity.category(),
                    health: entity.health.current(),
                    max_health,
                    x: f64::from(position.rx()) + f64::from(position.sub_x().to_num::<f32>()) / 256.0,
                    y: f64::from(position.ry()) + f64::from(position.sub_y().to_num::<f32>()) / 256.0,
                    z: f64::from(position.z()),
                },
            );
        }

        snapshot
    }

    pub(crate) fn entity(&self, id: u64) -> Option<&EntitySnapshot> {
        self.entities.get(&id)
    }
}

const fn difficulty_name(difficulty: crate::sim::house_state::HouseDifficulty) -> &'static str {
    use crate::sim::house_state::HouseDifficulty;
    match difficulty {
        HouseDifficulty::Hard => "hard",
        HouseDifficulty::Normal => "normal",
        HouseDifficulty::Easy => "easy",
    }
}
