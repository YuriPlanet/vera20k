//! Choose Map `0x6B`'s controls as `vera20k-gui` widgets: the game-type list
//! `0x6EB` and the map list `0x553` (the shell list subclass `0x00618D40`,
//! each a list window on its paint surface), the right column's buttons and
//! the statics whose hover writes the status line. The widgets own the
//! scrollbar capture and repeat, the double-click pairing and the button
//! capture; [`ChooseMapModalState`] keeps what the lists show and each sync
//! shows it.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use super::layout::{ChooseMapModalButton, ChooseMapModalLayout, RectPx};
use super::page::Pointer;
use super::state::{
    ChooseMapHoverTarget, ChooseMapModalState, status_help_key_for_choose_map_hover,
};
use crate::map::skirmish_scenarios::SkirmishScenarioRecord;
use crate::skirmish_modes::SkirmishGameMode;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::list::ListScrollPart;
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// The chooser's dialog in the shell controller.
pub const DIALOG: DialogId = DialogId(0x006B);

const MODE_LIST: u16 = 0x6EB;
const MAP_LIST: u16 = 0x553;
const PREVIEW: u16 = 0x468;
const STATUS_HELP: u16 = 0x695;
const BUTTONS: [(u16, ChooseMapModalButton); 4] = [
    (0x6C5, ChooseMapModalButton::UseMap0x6c5),
    (0x5C0, ChooseMapModalButton::Cancel0x5c0),
    (0x583, ChooseMapModalButton::CreateRandomMap0x583),
    // VERA's own button takes an id gamemd's dialog has none of.
    (0x7120, ChooseMapModalButton::Favorite),
];

fn button_rect(layout: &ChooseMapModalLayout, button: ChooseMapModalButton) -> RectPx {
    match button {
        ChooseMapModalButton::UseMap0x6c5 => layout.use_map_button,
        ChooseMapModalButton::Cancel0x5c0 => layout.cancel_button,
        ChooseMapModalButton::CreateRandomMap0x583 => layout.create_random_map_button,
        ChooseMapModalButton::Favorite => layout.favorite_button,
    }
}

fn button_of(id: u16) -> Option<ChooseMapModalButton> {
    BUTTONS
        .iter()
        .find(|(b, _)| *b == id)
        .map(|(_, button)| *button)
}

/// A list's paint surface: one pixel wider and taller than its window, like
/// every family list. The window is what the pointer hits.
fn surface(window: RectPx) -> RectPx {
    RectPx::new(window.x, window.y, window.w + 1, window.h + 1)
}

fn spec() -> NodeSpec {
    let list = |id: u16| {
        NodeSpec::new(NodeKind::List)
            .id(id.into())
            .with(Prop::ListWindow, Value::Bool(true))
            // 0x6B's double-click spans the rows' area, below the last row
            // too; any other press ends it.
            .with(Prop::AreaDoubleClick, Value::Bool(true))
            .with(Prop::ContentDoubleClick, Value::Bool(true))
    };
    let help = |target| status_help_key_for_choose_map_hover(target).unwrap_or_default();
    NodeSpec::new(NodeKind::Dialog)
        .child(list(MODE_LIST))
        .child(list(MAP_LIST))
        .children(BUTTONS.map(|(id, _)| NodeSpec::new(NodeKind::Button).id(id.into())))
        .child(
            NodeSpec::new(NodeKind::Text)
                .id(PREVIEW.into())
                .tooltip(help(ChooseMapHoverTarget::Preview0x468)),
        )
        // The status line writes its own (empty) help.
        .child(
            NodeSpec::new(NodeKind::Text)
                .id(STATUS_HELP.into())
                .tooltip(""),
        )
}

