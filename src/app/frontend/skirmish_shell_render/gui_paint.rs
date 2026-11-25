//! Paints a dialog's `vera20k-gui` widgets with the shell's owner-draw
//! emitters: one place that turns Button, Checkbox, Trackbar, Combo, List,
//! Edit and Text nodes into the sprites and texts each screen used to
//! assemble by hand. A screen keeps its backdrop and chrome and calls
//! [`paint_gui_children`] for the widgets it built under its dialog root.
//!
//! Each node paints in tree order, its sprites before its text, so a tree
//! in the order a screen drew lays its draws down in that order. A node's
//! own props choose its text: `Color` and `DisabledColor`, `Align`,
//! `TextInset`; the host's [`GuiStyle`] the buttons' art and the texts and
//! rows it keeps itself.
//!
//! The in-game shell dialogs paint here too ([`super::in_game_shell::InGamePaint::nodes`]):
//! their right-panel buttons ([`ButtonArt::Side`]), MNBTTN buttons with the
//! type-2 caption ([`ButtonArt::ModalType2`]), group boxes and sunken frames
//! ([`GuiStyle::frame_of`]), dual-state and disabled checkboxes
//! ([`checkbox_face`], [`GuiStyle::dim_disabled_checkboxes`]), flag
//! pictures (`Image` nodes) and host-coloured bars (`Bar` nodes).
//!
//! VERA's room paints here whole: its statics in host colours
//! ([`GuiStyle::rgb_of`]), its boxes, bands and roster scrollbar
//! ([`FrameArt::Outline`], [`FrameArt::Fill`], [`FrameArt::ListBar`]), its
//! roster's cells ([`ComboArt::Cell`]), its badges, status icons and emote
//! button ([`Picture`], [`ButtonArt::Image`]) and its chat edit under its
//! boxes ([`GuiStyle::focus_of`], [`GuiStyle::ime_target`]).

use std::borrow::Cow;

use vera20k_gui::{Gui, NodeId, NodeKind, Prop, Value};

use super::controls::{ControlPaint, dropdown_scroll_part, paint_control, push_popup_instances};
use super::text::{
    button_label_rect_px, combo_dropdown_text_fit_width, combo_face_text_color,
    combo_face_text_draw_rect, combo_face_text_fit_width, fit_owner_draw_label, rect_to_text_rect,
};
use super::{
    COMBODROPWIN_TEXT_INSET_X, OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
    SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB, SHELL_MODAL_PANEL_RGB,
    SHELL_SWATCH_DEPTH,
};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material, rgba_tint};
use crate::render::main_menu_shell_chrome::{MainMenuShellChromeEntry, WolPageArt};
use crate::render::shell_paint::{self, ModalButton, ModalDepths};
use crate::render::shell_text::{self, ShellTextDraw};
use crate::render::shell_text_reveal::PathAReveal;
use crate::render::sidebar_chrome::SidebarChromeEntry;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::checkbox::CheckboxFace;
use crate::ui::shell::geom::{
    FAMILY_CAPTION_W, RectPx, SDBTNANM_CELL_W_NARROW, ShellAlign, ShellScale,
};
use crate::ui::shell::pause_menu::PauseMenuButtonState;
use crate::ui::sidebar::SidebarTheme;
use vera20k_gui::shell::bar;
use vera20k_gui::shell::trackbar::value_text_rect;

/// A list's rows as the host keeps them: each row's text and packed
/// `00BBGGRR` colour (`u32::MAX` for the node's own).
pub(crate) type HostRows = Vec<(String, u32)>;

/// A multi-cell list row (`0x4A8`'s columns): texts and page-art pictures
/// `x` base pixels into the row; a `filled` row shows the selection fill
/// (a multi-select list's, `LBS_MULTIPLESEL`).
#[derive(Default)]
pub(crate) struct CellRow {
    /// `(x, w, text, align, rgb)`; a `w` of 0 runs to the row's end, two short.
    pub(crate) texts: Vec<(i32, i32, String, ShellAlign, [f32; 3])>,
    /// `(x, PCX)`, centred vertically.
    pub(crate) icons: Vec<(i32, &'static str)>,
    pub(crate) filled: bool,
    /// The texts' `x` and `w` are window pixels into the row rather than
    /// base pixels grown with the page (the Mods list's).
    pub(crate) window: bool,
}

impl CellRow {
    pub(crate) fn text(&mut self, x: i32, w: i32, text: &str, align: ShellAlign, rgb: [f32; 3]) {
        self.texts.push((x, w, text.to_owned(), align, rgb));
    }
}

/// How a dialog's buttons paint.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonArt {
    /// Not at all: the host paints them, or a slide's column covers them.
    Host,
    /// The right panel's owner-draw type-1 buttons (`0x00612B70`, SDBTNANM),
    /// pushed while their press holds them (under the pointer or not), lit
    /// while `Toggled` (record `+0xC5`), with the type-1 caption, or one in
    /// their rect in their own `Align`.
    Panel,
    /// [`Self::Panel`]'s art with a caption that keeps the rect's centre,
    /// pressed or not, and the normal colour when disabled (the saved-file
    /// browsers over Skirmish): only the art shows either.
    PanelCentred,
    /// [`Self::Panel`]'s art pushed only while the press is held with the
    /// pointer still on it (612B70), with the type-2 caption of
    /// [`Self::Side`] in the label colour (the front-end Keyboard page's
    /// Back).
    PanelType2,
    /// [`Self::Panel`] on a family page's column: the SDBTNANM art on the
    /// main-menu chrome ([`Material::Column`]), fitted right-anchored to the
    /// cell, and the type-1 caption inset in the window rect.
    Column,
    /// Owner-draw type-3 buttons (MNBTTN), pushed under the pointer, with
    /// their caption centred.
    Modal,
    /// [`Self::Modal`]'s art with its caption centred on the whole rect,
    /// pressed or not (VERA's team bar).
    ModalCentred,
    /// [`Self::Modal`] with a page button's caption: grown with the page and
    /// squeezed onto the face (Options').
    ModalSqueezed,
    /// A framed plate with its caption in its `Align` (VERA's Extras rows).
    Plate,
    /// The caption alone, in its `Align` (`0x113`'s text sort buttons, whose
    /// owner-draw face is not traced).
    Caption,
    /// The active-game shell's right-panel type-2 buttons (`0x00612B70`):
    /// [`GuiStyle::side`]'s SIDEBTTN frame on [`Material::Side`], pushed
    /// only while the press is held with the pointer still on it, lit while
    /// `Toggled`; the caption centred in the type-2 rect
    /// ([`button_text_rect`], pressed +2/+4) in
    /// [`button_text_rgb`]: the side's dark colour while disabled
    /// or lit. `Color` is not read. Nothing paints without `side`.
    Side,
    /// [`Self::Modal`]'s MNBTTN art with the type-2 caption of the in-game
    /// dialogs' MNBTTN buttons: centred in [`button_text_rect`]
    /// (pressed +2/+4, not owner-draw's +2/+5).
    ModalType2,
    /// An image button (`0x00612B70`, frame type 0): the released picture
    /// of [`GuiStyle::picture_of`], or its pressed one (the released one
    /// when it has none) while pushed, copied 1:1 at the rect's top-left
    /// and cut to the rect, with no frame or caption (`0x6131D7..0x613237`).
    Image,
    /// The shell's checkbox face on the rect, checked while `Toggled`, no
    /// pushed look: a button that keeps a button's presses and events but
    /// shows as a box (Ares' Urban Areas, whose caption is its own static).
    CheckboxFace,
    /// MNBTTN's face stretched over the rect, pushed under the pointer as
    /// [`Self::Modal`]'s, its lit frame while [`GuiStyle::lit_of`] says;
    /// the caption centred in the owner-draw label rect grown with the
    /// page, squeezed to fit the face less 12 base pixels (VERA online's
    /// entry chooser).
    ModalLit,
    /// A message or wait box's button ([`super::shell_box`]): [`Self::Modal`]'s
    /// MNBTTN art at [`GuiStyle::shell_box`]'s depths, its caption centred
    /// in the owner-draw label rect taken at the page's scale, in the style's
    /// colour or, greyed, the box's own. The caption shows without the art
    /// too, and gives way only where an overlay meets the caption itself.
    ModalBox,
}

impl ButtonArt {
    /// The button that shows pushed: the Modal and Side buttons' only while
    /// the pointer is still on it (`held`, 612B70), the others' while their
    /// press holds them (`captured`).
    fn pushed(self, held: Option<NodeId>, captured: Option<NodeId>) -> Option<NodeId> {
        match self {
            ButtonArt::Modal
            | ButtonArt::ModalCentred
            | ButtonArt::ModalSqueezed
            | ButtonArt::ModalType2
            | ButtonArt::ModalBox
            | ButtonArt::PanelType2
            | ButtonArt::Side
            | ButtonArt::Image
            | ButtonArt::ModalLit => held,
            _ => captured,
        }
    }

    /// Where and how a button's caption draws: `own_align` is the node's
    /// `Align`, when it has one.
    fn caption_rect(
        self,
        rect: RectPx,
        pressed: bool,
        own_align: Option<ShellAlign>,
        scale: ShellScale,
        standard_panel_art: bool,
    ) -> (RectPx, ShellAlign) {
        let centred = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
        let shared_face = self == ButtonArt::Column
            || (self == ButtonArt::Panel
                && standard_panel_art
                && scale.base_rect(rect).w >= SDBTNANM_CELL_W_NARROW);
        match (self, own_align) {
            (ButtonArt::Modal, _) => (
                crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed),
                centred,
            ),
            (ButtonArt::ModalCentred | ButtonArt::PanelCentred, _) => (rect, centred),
            (ButtonArt::ModalType2 | ButtonArt::PanelType2 | ButtonArt::Side, _) => {
                (button_text_rect(rect, pressed), centred)
            }
            (_, Some(align)) => (rect, align),
            (_, None) if shared_face => (
                right_panel_caption_rect(rect, pressed, scale),
                ShellAlign::ART_CAPTION | ShellAlign::V_CENTER,
            ),
            (_, None) => (
                button_label_rect_px(rect, pressed, scale),
                ShellAlign::BUTTON_CAPTION,
            ),
        }
    }
}

/// The type-2 buttons' art for [`ButtonArt::Side`]: an in-game dialog's
/// ([`super::in_game_shell::InGamePaint::side_buttons`]).
#[derive(Clone, Copy)]
pub(crate) struct SideButtons {
    /// SIDEBTTN released, pressed and lit (`InGameShellArt::buttons`).
    pub(crate) frames: [Option<SidebarChromeEntry>; 3],
    /// The side whose colour a disabled or lit caption takes.
    pub(crate) theme: SidebarTheme,
    /// The art is cut to it: the shell's screen.
    pub(crate) clip: RectPx,
}

/// What a node paints as, by its `Id`, in place of its kind
/// ([`GuiStyle::frame_of`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FrameArt {
    /// A group box (`0x0061E700`): the bevel from half a font cell below the
    /// node's top, its caption (`Label`, `Text` or `text_of`) 10 px in and
    /// one cell tall, cut out of the top edge; the caption in its `Align`
    /// (else none) and `Color`.
    Group,
    /// The plain bevel (`0x006208F0`) on the node's rect, no caption: the
    /// host gives the rect the frame covers (a hotkey field's grown by
    /// native's extra pixel and the two-pixel border). `outer_light` puts
    /// the light colour on the outer ring (a sunken field).
    Plain { outer_light: bool },
    /// A one-pixel outline in one colour on the node's rect (VERA's roster's
    /// group and team bar boxes).
    Outline { rgb: [f32; 3] },
    /// The node's rect filled in one colour at `depth` (VERA's roster's
    /// band behind the local player's row).
    Fill { rgb: [f32; 3], depth: f32 },
    /// A List node's scrollbar alone, as its geometry places it, its held
    /// part pushed; no frame or rows (VERA's roster's).
    ListBar,
    /// A list's two frame rings ([`super::list::paint_list_frame`]) on the
    /// node's rect less `inset` (base pixels, as a `TextInset`), no
    /// caption: the WOL pages' `BS_GROUPBOX` frames.
    ListRings { inset: RectPx },
    /// The random map dialog's PROGBAR2 shape at the meter control's origin.
    RandomMapProgress {
        shape: SkirmishShellChromeEntry,
        outline: Option<[f32; 3]>,
        percent: u8,
    },
    /// VERA's download bar ([`push_progress`]): the node's rect outlined
    /// in `rgb` and filled two pixels in for `done` of `total`, at `depth`.
    Progress {
        rgb: [f32; 3],
        done: u64,
        total: u64,
        depth: f32,
    },
}

/// How an edit paints in place of a NewEdit ([`GuiStyle::edit_face_of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditFace {
    /// VERA's search line ([`push_search_line`]), on a framed edit
    /// (`TextInset`).
    SearchLine,
    /// An editable combo's edit, inside the combo's face: no frame of its
    /// own, its caret and text in its rect (the face's text area), the
    /// text from its first visible unit and no composition (the WOL
    /// login's nickname).
    ComboText,
}

/// How a dialog's combos paint their face.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComboArt {
    /// The owner-draw face, arrow and caption.
    Plain,
    /// [`Self::Plain`] in the owner-draw two-pixel bevel, with a list of
    /// its own (`0x105`'s).
    Framed,
    /// No face: the host paints it (VERA's roster cells); only an open
    /// list paints.
    Host,
    /// [`Self::Plain`]'s face; its open list (the host's rows, all shown,
    /// no scrollbar) in the owner-draw two-pixel bevel round its
    /// `PopupRect`, the row under the pointer lit, else the selection
    /// (A3's category list).
    Beveled,
    /// No face, frame or arrow: the selected row's text in the node's
    /// `Align` (else centred vertically) at the style's glyph scale, or a
    /// colour row's 12 px swatch centred on the cell. A cell
    /// [`GuiStyle::rgb_of`] colours shows lit: its text in that colour, its
    /// swatch ringed in it (VERA's roster's cells).
    Cell,
}

/// A picture an `Image` node (or an [`ButtonArt::Image`] button) shows
/// from the shell art other than an `Art` flag, by the host's word
/// ([`GuiStyle::picture_of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picture {
    /// A player's WOL badge, fitted to the node.
    Badge(&'static str),
    /// The LAN host or accepted icon (host first), clipped and centred;
    /// neither shows none.
    Status { host: bool, accepted: bool },
    /// C2/C9's emote pictures (`wodact` released, `wouact` pressed).
    Emote,
    /// An image button's PCX faces on the page art ([`GuiStyle::art`]),
    /// the pressed one while pushed, centred on the node at their size
    /// grown with the page (a face larger than the node overhangs it
    /// evenly), on [`Material::Icons`] (the WOL lobby's and buddy list's).
    Icon {
        released: &'static str,
        pressed: &'static str,
    },
    /// The LAN observer image, on or off, clipped and centred: a checkbox
    /// that shows as it (the lobbies' and buddy list's eye).
    Observer { on: bool },
    /// A kind-2 icon static's PCX on the page art ([`GuiStyle::art`]),
    /// centred in the node, or at its left or top on an axis it outgrows
    /// ([`crate::ui::shell::geom::centred_in_window`]), at its size grown
    /// with the page, on
    /// [`Material::Icons`] (WOL welcome's glossary).
    Glyph(&'static str),
}

/// Share the right-panel heading's text width while keeping the retail
/// type-1 button label's center and pressed offset (0x00612B70). The
/// button's painted rect and hit target are still the original cell.
fn right_panel_caption_rect(rect: RectPx, pressed: bool, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        let label = crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed);
        let width = label.w.min(FAMILY_CAPTION_W);
        RectPx::new(label.x + (label.w - width) / 2, label.y, width, label.h)
    })
}

