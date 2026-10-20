//! Front-end modal substrate: `ModalKind` table + native result conventions.
//!
//! Render-agnostic data describing the YR front-end pop-up modals as plain Rust:
//! which RT_DIALOG template each `ModalKind` loads, the native "count-rule" the
//! generic CSF message-box helper uses to pick a template from its populated
//! string slots, and the TWO distinct result conventions the modals resolve under
//! (the message-box helper vs the in-game Options own dialog proc). Depends only on
//! the shared shell descriptor/geometry types (no sim/render/assets), so it honors
//! the ui/ layering rule. The shared modal renderer consumes these descriptors.

use super::descriptor::{
    AnchorRule, BgKind, ControlDescriptor, ControlKind, DialogDescriptor, DialogId,
    RepositionPolicy,
};
use super::geom::{self, RectPx};

/// Resource control ids shared by the count-rule message-box family (the generic
/// CSF helper resolves the dialog by which of these the click matched).
pub mod control {
    /// Body-text static. Produces no click result.
    pub const BODY_STATIC: u16 = 0x05B0;
    /// OK / affirmative button -> message-box result 0.
    pub const OK: u16 = 0x05AE;
    /// Cancel button — Win32 IDCANCEL (control id 2) -> message-box result 1.
    pub const CANCEL: u16 = 0x0002;
    /// Third owner-draw button -> message-box result 2 (0x121 template only).
    pub const THIRD: u16 = 0x05AF;
    /// F0 progress-dialog Cancel. The saved-load caller leaves it hidden.
    pub const PROGRESS_CANCEL: u16 = 0x05C0;
}

/// One front-end pop-up modal, identified by the RT_DIALOG template it loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    /// 0xCE — body static + a single OK button. (Skirmish start-validation.)
    BodyOk,
    /// 0x120 — body static + OK + Cancel (control 2). (Quit-confirm.)
    Confirm,
    /// 0x121 — adds a fourth owner-draw button. Reachable but unused by the three
    /// target modals; render-UNTESTED, excluded from the in-game OK acceptance.
    ThreeButton,
    /// 0xBBB (active game) / 0xF5 (shell) — the in-game Options dialog. A SEPARATE
    /// full-shell mechanism (its own dialog proc), NOT the count-rule message box;
    /// it must not be passed to `build_message_box_descriptor`.
    InGameOptions,
}

impl ModalKind {
    /// The RT_DIALOG resource id this modal loads. Only `InGameOptions` varies with
    /// `in_active_game`: its own proc reads a byte discriminator and selects 0xBBB
    /// when that byte equals 1 (active game), else 0xF5 (shell). The compare is an
    /// equality test (`== 1`), PROOFED from the binary (no emulate gate). The
    /// message-box template ids are fixed and ignore `in_active_game`.
    pub fn template_id(self, in_active_game: bool) -> u16 {
        match self {
            ModalKind::BodyOk => 0x00CE,
            ModalKind::Confirm => 0x0120,
            ModalKind::ThreeButton => 0x0121,
            ModalKind::InGameOptions => {
                if in_active_game {
                    0x0BBB
                } else {
                    0x00F5
                }
            }
        }
    }

    /// Which native result convention this modal resolves under. Keeping the two
    /// conventions explicit prevents reading a message-box code (0/1/2) as an
    /// Options code (1/2) or vice-versa — the slice's highest-risk parity fact.
    pub fn result_convention(self) -> ResultConvention {
        match self {
            ModalKind::BodyOk | ModalKind::Confirm | ModalKind::ThreeButton => {
                ResultConvention::MessageBox
            }
            ModalKind::InGameOptions => ResultConvention::OwnProc,
        }
    }
}

/// Native count-rule template selection for the generic CSF message-box helper.
///
/// The helper takes four pre-resolved CSF string pointers (body, ok, third,
/// fourth) and picks the template purely by which OPTIONAL slots are populated.
/// "Populated" means a non-null pointer whose first character is non-null, so an
/// empty caption counts as absent (see [`slot_populated`]). body+ok are mandatory;
/// only `third`/`fourth` escalate the template:
///   - neither optional populated -> [`ModalKind::BodyOk`]      (0xCE)
///   - third populated, fourth not -> [`ModalKind::Confirm`]     (0x120)
///   - fourth populated            -> [`ModalKind::ThreeButton`] (0x121)
///
/// (The three target modals only ever populate up to `third`, so the
/// fourth-without-third ordering is not produced in practice; the cascade follows
/// the "fourth slot present -> 0x121" rule regardless.)
#[cfg(test)]
pub fn message_box_kind(third_slot: Option<&str>, fourth_slot: Option<&str>) -> ModalKind {
    if slot_populated(fourth_slot) {
        ModalKind::ThreeButton
    } else if slot_populated(third_slot) {
        ModalKind::Confirm
    } else {
        ModalKind::BodyOk
    }
}

