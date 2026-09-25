//! Native-ordered and residual multi-way draw passes across atlas textures.
//!
//! The live Ground path consumes one integer-ordered parent stream and changes
//! texture bindings only between contiguous runs. Bridge and not-yet-promoted
//! residual families retain their existing depth merge.
//!
//! ## Dependency rules
//! - Internal to `presentation::render` — only called from draw_passes.rs.

use crate::render::batch::{BatchRenderer, BatchTexture, InstanceBufferPool, SpriteInstance};
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::palette_textures::PaletteSet;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::unit_atlas::UnitAtlas;
use crate::render::unit_slope_transition_cache::VxlSlopeTransitionCache;

use super::draw_plan_lowering::{GroundObjectPass, GroundTexture};
use crate::render::tactical_draw_plan::RenderZPolicy;

/// Which pipeline a `DrawGroup` should dispatch through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawKind {
    /// Standard SHP / overlay: RGBA atlas, batch_shader pipeline.
    Shp,
    /// Voxel unit: R8Uint atlas, sprite_voxel_shader pipeline + PaletteSet bind group.
    Voxel,
}

#[derive(Clone, Copy)]
enum DrawTexture<'tex, 'inst> {
    Single(&'tex BatchTexture),
    UnitPages {
        atlas: &'tex UnitAtlas,
        pages: &'inst [usize],
    },
}

/// Tracks a single draw group during the multi-way merge.
///
/// Each group represents one GPU buffer + texture pair (e.g., VXL units or one SHP page).
/// The `cursor` advances through the buffer as sub-ranges are drawn.
/// `kind` determines which pipeline is used to dispatch the draw.
struct DrawGroup<'tex, 'inst> {
    texture: DrawTexture<'tex, 'inst>,
    buffer: &'tex wgpu::Buffer,
    instances: &'inst [SpriteInstance],
    cursor: u32,
    total: u32,
    kind: DrawKind,
}

impl<'tex, 'inst> DrawGroup<'tex, 'inst> {
    fn new_shp(
        texture: &'tex BatchTexture,
        buffer: &'tex wgpu::Buffer,
        instances: &'inst [SpriteInstance],
        total: u32,
    ) -> Self {
        Self {
            texture: DrawTexture::Single(texture),
            buffer,
            instances,
            cursor: 0,
            total,
            kind: DrawKind::Shp,
        }
    }

    fn new_voxel(
        texture: &'tex BatchTexture,
        buffer: &'tex wgpu::Buffer,
        instances: &'inst [SpriteInstance],
        total: u32,
    ) -> Self {
        Self {
            texture: DrawTexture::Single(texture),
            buffer,
            instances,
            cursor: 0,
            total,
            kind: DrawKind::Voxel,
        }
    }

    fn new_unit_pages(
        atlas: &'tex UnitAtlas,
        pages: &'inst [usize],
        buffer: &'tex wgpu::Buffer,
        instances: &'inst [SpriteInstance],
        total: u32,
    ) -> Self {
        assert_eq!(
            instances.len(),
            pages.len(),
            "stable UnitAtlas instances and page tags must stay aligned"
        );
        Self {
            texture: DrawTexture::UnitPages { atlas, pages },
            buffer,
            instances,
            cursor: 0,
            total,
            kind: DrawKind::Voxel,
        }
    }

