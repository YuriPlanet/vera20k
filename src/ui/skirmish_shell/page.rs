//! Skirmish setup `0x102`'s controls as `vera20k-gui` widgets: the right
//! panel's owner-draw buttons, the player rows' combos, the option boxes and
//! sliders, and the statics whose hover writes the status line. The widgets
//! own the interaction (capture, the open list, its scrollbar, the hot row);
//! the session values the launch reads stay in [`SkirmishShellState`], and
//! each sync shows them. A row slot's combos show the player row
//! [`SkirmishShellState::slot_row`] maps it to.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Rgba, Value};

use super::layout::{
    DropdownMetrics, RectPx, SEAT_MENU, SKIRMISH_LONG_ROW_COUNT, SKIRMISH_ROW_COUNT,
    SkirmishCheckboxId, SkirmishShellLayout, SkirmishTrackbarId, TEAM_MENU, combo_face_rect,
};
use super::state::{
    OwnerDrawButton, SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice,
    SkirmishHoverTarget as Target, SkirmishShellAction, SkirmishShellState, apply_combo_selection,
    combo_dropdown_max_visible_rows, combo_items, combo_open, combo_rect, player_row_visible,
    selected_combo_item, selected_combo_item_index, status_help_key_for_hover,
};
use crate::map::scenario_menu::MapMenuEntry;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::game_speed::SpeedContext;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// The page's dialog in the shell controller.
pub const DIALOG: DialogResourceId = DialogResourceId(0x0102);

/// The page's fixed controls by resource id, in paint order. VERA's two
/// right-panel buttons take ids gamemd's dialog has none of.
const CONTROLS: [(u16, Target); 18] = [
    (0x54E, Target::Checkbox(SkirmishCheckboxId::ShortGame0x54e)),
    (0x693, Target::Checkbox(SkirmishCheckboxId::McvRepacks0x693)),
    (
        0x696,
        Target::Checkbox(SkirmishCheckboxId::CratesAppear0x696),
    ),
    (
        0x69A,
        Target::Checkbox(SkirmishCheckboxId::SuperWeapons0x69a),
    ),
    (
        0x69D,
        Target::Checkbox(SkirmishCheckboxId::BuildOffAlly0x69d),
    ),
    (0x529, Target::Trackbar(SkirmishTrackbarId::GameSpeed0x529)),
    (0x511, Target::Trackbar(SkirmishTrackbarId::Credits0x511)),
    (0x50C, Target::Trackbar(SkirmishTrackbarId::UnitCount0x50c)),
    (
        0x617,
        Target::OwnerDrawButton(OwnerDrawButton::StartGame0x617),
    ),
    (
        0x7101,
        Target::OwnerDrawButton(OwnerDrawButton::GameOptions),
    ),
    (
        0x5AA,
        Target::OwnerDrawButton(OwnerDrawButton::ChooseMap0x5aa),
    ),
    (0x7102, Target::OwnerDrawButton(OwnerDrawButton::Setups)),
    (0x5C0, Target::OwnerDrawButton(OwnerDrawButton::Back0x5c0)),
    (0x6A0, Target::PlayerName0x6a0),
    (0x468, Target::MapPreview0x468),
    (0x6EC, Target::GameTypeLabel0x6ec),
    (0x5A8, Target::ScenarioLabel0x5a8),
    (0x695, Target::StatusHelp0x695),
];
/// The name box: a dialog's Win32 edit, so a press that focuses it selects
/// the name, and `EM_LIMITTEXT` caps its ANSI bytes.
const PLAYER_NAME: u16 = 0x6A0;
/// VERA's scrollbar beside the computer rows: a List over them that only
/// scrolls (its rect is the scrollbar alone), a row an arrow step and a
/// wheel notch ([`super::state::opponent_scroll_rows`]).
pub const OPPONENT_SCROLL: u16 = 0x7104;
/// The row slots' flag pictures (gamemd's `0x6DA..0x6E1`, then VERA's).
const FLAG: u16 = 0x7300;
/// VERA: the row slots' left-margin numbers, `ROW_NUMBER + slot`. Past the
/// flags (`0x7300..0x730C`) and clear of the combos and headings.
const ROW_NUMBER: u16 = 0x7310;
/// VERA: a row number takes the same yellow as the player columns' text when
/// the seat is taken, and the disabled red an empty ("None") row's captions
/// take when it is not — the two colours the row's own combo face uses.
const NUMBER_TAKEN: Value = Value::Rgba(Rgba(0xFF, 0xFF, 0, 0xFF));
const NUMBER_EMPTY: Value = Value::Rgba(Rgba(0x9F, 0, 0, 0xFF));
/// The row slots' combos: `COMBO + kind * 0x20 + slot`, kind 0 the computer
/// type, then Side, Color, Start and Team.
const COMBO: u16 = 0x7200;

