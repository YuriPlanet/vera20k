//! Initial main-menu shell render glue for dialog 0xE2.

use crate::util::time::{Duration, Instant};
use std::sync::Arc;

use anyhow::Result;

use crate::app::BUILD_STAMP;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, FamilyPage, PanelState, encode_family_page, help_text, paint_shell_monitor,
    paint_shell_status_line,
};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::app::frontend::shell_transition::{
    ColumnDraw, ExternalMenuWait, ShellFrameWave, external_menu_wait,
};
use crate::app::frontend::state::FrontendState;
use crate::app::frontend::state::ShellMessageBox;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::mods::Mods;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::assets::asset_manager::AssetManager;
use crate::assets::bink_file::BinkHeader;
use crate::render::batch::SpriteInstance;
use crate::render::bink_movie::BinkMovieSurface;
use crate::render::gpu::GpuContext;
use crate::render::main_menu_shell_chrome::MainMenuShellChromeAtlas;
use crate::render::shell_paint::{
    self, CURSOR_DEPTH, MOVIE_DEPTH, PaintLabel, SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text_reveal::PathAReveal;
use crate::render::shell_ui_style_assets::{
    ShellUiAssetLocator, ShellUiAssetPair, ShellUiAssetResolver, mod_art_candidates,
};
use crate::ui::main_menu_shell::{MAIN_MENU_PAGE, MainMenuMovieBase, MainMenuShellLayout};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::static_reveal::Kind1RevealWindow;

/// Archive holding the Red Alert 2 menu loop used by the unchecked default.
///
/// `ra2ts_l.bik` exists under the same name in this archive and in `langmd.mix`
/// (Yuri's Revenge), whose copy normal priority resolves first. `ra2ts_s.bik`,
/// the 640-wide loop, ships only here, so both looks play it.
const RA2_MOVIE_ARCHIVE: &str = "language.mix";

pub(crate) enum MainMenuShellRenderResult {
    Rendered,
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
/// to make the decoder/texture reusable. VERA's Original YR Style option picks
/// which game's copy of the movie the session plays, so it is part of the
/// identity.
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

/// Time between consecutive paints of a playing menu movie.
#[derive(Default)]
pub(crate) struct MenuMovieClock {
    last_paint: Option<Instant>,
}

impl MenuMovieClock {
    /// A focus or visibility change can pass without a single menu redraw.
    pub(crate) fn pause(&mut self) {
        self.last_paint = None;
    }

    fn elapsed(&mut self, now: Instant, playing: bool) -> Option<Duration> {
        if !playing {
            self.pause();
            return None;
        }
        self.last_paint.replace(now).map(|last| now - last)
    }
}

/// Drop the one active RA2TS decoder/texture and all of its cache identity.
///
/// This models destruction of the owning dialog's child static `0x71A`. The
/// next movie-bearing dialog reconstructs its own session lazily on first paint.
pub(crate) fn clear_ra2ts_movie_session(frontend: &mut FrontendState) {
    frontend.main_menu_movie = None;
    frontend.main_menu_movie_identity = None;
    frontend.main_menu_movie_clock.pause();
}

/// The slide a painted `0xE2` frame belongs to.
#[derive(Debug, Clone)]
enum MainMenuSlide {
    Steady,
    /// The first-paint slide-in.
    Entry(ShellFrameWave),
    /// The teardown slide-out (`0x00608070`).
    Exit(ShellFrameWave),
}

impl MainMenuSlide {
    /// The slide engine's column draws, which replace the buttons while it runs.
    fn column_draws(&self) -> Option<Vec<ColumnDraw>> {
        match self {
            MainMenuSlide::Steady => None,
            MainMenuSlide::Entry(wave) | MainMenuSlide::Exit(wave) => Some(wave.button_draws()),
        }
    }
}

pub(crate) fn main_menu_title_text(process_assets: &ProcessAssets) -> std::borrow::Cow<'_, str> {
    process_assets.csf_text(MAIN_MENU_PAGE.title_key).into()
}

/// What 0xE2 shows in place of its live controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainMenuCover {
    /// Buttons, statics and the hover status line.
    Live,
    /// The empty shell backdrop: shuttered column, no statics, no movie.
    Backdrop,
    /// VERA: the backdrop of an external network launch that is joining its
    /// match (docs/spawn.md), with a connecting notice on the status line.
    /// gamemd has no such launch; buttons that could not be pressed made the
    /// wait look frozen.
    Joining,
}

