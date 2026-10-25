//! Replay each retained Display layer without sorting by texture or depth.
//! Draws keep retained parent order wherever their pixels overlap; disjoint
//! draws of one transaction family share a stage ([`super::object_stages`]),
//! so Anim bodies and shadows do not split the ordinary pass per parent.

use crate::render::batch::BatchRenderer;
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::palette_textures::PaletteSet;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::terrain_draw::{
    DestinationEditCommand, PackedComposite, PackedCompositePiece, PackedSpriteKind,
    TerrainBatchStats, TerrainDrawRenderer, packed_wave_key,
};
use crate::render::unit_atlas::UnitAtlas;
use crate::render::unit_pose_cache::VxlPoseFrameCache;

use super::draw_plan_lowering::{ObjectDrawRun, ObjectLayerPass, ObjectTexture};
use super::object_stages::{ObjectStages, StageKind};
use crate::render::tactical_draw_plan::RenderZPolicy;

/// Native background bases6D9920 and subsequent Display sweep6D8DB0 use
/// the same sprite payload and GPU blitters at their respective frame phases.
#[derive(Clone, Copy)]
pub(super) enum ObjectDrawPhase {
    BuildingBase,
    Display,
}

/// One planned draw. An ordinary item is one instance of `runs[run]`;
/// adjacent instances of one run re-coalesce into a single range at replay.
enum StageItem<'a> {
    Ordinary { run: usize, index: u32 },
    Packed(PackedComposite<'a>),
    Edit(DestinationEditCommand),
}

