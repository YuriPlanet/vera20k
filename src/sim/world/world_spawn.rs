//! Entity spawning for the Simulation.
//!
//! Handles spawning entities from map data (`spawn_from_map`) and from
//! production (`spawn_object`). All entities are stored in EntityStore only
//! (BTreeMap<u64, GameEntity>).
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

mod authored_health;
mod construction;

use std::collections::BTreeMap;
use std::fmt;

use super::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, SimSoundEvent, Simulation,
};
use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::base_plan::pack_base_plan_cell;
use crate::sim::base_plan_generation::{preflight_recalc, recalc_base_plan};
use crate::sim::combat::TargetKind;
use crate::sim::components::{BuildingDown, BuildingUp, Health};
use crate::sim::game_entity::{
    GameEntity, GeneratedTechnoInit, StructureUpgradeLink, TechnoConstructorInit,
};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::production::{ProductionCategory, foundation_dimensions};
use crate::sim::vision::MAX_SIGHT_RANGE;

/// Exact generated-object constructor handoff. The later RMG lifecycle owner
/// supplies this table after replaying all successful and discarded native
/// constructor events on the launch Scenario cursor.
#[derive(Debug, Clone, Default)]
pub(crate) struct GeneratedTechnoInitTable {
    entries: BTreeMap<usize, GeneratedTechnoInit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GeneratedTechnoInitError {
    TraceOrdinalMismatch {
        expected: usize,
        found: usize,
    },
    DuplicateEntityIndex(usize),
    MissingEntityIndex(usize),
    UnexpectedEntityIndex(usize),
    IdentityMismatch {
        entity_index: usize,
        expected_type: String,
        found_type: String,
        expected_cell: (u16, u16),
        found_cell: (u16, u16),
    },
    UnresolvableEntity {
        entity_index: usize,
        techno_type: String,
        owner: String,
    },
}

impl fmt::Display for GeneratedTechnoInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TraceOrdinalMismatch { expected, found } => write!(
                f,
                "generated construction trace expected ordinal {expected}, found {found}"
            ),
            Self::DuplicateEntityIndex(index) => {
                write!(
                    f,
                    "duplicate generated Techno binding for entity index {index}"
                )
            }
            Self::MissingEntityIndex(index) => {
                write!(
                    f,
                    "missing generated Techno binding for entity index {index}"
                )
            }
            Self::UnexpectedEntityIndex(index) => {
                write!(
                    f,
                    "unexpected generated Techno binding for entity index {index}"
                )
            }
            Self::IdentityMismatch {
                entity_index,
                expected_type,
                found_type,
                expected_cell,
                found_cell,
            } => write!(
                f,
                "generated Techno binding {entity_index} expected {expected_type} at {expected_cell:?}, found {found_type} at {found_cell:?}"
            ),
            Self::UnresolvableEntity {
                entity_index,
                techno_type,
                owner,
            } => write!(
                f,
                "generated Techno {entity_index} cannot resolve type {techno_type} or owner {owner}"
            ),
        }
    }
}

impl std::error::Error for GeneratedTechnoInitError {}

impl GeneratedTechnoInitTable {
    pub(crate) fn try_new(
        entries: impl IntoIterator<Item = GeneratedTechnoInit>,
    ) -> Result<Self, GeneratedTechnoInitError> {
        let mut by_index = BTreeMap::new();
        for entry in entries {
            let index = entry.entity_index;
            if by_index.insert(index, entry).is_some() {
                return Err(GeneratedTechnoInitError::DuplicateEntityIndex(index));
            }
        }
        Ok(Self { entries: by_index })
    }

    #[cfg(test)]
    pub(crate) fn entry(&self, entity_index: usize) -> Option<&GeneratedTechnoInit> {
        self.entries.get(&entity_index)
    }

    fn validate<'a>(
        &'a self,
        entities: &[MapEntity],
    ) -> Result<Vec<&'a GeneratedTechnoInit>, GeneratedTechnoInitError> {
        if let Some((&index, _)) = self
            .entries
            .iter()
            .find(|(index, _)| **index >= entities.len())
        {
            return Err(GeneratedTechnoInitError::UnexpectedEntityIndex(index));
        }
        let mut ordered = Vec::with_capacity(entities.len());
        for (entity_index, entity) in entities.iter().enumerate() {
            let init = self
                .entries
                .get(&entity_index)
                .ok_or(GeneratedTechnoInitError::MissingEntityIndex(entity_index))?;
            if !init.techno_type.eq_ignore_ascii_case(&entity.type_id)
                || init.cell != (entity.cell_x, entity.cell_y)
            {
                return Err(GeneratedTechnoInitError::IdentityMismatch {
                    entity_index,
                    expected_type: init.techno_type.clone(),
                    found_type: entity.type_id.clone(),
                    expected_cell: init.cell,
                    found_cell: (entity.cell_x, entity.cell_y),
                });
            }
            ordered.push(init);
        }
        Ok(ordered)
    }
}

fn object_uses_voxel(type_id: &str, object: &ObjectType, rules: &RuleSet) -> bool {
    rules
        .art_registry
        .resolve_metadata_entry(type_id, &object.image)
        .map(|entry| entry.voxel)
        .unwrap_or(matches!(
            object.category,
            ObjectCategory::Vehicle | ObjectCategory::Aircraft
        ))
}

impl Simulation {
    fn resolve_techno_constructor_word(
        &mut self,
        init: TechnoConstructorInit,
        expected_generated_identity: Option<(usize, &str, (u16, u16))>,
    ) -> Result<u16, GeneratedTechnoInitError> {
        match init {
            TechnoConstructorInit::FreshScenario => {
                Ok((self.scenario_rng.next_u32() & 0xFFFF) as u16)
            }
            TechnoConstructorInit::Restored(word) => Ok(word),
            TechnoConstructorInit::PreconsumedGenerated(generated) => {
                let Some((entity_index, techno_type, cell)) = expected_generated_identity else {
                    return Err(GeneratedTechnoInitError::UnexpectedEntityIndex(
                        generated.entity_index,
                    ));
                };
                if generated.entity_index != entity_index
                    || !generated.techno_type.eq_ignore_ascii_case(techno_type)
                    || generated.cell != cell
                {
                    return Err(GeneratedTechnoInitError::IdentityMismatch {
                        entity_index,
                        expected_type: generated.techno_type,
                        found_type: techno_type.to_string(),
                        expected_cell: generated.cell,
                        found_cell: cell,
                    });
                }
                Ok(generated.techno_ctor_random_word)
            }
        }
    }

    /// Build one deliberately lifecycle-free Techno for diagnostic tools.
    ///
    /// This is not a gameplay spawn: it does not reveal the entity, register
    /// occupancy, or update house ownership. It still owns the two native
    /// constructor invariants that apply to every Techno-shaped object:
    /// Simulation allocates the stable identity and consumes one Scenario RNG
    /// draw stored at `TechnoClass +0x3C8` (`0x006F3259`) before the entity
    /// enters its store.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn insert_synthetic_techno_for_diagnostics(
        &mut self,
        rx: u16,
        ry: u16,
        z: u8,
        facing: u8,
        owner: InternedId,
        health: Health,
        type_ref: InternedId,
        category: EntityCategory,
        veterancy: u16,
        vision_range: u16,
        is_voxel: bool,
    ) -> u64 {
        let stable_id = self.allocate_stable_id();
        let techno_ctor_random_word = self
            .resolve_techno_constructor_word(TechnoConstructorInit::FreshScenario, None)
            .expect("fresh Techno constructor initialization cannot fail");
        let entity = GameEntity::new_at_frame_from_constructor_word(
            stable_id,
            rx,
            ry,
            z,
            facing,
            owner,
            health,
            type_ref,
            category,
            veterancy,
            vision_range,
            is_voxel,
            self.session.binary_frame,
            techno_ctor_random_word,
        );
        self.substrate.entities.insert(entity);
        stable_id
    }
}

impl Simulation {
    /// Spawn entities from parsed map placements into EntityStore.
    #[cfg(test)]
    pub fn spawn_from_map(
        &mut self,
        entities: &[MapEntity],
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
    ) -> u32 {
        self.spawn_from_map_with_resolved(entities, rules, height_map, None)
    }

