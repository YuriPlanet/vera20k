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
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::base_plan::pack_base_plan_cell;
use crate::sim::base_plan_generation::{preflight_recalc, recalc_base_plan};
use crate::sim::combat::TargetKind;
use crate::sim::components::{BuildingUp, Health};
use crate::sim::game_entity::{
    GameEntity, GeneratedTechnoInit, StructureUpgradeLink, TechnoConstructorInit,
};
use crate::sim::intern::InternedId;
use crate::sim::production::{self, ProductionCategory, foundation_dimensions};
use crate::sim::vision::MAX_SIGHT_RANGE;
use crate::util::fixed_math::SimFixed;

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
        .art()
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
            // This deliberately lifecycle-free diagnostic is not a native
            // concrete constructor and must not spend the gameplay cursor.
            0,
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
    pub fn spawn_from_map(&mut self, entities: &[MapEntity], rules: Option<&RuleSet>) -> u32 {
        self.spawn_from_map_with_resolved(entities, rules, None)
    }

    pub fn spawn_from_map_with_resolved(
        &mut self,
        entities: &[MapEntity],
        rules: Option<&RuleSet>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
    ) -> u32 {
        self.spawn_from_map_with_resolved_and_overlay_registry(
            entities,
            rules,
            resolved_terrain,
            None,
        )
    }

    pub(crate) fn spawn_from_map_with_resolved_and_overlay_registry(
        &mut self,
        entities: &[MapEntity],
        rules: Option<&RuleSet>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> u32 {
        self.spawn_from_map_with_constructor_inits(
            entities,
            rules,
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
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        constructor_inits: &GeneratedTechnoInitTable,
    ) -> Result<u32, GeneratedTechnoInitError> {
        self.spawn_from_map_with_constructor_inits(
            entities,
            Some(rules),
            resolved_terrain,
            None,
            Some(constructor_inits),
        )
    }

    fn spawn_from_map_with_constructor_inits(
        &mut self,
        entities: &[MapEntity],
        rules: Option<&RuleSet>,
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

        // ReadScenario684685 / FullInit686B4F retain nonzero A8E7AC
        // through the authored readers (Unit743270 called at687AA7).
        // PostMapInit68691C clears that context for starting-unit callbacks,
        // which use the separate runtime spawn path. This projection retains
        // the same admission context through all constructor/Unlimbo callbacks.
        self.with_object_placement_scope(|sim| {
            let mut count: u32 = 0;

            for (entity_index, map_ent) in entities.iter().enumerate() {
                if !sim.map_entity_resolves_before_constructor(
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
                // Unit ReadUnits7434F3 writes OnBridge from HIGH itself; its
                // 7434FB..743510 coordinate is Map578080 ground plus B1D0AC.
                // HasBridge and navigation's bridge_walkable do not admit that
                // reader-owned pose. Other classes retain their existing adapter.
                let unit_high = map_ent.category == EntityCategory::Unit && map_ent.high;
                let bridge_spawn = if unit_high {
                    Some(
                        resolved_terrain
                            .or(sim.resolved_terrain.as_ref())
                            .and_then(|terrain| terrain.cell(map_ent.cell_x, map_ent.cell_y))
                            .map_or(0, |cell| cell.level)
                            .wrapping_add(4),
                    )
                } else {
                    map_ent
                        .high
                        .then(|| {
                            resolved_terrain
                                .and_then(|terrain| terrain.cell(map_ent.cell_x, map_ent.cell_y))
                                .filter(|cell| cell.bridge_walkable)
                                .map(|cell| cell.bridge_deck_level)
                        })
                        .flatten()
                };
                if map_ent.high && bridge_spawn.is_none() {
                    log::warn!(
                        "Map entity {} at ({},{}) requested HIGH spawn but no bridge deck was resolved; falling back to ground",
                        map_ent.type_id,
                        map_ent.cell_x,
                        map_ent.cell_y
                    );
                }
                let z: u8 = bridge_spawn.unwrap_or_else(|| {
                    resolved_terrain
                        .or(sim.resolved_terrain.as_ref())
                        .and_then(|terrain| terrain.cell(map_ent.cell_x, map_ent.cell_y))
                        .map_or(0, |cell| cell.level)
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

                let stable_id = sim.allocate_stable_id();
                let owner_id = sim.interner.intern(&map_ent.owner);
                let type_id = sim.interner.intern(&map_ent.type_id);
                let constructor_init = generated_inits
                    .as_ref()
                    .map_or(TechnoConstructorInit::FreshScenario, |inits| {
                        TechnoConstructorInit::PreconsumedGenerated((*inits[entity_index]).clone())
                    });
                let techno_ctor_random_word = sim.resolve_techno_constructor_word(
                    constructor_init,
                    generated_inits.as_ref().map(|_| {
                        (
                            entity_index,
                            map_ent.type_id.as_str(),
                            (map_ent.cell_x, map_ent.cell_y),
                        )
                    }),
                )?;
                // Concrete constructors assign after the base Techno Scenario word
                // and before Unlimbo. Generated rows already spent both effects at
                // their original construction point; projection must not repeat it.
                let native_unique_id = generated_inits.as_ref().map_or_else(
                    || sim.next_native_runtime_id(),
                    |inits| inits[entity_index].native_unique_id,
                );

                // Build the GameEntity with all required fields.
                let mut ge = GameEntity::new_at_frame_from_constructor_word(
                    stable_id,
                    native_unique_id,
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
                    sim.session.binary_frame,
                    techno_ctor_random_word,
                );
                if map_ent.category == EntityCategory::Unit {
                    let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&ge.position);
                    let mut requested = crate::sim::components::DriveCoord { x, y, z: 0 };
                    if unit_high {
                        requested.z = i32::from(z as i8)
                            .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS);
                        if let Some(terrain) = resolved_terrain.or(sim.resolved_terrain.as_ref()) {
                            match crate::sim::movement::ground_pose::query_ground_height(
                                &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
                                requested,
                            ) {
                                Ok(ground) => {
                                    requested.z = ground.wrapping_add(
                                        crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
                                    );
                                }
                                // Unsupported slope identities retain the prior
                                // coarse adapter; no native height claim for them.
                                Err(cause) => log::warn!("authored Unit HIGH height: {cause}"),
                            }
                        }
                    }
                    // LOW supplies Z0 (743408..743431); ObjectType747EB0 then
                    // clamps it to ground. Retain the legacy level projection
                    // separately from this exact caller coordinate.
                    crate::sim::movement::ground_pose::put_location(&mut ge.position, requested);
                }
                ge.base_defense_response.recruitable_a = map_ent.recruitable_a;
                ge.base_defense_response.recruitable_b = map_ent.recruitable_b;

                sim.install_techno_components(
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
                // The line's AI Sellable and AI Repairable, written after the
                // constructor and before the Unlimbo (`BuildingClass::ReadFromINI`
                // `0x0044FB5B`, `0x0044FB70`).
                if map_ent.category == EntityCategory::Structure {
                    ge.ai_sellable = map_ent.structure_ai_sellable;
                    ge.ai_repairable = map_ent.structure_ai_repairable;
                }
                let (stable_id, outcome) =
                    sim.unlimbo_authored_techno(ge, map_ent.health, rules, overlay_registry);
                if !matches!(outcome, RevealOutcome::Revealed { .. }) {
                    sim.discard_constructed_limbo(stable_id, rules);
                    continue;
                }
                if let Some(ruleset) = rules {
                    sim.initialize_cloak_after_unlimbo(stable_id, ruleset);
                    sim.add_unit_sensor_after_unlimbo(stable_id, ruleset);
                    sim.add_building_sensor_array_if_powered(stable_id, ruleset);
                }
                sim.commit_map_placement_mission(stable_id, map_ent.mission);
                if let Some(rules) = rules {
                    sim.finish_authored_building_enable(stable_id, rules);
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
                        if sim
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
        })
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
    ) -> Option<u64> {
        let stable_id =
            self.construct_object_limbo_at_height(type_id, owner, rx, ry, facing, z, rules)?;
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
            self.discard_constructed_limbo(stable_id, Some(rules));
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
    ///
    /// A building's `[Structures]` line has no mission column; it takes
    /// [`Self::queue_placed_building_guard`].
    fn commit_map_placement_mission(
        &mut self,
        stable_id: u64,
        authored: Option<crate::sim::mission::MissionType>,
    ) {
        if self.is_dispatchable_miner(stable_id) || self.queue_placed_building_guard(stable_id) {
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

    /// The mission of a building placed without a build-up (a map building,
    /// or one [`Self::spawn_object`] places): its Unlimbo arm (vt+0x484 =
    /// `0x0044D6A0`: Guard at `0x0044D6DC..0x0044D6F1` unless the build-up
    /// flag is set outside scenario init) queues Guard without commencing,
    /// and the first opening its discovery runs (`0x0044D5D0` ->
    /// Grand_Opening, `0x004467C9`) sets `+0x6DD`, so its first Update's
    /// ready check commences Guard. Native execution:
    /// `tools/spatial_oracle/building_guard_attack.json` (`unlimbo` rows).
    /// Answers whether `stable_id` is a building.
    ///
    /// RESIDUAL: VERA opens every such building at once; native reaches a
    /// campaign's human-owned building through its discovery
    /// (DiscoveredBy(Player) needs its cell's `+0x12C & 0x10` when GameMode
    /// is 0). Trigger: a campaign map's shrouded player building. Effect: it
    /// guards before it is discovered. Frequency: campaign maps only;
    /// skirmish and multiplayer open every building at Unlimbo.
    fn queue_placed_building_guard(&mut self, stable_id: u64) -> bool {
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Structure)
        {
            return false;
        }
        //The existing immediate-map/convenience-spawn adapter enters without
        //a build-up. Its native scope is separate from ordinary Construction.
        self.building_enter_idle_mode(stable_id, false, None);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.mission_leaf.set_building_ready_latch(1);
        }
        true
    }

    ///Building44D6A0, shared by map placement, owner change, factory Unload
    ///and Gate Open. The normal arm calls the sole447780 body owner, then
    ///queues Guard5 without commencing. Native returns false on either arm.
    ///
    ///The initial=true, scope0 branch also tests globalA8ED6B (build-up
    ///bypass) before choosing Construction18. Its caller/global initialization
    ///belongs to the separate initial Building placement mechanism; existing
    ///immediate-placement adapters deliberately enter the normal arm.
    pub(crate) fn building_enter_idle_mode(
        &mut self,
        stable_id: u64,
        initial: bool,
        rules: Option<&RuleSet>,
    ) -> bool {
        if initial && !self.object_placement_scope_active() {
            return false;
        }
        let now = self.session.binary_frame;
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.begin_building_body(
                crate::sim::building_construction::BuildingBodyMode::Idle,
                now as i32,
            );
        }
        let mission =
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard);
        if let Some(rules) = rules {
            let _ = self.mission_queue_exact(
                stable_id,
                mission,
                0,
                now,
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
        } else {
            let _ = self.mission_queue_exact(
                stable_id,
                mission,
                0,
                now,
                &crate::sim::mission::authority::EntityReadyInputProvider,
            );
        }
        false
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
    ) -> Option<u64> {
        let z: u8 = self.terrain_cell_level(rx, ry).unwrap_or(0);
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
        overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    ) -> Option<u64> {
        let z = self.terrain_cell_level(rx, ry).unwrap_or(0);
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
            self.discard_constructed_limbo(stable_id, Some(rules));
            return Ok(None);
        }
        self.initialize_cloak_after_unlimbo(stable_id, rules);
        self.add_unit_sensor_after_unlimbo(stable_id, rules);
        self.queue_placed_building_guard(stable_id);
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

    /// Place an already constructed limbo Techno, such as a held production
    /// object, without repeating its constructor draw, manager initialization
    /// or Add_Tracking. Failure restores this same identity to limbo so a
    /// caller may try another coordinate.
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
        self.reveal_constructed_object_at_height_with_overlay_context(
            stable_id, rx, ry, facing, z, placement, rules, None,
        )
    }

    /// Retain the caller's exact native coordinate until admission succeeds.
    /// Both coordinate and height-level callers use the same Unlimbo body.
    pub(crate) fn reveal_constructed_object_at_coord_with_overlay_context(
        &mut self,
        stable_id: u64,
        coord: crate::sim::components::DriveCoord,
        facing: u8,
        placement: PlacementEvidence,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Option<u64> {
        let mut position = self.substrate.entities.get(stable_id)?.position;
        crate::sim::movement::ground_pose::put_location(&mut position, coord);
        position.z = (coord.z / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS) as u8;
        self.reveal_constructed_object(
            stable_id,
            position,
            facing,
            placement,
            rules,
            overlay_registry,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reveal_constructed_object_at_height_with_overlay_context(
        &mut self,
        stable_id: u64,
        rx: u16,
        ry: u16,
        facing: u8,
        z: u8,
        placement: PlacementEvidence,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Option<u64> {
        let requested_position = RevealPosition {
            exact_z_leptons: None,
            rx,
            ry,
            z,
            // This compatibility API supplies a cell-center coordinate. Held
            // constructor Location is zero; exact-coordinate callers use the
            // coordinate entry point above instead of borrowing that pose.
            sub_x: SimFixed::from_num(128),
            sub_y: SimFixed::from_num(128),
        };
        self.reveal_constructed_object(
            stable_id,
            requested_position,
            facing,
            placement,
            rules,
            overlay_registry,
        )
    }

    /// Infantry51DFF0 places its requested floor coordinate before the common
    /// Object5F4F1B class query. Rejection retains the constructor-held pose
    /// and facing; a successful Techno snaps facing before its idle callback.
    fn reveal_constructed_object(
        &mut self,
        stable_id: u64,
        requested_position: RevealPosition,
        facing: u8,
        placement: PlacementEvidence,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Option<u64> {
        let is_infantry = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Infantry);
        let requested_position = if is_infantry {
            self.infantry_unlimbo_position(requested_position, Some(rules))?
        } else {
            requested_position
        };
        let placement = if placement == PlacementEvidence::EvaluateMark {
            self.constructor_unlimbo_placement(
                stable_id,
                requested_position,
                rules,
                overlay_registry,
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

        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && is_infantry
        {
            entity.sub_cell = Some(crate::sim::movement::bump_crush::priority_sub_cell(
                requested_position.sub_x,
                requested_position.sub_y,
            ));
            entity.on_bridge = false;
        }
        let outcome = self.try_reveal_entity_with_context(
            stable_id,
            RevealRequest {
                position: requested_position,
                placement,
                logic_eligible: true,
            },
            super::lifecycle::UninitContext::new(Some(rules), overlay_registry)
                .with_unlimbo_facing(Some(facing)),
        );
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            return None;
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && is_infantry
        {
            // Infantry51E114 restores its class water sentinel after Foot.
            entity.mission_leaf.reset_infantry_water_state();
        }
        self.allocate_building_light(stable_id, rules);
        self.initialize_cloak_after_unlimbo(stable_id, rules);
        self.add_unit_sensor_after_unlimbo(stable_id, rules);
        Some(stable_id)
    }

    /// InfantryClass51DFF0 -> CellPlaceInfantry481180, reused by factory,
    /// survivor and slave entry. The class places only at the sampled floor;
    /// airborne exact coordinates keep their XY. Scenario priority/outside
    /// usable-area placement takes the requested quadrant without a draw.
    /// Native frontend/FootXYZ/three-stream controls:
    /// tools/spatial_oracle/_factory_infantry_output, Infantry51DFF0 and
    /// Cell481180 on gamemd SHA1cdd1180; infantry_unlimbo_gate_native.json.
    pub(crate) fn infantry_unlimbo_position(
        &mut self,
        mut position: RevealPosition,
        rules: Option<&RuleSet>,
    ) -> Option<RevealPosition> {
        use crate::sim::movement::{bump_crush, ground_pose, locomotor::MovementLayer, walk_head};
        use crate::sim::occupancy::RawCellKey;
        let input = ground_pose::position_world_coord(&position);
        let xy = [input.x, input.y];
        let floor = ground_pose::ground_surface_z_at(
            xy,
            false,
            self.resolved_terrain.as_ref(),
            self.path_grid(),
        );
        //51E018..021 compares the supplied raw Z, including a coarse caller's
        //signed level representation, before the later Object type clamp.
        let requested_z = input.z;
        if floor.is_some_and(|floor| requested_z != floor) {
            return Some(position);
        }
        let packed_cell = (
            crate::util::lepton::lepton_to_cell_packed(xy[0]),
            crate::util::lepton::lepton_to_cell_packed(xy[1]),
        );
        //51E027..51E02E skips incoming Map578460 when A8E7AC is raised.
        //Membership uses the incoming packed words even when their linear
        //lookup aliases another real Cell; retained Map578540 is different.
        let priority = self.object_placement_scope_active()
            || self
                .resolved_terrain
                .as_ref()
                .zip(self.playfield_bounds)
                .is_some_and(|(terrain, bounds)| {
                    !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                        (i32::from(packed_cell.0), i32::from(packed_cell.1)),
                        Some(bounds),
                        Some(terrain),
                    )
                });
        let key = self
            .resolved_terrain
            .as_ref()
            .map_or(RawCellKey::Real(position.rx, position.ry), |terrain| {
                RawCellKey::from_native(terrain, terrain.native_cell_identity(packed_cell))
            });
        //481298..481313 reaches the live ground Gate only after ordinary
        //vehicle/object gates. Reuse its Building4525F0/Door4A51B0 owner.
        let gate_open = !priority && {
            let ground = self
                .substrate
                .raw_cell_occupation
                .bits_at(key, MovementLayer::Ground);
            ground & crate::sim::cell_kernel::INFANTRY_OCCUPATION_VEHICLE_BIT == 0
                && ground & crate::sim::cell_kernel::INFANTRY_OCCUPATION_OBJECT_BIT != 0
                && match key {
                    RawCellKey::Real(x, y) => bump_crush::ground_gate_is_open(
                        &self.substrate.occupancy,
                        &self.substrate.entities,
                        rules,
                        &self.interner,
                        (x, y),
                    ),
                    RawCellKey::Dummy => false,
                }
        };
        let spot = bump_crush::place_infantry_in_native_cell(
            &self.substrate.raw_cell_occupation,
            key,
            MovementLayer::Ground,
            input,
            priority,
            gate_open,
            &mut self.scenario_rng,
        )?;
        //Cell481180 removes each incoming XY low byte before adding the
        //selected slot, including negative positions and real-cell aliases.
        let selected = walk_head::selected_head(input, spot, requested_z, false);
        ground_pose::set_position_world_xy(&mut position, [selected.x, selected.y]);
        Some(position)
    }

    /// Store a freshly constructed object in native-style limbo and account for
    /// its owner. Placement is a separate, result-bearing Reveal transaction.
    fn store_spawned_limbo(&mut self, mut ge: GameEntity) -> u64 {
        let stable_id = ge.stable_id();

        // Object5F3993..5F39C0 copies AC1380's default CoordStruct, whose
        // original CRT initializer5F38A0 writes (0,0,0). Every held Techno
        // constructor reaches it. Immediate placement saves its caller input
        // before this store; later placement or launch supplies its own pose.
        // Evidence: anytown_damage/unit_unlimbo.{json,meta.json,md}.
        crate::sim::movement::ground_pose::put_location(
            &mut ge.position,
            crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 },
        );
        ge.position.z = 0;

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
    pub(crate) fn discard_constructed_limbo(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        debug_assert!(entity.lifecycle.in_limbo && !entity.lifecycle.cell_marked);
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
        // Factory cancel and rejected constructors invoke the same scalar
        // destructor as the late drain, without UnInit or loss bookkeeping.
        // Reuse that owner for Building43BCF0 sound/expiry/Anim/power cleanup
        // and delegated Techno Remove_Tracking, rather than a manual store drop.
        self.finalize_and_remove_common(
            stable_id,
            super::lifecycle::UninitContext::new(rules, None),
        );
        for child_id in spawn_children.into_iter().chain(slave_children) {
            if self.substrate.entities.contains(child_id) {
                let discarded = self.discard_constructed_limbo(child_id, rules);
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
        let position = ge.position;
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
        self.unlimbo_constructed_parent(stable_id, position, rules, overlay_registry, false)
    }

    fn store_with_constructor_managers(
        &mut self,
        ge: GameEntity,
        rules: Option<&RuleSet>,
    ) -> (u64, RevealPosition) {
        let position = ge.position;
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
        position: RevealPosition,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        scenario_initialization: bool,
    ) -> (u64, RevealOutcome) {
        // ScenarioFullInit686B4F raises A8E7AC before InfantryRead51FB00
        // at687ACB, retaining it until687C2B..687C44 after all map objects.
        // This initial-object entry preserves that caller state through the
        // class placement, common admission and downstream idle callbacks.
        if scenario_initialization && !self.object_placement_scope_active() {
            return self.with_object_placement_scope(|sim| {
                sim.unlimbo_constructed_parent(
                    stable_id,
                    position,
                    rules,
                    overlay_registry,
                    scenario_initialization,
                )
            });
        }
        let infantry = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category == EntityCategory::Infantry);
        let position = if infantry {
            let Some(position) = self.infantry_unlimbo_position(position, rules) else {
                return (
                    stable_id,
                    RevealOutcome::Failed(super::lifecycle::RevealFailure::RejectedEarly),
                );
            };
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.sub_cell = Some(crate::sim::movement::bump_crush::priority_sub_cell(
                    position.sub_x,
                    position.sub_y,
                ));
            }
            position
        } else {
            position
        };
        let facing = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.body_facing_dir(self.session.binary_frame));
        let placement = rules.map_or(PlacementEvidence::EvaluateMark, |rules| {
            self.constructor_unlimbo_placement(stable_id, position, rules, overlay_registry)
        });
        let outcome = self.try_reveal_entity_with_context(
            stable_id,
            RevealRequest {
                position,
                placement,
                logic_eligible: true,
            },
            super::lifecycle::UninitContext::new(rules, overlay_registry)
                .with_unlimbo_facing(facing),
        );
        if matches!(outcome, RevealOutcome::Revealed { .. }) {
            if let Some(rules) = rules {
                self.allocate_building_light(stable_id, rules);
                if self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| {
                        entity.category == EntityCategory::Structure && !entity.building_up()
                    })
                {
                    if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                        entity.initialize_building_idle_body(self.session.binary_frame as i32);
                    }
                    // Map import carries ScenarioInit explicitly. Runtime
                    //Construction enters through its own mission dispatcher.
                    self.grand_opening(
                        stable_id,
                        false,
                        scenario_initialization,
                        rules,
                        overlay_registry,
                    );
                }
            }
        }
        (stable_id, outcome)
    }

    /// Object5F4F1B..5F4F49 shares movement/search's native class receiver.
    /// Foot4D9C60 query-local plane outputs do not write OnBridge or Location.
    fn constructor_unlimbo_placement(
        &self,
        stable_id: u64,
        position: RevealPosition,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> PlacementEvidence {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return PlacementEvidence::RejectedEarly;
        };
        // Other classes' +1AC constructor receivers are separate migrations.
        // Terrain-less diagnostics retain their existing generic Mark seam.
        if !matches!(
            entity.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) || self.resolved_terrain.is_none()
        {
            return PlacementEvidence::EvaluateMark;
        }
        if self.object_placement_scope_active() {
            return PlacementEvidence::UnitEntryAdmitted;
        }
        let terrain = self.resolved_terrain.as_ref().unwrap();
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&position);
        let cell = cells.lookup_world(x, y);
        match self.foot_can_enter(
            stable_id,
            cell,
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            rules,
            overlay_registry,
        ) {
            Ok(0) => PlacementEvidence::UnitEntryAdmitted,
            Ok(_) => PlacementEvidence::RejectedEarly,
            Err(cause) => {
                log::debug!("Foot {stable_id} Unlimbo admission: {cause}");
                PlacementEvidence::RejectedEarly
            }
        }
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
                    entity.veterancy(),
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
        frame_counts: &std::collections::BTreeMap<
            (String, crate::sim::components::VxlLayer, i32),
            u32,
        >,
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
                frame_counts.get(&(self.interner.resolve(type_ref).to_string(), *layer, 0))
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

    /// `UnitClass::Deploy @ 0x007393C0`: a stopped unit whose DeploysInto
    /// type can stand at its origin turns to the type's `DeployFacing=` or,
    /// already facing it, becomes that building. False when it refuses.
    ///
    /// Not carried over to the building: the unit's Group (`+0x214`,
    /// `0x007397C6`; VERA's control groups live in the app), its AttachedTag
    /// (`+0x34`, `0x007399EE`), its looping sound handles (`+0x4DC..+0x4F4`,
    /// `0x00739971`), `DiscoveredBy(owner)` (vt+0x198, `0x00739848`) and the
    /// turret facing a `+0x16CA`/`+0x16C4` type starts at (`0x007397FB`).
    pub(crate) fn deploy_mcv(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        // Native early exits on a NavCom (0x7393E4) or the locomotor's
        // Is_Moving (vt+0x10 at 0x00739405, 0x73940A) retain runtime +0x68C.
        // They must not attempt placement while stopping.
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
        let stolen_mcv = source.mind_control.controller().is_some()
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
            || crate::sim::movement::motion_query::is_moving(source) == Some(true)
        {
            return false;
        }
        // Read deploy data from EntityStore before mutating.
        let deploy_data = self.substrate.entities.get(stable_id).and_then(|entity| {
            let type_str = self.interner.resolve(entity.type_ref());
            let yard_type = construction_yard_type_for_mcv(type_str, rules)?;
            let yard_obj = rules.object(&yard_type)?;
            let origin = deploy_origin_from_unit_cell(
                entity.position.rx,
                entity.position.ry,
                &yard_obj.foundation,
            );
            Some((
                entity.owner(),
                origin,
                entity.position.z,
                yard_type.clone(),
                yard_obj.deploy_facing,
                (
                    entity.selected,
                    entity.veterancy_raw,
                    entity.attack_target.as_ref().map(|t| t.target),
                ),
                // Native rounds FacingClass::Current, including wrap at 0xff80.
                entity.body_facing_dir(self.session.binary_frame),
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
            origin,
            z,
            yard_type,
            deploy_facing,
            (was_selected, veterancy, source_target),
            source_facing,
            is_construction_yard,
            deploy_cue,
            resource_gatherer,
        )) = deploy_data
        else {
            return false;
        };

        // `0x00739422..0x007394D8`: the unit leaves its cell (Mark(UP),
        // vt+0x124(0); the Drive locomotor's Mark_All_Occupation_Bits(0)
        // clears the same bits for a unit standing still), the DeploysInto
        // type's CanPlaceAt tests the origin for no house, and the unit is put
        // back (`0x0073953B..0x00739565` / `0x0073959C..0x007395B4`) before
        // either outcome acts.
        self.foot_mark_remove(stable_id, Some(rules), registry);
        let placeable = rules.object(&yard_type).is_some_and(|yard| {
            crate::sim::build_site::can_place_building_at(self, rules, registry, yard, origin, None)
        });
        self.foot_mark_put(stable_id, Some(rules), registry);
        if !placeable {
            log::info!("MCV deploy blocked at origin {origin:?}");
            // `0x007394E0..0x0073950A`: EVA CannotDeployHere only for the
            // local player's (`IsHumanPlayer`, carried by the event's owner)
            // non-`ResourceGatherer=` unit; a blocked Slave Miner is silent.
            //
            // RESIDUAL: a computer owner's refusal first runs
            // `BuildingTypeClass::Flush_For_Placement @ 0x0045EE70`
            // (`0x0073950F..0x00739536`), which scatters allied foot units off
            // the footprint. Trigger: a computer owner's Deploy onto a taken
            // footprint: the retry after the MCV's turn (retail yards keep
            // `DeployFacing=` 0x80, so most MCVs turn first) once a unit has
            // moved in, a Mission_Unload deploy, or a Slave Miner's
            // (`0x006AFEF9`, `0x006AFF6D`). Effect: the blockers are not asked
            // to move, so the MCV's mission tries again later, perhaps at
            // another site. Frequency: uncommon; any unit, infantry or wall in
            // the footprint refuses. Ported with its main consumer, the
            // computer's building placement.
            if !resource_gatherer {
                self.sound_events
                    .push(SimSoundEvent::CannotDeployHere { owner: owner_id });
            }
            // `0x00739573..0x0073957A`: +0x68C cleared, Queue_Mission(Guard, 0).
            self.substrate
                .entities
                .get_mut(stable_id)
                .unwrap()
                .mcv_deploy_pending = false;
            crate::sim::mcv_deploy::queue_guard(self, stable_id);
            return false;
        }
        // An admitted foundation lies on real cells, so the origin is on the map.
        let (rx, ry) = (origin.0 as u16, origin.1 as u16);

        if source_facing != deploy_facing {
            // `0x007395EF..0x0073965F`: Do_Turn unless already turning, OVER_OUT
            // to radio contact 0, then +0x68C.
            let now = self.session.binary_frame;
            if let Some(entity) = self.substrate.entities.get_mut(stable_id)
                && !crate::sim::movement::motion_query::is_moving_now(
                    entity,
                    Some(crate::sim::movement::SpeedRules::new(
                        rules,
                        &self.interner,
                        &self.type_handles,
                        &self.houses,
                    )),
                    now,
                )
            {
                crate::sim::movement::drive_do_turn(entity, u16::from(deploy_facing) << 8, now);
            }
            crate::sim::radio::transmit_to_contact(
                self,
                stable_id,
                crate::sim::radio::RadioMessage::Break,
                Some(rules),
            );
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
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
        // Native lifts the MCV again first (Mark(UP), `0x00739670`); VERA lifts
        // it at its UnInit instead, which leaves the same cells because the
        // yard's building bit and cell-list entry are its own
        // (`the_cell_the_mcv_stood_on_ends_as_a_yard_cell`).
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
        // Begin_Mode(0) at its Unlimbo, the Construction mission queued
        // (0x007396D5) and its ready byte set (0x0073984E).
        destination.install_building_up(
            BuildingUp::deployed(
                rules.buildup_control(&yard_type),
                self.session.binary_frame as i32,
            ),
            self.session.binary_frame as i32,
        );
        let (new_sid, outcome) =
            self.unlimbo_after_constructor_managers(destination, Some(rules), registry);
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            self.discard_constructed_limbo(new_sid, Some(rules));
            return false;
        }
        // 0x0073971F: OVER_OUT to radio contact 0.
        crate::sim::radio::transmit_to_contact(
            self,
            stable_id,
            crate::sim::radio::RadioMessage::Break,
            Some(rules),
        );
        // 0x0073972C..0x007397C0: every live Techno targeting the unit, in
        // TechnoClass::Array order, targets the building instead; a
        // `VehicleThief=` infantryman drops the target when it is a
        // Construction Yard. Each goes through its class's Assign_Target
        // (vt+0x3C8).
        let hijacker_drops = is_construction_yard;
        let targeters: Vec<(u64, bool)> = self
            .substrate
            .entities
            .iter_sorted()
            .filter(|(id, entity)| {
                *id != stable_id
                    && *id != new_sid
                    && entity.lifecycle.object_alive
                    && entity
                        .attack_target
                        .as_ref()
                        .is_some_and(|target| target.target == TargetKind::Entity(stable_id))
            })
            .map(|(id, entity)| {
                let drops = hijacker_drops
                    && entity.category == EntityCategory::Infantry
                    && self
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|obj| obj.vehicle_thief);
                (id, drops)
            })
            .collect();
        for (targeter, drops) in targeters {
            let target = (!drops).then_some(TargetKind::Entity(new_sid));
            let _ = self.assign_target_represented(targeter, target, Some(rules));
        }
        // 0x007397D2..0x007397DE: the unit's veterancy (`+0x150`); the rank
        // cache (`+0x13C`) stays the building's own.
        if let Some(building) = self.substrate.entities.get_mut(new_sid) {
            building.veterancy_raw = veterancy;
        }
        // 0x007397E4..0x007397F4: a building deployed for a house other than
        // the local player's (`IsHumanPlayer`, here whether a human controls
        // it) is AI-repairable (`+0x6CB`; its AI-rebuildable `+0x6CA` has no
        // building reader).
        if !self
            .houses
            .get(&owner_id)
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero))
            && let Some(building) = self.substrate.entities.get_mut(new_sid)
        {
            building.ai_repairable = true;
        }
        // 0x0073982C: the building takes the unit's target.
        let _ = self.assign_target_represented(new_sid, source_target, Some(rules));
        self.initialize_cloak_after_unlimbo(new_sid, rules);
        self.add_unit_sensor_after_unlimbo(new_sid, rules);
        self.mission_spawned_entities = true;
        // 0x00739956: a Slave Miner's manager moves to its refinery (the
        // hand-off 0x006B0D10, then SetOwner 0x006AF580) before the unit
        // leaves; the refinery's own fresh slaves are freed.
        self.transfer_slave_manager(stable_id, new_sid, true, rules, registry);
        self.uninit_with_context(stable_id, super::UninitContext::new(Some(rules), registry));

        if let Some((country_name, side_index, difficulty, tech_level, _)) = recalc_context {
            // The new Building's committed north-west anchor is the native
            // `+0x9C/+0xA0` source. This bounded write order is load-bearing:
            // primary center, optional Recalc, node zero, BasePlan center,
            // then the three independent House AI activation latches.
            let house = self
                .houses
                .get_mut(&owner_id)
                .expect("qualifying deploy owner remains registered");
            house.set_base_center((rx, ry));
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

    /// The player's undeploy order (VERA's stand-in for the retail undeploy
    /// click, `BuildingClass::Active_Click_With 0x004436F0`, whose SELL event
    /// follows the event that sets the building's ArchiveTarget): a building
    /// that can undeploy takes `Sell_Back(-1)`, and its Selling mission packs
    /// it up and converts it into its `UndeploysInto=` unit
    /// ([`Self::finish_undeploy`]).
    pub(crate) fn undeploy_building(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        self.can_undeploy_building_runtime(stable_id, rules)
            && production::sell_back(
                self,
                rules,
                stable_id,
                production::SellOrder::Undeploy,
                registry,
            )
    }

    /// `BuildingClass::Mission_Selling`'s UndeploysInto conversion (`0x00449CEA`), on
    /// the Selling mission's completing visit
    /// (`production::production_sell::sell_complete`). The unit is
    /// constructed at the building's undeploy cell with its
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
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some((unit_type, owner_id, rx, ry, z, was_selected)) =
            self.substrate.entities.get(sid).and_then(|entity| {
                let type_str = self.interner.resolve(entity.type_ref());
                let unit_type = production::undeploy_target(rules, type_str)?;
                let (rx, ry) = undeploy_unit_cell(
                    entity.position.rx,
                    entity.position.ry,
                    &rules.object(type_str)?.foundation,
                );
                Some((
                    unit_type,
                    entity.owner(),
                    rx,
                    ry,
                    entity.position.z,
                    entity.selected,
                ))
            })
        else {
            return;
        };
        let unit_type_id = self.interner.intern(unit_type);
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
            self.uninit_with_context(
                sid,
                super::UninitContext::new(Some(rules), overlay_registry),
            );
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
        let _ = self.techno_limbo_with_rules(sid, rules, overlay_registry);
        let (new_sid, outcome) = self.unlimbo_constructed_parent(
            new_sid,
            position,
            Some(rules),
            overlay_registry,
            false,
        );
        if !matches!(outcome, RevealOutcome::Revealed { .. }) {
            self.discard_constructed_limbo(new_sid, Some(rules));
            self.uninit_with_context(
                sid,
                super::UninitContext::new(Some(rules), overlay_registry),
            );
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
        }
        self.transfer_slave_manager(sid, new_sid, false, rules, overlay_registry);
        // A building's archive is a cell. Of its VERA writers (the Slave
        // Miner refinery's relocation in `slave_manager`, the rally click on
        // rally-line factories), only the relocation reaches an
        // UndeploysInto building: no retail rally-line type undeploys.
        if let Some(TargetKind::Cell(x, y)) = archive {
            if !self.set_unit_destination(
                new_sid,
                crate::sim::components::NavTargetRef::cell(x, y),
                rules,
                true,
            ) {
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
        self.uninit_with_context(
            sid,
            super::UninitContext::new(Some(rules), overlay_registry),
        );
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
            || entity.building_up()
            || entity.building_down()
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
        if entity.mind_control.controller().is_some() {
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
/// mirror; the two must stay inverses. Deploy adds the `(-1, -1)` step in
/// wrapping 16-bit words (`0x00739460..0x007394BA`), so a unit on row or
/// column 0 tests an origin off the map.
fn deploy_origin_from_unit_cell(unit_rx: u16, unit_ry: u16, foundation: &str) -> (i16, i16) {
    let (width, height) = foundation_dimensions(foundation);
    let (x, y) = (unit_rx as i16, unit_ry as i16);
    if width > 2 || height > 2 {
        (x.wrapping_sub(1), y.wrapping_sub(1))
    } else {
        (x, y)
    }
}

/// Resolve the deploy target for an MCV-like unit via rules.ini `DeploysInto=`.
fn construction_yard_type_for_mcv(type_id: &str, rules: &RuleSet) -> Option<String> {
    let obj = rules.object(type_id)?;
    let target: &str = obj.deploys_into.as_deref()?;
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
