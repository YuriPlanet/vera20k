//! SHP entity instance builders — per-frame SpriteInstance generation for buildings and infantry.
//!
//! Handles building animation overlays (Active/Idle/Special), bibs, build-up
//! animations, and infantry sprite frame resolution.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use super::helpers::{
    ANIM_DRAW_DEPTH_BIAS_PX, EntityDrawBand, apply_shape_z_adjust, compute_sprite_depth,
    effective_anim_z_adjust, entity_draw_band, ground_sort_row, ground_z_adjust, in_view,
    tactical_entity_render_admission,
};
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    GroundPieceInstance, GroundTexture, NativeGroundOrder, PlannedBuildingPieceInstance,
    PlannedGroundObjectInstance,
};
use crate::map::entities::EntityCategory;
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::{DrawState, ObserverDrawContext};
use crate::render::native_z::{
    self, BIB_Z_ADJUST_PX, SHP_DRAW_Z_ADJUST_PX, ZGradient, ZSHAPE_MAX_FOUNDATION_WIDTH,
    pack_z_gradient,
};
use crate::render::sprite_atlas::ShpSpriteKey;
use crate::render::tactical_draw_plan::{
    BlitPolicy, BuildingPieceKind, SpriteEncoding, TacticalCoord,
};
use crate::render::unit_atlas::{UnitSpriteKey, VxlLayer, canonical_turret_facing};
use crate::rules::house_colors::HouseColorIndex;
use crate::sim::animation;
use crate::sim::components::BuildingUp;

/// Sort keys of the bodies currently hanging under a parachute, by entity id.
///
/// A parachute canopy is not an object in gamemd — `AnimClass::GetLayer` forces
/// an owner-attached anim into the owner's own layer, and the canopy is
/// composed into the descending body's draw. So the canopy has to sort at
/// exactly the body's key, and the only way to guarantee that is for the body
/// to hand its key over rather than for the canopy builder to re-derive one
/// from the same inputs and drift when either side changes.
///
/// Only ever read by key, never iterated, so the hash order is not observable.
pub(crate) type ParachuteBodyDepths = std::collections::HashMap<u64, f32>;

fn shp_body_tint(
    grid: &crate::map::lighting::CellLightGrid,
    cell: (u16, u16),
    category: EntityCategory,
    extra_unit_light: i32,
    extra_infantry_light: i32,
) -> [f32; 3] {
    match category {
        EntityCategory::Unit => grid.unit_tint_at(cell, extra_unit_light),
        EntityCategory::Infantry => grid.infantry_tint_at(cell, extra_infantry_light),
        EntityCategory::Structure => grid.building_body_tint_at(cell),
        _ => grid.techno_tint_at(cell),
    }
}

