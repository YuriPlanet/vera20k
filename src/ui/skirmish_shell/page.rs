//! Skirmish setup `0x102`'s controls as `vera20k-gui` widgets: the right
//! panel's owner-draw buttons, the player rows' combos, the option boxes and
//! sliders, and the statics whose hover writes the status line. The widgets
//! own the interaction (capture, the open list, its scrollbar, the hot row);
//! the session values the launch reads stay in [`SkirmishShellState`], and
//! each sync shows them. A row slot's combos show the player row
//! [`SkirmishShellState::slot_row`] maps it to.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use super::layout::{
    DropdownMetrics, RectPx, SKIRMISH_LONG_ROW_COUNT, SKIRMISH_ROW_COUNT, SkirmishCheckboxId,
    SkirmishShellLayout, SkirmishTrackbarId, combo_face_rect,
};
use super::state::{
    OwnerDrawButton, SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice,
    SkirmishHoverTarget as Target, SkirmishShellAction, SkirmishShellState, SkirmishShellUiSound,
    apply_combo_selection, combo_dropdown_max_visible_rows, combo_items, combo_open, combo_rect,
    player_row_visible, selected_combo_item, selected_combo_item_index, status_help_key_for_hover,
};
use crate::map::scenario_menu::MapMenuEntry;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::game_speed::SpeedContext;
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// The page's dialog in the shell controller.
pub const DIALOG: DialogId = DialogId(0x0102);

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
/// The row slots' flag pictures (gamemd's `0x6DA..0x6E1`, then VERA's).
const FLAG: u16 = 0x7300;
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

