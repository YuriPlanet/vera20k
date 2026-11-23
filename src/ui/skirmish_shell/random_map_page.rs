//! The random-map dialog `0x105`'s controls as `vera20k-gui` widgets: its
//! five combos and Players slider, VERA's Advanced sliders and Extras rows,
//! and every button. The widgets own the interaction (capture, the open
//! list, the pushed look); the options stay the dialog's
//! ([`RandomMapSetupModalState`]) and each sync shows them. VERA's Advanced
//! scrollbar, which pages the slider windows, is a `ScrollBar` widget.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Rgba, Value};

use super::layout::{
    AdvancedField, RandomMapSetupControl as C, RandomMapSetupLayout, RectPx, checkbox_text_rect,
    combo_face_rect, random_map_setup_dropdown_rect,
};
use super::page::{Pointer, slider_held};
use super::state::{RandomMapSetupModalState, SetupCombo, status_help_key_for_random_map_setup};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// The dialog in the shell controller.
pub const DIALOG: DialogResourceId = DialogResourceId(0x0105);

/// Every fixed control by id, in hit order: VERA's right-column buttons, the
/// option rows, then the action row and the column. VERA's ids are its own.
const CONTROLS: [(u16, C); 18] = [
    (0x7401, C::AdvancedToggle),
    (0x7402, C::ExportMap),
    (0x7403, C::FairStartsToggle),
    (0x7404, C::PresetCycle),
    (0x405, C::MapType0x405),
    (0x3EA, C::Time0x3ea),
    (0x407, C::Theater0x407),
    (0x406, C::Size0x406),
    (0x408, C::Resources0x408),
    (0x3EB, C::Players0x3eb),
    (0x621, C::Randomize0x621),
    (0x620, C::Generate0x620),
    (0x6C5, C::Ok0x6c5),
    (0x6C2, C::Load0x6c2),
    (0x6C3, C::Save0x6c3),
    (0x6C4, C::Delete0x6c4),
    (0x5C0, C::Cancel0x5c0),
    (5000, C::UrbanAreas5000),
];
/// The seven reusable slider windows: an Advanced slider of the page on
/// show, or an Extras row.
const SLIDER: u16 = 0x7410;
const EXTRAS: u16 = 0x7420;
/// The Advanced scrollbar: a page a position, its thumb sized over the
/// Advanced rows past the first page's.
const SCROLLBAR: u16 = 0x7430;
/// The option rows' labels: the six original rows', an Advanced slider's
/// or an Extras row's.
const LABEL: u16 = 0x7440;
/// Ares' Urban Areas caption, beside its box like `0x102`'s.
pub const URBAN_CAPTION: u16 = 0x7450;
/// The progress text `0x6C6`'s place.
const PROGRESS: u16 = 0x7451;
/// The preview static, which hovers with no help: a press on it enlarges
/// the map (`preview_zoom`).
const PREVIEW: u16 = 0x468;
/// The right column's owner-draw type-1 buttons, which a slide's column
/// art covers.
const COLUMN: [u16; 9] = [
    0x6C5, 0x6C2, 0x6C3, 0x6C4, 0x5C0, 0x7401, 0x7402, 0x7403, 0x7404,
];

const YELLOW: Value = Value::Rgba(Rgba(0xFF, 0xFF, 0, 0xFF));
/// The disabled owner-draw caption ([`crate::render::shell_paint::SHELL_TEXT_RGB_DISABLED`]).
const DISABLED: Value = Value::Rgba(Rgba(0x48, 0, 0, 0xFF));

const ROWS: u16 = AdvancedField::PAGE_SIZE as u16;

/// The control widget `id` stands for now.
fn control_of(modal: &RandomMapSetupModalState, id: u16) -> Option<C> {
    if let Some(row) = id.checked_sub(SLIDER).filter(|row| *row < ROWS) {
        return advanced_field(modal, usize::from(row)).map(C::Advanced);
    }
    if let Some(row) = id.checked_sub(EXTRAS).filter(|row| *row < ROWS) {
        return Some(C::ExtrasRow(usize::from(row)));
    }
    if id == SCROLLBAR {
        return Some(C::AdvancedScrollbar);
    }
    CONTROLS
        .iter()
        .find(|(c, _)| *c == id)
        .map(|(_, control)| *control)
}

