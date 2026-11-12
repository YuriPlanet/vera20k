//! Render glue for VERA's Mods page (`ui::mods_shell`): the movie list's
//! parts (`movies_credits_render`) and a second box under the list for what
//! the selected mod says of itself.

use std::borrow::Cow;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::movies_credits_render::{
    FamilyListRows, family_backdrop, push_family_backdrop, push_family_list,
};
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::mods::Listed;
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::MainMenuShellChromeAtlas;
use crate::render::shell_paint::{
    self, CURSOR_DEPTH, PaintButton, PaintLabel, SHELL_TEXT_RGB_DISABLED, SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text::ShellAlign;
use crate::ui::main_menu_shell::RectPx;
use crate::ui::mods_shell::{self, ModsLayout, PAGE};
use crate::ui::shell::list::ListMetrics;
use crate::ui::shell::static_reveal::Kind1RevealWindow;

/// Row text inset from the row's left edge, as in the movie list, and from
/// its right edge for the mark of the mod in use.
const ROW_TEXT_INSET: i32 = 2;

fn label<'a>(text: impl Into<Cow<'a, str>>, rect: RectPx, align: ShellAlign) -> PaintLabel<'a> {
    PaintLabel {
        text: text.into(),
        rect,
        align,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    }
}

/// What the box under the list says about `row`: its name, whether it is in
/// use or what it needs, who made it, what it is, and its terms.
fn detail_labels<'a>(row: &'a Listed, in_use: bool, layout: &ModsLayout) -> Vec<PaintLabel<'a>> {
    let state: Option<String> = if in_use {
        Some(crate::i18n::tr("VERA:ModInUse"))
    } else {
        row.needs
            .as_ref()
            .map(|needs| crate::i18n::tr_fill("VERA:ModNeeds", &[("needs", needs)]))
    };
    let author = row
        .author
        .as_ref()
        .map(|author| crate::i18n::tr_fill("VERA:ModAuthor", &[("author", author)]));
    let terms = row
        .terms
        .as_ref()
        .map(|terms| crate::i18n::tr_fill("VERA:ModTerms", &[("terms", terms)]));
    let lines: [(Option<Cow<'a, str>>, RectPx); 5] = [
        (Some(row.title()), layout.name),
        (state.map(Cow::Owned), layout.state),
        (author.map(Cow::Owned), layout.author),
        (row.about(), layout.about),
        (terms.map(Cow::Owned), layout.terms),
    ];
    lines
        .into_iter()
        .filter_map(|(text, rect)| Some(label(text?, rect, ShellAlign::NONE)))
        .collect()
}

