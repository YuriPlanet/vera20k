//! Render glue for VERA's Mods page (`ui::mods_shell`): the movie list's
//! parts (`movies_credits_render`) and a second box under the list for what
//! the selected mod says of itself.

use std::borrow::Cow;

use anyhow::Result;

use crate::app::frontend::menu_page_render::{
    FamilyFrame, encode_family_page, family_art_ready, page_panel_state, page_tree,
};
use crate::app::frontend::movies_credits_render::family_list_hovered;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::skirmish_shell_render::gui_paint::{CellRow, GuiStyle, paint_gui_into};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::mods::Listed;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::hud_paint::Material;
use crate::render::shell_paint::SHELL_TEXT_RGB_ENABLED;
use crate::ui::mods_shell::{self, PAGE};
use crate::ui::movies_credits_shell::{family_list_selected, find_family_list};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::menu_page;
use vera20k_gui::{NodeId, Prop, Value};

/// Row text inset from the row's left edge, as in the movie list, and from
/// its right edge for the mark of the mod in use.
const ROW_TEXT_INSET: i32 = 2;

/// What the box under the list says about `row`, a line each: its name,
/// whether it is in use or what it needs, who made it, what it is, and its
/// terms ([`mods_shell::DETAIL_TEXTS`]).
fn detail_lines(row: &Listed, in_use: bool) -> [Option<String>; 5] {
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
    [
        Some(row.title().into_owned()),
        state,
        author,
        row.about().map(Cow::into_owned),
        terms,
    ]
}

/// The page's statics take their texts, written only on a change: the
/// empty folder's hint and the selected row's lines, or none.
fn sync_texts(frontend: &mut FrontendState, selected: Option<usize>) {
    let dialog = PAGE.dialog;
    let Some(root) = frontend.shell_controller.root_in(dialog) else {
        return;
    };
    let mods = &frontend.mods;
    // The hint, then the five lines.
    let mut texts: [Option<String>; 6] = Default::default();
    if let Some(page) = mods.page.as_ref() {
        texts[0] = crate::app::mods::none_installed_hint(page);
        if let Some(row) = selected.and_then(|selected| page.get(selected)) {
            let lines = detail_lines(row, mods.in_use(row));
            for (text, line) in texts[1..].iter_mut().zip(lines) {
                *text = line;
            }
        }
    }
    let gui = frontend.shell_controller.gui_mut();
    let count = gui.tree().children(root).map_or(0, <[NodeId]>::len);
    for index in 0..count {
        let Some(child) = gui
            .tree()
            .children(root)
            .ok()
            .and_then(|c| c.get(index).copied())
        else {
            break;
        };
        let id = gui.id_of(child).and_then(|id| u16::try_from(id).ok());
        let slot = match id {
            Some(mods_shell::HINT_TEXT) => 0,
            Some(id) => match mods_shell::DETAIL_TEXTS.iter().position(|text| *text == id) {
                Some(line) => line + 1,
                None => continue,
            },
            None => continue,
        };
        let text = Value::Text(texts[slot].take().unwrap_or_default());
        crate::ui::shell::page::set_if_changed(gui, child, Prop::Text, text);
    }
}

/// Hover help for the page's status line.
fn mods_status_key(shell_controller: &DialogController) -> Option<&'static str> {
    let controller = shell_controller;
    if controller.top_id() != Some(PAGE.dialog) {
        return None;
    }
    if family_list_hovered(shell_controller, PAGE.dialog) {
        return Some(mods_shell::PROMPT_KEY);
    }
    PAGE.button(menu_page::hovered_id(controller, PAGE.dialog)?)
        .map(|button| button.tooltip_key)
}

