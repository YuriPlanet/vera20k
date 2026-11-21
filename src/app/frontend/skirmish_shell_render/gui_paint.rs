//! Paints a dialog's `vera20k-gui` widgets with the shell's owner-draw
//! emitters: one place that turns Button, Checkbox, Trackbar, Combo, List,
//! Edit and Text nodes into the sprites and texts each screen used to
//! assemble by hand. A screen keeps its backdrop and chrome and calls
//! [`paint_gui_children`] for the widgets it built under its dialog root.
//!
//! Each node paints in tree order, its sprites before its text, so a tree
//! in the order a screen drew lays its draws down in that order. A node's
//! own props choose its text: `Color` and `DisabledColor`, `Align`,
//! `TextInset`; the host's [`GuiStyle`] the buttons' art and the texts and
//! rows it keeps itself.

use vera20k_gui::{Gui, NodeId, NodeKind, Prop, Value};

use super::controls::{ControlPaint, dropdown_scroll_part, paint_control, push_popup_instances};
use super::text::{
    button_label_rect_px, combo_dropdown_text_fit_width, combo_face_text_color,
    combo_face_text_draw_rect, combo_face_text_fit_width, fit_owner_draw_label, rect_to_text_rect,
};
use super::{
    COMBODROPWIN_TEXT_INSET_X, SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH,
    SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_LABEL_TEXT_RGB, SHELL_MODAL_PANEL_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{self, ModalButton, ModalDepths};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::trackbar::{TrackbarRange, value_text_rect};

/// Sprites (skirmish atlas) and texts of a dialog's widgets, and of an open
/// combo drop-down over them.
#[derive(Default)]
pub(crate) struct GuiPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
    pub(crate) popup: Vec<SpriteInstance>,
    pub(crate) popup_texts: Vec<ShellTextDraw>,
}

/// A list's rows as the host keeps them: each row's text and packed
/// `00BBGGRR` colour (`u32::MAX` for the node's own).
pub(crate) type HostRows = Vec<(String, u32)>;

/// How a dialog's buttons paint.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonArt {
    /// Not at all: the host paints them, or a slide's column covers them.
    Host,
    /// The right panel's owner-draw type-1 buttons (`0x00612B70`, SDBTNANM),
    /// pushed while their press holds them (under the pointer or not), lit
    /// while `Toggled` (record `+0xC5`), with the type-1 caption, or one in
    /// their rect in their own `Align`.
    Panel,
    /// Owner-draw type-3 buttons (MNBTTN), pushed under the pointer, with
    /// their caption centred.
    Modal,
    /// A framed plate with its caption in its `Align` (VERA's Extras rows).
    Plate,
}

/// How a dialog's combos paint their face.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComboArt {
    /// The owner-draw face, arrow and caption.
    Plain,
    /// [`Self::Plain`] in the owner-draw two-pixel bevel (`0x105`'s).
    Framed,
    /// No face: the host paints it (VERA's roster cells); only an open
    /// list paints.
    Host,
}

