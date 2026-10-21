//! SHP entity instance builders — per-frame SpriteInstance generation for buildings and infantry.
//!
//! Handles independently ordered building animations, bibs, build-up
//! animations, and infantry sprite frame resolution.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use super::helpers::{
    ANIM_DRAW_DEPTH_BIAS_PX, apply_shape_z_adjust, compute_sprite_depth,
    compute_sprite_depth_params_lifted, depth_axis, entity_draw_band, ground_sort_row, in_view,
    lifted_z_adjust, tactical_entity_admission,
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
    self, SHP_DRAW_Z_ADJUST_PX, ZGradient, ZSHAPE_MAX_FOUNDATION_WIDTH, pack_z_gradient,
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
/// Ground bodies, building bibs, and building turret VXLs are emitted as
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
    let local_owner_id = local_owner.as_deref().and_then(|o| sim.interner().get(o));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let art_reg: Option<&crate::rules::art_data::ArtRegistry> =
        state.rules().map(|rules| rules.art());
    let canopy_owners = super::overlays::parachute_canopy_owners(state, sim);

    // Drawing borrows live map fields but uses its own small Dummy identity,
    // so native-style misses cannot mutate lockstep simulation state.
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    // `0x00487950` on the local player's map: a sandbox view, or a viewer
    // with no house, sees no shroud.
    let shrouded = |cell: (u16, u16)| {
        !ignore_visibility
            && local_owner_id
                .zip(cells.as_ref())
                .is_some_and(|(viewer, cells)| {
                    crate::sim::vision::cell_is_shrouded(sim.fog(), cells, viewer, cell)
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
        // Buildings are the one class gamemd draws off its own render-coordinate
        // virtual rather than the plain object coordinate; everything below this
        // point — body, bib, anims, turret, and the row they sort on — wants that
        // lifted anchor. See `locomotor_visual::BUILDING_ART_LIFT_PX`. The screen
        // cull reads only that anchor, and the admission work below only reads,
        // so an object off the screen skips it.
        // One fresh sample for this read-only entity draw. Reuse it for the
        // anchor, infantry direction and building gun instead of reading the
        // complete pose again for each field. Never retain it across draws.
        let render_pose = crate::app::presentation::interpolation::pose(
            state,
            entity,
            sim.session().binary_frame(),
        );
        let (sx, sy) = {
            let [x, y] = render_pose.screen;
            if entity.category() == EntityCategory::Structure {
                crate::render::locomotor_visual::building_art_anchor(x, y)
            } else {
                // Infantry DrawIt519524/51958A uses the signed Foot+6B4 lift.
                (x, y - render_pose.visual_lift)
            }
        };
        if !in_view(sx, sy, 200.0, 200.0, cam_x, cam_y, sw, sh, 200.0) {
            continue;
        }
        // Common visibility, passenger, limbo, and DrawState admission is shared below.
        let owner_str = sim.interner().resolve(entity.owner());
        let mut observer = super::helpers::observer_draw_context(
            sim,
            entity,
            local_owner.as_deref(),
            local_owner_id,
            state.rules(),
        );
        // Infantry's draw (0x00518DDF) and the shared remap (0x00705DAB) both
        // ask vt+0x440 (0x0070EE30) before taking the disguise.
        let active_disguise = entity.disguise.as_ref().filter(|disguise| {
            disguise.is_disguised()
                && DrawState::draws_disguise(entity, sim.session().binary_frame(), observer)
        });
        let type_name = if entity.category() == EntityCategory::Unit {
            if entity.unit_deploying() {
                continue;
            }
            super::units::drawn_model_id(
                entity,
                sim.interner(),
                state.rules(),
                sim.session().binary_frame(),
                observer,
            )
        } else {
            std::borrow::Cow::Borrowed(
                active_disguise
                    .and_then(|state| state.type_id())
                    .map(|id| sim.interner().resolve(id))
                    .unwrap_or_else(|| sim.interner().resolve(entity.type_ref())),
            )
        };
        let type_str = type_name.as_ref();
        if super::helpers::drawn_type_uses_voxel(entity, type_str, sim.interner(), state.rules()) {
            continue;
        }
        observer.drawn_voxel = Some(false);
        let remap_owner = active_disguise
            .and_then(|state| state.house())
            .map(|id| sim.interner().resolve(id))
            .unwrap_or(owner_str);
        // Wall buildings render as overlays (auto-tiled connectivity frames).
        // Their fixed-cell overlay pass owns the connected wall frame.
        // Skip them here to avoid drawing frame0 (an isolated pillar).
        if entity.category() == EntityCategory::Structure {
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
            sim.fog(),
            sim.house_alliances(),
            ignore_visibility,
            sim.session().binary_frame(),
            super::units::house_color_to_remap_row(hc),
            observer,
        ) else {
            continue;
        };
        let interp_z = pos.z();
        // The lift `screen_position` drew this body with. A building's draw
        // cancels exactly that for its sort row and per-pixel Z seed
        // (`NormalZAdjust - AdjustForZ(Z)`), so its body, bib, anims and
        // turret sort and clip against the height they are drawn at.
        let lift_px = crate::render::locomotor_visual::screen_lift_px(entity);
        let draw_state = draw_decision.state;
        if entity.category() == EntityCategory::Unit
            && state.rules().is_some_and(|rules| {
                rules
                    .terrain_object_type_case_insensitive(type_str)
                    .is_some()
            })
        {
            if let Some(frame) = resolve_object_shp_frame_with_facing(
                state,
                sim.interner().resolve(entity.type_ref()),
                entity,
                render_pose.body,
                cells.as_ref(),
            ) {
                emit_unit_terrain_body(
                    state,
                    entity,
                    type_str,
                    frame,
                    [sx, sy],
                    draw_decision,
                    cells.as_ref(),
                    ground_order,
                    ground_objects,
                );
            }
            continue;
        }
        let Some(atlas) = atlas else { continue };
        // Determine if this building is in its make/build-up or build-down animation.
        // Only BState 0 runs the construction animation; a pack-up shows
        // the completed body until Sell's stage 1 starts it again
        // (`Begin_Mode(0)`).
        let is_building_up: bool = entity.category() == EntityCategory::Structure
            && !entity.building_down()
            && entity.in_construction_bstate();
        let is_building_down: bool = entity.category() == EntityCategory::Structure
            && entity.building_down()
            && entity.in_construction_bstate();
        // The image DrawBody draws. A building not yet opened outside BState
        // 0 (a human player's placement, idle for a frame before its
        // Construction mission begins) draws its Buildup at the completed
        // body's frame; a null Buildup draws neither body nor bib.
        let image = (entity.category() == EntityCategory::Structure).then(|| {
            crate::sim::building_art::building_image(
                entity,
                state
                    .rules()
                    .is_none_or(|rules| rules.has_buildup(type_str)),
            )
        });
        let null_image = image == Some(crate::sim::building_art::BuildingImage::Null);
        let draws_buildup =
            image.is_some_and(|image| image != crate::sim::building_art::BuildingImage::Body);
        let unopened_buildup = draws_buildup && !is_building_up && !is_building_down;
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
            match entity.category() {
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
                        // DrawBody's gate branch replaces GetCurrentFrame while
                        // the gate opens, stands open or closes.
                        let gate_frame = obj.gate.then(|| {
                            crate::sim::building_art::gate_open_body_frame(
                                entity,
                                art.map_or(9, |entry| entry.building_gate_stages),
                                obj.strength,
                                cy,
                                sim.session().binary_frame(),
                            )
                        });
                        // The completed body shares GetCurrentFrame43EF90 with
                        // receiver health comparisons. In particular Idle's
                        // damaged frame is base+1, including the first frame
                        // after Construction; it is not always body frame0.
                        let frame = gate_frame.flatten().unwrap_or_else(|| {
                            crate::sim::building_art::body_frame(
                                crate::sim::building_art::BodyFrameInput {
                                    state: entity.building_body_state().unwrap_or(1),
                                    base_frame: entity.construction_stage_value(),
                                    laser_frame: obj.laser_fence.then_some(0),
                                    firestorm_frame: obj.firestorm_wall.then_some(0),
                                    gate_stages: obj.gate.then_some(
                                        art.map_or(9, |entry| entry.building_gate_stages),
                                    ),
                                    occupants: obj.can_be_occupied.then_some(occupant_count as i32),
                                    tech_level: sim.object_tech_level(obj),
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
                            )
                        });
                        u16::try_from(frame).unwrap_or(0)
                    } else {
                        0
                    };
                    (
                        frame,
                        unopened_buildup.then(|| format!("{}_MAKE", type_str)),
                    )
                }
                _ => {
                    let Some(frame) = resolve_object_shp_frame_with_facing(
                        state,
                        type_str,
                        entity,
                        render_pose.body,
                        cells.as_ref(),
                    ) else {
                        continue;
                    };
                    (frame, None)
                }
            }
        };
        // Mission Unload's war-factory art ([`factory_unload_art`]).
        let unload_art = (entity.category() == EntityCategory::Structure
            && !is_building_up
            && !is_building_down)
            .then(|| state.rules().zip(art_reg))
            .flatten()
            .and_then(|(rules, art_reg)| {
                let object = rules.object(type_str)?;
                let art = art_reg.resolve_metadata_entry(type_str, &object.image)?;
                factory_unload_art(sim, rules, entity, art)
            });
        // A loaded `DeployingAnim=` replaces the body image and its
        // NormalZAdjust; its frame is GetCurrentFrame, at most half the
        // image's frame count (`0x0043D7A7..0x0043D7F0`).
        let door_body = unload_art.as_ref().and_then(|art| {
            let (name, z_adjust) = art.body?;
            let count = atlas.object_shp_frame_count(name)?;
            Some((name.to_ascii_uppercase(), count / 2, z_adjust, art.palette))
        });
        let key: ShpSpriteKey = match &door_body {
            Some((name, half, _, palette)) => ShpSpriteKey {
                palette_context: *palette,
                type_id: name.clone(),
                facing: 0,
                frame: shp_frame.min(*half),
                house_color: hc,
            },
            None => ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::Legacy,
                type_id: make_type_id.as_deref().unwrap_or(type_str).to_string(),
                facing: 0,
                frame: shp_frame,
                house_color: hc,
            },
        };
        // Fallback: a CanBeOccupied building requesting frame 1/2/3 may miss the
        // atlas if its SHP has fewer than 4 frames. Retry with frame 0 so the
        // building still draws (Approach A in the design doc). Non-garrisonable
        // misses keep their existing skip path.
        let (body_key, entry) = match atlas.get(&key) {
            _ if null_image => (key.clone(), None),
            Some(e) => (key.clone(), Some(e)),
            None if shp_frame != 0
                && entity.category() == EntityCategory::Structure
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
                    Some(e) => (fallback_key, Some(e)),
                    None => continue,
                }
            }
            // An empty (0x0) body frame has no atlas entry: `NAMISL`'s six
            // frames are all empty and its SuperAnims are the silo. The body
            // draw paints nothing and the building's bib, anims and turret
            // still draw after it.
            None if entity.category() == EntityCategory::Structure => (key.clone(), None),
            None => continue,
        };

        let base_depth: f32 = match entity.category() {
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
            crate::app::presentation::lighting::body_extra_light(entity, rules, sim, cells.as_ref())
        });
        let tint = shp_body_tint(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx(), pos.ry()),
            entity.category(),
            extra_light,
        );
        let selected_palette_light = crate::app::presentation::lighting::body_palette_light(
            state.match_state.match_presentation.lighting.grid(),
            (pos.rx(), pos.ry()),
            extra_light,
        );
        let palette_light = if entity.category() == EntityCategory::Structure {
            let obj = state.rules().and_then(|r| r.object(type_str));
            let image = obj.map_or(type_str, |o| o.image.as_str());
            let art = state
                .rules()
                .and_then(|r| r.art().resolve_metadata_entry(type_str, image));
            crate::app::presentation::lighting::building_palette_light(
                state.match_state.match_presentation.lighting.grid(),
                (pos.rx(), pos.ry()),
                art,
            )
        } else {
            selected_palette_light
        };
        // The shared DrawSHP curtain intensity and the class caller's colour
        // word have separate owners. Infantry tests the Location cell;
        // Building's body/bib/buildup test the object's centre cell.
        let (body_tint, palette_light) = crate::app::presentation::lighting::techno_effect_light(
            entity,
            tint,
            palette_light,
            sim,
        );
        let colour_word = match state.rules() {
            Some(rules) => {
                crate::app::presentation::lighting::shp_colour_word(entity, sim, rules, || {
                    let cell = if entity.category() == EntityCategory::Structure {
                        let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
                        ((x / 256) as u16, (y / 256) as u16)
                    } else {
                        (pos.rx(), pos.ry())
                    };
                    shrouded(cell)
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
        let (z_adjust, z_gradient, zshape_origin) = if entity.category()
            == EntityCategory::Structure
        {
            let object_type = state.rules().and_then(|r| r.object(type_str));
            let rules_image: String = object_type
                .map(|o| o.image.clone())
                .unwrap_or_else(|| type_str.to_string());
            let art_entry = art_reg.and_then(|a| a.resolve_metadata_entry(type_str, &rules_image));
            let normal_z_adjust: i32 = match &door_body {
                Some((_, _, z_adjust, _)) => *z_adjust,
                None => art_entry.map_or(0, |a| a.normal_z_adjust),
            };
            let point_move = building_zshape_point_move(entity, art_entry);
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
        let Some(parent) = ground_order.object_draw(stable_id, SpriteEncoding::Plain) else {
            continue;
        };
        // Building DrawBody's pieces use 0x6E00 (Z read/write), whose raw
        // leaf drops the word. Infantry518F90 uses 0x2E00 (Z read only),
        // whose raw and RLE leaves both OR it. The retained mobile parent
        // already owns that policy; use the same parent below for emission.
        let body_policy = if entity.category() == EntityCategory::Structure {
            BlitPolicy::opaque(SpriteEncoding::Plain)
        } else {
            parent.policy
        };
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
        if entity.category() == EntityCategory::Structure {
            if let Some((target, instance)) = body {
                building_pieces.push(PlannedBuildingPieceInstance {
                    kind: if draws_buildup {
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
                // DrawSHP's shadow arm (`0x00705E00`) follows whichever
                // image GetImage returned, the Buildup included.
                if let Some(body_entry) = entry
                    && let Some(shadow) = embedded_shp_shadow_piece(
                        atlas,
                        state.rules().and_then(|rules| rules.object(type_str)),
                        entity,
                        cells.as_ref(),
                        &body_key,
                        body_entry,
                        instance,
                        draw_decision,
                        body_policy,
                    )
                {
                    building_pieces.push(PlannedBuildingPieceInstance {
                        kind: BuildingPieceKind::Shadow,
                        z_bias: 0,
                        policy: body_policy,
                        target: shadow.target,
                        instance: shadow.instance,
                    });
                }
            }
        } else if let Some((target, instance)) = body {
            let shadow = (entity.category() == EntityCategory::Infantry)
                .then(|| infantry_air_shadow_piece(atlas, entity, cells.as_ref(), [sx, sy], depth))
                .flatten();
            let mut pieces: Vec<_> = shadow.into_iter().collect();
            pieces.push(ObjectPieceInstance {
                target,
                render_z: parent.policy.render_z,
                instance,
            });
            // Unit73CE5E/73CEAD -> Foot41C090 -> Techno705E00 draws
            // the embedded stencil after the body. Infantry51958A also calls
            // this shared Techno draw. Its separate airborne INFSHDW draw
            // above remains before its body.
            // Native positive/NoShadow controls:
            // tools/procedural_drawing_oracle/infantry_embedded_shadow.json.
            if matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            ) && let Some(body_entry) = entry
                && let Some(shadow) = embedded_shp_shadow_piece(
                    atlas,
                    state
                        .rules()
                        .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules)),
                    entity,
                    cells.as_ref(),
                    &body_key,
                    body_entry,
                    instance,
                    draw_decision,
                    parent.policy,
                )
            {
                pieces.push(shadow);
            }
            ground_objects.push(PlannedObjectInstance::object(parent, pieces));
        }

        // Emit building animation overlays and bib — but NOT during build-up/down.
        // Bibs and anims use the raw cell position (sy) — their own SHP offsets
        // (baked into the canvas) handle correct placement relative to the cell.
        if entity.category() == EntityCategory::Structure && !is_building_up && !is_building_down {
            if let Some(art) = art_reg
                && !null_image
            {
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
            }
            if let Some(name) = unload_art.as_ref().and_then(|art| art.under_door)
                && let Some(rules) = state.rules()
            {
                emit_factory_under_door(
                    &mut building_pieces,
                    atlas,
                    rules,
                    rules.object(type_str),
                    entity,
                    ShpSpriteKey {
                        palette_context: unload_art.as_ref().map_or(
                            crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme,
                            |art| art.palette,
                        ),
                        type_id: name.to_ascii_uppercase(),
                        facing: 0,
                        frame: 0,
                        house_color: hc,
                    },
                    [sx, sy],
                    lift_px,
                    depth,
                    body_tint,
                    palette_light,
                    colour_word,
                    draw_state,
                    draw_decision,
                );
            }
            // Building voxel guns remain inside their SHP hull's draw call.
            if let Some(rules_obj) = state.rules().and_then(|r| r.object(type_str))
                && rules_obj.turret_anim_is_voxel
                && let Some(unit_atlas) = &state.match_state.match_presentation.unit_atlas
            {
                let turret_offset = art_reg
                    .and_then(|art| art.resolve_metadata_entry(type_str, &rules_obj.image))
                    .map_or(0, |art| art.turret_offset);
                // TechnoClass::Draw's curtain arm (`0x0070678D..0x007067E2`)
                // and BuildingClass::Draw's colour word (`0x0043DC1C..
                // 0x0043DDF1`, DrawBody's) on the voxel cache blit.
                let (turret_tint, turret_light) =
                    crate::app::presentation::lighting::techno_effect_light(
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
                    render_pose.body,
                    render_pose.barrel,
                    entity.turret_anim_frame.frame(),
                    render_pose.recoil,
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
                    rules_obj.turret_anim_z_adjust,
                    &mut building_pieces,
                );
            }
        }

        if entity.category() == EntityCategory::Structure {
            ground_objects.push(PlannedObjectInstance::building(parent, building_pieces));
        }
    }
}

/// Techno705E00's embedded-shadow admission, also used when a Unit draws
/// a TerrainType image. Unit/Infantry GetHeight5F5F40 must be zero
/// (Infantry class15 dispatch705FA6..705FB2, shared70600C..70603F);
/// the existing isolated query includes live ramp and OnBridge height.
fn embedded_shp_shadow_admitted(
    object: Option<&crate::rules::object_type::ObjectType>,
    entity: &crate::sim::game_entity::GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    decision: crate::render::draw_state::DrawDecision,
) -> bool {
    matches!(decision.native_visual_state, 0 | 1)
        && !object.is_some_and(|object| object.no_shadow)
        && (!matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        ) || crate::sim::movement::air_movement::queried_flight_height(entity, cells) == 0)
}

/// The common embedded second-half shadow owner used by Building, Unit and Infantry
/// `TechnoClass_DrawSHP @ 0x00705E00` paths. Its caller passes the actual body
/// frame key/entry/instance and the same admitted native visual-state result;
/// this function does not infer a second frame, anchor, or visibility decision.
///
/// Native draws body frame `n`, then `n + frame_count/2` when its visual state
/// is 0/1 and `NoShadow=` is false. The darken blitter uses the shadow frame's
/// non-zero SHP indices as a mask and has no BUILDNGZ input. The caller's
/// Z policy survives: Unit2E01 only reads Z; Building6E01 reads/writes it.
pub(crate) fn embedded_shp_shadow_piece(
    atlas: &crate::render::sprite_atlas::SpriteAtlas,
    object: Option<&crate::rules::object_type::ObjectType>,
    entity: &crate::sim::game_entity::GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    body_key: &ShpSpriteKey,
    body_entry: &crate::render::sprite_atlas::ShpSpriteEntry,
    body_instance: SpriteInstance,
    decision: crate::render::draw_state::DrawDecision,
    policy: BlitPolicy,
) -> Option<ObjectPieceInstance> {
    if !embedded_shp_shadow_admitted(object, entity, cells, decision) {
        return None;
    }
    let frame_count = atlas.object_shp_frame_count(&body_key.type_id)?;
    let half = frame_count / 2;
    if body_key.frame >= half {
        return None;
    }
    let shadow_frame = body_key.frame.checked_add(half)?;
    if shadow_frame >= frame_count {
        return None;
    }
    let mut shadow_key = body_key.clone();
    shadow_key.frame = shadow_frame;
    let shadow_entry = atlas.get(&shadow_key)?;

    // Building43D290's body point is the Location art anchor plus the frame's
    // canvas offset. Recover that shared anchor before applying the shadow
    // frame's own offset.
    let anchor = [
        body_instance.position[0] - body_entry.offset_x,
        body_instance.position[1] - body_entry.offset_y,
    ];
    let lift_px = crate::render::locomotor_visual::screen_lift_px(entity);
    let mut instance = body_instance;
    instance.position = [
        anchor[0] + shadow_entry.offset_x,
        anchor[1] + shadow_entry.offset_y,
    ];
    instance.size = shadow_entry.pixel_size;
    instance.uv_origin = shadow_entry.uv_origin;
    instance.uv_size = shadow_entry.uv_size;
    instance.source_palette = shadow_entry.source_palette;
    // Both ordinary shadow arms clear body translucency and use intensity
    // 1000/colour word0 (706490..7064AC,706568..70657F).
    instance.draw_state = DrawState::default();
    instance.palette_light = instance
        .palette_light
        .with_brightness(1000)
        .with_colour_word(0);
    instance.alpha = 1.0;
    // Techno70647A/70649E (and70654F/706576 for visual state1) combine
    // both -2 terms before CC_Draw_Shape: -4 - AdjustForZ(Location.Z).
    instance.z_adjust = lifted_z_adjust(lift_px, -4);
    instance.z_gradient = pack_z_gradient(ZGradient::Flat, false);
    instance.zshape_origin = [0.0, 0.0];

    // RESIDUAL: Carryall child drawing raises this point another 14 pixels
    // when Techno+424 is set (706481/706559, Aircraft pickup store417297).
    // That rendering belongs to the existing unported Aircraft Carryall
    // mechanism; retail Carryall is on VehicleType HIND. Ordinary ground
    // Units use constructor6F2FF3's zero, without render-owned carryall state.
    Some(ObjectPieceInstance {
        render_z: policy.render_z,
        target: ObjectTexture::SpriteAtlasEdit(
            shadow_entry.page as usize,
            crate::render::terrain_draw::TerrainPiece::Shadow,
        ),
        instance,
    })
}

/// The shadow `InfantryClass::DrawIt @ 0x00518F90` draws under an airborne
/// infantry, before its body (`0x0051945D..0x0051958A`).
///
/// - Height is GetHeight (vtable `+0x1C8`, `0x005F5F40`). Over a structural
///   bridge cell (`+0x140 & 0x100`) at deck height or above, it drops the deck
///   (`0x00A8F234`, 416) when the cell's neighbour along the bridge also
///   carries one: north (`Adjacent_Cell(0)`) with flag `0x800`, else west (6).
/// - Only a positive height draws `INFSHDW.SHP` frame 1, at the body point
///   lowered by `AdjustForZ(height)`, with flags `0x2601` (centred shadow
///   edit, Z read only), gradient 0 and
///   `a7 = -5 - AdjustForZ(Location.Z - height)`.
/// - The DropPod locomotor's `POD.SHP` arm (`0x00519140`), which skips this
///   shadow, is dormant in skirmish.
fn infantry_air_shadow_piece(
    atlas: &crate::render::sprite_atlas::SpriteAtlas,
    entity: &crate::sim::game_entity::GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    body_point: [f32; 2],
    depth: f32,
) -> Option<ObjectPieceInstance> {
    use crate::map::bridge_facts::{BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL};
    use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
    use crate::util::native_x87::adjust_for_z_standard;
    let mut height = crate::sim::movement::air_movement::queried_flight_height(entity, cells);
    if let Some(cells) = cells {
        let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        let cell = cells.lookup_world(x, y);
        let flags = cells.flags(cell);
        if flags & BRIDGE_FLAG_STRUCTURAL != 0 && height >= BRIDGE_DECK_HEIGHT_LEPTONS {
            let direction = if flags & BRIDGE_FLAG_DIRECTION_ZERO != 0 {
                0
            } else {
                6
            };
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
            let (cx, cy) = cells.coord(cell);
            let neighbour = cells.lookup((cx.wrapping_add(dx as i16), cy.wrapping_add(dy as i16)));
            if cells.flags(neighbour) & BRIDGE_FLAG_STRUCTURAL != 0 {
                height = height.wrapping_sub(BRIDGE_DECK_HEIGHT_LEPTONS);
            }
        }
    }
    if height <= 0 {
        return None;
    }
    let entry = atlas.infantry_air_shadow()?;
    let location_z = crate::sim::movement::ground_pose::object_world_z_leptons(entity, None);
    Some(ObjectPieceInstance {
        target: ObjectTexture::SpriteAtlasEdit(
            entry.page as usize,
            crate::render::terrain_draw::TerrainPiece::Shadow,
        ),
        render_z: crate::render::tactical_draw_plan::RenderZPolicy::ReadOnly,
        instance: SpriteInstance {
            position: [
                body_point[0] + entry.offset_x,
                body_point[1] + adjust_for_z_standard(height) as f32 + entry.offset_y,
            ],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: entry.source_palette,
            depth,
            tint: crate::map::lighting::DEFAULT_TINT,
            palette_light: crate::render::palette_light::PaletteLight::plain(53, 1000),
            alpha: 1.0,
            z_adjust: (-5i32).wrapping_sub(adjust_for_z_standard(location_z.wrapping_sub(height)))
                as f32,
            z_gradient: pack_z_gradient(ZGradient::Flat, false),
            ..Default::default()
        },
    })
}

/// Emit a building's native B8 turret and C0 barrel at its TurretAnim anchor.
///
/// Building draw43DA80 uses the current primary facing and retained +148 HVA
/// counter; C0 pitches by +370 and stays on frame0 when a B8 turret exists.
/// The same cache key owns both parts, except during either part's recoil.
/// Executed controls: tools/voxel_oracle/building_barrel.json; the stock guns'
/// original rasters: tools/voxel_oracle/building_gun_raster.json.
///
/// The blit Z seed is Building vtable `0x7E3EBC+0x2EC` (`0x43E900`): it returns
/// `-AdjustForZ(this+0xA4)` plus `TurretAnimZAdjust` (`type+0x11E8`) when
/// `TurretAnimIsVoxel` (`type+0x16C5`) admits the voxel arm. The voxel blits pass
/// it as their Z argument: `0x707206` (uncached `0x706ED0`), `0x7073F1` (uncached
/// shadow `0x707280`) and `0x707706` (cached `0x707480`), plus the unit draws
/// `0x73B33D`/`0x73B3B2`/`0x73B412`, `0x73C772` and `0x73D236`. Shared `Techno`
/// draw `0x705E00` reaches the slot only on its `GetType()` 2 and 15 arms;
/// Building is type 6 (`0x459EC0`) and jumps past it, so a building's own SHP
/// draw never takes the adjustment:
/// Without it a gun loses the depth test to its own body and is clipped by the
/// base it stands on. Retail voxel-turret values: `NASAM`/`NALASR` `-20`,
/// `NAFLAK`/`YAGGUN` `-40`, `YAREFN` `-50`, `GTGCAN` `-60`, `CAOUTP` `-140`,
/// `CAEAST02` `-150`; `TESLA`/`ATESLA` set `-100` with no `TurretAnimIsVoxel`.
///
/// Neither gun blit takes a z-shape: 706ED0's 4AF2A0 call has none, and 707480
/// passes 437A10 a null one (`0x7076B7`, pushed at `0x7076F6`), so its per-pixel
/// adjustment row is the loader-zeroed `0x89C568`. The gun pieces carry none either.
///
/// It is not a ground-layer sort bias: the building's cell sort key reads only
/// `TurretAnimIsVoxel` (+32 leptons) and `Gate` (-16 leptons), so the turret
/// keeps the building's own sort depth. Folding the adjustment into the sort key
/// instead would pull the turret over units standing in front of the building.
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
    anim_z_adjust: i32,
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
        // 43DA80 submits the building draw point plus TurretAnimX/Y. A cache
        // miss (706ED0) blits the 754510 crop at that point plus the rect
        // origin (`0x007071CB..0x007071E2`); a hit (707480) draws what 6C89E0
        // cached from the same origin and rows (`0x006C8A16..0x006C8A3C`,
        // `0x007076A2..0x007076CE`). The atlas entry is that crop and origin.
        let tx = building_sx + anim_x as f32 + entry.offset_x + ox;
        let ty = building_sy + anim_y as f32 + entry.offset_y + oy;
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
                // VXL blit: gradient entry2, TurretAnimZAdjust as the class
                // term, lift cancelled, no DrawSHP -2.
                z_adjust: lifted_z_adjust(lift_px, anim_z_adjust),
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
/// The war-factory unload art `BuildingClass::DrawIt @ 0x0043D290` swaps in
/// while the building's mission (GetMission, vt+0x184) is Unload (0x10). The
/// roof variants apply when radio contact 0's type is `JumpJet=` (`+0xD94`)
/// or `BalloonHover=` (`+0xD6A`) (`0x0043D33F..0x0043D381`). A name whose
/// SHP did not load draws nothing in its place.
struct FactoryUnloadArt<'a> {
    /// `DeployingAnim=` or `RoofDeployingAnim=`, with the z adjustment that
    /// replaces `NormalZAdjust=` (-20 at `0x0043D6D3`, -40 at `0x0043D7D6`).
    body: Option<(&'a str, i32)>,
    /// `UnderDoorAnim=` or `UnderRoofDoorAnim=` (`0x0043D8EE..0x0043DA73`).
    under_door: Option<&'a str>,
    /// Both draw through the building's own DrawSHP palette, like its bib.
    palette: crate::render::sprite_atlas::ShpPaletteContext,
}

fn factory_unload_art<'a>(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    entity: &crate::sim::game_entity::GameEntity,
    art: &'a crate::rules::art_data::ArtEntry,
) -> Option<FactoryUnloadArt<'a>> {
    if entity.mission.effective().known() != Some(crate::sim::mission::MissionType::Unload) {
        return None;
    }
    let roof = entity
        .radio_contacts
        .slot(0)
        .and_then(|id| sim.entities().get(id))
        .and_then(|product| sim.interner().object_type(product.type_ref(), rules))
        .is_some_and(|object| object.jumpjet || object.balloon_hover);
    let door = &art.factory_door;
    let (body, under_door) = if roof {
        (
            door.roof_deploying.as_deref().map(|name| (name, -40)),
            door.under_roof_door.as_deref(),
        )
    } else {
        (
            door.deploying.as_deref().map(|name| (name, -20)),
            door.under_door.as_deref(),
        )
    };
    Some(FactoryUnloadArt {
        body,
        under_door,
        palette: building_art_palette_context(art),
    })
}