    pub fn spawn_from_map_with_resolved(
        &mut self,
        entities: &[MapEntity],
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
    ) -> u32 {
        self.spawn_from_map_with_resolved_and_overlay_registry(
            entities,
            rules,
            height_map,
            resolved_terrain,
            None,
        )
    }

    pub(crate) fn spawn_from_map_with_resolved_and_overlay_registry(
        &mut self,
        entities: &[MapEntity],
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> u32 {
        self.spawn_from_map_with_constructor_inits(
            entities,
            rules,
            height_map,
            resolved_terrain,
            overlay_registry,
            None,
        )
        .expect("fresh fixed-map Techno constructor initialization cannot fail")
    }

    /// Project generated-map Technos using constructor words already consumed
    /// by the launch-time generation cursor. The whole identity table is
    /// validated before the first entity mutates the simulation.
    pub(crate) fn spawn_generated_from_map_with_resolved(
        &mut self,
        entities: &[MapEntity],
        rules: &RuleSet,
        height_map: &BTreeMap<(u16, u16), u8>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        constructor_inits: &GeneratedTechnoInitTable,
    ) -> Result<u32, GeneratedTechnoInitError> {
        self.spawn_from_map_with_constructor_inits(
            entities,
            Some(rules),
            height_map,
            resolved_terrain,
            None,
            Some(constructor_inits),
        )
    }

    fn spawn_from_map_with_constructor_inits(
        &mut self,
        entities: &[MapEntity],
        rules: Option<&RuleSet>,
        height_map: &BTreeMap<(u16, u16), u8>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        constructor_inits: Option<&GeneratedTechnoInitTable>,
    ) -> Result<u32, GeneratedTechnoInitError> {
        let generated_inits = constructor_inits
            .map(|table| table.validate(entities))
            .transpose()?;

        if generated_inits.is_some() {
            let rules = rules.expect("generated-map projection requires rules");
            for (entity_index, entity) in entities.iter().enumerate() {
                if !self.map_entity_resolves_before_constructor(entity, Some(rules), true) {
                    return Err(GeneratedTechnoInitError::UnresolvableEntity {
                        entity_index,
                        techno_type: entity.type_id.clone(),
                        owner: entity.owner.clone(),
                    });
                }
            }
        }

        let mut count: u32 = 0;

        for (entity_index, map_ent) in entities.iter().enumerate() {
            if !self.map_entity_resolves_before_constructor(
                map_ent,
                rules,
                generated_inits.is_some(),
            ) {
                log::warn!(
                    "Skipping map Techno {} owned by {} before constructor resolution",
                    map_ent.type_id,
                    map_ent.owner
                );
                continue;
            }
            let bridge_spawn = map_ent
                .high
                .then(|| {
                    resolved_terrain
                        .and_then(|terrain| terrain.cell(map_ent.cell_x, map_ent.cell_y))
                        .filter(|cell| cell.bridge_walkable)
                        .map(|cell| cell.bridge_deck_level)
                })
                .flatten();
            if map_ent.high && bridge_spawn.is_none() {
                log::warn!(
                    "Map entity {} at ({},{}) requested HIGH spawn but no bridge deck was resolved; falling back to ground",
                    map_ent.type_id,
                    map_ent.cell_x,
                    map_ent.cell_y
                );
            }
            let z: u8 = bridge_spawn.unwrap_or_else(|| {
                height_map
                    .get(&(map_ent.cell_x, map_ent.cell_y))
                    .copied()
                    .unwrap_or(0)
            });

            // Typed production admission has already resolved the class type.
            // Rules-less construction remains an explicit diagnostic seam, using
            // the supplied health directly rather than inventing a type maximum.
            let health = Health {
                current: rules
                    .map(|rules| {
                        authored_health::map_object_type(map_ent.category, &map_ent.type_id, rules)
                            .expect("resolved map type")
                            .strength
                    })
                    .unwrap_or(map_ent.health),
            };

            let uses_voxel_default: bool = match map_ent.category {
                EntityCategory::Unit | EntityCategory::Aircraft => true,
                EntityCategory::Infantry | EntityCategory::Structure => false,
            };
            let uses_voxel: bool = rules
                .and_then(|rules| {
                    authored_health::map_object_type(map_ent.category, &map_ent.type_id, rules)
                        .map(|object| object_uses_voxel(&map_ent.type_id, object, rules))
                })
                .unwrap_or(uses_voxel_default);

            let sight_range = rules
                .and_then(|r| {
                    authored_health::map_object_type(map_ent.category, &map_ent.type_id, r)
                })
                .map(|obj| (obj.sight.max(0) as u16).min(MAX_SIGHT_RANGE))
                .unwrap_or_else(|| Self::default_vision_range_for_category(map_ent.category));

            let stable_id = self.allocate_stable_id();
            let owner_id = self.interner.intern(&map_ent.owner);
            let type_id = self.interner.intern(&map_ent.type_id);
            let constructor_init = generated_inits
                .as_ref()
                .map_or(TechnoConstructorInit::FreshScenario, |inits| {
                    TechnoConstructorInit::PreconsumedGenerated((*inits[entity_index]).clone())
                });
            let techno_ctor_random_word = self.resolve_techno_constructor_word(
                constructor_init,
                generated_inits.as_ref().map(|_| {
                    (
                        entity_index,
                        map_ent.type_id.as_str(),
                        (map_ent.cell_x, map_ent.cell_y),
                    )
                }),
            )?;
            if generated_inits.is_none() && self.native_unique_ids.is_some() {
                // Authored map readers construct each Techno after consuming
                // its unconditional Scenario word. The class-specific
                // constructor then assigns the shared native identity before
                // Unlimbo can succeed or fail.
                let _ = self
                    .next_native_load_id()
                    .expect("authored Techno native cursor was checked above");
            }

            // Build the GameEntity with all required fields.
            let mut ge = GameEntity::new_at_frame_from_constructor_word(
                stable_id,
                map_ent.cell_x,
                map_ent.cell_y,
                z,
                map_ent.facing,
                owner_id,
                health,
                type_id,
                map_ent.category,
                map_ent.veterancy,
                sight_range,
                uses_voxel,
                self.session.binary_frame,
                techno_ctor_random_word,
            );
            ge.base_defense_response.recruitable_a = map_ent.recruitable_a;
            ge.base_defense_response.recruitable_b = map_ent.recruitable_b;

            self.install_techno_components(
                &mut ge,
                rules.and_then(|rules| {
                    authored_health::map_object_type(map_ent.category, &map_ent.type_id, rules)
                }),
                rules,
                construction::ComponentOrigin::Authored {
                    sub_cell: map_ent.sub_cell,
                    bridge_deck: bridge_spawn,
                },
            );
            let (stable_id, outcome) =
                self.unlimbo_authored_techno(ge, map_ent.health, rules, overlay_registry);
            if !matches!(outcome, RevealOutcome::Revealed { .. }) {
                self.discard_constructed_limbo(stable_id);
                continue;
            }
            if let Some(ruleset) = rules {
                self.initialize_cloak_after_unlimbo(stable_id, ruleset);
                self.add_unit_sensor_after_unlimbo(stable_id, ruleset);
                self.add_building_sensor_array_if_powered(stable_id, ruleset);
            }
            self.commit_map_placement_mission(stable_id, map_ent.mission);
            if let Some(rules) = rules {
                self.finish_authored_building_enable(stable_id, rules);
            }
            count += 1;

            if map_ent.category == EntityCategory::Structure
                && let Some(ruleset) = rules
            {
                for (slot, upgrade_type) in map_ent.structure_upgrades.iter().enumerate() {
                    let Some(upgrade_type) = upgrade_type.as_deref() else {
                        continue;
                    };
                    let valid_upgrade = ruleset
                        .object(upgrade_type)
                        .is_some_and(|object| object.category == ObjectCategory::Building);
                    if !valid_upgrade {
                        log::warn!(
                            "Skipping unresolved authored upgrade {} in slot {} on {}",
                            upgrade_type,
                            slot,
                            map_ent.type_id
                        );
                        continue;
                    }
                    if self
                        .spawn_attached_map_upgrade(
                            stable_id,
                            slot as u8,
                            upgrade_type,
                            &map_ent.owner,
                            map_ent.cell_x,
                            map_ent.cell_y,
                            z,
                            map_ent.facing,
                            ruleset,
                            generated_inits.is_none(),
                        )
                        .is_some()
                    {
                        count += 1;
                    }
                }
            }
        }

        log::info!("Spawned {} entities", count);
        Ok(count)
    }

    fn map_entity_resolves_before_constructor(
        &self,
        map_ent: &MapEntity,
        rules: Option<&RuleSet>,
        require_owner: bool,
    ) -> bool {
        let type_resolves = rules.is_none_or(|rules| {
            authored_health::map_object_type(map_ent.category, &map_ent.type_id, rules).is_some()
        });
        let owner_resolves = (!require_owner && self.houses.is_empty())
            || crate::sim::house_state::house_state_for_owner(
                &self.houses,
                &map_ent.owner,
                &self.interner,
            )
            .is_some();
        type_resolves && owner_resolves
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_attached_map_upgrade(
        &mut self,
        parent_stable_id: u64,
        slot: u8,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        z: u8,
        facing: u8,
        rules: &RuleSet,
        consume_native_load_id: bool,
    ) -> Option<u64> {
        let stable_id =
            self.construct_object_limbo_at_height(type_id, owner, rx, ry, facing, z, rules)?;
        if consume_native_load_id && self.native_unique_ids.is_some() {
            let _ = self
                .next_native_load_id()
                .expect("authored upgrade native cursor was checked above");
        }
        {
            let upgrade = self.substrate.entities.get_mut(stable_id)?;
            upgrade.structure_upgrade_link = Some(StructureUpgradeLink {
                parent_stable_id,
                slot,
            });
        }
        if self
            .reveal_constructed_object_at_height(
                stable_id,
                rx,
                ry,
                facing,
                z,
                PlacementEvidence::AttachedUpgrade,
                rules,
            )
            .is_none()
        {
            self.discard_constructed_limbo(stable_id);
            return None;
        }
        Some(stable_id)
    }

    /// Commit the `MISSION=` column a map placement authored, at the position
    /// the scenario reader does it: immediately after the object is unlimboed
    /// onto the map.
    ///
    /// Retail runs `Queue_Mission(<name>, 0)` and then its readiness check plus
    /// `Commence` on the same line, and `Queue` refuses the `-1` sentinel — so
    /// an absent or unrecognised name leaves the object on the mission it was
    /// born with. Because the queue slot is empty at birth, `Commence`'s field
    /// writes are exactly `Assign`'s, which is the verb used here; the residual
    /// is that retail's Unit path can leave the promotion one tick late when
    /// its readiness predicate says no, and this cannot.
    ///
    /// Dispatchable miners keep the Harvest their Unlimbo idle mode gave them —
    /// the native creation-mission family is its own recorded UNCHECKED and the
    /// harvest FSM needs a truthful `current` from birth.
    fn commit_map_placement_mission(
        &mut self,
        stable_id: u64,
        authored: Option<crate::sim::mission::MissionType>,
    ) {
        if self.is_dispatchable_miner(stable_id) {
            return;
        }
        let Some(mission) = authored else {
            return;
        };
        let now = self.session.binary_frame;
        let _ = self.mission_assign_exact(
            stable_id,
            crate::sim::mission::MissionId::from_known(mission),
            now,
        );
    }

    /// A miner the harvest dispatch drives (not a Slave Miner): Unlimbo's idle
    /// mode already gave it Harvest (`UnitClass::Enter_Idle_Mode @ 0x00738970`,
    /// harvester arm `0x00738BD8`, owned by `foot_unlimbo_idle_mode`), which a
    /// map placement keeps.
    fn is_dispatchable_miner(&self, stable_id: u64) -> bool {
        self.substrate
            .entities
            .get(stable_id)
            .and_then(|e| e.miner.as_ref())
            .is_some_and(|m| m.kind != crate::sim::miner::MinerKind::Slave)
    }

    /// Spawn one object instance (used by production). Returns the stable_id on success.
    pub fn spawn_object(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        rules: &RuleSet,
        height_map: &BTreeMap<(u16, u16), u8>,
    ) -> Option<u64> {
        let z: u8 = height_map.get(&(rx, ry)).copied().unwrap_or(0);
        self.spawn_object_at_height(type_id, owner, rx, ry, facing, z, rules)
    }

    /// Immediate construction with the match's live OverlayTypeClass table.
    /// Production callers use this boundary whenever the constructed type may
    /// be a Unit, so its virtual Unlimbo sees the same overlay facts as native.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_object_with_overlay_registry(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        rules: &RuleSet,
        height_map: &BTreeMap<(u16, u16), u8>,
        overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    ) -> Option<u64> {
        let z = height_map.get(&(rx, ry)).copied().unwrap_or(0);
        self.spawn_object_at_height_with_overlay_registry(
            type_id,
            owner,
            rx,
            ry,
            facing,
            z,
            rules,
            overlay_registry,
        )
    }

    pub(crate) fn spawn_object_at_height(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
    ) -> Option<u64> {
        self.spawn_object_at_height_with_overlay_context(
            type_id, owner, rx, ry, facing, z, rules, None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_object_at_height_with_overlay_context(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Option<u64> {
        self.spawn_object_at_height_with_init(
            type_id,
            owner,
            rx,
            ry,
            facing,
            z,
            rules,
            overlay_registry,
            TechnoConstructorInit::FreshScenario,
        )
        .expect("fresh Techno constructor initialization cannot fail")
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_object_at_height_with_overlay_registry(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
        overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    ) -> Option<u64> {
        self.spawn_object_at_height_with_overlay_context(
            type_id,
            owner,
            rx,
            ry,
            facing,
            z,
            rules,
            Some(overlay_registry),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_object_at_height_with_init(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        init: TechnoConstructorInit,
    ) -> Result<Option<u64>, GeneratedTechnoInitError> {
        let Some(ge) =
            self.construct_runtime_techno(type_id, owner, rx, ry, facing, z, rules, init)?
        else {
            return Ok(None);
        };
        let (stable_id, outcome) =
            self.unlimbo_after_constructor_managers(ge, Some(rules), overlay_registry);
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            // This convenience path owns its transient constructor result.
            // Held production objects use the separate limbo/retry boundary.
            self.discard_constructed_limbo(stable_id);
            return Ok(None);
        }
        self.initialize_cloak_after_unlimbo(stable_id, rules);
        self.add_unit_sensor_after_unlimbo(stable_id, rules);
        Ok(Some(stable_id))
    }

    /// Create an object in limbo: stored in EntityStore and tracked by its
    /// house (Add_Tracking), but not registered in map occupancy. Used by
    /// paradrop cargo loading, where gamemd creates passengers directly into
    /// CargoClass without Unlimbo.
    pub(crate) fn spawn_object_limbo_at_height(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
    ) -> Option<u64> {
        self.spawn_object_limbo_at_height_with_init(
            type_id,
            owner,
            rx,
            ry,
            facing,
            z,
            rules,
            TechnoConstructorInit::FreshScenario,
        )
        .expect("fresh Techno constructor initialization cannot fail")
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_object_limbo_at_height_with_init(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
        init: TechnoConstructorInit,
    ) -> Result<Option<u64>, GeneratedTechnoInitError> {
        let Some(ge) =
            self.construct_runtime_techno(type_id, owner, rx, ry, facing, z, rules, init)?
        else {
            return Ok(None);
        };

        let stable_id = self.create_limbo(ge);
        self.commit_constructor_owned_techno_children(stable_id, rules);
        self.register_house_base_building(stable_id, rules);
        Ok(Some(stable_id))
    }

    /// Compatibility name for constructing and accounting the Techno identity
    /// held from `FactoryClass::StartProduction @ 0x004C9C70` through delivery.
    /// Production and other limbo constructors share the same manager/child
    /// transaction; later delivery only Unlimbos this retained identity.
    #[cfg(test)]
    pub(crate) fn create_production_object_limbo_at_height(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
    ) -> Option<u64> {
        self.construct_object_limbo_at_height(type_id, owner, rx, ry, facing, z, rules)
    }

    /// Construct one fully initialized runtime Techno and retain it in limbo
    /// for one or more placement attempts. Starting-force creation and held
    /// production both use this identity-preserving boundary.
    pub(crate) fn construct_object_limbo_at_height(
        &mut self,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        rules: &RuleSet,
    ) -> Option<u64> {
        self.spawn_object_limbo_at_height(type_id, owner, rx, ry, facing, z, rules)
    }

    /// Run one result-bearing Unlimbo transaction against an already stored
    /// production object. Mark failure restores this same identity to limbo;
    /// construction and its Add_Tracking are deliberately not repeated.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn unlimbo_held_production_object(
        &mut self,
        stable_id: u64,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        placement: PlacementEvidence,
        rules: &RuleSet,
    ) -> Option<u64> {
        self.reveal_constructed_object_at_height_with_unit_context(
            stable_id, rx, ry, facing, z, placement, rules, None, stable_id,
        )
    }

    /// Production delivery boundary for a held object whose concrete Unit
    /// virtual needs the live overlay table and selected producer identity.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn unlimbo_held_production_object_with_unit_context(
        &mut self,
        stable_id: u64,
        producer_id: u64,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        placement: PlacementEvidence,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Option<u64> {
        self.reveal_constructed_object_at_height_with_unit_context(
            stable_id,
            rx,
            ry,
            facing,
            z,
            placement,
            rules,
            overlay_registry,
            producer_id,
        )
    }

    /// Place an already constructed limbo Techno without repeating its
    /// constructor draw or manager initialization. Failure restores this same
    /// identity to limbo so a caller may try another coordinate.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reveal_constructed_object_at_height(
        &mut self,
        stable_id: u64,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        placement: PlacementEvidence,
        rules: &RuleSet,
    ) -> Option<u64> {
        self.reveal_constructed_object_at_height_with_unit_context(
            stable_id, rx, ry, facing, z, placement, rules, None, stable_id,
        )
    }

    /// Common concrete Unlimbo boundary. A Unit arriving with ordinary
    /// `EvaluateMark` first executes its exact `+0x1AC` CanEnter predicate;
    /// callers that already proved exact zero carry that evidence instead.
    /// Rejection returns before facing, subcell, bridge, or position mutation.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reveal_constructed_object_at_height_with_unit_context(
        &mut self,
        stable_id: u64,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        placement: PlacementEvidence,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        producer_id: u64,
    ) -> Option<u64> {
        let requested_position = RevealPosition {
            rx,
            ry,
            z,
            sub_x: self.substrate.entities.get(stable_id)?.position.sub_x,
            sub_y: self.substrate.entities.get(stable_id)?.position.sub_y,
        };
        let placement = if placement == PlacementEvidence::EvaluateMark {
            self.constructor_unlimbo_placement(
                stable_id,
                requested_position,
                rules,
                overlay_registry,
                producer_id,
            )
        } else {
            placement
        };
        if placement == PlacementEvidence::RejectedEarly {
            let _ = self.try_reveal_entity(
                stable_id,
                RevealRequest {
                    position: requested_position,
                    placement,
                    logic_eligible: true,
                },
            );
            return None;
        }

        let z = match placement {
            PlacementEvidence::UnitCanEnterExactZero {
                layer: MovementLayer::Bridge,
            } => self
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(rx, ry))
                .map_or(z, |cell| cell.bridge_deck_level),
            _ => z,
        };
        if let PlacementEvidence::UnitCanEnterExactZero { layer } = placement
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.on_bridge = layer == MovementLayer::Bridge;
        }
        let is_infantry = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Infantry);
        let infantry_sub_cell = is_infantry.then(|| self.allocate_infantry_sub_cell(rx, ry));
        let (sub_x, sub_y) = {
            let entity = self.substrate.entities.get_mut(stable_id)?;
            entity.facing = facing;
            if let Some(sub_cell) = infantry_sub_cell {
                entity.sub_cell = Some(sub_cell);
                let offsets = crate::util::lepton::subcell_lepton_offset(Some(sub_cell));
                entity.position.sub_x = offsets.0;
                entity.position.sub_y = offsets.1;
            }
            (entity.position.sub_x, entity.position.sub_y)
        };
        let outcome = self.try_reveal_entity_with_context(
            stable_id,
            RevealRequest {
                position: RevealPosition {
                    rx,
                    ry,
                    z,
                    sub_x,
                    sub_y,
                },
                placement,
                logic_eligible: true,
            },
            super::lifecycle::UninitContext::with_rules(rules),
        );
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            return None;
        }
        self.allocate_building_light(stable_id, rules);
        self.initialize_cloak_after_unlimbo(stable_id, rules);
        self.add_unit_sensor_after_unlimbo(stable_id, rules);
        Some(stable_id)
    }