/// What a dialog's widgets take from their host rather than their props.
#[derive(Clone, Copy)]
pub(crate) struct GuiStyle<'a> {
    /// A focused edit shows the open IME composition.
    pub(crate) ime: bool,
    /// The buttons' art and caption.
    pub(crate) buttons: ButtonArt,
    /// A button's own art by its `Id`, in place of `buttons`.
    pub(crate) art_of: Option<&'a dyn Fn(i32) -> Option<ButtonArt>>,
    /// The host's text for a node by its `Id` (a status, a name), in place
    /// of its `Label` or `Text`.
    pub(crate) text_of: Option<&'a dyn Fn(i32) -> Option<String>>,
    /// The rows of a host-counted (`RowCount`) list by its `Id`; a list
    /// without paints nothing, its host paints it. A host-counted combo's
    /// open list shows them too.
    pub(crate) rows_of: Option<&'a dyn Fn(i32) -> Option<HostRows>>,
    /// The host's enable for a node by its `Id`, in place of its `Enabled`
    /// (a button the host greys while it still takes presses).
    pub(crate) enabled_of: Option<&'a dyn Fn(i32) -> Option<bool>>,
    /// Lists paint as the shell dialogs' own ([`super::controls::list_view_controls`]:
    /// frame, selection, the control scrollbar) rather than as the WOL
    /// pages' over their art ([`super::list::paint_list`]).
    pub(crate) control_lists: bool,
    /// The combos' faces.
    pub(crate) combos: ComboArt,
    /// An edit's caret shows (it has the focus, in its blink) by the host's
    /// word, for a tree the shell controller does not own.
    pub(crate) caret_of: Option<&'a dyn Fn(NodeId) -> bool>,
    /// The glyph scale of every caption, value and row, when not the
    /// `scale` the widgets paint at (VERA's room's smaller text).
    pub(crate) text_factor: Option<f32>,
    /// The colour of a text with no `Color` (VERA's room's green).
    pub(crate) rgb: [f32; 3],
}

impl Default for GuiStyle<'_> {
    fn default() -> Self {
        GuiStyle {
            ime: true,
            buttons: ButtonArt::Host,
            art_of: None,
            text_of: None,
            rows_of: None,
            enabled_of: None,
            control_lists: false,
            combos: ComboArt::Plain,
            caret_of: None,
            text_factor: None,
            rgb: SHELL_LABEL_TEXT_RGB,
        }
    }
}

impl GuiStyle<'_> {
    /// The WOL and online boxes': MNBTTN buttons, the shell dialogs'
    /// lists, edits without the IME composition.
    pub(crate) fn popup() -> Self {
        GuiStyle {
            ime: false,
            buttons: ButtonArt::Modal,
            control_lists: true,
            ..Default::default()
        }
    }
}

/// A node's caption: `Label` is a string key (shown as the key when
/// missing), `Text` a literal.
fn label_of(state: &AppState, gui: &Gui, node: NodeId) -> Option<String> {
    match gui.get(node, Prop::Label).ok().flatten() {
        Some(Value::Text(key)) => Some(state.csf_label(key, key)),
        _ => match gui.get(node, Prop::Text).ok().flatten() {
            Some(Value::Text(text)) => Some(text.clone()),
            _ => None,
        },
    }
}

fn int(gui: &Gui, node: NodeId, prop: Prop, default: i32) -> i32 {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Int(v)) => *v,
        _ => default,
    }
}

fn flag(gui: &Gui, node: NodeId, prop: Prop, default: bool) -> bool {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Bool(v)) => *v,
        _ => default,
    }
}

fn color(gui: &Gui, node: NodeId, prop: Prop) -> Option<[f32; 3]> {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Rgba(c)) => Some([c.0, c.1, c.2].map(|c| f32::from(c) / 255.0)),
        _ => None,
    }
}

/// The node's text colour: `Color` (else `normal`), or while disabled its
/// `DisabledColor`, else its kind's `disabled` colour when it has one.
fn text_rgb(
    gui: &Gui,
    node: NodeId,
    enabled: bool,
    normal: [f32; 3],
    disabled: Option<[f32; 3]>,
) -> [f32; 3] {
    let off = (!enabled)
        .then(|| color(gui, node, Prop::DisabledColor).or(disabled))
        .flatten();
    off.or_else(|| color(gui, node, Prop::Color))
        .unwrap_or(normal)
}

/// `rect` less the node's `TextInset` (base pixels, grown with the page).
fn inset(gui: &Gui, node: NodeId, rect: RectPx, scale: ShellScale) -> Option<RectPx> {
    match gui.get(node, Prop::TextInset).ok().flatten() {
        Some(Value::Rect(i)) => Some(scale.derive_rect(rect, |r| {
            RectPx::new(r.x + i.x, r.y + i.y, r.w - i.x - i.w, r.h - i.y - i.h)
        })),
        _ => None,
    }
}