fn control(id: u16) -> Option<Target> {
    CONTROLS
        .iter()
        .find(|(c, _)| *c == id)
        .map(|(_, target)| *target)
}

/// The combo control `kind` of row slot `slot` stands for now.
fn combo_id(state: &SkirmishShellState, kind: u16, slot: usize) -> Option<SkirmishComboId> {
    let row = state.slot_row(slot);
    Some(match kind {
        0 => SkirmishComboId::AiType(row.checked_sub(1).filter(|_| slot > 0)?),
        1 => SkirmishComboId::Side(row),
        2 => SkirmishComboId::Color(row),
        3 => SkirmishComboId::Start(row),
        4 => SkirmishComboId::Team(row),
        _ => return None,
    })
}

/// VERA's Quick Team Setup and Bulk Seat Actions under the options, combos
/// of their own ids ([`TEAM_MENU`], [`SEAT_MENU`]) whose lists act on the rows
/// (`state::team_menus`).
const MENUS: [(u16, SkirmishComboId); 2] = [
    (TEAM_MENU, SkirmishComboId::TeamMenu),
    (SEAT_MENU, SkirmishComboId::SeatMenu),
];

/// The width VERA's menus open at, in 800x600 pixels: room for Fill with
/// AI's longest caption.
const MENU_LIST_W: i32 = 250;

fn menu_of(id: u16) -> Option<SkirmishComboId> {
    MENUS
        .iter()
        .find(|(menu, _)| *menu == id)
        .map(|(_, combo)| *combo)
}

/// The combo widget `id` stands for now: a row slot's or one of the menus.
fn any_combo(state: &SkirmishShellState, id: u16) -> Option<SkirmishComboId> {
    menu_of(id).or_else(|| combo_of(state, id).map(|(combo, _)| combo))
}

fn combo_of(state: &SkirmishShellState, id: u16) -> Option<(SkirmishComboId, usize)> {
    let offset = id.checked_sub(COMBO)?;
    let slot = usize::from(offset % 0x20);
    combo_id(state, offset / 0x20, slot)
        .filter(|_| slot < SKIRMISH_LONG_ROW_COUNT)
        .zip(Some(slot))
}

fn flag_slot(id: u16) -> Option<usize> {
    id.checked_sub(FLAG)
        .map(usize::from)
        .filter(|slot| *slot < SKIRMISH_LONG_ROW_COUNT)
}

/// The row slot `ROW_NUMBER + slot` numbers.
fn row_number_slot(id: u16) -> Option<usize> {
    id.checked_sub(ROW_NUMBER)
        .map(usize::from)
        .filter(|slot| *slot < SKIRMISH_LONG_ROW_COUNT)
}

/// The player columns' and the sliders' headings, statics with no control of
/// their own (VERA's ids).
use super::layout::HEADINGS;

/// The words the page's captions take: each combo row's label (the menus'
/// items among them), the Random a country face shows (`GUI:RandomEx`), the
/// menus' captions in [`MENUS`] order, the option boxes' and the right
/// panel's buttons' captions in [`CONTROLS`] order, and the headings.
#[derive(Debug, Clone, Default)]
pub struct SkirmishLabels {
    pub items: Vec<(SkirmishComboItem, String)>,
    pub random_country: String,
    pub menus: [String; 2],
    pub checkboxes: [String; 5],
    pub buttons: [String; 5],
    pub headings: [String; 8],
}

impl SkirmishLabels {
    fn item(&self, item: SkirmishComboItem) -> String {
        let found = self.items.iter().find(|(i, _)| *i == item);
        found.map_or_else(String::new, |(_, label)| label.clone())
    }
}