/// Puts the chooser's dialog on top of the controller (dropping whatever a
/// dialog it hid left above it), builds it once and shows `modal`; its root.
pub fn sync_page(
    controller: &mut DialogController,
    modal: &ChooseMapModalState,
    layout: &ChooseMapModalLayout,
) -> Option<NodeId> {
    while controller.root_in(DIALOG).is_some() && controller.top_id() != Some(DIALOG) {
        controller.pop();
    }
    if controller.top_id() != Some(DIALOG) {
        controller.push(DIALOG, false);
    }
    controller.set_scale(layout.scale);
    let root = controller.root_in(DIALOG)?;
    let gui = controller.gui_mut();
    page::ensure_built(gui, root, spec);
    page::sync_rects(gui, root, |id| {
        Some(match id {
            MODE_LIST => surface(layout.mode_list),
            MAP_LIST => surface(layout.map_list),
            PREVIEW => layout.preview,
            STATUS_HELP => layout.status_help,
            _ => button_rect(layout, button_of(id)?),
        })
    });
    let selected = |row: Option<usize>| Value::Int(row.map_or(-1, |row| row as i32));
    for (id, window, rows, top, chosen) in [
        (
            MODE_LIST,
            layout.mode_list,
            modal.mode_rows().len(),
            modal.mode_top_index,
            modal.selected_mode_row(),
        ),
        (
            MAP_LIST,
            layout.map_list,
            modal.filtered_record_indices.len(),
            modal.map_top_index,
            modal.highlighted_filtered_index,
        ),
    ] {
        page::set(gui, root, id, Prop::HitRect, Value::Rect(window));
        page::set(gui, root, id, Prop::RowCount, Value::Int(rows as i32));
        page::set(gui, root, id, Prop::Top, Value::Int(top as i32));
        page::set(gui, root, id, Prop::Selected, selected(chosen));
    }
    for (id, button) in BUTTONS {
        page::set(
            gui,
            root,
            id,
            Prop::Enabled,
            Value::Bool(modal.button_enabled(button)),
        );
    }
    Some(root)
}

/// A rebuilt map list drops a hold on its scrollbar, as the old list's
/// capture went with it.
pub fn take_rebuild(controller: &mut DialogController, modal: &mut ChooseMapModalState) {
    if std::mem::take(&mut modal.map_list_rebuilt)
        && let Some(list) = controller.root_in(DIALOG).and_then(|root| controller.gui().find_id(root, MAP_LIST.into()))
    {
        controller.gui_mut().cancel_list_scroll(list);
    }
}

/// Drops the chooser's dialog and any left above it.
pub fn close(controller: &mut DialogController) {
    while controller.root_in(DIALOG).is_some() {
        controller.pop();
    }
}

/// What one pointer input on the chooser asks of the app.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChooserInput {
    /// A press on an enabled button: GUIMainButtonSound (`0x00612B70`).
    pub button_press: bool,
    /// A press on a row: the list's GenericClick (`0x0061AA5A`).
    pub row_click: bool,
    /// A game-type row rebuilt the map list.
    pub rebuilt: bool,
    /// The second press of a double-click on a row, which `0x6B` ignores.
    pub double_click: bool,
    /// A button released over its own press.
    pub activated: Option<ChooseMapModalButton>,
}

/// Takes the widgets' events into `modal`: a row press selects, a scroll
/// keeps the list's first row.
fn take_events(
    controller: &mut DialogController,
    modal: &mut ChooseMapModalState,
    layout: &ChooseMapModalLayout,
    (modes, records): (&[SkirmishGameMode], &[SkirmishScenarioRecord]),
    last_mode_row: &mut Option<usize>,
) -> ChooserInput {
    let mut out = ChooserInput::default();
    let Some(root) = controller.root_in(DIALOG) else {
        return out;
    };
    let raw = controller.drain_events();
    let gui = controller.gui();
    let top = |id| page::int(gui, root, id, Prop::Top).unwrap_or(0).max(0) as usize;
    for event in page::page_events(gui, raw) {
        match event {
            PageEvent::Cue(PageCue::Button) => out.button_press = true,
            PageEvent::Cue(PageCue::ListClick) => out.row_click = true,
            PageEvent::Selected(MODE_LIST, row) => {
                out.rebuilt |=
                    modal.click_mode_row(row as usize, layout, modes, records, last_mode_row);
            }
            PageEvent::Selected(MAP_LIST, row) => {
                modal.highlighted_filtered_index = Some(row as usize)
            }
            PageEvent::Scrolled(MODE_LIST) => modal.mode_top_index = top(MODE_LIST),
            PageEvent::Scrolled(MAP_LIST) => modal.map_top_index = top(MAP_LIST),
            PageEvent::Activated(id) => out.activated = button_of(id),
            PageEvent::DoubleClicked(..) => out.double_click = true,
            _ => {}
        }
    }
    out
}

