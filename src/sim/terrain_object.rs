//! Live map terrain object state and lifecycle helpers.
//!
//! This module owns deterministic sim state for `TerrainClass`-style objects
//! loaded from map `[Terrain]`. TIBTRE ore spawners are a derived index of this
//! live state, not the lifecycle owner.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::warhead_type::WarheadType;
use crate::sim::combat::{armor_index, damage};
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::occupancy::RawCellOccupationGrid;
use crate::sim::production::ProductionState;
use crate::sim::terrain_spawn::TerrainSpawnerState;

const TERRAIN_LIMBO_CLEAR_BIT: u8 = 0x40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TerrainObjectLifecycle {
    Live,
    Limbo,
    Destroyed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerrainObjectState {
    pub stable_id: u64,
    /// Load-only numeric `AbstractClass` identity. Stable Rust storage remains
    /// the runtime owner, but retaining this value prevents later constructor
    /// chronology from becoming an unobservable counter approximation.
    #[serde(default)]
    pub native_unique_id: Option<i32>,
    /// LogicClass membership is reconstructed from the serialized mixed order.
    #[serde(skip)]
    pub in_logic_vector: bool,
    pub type_ref: InternedId,
    pub rx: u16,
    pub ry: u16,
    pub health: i32,
    pub max_health: i32,
    pub occupation_bits: u8,
    pub lifecycle: TerrainObjectLifecycle,
}

impl std::hash::Hash for TerrainObjectState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.stable_id.hash(state);
        self.type_ref.hash(state);
        self.rx.hash(state);
        self.ry.hash(state);
        self.health.hash(state);
        self.max_health.hash(state);
        self.occupation_bits.hash(state);
        self.lifecycle.hash(state);
    }
}

impl TerrainObjectState {
    pub fn new(
        stable_id: u64,
        type_ref: InternedId,
        rx: u16,
        ry: u16,
        terrain_type: &TerrainObjectType,
        snow_theater: bool,
    ) -> Self {
        Self {
            stable_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref,
            rx,
            ry,
            health: terrain_type.strength,
            max_health: terrain_type.strength,
            occupation_bits: occupation_bits_for(terrain_type, snow_theater),
            lifecycle: TerrainObjectLifecycle::Live,
        }
    }

    pub fn cell(&self) -> (u16, u16) {
        (self.rx, self.ry)
    }

    pub fn is_live(&self) -> bool {
        self.lifecycle == TerrainObjectLifecycle::Live
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainDamageResult {
    Ignored,
    Damaged { remaining: i32 },
    Destroyed,
}

/// Captured lethal Terrain receiver state needed by the synchronous finalize tail.
///
/// The object remains represented with exact-zero health between receive and
/// finalize so a caller can run the verified nested C4 transaction first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerrainLethalDamage {
    pub(crate) stable_id: u64,
    pub(crate) cell: (u16, u16),
    pub(crate) spawns_tiberium: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerrainAreaReceiveResult {
    Ignored,
    Damaged { remaining: i32 },
    Lethal(TerrainLethalDamage),
}

/// Inputs and outputs for phase-level Terrain receiver fixtures.
/// The adapter lends these fields to a private world that runs the production
/// receiver. Production Terrain authority stays resident in Simulation; this
/// container and its transfers exist only in tests.
#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub(crate) struct TerrainAreaState {
    terrain_spawners: BTreeMap<(u16, u16), TerrainSpawnerState>,
    terrain_objects: BTreeMap<u64, TerrainObjectState>,
    terrain_object_cells: BTreeMap<(u16, u16), u64>,
    terrain_occupation_bits: BTreeMap<(u16, u16), u8>,
    tiberium_spawning_terrain_cells: BTreeSet<(u16, u16)>,
    raw_occupation: RawCellOccupationGrid,
    navigation_changed_cells: Vec<(u16, u16)>,
    finalizing_terrain: BTreeSet<u64>,
}

#[cfg(test)]
impl TerrainAreaState {
    pub(crate) fn take_fixture_progress(&mut self) -> (Vec<(u16, u16)>, BTreeSet<u64>) {
        (
            std::mem::take(&mut self.navigation_changed_cells),
            std::mem::take(&mut self.finalizing_terrain),
        )
    }
    pub(crate) fn restore_fixture_progress(
        &mut self,
        cells: Vec<(u16, u16)>,
        finalizing: BTreeSet<u64>,
    ) {
        self.navigation_changed_cells = cells;
        self.finalizing_terrain = finalizing;
    }

    pub(crate) fn take_from(
        production: &mut ProductionState,
        raw_occupation: &mut RawCellOccupationGrid,
    ) -> Self {
        Self {
            terrain_spawners: std::mem::take(&mut production.terrain_spawners),
            terrain_objects: std::mem::take(&mut production.terrain_objects),
            terrain_object_cells: std::mem::take(&mut production.terrain_object_cells),
            terrain_occupation_bits: std::mem::take(&mut production.terrain_occupation_bits),
            tiberium_spawning_terrain_cells: std::mem::take(
                &mut production.tiberium_spawning_terrain_cells,
            ),
            raw_occupation: std::mem::take(raw_occupation),
            navigation_changed_cells: Vec::new(),
            finalizing_terrain: BTreeSet::new(),
        }
    }