/// Iterate visible SHP sprite entities from EntityStore and build SpriteInstances.
///
/// Build SpriteInstances for all SHP entities (buildings, infantry).
/// Ground bodies, building bibs/anims, and building turret VXLs are emitted as
/// one parent-owned group so the native global order cannot split their display
/// call at an atlas boundary.
/// `top_instances` and aligned `top_pages` receive SHP bodies whose locomotor
/// puts them above the Ground
/// band — in stock YR that is the Rocketeer at hover height, the one infantry
/// type on a Jumpjet locomotor.
/// `parachute_body_depths` collects the sort key of every body currently under
/// a parachute, keyed by entity — see [`ParachuteBodyDepths`].
/// Building bodies write their own per-pixel Z in the Ground pass, which is
/// what the post-shroud selection-bracket redraw tests against; no separate
/// depth stamp exists any more.
pub(crate) fn build_shp_instances(
    state: &AppState,
    paged: &mut [Vec<SpriteInstance>],
    top_instances: &mut Vec<SpriteInstance>,
    top_pages: &mut Vec<usize>,
    top_ids: &mut Vec<u64>,
    parachute_body_depths: &mut ParachuteBodyDepths,
    ground_objects: &mut Vec<PlannedGroundObjectInstance>,
    ground_order: &NativeGroundOrder,
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
    let z = state.match_state.input.zoom_level;
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

    let encounter_order = super::helpers::tactical_entity_encounter_order(sim, state.rules());
    for stable_id in encounter_order {
        let Some(entity) = sim.entities().get(stable_id) else {
            continue;
        };
        if entity.is_voxel {
            continue;
        }
        // Common visibility, passenger, limbo, and DrawState admission is shared below.
        let owner_str = sim.interner.resolve(entity.owner());
        let active_disguise = entity.disguise.as_ref().filter(|state| state.disguised);
        let type_str = active_disguise
            .and_then(|state| state.disguise_type)
            .map(|id| sim.interner.resolve(id))
            .unwrap_or_else(|| sim.interner.resolve(entity.type_ref()));
        let remap_owner = active_disguise
            .and_then(|state| state.disguised_as_house)
            .map(|id| sim.interner.resolve(id))
            .unwrap_or(owner_str);
        // Wall buildings render as overlays (auto-tiled connectivity frames).
        // Their Y-sorted rendering in the object pass is handled by including
        // wall overlay instances in the unified merge (draw_merged_object_pass),
        // not here. Skip them to avoid drawing frame 0 (isolated pillar).
        if entity.category == EntityCategory::Structure {
            let is_wall = state
                .rules()
                .and_then(|r| r.object(type_str))
                .map(|o| o.wall)
                .unwrap_or(false);
            if is_wall {
                continue;
            }
        }
        let pos = &entity.position;
        let hc: HouseColorIndex = state
            .match_state
            .match_presentation
            .house_color_map
            .get(remap_owner)
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        let Some(draw_decision) = tactical_entity_render_admission(
            entity,
            owner_str,
            local_owner.as_deref(),
            local_owner_id,
            &sim.fog,
            ignore_visibility,
            sim.session.binary_frame,
            super::units::house_color_to_remap_row(hc),
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
        // Buildings are the one class gamemd draws off its own render-coordinate
        // virtual rather than the plain object coordinate; everything below this
        // point — body, bib, anims, turret, and the row they sort on — wants that
        // lifted anchor. See `locomotor_visual::BUILDING_ART_LIFT_PX`.
        let (sx, sy) = {
            let anchor = crate::render::locomotor_visual::screen_position(entity);
            if entity.category == EntityCategory::Structure {
                crate::render::locomotor_visual::building_art_anchor(anchor.0, anchor.1)
            } else {
                anchor
            }
        };
        let interp_z = pos.z;
        if !in_view(sx, sy, 200.0, 200.0, cam_x, cam_y, sw, sh, 200.0) {
            continue;
        }
        let draw_state = draw_decision.state;
        // Determine if this building is in its make/build-up or build-down animation.
        let is_building_up: bool =
            entity.category == EntityCategory::Structure && entity.building_up.is_some();
        let is_building_down: bool =
            entity.category == EntityCategory::Structure && entity.building_down.is_some();
        let (shp_frame, make_type_id): (u16, Option<String>) = if is_building_up {
            let bu: &BuildingUp = entity.building_up.as_ref().expect("checked above");
            let make_key: String = format!("{}_MAKE", type_str);
            let total_make_frames: u16 =
                atlas.make_frame_counts.get(&make_key).copied().unwrap_or(0);
            if total_make_frames > 0 {
                // Map elapsed ticks to make frame index (forward: 0 → last).
                let progress: f32 = bu.elapsed_ticks as f32 / bu.total_ticks.max(1) as f32;
                let frame: u16 =
                    ((progress * total_make_frames as f32) as u16).min(total_make_frames - 1);
                (frame, Some(make_key))
            } else {
                (0, None)
            }
        } else if is_building_down {
            let bd = entity.building_down.as_ref().expect("checked above");
            let make_key: String = format!("{}_MAKE", type_str);
            let total_make_frames: u16 =
                atlas.make_frame_counts.get(&make_key).copied().unwrap_or(0);
            if total_make_frames > 0 {
                // Map elapsed ticks to make frame index in reverse (last → 0).
                let progress: f32 = bd.elapsed_ticks as f32 / bd.total_ticks.max(1) as f32;
                let reverse_frame: u16 = total_make_frames.saturating_sub(1).saturating_sub(
                    ((progress * total_make_frames as f32) as u16).min(total_make_frames - 1),
                );
                (reverse_frame, Some(make_key))
            } else {
                (0, None)
            }
        } else {
            match entity.category {
                EntityCategory::Structure => {
                    let obj = state.rules().and_then(|r| r.object(type_str));
                    let frame = if let Some(obj) = obj.filter(|o| o.can_be_occupied) {
                        let occupant_count = entity
                            .passenger_role
                            .cargo()
                            .map(|c| c.count())
                            .unwrap_or(0);
                        let tech_level = obj.tech_level;
                        let (cy, cr) = state
                            .rules()
                            .map(|r| (r.general.condition_yellow, r.general.condition_red))
                            .unwrap_or((0.5, 0.25));
                        building_frame_index(
                            occupant_count,
                            entity.health.current,
                            obj.strength,
                            tech_level,
                            cy,
                            cr,
                        )
                    } else {
                        0
                    };
                    (frame, None)
                }
                _ => (resolve_infantry_shp_frame(state, type_str, entity), None),
            }
        };
        let key: ShpSpriteKey = ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
            type_id: make_type_id.as_deref().unwrap_or(type_str).to_string(),
            facing: 0,
            frame: shp_frame,
            house_color: hc,
        };
        // Fallback: a CanBeOccupied building requesting frame 1/2/3 may miss the
        // atlas if its SHP has fewer than 4 frames. Retry with frame 0 so the
        // building still draws (Approach A in the design doc). Non-garrisonable
        // misses keep their existing skip path.
        let entry = match atlas.get(&key) {
            Some(e) => e,
            None if shp_frame != 0
                && entity.category == EntityCategory::Structure
                && state
                    .rules()
                    .and_then(|r| r.object(type_str))
                    .map(|o| o.can_be_occupied)
                    .unwrap_or(false) =>
            {
                let fallback_key = ShpSpriteKey {
                    palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                    type_id: make_type_id.as_deref().unwrap_or(type_str).to_string(),
                    facing: 0,
                    frame: 0,
                    house_color: hc,
                };
                match atlas.get(&fallback_key) {
                    Some(e) => e,
                    None => continue,
                }
            }
            None => continue,
        };

        let final_x: f32 = sx + entry.offset_x;
        let final_y: f32 = sy + entry.offset_y;
        let band = entity_draw_band(entity);
        let base_depth: f32 = match entity.category {
            EntityCategory::Structure => {
                // `sy` already carries the render-coordinate lift, so it *is* the
                // NW footprint cell's tile row — the row gamemd's YSort (X + Y
                // off the render coords) reduces to. A building therefore sorts
                // on its own cell rather than one iso row north of it.
                compute_sprite_depth(state, sy, interp_z)
            }
            _ => {
                // The drawn row carries this body's height lift; the sort key
                // must not. A hovering Rocketeer or a descending paradrop key
                // off the cell it is over, exactly like a GI standing there.
                let depth_y: f32 = sy + entry.canvas_rect[1] + entry.canvas_rect[3];
                compute_sprite_depth(state, ground_sort_row(entity, depth_y), interp_z)
            }
        };
        let depth: f32 = base_depth;
        if entity.parachute_state.is_some() {
            parachute_body_depths.insert(entity.stable_id(), depth);
        }
        let tint = shp_body_tint(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx, pos.ry),
            entity.category,
            state
                .rules()
                .map_or(0, |rules| rules.general.extra_unit_light),
            state
                .rules()
                .map_or(0, |rules| rules.general.extra_infantry_light),
        );
        let selected_palette_light = crate::app::presentation::lighting::body_palette_light(
            state.match_state.match_presentation.lighting.grid(),
            &sim.session.lighting,
            (pos.rx, pos.ry),
            entity.category,
            state.rules().map_or(0, |r| r.general.extra_unit_light),
            state.rules().map_or(0, |r| r.general.extra_infantry_light),
        );
        let palette_light = if entity.category == EntityCategory::Structure {
            let obj = state.rules().and_then(|r| r.object(type_str));
            let image = obj.map_or(type_str, |o| o.image.as_str());
            let art = state
                .rules()
                .and_then(|r| r.art_registry.resolve_metadata_entry(type_str, image));
            crate::app::presentation::lighting::building_palette_light(
                state.match_state.match_presentation.lighting.grid(),
                &sim.session.lighting,
                (pos.rx, pos.ry),
                art,
                is_building_up || is_building_down,
            )
        } else {
            selected_palette_light
        };
        // Direct SHP and infantry keep native Ground parent order. Infantry
        // does not use the Unit composite bridge split at 0x73B140.
        let collect_ground = band == EntityDrawBand::Ground;
        // Native per-pixel Z. Buildings (`BuildingClass_DrawBody`, flags
        // 0x6E00) seed from `NormalZAdjust - AdjustForZ(Z)`
        // minus DrawSHP's 2, write Z, and subtract the BUILDNGZ z-shape placed
        // by `ZShapePointMove` and the foundation (dropped for foundations 8
        // wide or more). Raw frames only clip to the shape. Extended BUILDNGZ
        // uses a constant seed;
        // ordinary sprites walk gradient 2. Infantry (`InfantryClass Draw_It`, flags 0x2E00) walks
        // entry 2 from `Get_Z_Adjust - 2`, whose base term also cancels the
        // lift, and never writes.
        let (z_adjust, z_gradient, zshape_origin) = if entity.category == EntityCategory::Structure
        {
            let object_type = state.rules().and_then(|r| r.object(type_str));
            let rules_image: String = object_type
                .map(|o| o.image.clone())
                .unwrap_or_else(|| type_str.to_string());
            let art_entry = art_reg.and_then(|a| a.resolve_metadata_entry(type_str, &rules_image));
            let normal_z_adjust: i32 = art_entry.map_or(0, |a| a.normal_z_adjust);
            let point_move: (i32, i32) = art_entry.map_or((0, 0), |a| a.z_shape_point_move);
            let foundation: (u16, u16) = object_type
                .map(|o| crate::rules::foundation::foundation_dimensions(&o.foundation))
                .unwrap_or((1, 1));
            let zshape = state
                .match_state
                .match_presentation
                .building_zshape
                .is_some()
                && foundation.0 <= ZSHAPE_MAX_FOUNDATION_WIDTH;
            let origin = native_z::zshape_origin(
                (sx.round() as i32, sy.round() as i32),
                foundation,
                point_move,
            );
            (
                ground_z_adjust(interp_z, normal_z_adjust + SHP_DRAW_Z_ADJUST_PX),
                native_z::pack_building_z_gradient(zshape, entry.extended),
                [origin.0 as f32, origin.1 as f32],
            )
        } else {
            // Unit no-turret SHP branch 0x73CE0D..0x73CE7F, active for SQD:
            // one complete draw with a7=-16, a8=0. Infantry is excluded by
            // the adapter. This is distinct from VXL's two-region composite.
            let bridge_fudge = super::foot_depth::shp_unit_bridge_fudge(state, entity);
            (
                super::foot_depth::shp_z_adjust(state, entity)
                    - if bridge_fudge { 16.0 } else { 0.0 },
                pack_z_gradient(
                    if bridge_fudge {
                        ZGradient::Flat
                    } else {
                        ZGradient::Vertical
                    },
                    false,
                ),
                [0.0, 0.0],
            )
        };
        let body = SpriteInstance {
            position: [final_x, final_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth,
            tint,
            palette_light,
            alpha: 1.0,
            draw_state,
            z_adjust,
            z_gradient,
            zshape_origin,
        };

        let mut building_pieces = Vec::new();
        if entity.category == EntityCategory::Structure {
            building_pieces.push(PlannedBuildingPieceInstance {
                kind: if is_building_up || is_building_down {
                    BuildingPieceKind::BuildupOrSpecial
                } else {
                    BuildingPieceKind::Body
                },
                z_bias: 0,
                // Body and buildup go through the same Z-writing body draw.
                policy: BlitPolicy::opaque(SpriteEncoding::Plain),
                target: GroundTexture::ShpPage(entry.page as usize),
                instance: body,
            });
        } else if collect_ground {
            let coord = TacticalCoord {
                x: i32::from(pos.rx) * 256 + crate::util::fixed_math::sim_to_i32(pos.sub_x),
                y: i32::from(pos.ry) * 256 + crate::util::fixed_math::sim_to_i32(pos.sub_y),
                z: i32::from(pos.z),
            };
            if let Some(parent) =
                ground_order.object_draw(entity.stable_id(), coord, SpriteEncoding::Plain)
            {
                ground_objects.push(PlannedGroundObjectInstance::object(
                    parent,
                    vec![GroundPieceInstance {
                        target: GroundTexture::ShpPage(entry.page as usize),
                        render_z: parent.policy.render_z,
                        instance: body,
                    }],
                ));
            }
        } else if band == EntityDrawBand::Top {
            top_instances.push(body);
            top_pages.push(entry.page as usize);
            top_ids.push(entity.stable_id());
        } else {
            paged[entry.page as usize].push(body);
        }

        // Emit building animation overlays and bib — but NOT during build-up/down.
        // Bibs and anims use the raw cell position (sy) — their own SHP offsets
        // (baked into the canvas) handle correct placement relative to the cell.
        if entity.category == EntityCategory::Structure && !is_building_up && !is_building_down {
            if let Some(art) = art_reg {
                // Bib is drawn INSIDE BuildingClass_DrawBody in the original engine,
                // right after the main body sprite, as part of the same object pass.
                // It overwrites the body's terrain-colored pixels at the ramp area.
                emit_building_bib(
                    &mut building_pieces,
                    atlas,
                    art,
                    state.rules(),
                    type_str,
                    hc,
                    sx,
                    sy,
                    interp_z,
                    depth,
                    tint,
                    palette_light,
                    draw_state,
                );
                // Building anims render in the same pass as building bodies so they
                // can sort together via depth. Anims use the building's entity depth
                // so they render at the same depth as the body — visible where the
                // body has transparent pixels, covered where it's opaque.
                let world_height: f32 = state
                    .match_state
                    .match_presentation
                    .terrain_grid
                    .as_ref()
                    .map(|g| g.world_height)
                    .unwrap_or(1.0);
                emit_building_anims(
                    &mut building_pieces,
                    atlas,
                    art,
                    state.rules(),
                    type_str,
                    hc,
                    sx,
                    sy,
                    depth,
                    tint,
                    selected_palette_light,
                    state.match_state.match_presentation.lighting.grid(),
                    &sim.session.lighting,
                    (pos.rx, pos.ry),
                    sim,
                    &entity.building_anim_slots,
                    world_height,
                    draw_state,
                    interp_z,
                );
            }
            // Emit VXL turret on top of building (e.g., SAM site, Prism Tower).
            if let Some(rules_obj) = state.rules().and_then(|r| r.object(type_str)) {
                if rules_obj.turret_anim_is_voxel {
                    if let Some(turret_id) = &rules_obj.turret_anim {
                        if let Some((page, instance)) = emit_building_turret_vxl(
                            state,
                            turret_id,
                            entity
                                .barrel_facing
                                .as_ref()
                                .map(|f| f.current(sim.session.binary_frame))
                                .unwrap_or(0u16),
                            hc,
                            sx,
                            sy,
                            interp_z,
                            depth,
                            tint,
                            // Building VXL 0043DA80 reads top directly (e.g.
                            // 0043E2B4..0043E2FF); 00707194 selects the scheme.
                            // SHP TerrainPalette/ExtraLight do not apply here.
                            selected_palette_light,
                            draw_state,
                            rules_obj.turret_anim_x,
                            rules_obj.turret_anim_y,
                        ) {
                            building_pieces.push(PlannedBuildingPieceInstance {
                                kind: BuildingPieceKind::PoweredOrActiveOverlay,
                                z_bias: 0,
                                // `FUN_0043DA80` -> `TechnoClass__Draw` 0x2800:
                                // the turret tests Z and never writes.
                                policy: BlitPolicy::z_read(SpriteEncoding::Voxel),
                                target: GroundTexture::UnitAtlasPage(page),
                                instance,
                            });
                        }
                    }
                }
            }
        }

        if entity.category == EntityCategory::Structure {
            let location = TacticalCoord {
                x: i32::from(pos.rx) * 256 + crate::util::fixed_math::sim_to_i32(pos.sub_x),
                y: i32::from(pos.ry) * 256 + crate::util::fixed_math::sim_to_i32(pos.sub_y),
                z: i32::from(pos.z),
            };
            let actual_type = state
                .rules()
                .and_then(|rules| rules.object(sim.interner.resolve(entity.type_ref())));
            if let Some(parent) = actual_type.and_then(|object_type| {
                ground_order.building_object_draw(
                    entity.stable_id(),
                    location,
                    object_type,
                    SpriteEncoding::Plain,
                )
            }) {
                ground_objects.push(PlannedGroundObjectInstance::building(
                    parent,
                    building_pieces,
                ));
            }
        }
    }
}