/// A building's own SHP pieces (bib, war-factory door art) draw through its
/// DrawSHP palette: the theater's with `TerrainPalette=`, else its scheme's.
fn building_art_palette_context(
    art: &crate::rules::art_data::ArtEntry,
) -> crate::render::sprite_atlas::ShpPaletteContext {
    if art.terrain_palette {
        crate::render::sprite_atlas::ShpPaletteContext::Cell
    } else {
        crate::render::sprite_atlas::ShpPaletteContext::SelectedScheme
    }
}

/// `UnderDoorAnim=` (or the roof variant) after the bib
/// (`BuildingClass::DrawIt @ 0x0043D8EE..0x0043DA73`): frame 1 at or below
/// `ConditionYellow=`, else 0, at the body's point. Like the bib it is a
/// 0x6E00 draw with gradient entry 0 and the colour word, but its a7 is
/// `-AdjustForZ(Z)`, and DrawSHP adds its embedded shadow.
#[allow(clippy::too_many_arguments)]
fn emit_factory_under_door(
    pieces: &mut Vec<PlannedBuildingPieceInstance>,
    atlas: &crate::render::sprite_atlas::SpriteAtlas,
    rules: &crate::rules::ruleset::RuleSet,
    object: Option<&crate::rules::object_type::ObjectType>,
    entity: &crate::sim::game_entity::GameEntity,
    mut key: ShpSpriteKey,
    [screen_x, screen_y]: [f32; 2],
    lift_px: i32,
    building_depth: f32,
    tint: [f32; 3],
    palette_light: crate::render::palette_light::PaletteLight,
    colour_word: u16,
    draw_state: DrawState,
    decision: crate::render::draw_state::DrawDecision,
) {
    key.frame = u16::from(object.is_some_and(|object| {
        crate::sim::building_art::requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        )
    }));
    let Some(entry) = atlas.get(&key) else {
        return;
    };
    let policy = BlitPolicy::opaque(SpriteEncoding::Plain);
    let instance = SpriteInstance {
        position: [screen_x + entry.offset_x, screen_y + entry.offset_y],
        size: entry.pixel_size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        source_palette: entry.source_palette,
        depth: building_depth,
        tint,
        palette_light: palette_light.with_colour_word(if policy.ors_colour_word(entry.extended) {
            colour_word
        } else {
            0
        }),
        alpha: 1.0,
        draw_state,
        z_adjust: lifted_z_adjust(lift_px, SHP_DRAW_Z_ADJUST_PX),
        z_gradient: pack_z_gradient(ZGradient::Flat, false),
        ..Default::default()
    };
    pieces.push(PlannedBuildingPieceInstance {
        kind: BuildingPieceKind::UnderDoor,
        z_bias: 0,
        policy,
        target: ObjectTexture::ShpPage(entry.page as usize),
        instance,
    });
    if let Some(shadow) = embedded_shp_shadow_piece(
        atlas, object, entity, None, &key, entry, instance, decision, policy,
    ) {
        pieces.push(PlannedBuildingPieceInstance {
            kind: BuildingPieceKind::UnderDoor,
            z_bias: 0,
            policy,
            target: shadow.target,
            instance: shadow.instance,
        });
    }
}

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
        palette_context: building_art_palette_context(art_entry),
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
    // 0x6E00 draw with gradient entry 0, a7 = `-AdjustForZ(Z)`, no
    // z-shape; shared case0 (0x00706430) adds -2. It tests and writes Z
    // like the body, and takes its colour word.
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
            z_adjust: lifted_z_adjust(lift_px, SHP_DRAW_Z_ADJUST_PX),
            z_gradient: pack_z_gradient(ZGradient::Flat, false),
            ..Default::default()
        },
    });
}