/// The page's widgets in paint order: the combos row by row, the option
/// boxes, the sliders, the right panel's buttons, then the statics that
/// only carry status help.
fn spec() -> NodeSpec {
    let combos = (0..SKIRMISH_LONG_ROW_COUNT).flat_map(|slot| {
        (u16::from(slot == 0)..5).map(move |kind| {
            NodeSpec::new(NodeKind::Combo).id(i32::from(COMBO + kind * 0x20) + slot as i32)
        })
    });
    let flags = (0..SKIRMISH_LONG_ROW_COUNT).map(|slot| (FLAG + slot as u16, Target::FlagPicture));
    let controls = CONTROLS.into_iter().chain(flags).map(|(id, target)| {
        let spec = match target {
            Target::Checkbox(_) => NodeSpec::new(NodeKind::Checkbox),
            Target::Trackbar(_) => {
                NodeSpec::new(NodeKind::Trackbar).with(Prop::RehoverOnRelease, Value::Bool(true))
            }
            Target::OwnerDrawButton(OwnerDrawButton::Back0x5c0) => {
                NodeSpec::new(NodeKind::Button).with(Prop::TabStop, Value::Bool(true))
            }
            Target::OwnerDrawButton(_) => NodeSpec::new(NodeKind::Button),
            Target::PlayerName0x6a0 => NodeSpec::new(NodeKind::Edit)
                .with(
                    Prop::TextInset,
                    Value::Rect(super::layout::FRAMED_EDIT_TEXT_INSET),
                )
                .with(Prop::SelectOnFocus, Value::Bool(true))
                // A press anywhere else on the page takes the focus.
                .with(Prop::BlurOnPress, Value::Bool(true))
                .with(
                    Prop::MaxBytes,
                    Value::Int(super::state::PLAYER_NAME_EDIT_LIMIT_BYTES as i32),
                ),
            // A static hovers for its status help, none for the status line.
            _ => NodeSpec::new(NodeKind::Text),
        };
        spec.id(id.into())
            .tooltip(status_help_key_for_hover(target).unwrap_or_default())
    });
    let headings = (0..8).map(|index| NodeSpec::new(NodeKind::Text).id((HEADINGS + index).into()));
    // VERA: a number for each visible player row, centred in the margin to
    // the name box's left. They carry no hover help and take no press.
    let centre = Value::Int((ShellAlign::H_CENTER | ShellAlign::V_CENTER).0.into());
    let row_numbers = (0..SKIRMISH_LONG_ROW_COUNT).map(|slot| {
        NodeSpec::new(NodeKind::Text)
            .id((ROW_NUMBER + slot as u16).into())
            .with(Prop::Align, centre.clone())
    });
    // The scrollbar takes a press first, as it did over the page's controls.
    let scroll = NodeSpec::new(NodeKind::List)
        .id(OPPONENT_SCROLL.into())
        .with(Prop::WheelStep, Value::Int(1))
        .with(Prop::RehoverOnRelease, Value::Bool(true));
    let menus = MENUS.map(|(id, combo)| {
        NodeSpec::new(NodeKind::Combo)
            .id(id.into())
            .tooltip(status_help_key_for_hover(Target::ComboFace(combo)).unwrap_or_default())
    });
    NodeSpec::new(NodeKind::Dialog)
        .child(scroll)
        .children(combos)
        .children(menus)
        .children(controls)
        .children(row_numbers)
        .children(headings)
}

/// Control `id`'s rect on `layout`; none while it does not show.
fn rect(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    id: u16,
) -> Option<RectPx> {
    let slot_shows = |slot: usize| {
        slot < layout.shown_rows && player_row_visible(state, maps, state.slot_row(slot))
    };
    if let Some((combo, slot)) = combo_of(state, id) {
        return combo_rect(layout, state, combo).filter(|_| slot_shows(slot));
    }
    if let Some(combo) = menu_of(id) {
        return combo_rect(layout, state, combo);
    }
    if let Some(slot) = flag_slot(id) {
        return layout.flags.get(slot).copied().filter(|_| slot_shows(slot));
    }
    if let Some(slot) = row_number_slot(id) {
        return layout
            .row_numbers
            .get(slot)
            .copied()
            .filter(|_| slot_shows(slot));
    }
    if id == OPPONENT_SCROLL {
        return super::state::opponent_scroll_geometry(layout, state, maps).map(|g| g.outer);
    }
    // The fixed controls and the headings stand in the layout's table.
    layout.statics.get(id)
}

