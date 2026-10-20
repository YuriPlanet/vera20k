//! Active Game Controls dialog (BBB), reached through pause menu state5.
//!
//! Original 4E1D00 selects BBB while A8E9A0==1 and installs 4E1FE0. This
//! render-agnostic descriptor carries all17 controls from the plain DLGTEMPLATE
//! at VA0x00C01B18 (RVA0x801B18), freshly decoded from the original image.
//! VisualDetails and its two labels are hidden/disabled. GameSpeed/ScrollRate
//! labels start with GUI:Faster and change only after a changed-position scroll
//! notification. Runtime input and persistence belong to in_game_options_state.

use super::descriptor::{
    AnchorRule, BgKind, ControlDescriptor, ControlKind, DialogDescriptor, DialogId,
    RepositionPolicy,
};
use super::geom::RectPx;

/// Resource control ids for the active-game Options dialog (`0xBBB`).
pub mod control {
    /// Back button -> own-proc result 1 (close + persist), unconditional.
    pub const BACK: u16 = 0x0686;
    /// Keyboard button -> sub-dialog; STUB until a later 5a step.
    pub const KEYBOARD: u16 = 0x052C;
    /// Sound button -> sub-dialog; STUB until a later 5a step. Runtime-conditional
    /// enable (the proc may disable it when no sound device is present).
    pub const SOUND: u16 = 0x052D;
    /// Game Speed trackbar (range 0..6; value inverted `6 - pos` at apply time).
    pub const GAME_SPEED: u16 = 0x0529;
    /// Scroll Rate trackbar (range 0..6; value inverted `6 - pos` at apply time).
    pub const SCROLL_RATE: u16 = 0x052A;
    /// Visual Details trackbar (range 0..2; direct value). Ships DISABLED in the
    /// active-game template (see `build_in_game_options_descriptor`).
    pub const VISUAL_DETAILS: u16 = 0x052B;
    /// Target Lines checkbox -> Options UnitActionLines.
    pub const TARGET_LINES: u16 = 0x0601;
    /// Show Hidden checkbox -> Options ShowHidden.
    pub const SHOW_HIDDEN: u16 = 0x0604;
    /// Tooltips checkbox -> Options ToolTips.
    pub const TOOLTIPS: u16 = 0x0602;

    // Text statics (render-only). Their template title strings ARE the CSF
    // caption keys; rects verified from the `0xBBB` DLGTEMPLATE at `0x00C01B18`.
    /// Dialog title static; `GUI:GameOptions`, SS_CENTER. Same control id/rect as
    /// the skirmish right-panel title `0x694` (rendered H-centered, top-anchored).
    pub const TITLE: u16 = 0x0694;
    /// GameSpeed caption static; `GUI:GameSpeed`, SS_RIGHT.
    pub const GAME_SPEED_CAPTION: u16 = 0x0714;
    /// ScrollRate caption static; `GUI:ScrollRate`, SS_RIGHT.
    pub const SCROLL_RATE_CAPTION: u16 = 0x0715;
    /// VisualDetails caption static; `GUI:VisualDetails`, SS_RIGHT. Hidden in
    /// `0xBBB` (WS_DISABLED, no WS_VISIBLE) — carried for fidelity, never emitted.
    pub const VISUAL_DETAILS_CAPTION: u16 = 0x0716;
    /// GameSpeed value label; template default `GUI:Faster`, SS_LEFT. The
    /// slider-driven swap (TXT_SLOWEST..TXT_FASTEST) lands in 5a-iii.
    pub const GAME_SPEED_VALUE: u16 = 0x0671;
    /// ScrollRate value label; template default `GUI:Faster`, SS_LEFT (5a-iii swaps).
    pub const SCROLL_RATE_VALUE: u16 = 0x0672;
    /// VisualDetails value label; `GUI:HigherDetail`, SS_LEFT. Hidden in `0xBBB` —
    /// never emitted.
    pub const VISUAL_DETAILS_VALUE: u16 = 0x0673;
    /// Blank footer / status-line static; `GUI:Blank` (resolves empty), SS_LEFT.
    pub const FOOTER: u16 = 0x0695;
}

