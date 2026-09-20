//! Teleport (chrono) locomotor — instant relocation with chrono delay.
//!
//! Implements the Teleport state machine for chrono-style movement:
//! Relocate (instant, one frame) → ChronoDelay (being_warped countdown) → Idle.
//!
//! Self-teleport relocates the unit in a single frame (Phase 0), then the unit
//! sits at the destination 50% translucent for `chrono_delay` frames until fully
//! materialized.
//!
//! Units with `Locomotor=Teleport` always use this. Units with `Teleporter=yes`
//! but a different base locomotor (e.g., Chrono Miner with Drive) get a temporary
//! override via the piggyback mechanism, restoring their base locomotor after arrival.
//!
//! No pathfinding — the unit is relocated instantly. Occupancy is cleared at the
//! old position and marked at the new position during the Relocate phase.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/game_entity, sim/entity_store, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::GeneralRules;
use crate::sim::components::{AnimClassSpawnDescriptor, WorldEffect};
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
use crate::util::fixed_math::isqrt_i64;
use crate::util::lepton::CELL_CENTER_LEPTON;

const TELEPORT_WARP_DRAW_FLAGS: u32 = 0x600;
const TELEPORT_WARP_DELAY: u16 = 0;
const TELEPORT_WARP_LOOP_COUNT: i32 = 1;
const TELEPORT_WARP_Z_ADJUST: i32 = 0;
const TELEPORT_WARP_REVERSE: bool = false;
pub(crate) const FALLBACK_WARP_FRAME_COUNT: u16 = 20;

/// World-effect bridge for verified teleport `AnimClass` constructor rows.
pub struct TeleportVisuals<'a> {
    pub world_effects: &'a mut Vec<WorldEffect>,
    pub warp_out_type: InternedId,
    pub warp_out_total_frames: u16,
    pub warp_out_frame_delay: u16,
}

impl TeleportVisuals<'_> {
    fn spawn_warp_out(&mut self, rx: u16, ry: u16, z: u8) {
        let mut anim_spawn = AnimClassSpawnDescriptor::new(
            self.warp_out_type,
            rx,
            ry,
            CELL_CENTER_LEPTON,
            CELL_CENTER_LEPTON,
            z,
        );
        anim_spawn.delay = TELEPORT_WARP_DELAY;
        anim_spawn.loop_count = TELEPORT_WARP_LOOP_COUNT;
        anim_spawn.draw_flags = TELEPORT_WARP_DRAW_FLAGS;
        anim_spawn.z_adjust = TELEPORT_WARP_Z_ADJUST;
        anim_spawn.reverse = TELEPORT_WARP_REVERSE;

        self.world_effects.push(WorldEffect::from_anim_spawn(
            anim_spawn,
            self.warp_out_total_frames,
            self.warp_out_frame_delay,
            true,
            None,
        ));
    }
}

/// Phase within the teleport state machine.
///
/// Phase 0 relocates instantly in one frame, then the chrono delay timer
/// counts down while the unit is semi-transparent at the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TeleportPhase {
    /// Instant relocation: position updated, occupancy swapped. Executes in
    /// one frame, then transitions to ChronoDelay.
    Relocate,
    /// Post-warp chrono delay: unit sits at destination 50% translucent,
    /// `being_warped_ticks` counts down each frame. When it reaches 0 the
    /// teleport is complete and the base locomotor is restored.
    ChronoDelay,
}

/// Per-frame result returned by the special locomotor Process adapters.
///
/// The native Process vtable slot owns the completion return; keeping that
/// result explicit prevents callers from inferring completion from an absent
/// movement target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialMovementOutcome {
    Continue,
    Complete,
    Abort,
}

/// State for an in-progress teleport.
///
/// Set by `issue_teleport_command()` and cleared when the chrono delay
/// expires. The render system reads `being_warped_ticks` to apply 50%
/// translucency while the unit materializes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TeleportState {
    /// Current phase in the teleport sequence.
    pub phase: TeleportPhase,
    /// Destination cell coordinates.
    pub target_rx: u16,
    pub target_ry: u16,
    /// Chrono delay countdown in native gameplay frames. While > 0 the unit is "being warped"
    /// and the renderer draws it at 50% alpha. Set from the distance-based formula
    /// in the original engine: `delay = distance_leptons / ChronoDistanceFactor`,
    /// clamped to `ChronoMinimumDelay`.
    pub being_warped_ticks: u32,
}

impl TeleportState {
    /// YR TeleportLocomotionClass::Process @ 0x007192f0 exposes separate
    /// warp-out and warp-in producer bytes. Relocation is the departure
    /// producer; the post-relocation delay is the arrival producer.
    pub fn warp_out_active(&self) -> bool {
        self.phase == TeleportPhase::Relocate
    }

    pub fn warp_in_active(&self) -> bool {
        self.phase == TeleportPhase::ChronoDelay && self.being_warped_ticks > 0
    }

    /// The relocation frame is removed from normal targeting before its cell
    /// and occupancy mutation; it becomes targetable again while materializing.
    pub fn is_targetable(&self) -> bool {
        self.phase == TeleportPhase::ChronoDelay
    }
}

/// Compute the chrono warp delay in native gameplay frames from distance.
///
/// When `ChronoTrigger=yes`, delay scales linearly with distance in leptons,
/// divided by `ChronoDistanceFactor` (default 48), clamped to at least
/// `ChronoMinimumDelay` (default 16). Short distances below `ChronoRangeMinimum`
/// are forced to the minimum.
pub fn compute_chrono_delay(rules: &GeneralRules, distance_leptons: i32) -> u32 {
    if !rules.chrono_trigger {
        return rules.chrono_minimum_delay.max(0) as u32;
    }
    let mut delay = if rules.chrono_distance_factor > 0 {
        distance_leptons / rules.chrono_distance_factor
    } else {
        0
    };
    if delay < rules.chrono_minimum_delay {
        delay = rules.chrono_minimum_delay;
    }
    if distance_leptons < rules.chrono_range_minimum {
        delay = rules.chrono_minimum_delay;
    }
    delay.max(0) as u32
}

