//! `TunnelLocomotionClass` methods that move the Foot: Process
//! (`0x00728E30`) with its state helpers ProcessPreDigIn (`0x007291F0`),
//! ProcessDigging (`0x00729580`) and ProcessPreDigOut (`0x007298F0`), and
//! the Move_To (`0x00728AF0`) and Stop_Moving (`0x00728C00`) the Foot setter
//! and the class itself reach. The state they write is
//! [`TunnelRuntime`](super::tunnel_movement::TunnelRuntime)'s.
//!
//! `world::object_turn` calls [`Simulation::process_tunnel_locomotor`] as
//! the Foot's one Process. Each owner call runs where native makes it; the
//! runtime is read again after any call that can reach the class (the
//! arrival's PerCellProcess and the class's own Move_To and Stop_Moving).
//!
//! ProcessDiggingIn (`0x00729370`) has no caller: Process repeats its body
//! inline for State 2 (`0x00728E90`), so the Ares hook in it (`0x007293DA`)
//! never runs.
//!
//! Residuals:
//! - Move_To asks the owner's vt+0x37C, Unit `0x00746C90`: the EMP counter
//!   (Techno `+0x504`) or the Unit's death-frame counter (`+0x6D8`). VERA has
//!   no producer of the second, so only the EMP counter refuses.
//! - Move_To's type adjustment is UnitType's (`0x00747EB0`, the Z raised to
//!   the ground). The Foot setter reaches this Move_To only for a Unit; an
//!   Infantry Tunnel owner (InfantryType `0x005247D0`) has no represented
//!   setter arm.
//!
//! ## Dependency rules
//! - Part of sim/ — sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{AnimClassSpawnDescriptor, DriveCoord};
use crate::sim::find_nearby_cell::{
    NearbyArgs, NearbyFootprint, NearbySearchOptions, PassabilityArgs,
};
use crate::sim::world::{FrameEffects, Simulation, UninitContext};
use crate::util::fixed_math::SimFixed;

use super::ground_pose::{ground_surface_z_at, object_location};
use super::tunnel_movement::{
    self, DigEffects, NULL_COORD, TUNNEL_DEPTH_Z, TUNNEL_SURFACE_EFFECT_HEIGHT, TunnelRuntime,
    TunnelState,
};

/// The AnimClass constructor's draw flags the dig anim passes (`0x600`).
const DIG_ANIM_DRAW_FLAGS: u32 = 0x600;

/// What one Tunnel Process did, for the Foot's AI.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TunnelProcessOutcome {
    /// DiggingOut's arrival ran `PerCellProcess(2)` (`0x00729086`).
    pub(crate) per_cell_ran: bool,
    pub(crate) bridge_state_changed: bool,
}

impl Simulation {
    fn tunnel_runtime(&self, id: u64) -> Option<&TunnelRuntime> {
        self.substrate
            .entities
            .get(id)?
            .locomotor
            .as_ref()?
            .tunnel_runtime()
    }

    fn tunnel_runtime_mut(&mut self, id: u64) -> Option<&mut TunnelRuntime> {
        self.substrate
            .entities
            .get_mut(id)?
            .locomotor
            .as_mut()?
            .tunnel_runtime_mut()
    }

    fn tunnel_set_state(&mut self, id: u64, state: TunnelState) {
        if let Some(runtime) = self.tunnel_runtime_mut(id) {
            runtime.set_state(state);
        }
    }

    /// The owner's Location (`+0x9C`).
    fn tunnel_location(&self, id: u64) -> Option<DriveCoord> {
        let entity = self.substrate.entities.get(id)?;
        Some(object_location(entity, self.resolved_terrain.as_ref()))
    }

    /// `ObjectClass::GetHeight` (vt+0x1C8).
    fn tunnel_height(&self, id: u64) -> i32 {
        self.substrate.entities.get(id).map_or(0, |entity| {
            super::air_movement::current_fly_height(entity, self.resolved_terrain.as_ref())
        })
    }

    /// `Map__GetGroundHeightAtCoord @ 0x00578080`.
    fn tunnel_ground_z(&self, at: DriveCoord) -> i32 {
        ground_surface_z_at([at.x, at.y], false, self.resolved_terrain.as_ref(), None).unwrap_or(0)
    }

