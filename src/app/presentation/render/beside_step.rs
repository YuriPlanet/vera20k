//! One game frame painted beside the next game step.
//!
//! The frame owner collects a frame on the main thread, records the window
//! overlays that follow the game picture, and parks the rest here as a
//! [`PendingPaint`]. The match service that runs next takes it at its
//! simulation step and paints it on a second thread while the step runs
//! (`match_runtime::sim_tick`): the paint borrows the renderer and the art,
//! the step borrows the world, and the compiler checks that they share
//! nothing. A service that reaches no step leaves it parked, and the frame
//! owner paints it in line.
//!
//! The picture is the one a single pass draws: the same commands in the same
//! order, in two command buffers submitted together.

use super::paint_state::{PaintArt, PaintState, PaintView};
use super::{GameFrame, PaintedGame, draw_software_cursor, paint_game};
use crate::render::batch::BatchTexture;
use crate::util::time::{Duration, Instant};

/// A collected frame waiting for its paint: everything the paint half owns.
pub(crate) struct PendingPaint {
    view: PaintView,
    job: PaintJob,
}

/// The paint half of one parked frame, without the view its `PaintState`
/// is built from.
pub(crate) struct PaintJob {
    /// The frame's encoder, holding what the collect half recorded.
    encoder: wgpu::CommandEncoder,
    /// The window overlays, the screen shake and the colour filter, recorded
    /// on the main thread; submitted after `encoder`.
    tail: wgpu::CommandBuffer,
    frame: GameFrame,
    /// The software cursor's texture, when the frame draws one.
    cursor_texture: Option<BatchTexture>,
}

/// A painted and handed-off frame.
pub(crate) struct PaintedFrame {
    pub(crate) game: PaintedGame,
    /// What handing the frame to the driver cost the painting thread: the
    /// bounded submissions, the send to the hand-off thread and any wait for
    /// the frame before it.
    pub(crate) handoff: Duration,
}

impl PendingPaint {
    /// Park a collected frame. Its surface frame is parked beside it, in
    /// the GPU context (`GpuContext::park_frame`).
    pub(crate) fn new(
        encoder: wgpu::CommandEncoder,
        tail: wgpu::CommandBuffer,
        frame: GameFrame,
        view: PaintView,
        cursor_texture: Option<BatchTexture>,
    ) -> Self {
        Self {
            view,
            job: PaintJob {
                encoder,
                tail,
                frame,
                cursor_texture,
            },
        }
    }

    /// The view to build the paint's `PaintState` from, and the paint.
    pub(crate) fn into_parts(self) -> (PaintView, PaintJob) {
        (self.view, self.job)
    }
}

impl PaintJob {
    /// Paint the frame and hand it to the driver: the game's passes, its
    /// copy to the window and the software cursor, as the frame owner
    /// records them in line, then the overlays recorded ahead.
    ///
    /// `None` when the surface frame is no longer parked: something else
    /// drew into it, and this picture is dropped.
    pub(crate) fn paint(self, state: &mut PaintState<'_>) -> Option<PaintedFrame> {
        let output = state.renderer.gpu.take_parked_frame()?;
        let Self {
            mut encoder,
            tail,
            frame,
            cursor_texture,
        } = self;
        let game = paint_game(state, &mut encoder, frame);
        state
            .renderer
            .copy_game_frame_to(&mut encoder, &output.texture);
        let target = output.texture.create_view(&Default::default());
        draw_software_cursor(
            state.renderer,
            cursor_texture.as_ref(),
            (state.render_width(), state.render_height()),
            &mut encoder,
            &target,
        );
        drop(target);
        let handoff_started = Instant::now();
        let gpu = &state.renderer.gpu;
        // The submission's index serves readbacks, which a parked frame has none of.
        let _ = gpu.hand_off_frame_with_tail(encoder, Some(tail), output);
        let handoff = state.renderer.terrain_draw_renderer.frame_submit_time()
            + handoff_started.elapsed()
            + gpu.take_handoff_wait();
        Some(PaintedFrame { game, handoff })
    }
}

/// Paint `job` on a second thread while `step` runs on this one.
///
/// The thread lives for this call only: it borrows `renderer` and `art`,
/// and `step` cannot name either, so the two share nothing. Returns `step`'s
/// result and the painted frame (`None` when its surface frame was gone).
/// With no thread to be had, the frame is painted here after the step.
pub(crate) fn paint_beside<R>(
    renderer: &mut crate::app::renderer_state::RendererState,
    art: PaintArt<'_>,
    view: PaintView,
    job: PaintJob,
    step: impl FnOnce() -> R,
) -> (R, Option<PaintedFrame>) {
    let mut parts = Some((renderer, art, view, job));
    let (stepped, painted) = std::thread::scope(|scope| {
        let parts = &mut parts;
        let painter = std::thread::Builder::new()
            .name("frame-paint".into())
            .spawn_scoped(scope, move || {
                let (renderer, art, view, job) = parts.take()?;
                crate::render::frame_handoff::off_main_thread(|| {
                    job.paint(&mut PaintState::from_parts(renderer, art, view))
                })
            });
        let stepped = step();
        let painted = match painter {
            Ok(painter) => match painter.join() {
                Ok(painted) => {
                    static FIRST: std::sync::Once = std::sync::Once::new();
                    FIRST.call_once(|| {
                        log::info!(
                            "Game frames are painted on a second thread beside the game step"
                        )
                    });
                    painted
                }
                Err(panic) => std::panic::resume_unwind(panic),
            },
            Err(error) => {
                log::warn!("No frame paint thread ({error}); painting after the step");
                None
            }
        };
        (stepped, painted)
    });
    match parts {
        Some((renderer, art, view, job)) => {
            let painted = job.paint(&mut PaintState::from_parts(renderer, art, view));
            (stepped, painted)
        }
        None => (stepped, painted),
    }
}
