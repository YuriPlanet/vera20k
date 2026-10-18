//! In-game Framework-A gadget driver: owns the retained sidebar button list
//! (study §6.1 `ui::gadget`), builds/synchronizes it from the live
//! `SidebarView`, feeds it mouse-edge events plus one idle tick per frame,
//! applies fired button IDs onto existing app actions, and publishes the
//! transient pressed bits for the 5-frame visuals.
//!
//! Replaces fire-on-mouse-DOWN for tabs / repair / sell (study G22) and adds
//! the strip-scroll pair (mask 0x55 ⇒ no hold-repeat, one page per click, G23).
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use winit::event::MouseButton;

#[path = "command_bar_input.rs"]
mod command_bar;

use crate::app::AppState;
use crate::app::presentation::sidebar_render::current_sidebar_view;
use crate::ui::gadget::focus::FocusState;
use crate::ui::gadget::list::{GadgetBehavior, GadgetList, GadgetSpec, ToggleKind};
use crate::ui::gadget::tick::{GadgetInput, TickOutput, tick};
use crate::ui::gadget::{
    GadgetHandle, GadgetRect, KEY_LMB_DOWN, KEY_LMB_UP, KEY_RMB_DOWN, KEY_RMB_UP, ListId,
    MINIMAP_REGION_FLAGS, RESULT_BUTTON, RESULT_RIGHT, TACTICAL_REGION_FLAGS,
};
use crate::ui::sidebar::{self, SidebarAction, SidebarTab, SidebarView};

/// gamemd sidebar button IDs (study §2.5 live-population table; Kind/mask
/// identities VERIFIED-LIVE — decompile citation in the plan Sources section).
pub(crate) const ID_TAB_BASE: u16 = 0x00CB; // tabs 0xCB..=0xCE, Kind 2 latch-ON
pub(crate) const ID_REPAIR: u16 = 0x0065; // Kind 1 flip
pub(crate) const ID_SELL: u16 = 0x0066; // Kind 1 flip
pub(crate) const ID_SCROLL_DOWN: u16 = 0x00C9; // +1 page, Kind 0
pub(crate) const ID_DIPLOMACY: u16 = 0x00F2; // 653010, mask5, plain
pub(crate) const ID_OPTIONS: u16 = 0x00F3;
pub(crate) const ID_SCROLL_UP: u16 = 0x00C8; // −1 page, Kind 0
/// Cameo control id base (A2, study cameo lane §2): runtime id = 1000 + visible
/// slot index. Mirrors the gamemd id space and the A4 tooltip id base
/// (`tooltips::CAMEO_TIP_ID_BASE`).
pub(crate) const ID_CAMEO_BASE: u16 = 1000;
/// Sidebar control/dev button id base (A6): 4 slots in a fixed order —
/// cycle-owner, starter-base, spawn-test-units, producer. These have NO gamemd
/// counterpart (sandbox/control utilities); ids are app-local and chosen clear
/// of the tab/repair/sell/scroll/cameo ranges.
const ID_CONTROL_BASE: u16 = 0x0200;
/// Number of control-button slots (A6).
const CONTROL_SLOTS: usize = 4;
/// Control-button mask (A6): left + right press, fire-on-press, no held bits —
/// preserves the legacy "any click fires" behavior of `sidebar::hit_test`
/// (which ignored the right/left distinction for these buttons).
const CONTROL_FLAGS: u16 = 0x0011;
/// Scroll mask: presses + releases for BOTH buttons, no held bits — no
/// hold-repeat (G23); right-release fires `ID|0xC000`, consumer masks it off.
const SCROLL_FLAGS: u16 = 0x0055;
/// The single in-game gadget list (ListId uniqueness is app-owned).
const IN_GAME_LIST: ListId = ListId(1);

/// What the in-game gadget walk did with a mouse edge (A3 routing). The caller
/// (`input::dispatch::handle_mouse_input`) dispatches on this instead of a bare bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GadgetConsume {
    /// Nothing on the list consumed it — fall through to the legacy path.
    NotConsumed,
    /// A chrome button or cameo handled it — the caller returns.
    Consumed,
    /// The full-tactical catcher consumed it — run the tactical body.
    Tactical,
    /// The minimap region consumed it — run the minimap body.
    Minimap,
}

