//! Naval+Organic Parasite progression, original6297F0/629720/629E90.
//! This is part of the Parasite owner, not another relationship or damage store.

use super::*;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::ObjectSubstrate;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53 as X, X87Value};
use crate::util::retail_trig::TrigTable;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct GrappleState {
    /// +44, cleared synchronously by the dedicated Anim expiry callback.
    pub(super) anim: Option<u64>,
    /// +48: entry0, initial animation1, roll2/3, impact4.
    phase: i32,
    stage: i32,
    delay: i32,
    /// +54: the held animation expired; not an independent liveness query.
    invalidated: bool,
}

#[cfg(any(test, feature = "test-support"))]
impl ParasiteState {
    pub(crate) fn grapple_timers_for_test(&self) -> (CdTimer, CdTimer) {
        (self.suppression, self.damage)
    }

    pub(crate) fn grapple_progress_for_test(&self) -> (i32, i32, i32, bool) {
        let progress = &self.grapple;
        (
            progress.phase,
            progress.stage,
            progress.delay,
            progress.invalidated,
        )
    }
}

impl Simulation {
    /// Shared AttachTo/ExitUnit/grapple cleanup. Delete broadcasts expiry before
    /// the held pointer and invalidation latch clear (62A7E0..62A84B).
    pub(crate) fn reset_parasite_grapple(&mut self, owner: u64, rules: Option<&RuleSet>) {
        let anim = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|entity| entity.parasite.class_mut())
            .and_then(|state| {
                state.grapple.phase = 0;
                state.grapple.stage = 0;
                state.grapple.delay = 0;
                state.grapple.anim
            });
        if let Some(anim) = anim {
            self.destroy_anim_with_context(anim, rules);
        }
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|entity| entity.parasite.class_mut())
        {
            state.grapple = Default::default();
        }
    }

    /// Helper629720: only a stage advance writes the Anim frame/timer.
    /// Reaching stage10 returns before writing; the new phase gets its own
    /// immediate helper visit in the caller.
    fn advance_grapple_stage(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        owner: u64,
        victim: u64,
    ) -> bool {
        let facing = substrate.entities.get(victim).map_or(0, |entity| {
            entity.body_facing_current(session.binary_frame())
        });
        let Some(state) = substrate
            .entities
            .get_mut(owner)
            .and_then(|entity| entity.parasite.class_mut())
        else {
            return false;
        };
        let (base, cadence) = match state.grapple.phase {
            0 | 1 => (0, 3),
            2 => (80, 4),
            _ => (160, 4),
        };
        state.grapple.delay = state.grapple.delay.wrapping_add(1);
        if state.grapple.delay < cadence {
            return false;
        }
        state.grapple.delay = 0;
        state.grapple.stage = state.grapple.stage.wrapping_add(1);
        if state.grapple.stage >= 10 {
            return true;
        }
        let frame = base + i32::from(dir_from_facing16(facing)) * 10 + state.grapple.stage;
        if let Some(anim) = state.grapple.anim {
            Self::set_anim_manual_frame(session, substrate, anim, frame, 128);
        }
        false
    }

    fn start_grapple_roll(
        &mut self,
        owner: u64,
        victim: u64,
        origin: AnimWorldCoord,
        rules: &RuleSet,
    ) {
        let choice = self.scenario_rng.next_range_i32_inclusive(0, 1);
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(owner)
            .and_then(|entity| entity.parasite.class_mut())
        {
            state.grapple.phase = if choice == 0 { 3 } else { 2 };
            state.grapple.stage = 0;
            state.grapple.delay = 0;
        }
        Self::advance_grapple_stage(&self.session, &mut self.substrate, owner, victim);
        self.grapple_wakes(victim, origin, rules);
    }

    pub(super) fn parasite_grapple(
        &mut self,
        owner: u64,
        victim: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(source) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(kind) = self.object_type(source.type_ref(), rules) else {
            return;
        };
        let Some(weapon) = weapon_zero(rules, kind, source.veterancy.rank()) else {
            return;
        };
        let Some(warhead_name) = weapon.warhead.as_deref() else {
            return;
        };
        let Some(warhead) = rules.warhead(warhead_name) else {
            return;
        };
        let Some(target) = self.substrate.entities.get(victim) else {
            return;
        };
        let origin = ground_pose::position_world_coord(&target.position);
        let origin = AnimWorldCoord {
            x: origin.x,
            y: origin.y,
            z: origin.z,
        };
        //62983A: a present unsuitable Cell releases. MapClass's shared dummy
        // is present too; only an absent map in a component fixture skips it.
        if self.resolved_terrain.as_ref().is_some_and(|terrain| {
            !terrain.native_cell_is_water_set_tile(
                terrain.native_cell_identity((
                    target.position.rx() as i16,
                    target.position.ry() as i16,
                )),
            )
        }) {
            self.parasite_exit_unit(owner, rules, effects);
            if let Some(source) = self.substrate.entities.get_mut(owner) {
                source.parasite.end_paralysis(self.session.binary_frame());
            }
            return;
        }
        self.substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .parasite
            .paralysis = CdTimer::started(self.session.binary_frame() as i32, warhead.paralyzes);
        let phase = self
            .substrate
            .entities
            .get(owner)
            .unwrap()
            .parasite
            .class()
            .unwrap()
            .grapple
            .phase;
        match phase {
            0 => {
                self.substrate.write_grapple_roll(victim, SimFixed::ZERO);
                let name = crate::rules::effect_asset_catalog::SQUID_GRAPPLE_ANIM;
                // Lookup-only427CB0, not a factory creating an unknown type.
                let anim = if rules.anim_type_names.contains(name) {
                    let type_id = self.interner.intern(name);
                    let (rx, ry, sx, sy, z) = origin.to_cell_sub_z();
                    let descriptor = AnimClassSpawnDescriptor {
                        draw_flags: 0x600,
                        ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sx, sy, z)
                    };
                    self.spawn_anim_at_world(rules, descriptor, origin).ok()
                } else {
                    None
                };
                let state = self
                    .substrate
                    .entities
                    .get_mut(owner)
                    .unwrap()
                    .parasite
                    .class_mut()
                    .unwrap();
                state.grapple.anim = anim;
                state.grapple.phase = 1;
                state.grapple.stage = 0;
                state.grapple.delay = 0;
                Self::advance_grapple_stage(&self.session, &mut self.substrate, owner, victim);
            }
            1 => {
                if Self::advance_grapple_stage(&self.session, &mut self.substrate, owner, victim) {
                    self.start_grapple_roll(owner, victim, origin, rules);
                }
            }
            2 | 3 => {
                if Self::advance_grapple_stage(&self.session, &mut self.substrate, owner, victim) {
                    let state = self
                        .substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .parasite
                        .class_mut()
                        .unwrap();
                    state.grapple.phase = 4;
                    state.grapple.stage = 0;
                    state.grapple.delay = 0;
                    Self::advance_grapple_stage(&self.session, &mut self.substrate, owner, victim);
                }
                let state = &self
                    .substrate
                    .entities
                    .get(owner)
                    .unwrap()
                    .parasite
                    .class()
                    .unwrap()
                    .grapple;
                self.substrate
                    .write_grapple_roll(victim, grapple_roll(state.stage, phase == 2));
            }
            4 => {
                self.substrate.write_grapple_roll(victim, SimFixed::ZERO);
                self.grapple_splashes(victim, origin, rules);
                if warhead.culling && self.grapple_cull_admitted(owner, victim, rules) {
                    self.grapple_cull(owner, victim, rules, effects);
                    return;
                }
                let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                    victim,
                    weapon.damage,
                    0,
                    owner,
                    None,
                    self.interner.intern(warhead_name),
                    crate::sim::combat::ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: true,
                    },
                );
                self.commit_direct_damage_receiver(rules, registry, event, effects);
                if self
                    .substrate
                    .entities
                    .get(owner)
                    .and_then(|entity| entity.parasite.class())
                    .and_then(|state| state.victim)
                    == Some(victim)
                {
                    self.start_grapple_roll(owner, victim, origin, rules);
                } else {
                    self.reset_parasite_grapple(owner, Some(rules));
                }
            }
            _ => self.parasite_exit_unit(owner, rules, effects),
        }
        //629D97: attachment/remap/Z-adjust only when the Anim changes owner.
        let Some(anim) = self
            .substrate
            .entities
            .get(owner)
            .and_then(|entity| entity.parasite.class())
            .and_then(|state| state.grapple.anim)
        else {
            return;
        };
        if self
            .anim(anim)
            .is_some_and(|anim| anim.owner_entity() != Some(victim))
        {
            self.set_anim_owner_object(anim, Some(victim), rules);
            if let Some(source) = self.substrate.entities.get(owner) {
                self.substrate
                    .anims
                    .set_anim_house_remap(anim, source.owner());
            }
            if let Some(target) = self.substrate.entities.get(victim)
                && let Some(cell) = self
                    .resolved_terrain
                    .as_ref()
                    .and_then(|terrain| terrain.cell(target.position.rx(), target.position.ry()))
            {
                let frame = self.anim(anim).unwrap().runtime().current_frame;
                self.substrate.set_anim_frame_and_z_adjust(
                    anim,
                    frame,
                    crate::map::lighting::cell_ground_z_adjust(
                        self.session.lighting().normal().into(),
                        cell.level,
                    ),
                );
            }
        }
    }

    fn grapple_wakes(&mut self, victim: u64, origin: AnimWorldCoord, rules: &RuleSet) {
        let facing = self
            .substrate
            .entities
            .get(victim)
            .unwrap()
            .body_facing_current(self.session.binary_frame());
        for _ in 0..3 {
            let offset = [
                self.scenario_rng.next_range_i32_inclusive(-180, 180),
                self.scenario_rng.next_range_i32_inclusive(-64, 64),
            ];
            let [x, y] = rotate_grapple_offset(facing, offset);
            let position = AnimWorldCoord {
                x: origin.x.wrapping_add(x),
                y: origin.y.wrapping_add(y),
                z: origin.z,
            };
            self.spawn_grapple_effect(&rules.general.wake.name, position, 0, 0, rules);
        }
    }

    fn grapple_splashes(&mut self, victim: u64, origin: AnimWorldCoord, rules: &RuleSet) {
        let facing = self
            .substrate
            .entities
            .get(victim)
            .unwrap()
            .body_facing_current(self.session.binary_frame());
        for index in 0..3 {
            let lateral = if self.scenario_rng.next_range_i32_inclusive(0, 1) == 0 {
                64
            } else {
                -64
            };
            let [x, y] = rotate_grapple_offset(facing, [index * 128 - 128, lateral]);
            let selected = self
                .scenario_rng
                .next_range_i32_inclusive(0, rules.combat_damage.splash_list.len() as i32 - 1);
            if let Some(name) = rules.combat_damage.splash_list.get(selected as usize) {
                let position = AnimWorldCoord {
                    x: origin.x.wrapping_add(x),
                    y: origin.y.wrapping_add(y),
                    z: origin.z,
                };
                self.spawn_grapple_effect(name, position, 2, -10, rules);
            }
        }
    }

    fn spawn_grapple_effect(
        &mut self,
        name: &str,
        position: AnimWorldCoord,
        delay: i32,
        z_adjust: i32,
        rules: &RuleSet,
    ) {
        let type_id = self.interner.intern(name);
        let (rx, ry, sx, sy, z) = position.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay: delay as u16,
            draw_flags: 0x600,
            z_adjust,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sx, sy, z)
        };
        if let Err(error) = self.spawn_anim_at_world(rules, descriptor, position) {
            log::debug!("grapple effect [{name}] did not construct: {error}");
        }
    }

    fn grapple_cull_admitted(&self, owner: u64, victim: u64, rules: &RuleSet) -> bool {
        let Some(target) = self.substrate.entities.get(victim) else {
            return false;
        };
        let Some(kind) = self.object_type(target.type_ref(), rules) else {
            return false;
        };
        match crate::sim::combat::health_category(
            target.health.current(),
            kind.strength,
            rules.general.condition_red,
            rules.general.condition_yellow,
        ) {
            0 => true,
            1 => self.substrate.entities.get(owner).is_some_and(|source| {
                super::super::veterancy::raw_is_elite(source.veterancy.raw())
            }),
            _ => false,
        }
    }

    fn grapple_cull(
        &mut self,
        owner: u64,
        victim: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(source) = self.substrate.entities.get(owner) else {
            return;
        };
        let Some(source_type) = self.object_type(source.type_ref(), rules) else {
            return;
        };
        let Some(target) = self.substrate.entities.get(victim) else {
            return;
        };
        let Some(target_type) = self.object_type(target.type_ref(), rules) else {
            return;
        };
        let source_cost = self.cost_of(source.owner(), source_type, rules);
        let target_cost = self.cost_of(target.owner(), target_type, rules);
        if source_type.trainable {
            self.substrate
                .entities
                .get_mut(owner)
                .unwrap()
                .veterancy
                .award_kill(
                    source_cost,
                    target_cost,
                    true,
                    rules.general.veteran_ratio,
                    rules.general.veteran_cap,
                );
        }
        self.parasite_exit_unit(owner, rules, effects);
        self.begin_living_ship_sinking(
            victim,
            crate::sim::world::LivingSinkTrigger::ParasiteCull,
            rules,
            effects,
        );
        self.reset_parasite_grapple(owner, Some(rules));
        if let Some(source) = self.substrate.entities.get_mut(owner) {
            source.parasite.end_paralysis(self.session.binary_frame());
        }
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// PointerExpired62A484..62A490, reached by Anim's dedicated subscribers.
    pub(crate) fn parasite_anim_pointer_expired(&mut self, owner: u64, expired: u64) {
        if let Some(state) = self
            .entities
            .get_mut(owner)
            .and_then(|entity| entity.parasite.class_mut())
            && state.grapple.anim == Some(expired)
        {
            state.grapple.anim = None;
            state.grapple.invalidated = true;
        }
    }
    fn write_grapple_roll(&mut self, victim: u64, angle: SimFixed) {
        if let Some(entity) = self.entities.get_mut(victim) {
            // Native Techno6F2EB0..6F2EC2 initializes these inline fields to zero.
            // Rust stores them sparsely until a producer first writes them;
            // an ordinary admitted ship has no crash-created component yet.
            entity
                .rocking
                .get_or_insert_with(Default::default)
                .set_angle_sideways(angle);
        }
    }
}

