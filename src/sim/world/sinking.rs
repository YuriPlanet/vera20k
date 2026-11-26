//! Retained sinking: Unit ReceiveDamage737E43, UnitAI7364A1 and AircraftAI414DB6.
//!
//! This owner alone mutates Techno+3CD/+3CE. The fatal receiver still runs
//! Mark(UP), passenger/crew work and its ordinary tail; +3CD then suppresses
//! immediate UnInit. PostWarpValidation and Squid culling also sink a living,
//! still-marked hull through [`Simulation::begin_living_ship_sinking`]. Foot AI
//! skips Process, observes the sound edge, and Unit/Aircraft AI lowers the raw
//! coordinate until its terminal RecordKill/UnInit.
//! Native execution: tools/spatial_oracle/naval_sink_tick.
//! Aircraft suffix control flow: AircraftClass::AI414DB6..414ED0.

use super::{SimSoundEvent, Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::StringInterner;
use crate::sim::movement::ground_pose::{ground_surface_z_at, position_world_coord};
use crate::sim::world::FrameChannel;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::util::forked::Forked;

/// Native callers of a living, still-marked hull's sink transition.
pub(crate) enum LivingSinkTrigger {
    PostWarpValidation,
    ParasiteCull,
}

/// Techno constructor6F2F2B/6F2F31 initializes both bytes to zero. Kept
/// separate from Foot+425/+426 (`GameEntity::crashing` and its sound edge).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SinkingState {
    active: bool,
    sound_edge_seen: bool,
    /// Sinking through broken ice (Firestorm 2.0 Unit+0x1F4, docs/ts-ice.md):
    /// Unit AI drifts the hull down instead of the ship sink's fall.
    ice: bool,
}

/// `ice` joins the hash only when set, so a game without ice hashes as before.
impl std::hash::Hash for SinkingState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.active.hash(state);
        self.sound_edge_seen.hash(state);
        if self.ice {
            self.ice.hash(state);
        }
    }
}

impl SinkingState {
    pub fn is_active(self) -> bool {
        self.active
    }

    pub(crate) fn is_default(self) -> bool {
        self == Self::default()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn sound_edge_seen(self) -> bool {
        self.sound_edge_seen
    }

    #[cfg(test)]
    pub(crate) fn is_ice(self) -> bool {
        self.ice
    }
}

impl Simulation {
    /// Unit737E43..737E58: restore Health/Alive, raise +3CD, then a second
    /// Stun. Keep cell marking and passenger/crew work at the receiver's
    /// existing postlude, after this call. Object's exact-zero callback has
    /// already consumed the kill before Destroy and Techno death effects.
    pub fn begin_ship_sinking(
        &mut self,
        id: u64,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.health.set(1);
        entity.lifecycle.restore_alive();
        entity.sinking.active = true;
        self.stun(id, UninitContext::new(rules).with_effects(frame_effects));
    }

    /// PostWarpValidation (`0x00718968..0x00718977`, `0x00718ABF..0x00718ACE`)
    /// and Squid culling (`0x00629C69..0x00629C80`) set +3CD and Stun
    /// (vt+0x3A0) on a living object, which no receiver unmarked. Unit/Aircraft AI lowers it through
    /// SetLocation's marked arm until its terminal RecordKill/UnInit.
    pub(crate) fn begin_living_ship_sinking(
        &mut self,
        id: u64,
        trigger: LivingSinkTrigger,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.sinking.active = true;
        // Squid629C69..629C80 announces after +3CD and before Stun.
        // Warp718968/718ABF has no announcement. Neither rewrites health,
        // Alive, marking or loss accounting as the fatal receiver does.
        if matches!(trigger, LivingSinkTrigger::ParasiteCull) {
            let announcement = self.substrate.entities.get(id).and_then(|entity| {
                crate::sim::combat::death_announcement_event(
                    self.object_type(entity.type_ref(), rules)?,
                    entity.type_ref(),
                    entity.category(),
                    entity.position.rx(),
                    entity.position.ry(),
                    entity.owner(),
                    rules.ares_form(),
                )
            });
            if let Some(event) = announcement {
                self.dispatch_unit_lost_events(&[event]);
            }
        }
        self.stun(id, UninitContext::new(rules).with_effects(frame_effects));
    }

    /// A unit sinking through broken ice (docs/ts-ice.md): Firestorm 2.0
    /// sets Unit+0x1F4 and calls Stun (vt+0x2E4) from Break_Ice
    /// (0x520c06..0x520c0c) and from Per_Cell_Process (0x651963..0x65196a).
    pub(crate) fn begin_ice_sinking(&mut self, id: u64, context: UninitContext<'_>) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.sinking.active = true;
        entity.sinking.ice = true;
        self.stun(id, context);
    }

