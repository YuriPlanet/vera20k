//! Shared debris constructors, death anims and building destruction effects.
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
//! Each producer constructs its Anim at the original call, before the next
//! pick, nested death weapon or escaped crew. The shared421EA0 constructor and
//! Logic/Start owners handle admission; explicit callback-disabled test packets
//! retain their collector seam. DBRIS constructors actively read RandomRate,
//! which normalizes to[1,1] on stock and consumes no raw word; independent
//! Bouncer launch draws remain ordered per piece.
//!
//! DestructionEffects steps ported here: 1 (`0x004415F9`, the eight damage
//! fire anims are UnInit), 5 (`0x00441702`, a computer house's
//! `RevealToAll=` building releases the sight it showed the current house), 7
//! (`0x0044177E`, the centre scorch/crater mark), 8 (`0x004418EC`, one
//! `Explosion=` anim per foundation cell), 12 (`0x00441C43`, the death fuse
//! that decides whether the building outlives its killing blow), 13
//! (`0x00441CAC`, one `DestroyAnim=` anim at the origin cell's corner) and the
//! NoSurvivor write before SpawnSurvivors (`0x00441F0B`).
//!
//! Original constructor/Logic-before-crew execution:
//! `tools/spatial_oracle/building_death_anims.py` (joined fatal lifecycle),
//! gamemd.
//!
//! A dropped bomb over water at height <= 10 takes the water splash
//! (`0x00737E78..0x00737F6B`) instead of `Death_Explosion`; this owner
//! constructs Wake and the last SplashList entry inline.
//!
//! RESIDUALS:
//! - Unit NowDead gates ahead of `Death_Explosion` (`0x00737DA7..0x00737F6F`):
//!   - The ship sink (`0x00737DE2..0x00737E5E`) skips these explosion draws.
//!     `world::sinking` retains Health 1/Alive/+3CD through its second Stun,
//!     then the receiver unmarks the cell. Unit AI lowers the retained hull
//!     and finally calls RecordKill/UnInit; cell removal is immediate in both.
//!   - `DeathFrames=` (`+0xE20` > 0) defers the explosion to `UnitClass::AI`
//!     (`0x00736381`) when the death frames end. No stock type sets it and
//!     VERA does not parse it.
//! - A `Crashable=` (`+0xD95`) unit's crash impact calls `Death_Explosion`
//!   once more (`0x007461D1`, the Jumpjet's 0x117C notice), outside any
//!   receiver transaction: [`Simulation::unit_death_explosion`] builds
//!   each anim right after its pick (`sim::world::crash`).
//! - The crush of a Unit victim (`0x007418E5` -> `vt+0x170` = `0x00746D60`)
//!   calls it too, from `Simulation::per_cell_process`: the Battle Fortress,
//!   stock's only `OmniCrusher=`, crushing a vehicle.
//! - The `DeathFrames=` completion (`0x00736381`), dead on stock, is not
//!   wired.
//! - DestroyAnim's palette (TechnoType `+0xDF0`/`+0xDD0` -> anim
//!   `+0xD4`/`+0xDC`, `0x00441D2F..0x00441D61`) is carried as the anim's
//!   extra-building-scheme remap (`AnimRemap::ExtraBuildingScheme`).
//! - Steps with no stock trigger or no VERA state: 2 (radar-spy bits `+0x210`),
//!   3 and 4 (CloakGenerator, LaserFencePost), 9 (FIRE3 on each cardinal
//!   neighbour of an `Explodes=` building whose cell holds an overlay with
//!   `Explodes=yes`, `0x00441A90..0x00441AC2`; no stock overlay has it), 10
//!   (stored-ore spill; YR never fills building storage), 11 (ShakeScreen, a
//!   bare `RET`), 14 (`DestroyParticleSystems=` smoke; no stock user). Step 6
//!   (BuildingDieSound) plays from the Techno death sounds.
//! - Ordinary ground weapon-fire ammo debits remain separate. The shared Ammo
//!   owner retains constructor and chrono reinforcement writes, which the
//!   `Death_Explosion` last-entry override reads. No stock `Explodes=` vehicle
//!   has finite `Ammo=`.

use crate::map::entities::EntityCategory;
use crate::rules::foundation::foundation_dimensions;
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;

use super::{ExplosionEffect, SmudgeSpawnRequest};

