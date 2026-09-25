//! Voxel unit instance builders — per-frame SpriteInstance generation for VXL entities.
//!
//! Handles turret/barrel separation, harvest overlays, and VXL animation frames.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use super::helpers::{
    EntityDrawBand, compute_sprite_depth, entity_draw_band, ground_sort_row, in_view,
    tactical_entity_render_admission,
};
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    GroundPieceInstance, GroundTexture, NativeGroundOrder, PlannedGroundObjectInstance,
};
use crate::map::entities::EntityCategory;
use crate::map::lighting;
use crate::map::terrain::{TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::{DrawState, FX_SHADOW, ObserverDrawContext};
use crate::render::native_z::{
    SHP_DRAW_Z_ADJUST_PX, ZGradient, pack_voxel_z_gradient, pack_z_gradient,
};
use crate::render::sprite_atlas::ShpSpriteKey;
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::unit_atlas::{
    UnitSpriteEntry, UnitSpriteKey, VxlLayer, canonical_turret_facing, canonical_unit_facing,
};
use crate::render::unit_slope_transition_cache::{
    TransitionUnitSpriteEntry, TransitionUnitSpriteKey,
};
use crate::rules::house_colors::{self, HouseColorIndex};
use crate::sim::components::HarvestOverlay;
#[cfg(test)]
use crate::sim::movement::slope_transition::SLOPE_TRANSITION_FRAMES;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

/// One-shot tripwire: fires the first time a `slope_type >= 17` byte is
/// observed at the render hand-off. Subsequent observations are silent
/// (single Relaxed load on the fast path, branch-prediction friendly).
///
/// Slopes 17-20 are unpopulated in gamemd's runtime slope-matrix table
/// (BSS-zero at DAT_00b454B8 per VOXEL_SLOPE_TILT_SYSTEM.md). The
/// existence of such bytes in shipping TMP data is empirically unknown;
/// this log surfaces them at runtime so the deferred TMP scan can be
/// scheduled if it ever fires.
static WARNED_SLOPE_GE_17: AtomicBool = AtomicBool::new(false);

const NO_SPAWN_ALT_SUFFIX: &str = "WO";
const NATIVE_TURRET_FIRST_START: u8 = 32;
const NATIVE_TURRET_FIRST_END_EXCLUSIVE: u8 = 160;

fn vxl_body_tint(
    grid: &lighting::CellLightGrid,
    cell: (u16, u16),
    category: EntityCategory,
    extra_unit_light: i32,
    extra_aircraft_light: i32,
) -> [f32; 3] {
    match category {
        EntityCategory::Unit => grid.unit_tint_at(cell, extra_unit_light),
        EntityCategory::Aircraft => {
            // AircraftClass adds a separate altitude/Scenario-Level term in
            // gamemd. Until that term is represented, preserve this existing
            // compatibility RGB path while consuming the native i32 parser
            // value at its normalized scale.
            let mut tint = grid.aircraft_tint_at(cell);
            let glow = extra_aircraft_light as f32 / lighting::LIGHT_UNIT as f32;
            if glow > 0.0 {
                tint[0] = (tint[0] + glow).min(lighting::TOTAL_AMBIENT_CAP);
                tint[1] = (tint[1] + glow).min(lighting::TOTAL_AMBIENT_CAP);
                tint[2] = (tint[2] + glow).min(lighting::TOTAL_AMBIENT_CAP);
            }
            tint
        }
        _ => grid.techno_tint_at(cell),
    }
}

/// The active `NoSpawnAlt` art id for one Unit draw, if any.
///
/// `UnitClass` reads `NoSpawnAlt`, calls
/// `SpawnManagerClass::CountDockedSpawns` (`0x006B7D50`), and selects the
/// preloaded `%sWO` auxiliary voxel only when that count is zero. This is a
/// presentation-time query: live but non-docked children still select the
/// alternate model, and no spawn-manager AI cadence participates.
fn no_spawn_alt_type_id(
    base_type: &str,
    no_spawn_alt: bool,
    manager: Option<&crate::sim::spawn_manager::SpawnManagerState>,
) -> Option<String> {
    (no_spawn_alt && manager.is_some_and(|manager| manager.count_docked_spawns() == 0))
        .then(|| format!("{base_type}{NO_SPAWN_ALT_SUFFIX}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitRenderSlopeState {
    Stable(u8),
    Transition {
        from_slope: u8,
        to_slope: u8,
        phase_num: i32,
        phase_den: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitTextureSource {
    Stable(usize),
    Transition(usize),
}

fn warn_unexpected_slope_once(slope: u8, rx: u16, ry: u16) {
    if WARNED_SLOPE_GE_17.load(Ordering::Relaxed) {
        return;
    }
    if WARNED_SLOPE_GE_17
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        log::warn!(
            "slope_type {} encountered at cell ({}, {}); gamemd has no \
             matrix populated for slopes 17-20 — rendering flat. This \
             is the first observation of this slope range in the \
             current process; subsequent observations are silent.",
            slope,
            rx,
            ry,
        );
    }
}

fn clamp_slope_for_render(slope: u8) -> u8 {
    if slope <= 16 { slope } else { 0 }
}

fn terrain_slope_for_render(state: &AppState, rx: u16, ry: u16) -> u8 {
    state
        .terrain_template()
        .and_then(|t| t.cell(rx, ry))
        .map(|c| {
            let raw = c.slope_type;
            if raw <= 16 {
                raw
            } else {
                warn_unexpected_slope_once(raw, rx, ry);
                0
            }
        })
        .unwrap_or(0)
}

fn unit_render_slope_state(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
    display_binary_frame: u32,
    band: EntityDrawBand,
) -> UnitRenderSlopeState {
    // Drive/Ship Draw_Matrix reads the locomotor-owned cache even when the
    // cached slope is zero. Terrain is only a compatibility fallback for
    // locomotor classes excluded from the native override pair.
    if let Some(slope_state) = locomotor_render_slope_state(entity, display_binary_frame) {
        return slope_state;
    }
    if entity.category == EntityCategory::Aircraft {
        return UnitRenderSlopeState::Stable(0);
    }
    // A body that has left the floor has no cell slope to sit on, so it never
    // enters the drive-track tilt transition. This also keeps every Top-band
    // body on the stable-atlas path, which is the only path the Top stream
    // carries. (VERA-internal; no stock YR voxel unit uses Jumpjet, so the
    // gamemd equivalent for an airborne tilt is UNCHECKED.)
    if band == EntityDrawBand::Top {
        return UnitRenderSlopeState::Stable(0);
    }

    UnitRenderSlopeState::Stable(terrain_slope_for_render(
        state,
        entity.position.rx,
        entity.position.ry,
    ))
}

fn locomotor_render_slope_state(
    entity: &crate::sim::game_entity::GameEntity,
    display_binary_frame: u32,
) -> Option<UnitRenderSlopeState> {
    crate::sim::movement::slope_transition::state_for_entity(entity).map(|slope_state| {
        match slope_state.render_phase(display_binary_frame) {
            crate::sim::movement::slope_transition::SlopeRenderPhase::Stable(slope) => {
                UnitRenderSlopeState::Stable(clamp_slope_for_render(slope))
            }
            crate::sim::movement::slope_transition::SlopeRenderPhase::Transition {
                from_slope,
                to_slope,
                phase_num,
                phase_den,
            } => {
                let from_slope = clamp_slope_for_render(from_slope);
                let to_slope = clamp_slope_for_render(to_slope);
                if from_slope == to_slope {
                    UnitRenderSlopeState::Stable(to_slope)
                } else {
                    UnitRenderSlopeState::Transition {
                        from_slope,
                        to_slope,
                        phase_num,
                        phase_den,
                    }
                }
            }
        }
    })
}

/// Presentation follows the just-committed simulation frame. Drive/Ship
/// `Draw_Matrix` therefore observes the last frame whose Process entry has
/// completed, including the unsigned wrap at the initial committed frame.
const fn display_binary_frame_for_committed_session(committed_binary_frame: u32) -> u32 {
    committed_binary_frame.wrapping_sub(1)
}

/// Depth key for one voxel body, from the screen row it was drawn at.
///
/// The entity's height comes off the sort key. Bridge depth treatment belongs
/// to the unit's composite blit, never to a separate object painter queue.
fn body_sort_depth(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
    _band: EntityDrawBand,
    drawn_row_y: f32,
    z: u8,
) -> f32 {
    compute_sprite_depth(state, ground_sort_row(entity, drawn_row_y), z)
}

/// Iterate visible voxel units from EntityStore and build SpriteInstances.
///
/// Non-turret units emit a single Composite sprite. Turret units emit up to 3
/// sprites: Body at body facing, Turret + Barrel at turret facing with screen
/// offset computed from art.ini TurretOffset.
///
/// `top_instances` receives bodies registered in Display layers above the
/// Ground band — an aircraft off its pad, a jumpjet at hover height, a missile
/// in flight. That band is drawn after every ground object, so it is kept
/// separate from `instances` rather than merged by depth.
pub(crate) fn build_unit_instances(
    state: &AppState,
    instances: &mut Vec<SpriteInstance>,
    instance_pages: &mut Vec<usize>,
    top_instances: &mut Vec<SpriteInstance>,
    top_instance_pages: &mut Vec<usize>,
    transition_instances: &mut Vec<Vec<SpriteInstance>>,
    shp_paged: &mut [Vec<SpriteInstance>],
    ground_objects: &mut Vec<PlannedGroundObjectInstance>,
    ground_order: &NativeGroundOrder,
) {
    let (sim, atlas) = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.unit_atlas,
    ) {
        (Some(s), Some(a)) => (s, a),
        _ => return,
    };
    let z = state.match_state.input.zoom_level;
    // Presentation runs after the just-completed simulation frame; snapshot
    // the one binary frame used by every Drive/Ship Draw_Matrix in this pass.
    let display_binary_frame = display_binary_frame_for_committed_session(sim.session.binary_frame);
    let (cam_x, cam_y, sw, sh) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.render_width() as f32 / z,
        state.render_height() as f32 / z,
    );
    let local_owner = crate::app::input::commands::preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|o| sim.interner.get(o));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let art_reg: Option<&crate::rules::art_data::ArtRegistry> =
        state.rules().map(|rules| &rules.art_registry);

    let encounter_order = super::helpers::tactical_entity_encounter_order(sim);
    for stable_id in encounter_order {
        let Some(entity) = sim.entities().get(stable_id) else {
            continue;
        };
        if !entity.is_voxel {
            continue;
        }
        let Some(band) = entity_draw_band(sim.display_layers(), stable_id) else {
            continue;
        };
        // Common visibility, passenger, limbo, and DrawState admission is shared below.
        let pos = &entity.position;
        let owner_str = sim.interner.resolve(entity.owner());
        // Disguise remains the outer display-type choice. For an ordinary
        // undisguised Unit, `NoSpawnAlt` is selected from the current docked
        // slot count at draw time; the serialized override remains solely the
        // miner dock sub-FSM's UnloadingClass (HORV/CMON) hint.
        let active_disguise = entity.disguise.as_ref().filter(|state| state.disguised);
        let base_type = sim.interner.resolve(entity.type_ref());
        let no_spawn_alt = state
            .rules()
            .and_then(|rules| rules.object(base_type))
            .is_some_and(|object| object.no_spawn_alt);
        let no_spawn_alt_type =
            no_spawn_alt_type_id(base_type, no_spawn_alt, entity.spawn_manager.as_ref());
        let type_name: Cow<'_, str> = if let Some(disguise_type) = active_disguise
            .and_then(|disguise| disguise.disguise_type)
            .map(|id| sim.interner.resolve(id))
        {
            Cow::Borrowed(disguise_type)
        } else if let Some(no_spawn_alt_type) = no_spawn_alt_type {
            Cow::Owned(no_spawn_alt_type)
        } else if let Some(display_override) = (!no_spawn_alt)
            .then_some(entity.display_type_override)
            .flatten()
            .map(|id| sim.interner.resolve(id))
        {
            Cow::Borrowed(display_override)
        } else {
            Cow::Borrowed(base_type)
        };
        let type_str = type_name.as_ref();
        let remap_owner = active_disguise
            .and_then(|state| state.disguised_as_house)
            .map(|id| sim.interner.resolve(id))
            .unwrap_or(owner_str);
        let hc: HouseColorIndex = state
            .match_state
            .match_presentation
            .house_color_map
            .get(remap_owner)
            .copied()
            .unwrap_or_default();
        let Some(draw_decision) = tactical_entity_render_admission(
            entity,
            owner_str,
            local_owner.as_deref(),
            local_owner_id,
            &sim.fog,
            ignore_visibility,
            sim.session.binary_frame,
            house_color_to_remap_row(hc),
            ObserverDrawContext {
                owner_is_allied: local_owner.as_deref().is_some_and(|observer| {
                    crate::map::houses::is_allied_with(&sim.house_alliances, observer, owner_str)
                }),
                detects_cloak: local_owner_id
                    .is_some_and(|observer| sim.fog.has_sensor_for_house(observer, pos.rx, pos.ry)),
            },
        ) else {
            continue;
        };
        // Render slope comes from the locomotor's cached previous/current
        // slope during gamemd's 3-frame transition, then falls back to the
        // stable terrain slope path.
        let slope_state = unit_render_slope_state(state, entity, display_binary_frame, band);
        let (sx, sy) = crate::render::locomotor_visual::screen_position(entity);
        let interp_z = pos.z;
        if !in_view(sx, sy, TILE_WIDTH, TILE_HEIGHT, cam_x, cam_y, sw, sh, 120.0) {
            continue;
        }
        let draw_state = draw_decision.state;
        let tint = vxl_body_tint(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx, pos.ry),
            entity.category,
            state
                .rules()
                .map_or(0, |rules| rules.general.extra_unit_light),
            state
                .rules()
                .map_or(0, |rules| rules.general.extra_aircraft_light),
        );
        let palette_light = crate::app::presentation::lighting::body_palette_light(
            state.match_state.match_presentation.lighting.grid(),
            &sim.session.lighting,
            (pos.rx, pos.ry),
            entity.category,
            state.rules().map_or(0, |r| r.general.extra_unit_light),
            state.rules().map_or(0, |r| r.general.extra_infantry_light),
        );
        let center_x: f32 = sx;
        let center_y: f32 = sy;

        // Docked miners render in front of the refinery building they're on.
        // The pad cell is inside the building footprint (north of the south edge),
        // so without adjustment the miner's depth_y is above the building's
        // foundation bottom and it draws behind. We offset depth_y in screen-space
        // (not depth-space) so the correction scales naturally with map size.
        // One full tile height pushes the sort point past the foundation bottom.
        // The condition is the miner standing in a building's cell — the pad,
        // from rolling in through the turn and the unload until it leaves —
        // not a dock phase: the War Miner docks through its Enter and Unload
        // missions. VERA-internal draw-order heuristic, not a port of the
        // native display sort.
        let dock_depth_y_offset: f32 = if entity.miner.is_some()
            && sim
                .substrate
                .occupancy
                .first_building_on_layer(
                    pos.rx,
                    pos.ry,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                )
                .is_some()
        {
            TILE_HEIGHT
        } else {
            0.0
        };

        let anim_frame: u32 = entity.voxel_animation.map(|a| a.frame).unwrap_or(0);
        // `UnitClass::DrawVoxelBody @ 0x0073B4DA..0x0073B50E`: the turret
        // takes the body's HVA frame, or while that is 0, `+0x148` modulo
        // the turret HVA's frame count (the Gattling's spinning barrels).
        let turret_frame = if anim_frame != 0 || entity.turret_anim_frame == 0 {
            anim_frame
        } else {
            atlas
                .frame_counts
                .get(&(type_str.to_string(), VxlLayer::Turret))
                .filter(|&&frames| frames > 1)
                .map_or(0, |&frames| {
                    entity.turret_anim_frame.rem_euclid(frames as i32) as u32
                })
        };

        // Chrono teleport doesn't tint the unit — the visual effect is the
        // WarpOut animation overlay; the unit itself stays fully opaque.
        let alpha: f32 = 1.0;
        // 0x73B140's split is inside the same native parent draw call.
        // Ground units, including those below bridges, keep LayerClass order.
        let collect_ground = band == EntityDrawBand::Ground;
        let mut ground_pieces = Vec::new();
        let (target_instances, target_instance_pages) = match band {
            EntityDrawBand::Top => (&mut *top_instances, &mut *top_instance_pages),
            EntityDrawBand::Ground => (&mut *instances, &mut *instance_pages),
        };

        if let Some(turret_facing) = entity
            .barrel_facing
            .as_ref()
            .map(|f| f.current(sim.session.binary_frame))
        {
            // Turret unit: emit body, turret, and barrel as separate sprites.
            emit_turret_unit_sprites(
                target_instances,
                target_instance_pages,
                atlas,
                art_reg,
                entity,
                type_str,
                entity.facing,
                turret_facing,
                hc,
                center_x,
                center_y,
                state,
                interp_z,
                tint,
                palette_light,
                alpha,
                draw_state,
                anim_frame,
                turret_frame,
                dock_depth_y_offset,
                slope_state,
                transition_instances,
                band,
                collect_ground,
                &mut ground_pieces,
            );
        } else {
            // Non-turret unit: single composite sprite.
            let key: UnitSpriteKey = UnitSpriteKey {
                type_id: type_str.to_string(),
                facing: canonical_unit_facing(entity.facing),
                layer: VxlLayer::Composite,
                frame: anim_frame,
                slope_type: stable_slope_for_key(slope_state),
            };
            if let Some(tilt) = crash_body_tilt(entity, band) {
                emit_crash_pose_sprite(
                    state,
                    target_instances,
                    target_instance_pages,
                    entity,
                    &key,
                    tilt,
                    [center_x, center_y],
                    interp_z,
                    tint,
                    palette_light,
                    draw_state,
                );
            } else if let Some((entry, texture_source)) =
                unit_entry_for_slope_state(state, atlas, &key, slope_state)
            {
                let depth_y: f32 = sy + entry.offset_y + entry.pixel_size[1] + dock_depth_y_offset;
                let depth: f32 = body_sort_depth(state, entity, band, depth_y, interp_z);
                let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true) as f32;
                let native_shadow = matches!(texture_source, UnitTextureSource::Stable(_))
                    && prepare_unit_shadow(
                        state,
                        atlas,
                        entity,
                        type_str,
                        draw_state,
                        slope_state,
                        band,
                        [(entry, [0.0, 0.0])],
                    );
                if !native_shadow {
                    emit_unit_shadow_sprite(
                        native_shadow,
                        voxel_adjust,
                        target_instances,
                        target_instance_pages,
                        atlas,
                        entity,
                        type_str,
                        center_x,
                        center_y,
                        depth,
                        draw_state,
                        slope_state,
                        transition_instances,
                        band,
                        collect_ground,
                        &mut ground_pieces,
                    );
                }
                let (composite_rect, split) = composite_depth_rect(
                    [(entry, [center_x, center_y])],
                    super::foot_depth::unit_bridge_split(state, entity, true),
                );
                let body_draw_state = composite_draw_state(state, draw_state, split);
                let sprite = SpriteInstance {
                    position: [center_x + entry.offset_x, center_y + entry.offset_y],
                    size: entry.pixel_size,
                    uv_origin: entry.uv_origin,
                    uv_size: entry.uv_size,
                    depth,
                    tint,
                    palette_light,
                    alpha,
                    draw_state: body_draw_state,
                    z_adjust: voxel_adjust,
                    z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
                    zshape_origin: composite_rect,
                    ..Default::default()
                };
                push_unit_sprite(
                    target_instances,
                    target_instance_pages,
                    transition_instances,
                    texture_source,
                    sprite,
                    collect_ground,
                    &mut ground_pieces,
                );
                if native_shadow {
                    emit_unit_shadow_sprite(
                        native_shadow,
                        voxel_adjust,
                        target_instances,
                        target_instance_pages,
                        atlas,
                        entity,
                        type_str,
                        center_x,
                        center_y,
                        depth,
                        draw_state,
                        slope_state,
                        transition_instances,
                        band,
                        collect_ground,
                        &mut ground_pieces,
                    );
                }
            }
        }

        // Emit harvest overlay (oregath.shp) if the miner is actively harvesting.
        // OREGATH is an SHP sprite from sprite_atlas, but remains an owned piece
        // of its harvester's Ground slot so atlas identity cannot re-sort it.
        if let Some(ref ho) = entity.harvest_overlay {
            if ho.visible {
                if let Some((page, instance)) = emit_harvest_overlay(
                    state,
                    entity,
                    entity.facing,
                    ho,
                    center_x,
                    center_y,
                    pos.z,
                    tint,
                    palette_light.brightness(),
                    draw_state,
                ) {
                    if collect_ground {
                        ground_pieces.push(GroundPieceInstance {
                            target: GroundTexture::ShpPage(page),
                            render_z: RenderZPolicy::ReadOnly,
                            instance,
                        });
                    } else if let Some(bucket) = shp_paged.get_mut(page) {
                        bucket.push(instance);
                    }
                }
            }
        }

        if collect_ground && !ground_pieces.is_empty() {
            let parent = ground_order.object_draw(
                entity.stable_id(),
                crate::render::tactical_draw_plan::SpriteEncoding::Voxel,
            );
            if let Some(parent) = parent {
                ground_objects.push(PlannedGroundObjectInstance::object(parent, ground_pieces));
            }
        }
    }
}