impl MainMenuCover {
    fn new(modal: bool, external: Option<ExternalMenuWait>) -> Self {
        match external {
            _ if modal => Self::Backdrop,
            Some(ExternalMenuWait::Joining) => Self::Joining,
            Some(ExternalMenuWait::Reporting) => Self::Backdrop,
            None => Self::Live,
        }
    }

    fn of(frontend: &FrontendState) -> Self {
        Self::new(
            frontend
                .shell_message_box
                .as_ref()
                .is_some_and(|message_box| !message_box.keeps_page())
                || frontend.quit_cascade.is_some()
                || crate::app::frontend::shell_transition::update_check_holds_main_menu(frontend),
            external_menu_wait(frontend),
        )
    }
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

/// 0xE2's own statics. VERA difference: gamemd prints "<GUI:Version>
/// <VERSION.TXT>" on the version line, a fixed "1.001TUC" in every build;
/// this line names the VERA20K release instead, the version players report
/// and updates compare, on the status line's baseline rather than gamemd's
/// version-line top, so the two bottom texts line up. The teardown slide
/// leaves it blank.
fn version_labels(mods: &Mods, layout: &MainMenuShellLayout) -> Vec<PaintLabel<'static>> {
    let version_rect = RectPx {
        y: layout.tooltip_line.y,
        ..layout.version_line
    };
    let mut out = vec![PaintLabel {
        text: BUILD_STAMP.into(),
        rect: version_rect,
        align: ShellAlign::H_CENTER,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    }];
    // VERA: the mod in use (`app::mods`), on the same line, between the
    // status help and the right panel. Retail 640x480 has no room there.
    if let Some(active) = mods.active.as_ref() {
        // As far from the panel as the status help is from the strip's left.
        let gap = layout.tooltip_line.x - layout.lower_strip.x;
        let left = layout.tooltip_line.x + layout.tooltip_line.w;
        let width = layout.right_panel.top.x - gap - left;
        if width > 0 {
            out.push(PaintLabel {
                text: crate::i18n::tr_fill("VERA:MainMenuMod", &[("mod", &active.title)]).into(),
                rect: RectPx::new(left, version_rect.y, width, version_rect.h),
                align: ShellAlign::H_RIGHT,
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            });
        }
    }
    out
}

/// The MNSCRN parent background of a right-panel shell at `screen_w` x
/// `screen_h` (`0x0060D20B`: MNSCRNL, MNSCRNS at 640 wide): behind the `0xE2`
/// movie and chrome, and in the RA2TS area of the menu pages while no movie
/// is drawn. The retail layout draws the canvas at the centred dialog origin;
/// a scaled page grows it with the page.
pub(crate) fn shell_parent_background_instances(
    atlas: &MainMenuShellChromeAtlas,
    screen_w: i32,
    screen_h: i32,
    scale: crate::ui::shell::geom::ShellScale,
) -> Vec<SpriteInstance> {
    let (entry, (x, y)) = match scale.is_unscaled() {
        true if screen_w == 640 => (
            atlas.parent_background_640_mnscrns,
            crate::ui::shell::geom::dialog_origin(screen_w, screen_h),
        ),
        true => (
            atlas.parent_background_large_mnscrnl,
            crate::ui::shell::geom::dialog_origin(screen_w, screen_h),
        ),
        false => (atlas.parent_background_large_mnscrnl, (0, 0)),
    };
    let mut out = Vec::new();
    if let Some(entry) = entry {
        let depth = shell_paint::PARENT_BACKGROUND_DEPTH;
        shell_paint::push_entry_scaled(&mut out, entry, x, y, scale, depth);
    }
    out
}

/// A family list box `window` (`0x00619230`) over that parent background:
/// inside its inner frame ring the background one RGB565 unit darker
/// (translucency 0), and, when the list has a scrollbar `bar`, the
/// background undarkened inside the bar. Both come from the same texels
/// [`shell_parent_background_instances`] draws there.
pub(crate) fn push_family_list_backing(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    (screen_w, screen_h): (i32, i32),
    scale: crate::ui::shell::geom::ShellScale,
    window: RectPx,
    bar: Option<RectPx>,
) {
    let (plain, dark) = match scale.is_unscaled() && screen_w == 640 {
        true => (
            atlas.parent_background_640_mnscrns,
            atlas.parent_background_640_mnscrns_list,
        ),
        false => (
            atlas.parent_background_large_mnscrnl,
            atlas.parent_background_large_mnscrnl_list,
        ),
    };
    // In front of the background (the pass depth-tests), the bar over the
    // interior, both behind the panel art.
    let back = shell_paint::PARENT_BACKGROUND_DEPTH;
    let mut push =
        |entry: Option<crate::render::main_menu_shell_chrome::MainMenuShellChromeEntry>,
         rect: RectPx,
         depth: f32| {
            let Some(entry) = entry else {
                return;
            };
            if scale.is_unscaled() {
                let origin = crate::ui::shell::geom::dialog_origin(screen_w, screen_h);
                shell_paint::push_entry_crop(out, entry, origin, rect, depth);
            } else {
                let (w, h) = entry.native_size();
                let area = scale.scale_rect(RectPx::new(0, 0, w, h));
                shell_paint::push_entry_window_crop(out, entry, area, rect, depth);
            }
        };
    let interior = RectPx::new(window.x + 1, window.y + 1, window.w - 1, window.h - 1);
    push(dark, interior, back - 0.00001);
    if let Some(bar) = bar {
        let track = vera20k_gui::shell::list::scrollbar_interior(bar);
        push(plain, track, back - 0.00002);
    }
}

