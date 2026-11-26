//! Per-frame simulation observations: actor, house, effect, laser, beam, bolt,
//! crate and terrain-render snapshots a recorded frame carries.

use super::*;

/// The observed House's Supers in interned-id order: the id a profile's
/// `LaunchSuperWeapon` names and the state the sidebar reads.
pub(super) fn super_weapon_rows(sim: &crate::sim::world::Simulation, owner: &str) -> Value {
    let frame = sim.session().binary_frame() as i32;
    sim.interner()
        .get(owner)
        .and_then(|id| sim.super_weapons().get(&id))
        .into_iter()
        .flatten()
        .map(|(&type_id, inst)| {
            let (fade_countdown, fade_coords) = inst.fade();
            json!({"type": sim.interner().resolve(type_id), "interned_id": type_id.index(),
                "granted": inst.is_active(), "ready": inst.is_ready(), "on_hold": inst.is_suspended(),
                "charge_start": inst.charge_start_tick(), "charge_duration": inst.charge_duration(),
                "remaining": inst.charge_remaining(frame), "fade_countdown": fade_countdown,
                "fade_coords": fade_coords})
        })
        .collect()
}

/// The observed House's allies in House array order, those its own mask
/// names (`HouseClass+0x5788`), and the enemy its anger chose (`+0x5600`).
pub(super) fn diplomacy_row(sim: &crate::sim::world::Simulation, owner: &str) -> Value {
    let allies: Vec<&str> = sim
        .houses()
        .order()
        .iter()
        .map(|&house| sim.interner().resolve(house))
        .filter(|house| {
            !house.eq_ignore_ascii_case(owner)
                && crate::map::houses::is_allied_with(sim.house_alliances().as_map(), owner, house)
        })
        .collect();
    let enemy = crate::sim::house_state::house_state_for_owner(sim.houses(), owner, sim.interner())
        .and_then(|house| house.enemy_house())
        .map(|enemy| sim.interner().resolve(enemy));
    json!({"allies": allies, "enemy": enemy})
}

pub(super) fn disguise_inputs(
    frontend: &FrontendState,
    match_state: &MatchState,
    entity: &crate::sim::game_entity::GameEntity,
) -> Result<Value> {
    use crate::app::presentation::instances;
    let Some(disguise) = entity.seldom.disguise.as_ref() else {
        return Ok(Value::Null);
    };
    let runtime = match_state
        .sim_runtime
        .runtime()
        .context("disguise simulation absent")?;
    let sim = &runtime.simulation;
    let draw = if entity.category() == crate::map::entities::EntityCategory::Unit {
        // The record the draw reads (`presentation::frame`).
        let frame = match_state
            .sim_runtime
            .frame()
            .context("presentation frame absent")?;
        let entity = frame
            .object(entity.stable_id())
            .context("disguised object has no record")?;
        let mut observer = entity.seen.observer;
        let chosen = instances::drawn_model_id(
            entity,
            sim.interner(),
            crate::app::state::active_rules(frontend, match_state),
            sim.session().binary_frame(),
            observer,
        );
        let voxel = instances::drawn_type_uses_voxel(
            entity,
            &chosen,
            sim.interner(),
            crate::app::state::active_rules(frontend, match_state),
        );
        observer.drawn_voxel = Some(voxel);
        let frame = (!voxel)
            .then(|| {
                instances::resolve_object_shp_frame(frontend, match_state, &chosen, entity, None)
            })
            .flatten();
        let terrain_pair = frame.is_some_and(|frame| {
            match_state
                .match_presentation
                .overlay_atlas
                .as_ref()
                .is_some_and(|atlas| {
                    let (body, shadow) = atlas.terrain_pieces(&chosen, i32::from(frame));
                    body.is_some() || shadow.is_some()
                })
        });
        let decision = crate::render::draw_state::DrawState::for_entity(
            entity,
            sim.session().binary_frame(),
            0,
            observer,
        );
        json!({"type_id": chosen, "voxel": voxel, "shp_frame": frame,
            "terrain_pair_available": terrain_pair, "draw_state_visible": decision.visible,
            "native_selector_bits": decision.state.native_selector_bits()})
    } else {
        Value::Null
    };
    Ok(
        json!({"active": disguise.is_disguised(), "creation_frame": disguise.creation_frame(),
        "type_id": disguise.type_id().map(|id| sim.interner().resolve(id)),
        "house": disguise.house().map(|id| sim.interner().resolve(id)),
        "reveal_start": disguise.reveal_timer().start_frame(),
        "reveal_duration": disguise.reveal_timer().duration(), "draw": draw}),
    )
}