/// The pointer at `x`, `y` on the chooser. A second press of a double-click
/// on a row only reports it, which `0x6B` ignores (`0x0061A904..0x0061A945`).
#[allow(clippy::too_many_arguments)]
pub fn pointer(
    controller: &mut DialogController,
    modal: &mut ChooseMapModalState,
    layout: &ChooseMapModalLayout,
    lists: (&[SkirmishGameMode], &[SkirmishScenarioRecord]),
    last_mode_row: &mut Option<usize>,
    at: Pointer,
    x: i32,
    y: i32,
) -> ChooserInput {
    take_rebuild(controller, modal);
    if sync_page(controller, modal, layout).is_none() {
        return ChooserInput::default();
    }
    match at {
        Pointer::Down => controller.on_pointer_down(x, y),
        Pointer::Move => controller.on_pointer_move(x, y),
        Pointer::Up => {
            controller.on_pointer_up(x, y);
        }
    }
    take_events(controller, modal, layout, lists, last_mode_row)
}

/// A held scrollbar arrow's repeat; whether a list moved, and the next poll.
pub fn poll(
    controller: &mut DialogController,
    modal: &mut ChooseMapModalState,
    layout: &ChooseMapModalLayout,
) -> (bool, Option<crate::util::time::Instant>) {
    if controller.top_id() != Some(DIALOG) {
        return (false, None);
    }
    take_rebuild(controller, modal);
    let next = controller.poll();
    let tops = (modal.mode_top_index, modal.map_top_index);
    take_events(controller, modal, layout, (&[], &[]), &mut None);
    (tops != (modal.mode_top_index, modal.map_top_index), next)
}

/// The control under the pointer at `x`, `y`: a game-type row names its
/// mode, whose description the status line shows.
pub fn hover_target(
    gui: &Gui,
    modal: &ChooseMapModalState,
    layout: &ChooseMapModalLayout,
    x: i32,
    y: i32,
) -> Option<ChooseMapHoverTarget> {
    let id = u16::try_from(gui.id_of(gui.hovered()?)?).ok()?;
    Some(match id {
        STATUS_HELP => ChooseMapHoverTarget::StatusHelp0x695,
        PREVIEW => ChooseMapHoverTarget::Preview0x468,
        MAP_LIST => ChooseMapHoverTarget::MapList0x553,
        MODE_LIST => {
            let rows = modal.mode_rows();
            match modal
                .mode_geometry(layout)
                .row_at(rows.len(), modal.mode_top_index, x, y)
            {
                Some(row) => ChooseMapHoverTarget::ModeListRow0x6eb { mode: rows[row] },
                None => ChooseMapHoverTarget::ModeList0x6eb,
            }
        }
        _ => ChooseMapHoverTarget::Button(button_of(id)?),
    })
}

/// What the chooser paints of its widgets: each list's arrow drawn pushed
/// and the button that shows pushed.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChooserLook {
    pub mode_arrow: Option<ListScrollPart>,
    pub map_arrow: Option<ListScrollPart>,
    pub pressed: Option<ChooseMapModalButton>,
}

pub fn look(controller: &DialogController) -> ChooserLook {
    let Some(root) = controller.root_in(DIALOG) else {
        return ChooserLook::default();
    };
    let gui = controller.gui();
    let arrow = |id: u16| gui.list_view(gui.find_id(root, id.into())?)?.pressed;
    ChooserLook {
        mode_arrow: arrow(MODE_LIST),
        map_arrow: arrow(MAP_LIST),
        // A captured button keeps its pushed look off the pointer too.
        pressed: controller
            .pressed_node_in(DIALOG)
            .and_then(|node| button_of(u16::try_from(gui.id_of(node)?).ok()?)),
    }
}