/// Dispatch the already-lowered native Display sequence without re-sorting it.
///
/// Every run is a contiguous slice of one flat instance buffer. Texture page,
/// atlas family, and `SpriteInstance.depth` select only GPU state; none can
/// change the retained Display parent order carried by `TacticalDrawPlan`
/// for any pixel. Surface lines, combat lights and status runs are barriers:
/// everything planned before them replays first.
pub(super) fn draw_native_object_pass<'a>(
    encoder: &mut wgpu::CommandEncoder,
    phase: ObjectDrawPhase,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    terrain: &mut TerrainDrawRenderer,
    tactical: [u32; 4],
    batch: &'a BatchRenderer,
    buffer: Option<(&'a wgpu::Buffer, u32)>,
    ground: &ObjectLayerPass,
    overlay_atlas: Option<&'a OverlayAtlas>,
    unit_atlas: Option<&'a UnitAtlas>,
    pose_cache: Option<&'a VxlPoseFrameCache>,
    sprite_atlas: Option<&'a SpriteAtlas>,
    palette_set: Option<&'a PaletteSet>,
    zshape: &'a wgpu::BindGroup,
    extras: Option<(
        &'a crate::render::selection_overlay::SelectionOverlay,
        &'a wgpu::Buffer,
        Option<&'a crate::render::batch::BatchTexture>,
    )>,
    lights: Option<&crate::render::combat_light::CombatLightRenderer>,
) -> TerrainBatchStats {
    let mut stats = TerrainBatchStats::default();
    assert_eq!(
        buffer.map_or(0, |(_, count)| count as usize),
        ground.instances.len()
    );
    let packed_binding = |target| match target {
        ObjectTexture::UnitAtlasPage(page) => unit_atlas
            .and_then(|atlas| atlas.page_texture(page))
            .zip(palette_set)
            .map(|(texture, palette)| {
                (
                    &texture.bind_group,
                    &palette.bind_group,
                    PackedSpriteKind::Voxel,
                )
            }),
        ObjectTexture::UnitPosePage(page) => pose_cache
            .and_then(|cache| cache.page_texture(page))
            .zip(palette_set)
            .map(|(texture, palette)| {
                (
                    &texture.bind_group,
                    &palette.bind_group,
                    PackedSpriteKind::Voxel,
                )
            }),
        ObjectTexture::ShpPage(page) => sprite_atlas
            .and_then(|atlas| atlas.page(page))
            .map(|page| (&page.texture.bind_group, zshape, PackedSpriteKind::Shp)),
        ObjectTexture::SpriteAtlasEdit(page, crate::render::terrain_draw::TerrainPiece::Shadow) => {
            sprite_atlas.and_then(|atlas| atlas.page(page)).map(|page| {
                (
                    &page.texture.bind_group,
                    zshape,
                    PackedSpriteKind::IndexMask,
                )
            })
        }
        ObjectTexture::TerrainShp(crate::render::terrain_draw::TerrainPiece::Body) => {
            overlay_atlas.map(|atlas| (&atlas.texture.bind_group, zshape, PackedSpriteKind::Shp))
        }
        _ => {
            unreachable!("packed source classification is retained by lowering")
        }
    };
    let runs = match phase {
        ObjectDrawPhase::BuildingBase => &ground.building_base_runs,
        ObjectDrawPhase::Display => &ground.runs,
    };
    let camera = batch.camera_uniform();
    let replay = StageReplay {
        color,
        depth,
        tactical,
        batch,
        buffer: buffer.map(|(buffer, _)| buffer),
        ground,
        runs,
        overlay_atlas,
        unit_atlas,
        pose_cache,
        sprite_atlas,
        palette_set,
        zshape,
    };
    let mut stages =
        ObjectStages::new([camera.screen_size[0] as u32, camera.screen_size[1] as u32]);
    let mut cursor = 0;
    while cursor < runs.len() {
        let run = &runs[cursor];
        match run.target {
            ObjectTexture::SurfaceLines(index) => {
                stats.accumulate(replay.draw(encoder, terrain, stages.take()));
                terrain.draw_surface_lines(encoder, color, ground.line_batches[index]);
                cursor += 1;
                continue;
            }
            ObjectTexture::CombatLight(index) => {
                stats.accumulate(replay.draw(encoder, terrain, stages.take()));
                let lights = lights.expect("a System light has this frame's prepared renderer");
                if lights.draw_record(encoder, tactical, ground.light_record_base + index) {
                    terrain.note_external_passes(encoder, 1);
                }
                cursor += 1;
                continue;
            }
            ObjectTexture::Status(index) => {
                stats.accumulate(replay.draw(encoder, terrain, stages.take()));
                super::object_extras::draw_status_runs(
                    encoder,
                    color,
                    depth,
                    terrain,
                    tactical,
                    batch,
                    extras.map(|(overlay, _, _)| overlay),
                    extras.map(|(_, buffer, _)| buffer),
                    &ground.extras_instances,
                    extras.and_then(|(_, _, font)| font),
                    &ground.extras_runs[index..index + 1],
                );
                cursor += 1;
                continue;
            }
            _ => {}
        }
        if run.packed_parent.is_some() {
            // Unit73B140 shades the opaque source cache and composites once.
            // Native hull/turret/barrel source ownership survives atlas pages;
            // an ancillary SHP draw remains its own Convert490E50 operation.
            // See procedural_drawing_oracle/translucent_blitter_a.md.
            let start = cursor;
            cursor = packed_span_end(runs, cursor);
            let pieces: Vec<_> = runs[start..cursor]
                .iter()
                .flat_map(|run| {
                    let binding = packed_binding(run.target);
                    (run.start..run.start + run.count).filter_map(move |index| {
                        let (source, material, kind) = binding?;
                        Some(PackedCompositePiece {
                            index,
                            instance: &ground.instances[index as usize],
                            source,
                            material,
                            kind,
                        })
                    })
                })
                .collect();
            if is_voxel_target(run.target) {
                plan_packed(
                    &mut stages,
                    terrain,
                    tactical,
                    PackedComposite {
                        pieces,
                        render_z: run.render_z,
                    },
                );
            } else {
                // Every SHP instance remains its own original source blit.
                for piece in pieces {
                    plan_packed(
                        &mut stages,
                        terrain,
                        tactical,
                        PackedComposite {
                            pieces: vec![piece],
                            render_z: run.render_z,
                        },
                    );
                }
            }
            continue;
        }
        if let Some((piece, atlas_slot)) = destination_edit(run.target) {
            // ReadOnly edits reduce in sequence inside one interval; ReadWrite
            // edits keep strict overlap waves (`TerrainDrawRenderer::draw_span`).
            let kind = if run.render_z == RenderZPolicy::ReadOnly {
                StageKind::EditReadOnly
            } else {
                StageKind::EditReadWrite
            };
            for index in run.start..run.start + run.count {
                if let Some(rect) =
                    terrain.piece_footprint(&ground.instances[index as usize], tactical)
                {
                    let command = DestinationEditCommand {
                        index,
                        piece,
                        render_z: run.render_z,
                        atlas_slot,
                    };
                    stages.push(kind, rect, StageItem::Edit(command));
                }
            }
            cursor += 1;
            continue;
        }
        for index in run.start..run.start + run.count {
            let instance = &ground.instances[index as usize];
            // Only an opaque body's unsafe signed candidates need manual
            // replay, one composite per instance: an ordinary run can coalesce
            // many Display parents, unlike native parent-grouped blend sources.
            if is_opaque_voxel_run(run) && voxel_needs_manual_depth(instance, camera) {
                if let Some((source, material, kind)) = packed_binding(run.target) {
                    let piece = PackedCompositePiece {
                        index,
                        instance,
                        source,
                        material,
                        kind,
                    };
                    let composite = PackedComposite {
                        pieces: vec![piece],
                        render_z: run.render_z,
                    };
                    plan_packed(&mut stages, terrain, tactical, composite);
                }
            } else if let Some(rect) = terrain.piece_footprint(instance, tactical) {
                stages.push(
                    StageKind::Ordinary,
                    rect,
                    StageItem::Ordinary { run: cursor, index },
                );
            }
        }
        cursor += 1;
    }
    stats.accumulate(replay.draw(encoder, terrain, stages.take()));
    stats
}