    fn depth_at(&self, index: u32) -> f32 {
        self.instances
            .get(index as usize)
            .map(|instance| instance.depth)
            .unwrap_or(f32::NEG_INFINITY)
    }
}

/// Dispatch the already-lowered native Ground sequence without re-sorting it.
///
/// Every run is a contiguous slice of one flat instance buffer. Texture page,
/// atlas family, and `SpriteInstance.depth` select only GPU state; none can
/// change the retained Display parent order carried by `TacticalDrawPlan`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_native_ground_object_pass<'a>(
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    terrain: &mut crate::render::terrain_draw::TerrainDrawRenderer,
    tactical: [u32; 4],
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    ground: &GroundObjectPass,
    overlay_atlas: Option<&'a OverlayAtlas>,
    unit_atlas: Option<&'a UnitAtlas>,
    transition_cache: &'a VxlSlopeTransitionCache,
    sprite_atlas: Option<&'a SpriteAtlas>,
    palette_set: Option<&'a PaletteSet>,
    zshape: &'a wgpu::BindGroup,
) -> crate::render::terrain_draw::TerrainBatchStats {
    let Some((buffer, count)) = pool.get("ground_objects") else {
        return Default::default();
    };
    let mut terrain_stats = crate::render::terrain_draw::TerrainBatchStats::default();
    assert_eq!(count as usize, ground.instances.len());
    let mut cursor = 0;
    while cursor < ground.runs.len() {
        let run = &ground.runs[cursor];
        if matches!(run.target, GroundTexture::TerrainStatic(_)) {
            let start = cursor;
            while cursor < ground.runs.len()
                && matches!(ground.runs[cursor].target, GroundTexture::TerrainStatic(_))
            {
                cursor += 1;
            }
            if let Some(atlas) = overlay_atlas {
                // Every ordinary run is a hard fence. Within this TREE-only
                // span the renderer can share snapshots across disjoint clips,
                // preserving every overlapping predecessor and original index.
                let commands = ground.runs[start..cursor].iter().flat_map(|run| {
                    let GroundTexture::TerrainStatic(piece) = run.target else {
                        unreachable!("TREE span ended at an ordinary fence")
                    };
                    (run.start..run.start + run.count).map(move |index| (index, piece))
                });
                terrain_stats.accumulate(terrain.draw_span(
                    encoder,
                    color,
                    depth,
                    batch,
                    &atlas.texture,
                    buffer,
                    &ground.instances,
                    commands,
                    tactical,
                ));
            }
            continue;
        }
        let mut pass =
            crate::app::presentation::sidebar_render::begin_main_load_pass(encoder, color, depth);
        pass.set_scissor_rect(tactical[0], tactical[1], tactical[2], tactical[3]);
        while cursor < ground.runs.len()
            && !matches!(ground.runs[cursor].target, GroundTexture::TerrainStatic(_))
        {
            let run = &ground.runs[cursor];
            match run.target {
                GroundTexture::TerrainStatic(_) => {
                    unreachable!("native destination edits split normal runs")
                }
                GroundTexture::OverlayAtlas => {
                    if let Some(atlas) = overlay_atlas {
                        batch.draw_passthrough_range(
                            &mut pass,
                            &atlas.texture,
                            buffer,
                            run.start,
                            run.count,
                        );
                    }
                }
                GroundTexture::UnitAtlasPage(page) => {
                    if let (Some(palette), Some(texture)) = (
                        palette_set,
                        unit_atlas.and_then(|atlas| atlas.page_texture(page)),
                    ) {
                        batch.draw_voxel_sprites_range(
                            &mut pass,
                            texture,
                            &palette.bind_group,
                            buffer,
                            run.start,
                            run.count,
                        );
                    }
                }
                GroundTexture::UnitTransitionPage(page) => {
                    if let (Some(texture), Some(palette)) =
                        (transition_cache.page_texture(page), palette_set)
                    {
                        batch.draw_voxel_sprites_range(
                            &mut pass,
                            texture,
                            &palette.bind_group,
                            buffer,
                            run.start,
                            run.count,
                        );
                    }
                }
                GroundTexture::ShpPage(page) => {
                    if let Some(texture) = sprite_atlas.and_then(|atlas| atlas.page(page)) {
                        // The plan's Z policy selects the native leaf family:
                        // buildings write (`0x6E00` -> `0x004990e0`), everything
                        // else tests only (`0x2E00` -> `0x00494b60`).
                        match run.render_z {
                            RenderZPolicy::None => batch.draw_passthrough_range(
                                &mut pass,
                                &texture.texture,
                                buffer,
                                run.start,
                                run.count,
                            ),
                            RenderZPolicy::ReadOnly => batch.draw_zsprite_range(
                                &mut pass,
                                &texture.texture,
                                zshape,
                                buffer,
                                run.start,
                                run.count,
                                false,
                            ),
                            RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite => batch
                                .draw_zsprite_range(
                                    &mut pass,
                                    &texture.texture,
                                    zshape,
                                    buffer,
                                    run.start,
                                    run.count,
                                    true,
                                ),
                        }
                    }
                }
            }
            cursor += 1;
        }
    }
    terrain_stats
}

