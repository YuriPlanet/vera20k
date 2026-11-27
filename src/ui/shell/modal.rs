//! The front-end's centred 451x326 boxes: the message and wait boxes
//! ([`ShellBox`], their templates in `shell_layout.ini`) and the WOL and LAN
//! popups that share the box.

use super::descriptor::DialogResourceId;
use super::geom::{self, RectPx, ScaledRects};
use super::layout::RectTable;

/// The count-rule message box's resource control ids (ShellMessageBox__Run
/// `0x005D3490`), in the order the box answers with them: OK 0, Cancel 1,
/// the third button 2.
pub mod control {
    pub const OK: u16 = 0x05AE;
    /// Win32 IDCANCEL.
    pub const CANCEL: u16 = 0x0002;
    /// Win32 IDOK: no button carries it. IsDialogMessage sends it for Enter
    /// and IDCANCEL for Escape; MessageBoxProc `0x005D36A0` answers both
    /// with 1 (`0x005D370D`), so the box's hidden IDOK takes both keys.
    pub const IDOK: u16 = 0x0001;
    pub const THIRD: u16 = 0x05AF;
}

/// Pixel client size of the 300x200 DLU box templates (F0/CE/120/C4). The
/// common 775BC0 resize adds one to the converted extents. Executed F0
/// controls and 777060/777080 centering: tools/storage_oracle/saved_game_layout.json.
pub const MESSAGE_BOX_W: i32 = 451;
pub const MESSAGE_BOX_H: i32 = 326;

/// The wait box's template, whose body is centred (`0x5B2` is SS_CENTER).
pub const WAIT_TEMPLATE: &str = "0xC4";
/// VERA's Esc menu: the three-button template, its buttons in a footer.
pub const FOOTER_TEMPLATE: &str = "0x121.Footer";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxButton {
    pub id: u16,
    pub caption: String,
    pub enabled: bool,
}

/// One front-end message or wait box as it paints and takes the pointer: its
/// dialog resource, body and buttons. A box answers with the index of the
/// button pressed; its owner keeps what the answer leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellBox {
    pub dialog: DialogResourceId,
    pub body: String,
    pub buttons: Vec<BoxButton>,
    /// Its `shell_layout.ini` section; without one, the count-rule template
    /// for its number of buttons (`0xCE`, `0x120`, `0x121`).
    pub template: Option<&'static str>,
    /// Centred on the whole window as gamemd centres it (a box over a
    /// match), not on [`menu_area`].
    pub on_screen: bool,
    /// VERA's footer arrangement ([`Footer`]); `None` keeps the template's.
    pub footer: Option<Footer>,
}

/// VERA's own arrangement of a count-rule box, Add Mod's
/// (`app::shell_mod_import`): the body fills the panel inside a 24-pixel
/// inset, OK, the third button and Cancel sit along its footer, and each
/// `raised` button stands one button height and 6 pixels above the footer
/// button it names. gamemd has no such box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
    /// The body is cut to its first 66 pixels, leaving room for a list
    /// under it.
    pub list: bool,
    /// `(button, footer button)`: the extra buttons, not counted by the
    /// count rule, each above its footer button.
    pub raised: &'static [(u16, u16)],
}

impl Footer {
    const INSET: i32 = 24;

    fn is_raised(&self, id: u16) -> bool {
        self.raised.iter().any(|&(raised, _)| raised == id)
    }

    /// `base`, the count-rule box, arranged; `ids` are the box's buttons.
    fn arrange(&self, base: BoxParts, ids: &[u16]) -> BoxParts {
        const INSET: i32 = Footer::INSET;
        let scale = base.scale;
        let mut body = scale.derive_rect(base.dialog, |dialog| {
            RectPx::new(
                dialog.x + INSET,
                dialog.y + INSET,
                dialog.w - 2 * INSET,
                dialog.h - 3 * INSET,
            )
        });
        if self.list {
            body = scale.derive_rect(body, |rect| RectPx::new(rect.x, rect.y, rect.w, 66));
        }
        let button = scale.base_rect(base.button(0));
        let place = |slot: i32| {
            scale.derive_rect(base.dialog, |dialog| {
                let x = match slot {
                    0 => INSET,
                    1 => (dialog.w - button.w) / 2,
                    _ => dialog.w - INSET - button.w,
                };
                RectPx::new(
                    dialog.x + x,
                    dialog.y + dialog.h - INSET - button.h,
                    button.w,
                    button.h,
                )
            })
        };
        let cancel = base.rect_of(control::CANCEL).is_some();
        let mut buttons: RectTable = base
            .buttons
            .iter()
            .map(|(id, rect)| match id {
                control::OK => (id, place(if cancel { 0 } else { 2 })),
                control::THIRD => (id, place(1)),
                control::CANCEL => (id, place(2)),
                _ => (id, rect),
            })
            .collect();
        for &(id, under) in self.raised {
            if let (true, Some(rect)) = (ids.contains(&id), buttons.get(under)) {
                let above = scale.derive_rect(rect, |rect| {
                    RectPx::new(rect.x, rect.y - rect.h - 6, rect.w, rect.h)
                });
                buttons.insert(id, above);
            }
        }
        BoxParts {
            body,
            buttons,
            ..base
        }
    }
}

/// A box's rects: its window, the body and each button by id, in the box's
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxParts {
    pub dialog: RectPx,
    pub body: RectPx,
    pub centred: bool,
    pub buttons: RectTable,
    pub scale: geom::ShellScale,
}

vera20k_gui::scaled_rects!(BoxParts { [scale] dialog, body, buttons; keep: centred });

impl BoxParts {
    pub fn rect_of(&self, id: u16) -> Option<RectPx> {
        self.buttons.get(id)
    }

    /// The `index`th button's rect (the first is OK).
    pub fn button(&self, index: usize) -> RectPx {
        self.buttons
            .iter()
            .nth(index)
            .map_or_else(RectPx::default, |(_, rect)| rect)
    }

    /// VERA's download bar under the body (the map downloads' boxes): six
    /// pixels below it, as wide, ten tall.
    pub fn progress_track(&self) -> RectPx {
        RectPx::new(self.body.x, self.body.y + self.body.h + 6, self.body.w, 10)
    }
}

