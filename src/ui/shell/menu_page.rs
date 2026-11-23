//! Shared layout for the RA2TS-bearing right-panel menu pages.
//!
//! Single Player `0x100` and Movies & Credits `0x101` are the same dialog
//! shape in gamemd.exe: a `0x71A` RA2TS movie static, heading `0x694`, status
//! help `0x695`, three stacked owner-draw buttons at DLU tops 122/149/176 and a
//! Back button on the final right-panel tile. Only the control identities,
//! labels and dialog-proc results differ, so each page is a `MenuPageSpec`
//! table over this one layout. The buttons live as `vera20k-gui` nodes under
//! the page's dialog ([`sync_nodes`]), which own their press, hover and
//! enable state.

use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

use super::controller::DialogController;
use super::descriptor::{
    AnchorRule, BACK_BUTTON, BUTTON, DialogResourceId, HEADING_ANCHOR, MONITOR_ANCHOR,
};
use super::geom::{
    RectPx, RightPanelRects, SHELL_BASE_H, SHELL_BASE_W, ScaledRects, ShellScale, center_offset,
    lower_strip_rect, right_panel_rects,
};
use super::layout::Placer;
use crate::ui::main_menu_shell::{
    MainMenuMovieBase, movie_base_for_responsive_width, movie_base_for_screen_width,
};

const RA2TS_L_W: i32 = 632;
const RA2TS_L_H: i32 = 570;
const RA2TS_S_W: i32 = 472;
const RA2TS_S_H: i32 = 450;

/// One owner-draw button of a menu page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPageButtonSpec {
    /// Win32 control resource id.
    pub id: u16,
    /// Resource-template DLU top. Stacked buttons snap from it; the Back
    /// button occupies the final tile and ignores it.
    pub dlu_top: i32,
    /// Caption CSF key from the dialog template.
    pub csf_key: &'static str,
    /// Status-help CSF key written to static `0x695` on hover.
    pub tooltip_key: &'static str,
    /// Constant value the dialog proc writes through `GetWindowLong(hwnd, 8)`,
    /// or `None` when it writes run-time data instead.
    pub result: Option<i32>,
}

impl MenuPageButtonSpec {
    /// A button whose dialog proc writes run-time data, or nothing.
    pub const fn new(
        id: u16,
        dlu_top: i32,
        csf_key: &'static str,
        tooltip_key: &'static str,
    ) -> Self {
        Self {
            id,
            dlu_top,
            csf_key,
            tooltip_key,
            result: None,
        }
    }

    /// The same button at template top `dlu_top`.
    pub const fn at(self, dlu_top: i32) -> Self {
        Self { dlu_top, ..self }
    }

    /// The same button writing the constant `result`.
    pub const fn returning(self, result: i32) -> Self {
        Self {
            result: Some(result),
            ..self
        }
    }
}

/// Static description of one right-panel menu page dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPageSpec {
    pub dialog: DialogResourceId,
    /// Heading static `0x694` caption.
    pub title_key: &'static str,
    /// Stacked right-panel buttons in resource order.
    pub stacked: &'static [MenuPageButtonSpec],
    /// Bottom-row Back button (`0x686` in both pages).
    pub back: MenuPageButtonSpec,
}

impl MenuPageSpec {
    /// Every owner-draw button, stacked first, then Back.
    pub fn buttons(&self) -> impl Iterator<Item = &MenuPageButtonSpec> {
        self.stacked.iter().chain(std::iter::once(&self.back))
    }

    pub fn button(&self, id: u16) -> Option<&MenuPageButtonSpec> {
        self.buttons().find(|button| button.id == id)
    }

    /// The page's buttons as nodes: resource id, caption and status help.
    pub fn nodes(&self) -> NodeSpec {
        NodeSpec::new(NodeKind::Dialog).children(self.buttons().map(|b| {
            NodeSpec::new(NodeKind::Button)
                .id(i32::from(b.id))
                .label(b.csf_key)
                .tooltip(b.tooltip_key)
        }))
    }
}

