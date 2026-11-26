//! VERA-only, experimental (`docs/picture-model-art.md`, chain 6): the
//! frame's picture art at its own resolution over the magnified world
//! (`render::picture_zoom`).
//!
//! Once per frame, from the lowered object runs, every opaque world instance
//! drawn from a picture page with an HD twin is copied, unchanged: the twin
//! sits at the same UV. The world pass still draws the 1x copy in native
//! order; the HD copy goes over it, where the world still shows that copy,
//! after the magnification, into the frame
//! after `WorldZoomPass::composite`, or into the window after
//! `WorldZoomPass::draw_window_copy` when the Options zoom scales the frame.
//! Everything here returns at once unless the sprite atlas has HD pages.

use super::draw_plan_lowering::{ObjectDrawRun, ObjectLayerPass, ObjectTexture};
use crate::app::presentation::state::MatchPresentationState;
use crate::app::renderer_state::RendererState;
use crate::ui::game_screen::GameScreen;
use crate::render::batch::SpriteInstance;
use crate::render::picture_zoom::{HdDepth, HdMask, HdPlacement, HdRun, HdTarget, PictureZoomPass};
use crate::render::tactical_draw_plan::RenderZPolicy;

/// Whether the Options zoom alone magnifies picture art this frame, so the
/// world needs `WorldZoomPass` (at wheel zoom 1) for its window copy, which
/// the HD draw follows.
pub(super) fn needs_world_pass(
    renderer: &RendererState,
    presentation: &MatchPresentationState,
    screen: &GameScreen,
) -> bool {
    presentation
        .sprite_atlas
        .as_ref()
        .is_some_and(|atlas| atlas.has_picture_hd_pages())
        && renderer
            .tactical_zoom(screen)
            .is_some_and(|zoom| zoom.percent > 100)
}

/// Build this frame's HD instances from `layers`, or clear them when nothing
/// is magnified or nothing draws from an HD page.
pub(super) fn prepare(
    renderer: &mut RendererState,
    presentation: &MatchPresentationState,
    screen: &GameScreen,
    layers: &[ObjectLayerPass; 5],
) {
    let Some(atlas) = presentation
        .sprite_atlas
        .as_ref()
        .filter(|atlas| atlas.has_picture_hd_pages())
    else {
        clear(renderer);
        return;
    };
    // The same choice `render_game` makes for the window copy.
    let window = renderer
        .tactical_zoom(screen)
        .filter(|zoom| zoom.percent > 100);
    let target = renderer.world_zoom_pass.as_ref().and_then(|world| {
        let window = window.filter(|_| world.window_copy_frames().is_some());
        let scale = world.scale() * window.map_or(1.0, |zoom| zoom.scale());
        (scale > 1.0).then_some(if window.is_some() {
            HdTarget::Window
        } else {
            HdTarget::Frame
        })
    });
    let Some(target) = target else {
        clear(renderer);
        return;
    };
    let (instances, runs) = hd_instances(layers, |page| atlas.picture_hd_page(page).is_some());
    if runs.is_empty() {
        clear(renderer);
        return;
    }
    let gpu = &renderer.gpu;
    let pass = renderer.picture_zoom.get_or_insert_with(|| {
        PictureZoomPass::new(
            &gpu.device,
            &gpu.queue,
            gpu.surface_format,
            &renderer.batch_renderer,
        )
    });
    pass.set_frame(&gpu.device, &gpu.queue, &instances, runs, target);
}

fn clear(renderer: &mut RendererState) {
    if let Some(pass) = renderer.picture_zoom.as_mut() {
        pass.clear();
    }
}

/// The world instances to draw again from an HD page, in the world's order:
/// the building bases (`ObjectDrawPhase::BuildingBase`, drawn from layer 2),
/// then layers 0 to 4. Only opaque stores to an SHP page whose page
/// `has_hd`; translucent, FX, packed and linear-shape draws keep their 1x
/// pixels (`merge_passes::restarts_shadow_count` names the same opaque
/// stores).
fn hd_instances(
    layers: &[ObjectLayerPass; 5],
    has_hd: impl Fn(usize) -> bool,
) -> (Vec<SpriteInstance>, Vec<HdRun>) {
    let mut instances = Vec::new();
    let mut runs: Vec<HdRun> = Vec::new();
    let bases = layers[2]
        .building_base_runs
        .iter()
        .map(|run| (&layers[2], run));
    let display = layers
        .iter()
        .flat_map(|layer| layer.runs.iter().map(move |run| (layer, run)));
    for (layer, run) in bases.chain(display) {
        let ObjectDrawRun {
            target: ObjectTexture::ShpPage(page),
            packed_parent: None,
            render_z,
            start,
            count,
        } = *run
        else {
            continue;
        };
        if !has_hd(page) {
            continue;
        }
        let depth = match render_z {
            RenderZPolicy::None => HdDepth::None,
            RenderZPolicy::ReadOnly => HdDepth::Less,
            RenderZPolicy::ReadWrite | RenderZPolicy::AlphaReadWrite => HdDepth::LessEqual,
        };
        let Some(drawn) = layer
            .instances
            .get(start as usize..start as usize + count as usize)
        else {
            continue;
        };
        for instance in drawn.iter().filter(|instance| opaque(instance)) {
            let at = instances.len() as u32;
            instances.push(*instance);
            match runs.last_mut() {
                Some(last)
                    if last.page == page
                        && last.depth == depth
                        && last.start + last.count == at =>
                {
                    last.count += 1;
                }
                _ => runs.push(HdRun {
                    page,
                    depth,
                    start: at,
                    count: 1,
                }),
            }
        }
    }
    (instances, runs)
}

