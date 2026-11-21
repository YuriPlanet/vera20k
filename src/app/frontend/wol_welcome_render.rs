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

use crate::app::AppState;
use crate::app::diagnostics::shell_capture::PresentedShell;
use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::app::frontend::movies_credits_render::push_group_box;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect,
    software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::skirmish_shell_render::WolLobbyPaint;
use crate::app::frontend::skirmish_shell_render::{BoxLook, paint_shell_box};
use crate::app::shell_wol::WolPage;
use crate::render::shell_paint::{
    self, CHROME_DEPTH, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_ENABLED, push_entry_scaled,
};
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, centred_in_window};
use crate::ui::shell::menu_page::{MenuPageButtonRect, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::slide::PanelArt;
use crate::ui::shell::sound::SHELL_SOUND_PAGE;
use crate::ui::wol_shell::{WOL_LOBBY_TITLE_KEY, WOL_WELCOME_DIALOG};

/// Icon and group-box children sit over the background, under the text.
const CHILD_DEPTH: f32 = CHROME_DEPTH - 0.00002;

const BOX_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: 0.00050,
    button: 0.00045,
    text: 0.00040,
};

/// Which shell the capture driver saw for the visit's current page.
pub(crate) fn presented_wol_page(state: &AppState) -> PresentedShell {
    match state.frontend.wol.as_ref().map(|visit| &visit.page) {
        Some(WolPage::Login(_)) => PresentedShell::WolLogin,
        Some(WolPage::Lobby) => PresentedShell::WolLobby,
        Some(WolPage::Options(page)) if page.sound.is_some() => PresentedShell::WolSoundOptions,
        Some(WolPage::Options(_)) => PresentedShell::WolOptions,
        Some(WolPage::AutoLoginQuery(_)) => PresentedShell::WolAutoLoginQuery,
        Some(WolPage::Locale(_)) => PresentedShell::WolLocale,
        Some(WolPage::MyInformation(_)) => PresentedShell::WolMyInformation,
        Some(WolPage::Buddy(_)) => PresentedShell::WolBuddyList,
        Some(WolPage::QuickMatch(_)) => PresentedShell::WolQuickMatch,
        Some(WolPage::AddBuddyQuery(_)) => PresentedShell::WolAddBuddyQuery,
        _ => PresentedShell::WolWelcome,
    }
}

/// `0xCF` or `0x11E` over whatever the page shows, and where it sits.
pub(crate) fn page_dialogs_paint(state: &AppState) -> Option<(RectPx, WolLobbyPaint)> {
    use crate::app::frontend::skirmish_shell_render::{
        paint_wol_page_dialog, paint_wol_page_received,
    };
    use crate::app::shell_wol::page;
    if let Some(dialog) = page::page_dialog(state) {
        let layout = page::page_layout(state);
        return Some((layout.dialog, paint_wol_page_dialog(state, dialog, &layout)));
    }
    let received = page::page_received(state)?;
    let layout = page::received_layout(state);
    Some((
        layout.dialog,
        paint_wol_page_received(state, received, &layout),
    ))
}

/// What a page paints beyond the family chrome every WOL page shares.
enum Body {
    /// `0x10E`'s group boxes, icons and statics.
    Welcome(crate::ui::wol_shell::WolWelcomeLayout),
    Login(crate::ui::wol_login::WolLoginLayout),
    Lobby(crate::ui::wol_lobby::WolLobbyLayout),
    /// `0x114`'s group box, under its widgets.
    MyInfo(RectPx),
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
    dialog: DialogId,
    page: MenuPageLayout,
    status_help: RectPx,
    spec: &'static MenuPageSpec,
    body: Body,
}

