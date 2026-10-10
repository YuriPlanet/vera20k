//! Retained surface-ship death: Unit ReceiveDamage737E43 and AI7364A1.
//!
//! This owner alone mutates Techno+3CD/+3CE. The fatal receiver still runs
//! Mark(UP), passenger/crew work and its ordinary tail; +3CD then suppresses
//! immediate UnInit. A Chrono Warp's PostWarpValidation also sinks a living
//! object it leaves marked ([`Simulation::begin_warp_sinking`]). Foot AI skips Process, observes the sound edge, and Unit
//! AI lowers the raw coordinate until its terminal RecordKill/UnInit.
//! Native execution: tools/spatial_oracle/naval_occupants and naval_sink_tick.

use super::{SimSoundEvent, Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::movement::ground_pose::{ground_surface_z_at, position_world_coord};

/// Techno constructor6F2F2B/6F2F31 initializes both bytes to zero. Kept
/// separate from Foot+425/+426 (`GameEntity::crashing` and its sound edge).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct SinkingState {
    active: bool,
    sound_edge_seen: bool,
}

impl SinkingState {
    pub(crate) fn is_active(self) -> bool {
        self.active
    }

    pub(crate) fn is_default(self) -> bool {
        self == Self::default()
    }

    #[cfg(test)]
    fn sound_edge_seen(self) -> bool {
        self.sound_edge_seen
    }
}

impl Simulation {
    /// Unit737E43..737E58: restore Health/Alive, raise +3CD, then a second
    /// Stun. Keep cell marking and passenger/crew work at the receiver's
    /// existing postlude, after this call. Object's exact-zero callback has
    /// already consumed the kill before Destroy and Techno death effects.
    pub(crate) fn begin_ship_sinking(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.health.current = 1;
        entity.lifecycle.object_alive = true;
        entity.sinking.active = true;
        self.techno_death_stun(id, UninitContext::with_rules(rules));
    }

    /// PostWarpValidation's sink (`0x00718968..0x00718977`,
    /// `0x00718ABF..0x00718ACE`): +3CD and the Stun (vt+0x3A0) on a living
    /// object, which no receiver unmarked: Unit AI lowers it through
    /// SetLocation's marked arm until its terminal RecordKill/UnInit.
    pub(crate) fn begin_warp_sinking(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.sinking.active = true;
        self.techno_death_stun(id, UninitContext::with_rules(rules));
    }

    /// Foot4DABC7..4DACD7: voice first (no local-player gate), then the type
    /// SinkingSound or Rules+208 on Foot's existing sound handle. The falling
    /// edge releases that handle only when MoveSoundActive+53C is clear.
    pub(crate) fn sinking_edge_sounds(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let state = entity.sinking;
        if state.active == state.sound_edge_seen {
            return;
        }
        if state.active {
            let object = self.object_type(entity.type_ref(), rules);
            let voice = object.and_then(|object| object.voice_sinking.clone());
            let sound = object
                .and_then(|object| object.sinking_sound.clone())
                .or_else(|| rules.general.sinking_sound.clone());
            let coord = position_world_coord(&entity.position);
            if let Some(sound_id) = voice {
                self.sound_events.push(SimSoundEvent::VocAt {
                    sound_id,
                    audible_to: None,
                    rx: entity.position.rx,
                    ry: entity.position.ry,
                    sub_x: entity.position.sub_x,
                    sub_y: entity.position.sub_y,
                    world_z_leptons: coord.z,
                });
            }
            if let Some(sound) = sound {
                self.sound_events.push(SimSoundEvent::AnimationStarted {
                    anim_id: id,
                    sound_id: sound,
                    world: AnimWorldCoord {
                        x: coord.x,
                        y: coord.y,
                        z: coord.z,
                    },
                });
            }
        } else if !entity.move_sound.is_active() {
            self.sound_events
                .push(SimSoundEvent::ObjectSoundReleased { owner: id });
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.sinking.sound_edge_seen = state.active;
        }
    }

    /// Unit7364A1..7365BB, after Foot AI and before the class Alive/fire
    /// suffix. Returns true only when the terminal arm UnInit'd the hull.
    /// Every reached visit lowers raw Z by five; GetHeight subtracts the live
    /// floor (and OnBridge height). Strictly below -400 records another kill
    /// with no attacker, then UnInit, before the frame-mod-four wake gate.
    pub(crate) fn tick_ship_sinking(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category != EntityCategory::Unit || !entity.sinking.active {
            return false;
        }
        let mut coord = position_world_coord(&entity.position);
        let floor = ground_surface_z_at(
            [coord.x, coord.y],
            entity.on_bridge,
            self.resolved_terrain.as_ref(),
            None,
        )
        .unwrap_or(0);
        coord.z = coord.z.wrapping_sub(5);
        // Unit7364E3 calls vt+1B4, Foot SetLocation4DB810. The fatal
        // receiver's Mark(UP) makes its unmarked arm4DB868 applicable; a
        // Chrono Warp's sink leaves the hull marked, so it takes the marked
        // arm. Both rejoin the changed-coordinate/OpenTopped tail, not a
        // separate raw-Z setter. Native execution: naval_sink_tick.json.
        self.foot_set_location_marked(id, coord, Some(rules), registry);
        if coord.z.wrapping_sub(floor) < -400 {
            // UnitAI736500's terminal RecordKill(NULL) is a second callback,
            // not a second accounting implementation. Native naval_sink_tick
            // retains Health1 and records two total losses across both calls.
            self.record_the_kill(
                id,
                None,
                None,
                crate::sim::combat::KillCallback::Terminal,
                rules,
            );
            self.uninit_with_rules(id, rules);
            self.sound_events
                .push(SimSoundEvent::ObjectSoundReleased { owner: id });
            return true;
        }
        if self.session.binary_frame & 3 == 0 {
            // Scenario RandomRanged65C7E0 draws Y then X. Its rejection loop
            // is observable (seed zero spends three raw draws for this pair).
            let y = self.scenario_rng.next_range_i32_inclusive(-170, 170);
            let x = self.scenario_rng.next_range_i32_inclusive(-170, 170);
            let wake = AnimWorldCoord {
                x: coord.x.wrapping_add(x),
                y: coord.y.wrapping_add(y),
                z: floor,
            };
            let type_id = self.interner.intern(&rules.general.wake.name);
            let (rx, ry, sub_x, sub_y, z) = wake.to_cell_sub_z();
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
            };
            if let Err(error) = self.spawn_anim_at_world(rules, descriptor, wake) {
                log::debug!(
                    "sinking wake [{}] did not construct: {error}",
                    rules.general.wake.name
                );
            }
        }
        false
    }
}

#[cfg(test)]
#[path = "sinking_tests.rs"]
mod tests;
