//! UI overlay instance builders — status bars, software cursor.
//!
//! Split from the presentation render path. Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::app::input::cursor::{
    current_cursor_feedback_kind, cursor_id_for_feedback, software_cursor_frame_for,
};
use crate::app::presentation::instances::in_view;
use crate::app::types::{CursorId, HoverTargetKind, SoftwareCursorSequence};
use crate::map::entities::EntityCategory;
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::Position;
/// Authentic pip counts for unit health bars.
const UNIT_PIPS_VEHICLE: u32 = 17; // vehicles + aircraft
const UNIT_PIPS_INFANTRY: u32 = 8;

/// Horizontal pixel spacing between unit health pips (each pip drawn 2px apart).
const UNIT_PIP_STEP_X: f32 = 2.0;

/// Health bar offsets are baked into SelectionOverlay at load time
/// (game constant + canvas centering combined).

/// Fixed pip step along the NW isometric edge.
/// Each pip moves 4px left and 2px down, following the isometric 2:1 slope.
const PIP_STEP_X: f32 = -4.0;
const PIP_STEP_Y: f32 = 2.0;

/// Multiplier for the art.ini Height= value when computing vertical pip lift (z_screen).
/// Z = Height * HeightFactor (104) * AdjustForZ (≈0.14348) ≈ Height * 15.
/// Height=4 → z_screen=60, Height=5 → 75.
/// NOTE: Phobos `Height * 12` is for bracket EXTENT (vertical span), NOT z_screen.
const PIP_HEIGHT_FACTOR: f32 = 15.0;

/// Health-pip display clamp. This ratio never feeds simulation or command gates.
fn display_health_ratio(current: i32, strength: i32) -> f32 {
    if strength == 0 {
        0.0
    } else {
        (current as f32 / strength as f32).clamp(0.0, 1.0)
    }
}

/// Get INI-driven health condition thresholds, falling back to RA2 defaults.
fn condition_thresholds(state: &AppState) -> (f32, f32) {
    state
        .rules()
        .map(|r| {
            (
                r.general.condition_yellow as f32,
                r.general.condition_red as f32,
            )
        })
        .unwrap_or((0.5, 0.25))
}

/// The object under the cursor whose health bar the original draws on hover.
///
/// The per-object overlay pass calls the health-bar slot from exactly two arms:
/// the object is selected, or the object carries the cursor's hover flag and is
/// *not* selected. The hover flag is set from the cursor's resolved action
/// target, so a shrouded enemy — which is never an action target — is excluded.
fn health_bar_hover_target(
    state: &AppState,
    local_owner: Option<&str>,
) -> Option<(u64, HoverTargetKind)> {
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)?;
    let local_owner = local_owner?;
    let (world_x, world_y) = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let hover = crate::app::input::entity_pick::hover_target_at_point(
        sim,
        world_x,
        world_y,
        local_owner,
        state.match_state.sandbox_full_visibility,
        state.rules(),
        &state.height_map(),
        Some(
            &state
                .match_state
                .match_presentation
                .tactical_bridge_inverse_map,
        ),
    )?;
    match hover.kind {
        HoverTargetKind::FriendlyStructure
        | HoverTargetKind::EnemyStructure
        | HoverTargetKind::FriendlyUnit
        | HoverTargetKind::EnemyUnit => Some((hover.stable_id, hover.kind)),
        HoverTargetKind::HiddenEnemy => None,
    }
}

/// The hovered *structure*, for the building pip pass.
fn building_health_hover_target(state: &AppState, local_owner: Option<&str>) -> Option<u64> {
    health_bar_hover_target(state, local_owner).and_then(|(id, kind)| {
        matches!(
            kind,
            HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
        )
        .then_some(id)
    })
}

/// The hovered *unit or infantry*, for the non-building health-bar pass.
fn unit_health_hover_target(state: &AppState, local_owner: Option<&str>) -> Option<u64> {
    health_bar_hover_target(state, local_owner).and_then(|(id, kind)| {
        matches!(
            kind,
            HoverTargetKind::FriendlyUnit | HoverTargetKind::EnemyUnit
        )
        .then_some(id)
    })
}

/// Whether a non-building draws its bracket background and its health pips.
///
/// Two separate rules in the original, and damage is not part of either. The
/// health bar itself is drawn when the object is selected *or* hovered, while
/// the PIPBRD bracket behind it is drawn only when the object is selected — so
/// a hovered-but-unselected unit shows a bare pip strip with no bracket, and an
/// unselected, unhovered unit shows nothing however badly damaged it is.
///
/// Returns `(draw_bracket_background, draw_health_pips)`.
pub(crate) fn unit_status_visibility(selected: bool, hovered: bool) -> (bool, bool) {
    (selected, selected || hovered)
}

