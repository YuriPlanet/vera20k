//! The one painter of the shell's message and wait boxes
//! ([`crate::ui::shell::modal::ShellBox`]): the mode-2 PUDLGBGN panel
//! (`0x00621E90`), its MNBTTN owner-draw buttons (`0x00612B70`), the body
//! static (`0x5B0` left-top wrapped; the wait box's `0x5B2` is SS_CENTER) and
//! each caption centred on its button with the press sink.

use super::chrome::{push_rect_outline, push_solid_rect, type3_button_frames};
use super::text::text_covered_by_overlay;
use super::{
    OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
    SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_MODAL_PANEL_RGB,
};
use crate::app::AppState;
use crate::app::frontend::shell_pass::owner_draw_button_label_rect;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_paint::{
    ModalButton, ModalDepths, ModalDraw, PaintLabel, SHELL_TEXT_RGB_ENABLED,
    paint_labels_at_depth_scaled, paint_modal_sprites,
};
use crate::render::shell_text::ShellTextDraw;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::ui::shell::modal::{BoxButton, BoxParts, ShellBox};

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
    let sprites = atlas.map_or_else(Vec::new, |a| {
        box_sprites(a, shell_box, parts, &pressed, look)
    });
    let text = box_texts(&state.renderer.bit_font, shell_box, parts, &pressed, look);
    ModalDraw { sprites, text }
}

/// Each shown button with its rect and whether it shows pushed.
fn shown<'a>(
    shell_box: &'a ShellBox,
    parts: &'a BoxParts,
    pressed: &'a impl Fn(u16) -> bool,
) -> impl Iterator<Item = (&'a BoxButton, RectPx, bool)> + 'a {
    let buttons = shell_box.buttons.iter();
    buttons.filter_map(move |b| Some((b, parts.rect_of(b.id)?, pressed(b.id))))
}

/// The panel and its buttons.
pub(crate) fn box_sprites(
    atlas: &SkirmishShellChromeAtlas,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: &impl Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> Vec<SpriteInstance> {
    let button = |(b, rect, pressed): (&BoxButton, _, _)| ModalButton {
        rect,
        pressed,
        enabled: b.enabled,
    };
    let buttons: Vec<ModalButton> = shown(shell_box, parts, pressed).map(button).collect();
    let (frames, at) = (type3_button_frames(atlas), parts.dialog);
    paint_modal_sprites(
        look.background,
        frames,
        at,
        &buttons,
        parts.scale,
        look.depths,
    )
}

/// The body, then the captions; those an overlay covers give way.
pub(crate) fn box_texts(
    font: &BitFont,
    shell_box: &ShellBox,
    parts: &BoxParts,
    pressed: &impl Fn(u16) -> bool,
    look: BoxLook<'_>,
) -> Vec<ShellTextDraw> {
    let label = |text: &str, rect, align, rgb| PaintLabel {
        text: text.to_owned().into(),
        rect,
        align,
        rgb,
        path_a_reveal: None,
    };
    let centre = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
    let captions: Vec<PaintLabel> = shown(shell_box, parts, pressed)
        .map(|(b, rect, pressed)| {
            let at = parts
                .scale
                .derive_rect(rect, |r| owner_draw_button_label_rect(r, pressed));
            let rgb = match b.enabled {
                true => SHELL_TEXT_RGB_ENABLED,
                false => look.disabled_rgb,
            };
            label(&b.caption, at, centre, rgb)
        })
        .collect();
    let align = [ShellAlign::NONE, ShellAlign::H_CENTER][usize::from(parts.centred)];
    let body = vec![label(
        &shell_box.body,
        parts.body,
        align,
        SHELL_TEXT_RGB_ENABLED,
    )];
    let mut text = Vec::new();
    for (mut labels, depth) in [(body, look.body_depth), (captions, look.depths.text)] {
        labels.retain(|label| !text_covered_by_overlay(label.rect, look.covered));
        text.extend(paint_labels_at_depth_scaled(
            font,
            &labels,
            depth,
            parts.scale.factor(),
        ));
    }
    text
}

/// A Skirmish-family box at retail size in a `w`x`h` window: its sprites
/// into `sprites` (over a solid panel when its art is missing) and its
/// texts into `texts`, which give up the box's rect first when
/// `exclude_under`.
pub(crate) fn push_skirmish_box(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    (w, h): (i32, i32),
    shell_box: &ShellBox,
    pressed: impl Fn(u16) -> bool,
    look: BoxLook<'_>,
    sprites: Option<&mut Vec<SpriteInstance>>,
    texts: Option<(&mut Vec<ShellTextDraw>, bool)>,
) {
    let parts = shell_box.parts(w, h, crate::ui::shell::geom::RETAIL_SCALE);
    if let (Some(out), Some(atlas)) = (sprites, atlas) {
        let (at, edge) = (parts.dialog, OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68);
        if look.background.is_none() {
            push_solid_rect(
                out,
                atlas,
                at,
                SHELL_MODAL_PANEL_RGB,
                look.depths.background,
            );
            push_rect_outline(out, atlas, at, edge, SHELL_DROPDOWN_DEPTH - 0.00015);
        }
        out.extend(box_sprites(atlas, shell_box, &parts, &pressed, look));
    }
    if let Some((out, exclude_under)) = texts {
        if exclude_under {
            super::text::exclude_text_under(out, parts.dialog);
        }
        out.extend(box_texts(
            &state.renderer.bit_font,
            shell_box,
            &parts,
            &pressed,
            look,
        ));
    }
}

/// The window a Skirmish-family box centres in: the render size.
pub(crate) fn render_size(state: &AppState) -> (i32, i32) {
    (state.render_width() as i32, state.render_height() as i32)
}

/// Use Map's two-button box over the chooser. gamemd's eject-AI question
/// and VERA's random-map failure draw gamemd's art; VERA's map-library
/// download boxes Red Alert 2's, with how much has arrived under the text.
pub(crate) fn push_choose_map_prompt(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    prompt: &crate::ui::skirmish_shell::ChooseMapPrompt,
    mut sprites: Option<&mut Vec<SpriteInstance>>,
    texts: Option<&mut Vec<ShellTextDraw>>,
) {
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
    if let (Some(out), Some(atlas), Kind::Downloading) =
        (sprites.as_deref_mut(), atlas, prompt.kind)
        && let Some(progress) = crate::app::map_library::download_progress(state)
    {
        let (w, h) = render_size(state);
        let body = shell_box
            .parts(w, h, crate::ui::shell::geom::RETAIL_SCALE)
            .body;
        let depth = BoxLook::skirmish(None).depths.button;
        super::modals::push_download_progress_bar(out, atlas, body, progress, depth);
    }
    // Its widgets' captured button keeps its pushed look off the pointer too.
    let held =
        crate::ui::shell::menu_page::pressed_id(&state.frontend.shell_controller, shell_box.dialog);
    let pressed = |id| held == Some(id);
    let (texts, look) = (texts.map(|out| (out, true)), BoxLook::prompt(background));
    push_skirmish_box(
        state,
        atlas,
        render_size(state),
        &shell_box,
        pressed,
        look,
        sprites,
        texts,
    );
}
