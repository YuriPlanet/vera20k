//! Shared `SelectAnim @ 0x0048A4F0`, after the caller's land/height decision.
//!
//! Native comparisons: tools/projectile_oracle/ifv_select_anim.{py,json,md}.
//! Selection coordinates and constructor coordinates are separate: Bullet
//! 469BCF supplies its live ObjectClass position even after bridge damage has
//! changed the terrain beneath the copied detonation coordinate.

use super::{ExplosionEffect, ProjectileCoord, RuleSet, WarheadType};
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback_leptons};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

fn cell_at(world: &Simulation, coord: ProjectileCoord) -> CellRef<'_> {
    if world.resolved_terrain.is_some() {
        get_cellclass_fallback_leptons(world.resolved_terrain.as_ref(), coord.x, coord.y)
    } else {
        let cell = world.effective_shared_cell_dummy();
        cell.stamp_coord(coord.x / 256, coord.y / 256);
        CellRef::Dummy { cell }
    }
}

pub(crate) fn land_at(world: &Simulation, coord: ProjectileCoord) -> i32 {
    match cell_at(world, coord) {
        CellRef::Real(cell) => i32::from(cell.yr_cell_land_type),
        CellRef::Dummy { cell } => cell.land_type(),
    }
}

/// Object::GetHeight and the first-ground-object gate at Bullet469AF0..469BCF.
pub(crate) fn bullet_land(
    world: &Simulation,
    rules: &RuleSet,
    coordinate: ProjectileCoord,
    on_bridge: bool,
    receiver_dispatched: bool,
) -> i32 {
    let floor = crate::sim::projectile::projectile_ground_z(
        world.resolved_terrain.as_ref(),
        &world.effective_shared_cell_dummy(),
        coordinate,
    );
    let height = coordinate.z.wrapping_sub(floor).wrapping_sub(if on_bridge {
        crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
    } else {
        0
    });
    if height >= 2 * crate::util::lepton::LEPTONS_PER_LEVEL as i32 {
        return -1;
    }
    let land = land_at(world, coordinate);
    if land == 2 && receiver_dispatched {
        // 469B45..469B87 reads exactly the ground list's first object, not
        // any naval object in the cell and not the bridge deck's list.
        let first = match cell_at(world, coordinate) {
            CellRef::Real(cell) => world
                .substrate
                .occupancy
                .cell_objects(
                    cell.rx,
                    cell.ry,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    world
                        .production
                        .terrain_object_cells
                        .get(&(cell.rx, cell.ry))
                        .copied(),
                )
                .next(),
            CellRef::Dummy { .. } => None,
        };
        if let Some(crate::sim::occupancy::CellObjectMember::Entity(id)) = first
            && let Some(unit) = world.substrate.entities.get(id)
            && unit.category == crate::map::entities::EntityCategory::Unit
            && rules
                .object(world.interner.resolve(unit.type_ref()))
                .is_some_and(|kind| kind.naval && !kind.underwater)
        {
            return -1;
        }
    }
    land
}

fn damage_band(list: &[String], damage: i32, step: i32) -> Option<&str> {
    // Native uses signed division after an upper clamp. Negative indices read
    // before the vector allocation; Rust deterministically rejects that native
    // invalid-memory case rather than inventing an animation or an RNG draw.
    let last = i32::try_from(list.len())
        .ok()?
        .checked_mul(step)?
        .checked_sub(1)?;
    let index = usize::try_from(damage.min(last) / step).ok()?;
    list.get(index).map(String::as_str)
}