/// A control the pointer hovers for its status help while its own state
/// machine takes the press (`0x94`'s emblems and slider): a disabled button.
pub fn hover_target(id: u16, help_key: &str) -> NodeSpec {
    NodeSpec::new(NodeKind::Button)
        .id(i32::from(id))
        .tooltip(help_key)
        .with(Prop::Enabled, Value::Bool(false))
}

/// Builds `nodes` once under `dialog`'s live instance and keeps them on
/// `rect_of` (a control without a rect is hidden), with each listed control's
/// runtime enable state (a disabled control still hovers).
pub fn sync_nodes(
    controller: &mut DialogController,
    dialog: DialogResourceId,
    nodes: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
    enabled: &[(u16, bool)],
) {
    let Some(root) = controller.root_in(dialog) else {
        return;
    };
    let gui = controller.gui_mut();
    super::page::ensure_built(gui, root, nodes);
    super::page::sync_rects(gui, root, rect_of);
    for &(id, on) in enabled {
        super::page::set(gui, root, id, Prop::Enabled, Value::Bool(on));
    }
}

/// The control `dialog`'s live instance holds captured.
pub fn pressed_id(controller: &DialogController, dialog: DialogResourceId) -> Option<u16> {
    node_id(controller, controller.pressed_node_in(dialog))
}

/// The control under the pointer in `dialog`'s live instance.
pub fn hovered_id(controller: &DialogController, dialog: DialogResourceId) -> Option<u16> {
    node_id(controller, controller.hovered_node_in(dialog))
}

fn node_id(controller: &DialogController, node: Option<vera20k_gui::NodeId>) -> Option<u16> {
    u16::try_from(controller.gui().id_of(node?)?).ok()
}