/// Emit a VXL turret sprite on top of a building (e.g., SAM site turret, Prism Tower).
///
/// Looks up the pre-rendered turret VXL from the UnitAtlas at the current turret facing,
/// positioned at the building's screen origin + pixel offset from TurretAnimX/Y.
///
/// The turret carries the building's own sort depth verbatim. `TurretAnimZAdjust=`
/// is deliberately not folded in: gamemd's ground-layer sort key for a building
/// reads only `TurretAnimIsVoxel` (+32 leptons) and `Gate` (−16 leptons), while
/// `TurretAnimZAdjust` is read by a different virtual that composes the
/// building's *draw* Z. Using it as a sort bias here would pull the turret
/// toward the camera and put it over units standing in front of the building —
/// by 1.3 to 4 iso rows on the defences a player actually fights around
/// (SAM Site and Sentry Gun −20, Flak and Gattling Cannon −40, Slave Miner −50,
/// Grand Cannon −60), more on a couple of civilian map props. The set is small
/// and does not include Prism Tower, whose turret is not a voxel.
fn emit_building_turret_vxl(
    state: &AppState,
    turret_id: &str,
    turret_facing: u16,
    _hc: HouseColorIndex,
    building_sx: f32,
    building_sy: f32,
    z: u8,
    building_depth: f32,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    draw_state: DrawState,
    anim_x: i32,
    anim_y: i32,
) -> Option<(usize, SpriteInstance)> {
    let unit_atlas = match &state.match_state.match_presentation.unit_atlas {
        Some(a) => a,
        None => return None,
    };
    let key = UnitSpriteKey {
        type_id: turret_id.to_string(),
        facing: canonical_turret_facing(turret_facing),
        layer: VxlLayer::Composite,
        frame: 0,
        slope_type: 0, // building turrets don't tilt on slopes
    };
    let entry = unit_atlas.get(&key)?;
    // Position turret at building cell origin + pixel offset from INI.
    // TurretAnimX/Y are screen pixel offsets added to the building's own draw
    // point, which is exactly what the native turret draw does with them.
    let center_x: f32 = building_sx;
    let tx: f32 = center_x + anim_x as f32 + entry.offset_x;
    let ty: f32 = building_sy + anim_y as f32 + entry.offset_y + 3.0;
    Some((
        entry.page,
        SpriteInstance {
            position: [tx, ty],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth: building_depth,
            tint,
            palette_light,
            alpha: 1.0,
            draw_state,
            // VXL blit: gradient entry 2, lift cancelled, no DrawSHP -2.
            z_adjust: ground_z_adjust(z, 0),
            z_gradient: pack_z_gradient(ZGradient::Vertical, false),
            ..Default::default()
        },
    ))
}