///629A1A..629A46: stage * sign * double7E8AF0, table-sin, float7EF8F8.
/// Native stores binary32; the existing deterministic RockingState retains
/// I16F16, so the roll is quantized by at most one fixed-point unit.
fn grapple_roll(stage: i32, negative: bool) -> SimFixed {
    let stage = if negative {
        stage.wrapping_neg()
    } else {
        stage
    };
    let angle = X::mul(
        X::load_i32(stage),
        X::load_f64(NativeF64Bits::from_bits(0x3fd4_1b2f_769c_f0e0)).unwrap(),
    );
    let angle = X::load_f64(X::store_f64(angle).unwrap()).unwrap();
    let value = X::mul(
        TrigTable::embedded().sin_from_table(angle),
        X::load_f32(NativeF32Bits::from_bits(0x3f49_0fdb)).unwrap(),
    );
    SimFixed::from_num(f32::from_bits(X::store_f32(value).unwrap().bits()))
}

///629E90 and629A73 share the facing and final truncation. Use the existing
/// native word-to-radians owner, then FCHS, and the shared table lookup owner.
fn rotate_grapple_offset(facing: u16, [x, y]: [i32; 2]) -> [i32; 2] {
    let angle =
        X::neg(X::load_f64(crate::util::direction_tables::radians_from_facing16(facing)).unwrap());
    let table = TrigTable::embedded();
    let sin = table.sin_from_table(angle);
    let cos = table.cos_from_table(angle);
    let x: X87Value = X::load_i32(x);
    let y: X87Value = X::load_i32(y);
    [
        X::ftol_i64(X::sub(X::mul(x, cos), X::mul(y, sin))).unwrap() as i32,
        X::ftol_i64(X::add(X::mul(x, sin), X::mul(y, cos))).unwrap() as i32,
    ]
}