// DEFERRED members of this row's overlay set, recorded with what a follow-up
// needs. None of them requires new texture infrastructure — pips.shp and
// pips2.shp are both already loaded, and `DrawPipScalePips` swaps the shape
// handle from `DAT_00AC147C` (PIPS.SHP) to `DAT_00AC1480` (PIPS2.SHP) at
// 0x00709AEF for every non-building.
//
// * **`PipScale=Passengers` and `PipScale=Ammo`.** `build_cargo_pip_instances`
//   accepts only `PipScale::Tiberium` and additionally requires a miner. 38
//   stock rulesmd.ini sections set one of the two (34 Passengers, 4 Ammo).
//   `DrawPipScalePips` branches on `TechnoTypeClass+0x3D4` (1 = ammo,
//   2 = tiberium/storage, 5 = the `+0x2B8` branch); the passenger-list branch is
//   the mutually exclusive `else` of that chain, gated on `+0x5E0 >= 1` alone,
//   so `PipScale=Passengers` may gate nothing there — UNCHECKED. Trigger:
//   selecting a loaded IFV, Flak Track, Battle Fortress, Nighthawk or any
//   transport. Player effect: no passenger or ammo readout at all. Frequency:
//   common — transports are ordinary play. Downstream risk: none.
//
// * **Control-group number, spawn pips, self-heal indicator**, all in the tail
//   of `DrawPipScalePips`: the group index from `param_1[0x85]` drawn as text at
//   `param_2 + (-4, -0x27)` for units and `(-4, -0x24)` for infantry; spawn pips
//   from `TypeClass+0xD5C` via `SpawnManagerClass__CountDockedSpawns`; and the
//   self-heal frame 0xD (infantry) / 0x14 (units) at `(+0x26, -0x20)` /
//   `(+0x13, -0x23)`, blinking. Trigger: for the group number, every recall of a
//   control group. Player effect: a selected group shows no number, so the
//   player cannot tell which group is up. Frequency: common. Downstream risk:
//   none. The other two are rarer (Kirov/carrier spawns; self-healing units).
//
/// Building health: discrete pips from pips.shp along the isometric NW foundation edge.
///
/// Pip positions are computed from Dimension2 (foundation size in leptons) projected
/// through CoordsToScreen along the NW edge.
///
/// Simplified formula (CTS offsets cancel with foundation geometry):
///   num_pips  = floor(H * 15 / 2)
///   pip_x[i]  = loc_x + (-(W+H)*15 + 3 + num_pips*4) - i*4
///   pip_y[i]  = loc_y + ((H-W)*7.5 + 4 - 2*num_pips) + i*2
///
/// Where loc = the **plain entity anchor**, the projection of the building's own
/// coordinate — the same point the original's health-bar path starts from. That
/// path is reached from the object-render loop's foundation-centre coordinate,
/// not from the render-coordinate override the building's art takes, so the pips
/// deliberately do not carry that override.
/// The draw point is then shifted by the PIPS.SHP canvas/frame offset so the
/// SpriteInstance position is the final frame-rect top-left, matching flags 0x600.
pub(crate) fn build_building_status_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let (Some(sim), Some(overlay)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.selection_overlay,
    ) else {
        return Vec::new();
    };
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let pip_size: [f32; 2] = overlay.pip_frame_size();
    let pip_uv_size: [f32; 2] = overlay.pip_uv_size();
    let (pip_adj_x, pip_adj_y) = overlay.pip_canvas_adj();
    let has_pips: bool = overlay.pip_texture().is_some();
    let (cond_y, cond_r) = condition_thresholds(state);
    let hovered_structure_id = building_health_hover_target(state, local_owner.as_deref());
    let mut instances = Vec::new();
    for e in sim.entities().values() {
        if e.category != EntityCategory::Structure {
            continue;
        }
        if !e.selected && hovered_structure_id != Some(e.stable_id()) {
            continue;
        }
        let health = &e.health;
        let type_str = sim.interner.resolve(e.type_ref());
        if !status_entity_visible_plain(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }
        let (sx, sy) = crate::render::locomotor_visual::screen_position(e);
        // Foundation= is merged from art.ini into ObjectType by merge_art_data().
        // Height= is an art.ini property, looked up via Image= redirect.
        let obj = state.rules().and_then(|r| r.object(type_str));
        let foundation: (u32, u32) = obj
            .map(|o| {
                let (w, h) = crate::rules::foundation::foundation_dimensions(&o.foundation);
                (u32::from(w), u32::from(h))
            })
            .unwrap_or((2, 2));
        // gamemd reads Height from the ART SECTION (Image= redirect target, NOT the
        // type ID itself). If the Image section doesn't define Height, use default 2
        // (BuildingTypeClass constructor default at 0x45DD90).
        // Do NOT fall back to the type_ref section — that would read the wrong value
        // for buildings with Image= redirect (e.g., GAPOWR→YAPOWR).
        let art_key: &str = obj
            .map(|o| {
                let img = o.image.as_str();
                if img.is_empty() { o.id.as_str() } else { img }
            })
            .unwrap_or(type_str);
        let art_height: f32 = state
            .rules()
            .and_then(|rules| rules.art_registry.get(art_key))
            .map(|entry| entry.height as f32)
            .unwrap_or(2.0);
        let Some(strength) = obj.map(|obj| obj.strength) else {
            // A health display needs the live type; no saved maximum fallback.
            continue;
        };
        let ratio = display_health_ratio(health.current, strength);
        let depth: f32 = 0.0006;

        // Pip count: floor(H * 7.5) — from original (screen1.y - screen2.y) / 2.
        let num_pips: u32 = (foundation.1 * 15) / 2;
        if num_pips == 0 {
            continue;
        }

        // DrawHealthBar computes a PIPS.SHP canvas-center draw point at GetCoords
        // plus the projected NW upper edge, then CC_Draw_Shape shifts it by the
        // frame/canvas offset from flags 0x600.
        let n: f32 = num_pips as f32;
        let fw = foundation.0 as f32;
        let fh = foundation.1 as f32;
        let foundation_center_x = sx + (fw - fh) * 15.0;
        let foundation_center_y = sy + (fw + fh) * 7.5 - 15.0;
        let projected_x = -(fw + fh) * 15.0;
        let projected_y = (fh - fw) * 7.5 - art_height * PIP_HEIGHT_FACTOR;
        let draw_start_x: f32 = foundation_center_x + projected_x + 3.0 + n * 4.0;
        let draw_start_y: f32 = foundation_center_y + projected_y + 4.0 - n * 2.0;
        let start_x: f32 = draw_start_x + pip_adj_x;
        let start_y: f32 = draw_start_y + pip_adj_y;

        if has_pips && foundation.1 > 0 {
            let pip_w: f32 = pip_size[0];
            let pip_h: f32 = pip_size[1];
            // Bounding box for culling: pips span from start to start + (N-1)*step.
            let end_x: f32 = start_x + (n - 1.0) * PIP_STEP_X;
            let end_y: f32 = start_y + (n - 1.0) * PIP_STEP_Y;
            let min_x: f32 = start_x.min(end_x);
            let min_y: f32 = start_y.min(end_y);
            let total_w: f32 = (start_x - end_x).abs() + pip_w;
            let total_h: f32 = (start_y - end_y).abs() + pip_h;
            if !in_view(
                min_x,
                min_y,
                total_w,
                total_h,
                state.match_state.input.camera_x,
                state.match_state.input.camera_y,
                sw,
                sh,
                48.0,
            ) {
                continue;
            }
            // ftol(ratio * numPips), clamped to [1, numPips].
            let filled: u32 = if health.current > 0 {
                ((num_pips as f32 * ratio) as u32).max(1).min(num_pips)
            } else {
                0
            };
            let health_variant: u32 = health_pip_variant(ratio, cond_y, cond_r);
            for i in 0..num_pips {
                let px: f32 = start_x + i as f32 * PIP_STEP_X;
                let py: f32 = start_y + i as f32 * PIP_STEP_Y;
                let variant: u32 = if i < filled { health_variant } else { 0 };
                let uv_origin: [f32; 2] = overlay.pip_uv_origin(variant);
                instances.push(SpriteInstance {
                    position: [px, py],
                    size: [pip_w, pip_h],
                    uv_origin,
                    uv_size: pip_uv_size,
                    depth,
                    tint: [1.0, 1.0, 1.0],
                    alpha: 1.0,
                    ..Default::default()
                });
            }
        } else {
            // Fallback: procedural colored segments when pips.shp unavailable.
            let seg_w: f32 = 3.0;
            let seg_h: f32 = 4.0;
            let end_x: f32 = start_x + (n - 1.0) * PIP_STEP_X;
            let end_y: f32 = start_y + (n - 1.0) * PIP_STEP_Y;
            let min_x: f32 = start_x.min(end_x);
            let min_y: f32 = start_y.min(end_y);
            let total_w: f32 = (start_x - end_x).abs() + seg_w;
            let total_h: f32 = (start_y - end_y).abs() + seg_h;
            if !in_view(
                min_x,
                min_y,
                total_w,
                total_h,
                state.match_state.input.camera_x,
                state.match_state.input.camera_y,
                sw,
                sh,
                48.0,
            ) {
                continue;
            }
            let fill_color = health_fill_color(ratio, cond_y, cond_r);
            let filled: u32 = if health.current > 0 {
                ((num_pips as f32 * ratio) as u32).max(1).min(num_pips)
            } else {
                0
            };
            for i in 0..num_pips {
                let seg_x: f32 = start_x + i as f32 * PIP_STEP_X;
                let seg_y: f32 = start_y + i as f32 * PIP_STEP_Y;
                let tint = if i < filled {
                    fill_color
                } else {
                    [0.10, 0.10, 0.10]
                };
                instances.push(SpriteInstance {
                    position: [seg_x, seg_y],
                    size: [seg_w, seg_h],
                    uv_origin: [0.0, 0.0],
                    uv_size: [1.0, 1.0],
                    depth,
                    tint,
                    alpha: 1.0,
                    ..Default::default()
                });
            }
        }
    }
    instances
}

