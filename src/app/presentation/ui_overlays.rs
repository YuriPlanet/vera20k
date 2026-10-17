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
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::bit_font::BitFont;
use crate::rules::house_colors::{HouseColorRamps, NO_REMAP};
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::Position;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
/// Authentic pip counts for unit health bars.
const UNIT_PIPS_VEHICLE: u32 = 17; // vehicles + aircraft
const UNIT_PIPS_INFANTRY: u32 = 8;

/// Horizontal pixel spacing between unit health pips (each pip drawn 2px apart).
const UNIT_PIP_STEP_X: f32 = 2.0;

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
        state.height_map(),
        crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
    )?;
    match hover.kind {
        HoverTargetKind::FriendlyStructure
        | HoverTargetKind::EnemyStructure
        | HoverTargetKind::FriendlyUnit
        | HoverTargetKind::EnemyUnit => Some((hover.stable_id, hover.kind)),
        HoverTargetKind::HiddenEnemy => None,
    }
}

/// The existing resolved hover target for either Techno health branch.
pub(crate) fn health_hover_target(state: &AppState, local_owner: Option<&str>) -> Option<u64> {
    health_bar_hover_target(state, local_owner).map(|(id, _)| id)
}

/// Whether a status-producing object draws its bracket background and pips.
///
/// Two separate rules in the original, and damage is not part of either. The
/// health bar itself is drawn when the object is selected *or* hovered, while
/// the PIPBRD bracket behind it is drawn only when the object is selected — so
/// a hovered-but-unselected unit shows a bare pip strip with no bracket, and an
/// unselected, unhovered unit shows nothing however badly damaged it is.
///
/// Returns `(draw_bracket_background, draw_health_pips)`.
pub(crate) fn status_visibility(selected: bool, hovered: bool) -> (bool, bool) {
    (selected, selected || hovered)
}

/// Health-bar admission shared by building pips, mobile pips and their extras.
///
/// Steam gamemd SHA3e81a617: Tactical6D907E/6D9111 marks a Display member for
/// Extras before drawing its body; 6D95D2..6D9789 dispatches that later sweep.
/// Techno6F5E27/6F5E37..6F5E87 then admits selected / ordinary hovered objects
/// to health-bar slot+44C (6F64A0). Neither arm retests the raw anchor's shroud
/// or discovery+41B. Hover already passed ProcessClickCoords' projected query.
/// Reuse the body admission owner for lifecycle/cloak instead of adding another
/// fog interpretation here. Native dispatch controls: health_hover.json.
fn health_bar_drawable(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    hovered_id: Option<u64>,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
) -> bool {
    let (_, draw_pips) = status_visibility(
        entity.selection.is_selected(),
        hovered_id == Some(entity.stable_id()),
    );
    if !draw_pips {
        return false;
    }
    admitted_techno_observer(sim, entity, local_owner, rules).is_some()
}

/// Retained Display membership and the shared tactical draw admission.
/// Return the already derived observer facts for native Extras queries.
fn admitted_techno_observer(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    local_owner: Option<&str>,
    rules: Option<&RuleSet>,
) -> Option<crate::render::draw_state::ObserverDrawContext> {
    sim.display_layers().layer_of(entity.stable_id())?;
    let local_owner_id = local_owner.and_then(|owner| sim.interner.get(owner));
    let observer = crate::app::presentation::instances::observer_draw_context(
        sim,
        entity,
        local_owner,
        local_owner_id,
        rules,
    );
    crate::app::presentation::instances::tactical_entity_admission(
        crate::app::presentation::instances::TacticalEntityPurpose::Drawing,
        entity,
        sim.interner.resolve(entity.owner()),
        local_owner,
        local_owner_id,
        &sim.fog,
        false,
        sim.session.binary_frame,
        0,
        observer,
    )?;
    Some(observer)
}

/// Native `DrawHealthBar @0x006F64A0` guards its class `vt+0x450` pip/status
/// tail after the health strip. It admits CurrentPlayer's allies directly; the
/// other arm tests the object's current-player display bit at Techno+0x210.
/// That bit has no Rust owner yet, so this covers the common current-owned /-
/// allied path and leaves the non-allied display-bit arm as a bounded residual.
/// Keep this gate separate from health-strip admission.
fn entity_status_pips_drawable(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    hovered_entity_id: Option<u64>,
    current_house_id: Option<crate::sim::intern::InternedId>,
    rules: Option<&RuleSet>,
) -> bool {
    let Some(current_house_id) = current_house_id else {
        return false;
    };
    // House4F9A50 reads the ASKING owner's ally bits (6F6A58..8C).
    health_bar_drawable(
        sim,
        entity,
        hovered_entity_id,
        Some(sim.interner.resolve(current_house_id)),
        rules,
    ) && crate::map::houses::is_allied_with(
        &sim.house_alliances,
        sim.interner.resolve(entity.owner()),
        sim.interner.resolve(current_house_id),
    )
}

/// Project the input-owned control-group membership onto the live objects that
/// reach the status-pip draw. The membership remains owned by
/// `MatchInputState`; this transient index only makes each frame's lookup
/// linear in the retained Display list plus the ten group lists.
///
/// Native `DrawHealthBar @0x006F64A0` calls the class pip-scale slot
/// (`vtable +0x450`), whose tail reads Techno `+0x214` at
/// `DrawPipScalePips @0x00709A90`. Native `+0x214` is a zero-based slot; Rust's
/// input-owned control groups are indexed by the registered hotkey digit
/// (`TeamCreate_1` stores slot 1 and `TeamCreate_10` stores slot 0), so the
/// displayed digit is the Rust slot itself. The native caller's selected/hovered
/// admission is shared with the health strip for units, infantry, and buildings.
/// The relationship gate is pip-only; mobile labels reuse retained body
/// admission. Building labels retain their separate existing plain visibility
/// policy. Stale stable IDs and concealed bodies naturally produce no label.
pub(crate) fn control_group_status_labels(
    sim: &crate::sim::world::Simulation,
    control_groups: &[Vec<u64>],
    hovered_entity_id: Option<u64>,
    current_house_id: Option<crate::sim::intern::InternedId>,
    ignore_visibility: bool,
    rules: Option<&RuleSet>,
) -> Vec<(u64, char)> {
    let mut slot_by_id = HashMap::new();
    for (slot, members) in control_groups.iter().take(10).enumerate() {
        for &stable_id in members {
            // The input owner maintains unique membership. Last-slot choice
            // only makes malformed/restored duplicates deterministic; it
            // does not reconstruct a native assignment history.
            slot_by_id.insert(stable_id, slot);
        }
    }

    crate::app::presentation::instances::tactical_entity_encounter_order(sim)
        .into_iter()
        .filter_map(|stable_id| {
            let entity = sim.entities().get(stable_id)?;
            let building_tiberium_tail = entity.category() == EntityCategory::Structure
                && rules
                    .and_then(|rules| sim.interner.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| {
                        object.pip_scale == crate::rules::object_type::PipScale::Tiberium
                    });
            if entity.sinking.is_active()
                || entity.passenger_role.is_inside_transport()
                //709B61 skips the entire Building status tail for Tiberium2.
                || building_tiberium_tail
                || !entity_status_pips_drawable(
                    sim,
                    entity,
                    hovered_entity_id,
                    current_house_id,
                    rules,
                )
                || (entity.category() == EntityCategory::Structure
                    && !status_entity_visible_plain(
                        current_house_id,
                        &sim.fog,
                        &entity.position,
                        entity.owner(),
                        ignore_visibility,
                    ))
            {
                return None;
            }
            let slot = *slot_by_id.get(&stable_id)?;
            let digit = char::from(b'0' + slot as u8);
            Some((stable_id, digit))
        })
        .collect()
}

/// One parent's fill/outline followed by its GAME.FNT glyphs. Both materials
/// bypass A/Z; each native caller retains its own phase and parent order.
#[derive(Debug, Default)]
pub(crate) struct StatusTextOverlayInstances {
    pub rectangles: Vec<SpriteInstance>,
    pub text: Vec<SpriteInstance>,
}

