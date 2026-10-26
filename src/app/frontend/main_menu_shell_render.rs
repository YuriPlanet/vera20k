//! Initial main-menu shell render glue for dialog 0xE2.

use std::sync::Arc;
use crate::util::time::Instant;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_pass::{owner_draw_button_label_rect, resolve_csf};
use crate::app::frontend::shell_transition::{
    ColumnDraw, MainMenuEntryPaintFrame, MainMenuEntryPresentToken, ShellFrameWave,
};
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::{MainMenuShellChromeAtlas, MainMenuShellChromeEntry};
use crate::render::shell_paint::{
    self, ArtFit, ButtonPolicy, CURSOR_DEPTH, MOVIE_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton,
    PaintLabel, SHELL_TEXT_RGB_ENABLED, push_entry_scaled,
};
use crate::render::shell_text::ShellAlign;
use crate::render::shell_text_reveal::PathAReveal;
use crate::render::shell_transition_pass::ShellRenderTarget;
use crate::render::shell_ui_style_assets::{
    ShellUiAssetLocator, ShellUiAssetPair, ShellUiAssetResolver,
};
use crate::ui::main_menu_shell::{
    MainMenuControlId, MainMenuMovieBase, MainMenuShellLayout, csf_key_for_control,
    tooltip_csf_key_for_control,
};
use crate::ui::shell::geom::{SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW};
use crate::ui::shell::static_reveal::{Kind1PaintWindow, Kind1RevealReceipt, Kind1RevealWindow};

/// Archive holding the Red Alert 2 menu loop used by the unchecked default.
///
/// `ra2ts_l.bik` exists under the same name in this archive and in `langmd.mix`
/// (Yuri's Revenge), whose copy normal priority resolves first. `ra2ts_s.bik`,
/// the 640-wide loop, ships only here, so both looks play it.
const RA2_MOVIE_ARCHIVE: &str = "language.mix";

/// Dialog 0xE2 owner-draw button policy: the SDBTNANM frame fills its 156x42
/// cell, right-anchored and vertically centred, so a stretched cell stretches
/// the art with it. At the authored cell size that placement is the native
/// top-left draw. The dialog has no hover flash or disabled owner-draw button.
const MAIN_MENU_BUTTON_POLICY: ButtonPolicy = ButtonPolicy {
    art_fit: ArtFit::FitRightAnchored {
        panel_w: SDBTNANM_CELL_W_NARROW as f32,
        tile_h: SDBTNANM_CELL_H as f32,
    },
    hover_flash: false,
    art_sink_y: 0.0,
};

pub(crate) enum MainMenuShellRenderResult {
    Rendered {
        title_receipt: Option<Kind1RevealReceipt>,
    },
    Fallback,
}

pub(crate) enum MainMenuEntryRenderResult {
    Rendered { token: MainMenuEntryPresentToken },
    Fallback,
}

/// Dialog whose child static `0x71A` owns the active RA2TS movie handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ra2tsDialogOwner {
    MainMenu0xE2,
    SinglePlayer0x100,
    MoviesAndCredits0x101,
}

/// Identity of the one RA2TS session installed for a movie-bearing dialog.
///
/// Owner and asset base change atomically because neither alone is sufficient
/// to make the decoder/texture reusable. VERA's YR Loading & Menu Movie
/// option picks which game's copy of the movie the session plays, so it is part
/// of the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ra2tsMovieSessionIdentity {
    owner: Ra2tsDialogOwner,
    base: MainMenuMovieBase,
    yr_original_style: bool,
}

impl Ra2tsMovieSessionIdentity {
    const fn new(
        owner: Ra2tsDialogOwner,
        base: MainMenuMovieBase,
        yr_original_style: bool,
    ) -> Self {
        Self {
            owner,
            base,
            yr_original_style,
        }
    }

    pub(crate) const fn owner(self) -> Ra2tsDialogOwner {
        self.owner
    }

