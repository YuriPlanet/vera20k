//! Shared helpers for instance builders — depth sorting, interpolation, visibility.
//!
//! These utilities are used by the unit, SHP, and overlay instance builders.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::map::terrain;
use crate::render::batch::DepthAxis;
use crate::render::native_z;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::Position;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use crate::util::fixed_math::SIM_ZERO;

/// Tactical6D8F19..6D95A9 visits the five retained Display vectors forward.
/// Store-only and Logic-only entities cannot become render/pick candidates.
pub(crate) fn tactical_entity_encounter_order(sim: &crate::sim::world::Simulation) -> Vec<u64> {
    sim.display_layers()
        .ordered_ids()
        .copied()
        .filter(|id| sim.entities().get(*id).is_some())
        .collect()
}

/// Common admission into the tactical render-tracked object set. SHP, voxel,
/// and input consumers share this exact gate so hidden passengers, limboed
/// objects, shrouded enemies, and invisible draw states cannot drift between
/// what is rendered and what can seed tactical selection.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tactical_entity_render_admission(
    entity: &GameEntity,
    owner: &str,
    local_owner: Option<&str>,
    local_owner_id: Option<InternedId>,
    fog: &FogState,
    ignore_visibility: bool,
    current_frame: u32,
    remap_row: u32,
    observer: crate::render::draw_state::ObserverDrawContext,
) -> Option<crate::render::draw_state::DrawDecision> {
    if entity.lifecycle.in_limbo
        || entity.passenger_role.is_inside_transport()
        || !is_entity_visible_for_local_owner(
            local_owner,
            fog,
            &entity.position,
            owner,
            ignore_visibility,
            local_owner_id,
        )
    {
        return None;
    }
    let decision = crate::render::draw_state::DrawState::for_entity(
        entity,
        current_frame,
        remap_row,
        observer,
    );
    decision.visible.then_some(decision)
}

/// The current visible-object window in the same encounter order used above.
/// Retail registration rejects anchors more than 32 screen pixels outside the
/// tactical viewport. Keep that bound here so TypeSelect's screen stage and
/// held exact-type scope cannot pull objects from elsewhere on the map.
pub(crate) fn tactical_screen_entity_encounter_order(state: &AppState) -> Vec<u64> {
    tactical_bounded_entity_encounter_order(state, false)
}

/// Native band-box caught-anything source: render-tracked mobiles plus the
/// live building array registered in bulk without a shroud test.
pub(crate) fn tactical_band_preflight_entity_encounter_order(state: &AppState) -> Vec<u64> {
    tactical_bounded_entity_encounter_order(state, true)
}

fn tactical_bounded_entity_encounter_order(
    state: &AppState,
    bulk_register_live_buildings: bool,
) -> Vec<u64> {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return Vec::new();
    };
    let zoom = state.match_state.input.zoom_level.max(f32::EPSILON);
    let margin = 32.0 / zoom;
    let (width_px, height_px) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
    let min_x = state.match_state.input.camera_x - margin;
    let min_y = state.match_state.input.camera_y - margin;
    let max_x = state.match_state.input.camera_x + width_px as f32 / zoom + margin;
    let max_y = state.match_state.input.camera_y + height_px as f32 / zoom + margin;
    let local_owner = crate::app::input::commands::preferred_local_owner_name(state);
    let local_owner_id = local_owner
        .as_deref()
        .and_then(|owner| sim.interner.get(owner));

    compose_tactical_screen_entity_encounter_order(
        sim,
        (min_x, min_y, max_x, max_y),
        local_owner.as_deref(),
        local_owner_id,
        &sim.fog,
        state.match_state.sandbox_full_visibility,
        sim.session.binary_frame,
        bulk_register_live_buildings,
    )
}