pub(crate) fn select(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    damage: i32,
    land: i32,
    coordinate: ProjectileCoord,
) -> Option<InternedId> {
    if damage == 0 {
        return None;
    }
    // 48A50D..48A592: water's Conventional branch precedes the weather
    // override, and the cell's structural flag suppresses splash selection.
    if land == 2
        && warhead.conventional
        && cell_at(world, coordinate).bridge_flags_0x1180()
            & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
            == 0
    {
        let floor = crate::sim::projectile::projectile_ground_z(
            world.resolved_terrain.as_ref(),
            &world.effective_shared_cell_dummy(),
            coordinate,
        );
        if coordinate.z < floor.wrapping_add(2 * crate::util::lepton::LEPTONS_PER_LEVEL as i32) {
            return damage_band(&rules.combat_damage.splash_list, damage, 35)
                .map(|name| world.interner.intern(name));
        }
    }
    // 48A594..48A5A6 compares the retained LightningWarhead pointer, then
    // returns WeatherConBoltExplosion (including its null default).
    if !rules.general.lightning_warhead.is_empty() && warhead.id == rules.general.lightning_warhead
    {
        return (!rules.general.weather_con_bolt_explosion.is_empty()).then(|| {
            world
                .interner
                .intern(&rules.general.weather_con_bolt_explosion)
        });
    }
    if warhead.anim_list.is_empty() {
        return None;
    }
    let name = if warhead.em_effect {
        let index = world.scenario_rng.next_range_i32_inclusive(
            0,
            i32::try_from(warhead.anim_list.len() - 1).expect("native AnimList count fits i32"),
        );
        &warhead.anim_list[index as usize]
    } else {
        damage_band(&warhead.anim_list, damage, 25)?
    };
    Some(world.interner.intern(name))
}

/// Build the selected animation's sole delivery packet. The caller decides
/// when to construct it relative to its own damage and RNG operations.
pub(crate) fn effect(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    damage: i32,
    land: i32,
    selection_coordinate: ProjectileCoord,
    placement: ProjectileCoord,
) -> Option<ExplosionEffect> {
    let shp_name = select(world, rules, warhead, damage, land, selection_coordinate)?;
    Some(placed_effect(shp_name, placement))
}