/// A CSF slot is populated iff present with a non-null first char (the native test
/// is a non-null pointer AND `*ptr != 0`).
#[cfg(test)]
fn slot_populated(slot: Option<&str>) -> bool {
    matches!(slot, Some(s) if !s.is_empty())
}

/// The native result convention a modal resolves under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultConvention {
    /// Count-rule message-box helper: writes 0 (OK), 1 (Cancel/IDCANCEL), or 2
    /// (third button); the caller inits the result slot to -1 (dismissed/unset).
    MessageBox,
    /// In-game Options own dialog proc: writes 1 for every close button (persist),
    /// and 2 only when the game ends while the modal is open (no persist).
    OwnProc,
}

/// Result value the message-box helper leaves when no button resolved the dialog
/// (the caller inits the result slot to -1 before pumping it).
pub const MESSAGE_BOX_DISMISSED: i32 = -1;

/// A resolved modal outcome, tagged by convention so the two native result codings
/// can never be confused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalResult {
    /// Count-rule message-box family (0xCE/0x120/0x121): 0=OK, 1=Cancel/ESC,
    /// 2=third button, -1=dismissed (slot left at its init value).
    MessageBox(i32),
    /// In-game Options own proc (0xBBB/0xF5): 1=persist (any close button),
    /// 2=game-ended (no persist).
    InGameOptions(i32),
}

impl ModalResult {
    /// Map a resolved message-box control id to its native result, or `None` for a
    /// control that produces no result (the body static). OK(`0x5AE`)->0,
    /// Cancel(control 2)->1, third(`0x5AF`)->2.
    pub fn from_message_box_control(control: u16) -> Option<ModalResult> {
        let code = match control {
            control::OK => 0,
            control::CANCEL => 1,
            control::THIRD => 2,
            _ => return None,
        };
        Some(ModalResult::MessageBox(code))
    }

    /// Quit-confirm (0x120) semantics: ONLY the OK click (message-box result 0)
    /// quits; Cancel(1), third(2), and dismissed(-1) all stay. ESC resolves to
    /// IDCANCEL -> result 1 -> stay.
    #[cfg(test)]
    pub fn quit_confirm_quits(self) -> bool {
        matches!(self, ModalResult::MessageBox(0))
    }

    /// In-game Options persist semantics: result == 1 (every close button) writes
    /// the settings; result == 2 (game ended) does not. No discard-without-save
    /// path exists.
    pub fn options_persists(self) -> bool {
        matches!(self, ModalResult::InGameOptions(1))
    }
}

/// DLU rects for the message-box controls, taken from the parsed RT_DIALOG
/// template. body+ok are always present; `cancel`/`third` are present only on the
/// templates that include them (0x120 carries cancel; 0x121 adds third).
#[derive(Debug, Clone, Copy)]
pub struct MessageBoxRects {
    pub body: RectPx,
    pub ok: RectPx,
    pub cancel: Option<RectPx>,
    pub third: Option<RectPx>,
}

/// Build the render-agnostic [`DialogDescriptor`] for a count-rule message-box
/// modal. Background is mode-2 SHP (PUDLGBGN + DIALOGN), centered (no fullscreen
/// re-anchor), not slide-eligible. The control SET follows the count rule: body
/// static + OK always; Cancel (control 2) for `Confirm`/`ThreeButton`; the third
/// button (`0x5AF`) for `ThreeButton`. `InGameOptions` is NOT a message box and
/// must not be passed here (it builds a full-shell owner-draw descriptor in a
/// later slice).
pub fn build_message_box_descriptor(kind: ModalKind, rects: &MessageBoxRects) -> DialogDescriptor {
    debug_assert!(
        kind.result_convention() == ResultConvention::MessageBox,
        "build_message_box_descriptor is only for the count-rule message-box family"
    );
    let mut controls = vec![
        modal_control(control::BODY_STATIC, ControlKind::Static, rects.body),
        modal_control(control::OK, ControlKind::Button, rects.ok),
    ];
    if includes_cancel(kind)
        && let Some(rect) = rects.cancel
    {
        controls.push(modal_control(control::CANCEL, ControlKind::Button, rect));
    }
    if includes_third(kind)
        && let Some(rect) = rects.third
    {
        controls.push(modal_control(control::THIRD, ControlKind::Button, rect));
    }
    DialogDescriptor {
        // in_active_game is irrelevant for message-box ids (only InGameOptions
        // varies); the debug_assert above guarantees we never reach here for it.
        id: DialogId(kind.template_id(false)),
        controls,
        bg_kind: BgKind::ModalShp,
        slide_eligible: false,
        reposition_policy: RepositionPolicy::ModalCentered,
    }
}