/// GameSpeed/ScrollRate value-label CSF keys, indexed by SLIDER POSITION (0..6).
/// Verbatim from the gamemd CSF pointer tables (slider pos 0 = "slowest" end).
pub const SPEED_LABEL_KEYS: [&str; 7] = [
    "TXT_SLOWEST", // pos 0
    "TXT_SLOWER",  // pos 1
    "TXT_SLOW",    // pos 2
    "TXT_MEDIUM",  // pos 3
    "TXT_FAST",    // pos 4
    "TXT_FASTER",  // pos 5
    "TXT_FASTEST", // pos 6
];

/// CSF key for a speed value-label at a slider position. Active 4E1FE0's
/// TBM_SETRANGE/TBM_SETPOS (GameSpeed 4E207F..4E20B8, ScrollRate
/// 4E2128..4E2159) change the trackbar from its 0..100 default, whose tail
/// 61E609..61E6AF sends WM_HSCROLL(5, min+pos) to the dialog; 4E2278..4E232B
/// then sets the sender's label from this table, so the open shows it too.
pub fn speed_value_label_key(slider_pos: u32) -> &'static str {
    SPEED_LABEL_KEYS[(slider_pos as usize).min(SPEED_LABEL_KEYS.len() - 1)]
}

/// Whether a control shows in this open. Active 4E1FE0's init (message
/// 0x497) hides GameSpeed, its caption and its label (529/714/671) in
/// GameMode 0 without `-SPEEDCONTROL` (4E2036..4E2079, flag 0xA8EDDC) and
/// whenever Session+0x300 is set (4E20BA..4E211A).
pub fn control_shown(control: &ControlDescriptor, game_speed_hidden: bool) -> bool {
    control.visible
        && !(game_speed_hidden
            && matches!(
                control.id,
                control::GAME_SPEED | control::GAME_SPEED_CAPTION | control::GAME_SPEED_VALUE
            ))
}

/// RT_DIALOG resource id of the active-game Options dialog.
const DIALOG_0BBB: u16 = 0x0BBB;