/// Occupant pips for garrisoned buildings (pips.shp frames 6-12).
///
/// Drawn for every visible building with `ShowOccupantPips=yes` and `MaxNumberOccupants > 0`.
/// One pip per slot: filled slots use the infantry's `OccupyPip` color, empty slots use
/// frame 6 (gray). Starts at (screen_x+6, screen_y-1), step (+4, +2) per pip along
/// the isometric NW edge.
pub(crate) fn build_occupant_pip_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let (Some(sim), Some(overlay)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.selection_overlay,
    ) else {
        return Vec::new();
    };
    let has_tex = overlay.occupant_pip_texture().is_some();
    if !has_tex {
        return Vec::new();
    }
    let pip_size: [f32; 2] = overlay.occupant_pip_frame_size();
    let pip_uv_size: [f32; 2] = overlay.occupant_pip_uv_size();
    let (adj_x, adj_y) = overlay.occupant_pip_canvas_adj();
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let rules = state.rules();
    // Occupant pips are NOT free-standing: `DrawPipScalePips` 0x00709A90 has
    // exactly two callsites, 0x006F682C and 0x006F6AB0, and both are inside
    // `TechnoClass__DrawHealthBar`. So they are reachable only through the same
    // selected-or-hovered arms that gate the health bar — the same rule that
    // stopped VERA drawing a bar over every damaged unit. Without it every
    // garrisoned building on a city map wears its pip strip permanently.
    let hovered_structure_id = building_health_hover_target(state, local_owner.as_deref());
    let mut instances = Vec::new();

    for e in sim.entities().values() {
        if e.category != EntityCategory::Structure {
            continue;
        }
        if !e.selected && hovered_structure_id != Some(e.stable_id()) {
            continue;
        }
        let type_str = sim.interner.resolve(e.type_ref());
        let Some(obj) = rules.and_then(|r| r.object(type_str)) else {
            continue;
        };
        if !obj.show_occupant_pips || obj.max_number_occupants == 0 {
            continue;
        }
        if !status_entity_visible_plain(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }
        let cargo = match e.passenger_role.cargo() {
            Some(c) => c,
            None => continue,
        };
        let (sx, sy) = crate::render::locomotor_visual::screen_position(e);
        // The (+6, -1) offset is applied to the point `DrawHealthBar`'s BUILDING
        // branch hands the pip routine, which is the entity anchor plus the
        // projected foundation edge — not the raw anchor. Dropping that term put
        // the strip on the building's art instead of at its west corner, a miss
        // of (-60, +15) px on a 2-deep footprint and (-90, +30) on a 3-deep one.
        // Same pair the health pips above already use.
        let (fw, fh) = {
            let (w, h) = crate::rules::foundation::foundation_dimensions(&obj.foundation);
            (w as f32, h as f32)
        };
        let anchor_x = sx + (fw - fh) * 15.0 - (fw + fh) * 15.0;
        let anchor_y = sy + (fw + fh) * 7.5 - 15.0 + (fh - fw) * 7.5;
        let start_x: f32 = anchor_x + 6.0 + adj_x;
        let start_y: f32 = anchor_y - 1.0 + adj_y;
        let count: u32 = obj.max_number_occupants;
        // Occupant pip step: +4 right, +2 down (isometric NW edge, same as health pips but positive X).
        const STEP_X: f32 = 4.0;
        const STEP_Y: f32 = 2.0;

        // Bounding box culling.
        let end_x: f32 = start_x + (count.saturating_sub(1)) as f32 * STEP_X;
        let end_y: f32 = start_y + (count.saturating_sub(1)) as f32 * STEP_Y;
        let min_x: f32 = start_x.min(end_x);
        let min_y: f32 = start_y.min(end_y);
        let total_w: f32 = (end_x - start_x).abs() + pip_size[0];
        let total_h: f32 = (end_y - start_y).abs() + pip_size[1];
        if !in_view(
            min_x,
            min_y,
            total_w,
            total_h,
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            48.0,
        ) {
            continue;
        }

        for i in 0..count {
            // Determine pip frame: occupied slot → occupant's OccupyPip, empty → frame 6.
            let frame_index: u32 = if (i as usize) < cargo.passengers.len() {
                let pax_id = cargo.passengers[i as usize];
                sim.entities()
                    .get(pax_id)
                    .and_then(|pax| {
                        rules.and_then(|r| r.object(sim.interner.resolve(pax.type_ref())))
                    })
                    .map(|pax_obj| pax_obj.occupy_pip)
                    .unwrap_or(7) // default PersonGreen
            } else {
                6 // empty slot
            };
            let px: f32 = start_x + i as f32 * STEP_X;
            let py: f32 = start_y + i as f32 * STEP_Y;
            let uv_origin: [f32; 2] = overlay.occupant_pip_uv_origin(frame_index);
            instances.push(SpriteInstance {
                position: [px, py],
                size: [pip_size[0], pip_size[1]],
                uv_origin,
                uv_size: pip_uv_size,
                depth: 0.0006,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
        }
    }

    // Veterancy chevrons ride this same atlas and pass.
    //
    // `DrawVeterancyPips` 0x0070A990 (TechnoClass vtable +0x454) has a single
    // callsite, 0x006F5382 in `DrawExtras`, and it sits BEFORE the selected test
    // at 0x006F5388-0x006F5390 — so unlike the health bar and the garrison pips,
    // rank draws for every visible object, selected or not. It blits at
    // `pLoc + (5, 2)` for infantry and `pLoc + (10, 6)` otherwise.
    for e in sim.entities().values() {
        if e.category == EntityCategory::Structure {
            continue;
        }
        // Rookie draws nothing: native's rookie frame is the flagged variant,
        // reached only from the veterancy < 0 branch, which a freshly built unit
        // is not in.
        let rank: u32 = if e.veterancy >= 200 {
            2
        } else if e.veterancy >= 100 {
            1
        } else {
            continue;
        };
        if !status_entity_visible_plain(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }
        let (sx, sy) = crate::render::locomotor_visual::screen_position(e);
        let (dx, dy) = if e.category == EntityCategory::Infantry {
            (5.0, 2.0)
        } else {
            (10.0, 6.0)
        };
        let px: f32 = sx + dx + adj_x;
        let py: f32 = sy + dy + adj_y;
        if !in_view(
            px,
            py,
            pip_size[0],
            pip_size[1],
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            48.0,
        ) {
            continue;
        }
        instances.push(SpriteInstance {
            position: [px, py],
            size: [pip_size[0], pip_size[1]],
            uv_origin: overlay.veterancy_pip_uv_origin(rank),
            uv_size: pip_uv_size,
            depth: 0.0006,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        });
    }

    instances
}

/// Non-building health bar backgrounds: pipbrd.shp bracket sprites.
/// Frame 0 = vehicle/aircraft (36×4), frame 1 = infantry (18×4).
pub(crate) fn build_unit_status_bg_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let (Some(sim), Some(overlay)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.selection_overlay,
    ) else {
        return Vec::new();
    };
    if overlay.pipbrd_texture().is_none() {
        return Vec::new();
    }
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let hovered_unit_id = unit_health_hover_target(state, local_owner.as_deref());
    let mut instances = Vec::new();
    for e in sim.entities().values() {
        if e.category == EntityCategory::Structure {
            continue;
        }
        if e.passenger_role.is_inside_transport() {
            continue;
        }
        let (draw_background, _) =
            unit_status_visibility(e.selected, hovered_unit_id == Some(e.stable_id()));
        if !draw_background {
            continue;
        }
        if !status_entity_visible_plain(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }
        // Already the drawn position, height lift included, so the bar tracks
        // the sprite for airborne units without repeating the lift here.
        let (sx, sy) = crate::app::presentation::instances::interpolated_screen_position_entity(e);
        let is_infantry: bool = e.category == EntityCategory::Infantry;
        let (bar_size, uv_origin, uv_size) = if is_infantry {
            (
                overlay.pipbrd_infantry_size(),
                overlay.pipbrd_infantry_uv().0,
                overlay.pipbrd_infantry_uv().1,
            )
        } else {
            (
                overlay.pipbrd_vehicle_size(),
                overlay.pipbrd_vehicle_uv().0,
                overlay.pipbrd_vehicle_uv().1,
            )
        };
        let bracket_delta: f32 = state
            .rules()
            .and_then(|r| r.object(sim.interner.resolve(e.type_ref())))
            .map(|obj| obj.pixel_selection_bracket_delta as f32)
            .unwrap_or(0.0);
        let (off_x, off_y) = overlay.pipbrd_offset(is_infantry);
        let (bar_x, bar_y) = (sx + off_x, sy + bracket_delta + off_y);
        if !in_view(
            bar_x,
            bar_y,
            bar_size[0],
            bar_size[1],
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            48.0,
        ) {
            continue;
        }
        instances.push(SpriteInstance {
            position: [bar_x, bar_y],
            size: bar_size,
            uv_origin,
            uv_size,
            depth: 0.0006,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        });
    }
    instances
}