    pub(crate) const fn base(self) -> MainMenuMovieBase {
        self.base
    }
}

fn ra2ts_movie_session_is_reusable(
    movie_loaded: bool,
    active_identity: Option<Ra2tsMovieSessionIdentity>,
    requested_identity: Ra2tsMovieSessionIdentity,
) -> bool {
    movie_loaded && active_identity == Some(requested_identity)
}

/// Drop the one active RA2TS decoder/texture and all of its cache identity.
///
/// This models destruction of the owning dialog's child static `0x71A`. The
/// next movie-bearing dialog reconstructs its own session lazily on first paint.
pub(crate) fn clear_ra2ts_movie_session(state: &mut AppState) {
    state.frontend.main_menu_movie = None;
    state.frontend.main_menu_movie_identity = None;
    state.frontend.main_menu_movie_last_step = Instant::now();
}

/// The slide a painted `0xE2` frame belongs to.
#[derive(Debug, Clone)]
enum MainMenuSlide {
    Steady,
    /// The first-paint slide-in, one presented tick.
    Entry(MainMenuEntryPaintFrame),
    /// The teardown slide-out (`0x00608070`).
    Exit(ShellFrameWave),
}

impl MainMenuSlide {
    /// The slide engine's column draws, which replace the buttons while it runs.
    fn column_draws(&self) -> Option<Vec<ColumnDraw>> {
        match self {
            MainMenuSlide::Steady => None,
            MainMenuSlide::Entry(frame) => Some(frame.button_draws()),
            MainMenuSlide::Exit(wave) => Some(wave.button_draws()),
        }
    }
}

/// Map the layout + shell state into the owner-draw button list for the steady
/// paint pass. 0xE2 never disables a control, so every button is `enabled: true`.
fn main_menu_paint_buttons(
    layout: &MainMenuShellLayout,
    pressed_button: Option<MainMenuControlId>,
) -> Vec<PaintButton> {
    layout
        .buttons
        .iter()
        .map(|button| PaintButton {
            rect: button.rect,
            pressed: pressed_button == Some(button.id),
            hovered: false, // 0xE2 never flashes; hover state is unused on art
            enabled: true,
        })
        .collect()
}

pub(crate) fn main_menu_title_text(state: &AppState) -> std::borrow::Cow<'_, str> {
    resolve_csf(state, "GUI:MainMenu")
}

fn main_menu_status_csf_key(hovered_button: Option<MainMenuControlId>) -> Option<&'static str> {
    hovered_button.map(tooltip_csf_key_for_control)
}

/// Path-A tint for a right-panel kind-1 static (heading `0x694`, status line
/// `0x695`): yellow text with a white highlight trail.
pub(crate) fn shell_reveal_path_a(window: Kind1RevealWindow) -> PathAReveal {
    PathAReveal {
        count: window.count,
        range: window.range,
        base_rgb: [255, 255, 0],
        highlight_rgb: [255, 255, 255],
    }
}