/// A composite with no visible pixels draws nothing and is not planned.
fn plan_packed<'a>(
    stages: &mut ObjectStages<StageItem<'a>>,
    terrain: &TerrainDrawRenderer,
    tactical: [u32; 4],
    composite: PackedComposite<'a>,
) {
    let Some(rect) = terrain.packed_footprint(&composite, tactical) else {
        return;
    };
    let kind = packed_wave_key(&composite).map_or(StageKind::PackedAlone, |(selector, z)| {
        StageKind::Packed(selector, z)
    });
    stages.push(kind, rect, StageItem::Packed(composite));
}

/// The frame state every stage replays against.
struct StageReplay<'a> {
    color: &'a wgpu::TextureView,
    depth: &'a wgpu::TextureView,
    tactical: [u32; 4],
    batch: &'a BatchRenderer,
    buffer: Option<&'a wgpu::Buffer>,
    ground: &'a ObjectLayerPass,
    runs: &'a [ObjectDrawRun],
    overlay_atlas: Option<&'a OverlayAtlas>,
    unit_atlas: Option<&'a UnitAtlas>,
    pose_cache: Option<&'a VxlPoseFrameCache>,
    sprite_atlas: Option<&'a SpriteAtlas>,
    palette_set: Option<&'a PaletteSet>,
    zshape: &'a wgpu::BindGroup,
}