/// The words the page's captions take: each combo row's label, the Random
/// a country face shows (`GUI:RandomEx`) and the option boxes' captions.
#[derive(Debug, Clone, Default)]
pub struct SkirmishLabels {
    pub items: Vec<(SkirmishComboItem, String)>,
    pub random_country: String,
    pub checkboxes: [String; 5],
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
            NodeSpec::new(NodeKind::Combo)
                .id(i32::from(COMBO + kind * 0x20) + slot as i32)
                .with(Prop::Truncate, Value::Bool(true))
                .with(Prop::ComboScrollbar, Value::Bool(true))
        })
    });
    let flags = (0..SKIRMISH_LONG_ROW_COUNT).map(|slot| (FLAG + slot as u16, Target::FlagPicture));
    let controls = CONTROLS.into_iter().chain(flags).map(|(id, target)| {
        let spec = match target {
            Target::Checkbox(_) => {
                NodeSpec::new(NodeKind::Checkbox).with(Prop::ActivateOnPress, Value::Bool(true))
            }
            Target::Trackbar(_) => NodeSpec::new(NodeKind::Trackbar),
            Target::OwnerDrawButton(_) => NodeSpec::new(NodeKind::Button),
            // A static hovers for its status help, none for the status line.
            _ => NodeSpec::new(NodeKind::Text),
        };
        spec.id(id.into())
            .tooltip(status_help_key_for_hover(target).unwrap_or_default())
    });
    NodeSpec::new(NodeKind::Dialog)
        .children(combos)
        .children(controls)
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
    if let Some(slot) = flag_slot(id) {
        return layout.flags.get(slot).copied().filter(|_| slot_shows(slot));
    }
    Some(match control(id)? {
        Target::Checkbox(checkbox) => layout.checkboxes.iter().find(|c| c.id == checkbox)?.rect,
        Target::Trackbar(SkirmishTrackbarId::GameSpeed0x529) => layout.trackbars.game_speed,
        Target::Trackbar(SkirmishTrackbarId::Credits0x511) => layout.trackbars.credits,
        Target::Trackbar(SkirmishTrackbarId::UnitCount0x50c) => layout.trackbars.unit_count,
        Target::OwnerDrawButton(OwnerDrawButton::StartGame0x617) => layout.start_button,
        Target::OwnerDrawButton(OwnerDrawButton::GameOptions) => layout.game_options_button,
        Target::OwnerDrawButton(OwnerDrawButton::ChooseMap0x5aa) => layout.choose_map_button,
        Target::OwnerDrawButton(OwnerDrawButton::Setups) => layout.setups_button,
        Target::OwnerDrawButton(OwnerDrawButton::Back0x5c0) => layout.back_button,
        Target::PlayerName0x6a0 => layout.player_name,
        Target::MapPreview0x468 => layout.map_preview,
        Target::GameTypeLabel0x6ec => layout.right_panel_text.game_type,
        Target::ScenarioLabel0x5a8 => layout.right_panel_text.map_label,
        Target::StatusHelp0x695 => layout.status_help,
        _ => return None,
    })
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
        controller.ensure_active(DIALOG, false);
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
            // A scrolling Start list grows by its scrollbar, so the rows keep
            // the face's width for a caption (gamemd's nine starts never
            // scroll; ComboDropWin 0x0060D540 narrows the rows instead).
            (
                Prop::PopupWiden,
                Value::Int(if kind == 3 { scrollbar_w } else { 0 }),
            ),
            (Prop::PopupFlipBelow, Value::Int(flip)),
        ] {
            page::set(gui, root, node, prop, value);
        }
    }
    let mut captions = labels.checkboxes.iter();
    for (id, target) in CONTROLS {
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

/// What the page's widget events do to the session, in order; the button the
/// user activated. Cues go to [`SkirmishShellState::pending_ui_sounds`].
fn handle(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    events: Vec<PageEvent>,
) -> Option<SkirmishShellAction> {
    let mut action = None;
    for event in events {
        let sound = match event {
            PageEvent::Cue(PageCue::Button) => Some(SkirmishShellUiSound::MainButton),
            PageEvent::Cue(PageCue::Checkbox) => Some(SkirmishShellUiSound::GuiCheckboxSound),
            PageEvent::Cue(PageCue::ComboOpen) => Some(SkirmishShellUiSound::GuiComboOpenSound),
            PageEvent::Cue(PageCue::ComboClose) => Some(SkirmishShellUiSound::GuiComboCloseSound),
            PageEvent::Activated(id) => {
                action = match control(id) {
                    Some(Target::OwnerDrawButton(button)) => Some(match button {
                        OwnerDrawButton::StartGame0x617 => SkirmishShellAction::StartGame,
                        OwnerDrawButton::ChooseMap0x5aa => SkirmishShellAction::ChooseMap,
                        OwnerDrawButton::GameOptions => SkirmishShellAction::GameOptions,
                        OwnerDrawButton::Setups => SkirmishShellAction::Setups,
                        OwnerDrawButton::Back0x5c0 => SkirmishShellAction::BackOrExit,
                    }),
                    _ => action,
                };
                None
            }
            PageEvent::Toggled(id, on) => {
                if let Some(Target::Checkbox(checkbox)) = control(id) {
                    *checkbox_value(state, checkbox) = on;
                }
                None
            }
            PageEvent::Picked(id, row) => {
                let combo = combo_of(state, id).map(|(combo, _)| combo);
                let item = combo.and_then(|combo| {
                    combo_items(state, maps, combo)
                        .get(usize::try_from(row).ok()?)
                        .copied()
                });
                if let (Some(combo), Some(item)) = (combo, item) {
                    apply_combo_selection(state, combo, item);
                }
                None
            }
            // A pointer-driven change clicks (`0x0061E6DD`); the dialog has no
            // `WM_HSCROLL` case for the notification sent before it.
            PageEvent::Slid {
                id,
                value,
                final_: false,
            } => match control(id) {
                Some(Target::Trackbar(trackbar)) if set_trackbar_value(state, trackbar, value) => {
                    Some(SkirmishShellUiSound::GenericClick)
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(sound) = sound {
            state.push_ui_sound(sound);
        }
    }
    action
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

/// The pointer at `x`, `y` on the page: VERA's scrollbar beside the
/// computer rows takes a press no list holds and every move while it holds
/// the mouse; the widgets take the rest. Returns the button the user
/// activated; the cues wait in [`SkirmishShellState::pending_ui_sounds`].
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
) -> Option<SkirmishShellAction> {
    sync_page(controller, state, layout, maps, labels)?;
    match at {
        Pointer::Down => {
            let now = crate::util::time::Instant::now();
            if controller.gui().popup().is_none()
                && super::state::press_opponent_scrollbar(state, layout, maps, x, y, now)
            {
                return None;
            }
            controller.on_pointer_down(x, y, &[]);
        }
        Pointer::Move if state.opponent_scroll.captured() => {
            super::state::move_opponent_scrollbar(state, layout, maps, x, y);
        }
        Pointer::Move => controller.on_pointer_move(x, y, &[]),
        Pointer::Up => {
            let held = slider_held(controller.gui()) || state.opponent_scroll.captured();
            state.opponent_scroll.cancel();
            controller.on_pointer_up(x, y, &[]);
            // The freed hit test sees the pointer where it let go.
            if held {
                controller.on_pointer_move(x, y, &[]);
            }
        }
    }
    let raw = controller.drain_events();
    let events = page::page_events(controller.gui(), raw);
    handle(state, maps, events)
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
    match (combo_of(state, id), flag_slot(id)) {
        (Some((combo, _)), _) => Some(Target::ComboFace(combo)),
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
    let (id, _) = combo_of(state, u16::try_from(gui.id_of(node)?).ok()?)?;
    let list = gui.combo_drop_down(node)?;
    let (top_index, hot_row) = (list.popup.first, list.hot_row);
    Some(super::state::OpenComboDropdown {
        id,
        top_index,
        hot_row,
    })
}

/// The right-panel button that shows pushed: the one a press captured, under
/// the pointer or not (the page's buttons keep their pushed look until the
/// release).
pub fn pressed_button(gui: &Gui, root: NodeId) -> Option<OwnerDrawButton> {
    CONTROLS.iter().find_map(|(id, target)| match target {
        Target::OwnerDrawButton(button)
            if gui
                .find_id(root, (*id).into())
                .is_some_and(|node| gui.pressed() == Some(node)) =>
        {
            Some(*button)
        }
        _ => None,
    })
}
