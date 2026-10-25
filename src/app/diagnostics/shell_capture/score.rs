//! Score dialog `0x108` capture routes. Once the main menu has settled, the
//! route opens the page through the production entry the scenario exit
//! cascade uses (`App::open_score_page`) with the retail comparison game's
//! values: Game 1, 00:01:19, the Allied side, "Computer" (DarkRed, 0/0/3/0)
//! above "[New Player]" (DarkBlue, 0/15/0/0). Input goes through the page's
//! production handlers.

use super::*;
use crate::app::App;
use crate::ui::campaign_score_shell::{
    CONTINUE_BUTTON, CampaignScoreLayout, CampaignScoreScreenModel, SCORE_ART_REGION_STATIC,
};
use crate::ui::score_shell::{ScoreRow, ScoreScreenModel};
use crate::ui::shell::geom::RectPx;
use image::ImageEncoder;
use serde_json::{Value, json};
use std::time::Instant;

/// Frames to present once the target state is reached.
const SETTLE_FRAMES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScoreTarget {
    /// Settled with every reveal finished, the pointer at the neutral point.
    Steady,
    /// The entry slide held at one tick.
    Entry(u32),
    /// Settled with the pointer resting on Continue: its help shows.
    HoverContinue,
    /// The pointer rests on Continue, then moves onto the bare art: the
    /// dialog's hit test clears the help.
    LeaveContinue,
    /// Continue pressed on the settled page: its teardown slide held at one
    /// tick.
    SlideOut(u32),
}

/// Continue's centre at 800x600.
const CONTINUE_POINT: (i32, i32) = (720, 556);
/// Bare art, on no child, at 800x600.
const ART_POINT: (i32, i32) = (40, 300);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    MainMenu,
    /// `0x108` is open: its entry slide runs and its texts reveal.
    Score,
    /// The pointer rests on Continue: its help reveals.
    Hover,
    /// The pointer left Continue for the art: the status line empties.
    Left,
    /// Continue released: the teardown slide runs toward the held tick.
    Leaving,
    Settling(u32),
}

pub(super) struct ScoreCapture {
    target: ScoreTarget,
    phase: Phase,
    route: Vec<Value>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum CampaignScorePhase {
    #[default]
    MainMenu,
    CampaignScore,
    Settling(u32),
}

const CAMPAIGN_ANIMATION_SAMPLE_COUNT: usize = 3;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum CampaignAnimationPhase {
    #[default]
    MainMenu,
    WaitingForFirstFrame,
    WaitingForFirstSample,
    WaitingForNextFrame,
    Continue,
    WaitingForMainMenu,
    WaitingForReopenedFrame,
    Done,
}

#[derive(Debug, Clone, Copy)]
struct PendingCampaignAnimationSample {
    label: &'static str,
    frame: u32,
    elapsed_ms: u128,
    emblem_frame: usize,
    roi: RectPx,
}

struct CampaignAnimationSample {
    label: &'static str,
    frame: u32,
    elapsed_ms: u128,
    emblem_frame: usize,
    roi: RectPx,
    frame_sha256: String,
    roi_sha256: String,
    roi_changed_pixels_from_previous: Option<usize>,
    pixels: Vec<u8>,
}

#[derive(Default)]
struct CampaignAnimationSequence {
    phase: CampaignAnimationPhase,
    page_opened_at: Option<Instant>,
    reopened_at: Option<Instant>,
    pending: Option<PendingCampaignAnimationSample>,
    samples: Vec<CampaignAnimationSample>,
}

/// A synthetic, asset-independent fixture for visually reviewing the real
/// campaign victory page through the production app renderer. These values
/// are display examples only and establish no native parity.
fn synthetic_campaign_score_model() -> CampaignScoreScreenModel {
    CampaignScoreScreenModel {
        side: 0,
        par_time: "00:10:00".into(),
        your_time: "00:08:31".into(),
        enemies_destroyed: "17".into(),
        rank: "Captain".into(),
        commendation: "Mission completed ahead of schedule.".into(),
    }
}

pub(super) struct CampaignScoreCapture {
    phase: CampaignScorePhase,
    route: Vec<Value>,
    animation: Option<CampaignAnimationSequence>,
}

impl CampaignScoreCapture {
    pub(super) fn new() -> Self {
        Self {
            phase: CampaignScorePhase::MainMenu,
            route: Vec::new(),
            animation: None,
        }
    }