impl StageReplay<'_> {
    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        terrain: &mut TerrainDrawRenderer,
        stages: Vec<(StageKind, Vec<StageItem<'_>>)>,
    ) -> TerrainBatchStats {
        let mut stats = TerrainBatchStats::default();
        for (kind, items) in stages {
            let buffer = self
                .buffer
                .expect("a sprite run has an uploaded instance buffer");
            match kind {
                StageKind::Ordinary => {
                    let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                        encoder, self.color, self.depth,
                    );
                    let [x, y, width, height] = self.tactical;
                    pass.set_scissor_rect(x, y, width, height);
                    let mut items = items
                        .iter()
                        .map(|item| match item {
                            StageItem::Ordinary { run, index } => (*run, *index),
                            _ => unreachable!("an ordinary stage holds ordinary draws"),
                        })
                        .peekable();
                    while let Some((run, start)) = items.next() {
                        let mut count = 1;
                        while items
                            .next_if(|&(next_run, index)| next_run == run && index == start + count)
                            .is_some()
                        {
                            count += 1;
                        }
                        self.draw_ordinary(&mut pass, &self.runs[run], buffer, start, count);
                    }
                    // Ordinary draws are submission boundaries as well as
                    // ordering fences. Many alternating Bullet/Anim parents must
                    // not fill Metal's native command-buffer pool before the
                    // frame reaches submission.
                    drop(pass);
                    terrain.note_external_passes(encoder, 1);
                }
                StageKind::Packed(..) | StageKind::PackedAlone => {
                    let composites: Vec<_> = items
                        .into_iter()
                        .map(|item| match item {
                            StageItem::Packed(composite) => composite,
                            _ => unreachable!("a packed stage holds packed composites"),
                        })
                        .collect();
                    stats.accumulate(terrain.draw_packed_span(
                        encoder,
                        self.color,
                        self.depth,
                        self.batch,
                        buffer,
                        &composites,
                        self.tactical,
                    ));
                }
                StageKind::EditReadOnly | StageKind::EditReadWrite => {
                    let commands = items.into_iter().map(|item| match item {
                        StageItem::Edit(command) => command,
                        _ => unreachable!("an edit stage holds destination edits"),
                    });
                    let (overlay_atlas, sprite_atlas) = (self.overlay_atlas, self.sprite_atlas);
                    stats.accumulate(terrain.draw_span(
                        encoder,
                        self.color,
                        self.depth,
                        self.batch,
                        |slot| {
                            if slot == 0 {
                                overlay_atlas.map(|atlas| &atlas.texture)
                            } else {
                                sprite_atlas
                                    .and_then(|atlas| atlas.page(slot - 1))
                                    .map(|page| &page.texture)
                            }
                        },
                        buffer,
                        &self.ground.instances,
                        commands,
                        self.tactical,
                    ));
                }
            }
        }
        stats
    }

    fn draw_ordinary<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        run: &ObjectDrawRun,
        buffer: &'pass wgpu::Buffer,
        start: u32,
        count: u32,
    ) {
        let batch = self.batch;
        match run.target {
            ObjectTexture::SurfaceLines(_)
            | ObjectTexture::CombatLight(_)
            | ObjectTexture::Status(_) => {
                unreachable!("surface commands replay before sprite dispatch")
            }
            ObjectTexture::TerrainShp(_) | ObjectTexture::SpriteAtlasEdit(_, _) => {
                unreachable!("native destination edits split normal runs")
            }
            ObjectTexture::OverlayAtlas => {
                if let Some(atlas) = self.overlay_atlas {
                    batch.draw_passthrough_range(pass, &atlas.texture, buffer, start, count);
                }
            }
            ObjectTexture::UnitAtlasPage(page) => {
                if let (Some(palette), Some(texture)) = (
                    self.palette_set,
                    self.unit_atlas.and_then(|atlas| atlas.page_texture(page)),
                ) {
                    batch.draw_voxel_sprites_range(
                        pass,
                        texture,
                        &palette.bind_group,
                        buffer,
                        start,
                        count,
                        run.render_z,
                    );
                }
            }
            ObjectTexture::UnitPosePage(page) => {
                if let (Some(texture), Some(palette)) = (
                    self.pose_cache.and_then(|cache| cache.page_texture(page)),
                    self.palette_set,
                ) {
                    batch.draw_voxel_sprites_range(
                        pass,
                        texture,
                        &palette.bind_group,
                        buffer,
                        start,
                        count,
                        run.render_z,
                    );
                }
            }
            ObjectTexture::ShpPage(page) => {
                if let Some(texture) = self.sprite_atlas.and_then(|atlas| atlas.page(page)) {
                    // The plan's Z policy selects the native leaf family:
                    // buildings write (`0x6E00` -> `0x004990e0`), everything
                    // else tests only (`0x2E00` -> `0x00494b60`).
                    match run.render_z {
                        RenderZPolicy::None => batch.draw_passthrough_range(
                            pass,
                            &texture.texture,
                            buffer,
                            start,
                            count,
                        ),
                        RenderZPolicy::ReadOnly => batch.draw_zsprite_range(
                            pass,
                            &texture.texture,
                            self.zshape,
                            buffer,
                            start,
                            count,
                            false,
                        ),
                        RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite => batch
                            .draw_zsprite_range(
                                pass,
                                &texture.texture,
                                self.zshape,
                                buffer,
                                start,
                                count,
                                true,
                            ),
                    }
                }
            }
        }
    }
}

/// Only fully opaque, native-paletted body stores have selector0 copy
/// semantics. Shadows/compatibility alpha and unsupported FX retain their
/// existing owners; sinking changes admission, already shared by both shaders.
fn is_opaque_voxel_run(run: &ObjectDrawRun) -> bool {
    run.packed_parent.is_none()
        && is_voxel_target(run.target)
        && run.render_z == RenderZPolicy::ReadOnly
}

