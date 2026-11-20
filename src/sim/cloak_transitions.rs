//! Native cloak transition entry points and their arg-sensitive sound edge.

use super::{CloakRuntime, CloakStepTimer};
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::CloakAnimKind;
use crate::sim::world::Simulation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StartCloakingResult {
    pub transitioned: bool,
    pub play_sound: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StartUncloakingResult {
    pub transitioned: bool,
    pub play_sound: bool,
}

impl CloakRuntime {
    /// `TechnoClass::StartCloaking @ 0x00703770`. Native accepts states zero
    /// and three, performs the state/timer writes first, then plays the exact
    /// current coordinate only when the boolean sound-suppression argument is
    /// zero.
    pub(crate) fn start_cloaking(
        &mut self,
        now: i32,
        speed: i32,
        suppress_sound: bool,
    ) -> StartCloakingResult {
        if !matches!(self.state, 0 | 3) {
            return StartCloakingResult {
                transitioned: false,
                play_sound: false,
            };
        }
        self.state = 1;
        self.depth = 0;
        self.step_delta = 1;
        self.step_timer = CloakStepTimer::started(now, speed);
        self.pending_anim = Some(CloakAnimKind::Cloak);
        StartCloakingResult {
            transitioned: true,
            play_sound: !suppress_sound,
        }
    }

    /// `TechnoClass::StartUncloaking @ 0x007036C0`. Native's boolean argument
    /// is a sound-suppression flag: zero plays RulesClass+0x6A0 through
    /// `VocClass::PlayAt @ 0x007509E0`, one performs only the state writes.
    pub(crate) fn start_uncloaking(
        &mut self,
        now: i32,
        speed: i32,
        cloaking_stages: i32,
        suppress_sound: bool,
    ) -> StartUncloakingResult {
        if !matches!(self.state, 1 | 2) {
            return StartUncloakingResult {
                transitioned: false,
                play_sound: false,
            };
        }
        self.state = 3;
        self.depth = cloaking_stages.wrapping_sub(1) as u32;
        self.step_delta = -1;
        self.step_timer = CloakStepTimer::started(now, speed);
        self.pending_anim = Some(CloakAnimKind::Decloak);
        StartUncloakingResult {
            transitioned: true,
            play_sound: !suppress_sound,
        }
    }
}

impl CloakRuntime {
    /// Takes the anim edge this runtime recorded, if any.
    pub(crate) fn take_pending_anim(
        &mut self,
    ) -> Option<crate::sim::cloak_disguise::CloakAnimKind> {
        self.pending_anim.take()
    }
}

impl Simulation {
    /// Spawns the pending Phobos cloak/decloak anim
    /// (`TechnoClass_Cloak_CloakAnim @ 0x7037F7`,
    /// `TechnoClass_Uncloak_DecloakAnim @ 0x703736`,
    /// `Ext/Techno/Hooks.cpp:1405-1420`): the type list when non-empty, else
    /// `[AudioVisual]`, one `RandomRanged(0, len-1)` pick, at the techno
    /// center with it as invoker and owner object.
    pub(crate) fn drain_cloak_anim(&mut self, id: u64, rules: &RuleSet) {
        let Some(kind) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.cloak.as_mut())
            .and_then(|runtime| runtime.take_pending_anim())
        else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let (type_list, global_list) = match kind {
            CloakAnimKind::Cloak => (
                &object.phobos_cloak_anims,
                &rules.phobos_globals.cloak_anims,
            ),
            CloakAnimKind::Decloak => (
                &object.phobos_decloak_anims,
                &rules.phobos_globals.decloak_anims,
            ),
        };
        let list = if type_list.is_empty() {
            global_list
        } else {
            type_list
        };
        if list.is_empty() {
            return;
        }
        let index = self
            .scenario_rng
            .next_range_u32_inclusive(0, (list.len() - 1) as u32) as usize;
        let type_name = list[index].clone();
        let type_id = self.interner.intern(&type_name);
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        };
        let (rx, ry, sub_x, sub_y, level) = world.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
            type_id, rx, ry, sub_x, sub_y, level,
        );
        let owner = entity.owner();
        let Ok(anim) = self.spawn_anim_at_world(rules, descriptor, world) else {
            return;
        };
        self.substrate.set_anim_invoker(anim, Some(id), Some(owner));
        self.set_anim_owner_object(anim, Some(id), rules);
    }
}
