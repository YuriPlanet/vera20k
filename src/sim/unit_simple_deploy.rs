//! Unit simple deployment,739AC0/739CD0 and Mission_Unload73DE6E.
//! The Unit Mission leaf owns6E0..6E2; Techno owns its one Stage and retained
//! deployment Anim. Jumpjet consumes the same landing-for-deploy byte134.
//! Evidence and executable boundary controls: tools/spatial_oracle/unit_simple_deploy.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::{AnimClassSpawnDescriptor, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;

pub(crate) fn is_simple_deployer(sim: &Simulation, entity: &GameEntity, rules: &RuleSet) -> bool {
    entity.category == EntityCategory::Unit
        && sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                object.is_simple_deployer
                    && object.passengers <= 0
                    && !object.harvester
                    && !object.weeder
                    && object.deploys_into.is_none()
            })
}

impl Simulation {
    /// The Unit SimpleDeployer arm of CanDeploySlashUnload700D50:
    /// Cell484AE0 precedes the Foot+684 tube-state gate, then Type+E13 admits.
    /// EMP timer504 has no active Rust producer; its separate mechanism remains
    /// unrepresented, as in the shared command admission owner.
    pub fn can_simple_deploy(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        is_simple_deployer(self, entity, rules)
            && entity.low_bridge_tube_state.is_none()
            && !self.resolved_terrain.as_ref().is_some_and(|terrain| {
                terrain.is_tube_deploy_neighborhood((
                    entity.position.rx as i16,
                    entity.position.ry as i16,
                ))
            })
    }

