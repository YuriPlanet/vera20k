//! Paint one Westwood Online visit: the welcome `0x10E` and the login `0x10F`
//! as family pages, the empty backdrop with the wait box `0xC4` or a box
//! `0xD0`/`0x126`, and the custom lobby `0x113`'s shell.
//!
//! A family page (paint mode 1, `0x00621FB1..0x00621FFE`) draws the right
//! panel, then its own background at the dialog origin (`0x0072E730`):
//! MultiplaySelection.shp for `0x10E` (`0x0072C7E0`), LoginScreen.shp for
//! `0x10F` (`0x0072C600`), both loaded only at 800 wide. `0x10E`'s children:
//! the family heading, status line and monitor, the owner-draw buttons, two
//! group boxes (`0x0061E700`), kind-0 text statics and kind-2 icon statics.

use anyhow::Result;

use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::shell_capture::PresentedShell;
use crate::app::frontend::menu_page_render::{PanelStatic, paint_panel_statics};
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::skirmish_shell_render::WolLobbyPaint;
use crate::app::frontend::skirmish_shell_render::gui_paint::{
    ButtonArt, FrameArt, GuiStyle, Picture, paint_gui_ids, paint_gui_into, paint_gui_styled,
};
use crate::app::frontend::skirmish_shell_render::{BoxLook, controller_tree, paint_shell_box};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_wol::WolPage;
use crate::app::shell_wol::WolVisit;
use crate::app::state::PlatformState;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_paint::{self, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, push_entry_scaled};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::menu_page::{MenuPageLayout, MenuPageSpec};
use crate::ui::shell::slide::PanelArt;
use crate::ui::shell::sound::SHELL_SOUND_PAGE;
use crate::ui::wol_shell::{WOL_LOBBY_TITLE_KEY, WOL_WELCOME_DIALOG};

const BOX_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: 0.00050,
    button: 0.00045,
    text: 0.00040,
};

/// Which shell the capture driver saw for the visit's current page.
pub(crate) fn presented_wol_page(wol: &Option<WolVisit>) -> PresentedShell {
    match wol.as_ref().map(|visit| &visit.page) {
        Some(WolPage::Login(_)) => PresentedShell::WolLogin,
        Some(WolPage::Lobby) => PresentedShell::WolLobby,
        Some(WolPage::Options(page)) if page.sound.is_some() => PresentedShell::WolSoundOptions,
        Some(WolPage::Options(_)) => PresentedShell::WolOptions,
        Some(WolPage::AutoLoginQuery) => PresentedShell::WolAutoLoginQuery,
        Some(WolPage::Locale(_)) => PresentedShell::WolLocale,
        Some(WolPage::MyInformation(_)) => PresentedShell::WolMyInformation,
        Some(WolPage::Buddy(_)) => PresentedShell::WolBuddyList,
        Some(WolPage::QuickMatch(_)) => PresentedShell::WolQuickMatch,
        Some(WolPage::AddBuddyQuery(_)) => PresentedShell::WolAddBuddyQuery,
        _ => PresentedShell::WolWelcome,
    }
}

/// `0xCF` or `0x11E` over whatever the page shows, and where it sits.
pub(crate) fn page_dialogs_paint(
    ime: &ImeState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
) -> Option<(RectPx, WolLobbyPaint)> {
    use crate::app::frontend::skirmish_shell_render::{
        paint_wol_page_dialog, paint_wol_page_received,
    };
    use crate::app::shell_wol::page;
    if let Some(dialog) = page::page_dialog(frontend.wol.as_ref()) {
        let layout = page::page_layout(renderer);
        return Some((
            layout.dialog,
            paint_wol_page_dialog(
                ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                dialog,
                &layout,
            ),
        ));
    }
    let received = page::page_received(frontend.wol.as_ref())?;
    let layout = page::received_layout(renderer);
    Some((
        layout.dialog,
        paint_wol_page_received(
            ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            received,
            &layout,
        ),
    ))
}