/// Saved-game loading's F0 progress dialog: original 559D60 -> 623230 supplies
/// TXT_LOADING and a null Cancel caption. F0's resource keeps 5C0 hidden; this
/// ordinary saved-load presentation has no interactive controls.
/// Source: tools/storage_oracle/saved_game_layout.py and its native corpus.
pub fn build_saved_game_loading_descriptor() -> DialogDescriptor {
    let mut body = modal_control(
        control::BODY_STATIC,
        ControlKind::Static,
        RectPx::new(40, 40, 220, 50),
    );
    body.csf_key = Some("TXT_LOADING");
    let mut cancel = modal_control(
        control::PROGRESS_CANCEL,
        ControlKind::Button,
        RectPx::new(207, 175, 83, 15),
    );
    cancel.visible = false;
    DialogDescriptor {
        id: DialogId(0xF0),
        controls: vec![cancel, body],
        bg_kind: BgKind::ModalShp,
        slide_eligible: false,
        reposition_policy: RepositionPolicy::ModalCentered,
    }
}

/// Whether the kind's template includes the Cancel (control 2) button.
fn includes_cancel(kind: ModalKind) -> bool {
    matches!(kind, ModalKind::Confirm | ModalKind::ThreeButton)
}

/// Whether the kind's template includes the third owner-draw button (`0x5AF`).
fn includes_third(kind: ModalKind) -> bool {
    matches!(kind, ModalKind::ThreeButton)
}

/// One message-box control descriptor. The `anchor` rule is unused — message-box
/// modals carry [`RepositionPolicy::ModalCentered`], which the layout pass resolves
/// by a plain DLU->pixel conversion with no re-anchor — but the field is required,
/// so a benign value is stored.
fn modal_control(id: u16, kind: ControlKind, dlu_rect: RectPx) -> ControlDescriptor {
    ControlDescriptor {
        id,
        kind,
        dlu_rect,
        anchor: AnchorRule::RightAnchor,
        csf_key: None,
        tooltip_key: None,
        group: 0,
        enabled: true,
        visible: true,
    }
}

// --- Centered message-box modal layout (screen-relative pixel rects) ---

/// Pixel client size of the 300x200 DLU modal templates (F0/CE/120). The common
/// 775BC0 resize adds one to the converted extents. Executed F0 controls and
/// 777060/777080 centering: tools/storage_oracle/saved_game_layout.json.
pub const MESSAGE_BOX_W: i32 = 451;
pub const MESSAGE_BOX_H: i32 = 326;

/// Screen-relative pixel rects for the quit-confirm (0x120) modal: the centered
/// PUDLGBGN panel plus its body static and its two buttons. Rects mirror the
/// RT_DIALOG `0x120` template (`0x00C00A68`): body static `0x5B0`
/// (40,40,220,50), OK `0x5AE` (207,155,83,15) directly above Cancel control 2
/// (207,175,83,15). (OK at y 135 belongs to the three-button `0x121`, whose
/// third button `0x5AF` takes y 155.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuitConfirmLayout {
    pub dialog: RectPx,
    pub body: RectPx,
    pub ok: RectPx,
    pub cancel: RectPx,
}

/// Screen-relative pixel rects for a one-button message box: `0xCE` (the
/// skirmish validation and saved-map notices) and `0xD0` (the Westwood Online
/// `TXT_APIMISSING` box) share this geometry: body static `0x5B0`
/// (40,40,220,50) and OK (207,175,83,15). F0's hidden Cancel shares that lower
/// control rect and its visible body uses the same projection. The box centres
/// on the live screen, not inside the 800x600 shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyOkLayout {
    pub dialog: RectPx,
    pub body: RectPx,
    pub ok: RectPx,
}

pub fn body_ok_layout(screen_w: i32, screen_h: i32) -> BodyOkLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    BodyOkLayout {
        dialog,
        body: modal_child(dialog, geom::child_window(40, 40, 220, 50)),
        ok: modal_child(dialog, geom::child_window(207, 175, 83, 15)),
    }
}

