//! SHP entity instance builders — per-frame SpriteInstance generation for buildings and infantry.
//!
//! Handles building animation overlays (Active/Idle/Special), bibs, build-up
//! animations, and infantry sprite frame resolution.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use super::helpers::{
    ANIM_DRAW_DEPTH_BIAS_PX, apply_shape_z_adjust, compute_sprite_depth,
    compute_sprite_depth_params_lifted, depth_axis, effective_anim_z_adjust, entity_draw_band,
    ground_sort_row, in_view, lifted_z_adjust, tactical_entity_admission,
};
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedBuildingPieceInstance,
    PlannedObjectInstance,
};
use crate::map::entities::EntityCategory;
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::native_z::{
    self, BIB_Z_ADJUST_PX, SHP_DRAW_Z_ADJUST_PX, ZGradient, ZSHAPE_MAX_FOUNDATION_WIDTH,
    pack_z_gradient,
};
use crate::render::sprite_atlas::ShpSpriteKey;
use crate::render::tactical_draw_plan::{BlitPolicy, BuildingPieceKind, SpriteEncoding};
use crate::render::unit_atlas::{UnitSpriteKey, VxlLayer, canonical_turret_facing};
use crate::rules::house_colors::HouseColorIndex;
use crate::sim::animation;

/// Sort keys of the bodies that currently own a parachute canopy, by entity id.
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

/// `extra_light` is the class's
/// [`crate::app::presentation::lighting::body_extra_light`].
///
/// RESIDUAL: an aircraft type without a voxel draws no body in gamemd.
/// AircraftClass::Draw_It branches (`0x00414673`, `0x00414681`) past its
/// light block to `0x004149E5`, whose call `0x004DB250` is a bare `RET 8`;
/// VERA draws its SHP body here, lit as a voxel aircraft. Trigger: an
/// AircraftType whose art lacks `Voxel=yes`. Frequency: none in retail data;
/// the one such type, APACHE, has no art section, and VERA sends a type
/// without art to the voxel draw (`world_spawn.rs` `object_uses_voxel`).
/// Effect: the aircraft is visible. Downstream: presentation only.
fn shp_body_tint(
    grid: &crate::map::lighting::CellLightGrid,
    cell: (u16, u16),
    category: EntityCategory,
    extra_light: i32,
) -> [f32; 3] {
    match category {
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft => {
            grid.body_tint_at(cell, extra_light)
        }
        EntityCategory::Structure => grid.building_body_tint_at(cell),
    }
}

