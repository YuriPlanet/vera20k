//! Shared helpers for instance builders — depth sorting, visibility.
//!
//! These utilities are used by the unit, SHP, and overlay instance builders.
//! Split from `presentation::instances` to keep files under the 600-line limit.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::frame::ObjectRecord;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::map::entities::EntityCategory;
use crate::map::terrain;
use crate::render::batch::DepthAxis;
use crate::render::native_z;
use crate::sim::components::Position;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use crate::ui::game_screen::GameScreen;

/// Tactical6D8F19..6D95A9 visits the five retained Display vectors forward.
/// Store-only and Logic-only entities cannot become render/pick candidates.
/// Ground is in draw order (`Simulation::display_draw_order`) under `rules`.
pub(crate) fn tactical_entity_encounter_order(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Vec<u64> {
    display_draw_ids(sim, rules)
        .into_iter()
        .filter(|&id| sim.entities().contains(id))
        .collect()
}

/// Every Display member's id in draw order
/// (`Simulation::display_draw_order`) under `rules`.
pub(crate) fn display_draw_ids(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Vec<u64> {
    sim.display_draw_order(rules)
        .into_iter()
        .map(|(_, id)| id)
        .collect()
}

/// Drawing and screen selection share lifecycle/DrawState admission, but an
/// object's pixels may cross the shroud frontier before its anchor does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TacticalEntityPurpose {
    Drawing,
    /// Health bars and other class extras draw separately from the body.
    Extras,
    /// The selectable list (`TacticalClass::AddSelectable @ 0x006D9EF0`).
    ScreenSelection,
}

/// Phobos `IsControlledByCurrentPlayer_Wrapper` (`Ext/Techno/Hooks.cpp:626`):
/// in campaign, a human or player-controlled owner answers from the
/// `DisguiseBlinkingVisibility` flags; every other viewer uses
/// `CanTargetHouse` against the current player (observers answer true, which
/// VERA approximates with the absence of a local House). Without the
/// capability the original House50B6F0 flag stands.
fn disguise_owner_visible(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    local_owner_id: Option<InternedId>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    owner_is_allied: bool,
) -> bool {
    #[cfg(feature = "phobos")]
    if let Some(rules) = rules {
        use crate::rules::phobos_shields::AffectedHouse;
        let flags = rules.phobos_draw_globals().disguise_blinking_visibility;
        let campaign = !sim.session().game_mode_nonzero();
        let player_controlled = campaign
            && sim
                .houses()
                .get(&entity.owner())
                .is_some_and(|house| house.is_human() || house.player_control());
        return if player_controlled {
            (flags.contains(AffectedHouse::ALLIES) && owner_is_allied)
                || flags.contains(AffectedHouse::OWNER)
        } else {
            local_owner_id.is_none_or(|observer| {
                crate::sim::combat::can_target_house(
                    flags,
                    observer,
                    entity.owner(),
                    sim.houses(),
                    sim.house_alliances(),
                    sim.interner(),
                )
            })
        };
    }
    let _ = (sim, entity, local_owner_id, rules, owner_is_allied);
    sim.house_is_human_player(entity.owner())
}

/// Gather facts for the simulation-owned visual-character query without
/// retaining a second ownership, alliance, sensor or Rules state in rendering.
/// Captured into each object's record for the local player
/// (`presentation::frame`), which drawing reads.
pub(crate) fn observer_draw_context(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    local_owner: Option<&str>,
    local_owner_id: Option<InternedId>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> crate::render::draw_state::ObserverDrawContext {
    // `HouseAlliances::is_allied` answers by the interned ally matrix as the
    // names would. An observer without an id is asked about by name.
    let alliances = sim.house_alliances();
    let owner = entity.owner();
    let owner_name = sim.interner().resolve(owner);
    let observer = local_owner.map(|name| (local_owner_id, name));
    let allied = observer.is_some_and(|observer| match observer {
        (Some(id), _) => alliances.is_allied(sim.interner(), id, owner),
        (None, name) => crate::map::houses::is_allied_with(alliances.as_map(), name, owner_name),
    });
    let object = rules.and_then(|r| sim.interner().object_type(entity.type_ref(), r));
    let disguise_cell = entity
        .seldom
        .disguise
        .as_ref()
        .filter(|disguise| disguise.is_disguised())
        .and(sim.resolved_terrain())
        .map(|terrain| {
            // Presentation queries isolate Dummy and never change lockstep state.
            let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
            let cell = crate::sim::movement::ground_pose::query_object_cell(
                &cells,
                crate::sim::movement::ground_pose::position_world_coord(&entity.position),
            );
            cells.coord(cell)
        });
    let disguise_owner_visible = disguise_owner_visible(sim, entity, local_owner_id, rules, allied);
    crate::render::draw_state::ObserverDrawContext {
        owner_is_allied: allied,
        owner_is_mutually_allied: allied
            && observer.is_some_and(|observer| match observer {
                (Some(id), _) => alliances.is_allied(sim.interner(), owner, id),
                (None, name) => {
                    crate::map::houses::is_allied_with(alliances.as_map(), owner_name, name)
                }
            }),
        observer_present: local_owner_id.is_some(),
        detects_cloak: local_owner_id
            .is_some_and(|observer| sim.fog().senses_object_for_house(observer, entity)),
        cloaking_stages: rules.map_or_else(
            || crate::rules::ruleset::GeneralRules::default().cloaking_stages,
            |r| {
                object.map_or(r.general.cloaking_stages, |object| {
                    object.cloaking_stages(r.general.cloaking_stages)
                })
            },
        ),
        invisible: object.is_some_and(|obj| {
            obj.invisible
                || (entity.category() == EntityCategory::Structure && obj.invisible_in_game)
        }),
        is_campaign: !sim.session().game_mode_nonzero(),
        owner_is_current_player: sim.house_is_human_player(entity.owner()),
        disguise_owner_visible,
        disguise_cell_present: disguise_cell.is_some(),
        detects_disguise: local_owner_id
            .zip(disguise_cell)
            .is_some_and(|(observer, cell)| {
                sim.fog()
                    .detects_disguise_for_house(observer, cell.0 as u16, cell.1 as u16)
            }),
        drawn_voxel: None,
        double_thick_building: entity.category() == EntityCategory::Structure
            && object.zip(rules).is_some_and(|(obj, rules)| {
                rules
                    .art()
                    .resolve_metadata_entry(sim.interner().resolve(entity.type_ref()), &obj.image)
                    .is_some_and(|art| art.building_double_thick)
            }),
    }
}

/// The actual UnitType during UnitClass::DrawIt (73CEC0).73D29C saves
/// Type,73D2C4 temporarily substitutes a Harvester's unloading type while
/// +6D1 is set, and73D3A2 restores it. Miner owns the display-image latch.
/// Simple deployment and disguise choose art and leave the actual type alone.
pub(super) fn unit_draw_type_ref(
    entity: &ObjectRecord,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> InternedId {
    let original = entity.type_ref();
    let Some(rules) = rules else { return original };
    if entity.category() == EntityCategory::Unit
        && interner
            .object_type(original, rules)
            .is_some_and(|object| object.harvester)
        && let Some(unloading) = entity.display_type_override.image()
        && interner.object_type(unloading, rules).is_some()
    {
        unloading
    } else {
        original
    }
}

/// Unit73D2CA..73D38F dispatches on the selected type's Voxel field. The
/// original identity remains authoritative for animation, pose and lifecycle.
pub(crate) fn drawn_type_uses_voxel(
    entity: &ObjectRecord,
    type_id: &str,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> bool {
    if type_id == interner.resolve(entity.type_ref()) {
        return entity.is_voxel();
    }
    let Some(rules) = rules else {
        return entity.is_voxel();
    };
    if rules
        .terrain_object_type_case_insensitive(type_id)
        .is_some()
    {
        return rules.art().get(type_id).is_some_and(|art| art.voxel);
    }
    rules
        .object(type_id)
        .and_then(|object| rules.art().resolve_metadata_entry(type_id, &object.image))
        .map_or(entity.is_voxel(), |art| art.voxel)
}

/// Native Building6D9920 ->43CEA0 ->43D290 submits ordinary building bodies
/// independently of top/center shroud; the existing ABuffer clips their pixels.
/// Unit73B0B0 and Infantry/Aircraft5F4B10 likewise reach their original
/// DrawIt entries without querying anchor shroud, and screen selection follows
/// them; lifecycle and DrawState still own transport/cloak admission.
/// Native controls: tools/procedural_drawing_oracle/{building_reveal,entity_reveal}.md.
pub(crate) fn tactical_entity_admission(
    purpose: TacticalEntityPurpose,
    entity: &ObjectRecord,
    local_owner_id: Option<InternedId>,
    fog: &FogState,
    ignore_visibility: bool,
    current_frame: u32,
    remap_row: u32,
    observer: crate::render::draw_state::ObserverDrawContext,
) -> Option<crate::render::draw_state::DrawDecision> {
    // AddSelectable (0x006D9EF0) tests no shroud, and the object loop
    // (0x006D8DB0) reaches a Techno's DrawIt, which registers it (UnitClass at
    // 0x0073CF6B), with no shroud test either: its FogOfWar test,
    // MapClass::IsFogged 0x005865E0, always answers no. The shroud only masks
    // the pixels, so screen selection takes what the draw takes.
    if (entity.category() == EntityCategory::Structure && !entity.lifecycle.object_alive())
        // Tactical6D9985..6D9996 skips the live building while its frozen
        // FoggedObjectClass is drawn by the separate4D1890 base pass.
        || (purpose != TacticalEntityPurpose::ScreenSelection
            && entity.category() == EntityCategory::Structure
            && !ignore_visibility
            && local_owner_id
                .is_some_and(|viewer| fog.is_object_fogged(viewer, entity.stable_id())))
        // BuildingClass::DrawIt (gamemd 0x0043D308..0x0043D316, RA2 game.exe
        // 0x0043A658..0x0043A666) skips the body even for the lamp's owner.
        || (purpose == TacticalEntityPurpose::Drawing
            && entity.place.structure.is_some_and(|art| art.invisible_in_game))
        // Unit DrawIt 73D30E skips both voxel and SHP bodies in a tube;
        // Infantry DrawIt 5190DE branches to AddSelectable at 519609.
        || (purpose == TacticalEntityPurpose::Drawing && entity.in_tube)
        // VERA: a client module's `Decoration` with `HideAnchor` hides the
        // body on this screen; it is still listed and selected.
        || (purpose == TacticalEntityPurpose::Drawing && entity.local_look().hidden)
        || entity.lifecycle.in_limbo()
        || entity.passenger_role.is_inside_transport()
        // UnitClass::DrawIt returns before AddSelectable while the unit
        // deploys or undeploys (+0x6E1 / +0x6E2, 0x0073CF46..0x0073CF5C); its
        // Extras still draw (0x006D9789), so only the list leaves it out.
        // RESIDUAL (dormant, no retail rules use drop pods): InfantryClass::
        // DrawIt also skips AddSelectable for an infantry still falling by
        // DropPod locomotor (0x0051913B..0x0051928C); VERA lists it.
        || (purpose == TacticalEntityPurpose::ScreenSelection
            && entity.category() == EntityCategory::Unit
            && entity.unit_deploying())
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
pub(crate) fn tactical_screen_entity_encounter_order(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Vec<u64> {
    tactical_bounded_entity_encounter_order(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
        false,
    )
}

/// Native band-box caught-anything source: render-tracked mobiles plus the
/// live building array registered in bulk without a shroud test.
pub(crate) fn tactical_band_preflight_entity_encounter_order(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> Vec<u64> {
    tactical_bounded_entity_encounter_order(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
        true,
    )
}

fn tactical_bounded_entity_encounter_order(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    persistence: &PersistenceState,
    bulk_register_live_buildings: bool,
) -> Vec<u64> {
    let Some(sim) = match_state
        .sim_runtime
        .presentation()
        .map(|rt| &rt.simulation)
    else {
        return Vec::new();
    };
    let local_owner = crate::app::input::commands::preferred_local_owner_name(match_state);
    // Screen selection takes what the battlefield shows.
    let Some(frame) = match_state.sim_runtime.frame() else {
        return Vec::new();
    };
    let zoom = match_state.input.zoom_level.max(f32::EPSILON);
    // RESIDUAL: the 32 pixels are `TacticalClass::AddSelectable @ 0x006D9EF0`'s,
    // for mobiles. `0x006D9CE0` first clips a building's anchor to the tactical
    // rectangle Draw passes (0x00886FA0, `0x006D45F2..0x006D461A`), so natively a
    // building just outside the view is never registered; here it is, and a click
    // at the view's edge can reach it through its anchor rather than its cell.
    let margin = 32.0 / zoom;
    // The world rectangle the battlefield shows: from the camera, the
    // frame's corner, in by the battlefield's own corner.
    let (left_px, top_px, width_px, height_px) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        screen,
        &match_state.match_presentation,
        persistence,
    );
    let left = match_state.input.camera_x + left_px as f32 / zoom;
    let top = match_state.input.camera_y + top_px as f32 / zoom;
    let min_x = left - margin;
    let min_y = top - margin;
    let max_x = left + width_px as f32 / zoom + margin;
    let max_y = top + height_px as f32 / zoom + margin;
    let local_owner_id = local_owner
        .as_deref()
        .and_then(|owner| sim.interner().get(owner));

    compose_tactical_screen_entity_encounter_order(
        sim,
        &frame,
        (min_x, min_y, max_x, max_y),
        local_owner_id,
        sim.fog(),
        match_state.sandbox_full_visibility,
        sim.session().binary_frame(),
        bulk_register_live_buildings,
    )
}

fn compose_tactical_screen_entity_encounter_order(
    sim: &crate::sim::world::Simulation,
    frame: &crate::app::presentation::frame::PresentationFrame,
    bounds: (f32, f32, f32, f32),
    local_owner_id: Option<InternedId>,
    fog: &FogState,
    ignore_visibility: bool,
    current_frame: u32,
    bulk_register_live_buildings: bool,
) -> Vec<u64> {
    let (min_x, min_y, max_x, max_y) = bounds;
    // RESIDUAL (kept by the maintainer, for big battles): AddSelectable
    // (0x006D9EF0) refuses entries past 500, so with more objects in view the
    // band box and the T key's screen pass reach only the first 500 drawn.
    // VERA registers all.
    let mut candidates: Vec<u64> = frame
        .display_order()
        .ordered()
        .iter()
        .map(|&(_, id)| id)
        .filter(|&id| frame.contains(id))
        .collect();
    // Band-box preflight has a separate native bulk Building registration
    // path. Ordinary picking/rendering never scans the unregistered store.
    if bulk_register_live_buildings {
        candidates.extend(frame.structures().filter_map(|entity| {
            sim.display_layers()
                .layer_of(entity.stable_id())
                .is_none()
                .then_some(entity.stable_id())
        }));
    }
    candidates
        .into_iter()
        .filter(|id| {
            frame.object(*id).is_some_and(|entity| {
                let admitted = if bulk_register_live_buildings
                    && entity.category() == EntityCategory::Structure
                {
                    entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo()
                } else {
                    tactical_entity_admission(
                        TacticalEntityPurpose::ScreenSelection,
                        entity,
                        local_owner_id,
                        fog,
                        ignore_visibility,
                        current_frame,
                        0,
                        entity.seen.observer,
                    )
                    .is_some()
                };
                if !admitted {
                    return false;
                }
                // `entity_pick::PresentedWorld::screen_position`.
                let [x, y] = entity.pose.screen;
                let y = y - f32::from(entity.ion_blast_lift_pixels());
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

/// Presentation buckets for retained native Display membership.
/// Air and Top retain their distinct native Display vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityDrawBand {
    Ground,
    Air,
    Top,
}

/// Tactical6D8F19..6D95A9 draws the Display members' registered layers, not
/// a fresh GetLayer query. Height/locomotor changes take effect here only after their
/// simulation-owned remove/submit transaction (Fly phase4CD2A0, for example).
/// Unregistered objects and layers without an entity body stream emit nothing.
pub(crate) fn layer_draw_band(
    layer: crate::sim::world::display_layers::DisplayLayer,
) -> Option<EntityDrawBand> {
    use crate::sim::world::display_layers::DisplayLayer;
    match layer {
        DisplayLayer::GROUND => Some(EntityDrawBand::Ground),
        DisplayLayer::AIR => Some(EntityDrawBand::Air),
        DisplayLayer::TOP => Some(EntityDrawBand::Top),
        _ => None,
    }
}

/// The Display's entities that draw a body, in Tactical's order, with their bands.
pub(crate) fn drawn_display_entities(
    frame: &crate::app::presentation::frame::PresentationFrame,
) -> impl Iterator<
    Item = (
        &crate::app::presentation::frame::ObjectRecord,
        EntityDrawBand,
    ),
> {
    frame
        .display_order()
        .ordered()
        .iter()
        .filter_map(|&(layer, id)| Some((frame.object(id)?, layer_draw_band(layer)?)))
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
pub(crate) fn ground_sort_row(entity: &ObjectRecord, drawn_row_y: f32) -> f32 {
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
pub(crate) fn compute_sprite_depth(
    presentation: &MatchPresentationState,
    screen_y: f32,
    z: u8,
) -> f32 {
    let axis = depth_axis(presentation);
    compute_sprite_depth_params(axis.origin_y, axis.world_height, screen_y, z)
}

/// The normalised depth axis of the loaded map (`DepthAxis::NONE` outside a
/// match): what `BatchRenderer::update_camera` uploads for the shaders.
pub(crate) fn depth_axis(presentation: &MatchPresentationState) -> DepthAxis {
    presentation
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
/// origin_y and world_height (avoids re-extracting them from the presentation).
pub(crate) fn compute_sprite_depth_params(
    origin_y: f32,
    world_height: f32,
    screen_y: f32,
    z: u8,
) -> f32 {
    let iso_row: f32 = screen_y + f32::from(z as i8) * terrain::HEIGHT_STEP;
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

/// Is `owner`'s object at `pos` shown to the local player? The one rule
/// VERA's translucent fog and input picking (TypeSelect's screen stage,
/// translucent pick concealment) share; callers keep their own
/// full-visibility switch. Screen selection and the band preflight take no
/// shroud test (`TacticalClass::AddSelectable @ 0x006D9EF0`).
///
/// Without a known local player everything is shown. The local player's
/// allies, as its house counts them, always are (VERA's policy, with no native
/// ally test). Anyone else's object needs an explored anchor cell no hostile
/// gap covers; `current_sight_only` (translucent fog under FogOfWar) also
/// needs the viewer's current sight there.
pub(crate) fn object_shown_to_local_owner(
    local_owner: Option<&str>,
    local_owner_id: Option<InternedId>,
    fog: &FogState,
    alliances: &crate::map::houses::HouseAllianceMap,
    pos: &Position,
    owner: &str,
    current_sight_only: bool,
) -> bool {
    let Some(local_owner) = local_owner else {
        return true;
    };
    crate::map::houses::is_allied_with(alliances, local_owner, owner)
        || fog.shows_objects_to(
            local_owner_id.unwrap_or_default(),
            pos.rx(),
            pos.ry(),
            current_sight_only,
        )
}

/// VERA's translucent fog removes the A buffer's native concealment, so what
/// it used to black out must leave the draw plan instead. Terrain stays
/// visible, but an enemy cell shows no object until the viewer actually sees
/// it; friendly and allied objects always draw.
pub(crate) fn translucent_fog_conceals_cell(
    viewer: Option<InternedId>,
    fog: Option<&FogState>,
    rx: u16,
    ry: u16,
    fog_of_war: bool,
) -> bool {
    let (Some(viewer), Some(fog)) = (viewer, fog) else {
        return false;
    };
    !fog.shows_objects_to(viewer, rx, ry, fog_of_war)
}

/// The local house VERA's translucent fog conceals other houses' unseen
/// objects from, or `None` under the retail fog or full visibility. The draw
/// plan and the tactical pickers ([`crate::app::input::entity_pick::presented`])
/// both read it, so whatever the fog takes out of the plan no pick finds.
pub(crate) fn translucent_fog_viewer(
    platform: &PlatformState,
    match_state: &MatchState,
    sim: &crate::sim::world::Simulation,
) -> Option<InternedId> {
    if !platform.translucent_fog_enabled() || match_state.sandbox_full_visibility {
        return None;
    }
    let name = crate::app::input::commands::preferred_local_owner_name(match_state)?;
    sim.interner().get(&name)
}

/// [`object_shown_to_local_owner`] as VERA's translucent fog applies it to
/// one live object: everyone but the viewer's allies follows the viewer's
/// sight at the anchor cell, current sight under FogOfWar.
///
/// Native draws such an object and lets the A buffer darken it: the IsFogged
/// gates of `Tactical_ObjectRenderingLoop @ 0x006D8DB0` (`0x006D8FDA`,
/// `0x006D920E`, `0x006D929F`, `0x006D965D`) call `MapClass::IsFogged @
/// 0x005865E0`, which is `xor al, al; ret 4`. Its click stays pickable too:
/// `DisplayClass::ProcessClickCoords @ 0x00692300` refuses the pick under
/// FogOfWar (`0x006924A4..0x006924FA`) only for a fogged Building (`+0x6E7`)
/// on the clicked cell, and otherwise asks that same stub.
pub(crate) fn translucent_fog_conceals(
    sim: &crate::sim::world::Simulation,
    viewer: InternedId,
    position: &Position,
    owner: InternedId,
) -> bool {
    let interner = sim.interner();
    !object_shown_to_local_owner(
        Some(interner.resolve(viewer)),
        Some(viewer),
        sim.fog(),
        sim.house_alliances().as_map(),
        position,
        interner.resolve(owner),
        sim.session().game_options().fog_of_war,
    )
}

/// Enemy weapon projectiles and their trails are hidden under the same rule.
/// A bullet without a live source object is treated as hostile, so a stray
/// tracer cannot outline an unseen firing position.
pub(crate) fn translucent_fog_conceals_bullet(
    sim: &crate::sim::world::Simulation,
    frame: &crate::app::presentation::frame::PresentationFrame,
    bullet: &crate::sim::projectile::Projectile,
    coord: [i32; 3],
    local_owner: Option<&str>,
    viewer: Option<InternedId>,
    fog_of_war: bool,
) -> bool {
    let source_owner = frame
        .object(bullet.source_id())
        .map(|entity| sim.interner().resolve(entity.owner()));
    let allied = source_owner.is_some_and(|owner| {
        local_owner.is_some_and(|local| {
            crate::map::houses::is_allied_with(sim.house_alliances().as_map(), local, owner)
        })
    });
    if allied {
        return false;
    }
    let cell = lepton_cell(coord[0], coord[1]);
    translucent_fog_conceals_cell(viewer, Some(sim.fog()), cell.0, cell.1, fog_of_war)
}

/// World-lepton coordinates to their map cell, clamped to the addressable
/// cell range.
pub(crate) fn lepton_cell(x: i32, y: i32) -> (u16, u16) {
    (
        (x / 256).clamp(0, i32::from(u16::MAX)) as u16,
        (y / 256).clamp(0, i32::from(u16::MAX)) as u16,
    )
}

/// `[General]` superweapon effect animations keep drawing over unseen cells:
/// these are the cinematic effects the fog must not swallow.
pub(crate) fn is_superweapon_effect_anim(
    name: &str,
    general: &crate::rules::ruleset::GeneralRules,
) -> bool {
    name.eq_ignore_ascii_case("NUKEBALL")
        || name.eq_ignore_ascii_case(&general.chrono_placement_anim)
        || name.eq_ignore_ascii_case(&general.chrono_blast_anim)
        || name.eq_ignore_ascii_case(&general.chrono_blast_dest_anim)
        || name.eq_ignore_ascii_case(&general.dominator_first_anim)
        || name.eq_ignore_ascii_case(&general.dominator_second_anim)
        || name.eq_ignore_ascii_case(&general.iron_curtain_invoke_anim)
        || name.eq_ignore_ascii_case(&general.force_shield_invoke_anim)
        || name.eq_ignore_ascii_case(&general.weather_con_bolt_explosion)
        || general
            .weather_con_clouds
            .iter()
            .any(|anim| name.eq_ignore_ascii_case(anim))
        || general
            .weather_con_bolts
            .iter()
            .any(|anim| name.eq_ignore_ascii_case(anim))
}

pub(crate) fn cell_visibility_for_local_owner(
    local_owner_id: Option<InternedId>,
    fog: Option<&FogState>,
    rx: u16,
    ry: u16,
    ignore_visibility: bool,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
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
    // Display::ProcessClickCoords69244C..692480 asks Map578080 for the
    // clicked Cell centre's floor, then IsShrouded586360. The shroud lattice
    // is flat: raw map coordinates reject visible high-ground targets.
    // CellGetCoords486840 has the same floor input and no bridge-deck term.
    // Executed suffix/Pick controls: spatial_oracle/click_shroud.json.
    // Standard YR has FogOfWar=false; current sight is not an extra gate.
    let shrouded = terrain.map_or_else(
        || !fog.is_cell_revealed(local_owner_id, rx, ry),
        |terrain| {
            crate::sim::vision::cell_is_shrouded(
                fog,
                &crate::map::resolved_terrain::NativeCellQuery::isolated(terrain),
                local_owner_id,
                (rx, ry),
            )
        },
    );
    if !shrouded {
        CellVisibilityState::Visible
    } else {
        CellVisibilityState::Shrouded
    }
}

/// The battlefield every Display member is admitted in, in world pixels: its
/// corner, Tactical's ViewOrigin (+0xB0/+0xB4), and its size, the tactical
/// rect 0xB0CE28's width and height. Set_View_Dimensions4A89B8 /
/// Scenario687620 copy that rect through6D5F60; the sidebar and bottom strip
/// are outside it, so a layout that sets the battlefield in from the frame's
/// corner moves this corner in from the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TacticalAdmission {
    pub(crate) corner: [f32; 2],
    pub(crate) size: [f32; 2],
}

impl TacticalAdmission {
    /// The battlefield `origin` in from the corner of a frame whose corner is
    /// the world point `camera`, and `size` across, all in world pixels.
    pub(crate) fn at(camera: [f32; 2], origin: [f32; 2], size: [f32; 2]) -> Self {
        Self {
            corner: [camera[0] + origin[0], camera[1] + origin[1]],
            size,
        }
    }

    /// The battlefield at `tactical` (`[x, y, width, height]` in frame
    /// pixels) of a frame whose corner is the world point `camera`, at `zoom`
    /// (`camera::tactical_origin_world`).
    pub(crate) fn of(camera: [f32; 2], tactical: [u32; 4], zoom: f32) -> Self {
        let [x, y, width, height] = tactical.map(|v| v as f32 / zoom);
        Self::at(camera, [x, y], [width, height])
    }

    /// This draw's battlefield (`camera::tactical_viewport_px`).
    pub(crate) fn of_state(
        platform: &PlatformState,
        renderer: &RendererState,
        screen: &GameScreen,
        match_state: &MatchState,
        persistence: &PersistenceState,
    ) -> Self {
        let input = &match_state.input;
        let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(
            platform,
            renderer,
            screen,
            &match_state.match_presentation,
            persistence,
        );
        Self::of(
            [input.camera_x, input.camera_y],
            [x, y, width, height],
            input.zoom_level,
        )
    }
}

/// Object5F4B10 -> CoordsToClient2 6D2140 admits this inclusive padding before
/// invoking a body draw: `point`, in world pixels, measured from `view`'s
/// corner. Native Unit boundaries and retained sinking history:
/// naval_sinking_clip.py.
pub(in crate::app::presentation) fn projection_admitted(
    point: [f32; 2],
    view: TacticalAdmission,
) -> bool {
    let [x, y] = [point[0] - view.corner[0], point[1] - view.corner[1]];
    x >= -360.0 && x <= view.size[0] + 360.0 && y >= -180.0 && y <= view.size[1] + 180.0
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

/// The box a Display record's span must reach before the object loop hands
/// the record to the unit and SHP visitors: every view they admit from (the
/// battlefield the unit visitor takes, which the centring size from the
/// camera covers; the render size; the building clip), widened by the widest
/// margin either uses ([`projection_admitted`]'s 360 px) and by
/// [`super::shp::BUILDING_SLOT_WINDOW_SLACK`] for building art and aircraft
/// shadows. A record outside it plans nothing in either.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PassByWindow {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl PassByWindow {
    const MARGIN: f32 = 360.0 + super::shp::BUILDING_SLOT_WINDOW_SLACK;

    /// The window around `views`, each `[left, top, right, bottom]` in
    /// world pixels.
    pub(crate) fn around(views: impl IntoIterator<Item = [f32; 4]>) -> Self {
        let [left, top, right, bottom] = views.into_iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |[left, top, right, bottom], view| {
                [
                    left.min(view[0]),
                    top.min(view[1]),
                    right.max(view[2]),
                    bottom.max(view[3]),
                ]
            },
        );
        Self {
            left: left - Self::MARGIN,
            top: top - Self::MARGIN,
            right: right + Self::MARGIN,
            bottom: bottom + Self::MARGIN,
        }
    }

    /// The window of this draw: `building_view` is the SHP visitor's
    /// building clip, when it runs.
    pub(crate) fn of(
        platform: &PlatformState,
        renderer: &RendererState,
        screen: &GameScreen,
        match_state: &MatchState,
        persistence: &PersistenceState,
        building_view: Option<crate::app::presentation::target_lines::TacticalViewport>,
    ) -> Self {
        let input = &match_state.input;
        let camera = [input.camera_x, input.camera_y];
        let zoom = input.zoom_level;
        let (centring_width, centring_height) = crate::app::input::camera::tactical_centring_size(
            platform,
            renderer,
            screen,
            &match_state.match_presentation,
            persistence,
        );
        let from_camera = |width: u32, height: u32| {
            [
                camera[0],
                camera[1],
                camera[0] + width as f32 / zoom,
                camera[1] + height as f32 / zoom,
            ]
        };
        let buildings = building_view.map(|view| {
            let [x, y] = view.camera.map(|v| v as f32);
            [x, y, x + view.clip[2] as f32, y + view.clip[3] as f32]
        });
        Self::around(
            [
                from_camera(centring_width, centring_height),
                from_camera(
                    renderer.render_width(screen),
                    renderer.render_height(screen),
                ),
            ]
            .into_iter()
            .chain(buildings),
        )
    }

    /// Whether `span` reaches the window.
    pub(crate) fn reaches(
        &self,
        span: crate::app::presentation::display_members::ScreenSpan,
    ) -> bool {
        span.x >= self.left
            && span.x <= self.right
            && span.bottom >= self.top
            && span.top <= self.bottom
    }
}

#[cfg(all(test, tests_app_presentation_instances))]
mod tests {
    use super::*;
    use crate::render::locomotor_visual::screen_position;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;

    #[test]
    fn a_level_below_zero_keeps_its_ground_row() {
        // Raw 0xFF is Level -1 on a terrain-deformation crater floor: drawn a
        // level lower, on the same ground row as Level 0 a level higher.
        assert_eq!(
            compute_sprite_depth_params(0.0, 1000.0, 115.0, 0xFF),
            compute_sprite_depth_params(0.0, 1000.0, 100.0, 0)
        );
    }

    /// Stock YR sets one global `FlightLevel=1500` for every aircraft.
    const STOCK_FLIGHT_LEVEL_LEPTONS: i32 = 1500;

    #[test]
    fn band_selection_before_drawing_uses_the_lent_frames_display_order() {
        use crate::app::match_runtime::sim_host::SimHost;
        use crate::sim::runtime::SimRuntime;
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let tank = sim.intern("MTNK");
        for id in [2, 1] {
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                10,
                10,
                0,
                0,
                owner,
                crate::sim::combat::Health::new(100),
                tank,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.lifecycle.set_in_limbo_for_test(true);
            sim.entities_mut_for_test().insert(entity);
            sim.reveal(id);
        }

        let mut host = SimHost::default();
        host.install(Some(SimRuntime::from_simulation(sim)));
        assert!(host.lend(Box::new(|_| Box::new(()))));
        let sim = &host.presentation().unwrap().simulation;
        let frame = host.frame().unwrap();
        let bounds = (-10_000.0, -10_000.0, 10_000.0, 10_000.0);

        // Box-release preflight can be the first consumer of this frame.
        let candidates = compose_tactical_screen_entity_encounter_order(
            sim,
            frame,
            bounds,
            Some(owner),
            sim.fog(),
            true,
            0,
            true,
        );
        assert_eq!(candidates, [2, 1]);
        let drawn: Vec<_> = frame
            .display_members(sim)
            .objects()
            .iter()
            .map(|member| member.id)
            .collect();
        assert_eq!(drawn, candidates);
        assert!(host.take_job_result().is_some());
    }

    #[test]
    fn item83_equal_tactical_keys_keep_live_registration_order() {
        let mut sim = crate::sim::world::Simulation::new();
        *sim.fog_mut_for_test() = FogState::new(32, 32);
        let owner = sim.intern("Americans");
        let type_ref = sim.intern("E1");
        // Stored entities use this Simulation's interner, not the separate
        // global test interner used by standalone rendering-only fixtures.
        let entity = |id| {
            GameEntity::new_at_frame_zero_for_test(
                id,
                10,
                10,
                0,
                0,
                owner,
                crate::sim::combat::Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            )
        };
        sim.entities_mut_for_test().insert(entity(1));
        sim.entities_mut_for_test().insert(entity(2));
        sim.entity_mut_for_test(2)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        sim.entity_mut_for_test(1)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        sim.reveal(2);
        sim.reveal(1);
        sim.set_logic_order_for_test(vec![1, 2]);
        assert_eq!(tactical_entity_encounter_order(&sim, None), [2, 1]);
        sim.entities_mut_for_test().insert(entity(3));
        sim.set_logic_order_for_test(vec![3, 1, 2]);
        assert_eq!(
            tactical_entity_encounter_order(&sim, None),
            [2, 1],
            "Logic/store membership is insufficient"
        );
        sim.conceal(2);
        assert_eq!(
            tactical_entity_encounter_order(&sim, None),
            [1],
            "conceal removes the pick/render candidate"
        );
        sim.reveal(2);
        assert_eq!(
            tactical_entity_encounter_order(&sim, None),
            [1, 2],
            "resubmission follows retained equal-key members"
        );
    }

    #[test]
    fn owned_invisible_lamp_keeps_its_light_without_drawing_its_body() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = retail.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 43);
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.resolve_type_handles(&rules);
        for (x, name, draws_body) in [(10, "GALITE", true), (12, "INYELWLAMP", false)] {
            let id = sim
                .spawn_object(name, "Americans", x, 10, 0, &rules)
                .unwrap();
            let building = ObjectRecord::of_test_sim(&sim, &rules, id);
            assert!(
                sim.entities()
                    .get(id)
                    .unwrap()
                    .seldom
                    .discovery
                    .owned_by_current_house()
            );
            let body = tactical_entity_admission(
                TacticalEntityPurpose::Drawing,
                &building,
                Some(owner),
                sim.fog(),
                false,
                sim.session().binary_frame(),
                0,
                building.seen.observer,
            );
            assert_eq!(body.is_some(), draws_body, "{name} building sprite");
            assert!(
                sim.lighting_sources().buildings()[&id].active,
                "{name} light"
            );
        }
    }

    #[test]
    fn ordinary_building_admission_matches_executed_native_controls() {
        use crate::sim::combat::Health;

        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/building_reveal.json",
        ))
        .unwrap();
        let top: [u16; 2] = std::array::from_fn(|i| {
            u16::try_from(native["native_top_cell"][i].as_u64().unwrap()).unwrap()
        });
        let center: [u16; 2] = std::array::from_fn(|i| {
            u16::try_from(native["native_center_cell"][i].as_u64().unwrap()).unwrap()
        });
        // The record captures through the test interner.
        let local = crate::sim::intern::test_intern("Americans");
        let owner = crate::sim::intern::test_intern("Neutral");
        let type_id =
            crate::sim::intern::test_intern(native["physical_type"]["type_id"].as_str().unwrap());
        let mut compared = 0;
        for row in native["admission_cases"].as_array().unwrap() {
            let input = &row["input"];
            // Enabled FogOfWar snapshots are a separate lifecycle. Geometry
            // clipping belongs to the existing builders, beyond this gate.
            if input.get("is_fogged").is_some() || input.get("rectangle").is_some() {
                continue;
            }
            let mut fog = FogState::new(32, 32);
            fog.mark_visible_for_owner(local, 1, 1);
            for (cell, key) in [(top, "top_revealed"), (center, "center_revealed")] {
                if input[key].as_bool().unwrap_or(false) {
                    fog.mark_visible_for_owner(local, cell[0], cell[1]);
                }
            }
            let mut building = GameEntity::new_at_frame_zero_for_test(
                7,
                top[0],
                top[1],
                0,
                0,
                owner,
                Health::new(100),
                type_id,
                EntityCategory::Structure,
                0,
                0,
                false,
            );
            building
                .lifecycle
                .set_object_alive_for_test(input["active"].as_bool().unwrap_or(true));
            building
                .lifecycle
                .set_in_limbo_for_test(input["limbo"].as_bool().unwrap_or(false));
            let coord = crate::sim::movement::ground_pose::position_world_coord(&building.position);
            assert_eq!(
                serde_json::json!([coord.x, coord.y, coord.z]),
                native["object_coords"]
            );
            let building = ObjectRecord::of_test(&building);
            let admitted = |purpose| {
                tactical_entity_admission(
                    purpose,
                    &building,
                    Some(local),
                    &fog,
                    false,
                    0,
                    0,
                    crate::render::draw_state::ObserverDrawContext::default(),
                )
                .is_some()
            };
            let expected = row["base_draw_called"].as_bool().unwrap();
            assert_eq!(
                admitted(TacticalEntityPurpose::Drawing),
                expected,
                "{input}"
            );
            // AddSelectable (0x006D9EF0) tests no shroud.
            assert_eq!(
                admitted(TacticalEntityPurpose::ScreenSelection),
                expected,
                "screen selection follows the draw: {input}",
            );
            compared += 1;
        }
        assert_eq!(
            compared, 6,
            "all ordinary visibility/lifecycle controls must run"
        );
    }

    #[test]
    fn a_foot_in_a_tube_skips_its_body_and_keeps_its_selectable_entry() {
        use crate::map::tube_facts::TubeId;
        use crate::sim::components::DriveCoord;
        use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

        let fog = FogState::new(8, 8);
        for (kind, category) in [
            ("MTNK", EntityCategory::Unit),
            ("E1", EntityCategory::Infantry),
        ] {
            let mut actor =
                GameEntity::test_default_of_category(1, kind, "Americans", 2, 2, category);
            actor.lifecycle.set_in_limbo_for_test(false);
            for inside in [false, true] {
                actor.low_bridge_tube_state = inside.then(|| {
                    LowBridgeTubeMovementState::for_test(TubeId(0), 1, DriveCoord::cell(3, 2, 0))
                });
                let record = ObjectRecord::of_test(&actor);
                let admitted = |purpose| {
                    tactical_entity_admission(
                        purpose,
                        &record,
                        Some(actor.owner()),
                        &fog,
                        true,
                        0,
                        0,
                        Default::default(),
                    )
                    .is_some()
                };
                assert_eq!(
                    admitted(TacticalEntityPurpose::Drawing),
                    !inside,
                    "{kind} body"
                );
                assert!(admitted(TacticalEntityPurpose::Extras), "{kind} extras");
                assert!(
                    admitted(TacticalEntityPurpose::ScreenSelection),
                    "{kind} selection"
                );
            }
        }
    }

    #[test]
    fn shroud_pop_in_mobile_admission_matches_original_class_entries() {
        use crate::sim::combat::Health;

        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/entity_reveal.json",
        ))
        .unwrap();
        // The record captures through the test interner.
        let local = crate::sim::intern::test_intern("Americans");
        let owner = crate::sim::intern::test_intern("Neutral");
        let type_id = crate::sim::intern::test_intern("prepared-native-mobile");
        let mut compared = 0;
        for row in native["admission_cases"].as_array().unwrap() {
            let input = &row["input"];
            // Redraw scheduling and camera clipping precede/consume this
            // shared gate; their native controls do not invent entity fields.
            if input.get("redraw").is_some()
                || input.get("camera").is_some()
                || input.get("marked").is_some()
            {
                continue;
            }
            let category = match row["class_name"].as_str().unwrap() {
                "unit" => EntityCategory::Unit,
                "infantry" => EntityCategory::Infantry,
                "aircraft" => EntityCategory::Aircraft,
                other => panic!("unexpected native class {other}"),
            };
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                7,
                10,
                20,
                0,
                0,
                owner,
                Health::new(100),
                type_id,
                category,
                0,
                0,
                true,
            );
            entity
                .lifecycle
                .set_object_alive_for_test(input["alive"].as_bool().unwrap_or(true));
            entity
                .lifecycle
                .set_in_limbo_for_test(input["limbo"].as_bool().unwrap_or(false));
            // Native fixture supplies its coordinate independently of class
            // construction; match it rather than testing Infantry's default slot.
            entity
                .position
                .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(
                    native["object_coords"][0].as_i64().unwrap().rem_euclid(256),
                ));
            entity
                .position
                .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(
                    native["object_coords"][1].as_i64().unwrap().rem_euclid(256),
                ));
            let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            assert_eq!(
                serde_json::json!([coord.x, coord.y, coord.z]),
                native["object_coords"]
            );
            let mut fog = FogState::new(32, 32);
            fog.mark_visible_for_owner(local, 1, 1);
            // Only raw native flag controls0/0x18 map to the ordinary Rust
            // unrevealed/revealed inputs. Partial0x08 is not a reveal producer.
            if input["cell_bits"].as_u64() == Some(0x08) {
                continue;
            }
            let revealed = input["cell_bits"].as_u64() == Some(0x18);
            if revealed {
                fog.mark_visible_for_owner(local, 10, 20);
            }
            let entity = ObjectRecord::of_test(&entity);
            let admitted = |purpose| {
                tactical_entity_admission(
                    purpose,
                    &entity,
                    Some(local),
                    &fog,
                    false,
                    0,
                    0,
                    crate::render::draw_state::ObserverDrawContext::default(),
                )
                .is_some()
            };
            let expected = row["body_admitted"].as_bool().unwrap();
            assert_eq!(admitted(TacticalEntityPurpose::Drawing), expected, "{row}");
            assert_eq!(
                admitted(TacticalEntityPurpose::ScreenSelection),
                expected,
                "a shrouded body still registers: {row}"
            );
            compared += 1;
        }
        assert_eq!(compared, 12);
    }

    #[test]
    fn undeployable_building_band_uses_registered_screen_candidates() {
        use crate::app::input::entity_pick::compute_box_selection_snapshot;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch;

        let Some(retail) = retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &IniFile::from_str("[GAPILL]\nUndeploysInto=AMCV\n[GAPOWR]\nUndeploysInto=AMCV\n"),
        ) else {
            return;
        };
        let rules = &retail.rules;
        assert!(rules.object("GAPILL").unwrap().is_1x1_with_undeploy());
        assert!(!rules.object("GAPOWR").unwrap().is_1x1_with_undeploy());
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let pill = sim
            .spawn_object("GAPILL", "Americans", 16, 16, 0, rules)
            .unwrap();
        let power = sim
            .spawn_object("GAPOWR", "Americans", 20, 16, 0, rules)
            .unwrap();
        for id in [pill, power] {
            assert!(sim.display_layers().layer_of(id).is_some());
        }
        let bounds = (-1.0e6, -1.0e6, 1.0e6, 1.0e6);
        let frame = crate::app::presentation::frame::PresentationFrame::of_test(&sim, rules);
        let order = |bulk| {
            compose_tactical_screen_entity_encounter_order(
                &sim,
                &frame,
                bounds,
                Some(owner),
                sim.fog(),
                false,
                0,
                bulk,
            )
        };
        let ordinary = order(false);
        let preflight = order(true);
        for id in [pill, power] {
            assert!(ordinary.contains(&id), "registered local building {id}");
            assert!(preflight.contains(&id));
        }
        let mutation = compute_box_selection_snapshot(
            sim.entities(),
            &preflight,
            &ordinary,
            &[],
            Some((sim.fog(), sim.house_alliances().as_map())),
            Some("Americans"),
            bounds.0,
            bounds.1,
            bounds.2,
            bounds.3,
            false,
            Some(rules),
            Some(sim.houses()),
            Some(sim.interner()),
        )
        .expect("registered buildings consume the band");
        // Building459C00 calls465D40 before Techno6FC030: Undeploy alone
        // does not admit the larger power plant's fixed-ART foundation.
        assert_eq!(mutation.select, [pill]);
        assert!(mutation.clear);
    }

    /// UnitClass::DrawIt returns before AddSelectable while the unit deploys
    /// (0x0073CF46..0x0073CF5C), so the band box and the T key miss it.
    #[test]
    fn a_deploying_unit_is_not_screen_selectable() {
        use crate::sim::combat::Health;

        let mut sim = crate::sim::world::Simulation::new();
        let owner = sim.intern("Americans");
        let typ = sim.intern("SCHP");
        for id in [1, 2] {
            let mut unit = GameEntity::new_at_frame_zero_for_test(
                id,
                10 + id as u16,
                10,
                0,
                0,
                owner,
                Health::new(100),
                typ,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            unit.lifecycle.set_in_limbo_for_test(true);
            sim.entities_mut_for_test().insert(unit);
            sim.reveal(id);
        }
        sim.entity_mut_for_test(2)
            .unwrap()
            .set_unit_simple_deploy_for_test(false, true, false);
        let frame = crate::app::presentation::frame::PresentationFrame::of_test(
            &sim,
            crate::rules::ruleset::RuleSet::empty_for_test(),
        );
        let order = compose_tactical_screen_entity_encounter_order(
            &sim,
            &frame,
            (-1.0e6, -1.0e6, 1.0e6, 1.0e6),
            Some(owner),
            sim.fog(),
            true,
            0,
            false,
        );
        assert_eq!(order, [1]);
    }

    #[test]
    fn band_preflight_catches_shrouded_enemies() {
        use crate::app::input::entity_pick::compute_box_selection_snapshot;
        use crate::sim::combat::Health;

        let mut sim = crate::sim::world::Simulation::new();
        let local_owner = sim.intern("Americans");
        let enemy_owner = sim.intern("Soviet");
        let mobile_type = sim.intern("E1");
        let building_type = sim.intern("NAPOWR");

        let mut hidden_mobile = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            enemy_owner,
            Health::new(100),
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
            Health::new(100),
            building_type,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        hidden_mobile.lifecycle.set_in_limbo_for_test(false);
        hidden_building.lifecycle.set_in_limbo_for_test(false);
        let mobile_anchor = screen_position(&hidden_mobile);
        let building_anchor = screen_position(&hidden_building);
        sim.entities_mut_for_test().insert(hidden_mobile);
        sim.entities_mut_for_test().insert(hidden_building);
        sim.entity_mut_for_test(1)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        sim.reveal(1);

        let bounds = (
            mobile_anchor.0.min(building_anchor.0) - 32.0,
            mobile_anchor.1.min(building_anchor.1) - 32.0,
            mobile_anchor.0.max(building_anchor.0) + 32.0,
            mobile_anchor.1.max(building_anchor.1) + 32.0,
        );
        let frame = crate::app::presentation::frame::PresentationFrame::of_test(
            &sim,
            crate::rules::ruleset::RuleSet::empty_for_test(),
        );
        let visible = compose_tactical_screen_entity_encounter_order(
            &sim,
            &frame,
            bounds,
            Some(local_owner),
            sim.fog(),
            false,
            0,
            false,
        );
        let preflight = compose_tactical_screen_entity_encounter_order(
            &sim,
            &frame,
            bounds,
            Some(local_owner),
            sim.fog(),
            false,
            0,
            true,
        );

        // UnitClass::DrawIt registers the shrouded unit (0x0073CF6B);
        // AddSelectable (0x006D9EF0) tests no shroud.
        assert_eq!(visible, [1], "the shrouded enemy unit still registers");
        assert_eq!(
            preflight,
            [1, 2],
            "the live building joins it in bulk for band preflight"
        );

        let building_box = compute_box_selection_snapshot(
            sim.entities(),
            &preflight,
            &visible,
            &[],
            Some((sim.fog(), sim.house_alliances().as_map())),
            Some("Americans"),
            building_anchor.0 - 1.0,
            building_anchor.1 - 1.0,
            building_anchor.0 + 1.0,
            building_anchor.1 + 1.0,
            false,
            None,
            None,
            Some(sim.interner()),
        )
        .expect("the hidden building must consume a non-Shift band release");
        assert!(building_box.clear);
        assert!(
            building_box.select.is_empty(),
            "bulk registration affects caught-anything only, not band candidates"
        );

        let mobile_box = compute_box_selection_snapshot(
            sim.entities(),
            &preflight,
            &visible,
            &[],
            Some((sim.fog(), sim.house_alliances().as_map())),
            Some("Americans"),
            mobile_anchor.0 - 1.0,
            mobile_anchor.1 - 1.0,
            mobile_anchor.0 + 1.0,
            mobile_anchor.1 + 1.0,
            false,
            None,
            None,
            Some(sim.interner()),
        )
        .expect("a shrouded enemy unit consumes the band release too");
        assert!(mobile_box.clear && mobile_box.select.is_empty());
    }

    #[test]
    fn body_band_follows_retained_display_until_resubmission() {
        use crate::sim::world::Simulation;
        use crate::sim::world::display_layers::DisplayLayer;

        let mut sim = Simulation::new();
        *sim.fog_mut_for_test() = FogState::new(64, 64);
        let owner = sim.intern("Americans");
        let type_ref = sim.intern("BEAG");
        let mut aircraft = GameEntity::new_at_frame_zero_for_test(
            1,
            40,
            40,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(100),
            type_ref,
            EntityCategory::Aircraft,
            0,
            5,
            true,
        );
        aircraft.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));
        aircraft.position.set_exact_z_leptons_for_test(Some(900));
        aircraft.lifecycle.set_in_limbo_for_test(true);
        sim.entities_mut_for_test().insert(aircraft);
        sim.reveal(1);
        // A headless Reveal commits no exact Z; lift the aircraft back to 900
        // and resubmit its display, as a takeoff does.
        sim.entity_mut_for_test(1)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(900));
        sim.resubmit_entity_display_for_test(1);
        assert_eq!(sim.display_layers().layer_of(1), Some(DisplayLayer::TOP));

        // Reproduce the stale-registration boundary: physical descent has
        // reached the ground, but the phase transaction has not resubmitted.
        // A fresh altitude classifier used to choose Ground and then lose the
        // body because NativeDisplayOrder correctly had no entry for this ID.
        sim.entity_mut_for_test(1)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(0));
        assert_eq!(
            sim.display_layers().layer_of(1).and_then(layer_draw_band),
            Some(EntityDrawBand::Top)
        );
        let ground = crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder::new(
            sim.display_layers().members(DisplayLayer::GROUND),
        );
        assert!(
            ground
                .object_draw(1, crate::render::tactical_draw_plan::SpriteEncoding::Voxel)
                .is_none()
        );

        // Exercise an existing real lifecycle resubmission. This establishes
        // the render consumer contract, not the still-missing landing callback.
        sim.conceal(1);
        assert_eq!(
            sim.display_layers().layer_of(1).and_then(layer_draw_band),
            None
        );
        sim.reveal(1);
        assert_eq!(
            sim.display_layers().layer_of(1).and_then(layer_draw_band),
            Some(EntityDrawBand::Ground)
        );
        let ground = crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder::new(
            sim.display_layers().members(DisplayLayer::GROUND),
        );
        assert!(
            ground
                .object_draw(1, crate::render::tactical_draw_plan::SpriteEncoding::Voxel)
                .is_some()
        );
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
        gi.set_category_for_test(crate::map::entities::EntityCategory::Infantry);
        gi.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        entities.insert(gi);
        assert!(begin_parachute_descent(
            &mut entities,
            1,
            STOCK_FLIGHT_LEVEL_LEPTONS
        ));
        let gi = entities.get(1).expect("entity");

        let (_, drawn_y) = screen_position(gi);
        let gi = crate::app::presentation::frame::ObjectRecord::of_test(gi);
        assert_eq!(ground_sort_row(&gi, drawn_y) - drawn_y, 216.0);
    }

    /// VERA's translucent fog removes the A buffer's concealment, so the draw
    /// plan relies on this gate: terrain stays visible but enemy objects are
    /// withheld over unexplored ground; under FogOfWar they also need current
    /// sight. Allies always draw. Standard YR keeps explored enemies visible.
    #[test]
    fn translucent_fog_withholds_only_unseen_enemies() {
        use crate::sim::combat::Health;

        let viewer_name = "Americans";
        let viewer = crate::sim::intern::test_intern(viewer_name);
        let enemy = crate::sim::intern::test_intern("Soviet");
        let entity = |owner: InternedId| {
            GameEntity::new_at_frame_zero_for_test(
                1,
                3,
                3,
                0,
                0,
                owner,
                Health::new(100),
                crate::sim::intern::test_intern("E1"),
                EntityCategory::Unit,
                0,
                5,
                true,
            )
        };
        let enemy_entity = entity(enemy);
        let own_entity = entity(viewer);
        let conceal = |fog: &FogState, entity: &GameEntity, owner: &str, fog_of_war: bool| {
            !object_shown_to_local_owner(
                Some(viewer_name),
                Some(viewer),
                fog,
                &Default::default(),
                &entity.position,
                owner,
                fog_of_war,
            )
        };

        // Never explored: hidden in both modes; the wall path agrees.
        let fog = FogState::new(16, 16);
        assert!(conceal(&fog, &enemy_entity, "Soviet", false));
        assert!(conceal(&fog, &enemy_entity, "Soviet", true));
        assert!(!conceal(&fog, &own_entity, viewer_name, false));
        assert!(translucent_fog_conceals_cell(
            Some(viewer),
            Some(&fog),
            3,
            3,
            false
        ));

        // Explored in standard YR: enemies stay visible even without current
        // sight; this is the regression that made them vanish from lit ground.
        let mut explored = FogState::new(16, 16);
        explored.set_ground_bits_for_test(viewer, 3, 3, 0x08);
        assert!(explored.is_cell_revealed(viewer, 3, 3));
        assert!(!explored.is_cell_visible(viewer, 3, 3));
        assert!(!conceal(&explored, &enemy_entity, "Soviet", false));
        assert!(conceal(&explored, &enemy_entity, "Soviet", true));
        assert!(!translucent_fog_conceals_cell(
            Some(viewer),
            Some(&explored),
            3,
            3,
            false
        ));
        assert!(translucent_fog_conceals_cell(
            Some(viewer),
            Some(&explored),
            3,
            3,
            true
        ));

        // Current sight keeps the enemy object in both modes.
        explored.mark_visible_for_owner(viewer, 3, 3);
        assert!(!conceal(&explored, &enemy_entity, "Soviet", false));
        assert!(!conceal(&explored, &enemy_entity, "Soviet", true));
    }

    /// The pass-by window is the union of the views widened by its margin:
    /// a span on its edge reaches it and one a pixel past does not, and a
    /// column reaches it by either end, as an aircraft above the view does
    /// by its shadow.
    #[test]
    fn a_span_on_the_widened_views_edge_reaches_it_and_one_past_does_not() {
        use crate::app::presentation::display_members::ScreenSpan;
        let window = PassByWindow::around([[100., 50., 900., 650.], [100., 50., 1000., 600.]]);
        let margin = PassByWindow::MARGIN;
        let reaches = |x: f32, top: f32, bottom: f32| window.reaches(ScreenSpan { x, top, bottom });
        assert!(reaches(1000. + margin, 300., 300.));
        assert!(!reaches(1001. + margin, 300., 300.));
        assert!(reaches(100. - margin, 300., 300.));
        assert!(!reaches(99. - margin, 300., 300.));
        assert!(reaches(500., -2000. - margin, 50. - margin));
        assert!(!reaches(500., -2000. - margin, 49. - margin));
        assert!(reaches(500., 650. + margin, 2000. + margin));
        assert!(!reaches(500., 651. + margin, 2000. + margin));
    }
}

#[cfg(all(test, tests_app_presentation_instances))]
#[path = "building_warp_draw_tests.rs"]
mod building_warp_draw_tests;