/// The RA2TS movie filling `rect`, once its session is loaded.
pub(crate) fn movie_draw(
    main_menu_movie: &Option<BinkMovieSurface>,
    rect: RectPx,
) -> Option<TexturedDraw<'_>> {
    let movie = main_menu_movie.as_ref()?;
    Some(TexturedDraw {
        texture: movie.batch_texture(),
        instances: vec![SpriteInstance {
            position: [rect.x as f32, rect.y as f32],
            size: [rect.w as f32, rect.h as f32],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: MOVIE_DEPTH,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        }],
    })
}

pub(crate) fn ensure_movie_for_current_layout(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    options_profile: &RetailOptionsProfile,
    requested_owner: Ra2tsDialogOwner,
) -> Result<()> {
    let layout = shell_layout::main_menu_layout(&renderer.gpu);
    let yr_original_style = options_profile.plays_yr_original_style();
    let requested_identity =
        Ra2tsMovieSessionIdentity::new(requested_owner, layout.movie_base, yr_original_style);
    if ra2ts_movie_session_is_reusable(
        frontend.main_menu_movie.is_some(),
        frontend.main_menu_movie_identity,
        requested_identity,
    ) {
        return Ok(());
    }
    clear_ra2ts_movie_session(frontend);

    let Some(assets) = process_assets.manager() else {
        frontend
            .main_menu_shell_error
            .get_or_insert_with(|| crate::i18n::tr("VERA:GameArchivesUnavailable"));
        return Ok(());
    };
    let load_movie = |bytes: &[u8], source: &str| {
        BinkMovieSurface::from_bytes(
            &renderer.gpu,
            &renderer.batch_renderer,
            Arc::<[u8]>::from(bytes),
            source.to_string(),
            true,
            wgpu::FilterMode::Nearest,
        )
    };
    let max_dimension = renderer.gpu.device.limits().max_texture_dimension_2d;
    let mod_movie = mod_menu_movie_candidates(assets, layout.movie_base, max_dimension).find_map(
        |(asset_name, bytes, source)| match load_movie(bytes, source) {
            Ok(movie) => Some((asset_name, movie)),
            Err(err) => {
                log::warn!("Skipping mod menu movie {asset_name} from {source}: {err:#}");
                None
            }
        },
    );
    let (asset_name, movie) = if let Some(movie) = mod_movie {
        movie
    } else {
        let asset_name = layout.movie_base.asset_name();
        let Some((bytes, source)) = menu_movie_bytes(assets, asset_name, yr_original_style) else {
            log::warn!("Missing main-menu movie asset {asset_name}");
            frontend.main_menu_shell_error = Some(crate::i18n::tr_fill(
                "VERA:MenuMovieMissing",
                &[("movie", &asset_name)],
            ));
            return Ok(());
        };
        match load_movie(bytes, source) {
            Ok(movie) => (asset_name, movie),
            Err(err) => {
                log::warn!("Failed to load main-menu movie {asset_name} from {source}: {err:#}");
                frontend.main_menu_shell_error = Some(crate::i18n::tr_fill(
                    "VERA:MenuMovieUnreadable",
                    &[
                        ("movie", &asset_name),
                        ("source", &source),
                        ("error", &format!("{err:#}")),
                    ],
                ));
                return Ok(());
            }
        }
    };
    log::info!(
        "Loaded {asset_name} for main menu from {}",
        movie.source_archive()
    );
    frontend.main_menu_movie = Some(movie);
    frontend.main_menu_movie_identity = Some(requested_identity);
    Ok(())
}