/// Iterate visible SHP sprite entities from EntityStore and build SpriteInstances.
///
/// Build SpriteInstances for all SHP entities (buildings, infantry).
/// Ground bodies, building bibs/anims, and building turret VXLs are emitted as
/// one parent-owned group so the native global order cannot split their display
/// call at an atlas boundary.
/// Air and Top bodies retain the same parent ownership in their own Display layers.
/// `parachute_body_depths` collects the sort key of every body currently under
/// a parachute, keyed by entity — see [`ParachuteBodyDepths`].
/// Building bodies write their own per-pixel Z in the Ground pass, which is
/// what the post-shroud selection-bracket redraw tests against; no separate
/// depth stamp exists any more.
pub(crate) fn build_shp_instances(
    state: &AppState,
    parachute_body_depths: &mut ParachuteBodyDepths,
    ground_objects: &mut Vec<PlannedObjectInstance>,
    ground_order: &NativeDisplayOrder,
) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let atlas = state.match_state.match_presentation.sprite_atlas.as_ref();
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
        state.rules().map(|rules| rules.art());
    let terrain = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|rt| rt.view().resolved_terrain());
    let canopy_owners = super::overlays::parachute_canopy_owners(state, sim);

    // Drawing borrows live map fields but uses its own small Dummy identity,
    // so native-style misses cannot mutate lockstep simulation state.
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    // `0x00487950` on the local player's map: a sandbox view, or a viewer
    // with no house, sees no shroud.
    let shrouded = |cell: (u16, u16)| {
        !ignore_visibility
            && local_owner_id
                .zip(cells.as_ref())
                .is_some_and(|(viewer, cells)| {
                    crate::sim::vision::cell_is_shrouded(&sim.fog, cells, viewer, cell)
                })
    };
    let encounter_order = super::helpers::tactical_entity_encounter_order(sim);
    for stable_id in encounter_order {
        let Some(entity) = sim.entities().get(stable_id) else {
            continue;
        };
        let Some(_band) = entity_draw_band(sim.display_layers(), stable_id) else {
            continue;
        };
        // Common visibility, passenger, limbo, and DrawState admission is shared below.
        let owner_str = sim.interner.resolve(entity.owner());
        let mut observer = super::helpers::observer_draw_context(
            sim,
            entity,
            local_owner.as_deref(),
            local_owner_id,
            state.rules(),
        );
        let active_disguise = entity.disguise.as_ref().filter(|disguise| {
            disguise.is_disguised()
                && (entity.category != EntityCategory::Unit
                    || DrawState::draws_disguise(entity, sim.session.binary_frame, observer))
        });
        let type_name = if entity.category == EntityCategory::Unit {
            if entity.unit_deploying() {
                continue;
            }
            super::units::drawn_model_id(
                entity,
                &sim.interner,
                state.rules(),
                sim.session.binary_frame,
                observer,
            )
        } else {
            std::borrow::Cow::Borrowed(
                active_disguise
                    .and_then(|state| state.type_id())
                    .map(|id| sim.interner.resolve(id))
                    .unwrap_or_else(|| sim.interner.resolve(entity.type_ref())),
            )
        };
        let type_str = type_name.as_ref();
        if super::helpers::drawn_type_uses_voxel(entity, type_str, &sim.interner, state.rules()) {
            continue;
        }
        observer.drawn_voxel = Some(false);
        let remap_owner = active_disguise
            .and_then(|state| state.house())
            .map(|id| sim.interner.resolve(id))
            .unwrap_or(owner_str);
        // Wall buildings render as overlays (auto-tiled connectivity frames).
        // Their fixed-cell overlay pass owns the connected wall frame.
        // Skip them here to avoid drawing frame0 (an isolated pillar).
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
        let Some(draw_decision) = tactical_entity_admission(
            super::helpers::TacticalEntityPurpose::Drawing,
            entity,
            owner_str,
            local_owner.as_deref(),
            local_owner_id,
            &sim.fog,
            ignore_visibility,
            sim.session.binary_frame,
            super::units::house_color_to_remap_row(hc),
            observer,
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
        // The lift `screen_position` drew this body with. A building's draw
        // cancels exactly that for its sort row and per-pixel Z seed
        // (`NormalZAdjust - AdjustForZ(Z)`), so its body, bib, anims and
        // turret sort and clip against the height they are drawn at.
        let lift_px = crate::render::locomotor_visual::screen_lift_px(entity);
        if !in_view(sx, sy, 200.0, 200.0, cam_x, cam_y, sw, sh, 200.0) {
            continue;
        }
        let draw_state = draw_decision.state;
        if entity.category == EntityCategory::Unit
            && state.rules().is_some_and(|rules| {
                rules
                    .terrain_object_type_case_insensitive(type_str)
                    .is_some()
            })
        {
            if let Some(frame) = resolve_object_shp_frame(
                state,
                sim.interner.resolve(entity.type_ref()),
                entity,
                sim.session.binary_frame,
                cells.as_ref(),
            ) {
                emit_unit_terrain_body(
                    state,
                    entity,
                    type_str,
                    frame,
                    [sx, sy],
                    draw_state,
                    cells.as_ref(),
                    ground_order,
                    ground_objects,
                );
            }
            continue;
        }
        let Some(atlas) = atlas else { continue };
        // Determine if this building is in its make/build-up or build-down animation.
        // Only BState 0 draws the construction animation: a human player's
        // placement shows its idle body for one frame before its mission
        // starts, and a pack-up until Sell's stage 1 starts the animation
        // again (`Begin_Mode(0)`).
        let is_building_up: bool = entity.category == EntityCategory::Structure
            && !entity.building_down()
            && entity.in_construction_bstate();
        let is_building_down: bool = entity.category == EntityCategory::Structure
            && entity.building_down()
            && entity.in_construction_bstate();
        // The construction animation's stage is the Buildup frame drawn,
        // reversed while selling (`BuildingClass::GetCurrentFrame`'s BState 0
        // arm, `sim::building_art::body_frame`).
        let make_frame = |selling: bool| {
            let make_key: String = format!("{}_MAKE", type_str);
            let total_make_frames: u16 =
                atlas.make_frame_counts.get(&make_key).copied().unwrap_or(0);
            if total_make_frames == 0 {
                return (0, None);
            }
            let control = entity.building_construction_control();
            let stage = crate::sim::building_art::body_frame(
                crate::sim::building_art::BodyFrameInput {
                    state: 0,
                    base_frame: entity.construction_stage_value(),
                    selling,
                    buildup: [control[0], control[1]],
                    laser_frame: None,
                    firestorm_frame: None,
                    gate_stages: None,
                    occupants: None,
                    tech_level: 0,
                    ordinary: [[0, 1]; 4],
                },
                entity.health,
                1,
                0.5,
                0.25,
            );
            let frame = stage.clamp(0, i32::from(total_make_frames) - 1) as u16;
            (frame, Some(make_key))
        };
        let (shp_frame, make_type_id): (u16, Option<String>) = if is_building_up {
            make_frame(false)
        } else if is_building_down {
            make_frame(true)
        } else {
            match entity.category {
                EntityCategory::Structure => {
                    let obj = state.rules().and_then(|r| r.object(type_str));
                    let frame = if let Some(obj) = obj {
                        let occupant_count = entity
                            .passenger_role
                            .cargo()
                            .map(|c| c.count())
                            .unwrap_or(0);
                        let (cy, cr) = state
                            .rules()
                            .map(|r| (r.general.condition_yellow, r.general.condition_red))
                            .unwrap_or((0.5, 0.25));
                        let art = art_reg.and_then(|registry| {
                            registry.resolve_metadata_entry(type_str, &obj.image)
                        });
                        // The completed body shares GetCurrentFrame43EF90 with
                        // receiver health comparisons. In particular Idle's
                        // damaged frame is base+1, including the first frame
                        // after Construction; it is not always body frame0.
                        let frame = crate::sim::building_art::body_frame(
                            crate::sim::building_art::BodyFrameInput {
                                state: entity.building_body_state().unwrap_or(1),
                                base_frame: entity.construction_stage_value(),
                                laser_frame: obj.laser_fence.then_some(0),
                                firestorm_frame: obj.firestorm_wall.then_some(0),
                                gate_stages: obj
                                    .gate
                                    .then_some(art.map_or(9, |entry| entry.building_gate_stages)),
                                occupants: obj.can_be_occupied.then_some(occupant_count as i32),
                                tech_level: obj.tech_level,
                                selling: entity.building_down(),
                                buildup: [0, 1],
                                ordinary: art.map_or([[0, 1]; 4], |entry| {
                                    entry
                                        .building_body_ranges
                                        .map(|[start, count, _]| [start, count])
                                }),
                            },
                            entity.health,
                            obj.strength,
                            cy,
                            cr,
                        );
                        u16::try_from(frame).unwrap_or(0)
                    } else {
                        0
                    };
                    (frame, None)
                }
                _ => {
                    let Some(frame) = resolve_object_shp_frame(
                        state,
                        type_str,
                        entity,
                        sim.session.binary_frame,
                        cells.as_ref(),
                    ) else {
                        continue;
                    };
                    (frame, None)
                }
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
            Some(e) => Some(e),
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
                    Some(e) => Some(e),
                    None => continue,
                }
            }
            // An empty (0x0) body frame has no atlas entry: `NAMISL`'s six
            // frames are all empty and its SuperAnims are the silo. The body
            // draw paints nothing and the building's bib, anims and turret
            // still draw after it.
            None if entity.category == EntityCategory::Structure => None,
            None => continue,
        };

        let base_depth: f32 = match entity.category {
            EntityCategory::Structure => {
                // `sy` already carries the render-coordinate lift, so it *is* the
                // NW footprint cell's tile row — the row gamemd's YSort (X + Y
                // off the render coords) reduces to. A building therefore sorts
                // on its own cell rather than one iso row north of it.
                let axis = depth_axis(state);
                compute_sprite_depth_params_lifted(axis.origin_y, axis.world_height, sy, lift_px)
            }
            _ => {
                // The drawn row carries this body's height lift; the sort key
                // must not. A hovering Rocketeer or a descending paradrop key
                // off the cell it is over, exactly like a GI standing there.
                // Only a Structure reaches here without a body entry.
                let canvas = entry.map_or([0.0; 4], |entry| entry.canvas_rect);
                let depth_y: f32 = sy + canvas[1] + canvas[3];
                compute_sprite_depth(state, ground_sort_row(entity, depth_y), interp_z)
            }
        };
        let depth: f32 = base_depth;
        // Keyed on the canopy, not on the fall: natively the canopy outlives
        // the landing while it plays out, still attached to the landed body.
        if canopy_owners.contains(&entity.stable_id()) {
            parachute_body_depths.insert(entity.stable_id(), depth);
        }
        let extra_light = state.rules().map_or(0, |rules| {
            crate::app::presentation::lighting::body_extra_light(entity, rules, sim, terrain)
        });
        let tint = shp_body_tint(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx, pos.ry),
            entity.category,
            extra_light,
        );
        let selected_palette_light = crate::app::presentation::lighting::body_palette_light(
            state.match_state.match_presentation.lighting.grid(),
            &sim.session.lighting,
            (pos.rx, pos.ry),
            extra_light,
        );
        let palette_light = if entity.category == EntityCategory::Structure {
            let obj = state.rules().and_then(|r| r.object(type_str));
            let image = obj.map_or(type_str, |o| o.image.as_str());
            let art = state
                .rules()
                .and_then(|r| r.art().resolve_metadata_entry(type_str, image));
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
        // TechnoClass::DrawSHP's curtain arm (`0x0070631F..0x00706389`) on the
        // body, bib and buildup, and the colour word DrawBody hands their
        // blits (`0x0043D386..0x0043D544`), tested at the building's
        // coordinate's cell. SHP vehicles' draws pass no colour word
        // (`0x0073CE3E`, `0x0073CE7D`).
        let (body_tint, palette_light) =
            crate::app::presentation::lighting::curtain_light(entity, tint, palette_light, sim);
        let colour_word = match state.rules() {
            Some(rules) if entity.category == EntityCategory::Structure => {
                let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
                crate::app::presentation::lighting::building_colour_word(entity, sim, rules, || {
                    shrouded(((x / 256) as u16, (y / 256) as u16))
                })
            }
            _ => 0,
        };
        // Direct SHP and infantry keep native Ground parent order. Infantry
        // does not use the Unit composite bridge split at 0x73B140.
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
                lifted_z_adjust(lift_px, normal_z_adjust + SHP_DRAW_Z_ADJUST_PX),
                native_z::pack_building_z_gradient(
                    zshape,
                    entry.is_some_and(|entry| entry.extended),
                ),
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
        // DrawBody's pieces blit with flags 0x6E00.
        let body_policy = BlitPolicy::opaque(SpriteEncoding::Plain);
        let body = entry.map(|entry| {
            let instance = SpriteInstance {
                position: [sx + entry.offset_x, sy + entry.offset_y],
                size: entry.pixel_size,
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                source_palette: entry.source_palette,
                depth,
                tint: body_tint,
                palette_light: palette_light.with_colour_word(
                    if body_policy.ors_colour_word(entry.extended) {
                        colour_word
                    } else {
                        0
                    },
                ),
                alpha: 1.0,
                draw_state,
                z_adjust,
                z_gradient,
                zshape_origin,
            };
            (ObjectTexture::ShpPage(entry.page as usize), instance)
        });

        let mut building_pieces = Vec::new();
        if entity.category == EntityCategory::Structure {
            if let Some((target, instance)) = body {
                building_pieces.push(PlannedBuildingPieceInstance {
                    kind: if is_building_up || is_building_down {
                        BuildingPieceKind::BuildupOrSpecial
                    } else {
                        BuildingPieceKind::Body
                    },
                    z_bias: 0,
                    // Body and buildup go through the same Z-writing body draw.
                    policy: body_policy,
                    target,
                    instance,
                });
            }
        } else if let Some((target, instance)) = body
            && let Some(parent) =
                ground_order.object_draw(entity.stable_id(), SpriteEncoding::Plain)
        {
            ground_objects.push(PlannedObjectInstance::object(
                parent,
                vec![ObjectPieceInstance {
                    target,
                    render_z: parent.policy.render_z,
                    instance,
                }],
            ));
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
                    lift_px,
                    depth,
                    body_tint,
                    palette_light,
                    colour_word,
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
                    entity,
                    &|anim| anim_colour_word(state, sim, cells.as_ref(), &shrouded, anim),
                    world_height,
                    draw_state,
                    lift_px,
                );
            }
            // Building voxel guns remain inside their SHP hull's draw call.
            if let Some(rules_obj) = state.rules().and_then(|r| r.object(type_str)) {
                if rules_obj.turret_anim_is_voxel
                    && let Some(unit_atlas) = &state.match_state.match_presentation.unit_atlas
                {
                    let turret_offset = art_reg
                        .and_then(|art| art.resolve_metadata_entry(type_str, &rules_obj.image))
                        .map_or(0, |art| art.turret_offset);
                    // TechnoClass::Draw's curtain arm (`0x0070678D..0x007067E2`)
                    // and BuildingClass::Draw's colour word (`0x0043DC1C..
                    // 0x0043DDF1`, DrawBody's) on the voxel cache blit.
                    let (turret_tint, turret_light) =
                        crate::app::presentation::lighting::curtain_light(
                            entity,
                            tint,
                            selected_palette_light,
                            sim,
                        );
                    emit_building_turret_vxl(
                        unit_atlas,
                        &mut state
                            .match_state
                            .match_presentation
                            .barrel_image_pitches
                            .borrow_mut(),
                        type_str,
                        entity.body_facing_current(sim.session.binary_frame),
                        entity.barrel_elevation().current(sim.session.binary_frame),
                        entity.turret_anim_frame,
                        entity.voxel_recoil(),
                        turret_offset,
                        sx,
                        sy,
                        lift_px,
                        depth,
                        turret_tint,
                        // Building VXL43DA80 reads top directly;707194 selects
                        // the scheme. SHP TerrainPalette/ExtraLight don't apply.
                        turret_light,
                        colour_word,
                        draw_state,
                        rules_obj.turret_anim_x,
                        rules_obj.turret_anim_y,
                        &mut building_pieces,
                    );
                }
            }
        }

        if entity.category == EntityCategory::Structure {
            if let Some(parent) =
                ground_order.object_draw(entity.stable_id(), SpriteEncoding::Plain)
            {
                ground_objects.push(PlannedObjectInstance::building(parent, building_pieces));
            }
        }
    }
}