/// What a page paints beyond the family chrome every WOL page shares.
enum Body {
    /// `0x10E`'s group boxes, icons and statics.
    Welcome(crate::ui::wol_shell::WolWelcomeLayout),
    Login(crate::ui::wol_login::WolLoginLayout),
    Lobby(crate::ui::wol_lobby::WolLobbyLayout),
    /// `0x114`'s group box, under its widgets.
    MyInfo,
    /// `0x116`.
    Buddy(crate::ui::wol_buddy::WolBuddyLayout),
    /// `0x117`.
    QuickMatch(crate::ui::wol_quick_match::WolQuickMatchLayout),
    /// Widgets alone: WOL Options, its Sound Options, `0x112`, `0xFE` and
    /// `0x122`.
    Widgets,
}

/// The page being painted: its dialog, right panel, status line, the page
/// whose captions its buttons show, and its body.
struct Painted {
    dialog: DialogResourceId,
    page: MenuPageLayout,
    status_help: RectPx,
    spec: &'static MenuPageSpec,
    body: Body,
}

/// The shell's Sound Options over WOL Options.
const SOUND_DIALOG: DialogResourceId = SHELL_SOUND_PAGE.dialog;

/// The WOL pages' slides.
const SLIDES: [ShellSlideKind; 11] = [
    ShellSlideKind::WolWelcome,
    ShellSlideKind::WolLogin,
    ShellSlideKind::WolLobby,
    ShellSlideKind::WolOptions,
    ShellSlideKind::WolSoundOptions,
    ShellSlideKind::WolAutoLoginQuery,
    ShellSlideKind::WolLocale,
    ShellSlideKind::WolMyInformation,
    ShellSlideKind::WolBuddyList,
    ShellSlideKind::WolQuickMatch,
    ShellSlideKind::WolAddBuddyQuery,
];

impl Painted {
    fn kind(&self) -> ShellSlideKind {
        let kind = SLIDES
            .into_iter()
            .find(|kind| kind.dialog_id() == self.dialog);
        kind.expect("every painted WOL page slides")
    }

    /// The heading's key: `0x113` keeps its own; `0x122` has none.
    fn title_key(&self) -> Option<&'static str> {
        let key = match self.body {
            Body::Lobby(_) => WOL_LOBBY_TITLE_KEY,
            _ => self.spec.title_key,
        };
        Some(key).filter(|key| !key.is_empty())
    }
}

