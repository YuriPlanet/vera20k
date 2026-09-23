//! The dying object's own death anims and the building destruction effects.
//!
//! - `UnitClass::Death_Explosion @ 0x00738680`, reached from the Unit death
//!   arm at `0x00737F6F`;
//! - the Aircraft death arm of `AircraftClass::ReceiveDamage`
//!   (`0x0041661F..0x0041668A`);
//! - `BuildingClass::DestructionEffects @ 0x004415F0` (slot `+0x4EC`, called
//!   once at `0x00442665` while the building is still on the map), up to its
//!   SpawnSurvivors call, which `crew_survival` owns.
//!
//! Each draws its picks, jitter and delays inline, in native order, and
//! records `AnimClass(type, coord, delay, 1, 0x600, 0, 0)` on the transaction's
//! ordered anim list (`ExplosionEffect::death`), which the consequence
//! boundary constructs in push order with the warhead impact and debris anims.
//!
//! DestructionEffects steps ported here: 1 (`0x004415F9`, the eight damage
//! fire anims are UnInit), 7 (`0x0044177E`, the centre scorch/crater mark),
//! 8 (`0x004418EC`, one `Explosion=` anim per foundation cell) and 13
//! (`0x00441CAC`, one `DestroyAnim=` anim at the origin cell's corner).
//!
//! RESIDUALS:
//! - Every anim of a damage transaction is constructed at the consequence
//!   boundary rather than at its native call (this producer, the warhead
//!   impact and the death debris alike), so an anim's constructor draws
//!   (`RandomRate=`: none on any stock death anim) follow the receiver's later
//!   draws, and survivors and crewmen constructed inside the receiver take
//!   identities ahead of their death's anims. Relative order among the anims
//!   is native. Trigger: every death. Risk: identity and logic order only on
//!   stock data.
//! - `AnimClass::Middle @ 0x00424F00` is not run for these anims, so the
//!   scorch/crater a multi-frame explosion leaves at its middle frame (and its
//!   Scenario coin flip and candidate pick in `AnimClass::AI`) is missing.
//!   Trigger: every building, vehicle and aircraft death with `Explosion=`
//!   (TWLT070, S_BANG48, S_BRNL58, S_TUMU60 mark; S_CLSN58 craters). Effect:
//!   no marks under the death explosions; later Scenario draws are absent.
//!   Frequency: continuous. It belongs to a shared AnimClass Middle port.
//! - An art-less type (stock `gtpowexp` on GAPOWR, YAPOWR and YAROCK,
//!   `tstlexp` on NAPOWR) constructs nothing, where native constructs an
//!   End=0 anim that draws nothing; the pick is still drawn, so only anim
//!   identities shift.
//! - DestroyAnim's palette (TechnoType `+0xDF0`/`+0xDD0` -> anim
//!   `+0xD4`/`+0xDC`) is presentation and not carried.
//! - Steps with no stock trigger or no VERA state: 2 (radar-spy bits `+0x210`),
//!   3 and 4 (CloakGenerator, LaserFencePost), 5 (`RevealToAll=` for a
//!   non-owner local player), 9 (FIRE3 next to an `Explodes=` overlay; no
//!   stock overlay explodes), 10 (stored-ore spill; YR never fills building
//!   storage), 11 (ShakeScreen, a bare `RET`), 14 (`DestroyParticleSystems=`
//!   smoke; no stock user). Step 6 (BuildingDieSound) plays from the Techno
//!   death sounds; step 12 (the zero death timer of `Explodes=`/Selling) is a
//!   `crew_survival` residual.
//! - A vehicle's current ammo (Unit `+0x2FC`) is not tracked; the
//!   `Death_Explosion` last-entry override reads a fresh unit's `Ammo=`. No
//!   stock `Explodes=` vehicle has a finite `Ammo=`.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::Simulation;

use super::{ExplosionEffect, SmudgeSpawnRequest};