/// Emit the BibShape SpriteInstance for a building's ground-level pad.
///
/// BibShape is a separate SHP (e.g., GAREFNBB for the Allied Refinery dock) drawn
/// behind the building at the same cell position. It provides the flat ground
/// surface where harvesters dock or other ground-level detail.
fn emit_building_bib(
    pieces: &mut Vec<PlannedBuildingPieceInstance>,
    atlas: &crate::render::sprite_atlas::SpriteAtlas,
    art_reg: &crate::rules::art_data::ArtRegistry,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    building_type: &str,
    house_color: HouseColorIndex,
    screen_x: f32,
    screen_y: f32,
    z: u8,
    building_depth: f32,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    draw_state: DrawState,
) {
    let rules_image: String = rules
        .and_then(|r| r.object(building_type))
        .map(|o| o.image.clone())
        .unwrap_or_else(|| building_type.to_string());
    let art_entry = match art_reg.resolve_metadata_entry(building_type, &rules_image) {
        Some(e) => e,
        None => return,
    };
    let bib_name: &str = match art_entry.bib_shape.as_deref() {
        Some(name) => name,
        None => return,
    };
    let bib_key: ShpSpriteKey = ShpSpriteKey {
        palette_context: if art_entry.terrain_palette {
            crate::render::sprite_atlas::ShpPaletteContext::Cell
        } else {
            crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme
        },
        type_id: bib_name.to_uppercase(),
        facing: 0,
        frame: 0,
        house_color,
    };
    let Some(bib_entry) = atlas.get(&bib_key) else {
        return;
    };
    let bx: f32 = screen_x + bib_entry.offset_x;
    let by: f32 = screen_y + bib_entry.offset_y;
    // The bib is drawn inside the building's draw body pass — it doesn't sort
    // independently. The entire building (body + bib) sorts as one unit at the
    // building's YSort position. Use the building's depth so bib and body stay
    // together in the Y-sorted merge, preventing bibs from incorrectly
    // overlapping walls at closer iso rows.
    //
    // Native Z (`BuildingClass_DrawBody @ 0x0043D9C9`): the bib is a second
    // 0x6E00 draw with gradient entry 0, a7 = `-1 - AdjustForZ(Z)`, no
    // z-shape; it tests and writes Z like the body.
    pieces.push(PlannedBuildingPieceInstance {
        kind: BuildingPieceKind::Bib,
        z_bias: 0,
        policy: BlitPolicy::opaque(SpriteEncoding::Plain),
        target: GroundTexture::ShpPage(bib_entry.page as usize),
        instance: SpriteInstance {
            position: [bx, by],
            size: bib_entry.pixel_size,
            uv_origin: bib_entry.uv_origin,
            uv_size: bib_entry.uv_size,
            depth: building_depth,
            tint,
            palette_light,
            alpha: 1.0,
            draw_state,
            z_adjust: ground_z_adjust(z, BIB_Z_ADJUST_PX + SHP_DRAW_Z_ADJUST_PX),
            z_gradient: pack_z_gradient(ZGradient::Flat, false),
            ..Default::default()
        },
    });
}