/// The Advanced slider of the page on show in window `row`.
fn advanced_field(modal: &RandomMapSetupModalState, row: usize) -> Option<AdvancedField> {
    AdvancedField::ALL
        .into_iter()
        .find(|field| field.page() == modal.advanced_page && field.page_row() == row)
}

/// The widgets in hit order, then the statics that show text only.
fn spec() -> NodeSpec {
    let centred = Value::Int((ShellAlign::H_CENTER | ShellAlign::V_CENTER).0.into());
    let rows = |base: u16, kind, colour: &Value| {
        let colour = colour.clone();
        (0..ROWS).map(move |row| {
            NodeSpec::new(kind)
                .id((base + row).into())
                .with(Prop::DisabledColor, colour.clone())
        })
    };
    // The faces, the Players value and an Extras row's value keep the label
    // colour; the captions of a control that is off take the disabled one.
    let fixed = |range: std::ops::Range<usize>| {
        CONTROLS[range].iter().map(|(id, control)| {
            let spec = |kind| NodeSpec::new(kind).id((*id).into());
            match control {
                C::Players0x3eb => spec(NodeKind::Trackbar)
                    .with(Prop::DisabledColor, YELLOW)
                    .with(Prop::RehoverOnRelease, Value::Bool(true)),
                control if SetupCombo::from_control(*control).is_some() => {
                    spec(NodeKind::Combo).with(Prop::DisabledColor, YELLOW)
                }
                // Owner-draw type 3: the caption centred in the button.
                C::Randomize0x621 | C::Generate0x620 => {
                    spec(NodeKind::Button).with(Prop::Align, centred.clone())
                }
                _ => spec(NodeKind::Button),
            }
        })
    };
    let text = |id: u16| NodeSpec::new(NodeKind::Text).id(id.into());
    let scrollbar = NodeSpec::new(NodeKind::ScrollBar)
        .id(SCROLLBAR.into())
        .with(Prop::Max, Value::Int(AdvancedField::PAGE_COUNT as i32 - 1))
        .with(Prop::RowCount, Value::Int(AdvancedField::ALL.len() as i32))
        .with(Prop::VisibleRows, Value::Int(AdvancedField::PAGE_SIZE as i32));
    NodeSpec::new(NodeKind::Dialog)
        .children(fixed(0..4))
        .child(scrollbar)
        .children(
            rows(SLIDER, NodeKind::Trackbar, &DISABLED)
                .map(|row| row.with(Prop::RehoverOnRelease, Value::Bool(true))),
        )
        .children(
            rows(EXTRAS, NodeKind::Button, &YELLOW)
                .map(|row| row.with(Prop::Align, centred.clone())),
        )
        .children(fixed(4..CONTROLS.len()))
        .children(rows(LABEL, NodeKind::Text, &DISABLED))
        .child(text(URBAN_CAPTION))
        .child(text(PROGRESS))
        .child(NodeSpec::new(NodeKind::Text).id(PREVIEW.into()).tooltip(""))
}