/// Section `template` in the box centred on `area`, in authored pixels.
fn parts_in(template: &str, area: RectPx) -> BoxParts {
    let dialog = centered_modal_dialog_in(area, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let rects = super::layout_data::in_box(template, dialog);
    let mut rects = rects.iter();
    BoxParts {
        dialog,
        body: rects.next().map_or(dialog, |(_, rect)| rect),
        centred: template == WAIT_TEMPLATE,
        buttons: rects.collect(),
        scale: geom::RETAIL_SCALE,
    }
}

/// Section `template`'s box on a menu at the shell's mapping.
pub fn template_on(template: &str, width: i32, height: i32, scale: geom::ShellScale) -> BoxParts {
    geom::layout_on(scale, width, height, |w, h| {
        parts_in(template, menu_area(w, h))
    })
}

fn count_rule(buttons: usize) -> &'static str {
    ["0xCE", "0x120", "0x121"][buttons.clamp(1, 3) - 1]
}

/// ShellMessageBox__Run's box with `buttons` buttons (OK, Cancel, third)
/// centred on `area` at retail size: [`menu_area`] for a menu's, the
/// match's frame for one over a match.
pub fn message_box_parts(buttons: usize, area: RectPx) -> BoxParts {
    parts_in(count_rule(buttons), area)
}

/// A menu's count-rule box (see [`message_box_parts`]).
pub fn menu_message_box(buttons: usize, width: i32, height: i32) -> BoxParts {
    message_box_parts(buttons, menu_area(width, height))
}

/// A box's body static under `root` ([`ShellBox::nodes`]): its first
/// `Text` child without an id.
pub fn body_node(gui: &vera20k_gui::Gui, root: vera20k_gui::NodeId) -> Option<vera20k_gui::NodeId> {
    let children = gui.tree().children(root).ok()?;
    children.iter().copied().find(|node| {
        gui.tree().kind(*node) == Ok(vera20k_gui::NodeKind::Text) && gui.id_of(*node).is_none()
    })
}

/// Hides the body static under `root`, for a dialog that shows its box only
/// at times (the connection verify's mismatch).
pub fn hide_body(gui: &mut vera20k_gui::Gui, root: vera20k_gui::NodeId) {
    use vera20k_gui::{Prop, Value};
    if let Some(body) = body_node(gui, root) {
        super::page::set_if_changed(gui, body, Prop::Visible, Value::Bool(false));
    }
}

impl ShellBox {
    pub fn new(dialog: DialogResourceId, body: impl Into<String>) -> Self {
        Self {
            dialog,
            body: body.into(),
            buttons: Vec::new(),
            template: None,
            on_screen: false,
            footer: None,
        }
    }

    /// ShellMessageBox__Run's box: `body` over OK, and Cancel and the third
    /// button where its count rule gives them: a third caption makes
    /// `0x121`, a Cancel one `0x120`, else `0xCE` (an empty caption is absent).
    pub fn message(body: String, ok: String, cancel: String, third: String) -> Self {
        let count = if !third.is_empty() {
            3
        } else {
            1 + usize::from(!cancel.is_empty())
        };
        let ids = [control::OK, control::CANCEL, control::THIRD];
        let dialog = DialogResourceId([0xCE, 0x120, 0x121][count - 1]);
        let captions = [ok, cancel, third].into_iter().take(count);
        ids.into_iter()
            .zip(captions)
            .fold(Self::new(dialog, body), |b, (id, caption)| {
                b.button(id, caption)
            })
    }

    /// [`Self::message`] with OK only (`0xCE`).
    pub fn notice(body: String, ok: String) -> Self {
        Self::message(body, ok, String::new(), String::new())
    }

    pub fn button(mut self, id: u16, caption: impl Into<String>) -> Self {
        self.buttons.push(BoxButton {
            id,
            caption: caption.into(),
            enabled: true,
        });
        self
    }

    /// Greys button `id`, which then takes no press.
    pub fn disabled(mut self, id: u16, disabled: bool) -> Self {
        if let Some(button) = self.buttons.iter_mut().find(|b| b.id == id) {
            button.enabled = !disabled;
        }
        self
    }

    pub fn template(mut self, template: &'static str) -> Self {
        self.template = Some(template);
        self
    }

    pub fn on_screen(mut self) -> Self {
        self.on_screen = true;
        self
    }

    /// Arranged as [`Footer`] says.
    pub fn footer(mut self, footer: Footer) -> Self {
        self.footer = Some(footer);
        self
    }

    /// The answer button `id` gives: its index; Enter and Escape's IDOK 1.
    pub fn answer(&self, id: u16) -> Option<usize> {
        if id == control::IDOK {
            return Some(1);
        }
        self.buttons.iter().position(|b| b.id == id)
    }

    /// Renames button `id`.
    pub fn set_caption(&mut self, id: u16, caption: String) {
        if let Some(button) = self.buttons.iter_mut().find(|b| b.id == id) {
            button.caption = caption;
        }
    }

    /// Its rects on the shell's mapping: retail 640/800 layouts keep their
    /// native size, a larger window the 800x600 box scaled with the page.
    pub fn parts(&self, width: i32, height: i32, scale: geom::ShellScale) -> BoxParts {
        // A footer's raised buttons are not the count rule's.
        let counted: Vec<u16> = self
            .buttons
            .iter()
            .map(|b| b.id)
            .filter(|id| !self.footer.is_some_and(|footer| footer.is_raised(*id)))
            .collect();
        let template = self.template.unwrap_or(count_rule(counted.len()));
        let mut parts = match self.on_screen {
            true => parts_in(template, RectPx::new(0, 0, width, height)),
            false => template_on(template, width, height, scale),
        };
        // The template's buttons by position, under this box's ids.
        let rects: Vec<RectPx> = parts.buttons.iter().map(|(_, rect)| rect).collect();
        parts.buttons = counted.iter().copied().zip(rects).collect();
        match self.footer {
            Some(footer) => {
                let ids: Vec<u16> = self.buttons.iter().map(|b| b.id).collect();
                footer.arrange(parts, &ids)
            }
            None => parts,
        }
    }

