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
use crate::ui::wol_shell::{
    BOX_NO, BOX_OK, BOX_YES, StaticAlign, WAIT_CHECK_STATUS, WAIT_DISCONNECT, WOL_LOBBY_TITLE_KEY,
    WOL_WELCOME_PAGE, WolBoxButtons,
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

/// Which shell the capture driver saw for the visit's current page.
pub(crate) fn presented_wol_page(state: &AppState) -> PresentedShell {
    match state.frontend.wol.as_ref().map(|visit| &visit.page) {
        Some(WolPage::Login(_)) => PresentedShell::WolLogin,
        Some(WolPage::Lobby) => PresentedShell::WolLobby,
        _ => PresentedShell::WolWelcome,
    }
}

/// The page being painted and what it needs beyond the shared chrome.
enum Painted {
    Welcome(crate::ui::wol_shell::WolWelcomeLayout),
    Login(crate::ui::wol_login::WolLoginLayout),
    Lobby(crate::ui::wol_lobby::WolLobbyLayout),
    /// A dialog has closed: the empty backdrop, under a box or the wait box.
    Backdrop,
}

impl Painted {
    fn kind(&self) -> Option<ShellSlideKind> {
        match self {
            Self::Welcome(_) => Some(ShellSlideKind::WolWelcome),
            Self::Login(_) => Some(ShellSlideKind::WolLogin),
            Self::Lobby(_) => Some(ShellSlideKind::WolLobby),
            Self::Backdrop => None,
        }
    }

    fn title_key(&self) -> Option<&'static str> {
        match self {
            Self::Welcome(_) => Some(WOL_WELCOME_PAGE.title_key),
            Self::Login(_) => Some(crate::ui::wol_login::PAGE.title_key),
            Self::Lobby(_) => Some(WOL_LOBBY_TITLE_KEY),
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
        WolPage::Backdrop { .. } | WolPage::Message { .. } => Painted::Backdrop,
    };
    // The right panel's geometry is the family's on every WOL page.
    let page: &MenuPageLayout = match &painted {
        Painted::Login(layout) => &layout.page,
        Painted::Lobby(layout) => &layout.page,
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
    let monitor_frame = if page_statics {
        crate::app::frontend::menu_page_render::paint_shell_monitor(state)
    } else {
        None
    };
    let (title_window, status_label) = if page_statics {
        let title_window = state
            .frontend
            .shell_page_title
            .paint(std::time::Instant::now());
        let (status_key, status_rect) = match &painted {
            Painted::Welcome(layout) => {
                (crate::app::App::wol_status_key(state), layout.status_help)
            }
            Painted::Lobby(layout) => (None, layout.page.status_help),
            Painted::Login(layout) => (
                state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.login())
                    .and_then(|login| login.buttons.hovered)
                    .and_then(crate::ui::wol_login::tooltip_key),
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
        Painted::Backdrop => None,
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
    sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, page.warning_monitor, frame, scale)
    }));

    // Children of the page: blank while the teardown slide runs and gone
    // once the page closed; the entry slide leaves the left side painted.
    let children = page_statics;
    let mut icon_sprites = Vec::new();
    let mut labels = Vec::new();
    if let (true, Painted::Welcome(layout)) = (children, &painted) {
        for window in layout.group_boxes {
            push_group_box(&mut sprites, chrome, window, scale);
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
        for (text, rect) in &layout.texts {
            let caption = resolve_csf(state, text.key);
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
                lobby.ui.caret_visible(std::time::Instant::now()),
            ),
        )
    });

    // The column: the slide engine draws it while a slide runs, the empty
    // backdrop draws it shut, otherwise the page's buttons paint.
    let controller = &state.frontend.shell_controller;
    let mut painted_buttons: Vec<(MenuPageButtonRect, bool, bool)> = Vec::new();
    let mut spec: Option<&MenuPageSpec> = None;
    let column = match (&painted, &wave) {
        (_, Some(wave)) => Some(wave.button_draws()),
        (Painted::Backdrop, None) => Some(shell_paint::shuttered_column(page.right_panel)),
        (Painted::Welcome(layout), None) => {
            spec = Some(&WOL_WELCOME_PAGE);
            let active = controller.top_id() == Some(WOL_WELCOME_PAGE.dialog);
            let pressed = active.then(|| controller.pressed()).flatten();
            painted_buttons = layout
                .painted_buttons(pressed)
                .into_iter()
                .map(|button| (button, pressed == Some(button.id), true))
                .collect();
            None
        }
        (Painted::Login(layout), None) => {
            spec = Some(&crate::ui::wol_login::PAGE);
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
        (Painted::Lobby(layout), None) => {
            spec = Some(&crate::ui::wol_lobby::PAGE);
            if let Some(lobby) = lobby {
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
        std::time::Instant::now(),
        None,
    );
    for (button, pressed, enabled) in &painted_buttons {
        let Some(spec) = spec.and_then(|spec| spec.button(button.id)) else {
            continue;
        };
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.csf_key),
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
        // Text draws after every sprite: a box over the shown lobby hides
        // the glyphs under it here.
        if let Some(dialog) = box_dialog {
            for draw in &mut lobby.texts {
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
        text.extend(lobby.texts);
    }
    if let (Some(atlas), Some(popup)) = (controls_atlas, lobby_popup) {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: popup.controls,
        });
        text.extend(popup.texts);
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