/// An instance whose drawn pixels replace the destination, so its HD copy
/// can replace them again.
fn opaque(instance: &SpriteInstance) -> bool {
    instance.alpha == 1.0
        && instance.draw_state.native_selector_bits() == 0
        && instance.draw_state.fx_flags == 0
        && instance.draw_state.fx_params[0] == 1.0
        && instance.z_gradient & crate::render::native_z::Z_GRADIENT_LINEAR_SHAPE_FLAG == 0
}

/// The HD draw over the frame, right after the world went in
/// (`composite_world_with_drag_rect`), inside the frame's `tactical` rect.
pub(super) fn draw_over_frame(
    renderer: &RendererState,
    presentation: &MatchPresentationState,
    encoder: &mut wgpu::CommandEncoder,
    tactical: [u32; 4],
) {
    let (Some(pass), Some(world)) = (
        renderer.picture_zoom.as_ref(),
        renderer.world_zoom_pass.as_ref(),
    ) else {
        return;
    };
    if pass.target() != Some(HdTarget::Frame) {
        return;
    }
    // World pixel (0, 0) is frame pixel (0, 0) (`WorldZoomPass::new`).
    let placement = HdPlacement {
        origin: [0.0; 2],
        scale: world.scale(),
        mask: None,
    };
    draw(
        renderer,
        presentation,
        pass,
        encoder,
        world.color_view(),
        &placement,
        tactical,
    );
}

/// The HD draw over the window, right after the world's window copy
/// (`draw_world_window_copy`), over the window pixels the copy covers.
pub(super) fn draw_over_window(
    renderer: &RendererState,
    presentation: &MatchPresentationState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    zoom: crate::ui::main_menu_dialogs::options::ZoomGeometry,
    tactical: (u32, u32, u32, u32),
) {
    let (Some(pass), Some(world)) = (
        renderer.picture_zoom.as_ref(),
        renderer.world_zoom_pass.as_ref(),
    ) else {
        return;
    };
    if pass.target() != Some(HdTarget::Window) {
        return;
    }
    let Some((now, before)) = world.window_copy_frames() else {
        return;
    };
    let (now, before) = (
        now.create_view(&Default::default()),
        before.create_view(&Default::default()),
    );
    let frame_origin = [zoom.origin.0 as f32, zoom.origin.1 as f32];
    let placement = HdPlacement {
        origin: frame_origin,
        scale: zoom.scale() * world.scale(),
        mask: Some(HdMask {
            frame_origin,
            frame_scale: zoom.scale(),
            now: &now,
            before: &before,
        }),
    };
    draw(
        renderer,
        presentation,
        pass,
        encoder,
        view,
        &placement,
        zoom.window_rect(tactical),
    );
}

fn draw(
    renderer: &RendererState,
    presentation: &MatchPresentationState,
    pass: &PictureZoomPass,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    placement: &HdPlacement<'_>,
    scissor: [u32; 4],
) {
    let (Some(atlas), Some(world)) = (
        presentation.sprite_atlas.as_ref(),
        renderer.world_zoom_pass.as_ref(),
    ) else {
        return;
    };
    let batch = &renderer.batch_renderer;
    // The shape binding the world's SHP runs draw with (`dispatch_draw_passes`).
    let zshape = presentation
        .building_zshape
        .as_ref()
        .map_or(batch.default_zshape_bind_group(), |zshape| {
            &zshape.bind_group
        });
    let gpu = &renderer.gpu;
    // The completed world the frame or the window copy was made from, read
    // as its encoded bytes.
    let composition = renderer.combat_light_renderer.composition_texture();
    let world_color = composition.create_view(&wgpu::TextureViewDescriptor {
        format: Some(composition.format().remove_srgb_suffix()),
        usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
        ..Default::default()
    });
    pass.draw(
        &gpu.device,
        &gpu.queue,
        encoder,
        view,
        placement,
        world.depth_view(),
        &world_color,
        atlas,
        batch,
        zshape,
        scissor,
    );
}