/// Widget `id`'s rect in the view on show; none while it does not show.
/// VERA: the Advanced and Extras views replace the option rows, and Ares'
/// Urban Areas box under them.
fn rect(
    modal: &RandomMapSetupModalState,
    layout: &RandomMapSetupLayout,
    id: u16,
) -> Option<RectPx> {
    let (basic, extras) = (!modal.advanced, modal.extras_page.is_some());
    if let Some(row) = id
        .checked_sub(LABEL)
        .filter(|row| *row < ROWS)
        .map(usize::from)
    {
        return match (basic, extras) {
            (true, _) => layout.label_rects.get(row).copied(),
            (false, true) => modal
                .extras_row(row)
                .map(|_| layout.advanced_label_rects[row]),
            (false, false) => advanced_field(modal, row).map(|_| layout.advanced_label_rects[row]),
        };
    }
    match id {
        URBAN_CAPTION => {
            return (basic && modal.ares)
                .then(|| checkbox_text_rect(layout.rect(5000), layout.scale));
        }
        PROGRESS => return Some(layout.rect(0x6C6)),
        PREVIEW => return Some(layout.preview),
        _ => {}
    }
    let control = control_of(modal, id)?;
    Some(match control {
        C::Advanced(field) if !basic && !extras => layout.advanced_control_rects[field.page_row()],
        C::ExtrasRow(row) if extras => layout.advanced_control_rects[row],
        C::AdvancedScrollbar if !basic && !extras => AdvancedField::advanced_scrollbar_rect(layout),
        C::Players0x3eb if basic => layout.control_rects[5],
        C::UrbanAreas5000 if basic && modal.ares => layout.rect(5000),
        // A closed combo is its face; its list hangs from it.
        _ if basic && SetupCombo::from_control(control).is_some() => combo_face_rect(
            layout.control_rects[SetupCombo::from_control(control)?.row()],
            layout.scale,
        ),
        C::Players0x3eb
        | C::UrbanAreas5000
        | C::Advanced(_)
        | C::ExtrasRow(_)
        | C::AdvancedScrollbar => return None,
        // The buttons stand in the layout's table by id.
        _ if SetupCombo::from_control(control).is_some() => return None,
        _ => return layout.rects.get(id),
    })
}

/// The control's status help (`0x006040B0`); an Extras row's is its entry's.
fn help(modal: &RandomMapSetupModalState, control: C) -> &'static str {
    match control {
        C::ExtrasRow(row) => modal
            .extras_row(row)
            .map_or("", super::state::ExtrasRow::help),
        control => status_help_key_for_random_map_setup(control),
    }
}

/// The Advanced scrollbar as its widget paints: the bar, and its thumb at
/// the page showing.
pub fn scrollbar_view(controller: &DialogController) -> Option<vera20k_gui::ScrollBarView> {
    controller.gui().scrollbar_view(controller.control(DIALOG, SCROLLBAR)?)
}

/// Builds the widgets under the controller's `0x105` (a fresh instance when
/// another dialog is on top), puts them on `layout` and shows the options.
pub fn sync_page(
    controller: &mut DialogController,
    modal: &RandomMapSetupModalState,
    layout: &RandomMapSetupLayout,
) -> Option<NodeId> {
    controller.ensure_active(DIALOG);
    controller.set_scale(layout.scale);
    let root = controller.root_in(DIALOG)?;
    let gui = controller.gui_mut();
    page::ensure_built(gui, root, spec);
    page::sync_rects(gui, root, |id| rect(modal, layout, id));
    for id in CONTROLS
        .iter()
        .map(|(id, _)| *id)
        .chain(SLIDER..SCROLLBAR + 1)
    {
        let Some(control) = control_of(modal, id) else {
            continue;
        };
        let mut set = |prop, value| page::set(gui, root, id, prop, value);
        set(Prop::Enabled, Value::Bool(modal.is_enabled(control)));
        set(Prop::Tooltip, Value::Text(help(modal, control).to_owned()));
        let (range, pos) = match control {
            C::Players0x3eb => (modal.players_range, modal.options.num_players),
            C::Advanced(field) => (super::state::ADVANCED_RANGE, modal.advanced_value(field)),
            C::AdvancedScrollbar => {
                set(Prop::Pos, Value::Int(modal.advanced_page as i32));
                continue;
            }
            _ => {
                if let Some(combo) = SetupCombo::from_control(control) {
                    let count = modal.combo_items(combo).len();
                    let selected = modal
                        .selected_item_index(combo)
                        .map_or(-1, |row| row as i32);
                    set(Prop::RowCount, Value::Int(count as i32));
                    set(Prop::Selected, Value::Int(selected));
                    let list = random_map_setup_dropdown_rect(layout, combo.row(), count);
                    set(Prop::PopupRect, Value::Rect(list));
                }
                continue;
            }
        };
        for (prop, value) in [
            (Prop::Min, range.min),
            (Prop::Max, range.max),
            (Prop::Step, range.step),
            (Prop::Pos, pos),
        ] {
            set(prop, Value::Int(value));
        }
    }
    Some(root)
}

