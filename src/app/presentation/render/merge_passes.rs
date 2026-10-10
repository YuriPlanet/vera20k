//! Replay each retained Display layer without sorting by texture or depth.
//! Adjacent native destination edits share the existing overlap scheduler;
//! ordinary GPU draw policies fence those spans in the same parent order.

use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::palette_textures::PaletteSet;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::terrain_draw::{PackedComposite, PackedCompositePiece, PackedSpriteKind};
use crate::render::unit_atlas::UnitAtlas;
use crate::render::unit_slope_transition_cache::VxlSlopeTransitionCache;

use super::draw_plan_lowering::{ObjectDrawRun, ObjectLayerPass, ObjectTexture};
use crate::render::tactical_draw_plan::RenderZPolicy;

/// Dispatch the already-lowered native Display sequence without re-sorting it.
///
/// Every run is a contiguous slice of one flat instance buffer. Texture page,
/// atlas family, and `SpriteInstance.depth` select only GPU state; none can
/// change the retained Display parent order carried by `TacticalDrawPlan`.
#[allow(clippy::too_many_arguments)]
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
    pose_texture: Option<&'a BatchTexture>,
    transition_cache: &'a VxlSlopeTransitionCache,
    sprite_atlas: Option<&'a SpriteAtlas>,
    palette_set: Option<&'a PaletteSet>,
    zshape: &'a wgpu::BindGroup,
) -> crate::render::terrain_draw::TerrainBatchStats {
    let Some((buffer, count)) = buffer else {
        return Default::default();
    };
    let mut terrain_stats = crate::render::terrain_draw::TerrainBatchStats::default();
    assert_eq!(count as usize, ground.instances.len());
    let mut cursor = 0;
    while cursor < ground.runs.len() {
        let run = &ground.runs[cursor];
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
                        let binding = match run.target {
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
                            ObjectTexture::UnitPose => {
                                pose_texture.zip(palette_set).map(|(texture, palette)| {
                                    (
                                        &texture.bind_group,
                                        &palette.bind_group,
                                        PackedSpriteKind::Voxel,
                                    )
                                })
                            }
                            ObjectTexture::UnitTransitionPage(page) => transition_cache
                                .page_texture(page)
                                .zip(palette_set)
                                .map(|(texture, palette)| {
                                    (
                                        &texture.bind_group,
                                        &palette.bind_group,
                                        PackedSpriteKind::Voxel,
                                    )
                                }),
                            ObjectTexture::ShpPage(page) => {
                                sprite_atlas.and_then(|atlas| atlas.page(page)).map(|page| {
                                    (&page.texture.bind_group, zshape, PackedSpriteKind::Shp)
                                })
                            }
                            ObjectTexture::TerrainShp(
                                crate::render::terrain_draw::TerrainPiece::Body,
                            ) => overlay_atlas.map(|atlas| {
                                (&atlas.texture.bind_group, zshape, PackedSpriteKind::Shp)
                            }),
                            _ => {
                                unreachable!("packed source classification is retained by lowering")
                            }
                        };
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
        {
            let run = &ground.runs[cursor];
            match run.target {
                ObjectTexture::TerrainShp(_) | ObjectTexture::ProjectileShp(_, _) => {
                    unreachable!("native destination edits split normal runs")
                }
                ObjectTexture::OverlayAtlas => {
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
                            run.start,
                            run.count,
                        );
                    }
                }
                ObjectTexture::UnitPose => {
                    if let (Some(texture), Some(palette)) = (pose_texture, palette_set) {
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
                ObjectTexture::UnitTransitionPage(page) => {
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
        // Ordinary draws are submission boundaries as well as ordering
        // fences. Many alternating Bullet/Anim parents must not fill Metal's
        // native command-buffer pool before the frame reaches submission.
        drop(pass);
        terrain.note_external_passes(encoder, 1);
    }
    terrain_stats
}

fn is_voxel_target(target: ObjectTexture) -> bool {
    matches!(
        target,
        ObjectTexture::UnitAtlasPage(_)
            | ObjectTexture::UnitTransitionPage(_)
            | ObjectTexture::UnitPose
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
        ObjectTexture::ProjectileShp(page, piece) => Some((piece, page + 1)),
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
    fn unit_source_cache_groups_pages_but_not_other_parent_or_ancillary_draws() {
        let order = NativeDisplayOrder::new(&[1, 2, 3]);
        let piece = |target, bits| ObjectPieceInstance {
            target,
            render_z: RenderZPolicy::ReadOnly,
            instance: SpriteInstance {
                draw_state: DrawState {
                    fx_params: [0.5, bits as f32, 1.0, 0.0],
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                order.object_draw(1, SpriteEncoding::Plain).unwrap(),
                vec![
                    piece(ObjectTexture::UnitAtlasPage(0), 4),
                    piece(ObjectTexture::UnitPose, 4),
                    piece(ObjectTexture::UnitTransitionPage(0), 4),
                    piece(ObjectTexture::ShpPage(0), 4),
                    piece(ObjectTexture::UnitPose, 4),
                ],
            ),
            PlannedObjectInstance::object(
                order.object_draw(2, SpriteEncoding::Plain).unwrap(),
                vec![piece(ObjectTexture::UnitPose, 4)],
            ),
            PlannedObjectInstance::object(
                order.object_draw(3, SpriteEncoding::Plain).unwrap(),
                vec![piece(ObjectTexture::UnitPose, 0)],
            ),
        ]);
        assert_eq!(packed_span_end(&pass.runs, 0), 3);
        assert_eq!(packed_span_end(&pass.runs, 3), 4);
        assert_eq!(packed_span_end(&pass.runs, 4), 5);
        assert_eq!(packed_span_end(&pass.runs, 5), 6);
        assert_eq!(pass.runs[6].packed_parent, None);
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