/// Build the owner-draw button labels and dialog statics consumed by
/// `shell_paint::paint_labels`. Button labels use the exact native normal/pressed
/// clipping rectangles. Status static `0x695` reads the dialog's immediate hover
/// state rather than the delayed in-game tooltip service.
fn main_menu_paint_labels<'a>(
    state: &'a AppState,
    layout: &MainMenuShellLayout,
    pressed_button: Option<MainMenuControlId>,
    version_text: Option<&'a str>,
    title_window: Option<Kind1RevealWindow>,
    captions: bool,
) -> Vec<PaintLabel<'a>> {
    use crate::render::shell_text::ShellAlign;
    let mut out = Vec::new();
    let button_align = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
    // The slide engine draws the button frames only (`0x006071E0`).
    for button in layout.buttons.iter().filter(|_| captions) {
        let pressed = pressed_button == Some(button.id);
        out.push(PaintLabel {
            text: resolve_csf(state, csf_key_for_control(button.id)),
            rect: owner_draw_button_label_rect(button.rect, pressed),
            align: button_align,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    // Statics: title heading, version, tooltip — top-anchored, h-centered.
    if let Some(window) = title_window {
        out.push(PaintLabel {
            text: main_menu_title_text(state),
            rect: layout.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    out.extend(version_text.map(|text| PaintLabel {
        text: text.into(),
        rect: layout.version_line,
        align: ShellAlign::H_CENTER,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    }));
    out
}

/// Select the parent-background SHP: MNSCRNS only at exactly 640 wide, else
/// MNSCRNL (mirrors gamemd's `g_ScreenWidth == 640` switch).
fn select_parent_background(
    screen_w: i32,
    mnscrns_640: Option<MainMenuShellChromeEntry>,
    mnscrnl_large: Option<MainMenuShellChromeEntry>,
) -> Option<MainMenuShellChromeEntry> {
    if screen_w == 640 {
        mnscrns_640
    } else {
        mnscrnl_large
    }
}

/// The MNSCRN parent background of a right-panel shell at `screen_w` x
/// `screen_h`: native SHP canvas at the centered shell origin on the retail
/// layout, scaled with the dialog when the layout scales the shell.
/// Behind the `0xE2` movie and chrome, and in the RA2TS area of the menu pages
/// while no movie is drawn.
pub(crate) fn shell_parent_background_instances(
    atlas: &MainMenuShellChromeAtlas,
    screen_w: i32,
    screen_h: i32,
    scale: crate::ui::shell::geom::ShellScale,
) -> Vec<SpriteInstance> {
    // A scaled page is authored at 800 wide, so it takes the large art.
    let art_w = if scale.is_retail() {
        screen_w
    } else {
        crate::ui::shell::geom::SHELL_BASE_W
    };
    let Some(entry) = select_parent_background(
        art_w,
        atlas.parent_background_640_mnscrns,
        atlas.parent_background_large_mnscrnl,
    ) else {
        return Vec::new();
    };
    // The retail dialog is centred by gamemd's origin; a scaled one starts at
    // its authored origin, which the scale places in the window.
    let (x, y) = if scale.is_retail() {
        crate::ui::shell::geom::dialog_origin(screen_w, screen_h)
    } else {
        (0, 0)
    };
    let mut out = Vec::new();
    push_entry_scaled(&mut out, entry, x, y, scale, PARENT_BACKGROUND_DEPTH);
    out
}

fn movie_instance(layout: &MainMenuShellLayout) -> SpriteInstance {
    SpriteInstance {
        position: [layout.movie.x as f32, layout.movie.y as f32],
        size: [layout.movie.w as f32, layout.movie.h as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: MOVIE_DEPTH,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }
}

fn build_movie_instances(layout: &MainMenuShellLayout) -> Vec<SpriteInstance> {
    vec![movie_instance(layout)]
}

pub(crate) fn ensure_movie_for_current_layout(
    state: &mut AppState,
    requested_owner: Ra2tsDialogOwner,
) -> Result<()> {
    let layout = shell_layout::main_menu_layout(state);
    let yr_original_style = state.persistence.options_profile.yr_original_style;
    let requested_identity =
        Ra2tsMovieSessionIdentity::new(requested_owner, layout.movie_base, yr_original_style);
    if ra2ts_movie_session_is_reusable(
        state.frontend.main_menu_movie.is_some(),
        state.frontend.main_menu_movie_identity,
        requested_identity,
    ) {
        return Ok(());
    }
    clear_ra2ts_movie_session(state);

    let Some(assets) = state.process_assets.manager() else {
        state
            .frontend
            .main_menu_shell_error
            .get_or_insert_with(|| "Game archives are unavailable.".to_owned());
        return Ok(());
    };
    let asset_name = layout.movie_base.asset_name();

    let Some((bytes, source)) = menu_movie_bytes(assets, asset_name, yr_original_style) else {
        log::warn!("Missing main-menu movie asset {asset_name}");
        state.frontend.main_menu_shell_error = Some(format!("Missing menu movie: {asset_name}"));
        return Ok(());
    };

    let movie = match crate::render::bink_movie::BinkMovieSurface::from_bytes(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        Arc::<[u8]>::from(bytes),
        source.to_string(),
        true,
        wgpu::FilterMode::Nearest,
    ) {
        Ok(movie) => movie,
        Err(err) => {
            log::warn!("Failed to load main-menu movie {asset_name} from {source}: {err:#}");
            state.frontend.main_menu_shell_error = Some(format!(
                "Could not load {asset_name} from {source}: {err:#}"
            ));
            return Ok(());
        }
    };
    log::info!(
        "Loaded {asset_name} for main menu from {}",
        movie.source_archive()
    );
    state.frontend.main_menu_movie = Some(movie);
    state.frontend.main_menu_movie_identity = Some(requested_identity);
    state.frontend.main_menu_movie_last_step = Instant::now();
    Ok(())
}

/// The menu movie's bytes and the archive they came from.
///
/// Native YR 1.11 gamemd.exe, GZF 2026-10-15, VA 0x00531cc0
/// (MainMenuDialog0xE2__Run), selects Ra2ts_s at width 640 and Ra2ts_l
/// otherwise, then gives that name to the movie control. Resolve through the
/// shared first-match asset lookup, preserving loose-file and archive priority.
/// VERA's YR Loading & Menu Movie option uses the ordinary first-match
/// lookup. The unchecked default requires the named member from
/// [`RA2_MOVIE_ARCHIVE`]; a missing RA2 copy is reported instead of silently
/// resolving the YR member with the same name.
fn menu_movie_bytes<'a>(
    assets: &'a crate::assets::asset_manager::AssetManager,
    asset_name: &str,
    yr_original_style: bool,
) -> Option<(&'a [u8], &'a str)> {
    let resolver = ShellUiAssetResolver::new(assets, yr_original_style);
    resolver
        .resolve(ShellUiAssetPair::new(
            ShellUiAssetLocator::archive(asset_name, RA2_MOVIE_ARCHIVE),
            ShellUiAssetLocator::active(asset_name),
        ))
        .map(|asset| (asset.bytes, asset.source))
        .map_err(|err| {
            log::warn!("Could not resolve {asset_name} for the selected shell style: {err:#}")
        })
        .ok()
}

/// Paint the normal 0xE2 route through its active-retail presentation boundary.
///
/// First-paint callers use [`render_main_menu_first_paint_frame`] so entry and
/// steady paint share this exact RGB565 presentation boundary.
pub(crate) fn render_main_menu_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<MainMenuShellRenderResult> {
    let slide = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        crate::app::frontend::shell_transition::ShellSlideKind::MainMenu,
    )
    .cloned()
    .map_or(MainMenuSlide::Steady, MainMenuSlide::Exit);
    // The teardown slide leaves the heading blank.
    let (title_window, title_receipt) = if matches!(slide, MainMenuSlide::Exit(_)) {
        (None, None)
    } else {
        match state
            .frontend
            .main_menu_shell_state
            .title_reveal
            .paint_window()
        {
            Kind1PaintWindow::Hidden => (None, None),
            Kind1PaintWindow::Retained(window) => (Some(window), None),
            Kind1PaintWindow::Due { window, receipt } => (Some(window), Some(receipt)),
        }
    };
    let color = state.renderer.shell_surface_presenter.source_render_view();
    let depth = state.renderer.depth_view.clone();
    let result = render_main_menu_shell_to_target_inner(
        state,
        encoder,
        ShellRenderTarget {
            color: &color,
            depth: &depth,
        },
        title_window,
        slide,
    )?;
    match result {
        MainMenuShellRenderResult::Rendered { .. } => {
            state
                .renderer
                .shell_surface_presenter
                .encode_present(encoder, destination);
            Ok(MainMenuShellRenderResult::Rendered { title_receipt })
        }
        MainMenuShellRenderResult::Fallback => Ok(MainMenuShellRenderResult::Fallback),
    }
}

pub(crate) fn render_main_menu_first_paint_frame(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    frame: MainMenuEntryPaintFrame,
) -> Result<MainMenuEntryRenderResult> {
    let color = state.renderer.shell_surface_presenter.source_render_view();
    let depth = state.renderer.depth_view.clone();
    let result = render_main_menu_shell_to_target_inner(
        state,
        encoder,
        ShellRenderTarget {
            color: &color,
            depth: &depth,
        },
        None,
        MainMenuSlide::Entry(frame),
    )?;
    match result {
        MainMenuShellRenderResult::Rendered { .. } => {
            state
                .renderer
                .shell_surface_presenter
                .encode_present(encoder, destination);
            let token = state
                .frontend
                .shell_first_paint_slide
                .as_ref()
                .and_then(|wave| wave.mint_present_token(frame))
                .ok_or_else(|| {
                    anyhow::anyhow!("main-menu entry frame changed before presenter token mint")
                })?;
            Ok(MainMenuEntryRenderResult::Rendered { token })
        }
        MainMenuShellRenderResult::Fallback => Ok(MainMenuEntryRenderResult::Fallback),
    }
}

fn render_main_menu_shell_to_target_inner(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
    title_window: Option<Kind1RevealWindow>,
    slide: MainMenuSlide,
) -> Result<MainMenuShellRenderResult> {
    // States 6 and 7 (Exit confirmation, then the quit) run after 0xE2 is
    // destroyed: the confirmation shows over the empty shell backdrop
    // (`0x0052FEC0`). Retail leaves the destroyed box's last image on screen
    // through the quit fade (nothing repaints after OK); here the backdrop
    // shows alone (evidence note residual).
    let backdrop =
        state.frontend.shell_message_box.is_some() || state.frontend.quit_cascade.is_some();
    if !backdrop {
        ensure_movie_for_current_layout(state, Ra2tsDialogOwner::MainMenu0xE2)?;
    }
    if state.frontend.main_menu_shell_error.is_some()
        || state.frontend.main_menu_shell_chrome.is_none()
    {
        state
            .frontend
            .main_menu_shell_error
            .get_or_insert_with(|| "Required game-menu artwork is unavailable.".to_owned());
        return Ok(MainMenuShellRenderResult::Fallback);
    }

    // While either slide runs the RA2TS static shows no movie and gets no
    // timer (`0x006071E0`; the teardown also stops it with 0x4E2), so the
    // parent background shows and the movie clock starts after the slide.
    let sliding = !matches!(slide, MainMenuSlide::Steady);
    let leaving = matches!(slide, MainMenuSlide::Exit(_));
    let movie_shown = !sliding && !backdrop;
    if !movie_shown {
        state.frontend.main_menu_movie_last_step = Instant::now();
    } else if let Some(movie) = state.frontend.main_menu_movie.as_mut() {
        let now = Instant::now();
        let elapsed = now
            .duration_since(state.frontend.main_menu_movie_last_step)
            .as_secs_f64();
        state.frontend.main_menu_movie_last_step = now;
        if let Err(err) = movie.step(&state.renderer.gpu, elapsed) {
            log::warn!("Failed to step main-menu RA2TS movie: {err:#}");
            state.frontend.main_menu_shell_error =
                Some(format!("Could not play the menu movie: {err:#}"));
            return Ok(MainMenuShellRenderResult::Fallback);
        }
    }

    let layout = shell_layout::main_menu_layout(state);
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // that covers the children, and the engine pumps no messages until it
    // ends: the heading, status and version statics stay blank and the
    // monitor window shows the right panel's own art underneath.
    let monitor_frame = if backdrop || leaving {
        None
    } else {
        crate::app::frontend::menu_page_render::paint_shell_monitor(state)
    };
    let status_label = if leaving || backdrop {
        None
    } else {
        let hovered = state
            .frontend
            .main_menu_shell_state
            .hovered_owner_draw_button;
        let status_text = main_menu_status_csf_key(hovered)
            .map(|key| resolve_csf(state, key).into_owned())
            .unwrap_or_default();
        crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            layout.tooltip_line,
        )
    };
    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    let movie_texture = state
        .frontend
        .main_menu_movie
        .as_ref()
        .map(|movie| movie.batch_texture());

    // 0xE2-only MNSCRN parent background, submitted FIRST (no analog on 0x100).
    let background_instances =
        shell_parent_background_instances(chrome, layout.screen.w, layout.screen.h, layout.scale);
    let movie_instances = if movie_shown && movie_texture.is_some() {
        build_movie_instances(&layout)
    } else {
        Vec::new()
    };
    let mut chrome_instances = shell_paint::paint_chrome(
        chrome,
        layout.right_panel,
        Some(layout.lower_strip),
        layout.screen.w,
    );
    chrome_instances.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.warning_monitor, frame, layout.scale)
    }));
    // The slide engine and the empty backdrop draw the whole tile column in
    // place of the buttons.
    let column = if backdrop {
        Some(shell_paint::shuttered_column(layout.right_panel))
    } else {
        slide.column_draws()
    };
    let buttons = match column {
        Some(draws) => {
            chrome_instances.extend(shell_paint::paint_slide_column(
                chrome,
                layout.right_panel,
                &draws,
            ));
            Vec::new()
        }
        None => main_menu_paint_buttons(
            &layout,
            state
                .frontend
                .main_menu_shell_state
                .pressed_owner_draw_button,
        ),
    };
    // 0xE2 never flashes, so the hover clock is unused (None) — keep the call
    // shape uniform with 0x100, which threads its hover_started_at.
    let button_instances = shell_paint::paint_buttons(
        chrome,
        &buttons,
        MAIN_MENU_BUTTON_POLICY,
        Instant::now(),
        None,
    );
    let version_text = format!(
        "{} {}",
        resolve_csf(state, "GUI:Version"),
        state.frontend.version_txt
    );
    let pressed = state
        .frontend
        .main_menu_shell_state
        .pressed_owner_draw_button;
    let mut labels = if backdrop {
        Vec::new()
    } else {
        main_menu_paint_labels(
            state,
            &layout,
            pressed,
            (!leaving).then_some(version_text.as_str()),
            title_window,
            !sliding,
        )
    };
    labels.extend(status_label);
    let text_draws =
        shell_paint::paint_labels_scaled(&state.renderer.bit_font, &labels, layout.scale.factor());

    // Quit-confirm SHP modal overlay (blocking; drawn over the menu, under the
    // cursor). `None` when the modal is closed or the skirmish atlas (which holds
    // PUDLGBGN/MNBTTN) is not loaded.
    let modal_overlay = build_shell_message_box_overlay(state);
    let skirmish_chrome = state.frontend.skirmish_shell_chrome.as_ref();

    state.renderer.batch_renderer.update_camera(
        &state.renderer.gpu,
        state.renderer.gpu.config.width as f32,
        state.renderer.gpu.config.height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let background_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &background_instances);
    let movie_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &movie_instances);
    let chrome_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &chrome_instances);
    let button_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &button_instances);
    let text_buffers: Vec<_> = text_draws
        .iter()
        .map(|draw| {
            state
                .renderer
                .batch_renderer
                .create_instance_buffer(&state.renderer.gpu, &draw.instances)
        })
        .collect();
    let modal_sprite_buffer = modal_overlay.as_ref().and_then(|m| {
        state
            .renderer
            .batch_renderer
            .create_instance_buffer(&state.renderer.gpu, &m.sprites)
    });
    let modal_text_buffers: Vec<_> = modal_overlay
        .as_ref()
        .map(|m| {
            m.text
                .iter()
                .map(|draw| {
                    state
                        .renderer
                        .batch_renderer
                        .create_instance_buffer(&state.renderer.gpu, &draw.instances)
                })
                .collect()
        })
        .unwrap_or_default();
    let cursor_instances: Vec<SpriteInstance> =
        crate::app::frontend::shell_pass::software_cursor(state, CURSOR_DEPTH)
            .map(|(_, instance)| instance)
            .into_iter()
            .collect();
    let cursor_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &cursor_instances);
    // Default-cursor frame-0 texture, borrowed for the duration of the pass.
    let cursor_texture = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()
        .and_then(|cursor| cursor.get(crate::app::types::CursorId::Default))
        .and_then(|sequence| sequence.frames.first())
        .map(|frame| &frame.texture);

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Main Menu Shell"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: target.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some((buffer, count)) = background_buffer.as_ref() {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            &chrome.texture,
            buffer,
            *count,
        );
    }
    if let (Some(texture), Some((buffer, count))) = (movie_texture, movie_buffer.as_ref()) {
        state
            .renderer
            .batch_renderer
            .draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    if let Some((buffer, count)) = chrome_buffer.as_ref() {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            &chrome.texture,
            buffer,
            *count,
        );
    }
    if let Some((buffer, count)) = button_buffer.as_ref() {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            &chrome.texture,
            buffer,
            *count,
        );
    }
    for (draw, buffer) in text_draws.iter().zip(text_buffers.iter()) {
        let Some((buffer, count)) = buffer.as_ref() else {
            continue;
        };
        pass.set_scissor_rect(
            draw.scissor.x,
            draw.scissor.y,
            draw.scissor.w,
            draw.scissor.h,
        );
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    pass.set_scissor_rect(
        0,
        0,
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
    // Quit-confirm modal overlay: SHP panel + buttons (skirmish atlas texture),
    // then labels (font atlas), above the menu but below the cursor.
    if let (Some(overlay), Some(sk_chrome)) = (modal_overlay.as_ref(), skirmish_chrome) {
        if let Some((buffer, count)) = modal_sprite_buffer.as_ref() {
            state.renderer.batch_renderer.draw_with_buffer_passthrough(
                &mut pass,
                &sk_chrome.texture,
                buffer,
                *count,
            );
        }
        for (draw, buffer) in overlay.text.iter().zip(modal_text_buffers.iter()) {
            let Some((buffer, count)) = buffer.as_ref() else {
                continue;
            };
            pass.set_scissor_rect(
                draw.scissor.x,
                draw.scissor.y,
                draw.scissor.w,
                draw.scissor.h,
            );
            state.renderer.batch_renderer.draw_with_buffer_passthrough(
                &mut pass,
                state.renderer.bit_font.atlas(),
                buffer,
                *count,
            );
        }
        pass.set_scissor_rect(
            0,
            0,
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
    }
    if let (Some((buffer, count)), Some(texture)) = (cursor_buffer.as_ref(), cursor_texture) {
        state
            .renderer
            .batch_renderer
            .draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);

    Ok(MainMenuShellRenderResult::Rendered {
        title_receipt: None,
    })
}

/// Back-to-front depths for the quit-confirm modal overlay. They sit in the clear
/// band between the menu's front-most text (`TEXT_DEPTH`) and the cursor
/// (`CURSOR_DEPTH`), so the modal blocks the menu while the cursor stays on top.
const EXIT_CONFIRM_MODAL_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: 0.00050,
    button: 0.00045,
    text: 0.00040,
};

/// Build the shell message box (0xCE/0x120) SHP modal overlay when it is open: the centered
/// PUDLGBGN panel + populated MNBTTN buttons and labels, sourced from
/// the skirmish chrome atlas (the only atlas that loads PUDLGBGN/MNBTTN). The
/// pressed button is read from the shared shell controller (its top dialog is the
/// modal while open). Returns `None` when the modal is closed or its art is absent.
fn build_shell_message_box_overlay(state: &AppState) -> Option<shell_paint::ModalDraw> {
    use crate::ui::shell::modal;
    let modal_state = &state.frontend.shell_message_box.as_ref()?.labels;
    let atlas = state.frontend.skirmish_shell_chrome.as_ref()?;
    let layout = shell_layout::shell_message_box_layout(state, modal_state.kind());
    let pressed = state.frontend.shell_controller.pressed();
    let ok_pressed = pressed == Some(modal::control::OK);
    let cancel_pressed = pressed == Some(modal::control::CANCEL);
    let frames = crate::app::frontend::skirmish_shell_render::type3_button_frames(atlas);
    let mut buttons = vec![shell_paint::ModalButton {
        rect: layout.ok,
        pressed: ok_pressed,
        enabled: true,
    }];
    // Body left-top wrapped; OK/Cancel centered on their buttons with the MNBTTN
    // press sink — same conventions as the skirmish validation modal.
    let mut labels = vec![
        PaintLabel {
            text: (&modal_state.title).into(),
            rect: layout.body,
            align: ShellAlign::NONE,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        },
        PaintLabel {
            text: (&modal_state.confirm).into(),
            rect: layout.scale.derive_rect(layout.ok, |rect| {
                owner_draw_button_label_rect(rect, ok_pressed)
            }),
            align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        },
    ];
    if let Some(rect) = layout.cancel {
        buttons.push(shell_paint::ModalButton {
            rect,
            pressed: cancel_pressed,
            enabled: true,
        });
        labels.push(PaintLabel {
            text: (&modal_state.cancel).into(),
            rect: layout.scale.derive_rect(rect, |rect| {
                owner_draw_button_label_rect(rect, cancel_pressed)
            }),
            align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    Some(shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        atlas.mode_two_dialog_background(None),
        frames,
        layout.dialog,
        &buttons,
        &labels,
        layout.scale,
        EXIT_CONFIRM_MODAL_DEPTHS,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn default_style_plays_ra2_movie_and_option_selects_yuri_revenge_copy() {
        use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
        let retail_dir = std::path::PathBuf::from(
            std::env::var_os("RA2_DIR").expect("set RA2_DIR to the retail RA2/YR install"),
        );
        let assets = AssetManager::new(&retail_dir, MediaArchiveMode::STOCK_DIGITAL)
            .expect("load retail RA2/YR assets");
        // The large loop ships in both games' archives: the default uses RA2,
        // while the option uses the active Yuri's Revenge copy.
        let large = MainMenuMovieBase::Ra2tsL.asset_name();
        let (ra2, ra2_source) = menu_movie_bytes(&assets, large, false).expect("RA2 large loop");
        let (yr, yr_source) = menu_movie_bytes(&assets, large, true).expect("YR large loop");
        assert_eq!((ra2_source, yr_source), (RA2_MOVIE_ARCHIVE, "langmd.mix"));
        assert!(ra2 != yr, "both looks play the same large loop");
        // The 640-wide loop ships only in language.mix, so both looks play it.
        let small = MainMenuMovieBase::Ra2tsS.asset_name();
        let (ra2, ra2_source) = menu_movie_bytes(&assets, small, false).expect("small loop");
        let (yr, yr_source) = menu_movie_bytes(&assets, small, true).expect("small loop");
        assert_eq!(
            (yr_source, ra2_source),
            (RA2_MOVIE_ARCHIVE, RA2_MOVIE_ARCHIVE)
        );
        assert!(yr == ra2, "the small loop differs between looks");
    }
}
