//! The one painter of the shell's message and wait boxes
//! ([`crate::ui::shell::modal::ShellBox`]): the mode-2 PUDLGBGN panel
//! (`0x00621E90`), then the box's widgets from one tree of its nodes
//! ([`ShellBox::nodes`], [`ShellBoxTree`]) through [`super::gui_paint`]: the
//! body static (`0x5B0` left-top wrapped; the wait box's `0x5B2` is
//! SS_CENTER) and the MNBTTN owner-draw buttons (`0x00612B70`), each caption
//! centred on its button with the press sink ([`ButtonArt::ModalBox`]).

use vera20k_gui::{Gui, NodeId, NodeKind, Prop, Value};

use super::chrome::{push_rect_outline, push_solid_rect, type3_button_frames};
use super::gui_paint::{BoxStyle, ButtonArt, GuiPaint, GuiStyle, paint_gui_into};
use super::{
    OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
    SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_MODAL_PANEL_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{ModalDepths, ModalDraw, SHELL_TEXT_RGB_ENABLED, paint_modal_sprites};
use crate::render::shell_text::ShellTextDraw;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::ui::shell::modal::{BoxParts, ShellBox};

/// How a box sits in its host's paint: its art, its layers' depths (the
/// body's text may sit apart from the captions'), and the host's overlays
/// its texts give way to.
#[derive(Clone, Copy)]
pub(crate) struct BoxLook<'a> {
    pub(crate) background: Option<SkirmishShellChromeEntry>,
    pub(crate) depths: ModalDepths,
    pub(crate) body_depth: f32,
    pub(crate) covered: &'a [RectPx],
    /// A greyed button's caption colour: the type-3 buttons' packed
    /// `0x9F` red, or VERA's own lists' darker one where a box asks.
    pub(crate) disabled_rgb: [f32; 3],
}

impl BoxLook<'_> {
    pub(crate) fn new(background: Option<SkirmishShellChromeEntry>, depths: ModalDepths) -> Self {
        let body_depth = depths.text;
        BoxLook {
            background,
            depths,
            body_depth,
            covered: &[],
            disabled_rgb: SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
        }
    }

    /// The Skirmish family's boxes, over its open drop-down.
    pub(crate) fn skirmish(background: Option<SkirmishShellChromeEntry>) -> Self {
        BoxLook {
            background,
            depths: ModalDepths {
                background: SHELL_DROPDOWN_DEPTH - 0.00014,
                button: SHELL_DROPDOWN_DEPTH - 0.00016,
                text: SHELL_DROPDOWN_TEXT_DEPTH - 0.00013,
            },
            body_depth: SHELL_DROPDOWN_TEXT_DEPTH - 0.00012,
            covered: &[],
            disabled_rgb: SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
        }
    }

    /// [`Self::skirmish`] with the body at the captions' depth (the Choose
    /// Map and saved-file prompts).
    pub(crate) fn prompt(background: Option<SkirmishShellChromeEntry>) -> Self {
        let look = Self::skirmish(background);
        BoxLook {
            body_depth: look.depths.text,
            ..look
        }
    }
}

/// The one tree every box paints from: [`ShellBox::nodes`] under a dialog
/// root, kept on the box showing ([`Self::sync`]). Its nodes are rebuilt
/// only when the box's buttons change; texts, enables and rects are written
/// only when they change, so a steady box writes and allocates nothing.
pub(crate) struct ShellBoxTree {
    gui: Gui,
    root: NodeId,
}

/// The body, five buttons and the hidden IDOK under the root, with room.
const BOX_TREE_NODES: usize = 16;

impl Default for ShellBoxTree {
    fn default() -> Self {
        let mut gui = Gui::new(BOX_TREE_NODES);
        let root = gui
            .create(NodeKind::Dialog, None)
            .expect("an empty tree takes its root");
        Self { gui, root }
    }
}

impl ShellBoxTree {
    /// The root's `index`th child.
    fn child(&self, index: usize) -> Option<NodeId> {
        let children = self.gui.tree().children(self.root).ok()?;
        children.get(index).copied()
    }