    /// Lend fixture fields to the adapter world, then reclaim its mutations.
    pub(crate) fn swap_authority(
        &mut self,
        production: &mut ProductionState,
        raw_occupation: &mut RawCellOccupationGrid,
    ) {
        std::mem::swap(&mut self.terrain_spawners, &mut production.terrain_spawners);
        std::mem::swap(&mut self.terrain_objects, &mut production.terrain_objects);
        std::mem::swap(
            &mut self.terrain_object_cells,
            &mut production.terrain_object_cells,
        );
        std::mem::swap(
            &mut self.terrain_occupation_bits,
            &mut production.terrain_occupation_bits,
        );
        std::mem::swap(
            &mut self.tiberium_spawning_terrain_cells,
            &mut production.tiberium_spawning_terrain_cells,
        );
        std::mem::swap(&mut self.raw_occupation, raw_occupation);
    }

    /// Return fixture fields and the receiver's ordered navigation receipts.
    pub(crate) fn restore_into(
        mut self,
        production: &mut ProductionState,
        raw_occupation: &mut RawCellOccupationGrid,
    ) -> Vec<(u16, u16)> {
        debug_assert!(
            self.finalizing_terrain.is_empty(),
            "every lethal Terrain receiver must finalize before authority restoration"
        );
        self.swap_authority(production, raw_occupation);
        self.navigation_changed_cells
    }

    pub(crate) fn navigation_changed_cells(&self) -> &[(u16, u16)] {
        &self.navigation_changed_cells
    }

    pub(crate) fn is_finalizing(&self, stable_id: u64) -> bool {
        self.finalizing_terrain.contains(&stable_id)
    }

    /// Enter the shared Object damage kernel for one captured Terrain receiver.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn receive_area_damage(
        &mut self,
        stable_id: u64,
        cell: (u16, u16),
        raw_damage: i32,
        distance_leptons: i32,
        warhead: &WarheadType,
        rules: &RuleSet,
        interner: &StringInterner,
    ) -> TerrainAreaReceiveResult {
        receive_terrain_damage_with_scenario(
            &mut self.terrain_objects,
            &self.terrain_object_cells,
            &mut self.finalizing_terrain,
            stable_id,
            cell,
            raw_damage,
            distance_leptons,
            warhead,
            rules,
            interner,
            false,
            false,
        )
    }

    /// Complete a lethal receiver after any nested C4 transaction has returned.
    pub(crate) fn finalize_lethal(
        &mut self,
        lethal: TerrainLethalDamage,
        resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    ) -> bool {
        finalize_terrain_lethal(
            TerrainAuthorityParts {
                terrain_spawners: &mut self.terrain_spawners,
                terrain_objects: &mut self.terrain_objects,
                terrain_object_cells: &mut self.terrain_object_cells,
                terrain_occupation_bits: &mut self.terrain_occupation_bits,
                tiberium_spawning_terrain_cells: &mut self.tiberium_spawning_terrain_cells,
                raw_occupation: &mut self.raw_occupation,
            },
            &mut self.finalizing_terrain,
            &mut self.navigation_changed_cells,
            lethal,
            resolved_terrain,
        )
    }
}

pub(crate) struct TerrainAuthorityParts<'a> {
    terrain_spawners: &'a mut BTreeMap<(u16, u16), TerrainSpawnerState>,
    terrain_objects: &'a mut BTreeMap<u64, TerrainObjectState>,
    terrain_object_cells: &'a mut BTreeMap<(u16, u16), u64>,
    terrain_occupation_bits: &'a mut BTreeMap<(u16, u16), u8>,
    tiberium_spawning_terrain_cells: &'a mut BTreeSet<(u16, u16)>,
    raw_occupation: &'a mut RawCellOccupationGrid,
}

pub fn occupation_bits_for(terrain_type: &TerrainObjectType, snow_theater: bool) -> u8 {
    (if snow_theater {
        terrain_type.snow_occupation_bits
    } else {
        terrain_type.temperate_occupation_bits
    }) & 0x07
}

pub(crate) fn terrain_raw_occupation_mask(source_mask: u8) -> u8 {
    (source_mask & 0x07) << 2
}

pub(crate) fn mark_terrain_raw_occupation(
    raw_occupation: &mut RawCellOccupationGrid,
    cell: (u16, u16),
    source_mask: u8,
) {
    raw_occupation.mark_ground(cell.0, cell.1, terrain_raw_occupation_mask(source_mask));
}

fn unmark_terrain_raw_occupation(
    raw_occupation: &mut RawCellOccupationGrid,
    cell: (u16, u16),
    source_mask: u8,
) {
    raw_occupation.clear_ground(cell.0, cell.1, terrain_raw_occupation_mask(source_mask));
}

pub fn mark_terrain_occupation(
    production: &mut ProductionState,
    terrain: &TerrainObjectState,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) {
    let cell = terrain.cell();
    if terrain.occupation_bits != 0 {
        production
            .terrain_occupation_bits
            .insert(cell, terrain.occupation_bits);
    } else {
        production.terrain_occupation_bits.remove(&cell);
    }
    if let Some(grid) = resolved_terrain {
        grid.set_terrain_object_occupation(cell, Some(terrain.occupation_bits));
    }
}

pub fn unmark_terrain_occupation(
    production: &mut ProductionState,
    terrain: &TerrainObjectState,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) {
    let cell = terrain.cell();
    production.terrain_occupation_bits.remove(&cell);
    if let Some(grid) = resolved_terrain {
        grid.set_terrain_object_occupation(cell, None);
    }
}

#[cfg(test)]
pub(crate) fn limbo_terrain_object_at_cell(
    production: &mut ProductionState,
    cell: (u16, u16),
    raw_occupation: &mut RawCellOccupationGrid,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) -> bool {
    limbo_terrain_object_at_cell_parts(
        production_authority_parts(production, raw_occupation),
        cell,
        resolved_terrain,
    )
    .is_some()
}