#[allow(clippy::too_many_arguments)]
fn compose_tactical_screen_entity_encounter_order(
    sim: &crate::sim::world::Simulation,
    bounds: (f32, f32, f32, f32),
    local_owner: Option<&str>,
    local_owner_id: Option<InternedId>,
    fog: &FogState,
    ignore_visibility: bool,
    current_frame: u32,
    bulk_register_live_buildings: bool,
) -> Vec<u64> {
    let (min_x, min_y, max_x, max_y) = bounds;
    let mut candidates = tactical_entity_encounter_order(sim);
    // Band-box preflight has a separate native bulk Building registration
    // path. Ordinary picking/rendering never scans the unregistered store.
    if bulk_register_live_buildings {
        candidates.extend(sim.entities().values().filter_map(|entity| {
            (entity.category == EntityCategory::Structure
                && sim.display_layers().layer_of(entity.stable_id()).is_none())
            .then_some(entity.stable_id())
        }));
    }
    candidates
        .into_iter()
        .filter(|id| {
            sim.entities().get(*id).is_some_and(|entity| {
                let owner = sim.interner.resolve(entity.owner());
                let admitted = if bulk_register_live_buildings
                    && entity.category == EntityCategory::Structure
                {
                    entity.lifecycle.object_alive && !entity.lifecycle.in_limbo
                } else {
                    tactical_entity_render_admission(
                        entity,
                        owner,
                        local_owner,
                        local_owner_id,
                        fog,
                        ignore_visibility,
                        current_frame,
                        0,
                        crate::render::draw_state::ObserverDrawContext {
                            owner_is_allied: local_owner.is_some_and(|observer| {
                                crate::map::houses::is_allied_with(
                                    &sim.house_alliances,
                                    observer,
                                    owner,
                                )
                            }),
                            detects_cloak: local_owner_id.is_some_and(|observer| {
                                fog.has_sensor_for_house(
                                    observer,
                                    entity.position.rx,
                                    entity.position.ry,
                                )
                            }),
                        },
                    )
                    .is_some()
                };
                if !admitted {
                    return false;
                }
                let (x, y) = interpolated_screen_position_entity(entity);
                x >= min_x && x <= max_x && y >= min_y && y <= max_y
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellVisibilityState {
    Visible,
    Shrouded,
}

/// Which display band an entity's body is drawn in.
///
/// gamemd keeps five display layers and asks each object which one it belongs
/// to. `UnitClass`, `InfantryClass` and `AircraftClass` all forward that
/// question straight to the attached locomotor, so the answer is a property of
/// the locomotor rather than of the unit category:
///
/// * Drive, Walk, Hover, Ship, Mech and Teleport are Ground (layer 2)
///   unconditionally — each of those locomotor slots is a two-instruction
///   `return 2`.
/// * Fly is Top (layer 4) the moment its object's height is above zero and
///   Ground otherwise. It never consults the in-air flag, so a landed aircraft
///   is an ordinary Ground object that sorts with the tanks around it.
/// * Jumpjet is Ground while grounded, then Air (layer 3) climbing and Top once
///   it reaches its own hover height.
/// * Rocket is Air. So is DropPod, which is unreachable in stock YR.
/// * A parachuting infantryman keeps its Walk locomotor, so it stays Ground
///   despite hanging in the air — its altitude changes only where it is drawn.
///
/// Only layer 2 is kept sorted; the rest append and render in submission order,
/// and every layer above 2 is drawn after all of layer 2. Air and Top are
/// therefore indistinguishable as far as ground objects are concerned, and we
/// have no separate Air object band, so both collapse into [`EntityDrawBand::Top`]
/// here. The only ordering that collapse can disturb is Air-vs-Top against the
/// layer-3 particle stream, which in stock YR is a takeoff's worth of frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityDrawBand {
    /// gamemd layer 2 — the single Y-sorted band that holds buildings,
    /// vehicles, infantry on foot and landed aircraft.
    Ground,
    /// gamemd layers 3 and 4 — drawn after every Ground object.
    Top,
}

/// Which band this entity's body belongs to, per gamemd's per-locomotor answer.
///
/// The state read here mirrors what `render::locomotor_visual` reads to decide
/// what is holding the entity up, because that is the same question: an entity
/// is above the Ground band exactly when something has lifted it off the floor.
pub(crate) fn entity_draw_band(entity: &GameEntity) -> EntityDrawBand {
    // Object-level falling. The locomotor underneath is still Walk or Drive,
    // both of which answer Ground, so a paradrop never leaves layer 2.
    if entity.parachute_state.is_some() {
        return EntityDrawBand::Ground;
    }
    // Scripted missiles fly on the Rocket locomotor, which answers Air.
    if entity.rocket_state.is_some() {
        return EntityDrawBand::Top;
    }
    let Some(loco) = entity.locomotor.as_ref() else {
        return EntityDrawBand::Ground;
    };
    match loco.kind {
        // The Rocket slot answers Air unconditionally — it has no height test
        // of its own. Reachable only if a missile is ever built locomotor-first,
        // since `rocket_state` answers above.
        LocomotorKind::Rocket => EntityDrawBand::Top,
        // The two flying locomotors gate on height, not on the locomotor's
        // nominal layer: `MovementLayer::Air` is fixed at construction for these
        // kinds, so a Harrier parked on its pad still carries it.
        LocomotorKind::Fly | LocomotorKind::Jumpjet => {
            if loco.altitude > SIM_ZERO {
                EntityDrawBand::Top
            } else {
                EntityDrawBand::Ground
            }
        }
        _ => EntityDrawBand::Ground,
    }
}

/// Convert the screen row an entity is *drawn* at into the row it *sorts* at.
///
/// gamemd's sort key is `renderCoords.X + renderCoords.Y` — the object's world
/// position, with the Z component structurally absent from the sum, for every
/// class that does not override the key. The row an entity is drawn at is not
/// that row: `render::locomotor_visual` lifts the projection by the entity's
/// height first, 216 px for an aircraft at stock `FlightLevel=1500`. Feeding
/// the drawn row into the depth key would sort that aircraft ~14 iso rows
/// behind its own cell, where later ground objects cover it and the terrain
/// depth buffer clips it. Adding the lift back recovers the ground row, which
/// is the screen-space image of X+Y.
///
/// `height_lift_px` is therefore exactly the term needed to recover the Z-free
/// row. Grounded infantry contribute zero lift, so their drawn and sort rows
/// are identical.
pub(crate) fn ground_sort_row(entity: &GameEntity, drawn_row_y: f32) -> f32 {
    drawn_row_y + crate::render::locomotor_visual::height_lift_px(entity)
}

/// Compute depth for a sprite from screen position.
///
/// The depth value is the painter's sort key for the residual depth-sorted
/// streams (largest = furthest back = drawn first) and the flat depth of
/// draws that go through the passthrough or depth-test pipelines. The
/// Z-tested pipelines recompute their per-pixel depth from `z_adjust` /
/// `z_gradient` on the same axis (`render::native_z`), so a sort depth and a
/// native Z are directly comparable: one native Z unit is one world row.
///
/// Lower screen_y → larger depth (further from camera). The height lift is
/// taken back off so the row is the ground-projected one, exactly as every
/// native class folds `-AdjustForZ(Location.Z)` into its Z term.
pub(crate) fn compute_sprite_depth(state: &AppState, screen_y: f32, z: u8) -> f32 {
    let axis = depth_axis(state);
    compute_sprite_depth_params(axis.origin_y, axis.world_height, screen_y, z)
}

/// The normalised depth axis of the loaded map (`DepthAxis::NONE` outside a
/// match): what `BatchRenderer::update_camera` uploads for the shaders.
pub(crate) fn depth_axis(state: &AppState) -> DepthAxis {
    state
        .match_state
        .match_presentation
        .terrain_grid
        .as_ref()
        .map(|g| DepthAxis {
            origin_y: g.origin_y,
            world_height: g.world_height,
        })
        .unwrap_or(DepthAxis::NONE)
}

/// Compute sprite depth from explicit parameters.
/// Same formula as `compute_sprite_depth` but for callers that already have
/// origin_y and world_height (avoids re-extracting from AppState).
pub(crate) fn compute_sprite_depth_params(
    origin_y: f32,
    world_height: f32,
    screen_y: f32,
    z: u8,
) -> f32 {
    let iso_row: f32 = screen_y + z as f32 * terrain::HEIGHT_STEP;
    native_z::depth_for_row(iso_row, origin_y, world_height)
}

/// Depth of a sprite whose rows were drawn lifted by `lift_px` pixels, the
/// exact `AdjustForZ(Location.Z)`; the un-lifted row is the ground-projected
/// one the depth keys on.
pub(crate) fn compute_sprite_depth_params_lifted(
    origin_y: f32,
    world_height: f32,
    screen_y: f32,
    lift_px: i32,
) -> f32 {
    native_z::depth_for_row(screen_y + lift_px as f32, origin_y, world_height)
}

/// `SpriteInstance::z_adjust` for a draw lifted by an exact `AdjustForZ`
/// rather than whole levels: every native class folds `-AdjustForZ` in.
pub(crate) fn lifted_z_adjust(lift_px: i32, class_term: i32) -> f32 {
    (class_term - lift_px) as f32
}

/// `SpriteInstance::z_adjust` for a draw whose rows are drawn lifted by
/// `z` height levels: the class term with the lift cancelled
/// (`native_z::ground_anchored_z_adjust`).
pub(crate) fn ground_z_adjust(z: u8, class_term: i32) -> f32 {
    native_z::ground_anchored_z_adjust(i32::from(z), class_term) as f32
}

/// Extra depth bias carried by every anim SHP draw in the original engine's
/// standard shape-depth expression, on top of the anim's own `ZAdjust=`.
pub(crate) const ANIM_DRAW_DEPTH_BIAS_PX: i32 = -2;

/// Apply a native `ZAdjust=` depth-sort bias to a computed sprite depth.
///
/// The original engine composes a draw's sort value as a cell/row base plus a
/// signed pixel bias, with the height correction subtracted — smaller value =
/// closer to the camera. Our normalized depth axis points the same way (lower
/// = closer) and the base depth already encodes the row term, so a ZAdjust of
/// N pixels maps to a depth delta of `N / world_height`. Negative ZAdjust
/// pulls the sprite toward the camera (damage fires, muzzle flashes, arrows
/// and parachutes all use negative values to draw in front).
///
/// Note: 1000 is NOT a neutral value here — that convention belongs to the
/// per-cell terrain z path, which is a separate mechanism. Neutral is 0.
pub(crate) fn apply_shape_z_adjust(depth: f32, z_adjust_px: i32, world_height: f32) -> f32 {
    (depth + z_adjust_px as f32 / world_height.max(1.0)).clamp(0.001, 0.999)
}

/// Effective anim `ZAdjust`: a nonzero per-slot override (e.g. a building's
/// `ActiveAnimZAdjust=`) wins; zero falls back to the anim type's own
/// `ZAdjust=` from its art section.
pub(crate) fn effective_anim_z_adjust(slot_z_adjust: i32, type_z_adjust: i32) -> i32 {
    if slot_z_adjust != 0 {
        slot_z_adjust
    } else {
        type_z_adjust
    }
}

/// Where this entity is drawn, sub-cell offsets and height lift included.
///
/// Thin wrapper over [`crate::render::locomotor_visual::screen_position`],
/// which is the single owner of the answer. Callers must NOT apply a height
/// offset of their own on top — doing that in two places is what used to lift
/// aircraft twice.
pub(crate) fn interpolated_screen_position_entity(
    entity: &crate::sim::game_entity::GameEntity,
) -> (f32, f32) {
    crate::render::locomotor_visual::screen_position(entity)
}

/// Check whether an entity is visible to the local player based on shroud.
///
/// In standard YR (FogOfWar=false), once a cell is explored it stays fully
/// visible forever. Friendly entities are always visible. Enemy entities are
/// visible if the cell they occupy has been explored (revealed).
pub(crate) fn is_entity_visible_for_local_owner(
    local_owner: Option<&str>,
    fog: &FogState,
    pos: &Position,
    owner: &str,
    ignore_visibility: bool,
    local_owner_id: Option<InternedId>,
) -> bool {
    if ignore_visibility {
        return true;
    }
    let Some(local_owner) = local_owner else {
        return true;
    };
    if fog.is_friendly(local_owner, owner) {
        return true;
    }
    let owner_id = local_owner_id.unwrap_or_default();
    fog.is_cell_revealed(owner_id, pos.rx, pos.ry)
        && !fog.is_cell_gap_covered(owner_id, pos.rx, pos.ry)
}

pub(crate) fn cell_visibility_for_local_owner(
    local_owner_id: Option<InternedId>,
    fog: Option<&FogState>,
    rx: u16,
    ry: u16,
    ignore_visibility: bool,
) -> CellVisibilityState {
    if ignore_visibility {
        return CellVisibilityState::Visible;
    }
    let Some(local_owner_id) = local_owner_id else {
        return CellVisibilityState::Visible;
    };
    let Some(fog) = fog else {
        return CellVisibilityState::Visible;
    };
    // Standard YR (FogOfWar=false): explored = fully visible, no intermediate state.
    if fog.is_cell_revealed(local_owner_id, rx, ry) {
        CellVisibilityState::Visible
    } else {
        CellVisibilityState::Shrouded
    }
}

/// Viewport frustum cull check: is the entity's bounding box visible on screen?
pub(crate) fn in_view(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    cam_x: f32,
    cam_y: f32,
    sw: f32,
    sh: f32,
    m: f32,
) -> bool {
    x + w >= cam_x - m && x <= cam_x + sw + m && y + h >= cam_y - m && y <= cam_y + sh + m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::locomotor_visual::{ground_screen_position, screen_position};
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::util::fixed_math::SimFixed;

    /// Stock YR sets one global `FlightLevel=1500` for every aircraft.
    const STOCK_FLIGHT_LEVEL_LEPTONS: i32 = 1500;

    fn entity_with_locomotor(
        type_ref: &str,
        kind: LocomotorKind,
        altitude_leptons: i32,
        rx: u16,
        ry: u16,
    ) -> GameEntity {
        let mut entity = GameEntity::test_default(1, type_ref, "Americans", rx, ry);
        let mut loco = LocomotorState::for_test_kind(kind);
        loco.altitude = SimFixed::from_num(altitude_leptons);
        entity.locomotor = Some(loco);
        entity
    }

    #[test]
    fn item83_equal_tactical_keys_keep_live_registration_order() {
        let mut sim = crate::sim::world::Simulation::new();
        sim.entities_mut()
            .insert(GameEntity::test_default(1, "E1", "Americans", 10, 10));
        sim.entities_mut()
            .insert(GameEntity::test_default(2, "E1", "Americans", 10, 10));
        sim.entities_mut().get_mut(2).unwrap().lifecycle.in_limbo = true;
        sim.entities_mut().get_mut(1).unwrap().lifecycle.in_limbo = true;
        sim.reveal(2);
        sim.reveal(1);
        sim.set_logic_order_for_test(vec![1, 2]);
        assert_eq!(tactical_entity_encounter_order(&sim), [2, 1]);
        sim.entities_mut()
            .insert(GameEntity::test_default(3, "E1", "Americans", 10, 10));
        sim.set_logic_order_for_test(vec![3, 1, 2]);
        assert_eq!(
            tactical_entity_encounter_order(&sim),
            [2, 1],
            "Logic/store membership is insufficient"
        );
        sim.conceal(2);
        assert_eq!(
            tactical_entity_encounter_order(&sim),
            [1],
            "conceal removes the pick/render candidate"
        );
        sim.reveal(2);
        assert_eq!(
            tactical_entity_encounter_order(&sim),
            [1, 2],
            "resubmission follows retained equal-key members"
        );
    }

    #[test]
    fn item83_shared_render_admission_excludes_hidden_passenger_and_limbo() {
        use crate::sim::passenger::PassengerRole;

        let fog = FogState::default();
        let local_owner_id = crate::sim::intern::test_intern("Americans");
        let mut entity = GameEntity::test_default(1, "AMCV", "Americans", 10, 10);
        entity.lifecycle.object_alive = true;
        entity.lifecycle.in_limbo = false;

        let admitted = |entity: &GameEntity, owner: &str, ignore_visibility: bool| {
            tactical_entity_render_admission(
                entity,
                owner,
                Some("Americans"),
                Some(local_owner_id),
                &fog,
                ignore_visibility,
                0,
                0,
                crate::render::draw_state::ObserverDrawContext::default(),
            )
            .is_some()
        };

        assert!(admitted(&entity, "Americans", false));
        entity.passenger_role = PassengerRole::Inside { transport_id: 9 };
        assert!(!admitted(&entity, "Americans", false));
        entity.passenger_role = PassengerRole::None;
        entity.lifecycle.in_limbo = true;
        assert!(!admitted(&entity, "Americans", false));

        entity.lifecycle.in_limbo = false;
        assert!(admitted(&entity, "Americans", false));
        entity.owner = crate::sim::intern::test_intern("Soviet");
        assert!(
            !admitted(&entity, "Soviet", false),
            "an unrevealed enemy is absent from every screen selection consumer"
        );
        assert!(
            admitted(&entity, "Soviet", true),
            "the same entity is admitted when the renderer's visibility override is active"
        );
    }

    #[test]
    fn item83_band_preflight_source_bulk_registers_hidden_live_building_only() {
        use crate::app::input::entity_pick::compute_box_selection_snapshot;
        use crate::sim::components::Health;

        let mut sim = crate::sim::world::Simulation::new();
        let local_owner = sim.interner.intern("Americans");
        let enemy_owner = sim.interner.intern("Soviet");
        let mobile_type = sim.interner.intern("E1");
        let building_type = sim.interner.intern("NAPOWR");

        let mut hidden_mobile = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            enemy_owner,
            Health { current: 100 },
            mobile_type,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        let mut hidden_building = GameEntity::new_at_frame_zero_for_test(
            2,
            20,
            20,
            0,
            0,
            enemy_owner,
            Health { current: 100 },
            building_type,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        hidden_mobile.lifecycle.in_limbo = false;
        hidden_building.lifecycle.in_limbo = false;
        let mobile_anchor = interpolated_screen_position_entity(&hidden_mobile);
        let building_anchor = interpolated_screen_position_entity(&hidden_building);
        sim.entities_mut().insert(hidden_mobile);
        sim.entities_mut().insert(hidden_building);
        sim.entities_mut().get_mut(1).unwrap().lifecycle.in_limbo = true;
        sim.reveal(1);

        let bounds = (
            mobile_anchor.0.min(building_anchor.0) - 32.0,
            mobile_anchor.1.min(building_anchor.1) - 32.0,
            mobile_anchor.0.max(building_anchor.0) + 32.0,
            mobile_anchor.1.max(building_anchor.1) + 32.0,
        );
        let visible = compose_tactical_screen_entity_encounter_order(
            &sim,
            bounds,
            Some("Americans"),
            Some(local_owner),
            &sim.fog,
            false,
            0,
            false,
        );
        let preflight = compose_tactical_screen_entity_encounter_order(
            &sim,
            bounds,
            Some("Americans"),
            Some(local_owner),
            &sim.fog,
            false,
            0,
            true,
        );

        assert!(
            visible.is_empty(),
            "neither shrouded enemy is render-tracked"
        );
        assert_eq!(
            preflight,
            [2],
            "only the live building is bulk-registered for band preflight"
        );

        let building_box = compute_box_selection_snapshot(
            sim.entities(),
            &preflight,
            &visible,
            &[],
            Some(&sim.fog),
            Some("Americans"),
            building_anchor.0 - 1.0,
            building_anchor.1 - 1.0,
            building_anchor.0 + 1.0,
            building_anchor.1 + 1.0,
            false,
            None,
            None,
            Some(&sim.interner),
        )
        .expect("the hidden building must consume a non-Shift band release");
        assert!(building_box.clear);
        assert!(
            building_box.select.is_empty(),
            "bulk registration affects caught-anything only, not band candidates"
        );

        assert!(
            compute_box_selection_snapshot(
                sim.entities(),
                &preflight,
                &visible,
                &[],
                Some(&sim.fog),
                Some("Americans"),
                mobile_anchor.0 - 1.0,
                mobile_anchor.1 - 1.0,
                mobile_anchor.0 + 1.0,
                mobile_anchor.1 + 1.0,
                false,
                None,
                None,
                Some(&sim.interner),
            )
            .is_none(),
            "a shrouded mobile leaves the band truly empty for click fallback"
        );
    }

    #[test]
    fn a_cruising_aircraft_leaves_the_ground_band() {
        // Black Eagle at the stock flight level.
        let beag = entity_with_locomotor(
            "BEAG",
            LocomotorKind::Fly,
            STOCK_FLIGHT_LEVEL_LEPTONS,
            40,
            40,
        );
        assert_eq!(entity_draw_band(&beag), EntityDrawBand::Top);
    }

    #[test]
    fn an_aircraft_on_its_pad_is_an_ordinary_ground_object() {
        // Fly answers the layer question from height alone and never looks at
        // the in-air flag, and `MovementLayer::Air` is fixed at construction
        // for the kind — so height is the only thing that may decide this.
        let parked = entity_with_locomotor("BEAG", LocomotorKind::Fly, 0, 40, 40);
        assert_eq!(
            parked.locomotor.as_ref().expect("locomotor").layer,
            MovementLayer::Air,
            "the nominal layer stays Air even parked; it must not be the predicate"
        );
        assert_eq!(entity_draw_band(&parked), EntityDrawBand::Ground);
    }

    #[test]
    fn a_hovering_jumpjet_leaves_the_ground_band_and_a_landed_one_does_not() {
        // ROCK, the Rocketeer — the one stock infantry type on a Jumpjet.
        let hovering = entity_with_locomotor("ROCK", LocomotorKind::Jumpjet, 500, 12, 12);
        let landed = entity_with_locomotor("ROCK", LocomotorKind::Jumpjet, 0, 12, 12);
        assert_eq!(entity_draw_band(&hovering), EntityDrawBand::Top);
        assert_eq!(entity_draw_band(&landed), EntityDrawBand::Ground);
    }

    #[test]
    fn ground_locomotors_never_leave_the_ground_band() {
        for kind in [
            LocomotorKind::Drive,
            LocomotorKind::Walk,
            LocomotorKind::Hover,
            LocomotorKind::Ship,
            LocomotorKind::Mech,
            LocomotorKind::Teleport,
        ] {
            let entity = entity_with_locomotor("MTNK", kind, 0, 5, 5);
            assert_eq!(
                entity_draw_band(&entity),
                EntityDrawBand::Ground,
                "{kind:?} answers Ground unconditionally in gamemd"
            );
        }
    }

    #[test]
    fn a_paradropping_infantryman_stays_in_the_ground_band() {
        // The locomotor underneath a parachute is still Walk, and Walk answers
        // Ground unconditionally — the altitude only moves where it is drawn.
        use crate::sim::entity_store::EntityStore;
        use crate::sim::movement::parachute_descent::begin_parachute_descent;

        let mut entities = EntityStore::default();
        let mut gi = GameEntity::test_default(1, "E1", "Americans", 20, 20);
        gi.category = crate::map::entities::EntityCategory::Infantry;
        gi.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        entities.insert(gi);
        assert!(begin_parachute_descent(
            &mut entities,
            1,
            SimFixed::from_num(400)
        ));
        let gi = entities.get(1).expect("entity");

        assert_eq!(entity_draw_band(gi), EntityDrawBand::Ground);
        // ...but its key still has to come off the ground row, not the row it
        // is drawn at 57 px above.
        let (_, ground_y) = ground_screen_position(&gi.position);
        let (_, drawn_y) = screen_position(gi);
        assert_eq!(ground_y - drawn_y, 57.0);
        assert_eq!(ground_sort_row(gi, drawn_y), ground_y);
    }

    /// The Rocket slot has no height test — it is a bare `return 3`. Today the
    /// object-level `rocket_state` check answers first, so this arm is only
    /// reachable if a missile is ever built locomotor-first; gating it on
    /// altitude would then silently drop a launching missile into the ground
    /// band.
    #[test]
    fn the_rocket_locomotor_is_above_the_ground_band_at_any_height() {
        let launching = entity_with_locomotor("V3ROCKET", LocomotorKind::Rocket, 0, 8, 8);
        assert!(
            launching.rocket_state.is_none(),
            "this must exercise the locomotor arm, not the rocket_state shortcut"
        );
        assert_eq!(entity_draw_band(&launching), EntityDrawBand::Top);
    }

    /// A parachute canopy takes its body's key rather than deriving one, and
    /// this is the size of the mistake if that ever regresses: a paradrop is
    /// released at the aircraft's own flight level, so a canopy keyed off the
    /// row it is *drawn* at starts a full 216 px — about 14 iso rows — away
    /// from the man hanging on it, and closes to zero only at touchdown.
    #[test]
    fn a_canopy_keyed_off_the_drawn_row_would_start_a_whole_drop_height_from_its_body() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::movement::parachute_descent::begin_parachute_descent;

        let mut entities = EntityStore::default();
        let mut gi = GameEntity::test_default(1, "E1", "Americans", 20, 20);
        gi.category = crate::map::entities::EntityCategory::Infantry;
        gi.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        entities.insert(gi);
        assert!(begin_parachute_descent(
            &mut entities,
            1,
            SimFixed::from_num(STOCK_FLIGHT_LEVEL_LEPTONS)
        ));
        let gi = entities.get(1).expect("entity");

        let (_, drawn_y) = screen_position(gi);
        assert_eq!(ground_sort_row(gi, drawn_y) - drawn_y, 216.0);
    }

    #[test]
    fn a_cruising_aircraft_sorts_on_its_own_cell_not_the_row_it_is_drawn_at() {
        let beag = entity_with_locomotor(
            "BEAG",
            LocomotorKind::Fly,
            STOCK_FLIGHT_LEVEL_LEPTONS,
            40,
            40,
        );
        let (_, ground_y) = ground_screen_position(&beag.position);
        let (_, drawn_y) = screen_position(&beag);

        // Cell (40, 40) at ground level: an entity is drawn on its diamond
        // centre, row 15*(40+40) + 15 + 15.
        assert_eq!(ground_y, 1230.0);
        // The stock flight level lifts the drawing 216 px — ~14 iso rows.
        assert_eq!(ground_y - drawn_y, 216.0);
        assert_eq!(ground_sort_row(&beag, drawn_y), ground_y);
    }

    /// What the drawn row does to a key, measured on a concrete case.
    ///
    /// A Black Eagle cruising over cell (40, 40) and a war factory five cells
    /// north-west of it, on a 5000 px-tall map. Keyed off the row it is drawn
    /// at, the aircraft's key lands *behind* the building's; keyed off its own
    /// cell it lands in front, which is what `GetYSort` — X + Y, no Z term —
    /// gives you.
    ///
    /// A cruising aircraft no longer sorts against buildings at all, because
    /// [`EntityDrawBand::Top`] draws after the whole ground pass. The case is
    /// pinned here because the same key correction carries every body that
    /// stays in the ground band while off the floor — a paradrop above all —
    /// and there the ordering against buildings is live.
    #[test]
    fn the_drawn_row_pushes_a_lifted_bodys_key_behind_a_building_it_passes() {
        const MAP_ORIGIN_Y: f32 = 0.0;
        const MAP_WORLD_HEIGHT: f32 = 5000.0;

        let beag = entity_with_locomotor(
            "BEAG",
            LocomotorKind::Fly,
            STOCK_FLIGHT_LEVEL_LEPTONS,
            40,
            40,
        );
        let (_, drawn_y) = screen_position(&beag);

        // A building's key row is its NW cell's tile row — the entity anchor
        // (15*(35+35) + 15 + 15) with the render-coordinate lift of 30/2 taken
        // back off.
        let building_row: f32 = 1065.0;
        let building_depth =
            compute_sprite_depth_params(MAP_ORIGIN_Y, MAP_WORLD_HEIGHT, building_row, 0);

        let old_depth = compute_sprite_depth_params(MAP_ORIGIN_Y, MAP_WORLD_HEIGHT, drawn_y, 0);
        let new_depth = compute_sprite_depth_params(
            MAP_ORIGIN_Y,
            MAP_WORLD_HEIGHT,
            ground_sort_row(&beag, drawn_y),
            0,
        );

        // Larger depth = further back = drawn first.
        assert!(
            old_depth > building_depth,
            "the old key drew the aircraft first, so the building covered it"
        );
        assert!(
            new_depth < building_depth,
            "the ground row draws the building first and the aircraft over it"
        );
    }
}
