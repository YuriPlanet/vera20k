//! The match's chrome on the GUI's chrome layer: the sidebar's tabs, mode
//! buttons, scroll pair, top buttons and cameos, the command bar, the timer
//! dock, the buttons a layout adds, and the battlefield's and the radar's
//! catchers, as nodes under the in-game dialogs of
//! [`crate::app::presentation::state::MatchPresentationState::dialog_ui`].
//! An in-game dialog over them takes the pointer. The chrome is built when
//! the match starts and stays through the in-game menus' resets of that
//! controller; the match's end takes it.
//!
//! The sidebar's and the command bar's buttons are also what draws them:
//! the sidebar's parts are a group of the layer, each button part the button
//! itself, each other part the panel its pictures go in, and the command bar
//! is a group of its rail and tiles, its buttons and its thumb
//! ([`ChromePaintNodes`], `presentation::picture_hud::ChromeHud`). The
//! tree's order is the pointer's alone, the gadget list's below; the paint
//! draws the parts in the layout's order (`spec.parts`) and the bar's
//! buttons in their slots', painting each part by itself, so a layout that
//! reorders its parts never changes which of two overlapping buttons takes
//! a press. A node keeps two rects: the one the cursor hits (`HitRect`,
//! frame pixels, written here) and the one its picture covers on its
//! surface (`Rect`, layout pixels, written by the paint).
//!
//! The layer's rules are the retained gadget list's (`vera20k_gui`'s
//! `Gui::push_chrome`): smallest-area hover seeded at 1024x768, the press to
//! the first node in tree order whose mask takes the button, sticky capture,
//! fire on release for the buttons (study G22), fire on the press for the
//! cameos (A2), and the two catchers (A3) handed back to the tactical and
//! minimap bodies unconsumed.
//!
//! Tree order is the gadget list's, after Phobos' superweapon column, whose
//! input ran before the list's: the column's toggle and buttons, the
//! layout's buttons (each added at the head, so the last first), the timer
//! dock, the sidebar's ten buttons (the tabs, Repair, Sell, the scroll
//! pair, Diplomacy, Options, whatever the layout's order of its parts), the
//! battlefield, the radar, the cameos, the command bar (its commands in
//! order, the thumb last).
//!
//! The nodes take their rects from the sidebar view, the frame and the
//! layout before each pointer edge. A change of places also updates hover
//! and the held button's pushed look under a stationary cursor.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use vera20k_gui::{
    DialogId, Gui, InputEvent, InputResult, MouseButton as GuiButton, NodeId, NodeKind, Prop, Rect,
    Refused, UiEvent, Value,
};
use winit::event::MouseButton;

#[path = "command_bar_input.rs"]
mod command_bar;
pub(crate) use command_bar::{places as command_bar_places, tooltips as command_bar_tooltips};

use crate::app::AppState;
use crate::app::PlatformState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::sidebar_render::current_sidebar_view;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::render::retained_hud::PICTURE;
use crate::ui::game_screen::GameScreen;
use crate::ui::sidebar::{self, SidebarAction, SidebarPart, SidebarTab, SidebarView};

/// gamemd sidebar button IDs (study §2.5 live-population table).
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
/// VERA-only local timer list; outside the native cameo/control ID ranges.
const ID_TIMER_DOCK: u16 = 0x0200;
/// The buttons a layout adds (`presentation::layout_parts`): one id each, in
/// the layout's order, below the tooltip ids at 999.
const ID_LAYOUT_BASE: u16 = 0x0300;
const LAYOUT_BUTTONS_MAX: usize = 0xE0;
/// Phobos control labels for diagnostics. A superweapon's action is resolved
/// from its GUI node, since a mod's added cameo slots can have the same id.
const ID_SW_TOGGLE: u16 = 0x0600;
const ID_SW_BASE: u16 = 0x0700;
/// Hit-test best-area seed: the fixed 1024×768 constants, NOT live
/// resolution (G14). A node larger than 786,432 px² never hovers.
pub(crate) const HIT_SEED_AREA: i64 = 1024 * 768;
/// The `Buttons` masks: mask 5 takes the left button's press and release,
/// 0x55 (the scroll pair, Team01..03) and the cameos' 0x19 and the catchers'
/// 0x7F / 0xDF both buttons'.
const LEFT: i32 = 1;
const BOTH: i32 = 3;

/// What the chrome did with a mouse edge. The caller
/// (`input::dispatch::handle_mouse_input`) dispatches on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChromeRoute {
    /// Nothing in the chrome took it.
    NotConsumed,
    /// A chrome button or cameo handled it — the caller returns.
    Consumed,
    /// The battlefield's catcher took it — run the tactical body.
    Tactical,
    /// The radar's catcher took it — run the minimap body.
    Minimap,
}

/// The chrome's nodes, in tree order.
#[derive(Debug)]
pub(crate) struct ChromeNodes {
    pub(crate) layer: DialogId,
    /// Which build of the chrome it is ([`ChromePaintNodes::tree`]).
    serial: u64,
    /// Phobos `ToggleSWButtonClass`: it listens to left press/release only
    /// and switches on the release; the right button does nothing over it.
    sw_toggle: NodeId,
    sw_group: NodeId,
    /// Phobos `SWButtonClass` per column button, by `sw_sidebar.entries`
    /// index: it fires on the left or right press. Grown, never shrunk.
    sw_buttons: Vec<NodeId>,
    layout_group: NodeId,
    /// The layout's added parts, a node each by part index, named by the
    /// part and of its kind, which client modules find and write (RFC 0001
    /// section 10): a part that is no button takes no pointer here. Grown
    /// to fit, never shrunk; made anew when a part's name or kind changes.
    pub(crate) layout_buttons: Vec<NodeId>,
    pub(crate) timer_dock: NodeId,
    /// Each sidebar part's node by `SidebarPart as usize`, in a group of
    /// the layer (its buttons in the gadget list's order, then the rest): a
    /// button or tab part is its button below, any other a panel its
    /// pictures go in.
    sidebar_parts: [NodeId; 19],
    pub(crate) tabs: [NodeId; 4],
    pub(crate) repair: NodeId,
    pub(crate) sell: NodeId,
    pub(crate) scroll_down: NodeId,
    pub(crate) scroll_up: NodeId,
    pub(crate) top: [NodeId; 2],
    /// Full-tactical catcher (A3): the play area left of the sidebar panel.
    pub(crate) tactical: NodeId,
    /// Minimap/radar catcher (A3); off while the radar is offline.
    pub(crate) minimap: NodeId,
    cameo_group: NodeId,
    /// Cameo pool (A2): index == slot == `id - ID_CAMEO_BASE`. Grown to fit
    /// the visible cameos, never shrunk; the tail past them takes no pointer.
    /// StripClass Activate 6A8330 / ActivateButtons 6A93F0 add a select
    /// button for every visible slot and Deactivate 6A83E0 / RemoveButtons
    /// 6A94B0 remove all 60; on an empty slot SelectClass::Action only sets
    /// the normal cursor (6AAD61) and passes flags 0 on (6AAD79 -> 6AB94F).
    pub(crate) cameos: Vec<NodeId>,
    /// The command bar's rail, caps and tiles, in a group of the layer
    /// before its buttons.
    bar_base: NodeId,
    /// Command `i`'s button, the thumb last.
    pub(crate) command_bar: [NodeId; 12],
}

/// What the sidebar's and the command bar's drawing reads of the chrome:
/// the nodes it draws into, each painted by itself in the drawing's order.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromePaintNodes {
    /// Which build of the chrome these are: a new one is other nodes.
    pub(crate) tree: u64,
    /// Each sidebar part's node by `SidebarPart as usize`, a button part's
    /// its button.
    pub(crate) sidebar_parts: [NodeId; 19],
    /// The bar's one part, its rail, caps and tiles.
    pub(crate) bar_parts: [NodeId; 1],
    /// Command `i`'s button, the thumb last.
    pub(crate) bar_buttons: [NodeId; 12],
}

impl ChromeNodes {
    #[cfg(feature = "phobos")]
    fn sw_index(&self, button: NodeId) -> Option<usize> {
        self.sw_buttons.iter().position(|&node| node == button)
    }

    fn paint(&self) -> ChromePaintNodes {
        ChromePaintNodes {
            tree: self.serial,
            sidebar_parts: self.sidebar_parts,
            bar_parts: [self.bar_base],
            bar_buttons: self.command_bar,
        }
    }
}

/// What places the nodes: a move re-reads the rects only when it changed.
/// The layout's added buttons count by their rects, which follow their own
/// live texts (credits, power), not by the parts' allocation.
#[derive(Debug, PartialEq)]
struct SyncKey {
    sw: SwPlaces,
    placement_revision: u64,
    frame: sidebar::SidebarFrame,
    command_bar_closed: bool,
    minimap: bool,
    timer: Option<Rect>,
    layout: Vec<Option<Rect>>,
    /// Each added part's name and node kind, for its node.
    layout_parts: Vec<(String, NodeKind)>,
}