/// Non-building health bar fills: colored rectangles drawn over pipbrd.shp backgrounds.
/// Uses white_texture with per-instance color tint. Falls back to procedural segments
/// if pipbrd.shp is unavailable.
pub(crate) fn build_unit_status_fill_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let (Some(sim), Some(overlay)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.selection_overlay,
    ) else {
        return Vec::new();
    };
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let (cond_y, cond_r) = condition_thresholds(state);
    let hovered_unit_id = unit_health_hover_target(state, local_owner.as_deref());
    let mut instances = Vec::new();
    for e in sim.entities().values() {
        if e.category == EntityCategory::Structure {
            continue;
        }
        if e.passenger_role.is_inside_transport() {
            continue;
        }
        let health = &e.health;
        let (_, draw_pips) =
            unit_status_visibility(e.selected, hovered_unit_id == Some(e.stable_id()));
        if !draw_pips {
            continue;
        }
        if !status_entity_visible_plain(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }
        // Already the drawn position, height lift included, so the bar tracks
        // the sprite for airborne units without repeating the lift here.
        let (sx, sy) = crate::app::presentation::instances::interpolated_screen_position_entity(e);
        let Some(obj) = state
            .rules()
            .and_then(|rules| rules.object(sim.interner.resolve(e.type_ref())))
        else {
            continue;
        };
        let ratio = display_health_ratio(health.current, obj.strength);

        let is_infantry: bool = e.category == EntityCategory::Infantry;
        let num_pips: u32 = if is_infantry {
            UNIT_PIPS_INFANTRY
        } else {
            UNIT_PIPS_VEHICLE
        };
        let bracket_delta: f32 = obj.pixel_selection_bracket_delta as f32;
        let (pip_off_x, pip_off_y) = overlay.pip_offset(is_infantry);
        let (pip_start_x, pip_start_y) = (sx + pip_off_x, sy + bracket_delta + pip_off_y);
        // Filled pip count: floor(ratio * maxPips), clamped to [1, maxPips].
        let raw_filled: u32 = (ratio * num_pips as f32) as u32;
        let filled: u32 = raw_filled.max(1).min(num_pips);
        // Map health ratio to unit pip variant: 0=green, 1=yellow, 2=red.
        let variant: u32 = if ratio > cond_y {
            0
        } else if ratio > cond_r {
            1
        } else {
            2
        };

        if let Some(_unit_pip_tex) = overlay.unit_pip_texture() {
            // Authentic RA2 style: individual pip sprites from pips.shp frames 16-18.
            let pip_size: [f32; 2] = overlay.unit_pip_frame_size();
            let pip_uv_size: [f32; 2] = overlay.unit_pip_uv_size();
            let total_w: f32 = (num_pips - 1) as f32 * UNIT_PIP_STEP_X + pip_size[0];
            if !in_view(
                pip_start_x,
                pip_start_y,
                total_w,
                pip_size[1],
                state.match_state.input.camera_x,
                state.match_state.input.camera_y,
                sw,
                sh,
                48.0,
            ) {
                continue;
            }
            // Only filled pips are drawn (no empty pips — PIPBRD.SHP is the background).
            let uv_origin: [f32; 2] = overlay.unit_pip_uv_origin(variant);
            for i in 0..filled {
                let px: f32 = pip_start_x + i as f32 * UNIT_PIP_STEP_X;
                let py: f32 = pip_start_y; // Y is constant for all pips in a bar.
                instances.push(SpriteInstance {
                    position: [px, py],
                    size: pip_size,
                    uv_origin,
                    uv_size: pip_uv_size,
                    depth: 0.0005,
                    tint: [1.0, 1.0, 1.0],
                    alpha: 1.0,
                    ..Default::default()
                });
            }
        } else {
            // Fallback: procedural colored segments when pips.shp unavailable.
            let seg_w: f32 = 2.0;
            let seg_h: f32 = 3.0;
            let total_w: f32 = num_pips as f32 * seg_w;
            if !in_view(
                pip_start_x,
                pip_start_y,
                total_w,
                seg_h,
                state.match_state.input.camera_x,
                state.match_state.input.camera_y,
                sw,
                sh,
                48.0,
            ) {
                continue;
            }
            let fill_color = health_fill_color(ratio, cond_y, cond_r);
            for i in 0..num_pips {
                let seg_x: f32 = pip_start_x + i as f32 * seg_w;
                let tint = if i < filled {
                    fill_color
                } else {
                    [0.10, 0.10, 0.10]
                };
                instances.push(SpriteInstance {
                    position: [seg_x, pip_start_y],
                    size: [seg_w, seg_h],
                    uv_origin: [0.0, 0.0],
                    uv_size: [1.0, 1.0],
                    depth: 0.0005,
                    tint,
                    alpha: 1.0,
                    ..Default::default()
                });
            }
        }
    }
    instances
}