/// Builds the page under the controller's `0x102` (pushing it when another
/// dialog is on top), puts it on `layout` and shows the session; its root.
pub fn sync_page(
    controller: &mut DialogController,
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    labels: &SkirmishLabels,
) -> Option<NodeId> {
    // VERA's Game Options box may sit over the page.
    if controller.root_in(DIALOG).is_none() {
        controller.ensure_active(DIALOG);
    }
    controller.set_scale(layout.scale);
    let root = controller.root_in(DIALOG)?;
    let gui = controller.gui_mut();
    page::ensure_built(gui, root, spec);
    page::sync_rects(gui, root, |id| rect(state, layout, maps, id));
    let scrollbar_w = DropdownMetrics::scaled(layout.scale).scrollbar_w;
    // VERA: on the longer player list a low row's list would run over the
    // status line or off the screen, so it opens upward instead. gamemd's
    // eight rows never reach it.
    let flip = match layout.shown_rows > SKIRMISH_ROW_COUNT {
        true => layout.status_help.y,
        false => i32::MAX,
    };
    for (slot, kind) in (0..SKIRMISH_LONG_ROW_COUNT).flat_map(|slot| (0..5).map(move |k| (slot, k)))
    {
        let Some(id) = combo_id(state, kind, slot) else {
            continue;
        };
        let items = combo_items(state, maps, id);
        let face = match selected_combo_item(state, id) {
            Some(SkirmishComboItem::Country(SkirmishCountryChoice::Random)) => {
                labels.random_country.clone()
            }
            Some(item) => labels.item(item),
            None => String::new(),
        };
        let swatches = items.iter().map(|item| match item {
            SkirmishComboItem::Color(color) => color.to_string(),
            _ => String::new(),
        });
        let selected = selected_combo_item_index(state, maps, id).map_or(-1, |row| row as i32);
        let node = COMBO + kind * 0x20 + slot as u16;
        // The control rect runs down over its list; only the face hits.
        let hit = combo_rect(layout, state, id).map(|rect| combo_face_rect(rect, layout.scale));
        for (prop, value) in [
            (Prop::HitRect, Value::Rect(hit.unwrap_or_default())),
            (
                Prop::Items,
                Value::Items(items.iter().map(|item| labels.item(*item)).collect()),
            ),
            (Prop::Swatches, Value::Items(swatches.collect())),
            (Prop::Selected, Value::Int(selected)),
            (Prop::Text, Value::Text(face)),
            (Prop::Enabled, Value::Bool(combo_open(state, id))),
            (
                Prop::VisibleRows,
                Value::Int(combo_dropdown_max_visible_rows(id)),
            ),
            // A scrolling Start or Team list grows by its scrollbar, so the
            // rows keep the face's width for a caption (gamemd's nine starts
            // and four teams never scroll; ComboDropWin 0x0060D540 narrows
            // the rows instead).
            (
                Prop::PopupWiden,
                Value::Int(if matches!(kind, 3 | 4) {
                    scrollbar_w
                } else {
                    0
                }),
            ),
            (Prop::PopupFlipBelow, Value::Int(flip)),
        ] {
            page::set(gui, root, node, prop, value);
        }
    }
    // The menus show their caption, not a pick. Their list opens at least
    // [`MENU_LIST_W`] wide from the face's left edge and, past the status
    // line, above the face.
    for ((node, combo), caption) in MENUS.into_iter().zip(&labels.menus) {
        let items = combo_items(state, maps, combo);
        let face = combo_rect(layout, state, combo).map(|rect| combo_face_rect(rect, layout.scale));
        let popup = face.map(|face| {
            let mut popup = vera20k_gui::shell::combobox::ComboPopup::new(
                face,
                layout.scale,
                items.len(),
                0,
                0,
            );
            popup.rect.w = layout
                .scale
                .derive_rect(RectPx::new(0, 0, MENU_LIST_W, 0), |rect| rect)
                .w
                .max(face.w);
            if popup.rect.y + popup.rect.h > layout.status_help.y {
                popup.rect.y = face.y - 1 - popup.rect.h;
            }
            popup.rect
        });
        for (prop, value) in [
            (Prop::HitRect, Value::Rect(face.unwrap_or_default())),
            // Host-counted: the painter takes the rows from [`menu_rows`],
            // a greyed one in its own colour.
            (Prop::RowCount, Value::Int(items.len() as i32)),
            (Prop::Selected, Value::Int(-1)),
            (Prop::Text, Value::Text(caption.clone())),
            (Prop::Enabled, Value::Bool(combo_open(state, combo))),
            (Prop::PopupRect, Value::Rect(popup.unwrap_or_default())),
        ] {
            page::set(gui, root, node, prop, value);
        }
    }
    let (rows, shown) = super::state::opponent_scroll_rows(state, maps).unwrap_or_default();
    for (prop, value) in [
        (Prop::RowCount, rows),
        (Prop::VisibleRows, shown),
        (Prop::Top, state.first_opponent),
    ] {
        page::set(gui, root, OPPONENT_SCROLL, prop, Value::Int(value as i32));
    }
    let name = Value::Text(state.player_name.clone());
    page::set(gui, root, PLAYER_NAME, Prop::Text, name);
    for (index, heading) in (HEADINGS..).zip(&labels.headings) {
        page::set(gui, root, index, Prop::Text, Value::Text(heading.clone()));
    }
    // VERA: number the visible rows by their player position, so a roster
    // scrolled past the eighth still reads 9, 10, 11... not 1, 2, 3... An
    // empty ("None") row's number takes the same disabled colour as the rest
    // of that row's captions; a taken seat stays yellow.
    for slot in 0..SKIRMISH_LONG_ROW_COUNT {
        let row = state.slot_row(slot);
        let taken = row == 0
            || state
                .opponents
                .get(row - 1)
                .is_some_and(super::state::SkirmishShellOpponent::is_active);
        let node = ROW_NUMBER + slot as u16;
        page::set(
            gui,
            root,
            node,
            Prop::Text,
            Value::Text((row + 1).to_string()),
        );
        page::set(
            gui,
            root,
            node,
            Prop::Color,
            if taken {
                NUMBER_TAKEN.clone()
            } else {
                NUMBER_EMPTY.clone()
            },
        );
    }
    let mut captions = labels.checkboxes.iter().chain(&labels.buttons);
    for (id, target) in CONTROLS {
        if matches!(target, Target::OwnerDrawButton(_)) {
            let caption = captions.next().cloned().unwrap_or_default();
            page::set(gui, root, id, Prop::Text, Value::Text(caption));
        }
        match target {
            Target::Checkbox(checkbox) => {
                let caption = captions.next().cloned().unwrap_or_default();
                page::set(gui, root, id, Prop::Text, Value::Text(caption));
                let checked = match checkbox {
                    SkirmishCheckboxId::ShortGame0x54e => state.short_game,
                    SkirmishCheckboxId::McvRepacks0x693 => state.mcv_redeploy,
                    SkirmishCheckboxId::CratesAppear0x696 => state.crates,
                    SkirmishCheckboxId::SuperWeapons0x69a => state.super_weapons,
                    SkirmishCheckboxId::BuildOffAlly0x69d => state.build_off_ally,
                };
                page::set(gui, root, id, Prop::Checked, Value::Bool(checked));
            }
            Target::Trackbar(trackbar) => {
                let range = state.trackbar_bounds.visual_range(trackbar);
                for (prop, value) in [
                    (Prop::Min, range.min),
                    (Prop::Max, range.max),
                    (Prop::Step, range.step),
                    (Prop::Pos, trackbar_value(state, trackbar).0),
                ] {
                    page::set(gui, root, id, prop, Value::Int(value));
                }
                let shown = trackbar_value(state, trackbar).1;
                page::set(gui, root, id, Prop::Text, Value::Text(shown));
            }
            _ => {}
        }
    }
    Some(root)
}