/// Unified Y-sorted object pass: multi-way merge of VXL units and SHP entities.
///
/// Ground objects (buildings, infantry, vehicles) are rendered in a single
/// Y-sorted pass (Layer 2). Our engine has multiple atlas textures, so we interleave
/// draw calls by walking cursors through each Y-sorted buffer and emitting sub-range draws.
pub(super) fn draw_merged_object_pass<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    pool: &'a InstanceBufferPool,
    unit_instances: &[SpriteInstance],
    unit_pages: &[usize],
    unit_transition_paged: &[Vec<SpriteInstance>],
    shp_paged: &[Vec<SpriteInstance>],
    unit_atlas: Option<&'a UnitAtlas>,
    transition_cache: &'a VxlSlopeTransitionCache,
    sprite_atlas: Option<&'a SpriteAtlas>,
    palette_set: Option<&'a PaletteSet>,
) {
    // Each draw group has a bind group, pool buffer, extracted depth values, and cursor.
    // Depth values are extracted into Vec<f32> to avoid lifetime entanglement between
    // GPU resources (lifetime 'a) and CPU-side instance data (function params).
    // Sort is by depth DESCENDING (largest depth = furthest back = draw first).
    // Depth is based on iso_row (elevation-independent): GetYSort = X + Y
    // (which ignores Z elevation).
    let mut groups: Vec<DrawGroup<'a, '_>> = Vec::new();

    // VXL units draw group -- voxel sprite pipeline (R8Uint atlas + PaletteSet).
    if let (Some(ua), Some((buf, count))) = (unit_atlas, pool.get("unit")) {
        if count > 0 {
            groups.push(DrawGroup::new_unit_pages(
                ua,
                unit_pages,
                buf,
                unit_instances,
                count,
            ));
        }
    }

    const UNIT_TRANSITION_KEYS: [&str; 4] = [
        "unit_transition_p0",
        "unit_transition_p1",
        "unit_transition_p2",
        "unit_transition_p3",
    ];
    for (i, instances) in unit_transition_paged.iter().enumerate() {
        if let (Some(texture), Some(key)) = (
            transition_cache.page_texture(i),
            UNIT_TRANSITION_KEYS.get(i),
        ) {
            if let Some((buf, count)) = pool.get(key) {
                if count > 0 {
                    groups.push(DrawGroup::new_voxel(texture, buf, instances, count));
                }
            }
        }
    }

    // SHP page draw groups — passthrough.
    const SHP_KEYS: [&str; 4] = ["shp_p0", "shp_p1", "shp_p2", "shp_p3"];
    if let Some(sa) = sprite_atlas {
        for (i, page) in sa.pages.iter().enumerate() {
            if let Some(key) = SHP_KEYS.get(i) {
                if let Some((buf, count)) = pool.get(key) {
                    if count > 0 {
                        let instances = shp_paged.get(i).map_or(&[][..], Vec::as_slice);
                        groups.push(DrawGroup::new_shp(&page.texture, buf, instances, count));
                    }
                }
            }
        }
    }

    if groups.is_empty() {
        return;
    }

    // Multi-way merge by depth DESCENDING: largest depth (furthest from camera)
    // draws first. Back-to-front rendering order based on GetYSort = X + Y,
    // which is elevation-independent.
    loop {
        // Find the group with the LARGEST current depth (furthest back).
        // At equal depth, prefer higher-index groups (SHP pages) over group 0 (VXL)
        // so buildings draw before VXL units at the same iso row.
        let mut best: Option<usize> = None;
        let mut best_d: f32 = -1.0;
        for (gi, g) in groups.iter().enumerate() {
            if g.cursor >= g.total {
                continue;
            }
            let d = g.depth_at(g.cursor);
            // Larger depth = further back = should draw first.
            // At equal depth, prefer SHP (gi > 0) over VXL (gi == 0).
            if d > best_d || (d == best_d && gi > 0) {
                best_d = d;
                best = Some(gi);
            }
        }
        let Some(gi) = best else { break };

        // Scan forward: how many consecutive instances from this group can we
        // draw before another group has a larger depth (needs to draw first)?
        let g = &groups[gi];
        let run_start = g.cursor;
        let mut run_end = run_start + 1;
        while run_end < g.total {
            let next_d = g.depth_at(run_end);
            // Check if any other group has a larger depth (further back, should draw first).
            let mut other_has_larger = false;
            for (oi, og) in groups.iter().enumerate() {
                if oi == gi || og.cursor >= og.total {
                    continue;
                }
                let other_d = og.depth_at(og.cursor);
                if other_d > next_d || (other_d == next_d && oi > gi) {
                    other_has_larger = true;
                    break;
                }
            }
            if other_has_larger {
                break;
            }
            run_end += 1;
        }

        // Draw the contiguous run. Voxel groups go through the voxel sprite
        // pipeline (R8Uint atlas + PaletteSet bind group); SHP groups
        // go through passthrough (RGBA atlas, no depth test).
        let count = run_end - run_start;
        let g = &groups[gi];
        draw_group_range(pass, batch, g, palette_set, run_start, count);
        groups[gi].cursor = run_end;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnitPageRun {
    page: usize,
    start: u32,
    count: u32,
}

struct UnitPageRuns<'a> {
    pages: &'a [usize],
    cursor: usize,
    end: usize,
}

fn unit_page_runs(pages: &[usize], start: u32, count: u32) -> UnitPageRuns<'_> {
    let start = start as usize;
    let end = start
        .checked_add(count as usize)
        .expect("UnitAtlas page-run range overflow");
    assert!(
        end <= pages.len(),
        "UnitAtlas page-run range must fit the aligned page tags"
    );
    UnitPageRuns {
        pages,
        cursor: start,
        end,
    }
}