    /// Store a freshly constructed object in native-style limbo and account for
    /// its owner. Placement is a separate, result-bearing Reveal transaction.
    fn store_spawned_limbo(&mut self, mut ge: GameEntity) -> u64 {
        let stable_id = ge.stable_id();

        // This boundary receives newly constructed objects. Make those constructor
        // facts explicit so storage can never imply cell or logic presence.
        ge.lifecycle.object_alive = true;
        ge.lifecycle.in_limbo = true;
        ge.lifecycle.cell_marked = false;
        ge.in_logic_vector = false;
        ge.destruction_recorded = false;

        self.substrate.entities.insert(ge);
        // The class constructors and InitFromType call Add_Tracking.
        self.update_house_tracking(
            stable_id,
            crate::sim::house_tracking::HouseTracking::add_tracking,
        );
        stable_id
    }

    /// Delete a constructor-complete object that never successfully left
    /// limbo. The stable ID and constructor RNG draw stay spent, while the
    /// transient store and its tracking are undone exactly once (the
    /// destructor's Remove_Tracking).
    pub(crate) fn discard_constructed_limbo(&mut self, stable_id: u64) -> bool {
        if !self.substrate.entities.contains(stable_id) {
            return false;
        }
        self.release_house_base_tracking(stable_id);
        let spawn_children = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.spawn_manager.as_ref())
            .map(|manager| {
                manager
                    .slots
                    .iter()
                    .filter_map(|slot| slot.spawn)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let slave_children = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.slave_manager.as_ref())
            .map(|manager| manager.slaves().collect::<Vec<_>>())
            .unwrap_or_default();
        self.update_house_tracking(
            stable_id,
            crate::sim::house_tracking::HouseTracking::remove_tracking,
        );
        let entity = self
            .substrate
            .entities
            .remove(stable_id)
            .expect("constructor object existence checked above");
        debug_assert!(entity.lifecycle.in_limbo && !entity.lifecycle.cell_marked);
        for child_id in spawn_children.into_iter().chain(slave_children) {
            if self.substrate.entities.contains(child_id) {
                let discarded = self.discard_constructed_limbo(child_id);
                debug_assert!(discarded, "constructor-owned child must remain in limbo");
            }
        }
        true
    }