    /// Foot4DABC7..4DACD7: voice first (no local-player gate), then the type
    /// SinkingSound or Rules+208 on Foot's existing sound handle. The falling
    /// edge releases that handle only when MoveSoundActive+53C is clear.
    /// `sound_events` is written only for an event (`util::forked`).
    pub(crate) fn sinking_edge_sounds(
        interner: &StringInterner,
        substrate: &mut ObjectSubstrate,
        sound_events: &mut Forked<FrameChannel<SimSoundEvent>>,
        id: u64,
        rules: &RuleSet,
    ) {
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        let state = entity.sinking;
        if state.active == state.sound_edge_seen {
            return;
        }
        if state.ice {
            // Firestorm's ice sink plays no sound; only the latch follows.
        } else if state.active {
            let object = interner.object_type(entity.type_ref(), rules);
            let voice = object.and_then(|object| object.voice_sinking.as_deref());
            let sound = object
                .and_then(|object| object.sinking_sound.as_deref())
                .or(rules.general.sinking_sound.as_deref());
            let coord = position_world_coord(&entity.position);
            if let Some(sound_id) = voice {
                sound_events.emit(SimSoundEvent::VocAt {
                    sound_id: rules.sound_name(sound_id),
                    audible_to: None,
                    rx: entity.position.rx(),
                    ry: entity.position.ry(),
                    sub_x: entity.position.sub_x(),
                    sub_y: entity.position.sub_y(),
                    world_z_leptons: coord.z,
                });
            }
            if let Some(sound) = sound {
                sound_events.emit(SimSoundEvent::AnimationStarted {
                    anim_id: id,
                    sound_id: rules.sound_name(sound),
                    world: AnimWorldCoord {
                        x: coord.x,
                        y: coord.y,
                        z: coord.z,
                    },
                });
            }
        } else if !entity.move_sound.is_active() {
            sound_events.emit(SimSoundEvent::ObjectSoundReleased { owner: id });
        }
        if let Some(entity) = substrate.entities.get_mut(id) {
            entity.sinking.sound_edge_seen = state.active;
        }
    }

    /// Unit7364A1..7365BB and Aircraft414DB6..414ED0, after Foot AI and before the class
    /// suffix. Returns true only when the terminal arm UnInit'd the hull.
    /// Every reached visit lowers raw Z by five; GetHeight subtracts the live
    /// floor (and OnBridge height). Strictly below -400 records another kill
    /// with no attacker, then UnInit, before the frame-mod-four wake gate.
    pub(crate) fn tick_ship_sinking(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if !matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Aircraft
        ) || !entity.sinking.active
        {
            return false;
        }
        if entity.sinking.ice {
            return self.tick_ice_sinking(id, rules, registry, frame_effects);
        }
        let mut coord = position_world_coord(&entity.position);
        let floor = ground_surface_z_at(
            [coord.x, coord.y],
            entity.on_bridge(),
            self.resolved_terrain.as_ref(),
            None,
        )
        .unwrap_or(0);
        coord.z = coord.z.wrapping_sub(5);
        // Unit7364E3/Aircraft414DF8 call vt+1B4, Foot SetLocation4DB810. The fatal
        // receiver's Mark(UP) makes its unmarked arm4DB868 applicable; a
        // Chrono Warp's sink leaves the hull marked, so it takes the marked
        // arm. Both rejoin the changed-coordinate/OpenTopped tail, not a
        // separate raw-Z setter. Native execution: naval_sink_tick.json.
        self.foot_set_location_marked(id, coord, rules, registry, frame_effects);
        if coord.z.wrapping_sub(floor) < -400 {
            // Unit736500/Aircraft414E15 call their actual RecordKill(NULL)
            // callback before UnInit. A fatal-receiver ship already recorded
            // a loss; living warp sinks retain their producer's accounting.
            self.record_the_kill(
                id,
                crate::sim::combat::KillSource::Techno(None),
                crate::sim::combat::KillCallback::Terminal,
                UninitContext::new(rules).with_effects(frame_effects),
            );
            self.uninit_with_context(id, UninitContext::new(rules).with_effects(frame_effects));
            self.sound_events
                .emit(SimSoundEvent::ObjectSoundReleased { owner: id });
            return true;
        }
        if self.session.binary_frame() & 3 == 0 {
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

    /// Firestorm 2.0 UnitClass::AI 0x64dfa3..0x64e122, after Foot AI: a unit
    /// sinking through ice moves three leptons along its body facing's
    /// eighth (table 0x80f818, whose north-west row is zero) and nine down
    /// through Set_Coord (vt+0x188). Below -400 leptons (Height_AGL,
    /// vt+0x19C) it records a kill with no attacker (vt+0xD0) and is deleted
    /// (vt+0xE4). No draw, wake or sound.
    fn tick_ice_sinking(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        const DRIFT: [(i32, i32); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (0, 0),
        ];
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let facing = entity.body_facing_current(self.session.binary_frame());
        let (dx, dy) = DRIFT[super::ice::facing_eighth(facing)];
        let on_bridge = entity.on_bridge();
        let mut coord = position_world_coord(&entity.position);
        coord.x = coord.x.wrapping_add(3 * dx);
        coord.y = coord.y.wrapping_add(3 * dy);
        coord.z = coord.z.wrapping_sub(9);
        self.foot_set_location_marked(id, coord, rules, registry, frame_effects);
        let floor = ground_surface_z_at(
            [coord.x, coord.y],
            on_bridge,
            self.resolved_terrain.as_ref(),
            None,
        )
        .unwrap_or(0);
        if coord.z.wrapping_sub(floor) >= -400 {
            return false;
        }
        self.record_the_kill(
            id,
            crate::sim::combat::KillSource::Techno(None),
            crate::sim::combat::KillCallback::Terminal,
            UninitContext::new(rules).with_effects(frame_effects),
        );
        self.uninit_with_context(id, UninitContext::new(rules).with_effects(frame_effects));
        self.sound_events
            .emit(SimSoundEvent::ObjectSoundReleased { owner: id });
        true
    }
}

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "sinking_tests.rs"]
mod tests;

#[cfg(all(test, tests_sim_world_p_s))]
#[path = "aircraft_sinking_tests.rs"]
mod aircraft_tests;
