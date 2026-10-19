//! PlanMgr63B0A0/63B150 and PlanNode63C690/63CAE0 surface stores.
//! The simulation owns node identity, membership and dynamic target projection;
//! this frame-local lowering owns packed colors, logical pixels and mouse badges.
//! Color caches are derived afresh; native cache invalidation equivalence remains
//! unproven. No presentation result feeds back into synchronized state.

use crate::app::AppState;
use crate::app::presentation::target_lines::{TacticalViewport, push_surface_rect};
use crate::render::batch::SpriteInstance;
use crate::render::native_surface_format::{
    ACTIVE_RETAIL_RGB565_PRESENTATION, RGB565, interpolate_rgb8,
};
use crate::render::surface_line::{clip_line, patterned_line, solid_line};
use crate::sim::components::DriveCoord;
use crate::sim::world::{PlanningBadgeKind, PlanningDrawSegment};

// Original static bytes836880;63D8B8 negates the presentation wall clock.
const PATTERN: [u8; 16] = [1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0];

pub(super) fn build(
    state: &AppState,
    view: TacticalViewport,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    let mut out = Vec::new();
    let mut highlights = Vec::new();
    // Tactical6D46DD calls both PlanMgr drawing passes only while active.
    if !state.match_state.input.planning.active() {
        return (out, highlights);
    }
    let Some(rt) = state.match_state.sim_runtime.as_ref() else {
        return (out, highlights);
    };
    let sim = &rt.simulation;
    let Some(viewer) = sim.session.current_house() else {
        return (out, highlights);
    };
    let graph = sim.planning_draw_graph(
        viewer,
        &crate::app::input::planning_mode::selected_in_order(state),
        crate::app::input::planning_mode::hover_native_screen(state),
        state.match_state.input.planning.highlighted_node(),
    );
    let phase = ((crate::app::input::tooltips::now_ms(state) as i32).wrapping_neg() >> 5) & 15;
    let color = |owner, bright| {
        let name = sim.interner.resolve(owner);
        let index = state
            .match_state
            .match_presentation
            .house_color_map
            .get(name)
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        let rgb = crate::render::palette_light::house_color_rgb(
            &rt.resources.rules.house_color_ramps,
            index,
        );
        if bright {
            interpolate_rgb8(rgb, [255; 3], 100)
        } else {
            interpolate_rgb8(rgb, [0; 3], 80)
        }
    };
    // A derived frame-local membership index avoids scanning the whole graph
    // once per badge in large selected groups. It dies with this projection.
    let highlighted_members: std::collections::HashSet<_> =
        graph.highlighted_members.iter().copied().collect();
    let badge_white = |actor_id| {
        if graph.highlighted_node.is_some() {
            highlighted_members.contains(&actor_id)
        } else {
            sim.entities()
                .get(actor_id)
                .is_some_and(|actor| actor.selection.is_selected())
        }
    };
    let segment_color = |segment: &PlanningDrawSegment| color(segment.owner, segment.bright);
    let marker_colors: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| {
            //635830 compares packed incoming colors against the dark base.
            let dark = color(node.marker_owner, false);
            if node
                .incoming
                .iter()
                .any(|line| RGB565.pack_rgb8(segment_color(line)) == RGB565.pack_rgb8(dark))
            {
                dark
            } else {
                color(node.marker_owner, node.marker_bright)
            }
        })
        .collect();
    // Tactical6D46EA calls63B0A0 before63B150: every solid outer
    // stroke/marker is drawn before any colored inner stroke. In particular
    // crossing paths must not paint a later outline over an earlier interior.
    //63D0F0 and63B5D6 choose white for selected/highlighted members, black
    // otherwise (native RGB initializers4E8160 and4E8120).
    let outer_color = |white| if white { [255; 3] } else { [0; 3] };
    for node in &graph.nodes {
        let outer = outer_color(node.members.first().is_some_and(|&id| badge_white(id)));
        for line in node.incoming.iter().chain(node.loop_segment.iter()) {
            let from = project(line.source, view);
            let to = project(line.target, view);
            emit_solid_segment(&mut out, from, to, outer, 4, view);
            emit_badge_outer(&mut out, node.badge, [from, to], outer, view);
        }
        emit_marker_outer(&mut out, project(node.coord, view), 8, outer, view);
    }
    for actor in &graph.actors {
        let outer = outer_color(badge_white(actor.actor_id));
        if let Some(coord) = actor.marker_only {
            emit_marker_outer(&mut out, project(coord, view), 6, outer, view);
            continue;
        }
        for (line, badge, width) in [
            (actor.current_leg.as_ref(), actor.current_badge, 3),
            (actor.mega_leg.as_ref(), actor.mega_badge, 3),
            (
                actor.connection.as_ref(),
                actor.badge,
                if actor.current_leg.is_some() { 3 } else { 4 },
            ),
        ] {
            let Some(line) = line else { continue };
            let from = project(line.source, view);
            let to = project(line.target, view);
            emit_marker_outer(&mut out, from, 6, outer, view);
            emit_solid_segment(&mut out, from, to, outer, width, view);
            emit_marker_outer(&mut out, to, 6, outer, view);
            emit_badge_outer(&mut out, badge, [from, to], outer, view);
        }
    }
    //63CAE0: each incoming line and its badge, loop closure and badge, marker.
    for (node, &marker) in graph.nodes.iter().zip(&marker_colors) {
        let white = node
            .members
            .first()
            .is_some_and(|&actor| badge_white(actor));
        //63CE0E gives the closing patrol leg the same badge and patterned
        // interior as63CCF9's incoming legs. Keep their lowering together.
        for line in node.incoming.iter().chain(node.loop_segment.iter()) {
            let from = project(line.source, view);
            let to = project(line.target, view);
            let rgb = segment_color(line);
            emit_segment(&mut out, from, to, rgb, phase, 2, view);
            emit_badge(
                &mut out,
                state,
                node.badge,
                [from, to],
                rgb,
                if white { [255; 3] } else { [0; 3] },
                view,
            );
        }
        emit_marker(&mut out, project(node.coord, view), 6, marker, view);
    }
    //63B150 refills markers after every node's incoming/loop strokes.
    for (node, &marker) in graph.nodes.iter().zip(&marker_colors) {
        emit_marker(&mut out, project(node.coord, view), 6, marker, view);
    }
    //63BE60 then walks allied actors in construction order.
    for actor in &graph.actors {
        //63BFC6..63C005 stores the selected/highlighted black-or-white color
        //at ESP+14. All three badge callers reuse it:63C1A8,63C528 and
        //63C675 (ESP+1C after two pushes), including the entry connection.
        let interior = outer_color(badge_white(actor.actor_id));
        if let Some(coord) = actor.marker_only {
            emit_marker(
                &mut out,
                project(coord, view),
                4,
                color(actor.owner, false),
                view,
            );
            continue;
        }
        if let Some(line) = &actor.current_leg {
            let from = project(line.source, view);
            let to = project(line.target, view);
            let rgb = segment_color(line);
            emit_marker(&mut out, from, 4, rgb, view);
            emit_segment(&mut out, from, to, rgb, phase, 1, view);
            emit_badge(
                &mut out,
                state,
                actor.current_badge,
                [from, to],
                rgb,
                interior,
                view,
            );
            emit_marker(&mut out, to, 4, rgb, view);
        }
        if let Some(line) = &actor.mega_leg {
            let from = project(line.source, view);
            let to = project(line.target, view);
            let rgb = segment_color(line);
            emit_segment(&mut out, from, to, rgb, phase, 1, view);
            emit_badge(
                &mut out,
                state,
                actor.mega_badge,
                [from, to],
                rgb,
                interior,
                view,
            );
            emit_marker(&mut out, to, 4, rgb, view);
        }
        let Some(line) = &actor.connection else {
            continue;
        };
        let from = project(line.source, view);
        let to = project(line.target, view);
        let rgb = segment_color(line);
        emit_marker(&mut out, from, 4, rgb, view);
        emit_segment(
            &mut out,
            from,
            to,
            rgb,
            phase,
            if actor.current_leg.is_some() { 1 } else { 2 },
            view,
        );
        emit_badge(
            &mut out,
            state,
            actor.badge,
            [from, to],
            rgb,
            interior,
            view,
        );
    }
    // Map6D491A calls63B2F0 after the ordinary selected action lines.
    // AC4CCC hover is16x16; retained AC4BF0 uses the node's8x8 rect
    // expanded twice by4, giving24x24. Both draw three white outlines.
    for (coord, size) in [(graph.hovered_coord, 16), (graph.highlighted_coord, 24)] {
        if let Some(coord) = coord {
            let at = project(coord, view);
            for inset in 0..3 {
                emit_outline(
                    &mut highlights,
                    [
                        at[0] - size / 2 + inset,
                        at[1] - size / 2 + inset,
                        size - inset * 2,
                        size - inset * 2,
                    ],
                    [255; 3],
                    view,
                );
            }
        }
    }
    (out, highlights)
}