    /// Spawn an object directly into limbo: stored in EntityStore and tracked by
    /// its house (Add_Tracking) but NOT registered in the active order or map
    /// occupancy. Registration happens later at reveal/landing (e.g. paradrop
    /// drop). Returns the stable id.
    pub(crate) fn create_limbo(&mut self, ge: GameEntity) -> u64 {
        self.store_spawned_limbo(ge)
    }

    /// Store a new object, then place it through active
    /// `ObjectClass::Reveal @ 0x005F4EC0`: coordinates commit, the mode-one
    /// playfield gate and Mark(PUT) own the result, and eligible logic
    /// registration happens last. A failed attempt keeps the constructed
    /// identity in limbo for its caller to retain or discard.
    #[cfg(test)]
    pub(crate) fn unlimbo(&mut self, ge: GameEntity) -> (u64, RevealOutcome) {
        let position = RevealPosition {
            rx: ge.position.rx,
            ry: ge.position.ry,
            z: ge.position.z,
            sub_x: ge.position.sub_x,
            sub_y: ge.position.sub_y,
        };
        let stable_id = self.store_spawned_limbo(ge);
        let outcome = self.try_reveal_entity(
            stable_id,
            RevealRequest {
                position,
                placement: PlacementEvidence::EvaluateMark,
                logic_eligible: true,
            },
        );
        (stable_id, outcome)
    }

