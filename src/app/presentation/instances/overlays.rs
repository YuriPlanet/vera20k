//! Overlay, AnimClass, and fog snapshot instance builders.
//!
//! Generates SpriteInstances for map overlays (ore/gems, bridges, terrain objects),
//! the simulation's AnimClass objects, and fog-of-war building snapshots.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::collections::HashMap;

use crate::app::AppState;
use crate::map::lighting::DEFAULT_TINT;
use crate::map::overlay_types::is_bridge_overlay_index;
use crate::map::terrain::{self, TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::SpriteInstance;
use crate::render::bridge_atlas::is_high_bridge_body_identity;
use crate::render::native_z::{self, ZGradient, pack_z_gradient};
use crate::render::overlay_atlas::{CRATE_BODY_FRAME, OverlaySpriteKey};
use crate::render::sprite_atlas::ShpSpriteKey;
use crate::render::tactical_draw_plan::{
    BlitPolicy, ObjectDraw, RenderZPolicy, SpriteEncoding, TacticalCoord,
};
use crate::rules::art_data::{AnimLayer, AnimTypeRuntimeConfig, anim_translucency_source_alpha};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::overlay_types::OverlayTypeFlags;
use crate::sim::projectile::ProjectileCoord;
use crate::util::fixed_math::SimFixed;

use super::helpers::{
    ANIM_DRAW_DEPTH_BIAS_PX, apply_shape_z_adjust, compute_sprite_depth_params, in_view,
};

/// Ordinary global/cell AnimClass palette ownership; explicit per-instance
/// converters remain with their producer until its brightness lifetime is known.
fn anim_palette_light(
    state: &AppState,
    cell: (u16, u16),
    cfg: Option<&AnimTypeRuntimeConfig>,
    cell_drawer: bool,
) -> crate::render::palette_light::PaletteLight {
    crate::app::presentation::lighting::anim_palette_light(
        state.match_state.match_presentation.lighting.grid(),
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|r| &r.simulation.session.lighting),
        cell,
        cfg,
        cell_drawer,
    )
}

/// Map terrain entries remain the render metadata source, but once rules-backed
/// simulation authority exists the live cell index decides whether an instance
/// still exists. This keeps loading/fallback screens static without letting a
/// destroyed runtime object remain visible forever.
#[cfg(test)]
fn terrain_object_is_render_visible(
    object: &crate::map::overlay::TerrainObject,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    authority: Option<&crate::sim::production::ProductionState>,
) -> bool {
    let (Some(rules), Some(production)) = (rules, authority) else {
        return true;
    };
    if rules
        .terrain_object_type_case_insensitive(&object.name)
        .is_none()
    {
        return true;
    }
    let Some(stable_id) = production.terrain_object_cells.get(&(object.rx, object.ry)) else {
        return false;
    };
    production
        .terrain_objects
        .get(stable_id)
        .is_some_and(|terrain| terrain.is_live() && terrain.cell() == (object.rx, object.ry))
}

/// Body frame the native overlay draw selects for a non-bridge overlay cell.
///
/// `Crate=yes` overlays take a dedicated branch that hardcodes frame 0; every
/// other overlay draws its cell overlay-data byte directly (ore density, wall
/// `damage << 4 | connectivity`).
fn overlay_body_frame(is_crate: bool, overlay_data: u8) -> u8 {
    if is_crate {
        CRATE_BODY_FRAME
    } else {
        overlay_data
    }
}

/// The ordinary overlay pass must not own any identity whose live numeric ID
/// dispatches through native high-bridge Mark, even when layered rules rename
/// its art away from the stock BRIDGE1/2 family.
fn ordinary_overlay_accepts_identity(overlay_id: u8, name: &str) -> bool {
    !is_high_bridge_body_identity(overlay_id, name)
}

/// Ordinary overlay bodies are SHP draws, not TMP tiles. The active path is
/// `Cell_ContentRendering @ 0x006D6D10 -> DrawOverlay_Body @ 0x0047F6A0`.
/// Every body requests 0x4E00; selector 0x00490B90's +0xBC family tests and
/// writes Z (same opaque leaf as buildings). Wall (+0x2A8) forces gradient 2
/// with `-15 * level - 2`; ordinary non-walls use DrawFlat (+0x2B3), adding
/// -15 only for upright non-rock art. Tiberium's flat branch uses gradient 0.
///
/// Sloped resources use a separate native slope Z-shape. Rubble queries
/// foundation art dynamically, and the TS vein family also has special shape
/// offsets. Preserve their existing passthrough until those consumers have
/// their own shape data; assigning a made-up flat Z would poison nearby walls.
fn ordinary_overlay_z(
    flags: Option<&OverlayTypeFlags>,
    slope: u8,
    level: u8,
) -> (RenderZPolicy, f32, u32) {
    let Some(flags) = flags else {
        return (RenderZPolicy::None, 0.0, 0);
    };
    if (flags.tiberium && slope != 0)
        || flags.is_rubble
        || flags.is_veins
        || flags.is_veinhole_monster
    {
        return (RenderZPolicy::None, 0.0, 0);
    }
    let (gradient, class_term) = if flags.tiberium {
        (ZGradient::Flat, -2)
    } else if flags.wall {
        (ZGradient::Vertical, -2)
    } else {
        (
            if flags.draw_flat {
                ZGradient::Flat
            } else {
                ZGradient::Vertical
            },
            if flags.draw_flat || flags.is_a_rock {
                -2
            } else {
                -17
            },
        )
    };
    (
        RenderZPolicy::ReadWrite,
        native_z::ground_anchored_z_adjust(i32::from(level), class_term) as f32,
        pack_z_gradient(gradient, false),
    )
}

/// Resolve the CellClass overlay identity/data used by the tactical overlay
/// draw. Low-bridge damage and repair mutate the identity in place, so a map
/// pack entry is only the iteration anchor once live cell authority exists.
fn overlay_render_identity(
    static_overlay_id: u8,
    static_overlay_data: u8,
    live_cell: Option<&crate::sim::overlay_grid::OverlayCell>,
) -> Option<(u8, u8)> {
    let Some(live_cell) = live_cell else {
        return Some((static_overlay_id, static_overlay_data));
    };
    if is_bridge_overlay_index(static_overlay_id) {
        return live_cell
            .overlay_id
            .map(|overlay_id| (overlay_id, live_cell.overlay_data));
    }
    (live_cell.overlay_id == Some(static_overlay_id))
        .then_some((static_overlay_id, live_cell.overlay_data))
}