/// Building slot AnimClass objects retain their own Display membership.
/// Native Tactical6D91D8 recognizes Anim+118 and dispatches that member's
/// DrawIfVisible at6D916C; Building43D290 draws its body/bib, not these slots.
/// This entry is independent of the parent's body admission and viewport:
/// a slot offset can put an animation on screen while its parent is outside.
/// The existing slot palette/frame/light owner is shared by every building.
pub(crate) fn build_building_anim_instances(
    state: &AppState,
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
) {
    let (Some(runtime), Some(atlas), Some(rules)) = (
        state.match_state.sim_runtime.as_ref(),
        state.match_state.match_presentation.sprite_atlas.as_ref(),
        state.rules(),
    ) else {
        return;
    };
    let sim = &runtime.simulation;
    let grid = state.match_state.match_presentation.lighting.grid();
    let local_owner = crate::app::input::commands::preferred_local_owner_name(state);
    let local_owner_id = local_owner
        .as_deref()
        .and_then(|name| sim.interner().get(name));
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let shrouded = |cell| {
        !state.match_state.sandbox_full_visibility
            && local_owner_id
                .zip(cells.as_ref())
                .is_some_and(|(viewer, cells)| {
                    crate::sim::vision::cell_is_shrouded(sim.fog(), cells, viewer, cell)
                })
    };
    let axis = depth_axis(state);
    let zoom = state.match_state.input.zoom_level;
    let viewport = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
        state.render_width() as f32 / zoom,
        state.render_height() as f32 / zoom,
    ];
    for building in sim.entities().values() {
        if building.category() != EntityCategory::Structure
            || building
                .building_art
                .anim_slots()
                .iter()
                .all(Option::is_none)
            || slot_anims_hidden_by_sale(building)
        {
            continue;
        }
        let owner = sim.interner().resolve(building.owner());
        let house_color = state
            .match_state
            .match_presentation
            .house_color_map
            .get(owner)
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        let mut observer = super::helpers::observer_draw_context(
            sim,
            building,
            local_owner.as_deref(),
            local_owner_id,
            Some(rules),
        );
        observer.drawn_voxel = Some(false);
        // Preserve the existing slot material selector. The parent's
        // lifecycle/viewport/body visibility is not this Anim's admission;
        // its own retained Display member and hidden byte decide that.
        let material = DrawState::for_building_slot_anims(
            building,
            sim.session().binary_frame(),
            super::units::house_color_to_remap_row(house_color),
            observer,
        );
        // Retain the preexisting material visibility policy while moving
        // ordering. RESIDUAL: original Anim+178/199 visual-character producers
        // are not all retained separately; cloaked/observer-dependent slots
        // still borrow their building's material rather than native Anim state.
        // This does not gate on body viewport, lifecycle or anchor shroud.
        if !material.visible {
            continue;
        }
        let draw_state = material.state;
        let cell = (building.position.rx(), building.position.ry());
        let tint = grid.building_body_tint_at(cell);
        let extra_light = crate::app::presentation::lighting::body_extra_light(
            building,
            rules,
            sim,
            cells.as_ref(),
        );
        let selected_palette =
            crate::app::presentation::lighting::body_palette_light(grid, cell, extra_light);
        emit_building_anims(
            objects,
            order,
            atlas,
            rules.art(),
            Some(rules),
            sim.interner().resolve(building.type_ref()),
            house_color,
            tint,
            selected_palette,
            grid,
            cell,
            sim,
            building,
            &|id| anim_colour_word(state, sim, cells.as_ref(), &shrouded, id),
            axis.origin_y,
            axis.world_height,
            viewport,
            draw_state,
            state
                .match_state
                .match_presentation
                .in_game_options
                .detail_level as i32,
        );
    }
}