/// What a dialog's widgets take from their host rather than their props.
#[derive(Clone, Copy)]
pub(crate) struct GuiStyle<'a> {
    /// A focused edit shows the open IME composition.
    pub(crate) ime: bool,
    /// The buttons' art and caption.
    pub(crate) buttons: ButtonArt,
    /// A button's own art by its `Id`, in place of `buttons`.
    pub(crate) art_of: Option<&'a dyn Fn(i32) -> Option<ButtonArt>>,
    /// The host's text for a node by its `Id` (a status, a name), in place
    /// of its `Label` or `Text`.
    pub(crate) text_of: Option<&'a dyn Fn(i32) -> Option<String>>,
    /// The rows of a host-counted (`RowCount`) list by its `Id`; a list
    /// without paints nothing, its host paints it. A host-counted combo's
    /// open list shows them too.
    pub(crate) rows_of: Option<&'a dyn Fn(i32) -> Option<HostRows>>,
    /// The multi-cell rows of a host-counted list by its `Id`, in place of
    /// `rows_of`'s, with their icons on `art`.
    pub(crate) cells_of: Option<&'a dyn Fn(i32) -> Option<Vec<CellRow>>>,
    pub(crate) art: Option<&'a WolPageArt>,
    /// The host's enable for a node by its `Id`, in place of its `Enabled`
    /// (a button the host greys while it still takes presses).
    pub(crate) enabled_of: Option<&'a dyn Fn(i32) -> Option<bool>>,
    /// Whether a button shows pushed by its `Id`, in place of its art's
    /// rule ([`ButtonArt::pushed`]): a host whose buttons keep the pushed
    /// look off the pointer, or lose it under a box.
    pub(crate) pressed_of: Option<&'a dyn Fn(i32) -> Option<bool>>,
    /// Lists paint as the shell dialogs' own ([`super::controls::list_view_controls`]:
    /// frame, selection, the control scrollbar) rather than as the WOL
    /// pages' over their art ([`super::list::paint_list`]).
    pub(crate) control_lists: bool,
    /// The combos' faces.
    pub(crate) combos: ComboArt,
    /// The open lists are Skirmish `0x102`'s: a thumb as wide as its
    /// scrollbar, and rows cut short of it (`0x0060DF38`). The other pages'
    /// lists keep the thumb a pixel in from each side and their rows whole.
    pub(crate) skirmish_lists: bool,
    /// An edit's caret shows (it has the focus, in its blink) by the host's
    /// word, for a tree the shell controller does not own.
    pub(crate) caret_of: Option<&'a dyn Fn(NodeId) -> bool>,
    /// The fill behind a framed edit (`TextInset`), for one no art backs.
    pub(crate) edit_fill: Option<[f32; 3]>,
    /// The glyph scale of every caption, value and row, when not the
    /// `scale` the widgets paint at (VERA's room's smaller text).
    pub(crate) text_factor: Option<f32>,
    /// The glyph scale of a combo face's caption, when not the `scale` the
    /// widgets paint at (VERA's team bar lists' words at the roster's size,
    /// over open lists in `text_factor`'s).
    pub(crate) face_factor: Option<f32>,
    /// The colour of a text with no `Color` (VERA's room's green).
    pub(crate) rgb: [f32; 3],
    /// [`ButtonArt::Side`]'s art.
    pub(crate) side: Option<SideButtons>,
    /// A node that paints as a frame by its `Id` ([`FrameArt`]).
    pub(crate) frame_of: Option<&'a dyn Fn(i32) -> Option<FrameArt>>,
    /// A disabled checkbox shows its face dimmed (`0x006163A0`); off, a
    /// checkbox's `Enabled` greys only its caption.
    pub(crate) dim_disabled_checkboxes: bool,
    /// A text that reveals as a kind-1 static does by its `Id` (a heading),
    /// in place of its colour.
    pub(crate) reveal_of: Option<&'a dyn Fn(i32) -> Option<PathAReveal>>,
    /// The host's colour for a text, or a [`ComboArt::Cell`], by its `Id`
    /// (a team's tint, a caption lit under the pointer), in place of its
    /// `Color`.
    pub(crate) rgb_of: Option<&'a dyn Fn(i32) -> Option<[f32; 3]>>,
    /// The picture an `Image` node or an image button shows by its `Id`,
    /// in place of its `Art` flag.
    pub(crate) picture_of: Option<&'a dyn Fn(i32) -> Option<Picture>>,
    /// An edit has the focus by the host's word (its dialog's own focus,
    /// under a box over it too), in place of the gui's.
    pub(crate) focus_of: Option<&'a dyn Fn(NodeId) -> bool>,
    /// The edits show this input-method target's composition, focused or
    /// not, in place of the open one in the focused edit.
    pub(crate) ime_target: Option<crate::app::input::ime::ImeTarget>,
    /// Combo faces show their caption whole in the arrow-reserved text
    /// rect, clipped there, rather than squeezed and cut (BC6's).
    pub(crate) clipped_faces: bool,
    /// A NewEdit paints this many base pixels inside its node's rect (in
    /// its one-pixel resource frame, BC6's chat); the node keeps the whole
    /// rect for its hits and its caret's scrolling.
    pub(crate) edit_inset: Option<i32>,
    /// An edit's hint by its `Id` while it shows nothing (the host's word),
    /// with its left inset in base pixels: drawn in the edit's rect less
    /// that inset, vertically centred in the style's colour, after the
    /// edit's own paint (VERA's search boxes).
    pub(crate) hint_of: Option<&'a dyn Fn(i32) -> Option<(String, i32)>>,
    /// An edit's face by its `Id`, in place of the NewEdit's ([`EditFace`]).
    pub(crate) edit_face_of: Option<&'a dyn Fn(i32) -> Option<EditFace>>,
    /// A list's message by its `Id` while it holds no rows (why it is
    /// empty), wrapped down the list from its first row.
    pub(crate) empty_of: Option<&'a dyn Fn(i32) -> Option<String>>,
    /// The lists' rows and scrollbar track are backed with the panel
    /// colour, for lists no art backs ([`super::list::paint_list`]'s
    /// `solid_fill`).
    pub(crate) solid_lists: bool,
    /// Where row `row` of a list (by its `Id`) starts its text, in base
    /// pixels into the row, in place of its `TextInset`: rounded once at
    /// the page's scale onto the row's left, and run to the list window's
    /// right edge (rows wrapped narrower than the list, such as hanging
    /// lines under a bullet).
    pub(crate) indent_of: Option<&'a dyn Fn(i32, usize) -> Option<i32>>,
    /// The picture an `Image` node shows from its page's own art by its
    /// `Id` ([`PageArt`]), onto [`Material::PageArt`], before `picture_of`.
    pub(crate) page_art_of: Option<&'a dyn Fn(i32) -> Option<PageArt>>,
    /// A [`ButtonArt::ModalLit`] button shows MNBTTN's lit frame by its
    /// `Id` (the host's word: under the pointer, else holding the focus).
    pub(crate) lit_of: Option<&'a dyn Fn(i32) -> bool>,
    /// A text's glyph size by its `Id`, a multiple of the page's scale, in
    /// place of the style's: squeezed to fit its rect's width.
    pub(crate) glyph_of: Option<&'a dyn Fn(i32) -> Option<f32>>,
    /// The texts [`Self::glyph_of`] sizes draw over a black copy of
    /// themselves one base pixel down and right.
    pub(crate) shadowed_texts: bool,
    /// A message box's depths and greyed caption colour
    /// ([`ButtonArt::ModalBox`]); its body `Text` paints at its own depth.
    pub(crate) shell_box: Option<BoxStyle>,
}

/// What a message box's widgets take from the box ([`GuiStyle::shell_box`]).
#[derive(Clone, Copy)]
pub(crate) struct BoxStyle {
    /// Its buttons' art (`button`) and captions (`text`).
    pub(crate) depths: ModalDepths,
    /// Its body's.
    pub(crate) body_depth: f32,
    /// A greyed button's caption colour.
    pub(crate) disabled_rgb: [f32; 3],
}

impl Default for GuiStyle<'_> {
    fn default() -> Self {
        GuiStyle {
            ime: true,
            buttons: ButtonArt::Host,
            art_of: None,
            text_of: None,
            rows_of: None,
            cells_of: None,
            art: None,
            enabled_of: None,
            pressed_of: None,
            control_lists: false,
            combos: ComboArt::Plain,
            skirmish_lists: false,
            caret_of: None,
            edit_fill: None,
            text_factor: None,
            face_factor: None,
            rgb: SHELL_LABEL_TEXT_RGB,
            side: None,
            frame_of: None,
            dim_disabled_checkboxes: false,
            reveal_of: None,
            rgb_of: None,
            picture_of: None,
            focus_of: None,
            ime_target: None,
            clipped_faces: false,
            edit_inset: None,
            hint_of: None,
            edit_face_of: None,
            empty_of: None,
            solid_lists: false,
            indent_of: None,
            page_art_of: None,
            lit_of: None,
            glyph_of: None,
            shadowed_texts: false,
            shell_box: None,
        }
    }
}

impl GuiStyle<'_> {
    /// The WOL and online boxes': MNBTTN buttons, the shell dialogs'
    /// lists, edits without the IME composition.
    pub(crate) fn popup() -> Self {
        GuiStyle {
            ime: false,
            buttons: ButtonArt::Modal,
            control_lists: true,
            ..Default::default()
        }
    }
}

/// A node's caption: `Label` is a string key (shown as the key when
/// missing), `Text` a literal (borrowed, not copied).
fn label_of<'g>(
    process_assets: &ProcessAssets,
    gui: &'g Gui,
    node: NodeId,
) -> Option<Cow<'g, str>> {
    match gui.get(node, Prop::Label).ok().flatten() {
        Some(Value::Text(key)) => Some(Cow::Owned(process_assets.csf_label(key, key))),
        _ => match gui.get(node, Prop::Text).ok().flatten() {
            Some(Value::Text(text)) => Some(Cow::Borrowed(text.as_str())),
            _ => None,
        },
    }
}

fn int(gui: &Gui, node: NodeId, prop: Prop, default: i32) -> i32 {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Int(v)) => *v,
        _ => default,
    }
}

fn flag(gui: &Gui, node: NodeId, prop: Prop, default: bool) -> bool {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Bool(v)) => *v,
        _ => default,
    }
}

fn color(gui: &Gui, node: NodeId, prop: Prop) -> Option<[f32; 3]> {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Rgba(c)) => Some(rgba_tint(*c).0),
        _ => None,
    }
}

/// The node's text colour: `Color` (else `normal`), or while disabled its
/// `DisabledColor`, else its kind's `disabled` colour when it has one.
fn text_rgb(
    gui: &Gui,
    node: NodeId,
    enabled: bool,
    normal: [f32; 3],
    disabled: Option<[f32; 3]>,
) -> [f32; 3] {
    let off = (!enabled)
        .then(|| color(gui, node, Prop::DisabledColor).or(disabled))
        .flatten();
    off.or_else(|| color(gui, node, Prop::Color))
        .unwrap_or(normal)
}

/// `rect` less the node's `TextInset` (base pixels, grown with the page).
fn inset(gui: &Gui, node: NodeId, rect: RectPx, scale: ShellScale) -> Option<RectPx> {
    match gui.get(node, Prop::TextInset).ok().flatten() {
        Some(Value::Rect(i)) => Some(scale.derive_rect(rect, |r| {
            RectPx::new(r.x + i.x, r.y + i.y, r.w - i.x - i.w, r.h - i.y - i.h)
        })),
        _ => None,
    }
}

fn text(
    bit_font: &BitFont,
    value: &str,
    rect: RectPx,
    align: ShellAlign,
    factor: f32,
    rgb: [f32; 3],
    depth: f32,
) -> ShellTextDraw {
    shell_text::draw_in_rect_scaled(
        bit_font,
        value,
        rect_to_text_rect(rect),
        rgb,
        align,
        [0.0; 2],
        depth,
        factor,
    )
}

fn items(gui: &Gui, node: NodeId, prop: Prop) -> &[String] {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Items(items)) => items,
        _ => &[],
    }
}

/// The widgets under `root`, in paint order. Invisible subtrees are skipped.
pub(crate) fn paint_gui_children(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
) -> HudPaint {
    paint_gui_styled(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        gui,
        root,
        scale,
        &[],
        GuiStyle::default(),
    )
}

/// [`paint_gui_children`] in the host's `style`, under `overlays` that
/// paint with the widgets' sprites but before their text (an open list in
/// a box): the captions of the nodes an overlay meets are left out, so they
/// don't show through it.
pub(crate) fn paint_gui_styled(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
    style: GuiStyle<'_>,
) -> HudPaint {
    let mut paint = HudPaint::default();
    paint_gui_styled_into(
        &mut paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        gui,
        root,
        scale,
        overlays,
        style,
    );
    paint
}

/// [`paint_gui_styled`] appended to `paint`'s lanes; nothing without the
/// shell control art.
pub(crate) fn paint_gui_styled_into(
    paint: &mut HudPaint,
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
    style: GuiStyle<'_>,
) {
    if let Some(atlas) = frontend.skirmish_shell_chrome.as_ref() {
        paint_gui_into(
            paint,
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            Some(atlas),
            gui,
            root,
            scale,
            overlays,
            style,
        );
    }
}

/// The widgets of `ids` under `root` in that order rather than the tree's
/// (a screen whose paint order is not its hit order), each with its own
/// subtree.
pub(crate) fn paint_gui_ids(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    gui: &Gui,
    root: NodeId,
    ids: impl IntoIterator<Item = u16>,
    scale: ShellScale,
    style: GuiStyle<'_>,
) -> HudPaint {
    let mut paint = HudPaint::default();
    paint_gui_ids_into(
        &mut paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        gui,
        root,
        ids,
        scale,
        style,
    );
    paint
}

/// [`paint_gui_ids`] appended to `paint`'s lanes.
pub(crate) fn paint_gui_ids_into(
    paint: &mut HudPaint,
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    gui: &Gui,
    root: NodeId,
    ids: impl IntoIterator<Item = u16>,
    scale: ShellScale,
    style: GuiStyle<'_>,
) {
    for node in ids
        .into_iter()
        .filter_map(|id| gui.find_id(root, id.into()))
    {
        paint_gui_styled_into(
            paint,
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            node,
            scale,
            &[],
            style,
        );
    }
}

/// [`paint_gui_styled`] on `atlas`, for a renderer that holds the shell
/// chrome while it paints (the Skirmish family's).
pub(crate) fn paint_gui_with(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: &SkirmishShellChromeAtlas,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
    style: GuiStyle<'_>,
) -> HudPaint {
    let mut paint = HudPaint::default();
    paint_gui_into(
        &mut paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        Some(atlas),
        gui,
        root,
        scale,
        overlays,
        style,
    );
    paint
}