/// One label's two material spans in a frame-local upload. Rectangle and
/// glyph stores stay together when several Building labels overlap.
#[derive(Debug)]
pub(crate) struct StatusTextRun {
    pub rectangles: std::ops::Range<u32>,
    pub text: std::ops::Range<u32>,
}

/// BuildingClass::Array-ordered text emitted once after Ground2. This is draw
/// data only; ProductionState owns primary identities and PowerState owns the
/// retained House totals.
#[derive(Debug, Default)]
pub(crate) struct BuildingStatusTextInstances {
    pub instances: Vec<SpriteInstance>,
    pub runs: Vec<StatusTextRun>,
}

impl BuildingStatusTextInstances {
    fn push(&mut self, label: StatusTextOverlayInstances) {
        let start = self.instances.len() as u32;
        self.instances.extend(label.rectangles);
        let rectangles_end = self.instances.len() as u32;
        self.instances.extend(label.text);
        self.runs.push(StatusTextRun {
            rectangles: start..rectangles_end,
            text: rectangles_end..self.instances.len() as u32,
        });
    }
}

fn status_text_tint(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    house_color_map: &HouseColorMap,
    house_color_ramps: &HouseColorRamps,
) -> [f32; 3] {
    let owner = sim.interner.resolve(entity.owner());
    let color = house_color_map.get(owner).copied().unwrap_or(NO_REMAP);
    //70A8xx/70ABxx/70ACxx read House+56F9, not ColorScheme UI text RGB.
    let [red, green, blue] =
        crate::render::palette_light::house_color_rgb(house_color_ramps, color);
    let [red, green, blue, _] =
        crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
            .quantize_rgba8([red, green, blue, 255]);
    [red, green, blue].map(|channel| f32::from(channel) / 255.0)
}

/// The shared7BB020/7BAD90/434120 material geometry. Callers supply their
/// already measured/clipped backing and native glyph origin; group and
/// Building labels deliberately use different rectangles and alignment.
fn status_text_instances(
    font: &BitFont,
    text: &str,
    origin: [f32; 2],
    [rect_x, rect_y, rect_width, rect_height]: [i32; 4],
    camera: [f32; 2],
    tint: [f32; 3],
) -> StatusTextOverlayInstances {
    let mut label = StatusTextOverlayInstances::default();
    if rect_width > 0 && rect_height > 0 {
        let rect_x = rect_x as f32 + camera[0];
        let rect_y = rect_y as f32 + camera[1];
        let rect_width = rect_width as f32;
        let rect_height = rect_height as f32;
        label.rectangles.push(SpriteInstance {
            position: [rect_x, rect_y],
            size: [rect_width, rect_height],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            tint: [0.0, 0.0, 0.0],
            alpha: 1.0,
            ..Default::default()
        });
        //7BADC0 uses inclusive endpoints. Avoid zero-area vertical quads
        // when clipping retains only a one- or two-pixel row.
        let edges = [
            ([rect_x, rect_y], [rect_width, 1.0]),
            ([rect_x, rect_y + rect_height - 1.0], [rect_width, 1.0]),
            ([rect_x, rect_y + 1.0], [1.0, rect_height - 2.0]),
            (
                [rect_x + rect_width - 1.0, rect_y + 1.0],
                [1.0, rect_height - 2.0],
            ),
        ];
        let count = if rect_height > 2.0 { 4 } else { 2 };
        label.rectangles.extend(
            edges[..count]
                .iter()
                .map(|&(position, size)| SpriteInstance {
                    position,
                    size,
                    uv_origin: [0.0, 0.0],
                    uv_size: [1.0, 1.0],
                    tint,
                    alpha: 1.0,
                    ..Default::default()
                }),
        );
    }
    label.text = font.build_text(text, origin[0], origin[1], 1.0, 0.0, tint, [0.0, 0.0]);
    label
}

/// Emit the selected/hovered unit's control-group digit with the shared GAME.FNT
/// atlas. Membership is read from the input owner; the per-frame output is only
/// draw data.
///
/// For the regular non-building call at native `DrawPipScalePips @ 0x00709A90`,
/// `DrawHealthBar @ 0x006F64A0` passes the screen point after `x-10,y+10` at
/// `0x006F6AAB`. The group tail then uses `x-4,y-0x27` only for Infantry
/// (`WhatAmI()==0xF`), and `x-4,y-0x24` for other classes. Relative to the
/// `0x006F6AB0` caller's input point, the final text origins are Unit
/// `(-14,-0x1A)` and Infantry `(-14,-0x1D)`. Native builds that point from the type vtable `+0x7C`
/// coordinate through `0x0041C230` and `0x006D1F10`; Rust uses the shared
/// tactical entity screen-position owner. Buildings enter the same slot with
/// the plain screen point plus the projected half-foundation offset from
/// `Dimension2`; the non-Infantry tail then applies `(-4,-0x24)`. The
/// selection-bracket delta is not part of the native group-number tail.
pub(crate) fn build_control_group_status_render_instances(
    sim: &crate::sim::world::Simulation,
    control_groups: &[Vec<u64>],
    hovered_entity_id: Option<u64>,
    current_house_id: Option<crate::sim::intern::InternedId>,
    ignore_visibility: bool,
    house_color_map: &HouseColorMap,
    house_color_ramps: &HouseColorRamps,
    font: &BitFont,
    camera: [f32; 2],
    sw: f32,
    sh: f32,
    tactical_clip: [i32; 4],
    rules: Option<&RuleSet>,
) -> BTreeMap<u64, StatusTextOverlayInstances> {
    let labels = control_group_status_labels(
        sim,
        control_groups,
        hovered_entity_id,
        current_house_id,
        ignore_visibility,
        rules,
    );
    let mut parents = BTreeMap::new();
    for (stable_id, digit) in labels {
        let Some(entity) = sim.entities().get(stable_id) else {
            continue;
        };
        // Tactical6D9676/6D9691 uses GetCoords and integer XYZ projection,
        // shared with this parent's health/rank producer.
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let (mut screen_x, mut screen_y) =
            crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        let building = entity.category() == EntityCategory::Structure;
        if building {
            //6F659E's third Dimension2 projection deliberately uses Z=0.
            // Production reads the canonical type query; reader-less fixtures
            // supply the retained foundation dimensions as an explicit prior.
            let [width, height] = rules
                .and_then(|rules| {
                    sim.interner
                        .object_type(entity.type_ref(), rules)
                        .map(|object| {
                            let [width, height, _] = rules.building_dimensions(object);
                            [width, height]
                        })
                })
                .unwrap_or_else(|| {
                    crate::rules::foundation::foundation_dimensions_leptons(
                        entity.type_facts.foundation(),
                    )
                });
            let (foundation_x, foundation_y) = crate::util::lepton::project_absolute_lepton_xy(
                -(width - width / 2),
                height - height / 2,
            );
            screen_x += foundation_x as f32;
            screen_y += foundation_y as f32;
        }
        // `DrawHealthBar` pre-adjusts non-buildings by (-10,+10); the shared
        // tail subtracts X=4 and only gives Infantry its additional -3 Y.
        let (caller_offset_x, caller_offset_y) = if building { (0.0, 0.0) } else { (-10.0, 10.0) };
        let tail_offset_y = if entity.category() == EntityCategory::Infantry {
            -0x27 as f32
        } else {
            -0x24 as f32
        };
        let text = digit.to_string();
        let (x, y) = (
            screen_x + caller_offset_x - 4.0,
            screen_y + caller_offset_y + tail_offset_y,
        );
        let width = font.text_width(&text) as f32;
        let height = font.glyph_height();
        if !in_view(x, y, width, height, camera[0], camera[1], sw, sh, 48.0) {
            continue;
        }
        let tint = status_text_tint(sim, entity, house_color_map, house_color_ramps);
        // The original measures the text rectangle, clips it to the tactical
        // surface, then expands the clipped result one pixel to the left for
        // its opaque black fill and House-color outline. All rectangles here
        // use the shared xywh owner; the GPU scissor remains the final clip.
        //70A75E..70A776 calls the same4A59E0 owner as PRIMARY, with
        // flags49 and signed margins[2,-2] (GAME.FNT cell17 -> height13).
        let measure_rect = font.compute_text_rect(
            &text,
            [
                (x - camera[0]).round() as i32,
                (y - camera[1]).round() as i32,
            ],
            0x49,
            [2, -2],
        );
        let mut clipped = crate::util::rect::clip_rect(measure_rect, tactical_clip);
        if clipped[2] > 0 && clipped[3] > 0 {
            clipped[0] -= 1;
            clipped[2] += 1;
        }
        // These are world-pixel coordinates; the tactical surface camera
        // applies the camera offset at draw time, like the neighboring pips.
        parents.insert(
            stable_id,
            status_text_instances(font, &text, [x, y], clipped, camera, tint),
        );
    }
    parents
}

