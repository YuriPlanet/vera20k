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
//! The interface pictures' strips (`render::interface_picture`) follow the
//! same way, after the world's: the status markers, pips and health bars of
//! each layer, the beacons, then the placement grid, in the order
//! `dispatch_draw_passes` draws them.
//! Everything here returns at once unless some art has HD pages.

use super::draw_plan_lowering::{ObjectDrawRun, ObjectExtrasRun, ObjectLayerPass, ObjectTexture};
use crate::app::presentation::state::MatchPresentationState;
use crate::app::renderer_state::RendererState;
use crate::ui::game_screen::GameScreen;
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::picture_zoom::{
    HdDepth, HdMask, HdPlacement, HdRun, HdSource, HdTarget, PictureHdPage, PictureZoomPass,
};
use crate::render::selection_overlay::InterfaceStrip;
use crate::render::tactical_draw_plan::RenderZPolicy;

/// Whether any art this match draws has an HD twin.
fn has_hd_pages(presentation: &MatchPresentationState) -> bool {
    presentation
        .sprite_atlas
        .as_ref()
        .is_some_and(|atlas| atlas.has_picture_hd_pages())
        || presentation
            .selection_overlay
            .as_ref()
            .is_some_and(|overlay| overlay.has_hd_pages())
        || presentation
            .beacon_art
            .as_ref()
            .is_some_and(|art| art.has_hd_pages())
}

/// The HD twin of `source` and the page the world drew it from.
fn hd_page(
    presentation: &MatchPresentationState,
    source: HdSource,
) -> Option<(&PictureHdPage, &BatchTexture)> {
    let strip = match source {
        HdSource::Atlas(page) => {
            let atlas = presentation.sprite_atlas.as_ref()?;
            return Some((atlas.picture_hd_page(page)?, &atlas.page(page)?.texture));
        }
        HdSource::Interface(InterfaceStrip::Beacon(house)) => {
            presentation.beacon_art.as_ref()?.tactical_strip(house)?
        }
        HdSource::Interface(strip) => presentation
            .selection_overlay
            .as_ref()?
            .interface_strip(strip)?,
    };
    Some((strip.hd_page()?, strip.texture()))
}

/// Whether the Options zoom alone magnifies picture art this frame, so the
/// world needs `WorldZoomPass` (at wheel zoom 1) for its window copy, which
/// the HD draw follows.
pub(super) fn needs_world_pass(
    renderer: &RendererState,
    presentation: &MatchPresentationState,
    screen: &GameScreen,
) -> bool {
    has_hd_pages(presentation)
        && renderer
            .tactical_zoom(screen)
            .is_some_and(|zoom| zoom.percent > 100)
}

/// Build this frame's HD instances from `layers`, the beacon rows'
/// `beacons` and the `placement` cells, or clear them when nothing is
/// magnified or nothing draws from an HD page.
pub(super) fn prepare(
    renderer: &mut RendererState,
    presentation: &MatchPresentationState,
    screen: &GameScreen,
    layers: &[ObjectLayerPass; 5],
    beacons: &[Vec<SpriteInstance>],
    placement: &[SpriteInstance],
) {
    if !has_hd_pages(presentation) {
        clear(renderer);
        return;
    }
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
    let mut list = HdList::default();
    world_instances(&mut list, layers, |page| {
        hd_page(presentation, HdSource::Atlas(page)).is_some()
    });
    interface_instances(&mut list, layers, beacons, placement, |strip| {
        hd_page(presentation, HdSource::Interface(strip)).is_some()
    });
    let HdList { instances, runs } = list;
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

/// The instances to draw again, in runs.
#[derive(Default)]
struct HdList {
    instances: Vec<SpriteInstance>,
    runs: Vec<HdRun>,
}

impl HdList {
    /// Add `instance` when it is opaque, extending the last run when it has
    /// the same page, Z test and camera.
    fn push(&mut self, page: HdSource, depth: HdDepth, neutral_a: bool, instance: &SpriteInstance) {
        if !opaque(instance) {
            return;
        }
        let at = self.instances.len() as u32;
        self.instances.push(*instance);
        match self.runs.last_mut() {
            Some(last)
                if last.page == page
                    && last.depth == depth
                    && last.neutral_a == neutral_a
                    && last.start + last.count == at =>
            {
                last.count += 1;
            }
            _ => self.runs.push(HdRun {
                page,
                depth,
                neutral_a,
                start: at,
                count: 1,
            }),
        }
    }
}

/// The world instances to draw again from an HD page, in the world's order:
/// the building bases (`ObjectDrawPhase::BuildingBase`, drawn from layer 2),
/// then layers 0 to 4. Only opaque stores to an SHP page whose page
/// `has_hd`; translucent, FX, packed and linear-shape draws keep their 1x
/// pixels (`merge_passes::restarts_shadow_count` names the same opaque
/// stores).
fn world_instances(
    list: &mut HdList,
    layers: &[ObjectLayerPass; 5],
    has_hd: impl Fn(usize) -> bool,
) {
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
        for instance in drawn {
            list.push(HdSource::Atlas(page), depth, false, instance);
        }
    }
}