/// Choose the display-only identity for a flat resource cell.
///
/// Active YR `CellClass__DrawOverlay_Body @ 0x0047F6A0` keeps the Cell's
/// overlay identity/data as resource state but selects a coordinate-derived
/// flat image when the resolved cell is not sloped. Missing render metadata,
/// invalid signed indices, slopes, and non-resource overlays retain the live
/// identity without approximation.
fn overlay_display_identity(
    live_overlay_id: u8,
    overlay_data: u8,
    rx: u16,
    ry: u16,
    slope_type: Option<u8>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    tiberium_types: Option<&crate::rules::tiberium_type::TiberiumTypeRegistry>,
) -> (u8, u8) {
    let (Some(overlay_registry), Some(tiberium_types)) = (overlay_registry, tiberium_types) else {
        return (live_overlay_id, overlay_data);
    };
    if slope_type != Some(0)
        || !overlay_registry
            .flags(live_overlay_id)
            .is_some_and(|flags| flags.tiberium)
    {
        return (live_overlay_id, overlay_data);
    }

    let display_overlay_id = overlay_registry
        .flat_tiberium_display_overlay_id(tiberium_types, live_overlay_id, rx, ry)
        .unwrap_or(live_overlay_id);
    (display_overlay_id, overlay_data)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimRenderDestination {
    Ground(ObjectDraw),
    Top,
    Existing,
}

fn anim_render_destination(
    stable_id: u64,
    owner_entity: Option<u64>,
    world_coord: crate::sim::anim_class::AnimWorldCoord,
    config: Option<&AnimTypeRuntimeConfig>,
    ground_order: &crate::app::presentation::render::draw_plan_lowering::NativeGroundOrder,
) -> Option<AnimRenderDestination> {
    // gamemd-derived: `AnimClass::GetLayer @ 0x00424CB0` forces layer 2
    // (Ground) for ANY anim carrying an owner at `Anim+0xCC`, ahead of both the
    // AnimType `Layer=` read and the Top default — so an attached anim joins the
    // sorted ground layer whatever its type asked for. Layer 2 is the only
    // sorted `DisplayClass` layer: `Submit_Object @ 0x004A9720` sets the sorted
    // flag with `CMP EDI,0x2` at `0x004A9747` / `SETZ CL` at `0x004A974D`, and every other layer
    // plain-appends, so a layer-2 member is inserted in ascending y-sort against
    // the non-anim ground objects by `ObjectClass::YSortComparator @ 0x005F6220`
    // keyed on vtable `+0xB8`. For an anim that key is
    // `AnimClass::GetYSort @ 0x00422BC0` = `ObjectClass::GetYSort @ 0x005F6BD0`
    // + AnimType `YSortAdjust`, and `ObjectClass::GetYSort` reads slot `+0xAC`
    // (`ObjectClass::GetRenderCoords @ 0x0041BE00`, which AnimClass does not
    // override) — that re-enters the anim's own virtual `+0x48`, so an attached
    // anim sorts at its OWNER-RESOLVED absolute position, not at the stored
    // relative delta. `coord` is already that resolved value.
    let config_y_sort_adjust = config.map_or(0, |config| config.y_sort_adjust);
    if owner_entity.is_some() {
        return ground_order
            .anim_object_draw(
                stable_id,
                TacticalCoord {
                    x: world_coord.x,
                    y: world_coord.y,
                    z: world_coord.z,
                },
                config_y_sort_adjust,
            )
            .map(AnimRenderDestination::Ground);
    }
    let config = config?;
    match config.layer {
        AnimLayer::Ground => ground_order
            .anim_object_draw(
                stable_id,
                TacticalCoord {
                    x: world_coord.x,
                    y: world_coord.y,
                    z: world_coord.z,
                },
                config.y_sort_adjust,
            )
            .map(AnimRenderDestination::Ground),
        AnimLayer::Top => Some(AnimRenderDestination::Top),
        AnimLayer::Other(_) => Some(AnimRenderDestination::Existing),
    }
}

/// Build ordinary scheduler-owned `AnimClass` sprites.
pub(crate) fn build_anim_class_instances(
    state: &AppState,
    paged: &mut [Vec<SpriteInstance>],
    top_instances: &mut Vec<SpriteInstance>,
    top_pages: &mut Vec<usize>,
    top_ids: &mut Vec<u64>,
    ground_objects: &mut Vec<
        crate::app::presentation::render::draw_plan_lowering::PlannedGroundObjectInstance,
    >,
    ground_order: &crate::app::presentation::render::draw_plan_lowering::NativeGroundOrder,
) {
    let (sim, atlas) = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.sprite_atlas,
    ) {
        (Some(s), Some(a)) => (s, a),
        _ => return,
    };
    let z2 = state.match_state.input.zoom_level;
    let (cam_x, cam_y, sw, sh) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.render_width() as f32 / z2,
        state.render_height() as f32 / z2,
    );
    for &stable_id in sim.tactical_registration_order() {
        let Some(anim) = sim.anim(stable_id) else {
            continue;
        };
        if anim.runtime.inactive || anim.building_slot.is_some() {
            continue;
        }
        let type_name: &str = sim.interner.resolve(anim.type_id);
        let config = state
            .rules()
            .and_then(|rules| rules.art_registry.anim_runtime_config(type_name));
        if config.is_some_and(|config| !config.art_body_read) {
            continue;
        }
        if !crate::sim::anim_class::anim_draw_detail_visible(
            crate::sim::anim_class::AnimDrawDetailInput {
                // No authoritative draw-rate degradation producer exists yet.
                frame_rate_below_minimum: false,
                type_detail_level: config.map_or(0, |value| value.detail_level),
                game_detail_level: state
                    .match_state
                    .match_presentation
                    .in_game_options
                    .detail_level as i32,
                hidden: anim.draw_runtime.hidden,
                special_hidden: anim.draw_runtime.special_hidden,
                // The native special-hide type bit remains an explicit residual.
                type_special_hide: false,
            },
        ) {
            continue;
        }
        let Ok(frame) = u16::try_from(anim.runtime.current_frame) else {
            continue;
        };
        // `AnimClass::GetCoords @ 0x00422BE0`: an owner-attached anim stores an
        // owner-relative delta, so the draw position must be resolved through
        // the owner rather than read off the field.
        let Some(anim_coord) = sim.anim_absolute_coord(anim.stable_id) else {
            continue;
        };
        let (center_x, center_y, rx, ry, _, lift_px) = anim_world_render_coords(anim_coord);
        if !in_view(
            center_x, center_y, 200.0, 200.0, cam_x, cam_y, sw, sh, 200.0,
        ) {
            continue;
        }
        let tint = state
            .match_state
            .match_presentation
            .lighting
            .grid()
            .anim_tint_at((rx, ry), config);
        let palette_light = if anim.remap_color.is_some() {
            Default::default()
        } else {
            anim_palette_light(state, (rx, ry), config, anim.use_cell_drawer)
        };
        let key = ShpSpriteKey {
            palette_context: if anim.use_cell_drawer {
                crate::render::sprite_atlas::ShpPaletteContext::Cell
            } else if anim.remap_color.is_some() {
                crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme
            } else {
                crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim
            },
            type_id: type_name.to_string(),
            facing: 0,
            frame,
            house_color: anim.remap_color.unwrap_or(HouseColorIndex(0)),
        };
        let Some(entry) = atlas.get(&key) else {
            continue;
        };
        let Some(alpha) = anim_instance_alpha_with_flags(
            config,
            anim.draw_flags,
            anim.runtime.current_frame,
            i32::from(
                presentation_anim_frame_count(&atlas.active_anim_frame_counts, type_name)
                    .unwrap_or(0),
            ),
            state
                .match_state
                .match_presentation
                .in_game_options
                .detail_level as i32,
            anim.draw_runtime,
        ) else {
            continue;
        };
        let (origin_y, world_height) = state
            .match_state
            .match_presentation
            .terrain_grid
            .as_ref()
            .map(|grid| (grid.origin_y, grid.world_height))
            .unwrap_or((0.0, 1.0));
        let fire_depth = super::helpers::compute_sprite_depth_params_lifted(
            origin_y,
            world_height,
            center_y,
            lift_px,
        );
        debug_assert!(!anim.terrain_attached || anim.use_cell_drawer);
        // Native Z (`AnimClass__DrawIt @ 0x00422CA0`): 0x2800, gradient entry
        // 2 (0 when Flat), `YDrawOffset + ZAdjust - AdjustForZ - 2`; tests Z
        // per pixel and never writes. YDrawOffset is already in the atlas
        // offset, so only ZAdjust - 2 remains beside the lift cancel.
        let instance = SpriteInstance {
            position: [center_x + entry.offset_x, center_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth: apply_shape_z_adjust(
                fire_depth,
                anim.z_adjust + ANIM_DRAW_DEPTH_BIAS_PX,
                world_height,
            ),
            tint,
            palette_light,
            alpha,
            // YDrawOffset is baked into the atlas offset, so it must also
            // ride the Z term to keep Z on the un-offset row, as natively.
            z_adjust: super::helpers::lifted_z_adjust(
                lift_px,
                anim.z_adjust + config.map_or(0, |c| c.y_draw_offset) + ANIM_DRAW_DEPTH_BIAS_PX,
            ),
            z_gradient: crate::render::native_z::pack_z_gradient(
                if config.is_some_and(|c| c.flat) {
                    crate::render::native_z::ZGradient::Flat
                } else {
                    crate::render::native_z::ZGradient::Vertical
                },
                false,
            ),
            ..Default::default()
        };
        match anim_render_destination(
            anim.stable_id,
            anim.owner_entity,
            anim_coord,
            config,
            ground_order,
        ) {
            Some(AnimRenderDestination::Ground(parent)) => ground_objects.push(
                crate::app::presentation::render::draw_plan_lowering::PlannedGroundObjectInstance::object(
                    parent,
                    vec![crate::app::presentation::render::draw_plan_lowering::GroundPieceInstance {
                        target: crate::app::presentation::render::draw_plan_lowering::GroundTexture::ShpPage(
                            entry.page as usize,
                        ),
                        render_z: parent.policy.render_z,
                        instance,
                    }],
                ),
            ),
            Some(AnimRenderDestination::Top) => {
                top_instances.push(instance);
                top_pages.push(entry.page as usize);
                top_ids.push(anim.stable_id);
            }
            Some(AnimRenderDestination::Existing) => {
                paged[entry.page as usize].push(instance);
            }
            None => {}
        }
    }
}