/// AppState adapter for the production tactical UI build.
pub(crate) fn build_control_group_status_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> BTreeMap<u64, StatusTextOverlayInstances> {
    let (Some(sim), Some(rules)) = (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation),
        state.rules(),
    ) else {
        return BTreeMap::new();
    };
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner
        .as_deref()
        .and_then(|name| sim.interner.get(name));
    let hovered_entity_id =
        health_bar_hover_target(state, local_owner.as_deref()).map(|(stable_id, _)| stable_id);
    let (tactical_width, tactical_height) =
        crate::app::input::camera::tactical_viewport_size(state);
    let zoom = state.match_state.input.zoom_level;
    let tactical_clip = [
        0,
        0,
        (tactical_width as f32 / zoom).round() as i32,
        (tactical_height as f32 / zoom).round() as i32,
    ];
    build_control_group_status_render_instances(
        sim,
        &state.match_state.input.control_groups,
        hovered_entity_id,
        local_owner_id,
        state.match_state.sandbox_full_visibility,
        &state.match_state.match_presentation.house_color_map,
        &rules.house_color_ramps,
        &state.renderer.bit_font,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
        sw,
        sh,
        tactical_clip,
        Some(rules),
    )
}

/// Building43E7B0 -> Techno70AA60 Power/Drain and PRIMARY, called from
/// Tactical6D957A after Ground2. The caller supplies this frame's native +99 body
/// opportunities; successful glyph/body pixels are not the admission fact.
/// BuildingClass::Array order is the existing monotonic construction-ID
/// order, independent of the retained Display sort. No RNG/timer/detach or
/// persistent state is changed. Native pixels: building_power_status.json and
/// primary_factory_status.json under tools/procedural_drawing_oracle.
pub(crate) fn build_building_status_render_instances(
    sim: &crate::sim::world::Simulation,
    body_opportunities: &BTreeSet<u64>,
    current_house_id: crate::sim::intern::InternedId,
    house_color_map: &HouseColorMap,
    house_color_ramps: &HouseColorRamps,
    font: &BitFont,
    csf: &crate::assets::csf_file::CsfFile,
    camera: [f32; 2],
    tactical_clip: [i32; 4],
    rules: &RuleSet,
) -> BuildingStatusTextInstances {
    let mut labels = BuildingStatusTextInstances::default();
    let current_house = sim.interner.resolve(current_house_id);
    for entity in sim.entities().values() {
        //6D944D/9458 tests Mark+74 and body-opportunity+99.43E7C7
        //requires selected, including when the object is also hovered.
        if entity.category() != EntityCategory::Structure
            || !body_opportunities.contains(&entity.stable_id())
            || !entity.lifecycle.cell_marked()
            || !entity.selection.is_selected()
            || !crate::map::houses::is_allied_with(
                &sim.house_alliances,
                sim.interner.resolve(entity.owner()),
                current_house,
            )
        {
            continue;
        }
        //43E7EF's separate non-allied Techno+210 display-bit producer is
        //still an explicit status-owner residual; do not invent its state.
        let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
            continue;
        };
        let primary = sim.production.is_primary_factory(entity.stable_id());
        //70AA88..70AAA4 admits the positive type Power arm only when the
        //primary latch is clear.70ABEA then tests that latch for PRIMARY.
        if !primary && object.power <= 0 {
            continue;
        }
        let location = crate::sim::movement::ground_pose::object_location(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        //6D9472 calls virtual+AC=459EF0, not GetCoords447AC0. Neither
        //foundation half-dimensions nor the group tail's offsets apply.
        let (render, _) =
            crate::sim::movement::ground_pose::building_render_order_parts(location, false, false);
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(render.x, render.y, render.z);
        let anchor = [
            (x - camera[0]).round() as i32,
            (y - camera[1]).round() as i32,
        ];
        //6D94DD..9516's coarse caller bounds precede the actual text clip.
        if anchor[0] < -360
            || anchor[0] > tactical_clip[2] + 360
            || anchor[1] < -180
            || anchor[1] > tactical_clip[3] + 180
        {
            continue;
        }
        let text = if primary {
            //70AC24's45EC90 uses foundation CELL width, not Dimension2 leptons.
            let key = if crate::rules::foundation::foundation_dimensions(&object.foundation).0 == 1
            {
                "TXT_PRI"
            } else {
                "TXT_PRIMARY"
            };
            csf.text(key)
        } else {
            //70AAAA..70AAEA reads the owner's retained IHouse4F6A00/4F6A10
            //slots and formats physical TXT_POWER_DRAIN2. Drawing neither
            //reassesses power nor substitutes the selected building's output.
            let (output, drain) = crate::sim::production::power_balance_for_owner(
                sim,
                rules,
                sim.interner.resolve(entity.owner()),
            );
            Cow::Owned(format_csf(
                &csf.text("TXT_POWER_DRAIN2"),
                &[
                    CsfArg::Int(i64::from(output)),
                    CsfArg::Int(i64::from(drain)),
                ],
            ))
        };
        let measure = font.compute_text_rect(&text, anchor, 0x149, [4, 2]);
        let clipped = crate::util::rect::clip_rect(measure, tactical_clip);
        //70AB0A/70AC60 and4A5EF9 share the whole-string center. Unlike
        //group, these labels do not expand the clipped backing one pixel left.
        let origin = [
            measure[0].wrapping_add(4) as f32 + camera[0],
            measure[1].wrapping_add(2) as f32 + camera[1],
        ];
        labels.push(status_text_instances(
            font,
            &text,
            origin,
            clipped,
            camera,
            status_text_tint(sim, entity, house_color_map, house_color_ramps),
        ));
    }
    labels
}

/// AppState supplies current resources and the existing tactical viewport;
/// simulation owners supply primary identity and retained power, and the
/// per-frame body plan supplies the current opportunities.
pub(crate) fn build_building_status_instances(
    state: &AppState,
    body_opportunities: &BTreeSet<u64>,
) -> BuildingStatusTextInstances {
    let (Some(runtime), Some(rules), Some(csf)) = (
        state.match_state.sim_runtime.as_ref(),
        state.rules(),
        state.process_assets.csf.as_ref(),
    ) else {
        return BuildingStatusTextInstances::default();
    };
    let sim = &runtime.simulation;
    let Some(current_house) = preferred_local_owner_name(state)
        .as_deref()
        .and_then(|name| sim.interner.get(name))
    else {
        return BuildingStatusTextInstances::default();
    };
    let (width, height) = crate::app::input::camera::tactical_viewport_size(state);
    let zoom = state.match_state.input.zoom_level;
    build_building_status_render_instances(
        sim,
        body_opportunities,
        current_house,
        &state.match_state.match_presentation.house_color_map,
        &rules.house_color_ramps,
        &state.renderer.bit_font,
        csf,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
        [
            0,
            0,
            (width as f32 / zoom).round() as i32,
            (height as f32 / zoom).round() as i32,
        ],
        rules,
    )
}

/// Which PIPS.SHP frame a self-healing non-building's health strip carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelfHealPip {
    /// Frame `0x0D`, the red cross: an Infantry, or a Unit whose type is
    /// `Organic`.
    Organic,
    /// Frame `0x14`, the white wrench: a Unit whose type is not `Organic`.
    Units,
}

