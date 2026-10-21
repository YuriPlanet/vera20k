//! Voxel unit instance builders — per-frame SpriteInstance generation for VXL entities.
//!
//! Handles turret/barrel separation, harvest overlays, and VXL animation frames.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use super::helpers::{
    EntityDrawBand, compute_sprite_depth, entity_draw_band, ground_sort_row, in_view,
    projection_admitted, tactical_entity_admission,
};
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
};
use crate::map::entities::EntityCategory;
use crate::map::lighting;
use crate::map::terrain::{TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::{DrawState, FX_SHADOW};
use crate::render::native_z::{
    SHP_DRAW_Z_ADJUST_PX, ZGradient, pack_voxel_z_gradient, pack_z_gradient,
};
use crate::render::sprite_atlas::ShpSpriteKey;
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::unit_atlas::{
    UnitSpriteEntry, UnitSpriteKey, VxlLayer, canonical_turret_facing, canonical_unit_facing,
};
use crate::rules::house_colors::{self, HouseColorIndex};
use crate::rules::voxel_frame_catalog::{
    NO_SPAWN_ALT_SUFFIX, draws_turret_parts, voxel_turret_index,
};
#[cfg(test)]
use crate::sim::movement::slope_transition::SLOPE_TRANSITION_FRAMES;
use std::borrow::Cow;

#[cfg(test)]
#[path = "naval_draw_tests.rs"]
mod naval_draw_tests;

#[cfg(test)]
#[path = "unit_body_draw_tests.rs"]
mod unit_body_draw_tests;

const NATIVE_TURRET_FIRST_START: u8 = 32;
const NATIVE_TURRET_FIRST_END_EXCLUSIVE: u8 = 160;

/// `extra_light` is the class's
/// [`crate::app::presentation::lighting::body_extra_light`].
fn vxl_body_tint(
    grid: &lighting::CellLightGrid,
    cell: (u16, u16),
    category: EntityCategory,
    extra_light: i32,
) -> [f32; 3] {
    match category {
        EntityCategory::Unit | EntityCategory::Aircraft => grid.body_tint_at(cell, extra_light),
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
    Pose(usize),
}

// Original755852..755875 initializes17..20 to identity, so the stable
// atlas can share the flat slot for those entries.
fn clamp_slope_for_render(slope: u8) -> u8 {
    if slope <= 16 { slope } else { 0 }
}

fn terrain_slope_for_render(state: &AppState, rx: u16, ry: u16) -> u8 {
    state
        .terrain_template()
        .and_then(|t| t.cell(rx, ry))
        .map(|c| clamp_slope_for_render(c.slope_type))
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
    if entity.category() == EntityCategory::Aircraft {
        return UnitRenderSlopeState::Stable(0);
    }
    // `JumpjetLocomotionClass` Draw_Matrix (`0x0054DCC0`) is the facing matrix
    // of `LocomotionClass::Draw_Matrix @ 0x0055A730`, tilted only by its
    // `TiltCrashJumpjet=` arm: a Jumpjet body never takes a cell slope, landed
    // or airborne.
    let jumpjet = entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
    });
    // A body that has left the floor has no cell slope to sit on, so it never
    // enters the drive-track tilt transition. Upper-layer bodies therefore
    // keep their stable atlas shape.
    if jumpjet || band != EntityDrawBand::Ground {
        return UnitRenderSlopeState::Stable(0);
    }

    UnitRenderSlopeState::Stable(terrain_slope_for_render(
        state,
        entity.position.rx(),
        entity.position.ry(),
    ))
}

fn locomotor_render_slope_state(
    entity: &crate::sim::game_entity::GameEntity,
    display_binary_frame: u32,
) -> Option<UnitRenderSlopeState> {
    crate::sim::movement::slope_transition::state_for_entity(entity)
        .map(|state| render_slope_phase(state.render_phase(display_binary_frame)))
}

