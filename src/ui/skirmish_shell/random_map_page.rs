//! The random-map dialog `0x105`'s controls as `vera20k-gui` widgets: its
//! five combos and Players slider, VERA's Advanced sliders and Extras rows,
//! and every button. The widgets own the interaction (capture, the open
//! list, the pushed look); the options stay the dialog's
//! ([`RandomMapSetupModalState`]) and each sync shows them. Only VERA's
//! Advanced scrollbar keeps its own press, as it pages the slider windows.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use super::layout::{
    AdvancedField, RandomMapSetupControl as C, RandomMapSetupLayout, RectPx, combo_face_rect,
    random_map_setup_dropdown_rect,
};
use super::page::{Pointer, slider_held};
use super::state::{
    DropdownScrollbarPart, RandomMapSetupModalState, SetupCombo, SkirmishShellUiSound,
    status_help_key_for_random_map_setup,
};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// The dialog in the shell controller.
pub const DIALOG: DialogId = DialogId(0x0105);

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
/// The Advanced scrollbar, a static here: it carries its status help.
const SCROLLBAR: u16 = 0x7430;

const ROWS: u16 = AdvancedField::PAGE_SIZE as u16;

/// The control widget `id` stands for now.
fn control_of(modal: &RandomMapSetupModalState, id: u16) -> Option<C> {
    if let Some(row) = id.checked_sub(SLIDER).filter(|row| *row < ROWS) {
        let field = AdvancedField::ALL.into_iter().find(|field| {
            field.page() == modal.advanced_page && field.page_row() == usize::from(row)
        })?;
        return Some(C::Advanced(field));
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

/// The widgets in hit order.
fn spec() -> NodeSpec {
    let rows =
        |base: u16, kind| (0..ROWS).map(move |row| NodeSpec::new(kind).id((base + row).into()));
    let kind = |control| match control {
        C::Players0x3eb => NodeKind::Trackbar,
        control if SetupCombo::from_control(control).is_some() => NodeKind::Combo,
        _ => NodeKind::Button,
    };
    let fixed = |range: std::ops::Range<usize>| {
        CONTROLS[range]
            .iter()
            .map(move |(id, control)| NodeSpec::new(kind(*control)).id((*id).into()))
    };
    NodeSpec::new(NodeKind::Dialog)
        .children(fixed(0..4))
        .child(NodeSpec::new(NodeKind::Text).id(SCROLLBAR.into()))
        .children(rows(SLIDER, NodeKind::Trackbar))
        .children(rows(EXTRAS, NodeKind::Button))
        .children(fixed(4..CONTROLS.len()))
}

/// Widget `id`'s rect in the view on show; none while it does not show.
/// VERA: the Advanced and Extras views replace the option rows, and Ares'
/// Urban Areas box under them.
fn rect(
    modal: &RandomMapSetupModalState,
    layout: &RandomMapSetupLayout,
    id: u16,
) -> Option<RectPx> {
    let control = control_of(modal, id)?;
    let (basic, extras) = (!modal.advanced, modal.extras_page.is_some());
    Some(match control {
        C::Advanced(field) if !basic && !extras => layout.advanced_control_rects[field.page_row()],
        C::ExtrasRow(row) if extras => layout.advanced_control_rects[row],
        C::AdvancedScrollbar if !basic && !extras => AdvancedField::advanced_scrollbar_rect(layout),
        C::Players0x3eb if basic => layout.control_rects[5],
        C::UrbanAreas5000 if basic && modal.ares => layout.urban_areas,
        _ if basic && SetupCombo::from_control(control).is_some() => {
            layout.control_rects[SetupCombo::from_control(control)?.row()]
        }
        C::Randomize0x621 => layout.randomize,
        C::Generate0x620 => layout.generate,
        C::Ok0x6c5 => layout.use_map,
        C::Load0x6c2 => layout.load,
        C::Save0x6c3 => layout.save,
        C::Delete0x6c4 => layout.delete,
        C::Cancel0x5c0 => layout.cancel,
        C::AdvancedToggle => layout.advanced,
        C::ExportMap => layout.export,
        C::FairStartsToggle => layout.fair_starts,
        C::PresetCycle => layout.preset,
        _ => return None,
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

/// Builds the widgets under the controller's `0x105` (a fresh instance when
/// another dialog is on top), puts them on `layout` and shows the options.
pub fn sync_page(
    controller: &mut DialogController,
    modal: &RandomMapSetupModalState,
    layout: &RandomMapSetupLayout,
) -> Option<NodeId> {
    controller.ensure_active(DIALOG, false);
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
            _ => {
                if let Some(combo) = SetupCombo::from_control(control) {
                    let count = modal.combo_items(combo).len();
                    let face = combo_face_rect(layout.control_rects[combo.row()], layout.scale);
                    let selected = modal
                        .selected_item_index(combo)
                        .map_or(-1, |row| row as i32);
                    set(Prop::RowCount, Value::Int(count as i32));
                    set(Prop::Selected, Value::Int(selected));
                    // A closed combo takes the pointer on its face only.
                    set(Prop::HitRect, Value::Rect(face));
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

/// What the widget events do to the dialog: the cues to play and the button
/// the user activated. The combos and sliders act at once; the buttons on
/// release.
fn handle(
    modal: &mut RandomMapSetupModalState,
    events: Vec<PageEvent>,
    sounds: &mut Vec<SkirmishShellUiSound>,
) -> Option<C> {
    let mut activated = None;
    for event in events {
        match event {
            PageEvent::Cue(cue) => sounds.extend(match cue {
                PageCue::Button => Some(SkirmishShellUiSound::MainButton),
                PageCue::ComboOpen => Some(SkirmishShellUiSound::GuiComboOpenSound),
                PageCue::ComboClose => Some(SkirmishShellUiSound::GuiComboCloseSound),
                PageCue::Checkbox | PageCue::ListClick => None,
            }),
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
                    _ => false,
                };
                if changed {
                    sounds.push(SkirmishShellUiSound::GenericClick);
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
/// pointer and holds the mouse until the release.
pub fn pointer(
    controller: &mut DialogController,
    modal: &mut RandomMapSetupModalState,
    layout: &RandomMapSetupLayout,
    at: Pointer,
    x: i32,
    y: i32,
) -> (Vec<SkirmishShellUiSound>, Option<C>) {
    let mut sounds = Vec::new();
    if sync_page(controller, modal, layout).is_none() {
        return (sounds, None);
    }
    let paged = match at {
        Pointer::Down
            if controller.gui().popup().is_none()
                && modal.advanced
                && modal.extras_page.is_none() =>
        {
            match AdvancedField::advanced_scrollbar_part_at(layout, modal.advanced_page, x, y) {
                Some(DropdownScrollbarPart::UpArrow) => Some(modal.scroll_advanced(-1)),
                Some(DropdownScrollbarPart::DownArrow) => Some(modal.scroll_advanced(1)),
                Some(_) => {
                    modal.advanced_scroll_hold = true;
                    let page = AdvancedField::advanced_scrollbar_page_at_pointer(
                        layout,
                        modal.advanced_page,
                        y,
                    );
                    Some(modal.set_advanced_scroll(page))
                }
                None => None,
            }
        }
        Pointer::Move if modal.advanced_scroll_hold => {
            let page =
                AdvancedField::advanced_scrollbar_page_at_pointer(layout, modal.advanced_page, y);
            Some(modal.set_advanced_scroll(page))
        }
        Pointer::Up if std::mem::take(&mut modal.advanced_scroll_hold) => Some(false),
        _ => None,
    };
    if let Some(paged) = paged {
        sounds.extend(paged.then_some(SkirmishShellUiSound::GenericClick));
        return (sounds, None);
    }
    match at {
        Pointer::Down => controller.on_pointer_down(x, y, &[]),
        Pointer::Move => controller.on_pointer_move(x, y, &[]),
        Pointer::Up => {
            let held = slider_held(controller.gui());
            controller.on_pointer_up(x, y, &[]);
            // The freed hit test sees the pointer where it let go.
            if held {
                controller.on_pointer_move(x, y, &[]);
            }
        }
    }
    let raw = controller.drain_events();
    let events = page::page_events(controller.gui(), raw);
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
pub fn holding(controller: &DialogController, modal: &RandomMapSetupModalState) -> bool {
    modal.advanced_scroll_hold
        || shown(controller).is_some_and(|gui| gui.popup().is_some() || slider_held(gui))
}

/// The status help of the control under the pointer.
pub fn hovered_help(
    controller: &DialogController,
    modal: &RandomMapSetupModalState,
) -> Option<&'static str> {
    let gui = shown(controller)?;
    let id = u16::try_from(gui.id_of(gui.hovered()?)?).ok()?;
    Some(help(modal, control_of(modal, id)?))
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
    (
        open,
        gui.pressed().and_then(control),
    )
}

/// Closes the dialog's open list, without a cue (VERA's Esc).
pub fn close_list(controller: &mut DialogController) -> bool {
    controller.top_id() == Some(DIALOG) && controller.gui_mut().close_popup()
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
    controller.on_pointer_move(x, y, &[]);
    let gui = controller.gui();
    control_of(&modal, u16::try_from(gui.id_of(gui.hovered()?)?).ok()?)
}