impl SelfHealPip {
    /// The PIPS.SHP frame index this arm draws (`ebx` at `0x0070A573` /
    /// `0x0070A5DF`).
    pub(crate) const fn frame(self) -> usize {
        match self {
            Self::Organic => 0x0D,
            Self::Units => 0x14,
        }
    }
}

///70A5EC chooses the offset by RTTI, independently of the icon frame.
/// Health6F6A8E supplies(-10,+10),709B14 subtracts5 X and adds11 for
/// Infantry.70A5F1 adds(38,-32) for every Unit, even Organic, while
///70A60A adds(19,-35) for Infantry. Type+3E0 bracket delta is not passed.
fn self_heal_pip_offset(category: EntityCategory) -> (f32, f32) {
    if category == EntityCategory::Unit {
        (23.0, -22.0)
    } else {
        (15.0, -25.0)
    }
}

/// The self-heal status pip an object draws, or `None`.
///
/// Native `DrawPipScalePips` (`0x0070A50C..0x0070A6BB`): an Infantry
/// (`WhatAmI()==0xF`) takes the organic arm directly, and a Unit
/// (`WhatAmI()==1`) whose type byte `+0xD97` (`Organic`) is set falls through
/// into the same arm; a Unit whose type is not organic takes the units arm;
/// every other class passes both tests and draws nothing. Each arm first
/// requires its owner house's counter — `HasInfSelfHeal @ 0x0050D9C0` on
/// `House+0x164`, `HasUnitSelfHeal @ 0x0050D9D0` on `House+0x168`.
pub(crate) fn self_heal_pip(
    category: EntityCategory,
    organic: bool,
    self_heal_infantry: i32,
    self_heal_units: i32,
) -> Option<SelfHealPip> {
    let (arm, count) = match category {
        EntityCategory::Infantry => (SelfHealPip::Organic, self_heal_infantry),
        EntityCategory::Unit if organic => (SelfHealPip::Organic, self_heal_infantry),
        EntityCategory::Unit => (SelfHealPip::Units, self_heal_units),
        EntityCategory::Structure | EntityCategory::Aircraft => return None,
    };
    (count > 0).then_some(arm)
}

// DEFERRED members of this row's overlay set, recorded with what a follow-up
// needs. None of them requires new texture infrastructure — pips.shp and
// pips2.shp are both already loaded, and `DrawPipScalePips` swaps the shape
// handle from `DAT_00AC147C` (PIPS.SHP) to `DAT_00AC1480` (PIPS2.SHP) at
// 0x00709AEF for every non-building.
//
// * **Wrapped ammo pips**,70A421..4E6: PipWrap!=0 selects signed-division
//   stacks and other frames. Stock ORCA retains PipWrap0 and uses the common
//   strip below. The wrapped configuration is dormant in the stock Ammo types,
//   not proven unreachable. Passenger fractional/mixed sizes and other
//   transport lifecycle paths retain their explicit owner residuals.
//
// * **Spawn pips**, from `TypeClass+0xD5C` via
//   `SpawnManagerClass__CountDockedSpawns`. Trigger: a selected Kirov/carrier
//   with docked spawns. Player effect: spawned aircraft count is absent.
//   Frequency: rare. Downstream risk: none. The group-number tail is projected
//   through the live input membership in `build_control_group_status_instances`
//   above; the self-heal status pip is ported below.
//
/// The repair wrench's WRENCH.SHP frame at `frame`
/// (`TechnoClass::DrawExtras 0x006F52D8..0x006F532B`): a cycle of
/// `SpeedNormalize(14) / 4` frames (at least 2) spread over frames 0..=6,
/// both signed IDIVs.
fn repair_wrench_frame(frame: i32, options: &crate::sim::game_options::GameOptions) -> i32 {
    let cycle = (options.speed_normalize(14) / 4).max(2);
    frame % cycle * 6 / (cycle - 1)
}

/// Frame-local screen viewport for Techno status emission.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StatusViewport {
    pub camera: [f32; 2],
    pub size: [f32; 2],
    /// Existing sandbox view policy; ordinary retail views ask the Cell owner.
    pub ignore_visibility: bool,
}

/// One Display parent's clock, wrench, rank, health and pip commands in Extras order.
#[derive(Debug, Default)]
pub(crate) struct TechnoExtras {
    ///6F519B..524E: BOMBCURS/MOUSEPAL N1 prefix, before every other status.
    pub bomb_clock: Option<SpriteInstance>,
    ///6F5253..5345: WRENCH/MOUSEPAL N1 prefix, before rank and Building lines.
    pub wrench: Option<SpriteInstance>,
    pub rank: Option<SpriteInstance>,
    pub background: Option<SpriteInstance>,
    /// Plain600 PIPS commands: health followed by any garrison occupants.
    pub fills: Vec<SpriteInstance>,
    /// Plain600 PIPS2 scale commands, after this parent's health.
    pub scale: Vec<SpriteInstance>,
    /// Existing House-owned self-heal marker, between scale and group text.
    pub self_heal: Option<SpriteInstance>,
    ///709A90's group tail follows this parent's pip scale, not a global sweep.
    pub group: StatusTextOverlayInstances,
}