/// Source-pixel weight one animation frame draws with.
///
/// gamemd picks a blitter family per draw from the art type's `Translucency=` /
/// `Translucent=` keys; `anim_frame_source_alpha` reproduces that selection and
/// returns the weight the family gives the incoming sprite pixel. An art type
/// the registry does not know draws opaque, matching the resolver's own
/// no-keys result.
fn anim_instance_alpha(
    config: Option<&AnimTypeRuntimeConfig>,
    current_frame: i32,
    shp_frame_count: i32,
) -> f32 {
    anim_instance_alpha_with_flags(
        config,
        0,
        current_frame,
        shp_frame_count,
        2,
        crate::sim::anim_class::AnimDrawRuntime::default(),
    )
    .unwrap_or(0.0)
}

fn anim_instance_alpha_with_flags(
    config: Option<&AnimTypeRuntimeConfig>,
    base_flags: u32,
    current_frame: i32,
    shp_frame_count: i32,
    game_detail_level: i32,
    draw_runtime: crate::sim::anim_class::AnimDrawRuntime,
) -> Option<f32> {
    let result = crate::sim::anim_class::anim_translucency_selection(
        crate::sim::anim_class::AnimTranslucencyInput {
            base_flags,
            forced_translucent: draw_runtime.forced_translucent,
            forced_uses_75: draw_runtime.forced_uses_75,
            translucency_detail_level: config.map_or(0, |value| value.translucency_detail_level),
            // Draw-time detail is presentation state and does not enter simulation.
            game_detail_level,
            translucent_ramp: config.is_some_and(|value| value.translucent),
            current_frame,
            frame_count: config
                .and_then(|value| value.raw_shp_frame_count)
                .unwrap_or(shp_frame_count),
            explicit_translucency: config.map_or(0, |value| value.translucency),
            instance_ramp: i32::from(draw_runtime.translucency_ramp),
        },
    );
    result
        .draw
        .then(|| anim_translucency_source_alpha(result.flags))
}

/// SHP header frame count for an animation type, as the translucency resolver
/// wants it.
///
/// `anim_frame_source_alpha` only consults this on the `Translucent=yes`
/// progressive path, and only for a type that never went through asset binding
/// (no `raw_shp_frame_count`, no explicit `End=`). This is presentation-only:
/// simulation timing reads the immutable rules-side asset catalog. Zero when
/// the atlas has no matching shape.
fn anim_shp_frame_count(state: &AppState, type_name: &str) -> i32 {
    state
        .match_state
        .match_presentation
        .sprite_atlas
        .as_ref()
        .and_then(|atlas| presentation_anim_frame_count(&atlas.active_anim_frame_counts, type_name))
        .map(i32::from)
        .unwrap_or(0)
}

fn presentation_anim_frame_count(
    frame_counts: &HashMap<String, u16>,
    type_name: &str,
) -> Option<u16> {
    frame_counts.get(type_name).copied().or_else(|| {
        let canonical = type_name.to_ascii_uppercase();
        frame_counts.get(&canonical).copied()
    })
}

/// Screen position, cell, height level and the exact pixel lift of an anim.
///
/// The sprite sits at the anim's exact Z (a muzzle or an airburst is not on a
/// level boundary). The lift is the same `AdjustForZ(z)` the projection
/// subtracted, so depth can cancel exactly what was drawn, as
/// `AnimClass::DrawIt @ 0x00422CA0` does.
fn anim_world_render_coords(
    world: crate::sim::anim_class::AnimWorldCoord,
) -> (f32, f32, u16, u16, u8, i32) {
    let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
    let (screen_x, screen_y) =
        crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, world.z);
    let lift_px = crate::util::flh_transform::adjust_for_z_leptons(world.z);
    (screen_x, screen_y, rx, ry, z, lift_px)
}