/// Horizontal pixel spacing between cargo pips (each pip drawn 4px apart).
const CARGO_PIP_STEP_X: f32 = 4.0;

/// Tiberium/cargo pips for harvesters (pips2.shp frames 0, 2, 5).
///
/// Drawn for selected vehicles with PipScale=Tiberium that have a miner component.
/// Each bale in the cargo is one pip: ore=green (variant 1), gem=colored (variant 2).
/// Empty slots shown as variant 0. Start at (sx - 15 + canvas_adj_x, sy + 10 + canvas_adj_y),
/// step (+4, 0) per pip. Draw order: gem pips first, then ore pips, then empty slots.
pub(crate) fn build_cargo_pip_instances(state: &AppState, sw: f32, sh: f32) -> Vec<SpriteInstance> {
    let (Some(sim), Some(overlay)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        &state.match_state.match_presentation.selection_overlay,
    ) else {
        return Vec::new();
    };
    let Some(_tib_tex) = overlay.tiberium_pip_texture() else {
        log::debug!("cargo pips: tiberium pip texture not loaded (pips2.shp missing?)");
        return Vec::new();
    };
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let mut instances = Vec::new();
    let (tib_adj_x, tib_adj_y) = overlay.tiberium_pip_canvas_adj();
    let pip_size: [f32; 2] = overlay.tiberium_pip_frame_size();
    let pip_uv_size: [f32; 2] = overlay.tiberium_pip_uv_size();

    for e in sim.entities().values() {
        if e.category == EntityCategory::Structure {
            continue;
        }
        if e.passenger_role.is_inside_transport() {
            continue;
        }
        if !e.selected {
            continue;
        }
        let obj = state
            .rules()
            .and_then(|r| r.object(sim.interner.resolve(e.type_ref())));
        let is_tiberium_scale = obj
            .map(|o| o.pip_scale == crate::rules::object_type::PipScale::Tiberium)
            .unwrap_or(false);
        if !is_tiberium_scale {
            continue;
        }
        let Some(ref miner) = e.miner else {
            continue;
        };
        if !status_entity_visible_plain(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }

        let (sx, sy) = crate::app::presentation::instances::interpolated_screen_position_entity(e);
        let bracket_delta: f32 = obj
            .map(|o| o.pixel_selection_bracket_delta as f32)
            .unwrap_or(0.0);
        // Pip scale offset for non-buildings: (pLoc.X - 15, pLoc.Y + 10).
        let start_x: f32 = sx - 15.0 + tib_adj_x;
        let start_y: f32 = sy + bracket_delta + 10.0 + tib_adj_y;
        // 5-pip display: cargo_pips() returns 0-5 based on fill ratio.
        const MAX_PIPS: u32 = 5;
        let filled: u32 = miner.cargo_pips() as u32;
        let empty_count: u32 = MAX_PIPS - filled;

        // Bounding box culling.
        let total_w: f32 = (MAX_PIPS - 1) as f32 * CARGO_PIP_STEP_X + pip_size[0];
        if !in_view(
            start_x,
            start_y,
            total_w,
            pip_size[1],
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            48.0,
        ) {
            continue;
        }

        // Proportion filled pips between gem and ore based on cargo contents.
        let gem_bales: u32 = miner
            .cargo
            .iter()
            .filter(|b| b.resource_type == crate::sim::miner::ResourceType::Gem)
            .count() as u32;
        let total_bales: u32 = miner.cargo.len() as u32;
        let gem_pips: u32 = if total_bales > 0 {
            (gem_bales * filled + total_bales - 1) / total_bales // round up
        } else {
            0
        };
        let ore_pips: u32 = filled - gem_pips.min(filled);

        let mut pip_idx: u32 = 0;
        // Gem pips (variant 2).
        let uv_gem: [f32; 2] = overlay.tiberium_pip_uv_origin(2);
        for _ in 0..gem_pips {
            let px: f32 = start_x + pip_idx as f32 * CARGO_PIP_STEP_X;
            instances.push(SpriteInstance {
                position: [px, start_y],
                size: pip_size,
                uv_origin: uv_gem,
                uv_size: pip_uv_size,
                depth: 0.0004,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
            pip_idx += 1;
        }
        // Ore pips (variant 1).
        let uv_ore: [f32; 2] = overlay.tiberium_pip_uv_origin(1);
        for _ in 0..ore_pips {
            let px: f32 = start_x + pip_idx as f32 * CARGO_PIP_STEP_X;
            instances.push(SpriteInstance {
                position: [px, start_y],
                size: pip_size,
                uv_origin: uv_ore,
                uv_size: pip_uv_size,
                depth: 0.0004,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
            pip_idx += 1;
        }
        // Empty pips (variant 0).
        let uv_empty: [f32; 2] = overlay.tiberium_pip_uv_origin(0);
        for _ in 0..empty_count {
            let px: f32 = start_x + pip_idx as f32 * CARGO_PIP_STEP_X;
            instances.push(SpriteInstance {
                position: [px, start_y],
                size: pip_size,
                uv_origin: uv_empty,
                uv_size: pip_uv_size,
                depth: 0.0004,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
            pip_idx += 1;
        }
    }
    if !instances.is_empty() {
        log::debug!(
            "cargo pips: {} instances for {} entities (pip_size={:?}, uv_size={:?})",
            instances.len(),
            sim.entities()
                .values()
                .filter(|e| e.selected && e.miner.is_some())
                .count(),
            pip_size,
            pip_uv_size,
        );
    }
    instances
}

/// Radius rings for selected sensor/gap buildings. This is Tactical action-visual
/// rendering, not part of selection brackets or health pips.
pub(crate) fn build_building_radius_ring_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> Vec<SpriteInstance> {
    let (Some(sim), Some(rules)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules().map(|r| r),
    ) else {
        return Vec::new();
    };
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let mut instances = Vec::new();

    for e in sim.entities().values() {
        if e.category != EntityCategory::Structure || !e.selected {
            continue;
        }
        if !status_entity_visible_plain(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }
        let type_str = sim.interner.resolve(e.type_ref());
        let Some(obj) = rules.object(type_str) else {
            continue;
        };
        let Some(radius_cells) = building_sensor_range_cells(obj, rules) else {
            continue;
        };
        if radius_cells == 0 {
            continue;
        }

        let (fw, fh) = crate::rules::foundation::foundation_dimensions(&obj.foundation);
        let (base_x, base_y) = crate::render::locomotor_visual::screen_position(e);
        let center_x = base_x + (fw as f32 - fh as f32) * 15.0;
        let center_y = base_y + (fw as f32 + fh as f32) * 7.5 - 15.0;
        let radius_x = radius_cells as f32 * 30.0;
        let radius_y = radius_cells as f32 * 15.0;
        if !in_view(
            center_x - radius_x,
            center_y - radius_y,
            radius_x * 2.0,
            radius_y * 2.0,
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
            sw,
            sh,
            16.0,
        ) {
            continue;
        }
        emit_ellipse_ring(
            &mut instances,
            center_x,
            center_y,
            radius_x,
            radius_y,
            [0.15, 1.0, 0.25],
        );
    }

    instances
}

fn building_sensor_range_cells(obj: &ObjectType, rules: &RuleSet) -> Option<u32> {
    if obj.psychic_detection_radius > 0 {
        return Some(u32::from(obj.psychic_detection_radius));
    }
    if obj.gap_generator {
        if obj.super_gap_radius_in_cells > 0 {
            return Some(u32::from(obj.super_gap_radius_in_cells));
        }
        if obj.gap_radius_in_cells > 0 {
            return Some(u32::from(obj.gap_radius_in_cells));
        }
        return Some(rules.general.gap_radius.max(0) as u32);
    }
    if (obj.sensor_array || obj.cloak_generator || obj.sensors) && obj.sensors_sight > 0 {
        return Some(u32::from(obj.sensors_sight));
    }
    None
}

fn emit_ellipse_ring(
    instances: &mut Vec<SpriteInstance>,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    tint: [f32; 3],
) {
    let segments = ((rx.max(ry) / 2.0).ceil() as usize).clamp(96, 384);
    let mut prev = ellipse_point(cx, cy, rx, ry, 0.0);
    for i in 1..=segments {
        let angle = std::f32::consts::TAU * i as f32 / segments as f32;
        let next = ellipse_point(cx, cy, rx, ry, angle);
        emit_colored_line(instances, prev, next, tint);
        prev = next;
    }
}

fn ellipse_point(cx: f32, cy: f32, rx: f32, ry: f32, angle: f32) -> [f32; 2] {
    [cx + angle.cos() * rx, cy + angle.sin() * ry]
}

fn emit_colored_line(
    instances: &mut Vec<SpriteInstance>,
    a: [f32; 2],
    b: [f32; 2],
    tint: [f32; 3],
) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let steps = dx.abs().max(dy.abs()).ceil() as i32;
    if steps <= 0 {
        return;
    }
    let step_x = dx / steps as f32;
    let step_y = dy / steps as f32;
    for i in 0..steps {
        instances.push(SpriteInstance {
            position: [
                (a[0] + step_x * i as f32).round(),
                (a[1] + step_y * i as f32).round(),
            ],
            size: [1.0, 1.0],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            tint,
            alpha: 1.0,
            depth: 0.00055,
            ..Default::default()
        });
    }
}

/// Map health ratio to pip atlas variant index (1=green, 2=yellow, 4=red).
fn health_pip_variant(ratio: f32, condition_yellow: f32, condition_red: f32) -> u32 {
    if ratio > condition_yellow {
        1 // Green.
    } else if ratio > condition_red {
        2 // Yellow.
    } else {
        4 // Red.
    }
}

/// Resolve the active cursor id and sequence for the current game state.
/// Maps game-state intent → CursorId → loaded sequence via HashMap lookup.
/// The id travels with the sequence because the animation phase is keyed on it —
/// changing shape restarts the sequence at frame 0.
fn active_cursor_sequence(state: &AppState) -> Option<(CursorId, &SoftwareCursorSequence)> {
    let cursor = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()?;
    let id: CursorId = current_cursor_feedback_kind(state)
        .and_then(cursor_id_for_feedback)
        .unwrap_or(CursorId::Default);
    cursor.get(id).map(|sequence| (id, sequence))
}

pub(crate) fn build_software_cursor_instances(state: &AppState) -> Vec<SpriteInstance> {
    if !state.use_software_cursor() {
        return Vec::new();
    }
    let Some((id, sequence)) = active_cursor_sequence(state) else {
        return Vec::new();
    };
    let Some(frame) = software_cursor_frame_for(id, sequence) else {
        return Vec::new();
    };
    // Cursor rendering: the hotspot pixel must sit exactly at the OS cursor position.
    // Scroll cursors use edge-aligned hotspots, so cursor_x/y represents the hotspot
    // location — subtract the hotspot offset to find the top-left of the sprite.
    // Note: cursor_x/y are in screen space; camera offset is NOT applied (cursor is UI).
    vec![SpriteInstance {
        position: [
            state.match_state.input.cursor_x + state.match_state.input.camera_x
                - sequence.hotspot[0],
            state.match_state.input.cursor_y + state.match_state.input.camera_y
                - sequence.hotspot[1],
        ],
        size: [frame.width, frame.height],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: 0.0001,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }]
}

pub(crate) fn current_software_cursor_texture(state: &AppState) -> Option<&BatchTexture> {
    let (id, sequence) = active_cursor_sequence(state)?;
    Some(&software_cursor_frame_for(id, sequence)?.texture)
}

fn status_entity_visible_plain(
    local_owner: Option<crate::sim::intern::InternedId>,
    fog: &crate::sim::vision::FogState,
    pos: &Position,
    entity_owner: crate::sim::intern::InternedId,
    ignore_visibility: bool,
) -> bool {
    if ignore_visibility {
        return true;
    }
    let Some(local_owner) = local_owner else {
        return true;
    };
    if local_owner == entity_owner {
        return true;
    }
    fog.is_cell_revealed(local_owner, pos.rx, pos.ry)
        && !fog.is_cell_gap_covered(local_owner, pos.rx, pos.ry)
}

pub(crate) fn health_fill_color(ratio: f32, condition_yellow: f32, condition_red: f32) -> [f32; 3] {
    if ratio > condition_yellow {
        // Bright lime green — high health (#00FF00).
        [0.0, 1.0, 0.0]
    } else if ratio > condition_red {
        // Yellow — medium health (#FFFF00).
        [1.0, 1.0, 0.0]
    } else {
        // Red — critical health (#FF0000).
        [1.0, 0.0, 0.0]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn health_pips_clamp_signed_actual_without_narrowing_live_strength() {
        assert_eq!(super::display_health_ratio(70_000, 140_000), 0.5);
        assert_eq!(super::display_health_ratio(70_000, 280_000), 0.25);
        assert_eq!(super::display_health_ratio(-1, 100_000), 0.0);
        assert_eq!(super::display_health_ratio(150_000, 100_000), 1.0);
        assert_eq!(super::display_health_ratio(100, 0), 0.0);
    }

    use super::*;

    /// A selected object draws both halves — bracket and pips.
    #[test]
    fn selected_unit_draws_bracket_and_pips() {
        assert_eq!(unit_status_visibility(true, false), (true, true));
        assert_eq!(unit_status_visibility(true, true), (true, true));
    }

    /// The hover arm draws the pip strip with no bracket behind it: the
    /// bracket blit sits under an is-selected test inside the health-bar
    /// routine, while the pips do not.
    #[test]
    fn hovered_unselected_unit_draws_pips_without_the_bracket() {
        assert_eq!(unit_status_visibility(false, true), (false, true));
    }

    /// The clause the old damage gate broke: an unselected, unhovered object
    /// draws no health bar at all, whatever its health is. The overlay pass
    /// reaches the health-bar slot only through the selected and hover arms.
    #[test]
    fn unselected_unhovered_unit_draws_nothing_regardless_of_damage() {
        assert_eq!(unit_status_visibility(false, false), (false, false));
    }
}