/// `AnimClass` constructor `drawFlags` every death producer pushes
/// (`0x0073871E`, `0x00738854`, `0x00416660`, `0x00441A04`).
const DEATH_ANIM_DRAW_FLAGS: u32 = 0x600;
/// DestructionEffects' per-cell jitter radius (`PUSH 0x40`, `0x0044198E`).
const CELL_EXPLOSION_SCATTER_LEPTONS: i32 = 0x40;

/// A death producer's `AnimClass` constructor call: the exact coordinate and
/// delay; loop 1, flags 0x600, zAdjust 0 and reverse 0 are fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathAnimSpawn {
    pub coord: AnimWorldCoord,
    pub delay: u16,
}

impl Simulation {
    /// Record `new AnimClass(type, coord, delay, 1, 0x600, 0, 0)` on the
    /// transaction's ordered anim list.
    fn push_death_anim(
        &mut self,
        anims: &mut Vec<ExplosionEffect>,
        type_name: &str,
        coord: AnimWorldCoord,
        delay: u16,
    ) {
        let shp_name = self.interner.intern(type_name);
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        anims.push(ExplosionEffect {
            shp_name,
            rx,
            ry,
            sub_x,
            sub_y,
            z,
            death: Some(DeathAnimSpawn { coord, delay }),
        });
    }