    pub(super) fn animation_sequence() -> Self {
        Self {
            phase: CampaignScorePhase::MainMenu,
            route: Vec::new(),
            animation: Some(CampaignAnimationSequence::default()),
        }
    }

    pub(super) fn is_animation_sequence(&self) -> bool {
        self.animation.is_some()
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if self.animation.is_some() {
            return self.after_present_animation(state, rendered, frame);
        }

        if self.phase == CampaignScorePhase::MainMenu {
            if rendered == PresentedShell::MainMenu
                && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))?
            {
                App::open_campaign_score_page(
                    state,
                    synthetic_campaign_score_model(),
                    "Synthetic campaign score capture".into(),
                    "Synthetic UI fixture; not native-parity evidence.".into(),
                );
                self.route.push(json!({
                    "dialog": 0x103,
                    "frame": frame,
                    "action": "open through App::open_campaign_score_page",
                    "fixture": "synthetic UI example",
                }));
                self.phase = CampaignScorePhase::CampaignScore;
            }
            return Ok(());
        }

        ensure!(
            rendered == PresentedShell::Score,
            "campaign capture did not render the production score page"
        );
        ensure!(
            App::campaign_score_shell_active(state),
            "campaign score capture left the 0x103 page"
        );
        if self.phase == CampaignScorePhase::CampaignScore
            && state
                .frontend
                .campaign_score_page
                .as_ref()
                .is_some_and(|page| page.reveals_terminal())
        {
            self.phase = CampaignScorePhase::Settling(SETTLE_FRAMES);
        } else if let CampaignScorePhase::Settling(remaining) = self.phase {
            self.phase = CampaignScorePhase::Settling(remaining.saturating_sub(1));
        }
        Ok(())
    }

    fn after_present_animation(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        let animation = self
            .animation
            .as_mut()
            .expect("animation sequence dispatch checks its state");
        match animation.phase {
            CampaignAnimationPhase::MainMenu => {
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))?
                {
                    App::open_campaign_score_page(
                        state,
                        synthetic_campaign_score_model(),
                        "Synthetic campaign score animation capture".into(),
                        "Synthetic UI fixture; not native-parity evidence.".into(),
                    );
                    ensure!(
                        state
                            .frontend
                            .campaign_score_art
                            .as_ref()
                            .and_then(|art| art.scra.as_ref())
                            .is_some_and(|strip| !strip.frames.is_empty()),
                        "campaign animation capture requires loaded SCRA frames"
                    );
                    let opened_at = Instant::now();
                    animation.page_opened_at = Some(opened_at);
                    animation.phase = CampaignAnimationPhase::WaitingForFirstFrame;
                    self.route.push(json!({
                        "dialog": 0x103,
                        "frame": frame,
                        "action": "open through App::open_campaign_score_page",
                        "fixture": "synthetic UI example",
                        "animation_art_loaded": true,
                    }));
                }
            }
            CampaignAnimationPhase::WaitingForFirstFrame
            | CampaignAnimationPhase::WaitingForFirstSample
            | CampaignAnimationPhase::WaitingForNextFrame => {
                ensure!(
                    rendered == PresentedShell::Score && App::campaign_score_shell_active(state),
                    "campaign animation sequence left the production 0x103 page"
                );
                if animation.phase == CampaignAnimationPhase::WaitingForFirstFrame
                    && state
                        .frontend
                        .campaign_score_page
                        .as_ref()
                        .is_some_and(|page| page.reveals_terminal())
                {
                    self.route.push(json!({
                        "dialog": 0x103,
                        "frame": frame,
                        "action": "wait for page text to finish revealing before first sample",
                    }));
                    animation.phase = CampaignAnimationPhase::WaitingForFirstSample;
                }
            }
            CampaignAnimationPhase::Continue => {
                ensure!(
                    rendered == PresentedShell::Score && App::campaign_score_shell_active(state),
                    "campaign Continue probe did not start on dialog 0x103"
                );
                click_campaign_continue(state)?;
                ensure!(
                    crate::app::frontend::shell_transition::shell_exit_wave(
                        state,
                        crate::app::frontend::shell_transition::ShellSlideKind::CampaignScore,
                    )
                    .is_some(),
                    "Continue did not start the 0x103 teardown slide"
                );
                animation.phase = CampaignAnimationPhase::WaitingForMainMenu;
                self.route.push(json!({
                    "dialog": 0x103,
                    "frame": frame,
                    "action": "Continue started the 0x103 teardown slide",
                    "teardown_slide": true,
                }));
            }
            CampaignAnimationPhase::WaitingForMainMenu => {
                if rendered == PresentedShell::MainMenu {
                    ensure!(
                        state.frontend.campaign_score_page.is_none()
                            && state.frontend.campaign_score_art.is_none(),
                        "campaign score page or art survived the Continue close"
                    );
                    App::open_campaign_score_page(
                        state,
                        synthetic_campaign_score_model(),
                        "Synthetic campaign score animation reopen".into(),
                        "Synthetic UI fixture; not native-parity evidence.".into(),
                    );
                    ensure!(
                        state
                            .frontend
                            .campaign_score_art
                            .as_ref()
                            .and_then(|art| art.scra.as_ref())
                            .is_some_and(|strip| !strip.frames.is_empty()),
                        "campaign animation reopen did not load SCRA frames"
                    );
                    animation.reopened_at = Some(Instant::now());
                    animation.phase = CampaignAnimationPhase::WaitingForReopenedFrame;
                    self.route.push(json!({
                        "dialog": 0x103,
                        "frame": frame,
                        "action": "reopen through App::open_campaign_score_page",
                        "animation_art_loaded": true,
                    }));
                }
            }
            CampaignAnimationPhase::WaitingForReopenedFrame => {
                ensure!(
                    rendered == PresentedShell::Score && App::campaign_score_shell_active(state),
                    "campaign animation reopen did not render dialog 0x103"
                );
            }
            CampaignAnimationPhase::Done => {}
        }
        Ok(())
    }

    pub(super) fn should_capture_animation_frame(
        &mut self,
        state: &AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<bool> {
        let Some(animation) = self.animation.as_mut() else {
            return Ok(false);
        };
        ensure!(
            animation.pending.is_none(),
            "campaign animation sequence already has an outstanding sample"
        );
        let (label, epoch) = match animation.phase {
            CampaignAnimationPhase::WaitingForFirstSample => {
                if !state
                    .frontend
                    .campaign_score_page
                    .as_ref()
                    .is_some_and(|page| page.reveals_terminal())
                {
                    return Ok(false);
                }
                ("animation-before", animation.page_opened_at)
            }
            CampaignAnimationPhase::WaitingForNextFrame => {
                ("animation-after", animation.page_opened_at)
            }
            CampaignAnimationPhase::WaitingForReopenedFrame => {
                ("reopened-frame-zero", animation.reopened_at)
            }
            _ => return Ok(false),
        };
        ensure!(
            rendered == PresentedShell::Score && App::campaign_score_shell_active(state),
            "campaign animation sample was requested outside rendered dialog 0x103"
        );
        let Some(emblem_frame) = state
            .frontend
            .campaign_score_page
            .as_ref()
            .and_then(|page| page.emblem_animation_frame())
        else {
            // 0x103 starts the kind-4 timer on its SHOW completion (0x4EC),
            // after the entry slide; a reopened frame is sampled afterward.
            return Ok(false);
        };
        if animation.phase == CampaignAnimationPhase::WaitingForNextFrame {
            let first = animation
                .samples
                .first()
                .context("second campaign animation sample has no first sample")?;
            if emblem_frame == first.emblem_frame {
                return Ok(false);
            }
        }
        if animation.phase == CampaignAnimationPhase::WaitingForReopenedFrame {
            ensure!(
                emblem_frame == 0,
                "reopened campaign page did not render SCRA frame zero"
            );
        }
        let epoch = epoch.context("campaign animation sample has no page epoch")?;
        let elapsed_ms = Instant::now().saturating_duration_since(epoch).as_millis();
        let layout = campaign_score_layout(state);
        let roi = layout
            .control_rect(SCORE_ART_REGION_STATIC)
            .context("campaign score layout has no SCRA control ROI")?;
        animation.pending = Some(PendingCampaignAnimationSample {
            label,
            frame,
            elapsed_ms,
            emblem_frame,
            roi,
        });
        Ok(true)
    }

    pub(super) fn record_animation_sample(
        &mut self,
        state: &AppState,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<()> {
        let animation = self
            .animation
            .as_mut()
            .context("animation sample was recorded for a steady capture")?;
        let pending = animation
            .pending
            .take()
            .context("campaign animation readback has no post-render snapshot")?;
        ensure!(
            App::campaign_score_shell_active(state),
            "campaign score page closed before its sample readback completed"
        );
        let roi_pixels = crop_bgra_roi(pixels, width, height, pending.roi)?;
        let roi_changed_pixels_from_previous = animation
            .samples
            .last()
            .map(|previous| {
                let previous_roi = crop_bgra_roi(&previous.pixels, width, height, pending.roi)?;
                Ok::<_, anyhow::Error>(changed_bgra_pixel_count(&previous_roi, &roi_pixels)?)
            })
            .transpose()?;
        let sample = CampaignAnimationSample {
            label: pending.label,
            frame: pending.frame,
            elapsed_ms: pending.elapsed_ms,
            emblem_frame: pending.emblem_frame,
            roi: pending.roi,
            frame_sha256: crate::util::sha256::sha256_hex(pixels),
            roi_sha256: crate::util::sha256::sha256_hex(&roi_pixels),
            roi_changed_pixels_from_previous,
            pixels: pixels.to_vec(),
        };
        animation.samples.push(sample);
        match animation.samples.len() {
            1 => animation.phase = CampaignAnimationPhase::WaitingForNextFrame,
            2 => {
                let first = &animation.samples[0];
                let second = &animation.samples[1];
                ensure!(
                    first.emblem_frame != second.emblem_frame,
                    "campaign SCRA animation frame did not advance between rendered samples"
                );
                ensure!(
                    second
                        .roi_changed_pixels_from_previous
                        .is_some_and(|count| count > 0),
                    "campaign SCRA ROI pixels did not change between rendered samples"
                );
                animation.phase = CampaignAnimationPhase::Continue;
            }
            3 => {
                ensure!(
                    animation.samples[2].emblem_frame == 0,
                    "reopened campaign score page did not restart at SCRA frame zero"
                );
                animation.phase = CampaignAnimationPhase::Done;
            }
            count => bail!("campaign animation captured unexpected sample {count}"),
        }
        Ok(())
    }

    pub(super) fn animation_sequence_complete(&self) -> bool {
        self.animation.as_ref().is_some_and(|animation| {
            animation.phase == CampaignAnimationPhase::Done
                && animation.samples.len() == CAMPAIGN_ANIMATION_SAMPLE_COUNT
        })
    }

    pub(super) fn animation_payload(&self) -> Vec<u8> {
        self.animation
            .as_ref()
            .into_iter()
            .flat_map(|animation| animation.samples.iter())
            .flat_map(|sample| sample.pixels.iter().copied())
            .collect()
    }

    pub(super) fn animation_manifest(
        &self,
        request: &ShellCaptureRequest,
        format: wgpu::TextureFormat,
        sample_byte_length: u64,
    ) -> Value {
        let animation = self.animation.as_ref().expect("animation capture exists");
        let samples = animation.samples.iter().enumerate().map(|(index, sample)| {
            json!({
                "sample": index,
                "label": sample.label,
                "preview_png": format!("sample-{index:02}.png"),
                "capture_frame": sample.frame,
                "elapsed_ms_since_page_open": sample.elapsed_ms,
                "scra_frame_index": sample.emblem_frame,
                "roi": {"x": sample.roi.x, "y": sample.roi.y,
                    "width": sample.roi.w, "height": sample.roi.h},
                "frame_sha256": sample.frame_sha256,
                "roi_sha256": sample.roi_sha256,
                "roi_changed_pixels_from_previous": sample.roi_changed_pixels_from_previous,
                "byte_offset": index as u64 * sample_byte_length,
                "byte_length": sample_byte_length,
            })
        });
        json!({
            "schema_version": "vera20k.campaign-score-animation-capture.v1",
            "checkpoint": request.checkpoint.as_str(),
            "dialog_resource_id": 0x103,
            "fixture_kind": "synthetic UI review; no campaign execution",
            "parity_certification": "NONE",
            "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height,
                "format": format!("{format:?}"), "pixel_layout": "BGRA8",
                "row_order": "top-left", "row_stride": request.width * 4},
            "route": self.route,
            "samples": samples.collect::<Vec<_>>(),
            "frame_data": {"path": "frames.bgra",
                "byte_length": sample_byte_length * CAMPAIGN_ANIMATION_SAMPLE_COUNT as u64},
        })
    }

    pub(super) fn animation_sample_byte_length(&self, width: u32, height: u32) -> u64 {
        u64::from(width) * u64::from(height) * 4
    }

    pub(super) fn animation_png_frames(
        &self,
        width: u32,
        height: u32,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let animation = self.animation.as_ref().expect("animation capture exists");
        animation
            .samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let mut rgba = Vec::with_capacity(sample.pixels.len());
                for bgra in sample.pixels.chunks_exact(4) {
                    rgba.extend_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
                }
                let mut png = Vec::new();
                image::codecs::png::PngEncoder::new(&mut png).write_image(
                    &rgba,
                    width,
                    height,
                    image::ExtendedColorType::Rgba8,
                )?;
                Ok((format!("sample-{index:02}.png"), png))
            })
            .collect()
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if self.phase != CampaignScorePhase::Settling(0) {
            return Ok(false);
        }
        ensure!(
            App::campaign_score_shell_active(state),
            "campaign score capture left the 0x103 page"
        );
        ensure!(
            state.frontend.shell_controller.top_id()
                == Some(crate::ui::campaign_score_shell::CAMPAIGN_SCORE_DIALOG),
            "campaign score capture lost dialog 0x103"
        );
        Ok(state
            .frontend
            .campaign_score_page
            .as_ref()
            .is_some_and(|page| page.reveals_terminal()))
    }

    pub(super) fn manifest(
        &self,
        request: &ShellCaptureRequest,
        format: wgpu::TextureFormat,
        pixels: &[u8],
        frame: u32,
    ) -> Value {
        let fixture = synthetic_campaign_score_model();
        json!({
            "schema_version": "vera20k.campaign-score-shell-capture.v1",
            "checkpoint": request.checkpoint.as_str(),
            "dialog_resource_id": 0x103,
            "fixture_kind": "synthetic UI review; no campaign execution",
            "fixture_values": {
                "par_time": fixture.par_time,
                "your_time": fixture.your_time,
                "enemies_destroyed": fixture.enemies_destroyed,
                "rank": fixture.rank,
                "commendation": fixture.commendation,
            },
            "parity_certification": "NONE",
            "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height,
                "format": format!("{format:?}"), "pixel_layout": "BGRA8",
                "row_order": "top-left", "row_stride": request.width * 4},
            "cursor": {"x": request.cursor_x, "y": request.cursor_y,
                "policy": "software-composited"},
            "route": self.route,
            "capture_frame": frame,
            "frame": {"path": FRAME_FILE_NAME, "byte_length": pixels.len(),
                "sha256": crate::util::sha256::sha256_hex(pixels)}
        })
    }
}

