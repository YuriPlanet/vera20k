//! VoxelAnim749F30's debris and meteor visit, the shared bounce-contact walk
//! and trigger action 43's constructor call.
//!
//! The object stays registered during constructors and damage callbacks.
//! Bounce owns physics; Object.Location advances only at the flight tail.
//! Remaining native paths: StartSound749F43 maintenance, Trailer74A70E's
//! distinct height conversion and AttachedSystem construction. No retail
//! [VoxelAnims] type authors StartSound, TrailerAnim or AttachedSystem;
//! authored types which do still miss those effects.

use glam::IVec3;

use super::{FrameEffects, Simulation, UninitContext};
use crate::rules::ruleset::RuleSet;
use crate::rules::voxel_anim_type::VoxelAnimTypeId;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::bounce::BounceOutcome;
use crate::sim::combat::{
    CombatLightRequest, DamageWriteback, EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags,
    TerrainDamageEvent, world_receiver,
};
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::{object_center_xy, position_world_xy};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::overlay_mark_host::RuntimeOverlayError;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::voxel_anim::VoxelAnimAiStep;

/// The native caller's packet and target-coordinate contract. Anim423A01
/// asks virtual GetCoords and supplies a fresh local damage value; Voxel
///74A8A9 reads Location and74A91B passes the shared Type field by address.
#[derive(Clone, Copy)]
pub(crate) enum BounceContactDamage {
    Anim(i32),
    /// The shared Type field, written back by each receiver.
    #[cfg_attr(feature = "ares", allow(dead_code))]
    VoxelType(VoxelAnimTypeId),
    /// Ares 3.0 VoxelAnimClass_Update_Damage (1005D640, hook 0x74A884):
    /// the Voxel walk with a copy of the type's Damage per object, each
    /// object's successor read before its receiver.
    #[cfg(feature = "ares")]
    AresVoxel(i32),
}

impl Simulation {
    /// Current shared VoxelAnimType+2F0, including concrete receiver writes.
    pub fn voxel_type_damage(&self, type_id: VoxelAnimTypeId, rules: &RuleSet) -> i32 {
        self.voxel_type_damage
            .get(type_id, rules.voxel_anim_type(type_id).damage)
    }

    pub(crate) fn write_voxel_type_damage(
        &mut self,
        type_id: VoxelAnimTypeId,
        value: i32,
        rules: &RuleSet,
    ) {
        let kind = rules.voxel_anim_type(type_id);
        self.voxel_type_damage
            .set(type_id, &kind.name, value, kind.damage);
    }