/// The page index a Top-band instance carries when it was drawn on this
/// frame's crash-pose page (`render::unit_pose_cache`) instead of the atlas.
pub(crate) const POSE_PAGE: usize = usize::MAX;

/// A crashing Fly body's roll and pitch (`TechnoClass+0x328`/`+0x32C`), which
/// Fly Draw_Matrix applies while the body is airborne (`0x004CF6A3`): every
/// airborne Fly body is in the Top band (`In_Which_Layer @ 0x004CFCF0`).
/// Other locomotors' Draw_Matrix keep their own crash arms.
fn crash_body_tilt(
    entity: &crate::sim::game_entity::GameEntity,
    band: EntityDrawBand,
) -> Option<[f32; 2]> {
    let fly = entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.kind == crate::rules::locomotor_type::LocomotorKind::Fly
    });
    if !entity.crashing || !fly || band != EntityDrawBand::Top {
        return None;
    }
    let rocking = entity.rocking.as_ref()?;
    Some([
        rocking.angle_sideways.to_num::<f32>(),
        rocking.angle_forwards.to_num::<f32>(),
    ])
}

/// One crashing body drawn at its current pose on this frame's pose page. It
/// joins the Top band in emission order like an atlas body; aircraft cast no
/// VERA shadow (recorded on the draw passes).
#[allow(clippy::too_many_arguments)]
fn emit_crash_pose_sprite(
    state: &AppState,
    instances: &mut Vec<SpriteInstance>,
    instance_pages: &mut Vec<usize>,
    entity: &crate::sim::game_entity::GameEntity,
    key: &UnitSpriteKey,
    tilt: [f32; 2],
    [center_x, center_y]: [f32; 2],
    z: u8,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    draw_state: DrawState,
) {
    let Some(assets) = state.process_assets.manager() else {
        return;
    };
    let Some(entry) = state.renderer.vxl_pose_frame_cache.borrow_mut().render(
        &state.renderer.gpu,
        assets,
        state.rules(),
        state.rules().map(|rules| &rules.art_registry),
        key,
        tilt,
    ) else {
        return;
    };
    let depth_y = center_y + entry.offset_y + entry.pixel_size[1];
    let depth = body_sort_depth(state, entity, EntityDrawBand::Top, depth_y, z);
    let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true) as f32;
    let (composite_rect, split) = composite_depth_rect([(entry, [center_x, center_y])], false);
    instances.push(SpriteInstance {
        position: [center_x + entry.offset_x, center_y + entry.offset_y],
        size: entry.pixel_size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint,
        palette_light,
        alpha: 1.0,
        draw_state: composite_draw_state(state, draw_state, split),
        z_adjust: voxel_adjust,
        z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
        zshape_origin: composite_rect,
    });
    instance_pages.push(POSE_PAGE);
}