/// [`paint_gui_with`] appended to `paint`'s lanes, after what is on them
/// (a host that paints several subtrees into one, or keeps lanes of its
/// own: an in-game dialog's, [`super::in_game_shell::InGamePaint::nodes`]).
/// Without `atlas` (the shell control art not loaded) only what needs none
/// paints: [`ButtonArt::Side`] and [`ButtonArt::Column`] buttons, captions
/// and texts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_gui_into(
    paint: &mut HudPaint,
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
    style: GuiStyle<'_>,
) {
    let chrome = atlas
        .map(SkirmishShellChromeAtlas::control_chrome)
        .unwrap_or_default();
    let white = atlas.and_then(|atlas| atlas.white_pixel);
    let font = bit_font;
    // Shell passes draw in queue order (no depth test).
    let depth = SHELL_CONTROL_TEXT_DEPTH;
    let glyph = style.text_factor.unwrap_or(scale.factor());
    // The pushed button of the dialog `root` belongs to.
    let mut dialog_root = root;
    while let Ok(Some(parent)) = gui.tree().parent(dialog_root) {
        dialog_root = parent;
    }
    let held = crate::ui::shell::page::held_in(gui, dialog_root);
    let captured = crate::ui::shell::page::captured_in(gui, dialog_root);
    let id = |node| gui.id_of(node);
    let caption = |node| match style
        .text_of
        .zip(id(node))
        .and_then(|(text_of, id)| text_of(id))
    {
        Some(text) => Some(Cow::Owned(text)),
        None => label_of(process_assets, gui, node),
    };
    // Each visible node in paint order, its sprites before its text.
    gui.tree().walk_visible(root, |node| {
        let Ok(kind) = gui.tree().kind(node) else {
            return;
        };
        let Ok(rect) = gui.tree().screen_rect(node) else {
            return;
        };
        let covered = super::text_covered_by_overlay(rect, overlays);
        let enabled = match style
            .enabled_of
            .zip(id(node))
            .and_then(|(enabled_of, id)| enabled_of(id))
        {
            Some(enabled) => enabled,
            None => flag(gui, node, Prop::Enabled, true),
        };
        let align = ShellAlign(int(gui, node, Prop::Align, ShellAlign::V_CENTER.0 as i32) as u8);
        if let Some(frame) = style
            .frame_of
            .zip(id(node))
            .and_then(|(frame_of, id)| frame_of(id))
        {
            if frame == FrameArt::ListBar {
                paint.draw(Material::Controls, |out, _| {
                    push_list_bar(out, &chrome, gui, node, scale)
                });
                return;
            }
            let label = caption(node);
            let look = TextLook {
                align: match gui.get(node, Prop::Align).ok().flatten() {
                    Some(_) => align,
                    None => ShellAlign::NONE,
                },
                glyph,
                rgb: text_rgb(gui, node, enabled, style.rgb, None),
            };
            let caption = label.as_deref().map(|label| (label, !covered));
            paint.draw(Material::Controls, |controls, texts| {
                push_frame(
                    controls, texts, font, white, rect, frame, caption, look, scale,
                )
            });
            return;
        }
        match kind {
            NodeKind::Button => {
                let art = style
                    .art_of
                    .zip(id(node))
                    .and_then(|(art_of, id)| art_of(id))
                    .unwrap_or(style.buttons);
                let pressed = match style
                    .pressed_of
                    .zip(id(node))
                    .and_then(|(pressed_of, id)| pressed_of(id))
                {
                    Some(pressed) => pressed,
                    None => art.pushed(held, captured) == Some(node),
                };
                if art == ButtonArt::Side {
                    let Some(side) = style.side else { return };
                    let look = PauseMenuButtonState {
                        pressed,
                        highlighted: flag(gui, node, Prop::Toggled, false),
                        enabled,
                    };
                    let label = caption(node).filter(|_| !covered);
                    paint.draw(Material::Side, |art, texts| {
                        push_side_button(
                            art,
                            texts,
                            font,
                            side,
                            rect,
                            label.as_deref(),
                            look,
                            glyph,
                        )
                    });
                    return;
                }
                let mut standard_panel_art = false;
                match (art, atlas) {
                    (ButtonArt::Host | ButtonArt::Side, _) => return,
                    (ButtonArt::Caption, _) => {}
                    (ButtonArt::Column, _) => {
                        let visual = vera20k_gui::shell::button::ButtonVisual {
                            pressed,
                            highlighted: flag(gui, node, Prop::Toggled, false),
                        };
                        let frame = super::chrome::right_panel_button_sdbtnanm_frame_index(
                            visual, !enabled,
                        );
                        let chrome = frontend.main_menu_shell_chrome.as_ref();
                        paint.draw(Material::Column, |out, _| {
                            out.extend(chrome.and_then(|chrome| {
                                shell_paint::column_button(chrome, rect, frame, SHELL_CONTROL_DEPTH)
                            }))
                        });
                    }
                    // A box's button: its art when there is any, its caption
                    // either way.
                    (ButtonArt::ModalBox, atlas) => {
                        let look = style.shell_box.unwrap_or(BoxStyle {
                            depths: ModalDepths {
                                background: SHELL_CONTROL_DEPTH,
                                button: SHELL_CONTROL_DEPTH,
                                text: depth,
                            },
                            body_depth: depth,
                            disabled_rgb: SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
                        });
                        if let Some(atlas) = atlas {
                            let button = ModalButton {
                                rect,
                                pressed,
                                enabled,
                            };
                            paint.draw(Material::Controls, |out, _| {
                                out.extend(shell_paint::paint_modal_sprites(
                                    None,
                                    super::type3_button_frames(atlas),
                                    rect,
                                    &[button],
                                    scale,
                                    look.depths,
                                ))
                            });
                        }
                        let at = scale.derive_rect(rect, |r| {
                            crate::app::frontend::shell_pass::owner_draw_button_label_rect(
                                r, pressed,
                            )
                        });
                        // The box's own caption (`ShellBox::sync`) over a
                        // host spec's `Label`.
                        let own = match gui.get(node, Prop::Text) {
                            Ok(Some(Value::Text(own))) => Some(Cow::Borrowed(own.as_str())),
                            _ => caption(node),
                        };
                        let shown = own.filter(|_| !super::text_covered_by_overlay(at, overlays));
                        if let Some(label) = shown {
                            let rgb = if enabled {
                                style.rgb
                            } else {
                                look.disabled_rgb
                            };
                            let centred = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
                            let depth = look.depths.text;
                            paint
                                .texts
                                .push(text(bit_font, &label, at, centred, glyph, rgb, depth));
                        }
                        return;
                    }
                    // The rest are the shell control art's.
                    (_, None) => return,
                    (ButtonArt::Image, Some(atlas)) => {
                        let picture = style
                            .picture_of
                            .zip(id(node))
                            .and_then(|(picture_of, id)| picture_of(id));
                        match (picture, style.art) {
                            (
                                Some(Picture::Icon {
                                    released,
                                    pressed: down,
                                }),
                                Some(art),
                            ) => {
                                let face = if pressed { down } else { released };
                                paint.draw(Material::Icons, |out, _| {
                                    push_page_icon(out, art, face, rect, false, scale)
                                });
                            }
                            (Some(Picture::Icon { .. }), None) | (None, _) => {}
                            (Some(picture), _) => {
                                paint.draw(Material::Controls, |out, _| {
                                    push_picture(out, atlas, picture, rect, pressed, scale)
                                });
                            }
                        }
                        return;
                    }
                    (ButtonArt::ModalLit, Some(atlas)) => {
                        let lit = style
                            .lit_of
                            .zip(id(node))
                            .is_some_and(|(lit_of, id)| lit_of(id));
                        let mut frames = super::type3_button_frames(atlas);
                        if lit {
                            frames.up = atlas.modal_button_mnbttn_frame2.or(frames.up);
                        }
                        if let Some(face) = frames.select(pressed, true) {
                            paint.draw(Material::Controls, |out, _| {
                                super::chrome::push_entry(out, face, rect, SHELL_CONTROL_DEPTH)
                            });
                        }
                        let Some(label) = caption(node).filter(|t| !covered && !t.is_empty())
                        else {
                            return;
                        };
                        let at = scale.derive_rect(rect, |r| {
                            crate::app::frontend::shell_pass::owner_draw_button_label_rect(
                                r, pressed,
                            )
                        });
                        let face_width = (scale.base_rect(at).w - 12).max(1) as u32;
                        let rgb = text_rgb(gui, node, true, style.rgb, None);
                        let centred = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
                        paint
                            .texts
                            .push(font.fitted(font.fit_line(&label, face_width), |font| {
                                shell_text::draw_in_rect_scaled(
                                    font,
                                    &label,
                                    rect_to_text_rect(at),
                                    rgb,
                                    centred,
                                    [0.0; 2],
                                    depth,
                                    scale.factor(),
                                )
                            }));
                        return;
                    }
                    (ButtonArt::CheckboxFace, Some(_)) => {
                        let checked = flag(gui, node, Prop::Toggled, false);
                        let control = ControlPaint::Checkbox { checked, rect };
                        paint.draw(Material::Controls, |out, _| {
                            paint_control(out, &chrome, control, scale)
                        });
                    }
                    (
                        ButtonArt::Panel | ButtonArt::PanelCentred | ButtonArt::PanelType2,
                        Some(atlas),
                    ) => {
                        let visual = vera20k_gui::shell::button::ButtonVisual {
                            pressed,
                            highlighted: flag(gui, node, Prop::Toggled, false),
                        };
                        standard_panel_art = paint.draw(Material::Controls, |out, _| {
                            super::chrome::push_right_panel_button_shp(
                                out,
                                atlas,
                                rect,
                                visual,
                                !enabled,
                                SHELL_CONTROL_DEPTH,
                            )
                        });
                    }
                    (
                        ButtonArt::Modal
                        | ButtonArt::ModalCentred
                        | ButtonArt::ModalSqueezed
                        | ButtonArt::ModalType2,
                        Some(atlas),
                    ) => {
                        let button = ModalButton {
                            rect,
                            pressed,
                            enabled,
                        };
                        let depths = ModalDepths {
                            background: SHELL_CONTROL_DEPTH,
                            button: SHELL_CONTROL_DEPTH,
                            text: depth,
                        };
                        paint.draw(Material::Controls, |out, _| {
                            out.extend(shell_paint::paint_modal_sprites(
                                None,
                                super::type3_button_frames(atlas),
                                rect,
                                &[button],
                                scale,
                                depths,
                            ))
                        });
                    }
                    (ButtonArt::Plate, Some(atlas)) => {
                        paint.draw(Material::Controls, |out, _| {
                            super::chrome::push_solid_rect(
                                out,
                                atlas,
                                rect,
                                SHELL_MODAL_PANEL_RGB,
                                SHELL_CONTROL_DEPTH,
                            );
                            super::chrome::push_ownerdraw_two_pixel_bevel_frame(
                                out,
                                atlas,
                                rect,
                                SHELL_CONTROL_DEPTH,
                            );
                        });
                    }
                }
                let Some(label) = caption(node).filter(|t| !covered && !t.is_empty()) else {
                    return;
                };
                let rgb = text_rgb(
                    gui,
                    node,
                    enabled || art == ButtonArt::PanelCentred,
                    style.rgb,
                    Some(shell_paint::SHELL_TEXT_RGB_DISABLED),
                );
                let own_align = gui.get(node, Prop::Align).ok().flatten().map(|_| align);
                let (at, align) =
                    art.caption_rect(rect, pressed, own_align, scale, standard_panel_art);
                paint
                    .texts
                    .push(text(bit_font, &label, at, align, glyph, rgb, depth));
            }
            // A checkbox that shows as the host's picture (the observer
            // image): no face or caption.
            NodeKind::Checkbox
                if style
                    .picture_of
                    .zip(id(node))
                    .is_some_and(|(f, id)| f(id).is_some()) =>
            {
                let picture = style.picture_of.zip(id(node)).and_then(|(f, id)| f(id));
                if let (Some(picture), Some(atlas)) = (picture, atlas) {
                    paint.draw(Material::Controls, |out, _| {
                        push_picture(out, atlas, picture, rect, false, scale)
                    });
                }
            }
            NodeKind::Checkbox => {
                // `Checkbox { checked }` is this face, never dimmed.
                let control = ControlPaint::StatefulCheckbox {
                    face: checkbox_face(gui, node),
                    disabled: style.dim_disabled_checkboxes && !enabled,
                    rect,
                };
                paint.draw(Material::Controls, |out, _| {
                    paint_control(out, &chrome, control, scale)
                });
                if let Some(label) = caption(node).filter(|_| !covered) {
                    let at = crate::ui::skirmish_shell::checkbox_text_rect(rect, scale);
                    let rgb = text_rgb(gui, node, enabled, style.rgb, None);
                    paint
                        .texts
                        .push(text(bit_font, &label, at, align, glyph, rgb, depth));
                }
            }
            NodeKind::Trackbar => {
                // The plaque and its value text unless the owner turned them
                // off (`0x4AC` with 0), which gives the track the whole window.
                let plaque = flag(gui, node, Prop::ShowValue, true);
                let Some(range) = gui.trackbar_range(node) else {
                    return;
                };
                let value = int(gui, node, Prop::Pos, range.min);
                let control = ControlPaint::Trackbar {
                    rect,
                    thumb_left: range.thumb_left_in(value, rect, scale),
                    plaque,
                    disabled: !enabled,
                };
                paint.draw(Material::Controls, |out, _| {
                    paint_control(out, &chrome, control, scale)
                });
                if plaque && !covered {
                    // A disabled trackbar's value text takes [0xAC1CB4] (`0x0061E2B1`).
                    let rgb = text_rgb(
                        gui,
                        node,
                        enabled,
                        style.rgb,
                        Some(SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F),
                    );
                    // A host value text (its own or `Text`) stands in for the
                    // position.
                    let shown = match (
                        style
                            .text_of
                            .zip(id(node))
                            .and_then(|(text_of, id)| text_of(id)),
                        gui.get(node, Prop::Text).ok().flatten(),
                    ) {
                        (Some(shown), _) => shown,
                        (None, Some(Value::Text(shown))) => shown.clone(),
                        _ => range.stepped(value).to_string(),
                    };
                    let at = value_text_rect(rect, scale);
                    paint.texts.push(text(
                        bit_font,
                        &shown,
                        at,
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                        glyph,
                        rgb,
                        depth,
                    ));
                }
            }
            NodeKind::Combo => {
                let Some(atlas) = atlas else { return };
                if style.combos == ComboArt::Framed {
                    paint.draw(Material::Controls, |out, _| {
                        super::chrome::push_ownerdraw_two_pixel_bevel_frame(
                            out,
                            atlas,
                            rect,
                            SHELL_CONTROL_DEPTH,
                        )
                    });
                }
                let face = (rect, !covered);
                paint_combo(
                    paint,
                    bit_font,
                    frontend,
                    match_state,
                    atlas,
                    gui,
                    node,
                    face,
                    enabled,
                    scale,
                    style,
                );
            }
            NodeKind::List => {
                let Some(atlas) = atlas else { return };
                // A host-counted list's rows are the host's, or it paints it.
                let cells = style
                    .cells_of
                    .zip(id(node))
                    .and_then(|(cells_of, id)| cells_of(id));
                let host_rows = match gui.get(node, Prop::RowCount).ok().flatten() {
                    Some(_) => match style
                        .rows_of
                        .zip(id(node))
                        .and_then(|(rows_of, id)| rows_of(id))
                    {
                        Some(rows) => Some(rows),
                        None if cells.is_some() => None,
                        None => return,
                    },
                    None => None,
                };
                // The owner-draw list `0x00618D40` over its dialog's art.
                let (Some(view), Some(geometry)) = (gui.list_view(node), gui.list_geometry(node))
                else {
                    return;
                };
                let selected = view
                    .selected
                    .filter(|_| !flag(gui, node, Prop::NoSelectionFill, false));
                paint.draw(Material::Controls, |out, _| match style.control_lists {
                    true => super::controls::list_view_controls(
                        out,
                        atlas,
                        geometry,
                        (selected, view.top, view.pressed),
                        scale,
                    ),
                    false => super::list::paint_list(
                        out,
                        atlas,
                        geometry,
                        view.top,
                        selected,
                        view.pressed,
                        style.solid_lists,
                        scale,
                    ),
                });
                // An empty list's message from its first row's text start
                // (two pixels in, grown with the page) to its content's end.
                let empty = (view.rows == 0 && !covered)
                    .then(|| style.empty_of.zip(id(node)))
                    .flatten()
                    .and_then(|(empty_of, id)| empty_of(id));
                if let Some(empty) = empty {
                    let row = geometry.row(0);
                    let left = (2.0 * scale.factor()).round() as i32;
                    let content = geometry.content;
                    let h = content.y + content.h - row.y;
                    let at = RectPx::new(row.x + left, row.y, row.w - left, h);
                    let rgb = style.rgb;
                    paint.texts.push(text(
                        bit_font,
                        &empty,
                        at,
                        ShellAlign::NONE,
                        glyph,
                        rgb,
                        depth,
                    ));
                }
                if let Some(cells) = cells {
                    let rows = cells.into_iter().skip(view.top).take(geometry.visible_rows);
                    for (visible, row) in rows.enumerate() {
                        let r = geometry.row(visible);
                        paint_cells(paint, bit_font, atlas, style, row, r, scale, covered);
                    }
                    return;
                }
                let normal = text_rgb(gui, node, true, style.rgb, None);
                let rows: Vec<(&str, u32)> = match &host_rows {
                    Some(rows) => rows.iter().map(|(row, rgb)| (row.as_str(), *rgb)).collect(),
                    None => items(gui, node, Prop::Items)
                        .iter()
                        .map(|row| (row.as_str(), u32::MAX))
                        .collect(),
                };
                let shown = rows.into_iter().skip(view.top).take(geometry.visible_rows);
                let right = geometry.outer.x + geometry.outer.w - 1;
                for (visible, (row, packed)) in shown.enumerate().filter(|_| !covered) {
                    let r = geometry.row(visible);
                    let indent = style
                        .indent_of
                        .zip(id(node))
                        .and_then(|(indent_of, id)| indent_of(id, view.top + visible));
                    let at = match indent {
                        Some(indent) => {
                            let x = r.x + (indent as f32 * scale.factor()).round() as i32;
                            RectPx::new(x, r.y, right - x, r.h)
                        }
                        None => inset(gui, node, r, scale).unwrap_or(RectPx::new(
                            r.x + 2,
                            r.y,
                            r.w - 2,
                            r.h,
                        )),
                    };
                    let rgb = match packed {
                        u32::MAX => normal,
                        c => {
                            [c as u8, (c >> 8) as u8, (c >> 16) as u8].map(|c| f32::from(c) / 255.0)
                        }
                    };
                    paint
                        .texts
                        .push(text(bit_font, row, at, align, glyph, rgb, depth));
                }
            }
            // NewEdit `0x00614B30` as `push_new_edit` paints it, or framed in
            // its rect with a `TextInset` field (or as a search line); the
            // open IME composition shows in the focused one, a whole-text
            // selection fills behind it; the host's hint follows. An edit
            // with no area draws nothing.
            NodeKind::Edit if rect.w <= 0 || rect.h <= 0 => {}
            NodeKind::Edit => {
                let Some(atlas) = atlas else { return };
                let mut edit = gui.edit_state(node).unwrap_or_default();
                if let Some(focus_of) = style.focus_of {
                    edit.focused = focus_of(node);
                }
                let composition = match style.ime_target {
                    Some(target) => ime.composition(target),
                    None => ime.open_composition().filter(|_| style.ime && edit.focused),
                };
                let caret = match style.caret_of {
                    Some(caret_of) => caret_of(node),
                    None => frontend.shell_controller.caret_visible(node),
                };
                edit.focused |= style.caret_of.is_some() && caret;
                let rgb = text_rgb(gui, node, true, style.rgb, None);
                let hint = style
                    .hint_of
                    .zip(id(node))
                    .and_then(|(hint_of, id)| hint_of(id));
                let hint = hint.filter(|_| !covered).map(|(hint, x)| {
                    let at = scale.derive_rect(rect, |r| RectPx::new(r.x + x, r.y, r.w - x, r.h));
                    text(bit_font, &hint, at, ShellAlign::V_CENTER, glyph, rgb, depth)
                });
                let face = style
                    .edit_face_of
                    .zip(id(node))
                    .and_then(|(edit_face_of, id)| edit_face_of(id));
                if face == Some(EditFace::ComboText) {
                    if caret {
                        paint.draw(Material::Controls, |out, _| {
                            super::controls::push_new_edit_caret(
                                out,
                                atlas,
                                font,
                                &edit,
                                rect,
                                super::SHELL_EDIT_CARET_DEPTH,
                                scale,
                            )
                        });
                    }
                    if !covered {
                        let shown = edit.painted_text();
                        let align = ShellAlign::V_CENTER;
                        paint
                            .texts
                            .push(text(bit_font, &shown, rect, align, glyph, rgb, depth));
                    }
                    paint.texts.extend(hint);
                    return;
                }
                if face == Some(EditFace::SearchLine)
                    && let Ok(Some(Value::Rect(text_inset))) = gui.get(node, Prop::TextInset)
                {
                    // The host's text (with its own composition), else the edit's.
                    let shown = match style.text_of.zip(id(node)).and_then(|(f, id)| f(id)) {
                        Some(shown) => shown,
                        None => edit.painted_text(),
                    };
                    let line = SearchLine {
                        rect,
                        inset: *text_inset,
                        fill: style.edit_fill,
                        look: TextLook {
                            align: ShellAlign::V_CENTER,
                            glyph,
                            rgb,
                        },
                    };
                    let shown = Some(shown.as_str()).filter(|_| !covered);
                    paint.draw(Material::Controls, |out, texts| {
                        push_search_line(out, texts, font, atlas, line, shown, scale)
                    });
                    paint.texts.extend(hint);
                    return;
                }
                let Some(field) = inset(gui, node, rect, scale) else {
                    let painted = match style.edit_inset {
                        Some(px) => scale.derive_rect(rect, |r| {
                            RectPx::new(r.x + px, r.y + px, r.w - 2 * px, r.h - 2 * px)
                        }),
                        None => rect,
                    };
                    paint.draw(Material::Controls, |out, texts| {
                        super::controls::push_new_edit(
                            out,
                            texts,
                            atlas,
                            font,
                            &edit,
                            composition,
                            painted,
                            caret,
                            rgb,
                            depth,
                            scale,
                        )
                    });
                    paint.texts.extend(hint);
                    return;
                };
                let selected = edit.focused && flag(gui, node, Prop::AllSelected, false);
                paint.draw(Material::Controls, |out, texts| {
                    if let Some(fill) = style.edit_fill {
                        super::chrome::push_solid_rect(out, atlas, rect, fill, depth);
                    }
                    if selected {
                        let width = font.text_width(&edit.painted_text()) as i32;
                        let at =
                            scale.derive_rect(field, |f| RectPx::new(f.x, f.y + 2, width, f.h - 4));
                        super::chrome::push_solid_rect(
                            out,
                            atlas,
                            at,
                            OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                            super::SHELL_EDIT_SELECTION_DEPTH,
                        );
                    }
                    super::controls::push_framed_edit(
                        out,
                        texts,
                        atlas,
                        font,
                        &edit,
                        composition,
                        (rect, field),
                        (caret && !selected, rgb, depth),
                        scale,
                    )
                });
                paint.texts.extend(hint);
            }
            // A text the host sizes: squeezed to its rect's width at that
            // size, over its shadow when the style shadows them.
            NodeKind::Text
                if style
                    .glyph_of
                    .zip(id(node))
                    .is_some_and(|(f, id)| f(id).is_some()) =>
            {
                let size = style.glyph_of.zip(id(node)).and_then(|(f, id)| f(id));
                let Some(label) = caption(node).filter(|t| !covered && !t.is_empty()) else {
                    return;
                };
                let sized = glyph * size.unwrap_or(1.0);
                let fit = font.fit_line(&label, (rect.w as f32 / sized).max(1.0) as u32);
                let rgb = text_rgb(gui, node, enabled, style.rgb, None);
                let shadow = style
                    .shadowed_texts
                    .then(|| (scale.derive_rect(rect, |r| r.translate(1, 1)), [0.0; 3]));
                font.fitted(fit, |font| {
                    for (at, rgb) in shadow.into_iter().chain([(rect, rgb)]) {
                        paint.texts.push(shell_text::draw_in_rect_scaled(
                            font,
                            &label,
                            rect_to_text_rect(at),
                            rgb,
                            align,
                            [0.0; 2],
                            depth,
                            sized,
                        ));
                    }
                });
            }
            NodeKind::Text => {
                // A box's body sits at the box's depth.
                let depth = style.shell_box.map_or(depth, |look| look.body_depth);
                if let Some(label) = caption(node).filter(|t| !covered && !t.is_empty()) {
                    let at = inset(gui, node, rect, scale).unwrap_or(rect);
                    let reveal = style
                        .reveal_of
                        .zip(id(node))
                        .and_then(|(reveal_of, id)| reveal_of(id));
                    paint.texts.push(match reveal {
                        Some(reveal) => shell_text::draw_in_rect_path_a_scaled(
                            font,
                            &label,
                            rect_to_text_rect(at),
                            align,
                            [0.0; 2],
                            depth,
                            reveal,
                            glyph,
                        ),
                        None => {
                            let rgb = style
                                .rgb_of
                                .zip(id(node))
                                .and_then(|(rgb_of, id)| rgb_of(id))
                                .unwrap_or_else(|| text_rgb(gui, node, enabled, style.rgb, None));
                            text(bit_font, &label, at, align, glyph, rgb, depth)
                        }
                    });
                }
            }
            // The host's picture, else a flag or insignia PCX (`Art`) from
            // the shell art's flags, clipped to the node and centred in it.
            NodeKind::Image => {
                if let Some(art) = style
                    .page_art_of
                    .zip(id(node))
                    .and_then(|(page_art_of, id)| page_art_of(id))
                {
                    paint.draw(Material::PageArt, |out, _| {
                        push_page_art(out, art, rect, scale)
                    });
                    return;
                }
                let Some(atlas) = atlas else { return };
                let host = style
                    .picture_of
                    .zip(id(node))
                    .and_then(|(picture_of, id)| picture_of(id));
                if let (Some(Picture::Glyph(file)), Some(art)) = (host, style.art) {
                    paint.draw(Material::Icons, |out, _| {
                        push_page_icon(out, art, file, rect, true, scale)
                    });
                    return;
                }
                if let Some(picture) = host {
                    paint.draw(Material::Controls, |out, _| {
                        push_picture(out, atlas, picture, rect, false, scale)
                    });
                    return;
                }
                let Ok(Some(Value::Text(name))) = gui.get(node, Prop::Art) else {
                    return;
                };
                if let Some(entry) = atlas.lan_player_side(name) {
                    paint.draw(Material::Controls, |out, _| {
                        super::chrome::push_flag_entry_clipped_centered(
                            out,
                            entry,
                            rect,
                            scale,
                            SHELL_CONTROL_DEPTH,
                        )
                    });
                }
            }
            // A lone scrollbar (VERA's random-map Advanced pages): the
            // control scrollbar on its bar, its thumb at its position; it
            // acts on press, so no part shows pushed.
            NodeKind::ScrollBar => {
                if let Some(view) = gui.scrollbar_view(node) {
                    let control = ControlPaint::ScrollBar {
                        scrollbar: view.bar,
                        thumb: view.thumb,
                        pressed_part: None,
                    };
                    paint.draw(Material::Controls, |out, _| {
                        paint_control(out, &chrome, control, scale)
                    });
                }
            }
            // A bar's one meaning (`vera20k_gui::shell::bar`): the outline
            // in `Background`, the fill `Fill` percent wide in `Color`, here
            // the style's without one.
            NodeKind::Bar => {
                let look = BarLook {
                    percent: int(gui, node, Prop::Fill, bar::FULL),
                    outline: color(gui, node, Prop::Background).unwrap_or_else(|| {
                        let [r, g, b] = bar::OUTLINE_RGB;
                        rgba_tint(vera20k_gui::Rgba(r, g, b, 255)).0
                    }),
                    fill: color(gui, node, Prop::Color).unwrap_or(style.rgb),
                };
                paint.draw(Material::Controls, |out, _| {
                    push_bar(out, white, rect, look)
                });
            }
            _ => {}
        }
    });
}

