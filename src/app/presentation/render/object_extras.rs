//! Replay wrench, rank, health, scale pips and building lines in Extras order.
//! Other status families retain their existing owners and open parity bounds.
use super::draw_plan_lowering::{ObjectExtrasRun, ObjectLayerPass};
use crate::app::presentation::sidebar_render::begin_main_load_pass;
use crate::app::presentation::ui_overlays::StatusTextRun;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::selection_overlay::SelectionOverlay;
use crate::render::terrain_draw::TerrainDrawRenderer;

/// Native6F5190/70A990/6F64A0: rank and PIPBRD E00 consume A (493DF0);
/// PIPS/PIPS2 600 bypasses A (4912B0). Neither shape reads or writes Z. Commands
/// and ranges belong only to this frame's retained Display traversal.
pub(super) fn draw_object_extras(
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    terrain: &mut TerrainDrawRenderer,
    clip: [u32; 4],
    batch: &BatchRenderer,
    overlay: Option<&SelectionOverlay>,
    buffer: Option<&wgpu::Buffer>,
    font: Option<&BatchTexture>,
    layer: &ObjectLayerPass,
) {
    let mut next = 0;
    while next < layer.extras_runs.len() {
        if let ObjectExtrasRun::Lines { batch, .. } = &layer.extras_runs[next] {
            terrain.draw_surface_lines(encoder, color, *batch);
            next += 1;
            continue;
        }
        // Adjacent status parents share one attachment pass; each keeps its
        // wrench->rank->border->fill->scale->self-heal->group order. Lines fence Z.
        let start = next;
        while matches!(
            layer.extras_runs.get(next),
            Some(ObjectExtrasRun::Status { .. })
        ) {
            next += 1;
        }
        draw_status_runs(
            encoder,
            color,
            depth,
            terrain,
            clip,
            batch,
            overlay,
            buffer,
            font,
            &layer.extras_runs[start..next],
        );
    }
}

/// Shared material owner for initial Building Extras and the late Techno
/// sweep. Native70A990/6F64A0/4AED70: rank/border E00 reads A, fills600 bypass A; neither
/// reads or writes Z. The caller retains phase and parent order.
pub(super) fn draw_status_runs(
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    terrain: &mut TerrainDrawRenderer,
    clip: [u32; 4],
    batch: &BatchRenderer,
    overlay: Option<&SelectionOverlay>,
    buffer: Option<&wgpu::Buffer>,
    font: Option<&BatchTexture>,
    runs: &[ObjectExtrasRun],
) {
    let mut pass = begin_main_load_pass(encoder, color, depth);
    pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
    let mut next = 0;
    while next < runs.len() {
        let end = rank_run_end(runs, next);
        let ObjectExtrasRun::Status {
            wrench,
            rank,
            background,
            fills,
            scale,
            self_heal,
            group_rectangles,
            group_text,
            ..
        } = &runs[next]
        else {
            unreachable!("line parents fence status attachment passes");
        };
        if let (Some(overlay), Some(buffer)) = (overlay, buffer) {
            if let (Some(index), Some(shape)) = (wrench, overlay.repair_wrench()) {
                //6F5340 uses Init52C1D8's MOUSEPAL N1 Convert, E00/493DF0.
                // The producer supplies its palette; this existing material
                // consumes A and bypasses Z for both native Extras visits.
                batch.draw_passthrough_range(&mut pass, shape.texture(), buffer, *index, 1);
            }
            if let (Some(index), Some(shape)) = (rank, overlay.pips()) {
                batch.draw_passthrough_range(
                    &mut pass,
                    shape.texture(),
                    buffer,
                    *index,
                    (end - next) as u32,
                );
            }
            if let (Some(index), Some(shape)) = (background, overlay.pipbrd()) {
                batch.draw_passthrough_range(&mut pass, shape.texture(), buffer, *index, 1);
            }
            if let Some(shape) = overlay.pips() {
                batch.draw_with_buffer_passthrough_range(
                    &mut pass,
                    shape.texture(),
                    buffer,
                    fills.start,
                    fills.end - fills.start,
                );
            }
            if let Some(shape) = overlay.scale_pips() {
                //709A90 uses physical PIPS2 through the same plain600,
                //neutral-A/no-Z store owner as health and garrison fills.
                batch.draw_with_buffer_passthrough_range(
                    &mut pass,
                    shape.texture(),
                    buffer,
                    scale.start,
                    scale.end - scale.start,
                );
            }
            if let (Some(index), Some(shape)) = (self_heal, overlay.pips()) {
                batch.draw_with_buffer_passthrough_range(
                    &mut pass,
                    shape.texture(),
                    buffer,
                    *index,
                    1,
                );
            }
            //70A6FD..70A987: each parent's black fill, packed House outline,
            //then GAME.FNT glyph. Surface7BB050/7BADC0/434120 bypass A/Z.
            draw_status_text_material(
                &mut pass,
                batch,
                overlay.white_texture(),
                font,
                buffer,
                group_rectangles.clone(),
                group_text.clone(),
            );
        }
        next = end;
    }
    // Account closed passes even when clipped line batches emitted none.
    drop(pass);
    terrain.note_external_passes(encoder, 1);
}