/// Compute the screen-space offset for a turret pivot point from art.ini TurretOffset.
///
/// Delegates to the voxel renderer, which walks the offset through the same
/// camera/slope/body-facing chain the hull was drawn with. That matters on ramps:
/// the pivot is a point on the tilted hull, so it has to rise and fall with it
/// rather than being nudged by a fixed screen-space vector.
fn turret_screen_offset(
    turret_offset: i32,
    body_facing: u8,
    slope_state: UnitRenderSlopeState,
) -> (f32, f32) {
    crate::render::vxl_raster::turret_pivot_screen_offset_for_slope_state(
        turret_offset,
        body_facing,
        stable_slope_for_key(slope_state),
        slope_blend_for_render_state(slope_state),
        crate::render::vxl_raster::VxlRenderParams::default().scale,
    )
}

/// Return the native local draw order for a Unit's independently rendered
/// turret and barrel pieces.
///
/// gamemd-derived: active YR `UnitClass__DrawVoxelBody @ 0x0073B470`, selector
/// instructions `0x0073BC5B..0x0073BDAB`.
fn native_turret_barrel_order<T: Copy>(turret_facing: u16, turret: T, barrel: T) -> [T; 2] {
    let facing_high = (turret_facing >> 8) as u8;
    if (NATIVE_TURRET_FIRST_START..NATIVE_TURRET_FIRST_END_EXCLUSIVE).contains(&facing_high) {
        [turret, barrel]
    } else {
        [barrel, turret]
    }
}