/// Stable handles of the 8 sidebar buttons, in retained order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SidebarButtonHandles {
    pub tabs: [GadgetHandle; 4],
    pub repair: GadgetHandle,
    pub sell: GadgetHandle,
    pub scroll_down: GadgetHandle,
    pub scroll_up: GadgetHandle,
    pub top: [GadgetHandle; 2],
}

/// Persistent driver state on `AppState`.
#[derive(Debug)]
pub(crate) struct InGameGadgets {
    pub list: GadgetList,
    pub focus: FocusState,
    /// Reused tick output (buffers cleared per tick, never reallocated).
    pub out: TickOutput,
    pub handles: Option<SidebarButtonHandles>,
    /// Cameo gadget pool (A2): grown to fit the visible cameo count, never
    /// auto-shrunk; the tail past `view.items.len()` is disabled. Index in this
    /// Vec == cameo slot index == `id - ID_CAMEO_BASE`.
    pub cameos: Vec<GadgetHandle>,
    /// Full-tactical catcher (A3): invisible sticky region over the play area.
    pub tactical: Option<GadgetHandle>,
    /// Minimap/radar click region (A3): invisible sticky region over the radar
    /// minimap; disabled when the radar is offline / minimap absent.
    pub minimap: Option<GadgetHandle>,
    /// Control/dev button gadgets (A6): fixed 6-slot set built once, synced from
    /// the view each frame (rect + presence); the optional pause/producer slots
    /// disable when absent. Slot order matches `apply_gadget_result`'s mapping.
    pub controls: Option<[GadgetHandle; CONTROL_SLOTS]>,
    pub command_bar: Option<[GadgetHandle; 12]>,
}

impl InGameGadgets {
    pub fn new() -> Self {
        Self {
            list: GadgetList::new(IN_GAME_LIST),
            focus: FocusState::new(),
            out: TickOutput::default(),
            handles: None,
            cameos: Vec::new(),
            tactical: None,
            minimap: None,
            controls: None,
            command_bar: None,
        }
    }

    fn is_cameo(&self, h: GadgetHandle) -> bool {
        self.cameos.contains(&h)
    }

    /// Cancel the retained driver's pointer gesture after the display loses
    /// capture. No delayed release may fire the pre-lock button. Toggle/mode
    /// state is synchronized separately from its existing sidebar owner.
    pub(crate) fn cancel_pointer_capture(&mut self) {
        if let Some(handle) = self.focus.sticky.take()
            && let Some(gadget) = self.list.get_mut(handle)
        {
            if let GadgetBehavior::Button(button) = &mut gadget.behavior {
                button.is_pressed = false;
            }
            gadget.is_to_redraw = true;
        }
        self.out.clear();
    }
}

/// A sidebar rect, in the sidebar's layout space, as a gadget rect in frame
/// pixels, where the cursor is.
fn rect_px(frame: sidebar::SidebarFrame, r: sidebar::Rect) -> GadgetRect {
    frame_rect_px(frame.rect_to_frame(r))
}

/// A frame-pixel rect, rounded to whole pixels.
fn frame_rect_px(r: sidebar::Rect) -> GadgetRect {
    GadgetRect::new(
        r.x.round() as i32,
        r.y.round() as i32,
        r.w.round() as i32,
        r.h.round() as i32,
    )
}