fn render_slope_phase(
    phase: crate::sim::movement::slope_transition::SlopeRenderPhase,
) -> UnitRenderSlopeState {
    match phase {
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
/// All pieces retain the body's Display parent, including Air and Top objects.
/// Atlas pages select textures only; they never replace native member order.
pub(crate) fn build_unit_instances(
    state: &AppState,
    objects: &mut Vec<PlannedObjectInstance>,
    display_order: &NativeDisplayOrder,
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
    let display_binary_frame =
        display_binary_frame_for_committed_session(sim.session().binary_frame());
    let (cam_x, cam_y, sw, sh) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.render_width() as f32 / z,
        state.render_height() as f32 / z,
    );
    let (_, _, tactical_width, tactical_height) =
        crate::app::input::camera::tactical_viewport_px(state);
    let unit_viewport = [tactical_width as f32 / z, tactical_height as f32 / z];
    let local_owner = crate::app::input::commands::preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|o| sim.interner().get(o));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let art_reg: Option<&crate::rules::art_data::ArtRegistry> =
        state.rules().map(|rules| rules.art());
    let terrain = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|rt| rt.view().resolved_terrain());
    let light_cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    state
        .match_state
        .match_presentation
        .sinking_waterlines
        .borrow_mut()
        .retain(|id| sim.entities().contains(id));

    let surface_view = crate::app::presentation::render::tactical_surface_line_viewport(state);
    let laser_palette = std::cell::OnceCell::new();
    let encounter_order = super::helpers::tactical_entity_encounter_order(sim);
    for stable_id in encounter_order {
        let Some(entity) = sim.entities().get(stable_id) else {
            continue;
        };
        if entity.category() != EntityCategory::Unit && !entity.is_voxel() {
            continue;
        }
        let Some(band) = entity_draw_band(sim.display_layers(), stable_id) else {
            continue;
        };
        // The screen cull reads only the position, and nothing between here
        // and the waterline capture writes, so an object off the screen
        // skips the admission work below.
        let viewport = if entity.category() == EntityCategory::Unit {
            unit_viewport
        } else {
            [sw, sh]
        };
        let (aircraft_shadow, shadow_render_pose) = aircraft_draw_sample(state, entity)
            .map_or((None, None), |sample| (sample.shadow, Some(sample.pose)));
        let Some([sx, _sy]) = unit_draw_anchor(
            entity,
            [cam_x, cam_y],
            viewport,
            aircraft_shadow.as_ref().map(|pose| pose.anchor),
        ) else {
            continue;
        };
        let render_pose = shadow_render_pose.unwrap_or_else(|| {
            crate::app::presentation::interpolation::pose(
                state,
                entity,
                sim.session().binary_frame(),
            )
        });
        let native_anchor = crate::render::locomotor_visual::screen_position(entity);
        let sx = sx + render_pose.screen[0] - native_anchor.0;
        let sy = render_pose.screen[1] - render_pose.visual_lift;
        // Common visibility, passenger, limbo, and DrawState admission is shared below.
        let pos = &entity.position;
        let owner_str = sim.interner().resolve(entity.owner());
        let mut observer = super::helpers::observer_draw_context(
            sim,
            entity,
            local_owner.as_deref(),
            local_owner_id,
            state.rules(),
        );
        let Some((type_name, mut body)) = unit_body_draw(
            entity,
            sim.interner(),
            state.rules(),
            band,
            sim.session().binary_frame(),
            observer,
        ) else {
            continue;
        };
        // Interpolate continuous attitude, including the last interval back
        // to level. Native admission/type/model choice is left unchanged.
        let tilt_crash = state
            .rules()
            .and_then(|r| sim.interner().object_type(entity.type_ref(), r))
            .is_some_and(|o| o.tilt_crash_jumpjet);
        if let Some(tilt) = body_pose_with_angles(
            entity,
            band,
            tilt_crash,
            render_pose.angles,
            render_pose.rocket_pitch,
        ) {
            body = BodyDraw::Pose(tilt);
        } else if matches!(body, BodyDraw::Pose(_)) {
            body = BodyDraw::Composite;
        }
        let no_shadow = state
            .rules()
            .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.no_shadow);
        let type_str = type_name.as_ref();
        observer.drawn_voxel = Some(true);
        let remap_owner = entity
            .disguise
            .as_ref()
            .filter(|_| DrawState::draws_disguise(entity, sim.session().binary_frame(), observer))
            .and_then(|state| state.house())
            .map(|id| sim.interner().resolve(id))
            .unwrap_or(owner_str);
        let hc: HouseColorIndex = state
            .match_state
            .match_presentation
            .house_color_map
            .get(remap_owner)
            .copied()
            .unwrap_or_default();
        let Some(draw_decision) = tactical_entity_admission(
            super::helpers::TacticalEntityPurpose::Drawing,
            entity,
            owner_str,
            local_owner.as_deref(),
            local_owner_id,
            sim.fog(),
            sim.house_alliances(),
            ignore_visibility,
            sim.session().binary_frame(),
            house_color_to_remap_row(hc),
            observer,
        ) else {
            continue;
        };
        // Render slope comes from the locomotor's cached previous/current
        // slope during gamemd's 3-frame transition, then falls back to the
        // stable terrain slope path.
        let slope_state = render_pose
            .slope
            .map(render_slope_phase)
            .unwrap_or_else(|| unit_render_slope_state(state, entity, display_binary_frame, band));
        let interp_z = pos.z();
        let draw_state = draw_decision.state;
        let extra_light = state.rules().map_or(0, |rules| {
            crate::app::presentation::lighting::body_extra_light(
                entity,
                rules,
                sim,
                light_cells.as_ref(),
            )
        });
        let tint = vxl_body_tint(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx(), pos.ry()),
            entity.category(),
            extra_light,
        );
        let palette_light = crate::app::presentation::lighting::body_palette_light(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx(), pos.ry()),
            extra_light,
        );
        // UnitClass::DrawVoxelBody's curtain arm (`0x0073BF9C..0x0073BFB8`):
        // the intensity of the hull, turret and barrel composite, whose blit
        // (vt+0x55C, `0x0073C1A5`) picks its LightConvert row from it. The
        // parts are drawn into the composite's 8-bit surface (`0x0073B547`; a
        // 256x256 surface of one byte per pixel, `0x007473FA..0x0074741C`),
        // where TechnoClass::Draw's own arm lights nothing. The harvest
        // overlay keeps the unscaled light: UnitClass::Draw blits it with its
        // own intensity (`0x0073D283`). Every other voxel body, such as an
        // aircraft's, takes TechnoClass::Draw's arm (`0x0070678D..
        // 0x007067E2`), the same scale.
        // Unit's remaining colour/deactivation phase is shared by all
        // stable and posed hull/turret/barrel consumers below. The harvest
        // overlay continues to receive the original light.
        let (body_tint, body_light) = crate::app::presentation::lighting::techno_effect_light(
            entity,
            tint,
            palette_light,
            sim,
        );
        let (body_tint, body_light) = crate::app::presentation::lighting::unit_voxel_effect_light(
            entity,
            body_tint,
            body_light,
            state.rules(),
            sim.session().binary_frame(),
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
                .occupancy()
                .first_building_on_layer(
                    pos.rx(),
                    pos.ry(),
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                )
                .is_some()
        {
            TILE_HEIGHT
        } else {
            0.0
        };

        let turret_index =
            voxel_turret_index(type_str, state.rules(), entity.current_turret_index());
        let (anim_frame, turret_frame) =
            unit_animation_frames(entity, type_str, turret_index, &atlas.frame_counts);

        // Chrono teleport doesn't tint the unit — the visual effect is the
        // WarpOut animation overlay; the unit itself stays fully opaque.
        let alpha: f32 = 1.0;
        // 0x73B140's split is inside the same native parent draw call.
        // Ground units, including those below bridges, keep LayerClass order.
        let mut pieces = Vec::new();
        // Aircraft4148A0 draws the unmasked ground shadow before its body
        // at4149D7. Unlike Unit's masked suffix, the body's later pixels
        // cover its own stencil when parked or low above the ground.
        if let Some(pose) = aircraft_shadow {
            emit_aircraft_shadow_sprite(
                state,
                atlas,
                entity,
                type_str,
                anim_frame,
                pose,
                draw_state,
                &mut pieces,
            );
        }

        // Sampled at the turret's frame, so the two sprites cannot disagree.
        let body_facing = (render_pose.body >> 8) as u8;
        let mut ground_pose = ground_pose_with_angles(
            entity,
            slope_state,
            sim.session().binary_frame(),
            render_pose.angles,
        );
        if let Some(ground_pose) = ground_pose.as_mut() {
            ground_pose.body_facing =
                crate::render::vxl_raster::voxel_facing_step_u16(render_pose.body) * 8;
        }
        if let BodyDraw::Pose(tilt) = body {
            let key = UnitSpriteKey {
                type_id: type_str.to_string(),
                turret_index,
                facing: canonical_unit_facing(body_facing),
                layer: VxlLayer::Composite,
                frame: anim_frame,
                slope_type: stable_slope_for_key(slope_state),
                barrel_pitch: 0,
            };
            emit_pose_sprite(
                state,
                &mut pieces,
                entity,
                &key,
                tilt,
                [center_x, center_y],
                interp_z,
                body_tint,
                body_light,
                draw_state,
            );
        } else if ground_pose.is_some() || matches!(body, BodyDraw::Turret { .. }) {
            let (turret_facing, barrel_elevation) = match body {
                BodyDraw::Turret {
                    turret: _,
                    barrel_elevation: _,
                } => (render_pose.turret, render_pose.barrel),
                _ => (render_pose.body, render_pose.barrel),
            };
            // Turret unit: emit body, turret, and barrel as separate sprites.
            emit_turret_unit_sprites(
                no_shadow,
                atlas,
                art_reg,
                entity,
                type_str,
                body_facing,
                turret_facing,
                turret_index,
                barrel_elevation,
                hc,
                center_x,
                center_y,
                state,
                interp_z,
                body_tint,
                body_light,
                alpha,
                draw_state,
                anim_frame,
                turret_frame,
                dock_depth_y_offset,
                slope_state,
                band,
                ground_pose,
                &mut pieces,
            );
        } else {
            // A type with indexed gun models shares its bare hull key when
            // current=-1 suppresses the gun arm; ordinary types use Composite.
            let key: UnitSpriteKey = UnitSpriteKey {
                type_id: type_str.to_string(),
                turret_index: 0,
                facing: canonical_unit_facing(body_facing),
                layer: if draws_turret_parts(type_str, state.rules(), 0) {
                    VxlLayer::Body
                } else {
                    VxlLayer::Composite
                },
                frame: anim_frame,
                slope_type: stable_slope_for_key(slope_state),
                barrel_pitch: 0,
            };
            if let Some((entry, texture_source)) = unit_entry_for_pose(state, atlas, &key, None) {
                let depth_y: f32 = sy + entry.offset_y + entry.pixel_size[1] + dock_depth_y_offset;
                let depth: f32 = body_sort_depth(state, entity, depth_y, interp_z);
                let voxel_adjust =
                    super::foot_depth::unit_z_adjust(state, entity, true, None) as f32;
                let native_shadow = matches!(texture_source, UnitTextureSource::Stable(_))
                    && prepare_unit_shadow(
                        state,
                        atlas,
                        entity,
                        type_str,
                        body_facing,
                        no_shadow,
                        slope_state,
                        band,
                        [(entry, [0.0, 0.0])],
                    );
                if !native_shadow {
                    emit_unit_shadow_sprite(
                        native_shadow,
                        no_shadow,
                        voxel_adjust,
                        atlas,
                        entity,
                        type_str,
                        body_facing,
                        center_x,
                        center_y,
                        depth,
                        draw_state,
                        slope_state,
                        band,
                        &mut pieces,
                    );
                }
                let bounds = composite_draw_bounds([(entry, [center_x, center_y])]);
                let (composite_rect, split) =
                    bounds.depth_rect(super::foot_depth::unit_bridge_split(state, entity, true));
                let body_draw_state =
                    unit_body_draw_state(state, entity, draw_state, split, bounds);
                let sprite = SpriteInstance {
                    position: [center_x + entry.offset_x, center_y + entry.offset_y],
                    size: entry.pixel_size,
                    uv_origin: entry.uv_origin,
                    uv_size: entry.uv_size,
                    depth,
                    tint: body_tint,
                    palette_light: body_light,
                    alpha,
                    draw_state: body_draw_state,
                    z_adjust: voxel_adjust,
                    z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
                    zshape_origin: composite_rect,
                    ..Default::default()
                };
                push_unit_sprite(texture_source, sprite, &mut pieces);
                if native_shadow {
                    emit_unit_shadow_sprite(
                        native_shadow,
                        no_shadow,
                        voxel_adjust,
                        atlas,
                        entity,
                        type_str,
                        body_facing,
                        center_x,
                        center_y,
                        depth,
                        draw_state,
                        slope_state,
                        band,
                        &mut pieces,
                    );
                }
            }
        }

        emit_jumpjet_unit_shadow(
            state,
            atlas,
            entity,
            type_str,
            anim_frame,
            [center_x, center_y],
            no_shadow,
            draw_state,
            band,
            &mut pieces,
        );

        // OREGATH is an SHP sprite from sprite_atlas, but remains an owned piece
        // of its harvester's Ground slot so atlas identity cannot re-sort it.
        if let Some(frame) = harvest_overlay_frame(state.rules(), sim, entity, display_binary_frame)
            && let Some((page, instance)) = emit_harvest_overlay(
                state,
                entity,
                frame,
                {
                    let arm = harvest_arm_screen_position(entity, sim.session().binary_frame());
                    (
                        arm.0 + render_pose.screen[0] - native_anchor.0,
                        arm.1 + render_pose.screen[1] - native_anchor.1,
                    )
                },
                pos.z(),
                tint,
                palette_light.brightness(),
                draw_state,
            )
        {
            pieces.push(ObjectPieceInstance {
                target: ObjectTexture::ShpPage(page),
                render_z: RenderZPolicy::ReadOnly,
                instance,
            });
        }

        if !pieces.is_empty() {
            let parent = display_order.object_draw(
                entity.stable_id(),
                crate::render::tactical_draw_plan::SpriteEncoding::Voxel,
            );
            if let Some(parent) = parent {
                let mut planned = PlannedObjectInstance::object(parent, pieces);
                if let Some(rules) = state.rules() {
                    planned.tail_draws = crate::app::presentation::target_lines::target_laser_draws(
                        entity,
                        sim,
                        rules,
                        || {
                            laser_palette
                                .get_or_init(|| {
                                    state
                                        .process_assets
                                        .manager()
                                        .and_then(|assets| assets.get_ref("palette.pal"))
                                        .and_then(|bytes| {
                                            crate::assets::pal_file::Palette::from_bytes(bytes).ok()
                                        })
                                })
                                .as_ref()
                        },
                        surface_view,
                    );
                }
                objects.push(planned);
            }
        }
    }
}

/// How an object's body is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BodyDraw {
    /// A body at its locomotor arm's pose, on the per-frame pose page.
    Pose(crate::render::unit_atlas::PoseTilt),
    /// Body, turret and barrel sprites: the turret facing (`+0x3A0`) and the
    /// barrel elevation (`+0x370`), sampled at one frame.
    Turret { turret: u16, barrel_elevation: u16 },
    /// One composite atlas sprite.
    Composite,
}

/// Unit DrawVoxelBody73B4DA..73B50E and `AircraftClass::Draw_It`
/// (`0x004145B0..0x004145DB`) divide the persistent Foot+538 counter by the
/// selected model's main HVA frame count. The object's original model may
/// have a different count (SCHP has two frames, deployed SCHD has one). When
/// that signed remainder is zero, a Unit's turret takes Techno+148 modulo its
/// own HVA count; an aircraft draws its main voxel alone. Negative remainders
/// retain their bits in the atlas key; invalid frame indices have no sprite.
/// Native vectors: tools/spatial_oracle/unit_simple_deploy.json draw_frames
/// and aircraft_draw_frames. Aircraft also includes this remainder in its
/// shadow cache key (414728); its shadow uses HVA frame0 and ShadowIndex
/// separately.
fn unit_animation_frames(
    entity: &crate::sim::game_entity::GameEntity,
    model: &str,
    turret_index: i32,
    frame_counts: &std::collections::BTreeMap<(String, VxlLayer, i32), u32>,
) -> (u32, u32) {
    let frames = frame_counts
        .get(&(model.to_string(), VxlLayer::Body, 0))
        .or_else(|| frame_counts.get(&(model.to_string(), VxlLayer::Composite, 0)))
        .copied()
        .unwrap_or(1)
        .max(1);
    let body = ((entity.body_frame_counter.get() as i32) % frames as i32) as u32;
    let turret = if body != 0 || entity.turret_anim_frame.frame() == 0 {
        body
    } else {
        frame_counts
            .get(&(model.to_string(), VxlLayer::Turret, turret_index))
            .filter(|&&frames| frames > 1)
            .map_or(0, |&frames| {
                (entity.turret_anim_frame.frame() % frames as i32) as u32
            })
    };
    (body, turret)
}