/// The active mod can supply art for either native movie slot. A launcher can
/// ship a short, wide status card under the same name, so look for a clip that
/// fits the menu's picture area. Within each slot, keep the mod's search order.
fn mod_menu_movie_candidates(
    assets: &AssetManager,
    preferred: MainMenuMovieBase,
    max_dimension: u32,
) -> impl Iterator<Item = (&'static str, &[u8], &str)> {
    let alternative = match preferred {
        MainMenuMovieBase::Ra2tsS => MainMenuMovieBase::Ra2tsL,
        MainMenuMovieBase::Ra2tsL => MainMenuMovieBase::Ra2tsS,
    };
    [preferred, alternative].into_iter().flat_map(move |base| {
        let name = base.asset_name();
        mod_art_candidates(assets, name)
            .filter(move |asset| {
                BinkHeader::probe(asset.bytes).is_ok_and(|header| {
                    header.width.max(header.height) <= max_dimension
                        && header.width <= header.height * 2
                        && header.height <= header.width * 2
                })
            })
            .map(move |asset| (name, asset.bytes, asset.source))
    })
}

/// Step the shared menu loop once for this paint.
pub(super) fn step_ra2ts_movie(
    platform: &PlatformState,
    gpu: &GpuContext,
    frontend: &mut FrontendState,
    shown: bool,
) -> bool {
    let playing = shown && platform.movie_activity(None).runs();
    let elapsed = frontend
        .main_menu_movie_clock
        .elapsed(Instant::now(), playing);
    let Some(movie) = frontend.main_menu_movie.as_mut() else {
        return true;
    };
    let Some(elapsed) = elapsed else {
        // The next interval starts at the frame that remained on screen.
        movie.resume_clock();
        return true;
    };
    if let Err(err) = movie.step(gpu, elapsed.as_secs_f64()) {
        log::warn!("Failed to step RA2TS menu movie: {err:#}");
        frontend.main_menu_shell_error = Some(crate::i18n::tr_fill(
            "VERA:MenuMovieFailed",
            &[("error", &format!("{err:#}"))],
        ));
        return false;
    }
    true
}

/// The menu movie's bytes and the archive they came from.
///
/// Native YR 1.11 gamemd.exe, GZF 2026-10-15, VA 0x00531cc0
/// (MainMenuDialog0xE2__Run), selects Ra2ts_s at width 640 and Ra2ts_l
/// otherwise, then gives that name to the movie control. Resolve through the
/// shared first-match asset lookup, preserving loose-file and archive priority.
/// VERA's Original YR Style option uses the ordinary first-match
/// lookup. The unchecked default requires the named member from
/// [`RA2_MOVIE_ARCHIVE`]; a missing RA2 copy is reported instead of silently
/// resolving the YR member with the same name.
fn menu_movie_bytes<'a>(
    assets: &'a AssetManager,
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