/// Center the modal panel on the live screen, then place each control at its
/// dialog-relative DLU rect (the native modal-centered policy: no fullscreen
/// re-anchor).
pub fn quit_confirm_layout(screen_w: i32, screen_h: i32) -> QuitConfirmLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    QuitConfirmLayout {
        dialog,
        body: modal_child(dialog, geom::child_window(40, 40, 220, 50)),
        ok: modal_child(dialog, geom::child_window(207, 155, 83, 15)),
        cancel: modal_child(dialog, geom::child_window(207, 175, 83, 15)),
    }
}

/// The Westwood Online Yes/No box `0x126` (`0x005E2CD0` style 4): its
/// template places body `0x5B0` (40,40,220,50), Yes 6 (207,155,83,15) and
/// No 7 (207,175,83,15), the rects of `0x120`'s body, OK and Cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YesNoLayout {
    pub dialog: RectPx,
    pub body: RectPx,
    pub yes: RectPx,
    pub no: RectPx,
}

pub fn yes_no_layout(screen_w: i32, screen_h: i32) -> YesNoLayout {
    let shared = quit_confirm_layout(screen_w, screen_h);
    YesNoLayout {
        dialog: shared.dialog,
        body: shared.body,
        yes: shared.ok,
        no: shared.cancel,
    }
}

/// The Westwood Online wait box `0xC4` (300x200 DLU, compact popup centred
/// like the message boxes): Disconnect `0x58D` (109,105,83,15), the
/// centred text `0x5B2` (51,14,197,54), WOL Check Status `0x58E`
/// (109,130,83,15) and the animation static `0x7A9` (68,163,165,20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitBoxLayout {
    pub dialog: RectPx,
    pub text: RectPx,
    pub disconnect: RectPx,
    pub check_status: RectPx,
    pub animation: RectPx,
}

pub fn wait_box_layout(screen_w: i32, screen_h: i32) -> WaitBoxLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    WaitBoxLayout {
        dialog,
        text: modal_child(dialog, geom::child_window(51, 14, 197, 54)),
        disconnect: modal_child(dialog, geom::child_window(109, 105, 83, 15)),
        check_status: modal_child(dialog, geom::child_window(109, 130, 83, 15)),
        animation: modal_child(dialog, geom::child_window(68, 163, 165, 20)),
    }
}

/// The Westwood Online Create Game popup `0xC3` (`0x0078DB70`), the same
/// 451x326 centred box: executed client rects (tools/storage_oracle/
/// wol_shell.json) of its controls from their templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolCreateGameLayout {
    pub dialog: RectPx,
    /// Static `GUI:Password`, right aligned.
    pub password_label: RectPx,
    /// Edit `0x72C` after its NewEdit inset.
    pub password: RectPx,
    pub clan: RectPx,
    pub limit: RectPx,
    pub observe: RectPx,
    pub ok: RectPx,
    pub cancel: RectPx,
    /// Its own status line `0x695`.
    pub status: RectPx,
}

/// A NewEdit's window after `0x00614CCF`'s +1/-2 inset.
fn new_edit_window(rect: RectPx) -> RectPx {
    RectPx::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2)
}

pub fn wol_create_game_layout(screen_w: i32, screen_h: i32) -> WolCreateGameLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    WolCreateGameLayout {
        dialog,
        password_label: child(11, 48, 113, 12),
        password: new_edit_window(child(133, 47, 140, 12)),
        clan: child(34, 76, 234, 14),
        limit: child(34, 93, 234, 14),
        observe: child(34, 110, 234, 14),
        ok: child(33, 128, 83, 15),
        cancel: child(184, 128, 83, 15),
        status: child(0, 186, 291, 12),
    }
}

/// The channel password popup `0xE1` (`0x0078E030`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolChannelPasswordLayout {
    pub dialog: RectPx,
    /// Static `GUI:ChannelPassword`, centred.
    pub label: RectPx,
    pub password: RectPx,
    pub ok: RectPx,
    pub cancel: RectPx,
}

pub fn wol_channel_password_layout(screen_w: i32, screen_h: i32) -> WolChannelPasswordLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    WolChannelPasswordLayout {
        dialog,
        label: child(49, 76, 200, 12),
        password: new_edit_window(child(48, 94, 202, 12)),
        ok: child(49, 163, 83, 15),
        cancel: child(157, 163, 83, 15),
    }
}