/// Look up a unit sprite from the atlas with cascading fallbacks:
/// 1. Try the exact key (slope + frame).
/// 2. Fall back to frame 0 if the requested frame doesn't exist (mismatched HVA counts).
/// 3. Fall back to slope_type=0 if the tilted sprite isn't in the atlas yet
///    (unit just moved onto a ramp and the atlas hasn't rebuilt).
/// This prevents units from disappearing during atlas rebuilds.
fn atlas_get_with_frame_fallback<'a>(
    atlas: &'a crate::render::unit_atlas::UnitAtlas,
    key: &UnitSpriteKey,
) -> Option<&'a crate::render::unit_atlas::UnitSpriteEntry> {
    atlas.get(key).or_else(|| {
        // Fallback 1: try frame 0 with same slope.
        if key.frame > 0 {
            let fallback = UnitSpriteKey {
                frame: 0,
                ..key.clone()
            };
            if let Some(entry) = atlas.get(&fallback) {
                return Some(entry);
            }
        }
        // Fallback 2: try slope_type=0 (flat) with original frame.
        if key.slope_type != 0 {
            let flat_key = UnitSpriteKey {
                slope_type: 0,
                ..key.clone()
            };
            if let Some(entry) = atlas.get(&flat_key) {
                return Some(entry);
            }
            // Fallback 3: slope_type=0 + frame 0.
            if key.frame > 0 {
                let flat_frame0 = UnitSpriteKey {
                    slope_type: 0,
                    frame: 0,
                    ..key.clone()
                };
                return atlas.get(&flat_frame0);
            }
        }
        None
    })
}

fn stable_slope_for_key(slope_state: UnitRenderSlopeState) -> u8 {
    match slope_state {
        UnitRenderSlopeState::Stable(slope) => slope,
        UnitRenderSlopeState::Transition { to_slope, .. } => to_slope,
    }
}

fn slope_blend_for_render_state(
    slope_state: UnitRenderSlopeState,
) -> Option<crate::render::vxl_raster::VxlSlopeBlend> {
    match slope_state {
        UnitRenderSlopeState::Stable(_) => None,
        UnitRenderSlopeState::Transition {
            from_slope,
            to_slope,
            phase_num,
            phase_den,
        } => Some(crate::render::vxl_raster::VxlSlopeBlend {
            from_slope,
            to_slope,
            phase_num,
            phase_den,
        }),
    }
}

fn transition_key_for_unit(
    key: &UnitSpriteKey,
    slope_state: UnitRenderSlopeState,
) -> Option<TransitionUnitSpriteKey> {
    match slope_state {
        UnitRenderSlopeState::Stable(_) => None,
        UnitRenderSlopeState::Transition {
            from_slope,
            to_slope,
            phase_num,
            phase_den,
        } => Some(TransitionUnitSpriteKey {
            type_id: key.type_id.clone(),
            facing: key.facing,
            layer: key.layer,
            frame: key.frame,
            from_slope,
            to_slope,
            phase_num,
            phase_den,
        }),
    }
}

fn unit_entry_for_slope_state(
    state: &AppState,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    key: &UnitSpriteKey,
    slope_state: UnitRenderSlopeState,
) -> Option<(UnitSpriteEntry, UnitTextureSource)> {
    if let Some(transition_key) = transition_key_for_unit(key, slope_state) {
        if let Some(asset_manager) = state.process_assets.manager() {
            if let Some(TransitionUnitSpriteEntry { page, entry }) = state
                .renderer
                .vxl_slope_transition_cache
                .borrow_mut()
                .get_or_render(
                    &state.renderer.gpu,
                    &state.renderer.batch_renderer,
                    asset_manager,
                    state.rules(),
                    state.rules().map(|rules| &rules.art_registry),
                    transition_key,
                )
            {
                return Some((entry, UnitTextureSource::Transition(page)));
            }
        }
    }

    atlas_get_with_frame_fallback(atlas, key)
        .copied()
        .map(|entry| (entry, UnitTextureSource::Stable(entry.page)))
}

fn push_transition_sprite(
    transition_instances: &mut Vec<Vec<SpriteInstance>>,
    page: usize,
    sprite: SpriteInstance,
) {
    if transition_instances.len() <= page {
        transition_instances.resize_with(page + 1, Vec::new);
    }
    transition_instances[page].push(sprite);
}

/// First-fill mask owner for the supported ordinary flat ground path. The
/// input entries are precisely the parts selected for this parent's draw;
/// cache hits skip their pixel work and retain the first completed mask.
#[allow(clippy::too_many_arguments)]
fn prepare_unit_shadow(
    state: &AppState,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    draw_state: DrawState,
    slope: UnitRenderSlopeState,
    band: EntityDrawBand,
    parts: impl IntoIterator<Item = (crate::render::unit_atlas::UnitSpriteEntry, [f32; 2])>,
) -> bool {
    if !native_shadow_caller_eligible(entity, draw_state, slope, band) {
        return false;
    }
    let key = UnitSpriteKey {
        type_id: type_id.to_owned(),
        facing: canonical_unit_facing(entity.facing),
        layer: VxlLayer::Shadow,
        frame: 0,
        slope_type: 0,
    };
    atlas.prepare_native_shadow(&state.renderer.gpu.queue, &key, parts)
}

fn native_shadow_caller_eligible(
    entity: &crate::sim::game_entity::GameEntity,
    draw_state: DrawState,
    slope: UnitRenderSlopeState,
    band: EntityDrawBand,
) -> bool {
    if entity.category != EntityCategory::Unit
        || band != EntityDrawBand::Ground
        || !entity
            .locomotor
            .as_ref()
            .is_some_and(|l| l.active_kind() == crate::rules::locomotor_type::LocomotorKind::Drive)
        || !matches!(slope, UnitRenderSlopeState::Stable(0))
        || draw_state.fx_flags & crate::render::draw_state::FX_CLOAK != 0
    {
        return false;
    }
    true
}

fn shadow_lookup_key(mut key: UnitSpriteKey, native_shadow: bool) -> UnitSpriteKey {
    if !native_shadow {
        key.frame = crate::render::unit_atlas::LEGACY_SHADOW_FRAME;
    }
    key
}