fn checkbox_value(state: &mut SkirmishShellState, id: SkirmishCheckboxId) -> &mut bool {
    match id {
        SkirmishCheckboxId::ShortGame0x54e => &mut state.short_game,
        SkirmishCheckboxId::McvRepacks0x693 => &mut state.mcv_redeploy,
        SkirmishCheckboxId::CratesAppear0x696 => &mut state.crates,
        SkirmishCheckboxId::SuperWeapons0x69a => &mut state.super_weapons,
        SkirmishCheckboxId::BuildOffAlly0x69d => &mut state.build_off_ally,
    }
}

/// A slider's thumb position and its value text: the speed's name, the
/// credits the dialog reads back ([`SkirmishShellState::credits`]) or the
/// unit count.
fn trackbar_value(state: &SkirmishShellState, id: SkirmishTrackbarId) -> (i32, String) {
    let bounds = state.trackbar_bounds;
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => (
            SpeedContext::Offline.visual_position(state.game_speed),
            SpeedContext::Offline.value_text(state.game_speed),
        ),
        SkirmishTrackbarId::Credits0x511 => (
            bounds.credits_visual_position(state.starting_credits),
            state.credits().to_string(),
        ),
        SkirmishTrackbarId::UnitCount0x50c => (state.unit_count, state.unit_count.to_string()),
    }
}

/// Stores the slider's new thumb position; whether the value changed.
fn set_trackbar_value(state: &mut SkirmishShellState, id: SkirmishTrackbarId, visual: i32) -> bool {
    let (value, stored) = match id {
        SkirmishTrackbarId::GameSpeed0x529 => (
            SpeedContext::Offline.stored_speed(visual),
            &mut state.game_speed,
        ),
        SkirmishTrackbarId::Credits0x511 => (
            state.trackbar_bounds.credits_from_visual_position(visual),
            &mut state.starting_credits,
        ),
        SkirmishTrackbarId::UnitCount0x50c => (visual, &mut state.unit_count),
    };
    std::mem::replace(stored, value) != value
}