fn campaign_score_layout(state: &AppState) -> CampaignScoreLayout {
    let (width, height) = crate::app::frontend::shell_layout::shell_screen_size(state);
    let fills_window = crate::app::frontend::shell_layout::fills_window(state);
    state.frontend.campaign_score_page.as_ref().map_or_else(
        || CampaignScoreLayout::compute_layout(width, height),
        |page| page.layout(width, height, fills_window),
    )
}

pub(super) fn click_campaign_continue(state: &mut AppState) -> Result<()> {
    let layout = campaign_score_layout(state);
    let button = layout.continue_button;
    state.match_state.input.cursor_x = (button.x + button.w / 2) as f32;
    state.match_state.input.cursor_y = (button.y + button.h / 2) as f32;
    App::handle_campaign_score_mouse_move(state);
    App::handle_campaign_score_mouse_down(state);
    ensure!(
        state.frontend.shell_controller.pressed() == Some(CONTINUE_BUTTON),
        "campaign Continue probe did not press control 0x423"
    );
    App::handle_campaign_score_mouse_up(state);
    ensure!(
        crate::app::frontend::shell_transition::shell_exit_wave(
            state,
            crate::app::frontend::shell_transition::ShellSlideKind::CampaignScore,
        )
        .is_some(),
        "campaign Continue probe did not start dialog 0x103's teardown slide"
    );
    Ok(())
}