/// The hull's ground shadow as one more piece of the unit's draw.
///
/// Original UnitClass::DrawVoxelBody ends with shadow call 73C5C4 after the
/// composed body. Supported flat native stencils follow that order and use
/// its first-fill body mask. Unsupported geometry retains the legacy order.
/// Cloak states suppress this shadow; aircraft and building VXL paths remain
/// separate. The voxel pipeline reads depth and uses FX_SHADOW alpha darkening;
/// packed RGB565 half and full native cache lifecycle remain outside this fix.
#[allow(clippy::too_many_arguments)]
fn emit_unit_shadow_sprite(
    native_shadow: bool,
    voxel_adjust: f32,
    stable_instances: &mut Vec<SpriteInstance>,
    stable_instance_pages: &mut Vec<usize>,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    center_x: f32,
    center_y: f32,
    depth: f32,
    draw_state: DrawState,
    slope_state: UnitRenderSlopeState,
    transition_instances: &mut Vec<Vec<SpriteInstance>>,
    band: EntityDrawBand,
    collect_ground: bool,
    ground_pieces: &mut Vec<GroundPieceInstance>,
) {
    if entity.category != EntityCategory::Unit || band != EntityDrawBand::Ground {
        return;
    }
    if draw_state.fx_flags & crate::render::draw_state::FX_CLOAK != 0 {
        return;
    }
    let key = UnitSpriteKey {
        type_id: type_id.to_string(),
        facing: canonical_unit_facing(entity.facing),
        layer: VxlLayer::Shadow,
        frame: 0,
        slope_type: stable_slope_for_key(slope_state),
    };
    let lookup_key = shadow_lookup_key(key.clone(), native_shadow);
    let Some(entry) = atlas.get(&lookup_key).or_else(|| atlas.get(&key)).copied() else {
        return;
    };
    let mut shadow_state: DrawState = draw_state;
    shadow_state.fx_flags |= FX_SHADOW;
    let sprite = SpriteInstance {
        position: [center_x + entry.offset_x, center_y + entry.offset_y],
        size: entry.pixel_size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: lighting::DEFAULT_TINT,
        alpha: 1.0,
        draw_state: shadow_state,
        // Cached 0x707480 and uncached 0x707280 shadow blits both consume
        // Foot GetZAdjustment, just like the body.
        z_adjust: voxel_adjust,
        z_gradient: VOXEL_Z_GRADIENT,
        ..Default::default()
    };
    push_unit_sprite(
        stable_instances,
        stable_instance_pages,
        transition_instances,
        UnitTextureSource::Stable(entry.page),
        sprite,
        collect_ground,
        ground_pieces,
    );
}

fn push_unit_sprite(
    stable_instances: &mut Vec<SpriteInstance>,
    stable_instance_pages: &mut Vec<usize>,
    transition_instances: &mut Vec<Vec<SpriteInstance>>,
    texture_source: UnitTextureSource,
    sprite: SpriteInstance,
    collect_ground: bool,
    ground_pieces: &mut Vec<GroundPieceInstance>,
) {
    if collect_ground {
        let target = match texture_source {
            UnitTextureSource::Transition(page) => GroundTexture::UnitTransitionPage(page),
            UnitTextureSource::Stable(page) => GroundTexture::UnitAtlasPage(page),
        };
        ground_pieces.push(GroundPieceInstance {
            target,
            render_z: RenderZPolicy::ReadOnly,
            instance: sprite,
        });
        return;
    }
    match texture_source {
        UnitTextureSource::Transition(page) => {
            push_transition_sprite(transition_instances, page, sprite);
        }
        UnitTextureSource::Stable(page) => {
            stable_instances.push(sprite);
            stable_instance_pages.push(page);
        }
    }
}

/// Select the rectangle consumed by Unit's 0x73B140 split. Atlas padding is
/// storage, not the native draw rectangle. Use only the requested parts at
/// their actual facings; 0x70755D unions them in body/turret/barrel draw order.
/// Unsplit draws retain the existing rectangle until that wider path is audited.
fn composite_depth_rect(
    parts: impl IntoIterator<Item = (UnitSpriteEntry, [f32; 2])>,
    wants_split: bool,
) -> ([f32; 2], bool) {
    let mut top = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    let mut native = None;
    let mut all_native = true;
    for (entry, anchor) in parts {
        top = top.min(anchor[1] + entry.offset_y);
        bottom = bottom.max(anchor[1] + entry.offset_y + entry.pixel_size[1]);
        if let Some(mut bounds) = entry.native_draw_bounds {
            // Draw anchors are integer pixels in the native cached blit.
            bounds[0] += anchor[0] as i32;
            bounds[1] += anchor[1] as i32;
            crate::render::vxl_raster::union_native_voxel_draw_bounds(&mut native, bounds);
        } else {
            all_native = false;
        }
    }
    if wants_split && all_native {
        if let Some([_, y, _, height]) = native {
            return ([y as f32, height as f32], true);
        }
    }
    if bottom > top {
        ([top, bottom - top], false)
    } else {
        ([0.0, 0.0], false)
    }
}

fn composite_draw_state(state: &AppState, mut draw_state: DrawState, split: bool) -> DrawState {
    if split {
        // Only voxel body draws use this otherwise unused parameter. Shadows
        // keep their own unsplit state and depth. The shader needs the actual
        // tactical scissor to seed each clipped native blit independently.
        let (_, _, _, height) = crate::app::input::camera::tactical_viewport_px(state);
        draw_state.fx_params[3] = height as f32 / state.match_state.input.zoom_level;
    }
    draw_state
}

