//! Render glue for VERA's Mods page (`ui::mods_shell`): the movie list's
//! parts (`movies_credits_render`) and a second box under the list for what
//! the selected mod says of itself.

use std::borrow::Cow;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, encode_family_page, family_art_ready, page_panel_state, page_tree,
};
use crate::app::frontend::movies_credits_render::{family_list_hovered, paint_family_list};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::mods::Listed;
use crate::render::shell_paint::{PaintLabel, SHELL_TEXT_RGB_ENABLED};
use crate::ui::main_menu_shell::RectPx;
use crate::ui::mods_shell::{self, ModsLayout, PAGE};
use crate::ui::movies_credits_shell::family_list_selected;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::menu_page;

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

/// Hover help for the page's status line.
fn mods_status_key(state: &AppState) -> Option<&'static str> {
    let controller = &state.frontend.shell_controller;
    if controller.top_id() != Some(PAGE.dialog) {
        return None;
    }
    if family_list_hovered(state, PAGE.dialog) {
        return Some(mods_shell::PROMPT_KEY);
    }
    PAGE.button(menu_page::hovered_id(controller, PAGE.dialog)?)
        .map(|button| button.tooltip_key)
}

/// Paint the Mods page. Returns `false` when the shell chrome is missing.
pub(crate) fn render_mods_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if !family_art_ready(state) {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::mods_layout(state);
    let (scale, dialog) = (layout.page.scale, PAGE.dialog);
    // Use This Mod is dark while the selection is nothing to change to, Check
    // Files while it is the original game.
    let mods = &state.frontend.mods;
    let selected = family_list_selected(&state.frontend.shell_controller, dialog);
    let enabled = [
        (mods_shell::USE, mods.switch_to(selected).is_some()),
        (mods_shell::CHECK, mods.picked_is_mod(selected)),
    ];
    let rect_of = |id| layout.page.button_rect(id);
    let tree = page_tree(state, dialog, || PAGE.nodes(), rect_of, &enabled);
    let status = |state: &AppState| mods_status_key(state).map(str::to_owned);
    let kind = ShellSlideKind::Mods;
    let panel = page_panel_state(state, kind, true, status, layout.page.status_help);
    // As for the movie list: the teardown slide repaints the dialog over its
    // children, and nothing draws them again before it is gone.
    let leaving = panel.leaving;
    let mut frame = FamilyFrame::new("Mods Shell", &layout.page, panel, PAGE.title_key)
        .on(state, dialog, &tree);
    frame.backdrop = true;
    let mods = &state.frontend.mods;
    if let Some(page) = mods.page.as_ref().filter(|_| !leaving)
        && let Some((sprites, geometry, view)) =
            paint_family_list(state, dialog, layout.list, scale)
    {
        frame.over.controls.extend(sprites);
        frame.list_boxes.push((layout.list, geometry.scrollbar));
        frame.list_boxes.push((layout.details, None));
        if let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() {
            let details = &mut frame.over.controls;
            crate::app::frontend::skirmish_shell_render::paint_list_frame(
                details,
                atlas,
                layout.details,
            );
        }
        let labels = &mut frame.labels;
        for (visible, index) in (view.top..page.len())
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
            let listed = &page[index];
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
        // An empty Mods folder: where a mod goes, across the empty rows under No
        // mod, one row down.
        if let Some(hint) = crate::app::mods::none_installed_hint(page) {
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
        if let Some(row) = view.selected.and_then(|selected| page.get(selected)) {
            labels.extend(detail_labels(row, mods.in_use(row), &layout));
        }
        labels.push(label(
            state.csf_text(mods_shell::PROMPT_KEY),
            layout.prompt,
            ShellAlign::H_CENTER,
        ));
    }
    encode_family_page(state, encoder, destination, frame);
    Ok(true)
}