/// `AnimClass` constructor `drawFlags` shared by death and SHP debris producers
/// (`0x0073871E`, `0x00738854`, `0x00416660`, `0x00441A04`, `0x00441D01`,
/// `0x00469E94`).
const DEATH_ANIM_DRAW_FLAGS: u32 = 0x600;
/// DestructionEffects' per-cell jitter radius (`PUSH 0x40`, `0x0044198E`).
const CELL_EXPLOSION_SCATTER_LEPTONS: i32 = 0x40;

/// A death producer's `AnimClass` constructor call: the exact coordinate and
/// delay; loop 1, flags 0x600, zAdjust 0 and reverse 0 are fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathAnimSpawn {
    pub coord: AnimWorldCoord,
    pub delay: u16,
    /// The constructor's draws when the producer already took them at their
    /// native point (a death debris piece); otherwise the spawn draws.
    pub draws: Option<crate::sim::anim_class::AnimConstructorDraws>,
    /// What the producer writes on the anim right after its constructor.
    pub follow_up: Option<DeathAnimFollowUp>,
}

impl DeathAnimSpawn {
    /// A spawn that takes its own constructor draws and has no follow-up.
    pub const fn at(coord: AnimWorldCoord, delay: u16) -> Self {
        Self {
            coord,
            delay,
            draws: None,
            follow_up: None,
        }
    }
}

/// An infantry death arm's writes on its anim after the constructor
/// (`InfantryClass::ReceiveDamage @ 0x00517FA0`). Each takes the killing
/// source's owner (`+0x21C`), else the receiver's house argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathAnimFollowUp {
    /// InfDeath 8 (`0x0051887B..0x005188A9`): SetOwnerHouse
    /// (`0x00424CA0`).
    Owner(InternedId),
    /// InfDeath 9 (`0x00518A96..0x00518B26`): with a house, SetOwnerHouse
    /// and the house's colour scheme as the anim's remap (`+0xD4`, the
    /// scheme's `+0x30C` from the array `0x00B054D4`); then, house or not,
    /// the anim's MarkCellOccupancy (vt+0xF0 = `0x00426270`).
    Mutate(Option<InternedId>),
    /// BuildingClass::DestructionEffects' DestroyAnim of a custom-palette
    /// type (`0x00441D2F..0x00441D61`): GetRemapColour's scheme from the
    /// extra building palette as the remap (`+0xD4`); the palette name it
    /// copies to `+0xDC` is the scenario's one.
    ///
    /// RESIDUAL: AnimClass::Save (`0x004253B0`) finds no global index for
    /// this scheme, so Load (`0x00425280`) regenerates the palette by name and
    /// takes its scheme 0; a restored VERA anim keeps the owner's. Retail's
    /// only such anim, CANY04DM, has no 16..31 pixels and draws at full light.
    ExtraBuildingRemap(InternedId),
}

impl Simulation {
    /// One SHP debris constructor boundary, after its list pick:
    /// Techno702566 and Bullet469EAE. Spend the common identity before
    /// constructor RNG, then admit synchronously before another piece picks.
    /// Callback-disabled numeric fixtures retain the preconsumed packet seam.
    pub(crate) fn emit_debris_anim(
        &mut self,
        rules: &RuleSet,
        name: Option<&str>,
        coord: AnimWorldCoord,
        anims: &mut Vec<ExplosionEffect>,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let native_unique_id = self.next_native_runtime_id();
        let config =
            name.and_then(|name| rules.art().anim_runtime_config(&name.to_ascii_uppercase()));
        let mut draws = if let Some(config) = config {
            crate::sim::anim_class::anim_constructor_draws(config, coord, &mut self.scenario_rng)?
        } else {
            crate::sim::anim_class::AnimConstructorDraws {
                native_unique_id: None,
                random_rate: None,
                bounce: None,
            }
        };
        draws.native_unique_id = Some(native_unique_id);
        if let Some(name) = name {
            self.emit_death_anim(
                rules,
                anims,
                name,
                DeathAnimSpawn {
                    draws: Some(draws),
                    ..DeathAnimSpawn::at(coord, 0)
                },
            );
        }
        Ok(())
    }

