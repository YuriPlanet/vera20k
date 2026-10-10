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
use crate::render::unit_slope_transition_cache::{
    TransitionUnitSpriteEntry, TransitionUnitSpriteKey,
};
use crate::rules::house_colors::{self, HouseColorIndex};
use crate::sim::components::HarvestOverlay;
#[cfg(test)]
use crate::sim::movement::slope_transition::SLOPE_TRANSITION_FRAMES;
use crate::sim::voxel_frame_catalog::{
    NO_SPAWN_ALT_SUFFIX, draws_turret_parts, voxel_turret_index,
};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
#[path = "naval_draw_tests.rs"]
mod naval_draw_tests;

#[cfg(test)]
#[path = "unit_body_draw_tests.rs"]
mod unit_body_draw_tests;

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
    // `JumpjetLocomotionClass` Draw_Matrix (`0x0054DCC0`) is the facing matrix
    // of `LocomotionClass::Draw_Matrix @ 0x0055A730`, tilted only by its
    // `TiltCrashJumpjet=` arm: a Jumpjet body never takes a cell slope, landed
    // or airborne.
    let jumpjet = entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.kind == crate::rules::locomotor_type::LocomotorKind::Jumpjet
    });
    // A body that has left the floor has no cell slope to sit on, so it never
    // enters the drive-track tilt transition. Upper-layer bodies therefore
    // keep their stable atlas shape.
    if jumpjet || band != EntityDrawBand::Ground {
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
    let display_binary_frame = display_binary_frame_for_committed_session(sim.session.binary_frame);
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
    let local_owner_id = local_owner.as_deref().and_then(|o| sim.interner.get(o));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let art_reg: Option<&crate::rules::art_data::ArtRegistry> =
        state.rules().map(|rules| rules.art());
    let terrain = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|rt| rt.view().resolved_terrain());
    state
        .match_state
        .match_presentation
        .sinking_waterlines
        .borrow_mut()
        .retain(|id| sim.entities().contains(id));

    let encounter_order = super::helpers::tactical_entity_encounter_order(sim);
    for stable_id in encounter_order {
        let Some(entity) = sim.entities().get(stable_id) else {
            continue;
        };
        if entity.category != EntityCategory::Unit && !entity.is_voxel {
            continue;
        }
        let Some(band) = entity_draw_band(sim.display_layers(), stable_id) else {
            continue;
        };
        // Common visibility, passenger, limbo, and DrawState admission is shared below.
        let pos = &entity.position;
        let owner_str = sim.interner.resolve(entity.owner());
        let mut observer = super::helpers::observer_draw_context(
            sim,
            entity,
            local_owner.as_deref(),
            local_owner_id,
            state.rules(),
        );
        let Some((type_name, body)) = unit_body_draw(
            entity,
            &sim.interner,
            state.rules(),
            band,
            sim.session.binary_frame,
            observer,
        ) else {
            continue;
        };
        let no_shadow = state
            .rules()
            .and_then(|rules| rules.object(sim.interner.resolve(entity.type_ref())))
            .is_some_and(|object| object.no_shadow);
        let type_str = type_name.as_ref();
        observer.drawn_voxel = Some(true);
        let remap_owner = entity
            .disguise
            .as_ref()
            .filter(|_| DrawState::draws_disguise(entity, sim.session.binary_frame, observer))
            .and_then(|state| state.house())
            .map(|id| sim.interner.resolve(id))
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
            &sim.fog,
            ignore_visibility,
            sim.session.binary_frame,
            house_color_to_remap_row(hc),
            observer,
        ) else {
            continue;
        };
        // Render slope comes from the locomotor's cached previous/current
        // slope during gamemd's 3-frame transition, then falls back to the
        // stable terrain slope path.
        let slope_state = unit_render_slope_state(state, entity, display_binary_frame, band);
        let viewport = if entity.category == EntityCategory::Unit {
            unit_viewport
        } else {
            [sw, sh]
        };
        let Some([sx, sy]) = unit_draw_anchor(entity, [cam_x, cam_y], viewport) else {
            continue;
        };
        let interp_z = pos.z;
        let draw_state = draw_decision.state;
        let extra_light = state.rules().map_or(0, |rules| {
            crate::app::presentation::lighting::body_extra_light(entity, rules, sim, terrain)
        });
        let tint = vxl_body_tint(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx, pos.ry),
            entity.category,
            extra_light,
        );
        let palette_light = crate::app::presentation::lighting::body_palette_light(
            state.match_state.match_presentation.lighting.grid(),
            &sim.session.lighting,
            (pos.rx, pos.ry),
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
        //
        // RESIDUAL: the rest of DrawVoxelBody's block
        // (`0x0073BF7B..0x0073C15F`). The curtain ORs the `IronCurtainColor=`
        // `[ColorAdd]` colour, and a Berzerk unit the `BerserkColor=` one,
        // into the colour word the composite's blit ORs into each pixel
        // (retail IronCurtainColor=0 selects None, whose OR is zero); VERA
        // hands that blit no colour word. A Deactivated unit (`+0x1C8`,
        // `0x0070FBD0`) draws at half the intensity.
        let (body_tint, body_light) =
            crate::app::presentation::lighting::curtain_light(entity, tint, palette_light, sim);
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

        // Sampled at the turret's frame, so the two sprites cannot disagree.
        let body_facing = entity.body_facing_byte(sim.session.binary_frame);
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
        } else if let BodyDraw::Turret {
            turret: turret_facing,
            barrel_elevation,
        } = body
        {
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
            if let Some((entry, texture_source)) =
                unit_entry_for_slope_state(state, atlas, &key, slope_state)
            {
                let depth_y: f32 = sy + entry.offset_y + entry.pixel_size[1] + dock_depth_y_offset;
                let depth: f32 = body_sort_depth(state, entity, depth_y, interp_z);
                let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true) as f32;
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

        // Emit harvest overlay (oregath.shp) if the miner is actively harvesting.
        // OREGATH is an SHP sprite from sprite_atlas, but remains an owned piece
        // of its harvester's Ground slot so atlas identity cannot re-sort it.
        // `UnitClass::Draw @ 0x0073CEC0` draws it from Unit+0x6D2 only
        // while the locomotor is not moving now (`[loco+0x80]` at
        // `0x0073D114`: Drive `0x004AFC20`, rotating or moving with speed),
        // so a miner hopping to its next ore cell shows none. RESIDUAL:
        // native frames it from `(Unit+0x538 + frame) % 15`; this overlay
        // keeps its own counter.
        let moving = crate::sim::movement::motion_query::is_moving_now(
            entity,
            state.rules().map(|rules| {
                crate::sim::movement::SpeedRules::new(
                    rules,
                    &sim.interner,
                    &sim.type_handles,
                    &sim.houses,
                )
            }),
            display_binary_frame,
        );
        if let Some(ref ho) = entity.harvest_overlay
            && ho.visible
            && !moving
            && let Some((page, instance)) = emit_harvest_overlay(
                state,
                entity,
                body_facing,
                ho,
                center_x,
                center_y,
                pos.z,
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
                objects.push(PlannedObjectInstance::object(parent, pieces));
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

/// Unit DrawVoxelBody73B4DA..73B50E divides the persistent Foot+538 counter
/// by the selected model's main HVA frame count. The object's original model
/// may have a different count (SCHP has two frames, deployed SCHD has one).
/// When that signed remainder is zero, the turret takes Techno+148 modulo
/// its own HVA count. Negative remainders retain their bits in the atlas key;
/// invalid frame indices have no sprite. Aircraft retain their body path.
/// Native vectors: tools/spatial_oracle/unit_simple_deploy.json draw_frames.
fn unit_animation_frames(
    entity: &crate::sim::game_entity::GameEntity,
    model: &str,
    turret_index: i32,
    frame_counts: &std::collections::BTreeMap<(String, VxlLayer, i32), u32>,
) -> (u32, u32) {
    let body = if entity.category != EntityCategory::Unit {
        entity
            .voxel_animation
            .map_or(0, |animation| animation.frame)
    } else {
        let frames = frame_counts
            .get(&(model.to_string(), VxlLayer::Body, 0))
            .or_else(|| frame_counts.get(&(model.to_string(), VxlLayer::Composite, 0)))
            .copied()
            .unwrap_or(1)
            .max(1);
        ((entity.body_frame_counter as i32) % frames as i32) as u32
    };
    let turret = if body != 0 || entity.turret_anim_frame == 0 {
        body
    } else {
        frame_counts
            .get(&(model.to_string(), VxlLayer::Turret, turret_index))
            .filter(|&&frames| frames > 1)
            .map_or(0, |&frames| {
                (entity.turret_anim_frame % frames as i32) as u32
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
    if entity.category == EntityCategory::Unit
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
        .then_some(entity.display_type_override)
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
    if entity.category == EntityCategory::Unit && entity.unit_deploying() {
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
    use crate::render::unit_atlas::PoseTilt;
    use crate::rules::locomotor_type::LocomotorKind;
    let locomotor = entity.locomotor.as_ref()?;
    if let Some(rocket) = locomotor.rocket_runtime() {
        let pitch = rocket.draw_pitch();
        return (pitch != 0.0).then_some(PoseTilt::Rocket(pitch));
    }
    let rocking = entity.rocking.as_ref()?;
    let angles = [
        rocking.angle_sideways.to_num::<f32>(),
        rocking.angle_forwards.to_num::<f32>(),
    ];
    match locomotor.kind {
        LocomotorKind::Fly if entity.crashing && band != EntityDrawBand::Ground => {
            Some(PoseTilt::Fly(angles))
        }
        LocomotorKind::Jumpjet
            if tilt_crash_jumpjet && crate::render::vxl_raster::jumpjet_tilt_applies(angles) =>
        {
            Some(PoseTilt::Jumpjet(angles))
        }
        _ => None,
    }
}

/// One posed body drawn on this frame's pose page. It joins its display
/// layer in emission order like an atlas body; aircraft cast no VERA shadow
/// (recorded on the draw passes).
#[allow(clippy::too_many_arguments)]
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
    let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true) as f32;
    let bounds = composite_draw_bounds([(entry, [center_x, center_y])]);
    let (composite_rect, split) = bounds.depth_rect(false);
    pieces.push(ObjectPieceInstance {
        target: ObjectTexture::UnitPose,
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
/// (`0x004AFFB0..0x004AFFE8`, `[0x007E44E8]`); otherwise it keys -1
/// (`0x004B0187`), and the turret and barrel draw uncached (`0x0073B74C`,
/// `0x007067EF`). VERA's rocking angles approximate the native floats.
fn turret_images_cached(
    entity: &crate::sim::game_entity::GameEntity,
    slope_state: UnitRenderSlopeState,
) -> bool {
    const TILT_GATE: crate::util::fixed_math::SimFixed =
        crate::util::fixed_math::SimFixed::lit("0.005");
    matches!(slope_state, UnitRenderSlopeState::Stable(_))
        && entity.rocking.as_ref().is_none_or(|rocking| {
            rocking.angle_sideways.abs() < TILT_GATE && rocking.angle_forwards.abs() < TILT_GATE
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
            turret_index: key.turret_index,
            facing: key.facing,
            layer: key.layer,
            frame: key.frame,
            from_slope,
            to_slope,
            phase_num,
            phase_den,
            barrel_pitch: key.barrel_pitch,
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

/// First-fill mask owner for the supported ordinary flat ground path. The
/// input entries are precisely the parts selected for this parent's draw;
/// cache hits skip their pixel work and retain the first completed mask.
#[allow(clippy::too_many_arguments)]
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
    entity.category == EntityCategory::Unit
        && !entity.sinking.is_active()
        && band == EntityDrawBand::Ground
        && !no_shadow
        && entity.cloak.as_ref().is_none_or(|cloak| cloak.state == 0)
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
    if !unit_shadow_admitted(entity, no_shadow, band) {
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
    push_unit_sprite(UnitTextureSource::Stable(entry.page), sprite, pieces);
}

fn push_unit_sprite(
    texture_source: UnitTextureSource,
    sprite: SpriteInstance,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let target = match texture_source {
        UnitTextureSource::Transition(page) => ObjectTexture::UnitTransitionPage(page),
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
) -> Option<[f32; 2]> {
    let (x, y) = crate::render::locomotor_visual::screen_position(entity);
    // Unit draw admission precedes stateful +3CA capture. A smaller visual
    // cull delays that first capture and changes later visible clipping.
    let admitted = if entity.category == EntityCategory::Unit {
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
        )
    };
    admitted.then_some([x, y])
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
        if wants_split {
            if let Some([_, y, _, height]) = self.native {
                return ([y as f32, height as f32], true);
            }
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
    if entity.category == EntityCategory::Unit {
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
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let slope_type = stable_slope_for_key(slope_state);
    let voxel_adjust = super::foot_depth::unit_z_adjust(state, entity, true) as f32;
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
    let (tur_ox, tur_oy) = turret_screen_offset(art_offset, body_facing, slope_state);

    // The barrel pitches by its elevation's step (`d32 - 8`, `0x0073BB85..
    // 0x0073BB96`). A pose the turret image is cached under draws the pitch its
    // type cached there; an uncached pose draws the unit's own.
    let drawn_pitch = crate::render::vxl_raster::voxel_facing_step_u16(barrel_elevation) as i8 - 8;
    barrel_key.barrel_pitch = if turret_images_cached(entity, slope_state) {
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
    let (barrel_ox, barrel_oy) = {
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
    let body_entry_opt = unit_entry_for_slope_state(state, atlas, &body_key, slope_state);
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
        unit_entry_for_slope_state(state, atlas, key, slope_state)
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
    use crate::render::unit_atlas::PoseTilt;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::InternedId;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::spawn_manager::{
        SpawnManagerMode, SpawnManagerState, SpawnSlot, SpawnSlotState,
    };
    use crate::sim::timer::CdTimer;
    use crate::sim::world::Simulation;

    /// Each `curtain_draw_arm` row (DrawVoxelBody `0x0073BF7B..0x0073C166`
    /// run natively) whose other arms are idle: no flash count, not Berzerk
    /// or Deactivated, no caller colour word and retail's IronCurtainColor=0
    /// in the active RGB565 format. The intensity the composite takes, at the
    /// row's frame as the committed `binary_frame`, is the brightness
    /// [`crate::app::presentation::lighting::curtain_light`] gives the body,
    /// and the colour word VERA does
    /// not draw has no bit in its 16-bit pixel lane (above it the conversion
    /// leaves stale ECX bits, `0x0073BFFD`). VERA's compatibility tint takes
    /// the intensity's ratio. The other rows are the block's residuals (on
    /// the call site).
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
        let mut compared = 0;
        for row in rows {
            if int(&row["flash"]) != 0
                || row["berzerk"].as_bool().unwrap()
                || row["deactivated"].as_bool().unwrap()
                || int(&row["word"]) != 0
                || int(&row["iron_color"]) != 0
                || int(&row["pixel_format"]) != 2
            {
                continue;
            }
            let mut unit = GameEntity::test_default(1, "HTNK", "Russians", 10, 10);
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
                crate::render::palette_light::PaletteLight::color_scheme([1000; 3], intensity);
            sim.session.binary_frame = int(&row["frame"]) as u32;
            let (lit_tint, lit) =
                crate::app::presentation::lighting::curtain_light(&unit, tint, light, &sim);
            let out = int(&row["out_intensity"]);
            assert_eq!(lit.brightness(), out, "{row}");
            assert_eq!(lit.rows(), light.rows(), "{row}");
            let ratio = out as f32 / intensity as f32;
            assert_eq!(lit_tint, tint.map(|channel| channel * ratio), "{row}");
            assert_eq!(row["out_word"].as_u64().unwrap() & 0xFFFF, 0, "{row}");
            compared += 1;
        }
        assert_eq!(compared, 38);
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
        assert_eq!(colors.slots[0].name.as_deref(), Some("None"));
        assert_eq!(colors.slots[0].rgb, [0, 0, 0]);
    }

    #[test]
    fn vehicle_shadow_active_locomotor_selects_native_or_legacy_companion() {
        let mut entity = GameEntity::test_default(1, "GENERATED", "Americans", 0, 0);
        entity.category = EntityCategory::Unit;
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
        cloak.state = 1;
        entity.cloak = Some(cloak);
        assert!(!native_shadow_caller_eligible(
            &entity,
            false,
            UnitRenderSlopeState::Stable(0),
            EntityDrawBand::Ground
        ));
    }

    #[test]
    fn raw_cloak_shadow_gate_covers_zero_progress_and_type_flag() {
        let mut entity = GameEntity::test_default(1, "SUB", "Russians", 0, 0);
        entity.category = EntityCategory::Unit;
        assert!(unit_shadow_admitted(&entity, false, EntityDrawBand::Ground));
        assert!(!unit_shadow_admitted(&entity, true, EntityDrawBand::Ground));
        for state in [1, 2, 3] {
            let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
            cloak.state = state;
            entity.cloak = Some(cloak);
            // StartCloaking703799 writes state1/progress0. At zero progress
            // the body's visual query may be ordinary; the shadow stays absent.
            assert!(!unit_shadow_admitted(
                &entity,
                false,
                EntityDrawBand::Ground
            ));
        }
        entity.cloak.as_mut().unwrap().state = 0;
        assert!(unit_shadow_admitted(&entity, false, EntityDrawBand::Ground));
    }

    /// A crashing body takes its pose before the turret split, whether or not
    /// its model has turret parts. A Fly's Secondary facing in `barrel_facing`
    /// is not a turret: its model has none.
    #[test]
    fn a_crashing_body_takes_its_pose_before_the_turret_split() {
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 0, 0);
        entity.category = EntityCategory::Aircraft;
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));
        crate::sim::movement::air_movement::ensure_fly_secondary_facing(&mut entity);
        assert_eq!(
            body_draw(&entity, EntityDrawBand::Top, 0, false, false),
            BodyDraw::Composite
        );
        entity.crashing = true;
        entity.rocking = Some(crate::sim::components::RockingState {
            angle_sideways: crate::util::fixed_math::SimFixed::from_num(0.5),
            angle_forwards: crate::util::fixed_math::SimFixed::from_num(-0.25),
            ..Default::default()
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
        entity.rocking = Some(crate::sim::components::RockingState {
            angle_sideways: crate::util::fixed_math::SimFixed::from_num(0.004),
            angle_forwards: crate::util::fixed_math::SimFixed::from_num(-0.004),
            ..Default::default()
        });
        assert!(matches!(
            body_draw(&entity, EntityDrawBand::Top, 0, true, true),
            BodyDraw::Turret { .. }
        ));
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
            entity.category = EntityCategory::Aircraft;
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
                    timer: CdTimer::default(),
                    is_missile_spawn: true,
                })
                .collect(),
            update_timer: CdTimer::started(10, 20),
            spawn_timer: CdTimer::default(),
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
        assert!(!manager.update_timer.expired(10));
        assert_eq!(no_spawn_alt_type_id("V3", true, Some(&manager)), None);

        manager.slots[0].state = SpawnSlotState::InFlight;
        assert_eq!(
            no_spawn_alt_type_id("V3", true, Some(&manager)).as_deref(),
            Some("V3WO"),
            "the next presentation query sees the slot transition without an AI pass"
        );
    }

    #[test]
    fn gsi_13_10_vxl_selector_adds_the_class_extra_to_the_top_scalar() {
        let mut grid = lighting::CellLightGrid::new();
        grid.insert_profiled_light((4, 5), [1.0, 0.88, 0.88], 1.0);

        let unit = vxl_body_tint(&grid, (4, 5), EntityCategory::Unit, 200);
        let aircraft = vxl_body_tint(&grid, (4, 5), EntityCategory::Aircraft, 424);
        let structure = vxl_body_tint(&grid, (4, 5), EntityCategory::Structure, 200);

        for (actual, expected) in unit.into_iter().zip([1.2, 1.056, 1.056]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in aircraft.into_iter().zip([1.424, 1.25312, 1.25312]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in structure.into_iter().zip([1.0, 0.88, 0.88]) {
            assert!((actual - expected).abs() < 0.0001);
        }
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
            composite_draw_bounds([(body, [0.0, 100.0]), (turret, [0.0, 100.0])]).depth_rect(true);
        assert!(split);
        assert_eq!(rect, [80.0, 30.0]);
        let (rect, split) = composite_draw_bounds([(body, [0.0, 100.0])]).depth_rect(false);
        assert!(!split);
        assert_eq!(rect, [50.0, 100.0]);
        let unknown = UnitSpriteEntry {
            native_draw_bounds: None,
            ..body
        };
        assert!(
            !composite_draw_bounds([(body, [0.0, 0.0]), (unknown, [0.0, 0.0])])
                .depth_rect(true)
                .1
        );
    }
}