/// Issue a teleport move command to an entity.
///
/// If the entity's base locomotor is not Teleport but it has `Teleporter=yes`,
/// a temporary override is applied for legacy callers.
///
/// The chrono delay is computed from the Euclidean distance in leptons
/// (see `compute_chrono_delay`). One cell = 256 leptons.
///
/// `is_harvester` skips the chrono lock entirely for harvester units (e.g.,
/// the Chrono Miner): `being_warped_ticks` is forced to 0 and the Relocate
/// phase finishes the teleport in a single frame. Non-harvester teleporters
/// (Chrono Legionnaire and friends) run the full distance-based delay.
///
/// Returns `true` if the teleport was initiated, `false` if the entity
/// is missing required fields.
pub fn issue_teleport_command(
    entities: &mut EntityStore,
    entity_id: u64,
    target: (u16, u16),
    rules: &GeneralRules,
    is_harvester: bool,
    binary_frame: u32,
) -> bool {
    {
        let Some(entity) = entities.get_mut(entity_id) else {
            log::warn!("issue_teleport_command: entity {} not found", entity_id);
            return false;
        };

        // Legacy helper path: non-migrated callers may still put Teleport over a
        // non-Teleport base locomotor as a temporary override. CMIN far return uses
        // `issue_active_teleport_head_to_coord` instead, because Teleport is its
        // primary active locomotor in gamemd.
        let physical = super::foot_coordinate::current_coordinate(entity);
        if let Some(ref mut loco) = entity.locomotor {
            if loco.kind != LocomotorKind::Teleport {
                if !loco.begin_piggyback(
                    crate::rules::locomotor_type::LocomotorKind::Teleport,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    binary_frame,
                ) {
                    return false;
                }
                // BEGIN719E90 replaces an interface, not Object+9C. Publish
                // the outgoing controller's legacy split coordinate before
                // the new raw-copy +18 receiver can observe the owner.
                entity.position.exact_z_leptons = Some(physical.z);
            }
        }
    }

    start_teleport_state(entities, entity_id, target, rules, is_harvester)
}

/// Start a teleport because the active Teleport locomotor received Head_To_Coord.
///
/// This is the gamemd-shaped entry point for CMIN far return after the
/// Set_Destination bridge decides not to activate Drive piggyback.
pub fn issue_active_teleport_head_to_coord(
    entities: &mut EntityStore,
    entity_id: u64,
    target: (u16, u16),
    rules: &GeneralRules,
    is_harvester: bool,
) -> bool {
    {
        let Some(entity) = entities.get(entity_id) else {
            log::warn!(
                "issue_active_teleport_head_to_coord: entity {} not found",
                entity_id
            );
            return false;
        };
        if !entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Teleport)
        {
            return false;
        }
    }
    start_teleport_state(entities, entity_id, target, rules, is_harvester)
}

fn start_teleport_state(
    entities: &mut EntityStore,
    entity_id: u64,
    target: (u16, u16),
    rules: &GeneralRules,
    is_harvester: bool,
) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        log::warn!("start_teleport_state: entity {} not found", entity_id);
        return false;
    };

    // Compute distance in leptons (1 cell = 256 leptons) for chrono delay.
    let dx = (entity.position.rx as i32 - target.0 as i32) * 256;
    let dy = (entity.position.ry as i32 - target.1 as i32) * 256;
    let dist_sq = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
    let distance_leptons = isqrt_i64(dist_sq) as i32;
    let chrono_ticks = if is_harvester {
        0
    } else {
        compute_chrono_delay(rules, distance_leptons)
    };

    // Remove any existing ground movement.
    entity.movement_target = None;

    // Attach the teleport state machine — starts in Relocate (instant).
    let teleport_state = TeleportState {
        phase: TeleportPhase::Relocate,
        target_rx: target.0,
        target_ry: target.1,
        being_warped_ticks: chrono_ticks,
    };
    entity.teleport_state = Some(teleport_state.clone());
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.runtime_payload = LocomotorRuntimePayload::Teleport(Some(teleport_state));
    }
    entity.push_debug_event(
        0,
        DebugEventKind::SpecialMovementStart {
            kind: "Teleport".into(),
        },
    );

    true
}

/// Drop attack locks that name `teleporting_id` before the relocation tick.
///
/// This is the available clean-room analogue of the native incoming-target
/// release. Radio and presentation links remain root-owned integration work.
fn release_incoming_target_locks(entities: &mut EntityStore, teleporting_id: u64) {
    for id in entities.keys_sorted() {
        if id == teleporting_id {
            continue;
        }
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        if entity.attack_target.as_ref().is_some_and(|target| {
            matches!(target.target, crate::sim::combat::TargetKind::Entity(id) if id == teleporting_id)
        }) {
            entity.attack_target = None;
        }
    }
}