/// Build the render-agnostic descriptor for the ACTIVE in-game Options dialog
/// (`0xBBB`), including resource captions and hidden controls. The shared
/// active-game layout positions its chrome and owner-draw buttons.
///
/// `enabled` carries the **resource-template default** (per `ControlDescriptor`):
/// VisualDetails (`0x52B`) is created disabled in the active-game template and
/// the Options proc's populate path sets its range/position but never enables it,
/// so it is stored `enabled: false`. The Sound button is template-enabled here
/// (its runtime no-device disable layers over this in the controller, 5a-iii).
pub fn build_in_game_options_descriptor() -> DialogDescriptor {
    DialogDescriptor {
        id: DialogId(DIALOG_0BBB),
        controls: vec![
            options_control(
                control::BACK,
                ControlKind::Button,
                RectPx::new(425, 346, 108, 23),
                true,
                true,
            ),
            options_control(
                control::KEYBOARD,
                ControlKind::Button,
                RectPx::new(425, 149, 108, 23),
                true,
                true,
            ),
            options_control(
                control::SOUND,
                ControlKind::Button,
                RectPx::new(425, 122, 108, 23),
                true,
                true,
            ),
            options_control(
                control::GAME_SPEED,
                ControlKind::Trackbar,
                RectPx::new(144, 100, 128, 13),
                true,
                true,
            ),
            options_control(
                control::SCROLL_RATE,
                ControlKind::Trackbar,
                RectPx::new(144, 131, 128, 13),
                true,
                true,
            ),
            // Hidden + disabled in the active-game template (WS_DISABLED, no
            // WS_VISIBLE); the populate path never shows or enables it.
            options_control(
                control::VISUAL_DETAILS,
                ControlKind::Trackbar,
                RectPx::new(144, 162, 128, 13),
                false,
                false,
            ),
            options_control(
                control::TARGET_LINES,
                ControlKind::Checkbox,
                RectPx::new(89, 206, 119, 10),
                true,
                true,
            ),
            options_control(
                control::SHOW_HIDDEN,
                ControlKind::Checkbox,
                RectPx::new(89, 224, 119, 10),
                true,
                true,
            ),
            options_control(
                control::TOOLTIPS,
                ControlKind::Checkbox,
                RectPx::new(214, 206, 127, 10),
                true,
                true,
            ),
            // Text statics (render-only). Rects + CSF titles verbatim from the
            // `0xBBB` DLGTEMPLATE at `0x00C01B18` (`read_memory 0x00C01B18`). The
            // two VisualDetails statics (`0x716`/`0x673`) are WS_DISABLED with no
            // WS_VISIBLE in the template, so `visible: false` (emitter skips them).
            options_static(
                control::TITLE,
                RectPx::new(425, 1, 108, 10),
                "GUI:GameOptions",
                true,
            ),
            options_static(
                control::GAME_SPEED_CAPTION,
                RectPx::new(61, 99, 78, 15),
                "GUI:GameSpeed",
                true,
            ),
            options_static(
                control::SCROLL_RATE_CAPTION,
                RectPx::new(61, 130, 78, 15),
                "GUI:ScrollRate",
                true,
            ),
            options_static(
                control::VISUAL_DETAILS_CAPTION,
                RectPx::new(61, 161, 78, 15),
                "GUI:VisualDetails",
                false,
            ),
            options_static(
                control::GAME_SPEED_VALUE,
                RectPx::new(278, 99, 92, 15),
                "GUI:Faster",
                true,
            ),
            options_static(
                control::SCROLL_RATE_VALUE,
                RectPx::new(278, 130, 92, 15),
                "GUI:Faster",
                true,
            ),
            options_static(
                control::VISUAL_DETAILS_VALUE,
                RectPx::new(278, 161, 92, 15),
                "GUI:HigherDetail",
                false,
            ),
            options_static(
                control::FOOTER,
                RectPx::new(2, 355, 303, 12),
                "GUI:Blank",
                true,
            ),
        ],
        bg_kind: BgKind::InGameOptions,
        slide_eligible: false,
        reposition_policy: RepositionPolicy::InGameOptions,
    }
}

/// One Options control descriptor. The `anchor` field is unused under
/// `RepositionPolicy::InGameOptions` (the native child-resize helpers key off
/// control id/kind, not a per-control anchor enum), so a benign value is stored;
/// CSF captions come directly from the original resource. `enabled`
/// records the resource-template default (runtime enable/disable layers over it
/// in the controller).
fn options_control(
    id: u16,
    kind: ControlKind,
    dlu_rect: RectPx,
    enabled: bool,
    visible: bool,
) -> ControlDescriptor {
    ControlDescriptor {
        id,
        kind,
        dlu_rect,
        anchor: AnchorRule::RightAnchor,
        // Original plain DLGTEMPLATE at VA0x00C01B18, all17 records.
        csf_key: match id {
            control::BACK => Some("GUI:Back"),
            control::KEYBOARD => Some("GUI:Keyboard"),
            control::SOUND => Some("GUI:Sound"),
            control::TARGET_LINES => Some("GUI:TargetLines"),
            control::SHOW_HIDDEN => Some("GUI:ShowHidden"),
            control::TOOLTIPS => Some("GUI:Tooltips"),
            _ => None,
        },
        tooltip_key: None,
        group: 0,
        enabled,
        visible,
    }
}