/// Paint 0xE2, sliding in, sliding out or steady.
pub(crate) fn render_main_menu_shell(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<MainMenuShellRenderResult> {
    let slide = match crate::app::frontend::shell_transition::shell_exit_wave(
        &frontend.shell_exit,
        crate::app::frontend::shell_transition::ShellSlideKind::MainMenu,
    ) {
        Some(wave) => MainMenuSlide::Exit(wave.clone()),
        None => frontend
            .shell_first_paint_slide
            .clone()
            .map_or(MainMenuSlide::Steady, MainMenuSlide::Entry),
    };
    // Either slide leaves the heading blank.
    let title_window = if matches!(slide, MainMenuSlide::Steady) {
        frontend.shell_page_title.paint(Instant::now())
    } else {
        None
    };
    Ok(
        if render_main_menu(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
            encoder,
            destination,
            title_window,
            slide,
        )? {
            MainMenuShellRenderResult::Rendered
        } else {
            MainMenuShellRenderResult::Fallback
        },
    )
}

/// 0xE2 on the family shell over its MNSCRN parent background and RA2TS
/// movie; false when its art is missing.
fn render_main_menu(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    title_window: Option<Kind1RevealWindow>,
    slide: MainMenuSlide,
) -> Result<bool> {
    // States 6 and 7 (Exit confirmation, then the quit) run after 0xE2 is
    // destroyed: the confirmation shows over the empty shell backdrop
    // (`0x0052FEC0`). Retail leaves the destroyed box's last image on screen
    // through the quit fade (nothing repaints after OK); here the backdrop
    // shows alone (evidence note residual).
    sync_shell_menu_settings(renderer, &mut frontend.shell_message_box);
    let cover = MainMenuCover::of(frontend);
    let backdrop = cover != MainMenuCover::Live;
    if !backdrop {
        ensure_movie_for_current_layout(
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
            Ra2tsDialogOwner::MainMenu0xE2,
        )?;
    }
    if frontend.main_menu_shell_error.is_some()
        || !super::menu_page_render::family_art_ready(
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
        )
    {
        frontend
            .main_menu_shell_error
            .get_or_insert_with(|| crate::i18n::tr("VERA:MenuArtworkUnavailable"));
        return Ok(false);
    }

    // While either slide runs the RA2TS static shows no movie and gets no
    // timer (`0x006071E0`; the teardown also stops it with 0x4E2), so the
    // parent background shows and the movie clock starts after the slide.
    let sliding = !matches!(slide, MainMenuSlide::Steady);
    let leaving = matches!(slide, MainMenuSlide::Exit(_));
    let movie_shown = !sliding && !backdrop;
    if !step_ra2ts_movie(platform, &renderer.gpu, frontend, movie_shown) {
        return Ok(false);
    }

    let layout = shell_layout::main_menu_layout(&renderer.gpu);
    let page = layout.page();
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // that covers the children, and the engine pumps no messages until it
    // ends: the heading, status and version statics stay blank and the
    // monitor window shows the right panel's own art underneath.
    let monitor = if backdrop || leaving {
        None
    } else {
        paint_shell_monitor(frontend)
    };
    let status = if leaving || cover == MainMenuCover::Backdrop {
        None
    } else {
        let key = match cover {
            MainMenuCover::Joining => Some("GUI:ConnectingToPlayers".to_owned()),
            _ => FamilyPage::MainMenu.status_key(&frontend.shell_controller),
        };
        let text = help_text(process_assets, key.as_deref());
        paint_shell_status_line(&mut frontend.shell_status_line, text, page.status_help)
    };
    let panel = PanelState {
        leaving,
        // The slide engine and the empty backdrop draw the whole tile column
        // in place of the buttons.
        column: if backdrop {
            Some(shell_paint::shuttered_column(page.right_panel))
        } else {
            slide.column_draws()
        },
        monitor,
        title: title_window.filter(|_| !backdrop),
        status,
    };
    let tree = FamilyPage::MainMenu.tree(&renderer.gpu, frontend);
    // An open box paints over the page next, with the cursor above it.
    let boxed = frontend.shell_message_box.is_some();
    let title = MAIN_MENU_PAGE.title_key;
    let mut frame = FamilyFrame::new("Main Menu Shell", &page, panel, title).on(
        &frontend.shell_controller,
        MAIN_MENU_PAGE.dialog,
        &tree,
    );
    frame.backdrop = true;
    frame
        .under
        .extend(movie_draw(&frontend.main_menu_movie, page.movie).filter(|_| movie_shown));
    if !backdrop && !leaving {
        frame.labels = version_labels(&frontend.mods, &layout);
    }
    frame.cursor = !boxed;
    encode_family_page(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        encoder,
        destination,
        frame,
    );
    if boxed {
        render_shell_message_box_over_page(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
            encoder,
            destination,
        );
    }
    Ok(true)
}

/// Back-to-front depths for the quit-confirm modal overlay. They sit in the clear
/// band between the menu's front-most text (`TEXT_DEPTH`) and the cursor
/// (`CURSOR_DEPTH`), so the modal blocks the menu while the cursor stays on top.
const EXIT_CONFIRM_MODAL_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: 0.00050,
    button: 0.00045,
    text: 0.00040,
};

/// Paint the open message box over a page that keeps showing: the page has
/// been composed on the shell surface, so the box and the cursor go on top
/// and the surface is presented again.
pub(crate) fn render_shell_message_box_over_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) {
    sync_shell_menu_settings(renderer, &mut frontend.shell_message_box);
    let parts = sync_shell_message_box(&renderer.gpu, frontend);
    // The box's lists keep their widgets on the box as it lays out now.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let _ = crate::app::updater::notes::sync(&renderer.gpu, frontend);
    #[cfg(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))]
    let _ = crate::app::shell_mod_import::sync(renderer, frontend);
    let (Some(overlay), Some(chrome)) = (
        parts.and_then(|parts| {
            build_shell_message_box_overlay(
                &platform.ime,
                renderer,
                frontend,
                match_state,
                process_assets,
                &parts,
            )
        }),
        frontend.skirmish_shell_chrome.as_ref(),
    ) else {
        return;
    };
    let draws = [crate::app::frontend::shell_pass::TexturedDraw {
        texture: &chrome.texture,
        instances: overlay.sprites,
    }];
    // Over the online client's pages the box matches the page. Read before
    // the cursor's sprite borrows the match state.
    let smooth_art = persistence.smooth_online_menus(frontend.native_shell_ui)
        && matches!(
            crate::app::input::precedence::presented(
                platform,
                renderer,
                frontend,
                match_state,
                persistence
            )
            .1,
            crate::app::input::precedence::ScreenPage::Wol
                | crate::app::input::precedence::ScreenPage::NetworkSetup
        );
    crate::app::frontend::shell_pass::encode_shell_pass_over_page(
        renderer,
        encoder,
        destination,
        "Shell message box over page",
        crate::app::frontend::shell_pass::ShellComposition {
            draws: &draws,
            text: &overlay.text,
            cursor: crate::app::frontend::shell_pass::top_software_cursor(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
                CURSOR_DEPTH,
            ),
            effects: None,
            smooth_art,
        },
    );
}