    /// The tree holds `shell_box`'s nodes: the body, its buttons in order,
    /// then IDOK.
    fn holds(&self, shell_box: &ShellBox) -> bool {
        let count = self.gui.tree().children(self.root).map_or(0, <[NodeId]>::len);
        count == shell_box.buttons.len() + 2
            && shell_box.buttons.iter().enumerate().all(|(index, button)| {
                let node = self.child(index + 1);
                node.and_then(|node| self.gui.id_of(node)) == Some(i32::from(button.id))
            })
    }

    /// Puts `shell_box` on `parts` in the tree.
    fn sync(&mut self, shell_box: &ShellBox, parts: &BoxParts) {
        use crate::ui::shell::page;
        if !self.holds(shell_box) {
            while let Some(node) = self.child(0) {
                if self.gui.remove(node).is_err() {
                    break;
                }
            }
            for child in shell_box.nodes().children {
                let _ = self.gui.create_tree(&child, Some(self.root));
            }
        }
        if let Some(body) = self.child(0) {
            let align = [ShellAlign::NONE, ShellAlign::H_CENTER][usize::from(parts.centred)];
            let gui = &mut self.gui;
            page::set_node_text(gui, body, Prop::Text, &shell_box.body);
            page::set_if_changed(gui, body, Prop::Align, Value::Int(i32::from(align.0)));
        }
        for (index, button) in shell_box.buttons.iter().enumerate() {
            let Some(node) = self.child(index + 1) else {
                break;
            };
            let gui = &mut self.gui;
            page::set_node_text(gui, node, Prop::Text, &button.caption);
            page::set_if_changed(gui, node, Prop::Enabled, Value::Bool(button.enabled));
        }
        page::sync_rects(&mut self.gui, self.root, |id| parts.node_rect(id));
    }
}

/// `shell_box` on `parts` appended to `paint`: the panel's art, when
/// `atlas` holds it, then its widgets from [`ShellBoxTree`], `pressed`
/// telling which buttons show pushed. Without `atlas` only its texts paint.
fn paint_box_into(
    paint: &mut GuiPaint,
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: &dyn Fn(u16) -> bool,
    look: BoxLook<'_>,
) {
    if let Some(atlas) = atlas {
        paint.controls.extend(paint_modal_sprites(
            look.background,
            type3_button_frames(atlas),
            parts.dialog,
            &[],
            parts.scale,
            look.depths,
        ));
    }
    let Ok(mut tree) = state.frontend.shell_box_tree.lock() else {
        return;
    };
    tree.sync(shell_box, parts);
    let pressed_of = |id: i32| Some(u16::try_from(id).is_ok_and(pressed));
    let style = GuiStyle {
        buttons: ButtonArt::ModalBox,
        pressed_of: Some(&pressed_of),
        rgb: SHELL_TEXT_RGB_ENABLED,
        shell_box: Some(BoxStyle {
            depths: look.depths,
            body_depth: look.body_depth,
            disabled_rgb: look.disabled_rgb,
        }),
        ..GuiStyle::default()
    };
    let (gui, root) = (&tree.gui, tree.root);
    paint_gui_into(paint, state, atlas, gui, root, parts.scale, look.covered, style);
}