fn voxel_needs_manual_depth(
    instance: &crate::render::batch::SpriteInstance,
    camera: crate::render::batch::CameraUniform,
) -> bool {
    if instance.draw_state.native_selector_bits() != 0
        || instance.alpha != 1.0
        || instance.draw_state.fx_params[0] != 1.0
        || instance.draw_state.fx_flags & !crate::render::draw_state::FX_SINKING_CLIP != 0
        || instance.palette_light.0[0] >> 24 == 0
    {
        return false;
    }
    // GPU location13 aliases the CPU's zshape_origin as voxel z_rect.
    let rect = if instance.zshape_origin[1] > 0.0 {
        instance.zshape_origin
    } else {
        [instance.position[1], instance.size[1]]
    };
    let camera_row = (camera.camera_pos[1] + camera.native_z_origin_y).round() as i32;
    let clip_height = if instance.draw_state.fx_params[3] > 0.0 {
        instance.draw_state.fx_params[3]
    } else {
        camera.screen_size[1] / camera.zoom
    };
    let Some(bottom) = camera_row.checked_add(clip_height.round() as i32) else {
        return true;
    };
    crate::render::native_z::voxel_depth_range(
        [rect[0].round() as i32, (rect[1].round() as i32).max(1)],
        instance.z_gradient,
        instance.z_adjust.round() as i32,
        camera_row,
        [camera_row, bottom],
    )
    .is_none_or(|[min, max]| min < 0 || max > i32::from(u16::MAX))
}

fn is_voxel_target(target: ObjectTexture) -> bool {
    matches!(
        target,
        ObjectTexture::UnitAtlasPage(_) | ObjectTexture::UnitPosePage(_)
    )
}

/// Only consecutive voxel pieces owned by the same native source-cache draw
/// share the final destination operation. Ordinary, SHP and other parents fence it.
fn packed_span_end(runs: &[ObjectDrawRun], start: usize) -> usize {
    let first = &runs[start];
    let mut end = start + 1;
    if is_voxel_target(first.target) {
        while end < runs.len()
            && is_voxel_target(runs[end].target)
            && runs[end].packed_parent == first.packed_parent
            && runs[end].render_z == first.render_z
        {
            end += 1;
        }
    }
    end
}

fn destination_edit(
    target: ObjectTexture,
) -> Option<(crate::render::terrain_draw::TerrainPiece, usize)> {
    match target {
        ObjectTexture::TerrainShp(piece) => Some((piece, 0)),
        ObjectTexture::SpriteAtlasEdit(page, piece) => Some((piece, page + 1)),
        _ => None,
    }
}

