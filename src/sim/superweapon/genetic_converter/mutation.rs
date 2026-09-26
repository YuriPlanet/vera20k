//! Owns mutation selection, damage and replacement admission as one operation.

use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_aoe::{
    AoELayerContext, AreaDamageReceiver, TerrainCollectionView,
    apply_aoe_damage_with_terrain_and_scenario, bridge_adjusted_impact_z,
};
use crate::sim::intern::InternedId;
use crate::sim::superweapon::cell_grid::{native_cells_3x3, selected_cell_list};
use crate::sim::world::Simulation;
use std::collections::BTreeSet;

/// Brute type_ref for Tier 1. Generalize to rules.general.animation_to_infantry[0]
/// when the full AnimClass death-to-infantry pipeline is implemented.
const BRUTE_TYPE_REF: &str = "BRUTE";

/// Exact signed damage loaded by SuperClass::Launch case 9 immediately before
/// its direct Apply_area_damage call (`MOV EDX, 0x2710`).
const MUTATE_AOE_DAMAGE: i32 = 10_000;

/// Complete the selected mutation batch and all replacement attempts before returning.
/// Current immediate BRUTE ownership, placement and corpse timing are deliberate
/// Rust compatibility policy. Native AnimToInfantry replacement remains separate.
pub(super) fn execute(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> usize {
    let killed = if rules.general.mutate_explosion {
        apply_mutate_explosion(sim, rules, target_rx, target_ry, owner, overlay_registry)
    } else {
        apply_mutate_per_cell(sim, target_rx, target_ry)
    };
    // Observe the entire killed batch before admission. Constructors must not
    // interleave with recursive damage or select freshly created replacements.
    let count = killed.len();
    let owner_name = sim.interner.resolve(owner).to_owned();
    for (rx, ry) in killed {
        spawn_brute(sim, rules, &owner_name, rx, ry);
    }
    count
}

/// MutateExplosion path: AoE damage via MutateExplosionWarhead.
/// Returns list of (rx, ry) cell positions of infantry killed.
fn apply_mutate_explosion(
    sim: &mut Simulation,
    rules: &RuleSet,
    target_rx: u16,
    target_ry: u16,
    owner: InternedId,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Vec<(u16, u16)> {
    let handles = sim.rule_handles;
    let warhead_id = rules.general.mutate_explosion_warhead.clone();
    let Some(warhead) = rules.warhead(&warhead_id) else {
        log::warn!("MutateExplosionWarhead '{}' not found in rules", warhead_id);
        return Vec::new();
    };
    let warhead_ref = sim.interner.intern(&warhead_id);
    let base_damage: i32 = MUTATE_AOE_DAMAGE;
    let impact_z = bridge_adjusted_impact_z(sim.resolved_terrain.as_ref(), target_rx, target_ry);
    let air_impact = crate::sim::combat::combat_aoe::air_impact_from_layer_z(
        sim.resolved_terrain.as_ref(),
        target_rx,
        target_ry,
        crate::util::lepton::CELL_CENTER_LEPTON,
        crate::util::lepton::CELL_CENTER_LEPTON,
        impact_z,
    );
    // Native SuperClass::Launch already constructed the launch-level
    // MutateExplosion animation before this direct Apply_area_damage call and
    // passes affect_resource=false. There is no Warhead AnimList producer here.
    let scenario_no_damage = sim.session.no_damage;
    let terrain_objects = TerrainCollectionView {
        objects: &sim.production.terrain_objects,
        cells: &sim.production.terrain_object_cells,
    };
    let aoe = apply_aoe_damage_with_terrain_and_scenario(
        &mut sim.substrate.entities,
        target_rx,
        target_ry,
        base_damage,
        warhead,
        rules,
        &sim.interner,
        handles,
        (
            crate::sim::combat::RAD_NO_ATTACKER,
            Some(owner),
            warhead_ref,
        ),
        AoELayerContext {
            occupancy: Some(&sim.substrate.occupancy),
            terrain: sim.resolved_terrain.as_mut(),
            overlay_grid: sim.overlay_grid.as_mut(),
            overlay_registry,
            scenario_rng: Some(&mut sim.scenario_rng),
            air_impact,
            impact_z,
        },
        Some(terrain_objects),
        scenario_no_damage,
        None,
    );
    let receivers = aoe.receivers;

    // Mutation is infantry-only, but its damage still enters the ordinary
    // ReceiveDamage -> death helper transaction. Snapshot the transformation
    // cells first, preserve the AoE's object-list order, then create Brutes only
    // after every nested death detonation has returned. A `JumpJet=` type's
    // death explodes ahead of the InfDeath table (`0x00518313`, before the
    // mutation arm `0x005188AE`), so it never mutates.
    let candidates: Vec<(u64, u16, u16)> = receivers
        .iter()
        .filter_map(|receiver| {
            let AreaDamageReceiver::Entity(event) = receiver else {
                return None;
            };
            sim.substrate
                .entities
                .get(event.target_id)
                .and_then(|entity| {
                    let jumpjet = sim
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.jumpjet);
                    (entity.category == EntityCategory::Infantry && !jumpjet).then_some((
                        event.target_id,
                        entity.position.rx,
                        entity.position.ry,
                    ))
                })
        })
        .collect();
    let fatal_ids: BTreeSet<_> = sim
        .commit_noncombat_aoe_receivers(rules, overlay_registry, &receivers)
        .into_iter()
        .collect();

    let killed: Vec<(u16, u16)> = candidates
        .into_iter()
        .filter_map(|(id, rx, ry)| fatal_ids.contains(&id).then_some((rx, ry)))
        .collect();
    killed
}

/// Legacy per-cell death policy over authoritative CellClass membership.
/// SuperClass::Launch case 9: GetCell 0x006CD954; Cell+0x140 & 0x100
/// 0x006CD959..0x006CD962 selects +0xE8 at 0x006CD999 or +0xE4 at
/// 0x006CD9D6. Retail gamemd.exe SHA256:
/// 1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C.
/// Source: SUPERWEAPON_LAUNCH_HANDLERS_REPORT.md; selection rechecked from bytes.
///
/// VERA-internal compatibility, gamemd equivalent UNCHECKED: native saves its
/// next link BEFORE ReceiveDamage (0x006CD9E0/0x006CDA29). This preserves Rust's
/// stable-ID snapshot, HP/dying marking and whole-batch immediate replacement;
/// it does not substitute IC's after-call cursor or introduce InfDeath=9 effects.
fn apply_mutate_per_cell(sim: &mut Simulation, target_rx: u16, target_ry: u16) -> Vec<(u16, u16)> {
    let mut ids = BTreeSet::new();
    for (x, y) in native_cells_3x3(target_rx, target_ry) {
        let Some(((rx, ry), layer)) = selected_cell_list(sim, x, y) else {
            continue;
        };
        if let Some(cell) = sim.substrate.occupancy.get(rx, ry) {
            ids.extend(cell.iter_layer(layer).map(|member| member.entity_id));
        }
    }
    // EntityStore::values formerly imposed ascending stable-ID order. Retain
    // it while excluding factory-held and other unmarked coordinate lookalikes.
    let victims: Vec<(u64, u16, u16)> = ids
        .into_iter()
        .filter_map(|id| {
            let entity = sim.substrate.entities.get(id)?;
            (entity.category == EntityCategory::Infantry
                && entity.health.current > 0
                && !entity.dying)
                .then_some((id, entity.position.rx, entity.position.ry))
        })
        .collect();

    let mut killed: Vec<(u16, u16)> = Vec::new();
    for (id, rx, ry) in &victims {
        if sim.mark_raw_mutation_victim(*id) {
            killed.push((*rx, *ry));
        }
    }
    killed
}

/// Spawn a Brute infantry at the given cell, owned by the launching player.
fn spawn_brute(sim: &mut Simulation, rules: &RuleSet, owner_name: &str, rx: u16, ry: u16) {
    let spawned = sim.spawn_object_at_height(
        BRUTE_TYPE_REF,
        owner_name,
        rx,
        ry,
        /* facing */ 0,
        /* z */ 0,
        rules,
    );
    if spawned.is_none() {
        log::warn!(
            "GeneticConverter: failed to spawn Brute for '{}' at ({},{})",
            owner_name,
            rx,
            ry
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::CellListInsertion;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::rng::SimRng;

    #[test]
    fn mutate_explosion_bridge_target_mutates_only_bridge_layer() {
        let rules = genetic_test_rules();
        let mut sim = Simulation::new();
        add_same_cell_bridge_infantry(&mut sim);
        let owner = sim.interner.intern("Americans");

        let killed = apply_mutate_explosion(&mut sim, &rules, 5, 5, owner, None);
        let count = killed.len();

        assert_eq!(count, 1);
        assert_eq!(killed, vec![(5, 5)]);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().health.current,
            100,
            "ground infantry under the bridge must not be mutated by a deck impact"
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().health.current,
            0,
            "bridge-deck infantry must be mutated by a bridge-targeted impact"
        );
        assert!(!sim.substrate.entities.get(1).unwrap().dying);
        assert!(sim.substrate.entities.get(2).unwrap().dying);
    }

    #[test]
    fn gsi_04_07_damage_gsi_04_11_mutate_explosion_exact_boundary_and_death_transaction() {
        fn run(victim_hp: i32) -> (Simulation, Vec<(u16, u16)>, usize, u64) {
            let ini = IniFile::from_str(
                "[InfantryTypes]\n0=BOOMER\n1=BRUTE\n\
                 [VehicleTypes]\n0=TANK\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [Warheads]\n0=MutateExplosion\n1=WallWH\n\
                 [OverlayTypes]\n0=TESTWALL\n\
                 [General]\nMutateExplosion=yes\n\
                 [CombatDamage]\nMaxDamage=10000\nMutateExplosionWarhead=MutateExplosion\n\
                 [BOOMER]\nStrength=10000\nArmor=none\nSpeed=4\nExplodes=yes\nDeathWeapon=DeathBoom\n\
                 [TANK]\nStrength=10000\nArmor=heavy\nSpeed=4\nExplodes=yes\nDeathWeapon=DeathBoom\n\
                 [BRUTE]\nStrength=200\nArmor=none\nSpeed=4\n\
                 [DeathBoom]\nDamage=400\nWarhead=WallWH\n\
                 [MutateExplosion]\nCellSpread=1\nPercentAtMax=1\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [WallWH]\nCellSpread=0\nWall=yes\n\
                 Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
            );
            let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
            let rules = RuleSet::from_ini(&ini).expect("mutation death transaction rules");
            let registry = OverlayTypeRegistry::from_ini(&ini, Some(&art));
            assert!(rules.warhead("MutateExplosion").is_some());
            assert!(rules.warhead("WallWH").is_some());
            for object_id in ["BOOMER", "TANK"] {
                assert_eq!(
                    rules.object(object_id).unwrap().death_weapon.as_deref(),
                    Some("DeathBoom")
                );
            }

            let mut sim = Simulation::with_seed(1);
            let owner = sim.interner.intern("Americans");
            let soviet = sim.interner.intern("Soviet");
            let mut infantry = GameEntity::test_default(10, "BOOMER", "Soviet", 5, 5);
            infantry.owner = soviet;
            infantry.type_ref = sim.interner.intern("BOOMER");
            infantry.category = EntityCategory::Infantry;
            infantry.is_voxel = false;
            infantry.health = Health { current: victim_hp };
            sim.substrate.entities.insert(infantry);
            let _ = sim.reveal(10);

            let mut unit = GameEntity::test_default(20, "TANK", "Soviet", 6, 5);
            unit.owner = soviet;
            unit.type_ref = sim.interner.intern("TANK");
            unit.health = Health { current: victim_hp };
            sim.substrate.entities.insert(unit);
            let _ = sim.reveal(20);

            let mut overlays = OverlayGrid::new(12, 12);
            overlays.place_overlay(5, 5, 0, 0);
            overlays.place_overlay(6, 5, 0, 0);
            sim.overlay_grid = Some(overlays);

            let killed = apply_mutate_explosion(&mut sim, &rules, 5, 5, owner, Some(&registry));
            let count = killed.len();
            let rng_state = sim.scenario_rng.state();
            (sim, killed, count, rng_state)
        }

        let (fatal, killed, count, fatal_rng) = run(MUTATE_AOE_DAMAGE);
        for id in [10, 20] {
            assert!(fatal.substrate.entities.get(id).is_some_and(|entity| {
                entity.health.current == 0 && entity.dying && !entity.in_logic_vector
            }));
            assert!(!fatal.live_object_order_snapshot().contains(&id));
        }
        assert_eq!((killed, count), (vec![(5, 5)], 1));
        for cell in [(5, 5), (6, 5)] {
            assert_eq!(
                fatal
                    .overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(cell.0, cell.1)
                    .overlay_id,
                None
            );
        }
        assert_eq!(
            fatal.substrate.pending_delete,
            vec![20, 10],
            "concrete Unit UnInit is inline; the synthetic non-animated Infantry fixture drains later"
        );
        assert_eq!(fatal_rng, SimRng::new(1).state());

        let (boundary, killed, count, boundary_rng) = run(MUTATE_AOE_DAMAGE + 1);
        assert_eq!((killed, count), (Vec::new(), 0));
        for (id, cell) in [(10, (5, 5)), (20, (6, 5))] {
            assert_eq!(
                boundary
                    .overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(cell.0, cell.1)
                    .overlay_id,
                Some(0)
            );
            assert!(boundary.live_object_order_snapshot().contains(&id));
            assert_eq!(
                boundary.substrate.entities.get(id).unwrap().health.current,
                1
            );
        }
        assert!(boundary.substrate.pending_delete.is_empty());
        assert_eq!(boundary_rng, SimRng::new(1).state());
    }

    /// A `JumpJet=` infantryman in the blast dies without mutating: its death
    /// builds InfantryExplode ahead of the InfDeath table (`0x00518313`, before
    /// the mutation arm `0x005188AE`), so only the rifleman becomes a Brute.
    #[test]
    fn mutate_explosion_leaves_a_jumpjet_infantryman_unmutated() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=BRUTE\n2=ROCKET\n\n\
             [VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
             [Warheads]\n0=MutateExplosion\n\n\
             [General]\nMutateExplosion=yes\n\n\
             [CombatDamage]\nMaxDamage=10000\nMutateExplosionWarhead=MutateExplosion\n\n\
             [E1]\nStrength=100\nArmor=none\nSpeed=4\n\n\
             [ROCKET]\nStrength=100\nArmor=none\nSpeed=9\nJumpJet=yes\nCrashable=yes\n\n\
             [BRUTE]\nStrength=200\nArmor=none\nSpeed=4\n\n\
             [MutateExplosion]\nCellSpread=1\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("mutation rules");
        let mut sim = Simulation::with_seed(1);
        let owner = sim.interner.intern("Americans");
        let soviet = sim.interner.intern("Soviet");
        for (id, type_id, rx) in [(10, "E1", 5), (11, "ROCKET", 6)] {
            let mut infantry = GameEntity::test_default(id, type_id, "Soviet", rx, 5);
            infantry.owner = soviet;
            infantry.type_ref = sim.interner.intern(type_id);
            infantry.category = EntityCategory::Infantry;
            infantry.is_voxel = false;
            infantry.health = Health { current: 100 };
            sim.substrate.entities.insert(infantry);
            let _ = sim.reveal(id);
        }
        let killed = apply_mutate_explosion(&mut sim, &rules, 5, 5, owner, None);
        assert_eq!(killed, vec![(5, 5)]);
        assert_eq!(sim.substrate.entities.get(11).unwrap().health.current, 0);
    }

    fn genetic_test_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=BRUTE\n\n\
             [VehicleTypes]\n\n\
             [AircraftTypes]\n\n\
             [BuildingTypes]\n\n\
             [General]\nMutateExplosion=yes\n\n\
             [CombatDamage]\nMutateExplosionWarhead=MutateExplosion\n\n\
             [E1]\nStrength=100\nArmor=none\nSpeed=4\nPrimary=DUMMYW\n\n\
             [BRUTE]\nStrength=200\nArmor=none\nSpeed=4\n\n\
             [DUMMYW]\nDamage=1\nROF=1\nRange=1\nWarhead=MutateExplosion\n\n\
             [MutateExplosion]\nCellSpread=1\nPercentAtMax=1\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("genetic test rules should parse")
    }

    fn add_same_cell_bridge_infantry(sim: &mut Simulation) {
        let owner = sim.interner.intern("Soviet");
        let type_ref = sim.interner.intern("E1");

        let mut ground = GameEntity::test_default(1, "E1", "Soviet", 5, 5);
        ground.owner = owner;
        ground.type_ref = type_ref;
        ground.category = EntityCategory::Infantry;
        ground.is_voxel = false;
        ground.health = Health { current: 100 };
        // In the cell's lists, so on the map: out of limbo and marked.
        ground.lifecycle.in_limbo = false;
        ground.lifecycle.cell_marked = true;

        let mut bridge = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        bridge.owner = owner;
        bridge.type_ref = type_ref;
        bridge.category = EntityCategory::Infantry;
        bridge.is_voxel = false;
        bridge.health = Health { current: 100 };
        bridge.on_bridge = true;
        bridge.position.z = 4;
        bridge.lifecycle.in_limbo = false;
        bridge.lifecycle.cell_marked = true;

        sim.substrate.entities.insert(ground);
        sim.substrate.entities.insert(bridge);
        sim.substrate.occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        sim.substrate.occupancy.add(
            5,
            5,
            2,
            MovementLayer::Bridge,
            Some(2),
            CellListInsertion::PrependNonBuilding,
        );
        sim.resolved_terrain = Some(bridge_terrain());
    }

    fn bridge_terrain() -> ResolvedTerrainGrid {
        let mut cells = Vec::new();
        for ry in 0..10 {
            for rx in 0..10 {
                cells.push(test_terrain_cell(rx, ry));
            }
        }
        let idx = 5 * 10 + 5;
        cells[idx].bridge_facts = BridgeCellFacts {
            raw_flags: BRIDGE_FLAG_STRUCTURAL,
            ..BridgeCellFacts::default()
        };
        cells[idx].has_bridge_deck = true;
        cells[idx].bridge_walkable = true;
        cells[idx].bridge_deck_level = 4;
        ResolvedTerrainGrid::from_cells(10, 10, cells)
    }

    fn test_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: 0,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }
}