    /// Its body static and buttons as nodes under its dialog root, in the
    /// order they paint, then the hidden IDOK. The body (`0x5B0`; the wait
    /// box's `0x5B2`) carries no id: no host's rect table names it, and
    /// [`Self::sync`] places it.
    pub fn nodes(&self) -> vera20k_gui::NodeSpec {
        use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};
        let body = NodeSpec::new(NodeKind::Text);
        let button = |b: &BoxButton| NodeSpec::new(NodeKind::Button).id(i32::from(b.id));
        let keys = NodeSpec::new(NodeKind::Button)
            .id(i32::from(control::IDOK))
            .with(Prop::Default, Value::Bool(true))
            .with(Prop::Cancel, Value::Bool(true));
        NodeSpec::new(NodeKind::Dialog)
            .child(body)
            .children(self.buttons.iter().map(button))
            .child(keys)
    }

    /// Puts the box on `parts` in its widgets under `root`, the one tree its
    /// input hits and its paint reads, whichever spec built them (its own
    /// [`Self::nodes`] or a host's): the body static (added on the first
    /// sync when the spec has none) with its text, alignment and rect, and
    /// each of its buttons' caption, enable and rect. Each is written only
    /// when it changes, so a steady box writes and allocates nothing.
    pub fn sync(&self, gui: &mut vera20k_gui::Gui, root: vera20k_gui::NodeId, parts: &BoxParts) {
        use super::page::{set_if_changed, set_node_text};
        use vera20k_gui::{NodeKind, Prop, Value};
        let body = body_node(gui, root).or_else(|| gui.create(NodeKind::Text, Some(root)).ok());
        if let Some(body) = body {
            let align = [geom::ShellAlign::NONE, geom::ShellAlign::H_CENTER];
            let align = Value::Int(i32::from(align[usize::from(parts.centred)].0));
            set_node_text(gui, body, Prop::Text, &self.body);
            set_if_changed(gui, body, Prop::Align, align);
            set_if_changed(gui, body, Prop::Rect, Value::Rect(parts.body));
            set_if_changed(gui, body, Prop::Visible, Value::Bool(true));
        }
        // Each button's first node: the hidden IDOK may share an id.
        let mut synced = 0u64;
        let count = gui.tree().children(root).map_or(0, <[vera20k_gui::NodeId]>::len);
        for index in 0..count {
            let child = gui.tree().children(root).ok().and_then(|c| c.get(index).copied());
            let Some(node) = child else { break };
            let id = gui.id_of(node).and_then(|id| u16::try_from(id).ok());
            let Some(at) = id.and_then(|id| self.buttons.iter().position(|b| b.id == id)) else {
                continue;
            };
            let Some(bit) = 1u64.checked_shl(at as u32).filter(|bit| synced & bit == 0) else {
                continue;
            };
            synced |= bit;
            let button = &self.buttons[at];
            let rect = parts.rect_of(button.id);
            set_node_text(gui, node, Prop::Text, &button.caption);
            set_if_changed(gui, node, Prop::Enabled, Value::Bool(button.enabled));
            set_if_changed(gui, node, Prop::Rect, Value::Rect(rect.unwrap_or_default()));
            set_if_changed(gui, node, Prop::Visible, Value::Bool(rect.is_some()));
        }
    }

    /// Each button's enable, for its node.
    pub fn enabled(&self) -> Vec<(u16, bool)> {
        self.buttons.iter().map(|b| (b.id, b.enabled)).collect()
    }
}

/// The Westwood Online Create Game popup `0xC3` (`0x0078DB70`), the same
/// 451x326 centred box: executed screen rects (tools/storage_oracle/
/// wol_shell.json) of its controls from their templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolCreateGameLayout {
    pub dialog: RectPx,
    /// Static `GUI:Password`.
    pub password_label: RectPx,
    /// Edit `0x72C` after its NewEdit inset.
    pub password: RectPx,
    /// The clan and observer boxes; VERA's online popup has neither.
    pub clan: Option<RectPx>,
    pub limit: RectPx,
    pub observe: Option<RectPx>,
    /// VERA's online popup's Max players and Allow spectators.
    pub room: Option<crate::ui::room_limits::RoomLimitsLayout>,
    /// VERA's online popup's Room name caption and edit, a row above the
    /// password.
    pub name: Option<(RectPx, RectPx)>,
    /// VERA's online popup's Ranked game box, below Allow spectators.
    pub ranked: Option<RectPx>,
    /// VERA's online popup's Watching box, below Ranked game.
    pub watching: Option<RectPx>,
    pub ok: RectPx,
    pub cancel: RectPx,
    /// Its own status line `0x695`.
    pub status: RectPx,
}

impl WolCreateGameLayout {
    /// The left-hand title of an online setting, on the same baseline
    /// beside its control as Password is beside its edit.
    pub(crate) fn setting_label(&self, control: RectPx) -> RectPx {
        self.password_label
            .translate(0, control.y - self.password.y)
    }
}

/// Label and trackbar in one row of the in-room Game Options form, in
/// authored pixels. Create Room uses the same rows for its settings.
pub(crate) fn game_options_form_row(dialog: RectPx, index: usize) -> (RectPx, RectPx) {
    let y = dialog.y + 30 + 30 * index as i32;
    (
        RectPx::new(dialog.x + 24, y + 2, 185, 17),
        RectPx::new(dialog.x + 215, y, 129, 21),
    )
}

/// A NewEdit's window after `0x00614CCF`'s +1/-2 inset.
fn new_edit_window(rect: RectPx) -> RectPx {
    RectPx::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2)
}

/// The online popup's status line inset from each side of the box art's frame.
const ONLINE_STATUS_INSET: i32 = 10;

/// Width of the online Create Room player slider, including its value plaque.
pub(crate) const ONLINE_CREATE_PLAYERS_WIDTH: i32 = 161;

/// VERA's online lobby uses the in-room Game Options columns for its
/// labels and controls. The native lobby uses the 0xC3 template.
pub fn wol_create_game_layout(screen_w: i32, screen_h: i32, online: bool) -> WolCreateGameLayout {
    let dialog = menu_box(screen_w, screen_h);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    let field = child(133, 47, 140, 12);
    let clan = child(34, 76, 234, 14);
    let limit = child(34, 93, 234, 14);
    let observe = child(34, 110, 234, 14);
    let status = child(0, 186, 291, 12);
    let layout = WolCreateGameLayout {
        dialog,
        password_label: child(11, 48, 113, 12),
        password: new_edit_window(field),
        clan: Some(clan),
        limit,
        observe: Some(observe),
        room: None,
        name: None,
        ranked: None,
        watching: None,
        ok: child(33, 128, 83, 15),
        cancel: child(184, 128, 83, 15),
        status,
    };
    if !online {
        return layout;
    }
    let (name_label, name) = game_options_form_row(dialog, 0);
    let (password_label, password) = game_options_form_row(dialog, 1);
    let (players_label, players) = game_options_form_row(dialog, 2);
    let inset = name_label.x - dialog.x;
    let right = dialog.x + dialog.w - inset;
    let field = |rect: RectPx| RectPx {
        w: right - rect.x,
        ..rect
    };
    let check = |index| RectPx {
        h: 18,
        ..field(game_options_form_row(dialog, index).1)
    };
    let ok = RectPx::new(name_label.x, dialog.y + 292, layout.ok.w, layout.ok.h);
    WolCreateGameLayout {
        password_label,
        password: field(password),
        clan: None,
        observe: None,
        limit: check(3),
        room: Some(crate::ui::room_limits::RoomLimitsLayout {
            players_label,
            players: RectPx {
                w: ONLINE_CREATE_PLAYERS_WIDTH,
                ..players
            },
            spectators: check(4),
        }),
        name: Some((name_label, field(name))),
        ranked: Some(check(5)),
        watching: Some(check(6)),
        ok,
        cancel: RectPx {
            x: right - ok.w,
            ..ok
        },
        status: RectPx::new(name_label.x, dialog.y + 246, right - name_label.x, 34),
        ..layout
    }
}