/// `shell_box` on `parts`, `pressed` telling which buttons show pushed.
/// Without `atlas` only its texts paint.
pub(crate) fn paint_shell_box(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: impl Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> ModalDraw {
    let mut paint = GuiPaint::default();
    paint_box_into(&mut paint, state, atlas, shell_box, parts, &pressed, look);
    ModalDraw {
        sprites: paint.controls,
        text: paint.texts,
    }
}

/// A Skirmish-family box's draws: its sprites, its texts, and its rect,
/// which the texts queued before them give up when the host asks
/// ([`Self::push_texts`]).
pub(crate) struct SkirmishBoxDraw {
    paint: GuiPaint,
    dialog: RectPx,
}

impl SkirmishBoxDraw {
    /// Its sprites after `out`'s.
    pub(crate) fn push_sprites(&mut self, out: &mut Vec<SpriteInstance>) {
        out.append(&mut self.paint.controls);
    }

    /// Its texts after `out`'s, which give up the box's rect first when
    /// `exclude_under`.
    pub(crate) fn push_texts(&mut self, out: &mut Vec<ShellTextDraw>, exclude_under: bool) {
        if exclude_under {
            super::text::exclude_text_under(out, self.dialog);
        }
        out.append(&mut self.paint.texts);
    }
}

/// A Skirmish-family box at retail size in a `w`x`h` window, after what
/// `paint` holds: over a solid panel when its art is missing.
fn skirmish_box_into(
    mut paint: GuiPaint,
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    (w, h): (i32, i32),
    shell_box: &ShellBox,
    pressed: &dyn Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> SkirmishBoxDraw {
    let parts = shell_box.parts(w, h, crate::ui::shell::geom::RETAIL_SCALE);
    if let (Some(atlas), None) = (atlas, look.background) {
        let (at, edge) = (parts.dialog, OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68);
        let out = &mut paint.controls;
        push_solid_rect(out, atlas, at, SHELL_MODAL_PANEL_RGB, look.depths.background);
        push_rect_outline(out, atlas, at, edge, SHELL_DROPDOWN_DEPTH - 0.00015);
    }
    paint_box_into(&mut paint, state, atlas, shell_box, &parts, pressed, look);
    SkirmishBoxDraw {
        paint,
        dialog: parts.dialog,
    }
}

/// A Skirmish-family box at retail size in a `w`x`h` window
/// ([`SkirmishBoxDraw`]).
pub(crate) fn skirmish_box(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    size: (i32, i32),
    shell_box: &ShellBox,
    pressed: impl Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> SkirmishBoxDraw {
    let paint = GuiPaint::default();
    skirmish_box_into(paint, state, atlas, size, shell_box, &pressed, look)
}

/// [`skirmish_box`]'s sprites into `sprites` and its texts into `texts`,
/// which give up the box's rect first when `exclude_under`.
pub(crate) fn push_skirmish_box(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    size: (i32, i32),
    shell_box: &ShellBox,
    pressed: impl Fn(u16) -> bool,
    look: BoxLook<'_>,
    sprites: &mut Vec<SpriteInstance>,
    (texts, exclude_under): (&mut Vec<ShellTextDraw>, bool),
) {
    let mut draw = skirmish_box(state, atlas, size, shell_box, pressed, look);
    draw.push_sprites(sprites);
    draw.push_texts(texts, exclude_under);
}

/// The window a Skirmish-family box centres in: the render size.
pub(crate) fn render_size(state: &AppState) -> (i32, i32) {
    (state.render_width() as i32, state.render_height() as i32)
}

/// Use Map's two-button box over the chooser. gamemd's eject-AI question
/// and VERA's random-map failure draw gamemd's art; VERA's map-library
/// download boxes Red Alert 2's, with how much has arrived under the text.
/// Its texts give up the box's rect.
pub(crate) fn choose_map_prompt_box(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    prompt: &crate::ui::skirmish_shell::ChooseMapPrompt,
) -> SkirmishBoxDraw {
    use crate::ui::skirmish_shell::ChooseMapPromptKind as Kind;
    let (body, ok) = super::text::choose_map_prompt_labels(state, prompt);
    let cancel = state.csf_label("GUI:Cancel", "Cancel");
    let shell_box = prompt.shell_box(body, ok, cancel);
    let background = atlas.and_then(|atlas| {
        match prompt.kind {
            Kind::EjectAi | Kind::RandomMapFailed => None,
            _ => atlas.ra2_dialog_background,
        }
        .or_else(|| atlas.mode_two_dialog_background(None))
    });
    let mut paint = GuiPaint::default();
    if let (Some(atlas), Kind::Downloading) = (atlas, prompt.kind)
        && let Some(progress) = crate::app::map_library::download_progress(state)
    {
        let (w, h) = render_size(state);
        let body = shell_box
            .parts(w, h, crate::ui::shell::geom::RETAIL_SCALE)
            .body;
        let depth = BoxLook::skirmish(None).depths.button;
        let out = &mut paint.controls;
        super::modals::push_download_progress_bar(out, atlas, body, progress, depth);
    }
    // Its widgets' captured button keeps its pushed look off the pointer too.
    let held =
        crate::ui::shell::menu_page::pressed_id(&state.frontend.shell_controller, shell_box.dialog);
    let pressed = |id| held == Some(id);
    let look = BoxLook::prompt(background);
    let size = render_size(state);
    skirmish_box_into(paint, state, atlas, size, &shell_box, &pressed, look)
}