/// Build the list once (retained order = tabs 0..3, repair, sell, scroll-down,
/// scroll-up; rects are disjoint so relative order is unobservable today, and
/// this pins ONE order for hit priority + draw, study O7/G20), then re-sync
/// every tick: rects from the live view, disabled bits + is_on from app state
/// (the native external latch-on/latch-off equivalent — tabs are externally
/// driven, study §2.1).
fn sync_gadgets(state: &mut AppState, view: &SidebarView) {
    let frame = state.sidebar_frame();
    let tab_rects: Vec<GadgetRect> = view.tabs.iter().map(|t| rect_px(frame, t.rect)).collect();
    let tab_active: Vec<bool> = view.tabs.iter().map(|t| t.active).collect();
    let tab_disabled: Vec<bool> = view.tabs.iter().map(|t| t.disabled).collect();
    let repair_rect = rect_px(frame, view.repair_button.rect);
    let sell_rect = rect_px(frame, view.sell_button.rect);

    let gadgets = &mut state.match_state.match_presentation.in_game_gadgets;
    if gadgets.handles.is_none() {
        let list = &mut gadgets.list;
        let zero = GadgetRect::new(0, 0, 0, 0);
        let tabs = [0u16, 1, 2, 3].map(|i| {
            list.add_tail(GadgetSpec::button(
                zero,
                ID_TAB_BASE + i,
                ToggleKind::LatchOn,
            ))
        });
        let repair = list.add_tail(GadgetSpec::button(zero, ID_REPAIR, ToggleKind::Flip));
        let sell = list.add_tail(GadgetSpec::button(zero, ID_SELL, ToggleKind::Flip));
        let scroll_down = list.add_tail(
            GadgetSpec::button(zero, ID_SCROLL_DOWN, ToggleKind::Plain).with_flags(SCROLL_FLAGS),
        );
        let scroll_up = list.add_tail(
            GadgetSpec::button(zero, ID_SCROLL_UP, ToggleKind::Plain).with_flags(SCROLL_FLAGS),
        );
        let top = [ID_DIPLOMACY, ID_OPTIONS]
            .map(|id| list.add_tail(GadgetSpec::button(zero, id, ToggleKind::Plain).with_flags(5)));
        gadgets.handles = Some(SidebarButtonHandles {
            tabs,
            repair,
            sell,
            scroll_down,
            scroll_up,
            top,
        });
    }
    let handles = gadgets.handles.expect("built above");
    let sync = |list: &mut GadgetList, h: GadgetHandle, rect, disabled, is_on: Option<bool>| {
        if let Some(g) = list.get_mut(h) {
            g.rect = rect;
            g.is_disabled = disabled;
            if let (Some(on), GadgetBehavior::Button(b)) = (is_on, &mut g.behavior) {
                b.is_on = on;
            }
        }
    };
    for i in 0..4 {
        let rect = tab_rects
            .get(i)
            .copied()
            .unwrap_or(GadgetRect::new(0, 0, 0, 0));
        let active = tab_active.get(i).copied().unwrap_or(false);
        sync(
            &mut gadgets.list,
            handles.tabs[i],
            rect,
            tab_disabled.get(i).copied().unwrap_or(false),
            Some(active),
        );
    }
    sync(
        &mut gadgets.list,
        handles.repair,
        repair_rect,
        view.repair_button.disabled,
        Some(view.repair_button.active),
    );
    sync(
        &mut gadgets.list,
        handles.sell,
        sell_rect,
        view.sell_button.disabled,
        Some(view.sell_button.active),
    );
    sync(
        &mut gadgets.list,
        handles.scroll_down,
        rect_px(frame, view.scroll_down_button.rect),
        view.scroll_down_button.disabled,
        None,
    );
    sync(
        &mut gadgets.list,
        handles.scroll_up,
        rect_px(frame, view.scroll_up_button.rect),
        view.scroll_up_button.disabled,
        None,
    );
    for (h, button) in handles.top.into_iter().zip(&view.top_buttons) {
        sync(
            &mut gadgets.list,
            h,
            rect_px(frame, button.rect),
            button.disabled,
            None,
        );
    }
    sync_regions(state, view);
    sync_cameos(state, view);
    sync_controls(state, view);
    command_bar::sync(state);
}