pub(crate) fn production_authority_parts<'a>(
    production: &'a mut ProductionState,
    raw_occupation: &'a mut RawCellOccupationGrid,
) -> TerrainAuthorityParts<'a> {
    TerrainAuthorityParts {
        terrain_spawners: &mut production.terrain_spawners,
        terrain_objects: &mut production.terrain_objects,
        terrain_object_cells: &mut production.terrain_object_cells,
        terrain_occupation_bits: &mut production.terrain_occupation_bits,
        tiberium_spawning_terrain_cells: &mut production.tiberium_spawning_terrain_cells,
        raw_occupation,
    }
}

fn limbo_terrain_object_at_cell_parts(
    authority: TerrainAuthorityParts<'_>,
    cell: (u16, u16),
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) -> Option<u64> {
    let stable_id = *authority.terrain_object_cells.get(&cell)?;
    let snapshot = authority.terrain_objects.get(&stable_id)?.clone();
    if !snapshot.is_live() {
        return None;
    }

    let source_cell = snapshot.cell();
    authority
        .raw_occupation
        .clear_ground(source_cell.0, source_cell.1, TERRAIN_LIMBO_CLEAR_BIT);
    authority.terrain_object_cells.remove(&cell);
    unmark_terrain_raw_occupation(
        authority.raw_occupation,
        source_cell,
        snapshot.occupation_bits,
    );
    authority.terrain_occupation_bits.remove(&source_cell);
    if let Some(grid) = resolved_terrain {
        grid.set_terrain_object_occupation(source_cell, None);
    }
    if let Some(terrain) = authority.terrain_objects.get_mut(&stable_id) {
        terrain.lifecycle = TerrainObjectLifecycle::Limbo;
    }
    authority.terrain_spawners.remove(&source_cell);
    authority
        .tiberium_spawning_terrain_cells
        .remove(&source_cell);
    Some(stable_id)
}