    /// Complete `TechnoClass::Init_Managers @ 0x006F3F40` while the freshly
    /// constructed parent is still in limbo, then attempt the parent's first
    /// Unlimbo. Both SpawnManagerClass @ 0x006B6C90 and SlaveManagerClass
    /// @ 0x006AF1A0 construct their child Technos before parent placement.
    fn unlimbo_after_constructor_managers(
        &mut self,
        ge: GameEntity,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> (u64, RevealOutcome) {
        let (stable_id, position) = self.store_with_constructor_managers(ge, rules);
        self.unlimbo_constructed_parent(stable_id, position, rules, overlay_registry)
    }

    fn store_with_constructor_managers(
        &mut self,
        ge: GameEntity,
        rules: Option<&RuleSet>,
    ) -> (u64, RevealPosition) {
        let position = RevealPosition {
            rx: ge.position.rx,
            ry: ge.position.ry,
            z: ge.position.z,
            sub_x: ge.position.sub_x,
            sub_y: ge.position.sub_y,
        };
        let stable_id = self.store_spawned_limbo(ge);
        if let Some(rules) = rules {
            self.commit_constructor_owned_techno_children(stable_id, rules);
            self.register_house_base_building(stable_id, rules);
        }
        (stable_id, position)
    }

    fn unlimbo_constructed_parent(
        &mut self,
        stable_id: u64,
        mut position: RevealPosition,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> (u64, RevealOutcome) {
        let placement = rules.map_or(PlacementEvidence::EvaluateMark, |rules| {
            self.constructor_unlimbo_placement(
                stable_id,
                position,
                rules,
                overlay_registry,
                stable_id,
            )
        });
        if let PlacementEvidence::UnitCanEnterExactZero { layer } = placement {
            if layer == MovementLayer::Bridge {
                position.z = self
                    .resolved_terrain
                    .as_ref()
                    .and_then(|terrain| terrain.cell(position.rx, position.ry))
                    .map_or(position.z, |cell| cell.bridge_deck_level);
            }
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.on_bridge = layer == MovementLayer::Bridge;
            }
        }
        let outcome = self.try_reveal_entity_with_context(
            stable_id,
            RevealRequest {
                position,
                placement,
                logic_eligible: true,
            },
            rules.map_or_else(
                super::lifecycle::UninitContext::default,
                super::lifecycle::UninitContext::with_rules,
            ),
        );
        if matches!(outcome, RevealOutcome::Revealed { .. }) {
            if let Some(rules) = rules {
                self.allocate_building_light(stable_id, rules);
                self.initialize_completed_building_anims(stable_id, rules);
            }
        }
        (stable_id, outcome)
    }

    /// Collapse the concrete Techno `+0x1AC` return code at the same boundary
    /// as `ObjectClass::Unlimbo @ 0x005F4F1B..0x005F4F49`: exact zero admits,
    /// every nonzero code rejects before any object mutation. UnitClass owns
    /// the first active constructor specialization promoted here; the shared
    /// evaluator is the same `(cell,-1,-1,0,0)` body used by production.
    fn constructor_unlimbo_placement(
        &self,
        stable_id: u64,
        position: RevealPosition,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        producer_id: u64,
    ) -> PlacementEvidence {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return PlacementEvidence::RejectedEarly;
        };
        // Active scenario construction installs its CellClass grid before any
        // Techno can reach Unlimbo. Terrain-less diagnostic/unit-test harnesses
        // remain on the generic Mark seam because no native UnitClass cell input
        // exists there to evaluate.
        if entity.category != EntityCategory::Unit || self.resolved_terrain.is_none() {
            return PlacementEvidence::EvaluateMark;
        }
        let owner = self.interner.resolve(entity.owner()).to_string();
        let type_id = self.interner.resolve(entity.type_ref()).to_string();
        let admission = crate::sim::production::produced_unit_unlimbo_entry_at_resolved_cell(
            self,
            rules,
            &owner,
            &type_id,
            stable_id,
            producer_id,
            (position.rx, position.ry),
            overlay_registry,
        );
        let Some(layer) = admission.exact_zero_layer() else {
            return PlacementEvidence::RejectedEarly;
        };
        PlacementEvidence::UnitCanEnterExactZero { layer }
    }

    /// Materialize constructor-owned Technos in native manager order. The
    /// parent has already consumed its own constructor word; every child uses
    /// the same FreshScenario funnel and remains a stable limbo identity.
    fn commit_constructor_owned_techno_children(&mut self, parent_id: u64, rules: &RuleSet) {
        crate::sim::spawn_manager::commit_spawn_manager_pool(self, parent_id, rules);

        if self
            .substrate
            .entities
            .get(parent_id)
            .is_some_and(|parent| parent.slave_manager.is_some())
        {
            return;
        }
        self.create_slave_manager(parent_id, rules);
    }

    /// Unit/Infantry constructor cloak ability plus UnitClass::Unlimbo's
    /// exceptional state-2 establishment. Active evidence: constructor copy at
    /// 0x007355B6/0x00517D88 and UnitClass::Unlimbo @ 0x00737BA0.
    fn initialize_cloak_after_unlimbo(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((category, veterancy, in_playfield, type_ref)) =
            self.substrate.entities.get(stable_id).map(|entity| {
                (
                    entity.category,
                    entity.veterancy,
                    entity.in_playfield,
                    entity.type_ref(),
                )
            })
        else {
            return;
        };
        if !matches!(category, EntityCategory::Unit | EntityCategory::Infantry) {
            return;
        }
        let Some(object) = rules.object(self.interner.resolve(type_ref)) else {
            return;
        };
        let rank_cloak =
            veterancy >= 100 && object.veteran_cloak || veterancy >= 200 && object.elite_cloak;
        if !object.cloakable && !rank_cloak {
            return;
        }
        let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(
            self.session.binary_frame as i32,
            rules.general.cloaking_stages,
        );
        // Only UnitClass owns the direct Unlimbo state write, and it tests the
        // copied runtime Cloakable byte rather than rank-granted CLOAK.
        if category == EntityCategory::Unit && object.cloakable && !in_playfield {
            cloak.establish_unlimbo_fully_cloaked();
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.cloak = Some(cloak);
        }
    }

    /// Update VoxelAnimation frame_counts for all voxel entities from atlas data.
    ///
    /// Called after the unit atlas is built, since frame counts are only known after
    /// loading HVA files.
    pub fn update_voxel_anim_frame_counts(
        &mut self,
        frame_counts: &std::collections::BTreeMap<(String, crate::sim::components::VxlLayer), u32>,
    ) {
        use crate::sim::components::VxlLayer;

        let keys = self.substrate.entities.keys_sorted();
        let mut updated: u32 = 0;
        for &sid in &keys {
            let Some(entity) = self.substrate.entities.get_mut(sid) else {
                continue;
            };
            let type_ref = entity.type_ref();
            let Some(ref mut va) = entity.voxel_animation else {
                continue;
            };
            let max_fc: u32 = [
                VxlLayer::Composite,
                VxlLayer::Body,
                VxlLayer::Turret,
                VxlLayer::Barrel,
            ]
            .iter()
            .filter_map(|layer| {
                frame_counts.get(&(self.interner.resolve(type_ref).to_string(), *layer))
            })
            .copied()
            .max()
            .unwrap_or(1);

            if max_fc > 1 && va.frame_count != max_fc {
                va.frame_count = max_fc;
                updated += 1;
            }
        }
        if updated > 0 {
            log::info!(
                "Updated VoxelAnimation frame_count for {} entities",
                updated
            );
        }
    }