fn project(coord: DriveCoord, view: TacticalViewport) -> [i32; 2] {
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    [
        x as i32 - view.camera[0] + view.clip[0],
        y as i32 - view.camera[1] + view.clip[1],
    ]
}

fn tint(rgb: [u8; 3]) -> [f32; 3] {
    let rgba = ACTIVE_RETAIL_RGB565_PRESENTATION.quantize_rgba8([rgb[0], rgb[1], rgb[2], 255]);
    [rgba[0], rgba[1], rgba[2]].map(|v| f32::from(v) / 255.)
}

fn emit_rect(out: &mut Vec<SpriteInstance>, rect: [i32; 4], rgb: [u8; 3], view: TacticalViewport) {
    let rect = crate::util::rect::clip_rect(rect, view.clip);
    push_surface_rect(out, rect, tint(rgb), view);
}

fn emit_marker(
    out: &mut Vec<SpriteInstance>,
    at: [i32; 2],
    size: i32,
    rgb: [u8; 3],
    view: TacticalViewport,
) {
    //635490 fills6x6;63D480 fills4x4.
    emit_rect(
        out,
        [at[0] - size / 2, at[1] - size / 2, size, size],
        rgb,
        view,
    );
}

fn emit_outline(
    out: &mut Vec<SpriteInstance>,
    [x, y, w, h]: [i32; 4],
    rgb: [u8; 3],
    view: TacticalViewport,
) {
    emit_rect(out, [x, y, w, 1], rgb, view);
    emit_rect(out, [x, y + h - 1, w, 1], rgb, view);
    emit_rect(out, [x, y, 1, h], rgb, view);
    emit_rect(out, [x + w - 1, y, 1, h], rgb, view);
}