/// Emit SpriteInstances for a building's animation overlays.
///
/// Each anim overlay (e.g., CAOILD_A for Oil Derrick's tower) is looked up
/// in the sprite atlas and positioned at the building's cell center + the
/// animation's (X, Y) pixel offset from art.ini.
fn emit_building_anims(
    pieces: &mut Vec<PlannedBuildingPieceInstance>,
    atlas: &crate::render::sprite_atlas::SpriteAtlas,
    art_reg: &crate::rules::art_data::ArtRegistry,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    building_type: &str,
    house_color: HouseColorIndex,
    screen_x: f32,
    screen_y: f32,
    building_depth: f32,
    tint: [f32; 3],
    attached_palette: crate::render::palette_light::PaletteLight,
    light_grid: &crate::map::lighting::CellLightGrid,
    scenario: &crate::sim::scenario_session::ScenarioLightingState,
    cell: (u16, u16),
    sim: &crate::sim::world::Simulation,
    slots: &[Option<u64>; 21],
    world_height: f32,
    draw_state: DrawState,
    z: u8,
) {
    let rules_image: String = rules
        .and_then(|r| r.object(building_type))
        .map(|o| o.image.clone())
        .unwrap_or_else(|| building_type.to_string());
    let art_entry = match art_reg.resolve_metadata_entry(building_type, &rules_image) {
        Some(e) => e,
        None => return,
    };
    for anim in &art_entry.building_anims {
        let Some(instance) = slots[usize::from(anim.native_slot)].and_then(|id| sim.anim(id))
        else {
            continue;
        };
        if instance.runtime.inactive || instance.draw_runtime.hidden {
            continue;
        }
        let anim_name = sim.interner.resolve(instance.type_id);
        let Some(runtime_config) = art_reg.anim_runtime_config(anim_name) else {
            continue;
        };
        if !runtime_config.art_body_read {
            continue;
        }
        // Anim+AC is relative to this instance's current type Start. Replacement
        // copies only AC; its timer/loop state remains the new constructor's.
        let Ok(frame) = u16::try_from(
            instance
                .runtime
                .current_frame
                .wrapping_add(runtime_config.start),
        ) else {
            continue;
        };
        let context =
            crate::render::sprite_atlas::attached_anim_palette_context(Some(runtime_config));
        let anim_key: ShpSpriteKey = ShpSpriteKey {
            palette_context: context,
            type_id: anim_name.to_string(),
            facing: 0,
            frame,
            house_color: if context == crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim {
                HouseColorIndex(0)
            } else {
                house_color
            },
        };
        let anim_entry_opt = atlas.get(&anim_key);

        let Some(anim_entry) = anim_entry_opt else {
            continue;
        };

        // Position: cell center + anim X/Y offset from art.ini.
        // Building anims use building positioning (building convention).
        // The anim's own draw offset (XDrawOffset/YDrawOffset) is already baked
        // into anim_entry.offset_x/y by the sprite atlas builder.
        let ax: f32 = screen_x + anim.x as f32 + anim_entry.offset_x;
        let ay: f32 = screen_y + anim.y as f32 + anim_entry.offset_y;

        // Building anims start from the building body's depth (emitted in the
        // same pass; on-top-of-own-body comes from instance order), then apply
        // the native ZAdjust sort bias: the per-slot override from the
        // building's art section (e.g. ActiveAnimZAdjust=) wins when nonzero,
        // else the anim type's own ZAdjust= applies; anim SHP draws also carry
        // a constant -2px bias. Negative = toward camera. This orders anims
        // correctly against OTHER nearby objects.
        let type_z_adjust: i32 = art_reg
            .anim_runtime_config(anim_name)
            .map(|c| c.z_adjust)
            .unwrap_or(0);
        let z_adjust_px: i32 =
            effective_anim_z_adjust(anim.z_adjust, type_z_adjust) + ANIM_DRAW_DEPTH_BIAS_PX;
        let anim_depth: f32 = apply_shape_z_adjust(building_depth, z_adjust_px, world_height);

        let config = art_reg.anim_runtime_config(anim_name);
        // Building mark 0043F9A6..0043FA68 supplies explicit selected Convert/top
        // only when ShouldUseCellDrawer. Its effect brightness hook remains a
        // residual for affected buildings; ordinary brightness is unchanged.
        let palette_light = if config.is_none_or(|c| c.should_use_cell_drawer) {
            if config.is_some_and(|c| c.use_normal_light) {
                attached_palette.with_brightness(1000)
            } else {
                attached_palette
            }
        } else {
            crate::app::presentation::lighting::anim_palette_light(
                light_grid,
                Some(scenario),
                cell,
                config,
                false,
            )
        };
        // Native Z (`AnimClass__DrawIt @ 0x00422CA0`): an anim draw carries
        // 0x2800 with gradient entry 2 and `YDrawOffset + ZAdjust -
        // AdjustForZ - 2`; it tests Z per pixel and never writes.
        pieces.push(PlannedBuildingPieceInstance {
            kind: BuildingPieceKind::PoweredOrActiveOverlay,
            z_bias: z_adjust_px,
            policy: BlitPolicy::z_read(SpriteEncoding::Plain),
            target: GroundTexture::ShpPage(anim_entry.page as usize),
            instance: SpriteInstance {
                position: [ax, ay],
                size: anim_entry.pixel_size,
                uv_origin: anim_entry.uv_origin,
                uv_size: anim_entry.uv_size,
                depth: anim_depth,
                tint,
                palette_light,
                alpha: 1.0,
                draw_state,
                // The anim's YDrawOffset is baked into the atlas offset, so it
                // also rides the Z term (native `YDrawOffset + ZAdjust - 2`).
                z_adjust: ground_z_adjust(
                    z,
                    z_adjust_px
                        + art_reg
                            .anim_runtime_config(anim_name)
                            .map_or(0, |c| c.y_draw_offset),
                ),
                z_gradient: pack_z_gradient(ZGradient::Vertical, false),
                ..Default::default()
            },
        });
    }
}