/// Advance all in-progress teleport state machines.
///
/// Called once per admitted simulation frame from `advance_tick()`.
/// Relocate executes instantly (one frame), then ChronoDelay counts down
/// `being_warped_ticks` each subsequent frame until the teleport completes.
pub fn tick_teleport_movement(
    entities: &mut EntityStore,
    occupancy: &mut OccupancyGrid,
    live_order: &[u64],
    sim_tick: u64,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    mut visuals: Option<&mut TeleportVisuals<'_>>,
) -> Vec<(u64, SpecialMovementOutcome)> {
    // Collect entity IDs that need cleanup after ticking.
    let mut finished: Vec<u64> = Vec::new();

    let sorted_keys;
    let ordered_ids = if live_order.is_empty() {
        sorted_keys = entities.keys_sorted();
        sorted_keys.as_slice()
    } else {
        live_order
    };

    let mut outcomes = Vec::new();
    for &id in ordered_ids {
        // Teleport removes incoming target locks before its owner is relocated.
        // Do this before borrowing the owner mutably for the Process state.
        let is_relocating = entities
            .get(id)
            .and_then(|entity| entity.teleport_state.as_ref())
            .is_some_and(|state| state.phase == TeleportPhase::Relocate);
        if is_relocating {
            release_incoming_target_locks(entities, id);
        }
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        let Some(ref mut teleport) = entity.teleport_state else {
            continue;
        };

        // Track phase before processing to detect transitions.
        let phase_before = teleport.phase;

        match teleport.phase {
            TeleportPhase::Relocate => {
                // Instant relocation in one frame.
                let old_rx = entity.position.rx;
                let old_ry = entity.position.ry;
                let old_z = entity.position.z;
                if let Some(visuals) = visuals.as_deref_mut() {
                    visuals.spawn_warp_out(old_rx, old_ry, old_z);
                }
                entity.position.rx = teleport.target_rx;
                entity.position.ry = teleport.target_ry;
                entity.position.sub_x = CELL_CENTER_LEPTON;
                entity.position.sub_y = CELL_CENTER_LEPTON;
                // Process719631..7196B2: SetCoords, resolve destination bridge,
                // then Object+1CC/5F5FA0 SetHeight(0). An old split altitude or
                // suspended locomotor must not reappear in the arrival XYZ.
                if let Some(terrain) = terrain {
                    let cell = terrain.native_cell_identity((
                        teleport.target_rx as i16,
                        teleport.target_ry as i16,
                    ));
                    entity.on_bridge = terrain.native_cell_flags(cell) & 0x100 != 0;
                }
                if !super::ground_pose::commit_ground_height(
                    &mut entity.position,
                    entity.on_bridge,
                    terrain,
                    None,
                ) {
                    entity.position.exact_z_leptons = Some(
                        i32::from(entity.position.z as i8)
                            .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
                    );
                }
                // Native Fly4CCC25/Rocket66295C read owner+1C8 height. The
                // legacy Rust controllers cache that height separately; a
                // suspended interface cannot restore the pre-warp value.
                if let Some(loco) = entity.locomotor.as_mut() {
                    if let Some(stashed) = loco.piggyback.as_mut() {
                        stashed.owner_grounded();
                    }
                }
                if let Some(rocket) = entity.rocket_state.as_mut() {
                    rocket.altitude = crate::util::fixed_math::SIM_ZERO;
                }
                if let Some(visuals) = visuals.as_deref_mut() {
                    visuals.spawn_warp_out(
                        entity.position.rx,
                        entity.position.ry,
                        entity.position.z,
                    );
                }
                let layer = entity.locomotor.as_ref().map_or(
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    |l| l.layer,
                );
                occupancy.move_entity(
                    old_rx,
                    old_ry,
                    teleport.target_rx,
                    teleport.target_ry,
                    id,
                    layer,
                    entity.sub_cell,
                    CellListInsertion::from_category(entity.category),
                );
                // Harvester instant-warp: when chrono delay is 0, finish in one
                // frame (cleanup runs at end of this frame) — no post-warp lock.
                if teleport.being_warped_ticks == 0 {
                    finished.push(id);
                    outcomes.push((id, SpecialMovementOutcome::Complete));
                } else {
                    teleport.phase = TeleportPhase::ChronoDelay;
                    outcomes.push((id, SpecialMovementOutcome::Continue));
                }
            }
            TeleportPhase::ChronoDelay => {
                // Count down chrono delay frames. Unit remains 50% translucent until 0.
                if teleport.being_warped_ticks > 0 {
                    teleport.being_warped_ticks -= 1;
                }
                if teleport.being_warped_ticks == 0 {
                    finished.push(id);
                    outcomes.push((id, SpecialMovementOutcome::Complete));
                } else {
                    outcomes.push((id, SpecialMovementOutcome::Continue));
                }
            }
        }

        // Log phase transition if it changed.
        let phase_after = teleport.phase;
        if let Some(locomotor) = entity.locomotor.as_mut() {
            locomotor.runtime_payload = LocomotorRuntimePayload::Teleport(Some(teleport.clone()));
        }
        if phase_after != phase_before {
            let phase_name = format!("{:?}", phase_after);
            // Drop the borrow on teleport before pushing debug event.
            let _ = teleport;
            entity.push_debug_event(
                sim_tick as u32,
                DebugEventKind::SpecialMovementPhase { phase: phase_name },
            );
        }
    }

    // Clean up finished teleports: remove TeleportState and restore base locomotor.
    for id in finished {
        if let Some(entity) = entities.get_mut(id) {
            entity.teleport_state = None;
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.runtime_payload = LocomotorRuntimePayload::Teleport(None);
            }
            entity.push_debug_event(sim_tick as u32, DebugEventKind::SpecialMovementEnd);
        }
        // This finished-warp path uses the existing `Is_Ok_To_End` gate before
        // handing the warped unit back to its suspended locomotor. The warp
        // state was cleared just above, so the ordinary
        // finished warp still ends here — a unit that is simultaneously
        // deploying (or otherwise gated) keeps the stash and unwinds on the
        // per-tick restore instead, as FootClass::AI does.
        let Some(entity) = entities.get(id) else {
            continue;
        };
        let gate = crate::sim::movement::locomotor_end_gate_context(entity);
        let may_end = entity.locomotor.as_ref().is_some_and(|loco| {
            loco.is_overridden()
                && loco.can_restore_primary_from_piggyback(
                    gate.owner_moving,
                    gate.owner_teleporting,
                    gate.owner_deploying,
                )
        });
        if may_end && let Some(entity) = entities.get_mut(id) {
            super::locomotor_owner::restore_admitted_primary(entity);
        }
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::rules::object_type::{ObjectCategory, ObjectType, PipScale};
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::pathfinding::PathGrid;
    use crate::util::fixed_math::SimFixed;

    fn make_drive_obj() -> ObjectType {
        ObjectType {
            id: "CMIN".to_string(),
            category: ObjectCategory::Vehicle,
            name: None,
            ui_name: None,
            cost: 0,
            factory_plant: false,
            cost_bonuses: [crate::util::native_x87::NativeF32Bits::ONE; 5],
            trainable: true,
            explosion_anims: Vec::new(),
            destroy_anims: Vec::new(),
            strength: 100,
            dont_score: false,
            special_threat_value: 0.0,
            threat_posed: 0,
            my_effectiveness_coefficient: None,
            target_effectiveness_coefficient: None,
            target_special_threat_coefficient: None,
            target_strength_coefficient: None,
            target_distance_coefficient: None,
            armor: "none".to_string(),
            speed: 6,
            walk_rate: 1,
            idle_rate: 0,
            weight: SimFixed::lit("2.0"),
            accel_factor: SimFixed::lit("0.03"),
            decel_factor: SimFixed::lit("0.02"),
            accelerates: true,
            passive: false,
            slowdown_distance: 512,
            sight: 5,
            tech_level: -1,
            build_time_multiplier: 1.0,
            build_time_multiplier_x1000: 1000,
            owner: vec![],
            required_houses: vec![],
            forbidden_houses: vec![],
            ai_base_planning_side: -1,
            ai_build_this: false,
            allowed_to_start_in_multiplayer: true,
            prerequisite: vec![],
            prerequisite_override: vec![],
            build_limit: 0,
            requires_stolen_allied_tech: false,
            requires_stolen_soviet_tech: false,
            requires_stolen_third_tech: false,
            primary: None,
            secondary: None,
            elite_primary: None,
            elite_secondary: None,
            fire_up_frame: 0,
            fire_prone_frame: 0,
            secondary_fire_frame: 0,
            secondary_prone_frame: 0,
            image: "CMIN".to_string(),
            power: 0,
            extra_power: 0,
            foundation: "1x1".to_string(),
            pixel_selection_bracket_delta: 0,
            build_cat: None,
            adjacent: 6,
            protect_with_wall: false,
            wants_extra_space: false,
            base_normal: true,
            eligibile_for_ally_building: false,
            crewed: false,
            voice_select: None,
            voice_move: None,
            voice_attack: None,
            voice_harvest: None,
            voice_enter: None,
            voice_capture: None,
            prevent_attack_move: false,
            voice_die: Vec::new(),
            die_sounds: Vec::new(),
            damage_sound: None,
            move_sound: None,
            voice_feedback: None,
            voice_special_attack: None,
            crush_sound: None,
            deploy_sound: None,
            undeploy_sound: None,
            leave_transport_sound: None,
            chrono_in_sound: None,
            chrono_out_sound: None,
            has_turret: false,
            turret_rot: 0,
            turret_anim: None,
            turret_anim_is_voxel: false,
            turret_anim_x: 0,
            turret_anim_y: 0,
            turret_anim_z_adjust: 0,
            guard_range: None,
            air_range_bonus: None,
            opportunity_fire: false,
            can_retaliate: true,
            can_passive_acquire: true,
            distributed_fire: false,
            vhp_scan: crate::rules::object_type::VhpScan::None,
            explodes: false,
            veteran_abilities: Default::default(),
            elite_abilities: Default::default(),
            self_healing: false,
            veteran_explodes: false,
            elite_explodes: false,
            veteran_stronger: false,
            elite_stronger: false,
            veteran_scatter: false,
            elite_scatter: false,
            veteran_cloak: false,
            elite_cloak: false,
            veteran_crusher: false,
            elite_crusher: false,
            death_weapon: None,
            death_weapon_damage_modifier: 1.0,
            super_weapon: None,
            super_weapon2: None,
            spy_sat: false,
            gap_generator: false,
            psychic_detection_radius: 0,
            sensor_array: false,
            sensors: false,
            sensors_sight: 0,
            detect_disguise: false,
            detect_disguise_range: 0,
            cloakable: false,
            cloaking_speed: 1,
            cloak_stop: false,
            cloak_radius_in_cells: 20,
            cloak_generator: false,
            radar: false,
            radar_invisible: false,
            veteran_radar_invisible: false,
            elite_radar_invisible: false,
            radar_visible: false,
            insignificant: false,
            to_protect: false,
            harvester: false,
            spawned: false,
            refinery: false,
            weeder: false,
            bib: false,
            gate: false,
            deploy_time_ticks: 0,
            gate_close_delay_ticks: 0,
            storage: 0,
            free_unit: None,
            dock: vec![],
            queueing_cell: None,
            pads: Vec::new(),
            hidden_occupancy: crate::rules::object_type::BuildingHiddenOccupancyProfile::default(),
            base_reservation_spacing: None,
            unloading_class: None,
            ammo: -1,
            spawns: None,
            spawns_number: 0,
            spawn_regen_rate: 0,
            spawn_reload_rate: 0,
            missile_spawn: false,
            no_spawn_alt: false,
            enslaves: None,
            slaves_number: 0,
            slave_regen_rate: 0,
            slave_reload_rate: 0,
            slaved: false,
            fearless: false,
            fraidycat: false,
            crawls: false,
            veteran_fearless: false,
            elite_fearless: false,
            harvest_rate: 0,
            resource_gatherer: false,
            resource_destination: false,
            ore_purifier: false,
            locomotor: LocomotorKind::Drive,
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
            movement_restricted_to: None,
            considered_aircraft: false,
            zfudge_cliff: 10,
            zfudge_column: 5,
            zfudge_tunnel: 10,
            zfudge_bridge: 7,
            too_big_to_fit_under_bridge: false,
            crashable: false,
            move_to_shroud: true,
            teleporter: true,
            hover_attack: false,
            balloon_hover: false,
            is_simple_deployer: false,
            deploy_to_land: false,
            airport_bound: false,
            fighter: false,
            fly_by: false,
            fly_back: false,
            landable: false,
            jumpjet: false,
            jumpjet_params: crate::rules::jumpjet_params::JumpjetParams::default(),
            deploys_into: None,
            undeploys_into: None,
            deploy_facing: 0x80,
            construction_yard: false,
            build_const_eligible: false,
            base_plan_type_index: -1,
            is_base_defense: false,
            factory: None,
            weapons_factory: false,
            cloning: false,
            exit_coord: None,
            crushable: false,
            deployed_crushable: true,
            crusher: false,
            no_force_shield: false,
            omni_crusher: false,
            omni_crush_resistant: false,
            immune_to_radiation: false,
            damage_self: false,
            immune: false,
            type_immune: false,
            immune_to_psionics: false,
            immune_to_psionic_weapons: false,
            immune_to_poison: false,
            engineer: false,
            deployer: false,
            capturable: false,
            needs_engineer: false,
            capture_eva_event: None,
            repairable: false,
            can_be_occupied: false,
            can_occupy_fire: false,
            show_occupant_pips: false,
            place_anywhere: false,
            to_tile: None,
            bridge_repair_hut: false,
            laser_fence: false,
            firestorm_wall: false,
            passengers: 0,
            size_limit: 0,
            size: 3,
            open_topped: false,
            gunner: false,
            ifv_mode: 0,
            open_transport_weapon: -1,
            deploy_fire: false,
            deploy_fire_weapon: None,
            max_number_occupants: 0,
            occupier: false,
            assaulter: false,
            occupy_weapon: None,
            elite_occupy_weapon: None,
            occupy_pip: 7,
            pip_scale: PipScale::None,
            infantry_absorb: false,
            unit_absorb: false,
            grinding: false,
            bunkerable: true,
            weapon_list: vec![None; crate::rules::object_type::WEAPON_SLOT_COUNT],
            elite_weapon_list: vec![None; crate::rules::object_type::WEAPON_SLOT_COUNT],
            weapon_count: 0,
            naval_targeting: 0,
            land_targeting: 0,
            underwater: false,
            organic: false,
            unnatural: false,
            is_gattling: false,
            turret_count: 0,
            drainable: false,
            produce_cash_startup: 0,
            produce_cash_amount: 0,
            produce_cash_delay: 0,
            overpowerable: false,
            attack_cursor_on_friendlies: false,
            sabotage_cursor: false,
            c4: false,
            can_c4: false,
            eligible_for_delay_kill: false,
            invisible: false,
            invisible_in_game: false,
            unit_repair: false,
            bunker: false,
            unit_reload: false,
            helipad: false,
            number_of_docks: 1,
            toggle_power: false,
            powered: false,
            powered_special: false,
            can_disguise: false,
            disguise_when_still: false,
            wall: false,
            to_overlay: None,
            unsellable: false,
            click_repairable: true,
            selectable: true,
            light_visibility: 0,
            light_intensity: 0.0,
            has_spotlight: false,
            light_red_tint: 1.0,
            light_green_tint: 1.0,
            light_blue_tint: 1.0,
            water_bound: false,
            naval: false,
            number_impassable_rows: -1,
            natural_particle_system: None,
            natural_particle_location: glam::IVec3::ZERO,
            refinery_smoke_particle_system: None,
            damage_particle_systems: Vec::new(),
            max_debris: 0,
            min_debris: 0,
            debris_types: Vec::new(),
            debris_maximums: Vec::new(),
            debris_anims: Vec::new(),
            close_range: false,
            cyborg: false,
            destroy_particle_systems: Vec::new(),
            damage_smoke_offset: glam::IVec3::ZERO,
            dam_smk_off_scrn_rel: false,
            destroy_smoke_offset: glam::IVec3::ZERO,
            refinery_smoke_offsets: [glam::IVec3::ZERO; 4],
            refinery_smoke_frames: 0,
            gap_radius_in_cells: 0,
            super_gap_radius_in_cells: 0,
            stupid_hunt: false,
            vehicle_thief: false,
            undeploy_delay: -1,
        }
    }

    fn make_teleport_harvester_obj() -> ObjectType {
        let mut obj = make_drive_obj();
        obj.locomotor = LocomotorKind::Teleport;
        obj.harvester = true;
        obj.teleporter = true;
        obj.turret_rot = 5;
        obj
    }

    fn default_rules() -> GeneralRules {
        GeneralRules::default()
    }

    #[test]
    fn test_teleport_issues_and_completes() {
        let mut entities = EntityStore::new();
        let mut e = GameEntity::test_default(1, "CLEG", "Americans", 5, 5);
        e.position.z = 0;
        entities.insert(e);
        let rules = default_rules();

        assert!(issue_teleport_command(
            &mut entities,
            1,
            (20, 20),
            &rules,
            false,
            0
        ));
        let entity = entities.get(1).expect("should exist");
        let ts = entity
            .teleport_state
            .as_ref()
            .expect("should have TeleportState");
        assert_eq!(ts.phase, TeleportPhase::Relocate);
        assert!(
            ts.being_warped_ticks >= 16,
            "should have at least minimum delay"
        );

        // One admitted frame relocates instantly.
        tick_teleport_movement(&mut entities, &mut OccupancyGrid::new(), &[], 0, None, None);

        let entity = entities.get(1).expect("should exist");
        assert_eq!(entity.position.rx, 20, "Should have relocated to target");
        assert_eq!(entity.position.ry, 20);
        let ts = entity.teleport_state.as_ref().expect("still warping");
        assert_eq!(
            ts.phase,
            TeleportPhase::ChronoDelay,
            "should be in chrono delay"
        );

        // Advance through the ChronoDelay countdown.
        let delay = ts.being_warped_ticks;
        for _ in 0..delay + 5 {
            tick_teleport_movement(&mut entities, &mut OccupancyGrid::new(), &[], 0, None, None);
        }

        // TeleportState should be removed after completion.
        let entity = entities.get(1).expect("should exist");
        assert!(
            entity.teleport_state.is_none(),
            "TeleportState should be removed after completion"
        );
    }

    #[test]
    fn relocate_spawns_departure_and_arrival_warpout_rows() {
        let mut entities = EntityStore::new();
        let mut e = GameEntity::test_default(1, "CLEG", "Americans", 5, 5);
        e.position.z = 2;
        entities.insert(e);
        let rules = default_rules();

        assert!(issue_teleport_command(
            &mut entities,
            1,
            (8, 9),
            &rules,
            true,
            0
        ));

        let warp_out_type = crate::sim::intern::test_intern("WARPOUT");
        let mut world_effects = Vec::new();
        {
            let mut visuals = TeleportVisuals {
                world_effects: &mut world_effects,
                warp_out_type,
                warp_out_total_frames: 13,
                warp_out_frame_delay: 1,
            };
            tick_teleport_movement(
                &mut entities,
                &mut OccupancyGrid::new(),
                &[],
                0,
                None,
                Some(&mut visuals),
            );
        }

        assert_eq!(world_effects.len(), 2);
        for (effect, (rx, ry)) in world_effects.iter().zip([(5, 5), (8, 9)]) {
            assert_eq!(effect.shp_name, warp_out_type);
            assert_eq!((effect.rx, effect.ry, effect.z), (rx, ry, 2));
            assert_eq!(effect.total_frames, 13);
            assert_eq!(effect.frame_delay, 1);
            let row = effect.anim_spawn.as_ref().expect("AnimClass row");
            assert_eq!(row.type_name, warp_out_type);
            assert_eq!((row.rx, row.ry, row.z), (rx, ry, 2));
            assert_eq!(row.delay, TELEPORT_WARP_DELAY);
            assert_eq!(row.loop_count, TELEPORT_WARP_LOOP_COUNT);
            assert_eq!(row.draw_flags, TELEPORT_WARP_DRAW_FLAGS);
            assert_eq!(row.z_adjust, TELEPORT_WARP_Z_ADJUST);
            assert_eq!(row.reverse, TELEPORT_WARP_REVERSE);
        }
    }

    #[test]
    fn chrono_delay_tick_does_not_spawn_extra_warpout_rows() {
        let mut entities = EntityStore::new();
        let e = GameEntity::test_default(1, "CLEG", "Americans", 5, 5);
        entities.insert(e);
        let rules = default_rules();

        assert!(issue_teleport_command(
            &mut entities,
            1,
            (20, 20),
            &rules,
            false,
            0
        ));

        let warp_out_type = crate::sim::intern::test_intern("WARPOUT");
        let mut world_effects = Vec::new();
        {
            let mut visuals = TeleportVisuals {
                world_effects: &mut world_effects,
                warp_out_type,
                warp_out_total_frames: FALLBACK_WARP_FRAME_COUNT,
                warp_out_frame_delay: 2,
            };
            tick_teleport_movement(
                &mut entities,
                &mut OccupancyGrid::new(),
                &[],
                0,
                None,
                Some(&mut visuals),
            );
            tick_teleport_movement(
                &mut entities,
                &mut OccupancyGrid::new(),
                &[],
                1,
                None,
                Some(&mut visuals),
            );
        }

        assert_eq!(
            world_effects.len(),
            2,
            "only Relocate emits the verified departure and arrival rows"
        );
    }

    #[test]
    fn teleport_movement_uses_live_object_order_not_stable_id_scan() {
        fn teleporter(id: u64, rx: u16, ry: u16, target_rx: u16) -> GameEntity {
            let mut entity = GameEntity::test_default(id, "CLEG", "Americans", rx, ry);
            entity.teleport_state = Some(TeleportState {
                phase: TeleportPhase::Relocate,
                target_rx,
                target_ry: 20,
                being_warped_ticks: 0,
            });
            entity
        }

        let mut live_entities = EntityStore::new();
        live_entities.insert(teleporter(1, 5, 5, 21));
        live_entities.insert(teleporter(2, 6, 5, 22));

        tick_teleport_movement(
            &mut live_entities,
            &mut OccupancyGrid::new(),
            &[2],
            0,
            None,
            None,
        );

        let first = live_entities.get(1).expect("id 1");
        assert_eq!(
            (first.position.rx, first.position.ry),
            (5, 5),
            "non-live-order IDs are not swept by stable-id fallback"
        );
        assert!(first.teleport_state.is_some());
        let second = live_entities.get(2).expect("id 2");
        assert_eq!((second.position.rx, second.position.ry), (22, 20));
        assert!(second.teleport_state.is_none());

        let mut fallback_entities = EntityStore::new();
        fallback_entities.insert(teleporter(1, 5, 5, 21));
        fallback_entities.insert(teleporter(2, 6, 5, 22));

        tick_teleport_movement(
            &mut fallback_entities,
            &mut OccupancyGrid::new(),
            &[],
            0,
            None,
            None,
        );

        assert_eq!(
            fallback_entities.get(1).map(|entity| (
                entity.position.rx,
                entity.position.ry,
                entity.teleport_state.is_none()
            )),
            Some((21, 20, true))
        );
        assert_eq!(
            fallback_entities.get(2).map(|entity| (
                entity.position.rx,
                entity.position.ry,
                entity.teleport_state.is_none()
            )),
            Some((22, 20, true))
        );
    }

    #[test]
    fn test_teleport_with_piggyback_restores_drive() {
        let mut entities = EntityStore::new();
        let obj = make_drive_obj();
        let loco = LocomotorState::from_object_type(&obj, 1500, 0);
        let mut e = GameEntity::test_default(1, "CMIN", "Americans", 5, 5);
        e.locomotor = Some(loco);
        entities.insert(e);
        let rules = default_rules();

        // Pass is_harvester=false so the test still exercises the full chrono-delay path.
        // (CMIN type fixture used here has harvester=false; the harvester instant-warp
        // path is covered by the dedicated tests below.)
        assert!(issue_teleport_command(
            &mut entities,
            1,
            (20, 20),
            &rules,
            false,
            0
        ));
        // Should have overridden to Teleport.
        let entity = entities.get(1).expect("should exist");
        let loco = entity.locomotor.as_ref().expect("has loco");
        assert_eq!(loco.kind, LocomotorKind::Teleport);
        assert!(loco.is_overridden());

        // Complete the whole sequence: one Relocate frame plus the chrono delay.
        for _ in 0..200 {
            tick_teleport_movement(&mut entities, &mut OccupancyGrid::new(), &[], 0, None, None);
        }

        // Should have restored to Drive.
        let entity = entities.get(1).expect("should exist");
        let loco = entity.locomotor.as_ref().expect("has loco");
        assert_eq!(loco.kind, LocomotorKind::Drive);
        assert!(!loco.is_overridden());
        assert_eq!(loco.layer, MovementLayer::Ground);
    }

    #[test]
    fn teleporter_empty_destination_starts_teleport_without_drive_override() {
        let mut entities = EntityStore::new();
        let obj = make_teleport_harvester_obj();
        let loco = LocomotorState::from_object_type(&obj, 1500, 0);
        let mut e = GameEntity::test_default(1, "CMIN", "Americans", 5, 5);
        e.locomotor = Some(loco);
        entities.insert(e);
        let rules = default_rules();

        assert!(crate::sim::movement::set_destination_for_teleporter_entity(
            &mut entities,
            None,
            1,
            (20, 20),
            SimFixed::from_num(6),
            false,
            None,
            None,
            None,
            None,
            None,
            false,
            &rules,
            true,
            true,
            false,
            None,
            0,
        ));

        let entity = entities.get(1).expect("entity");
        assert!(entity.teleport_state.is_some());
        let loco = entity.locomotor.as_ref().expect("loco");
        assert_eq!(loco.active_kind(), LocomotorKind::Teleport);
        assert_eq!(loco.effective_kind(), LocomotorKind::Teleport);
        assert!(loco.piggyback.is_none());
        assert!(!loco.is_overridden());
    }

    #[test]
    fn teleporter_building_destination_activates_drive_piggyback() {
        let mut entities = EntityStore::new();
        let obj = make_teleport_harvester_obj();
        let loco = LocomotorState::from_object_type(&obj, 1500, 0);
        let mut e = GameEntity::test_default(1, "CMIN", "Americans", 5, 5);
        e.locomotor = Some(loco);
        entities.insert(e);
        let rules = default_rules();
        let grid = PathGrid::test_all_passable(32, 32);

        assert!(crate::sim::movement::set_destination_for_teleporter_entity(
            &mut entities,
            Some(&grid),
            1,
            (10, 10),
            SimFixed::from_num(6),
            false,
            None,
            None,
            None,
            None,
            None,
            false,
            &rules,
            true,
            true,
            true,
            None,
            0,
        ));

        let entity = entities.get(1).expect("entity");
        assert!(entity.teleport_state.is_none());
        assert!(entity.movement_target.is_some());
        let loco = entity.locomotor.as_ref().expect("loco");
        assert_eq!(loco.active_kind(), LocomotorKind::Drive);
        assert_eq!(loco.effective_kind(), LocomotorKind::Teleport);
        assert!(loco.piggyback.is_some());

        // gamemd's `Drive::Is_Ok_To_End` asks the ACTIVE locomotor's own
        // `Is_Moving` (ILocomotion slot 4), and that predicate reads the Drive
        // locomotor's destination and head-to coords — not the owner's path
        // queue. Dropping the path alone therefore does NOT unwind the stash.
        entities.get_mut(1).expect("entity").movement_target = None;
        assert_eq!(
            crate::sim::movement::tick_locomotor_piggyback_restore(&mut entities),
            0,
            "the Drive locomotor still holds a destination, so it still reports Is_Moving"
        );

        // Arrival clears the Drive destination and head-to; now the gate opens.
        {
            let drive = entities
                .get_mut(1)
                .and_then(|entity| entity.drive_locomotion.as_mut())
                .expect("drive state");
            drive.destination = None;
            drive.head_to = None;
        }
        assert_eq!(
            crate::sim::movement::tick_locomotor_piggyback_restore(&mut entities),
            1
        );
        let loco = entities
            .get(1)
            .and_then(|entity| entity.locomotor.as_ref())
            .expect("loco");
        assert_eq!(loco.active_kind(), LocomotorKind::Teleport);
        assert!(loco.is_primary_active());
    }

    #[test]
    fn test_chrono_delay_formula() {
        let mut rules = default_rules();
        // Default: factor=48, minimum=16, trigger=true, range_minimum=0

        // Short distance: 256 leptons (1 cell) → 256/48 = 5, clamped to 16
        assert_eq!(compute_chrono_delay(&rules, 256), 16);

        // Medium distance: 5120 leptons (20 cells) → 5120/48 = 106
        assert_eq!(compute_chrono_delay(&rules, 5120), 106);

        // Very short distance below range minimum
        rules.chrono_range_minimum = 512;
        assert_eq!(compute_chrono_delay(&rules, 200), 16); // forced to minimum

        // ChronoTrigger=false → always minimum
        rules.chrono_trigger = false;
        assert_eq!(compute_chrono_delay(&rules, 5120), 16);
    }

    /// Harvester units skip the chrono lock entirely — when is_harvester=true
    /// the lock duration is 0 regardless of distance.
    #[test]
    fn test_harvester_skips_chrono_delay() {
        let mut entities = EntityStore::new();
        let e = GameEntity::test_default(1, "CMIN", "Americans", 5, 5);
        entities.insert(e);
        let rules = default_rules();

        // Long distance (~80 cells diagonal) — non-harvester computes ~604 frames delay.
        assert!(issue_teleport_command(
            &mut entities,
            1,
            (90, 90),
            &rules,
            true,
            0
        ));
        let ts = entities
            .get(1)
            .and_then(|e| e.teleport_state.as_ref())
            .expect("should have TeleportState");
        assert_eq!(
            ts.being_warped_ticks, 0,
            "harvester instant-warp must zero the chrono lock"
        );
    }

    /// With is_harvester=true, the Relocate phase finishes the teleport in a single
    /// tick (skipping ChronoDelay).
    #[test]
    fn test_harvester_relocate_cleans_up_in_one_tick() {
        let mut entities = EntityStore::new();
        let obj = make_drive_obj();
        let loco = LocomotorState::from_object_type(&obj, 1500, 0);
        let mut e = GameEntity::test_default(1, "CMIN", "Americans", 5, 5);
        e.locomotor = Some(loco);
        entities.insert(e);
        let rules = default_rules();

        assert!(issue_teleport_command(
            &mut entities,
            1,
            (20, 20),
            &rules,
            true,
            0
        ));
        // Override applied at issue time.
        let entity = entities.get(1).expect("should exist");
        assert!(entity.locomotor.as_ref().expect("loco").is_overridden());

        // Single frame: position snaps, then cleanup runs because being_warped_ticks==0.
        tick_teleport_movement(&mut entities, &mut OccupancyGrid::new(), &[], 0, None, None);

        let entity = entities.get(1).expect("should exist");
        assert_eq!(entity.position.rx, 20);
        assert_eq!(entity.position.ry, 20);
        assert!(
            entity.teleport_state.is_none(),
            "harvester teleport should clean up in one frame"
        );
        let loco = entity.locomotor.as_ref().expect("has loco");
        assert_eq!(loco.kind, LocomotorKind::Drive, "base locomotor restored");
        assert!(!loco.is_overridden(), "override ended");
    }

    /// Regression: non-harvester (Chrono Legionnaire path) still goes through the
    /// full Relocate → ChronoDelay countdown.
    #[test]
    fn test_non_harvester_uses_full_chrono_delay() {
        let mut entities = EntityStore::new();
        let e = GameEntity::test_default(1, "CLEG", "Americans", 5, 5);
        entities.insert(e);
        let rules = default_rules();

        assert!(issue_teleport_command(
            &mut entities,
            1,
            (20, 20),
            &rules,
            false,
            0
        ));
        let initial_ticks = entities
            .get(1)
            .and_then(|e| e.teleport_state.as_ref())
            .map(|t| t.being_warped_ticks)
            .expect("teleport_state");
        assert!(
            initial_ticks > 0,
            "non-harvester must keep the distance-based chrono lock"
        );

        // Frame 1: Relocate snaps position and transitions to ChronoDelay (NOT cleanup).
        tick_teleport_movement(&mut entities, &mut OccupancyGrid::new(), &[], 0, None, None);
        let ts = entities
            .get(1)
            .and_then(|e| e.teleport_state.as_ref())
            .expect("still warping after Relocate");
        assert_eq!(ts.phase, TeleportPhase::ChronoDelay);
        assert_eq!(ts.being_warped_ticks, initial_ticks);
    }

    #[test]
    fn relocation_releases_incoming_attack_locks_before_moving_the_target() {
        let mut entities = EntityStore::new();
        let target = GameEntity::test_default(1, "CLEG", "Americans", 5, 5);
        let mut attacker = GameEntity::test_default(2, "MTNK", "Russians", 4, 5);
        attacker.attack_target = Some(crate::sim::combat::AttackTarget::new(1));
        entities.insert(target);
        entities.insert(attacker);

        assert!(issue_teleport_command(
            &mut entities,
            1,
            (20, 20),
            &default_rules(),
            false,
            0,
        ));
        let outcomes =
            tick_teleport_movement(&mut entities, &mut OccupancyGrid::new(), &[], 0, None, None);

        assert_eq!(outcomes, vec![(1, SpecialMovementOutcome::Continue)]);
        assert!(entities.get(2).expect("attacker").attack_target.is_none());
    }

    #[test]
    fn teleport_exposes_distinct_warp_and_targetability_producers() {
        let relocate = TeleportState {
            phase: TeleportPhase::Relocate,
            target_rx: 1,
            target_ry: 1,
            being_warped_ticks: 10,
        };
        assert!(relocate.warp_out_active());
        assert!(!relocate.warp_in_active());
        assert!(!relocate.is_targetable());

        let arrival = TeleportState {
            phase: TeleportPhase::ChronoDelay,
            target_rx: 1,
            target_ry: 1,
            being_warped_ticks: 10,
        };
        assert!(!arrival.warp_out_active());
        assert!(arrival.warp_in_active());
        assert!(arrival.is_targetable());
    }
}