/// Building43E7B0/70AA60's one ordered text pass at Tactical6D957A. The
/// frame producer retains BuildingClass::Array order; each backing/outline
/// and glyph is submitted before the following Building's label.
pub(super) fn draw_building_status_text(
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    terrain: &mut TerrainDrawRenderer,
    clip: [u32; 4],
    batch: &BatchRenderer,
    overlay: Option<&SelectionOverlay>,
    buffer: Option<&wgpu::Buffer>,
    font: Option<&BatchTexture>,
    runs: &[StatusTextRun],
) {
    let (Some(overlay), Some(buffer)) = (overlay, buffer) else {
        return;
    };
    if runs.is_empty() {
        return;
    }
    let mut pass = begin_main_load_pass(encoder, color, depth);
    pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
    for run in runs {
        draw_status_text_material(
            &mut pass,
            batch,
            overlay.white_texture(),
            font,
            buffer,
            run.rectangles.clone(),
            run.text.clone(),
        );
    }
    drop(pass);
    terrain.note_external_passes(encoder, 1);
}

/// Surface7BB050/7BADC0/434120: opaque black backing, packed House-color
/// outline, then GAME.FNT. Both native text callers bypass A and Z.
fn draw_status_text_material<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    white: &'a BatchTexture,
    font: Option<&'a BatchTexture>,
    buffer: &'a wgpu::Buffer,
    rectangles: std::ops::Range<u32>,
    text: std::ops::Range<u32>,
) {
    batch.draw_with_buffer_passthrough_range(
        pass,
        white,
        buffer,
        rectangles.start,
        rectangles.end - rectangles.start,
    );
    if let Some(font) = font {
        batch.draw_with_buffer_passthrough_range(
            pass,
            font,
            buffer,
            text.start,
            text.end - text.start,
        );
    }
}

/// Only adjacent rank-only parents with contiguous uploaded instances can
/// share a draw. Health, line, material and phase boundaries remain fences;
/// increasing instance order preserves the retained Display order.
fn rank_run_end(runs: &[ObjectExtrasRun], start: usize) -> usize {
    let ObjectExtrasRun::Status {
        wrench: None,
        rank: Some(first),
        background: None,
        fills,
        scale,
        self_heal: None,
        group_rectangles,
        group_text,
        ..
    } = &runs[start]
    else {
        return start + 1;
    };
    if !fills.is_empty()
        || !scale.is_empty()
        || !group_rectangles.is_empty()
        || !group_text.is_empty()
    {
        return start + 1;
    }
    let mut end = start + 1;
    while let Some(ObjectExtrasRun::Status {
        wrench: None,
        rank: Some(index),
        background: None,
        fills,
        scale,
        self_heal: None,
        group_rectangles,
        group_text,
        ..
    }) = runs.get(end)
    {
        if !fills.is_empty()
            || !scale.is_empty()
            || !group_rectangles.is_empty()
            || !group_text.is_empty()
            || *index != *first + (end - start) as u32
        {
            break;
        }
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(index: u32) -> ObjectExtrasRun {
        ObjectExtrasRun::Status {
            parent: u64::from(index),
            wrench: None,
            rank: Some(index),
            background: None,
            fills: index + 1..index + 1,
            scale: index + 1..index + 1,
            group_rectangles: 0..0,
            group_text: 0..0,
            self_heal: None,
        }
    }

    #[test]
    fn adjacent_rank_only_parents_share_one_draw_at_the_unit_bound() {
        let runs: Vec<_> = (0..20_000).map(rank).collect();
        assert_eq!(rank_run_end(&runs, 0), runs.len());
    }

    #[test]
    fn rank_batching_preserves_health_line_and_instance_fences() {
        let mut rectangles = rank(14);
        if let ObjectExtrasRun::Status {
            group_rectangles, ..
        } = &mut rectangles
        {
            *group_rectangles = 15..20;
        }
        let mut glyph = rank(21);
        if let ObjectExtrasRun::Status { group_text, .. } = &mut glyph {
            *group_text = 22..23;
        }
        let mut marker = rank(24);
        if let ObjectExtrasRun::Status { self_heal, .. } = &mut marker {
            *self_heal = Some(25);
        }
        let mut repairing = rank(27);
        if let ObjectExtrasRun::Status { wrench, .. } = &mut repairing {
            *wrench = Some(28);
        }
        let runs = [
            rank(0),
            rank(1),
            ObjectExtrasRun::Status {
                parent: 2,
                wrench: None,
                rank: Some(2),
                background: Some(3),
                fills: 4..5,
                scale: 5..5,
                group_rectangles: 0..0,
                group_text: 0..0,
                self_heal: None,
            },
            rank(5),
            ObjectExtrasRun::Status {
                parent: 6,
                wrench: None,
                rank: None,
                background: None,
                fills: 6..7,
                scale: 7..7,
                group_rectangles: 0..0,
                group_text: 0..0,
                self_heal: None,
            },
            rank(7),
            ObjectExtrasRun::Lines {
                parent: 8,
                lines: 0..0,
                batch: Default::default(),
            },
            rank(8),
            rank(10),
            ObjectExtrasRun::Status {
                parent: 11,
                wrench: None,
                rank: Some(11),
                background: None,
                fills: 12..12,
                scale: 12..13,
                group_rectangles: 0..0,
                group_text: 0..0,
                self_heal: None,
            },
            rank(13),
            rectangles,
            rank(20),
            glyph,
            rank(23),
            marker,
            rank(26),
            repairing,
            rank(28),
        ];
        for (start, end) in [
            (0, 2),
            (2, 3),
            (3, 4),
            (5, 6),
            (7, 8),
            (8, 9),
            (9, 10),
            (10, 11),
            (11, 12),
            (12, 13),
            (13, 14),
            (14, 15),
            (15, 16),
            (16, 17),
            (17, 18),
            (18, 19),
        ] {
            assert_eq!(rank_run_end(&runs, start), end, "run {start}");
        }
    }
}