/// Build SpriteInstances for visible overlay objects and terrain objects.
///
/// Bridge body, body shadow, and railing instances are emitted separately by
/// `instances::bridges` (Phase D). Low bridges (LOBRDG*) ride in the
/// generic `instances` bucket and use the regular overlay atlas.
pub(crate) fn build_overlay_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
    instances: &mut Vec<SpriteInstance>,
    render_z: &mut Vec<RenderZPolicy>,
    ground_objects: &mut Vec<
        crate::app::presentation::render::draw_plan_lowering::PlannedGroundObjectInstance,
    >,
    ground_order: &crate::app::presentation::render::draw_plan_lowering::NativeGroundOrder,
) {
    let atlas = match &state.match_state.match_presentation.overlay_atlas {
        Some(a) => a,
        None => return,
    };
    let (cam_x, cam_y) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    );
    let (origin_y, world_height) = state
        .match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|g| (g.origin_y, g.world_height))
        .unwrap_or((0.0, 1.0));

    // Cell visibility for the local owner — used to cull overlays and terrain
    // objects in unrevealed cells. The shroud multiply pass darkens per-pixel,
    // but tall sprites (bridges, trees) extend their canopy into screen-space
    // owned by neighboring cells; if those neighbors are revealed, the canopy
    // shows above the shroud edge. gamemd gates these renders on the cell's
    // explored bit. Computed once and shared by both loops below.
    let cell_visibility_fog: Option<(
        crate::sim::intern::InternedId,
        &crate::sim::vision::FogState,
    )> = if state.match_state.sandbox_full_visibility {
        None
    } else {
        let local_owner_name = crate::app::input::commands::preferred_local_owner_name(state);
        match (
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation),
            &local_owner_name,
        ) {
            (Some(sim), Some(owner)) => sim.interner.get(owner).map(|id| (id, &sim.fog)),
            _ => None,
        }
    };

    // Overlay entries from [OverlayPack]. `YR TacticalClass::Draw` keeps walls
    // in the fixed cell overlay family, not the `LayerClass` object sort.
    let mut planned_cells = Vec::new();
    let mut next_draw_id = 0u64;
    for entry in state.match_state.match_presentation.overlays.iter() {
        if let Some((owner_id, fog)) = cell_visibility_fog {
            if !fog.is_cell_revealed(owner_id, entry.rx, entry.ry) {
                continue;
            }
        }

        let Some(static_name) = state
            .match_state
            .match_presentation
            .overlay_names
            .get(&entry.overlay_id)
        else {
            continue;
        };

        // High-bridge bodies are emitted by `instances::bridges` reading
        // `BridgeRuntimeCell` post-tick. Skip them here so they don't double-
        // render via the static map overlay list.
        if !ordinary_overlay_accepts_identity(entry.overlay_id, static_name) {
            continue;
        }

        // Low bridges remain in the ordinary overlay pass, but CellClass's
        // live identity—not the map-pack seed—selects damaged/collapsed art.
        let live_overlay_cell = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .and_then(|sim| sim.overlay_grid.as_ref())
            .map(|grid| grid.cell(entry.rx, entry.ry));
        let Some((live_overlay_id, live_overlay_data)) =
            overlay_render_identity(entry.overlay_id, entry.frame, live_overlay_cell)
        else {
            continue;
        };
        let overlay_registry = state.overlay_registry();
        let overlay_flags = overlay_registry.and_then(|reg| reg.flags(live_overlay_id));
        let slope_type = state
            .terrain_template()
            .and_then(|terrain| terrain.cell(entry.rx, entry.ry))
            .map(|cell| cell.slope_type);
        let (display_overlay_id, live_overlay_data) = overlay_display_identity(
            live_overlay_id,
            live_overlay_data,
            entry.rx,
            entry.ry,
            slope_type,
            overlay_registry,
            state.rules().map(|rules| &rules.tiberium_types),
        );
        let name = if display_overlay_id == live_overlay_id {
            state
                .match_state
                .match_presentation
                .overlay_names
                .get(&live_overlay_id)
                .cloned()
        } else {
            overlay_registry
                .and_then(|registry| registry.name(display_overlay_id).map(str::to_owned))
        };
        let Some(name) = name else {
            continue;
        };
        if !ordinary_overlay_accepts_identity(live_overlay_id, &name) {
            continue;
        }

        // The live identity owns resource type, wall/crate flags, and bridge
        // state even when a flat resource cell selects another display image.
        let is_wall: bool = overlay_flags.map(|f| f.wall).unwrap_or(false);
        let is_crate: bool = overlay_flags.map(|f| f.crate_type).unwrap_or(false);
        let render_frame: u8 = overlay_body_frame(is_crate, live_overlay_data);

        // FA2 IsoView.cpp:5955-5956: track overlays render +CellHeight (15px) lower.
        let track_y_offset: f32 = if overlay_flags.map(|f| f.track).unwrap_or(false) {
            15.0
        } else {
            0.0
        };

        let z: u8 = state
            .height_map()
            .get(&(entry.rx, entry.ry))
            .copied()
            .unwrap_or(0);
        let (screen_x, screen_y) = terrain::iso_to_screen(entry.rx, entry.ry, z);
        let screen_y: f32 = screen_y + track_y_offset;

        // No LocalSize gate: gamemd draws overlays on border filler cells like
        // any other; fog visibility and the camera clamp are the only hiders.
        if !in_view(
            screen_x, screen_y, 120.0, 120.0, cam_x, cam_y, sw, sh, 120.0,
        ) {
            continue;
        }

        // Exactly the frame the cell names, or nothing. Overlay draw:
        // `CellClass__DrawOverlay_Body @ 0x0047F6A0` blits `Cell+0x11E` with no
        // substitution, so a cell whose frame is empty art draws nothing — the
        // authored state for a low bridge's flanking columns (overlay data 0 and
        // 2, art only in frame 1). Collapsing to frame 0 here resurrected them
        // as phantom deck slabs. Frame selection lives in
        // `render::overlay_atlas::resolve_body_frame`.
        let key = OverlaySpriteKey {
            name: name.clone(),
            frame: render_frame,
        };
        let Some(spr) = atlas.get(&key) else { continue };
        let depth_z: u8 = z;
        let depth: f32 = compute_sprite_depth_params(origin_y, world_height, screen_y, depth_z);
        let (policy, z_adjust, z_gradient) =
            ordinary_overlay_z(overlay_flags, slope_type.unwrap_or(0), depth_z);
        let tint: [f32; 3] = state
            .match_state
            .match_presentation
            .lighting
            .grid()
            .overlay_tint_at((entry.rx, entry.ry));
        // 0047F6A0: ordinary cell Convert/common; resource and veins use
        // global ordinary Convert, with resource brightness fixed at 1000.
        // Wall owner palette remapping is still absent from this atlas.
        let light_grid = state.match_state.match_presentation.lighting.grid();
        let palette_light = if is_wall {
            Default::default()
        } else if overlay_flags.is_some_and(|f| f.tiberium) {
            crate::render::palette_light::PaletteLight::plain(53, 1000)
        } else if overlay_flags.is_some_and(|f| f.is_veins) {
            crate::render::palette_light::PaletteLight::plain(
                53,
                light_grid
                    .cell_light_at((entry.rx, entry.ry))
                    .map_or(1000, |l| l.common_scalar),
            )
        } else {
            crate::render::palette_light::PaletteLight::cell(
                light_grid,
                (entry.rx, entry.ry),
                false,
            )
        };
        planned_cells.push(
            crate::app::presentation::render::draw_plan_lowering::PlannedCellInstance {
                draw: crate::render::tactical_draw_plan::CellDraw {
                    id: next_draw_id,
                    kind: crate::app::presentation::render::draw_plan_lowering::cell_draw_kind(
                        is_wall,
                    ),
                    policy: BlitPolicy::translucent(SpriteEncoding::Terrain, policy),
                },
                instance: SpriteInstance {
                    position: [
                        screen_x + TILE_WIDTH / 2.0 + spr.offset_x,
                        screen_y + TILE_HEIGHT / 2.0 + spr.offset_y,
                    ],
                    size: spr.pixel_size,
                    uv_origin: spr.uv_origin,
                    uv_size: spr.uv_size,
                    depth,
                    tint,
                    palette_light,
                    alpha: 1.0,
                    z_adjust,
                    z_gradient,
                    ..Default::default()
                },
            },
        );
        next_draw_id += 1;
    }

    let (ordered, policies) =
        crate::app::presentation::render::draw_plan_lowering::lower_cell_instances_with_policy(
            planned_cells,
        );
    instances.extend(ordered);
    render_z.extend(policies);

    if std::env::var("RA2_DEBUG_BRIDGE_RENDER_BUCKETS").is_ok() {
        log::debug!("Cell overlay instances: {}", instances.len());
    }

    // Terrain objects from [Terrain] section.
    // Residual raw/animated terrain paths retain the old editor adjustment.
    // The proven ordinary extended body/shadow branch below does not use it.
    // FA2 IsoView.cpp:6389 applies a -3px Y fudge to terrain objects (trees, rocks):
    //   drawy = ... + f_y/2 - 3 - pic.wMaxHeight/2
    const TERRAIN_OBJECT_Y_FUDGE: f32 = -3.0;

    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    for obj in sim.production.terrain_objects.values() {
        if !obj.is_live() || !obj.in_logic_vector {
            continue;
        }
        let name = sim.interner.resolve(obj.type_ref);
        if let Some((owner_id, fog)) = cell_visibility_fog {
            if !fog.is_cell_revealed(owner_id, obj.rx, obj.ry) {
                continue;
            }
        }

        let z: u8 = state
            .height_map()
            .get(&(obj.rx, obj.ry))
            .copied()
            .unwrap_or(0);
        let (screen_x, screen_y) = terrain::iso_to_screen(obj.rx, obj.ry, z);
        if !in_view(
            screen_x, screen_y, 120.0, 120.0, cam_x, cam_y, sw, sh, 120.0,
        ) {
            continue;
        }

        // Animated terrain objects (flags) cycle through all frames using the
        // global idle animation timer. Static terrain uses frame 0.
        let frame: u8 = if let Some(count) = atlas.terrain_anim_frame_count(name) {
            // RA2 terrain animation rate: ~83ms per frame (12 fps).
            const TERRAIN_ANIM_RATE_MS: u32 = 83;
            let tick =
                state.match_state.match_presentation.idle_anim_elapsed_ms / TERRAIN_ANIM_RATE_MS;
            (tick % count as u32) as u8
        } else {
            0
        };
        let key = OverlaySpriteKey {
            name: name.to_string(),
            frame,
        };
        let Some(spr) = atlas.get(&key) else { continue };

        let depth: f32 = compute_sprite_depth_params(origin_y, world_height, screen_y, z);
        let spawns_tiberium = state
            .rules()
            .and_then(|rules| rules.terrain_object_type_case_insensitive(name))
            .map(|terrain_type| terrain_type.spawns_tiberium)
            .unwrap_or(false);
        let tint: [f32; 3] = state
            .match_state
            .match_presentation
            .lighting
            .grid()
            .terrain_object_tint_for_type((obj.rx, obj.ry), spawns_tiberium);
        // Terrain DrawIt 0071C286..0071C304: SpawnsTiberium selects the
        // global ordinary Convert/top; ordinary trees select cell Convert/common.
        let light_grid = state.match_state.match_presentation.lighting.grid();
        let palette_light = if spawns_tiberium {
            crate::render::palette_light::PaletteLight::plain(
                53,
                light_grid
                    .cell_light_at((obj.rx, obj.ry))
                    .map_or(1000, |l| l.top_scalar),
            )
        } else {
            crate::render::palette_light::PaletteLight::cell(light_grid, (obj.rx, obj.ry), false)
        };

        let Some(parent) = ground_order.terrain_object_draw(obj.stable_id, obj.rx, obj.ry) else {
            continue;
        };
        if let Some((body, shadow)) = atlas.native_static_terrain_pair(name) {
            use crate::app::presentation::render::draw_plan_lowering::{
                GroundPieceInstance, GroundTexture, PlannedGroundObjectInstance,
            };
            use crate::render::tactical_draw_plan::RenderZPolicy;
            use crate::render::terrain_draw::TerrainPiece;
            // Terrain DrawIt 0071C304/0071C34E preserves one draw point for
            // body and shadow. Each SHP's own integer canvas/frame offsets
            // then locate the stored rectangle. No FA2 editor Y adjustment.
            let point = [screen_x + TILE_WIDTH / 2.0, screen_y + TILE_HEIGHT / 2.0];
            let [body_instance, shadow_instance] =
                native_static_terrain_instances(body, shadow, point, z, depth, tint, palette_light);
            let mut parent = parent;
            parent.policy.render_z = RenderZPolicy::ReadWrite;
            ground_objects.push(PlannedGroundObjectInstance::object(
                parent,
                vec![
                    GroundPieceInstance {
                        target: GroundTexture::TerrainStatic(TerrainPiece::Body),
                        render_z: RenderZPolicy::ReadWrite,
                        instance: body_instance,
                    },
                    GroundPieceInstance {
                        target: GroundTexture::TerrainStatic(TerrainPiece::Shadow),
                        render_z: RenderZPolicy::ReadWrite,
                        instance: shadow_instance,
                    },
                ],
            ));
            continue;
        }
        ground_objects.push(
            crate::app::presentation::render::draw_plan_lowering::PlannedGroundObjectInstance::object(
                parent,
                vec![crate::app::presentation::render::draw_plan_lowering::GroundPieceInstance {
                    target: crate::app::presentation::render::draw_plan_lowering::GroundTexture::OverlayAtlas,
                    render_z: parent.policy.render_z,
                    instance: SpriteInstance {
                        position: [
                            screen_x + TILE_WIDTH / 2.0 + spr.offset_x,
                            screen_y + TILE_HEIGHT / 2.0 + spr.offset_y + TERRAIN_OBJECT_Y_FUDGE,
                        ],
                        size: spr.pixel_size,
                        uv_origin: spr.uv_origin,
                        uv_size: spr.uv_size,
                        depth,
                        tint,
                        palette_light,
                        alpha: 1.0,
                        ..Default::default()
                    },
                }],
            ),
        );
    }
}

