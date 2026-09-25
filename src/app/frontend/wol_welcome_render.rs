//! Paint Main Menu -> Internet: the Westwood Online welcome `0x10E` as a
//! family page, and afterwards the `TXT_APIMISSING` box `0xD0` over the empty
//! shell backdrop.
//!
//! `0x10E` (paint mode 1, `0x00621FB1..0x00621FFE`) draws the right panel,
//! then its own background MultiplaySelection.shp at the dialog origin
//! (`0x0072E730`; loaded only at 800 wide by `0x0072C7E0`). Its children:
//! the family heading, status line and monitor, the owner-draw buttons, two
//! group boxes (`0x0061E700`), kind-0 text statics and kind-2 icon statics.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::app::frontend::movies_credits_render::push_group_box;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_paint::{
    self, CHROME_DEPTH, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_ENABLED, push_entry_native,
};
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::centred_in_window;
use crate::ui::wol_shell::{StaticAlign, WOL_WELCOME_PAGE};

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

/// Paint `0x10E` or, once a WOL action failed, the `0xD0` box. Returns
/// `false` when the shell chrome is missing.
pub(crate) fn render_wol_welcome_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.main_menu_shell_chrome.is_none() || state.frontend.wol_welcome.is_none() {
        return Ok(false);
    }
    crate::app::App::ensure_wol_welcome_art(state);
    let layout = crate::app::App::wol_welcome_layout(state);
    let screen_w = layout.page.screen.w;
    let screen_h = layout.page.screen.h;
    let api_missing = state
        .frontend
        .wol_welcome
        .as_ref()
        .and_then(|wol| wol.api_missing.clone());

    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: every child stays blank.
    let exit_wave =
        crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::WolWelcome)
            .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| {
        api_missing
            .is_none()
            .then(|| state.frontend.shell_first_paint_slide.clone())
            .flatten()
    });
    let page_statics = api_missing.is_none() && !leaving;
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
        let status_text = crate::app::App::wol_status_key(state)
            .map(|key| resolve_csf(state, key).into_owned())
            .unwrap_or_default();
        let status_label = crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            layout.status_help,
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
    let art = state.frontend.wol_welcome_art.as_ref();
    let mut sprites = Vec::new();
    // The dialog's own background exists only at 800 wide; elsewhere, and
    // behind the box, the empty shell backdrop shows (`0x0052FEC0`).
    let dialog_background = art
        .and_then(|art| art.background)
        .filter(|_| api_missing.is_none());
    let mut background_sprites = Vec::new();
    match dialog_background {
        Some(entry) => push_entry_native(
            &mut background_sprites,
            entry,
            0,
            0,
            PARENT_BACKGROUND_DEPTH,
        ),
        None => sprites.extend(
            crate::app::frontend::main_menu_shell_render::shell_parent_background_instances(
                chrome, screen_w, screen_h,
            ),
        ),
    }
    sprites.extend(shell_paint::paint_chrome(
        chrome,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        screen_w,
    ));
    sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.page.warning_monitor, frame)
    }));

    // Children of the page: blank while the teardown slide runs and gone
    // once the box shows; the entry slide leaves the left side painted. The
    // icons draw after the backdrop, wherever it came from.
    let children = page_statics;
    let mut icon_sprites = Vec::new();
    if children {
        for window in layout.group_boxes {
            push_group_box(&mut sprites, chrome, window);
        }
        if let Some(art) = art {
            for (file, window) in &layout.icons {
                let Some((_, entry)) = art.icons.iter().find(|(name, _)| name == file) else {
                    continue;
                };
                let (w, h) = entry.native_size();
                let (x, y) = centred_in_window(*window, w, h);
                push_entry_native(&mut icon_sprites, *entry, x, y, CHILD_DEPTH);
            }
        }
    }

    // The column: the slide engine draws it while a slide runs, the empty
    // backdrop draws it shut, otherwise the buttons paint.
    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(WOL_WELCOME_PAGE.dialog);
    let pressed = active.then(|| controller.pressed()).flatten();
    let column = if api_missing.is_some() {
        Some(shell_paint::shuttered_column(layout.page.right_panel))
    } else {
        wave.as_ref().map(|wave| wave.button_draws())
    };
    let painted = match column {
        Some(draws) => {
            sprites.extend(shell_paint::paint_slide_column(
                chrome,
                layout.page.right_panel,
                &draws,
            ));
            Vec::new()
        }
        None => layout.painted_buttons(pressed),
    };
    let buttons: Vec<PaintButton> = painted
        .iter()
        .map(|button| PaintButton {
            rect: button.rect,
            pressed: pressed == Some(button.id),
            hovered: false,
            enabled: true,
        })
        .collect();
    let button_sprites = shell_paint::paint_buttons(
        chrome,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        std::time::Instant::now(),
        None,
    );

    let mut labels = Vec::new();
    if children {
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
    for button in &painted {
        let Some(spec) = WOL_WELCOME_PAGE.button(button.id) else {
            continue;
        };
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.csf_key),
            rect: owner_draw_button_label_rect(button.rect, pressed == Some(button.id)),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    if let Some(window) = title_window {
        labels.push(PaintLabel {
            text: resolve_csf(state, WOL_WELCOME_PAGE.title_key),
            rect: layout.page.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    labels.extend(status_label);
    let mut text = shell_paint::paint_labels(&state.renderer.bit_font, &labels);

    // The TXT_APIMISSING box `0xD0`: PUDLGBGN with one MNBTTN OK.
    let mut box_sprites = Vec::new();
    let box_texture = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .filter(|_| api_missing.is_some());
    if let (Some(message), Some(atlas)) = (api_missing.as_ref(), box_texture) {
        let geometry = crate::ui::shell::modal::body_ok_layout(screen_w, screen_h);
        let ok_pressed =
            state.frontend.shell_controller.pressed() == Some(crate::ui::shell::modal::control::OK);
        let box_labels = [
            PaintLabel {
                text: message.body.as_str().into(),
                rect: geometry.body,
                align: ShellAlign::NONE,
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            },
            PaintLabel {
                text: message.ok.as_str().into(),
                rect: owner_draw_button_label_rect(geometry.ok, ok_pressed),
                align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            },
        ];
        let draw = shell_paint::paint_modal_shp(
            &state.renderer.bit_font,
            atlas.validation_modal_background_pudlgbgn,
            crate::app::frontend::skirmish_shell_render::type3_button_frames(atlas),
            geometry.dialog,
            &[shell_paint::ModalButton {
                rect: geometry.ok,
                pressed: ok_pressed,
                enabled: true,
            }],
            &box_labels,
            BOX_DEPTHS,
        );
        box_sprites = draw.sprites;
        text.extend(draw.text);
    }

    // Draw order is paint order: the dialog background, the backdrop and
    // chrome, the icons, then the buttons.
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
    if let Some(atlas) = box_texture {
        draws.push(TexturedDraw {
            texture: &atlas.texture,
            instances: box_sprites,
        });
    }
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Westwood Online Welcome",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
        },
    );
    Ok(true)
}