/// The Create Game box on the lobby page's shell mapping: the retail
/// placement at `scale`'s retail size, otherwise the 800x600 box scaled with
/// the page, as [`ShellBox::parts`] does.
pub fn wol_create_game_layout_on(
    screen_w: i32,
    screen_h: i32,
    online: bool,
    scale: geom::ShellScale,
) -> WolCreateGameLayout {
    geom::layout_on(scale, screen_w, screen_h, |w, h| {
        wol_create_game_layout(w, h, online)
    })
}

vera20k_gui::scaled_rects!(WolCreateGameLayout {
    dialog,
    password_label,
    password,
    clan,
    limit,
    observe,
    room,
    name,
    ranked,
    watching,
    ok,
    cancel,
    status
});

/// VERA Online's welcome and player count are centred on the Create Room art.
/// The original MNBTTN buttons share their rectangles with pointer input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WolEntryChoiceLayout {
    pub dialog: RectPx,
    pub heading: RectPx,
    pub players_online: RectPx,
    pub guest: RectPx,
    pub account: RectPx,
}

pub(crate) fn wol_entry_choice_layout_on(
    area: RectPx,
    scale: geom::ShellScale,
) -> WolEntryChoiceLayout {
    let dialog = centered_modal_dialog_in(area, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let child = |x, y, w, h| scale.scale_rect(RectPx::new(dialog.x + x, dialog.y + y, w, h));
    let button = geom::child_window(0, 0, 99, 15);
    let gap = 16;
    let left = (dialog.w - 2 * button.w - gap) / 2;
    let top = dialog.h - 34 - button.h;
    let middle = dialog.h / 2;
    let line = |y, h| child(20, y, dialog.w - 40, h);
    WolEntryChoiceLayout {
        dialog: scale.scale_rect(dialog),
        heading: line(middle - 34, 32),
        players_online: line(middle, 28),
        guest: child(left, top, button.w, button.h),
        account: child(left + button.w + gap, top, button.w, button.h),
    }
}

/// VERA online's account popups (Create Account, its mailed code, Forgot
/// Password) on the online Create Game box: one caption and edit per field,
/// OK and Cancel, and a two-line status inside the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WolAccountFormLayout {
    pub dialog: RectPx,
    /// Each field's right-aligned caption and its edit.
    pub fields: Vec<(RectPx, RectPx)>,
    pub ok: RectPx,
    pub cancel: RectPx,
    pub status: RectPx,
}

/// Up to four fields, 18 units apart, centred above the buttons. The box
/// is centred on `area` in the shell's authored 800x600 units.
pub fn wol_account_form_layout(area: RectPx, count: usize) -> WolAccountFormLayout {
    let dialog = centered_modal_dialog_in(area, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    let count = count.min(4) as i32;
    let top = 40 + 9 * (4 - count);
    let row = |index: i32| top + 18 * index;
    let status = child(0, 172, 291, 24);
    WolAccountFormLayout {
        dialog,
        fields: (0..count)
            .map(|index| {
                let y = row(index);
                (
                    child(11, y + 1, 113, 12),
                    new_edit_window(child(133, y, 140, 12)),
                )
            })
            .collect(),
        ok: child(33, 128, 83, 15),
        cancel: child(184, 128, 83, 15),
        status: RectPx::new(
            dialog.x + ONLINE_STATUS_INSET,
            status.y,
            dialog.w - 2 * ONLINE_STATUS_INSET,
            status.h,
        ),
    }
}

/// The account popup on the login page's shell mapping, as
/// [`wol_create_game_layout_on`].
pub fn wol_account_form_layout_on(
    area: RectPx,
    count: usize,
    scale: geom::ShellScale,
) -> WolAccountFormLayout {
    let base = wol_account_form_layout(area, count);
    if scale.is_unscaled() {
        base
    } else {
        base.scaled_rects(scale)
    }
}

vera20k_gui::scaled_rects!(WolAccountFormLayout {
    dialog,
    fields,
    ok,
    cancel,
    status
});

/// VERA's player card in the online lobby on the online Create Game box: a
/// caption and value per row, Message and Add or Remove Buddy over Full
/// Profile and Close, and the status line inside the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolPlayerCardLayout {
    pub dialog: RectPx,
    /// Each row's right-aligned caption and its value.
    pub rows: [(RectPx, RectPx); PLAYER_CARD_ROWS],
    pub message: RectPx,
    pub buddy: RectPx,
    pub profile: RectPx,
    pub close: RectPx,
    pub status: RectPx,
}

pub const PLAYER_CARD_ROWS: usize = 5;

/// The account popups' caption and field columns 18 units apart from the
/// Room name row, and two rows of the online box's OK and Cancel buttons.
pub fn wol_player_card_layout(screen_w: i32, screen_h: i32) -> WolPlayerCardLayout {
    let dialog = menu_box(screen_w, screen_h);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    let status = child(0, 186, 291, 12);
    WolPlayerCardLayout {
        dialog,
        rows: std::array::from_fn(|row| {
            let y = 41 + 18 * row as i32;
            (child(11, y, 113, 12), child(133, y, 150, 12))
        }),
        message: child(33, 134, 83, 15),
        buddy: child(184, 134, 83, 15),
        profile: child(33, 152, 83, 15),
        close: child(184, 152, 83, 15),
        status: RectPx::new(
            dialog.x + ONLINE_STATUS_INSET,
            status.y,
            dialog.w - 2 * ONLINE_STATUS_INSET,
            status.h,
        ),
    }
}

/// The player card on the lobby page's shell mapping, as
/// [`wol_create_game_layout_on`].
pub fn wol_player_card_layout_on(
    screen_w: i32,
    screen_h: i32,
    scale: geom::ShellScale,
) -> WolPlayerCardLayout {
    geom::layout_on(scale, screen_w, screen_h, wol_player_card_layout)
}

