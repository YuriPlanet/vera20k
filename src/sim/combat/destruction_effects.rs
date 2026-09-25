//! The dying object's own death anims and the building destruction effects.
//!
//! - `UnitClass::Death_Explosion @ 0x00738680`, reached from the Unit death
//!   arm at `0x00737F6F` once its NowDead gates pass
//!   ([`Simulation::unit_sinks_on_death`]);
//! - the Aircraft death arm of `AircraftClass::ReceiveDamage`
//!   (`0x0041661F..0x0041668A`);
//! - `BuildingClass::DestructionEffects @ 0x004415F0` (slot `+0x4EC`, called
//!   once at `0x00442665` while the building is still on the map), up to its
//!   SpawnSurvivors call, which `crew_survival` owns.
//!
//! Each draws its picks, jitter and delays inline, in native order, and
//! records `AnimClass(type, coord, delay, 1, 0x600, 0, 0)` on the transaction's
//! ordered anim list (`ExplosionEffect::death`), which the consequence
//! boundary constructs in push order (see the construction-order residual).
//!
//! DestructionEffects steps ported here: 1 (`0x004415F9`, the eight damage
//! fire anims are UnInit), 7 (`0x0044177E`, the centre scorch/crater mark),
//! 8 (`0x004418EC`, one `Explosion=` anim per foundation cell) and 13
//! (`0x00441CAC`, one `DestroyAnim=` anim at the origin cell's corner).
//!
//! RESIDUALS:
//! - Construction order. The receiver constructs survivors, crewmen, building
//!   damaged-art anims and damage-smoke systems at their native calls, but
//!   defers the anims on the transaction's list (death, debris, InfDeath and
//!   warhead impact anims) to the consequence boundary, and admits the whole
//!   transaction's voxel debris there ahead of them. Native constructs each at
//!   its call: debris, death anims, then the crewman or survivors, the
//!   detonation's impact anim after its receivers. VERA's live order is
//!   survivors/crew (and later receivers' inline objects), voxel debris, then
//!   the anims. Effects: identities; logic order, because an object that
//!   unregisters during its AI makes the live pass skip its successor
//!   (`Simulation::try_for_each_live_object`), so the frame a death's
//!   last-constructed anim expires natively skips the first crewman or
//!   survivor and in VERA another object, shifting the crew's movement and
//!   later draws by a frame; and once `AnimClass::Middle` runs, its draws
//!   interleave differently with survivor AI. No stock death anim has a
//!   constructor draw (`RandomRate=`).
//!   Trigger: every crewed vehicle death whose crew escapes and every building
//!   death with survivors; also a multi-record transaction whose later
//!   receiver changes damaged art. Frequency: routine. The fix is to
//!   construct each object at its call (the plan's next destruction item).
//! - Unit NowDead gates ahead of `Death_Explosion` (`0x00737DA7..0x00737F6F`):
//!   - The ship sink (`0x00737DE2..0x00737E5E`) is gated, so no pick is drawn,
//!     but the sink is absent: native sets Health 1, IsAlive, the sinking
//!     byte `+0x3CD`, calls `vt+0x3A0`, and the hull goes down in
//!     `UnitClass::AI` before removal; VERA removes it at once with no anim.
//!     Trigger: stock DEST, AEGIS, CARRIER, DRED, VLAD, CRUISE and CDEST
//!     killed on water. Effect: the hull vanishes and its cell frees early.
//!     Frequency: every such naval death.
//!   - `DeathFrames=` (`+0xE20` > 0) defers the explosion to `UnitClass::AI`
//!     (`0x00736381`) when the death frames end. No stock type sets it and
//!     VERA does not parse it.
//!   - The water splash (`0x00737E78..0x00737F6B`): a unit at height <= 10
//!     whose `+0x8F` byte is set, over water, gets two splash anims (Rules
//!     `+0x94` and the last Rules `+0xBC4` entry) instead. VERA has no
//!     producer of the byte (`drop_in_bridge_member` snaps a falling unit
//!     to the ground, recorded DRIFT there).
//! - The other `Death_Explosion` callers are not wired:
//!   - The crush of a Unit victim (`0x007418E5` -> `vt+0x170` =
//!     `0x00746D60`: Death_Explosion, then the capture release `0x00710460`).
//!     VERA's crush teardown (`movement_tick`) draws no pick and plays no
//!     anim. Trigger: the Battle Fortress, stock's only `OmniCrusher=`,
//!     crushing any vehicle but the five `OmniCrushResistant=` types.
//!   - A `Crashable=` (`+0xD95`) unit's crash (`0x007461D1`, locomotor
//!     message 0x117C through the Unit vtable at `0x007F5C4C`:
//!     Death_Explosion then UnInit; `BalloonHover=` types fire their
//!     DeathWeapon instead). VERA has no crash; stock SHAD, HIND, SCHP and
//!     SCHD die through the ReceiveDamage arm. Whether FootClass defers
//!     their in-air death to the crash is untraced.
//!   - The `DeathFrames=` completion (`0x00736381`), dead on stock.
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
//! - Step 5 (`RevealToAll=`, stock NAIRON, GACSPH, GADUMY, GAWEAT, NAMISL,
//!   YAGNTC, YAPPET): a building the local player does not own reveals its
//!   stored sight record to that player as it dies (`vt+0x48C` =
//!   `0x0070B1D0`). VERA does not parse `RevealToAll=`. Effect: shroud only,
//!   no draws. Frequency: every such building death.
//! - Steps with no stock trigger or no VERA state: 2 (radar-spy bits `+0x210`),
//!   3 and 4 (CloakGenerator, LaserFencePost), 9 (FIRE3 on each cardinal
//!   neighbour of an `Explodes=` building whose cell holds an overlay with
//!   `Explodes=yes`, `0x00441A90..0x00441AC2`; no stock overlay has it), 10
//!   (stored-ore spill; YR never fills building storage), 11 (ShakeScreen, a
//!   bare `RET`), 14 (`DestroyParticleSystems=` smoke; no stock user). Step 6
//!   (BuildingDieSound) plays from the Techno death sounds; step 12 (the zero
//!   death timer of `Explodes=`/Selling) is a `crew_survival` residual.
//! - A vehicle's current ammo (Unit `+0x2FC`) is not tracked; the
//!   `Death_Explosion` last-entry override reads a fresh unit's `Ammo=`. No
//!   stock `Explodes=` vehicle has a finite `Ammo=`.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::Simulation;

