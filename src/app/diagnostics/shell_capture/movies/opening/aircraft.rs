//! One-shot aircraft raster diagnosis using the production body builder, atlas,
//! camera, A-buffer and depth attachment. No simulation state is supplied.

use super::*;
use crate::app::presentation::instances::build_unit_instances;
use crate::app::presentation::render::draw_plan_lowering::{NativeDisplayOrder, ObjectTexture};
use crate::render::batch::{InstanceBufferPool, SpriteInstance};
use crate::render::tactical_draw_plan::RenderZPolicy;

const CLEAR_PIXEL: [u8; 4] = [255, 0, 255, 255];

pub(super) fn observe(state: &AppState, output_prefix: &Path) -> Result<Value> {
    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .context("aircraft runtime absent")?
        .simulation;
    let mut objects = Vec::new();
    build_unit_instances(
        state,
        &mut objects,
        &NativeDisplayOrder::from_display(sim.display_layers()),
    );
    let mut observations = Vec::new();
    let model_coverage = sim.entities().values()
        .filter(|entity| entity.category() == crate::map::entities::EntityCategory::Aircraft)
        .map(|entity| {
            let name = sim.interner.resolve(entity.type_ref());
            let frames = state.match_state.match_presentation.unit_atlas.as_ref()
                .map(|atlas| atlas.frame_counts.iter().filter(|((model,_,_),_)| model == name)
                    .map(|((_,layer,turret),count)| json!({"layer": format!("{layer:?}"), "turret": turret, "frames": count}))
                    .collect::<Vec<_>>()).unwrap_or_default();
            json!({"id": entity.stable_id(), "type": name, "voxel": entity.is_voxel(),
                "limbo": entity.lifecycle.in_limbo(), "passenger": entity.passenger_role.is_inside_transport(),
                "atlas_frames": frames})
        }).collect::<Vec<_>>();
    for object in objects {
        let Some(entity) = sim.entities().get(object.parent.id) else {
            continue;
        };
        if entity.category() != crate::map::entities::EntityCategory::Aircraft {
            continue;
        }
        let mut pool = InstanceBufferPool::new();
        let instances: Vec<SpriteInstance> =
            object.pieces.iter().map(|piece| piece.instance).collect();
        pool.upload_on_device(
            &state.renderer.gpu.device,
            &state.renderer.gpu.queue,
            "aircraft_probe",
            &instances,
        );
        let mut outputs = Vec::new();
        for (label, policy) in [
            ("z-read", RenderZPolicy::ReadOnly),
            ("z-bypass", RenderZPolicy::None),
        ] {
            let pixels = raster(state, &pool, &object.pieces, policy)?;
            let count = pixels
                .chunks_exact(4)
                .filter(|pixel| *pixel != CLEAR_PIXEL)
                .count();
            let black = pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[..3] == [0, 0, 0])
                .count();
            let path = output_prefix
                .with_extension(format!("aircraft-{}-{label}.png", entity.stable_id()));
            ensure!(
                !path.exists(),
                "aircraft diagnostic output already exists: {}",
                path.display()
            );
            let mut rgba = pixels.clone();
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            image::save_buffer(
                &path,
                &rgba,
                state.render_width(),
                state.render_height(),
                image::ColorType::Rgba8,
            )?;
            outputs.push(
                json!({"policy": label, "non_clear_pixels": count, "black_pixels": black,
                "image": path, "bgra_sha256": crate::util::sha256::sha256_hex(&pixels)}),
            );
        }
        let shroud = state.match_state.match_presentation.shroud_buffer.as_ref();
        let input = &state.match_state.input;
        observations.push(json!({
            "id": entity.stable_id(), "type": sim.interner.resolve(entity.type_ref()),
            "display_layer": object.parent.layer.0,
            "pieces": object.pieces.iter().map(|piece| {
                let sprite = &piece.instance;
                let [x,y] = [sprite.position[0] + sprite.size[0] / 2., sprite.position[1] + sprite.size[1] / 2.];
                json!({"texture": format!("{:?}",piece.target), "position": sprite.position,
                    "size": sprite.size, "z_adjust": sprite.z_adjust, "z_gradient": sprite.z_gradient,
                    "z_rect": sprite.zshape_origin, "a_at_rect_center": shroud.and_then(|a|
                        a.sample_world(x,y,input.camera_x,input.camera_y))})
            }).collect::<Vec<_>>(),
            "outputs": outputs,
        }));
    }
    Ok(
        json!({"coverage": "Opaque aircraft pieces replayed against the already-rendered production depth attachment; same camera, palette and A-buffer. The bypass is diagnostic only and is not a parity claim.",
        "model_coverage": model_coverage, "objects": observations}),
    )
}

fn raster(
    state: &AppState,
    pool: &InstanceBufferPool,
    pieces: &[crate::app::presentation::render::draw_plan_lowering::ObjectPieceInstance],
    policy: RenderZPolicy,
) -> Result<Vec<u8>> {
    let renderer = &state.renderer;
    let gpu = &renderer.gpu;
    let [width, height] = [state.render_width(), state.render_height()];
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Aircraft production-depth diagnostic"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: gpu.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let pose = renderer.vxl_pose_frame_cache.borrow();
    let palette = state
        .match_state
        .match_presentation
        .palette_set
        .as_ref()
        .context("aircraft palette absent")?;
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Aircraft unchanged-depth probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.,
                        g: 0.,
                        b: 1.,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &renderer.depth_view,
                stencil_ops: None,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let (x, y, w, h) = crate::app::input::camera::tactical_viewport_px(state);
        pass.set_scissor_rect(x, y, w, h);
        for (index, piece) in pieces.iter().enumerate() {
            let texture = match piece.target {
                ObjectTexture::UnitAtlasPage(page) => state
                    .match_state
                    .match_presentation
                    .unit_atlas
                    .as_ref()
                    .and_then(|atlas| atlas.page_texture(page)),
                ObjectTexture::UnitPosePage(page) => pose.page_texture(page),
                _ => None,
            }
            .context("aircraft diagnostic expected a voxel body")?;
            renderer.batch_renderer.draw_voxel_sprites_range(
                &mut pass,
                texture,
                &palette.bind_group,
                pool.get("aircraft_probe")
                    .context("aircraft instances absent")?
                    .0,
                index as u32,
                1,
                policy,
            );
        }
    }
    let read = crate::render::frame_readback::PendingBgra8Readback::encode(
        &gpu.device,
        &mut encoder,
        &texture,
        gpu.config.format,
        width,
        height,
    )?;
    let submission = gpu.queue.submit([encoder.finish()]);
    Ok(read.finish(&gpu.device, submission, Duration::from_secs(20))?)
}