/// The chrome's state on the match presentation.
#[derive(Debug, Default)]
pub(crate) struct InGameChrome {
    nodes: Option<ChromeNodes>,
    key: Option<SyncKey>,
    /// Last point routed to the chrome, cleared when it loses the pointer.
    hover_point: Option<(i32, i32)>,
    /// The node the last pointer button message reached: the one it fired,
    /// pressed or passed to the host, else the holder.
    pub(crate) last_route: Option<NodeId>,
    /// The node the last pointer button message fired, and whether by the
    /// right button.
    pub(crate) last_fired: Option<(NodeId, bool)>,
}

impl InGameChrome {
    /// The nodes while `gui` still holds them (a reset controller drops them).
    pub(crate) fn nodes<'a>(&'a self, gui: &Gui) -> Option<&'a ChromeNodes> {
        self.nodes
            .as_ref()
            .filter(|nodes| gui.chrome() == Some(nodes.layer))
    }
}

/// The match's chrome nodes and the GUI they live in.
pub(crate) fn chrome(presentation: &MatchPresentationState) -> Option<(&Gui, &ChromeNodes)> {
    let gui = presentation.dialog_ui.gui();
    Some((gui, presentation.in_game_chrome.nodes(gui)?))
}

fn button(gui: &mut Gui, parent: NodeId, id: u16, buttons: i32) -> Result<NodeId, Refused> {
    let node = gui.create(NodeKind::Button, Some(parent))?;
    gui.set(node, Prop::Id, Value::Int(i32::from(id)))?;
    gui.set(node, Prop::Buttons, Value::Int(buttons))?;
    gui.set(node, Prop::Enabled, Value::Bool(false))?;
    Ok(node)
}

/// An invisible sticky region the host handles (A3): it takes both
/// buttons, captures on a press and posts no id.
fn catcher(gui: &mut Gui, parent: NodeId) -> Result<NodeId, Refused> {
    let node = button(gui, parent, 0, BOTH)?;
    gui.set(node, Prop::Passthrough, Value::Bool(true))?;
    Ok(node)
}

/// A button that is also the picture its part draws: the paint gives it
/// its picture and its `Rect` (`presentation::picture_hud::ChromeHud`).
fn drawn_button(gui: &mut Gui, parent: NodeId, id: u16, buttons: i32) -> Result<NodeId, Refused> {
    let node = button(gui, parent, id, buttons)?;
    gui.set(node, Prop::Art, Value::Text(PICTURE.to_owned()))?;
    Ok(node)
}

/// The id and `Buttons` mask of a sidebar part that is a button.
fn part_button(part: SidebarPart) -> Option<(u16, i32)> {
    Some(match part {
        SidebarPart::Buildings => (ID_TAB_BASE, LEFT),
        SidebarPart::Defenses => (ID_TAB_BASE + 1, LEFT),
        SidebarPart::Infantry => (ID_TAB_BASE + 2, LEFT),
        SidebarPart::Vehicles => (ID_TAB_BASE + 3, LEFT),
        SidebarPart::Repair => (ID_REPAIR, LEFT),
        SidebarPart::Sell => (ID_SELL, LEFT),
        // The native sidebar init writes the 0x55 scroll mask over the
        // ctor's 5.
        SidebarPart::ScrollDown => (ID_SCROLL_DOWN, BOTH),
        SidebarPart::ScrollUp => (ID_SCROLL_UP, BOTH),
        SidebarPart::Diplomacy => (ID_DIPLOMACY, LEFT),
        SidebarPart::Options => (ID_OPTIONS, LEFT),
        _ => return None,
    })
}

/// The sidebar's buttons in the order the gadget list had them, which
/// gives the press where two overlap (the Soviet tabs, 32 wide, on the GDI
/// table's 29-pixel pitch), whatever order the layout draws them in.
const PRESS_ORDER: [SidebarPart; 10] = [
    SidebarPart::Buildings,
    SidebarPart::Defenses,
    SidebarPart::Infantry,
    SidebarPart::Vehicles,
    SidebarPart::Repair,
    SidebarPart::Sell,
    SidebarPart::ScrollDown,
    SidebarPart::ScrollUp,
    SidebarPart::Diplomacy,
    SidebarPart::Options,
];

/// The sidebar's group: a node per part, named by it, the buttons first in
/// [`PRESS_ORDER`], the panels after (they take no pointer). The paint
/// draws them in the layout's order, each by itself.
fn build_sidebar(gui: &mut Gui, root: NodeId) -> Result<[NodeId; 19], Refused> {
    let sidebar = gui.create(NodeKind::Panel, Some(root))?;
    let mut parts = [sidebar; 19];
    let panels = SidebarPart::STOCK_ORDER
        .into_iter()
        .filter(|part| part_button(*part).is_none());
    for part in PRESS_ORDER.into_iter().chain(panels) {
        let node = match part_button(part) {
            Some((id, buttons)) => drawn_button(gui, sidebar, id, buttons)?,
            None => gui.create(NodeKind::Panel, Some(sidebar))?,
        };
        gui.set(node, Prop::Name, Value::Text(part.name().to_owned()))?;
        parts[part as usize] = node;
    }
    Ok(parts)
}

/// Which build of the chrome is the newest: each is other nodes.
static BUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Pushes the chrome layer with its fixed nodes; the pools start empty.
fn build(gui: &mut Gui) -> Result<ChromeNodes, Refused> {
    let root = gui.create(NodeKind::Dialog, None)?;
    let layer = gui.push_chrome(root, HIT_SEED_AREA)?;
    let sw_toggle = button(gui, root, ID_SW_TOGGLE, LEFT)?;
    gui.set(sw_toggle, Prop::Opaque, Value::Bool(true))?;
    let sw_group = gui.create(NodeKind::Panel, Some(root))?;
    let layout_group = gui.create(NodeKind::Panel, Some(root))?;
    let timer_dock = button(gui, root, ID_TIMER_DOCK, LEFT)?;
    let sidebar_parts = build_sidebar(gui, root)?;
    let part = |part: SidebarPart| sidebar_parts[part as usize];
    let tabs = SidebarPart::TABS.map(part);
    let top = [part(SidebarPart::Diplomacy), part(SidebarPart::Options)];
    let tactical = catcher(gui, root)?;
    let minimap = catcher(gui, root)?;
    let cameo_group = gui.create(NodeKind::Panel, Some(root))?;
    let bar = gui.create(NodeKind::Panel, Some(root))?;
    gui.set(bar, Prop::Name, Value::Text("CommandBar".to_owned()))?;
    let bar_base = gui.create(NodeKind::Panel, Some(bar))?;
    let command_bar = command_bar::build(gui, bar)?;
    Ok(ChromeNodes {
        layer,
        serial: BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
        sw_toggle,
        sw_group,
        sw_buttons: Vec::new(),
        layout_group,
        layout_buttons: Vec::new(),
        timer_dock,
        sidebar_parts,
        tabs,
        repair: part(SidebarPart::Repair),
        sell: part(SidebarPart::Sell),
        scroll_down: part(SidebarPart::ScrollDown),
        scroll_up: part(SidebarPart::ScrollUp),
        top,
        tactical,
        minimap,
        cameo_group,
        cameos: Vec::new(),
        bar_base,
        command_bar,
    })
}

/// The chrome's nodes, built when the controller has none (a new match's
/// controller): `None` only if the node budget refuses them.
fn ensure<'a>(chrome: &'a mut InGameChrome, gui: &mut Gui) -> Option<&'a mut ChromeNodes> {
    if chrome.nodes(gui).is_none() {
        chrome.key = None;
        chrome.nodes = match build(gui) {
            Ok(nodes) => Some(nodes),
            Err(err) => {
                log::warn!("the match's chrome could not be built: {err:?}");
                None
            }
        };
    }
    chrome.nodes.as_mut()
}

/// Builds the match's chrome as the match starts; it then stays until the
/// match ends.
pub(crate) fn build_for_match(presentation: &mut MatchPresentationState) {
    ensure(
        &mut presentation.in_game_chrome,
        presentation.dialog_ui.gui_mut(),
    );
}

/// The chrome's nodes the sidebar and the command bar draw into, built if
/// the match's GUI has none.
pub(crate) fn paint_nodes(presentation: &mut MatchPresentationState) -> Option<ChromePaintNodes> {
    let gui = presentation.dialog_ui.gui_mut();
    Some(ensure(&mut presentation.in_game_chrome, gui)?.paint())
}

/// Puts `node`'s hit rect on `rect` taking the pointer, or takes it off the
/// pointer (`None`, or `on` false: a disabled gadget, skipped by hit and
/// walk). The node's `Rect` is where its picture draws, the paint's.
fn place(gui: &mut Gui, node: NodeId, rect: Option<Rect>, on: bool) {
    if let Some(rect) = rect {
        let _ = gui.set(node, Prop::HitRect, Value::Rect(rect));
    }
    let enabled = rect.is_some() && on;
    if gui.get(node, Prop::Enabled).ok().flatten() != Some(&Value::Bool(enabled)) {
        let _ = gui.set(node, Prop::Enabled, Value::Bool(enabled));
    }
}