use super::{ExplosionEffect, SmudgeSpawnRequest};

/// `AnimClass` constructor `drawFlags` every death producer pushes
/// (`0x0073871E`, `0x00738854`, `0x00416660`, `0x00441A04`, `0x00441D01`).
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
            world_z: coord.z,
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
    pub(crate) fn pick_death_anim<'r>(&mut self, list: &'r [String]) -> &'r str {
        let index = (self.scenario_rng.next_u32() % list.len() as u32) as usize;
        &list[index]
    }

    /// The ship-sinking gate of the Unit NowDead block
    /// (`UnitClass::ReceiveDamage @ 0x00737C90`, `0x00737DE2..0x00737E32`):
    /// `Naval=` (`+0xCCE`), not `Underwater=` (`+0xD69`) or `Organic=`
    /// (`+0xD97`), `Weight=` (`+0x370`) at least `ShipSinkingWeight=` (the
    /// `FCOMP` skips on less or unordered), the unit's cell (`vt+0x1BC`) of
    /// LandType water (`+0xEC == 2`), and not being warped (`+0x271`, set by
    /// the Chronosphere at `0x0065F1F4` and the Teleport locomotor at
    /// `0x00719579`/`0x007198DA`). Such a unit sinks and never reaches
    /// `Death_Explosion`; the sink itself is a module residual. VERA has no
    /// Chronosphere unit warp, so a teleport in flight stands in for the byte.
    pub(crate) fn unit_sinks_on_death(&self, rules: &RuleSet, unit_id: u64) -> bool {
        let Some(entity) = self.substrate.entities.get(unit_id) else {
            return false;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        object.naval
            && !object.underwater
            && !object.organic
            && object.weight >= rules.general.ship_sinking_weight
            && self.resolved_terrain.as_ref().is_some_and(|terrain| {
                terrain
                    .cell(entity.position.rx, entity.position.ry)
                    .is_some_and(|cell| cell.yr_cell_land_type == LandType::Water.as_index())
            })
            && entity.teleport_state.is_none()
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