/// The voxel model one Unit or Aircraft body is drawn from this frame.
///
/// `UnitClass::DrawVoxelBody` selects a deployed unit's `UnloadingClass=`
/// after its disguise arm (`0x0073B4BC..0x0073B4D8`): Unit+0x6E0 therefore
/// overrides disguise. The selected model owns the turret split below.
/// See `tools/spatial_oracle/unit_simple_deploy.md`.
///
/// Otherwise disguise precedes `NoSpawnAlt`, selected from the current
/// docked slot count at draw time. The serialized override remains solely
/// the miner dock sub-FSM's UnloadingClass (HORV/CMON) hint.
pub(crate) fn drawn_model_id<'a>(
    entity: &'a crate::sim::game_entity::GameEntity,
    interner: &'a crate::sim::intern::StringInterner,
    rules: Option<&'a crate::rules::ruleset::RuleSet>,
    binary_frame: u32,
    observer: crate::render::draw_state::ObserverDrawContext,
) -> Cow<'a, str> {
    let base_type = interner.resolve(entity.type_ref());
    let object = rules.and_then(|rules| rules.object(base_type));
    if entity.category() == EntityCategory::Unit
        && entity.is_fully_deployed()
        && let Some(unloading_type) = object.and_then(|object| object.unloading_class.as_deref())
    {
        return Cow::Borrowed(unloading_type);
    }
    let no_spawn_alt = object.is_some_and(|object| object.no_spawn_alt);
    if let Some(disguise_type) = entity
        .disguise
        .as_ref()
        .filter(|_| DrawState::draws_disguise(entity, binary_frame, observer))
        .and_then(|disguise| disguise.type_id())
    {
        Cow::Borrowed(interner.resolve(disguise_type))
    } else if let Some(no_spawn_alt_type) =
        no_spawn_alt_type_id(base_type, no_spawn_alt, entity.spawn_manager.as_ref())
    {
        Cow::Owned(no_spawn_alt_type)
    } else if let Some(display_override) = (!no_spawn_alt)
        .then_some(entity.display_type_override.image())
        .flatten()
    {
        Cow::Borrowed(interner.resolve(display_override))
    } else {
        Cow::Borrowed(base_type)
    }
}

/// One Unit or Aircraft body's voxel model and how it is drawn this frame.
/// `UnitClass::DrawIt @ 0x0073CEC0` suppresses the body during either
/// deployment transition (`0x0073CF46` / `0x0073CF54` read Unit+0x6E1 /
/// +0x6E2); the owned deploy animation draws separately. See
/// `tools/spatial_oracle/unit_simple_deploy.md`.
fn unit_body_draw<'a>(
    entity: &'a crate::sim::game_entity::GameEntity,
    interner: &'a crate::sim::intern::StringInterner,
    rules: Option<&'a crate::rules::ruleset::RuleSet>,
    band: EntityDrawBand,
    binary_frame: u32,
    observer: crate::render::draw_state::ObserverDrawContext,
) -> Option<(Cow<'a, str>, BodyDraw)> {
    if entity.category() == EntityCategory::Unit && entity.unit_deploying() {
        return None;
    }
    let model = drawn_model_id(entity, interner, rules, binary_frame, observer);
    if !super::helpers::drawn_type_uses_voxel(entity, &model, interner, rules) {
        return None;
    }
    let tilt_crash_jumpjet = rules
        .and_then(|rules| rules.object(&model))
        .is_some_and(|object| object.tilt_crash_jumpjet);
    let turret_parts = draws_turret_parts(&model, rules, entity.current_turret_index());
    let body = body_draw(entity, band, binary_frame, tilt_crash_jumpjet, turret_parts);
    Some((model, body))
}

/// The locomotor pose is decided before the turret split. `tilt_crash_jumpjet` is
/// the drawn model's `TiltCrashJumpjet=` and `turret_parts` its
/// [`draws_turret_parts`]: the split follows the model being drawn, as
/// `UnitClass::DrawVoxelBody` tests its draw type (`0x0073B7A3`), not the
/// object's own `+0x3A0`. A War Miner unloading as HORV has a turret facing
/// and draws none, and every Fly keeps its Secondary facing in
/// `barrel_facing` and draws no turret.
///
/// RESIDUAL: the turret arm reads `+0x3A0` on every unit; VERA keeps it only
/// for a unit whose own type has a turret, so one without draws its model's
/// turret at the hull's facing. Trigger: a `Turret=no` unit drawn as a
/// `Turret=yes` model, by `UnloadingClass=` or disguise. The stock miner
/// conversions in the Hills/Battle regression do not trigger it; other
/// disguise and layered-rule combinations remain unverified.
fn body_draw(
    entity: &crate::sim::game_entity::GameEntity,
    band: EntityDrawBand,
    binary_frame: u32,
    tilt_crash_jumpjet: bool,
    turret_parts: bool,
) -> BodyDraw {
    if let Some(tilt) = body_pose(entity, band, tilt_crash_jumpjet) {
        return BodyDraw::Pose(tilt);
    }
    if !turret_parts {
        return BodyDraw::Composite;
    }
    BodyDraw::Turret {
        turret: entity.barrel_facing.as_ref().map_or_else(
            || entity.body_facing_current(binary_frame),
            |facing| facing.current(binary_frame),
        ),
        barrel_elevation: entity.barrel_elevation().current(binary_frame),
    }
}

/// The locomotor Draw_Matrix arm that poses this body, if any. Fly
/// Draw_Matrix tilts a crashing body by its roll and pitch
/// (`TechnoClass+0x328`/`+0x32C`) while it is airborne (`0x004CF6A3`): every
/// airborne Fly body is in the Top band (`In_Which_Layer @ 0x004CFCF0`).
/// Jumpjet Draw_Matrix tilts a `TiltCrashJumpjet=` type whenever either angle
/// passes its gate (`0x0054DCDA..0x0054DD13`), crashing or not. Rocket
/// Draw_Matrix (`0x00663470`) pitches the body whenever CurrentPitch is not
/// zero; at zero it is the plain facing matrix.
fn body_pose(
    entity: &crate::sim::game_entity::GameEntity,
    band: EntityDrawBand,
    tilt_crash_jumpjet: bool,
) -> Option<crate::render::unit_atlas::PoseTilt> {
    let angles = entity
        .rocking
        .as_ref()
        .map_or([0.; 2], |r| r.angles_native());
    let pitch = entity
        .locomotor
        .as_ref()
        .and_then(|l| l.rocket_runtime())
        .map_or(0., |r| r.draw_pitch());
    body_pose_with_angles(entity, band, tilt_crash_jumpjet, angles, pitch)
}

fn body_pose_with_angles(
    entity: &crate::sim::game_entity::GameEntity,
    band: EntityDrawBand,
    tilt_crash_jumpjet: bool,
    angles: [f32; 2],
    pitch: f32,
) -> Option<crate::render::unit_atlas::PoseTilt> {
    use crate::render::unit_atlas::PoseTilt;
    use crate::rules::locomotor_type::LocomotorKind;
    let locomotor = entity.locomotor.as_ref()?;
    if locomotor.rocket_runtime().is_some() {
        return (pitch != 0.0).then_some(PoseTilt::Rocket(pitch));
    }
    entity.rocking.as_ref()?;
    match locomotor.active_kind() {
        LocomotorKind::Fly if entity.crash.is_crashing() && band != EntityDrawBand::Ground => {
            Some(PoseTilt::Fly(angles))
        }
        LocomotorKind::Jumpjet
            if tilt_crash_jumpjet && crate::map::voxel_pose::voxel_tilt_applies(angles) =>
        {
            Some(PoseTilt::Jumpjet(angles))
        }
        _ => None,
    }
}

/// Drive4AFFB0..4AFFE8 / Ship69F6C0..69F6F8 key dynamic draws for an unsettled slope or either
/// qualifying angle. Model loading stays with UnitModel; no ART boolean is
/// used as evidence that a VXL loaded. Read-only: no RNG/timer/detach writes.
#[cfg(test)]
fn ground_pose(
    entity: &crate::sim::game_entity::GameEntity,
    slope: UnitRenderSlopeState,
    binary_frame: u32,
) -> Option<crate::render::vxl_raster::GroundPose> {
    ground_pose_with_angles(
        entity,
        slope,
        binary_frame,
        entity
            .rocking
            .as_ref()
            .map_or([0.; 2], |r| r.angles_native()),
    )
}

fn ground_pose_with_angles(
    entity: &crate::sim::game_entity::GameEntity,
    slope: UnitRenderSlopeState,
    binary_frame: u32,
    angles: [f32; 2],
) -> Option<crate::render::vxl_raster::GroundPose> {
    if !matches!(
        entity.locomotor.as_ref()?.active_kind(),
        crate::rules::locomotor_type::LocomotorKind::Drive
            | crate::rules::locomotor_type::LocomotorKind::Ship
    ) {
        return None;
    }
    (matches!(slope, UnitRenderSlopeState::Transition { .. })
        || crate::map::voxel_pose::voxel_tilt_applies(angles))
    .then_some(crate::render::vxl_raster::GroundPose {
        angles,
        body_facing: crate::render::vxl_raster::voxel_facing_step_u16(
            entity.body_facing_current(binary_frame),
        ) * 8,
        slope_blend: slope_blend_for_render_state(slope),
        turret_offset: 0,
    })
}