/// Control/dev button gadgets (A6): build the fixed 6-slot set once, then sync
/// each slot's rect + presence from the view. Control behavior (fire-on-press,
/// post `id|0x8000`), mask 0x11 (left+right press), NOT sticky. Slot order is
/// fixed and mirrored by `apply_gadget_result`: 0 cancel, 1 cycle-owner,
/// 2 starter-base, 3 spawn-test-units, 4 pause, 5 producer. The optional
/// pause/producer slots disable when the view omits them. Appended after the
/// cameos — disjoint rects, so order is observationally irrelevant (O7/G20).
fn sync_controls(state: &mut AppState, view: &SidebarView) {
    let frame = state.sidebar_frame();
    if state
        .match_state
        .match_presentation
        .in_game_gadgets
        .controls
        .is_none()
    {
        let zero = GadgetRect::new(0, 0, 0, 0);
        let mut handles = [GadgetHandle(0); CONTROL_SLOTS];
        for (i, h) in handles.iter_mut().enumerate() {
            let mut spec = GadgetSpec::new(zero, CONTROL_FLAGS, false);
            spec.id = ID_CONTROL_BASE + i as u16;
            spec.behavior = GadgetBehavior::Control;
            *h = state
                .match_state
                .match_presentation
                .in_game_gadgets
                .list
                .add_tail(spec);
        }
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .controls = Some(handles);
    }
    let handles = state
        .match_state
        .match_presentation
        .in_game_gadgets
        .controls
        .expect("built above");
    let slots: [Option<GadgetRect>; CONTROL_SLOTS] = [
        Some(rect_px(frame, view.cycle_owner_button.rect)),
        Some(rect_px(frame, view.starter_base_button.rect)),
        Some(rect_px(frame, view.spawn_test_units_button.rect)),
        view.producer_button
            .as_ref()
            .map(|b| rect_px(frame, b.rect)),
    ];
    for (i, h) in handles.iter().enumerate() {
        if let Some(g) = state
            .match_state
            .match_presentation
            .in_game_gadgets
            .list
            .get_mut(*h)
        {
            match slots[i] {
                Some(r) => {
                    g.rect = r;
                    g.is_disabled = false;
                }
                None => g.is_disabled = true,
            }
        }
    }
}

/// A3 click regions: build the two invisible sticky catchers once (independent
/// of the chrome `handles` guard), then sync their rects each frame. List order
/// ends up chrome → tactical → minimap → cameos; all four groups occupy
/// DISJOINT rects (play area / radar panel / cameo strip / chrome), so the
/// relative order is observationally irrelevant — the broadcast walk's
/// first-consumer-by-rect and the smallest-area hover both resolve to the unique
/// containing gadget regardless (study tactical/minimap lanes §6/§4).
fn sync_regions(state: &mut AppState, _view: &SidebarView) {
    let frame = state.sidebar_frame();
    if state
        .match_state
        .match_presentation
        .in_game_gadgets
        .tactical
        .is_none()
    {
        let zero = GadgetRect::new(0, 0, 0, 0);
        let tac = state
            .match_state
            .match_presentation
            .in_game_gadgets
            .list
            .add_tail(GadgetSpec::click_region(zero, TACTICAL_REGION_FLAGS));
        let mini = state
            .match_state
            .match_presentation
            .in_game_gadgets
            .list
            .add_tail(GadgetSpec::click_region(zero, MINIMAP_REGION_FLAGS));
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .tactical = Some(tac);
        state.match_state.match_presentation.in_game_gadgets.minimap = Some(mini);
    }
    // Tactical catcher rect = the play area left of the sidebar panel (the Rust
    // equivalent of gamemd's g_RadarViewport*). Always enabled in-game — we have
    // no in-game map editor (gamemd registers it only when !g_IsMapEditor).
    let (tactical_width, tactical_height) =
        crate::app::input::camera::tactical_viewport_size(state);
    let play_rect = GadgetRect::new(0, 0, tactical_width as i32, tactical_height as i32);
    if let Some(th) = state
        .match_state
        .match_presentation
        .in_game_gadgets
        .tactical
        && let Some(g) = state
            .match_state
            .match_presentation
            .in_game_gadgets
            .list
            .get_mut(th)
    {
        g.rect = play_rect;
        g.is_disabled = false;
    }
    // Minimap region rect = the live minimap screen rect; disabled when the radar
    // is offline or the minimap is absent (the exact gate `is_cursor_over_minimap`
    // uses, `presentation::sidebar_render`).
    let minimap_available = state
        .match_state
        .match_presentation
        .radar_anim
        .as_ref()
        .is_none_or(|ra| ra.is_minimap_visible())
        && state.match_state.match_presentation.minimap.is_some();
    let mini_rect = rect_px(
        frame,
        crate::app::presentation::sidebar_render::active_minimap_content_screen_rect(state),
    );
    if let Some(mh) = state.match_state.match_presentation.in_game_gadgets.minimap
        && let Some(g) = state
            .match_state
            .match_presentation
            .in_game_gadgets
            .list
            .get_mut(mh)
    {
        g.rect = mini_rect;
        g.is_disabled = !minimap_available;
    }
}