/// `BuildingClass::DrawIt` adds `ZShapePointMove=` to the BUILDNGZ origin
/// except during Construction or Selling (`0x0043D6DB..0x0043D712`).
fn building_zshape_point_move(
    entity: &crate::sim::game_entity::GameEntity,
    art: Option<&crate::rules::art_data::ArtEntry>,
) -> (i32, i32) {
    if entity.building_up() || entity.building_down() {
        (0, 0)
    } else {
        art.map_or((0, 0), |art| art.z_shape_point_move)
    }
}

/// `BuildingClass::DrawIt` (`0x0043D2C1..0x0043D306`) hides all 21 slot
/// anims (Anim `+0x19D`) while the building plays its build-up backwards: a
/// sale or undeploy (Mission Selling, `0x13`) in BState 0. Native writes the
/// byte each time it draws the building, and nothing clears it before the
/// sale removes the building, so the draw itself asks the same question.
fn slot_anims_hidden_by_sale(building: &crate::sim::game_entity::GameEntity) -> bool {
    building.building_down() && building.in_construction_bstate()
}

fn emit_building_anims(
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
    atlas: &crate::render::sprite_atlas::SpriteAtlas,
    art_reg: &crate::rules::art_data::ArtRegistry,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    building_type: &str,
    house_color: HouseColorIndex,
    tint: [f32; 3],
    attached_palette: crate::render::palette_light::PaletteLight,
    light_grid: &crate::map::lighting::CellLightGrid,
    cell: (u16, u16),
    sim: &crate::sim::world::Simulation,
    building: &crate::sim::game_entity::GameEntity,
    anim_word: &dyn Fn(u64) -> u16,
    origin_y: f32,
    world_height: f32,
    viewport: [f32; 4],
    draw_state: DrawState,
    detail_level: i32,
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
        let Some(anim_id) = building.building_art.anim_slots()[usize::from(anim.native_slot)]
        else {
            continue;
        };
        let Some(instance) = sim.anim(anim_id) else {
            continue;
        };
        let Some(parent) = order.object_draw(anim_id, SpriteEncoding::Plain) else {
            continue;
        };
        // Building's retained slot reaches DrawIt422CA0..4238AF until physical
        // release. That body gates on hidden19D, not19B or Logic membership.
        if instance.draw_runtime().hidden {
            continue;
        }
        let anim_name = sim.interner().resolve(instance.type_id());
        let Some(runtime_config) = art_reg.anim_runtime_config(anim_name) else {
            continue;
        };
        if !runtime_config.art_body_read {
            continue;
        }
        // DrawIt's translucency selection (`0x00423061`): a building being
        // erased draws its slot anims at 50%, or 75% for `DoubleThick=`.
        let Some(flags) = super::overlays::anim_instance_flags(
            Some(runtime_config),
            instance.draw_flags(),
            instance.runtime().current_frame,
            i32::from(
                super::overlays::presentation_anim_frame_count(
                    &atlas.active_anim_frame_counts,
                    anim_name,
                )
                .unwrap_or(0),
            ),
            detail_level,
            instance.draw_runtime(),
        ) else {
            continue;
        };
        let alpha = crate::rules::art_data::anim_translucency_source_alpha(flags);
        // Anim+AC is relative to this instance's current type Start. Replacement
        // copies only AC; its timer/loop state remains the new constructor's.
        let Ok(frame) = u16::try_from(
            instance
                .runtime()
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
        let anim_entry = atlas.get(&anim_key);

        // Slot construction/repositioning already converts art X/Y into
        // this Anim's retained coordinate. Consume that owner rather than
        // deriving a second coordinate from the current building body.
        let Some(coord) = sim.anim_absolute_coord(anim_id) else {
            continue;
        };
        let (screen_x, screen_y, _, _, _, lift_px) =
            super::overlays::anim_world_render_coords(coord);
        // Original451988 copies the slot's ZAdjust even when it is zero;
        // the constructor's AnimType ZAdjust is not a zero-value fallback.
        let z_adjust_px = instance.z_adjust() + ANIM_DRAW_DEPTH_BIAS_PX;
        let anim_depth = apply_shape_z_adjust(
            compute_sprite_depth_params_lifted(origin_y, world_height, screen_y, lift_px),
            z_adjust_px,
            world_height,
        );

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
                    cell,
                    config,
                    false,
                    instance.remap().as_ref(),
                ),
            )
        };
        // Preserve the existing body's color-word path. The shared pass
        // builder clears it independently for the later shadow.
        let policy = BlitPolicy::z_read(SpriteEncoding::Plain);
        let palette_light = palette_light.with_colour_word(anim_word(anim_id));
        // DrawIt422CA0 -> 4AED70 retains the Anim's Convert selector. Reducing
        // it to alpha loses the indexed palette/A lookup and packed RGB565
        // blend (spatial_oracle/building_anim_translucency). The inherited
        // cloak/offset/effect composition is a separate, unresolved owner.
        // The shared pass builder replaces this material for the shadow.
        let (alpha, draw_state) = if flags & 6 != 0
            && super::anim_draw::ordinary_anim_type(anim_name, Some(runtime_config))
            && anim_entry.is_some_and(|entry| entry.source_palette != 0)
            && palette_light.rows() != 0
            && draw_state
                == (DrawState {
                    remap_row: draw_state.remap_row,
                    ..DrawState::default()
                }) {
            (
                1.0,
                DrawState::from_native_selector(draw_state.remap_row, (flags & 6) as u8),
            )
        } else {
            (alpha, draw_state)
        };
        objects.push(PlannedObjectInstance::object(
            parent,
            super::anim_draw::anim_draw_pieces(
                atlas,
                super::anim_draw::AnimDrawInput {
                    key: &anim_key,
                    entry: anim_entry,
                    point: [screen_x, screen_y],
                    config,
                    flags,
                    lift_px,
                    axis: crate::render::batch::DepthAxis {
                        origin_y,
                        world_height,
                    },
                    viewport,
                    legacy_policy: policy.render_z,
                    bounce_drop_px: super::anim_draw::anim_bounce_drop_px(
                        sim,
                        instance,
                        [coord.x, coord.y, coord.z],
                    ),
                    body: SpriteInstance {
                        depth: anim_depth,
                        tint: anim_tint,
                        palette_light,
                        alpha,
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
                },
            ),
        ));
    }
}

