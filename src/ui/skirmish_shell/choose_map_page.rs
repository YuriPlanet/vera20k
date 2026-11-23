//! Choose Map `0x6B`'s controls as `vera20k-gui` widgets: the game-type list
//! `0x6EB` and the map list `0x553` (the shell list subclass `0x00618D40`,
//! each a list window on its paint surface), the right column's buttons and
//! the statics whose hover writes the status line. The widgets own the
//! scrollbar capture and repeat, the double-click pairing and the button
//! capture; [`ChooseMapModalState`] keeps what the lists show and each sync
//! shows it.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::ui::shell::geom::ShellAlign;

use super::layout::{ChooseMapModalButton, ChooseMapModalLayout, RectPx};
use super::page::Pointer;
use super::state::{
    ChooseMapHoverTarget, ChooseMapModalState, status_help_key_for_choose_map_hover,
};
use crate::map::skirmish_scenarios::SkirmishScenarioRecord;
use crate::skirmish_modes::SkirmishGameMode;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::page::{self, PageCue, PageEvent};
use vera20k_gui::shell::list::ListScrollPart;

/// The chooser's dialog in the shell controller.
pub const DIALOG: DialogResourceId = DialogResourceId(0x006B);

const MODE_LIST: u16 = 0x6EB;
const MAP_LIST: u16 = 0x553;
const PREVIEW: u16 = 0x468;
const STATUS_HELP: u16 = 0x695;
/// The `SS_CENTER` headings: Select Engagement, Game Type, Game Map.
const HEADINGS: [(u16, &str, &str); 3] = [
    (0x7130, "GUI:SelectEngagement", "Select Engagement"),
    (0x7131, "GUI:GameType", "Game Type"),
    (0x7132, "GUI:GameMap", "Game Map"),
];
/// VERA: the search line over the map list, an edit that keeps the focus.
/// Typing anywhere on the chooser fills it and no press reaches it, so it
/// has no rect; the host paints it from [`ChooseMapModalState::search`].
const MAP_SEARCH: u16 = 0x7121;
/// VERA: the selected map's author under the preview.
const AUTHOR: u16 = super::CHOOSE_MAP_AUTHOR;
const BUTTONS: [(u16, ChooseMapModalButton); 4] = [
    (0x6C5, ChooseMapModalButton::UseMap0x6c5),
    (0x5C0, ChooseMapModalButton::Cancel0x5c0),
    (0x583, ChooseMapModalButton::CreateRandomMap0x583),
    // VERA's own button takes an id gamemd's dialog has none of.
    (0x7120, ChooseMapModalButton::Favorite),
];

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
    // The owner-draw list (`0x00618D40`) draws a row's text with
    // `Shell__DrawTextAligned` (`0x006211D0`) and flags 0: top-left, so a
    // name too wide for the row keeps its first line in the row.
    let list = |id: u16| {
        NodeSpec::new(NodeKind::List)
            .id(id.into())
            .with(Prop::Align, Value::Int(0))
    };
    let help = |target| status_help_key_for_choose_map_hover(target).unwrap_or_default();
    let align = |align: ShellAlign| Value::Int(align.0.into());
    let centred = align(ShellAlign::H_CENTER | ShellAlign::V_CENTER);
    let text = |id: u16, align| {
        NodeSpec::new(NodeKind::Text)
            .id(id.into())
            .with(Prop::Align, align)
    };
    NodeSpec::new(NodeKind::Dialog)
        .child(list(MODE_LIST))
        .child(list(MAP_LIST))
        // The column's captions sit centred in the buttons.
        .children(BUTTONS.map(|(id, _)| {
            NodeSpec::new(NodeKind::Button)
                .id(id.into())
                .with(Prop::Align, centred.clone())
        }))
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
        .children(HEADINGS.map(|(id, ..)| text(id, centred.clone())))
        // Top-aligned like `0x102`'s game type there, so a long name keeps
        // its first line.
        .child(text(AUTHOR, align(ShellAlign::H_CENTER)))
        .child(NodeSpec::new(NodeKind::Edit).id(MAP_SEARCH.into()))
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
        controller.push(DIALOG);
    }
    controller.set_scale(layout.scale);
    let root = controller.root_in(DIALOG)?;
    let gui = controller.gui_mut();
    page::ensure_built(gui, root, spec);
    page::sync_rects(gui, root, |id| match id {
        MODE_LIST => Some(surface(layout.mode_list)),
        MAP_LIST => Some(surface(layout.map_list)),
        // Shown (so it takes typing) but with an empty rect, so presses
        // never reach it; the host paints the search line itself.
        MAP_SEARCH => Some(RectPx::default()),
        _ => layout.rects.get(id),
    });
    // The search as the chooser keeps it, the caret after it.
    let search = Value::Text(modal.search.clone());
    if page::get(gui, root, MAP_SEARCH, Prop::Text) != Some(&search) {
        let end = Value::Int(modal.search.chars().count() as i32);
        page::set(gui, root, MAP_SEARCH, Prop::Text, search);
        page::set(gui, root, MAP_SEARCH, Prop::Caret, end);
    }
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
            modal.map_row_count(),
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
    let search = gui.find_id(root, MAP_SEARCH.into());
    if search.is_some() && gui.focus() != search {
        controller.focus_edit(search);
    }
    Some(root)
}