fn crop_bgra_roi(pixels: &[u8], width: u32, height: u32, roi: RectPx) -> Result<Vec<u8>> {
    ensure!(
        roi.x >= 0 && roi.y >= 0 && roi.w > 0 && roi.h > 0,
        "invalid campaign ROI"
    );
    ensure!(
        i64::from(roi.x) + i64::from(roi.w) <= i64::from(width)
            && i64::from(roi.y) + i64::from(roi.h) <= i64::from(height),
        "campaign ROI falls outside the captured surface"
    );
    let expected_len = usize::try_from(u64::from(width) * u64::from(height) * 4)
        .context("campaign frame byte length does not fit usize")?;
    ensure!(
        pixels.len() == expected_len,
        "campaign ROI source is not a complete BGRA8 frame"
    );
    let row_stride = usize::try_from(width).context("campaign frame width does not fit usize")? * 4;
    let roi_x = usize::try_from(roi.x).context("campaign ROI x is negative")? * 4;
    let roi_width = usize::try_from(roi.w).context("campaign ROI width is negative")? * 4;
    let roi_y = usize::try_from(roi.y).context("campaign ROI y is negative")?;
    let roi_height = usize::try_from(roi.h).context("campaign ROI height is negative")?;
    let mut cropped = Vec::with_capacity(roi_width * roi_height);
    for y in roi_y..roi_y + roi_height {
        let start = y * row_stride + roi_x;
        cropped.extend_from_slice(&pixels[start..start + roi_width]);
    }
    Ok(cropped)
}