    /// Deploy an MCV entity: despawn it and spawn a construction yard in its place.
    /// Checks that the footprint area is free of other structures and passable terrain
    /// before deploying. Returns false if deployment is blocked.
    pub(crate) fn deploy_mcv(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        _height_map: &BTreeMap<(u16, u16), u8>,
    ) -> bool {
        // Native early navigation/movement exits (0x7393E4/0x73940A) retain
        // runtime +0x68C. They must not attempt placement while stopping.
        let Some(source) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        if source.dying || source.lifecycle.in_limbo {
            return false;
        }
        // Deploy asks CanDeploySlashUnload (vslot +0x314, 0x00700D50) first
        // (0x007393CC); its DeploysInto arm refuses a unit a parasite is
        // eating (0x00700EB4..0x00700EBC) and, when DeploysInto is a
        // `ConstructionYard=` building, a mind-controlled one (+0x2C0,
        // 0x00700EC6..0x00700ED8), and that refusal clears Unit+0x68C
        // (0x00739AA7). The predicate's other arms are not represented here.
        let stolen_mcv = source.mind_control.is_mind_controlled()
            && construction_yard_type_for_mcv(self.interner.resolve(source.type_ref()), rules)
                .and_then(|yard| rules.object(&yard))
                .is_some_and(|yard| yard.construction_yard);
        if source.parasite_eating_me.is_some() || stolen_mcv {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.mcv_deploy_pending = false;
            }
            return false;
        }
        if source.navigation.nav_com.is_some()
            || crate::sim::movement::ready_producer::is_moving_for_unit_shp_draw(source)
        {
            return false;
        }
        // Read deploy data from EntityStore before mutating.
        let deploy_data = self.substrate.entities.get(stable_id).and_then(|entity| {
            let type_str = self.interner.resolve(entity.type_ref());
            let yard_type = construction_yard_type_for_mcv(type_str, rules)?;
            let yard_obj = rules.object(&yard_type)?;
            let (spawn_rx, spawn_ry) = deploy_origin_from_unit_cell(
                entity.position.rx,
                entity.position.ry,
                &yard_obj.foundation,
            );
            Some((
                entity.owner(),
                spawn_rx,
                spawn_ry,
                entity.position.z,
                yard_type.clone(),
                yard_obj.deploy_facing,
                entity.selected,
                yard_obj.foundation.clone(),
                crate::sim::mcv_deploy::current_direction(entity, self.session.binary_frame),
                yard_obj.construction_yard,
                rules
                    .object(type_str)
                    .and_then(|obj| obj.deploy_sound.clone())
                    .filter(|sound| !sound.is_empty())
                    .map(|sound| (sound, entity.position.rx, entity.position.ry)),
                rules
                    .object(type_str)
                    .is_some_and(|obj| obj.resource_gatherer),
            ))
        });
        let Some((
            owner_id,
            rx,
            ry,
            z,
            yard_type,
            deploy_facing,
            was_selected,
            foundation,
            source_facing,
            is_construction_yard,
            deploy_cue,
            resource_gatherer,
        )) = deploy_data
        else {
            return false;
        };

        // Check that all footprint cells are free before deploying.
        //
        // `UnitClass::Deploy @ 0x007393C0` speaks `EVA_CannotDeployHere`
        // (`0x0073950A`) only when the owner is a human player AND
        // `Type+0x5EC` (`ResourceGatherer=`, `ReadINI 0x007143E4`) is clear
        // (`0x007394EB..0x0073950A`): a blocked Slave Miner (`[SMIN]`, the
        // one stock ResourceGatherer with DeploysInto) stays silent. The
        // event's owner carries the human half to presentation.
        let (fw, fh) = foundation_dimensions(&foundation);
        for dy in 0..fh {
            for dx in 0..fw {
                let cell_x = rx.saturating_add(dx);
                let cell_y = ry.saturating_add(dy);
                // Check for existing structures (excluding the MCV itself).
                let occupied = self.substrate.entities.values().any(|e| {
                    // A Dying structure corpse (sold/destroyed earlier in this
                    // command batch) no longer blocks an MCV deploy footprint.
                    if e.dying
                        || e.lifecycle.in_limbo
                        || e.stable_id() == stable_id
                        || e.category != EntityCategory::Structure
                    {
                        return false;
                    }
                    let Some(existing) = self.object_type(e.type_ref(), rules) else {
                        return false;
                    };
                    if existing.wall {
                        return false;
                    }
                    let (ew, eh) = foundation_dimensions(&existing.foundation);
                    cell_x >= e.position.rx
                        && cell_x < e.position.rx.saturating_add(ew)
                        && cell_y >= e.position.ry
                        && cell_y < e.position.ry.saturating_add(eh)
                });
                if occupied {
                    log::info!("MCV deploy blocked: structure at ({},{})", cell_x, cell_y,);
                    if !resource_gatherer {
                        self.sound_events
                            .push(SimSoundEvent::CannotDeployHere { owner: owner_id });
                    }
                    self.substrate
                        .entities
                        .get_mut(stable_id)
                        .unwrap()
                        .mcv_deploy_pending = false;
                    crate::sim::mcv_deploy::queue_guard(self, stable_id);
                    return false;
                }
                // Check terrain build-blocked.
                if self
                    .effective_build_blocked(cell_x, cell_y)
                    .unwrap_or(false)
                {
                    log::info!("MCV deploy blocked: terrain at ({},{})", cell_x, cell_y,);
                    if !resource_gatherer {
                        self.sound_events
                            .push(SimSoundEvent::CannotDeployHere { owner: owner_id });
                    }
                    self.substrate
                        .entities
                        .get_mut(stable_id)
                        .unwrap()
                        .mcv_deploy_pending = false;
                    crate::sim::mcv_deploy::queue_guard(self, stable_id);
                    return false;
                }
            }
        }