/// Ordinary Terrain DrawIt 0071C304/0071C34E: both stored frames share one
/// projected draw point. Native class terms and gradients remain piece data.
fn native_static_terrain_instances(
    body: &crate::render::overlay_atlas::OverlaySpriteEntry,
    shadow: &crate::render::overlay_atlas::OverlaySpriteEntry,
    point: [f32; 2],
    height_level: u8,
    depth: f32,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
) -> [SpriteInstance; 2] {
    [(body, 2, -12), (shadow, 0, -3)].map(|(sprite, gradient, class_z)| SpriteInstance {
        position: [point[0] + sprite.offset_x, point[1] + sprite.offset_y],
        size: sprite.pixel_size,
        uv_origin: sprite.uv_origin,
        uv_size: sprite.uv_size,
        depth,
        tint,
        palette_light,
        alpha: 1.0,
        z_gradient: gradient,
        z_adjust: super::helpers::ground_z_adjust(height_level, class_z),
        ..Default::default()
    })
}

fn projectile_authoritative_screen_position(
    coordinate: ProjectileCoord,
) -> Option<(f32, f32, u16, u16, u8)> {
    let rx = u16::try_from(coordinate.x.div_euclid(256)).ok()?;
    let ry = u16::try_from(coordinate.y.div_euclid(256)).ok()?;
    let sub_x = SimFixed::from_num(coordinate.x.rem_euclid(256));
    let sub_y = SimFixed::from_num(coordinate.y.rem_euclid(256));
    let z = coordinate.z.clamp(0, i32::from(u8::MAX)) as u8;
    let (screen_x, screen_y) = crate::util::lepton::lepton_to_screen(rx, ry, sub_x, sub_y, z);
    Some((screen_x, screen_y, rx, ry, z))
}

/// Build visible persistent shots from `Simulation::projectiles`.
///
/// YR `BulletClass::AI` linkage: rendering reads the same committed CoordStruct
/// that the next authoritative flight pass will advance.
pub(crate) fn build_projectile_visual_instances(
    state: &AppState,
    paged: &mut [Vec<SpriteInstance>],
) {
    let (sim, rules, atlas) = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules().map(|r| r),
        &state.match_state.match_presentation.sprite_atlas,
    ) {
        (Some(sim), Some(rules), Some(atlas)) => (sim, rules, atlas),
        _ => return,
    };
    let z = state.match_state.input.zoom_level;
    let (cam_x, cam_y, sw, sh) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.render_width() as f32 / z,
        state.render_height() as f32 / z,
    );
    let (origin_y, world_height) = state
        .match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|grid| (grid.origin_y, grid.world_height))
        .unwrap_or((0.0, 1.0));

    for (_, projectile) in sim.projectiles.iter() {
        let Some(weapon) = rules.weapon(sim.interner.resolve(projectile.payload.weapon)) else {
            continue;
        };
        let Some(projectile_type_id) = weapon.projectile.as_deref() else {
            continue;
        };
        let Some(projectile_type) = rules.projectile(projectile_type_id) else {
            continue;
        };
        let Some(image) = projectile_type.image.as_deref() else {
            continue;
        };
        let Some((screen_x, screen_y, _rx, _ry, projectile_z)) =
            projectile_authoritative_screen_position(projectile.position)
        else {
            continue;
        };
        if !in_view(screen_x, screen_y, 96.0, 96.0, cam_x, cam_y, sw, sh, 96.0) {
            continue;
        }
        let frame_count =
            presentation_anim_frame_count(&atlas.active_anim_frame_counts, image).unwrap_or(32);
        let key = ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
            type_id: image.to_string(),
            facing: 0,
            frame: if frame_count == 0 {
                0
            } else {
                u16::from(crate::sim::projectile::projectile_shp_frame(projectile)) % frame_count
            },
            house_color: HouseColorIndex(0),
        };
        let Some(entry) = atlas.get(&key) else {
            continue;
        };
        // In-flight projectile shapes draw at FIXED full brightness: the native
        // bullet draw passes a literal neutral value for both its shadow and
        // body passes and never reads a cell lighting field at all. (An earlier
        // revision sampled the grid here; a merge briefly reintroduced that —
        // do not re-lit projectiles.)
        let tint = DEFAULT_TINT;
        let depth = compute_sprite_depth_params(origin_y, world_height, screen_y, projectile_z);
        paged[entry.page as usize].push(SpriteInstance {
            position: [screen_x + entry.offset_x, screen_y + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth,
            tint,
            alpha: 1.0,
            ..Default::default()
        });
    }
}

/// Build persistent WaveClass polygon edges from simulation registration state.
pub(crate) fn build_weapon_wave_instances(state: &AppState) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return instances;
    };
    let observer = crate::app::input::commands::preferred_local_owner(state)
        .as_deref()
        .and_then(|owner| sim.interner.get(owner));
    for wave in crate::app::presentation::fire_effects::build_weapon_wave_visuals(sim, observer) {
        let projected: Vec<[f32; 2]> = crate::render::wave_geometry::draw_order(wave.geometry)
            .into_iter()
            .map(|point| {
                let (x, y) = crate::map::terrain::lepton_to_screen(glam::IVec3::new(
                    point.x, point.y, point.z,
                ));
                [x, y]
            })
            .collect();
        instances.extend(crate::render::wave_geometry::build_wave_instances(
            &projected, wave.tint, 1.0, 0.00045,
        ));
    }
    // Live BuildingLightRuntime producers are intentionally not lowered here:
    // native DrawExtras first uses SpotlightClass type 16's zero-blend
    // shape-blitter/light-mask path, which this renderer does not yet expose.
    // A white quad or alpha approximation would invent visible behavior.
    instances
}

