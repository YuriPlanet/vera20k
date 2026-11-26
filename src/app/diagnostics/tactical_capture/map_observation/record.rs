//! Recording and publishing a map observation from the capture session: frame and
//! draw records, fingerprint, render readiness and the published run.

use super::*;

impl TacticalCaptureSession {
    pub(super) fn record_map_frame(
        &mut self,
        frontend: &FrontendState,
        match_state: &MatchState,
        sfx_player: Option<&crate::audio::sfx::SfxPlayer>,
    ) -> Result<()> {
        let profile = &self
            .request
            .map_profile()
            .context("map profile missing")?
            .value;
        let runtime = match_state
            .sim_runtime
            .runtime()
            .context("observation simulation absent")?;
        let sim = &runtime.simulation;
        let grid = runtime.view().resolved_terrain();
        // This derived diagnostic index retains every observed stable handle.
        // Captured or retyped objects continue to report their actual state;
        // destroyed objects retain a missing row rather than disappearing.
        let map = self.map_state()?;
        let mut ids = map.observed_ids.clone();
        let mut actors = Vec::new();
        if !profile.observe_owners().is_empty() {
            for (id, entity) in sim.entities().iter_sorted() {
                let owner = sim.interner().resolve(entity.owner());
                let type_name = sim.interner().resolve(entity.type_ref());
                if !map.observes_actor(profile, id, owner, type_name) {
                    continue;
                }
                ids.insert(id);
                let coord =
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                let timer = entity.mission.dispatch_timer();
                // Only the installed class owns this runtime. These value
                // getters preserve retained integer/float bits without running
                // a movement, facing, height or owner callback.
                let jumpjet = entity
                    .locomotor
                    .as_ref()
                    .and_then(|locomotor| locomotor.jumpjet_runtime())
                    .map(|runtime| {
                        let destination = runtime.destination();
                        json!({"destination_leptons": [destination.x, destination.y, destination.z],
                            "moving": runtime.moving(), "phase": runtime.phase(),
                            "landing_latched": runtime.landing_latched(),
                            "params": runtime.params(), "flight": runtime.flight()})
                    });
                let foot = if entity.category() != crate::map::entities::EntityCategory::Structure {
                    let (navigation_leptons, navigation_unavailable) =
                        match sim.foot_navigation_coordinate(id) {
                            Ok(coord) => (Some([coord.x, coord.y, coord.z]), None),
                            Err(cause) => (None, Some(cause)),
                        };
                    let tracker_cell = entity.air_tracker_cell();
                    let slot_cell = entity.air_slot_cell();
                    // Read the active Walk owner's retained coordinates and
                    // moving byte. Nav and fresh ground samples cannot stand
                    // in for its paid head or destination; other active classes
                    // and absent locomotors explicitly have no Walk observation.
                    let walk = entity.locomotor.as_ref().filter(|locomotor| {
                        locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Walk
                    });
                    let walk_head_leptons: Option<[i32; 3]> = walk
                        .and_then(|walk| walk.step_head())
                        .map(|coord| [coord.x, coord.y, coord.z]);
                    let walk_destination_leptons: Option<[i32; 3]> = walk
                        .and_then(|walk| walk.walk_destination())
                        .map(|coord| [coord.x, coord.y, coord.z]);
                    let walk_is_moving: Option<bool> = walk.and_then(|walk| walk.walk_is_moving());
                    // Read the installed Drive/Ship owner without allocating
                    // runtime or deriving a head from NavCom/path directions.
                    // Its selector/cursor distinguish a paid segment from a
                    // destination awaiting the first Process invocation.
                    let track = entity.locomotor.as_ref().and_then(|loco| {
                        use crate::sim::movement::track_process::TrackFamily;
                        let family = TrackFamily::from_kind(loco.active_kind())?;
                        let progress = loco.track_progress(family)?;
                        let valid = loco.track_valid(family)?;
                        Some(json!({
                            "family": match family {
                                TrackFamily::Drive => "Drive",
                                TrackFamily::Ship => "Ship",
                            },
                            "destination_leptons": loco.track_destination(family)
                                .map(|coord| [coord.x, coord.y, coord.z]),
                            "head_leptons": loco.track_head(family)
                                .map(|coord| [coord.x, coord.y, coord.z]),
                            "selector": progress.turn_index(),
                            "cursor": progress.cursor(),
                            "valid": valid,
                        }))
                    });
                    Some(json!({
                        // Immutable query of the sole Techno+2FC owner. A
                        // missing owner is null, never inferred full/empty.
                        "ammo_2fc": entity.ammo().map(|ammo| ammo.current()),
                        "pending_entry_500": entity.pending_entry(),
                        "retarget_after_stop_688": entity.foot_retarget_after_stop(),
                        "firing_sequence_latch_68d": entity.mission_leaf.foot_firing_sequence_latch(),
                        "infantry_doing": entity.mission_leaf.as_infantry().map(|leaf| leaf.doing()),
                        "navigation_leptons": navigation_leptons,
                        "navigation_unavailable": navigation_unavailable,
                        // Foot +560 and +564 are independent native retained
                        // Cells. NativeNull remains [0,0]; slot holders are
                        // immutable raw Cell+E0 reads, never inferred from pose.
                        "air": {
                            "tracker_cell_560": [tracker_cell.0, tracker_cell.1],
                            "slot_cell_564": [slot_cell.0, slot_cell.1],
                            "spatial_bucket": entity.air_spatial_bucket(),
                            "spatial_enter_order": entity.air_spatial_enter_order(),
                            "current_cell_slot_holder": sim.air_slots().holder(
                                entity.position.rx(), entity.position.ry()),
                            "tracker_cell_slot_holder": sim.air_slots().holder(
                                tracker_cell.0 as u16, tracker_cell.1 as u16),
                            "slot_cell_slot_holder": sim.air_slots().holder(
                                slot_cell.0 as u16, slot_cell.1 as u16),
                        },
                        "walk_head_leptons": walk_head_leptons,
                        "walk_destination_leptons": walk_destination_leptons,
                        "walk_is_moving": walk_is_moving,
                        "track": track,
                    }))
                } else {
                    None
                };
                let unit = entity.mission_leaf.as_unit().map(|leaf| {
                    let animation = entity.deploy_anim().map(|anim_id| {
                        json!({"stable_id": anim_id, "live": sim.anim(anim_id).map(|anim| {
                            json!({"type_id": sim.interner().resolve(anim.type_id()),
                                "frame": anim.runtime().current_frame, "owner_entity": anim.owner_entity()})
                        })})
                    });
                    json!({"deployed_6e0": leaf.deployed(),
                        "deploying_6e1": leaf.deploy_begin_active(),
                        "undeploying_6e2": leaf.deploy_reverse_active(),
                        "landing_for_deploy_134": entity.landing_for_deploy(),
                        "deploy_anim_130": animation, "stage_f8": entity.native_stage().value(),
                        "body_counter_538": entity.body_frame_counter,
                        "current_weapon_138": entity.current_weapon_number(),
                        "current_turret_124": entity.current_turret_index()})
                });
                let building = if entity.category()
                    == crate::map::entities::EntityCategory::Structure
                {
                    let slots: Vec<_> = entity
                        .building_art.anim_slots()
                        .iter()
                        .enumerate()
                        .filter_map(|(slot, anim_id)| {
                            anim_id.map(|anim_id| {
                                let animation = sim.anim(anim_id).map(|anim| {
                                    json!({"stable_id": anim.stable_id(),
                                        "native_id": anim.native_unique_id(),
                                        "type_id": sim.interner().resolve(anim.type_id()),
                                        "interned_type_id": anim.type_id().index(),
                                        "physical_leptons": [anim.world_coord().x, anim.world_coord().y, anim.world_coord().z],
                                        "in_logic_vector": anim.in_logic_vector(),
                                        "owner_entity": anim.owner_entity(),
                                        "building_slot": anim.building_slot(),
                                        "runtime": anim.runtime()})
                                });
                                json!({"slot": slot, "anim_id": anim_id, "animation": animation})
                            })
                        })
                        .collect();
                    Some(json!({"body_state": entity.building_body_state(),
                    "queued_body_state": entity.queued_building_body_state(),
                    "construction_control": entity.building_construction_control(),
                    "stage": entity.native_stage(), "ready_latch": entity.building_ready_latch(),
                    "actually_placed": entity.building_art.actually_placed(),
                    "last_operational": entity.building.building_last_operational,
                    "animation_slots": slots,
                    "voxel_gun": {
                        "facing": entity.body_facing_current(sim.session().binary_frame()),
                        "elevation": entity.barrel_elevation().current(sim.session().binary_frame()),
                        "hva_counter": entity.turret_anim_frame.frame(),
                        "recoil": entity.voxel_recoil().0,
                        "recoil_active": entity.voxel_recoil().1
                    }}))
                } else {
                    None
                };
                let miner = entity.miner.as_ref().map(|miner| {
                    json!({"cargo_bales": miner.cargo().len(),
                        "capacity_bales": miner.capacity_bales(),
                        "unload_active": miner.unload_active(),
                        "harvesting": miner.harvesting()})
                });
                // Preserve contact slot positions, including null holes. Reads
                // of the contact/tether owners never send a radio query.
                let contacts: Vec<_> = (0..entity.seldom.radio_contacts.capacity())
                    .map(|slot| entity.seldom.radio_contacts.slot(slot))
                    .collect();
                // Read the existing cloak/type owners, without advancing a
                // timer or inferring a phase from the rendered pixels. Signed
                // +224 progress is what the native visual query consumes.
                let cloak = if let Some(cloak) = entity.cloak.as_ref() {
                    let rules = crate::app::state::active_rules(frontend, match_state)
                        .context("cloak observation rules absent")?;
                    let object = rules
                        .object(type_name)
                        .context("cloak observation type absent")?;
                    let mut snapshot = json!({
                        "state_i32": cloak.state(),
                        "progress_i32": cloak.depth() as i32,
                        "cloaking_stages_i32": rules.general.cloaking_stages,
                        "voxel": entity.is_voxel(),
                        "no_shadow": object.no_shadow,
                    });
                    if profile.observe_cloak == Some(true) {
                        snapshot["runtime"] = cloak_runtime_observation(
                            cloak,
                            entity.in_playfield(),
                            sim.session().binary_frame() as i32,
                        );
                    }
                    Some(snapshot)
                } else {
                    None
                };
                let mut observation = json!({
                    "stable_id": id, "owner": owner,
                    "type_id": type_name, "category": entity.category(),
                    "cell": [entity.position.rx(), entity.position.ry()],
                    "physical_leptons": [coord.x, coord.y, coord.z], "on_bridge": entity.on_bridge(),
                    "health": entity.health.current(), "active": entity.is_active(),
                    "veterancy_raw_bits": entity.veterancy.raw().bits(),
                    "veterancy_rank_cache": entity.veterancy.rank_cache(),
                    "elite_flash_frames": entity.flashing.frames(),
                    "object_alive": entity.is_object_alive(),
                    "in_limbo": entity.lifecycle.in_limbo(), "dying": entity.death.is_dying(),
                    // Techno +0x1C8, the offline latch (`sim::powered_unit`).
                    "deactivated": entity.deactivated,
                    // Mark and Logic membership are independent retained facts.
                    "cell_marked": entity.lifecycle.cell_marked(),
                    "in_logic_vector": entity.seldom.in_logic_vector.is_member(),
                    "mission": {"current": entity.mission.current().raw(),
                        "queued": entity.mission.queued().raw(), "suspended": entity.mission.suspended().raw(),
                        "effective": entity.mission.effective().raw(), "handler_state": entity.mission.handler_state(),
                        "start_frame": entity.mission.mission_start_frame(), "ai_counter": entity.mission.ai_counter(),
                        "dispatch_timer": {"start_frame": timer.start_frame(), "delay": timer.delay()}},
                    "target": entity.attack_target.target(),
                    "archive": entity.archive_target(), "nav": entity.navigation.nav_com(), "foot": foot,
                    "retask": retask_observation(entity),
                    "jumpjet": jumpjet,
                    "cloak": cloak,
                    "building": building, "unit": unit,
                    "miner": miner,
                    "radio": {"contacts": contacts, "dock_entered_with": entity.seldom.tether.partner()},
                });
                if profile.observe_disguise_inputs == Some(true) {
                    observation["disguise_inputs"] =
                        disguise_inputs(frontend, match_state, entity)?;
                }
                if profile.observe_lasers == Some(true)
                    || profile.observe_electric_bolts == Some(true)
                {
                    observation["prism"] =
                        prism_observation(entity, sim.session().binary_frame() as i32);
                }
                if profile.observe_electric_bolts == Some(true) {
                    observation["damage_smoke_system_id"] =
                        json!(entity.seldom.damage_smoke_system_id);
                }
                if profile.observe_bombs == Some(true)
                    || profile.observe_temporal == Some(true)
                    || profile.observe_cloak == Some(true)
                {
                    observation["fire"] =
                        fire_observation(entity, sim.session().binary_frame() as i32);
                }
                if profile.observe_temporal == Some(true) {
                    observation["temporal"] = temporal_observation(entity);
                }
                if profile.observe_spawns == Some(true) {
                    observation["spawns"] =
                        spawn_observation(sim, entity, &runtime.resources.rules)?;
                }
                if profile.observe_action_line_inputs == Some(true) {
                    let inputs = if entity.category()
                        == crate::map::entities::EntityCategory::Structure
                    {
                        Value::Null
                    } else {
                        let rules = crate::app::state::active_rules(frontend, match_state)
                            .context("action-line observation rules absent")?;
                        let object = rules
                            .object(type_name)
                            .context("action-line observation type absent")?;
                        // Read existing authority; never call a setter, advance
                        // Facing, change SharedCellDummy or recompute an order.
                        // Fixed-point raw bits are labelled as such, not passed
                        // off as native doubles. Native comparisons establish
                        // each getter before consuming these prepared inputs.
                        json!({
                            "body_facing": entity.body_facing_current(sim.session().binary_frame()),
                            "turret_facing": entity.barrel_facing.as_ref()
                                .map(|facing| facing.current(sim.session().binary_frame())),
                            "locomotor": entity.locomotor.as_ref().map(|loco| loco.active_kind()),
                            "is_moving": crate::sim::movement::motion_query::is_moving(entity),
                            "applied_speed_fraction_fixed_bits": entity.foot_speed.applied_fraction().to_bits(),
                            "crate_speed_multiplier_f64_bits": entity.foot_speed.crate_multiplier().bits(),
                            "house_speed_bonus_f32_bits": crate::sim::movement::owner_speed_bonus(
                                sim.houses(), entity, Some(object)).bits(),
                            "current_speed": crate::sim::movement::owner_current_speed(
                                entity, Some(object), rules.general.veteran_speed, sim.houses()),
                            "veterancy": entity.veterancy.rank(),
                            "current_weapon": crate::sim::combat::combat_weapon::current_weapon(
                                entity, object, sim.entities(), rules, sim.interner())
                                .map(|weapon| weapon.id.as_str()),
                            "turret_offset": crate::sim::combat::fire_coord::firer_art(rules, object)
                                .map_or(0, |art| art.turret_offset),
                            "rocking_angles_fixed_bits": entity.rocking.as_ref().map(|rocking| [
                                rocking.angle_sideways().to_bits(), rocking.angle_forwards().to_bits()]),
                        })
                    };
                    observation["action_line_inputs"] = inputs;
                }
                actors.push(observation);
            }
        }
        let missing_actor_ids = ids
            .iter()
            .copied()
            .filter(|id| !sim.entities().contains(*id))
            .collect();
        let houses = profile
            .observe_owners()
            .iter()
            .map(|owner| {
                let economy = crate::sim::house_state::house_state_for_owner(
                    sim.houses(),
                    owner,
                    sim.interner(),
                )
                .map(|house| {
                    json!({
                        "credits": house.economy().credits(),
                        "spent_credits": house.economy().spent_credits(),
                        "score": house.economy().score(),
                    })
                });
                // Missing Houses stay explicit rather than inventing a zero
                // balance. The row reads the wallet play uses, but only the
                // run schema's three fields, not its saved layout.
                let mut row = json!({"owner": owner, "economy": economy});
                if profile.observe_super_weapons == Some(true) {
                    row["super_weapons"] = super_weapon_rows(sim, owner);
                }
                if profile.observe_diplomacy == Some(true) {
                    row["diplomacy"] = diplomacy_row(sim, owner);
                }
                row
            })
            .collect();
        let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state);
        let local_visibility_owner = local_owner
            .as_deref()
            .and_then(|owner| sim.interner().get(owner).map(|id| (owner, id)));
        let terrain = profile.terrain_cells().iter().map(|&[rx, ry]| {
            // Immutable real-cell indexing only. A diagnostic lookup must not
            // stamp canonical Dummy or evaluate gameplay height/zone queries.
            let cell = grid.and_then(|grid| grid.cell(rx, ry));
            let at = (rx, ry);
            let overlay = sim.overlay_grid().map(|grid| grid.cell(rx, ry));
            let terrain_object = sim.terrain_objects().cell_index().get(&at)
                .and_then(|id| sim.terrain_objects().objects().get(id));
            let animation = terrain_object.and_then(|object| sim.terrain_objects().animations().get(&object.stable_id()));
            let terrain_row = |object: &crate::sim::terrain_object::TerrainObjectState| {
                let id = object.stable_id();
                let animation = sim.terrain_objects().animations().get(&id);
                json!({"id": id, "native_id": object.native_unique_id(), "name": sim.interner().resolve(object.type_ref()),
                    "health": object.health(), "max_health": object.max_health(),
                    "frame": animation.map(|animation| animation.current_frame()),
                    "active": animation.map(|animation| animation.is_active())})
            };
            let member_ids: Vec<_> = sim.terrain_objects().members_at(at).collect();
            let terrain_members: Vec<_> = member_ids.iter().map(|id| {
                terrain_row(sim.terrain_objects().get(*id).expect("indexed Terrain remains represented"))
            }).collect();
            // Location, Cell content, Display and Logic are independent native
            // memberships. Observe the retained owner even when Occupy_List
            // placed no Cell member, and foreign-anchor members reached here;
            // this never constructs or marks an object.
            let terrain_instances: Vec<_> = sim.terrain_objects().objects().values()
                .filter(|object| object.cell() == at || member_ids.contains(&object.stable_id())).map(|object| {
                    let mut row = terrain_row(object);
                    row["lifecycle"] = json!(object.lifecycle());
                    row["cell_member"] = json!(member_ids.contains(&object.stable_id()));
                    row["display_layer"] = json!(sim.display_layers().layer_of(object.stable_id()).map(|layer| layer.index()));
                    row["in_logic_vector"] = json!(object.in_logic_vector());
                    row
            }).collect();
            // Observe the same viewer plane consumed by drawing and picking;
            // never reveal a cell or create missing visibility state for capture.
            let local_visibility = local_visibility_owner.map(|(owner, id)| json!({
                "owner": owner, "revealed": sim.fog().is_cell_revealed(id, rx, ry),
                "visible": sim.fog().is_cell_visible(id, rx, ry),
                "gap_covered": sim.fog().is_cell_gap_covered(id, rx, ry),
            }));
            // Borrow the current simulation projection at the point of use.
            // The existing `walkable` field below describes bridge terrain,
            // not this dynamic structure-blocking navigation grid.
            let path_cell = sim.path_grid().and_then(|path| path.cell(rx, ry));
            json!({"cell": [rx, ry], "allocated": cell.is_some(),
                "path_grid": path_cell.map(|path| json!({
                    "ground_walkable": path.ground_walkable,
                    "bridge_walkable": path.bridge_walkable,
                })),
                "local_visibility": local_visibility,
                "overlay": overlay.map(|overlay| json!({"id": overlay.overlay_id, "density": overlay.overlay_data})),
                "terrain_objects": terrain_members,
                "terrain_instances": terrain_instances,
                "terrain_object": terrain_object.map(|object| json!({"name": sim.interner().resolve(object.type_ref()),
                    "frame": animation.map(|animation| animation.current_frame()),
                    "active": animation.map(|animation| animation.is_active())})),
                "final_tile_index": cell.map(|cell| cell.final_tile_index),
                "final_sub_tile": cell.map(|cell| cell.final_sub_tile),
                "presentation_tile": cell.and_then(|cell| grid.map(|grid| grid.presentation_tile(cell))),
                "level": cell.map(|cell| cell.level), "slope": cell.map(|cell| cell.slope_type),
                "raw_bridge_flags": cell.map(|cell| cell.bridge_facts.raw_flags),
                "bridge_state": cell.and(overlay).map(|overlay| overlay.overlay_data),
                "has_deck": cell.map(|cell| cell.has_bridge_deck),
                "deck_level": cell.map(|cell| grid
                    .and_then(|grid| grid.corrected_high_bridge_deck_level(at))
                    .map_or(cell.bridge_deck_level, |deck| deck as u8)),
                "walkable": cell.map(|cell| cell.bridge_walkable), "transition": cell.map(|cell| cell.bridge_transition),
                "render": null})
        }).collect();
        let frame = MapFrameObservation {
            completed_steps: self.exact_step_receipts.len() as u64,
            simulation_tick: sim.session().tick(),
            binary_frame: sim.session().binary_frame(),
            total_simulation_ms: sim.session().total_sim_ms(),
            audio_state: profile
                .observe_audio
                .as_ref()
                .map(|_| sound_state(&match_state.sim_runtime, sfx_player, profile))
                .transpose()?,
            actors,
            houses,
            missing_actor_ids,
            terrain,
            effects: profile
                .observes_effects()
                .then(|| MapEffectObservation::capture(sim, &runtime.resources.rules, profile))
                .transpose()?,
            lasers: (profile.observe_lasers == Some(true))
                .then(|| MapLaserObservation::capture(match_state))
                .transpose()?,
            electric_bolts: profile
                .observes_electric_step(self.exact_step_receipts.len() as u64)
                .then(|| MapElectricBoltObservation::capture(match_state))
                .transpose()?,
            radiation_beams: (profile.observe_radiation_beams == Some(true))
                .then(|| {
                    MapRadiationBeamObservation::capture(
                        &match_state.match_presentation.radiation_beams,
                    )
                })
                .transpose()?,
            bombs: (profile.observe_bombs == Some(true))
                .then(|| sim.bomb_observation(MAX_OBSERVATION_SAMPLES + 1)),
            crates: (profile.observe_crates == Some(true))
                .then(|| MapCrateSlotObservation::capture(sim)),
            input: profile
                .gestures
                .as_ref()
                .map(|_| {
                    MapInputObservation::capture(
                        frontend,
                        match_state,
                        profile.observes_local_input(),
                    )
                })
                .transpose()?,
        };
        let sidebar = profile
            .observes_sidebar_step(&frame)
            .then(|| MapSidebarObservation::capture(&match_state.match_presentation))
            .transpose()?;
        let completed_steps = frame.completed_steps;
        let map = self.map_state_mut()?;
        map.observe_frame(frame, ids)?;
        if let Some(sidebar) = sidebar {
            map.sample_count = map
                .sample_count
                .checked_add(sidebar.sample_count())
                .context("sidebar observation sample count overflow")?;
            ensure!(
                map.sample_count <= MAX_OBSERVATION_SAMPLES,
                "sidebar observation exceeds sample budget"
            );
            map.sidebar_frames.push(MapSidebarFrame {
                completed_steps,
                rendered: false,
                sidebar,
            });
        }
        Ok(())
    }

    pub(in crate::app::diagnostics::tactical_capture::session) fn observe_map_draw(
        &mut self,
        diag: &DiagnosticsState,
        match_state: &MatchState,
        gpu: &crate::render::gpu::GpuContext,
        output: &GameRenderOutput,
    ) -> Result<()> {
        let profile = &self
            .request
            .map_profile()
            .context("map profile missing")?
            .value;
        let requested = profile.ticks;
        let tooltip = (profile.observe_tooltips == Some(true))
            .then(|| capture_tooltip(match_state))
            .transpose()?;
        let sim = &match_state
            .sim_runtime
            .runtime()
            .context("map simulation absent")?
            .simulation;
        let step = self.exact_step_receipts.len() as u64;
        ensure!(
            (sim.session().tick(), sim.session().binary_frame())
                == self.map_state()?.expected_clock(step, false),
            "map draw differs from committed exact-step receipts"
        );
        ensure!(
            crate::app::diagnostics::state::diagnostic_presentation_ms(diag, match_state)
                == Some(output.times.radar_ms),
            "map diagnostic presentation policy is not active"
        );
        let electric = self
            .request
            .map_profile()
            .unwrap()
            .value
            .observes_electric_step(step)
            .then(|| MapElectricBoltObservation::capture(match_state))
            .transpose()?;
        let radiation = (profile.observe_radiation_beams == Some(true))
            .then(|| {
                MapRadiationBeamObservation::capture(
                    &match_state.match_presentation.radiation_beams,
                )
            })
            .transpose()?;
        let terrain_render: Vec<_> = profile
            .terrain_cells()
            .iter()
            .map(|&[rx, ry]| terrain_render_observation(match_state, gpu, rx, ry))
            .collect();
        let map = self.map_state_mut()?;
        if let Some(effects) = electric {
            map.sample_count = map
                .sample_count
                .checked_add(
                    effects.sample_count() + output.particle_draws.as_ref().map_or(0, Vec::len),
                )
                .context("draw observation count overflow")?;
            ensure!(
                map.sample_count <= MAX_OBSERVATION_SAMPLES,
                "draw observation exceeds sample budget"
            );
            map.electric_bolt_draws
                .push(MapElectricBoltDrawObservation {
                    ordinal: map.electric_bolt_draws.len(),
                    completed_steps: step,
                    simulation_tick: sim.session().tick(),
                    binary_frame: sim.session().binary_frame(),
                    effects,
                    particle_draws: output.particle_draws.clone(),
                });
        }
        if let Some(beams) = radiation {
            map.sample_count = map
                .sample_count
                .checked_add(beams.sample_count())
                .context("radiation beam draw observation count overflow")?;
            ensure!(
                map.sample_count <= MAX_OBSERVATION_SAMPLES,
                "radiation beam draw observation exceeds sample budget"
            );
            map.radiation_beam_draws
                .push(MapRadiationBeamDrawObservation {
                    ordinal: map.radiation_beam_draws.len(),
                    completed_steps: step,
                    simulation_tick: sim.session().tick(),
                    binary_frame: sim.session().binary_frame(),
                    beams,
                });
        }
        if map.audio_tail_started.is_some() && map.draws.len() == requested.max(1) as usize {
            // The existing render/audio service keeps running during the tail.
            // These repeat draws do not manufacture extra exact-step receipts.
            ensure!(
                output.times.radar_ms == sim.session().tick() * MAP_PRESENTATION_INTERVAL_MS,
                "audio-tail draw changed the diagnostic simulation clock"
            );
            map.audio_tail_draws = map.audio_tail_draws.saturating_add(1);
            return Ok(());
        }
        map.observe_draw(requested, step, output.times)?;
        map.draws.last_mut().expect("draw just observed").tooltip = tooltip;
        if let Some(frame) = map
            .frames
            .last_mut()
            .filter(|frame| frame.completed_steps == step)
        {
            for (cell, rendered) in frame.terrain.iter_mut().zip(terrain_render) {
                cell["render"] = rendered;
            }
        }
        if let Some(frame) = map
            .sidebar_frames
            .last_mut()
            .filter(|frame| frame.completed_steps == step)
        {
            let rendered = MapSidebarObservation::from_view(
                output
                    .drawn_sidebar_view(&match_state.match_presentation)
                    .context("requested sidebar draw is absent")?,
            )?;
            ensure!(
                frame.sidebar == rendered,
                "rendered sidebar differs from its retained frame observation"
            );
            frame.rendered = true;
        }
        Ok(())
    }

    pub(in crate::app::diagnostics::tactical_capture::session) fn map_fingerprint(
        &self,
        sim_runtime: &SimHost,
    ) -> Result<Value> {
        let sim = &sim_runtime
            .runtime()
            .context("simulation absent")?
            .simulation;
        Ok(json!(FinalFingerprint {
            simulation_tick: sim.session().tick(),
            total_simulation_ms: sim.session().total_sim_ms(),
            binary_frame: sim.session().binary_frame(),
            deterministic_state_hash: sim.state_hash()
        }))
    }

    pub(in crate::app::diagnostics::tactical_capture::session) fn map_render_readiness(
        &self,
        platform: &PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        persistence: &PersistenceState,
        output: &GameRenderOutput,
    ) -> Result<(bool, Value)> {
        validate_loaded_resources(frontend, match_state, process_assets)?;
        let unit_atlas = unit_atlas_statistics(match_state.match_presentation.unit_atlas.as_ref())?;
        let sidebar_drawn = output
            .drawn_sidebar_view(&match_state.match_presentation)
            .is_some();
        let ready = sidebar_drawn
            && !match_state.paused()
            && !frontend.main_menu_dialog_open()
            && !diag.debug_show_pathgrid
            && !diag.debug_show_cell_grid
            && !diag.debug_show_heightmap
            && self.focus_violations == 0
            && self.input_violations == 0;
        let static_default_cursor = crate::app::presentation::ui_overlays::static_default_cursor(
            platform,
            renderer,
            frontend,
            match_state,
            persistence,
        );
        let camera_input_idle = crate::app::input::camera::camera_input_idle(
            renderer,
            &frontend.screen,
            &match_state.input,
        );
        let cursor_position = [match_state.input.cursor_x, match_state.input.cursor_y];
        ensure!(
            ready && static_default_cursor && camera_input_idle,
            "map observation requires every draw ready with a static cursor and idle camera input: ready={ready}, static_cursor={static_default_cursor}, camera_idle={camera_input_idle}, cursor_feedback={:?}, cursor_position={cursor_position:?}",
            crate::app::input::cursor::current_cursor_feedback_kind(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
                &crate::app::input::cursor::CursorTargets::read(platform, frontend, match_state),
            )
        );
        let profile = &self
            .request
            .map_profile()
            .context("map profile missing")?
            .value;
        if let Some(requested) = profile.cursor_position {
            ensure!(
                cursor_position == requested.map(|value| value as f32),
                "map observation cursor moved: requested={requested:?}, actual={cursor_position:?}"
            );
        }
        // Existing frame-boundary timer: up to 60 wall intervals, including
        // simulation, diagnostic observation and presentation/vsync work.
        // This is capture cadence metadata, not GPU time or ordinary play FPS.
        let frame_wall_mean_ms = diag.frame_timer.frame_ms_mean();
        ensure!(
            frame_wall_mean_ms.is_finite() && frame_wall_mean_ms >= 0.0,
            "map observation frame wall mean is invalid"
        );
        let mut render = json!({"ready": ready, "sidebar_view_present": sidebar_drawn,
            "instance_counts": super::super::super::evidence::RenderInstanceCountEvidence::from_counts(output.instance_counts)?,
            "internal_extent": [renderer.render_width(&frontend.screen), renderer.render_height(&frontend.screen)],
            "surface_extent": [renderer.gpu.config.width, renderer.gpu.config.height],
            "gpu": super::super::super::evidence::GpuAdapterEvidence::from_observation(renderer.gpu.capture_adapter_observation()),
            "unit_atlas": unit_atlas,
            "frame_wall_mean_ms": frame_wall_mean_ms,
            "neutral_input": {"static_default_cursor": static_default_cursor, "camera_input_idle": camera_input_idle},
            "camera": {"requested_cell": profile.camera_cell,
                "top_left": [match_state.input.camera_x, match_state.input.camera_y],
                "zoom": match_state.input.zoom_level},
        });
        if profile.cursor_position.is_some() {
            render["cursor_position"] = json!(cursor_position);
        }
        Ok((ready, render))
    }

    pub(in crate::app::diagnostics::tactical_capture::session) fn publish_map_observation(
        &self,
        platform: &PlatformState,
        sim_runtime: &SimHost,
        format: wgpu::TextureFormat,
        pixels: &[u8],
    ) -> Result<()> {
        let profile = self.request.map_profile().context("map profile missing")?;
        ensure!(
            self.exact_step_receipts.len() == profile.value.ticks as usize,
            "incomplete exact-step sequence"
        );
        let frame = FrameArtifact::from_bgra(
            self.request.width(),
            self.request.height(),
            format!("{format:?}"),
            pixels,
        )?;
        let map = self.map_state()?;
        if profile.value.observe_audio.is_some() {
            ensure!(
                map.audio.is_some(),
                "audio observation has no completion report"
            );
        }
        ensure!(
            map.draws.len() == profile.value.ticks.max(1) as usize,
            "incomplete map draw schedule"
        );
        ensure!(
            map.frames.len() == profile.value.ticks as usize + 1
                && map.commands.len() == profile.value.commands().len()
                && map.gestures.len() == profile.value.gestures().len(),
            "incomplete actor/command observation transcript"
        );
        let mut render = self
            .last_render_evidence
            .clone()
            .context("map render evidence absent")?;
        // Serialize the actual transcript only once, avoiding quadratic work
        // across the up-to-100000-step capture route.
        render["presentation_clock"] = json!({"policy": MAP_PRESENTATION_CLOCK_POLICY,
            "origin_ms": 0, "interval_ms": MAP_PRESENTATION_INTERVAL_MS, "draws": map.draws});
        let mut manifest = json!({
            "schema_version": CHILD_SCHEMA, "status": "COMPLETE",
            "profile": {"sha256": profile.sha256, "request": profile.value},
            "contract": {"sha256": self.request.sealed_contract().sha256},
            "inputs": self.map_state()?.inputs, "map_source": self.map_source_evidence,
            "startup": self.startup_evidence, "initial": self.map_state()?.initial,
            "loaded_session": self.map_state()?.loaded_session,
            "final": self.map_fingerprint(sim_runtime)?, "exact_step_count": self.exact_step_receipts.len(),
            "first_exact_step": self.exact_step_receipts.first(), "last_exact_step": self.exact_step_receipts.last(),
            "frame": frame, "render": render,
            "observations": map.transcript(&profile.value),
            "lifecycle": {"window_hidden": platform.window.is_visible() == Some(false),
                "window_focused": platform.window.has_focus(), "focus_violations": self.focus_violations,
                "input_violations": self.input_violations},
            "native_comparator": "NONE", "parity_certification": "NONE",
            "evidence_limitations": ["Production loading, exact stepping and GPU readback only; no native pixel or gameplay equivalence is established.",
                "Radar and timed HUD presentation consume the recorded diagnostic exact-step clock; ordinary gameplay clocks are unchanged. Audio, menus, animated input and scenario exit are outside this comparison."],
        });
        if profile.value.observe_audio.is_some() {
            manifest["observations"]["audio"]["tail_draw_count"] = json!(map.audio_tail_draws);
            manifest["evidence_limitations"].as_array_mut().expect("limits array").push(json!(
                "Opt-in audio records the production device mixer's Player queue pulls, including possible initialization prefetch, before resampling/summing. It is not OS loopback, proof of physical speaker output, or native audio parity. Audio-only tail draws advance no simulation steps."));
        }
        ensure!(
            encode_manifest(&manifest)?.len() < MAX_RECEIPT_BYTES,
            "map observation manifest exceeds the 128 MiB receipt budget"
        );
        publish_transaction(
            self.request.output_dir(),
            &manifest,
            Some(pixels),
            PublishFault::None,
        )
    }

    pub(in crate::app::diagnostics::tactical_capture::session) fn publish_map_failure(
        &self,
        error: &str,
    ) -> Result<()> {
        let profile = self.request.map_profile().context("map profile missing")?;
        let map = self.map_state()?;
        let manifest = json!({"schema_version": CHILD_SCHEMA, "status": "FAILED",
            "profile": {"sha256": profile.sha256, "request": profile.value},
            "contract": {"sha256": self.request.sealed_contract().sha256},
            "failure": {"stage": self.failure_stage, "message": error}, "frame": null,
            "exact_step_count": self.exact_step_receipts.len(), "map_source": self.map_source_evidence,
            "observations": map.transcript(&profile.value),
            "native_comparator": "NONE", "parity_certification": "NONE"});
        ensure!(
            encode_manifest(&manifest)?.len() < MAX_RECEIPT_BYTES,
            "map failure manifest exceeds the 128 MiB receipt budget"
        );
        publish_transaction(
            self.request.output_dir(),
            &manifest,
            None,
            PublishFault::None,
        )
    }
}

/// A scene with no voxel demand never builds the unit atlas (the builder keeps
/// its absent `existing` atlas), so its receipt is an atlas with no pages.
pub(super) fn unit_atlas_statistics(
    atlas: Option<&crate::render::unit_atlas::UnitAtlas>,
) -> Result<crate::render::unit_atlas::UnitAtlasStatistics> {
    atlas.map_or_else(|| Ok(Default::default()), |atlas| atlas.statistics())
}