        if source_facing != deploy_facing {
            let now = self.session.binary_frame;
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                if !crate::sim::movement::ready_producer::is_moving_now_for(entity, now) {
                    crate::sim::mcv_deploy::start_turn(entity, deploy_facing, now);
                }
                entity.mcv_deploy_pending = true;
            }
            return true;
        }

        // Failure in the aligned construction transaction clears +0x68C
        // (0x739AA7); a successful transaction removes the source entirely.
        self.substrate
            .entities
            .get_mut(stable_id)
            .unwrap()
            .mcv_deploy_pending = false;
        // Native successful deploy transaction:
        // `UnitClass__Deploy @ 0x007393C0`, block `0x00739855..0x00739926`,
        // calls `FUN_00505180 @ 0x00505180` only for a non-controlled
        // ConstructionYard in a nonzero game mode. VERA preflights only the
        // directly indexed Recalc vectors before the destructive MCV removal.
        let recalc_context = self.houses.get(&owner_id).and_then(|house| {
            (is_construction_yard
                && self.session.game_mode_nonzero
                && !house.is_controlled_by_human(true))
            .then(|| {
                let country_name = house
                    .country
                    .map(|country| self.interner.resolve(country).to_owned());
                (
                    country_name,
                    house.side_index,
                    house.difficulty,
                    house.tech_level,
                    house.base_plan.nodes.is_empty(),
                )
            })
        });
        if let Some((country_name, side_index, difficulty, _, true)) = &recalc_context {
            let Some(country_name) = country_name.as_deref() else {
                return false;
            };
            if preflight_recalc(rules, country_name, *side_index, *difficulty).is_err() {
                return false;
            }
        }

        // Native73992B..953 scales actual HP by live source/destination type
        // Strength and resets Object+70; signed storage retains the full result.
        let Some(converted_health) = self.substrate.entities.get(stable_id).and_then(|source| {
            Some(crate::sim::conversion_health::ConversionHealth::capture(
                source,
                self.object_type(source.type_ref(), rules)?,
                rules.object(&yard_type)?,
                crate::sim::conversion_health::ConversionKind::Unit,
            ))
        }) else {
            return false;
        };

        // Native reaches the bounded post-deploy transaction only after the
        // target Building was created successfully. Keep the source MCV live
        // until target Unlimbo commits so a late placement rejection is atomic.
        let owner_str = self.interner.resolve(owner_id).to_string();
        let Some(mut destination) = self
            .construct_runtime_techno(
                &yard_type,
                &owner_str,
                rx,
                ry,
                0,
                z,
                rules,
                TechnoConstructorInit::FreshScenario,
            )
            .expect("fresh Techno constructor initialization cannot fail")
        else {
            return false;
        };
        // Conversion and construction admission precede Unlimbo's completed-
        // slot initialization. Otherwise full-health animations consume IDs/RNG
        // here and ActuallyPlaced suppresses the real completion callback.
        converted_health.apply(&mut destination);
        destination.selected = was_selected;
        destination.building_up = Some(BuildingUp {
            elapsed_ticks: 0,
            total_ticks: 30,
        });
        let (new_sid, outcome) =
            self.unlimbo_after_constructor_managers(destination, Some(rules), None);
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            self.discard_constructed_limbo(new_sid);
            return false;
        }
        self.initialize_cloak_after_unlimbo(new_sid, rules);
        self.add_unit_sensor_after_unlimbo(new_sid, rules);
        self.mission_spawned_entities = true;
        // 0x00739956: a Slave Miner's manager moves to its refinery (the
        // hand-off 0x006B0D10, then SetOwner 0x006AF580) before the unit
        // leaves; the refinery's own fresh slaves are freed.
        self.transfer_slave_manager(stable_id, new_sid, true, rules, None);
        self.uninit_with_rules(stable_id, rules);

        if let Some((country_name, side_index, difficulty, tech_level, _)) = recalc_context {
            // The new Building's committed north-west anchor is the native
            // `+0x9C/+0xA0` source. This bounded write order is load-bearing:
            // primary center, optional Recalc, node zero, BasePlan center,
            // then the three independent House AI activation latches.
            let house = self
                .houses
                .get_mut(&owner_id)
                .expect("qualifying deploy owner remains registered");
            house.base_center = Some((rx, ry));
            if house.base_plan.nodes.is_empty() {
                recalc_base_plan(
                    &mut house.base_plan,
                    rules,
                    country_name
                        .as_deref()
                        .expect("empty qualifying plan was preflighted with a country"),
                    side_index,
                    difficulty,
                    tech_level,
                    self.session.game_options.super_weapons,
                    &mut self.scenario_rng,
                );
            }
            if let Some(node_zero) = house.base_plan.nodes.first_mut() {
                node_zero.packed_cell = pack_base_plan_cell(i32::from(rx), i32::from(ry));
            }
            house.base_plan_center = (rx, ry);
            house.enable_ai_deploy_latches();
        }

        // UnitClass::Deploy 0x00739A19..0x00739A54 plays Type+0x56C
        // (DeploySound) at the source MCV's location, only after the yard's
        // Unlimbo succeeded. This accompanies build-up; it is not a sound on
        // the turn request or an animation-completion cue. Stock MCVs use
        // PlaceBuilding -> uplace (RULESMD.INI / SOUNDMD.INI).
        if let Some((sound, rx, ry)) = deploy_cue {
            let deploy_sound_id = self.interner.intern(&sound);
            self.sound_events.push(SimSoundEvent::EntityDeployed {
                deploy_sound_id,
                rx,
                ry,
            });
        }

        true
    }

    /// Undeploy a structure back into its mobile unit (e.g. ConYard → MCV).
    /// Reads `UndeploysInto` from rules.ini to determine the spawned unit type.
    /// Starts a reverse build-up animation (`BuildingDown`); the actual unit
    /// spawn happens when the animation completes (see `tick_building_down`).
    ///
    /// The start is `BuildingClass::Sell`'s first UndeploysInto visit, which
    /// plays the building type's `DeploySound=` (`+0x56C`) at its Location
    /// (`0x0044A9E5..0x0044AA38`, after the voice `vt+0x36C` VERA does not
    /// play).
    pub(crate) fn undeploy_building(&mut self, stable_id: u64, rules: &RuleSet) -> bool {
        // Read undeploy data before mutating.
        let undeploy_data = self.substrate.entities.get(stable_id).and_then(|entity| {
            if !self.can_undeploy_building_runtime(stable_id, rules) {
                return None;
            }
            let type_str = self.interner.resolve(entity.type_ref());
            let unit_type = undeploy_target_for_building(type_str, rules)?;
            let obj = rules.object(type_str)?;
            let (center_rx, center_ry) =
                undeploy_unit_cell(entity.position.rx, entity.position.ry, &obj.foundation);
            Some((
                entity.owner(),
                center_rx,
                center_ry,
                entity.position.z,
                unit_type,
                entity.selected,
            ))
        });
        let Some((owner_id, rx, ry, z, unit_type, was_selected)) = undeploy_data else {
            return false;
        };
        let sound = self.substrate.entities.get(stable_id).and_then(|entity| {
            let sound = self
                .object_type(entity.type_ref(), rules)?
                .deploy_sound
                .clone()?;
            Some((sound, entity.position.rx, entity.position.ry))
        });
        if let Some((sound, sound_rx, sound_ry)) = sound {
            let deploy_sound_id = self.interner.intern(&sound);
            self.sound_events.push(SimSoundEvent::EntityDeployed {
                deploy_sound_id,
                rx: sound_rx,
                ry: sound_ry,
            });
        }

        // Start the reverse build-up animation instead of instant despawn.
        let unit_type_id = self.interner.intern(&unit_type);
        if let Some(ge) = self.substrate.entities.get_mut(stable_id) {
            ge.building_down = Some(BuildingDown {
                elapsed_ticks: 0,
                total_ticks: 30,
                spawn_type: unit_type_id,
                spawn_owner: owner_id,
                spawn_rx: rx,
                spawn_ry: ry,
                spawn_z: z,
                was_selected,
            });
        }
        true
    }

    /// `BuildingClass::Sell`'s UndeploysInto conversion (stage 2), once the
    /// build-down (`building_down`) has run. The unit is constructed with its
    /// managers' children, the building's live attackers are listed
    /// (`0x00449F23..0x00449FDC`, Techno array order) and the building leaves
    /// the map (vt+0xD4, whose Detach_All clears their targets) for the
    /// unit's Unlimbo (`0x0044A002`). After its health
    /// (`0x0044A010..0x0044A039`) a building's slave manager moves to it
    /// (SetOwner `0x006AF580` at `0x0044A047`, which frees the unit's own
    /// fresh slaves), the unit takes the building's veterancy
    /// (`0x0044A058..0x0044A05E`), a building with an ArchiveTarget
    /// (`+0x218`, read at `0x00449E84`) sends the unit there through its
    /// class setter `vt+0x480(archive, 1)` and `Queue_Mission(Move, 0)`
    /// (`0x0044A091..0x0044A0AE`), and the listed attackers target the unit
    /// (`0x0044A146..0x0044A167`). The building's UnInit comes last. Not
    /// carried over: its Group (`+0x214`, `0x0044A04C`; VERA's control groups
    /// live in the app), AttachedTag (`+0x34`, `0x0044A0B4`) and looping sound
    /// handles (`+0x4DC..+0x4F4`); a refused Unlimbo does not refund the
    /// building's value (`0x0044A16B`).
    pub(crate) fn finish_undeploy(
        &mut self,
        sid: u64,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some((unit_type_id, owner_id, rx, ry, z, was_selected)) =
            self.substrate.entities.get(sid).and_then(|entity| {
                entity.building_down.as_ref().map(|down| {
                    (
                        down.spawn_type,
                        down.spawn_owner,
                        down.spawn_rx,
                        down.spawn_ry,
                        down.spawn_z,
                        down.was_selected,
                    )
                })
            })
        else {
            return;
        };
        let Some(rules) = rules else {
            self.uninit(sid);
            return;
        };
        // Building449E66/70 captures current health/type ratio at actual
        // conversion, not when the reverse animation was requested. A missing
        // live type cannot supply a conversion ratio.
        let Some((converted_health, archive, veterancy)) =
            self.substrate.entities.get(sid).and_then(|building| {
                let health = crate::sim::conversion_health::ConversionHealth::capture(
                    building,
                    self.object_type(building.type_ref(), rules)?,
                    rules.object(self.interner.resolve(unit_type_id))?,
                    crate::sim::conversion_health::ConversionKind::Building,
                );
                Some((health, building.archive_target(), building.veterancy_raw))
            })
        else {
            return;
        };
        let unit_type = self.interner.resolve(unit_type_id).to_string();
        let owner = self.interner.resolve(owner_id).to_string();
        let Some(unit) = self
            .construct_runtime_techno(
                &unit_type,
                &owner,
                rx,
                ry,
                0,
                z,
                rules,
                TechnoConstructorInit::FreshScenario,
            )
            .expect("fresh Techno constructor initialization cannot fail")
        else {
            self.uninit_with_rules(sid, rules);
            return;
        };
        let (new_sid, position) = self.store_with_constructor_managers(unit, Some(rules));
        let targeters: Vec<u64> = self
            .substrate
            .entities
            .iter_sorted()
            .filter(|(id, entity)| {
                *id != sid
                    && *id != new_sid
                    && entity.lifecycle.object_alive
                    && entity
                        .attack_target
                        .as_ref()
                        .is_some_and(|target| target.target == TargetKind::Entity(sid))
            })
            .map(|(id, _)| id)
            .collect();
        let _ = self.techno_limbo_with_rules(sid, rules);
        let (new_sid, outcome) =
            self.unlimbo_constructed_parent(new_sid, position, Some(rules), overlay_registry);
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            self.discard_constructed_limbo(new_sid);
            self.uninit_with_rules(sid, rules);
            return;
        }
        self.initialize_cloak_after_unlimbo(new_sid, rules);
        self.add_unit_sensor_after_unlimbo(new_sid, rules);
        if let Some(unit) = self.substrate.entities.get_mut(new_sid) {
            converted_health.apply(unit);
            unit.selected = was_selected;
            // The VeterancyClass (`+0x150`); the rank cache (`+0x13C`) stays
            // the unit's own.
            unit.veterancy_raw = veterancy;
            unit.veterancy = crate::sim::combat::veterancy::rank_u16(veterancy);
        }
        self.transfer_slave_manager(sid, new_sid, false, rules, overlay_registry);
        // A building's archive is a cell: the Slave Miner refinery's
        // relocation is its only VERA writer (`slave_manager`).
        if let Some(TargetKind::Cell(x, y)) = archive {
            if !self.set_unit_cell_destination(new_sid, (x, y), rules) {
                log::debug!("undeployed unit {new_sid} refused its archive ({x}, {y})");
            }
            if let Some(unit) = self.substrate.entities.get_mut(new_sid) {
                crate::sim::mission::authority::queue_entity_mission_deferred(
                    unit,
                    crate::sim::mission::MissionId::from_known(
                        crate::sim::mission::MissionType::Move,
                    ),
                );
            }
        }
        let target = Some(TargetKind::Entity(new_sid));
        let commits = crate::sim::mission::concrete_effects::assign_target_commits(
            &self.substrate.entities,
            target,
        );
        for targeter in targeters {
            if let Some(entity) = self.substrate.entities.get_mut(targeter) {
                crate::sim::mission::concrete_effects::represented_assign_target_admitted(
                    entity, target, commits,
                );
            }
        }
        self.uninit_with_rules(sid, rules);
    }

    pub(crate) fn should_show_undeploy_building_command(
        &self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        let Some(obj) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        if obj.construction_yard && self.owner_has_building_production_busy(entity.owner()) {
            return false;
        }
        self.can_undeploy_building_runtime(stable_id, rules)
    }

    pub(crate) fn can_undeploy_building_runtime(&self, stable_id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        if entity.category != EntityCategory::Structure
            || entity.building_up.is_some()
            || entity.building_down.is_some()
        {
            return false;
        }
        let type_str = self.interner.resolve(entity.type_ref());
        let Some(obj) = rules.object(type_str) else {
            return false;
        };
        let Some(target) = obj.undeploys_into.as_deref() else {
            return false;
        };
        if rules.object(target).is_none() {
            return false;
        }
        if !obj.construction_yard {
            return true;
        }
        self.construction_yard_redeploy_core_gate(entity)
    }

    fn construction_yard_redeploy_core_gate(&self, entity: &GameEntity) -> bool {
        if !self.session.game_options.mcv_redeploy || !entity.radio_contacts.is_empty() {
            return false;
        }
        // `BuildingClass::CanUndeployMCV @ 0x00449C15` and
        // `ShouldShowDeployButton @ 0x0044F614`: a mind-controlled yard (+0x2C0)
        // cannot repack.
        if entity.mind_control.is_mind_controlled() {
            return false;
        }
        self.houses
            .get(&entity.owner())
            .is_some_and(|house| house.is_human)
    }

    fn owner_has_building_production_busy(&self, owner: crate::sim::intern::InternedId) -> bool {
        // P5d: the registry is the queue-of-record. Busy = an active Building build held OR
        // a non-empty Building tail.
        self.production
            .factory_shadow
            .view(owner, ProductionCategory::Building)
            .is_some_and(|v| v.object.is_some() || !v.queue.is_empty())
    }

    /// Find the next available infantry sub-cell at a given cell position.
    /// Scans existing infantry entities at (rx, ry) and returns the first unused
    /// spot from FUNCTIONAL_SUB_CELLS. Falls back to the first entry if all taken
    /// (caller should have avoided full cells via spawn cell selection).
    fn allocate_infantry_sub_cell(&self, rx: u16, ry: u16) -> u8 {
        let mut occupied: [bool; 5] = [false; 5];
        for entity in self.substrate.entities.values() {
            if !entity.dying
                && !entity.lifecycle.in_limbo
                && entity.position.rx == rx
                && entity.position.ry == ry
                && entity.category == EntityCategory::Infantry
            {
                if let Some(sub) = entity.sub_cell {
                    if (sub as usize) < occupied.len() {
                        occupied[sub as usize] = true;
                    }
                }
            }
        }
        for &spot in &crate::sim::movement::bump_crush::FUNCTIONAL_SUB_CELLS {
            if !occupied[spot as usize] {
                return spot;
            }
        }
        crate::sim::movement::bump_crush::FUNCTIONAL_SUB_CELLS[0]
    }
}