/// One Options text-static descriptor. `csf_key` is the control's template title
/// string (which IS its CSF caption key); the emitter resolves it to display text
/// and paints it as BitFont glyphs. `anchor` is unused under
/// `RepositionPolicy::InGameOptions` (statics take the centered offset like other
/// ordinary controls). Statics are always template-enabled; `visible` carries the
/// template `WS_VISIBLE` bit (the emitter skips `!visible` statics).
fn options_static(
    id: u16,
    dlu_rect: RectPx,
    csf_key: &'static str,
    visible: bool,
) -> ControlDescriptor {
    ControlDescriptor {
        id,
        kind: ControlKind::Static,
        dlu_rect,
        anchor: AnchorRule::RightAnchor,
        csf_key: Some(csf_key),
        tooltip_key: None,
        group: 0,
        enabled: true,
        visible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::geom;
    use crate::ui::shell::layout::layout_pass;

    fn control_ids(d: &DialogDescriptor) -> Vec<u16> {
        d.controls.iter().map(|c| c.id).collect()
    }
    fn kind_of(d: &DialogDescriptor, id: u16) -> ControlKind {
        d.controls
            .iter()
            .find(|c| c.id == id)
            .expect("control id")
            .kind
    }

    #[test]
    fn descriptor_carries_all_seventeen_0bbb_controls() {
        let d = build_in_game_options_descriptor();
        assert_eq!(d.id, DialogId(0x0BBB));
        assert_eq!(d.bg_kind, BgKind::InGameOptions);
        assert_eq!(d.reposition_policy, RepositionPolicy::InGameOptions);
        assert!(!d.slide_eligible);
        let ids = control_ids(&d);
        // 9 interactive (3 buttons + 3 trackbars + 3 checkboxes) + 8 statics.
        assert_eq!(ids.len(), 17);
        for id in [
            control::BACK,
            control::KEYBOARD,
            control::SOUND,
            control::GAME_SPEED,
            control::SCROLL_RATE,
            control::VISUAL_DETAILS,
            control::TARGET_LINES,
            control::SHOW_HIDDEN,
            control::TOOLTIPS,
            control::TITLE,
            control::GAME_SPEED_CAPTION,
            control::SCROLL_RATE_CAPTION,
            control::VISUAL_DETAILS_CAPTION,
            control::GAME_SPEED_VALUE,
            control::SCROLL_RATE_VALUE,
            control::VISUAL_DETAILS_VALUE,
            control::FOOTER,
        ] {
            assert!(ids.contains(&id), "missing control {id:#06x}");
        }
    }

    #[test]
    fn control_kinds_match_template() {
        let d = build_in_game_options_descriptor();
        assert_eq!(kind_of(&d, control::BACK), ControlKind::Button);
        assert_eq!(kind_of(&d, control::KEYBOARD), ControlKind::Button);
        assert_eq!(kind_of(&d, control::SOUND), ControlKind::Button);
        assert_eq!(kind_of(&d, control::GAME_SPEED), ControlKind::Trackbar);
        assert_eq!(kind_of(&d, control::SCROLL_RATE), ControlKind::Trackbar);
        assert_eq!(kind_of(&d, control::VISUAL_DETAILS), ControlKind::Trackbar);
        assert_eq!(kind_of(&d, control::TARGET_LINES), ControlKind::Checkbox);
        assert_eq!(kind_of(&d, control::SHOW_HIDDEN), ControlKind::Checkbox);
        assert_eq!(kind_of(&d, control::TOOLTIPS), ControlKind::Checkbox);
        for id in [
            control::TITLE,
            control::GAME_SPEED_CAPTION,
            control::SCROLL_RATE_CAPTION,
            control::VISUAL_DETAILS_CAPTION,
            control::GAME_SPEED_VALUE,
            control::SCROLL_RATE_VALUE,
            control::VISUAL_DETAILS_VALUE,
            control::FOOTER,
        ] {
            assert_eq!(kind_of(&d, id), ControlKind::Static, "static {id:#06x}");
        }
    }

    #[test]
    fn descriptor_dlu_rects_match_verified_template() {
        // Verbatim from the live 0xBBB dialog resource template. A 1-DLU drift
        // here shifts the control every time in-game Options opens.
        let d = build_in_game_options_descriptor();
        let dlu = |id: u16| d.controls.iter().find(|c| c.id == id).unwrap().dlu_rect;
        assert_eq!(dlu(control::BACK), RectPx::new(425, 346, 108, 23));
        assert_eq!(dlu(control::KEYBOARD), RectPx::new(425, 149, 108, 23));
        assert_eq!(dlu(control::SOUND), RectPx::new(425, 122, 108, 23));
        assert_eq!(dlu(control::GAME_SPEED), RectPx::new(144, 100, 128, 13));
        assert_eq!(dlu(control::SCROLL_RATE), RectPx::new(144, 131, 128, 13));
        assert_eq!(dlu(control::VISUAL_DETAILS), RectPx::new(144, 162, 128, 13));
        assert_eq!(dlu(control::TARGET_LINES), RectPx::new(89, 206, 119, 10));
        assert_eq!(dlu(control::SHOW_HIDDEN), RectPx::new(89, 224, 119, 10));
        assert_eq!(dlu(control::TOOLTIPS), RectPx::new(214, 206, 127, 10));
        // Statics (verbatim from the `0xBBB` DLGTEMPLATE at `0x00C01B18`).
        assert_eq!(dlu(control::TITLE), RectPx::new(425, 1, 108, 10));
        assert_eq!(
            dlu(control::GAME_SPEED_CAPTION),
            RectPx::new(61, 99, 78, 15)
        );
        assert_eq!(
            dlu(control::SCROLL_RATE_CAPTION),
            RectPx::new(61, 130, 78, 15)
        );
        assert_eq!(
            dlu(control::VISUAL_DETAILS_CAPTION),
            RectPx::new(61, 161, 78, 15)
        );
        assert_eq!(dlu(control::GAME_SPEED_VALUE), RectPx::new(278, 99, 92, 15));
        assert_eq!(
            dlu(control::SCROLL_RATE_VALUE),
            RectPx::new(278, 130, 92, 15)
        );
        assert_eq!(
            dlu(control::VISUAL_DETAILS_VALUE),
            RectPx::new(278, 161, 92, 15)
        );
        assert_eq!(dlu(control::FOOTER), RectPx::new(2, 355, 303, 12));
    }

    #[test]
    fn static_csf_keys_match_template() {
        // The template title string of each static IS its CSF caption key.
        let d = build_in_game_options_descriptor();
        let key = |id: u16| d.controls.iter().find(|c| c.id == id).unwrap().csf_key;
        assert_eq!(key(control::TITLE), Some("GUI:GameOptions"));
        assert_eq!(key(control::GAME_SPEED_CAPTION), Some("GUI:GameSpeed"));
        assert_eq!(key(control::SCROLL_RATE_CAPTION), Some("GUI:ScrollRate"));
        assert_eq!(
            key(control::VISUAL_DETAILS_CAPTION),
            Some("GUI:VisualDetails")
        );
        assert_eq!(key(control::GAME_SPEED_VALUE), Some("GUI:Faster"));
        assert_eq!(key(control::SCROLL_RATE_VALUE), Some("GUI:Faster"));
        assert_eq!(key(control::VISUAL_DETAILS_VALUE), Some("GUI:HigherDetail"));
        assert_eq!(key(control::FOOTER), Some("GUI:Blank"));
        assert_eq!(key(control::BACK), Some("GUI:Back"));
        assert_eq!(key(control::KEYBOARD), Some("GUI:Keyboard"));
        assert_eq!(key(control::SOUND), Some("GUI:Sound"));
        assert_eq!(key(control::TARGET_LINES), Some("GUI:TargetLines"));
        assert_eq!(key(control::SHOW_HIDDEN), Some("GUI:ShowHidden"));
        assert_eq!(key(control::TOOLTIPS), Some("GUI:Tooltips"));
        // Trackbar resource titles are identifiers, not displayed CSF captions.
        assert_eq!(key(control::GAME_SPEED), None);
    }

    #[test]
    fn value_label_is_the_position_key() {
        assert_eq!(speed_value_label_key(3), "TXT_MEDIUM");
        assert_eq!(speed_value_label_key(6), "TXT_FASTEST");
        assert_eq!(speed_value_label_key(0), "TXT_SLOWEST");
    }

    #[test]
    fn enabled_state_matches_template_default() {
        // Resource-template default: VisualDetails is the only control created
        // disabled in the active-game template; the rest are template-enabled.
        let d = build_in_game_options_descriptor();
        let enabled = |id: u16| d.controls.iter().find(|c| c.id == id).unwrap().enabled;
        assert!(
            !enabled(control::VISUAL_DETAILS),
            "VisualDetails ships disabled"
        );
        for id in [
            control::BACK,
            control::KEYBOARD,
            control::SOUND,
            control::GAME_SPEED,
            control::SCROLL_RATE,
            control::TARGET_LINES,
            control::SHOW_HIDDEN,
            control::TOOLTIPS,
        ] {
            assert!(enabled(id), "control {id:#06x} should be template-enabled");
        }
    }

    #[test]
    fn visualdetails_triplet_hidden_rest_visible() {
        // gamemd hides the whole VisualDetails triplet in 0xBBB (WS_DISABLED + no
        // WS_VISIBLE; the populate path never shows them): the 0x52B trackbar plus
        // its caption 0x716 and value label 0x673. All three carry visible:false
        // so the emitter skips them.
        let d = build_in_game_options_descriptor();
        let vis = |id: u16| d.controls.iter().find(|c| c.id == id).unwrap().visible;
        for id in [
            control::VISUAL_DETAILS,
            control::VISUAL_DETAILS_CAPTION,
            control::VISUAL_DETAILS_VALUE,
        ] {
            assert!(!vis(id), "VisualDetails control {id:#06x} hidden in 0xBBB");
        }
        for id in [
            control::BACK,
            control::KEYBOARD,
            control::SOUND,
            control::GAME_SPEED,
            control::SCROLL_RATE,
            control::TARGET_LINES,
            control::SHOW_HIDDEN,
            control::TOOLTIPS,
            control::TITLE,
            control::GAME_SPEED_CAPTION,
            control::SCROLL_RATE_CAPTION,
            control::GAME_SPEED_VALUE,
            control::SCROLL_RATE_VALUE,
            control::FOOTER,
        ] {
            assert!(vis(id), "control {id:#06x} visible");
        }
    }

    #[test]
    fn baseline_layout_is_raw_dlu_to_pixel_per_control() {
        // 5a-i baseline: every control == its raw DLU->pixel rect (no anchor).
        let d = build_in_game_options_descriptor();
        let laid = layout_pass(&d, 800, 600);
        let rect_for = |id: u16| laid.iter().find(|c| c.id == id).unwrap().rect;
        for c in &d.controls {
            let expected = geom::dlu_rect(c.dlu_rect.x, c.dlu_rect.y, c.dlu_rect.w, c.dlu_rect.h);
            assert_eq!(rect_for(c.id), expected, "control {:#06x}", c.id);
        }
        // Concrete spot-checks (round-half-up DLU factor x*6/4, y*13/8).
        assert_eq!(rect_for(control::BACK), RectPx::new(638, 562, 162, 37));
        assert_eq!(
            rect_for(control::GAME_SPEED),
            RectPx::new(216, 163, 192, 21)
        );
        assert_eq!(rect_for(control::TOOLTIPS), RectPx::new(321, 335, 191, 16));
    }
}