/// AnimClass::DrawIt's colour word for the anim `anim`
/// (`0x004233EE..0x00423630`): a building slot anim (its `+0x118`, set by
/// CreateAnimForSlot at `0x0045199B`) takes
/// [`crate::app::presentation::lighting::shp_colour_word`] of the first
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
    sim.occupancy()
        .first_building_on_layer(
            cell.0,
            cell.1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
        )
        .and_then(|id| sim.entities().get(id))
        .map_or(0, |building| {
            crate::app::presentation::lighting::shp_colour_word(building, sim, rules, || {
                shrouded(cell)
            })
        })
}

/// Unit73C5F0 remains the body owner when its image is a TerrainType. Only
/// the source texture and Cell Convert differ: no Terrain object, placement,
/// sorting key, or Terrain71C1B0 draw constants are introduced.
fn emit_unit_terrain_body(
    state: &AppState,
    entity: &crate::sim::game_entity::GameEntity,
    type_id: &str,
    frame: u16,
    point: [f32; 2],
    decision: crate::render::draw_state::DrawDecision,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    order: &NativeDisplayOrder,
    output: &mut Vec<PlannedObjectInstance>,
) {
    use crate::render::terrain_draw::TerrainPiece;
    let draw_state = decision.state;
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
        crate::app::presentation::lighting::techno_effect_light(entity, tint, palette_light, sim);
    let depth = compute_sprite_depth(
        state,
        ground_sort_row(entity, point[1] + body.offset_y + body.pixel_size[1]),
        entity.position.z(),
    );
    let bridge_fudge = super::foot_depth::shp_unit_bridge_fudge(state, entity);
    let body_z =
        super::foot_depth::shp_z_adjust(state, entity) - if bridge_fudge { 16.0 } else { 0.0 };
    let shadow_z = lifted_z_adjust(crate::render::locomotor_visual::screen_lift_px(entity), -4);
    let shadow_admitted = embedded_shp_shadow_admitted(
        state
            .rules()
            .and_then(|rules| sim.interner().object_type(entity.type_ref(), rules)),
        entity,
        Some(cells),
        decision,
    );
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
        if piece == TerrainPiece::Shadow && !shadow_admitted {
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
    let facing = crate::app::presentation::interpolation::pose(state, entity, binary_frame).body;
    resolve_object_shp_frame_with_facing(state, type_id, entity, facing, cells)
}

/// The draw already owns a fresh pose. Other callers still use the wrapper
/// above, which reads its own sample and retains same-frame mutation checks.
fn resolve_object_shp_frame_with_facing(
    state: &AppState,
    type_id: &str,
    entity: &crate::sim::game_entity::GameEntity,
    facing: u16,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> Option<u16> {
    let facing = (facing >> 8) as u8;
    // Pass raw facing (not canonical) to resolve_shp_frame so the
    // facing-to-index division works correctly for any facing count
    // (6, 8, 10, etc.). The absolute frame index encodes the direction.
    let type_id = if entity.category() == EntityCategory::Unit {
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map_or(type_id, |runtime| {
                runtime.simulation.interner().resolve(entity.type_ref())
            })
    } else {
        type_id
    };
    let sequence_set = state
        .rules()
        .and_then(|rules| rules.animation_sequence(type_id));
    if entity.category() == EntityCategory::Unit
        && entity.animation.as_ref().is_none_or(|anim_state| {
            matches!(
                anim_state.sequence(),
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
            entity.body_frame_counter.get(),
            entity.unit_firing_counter.get(),
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
                let cell = cells.lookup((entity.position.rx() as i16, entity.position.ry() as i16));
                cells.land_type(cell) == 2
            });
            if water && !entity.on_bridge() { 16 } else { 0 }
        } else {
            doing
        };
        let record = set.infantry_action(selected)?;
        let facing = state
            .rules()
            .and_then(|rules| {
                let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
                sim.interner().object_type(entity.type_ref(), rules)
            })
            .filter(|object| object.jumpjet && !object.jumpjet_turn)
            .filter(|_| {
                entity.locomotor.as_ref().is_some_and(|loco| {
                    loco.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
                })
            })
            .and(entity.attack_target.target())
            .and_then(|target| {
                let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
                crate::sim::movement::turret::facing_toward_target(entity, &target, sim)
            })
            .map_or(facing, |direction| (direction >> 8) as u8);
        // Native EAX remains signed. A negative/wide result cannot alias an
        // unrelated valid frame in the u16 SHP atlas. This is an asset boundary,
        // not evidence for native invalid-index shape-access behavior.
        return u16::try_from(animation::resolve_shp_frame(record, facing, stage)).ok();
    }
    if let (Some(anim_state), Some(set)) = (entity.animation.as_ref(), sequence_set)
        && let Some(def) = set.get(&anim_state.sequence())
    {
        return u16::try_from(animation::resolve_shp_frame(
            def,
            facing,
            i32::from(anim_state.frame_index()),
        ))
        .ok();
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
                    crate::sim::combat::Health::new(row.input.current),
                    row.input.strength,
                    row.input.yellow(),
                ),
                row.output.generic_art_damaged,
                "{row:?}"
            );
        }
    }
    use crate::sim::building_art::occupied_body_frame;

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

#[cfg(test)]
#[path = "building_anim_tests.rs"]
mod building_anim_tests;

#[cfg(test)]
#[path = "building_anim_translucency_gpu_tests.rs"]
mod building_anim_translucency_gpu_tests;
#[cfg(test)]
pub(crate) use building_anim_translucency_gpu_tests::{PLANT_OWNER, SlotScene};

#[cfg(test)]
#[path = "building_anim_detail_tests.rs"]
mod building_anim_detail_tests;

#[cfg(test)]
#[path = "building_anim_detail_order_tests.rs"]
mod building_anim_detail_order_tests;

#[cfg(test)]
#[path = "anim_detail_delay_tests.rs"]
mod anim_detail_delay_tests;

#[cfg(test)]
#[path = "building_sale_draw_tests.rs"]
mod building_sale_draw_tests;

#[cfg(test)]
#[path = "factory_door_tests.rs"]
mod factory_door_tests;

#[cfg(test)]
#[path = "infantry_air_shadow_tests.rs"]
mod infantry_air_shadow_tests;

#[cfg(test)]
#[path = "embedded_shadow_tests.rs"]
mod embedded_shadow_tests;