/// What the page's widget events do to the session, in order: the cues to
/// play and the button the user activated.
fn handle(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    events: Vec<PageEvent>,
) -> (Vec<PageCue>, Option<SkirmishShellAction>) {
    let (mut cues, mut action) = (Vec::new(), None);
    for event in events {
        match event {
            PageEvent::Cue(cue) => cues.push(cue),
            PageEvent::TextChanged(PLAYER_NAME, name) => {
                state.player_name = crate::util::native_string::acp_round_trip(&name);
            }
            PageEvent::Activated(id) => {
                if let Some(Target::OwnerDrawButton(button)) = control(id) {
                    action = Some(match button {
                        OwnerDrawButton::StartGame0x617 => SkirmishShellAction::StartGame,
                        OwnerDrawButton::ChooseMap0x5aa => SkirmishShellAction::ChooseMap,
                        OwnerDrawButton::GameOptions => SkirmishShellAction::GameOptions,
                        OwnerDrawButton::Setups => SkirmishShellAction::Setups,
                        OwnerDrawButton::Back0x5c0 => SkirmishShellAction::BackOrExit,
                    });
                }
            }
            PageEvent::Toggled(id, on) => {
                match control(id) {
                    // VERA: the page ties Short Game to the Bases its Game
                    // Options hold, as C2 does (`0x0078FB8D`).
                    Some(Target::Checkbox(SkirmishCheckboxId::ShortGame0x54e))
                        if state.short_game != on =>
                    {
                        crate::skirmish_launch::click_short_game(
                            &mut state.short_game,
                            &mut state.session.bases,
                        );
                    }
                    Some(Target::Checkbox(checkbox)) => *checkbox_value(state, checkbox) = on,
                    _ => {}
                }
            }
            PageEvent::Picked(id, row) => {
                let combo = any_combo(state, id);
                let item = combo.and_then(|combo| {
                    combo_items(state, maps, combo)
                        .get(usize::try_from(row).ok()?)
                        .copied()
                });
                if let (Some(combo), Some(item)) = (combo, item) {
                    apply_combo_selection(state, combo, item);
                }
            }
            // A pointer-driven change clicks (`0x0061E6DD`); the dialog has no
            // `WM_HSCROLL` case for the notification sent before it.
            PageEvent::Slid {
                id,
                value,
                final_: false,
            } => {
                if let Some(Target::Trackbar(trackbar)) = control(id)
                    && set_trackbar_value(state, trackbar, value)
                {
                    cues.push(PageCue::ListClick);
                }
            }
            _ => {}
        }
    }
    (cues, action)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Down,
    Move,
    Up,
}

/// Whether a slider holds the mouse (`SetCapture` 0x0061E4DF) until the
/// release.
pub fn slider_held(gui: &Gui) -> bool {
    gui.pressed()
        .is_some_and(|node| gui.tree().kind(node) == Ok(NodeKind::Trackbar))
}

/// Whether a slider or the computer rows' scrollbar holds the mouse until
/// the release; the status line keeps its text meanwhile.
pub fn pointer_held(gui: &Gui) -> bool {
    slider_held(gui)
        || gui
            .pressed()
            .is_some_and(|node| gui.tree().kind(node) == Ok(NodeKind::List))
}

/// The computer rows' scrollbar moved: the rows follow its top.
fn take_scroll(
    controller: &DialogController,
    state: &mut SkirmishShellState,
    events: &[PageEvent],
) {
    if events.contains(&PageEvent::Scrolled(OPPONENT_SCROLL))
        && let Some(root) = controller.root_in(DIALOG)
        && let Some(top) = page::int(controller.gui(), root, OPPONENT_SCROLL, Prop::Top)
    {
        state.first_opponent = top.max(0) as usize;
    }
}

/// One pointer message on the controller's top dialog of the Skirmish
/// family, as its widgets' events. A release that frees a held slider or
/// the computer rows (`RehoverOnRelease`) lets the dialog's hit test see the
/// pointer where it let go.
pub fn route(controller: &mut DialogController, at: Pointer, x: i32, y: i32) -> Vec<PageEvent> {
    match at {
        Pointer::Down => controller.on_pointer_down(x, y),
        Pointer::Move => controller.on_pointer_move(x, y),
        Pointer::Up => controller.on_pointer_up(x, y),
    }
    let raw = controller.drain_events();
    page::page_events(controller.gui(), raw)
}

/// The pointer at `x`, `y` on the page's widgets, VERA's scrollbar beside
/// the computer rows among them. Returns the cues to play and the button
/// the user activated.
#[allow(clippy::too_many_arguments)]
pub fn pointer(
    controller: &mut DialogController,
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    labels: &SkirmishLabels,
    at: Pointer,
    x: i32,
    y: i32,
) -> (Vec<PageCue>, Option<SkirmishShellAction>) {
    if sync_page(controller, state, layout, maps, labels).is_none() {
        return (Vec::new(), None);
    }
    let open_menu = open_menu_top(controller);
    let mut events = route(controller, at, x, y);
    keep_menu_open(controller, state, maps, open_menu, &mut events);
    take_scroll(controller, state, &events);
    handle(state, maps, events)
}

/// Menu widget `id`'s rows as the painter shows them: each item's caption
/// and whether a click picks it (a greyed item or heading does not).
pub fn menu_rows(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    labels: &SkirmishLabels,
    id: u16,
) -> Option<Vec<(String, bool)>> {
    let items = combo_items(state, maps, menu_of(id)?);
    Some(
        items
            .into_iter()
            .map(|item| (labels.item(item), item.pickable()))
            .collect(),
    )
}

/// The menu whose list is open, with its first row.
fn open_menu_top(controller: &DialogController) -> Option<(u16, usize)> {
    let gui = controller.gui();
    let node = gui.popup()?;
    let id = u16::try_from(gui.id_of(node)?).ok()?;
    menu_of(id)?;
    Some((id, gui.combo_drop_down(node)?.popup.first))
}