/// The status help of the control under the pointer in `dialog`'s live
/// instance (enable-unfiltered). A trackbar holding the mouse keeps its own:
/// no hit test runs until its release (`0x00622CCB`).
pub fn hovered_help(controller: &DialogController, dialog: DialogResourceId) -> Option<String> {
    let gui = controller.gui();
    let held = controller
        .pressed_node_in(dialog)
        .filter(|node| gui.tree().kind(*node) == Ok(NodeKind::Trackbar));
    super::page::hovered_help(gui, held.or(controller.hovered_node_in(dialog)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPageButtonRect {
    pub id: u16,
    pub rect: RectPx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPageLayout {
    /// How far these rects sit from the 800x600 art they were authored against.
    /// `RETAIL` on the retail layout at every screen size.
    pub scale: ShellScale,
    pub screen: RectPx,
    pub movie_base: MainMenuMovieBase,
    pub movie: RectPx,
    /// Heading static `0x694` paint rect.
    pub title: RectPx,
    /// Static `0x71C` window rect (the SDWRNANM monitor).
    pub warning_monitor: RectPx,
    pub status_help: RectPx,
    /// Stacked buttons in spec order, then Back.
    pub buttons: Vec<MenuPageButtonRect>,
    pub right_panel: RightPanelRects,
    pub lower_strip: RectPx,
}

fn movie_origin(screen_w: i32, screen_h: i32) -> (i32, i32) {
    (
        center_offset(screen_w, SHELL_BASE_W),
        center_offset(screen_h, SHELL_BASE_H),
    )
}

pub fn compute_layout(spec: &MenuPageSpec, screen_w: u32, screen_h: u32) -> MenuPageLayout {
    let screen_w = screen_w as i32;
    let screen_h = screen_h as i32;
    let movie_base = movie_base_for_screen_width(screen_w as u32);
    let (movie_x, movie_y) = movie_origin(screen_w, screen_h);
    let (movie_w, movie_h) = match movie_base {
        MainMenuMovieBase::Ra2tsS => (RA2TS_S_W, RA2TS_S_H),
        MainMenuMovieBase::Ra2tsL => (RA2TS_L_W, RA2TS_L_H),
    };
    let place = Placer::new(screen_w, screen_h);
    let stacked = spec.stacked.iter().map(|button| MenuPageButtonRect {
        id: button.id,
        rect: place.at(BUTTON, 425, button.dlu_top, 108, 23),
    });
    let back = MenuPageButtonRect {
        id: spec.back.id,
        rect: place.rule(BACK_BUTTON),
    };

    MenuPageLayout {
        scale: crate::ui::shell::geom::RETAIL_SCALE,
        screen: RectPx::new(0, 0, screen_w, screen_h),
        movie_base,
        movie: RectPx::new(movie_x, movie_y, movie_w, movie_h),
        // Both statics sit at the same template rects as in main menu 0xE2
        // (the right-panel anchor reads only their size, not the DLU x).
        title: place.at(HEADING_ANCHOR, 425, 1, 108, 10),
        warning_monitor: place.at(MONITOR_ANCHOR, 447, 29, 61, 33),
        status_help: place.at(AnchorRule::StatusLine, 2, 355, 303, 12),
        buttons: stacked.chain(std::iter::once(back)).collect(),
        right_panel: right_panel_rects(screen_w, screen_h),
        lower_strip: lower_strip_rect(screen_w, screen_h),
    }
}

impl MenuPageLayout {
    pub fn button_rect(&self, id: u16) -> Option<RectPx> {
        self.buttons
            .iter()
            .find(|button| button.id == id)
            .map(|button| button.rect)
    }

    /// Map the verified 800x600 page into the live window through one uniform
    /// scale, centred. Identity at the base size, so the responsive page equals
    /// the retail page there. The 640-wide asset switch belongs to the unscaled
    /// layout; a scaled page always uses the large loop.
    fn scaled_to(mut self, scale: ShellScale) -> Self {
        let screen = scale.window();
        self.scale = scale;
        self.screen = screen;
        self.movie_base = movie_base_for_responsive_width(screen.w as u32);
        self.movie = scale.scale_rect(self.movie);
        self.title = scale.scale_rect(self.title);
        self.warning_monitor = scale.scale_rect(self.warning_monitor);
        self.status_help = scale.scale_rect(self.status_help);
        self.lower_strip = scale.scale_rect(self.lower_strip);
        self.right_panel = self.right_panel.scaled(scale);
        self.buttons = self.buttons.scaled_rects(scale);
        self
    }
}

vera20k_gui::scaled_rects!(MenuPageButtonRect { rect; keep: id });

impl ScaledRects for MenuPageLayout {
    fn scaled_rects(&self, scale: ShellScale) -> Self {
        self.clone().scaled_to(scale)
    }
}

/// The responsive variant of [`compute_layout`]: the verified 800x600 page
/// scaled to fit the window, keeping its shape.
pub fn compute_responsive_layout(
    spec: &MenuPageSpec,
    screen_w: u32,
    screen_h: u32,
) -> MenuPageLayout {
    compute_layout_on(
        spec,
        crate::ui::shell::geom::shell_scale(screen_w as i32, screen_h as i32),
    )
}

/// The page on `scale`'s mapping into a `screen_w`x`screen_h` window: the
/// retail page for the retail mapping, the scaled 800x600 page otherwise.
pub fn compute_layout_for(
    spec: &MenuPageSpec,
    scale: ShellScale,
    screen_w: i32,
    screen_h: i32,
) -> MenuPageLayout {
    if scale.is_unscaled() {
        compute_layout(spec, screen_w as u32, screen_h as u32)
    } else {
        compute_layout_on(spec, scale)
    }
}

/// The verified 800x600 page on `scale`'s mapping into its window: VERA's
/// game room's shell at the window's right edge
/// ([`ShellScale::anchored_right`]).
pub fn compute_layout_on(spec: &MenuPageSpec, scale: ShellScale) -> MenuPageLayout {
    compute_layout(spec, SHELL_BASE_W as u32, SHELL_BASE_H as u32).scaled_to(scale)
}