/// The visit's page with its art loaded and its widgets placed; `None` for
/// the empty backdrop (a dialog has closed, under a box or the wait box).
fn painted(
    frontend: &mut FrontendState,
    renderer: &RendererState,
    process_assets: &ProcessAssets,
    options_profile: &RetailOptionsProfile,
    audio: &AppAudioRuntime,
) -> Option<Painted> {
    use crate::app::shell_wol::{buddy, leaf, lobby, my_info, options, persona, quick_match};
    use crate::ui::{wol_buddy, wol_lobby, wol_login, wol_my_info, wol_options, wol_quick_match};
    // 0x11E opens over the empty backdrop (0x0052FEC0), except from 0x116's
    // Page, which runs it over the Buddy List.
    let received = crate::app::shell_wol::page::page_received(frontend.wol.as_ref());
    if received.is_some_and(|received| !received.modal) {
        return None;
    }
    let dialog = match &frontend.wol.as_ref()?.page {
        WolPage::Welcome => WOL_WELCOME_DIALOG,
        WolPage::Login(_) => wol_login::DIALOG,
        WolPage::Lobby => wol_lobby::DIALOG,
        WolPage::Options(page) if page.sound.is_some() => SOUND_DIALOG,
        WolPage::Options(_) => wol_options::DIALOG,
        WolPage::AutoLoginQuery | WolPage::Locale(_) | WolPage::AddBuddyQuery(_) => {
            persona::dialog(frontend.wol.as_ref())?
        }
        WolPage::MyInformation(_) => wol_my_info::DIALOG,
        WolPage::Buddy(_) => wol_buddy::DIALOG,
        WolPage::QuickMatch(_) => wol_quick_match::DIALOG,
        WolPage::Backdrop { .. } | WolPage::Message { .. } => return None,
    };
    leaf::open_art(frontend, process_assets, renderer, dialog);
    let painted = |page, status_help, spec, body| Painted {
        dialog,
        page,
        status_help,
        spec,
        body,
    };
    Some(match dialog {
        WOL_WELCOME_DIALOG => {
            crate::app::App::wol_welcome_sync(&mut frontend.shell_controller, renderer, &[]);
            let layout = crate::app::frontend::shell_layout::wol_welcome_layout(&renderer.gpu);
            let spec = layout.page_spec();
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::Welcome(layout),
            )
        }
        wol_login::DIALOG => {
            let layout = crate::app::shell_wol::login_layout(frontend.wol.as_ref(), &renderer.gpu);
            let spec = layout.page_spec();
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::Login(layout),
            )
        }
        wol_lobby::DIALOG => {
            // Hidden (created hidden, or slid out for a join) or covered by
            // the empty backdrop for a popup: only the backdrop shows.
            let lobby_ui = frontend.wol.as_ref().and_then(|visit| visit.lobby());
            if !lobby_ui.is_some_and(|lobby| lobby.shown && !lobby.covered) {
                return None;
            }
            let spec = lobby_ui.map_or(&wol_lobby::PAGE, |lobby| lobby.ui.page_spec());
            lobby::sync(frontend, process_assets, renderer);
            let layout = lobby::layout(frontend.wol.as_ref(), &renderer.gpu);
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::Lobby(layout),
            )
        }
        wol_options::DIALOG => {
            let spec = options::menu_spec(frontend.wol.as_ref());
            let layout = options::layout(frontend.wol.as_ref(), &renderer.gpu);
            options::sync_gui(
                frontend,
                process_assets,
                &renderer.gpu,
                options_profile,
                audio,
            );
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::Widgets,
            )
        }
        SOUND_DIALOG => {
            let layout = options::sound_layout(&renderer.gpu);
            options::sync_gui(
                frontend,
                process_assets,
                &renderer.gpu,
                options_profile,
                audio,
            );
            painted(
                layout.page.clone(),
                layout.page.status_help,
                &SHELL_SOUND_PAGE,
                Body::Widgets,
            )
        }
        wol_my_info::DIALOG => {
            my_info::sync(&mut frontend.shell_controller, renderer);
            let layout = my_info::layout(&renderer.gpu);
            let help = layout.page.status_help;
            painted(layout.page.clone(), help, &wol_my_info::PAGE, Body::MyInfo)
        }
        wol_buddy::DIALOG => {
            buddy::sync(frontend, renderer);
            let layout = buddy::layout(&renderer.gpu);
            painted(
                layout.page.clone(),
                layout.page.status_help,
                &wol_buddy::PAGE,
                Body::Buddy(layout),
            )
        }
        wol_quick_match::DIALOG => {
            quick_match::sync(frontend, renderer);
            let layout = quick_match::layout(&renderer.gpu);
            let spec = &wol_quick_match::PAGE;
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::QuickMatch(layout),
            )
        }
        _ => {
            persona::sync(frontend, renderer);
            let layout = persona::layout(frontend.wol.as_ref(), &renderer.gpu)?;
            let spec = crate::ui::wol_persona::WolPersonaLayout::spec(dialog);
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::Widgets,
            )
        }
    })
}