vera20k_gui::scaled_rects!(WolPlayerCardLayout {
    dialog,
    rows,
    message,
    buddy,
    profile,
    close,
    status
});

/// VERA's Create Game box in the LAN lobby, the online box's art and
/// buttons: Max players in the password row, Allow spectators in the clan row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanCreateGameLayout {
    pub scale: geom::ShellScale,
    pub dialog: RectPx,
    pub room: crate::ui::room_limits::RoomLimitsLayout,
    pub ok: RectPx,
    pub cancel: RectPx,
}

vera20k_gui::scaled_rects!(LanCreateGameLayout { [scale] dialog, room, ok, cancel });

/// The Create Game box drawn and hit on the LAN lobby's shell mapping.
pub fn lan_create_game_layout_on(
    screen_w: i32,
    screen_h: i32,
    scale: geom::ShellScale,
) -> LanCreateGameLayout {
    geom::layout_on(scale, screen_w, screen_h, lan_create_game_layout)
}

pub fn lan_create_game_layout(screen_w: i32, screen_h: i32) -> LanCreateGameLayout {
    let native = wol_create_game_layout(screen_w, screen_h, false);
    let field = modal_child(native.dialog, geom::child_window(133, 47, 140, 12));
    let clan = native.clan.expect("the native box has its clan row");
    let players = RectPx::new(field.x, field.y, 129, 22);
    LanCreateGameLayout {
        scale: geom::RETAIL_SCALE,
        dialog: native.dialog,
        room: crate::ui::room_limits::RoomLimitsLayout {
            players_label: RectPx {
                y: players.y,
                h: players.h,
                ..native.password_label
            },
            players,
            spectators: RectPx::new(field.x, clan.y, field.w, clan.h),
        },
        ok: native.ok,
        cancel: native.cancel,
    }
}

/// The channel password popup `0xE1` (`0x0078E030`) on the lobby page's
/// shell mapping, as [`wol_create_game_layout_on`].
pub fn wol_channel_password_layout_on(w: i32, h: i32, scale: geom::ShellScale) -> WolPageLayout {
    geom::layout_on(scale, w, h, |w, h| {
        let dialog = menu_box(w, h);
        let rects = super::layout_data::in_box("0xE1", dialog);
        WolPageLayout { dialog, rects }
    })
}

/// The WOL page dialog `0xCF` (proc `0x0078D120`), the same 451x326
/// centred box: executed screen rects (tools/storage_oracle/wol_shell.json).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WolPageLayout {
    pub dialog: RectPx,
    pub rects: super::layout::RectTable,
}

vera20k_gui::scaled_rects!(WolPageLayout { dialog, rects });

impl WolPageLayout {
    /// Where `0xCF`'s control `id` sits.
    pub fn rect_of(&self, id: u16) -> Option<RectPx> {
        self.rects.get(id)
    }
}

pub fn wol_page_layout(screen_w: i32, screen_h: i32) -> WolPageLayout {
    wol_page_layout_in(menu_area(screen_w, screen_h))
}