/// Emit one sprite instance per active parachute anim, anchored at the
/// descending GI's screen position with the SHP atlas's pre-baked
/// offset_x/offset_y handling sprite-center anchoring.
///
/// Depth: chute depth = GI body depth − epsilon, so it sorts above the body
/// in the same Layer 2 (Ground) band — matching gamemd's
/// AnimClass::GetLayer override that forces owner-attached anims to Layer 2
/// regardless of art.ini Layer=.
///
/// The body's key arrives in `body_depths` from `build_shp_instances`, which
/// must therefore have run first this frame. It is not re-derived here: the
/// body's key is anchored on the ground row the GI is descending onto, not on
/// the row it is drawn at, and a paradrop starts 216 px above that row. A
/// canopy keyed off the drawn row would sort ~14 iso rows behind the man
/// hanging on it and disappear behind any building in between.
///
/// Palette: AltPalette=yes selects the unit/Convert palette in gamemd. This
/// matches the default palette branch in `sprite_atlas` so long as the
/// PARACH frames are NOT registered in `effect_type_ids` (see Task 8).
pub(crate) fn build_parachute_instances(
    state: &AppState,
    ground_objects: &mut [crate::app::presentation::render::draw_plan_lowering::PlannedGroundObjectInstance],
    body_depths: &super::shp::ParachuteBodyDepths,
) {
    /// Depth epsilon — chute sorts slightly above the GI body. Half of the
    /// per-Z bias used in `compute_sprite_depth_params`. Increase if
    /// z-fighting is observed in-game.
    const CHUTE_DEPTH_EPSILON: f32 = 0.0005;

    /// Vertical lift, in pixels, applied to the chute sprite so the canopy
    /// sits above the GI's head rather than centered on the body. Tunable;
    /// gamemd's PARACH SHP layout produces this offset implicitly through
    /// frame-internal positioning, which our atlas doesn't replicate exactly.
    const CHUTE_Y_LIFT: f32 = 8.0;

    let (sim, atlas) = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.sprite_atlas,
    ) {
        (Some(s), Some(a)) => (s, a),
        _ => return,
    };
    let z = state.match_state.input.zoom_level;
    let (cam_x, cam_y, sw, sh) = (
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.render_width() as f32 / z,
        state.render_height() as f32 / z,
    );
    let config = match state
        .rules()
        .and_then(|r| r.general.parachute_render.as_ref())
    {
        Some(c) => c,
        None => return,
    };

    for anim in &state.match_state.match_presentation.parachute_anims {
        let entity = match sim.entities().get(anim.target_id) {
            Some(e) => e,
            None => continue,
        };
        // The body's own key for this frame. Absent means the body was culled
        // or never emitted, in which case there is nothing for a canopy to
        // hang on and nothing to sort it against.
        let Some(&body_depth) = body_depths.get(&anim.target_id) else {
            continue;
        };
        // Draw the chute exactly where the body is drawn, so it follows the
        // airborne GI rather than the ground beneath it.
        let (gx, gy) = crate::render::locomotor_visual::screen_position(entity);
        if !in_view(gx, gy, 200.0, 200.0, cam_x, cam_y, sw, sh, 200.0) {
            continue;
        }

        // Single-facing anim (no Facings= in art.ini for PARACH).
        let key = ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim,
            type_id: config.shp_name.clone(),
            facing: 0,
            frame: anim.frame,
            house_color: HouseColorIndex(0),
        };
        let Some(entry) = atlas.get(&key) else {
            // PARACH not yet loaded into the atlas. Logged once at startup
            // by the atlas-load path; per-frame silence here is intentional.
            continue;
        };
        let cx: f32 = gx + entry.offset_x;
        let cy: f32 = gy + entry.offset_y - CHUTE_Y_LIFT;

        // No owner tint and no lighting tint: chutes look identical
        // regardless of dropping house, matching gamemd's AltPalette=yes
        // path (ColorScheme[0]'s ConvertPalette).
        let tint: [f32; 3] = [1.0, 1.0, 1.0];

        // Depth: GI body depth minus a small epsilon so the chute draws on
        // top. ZAdjust=-10 in gamemd is a depth-sort fudge with no precise
        // pixel mapping; in our depth-buffer rendering, lower depth = closer
        // to camera = on top.
        let depth = (body_depth - CHUTE_DEPTH_EPSILON).clamp(0.001, 0.999);

        let Some(parent) = ground_objects
            .iter_mut()
            .find(|object| object.parent.id == anim.target_id)
        else {
            continue;
        };
        parent.pieces.push(
            crate::app::presentation::render::draw_plan_lowering::GroundPieceInstance {
                target:
                    crate::app::presentation::render::draw_plan_lowering::GroundTexture::ShpPage(
                        entry.page as usize,
                    ),
                // VERA-internal: the chute's native Z term (an anim ZAdjust of
                // -10 above a descending body) is not carried by this piece,
                // so it draws without a Z test rather than with a wrong one.
                render_z: crate::render::tactical_draw_plan::RenderZPolicy::None,
                instance: SpriteInstance {
                    position: [cx, cy],
                    size: entry.pixel_size,
                    uv_origin: entry.uv_origin,
                    uv_size: entry.uv_size,
                    depth,
                    tint,
                    palette_light: anim_palette_light(
                        state,
                        (entity.position.rx, entity.position.ry),
                        state
                            .rules()
                            .and_then(|r| r.art_registry.anim_runtime_config(&config.shp_name)),
                        false,
                    ),
                    alpha: 1.0,
                    ..Default::default()
                },
            },
        );
    }
}

#[cfg(test)]
mod tests {
    /// The sprite is projected from the anim's exact Z. Half a level up lands
    /// between the two level rows; the old level-byte projection drew it on the
    /// lower row, and drew a 104-frame level-1 coordinate on row 0.
    #[test]
    fn anim_sprite_is_projected_at_its_exact_height() {
        let at = |z: i32| {
            super::anim_world_render_coords(crate::sim::anim_class::AnimWorldCoord {
                x: 10 * 256 + 128,
                y: 12 * 256 + 128,
                z,
            })
        };
        let (ground, half, level_one) = (at(0), at(52), at(104));
        assert_eq!((ground.0, ground.4), (level_one.0, 0));
        assert_eq!(level_one.4, 1, "104 leptons is height level 1");
        assert!(
            level_one.1 < half.1 && half.1 < ground.1,
            "screen Y rises with exact Z: {} < {} < {}",
            level_one.1,
            half.1,
            ground.1
        );
        // The lift handed to depth is exactly what the projection subtracted,
        // on and between levels, so the depth row is the ground row.
        assert_eq!(ground.5, 0);
        for lifted in [half, level_one] {
            assert_eq!(lifted.1 + lifted.5 as f32, ground.1);
        }
        assert_eq!(level_one.5, 15, "one level is one 15 px height step");
    }

    #[test]
    fn static_terrain_body_and_shadow_use_native_shared_point_and_piece_z() {
        use crate::render::overlay_atlas::OverlaySpriteEntry;
        let body = OverlaySpriteEntry {
            uv_origin: [0.1, 0.2],
            uv_size: [0.2, 0.3],
            pixel_size: [33.0, 78.0],
            offset_x: -16.0,
            offset_y: -76.0,
        };
        let shadow = OverlaySpriteEntry {
            uv_origin: [0.4, 0.5],
            uv_size: [0.3, 0.4],
            pixel_size: [74.0, 36.0],
            offset_x: 2.0,
            offset_y: -34.0,
        };
        let palette =
            crate::render::palette_light::PaletteLight::new([288, 576, 992], 27, 799, false);
        let pieces = super::native_static_terrain_instances(
            &body,
            &shadow,
            [376.0, 404.0],
            0,
            0.3,
            [0.5, 0.6, 0.7],
            palette,
        );
        assert_eq!(pieces[0].position, [360.0, 328.0]);
        assert_eq!(pieces[1].position, [378.0, 370.0]);
        assert_eq!((pieces[0].z_gradient, pieces[0].z_adjust), (2, -12.0));
        assert_eq!((pieces[1].z_gradient, pieces[1].z_adjust), (0, -3.0));
        assert_eq!(pieces[0].palette_light, palette);
        assert_eq!(pieces[0].uv_origin, body.uv_origin);
        assert_eq!(pieces[1].uv_origin, shadow.uv_origin);
    }