/// Emit body + turret + barrel sprites for a turret-equipped voxel unit.
///
/// Body is drawn at body facing. Turret + barrel are drawn at turret facing,
/// shifted by the art.ini TurretOffset (rotated by body facing) so the turret
/// sits on its correct pivot point on the hull.
fn emit_turret_unit_sprites(
    instances: &mut Vec<SpriteInstance>,
    instance_pages: &mut Vec<usize>,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    art_reg: Option<&crate::rules::art_data::ArtRegistry>,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    body_facing: u8,
    turret_facing: u16,
    _hc: HouseColorIndex,
    center_x: f32,
    center_y: f32,
    state: &AppState,
    z: u8,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    alpha: f32,
    draw_state: DrawState,
    anim_frame: u32,
    turret_frame: u32,
    dock_depth_y_offset: f32,
    slope_state: UnitRenderSlopeState,
    transition_instances: &mut Vec<Vec<SpriteInstance>>,
    band: EntityDrawBand,
    collect_ground: bool,
    ground_pieces: &mut Vec<GroundPieceInstance>,
) {
    let slope_type = stable_slope_for_key(slope_state);
    let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true) as f32;
    let body_key = UnitSpriteKey {
        type_id: type_id.to_string(),
        facing: canonical_unit_facing(body_facing),
        layer: VxlLayer::Body,
        frame: anim_frame,
        slope_type,
    };
    let turret_key = UnitSpriteKey {
        type_id: type_id.to_string(),
        facing: canonical_turret_facing(turret_facing),
        layer: VxlLayer::Turret,
        frame: turret_frame,
        slope_type,
    };
    let barrel_key = UnitSpriteKey {
        type_id: type_id.to_string(),
        facing: canonical_turret_facing(turret_facing),
        layer: VxlLayer::Barrel,
        frame: anim_frame,
        slope_type,
    };

    // Look up TurretOffset from art.ini and compute screen-space shift.
    let art_offset: i32 = art_reg
        .and_then(|a| a.get(type_id))
        .map(|e| e.turret_offset)
        .unwrap_or(0);
    // Use the same stable or quaternion-SLERP slope matrix as every hull,
    // turret, and barrel raster layer in this presentation phase.
    let (tur_ox, tur_oy) = turret_screen_offset(art_offset, body_facing, slope_state);

    // All layers of a turreted unit share one depth so insertion order
    // (body, then turret/barrel) controls visual stacking via stable sort.
    // Per-layer depth derived from each sprite's bounding box caused tie-break
    // collisions where body could sort over turret at certain facings.
    let body_entry_opt = unit_entry_for_slope_state(state, atlas, &body_key, slope_state);
    let entity_depth_y: f32 = match body_entry_opt {
        Some((e, _)) => center_y + e.offset_y + e.pixel_size[1] + dock_depth_y_offset,
        None => center_y + dock_depth_y_offset,
    };
    let entity_depth: f32 = body_sort_depth(state, entity, band, entity_depth_y, z);

    // Emit body first (always). Uses frame fallback for mismatched HVA counts.
    // Natively hull, turret and barrel are composited off-screen and blitted
    // as ONE rect (`UnitClass vtable+0x55C = 0x0073B140`), so all three share
    // the entry-2 seed of the composite rect: its top and height ride
    // `zshape_origin` (the voxel shader's `z_rect`).
    let turret_layers: Vec<_> = native_turret_barrel_order(turret_facing, &turret_key, &barrel_key)
        .into_iter()
        .filter_map(|key| unit_entry_for_slope_state(state, atlas, key, slope_state))
        .collect();
    let native_shadow = body_entry_opt
        .is_some_and(|(_, source)| matches!(source, UnitTextureSource::Stable(_)))
        && turret_layers
            .iter()
            .all(|(_, source)| matches!(source, UnitTextureSource::Stable(_)))
        && prepare_unit_shadow(
            state,
            atlas,
            entity,
            type_id,
            draw_state,
            slope_state,
            band,
            body_entry_opt
                .into_iter()
                .map(|(entry, _)| (entry, [0.0, 0.0]))
                .chain(
                    turret_layers
                        .iter()
                        .map(|(entry, _)| (*entry, [tur_ox, tur_oy])),
                ),
        );
    if !native_shadow {
        emit_unit_shadow_sprite(
            native_shadow,
            voxel_adjust,
            instances,
            instance_pages,
            atlas,
            entity,
            type_id,
            center_x,
            center_y,
            entity_depth,
            draw_state,
            slope_state,
            transition_instances,
            band,
            collect_ground,
            ground_pieces,
        );
    }
    let (composite_rect, split) = composite_depth_rect(
        body_entry_opt
            .into_iter()
            .map(|(entry, _)| (entry, [center_x, center_y]))
            .chain(
                turret_layers
                    .iter()
                    .map(|(entry, _)| (*entry, [center_x + tur_ox, center_y + tur_oy])),
            ),
        super::foot_depth::unit_bridge_split(state, entity, true),
    );
    let body_draw_state = composite_draw_state(state, draw_state, split);
    if let Some((entry, texture_source)) = body_entry_opt {
        let sprite = SpriteInstance {
            position: [center_x + entry.offset_x, center_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth: entity_depth,
            tint,
            palette_light,
            alpha,
            draw_state: body_draw_state,
            z_adjust: voxel_adjust,
            z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
            zshape_origin: composite_rect,
        };
        push_unit_sprite(
            instances,
            instance_pages,
            transition_instances,
            texture_source,
            sprite,
            collect_ground,
            ground_pieces,
        );
    }

    for (entry, texture_source) in turret_layers {
        let sprite = SpriteInstance {
            position: [
                center_x + entry.offset_x + tur_ox,
                center_y + entry.offset_y + tur_oy,
            ],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth: entity_depth,
            tint,
            palette_light,
            alpha,
            draw_state: body_draw_state,
            z_adjust: voxel_adjust,
            z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
            zshape_origin: composite_rect,
        };
        push_unit_sprite(
            instances,
            instance_pages,
            transition_instances,
            texture_source,
            sprite,
            collect_ground,
            ground_pieces,
        );
    }
    if native_shadow {
        emit_unit_shadow_sprite(
            native_shadow,
            voxel_adjust,
            instances,
            instance_pages,
            atlas,
            entity,
            type_id,
            center_x,
            center_y,
            entity_depth,
            draw_state,
            slope_state,
            transition_instances,
            band,
            collect_ground,
            ground_pieces,
        );
    }
}

/// Arm offset in leptons for the oregath harvest overlay. The overlay is drawn offset
/// from the unit center by this distance, rotated by the body facing, so the harvest
/// arm visually tracks the correct side of the harvester.
const OREGATH_ARM_OFFSET_LEPTONS: f32 = 30.0;

/// Emit the oregath.shp harvest overlay sprite for a mining harvester.
///
/// The overlay uses the sprite atlas (keyed as "OREGATH") with 15 frames × 8 facings.
/// SHP frame index = facing_index * 15 + anim_frame.
///
/// The draw position is offset from the unit center by 30 leptons rotated by body
/// facing (verified from binary at 0x0073D12F–0x0073D1D6). This places the overlay
/// at the harvest arm position rather than dead center on the unit.
fn emit_harvest_overlay(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
    body_facing: u8,
    overlay: &HarvestOverlay,
    center_x: f32,
    center_y: f32,
    z: u8,
    tint: [f32; 3],
    brightness: i32,
    draw_state: DrawState,
) -> Option<(usize, SpriteInstance)> {
    let sprite_atlas = match &state.match_state.match_presentation.sprite_atlas {
        Some(a) => a,
        None => return None,
    };
    // Map body facing (0-255) to counter-clockwise SHP frame index (0..7).
    // +32 offset for isometric rotation (SHP frame 0 = screen-N, not cell-N).
    let facing_index: u16 = (8 - (body_facing.wrapping_add(32) / 32) as u16) % 8;
    let shp_frame: u16 = facing_index * 15 + overlay.frame;
    let key = ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "OREGATH".to_string(),
        facing: 0,
        frame: shp_frame,
        house_color: HouseColorIndex::default(),
    };
    let entry = sprite_atlas.get(&key)?;
    let page = entry.page as usize;
    // Compute arm offset: rotate 30 leptons by body facing, then convert to screen.
    // Same sin/cos + isometric transform used by turret_screen_offset.
    let (arm_sx, arm_sy) = harvest_arm_screen_offset(body_facing);
    let draw_x: f32 = center_x + arm_sx;
    let draw_y: f32 = center_y + arm_sy;
    let depth_y: f32 = draw_y + entry.offset_y + entry.pixel_size[1];
    let depth: f32 = compute_sprite_depth(state, depth_y, z);
    Some((
        page,
        SpriteInstance {
            position: [draw_x + entry.offset_x, draw_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth,
            tint,
            // Unit DrawExtras 0073D276 selects global ANIM Convert, not the
            // unit ColorScheme. Ordinary scalar is top + ExtraUnitLight.
            // Its bridge/special-cell brightness branches remain unresolved.
            palette_light: crate::render::palette_light::PaletteLight::plain(53, brightness),
            alpha: 1.0,
            draw_state,
            // An SHP draw of the harvester (`TechnoClass_DrawSHP`, a7 - 2).
            z_adjust: super::foot_depth::unit_z_adjust(state, entity, false)
                .wrapping_add(SHP_DRAW_Z_ADJUST_PX) as f32,
            z_gradient: pack_z_gradient(ZGradient::Vertical, false),
            ..Default::default()
        },
    ))
}

/// Native VXL blits walk entry 2 with the Foot adjustment supplied by the
/// shared production adapter. Hull, turret and barrel retain one composite seed.
const VOXEL_Z_GRADIENT: u32 = pack_z_gradient(ZGradient::Vertical, false);

/// Convert the oregath arm offset (30 leptons) into isometric screen pixels.
///
/// Mirrors the binary's logic at 0x0073D12F–0x0073D1D6:
///   world_x = sin(angle) * 30 + base.X
///   world_y = base.Y - cos(angle) * 30
/// Then isometric projection converts leptons to screen pixels.
fn harvest_arm_screen_offset(body_facing: u8) -> (f32, f32) {
    let angle: f32 = std::f32::consts::TAU * (body_facing as f32 / 256.0);
    let (sin, cos) = angle.sin_cos();
    // World-space offset in leptons, matching the binary's sin/cos convention.
    let lx: f32 = OREGATH_ARM_OFFSET_LEPTONS * sin;
    let ly: f32 = -OREGATH_ARM_OFFSET_LEPTONS * cos;
    // Leptons → tile fractions (256 leptons per cell).
    let cx: f32 = lx / 256.0;
    let cy: f32 = ly / 256.0;
    // Isometric projection: tile offset → screen pixels (60×30 cell).
    let screen_x: f32 = (cx - cy) * 60.0 / 2.0;
    let screen_y: f32 = (cx + cy) * 30.0 / 2.0;
    (screen_x, screen_y)
}