fn emit_marker_outer(
    out: &mut Vec<SpriteInstance>,
    at: [i32; 2],
    size: i32,
    rgb: [u8; 3],
    view: TacticalViewport,
) {
    //635350/63D400 use the surface rectangle-outline slot54.
    emit_outline(
        out,
        [at[0] - size / 2, at[1] - size / 2, size, size],
        rgb,
        view,
    );
}

fn emit_badge_outer(
    out: &mut Vec<SpriteInstance>,
    kind: Option<PlanningBadgeKind>,
    [from, to]: [[i32; 2]; 2],
    rgb: [u8; 3],
    view: TacticalViewport,
) {
    //63D560 and63C8D7:21px outer square around the19px inner badge.
    if kind.is_some()
        && crate::util::native_x87::sqrt_approx_length([to[0] - from[0], to[1] - from[1]]) >= 37
    {
        let mid = [(from[0] + to[0]) / 2, (from[1] + to[1]) / 2];
        emit_outline(out, [mid[0] - 10, mid[1] - 10, 21, 21], rgb, view);
    }
}

fn emit_badge(
    out: &mut Vec<SpriteInstance>,
    state: &AppState,
    kind: Option<PlanningBadgeKind>,
    [from, to]: [[i32; 2]; 2],
    rgb: [u8; 3],
    interior: [u8; 3],
    view: TacticalViewport,
) {
    let Some(kind) = kind else {
        return;
    };
    //63D630: native approximate screen length truncates before the37px gate.
    if crate::util::native_x87::sqrt_approx_length([
        to[0].wrapping_sub(from[0]),
        to[1].wrapping_sub(from[1]),
    ]) < 37
    {
        return;
    }
    let mid = [
        from[0].wrapping_add(to[0]) / 2,
        from[1].wrapping_add(to[1]) / 2,
    ];
    emit_outline(out, [mid[0] - 9, mid[1] - 9, 19, 19], rgb, view);
    emit_rect(out, [mid[0] - 8, mid[1] - 8, 17, 17], interior, view);
    //731CE0/731CD0 cursor rows76/77 are mouse frames433/434, not text.
    let frame = match kind {
        PlanningBadgeKind::AreaGuard => 433,
        PlanningBadgeKind::AttackMove => 434,
    };
    let Some(badge) = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()
        .and_then(|cursor| cursor.planning_badge(frame))
    else {
        return;
    };
    let origin = [mid[0] - badge.width / 2 - 1, mid[1] - badge.height / 2];
    for y in 0..badge.height {
        for x in 0..badge.width {
            let offset = ((y * badge.width + x) * 4) as usize;
            let pixel = &badge.rgba[offset..offset + 4];
            if pixel[3] != 0 {
                emit_rect(
                    out,
                    [origin[0] + x, origin[1] + y, 1, 1],
                    [pixel[0], pixel[1], pixel[2]],
                    view,
                );
            }
        }
    }
}