/// The interface strips' instances to draw again, as
/// `dispatch_draw_passes` draws them after the world: each layer's status
/// parents (`object_extras::draw_status_run`), the beacons, then the PLACE
/// cells. All bypass Z; the camera each draw binds says whether it reads
/// the A plane. A pulsing self-heal marker is a packed composite and keeps
/// its 1x pixels.
fn interface_instances(
    list: &mut HdList,
    layers: &[ObjectLayerPass; 5],
    beacons: &[Vec<SpriteInstance>],
    placement: &[SpriteInstance],
    has_hd: impl Fn(InterfaceStrip) -> bool,
) {
    let mut push = |strip: InterfaceStrip, neutral_a: bool, instance: Option<&SpriteInstance>| {
        if let Some(instance) = instance
            && has_hd(strip)
        {
            list.push(
                HdSource::Interface(strip),
                HdDepth::None,
                neutral_a,
                instance,
            );
        }
    };
    for layer in layers {
        let at = |index: &Option<u32>| {
            index.and_then(|index| layer.extras_instances.get(index as usize))
        };
        let range = |range: &std::ops::Range<u32>| {
            layer
                .extras_instances
                .get(range.start as usize..range.end as usize)
                .unwrap_or_default()
        };
        for run in &layer.extras_runs {
            let ObjectExtrasRun::Status {
                bomb_clock,
                wrench,
                power_off,
                rank,
                rank_insignia,
                background,
                shield_background,
                fills,
                scale,
                self_heal,
                talk_bubble,
                ..
            } = run
            else {
                continue;
            };
            push(InterfaceStrip::BombClock, false, at(bomb_clock));
            push(InterfaceStrip::Wrench, false, at(wrench));
            push(InterfaceStrip::PowerOff, false, at(power_off));
            if rank_insignia.is_none() {
                push(InterfaceStrip::Pips, false, at(rank));
            }
            push(InterfaceStrip::Pipbrd, false, at(background));
            push(InterfaceStrip::Pipbrd, false, at(shield_background));
            for fill in range(fills) {
                push(InterfaceStrip::Pips, true, Some(fill));
            }
            for pip in range(scale) {
                push(InterfaceStrip::ScalePips, true, Some(pip));
            }
            if !super::object_extras::has_pulse_marker(run, &layer.extras_instances) {
                push(InterfaceStrip::Pips, true, at(self_heal));
            }
            push(InterfaceStrip::TalkBubble, true, at(talk_bubble));
        }
    }
    for (house, row) in beacons.iter().enumerate() {
        for beacon in row {
            push(InterfaceStrip::Beacon(house), true, Some(beacon));
        }
    }
    for cell in placement {
        push(InterfaceStrip::Placement, true, Some(cell));
    }
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
    let Some(world) = renderer.world_zoom_pass.as_ref() else {
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
        &|source| hd_page(presentation, source),
        batch,
        zshape,
        scissor,
    );
}

#[cfg(all(test, tests_app_presentation_render))]
mod tests {
    use super::*;

    /// A status marker drawn with the A plane, health fills drawn at
    /// neutral A and the PLACE cells come back in draw order, each with the
    /// camera its world draw bound; a strip without an HD twin and a
    /// see-through cell are left out.
    #[test]
    fn interface_strips_with_twins_draw_again_with_their_world_camera() {
        let drawn = SpriteInstance {
            alpha: 1.0,
            ..Default::default()
        };
        let mut layers: [ObjectLayerPass; 5] = Default::default();
        layers[0].extras_instances = vec![drawn; 3];
        layers[0].extras_runs.push(ObjectExtrasRun::Status {
            parent: 1,
            bomb_clock: Some(0),
            wrench: Some(1),
            power_off: None,
            rank: None,
            rank_insignia: None,
            background: None,
            shield_background: None,
            fills: 2..3,
            scale: 0..0,
            self_heal: None,
            group_rectangles: 0..0,
            group_text: 0..0,
            digital_text: Vec::new(),
            digital_shapes: Vec::new(),
            talk_bubble: None,
        });
        let faint = SpriteInstance {
            alpha: 0.5,
            ..Default::default()
        };
        let placement = [drawn, faint];
        let mut list = HdList::default();
        interface_instances(&mut list, &layers, &[], &placement, |strip| {
            strip != InterfaceStrip::BombClock
        });
        let runs: Vec<_> = list
            .runs
            .iter()
            .map(|run| (run.page, run.neutral_a, run.count))
            .collect();
        assert_eq!(
            runs,
            [
                (HdSource::Interface(InterfaceStrip::Wrench), false, 1),
                (HdSource::Interface(InterfaceStrip::Pips), true, 1),
                (HdSource::Interface(InterfaceStrip::Placement), true, 1),
            ]
        );
    }
}