/// A text's alignment, glyph scale and colour.
#[derive(Clone, Copy)]
pub(crate) struct TextLook {
    pub(crate) align: ShellAlign,
    pub(crate) glyph: f32,
    pub(crate) rgb: [f32; 3],
}

/// A checkbox node's face: dual-state (message 0x4E5) when it carries its
/// partner flag (0x4E6) in `Selected`, 0 or 1 ([`CheckboxFace::dual_state`]),
/// else checked or not by `Checked`.
pub(crate) fn checkbox_face(gui: &Gui, node: NodeId) -> CheckboxFace {
    let checked = flag(gui, node, Prop::Checked, false);
    match gui.get(node, Prop::Selected).ok().flatten() {
        Some(Value::Int(partner)) => CheckboxFace::dual_state(checked, *partner != 0),
        _ if checked => CheckboxFace::Checked,
        _ => CheckboxFace::Unchecked,
    }
}

/// Type2 chooses released0 / pressed1 / timer-highlight2 (612EE8..612F5B).
/// Disabled buttons retain their SHP frame;612F5F changes their text color only.
pub(crate) fn button_frame(state: PauseMenuButtonState) -> usize {
    vera20k_gui::shell::button::owner_button_frame(state.pressed, state.highlighted)
}

pub(crate) fn button_text_rgb(theme: SidebarTheme, enabled: bool) -> [f32; 3] {
    if enabled {
        return SHELL_LABEL_TEXT_RGB;
    }
    // Original612F5F..613136 uses active scenario side and the initialized colors
    // 72A8E0/72A900/72A920 (registered815454/58/5C). Its RGB565 encode/decode
    // truncates [0,82,117] to[0,80,112] for Allied and [72,0,0] for Soviet/Yuri.
    let rgb = match theme {
        SidebarTheme::Allied => [0, 80, 112],
        SidebarTheme::Soviet | SidebarTheme::Yuri => [72, 0, 0],
    };
    rgb.map(|value| value as f32 / 255.0)
}