/// The box centred on the given shell area.
pub fn wol_page_layout_in(area: RectPx) -> WolPageLayout {
    let dialog = centered_modal_dialog_in(area, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let rects = super::layout_data::in_box("0xCF", dialog);
    WolPageLayout { dialog, rects }
}

/// The page-received dialog `0x11E` (proc `0x0079DB50`), the same box.
pub type WolPageReceivedLayout = WolPageLayout;

pub fn wol_page_received_layout(screen_w: i32, screen_h: i32) -> WolPageReceivedLayout {
    wol_page_received_layout_in(menu_area(screen_w, screen_h))
}

/// The box centred on the given shell area.
pub fn wol_page_received_layout_in(area: RectPx) -> WolPageReceivedLayout {
    let dialog = centered_modal_dialog_in(area, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let rects = super::layout_data::in_box("0x11E", dialog);
    WolPageLayout { dialog, rects }
}

/// Add Buddy `0x115` (proc `0x0079DFE0`), the same 451x326 centred box.
pub type WolAddBuddyLayout = WolPageLayout;

pub fn wol_add_buddy_layout(screen_w: i32, screen_h: i32) -> WolAddBuddyLayout {
    let dialog = menu_box(screen_w, screen_h);
    let rects = super::layout_data::in_box("0x115", dialog);
    WolPageLayout { dialog, rects }
}

/// The connection verify `0x130` (`0x00794C5B`), the same 451x326 centred
/// box: the title `0x5B2` and node `i`'s checkbox 14 DLU below node `i-1`'s
/// ([`crate::ui::wol_verify::BOXES`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolVerifyLayout {
    pub scale: geom::ShellScale,
    pub dialog: RectPx,
    pub title: RectPx,
    pub boxes: [RectPx; 8],
}

pub fn wol_verify_layout(
    screen_w: i32,
    screen_h: i32,
    scale: geom::ShellScale,
) -> WolVerifyLayout {
    // Modern rooms carry the fitted room transform. The classic popup retains
    // its executed native geometry, even in a larger fitted shell.
    let dialog = if scale.is_unscaled() {
        menu_box(screen_w, screen_h)
    } else {
        menu_box(geom::SHELL_BASE_W, geom::SHELL_BASE_H)
    };
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    WolVerifyLayout {
        scale,
        dialog: scale.scale_rect(dialog),
        title: scale.scale_rect(child(59, 10, 180, 35)),
        boxes: std::array::from_fn(|node| {
            scale.scale_rect(child(33, 56 + 14 * node as i32, 234, 14))
        }),
    }
}

/// Where the front-end's boxes centre in a `screen_w`x`screen_h` window: the
/// page's content, left of its right button column and above its status strip,
/// wherever the shell's mapping for that window puts them.
///
/// VERA deviation: gamemd's CenterChildWindow (0x00777080) centres a box on
/// the whole screen, at 800x600 84 px right of the page's content and against
/// its button column. Boxes over a match keep that centring on the match's
/// frame (the `_in` layouts).
pub fn menu_area(screen_w: i32, screen_h: i32) -> RectPx {
    let content = |w, h| RectPx::new(0, 0, w - geom::RIGHT_PANEL_WIDTH, h - geom::LOWER_STRIP_H);
    let scale = geom::shell_scale_for_window(screen_w, screen_h);
    if scale.is_unscaled() {
        content(screen_w, screen_h)
    } else {
        scale.scale_rect(content(geom::SHELL_BASE_W, geom::SHELL_BASE_H))
    }
}

/// Where a menu's 451x326 box sits: centred on [`menu_area`].
pub fn menu_box(screen_w: i32, screen_h: i32) -> RectPx {
    centered_modal_dialog_in(menu_area(screen_w, screen_h), MESSAGE_BOX_W, MESSAGE_BOX_H)
}

/// Centre a panel of `(w, h)` on `area` (`((area - size) + 1) / 2`, clamped >= 0).
fn centered_modal_dialog_in(area: RectPx, w: i32, h: i32) -> RectPx {
    RectPx::new(
        area.x + (((area.w - w) + 1) / 2).max(0),
        area.y + (((area.h - h) + 1) / 2).max(0),
        w,
        h,
    )
}

/// Place a control at the dialog origin plus its DLU-derived local rect.
fn modal_child(dialog: RectPx, local: RectPx) -> RectPx {
    RectPx::new(dialog.x + local.x, dialog.y + local.y, local.w, local.h)
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    #[test]
    fn count_rule_picks_the_template_and_answers_by_button() {
        let s = |t: &str| t.to_owned();
        let notice = ShellBox::message(s("b"), s("OK"), s(""), s(""));
        assert_eq!(
            (notice.dialog, notice.buttons.len()),
            (DialogResourceId(0xCE), 1)
        );
        let quit = ShellBox::message(s("b"), s("OK"), s("Cancel"), s(""));
        assert_eq!(
            (quit.dialog, quit.answer(control::CANCEL)),
            (DialogResourceId(0x120), Some(1))
        );
        let three = ShellBox::message(s("b"), s("OK"), s(""), s("Maybe"));
        assert_eq!(
            (three.dialog, three.answer(control::THIRD)),
            (DialogResourceId(0x121), Some(2))
        );
        // At 800x600 the 0x120 box centres on the page left of the button
        // column, OK directly above Cancel.
        let parts = quit.parts(800, 600, geom::RETAIL_SCALE);
        assert_eq!(
            parts.dialog,
            RectPx::new(91, 121, MESSAGE_BOX_W, MESSAGE_BOX_H)
        );
        let child = |x, y, w, h| geom::child_window(x, y, w, h).translate(91, 121);
        assert_eq!(parts.body, child(40, 40, 220, 50));
        assert_eq!(parts.button(0), child(207, 155, 83, 15));
        assert_eq!(
            parts.rect_of(control::CANCEL),
            Some(child(207, 175, 83, 15))
        );
    }

    #[test]
    fn esc_menu_actions_share_a_balanced_footer() {
        let menu = ShellBox::message("".into(), "O".into(), "C".into(), "E".into())
            .template(FOOTER_TEMPLATE);
        for (w, h) in [(640, 480), (1024, 800)] {
            let scale = geom::shell_scale_for_window(w, h);
            let parts = menu.parts(w, h, scale);
            let panel = scale.base_rect(parts.dialog);
            let [options, cancel, exit] = [0, 1, 2].map(|i| scale.base_rect(parts.button(i)));
            let native = message_box_parts(3, panel).button(0);
            for button in [options, exit, cancel] {
                assert_eq!(
                    (button.y, button.w, button.h),
                    (options.y, native.w, native.h)
                );
            }
            let left_gap = exit.x - (options.x + options.w);
            let right_gap = cancel.x - (exit.x + exit.w);
            assert!(left_gap > 0 && (left_gap - right_gap).abs() <= 1);
            let inset = options.x - panel.x;
            assert!(inset > 0);
            assert_eq!(panel.x + panel.w - (cancel.x + cancel.w), inset);
            assert_eq!(panel.y + panel.h - (options.y + options.h), inset);
        }
    }

    #[test]
    fn menu_boxes_centre_on_the_page_left_of_the_button_column() {
        // The retail pages: the 632x568 and 472x448 areas left of the button
        // column and above the status strip.
        assert_eq!(menu_box(800, 600), RectPx::new(91, 121, 451, 326));
        assert_eq!(menu_box(640, 480), RectPx::new(11, 61, 451, 326));
        // A fitted page (x2 at 1600x1200) centres a retail-size box on its
        // scaled content, and a scaled box where the 800x600 one sits.
        assert_eq!(menu_box(1600, 1200), RectPx::new(407, 405, 451, 326));
        let scale = geom::shell_scale(1600, 1200);
        let notice = ShellBox::notice("b".into(), "OK".into());
        assert_eq!(
            notice.parts(1600, 1200, scale).dialog,
            scale.scale_rect(menu_box(800, 600))
        );
        // A box over a match keeps gamemd's centring on the frame; a screen
        // smaller than the box clamps its corner to the origin.
        let frame = |w, h| message_box_parts(2, RectPx::new(0, 0, w, h)).dialog;
        assert_eq!(frame(800, 600), RectPx::new(175, 137, 451, 326));
        assert_eq!(frame(320, 240), RectPx::new(0, 0, 451, 326));
    }

    /// `rect` from `dialog`'s corner. The executed tables centre each box on
    /// the screen; a menu's box centres on the page, its controls in place.
    fn local(rect: RectPx, dialog: RectPx) -> RectPx {
        rect.translate(-dialog.x, -dialog.y)
    }

    /// Saved-game loading's F0 shares the CE box: its body and hidden Cancel
    /// sit at CE's body and OK.
    #[test]
    fn boxes_match_the_executed_saved_progress_box() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/saved_game_layout.json",
        ))
        .unwrap();
        let rect = |value: &serde_json::Value| {
            let values: [i32; 4] = serde_json::from_value(value.clone()).unwrap();
            RectPx::new(values[0], values[1], values[2], values[3])
        };
        for row in corpus["progress_geometry"].as_array().unwrap() {
            let width = row["input"]["width"].as_i64().unwrap() as i32;
            let height = row["input"]["height"].as_i64().unwrap() as i32;
            let notice = message_box_parts(1, RectPx::new(0, 0, width, height));
            assert_eq!(notice.dialog, rect(&row["dialog_rect"]));
            for native in row["children"].as_array().unwrap() {
                let expected = rect(&native["rect"]);
                match native["id"].as_u64().unwrap() {
                    0x5B0 => assert_eq!(notice.body, expected),
                    0x5C0 => assert_eq!(notice.button(0), expected),
                    _ => panic!("unexpected F0 child"),
                }
            }
        }
    }

    #[test]
    fn wol_wait_box_matches_the_template_arithmetic_at_800x600() {
        // RT_DIALOG 0xC4 through the shared 300x200 DLU modal policy (the
        // relayout tool's MulDiv; the popup was not executed).
        let wait = ShellBox::new(DialogResourceId(0xC4), "")
            .button(0x58D, "D")
            .button(0x58E, "C")
            .template(WAIT_TEMPLATE)
            .parts(800, 600, geom::RETAIL_SCALE);
        assert_eq!(wait.dialog, RectPx::new(91, 121, 451, 326));
        assert_eq!(wait.button(0), RectPx::new(255, 292, 126, 25));
        assert_eq!(wait.body, RectPx::new(168, 144, 297, 89));
        assert_eq!(wait.button(1), RectPx::new(255, 332, 126, 25));
        assert!(wait.centred);
    }

    #[test]
    fn wol_lobby_popups_follow_the_executed_0xc3_0xe1_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let rect = |value: &serde_json::Value| {
            let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
            RectPx::new(a[0], a[1], a[2], a[3])
        };
        let mut checked = 0;
        for key in ["0xC3", "0xE1"] {
            for (size, case) in corpus["layouts"][key]["sizes"].as_object().unwrap() {
                let (w, h) = size.split_once('x').unwrap();
                let (w, h): (i32, i32) = (w.parse().unwrap(), h.parse().unwrap());
                let create = wol_create_game_layout(w, h, false);
                let password = wol_channel_password_layout_on(w, h, geom::RETAIL_SCALE);
                let at = |id| password.rect_of(id).unwrap();
                let native_dialog = rect(&case["dialog"]);
                assert_eq!(create.dialog.w, native_dialog.w, "{key} {size}");
                assert_eq!(create.dialog.h, native_dialog.h, "{key} {size}");
                assert_eq!(password.dialog, create.dialog);
                for (id, child) in case["controls"].as_object().unwrap() {
                    let mut native = rect(&child["screen_rect"]);
                    let actual = match (key, id.as_str()) {
                        ("0xC3", "0xFFFF") => create.password_label,
                        ("0xC3", "0x72C") => create.password,
                        ("0xC3", "0x66C") => create.clan.unwrap(),
                        ("0xC3", "0x71E") => create.limit,
                        ("0xC3", "0x71F") => create.observe.unwrap(),
                        ("0xC3", "0x1") => create.ok,
                        ("0xC3", "0x2") => create.cancel,
                        ("0xC3", "0x695") => create.status,
                        (_, "0xFFFF") => at(0x7FD0),
                        (_, "0x72C") => at(0x72C),
                        (_, "0x1") => at(1),
                        (_, "0x2") => at(2),
                        other => panic!("{other:?}"),
                    };
                    if id == "0x72C" {
                        native = new_edit_window(native);
                    }
                    assert_eq!(
                        local(actual, create.dialog),
                        local(native, native_dialog),
                        "{key} {size} {id}"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 36);
    }

    #[test]
    fn lobby_popups_grow_with_a_scaled_page() {
        let scale = geom::shell_scale(1600, 1200);
        let retail = wol_create_game_layout(800, 600, true);
        let scaled = wol_create_game_layout_on(1600, 1200, true, scale);
        assert_eq!(scaled.dialog, scale.scale_rect(retail.dialog));
        assert_eq!((scaled.dialog.w, scaled.dialog.h), (902, 652));
        assert_eq!(
            scaled.name.unwrap().1,
            scale.scale_rect(retail.name.unwrap().1)
        );
        let password = |w, h, scale| wol_channel_password_layout_on(w, h, scale).rect_of(1);
        let retail = password(800, 600, geom::RETAIL_SCALE).unwrap();
        assert_eq!(password(1600, 1200, scale), Some(scale.scale_rect(retail)));
        // A window the page keeps at retail size keeps the retail placement.
        assert_eq!(
            wol_create_game_layout_on(1024, 768, true, geom::RETAIL_SCALE),
            wol_create_game_layout(1024, 768, true)
        );
    }

    #[test]
    fn lan_create_game_box_and_controls_follow_a_wide_lobby() {
        let retail = lan_create_game_layout(800, 600);
        let scale = geom::shell_scale(1600, 900);
        let wide = lan_create_game_layout_on(1600, 900, scale);
        assert_eq!(wide.scale, scale);
        assert_eq!(wide.dialog, scale.scale_rect(retail.dialog));
        assert_eq!(wide.room.players, scale.scale_rect(retail.room.players));
        assert_eq!(
            wide.room.spectators,
            scale.scale_rect(retail.room.spectators)
        );
        assert_eq!(wide.ok, scale.scale_rect(retail.ok));
        assert_eq!(wide.cancel, scale.scale_rect(retail.cancel));
    }

    #[test]
    fn account_popups_keep_their_fields_between_the_title_and_the_buttons() {
        let screen = menu_area(800, 600);
        let create = wol_account_form_layout(screen, 4);
        let frame = create.dialog;
        let (_, first) = create.fields[0];
        let (_, last) = create.fields[3];
        assert!(last.y + last.h < create.ok.y);
        assert!(create.status.y >= create.ok.y + create.ok.h);
        assert!(create.status.y + create.status.h <= frame.y + frame.h);
        // One field sits mid-way between the four's first and last rows.
        let (_, code) = wol_account_form_layout(screen, 1).fields[0];
        assert_eq!((code.x, code.w), (first.x, first.w));
        assert!(((code.y - first.y) - (last.y - code.y)).abs() <= 1);
    }

    #[test]
    fn online_create_game_uses_the_in_room_game_options_columns() {
        use crate::ui::multiplayer_setup::{
            MultiplayerSetupLayout, SPEED, SetupDialog, SetupFlavor,
        };
        let options = MultiplayerSetupLayout::new(
            SetupDialog {
                host: true,
                flavor: SetupFlavor::Wol,
                modern: true,
            },
            800,
            600,
            false,
        );
        let online = wol_create_game_layout(800, 600, true);
        assert_eq!((online.clan, online.observe), (None, None));
        let (name_label, name) = online.name.unwrap();
        let room = online.room.unwrap();
        let watching = online.watching.unwrap();
        let bar = options.control_rect(SPEED).unwrap();
        assert_eq!(name_label, options.option_labels[0].1);
        assert_eq!(online.password_label, options.option_labels[1].1);
        assert_eq!(room.players_label, options.option_labels[2].1);
        assert!(bar.w < room.players.w && room.players.w < name.w);
        let rows = [
            name,
            online.password,
            room.players,
            online.limit,
            room.spectators,
            online.ranked.unwrap(),
            watching,
        ];
        let pitch = options.option_labels[1].1.y - name_label.y;
        for (index, row) in rows.into_iter().enumerate() {
            assert_eq!((row.x, row.y), (bar.x, bar.y + index as i32 * pitch));
            assert_eq!(
                online.setting_label(row),
                name_label.translate(0, index as i32 * pitch)
            );
            assert!(row.x + row.w <= online.dialog.x + online.dialog.w - ONLINE_STATUS_INSET);
        }
        assert!(watching.y + watching.h <= online.status.y);
        assert!(online.status.y + online.status.h <= online.ok.y);
        assert_eq!(online.cancel.y, online.ok.y);
        assert!(online.cancel.y + online.cancel.h <= online.dialog.y + online.dialog.h);
    }

    #[test]
    fn wol_page_dialogs_follow_the_executed_0xcf_0x11e_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let rect = |value: &serde_json::Value| {
            let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
            RectPx::new(a[0], a[1], a[2], a[3])
        };
        let mut checked = 0;
        for key in ["0xCF", "0x11E"] {
            for (size, case) in corpus["layouts"][key]["sizes"].as_object().unwrap() {
                let (w, h) = size.split_once('x').unwrap();
                let (w, h): (i32, i32) = (w.parse().unwrap(), h.parse().unwrap());
                // The executed tables centre on the screen.
                let screen = RectPx::new(0, 0, w, h);
                let page = wol_page_layout_in(screen);
                let received = wol_page_received_layout_in(screen);
                assert_eq!(page.dialog, rect(&case["dialog"]), "{key} {size}");
                assert_eq!(received.dialog, page.dialog);
                for (id, child) in case["controls"].as_object().unwrap() {
                    let mut native = rect(&child["screen_rect"]);
                    use crate::ui::wol_page::*;
                    let layout = if key == "0xCF" { &page } else { &received };
                    let control = match (key, id.as_str()) {
                        ("0xCF", "0xFFFF") => TEXT_CAPTION,
                        ("0xCF", "0xFFFF#1") => NAME_CAPTION,
                        ("0xCF", "0xFFFF#2") => STATUS_CAPTION,
                        (_, "0xFFFF") => TITLE_CAPTION,
                        (_, "0xFFFF#1") => REPLY_TO_CAPTION,
                        (_, id) => u16::from_str_radix(&id[2..], 16).unwrap(),
                    };
                    let actual = layout
                        .rect_of(control)
                        .unwrap_or_else(|| panic!("{key} {id}"));
                    if child["class"] == "Edit" {
                        native = new_edit_window(native);
                    }
                    assert_eq!(actual, native, "{key} {size} {id}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 48);
    }

    #[test]
    fn wol_add_buddy_follows_the_executed_0x115_table() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let rect = |value: &serde_json::Value| {
            let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
            RectPx::new(a[0], a[1], a[2], a[3])
        };
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x115"]["sizes"].as_object().unwrap() {
            let (w, h) = size.split_once('x').unwrap();
            let layout = wol_add_buddy_layout(w.parse().unwrap(), h.parse().unwrap());
            let native_dialog = rect(&case["dialog"]);
            assert_eq!(
                (layout.dialog.w, layout.dialog.h),
                (native_dialog.w, native_dialog.h),
                "{size}"
            );
            for (id, child) in case["controls"].as_object().unwrap() {
                let mut native = rect(&child["screen_rect"]);
                let control = match id.as_str() {
                    "0xFFFF" => crate::ui::wol_buddy::NICKNAME_CAPTION,
                    id => u16::from_str_radix(&id[2..], 16).unwrap(),
                };
                let actual = layout.rect_of(control).unwrap_or_else(|| panic!("{id}"));
                if child["class"] == "Edit" {
                    native = new_edit_window(native);
                }
                assert_eq!(
                    local(actual, layout.dialog),
                    local(native, native_dialog),
                    "{size} {id}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 15);
    }

    #[test]
    fn modern_wol_verify_uses_the_room_transform_without_changing_retail_sizes() {
        let native = wol_verify_layout(800, 600, geom::RETAIL_SCALE);
        assert_eq!(native.dialog, RectPx::new(91, 121, 451, 326));
        assert_eq!(
            wol_verify_layout(640, 480, geom::RETAIL_SCALE).dialog,
            RectPx::new(11, 61, 451, 326),
        );

        for (w, h) in [(320, 240), (1024, 768), (1600, 1200), (2048, 1152)] {
            let scale = geom::shell_scale_for_window(w, h).anchored_right();
            let layout = wol_verify_layout(w, h, scale);
            assert_eq!(layout.scale, scale);
            assert_eq!(layout.dialog, scale.scale_rect(native.dialog));
            assert_eq!(layout.title, scale.scale_rect(native.title));
            assert_eq!(layout.boxes, native.boxes.map(|r| scale.scale_rect(r)));
            for rect in [layout.dialog, layout.title].into_iter().chain(layout.boxes) {
                assert!(rect.x >= 0 && rect.y >= 0 && rect.w > 0 && rect.h > 0);
                assert!(rect.x + rect.w <= w && rect.y + rect.h <= h);
            }
        }
        assert_eq!(
            wol_verify_layout(1600, 1200, geom::shell_scale(1600, 1200)).dialog,
            RectPx::new(182, 242, 902, 652),
        );
    }

    #[test]
    fn wol_verify_follows_the_executed_0x130_table() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let rect = |value: &serde_json::Value| {
            let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
            RectPx::new(a[0], a[1], a[2], a[3])
        };
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x130"]["sizes"].as_object().unwrap() {
            let (w, h) = size.split_once('x').unwrap();
            let layout =
                wol_verify_layout(w.parse().unwrap(), h.parse().unwrap(), geom::RETAIL_SCALE);
            let native_dialog = rect(&case["dialog"]);
            assert_eq!(
                (layout.dialog.w, layout.dialog.h),
                (native_dialog.w, native_dialog.h),
                "{size}"
            );
            for (id, child) in case["controls"].as_object().unwrap() {
                let id = u16::from_str_radix(id.trim_start_matches("0x"), 16).unwrap();
                let actual = match crate::ui::wol_verify::BOXES.iter().position(|b| *b == id) {
                    Some(node) => layout.boxes[node],
                    None if id == 0x5B2 => layout.title,
                    None => panic!("{id:#x}"),
                };
                assert_eq!(
                    local(actual, layout.dialog),
                    local(rect(&child["screen_rect"]), native_dialog),
                    "{size} {id:#x}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 27);
    }
}