/// A press on a menu's greyed item or heading picks nothing and keeps the
/// list open, with no close cue.
fn keep_menu_open(
    controller: &mut DialogController,
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    open: Option<(u16, usize)>,
    events: &mut Vec<PageEvent>,
) {
    let Some((menu, top)) = open else {
        return;
    };
    let Some(at) = events.iter().position(|event| match *event {
        PageEvent::Picked(id, row) if id == menu => menu_of(id)
            .and_then(|combo| {
                combo_items(state, maps, combo)
                    .get(usize::try_from(row).ok()?)
                    .copied()
            })
            .is_some_and(|item| !item.pickable()),
        _ => false,
    }) else {
        return;
    };
    events.remove(at);
    events.retain(|event| *event != PageEvent::Cue(PageCue::ComboClose));
    if let Some(node) = controller.control(DIALOG, menu) {
        let _ = controller.gui_mut().open_popup(node, top);
    }
}

/// A held scrollbar arrow's repeat on the computer rows; whether anything
/// changed, and the next poll.
pub fn poll(
    controller: &mut DialogController,
    state: &mut SkirmishShellState,
) -> (bool, Option<crate::util::time::Instant>) {
    let gui = controller.gui();
    let held = controller.top_id() == Some(DIALOG)
        && gui
            .pressed()
            .is_some_and(|node| gui.id_of(node) == Some(OPPONENT_SCROLL.into()));
    if !held {
        return (false, None);
    }
    let next = controller.poll();
    let raw = controller.drain_events();
    let events = page::page_events(controller.gui(), raw);
    take_scroll(controller, state, &events);
    (!events.is_empty(), next)
}

/// The wheel: a notch moves the computer rows one row, wheel up toward the
/// first, wherever the pointer is. Not while a combo's list is open, which
/// takes no wheel (no owner-draw combo, drop-down or scrollbar callback has
/// a wheel handler), nor while the scrollbar holds the mouse. Returns
/// whether the rows moved.
pub fn wheel(
    controller: &mut DialogController,
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    labels: &SkirmishLabels,
    lines: f32,
) -> bool {
    if sync_page(controller, state, layout, maps, labels).is_none() {
        return false;
    }
    let gui = controller.gui();
    if gui.popup().is_some() || pointer_held(gui) {
        return false;
    }
    let Some(node) = controller.control(DIALOG, OPPONENT_SCROLL) else {
        return false;
    };
    let now = controller.now();
    let scrolled = controller.gui_mut().wheel_list(node, lines as i32, now);
    let events = controller.gui_mut().drain_events();
    let events = page::page_events(controller.gui(), events);
    take_scroll(controller, state, &events);
    scrolled
}

/// The computer rows' scrollbar arrow drawn pushed.
pub fn opponent_arrow(
    controller: &DialogController,
) -> Option<vera20k_gui::shell::list::ListScrollPart> {
    let node = controller.control(DIALOG, OPPONENT_SCROLL)?;
    controller.gui().list_view(node)?.pressed
}

/// A key or typed text (none: a key the widgets do not read) for the
/// focused name box, which keeps its caret in view (`measure` the font's
/// text widths); whether the name box has the focus. Tab moves the focus to
/// the next stop, which the page models as Back's, so the box loses it.
pub fn key(
    controller: &mut DialogController,
    state: &mut SkirmishShellState,
    event: Option<vera20k_gui::InputEvent>,
    measure: impl Fn(&str) -> u32,
) -> bool {
    if controller.focused_control(DIALOG) != Some(PLAYER_NAME) {
        return false;
    }
    let Some(event) = event else {
        return true;
    };
    controller.on_input(event);
    let raw = controller.drain_events();
    handle(state, &[], page::page_events(controller.gui(), raw));
    scroll_player_name(controller, measure);
    true
}

/// Whether a press at `x`, `y` lands on the name box, as the page's widgets
/// hit-test it (an open list over it does not count).
pub fn over_name(controller: &DialogController, x: i32, y: i32) -> bool {
    let gui = controller.gui();
    let node = controller
        .root_in(DIALOG)
        .and_then(|root| gui.hit_at(root, x, y, true));
    node.is_some() && node == controller.control(DIALOG, PLAYER_NAME)
}

/// NewEdit keeps the name box's caret in view.
pub fn scroll_player_name(controller: &mut DialogController, measure: impl Fn(&str) -> u32) {
    if let Some(node) = controller.control(DIALOG, PLAYER_NAME) {
        let _ = controller.gui_mut().keep_caret_in_view(node, &measure);
    }
}