/// A sidebar rect, in the sidebar's layout space, in frame pixels, where the
/// cursor is.
fn rect_px(frame: sidebar::SidebarFrame, r: sidebar::Rect) -> Rect {
    let [x, y, w, h] = frame.rect_to_frame_px(r);
    Rect::new(x, y, w, h)
}

/// Every rect the chrome takes, read from the match before the nodes are
/// written.
struct Places {
    /// The four tabs: rect and disabled.
    tabs: [Option<(Rect, bool)>; 4],
    repair: Rect,
    sell: Rect,
    scroll_down: (Rect, bool),
    scroll_up: (Rect, bool),
    top: [Option<(Rect, bool)>; 2],
    tactical: Rect,
    minimap: Option<Rect>,
    cameos: Vec<Rect>,
    command_bar: command_bar::CommandBarPlaces,
    timer: Option<Rect>,
}

/// The Phobos toggle and the buttons currently open on the battlefield.
#[derive(Debug, Default, PartialEq)]
struct SwPlaces {
    toggle: Option<Rect>,
    buttons: Vec<Rect>,
}

/// The column's places. The toggle takes nothing without its On or Off PCX
/// (`ToggleSWButtonClass::Draw` draws nothing and its `Action` ignores
/// input), and the columns are inert while collapsed.
#[cfg(feature = "phobos")]
fn sw_places(presentation: &MatchPresentationState) -> SwPlaces {
    let sw = &presentation.sw_sidebar;
    if !sw.ui.enabled {
        return SwPlaces::default();
    }
    let expanded = presentation.sidebar_projection.model.sw_sidebar_expanded();
    let has_toggle = crate::app::presentation::sw_sidebar::toggle_art(presentation).is_some();
    sw_layout_places(&sw.layout, expanded, has_toggle)
}

#[cfg(feature = "phobos")]
fn sw_layout_places(
    layout: &crate::ui::sw_sidebar::SwSidebarLayout,
    expanded: bool,
    has_toggle: bool,
) -> SwPlaces {
    SwPlaces {
        toggle: layout.toggle.filter(|_| has_toggle).map(cursor_rect),
        buttons: if expanded {
            layout
                .entries
                .iter()
                .map(|entry| cursor_rect(entry.rect))
                .collect()
        } else {
            Vec::new()
        },
    }
}

#[cfg(not(feature = "phobos"))]
fn sw_places(_: &MatchPresentationState) -> SwPlaces {
    SwPlaces::default()
}

/// A half-open layout rect as the whole cursor pixels it holds: those at
/// `x <= px < x + w`.
#[cfg_attr(not(feature = "phobos"), allow(dead_code))]
fn cursor_rect(r: sidebar::Rect) -> Rect {
    let (x, y) = (r.x.ceil() as i32, r.y.ceil() as i32);
    Rect::new(
        x,
        y,
        (r.x + r.w).ceil() as i32 - x,
        (r.y + r.h).ceil() as i32 - y,
    )
}

fn places(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
    view: &SidebarView,
) -> Places {
    let frame = crate::app::state::sidebar_frame(
        platform,
        persistence,
        renderer,
        &frontend.screen,
        &match_state.match_presentation.sidebar_layout_spec,
    );
    let tab = |i: usize| {
        view.tabs
            .get(i)
            .map(|t| (rect_px(frame, t.rect), t.disabled))
    };
    let top = |i: usize| {
        view.top_buttons
            .get(i)
            .map(|b| (rect_px(frame, b.rect), b.disabled))
    };
    // Tactical catcher rect = the play area left of the sidebar panel (the
    // Rust equivalent of gamemd's g_RadarViewport*). Always on in game — we
    // have no in-game map editor (gamemd registers it only when
    // !g_IsMapEditor).
    let (x, y, w, h) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    // Minimap region rect = the live minimap screen rect; off when the radar
    // is offline or the minimap is absent (the gate `is_cursor_over_minimap`
    // uses, `presentation::sidebar_render`).
    let minimap = minimap_available(&match_state.match_presentation).then(|| {
        rect_px(
            frame,
            crate::app::presentation::sidebar_render::active_minimap_content_screen_rect(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
            ),
        )
    });
    Places {
        tabs: [tab(0), tab(1), tab(2), tab(3)],
        repair: rect_px(frame, view.repair_button.rect),
        sell: rect_px(frame, view.sell_button.rect),
        scroll_down: (
            rect_px(frame, view.scroll_down_button.rect),
            view.scroll_down_button.disabled,
        ),
        scroll_up: (
            rect_px(frame, view.scroll_up_button.rect),
            view.scroll_up_button.disabled,
        ),
        top: [top(0), top(1)],
        tactical: Rect::new(x as i32, y as i32, w as i32, h as i32),
        minimap,
        cameos: view
            .items
            .iter()
            .map(|item| rect_px(frame, item.rect))
            .collect(),
        command_bar: command_bar::places(platform, renderer, frontend, match_state, persistence),
        timer: timer_rect(
            platform,
            renderer,
            &frontend.screen,
            &match_state.match_presentation,
            persistence,
        ),
    }
}

fn minimap_available(presentation: &MatchPresentationState) -> bool {
    crate::app::presentation::sidebar_render::minimap_shown(presentation)
        && presentation.minimap.is_some()
}

/// The timer dock's arrow: the dock keeps its areas in the pixels of the
/// timers' box, the battlefield's own in the original interface. A tiny
/// control before the full-tactical catcher, without catching timer text or
/// transparent space; no tooltip is registered for it.
fn timer_rect(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &MatchPresentationState,
    persistence: &PersistenceState,
) -> Option<Rect> {
    let rect = presentation.super_timer_dock.toggle_rect()?;
    let [left, top, _, _] = crate::app::presentation::field_parts::rect(
        platform,
        renderer,
        screen,
        presentation,
        persistence,
        crate::ui::field_layout::FieldPart::Timers,
    );
    Some(Rect::new(rect.x + left, rect.y + top, rect.w, rect.h))
}