fn text(
    state: &AppState,
    value: &str,
    rect: RectPx,
    align: ShellAlign,
    factor: f32,
    rgb: [f32; 3],
    depth: f32,
) -> ShellTextDraw {
    shell_text::draw_in_rect_scaled(
        &state.renderer.bit_font,
        value,
        rect_to_text_rect(rect),
        rgb,
        align,
        [0.0; 2],
        depth,
        factor,
    )
}

fn items(gui: &Gui, node: NodeId, prop: Prop) -> &[String] {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Items(items)) => items,
        _ => &[],
    }
}

/// An `Edit` node as the NewEdit its paint reads: text, caret, the first
/// shown unit the host keeps in `Top` ([`crate::app::shell_wol::leaf`]
/// scrolls it), the password mask and whether it holds the focus.
pub(crate) fn new_edit(
    gui: &Gui,
    node: NodeId,
) -> crate::ui::skirmish_shell::SavedSeedDescriptionEdit {
    let text = match gui.get(node, Prop::Text).ok().flatten() {
        Some(Value::Text(text)) => text.as_str(),
        _ => "",
    };
    let chars = text.chars().count();
    let caret_chars = usize::try_from(int(gui, node, Prop::Caret, chars as i32))
        .unwrap_or(0)
        .min(chars);
    let caret = text.chars().take(caret_chars).map(char::len_utf16).sum();
    let units: Vec<u16> = text.encode_utf16().collect();
    crate::ui::skirmish_shell::SavedSeedDescriptionEdit {
        first_visible_unit: usize::try_from(int(gui, node, Prop::Top, 0))
            .unwrap_or(0)
            .min(units.len()),
        units,
        caret,
        focused: gui.focus() == Some(node),
        password: flag(gui, node, Prop::Password, false),
    }
}

/// The widgets under `root`, in tree order. Invisible subtrees are skipped.
pub(crate) fn paint_gui_children(
    state: &AppState,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
) -> GuiPaint {
    paint_gui_children_under(state, gui, root, scale, &[])
}

/// [`paint_gui_children`] under `overlays` that paint with the widgets'
/// sprites but before their text (an open list in a box): the captions of
/// the nodes an overlay meets are left out, so they don't show through it.
pub(crate) fn paint_gui_children_under(
    state: &AppState,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
) -> GuiPaint {
    paint_gui_styled(state, gui, root, scale, overlays, GuiStyle::default())
}

/// [`paint_gui_children_under`] in the host's `style`.
pub(crate) fn paint_gui_styled(
    state: &AppState,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
    style: GuiStyle<'_>,
) -> GuiPaint {
    match state.frontend.skirmish_shell_chrome.as_ref() {
        Some(atlas) => paint_gui_with(state, atlas, gui, root, scale, overlays, style),
        None => GuiPaint::default(),
    }
}

/// The widgets of `ids` under `root` in that order rather than the tree's
/// (a screen whose paint order is not its hit order), each with its own
/// subtree.
pub(crate) fn paint_gui_ids(
    state: &AppState,
    gui: &Gui,
    root: NodeId,
    ids: impl IntoIterator<Item = u16>,
    scale: ShellScale,
    style: GuiStyle<'_>,
) -> GuiPaint {
    let mut paint = GuiPaint::default();
    for node in ids
        .into_iter()
        .filter_map(|id| gui.find_id(root, id.into()))
    {
        paint.extend(paint_gui_styled(state, gui, node, scale, &[], style));
    }
    paint
}

impl GuiPaint {
    /// Appends `other`'s draws after this one's.
    pub(crate) fn extend(&mut self, other: GuiPaint) {
        self.controls.extend(other.controls);
        self.texts.extend(other.texts);
        self.popup.extend(other.popup);
        self.popup_texts.extend(other.popup_texts);
    }
}