/// Paint the visit's page. Returns `false` when the shell chrome is missing.
pub(crate) fn render_wol_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    audio: &AppAudioRuntime,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if frontend.main_menu_shell_chrome.is_none() || frontend.wol.is_none() {
        return Ok(false);
    }
    crate::app::shell_network_setup::verify_sync(&renderer.gpu, frontend, match_state);
    let painted = painted(
        frontend,
        renderer,
        process_assets,
        &persistence.options_profile,
        audio,
    );
    let kind = painted.as_ref().map(Painted::kind);
    let page = match &painted {
        Some(painted) => painted.page.clone(),
        // The right panel's geometry is the family's on every WOL page.
        None => crate::app::frontend::shell_layout::wol_welcome_layout(&renderer.gpu).page,
    };
    let screen_w = page.screen.w;
    let screen_h = page.screen.h;
    let scale = page.scale;
    let visit = frontend.wol.as_ref().expect("checked");
    let message = visit.message().cloned();
    let wait = visit.wait_box().filter(|wait| wait.shown).cloned();
    // The box over the page, on its widgets before they paint.
    let open = message.or(wait.map(|wait| wait.shell_box));
    let box_parts = open.as_ref().map(|shell_box| {
        let parts = shell_box.parts(screen_w, screen_h, scale);
        let controller = &mut frontend.shell_controller;
        crate::ui::shell::menu_page::sync_box(controller, shell_box, &parts);
        parts
    });

    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: every child stays blank.
    let exit_wave = kind.and_then(|kind| {
        crate::app::frontend::shell_transition::shell_exit_wave(&frontend.shell_exit, kind).cloned()
    });
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| kind.and_then(|_| frontend.shell_first_paint_slide.clone()));
    let page_statics = kind.is_some() && !leaving;
    let body = painted.as_ref().map(|painted| &painted.body);
    // The chat lobby 0x113 has no warning monitor (template static 0x71C;
    // 0x60AB71..0x60AC9E lists the family dialogs carrying it) but has the
    // +0xD5 top panel instead; every other painted WOL page is the reverse,
    // as is the online lobby, which has no lobby list.
    let top_panel = matches!(body, Some(Body::Lobby(_)))
        && frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .is_none_or(|lobby| crate::ui::wol_lobby::keeps_top_panel(lobby.ui.page_spec()));
    let monitor_frame = if page_statics && !top_panel {
        crate::app::frontend::menu_page_render::paint_shell_monitor(frontend)
    } else {
        None
    };
    let (title_window, status_label) = if page_statics {
        let title_window = frontend
            .shell_page_title
            .paint(crate::util::time::Instant::now());
        use crate::app::shell_wol::{leaf, options, quick_match};
        let status_rect = painted
            .as_ref()
            .map_or(page.status_help, |painted| painted.status_help);
        let status_key = match painted.as_ref().map(|painted| painted.dialog) {
            // The lobby writes its own line.
            _ if matches!(body, Some(Body::Lobby(_))) => None,
            Some(crate::ui::wol_login::DIALOG) => crate::app::shell_wol::login_status_key(
                &frontend.shell_controller,
                &match_state.input,
            ),
            Some(crate::ui::wol_options::DIALOG) => options::status_key(frontend),
            Some(crate::ui::wol_quick_match::DIALOG) => {
                quick_match::status_key(frontend.wol.as_ref())
            }
            Some(SOUND_DIALOG) => None,
            // The control the last move hovered (none arrives while a button
            // holds the mouse).
            Some(dialog) => leaf::status_key(&frontend.shell_controller, dialog),
            None => None,
        };
        let status_text = match body {
            Some(Body::Lobby(_)) => crate::app::shell_wol::lobby::status_help(
                frontend,
                &match_state.input,
                process_assets,
            ),
            _ => status_key
                .map(|key| process_assets.csf_text(key))
                .unwrap_or_default(),
        };
        let status_label = crate::app::frontend::menu_page_render::paint_shell_status_line(
            &mut frontend.shell_status_line,
            status_text,
            status_rect,
        );
        (title_window, status_label)
    } else {
        (None, None)
    };

    let chrome = frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    // 0xFE and 0x122 load no art of their own.
    let art = painted
        .as_ref()
        .and_then(|painted| crate::app::shell_wol::leaf::art(&frontend.wol_art, painted.dialog));
    let mut sprites = Vec::new();
    // The dialog's own background exists only at 800 wide; elsewhere, and
    // behind the boxes, the empty shell backdrop shows (`0x0052FEC0`).
    let mut background_sprites = Vec::new();
    match art.and_then(|art| art.background) {
        // The background canvas is the authored 800x600 dialog; a scaled
        // page scales it with the dialog, as the parent backdrop does.
        Some(entry) => push_entry_scaled(
            &mut background_sprites,
            entry,
            0,
            0,
            scale,
            PARENT_BACKGROUND_DEPTH,
        ),
        None => sprites.extend(
            crate::app::frontend::main_menu_shell_render::shell_parent_background_instances(
                chrome, screen_w, screen_h, scale,
            ),
        ),
    }
    sprites.extend(shell_paint::paint_chrome(
        chrome,
        page.right_panel,
        Some(page.lower_strip),
        screen_w,
    ));
    if top_panel {
        let art = match &wave {
            Some(wave) => wave.panel_art(),
            None => vec![PanelArt::Sdtp(1)],
        };
        sprites.extend(shell_paint::paint_top_panel(chrome, page.right_panel, &art));
    }
    sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, page.warning_monitor, frame, scale)
    }));

    // Children of the page: blank while the teardown slide runs and gone
    // once the page closed; the entry slide leaves the left side painted.
    let children = page_statics;
    let mut icon_sprites = Vec::new();
    let mut welcome_texts = None;
    // `BS_GROUPBOX` frames (`0x0061E700`) from the page's widgets: the
    // list's two rings, the top edge eight pixels below the window top, on
    // the window's last column and row.
    let welcome_boxes: &'static [u16] = &crate::ui::wol_shell::GROUP_BOXES;
    let my_info_box = std::slice::from_ref(&crate::ui::wol_my_info::GROUP_BOX);
    let buddy_box = std::slice::from_ref(&crate::ui::wol_buddy::GROUP_BOX);
    let frames = match body.filter(|_| children) {
        Some(Body::Welcome(_)) => Some((WOL_WELCOME_DIALOG, welcome_boxes)),
        Some(Body::MyInfo) => Some((crate::ui::wol_my_info::DIALOG, my_info_box)),
        Some(Body::Buddy(_)) => Some((crate::ui::wol_buddy::DIALOG, buddy_box)),
        _ => None,
    };
    let controller = &frontend.shell_controller;
    let group_boxes =
        match frames.and_then(|(dialog, ids)| Some((controller.root_in(dialog)?, ids))) {
            Some((root, ids)) => {
                let inset = RectPx::new(1, 9, 2, 2);
                let frame_of = |id: i32| {
                    ids.iter()
                        .any(|frame| i32::from(*frame) == id)
                        .then_some(FrameArt::ListRings { inset })
                };
                let style = GuiStyle {
                    frame_of: Some(&frame_of),
                    ..Default::default()
                };
                let gui = controller.gui();
                let mut paint = paint_gui_ids(
                    &platform.ime,
                    &renderer.bit_font,
                    frontend,
                    match_state,
                    process_assets,
                    gui,
                    root,
                    ids.iter().copied(),
                    scale,
                    style,
                );
                paint.take_lane(Material::Controls)
            }
            None => Vec::new(),
        };
    // The glossary's icons and the page's texts from its widgets.
    if let (Some(Body::Welcome(_)), Some(root)) = (
        body.filter(|_| children),
        controller.root_in(WOL_WELCOME_DIALOG),
    ) {
        let picture_of = |id: i32| {
            let icon = crate::ui::wol_shell::WOL_ICONS.iter();
            let mut icon = icon.filter(|icon| i32::from(icon.id) == id);
            icon.next().map(|icon| Picture::Glyph(icon.file))
        };
        let style = GuiStyle {
            art,
            picture_of: Some(&picture_of),
            ..Default::default()
        };
        let mut welcome = paint_gui_styled(
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            controller.gui(),
            root,
            scale,
            &[],
            style,
        );
        icon_sprites = welcome.take_lane(Material::Icons);
        welcome_texts = Some(welcome.texts);
    }
    let shown = body.filter(|_| children);
    let buddy_page = crate::app::shell_wol::buddy::page(frontend.wol.as_ref());
    let buddy_children = match (shown, buddy_page) {
        (Some(Body::Buddy(layout)), Some(dialog)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_buddy_children(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                &dialog.ui,
                layout,
                art,
            ),
        ),
        _ => None,
    };
    let add_buddy = buddy_page
        .and_then(|dialog| dialog.ui.add.as_ref())
        .map(|_| {
            crate::app::frontend::skirmish_shell_render::paint_wol_add_buddy(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                &crate::ui::shell::modal::wol_add_buddy_layout(screen_w, screen_h),
            )
        });
    // 0x115 sits over 0x116 without covering it.
    let buddy_popup = match body {
        Some(Body::Buddy(_)) if add_buddy.is_some() => {
            Some(crate::ui::shell::modal::wol_add_buddy_layout(screen_w, screen_h).dialog)
        }
        _ => None,
    };
    // The pages drawn by their widgets alone.
    let widget_children = match (&painted, shown) {
        (Some(painted), Some(Body::MyInfo | Body::Widgets)) => {
            Some(crate::app::shell_wol::leaf::paint(
                frontend,
                &platform.ime,
                match_state,
                process_assets,
                &renderer.bit_font,
                painted.dialog,
                painted.page.scale,
            ))
        }
        _ => None,
    };
    let login = frontend.wol.as_ref().and_then(|visit| visit.login());
    let login_children = match (shown, login) {
        (Some(Body::Login(layout)), Some(login)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_login_children(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                login,
                layout,
            ),
        ),
        _ => None,
    };
    // VERA online's account popup over the login page.
    let account_form = match (shown, login.and_then(|login| login.account.as_ref())) {
        (Some(Body::Login(layout)), Some(form)) => {
            crate::app::shell_wol::account_form_layout(frontend.wol.as_ref(), &renderer.gpu).map(
                |rects| {
                    (
                        rects.dialog,
                        crate::app::frontend::skirmish_shell_render::paint_wol_account_form(
                            &platform.ime,
                            &renderer.bit_font,
                            frontend,
                            match_state,
                            process_assets,
                            form,
                            &rects,
                            layout.page.scale,
                        ),
                    )
                },
            )
        }
        _ => None,
    };
    let lobby = frontend.wol.as_ref().and_then(|visit| visit.lobby());
    let lobby_children = match (shown, lobby) {
        (Some(Body::Lobby(layout)), Some(lobby)) => {
            let base_names =
                crate::app::shell_wol::lobby::base_names(frontend.wol.as_ref(), process_assets);
            Some(
                crate::app::frontend::skirmish_shell_render::paint_wol_lobby_children(
                    &platform.ime,
                    &renderer.bit_font,
                    frontend,
                    match_state,
                    process_assets,
                    &lobby.ui,
                    layout,
                    art,
                    &base_names,
                ),
            )
        }
        _ => None,
    };
    // VERA's player card over the modern lobby.
    let player_card = match shown {
        Some(Body::Lobby(layout)) => {
            crate::app::shell_wol::player_card::view(frontend, process_assets, &renderer.gpu).map(
                |view| {
                    let scale = layout.page.scale;
                    (
                        view.layout.dialog,
                        crate::app::frontend::skirmish_shell_render::paint_wol_player_card(
                            &platform.ime,
                            &renderer.bit_font,
                            frontend,
                            match_state,
                            process_assets,
                            &view,
                            scale,
                        ),
                        view.countries.as_ref().map(|list| {
                            (
                        list.popup.rect,
                        crate::app::frontend::skirmish_shell_render::paint_wol_card_countries(
                            &platform.ime,
                            &renderer.bit_font,
                            frontend,
                            match_state,
                            process_assets,
                            scale,
                        ),
                    )
                        }),
                    )
                },
            )
        }
        _ => None,
    };
    let lobby_popup = lobby.and_then(|lobby| {
        lobby.ui.popup.as_ref()?;
        let status = if page_statics || painted.is_none() {
            crate::app::shell_wol::lobby::status_help(frontend, &match_state.input, process_assets)
        } else {
            String::new()
        };
        Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_lobby_popup(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                &lobby.ui,
                screen_w,
                screen_h,
                crate::app::shell_wol::lobby::popup_scale(frontend.wol.as_ref(), &renderer.gpu),
                &status,
            ),
        )
    });

    let quick_match_page = crate::app::shell_wol::quick_match::dialog(frontend.wol.as_ref());
    let quick_match_children = match (shown, quick_match_page) {
        (Some(Body::QuickMatch(layout)), Some(dialog)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_quick_match_children(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                &dialog.ui,
                layout,
            ),
        ),
        _ => None,
    };
    // Start Game Now's 0x130 over Quick Match's backdrop (0x117 closed at
    // OnGameStart; no C2/C9 exists).
    let verify = crate::app::shell_network_setup::verify(frontend.network_setup_protocol.as_ref())
        .filter(|_| {
            crate::app::shell_network_setup::quick_match(frontend.network_setup_protocol.as_ref())
        })
        .map(|verify| {
            crate::app::frontend::skirmish_shell_render::paint_wol_verify(
                &platform.ime,
                renderer,
                frontend,
                match_state,
                process_assets,
                verify,
                &crate::app::shell_network_setup::verify_layout(
                    &renderer.gpu,
                    frontend,
                    match_state,
                ),
            )
        });

    let page_dialogs = page_dialogs_paint(
        &platform.ime,
        renderer,
        frontend,
        match_state,
        process_assets,
    );

    // The column: the slide engine draws it while a slide runs, the empty
    // backdrop draws it shut, otherwise the page's buttons paint from their
    // nodes, the lobby's in its own tree and every other page's in the
    // shell's (`0x10E` paints the one of each row its press shows).
    let column = match (&painted, &wave) {
        (_, Some(wave)) => Some(wave.button_draws()),
        (None, None) => Some(shell_paint::shuttered_column(page.right_panel)),
        (Some(_), None) => None,
    };
    let mut buttons = HudPaint::default();
    match (&painted, column) {
        (_, Some(draws)) => sprites.extend(shell_paint::paint_slide_column(
            chrome,
            page.right_panel,
            &draws,
        )),
        (Some(painted), None) => {
            let controller = &frontend.shell_controller;
            let ids: Vec<u16> = match &painted.body {
                Body::Welcome(layout) => {
                    let active = controller.top_id() == Some(painted.dialog);
                    let pressed = active
                        .then(|| crate::app::App::wol_held(&frontend.shell_controller))
                        .flatten();
                    let shown = layout.painted_buttons(pressed);
                    shown.iter().map(|button| button.id).collect()
                }
                _ => page.buttons.iter().map(|button| button.id).collect(),
            };
            let lobby_ui = lobby
                .filter(|_| matches!(painted.body, Body::Lobby(_)))
                .map(|lobby| &lobby.ui);
            let text_of = |id: i32| {
                let id = u16::try_from(id).ok()?;
                // VERA: the online page's view button names the view it shows next.
                if let Some(ui) = lobby_ui.filter(|_| id == crate::ui::wol_lobby::VIEW) {
                    return Some(ui.view_caption());
                }
                let button = painted.spec.button(id)?;
                Some(process_assets.csf_text(button.csf_key))
            };
            let enabled_of = |id: i32| Some(lobby_ui?.enabled(u16::try_from(id).ok()?));
            let style = GuiStyle {
                buttons: ButtonArt::Panel,
                text_of: Some(&text_of),
                enabled_of: Some(&enabled_of),
                ..Default::default()
            };
            for id in ids {
                let node = match lobby_ui {
                    Some(ui) => ui.node(id).map(|node| (ui.gui(), node)),
                    None => (controller.control(painted.dialog, id))
                        .map(|node| (controller.gui(), node)),
                };
                let atlas = frontend.skirmish_shell_chrome.as_ref();
                if let (Some((gui, node)), Some(atlas)) = (node, atlas) {
                    let atlas = Some(atlas);
                    paint_gui_into(
                        &mut buttons,
                        &platform.ime,
                        &renderer.bit_font,
                        frontend,
                        match_state,
                        process_assets,
                        atlas,
                        gui,
                        node,
                        scale,
                        &[],
                        style,
                    );
                }
            }
        }
        (None, None) => {}
    }
    let title_key = painted.as_ref().and_then(Painted::title_key);
    let title = title_window.zip(title_key).map(|(window, key)| {
        let text = process_assets.csf_text(key);
        PanelStatic::title(text, page.title, ShellAlign::PANEL_TITLE, window)
    });
    let status = status_label.map(PanelStatic::status);
    let mut text = paint_panel_statics(
        &platform.ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
        [title, status],
        scale,
    );
    text.extend(welcome_texts.into_iter().flatten());

    // The boxes over the backdrop: PUDLGBGN with MNBTTN buttons.
    let mut box_sprites = Vec::new();
    let mut box_dialog = None;
    if let (Some(shell_box), Some(parts), Some(atlas)) = (
        open.as_ref(),
        box_parts.as_ref(),
        frontend.skirmish_shell_chrome.as_ref(),
    ) {
        box_dialog = Some(parts.dialog);
        let held = crate::app::App::wol_held(&frontend.shell_controller);
        let look = BoxLook::new(atlas.mode_two_dialog_background(None), BOX_DEPTHS);
        let tree = controller_tree(&frontend.shell_controller, shell_box.dialog);
        let draw = paint_shell_box(
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            Some(atlas),
            tree,
            shell_box,
            parts,
            |id| held == Some(id),
            look,
        );
        box_sprites = draw.sprites;
        text.extend(draw.text);
    }

    // Draw order is paint order: the dialog background, the backdrop and
    // chrome, the icons, the buttons, the login's controls, then the boxes.
    let mut draws = Vec::new();
    if let Some(art) = art {
        draws.push(TexturedDraw {
            texture: &art.texture,
            instances: background_sprites,
        });
    }
    draws.push(TexturedDraw {
        texture: &chrome.texture,
        instances: sprites,
    });
    if let Some(atlas) = frontend.skirmish_shell_chrome.as_ref() {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: group_boxes,
        });
    }
    if let Some(art) = art {
        draws.push(TexturedDraw {
            texture: &art.texture,
            instances: icon_sprites,
        });
    }
    if let Some(atlas) = frontend.skirmish_shell_chrome.as_ref() {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: buttons.take_lane(Material::Controls),
        });
    }
    text.extend(buttons.texts);
    // The widget layers, in paint order, each on the controls' atlas with
    // its icons on the page art: a dialog over the page hides the glyphs
    // painted before it under it (`cover`), and the boxes hide a layer's
    // own (`own`).
    let gui = |mut paint: HudPaint| {
        let controls = paint.take_lanes(&[Material::Controls, Material::Popup]);
        let mut texts = paint.texts;
        texts.extend(paint.popup_texts);
        WolLobbyPaint {
            controls,
            icons: Vec::new(),
            texts,
        }
    };
    let translate = crate::app::shell_wol::options::translate_popup(frontend).filter(|_| {
        painted.as_ref().map(|painted| painted.dialog) == Some(crate::ui::wol_options::DIALOG)
    });
    let widget_children = widget_children.map(|mut widgets| {
        if let Some(list) = translate {
            hide_glyphs_under(&mut widgets.texts, list.rect);
        }
        gui(widgets)
    });
    let (card, countries) = match player_card {
        Some((dialog, card, countries)) => (Some((dialog, card)), countries),
        None => (None, None),
    };
    let cover = |layer: Option<(RectPx, WolLobbyPaint)>| match layer {
        Some((dialog, paint)) => (Some(dialog), None, Some(paint)),
        None => (None, None, None),
    };
    let layers = [
        (None, None, login_children.map(gui)),
        cover(account_form),
        (None, None, widget_children),
        (None, box_dialog, lobby_children),
        (None, box_dialog, quick_match_children),
        (None, None, verify),
        (None, None, buddy_children),
        (buddy_popup, None, add_buddy),
        (None, None, lobby_popup),
        cover(card),
        cover(countries),
        cover(page_dialogs),
    ];
    let controls_atlas = frontend.skirmish_shell_chrome.as_ref();
    for (cover, own, paint) in layers {
        let (Some(atlas), Some(mut paint)) = (controls_atlas, paint) else {
            continue;
        };
        if let Some(dialog) = cover {
            hide_glyphs_under(&mut text, dialog);
        }
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: paint.controls,
        });
        if let Some(art) = art {
            draws.push(TexturedDraw {
                texture: &art.texture,
                instances: paint.icons,
            });
        }
        if let Some(dialog) = own {
            hide_glyphs_under(&mut paint.texts, dialog);
        }
        text.extend(paint.texts);
    }
    if let (Some(atlas), Some(_)) = (frontend.skirmish_shell_chrome.as_ref(), box_dialog) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: box_sprites,
        });
    }
    encode_shell_pass(
        renderer,
        encoder,
        destination,
        "Westwood Online",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
                CURSOR_DEPTH,
            ),
            effects: None,
            smooth_art: persistence.smooth_online_menus(frontend.native_shell_ui),
        },
    );
    platform.window.request_redraw();
    Ok(true)
}

/// Text draws after every sprite: a dialog painted over a page hides the
/// page's glyphs under it here.
fn hide_glyphs_under(
    text: &mut [crate::render::shell_text::ShellTextDraw],
    dialog: crate::ui::shell::geom::RectPx,
) {
    for draw in text {
        draw.instances.retain(|glyph| {
            let [x, y] = glyph.position;
            let [w, h] = glyph.size;
            x + w <= dialog.x as f32
                || y + h <= dialog.y as f32
                || x >= (dialog.x + dialog.w) as f32
                || y >= (dialog.y + dialog.h) as f32
        });
    }
}
