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
use super::descriptor::{DialogId, HEADING_ANCHOR, MONITOR_ANCHOR};
use super::geom::{
    RectPx, RightPanelRects, SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW, SHELL_BASE_H, SHELL_BASE_W,
    ShellScale, center_offset, dlu_rect, lower_strip_rect, right_panel_rects,
    snap_button_biased_truncate,
};
use super::layout::{anchor_rect, status_line_rect};
use crate::ui::main_menu_shell::{
    MainMenuMovieBase, movie_base_for_responsive_width, movie_base_for_screen_width,
};

const RIGHT_PANEL_TILE_H: i32 = super::geom::RIGHT_PANEL_TILE_H;
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

/// Static description of one right-panel menu page dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPageSpec {
    pub dialog: DialogId,
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
    dialog: DialogId,
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
pub fn pressed_id(controller: &DialogController, dialog: DialogId) -> Option<u16> {
    node_id(controller, controller.pressed_node_in(dialog))
}

/// The control under the pointer in `dialog`'s live instance.
pub fn hovered_id(controller: &DialogController, dialog: DialogId) -> Option<u16> {
    node_id(controller, controller.hovered_node_in(dialog))
}

fn node_id(controller: &DialogController, node: Option<vera20k_gui::NodeId>) -> Option<u16> {
    u16::try_from(controller.gui().id_of(node?)?).ok()
}

/// The status help of the control under the pointer in `dialog`'s live
/// instance (enable-unfiltered).
pub fn hovered_help(controller: &DialogController, dialog: DialogId) -> Option<String> {
    super::page::hovered_help(controller.gui(), controller.hovered_node_in(dialog))
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

fn back_rect(screen_w: i32, panel: RightPanelRects) -> RectPx {
    let offset_x = center_offset(screen_w, SHELL_BASE_W);
    RectPx::new(
        screen_w - offset_x - SDBTNANM_CELL_W_NARROW,
        panel.tile.y + (panel.tile_count - 1).max(0) * RIGHT_PANEL_TILE_H,
        SDBTNANM_CELL_W_NARROW,
        SDBTNANM_CELL_H,
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
    let panel = right_panel_rects(screen_w, screen_h);
    // Both statics sit at the same template rects as in main menu 0xE2 (the
    // right-panel anchor reads only their size, not the DLU x).
    let title = anchor_rect(
        HEADING_ANCHOR,
        RectPx::new(425, 1, 108, 10),
        screen_w,
        screen_h,
    );
    let warning_monitor = anchor_rect(
        MONITOR_ANCHOR,
        RectPx::new(447, 29, 61, 33),
        screen_w,
        screen_h,
    );
    let mut buttons: Vec<MenuPageButtonRect> = spec
        .stacked
        .iter()
        .map(|button| MenuPageButtonRect {
            id: button.id,
            rect: snap_button_biased_truncate(
                screen_w,
                screen_h,
                dlu_rect(425, button.dlu_top, 108, 23),
                panel,
                SDBTNANM_CELL_W_NARROW,
            ),
        })
        .collect();
    buttons.push(MenuPageButtonRect {
        id: spec.back.id,
        rect: back_rect(screen_w, panel),
    });

    MenuPageLayout {
        scale: ShellScale::RETAIL,
        screen: RectPx::new(0, 0, screen_w, screen_h),
        movie_base,
        movie: RectPx::new(movie_x, movie_y, movie_w, movie_h),
        title,
        warning_monitor,
        status_help: status_line_rect(RectPx::new(2, 355, 303, 12), screen_w, screen_h),
        buttons,
        right_panel: panel,
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
        self.right_panel = scale.scale_panel(self.right_panel);
        for button in &mut self.buttons {
            button.rect = scale.scale_rect(button.rect);
        }
        self
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
        ShellScale::from_screen(screen_w as i32, screen_h as i32),
    )
}

/// The verified 800x600 page on `scale`'s mapping into its window: VERA's
/// game room's shell at the window's right edge
/// ([`ShellScale::anchored_right`]).
pub fn compute_layout_on(spec: &MenuPageSpec, scale: ShellScale) -> MenuPageLayout {
    compute_layout(spec, SHELL_BASE_W as u32, SHELL_BASE_H as u32).scaled_to(scale)
}