/// Puts the dialog's words on its widgets, each string key and fallback
/// through `label`: the row labels (the six original rows' are `SS_LEFT`,
/// unlike Choose Map's centred headings), the faces, the values, the
/// captions and the progress text. A face whose value has no entry (map
/// type 0, which the list omits) stays blank, as the original's
/// match-the-entry selection leaves it. While a slide runs (`sliding`) the
/// column art covers the right column's captions.
pub fn show_texts(
    controller: &mut DialogController,
    modal: &RandomMapSetupModalState,
    sliding: bool,
    label: &dyn Fn(&str, &str) -> String,
) {
    use super::state::{ExtrasValue, RandomMapPreset};
    let Some(root) = controller.root_in(DIALOG) else {
        return;
    };
    let gui = controller.gui_mut();
    let key = |(key, fallback): (&str, &str)| label(key, fallback);
    // The 0x405 label reads "Environment" though the control writes the map
    // type.
    const BASIC: [(&str, &str); 6] = [
        ("GUI:Environment", "Environment"),
        ("GUI:TimeOfDay", "Time of Day"),
        ("GUI:Theater", "Theater"),
        ("GUI:MapSize", "Map Size"),
        ("GUI:Resources", "Resources"),
        ("GUI:Players", "Players"),
    ];
    let mut texts: Vec<(u16, String)> = Vec::new();
    for row in 0..usize::from(ROWS) {
        let id = |base: u16| base + row as u16;
        let (caption, enabled) = match (modal.advanced, modal.extras_page) {
            (false, _) => (BASIC.get(row).map(|l| key(*l)), true),
            (true, Some(_)) => (modal.extras_row(row).map(|entry| key(entry.label())), true),
            (true, None) => match advanced_field(modal, row) {
                Some(field) => (
                    Some(key(field.label())),
                    modal.is_enabled(C::Advanced(field)),
                ),
                None => (None, true),
            },
        };
        page::set(gui, root, id(LABEL), Prop::Enabled, Value::Bool(enabled));
        texts.push((id(LABEL), caption.unwrap_or_default()));
        // VERA: an Extras row shows its value or tool in its face.
        let value = modal
            .extras_row(row)
            .map(|entry| match modal.extras_value(entry) {
                ExtrasValue::Label(k, f) => label(k, f),
                ExtrasValue::Number(number) => number.to_string(),
                ExtrasValue::Page(page, pages) => format!("{page} / {pages}"),
            });
        texts.push((id(EXTRAS), value.unwrap_or_default()));
    }
    for (id, control) in CONTROLS {
        if let Some(combo) = SetupCombo::from_control(control) {
            let items = modal.combo_items(combo);
            let face = modal
                .selected_item_index(combo)
                .map(|row| label(items[row].key, items[row].fallback));
            texts.push((id, face.unwrap_or_default()));
        }
    }
    // The column's captions; its toggle names the view its press shows next.
    let toggle = match (modal.extras_page, modal.advanced) {
        (Some(_), _) => ("VERA:RandomMapBasic", "Basic"),
        (None, true) => ("VERA:RandomMapExtras", "Extras"),
        (None, false) => ("VERA:RandomMapAdvanced", "Advanced"),
    };
    let export = match modal.exported {
        true => ("VERA:RandomMapExported", "Map Exported"),
        false => ("VERA:RandomMapExport", "Export Map"),
    };
    let fair = match modal.options.fair_starts {
        true => ("VERA:RandomMapFairStartsOn", "Fair Starts: On"),
        false => ("VERA:RandomMapFairStartsOff", "Fair Starts: Off"),
    };
    // One of the player's own presets is named by its number.
    let preset = match modal.preset {
        Some(RandomMapPreset::Mine(slot)) => key(RandomMapPreset::Mine(slot).label())
            .replace("{number}", &(u32::from(slot) + 1).to_string()),
        preset => key(preset.map_or(("VERA:RandomMapPresets", "Presets"), RandomMapPreset::label)),
    };
    for (id, caption) in [
        (0x621, key(("GUI:SurpriseMe", "Surprise Me"))),
        (0x620, key(("GUI:PreviewMap", "Preview Map"))),
        (0x6C5, key(("GUI:UseMap", "Use Map"))),
        (0x6C2, key(("GUI:LoadMap", "Load Map"))),
        (0x6C3, key(("GUI:SaveMap", "Save Map"))),
        (0x6C4, key(("GUI:DeleteMap", "Delete Map"))),
        (0x5C0, key(("GUI:Cancel", "Cancel"))),
        (0x7401, key(toggle)),
        (0x7402, key(export)),
        (0x7403, key(fair)),
        (0x7404, preset),
        (URBAN_CAPTION, key(("GUI:UrbanAreas", "Urban Areas"))),
        (PROGRESS, progress_text(modal, label).unwrap_or_default()),
    ] {
        let covered = sliding && COLUMN.contains(&id);
        texts.push((id, if covered { String::new() } else { caption }));
    }
    for (id, text) in texts {
        page::set(gui, root, id, Prop::Text, Value::Text(text));
    }
}