/// [`paint_gui_styled`] on `atlas`, for a renderer that holds the shell
/// chrome while it paints (the Skirmish family's).
pub(crate) fn paint_gui_with(
    state: &AppState,
    atlas: &SkirmishShellChromeAtlas,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
    style: GuiStyle<'_>,
) -> GuiPaint {
    let mut paint = GuiPaint::default();
    let chrome = atlas.control_chrome();
    // Shell passes draw in queue order (no depth test).
    let depth = SHELL_CONTROL_TEXT_DEPTH;
    let glyph = style.text_factor.unwrap_or(scale.factor());
    // The pushed button of the dialog `root` belongs to.
    let mut dialog_root = root;
    while let Ok(Some(parent)) = gui.tree().parent(dialog_root) {
        dialog_root = parent;
    }
    let held = crate::ui::shell::page::held_in(gui, dialog_root);
    let captured = crate::ui::shell::page::captured_in(gui, dialog_root);
    let id = |node| gui.id_of(node);
    let caption = |node| match style
        .text_of
        .zip(id(node))
        .and_then(|(text_of, id)| text_of(id))
    {
        Some(text) => Some(text),
        None => label_of(state, gui, node),
    };
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !flag(gui, node, Prop::Visible, true) {
            continue;
        }
        if let Ok(children) = gui.tree().children(node) {
            stack.extend(children.iter().rev());
        }
        let Ok(kind) = gui.tree().kind(node) else {
            continue;
        };
        let Ok(rect) = gui.tree().screen_rect(node) else {
            continue;
        };
        let covered = super::text_covered_by_overlay(rect, overlays);
        let enabled = match style
            .enabled_of
            .zip(id(node))
            .and_then(|(enabled_of, id)| enabled_of(id))
        {
            Some(enabled) => enabled,
            None => flag(gui, node, Prop::Enabled, true),
        };
        let align = ShellAlign(int(gui, node, Prop::Align, ShellAlign::V_CENTER.0 as i32) as u8);
        match kind {
            NodeKind::Button => {
                let art = style
                    .art_of
                    .zip(id(node))
                    .and_then(|(art_of, id)| art_of(id))
                    .unwrap_or(style.buttons);
                let pressed = match art {
                    ButtonArt::Modal => held,
                    _ => captured,
                } == Some(node);
                match art {
                    ButtonArt::Host => continue,
                    ButtonArt::Panel => super::chrome::push_right_panel_button_shp(
                        &mut paint.controls,
                        atlas,
                        rect,
                        crate::ui::shell::button::ButtonVisual {
                            pressed,
                            highlighted: flag(gui, node, Prop::Toggled, false),
                        },
                        !enabled,
                        SHELL_CONTROL_DEPTH,
                    ),
                    ButtonArt::Modal => {
                        let button = ModalButton {
                            rect,
                            pressed,
                            enabled,
                        };
                        let depths = ModalDepths {
                            background: SHELL_CONTROL_DEPTH,
                            button: SHELL_CONTROL_DEPTH,
                            text: depth,
                        };
                        paint.controls.extend(shell_paint::paint_modal_sprites(
                            None,
                            super::type3_button_frames(atlas),
                            rect,
                            &[button],
                            scale,
                            depths,
                        ));
                    }
                    ButtonArt::Plate => {
                        let out = &mut paint.controls;
                        super::chrome::push_solid_rect(
                            out,
                            atlas,
                            rect,
                            SHELL_MODAL_PANEL_RGB,
                            SHELL_CONTROL_DEPTH,
                        );
                        super::chrome::push_ownerdraw_two_pixel_bevel_frame(
                            out,
                            atlas,
                            rect,
                            SHELL_CONTROL_DEPTH,
                        );
                    }
                }
                let Some(label) = caption(node).filter(|t| !covered && !t.is_empty()) else {
                    continue;
                };
                let rgb = text_rgb(
                    gui,
                    node,
                    enabled,
                    style.rgb,
                    Some(shell_paint::SHELL_TEXT_RGB_DISABLED),
                );
                let (at, align) = match (art, gui.get(node, Prop::Align).ok().flatten()) {
                    (ButtonArt::Modal, _) => (
                        crate::app::frontend::shell_pass::owner_draw_button_label_rect(
                            rect, pressed,
                        ),
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                    ),
                    (_, Some(_)) => (rect, align),
                    (_, None) => (
                        button_label_rect_px(rect, pressed, scale),
                        ShellAlign::BUTTON_CAPTION,
                    ),
                };
                paint
                    .texts
                    .push(text(state, &label, at, align, glyph, rgb, depth));
            }
            NodeKind::Checkbox => {
                let checked = flag(gui, node, Prop::Checked, false);
                paint_control(
                    &mut paint.controls,
                    &chrome,
                    ControlPaint::Checkbox { rect, checked },
                    scale,
                );
                if let Some(label) = caption(node).filter(|_| !covered) {
                    let at = crate::ui::skirmish_shell::checkbox_text_rect(rect, scale);
                    let rgb = text_rgb(gui, node, enabled, style.rgb, None);
                    paint
                        .texts
                        .push(text(state, &label, at, align, glyph, rgb, depth));
                }
            }
            NodeKind::Trackbar => {
                // The plaque and its value text unless the owner turned them
                // off (`0x4AC` with 0), which gives the track the whole window.
                let plaque = flag(gui, node, Prop::ShowValue, true);
                let range = TrackbarRange::new(
                    int(gui, node, Prop::Min, 0),
                    int(gui, node, Prop::Max, 100),
                    int(gui, node, Prop::Step, 1),
                )
                .with_plaque(plaque);
                let value = int(gui, node, Prop::Pos, range.min);
                paint_control(
                    &mut paint.controls,
                    &chrome,
                    ControlPaint::Trackbar {
                        rect,
                        thumb_left: range.thumb_left_in(value, rect, scale),
                        plaque,
                        disabled: !enabled,
                    },
                    scale,
                );
                if plaque && !covered {
                    // A disabled trackbar's value text takes [0xAC1CB4] (`0x0061E2B1`).
                    let rgb = text_rgb(
                        gui,
                        node,
                        enabled,
                        style.rgb,
                        Some(SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F),
                    );
                    // A host value text (its own or `Text`) stands in for the
                    // position.
                    let shown = match (
                        style
                            .text_of
                            .zip(id(node))
                            .and_then(|(text_of, id)| text_of(id)),
                        gui.get(node, Prop::Text).ok().flatten(),
                    ) {
                        (Some(shown), _) => shown,
                        (None, Some(Value::Text(shown))) => shown.clone(),
                        _ => range.stepped(value).to_string(),
                    };
                    let at = value_text_rect(rect, scale);
                    paint.texts.push(text(
                        state,
                        &shown,
                        at,
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                        glyph,
                        rgb,
                        depth,
                    ));
                }
            }
            NodeKind::Combo => {
                let host_rows = gui
                    .get(node, Prop::RowCount)
                    .ok()
                    .flatten()
                    .and(style.rows_of)
                    .zip(id(node))
                    .and_then(|(rows_of, id)| rows_of(id));
                if style.combos == ComboArt::Framed {
                    let out = &mut paint.controls;
                    super::chrome::push_ownerdraw_two_pixel_bevel_frame(
                        out,
                        atlas,
                        rect,
                        SHELL_CONTROL_DEPTH,
                    );
                }
                paint_combo(
                    &mut paint, state, atlas, gui, node, rect, enabled, scale, style, host_rows,
                );
            }
            NodeKind::List => {
                // A host-counted list's rows are the host's, or it paints it.
                let host_rows = match gui.get(node, Prop::RowCount).ok().flatten() {
                    Some(_) => match style
                        .rows_of
                        .zip(id(node))
                        .and_then(|(rows_of, id)| rows_of(id))
                    {
                        Some(rows) => Some(rows),
                        None => continue,
                    },
                    None => None,
                };
                // The owner-draw list `0x00618D40` over its dialog's art.
                let Some(view) = gui.list_view(node) else {
                    continue;
                };
                let mut metrics = crate::ui::shell::list::ListMetrics::scaled(scale);
                metrics.row_h = int(gui, node, Prop::RowHeight, metrics.row_h);
                let geometry = view.geometry(rect, metrics);
                let selected = view
                    .selected
                    .filter(|_| !flag(gui, node, Prop::NoSelectionFill, false));
                match style.control_lists {
                    true => super::controls::list_view_controls(
                        &mut paint.controls,
                        atlas,
                        geometry,
                        (selected, view.top, view.pressed),
                        scale,
                    ),
                    false => super::list::paint_list(
                        &mut paint.controls,
                        atlas,
                        geometry,
                        view.top,
                        selected,
                        view.pressed,
                        false,
                        scale,
                    ),
                }
                let normal = text_rgb(gui, node, true, style.rgb, None);
                let rows: Vec<(&str, u32)> = match &host_rows {
                    Some(rows) => rows.iter().map(|(row, rgb)| (row.as_str(), *rgb)).collect(),
                    None => items(gui, node, Prop::Items)
                        .iter()
                        .map(|row| (row.as_str(), u32::MAX))
                        .collect(),
                };
                let shown = rows.into_iter().skip(view.top).take(geometry.visible_rows);
                for (visible, (row, packed)) in shown.enumerate().filter(|_| !covered) {
                    let r = geometry.row(visible);
                    let at = inset(gui, node, r, scale).unwrap_or(RectPx::new(
                        r.x + 2,
                        r.y,
                        r.w - 2,
                        r.h,
                    ));
                    let rgb = match packed {
                        u32::MAX => normal,
                        c => {
                            [c as u8, (c >> 8) as u8, (c >> 16) as u8].map(|c| f32::from(c) / 255.0)
                        }
                    };
                    paint
                        .texts
                        .push(text(state, row, at, align, glyph, rgb, depth));
                }
            }
            // NewEdit `0x00614B30` as `push_new_edit` paints it; the open IME
            // composition shows in the focused one.
            NodeKind::Edit => {
                let mut edit = new_edit(gui, node);
                let composition = state
                    .platform
                    .ime
                    .open_composition()
                    .filter(|_| style.ime && edit.focused);
                let caret = match style.caret_of {
                    Some(caret_of) => caret_of(node),
                    None => state.frontend.shell_controller.caret_visible(node),
                };
                edit.focused |= style.caret_of.is_some() && caret;
                super::controls::push_new_edit(
                    &mut paint.controls,
                    &mut paint.texts,
                    atlas,
                    &state.renderer.bit_font,
                    &edit,
                    composition,
                    rect,
                    caret,
                    text_rgb(gui, node, true, style.rgb, None),
                    depth,
                    scale,
                );
            }
            NodeKind::Text => {
                if let Some(label) = caption(node).filter(|t| !covered && !t.is_empty()) {
                    let at = inset(gui, node, rect, scale).unwrap_or(rect);
                    let rgb = text_rgb(gui, node, enabled, style.rgb, None);
                    paint
                        .texts
                        .push(text(state, &label, at, align, glyph, rgb, depth));
                }
            }
            _ => {}
        }
    }
    paint
}