#[cfg(test)]
fn resolve_infantry_shp_frame(
    state: &AppState,
    type_id: &str,
    entity: &crate::sim::game_entity::GameEntity,
) -> u16 {
    // Pass raw facing (not canonical) to resolve_shp_frame so the
    // facing-to-index division works correctly for any facing count
    // (6, 8, 10, etc.). The absolute frame index encodes the direction.
    let sequence_set = state
        .rules()
        .and_then(|rules| rules.animation_sequence(type_id));
    if entity.category == EntityCategory::Unit
        && !entity.is_voxel
        && entity.animation.as_ref().is_none_or(|anim_state| {
            matches!(
                anim_state.sequence,
                animation::SequenceKind::Stand | animation::SequenceKind::Walk
            )
        })
        && let Some(set) = sequence_set
        && let Some(frame) = animation::resolve_shp_vehicle_body_frame(
            set,
            entity.facing,
            entity.body_frame_counter,
            crate::sim::movement::ready_producer::is_moving_for_unit_shp_draw(entity),
        )
    {
        return frame;
    }
    if let (Some(anim_state), Some(set)) = (entity.animation.as_ref(), sequence_set) {
        if let Some(def) = set.get(&anim_state.sequence) {
            return animation::resolve_shp_frame(def, entity.facing, anim_state.frame_index);
        }
    }
    // Fallback when no sequence data was built for this type: the standing
    // block is frames 0..7, so the facing slot is the frame index. Uses the
    // same native facing table as the real path so the two cannot disagree.
    animation::infantry_facing_slot(entity.facing)
}