/// The WOL page dialog `0xCF` (proc `0x0078D120`), the same 451x326
/// centred box: executed client rects (tools/storage_oracle/wol_shell.json).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolPageLayout {
    pub dialog: RectPx,
    /// Static `GUI:PageBuddy`, right aligned.
    pub name_label: RectPx,
    /// Edit `0x5C1` after its NewEdit inset.
    pub name: RectPx,
    /// Static `GUI:TextToSend`.
    pub text_label: RectPx,
    /// Edit `0x540` after its NewEdit inset.
    pub text: RectPx,
    /// Owner-draw list `0x5B5` (`LBS_NOSEL`).
    pub transcript: RectPx,
    /// Static `GUI:FindStatus` and the status `0x5C8`.
    pub status_label: RectPx,
    pub status: RectPx,
    /// `0x5C2` (`GUI:Page`) and Cancel.
    pub send: RectPx,
    pub cancel: RectPx,
}

pub fn wol_page_layout(screen_w: i32, screen_h: i32) -> WolPageLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    WolPageLayout {
        dialog,
        name_label: child(20, 9, 54, 11),
        name: new_edit_window(child(79, 9, 80, 12)),
        text_label: child(20, 26, 137, 11),
        text: new_edit_window(child(16, 39, 268, 12)),
        transcript: child(16, 63, 268, 78),
        status_label: child(183, 9, 49, 10),
        status: child(183, 22, 98, 10),
        send: child(45, 164, 83, 15),
        cancel: child(163, 163, 83, 15),
    }
}

/// The page-received dialog `0x11E` (proc `0x0079DB50`), the same box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolPageReceivedLayout {
    pub dialog: RectPx,
    /// Static `GUI:PageReceived`, centred.
    pub title: RectPx,
    /// List `0x53F` (selectable).
    pub transcript: RectPx,
    /// Multiline edit `0x540` after its NewEdit inset.
    pub reply: RectPx,
    /// Static `GUI:ReplyTo`, right aligned, and the name `0x718`.
    pub reply_to_label: RectPx,
    pub reply_to: RectPx,
    /// `0x5C2` (`GUI:ReplyPage`) and `0x686` (`GUI:Back`).
    pub send: RectPx,
    pub back: RectPx,
}

pub fn wol_page_received_layout(screen_w: i32, screen_h: i32) -> WolPageReceivedLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    WolPageReceivedLayout {
        dialog,
        title: child(7, 4, 277, 11),
        transcript: child(8, 22, 281, 79),
        reply: new_edit_window(child(8, 118, 283, 14)),
        reply_to_label: child(25, 146, 105, 11),
        reply_to: child(142, 147, 139, 8),
        send: child(42, 166, 83, 15),
        back: child(164, 166, 83, 15),
    }
}

/// Add Buddy `0x115` (proc `0x0079DFE0`), the same 451x326 centred box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WolAddBuddyLayout {
    pub dialog: RectPx,
    /// Static `GUI:BuddyNickname`, centred.
    pub label: RectPx,
    /// Edit `0x540` after its NewEdit inset.
    pub name: RectPx,
    /// `0x702` (`GUI:AddBuddy`, created disabled) and `0x686` (`GUI:Back`).
    pub add: RectPx,
    pub back: RectPx,
    /// The status line `0x695` along the box's foot.
    pub status: RectPx,
}

pub fn wol_add_buddy_layout(screen_w: i32, screen_h: i32) -> WolAddBuddyLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    let child = |x, y, w, h| modal_child(dialog, geom::child_window(x, y, w, h));
    WolAddBuddyLayout {
        dialog,
        label: child(57, 61, 193, 15),
        name: new_edit_window(child(77, 85, 152, 14)),
        add: child(65, 118, 83, 15),
        back: child(159, 118, 83, 15),
        status: child(0, 186, 273, 12),
    }
}

/// Center a modal panel of `(w, h)` on the live screen. Byte-identical to the
/// skirmish validation-modal centering (`((screen - size) + 1) / 2`, clamped >= 0).
fn centered_modal_dialog(screen_w: i32, screen_h: i32, w: i32, h: i32) -> RectPx {
    RectPx::new(
        (((screen_w - w) + 1) / 2).max(0),
        (((screen_h - h) + 1) / 2).max(0),
        w,
        h,
    )
}

