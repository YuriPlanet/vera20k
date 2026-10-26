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
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::shell_wol::WolPage;
use crate::render::shell_paint::{
    self, CHROME_DEPTH, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_ENABLED, push_entry_scaled,
};
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::{ShellScale, centred_in_window};
use crate::ui::shell::menu_page::{MenuPageButtonRect, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::slide::PanelArt;
use crate::ui::wol_shell::{
    BOX_NO, BOX_OK, BOX_YES, StaticAlign, WAIT_CHECK_STATUS, WAIT_DISCONNECT, WOL_LOBBY_TITLE_KEY,
    WolBoxButtons, WolTextCaption,
};

/// Icon and group-box children sit over the background, under the text.
const CHILD_DEPTH: f32 = CHROME_DEPTH - 0.00002;

const BOX_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: 0.00050,
    button: 0.00045,
    text: 0.00040,
};

fn align(value: StaticAlign) -> ShellAlign {
    match value {
        StaticAlign::Left => ShellAlign::NONE,
        StaticAlign::Center => ShellAlign::H_CENTER,
        StaticAlign::Right => ShellAlign::H_RIGHT,
    }
}

/// A box `0xD0` or `0x126` from `0x005E2CD0`: its dialog, buttons with
/// their pushed state, and labels (the body, then the button captions).
pub(crate) fn message_box_parts(
    message: &crate::ui::wol_shell::WolMessageBox,
    pressed: Option<u16>,
    screen_w: i32,
    screen_h: i32,
) -> (
    crate::ui::shell::geom::RectPx,
    Vec<(crate::ui::shell::geom::RectPx, bool)>,
    Vec<PaintLabel<'static>>,
) {
    let label = |value: &str, rect, align| PaintLabel {
        text: value.to_owned().into(),
        rect,
        align,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    };
    let button_align = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
    match &message.buttons {
        WolBoxButtons::Ok(ok) => {
            let geometry = crate::ui::shell::modal::body_ok_layout(screen_w, screen_h);
            let ok_pressed = pressed == Some(BOX_OK);
            (
                geometry.dialog,
                vec![(geometry.ok, ok_pressed)],
                vec![
                    label(&message.body, geometry.body, ShellAlign::NONE),
                    label(
                        ok,
                        owner_draw_button_label_rect(geometry.ok, ok_pressed),
                        button_align,
                    ),
                ],
            )
        }
        WolBoxButtons::YesNo { yes, no } => {
            let geometry = crate::ui::shell::modal::yes_no_layout(screen_w, screen_h);
            let yes_pressed = pressed == Some(BOX_YES);
            let no_pressed = pressed == Some(BOX_NO);
            (
                geometry.dialog,
                vec![(geometry.yes, yes_pressed), (geometry.no, no_pressed)],
                vec![
                    label(&message.body, geometry.body, ShellAlign::NONE),
                    label(
                        yes,
                        owner_draw_button_label_rect(geometry.yes, yes_pressed),
                        button_align,
                    ),
                    label(
                        no,
                        owner_draw_button_label_rect(geometry.no, no_pressed),
                        button_align,
                    ),
                ],
            )
        }
    }
}

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

/// `0xCF` or `0x11E` over whatever the page shows.
pub(crate) fn page_dialogs_paint(
    state: &AppState,
) -> Option<crate::app::frontend::skirmish_shell_render::WolLobbyPaint> {
    use crate::app::frontend::skirmish_shell_render::{
        paint_wol_page_dialog, paint_wol_page_received,
    };
    use crate::app::shell_wol::page;
    if let Some(dialog) = page::page_dialog(state) {
        return Some(paint_wol_page_dialog(
            state,
            dialog,
            &page::page_layout(state),
        ));
    }
    let received = page::page_received(state)?;
    Some(paint_wol_page_received(
        state,
        received,
        &page::received_layout(state),
    ))
}