/// A shimmered Unit can borrow the same terrain texture as an ordinary tree.
/// Its packed Convert blit is a fence even though both are TerrainShp sources.
fn destination_span_end(runs: &[ObjectDrawRun], start: usize) -> usize {
    let mut end = start;
    while end < runs.len()
        && runs[end].packed_parent.is_none()
        && destination_edit(runs[end].target).is_some()
    {
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::super::draw_plan_lowering::{
        NativeDisplayOrder, ObjectPieceInstance, PlannedObjectInstance,
        lower_ground_object_instances,
    };
    use super::*;
    use crate::render::batch::SpriteInstance;
    use crate::render::draw_state::DrawState;
    use crate::render::tactical_draw_plan::SpriteEncoding;

    #[test]
    fn opaque_voxel_manual_depth_preserves_effect_owners() {
        let camera = crate::render::terrain_draw_gpu_tests::camera([64, 96]);
        let mut safe = SpriteInstance {
            position: [27.0, 43.0],
            size: [18.0, 15.0],
            alpha: 1.0,
            tint: crate::map::lighting::DEFAULT_TINT,
            palette_light: crate::render::palette_light::PaletteLight::new(
                [1000; 3], 53, 1000, true,
            ),
            draw_state: DrawState::default(),
            z_adjust: -65.0,
            ..Default::default()
        };
        let mut unsafe_body = safe;
        // Supplied render boundary near signed zero, using the real shader axis.
        unsafe_body.position[1] = 32768.0;
        assert!(!voxel_needs_manual_depth(&safe, camera));
        assert!(voxel_needs_manual_depth(&unsafe_body, camera));
        for target in [
            ObjectTexture::UnitAtlasPage(0),
            ObjectTexture::UnitAtlasPage(1),
            ObjectTexture::UnitPosePage(0),
            ObjectTexture::UnitPosePage(3),
        ] {
            assert!(is_opaque_voxel_run(&ObjectDrawRun {
                target,
                render_z: RenderZPolicy::ReadOnly,
                packed_parent: None,
                start: 0,
                count: 5
            }));
        }
        unsafe_body.draw_state.fx_flags = crate::render::draw_state::FX_SHADOW;
        assert!(
            !voxel_needs_manual_depth(&unsafe_body, camera),
            "shadow must keep its source owner"
        );
        unsafe_body.draw_state.fx_flags = crate::render::draw_state::FX_SINKING_CLIP;
        assert!(voxel_needs_manual_depth(&unsafe_body, camera));
        unsafe_body.alpha = 0.5;
        assert!(
            !voxel_needs_manual_depth(&unsafe_body, camera),
            "compatibility alpha retains its own shading"
        );
        safe.zshape_origin = [32768.0, 15.0];
        assert!(
            voxel_needs_manual_depth(&safe, camera),
            "the shared composite rect owns row Z"
        );
    }

    #[test]
    fn tree_and_shadow_edits_do_not_absorb_a_following_mirage_blend() {
        use crate::render::terrain_draw::TerrainPiece;
        let order = NativeDisplayOrder::new(&[1, 2, 3]);
        let piece = |kind, bits| ObjectPieceInstance {
            target: ObjectTexture::TerrainShp(kind),
            render_z: RenderZPolicy::ReadOnly,
            instance: SpriteInstance {
                draw_state: DrawState {
                    fx_params: [1.0, bits as f32, 1.0, 0.0],
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                order.object_draw(1, SpriteEncoding::Terrain).unwrap(),
                vec![piece(TerrainPiece::Body, 0), piece(TerrainPiece::Shadow, 0)],
            ),
            PlannedObjectInstance::object(
                order.object_draw(2, SpriteEncoding::Plain).unwrap(),
                vec![piece(TerrainPiece::Body, 4), piece(TerrainPiece::Shadow, 0)],
            ),
            PlannedObjectInstance::object(
                order.object_draw(3, SpriteEncoding::Plain).unwrap(),
                vec![piece(TerrainPiece::Body, 0)],
            ),
        ]);
        assert_eq!(destination_span_end(&pass.runs, 0), 2);
        assert_eq!(pass.runs[2].packed_parent, Some(2));
        assert_eq!(packed_span_end(&pass.runs, 2), 3);
        assert_eq!(destination_span_end(&pass.runs, 3), pass.runs.len());
    }

    #[test]
    fn no_z_anim_shadow_fences_depth_edits_in_retained_parent_order() {
        use crate::render::terrain_draw::TerrainPiece;
        let order = NativeDisplayOrder::new(&[9, 3]);
        let piece = |page, render_z| ObjectPieceInstance {
            target: ObjectTexture::SpriteAtlasEdit(page, TerrainPiece::Shadow),
            render_z,
            instance: SpriteInstance {
                draw_state: DrawState::from_native_selector(0, 1),
                ..Default::default()
            },
        };
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                order.object_draw(3, SpriteEncoding::Plain).unwrap(),
                vec![piece(0, RenderZPolicy::ReadOnly)],
            ),
            PlannedObjectInstance::object(
                order.object_draw(9, SpriteEncoding::Plain).unwrap(),
                vec![
                    piece(1, RenderZPolicy::ReadOnly),
                    piece(0, RenderZPolicy::None),
                ],
            ),
        ]);
        assert_eq!(pass.owners, [9, 9, 3]);
        assert_eq!(destination_span_end(&pass.runs, 0), 1);
        assert_eq!(pass.runs[1].packed_parent, Some(9));
        assert_eq!(packed_span_end(&pass.runs, 1), 2);
        assert_eq!(destination_span_end(&pass.runs, 2), 3);
    }
}

#[cfg(test)]
#[path = "terrain_ground_gpu_tests.rs"]
mod terrain_ground_gpu_tests;

#[cfg(test)]
#[path = "naval_sinking_gpu_tests.rs"]
mod naval_sinking_gpu_tests;

#[cfg(test)]
#[path = "projectile_shape_gpu_tests.rs"]
mod projectile_shape_gpu_tests;

#[cfg(test)]
#[path = "projectile_submission_gpu_tests.rs"]
mod projectile_submission_gpu_tests;

#[cfg(test)]
#[path = "packed_material_gpu_tests.rs"]
mod packed_material_gpu_tests;

#[cfg(test)]
#[path = "infantry_berserk_gpu_tests.rs"]
mod infantry_berserk_gpu_tests;

#[cfg(test)]
#[path = "projectile_voxel_gpu_tests.rs"]
mod projectile_voxel_gpu_tests;

#[cfg(test)]
#[path = "welding_gpu_tests.rs"]
mod welding_gpu_tests;

#[cfg(test)]
#[path = "anim_shadow_gpu_tests.rs"]
mod anim_shadow_gpu_tests;