    /// Unload73DE6E..73DEC8 observes6E0 before choosing its updater. Pending
    /// animation/landing returns1; completed work queues/commences Guard then
    /// FootUnload4DA2B0 returns450. Neither tail consumes scenario RNG.
    pub(crate) fn unit_simple_mission_unload(
        &mut self,
        id: u64,
        rules: &RuleSet,
    ) -> Result<i32, String> {
        let deployed = self
            .substrate
            .entities
            .get(id)
            .ok_or("simple deploy receiver retired")?
            .is_fully_deployed();
        self.update_unit_simple_deploy(id, !deployed, rules)?;
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("simple deploy receiver retired")?;
        if entity.unit_deploying() || entity.landing_for_deploy() {
            return Ok(1);
        }
        self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Guard),
            0,
            self.session.binary_frame,
            &EntityReadyInputProvider,
        )
        .map_err(|error| error.to_string())?;
        self.mission_commence_exact(id, self.session.binary_frame)
            .map_err(|error| error.to_string())?;
        Ok(450)
    }

    /// UpdateSimpleDeployState739AC0 and Undeploy739CD0. The two native bodies
    /// share an attachment/Stage setup but have different admission/completion
    /// effects. AnimClass advances its own forward/reverse display clock.
    pub(crate) fn update_unit_simple_deploy(
        &mut self,
        id: u64,
        deploying: bool,
        rules: &RuleSet,
    ) -> Result<(), String> {
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("simple deploy receiver retired")?;
        let object = self
            .object_type(entity.type_ref(), rules)
            .ok_or("simple deploy type missing")?;
        let leaf = entity
            .mission_leaf
            .as_unit()
            .ok_or("simple deploy requires Unit")?;
        let height = crate::sim::movement::ground_pose::object_altitude_leptons(entity);
        if deploying {
            if !object.is_simple_deployer
                || (height > 0 && !object.deploy_to_land)
                || leaf.deployed() != 0
            {
                return Ok(());
            }
        } else if leaf.deployed() == 0 {
            return Ok(());
        }
        let deploy_to_land = object.deploy_to_land;
        let configured_anim = object.deploying_anim.clone();
        let active = if deploying {
            leaf.deploy_begin_active()
        } else {
            leaf.deploy_reverse_active()
        } != 0;
        let retained = entity.deploy_anim();
        if deploying && !entity.landing_for_deploy() && height > 0 {
            self.substrate
                .entities
                .get_mut(id)
                .expect("same Unit")
                .set_landing_for_deploy(true);
        }
        if active && let Some(anim_id) = retained {
            let anim = self
                .anim(anim_id)
                .ok_or("Unit retained an expired deployment animation")?;
            let config = rules
                .art()
                .anim_runtime_config(self.interner.resolve(anim.type_id))
                .ok_or("retained deployment animation type is unbound")?;
            // Original ADD/SUB wrap before a signed comparison. Owner Stage
            // increments positively even for the separately reversed Anim.
            let threshold = config
                .start
                .wrapping_add(config.end)
                .wrapping_sub(if deploying { 1 } else { 2 });
            let entity = self.substrate.entities.get_mut(id).expect("same Unit");
            if entity.native_stage().value() >= threshold {
                entity.mission_leaf.set_unit_deployed(u8::from(deploying));
                if deploying {
                    entity.mission_leaf.set_unit_deploy_begin_active(0);
                } else {
                    entity.mission_leaf.set_unit_deploy_reverse_active(0);
                    //739D37..739D67: only animated completion takes NearbyLocation.
                    if deploy_to_land
                        && let Some((rx, ry)) = self.techno_nearby_location(id, Some(id), rules)
                    {
                        self.assign_destination_represented(
                            id,
                            Some(NavTargetRef::cell(rx as u16, ry as u16)),
                            Some(rules),
                            None,
                        )
                        .map_err(|error| error.to_string())?;
                    }
                }
            }
        } else if !deploying
            || !self
                .substrate
                .entities
                .get(id)
                .expect("same Unit")
                .landing_for_deploy()
        {
            if let Some(name) = configured_anim {
                let anim_id = if let Some(anim_id) = retained {
                    anim_id
                } else {
                    self.start_unit_deploy_anim(id, &name, !deploying, rules)?
                };
                let anim = self
                    .anim(anim_id)
                    .ok_or("deployment animation missing after construction")?;
                let config = rules
                    .art()
                    .anim_runtime_config(self.interner.resolve(anim.type_id))
                    .ok_or("deployment animation type is unbound")?;
                let (start, rate) = (config.start, i32::from(config.rate_logic_frames));
                let entity = self.substrate.entities.get_mut(id).expect("same Unit");
                entity.restart_native_stage(start, self.session.binary_frame as i32, rate);
                if deploying {
                    entity.mission_leaf.set_unit_deploy_begin_active(1);
                } else {
                    entity.mission_leaf.set_unit_deploy_reverse_active(1);
                }
            } else {
                self.substrate
                    .entities
                    .get_mut(id)
                    .expect("same Unit")
                    .mission_leaf
                    .set_unit_deployed(u8::from(deploying));
            }
        }
        // Undeploy739E56..739E6C: IsDisguised41C010 then ClearDisguise746720.
        // The latter's radar invalidation callback changes no simulation state.
        if !deploying
            && let Some(disguise) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|e| e.disguise.as_mut())
            && disguise.is_disguised()
        {
            disguise.clear_unit();
        }
        // The native updaters request these sounds on every admitted visit,
        // independently of whether this call finished or merely waited.
        self.emit_deploy_action_sound(id, if deploying { 27 } else { 31 }, rules)
    }

    fn start_unit_deploy_anim(
        &mut self,
        id: u64,
        name: &str,
        reverse: bool,
        rules: &RuleSet,
    ) -> Result<u64, String> {
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("deployment animation owner retired")?;
        let position = &entity.position;
        let coord = crate::sim::movement::ground_pose::object_location(
            entity,
            self.resolved_terrain.as_ref(),
        );
        // GetRemapColour705D70 freezes the owner's current/disguised palette
        // at this producer, rather than following later owner/disguise changes.
        let remap_house = entity
            .disguise
            .as_ref()
            .filter(|state| state.is_disguised())
            .and_then(|state| state.house())
            .unwrap_or(entity.owner());
        let (rx, ry, sx, sy, z) = (
            position.rx,
            position.ry,
            position.sub_x,
            position.sub_y,
            position.z,
        );
        let type_name = self.interner.intern(name);
        let descriptor = AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            reverse,
            ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sx, sy, z)
        };
        let anim = self
            .spawn_anim_at_world(
                rules,
                descriptor,
                AnimWorldCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                },
            )
            .map_err(|error| error.to_string())?;
        self.substrate
            .entities
            .get_mut(id)
            .expect("same Unit")
            .retain_deploy_anim(anim);
        self.set_anim_owner_object(anim, Some(id), rules);
        self.set_anim_house_remap(anim, remap_house);
        Ok(anim)
    }
}

#[cfg(test)]
#[path = "unit_simple_deploy_tests.rs"]
mod tests;