/// One Techno DrawExtras6F5190 parent: clock, wrench, rank, then health6F64A0.
/// Rank70A990 precedes the selected/hovered health gate at6F5388. It uses
/// the shared cloak/disguise and raw Veterancy owners, with opaque status art.
/// Ordinary MTNK is executed in procedural_drawing_oracle/unit_rank.json;
/// other locomotor +68 overrides and promoted-Building lifecycle remain open.
///
/// Building6F64A0..6839 projects Dimension2's upper edge and draws raw PIPS
/// frames1/2/4 followed by frame0. Infantry6F6852..68E3 uses8 pips and PIPBRD1:
/// pip(-5,delta-24), background(11,delta-25). Other mobile6F68E8..6976 uses17
/// and PIPBRD0: pip(-15,delta-25), background(1,delta-26). The common centered
/// resource owns every frame's original origin and index plane (4AED70).
/// Source/executable controls: procedural_drawing_oracle/building_health.json
/// and unit_health.json. Stock selected/hovered GAPOWR and mobile coverage;
/// conditional pip/status tails remain with their existing owners.
/// Clock prefix6F519B..524E and its body/parent overlap execute in
/// procedural_drawing_oracle/bomb_clock.json with physical BOMBCURS/MOUSEPAL.
/// No RNG, timer, detach or retained presentation state changes here.
pub(crate) fn techno_extras_for_parent(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    rules: Option<&RuleSet>,
    overlay: &crate::render::selection_overlay::SelectionOverlay,
    hovered_id: Option<u64>,
    local_owner: Option<&str>,
    viewport: StatusViewport,
) -> TechnoExtras {
    let mut strip = TechnoExtras::default();
    // DrawExtras6F51AA precedes every status family. Display/lifecycle/body
    // admission remains the existing shared owner, independent of selection.
    if entity.sinking.is_active() || entity.passenger_role.is_inside_transport() {
        return strip;
    }
    //6F519B..524E clock and6F5253..5345 wrench precede observer/selection
    // admission. Both use the existing per-parent MOUSEPAL N1 material; the
    // clock's886FA0 clip is the tactical viewport, not a sprite's body bounds.
    let bomb_visible = entity.bomb.is_some()
        && (viewport.ignore_visibility
            || local_owner
                .and_then(|name| sim.interner.get(name))
                .is_some_and(|viewer| sim.bomb_seen_by(entity.stable_id(), viewer)));
    let repairing = entity.category() == EntityCategory::Structure && entity.repairing.is_on();
    if (bomb_visible || repairing)
        && (viewport.ignore_visibility
            || local_owner
                .and_then(|name| sim.interner.get(name))
                .zip(sim.resolved_terrain.as_ref())
                .is_some_and(|(viewer, terrain)| {
                    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
                    !crate::sim::vision::cell_is_shrouded(
                        &sim.fog,
                        &cells,
                        viewer,
                        (entity.position.rx(), entity.position.ry()),
                    )
                }))
    {
        // Virtual+AC: Building459EF0 uses Location-128XY; the ordinary
        // mobile5F65A0 uses its existing GetCoords owner. Tactical6D2140
        // projects integer XYZ, independently of the status caller's point.
        let render = if entity.category() == EntityCategory::Structure {
            let location = crate::sim::movement::ground_pose::object_location(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            crate::sim::movement::ground_pose::building_render_order_parts(location, false, false).0
        } else {
            crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            )
        };
        let point = crate::util::lepton::absolute_leptons_to_screen(render.x, render.y, render.z);
        let prefix = |shape: &crate::render::selection_overlay::CenteredShapeStrip, frame| {
            let mut instance = shape.instance(frame, point, 0.0)?;
            // Init52C1D8 MOUSEPAL N1; E00/493DF0 consumes A with zero
            // row offsets and performs opaque packed stores, bypassing Z.
            instance.palette_light = crate::render::palette_light::PaletteLight::plain(1, 1000);
            in_view(
                instance.position[0],
                instance.position[1],
                instance.size[0],
                instance.size[1],
                viewport.camera[0],
                viewport.camera[1],
                viewport.size[0],
                viewport.size[1],
                48.0,
            )
            .then_some(instance)
        };
        if bomb_visible
            && let (Some(bomb), Some(rules), Some(shape)) =
                (entity.bomb, rules, overlay.bomb_clock())
            && let Some(frame) = bomb.clock_frame(
                sim.session.binary_frame as i32,
                rules.combat_damage.ivan_timed_delay,
                rules.combat_damage.ivan_icon_flicker_rate,
            )
            && let Ok(frame) = usize::try_from(frame)
        {
            strip.bomb_clock = prefix(shape, frame);
        }
        if repairing
            && let Some(shape) = overlay.repair_wrench()
            && let Ok(frame) = usize::try_from(repair_wrench_frame(
                sim.session.binary_frame as i32,
                &sim.session.game_options,
            ))
        {
            strip.wrench = prefix(shape, frame);
        }
    }
    let Some(observer) = admitted_techno_observer(sim, entity, local_owner, rules) else {
        return strip;
    };
    // Tactical6D9676 dispatches Object5F65A0/Building447AC0 GetCoords;
    //6D9691..9726 projects integer XYZ before subtracting the camera.
    let coord =
        crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    let (sx, sy) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    let object = rules.and_then(|rules| sim.interner.object_type(entity.type_ref(), rules));
    let instance = |shape: &crate::render::selection_overlay::CenteredShapeStrip,
                    frame: usize,
                    point: (f32, f32),
                    depth: f32| {
        let mut instance = shape.instance(frame, point, depth)?;
        // E00 rank/background reads tactical A; 600 fills bypass A. The renderer
        // chooses those bindings; both use the same plain N53 Convert.
        instance.palette_light = crate::render::palette_light::PaletteLight::plain(53, 1000);
        in_view(
            instance.position[0],
            instance.position[1],
            instance.size[0],
            instance.size[1],
            viewport.camera[0],
            viewport.camera[1],
            viewport.size[0],
            viewport.size[1],
            48.0,
        )
        .then_some(instance)
    };
    //6F534E..5382: +C8's canonical observer-disguise query is distinct
    // from drawing disguise art; +68 uses VisualCharacter's screen(0,NULL)
    // query. The first screen argument means graphical client, not force_sensor.
    let observer_id = local_owner.and_then(|owner| sim.interner.get(owner));
    let disguised = observer_id.is_some_and(|viewer| {
        entity.is_disguised_to(
            viewer,
            Some(&sim.fog),
            Some(&sim.house_alliances),
            &sim.interner,
        )
    });
    let character = entity.visual_character(
        observer.cloaking_stages,
        observer.invisible,
        crate::sim::cloak_disguise::VisualCharacterQuery::screen(
            true,
            observer.observer_present,
            observer.detects_cloak,
            observer.owner_is_mutually_allied,
            observer.is_campaign,
            false,
        ),
    );
    if !disguised && character != 5 {
        use crate::sim::combat::veterancy::{raw_is_elite, raw_is_negative, raw_is_veteran};
        //70A999's +88 getter is discarded (Unit741490, Building459EE0);
        // there is no Trainable gate.70A9B2/C2/D2 tests the raw accumulator,
        // not the cached/quantized rank. A negative value selects frame19.
        let raw = entity.veterancy.raw();
        let frame = if raw_is_veteran(raw) {
            Some(14)
        } else if raw_is_elite(raw) {
            Some(15)
        } else if raw_is_negative(raw) {
            Some(19)
        } else {
            None
        };
        if let (Some(frame), Some(shape)) = (frame, overlay.pips()) {
            let (dx, dy) = if entity.category() == EntityCategory::Infantry {
                (5.0, 2.0)
            } else {
                (10.0, 6.0)
            };
            //70AA20..AA4F: opaque PIPS E00, plain53/1000, A-consuming/no-Z.
            // Its -2 Z-adjust is unused by this leaf; do not inherit body FX.
            strip.rank = instance(shape, frame, (sx + dx, sy + dy), 0.0006).map(|mut rank| {
                rank.z_adjust = -2.0;
                rank
            });
        }
    }
    let (draw_background, draw_health) = status_visibility(
        entity.selection.is_selected(),
        hovered_id == Some(entity.stable_id()),
    );
    if !draw_health {
        return strip;
    }
    // Both6F65D7 and6F697C use Object5F5C60 then multiply/ftol7C5F00.
    // Keep the shared health/numeric owners and the signed [1,N] clamp.
    let filled_count = |strength: i32, count: i32| {
        use crate::util::native_x87::MaskedX87Chop53 as X87;
        X87::ftol_i32_low_masked(X87::mul(
            entity.health.ratio(strength),
            X87::load_i32(count),
        ))
        .clamp(1, count)
    };
    if entity.category() == EntityCategory::Structure {
        let (Some(rules), Some(object), Some(shape)) = (rules, object, overlay.pips()) else {
            return strip;
        };
        // BuildingType464AF0 owns Foundation*256 and fixed ART Height*104.
        //6F6536/656E project local offsets through Tactical6D1F10, without
        // the absolute projection's camera or +15 world-row origin.
        let [width, height, z] = rules.building_dimensions(object);
        let left = (width - width / 2).wrapping_neg();
        let back = height - height / 2;
        let (ax, ay) = crate::util::lepton::project_absolute_lepton_xy(left, back);
        let (_, by) = crate::util::lepton::project_absolute_lepton_xy(left, back.wrapping_neg());
        let upper_y = ay - crate::util::native_x87::adjust_for_z_standard(z);
        // Both upper projections carry the same Z; it cancels in6F65BC..CF.
        let num_pips = (ay - by) / 2;
        if num_pips <= 0 {
            return strip;
        }
        let filled = filled_count(object.strength, num_pips);
        //6F660A/661F use the existing health-category predicates; zero HP
        // does not satisfy IsRed. Raw frame selection is green1/yellow2/red4.
        let frame = match crate::sim::combat::health_category(
            entity.health.current(),
            object.strength,
            rules.general.condition_red,
            rules.general.condition_yellow,
        ) {
            0 => 4,
            1 => 2,
            _ => 1,
        };
        let start = (
            sx + (ax + 4 * num_pips + 3) as f32,
            sy + (upper_y + 4 - 2 * num_pips) as f32,
        );
        strip.fills.reserve(num_pips as usize);
        //6F6658..6774: filled first, then empty frame0, walking [-4,+2].
        // Building PIPS600 bypasses A and Z; the replay owner preserves both
        // original first-Extras and late-Extras positions around the body.
        for index in 0..num_pips {
            strip.fills.extend(instance(
                shape,
                if index < filled { frame } else { 0 },
                (start.0 - (index * 4) as f32, start.1 + (index * 2) as f32),
                0.0006,
            ));
        }
        //6F659E projects the foundation edge with Z=0;6F682C hands it to
        //709A90. Garrison709C27..D17 reads raw OccupyPip, including frames1..5
        // produced by the shared native reader, not just person frames6..12.
        // Building6F67DC additionally admits CanBeOccupied regardless of
        // relationship. Type+157B defaults false45E07D;4600DA reads exact
        // CanBeOccupied81ADBC through ReadBool5295F0 with the current default.
        // Preserve this separate native Building policy.
        if (object.can_be_occupied
            || entity_status_pips_drawable(sim, entity, hovered_id, observer_id, Some(rules)))
            && object.show_occupant_pips
            && object.max_number_occupants > 0
        {
            let occupants = entity
                .passenger_role
                .cargo()
                .map(|cargo| cargo.passengers());
            for slot in 0..object.max_number_occupants {
                let frame = occupants
                    .and_then(|ids| ids.get(slot as usize))
                    .and_then(|id| sim.entities().get(*id))
                    .and_then(|passenger| sim.interner.object_type(passenger.type_ref(), rules))
                    .map_or(6, |passenger| passenger.occupy_pip() as usize);
                strip.fills.extend(instance(
                    shape,
                    frame,
                    (
                        sx + ax as f32 + 6.0 + (slot * 4) as f32,
                        sy + ay as f32 - 1.0 + (slot * 2) as f32,
                    ),
                    0.0006,
                ));
            }
        }
        return strip;
    }
    let delta = object.map_or(0.0, |object| object.pixel_selection_bracket_delta as f32);
    let infantry = entity.category() == EntityCategory::Infantry;
    let (num_pips, background_frame, background_offset, pip_offset) = if infantry {
        (UNIT_PIPS_INFANTRY, 1, [11.0, -25.0], [-5.0, -24.0])
    } else {
        (UNIT_PIPS_VEHICLE, 0, [1.0, -26.0], [-15.0, -25.0])
    };
    if draw_background && let Some(shape) = overlay.pipbrd() {
        strip.background = instance(
            shape,
            background_frame,
            (sx + background_offset[0], sy + delta + background_offset[1]),
            0.0006,
        );
    }
    let (Some(rules), Some(object), Some(shape)) = (rules, object, overlay.pips()) else {
        return strip;
    };
    let filled = filled_count(object.strength, num_pips as i32) as u32;
    //6F69B1..69F3 compares independently: <=red overrides <=yellow. These
    // are direct ratio comparisons, unlike Building's positive-HP red gate.
    use crate::util::native_x87::MaskedX87Ordering::Greater;
    let frame = if entity
        .health
        .compare_ratio(object.strength, rules.general.condition_red)
        != Greater
    {
        18
    } else if entity
        .health
        .compare_ratio(object.strength, rules.general.condition_yellow)
        != Greater
    {
        17
    } else {
        16
    };
    strip.fills.reserve(filled as usize);
    for index in 0..filled {
        strip.fills.extend(instance(
            shape,
            frame,
            (
                sx + pip_offset[0] + index as f32 * UNIT_PIP_STEP_X,
                sy + delta + pip_offset[1],
            ),
            0.0005,
        ));
    }
    //709B0F/6F6A8E: nonbuilding scale starts at health anchor(-15,+10),
    //with the Infantry-only+11 X. The shared raw-frame owner retains cargo
    //and native relationship admission; no global scale bucket remains.
    if let Some(art) = overlay.scale_pips() {
        let frames = scale_pip_frames(sim, entity, hovered_id, local_owner, rules);
        //70A3B7: unwrapped Ammo sits three pixels above the ordinary scale
        //anchor. Positive Passengers takes its earlier arm, even with Ammo.
        let scale_y = if object.passengers <= 0
            && object.pip_scale == crate::rules::object_type::PipScale::Ammo
        {
            7.0
        } else {
            10.0
        };
        for (slot, frame) in frames.into_iter().enumerate() {
            //709F50..A06F: Gunner draws slot0 normally, then advances an extra
            //two steps before slot1. Stock FV has one slot; shared callers
            //still preserve the literal later-slot displacement.
            let gunner_step = if object.passengers > 0 && object.gunner && slot > 0 {
                2
            } else {
                0
            };
            let x =
                sx - 15.0 + if infantry { 11.0 } else { 0.0 } + ((slot + gunner_step) * 4) as f32;
            strip
                .scale
                .extend(instance(art, frame, (x, sy + delta + scale_y), 0.0004));
        }
    }
    //70A50C..6BB: House admission precedes signed frame division, even for
    //a healthy object. Damaged remainder<=5 uses601 destination-halving;
    //otherwise the same icon/frame uses ordinary600. Native controls:
    //procedural_drawing_oracle/self_heal_status. No render timer is retained.
    if entity_status_pips_drawable(sim, entity, hovered_id, observer_id, Some(rules)) {
        let (infantry_heal, unit_heal) = sim.houses.get(&entity.owner()).map_or((0, 0), |house| {
            (house.self_heal_infantry(), house.self_heal_units())
        });
        if let Some(arm) =
            self_heal_pip(entity.category(), object.organic, infantry_heal, unit_heal)
        {
            let period = match arm {
                SelfHealPip::Organic => rules.general.self_heal_infantry_frames,
                SelfHealPip::Units => rules.general.self_heal_unit_frames,
            };
            let frame = sim.session.binary_frame as i32;
            let remainder = frame.checked_rem(period).unwrap_or_else(|| {
                panic!("native self-heal marker IDIV fault at 0070A549/0070A5B5: frame={frame} period={period}")
            });
            let selector = u8::from(remainder <= 5 && entity.health.current() < object.strength);
            let (offset_x, offset_y) = self_heal_pip_offset(entity.category());
            strip.self_heal = instance(shape, arm.frame(), (sx + offset_x, sy + offset_y), 0.0004)
                .map(|mut instance| {
                    instance.draw_state =
                        crate::render::draw_state::DrawState::from_native_selector(0, selector);
                    instance
                });
        }
    }
    strip
}