/// The box's own widgets on the shell controller follow it and its layout
/// before they paint ([`crate::ui::shell::modal::ShellBox::sync`]).
/// Its rects; none while no box is open.
fn sync_shell_message_box(
    gpu: &GpuContext,
    frontend: &mut FrontendState,
) -> Option<crate::ui::shell::modal::BoxParts> {
    let message_box = frontend.shell_message_box.as_ref()?;
    let parts = shell_layout::shell_box_parts(gpu, &message_box.shell_box);
    if let Some(message_box) = frontend.shell_message_box.as_ref() {
        let controller = &mut frontend.shell_controller;
        crate::ui::shell::menu_page::sync_box(controller, &message_box.shell_box, &parts);
    }
    Some(parts)
}

/// The Esc menu's settings tree follows its layout and values before it
/// paints.
fn sync_shell_menu_settings(
    renderer: &RendererState,
    shell_message_box: &mut Option<ShellMessageBox>,
) {
    if shell_message_box
        .as_ref()
        .and_then(crate::app::frontend::state::ShellMessageBox::menu_settings)
        .is_none()
    {
        return;
    }
    let controls = shell_layout::shell_menu_settings_layout(
        shell_layout::shell_screen_size(&renderer.gpu),
        &renderer.bit_font,
    );
    if let Some(settings) = shell_message_box
        .as_mut()
        .and_then(crate::app::frontend::state::ShellMessageBox::menu_settings_mut)
    {
        settings.shell_sync(&controls);
    }
}

