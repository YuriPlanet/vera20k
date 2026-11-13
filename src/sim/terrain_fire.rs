//! Ares 3.0 terrain fire (`Ext/Terrain/Hooks.cpp`): a `Sparky=` warhead sets
//! an `IsFlammable=` tree alight, and a burning tree spreads the fire to the
//! trees around it.
//!
//! Stock YR has the whole body but never calls it: `TerrainClass::Catch_Fire
//! @ 0x0071C5B0` (vt+0x178) has no virtual caller, so IsOnFire (`+0xCC`)
//! stays false. Ares 3.0 calls it from Take_Damage (1004F4E0, hook
//! 0x71B99E) and from the Terrain AI (1004F530, hook 0x71C7C2), replaces its
//! Wood-armor test (1004F4B0, hook 0x71C5D2) and admits flammable trees to
//! Logic (1004F470, hook 0x5F4FF9, `TerrainObjectState::joins_logic_on_unlimbo`).
//! The fire anims are owned by the tree (`AnimObject::owner_terrain`); when
//! the last one goes, its SetOwnerObject callback (vt+0x17C) is
//! `TerrainClass::Finish_Fire_Death @ 0x0071C6B0`.
//!
//! Retail data leaves it dormant: no warhead sets `Sparky=`, no TerrainType
//! `IsFlammable=`, `TreeFlammability=0.0` and `TreeFire=` is unset.

use crate::rules::ruleset::RuleSet;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;

/// `TreeFire=` anims are constructed with flags 0x600 and 255 loops
/// (`0x0071C61A..0x0071C637`); ZAdjust then drops by 20 (`0x0071C682`).
const TREE_FIRE_FLAGS: u32 = 0x600;
const TREE_FIRE_LOOPS: i32 = 0xFF;
const TREE_FIRE_Z_DROP: i32 = 20;
/// `RandomRanged(1, 0x7FFFFFFF) * 2^-31`, the 1004F530 roll; the product is
/// exact in binary64.
const SPREAD_ROLL_SCALE: f64 = 1.0 / 2_147_483_648.0;

impl Simulation {
    /// `TerrainClass::Catch_Fire @ 0x0071C5B0` with Ares 3.0
    /// TerrainClass_Ignite_IsFlammable (1004F4B0, hook 0x71C5D2): a burning
    /// tree, or a `SpawnsTiberium=` or not `IsFlammable=` type, refuses (the
    /// Ares test replaces stock's Wood armor one). Otherwise one Scenario
    /// `Random::Next` picks `TreeFire[r & 1]`, built at the tree's GetCoords
    /// and attached to it, and IsOnFire is set. IsCrumbling is not retained
    /// (a lethal hit UnInits the tree in the same call). Native indexes
    /// `TreeFire=` unchecked; a missing entry builds no anim.
    pub(crate) fn terrain_catch_fire(&mut self, terrain_id: u64, rules: &RuleSet) -> bool {
        let Some(terrain) = self.terrain_objects.get(terrain_id) else {
            return false;
        };
        if terrain.on_fire() {
            return false;
        }
        let Some(terrain_type) =
            rules.terrain_object_type_case_insensitive(self.interner.resolve(terrain.type_ref()))
        else {
            return false;
        };
        if terrain_type.spawns_tiberium || !terrain_type.is_flammable {
            return false;
        }
        let centre = terrain.world_coord();
        let pick = (self.scenario_rng.next_u32() & 1) as usize;
        if let Some(name) = rules.forest_fire().tree_fire.get(pick) {
            let world = crate::sim::anim_class::AnimWorldCoord {
                x: centre.x,
                y: centre.y,
                z: centre.z,
            };
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: TREE_FIRE_FLAGS,
                loop_count: TREE_FIRE_LOOPS,
                owner: Some(terrain_id),
                ..AnimClassSpawnDescriptor::new(self.interner.intern(name), rx, ry, sub_x, sub_y, z)
            };
            if let Ok(anim) = self.spawn_anim_at_world(rules, descriptor, world) {
                let z_adjust = self.anim(anim).map_or(0, |anim| anim.z_adjust());
                self.substrate
                    .set_anim_z_adjust(anim, z_adjust.wrapping_sub(TREE_FIRE_Z_DROP));
            }
        }
        self.terrain_objects.set_on_fire(terrain_id, true);
        true
    }

    /// `TerrainClass::Finish_Fire_Death @ 0x0071C6B0`, the tree's vt+0x17C
    /// that SetOwnerObject (`0x00424BB0`) calls when its last fire anim
    /// leaves: IsOnFire clears. Its tail (`0x0071C6CD..0x0071C72B`) starts the
    /// crumble of a tree left at zero Health, which VERA never holds: a
    /// lethal hit UnInits the tree in the same Take_Damage call.
    pub(crate) fn terrain_finish_fire_death(&mut self, terrain_id: u64) {
        if self
            .terrain_objects
            .get(terrain_id)
            .is_some_and(|terrain| terrain.on_fire())
        {
            self.terrain_objects.set_on_fire(terrain_id, false);
        }
    }

    /// Ares 3.0 TerrainClass_Update_ForestFire (1004F530, hook 0x71C7C2, in
    /// `TerrainClass::AI @ 0x0071C730`): with `TreeFlammability=` above zero,
    /// a burning tree draws Scenario `RandomRanged(0, 99)`; on 0 it visits the
    /// 8 neighbour cells in compass order and, for each first terrain object
    /// not on fire, draws `RandomRanged(1, 0x7FFFFFFF)` and sets it alight
    /// when that over 2^31 is below the flammability.
    pub(crate) fn ares_spread_forest_fire(&mut self, terrain_id: u64, rules: &RuleSet) {
        let flammability = f64::from_bits(rules.forest_fire().tree_flammability.bits());
        let Some(terrain) = self.terrain_objects.get(terrain_id) else {
            return;
        };
        if !terrain.on_fire() || !(flammability > 0.0) {
            return;
        }
        let (rx, ry) = terrain.cell();
        if self.scenario_rng.next_range_i32_inclusive(0, 99) != 0 {
            return;
        }
        for (dx, dy) in crate::util::direction::DIRECTION_DELTAS {
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(rx) + dx),
                u16::try_from(i32::from(ry) + dy),
            ) else {
                continue;
            };
            let Some(neighbour) = self.terrain_objects.id_at((x, y)) else {
                continue;
            };
            if self
                .terrain_objects
                .get(neighbour)
                .is_none_or(|tree| tree.on_fire())
            {
                continue;
            }
            let roll = self.scenario_rng.next_range_i32_inclusive(1, 0x7FFF_FFFF);
            if f64::from(roll) * SPREAD_ROLL_SCALE < flammability {
                self.terrain_catch_fire(neighbour, rules);
            }
        }
    }
}

// The fire anims' tree owner (`AnimObject::owner_terrain`) moves to the
// switch with the anim code; until then these tests need the `ares` feature.
#[cfg(all(test, feature = "ares"))]
#[path = "terrain_fire_tests.rs"]
mod tests;