/// Raw PIPS2 frames after selected/hovered body admission and the shared
/// native pip-tail relationship gate. DrawHealthBar6F6A58 ->709A90.
/// Rendering only reads retained state; it never advances timers or RNG.
pub(crate) fn scale_pip_frames(
    sim: &crate::sim::world::Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    hovered_id: Option<u64>,
    local_owner: Option<&str>,
    rules: &RuleSet,
) -> Vec<usize> {
    if entity.category() == EntityCategory::Structure
        || entity.sinking.is_active()
        || entity.passenger_role.is_inside_transport()
        || !entity_status_pips_drawable(
            sim,
            entity,
            hovered_id,
            local_owner.and_then(|owner| sim.interner.get(owner)),
            Some(rules),
        )
    {
        return Vec::new();
    }
    let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
        return Vec::new();
    };
    //709D1D tests positive Passengers before the non-passenger PipScale arms.
    //GetPipMax716290 remains the shared type query, never cargo-vector length.
    if object.passengers > 0 {
        let slots = usize::try_from(object.pip_max(rules)).unwrap_or(0);
        let mut frames = vec![0; slots];
        let Some(cargo) = entity.passenger_role.cargo() else {
            return frames;
        };
        //473460 normally rereads current binary64 Size and truncates each sum.
        //The existing cargo owner retains integer admission-time sizes. Stock
        //FV+size-one infantry are equivalent; fractional/mixed sizes and type
        //reloads remain an explicit transport-owner residual, not a new cache.
        let total = cargo.total_size() as usize;
        if total > slots {
            return Vec::new();
        }
        let mut next = total;
        for &id in cargo.passengers() {
            let Some(passenger) = sim.entities().get(id) else {
                break;
            };
            let Some(kind) = sim.interner.object_type(passenger.type_ref(), rules) else {
                break;
            };
            let frame = match passenger.category() {
                EntityCategory::Infantry => kind.pip() as usize,
                EntityCategory::Unit => 5,
                _ => 1,
            };
            //709EB5..ECA/709F0A..F23 consume high slots with Size-1 fillers
            //BEFORE the passenger's own lower slot. Cargo head comes first.
            //The other-RTTI arm709F2C writes only frame1. Malformed links/
            //sizes cannot address outside this frame-local vector in Rust.
            if matches!(
                passenger.category(),
                EntityCategory::Infantry | EntityCategory::Unit
            ) {
                for _ in 1..kind.size {
                    let Some(index) = next.checked_sub(1) else {
                        break;
                    };
                    frames[index] = 3;
                    next = index;
                }
            }
            let Some(index) = next.checked_sub(1) else {
                break;
            };
            frames[index] = frame;
            next = index;
        }
        return frames;
    }
    if object.pip_scale == crate::rules::object_type::PipScale::Ammo {
        //709A90/70A345..41C: ordinary stock ORCA reaches the unwrapped
        //PIPS2 frame13 arm. Iterate GetPipMax slots but draw only positive
        //fill; depleted Ammo emits no empty outline. Draw flags600/noA/Z
        //are owned by the existing per-parent scale replay.
        //Nonzero PipWrap70A421..4E6 remains a separate dormant configuration.
        if object.pip_wrap() != 0 {
            return Vec::new();
        }
        let filled = entity
            .techno_pip_fill_level(object, rules)
            .clamp(0, object.pip_max(rules).max(0));
        return vec![13; filled as usize];
    }
    if object.pip_scale == crate::rules::object_type::PipScale::MindControl {
        //716290 case5 reads current primary Weapon.Damage, independently of
        //the manager's retained admission capacity.
        let maximum = object.pip_max(rules);
        let slots = usize::try_from(maximum).unwrap_or(0);
        let manager = entity.capture_manager.as_ref();
        let count = entity.techno_pip_fill_level(object, rules);
        //Canonical708C30 owns count/max rounding, including signed numeric
        //boundaries. Draw70A0B5 uses frame1 below fill, frame0 afterward.
        let mut frames: Vec<_> = (0..slots)
            .map(|index| usize::from((index as i64) < i64::from(count)))
            .collect();
        // IsOverloading4726C0 owns infinite/capacity/counter policy;
        //70A17F..1C2 adds exactly one frame3 (positive) or4 (idle).
        if let Some(active) = manager.and_then(|manager| manager.overload_pip_state()) {
            frames.push(if active { 3 } else { 4 });
        }
        return frames;
    }
    if object.pip_scale != crate::rules::object_type::PipScale::Tiberium {
        return Vec::new();
    }
    let Some(miner) = &entity.miner else {
        return Vec::new();
    };
    let gems = miner
        .cargo()
        .iter()
        .filter(|bale| bale.resource_type == crate::sim::miner::ResourceType::Gem)
        .count();
    let ore = miner.cargo().len() - gems;
    let maximum = object.pip_max(rules);
    let slots = usize::try_from(maximum).unwrap_or(0);
    //70A230..29C independently rounds Storage slot1 (gems) and (slot3+2)+0
    //(ore) through amount / current Type.Storage * GetPipMax + binary64 0.5.
    //Ambient0E7F is PC53/chop: stock4/40 remains just below0.5 after the
    //multiply, so rational half-up is wrong. Reuse the existing numeric owner;
    //original executable controls: passenger_pips.json/numeric_results.
    let count = |amount: usize| {
        let rounded =
            crate::sim::game_entity::rounded_pip_quantity(amount as i32, object.storage, maximum);
        usize::try_from(rounded).unwrap_or(0).min(slots)
    };
    let gem_pips = count(gems);
    let ore_pips = count(ore).min(slots - gem_pips);
    let mut frames = Vec::with_capacity(slots);
    frames.extend(std::iter::repeat_n(5, gem_pips));
    frames.extend(std::iter::repeat_n(2, ore_pips));
    frames.resize(slots, 0);
    frames
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
        state.rules(),
    ) else {
        return Vec::new();
    };
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let mut instances = Vec::new();

    for e in sim.entities().values() {
        if e.category() != EntityCategory::Structure || !e.selection.is_selected() {
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

/// Observation of the same cursor selection used by the production draw.
/// Map captures admit only a static arrow, so cursor wall time cannot alter it.
pub(crate) fn static_default_cursor(state: &AppState) -> bool {
    state.use_software_cursor()
        && active_cursor_sequence(state)
            .is_some_and(|(id, sequence)| id == CursorId::Default && sequence.interval_ms == 0)
}

pub(crate) fn build_software_cursor_instances(state: &AppState) -> Vec<SpriteInstance> {
    if !state.use_software_cursor() || !state.match_state.input.cursor_visibility.is_visible() {
        return Vec::new();
    }
    let Some((id, sequence)) = active_cursor_sequence(state) else {
        return Vec::new();
    };
    let frame = if id == CursorId::Default
        && crate::app::input::planning_mode::node_at_cursor(state).is_some()
    {
        //63AC41..4E selects row0's static small frame when only hovering.
        sequence.frames.get(1)
    } else {
        software_cursor_frame_for(id, sequence)
    };
    let Some(frame) = frame else {
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
    if !state.match_state.input.cursor_visibility.is_visible() {
        return None;
    }
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
    fog.is_cell_revealed(local_owner, pos.rx(), pos.ry())
        && !fog.is_cell_gap_covered(local_owner, pos.rx(), pos.ry())
}

#[cfg(test)]
#[path = "mind_control_pip_tests.rs"]
mod mind_control_pip_tests;

#[cfg(test)]
mod tests {
    /// `tools/spatial_oracle/building_repair.json` `wrench` rows: the frame
    /// DrawExtras picks over every stored game speed.
    #[test]
    fn the_repair_wrench_frame_matches_the_original() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_repair.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["wrench"].as_array().unwrap() {
            let input = &row["input"];
            let options = crate::sim::game_options::GameOptions {
                game_speed: input["speed"].as_i64().unwrap() as i32,
                ..Default::default()
            };
            assert_eq!(
                i64::from(super::repair_wrench_frame(
                    input["frame"].as_i64().unwrap() as i32,
                    &options
                )),
                row["frame_index"].as_i64().unwrap(),
                "{}",
                input["name"]
            );
            compared += 1;
        }
        assert_eq!(compared, 144);
    }

    use super::*;

    #[test]
    fn status_tail_asks_owner_alliance_and_shares_selected_hover_body_admission() {
        use crate::sim::{combat::Health, game_entity::GameEntity, world::Simulation};
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Owner");
        let viewer = sim.interner.intern("Viewer");
        let type_ref = sim.interner.intern("MIND");
        let mut actor = GameEntity::new_at_frame_zero_for_test(
            1,
            12,
            12,
            0,
            0,
            owner,
            Health::new(500),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        actor.lifecycle.set_in_limbo_for_test(false);
        actor.selection.set_selected_for_test(true);
        sim.entities_mut().insert(actor);
        sim.resubmit_entity_display_for_test(1);
        let admitted = |sim: &Simulation, hover| {
            entity_status_pips_drawable(
                sim,
                sim.entities().get(1).unwrap(),
                hover,
                Some(viewer),
                None,
            )
        };
        assert!(
            health_bar_drawable(
                &sim,
                sim.entities().get(1).unwrap(),
                None,
                Some("Viewer"),
                None
            ),
            "enemy health strip has independent admission"
        );
        assert!(!admitted(&sim, None));
        sim.house_alliances
            .entry("VIEWER".into())
            .or_default()
            .insert("OWNER".into());
        assert!(
            !admitted(&sim, None),
            "viewer's one-way alliance does not admit owner status"
        );
        sim.house_alliances.clear();
        sim.house_alliances
            .entry("OWNER".into())
            .or_default()
            .insert("VIEWER".into());
        assert!(
            admitted(&sim, None),
            "owner's one-way alliance admits shared pip tail"
        );
        sim.entities_mut()
            .get_mut(1)
            .unwrap()
            .selection
            .set_selected_for_test(false);
        assert!(!admitted(&sim, None));
        assert!(
            admitted(&sim, Some(1)),
            "ordinary hover also admits count/self-heal/group tail"
        );
        sim.entities_mut()
            .get_mut(1)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        assert!(
            !admitted(&sim, Some(1)),
            "retained Display cannot resurrect limbo feedback"
        );
    }

    /// Original Building DrawExtras selected/hover dispatch, with raw shroud
    /// and +41B contrasts. The same health admission owns mobile pips too.
    #[test]
    fn ordinary_health_hover_matches_native_without_raw_anchor_gate() {
        use crate::sim::{combat::Health, game_entity::GameEntity, world::Simulation};
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/health_hover.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["controls"].as_array().unwrap() {
            let input = &row["input"];
            // The disguised hover branch is a separate existing residual;
            // this comparison is ordinary buildings and the shared admission.
            if input["disguised"].as_bool().unwrap() {
                continue;
            }
            for category in [
                EntityCategory::Structure,
                EntityCategory::Unit,
                EntityCategory::Infantry,
                EntityCategory::Aircraft,
            ] {
                let mut sim = Simulation::new();
                let player = sim.interner.intern("Player");
                let owner = sim.interner.intern("Neutral");
                let type_ref = sim.interner.intern("CATIME01");
                sim.fog = crate::sim::vision::FogState::new(32, 32);
                if input["raw_mapped"].as_bool().unwrap() {
                    sim.fog.reveal_cells_for_owner(player, [(20, 20)]);
                }
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    1,
                    20,
                    20,
                    4,
                    0,
                    owner,
                    Health::new(100),
                    type_ref,
                    category,
                    0,
                    5,
                    false,
                );
                entity.lifecycle.set_in_limbo_for_test(false);
                entity
                    .selection
                    .set_selected_for_test(input["selected"].as_bool().unwrap());
                entity.discovery.set_discovered_by_current_house_for_test(
                    input["discovered"].as_bool().unwrap(),
                );
                sim.entities_mut().insert(entity);
                sim.resubmit_entity_display_for_test(1);
                let hovered = input["hovered"].as_bool().unwrap().then_some(1);
                let expected = row["draw_health_bar"].as_bool().unwrap();
                assert_eq!(
                    health_bar_drawable(
                        &sim,
                        sim.entities().get(1).unwrap(),
                        hovered,
                        Some("Player"),
                        None
                    ),
                    expected,
                    "{} {category:?}",
                    row["name"]
                );
                if expected {
                    // Being hovered cannot resurrect a limbo member, even
                    // while a retained Display registration still exists.
                    sim.entities_mut()
                        .get_mut(1)
                        .unwrap()
                        .lifecycle
                        .set_in_limbo_for_test(true);
                    assert!(!health_bar_drawable(
                        &sim,
                        sim.entities().get(1).unwrap(),
                        hovered,
                        Some("Player"),
                        None
                    ));
                }
                compared += 1;
            }
        }
        assert!(
            compared >= 16,
            "ordinary selected/hover and anchor contrasts"
        );
    }

    #[test]
    #[ignore = "requires retail SOV01 assets"]
    fn sov01_trigger_revealed_time_machine_hover_admits_health_bar() {
        let (retail, _) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        let mut scene = crate::headless_scenario::load(&retail, "SOV01UMD.MAP", 339173046)
            .expect("retail SOV01 scenario");
        let sim = &mut scene.runtime.simulation;
        let rules = &scene.runtime.resources.rules;
        let id = sim
            .entities()
            .values()
            .find(|entity| {
                sim.resolve(entity.type_ref()) == "CATIME01"
                    && (entity.position.rx(), entity.position.ry()) == (142, 96)
            })
            .expect("authored Neutral time machine")
            .stable_id();
        assert_eq!(rules.object("CATIME01").unwrap().strength, 2500);
        let viewer = sim.session.current_house.unwrap();
        let viewer_name = sim.resolve(viewer).to_owned();
        for waypoint in [63, 66, 71, 99, 100, 114, 437, 647] {
            assert!(sim.trigger_reveal_at_waypoint(
                waypoint,
                crate::sim::world::UninitContext::with_rules(rules)
            ));
        }
        let target = sim.entities().get(id).unwrap();
        let (sx, sy) = crate::render::locomotor_visual::ground_screen_position(&target.position);
        let heights = sim.resolved_terrain.as_ref().unwrap().build_height_map();
        let hover = crate::app::input::entity_pick::hover_target_at_point(
            sim,
            sx,
            sy,
            &viewer_name,
            false,
            Some(rules),
            &heights,
            None,
        )
        .expect("revealed time-machine hover");
        assert_eq!(hover.stable_id, id);
        assert!(!target.selection.is_selected());
        println!(
            "CATIME01 raw_anchor_mapped={} hover={hover:?}",
            sim.fog
                .is_cell_revealed(viewer, target.position.rx(), target.position.ry())
        );
        assert!(health_bar_drawable(
            sim,
            target,
            Some(hover.stable_id),
            Some(&viewer_name),
            Some(rules)
        ));
        assert!(!health_bar_drawable(
            sim,
            target,
            None,
            Some(&viewer_name),
            Some(rules)
        ));
    }

    /// A selected object draws both halves — bracket and pips.
    #[test]
    fn selected_unit_draws_bracket_and_pips() {
        assert_eq!(status_visibility(true, false), (true, true));
        assert_eq!(status_visibility(true, true), (true, true));
    }

    /// The hover arm draws the pip strip with no bracket behind it: the
    /// bracket blit sits under an is-selected test inside the health-bar
    /// routine, while the pips do not.
    #[test]
    fn hovered_unselected_unit_draws_pips_without_the_bracket() {
        assert_eq!(status_visibility(false, true), (false, true));
    }

    /// The clause the old damage gate broke: an unselected, unhovered object
    /// draws no health bar at all, whatever its health is. The overlay pass
    /// reaches the health-bar slot only through the selected and hover arms.
    #[test]
    fn unselected_unhovered_unit_draws_nothing_regardless_of_damage() {
        assert_eq!(status_visibility(false, false), (false, false));
    }

    /// The arm table `DrawPipScalePips` walks at `0x0070A50C..0x0070A5E4`:
    /// `WhatAmI()==0xF` (Infantry) enters the organic arm directly, a `Unit`
    /// enters it too while `Type+0xD97` (`Organic`) is set and the units arm
    /// when that byte is clear, and every other class fails both tests.
    #[test]
    fn the_self_heal_pip_arm_table_matches_the_original() {
        use EntityCategory::{Aircraft, Infantry, Structure, Unit};
        assert_eq!(
            self_heal_pip(Infantry, true, 1, 1),
            Some(SelfHealPip::Organic)
        );
        // The organic byte decides the arm for a Unit, not the class alone.
        assert_eq!(self_heal_pip(Unit, true, 1, 1), Some(SelfHealPip::Organic));
        assert_eq!(self_heal_pip(Unit, false, 1, 1), Some(SelfHealPip::Units));
        // `WhatAmI()==2` (Aircraft) and `==6` (Building) never reach a draw.
        assert_eq!(self_heal_pip(Aircraft, false, 1, 1), None);
        assert_eq!(self_heal_pip(Structure, false, 1, 1), None);
        // Each arm needs its own house counter above zero
        // (`HasInfSelfHeal @ 0x0050D9C0`, `HasUnitSelfHeal @ 0x0050D9D0`),
        // and the executed predicate is false for a negative count.
        assert_eq!(self_heal_pip(Infantry, true, 0, 1), None);
        assert_eq!(self_heal_pip(Infantry, true, -1, 1), None);
        assert_eq!(self_heal_pip(Unit, true, 0, 1), None);
        assert_eq!(self_heal_pip(Unit, false, 1, 0), None);
        assert_eq!(self_heal_pip(Unit, false, 1, -1), None);
        // The other arm's counter never substitutes for the object's own.
        assert_eq!(self_heal_pip(Unit, false, 1, 0), None);
        assert_eq!(self_heal_pip(Unit, false, 0, 1), Some(SelfHealPip::Units));
    }

    /// Frame selection and RTTI projection are independent at70A573/5DF/5EC.
    /// The consumed control_group_status corpus executes both category arms.
    #[test]
    fn the_self_heal_pip_frames_and_offsets_match_the_original() {
        assert_eq!(SelfHealPip::Organic.frame(), 0x0D);
        assert_eq!(SelfHealPip::Units.frame(), 0x14);
        assert_eq!(
            self_heal_pip_offset(EntityCategory::Infantry),
            (15.0, -25.0)
        );
        assert_eq!(self_heal_pip_offset(EntityCategory::Unit), (23.0, -22.0));
    }
}