#[cfg(test)]
pub(crate) fn damage_terrain_object_at_cell(
    production: &mut ProductionState,
    raw_occupation: &mut RawCellOccupationGrid,
    rules: &RuleSet,
    interner: &StringInterner,
    cell: (u16, u16),
    base_damage: i32,
    warhead: &WarheadType,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) -> TerrainDamageResult {
    let mut area_state = TerrainAreaState::take_from(production, raw_occupation);
    let receive_result = area_state.terrain_object_cells.get(&cell).copied().map_or(
        TerrainAreaReceiveResult::Ignored,
        |stable_id| {
            area_state.receive_area_damage(
                stable_id,
                cell,
                base_damage,
                0,
                warhead,
                rules,
                interner,
            )
        },
    );
    let result = match receive_result {
        TerrainAreaReceiveResult::Ignored => TerrainDamageResult::Ignored,
        TerrainAreaReceiveResult::Damaged { remaining } => {
            TerrainDamageResult::Damaged { remaining }
        }
        TerrainAreaReceiveResult::Lethal(lethal) => {
            if area_state.finalize_lethal(lethal, resolved_terrain) {
                TerrainDamageResult::Destroyed
            } else {
                TerrainDamageResult::Ignored
            }
        }
    };
    let _ = area_state.restore_into(production, raw_occupation);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::overlay::TerrainObject;
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::MovementZone;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use crate::sim::movement::bump_crush::{
        CrushCapability, build_blocker_neighbor_counts, collect_crush_victims,
    };
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::pathfinding::cell_entry::{
        CanEnterCellContext, CanEnterCellResult, TerrainEntryMode, evaluate_can_enter_cell,
    };
    use crate::sim::terrain_spawn::seed_terrain_spawners;
    use crate::sim::world::Simulation;

    fn terrain_rules(type_name: &str, wood: bool, type_section: &str) -> RuleSet {
        let wood = if wood { "yes" } else { "no" };
        let ini = IniFile::from_str(&format!(
            "[General]\nTreeStrength=10\n\
              [InfantryTypes]\n\
              [VehicleTypes]\n0=DUMMY\n\
              [AircraftTypes]\n\
              [BuildingTypes]\n\
              [TerrainTypes]\n0={}\n\
              [DUMMY]\nPrimary=Gun\nStrength=100\nArmor=heavy\n\
              [Gun]\nDamage=10\nWarhead=WH\n\
              [WH]\nWood={}\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
              [{}]\nFixtureOnly=1\n{}",
            type_name, wood, type_name, type_section
        ));
        RuleSet::from_ini(&ini).expect("rules")
    }

    fn terrain_kernel_rules(warhead_section: &str, max_damage: i32) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\nTreeStrength=100\n\
              [CombatDamage]\nMaxDamage={}\n\
              [InfantryTypes]\n\
              [VehicleTypes]\n0=DUMMY\n\
              [AircraftTypes]\n\
              [BuildingTypes]\n\
              [TerrainTypes]\n0=TREE01\n\
              [DUMMY]\nPrimary=Gun\nStrength=100\nArmor=heavy\n\
              [Gun]\nDamage=10\nWarhead=WH\n\
              [WH]\nWood=yes\n{}\n\
              [TREE01]\nStrength=100\nArmor=wood\nTemperateOccupationBits=7\n",
            max_damage, warhead_section
        ));
        RuleSet::from_ini(&ini).expect("kernel rules")
    }

    #[test]
    fn bridge_direct_terrain_damage_bypasses_kernel_and_finishes_removal() {
        let rules = terrain_kernel_rules(
            "Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\nCellSpread=1\nPercentAtMax=0",
            1,
        );
        let mut sim = Simulation::new();
        sim.session.no_damage = true;
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];
        let health = sim.production.terrain_objects[&stable_id].health;
        let warhead_ref = sim.interner.intern("WH");
        sim.commit_direct_terrain_damage_receiver(
            &rules,
            None,
            crate::sim::combat::TerrainDamageEvent {
                stable_id,
                rx: 0,
                ry: 0,
                damage: health,
                distance_leptons: 512,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
        );
        let terrain = &sim.production.terrain_objects[&stable_id];
        assert_eq!(terrain.health, 0);
        assert_eq!(terrain.lifecycle, TerrainObjectLifecycle::Destroyed);
        assert!(
            !terrain.in_logic_vector,
            "direct receiver finishes retirement before returning"
        );
        assert!(!sim.production.terrain_object_cells.contains_key(&(0, 0)));
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0);
    }

    #[test]
    fn bridge_direct_terrain_damage_keeps_wood_and_immune_gates() {
        for (wood, section) in [(false, ""), (true, "Immune=yes\n")] {
            let rules = terrain_rules("TREE01", wood, section);
            let mut sim = Simulation::new();
            seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
            let stable_id = sim.production.terrain_object_cells[&(0, 0)];
            let health = sim.production.terrain_objects[&stable_id].health;
            let warhead_ref = sim.interner.intern("WH");
            sim.commit_direct_terrain_damage_receiver(
                &rules,
                None,
                crate::sim::combat::TerrainDamageEvent {
                    stable_id,
                    rx: 0,
                    ry: 0,
                    damage: health,
                    distance_leptons: 0,
                    warhead_ref,
                    near_center_ic_isolation_eligible: false,
                },
            );
            assert_eq!(sim.production.terrain_objects[&stable_id].health, health);
            assert!(sim.production.terrain_objects[&stable_id].is_live());
            assert_eq!(sim.production.terrain_object_cells[&(0, 0)], stable_id);
        }
    }

    #[test]
    fn bridge_direct_terrain_receiver_rejects_zero_health_during_nested_death() {
        let rules = terrain_rules("TREE01", true, "");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];
        // During a nested death callback, Health is zero before outer Limbo.
        sim.production
            .terrain_objects
            .get_mut(&stable_id)
            .unwrap()
            .health = 0;
        let warhead_ref = sim.interner.intern("WH");
        sim.commit_direct_terrain_damage_receiver(
            &rules,
            None,
            crate::sim::combat::TerrainDamageEvent {
                stable_id,
                rx: 0,
                ry: 0,
                damage: -10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
        );
        assert_eq!(sim.production.terrain_objects[&stable_id].health, 0);
        assert!(
            sim.production.terrain_objects[&stable_id].is_live(),
            "outer receiver still owns finalization"
        );
    }

    fn rules(tib_section: &str) -> RuleSet {
        let mut rules = terrain_rules(
            "TIBTRE01",
            true,
            &format!(
                "SpawnsTiberium=yes\nIsAnimated=yes\nAnimationRate=3\nAnimationProbability=1\n{}",
                tib_section
            ),
        );
        rules.set_terrain_spawner_frame_count_for_test("TIBTRE01", 22);
        rules
    }

    fn seed_one(sim: &mut Simulation, rules: &RuleSet) {
        seed_one_at(sim, rules, "TIBTRE01", (10, 11));
    }

    fn seed_one_at(sim: &mut Simulation, rules: &RuleSet, type_name: &str, cell: (u16, u16)) {
        seed_terrain_spawners(
            sim,
            &[TerrainObject {
                rx: cell.0,
                ry: cell.1,
                name: type_name.to_string(),
            }],
            rules,
            false,
        );
    }

    fn resolved_clear_grid() -> ResolvedTerrainGrid {
        ResolvedTerrainGrid::from_cells(1, 1, vec![ResolvedTerrainCell::clear_for_test(0, 0)])
    }

    fn resolved_clear_grid_3x3() -> ResolvedTerrainGrid {
        let template = resolved_clear_grid()
            .cell(0, 0)
            .expect("clear template")
            .clone();
        let mut cells = Vec::with_capacity(9);
        for ry in 0..3 {
            for rx in 0..3 {
                cells.push(ResolvedTerrainCell {
                    rx,
                    ry,
                    ..template.clone()
                });
            }
        }
        ResolvedTerrainGrid::from_cells(3, 3, cells)
    }

    #[test]
    fn gsi_04_10_crushers_do_not_admit_or_remove_terrain_even_when_crushable_yes() {
        let rules = terrain_rules("TREE01", true, "Crushable=yes\nTemperateOccupationBits=4\n");
        let mut sim = Simulation::new();
        let source_cell = (1, 1);
        seed_one_at(&mut sim, &rules, "TREE01", source_cell);
        let stable_id = sim.production.terrain_object_cells[&source_cell];
        let before = sim.production.terrain_objects[&stable_id].clone();

        let mut resolved = resolved_clear_grid_3x3();
        mark_terrain_occupation(&mut sim.production, &before, Some(&mut resolved));
        let path_grid = PathGrid::from_resolved_terrain(&resolved);
        let blocker_counts = build_blocker_neighbor_counts(
            &sim.substrate.entities,
            3,
            3,
            Some(&resolved),
            &sim.interner,
            None,
        );
        let blocker_counts_ref = &blocker_counts;
        let before_neighbor_total = (0..3)
            .flat_map(|ry| (0..3).map(move |rx| blocker_counts_ref.count_at(rx, ry) as u32))
            .sum::<u32>();
        let before_zone = resolved
            .cell(source_cell.0, source_cell.1)
            .expect("Terrain source")
            .zone_type;

        for movement_zone in [MovementZone::Crusher, MovementZone::CrusherAll] {
            assert_eq!(
                evaluate_can_enter_cell(CanEnterCellContext {
                    wall: None,
                    target: source_cell,
                    terrain_layer: MovementLayer::Ground,
                    movement_zone: Some(movement_zone),
                    speed_type: None,
                    path_grid: Some(&path_grid),
                    resolved_terrain: Some(&resolved),
                    terrain_costs: None,
                    bypass_grid: false,
                    mode: TerrainEntryMode::RuntimeTransition,
                    is_infantry: false,
                    mover_is_crusher: false,
                }),
                CanEnterCellResult::HardBlocked,
                "Terrain's ObjectClass identity blocks entry even when custom rules spell Crushable=yes"
            );
        }

        for capability in [
            CrushCapability::new(true, false),
            CrushCapability::new(false, true),
        ] {
            assert!(
                collect_crush_victims(
                    source_cell,
                    &sim.substrate.occupancy,
                    MovementLayer::Ground,
                    capability,
                    &sim.substrate.entities,
                    // A terrain object is never a crush victim whoever asks, so
                    // the ally gate is irrelevant here; a neutral crusher keeps
                    // the assertion about terrain rather than about alliance.
                    crate::sim::movement::bump_crush::CrushAllyGate::new(
                        "Neutral",
                        &crate::map::houses::HouseAllianceMap::new(),
                        &sim.interner,
                    ),
                )
                .is_empty(),
                "Terrain never enters the Techno crush-victim list"
            );
        }

        let after = &sim.production.terrain_objects[&stable_id];
        assert_eq!(sim.production.terrain_object_cells[&source_cell], stable_id);
        assert_eq!(after.lifecycle, TerrainObjectLifecycle::Live);
        assert_eq!(after.health, before.health);
        assert_eq!(
            resolved
                .cell(source_cell.0, source_cell.1)
                .unwrap()
                .terrain_object_occupation,
            Some(4)
        );
        assert_eq!(before_neighbor_total, 8);
        assert_eq!(before_zone, zone_class::BUILDING);
        assert_eq!(
            resolved
                .cell(source_cell.0, source_cell.1)
                .expect("unchanged Terrain source")
                .zone_type,
            before_zone
        );
    }

    #[test]
    fn gsi_04_10_terrain_area_receive_uses_shared_fractional_kernel_and_max_damage() {
        let fractional_rules = terrain_kernel_rules(
            "CellSpread=1\n\
             PercentAtMax=0.5\n\
             Verses=100%,100%,100%,100%,100%,100%,50.5%,100%,100%,0%,0%",
            10_000,
        );
        let mut fractional = Simulation::new();
        seed_one_at(&mut fractional, &fractional_rules, "TREE01", (0, 0));
        let stable_id = fractional.production.terrain_object_cells[&(0, 0)];
        let warhead = fractional_rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut fractional.production,
            &mut fractional.substrate.raw_cell_occupation,
        );
        assert_eq!(
            area.receive_area_damage(
                stable_id,
                (0, 0),
                99,
                128,
                warhead,
                &fractional_rules,
                &fractional.interner,
            ),
            TerrainAreaReceiveResult::Damaged { remaining: 63 }
        );
        let _ = area.restore_into(
            &mut fractional.production,
            &mut fractional.substrate.raw_cell_occupation,
        );

        let capped_rules = terrain_kernel_rules(
            "CellSpread=0\n\
             PercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,200%,100%,100%,0%,0%",
            25,
        );
        let mut capped = Simulation::new();
        seed_one_at(&mut capped, &capped_rules, "TREE01", (0, 0));
        let stable_id = capped.production.terrain_object_cells[&(0, 0)];
        let warhead = capped_rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut capped.production,
            &mut capped.substrate.raw_cell_occupation,
        );
        assert_eq!(
            area.receive_area_damage(
                stable_id,
                (0, 0),
                8_000,
                0,
                warhead,
                &capped_rules,
                &capped.interner,
            ),
            TerrainAreaReceiveResult::Damaged { remaining: 75 }
        );
        let _ = area.restore_into(
            &mut capped.production,
            &mut capped.substrate.raw_cell_occupation,
        );
    }

    #[test]
    fn gsi_04_10_terrain_area_receive_heals_near_and_clamps_to_max() {
        let rules = terrain_rules("TREE01", true, "TemperateOccupationBits=7\n");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];
        let warhead = rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
        );
        area.terrain_objects.get_mut(&stable_id).unwrap().health = 4;

        assert_eq!(
            area.receive_area_damage(stable_id, (0, 0), -50, 7, warhead, &rules, &sim.interner,),
            TerrainAreaReceiveResult::Damaged { remaining: 10 }
        );
        area.terrain_objects.get_mut(&stable_id).unwrap().health = 4;
        assert_eq!(
            area.receive_area_damage(stable_id, (0, 0), -50, 8, warhead, &rules, &sim.interner,),
            TerrainAreaReceiveResult::Ignored
        );
        assert_eq!(area.terrain_objects[&stable_id].health, 4);
        let _ = area.restore_into(&mut sim.production, &mut sim.substrate.raw_cell_occupation);
    }

    #[test]
    fn gsi_04_10_terrain_area_lethal_is_exact_zero_guarded_then_finalized_once() {
        let rules = rules("Immune=no\nTemperateOccupationBits=7\n");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TIBTRE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];
        let terrain = sim.production.terrain_objects[&stable_id].clone();
        let mut grid = resolved_clear_grid();
        mark_terrain_occupation(&mut sim.production, &terrain, Some(&mut grid));
        let warhead = rules.warhead("WH").expect("warhead");
        let mut area = TerrainAreaState::take_from(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
        );

        let TerrainAreaReceiveResult::Lethal(lethal) =
            area.receive_area_damage(stable_id, (0, 0), 100, 0, warhead, &rules, &sim.interner)
        else {
            panic!("expected lethal Terrain receiver");
        };
        assert!(lethal.spawns_tiberium);
        assert_eq!(area.terrain_objects[&stable_id].health, 0);
        assert_eq!(
            area.terrain_objects[&stable_id].lifecycle,
            TerrainObjectLifecycle::Live
        );
        assert!(area.is_finalizing(stable_id));
        assert_eq!(
            area.receive_area_damage(stable_id, (0, 0), 100, 0, warhead, &rules, &sim.interner,),
            TerrainAreaReceiveResult::Ignored,
            "nested Wood=yes C4 reentry must not receive or finalize twice"
        );

        assert!(area.finalize_lethal(lethal, Some(&mut grid)));
        assert!(!area.finalize_lethal(lethal, Some(&mut grid)));
        assert!(!area.is_finalizing(stable_id));
        assert_eq!(area.navigation_changed_cells(), &[(0, 0)]);
        assert_eq!(
            area.terrain_objects[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
        assert_eq!(area.raw_occupation.ground_bits(0, 0), 0);
        assert!(!area.terrain_object_cells.contains_key(&(0, 0)));
        assert!(!area.terrain_spawners.contains_key(&(0, 0)));
        assert!(!area.tiberium_spawning_terrain_cells.contains(&(0, 0)));
        assert!(!area.terrain_occupation_bits.contains_key(&(0, 0)));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, None);
        assert!(!cell.terrain_object_blocks);

        let changed =
            area.restore_into(&mut sim.production, &mut sim.substrate.raw_cell_occupation);
        assert_eq!(changed, vec![(0, 0)]);
        assert_eq!(
            sim.production.terrain_objects[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
    }

    #[test]
    fn gsi_04_04_runtime_terrain_occupation_uses_shared_zone_writer() {
        let rules = rules("TemperateOccupationBits=7\n");
        let terrain_type = rules
            .terrain_object_type_case_insensitive("TIBTRE01")
            .expect("terrain type");
        let mut interner = StringInterner::default();
        let type_ref = interner.intern("TIBTRE01");
        let mut terrain = TerrainObjectState::new(1, type_ref, 0, 0, terrain_type, false);
        let mut production = ProductionState::default();
        let mut grid = resolved_clear_grid();

        mark_terrain_occupation(&mut production, &terrain, Some(&mut grid));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, Some(7));
        assert!(cell.terrain_object_blocks);
        assert_eq!(cell.zone_type, zone_class::WALL);

        unmark_terrain_occupation(&mut production, &terrain, Some(&mut grid));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, None);
        assert!(!cell.terrain_object_blocks);
        assert_eq!(cell.zone_type, zone_class::GROUND);
        assert_eq!(cell.land_type, LandType::Clear.as_index());

        terrain.occupation_bits = 4;
        mark_terrain_occupation(&mut production, &terrain, Some(&mut grid));
        assert_eq!(grid.cell(0, 0).unwrap().zone_type, zone_class::BUILDING);
        unmark_terrain_occupation(&mut production, &terrain, Some(&mut grid));

        terrain.occupation_bits = 0;
        mark_terrain_occupation(&mut production, &terrain, Some(&mut grid));
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, Some(0));
        assert!(!cell.terrain_object_blocks);
        assert_eq!(cell.zone_type, zone_class::BUILDING);

        let cell = grid.cell_mut(0, 0).unwrap();
        cell.overlay_zone_type = Some(zone_class::GROUND);
        terrain.occupation_bits = 7;
        mark_terrain_occupation(&mut production, &terrain, Some(&mut grid));
        assert_eq!(
            grid.cell(0, 0).unwrap().zone_type,
            zone_class::GROUND,
            "terminal rubble result outranks terrain occupation"
        );
    }

    #[test]
    fn gsi_04_10_stock_immune_tibtree_ignores_wood_damage_and_keeps_spawner() {
        let rules = rules("Immune=yes\n");
        let mut sim = Simulation::new();
        seed_one(&mut sim, &rules);
        let warhead = rules.warhead("WH").expect("warhead");

        let result = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (10, 11),
            10,
            warhead,
            None,
        );

        assert_eq!(result, TerrainDamageResult::Ignored);
        assert!(sim.production.terrain_spawners.contains_key(&(10, 11)));
        let terrain = sim.production.terrain_objects.values().next().unwrap();
        assert_eq!(terrain.lifecycle, TerrainObjectLifecycle::Live);
        assert_eq!(terrain.health, 10);
    }

    #[test]
    fn gsi_04_10_nonimmune_tibtree_death_limbos_object_and_removes_spawner_indices() {
        let rules = rules("Immune=no\n");
        let mut sim = Simulation::new();
        seed_one(&mut sim, &rules);
        let stable_id = *sim.production.terrain_object_cells.get(&(10, 11)).unwrap();
        let warhead = rules.warhead("WH").expect("warhead");

        let result = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (10, 11),
            10,
            warhead,
            None,
        );

        assert_eq!(result, TerrainDamageResult::Destroyed);
        assert!(!sim.production.terrain_object_cells.contains_key(&(10, 11)));
        assert!(!sim.production.terrain_spawners.contains_key(&(10, 11)));
        assert!(
            !sim.production
                .tiberium_spawning_terrain_cells
                .contains(&(10, 11))
        );
        assert!(
            !sim.production
                .terrain_occupation_bits
                .contains_key(&(10, 11))
        );
        assert_eq!(
            sim.production.terrain_objects[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
    }

    #[test]
    fn gsi_04_10_non_wood_and_immune_damage_do_not_mutate_ordinary_tree() {
        for (wood, type_section) in [(false, ""), (true, "Immune=yes\n")] {
            let rules = terrain_rules("TREE01", wood, type_section);
            let mut sim = Simulation::new();
            seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
            let stable_id = sim.production.terrain_object_cells[&(0, 0)];
            let terrain = sim.production.terrain_objects[&stable_id].clone();
            let mut grid = resolved_clear_grid();
            mark_terrain_occupation(&mut sim.production, &terrain, Some(&mut grid));
            let before = sim.production.terrain_objects[&stable_id].clone();
            let warhead = rules.warhead("WH").expect("warhead");

            let result = damage_terrain_object_at_cell(
                &mut sim.production,
                &mut sim.substrate.raw_cell_occupation,
                &rules,
                &sim.interner,
                (0, 0),
                5,
                warhead,
                Some(&mut grid),
            );

            assert_eq!(result, TerrainDamageResult::Ignored);
            assert_eq!(sim.production.terrain_objects[&stable_id], before);
            assert_eq!(sim.production.terrain_object_cells[&(0, 0)], stable_id);
            assert_eq!(sim.production.terrain_occupation_bits[&(0, 0)], 7);
            let cell = grid.cell(0, 0).unwrap();
            assert_eq!(cell.terrain_object_occupation, Some(7));
            assert!(cell.terrain_object_blocks);
        }
    }

    #[test]
    fn gsi_04_10_wood_sublethal_damage_changes_only_health() {
        let rules = terrain_rules("TREE01", true, "");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];
        let terrain = sim.production.terrain_objects[&stable_id].clone();
        let mut grid = resolved_clear_grid();
        mark_terrain_occupation(&mut sim.production, &terrain, Some(&mut grid));
        let mut expected = sim.production.terrain_objects[&stable_id].clone();
        expected.health = 6;
        let warhead = rules.warhead("WH").expect("warhead");

        let result = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            4,
            warhead,
            Some(&mut grid),
        );

        assert_eq!(result, TerrainDamageResult::Damaged { remaining: 6 });
        assert_eq!(sim.production.terrain_objects[&stable_id], expected);
        assert_eq!(sim.production.terrain_object_cells[&(0, 0)], stable_id);
        assert_eq!(sim.production.terrain_occupation_bits[&(0, 0)], 7);
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, Some(7));
        assert!(cell.terrain_object_blocks);
    }

    #[test]
    fn gsi_04_10_lethal_ordinary_tree_uninits_and_clears_spatial_authority_same_call() {
        let rules = terrain_rules("TREE01", true, "");
        let mut sim = Simulation::new();
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];
        let terrain = sim.production.terrain_objects[&stable_id].clone();
        let mut grid = resolved_clear_grid();
        mark_terrain_occupation(&mut sim.production, &terrain, Some(&mut grid));
        assert!(grid.cell(0, 0).unwrap().terrain_object_blocks);
        let warhead = rules.warhead("WH").expect("warhead");

        let result = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            10,
            warhead,
            Some(&mut grid),
        );

        assert_eq!(result, TerrainDamageResult::Destroyed);
        assert!(!sim.production.terrain_object_cells.contains_key(&(0, 0)));
        assert!(!sim.production.terrain_occupation_bits.contains_key(&(0, 0)));
        assert!(!sim.production.terrain_spawners.contains_key(&(0, 0)));
        assert!(
            !sim.production
                .tiberium_spawning_terrain_cells
                .contains(&(0, 0))
        );
        assert_eq!(
            sim.production.terrain_objects[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.terrain_object_occupation, None);
        assert!(!cell.terrain_object_blocks);
        assert!(!cell.ground_walk_blocked);
        assert!(!cell.build_blocked);
        assert_eq!(cell.zone_type, zone_class::GROUND);
    }

    #[test]
    fn gsi_04_12_terrain_raw_occupation_live_limbo_clears_native_bits_only() {
        let rules = terrain_rules("TREE01", true, "TemperateOccupationBits=7\n");
        let mut sim = Simulation::new();
        sim.substrate.raw_cell_occupation.mark_ground(0, 0, 0xE0);
        sim.substrate.raw_cell_occupation.mark_deck(0, 0, 0xA5);
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];

        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xFC);
        let did_limbo = limbo_terrain_object_at_cell(
            &mut sim.production,
            (0, 0),
            &mut sim.substrate.raw_cell_occupation,
            None,
        );

        assert!(did_limbo);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xA0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(0, 0), 0xA5);
        assert!(!sim.production.terrain_object_cells.contains_key(&(0, 0)));
        assert!(!sim.production.terrain_occupation_bits.contains_key(&(0, 0)));
        assert_eq!(
            sim.production.terrain_objects[&stable_id].lifecycle,
            TerrainObjectLifecycle::Limbo
        );
    }

    #[test]
    fn gsi_04_12_terrain_raw_occupation_damage_preserves_until_lethal_limbo() {
        let rules = terrain_rules("TREE01", true, "TemperateOccupationBits=7\n");
        let mut sim = Simulation::new();
        sim.substrate.raw_cell_occupation.mark_ground(0, 0, 0xE0);
        sim.substrate.raw_cell_occupation.mark_deck(0, 0, 0x5A);
        seed_one_at(&mut sim, &rules, "TREE01", (0, 0));
        let stable_id = sim.production.terrain_object_cells[&(0, 0)];
        let warhead = rules.warhead("WH").expect("warhead");

        let ignored = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            0,
            warhead,
            None,
        );
        assert_eq!(ignored, TerrainDamageResult::Ignored);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xFC);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(0, 0), 0x5A);

        let damaged = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            4,
            warhead,
            None,
        );
        assert_eq!(damaged, TerrainDamageResult::Damaged { remaining: 6 });
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xFC);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(0, 0), 0x5A);
        assert_eq!(sim.production.terrain_occupation_bits[&(0, 0)], 7);

        let destroyed = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            (0, 0),
            6,
            warhead,
            None,
        );
        assert_eq!(destroyed, TerrainDamageResult::Destroyed);
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 0), 0xA0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(0, 0), 0x5A);
        assert_eq!(
            sim.production.terrain_objects[&stable_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
    }
}