pub(super) fn retask_observation(entity: &crate::sim::game_entity::GameEntity) -> Value {
    json!({
        "suspended_target": entity.seldom.suspended_attack_target.target(),
        "suspended_nav": entity.navigation.suspended_nav_com(),
    })
}

pub(super) fn prism_observation(entity: &crate::sim::game_entity::GameEntity, frame: i32) -> Value {
    use crate::sim::world::DelayedFire;
    if entity.category() != crate::map::entities::EntityCategory::Structure {
        return Value::Null;
    }
    let pending = entity
        .building
        .pending_building_fire
        .pending()
        .map(|pending| match pending.fire() {
            DelayedFire::Weapon(slot) => json!({"mode": 1, "payload": {"weapon": slot},
            "remaining": pending.remaining_ticks()}),
            DelayedFire::SupportBeam { to } => json!({"mode": 2,
            "payload": {"to": [to.x, to.y, to.z]}, "remaining": pending.remaining_ticks()}),
        });
    json!({"support_count": entity.building.prism_support_count, "pending": pending,
        "rearm": {"start": entity.seldom.rearm_timer.start_frame(),
                  "duration": entity.seldom.rearm_timer.duration(),
                  "remaining": entity.seldom.rearm_timer.remaining(frame)}})
}

pub(super) fn fire_observation(entity: &crate::sim::game_entity::GameEntity, frame: i32) -> Value {
    // Existing immutable counters and timer queries only. In particular this
    // neither completes a firing sequence nor draws GetROF's Scenario RNG.
    json!({"last_fire_frame": entity.last_fire_frame.frame(),
        "body_counter": entity.body_frame_counter,
        "stage_value": entity.native_stage().value(),
        "rearm": timer_observation(entity.seldom.rearm_timer.timer(), frame)})
}

fn timer_observation(timer: crate::sim::timer::CdTimer, frame: i32) -> Value {
    json!({"start_frame": timer.start_frame(), "duration": timer.duration(),
        "remaining": timer.remaining(frame)})
}

pub(super) fn cloak_runtime_observation(
    cloak: &crate::sim::cloak_disguise::CloakRuntime,
    in_playfield: bool,
    frame: i32,
) -> Value {
    let step = cloak.step_timer();
    json!({"in_playfield": in_playfield, "step_delta_i32": cloak.step_delta(),
        "step_speed_i32": step.speed(), "step_timer": timer_observation(step.timer(), frame),
        "recloak_delay": timer_observation(cloak.recloak_delay(), frame)})
}

pub(super) fn temporal_observation(entity: &crate::sim::game_entity::GameEntity) -> Value {
    // Independent immutable owner queries: diagnostics neither transfer a
    // link nor infer a carrier relation from a cargo entry (or vice versa).
    let cargo = entity.passenger_role().cargo().map(|cargo| {
        let passengers: Vec<_> = cargo
            .passengers()
            .iter()
            .take(MAX_OBSERVATION_SAMPLES + 1)
            .copied()
            .collect();
        json!({"count": cargo.count(), "passengers": passengers})
    });
    let inside = entity.passenger_role().inside_transport().map(
        |(transport, open_topped)| json!({"transport": transport, "open_topped": open_topped}),
    );
    json!({"has_link": entity.temporal.has_link(),
        "warp_target": entity.temporal.warp_target(),
        "chain_head": entity.temporal.chain_head(),
        "inside_transport": inside, "cargo": cargo})
}

pub(super) fn temporal_sample_count(snapshot: &Value) -> usize {
    usize::from(!snapshot.is_null())
        + snapshot["cargo"]["passengers"]
            .as_array()
            .map_or(0, Vec::len)
}

