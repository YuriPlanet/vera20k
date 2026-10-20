//! Replay each retained Display layer without sorting by texture or depth.
//! Adjacent native destination edits share the existing overlap scheduler;
//! ordinary GPU draw policies fence those spans in the same parent order.

use crate::render::batch::BatchRenderer;
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::palette_textures::PaletteSet;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::terrain_draw::{PackedComposite, PackedCompositePiece, PackedSpriteKind};
use crate::render::unit_atlas::UnitAtlas;
use crate::render::unit_pose_cache::VxlPoseFrameCache;

use super::draw_plan_lowering::{ObjectDrawRun, ObjectLayerPass, ObjectTexture};
use crate::render::tactical_draw_plan::RenderZPolicy;

/// Dispatch the already-lowered native Display sequence without re-sorting it.
///
/// Every run is a contiguous slice of one flat instance buffer. Texture page,
/// atlas family, and `SpriteInstance.depth` select only GPU state; none can
/// change the retained Display parent order carried by `TacticalDrawPlan`.
pub(super) fn draw_native_object_pass<'a>(
    encoder: &mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    terrain: &mut crate::render::terrain_draw::TerrainDrawRenderer,
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
) -> crate::render::terrain_draw::TerrainBatchStats {
    let mut terrain_stats = crate::render::terrain_draw::TerrainBatchStats::default();
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
        ObjectTexture::TerrainShp(crate::render::terrain_draw::TerrainPiece::Body) => {
            overlay_atlas.map(|atlas| (&atlas.texture.bind_group, zshape, PackedSpriteKind::Shp))
        }
        _ => {
            unreachable!("packed source classification is retained by lowering")
        }
    };
    let camera = batch.camera_uniform();
    let mut run_offset = 0;
    let mut cursor = 0;
    while cursor < ground.runs.len() {
        let run = &ground.runs[cursor];
        if let ObjectTexture::SurfaceLines(index) = run.target {
            terrain.draw_surface_lines(encoder, color, ground.line_batches[index]);
            cursor += 1;
            continue;
        }
        if let ObjectTexture::CombatLight(index) = run.target {
            let lights = lights.expect("a System light has this frame's prepared renderer");
            if lights.draw_record(encoder, tactical, ground.light_record_base + index) {
                terrain.note_external_passes(encoder, 1);
            }
            cursor += 1;
            continue;
        }
        if let ObjectTexture::Status(index) = run.target {
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
        let (buffer, _) = buffer.expect("a sprite run has an uploaded instance buffer");
        // Only an opaque body's unsafe signed candidates need manual replay.
        // Retain one composite per instance: an ordinary run can coalesce
        // many Display parents, unlike native parent-grouped blend sources.
        if is_opaque_voxel_run(run)
            && voxel_needs_manual_depth(
                &ground.instances[(run.start + run_offset) as usize],
                camera,
            )
        {
            let index = run.start + run_offset;
            if let Some((source, material, kind)) = packed_binding(run.target) {
                let composite = PackedComposite {
                    pieces: vec![PackedCompositePiece {
                        index,
                        instance: &ground.instances[index as usize],
                        source,
                        material,
                        kind,
                    }],
                    render_z: run.render_z,
                };
                terrain_stats.accumulate(terrain.draw_packed_span(
                    encoder,
                    color,
                    depth,
                    batch,
                    buffer,
                    &[composite],
                    tactical,
                ));
            }
            run_offset += 1;
            if run_offset == run.count {
                cursor += 1;
                run_offset = 0;
            }
            continue;
        }
        if run.packed_parent.is_some() {
            // Unit73B140 shades the opaque source cache and composites once.
            // Native hull/turret/barrel source ownership survives atlas pages;
            // an ancillary SHP draw remains its own Convert490E50 operation.
            // See procedural_drawing_oracle/translucent_blitter_a.md.
            let mut composites = Vec::new();
            while cursor < ground.runs.len() && ground.runs[cursor].packed_parent.is_some() {
                let start = cursor;
                let run = &ground.runs[cursor];
                cursor = packed_span_end(&ground.runs, cursor);
                let pieces: Vec<_> = ground.runs[start..cursor]
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
                    composites.push(PackedComposite {
                        pieces,
                        render_z: run.render_z,
                    });
                } else {
                    // Every SHP instance remains its own original source blit.
                    composites.extend(pieces.into_iter().map(|piece| PackedComposite {
                        pieces: vec![piece],
                        render_z: run.render_z,
                    }));
                }
            }
            terrain_stats.accumulate(terrain.draw_packed_span(
                encoder,
                color,
                depth,
                batch,
                buffer,
                &composites,
                tactical,
            ));
            continue;
        }
        if destination_edit(run.target).is_some() {
            let start = cursor;
            cursor = destination_span_end(&ground.runs, start);
            // Atlas changes preserve parent order. All destination edits in
            // this span share overlap dependencies; ordinary draws are fences.
            let commands = ground.runs[start..cursor].iter().flat_map(|run| {
                let (piece, atlas_slot) =
                    destination_edit(run.target).expect("destination edit span");
                (run.start..run.start + run.count).map(move |index| {
                    crate::render::terrain_draw::DestinationEditCommand {
                        index,
                        piece,
                        render_z: run.render_z,
                        atlas_slot,
                    }
                })
            });
            terrain_stats.accumulate(terrain.draw_span(
                encoder,
                color,
                depth,
                batch,
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
                &ground.instances,
                commands,
                tactical,
            ));
            continue;
        }
        let mut pass =
            crate::app::presentation::sidebar_render::begin_main_load_pass(encoder, color, depth);
        pass.set_scissor_rect(tactical[0], tactical[1], tactical[2], tactical[3]);
        while cursor < ground.runs.len()
            && ground.runs[cursor].packed_parent.is_none()
            && destination_edit(ground.runs[cursor].target).is_none()
            && !matches!(
                ground.runs[cursor].target,
                ObjectTexture::SurfaceLines(_)
                    | ObjectTexture::CombatLight(_)
                    | ObjectTexture::Status(_)
            )
        {
            let run = &ground.runs[cursor];
            let start = run.start + run_offset;
            let remaining = run.count - run_offset;
            let count = if is_opaque_voxel_run(run) {
                voxel_fast_prefix(
                    &ground.instances[start as usize..(start + remaining) as usize],
                    camera,
                )
            } else {
                remaining
            };
            if count == 0 {
                break;
            }
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
                    if let Some(atlas) = overlay_atlas {
                        batch.draw_passthrough_range(
                            &mut pass,
                            &atlas.texture,
                            buffer,
                            start,
                            count,
                        );
                    }
                }
                ObjectTexture::UnitAtlasPage(page) => {
                    if let (Some(palette), Some(texture)) = (
                        palette_set,
                        unit_atlas.and_then(|atlas| atlas.page_texture(page)),
                    ) {
                        batch.draw_voxel_sprites_range(
                            &mut pass,
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
                        pose_cache.and_then(|cache| cache.page_texture(page)),
                        palette_set,
                    ) {
                        batch.draw_voxel_sprites_range(
                            &mut pass,
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
                    if let Some(texture) = sprite_atlas.and_then(|atlas| atlas.page(page)) {
                        // The plan's Z policy selects the native leaf family:
                        // buildings write (`0x6E00` -> `0x004990e0`), everything
                        // else tests only (`0x2E00` -> `0x00494b60`).
                        match run.render_z {
                            RenderZPolicy::None => batch.draw_passthrough_range(
                                &mut pass,
                                &texture.texture,
                                buffer,
                                start,
                                count,
                            ),
                            RenderZPolicy::ReadOnly => batch.draw_zsprite_range(
                                &mut pass,
                                &texture.texture,
                                zshape,
                                buffer,
                                start,
                                count,
                                false,
                            ),
                            RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite => batch
                                .draw_zsprite_range(
                                    &mut pass,
                                    &texture.texture,
                                    zshape,
                                    buffer,
                                    start,
                                    count,
                                    true,
                                ),
                        }
                    }
                }
            }
            run_offset += count;
            if run_offset == run.count {
                cursor += 1;
                run_offset = 0;
            } else {
                break;
            }
        }
        // Ordinary draws are submission boundaries as well as ordering
        // fences. Many alternating Bullet/Anim parents must not fill Metal's
        // native command-buffer pool before the frame reaches submission.
        drop(pass);
        terrain.note_external_passes(encoder, 1);
    }
    terrain_stats
}

/// Only fully opaque, native-paletted body stores have selector0 copy
/// semantics. Shadows/compatibility alpha and unsupported FX retain their
/// existing owners; sinking changes admission, already shared by both shaders.
fn is_opaque_voxel_run(run: &ObjectDrawRun) -> bool {
    run.packed_parent.is_none()
        && is_voxel_target(run.target)
        && run.render_z == RenderZPolicy::ReadOnly
}

fn voxel_fast_prefix(
    instances: &[crate::render::batch::SpriteInstance],
    camera: crate::render::batch::CameraUniform,
) -> u32 {
    instances
        .iter()
        .take_while(|instance| !voxel_needs_manual_depth(instance, camera))
        .count() as u32
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
    fn opaque_voxel_fast_prefix_preserves_coalesced_order_and_effect_owners() {
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
        let slices = [safe, safe, unsafe_body, safe, unsafe_body];
        assert_eq!(voxel_fast_prefix(&slices, camera), 2);
        assert_eq!(voxel_fast_prefix(&slices[2..], camera), 0);
        assert_eq!(voxel_fast_prefix(&slices[3..], camera), 1);
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