/// The page being painted and what it needs beyond the shared chrome.
enum Painted {
    Welcome(crate::ui::wol_shell::WolWelcomeLayout),
    Login(crate::ui::wol_login::WolLoginLayout),
    Lobby(crate::ui::wol_lobby::WolLobbyLayout),
    Options(crate::ui::wol_options::WolOptionsLayout),
    SoundOptions(crate::ui::shell::sound::ShellSoundLayout),
    /// `0x112` or `0xFE`.
    Persona(crate::ui::wol_persona::WolPersonaLayout),
    /// `0x114`.
    MyInfo(crate::ui::wol_my_info::WolMyInfoLayout),
    /// `0x116`.
    Buddy(crate::ui::wol_buddy::WolBuddyLayout),
    /// `0x117`.
    QuickMatch(crate::ui::wol_quick_match::WolQuickMatchLayout),
    /// A dialog has closed: the empty backdrop, under a box or the wait box.
    Backdrop,
}

impl Painted {
    fn kind(&self) -> Option<ShellSlideKind> {
        match self {
            Self::Welcome(_) => Some(ShellSlideKind::WolWelcome),
            Self::Login(_) => Some(ShellSlideKind::WolLogin),
            Self::Lobby(_) => Some(ShellSlideKind::WolLobby),
            Self::Options(_) => Some(ShellSlideKind::WolOptions),
            Self::SoundOptions(_) => Some(ShellSlideKind::WolSoundOptions),
            Self::Persona(layout) if layout.dialog == crate::ui::wol_persona::LOCALE_DIALOG => {
                Some(ShellSlideKind::WolLocale)
            }
            Self::Persona(layout) if layout.dialog == crate::ui::wol_persona::ADD_BUDDY_DIALOG => {
                Some(ShellSlideKind::WolAddBuddyQuery)
            }
            Self::Persona(_) => Some(ShellSlideKind::WolAutoLoginQuery),
            Self::MyInfo(_) => Some(ShellSlideKind::WolMyInformation),
            Self::Buddy(_) => Some(ShellSlideKind::WolBuddyList),
            Self::QuickMatch(_) => Some(ShellSlideKind::WolQuickMatch),
            Self::Backdrop => None,
        }
    }