fn changed_bgra_pixel_count(before: &[u8], after: &[u8]) -> Result<usize> {
    ensure!(
        before.len() == after.len() && before.len() % 4 == 0,
        "campaign ROI comparison has mismatched BGRA8 data"
    );
    Ok(before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .filter(|(before, after)| before != after)
        .count())
}

/// The retail comparison game's page: a loss on DC Uprising, 800x600.
fn comparison_model(state: &AppState) -> Result<ScoreScreenModel> {
    let rules = state.rules().context("score capture needs the rules")?;
    let color = |name: &str| -> Result<[u8; 3]> {
        let entry = crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, name)
            .with_context(|| format!("[Colors] has no {name}"))?;
        Ok(crate::rules::house_colors::house_text_rgb(
            &rules.color_schemes,
            crate::rules::house_colors::HouseColorIndex(entry as u8),
        ))
    };
    let computer =
        crate::app::frontend::shell_pass::resolve_csf(state, "TXT_COMPUTER").into_owned();
    Ok(ScoreScreenModel {
        title_key: "GUI:SkirmishScore",
        game_number: 1,
        elapsed_seconds: 79,
        side: 0,
        rows: vec![
            ScoreRow {
                name: computer,
                rgb: color("DarkRed")?,
                kills: 0,
                losses: 0,
                built: 3,
                score: 0,
            },
            ScoreRow {
                name: "[New Player]".into(),
                rgb: color("DarkBlue")?,
                kills: 0,
                losses: 15,
                built: 0,
                score: 0,
            },
        ],
    })
}