pub(crate) fn placed_effect(shp_name: InternedId, placement: ProjectileCoord) -> ExplosionEffect {
    let (rx, ry, sub_x, sub_y, world_z) = super::projectile_impact_cell(placement);
    ExplosionEffect {
        shp_name,
        rx,
        ry,
        sub_x,
        sub_y,
        z: super::impact_z_byte(world_z.div_euclid(crate::util::lepton::LEPTONS_PER_LEVEL as i32)),
        world_z,
        death: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::SimRng;
    use serde_json::Value;

    #[test]
    fn dummy_land_survives_misses_and_resets_at_native_resize() {
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_select_anim.json",
        ))
        .unwrap();
        let mut world = Simulation::new();
        let dummy = world.effective_shared_cell_dummy();
        let rows = native["dummy_land_controls"].as_array().unwrap();
        assert_eq!(dummy.land_type(), rows[0]["land"].as_i64().unwrap() as i32);
        let clean_hash = world.state_hash();
        for row in &rows[1..4] {
            dummy.test_set_land_type(row["supplied_land"].as_i64().unwrap() as i32);
            assert_eq!(
                land_at(&world, ProjectileCoord::new(1_000_000, 0, 0)),
                row["land"].as_i64().unwrap() as i32
            );
            assert_eq!(serde_json::json!(dummy.snapshot().coord), row["coord"]);
            assert_eq!(dummy.fork_query_identity().land_type(), dummy.land_type());
            let prepared = crate::map::resolved_terrain::SharedCellDummy::fresh();
            prepared.adopt_prepared_load_state(&dummy);
            assert_eq!(prepared.land_type(), dummy.land_type());
        }
        assert_ne!(
            world.state_hash(),
            clean_hash,
            "live fallback land affects subsequent impact choice"
        );
        world.reconstruct_cellclass_dummy_for_map_resize();
        assert_eq!(dummy.land_type(), rows[4]["land"].as_i64().unwrap() as i32);
        assert_eq!(world.state_hash(), clean_hash);
    }

    #[test]
    fn first_ground_occupant_and_area_receipt_match_native_naval_selection() {
        use crate::map::entities::EntityCategory;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::{CellListInsertion, CellObjectMember};

        let Some((base, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_select_anim.json",
        ))
        .unwrap();
        let rows = native["rows"].as_array().unwrap();
        let controls: Vec<_> = rows
            .iter()
            .filter(|row| row["input"]["building"] == true)
            .collect();
        assert_eq!(controls.len(), 5);
        for row in controls {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let naval = row["unit_naval"].as_bool().unwrap();
            let underwater = row["unit_underwater"].as_bool().unwrap();
            let mut layers = RulesLayerStack::new(base.clone());
            // Same yes/no overrides as the original FV UnitType reader in
            // the corpus. The legacy input label "building" means an object
            // was supplied; only nonbuilding=true selects its Building vtable.
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&format!(
                    "[FV]\nNaval={}\nUnderwater={}\n",
                    if naval { "yes" } else { "no" },
                    if underwater { "yes" } else { "no" },
                )),
            );
            let processed = layers.process_with_fixed_art(&art).unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            let kind = rules.object("FV").unwrap();
            assert_eq!((kind.naval, kind.underwater), (naval, underwater));
            let mut world = Simulation::with_seed(31);
            for id in ["Americans", "FV", "DEST"] {
                crate::sim::intern::test_intern(id);
            }
            world.interner = crate::sim::intern::test_interner();
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
            let cell = terrain.cell_mut(10, 20).unwrap();
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.yr_cell_land_type = input["land"].as_i64().unwrap() as u8;
            cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
            world.install_resolved_terrain_for_new_map(terrain);
            let mut entity = GameEntity::test_default(1, "FV", "Americans", 10, 20);
            entity.category = if input["nonbuilding"] == true {
                EntityCategory::Structure
            } else {
                EntityCategory::Unit
            };
            let insertion = CellListInsertion::from_category(entity.category);
            world.substrate.entities.insert(entity);
            // Original fixture supplies Cell+E4 and the object/type pointer;
            // this uses the production retained-list owner at that boundary.
            // It does not claim native Unlimbo/Mark or placement admission.
            world
                .substrate
                .occupancy
                .add(10, 20, 1, MovementLayer::Ground, None, insertion);
            assert_eq!(
                world
                    .substrate
                    .occupancy
                    .cell_objects(10, 20, MovementLayer::Ground, None)
                    .next(),
                Some(CellObjectMember::Entity(1))
            );
            let coordinate = ProjectileCoord::new(2688, 5248, input["z"].as_i64().unwrap() as i32);
            let on_bridge = input["on_bridge"].as_bool().unwrap();
            let dispatched = input["direct"] == 0;
            let land = bullet_land(&world, &rules, coordinate, on_bridge, dispatched);
            assert_eq!(
                land,
                row["passed"]["land"].as_i64().unwrap() as i32,
                "{name}"
            );
            let before = world.scenario_rng.logical_state();
            let selected = select(
                &mut world,
                &rules,
                rules.warhead("HE").unwrap(),
                25,
                land,
                coordinate,
            );
            assert_eq!(
                selected.map(|id| world.interner.resolve(id)),
                row["selected"].as_str(),
                "{name}"
            );
            assert_eq!(
                world.scenario_rng.logical_state(),
                before,
                "{name} native RNG unchanged"
            );
            assert_eq!(row["rng_unchanged"], true);

            // Additional list-owner invariants from469B45..469B87, not new
            // native executable rows: neither a later object nor deck list
            // membership may replace the supplied first Ground object.
            if !naval {
                let naval_kind = rules.object("DEST").unwrap();
                assert!(naval_kind.naval && !naval_kind.underwater);
                world.substrate.entities.insert(GameEntity::test_default(
                    2,
                    "DEST",
                    "Americans",
                    10,
                    20,
                ));
                world.substrate.occupancy.add(
                    10,
                    20,
                    2,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
                // Reinsert the original nonnaval head in front of the new unit.
                world.substrate.occupancy.remove(10, 20, 1);
                world
                    .substrate
                    .occupancy
                    .add(10, 20, 1, MovementLayer::Ground, None, insertion);
                assert_eq!(
                    bullet_land(&world, &rules, coordinate, on_bridge, dispatched),
                    land,
                    "later naval member"
                );
                world.substrate.occupancy.remove(10, 20, 2);
                world.substrate.occupancy.add(
                    10,
                    20,
                    2,
                    MovementLayer::Bridge,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
                assert_eq!(
                    bullet_land(&world, &rules, coordinate, on_bridge, dispatched),
                    land,
                    "naval deck member"
                );
                for height in [207, 208, 209] {
                    let height_name = format!("caller_flags0_height{height}");
                    let expected = rows
                        .iter()
                        .find(|r| r["input"]["name"] == height_name)
                        .unwrap();
                    let probe = ProjectileCoord::new(
                        2688,
                        5248,
                        expected["input"]["z"].as_i64().unwrap() as i32,
                    );
                    assert_eq!(
                        bullet_land(&world, &rules, probe, false, true),
                        expected["passed"]["land"].as_i64().unwrap() as i32,
                        "{height_name} with nonnaval head"
                    );
                }
            }
        }
    }

    #[test]
    fn selection_and_bullet_height_match_original_executed_controls() {
        let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let native: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_select_anim.json",
        ))
        .unwrap();
        assert_eq!(native["rows"].as_array().unwrap().len(), 48);
        for row in native["rows"].as_array().unwrap() {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut world = Simulation::with_seed(31);
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
            let cell = terrain.cell_mut(10, 20).unwrap();
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.yr_cell_land_type = input["land"].as_i64().unwrap() as u8;
            cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32;
            world.install_resolved_terrain_for_new_map(terrain);
            let coordinate = ProjectileCoord::new(2688, 5248, input["z"].as_i64().unwrap() as i32);
            // Unit occupancy/type bytes are independently supplied in the
            // caller corpus. Here selection consumes the actual passed land;
            // the plain height rows also exercise our production caller probe.
            let land = row["passed"]["land"].as_i64().unwrap() as i32;
            if input["route"] == "caller"
                && !input.get("unit").is_some_and(|v| v == true)
                && !name.contains("unit")
            {
                assert_eq!(
                    bullet_land(
                        &world,
                        &rules,
                        coordinate,
                        input["on_bridge"].as_bool().unwrap(),
                        input["direct"] == 0
                    ),
                    land,
                    "{name} caller land"
                );
            }
            let mut warhead = rules.warhead("HE").unwrap().clone();
            // Reader controls are tested by select_anim_inputs; these bytes
            // are the original retained inputs to this executable comparison.
            warhead.conventional = row["conventional"].as_bool().unwrap();
            warhead.em_effect = row["em_effect"].as_bool().unwrap();
            let actual = if input["null_warhead"] == true {
                None
            } else {
                select(
                    &mut world,
                    &rules,
                    &warhead,
                    input["damage"].as_i64().unwrap() as i32,
                    land,
                    coordinate,
                )
            };
            assert_eq!(
                actual.map(|id| world.interner.resolve(id)),
                row["selected"].as_str(),
                "{name}"
            );
            let hex = world.scenario_rng.native_state_hex();
            let bytes: Vec<u8> = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|digits| u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(
                crate::util::sha256::sha256_hex(&bytes),
                row["rng_after_sha256"].as_str().unwrap(),
                "{name} full RNG"
            );
            assert_eq!(
                world.scenario_rng.state() == SimRng::new(31).state(),
                row["rng_unchanged"].as_bool().unwrap(),
                "{name}"
            );
        }
    }
}