/// Map a HouseColorIndex to the per-house ramp row index in PaletteSet's
/// house_ramp_tex. Row 0 is the no-remap fallback (mirrors the theater
/// palette's [16, 32) range); civilian/neutral units (`NO_REMAP`) map to
/// row 0. Real players occupy rows 1..N (the +1 reserves row 0).
pub(super) fn house_color_to_remap_row(hc: HouseColorIndex) -> u32 {
    if hc == house_colors::NO_REMAP {
        0
    } else {
        (hc.0 as u32) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::InternedId;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::spawn_manager::{
        SpawnManagerMode, SpawnManagerState, SpawnSlot, SpawnSlotState, SpawnTimer,
    };
    use crate::sim::world::Simulation;

    #[test]
    fn vehicle_shadow_active_locomotor_selects_native_or_legacy_companion() {
        let mut entity = GameEntity::test_default(1, "GENERATED", "Americans", 0, 0);
        entity.category = EntityCategory::Unit;
        let key = UnitSpriteKey {
            type_id: "GENERATED".into(),
            facing: 0,
            layer: VxlLayer::Shadow,
            frame: 0,
            slope_type: 0,
        };
        for (kind, slope, expected_frame) in [
            (LocomotorKind::Drive, 0, 0),
            (
                LocomotorKind::Teleport,
                0,
                crate::render::unit_atlas::LEGACY_SHADOW_FRAME,
            ),
            (
                LocomotorKind::Hover,
                0,
                crate::render::unit_atlas::LEGACY_SHADOW_FRAME,
            ),
            (
                LocomotorKind::Drive,
                1,
                crate::render::unit_atlas::LEGACY_SHADOW_FRAME,
            ),
        ] {
            entity.locomotor = Some(LocomotorState::for_test_kind(kind));
            let eligible = native_shadow_caller_eligible(
                &entity,
                DrawState::default(),
                UnitRenderSlopeState::Stable(slope),
                EntityDrawBand::Ground,
            );
            assert_eq!(
                shadow_lookup_key(key.clone(), eligible).frame,
                expected_frame
            );
        }
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        let mut cloak = DrawState::default();
        cloak.fx_flags |= crate::render::draw_state::FX_CLOAK;
        assert!(!native_shadow_caller_eligible(
            &entity,
            cloak,
            UnitRenderSlopeState::Stable(0),
            EntityDrawBand::Ground
        ));
    }

    #[test]
    fn unit_vxl_piece_order_keeps_native_boundary_values() {
        for (high, expected) in [
            (0u8, ["barrel", "turret"]),
            (31, ["barrel", "turret"]),
            (32, ["turret", "barrel"]),
            (159, ["turret", "barrel"]),
            (160, ["barrel", "turret"]),
            (255, ["barrel", "turret"]),
        ] {
            assert_eq!(
                native_turret_barrel_order(u16::from(high) << 8, "turret", "barrel"),
                expected,
                "facing high byte {high}"
            );
        }
    }

    #[test]
    fn unit_vxl_piece_order_matches_native_selector_for_every_facing() {
        for facing in 0..=u16::MAX {
            let native_quadrant = ((((u32::from(facing) >> 13) + 1) >> 1) & 3) as u8;
            let expected = if matches!(native_quadrant, 1 | 2) {
                ["turret", "barrel"]
            } else {
                ["barrel", "turret"]
            };
            assert_eq!(
                native_turret_barrel_order(facing, "turret", "barrel"),
                expected,
                "facing {facing:#06x}, native quadrant {native_quadrant}"
            );
        }
    }

    #[test]
    fn drive_ship_slope_unit_extraction_uses_last_processed_frame_without_mutation() {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "DRIVE", "Americans", 0, 0);
        entity.owner = sim.intern("Americans");
        entity.type_ref = sim.intern("DRIVE");
        entity.locomotor = Some(LocomotorState::for_test_kind_at_frame(
            LocomotorKind::Drive,
            39,
        ));
        let slope = entity
            .locomotor
            .as_mut()
            .unwrap()
            .active_slope_transition_mut()
            .unwrap();
        slope.snap(3, 39);
        slope.sample_process_entry(8, 40);
        sim.substrate.entities.insert(entity);

        assert_eq!(display_binary_frame_for_committed_session(0), u32::MAX);

        for (committed_binary_frame, expected) in [
            (
                41,
                UnitRenderSlopeState::Transition {
                    from_slope: 3,
                    to_slope: 8,
                    phase_num: 0,
                    phase_den: SLOPE_TRANSITION_FRAMES,
                },
            ),
            (
                42,
                UnitRenderSlopeState::Transition {
                    from_slope: 3,
                    to_slope: 8,
                    phase_num: 1,
                    phase_den: SLOPE_TRANSITION_FRAMES,
                },
            ),
            (
                43,
                UnitRenderSlopeState::Transition {
                    from_slope: 3,
                    to_slope: 8,
                    phase_num: 2,
                    phase_den: SLOPE_TRANSITION_FRAMES,
                },
            ),
            (44, UnitRenderSlopeState::Stable(8)),
        ] {
            sim.session.binary_frame = committed_binary_frame;
            let before_hash = sim.state_hash();
            let display_binary_frame =
                display_binary_frame_for_committed_session(sim.session.binary_frame);
            assert_eq!(display_binary_frame, committed_binary_frame.wrapping_sub(1));
            let entity = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                locomotor_render_slope_state(entity, display_binary_frame),
                Some(expected)
            );
            assert_eq!(
                locomotor_render_slope_state(entity, display_binary_frame),
                Some(expected),
                "repeated presentation extraction for one committed frame is stable"
            );
            assert_eq!(
                display_binary_frame_for_committed_session(sim.session.binary_frame),
                display_binary_frame,
                "a paused presentation pass retains the same committed-frame selector"
            );
            assert_eq!(sim.state_hash(), before_hash, "presentation is read-only");
        }
    }

    #[test]
    fn drive_ship_slope_snapshot_uses_production_display_frame_selector() {
        let mut sim = Simulation::with_seed(0);
        sim.session.binary_frame = 51;
        let mut entity = GameEntity::test_default(1, "SHIP", "Americans", 0, 0);
        entity.owner = sim.intern("Americans");
        entity.type_ref = sim.intern("SHIP");
        entity.locomotor = Some(LocomotorState::for_test_kind_at_frame(
            LocomotorKind::Ship,
            40,
        ));
        let slope = entity
            .locomotor
            .as_mut()
            .unwrap()
            .active_slope_transition_mut()
            .unwrap();
        slope.snap(4, 40);
        slope.sample_process_entry(9, 49);
        sim.substrate.entities.insert(entity);

        let bytes = GameSnapshot::save(&sim, 1, 2, "slope display selector", 3);
        let restored = GameSnapshot::load(&bytes)
            .expect("current slope snapshot")
            .sim;
        let display_binary_frame =
            display_binary_frame_for_committed_session(restored.session.binary_frame);
        assert_eq!(display_binary_frame, 50);
        assert_eq!(
            locomotor_render_slope_state(
                restored.substrate.entities.get(1).unwrap(),
                display_binary_frame,
            ),
            Some(UnitRenderSlopeState::Transition {
                from_slope: 4,
                to_slope: 9,
                phase_num: 1,
                phase_den: SLOPE_TRANSITION_FRAMES,
            }),
            "session frame 51 presents processed frame 50, one third through a timer started at 49"
        );
    }

    #[test]
    fn turret_offset_pivot_uses_the_same_slope_blend_as_raster_layers() {
        const OFFSET: i32 = 80;
        const FACING: u8 = 0;
        fn close(a: (f32, f32), b: (f32, f32)) -> bool {
            (a.0 - b.0).abs() < 0.000_01 && (a.1 - b.1).abs() < 0.000_01
        }

        let stable_from = turret_screen_offset(OFFSET, FACING, UnitRenderSlopeState::Stable(0));
        let stable_to = turret_screen_offset(OFFSET, FACING, UnitRenderSlopeState::Stable(4));
        assert_eq!(
            stable_to,
            crate::render::vxl_raster::turret_pivot_screen_offset(OFFSET, FACING, 4, 1.0),
            "the stable path remains the existing exact slope transform"
        );

        let blended = |phase_num| {
            turret_screen_offset(
                OFFSET,
                FACING,
                UnitRenderSlopeState::Transition {
                    from_slope: 0,
                    to_slope: 4,
                    phase_num,
                    phase_den: SLOPE_TRANSITION_FRAMES,
                },
            )
        };
        let phase_zero = blended(0);
        let phase_one_third = blended(1);
        let phase_two_thirds = blended(2);

        assert!(close(phase_zero, stable_from));
        assert!(!close(phase_zero, stable_to));
        assert!(!close(phase_one_third, stable_from));
        assert!(!close(phase_one_third, stable_to));
        assert!(!close(phase_two_thirds, stable_from));
        assert!(!close(phase_two_thirds, stable_to));
        assert!(!close(phase_one_third, phase_two_thirds));
    }

    fn spawn_manager(states: &[SpawnSlotState]) -> SpawnManagerState {
        SpawnManagerState {
            spawn_type: InternedId::default(),
            missile_family: None,
            regen_rate: 400,
            reload_rate: 0,
            kamikaze_wait_frames: 0,
            slots: states
                .iter()
                .enumerate()
                .map(|(index, &state)| SpawnSlot {
                    spawn: (state != SpawnSlotState::Regenerating).then_some(index as u64 + 1),
                    state,
                    timer: SpawnTimer::ready(),
                    is_missile_spawn: true,
                })
                .collect(),
            update_timer: SpawnTimer::armed(10, 20),
            reload_timer: SpawnTimer::ready(),
            current_target: None,
            queued_target: None,
            mode: SpawnManagerMode::Idle,
        }
    }

    #[test]
    fn gsi_13_07_no_spawn_alt_uses_zero_docked_not_zero_alive() {
        for (state, expects_alt) in [
            (SpawnSlotState::ReadyDocked, false),
            (SpawnSlotState::KamikazeWait, true),
            (SpawnSlotState::InFlight, true),
            (SpawnSlotState::ReturningToDock, true),
            (SpawnSlotState::LandingAtDock, true),
            (SpawnSlotState::Reloading, false),
            (SpawnSlotState::Regenerating, true),
        ] {
            let manager = spawn_manager(&[state]);
            assert_eq!(
                no_spawn_alt_type_id("V3", true, Some(&manager)).as_deref(),
                expects_alt.then_some("V3WO"),
                "state {state:?}"
            );
        }

        let in_flight = spawn_manager(&[SpawnSlotState::InFlight]);
        assert_eq!(in_flight.count_alive_spawns(), 1);
        assert_eq!(
            no_spawn_alt_type_id("V3", true, Some(&in_flight)).as_deref(),
            Some("V3WO"),
            "a live child away from its dock still selects the empty rack"
        );
        assert_eq!(no_spawn_alt_type_id("V3", false, Some(&in_flight)), None);
        assert_eq!(no_spawn_alt_type_id("V3", true, None), None);
    }

    #[test]
    fn gsi_13_07_dreadnought_stays_loaded_while_any_slot_is_docked() {
        for (states, expected) in [
            (
                [SpawnSlotState::ReadyDocked, SpawnSlotState::InFlight],
                None,
            ),
            (
                [SpawnSlotState::Reloading, SpawnSlotState::Regenerating],
                None,
            ),
            (
                [SpawnSlotState::InFlight, SpawnSlotState::Regenerating],
                Some("DREDWO"),
            ),
        ] {
            let manager = spawn_manager(&states);
            assert_eq!(
                no_spawn_alt_type_id("DRED", true, Some(&manager)).as_deref(),
                expected,
                "states {states:?}"
            );
        }
    }

    #[test]
    fn gsi_13_07_no_spawn_alt_selection_is_not_manager_timer_gated() {
        let mut manager = spawn_manager(&[SpawnSlotState::ReadyDocked]);
        assert!(!manager.update_timer.due(10));
        assert_eq!(no_spawn_alt_type_id("V3", true, Some(&manager)), None);

        manager.slots[0].state = SpawnSlotState::InFlight;
        assert_eq!(
            no_spawn_alt_type_id("V3", true, Some(&manager)).as_deref(),
            Some("V3WO"),
            "the next presentation query sees the slot transition without an AI pass"
        );
    }

    #[test]
    fn gsi_13_10_vxl_selector_uses_unit_scalar_and_keeps_aircraft_compatibility_path() {
        let mut grid = lighting::CellLightGrid::new();
        grid.insert_profiled_light((4, 5), [1.0, 0.88, 0.88], 1.0);

        let unit = vxl_body_tint(&grid, (4, 5), EntityCategory::Unit, 200, 200);
        let aircraft = vxl_body_tint(&grid, (4, 5), EntityCategory::Aircraft, 200, 200);
        let structure = vxl_body_tint(&grid, (4, 5), EntityCategory::Structure, 200, 200);

        for (actual, expected) in unit.into_iter().zip([1.2, 1.056, 1.056]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in aircraft.into_iter().zip([1.2, 1.08, 1.08]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in structure.into_iter().zip([1.0, 0.88, 0.88]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        assert_ne!(unit, aircraft);
    }

    #[test]
    fn stable_and_transition_body_pieces_keep_one_ground_parent_order() {
        let sprite = SpriteInstance::default();
        let mut stable = Vec::new();
        let mut stable_pages = Vec::new();
        let mut transition = Vec::new();
        let mut ground_pieces = Vec::new();
        for source in [
            UnitTextureSource::Stable(5),
            UnitTextureSource::Transition(1),
        ] {
            push_unit_sprite(
                &mut stable,
                &mut stable_pages,
                &mut transition,
                source,
                sprite,
                true,
                &mut ground_pieces,
            );
        }
        assert!(stable.is_empty());
        assert!(transition.is_empty());
        assert_eq!(ground_pieces.len(), 2);
        assert_eq!(ground_pieces[0].target, GroundTexture::UnitAtlasPage(5));
        assert_eq!(
            ground_pieces[1].target,
            GroundTexture::UnitTransitionPage(1)
        );
        assert!(
            ground_pieces
                .iter()
                .all(|piece| piece.render_z == RenderZPolicy::ReadOnly)
        );
        push_unit_sprite(
            &mut stable,
            &mut stable_pages,
            &mut transition,
            UnitTextureSource::Stable(3),
            sprite,
            false,
            &mut ground_pieces,
        );
        push_unit_sprite(
            &mut stable,
            &mut stable_pages,
            &mut transition,
            UnitTextureSource::Transition(2),
            sprite,
            false,
            &mut ground_pieces,
        );
        assert_eq!(stable_pages, [3]);
        assert_eq!(transition[2].len(), 1);
    }

    #[test]
    fn split_bound_ignores_atlas_padding_and_unrequested_part_facings() {
        let body = UnitSpriteEntry {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [100.0, 100.0],
            offset_x: -50.0,
            offset_y: -50.0,
            page: 0,
            native_draw_bounds: Some([-10, -15, 20, 25]),
        };
        let turret = UnitSpriteEntry {
            native_draw_bounds: Some([-5, -20, 10, 15]),
            ..body
        };
        let (rect, split) =
            composite_depth_rect([(body, [0.0, 100.0]), (turret, [0.0, 100.0])], true);
        assert!(split);
        assert_eq!(rect, [80.0, 30.0]);
        let (rect, split) = composite_depth_rect([(body, [0.0, 100.0])], false);
        assert!(!split);
        assert_eq!(rect, [50.0, 100.0]);
        let unknown = UnitSpriteEntry {
            native_draw_bounds: None,
            ..body
        };
        assert!(!composite_depth_rect([(body, [0.0, 0.0]), (unknown, [0.0, 0.0])], true).1);
    }
}