    /// `FootClass::SetLocation` (vt+0x1B4, `0x004DB810`): a marked owner
    /// leaves and rejoins its cell around the write.
    fn tunnel_set_location(
        &mut self,
        id: u64,
        coord: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.foot_set_location_marked(id, coord, rules, registry, effects);
    }

    /// The owner's type ROT (TechnoType `+0x71C`).
    fn tunnel_rot(&self, id: u64, rules: &RuleSet) -> i32 {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .map_or(0, |object| object.turret_rot)
    }

    /// `FootClass::GetCurrentSpeed` (vt+0x538, `0x004DB1A0`).
    fn tunnel_current_speed(&self, id: u64, rules: &RuleSet) -> i32 {
        self.substrate.entities.get(id).map_or(0, |entity| {
            super::foot_speed::owner_current_speed(
                entity,
                self.object_type(entity.type_ref(), rules),
                rules.general.veteran_speed,
                &self.houses,
            )
        })
    }

    /// `FootClass::SetSpeedFraction` (vt+0x544, `0x004D3710`) with 1.0 or 0.0.
    fn tunnel_set_speed_fraction(&mut self, id: u64, full: bool) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity
                .foot_speed
                .set_speed_fraction(if full { SimFixed::ONE } else { SimFixed::ZERO });
        }
    }

    /// The dig sound and, with `effects.anim`, the dig anim at the owner's
    /// Location: stock `VocClass::PlayAt([AudioVisual] DigSound=)` and
    /// `AnimClass([AudioVisual] Dig=, Location, 0, 1, 0x600, 0, 0)`.
    pub(crate) fn tunnel_dig_effects(&mut self, id: u64, effects: DigEffects, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let (sound, anim) = self.tunnel_dig_effect_types(entity, effects, rules);
        if let Some(sound) = sound {
            self.sound_events
                .emit(crate::sim::world::SimSoundEvent::voc_at_location(
                    sound,
                    entity,
                    self.resolved_terrain.as_ref(),
                ));
        }
        if !effects.anim {
            return;
        }
        let Some(anim) = anim else {
            return;
        };
        let Some(location) = self.tunnel_location(id) else {
            return;
        };
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: location.x,
            y: location.y,
            z: location.z,
        };
        let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
        let type_id = self.interner.intern(&anim);
        let descriptor = AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: DIG_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
        };
        if let Err(error) = self.spawn_anim_at_world(rules, descriptor, world) {
            log::debug!("Tunnel dig anim [{anim}] did not construct: {error}");
        }
    }

    /// The sound and anim a dig effect names: `[AudioVisual] DigSound=` and
    /// `Dig=`. Ares 3.0 replaces the four stock call sites with
    /// `PlayDigEffects` (Ares.dll `0x100443E0`; hooks `0x728EF0`, `0x7292CF`,
    /// `0x7297C4`, `0x7299A9`), which asks the type's `DigInSound`/`DigIn`
    /// or `DigOutSound`/`DigOut` first.
    pub(super) fn tunnel_dig_effect_types(
        &self,
        entity: &crate::sim::game_entity::GameEntity,
        effects: DigEffects,
        rules: &RuleSet,
    ) -> (Option<String>, Option<String>) {
        let stock = (
            rules.general.dig_sound.clone(),
            rules.general.dig_anim.clone(),
        );
        #[cfg(feature = "ares")]
        if let Some(object) = self.object_type(entity.type_ref(), rules) {
            let [sound, anim] = object.techno_combat().dig_effects(effects.dig_in);
            return (
                sound.map_or(stock.0, |sound| sound.map(str::to_owned)),
                anim.map_or(stock.1, |anim| anim.map(str::to_owned)),
            );
        }
        #[cfg(not(feature = "ares"))]
        let _ = (entity, effects);
        stock
    }

    /// `ILocomotion::Move_To` (`0x00728AF0`). A refusing owner keeps the
    /// object as it was: EMP or the death-frame counter (vt+0x37C), a
    /// parasite's paralysis (vt+0x380), warping out (vt+0x1D4) or in
    /// (vt+0x1D8). Otherwise Destination takes `to` with its Z raised to the
    /// ground (UnitType `0x00747EB0`); an owner below the ground that is
    /// Idle, Digging or DugOut leaves its cell (Mark(UP)) and travels on
    /// (State 4); an Idle object starts PreDigIn.
    pub(crate) fn tunnel_move_to(
        &mut self,
        id: u64,
        to: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame();
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if self.tunnel_runtime(id).is_none()
            || actor.emp_remaining() > 0
            || actor.parasite.is_paralyzed(frame)
            || actor.is_warped_out()
            || actor.is_warping_in()
        {
            return;
        }
        let ground = self.tunnel_ground_z(to);
        let stored = DriveCoord {
            z: to.z.max(ground),
            ..to
        };
        let height = self.tunnel_height(id);
        let runtime = self.tunnel_runtime_mut(id).expect("checked Tunnel");
        runtime.set_destination(stored);
        if to != NULL_COORD
            && height < 0
            && matches!(
                runtime.state(),
                TunnelState::Idle | TunnelState::Digging | TunnelState::DugOut
            )
        {
            self.foot_mark_remove(id, rules, registry, effects);
            self.tunnel_set_state(id, TunnelState::Digging);
        }
        let runtime = self.tunnel_runtime_mut(id).expect("checked Tunnel");
        if runtime.state() == TunnelState::Idle {
            runtime.set_state(TunnelState::PreDigIn);
        }
    }

    /// `ILocomotion::Stop_Moving` (`0x00728C00`): the State table
    /// ([`TunnelRuntime::stop_moving`]), and for an owner travelling below
    /// the cells farther than a cell diagonal from Destination, a new
    /// Destination at the centre of a passable cell near it, at Z 0. With no
    /// such cell the owner takes its own Health as C4Warhead damage and
    /// Destination is nulled.
    pub(crate) fn tunnel_stop_moving(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(runtime) = self.tunnel_runtime_mut(id) else {
            return;
        };
        if !runtime.stop_moving() {
            return;
        }
        let destination = runtime.raw_destination();
        let Some(location) = self.tunnel_location(id) else {
            return;
        };
        if !tunnel_movement::beyond_cell_diagonal(location, destination) {
            return;
        }
        // 0x00728C94..0x00728D5A: Track, MovementZone Normal, then any zone.
        let cell = self.tunnel_owner_cell(id);
        let found = self
            .tunnel_nearby_cell(
                cell,
                crate::rules::locomotor_type::MovementZone::Normal,
                true,
            )
            .or_else(|| {
                self.tunnel_nearby_cell(
                    cell,
                    crate::rules::locomotor_type::MovementZone::Normal,
                    false,
                )
            });
        match found {
            Some((x, y)) => {
                if let Some(runtime) = self.tunnel_runtime_mut(id) {
                    runtime.set_destination(DriveCoord {
                        x: i32::from(x) * 256 + 128,
                        y: i32::from(y) * 256 + 128,
                        z: 0,
                    });
                }
            }
            None => {
                self.receive_own_health_c4(id, rules, registry, effects);
                if let Some(runtime) = self.tunnel_runtime_mut(id) {
                    runtime.set_destination(NULL_COORD);
                }
            }
        }
    }

    /// `ObjectClass::Get_Cell_Packed` (vt+0x1B8): the Location's cell.
    fn tunnel_owner_cell(&self, id: u64) -> (i32, i32) {
        self.tunnel_location(id)
            .map_or((0, 0), |location| (location.x / 256, location.y / 256))
    }

    /// Find_Nearby_Passable_Cell as Stop_Moving and ProcessDigging push it:
    /// Track; the zone `GetZoneID(seed, Normal, no bridge)` gives, or any
    /// zone; `movement_zone`; not bridge-aware; 1x1; no overlay or height
    /// gate; the burrow test; bridge cells allowed; no target cell; no
    /// occupancy check.
    fn tunnel_nearby_cell(
        &self,
        seed: (i32, i32),
        movement_zone: crate::rules::locomotor_type::MovementZone,
        seed_zone: bool,
    ) -> Option<(u16, u16)> {
        let required_zone_id = if seed_zone {
            self.zone_grid
                .as_ref()
                .zip(self.resolved_terrain.as_ref())
                .and_then(|(zones, terrain)| {
                    zones.get_zone_id_native(
                        terrain,
                        (seed.0 as u16, seed.1 as u16),
                        crate::rules::locomotor_type::MovementZone::Normal,
                        false,
                    )
                })
        } else {
            None
        };
        self.nearby_passable_cell(
            seed,
            NearbyArgs {
                passability: PassabilityArgs {
                    speed_type: crate::rules::locomotor_type::SpeedType::Track,
                    required_zone_id,
                    movement_zone,
                    bridge_aware_zone: false,
                },
                footprint: NearbyFootprint::SINGLE,
                allow_bridge_cells: true,
                check_height: false,
                check_occupancy: false,
                target_cell: None,
                require_burrowable: true,
                options: NearbySearchOptions::default(),
            },
        )
    }

    /// `ILocomotion::Process` (`0x00728E30`) of an active Tunnel: nothing
    /// while Idle; otherwise its State's step, then the shroud deselect
    /// (`0x007290F6..0x00729189`) and the damage-smoke test (`0x00729192`).
    pub(crate) fn process_tunnel_locomotor(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> TunnelProcessOutcome {
        let mut outcome = TunnelProcessOutcome::default();
        let Some(state) = self
            .tunnel_runtime(id)
            .filter(|runtime| runtime.is_moving())
            .map(TunnelRuntime::state)
        else {
            return outcome;
        };
        let frame = self.session.binary_frame();
        let height = self.tunnel_height(id);
        let coord_z = self.tunnel_location(id).map_or(0, |location| location.z);
        match state {
            TunnelState::Idle => {}
            TunnelState::PreDigIn => {
                if height < 0 {
                    self.tunnel_set_state(id, TunnelState::DugIn);
                } else {
                    self.tunnel_pre_dig_in(id, rules);
                }
            }
            TunnelState::DiggingIn => {
                if height < 0 {
                    self.tunnel_set_state(id, TunnelState::DugIn);
                } else if self
                    .tunnel_runtime(id)
                    .is_some_and(|runtime| runtime.dig_timer_done(frame))
                {
                    // 0x00728ECC..0x00728F80.
                    self.tunnel_set_speed_fraction(id, true);
                    self.tunnel_set_state(id, TunnelState::DugIn);
                    self.tunnel_dig_effects(id, DigEffects::DIG_IN, rules);
                    let context = UninitContext::new(rules, registry).with_effects(effects);
                    self.detach_all(id, false, context);
                }
            }
            TunnelState::DugIn => self.tunnel_dug_in(id, coord_z, rules, registry, effects),
            TunnelState::Digging => self.tunnel_digging(id, rules, registry, effects),
            TunnelState::PreDigOut => self.tunnel_pre_dig_out(id, rules, registry, effects),
            TunnelState::DiggingOut => {
                if self
                    .tunnel_runtime(id)
                    .is_some_and(|runtime| runtime.dig_timer_done(frame))
                {
                    // 0x0072907F..0x007290B5: PerCellProcess(2), then a live
                    // owner on the map stops itself and ends the move.
                    match self.per_cell_process(
                        id,
                        super::PerCellReason::Arrival,
                        rules,
                        registry,
                        effects,
                    ) {
                        Ok(changed) => outcome.bridge_state_changed |= changed,
                        Err(error) => log::debug!("Tunnel {id} arrival PerCellProcess: {error}"),
                    }
                    outcome.per_cell_ran = true;
                    let on_map = self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.is_object_alive()
                            && !entity.lifecycle.in_limbo()
                            && !entity.is_falling_down()
                    });
                    if on_map {
                        self.tunnel_stop_moving(id, rules, registry, effects);
                        self.tunnel_end_move(id);
                    }
                }
            }
            TunnelState::DugOut => {
                if self
                    .tunnel_runtime(id)
                    .is_some_and(|runtime| runtime.dig_timer_done(frame))
                {
                    self.tunnel_end_move(id);
                }
            }
        }
        // 0x007290F6..0x0072918F, the owner test `0x0050B6F0`.
        self.deselect_over_player_shroud(id, Self::house_is_human_player);
        self.tunnel_damage_smoke(id, rules);
        outcome
    }

    /// `0x007290E7..0x007290F1`: Idle, and the Foot's own Stop
    /// (`FootClass::Stop @ 0x004DF0D0`).
    fn tunnel_end_move(&mut self, id: u64) {
        self.tunnel_set_state(id, TunnelState::Idle);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.navigation.stop_moving();
        }
    }

    /// `0x0070D7A0`, Process's last call: unless the owner is at or below
    /// `ConditionYellow` and no deeper than 10 leptons below the ground, its
    /// damage smoke (Techno `+0x310`) stops spawning (vt+0xF8).
    fn tunnel_damage_smoke(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let strength = self
            .object_type(entity.type_ref(), rules)
            .map_or(0, |object| object.strength);
        let healthy = entity
            .health
            .compare_ratio(strength, rules.general.condition_yellow)
            == crate::util::native_x87::MaskedX87Ordering::Greater;
        if healthy || self.tunnel_height(id) < -10 {
            self.substrate.retire_attached_damage_smoke(id);
        }
    }

    /// ProcessPreDigIn (`0x007291F0`): while the owner's body is not
    /// turning, face Destination (Do_Turn, `0x0072A0E0`), then start the
    /// dig-in timer of `64 / ROT / TunnelSpeed` frames, enter DiggingIn and
    /// play the dig-in effects.
    fn tunnel_pre_dig_in(&mut self, id: u64, rules: &RuleSet) {
        let frame = self.session.binary_frame();
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.body_facing.is_rotating(frame) {
            return;
        }
        let destination = self
            .tunnel_runtime(id)
            .map_or(NULL_COORD, TunnelRuntime::raw_destination);
        // Ares 3.0 TunnelLocomotionClass_Turning (Ares.dll 1005D500, hook
        // 0x0072920C): a null Destination ends the dig (State 7) instead of
        // turning toward the map origin.
        #[cfg(feature = "ares")]
        if destination == NULL_COORD {
            self.tunnel_set_state(id, TunnelState::DugOut);
            return;
        }
        let Some(location) = self.tunnel_location(id) else {
            return;
        };
        let facing = tunnel_movement::facing_toward(location, destination);
        let entity = self.substrate.entities.get_mut(id).expect("same owner");
        if facing != entity.body_facing.current(frame) {
            super::drive_do_turn(entity, facing, frame);
            return;
        }
        let frames =
            tunnel_movement::dig_in_frames(self.tunnel_rot(id, rules), rules.general.tunnel_speed);
        if let Some(runtime) = self.tunnel_runtime_mut(id) {
            runtime.set_state(TunnelState::DiggingIn);
            runtime.start_dig_timer(frame, frames);
        }
        self.tunnel_dig_effects(id, DigEffects::DIG_IN, rules);
    }

    /// DugIn (`0x00728F86..0x00729030`): at Z -256 or below, Mark(UP) and
    /// Digging; otherwise sink by the vertical step, at most to -256.
    fn tunnel_dug_in(
        &mut self,
        id: u64,
        coord_z: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(location) = self.tunnel_location(id) else {
            return;
        };
        if coord_z == TUNNEL_DEPTH_Z || location.z <= TUNNEL_DEPTH_Z {
            self.foot_mark_remove(id, rules, registry, effects);
            self.tunnel_set_state(id, TunnelState::Digging);
            return;
        }
        let step = tunnel_movement::vertical_step(
            self.tunnel_current_speed(id, rules),
            rules.general.tunnel_speed,
        );
        let z = location.z.wrapping_sub(step).max(TUNNEL_DEPTH_Z);
        self.tunnel_set_location(id, DriveCoord { z, ..location }, rules, registry, effects);
    }

    /// ProcessDigging (`0x00729580`): travel toward Destination; within
    /// 0x14 leptons of it, either re-target a nearby passable cell when the
    /// owner may not enter its own cell (class Can_Enter_Cell, vt+0x1AC, with
    /// the locomotor test) inside the playfield, or surface: the dig-out
    /// sound when the height is above -50, PreDigOut, Mark(DOWN). Either way
    /// the owner then stands on Destination at Z -256.
    fn tunnel_digging(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(location) = self.tunnel_location(id) else {
            return;
        };
        let destination = self
            .tunnel_runtime(id)
            .map_or(NULL_COORD, TunnelRuntime::raw_destination);
        if !tunnel_movement::within_arrival_radius(location, destination) {
            let next = tunnel_movement::underground_step(location, destination);
            self.tunnel_set_location(id, next, rules, registry, effects);
            return;
        }
        let arrival = DriveCoord {
            x: destination.x,
            y: destination.y,
            z: TUNNEL_DEPTH_Z,
        };
        let cell = (location.x / 256, location.y / 256);
        let blocked = self
            .mover_can_enter(
                id,
                (cell.0 as i16, cell.1 as i16),
                crate::sim::movement::infantry_entry::InfantryEntryArgs {
                    direction: -1,
                    height: -1,
                    previous_cell: None,
                },
                crate::sim::movement::infantry_entry::EntryQueryMode::CheckLocomotor,
                rules,
                registry,
            )
            .is_ok_and(|code| code != 0);
        let in_playfield = crate::sim::cell_rect::cell_is_in_playfield_leptons(
            (location.x, location.y, location.z),
            self.playfield_bounds(),
            self.resolved_terrain.as_ref(),
        );
        if blocked && in_playfield {
            // 0x0072966F..0x007297AC: Normal over the seed's zone, then
            // Subterannean over any zone.
            let seed = self.tunnel_owner_cell(id);
            let found = self
                .tunnel_nearby_cell(
                    seed,
                    crate::rules::locomotor_type::MovementZone::Normal,
                    true,
                )
                .or_else(|| {
                    self.tunnel_nearby_cell(
                        seed,
                        crate::rules::locomotor_type::MovementZone::Subterranean,
                        false,
                    )
                });
            let Some((x, y)) = found else {
                self.receive_own_health_c4(id, rules, registry, effects);
                return;
            };
            self.tunnel_move_to(
                id,
                DriveCoord {
                    x: i32::from(x) * 256 + 128,
                    y: i32::from(y) * 256 + 128,
                    z: 0,
                },
                rules,
                registry,
                effects,
            );
        } else {
            if self.tunnel_height(id) > TUNNEL_SURFACE_EFFECT_HEIGHT {
                self.tunnel_dig_effects(id, DigEffects::SURFACING, rules);
            }
            self.tunnel_set_state(id, TunnelState::PreDigOut);
            self.foot_mark_put(id, rules, registry, effects);
        }
        self.tunnel_set_location(id, arrival, rules, registry, effects);
    }

    /// ProcessPreDigOut (`0x007298F0`): below the ground, rise by the
    /// vertical step, at most to it, with the dig-out effects as the height
    /// passes -50; at the ground, See, start the dig-out timer of `64 / ROT`
    /// frames, stop (SetSpeedFraction 0.0) and enter DiggingOut.
    fn tunnel_pre_dig_out(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(location) = self.tunnel_location(id) else {
            return;
        };
        let ground = self.tunnel_ground_z(location);
        if location.z < ground {
            let before = self.tunnel_height(id);
            let step = tunnel_movement::vertical_step(
                self.tunnel_current_speed(id, rules),
                rules.general.tunnel_speed,
            );
            let z = location.z.wrapping_add(step).min(ground);
            self.tunnel_set_location(id, DriveCoord { z, ..location }, rules, registry, effects);
            if before < TUNNEL_SURFACE_EFFECT_HEIGHT + 1
                && self.tunnel_height(id) > TUNNEL_SURFACE_EFFECT_HEIGHT
            {
                self.tunnel_dig_effects(id, DigEffects::DIG_OUT, rules);
            }
            return;
        }
        // 0x00729A3D..0x00729A8F.
        let context = UninitContext::new(rules, registry).with_effects(effects);
        self.reveal_uncached_object_sight(id, false, false, None, context);
        let frame = self.session.binary_frame();
        let frames = tunnel_movement::dig_out_frames(self.tunnel_rot(id, rules));
        if let Some(runtime) = self.tunnel_runtime_mut(id) {
            runtime.start_dig_timer(frame, frames);
        }
        self.tunnel_set_speed_fraction(id, false);
        self.tunnel_set_state(id, TunnelState::DiggingOut);
    }

    /// The burrow test at the Cell `cell` names, on the map's canonical cells.
    pub(crate) fn tunnel_cell_allows_burrowing(&self, cell: (i16, i16)) -> bool {
        self.resolved_terrain.as_ref().is_none_or(|terrain| {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            self.cell_allows_burrowing(&cells, cells.lookup(cell))
        })
    }

    /// `UnitClass::AStarAttempt @ 0x00746000`, whether a Tunnel Unit digs to
    /// `to` rather than driving there: an owner below the ground, a cell its
    /// zone does not reach (`MapClass::Can_Reach_Zone @ 0x0056D100`,
    /// MovementZone Normal, no bridges), a trip of 12 or more cells
    /// (Chebyshev), an end outside the playfield, or a route the zone
    /// estimate (`0x0042D170`) prices above 15 digs; the same cell and a
    /// neighbour drive.
    fn tunnel_should_dig(
        &self,
        id: u64,
        from: (i16, i16),
        to: (i16, i16),
        rules: &RuleSet,
    ) -> bool {
        if self.tunnel_height(id) < 0 {
            return true;
        }
        let (Some(terrain), Some(zones), Some(bounds), Some(size)) = (
            self.resolved_terrain.as_ref(),
            self.zone_grid.as_ref(),
            self.playfield_bounds(),
            self.map_size_diamond(),
        ) else {
            return true;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let reachable = zones.can_reach_native(
            &cells,
            crate::sim::pathfinding::zone_map::ZoneQueryCell::Copied(from),
            crate::sim::pathfinding::zone_map::ZoneQueryCell::Copied(to),
            crate::rules::locomotor_type::MovementZone::Normal,
            false,
            false,
            false,
            bounds,
            size,
        );
        if reachable != Some(true) {
            return true;
        }
        if from == to {
            return false;
        }
        let distance = (i32::from(to.0) - i32::from(from.0))
            .abs()
            .max((i32::from(to.1) - i32::from(from.1)).abs());
        if distance == 1 {
            return false;
        }
        if distance >= 12 {
            return true;
        }
        let in_playfield = |cell: (i16, i16)| {
            crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(cell.0), i32::from(cell.1)),
                Some(bounds),
                Some(terrain),
            )
        };
        if !in_playfield(from) || !in_playfield(to) {
            return true;
        }
        let to_bridge = terrain.native_cell_flags(terrain.native_cell_identity(to)) & 0x100 != 0;
        self.estimate_zone_cost_in_query(
            id,
            crate::sim::pathfinding::zone_map::ZoneQueryCell::Copied(from),
            crate::sim::pathfinding::zone_map::ZoneQueryCell::Copied(to),
            false,
            to_bridge,
            rules,
            Some(crate::rules::locomotor_type::MovementZone::Normal),
        )
        .map_or(true, |cost| cost > 15)
    }

    /// The Unit setter's Tunnel arm (`0x007427C0..0x00742C14`), after its
    /// Teleporter arm: a Unit whose active locomotor is an idle Tunnel at
    /// height 0 keeps it when the trip is one to dig ([`Self::tunnel_should_dig`])
    /// between two cells that allow burrowing ([`Self::cell_allows_burrowing`],
    /// CellClass `0x00486FF0`)
    /// to a destination off any bridge. Otherwise a fresh Drive suspends the
    /// Tunnel (`0x00742833..0x00742BE3`) and drives the trip; the FootClass::AI
    /// tail ends that Drive once it may end. Tunnel has no IPiggyback, so the
    /// arm's END of an active piggyback never applies.
    ///
    /// RESIDUAL: the new Drive's Force_New_Slope with the owner Cell's slope
    /// (ILocomotion +0x7C, `0x00742BE3`) is not ported; the Drive keeps its
    /// constructor's slope state. Effect: a Tunnel Unit that starts a drive
    /// on a slope draws flat until its next slope change. Mod-only.
    pub(crate) fn unit_tunnel_arm(
        &mut self,
        id: u64,
        requested: crate::sim::components::NavTargetRef,
        rules: &RuleSet,
    ) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if actor.category() != crate::map::entities::EntityCategory::Unit
            || !self
                .tunnel_runtime(id)
                .is_some_and(|runtime| !runtime.is_moving())
            || self.tunnel_height(id) != 0
        {
            return;
        }
        // The destination's own coordinate (vt+0x48, `0x007428B0`).
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let Some(destination) =
            super::ground_pose::target_get_coords(requested.into(), self, cells.as_ref())
        else {
            return;
        };
        let Some(location) = self.tunnel_location(id) else {
            return;
        };
        let from = ((location.x / 256) as i16, (location.y / 256) as i16);
        let to = ((destination.x / 256) as i16, (destination.y / 256) as i16);
        // vt+0xBC, ShouldBeOnBridge(false) (`0x004DDC40`).
        let on_bridge = self.foot_should_be_on_bridge(id).unwrap_or(false);
        let to_bridge = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            terrain.native_cell_flags(terrain.native_cell_identity(to)) & 0x100 != 0
        });
        let digs = !on_bridge
            && !to_bridge
            && self.tunnel_cell_allows_burrowing(from)
            && self.tunnel_cell_allows_burrowing(to)
            && self.tunnel_should_dig(id, from, to, rules);
        if digs {
            return;
        }
        let frame = self.session.binary_frame();
        if let Some(locomotor) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|actor| actor.locomotor.as_mut())
        {
            locomotor.begin_piggyback(crate::rules::locomotor_type::LocomotorKind::Drive, frame);
        }
    }
}