/// Paint the Mods page. Returns `false` when the shell chrome is missing.
pub(crate) fn render_mods_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if !family_art_ready(
        renderer,
        frontend,
        process_assets,
        &persistence.options_profile,
    ) {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::mods_layout(&renderer.gpu);
    let (scale, dialog) = (layout.page.scale, PAGE.dialog);
    // Use This Mod is dark while the selection is nothing to change to, Check
    // Files while it is the original game.
    let mods = &frontend.mods;
    let selected = family_list_selected(&frontend.shell_controller, dialog);
    let enabled = [
        (mods_shell::USE, mods.switch_to(selected).is_some()),
        (mods_shell::CHECK, mods.picked_is_mod(selected)),
    ];
    let rect_of = |id| layout.rect_of(id);
    let tree = page_tree(
        &mut frontend.shell_controller,
        dialog,
        mods_shell::nodes,
        rect_of,
        &enabled,
    );
    // Android: the name dialog's widgets; the dialog itself is drawn after
    // the page, in `App::draw_name_dialog_over`.
    #[cfg(target_os = "android")]
    crate::app::App::sync_name_dialog(&renderer.gpu, frontend);
    sync_texts(frontend, selected);
    let status =
        |frontend: &FrontendState| mods_status_key(&frontend.shell_controller).map(str::to_owned);
    let kind = ShellSlideKind::Mods;
    let panel = page_panel_state(
        frontend,
        process_assets,
        kind,
        true,
        status,
        layout.page.status_help,
    );
    // As for the movie list: the teardown slide repaints the dialog over its
    // children, and nothing draws them again before it is gone.
    let leaving = panel.leaving;
    let mut frame = FamilyFrame::new("Mods Shell", &layout.page, panel, PAGE.title_key).on(
        &frontend.shell_controller,
        dialog,
        &tree,
    );
    frame.backdrop = true;
    // Android: the name dialog is painted over the finished page, so the
    // page's pass must not draw the cursor under the dialog's art; the
    // dialog's own pass draws it last.
    #[cfg(target_os = "android")]
    {
        frame.cursor = frontend.mods.name_dialog.is_none();
    }
    let mods = &frontend.mods;
    let controller = &frontend.shell_controller;
    let gui = controller.gui();
    if let Some(page) = mods.page.as_ref().filter(|_| !leaving)
        && let Some(node) = find_family_list(controller, dialog)
        && let (Some(geometry), Some(view)) = (gui.list_geometry(node), gui.list_view(node))
    {
        // The list over its widget, its rows the mods' names, the one in use
        // marked at the row's right: only the rows it shows are named. The
        // row's text keeps two pixels from either end.
        let shown = view.top..view.top + geometry.visible_rows;
        let text_w = geometry.row(0).w - 2 * ROW_TEXT_INSET;
        let in_use = crate::i18n::tr("VERA:ModInUse");
        let cells_of = |id: i32| {
            (id == mods_shell::LIST_ID).then(|| {
                let rows = page.iter().enumerate();
                rows.map(|(index, listed)| {
                    let mut row = CellRow {
                        window: true,
                        ..CellRow::default()
                    };
                    if !shown.contains(&index) {
                        return row;
                    }
                    let (x, rgb) = (ROW_TEXT_INSET, SHELL_TEXT_RGB_ENABLED);
                    if mods.in_use(listed) {
                        // The name keeps the row's left three quarters.
                        let name_w = text_w * 3 / 4;
                        row.text(x, name_w, &listed.title(), ShellAlign::NONE, rgb);
                        let rest = text_w - name_w;
                        row.text(x + name_w, rest, &in_use, ShellAlign::H_RIGHT, rgb);
                    } else {
                        row.text(x, text_w, &listed.title(), ShellAlign::NONE, rgb);
                    }
                    row
                })
                .collect()
            })
        };
        let style = GuiStyle {
            cells_of: Some(&cells_of),
            ..Default::default()
        };
        // Without the control art none of it paints.
        let atlas = frontend.skirmish_shell_chrome.as_ref();
        if let Some(atlas) = atlas {
            let over = &mut frame.over;
            paint_gui_into(
                over,
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                Some(atlas),
                gui,
                node,
                scale,
                &[],
                style,
            );
        }
        frame.list_boxes.push((layout.list, geometry.scrollbar));
        frame.list_boxes.push((layout.details, None));
        if let Some(atlas) = atlas {
            frame.over.draw(Material::Controls, |details, _| {
                crate::app::frontend::skirmish_shell_render::paint_list_frame(
                    details,
                    atlas,
                    layout.details,
                )
            });
        }
        // The hint, the selected mod's lines and the caption, from their
        // statics.
        if let (Some((gui, root)), Some(atlas)) = (frame.buttons, atlas) {
            let (over, style) = (&mut frame.over, GuiStyle::default());
            paint_gui_into(
                over,
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                Some(atlas),
                gui,
                root,
                scale,
                &[],
                style,
            );
        }
    }
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
    // Android: New Mod's dialog goes over the finished page, as a message box
    // would, so the page's own text cannot cover it.
    #[cfg(target_os = "android")]
    crate::app::App::draw_name_dialog_over(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        destination,
    );
    Ok(true)
}