/// Emit a building's native B8 turret and C0 barrel at its TurretAnim anchor.
///
/// Building draw43DA80 uses the current primary facing and retained +148 HVA
/// counter; C0 pitches by +370 and stays on frame0 when a B8 turret exists.
/// The same cache key owns both parts, except during either part's recoil.
/// Executed controls: tools/voxel_oracle/building_barrel.json.
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
    unit_atlas: &crate::render::unit_atlas::UnitAtlas,
    barrel_image_pitches: &mut crate::render::unit_atlas::BarrelImagePitches,
    type_id: &str,
    turret_facing: u16,
    barrel_elevation: u16,
    frame_counter: i32,
    (recoil, recoil_active): ([f32; 2], bool),
    turret_offset: i32,
    building_sx: f32,
    building_sy: f32,
    lift_px: i32,
    building_depth: f32,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    colour_word: u16,
    draw_state: DrawState,
    anim_x: i32,
    anim_y: i32,
    pieces: &mut Vec<PlannedBuildingPieceInstance>,
) {
    let frame_count = |layer| {
        unit_atlas
            .frame_counts
            .get(&(type_id.to_string(), layer, 0))
            .copied()
    };
    let turret_frames = frame_count(VxlLayer::Turret);
    let barrel_frames = frame_count(VxlLayer::Barrel);
    let has_turret = turret_frames.is_some();
    let frame =
        |count: Option<u32>| count.map_or(0, |count| (frame_counter % count.max(1) as i32) as u32);
    let turret_key = UnitSpriteKey {
        type_id: type_id.to_string(),
        turret_index: 0,
        facing: canonical_turret_facing(turret_facing),
        layer: VxlLayer::Turret,
        frame: frame(turret_frames),
        slope_type: 0, // building turrets don't tilt on slopes
        barrel_pitch: 0,
    };
    let mut barrel_key = UnitSpriteKey {
        layer: VxlLayer::Barrel,
        frame: if has_turret { 0 } else { frame(barrel_frames) },
        ..turret_key.clone()
    };
    let pitch = crate::render::vxl_raster::voxel_facing_step_u16(barrel_elevation) as i8 - 8;
    barrel_key.barrel_pitch = if has_turret && recoil_active {
        pitch
    } else {
        let mut cache_key = if has_turret { &turret_key } else { &barrel_key }.clone();
        cache_key.frame &= 0xff;
        barrel_image_pitches.pitch(&cache_key, None, pitch)
    };
    let offsets = if has_turret {
        crate::render::vxl_raster::building_gun_screen_offsets(
            turret_offset,
            turret_key.facing,
            barrel_key.barrel_pitch,
            recoil,
        )
    } else {
        [[0.0; 2]; 2]
    };
    for (key, [ox, oy]) in super::units::native_turret_barrel_order(
        turret_facing,
        (&turret_key, offsets[0]),
        (&barrel_key, offsets[1]),
    ) {
        let Some(entry) = unit_atlas.get(key) else {
            continue;
        };
        // TurretAnimX/Y are screen-pixel offsets at the building draw point.
        // The existing +3 raster-origin adjustment remains a small alignment
        // residual; the missing-barrel comparison does not establish it.
        let tx = building_sx + anim_x as f32 + entry.offset_x + ox;
        let ty = building_sy + anim_y as f32 + entry.offset_y + 3.0 + oy;
        // Building43DA80 -> Techno Draw0x2800: test Z, never write it.
        let policy = BlitPolicy::z_read(SpriteEncoding::Voxel);
        pieces.push(PlannedBuildingPieceInstance {
            kind: BuildingPieceKind::PoweredOrActiveOverlay,
            z_bias: 0,
            policy,
            target: ObjectTexture::UnitAtlasPage(entry.page),
            instance: SpriteInstance {
                position: [tx, ty],
                size: entry.pixel_size,
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                depth: building_depth,
                tint,
                // A voxel frame is a compressed one.
                palette_light: palette_light.with_colour_word(if policy.ors_colour_word(true) {
                    colour_word
                } else {
                    0
                }),
                alpha: 1.0,
                draw_state,
                // VXL blit: gradient entry2, lift cancelled, no DrawSHP -2.
                z_adjust: lifted_z_adjust(lift_px, 0),
                z_gradient: pack_z_gradient(ZGradient::Vertical, false),
                ..Default::default()
            },
        });
    }
}