/// The shell's Sound Options over WOL Options.
const SOUND_DIALOG: DialogId = SHELL_SOUND_PAGE.dialog;

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
fn painted(state: &mut AppState) -> Option<Painted> {
    use crate::app::shell_wol::{buddy, leaf, lobby, my_info, options, persona, quick_match};
    use crate::ui::{wol_buddy, wol_lobby, wol_login, wol_my_info, wol_options, wol_quick_match};
    // 0x11E opens over the empty backdrop (0x0052FEC0), except from 0x116's
    // Page, which runs it over the Buddy List.
    let received = crate::app::shell_wol::page::page_received(state);
    if received.is_some_and(|received| !received.modal) {
        return None;
    }
    let dialog = match &state.frontend.wol.as_ref()?.page {
        WolPage::Welcome => WOL_WELCOME_DIALOG,
        WolPage::Login(_) => wol_login::DIALOG,
        WolPage::Lobby => wol_lobby::DIALOG,
        WolPage::Options(page) if page.sound.is_some() => SOUND_DIALOG,
        WolPage::Options(_) => wol_options::DIALOG,
        WolPage::AutoLoginQuery(_) | WolPage::Locale(_) | WolPage::AddBuddyQuery(_) => {
            persona::dialog(state)?
        }
        WolPage::MyInformation(_) => wol_my_info::DIALOG,
        WolPage::Buddy(_) => wol_buddy::DIALOG,
        WolPage::QuickMatch(_) => wol_quick_match::DIALOG,
        WolPage::Backdrop { .. } | WolPage::Message { .. } => return None,
    };
    leaf::open_art(state, dialog);
    let painted = |page, status_help, spec, body| Painted {
        dialog,
        page,
        status_help,
        spec,
        body,
    };
    Some(match dialog {
        WOL_WELCOME_DIALOG => {
            crate::app::App::wol_welcome_sync(state, &[]);
            let layout = crate::app::frontend::shell_layout::wol_welcome_layout(state);
            let spec = layout.page_spec();
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::Welcome(layout),
            )
        }
        wol_login::DIALOG => {
            let layout = crate::app::shell_wol::login_layout(state);
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
            let lobby_ui = state.frontend.wol.as_ref().and_then(|visit| visit.lobby());
            if !lobby_ui.is_some_and(|lobby| lobby.shown && !lobby.covered) {
                return None;
            }
            let spec = lobby_ui.map_or(&wol_lobby::PAGE, |lobby| lobby.ui.page_spec());
            let layout = lobby::layout(state);
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::Lobby(layout),
            )
        }
        wol_options::DIALOG => {
            let layout = options::layout(state);
            options::sync_gui(state);
            painted(
                layout.page.clone(),
                layout.page.status_help,
                &wol_options::PAGE,
                Body::Widgets,
            )
        }
        SOUND_DIALOG => {
            let layout = options::sound_layout(state);
            options::sync_gui(state);
            painted(
                layout.page.clone(),
                layout.page.status_help,
                &SHELL_SOUND_PAGE,
                Body::Widgets,
            )
        }
        wol_my_info::DIALOG => {
            my_info::sync(state);
            let layout = my_info::layout(state);
            let group_box = layout.rect_of(wol_my_info::GROUP_BOX).unwrap_or_default();
            let help = layout.page.status_help;
            painted(
                layout.page.clone(),
                help,
                &wol_my_info::PAGE,
                Body::MyInfo(group_box),
            )
        }
        wol_buddy::DIALOG => {
            buddy::sync(state);
            let layout = buddy::layout(state);
            painted(
                layout.page.clone(),
                layout.page.status_help,
                &wol_buddy::PAGE,
                Body::Buddy(layout),
            )
        }
        wol_quick_match::DIALOG => {
            quick_match::sync(state);
            let layout = quick_match::layout(state);
            let spec = &wol_quick_match::PAGE;
            painted(
                layout.page.clone(),
                layout.page.status_help,
                spec,
                Body::QuickMatch(layout),
            )
        }
        _ => {
            persona::sync(state);
            let layout = persona::layout(state)?;
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
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.main_menu_shell_chrome.is_none() || state.frontend.wol.is_none() {
        return Ok(false);
    }
    let painted = painted(state);
    let kind = painted.as_ref().map(Painted::kind);
    let page = match &painted {
        Some(painted) => painted.page.clone(),
        // The right panel's geometry is the family's on every WOL page.
        None => crate::app::frontend::shell_layout::wol_welcome_layout(state).page,
    };
    let screen_w = page.screen.w;
    let screen_h = page.screen.h;
    let scale = page.scale;
    let visit = state.frontend.wol.as_ref().expect("checked");
    let message = visit.message().cloned();
    let wait = visit.wait_box().filter(|wait| wait.shown).cloned();

    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: every child stays blank.
    let exit_wave = kind.and_then(|kind| {
        crate::app::frontend::shell_transition::shell_exit_wave(state, kind).cloned()
    });
    let leaving = exit_wave.is_some();
    let wave =
        exit_wave.or_else(|| kind.and_then(|_| state.frontend.shell_first_paint_slide.clone()));
    let page_statics = kind.is_some() && !leaving;
    let body = painted.as_ref().map(|painted| &painted.body);
    // The chat lobby 0x113 has no warning monitor (template static 0x71C;
    // 0x60AB71..0x60AC9E lists the family dialogs carrying it) but has the
    // +0xD5 top panel instead; every other painted WOL page is the reverse,
    // as is the online lobby, which has no lobby list.
    let top_panel = matches!(body, Some(Body::Lobby(_)))
        && state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .is_none_or(|lobby| crate::ui::wol_lobby::keeps_top_panel(lobby.ui.page_spec()));
    let monitor_frame = if page_statics && !top_panel {
        crate::app::frontend::menu_page_render::paint_shell_monitor(state)
    } else {
        None
    };
    let (title_window, status_label) = if page_statics {
        let title_window = state
            .frontend
            .shell_page_title
            .paint(crate::util::time::Instant::now());
        use crate::app::shell_wol::{leaf, options, quick_match};
        let status_rect = painted
            .as_ref()
            .map_or(page.status_help, |painted| painted.status_help);
        let status_key = match painted.as_ref().map(|painted| painted.dialog) {
            // The lobby writes its own line.
            _ if matches!(body, Some(Body::Lobby(_))) => None,
            Some(crate::ui::wol_login::DIALOG) => crate::app::shell_wol::login_status_key(state),
            Some(crate::ui::wol_options::DIALOG) => options::status_key(state),
            Some(crate::ui::wol_quick_match::DIALOG) => quick_match::status_key(state),
            Some(SOUND_DIALOG) => None,
            // The control the last move hovered (none arrives while a button
            // holds the mouse).
            Some(dialog) => leaf::status_key(state, dialog),
            None => None,
        };
        let status_text = match body {
            Some(Body::Lobby(_)) => crate::app::shell_wol::lobby::status_help(state),
            _ => status_key
                .map(|key| state.csf_text(key))
                .unwrap_or_default(),
        };
        let status_label = crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            status_rect,
        );
        (title_window, status_label)
    } else {
        (None, None)
    };

    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    // 0xFE and 0x122 load no art of their own.
    let art = painted
        .as_ref()
        .and_then(|painted| crate::app::shell_wol::leaf::art(state, painted.dialog));
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
    let mut labels = Vec::new();
    let mut welcome_texts = None;
    match body.filter(|_| children) {
        Some(Body::Welcome(layout)) => {
            for window in layout.group_boxes() {
                push_group_box(&mut sprites, chrome, window, scale);
            }
            let icons = art.iter().flat_map(|art| &art.icons);
            for (file, window) in layout.icons() {
                let Some((_, entry)) = icons.clone().find(|(name, _)| name == file) else {
                    continue;
                };
                let (w, h) = entry.native_size();
                let (x, y) = centred_in_window(scale.base_rect(window), w, h);
                push_entry_scaled(&mut icon_sprites, *entry, x, y, scale, CHILD_DEPTH);
            }
            welcome_texts =
                Some(crate::app::shell_wol::leaf::paint(state, WOL_WELCOME_DIALOG, scale).texts);
        }
        Some(Body::MyInfo(group_box)) => push_group_box(&mut sprites, chrome, *group_box, scale),
        Some(Body::Buddy(layout)) => {
            let group_box = layout
                .rect_of(crate::ui::wol_buddy::GROUP_BOX)
                .unwrap_or_default();
            push_group_box(&mut sprites, chrome, group_box, scale)
        }
        _ => {}
    }
    let shown = body.filter(|_| children);
    let buddy_page = crate::app::shell_wol::buddy::page(state);
    let buddy_children = match (shown, buddy_page) {
        (Some(Body::Buddy(layout)), Some(dialog)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_buddy_children(
                state, &dialog.ui, layout, art,
            ),
        ),
        _ => None,
    };
    let add_buddy = buddy_page
        .and_then(|dialog| dialog.ui.add.as_ref())
        .map(|_| {
            crate::app::frontend::skirmish_shell_render::paint_wol_add_buddy(
                state,
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
        (Some(painted), Some(Body::MyInfo(_) | Body::Widgets)) => Some(
            crate::app::shell_wol::leaf::paint(state, painted.dialog, painted.page.scale),
        ),
        _ => None,
    };
    let login = state.frontend.wol.as_ref().and_then(|visit| visit.login());
    let login_children = match (shown, login) {
        (Some(Body::Login(layout)), Some(login)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_login_children(
                state, login, layout,
            ),
        ),
        _ => None,
    };
    // VERA online's account popup over the login page.
    let account_form = match (shown, login.and_then(|login| login.account.as_ref())) {
        (Some(Body::Login(layout)), Some(form)) => {
            crate::app::shell_wol::account_form_layout(state).map(|rects| {
                (
                    rects.dialog,
                    crate::app::frontend::skirmish_shell_render::paint_wol_account_form(
                        state,
                        form,
                        &rects,
                        layout.page.scale,
                    ),
                )
            })
        }
        _ => None,
    };
    let lobby = state.frontend.wol.as_ref().and_then(|visit| visit.lobby());
    let lobby_children = match (shown, lobby) {
        (Some(Body::Lobby(layout)), Some(lobby)) => {
            let base_names = crate::app::shell_wol::lobby::base_names(state);
            Some(
                crate::app::frontend::skirmish_shell_render::paint_wol_lobby_children(
                    state,
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
        Some(Body::Lobby(layout)) => crate::app::shell_wol::player_card::view(state).map(|view| {
            let scale = layout.page.scale;
            (
                view.layout.dialog,
                crate::app::frontend::skirmish_shell_render::paint_wol_player_card(
                    state, &view, art, scale,
                ),
                view.countries.as_ref().map(|list| {
                    (
                        list.popup.rect,
                        crate::app::frontend::skirmish_shell_render::paint_wol_card_countries(
                            state, list, scale,
                        ),
                    )
                }),
            )
        }),
        _ => None,
    };
    let lobby_popup = lobby.and_then(|lobby| {
        lobby.ui.popup.as_ref()?;
        let status = if page_statics || painted.is_none() {
            crate::app::shell_wol::lobby::status_help(state)
        } else {
            String::new()
        };
        Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_lobby_popup(
                state,
                &lobby.ui,
                screen_w,
                screen_h,
                crate::app::shell_wol::lobby::popup_scale(state),
                &status,
            ),
        )
    });

    let quick_match_page = crate::app::shell_wol::quick_match::dialog(state);
    let quick_match_children = match (shown, quick_match_page) {
        (Some(Body::QuickMatch(layout)), Some(dialog)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_quick_match_children(
                state, &dialog.ui, layout,
            ),
        ),
        _ => None,
    };
    // Start Game Now's 0x130 over Quick Match's backdrop (0x117 closed at
    // OnGameStart; no C2/C9 exists).
    let verify = crate::app::shell_network_setup::verify(state)
        .filter(|_| crate::app::shell_network_setup::quick_match(state))
        .map(|verify| {
            crate::app::frontend::skirmish_shell_render::paint_wol_verify(
                state,
                verify,
                &crate::app::shell_network_setup::verify_layout(state),
            )
        });

    let page_dialogs = page_dialogs_paint(state);

    // The column: the slide engine draws it while a slide runs, the empty
    // backdrop draws it shut, otherwise the page's buttons paint.
    let controller = &state.frontend.shell_controller;
    let mut painted_buttons: Vec<(MenuPageButtonRect, bool, bool)> = Vec::new();
    let mut spec: Option<&MenuPageSpec> = None;
    let column = match (&painted, &wave) {
        (_, Some(wave)) => Some(wave.button_draws()),
        (None, None) => Some(shell_paint::shuttered_column(page.right_panel)),
        (Some(painted), None) => {
            spec = Some(painted.spec);
            painted_buttons = match (&painted.body, painted.dialog) {
                (Body::Welcome(layout), dialog) => {
                    let active = controller.top_id() == Some(dialog);
                    let pressed = active.then(|| crate::app::App::wol_held(state)).flatten();
                    let buttons = layout.painted_buttons(pressed).into_iter();
                    buttons
                        .map(|button| (button, pressed == Some(button.id), true))
                        .collect()
                }
                (Body::Lobby(_), _) => lobby.map_or_else(Vec::new, |lobby| {
                    let ui = &lobby.ui;
                    let buttons = page.buttons.iter();
                    buttons
                        .map(|button| (*button, ui.pressed(button.id), ui.enabled(button.id)))
                        .collect()
                }),
                (_, crate::ui::wol_options::DIALOG | SOUND_DIALOG) => page
                    .buttons
                    .iter()
                    .map(|button| {
                        let pressed =
                            crate::app::shell_wol::options::button_pressed(state, button.id);
                        (*button, pressed, true)
                    })
                    .collect(),
                (_, dialog) => crate::app::shell_wol::leaf::buttons(state, dialog, &page.buttons),
            };
            None
        }
    };
    if let Some(draws) = column {
        sprites.extend(shell_paint::paint_slide_column(
            chrome,
            page.right_panel,
            &draws,
        ));
    }
    let buttons: Vec<PaintButton> = painted_buttons
        .iter()
        .map(|(button, pressed, enabled)| PaintButton {
            rect: button.rect,
            pressed: *pressed,
            hovered: false,
            enabled: *enabled,
        })
        .collect();
    let button_sprites = shell_paint::paint_buttons(
        chrome,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        crate::util::time::Instant::now(),
        None,
    );
    for (button, pressed, enabled) in &painted_buttons {
        let Some(spec) = spec.and_then(|spec| spec.button(button.id)) else {
            continue;
        };
        let text = state.csf_text(spec.csf_key);
        labels.push(PaintLabel {
            text: text.into(),
            rect: owner_draw_button_label_rect(button.rect, *pressed),
            align: ShellAlign::BUTTON_CAPTION,
            rgb: crate::app::frontend::skirmish_shell_render::wol_button_label_rgb(*enabled),
            path_a_reveal: None,
        });
    }
    let title_key = painted.as_ref().and_then(Painted::title_key);
    if let (Some(window), Some(key)) = (title_window, title_key) {
        labels.push(PaintLabel {
            text: state.csf_text(key).into(),
            rect: page.title,
            align: ShellAlign::PANEL_TITLE,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    labels.extend(status_label);
    let mut text =
        shell_paint::paint_labels_scaled(&state.renderer.bit_font, &labels, scale.factor());
    text.extend(welcome_texts.into_iter().flatten());

    // The boxes over the backdrop: PUDLGBGN with MNBTTN buttons.
    let mut box_sprites = Vec::new();
    let mut box_dialog = None;
    let open = message.or(wait.map(|wait| wait.shell_box));
    if let (Some(shell_box), Some(atlas)) = (open, state.frontend.skirmish_shell_chrome.as_ref()) {
        let parts = shell_box.parts(screen_w, screen_h, scale);
        box_dialog = Some(parts.dialog);
        let held = crate::app::App::wol_held(state);
        let look = BoxLook::new(atlas.mode_two_dialog_background(None), BOX_DEPTHS);
        let draw = paint_shell_box(
            state,
            Some(atlas),
            &shell_box,
            &parts,
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
    if let Some(art) = art {
        draws.push(TexturedDraw {
            texture: &art.texture,
            instances: icon_sprites,
        });
    }
    draws.push(TexturedDraw {
        texture: &chrome.texture,
        instances: button_sprites,
    });
    let controls_atlas = state.frontend.skirmish_shell_chrome.as_ref();
    if let (Some(atlas), Some(login)) = (controls_atlas, login_children) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: login.controls,
        });
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: login.popup,
        });
        text.extend(login.texts);
        text.extend(login.popup_texts);
    }
    if let (Some(atlas), Some((dialog, form))) = (controls_atlas, account_form) {
        hide_glyphs_under(&mut text, dialog);
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: form.controls,
        });
        text.extend(form.texts);
    }
    if let (Some(atlas), Some(mut widgets)) = (controls_atlas, widget_children) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: widgets.controls,
        });
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: widgets.popup,
        });
        if painted.as_ref().map(|painted| painted.dialog) == Some(crate::ui::wol_options::DIALOG)
            && let Some(list) = crate::app::shell_wol::options::translate_popup(state)
        {
            hide_glyphs_under(&mut widgets.texts, list.rect);
        }
        text.extend(widgets.texts);
        text.extend(widgets.popup_texts);
    }
    if let (Some(atlas), Some(mut lobby)) = (controls_atlas, lobby_children) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: lobby.controls,
        });
        if let Some(art) = art {
            draws.push(TexturedDraw {
                texture: &art.texture,
                instances: lobby.icons,
            });
        }
        if let Some(dialog) = box_dialog {
            hide_glyphs_under(&mut lobby.texts, dialog);
        }
        text.extend(lobby.texts);
    }
    if let (Some(atlas), Some(mut quick_match)) = (controls_atlas, quick_match_children) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: quick_match.controls,
        });
        if let Some(dialog) = box_dialog {
            hide_glyphs_under(&mut quick_match.texts, dialog);
        }
        text.extend(quick_match.texts);
    }
    if let (Some(atlas), Some(verify)) = (controls_atlas, verify) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: verify.controls,
        });
        text.extend(verify.texts);
    }
    if let (Some(atlas), Some(buddy)) = (controls_atlas, buddy_children) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: buddy.controls,
        });
        if let Some(art) = art {
            draws.push(TexturedDraw {
                texture: &art.texture,
                instances: buddy.icons,
            });
        }
        text.extend(buddy.texts);
    }
    if let Some(dialog) = buddy_popup {
        hide_glyphs_under(&mut text, dialog);
    }
    if let (Some(atlas), Some(add)) = (controls_atlas, add_buddy) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: add.controls,
        });
        text.extend(add.texts);
    }
    if let (Some(atlas), Some(popup)) = (controls_atlas, lobby_popup) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: popup.controls,
        });
        text.extend(popup.texts);
    }
    if let (Some(atlas), Some((dialog, card, countries))) = (controls_atlas, player_card) {
        hide_glyphs_under(&mut text, dialog);
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: card.controls,
        });
        if let Some(art) = art {
            draws.push(TexturedDraw {
                texture: &art.texture,
                instances: card.icons,
            });
        }
        text.extend(card.texts);
        if let Some((list, countries)) = countries {
            hide_glyphs_under(&mut text, list);
            draws.push(TexturedDraw {
                texture: &atlas.texture,
                instances: countries.controls,
            });
            text.extend(countries.texts);
        }
    }
    // 0xCF and 0x11E leave the page painting beneath them, over the lobby,
    // the buddy list or any other page.
    if let (Some(atlas), Some((dialog, page))) = (controls_atlas, page_dialogs) {
        hide_glyphs_under(&mut text, dialog);
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: page.controls,
        });
        text.extend(page.texts);
    }
    if let (Some(atlas), Some(_)) = (state.frontend.skirmish_shell_chrome.as_ref(), box_dialog) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: box_sprites,
        });
    }
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Westwood Online",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
            smooth_art: state.smooth_online_menus(),
        },
    );
    state.platform.window.request_redraw();
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