pub(super) fn spawn_observation(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    rules: &crate::rules::ruleset::RuleSet,
) -> Result<Value> {
    let Some(manager) = entity.spawn_manager.as_ref() else {
        return Ok(Value::Null);
    };
    let object = rules
        .object(sim.interner().resolve(entity.type_ref()))
        .context("spawn observation owner type absent")?;
    // Independent immutable owner reads. In particular current type capacity
    // is not the retained slot count, and diagnostics never run manager AI.
    let slots: Vec<_> = manager
        .slots()
        .iter()
        .map(|slot| {
            json!({"child": slot.spawn(), "state": slot.state(),
                "timer": slot.timer(), "is_missile": slot.is_missile_spawn()})
        })
        .collect();
    Ok(json!({"type_spawns_number": object.spawns_number,
        "spawn_type": sim.interner().resolve(manager.spawn_type()),
        "count_docked": manager.count_docked_spawns(),
        "count_alive": manager.count_alive_spawns(), "mode": manager.mode(),
        "regen_rate": manager.regen_rate(), "reload_rate": manager.reload_rate(),
        "current_target": manager.current_target(), "queued_target": manager.queued_target(),
        "update_timer": manager.update_timer(), "spawn_timer": manager.spawn_timer(),
        "slots": slots}))
}

pub(super) fn spawn_sample_count(snapshot: &Value) -> usize {
    snapshot["slots"]
        .as_array()
        .map_or(0, |slots| 1 + slots.len())
}

pub(super) fn bomb_sample_count(snapshot: &crate::sim::bomb::BombObservation) -> usize {
    // Count the owner snapshot and both independent collections, including
    // index entries whose carrier record may be missing in a broken owner.
    1 + snapshot.carriers.len() + snapshot.records.len()
}

#[derive(Debug, Serialize)]
pub(super) struct MapLaserObservation {
    detail: crate::app::presentation::detail::DetailObservation,
    live: Vec<crate::app::presentation::lasers::LaserObservation>,
    disk_shots: Vec<Value>,
}

impl MapLaserObservation {
    pub(super) fn capture(match_state: &MatchState) -> Result<Self> {
        let runtime = match_state
            .sim_runtime
            .runtime()
            .context("laser observation simulation absent")?;
        let sim = &runtime.simulation;
        let presentation = &match_state.match_presentation;
        let snapshot = Self {
            detail: presentation.detail.borrow().observation(),
            live: presentation
                .lasers
                .observations()
                .take(MAX_OBSERVATION_SAMPLES + 1)
                .collect(),
            // Copy the authoritative delayed-shot owner in birth order. This
            // reads no coordinates and advances no timer, ID cursor or RNG.
            disk_shots: sim
                .disk_laser_observations()
                .take(MAX_OBSERVATION_SAMPLES + 1)
                .map(|shot| {
                    let weapon = shot.weapon.map(|id| sim.interner().resolve(id));
                    json!({"id": shot.id, "source": shot.source, "target": shot.target,
                        "weapon": weapon, "countdown": shot.countdown,
                        "start": shot.start, "step": shot.step, "damage": shot.damage})
                })
                .collect(),
        };
        ensure!(
            snapshot.sample_count() <= MAX_OBSERVATION_SAMPLES,
            "laser observation exceeds sample budget"
        );
        Ok(snapshot)
    }

    pub(super) fn sample_count(&self) -> usize {
        1 + self.live.len() + self.disk_shots.len()
    }
}

/// Copied owner output, including expired beams' last actual geometry. No
/// diagnostic projection or extra Tactical-composite visit participates.
#[derive(Debug, Serialize)]
pub(super) struct MapRadiationBeamObservation {
    live: Vec<crate::app::presentation::radiation_beams::BeamObservation>,
    composites: Vec<Value>,
    segments: Vec<Value>,
}