/// Receive against live Terrain maps. The recursion guard belongs to the
/// outer damage operation; no persisted authority leaves ProductionState.
#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_terrain_damage_with_scenario(
    terrain_objects: &mut BTreeMap<u64, TerrainObjectState>,
    terrain_object_cells: &BTreeMap<(u16, u16), u64>,
    finalizing_terrain: &mut BTreeSet<u64>,
    stable_id: u64,
    cell: (u16, u16),
    raw_damage: i32,
    distance_leptons: i32,
    warhead: &WarheadType,
    rules: &RuleSet,
    interner: &StringInterner,
    scenario_no_damage: bool,
    ignore_defenses: bool,
) -> TerrainAreaReceiveResult {
    if finalizing_terrain.contains(&stable_id)
        || terrain_object_cells.get(&cell) != Some(&stable_id)
    {
        return TerrainAreaReceiveResult::Ignored;
    }

    let Some(snapshot) = terrain_objects.get(&stable_id) else {
        return TerrainAreaReceiveResult::Ignored;
    };
    // ObjectClass::ReceiveDamage 005F53A1..005F53B7 rejects these before
    // either ordinary armor calculation or the forced direct-damage path.
    if !snapshot.is_live() || snapshot.cell() != cell || snapshot.health <= 0 || raw_damage == 0 {
        return TerrainAreaReceiveResult::Ignored;
    }
    let Some(terrain_type) =
        rules.terrain_object_type_case_insensitive(interner.resolve(snapshot.type_ref))
    else {
        return TerrainAreaReceiveResult::Ignored;
    };
    if !warhead.wood || terrain_type.immune {
        return TerrainAreaReceiveResult::Ignored;
    }

    // TerrainClass 0071B920 always checks Wood/Immune above, then forwards
    // ignore_defenses to ObjectClass 005F5390. BlowUpBridge 0047DD70 passes
    // true: current Health bypasses verses, falloff, NoDamage and MaxDamage.
    let resolved_damage = if ignore_defenses {
        raw_damage
    } else {
        damage::kernel::apply_warhead_damage(
            raw_damage,
            warhead.cell_spread_f64,
            warhead.percent_at_max_f64,
            &warhead.verses_f64,
            damage::ArmorClass(armor_index(&terrain_type.armor) as u8),
            distance_leptons,
            scenario_no_damage,
            rules.combat_damage.max_damage,
        )
    };
    if resolved_damage == 0 {
        return TerrainAreaReceiveResult::Ignored;
    }

    let terrain = terrain_objects
        .get_mut(&stable_id)
        .expect("captured Terrain receiver remains represented");
    if resolved_damage < 0 {
        terrain.health = terrain
            .health
            .wrapping_sub(resolved_damage)
            .min(terrain.max_health);
        return TerrainAreaReceiveResult::Damaged {
            remaining: terrain.health,
        };
    }

    let remaining = terrain.health.wrapping_sub(resolved_damage);
    if remaining > 0 {
        terrain.health = remaining;
        return TerrainAreaReceiveResult::Damaged { remaining };
    }

    terrain.health = 0;
    finalizing_terrain.insert(stable_id);
    TerrainAreaReceiveResult::Lethal(TerrainLethalDamage {
        stable_id,
        cell,
        spawns_tiberium: terrain_type.spawns_tiberium,
    })
}

