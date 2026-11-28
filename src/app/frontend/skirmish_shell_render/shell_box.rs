//! The one painter of the shell's message and wait boxes
//! ([`crate::ui::shell::modal::ShellBox`]): the mode-2 PUDLGBGN panel
//! (`0x00621E90`), then the box's widgets, from the tree its input hits
//! ([`ShellBox::sync`] puts the box on it first), through
//! [`super::gui_paint`]: the
//! body static (`0x5B0` left-top wrapped; the wait box's `0x5B2` is
//! SS_CENTER) and the MNBTTN owner-draw buttons (`0x00612B70`), each caption
//! centred on its button with the press sink ([`ButtonArt::ModalBox`]).

use vera20k_gui::{Gui, NodeId};

use super::chrome::{push_rect_outline, push_solid_rect, type3_button_frames};
use super::gui_paint::{BoxStyle, ButtonArt, GuiStyle, paint_gui_into};
use super::{
    OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
    SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_MODAL_PANEL_RGB,
};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_paint::{
    ModalDepths, ModalDraw, SHELL_TEXT_RGB_ENABLED, paint_modal_sprites,
};
use crate::render::shell_text::ShellTextDraw;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::game_screen::GameScreen;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{BoxParts, ShellBox, body_node};

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

/// A box's widgets ([`ShellBox::sync`]): the gui of the controller its input
/// hits and the dialog root they hang under; none while its dialog is not up.
pub(crate) type BoxTree<'a> = Option<(&'a Gui, NodeId)>;

/// `dialog`'s widgets in `ui`'s live instance.
pub(crate) fn controller_tree(ui: &DialogController, dialog: DialogResourceId) -> BoxTree<'_> {
    ui.root_in(dialog).map(|root| (ui.gui(), root))
}

/// `shell_box` on `parts` appended to `paint`: the panel's art, when
/// `atlas` holds it, then its widgets from `tree` (body, then its buttons),
/// `pressed` telling which buttons show pushed. Without `atlas` only its
/// texts paint.
#[allow(clippy::too_many_arguments)]
fn paint_box_into(
    paint: &mut HudPaint,
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    tree: BoxTree<'_>,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: &dyn Fn(u16) -> bool,
    look: BoxLook<'_>,
) {
    if let Some(atlas) = atlas {
        paint.draw(Material::Controls, |out, _| {
            out.extend(paint_modal_sprites(
                look.background,
                type3_button_frames(atlas),
                parts.dialog,
                &[],
                parts.scale,
                look.depths,
            ))
        });
    }
    let Some((gui, root)) = tree else {
        return;
    };
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
    let (scale, covered) = (parts.scale, look.covered);
    if let Some(body) = body_node(gui, root) {
        paint_gui_into(
            paint,
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            atlas,
            gui,
            body,
            scale,
            covered,
            style,
        );
    }
    // Only the box's own buttons, each once: a host's dialog may hold more
    // (a list, the verify's boxes), and the hidden IDOK may share an id.
    let mut painted = 0u64;
    for &node in gui.tree().children(root).unwrap_or_default() {
        let id = gui.id_of(node).and_then(|id| u16::try_from(id).ok());
        let index = id.and_then(|id| shell_box.buttons.iter().position(|b| b.id == id));
        let Some(bit) = index.and_then(|index| 1u64.checked_shl(index as u32)) else {
            continue;
        };
        if painted & bit == 0 {
            painted |= bit;
            paint_gui_into(
                paint,
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                atlas,
                gui,
                node,
                scale,
                covered,
                style,
            );
        }
    }
}

/// `shell_box` on `parts` from its widgets in `tree`, `pressed` telling
/// which buttons show pushed. Without `atlas` only its texts paint.
pub(crate) fn paint_shell_box(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    tree: BoxTree<'_>,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: impl Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> ModalDraw {
    let mut paint = HudPaint::default();
    paint_box_into(
        &mut paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        tree,
        shell_box,
        parts,
        &pressed,
        look,
    );
    ModalDraw {
        sprites: paint.take_lane(Material::Controls),
        text: paint.texts,
    }
}

/// A Skirmish-family box's draws: its sprites, its texts, and its rect,
/// which the texts queued before them give up when the host asks
/// ([`Self::push_texts`]).
pub(crate) struct SkirmishBoxDraw {
    paint: HudPaint,
    dialog: RectPx,
}