    fn title_key(&self) -> Option<&'static str> {
        match self {
            Self::Welcome(layout) => Some(layout.page_spec().title_key),
            Self::Login(layout) => Some(layout.page_spec().title_key),
            Self::Lobby(_) => Some(WOL_LOBBY_TITLE_KEY),
            Self::Options(_) => Some(crate::ui::wol_options::PAGE.title_key),
            Self::SoundOptions(_) => Some(crate::ui::shell::sound::SHELL_SOUND_PAGE.title_key),
            // 0x122 has no heading.
            Self::Persona(layout) => {
                Some(crate::ui::wol_persona::WolPersonaLayout::spec(layout.dialog).title_key)
                    .filter(|key| !key.is_empty())
            }
            Self::MyInfo(_) => Some(crate::ui::wol_my_info::PAGE.title_key),
            Self::Buddy(_) => Some(crate::ui::wol_buddy::PAGE.title_key),
            Self::QuickMatch(_) => Some(crate::ui::wol_quick_match::PAGE.title_key),
            Self::Backdrop => None,
        }
    }
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
    let welcome = crate::app::App::wol_welcome_layout(state);
    let painted = match &state.frontend.wol.as_ref().expect("checked").page {
        // 0x11E opens over the empty backdrop (0x0052FEC0), except from
        // 0x116's Page, which runs it over the Buddy List.
        _ if crate::app::shell_wol::page::page_received(state)
            .is_some_and(|received| !received.modal) =>
        {
            Painted::Backdrop
        }
        WolPage::Welcome => {
            crate::app::App::ensure_wol_welcome_art(state);
            Painted::Welcome(welcome.clone())
        }
        WolPage::Login(_) => {
            crate::app::shell_wol::open_login_art(state);
            Painted::Login(crate::app::shell_wol::login_layout(state))
        }
        WolPage::Lobby => {
            crate::app::shell_wol::lobby::open_art(state);
            // Hidden (created hidden, or slid out for a join) or covered by
            // the empty backdrop for a popup: only the backdrop shows.
            let visible = state
                .frontend
                .wol
                .as_ref()
                .and_then(|visit| visit.lobby())
                .is_some_and(|lobby| lobby.shown && !lobby.covered);
            if visible {
                Painted::Lobby(crate::app::shell_wol::lobby::layout(state))
            } else {
                Painted::Backdrop
            }
        }
        WolPage::Options(page) => {
            let sound = page.sound.is_some();
            if sound {
                crate::app::shell_wol::options::open_sound_art(state);
                Painted::SoundOptions(crate::app::shell_wol::options::sound_layout(state))
            } else {
                crate::app::shell_wol::options::open_art(state);
                Painted::Options(crate::app::shell_wol::options::layout(state))
            }
        }
        WolPage::AutoLoginQuery(_) | WolPage::Locale(_) | WolPage::AddBuddyQuery(_) => {
            if matches!(
                state.frontend.wol.as_ref().map(|visit| &visit.page),
                Some(WolPage::AutoLoginQuery(_))
            ) {
                crate::app::shell_wol::persona::open_art(state);
            }
            match crate::app::shell_wol::persona::layout(state) {
                Some(layout) => Painted::Persona(layout),
                None => Painted::Backdrop,
            }
        }
        WolPage::MyInformation(_) => {
            crate::app::shell_wol::my_info::open_art(state);
            Painted::MyInfo(crate::app::shell_wol::my_info::layout(state))
        }
        WolPage::Buddy(_) => {
            crate::app::shell_wol::buddy::open_art(state);
            Painted::Buddy(crate::app::shell_wol::buddy::layout(state))
        }
        WolPage::QuickMatch(_) => {
            crate::app::shell_wol::quick_match::open_art(state);
            Painted::QuickMatch(crate::app::shell_wol::quick_match::layout(state))
        }
        WolPage::Backdrop { .. } | WolPage::Message { .. } => Painted::Backdrop,
    };
    // The right panel's geometry is the family's on every WOL page.
    let page: &MenuPageLayout = match &painted {
        Painted::Login(layout) => &layout.page,
        Painted::Lobby(layout) => &layout.page,
        Painted::Options(layout) => &layout.page,
        Painted::SoundOptions(layout) => &layout.page,
        Painted::Persona(layout) => &layout.page,
        Painted::MyInfo(layout) => &layout.page,
        Painted::Buddy(layout) => &layout.page,
        Painted::QuickMatch(layout) => &layout.page,
        _ => &welcome.page,
    };
    let page = page.clone();
    let screen_w = page.screen.w;
    let screen_h = page.screen.h;
    let scale = page.scale;
    let visit = state.frontend.wol.as_ref().expect("checked");
    let message = visit.message().cloned();
    let wait = visit.wait_box().filter(|wait| wait.shown).cloned();

    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: every child stays blank.
    let exit_wave = painted.kind().and_then(|kind| {
        crate::app::frontend::shell_transition::shell_exit_wave(state, kind).cloned()
    });
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| {
        painted
            .kind()
            .and_then(|_| state.frontend.shell_first_paint_slide.clone())
    });
    let page_statics = painted.kind().is_some() && !leaving;
    // The chat lobby 0x113 has no warning monitor (template static 0x71C;
    // 0x60AB71..0x60AC9E lists the family dialogs carrying it) but has the
    // +0xD5 top panel instead; every other painted WOL page is the reverse.
    let top_panel = matches!(painted, Painted::Lobby(_));
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
        let (status_key, status_rect) = match &painted {
            Painted::Welcome(layout) => {
                (crate::app::App::wol_status_key(state), layout.status_help)
            }
            Painted::Lobby(layout) => (None, layout.page.status_help),
            Painted::Options(layout) => (
                crate::app::shell_wol::options::status_key(state),
                layout.status_help,
            ),
            Painted::SoundOptions(layout) => (None, layout.status_help),
            Painted::MyInfo(layout) => (
                crate::app::shell_wol::my_info::status_key(state),
                layout.status_help,
            ),
            Painted::Buddy(layout) => (
                crate::app::shell_wol::buddy::status_key(state),
                layout.status_help,
            ),
            Painted::Persona(layout) => (
                crate::app::shell_wol::persona::status_key(state),
                layout.status_help,
            ),
            Painted::QuickMatch(layout) => (
                crate::app::shell_wol::quick_match::status_key(state),
                layout.status_help,
            ),
            Painted::Login(layout) => (
                state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.login())
                    .and_then(|login| login.buttons.hovered)
                    .and_then(|control| layout.tooltip_key(control)),
                layout.status_help,
            ),
            _ => (None, page.status_help),
        };
        let status_text = match &painted {
            Painted::Lobby(_) => crate::app::shell_wol::lobby::status_help(state),
            _ => status_key
                .map(|key| resolve_csf(state, key).into_owned())
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
    let art = match &painted {
        Painted::Welcome(_) => state.frontend.wol_welcome_art.as_ref(),
        Painted::Login(_) => state.frontend.wol_login_art.as_ref(),
        Painted::Lobby(_) => state.frontend.wol_lobby_art.as_ref(),
        Painted::Options(_) => state.frontend.wol_options_art.as_ref(),
        Painted::SoundOptions(_) => state.frontend.wol_sound_options_art.as_ref(),
        Painted::Persona(layout) if layout.dialog == crate::ui::wol_persona::AUTO_LOGIN_DIALOG => {
            state.frontend.wol_auto_login_art.as_ref()
        }
        Painted::MyInfo(_) => state.frontend.wol_my_info_art.as_ref(),
        Painted::Buddy(_) => state.frontend.wol_buddy_art.as_ref(),
        Painted::QuickMatch(_) => state.frontend.wol_quick_match_art.as_ref(),
        // 0xFE and 0x122 load no art of their own.
        Painted::Persona(_) | Painted::Backdrop => None,
    };
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
    if top_panel && painted.kind().is_some() {
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
    if let (true, Painted::Welcome(layout)) = (children, &painted) {
        for window in &layout.group_boxes {
            push_group_box(&mut sprites, chrome, *window, scale);
        }
        if let Some(art) = art {
            for (file, window) in &layout.icons {
                let Some((_, entry)) = art.icons.iter().find(|(name, _)| name == file) else {
                    continue;
                };
                let (w, h) = entry.native_size();
                let (x, y) = centred_in_window(scale.base_rect(*window), w, h);
                push_entry_scaled(&mut icon_sprites, *entry, x, y, scale, CHILD_DEPTH);
            }
        }
        let filled = &state.frontend.wol.as_ref().expect("checked").welcome_texts;
        for (text, rect) in &layout.texts {
            let caption = match filled.iter().find(|(id, _)| Some(*id) == text.id) {
                Some((_, value)) => value.as_str().into(),
                None => match text.caption {
                    WolTextCaption::Csf(key) => resolve_csf(state, key),
                    WolTextCaption::Literal(value) => value.into(),
                },
            };
            if caption.is_empty() {
                continue;
            }
            labels.push(PaintLabel {
                text: caption,
                rect: *rect,
                align: align(text.align),
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            });
        }
    }
    if let (true, Painted::MyInfo(layout)) = (children, &painted) {
        push_group_box(&mut sprites, chrome, layout.group_box, scale);
    }
    if let (true, Painted::Buddy(layout)) = (children, &painted) {
        push_group_box(&mut sprites, chrome, layout.group_box, scale);
    }
    let buddy_page = crate::app::shell_wol::buddy::page(state);
    let buddy_children = match (&painted, children, buddy_page) {
        (Painted::Buddy(layout), true, Some(dialog)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_buddy_children(
                state, &dialog.ui, layout, art,
            ),
        ),
        _ => None,
    };
    let add_buddy = buddy_page
        .and_then(|dialog| dialog.ui.add.as_ref())
        .map(|add| {
            crate::app::frontend::skirmish_shell_render::paint_wol_add_buddy(
                state,
                add,
                &crate::ui::shell::modal::wol_add_buddy_layout(screen_w, screen_h),
            )
        });
    // 0x115 and the modal 0x11E sit over 0x116 without covering it.
    let buddy_popup = match &painted {
        Painted::Buddy(_) if add_buddy.is_some() => {
            Some(crate::ui::shell::modal::wol_add_buddy_layout(screen_w, screen_h).dialog)
        }
        Painted::Buddy(_) if crate::app::shell_wol::page::page_received(state).is_some() => {
            Some(crate::app::shell_wol::page::received_layout(state).dialog)
        }
        _ => None,
    };
    let my_info_children = match (&painted, children) {
        (Painted::MyInfo(layout), true) => {
            crate::app::shell_wol::my_info::page(state).map(|page| {
                crate::app::frontend::skirmish_shell_render::paint_wol_my_info_children(
                    state, page, layout,
                )
            })
        }
        _ => None,
    };
    let login_children = match (&painted, children) {
        (Painted::Login(layout), true) => state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.login())
            .map(|login| {
                crate::app::frontend::skirmish_shell_render::paint_wol_login_children(
                    state, login, layout,
                )
            }),
        _ => None,
    };
    let options_page = match &state.frontend.wol.as_ref().expect("checked").page {
        WolPage::Options(page) => Some(page.as_ref()),
        _ => None,
    };
    let options_children = match (&painted, children, options_page) {
        (Painted::Options(layout), true, Some(page)) => Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_options_children(
                state, &page.ui, layout,
            ),
        ),
        (Painted::SoundOptions(layout), true, Some(page)) => page.sound.as_ref().map(|dialog| {
            crate::app::frontend::skirmish_shell_render::paint_shell_sound_children(
                state, dialog, layout,
            )
        }),
        (Painted::Persona(layout), true, _) => {
            match &state.frontend.wol.as_ref().expect("checked").page {
                WolPage::AutoLoginQuery(query) => Some(
                    crate::app::frontend::skirmish_shell_render::paint_wol_auto_login_children(
                        state,
                        query,
                        &resolve_csf(state, crate::ui::wol_persona::AUTO_LOGIN_TEXT_KEY),
                        layout,
                    ),
                ),
                WolPage::AddBuddyQuery(page) => Some(
                    crate::app::frontend::skirmish_shell_render::paint_wol_auto_login_children(
                        state,
                        &page.query,
                        &crate::app::shell_wol::persona::add_buddy_text(state).unwrap_or_default(),
                        layout,
                    ),
                ),
                WolPage::Locale(page) => Some(
                    crate::app::frontend::skirmish_shell_render::paint_wol_locale_children(
                        state, page, layout,
                    ),
                ),
                _ => None,
            }
        }
        _ => None,
    };
    let lobby = state.frontend.wol.as_ref().and_then(|visit| visit.lobby());
    let lobby_children = match (&painted, children, lobby) {
        (Painted::Lobby(layout), true, Some(lobby)) => {
            let base_names: [String; 8] = std::array::from_fn(|index| {
                resolve_csf(state, &format!("TXT_LOB_{}", index + 1)).into_owned()
            });
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
    let lobby_popup = lobby.and_then(|lobby| {
        let popup = lobby.ui.popup.as_ref()?;
        let status = if page_statics || matches!(painted, Painted::Backdrop) {
            crate::app::shell_wol::lobby::status_help(state)
        } else {
            String::new()
        };
        Some(
            crate::app::frontend::skirmish_shell_render::paint_wol_lobby_popup(
                state,
                popup,
                screen_w,
                screen_h,
                &status,
                lobby.ui.caret_visible(crate::util::time::Instant::now()),
            ),
        )
    });

    let quick_match_page = crate::app::shell_wol::quick_match::dialog(state);
    let quick_match_children = match (&painted, children, quick_match_page) {
        (Painted::QuickMatch(layout), true, Some(dialog)) => Some(
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
        (Painted::Backdrop, None) => Some(shell_paint::shuttered_column(page.right_panel)),
        (Painted::Welcome(layout), None) => {
            spec = Some(layout.page_spec());
            let active = controller.top_id() == Some(layout.page_spec().dialog);
            let pressed = active.then(|| controller.pressed()).flatten();
            painted_buttons = layout
                .painted_buttons(pressed)
                .into_iter()
                .map(|button| (button, pressed == Some(button.id), true))
                .collect();
            None
        }
        (Painted::Login(layout), None) => {
            spec = Some(layout.page_spec());
            if let Some(login) = state.frontend.wol.as_ref().and_then(|visit| visit.login()) {
                painted_buttons = layout
                    .page
                    .buttons
                    .iter()
                    .map(|button| {
                        (
                            *button,
                            login.buttons.is_pressed(button.id),
                            login.button_enabled(button.id),
                        )
                    })
                    .collect();
            }
            None
        }
        (Painted::Options(layout), None) => {
            spec = Some(&crate::ui::wol_options::PAGE);
            if let Some(page) = options_page {
                painted_buttons = layout
                    .page
                    .buttons
                    .iter()
                    .map(|button| (*button, page.ui.buttons.is_pressed(button.id), true))
                    .collect();
            }
            None
        }
        (Painted::SoundOptions(layout), None) => {
            spec = Some(&crate::ui::shell::sound::SHELL_SOUND_PAGE);
            if let Some(dialog) = options_page.and_then(|page| page.sound.as_ref()) {
                painted_buttons = layout
                    .page
                    .buttons
                    .iter()
                    .map(|button| {
                        (
                            *button,
                            dialog
                                .buttons
                                .is_pressed(crate::ui::shell::sound::SoundButton::Back),
                            true,
                        )
                    })
                    .collect();
            }
            None
        }
        (Painted::Persona(layout), None) => {
            spec = Some(crate::ui::wol_persona::WolPersonaLayout::spec(
                layout.dialog,
            ));
            match &state.frontend.wol.as_ref().expect("checked").page {
                WolPage::AutoLoginQuery(query) => {
                    painted_buttons = layout
                        .page
                        .buttons
                        .iter()
                        .map(|button| (*button, query.buttons.is_pressed(button.id), true))
                        .collect();
                }
                WolPage::AddBuddyQuery(page) => {
                    painted_buttons = layout
                        .page
                        .buttons
                        .iter()
                        .map(|button| (*button, page.query.buttons.is_pressed(button.id), true))
                        .collect();
                }
                WolPage::Locale(page) => {
                    painted_buttons = layout
                        .page
                        .buttons
                        .iter()
                        .map(|button| {
                            (
                                *button,
                                page.buttons.is_pressed(button.id),
                                page.button_enabled(button.id),
                            )
                        })
                        .collect();
                }
                _ => {}
            }
            None
        }
        (Painted::MyInfo(layout), None) => {
            spec = Some(&crate::ui::wol_my_info::PAGE);
            if let Some(page) = crate::app::shell_wol::my_info::page(state) {
                painted_buttons = layout
                    .page
                    .buttons
                    .iter()
                    .map(|button| {
                        (
                            *button,
                            page.buttons.is_pressed(button.id),
                            page.enabled(button.id),
                        )
                    })
                    .collect();
            }
            None
        }
        (Painted::Buddy(layout), None) => {
            spec = Some(&crate::ui::wol_buddy::PAGE);
            if let Some(dialog) = buddy_page {
                painted_buttons = layout
                    .page
                    .buttons
                    .iter()
                    .map(|button| {
                        (
                            *button,
                            dialog.ui.buttons.is_pressed(button.id),
                            dialog.ui.enabled(button.id),
                        )
                    })
                    .collect();
            }
            None
        }
        (Painted::QuickMatch(layout), None) => {
            spec = Some(&crate::ui::wol_quick_match::PAGE);
            if let Some(dialog) = quick_match_page {
                painted_buttons = layout
                    .page
                    .buttons
                    .iter()
                    .map(|button| (*button, dialog.ui.buttons.is_pressed(button.id), true))
                    .collect();
            }
            None
        }
        (Painted::Lobby(layout), None) => {
            spec = Some(&crate::ui::wol_lobby::PAGE);
            if let Some(lobby) = lobby {
                spec = Some(lobby.ui.page_spec());
                painted_buttons = layout
                    .page
                    .buttons
                    .iter()
                    .map(|button| {
                        (
                            *button,
                            lobby.ui.buttons.is_pressed(button.id),
                            lobby.ui.enabled(button.id),
                        )
                    })
                    .collect();
            }
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
        let text = resolve_csf(state, spec.csf_key);
        labels.push(PaintLabel {
            text,
            rect: owner_draw_button_label_rect(button.rect, *pressed),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: crate::app::frontend::skirmish_shell_render::wol_button_label_rgb(*enabled),
            path_a_reveal: None,
        });
    }
    if let (Some(window), Some(key)) = (title_window, painted.title_key()) {
        labels.push(PaintLabel {
            text: resolve_csf(state, key),
            rect: page.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    labels.extend(status_label);
    let mut text =
        shell_paint::paint_labels_scaled(&state.renderer.bit_font, &labels, scale.factor());

    // The boxes over the backdrop: PUDLGBGN with MNBTTN buttons.
    let mut box_sprites = Vec::new();
    let mut box_dialog = None;
    let box_atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .filter(|_| message.is_some() || wait.is_some());
    if let Some(atlas) = box_atlas {
        let pressed = state.frontend.shell_controller.pressed();
        let label = |value: &str, rect, align| PaintLabel {
            text: value.to_owned().into(),
            rect,
            align,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        };
        let button_align = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
        let (dialog, buttons, box_labels) = if let Some(message) = &message {
            message_box_parts(message, pressed, screen_w, screen_h)
        } else {
            let wait = wait.as_ref().expect("filtered above");
            let geometry = crate::ui::shell::modal::wait_box_layout(screen_w, screen_h);
            let disconnect = pressed == Some(WAIT_DISCONNECT);
            let check = pressed == Some(WAIT_CHECK_STATUS);
            (
                geometry.dialog,
                vec![
                    (geometry.disconnect, disconnect),
                    (geometry.check_status, check),
                ],
                vec![
                    // 0x5B2 is SS_CENTER.
                    label(&wait.text, geometry.text, ShellAlign::H_CENTER),
                    label(
                        &wait.disconnect,
                        owner_draw_button_label_rect(geometry.disconnect, disconnect),
                        button_align,
                    ),
                    label(
                        &wait.check_status,
                        owner_draw_button_label_rect(geometry.check_status, check),
                        button_align,
                    ),
                ],
            )
        };
        box_dialog = Some(dialog);
        let modal_buttons: Vec<shell_paint::ModalButton> = buttons
            .iter()
            .map(|(rect, pressed)| shell_paint::ModalButton {
                rect: *rect,
                pressed: *pressed,
                enabled: true,
            })
            .collect();
        let draw = shell_paint::paint_modal_shp(
            &state.renderer.bit_font,
            atlas.mode_two_dialog_background(None),
            crate::app::frontend::skirmish_shell_render::type3_button_frames(atlas),
            dialog,
            &modal_buttons,
            &box_labels,
            ShellScale::RETAIL,
            BOX_DEPTHS,
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
    if let (Some(atlas), Some(my_info)) = (controls_atlas, my_info_children) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: my_info.controls,
        });
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: my_info.popup,
        });
        text.extend(my_info.texts);
        text.extend(my_info.popup_texts);
    }
    if let (Some(atlas), Some(options)) = (controls_atlas, options_children) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: options.controls,
        });
        text.extend(options.texts);
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
    if let (Some(atlas), Some(page)) = (controls_atlas, page_dialogs) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: page.controls,
        });
        text.extend(page.texts);
    }
    if let Some(atlas) = box_atlas {
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