/// One posed body drawn on this frame's pose page. It joins its display
/// layer in emission order like an atlas body. The concrete Unit and Aircraft
/// callers emit their shadow separately in native order.
fn emit_pose_sprite(
    state: &AppState,
    pieces: &mut Vec<ObjectPieceInstance>,
    entity: &crate::sim::game_entity::GameEntity,
    key: &UnitSpriteKey,
    tilt: crate::render::unit_atlas::PoseTilt,
    [center_x, center_y]: [f32; 2],
    z: u8,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    draw_state: DrawState,
) {
    let Some(assets) = state.process_assets.manager() else {
        return;
    };
    let Some(entry) =
        state
            .renderer
            .vxl_pose_frame_cache
            .borrow_mut()
            .render(assets, state.rules(), key, tilt)
    else {
        return;
    };
    let depth_y = center_y + entry.offset_y + entry.pixel_size[1];
    let depth = body_sort_depth(state, entity, depth_y, z);
    let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true, None) as f32;
    let bounds = composite_draw_bounds([(entry, [center_x, center_y])]);
    let (composite_rect, split) = bounds.depth_rect(false);
    pieces.push(ObjectPieceInstance {
        target: ObjectTexture::UnitPosePage(entry.page),
        render_z: RenderZPolicy::ReadOnly,
        instance: SpriteInstance {
            position: [center_x + entry.offset_x, center_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: 0,
            depth,
            tint,
            palette_light,
            alpha: 1.0,
            draw_state: unit_body_draw_state(state, entity, draw_state, split, bounds),
            z_adjust: voxel_adjust,
            z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
            zshape_origin: composite_rect,
        },
    });
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

/// Whether Drive/Ship `Draw_Matrix` gives `UnitClass::DrawVoxelBody` a turret
/// image key. Its keyed arm needs a finished slope transition and both body
/// tilt angles (`TechnoClass+0x328`, `+0x32C`) under 0.005
/// (`0x004AFFB0..0x004AFFE8`, `[0x007E44E8]`): the binary32 gate
/// [`voxel_tilt_applies`](crate::map::voxel_pose::voxel_tilt_applies) owns,
/// which [`ground_pose`] reads too. Otherwise it keys -1 (`0x004B0187`), and
/// the turret and barrel draw uncached (`0x0073B74C`, `0x007067EF`).
fn turret_images_cached(
    entity: &crate::sim::game_entity::GameEntity,
    slope_state: UnitRenderSlopeState,
) -> bool {
    matches!(slope_state, UnitRenderSlopeState::Stable(_))
        && !entity.rocking.as_ref().is_some_and(|rocking| {
            crate::map::voxel_pose::voxel_tilt_applies(rocking.angles_native())
        })
}

/// Return the native local draw order for a Unit's independently rendered
/// turret and barrel pieces.
///
/// gamemd-derived: active YR `UnitClass__DrawVoxelBody @ 0x0073B470`, selector
/// instructions `0x0073BC5B..0x0073BDAB`. Building draw43DA80 uses the same
/// rounded-quadrant order (tools/voxel_oracle/building_barrel.json).
pub(super) fn native_turret_barrel_order<T: Copy>(
    turret_facing: u16,
    turret: T,
    barrel: T,
) -> [T; 2] {
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
/// 4. Fall back to a level barrel if the pitched one isn't in the atlas.
///
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
                if let Some(entry) = atlas.get(&flat_frame0) {
                    return Some(entry);
                }
            }
        }
        // Fallback 4: the level barrel, through the same chain.
        if key.barrel_pitch != 0 {
            let level = UnitSpriteKey {
                barrel_pitch: 0,
                ..key.clone()
            };
            return atlas_get_with_frame_fallback(atlas, &level);
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
) -> Option<crate::map::voxel_pose::VxlSlopeBlend> {
    match slope_state {
        UnitRenderSlopeState::Stable(_) => None,
        UnitRenderSlopeState::Transition {
            from_slope,
            to_slope,
            phase_num,
            phase_den,
        } => Some(crate::map::voxel_pose::VxlSlopeBlend {
            from_slope,
            to_slope,
            phase_num,
            phase_den,
        }),
    }
}

fn unit_entry_for_pose(
    state: &AppState,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    key: &UnitSpriteKey,
    ground_pose: Option<crate::render::vxl_raster::GroundPose>,
) -> Option<(UnitSpriteEntry, UnitTextureSource)> {
    if let Some(pose) = ground_pose {
        let assets = state.process_assets.manager()?;
        let entry = state.renderer.vxl_pose_frame_cache.borrow_mut().render(
            assets,
            state.rules(),
            key,
            crate::render::unit_atlas::PoseTilt::Ground(pose),
        )?;
        return Some((entry, UnitTextureSource::Pose(entry.page)));
    }
    atlas_get_with_frame_fallback(atlas, key)
        .copied()
        .map(|entry| (entry, UnitTextureSource::Stable(entry.page)))
}