impl MapRadiationBeamObservation {
    pub(super) fn capture(
        beams: &crate::app::presentation::radiation_beams::RadiationBeams,
    ) -> Result<Self> {
        use crate::render::surface_line::{SurfaceDraw, SurfaceLineBlend};

        let live = beams.observation();
        ensure!(
            1 + live.len() + beams.composites().len() + beams.draws().len()
                <= MAX_OBSERVATION_SAMPLES,
            "radiation beam observation exceeds sample budget"
        );
        let composites = beams
            .composites()
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let segments = beams
            .draws()
            .iter()
            .map(|draw| {
                let SurfaceDraw::Line(line) = draw else {
                    bail!("radiation beam observer expected an actual surface line")
                };
                let SurfaceLineBlend::Replace(word) = line.blend else {
                    bail!("radiation beam observer expected packed replacement color")
                };
                Ok(json!({"from": line.from, "to": line.to,
                    "z_adjust": line.z_adjust, "rgb565": word}))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            live,
            composites,
            segments,
        })
    }

    pub(super) fn sample_count(&self) -> usize {
        1 + self.live.len() + self.composites.len() + self.segments.len()
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapRadiationBeamDrawObservation {
    pub(super) ordinal: usize,
    pub(super) completed_steps: u64,
    pub(super) simulation_tick: u64,
    pub(super) binary_frame: u32,
    pub(super) beams: MapRadiationBeamObservation,
}

/// Read-only boundaries of the actual effect owners. Full logical RNG words
/// permit native continuation comparisons; observations never consume a draw.
#[derive(Debug, Serialize)]
pub(super) struct MapElectricBoltObservation {
    detail: crate::app::presentation::detail::DetailObservation,
    game_detail_level: u32,
    live: Vec<crate::app::presentation::electric_bolts::ElectricBoltObservation>,
    lights: Vec<crate::app::presentation::combat_lights::CombatLightObservation>,
    particle_systems: Vec<Value>,
    native_id_cursor: u32,
    rng: Value,
}

impl MapElectricBoltObservation {
    pub(super) fn capture(match_state: &MatchState) -> Result<Self> {
        let runtime = match_state
            .sim_runtime
            .runtime()
            .context("electric observation simulation absent")?;
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let presentation = &match_state.match_presentation;
        let live: Vec<_> = presentation
            .electric_bolts
            .observations()
            .take(MAX_OBSERVATION_SAMPLES + 1)
            .collect();
        let lights: Vec<_> = presentation
            .combat_lights
            .observations()
            .take(MAX_OBSERVATION_SAMPLES + 1)
            .collect();
        let mut count = 4 + live.len() + lights.len();
        ensure!(
            count <= MAX_OBSERVATION_SAMPLES,
            "electric observation exceeds sample budget"
        );
        let mut particle_systems = Vec::new();
        for (&id, system) in sim.particle_systems().iter() {
            count = count
                .checked_add(1 + system.particles().len())
                .context("particle observation count overflow")?;
            ensure!(
                count <= MAX_OBSERVATION_SAMPLES,
                "particle observation exceeds sample budget"
            );
            let particles: Vec<_> = system.particles().iter().map(|particle| json!({
                "stable_id": particle.stable_id(),
                "native_id": particle.native_unique_id(), "type_id": rules.particle_type(particle.type_id()).name,
                "display_layer": sim.display_layers().layer_of(particle.stable_id()).map(|layer| layer.index()),
                "coord": particle.coords().to_array(), "lifetime": particle.lifetime_remaining(),
                "damage_counter": particle.damage_counter(), "marked_for_deletion": particle.marked_for_deletion(),
                "velocity_bits": particle.velocity().bits(), "origin": particle.origin().to_array(),
                "drift": [particle.drift_x(), particle.drift_y(), particle.drift_z()],
                "animation_state": particle.animation_state(), "state_ai_advance": particle.state_ai_advance(),
                "translucency": particle.translucency(),
            })).collect();
            particle_systems.push(json!({"stable_id": id, "native_id": system.native_unique_id(),
                "type_id": rules.particle_system_type(system.type_id()).name, "coord": system.coords().to_array(),
                "lifetime": system.lifetime(), "spawn_frames": system.spark_spawn_frames(),
                "spawn_timer_bits": system.spawn_timer().bits(),
                "in_limbo": system.in_limbo(),
                "display_layer": sim.display_layers().layer_of(id).map(|layer| layer.index()),
                "done_spawning": system.done_spawning(), "in_logic_vector": system.in_logic_vector(),
                "owner_entity": system.owner_entity(), "attached_entity": match system.target_ref() { Some(crate::sim::combat::TargetKind::Entity(id)) => Some(id), _ => None },
                "particles": particles}));
        }
        let rng = sim.rng_views();
        let rng_value = |stream: crate::sim::rng::SimRngLogicalView<'_>| {
            json!({
            "disabled": stream.disabled, "index_a": stream.index_a, "index_b": stream.index_b, "words": stream.words})
        };
        let main = crate::sim::rng::SimRngLogicalView {
            disabled: rng.main.disabled,
            index_a: rng.main.index_a,
            index_b: rng.main.index_b,
            words: &rng.main.words,
        };
        Ok(Self {
            detail: presentation.detail.borrow().observation(),
            game_detail_level: presentation.in_game_options.detail_level,
            live,
            lights,
            particle_systems,
            native_id_cursor: sim
                .native_identity_cursor()
                .context("Scenario native identity absent")?,
            rng: json!({"main": rng_value(main), "scenario": rng_value(rng.scenario), "mapgen": rng_value(rng.mapgen)}),
        })
    }

    pub(super) fn sample_count(&self) -> usize {
        4 + self.live.len()
            + self.lights.len()
            + self
                .particle_systems
                .iter()
                .map(|system| 1 + system["particles"].as_array().map_or(0, Vec::len))
                .sum::<usize>()
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapElectricBoltDrawObservation {
    pub(super) ordinal: usize,
    pub(super) completed_steps: u64,
    pub(super) simulation_tick: u64,
    pub(super) binary_frame: u32,
    pub(super) effects: MapElectricBoltObservation,
    /// Copied from the actual pieces uploaded by this render; no diagnostic
    /// projection, atlas lookup or second instance-building pass participates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) particle_draws:
        Option<Vec<crate::app::presentation::instances::ParticleDrawObservation>>,
}

/// Boundary snapshots of the production stores, without retained provenance,
/// synthetic spawns, timer advancement or implied native frame equivalence.
#[derive(Debug, Serialize)]
pub(super) struct MapEffectObservation {
    scenario_rng_cursor: [i32; 2],
    voxel_anims: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) projectiles: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) waves: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) animations: Option<Vec<Value>>,
}