/// Build the shell message box (0xCE/0x120/0x121) SHP modal overlay when it is open: the centered
/// PUDLGBGN panel + populated MNBTTN buttons and labels, sourced from
/// the skirmish chrome atlas (the only atlas that loads PUDLGBGN/MNBTTN). The
/// pressed button is read from the shared shell controller (its top dialog is the
/// modal while open). Returns `None` when the modal is closed or its art is absent.
fn build_shell_message_box_overlay(
    ime: &ImeState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    parts: &crate::ui::shell::modal::BoxParts,
) -> Option<shell_paint::ModalDraw> {
    use crate::app::frontend::state::ShellMessageBoxPurpose;
    let message_box = frontend.shell_message_box.as_ref()?;
    let atlas = frontend.skirmish_shell_chrome.as_ref()?;
    // VERA's own boxes pick their art: the Esc menu the Allied eagle, the
    // updater's Red Alert 2's. The others draw gamemd's.
    let background = match message_box.purpose {
        ShellMessageBoxPurpose::ShellMenu(_) => {
            atlas.mode_two_dialog_background(Some(crate::ui::sidebar::SidebarTheme::Allied))
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        ShellMessageBoxPurpose::UpdateOffer(_)
        | ShellMessageBoxPurpose::UpdateProgress
        | ShellMessageBoxPurpose::UpdateCheck => atlas.ra2_dialog_background,
        #[cfg(any(
            windows,
            target_os = "macos",
            target_os = "linux",
            target_os = "android"
        ))]
        ShellMessageBoxPurpose::ModImport(_) | ShellMessageBoxPurpose::ModAddChoice(_) => {
            atlas.ra2_dialog_background
        }
        ShellMessageBoxPurpose::Notice
        | ShellMessageBoxPurpose::ModRestart(_)
        | ShellMessageBoxPurpose::ModFiles(_) => atlas.ra2_dialog_background,
        _ => None,
    }
    .or_else(|| {
        // Original RA2 draws every shell message box (including the
        // "Are you sure you want to exit Red Alert 2?" confirmation) over
        // `PUDLGBGN.SHP` tinted with `DIALOG.PAL`, both in `ra2.mix ->
        // local.mix`. The stock slot above is gamemd's `DIALOGN.PAL` art.
        // Prefer RA2's own dialog art so -ra2 boxes don't look like YR.
        if crate::util::ra2_launch::enabled() {
            atlas.ra2_dialog_background
        } else {
            None
        }
    })
    .or_else(|| atlas.mode_two_dialog_background(None));
    let shell_box = &message_box.shell_box;
    let controller = &frontend.shell_controller;
    let pressed = controller
        .top_id()
        .and_then(|top| crate::ui::shell::menu_page::pressed_id(controller, top));
    let menu = message_box.menu_settings().map(|settings| {
        let controls = shell_layout::shell_menu_settings_layout(
            shell_layout::shell_screen_size(&renderer.gpu),
            &renderer.bit_font,
        );
        (settings, controls)
    });
    // The shell draws all text after its sprites, the open list's backing
    // too: what it covers gives way.
    let list: Vec<RectPx> = menu
        .as_ref()
        .and_then(|(settings, _)| settings.shell_popup_rect())
        .into_iter()
        .collect();
    let base_look = crate::app::frontend::skirmish_shell_render::BoxLook::new(
        background,
        EXIT_CONFIRM_MODAL_DEPTHS,
    );
    let look = crate::app::frontend::skirmish_shell_render::BoxLook {
        covered: &list,
        // Add Mod's greyed reorder buttons use the shell's disabled text. On
        // Android's screens that red is nearly the button's own colour, so
        // the labels stay readable in a grey.
        disabled_rgb: if message_box.is_mod_import() {
            #[cfg(not(target_os = "android"))]
            {
                shell_paint::SHELL_TEXT_RGB_DISABLED
            }
            #[cfg(target_os = "android")]
            {
                [0.62, 0.55, 0.5]
            }
        } else {
            base_look.disabled_rgb
        },
        ..base_look
    };
    let tree =
        crate::app::frontend::skirmish_shell_render::controller_tree(controller, shell_box.dialog);
    let mut draw = crate::app::frontend::skirmish_shell_render::paint_shell_box(
        ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
        Some(atlas),
        tree,
        shell_box,
        parts,
        |id| pressed == Some(id),
        look,
    );
    // The updater's offer lists what changed (`app::updater::notes`), and
    // Add Mod its parts, each from its list's widget: the lists lie on the
    // dialog's picture, which has no list backing, so their sprites go over
    // the box's and their text after its own.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let notes = crate::app::updater::notes::node(&renderer.gpu, frontend).map(|(node, notes)| {
        use crate::app::frontend::skirmish_shell_render::gui_paint::{GuiStyle, paint_gui_styled};
        use crate::app::updater::notes::LIST_ID;
        // The rows are the list's `Items`, written as it was built.
        let gui = frontend.shell_controller.gui();
        // A change's further rows hang under its text.
        let indent_of = |id: i32, row: usize| {
            (id == LIST_ID)
                .then(|| notes.row(row).map(|(_, indent)| indent))
                .flatten()
        };
        let style = GuiStyle {
            indent_of: Some(&indent_of),
            solid_lists: true,
            ..Default::default()
        };
        paint_gui_styled(
            ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            node,
            parts.scale,
            &[],
            style,
        )
    });
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let notes = notes.map(|mut notes| {
        let controls = notes.take_lane(crate::render::hud_paint::Material::Controls);
        draw.text.extend(notes.texts);
        controls
    });
    if let Some((settings, controls)) = menu {
        crate::app::frontend::skirmish_shell_render::paint_options_settings(
            &mut draw,
            ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            &controls,
            settings,
        );
    }
    // Add Mod's parts, each row's text inset 4 pixels from both ends and cut
    // to its row: the list's `Items`, which its sync writes as they change.
    #[cfg(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))]
    let import_parts = crate::app::shell_mod_import::node(&renderer.gpu, frontend).map(|node| {
        use crate::app::frontend::skirmish_shell_render::gui_paint::{GuiStyle, paint_gui_styled};
        let gui = frontend.shell_controller.gui();
        let style = GuiStyle {
            solid_lists: true,
            ..Default::default()
        };
        paint_gui_styled(
            ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            node,
            parts.scale,
            &[],
            style,
        )
    });
    #[cfg(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))]
    let import_parts = import_parts.map(|mut import_parts| {
        let controls = import_parts.take_lane(crate::render::hud_paint::Material::Controls);
        draw.text.extend(import_parts.texts);
        controls
    });
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    draw.sprites.extend(notes.into_iter().flatten());
    #[cfg(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))]
    draw.sprites.extend(import_parts.into_iter().flatten());
    Some(draw)
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::assets::asset_manager::MediaArchiveMode;
    use crate::assets::asset_manager::test_support::{TestDirectory, audio_test_install};

    fn bink_header(width: u32, height: u32) -> [u8; 44] {
        let mut header = [0; 44];
        header[..4].copy_from_slice(b"BIKi");
        for (offset, value) in [
            (4, 36),
            (8, 1),
            (20, width),
            (24, height),
            (28, 15),
            (32, 1),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        header
    }

    #[test]
    fn mod_menu_uses_full_art_behind_a_launcher_warning() {
        let install = audio_test_install("mod-menu-retail");
        let mod_dir = TestDirectory::new("mod-menu-branding");
        let large = MainMenuMovieBase::Ra2tsL;
        let name = large.asset_name();
        let stock = bink_header(632, 570);
        let branding = bink_header(632, 568);
        install.write_mix("expandmd98.mix", name, &stock);
        mod_dir.write_mix("expandmd99.mix", name, &bink_header(632, 60));
        mod_dir.write_mix("expandmd96.mix", name, &branding);
        let assets = AssetManager::with_mod(
            install.path(),
            Some(mod_dir.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();

        assert_eq!(
            assets.get_with_source_ref(name).unwrap().1,
            "expandmd99.mix"
        );
        let candidates: Vec<_> = mod_menu_movie_candidates(&assets, large, 2048).collect();
        assert_eq!(candidates, [(name, branding.as_slice(), "expandmd96.mix")]);
        // A mod with only the large clip also brands the compact shell.
        assert_eq!(
            mod_menu_movie_candidates(&assets, MainMenuMovieBase::Ra2tsS, 2048).next(),
            candidates.first().copied()
        );
    }

    #[test]
    fn mod_menu_prefers_loose_art_for_the_current_slot() {
        let install = audio_test_install("mod-menu-native-slot");
        let mod_dir = TestDirectory::new("mod-menu-loose");
        let small = MainMenuMovieBase::Ra2tsS;
        let large = MainMenuMovieBase::Ra2tsL;
        mod_dir.write_mix("expandmd99.mix", large.asset_name(), &bink_header(632, 570));
        mod_dir.write_mix("expandmd98.mix", small.asset_name(), &bink_header(472, 448));
        let loose = bink_header(472, 450);
        std::fs::write(mod_dir.path().join(small.asset_name()), loose).unwrap();
        let assets = AssetManager::with_mod(
            install.path(),
            Some(mod_dir.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();

        let (name, bytes, source) = mod_menu_movie_candidates(&assets, small, 2048)
            .next()
            .unwrap();
        assert_eq!((name, bytes), (small.asset_name(), loose.as_slice()));
        assert!(source.starts_with("loose:"));
    }

    #[test]
    fn menu_movie_resumes_at_normal_speed_after_backgrounding() {
        let mut clock = MenuMovieClock::default();
        let start = Instant::now();
        let frame = Duration::from_millis(67);
        assert_eq!(clock.elapsed(start, true), None);
        assert_eq!(clock.elapsed(start + frame, true), Some(frame));

        // Tab away and back without a single background redraw.
        clock.pause();
        let restored = start + Duration::from_secs(30);
        assert_eq!(clock.elapsed(restored, true), None);
        assert_eq!(clock.elapsed(restored + frame, true), Some(frame));

        // A still-visible inactive window can also receive background paints.
        assert_eq!(clock.elapsed(restored + 2 * frame, false), None);
        assert_eq!(clock.elapsed(restored + 3 * frame, false), None);
        let focused = restored + Duration::from_secs(10);
        assert_eq!(clock.elapsed(focused, true), None);
        assert_eq!(clock.elapsed(focused + frame, true), Some(frame));
    }

    #[test]
    fn external_launch_wait_covers_the_menu_controls() {
        // docs/spawn.md: a joining network launch takes no shell input, so it
        // must not paint the live buttons; it names what it waits for.
        assert_eq!(
            MainMenuCover::new(false, Some(ExternalMenuWait::Joining)),
            MainMenuCover::Joining
        );
        assert_eq!(
            MainMenuCover::new(false, Some(ExternalMenuWait::Reporting)),
            MainMenuCover::Backdrop
        );
        assert_eq!(MainMenuCover::new(false, None), MainMenuCover::Live);
    }

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