/// First-fill mask owner for the supported ordinary flat ground path. The
/// input entries are precisely the parts selected for this parent's draw;
/// cache hits skip their pixel work and retain the first completed mask.
fn prepare_unit_shadow(
    state: &AppState,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    body_facing: u8,
    no_shadow: bool,
    slope: UnitRenderSlopeState,
    band: EntityDrawBand,
    parts: impl IntoIterator<Item = (crate::render::unit_atlas::UnitSpriteEntry, [f32; 2])>,
) -> bool {
    if !native_shadow_caller_eligible(entity, no_shadow, slope, band) {
        return false;
    }
    let key = UnitSpriteKey {
        type_id: type_id.to_owned(),
        turret_index: 0,
        facing: canonical_unit_facing(body_facing),
        layer: VxlLayer::Shadow,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    atlas.prepare_native_shadow(&state.renderer.gpu.queue, &key, parts)
}

fn native_shadow_caller_eligible(
    entity: &crate::sim::game_entity::GameEntity,
    no_shadow: bool,
    slope: UnitRenderSlopeState,
    band: EntityDrawBand,
) -> bool {
    if !unit_shadow_admitted(entity, no_shadow, band)
        || !entity
            .locomotor
            .as_ref()
            .is_some_and(|l| l.active_kind() == crate::rules::locomotor_type::LocomotorKind::Drive)
        || !matches!(slope, UnitRenderSlopeState::Stable(0))
    {
        return false;
    }
    true
}

/// Unit73C5C4 -> Foot4DB0D0 -> Techno706BD0: raw +220 must be
/// zero (706BDD), and TechnoType+D98 NoShadow must be false (706BF3).
/// A real StartCloaking703799/+224=0 is still character0; presentation FX
/// therefore cannot supply this decision. This owner serves cache and emit.
/// Native caller references: tools/procedural_drawing_oracle/translucent_blitter_a.md.
fn unit_shadow_admitted(
    entity: &crate::sim::game_entity::GameEntity,
    no_shadow: bool,
    band: EntityDrawBand,
) -> bool {
    entity.category() == EntityCategory::Unit
        && !entity.sinking.is_active()
        && (band == EntityDrawBand::Ground || has_jumpjet_unit_shadow(entity))
        && techno_shadow_admitted(entity, no_shadow)
}

fn has_jumpjet_unit_shadow(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.category() == EntityCategory::Unit
        && entity.locomotor.as_ref().is_some_and(|l| {
            l.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
        })
}

fn techno_shadow_admitted(entity: &crate::sim::game_entity::GameEntity, no_shadow: bool) -> bool {
    !no_shadow && entity.cloak.as_ref().is_none_or(|cloak| cloak.state() == 0)
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
fn emit_unit_shadow_sprite(
    native_shadow: bool,
    no_shadow: bool,
    voxel_adjust: f32,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    body_facing: u8,
    center_x: f32,
    center_y: f32,
    depth: f32,
    draw_state: DrawState,
    slope_state: UnitRenderSlopeState,
    band: EntityDrawBand,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    // Original Unit73C1D2 also skips its shadow suffix while sinking +3CD.
    if !unit_shadow_admitted(entity, no_shadow, band) || has_jumpjet_unit_shadow(entity) {
        return;
    }
    let key = UnitSpriteKey {
        type_id: type_id.to_string(),
        turret_index: 0,
        facing: canonical_unit_facing(body_facing),
        layer: VxlLayer::Shadow,
        frame: 0,
        slope_type: stable_slope_for_key(slope_state),
        barrel_pitch: 0,
    };
    let lookup_key = shadow_lookup_key(key.clone(), native_shadow);
    let Some(entry) = atlas.get(&lookup_key).or_else(|| atlas.get(&key)).copied() else {
        return;
    };
    emit_shadow_stencil(
        entry,
        UnitTextureSource::Stable(entry.page),
        [center_x, center_y],
        depth,
        voxel_adjust,
        draw_state,
        pieces,
    );
}

/// Shared Techno706BD0 stencil emission. Aircraft4148A0 supplies no composed
/// body mask, so its immutable atlas entry also serves pitched/rolled bodies.
fn emit_shadow_stencil(
    entry: UnitSpriteEntry,
    source: UnitTextureSource,
    anchor: [f32; 2],
    depth: f32,
    voxel_adjust: f32,
    draw_state: DrawState,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let mut shadow_state: DrawState = draw_state;
    shadow_state.fx_flags |= FX_SHADOW;
    let sprite = SpriteInstance {
        position: [anchor[0] + entry.offset_x, anchor[1] + entry.offset_y],
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
    push_unit_sprite(source, sprite, pieces);
}

/// The original Unit suffix follows stable, split-gun and posed bodies alike.
/// Its composed mask reads those exact pieces; a high-flight stencil is
/// unmasked, and positive low flight uses the existing frame-local pages.
fn emit_jumpjet_unit_shadow(
    state: &AppState,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    frame: u32,
    body_anchor: [f32; 2],
    no_shadow: bool,
    draw_state: DrawState,
    band: EntityDrawBand,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    use crate::render::unit_shadow::{UnitShadowContext, UnitShadowMask};
    if !has_jumpjet_unit_shadow(entity)
        || !unit_shadow_admitted(entity, no_shadow, band)
        || pieces.is_empty()
    {
        return;
    }
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let view = runtime.view();
    let Some(terrain) = view.resolved_terrain() else {
        return;
    };
    let xy = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
    let Some(mut pose) = crate::render::unit_shadow::pose(
        UnitShadowContext {
            xyz: [
                xy[0],
                xy[1],
                crate::sim::movement::ground_pose::object_world_z_leptons(entity, Some(terrain)),
            ],
            on_bridge: entity.on_bridge(),
            marked: entity.lifecycle.cell_marked(),
            primary_facing: entity.body_facing_current(view.session().binary_frame()),
        },
        |coord| super::foot_depth::depth_cell(terrain, None, None, coord),
    ) else {
        return;
    };
    pose.anchor[1] -= f32::from(entity.ion_blast_lift_pixels());
    let key = UnitSpriteKey {
        type_id: type_id.to_owned(),
        turret_index: 0,
        facing: pose.facing,
        layer: VxlLayer::Shadow,
        frame: if pose.mask == UnitShadowMask::Retained {
            frame | crate::render::unit_atlas::GROUNDED_SHADOW_FRAME_FLAG
        } else {
            frame
        },
        slope_type: pose.slope,
        barrel_pitch: 0,
    };
    let Some(entry) = atlas
        .get(&key)
        .filter(|e| e.native_draw_bounds.is_some())
        .copied()
    else {
        return;
    };
    let (entry, source) = if pose.mask == UnitShadowMask::None {
        (entry, UnitTextureSource::Stable(entry.page))
    } else {
        let parts: Vec<_> = pieces
            .iter()
            .filter_map(|piece| {
                let (page, posed) = match piece.target {
                    ObjectTexture::UnitAtlasPage(page) => (page, false),
                    ObjectTexture::UnitPosePage(page) => (page, true),
                    _ => return None,
                };
                let sprite = &piece.instance;
                Some(crate::render::unit_atlas::UnitShadowPart {
                    entry: UnitSpriteEntry {
                        uv_origin: sprite.uv_origin,
                        uv_size: sprite.uv_size,
                        pixel_size: sprite.size,
                        offset_x: 0.,
                        offset_y: 0.,
                        native_draw_bounds: None,
                        page,
                    },
                    point: [
                        128 + (sprite.position[0] - body_anchor[0]) as i32,
                        128 + (sprite.position[1] - body_anchor[1]) as i32,
                    ],
                    posed,
                })
            })
            .collect();
        let mut cache = state.renderer.vxl_pose_frame_cache.borrow_mut();
        if pose.mask == UnitShadowMask::Retained {
            if !atlas.prepare_native_shadow_with(&state.renderer.gpu.queue, &key, |body| {
                cache.stamp_shadow_parts(atlas, &parts, body)
            }) {
                return;
            }
            (entry, UnitTextureSource::Stable(entry.page))
        } else {
            let Some(entry) = cache.render_unit_shadow(atlas, &key, pose.mask_offset, &parts)
            else {
                return;
            };
            (entry, UnitTextureSource::Pose(entry.page))
        }
    };
    let depth = body_sort_depth(
        state,
        entity,
        pose.anchor[1] + entry.offset_y + entry.pixel_size[1],
        entity.position.z(),
    );
    let z_adjust = super::foot_depth::unit_z_adjust(state, entity, true, None);
    emit_shadow_stencil(
        entry,
        source,
        pose.anchor,
        depth,
        z_adjust as f32,
        draw_state,
        pieces,
    );
}

struct AircraftDrawSample {
    shadow: Option<crate::render::aircraft_shadow::AircraftShadowPose>,
    pose: crate::app::presentation::interpolation::RenderPose,
}

fn aircraft_draw_sample(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
) -> Option<AircraftDrawSample> {
    if entity.category() != EntityCategory::Aircraft || entity.sinking.is_active() {
        return None;
    }
    let runtime = state.match_state.sim_runtime.as_ref()?;
    let view = runtime.view();
    let object = view
        .interner()
        .object_type(entity.type_ref(), &runtime.resources.rules)?;
    if !techno_shadow_admitted(entity, object.no_shadow) {
        return None;
    }
    let terrain = view.resolved_terrain()?;
    let xy = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
    let frame = view.session().binary_frame();
    let (sampled, xyz) = state
        .match_state
        .match_presentation
        .interpolation
        .sample_entity_and_coord(
            entity,
            frame,
            [
                xy[0],
                xy[1],
                crate::sim::movement::ground_pose::object_world_z_leptons(entity, Some(terrain)),
            ],
        );
    // Keep the pose even if this locomotor has no shadow. The old path had
    // already read it here; a later admitted body need not request it again.
    let shadow = entity.locomotor.as_ref().and_then(|locomotor| {
        crate::render::aircraft_shadow::pose(
            crate::render::aircraft_shadow::AircraftShadowContext {
                xyz,
                on_bridge: entity.on_bridge(),
                locomotor: locomotor.active_kind(),
                is_dropship: object.is_dropship(),
                primary_facing: sampled.body,
                secondary_facing: sampled.turret,
            },
            |coord| super::foot_depth::depth_cell(terrain, None, None, coord),
        )
    });
    Some(AircraftDrawSample {
        shadow,
        pose: sampled,
    })
}

fn emit_aircraft_shadow_sprite(
    state: &AppState,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    frame: u32,
    pose: crate::render::aircraft_shadow::AircraftShadowPose,
    draw_state: DrawState,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let key = UnitSpriteKey {
        type_id: type_id.to_owned(),
        turret_index: 0,
        facing: pose.facing,
        layer: VxlLayer::Shadow,
        frame,
        slope_type: pose.slope,
        barrel_pitch: 0,
    };
    let Some(entry) = atlas
        .get(&key)
        .filter(|entry| entry.native_draw_bounds.is_some())
        .copied()
    else {
        return;
    };
    let depth = compute_sprite_depth(
        state,
        pose.anchor[1] + entry.offset_y + entry.pixel_size[1],
        0,
    );
    let z_adjust = super::foot_depth::unit_z_adjust(state, entity, false, Some(pose.world_z));
    emit_shadow_stencil(
        entry,
        UnitTextureSource::Stable(entry.page),
        pose.anchor,
        depth,
        z_adjust as f32,
        draw_state,
        pieces,
    );
}

fn push_unit_sprite(
    texture_source: UnitTextureSource,
    sprite: SpriteInstance,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let target = match texture_source {
        UnitTextureSource::Pose(page) => ObjectTexture::UnitPosePage(page),
        UnitTextureSource::Stable(page) => ObjectTexture::UnitAtlasPage(page),
    };
    pieces.push(ObjectPieceInstance {
        target,
        render_z: RenderZPolicy::ReadOnly,
        instance: sprite,
    });
}

fn unit_draw_anchor(
    entity: &crate::sim::game_entity::GameEntity,
    camera: [f32; 2],
    viewport: [f32; 2],
    shadow_anchor: Option<[f32; 2]>,
) -> Option<[f32; 2]> {
    let (x, y) = crate::render::locomotor_visual::screen_position(entity);
    // Unit draw admission precedes stateful +3CA capture. A smaller visual
    // cull delays that first capture and changes later visible clipping.
    let admitted = if entity.category() == EntityCategory::Unit {
        projection_admitted([x, y], camera, viewport)
    } else {
        in_view(
            x,
            y,
            TILE_WIDTH,
            TILE_HEIGHT,
            camera[0],
            camera[1],
            viewport[0],
            viewport[1],
            120.0,
        ) || shadow_anchor.is_some_and(|[sx, sy]| {
            in_view(
                sx,
                sy,
                TILE_WIDTH,
                TILE_HEIGHT,
                camera[0],
                camera[1],
                viewport[0],
                viewport[1],
                120.0,
            )
        })
    };
    admitted.then_some([x, y - f32::from(entity.ion_blast_lift_pixels())])
}

/// One parent raster has separate atlas storage and native drawing bounds.
/// Unit73BEA4's waterline and 73B140's split both consume the native union;
/// atlas padding must not move either boundary. Unsplit depth retains its
/// existing storage rectangle until that wider depth path is audited.
#[derive(Clone, Copy)]
struct CompositeDrawBounds {
    padded: [f32; 2],
    native: Option<[i32; 4]>,
}

impl CompositeDrawBounds {
    fn unit_waterline(
        self,
        cache: &mut crate::render::sinking::SinkingWaterlines,
        id: u64,
        sinking: bool,
    ) -> Option<i16> {
        if let Some([_, y, _, height]) = self.native {
            cache.unit_draw(id, sinking, y.wrapping_add(height))
        } else {
            // Unsupported fallback geometry cannot establish a new native
            // rectangle; an already retained +3CA still clips its body.
            cache.retained_clip(id)
        }
    }

    fn depth_rect(self, wants_split: bool) -> ([f32; 2], bool) {
        if wants_split && let Some([_, y, _, height]) = self.native {
            return ([y as f32, height as f32], true);
        }
        (self.padded, false)
    }
}

fn composite_draw_bounds(
    parts: impl IntoIterator<Item = (UnitSpriteEntry, [f32; 2])>,
) -> CompositeDrawBounds {
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
    CompositeDrawBounds {
        padded: if bottom > top {
            [top, bottom - top]
        } else {
            [0.0, 0.0]
        },
        native: all_native.then_some(native).flatten(),
    }
}

fn unit_body_draw_state(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
    draw_state: DrawState,
    split: bool,
    bounds: CompositeDrawBounds,
) -> DrawState {
    let mut draw_state = composite_draw_state(state, draw_state, split);
    // The final parent rectangle is known only after all selected voxel parts
    // have been admitted. Never capture from an intermediate/offscreen part.
    if entity.category() == EntityCategory::Unit {
        let mut cache = state
            .match_state
            .match_presentation
            .sinking_waterlines
            .borrow_mut();
        let clip =
            bounds.unit_waterline(&mut cache, entity.stable_id(), entity.sinking.is_active());
        crate::render::sinking::apply_waterline_clip(&mut draw_state, clip);
    }
    draw_state
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
    no_shadow: bool,
    atlas: &crate::render::unit_atlas::UnitAtlas,
    art_reg: Option<&crate::rules::art_data::ArtRegistry>,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    body_facing: u8,
    turret_facing: u16,
    turret_index: i32,
    barrel_elevation: u16,
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
    band: EntityDrawBand,
    mut ground_pose: Option<crate::render::vxl_raster::GroundPose>,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let slope_type = stable_slope_for_key(slope_state);
    let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true, None) as f32;
    let body_key = UnitSpriteKey {
        type_id: type_id.to_string(),
        turret_index: 0,
        facing: canonical_unit_facing(body_facing),
        layer: VxlLayer::Body,
        frame: anim_frame,
        slope_type,
        barrel_pitch: 0,
    };
    let turret_key = UnitSpriteKey {
        type_id: type_id.to_string(),
        turret_index,
        facing: canonical_turret_facing(turret_facing),
        layer: VxlLayer::Turret,
        frame: turret_frame,
        slope_type,
        barrel_pitch: 0,
    };
    let mut barrel_key = UnitSpriteKey {
        type_id: type_id.to_string(),
        turret_index,
        facing: canonical_turret_facing(turret_facing),
        layer: VxlLayer::Barrel,
        frame: anim_frame,
        slope_type,
        barrel_pitch: 0,
    };

    // Look up TurretOffset from art.ini and compute screen-space shift.
    let art_offset: i32 = art_reg
        .and_then(|a| a.get(type_id))
        .map(|e| e.turret_offset)
        .unwrap_or(0);
    // Use the same stable or quaternion-SLERP slope matrix as every hull,
    // turret, and barrel raster layer in this presentation phase.
    if let Some(pose) = ground_pose.as_mut() {
        pose.turret_offset = art_offset;
    }
    let (tur_ox, tur_oy) = if ground_pose.is_some() {
        (0., 0.)
    } else {
        turret_screen_offset(art_offset, body_facing, slope_state)
    };

    // The barrel pitches by its elevation's step (`d32 - 8`, `0x0073BB85..
    // 0x0073BB96`). A pose the turret image is cached under draws the pitch its
    // type cached there; an uncached pose draws the unit's own.
    let drawn_pitch = crate::render::vxl_raster::voxel_facing_step_u16(barrel_elevation) as i8 - 8;
    barrel_key.barrel_pitch = if ground_pose.is_none() && turret_images_cached(entity, slope_state)
    {
        state
            .match_state
            .match_presentation
            .barrel_image_pitches
            .borrow_mut()
            .pitch(
                &turret_key,
                (art_offset != 0).then_some(canonical_unit_facing(body_facing)),
                drawn_pitch,
            )
    } else {
        drawn_pitch
    };
    let (barrel_ox, barrel_oy) = if ground_pose.is_some() {
        (0., 0.)
    } else {
        let (x, y) = crate::render::vxl_raster::barrel_pivot_screen_offset(
            art_offset,
            body_facing,
            barrel_key.facing,
            slope_type,
            slope_blend_for_render_state(slope_state),
            barrel_key.barrel_pitch,
        );
        (tur_ox + x, tur_oy + y)
    };

    // All layers of a turreted unit share one depth so insertion order
    // (body, then turret/barrel) controls visual stacking via stable sort.
    // Per-layer depth derived from each sprite's bounding box caused tie-break
    // collisions where body could sort over turret at certain facings.
    let body_entry_opt = unit_entry_for_pose(state, atlas, &body_key, ground_pose);
    let entity_depth_y: f32 = match body_entry_opt {
        Some((e, _)) => center_y + e.offset_y + e.pixel_size[1] + dock_depth_y_offset,
        None => center_y + dock_depth_y_offset,
    };
    let entity_depth: f32 = body_sort_depth(state, entity, entity_depth_y, z);

    // Emit body first (always). Uses frame fallback for mismatched HVA counts.
    // Natively hull, turret and barrel are composited off-screen and blitted
    // as ONE rect (`UnitClass vtable+0x55C = 0x0073B140`), so all three share
    // the entry-2 seed of the composite rect: its top and height ride
    // `zshape_origin` (the voxel shader's `z_rect`).
    let turret_layers: Vec<_> = native_turret_barrel_order(
        turret_facing,
        (&turret_key, [tur_ox, tur_oy]),
        (&barrel_key, [barrel_ox, barrel_oy]),
    )
    .into_iter()
    .filter_map(|(key, offset)| {
        unit_entry_for_pose(state, atlas, key, ground_pose)
            .map(|(entry, source)| (entry, source, offset))
    })
    .collect();
    let native_shadow = body_entry_opt
        .is_some_and(|(_, source)| matches!(source, UnitTextureSource::Stable(_)))
        && turret_layers
            .iter()
            .all(|(_, source, _)| matches!(source, UnitTextureSource::Stable(_)))
        && prepare_unit_shadow(
            state,
            atlas,
            entity,
            type_id,
            body_facing,
            no_shadow,
            slope_state,
            band,
            body_entry_opt
                .into_iter()
                .map(|(entry, _)| (entry, [0.0, 0.0]))
                .chain(
                    turret_layers
                        .iter()
                        .map(|(entry, _, offset)| (*entry, *offset)),
                ),
        );
    if !native_shadow {
        emit_unit_shadow_sprite(
            native_shadow,
            no_shadow,
            voxel_adjust,
            atlas,
            entity,
            type_id,
            body_facing,
            center_x,
            center_y,
            entity_depth,
            draw_state,
            slope_state,
            band,
            pieces,
        );
    }
    let bounds = composite_draw_bounds(
        body_entry_opt
            .into_iter()
            .map(|(entry, _)| (entry, [center_x, center_y]))
            .chain(
                turret_layers
                    .iter()
                    .map(|(entry, _, [ox, oy])| (*entry, [center_x + ox, center_y + oy])),
            ),
    );
    let (composite_rect, split) =
        bounds.depth_rect(super::foot_depth::unit_bridge_split(state, entity, true));
    let body_draw_state = unit_body_draw_state(state, entity, draw_state, split, bounds);
    if let Some((entry, texture_source)) = body_entry_opt {
        let sprite = SpriteInstance {
            position: [center_x + entry.offset_x, center_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: 0,
            depth: entity_depth,
            tint,
            palette_light,
            alpha,
            draw_state: body_draw_state,
            z_adjust: voxel_adjust,
            z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
            zshape_origin: composite_rect,
        };
        push_unit_sprite(texture_source, sprite, pieces);
    }

    for (entry, texture_source, [ox, oy]) in turret_layers {
        let sprite = SpriteInstance {
            position: [
                center_x + entry.offset_x + ox,
                center_y + entry.offset_y + oy,
            ],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: 0,
            depth: entity_depth,
            tint,
            palette_light,
            alpha,
            draw_state: body_draw_state,
            z_adjust: voxel_adjust,
            z_gradient: pack_voxel_z_gradient(ZGradient::Vertical, split),
            zshape_origin: composite_rect,
        };
        push_unit_sprite(texture_source, sprite, pieces);
    }
    if native_shadow {
        emit_unit_shadow_sprite(
            native_shadow,
            no_shadow,
            voxel_adjust,
            atlas,
            entity,
            type_id,
            body_facing,
            center_x,
            center_y,
            entity_depth,
            draw_state,
            slope_state,
            band,
            pieces,
        );
    }
}

/// `UnitClass::DrawIt @ 0x0073CEC0`'s OREGATH.SHP frame, when it draws one: a
/// `Harvester=` type (`+0xE0E` of the unit's own type `+0x6C4`) whose unit is
/// harvesting (`+0x6D2`), whose locomotor is not moving now (`[loco+0x80]` at
/// `0x0073D114`: Drive `0x004AFC20`, rotating or moving with speed), and that
/// no Temporal weapon is warping (`+0x278`), warping out (vt+0x1D4) or warping
/// in (vt+0x1D8) (`0x0073D0DB..0x0073D221`). So a miner hopping to its next
/// ore cell shows none.
fn harvest_overlay_frame(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    display_binary_frame: u32,
) -> Option<u16> {
    let harvester = rules
        .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules))
        .is_some_and(|object| object.harvester);
    if !harvester
        || !entity
            .miner
            .as_ref()
            .is_some_and(|miner| miner.harvesting())
        || entity.temporal.chain_head().is_some()
        || entity.is_warped_out()
        || entity.is_warping_in()
        || rules.is_some_and(|rules| {
            crate::sim::movement::motion_query::is_moving_now(
                entity,
                crate::sim::movement::SpeedRules::new(rules, sim.interner(), sim.houses()),
                display_binary_frame,
            )
        })
    {
        return None;
    }
    let frame = sim.session().binary_frame();
    oregath_frame(
        entity.body_frame_counter.get(),
        frame,
        entity.body_facing_current(frame),
    )
}

/// OREGATH.SHP's frame (`0x0073D1E2..0x0073D273`): `(Foot+0x538 + Frame) % 15`,
/// a signed remainder, in the row `7 - ((((Current >> 12) + 1) >> 1) & 7)` of
/// the body facing (`+0x388`), 15 frames to a row. A negative index draws
/// nothing.
fn oregath_frame(body_frame_counter: u32, frame: u32, body_facing: u16) -> Option<u16> {
    let within = (body_frame_counter as i32).wrapping_add(frame as i32) % 15;
    let row = 7 - i32::from(crate::util::direction_tables::dir_from_facing16(
        body_facing,
    ));
    u16::try_from(row * 15 + within).ok()
}

/// `UnitClass::DrawIt`'s OREGATH point (`0x0073D122..0x0073D1D6`): the render
/// coordinate (vt+0xAC, a unit's Location) moved 30 leptons (double
/// `0x007F61D0`) along its body facing rounded to eight directions,
/// `(((Current >> 12) + 1) >> 1) << 13` (`0x0073D134..0x0073D145`), then
/// projected by `CoordsToClient2` (`0x0073D1D6`). The step is Walk's
/// instruction for instruction, with 30 for its speed.
fn harvest_arm_screen_position(
    entity: &crate::sim::game_entity::GameEntity,
    frame: u32,
) -> (f32, f32) {
    let current = entity.body_facing_current(frame);
    // `MOVSX` takes the low word, so a heading from 0xF000 rounds up to north.
    let eighth = u16::from(crate::util::direction_tables::dir_from_facing16(current)) << 13;
    crate::render::locomotor_visual::facing_step_screen_position(entity, eighth, 30)
}

/// Emit the oregath.shp harvest overlay sprite at `frame`
/// ([`harvest_overlay_frame`]) on the arm point `arm`
/// ([`harvest_arm_screen_position`]), from the sprite atlas (keyed as
/// "OREGATH").
fn emit_harvest_overlay(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
    frame: u16,
    arm: (f32, f32),
    z: u8,
    tint: [f32; 3],
    brightness: i32,
    draw_state: DrawState,
) -> Option<(usize, SpriteInstance)> {
    let sprite_atlas = state.match_state.match_presentation.sprite_atlas.as_ref()?;
    let key = ShpSpriteKey {
        palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
        type_id: "OREGATH".to_string(),
        facing: 0,
        frame,
        house_color: HouseColorIndex::default(),
    };
    let entry = sprite_atlas.get(&key)?;
    let page = entry.page as usize;
    let (draw_x, draw_y) = arm;
    let depth_y: f32 = draw_y + entry.offset_y + entry.pixel_size[1];
    let depth: f32 = compute_sprite_depth(state, depth_y, z);
    Some((
        page,
        SpriteInstance {
            position: [draw_x + entry.offset_x, draw_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: entry.source_palette,
            depth,
            tint,
            // Unit DrawExtras 0073D276 selects global ANIM Convert, not the
            // unit ColorScheme. Ordinary scalar is top + ExtraUnitLight.
            // Its bridge/special-cell brightness branches remain unresolved.
            palette_light: crate::render::palette_light::PaletteLight::plain(53, brightness),
            alpha: 1.0,
            draw_state,
            // An SHP draw of the harvester (`TechnoClass_DrawSHP`, a7 - 2).
            z_adjust: super::foot_depth::unit_z_adjust(state, entity, false, None)
                .wrapping_add(SHP_DRAW_Z_ADJUST_PX) as f32,
            z_gradient: pack_z_gradient(ZGradient::Vertical, false),
            ..Default::default()
        },
    ))
}

/// Native VXL blits walk entry 2 with the Foot adjustment supplied by the
/// shared production adapter. Hull, turret and barrel retain one composite seed.
const VOXEL_Z_GRADIENT: u32 = pack_z_gradient(ZGradient::Vertical, false);

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
    use crate::render::unit_atlas::PoseTilt;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::InternedId;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::spawn_manager::{SpawnManagerState, SpawnSlot, SpawnSlotState};
    use crate::sim::timer::CdTimer;
    use crate::sim::world::Simulation;

    /// OREGATH's row rounds the body facing to eight directions, `0x1000` on
    /// either side of each (`0x0073D1E7..0x0073D1FD`); the frame within the
    /// row is `(Foot+0x538 + Frame) % 15`, signed (`0x0073D25C..0x0073D273`).
    #[test]
    fn oregath_frame_rounds_the_facing_row_and_adds_the_global_frame() {
        assert_eq!(oregath_frame(0, 0, 0x0000), Some(105));
        assert_eq!(oregath_frame(0, 0, 0x0FFF), Some(105));
        assert_eq!(oregath_frame(0, 0, 0x1000), Some(90));
        assert_eq!(oregath_frame(0, 0, 0x2FFF), Some(90));
        assert_eq!(oregath_frame(0, 0, 0x3000), Some(75));
        assert_eq!(oregath_frame(0, 0, 0xEFFF), Some(0));
        assert_eq!(oregath_frame(0, 0, 0xF000), Some(105));
        // (7 + 20) % 15 in row 6.
        assert_eq!(oregath_frame(7, 20, 0x2000), Some(102));
        // A negative sum leaves a negative remainder, which row 0 cannot hold.
        assert_eq!(oregath_frame(u32::MAX - 20, 0, 0xE000), None);
    }

    /// All51 active RGB565 rows execute original Unit73BF7B..73C166.
    /// Production Rules readers supply the stock table and per-row Iron index;
    /// the same material phase used by build_unit_instances must preserve the
    /// native intensity, palette rows and low16 caller/Iron/Berserk OR word.
    /// Native formats0/1 remain outside the active RGB565 renderer's scope.
    #[test]
    fn the_curtain_arm_matches_native() {
        let oracle: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json"))
                .unwrap();
        let rows = oracle["curtain_draw_arm"].as_array().unwrap();
        assert_eq!(rows.len(), 55);
        let int = |value: &serde_json::Value| value.as_i64().unwrap() as i32;
        let mut sim = Simulation::new();
        let tint = [0.6, 0.9, 1.2];
        let mut rules_by_iron_index = std::collections::BTreeMap::new();
        for index in [0, 4, 9] {
            let patch = crate::rules::ini_parser::IniFile::from_str(&format!(
                "[AudioVisual]\nIronCurtainColor={index}\n"
            ));
            let Some(battle) =
                crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
                    "XMP03T4.MAP",
                    &patch,
                )
            else {
                return;
            };
            rules_by_iron_index.insert(index, battle.rules);
        }
        let mut compared = 0;
        for row in rows {
            if int(&row["pixel_format"]) != 2 {
                continue;
            }
            let mut unit = GameEntity::test_default(1, "HTNK", "Russians", 10, 10);
            unit.berserk
                .set_active_for_test(row["berzerk"].as_bool().unwrap());
            unit.flashing.start(int(&row["flash"]));
            unit.deactivated
                .set_for_test(row["deactivated"].as_bool().unwrap());
            unit.invulnerability = Some(
                crate::sim::superweapon::invulnerability::InvulnerabilityState::with_tint(
                    CdTimer::from_raw(int(&row["curtain"][0]), int(&row["curtain"][1])),
                    crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
                    u8::try_from(int(&row["stage"])).unwrap(),
                    CdTimer::from_raw(int(&row["tint"][0]), int(&row["tint"][1])),
                ),
            );
            let intensity = int(&row["intensity"]);
            let light =
                crate::render::palette_light::PaletteLight::color_scheme([1000; 3], intensity)
                    .with_colour_word(int(&row["word"]) as u16);
            sim.session_mut_for_test()
                .set_binary_frame_for_test(int(&row["frame"]) as u32);
            let (lit_tint, lit) =
                crate::app::presentation::lighting::techno_effect_light(&unit, tint, light, &sim);
            let (lit_tint, lit) = crate::app::presentation::lighting::unit_voxel_effect_light(
                &unit,
                lit_tint,
                lit,
                Some(&rules_by_iron_index[&int(&row["iron_color"])]),
                sim.session().binary_frame(),
            );
            let out = int(&row["out_intensity"]);
            assert_eq!(lit.brightness(), out, "{row}");
            assert_eq!(lit.rows(), light.rows(), "{row}");
            let ratio = out as f32 / intensity as f32;
            assert_eq!(lit_tint, tint.map(|channel| channel * ratio), "{row}");
            assert_eq!(
                u64::from(lit.colour_word()),
                row["out_word"].as_u64().unwrap() & 0xFFFF,
                "{row}"
            );
            compared += 1;
        }
        assert_eq!(compared, 51);
    }

    /// Retail `[AudioVisual] IronCurtainColor=0;4` reads 0 through ReadInt
    /// (`0x0066B84C`; the INI loader drops the `;` comment), and `[ColorAdd]`'s
    /// first entry, None, is black: the curtain ORs nothing into a retail
    /// unit's pixels.
    #[test]
    fn retail_curtain_colour_adds_nothing() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let index = ini
            .section("AudioVisual")
            .unwrap()
            .read_int("IronCurtainColor", i32::MIN);
        assert_eq!(index, 0);
        let colors = crate::rules::color_add::ColorAddTable::from_ini(&ini);
        assert_eq!(colors.entries()[0].name.as_deref(), Some("None"));
        assert_eq!(colors.entries()[0].rgb, [0, 0, 0]);
    }

    #[test]
    fn jumpjet_unit_shadow_admission_matches_original_gates_across_bands() {
        let fixture = crate::test_fixture::json("tools/voxel_oracle/unit_shadow.json");
        for row in fixture["callers"].as_array().unwrap() {
            let mut entity = GameEntity::test_default(1, "ZEP", "Russians", 10, 10);
            entity.set_category_for_test(EntityCategory::Unit);
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
            let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
            cloak.set_state_for_test(row["input"]["cloak"].as_i64().unwrap_or(0) as i32);
            entity.cloak = Some(cloak);
            if row["input"]["sinking"].as_bool().unwrap_or(false) {
                continue;
            }
            for band in [
                EntityDrawBand::Ground,
                EntityDrawBand::Air,
                EntityDrawBand::Top,
            ] {
                assert_eq!(
                    unit_shadow_admitted(
                        &entity,
                        row["input"]["no_shadow"].as_bool().unwrap_or(false),
                        band
                    ),
                    row["draw_admitted"].as_bool().unwrap(),
                    "{row}"
                );
            }
        }
    }

    #[test]
    fn vehicle_shadow_active_locomotor_selects_native_or_legacy_companion() {
        let mut entity = GameEntity::test_default(1, "GENERATED", "Americans", 0, 0);
        entity.set_category_for_test(EntityCategory::Unit);
        let key = UnitSpriteKey {
            type_id: "GENERATED".into(),
            turret_index: 0,
            facing: 0,
            layer: VxlLayer::Shadow,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
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
                false,
                UnitRenderSlopeState::Stable(slope),
                EntityDrawBand::Ground,
            );
            assert_eq!(
                shadow_lookup_key(key.clone(), eligible).frame,
                expected_frame
            );
        }
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
        cloak.set_state_for_test(1);
        entity.cloak = Some(cloak);
        assert!(!native_shadow_caller_eligible(
            &entity,
            false,
            UnitRenderSlopeState::Stable(0),
            EntityDrawBand::Ground
        ));
    }

    #[test]
    fn fly_body_draw_uses_the_executed_secondary_facing_cache_key() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/aircraft_shadow.json",
        ))
        .unwrap();
        let mut compared = 0;
        for case in fixture["callers"].as_array().unwrap() {
            let Some(native_key) = case["body_cache_key"].as_u64() else {
                continue;
            };
            let input = &case["input"];
            let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
            entity.set_category_for_test(EntityCategory::Aircraft);
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));
            entity
                .body_facing
                .snap(input["primary"].as_u64().unwrap() as u16, 0);
            entity.ensure_fly_secondary_facing();
            entity
                .barrel_facing
                .as_mut()
                .unwrap()
                .snap(input["secondary"].as_u64().unwrap() as u16, 0);
            assert_eq!(
                crate::render::vxl_raster::voxel_facing_step_u16(
                    crate::app::presentation::interpolation::body_draw_facing(&entity, 0)
                ) * 8,
                native_key as u8,
                "{input}"
            );
            compared += 1;
        }
        assert_eq!(compared, 4);
    }

    /// A crashing body takes its pose before the turret split, whether or not
    /// its model has turret parts. A Fly's Secondary facing in `barrel_facing`
    /// is not a turret: its model has none.
    #[test]
    fn a_crashing_body_takes_its_pose_before_the_turret_split() {
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 0, 0);
        entity.set_category_for_test(EntityCategory::Aircraft);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));
        entity.ensure_fly_secondary_facing();
        assert_eq!(
            body_draw(&entity, EntityDrawBand::Top, 0, false, false),
            BodyDraw::Composite
        );
        entity.crash.set_crashing_for_test(true);
        entity.rocking = Some({
            let mut value = crate::sim::rocking::RockingState::default();
            value.set_angle_sideways(crate::util::fixed_math::SimFixed::from_num(0.5));
            value.set_angle_forwards_for_test(crate::util::fixed_math::SimFixed::from_num(-0.25));
            value
        });
        for turret_parts in [false, true] {
            assert_eq!(
                body_draw(&entity, EntityDrawBand::Top, 0, false, turret_parts),
                BodyDraw::Pose(PoseTilt::Fly([0.5, -0.25]))
            );
        }
        // Only the airborne (Top) Fly body is posed.
        assert_eq!(
            body_draw(&entity, EntityDrawBand::Ground, 0, false, false),
            BodyDraw::Composite
        );
        // A Jumpjet tilts only for `TiltCrashJumpjet=`, then in any band.
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
        assert!(matches!(
            body_draw(&entity, EntityDrawBand::Top, 0, false, true),
            BodyDraw::Turret { .. }
        ));
        assert_eq!(
            body_draw(&entity, EntityDrawBand::Ground, 0, true, true),
            BodyDraw::Pose(PoseTilt::Jumpjet([0.5, -0.25]))
        );
        // Under 0.005 on both axes the Jumpjet draws its plain facing matrix.
        entity.rocking = Some({
            let mut value = crate::sim::rocking::RockingState::default();
            value.set_angle_sideways(crate::util::fixed_math::SimFixed::from_num(0.004));
            value.set_angle_forwards_for_test(crate::util::fixed_math::SimFixed::from_num(-0.004));
            value
        });
        assert!(matches!(
            body_draw(&entity, EntityDrawBand::Top, 0, true, true),
            BodyDraw::Turret { .. }
        ));
    }

    #[test]
    fn ground_pose_reads_drive_and_ship_roll_or_slope_without_replacing_other_locomotors() {
        for ground_kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
            let mut entity = GameEntity::test_default(1, "DEST", "Americans", 0, 0);
            entity.locomotor = Some(LocomotorState::for_test_kind(ground_kind));
            assert!(ground_pose(&entity, UnitRenderSlopeState::Stable(0), 0).is_none());
            let transition = UnitRenderSlopeState::Transition {
                from_slope: 0,
                to_slope: 1,
                phase_num: 1,
                phase_den: 3,
            };
            let pose = ground_pose(&entity, transition, 0)
                .expect("slope transition is uncached even without rocking");
            assert_eq!(pose.angles, [0.; 2]);
            assert_eq!(
                pose.slope_blend.unwrap(),
                crate::map::voxel_pose::VxlSlopeBlend {
                    from_slope: 0,
                    to_slope: 1,
                    phase_num: 1,
                    phase_den: 3,
                }
            );
            entity.rocking = Some(crate::sim::rocking::RockingState::default());
            for roll in [-0.5, 0.5] {
                entity
                    .rocking
                    .as_mut()
                    .unwrap()
                    .set_angle_sideways(crate::util::fixed_math::SimFixed::from_num(roll));
                let pose = ground_pose(&entity, UnitRenderSlopeState::Stable(0), 0).unwrap();
                assert_eq!(pose.angles, [roll, 0.]);
                assert!(pose.slope_blend.is_none());
                assert!(!turret_images_cached(
                    &entity,
                    UnitRenderSlopeState::Stable(0)
                ));
                // Both dynamic ground bodies retain the existing untilted shadow
                // owner (Drive4B0410 / Ship69FB20 → base55A7D0).
                assert!(unit_shadow_admitted(&entity, false, EntityDrawBand::Ground));
                for kind in [
                    LocomotorKind::Fly,
                    LocomotorKind::Jumpjet,
                    LocomotorKind::Rocket,
                ] {
                    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
                    assert!(ground_pose(&entity, transition, 0).is_none());
                }
                entity.locomotor = Some(LocomotorState::for_test_kind(ground_kind));
            }
            entity
                .rocking
                .as_mut()
                .unwrap()
                .set_angle_sideways(crate::util::fixed_math::SimFixed::ZERO);
            assert!(
                ground_pose(&entity, UnitRenderSlopeState::Stable(0), 0).is_none(),
                "release restores ordinary atlas admission"
            );
        }
    }

    /// Drive Draw_Matrix `0x004AFFC0..0x004AFFE8` loads each binary32 body
    /// angle, takes FABS and compares it with the double 0.005 at `0x007E44E8`;
    /// the keyed arm needs both below. Binary32 0.005 lies just under that
    /// double, so it keys the turret images; I16F16 rounds it up to the gate.
    #[test]
    fn turret_image_key_compares_binary32_angles_with_the_native_gate() {
        let mut entity = GameEntity::test_default(1, "DEST", "Americans", 0, 0);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        let mut rocking = crate::sim::rocking::RockingState::default();
        rocking.set_angles_native_for_test([0.005, -0.005]);
        entity.rocking = Some(rocking);
        let stable = UnitRenderSlopeState::Stable(0);
        assert!(ground_pose(&entity, stable, 0).is_none());
        assert!(turret_images_cached(&entity, stable));
    }

    /// A missile draws its plain facing while CurrentPitch is zero and Rocket
    /// Draw_Matrix's pitched pose once Move_To sets it, in any band and ahead
    /// of the turret split.
    #[test]
    fn a_rocket_takes_its_pitched_pose_once_move_to_sets_its_pitch() {
        use crate::rules::missile_spawn::MissileSpawnRules;
        use crate::sim::components::DriveCoord;
        use crate::sim::movement::rocket_movement::move_to;
        let blocks = MissileSpawnRules::default();
        let destination = DriveCoord {
            x: 2560,
            y: 0,
            z: 0,
        };
        let launched = |block| {
            let mut entity = GameEntity::test_default(1, "V3ROCKET", "Russians", 0, 0);
            entity.set_category_for_test(EntityCategory::Aircraft);
            let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Rocket);
            let rocket = locomotor.rocket_runtime_mut().expect("a Rocket payload");
            assert_eq!(rocket.draw_pitch().to_bits(), 0);
            move_to(rocket, block, destination, 0);
            entity.locomotor = Some(locomotor);
            entity
        };
        // The constructor's V3 block starts at PitchInitial 0.
        let level = launched(&blocks.v3);
        assert_eq!(
            body_draw(&level, EntityDrawBand::Top, 0, false, false),
            BodyDraw::Composite
        );
        // Its CMisl block starts vertical: PitchInitial 1 quarter turn.
        let raised = launched(&blocks.cmisl);
        let pitch = raised
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.rocket_runtime())
            .map(|rocket| rocket.draw_pitch())
            .unwrap();
        assert!((pitch - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        for band in [EntityDrawBand::Top, EntityDrawBand::Ground] {
            for turret_parts in [false, true] {
                assert_eq!(
                    body_draw(&raised, band, 0, false, turret_parts),
                    BodyDraw::Pose(PoseTilt::Rocket(pitch))
                );
            }
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
    fn drive_ship_slope_snapshot_uses_production_display_frame_selector() {
        let mut sim = Simulation::with_seed(0);
        sim.session_mut_for_test().set_binary_frame_for_test(51);
        let mut entity = GameEntity::test_default(1, "SHIP", "Americans", 0, 0);
        entity.set_owner_for_test(sim.intern("Americans"));
        entity.set_type_ref_for_test(sim.intern("SHIP"));
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
        sim.entities_mut_for_test().insert(entity);

        let bytes = GameSnapshot::save(&sim, 1, 2, "slope display selector", 3);
        let restored = GameSnapshot::load(&bytes)
            .expect("current slope snapshot")
            .sim;
        let display_binary_frame =
            display_binary_frame_for_committed_session(restored.session().binary_frame());
        assert_eq!(display_binary_frame, 50);
        assert_eq!(
            locomotor_render_slope_state(restored.entities().get(1).unwrap(), display_binary_frame,),
            Some(UnitRenderSlopeState::Transition {
                from_slope: 4,
                to_slope: 9,
                phase_num: 1,
                phase_den: SLOPE_TRANSITION_FRAMES,
            }),
            "session frame 51 presents processed frame 50, one third through a timer started at 49"
        );
    }

    fn spawn_manager(states: &[SpawnSlotState]) -> SpawnManagerState {
        let mut manager = SpawnManagerState::for_test(
            InternedId::default(),
            states
                .iter()
                .enumerate()
                .map(|(index, &state)| {
                    SpawnSlot::for_test(
                        (state != SpawnSlotState::Regenerating).then_some(index as u64 + 1),
                        state,
                        CdTimer::default(),
                        true,
                    )
                })
                .collect(),
        );
        manager.set_rates_for_test(400, 0);
        manager.set_update_timer_for_test(CdTimer::started(10, 20));
        manager
    }

    #[test]
    fn gsi_13_07_no_spawn_alt_uses_zero_docked_not_zero_alive() {
        for (state, expects_alt) in [
            (SpawnSlotState::ReadyDocked, false),
            (SpawnSlotState::KamikazeWait, true),
            (SpawnSlotState::InFlight, true),
            (SpawnSlotState::Attacking, true),
            (SpawnSlotState::ComingHome, true),
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
}
