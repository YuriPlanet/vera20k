//! The hidden runs the app drives itself, with no player and no window
//! anyone sees: a tactical capture, or a map picture for the setup screens'
//! previews. Both render offscreen at a fixed size, are pumped from
//! `about_to_wait`, get no simulation step from the ordinary frame loop, read
//! back the frames they ask for, and end the process with one outcome.

use std::fmt::Display;

use anyhow::Result;
use winit::dpi::PhysicalSize;

use crate::app::AppState;
use crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession;
use crate::app::map_picture::session::MapPictureSession;
use crate::app::presentation::render::GameRenderOutput;
use crate::util::time::{Duration, Instant};

pub(crate) enum HiddenRun {
    Capture(Box<TacticalCaptureSession>),
    Picture(Box<MapPictureSession>),
}

impl HiddenRun {
    /// The fixed size of the hidden window's frame.
    pub(crate) fn size(&self) -> (u32, u32) {
        match self {
            Self::Capture(session) => (session.request().width(), session.request().height()),
            Self::Picture(session) => session.size(),
        }
    }

    /// What the run is called in its failure messages.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Capture(_) => "tactical capture",
            Self::Picture(_) => "map picture",
        }
    }

    pub(crate) fn prepare_state(&mut self, state: &mut AppState) -> Result<()> {
        match self {
            Self::Capture(session) => session.prepare_state(state),
            Self::Picture(session) => session.prepare_state(state),
        }
    }

    pub(crate) fn drive_before_render(&mut self, state: &mut AppState) -> Result<()> {
        match self {
            Self::Capture(session) => session.drive_before_render(state),
            Self::Picture(session) => session.drive_before_render(state),
        }
    }

    /// Whether to read this frame back.
    pub(crate) fn observe_after_render(
        &mut self,
        state: &AppState,
        output: &GameRenderOutput,
    ) -> Result<bool> {
        match self {
            Self::Capture(session) => session.observe_after_render(state, output),
            Self::Picture(session) => session.observe_after_render(state, output),
        }
    }

    pub(crate) fn readback_timeout(&self) -> Result<Duration> {
        match self {
            Self::Capture(session) => session.readback_timeout(),
            Self::Picture(session) => session.readback_timeout(),
        }
    }

    /// Take the frame read back; true once the run is over.
    pub(crate) fn complete_after_readback(
        &mut self,
        state: &AppState,
        format: wgpu::TextureFormat,
        pixels: &[u8],
    ) -> Result<bool> {
        match self {
            Self::Capture(session) => session
                .complete_after_readback(state, format, pixels)
                .map(|()| true),
            Self::Picture(session) => session.complete_after_readback(state, format, pixels),
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        match self {
            Self::Capture(session) => session.is_finished(),
            Self::Picture(session) => session.is_finished(),
        }
    }

    pub(crate) fn next_wake_deadline(&self) -> Instant {
        match self {
            Self::Capture(session) => session.next_wake_deadline(),
            Self::Picture(session) => session.next_wake_deadline(),
        }
    }

    pub(crate) fn fail(&mut self, error: impl Display) {
        match self {
            Self::Capture(session) => session.fail(error),
            Self::Picture(session) => session.fail(error),
        }
    }

    pub(crate) fn take_outcome(&mut self) -> Result<()> {
        match self {
            Self::Capture(session) => session.take_outcome(),
            Self::Picture(session) => session.take_outcome(),
        }
    }

    pub(crate) fn close_requested(&mut self) {
        match self {
            Self::Capture(session) => {
                session.fail("tactical-capture window closed before bundle completion")
            }
            Self::Picture(session) => session.fail("the map-picture window was closed"),
        }
    }

    /// Whether the surface may take this size; a refused one ends the run.
    pub(crate) fn accepts_size(&mut self, size: PhysicalSize<u32>) -> bool {
        let (width, height) = self.size();
        if size == PhysicalSize::new(width, height) {
            return true;
        }
        let kind = match self {
            Self::Capture(_) => "tactical-capture",
            Self::Picture(_) => "map-picture",
        };
        self.fail(format!(
            "{kind} surface resized to {}x{}, expected {width}x{height}",
            size.width, size.height
        ));
        false
    }

    /// The hidden window gained focus. A capture admits no focus and ends;
    /// a map picture ignores it. True when the run ends.
    pub(crate) fn reject_focus(&mut self) -> bool {
        match self {
            Self::Capture(session) => {
                session.record_focus_violation();
                true
            }
            Self::Picture(_) => false,
        }
    }

    /// Input reached the hidden window. It is never dispatched: a capture
    /// counts it as a violation and ends, a map picture drops it. True when
    /// the run ends.
    pub(crate) fn reject_input(&mut self, kind: &str) -> bool {
        match self {
            Self::Capture(session) => {
                session.record_input_violation(kind);
                true
            }
            Self::Picture(_) => false,
        }
    }
}