/// Finalize only after nested C4 receivers return, against the same live maps.
pub(crate) fn finalize_terrain_lethal(
    authority: TerrainAuthorityParts<'_>,
    finalizing_terrain: &mut BTreeSet<u64>,
    navigation_changed_cells: &mut Vec<(u16, u16)>,
    lethal: TerrainLethalDamage,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
) -> bool {
    if !finalizing_terrain.contains(&lethal.stable_id) {
        return false;
    }
    if authority.terrain_object_cells.get(&lethal.cell) != Some(&lethal.stable_id) {
        finalizing_terrain.remove(&lethal.stable_id);
        return false;
    }

    let removed_id = limbo_terrain_object_at_cell_parts(
        TerrainAuthorityParts {
            terrain_spawners: &mut *authority.terrain_spawners,
            terrain_objects: &mut *authority.terrain_objects,
            terrain_object_cells: &mut *authority.terrain_object_cells,
            terrain_occupation_bits: &mut *authority.terrain_occupation_bits,
            tiberium_spawning_terrain_cells: &mut *authority.tiberium_spawning_terrain_cells,
            raw_occupation: &mut *authority.raw_occupation,
        },
        lethal.cell,
        resolved_terrain,
    );
    let finalized = removed_id == Some(lethal.stable_id);
    if finalized {
        if let Some(terrain) = authority.terrain_objects.get_mut(&lethal.stable_id) {
            terrain.lifecycle = TerrainObjectLifecycle::Destroyed;
        }
        if !navigation_changed_cells.contains(&lethal.cell) {
            navigation_changed_cells.push(lethal.cell);
        }
    }
    finalizing_terrain.remove(&lethal.stable_id);
    finalized
}