fn emit_segment(
    out: &mut Vec<SpriteInstance>,
    from: [i32; 2],
    to: [i32; 2],
    rgb: [u8; 3],
    phase: i32,
    strokes: i32,
    view: TacticalViewport,
) {
    //63D810 and63D960 draw only the patterned inner strokes;63B0A0
    //has already drawn their selected-white/unselected-black outer pass.
    //The strict minor-axis comparison gives45-degree ties the X offset.
    let minor = usize::from((to[1] - from[1]).abs() < (to[0] - from[0]).abs());
    let tint = tint(rgb);
    for offset in 0..strokes {
        let mut start = from;
        let mut end = to;
        start[minor] += offset;
        end[minor] += offset;
        if clip_line(&mut start, &mut end, view.clip) {
            patterned_line(start, end, &PATTERN, phase, |point| {
                push_surface_rect(out, [point[0], point[1], 1, 1], tint, view)
            });
        }
    }
}

fn emit_solid_segment(
    out: &mut Vec<SpriteInstance>,
    from: [i32; 2],
    to: [i32; 2],
    rgb: [u8; 3],
    width: i32,
    view: TacticalViewport,
) {
    //63D910 centers width rows with offsets1-ceil(width/2)..floor(width/2).
    //63D810 uses strict abs(dx)>abs(dy), including X-offset diagonal ties.
    let minor = usize::from((to[1] - from[1]).abs() < (to[0] - from[0]).abs());
    let tint = tint(rgb);
    for offset in width / 2 - width + 1..=width / 2 {
        let mut start = from;
        let mut end = to;
        start[minor] += offset;
        end[minor] += offset;
        if clip_line(&mut start, &mut end, view.clip) {
            solid_line(start, end, |rect| push_surface_rect(out, rect, tint, view));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composed_planning_strokes_match_original_surface() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/planning_mode.json",
        ))
        .unwrap();
        let point =
            |v: &serde_json::Value| [v[0].as_i64().unwrap() as i32, v[1].as_i64().unwrap() as i32];
        let view = TacticalViewport {
            camera: [0; 2],
            clip: [0, 0, 160, 120],
            zoom: 1.,
        };
        for row in corpus["render"]["stroke_controls"].as_array().unwrap() {
            let from = point(&row["start"]);
            let to = point(&row["end"]);
            let width = row["width"].as_i64().unwrap() as i32;
            let outer = if row["selected"].as_bool().unwrap() {
                [255; 3]
            } else {
                [0; 3]
            };
            let word = row["inner_rgb565"].as_u64().unwrap() as u16;
            let inner = [
                ((word >> 11) as u8) << 3,
                (((word >> 5) & 63) as u8) << 2,
                ((word & 31) as u8) << 3,
            ];
            let ms = row["ms"].as_i64().unwrap() as i32;
            let mut instances = Vec::new();
            emit_solid_segment(&mut instances, from, to, outer, width, view);
            emit_segment(
                &mut instances,
                from,
                to,
                inner,
                (ms.wrapping_neg() >> 5) & 15,
                width - 2,
                view,
            );
            emit_outline(&mut instances, [to[0] - 3, to[1] - 3, 6, 6], outer, view);
            emit_marker(&mut instances, to, 4, inner, view);
            let mut actual = vec![0x39E7u16; 160 * 120];
            for instance in instances {
                let x = instance.position[0] as i32;
                let y = instance.position[1] as i32;
                let rgb = instance.tint.map(|v| (v * 255.).round() as u8);
                let word = RGB565.pack_rgb8([rgb[0], rgb[1], rgb[2]]);
                for dy in 0..instance.size[1] as i32 {
                    for dx in 0..instance.size[0] as i32 {
                        actual[((y + dy) * 160 + x + dx) as usize] = word;
                    }
                }
            }
            let mut expected = vec![0x39E7u16; actual.len()];
            for pixel in row["pixels"].as_array().unwrap() {
                expected[pixel[1].as_u64().unwrap() as usize * 160
                    + pixel[0].as_u64().unwrap() as usize] = pixel[2].as_u64().unwrap() as u16;
            }
            let differences: Vec<_> = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(12)
                .map(|(i, (a, b))| (i % 160, i / 160, a, b))
                .collect();
            assert!(
                differences.is_empty(),
                "{from:?}->{to:?} width={width} ms={ms}: {differences:?}"
            );
        }
    }
}