/// The progress text's place: the generate block's text, else VERA's word
/// on where Export Map wrote the file (not the game folder), what the last
/// Extras tool did, or how evenly the map on show treats its starts.
fn progress_text(
    modal: &RandomMapSetupModalState,
    label: &dyn Fn(&str, &str) -> String,
) -> Option<String> {
    if modal.generating {
        return Some(label("GUI:WorkingPleaseWait", "Working, please wait..."));
    }
    if let Some(path) = modal.exported_to.as_ref().filter(|_| modal.exported) {
        return Some(label("VERA:RandomMapExportedTo", "Saved to {path}").replace("{path}", path));
    }
    if let Some((key, fallback)) = modal.extras_note {
        return Some(label(key, fallback));
    }
    let report = modal.fairness.filter(|_| modal.generated)?;
    let readout = report.readout();
    let text = label(
        "VERA:RandomMapFairness",
        "Fairness: ore {ore}%, 2nd field {expansion}%, room {room}%, spacing {spacing}%",
    )
    .replace("{ore}", &readout.ore.to_string())
    .replace("{expansion}", &readout.expansion.to_string())
    .replace("{room}", &readout.room.to_string())
    .replace("{spacing}", &readout.spacing.to_string());
    // The water near the starts joins the line with Sea Access on.
    Some(match readout.sea.filter(|_| report.sea_wanted) {
        Some(sea) => {
            text + &label("VERA:RandomMapFairnessSea", ", sea {sea}%")
                .replace("{sea}", &sea.to_string())
        }
        None => text,
    })
}