/// Where a deploying unit's building lands, given the cell the unit is standing on.
///
/// gamemd takes a single step north-west, gated on the foundation being larger
/// than 2 in either axis — the dimensions are read separately but only feed one
/// OR, so a 3x3, a 4x4 and a 6x4 all get the same one-cell step and nothing is
/// ever halved. The unit's cell is therefore NOT the footprint's centre for an
/// even-sized building: a 4x4 Construction Yard puts it at local index (1,1),
/// the north-west one of the four middle cells, leaving one cell of yard to the
/// north-west and two to the south-east. That lopsidedness is authentic — it is
/// what a 4x4 with a one-cell step has to look like.
///
/// Verified against gamemd 2026-08-05. See [`undeploy_unit_cell`] for the
/// mirror; the two must stay inverses.
fn deploy_origin_from_unit_cell(unit_rx: u16, unit_ry: u16, foundation: &str) -> (u16, u16) {
    let (width, height) = foundation_dimensions(foundation);
    if width > 2 || height > 2 {
        (unit_rx.saturating_sub(1), unit_ry.saturating_sub(1))
    } else {
        (unit_rx, unit_ry)
    }
}

/// Resolve the deploy target for an MCV-like unit via rules.ini `DeploysInto=`.
fn construction_yard_type_for_mcv(type_id: &str, rules: &RuleSet) -> Option<String> {
    let obj = rules.object(type_id)?;
    let target: &str = obj.deploys_into.as_deref()?;
    rules.object(target)?;
    Some(target.to_string())
}

/// Resolve the undeploy target for a building via rules.ini `UndeploysInto=`.
fn undeploy_target_for_building(type_id: &str, rules: &RuleSet) -> Option<String> {
    let obj = rules.object(type_id)?;
    let target: &str = obj.undeploys_into.as_deref()?;
    rules.object(target)?;
    Some(target.to_string())
}

/// Where the unit reappears when a building undeploys, given the building's
/// north-west footprint cell.
///
/// The exact mirror of [`deploy_origin_from_unit_cell`]: gamemd steps one cell
/// south-east behind the same `> 2` foundation gate, so deploy-then-undeploy
/// returns the vehicle to the cell it started on, for every foundation size.
///
/// This used to add `width / 2`, which is `+2` on the 4x4 Construction Yard
/// against gamemd's `+1`. The two halves were not inverses, so every
/// deploy/undeploy cycle walked the MCV one cell east and one cell south, and
/// the error compounded across cycles until a redeploy could fail on terrain
/// gamemd would never have put the vehicle on. The old name asserted the
/// footprint had a centre cell; a 4x4 does not, and believing it did is what
/// produced the wrong inverse.
///
/// Verified against gamemd 2026-08-05.
fn undeploy_unit_cell(origin_rx: u16, origin_ry: u16, foundation: &str) -> (u16, u16) {
    let (width, height) = foundation_dimensions(foundation);
    if width > 2 || height > 2 {
        (origin_rx.saturating_add(1), origin_ry.saturating_add(1))
    } else {
        (origin_rx, origin_ry)
    }
}

#[cfg(test)]
#[path = "world_spawn/tests.rs"]
mod techno_constructor_tests;