/// Sprites and labels for the Mods page.
fn mods_composition<'a>(
    state: &'a AppState,
    atlas: &MainMenuShellChromeAtlas,
    layout: &ModsLayout,
    monitor_frame: Option<usize>,
    title_window: Option<Kind1RevealWindow>,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<PaintLabel<'a>>,
) {
    let screen_w = layout.page.screen.w;
    let backdrop = family_backdrop(atlas, screen_w, layout.page.screen.h, layout.page.scale);
    let mut sprites = Vec::new();
    push_family_backdrop(&mut sprites, &backdrop);
    sprites.extend(shell_paint::paint_chrome(
        atlas,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        screen_w,
    ));
    sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(
            atlas,
            layout.page.warning_monitor,
            frame,
            layout.page.scale,
        )
    }));

    let exit_wave =
        crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::Mods);
    // As for the movie list: the teardown slide repaints the dialog over its
    // children, and nothing draws them again before it is gone.
    let leaving = exit_wave.is_some();
    let mods = &state.frontend.mods;
    let mut labels = Vec::new();
    if let Some(page) = mods.page.as_ref().filter(|_| !leaving) {
        let geometry = page.geometry(layout.list, ListMetrics::scaled(layout.page.scale));
        push_family_list(
            &mut sprites,
            atlas,
            &backdrop,
            layout.list,
            Some(FamilyListRows {
                geometry: &geometry,
                top: page.top,
                selected: page.selected,
                pressed: page.scroll.pressed_part(),
            }),
        );
        push_family_list(&mut sprites, atlas, &backdrop, layout.details, None);
        for (visible, index) in (page.top..page.rows.len())
            .take(geometry.visible_rows)
            .enumerate()
        {
            let row = geometry.row(visible);
            let text = RectPx::new(
                row.x + ROW_TEXT_INSET,
                row.y,
                row.w - 2 * ROW_TEXT_INSET,
                row.h,
            );
            let listed = &page.rows[index];
            if mods.in_use(listed) {
                // The name keeps the row's left three quarters.
                let name_w = text.w * 3 / 4;
                labels.push(label(
                    listed.title(),
                    RectPx::new(text.x, text.y, name_w, text.h),
                    ShellAlign::NONE,
                ));
                labels.push(label(
                    crate::i18n::tr("VERA:ModInUse"),
                    RectPx::new(text.x + name_w, text.y, text.w - name_w, text.h),
                    ShellAlign::H_RIGHT,
                ));
            } else {
                labels.push(label(listed.title(), text, ShellAlign::NONE));
            }
        }
        // An empty Mods folder: where a mod goes, across the empty rows
        // under No mod, one row down.
        if let Some(hint) = crate::app::mods::none_installed_hint(&page.rows) {
            let content = geometry.content;
            let top = geometry.row(2).y;
            labels.push(label(
                hint,
                RectPx::new(
                    content.x + 4 * ROW_TEXT_INSET,
                    top,
                    content.w - 8 * ROW_TEXT_INSET,
                    (content.y + content.h - top).max(0),
                ),
                ShellAlign::NONE,
            ));
        }
        if let Some(row) = page.selected.and_then(|selected| page.rows.get(selected)) {
            labels.extend(detail_labels(row, mods.in_use(row), layout));
        }
        labels.push(label(
            resolve_csf(state, mods_shell::PROMPT_KEY),
            layout.prompt,
            ShellAlign::H_CENTER,
        ));
    }

    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(PAGE.dialog);
    let pressed = active.then(|| controller.pressed()).flatten();
    let hovered = active.then(|| controller.hovered()).flatten();
    // Use This Mod is dark while the selection is nothing to change to.
    let enabled = |id: u16| id != mods_shell::USE || mods.switch_to().is_some();
    let wave = exit_wave.or(state.frontend.shell_first_paint_slide.as_ref());
    // While a slide runs the engine draws the whole tile column in place of
    // the buttons (`0x006071E0`).
    let buttons: Vec<PaintButton> = match wave {
        Some(wave) => {
            sprites.extend(shell_paint::paint_slide_column(
                atlas,
                layout.page.right_panel,
                &wave.button_draws(),
            ));
            Vec::new()
        }
        None => layout
            .page
            .buttons
            .iter()
            .map(|button| PaintButton {
                rect: button.rect,
                pressed: enabled(button.id) && pressed == Some(button.id),
                hovered: enabled(button.id) && hovered == Some(button.id),
                enabled: enabled(button.id),
            })
            .collect(),
    };
    let button_sprites = shell_paint::paint_buttons(
        atlas,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        crate::util::time::Instant::now(),
        None,
    );
    // The slide engine draws the button frames only; captions return with
    // the first ordinary paint after the entry slide.
    for button in layout.page.buttons.iter().filter(|_| wave.is_none()) {
        let Some(spec) = PAGE.button(button.id) else {
            continue;
        };
        let down = enabled(button.id) && pressed == Some(button.id);
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.csf_key),
            rect: owner_draw_button_label_rect(button.rect, down),
            align: ShellAlign::BUTTON_CAPTION,
            rgb: if enabled(button.id) {
                SHELL_TEXT_RGB_ENABLED
            } else {
                SHELL_TEXT_RGB_DISABLED
            },
            path_a_reveal: None,
        });
    }
    if let Some(window) = title_window {
        labels.push(PaintLabel {
            text: resolve_csf(state, PAGE.title_key),
            rect: layout.page.title,
            align: ShellAlign::PANEL_TITLE,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(
                crate::app::frontend::main_menu_shell_render::shell_reveal_path_a(window),
            ),
        });
    }
    (sprites, button_sprites, labels)
}

/// Hover help for the page's status line.
fn mods_status_key(state: &AppState) -> Option<&'static str> {
    let controller = &state.frontend.shell_controller;
    if controller.top_id() != Some(PAGE.dialog) {
        return None;
    }
    match controller.hovered()? {
        mods_shell::LIST => Some(mods_shell::PROMPT_KEY),
        id => PAGE.button(id).map(|button| button.tooltip_key),
    }
}

/// Paint the Mods page. Returns `false` when the shell chrome is missing.
pub(crate) fn render_mods_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.main_menu_shell_chrome.is_none() {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::mods_layout(state);
    // As for the movie list: no statics and no monitor while the page leaves.
    let leaving =
        crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::Mods)
            .is_some();
    let monitor_frame = if leaving {
        None
    } else {
        crate::app::frontend::menu_page_render::paint_shell_monitor(state)
    };
    let (title_window, status_label) = if leaving {
        (None, None)
    } else {
        let title_window = state
            .frontend
            .shell_page_title
            .paint(crate::util::time::Instant::now());
        let status_text = mods_status_key(state)
            .map(|key| resolve_csf(state, key).into_owned())
            .unwrap_or_default();
        let status_label = crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            layout.page.status_help,
        );
        (title_window, status_label)
    };
    let Some(atlas) = state.frontend.main_menu_shell_chrome.as_ref() else {
        return Ok(false);
    };
    let (sprites, button_sprites, mut labels) =
        mods_composition(state, atlas, &layout, monitor_frame, title_window);
    labels.extend(status_label);
    let text = shell_paint::paint_labels_scaled(
        &state.renderer.bit_font,
        &labels,
        layout.page.scale.factor(),
    );
    let draws = [
        TexturedDraw {
            texture: &atlas.texture,
            instances: sprites,
        },
        TexturedDraw {
            texture: &atlas.texture,
            instances: button_sprites,
        },
    ];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Mods Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
            smooth_art: false,
        },
    );
    Ok(true)
}