impl MapEffectObservation {
    pub(super) fn capture(
        sim: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        profile: &MapCaptureProfile,
    ) -> Result<Self> {
        let voxel_anims = sim
            .voxel_anims()
            .iter()
            .take(MAX_OBSERVATION_SAMPLES + 1)
            .map(|(&id, debris)| {
                let location = debris.world_coord();
                let bounce = debris.bounce();
                let lifecycle = debris.lifecycle();
                // Retained Object.Location and the independent Bounce body are
                // read separately; observing never advances either authority.
                json!({"stable_id": id, "native_id": debris.native_unique_id(),
                "type_id": rules.voxel_anim_type(debris.type_id()).name.as_str(),
                "owner_house": debris.owner_house().map(|owner| sim.interner().resolve(owner)),
                "stored_leptons": [location.x, location.y, location.z],
                "duration": debris.duration(), "marked_for_deletion": debris.marked_for_deletion(),
                "in_logic_vector": debris.in_logic_vector(),
                "object_alive": lifecycle.object_alive(), "in_limbo": lifecycle.in_limbo(),
                "bounce_position_bits": bounce.position().map(|value| value.bits()),
                "bounce_velocity_bits": bounce.velocity().map(|value| value.bits())})
            })
            .collect::<Vec<_>>();
        let projectiles = (profile.observe_projectiles == Some(true)).then(|| {
            sim.projectiles().iter().take(MAX_OBSERVATION_SAMPLES + 1).map(|(&id, bullet)| {
                let weapon = bullet.payload().installed_weapon().map(|id| sim.interner().resolve(id));
                let type_id = bullet.payload().projectile_type().map(|id| sim.interner().resolve(id));
                json!({"stable_id": id, "native_id": bullet.native_unique_id(),
                    "weapon": weapon, "type_id": type_id, "source_id": bullet.source_id(),
                    "physical_leptons": [bullet.position().x, bullet.position().y, bullet.position().z],
                    "in_logic_vector": bullet.in_logic_vector(), "awaiting_anim": bullet.awaiting_anim(),
                    "visual_frame": bullet.visual().runtime_frame,
                    "visual_countdown": bullet.visual().runtime_countdown})
            }).collect::<Vec<_>>()
        });
        let animations = profile.observe_anim_types.as_ref().map(|types| {
            sim.anims().filter(|(_, anim)| types.iter().any(|name|
                name.eq_ignore_ascii_case(sim.interner().resolve(anim.type_id()))))
                .take(MAX_OBSERVATION_SAMPLES + 1).map(|(&id, anim)| {
                    let absolute = sim.anim_absolute_coord(id).map(|at| [at.x, at.y, at.z]);
                    json!({"stable_id": id, "native_id": anim.native_unique_id(),
                        "type_id": sim.interner().resolve(anim.type_id()),
                        "stored_leptons": [anim.world_coord().x, anim.world_coord().y, anim.world_coord().z],
                        "physical_leptons": absolute, "owner_entity": anim.owner_entity(),
                        "in_logic_vector": anim.in_logic_vector(), "completed": anim.completed(),
                        "effective_end": anim.effective_end(), "effective_loop_end": anim.effective_loop_end(),
                        "draw_flags": anim.draw_flags(), "z_adjust": anim.z_adjust(),
                        "hidden": anim.draw_runtime().hidden,
                        "translucency_ramp": anim.draw_runtime().translucency_ramp,
                        "runtime": anim.runtime()})
                }).collect::<Vec<_>>()
        });
        let waves = (profile.observe_waves == Some(true)).then(|| {
            sim.waves().iter().take(MAX_OBSERVATION_SAMPLES + 1).map(|(&id, wave)| {
                let source = wave.source();
                let target = wave.target();
                let polygon = wave.sonic_draw_geometry([0, 0]).map(|geometry| geometry.polygon);
                json!({"stable_id": id, "wave_type": wave.wave_type(), "owner_id": wave.owner_id(),
                    "in_logic_vector": wave.in_logic_vector(), "lifetime": wave.lifetime(),
                    "active_geometry": wave.active_geometry(), "decaying": wave.decaying(),
                    "direction_octant": wave.direction_octant(),
                    "source_leptons": [source.x, source.y, source.z],
                    "target_leptons": [target.x, target.y, target.z], "sonic_polygon": polygon})
            }).collect::<Vec<_>>()
        });
        let rng = sim.rng_views().scenario;
        let snapshot = Self {
            scenario_rng_cursor: [rng.index_a, rng.index_b],
            voxel_anims,
            projectiles,
            waves,
            animations,
        };
        ensure!(
            snapshot.sample_count() <= MAX_OBSERVATION_SAMPLES,
            "effect observation exceeds sample budget"
        );
        Ok(snapshot)
    }