/// An owner-draw combo's face, and its drop-down while it is open, its
/// rows in the style's colour and glyph scale.
#[allow(clippy::too_many_arguments)]
fn paint_combo(
    paint: &mut GuiPaint,
    state: &AppState,
    atlas: &SkirmishShellChromeAtlas,
    gui: &Gui,
    node: NodeId,
    rect: RectPx,
    enabled: bool,
    scale: ShellScale,
    style: GuiStyle<'_>,
    host_rows: Option<HostRows>,
) {
    let drop_down = gui.combo_drop_down(node);
    let selected = usize::try_from(int(gui, node, Prop::Selected, -1)).ok();
    // A colour row shows its swatch (`house_color_tint`) and no text.
    let swatches = items(gui, node, Prop::Swatches);
    let swatch = |row: usize| {
        let index = swatches.get(row)?.parse().ok()?;
        Some(super::controls::house_color_tint(state.rules(), index))
    };
    let face_swatch = selected.and_then(swatch);
    let open = drop_down.is_some();
    let items = items(gui, node, Prop::Items);
    // A host-counted combo's rows are the host's.
    let rows: Vec<&str> = match &host_rows {
        Some(rows) => rows.iter().map(|(row, _)| row.as_str()).collect(),
        None => items.iter().map(String::as_str).collect(),
    };
    let font = &state.renderer.bit_font;
    if style.combos != ComboArt::Host {
        paint_control(
            &mut paint.controls,
            &atlas.control_chrome(),
            ControlPaint::Combo {
                rect,
                swatch: face_swatch,
                open,
                disabled: !enabled,
            },
            scale,
        );
        // The host's caption, or `Text`, stands in for the selected row's.
        let host = (style.text_of.zip(gui.id_of(node))).and_then(|(text_of, id)| text_of(id));
        let face = match (&host, gui.get(node, Prop::Text).ok().flatten()) {
            (Some(face), _) | (None, Some(Value::Text(face))) => Some(face.as_str()),
            _ => selected.and_then(|row| items.get(row)).map(String::as_str),
        };
        let rgb = text_rgb(
            gui,
            node,
            enabled,
            SHELL_LABEL_TEXT_RGB,
            Some(combo_face_text_color(true)),
        );
        if let Some(face) = face {
            // Cut to gamemd's arrow reserve (`0x00617250`), squeezed first.
            let at = combo_face_text_draw_rect(rect, scale);
            fit_owner_draw_label(font, face, combo_face_text_fit_width(rect, scale), |face| {
                paint.texts.push(text(
                    state,
                    face,
                    at,
                    ShellAlign::V_CENTER,
                    scale.factor(),
                    rgb,
                    SHELL_CONTROL_TEXT_DEPTH,
                ));
            });
        }
    }
    let Some(drop_down) = drop_down else { return };
    let popup = drop_down.popup;
    push_popup_instances(
        &mut paint.popup,
        atlas,
        popup.rect,
        drop_down.hot_row.and_then(|row| popup.row_rect(row)),
        (popup.first..popup.first + popup.visible_rows).filter_map(|row| {
            let at = scale.derive_rect(popup.row_rect(row)?, |r| {
                RectPx::new(r.x + 2, r.y + 2, r.w - 4, r.h - 4)
            });
            Some((at, swatch(row)?))
        }),
        popup.scrollbar().zip(popup.thumb()),
        drop_down.pressed_part.map(dropdown_scroll_part),
        scale,
        SHELL_DROPDOWN_DEPTH - 0.00002,
    );
    // The rows fit the list in their own glyph size.
    let glyph = style.text_factor.unwrap_or(scale.factor());
    let fit = combo_dropdown_text_fit_width(popup.content(), scale) as f32 * scale.factor() / glyph;
    for row in popup.first..popup.first + popup.visible_rows {
        let (Some(at), Some(item)) = (popup.row_rect(row), rows.get(row).copied()) else {
            continue;
        };
        // `ComboDropWin` draws from x+3, the caption cut to the row less the
        // scrollbar reserve (`0x0060DF38`).
        let inset = COMBODROPWIN_TEXT_INSET_X;
        let at = scale.derive_rect(at, |r| RectPx::new(r.x + inset, r.y, r.w - 2 * inset, r.h));
        fit_owner_draw_label(font, item, fit.round() as u32, |item| {
            paint.popup_texts.push(text(
                state,
                item,
                at,
                ShellAlign::V_CENTER,
                glyph,
                style.rgb,
                SHELL_DROPDOWN_TEXT_DEPTH,
            ));
        });
    }
}