impl SkirmishBoxDraw {
    /// Its sprites after `out`'s.
    pub(crate) fn push_sprites(&mut self, out: &mut Vec<SpriteInstance>) {
        self.paint.drain_lane(Material::Controls, out);
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

/// A Skirmish-family box's rects at retail size in a `w`x`h` window.
pub(crate) fn skirmish_parts(shell_box: &ShellBox, (w, h): (i32, i32)) -> BoxParts {
    shell_box.parts(w, h, crate::ui::shell::geom::RETAIL_SCALE)
}

/// A Skirmish-family box on `parts` ([`skirmish_parts`]) from its widgets
/// in `tree`, after what `paint` holds: over a solid panel when its art is
/// missing.
#[allow(clippy::too_many_arguments)]
fn skirmish_box_into(
    mut paint: HudPaint,
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    tree: BoxTree<'_>,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: &dyn Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> SkirmishBoxDraw {
    if let (Some(atlas), None) = (atlas, look.background) {
        let (at, edge) = (parts.dialog, OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68);
        paint.draw(Material::Controls, |out, _| {
            push_solid_rect(
                out,
                atlas,
                at,
                SHELL_MODAL_PANEL_RGB,
                look.depths.background,
            );
            push_rect_outline(out, atlas, at, edge, SHELL_DROPDOWN_DEPTH - 0.00015);
        });
    }
    paint_box_into(
        &mut paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        tree,
        shell_box,
        parts,
        pressed,
        look,
    );
    SkirmishBoxDraw {
        paint,
        dialog: parts.dialog,
    }
}

/// A Skirmish-family box on `parts` from its widgets in `tree`
/// ([`SkirmishBoxDraw`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn skirmish_box(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    tree: BoxTree<'_>,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: impl Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> SkirmishBoxDraw {
    let paint = HudPaint::default();
    skirmish_box_into(
        paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        tree,
        shell_box,
        parts,
        &pressed,
        look,
    )
}

/// [`skirmish_box`]'s sprites into `sprites` and its texts into `texts`,
/// which give up the box's rect first when `exclude_under`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_skirmish_box(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    tree: BoxTree<'_>,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: impl Fn(u16) -> bool,
    look: BoxLook<'_>,
    sprites: &mut Vec<SpriteInstance>,
    (texts, exclude_under): (&mut Vec<ShellTextDraw>, bool),
) {
    let mut draw = skirmish_box(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        tree,
        shell_box,
        parts,
        pressed,
        look,
    );
    draw.push_sprites(sprites);
    draw.push_texts(texts, exclude_under);
}

/// The window a Skirmish-family box centres in: the render size.
pub(crate) fn render_size(renderer: &RendererState, screen: &GameScreen) -> (i32, i32) {
    (
        renderer.render_width(screen) as i32,
        renderer.render_height(screen) as i32,
    )
}

/// Use Map's two-button box over the chooser as it shows: its body and
/// captions in the current language.
pub(crate) fn choose_map_prompt_shell_box(
    frontend: &FrontendState,
    process_assets: &ProcessAssets,
    prompt: &crate::ui::skirmish_shell::ChooseMapPrompt,
) -> ShellBox {
    let (body, ok) = super::text::choose_map_prompt_labels(frontend, process_assets, prompt);
    let cancel = process_assets.csf_label("GUI:Cancel", "Cancel");
    prompt.shell_box(body, ok, cancel)
}

/// Use Map's box `shell_box` ([`choose_map_prompt_shell_box`]) on `parts`
/// from its widgets on the shell controller. gamemd's eject-AI question and VERA's
/// random-map failure draw gamemd's art; VERA's map-library download boxes
/// Red Alert 2's, with how much has arrived under the text. Its texts give
/// up the box's rect.
pub(crate) fn choose_map_prompt_box(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    prompt: &crate::ui::skirmish_shell::ChooseMapPrompt,
    shell_box: &ShellBox,
    parts: &BoxParts,
) -> SkirmishBoxDraw {
    use crate::ui::skirmish_shell::ChooseMapPromptKind as Kind;
    let background = atlas.and_then(|atlas| {
        match prompt.kind {
            Kind::EjectAi | Kind::RandomMapFailed => None,
            _ => atlas.ra2_dialog_background,
        }
        .or_else(|| atlas.mode_two_dialog_background(None))
    });
    let mut paint = HudPaint::default();
    if let (Some(atlas), Kind::Downloading) = (atlas, prompt.kind)
        && let Some(progress) = crate::app::map_library::download_progress(frontend)
    {
        let track = parts.progress_track();
        let depth = BoxLook::skirmish(None).depths.button;
        let (white, rgb) = (atlas.white_pixel, super::SHELL_LABEL_TEXT_RGB);
        paint.draw(Material::Controls, |out, _| {
            super::gui_paint::push_progress(out, white, track, rgb, progress, depth)
        });
    }
    // Its widgets' captured button keeps its pushed look off the pointer too.
    let controller = &frontend.shell_controller;
    let held = crate::ui::shell::menu_page::pressed_id(controller, shell_box.dialog);
    let pressed = |id| held == Some(id);
    let look = BoxLook::prompt(background);
    let tree = controller_tree(controller, shell_box.dialog);
    skirmish_box_into(
        paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        tree,
        shell_box,
        parts,
        &pressed,
        look,
    )
}