/// Every kind-1 static of the page has run its reveal to the end.
pub(super) fn reveals_settled(state: &AppState) -> bool {
    state.frontend.shell_page_title.is_terminal()
        && state.frontend.shell_status_line.is_terminal()
        && state
            .frontend
            .score_page
            .as_ref()
            .is_some_and(|page| page.reveals_terminal())
}

impl ScoreCapture {
    pub(super) fn new(target: ScoreTarget) -> Self {
        Self {
            target,
            phase: Phase::MainMenu,
            route: Vec::new(),
        }
    }

    fn set_pointer(state: &mut AppState, point: (i32, i32)) {
        state.match_state.input.cursor_x = point.0 as f32;
        state.match_state.input.cursor_y = point.1 as f32;
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        if self.phase == Phase::MainMenu {
            if rendered == PresentedShell::MainMenu
                && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(state))?
            {
                let model = comparison_model(state)?;
                App::open_score_page(state, model, String::new(), String::new());
                self.route
                    .push(json!({"dialog": 0x108, "frame": frame, "action": "open"}));
                self.phase = Phase::Score;
            }
            return Ok(());
        }
        ensure!(
            App::score_shell_active(state),
            "score capture left the score page"
        );
        let entry_tick = state
            .frontend
            .shell_first_paint_slide
            .as_ref()
            .filter(|_| state.frontend.shell_slide_active_shell == Some(ShellSlideKind::Score))
            .and_then(|wave| wave.compatibility_tick());
        match self.phase {
            Phase::Score => match self.target {
                ScoreTarget::Entry(target) => {
                    if let Some(tick) = entry_tick {
                        ensure!(tick <= target, "entry slide passed tick {target}");
                        if tick == target {
                            if let Some(wave) = state.frontend.shell_first_paint_slide.as_mut() {
                                wave.hold_for_capture();
                            }
                            self.route.push(json!({"dialog": 0x108, "frame": frame,
                                "action": "hold entry slide", "tick": tick}));
                            self.phase = Phase::Settling(SETTLE_FRAMES);
                        }
                    }
                }
                _ if entry_tick.is_some() || !reveals_settled(state) => {}
                ScoreTarget::Steady => self.phase = Phase::Settling(SETTLE_FRAMES),
                ScoreTarget::HoverContinue | ScoreTarget::LeaveContinue => {
                    Self::set_pointer(state, CONTINUE_POINT);
                    App::handle_score_shell_mouse_move(state);
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "hover Continue"}));
                    self.phase = Phase::Hover;
                }
                ScoreTarget::SlideOut(_) => {
                    Self::set_pointer(state, CONTINUE_POINT);
                    App::handle_score_shell_mouse_move(state);
                    App::handle_score_shell_mouse_down(state);
                    App::handle_score_shell_mouse_up(state);
                    Self::set_pointer(state, (EXPECTED_CURSOR_X as i32, EXPECTED_CURSOR_Y as i32));
                    ensure!(
                        crate::app::frontend::shell_transition::shell_exit_wave(
                            state,
                            ShellSlideKind::Score
                        )
                        .is_some(),
                        "Continue did not start the 0x108 teardown slide"
                    );
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "Continue"}));
                    self.phase = Phase::Leaving;
                }
            },
            Phase::Hover if state.frontend.shell_status_line.is_terminal() => {
                if self.target == ScoreTarget::LeaveContinue {
                    Self::set_pointer(state, ART_POINT);
                    App::handle_score_shell_mouse_move(state);
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "move onto the art", "point": [ART_POINT.0, ART_POINT.1]}));
                    self.phase = Phase::Left;
                } else {
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            Phase::Left if state.frontend.shell_status_line.is_terminal() => {
                self.phase = Phase::Settling(SETTLE_FRAMES);
            }
            Phase::Leaving => {
                let ScoreTarget::SlideOut(target) = self.target else {
                    bail!("only the slide-out target leaves the page");
                };
                let tick = crate::app::frontend::shell_transition::shell_exit_wave(
                    state,
                    ShellSlideKind::Score,
                )
                .and_then(|wave| wave.compatibility_tick())
                .context("the 0x108 teardown slide ended before its held tick")?;
                ensure!(tick <= target, "teardown slide passed tick {target}");
                if tick == target {
                    crate::app::frontend::shell_transition::hold_shell_exit_for_capture(state);
                    self.route.push(json!({"dialog": 0x108, "frame": frame,
                        "action": "hold teardown slide", "tick": tick}));
                    self.phase = Phase::Settling(SETTLE_FRAMES);
                }
            }
            Phase::Settling(remaining) if remaining > 0 => {
                self.phase = Phase::Settling(remaining - 1);
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if self.phase != Phase::Settling(0) {
            return Ok(false);
        }
        ensure!(
            App::score_shell_active(state),
            "score capture left the score page"
        );
        Ok(match self.target {
            ScoreTarget::Entry(_) | ScoreTarget::SlideOut(_) => true,
            ScoreTarget::Steady | ScoreTarget::HoverContinue | ScoreTarget::LeaveContinue => {
                state.frontend.shell_first_paint_slide.is_none()
                    && state.frontend.shell_exit.is_none()
                    && reveals_settled(state)
            }
        })
    }

    pub(super) fn manifest(
        &self,
        request: &ShellCaptureRequest,
        format: wgpu::TextureFormat,
        pixels: &[u8],
        frame: u32,
    ) -> Value {
        json!({
            "schema_version": "vera20k.score-shell-capture.v1",
            "checkpoint": request.checkpoint.as_str(),
            "target": format!("{:?}", self.target),
            "parity_certification": "NONE", "presenter_domain": "final-swapchain-after-rgb565",
            "surface": {"width": request.width, "height": request.height, "format": format!("{format:?}"),
                "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": request.width * 4},
            "cursor": {"x": request.cursor_x, "y": request.cursor_y, "policy": "software-composited"},
            "route": self.route, "capture_frame": frame,
            "frame": {"path": FRAME_FILE_NAME, "byte_length": pixels.len(),
                "sha256": crate::util::sha256::sha256_hex(pixels)}
        })
    }
}