/// Grow/sync the cameo gadget pool to mirror `view.items` (the already
/// visible/scrolled/per-tab cameo set, study cameo lane §5). One cameo gadget
/// per visible item, id = ID_CAMEO_BASE + slot, rect from the item; the unused
/// tail is disabled (skipped by hit-test + walk). Cameos are appended after the
/// chrome buttons — all groups occupy disjoint rects, so order is observationally
/// irrelevant (one pinned order for hit + draw, O7/G20). The pool grows on
/// demand and is never auto-shrunk; growth is gated to NOT run while a region
/// holds capture (defensive — cameos themselves are never sticky).
fn sync_cameos(state: &mut AppState, view: &SidebarView) {
    let frame = state.sidebar_frame();
    let want = view.items.len();
    if state
        .match_state
        .match_presentation
        .in_game_gadgets
        .cameos
        .len()
        < want
        && state
            .match_state
            .match_presentation
            .in_game_gadgets
            .focus
            .sticky
            .is_none()
    {
        let zero = GadgetRect::new(0, 0, 0, 0);
        while state
            .match_state
            .match_presentation
            .in_game_gadgets
            .cameos
            .len()
            < want
        {
            let slot = state
                .match_state
                .match_presentation
                .in_game_gadgets
                .cameos
                .len();
            let id = ID_CAMEO_BASE + slot as u16;
            let h = state
                .match_state
                .match_presentation
                .in_game_gadgets
                .list
                .add_tail(GadgetSpec::cameo(zero, id));
            state
                .match_state
                .match_presentation
                .in_game_gadgets
                .cameos
                .push(h);
        }
    }
    let cameos = state
        .match_state
        .match_presentation
        .in_game_gadgets
        .cameos
        .clone();
    for (slot, h) in cameos.iter().enumerate() {
        let rect = view.items.get(slot).map(|it| rect_px(frame, it.rect));
        if let Some(g) = state
            .match_state
            .match_presentation
            .in_game_gadgets
            .list
            .get_mut(*h)
        {
            match rect {
                Some(r) => {
                    g.rect = r;
                    g.is_disabled = false;
                }
                None => g.is_disabled = true, // unused tail
            }
        }
    }
}

/// Route a mouse press/release edge into the gadget tick. Returns true when
/// the substrate consumed the event — the caller must NOT fall through to the
/// legacy sidebar/minimap/selection paths. A release completing a captured
/// gesture is always consumed (in gamemd the sticky tier is exclusive: no
/// other gadget — including the tactical catcher — ever sees that event).
pub(crate) fn handle_mouse_button_event(
    state: &mut AppState,
    button: MouseButton,
    pressed: bool,
) -> GadgetConsume {
    if crate::app::input::campaign_input::is_locked(state) {
        return GadgetConsume::Consumed;
    }
    let Some(view) = current_sidebar_view(state).cloned() else {
        return GadgetConsume::NotConsumed;
    };
    sync_gadgets(state, &view);
    let key = match (button, pressed) {
        (MouseButton::Left, true) => KEY_LMB_DOWN,
        (MouseButton::Left, false) => KEY_LMB_UP,
        (MouseButton::Right, true) => KEY_RMB_DOWN,
        (MouseButton::Right, false) => KEY_RMB_UP,
        _ => return GadgetConsume::NotConsumed,
    };
    run_tick(state, &view, key)
}