/// Place a control at the dialog origin plus its DLU-derived local rect.
fn modal_child(dialog: RectPx, local: RectPx) -> RectPx {
    RectPx::new(dialog.x + local.x, dialog.y + local.y, local.w, local.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control_ids(d: &DialogDescriptor) -> Vec<u16> {
        d.controls.iter().map(|c| c.id).collect()
    }

    fn sample_rects() -> MessageBoxRects {
        MessageBoxRects {
            body: RectPx::new(40, 40, 220, 50),
            ok: RectPx::new(207, 175, 83, 15),
            cancel: Some(RectPx::new(10, 175, 83, 15)),
            third: Some(RectPx::new(110, 175, 83, 15)),
        }
    }

    #[test]
    fn template_id_table() {
        assert_eq!(ModalKind::BodyOk.template_id(false), 0x00CE);
        assert_eq!(ModalKind::BodyOk.template_id(true), 0x00CE);
        assert_eq!(ModalKind::Confirm.template_id(false), 0x0120);
        assert_eq!(ModalKind::ThreeButton.template_id(false), 0x0121);
        // Only InGameOptions varies with the active-game discriminator (== 1).
        assert_eq!(ModalKind::InGameOptions.template_id(true), 0x0BBB);
        assert_eq!(ModalKind::InGameOptions.template_id(false), 0x00F5);
    }

    #[test]
    fn count_rule_selects_template_by_populated_optional_slots() {
        // The three reachable rows of the native count-rule table.
        assert_eq!(message_box_kind(None, None), ModalKind::BodyOk);
        assert_eq!(message_box_kind(Some("Cancel"), None), ModalKind::Confirm);
        assert_eq!(
            message_box_kind(Some("Cancel"), Some("Maybe")),
            ModalKind::ThreeButton
        );
    }

    #[test]
    fn count_rule_treats_empty_string_as_absent() {
        // Native populated-test is a non-null pointer AND a non-null first char, so
        // an empty caption counts as an unpopulated slot.
        assert_eq!(message_box_kind(Some(""), None), ModalKind::BodyOk);
        assert_eq!(
            message_box_kind(Some("Cancel"), Some("")),
            ModalKind::Confirm
        );
        assert_eq!(message_box_kind(Some(""), Some("")), ModalKind::BodyOk);
    }

    #[test]
    fn result_convention_split() {
        assert_eq!(
            ModalKind::BodyOk.result_convention(),
            ResultConvention::MessageBox
        );
        assert_eq!(
            ModalKind::Confirm.result_convention(),
            ResultConvention::MessageBox
        );
        assert_eq!(
            ModalKind::ThreeButton.result_convention(),
            ResultConvention::MessageBox
        );
        assert_eq!(
            ModalKind::InGameOptions.result_convention(),
            ResultConvention::OwnProc
        );
    }

    #[test]
    fn message_box_control_result_mapping() {
        // OK 0x5AE -> 0, Cancel control 2 -> 1, third 0x5AF -> 2, body -> none.
        assert_eq!(
            ModalResult::from_message_box_control(control::OK),
            Some(ModalResult::MessageBox(0))
        );
        assert_eq!(
            ModalResult::from_message_box_control(control::CANCEL),
            Some(ModalResult::MessageBox(1))
        );
        assert_eq!(
            ModalResult::from_message_box_control(control::THIRD),
            Some(ModalResult::MessageBox(2))
        );
        assert_eq!(
            ModalResult::from_message_box_control(control::BODY_STATIC),
            None
        );
    }

    #[test]
    fn quit_confirm_only_quits_on_ok() {
        assert!(ModalResult::MessageBox(0).quit_confirm_quits());
        assert!(!ModalResult::MessageBox(1).quit_confirm_quits()); // Cancel / ESC
        assert!(!ModalResult::MessageBox(2).quit_confirm_quits());
        assert!(!ModalResult::MessageBox(MESSAGE_BOX_DISMISSED).quit_confirm_quits());
    }

    #[test]
    fn options_persists_only_on_result_one() {
        assert!(ModalResult::InGameOptions(1).options_persists());
        assert!(!ModalResult::InGameOptions(2).options_persists());
    }

    #[test]
    fn message_box_descriptor_control_sets_match_count_rule() {
        let r = sample_rects();

        let body_ok = build_message_box_descriptor(ModalKind::BodyOk, &r);
        assert_eq!(
            control_ids(&body_ok),
            vec![control::BODY_STATIC, control::OK]
        );
        assert_eq!(body_ok.id, DialogId(0x00CE));
        assert_eq!(body_ok.bg_kind, BgKind::ModalShp);
        assert_eq!(body_ok.reposition_policy, RepositionPolicy::ModalCentered);
        assert!(!body_ok.slide_eligible);

        let confirm = build_message_box_descriptor(ModalKind::Confirm, &r);
        assert_eq!(
            control_ids(&confirm),
            vec![control::BODY_STATIC, control::OK, control::CANCEL]
        );
        assert_eq!(confirm.id, DialogId(0x0120));

        let three = build_message_box_descriptor(ModalKind::ThreeButton, &r);
        assert_eq!(
            control_ids(&three),
            vec![
                control::BODY_STATIC,
                control::OK,
                control::CANCEL,
                control::THIRD
            ]
        );
        assert_eq!(three.id, DialogId(0x0121));
    }

    #[test]
    fn body_ok_descriptor_never_adds_cancel_or_third() {
        // 0xCE must never carry control 2 / 0x5AF even when those rects are present.
        let r = sample_rects();
        let d = build_message_box_descriptor(ModalKind::BodyOk, &r);
        assert!(!control_ids(&d).contains(&control::CANCEL));
        assert!(!control_ids(&d).contains(&control::THIRD));
    }

    #[test]
    fn quit_confirm_layout_centers_panel_and_places_controls() {
        let l = quit_confirm_layout(800, 600);
        // Panel centered on the live screen (round-half-up, clamped).
        assert_eq!(
            l.dialog,
            RectPx::new(175, 137, MESSAGE_BOX_W, MESSAGE_BOX_H)
        );
        // Controls at the dialog origin + their template DLU rects.
        let child = |dx, dy, dw, dh| {
            let local = geom::child_window(dx, dy, dw, dh);
            RectPx::new(l.dialog.x + local.x, l.dialog.y + local.y, local.w, local.h)
        };
        assert_eq!(l.body, child(40, 40, 220, 50));
        assert_eq!(l.ok, child(207, 155, 83, 15));
        assert_eq!(l.cancel, child(207, 175, 83, 15));
        // OK sits directly above Cancel (DLU y=155 vs 175), both right-aligned at
        // DLU x=207, as in the 0x120 template and the retail capture.
        assert!(l.ok.y < l.cancel.y);
        assert_eq!(l.ok.x, l.cancel.x);
    }

    #[test]
    fn modal_resource_and_shared_projection_match_original_saved_progress() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/saved_game_layout.json",
        ))
        .unwrap();
        let rect = |value: &serde_json::Value| {
            let values: [i32; 4] = serde_json::from_value(value.clone()).unwrap();
            RectPx::new(values[0], values[1], values[2], values[3])
        };
        let descriptor = build_saved_game_loading_descriptor();
        let resource = &corpus["progress_resource"];
        assert_eq!(
            descriptor.id.0 as u64,
            resource["dialog_id"].as_u64().unwrap()
        );
        for (control, native) in descriptor
            .controls
            .iter()
            .zip(resource["controls"].as_array().unwrap())
        {
            assert_eq!(control.id as u64, native["control_id"].as_u64().unwrap());
            assert_eq!(control.dlu_rect, rect(&native["dlu_rect"]));
            assert_eq!(
                control.visible,
                native["style"].as_u64().unwrap() & 0x1000_0000 != 0
            );
        }
        for row in corpus["progress_geometry"].as_array().unwrap() {
            let width = row["input"]["width"].as_i64().unwrap() as i32;
            let height = row["input"]["height"].as_i64().unwrap() as i32;
            let projected = body_ok_layout(width, height);
            let quit = quit_confirm_layout(width, height);
            assert_eq!(projected.dialog, rect(&row["dialog_rect"]));
            assert_eq!(quit.dialog, projected.dialog);
            let children = crate::ui::shell::layout::layout_pass(&descriptor, width, height);
            for native in row["children"].as_array().unwrap() {
                let id = native["id"].as_u64().unwrap() as u16;
                let expected = rect(&native["rect"]);
                let child = children.iter().find(|child| child.id == id).unwrap();
                assert_eq!(
                    child.rect.translate(projected.dialog.x, projected.dialog.y),
                    expected
                );
                match id {
                    control::BODY_STATIC => {
                        assert_eq!(projected.body, expected);
                        assert_eq!(quit.body, expected);
                    }
                    control::PROGRESS_CANCEL => {
                        assert_eq!(projected.ok, expected);
                        assert_eq!(quit.cancel, expected);
                    }
                    _ => panic!("unexpected F0 child"),
                }
            }
        }
    }

    #[test]
    fn quit_confirm_layout_clamps_on_tiny_screen() {
        // A screen smaller than the panel clamps the origin to >= 0 (no negative rect).
        let l = quit_confirm_layout(320, 240);
        assert_eq!(l.dialog, RectPx::new(0, 0, MESSAGE_BOX_W, MESSAGE_BOX_H));
    }

    #[test]
    fn wol_wait_box_matches_the_template_arithmetic_at_800x600() {
        // RT_DIALOG 0xC4 through the shared 300x200 DLU modal policy (the
        // relayout tool's MulDiv; the popup was not executed).
        let wait = wait_box_layout(800, 600);
        assert_eq!(wait.dialog, RectPx::new(175, 137, 451, 326));
        assert_eq!(wait.disconnect, RectPx::new(339, 308, 126, 25));
        assert_eq!(wait.text, RectPx::new(252, 160, 297, 89));
        assert_eq!(wait.check_status, RectPx::new(339, 348, 126, 25));
        assert_eq!(wait.animation, RectPx::new(277, 402, 249, 34));
        let yes_no = yes_no_layout(800, 600);
        let quit = quit_confirm_layout(800, 600);
        assert_eq!((yes_no.yes, yes_no.no), (quit.ok, quit.cancel));
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
                let create = wol_create_game_layout(w, h);
                let password = wol_channel_password_layout(w, h);
                assert_eq!(create.dialog, rect(&case["dialog"]), "{key} {size}");
                for (id, child) in case["controls"].as_object().unwrap() {
                    let mut native = rect(&child["screen_rect"]);
                    let actual = match (key, id.as_str()) {
                        ("0xC3", "0xFFFF") => create.password_label,
                        ("0xC3", "0x72C") => create.password,
                        ("0xC3", "0x66C") => create.clan,
                        ("0xC3", "0x71E") => create.limit,
                        ("0xC3", "0x71F") => create.observe,
                        ("0xC3", "0x1") => create.ok,
                        ("0xC3", "0x2") => create.cancel,
                        ("0xC3", "0x695") => create.status,
                        (_, "0xFFFF") => password.label,
                        (_, "0x72C") => password.password,
                        (_, "0x1") => password.ok,
                        (_, "0x2") => password.cancel,
                        other => panic!("{other:?}"),
                    };
                    if id == "0x72C" {
                        native = new_edit_window(native);
                    }
                    assert_eq!(actual, native, "{key} {size} {id}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 36);
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
                let page = wol_page_layout(w, h);
                let received = wol_page_received_layout(w, h);
                assert_eq!(page.dialog, rect(&case["dialog"]), "{key} {size}");
                assert_eq!(received.dialog, page.dialog);
                for (id, child) in case["controls"].as_object().unwrap() {
                    let mut native = rect(&child["screen_rect"]);
                    let actual = match (key, id.as_str()) {
                        ("0xCF", "0x540") => page.text,
                        ("0xCF", "0x5C1") => page.name,
                        ("0xCF", "0x5B5") => page.transcript,
                        ("0xCF", "0x5C2") => page.send,
                        ("0xCF", "0x2") => page.cancel,
                        ("0xCF", "0x5C8") => page.status,
                        ("0xCF", "0xFFFF") => page.text_label,
                        ("0xCF", "0xFFFF#1") => page.name_label,
                        ("0xCF", "0xFFFF#2") => page.status_label,
                        (_, "0x53F") => received.transcript,
                        (_, "0x540") => received.reply,
                        (_, "0x5C2") => received.send,
                        (_, "0x686") => received.back,
                        (_, "0x718") => received.reply_to,
                        (_, "0xFFFF") => received.title,
                        (_, "0xFFFF#1") => received.reply_to_label,
                        other => panic!("{other:?}"),
                    };
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
            assert_eq!(layout.dialog, rect(&case["dialog"]), "{size}");
            for (id, child) in case["controls"].as_object().unwrap() {
                let mut native = rect(&child["screen_rect"]);
                let actual = match id.as_str() {
                    "0x540" => layout.name,
                    "0x686" => layout.back,
                    "0x695" => layout.status,
                    "0x702" => layout.add,
                    "0xFFFF" => layout.label,
                    other => panic!("{other}"),
                };
                if child["class"] == "Edit" {
                    native = new_edit_window(native);
                }
                assert_eq!(actual, native, "{size} {id}");
                checked += 1;
            }
        }
        assert_eq!(checked, 15);
    }
}