/// The control whose status help the pointer at `x`, `y` shows: nothing
/// under a box, the Game Options box's control while it covers the page,
/// else an open list's row, then the hovered control.
pub fn hover_target(
    gui: &Gui,
    root: Option<NodeId>,
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> Option<Target> {
    if state.choose_map_modal.is_some() || state.validation_modal.is_some() {
        return None;
    }
    if state.game_options.is_some() {
        return super::game_options::hovered(gui).map(Target::GameOptions);
    }
    if let Some(open) = open_combo(gui, state)
        && let Some(list) = gui.popup().and_then(|node| gui.combo_drop_down(node))
        && let Some(row) = list.popup.row_at(x, y)
    {
        let item = combo_items(state, maps, open.id).get(row).copied();
        return item.map(|item| Target::ComboItem { id: open.id, item });
    }
    let node = gui
        .hovered()
        .filter(|node| root.is_some() && gui.tree().parent(*node) == Ok(root))?;
    let id = u16::try_from(gui.id_of(node)?).ok()?;
    match (any_combo(state, id), flag_slot(id)) {
        (Some(combo), _) => Some(Target::ComboFace(combo)),
        (_, Some(_)) => Some(Target::FlagPicture),
        _ => control(id),
    }
}

/// The combo the open list belongs to, with its first row and hot row.
pub fn open_combo(
    gui: &Gui,
    state: &SkirmishShellState,
) -> Option<super::state::OpenComboDropdown> {
    let node = gui.popup()?;
    let id = any_combo(state, u16::try_from(gui.id_of(node)?).ok()?)?;
    let list = gui.combo_drop_down(node)?;
    let (top_index, hot_row) = (list.popup.first, list.hot_row);
    Some(super::state::OpenComboDropdown {
        id,
        top_index,
        hot_row,
    })
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::controller::DialogController;

    /// A property of row slot `slot`'s number after the list scrolls by
    /// `first_opponent` on a page showing `rows` player rows, with `edit`
    /// setting the opponent rows before the page is synced.
    fn number_prop(
        slot: usize,
        first_opponent: usize,
        rows: usize,
        edit: impl FnOnce(&mut SkirmishShellState),
        prop: Prop,
    ) -> Option<Value> {
        let mut state = SkirmishShellState::default();
        state.first_opponent = first_opponent;
        edit(&mut state);
        let layout = crate::ui::skirmish_shell::layout::compute_layout_for_rows(800, 600, rows);
        let mut controller = DialogController::default();
        let root = sync_page(
            &mut controller,
            &state,
            &layout,
            &[],
            &SkirmishLabels::default(),
        )?;
        page::get(controller.gui(), root, ROW_NUMBER + slot as u16, prop).cloned()
    }

    fn number_text(slot: usize, first_opponent: usize, rows: usize) -> Option<String> {
        match number_prop(slot, first_opponent, rows, |_| {}, Prop::Text)? {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    #[test]
    fn row_numbers_start_at_one_on_the_players_row() {
        assert_eq!(number_text(0, 0, 8).as_deref(), Some("1"));
        assert_eq!(number_text(1, 0, 8).as_deref(), Some("2"));
        assert_eq!(number_text(7, 0, 8).as_deref(), Some("8"));
    }

    #[test]
    fn row_numbers_follow_scrolled_player_positions() {
        // Past the eighth: slot 1 shows the ninth opponent, player 10, not 2.
        assert_eq!(number_text(1, 8, 13).as_deref(), Some("10"));
        assert_eq!(number_text(12, 8, 13).as_deref(), Some("21"));
    }

    #[test]
    fn row_numbers_take_the_empty_seat_color() {
        // Default: the local row and opponent 0 (Easy) are taken; the rest
        // are None. A taken seat's number stays yellow; an empty one's takes
        // the same disabled red as its "None" captions.
        assert_eq!(
            number_prop(0, 0, 8, |_| {}, Prop::Color),
            Some(NUMBER_TAKEN.clone())
        );
        assert_eq!(
            number_prop(1, 0, 8, |_| {}, Prop::Color),
            Some(NUMBER_TAKEN.clone())
        );
        assert_eq!(
            number_prop(2, 0, 8, |_| {}, Prop::Color),
            Some(NUMBER_EMPTY.clone())
        );
        assert_eq!(
            number_prop(7, 0, 8, |_| {}, Prop::Color),
            Some(NUMBER_EMPTY.clone())
        );
    }

    #[test]
    fn filling_an_empty_seat_turns_its_number_yellow() {
        let fill_second = |state: &mut SkirmishShellState| {
            state.opponents[1].row_type = crate::ui::skirmish_shell::state::SkirmishAiRowType::Hard;
        };
        assert_eq!(
            number_prop(2, 0, 8, fill_second, Prop::Color),
            Some(NUMBER_TAKEN.clone())
        );
    }

    #[test]
    fn row_numbers_sit_left_of_the_name_box() {
        let layout = crate::ui::skirmish_shell::layout::compute_layout(800, 600);
        assert!(
            layout
                .row_numbers
                .iter()
                .all(|number| number.x + number.w <= layout.player_name.x)
        );
    }
}