/// Completed `CanBeOccupied` body frame: native GetCurrentFrame 0x0043EF90.
/// Building+0x534 is the animation state, not damage: Guard selects state 1
/// (0x0044995D), while construction selects state 0. Build-up/down are handled
/// before this caller. Native caller evidence and Unicorn rerun commands live
/// in tools/garrison_oracle/body_frame.py; its body_frame.json native outputs
/// are checked by completed_garrison_body_frames_match_native_oracle below.
/// Civilian red-health occupied art collapses frame 3 to frame 1.
fn building_frame_index(
    occupant_count: u32,
    health_current: i32,
    strength: i32,
    tech_level: i32,
    condition_yellow: f64,
    condition_red: f64,
) -> u16 {
    crate::sim::building_art::occupied_body_frame(
        occupant_count,
        health_current,
        strength,
        tech_level,
        condition_yellow,
        condition_red,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_health_ratio_corpus_matches_requested_building_art_transition() {
        for row in crate::sim::health_ratio_fixture::rows() {
            assert_eq!(
                crate::sim::building_art::requested_damage_state(
                    crate::sim::components::Health {
                        current: row.input.current
                    },
                    row.input.strength,
                    row.input.yellow(),
                ),
                row.output.generic_art_damaged,
                "{row:?}"
            );
        }
    }
    use super::building_frame_index;
    use super::shp_body_tint;
    use crate::app::presentation::building_anim::building_anim_rate_logic_frames;
    use crate::map::entities::EntityCategory;
    use crate::map::lighting::CellLightGrid;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_options::GameOptions;

    /// Stock `[GAPOWR_A]`, the Allied power plant's looping smokestack.
    const GAPOWR_A_ART: &str = "[GAPOWR_A]\nNormalized=yes\nStart=0\nLoopStart=0\nLoopEnd=8\n\
                                LoopCount=-1\nRate=220\n";

    fn stock_game_options() -> GameOptions {
        GameOptions::default()
    }

    #[test]
    fn gsi_13_10_shp_selector_keeps_unit_and_infantry_extras_distinct() {
        let mut grid = CellLightGrid::new();
        grid.insert_profiled_light((4, 5), [1.0, 0.88, 0.88], 1.0);

        let unit = shp_body_tint(&grid, (4, 5), EntityCategory::Unit, 200, 300);
        let infantry = shp_body_tint(&grid, (4, 5), EntityCategory::Infantry, 200, 300);
        let structure = shp_body_tint(&grid, (4, 5), EntityCategory::Structure, 200, 300);
        for (actual, expected) in unit.into_iter().zip([1.2, 1.056, 1.056]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in infantry.into_iter().zip([1.3, 1.144, 1.144]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in structure.into_iter().zip([1.0, 0.88, 0.88]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        assert_ne!(
            unit, infantry,
            "Unit and Infantry extras must not cross-feed"
        );
    }

    #[test]
    fn looping_building_anim_rate_applies_normalized_game_speed_scaling() {
        // Rate=220 is a native frame delay of 900/220 = 4 logic frames, and
        // Normalized=yes rescales that through the match game speed on
        // construction. At the stock GameSpeed=1 the delay becomes 6.
        let art = ArtRegistry::from_ini(&IniFile::from_str(GAPOWR_A_ART));
        let options = stock_game_options();
        assert_eq!(options.game_speed, 1);

        assert_eq!(
            building_anim_rate_logic_frames(&art, "GAPOWR_A", Some(&options)),
            6
        );
        // Without the Normalized= step the raw 900/Rate delay stands.
        assert_eq!(building_anim_rate_logic_frames(&art, "GAPOWR_A", None), 4);
    }

    #[test]
    fn looping_building_anim_rate_of_unnormalized_section_is_not_rescaled() {
        // [NATSLA_B] is explicitly Normalized=no so its hard frame delay is kept.
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[NATSLA_B]\nNormalized=no\nStart=0\nEnd=9\nRate=300\n",
        ));
        assert_eq!(
            building_anim_rate_logic_frames(&art, "NATSLA_B", Some(&stock_game_options())),
            3
        );
    }

    #[test]
    fn looping_building_anim_damaged_variant_uses_its_own_section_rate() {
        // Stock `[GARADR]`: the damaged dish replacement carries Rate=180 where
        // the healthy one carries Rate=220, so the delay has to be resolved from
        // whichever variant was selected, not from the base slot.
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[GARADR_A]\nNormalized=yes\nLoopStart=0\nLoopEnd=14\nLoopCount=-1\nRate=220\n\
             PingPong=yes\n\
             [GARADR_AD]\nImage=GARADR_A\nNormalized=yes\nLoopStart=15\nLoopEnd=29\n\
             LoopCount=-1\nRate=180\nPingPong=yes\n",
        ));
        let options = stock_game_options();

        // 900/220 = 4 → normalized 6; 900/180 = 5 → (5*8)/(1+1) = 20.
        assert_eq!(
            building_anim_rate_logic_frames(&art, "GARADR_A", Some(&options)),
            6
        );
        assert_eq!(
            building_anim_rate_logic_frames(&art, "GARADR_AD", Some(&options)),
            20
        );
    }

    #[test]
    fn looping_building_anim_rate_falls_back_to_native_default_without_a_section() {
        let art = ArtRegistry::empty();
        assert_eq!(
            building_anim_rate_logic_frames(&art, "NAOBEL_A", Some(&stock_game_options())),
            crate::rules::art_data::DEFAULT_ART_RATE_LOGIC_FRAMES
        );
    }

    // Civilian (TechLevel == -1) — matches CABHUT, CALA01, CAGAS01, CABUNK01, etc.
    // Yellow-tier damage step is gated on TechLevel > 0, so it never fires here.
    // Frame 3 collapses to 1 (occupied + red).

    #[test]
    fn civilian_empty_healthy_returns_0() {
        assert_eq!(building_frame_index(0, 100, 100, -1, 0.5, 0.25), 0);
    }

    #[test]
    fn civilian_empty_yellow_tier_returns_0() {
        // ratio = 0.4: below ConditionYellow but above ConditionRed.
        // Yellow gate is `tech_level > 0` — fails for civilian, so no +1.
        assert_eq!(building_frame_index(0, 40, 100, -1, 0.5, 0.25), 0);
    }

    #[test]
    fn civilian_empty_red_tier_returns_1() {
        assert_eq!(building_frame_index(0, 20, 100, -1, 0.5, 0.25), 1);
    }

    #[test]
    fn civilian_occupied_healthy_bstate_formula_returns_2() {
        assert_eq!(building_frame_index(1, 100, 100, -1, 0.5, 0.25), 2);
    }

    #[test]
    fn civilian_occupied_yellow_tier_returns_2() {
        // Same yellow-gate behavior as empty case.
        assert_eq!(building_frame_index(1, 40, 100, -1, 0.5, 0.25), 2);
    }

    #[test]
    fn civilian_occupied_red_tier_collapses_to_1() {
        // base=2 (occupied) + 1 (red) = 3 → collapse rule → 1.
        assert_eq!(building_frame_index(1, 20, 100, -1, 0.5, 0.25), 1);
    }

    // Buildable (TechLevel >= 1) — TS-era "buildable garrisonable" structures
    // (none in standard YR but the formula path is real). Yellow tier fires.

    #[test]
    fn buildable_empty_healthy_returns_0() {
        assert_eq!(building_frame_index(0, 100, 100, 5, 0.5, 0.25), 0);
    }

    #[test]
    fn buildable_empty_yellow_tier_returns_1() {
        assert_eq!(building_frame_index(0, 40, 100, 5, 0.5, 0.25), 1);
    }

    #[test]
    fn buildable_occupied_healthy_returns_2() {
        assert_eq!(building_frame_index(1, 100, 100, 5, 0.5, 0.25), 2);
    }

    #[test]
    fn buildable_occupied_red_tier_returns_3() {
        // No civilian collapse (tech_level != -1).
        assert_eq!(building_frame_index(1, 20, 100, 5, 0.5, 0.25), 3);
    }

    // Edge cases.

    #[test]
    fn zero_over_zero_selects_damaged_body_frame() {
        // Native masked 0/0 is unordered and TEST AH,41 enters the damage arm.
        assert_eq!(building_frame_index(0, 0, 0, -1, 0.5, 0.25), 1);
    }

    #[test]
    fn original_health_ratio_corpus_matches_completed_occupied_body_frame() {
        for row in crate::sim::health_ratio_fixture::rows() {
            for expected in &row.output.occupied_body_frames {
                assert_eq!(
                    building_frame_index(
                        expected.occupants,
                        row.input.current,
                        row.input.strength,
                        expected.tech_level,
                        row.input.yellow(),
                        row.input.red()
                    ),
                    expected.frame,
                    "{row:?}, {expected:?}"
                );
            }
        }
    }

    #[test]
    fn building_frame_keeps_signed_health_and_live_strength_width() {
        assert_eq!(building_frame_index(0, 70_000, 100_000, 5, 0.5, 0.25), 0);
        assert_eq!(building_frame_index(0, 70_000, 200_000, 5, 0.5, 0.25), 1);
        assert_eq!(building_frame_index(0, -1, 100_000, 5, 0.5, 0.25), 1);
    }

    #[test]
    fn boundary_at_condition_red_inclusive() {
        // ratio == ConditionRed exactly → red_tier fires (<=).
        assert_eq!(building_frame_index(0, 25, 100, -1, 0.5, 0.25), 1);
    }

    #[test]
    fn completed_garrison_body_frames_match_native_oracle() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tools/garrison_oracle/body_frame.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 54);
        for case in cases {
            let n = |key: &str| case[key].as_i64().unwrap();
            assert_eq!(
                building_frame_index(
                    n("occupants") as u32,
                    n("health") as i32,
                    2000,
                    n("tech_level") as i32,
                    0.5,
                    0.25,
                ),
                n("frame") as u16,
                "{case}"
            );
        }
    }
}
