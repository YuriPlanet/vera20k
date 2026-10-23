//! VoxelAnim749F30's ordinary debris visit and the shared bounce-contact walk.
//!
//! The object stays registered during constructors and damage callbacks.
//! Bounce owns physics; Object.Location advances only at the flight tail.
//! Remaining native paths: StartSound749F43 maintenance, Trailer74A70E's
//! distinct height conversion, AttachedSystem construction, and meteor child/
//! ore expiry. Stock GASTANK/PIECE author none of those producers; authored
//! types which do still miss those effects. No stock-activation exclusion is
//! claimed for the whole class.

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
use crate::sim::projectile::ProjectileCoord;
use crate::sim::voxel_anim::VoxelAnimAiStep;

/// The native caller's packet and target-coordinate contract. Anim423A01
/// asks virtual GetCoords and supplies a fresh local damage value; Voxel
///74A8A9 reads Location and74A91B passes the shared Type field by address.
#[derive(Clone, Copy)]
pub(crate) enum BounceContactDamage {
    Anim(i32),
    VoxelType(VoxelAnimTypeId),
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

    /// One ordinary VoxelAnimClass::AI749F30 visit. Original gamemd:
    /// prefix749F67, dry expiry74A144..225, contact74A7EF..949 and common
    /// UnInit74A700. The caller publishes the returned bridge notification.
    pub fn visit_voxel_anim(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(debris) = self.substrate.voxel_anims.get_mut(id) else {
            return false;
        };
        if !debris.lifecycle().object_alive() {
            return false;
        }
        let step = debris.begin_ai_visit();
        let type_id = debris.type_id();
        let context =
            UninitContext::new(rules, Some(rules.overlay_registry())).with_effects(effects);
        match step {
            VoxelAnimAiStep::Delete => {
                self.uninit_with_context(id, context);
                return false;
            }
            VoxelAnimAiStep::Expire => {
                let changed = self.voxel_anim_expiry(id, rules, effects);
                self.uninit_with_context(id, context);
                return changed;
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
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                log::warn!("voxel debris {id} left the supported bounce domain: {error}");
                self.uninit_with_context(id, context);
                return false;
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
                        changed |= self.apply_bounce_contact_damage(
                            body.position_leptons(),
                            kind.damage_radius,
                            BounceContactDamage::VoxelType(type_id),
                            warhead,
                            rules,
                            effects,
                        );
                    }
                }
            }
        }
        //74A949 has no Alive gate after its synchronous receiver walk.
        // A retained, now-dead body still receives this last Location write.
        if let Some(debris) = self.substrate.voxel_anims.get_mut(id) {
            debris.finish_flight();
        }
        changed
    }

    /// Natural Voxel expiry749FA2..74A225. The land/deck query reads the
    /// retained Location; the dry Anim, area and light read Bounce coords.
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
        if water
            && location.z < ground.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS)
            && !kind.is_meteor
        {
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
            (RAD_NO_ATTACKER, None, warhead_ref),
            effects,
        );
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
            let center = match target {
                CellObjectMember::Entity(id) => {
                    self.substrate.entities.get(id).map(|entity| match source {
                        BounceContactDamage::Anim(_) => object_center_xy(entity),
                        BounceContactDamage::VoxelType(_) => position_world_xy(&entity.position),
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