impl Iterator for UnitPageRuns<'_> {
    type Item = UnitPageRun;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.end {
            return None;
        }
        let start = self.cursor;
        let page = self.pages[start];
        self.cursor += 1;
        while self.cursor < self.end && self.pages[self.cursor] == page {
            self.cursor += 1;
        }
        Some(UnitPageRun {
            page,
            start: start as u32,
            count: (self.cursor - start) as u32,
        })
    }
}

/// Draw a flat UnitAtlas stream in its existing order, rebinding textures only
/// at contiguous page changes. Page assignment never becomes a merge tie-break.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_unit_atlas_page_runs<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    atlas: &'a UnitAtlas,
    palette_set: &'a PaletteSet,
    buffer: &'a wgpu::Buffer,
    pages: &[usize],
    start: u32,
    count: u32,
    pose_texture: Option<&'a crate::render::batch::BatchTexture>,
) {
    for run in unit_page_runs(pages, start, count) {
        if run.page == crate::app::presentation::instances::POSE_PAGE {
            // A crashing body drawn this frame (`render::unit_pose_cache`).
            if let Some(texture) = pose_texture {
                batch.draw_voxel_sprites_range(
                    pass,
                    texture,
                    &palette_set.bind_group,
                    buffer,
                    run.start,
                    run.count,
                );
            }
            continue;
        }
        let texture = atlas.page_texture(run.page).unwrap_or_else(|| {
            panic!(
                "UnitAtlas instance references missing page {} of {}",
                run.page,
                atlas.page_count()
            )
        });
        batch.draw_voxel_sprites_range(
            pass,
            texture,
            &palette_set.bind_group,
            buffer,
            run.start,
            run.count,
        );
    }
}

/// Draw one flat SHP stream without regrouping it by atlas page.
///
/// Contiguous page changes only rebind the texture; the instance range remains
/// the native Top-layer append sequence.
pub(super) fn draw_shp_atlas_page_runs<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    atlas: &'a SpriteAtlas,
    buffer: &'a wgpu::Buffer,
    pages: &[usize],
    start: u32,
    count: u32,
) {
    for run in unit_page_runs(pages, start, count) {
        let texture = atlas.page(run.page).unwrap_or_else(|| {
            panic!(
                "SpriteAtlas instance references missing page {} of {}",
                run.page,
                atlas.page_count()
            )
        });
        // Top SHP producers are ordinary bodies and AnimClass draws: their
        // native 0x2000 test remains active independently of display layer.
        batch.draw_zsprite_range(
            pass,
            &texture.texture,
            batch.default_zshape_bind_group(),
            buffer,
            run.start,
            run.count,
            false,
        );
    }
}