pub(crate) fn button_text_rect(rect: RectPx, pressed: bool) -> RectPx {
    // Common native type2 text tail61358D..6135EE: fixed right/bottom, and
    // the pressed state moves left/top by2/4 within that same rectangle.
    let dx = if pressed { 2 } else { 0 };
    let dy = if pressed { 4 } else { 0 };
    RectPx::new(
        rect.x + dx,
        rect.y + 1 + dy,
        rect.w - 2 - dx,
        rect.h - 1 - dy,
    )
}

/// [`ButtonArt::Side`]: `look`'s SIDEBTTN frame (frame 0 when the art lacks
/// it) at its canvas size on `rect`, cut to the clip, then the caption, if
/// any, centred in the type-2 rect in the type-2 colour.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_side_button(
    art: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    font: &BitFont,
    side: SideButtons,
    rect: RectPx,
    caption: Option<&str>,
    look: PauseMenuButtonState,
    glyph: f32,
) {
    let frames = side.frames;
    if let Some(entry) = frames[button_frame(look)].or(frames[0]) {
        super::in_game_shell::push_art(art, entry, rect, side.clip);
    }
    let Some(caption) = caption else { return };
    // A lit face takes the art's dark label colour, as a disabled one
    // does: yellow is hard to read on it.
    let rgb = button_text_rgb(side.theme, look.enabled && !look.highlighted);
    let scale = crate::ui::shell::geom::RETAIL_SCALE;
    let (at, align) = ButtonArt::Side.caption_rect(rect, look.pressed, None, scale, false);
    texts.push(shell_text::draw_in_rect_scaled(
        font,
        caption,
        rect_to_text_rect(at),
        rgb,
        align,
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
        glyph,
    ));
}

/// A [`FrameArt`] on `rect`, and a group's caption when it has one and it
/// shows (`caption.1`); the caption's width cuts the frame either way.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_frame(
    controls: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    font: &BitFont,
    white: Option<SkirmishShellChromeEntry>,
    rect: RectPx,
    frame: FrameArt,
    caption: Option<(&str, bool)>,
    look: TextLook,
    scale: ShellScale,
) {
    use super::controls::{group_box_geometry, push_group_or_plain_frame};
    match frame {
        FrameArt::Group => {
            let cell = font.cell_height() as i32;
            let (frame, caption_rect) = group_box_geometry(rect, cell, scale);
            let width = caption.map(|(caption, _)| font.text_width(caption) as i32);
            push_group_or_plain_frame(controls, white, frame, width, false, scale);
            if let Some((caption, true)) = caption {
                texts.push(shell_text::draw_in_rect_scaled(
                    font,
                    caption,
                    rect_to_text_rect(caption_rect),
                    look.rgb,
                    look.align,
                    [0.0; 2],
                    SHELL_CONTROL_TEXT_DEPTH,
                    look.glyph,
                ));
            }
        }
        FrameArt::Plain { outer_light } => {
            push_group_or_plain_frame(controls, white, rect, None, outer_light, scale)
        }
        FrameArt::Outline { rgb } => {
            super::chrome::push_rect_outline_px(controls, white, rect, rgb, SHELL_CONTROL_DEPTH)
        }
        FrameArt::Fill { rgb, depth } => {
            super::chrome::push_solid_rect_px(controls, white, rect, rgb, depth)
        }
        // [`push_list_bar`]: it needs the list's own geometry.
        FrameArt::ListBar => {}
        FrameArt::ListRings { inset } => {
            let window = scale.derive_rect(rect, |r| {
                RectPx::new(
                    r.x + inset.x,
                    r.y + inset.y,
                    r.w - inset.x - inset.w,
                    r.h - inset.y - inset.h,
                )
            });
            super::list::paint_list_frame_px(controls, white, window);
        }
        FrameArt::RandomMapProgress {
            shape,
            outline,
            percent,
        } => {
            if let Some(white) = white {
                use crate::app::loading::pump::{LoadingProgressState, push_progress_meter};
                let mut progress = LoadingProgressState::standard_skirmish();
                progress.advance_progress(u32::from(percent));
                let inset = (3.0 * scale.factor()).round();
                push_progress_meter(
                    controls,
                    shape,
                    white,
                    &progress,
                    [rect.x as f32 + inset, rect.y as f32 + inset],
                    outline,
                    scale,
                    [SHELL_DROPDOWN_DEPTH - 0.00014; 2],
                );
            }
        }
        FrameArt::Progress {
            rgb,
            done,
            total,
            depth,
        } => push_progress(controls, white, rect, rgb, (done, total), depth),
    }
}

/// VERA's download boxes show how much has arrived: `track` outlined in
/// `rgb`, filled two pixels in for `done` of `total` of its inner width.
pub(crate) fn push_progress(
    out: &mut Vec<SpriteInstance>,
    white: Option<SkirmishShellChromeEntry>,
    track: RectPx,
    rgb: [f32; 3],
    (done, total): (u64, u64),
    depth: f32,
) {
    super::chrome::push_rect_outline_px(out, white, track, rgb, depth);
    let inner = track.w - 4;
    let filled = (inner as u64 * done.min(total) / total.max(1)) as i32;
    if filled > 0 {
        let fill = RectPx::new(track.x + 2, track.y + 2, filled, track.h - 4);
        super::chrome::push_solid_rect_px(out, white, fill, rgb, depth);
    }
}

/// [`FrameArt::ListBar`]: List `node`'s scrollbar where its geometry puts
/// it, while its rows do not all fit, the part a press holds pushed.
fn push_list_bar(
    out: &mut Vec<SpriteInstance>,
    chrome: &crate::render::skirmish_shell_chrome::ControlChrome,
    gui: &Gui,
    node: NodeId,
    scale: ShellScale,
) {
    let (Some(view), Some(geometry)) = (gui.list_view(node), gui.list_geometry(node)) else {
        return;
    };
    let (Some(scrollbar), Some(thumb)) = (geometry.scrollbar, geometry.thumb) else {
        return;
    };
    let control = ControlPaint::ScrollBar {
        scrollbar,
        thumb,
        pressed_part: view.pressed.map(dropdown_scroll_part),
    };
    paint_control(out, chrome, control, scale);
}

/// A [`Picture`] on `rect`: a status icon clipped and centred, a badge
/// fitted, the emote picture (its pressed one while `pressed`) at the
/// rect's top-left, cut to it.
fn push_picture(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    picture: Picture,
    rect: RectPx,
    pressed: bool,
    scale: ShellScale,
) {
    use super::chrome::{push_entry_fitted, push_entry_sized, push_flag_entry_clipped_centered};
    match picture {
        Picture::Status { host, accepted } => {
            if let Some(entry) = atlas.lan_player_status(host, accepted) {
                push_flag_entry_clipped_centered(out, entry, rect, scale, SHELL_CONTROL_DEPTH);
            }
        }
        Picture::Badge(file) => {
            if let Some(entry) = atlas.wol_badge(file) {
                push_entry_fitted(out, entry, rect, scale, SHELL_SWATCH_DEPTH);
            }
        }
        Picture::Observer { on } => {
            if let Some(entry) = atlas.lan_observer_toggle(on) {
                push_flag_entry_clipped_centered(out, entry, rect, scale, SHELL_CONTROL_DEPTH);
            }
        }
        // On the page art: [`push_page_icon`].
        Picture::Icon { .. } | Picture::Glyph(_) => {}
        Picture::Emote => {
            let (Some(released), pressed_art) = atlas.wol_emote_images() else {
                return;
            };
            let mut entry = match pressed {
                true => pressed_art.unwrap_or(released),
                false => released,
            };
            let [art_w, art_h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
            if art_w > 0.0 && art_h > 0.0 {
                let draw = [art_w.min(rect.w as f32), art_h.min(rect.h as f32)];
                entry.uv_size[0] *= draw[0] / art_w;
                entry.uv_size[1] *= draw[1] / art_h;
                let (x, y) = (rect.x as f32, rect.y as f32);
                push_entry_sized(out, entry, x, y, draw, SHELL_CONTROL_DEPTH);
            }
        }
    }
}

/// A picture from a page's own art (not the shell control art) that an
/// `Image` node shows ([`GuiStyle::page_art_of`]); the host draws
/// [`Material::PageArt`] with that art's texture.
#[derive(Clone, Copy)]
pub(crate) enum PageArt {
    /// A PCX static (`0x108`'s band bars): at its native size
    /// centred in its window on the 800x600 page, then grown with the page.
    Pcx(MainMenuShellChromeEntry),
    /// A kind-4 static's shape frame (`0x72B`'s SCRA): grown, centred in
    /// its window and clipped to it ([`shell_paint::paint_kind4_static`]).
    Shape(MainMenuShellChromeEntry),
}

fn push_page_art(out: &mut Vec<SpriteInstance>, art: PageArt, rect: RectPx, scale: ShellScale) {
    match art {
        PageArt::Pcx(entry) => {
            let (w, h) = entry.native_size();
            let (x, y) = crate::ui::shell::geom::centred_in_window(scale.base_rect(rect), w, h);
            let depth = shell_paint::STATIC_IMAGE_DEPTH;
            shell_paint::push_entry_scaled(out, entry, x, y, scale, depth);
        }
        PageArt::Shape(entry) => out.extend(shell_paint::paint_kind4_static(entry, rect, scale)),
    }
}

/// [`Picture::Icon`]'s or [`Picture::Glyph`]'s `face` from the page art,
/// centred on `rect` in base pixels at its size grown with the page; held
/// to the rect's left and top where it outgrows it while `held`.
fn push_page_icon(
    out: &mut Vec<SpriteInstance>,
    art: &WolPageArt,
    face: &str,
    rect: RectPx,
    held: bool,
    scale: ShellScale,
) {
    let Some((_, entry)) = art.icons.iter().find(|(name, _)| name == face) else {
        return;
    };
    let (w, h) = entry.native_size();
    let base = scale.base_rect(rect);
    let (x, y) = match held {
        true => crate::ui::shell::geom::centred_in_window(base, w, h),
        false => (base.x + (base.w - w) / 2, base.y + (base.h - h) / 2),
    };
    let [x, y] = scale.scale_point([x, y]);
    shell_paint::push_entry_grown(out, *entry, x, y, scale, SHELL_CONTROL_DEPTH - 0.00003);
}

/// A `Bar` node's fill share (0..=100) and colours.
#[derive(Clone, Copy)]
pub(crate) struct BarLook {
    pub(crate) percent: i32,
    pub(crate) outline: [f32; 3],
    pub(crate) fill: [f32; 3],
}

/// 0x0064A7B0's bar on `rect`: the one-pixel outline, then the fill
/// ([`bar::fill_rect`]) just over it.
pub(crate) fn push_bar(
    out: &mut Vec<SpriteInstance>,
    white: Option<SkirmishShellChromeEntry>,
    rect: RectPx,
    look: BarLook,
) {
    super::chrome::push_rect_outline_px(out, white, rect, look.outline, SHELL_CONTROL_DEPTH);
    let fill = bar::fill_rect(rect, look.percent);
    let depth = SHELL_CONTROL_DEPTH - 0.00001;
    super::chrome::push_solid_rect_px(out, white, fill, look.fill, depth);
}

/// A search line's caret width, in base pixels, as a NewEdit's.
pub(crate) const SEARCH_CARET_W: i32 = 2;

/// A search line ([`EditFace::SearchLine`]): its rect, its `TextInset`
/// (base pixels), the fill no art backs and its text's look.
#[derive(Clone, Copy)]
pub(crate) struct SearchLine {
    pub(crate) rect: RectPx,
    pub(crate) inset: RectPx,
    pub(crate) fill: Option<[f32; 3]>,
    pub(crate) look: TextLook,
}

/// The end of `shown` a search line on `rect` (less `inset`, base pixels)
/// shows with the caret after it, and the caret's rect on the window: a
/// long text scrolls its start out of the field. Widths are 800x600
/// numbers.
pub(crate) fn search_line_tail<'a>(
    font: &BitFont,
    shown: &'a str,
    rect: RectPx,
    inset: RectPx,
    scale: ShellScale,
) -> (&'a str, RectPx) {
    let base = scale.base_rect(rect);
    let field = RectPx::new(
        base.x + inset.x,
        base.y + inset.y,
        base.w - inset.x - inset.w,
        base.h - inset.y - inset.h,
    );
    let room = field.w - SEARCH_CARET_W;
    let mut tail = shown;
    let width = loop {
        let width = font.text_width(tail) as i32;
        let mut chars = tail.chars();
        if width <= room || chars.next().is_none() {
            break width;
        }
        tail = chars.as_str();
    };
    let caret = RectPx::new(field.x + width, field.y + 2, SEARCH_CARET_W, field.h - 4);
    (tail, scale.scale_rect(caret))
}

/// VERA's search line on `line.rect`: the fill when no art backs it, the
/// list's two frame rings round it, and the text (`shown`, none under an
/// overlay) from the inset's left, its tail kept in the field with a steady
/// caret after it while it holds any: the line has no focus to lose. The
/// inset's right edge only bounds the tail.
fn push_search_line(
    out: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    font: &BitFont,
    atlas: &SkirmishShellChromeAtlas,
    line: SearchLine,
    shown: Option<&str>,
    scale: ShellScale,
) {
    let SearchLine {
        rect,
        inset,
        fill,
        look,
    } = line;
    let depth = SHELL_CONTROL_DEPTH;
    if let Some(fill) = fill {
        super::chrome::push_solid_rect(out, atlas, rect, fill, depth);
    }
    super::list::paint_list_frame(out, atlas, rect);
    let Some(shown) = shown.filter(|shown| !shown.is_empty()) else {
        return;
    };
    let (tail, caret) = search_line_tail(font, shown, rect, inset, scale);
    super::chrome::push_solid_rect(out, atlas, caret, look.rgb, depth);
    let field = scale.derive_rect(rect, |r| {
        RectPx::new(
            r.x + inset.x,
            r.y + inset.y,
            r.w - inset.x,
            r.h - inset.y - inset.h,
        )
    });
    texts.push(shell_text::draw_in_rect_scaled(
        font,
        tail,
        rect_to_text_rect(field),
        look.rgb,
        look.align,
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
        look.glyph,
    ));
}