    use super::{
        ANIM_DRAW_DEPTH_BIAS_PX, AnimRenderDestination, CRATE_BODY_FRAME, anim_instance_alpha,
        anim_render_destination, apply_shape_z_adjust, ordinary_overlay_accepts_identity,
        ordinary_overlay_z, overlay_body_frame, overlay_display_identity, overlay_render_identity,
        terrain_object_is_render_visible,
    };
    use crate::map::overlay::TerrainObject;
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::render::native_z::ZGradient;
    use crate::render::tactical_draw_plan::RenderZPolicy;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::overlay_types::OverlayTypeFlags;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::tiberium_type::TiberiumTypeRegistry;
    use crate::sim::intern::StringInterner;
    use crate::sim::overlay_grid::OverlayCell;
    use crate::sim::production::ProductionState;
    use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};
    use crate::util::fixed_math::SimFixed;

    #[test]
    fn numeric_high_bridge_identity_has_no_ordinary_overlay_instance_route() {
        assert!(!ordinary_overlay_accepts_identity(0x18, "HIGHANCHOR"));
        assert!(!ordinary_overlay_accepts_identity(0x19, "RENAMED_EW"));
        assert!(!ordinary_overlay_accepts_identity(0xED, "RENAMED_NS"));
        assert!(!ordinary_overlay_accepts_identity(0xEE, "BRIDGEB2"));
        assert!(ordinary_overlay_accepts_identity(0x20, "HIGHANCHOR"));
    }

    #[test]
    fn gsi_05_12_owner_attached_anim_is_forced_onto_the_sorted_ground_layer() {
        // `AnimClass::GetLayer @ 0x00424CB0` tests the owner at `Anim+0xCC`
        // FIRST and returns 2 (Ground); only an ownerless anim reaches the
        // AnimType `Layer=` read or the Top default. Layer 2 is the one sorted
        // `DisplayClass` layer (`Submit_Object @ 0x004A9720`,
        // `CMP EDI,0x2` at `0x004A9747` / `SETZ CL` at `0x004A974D`), so an attached anim is
        // y-sorted against ordinary ground objects rather than appended.
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[FIRE_TOP]\nLayer=top\nYSortAdjust=7\n\
             [FIRE_AIR]\nLayer=air\n",
        ));
        let order =
            crate::app::presentation::render::draw_plan_lowering::NativeGroundOrder::new(&[
                5, 10, 20, 30,
            ]);
        // The owner-resolved absolute, which is what
        // `ObjectClass::GetRenderCoords @ 0x0041BE00` hands the y-sort: it
        // re-enters the anim's own `GetCoords`, so the sort key is built from
        // the absolute, never from the stored owner-relative delta.
        let resolved = crate::sim::anim_class::AnimWorldCoord {
            x: 2450,
            y: 2653,
            z: 0,
        };

        let top_config = art.anim_runtime_config("FIRE_TOP");
        assert_eq!(
            anim_render_destination(10, None, resolved, top_config, &order),
            Some(AnimRenderDestination::Top),
            "without an owner the type's Layer=top still wins"
        );

        let Some(AnimRenderDestination::Ground(draw)) =
            anim_render_destination(10, Some(77), resolved, top_config, &order)
        else {
            panic!("an owner-attached anim must enter the sorted ground layer");
        };
        assert_eq!((draw.coord.x, draw.coord.y, draw.coord.z), (2450, 2653, 0));
        assert_eq!(draw.y_sort_adjust, 7);
        assert_eq!(
            draw.y_sort_key(),
            2450 + 2653 + 7,
            "ObjectClass::GetYSort @ 0x005F6BD0 returns coord.X + coord.Y, and \
             AnimClass::GetYSort @ 0x00422BC0 adds the AnimType YSortAdjust"
        );

        // The override is unconditional on the type: air is pulled down too.
        assert!(matches!(
            anim_render_destination(
                20,
                Some(77),
                resolved,
                art.anim_runtime_config("FIRE_AIR"),
                &order
            ),
            Some(AnimRenderDestination::Ground(_)),
        ));
        // And it does not need a known AnimType at all, because GetLayer
        // returns before it reads `Anim+0xC8`.
        assert!(matches!(
            anim_render_destination(30, Some(77), resolved, None, &order),
            Some(AnimRenderDestination::Ground(_)),
        ));
    }

    #[test]
    fn gsi_13_04_wa_top_and_tuntop_ground_use_native_layer_and_ysort_lowering() {
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[WA_CUSTOM]\nYSortAdjust=7\n\
             [TUNTOP_CUSTOM]\nLayer=ground\nYSortAdjust=1000\n",
        ));
        let order =
            crate::app::presentation::render::draw_plan_lowering::NativeGroundOrder::new(&[
                5, 10, 20,
            ]);
        let wa = art.anim_runtime_config("WA_CUSTOM");
        let tuntop = art.anim_runtime_config("TUNTOP_CUSTOM");
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: 400,
            y: 600,
            z: 208,
        };

        assert_eq!(
            anim_render_destination(10, None, world, wa, &order),
            Some(AnimRenderDestination::Top)
        );
        let Some(AnimRenderDestination::Ground(tunnel_draw)) =
            anim_render_destination(20, None, world, tuntop, &order)
        else {
            panic!("ground tile animation must enter TacticalDrawPlan");
        };
        assert_eq!(tunnel_draw.coord.x, 400);
        assert_eq!(tunnel_draw.coord.y, 600);
        assert_eq!(tunnel_draw.coord.z, 208);
        assert_eq!(tunnel_draw.y_sort_adjust, 1000);
        assert_eq!(tunnel_draw.y_sort_key(), 2000);

        let ordinary = order
            .object_draw(
                5,
                crate::render::tactical_draw_plan::TacticalCoord {
                    x: 900,
                    y: 900,
                    z: 0,
                },
                crate::render::tactical_draw_plan::SpriteEncoding::Plain,
            )
            .unwrap();
        let pieces = |parent| {
            crate::app::presentation::render::draw_plan_lowering::PlannedGroundObjectInstance::object(
                parent,
                vec![crate::app::presentation::render::draw_plan_lowering::GroundPieceInstance {
                    target: crate::app::presentation::render::draw_plan_lowering::GroundTexture::ShpPage(0),
                    render_z: crate::render::tactical_draw_plan::RenderZPolicy::ReadOnly,
                    instance: crate::render::batch::SpriteInstance::default(),
                }],
            )
        };
        let lowered =
            crate::app::presentation::render::draw_plan_lowering::lower_ground_object_instances(
                vec![pieces(tunnel_draw), pieces(ordinary)],
            );
        assert_eq!(lowered.owners, [5, 20]);
    }

    #[test]
    fn gsi_04_10_render_visibility_distinguishes_unregistered_live_and_destroyed() {
        let ini = IniFile::from_str(
            "[General]\n\
             FixtureOnly=1\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=DUMMY\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n\
             [DUMMY]\nStrength=100\n\
             [TREE01]\nFixtureOnly=1\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let registered = TerrainObject {
            rx: 4,
            ry: 7,
            name: "TREE01".to_string(),
        };
        let unregistered = TerrainObject {
            rx: 8,
            ry: 9,
            name: "MAPONLY".to_string(),
        };
        let mut production = ProductionState::default();

        assert!(terrain_object_is_render_visible(&registered, None, None));
        assert!(terrain_object_is_render_visible(
            &unregistered,
            Some(&rules),
            Some(&production),
        ));
        assert!(!terrain_object_is_render_visible(
            &registered,
            Some(&rules),
            Some(&production),
        ));

        let mut interner = StringInterner::default();
        let stable_id = 1;
        production.terrain_object_cells.insert((4, 7), stable_id);
        production.terrain_objects.insert(
            stable_id,
            TerrainObjectState {
                stable_id,
                native_unique_id: None,
                in_logic_vector: false,
                type_ref: interner.intern("TREE01"),
                rx: 4,
                ry: 7,
                health: 10,
                max_health: 10,
                occupation_bits: 7,
                lifecycle: TerrainObjectLifecycle::Live,
            },
        );
        assert!(terrain_object_is_render_visible(
            &registered,
            Some(&rules),
            Some(&production),
        ));

        production
            .terrain_objects
            .get_mut(&stable_id)
            .unwrap()
            .lifecycle = TerrainObjectLifecycle::Destroyed;
        assert!(!terrain_object_is_render_visible(
            &registered,
            Some(&rules),
            Some(&production),
        ));
    }

    #[test]
    fn damaged_wall_keeps_its_overlay_data_byte_as_the_render_frame() {
        // A GAWALL segment at damage stage 2 with all four neighbours joined
        // stores 0x2F. The renderer must ask the atlas for exactly that frame;
        // any collapse to 0 draws a pristine, isolated post.
        assert_eq!(overlay_body_frame(false, 0x2F), 0x2F);
        // Damage stage 1, N+E connected.
        assert_eq!(overlay_body_frame(false, 0x13), 0x13);
        // Undamaged, isolated.
        assert_eq!(overlay_body_frame(false, 0x00), 0x00);
    }

    #[test]
    fn ordinary_overlay_depth_uses_native_wall_and_nonwall_call_arguments() {
        // Parsed default matters: +0x2B3 starts true, but +0x2A8 Wall forces
        // gradient 2 even if DrawFlat is omitted or explicitly true.
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=WALL\n1=FLAT\n2=UPRIGHT\n3=ROCK\n4=ORE\n\
                 [WALL]\nWall=yes\n[FLAT]\n[UPRIGHT]\nDrawFlat=no\n\
                 [ROCK]\nDrawFlat=no\nIsARock=yes\n[ORE]\nTiberium=yes\nDrawFlat=no\n",
            ),
            None,
        );
        for (id, gradient, class_term) in [
            (0, ZGradient::Vertical, -2),
            (1, ZGradient::Flat, -2),
            (2, ZGradient::Vertical, -17),
            (3, ZGradient::Vertical, -2),
            (4, ZGradient::Flat, -2),
        ] {
            for level in [0, 1, 4, 14] {
                let (policy, z_adjust, packed) = ordinary_overlay_z(registry.flags(id), 0, level);
                assert_eq!(policy, RenderZPolicy::ReadWrite);
                assert_eq!(
                    packed, gradient as u32,
                    "ordinary overlays have no BUILDNGZ"
                );
                assert_eq!(z_adjust as i32, class_term - i32::from(level) * 15);
            }
        }
    }

    #[test]
    fn unsupported_overlay_slope_shapes_do_not_acquire_fabricated_depth() {
        let ore = OverlayTypeFlags {
            tiberium: true,
            ..Default::default()
        };
        assert_eq!(ordinary_overlay_z(Some(&ore), 1, 4).0, RenderZPolicy::None);
        assert_eq!(
            ordinary_overlay_z(Some(&ore), 0, 4).0,
            RenderZPolicy::ReadWrite
        );
        let wall = OverlayTypeFlags {
            wall: true,
            ..Default::default()
        };
        assert_eq!(
            ordinary_overlay_z(Some(&wall), 1, 4).0,
            RenderZPolicy::ReadWrite
        );
        assert_eq!(ordinary_overlay_z(None, 0, 0).0, RenderZPolicy::None);
    }

    #[test]
    fn crate_overlays_ignore_the_cell_byte_and_draw_frame_zero() {
        // gamemd's overlay-body draw takes a Crate=yes branch that hardcodes
        // the frame; the cell's overlay data never reaches the shape call.
        assert_eq!(overlay_body_frame(true, 0), CRATE_BODY_FRAME);
        assert_eq!(overlay_body_frame(true, 7), CRATE_BODY_FRAME);
        assert_eq!(overlay_body_frame(true, 0x2F), CRATE_BODY_FRAME);
    }

    #[test]
    fn gsi_04_13_low_overlay_renderer_follows_live_identity_through_terminal_collapse() {
        let mut live = OverlayCell {
            overlay_id: Some(0x50),
            overlay_data: 0xA5,
            wall_owner: None,
        };

        assert_eq!(overlay_render_identity(0x4A, 7, None), Some((0x4A, 7)));
        assert_eq!(
            overlay_render_identity(0x4A, 7, Some(&live)),
            Some((0x50, 0xA5)),
            "first-damaged low bridge must select the live overlay variant"
        );

        live.overlay_id = Some(0x64);
        assert_eq!(
            overlay_render_identity(0x4A, 7, Some(&live)),
            Some((0x64, 0xA5)),
            "terminal collapse remains a drawable overlay identity"
        );

        live.overlay_id = Some(0x4D);
        assert_eq!(
            overlay_render_identity(0x4A, 7, Some(&live)),
            Some((0x4D, 0xA5)),
            "repair art must follow the live healthy variant"
        );

        live.overlay_id = None;
        assert_eq!(overlay_render_identity(0x4A, 7, Some(&live)), None);

        live.overlay_id = Some(6);
        assert_eq!(
            overlay_render_identity(5, 3, Some(&live)),
            None,
            "ordinary overlays retain exact identity matching"
        );
    }

    #[test]
    fn gsi_13_05_flat_resource_changes_only_display_identity_on_known_flat_cells() {
        let mut text = String::from(
            "[Tiberiums]\n0=Riparius\n1=Cruentus\n\
             [Riparius]\nImage=1\n\
             [Cruentus]\nImage=2\n\
             [OverlayTypes]\n",
        );
        let mut resource_names = Vec::new();
        for overlay_id in 0..=113 {
            let name = match overlay_id {
                27..=38 => format!("GEM{:02}", overlay_id - 26),
                102..=113 => format!("TIB{:02}", overlay_id - 101),
                _ => format!("FILL{overlay_id:03}"),
            };
            text.push_str(&format!("{overlay_id}={name}\n"));
            if matches!(overlay_id, 27..=38 | 102..=113) {
                resource_names.push(name);
            }
        }
        for name in resource_names {
            text.push_str(&format!("[{name}]\nTiberium=yes\n"));
        }
        let ini = IniFile::from_str(&text);
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let tiberiums = TiberiumTypeRegistry::from_ini(&ini);
        let tib12 = overlays.id_for_name("TIB12").expect("TIB12");
        let tib01 = overlays.id_for_name("TIB01").expect("TIB01");
        let tib05 = overlays.id_for_name("TIB05").expect("TIB05");
        let gem12 = overlays.id_for_name("GEM12").expect("GEM12");
        let gem01 = overlays.id_for_name("GEM01").expect("GEM01");
        let non_tiberium = overlays.id_for_name("FILL000").expect("FILL000");

        assert_eq!(
            overlay_display_identity(tib12, 8, 4, 7, Some(0), Some(&overlays), Some(&tiberiums),),
            (tib05, 8)
        );
        assert_eq!(
            overlay_display_identity(gem12, 11, 0, 7, Some(0), Some(&overlays), Some(&tiberiums),),
            (gem01, 11)
        );
        assert_eq!(
            overlay_display_identity(tib12, 8, 4, 7, Some(2), Some(&overlays), Some(&tiberiums),),
            (tib12, 8),
            "sloped resource cells retain their stored display identity"
        );
        assert_eq!(
            overlay_display_identity(
                non_tiberium,
                9,
                4,
                7,
                Some(0),
                Some(&overlays),
                Some(&tiberiums),
            ),
            (non_tiberium, 9),
            "non-resource overlays remain unchanged"
        );
        assert_eq!(
            overlay_display_identity(
                tib12,
                8,
                u16::MAX,
                1,
                Some(0),
                Some(&overlays),
                Some(&tiberiums),
            ),
            (tib01.checked_sub(1).expect("registered base - 1"), 8),
            "signed base-relative display selection must not change live density state"
        );
    }

    #[test]
    fn apply_shape_z_adjust_is_pixel_exact_with_zero_neutral() {
        let world_height: f32 = 2000.0;
        let base: f32 = 0.5;
        // Neutral is 0, NOT 1000 (the 1000 convention is the per-cell terrain
        // path, a separate mechanism).
        assert_eq!(apply_shape_z_adjust(base, 0, world_height), base);
        // Negative = toward camera (smaller depth), pixel-exact magnitude.
        let toward = apply_shape_z_adjust(base, -300, world_height);
        assert!((toward - (base - 300.0 / world_height)).abs() < 1e-6);
        // Positive = away from camera.
        let away = apply_shape_z_adjust(base, 40, world_height);
        assert!(away > base);
        // Clamped to the valid depth range.
        assert_eq!(apply_shape_z_adjust(0.002, -100_000, world_height), 0.001);
        assert_eq!(apply_shape_z_adjust(0.998, 100_000, world_height), 0.999);
    }

    #[test]
    fn effective_anim_z_adjust_slot_overrides_type() {
        use crate::app::presentation::instances::helpers::effective_anim_z_adjust;
        // Nonzero slot override (e.g. ActiveAnimZAdjust=-100) wins.
        assert_eq!(effective_anim_z_adjust(-100, -300), -100);
        // Zero slot falls back to the anim type's own ZAdjust=.
        assert_eq!(effective_anim_z_adjust(0, -300), -300);
        assert_eq!(effective_anim_z_adjust(0, 0), 0);
    }

    #[test]
    fn anim_draw_bias_constant_matches_native() {
        assert_eq!(ANIM_DRAW_DEPTH_BIAS_PX, -2);
    }

    #[test]
    fn gsi_13_09_anim_emitters_carry_the_art_types_translucency_into_instance_alpha() {
        // BURN-S/M/L (`Translucency=25`) and the wake/warp family
        // (`Translucent=yes`) are the two stock shapes the emitters must honour.
        let ini = IniFile::from_str(
            "[BURNLIKE]\n\
             Translucency=25\n\
             [FIFTYLIKE]\n\
             Translucency=50\n\
             [WAKELIKE]\n\
             Translucent=yes\n\
             End=10\n\
             [PLAIN]\nFixtureOnly=1\n",
        );
        let reg = ArtRegistry::from_ini(&ini);

        // Translucency=N is N percent TRANSPARENT: 25 leaves three quarters of
        // the source. A fire drawn at 0.25 would be nearly invisible.
        let burn = reg.anim_runtime_config("BURNLIKE");
        assert_eq!(anim_instance_alpha(burn, 0, 8), 0.75);
        assert_eq!(anim_instance_alpha(burn, 7, 8), 0.75);
        assert_eq!(
            anim_instance_alpha(reg.anim_runtime_config("FIFTYLIKE"), 0, 8),
            0.5
        );

        // Translucent=yes is a progressive fade against End: opaque through
        // 0.2*End, then 3/4, 1/2, 1/4.
        let wake = reg.anim_runtime_config("WAKELIKE");
        assert_eq!(anim_instance_alpha(wake, 2, 10), 1.0);
        assert_eq!(anim_instance_alpha(wake, 3, 10), 0.75);
        assert_eq!(anim_instance_alpha(wake, 5, 10), 0.5);
        assert_eq!(anim_instance_alpha(wake, 9, 10), 0.25);

        // A type with neither key, and a type the registry never saw, draw
        // opaque — wiring this in cannot dim an animation that was not marked.
        assert_eq!(
            anim_instance_alpha(reg.anim_runtime_config("PLAIN"), 4, 8),
            1.0
        );
        assert_eq!(anim_instance_alpha(None, 4, 8), 1.0);
    }
}