    pub(super) fn sample_count(&self) -> usize {
        1 + self.voxel_anims.len()
            + self.projectiles.as_ref().map_or(0, Vec::len)
            + self.waves.as_ref().map_or(0, Vec::len)
            + self.animations.as_ref().map_or(0, Vec::len)
    }
}

/// Immutable occupied-slot projection. Empty coordinates, including timed
/// ghosts cleared by pickup, disappear; the slot number preserves array order.
#[derive(Debug, Serialize)]
pub(super) struct MapCrateSlotObservation {
    pub(super) slot: usize,
    pub(super) cell: [i16; 2],
    pub(super) start_frame: i32,
    pub(super) aux: u32,
    pub(super) duration: i32,
    pub(super) overlay: Option<Value>,
}

impl MapCrateSlotObservation {
    pub(super) fn capture(sim: &crate::sim::world::Simulation) -> Vec<Self> {
        sim.crate_authority()
            .slots()
            .iter()
            .enumerate()
            .filter(|(_, slot)| !slot.is_empty())
            .map(|(index, slot)| {
                // Real-grid indexing only: observing a slot must never stamp
                // the mutable canonical Dummy through a gameplay map lookup.
                let overlay = u16::try_from(slot.cell_x)
                    .ok()
                    .zip(u16::try_from(slot.cell_y).ok())
                    .and_then(|(x, y)| {
                        let grid = sim.overlay_grid()?;
                        (x < grid.width() && y < grid.height()).then(|| grid.cell(x, y))
                    })
                    .map(|cell| json!({"id": cell.overlay_id, "data": cell.overlay_data}));
                Self {
                    slot: index,
                    cell: [slot.cell_x, slot.cell_y],
                    start_frame: slot.start_frame,
                    aux: slot.aux,
                    duration: slot.duration,
                    overlay,
                }
            })
            .collect()
    }
}