/// A multi-cell row on `row`: its fill, then its cells' texts and pictures.
fn paint_cells(
    paint: &mut HudPaint,
    bit_font: &BitFont,
    atlas: &SkirmishShellChromeAtlas,
    style: GuiStyle<'_>,
    cells: CellRow,
    row: RectPx,
    scale: ShellScale,
    covered: bool,
) {
    if cells.filled {
        paint.draw(Material::Controls, |out, _| {
            super::chrome::push_solid_rect(
                out,
                atlas,
                row,
                OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                SHELL_CONTROL_DEPTH - 0.00001,
            )
        });
    }
    let glyph = style.text_factor.unwrap_or(scale.factor());
    let window = cells.window;
    for (x, w, value, align, rgb) in cells.texts.into_iter().filter(|_| !covered) {
        let cell = |row: RectPx| {
            let w = if w > 0 { w } else { row.w - x - 2 };
            RectPx::new(row.x + x, row.y, w, row.h)
        };
        let at = match window {
            true => cell(row),
            false => scale.derive_rect(row, cell),
        };
        let depth = SHELL_CONTROL_TEXT_DEPTH;
        paint
            .texts
            .push(text(bit_font, &value, at, align, glyph, rgb, depth));
    }
    if let Some(art) = style.art {
        paint.draw(Material::Icons, |out, _| {
            for (x, file) in cells.icons {
                super::wol_lobby::push_icon(out, art, file, row, x, scale);
            }
        });
    }
}

/// [`ComboArt::Cell`]'s face: the row's swatch (ringed while `lit`) or its
/// text in `lit`'s colour, else `rgb`.
#[allow(clippy::too_many_arguments)]
fn push_cell(
    paint: &mut HudPaint,
    bit_font: &BitFont,
    atlas: &SkirmishShellChromeAtlas,
    gui: &Gui,
    node: NodeId,
    (rect, caption, swatch, item): (RectPx, bool, Option<[f32; 3]>, &str),
    (lit, glyph): (Option<[f32; 3]>, f32),
    rgb: [f32; 3],
    scale: ShellScale,
) {
    let rgb = lit.unwrap_or(rgb);
    if let Some(tint) = swatch {
        let chip = scale.derive_rect(rect, |rect| {
            RectPx::new(
                rect.x + (rect.w - 12) / 2,
                rect.y + (rect.h - 12) / 2,
                12,
                12,
            )
        });
        paint.draw(Material::Controls, |out, _| {
            super::chrome::push_solid_rect(out, atlas, chip, tint, SHELL_SWATCH_DEPTH);
            if lit.is_some() {
                let ring = scale.derive_rect(chip, |rect| {
                    RectPx::new(rect.x - 1, rect.y - 1, rect.w + 2, rect.h + 2)
                });
                super::chrome::push_rect_outline(out, atlas, ring, rgb, SHELL_SWATCH_DEPTH);
            }
        });
        return;
    }
    if !caption {
        return;
    }
    let align = ShellAlign(int(gui, node, Prop::Align, ShellAlign::V_CENTER.0 as i32) as u8);
    let depth = SHELL_CONTROL_TEXT_DEPTH;
    paint
        .texts
        .push(text(bit_font, item, rect, align, glyph, rgb, depth));
}

/// The open popup and any rows supplied by its host.
fn combo_popup_rows(
    gui: &Gui,
    node: NodeId,
    style: GuiStyle<'_>,
) -> Option<(vera20k_gui::ComboDropDown, Option<HostRows>)> {
    let drop_down = gui.combo_drop_down(node)?;
    let rows = gui
        .get(node, Prop::RowCount)
        .ok()
        .flatten()
        .and(style.rows_of)
        .zip(gui.id_of(node))
        .and_then(|(rows_of, id)| rows_of(id));
    Some((drop_down, rows))
}