fn draw_group_range<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    batch: &'a BatchRenderer,
    group: &DrawGroup<'a, '_>,
    palette_set: Option<&'a PaletteSet>,
    start: u32,
    count: u32,
) {
    match (group.kind, group.texture) {
        (DrawKind::Voxel, DrawTexture::Single(texture)) => {
            if let Some(palette_set) = palette_set {
                batch.draw_voxel_sprites_range(
                    pass,
                    texture,
                    &palette_set.bind_group,
                    group.buffer,
                    start,
                    count,
                );
            }
        }
        (DrawKind::Voxel, DrawTexture::UnitPages { atlas, pages }) => {
            if let Some(palette_set) = palette_set {
                draw_unit_atlas_page_runs(
                    pass,
                    batch,
                    atlas,
                    palette_set,
                    group.buffer,
                    pages,
                    start,
                    count,
                    None,
                );
            }
        }
        (DrawKind::Shp, DrawTexture::Single(texture)) => {
            batch.draw_passthrough_range(pass, texture, group.buffer, start, count);
        }
        (DrawKind::Shp, DrawTexture::UnitPages { .. }) => {
            unreachable!("SHP draw groups cannot use UnitAtlas pages")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_page_runs_preserve_equal_depth_layer_order() {
        // Body, barrel, and turret deliberately share a depth. Their page
        // assignment must not become a new ordering authority.
        let pages = [1usize, 0, 2];
        let runs: Vec<UnitPageRun> = unit_page_runs(&pages, 0, 3).collect();
        assert_eq!(
            runs,
            vec![
                UnitPageRun {
                    page: 1,
                    start: 0,
                    count: 1,
                },
                UnitPageRun {
                    page: 0,
                    start: 1,
                    count: 1,
                },
                UnitPageRun {
                    page: 2,
                    start: 2,
                    count: 1,
                },
            ]
        );
    }

    #[test]
    fn unit_page_runs_cover_subrange_once_without_reordering() {
        let pages = [9usize, 2, 2, 3, 3, 3, 4];
        let runs: Vec<UnitPageRun> = unit_page_runs(&pages, 1, 5).collect();
        assert_eq!(
            runs,
            vec![
                UnitPageRun {
                    page: 2,
                    start: 1,
                    count: 2,
                },
                UnitPageRun {
                    page: 3,
                    start: 3,
                    count: 3,
                },
            ]
        );
        assert_eq!(runs.iter().map(|run| run.count).sum::<u32>(), 5);
    }

    #[test]
    fn building_turret_page_runs_preserve_emission_order() {
        let pages = [2usize, 0, 2, 2, 1];
        let visited = unit_page_runs(&pages, 0, pages.len() as u32)
            .flat_map(|run| run.start..run.start + run.count)
            .collect::<Vec<_>>();

        assert_eq!(visited, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn gsi_13_04_top_shp_page_runs_preserve_registration_append_order() {
        let pages = [2usize, 0, 2, 1];
        let runs: Vec<UnitPageRun> = unit_page_runs(&pages, 0, pages.len() as u32).collect();
        assert_eq!(
            runs,
            vec![
                UnitPageRun {
                    page: 2,
                    start: 0,
                    count: 1,
                },
                UnitPageRun {
                    page: 0,
                    start: 1,
                    count: 1,
                },
                UnitPageRun {
                    page: 2,
                    start: 2,
                    count: 1,
                },
                UnitPageRun {
                    page: 1,
                    start: 3,
                    count: 1,
                },
            ]
        );
    }
}

#[cfg(test)]
#[path = "terrain_ground_gpu_tests.rs"]
mod terrain_ground_gpu_tests;