    /// Canonical sparse order for snapshot validation and the shared hash.
    pub fn voxel_type_damage_entries(&self) -> impl Iterator<Item = (VoxelAnimTypeId, i32)> + '_ {
        self.voxel_type_damage.entries()
    }

    /// Validate serialized identity metadata against the incoming bound Rules.
    pub fn voxel_type_damage_identity_entries(
        &self,
    ) -> impl Iterator<Item = (VoxelAnimTypeId, &str, i32)> + '_ {
        self.voxel_type_damage.identity_entries()
    }

    /// Move current values back to the process Rules owner after the scene
    /// stops, before a later native read/reset. Exact stored family names
    /// identify these types across the authority transfer, not Rust handles.
    pub fn retire_voxel_type_damage(&mut self) -> Vec<(String, i32)> {
        self.voxel_type_damage.take()
    }

    /// Original TAction43 PlayVoxelAnimAtWP6E2520, reached at6DECEF: the
    /// shared signed waypoint/Ground/bridge coordinate, then constructor
    /// 7493B0(VoxelAnimTypes[+90], coord, null owner); native returns true.
    /// Invalid Rust input fails instead of taking the original unchecked
    /// array read.
    pub(crate) fn trigger_play_voxel_anim_at_waypoint(
        &mut self,
        rules: &RuleSet,
        type_index: i32,
        waypoint: i32,
    ) -> bool {
        let Some(type_id) = u32::try_from(type_index)
            .ok()
            .filter(|&index| (index as usize) < rules.voxel_anim_type_count())
            .map(VoxelAnimTypeId)
        else {
            return false;
        };
        let Some([x, y, z]) = self.trigger_waypoint_coord(waypoint) else {
            return false;
        };
        if let Err(error) = self.emit_debris_voxel(
            type_id,
            rules.voxel_anim_type(type_id),
            None,
            IVec3::new(x, y, z),
            true,
            &mut Vec::new(),
        ) {
            log::warn!("trigger voxel anim {type_index} left the supported launch domain: {error}");
        }
        true
    }

    /// One VoxelAnimClass::AI749F30 visit. Original gamemd: prefix749F67,
    /// expiry749FA2..74A6FC, flight74A7AB..949 and common UnInit74A700.
    /// The caller publishes the returned bridge notification.
    pub fn visit_voxel_anim(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> Result<bool, RuntimeOverlayError> {
        let Some(debris) = self.substrate.voxel_anims.get_mut(id) else {
            return Ok(false);
        };
        if !debris.lifecycle().object_alive() {
            return Ok(false);
        }
        let step = debris.begin_ai_visit();
        let type_id = debris.type_id();
        let context =
            UninitContext::new(rules, Some(rules.overlay_registry())).with_effects(effects);
        match step {
            VoxelAnimAiStep::Delete => {
                self.uninit_with_context(id, context);
                return Ok(false);
            }
            VoxelAnimAiStep::Expire => {
                // Native expiry always reaches UnInit (`0x0074A700`). Ore
                // VERA cannot place is the expiry's last step: log it, keep
                // the area damage's bridge notice and retire the piece.
                let changed = self.voxel_anim_expiry(id, rules, effects);
                self.uninit_with_context(id, context);
                return Ok(changed);
            }
            VoxelAnimAiStep::Flight => {}
        }

        // Copy only the numeric body for the terrain query. The live object
        // and its old Location remain observable during all later callbacks.
        let mut body = self.substrate.voxel_anims.get(id).unwrap().bounce();
        let outcome = {
            let terrain = super::bounce_terrain::ResolvedBounceTerrain { sim: self, rules };
            body.update(&terrain)
        };
        //74A7B8..74A7D4: a meteor adds gravity back after Update.
        let outcome = outcome.and_then(|outcome| {
            if rules.voxel_anim_type(type_id).is_meteor {
                body.compensate_meteor_gravity()?;
            }
            Ok(outcome)
        });
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                log::warn!("voxel debris {id} left the supported bounce domain: {error}");
                self.uninit_with_context(id, context);
                return Ok(false);
            }
        };
        self.substrate
            .voxel_anims
            .get_mut(id)
            .unwrap()
            .replace_bounce(body);

        let mut changed = false;
        match outcome {
            BounceOutcome::Falling => {}
            BounceOutcome::Stopped => {
                self.substrate
                    .voxel_anims
                    .get_mut(id)
                    .unwrap()
                    .stop_flight();
            }
            BounceOutcome::Bounced => {
                let old_location = self.substrate.voxel_anims.get(id).unwrap().world_coord();
                //74A7EF..829 selects Water from retained Object.Location,
                // not from the newly integrated Bounce position.
                if self.bounce_cell_is_water(old_location, rules) {
                    self.substrate
                        .voxel_anims
                        .get_mut(id)
                        .unwrap()
                        .stop_flight();
                } else {
                    let kind = rules.voxel_anim_type(type_id);
                    if let Some(name) = kind.bounce_anim.as_deref() {
                        self.spawn_named_anim(rules, name, anim_coord(old_location), 0, 0x600, 0);
                    }
                    if let Some(name) = kind.warhead.as_deref() {
                        let warhead = self.interner.intern(name);
                        // 1005D640: a negative DamageRadius or a Damage of
                        // zero skips the walk.
                        #[cfg(feature = "ares")]
                        let source = {
                            let damage = self.voxel_type_damage(type_id, rules);
                            (kind.damage_radius >= 0 && damage != 0)
                                .then_some(BounceContactDamage::AresVoxel(damage))
                        };
                        #[cfg(not(feature = "ares"))]
                        let source = Some(BounceContactDamage::VoxelType(type_id));
                        if let Some(source) = source {
                            changed |= self.apply_bounce_contact_damage(
                                body.position_leptons(),
                                kind.damage_radius,
                                source,
                                warhead,
                                rules,
                                effects,
                            );
                        }
                    }
                }
            }
        }
        //74A949 has no Alive gate after its synchronous receiver walk.
        // A retained, now-dead body still receives this last Location write.
        if let Some(debris) = self.substrate.voxel_anims.get_mut(id) {
            debris.finish_flight();
        }
        Ok(changed)
    }

    /// Natural Voxel expiry749FA2..74A6FC. The land/deck query reads the
    /// retained Location; the dry Anim, area, light, children and ore read
    /// Bounce coords. A water landing below the deck ends at its splash.
    fn voxel_anim_expiry(&mut self, id: u64, rules: &RuleSet, effects: FrameEffects<'_>) -> bool {
        let debris = self.substrate.voxel_anims.get(id).unwrap();
        let type_id = debris.type_id();
        let location = debris.world_coord();
        let impact = debris.bounce().position_leptons();
        let kind = rules.voxel_anim_type(type_id);
        let water = self.bounce_cell_is_water(location, rules);
        let ground = crate::sim::projectile::projectile_ground_z(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            ProjectileCoord::new(location.x, location.y, location.z),
        );
        let above =
            location.z >= ground.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        if water && !above {
            if kind.is_meteor {
                //74A035..099: the last SplashList entry at Location+5.
                if let Some(splash) = rules.combat_damage.splash_list.last() {
                    self.spawn_named_anim(
                        rules,
                        splash,
                        AnimWorldCoord {
                            z: location.z.wrapping_add(5),
                            ..anim_coord(location)
                        },
                        0,
                        0x600,
                        0,
                    );
                }
                return false;
            }
            self.spawn_named_anim(
                rules,
                &rules.general.wake.name,
                anim_coord(location),
                0,
                0x600,
                0,
            );
            if let Some(splash) = rules.combat_damage.splash_list.first() {
                self.spawn_named_anim(
                    rules,
                    splash,
                    AnimWorldCoord {
                        z: location.z.wrapping_add(10),
                        ..anim_coord(location)
                    },
                    0,
                    0x600,
                    0,
                );
            }
            return false;
        }
        let changed = self.voxel_anim_expire_damage(type_id, impact, rules, effects);
        let mut ore = Ok(());
        if kind.is_meteor {
            //74A288..301: two RandomRanged(0, SpawnCount) draws, then that
            // many children at the Bounce coords, each admitted before the
            // next. A Spawns name outside [VoxelAnims] still draws, but
            // constructs nothing: native would allocate a default Type.
            if let Some(child) = kind.spawns.as_deref().filter(|_| kind.spawn_count > 0) {
                let first = self
                    .scenario_rng
                    .next_range_i32_inclusive(0, kind.spawn_count);
                let second = self
                    .scenario_rng
                    .next_range_i32_inclusive(0, kind.spawn_count);
                if let Some(child_id) = rules.voxel_anim_type_id_by_name(child) {
                    for _ in 0..first.wrapping_add(second) {
                        if let Err(error) = self.emit_debris_voxel(
                            child_id,
                            rules.voxel_anim_type(child_id),
                            None,
                            impact,
                            true,
                            &mut Vec::new(),
                        ) {
                            log::warn!("meteor child {child} left the launch domain: {error}");
                            break;
                        }
                    }
                }
            }
            //74A303..572: ore in the eight neighbours of the impact cell,
            // AdjacentCell89F688 order, never the impact cell itself; each
            // looked up through GetCell5657A0.
            if kind.is_tiberium && !above {
                let ring = crate::sim::tiberium::ADJACENT_OFFSETS.map(|(dx, dy)| {
                    (
                        (impact.x / 256).wrapping_add(dx) as u16,
                        (impact.y / 256).wrapping_add(dy) as u16,
                    )
                });
                ore = self.voxel_anim_place_tiberium(rules, &ring, effects);
            }
        } else if kind.is_tiberium
            && !above
            && let Some(cell) = Self::map_coord_cell(&self.resolved_terrain, impact.x, impact.y)
        {
            //74A5B6..6F7: ore in the impact cell (565730).
            ore = self.voxel_anim_place_tiberium(rules, &[cell], effects);
        }
        if let Err(error) = ore {
            log::warn!("voxel debris {id} expired without placing its ore: {error}");
        }
        changed
    }

    /// Expiry74A13E..225: without ExpireAnim, no anim, area or light.
    fn voxel_anim_expire_damage(
        &mut self,
        type_id: VoxelAnimTypeId,
        impact: IVec3,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let kind = rules.voxel_anim_type(type_id);
        let Some(expire) = kind.expire_anim.as_deref() else {
            return false;
        };
        self.spawn_named_anim(rules, expire, anim_coord(impact), 0, 0x2600, -30);
        let Some(warhead) = kind.warhead.as_deref().and_then(|name| rules.warhead(name)) else {
            return false;
        };
        let warhead_ref = self.interner.intern(&warhead.id);
        let coord = ProjectileCoord::new(impact.x, impact.y, impact.z);
        let damage = self.voxel_type_damage(type_id, rules);
        let changed = world_receiver::apply_area_damage(
            self,
            rules,
            Some(rules.overlay_registry()),
            coord,
            damage,
            warhead,
            true,
            (RAD_NO_ATTACKER, None, warhead_ref),
            effects,
        )
        .bridge_state_changed;
        //74A1F6..225 reloads the shared Type value after the nested area.
        let damage = self.voxel_type_damage(type_id, rules);
        self.combat_light_requests.emit(CombatLightRequest::Impact {
            target_id: None,
            damage,
            warhead_ref,
            coord,
            force_create: false,
            flags: 0,
        });
        changed
    }

    /// Expiry ore74A385..566 (meteor ring) / 74A5F0..6F7 (ordinary): an
    /// inline copy of PlaceTiberium487190's new-cell arm with amount 0
    /// ([`crate::sim::tiberium::place_new_tiberium`]), for the Tiberium of
    /// overlay 0x7F (OverlayToTiberiumIndex5FDD20; TIB2_01, Vinifera, in
    /// retail). The ring dirties one union rectangle and the ordinary arm
    /// none; both mark radar per cell. When overlay 0x7F is not Tiberium,
    /// native reads Tiberiums[-1]; VERA places nothing there.
    fn voxel_anim_place_tiberium(
        &mut self,
        rules: &RuleSet,
        cells: &[(u16, u16)],
        effects: FrameEffects<'_>,
    ) -> Result<(), RuntimeOverlayError> {
        use crate::sim::tiberium::{NewTiberiumAdmission, TiberiumPlacementObjectContext};

        let registry = rules.overlay_registry();
        let mut placed = Vec::new();
        for &requested in cells {
            // VoxelAI74A38C..74A3A2 /74A5ED..74A602 first admits this
            // cell, then classifies literal Overlay127. A blocked landing
            // must not dereference an otherwise unused null Tiberium image.
            let admitted = {
                let (Some(terrain), Some(grid)) =
                    (self.resolved_terrain.as_ref(), self.overlay_grid.as_ref())
                else {
                    return Ok(());
                };
                let objects = TiberiumPlacementObjectContext::new(
                    &self.substrate.entities,
                    &self.substrate.occupancy,
                    &self.substrate.raw_cell_occupation,
                    rules,
                    &self.interner,
                    self.terrain_objects.cell_index(),
                );
                crate::sim::tiberium::admit_new_tiberium_target(
                    grid,
                    self.terrain_objects.spawning_cells(),
                    NewTiberiumAdmission::runtime(terrain, objects),
                    requested,
                )
            };
            let Some(cell) = admitted else {
                continue;
            };
            let Some(type_id) = registry.tiberium_type_for_overlay(&rules.tiberium_types, 0x7F)?
            else {
                continue;
            };
            if let Some(placement) = crate::sim::tiberium::place_new_tiberium(
                self, rules, registry, cell, type_id, 0, effects,
            )? {
                placed.push(placement.cell());
            }
        }
        // Ordinary ore keeps the existing batch Recalc policy. The shared
        // constructor has already completed each lifecycle and special Mark
        // callback, including nested CellAnim/Start, before the next cell.
        if !placed.is_empty() {
            self.publish_tiberium_cells(rules, registry, &placed);
        }
        Ok(())
    }

    /// Anim4239D3..23A88 / Voxel74A884..929: select the live ground-list
    /// head, test wrapping Manhattan XY radius, dispatch one concrete
    /// receiver, then read Object.Next after all synchronous callbacks.
    pub(crate) fn apply_bounce_contact_damage(
        &mut self,
        position: IVec3,
        radius: i32,
        source: BounceContactDamage,
        warhead_ref: InternedId,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(cell) = Self::map_coord_cell(&self.resolved_terrain, position.x, position.y)
        else {
            return false;
        };
        let mut current = self.cell_objects(cell, MovementLayer::Ground).next();
        let mut changed = false;
        while let Some(target) = current {
            #[cfg(feature = "ares")]
            let ares_next = matches!(source, BounceContactDamage::AresVoxel(_))
                .then(|| self.next_cell_object(target));
            let center = match target {
                CellObjectMember::Entity(id) => {
                    self.substrate.entities.get(id).map(|entity| match source {
                        BounceContactDamage::Anim(_) => object_center_xy(entity),
                        BounceContactDamage::VoxelType(_) => position_world_xy(&entity.position),
                        #[cfg(feature = "ares")]
                        BounceContactDamage::AresVoxel(_) => position_world_xy(&entity.position),
                    })
                }
                CellObjectMember::Terrain(id) => self.terrain_objects.get(id).map(|tree| {
                    let coord = tree.world_coord();
                    [coord.x, coord.y]
                }),
            };
            if let Some([x, y]) = center {
                let distance = position
                    .x
                    .wrapping_sub(x)
                    .wrapping_abs()
                    .wrapping_add(position.y.wrapping_sub(y).wrapping_abs());
                if distance <= radius {
                    let distance_leptons = crate::util::native_x87::adjust_for_z_standard(distance);
                    let (damage, writeback) = match source {
                        BounceContactDamage::Anim(value) => (value, None),
                        BounceContactDamage::VoxelType(id) => (
                            self.voxel_type_damage(id, rules),
                            Some(DamageWriteback::VoxelType(id)),
                        ),
                        #[cfg(feature = "ares")]
                        BounceContactDamage::AresVoxel(value) => (value, None),
                    };
                    let receipt = match target {
                        CellObjectMember::Entity(id) => self.commit_direct_damage_receiver(
                            rules,
                            Some(rules.overlay_registry()),
                            EntityDamageEvent::direct_receiver(
                                id,
                                damage,
                                distance_leptons,
                                RAD_NO_ATTACKER,
                                None,
                                warhead_ref,
                                ReceiverCallFlags {
                                    ignore_defenses: false,
                                    arg6: false,
                                },
                            )
                            .with_damage_writeback(writeback),
                            effects,
                        ),
                        CellObjectMember::Terrain(id) => {
                            let tree = self.terrain_objects.get(id).unwrap();
                            let event = TerrainDamageEvent {
                                stable_id: id,
                                rx: tree.cell().0,
                                ry: tree.cell().1,
                                damage,
                                distance_leptons,
                                warhead_ref,
                                near_center_ic_isolation_eligible: false,
                                damage_writeback: writeback,
                            };
                            self.commit_direct_terrain_damage_receiver(
                                rules,
                                Some(rules.overlay_registry()),
                                event,
                                false,
                                effects,
                            )
                        }
                    };
                    changed |= receipt.bridge_state_changed;
                }
            }
            #[cfg(feature = "ares")]
            if let Some(next) = ares_next {
                current = next;
                continue;
            }
            current = self.next_cell_object(target);
        }
        changed
    }
}

fn anim_coord(coord: IVec3) -> AnimWorldCoord {
    AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    }
}