/// An owner-draw combo's face, its caption unless an overlay covers it, and
/// its drop-down while it is open, its rows in the style's colour and glyph
/// scale.
#[allow(clippy::too_many_arguments)]
fn paint_combo(
    paint: &mut HudPaint,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    atlas: &SkirmishShellChromeAtlas,
    gui: &Gui,
    node: NodeId,
    (rect, caption): (RectPx, bool),
    enabled: bool,
    scale: ShellScale,
    style: GuiStyle<'_>,
) {
    let popup_rows = combo_popup_rows(gui, node, style);
    let selected = usize::try_from(int(gui, node, Prop::Selected, -1)).ok();
    // A colour row shows its swatch (`house_color_tint`) and no text.
    let swatches = items(gui, node, Prop::Swatches);
    let swatch = |row: usize| {
        let index = swatches.get(row)?.parse().ok()?;
        Some(super::controls::house_color_tint(
            crate::app::state::active_rules(frontend, match_state),
            index,
        ))
    };
    let face_swatch = selected.and_then(swatch);
    let items = items(gui, node, Prop::Items);
    let selected_item = selected.and_then(|row| items.get(row)).map(String::as_str);
    let font = bit_font;
    if style.combos == ComboArt::Cell {
        // The room's face caption already comes from the selected choice.
        let host = if caption && face_swatch.is_none() {
            style
                .text_of
                .zip(gui.id_of(node))
                .and_then(|(text_of, id)| text_of(id))
        } else {
            None
        };
        if let Some(item) = face_swatch
            .map(|_| "")
            .or(host.as_deref())
            .or(selected_item)
        {
            let lit = style
                .rgb_of
                .zip(gui.id_of(node))
                .and_then(|(rgb_of, id)| rgb_of(id));
            let look = (lit, style.text_factor.unwrap_or(scale.factor()));
            let face = (rect, caption, face_swatch, item);
            push_cell(
                paint, bit_font, atlas, gui, node, face, look, style.rgb, scale,
            );
        }
    } else if style.combos != ComboArt::Host {
        let control = ControlPaint::Combo {
            rect,
            swatch: face_swatch,
            open: popup_rows.is_some(),
            disabled: !enabled,
        };
        paint.draw(Material::Controls, |out, _| {
            paint_control(out, &atlas.control_chrome(), control, scale)
        });
        // The host's caption, or `Text`, stands in for the selected row's.
        let host = (style.text_of.zip(gui.id_of(node))).and_then(|(text_of, id)| text_of(id));
        let face = match (&host, gui.get(node, Prop::Text).ok().flatten()) {
            (Some(face), _) | (None, Some(Value::Text(face))) => Some(face.as_str()),
            _ => selected_item,
        };
        let rgb = text_rgb(
            gui,
            node,
            enabled,
            SHELL_LABEL_TEXT_RGB,
            Some(combo_face_text_color(true)),
        );
        if let Some(face) = face.filter(|_| caption && style.clipped_faces) {
            // Whole, clipped at the arrow reserve.
            let at = crate::ui::skirmish_shell::combo_text_rect(rect, scale);
            let glyph = style.face_factor.unwrap_or(scale.factor());
            let depth = SHELL_CONTROL_TEXT_DEPTH;
            let align = ShellAlign::V_CENTER;
            paint
                .texts
                .push(text(bit_font, face, at, align, glyph, rgb, depth));
        } else if let Some(face) = face.filter(|_| caption) {
            // Cut to gamemd's arrow reserve (`0x00617250`), squeezed first;
            // drawn in the face less its `TextInset` when it has one.
            let at = inset(gui, node, rect, scale)
                .unwrap_or_else(|| combo_face_text_draw_rect(rect, scale));
            fit_owner_draw_label(font, face, combo_face_text_fit_width(rect, scale), |face| {
                paint.texts.push(text(
                    bit_font,
                    face,
                    at,
                    ShellAlign::V_CENTER,
                    style.face_factor.unwrap_or(scale.factor()),
                    rgb,
                    SHELL_CONTROL_TEXT_DEPTH,
                ));
            });
        }
    }
    let Some((drop_down, host_rows)) = popup_rows else {
        return;
    };
    let popup = drop_down.popup;
    // A host-counted combo supplies a color for each row.
    let row_at = |row: usize| match &host_rows {
        Some(rows) => rows.get(row).map(|(item, color)| (item.as_str(), *color)),
        None => items.get(row).map(|item| (item.as_str(), u32::MAX)),
    };
    let row_count = host_rows.as_ref().map_or(items.len(), Vec::len);
    let visible_rows = popup.first..popup.first + popup.visible_rows;
    if style.combos == ComboArt::Beveled {
        let rows_rect = popup.rect;
        let frame = RectPx::new(
            rows_rect.x - 2,
            rows_rect.y - 2,
            rows_rect.w + 4,
            rows_rect.h + 4,
        );
        paint.draw(Material::Popup, |out, _| {
            super::chrome::push_solid_rect(
                out,
                atlas,
                frame,
                super::SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
                SHELL_DROPDOWN_DEPTH,
            )
        });
        let row_h = vera20k_gui::shell::list::ListMetrics::combo(scale).row_h;
        let lit = drop_down.hot_row.or(selected);
        for (index, (item, _)) in (0..row_count).filter_map(|index| Some((index, row_at(index)?))) {
            let row = RectPx::new(
                rows_rect.x,
                rows_rect.y + index as i32 * row_h,
                rows_rect.w,
                row_h,
            );
            if lit == Some(index) {
                paint.draw(Material::Popup, |out, _| {
                    super::chrome::push_solid_rect(
                        out,
                        atlas,
                        row,
                        OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                        SHELL_DROPDOWN_DEPTH - 0.00001,
                    )
                });
            }
            paint.popup_texts.push(text(
                bit_font,
                item,
                RectPx::new(row.x + 3, row.y, row.w - 6, row.h),
                ShellAlign::V_CENTER,
                scale.factor(),
                SHELL_LABEL_TEXT_RGB,
                SHELL_DROPDOWN_TEXT_DEPTH,
            ));
        }
        paint.draw(Material::Popup, |out, _| {
            super::chrome::push_ownerdraw_two_pixel_bevel_frame(
                out,
                atlas,
                frame,
                SHELL_DROPDOWN_DEPTH - 0.00002,
            )
        });
        return;
    }
    if style.combos == ComboArt::Framed {
        // `0x105`'s list is no `ComboDropWin`: a one-pixel outline, the
        // selection lit wherever the pointer is, its rows uncut.
        let depth = SHELL_DROPDOWN_DEPTH - 0.00015;
        paint.draw(Material::Popup, |out, _| {
            super::chrome::push_solid_rect(
                out,
                atlas,
                popup.rect,
                super::SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
                depth,
            )
        });
        paint.draw(Material::Popup, |out, _| {
            super::chrome::push_rect_outline(
                out,
                atlas,
                popup.rect,
                super::OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
                depth - 0.00001,
            )
        });
        if let Some(row) = selected.and_then(|row| popup.row_rect(row)) {
            paint.draw(Material::Popup, |out, _| {
                super::chrome::push_solid_rect(
                    out,
                    atlas,
                    row,
                    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                    depth - 0.00002,
                )
            });
        }
        let left = crate::ui::skirmish_shell::COMBO_TEXT_LEFT_INSET as f32;
        let left = (left * scale.factor()).round() as i32;
        for index in visible_rows {
            let (Some(row), Some((item, _))) = (popup.row_rect(index), row_at(index)) else {
                continue;
            };
            paint.popup_texts.push(text(
                bit_font,
                item,
                RectPx::new(row.x + left, row.y, row.w - left, row.h),
                ShellAlign::V_CENTER,
                scale.factor(),
                SHELL_LABEL_TEXT_RGB,
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00011,
            ));
        }
        return;
    }
    paint.draw(Material::Popup, |out, _| {
        push_popup_instances(
            out,
            atlas,
            popup.rect,
            drop_down.hot_row.and_then(|row| popup.row_rect(row)),
            visible_rows.clone().filter_map(|row| {
                let at = scale.derive_rect(popup.row_rect(row)?, |r| {
                    RectPx::new(r.x + 2, r.y + 2, r.w - 4, r.h - 4)
                });
                Some((at, swatch(row)?))
            }),
            popup.scrollbar().zip(popup.thumb()).map(|(bar, thumb)| {
                let wide = RectPx::new(bar.x, thumb.y, bar.w, thumb.h);
                (bar, if style.skirmish_lists { wide } else { thumb })
            }),
            drop_down.pressed_part.map(dropdown_scroll_part),
            scale,
            SHELL_DROPDOWN_DEPTH - 0.00002,
        )
    });
    // The rows fit the list in their own glyph size.
    let glyph = style.text_factor.unwrap_or(scale.factor());
    let fit = combo_dropdown_text_fit_width(popup.content(), scale) as f32 * scale.factor() / glyph;
    for row in visible_rows {
        let (Some(at), Some((item, packed))) = (popup.row_rect(row), row_at(row)) else {
            continue;
        };
        let rgb = match packed {
            u32::MAX => style.rgb,
            c => [c as u8, (c >> 8) as u8, (c >> 16) as u8].map(|c| f32::from(c) / 255.0),
        };
        // `ComboDropWin` draws from x+3, the caption cut to the row less the
        // scrollbar reserve (`0x0060DF38`).
        let inset = COMBODROPWIN_TEXT_INSET_X;
        let at = scale.derive_rect(at, |r| RectPx::new(r.x + inset, r.y, r.w - 2 * inset, r.h));
        let mut draw = |item: &str| {
            paint.popup_texts.push(text(
                bit_font,
                item,
                at,
                ShellAlign::V_CENTER,
                glyph,
                rgb,
                SHELL_DROPDOWN_TEXT_DEPTH,
            ));
        };
        if style.skirmish_lists {
            fit_owner_draw_label(font, item, fit.round() as u32, draw);
        } else {
            draw(item);
        }
    }
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::render::shell_text::ScissorRect;
    use crate::ui::shell::geom::RETAIL_SCALE;
    use vera20k_gui::{DialogKind, InputEvent, MouseButton, NodeKind, NodeSpec};

    fn font() -> BitFont {
        crate::render::bit_font::tests::make_test_font(&[(u16::from(b'A'), 6)], 4)
    }

    fn entry(w: f32, h: f32, u: f32) -> SkirmishShellChromeEntry {
        SkirmishShellChromeEntry {
            uv_origin: [u, 0.0],
            uv_size: [0.1, 0.1],
            pixel_size: [w, h],
        }
    }

    fn side_entry(w: f32, h: f32, u: f32) -> SidebarChromeEntry {
        SidebarChromeEntry {
            uv_origin: [u, 0.0],
            uv_size: [0.1, 0.1],
            pixel_size: [w, h],
        }
    }

    fn same_sprites(a: &[SpriteInstance], b: &[SpriteInstance]) {
        assert_eq!(a.len(), b.len());
        assert_eq!(
            bytemuck::cast_slice::<SpriteInstance, u8>(a),
            bytemuck::cast_slice::<SpriteInstance, u8>(b)
        );
    }

    fn scissor(s: ScissorRect) -> [u32; 4] {
        [s.x, s.y, s.w, s.h]
    }

    fn same_texts(a: &[ShellTextDraw], b: &[ShellTextDraw]) {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!(scissor(a.scissor), scissor(b.scissor));
            same_sprites(&a.instances, &b.instances);
        }
    }

    /// `InGamePaint::owner_button` as it was before it called
    /// [`push_side_button`].
    fn hand_owner_button(
        font: &BitFont,
        side: SideButtons,
        rect: RectPx,
        caption: &str,
        look: PauseMenuButtonState,
    ) -> (Vec<SpriteInstance>, Vec<ShellTextDraw>) {
        let (mut art, mut texts) = (Vec::new(), Vec::new());
        let buttons = side.frames;
        if let Some(entry) = buttons[super::button_frame(look)].or(buttons[0]) {
            super::super::in_game_shell::push_art(&mut art, entry, rect, side.clip);
        }
        let rgb = super::button_text_rgb(side.theme, look.enabled && !look.highlighted);
        let at = super::button_text_rect(rect, look.pressed);
        texts.push(shell_text::draw_in_rect_scaled(
            font,
            caption,
            rect_to_text_rect(at),
            rgb,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        ));
        (art, texts)
    }

    /// The lanes the painter kept before the HUD paint held them: a
    /// vector each (`GuiPaint`).
    #[derive(Default)]
    struct OldLanes {
        controls: Vec<SpriteInstance>,
        side: Vec<SpriteInstance>,
        column: Vec<SpriteInstance>,
        icons: Vec<SpriteInstance>,
        popup: Vec<SpriteInstance>,
        page_art: Vec<SpriteInstance>,
        texts: Vec<ShellTextDraw>,
        popup_texts: Vec<ShellTextDraw>,
    }

    /// Each lane of `paint` is the old lane, sprite for sprite, and holds
    /// nothing else; the texts are the old texts.
    fn same_lanes(paint: &HudPaint, old: &OldLanes) {
        for (material, lane) in [
            (Material::Controls, &old.controls),
            (Material::Side, &old.side),
            (Material::Column, &old.column),
            (Material::Icons, &old.icons),
            (Material::Popup, &old.popup),
            (Material::PageArt, &old.page_art),
        ] {
            let painted: Vec<SpriteInstance> = paint.lane(material).copied().collect();
            same_sprites(&painted, lane);
        }
        let all: usize = paint.runs().iter().map(|(_, run)| run.len()).sum();
        assert_eq!(all, paint.instances().len());
        same_texts(&paint.texts, &old.texts);
        same_texts(&paint.popup_texts, &old.popup_texts);
    }

    /// The painter's old walk: a stack of nodes, a hidden one's subtree
    /// skipped.
    fn old_walk(gui: &Gui, root: NodeId) -> Vec<NodeId> {
        let mut walked = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if !flag(gui, node, Prop::Visible, true) {
                continue;
            }
            if let Ok(children) = gui.tree().children(node) {
                stack.extend(children.iter().rev());
            }
            walked.push(node);
        }
        walked
    }

    #[test]
    fn the_layouts_walk_visits_a_dialog_as_the_painters_old_walk() {
        let mut gui = Gui::new(32);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        let node = |gui: &mut Gui, kind, id, parent| {
            gui.create_tree(&NodeSpec::new(kind).id(id), Some(parent))
                .unwrap()
        };
        node(&mut gui, NodeKind::Button, 1, root);
        let hidden = node(&mut gui, NodeKind::Panel, 2, root);
        node(&mut gui, NodeKind::Text, 3, hidden);
        let panel = node(&mut gui, NodeKind::Panel, 4, root);
        node(&mut gui, NodeKind::Checkbox, 5, panel);
        let gone = node(&mut gui, NodeKind::Panel, 6, panel);
        node(&mut gui, NodeKind::Button, 7, gone);
        node(&mut gui, NodeKind::Text, 8, panel);
        node(&mut gui, NodeKind::Combo, 9, root);
        for off in [hidden, gone] {
            gui.set(off, Prop::Visible, Value::Bool(false)).unwrap();
        }
        for from in [root, panel, hidden] {
            let mut walked = Vec::new();
            gui.tree().walk_visible(from, |node| walked.push(node));
            assert_eq!(walked, old_walk(&gui, from));
        }
        let ids: Vec<_> = old_walk(&gui, root)
            .into_iter()
            .filter_map(|node| gui.id_of(node))
            .collect();
        assert_eq!(ids, [1, 4, 5, 8, 9]);
    }

    #[test]
    fn random_map_meter_uncovers_the_shape_between_empty_and_finished() {
        let shape = entry(100.0, 10.0, 0.25);
        let render = |percent| {
            let mut sprites = Vec::new();
            push_frame(
                &mut sprites,
                &mut Vec::new(),
                &font(),
                Some(entry(1.0, 1.0, 0.9)),
                RectPx::new(40, 50, 150, 21),
                FrameArt::RandomMapProgress {
                    shape,
                    outline: None,
                    percent,
                },
                None,
                TextLook {
                    align: ShellAlign::NONE,
                    glyph: 1.0,
                    rgb: [1.0; 3],
                },
                RETAIL_SCALE,
            );
            sprites
        };
        assert!(render(0).is_empty());
        let partial = render(60);
        let full = render(100);
        assert_eq!(partial.len(), 1);
        assert_eq!(full.len(), 1);
        assert_eq!(partial[0].position, [43.0, 53.0]);
        assert_eq!(partial[0].size, [60.0, 10.0]);
        assert_eq!(full[0].size, shape.pixel_size);
        assert!(partial[0].uv_size[0] < full[0].uv_size[0]);
        assert_eq!(partial[0].uv_origin, shape.uv_origin);
    }

    #[test]
    fn combo_requests_host_rows_only_for_its_open_popup() {
        let mut gui = Gui::new(8);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        let combo = gui
            .create_tree(&NodeSpec::new(NodeKind::Combo).id(7), Some(root))
            .unwrap();
        gui.set(combo, Prop::Rect, Value::Rect(RectPx::new(10, 20, 120, 24)))
            .unwrap();
        gui.set(combo, Prop::RowCount, Value::Int(1)).unwrap();
        gui.push_dialog(root, DialogKind::Page).unwrap();

        let calls = std::cell::Cell::new(0);
        let rows_of = |id| {
            assert_eq!(id, 7);
            calls.set(calls.get() + 1);
            Some(vec![("Host choice".to_owned(), 0x0000_009f)])
        };
        let style = GuiStyle {
            rows_of: Some(&rows_of),
            ..GuiStyle::default()
        };
        assert!(combo_popup_rows(&gui, combo, style).is_none());
        assert_eq!(calls.get(), 0);

        gui.open_popup(combo, 0).unwrap();
        let (_, rows) = combo_popup_rows(&gui, combo, style).unwrap();
        assert_eq!(rows.unwrap(), [("Host choice".to_owned(), 0x0000_009f)]);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn an_in_game_dialogs_lanes_are_the_old_lanes() {
        // FF's pause menu as it paints: a side button, the group box, a
        // checkbox, the second side button, a connection bar and a sunken
        // field, each into its lane, their texts in order.
        let font = font();
        let white = Some(entry(1.0, 1.0, 0.9));
        let side = SideButtons {
            frames: [
                Some(side_entry(125.0, 25.0, 0.0)),
                Some(side_entry(125.0, 25.0, 0.2)),
                Some(side_entry(125.0, 25.0, 0.4)),
            ],
            theme: SidebarTheme::Allied,
            clip: RectPx::new(0, 0, 800, 600),
        };
        let chrome = crate::render::skirmish_shell_chrome::ControlChrome {
            checkbox_unchecked_cue_i: Some(entry(18.0, 18.0, 0.0)),
            checkbox_checked_cce_i: Some(entry(18.0, 18.0, 0.2)),
            ..Default::default()
        };
        let look = TextLook {
            align: ShellAlign::NONE,
            glyph: 1.0,
            rgb: SHELL_LABEL_TEXT_RGB,
        };
        let up = PauseMenuButtonState {
            enabled: true,
            ..Default::default()
        };
        let down = PauseMenuButtonState {
            pressed: true,
            ..up
        };
        let (first, second) = (
            RectPx::new(653, 227, 125, 25),
            RectPx::new(653, 262, 125, 25),
        );
        let group = RectPx::new(100, 300, 400, 120);
        let checkbox = || ControlPaint::Checkbox {
            checked: true,
            rect: RectPx::new(120, 330, 120, 18),
        };
        let bar = BarLook {
            percent: 42,
            outline: [0.5; 3],
            fill: [0.0, 0.8, 0.0],
        };
        let field = RectPx::new(92, 394, 191, 28);
        let plain = FrameArt::Plain { outer_light: true };
        let s = RETAIL_SCALE;

        let mut old = OldLanes::default();
        push_side_button(
            &mut old.side,
            &mut old.texts,
            &font,
            side,
            first,
            Some("AAA"),
            up,
            1.0,
        );
        let caption = Some(("AAA", true));
        push_frame(
            &mut old.controls,
            &mut old.texts,
            &font,
            white,
            group,
            FrameArt::Group,
            caption,
            look,
            s,
        );
        paint_control(&mut old.controls, &chrome, checkbox(), s);
        push_side_button(
            &mut old.side,
            &mut old.texts,
            &font,
            side,
            second,
            Some("A"),
            down,
            1.0,
        );
        push_bar(&mut old.controls, white, RectPx::new(500, 120, 96, 14), bar);
        push_frame(
            &mut old.controls,
            &mut old.texts,
            &font,
            white,
            field,
            plain,
            None,
            look,
            s,
        );

        let mut paint = HudPaint::default();
        paint.draw(Material::Side, |art, texts| {
            push_side_button(art, texts, &font, side, first, Some("AAA"), up, 1.0)
        });
        paint.draw(Material::Controls, |out, texts| {
            push_frame(
                out,
                texts,
                &font,
                white,
                group,
                FrameArt::Group,
                caption,
                look,
                s,
            )
        });
        paint.draw(Material::Controls, |out, _| {
            paint_control(out, &chrome, checkbox(), s)
        });
        paint.draw(Material::Side, |art, texts| {
            push_side_button(art, texts, &font, side, second, Some("A"), down, 1.0)
        });
        paint.draw(Material::Controls, |out, _| {
            push_bar(out, white, RectPx::new(500, 120, 96, 14), bar)
        });
        paint.draw(Material::Controls, |out, texts| {
            push_frame(out, texts, &font, white, field, plain, None, look, s)
        });

        assert!(!old.side.is_empty() && !old.controls.is_empty() && !old.texts.is_empty());
        same_lanes(&paint, &old);
        // Neighbours of one lane share a run: side, controls, side, controls.
        let lanes: Vec<_> = paint.runs().iter().map(|(material, _)| *material).collect();
        let (c, d) = (Material::Controls, Material::Side);
        assert_eq!(lanes, [d, c, d, c]);
    }

    #[test]
    fn a_combos_open_list_keeps_its_popup_lane_and_texts_apart() {
        // A combo's face and an open 0x105-style list (fill, outline, the
        // selected row) with its rows' texts, then a list's scrollbar under
        // it, then a second open list: the popup lane holds both lists in
        // order, the controls lane the face and the bar.
        let font = font();
        let white = Some(entry(1.0, 1.0, 0.9));
        let chrome = crate::render::skirmish_shell_chrome::ControlChrome {
            combo_face_150: Some(entry(150.0, 20.0, 0.1)),
            combo_arrow_down_released: Some(entry(16.0, 16.0, 0.3)),
            white_pixel: white,
            scrollbar_arrow_up_released: Some(entry(16.0, 16.0, 0.5)),
            scrollbar_arrow_down_released: Some(entry(16.0, 16.0, 0.6)),
            scrollbar_thumb_top: Some(entry(16.0, 4.0, 0.7)),
            scrollbar_thumb_mid: Some(entry(16.0, 1.0, 0.75)),
            scrollbar_thumb_bottom: Some(entry(16.0, 4.0, 0.8)),
            ..Default::default()
        };
        let face = RectPx::new(200, 100, 150, 20);
        let combo = || ControlPaint::Combo {
            rect: face,
            swatch: None,
            open: true,
            disabled: false,
        };
        let list = RectPx::new(200, 120, 150, 80);
        let row = RectPx::new(201, 141, 148, 20);
        let open = |out: &mut Vec<SpriteInstance>, rect: RectPx| {
            let depth = super::super::SHELL_DROPDOWN_DEPTH - 0.00015;
            super::super::chrome::push_solid_rect_px(out, white, rect, [0.1; 3], depth);
            super::super::chrome::push_rect_outline_px(out, white, rect, [0.5; 3], depth);
            super::super::chrome::push_solid_rect_px(out, white, row, [0.2; 3], depth);
        };
        let row_text = |at: RectPx| {
            shell_text::draw_in_rect_scaled(
                &font,
                "AA",
                rect_to_text_rect(at),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::V_CENTER,
                [0.0; 2],
                SHELL_DROPDOWN_TEXT_DEPTH,
                1.0,
            )
        };
        let bar = || ControlPaint::ScrollBar {
            scrollbar: RectPx::new(400, 100, 16, 120),
            thumb: RectPx::new(400, 116, 16, 30),
            pressed_part: None,
        };
        let second = RectPx::new(450, 120, 150, 80);
        let s = RETAIL_SCALE;

        let mut old = OldLanes::default();
        paint_control(&mut old.controls, &chrome, combo(), s);
        old.texts.push(row_text(face));
        open(&mut old.popup, list);
        old.popup_texts.push(row_text(row));
        paint_control(&mut old.controls, &chrome, bar(), s);
        open(&mut old.popup, second);
        old.popup_texts.push(row_text(second));

        let mut paint = HudPaint::default();
        paint.draw(Material::Controls, |out, _| {
            paint_control(out, &chrome, combo(), s)
        });
        paint.texts.push(row_text(face));
        paint.draw(Material::Popup, |out, _| open(out, list));
        paint.popup_texts.push(row_text(row));
        paint.draw(Material::Controls, |out, _| {
            paint_control(out, &chrome, bar(), s)
        });
        paint.draw(Material::Popup, |out, _| open(out, second));
        paint.popup_texts.push(row_text(second));

        assert!(!old.controls.is_empty() && !old.popup.is_empty());
        same_lanes(&paint, &old);
    }

    #[test]
    fn a_side_button_paints_as_the_hand_owner_button() {
        let font = font();
        for theme in [SidebarTheme::Allied, SidebarTheme::Soviet] {
            for frames in [
                [
                    Some(side_entry(125.0, 25.0, 0.0)),
                    Some(side_entry(125.0, 25.0, 0.2)),
                    Some(side_entry(125.0, 25.0, 0.4)),
                ],
                // A missing frame falls back to the released one.
                [Some(side_entry(125.0, 25.0, 0.0)), None, None],
            ] {
                let side = SideButtons {
                    frames,
                    theme,
                    clip: RectPx::new(0, 0, 800, 600),
                };
                let rect = RectPx::new(653, 227, 125, 25);
                for bits in 0..8 {
                    let look = PauseMenuButtonState {
                        pressed: bits & 1 != 0,
                        highlighted: bits & 2 != 0,
                        enabled: bits & 4 != 0,
                    };
                    let (mut art, mut texts) = (Vec::new(), Vec::new());
                    push_side_button(
                        &mut art,
                        &mut texts,
                        &font,
                        side,
                        rect,
                        Some("AAA"),
                        look,
                        1.0,
                    );
                    let (hand_art, hand_texts) = hand_owner_button(&font, side, rect, "AAA", look);
                    same_sprites(&art, &hand_art);
                    same_texts(&texts, &hand_texts);
                }
            }
        }
    }

    #[test]
    fn side_and_type2_modal_buttons_push_only_while_held_and_caption_by_2_4() {
        let mut gui = Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        gui.push_dialog(root, DialogKind::Page).unwrap();
        let button = gui
            .create_tree(&NodeSpec::new(NodeKind::Button).id(1), Some(root))
            .unwrap();
        gui.set(button, Prop::Rect, Value::Rect(RectPx::new(0, 0, 50, 20)))
            .unwrap();
        let left = MouseButton::Left;
        gui.input(InputEvent::PointerMove { x: 5, y: 5 }, 0);
        gui.input(
            InputEvent::PointerDown {
                x: 5,
                y: 5,
                button: left,
            },
            0,
        );
        let held = crate::ui::shell::page::held_in(&gui, root);
        let captured = crate::ui::shell::page::captured_in(&gui, root);
        for art in [ButtonArt::Side, ButtonArt::ModalType2, ButtonArt::Panel] {
            assert_eq!(art.pushed(held, captured), Some(button));
        }
        // Off the button the press still holds it, but only the Panel
        // buttons keep their pushed look.
        gui.input(InputEvent::PointerMove { x: 90, y: 90 }, 0);
        let held = crate::ui::shell::page::held_in(&gui, root);
        let captured = crate::ui::shell::page::captured_in(&gui, root);
        assert_eq!(ButtonArt::Side.pushed(held, captured), None);
        assert_eq!(ButtonArt::ModalType2.pushed(held, captured), None);
        assert_eq!(ButtonArt::Panel.pushed(held, captured), Some(button));

        // Sound's Play/Stop and the connection rows: `button_text_rect`.
        let rect = RectPx::new(100, 200, 108, 23);
        for pressed in [false, true] {
            let centred = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
            let hand = (super::button_text_rect(rect, pressed), centred);
            for art in [ButtonArt::ModalType2, ButtonArt::Side] {
                assert_eq!(
                    art.caption_rect(rect, pressed, None, RETAIL_SCALE, false),
                    hand
                );
                let aligned = Some(ShellAlign::NONE);
                assert_eq!(
                    art.caption_rect(rect, pressed, aligned, RETAIL_SCALE, false),
                    hand
                );
            }
        }
        let (pressed_at, _) =
            ButtonArt::ModalType2.caption_rect(rect, true, None, RETAIL_SCALE, false);
        assert_eq!(pressed_at, RectPx::new(102, 205, 104, 18));
    }

    #[test]
    fn a_group_box_and_a_plain_frame_paint_as_the_hand_frames() {
        use super::super::controls::{group_box_geometry, push_group_or_plain_frame};
        let font = font();
        let white = Some(entry(1.0, 1.0, 0.9));
        let group = RectPx::new(100, 300, 400, 120);
        let look = TextLook {
            align: ShellAlign::NONE,
            glyph: 1.0,
            rgb: SHELL_LABEL_TEXT_RGB,
        };
        // FF's Internet Game Controls box (pause_menu.rs).
        let (frame, caption_rect) =
            group_box_geometry(group, font.cell_height() as i32, RETAIL_SCALE);
        let mut hand = Vec::new();
        let width = Some(font.text_width("AAA") as i32);
        push_group_or_plain_frame(&mut hand, white, frame, width, false, RETAIL_SCALE);
        let hand_text = vec![shell_text::draw_in_rect_scaled(
            &font,
            "AAA",
            rect_to_text_rect(caption_rect),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::NONE,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        )];
        let (mut sprites, mut texts) = (Vec::new(), Vec::new());
        let caption = Some(("AAA", true));
        let at = RETAIL_SCALE;
        push_frame(
            &mut sprites,
            &mut texts,
            &font,
            white,
            group,
            FrameArt::Group,
            caption,
            look,
            at,
        );
        same_sprites(&sprites, &hand);
        same_texts(&texts, &hand_text);
        // Under an overlay the caption goes, its gap stays.
        let (mut covered, mut none) = (Vec::new(), Vec::new());
        let caption = Some(("AAA", false));
        push_frame(
            &mut covered,
            &mut none,
            &font,
            white,
            group,
            FrameArt::Group,
            caption,
            look,
            at,
        );
        same_sprites(&covered, &hand);
        assert!(none.is_empty());

        // A3's hotkey field (keyboard.rs): the grown rect, light outside.
        let field = RectPx::new(92, 394, 191, 28);
        let mut hand = Vec::new();
        push_group_or_plain_frame(&mut hand, white, field, None, true, RETAIL_SCALE);
        let (mut sprites, mut texts) = (Vec::new(), Vec::new());
        let plain = FrameArt::Plain { outer_light: true };
        push_frame(
            &mut sprites,
            &mut texts,
            &font,
            white,
            field,
            plain,
            None,
            look,
            at,
        );
        same_sprites(&sprites, &hand);
        assert!(texts.is_empty());
    }

    #[test]
    fn checkbox_faces_follow_checked_and_the_partner_flag() {
        use crate::ui::shell::diplomacy::DiplomacyAlliesBox;
        let mut gui = Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        let [single, dual] = [1, 2].map(|id| {
            gui.create_tree(&NodeSpec::new(NodeKind::Checkbox).id(id), Some(root))
                .unwrap()
        });
        for checked in [false, true] {
            for node in [single, dual] {
                gui.set(node, Prop::Checked, Value::Bool(checked)).unwrap();
            }
            let face = match checked {
                true => CheckboxFace::Checked,
                false => CheckboxFace::Unchecked,
            };
            assert_eq!(checkbox_face(&gui, single), face);
            for partner in [false, true] {
                gui.set(dual, Prop::Selected, Value::Int(i32::from(partner)))
                    .unwrap();
                let allies = DiplomacyAlliesBox {
                    checked,
                    partner,
                    enabled: true,
                };
                assert_eq!(checkbox_face(&gui, dual), allies.face());
            }
        }

        // The single-state face is the plain checkbox's paint; disabled,
        // it is diplomacy's dimmed one.
        let chrome = crate::render::skirmish_shell_chrome::ControlChrome {
            checkbox_unchecked_cue_i: Some(entry(18.0, 18.0, 0.0)),
            checkbox_checked_cce_i: Some(entry(18.0, 18.0, 0.2)),
            checkbox_dual_left_cce_il: Some(entry(18.0, 18.0, 0.4)),
            checkbox_dual_right_cce_ir: Some(entry(18.0, 18.0, 0.6)),
            ..Default::default()
        };
        let rect = RectPx::new(40, 50, 120, 18);
        for checked in [false, true] {
            let (mut plain, mut face) = (Vec::new(), Vec::new());
            paint_control(
                &mut plain,
                &chrome,
                ControlPaint::Checkbox { rect, checked },
                RETAIL_SCALE,
            );
            let face_of = match checked {
                true => CheckboxFace::Checked,
                false => CheckboxFace::Unchecked,
            };
            let control = ControlPaint::StatefulCheckbox {
                face: face_of,
                disabled: false,
                rect,
            };
            paint_control(&mut face, &chrome, control, RETAIL_SCALE);
            same_sprites(&face, &plain);
        }
    }

    #[test]
    fn a_bar_paints_as_the_hand_connection_bar() {
        use crate::ui::shell::connection::bar_paint;
        use vera20k_gui::shell::bar::OUTLINE_RGB as BAR_OUTLINE_RGB;
        let white = Some(entry(1.0, 1.0, 0.9));
        let rgb = |unit: [u8; 3]| unit.map(|c| f32::from(c) / 255.0);
        let rect = RectPx::new(500, 120, 96, 14);
        for (lag, age) in [
            (0, 0),
            (100, 0),
            (300, 0),
            (600, 100),
            (600, 900),
            (2000, 9000),
        ] {
            let bar = bar_paint(rect, lag, age);
            // connection.rs before the node: outline, then the fill.
            let mut hand = Vec::new();
            super::super::chrome::push_rect_outline_px(
                &mut hand,
                white,
                bar.outline,
                rgb(BAR_OUTLINE_RGB),
                SHELL_CONTROL_DEPTH,
            );
            super::super::chrome::push_solid_rect_px(
                &mut hand,
                white,
                bar.fill,
                rgb(bar.fill_rgb),
                SHELL_CONTROL_DEPTH - 0.00001,
            );
            let look = BarLook {
                percent: bar.percent,
                outline: rgb(BAR_OUTLINE_RGB),
                fill: rgb(bar.fill_rgb),
            };
            let mut sprites = Vec::new();
            push_bar(&mut sprites, white, rect, look);
            same_sprites(&sprites, &hand);
        }
        // The host puts them on a Bar node.
        let mut gui = Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        let node = gui
            .create_tree(&NodeSpec::new(NodeKind::Bar).id(1), Some(root))
            .unwrap();
        gui.set(node, Prop::Fill, Value::Int(42)).unwrap();
        gui.set(
            node,
            Prop::Color,
            Value::Rgba(vera20k_gui::Rgba(0, 200, 0, 255)),
        )
        .unwrap();
        assert_eq!(int(&gui, node, Prop::Fill, 100), 42);
        assert_eq!(color(&gui, node, Prop::Color), Some(rgb([0, 200, 0])));
    }

    #[test]
    fn a_shell_bar_is_the_quads_it_drew_before_one_quad_drew_them() {
        use vera20k_gui::Rgba;
        let white = entry(1.0, 1.0, 0.9);
        // `entry_sprite` with the tint over it, as `push_tinted_entry` built
        // each by hand.
        let hand_quad = |rect: RectPx, tint: [f32; 3], depth: f32| SpriteInstance {
            position: [rect.x as f32, rect.y as f32],
            size: [rect.w as f32, rect.h as f32],
            uv_origin: white.uv_origin,
            uv_size: white.uv_size,
            depth,
            tint,
            alpha: 1.0,
            ..Default::default()
        };
        let rect = RectPx::new(500, 120, 96, 14);
        for percent in [0, 42, 100] {
            let look = BarLook {
                percent,
                outline: rgba_tint(Rgba(90, 91, 92, 255)).0,
                fill: [0.0, 0.8, 0.0],
            };
            let mut hand = Vec::new();
            for edge in bar::outline_edges(rect).into_iter().flatten() {
                hand.push(hand_quad(edge, look.outline, SHELL_CONTROL_DEPTH));
            }
            let fill = bar::fill_rect(rect, percent);
            hand.push(hand_quad(fill, look.fill, SHELL_CONTROL_DEPTH - 0.00001));
            let mut sprites = Vec::new();
            push_bar(&mut sprites, Some(white), rect, look);
            same_sprites(&sprites, &hand);
        }
        // The skin's colours take the HUD's one conversion, bit for bit
        // the skin's own.
        for c in 0..=255u8 {
            let (rgb, alpha) = rgba_tint(Rgba(c, c, c, c));
            let old = f32::from(c) / 255.0;
            assert!(rgb.iter().all(|v| v.to_bits() == old.to_bits()));
            assert_eq!(alpha.to_bits(), old.to_bits());
        }
    }

    #[test]
    fn an_image_node_keeps_its_picture_name_without_reallocating() {
        let mut gui = Gui::new(16);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        let node = gui
            .create_tree(&NodeSpec::new(NodeKind::Image).id(7), Some(root))
            .unwrap();
        crate::ui::shell::page::set_text(&mut gui, root, 7, Prop::Art, "usai.pcx");
        let before = match gui.get(node, Prop::Art) {
            Ok(Some(Value::Text(name))) => name.as_ptr(),
            _ => panic!("no picture"),
        };
        // The same name again is no new string.
        crate::ui::shell::page::set_text(&mut gui, root, 7, Prop::Art, "usai.pcx");
        let Ok(Some(Value::Text(name))) = gui.get(node, Prop::Art) else {
            panic!("no picture");
        };
        assert_eq!(name.as_ptr(), before);
        crate::ui::shell::page::set_text(&mut gui, root, 7, Prop::Art, "rusi.pcx");
        assert_eq!(
            gui.get(node, Prop::Art).unwrap(),
            Some(&Value::Text("rusi.pcx".into()))
        );
    }

    #[test]
    fn full_panel_and_column_captions_share_width_and_keep_the_pressed_anchor() {
        use crate::ui::shell::geom::{RETAIL_SCALE, shell_scale};

        let button = RectPx::new(644, 199, 156, 42);
        let align = ShellAlign::ART_CAPTION | ShellAlign::V_CENTER;
        for (pressed, expected) in [
            (false, RectPx::new(664, 200, 114, 41)),
            (true, RectPx::new(665, 204, 114, 37)),
        ] {
            for scale in [
                RETAIL_SCALE,
                shell_scale(1024, 768),
                shell_scale(1600, 1200),
            ] {
                for (art, standard) in [(ButtonArt::Column, false), (ButtonArt::Panel, true)] {
                    assert_eq!(
                        art.caption_rect(scale.scale_rect(button), pressed, None, scale, standard),
                        (scale.scale_rect(expected), align),
                    );
                }
            }
        }
    }

    #[test]
    fn other_button_faces_and_explicit_alignment_keep_their_caption_fit() {
        let button = RectPx::new(644, 199, 156, 42);
        let narrow = RectPx::new(690, 199, 110, 42);
        for (art, standard, rect) in [
            (ButtonArt::Panel, false, button),
            (ButtonArt::Panel, true, narrow),
            (ButtonArt::ModalSqueezed, false, button),
        ] {
            assert_eq!(
                art.caption_rect(rect, true, None, RETAIL_SCALE, standard),
                (
                    button_label_rect_px(rect, true, RETAIL_SCALE),
                    ShellAlign::BUTTON_CAPTION,
                ),
            );
        }
        assert_eq!(
            ButtonArt::PanelCentred.caption_rect(button, true, None, RETAIL_SCALE, true),
            (button, ShellAlign::H_CENTER | ShellAlign::V_CENTER),
        );
        for art in [ButtonArt::Panel, ButtonArt::Column] {
            assert_eq!(
                art.caption_rect(button, true, Some(ShellAlign::NONE), RETAIL_SCALE, true),
                (button, ShellAlign::NONE),
            );
        }
    }

    #[test]
    fn disabled_type2_changes_side_specific_text_without_dimming_art() {
        for theme in [
            SidebarTheme::Allied,
            SidebarTheme::Soviet,
            SidebarTheme::Yuri,
        ] {
            assert_eq!(button_text_rgb(theme, true), SHELL_LABEL_TEXT_RGB);
        }
        assert_eq!(
            button_text_rgb(SidebarTheme::Allied, false),
            [0.0, 80.0 / 255.0, 112.0 / 255.0]
        );
        assert_eq!(
            button_text_rgb(SidebarTheme::Soviet, false),
            [72.0 / 255.0, 0.0, 0.0]
        );
        assert_eq!(
            button_text_rgb(SidebarTheme::Yuri, false),
            button_text_rgb(SidebarTheme::Soviet, false)
        );
        let released = PauseMenuButtonState {
            enabled: false,
            ..Default::default()
        };
        assert_eq!(button_frame(released), 0);
        assert_eq!(
            button_frame(PauseMenuButtonState {
                highlighted: true,
                ..released
            }),
            2
        );
        assert_eq!(
            button_frame(PauseMenuButtonState {
                pressed: true,
                highlighted: true,
                ..released
            }),
            1
        );
    }

    #[test]
    fn pressed_caption_preserves_native_right_and_bottom_clip_edges() {
        let rect = RectPx::new(653, 227, 125, 25);
        assert_eq!(
            button_text_rect(rect, false),
            RectPx::new(653, 228, 123, 24)
        );
        assert_eq!(button_text_rect(rect, true), RectPx::new(655, 232, 121, 20));
    }
}