#[derive(Debug, Serialize)]
pub(super) struct MapFrameObservation {
    pub(super) completed_steps: u64,
    pub(super) simulation_tick: u64,
    pub(super) binary_frame: u32,
    pub(super) total_simulation_ms: u64,
    pub(super) actors: Vec<Value>,
    pub(super) houses: Vec<Value>,
    pub(super) missing_actor_ids: Vec<u64>,
    pub(super) terrain: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) effects: Option<MapEffectObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) lasers: Option<MapLaserObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) electric_bolts: Option<MapElectricBoltObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) radiation_beams: Option<MapRadiationBeamObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) bombs: Option<crate::sim::bomb::BombObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) crates: Option<Vec<MapCrateSlotObservation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) input: Option<MapInputObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) audio_state: Option<Value>,
}

/// Inputs retained by the ordinary terrain owners after this exact step's
/// draw. This is not a GPU readback or proof that a fragment reached the target.
/// In particular, a missing/out-of-view A sample remains null rather than
/// claiming the shader's disabled/out-of-bounds neutral fallback was sampled.
pub(super) fn terrain_render_observation(
    match_state: &MatchState,
    gpu: &crate::render::gpu::GpuContext,
    rx: u16,
    ry: u16,
) -> Value {
    let presentation = &match_state.match_presentation;
    let grid = match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| runtime.view().resolved_terrain());
    let cell = grid.and_then(|grid| grid.cell(rx, ry));
    let tactical_variant = grid.and_then(|grid| grid.tactical_draw_variant(rx, ry));
    // Observe the same live key that terrain_instances::build_visible_instances
    // requests, without calling Recalc, creating an atlas entry or substituting
    // a pristine image when the exact selected key is absent.
    let atlas_key = cell.and_then(|cell| {
        let (tile_id, sub_tile) = grid?.presentation_tile(cell);
        Some(crate::map::theater::TileKey {
            tile_id,
            sub_tile,
            variant: tactical_variant.unwrap_or(cell.variant),
        })
    });
    let uv = atlas_key.and_then(|key| presentation.tile_atlas.as_ref()?.get_uv(key));
    let lighting = presentation.lighting.grid();
    let palette_light = cell.and_then(|_| {
        lighting.cell_light_at((rx, ry))?;
        Some(crate::render::palette_light::PaletteLight::cell(lighting, (rx, ry), false).0)
    });
    let center = cell.map(|cell| {
        let (x, y) = crate::map::terrain::iso_to_screen(rx, ry, cell.level);
        [
            x + crate::map::terrain::TILE_WIDTH / 2.0,
            y + crate::map::terrain::TILE_HEIGHT / 2.0,
        ]
    });
    let camera = [match_state.input.camera_x, match_state.input.camera_y];
    let a_source = (!match_state.sandbox_full_visibility)
        .then_some(presentation.shroud_buffer.as_ref())
        .flatten();
    let a_center = center.and_then(|point| {
        a_source?.read_world(
            &gpu.device,
            &gpu.queue,
            point[0],
            point[1],
            camera[0],
            camera[1],
        )
    });
    json!({
        "phase": "after_draw",
        "variant": cell.map(|cell| cell.variant),
        "tactical_variant": tactical_variant,
        "atlas_key": atlas_key.map(|key| [u32::from(key.tile_id), u32::from(key.sub_tile), u32::from(key.variant)]),
        "atlas_ready": presentation.tile_atlas.is_some(),
        "exact_atlas_uv": uv.map(|uv| json!({"origin": uv.uv_origin, "size": uv.uv_size,
            "pixel_size": uv.pixel_size, "draw_offset": uv.draw_offset})),
        "palette_light": palette_light,
        "cell_center_world": center,
        "camera_top_left": camera,
        "a_bound": a_source.is_some(),
        "a_center": a_center,
    })
}