/// Each added part's frame-pixel rect: none for a row, a column or a cameo
/// grid (which place or take their own) and for a part a client module hid.
fn layout_rects(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> Vec<Option<Rect>> {
    crate::app::presentation::layout_parts::placed(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    )
    .iter()
    .take(LAYOUT_BUTTONS_MAX)
    .map(|part| {
        (!part.group && !part.hidden).then(|| {
            let [x, y, w, h] = crate::app::presentation::layout_parts::click_rect(
                platform,
                renderer,
                &frontend.screen,
                &match_state.match_presentation,
                persistence,
                part,
            );
            Rect::new(x, y, w, h)
        })
    })
    .collect()
}

/// Each added part's name (its section's after the layout's) and the kind
/// of node it is.
fn layout_parts(presentation: &MatchPresentationState) -> Vec<(String, NodeKind)> {
    use crate::ui::added_parts::AddedKind;
    presentation
        .added_parts
        .iter()
        .take(LAYOUT_BUTTONS_MAX)
        .map(|part| {
            let name = part
                .section
                .split_once('.')
                .map_or(part.section.as_str(), |(_, name)| name);
            let kind = match part.kind {
                AddedKind::Button { .. } => NodeKind::Button,
                AddedKind::Text { .. } => NodeKind::Text,
                AddedKind::Image { .. } => NodeKind::Image,
                AddedKind::Bar => NodeKind::Bar,
                AddedKind::List => NodeKind::List,
                AddedKind::Group { .. } | AddedKind::CameoGrid { .. } => NodeKind::Panel,
            };
            (name.to_owned(), kind)
        })
        .collect()
}

/// The node of the layout's added part `index`, in the chrome's GUI.
pub(crate) fn layout_part_node(
    presentation: &MatchPresentationState,
    index: usize,
) -> Option<(&Gui, NodeId)> {
    let (gui, nodes) = chrome(presentation)?;
    Some((gui, *nodes.layout_buttons.get(index)?))
}

/// The node of the cameo grid that `view`'s cameo `slot` is in: a grid the
/// layout adds is its part's node, any other cameo the sidebar's own grid's
/// (the part `Cameos`). Client modules find either by its part name and
/// write its `ItemTooltips` (RFC 0001 appendix 7.10).
pub(crate) fn cameo_grid_node(
    presentation: &MatchPresentationState,
    view: &SidebarView,
    slot: usize,
) -> Option<NodeId> {
    let (_, nodes) = chrome(presentation)?;
    match view
        .added_grids
        .iter()
        .find(|grid| (grid.first..grid.first + grid.len).contains(&slot))
    {
        Some(grid) => nodes.layout_buttons.get(grid.part).copied(),
        None => Some(nodes.sidebar_parts[SidebarPart::Cameos as usize]),
    }
}

fn sync_key(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> SyncKey {
    let presentation = &match_state.match_presentation;
    SyncKey {
        sw: sw_places(&match_state.match_presentation),
        placement_revision: presentation.sidebar_projection.placement_revision(),
        frame: crate::app::state::sidebar_frame(
            platform,
            persistence,
            renderer,
            &frontend.screen,
            &match_state.match_presentation.sidebar_layout_spec,
        ),
        command_bar_closed: presentation.sidebar_gadget_state.command_bar_closed,
        minimap: minimap_available(&match_state.match_presentation),
        timer: timer_rect(
            platform,
            renderer,
            &frontend.screen,
            &match_state.match_presentation,
            persistence,
        ),
        layout: layout_rects(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
        ),
        layout_parts: layout_parts(&match_state.match_presentation),
    }
}

/// Builds the chrome when the controller has none and, when `force` or
/// what places it changed, puts its nodes where the match draws them.
/// False without a sidebar view: the chrome takes nothing then.
fn sync(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    force: bool,
) -> bool {
    let Some(view) = match_state.match_presentation.sidebar_projection.view() else {
        return false;
    };
    let presentation = &match_state.match_presentation;
    let built = presentation
        .in_game_chrome
        .nodes(presentation.dialog_ui.gui())
        .is_some();
    let key = sync_key(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    );
    let changed = !built || presentation.in_game_chrome.key.as_ref() != Some(&key);
    if changed || force {
        let places = places(platform, renderer, frontend, match_state, persistence, view);
        let presentation = &mut match_state.match_presentation;
        let gui = presentation.dialog_ui.gui_mut();
        let Some(nodes) = ensure(&mut presentation.in_game_chrome, gui) else {
            return false;
        };
        write_places(gui, nodes, &places, &key);
        presentation.in_game_chrome.key = Some(key);
    }
    let point = match_state.input.cursor_px();
    let presentation = &mut match_state.match_presentation;
    if let Some((hovered, held)) = presentation.in_game_chrome.refresh_hover(
        &mut presentation.dialog_ui,
        point,
        changed || force,
    ) {
        if held.is_some() {
            publish_pressed_visuals(&mut match_state.match_presentation);
        }
        apply_cameo_hover_tooltip(&mut match_state.match_presentation, hovered);
    }
    true
}

impl InGameChrome {
    /// Native G7/G22 run before dispatch even with a parked pointer. Skip
    /// the walk when neither its point nor its hit regions have changed.
    fn refresh_hover(
        &mut self,
        ui: &mut crate::ui::shell::controller::DialogController,
        point: (i32, i32),
        places_changed: bool,
    ) -> Option<(Option<NodeId>, Option<NodeId>)> {
        if ui.top_id().is_some() {
            self.hover_point = None;
            return None;
        }
        if !places_changed && self.hover_point == Some(point) {
            return None;
        }
        let layer = self.nodes(ui.gui())?.layer;
        let hovered = ui.gui().hovered_in(layer);
        let held = ui.gui().pressed_in(layer);
        let (x, y) = point;
        ui.chrome_input(InputEvent::PointerMove { x, y })?;
        self.hover_point = Some(point);
        Some((hovered, held))
    }
}

fn write_places(gui: &mut Gui, nodes: &mut ChromeNodes, places: &Places, key: &SyncKey) {
    write_sw(gui, nodes, &key.sw);
    for (node, tab) in nodes.tabs.iter().zip(places.tabs) {
        place(gui, *node, tab.map(|t| t.0), !tab.is_some_and(|t| t.1));
    }
    // Native never disables Repair/Sell (`SidebarGadgetState::repair_frame`).
    place(gui, nodes.repair, Some(places.repair), true);
    place(gui, nodes.sell, Some(places.sell), true);
    place(
        gui,
        nodes.scroll_down,
        Some(places.scroll_down.0),
        !places.scroll_down.1,
    );
    place(
        gui,
        nodes.scroll_up,
        Some(places.scroll_up.0),
        !places.scroll_up.1,
    );
    for (node, top) in nodes.top.iter().zip(places.top) {
        place(gui, *node, top.map(|t| t.0), !top.is_some_and(|t| t.1));
    }
    place(gui, nodes.tactical, Some(places.tactical), true);
    place(gui, nodes.minimap, places.minimap, true);
    write_cameos(gui, nodes, &places.cameos);
    command_bar::sync(gui, &nodes.command_bar, &places.command_bar);
    place(gui, nodes.timer_dock, places.timer, true);
    write_layout_buttons(gui, nodes, &key.layout, &key.layout_parts);
}

/// Phobos' column: the toggle, and the button pool grown to the column's
/// buttons (`SWButtonClass::Action` fires on the left or right press; the
/// press is consumed), the tail off the pointer.
fn write_sw(gui: &mut Gui, nodes: &mut ChromeNodes, places: &SwPlaces) {
    place(gui, nodes.sw_toggle, places.toggle, true);
    while nodes.sw_buttons.len() < places.buttons.len() {
        let index = nodes.sw_buttons.len();
        let Ok(node) = button(gui, nodes.sw_group, ID_SW_BASE + index as u16, BOTH) else {
            break;
        };
        let _ = gui.set(node, Prop::PressActivates, Value::Bool(true));
        let _ = gui.set(node, Prop::Opaque, Value::Bool(true));
        nodes.sw_buttons.push(node);
    }
    for (index, node) in nodes.sw_buttons.iter().enumerate() {
        place(gui, *node, places.buttons.get(index).copied(), true);
    }
}

/// Grow the cameo pool to the visible cameos (`view.items`, already per tab
/// and scrolled) and put each on its item; the unused tail takes no pointer.
fn write_cameos(gui: &mut Gui, nodes: &mut ChromeNodes, rects: &[Rect]) {
    while nodes.cameos.len() < rects.len() {
        let slot = nodes.cameos.len();
        let Ok(node) = button(gui, nodes.cameo_group, ID_CAMEO_BASE + slot as u16, BOTH) else {
            break;
        };
        // SelectClass mask 0x19 (Sidebar__InitializeStaticCameoControls
        // 6A4DC0): the cameo Action acts on the press bits and discards
        // LEFTUP; with no release bit it fires on mouse-DOWN and is not
        // sticky.
        let _ = gui.set(node, Prop::PressActivates, Value::Bool(true));
        nodes.cameos.push(node);
    }
    for (slot, node) in nodes.cameos.iter().enumerate() {
        place(gui, *node, rects.get(slot).copied(), true);
    }
}

/// One node per added part, in the layout group at the list's head so a
/// button over the battlefield comes before the full-tactical catcher. The
/// gadget driver added each at the head, so the last part comes first; a
/// pool that grows, or whose parts' names or kinds change, is laid out
/// again in that order. `parts` gives each part's name and kind (a part
/// past it is an unnamed button). Only a button takes the pointer here; any
/// other part's rect is where a client module's pointer events find it.
fn write_layout_buttons(
    gui: &mut Gui,
    nodes: &mut ChromeNodes,
    rects: &[Option<Rect>],
    parts: &[(String, NodeKind)],
) {
    let part = |index: usize| {
        parts
            .get(index)
            .map_or(("", NodeKind::Button), |(name, kind)| (name.as_str(), *kind))
    };
    let stale = nodes.layout_buttons.iter().enumerate().any(|(index, &node)| {
        let (name, kind) = part(index);
        let named = match gui.get(node, Prop::Name) {
            Ok(Some(Value::Text(own))) => own == name,
            Ok(_) => name.is_empty(),
            Err(_) => false,
        };
        index < parts.len() && (gui.tree().kind(node) != Ok(kind) || !named)
    });
    if nodes.layout_buttons.len() < rects.len() || stale {
        for node in nodes.layout_buttons.drain(..) {
            let _ = gui.remove(node);
        }
        let count = rects.len().max(parts.len());
        let mut made = Vec::with_capacity(count);
        for index in (0..count).rev() {
            let (name, kind) = part(index);
            let id = ID_LAYOUT_BASE + index as u16;
            let node = match kind {
                NodeKind::Button => button(gui, nodes.layout_group, id, LEFT),
                kind => gui.create(kind, Some(nodes.layout_group)).and_then(|node| {
                    gui.set(node, Prop::Id, Value::Int(i32::from(id)))?;
                    Ok(node)
                }),
            };
            let named = node.and_then(|node| {
                if !name.is_empty() {
                    gui.set(node, Prop::Name, Value::Text(name.to_owned()))?;
                }
                Ok(node)
            });
            match named {
                Ok(node) => made.push(node),
                Err(_) => break,
            }
        }
        made.reverse();
        // A pool cut short by the node budget keeps none of its tail.
        if made.len() == count {
            nodes.layout_buttons = made;
        } else {
            for node in made {
                let _ = gui.remove(node);
            }
        }
    }
    for (index, node) in nodes.layout_buttons.iter().enumerate() {
        let rect = rects.get(index).copied().flatten();
        if part(index).1 == NodeKind::Button {
            place(gui, *node, rect, true);
        } else if let Some(rect) = rect {
            // No pointer here: its enabled state is the client module's.
            let _ = gui.set(*node, Prop::HitRect, Value::Rect(rect));
        }
    }
}

/// Routes one pointer message to the chrome while no in-game dialog is up,
/// with its events, the hover before it and the holder before it.
fn route(
    presentation: &mut MatchPresentationState,
    event: InputEvent,
) -> Option<(InputResult, Vec<UiEvent>, Option<NodeId>, Option<NodeId>)> {
    let layer = presentation
        .in_game_chrome
        .nodes(presentation.dialog_ui.gui())?
        .layer;
    let gui = presentation.dialog_ui.gui();
    let (hovered, held) = (gui.hovered_in(layer), gui.pressed_in(layer));
    let (result, events) = presentation.dialog_ui.chrome_input(event)?;
    Some((result, events, hovered, held))
}

/// Route a mouse press/release edge to the chrome. A release completing a
/// captured gesture always goes to its holder (in gamemd the sticky tier is
/// exclusive: no other gadget — including the tactical catcher — sees it).
pub(crate) fn mouse_button(
    state: &mut AppState,
    button: MouseButton,
    pressed: bool,
) -> ChromeRoute {
    if crate::app::input::campaign_input::is_locked(&state.match_state) {
        return ChromeRoute::Consumed;
    }
    // VERA's radar order owns one button. The retail gadget's sticky byte
    // releases on either button, so keep unrelated edges out of the chrome
    // until the owning release; otherwise capture can escape to the map.
    if state.persistence.options_profile.right_click_orders
        && state
            .match_state
            .input
            .minimap_press_button()
            .is_some_and(|owner| owner != button)
    {
        return ChromeRoute::Consumed;
    }
    if !sync(
        &state.platform,
        &state.renderer,
        &state.frontend,
        &mut state.match_state,
        &state.process_assets,
        &state.persistence,
        true,
    ) {
        return ChromeRoute::NotConsumed;
    }
    let button = match button {
        MouseButton::Left => GuiButton::Left,
        MouseButton::Right => GuiButton::Right,
        _ => return ChromeRoute::NotConsumed,
    };
    // The edge is routed as it comes, so the event's point is the live
    // cursor (G6).
    let (x, y) = state.match_state.input.cursor_px();
    let event = match pressed {
        true => InputEvent::PointerDown { x, y, button },
        false => InputEvent::PointerUp { x, y, button },
    };
    let Some((result, events, hovered, held)) =
        route(&mut state.match_state.match_presentation, event)
    else {
        return ChromeRoute::NotConsumed;
    };
    let fired = events.iter().find_map(|event| match *event {
        UiEvent::Activated { node } => Some((node, false)),
        UiEvent::RightActivated { node } => Some((node, true)),
        _ => None,
    });
    let pressed_node = events.iter().find_map(|event| match *event {
        UiEvent::Pressed { node } => Some(node),
        _ => None,
    });
    let kept = &mut state.match_state.match_presentation.in_game_chrome;
    kept.last_route = result
        .passthrough
        .or(fired.map(|(node, _)| node))
        .or(pressed_node)
        .or(held);
    kept.last_fired = fired;
    let routed = classify(&state.match_state.match_presentation, result);
    if let Some((node, right)) = fired
        && !apply_sw(
            &mut state.platform,
            &state.renderer,
            &state.diag,
            &mut state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &mut state.persistence,
            node,
            right,
        )
        && let Some(id) =
            chrome(&state.match_state.match_presentation).and_then(|(gui, _)| gui.id_of(node))
    {
        apply_result(state, id as u16, right);
    }
    publish_pressed_visuals(&mut state.match_state.match_presentation);
    apply_cameo_hover_tooltip(&mut state.match_state.match_presentation, hovered);
    routed
}

/// Builds the chrome if needed and puts its nodes where the match draws them
/// now, for a reader outside the pointer path (a diagnostic receipt).
pub(crate) fn refresh(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) -> bool {
    sync(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        true,
    )
}

/// Reconcile the hit regions and pointer when either may have changed.
pub(crate) fn refresh_pointer(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
) {
    if !crate::app::input::campaign_input::is_locked(match_state) {
        sync(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            persistence,
            false,
        );
    }
}

/// Cancel the chrome's pointer gesture after the display loses capture or a
/// modal takes the pointer. No delayed release may fire the pre-lock button;
/// toggle/mode state stays with its sidebar owner.
pub(crate) fn cancel(presentation: &mut MatchPresentationState) {
    presentation.in_game_chrome.hover_point = None;
    if let Some(layer) = presentation
        .in_game_chrome
        .nodes(presentation.dialog_ui.gui())
        .map(|nodes| nodes.layer)
    {
        presentation.dialog_ui.gui_mut().drop_pointer_in(layer);
    }
    publish_pressed_visuals(presentation);
}

/// Map the passthrough or consumed result to a route. Cameo fires and
/// button captures are `Consumed` (the caller returns); the catchers run
/// their own body.
fn classify(presentation: &MatchPresentationState, result: InputResult) -> ChromeRoute {
    let Some((_, nodes)) = chrome(presentation) else {
        return ChromeRoute::NotConsumed;
    };
    match result.passthrough {
        Some(node) if node == nodes.tactical => ChromeRoute::Tactical,
        Some(node) if node == nodes.minimap => ChromeRoute::Minimap,
        Some(_) => ChromeRoute::Consumed,
        None if result.consumed => ChromeRoute::Consumed,
        None => ChromeRoute::NotConsumed,
    }
}

/// Whether a live control, including a layout button over the battlefield,
/// owns this frame pixel: what the chrome hovers there (G14). The full-map
/// tactical catcher is terrain.
pub(crate) fn control_at(presentation: &MatchPresentationState, x: i32, y: i32) -> bool {
    chrome(presentation).is_some_and(|(gui, nodes)| {
        gui.chrome_hit(x, y)
            .is_some_and(|node| node != nodes.tactical)
    })
}

/// The visible slot whose cameo the chrome hovers.
pub(crate) fn hovered_cameo(presentation: &MatchPresentationState) -> Option<usize> {
    let (gui, nodes) = chrome(presentation)?;
    let hovered = gui.hovered_in(nodes.layer)?;
    nodes.cameos.iter().position(|&node| node == hovered)
}

/// The `sw_sidebar.entries` index of the superweapon button the chrome
/// hovers.
#[cfg(feature = "phobos")]
pub(crate) fn hovered_sw_button(presentation: &MatchPresentationState) -> Option<usize> {
    let (gui, nodes) = chrome(presentation)?;
    let hovered = gui.hovered_in(nodes.layer)?;
    nodes.sw_index(hovered)
}

/// Whether the layout's added part `index` is a button held down now.
pub(crate) fn layout_button_pressed(presentation: &MatchPresentationState, index: usize) -> bool {
    chrome(presentation).is_some_and(|(gui, nodes)| {
        nodes
            .layout_buttons
            .get(index)
            .is_some_and(|&node| gui.shows_pressed_in(nodes.layer, node))
    })
}

/// Whether `node` is a button the side's layout added.
pub(crate) fn is_layout_button(presentation: &MatchPresentationState, node: NodeId) -> bool {
    chrome(presentation).is_some_and(|(_, nodes)| nodes.layout_buttons.contains(&node))
}

/// G7 hover hook (study cameo lane §4): entering a cameo forces the tooltip
/// delay to 0 (cameo tips show immediately on the next mouse-move); leaving a
/// cameo for a non-cameo (or nothing) restores the default 1000 ms —
/// SelectClass::Mouse_Enter/Leave's save-and-zero / restore.
fn apply_cameo_hover_tooltip(presentation: &mut MatchPresentationState, before: Option<NodeId>) {
    let Some((gui, nodes)) = chrome(presentation) else {
        return;
    };
    let after = gui.hovered_in(nodes.layer);
    if after == before {
        return;
    }
    let is_cameo = |node: Option<NodeId>| node.is_some_and(|node| nodes.cameos.contains(&node));
    let (entered_cameo, left_cameo) = (is_cameo(after), is_cameo(before));
    let tooltips = &mut presentation.tooltips;
    if entered_cameo {
        tooltips.set_delay_override(Some(0));
    } else if left_cameo {
        tooltips.set_delay_override(None);
    }
}

/// [AudioVisual] GUITabSound — played on every consumed tab click, including
/// a click on the tab that is already active (Sidebar Action `0x006A7A66`).
fn play_gui_tab_sound(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
) {
    crate::app::App::play_shell_rules_sound(
        frontend,
        match_state,
        process_assets,
        sfx_player,
        |rules| rules.general.gui_tab_sound.as_deref(),
    );
}

/// [AudioVisual] GUIMainButtonSound — the in-game Repair/Sell toggle click
/// sound (the same event the main-menu shell buttons play).
pub(crate) fn play_gui_main_button_sound(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
) {
    crate::app::App::play_shell_rules_sound(
        frontend,
        match_state,
        process_assets,
        sfx_player,
        |rules| rules.general.gui_main_button_sound.as_deref(),
    );
}

/// Map a fired control id onto the existing app actions; `right` is the
/// right-release (or right-press) marker, which a right-click scrolls with
/// identically (study §2.2: `key & ~0x4000`). The retained sidebar view the
/// press ran on is read before the action that may change it.
fn apply_result(state: &mut AppState, id: u16, right: bool) {
    if command_bar::apply(
        &mut state.platform,
        &mut state.renderer,
        &state.diag,
        &mut state.frontend,
        &mut state.match_state,
        &mut state.process_assets,
        &mut state.audio,
        &mut state.persistence,
        id,
        right,
    ) {
        return;
    }
    let (side2_tiles, cameos) = current_sidebar_view(&state.match_state.match_presentation)
        .map_or((0, 0), |view| {
            (view.layout.side2_tile_count, view.items.len())
        });
    // A built-in button the side's layout gives another `Command=` gives
    // that command, as a button the layout adds would; its look stays.
    let stock = match id {
        ID_DIPLOMACY => Some(sidebar::SidebarPart::Diplomacy),
        ID_OPTIONS => Some(sidebar::SidebarPart::Options),
        ID_REPAIR => Some(sidebar::SidebarPart::Repair),
        ID_SELL => Some(sidebar::SidebarPart::Sell),
        ID_SCROLL_DOWN => Some(sidebar::SidebarPart::ScrollDown),
        ID_SCROLL_UP => Some(sidebar::SidebarPart::ScrollUp),
        _ => None,
    };
    if let Some(command) = stock
        .and_then(|part| state.match_state.match_presentation.stock_parts.commands[part as usize])
    {
        crate::app::presentation::layout_parts::run(state, command, right);
        return;
    }
    match id {
        ID_TIMER_DOCK => state
            .match_state
            .match_presentation
            .super_timer_dock
            .toggle(),
        _ if (ID_LAYOUT_BASE..ID_LAYOUT_BASE + LAYOUT_BUTTONS_MAX as u16).contains(&id) => {
            crate::app::presentation::layout_parts::clicked(
                state,
                usize::from(id - ID_LAYOUT_BASE),
                right,
            );
        }
        _ if (ID_TAB_BASE..ID_TAB_BASE + 4).contains(&id) => {
            let tab = SidebarTab::all()[(id - ID_TAB_BASE) as usize];
            crate::app::input::dispatch::apply_sidebar_action(
                &mut state.platform,
                &state.renderer,
                &state.diag,
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &mut state.persistence,
                SidebarAction::SelectTab(tab),
            );
            play_gui_tab_sound(
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
            );
        }
        ID_DIPLOMACY => {
            crate::app::input::dispatch::apply_sidebar_action(
                &mut state.platform,
                &state.renderer,
                &state.diag,
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &mut state.persistence,
                SidebarAction::OpenDiplomacy,
            );
        }
        ID_OPTIONS => {
            crate::app::input::dispatch::apply_sidebar_action(
                &mut state.platform,
                &state.renderer,
                &state.diag,
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &mut state.persistence,
                SidebarAction::OpenPauseMenu,
            );
        }
        ID_REPAIR => {
            crate::app::input::dispatch::apply_sidebar_action(
                &mut state.platform,
                &state.renderer,
                &state.diag,
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &mut state.persistence,
                SidebarAction::ToggleRepairMode,
            );
            play_gui_main_button_sound(
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
            );
        }
        ID_SELL => {
            crate::app::input::dispatch::apply_sidebar_action(
                &mut state.platform,
                &state.renderer,
                &state.diag,
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &mut state.audio,
                &mut state.persistence,
                SidebarAction::ToggleSellMode,
            );
            play_gui_main_button_sound(
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
            );
        }
        // One page per click (mask 0x55 has no held bits ⇒ no repeat).
        // Sidebar Action 0x006A7780 queues the page on the active strip while
        // that direction has rows left and plays GUITabSound; at an end it
        // plays ScoldSound and queues nothing. StripClass::AI rolls the queue.
        ID_SCROLL_DOWN | ID_SCROLL_UP => {
            let presentation = &mut state.match_state.match_presentation;
            let queued = presentation.sidebar_projection.model.queue_scroll(
                presentation.active_sidebar_tab,
                id == ID_SCROLL_DOWN,
                side2_tiles,
                side2_tiles,
            );
            crate::app::App::play_shell_rules_sound(
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
                |rules| {
                    if queued {
                        rules.general.gui_tab_sound.as_deref()
                    } else {
                        rules.general.scold_sound.as_deref()
                    }
                },
            );
        }
        // Cameo press (A2): map the fired id back to its SidebarItem and run the
        // cameo action. The right marker selects the right button and Shift
        // the ABANDON_ALL variant (`SelectClass::Action` reads modifier bit 0
        // at `0x006AAD66`, the modifier word the press carries); the click
        // decision plays `GUIBuildSound` itself (`dispatch::sidebar_cameo_press`).
        _ if (ID_CAMEO_BASE..ID_CAMEO_BASE.saturating_add(cameos as u16)).contains(&id) => {
            let slot = (id - ID_CAMEO_BASE) as usize;
            let shift = crate::app::input::dispatch::is_shift_held(&state.match_state.input);
            let action = current_sidebar_view(&state.match_state.match_presentation)
                .and_then(|view| view.items.get(slot))
                .map(|item| crate::ui::sidebar::hit_test_item(item, right, shift));
            if let Some(action) = action {
                crate::app::input::dispatch::apply_sidebar_action(
                    &mut state.platform,
                    &state.renderer,
                    &state.diag,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    &mut state.persistence,
                    action,
                );
            }
        }
        _ => {}
    }
}

/// Phobos' column fired `node`: the toggle switches the column on its left
/// release, a button launches or selects its superweapon by the same
/// `apply_sidebar_action` path as the vanilla cameo. False when `node` is none
/// of the column's.
#[cfg(feature = "phobos")]
fn apply_sw(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &crate::app::diagnostics::state::DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut crate::app::audio_runtime::AppAudioRuntime,
    persistence: &mut PersistenceState,
    node: NodeId,
    right: bool,
) -> bool {
    let Some((_, nodes)) = chrome(&match_state.match_presentation) else {
        return false;
    };
    if node == nodes.sw_toggle {
        if !right {
            crate::app::presentation::sw_sidebar::toggle(&mut match_state.match_presentation);
            play_gui_main_button_sound(
                frontend,
                match_state,
                process_assets,
                &mut audio.sfx_player,
            );
        }
        return true;
    }
    let Some(index) = nodes.sw_index(node) else {
        return false;
    };
    let section = match_state
        .match_presentation
        .sw_sidebar
        .entries
        .get(index)
        .map(|entry| entry.type_id.clone());
    if let Some(section) = section {
        crate::app::input::dispatch::apply_sidebar_action(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            SidebarAction::SuperWeaponPress { section, right },
        );
    }
    true
}

#[cfg(not(feature = "phobos"))]
fn apply_sw(
    _: &mut PlatformState,
    _: &RendererState,
    _: &crate::app::diagnostics::state::DiagnosticsState,
    _: &mut FrontendState,
    _: &mut MatchState,
    _: &ProcessAssets,
    _: &mut crate::app::audio_runtime::AppAudioRuntime,
    _: &mut PersistenceState,
    _: NodeId,
    _: bool,
) -> bool {
    false
}

/// Publish the transient pushed looks of the buttons whose frame shows them
/// (`shape_button_frame`): the scroll pair, Diplomacy/Options and the
/// command bar. Without the chrome nothing shows pushed.
fn publish_pressed_visuals(presentation: &mut MatchPresentationState) {
    let looks = chrome(presentation).map(|(gui, nodes)| {
        let pressed = |node: NodeId| gui.shows_pressed_in(nodes.layer, node);
        (
            pressed(nodes.scroll_down),
            pressed(nodes.scroll_up),
            nodes.top.map(pressed),
            nodes.command_bar.map(pressed),
        )
    });
    let (down, up, top, bar) = looks.unwrap_or_default();
    command_bar::publish(presentation, bar);
    let gs = &mut presentation.sidebar_gadget_state;
    gs.scroll_down_pressed = down;
    gs.scroll_up_pressed = up;
    gs.top_pressed = top;
}

/// A GUI with the match's chrome built and no rects yet.
#[cfg(test)]
pub(crate) fn test_layer() -> (Gui, ChromeNodes) {
    let mut gui = Gui::new(1024);
    let nodes = build(&mut gui).expect("the chrome fits the budget");
    (gui, nodes)
}

/// The nodes the paint draws into.
#[cfg(test)]
pub(crate) fn paint_nodes_of(nodes: &ChromeNodes) -> ChromePaintNodes {
    nodes.paint()
}

/// Where `node` takes the pointer: its hit rect.
#[cfg(test)]
pub(crate) fn rect_of(gui: &Gui, node: NodeId) -> Option<Rect> {
    match gui.get(node, Prop::HitRect) {
        Ok(Some(Value::Rect(rect))) => Some(*rect),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn enabled(gui: &Gui, node: NodeId) -> bool {
    !matches!(gui.get(node, Prop::Enabled), Ok(Some(Value::Bool(false))))
}

/// A move to `x`, `y` and a `button` edge there on `gui`'s chrome: the
/// result and the `(id, right)` of what fired.
#[cfg(test)]
pub(crate) fn edge(
    gui: &mut Gui,
    x: i32,
    y: i32,
    button: GuiButton,
    pressed: bool,
) -> (InputResult, Vec<(u16, bool)>) {
    gui.input(InputEvent::PointerMove { x, y }, 0);
    let event = match pressed {
        true => InputEvent::PointerDown { x, y, button },
        false => InputEvent::PointerUp { x, y, button },
    };
    let result = gui.input(event, 0);
    let events = gui.drain_events();
    let fired = events
        .into_iter()
        .filter_map(|event| match event {
            UiEvent::Activated { node } => Some((node, false)),
            UiEvent::RightActivated { node } => Some((node, true)),
            _ => None,
        })
        .filter_map(|(node, right)| Some((u16::try_from(gui.id_of(node)?).ok()?, right)))
        .collect();
    (result, fired)
}

/// The cameo pool on `rects` and a press at `x`, `y`: the slot it fired and
/// whether by the right button (the cameo id's `1000 + slot`). The release
/// that follows fires nothing and passes on.
#[cfg(test)]
pub(crate) fn press_cameo(rects: &[Rect], x: i32, y: i32, right: bool) -> Option<(usize, bool)> {
    let (mut gui, mut nodes) = test_layer();
    write_cameos(&mut gui, &mut nodes, rects);
    let button = if right {
        GuiButton::Right
    } else {
        GuiButton::Left
    };
    let (result, fired) = edge(&mut gui, x, y, button, true);
    assert!(result.consumed && result.passthrough.is_none());
    assert_eq!(
        gui.pressed_in(nodes.layer),
        None,
        "a cameo takes no capture"
    );
    let (release, again) = edge(&mut gui, x, y, button, false);
    assert!(
        again.is_empty() && !release.consumed,
        "the UP edge fires no other entry"
    );
    let &[(id, right)] = fired.as_slice() else {
        return None;
    };
    Some((usize::from(id.checked_sub(ID_CAMEO_BASE)?), right))
}

#[cfg(all(test, tests_app_input_a_c))]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn timer_arrow_takes_its_click_ahead_of_tactical_orders() {
        let (mut gui, nodes) = test_layer();
        place(&mut gui, nodes.tactical, Some(r(0, 0, 800, 600)), true);
        place(&mut gui, nodes.timer_dock, Some(r(780, 500, 16, 14)), true);
        let (press, _) = edge(&mut gui, 785, 505, GuiButton::Left, true);
        assert_eq!(press.passthrough, None, "the arrow, not the battlefield");
        assert_eq!(gui.pressed_in(nodes.layer), Some(nodes.timer_dock));
        assert_eq!(
            edge(&mut gui, 785, 505, GuiButton::Left, false).1,
            [(ID_TIMER_DOCK, false)]
        );
        assert_eq!(gui.pressed_in(nodes.layer), None);
        place(&mut gui, nodes.timer_dock, None, true);
        assert!(!enabled(&gui, nodes.timer_dock));
    }

    /// A layout's button over the battlefield takes the click before the
    /// tactical catcher and gives its own id; a text part takes none.
    #[test]
    fn a_layout_button_takes_its_click_ahead_of_tactical_orders() {
        let (mut gui, mut nodes) = test_layer();
        place(&mut gui, nodes.tactical, Some(r(0, 0, 800, 600)), true);
        // The layout's first part is a text, its second a button.
        write_layout_buttons(&mut gui, &mut nodes, &[None, Some(r(20, 100, 60, 17))], &[]);
        let hit = |gui: &Gui, x, y| gui.chrome_hit(x, y).filter(|n| *n != nodes.tactical);
        assert!(hit(&gui, 25, 105).is_some());
        assert!(hit(&gui, 400, 300).is_none());
        let (press, _) = edge(&mut gui, 30, 108, GuiButton::Left, true);
        assert_eq!(press.passthrough, None);
        assert_eq!(gui.pressed_in(nodes.layer), Some(nodes.layout_buttons[1]));
        assert_eq!(
            edge(&mut gui, 30, 108, GuiButton::Left, false).1,
            [(ID_LAYOUT_BASE + 1, false)]
        );
        // Beside it the battlefield still takes the press.
        let (press, _) = edge(&mut gui, 300, 300, GuiButton::Left, true);
        assert_eq!(press.passthrough, Some(nodes.tactical));
        edge(&mut gui, 300, 300, GuiButton::Left, false);
        // A layout without the button leaves its node off the pointer.
        write_layout_buttons(&mut gui, &mut nodes, &[None, None], &[]);
        let (press, _) = edge(&mut gui, 30, 108, GuiButton::Left, true);
        assert_eq!(press.passthrough, Some(nodes.tactical));
    }

    /// The gadget driver added each layout button at the list's head: of
    /// two over each other the later part takes the press.
    #[test]
    fn a_later_layout_button_comes_first_as_the_gadget_list_had_it() {
        let (mut gui, mut nodes) = test_layer();
        write_layout_buttons(&mut gui, &mut nodes, &[Some(r(0, 0, 50, 20))], &[]);
        write_layout_buttons(
            &mut gui,
            &mut nodes,
            &[Some(r(0, 0, 50, 20)), Some(r(0, 0, 50, 20))],
            &[],
        );
        edge(&mut gui, 5, 5, GuiButton::Left, true);
        assert_eq!(gui.pressed_in(nodes.layer), Some(nodes.layout_buttons[1]));
    }

    #[test]
    fn cancelled_pointer_capture_cannot_fire_a_delayed_release() {
        let (mut gui, nodes) = test_layer();
        place(&mut gui, nodes.repair, Some(r(0, 0, 20, 20)), true);
        assert!(edge(&mut gui, 8, 8, GuiButton::Left, true).1.is_empty());
        assert_eq!(gui.pressed_in(nodes.layer), Some(nodes.repair));
        gui.drop_pointer_in(nodes.layer);
        assert_eq!(gui.pressed_in(nodes.layer), None);
        assert!(!gui.shows_pressed_in(nodes.layer, nodes.repair));
        assert!(edge(&mut gui, 8, 8, GuiButton::Left, false).1.is_empty());
        edge(&mut gui, 8, 8, GuiButton::Left, true);
        assert_eq!(
            edge(&mut gui, 8, 8, GuiButton::Left, false).1,
            [(ID_REPAIR, false)]
        );
    }

    #[test]
    fn cameos_fire_on_the_press_with_their_slot_and_button() {
        let rects = [r(600, 200, 60, 48), r(660, 200, 60, 48)];
        assert_eq!(press_cameo(&rects, 670, 210, false), Some((1, false)));
        assert_eq!(press_cameo(&rects, 610, 210, true), Some((0, true)));
    }

    #[test]
    fn a_parked_pointer_follows_new_cameos_without_transferring_its_held_press() {
        let mut ui = crate::ui::shell::controller::DialogController::default();
        let mut chrome = InGameChrome::default();
        let nodes = ensure(&mut chrome, ui.gui_mut()).unwrap();
        let (layer, repair) = (nodes.layer, nodes.repair);
        let rect = r(10, 20, 60, 48);
        place(ui.gui_mut(), repair, Some(rect), true);
        let point = (15, 25);
        chrome.refresh_hover(&mut ui, point, true);
        ui.chrome_input(InputEvent::PointerDown {
            x: 15,
            y: 25,
            button: GuiButton::Left,
        });
        assert!(ui.gui().shows_pressed_in(layer, repair));

        let nodes = ensure(&mut chrome, ui.gui_mut()).unwrap();
        place(ui.gui_mut(), repair, Some(r(90, 20, 60, 48)), true);
        write_cameos(ui.gui_mut(), nodes, &[rect]);
        let cameo = nodes.cameos[0];
        chrome.refresh_hover(&mut ui, point, true);
        assert_eq!(ui.gui().hovered_in(layer), Some(cameo));
        assert_eq!(ui.gui().pressed_in(layer), Some(repair));
        assert!(!ui.gui().shows_pressed_in(layer, repair));
        let (_, events) = ui
            .chrome_input(InputEvent::PointerUp {
                x: 15,
                y: 25,
                button: GuiButton::Left,
            })
            .unwrap();
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, UiEvent::Activated { .. }))
        );
    }

    #[test]
    fn the_cameo_pool_keeps_its_tail_off_the_pointer() {
        let (mut gui, mut nodes) = test_layer();
        write_cameos(&mut gui, &mut nodes, &[r(0, 0, 60, 48), r(60, 0, 60, 48)]);
        write_cameos(&mut gui, &mut nodes, &[r(0, 0, 60, 48)]);
        assert_eq!(nodes.cameos.len(), 2, "never shrunk");
        assert!(!enabled(&gui, nodes.cameos[1]));
        assert!(edge(&mut gui, 70, 5, GuiButton::Left, true).1.is_empty());
    }

    /// Phobos' column over the battlefield: a button fires on either press
    /// and swallows its release; the toggle switches on the left release
    /// over it only and the right button does nothing there; a collapsed
    /// column's buttons take nothing.
    #[test]
    fn the_superweapon_column_fires_its_buttons_on_the_press_and_its_toggle_on_the_release() {
        let (mut gui, mut nodes) = test_layer();
        place(&mut gui, nodes.tactical, Some(r(0, 0, 800, 600)), true);
        let column = |expanded: bool| SwPlaces {
            toggle: Some(r(0, 0, 10, 10)),
            buttons: if expanded {
                vec![r(10, 20, 60, 48)]
            } else {
                Vec::new()
            },
        };
        write_sw(&mut gui, &mut nodes, &column(true));
        let sw = ID_SW_BASE;
        assert_eq!(
            edge(&mut gui, 15, 25, GuiButton::Right, true).1,
            [(sw, true)]
        );
        let (release, fired) = edge(&mut gui, 15, 25, GuiButton::Right, false);
        assert!(fired.is_empty() && release.consumed && release.passthrough.is_none());
        assert_eq!(
            edge(&mut gui, 15, 25, GuiButton::Left, true).1,
            [(sw, false)]
        );
        edge(&mut gui, 15, 25, GuiButton::Left, false);
        // The toggle: silent press, the release over it switches.
        assert!(edge(&mut gui, 5, 5, GuiButton::Left, true).1.is_empty());
        assert_eq!(
            edge(&mut gui, 5, 5, GuiButton::Left, false).1,
            [(ID_SW_TOGGLE, false)]
        );
        // Let go off it, nothing switches.
        edge(&mut gui, 5, 5, GuiButton::Left, true);
        assert!(
            edge(&mut gui, 300, 300, GuiButton::Left, false)
                .1
                .is_empty()
        );
        // The right button over it reaches neither it nor the battlefield.
        let (press, fired) = edge(&mut gui, 5, 5, GuiButton::Right, true);
        assert!(fired.is_empty() && press.consumed && press.passthrough.is_none());
        let (release, fired) = edge(&mut gui, 5, 5, GuiButton::Right, false);
        assert!(fired.is_empty() && release.consumed && release.passthrough.is_none());
        // Collapsed, the button's place is the battlefield's.
        write_sw(&mut gui, &mut nodes, &column(false));
        let (press, _) = edge(&mut gui, 15, 25, GuiButton::Left, true);
        assert_eq!(press.passthrough, Some(nodes.tactical));
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn the_257th_drawn_superweapon_owns_its_press_over_the_battlefield() {
        let settings = crate::rules::phobos_sw_sidebar::SuperWeaponSidebarUi {
            enabled: true,
            pyramid: false,
            ..Default::default()
        };
        let weapons: Vec<_> = (0..257).map(|i| format!("Power{i}")).collect();
        let layout =
            crate::ui::sw_sidebar::compute_layout(settings, &weapons, 1080, 0, 0, None, true);
        let last = cursor_rect(layout.entries[256].rect);
        let (mut gui, mut nodes) = test_layer();
        place(&mut gui, nodes.tactical, Some(r(0, 0, 1920, 1080)), true);
        write_sw(
            &mut gui,
            &mut nodes,
            &sw_layout_places(&layout, true, false),
        );
        let press = gui.input(
            InputEvent::PointerDown {
                x: last.x,
                y: last.y,
                button: GuiButton::Left,
            },
            0,
        );
        assert!(press.consumed && press.passthrough.is_none());
        let fired = gui
            .drain_events()
            .into_iter()
            .find_map(|event| match event {
                UiEvent::Activated { node } => Some(node),
                _ => None,
            })
            .unwrap();
        assert_eq!(nodes.sw_index(fired), Some(256));

        // Layout-provided build grids can label a regular cameo with the same id.
        let cameo = button(&mut gui, nodes.cameo_group, ID_SW_BASE + 256, BOTH).unwrap();
        assert_eq!(gui.id_of(cameo), gui.id_of(fired));
        assert_eq!(nodes.sw_index(cameo), None);
    }

    #[test]
    fn a_layout_rect_holds_the_cursor_pixels_from_its_edge_to_before_its_far_edge() {
        let rect = |x, y, w, h| sidebar::Rect { x, y, w, h };
        assert_eq!(cursor_rect(rect(10.0, 20.0, 60.0, 48.0)), r(10, 20, 60, 48));
        // `x <= px < x + w` with whole cursor pixels: 11..=70 of 10.5..70.5.
        assert_eq!(cursor_rect(rect(10.5, 20.0, 60.0, 48.0)), r(11, 20, 60, 48));
    }

    /// The sidebar buttons' pictures as the retail art has them (each SHP's
    /// canvas, which the view sizes the button by): `TAB00..03`, `REPAIR`,
    /// `SELL`, `R-DN`/`R-UP` and `DIPLOBTN`/`OPTBTN`, the Allied art of
    /// SIDEC01.MIX and the Soviet and Yuri art of SIDEC02.MIX. The scroll
    /// pair's 46x25 is the stock Allied sidebar's, captured at 800x600, and
    /// is taken for SIDEC02's too.
    const NATIVE_SIZES: [(sidebar::SidebarTheme, NativeSizes); 3] = [
        (sidebar::SidebarTheme::Allied, ALLIED_SIZES),
        (sidebar::SidebarTheme::Soviet, SOVIET_SIZES),
        (sidebar::SidebarTheme::Yuri, SOVIET_SIZES),
    ];

    /// Tab, Repair, Sell, scroll pair, top pair.
    type NativeSizes = [[f32; 2]; 5];
    const ALLIED_SIZES: NativeSizes = [
        [28.0, 27.0],
        [64.0, 31.0],
        [64.0, 31.0],
        [46.0, 25.0],
        [72.0, 18.0],
    ];
    const SOVIET_SIZES: NativeSizes = [
        [32.0, 28.0],
        [52.0, 32.0],
        [52.0, 32.0],
        [46.0, 25.0],
        [72.0, 22.0],
    ];

    /// Where two sidebar buttons overlap (in the built-in layout and the
    /// sample's, with the art's own sizes: the Soviet tabs, 32 wide, on the
    /// GDI table's 29-pixel pitch overlap), the press goes to the first in
    /// the gadget list's order, whatever order the layout draws them in:
    /// the chrome's tree, which the press walks, never follows the paint.
    #[test]
    fn a_press_where_sidebar_buttons_overlap_goes_by_the_gadget_order() {
        use crate::ui::match_layout::MatchLayoutFile;
        use crate::ui::sidebar::{SidebarChromeLayoutSpec, SidebarFrame};
        let file = MatchLayoutFile::from_bytes(
            crate::test_fixture::text("docs/examples/veraui.ini").as_bytes(),
        );
        let mut overlaps = 0;
        for (theme, [tab, repair, sell, scroll, top]) in NATIVE_SIZES {
            for side in [None, Some("GDI"), Some("Nod"), Some("ThirdSide")] {
                for gdi_positions in [true, false] {
                    let mut spec =
                        SidebarChromeLayoutSpec::for_theme_and_positions(theme, gdi_positions);
                    let layout = side.and_then(|side| file.for_side(Some(side), gdi_positions));
                    if let Some(layout) = layout {
                        layout.apply_to_sidebar(&mut spec);
                    }
                    let frame = SidebarFrame::for_layout(800, 600, 100, &spec);
                    let view = sidebar::build_sidebar_view_with_spec(
                        spec,
                        frame,
                        SidebarTab::Building,
                        0,
                        0,
                        0,
                        Some(tab),
                        &[],
                        &[],
                        &[],
                        None,
                        None,
                        &[],
                        &sidebar::gadget_flash::SidebarGadgetState::new(),
                        Some(repair),
                        Some(sell),
                        Some(scroll),
                        Some(scroll),
                        [Some(top); 2],
                        &sidebar::cameo_order::CameoStrips::default(),
                    );
                    let (mut gui, nodes) = test_layer();
                    let hits: Vec<(NodeId, Rect)> = PRESS_ORDER
                        .into_iter()
                        .filter_map(|part| {
                            let rect = rect_px(frame, view.button(part)?.0);
                            (rect.w > 0 && rect.h > 0)
                                .then_some((nodes.sidebar_parts[part as usize], rect))
                        })
                        .collect();
                    for &(node, rect) in &hits {
                        place(&mut gui, node, Some(rect), true);
                    }
                    let case = format!("{theme:?} {side:?} gdi={gdi_positions}");
                    for (at, &(_, rect)) in hits.iter().enumerate() {
                        for &(_, other) in &hits[at + 1..] {
                            let both = rect.intersect(other);
                            if both.w <= 0 || both.h <= 0 {
                                continue;
                            }
                            overlaps += 1;
                            let first = hits
                                .iter()
                                .find(|(_, rect)| rect.contains(both.x, both.y))
                                .map(|(node, _)| *node);
                            edge(&mut gui, both.x, both.y, GuiButton::Left, true);
                            assert_eq!(gui.pressed_in(nodes.layer), first, "{case}");
                            edge(&mut gui, both.x, both.y, GuiButton::Left, false);
                        }
                    }
                }
            }
        }
        assert!(overlaps > 0, "the Soviet tabs overlap on the GDI table");
    }

    #[test]
    fn the_radar_catcher_frees_its_capture_on_the_right_release() {
        let (mut gui, nodes) = test_layer();
        place(&mut gui, nodes.minimap, Some(r(600, 20, 160, 120)), true);
        let (press, _) = edge(&mut gui, 650, 50, GuiButton::Right, true);
        assert_eq!(press.passthrough, Some(nodes.minimap));
        let (release, _) = edge(&mut gui, 650, 50, GuiButton::Right, false);
        assert_eq!(release.passthrough, Some(nodes.minimap));
        assert_eq!(gui.pressed_in(nodes.layer), None);
    }
}