    /// Construct one recorded death anim at the consequence boundary.
    pub(crate) fn admit_death_anim(
        &mut self,
        rules: &RuleSet,
        type_id: InternedId,
        spawn: DeathAnimSpawn,
    ) {
        let (rx, ry, sub_x, sub_y, z) = spawn.coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay: spawn.delay,
            loop_count: 1,
            draw_flags: DEATH_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
        };
        if let Err(error) = self.spawn_anim_at_world(rules, descriptor, spawn.coord) {
            log::debug!(
                "death anim [{}] did not construct: {error}",
                self.interner.resolve(type_id)
            );
        }
    }

    /// One Scenario `Next() % count` pick from a non-empty anim list.
    fn pick_death_anim<'r>(&mut self, list: &'r [String]) -> &'r str {
        let index = (self.scenario_rng.next_u32() % list.len() as u32) as usize;
        &list[index]
    }

    /// `UnitClass::Death_Explosion @ 0x00738680`: one `Explosion=` anim and
    /// then one `DestroyAnim=` anim at the unit's Location, each picked with
    /// one Scenario `Next()` (`0x007386A7`, `0x0073881D`).
    pub(crate) fn unit_death_explosion(
        &mut self,
        rules: &RuleSet,
        unit_id: u64,
        anims: &mut Vec<ExplosionEffect>,
    ) {
        let Some(entity) = self.substrate.entities.get(unit_id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let location = position_world_coord(&entity.position);
        let coord = AnimWorldCoord {
            x: location.x,
            y: location.y,
            z: location.z,
        };
        // `0x007386C3..0x007386FF`: an `Explodes=` unit, or one with the
        // EXPLODES veteran ability (`HasWeaponAbility(10)`, `0x0070D0D0`),
        // that has ammo (`Ammo=` -1 or Unit `+0x2FC` > 0) plays the last
        // `Explosion=` entry; the pick has been drawn already.
        let explodes = object.explodes
            || super::veterancy::has_weapon_ability(
                super::veterancy::rank_of(entity.veterancy_raw),
                object,
                Ability::Explodes,
            );
        let armed = object.ammo == -1 || object.ammo > 0;
        if !object.explosion_anims.is_empty() {
            let picked = self.pick_death_anim(&object.explosion_anims);
            let anim = if explodes && armed {
                object.explosion_anims.last().map_or(picked, String::as_str)
            } else {
                picked
            };
            self.push_death_anim(anims, anim, coord, 0);
        }
        // `0x00738749..0x007387FC` sums the stored ore's value into a local
        // nothing reads and calls the ShakeScreen stub (`0x0048DED0`, a bare
        // `RET`): no effect.
        if !object.destroy_anims.is_empty() {
            let anim = self.pick_death_anim(&object.destroy_anims);
            self.push_death_anim(anims, anim, coord, 0);
        }
    }

    /// The Aircraft death arm (`0x0041661F..0x0041668A`): after the
    /// allocation, one Scenario `Next()` (`0x00416649`) picks an `Explosion=`
    /// anim at the aircraft's coordinate (vt+0xA4 `0x0041BDD0` returns
    /// GetCoords). Aircraft play no `DestroyAnim=`.
    pub(crate) fn aircraft_death_explosion(
        &mut self,
        rules: &RuleSet,
        aircraft_id: u64,
        anims: &mut Vec<ExplosionEffect>,
    ) {
        let Some(entity) = self.substrate.entities.get(aircraft_id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if object.explosion_anims.is_empty() {
            return;
        }
        let location = position_world_coord(&entity.position);
        let anim = self.pick_death_anim(&object.explosion_anims);
        self.push_death_anim(
            anims,
            anim,
            AnimWorldCoord {
                x: location.x,
                y: location.y,
                z: location.z,
            },
            0,
        );
    }

    /// `BuildingClass::DestructionEffects @ 0x004415F0` from its entry to its
    /// SpawnSurvivors call: the damage fires go out, the centre mark is
    /// committed through `commit_smudge`, each foundation cell (in the
    /// `vt+0x108(0)` list order) gets an `Explosion=` anim and the origin
    /// corner a `DestroyAnim=` anim.
    pub(crate) fn building_destruction_anims(
        &mut self,
        rules: &RuleSet,
        building_id: u64,
        anims: &mut Vec<ExplosionEffect>,
        mut commit_smudge: impl FnMut(&mut Simulation, SmudgeSpawnRequest),
    ) {
        // 1. `0x004415F9..0x00441617`.
        self.clear_building_damage_fire_slots(building_id, Some(rules));
        let Some(entity) = self.substrate.entities.get(building_id) else {
            return;
        };
        if entity.category != EntityCategory::Structure {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let (rx, ry, z) = (entity.position.rx, entity.position.ry, entity.position.z);
        let location = position_world_coord(&entity.position);

        // 7. `0x0044177E..0x004418E7`.
        commit_smudge(
            self,
            super::building_center_smudge_request(rx, ry, i32::from(z), &object.foundation),
        );

        // 8. `0x004418EC..0x00441A26`, per foundation cell: the cell centre
        // jittered by `0x0049F420` (one Scenario `Next`, radius 0x40) at the
        // building's Z, then `RandomRanged(0, 3)` for the delay and `Next`
        // for the type.
        if !object.explosion_anims.is_empty() {
            for (cell_rx, cell_ry) in
                crate::sim::crew_survival::foundation_cells(rx, ry, &object.foundation)
            {
                let (x, y) = super::inviso_scatter::random_direction_coord(
                    &mut self.scenario_rng,
                    i32::from(cell_rx) * 256 + 0x80,
                    i32::from(cell_ry) * 256 + 0x80,
                    CELL_EXPLOSION_SCATTER_LEPTONS,
                );
                let delay = self.scenario_rng.next_range_u32_inclusive(0, 3) as u16;
                let anim = self.pick_death_anim(&object.explosion_anims);
                self.push_death_anim(
                    anims,
                    anim,
                    AnimWorldCoord {
                        x,
                        y,
                        z: location.z,
                    },
                    delay,
                );
            }
        }

        // 13. `0x00441CAC..0x00441D64`: the pick is drawn even when the entry
        // is empty; the coordinate is vt+0xAC (`0x00459EF0`, Location minus
        // 0x80 on X and Y).
        if !object.destroy_anims.is_empty() {
            let anim = self.pick_death_anim(&object.destroy_anims);
            self.push_death_anim(
                anims,
                anim,
                AnimWorldCoord {
                    x: location.x - 0x80,
                    y: location.y - 0x80,
                    z: location.z,
                },
                0,
            );
        }
    }
}

#[cfg(test)]
#[path = "destruction_effects_tests.rs"]
mod tests;