/// VERA: typed text or Backspace for the search line; every list then shows
/// only maps whose name holds the search ([`ChooseMapModalState::set_search`],
/// which keeps its first [`super::SEARCH_MAX_CHARS`] characters).
pub fn search_input(
    controller: &mut DialogController,
    modal: &mut ChooseMapModalState,
    layout: &ChooseMapModalLayout,
    (modes, records): (&[SkirmishGameMode], &[SkirmishScenarioRecord]),
    event: vera20k_gui::InputEvent,
) {
    if sync_page(controller, modal, layout).is_none() {
        return;
    }
    controller.on_input(event);
    controller.drain_events();
    let text = controller.text(DIALOG, MAP_SEARCH);
    modal.set_search(text, modes, records, layout);
    controller.set_text(DIALOG, MAP_SEARCH, &modal.search);
}

/// Puts the chooser's words on its widgets, each string key and fallback
/// through `label`: the headings, the buttons' captions and the selected
/// map's author. While a slide runs (`sliding`) the column art covers the
/// buttons and the preview's credit.
pub fn show_texts(
    controller: &mut DialogController,
    modal: &ChooseMapModalState,
    records: &[SkirmishScenarioRecord],
    sliding: bool,
    label: &dyn Fn(&str, &str) -> String,
) {
    let Some(root) = controller.root_in(DIALOG) else {
        return;
    };
    // VERA: Favorite stars the highlighted map, or takes its star away.
    let favorite = match modal.favorite_button(records) {
        Some(true) => "VERA:RemoveFavorite",
        _ => "VERA:AddFavorite",
    };
    // VERA: a map's `[Basic]` `Author`, when it names one.
    let author = modal
        .selected_record_index()
        .and_then(|index| records.get(index)?.author.as_deref())
        .map(str::trim)
        .filter(|author| !author.is_empty())
        .map(|author| crate::i18n::tr_fill("VERA:MapAuthor", &[("author", &author)]));
    let column = [
        (0x6C5, label("GUI:UseMap", "Use Map")),
        (0x5C0, label("GUI:Cancel", "Cancel")),
        (0x583, label("GUI:CreateRandomMap", "Create Random Map")),
        (0x7120, label(favorite, "")),
        (AUTHOR, author.unwrap_or_default()),
    ];
    let headings = HEADINGS.map(|(id, key, fallback)| (id, label(key, fallback)));
    let gui = controller.gui_mut();
    for (id, text) in headings
        .into_iter()
        .chain(column.map(|(id, text)| (id, if sliding { String::new() } else { text })))
    {
        page::set(gui, root, id, Prop::Text, Value::Text(text));
    }
}

/// Drops the chooser's dialog and any left above it.
pub fn close(controller: &mut DialogController) {
    while controller.root_in(DIALOG).is_some() {
        controller.pop();
    }
}

/// What one pointer input on the chooser asks of the app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChooserInput {
    /// The cues to play: a press on an enabled button's GUIMainButtonSound
    /// (`0x00612B70`), a row press's GenericClick (`0x0061AA5A`).
    pub cues: Vec<PageCue>,
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
    controller: &DialogController,
    events: Vec<PageEvent>,
    modal: &mut ChooseMapModalState,
    layout: &ChooseMapModalLayout,
    (modes, records): (&[SkirmishGameMode], &[SkirmishScenarioRecord]),
    last_mode_row: &mut Option<usize>,
) -> ChooserInput {
    let mut out = ChooserInput::default();
    let Some(root) = controller.root_in(DIALOG) else {
        return out;
    };
    let gui = controller.gui();
    let top = |id| page::int(gui, root, id, Prop::Top).unwrap_or(0).max(0) as usize;
    for event in events {
        match event {
            PageEvent::Cue(cue) => out.cues.push(cue),
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
    if sync_page(controller, modal, layout).is_none() {
        return ChooserInput::default();
    }
    let events = super::page::route(controller, at, x, y);
    take_events(controller, events, modal, layout, lists, last_mode_row)
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
    let next = controller.poll();
    let tops = (modal.mode_top_index, modal.map_top_index);
    let raw = controller.drain_events();
    let events = page::page_events(controller.gui(), raw);
    take_events(controller, events, modal, layout, (&[], &[]), &mut None);
    (tops != (modal.mode_top_index, modal.map_top_index), next)
}

/// The control under the pointer at `x`, `y`: a game-type row names its
/// mode, whose description the status line shows.
pub fn hover_target(
    gui: &Gui,
    modal: &ChooseMapModalState,
    x: i32,
    y: i32,
) -> Option<ChooseMapHoverTarget> {
    let node = gui.hovered()?;
    let id = u16::try_from(gui.id_of(node)?).ok()?;
    Some(match id {
        STATUS_HELP => ChooseMapHoverTarget::StatusHelp0x695,
        PREVIEW => ChooseMapHoverTarget::Preview0x468,
        MAP_LIST => ChooseMapHoverTarget::MapList0x553,
        MODE_LIST => {
            // The game-type row the list has under the pointer.
            match gui
                .list_row_at(node, x, y)
                .and_then(|row| modal.mode_rows().get(row))
            {
                Some(mode) => ChooseMapHoverTarget::ModeListRow0x6eb { mode: *mode },
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