/// Once-per-frame idle tick: drives the masked-0 sticky re-dispatch that pops
/// the pressed visual on drag-off and restores it on drag-back (G22 rows 2/3)
/// and would drive G23 hold-repeat for any future held-mask gadget.
/// Each unlocked pass also republishes the complete sidebar projection,
/// including the retained House power pair consumed by the tooltip driver.
pub(crate) fn idle_tick(state: &mut AppState) {
    let Some(view) = current_sidebar_view(state).cloned() else {
        return;
    };
    sync_gadgets(state, &view);
    if crate::app::input::campaign_input::is_locked(state) {
        publish_pressed_visuals(state);
        return;
    }
    let _ = run_tick(state, &view, 0);
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

fn run_tick(state: &mut AppState, view: &SidebarView, key: u16) -> GadgetConsume {
    // We tick synchronously on the edge, so event coords == live coords
    // (gamemd latches coords at enqueue; with no queue lag the two sources
    // are identical — G6 still selects per the key's low byte).
    let (cx, cy) = state.match_state.input.cursor_px();
    let input = GadgetInput {
        queued_key: key,
        event_x: cx,
        event_y: cy,
        mouse_x: cx,
        mouse_y: cy,
        left_held: state.match_state.input.tactical_mouse.left_held(),
        right_held: state.match_state.input.tactical_mouse.right_held(),
        shift: crate::app::input::dispatch::is_shift_held(state),
        ctrl: crate::app::input::dispatch::is_ctrl_held(state),
        alt: crate::app::input::dispatch::is_alt_held(state),
    };
    // The sticky tier dispatches the holder but does NOT set `consumed_by`, so
    // capture the pre-tick holder to route a captured drag/release back to its
    // region (study tactical lane §5).
    let prev_sticky = state
        .match_state
        .match_presentation
        .in_game_gadgets
        .focus
        .sticky;
    let gadgets = &mut state.match_state.match_presentation.in_game_gadgets;
    let result = tick(
        &mut gadgets.list,
        &mut gadgets.focus,
        &input,
        &mut gadgets.out,
    );
    let routed = state
        .match_state
        .match_presentation
        .in_game_gadgets
        .out
        .consumed_by
        .or(prev_sticky);
    let fired = (result & RESULT_BUTTON) != 0;
    if fired {
        apply_gadget_result(state, view, result);
    }
    publish_pressed_visuals(state);
    apply_cameo_hover_tooltip(state);
    classify(state, routed, fired)
}

/// Map the consuming / capture-holding handle to a routing class. Cameo fires
/// and chrome-button captures resolve to `Consumed` (caller returns); the
/// tactical / minimap regions resolve to their own class (caller runs the
/// matching body). A fired control always sets `consumed_by`, so a `None`
/// routed handle with `fired` cannot happen — handled defensively.
fn classify(state: &AppState, routed: Option<GadgetHandle>, fired: bool) -> GadgetConsume {
    let g = &state.match_state.match_presentation.in_game_gadgets;
    match routed {
        Some(h) if Some(h) == g.tactical => GadgetConsume::Tactical,
        Some(h) if Some(h) == g.minimap => GadgetConsume::Minimap,
        Some(_) => GadgetConsume::Consumed,
        None => {
            if fired {
                GadgetConsume::Consumed
            } else {
                GadgetConsume::NotConsumed
            }
        }
    }
}

/// G7 hover hook (study cameo lane §4): entering a cameo forces the tooltip
/// delay to 0 (cameo tips show immediately on the next mouse-move); leaving a
/// cameo for a non-cameo (or nothing) restores the default 1000 ms. The walk's
/// hover transition (`out.hover_entered`/`hover_left`) is the Mouse_Enter/Leave
/// edge — reproducing SelectClass::Mouse_Enter/Leave's save-and-zero / restore.
fn apply_cameo_hover_tooltip(state: &mut AppState) {
    let entered = state
        .match_state
        .match_presentation
        .in_game_gadgets
        .out
        .hover_entered;
    let left = state
        .match_state
        .match_presentation
        .in_game_gadgets
        .out
        .hover_left;
    let entered_cameo = entered.is_some_and(|h| {
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .is_cameo(h)
    });
    let left_cameo = left.is_some_and(|h| {
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .is_cameo(h)
    });
    if entered_cameo {
        state
            .match_state
            .match_presentation
            .tooltips
            .set_delay_override(Some(0));
    } else if left_cameo {
        // Left a cameo and did NOT enter another cameo this tick → restore.
        state
            .match_state
            .match_presentation
            .tooltips
            .set_delay_override(None);
    }
}

/// [AudioVisual] GUITabSound — played on every consumed tab click AND every
/// consumed strip-scroll click (even when the scroll is clamped at an end).
fn play_gui_tab_sound(state: &mut AppState) {
    let sound = state.rules().and_then(|r| r.general.gui_tab_sound.clone());
    crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

/// [AudioVisual] GUIMainButtonSound — the in-game Repair/Sell toggle click
/// sound (the same event the main-menu shell buttons play).
fn play_gui_main_button_sound(state: &mut AppState) {
    let sound = state
        .rules()
        .and_then(|r| r.general.gui_main_button_sound.clone());
    crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

/// Map a fired `ID|0x8000[|0x4000]` onto the existing app actions. Consumers
/// mask the right-release marker off (study §2.2: `key & ~0x4000`), so a
/// right-click scrolls identically.
fn apply_gadget_result(state: &mut AppState, view: &SidebarView, result: u16) {
    if command_bar::apply(state, result) {
        return;
    }
    let id = result & !(RESULT_BUTTON | RESULT_RIGHT);
    match id {
        _ if (ID_TAB_BASE..ID_TAB_BASE + 4).contains(&id) => {
            let tab = SidebarTab::all()[(id - ID_TAB_BASE) as usize];
            crate::app::input::dispatch::apply_sidebar_action(state, SidebarAction::SelectTab(tab));
            play_gui_tab_sound(state);
        }
        ID_DIPLOMACY => {
            crate::app::input::dispatch::apply_sidebar_action(state, SidebarAction::OpenDiplomacy);
        }
        ID_OPTIONS => {
            crate::app::input::dispatch::apply_sidebar_action(state, SidebarAction::OpenPauseMenu);
        }
        ID_REPAIR => {
            crate::app::input::dispatch::apply_sidebar_action(
                state,
                SidebarAction::ToggleRepairMode,
            );
            play_gui_main_button_sound(state);
        }
        ID_SELL => {
            crate::app::input::dispatch::apply_sidebar_action(state, SidebarAction::ToggleSellMode);
            play_gui_main_button_sound(state);
        }
        // One PAGE per click (G23: mask 0x55 has no held bits ⇒ no repeat).
        // Page = visible cameo rows; gamemd computes (strip px height)/50
        // which equals the visible row count. The click sound fires on every
        // consumed release, including clamped no-op scrolls at either end.
        ID_SCROLL_DOWN => {
            let page = view.layout.side2_tile_count.max(1);
            let presentation = &mut state.match_state.match_presentation;
            presentation.sidebar_projection.set_scroll_row(
                presentation.active_sidebar_tab,
                (view.scroll_rows + page).min(view.max_scroll_rows),
            );
            play_gui_tab_sound(state);
        }
        ID_SCROLL_UP => {
            let page = view.layout.side2_tile_count.max(1);
            let presentation = &mut state.match_state.match_presentation;
            presentation.sidebar_projection.set_scroll_row(
                presentation.active_sidebar_tab,
                view.scroll_rows.saturating_sub(page),
            );
            play_gui_tab_sound(state);
        }
        // Cameo press (A2): map the fired id back to its SidebarItem and run the
        // cameo action. `RESULT_RIGHT` (right-press marker) selects the right
        // button and Shift the ABANDON_ALL variant (`SelectClass::Action`
        // reads modifier bit 0 at `0x006AAD66`); the click decision plays
        // `GUIBuildSound` itself (`dispatch::sidebar_cameo_press`).
        _ if (ID_CAMEO_BASE..ID_CAMEO_BASE.saturating_add(view.items.len() as u16))
            .contains(&id) =>
        {
            let slot = (id - ID_CAMEO_BASE) as usize;
            if let Some(item) = view.items.get(slot) {
                let right = (result & RESULT_RIGHT) != 0;
                let shift = crate::app::input::dispatch::is_shift_held(state);
                let action = crate::ui::sidebar::hit_test_item(item, right, shift);
                crate::app::input::dispatch::apply_sidebar_action(state, action);
            }
        }
        // Control/dev button press (A6): apply the view button's own action.
        // Slot order matches `sync_controls`.
        _ if (ID_CONTROL_BASE..ID_CONTROL_BASE + CONTROL_SLOTS as u16).contains(&id) => {
            let action = match id - ID_CONTROL_BASE {
                0 => Some(view.cycle_owner_button.action.clone()),
                1 => Some(view.starter_base_button.action.clone()),
                2 => Some(view.spawn_test_units_button.action.clone()),
                3 => view.producer_button.as_ref().map(|b| b.action.clone()),
                _ => None,
            };
            if let Some(action) = action {
                crate::app::input::dispatch::apply_sidebar_action(state, action);
            }
        }
        _ => {}
    }
}

/// Publish the transient pressed bits for the 5-frame visuals (frames 3/4).
fn publish_pressed_visuals(state: &mut AppState) {
    command_bar::publish(state);
    let Some(handles) = state.match_state.match_presentation.in_game_gadgets.handles else {
        return;
    };
    let pressed = |h: GadgetHandle| {
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .list
            .get(h)
            .is_some_and(|g| matches!(g.behavior, GadgetBehavior::Button(b) if b.is_pressed))
    };
    let tabs = handles.tabs.map(pressed);
    let repair = pressed(handles.repair);
    let sell = pressed(handles.sell);
    let down = pressed(handles.scroll_down);
    let up = pressed(handles.scroll_up);
    let top = handles.top.map(pressed);
    let gs = &mut state.match_state.match_presentation.sidebar_gadget_state;
    gs.tab_pressed = tabs;
    gs.repair_pressed = repair;
    gs.sell_pressed = sell;
    gs.scroll_down_pressed = down;
    gs.scroll_up_pressed = up;
    gs.top_pressed = top;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_pointer_capture_cannot_fire_a_delayed_release() {
        let mut gadgets = InGameGadgets::new();
        let button = gadgets.list.add_tail(GadgetSpec::button(
            GadgetRect::new(0, 0, 20, 20),
            9,
            ToggleKind::Flip,
        ));
        let mut input = GadgetInput {
            queued_key: KEY_LMB_DOWN,
            event_x: 8,
            event_y: 8,
            mouse_x: 8,
            mouse_y: 8,
            left_held: true,
            ..Default::default()
        };
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input,
                &mut gadgets.out
            ),
            0
        );
        assert_eq!(gadgets.focus.sticky, Some(button));
        gadgets.cancel_pointer_capture();
        assert_eq!(gadgets.focus.sticky, None);
        assert!(gadgets.out.dispatches.is_empty());
        input.queued_key = KEY_LMB_UP;
        input.left_held = false;
        let release = tick(
            &mut gadgets.list,
            &mut gadgets.focus,
            &input,
            &mut gadgets.out,
        );
        assert_eq!(release & RESULT_BUTTON, 0);
        let GadgetBehavior::Button(state) = gadgets.list.get(button).unwrap().behavior else {
            panic!("retained button lost its behavior");
        };
        assert!(!state.is_pressed);
        assert!(!state.is_on);

        input.queued_key = KEY_LMB_DOWN;
        input.left_held = true;
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input,
                &mut gadgets.out
            ),
            0
        );
        input.queued_key = KEY_LMB_UP;
        input.left_held = false;
        assert_eq!(
            tick(
                &mut gadgets.list,
                &mut gadgets.focus,
                &input,
                &mut gadgets.out
            ),
            9 | RESULT_BUTTON
        );
    }
}