#[cfg(test)]
#[path = "building_voxel_tests.rs"]
mod building_voxel_tests;

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
    lift_px: i32,
    building_depth: f32,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    colour_word: u16,
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
    // z-shape; it tests and writes Z like the body, and takes its colour word.
    let policy = BlitPolicy::opaque(SpriteEncoding::Plain);
    pieces.push(PlannedBuildingPieceInstance {
        kind: BuildingPieceKind::Bib,
        z_bias: 0,
        policy,
        target: ObjectTexture::ShpPage(bib_entry.page as usize),
        instance: SpriteInstance {
            position: [bx, by],
            size: bib_entry.pixel_size,
            uv_origin: bib_entry.uv_origin,
            uv_size: bib_entry.uv_size,
            source_palette: bib_entry.source_palette,
            depth: building_depth,
            tint,
            palette_light: palette_light.with_colour_word(
                if policy.ors_colour_word(bib_entry.extended) {
                    colour_word
                } else {
                    0
                },
            ),
            alpha: 1.0,
            draw_state,
            z_adjust: lifted_z_adjust(lift_px, BIB_Z_ADJUST_PX + SHP_DRAW_Z_ADJUST_PX),
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
    building: &crate::sim::game_entity::GameEntity,
    anim_word: &dyn Fn(u64) -> u16,
    world_height: f32,
    draw_state: DrawState,
    lift_px: i32,
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
        let Some(anim_id) = building.building_anim_slots[usize::from(anim.native_slot)] else {
            continue;
        };
        let Some(instance) = sim.anim(anim_id) else {
            continue;
        };
        // Building's retained slot reaches DrawIt422CA0..4238AF until physical
        // release. That body gates on hidden19D, not19B or Logic membership.
        if instance.draw_runtime.hidden {
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
        // only when ShouldUseCellDrawer, at the light UpdateAnimation keeps
        // armed (`lighting::building_anim_light`).
        let (anim_tint, palette_light) = if config.is_none_or(|c| c.should_use_cell_drawer) {
            if config.is_some_and(|c| c.use_normal_light) {
                (tint, attached_palette.with_brightness(1000))
            } else {
                crate::app::presentation::lighting::building_anim_light(
                    building,
                    tint,
                    attached_palette,
                    sim,
                )
            }
        } else {
            (
                tint,
                crate::app::presentation::lighting::anim_palette_light(
                    light_grid,
                    Some(scenario),
                    cell,
                    config,
                    false,
                ),
            )
        };
        // DrawIt blits with flags 0x2800 (`z_read`), which takes the word
        // from either frame encoding.
        let policy = BlitPolicy::z_read(SpriteEncoding::Plain);
        let palette_light =
            palette_light.with_colour_word(if policy.ors_colour_word(anim_entry.extended) {
                anim_word(anim_id)
            } else {
                0
            });
        // Native Z (`AnimClass__DrawIt @ 0x00422CA0`): an anim draw carries
        // 0x2800 with gradient entry 2 and `YDrawOffset + ZAdjust -
        // AdjustForZ - 2`; it tests Z per pixel and never writes.
        pieces.push(PlannedBuildingPieceInstance {
            kind: BuildingPieceKind::PoweredOrActiveOverlay,
            z_bias: z_adjust_px,
            policy,
            target: ObjectTexture::ShpPage(anim_entry.page as usize),
            instance: SpriteInstance {
                position: [ax, ay],
                size: anim_entry.pixel_size,
                uv_origin: anim_entry.uv_origin,
                uv_size: anim_entry.uv_size,
                source_palette: anim_entry.source_palette,
                depth: anim_depth,
                tint: anim_tint,
                palette_light,
                alpha: 1.0,
                draw_state,
                // The anim's YDrawOffset is baked into the atlas offset, so it
                // also rides the Z term (native `YDrawOffset + ZAdjust - 2`).
                z_adjust: lifted_z_adjust(
                    lift_px,
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

/// AnimClass::DrawIt's colour word for the anim `anim`
/// (`0x004233EE..0x00423630`): a building slot anim (its `+0x118`, set by
/// CreateAnimForSlot at `0x0045199B`) takes
/// [`crate::app::presentation::lighting::building_colour_word`] of the first
/// building among the ground objects of the cell under its coordinate
/// (`0x00565730`, `0x0047C520`; off the map, the Dummy, which holds none),
/// with that cell's shroud; any other anim none. The lookup is read, not
/// executed: the oracle's rows stub the cell.
fn anim_colour_word(
    state: &AppState,
    sim: &crate::sim::world::Simulation,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    shrouded: &impl Fn((u16, u16)) -> bool,
    anim: u64,
) -> u16 {
    let (Some(rules), Some(cells), Some(coord)) =
        (state.rules(), cells, sim.anim_absolute_coord(anim))
    else {
        return 0;
    };
    if !sim
        .anim(anim)
        .is_some_and(|object| object.is_building_anim())
    {
        return 0;
    }
    let cell = cells.lookup_world(coord.x, coord.y);
    if matches!(cell, crate::map::cell_index::NativeCellIdentity::Dummy) {
        return 0;
    }
    let (x, y) = cells.coord(cell);
    let cell = (x as u16, y as u16);
    sim.substrate
        .occupancy
        .first_building_on_layer(
            cell.0,
            cell.1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
        )
        .and_then(|id| sim.entities().get(id))
        .map_or(0, |building| {
            crate::app::presentation::lighting::building_colour_word(building, sim, rules, || {
                shrouded(cell)
            })
        })
}

/// Unit73C5F0 remains the body owner when its image is a TerrainType. Only
/// the source texture and Cell Convert differ: no Terrain object, placement,
/// sorting key, or Terrain71C1B0 draw constants are introduced.
#[allow(clippy::too_many_arguments)]
fn emit_unit_terrain_body(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    frame: u16,
    point: [f32; 2],
    draw_state: DrawState,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    order: &NativeDisplayOrder,
    output: &mut Vec<PlannedObjectInstance>,
) {
    use crate::render::terrain_draw::TerrainPiece;
    let Some(atlas) = state.match_state.match_presentation.overlay_atlas.as_ref() else {
        return;
    };
    let Ok(frame) = u8::try_from(frame) else {
        return;
    };
    let Some((body, shadow)) = atlas.native_terrain_pair(type_id, frame) else {
        return;
    };
    let Some(parent) = order.object_draw(entity.stable_id(), SpriteEncoding::Plain) else {
        return;
    };
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let sim = &runtime.simulation;
    let grid = state.match_state.match_presentation.lighting.grid();
    let Some(cells) = cells else { return };
    let mut physical = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    physical.z = crate::sim::movement::ground_pose::object_world_z_leptons(entity, None);
    let cell = cells.coord(crate::sim::movement::ground_pose::query_object_cell(
        cells, physical,
    ));
    let cell = (cell.0 as u16, cell.1 as u16);
    // Techno7060CC..70610A uses Cell+34 and the common/ground light+10C.
    let palette_light = crate::render::palette_light::PaletteLight::cell(grid, cell, false);
    let tint = grid.terrain_object_tint_for_type(cell, false);
    let (tint, palette_light) =
        crate::app::presentation::lighting::curtain_light(entity, tint, palette_light, sim);
    let depth = compute_sprite_depth(
        state,
        ground_sort_row(entity, point[1] + body.offset_y + body.pixel_size[1]),
        entity.position.z,
    );
    let bridge_fudge = super::foot_depth::shp_unit_bridge_fudge(state, entity);
    let body_z =
        super::foot_depth::shp_z_adjust(state, entity) - if bridge_fudge { 16.0 } else { 0.0 };
    let world_z = crate::sim::movement::ground_pose::object_world_z_leptons(entity, None);
    let shadow_z = crate::util::native_x87::adjust_for_z_standard(world_z)
        .wrapping_neg()
        .wrapping_sub(4) as f32;
    // Techno70600C..70603F suppresses the SHP stencil above ground. Borrow
    // the shared Object5F5F40/Map578080 owners with an isolated render query.
    let grounded = crate::sim::movement::ground_pose::query_ground_height(cells, physical)
        .is_ok_and(|ground| {
            crate::sim::movement::ground_pose::height_at_z(world_z, ground, entity.on_bridge) == 0
        });
    let mut pieces = Vec::with_capacity(2);
    for (piece, sprite, z_adjust, gradient, material) in [
        (
            TerrainPiece::Body,
            body,
            body_z,
            if bridge_fudge {
                ZGradient::Flat
            } else {
                ZGradient::Vertical
            },
            draw_state,
        ),
        // Techno706469..7064CF clears translucency on the second-half stencil.
        (
            TerrainPiece::Shadow,
            shadow,
            shadow_z,
            ZGradient::Flat,
            DrawState::default(),
        ),
    ] {
        if piece == TerrainPiece::Shadow
            && (!grounded
                || state
                    .rules()
                    .and_then(|rules| rules.object(sim.interner.resolve(entity.type_ref())))
                    .is_some_and(|object| object.no_shadow))
        {
            continue;
        }
        pieces.push(ObjectPieceInstance {
            target: ObjectTexture::TerrainShp(piece),
            render_z: parent.policy.render_z,
            instance: SpriteInstance {
                position: [point[0] + sprite.offset_x, point[1] + sprite.offset_y],
                size: sprite.pixel_size,
                uv_origin: sprite.uv_origin,
                uv_size: sprite.uv_size,
                depth,
                tint,
                palette_light: if piece == TerrainPiece::Shadow {
                    palette_light.with_brightness(1000)
                } else {
                    palette_light
                },
                alpha: 1.0,
                draw_state: material,
                z_adjust,
                z_gradient: pack_z_gradient(gradient, false),
                ..Default::default()
            },
        });
    }
    output.push(PlannedObjectInstance::object(parent, pieces));
}

pub(crate) fn resolve_object_shp_frame(
    state: &AppState,
    type_id: &str,
    entity: &crate::sim::game_entity::GameEntity,
    binary_frame: u32,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> Option<u16> {
    let facing = entity.body_facing_byte(binary_frame);
    // Pass raw facing (not canonical) to resolve_shp_frame so the
    // facing-to-index division works correctly for any facing count
    // (6, 8, 10, etc.). The absolute frame index encodes the direction.
    let type_id = if entity.category == EntityCategory::Unit {
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map_or(type_id, |runtime| {
                runtime.simulation.interner.resolve(entity.type_ref())
            })
    } else {
        type_id
    };
    let sequence_set = state
        .rules()
        .and_then(|rules| rules.animation_sequence(type_id));
    if entity.category == EntityCategory::Unit
        && entity.animation.as_ref().is_none_or(|anim_state| {
            matches!(
                anim_state.sequence,
                animation::SequenceKind::Stand | animation::SequenceKind::Walk
            )
        })
        && let Some(set) = sequence_set
    {
        // A retained signed/wide native layout can be outside the atlas frame
        // representation. Do not replace that failure with an infantry-facing
        // fallback, which would draw an unrelated valid frame.
        return animation::resolve_shp_vehicle_body_frame(
            set,
            facing,
            entity.body_frame_counter,
            // The draw asks the locomotor's Is_Moving (vt+0x10 at `0x0073C696`).
            crate::sim::movement::motion_query::is_moving(entity) == Some(true),
            entity
                .disguise
                .as_ref()
                .is_some_and(|disguise| disguise.is_disguised()),
        );
    }
    if let (Some((doing, stage)), Some(set)) = (entity.infantry_sprite_pose(), sequence_set) {
        // Original518D93..518DC8: Doing-1 draws Tread16 only when the
        // current CellClass+EC is Water2 and OnBridge is clear. This reads
        // live map land; it does not depend on the separate Infantry6E8 byte.
        let selected = if doing == -1 {
            let water = cells.is_some_and(|cells| {
                let cell = cells.lookup((entity.position.rx as i16, entity.position.ry as i16));
                cells.land_type(cell) == 2
            });
            if water && !entity.on_bridge { 16 } else { 0 }
        } else {
            doing
        };
        let record = set.infantry_action(selected)?;
        let facing = state
            .rules()
            .and_then(|rules| {
                let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
                rules.object(sim.interner.resolve(entity.type_ref()))
            })
            .filter(|object| object.jumpjet && !object.jumpjet_turn)
            .filter(|_| {
                entity.locomotor.as_ref().is_some_and(|loco| {
                    loco.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
                })
            })
            .and_then(|_| entity.attack_target.as_ref())
            .and_then(|target| {
                let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
                crate::sim::movement::turret::facing_toward_target(
                    entity,
                    &target.target,
                    sim.entities(),
                )
            })
            .map_or(facing, |direction| (direction >> 8) as u8);
        // Native EAX remains signed. A negative/wide result cannot alias an
        // unrelated valid frame in the u16 SHP atlas. This is an asset boundary,
        // not evidence for native invalid-index shape-access behavior.
        return u16::try_from(animation::resolve_shp_frame(record, facing, stage)).ok();
    }
    if let (Some(anim_state), Some(set)) = (entity.animation.as_ref(), sequence_set) {
        if let Some(def) = set.get(&anim_state.sequence) {
            return u16::try_from(animation::resolve_shp_frame(
                def,
                facing,
                i32::from(anim_state.frame_index),
            ))
            .ok();
        }
    }
    // Fallback when no sequence data was built for this type: the standing
    // block is frames 0..7, so the facing slot is the frame index. Uses the
    // same native facing table as the real path so the two cannot disagree.
    Some(animation::infantry_facing_slot(facing))
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
    use super::shp_body_tint;
    use crate::map::entities::EntityCategory;
    use crate::map::lighting::CellLightGrid;
    use crate::sim::building_art::occupied_body_frame;

    #[test]
    fn gsi_13_10_shp_selector_adds_the_class_extra_except_for_buildings() {
        let mut grid = CellLightGrid::new();
        grid.insert_profiled_light((4, 5), [1.0, 0.88, 0.88], 1.0);

        let unit = shp_body_tint(&grid, (4, 5), EntityCategory::Unit, 200);
        let infantry = shp_body_tint(&grid, (4, 5), EntityCategory::Infantry, 300);
        let aircraft = shp_body_tint(&grid, (4, 5), EntityCategory::Aircraft, 424);
        let structure = shp_body_tint(&grid, (4, 5), EntityCategory::Structure, 300);
        for (actual, expected) in unit.into_iter().zip([1.2, 1.056, 1.056]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in infantry.into_iter().zip([1.3, 1.144, 1.144]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in aircraft.into_iter().zip([1.424, 1.25312, 1.25312]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in structure.into_iter().zip([1.0, 0.88, 0.88]) {
            assert!((actual - expected).abs() < 0.0001);
        }
    }

    // Civilian (TechLevel == -1) — matches CABHUT, CALA01, CAGAS01, CABUNK01, etc.
    // Yellow-tier damage step is gated on TechLevel > 0, so it never fires here.
    // Frame 3 collapses to 1 (occupied + red).

    #[test]
    fn civilian_empty_healthy_returns_0() {
        assert_eq!(occupied_body_frame(0, 100, 100, -1, 0.5, 0.25), 0);
    }

    #[test]
    fn civilian_empty_yellow_tier_returns_0() {
        // ratio = 0.4: below ConditionYellow but above ConditionRed.
        // Yellow gate is `tech_level > 0` — fails for civilian, so no +1.
        assert_eq!(occupied_body_frame(0, 40, 100, -1, 0.5, 0.25), 0);
    }

    #[test]
    fn civilian_empty_red_tier_returns_1() {
        assert_eq!(occupied_body_frame(0, 20, 100, -1, 0.5, 0.25), 1);
    }

    #[test]
    fn civilian_occupied_healthy_bstate_formula_returns_2() {
        assert_eq!(occupied_body_frame(1, 100, 100, -1, 0.5, 0.25), 2);
    }

    #[test]
    fn civilian_occupied_yellow_tier_returns_2() {
        // Same yellow-gate behavior as empty case.
        assert_eq!(occupied_body_frame(1, 40, 100, -1, 0.5, 0.25), 2);
    }

    #[test]
    fn civilian_occupied_red_tier_collapses_to_1() {
        // base=2 (occupied) + 1 (red) = 3 → collapse rule → 1.
        assert_eq!(occupied_body_frame(1, 20, 100, -1, 0.5, 0.25), 1);
    }

    // Buildable (TechLevel >= 1) — TS-era "buildable garrisonable" structures
    // (none in standard YR but the formula path is real). Yellow tier fires.

    #[test]
    fn buildable_empty_healthy_returns_0() {
        assert_eq!(occupied_body_frame(0, 100, 100, 5, 0.5, 0.25), 0);
    }

    #[test]
    fn buildable_empty_yellow_tier_returns_1() {
        assert_eq!(occupied_body_frame(0, 40, 100, 5, 0.5, 0.25), 1);
    }

    #[test]
    fn buildable_occupied_healthy_returns_2() {
        assert_eq!(occupied_body_frame(1, 100, 100, 5, 0.5, 0.25), 2);
    }

    #[test]
    fn buildable_occupied_red_tier_returns_3() {
        // No civilian collapse (tech_level != -1).
        assert_eq!(occupied_body_frame(1, 20, 100, 5, 0.5, 0.25), 3);
    }

    // Edge cases.

    #[test]
    fn zero_over_zero_selects_damaged_body_frame() {
        // Native masked 0/0 is unordered and TEST AH,41 enters the damage arm.
        assert_eq!(occupied_body_frame(0, 0, 0, -1, 0.5, 0.25), 1);
    }

    #[test]
    fn original_health_ratio_corpus_matches_completed_occupied_body_frame() {
        for row in crate::sim::health_ratio_fixture::rows() {
            for expected in &row.output.occupied_body_frames {
                assert_eq!(
                    occupied_body_frame(
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
        assert_eq!(occupied_body_frame(0, 70_000, 100_000, 5, 0.5, 0.25), 0);
        assert_eq!(occupied_body_frame(0, 70_000, 200_000, 5, 0.5, 0.25), 1);
        assert_eq!(occupied_body_frame(0, -1, 100_000, 5, 0.5, 0.25), 1);
    }

    #[test]
    fn boundary_at_condition_red_inclusive() {
        // ratio == ConditionRed exactly → red_tier fires (<=).
        assert_eq!(occupied_body_frame(0, 25, 100, -1, 0.5, 0.25), 1);
    }

    #[test]
    fn completed_garrison_body_frames_match_native_oracle() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/garrison_oracle/body_frame.json",
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 54);
        for case in cases {
            let n = |key: &str| case[key].as_i64().unwrap();
            assert_eq!(
                occupied_body_frame(
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
