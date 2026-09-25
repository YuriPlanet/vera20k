//! Front-end modal substrate: `ModalKind` table + native result conventions.
//!
//! Render-agnostic data describing the YR front-end pop-up modals as plain Rust:
//! which RT_DIALOG template each `ModalKind` loads, the native "count-rule" the
//! generic CSF message-box helper uses to pick a template from its populated
//! string slots, and the TWO distinct result conventions the modals resolve under
//! (the message-box helper vs the in-game Options own dialog proc). Depends only on
//! the shared shell descriptor/geometry types (no sim/render/assets), so it honors
//! the ui/ layering rule. The modal render emitter and the quit-confirm/validation
//! wiring in later slices consume this; nothing reads it yet (zero behavior change).

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
    #[cfg(test)]
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
    if includes_cancel(kind) {
        if let Some(rect) = rects.cancel {
            controls.push(modal_control(control::CANCEL, ControlKind::Button, rect));
        }
    }
    if includes_third(kind) {
        if let Some(rect) = rects.third {
            controls.push(modal_control(control::THIRD, ControlKind::Button, rect));
        }
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

/// Pixel size of the message-box modal panel. The 0xCE/0x120 templates declare a
/// 300x200 DLU dialog, which at the 8pt MS Sans Serif DLU factor (x*6/4, y*13/8)
/// is 450x325 px; PUDLGBGN frame 0 fills this box.
pub const MESSAGE_BOX_W: i32 = 450;
pub const MESSAGE_BOX_H: i32 = 325;

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

/// Screen-relative pixel rects for a one-button message box: `0xCE`, and
/// `0xD0` (the Westwood Online `TXT_APIMISSING` box), which shares its
/// geometry: body static `0x5B0` (40,40,220,50) and OK (207,175,83,15).
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
        body: modal_child(dialog, geom::dlu_rect(40, 40, 220, 50)),
        ok: modal_child(dialog, geom::dlu_rect(207, 175, 83, 15)),
    }
}

/// Center the modal panel on the live screen, then place each control at its
/// dialog-relative DLU rect (the native modal-centered policy: no fullscreen
/// re-anchor).
pub fn quit_confirm_layout(screen_w: i32, screen_h: i32) -> QuitConfirmLayout {
    let dialog = centered_modal_dialog(screen_w, screen_h, MESSAGE_BOX_W, MESSAGE_BOX_H);
    QuitConfirmLayout {
        dialog,
        body: modal_child(dialog, geom::dlu_rect(40, 40, 220, 50)),
        ok: modal_child(dialog, geom::dlu_rect(207, 155, 83, 15)),
        cancel: modal_child(dialog, geom::dlu_rect(207, 175, 83, 15)),
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
            RectPx::new(175, 138, MESSAGE_BOX_W, MESSAGE_BOX_H)
        );
        // Controls at the dialog origin + their template DLU rects.
        let child = |dx, dy, dw, dh| {
            let local = geom::dlu_rect(dx, dy, dw, dh);
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
    fn quit_confirm_layout_clamps_on_tiny_screen() {
        // A screen smaller than the panel clamps the origin to >= 0 (no negative rect).
        let l = quit_confirm_layout(320, 240);
        assert_eq!(l.dialog, RectPx::new(0, 0, MESSAGE_BOX_W, MESSAGE_BOX_H));
    }
}