/// What the widget events do to the dialog: the cues to play and the button
/// the user activated. The combos and sliders act at once; the buttons on
/// release.
fn handle(
    modal: &mut RandomMapSetupModalState,
    events: Vec<PageEvent>,
    sounds: &mut Vec<PageCue>,
) -> Option<C> {
    let mut activated = None;
    for event in events {
        match event {
            PageEvent::Cue(cue) => sounds.push(cue),
            PageEvent::Activated(id) => activated = control_of(modal, id),
            PageEvent::Picked(id, row) => {
                let Some(combo) = control_of(modal, id).and_then(SetupCombo::from_control) else {
                    continue;
                };
                if let Some(item) = usize::try_from(row)
                    .ok()
                    .and_then(|row| modal.combo_items(combo).get(row))
                {
                    modal.set_combo_value(combo, item.value);
                }
            }
            // A slider clicks (GenericClick, `0x0061E6DD`) on each change of
            // its position (`0x0061E609`), since `0x105` never turns its cue
            // off (`0x4AE`).
            PageEvent::Slid {
                id,
                value,
                final_: false,
            } => {
                let changed = match control_of(modal, id) {
                    Some(C::Players0x3eb) => modal.change_num_players(value),
                    Some(C::Advanced(field)) => modal.change_advanced(field, value),
                    Some(C::AdvancedScrollbar) => {
                        modal.set_advanced_scroll(usize::try_from(value).unwrap_or(0))
                    }
                    _ => false,
                };
                if changed {
                    sounds.push(PageCue::ListClick);
                }
            }
            _ => {}
        }
    }
    activated
}

/// The pointer at `x`, `y` on the dialog: the cues it plays and the button
/// it activated. VERA's Advanced scrollbar acts on press like the sliders:
/// an arrow steps a page, the thumb or track takes the page under the
/// pointer and holds the mouse until the release, each new page clicking.
pub fn pointer(
    controller: &mut DialogController,
    modal: &mut RandomMapSetupModalState,
    layout: &RandomMapSetupLayout,
    at: Pointer,
    x: i32,
    y: i32,
) -> (Vec<PageCue>, Option<C>) {
    let mut sounds = Vec::new();
    if sync_page(controller, modal, layout).is_none() {
        return (sounds, None);
    }
    let events = super::page::route(controller, at, x, y);
    let activated = handle(modal, events, &mut sounds);
    (sounds, activated)
}

/// The dialog's widgets while they are the controller's top dialog.
fn shown(controller: &DialogController) -> Option<&Gui> {
    (controller.top_id() == Some(DIALOG)).then(|| controller.gui())
}

/// Whether something holds the mouse: an open list (`ComboDropWin`
/// `0x0060E0F1`), a slider since a press on it, or the Advanced scrollbar.
/// The status line then keeps its text.
pub fn holding(controller: &DialogController, _modal: &RandomMapSetupModalState) -> bool {
    shown(controller).is_some_and(|gui| {
        let scrollbar = gui
            .pressed()
            .is_some_and(|node| gui.tree().kind(node) == Ok(NodeKind::ScrollBar));
        gui.popup().is_some() || slider_held(gui) || scrollbar
    })
}

/// What the paint reads of the widgets: the combo whose list is open and
/// the button that shows pushed (captured, under the pointer or not).
pub fn view(
    controller: &DialogController,
    modal: &RandomMapSetupModalState,
) -> (Option<SetupCombo>, Option<C>) {
    let Some(gui) = shown(controller) else {
        return (None, None);
    };
    let control = |node: NodeId| control_of(modal, u16::try_from(gui.id_of(node)?).ok()?);
    let open = gui
        .popup()
        .and_then(control)
        .and_then(SetupCombo::from_control);
    (open, gui.pressed().and_then(control))
}

/// The control the pointer at `x`, `y` hovers on a fresh dialog in the
/// given view.
#[cfg(test)]
pub(crate) fn control_at(
    layout: &RandomMapSetupLayout,
    x: i32,
    y: i32,
    advanced: bool,
    extras: bool,
    ares: bool,
    advanced_page: usize,
) -> Option<C> {
    let options = crate::map::rmg::options::RmgOptions {
        seed: 1,
        ..Default::default()
    };
    let mut modal = RandomMapSetupModalState::open(options, None, false);
    (modal.advanced, modal.extras_page) = (advanced, extras.then_some(0));
    (modal.ares, modal.advanced_page) = (ares, advanced_page);
    let mut controller = DialogController::default();
    sync_page(&mut controller, &modal, layout)?;
    controller.on_pointer_move(x, y);
    let gui = controller.gui();
    control_of(&modal, u16::try_from(gui.id_of(gui.hovered()?)?).ok()?)
}