    /// Native constructor call, or an explicit callback-disabled fixture's
    /// packet. Production never carries these births to the consequence tail.
    pub(crate) fn emit_death_anim(
        &mut self,
        rules: &RuleSet,
        anims: &mut Vec<ExplosionEffect>,
        type_name: &str,
        spawn: DeathAnimSpawn,
    ) {
        let shp_name = self.interner.intern(type_name);
        let (rx, ry, sub_x, sub_y, z) = spawn.coord.to_cell_sub_z();
        let effect = ExplosionEffect {
            shp_name,
            rx,
            ry,
            sub_x,
            sub_y,
            z,
            world_z: spawn.coord.z,
            death: Some(spawn),
        };
        if super::world_receiver::callbacks_enabled(self) {
            crate::sim::world::damage_consequences::admit_explosion_effect(self, rules, effect);
        } else {
            anims.push(effect);
        }
    }

    /// One full Anim421EA0 constructor, reused by every death producer.
    pub(crate) fn admit_death_anim(
        &mut self,
        rules: &RuleSet,
        type_id: InternedId,
        spawn: DeathAnimSpawn,
    ) -> Option<crate::sim::anim_class::AnimId> {
        let (rx, ry, sub_x, sub_y, z) = spawn.coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay: spawn.delay,
            loop_count: 1,
            draw_flags: DEATH_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
        };
        let anim = match self.spawn_anim_at_world_with_constructor(
            rules,
            descriptor,
            spawn.coord,
            spawn.draws.map_or(
                crate::sim::anim_class::AnimConstructorInput::Runtime,
                crate::sim::anim_class::AnimConstructorInput::Preconsumed,
            ),
        ) {
            Ok(anim) => anim,
            Err(error) => {
                log::debug!(
                    "death anim [{}] did not construct: {error}",
                    self.interner.resolve(type_id)
                );
                return None;
            }
        };
        match spawn.follow_up {
            None => {}
            Some(DeathAnimFollowUp::Owner(house)) => {
                self.substrate.set_anim_owner_house(anim, house);
            }
            Some(DeathAnimFollowUp::Mutate(house)) => {
                if let Some(house) = house {
                    self.substrate.set_anim_owner_house(anim, house);
                    self.substrate.anims.set_anim_house_remap(anim, house);
                }
                Self::anim_mark_cell_occupancy(&mut self.substrate, &self.resolved_terrain, anim);
            }
            Some(DeathAnimFollowUp::ExtraBuildingRemap(house)) => {
                self.substrate
                    .anims
                    .set_anim_extra_building_remap(anim, house);
            }
        }
        Some(anim)
    }

    /// The shared low dropped-bomb water gate in Techno ReceiveDamage
    /// (`0x00702232..0x0070227B`), Unit (`0x00737E78..0x00737EBA`) and
    /// Infantry (`0x005181B5..0x00518200`). The bomb byte persists after
    /// landing. GetHeight is relative to the live ground; the cell lookup
    /// and the subsequent anims use raw Location, not GetCoords or floor Z.
    /// Original instructions: `python -m tools.native_inspect disasm
    /// 0x70222E --bytes 96` (also 0x737E78/260, 0x5181B5/255), supported
    /// retail gamemd.
    pub(crate) fn dropped_bomb_water_death_location(&self, id: u64) -> Option<AnimWorldCoord> {
        let entity = self.substrate.entities.get(id)?;
        if crate::sim::movement::air_movement::current_fly_height(
            entity,
            self.resolved_terrain.as_ref(),
        ) > 10
            || !entity.was_dropped_as_bomb()
        {
            return None;
        }
        let location = position_world_coord(&entity.position);
        (super::detonation_anim::land_at(
            self,
            super::ProjectileCoord::new(location.x, location.y, location.z),
        ) == i32::from(LandType::Water.as_index()))
        .then_some(AnimWorldCoord {
            x: location.x,
            y: location.y,
            z: location.z,
        })
    }

    /// Unit737EBA..737F6B, after the ship-sinking gate: Wake at Location,
    /// then the last SplashList entry five leptons higher. Neither selection
    /// draws RNG; each Anim421EA0 constructor runs before the next one.
    pub(crate) fn unit_dropped_bomb_water_splash(
        &mut self,
        rules: &RuleSet,
        id: u64,
        anims: &mut Vec<ExplosionEffect>,
    ) -> bool {
        let Some(coord) = self.dropped_bomb_water_death_location(id) else {
            return false;
        };
        self.emit_death_anim(
            rules,
            anims,
            &rules.general.wake.name,
            DeathAnimSpawn::at(coord, 0),
        );
        if let Some(splash) = rules.combat_damage.splash_list.last() {
            self.emit_death_anim(
                rules,
                anims,
                splash,
                DeathAnimSpawn::at(
                    AnimWorldCoord {
                        z: coord.z.wrapping_add(5),
                        ..coord
                    },
                    0,
                ),
            );
        }
        true
    }

    /// The ship-sinking gate of the Unit NowDead block
    /// (`UnitClass::ReceiveDamage @ 0x00737C90`, `0x00737DE2..0x00737E32`):
    /// `Naval=` (`+0xCCE`), not `Underwater=` (`+0xD69`) or `Organic=`
    /// (`+0xD97`), `Weight=` (`+0x370`) at least `ShipSinkingWeight=` (the
    /// `FCOMP` skips on less or unordered), the unit's cell (`vt+0x1BC`) of
    /// LandType water (`+0xEC == 2`), and not being warped (`+0x271`, set by
    /// the Chronosphere at `0x0065F1F4` and the Teleport locomotor at
    /// `0x00719579`/`0x007198DA`). Such a unit sinks and never reaches
    /// `Death_Explosion`; `world::sinking` owns its retained lifetime. VERA has no
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
                    .cell(entity.position.rx(), entity.position.ry())
                    .is_some_and(|cell| cell.land_type == LandType::Water.as_index())
            })
            && entity.teleport_state().is_none()
    }

    /// `UnitClass::Death_Explosion @ 0x00738680`: one `Explosion=` anim and
    /// then one `DestroyAnim=` at the Location. Each Next pick (7386A7,
    /// 73881D) is followed by its actual Anim constructor, also when a crash
    /// impact7461D1 calls this shared owner outside ReceiveDamage.
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
                entity.veterancy.rank_kind(),
                object,
                Ability::Explodes,
            );
        let armed =
            object.ammo == -1 || entity.ammo().map_or(object.ammo, |ammo| ammo.current()) > 0;
        if !object.explosion_anims.is_empty() {
            let picked = pick_death_anim(&mut self.scenario_rng, &object.explosion_anims);
            let anim = if explodes && armed {
                object.explosion_anims.last().map_or(picked, String::as_str)
            } else {
                picked
            };
            self.emit_death_anim(rules, anims, anim, DeathAnimSpawn::at(coord, 0));
        }
        // `0x00738749..0x007387FC` sums the stored ore's value into a local
        // nothing reads and calls the ShakeScreen stub (`0x0048DED0`, a bare
        // `RET`): no effect.
        if !object.destroy_anims.is_empty() {
            let anim = pick_death_anim(&mut self.scenario_rng, &object.destroy_anims);
            self.emit_death_anim(rules, anims, anim, DeathAnimSpawn::at(coord, 0));
        }
    }

    /// The Aircraft death arm (`0x0041661F..0x0041668A`): after the
    /// allocation, one Scenario `Next()` (`0x00416649`) picks an `Explosion=`
    /// anim at the aircraft's coordinate (vt+0xA4 `0x0041BDD0` returns
    /// GetCoords), then its constructor runs inline. Aircraft play no `DestroyAnim=`.
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
        let anim = pick_death_anim(&mut self.scenario_rng, &object.explosion_anims);
        self.emit_death_anim(
            rules,
            anims,
            anim,
            DeathAnimSpawn::at(
                AnimWorldCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
                0,
            ),
        );
    }

    /// `BuildingClass::DestructionEffects @ 0x004415F0` from its entry to its
    /// SpawnSurvivors call: the damage fires go out, the centre mark is
    /// committed through `commit_smudge`, each foundation cell (in the
    /// `vt+0x108(0)` list order) gets an `Explosion=` anim, the death fuse is
    /// armed, the origin corner gets a `DestroyAnim=` anim and the killing
    /// call's `ignore_defenses` becomes NoSurvivor.
    pub(crate) fn building_destruction_effects(
        &mut self,
        rules: &RuleSet,
        building_id: u64,
        ignore_defenses: bool,
        anims: &mut Vec<ExplosionEffect>,
        mut commit_smudge: impl FnMut(&mut Simulation, SmudgeSpawnRequest),
    ) {
        // 1. `0x004415F9..0x00441617`.
        self.clear_building_damage_fire_slots(building_id, rules);
        let Some(entity) = self.substrate.entities.get(building_id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure {
            return;
        }
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let (rx, ry, z) = (
            entity.position.rx(),
            entity.position.ry(),
            entity.position.z(),
        );
        let location = position_world_coord(&entity.position);
        let owner = entity.owner();
        let selling =
            entity.mission.effective().known() == Some(crate::sim::mission::MissionType::Selling);

        // 5. `0x00441702..0x0044172F`: a computer house's `RevealToAll=`
        // building releases its sight for the current house
        // (`ReReveal(0, 0, 1, PlayerPtr)`).
        if object.reveal_to_all && !self.house_is_human_player(owner) {
            self.rereveal_sight_to_current_house(building_id, rules);
        }

        // 7. `0x0044177E..0x004418E7`.
        commit_smudge(
            self,
            building_center_smudge_request(rx, ry, i32::from(z), &object.foundation),
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
                    super::inviso_scatter::RandomDirectionSnap::Preserve,
                );
                let delay = self.scenario_rng.next_range_u32_inclusive(0, 3) as u16;
                let anim = pick_death_anim(&mut self.scenario_rng, &object.explosion_anims);
                self.emit_death_anim(
                    rules,
                    anims,
                    anim,
                    DeathAnimSpawn::at(
                        AnimWorldCoord {
                            x,
                            y,
                            z: location.z,
                        },
                        delay,
                    ),
                );
            }
        }

        // 12. `0x00441C3D..0x00441CA9`: the fuse (`+0x528`) restarts at the
        // frame, 0 long for an `Explodes=` type (TechnoType `+0xD15`) or a
        // building whose Get_Mission (vt+0x184, current else queued) is
        // Selling (0x13), else 8. ReceiveDamage then UnInits only a building
        // whose fuse has time left (`0x0044266B..0x00442690`); the others stay
        // alive at Health 0 until their Update retires them
        // (`0x00440072..0x004400F1`), their StageClass clock stopped: Rate 0
        // and a zero timer from the frame, Value kept (`0x00441C8F..0x00441CA9`).
        let outlives = object.explodes || selling;
        let frame = self.session.binary_frame() as i32;
        if let Some(building) = self.substrate.entities.get_mut(building_id) {
            building
                .pending_c4_detonation
                .arm_destruction(frame, if outlives { 0 } else { 8 });
            if outlives {
                let value = building.native_stage().value();
                building.restart_native_stage(value, frame, 0);
            }
        }

        // 13. `0x00441CAC..0x00441D64`: the pick is drawn even when the entry
        // is empty; the coordinate is vt+0xAC (`0x00459EF0`, Location minus
        // 0x80 on X and Y). A custom-palette type's anim takes its scheme.
        if !object.destroy_anims.is_empty() {
            let anim = pick_death_anim(&mut self.scenario_rng, &object.destroy_anims);
            let (base, _) = crate::sim::movement::ground_pose::building_render_order_parts(
                crate::sim::components::DriveCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
                false,
                false,
            );
            let spawn = DeathAnimSpawn::at(
                AnimWorldCoord {
                    x: base.x,
                    y: base.y,
                    z: base.z,
                },
                0,
            );
            let follow_up = object
                .custom_palette
                .then_some(DeathAnimFollowUp::ExtraBuildingRemap(owner));
            self.emit_death_anim(rules, anims, anim, DeathAnimSpawn { follow_up, ..spawn });
        }
        // `0x00441EFC..0x00441F0B`: after the Health 0 write (`0x00441F00`),
        // the killing call's IgnoreDefenses becomes NoSurvivor.
        if ignore_defenses && let Some(building) = self.substrate.entities.get_mut(building_id) {
            building.no_survivor.mark();
        }
    }
}

/// One Scenario `Next() % count` pick from a non-empty anim list.
pub(crate) fn pick_death_anim<'r>(rng: &mut SimRng, list: &'r [String]) -> &'r str {
    let index = (rng.next_u32() % list.len() as u32) as usize;
    &list[index]
}

#[cfg(test)]
#[path = "destruction_effects_tests.rs"]
mod tests;

/// `BuildingClass::DestructionEffects`' centre mark for a destroyed building.
pub(super) fn building_center_smudge_request(
    rx: u16,
    ry: u16,
    building_z: i32,
    foundation: &str,
) -> SmudgeSpawnRequest {
    let (foundation_w, foundation_h) = foundation_dimensions(foundation);
    SmudgeSpawnRequest::BuildingCenter {
        rx,
        ry,
        building_z,
        foundation_w: foundation_w as u8,
        foundation_h: foundation_h as u8,
    }
}
