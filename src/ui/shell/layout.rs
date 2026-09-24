//! Shell dialog layout pass.
//!
//! Implements contract C7: convert each control's resource rect from DLUs to
//! pixels once, then re-anchor it by its per-control `AnchorRule` for include-set
//! dialogs. Modal-centered dialogs skip the re-anchor (the caller centers them).
//! Render-agnostic; consumes only the descriptor + shared geometry primitives.

use super::descriptor::{AnchorRule, ControlKind, DialogDescriptor, RepositionPolicy};
use super::geom::{self, RectPx, RightPanelRects};

/// Centering base width — the logical shell is authored at 800x600 and
/// horizontally compensated on wider screens. Matches the per-shell helpers.
const SHELL_BASE_W: i32 = 800;
/// Centering base height — the in-game Options dialog is authored at 800x600 and
/// its ordinary controls take the centered vertical offset above this.
const SHELL_BASE_H: i32 = 600;
/// Active in-game Options (`0xBBB`) owner-draw button right-edge inset:
/// `x = screen_w - 147`. A literal pixel inset the native child-resize helper
/// applies, not a struct field or canvas-size read.
const IN_GAME_OPTIONS_BUTTON_RIGHT_INSET: i32 = 147;

/// One control's resolved pixel rect, keyed by its resource id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaidOutControl {
    pub id: u16,
    pub rect: RectPx,
}

/// Run the shell layout pass over a dialog descriptor (contract C7). Returns one
/// resolved pixel rect per control, in descriptor order.
pub fn layout_pass(desc: &DialogDescriptor, screen_w: i32, screen_h: i32) -> Vec<LaidOutControl> {
    let panel = geom::right_panel_rects(screen_w, screen_h);
    desc.controls
        .iter()
        .map(|c| {
            let rect = match desc.reposition_policy {
                RepositionPolicy::IncludeSetReanchor => {
                    apply_anchor(c.anchor, c.dlu_rect, screen_w, screen_h, panel)
                }
                // Modal-centered dialogs keep their DLU-derived client rect; the
                // caller positions the modal panel (no fullscreen re-anchor).
                RepositionPolicy::ModalCentered => {
                    geom::dlu_rect(c.dlu_rect.x, c.dlu_rect.y, c.dlu_rect.w, c.dlu_rect.h)
                }
                // Active in-game Options (`0xBBB`). Ordinary controls
                // (trackbars/checkboxes/statics) take signed screen-centered offsets; owner-draw
                // buttons additionally pin to the right edge at the SIDEBTTN canvas
                // with a sidebar-anchored row Y. That button anchoring needs the
                // runtime SIDEBTTN size + in-game sidebar geometry (above this
                // layer), so the production overlay resolves the full layout via
                // `layout_pass_in_game_options`; this bare `layout_pass` applies
                // only the centered offset every child shares.
                RepositionPolicy::InGameOptions => {
                    let (dx, dy) = in_game_options_centered_offset(screen_w, screen_h);
                    geom::dlu_rect(c.dlu_rect.x, c.dlu_rect.y, c.dlu_rect.w, c.dlu_rect.h)
                        .translate(dx, dy)
                }
            };
            LaidOutControl { id: c.id, rect }
        })
        .collect()
}

/// Active-only centered offset for the in-game Options (`0xBBB`) ordinary
/// controls: `((screen-base)/2).max(0)` per axis (0 at the 800x600 base,
/// +112/+84 at 1024x768). Owner-draw buttons do NOT take this — they right-edge
/// anchor instead.
fn in_game_options_centered_offset(screen_w: i32, screen_h: i32) -> (i32, i32) {
    ((screen_w - SHELL_BASE_W) / 2, (screen_h - SHELL_BASE_H) / 2)
}

/// App-supplied anchoring inputs for the active in-game Options (`0xBBB`) overlay
/// that `ui/shell` cannot compute itself. The native child-resize helper anchors
/// the owner-draw buttons to the in-game SIDEBAR geometry (a 25-px row stack) and
/// sizes them to the loaded SIDEBTTN canvas; both live above this layer, so the
/// render/app layer fills these in and `layout_pass_in_game_options` consumes
/// them — keeping `ui/shell` render-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InGameOptionsAnchor {
    /// SIDEBTTN SHP canvas width (read from the loaded SHP header; 125 px).
    pub button_canvas_w: i32,
    /// SIDEBTTN SHP canvas height (read from the loaded SHP header; 25 px).
    pub button_canvas_h: i32,
    /// Top Y of the upper button stack: Sound at +0, Keyboard at +25. Bound to
    /// the in-game sidebar's button-column anchor (KD-4 — verify at the gate).
    pub button_stack_top_y: i32,
    /// Top Y of the bottom-anchored Back button row (sidebar's bottom anchor).
    pub back_button_y: i32,
}

/// Resolve the active in-game Options (`0xBBB`) layout with the app-supplied
/// `anchor`. Ordinary controls (trackbars/checkboxes/statics) take the
/// screen-centered offset; owner-draw buttons (Back/Keyboard/Sound) render at the
/// SIDEBTTN canvas size, right-edge anchored at `screen_w - 147`, with a 25-px
/// row-stack Y bound to the in-game sidebar. `!visible` controls are still laid
/// out in descriptor order (the emitter skips them); their rect is harmless.
pub fn layout_pass_in_game_options(
    desc: &DialogDescriptor,
    screen_w: i32,
    screen_h: i32,
    anchor: InGameOptionsAnchor,
) -> Vec<LaidOutControl> {
    use super::in_game_options::control;
    let (dx, dy) = in_game_options_centered_offset(screen_w, screen_h);
    desc.controls
        .iter()
        .map(|c| {
            let rect = if c.kind == ControlKind::Button {
                let y = match c.id {
                    control::BACK => anchor.back_button_y,
                    control::KEYBOARD => anchor.button_stack_top_y + anchor.button_canvas_h,
                    // Sound tops the stack; any other button shares its anchor.
                    _ => anchor.button_stack_top_y,
                };
                RectPx::new(
                    screen_w - IN_GAME_OPTIONS_BUTTON_RIGHT_INSET,
                    y,
                    anchor.button_canvas_w,
                    anchor.button_canvas_h,
                )
            } else if c.id == control::TITLE {
                // Active branch60B1D0 and title finalizer60B950: no launcher nudge.
                RectPx::new(screen_w - 165, 2, 162, 16)
            } else if c.id == control::FOOTER {
                // Active60B550 footer branch: fixed10px left,1px above bottom.
                let raw = geom::dlu_rect(c.dlu_rect.x, c.dlu_rect.y, c.dlu_rect.w, c.dlu_rect.h);
                RectPx::new(10, screen_h - raw.h - 1, raw.w, raw.h)
            } else {
                // 60B7A0 shifts even smaller screens, then clamps final positions.
                let raw = geom::dlu_rect(c.dlu_rect.x, c.dlu_rect.y, c.dlu_rect.w, c.dlu_rect.h);
                RectPx::new((raw.x + dx).max(0), (raw.y + dy).max(0), raw.w, raw.h)
            };
            LaidOutControl { id: c.id, rect }
        })
        .collect()
}

/// Resolve one include-set control outside a descriptor table, with the same
/// per-control rule `layout_pass` applies.
pub fn anchor_rect(rule: AnchorRule, dlu: RectPx, screen_w: i32, screen_h: i32) -> RectPx {
    apply_anchor(
        rule,
        dlu,
        screen_w,
        screen_h,
        geom::right_panel_rects(screen_w, screen_h),
    )
}

/// Resolve one control's pixel rect by re-anchor rule. `dlu` is the raw DLU
/// resource rect; the snapped owner-draw rule consumes only `dlu.y`, while the
/// bottom-row rule is derived entirely from the resolved panel geometry.
fn apply_anchor(
    rule: AnchorRule,
    dlu: RectPx,
    screen_w: i32,
    screen_h: i32,
    panel: RightPanelRects,
) -> RectPx {
    match rule {
        AnchorRule::OwnerDrawButtonSnap { cell_w } => {
            geom::snap_button_round_half_up(dlu.y, panel, cell_w)
        }
        AnchorRule::OwnerDrawButtonBottomRow { cell_w } => {
            let y = panel.bottom.y - geom::RIGHT_PANEL_TILE_H;
            let x = panel.top.x + (geom::RIGHT_PANEL_WIDTH - cell_w);
            RectPx::new(x, y, cell_w, geom::SDBTNANM_CELL_H)
        }
        AnchorRule::RightAnchor => right_anchor(
            screen_w,
            screen_h,
            geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h),
        ),
        AnchorRule::RightAnchorRuntimeAdjust {
            resource_dw,
            resource_dh,
            dy,
            dh,
        } => {
            let resource = geom::dlu_rect(dlu.x, dlu.y, dlu.w, dlu.h);
            let adjusted = RectPx::new(
                resource.x,
                resource.y,
                resource.w + resource_dw,
                resource.h + resource_dh,
            );
            let a = right_anchor(screen_w, screen_h, adjusted);
            RectPx::new(a.x, a.y + dy, a.w, a.h + dh)
        }
    }
}

/// Right-panel static anchor (`0x0060B1D0`, no network session): inset
/// `(168 - w) / 2` from the right edge, less the horizontal half of the
/// screen beyond 800; `y` is the resource `y` plus the vertical half beyond
/// 600. Both halves clamp at 0. `rect` is the converted client rect.
fn right_anchor(screen_w: i32, screen_h: i32, rect: RectPx) -> RectPx {
    let inset = (geom::RIGHT_PANEL_WIDTH - rect.w) / 2;
    let delta_x = geom::center_offset(screen_w, SHELL_BASE_W);
    let delta_y = geom::center_offset(screen_h, SHELL_BASE_H);
    RectPx::new(
        screen_w - inset - rect.w - delta_x,
        rect.y + delta_y,
        rect.w,
        rect.h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::descriptor::{
        BgKind, ControlDescriptor, ControlKind, DialogDescriptor, DialogId,
    };
    use crate::ui::shell::geom::SDBTNANM_CELL_W_NARROW;
    use crate::ui::shell::in_game_options::{build_in_game_options_descriptor, control};

    fn ctrl(id: u16, kind: ControlKind, dlu: RectPx, anchor: AnchorRule) -> ControlDescriptor {
        ControlDescriptor {
            id,
            kind,
            dlu_rect: dlu,
            anchor,
            csf_key: None,
            tooltip_key: None,
            group: 0,
            enabled: true,
            visible: true,
        }
    }

    fn rect_for(laid: &[LaidOutControl], id: u16) -> RectPx {
        laid.iter().find(|c| c.id == id).expect("control id").rect
    }

    /// All four anchor rules reproduce the 0xE2 helper outputs at 800x600.
    #[test]
    fn anchor_rules_reproduce_main_menu_rects_800x600() {
        let desc = DialogDescriptor {
            id: DialogId(0x00E2),
            bg_kind: BgKind::RightPanelShell,
            slide_eligible: true,
            reposition_policy: RepositionPolicy::IncludeSetReanchor,
            controls: vec![
                ctrl(
                    0x0683,
                    ControlKind::Button,
                    RectPx::new(425, 125, 108, 23),
                    AnchorRule::OwnerDrawButtonSnap {
                        cell_w: SDBTNANM_CELL_W_NARROW,
                    },
                ),
                ctrl(
                    0x03EE,
                    ControlKind::Button,
                    RectPx::new(425, 330, 108, 23),
                    AnchorRule::OwnerDrawButtonBottomRow {
                        cell_w: SDBTNANM_CELL_W_NARROW,
                    },
                ),
                ctrl(
                    0x0694,
                    ControlKind::Static,
                    RectPx::new(425, 1, 108, 10),
                    AnchorRule::RightAnchorRuntimeAdjust {
                        resource_dw: 1,
                        resource_dh: 1,
                        dy: 7,
                        dh: 1,
                    },
                ),
                ctrl(
                    0x071C,
                    ControlKind::Static,
                    RectPx::new(447, 29, 61, 33),
                    crate::ui::shell::descriptor::MONITOR_ANCHOR,
                ),
            ],
        };
        let laid = layout_pass(&desc, 800, 600);
        // Snap button: flush-right 156-cell, first row at y=199.
        assert_eq!(rect_for(&laid, 0x0683), RectPx::new(644, 199, 156, 42));
        // Exit: same column, in the final tile immediately above the bottom cap.
        assert_eq!(rect_for(&laid, 0x03EE), RectPx::new(644, 535, 156, 42));
        // Title: compatibility +1w/+1h, right-anchor (635,2,163,17),
        // then the title finalizer adds +7y/+1h.
        assert_eq!(rect_for(&laid, 0x0694), RectPx::new(635, 9, 163, 18));
        // 0x71C monitor window: +1w/+1h, x = 800 - 37 - 93.
        assert_eq!(rect_for(&laid, 0x071C), RectPx::new(670, 47, 93, 55));
    }

    #[test]
    fn right_panel_statics_take_the_vertical_half_beyond_600() {
        // 0x0060B1D0: y = resource y + max(0, (H - 600) / 2), independent of
        // the panel art, which only moves from height 768.
        for (screen_h, heading_y, monitor_y) in [(480, 9, 47), (700, 59, 97), (720, 69, 107)] {
            let heading = anchor_rect(
                crate::ui::shell::descriptor::HEADING_ANCHOR,
                RectPx::new(425, 1, 108, 10),
                800,
                screen_h,
            );
            let monitor = anchor_rect(
                crate::ui::shell::descriptor::MONITOR_ANCHOR,
                RectPx::new(447, 29, 61, 33),
                800,
                screen_h,
            );
            assert_eq!(heading, RectPx::new(635, heading_y, 163, 18), "{screen_h}");
            assert_eq!(monitor, RectPx::new(670, monitor_y, 93, 55), "{screen_h}");
        }
    }

    #[test]
    fn production_main_menu_title_uses_exact_runtime_rect_at_required_resolutions() {
        for (screen_w, screen_h, expected) in [
            (640, 480, RectPx::new(475, 9, 163, 18)),
            (800, 600, RectPx::new(635, 9, 163, 18)),
            (1024, 768, RectPx::new(747, 93, 163, 18)),
        ] {
            let actual = crate::ui::main_menu_shell::compute_layout(screen_w, screen_h).title;
            assert_eq!(actual, expected, "{screen_w}x{screen_h}");
        }
    }

    /// Modal-centered dialogs are NOT re-anchored — they keep their DLU->pixel
    /// client rect (contract C7 include-set gating; 0x120/0xCE are excluded).
    #[test]
    fn modal_centered_policy_skips_reanchor() {
        let desc = DialogDescriptor {
            id: DialogId(0x0120),
            bg_kind: BgKind::ModalShp,
            slide_eligible: false,
            reposition_policy: RepositionPolicy::ModalCentered,
            controls: vec![ctrl(
                0x0001,
                ControlKind::Button,
                RectPx::new(425, 125, 108, 23),
                // Anchor rule is ignored under ModalCentered.
                AnchorRule::OwnerDrawButtonSnap {
                    cell_w: SDBTNANM_CELL_W_NARROW,
                },
            )],
        };
        let laid = layout_pass(&desc, 800, 600);
        // Plain DLU->pixel: (425,125,108,23) -> (638,203,162,37). No snap/anchor.
        assert_eq!(rect_for(&laid, 0x0001), geom::dlu_rect(425, 125, 108, 23));
        assert_eq!(rect_for(&laid, 0x0001), RectPx::new(638, 203, 162, 37));
    }

    /// Re-anchor is oversized-screen aware: 1024x768 reproduces the 0xE2 cells.
    #[test]
    fn snap_and_bottom_row_track_supported_screen_sizes() {
        let desc = DialogDescriptor {
            id: DialogId(0x00E2),
            bg_kind: BgKind::RightPanelShell,
            slide_eligible: true,
            reposition_policy: RepositionPolicy::IncludeSetReanchor,
            controls: vec![
                ctrl(
                    0x0683,
                    ControlKind::Button,
                    RectPx::new(425, 125, 108, 23),
                    AnchorRule::OwnerDrawButtonSnap {
                        cell_w: SDBTNANM_CELL_W_NARROW,
                    },
                ),
                ctrl(
                    0x03EE,
                    ControlKind::Button,
                    RectPx::new(425, 330, 108, 23),
                    AnchorRule::OwnerDrawButtonBottomRow {
                        cell_w: SDBTNANM_CELL_W_NARROW,
                    },
                ),
            ],
        };
        let at_640 = layout_pass(&desc, 640, 480);
        assert_eq!(rect_for(&at_640, 0x03EE), RectPx::new(484, 409, 156, 42));

        let at_1024 = layout_pass(&desc, 1024, 768);
        assert_eq!(rect_for(&at_1024, 0x0683), RectPx::new(756, 283, 156, 42));
        assert_eq!(rect_for(&at_1024, 0x03EE), RectPx::new(756, 619, 156, 42));
    }

    /// Test anchor with deterministic sidebar-derived button Y values so the
    /// owner-draw button placement is fixed regardless of the runtime sidebar.
    fn options_test_anchor() -> InGameOptionsAnchor {
        InGameOptionsAnchor {
            button_canvas_w: 125,
            button_canvas_h: 25,
            button_stack_top_y: 200,
            back_button_y: 540,
        }
    }

    /// 5a-ii: ordinary in-game Options controls take the screen-centered offset —
    /// zero at the 800x600 base (== raw DLU), +112/+84 at 1024x768. (Replaces the
    /// superseded 5a-i screen-invariant raw-DLU baseline.)
    #[test]
    fn in_game_options_ordinary_controls_centered_offset() {
        let desc = build_in_game_options_descriptor();
        let at800 = layout_pass_in_game_options(&desc, 800, 600, options_test_anchor());
        // GameSpeed trackbar 0x529 (144,100,128,13): no shift at the base size.
        assert_eq!(
            rect_for(&at800, control::GAME_SPEED),
            geom::dlu_rect(144, 100, 128, 13)
        );
        assert_eq!(
            rect_for(&at800, control::GAME_SPEED),
            RectPx::new(216, 163, 192, 21)
        );
        let at1024 = layout_pass_in_game_options(&desc, 1024, 768, options_test_anchor());
        let base = geom::dlu_rect(144, 100, 128, 13);
        assert_eq!(
            rect_for(&at1024, control::GAME_SPEED),
            RectPx::new(base.x + 112, base.y + 84, base.w, base.h)
        );
    }

    #[test]
    fn in_game_options_ordinary_children_match_original_procedure() {
        let native: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/in_game_shell_geometry.json"
        ))
        .unwrap();
        let cases = native["child_layout"].as_array().unwrap();
        assert_eq!(cases.len(), 9);
        let desc = build_in_game_options_descriptor();
        for case in cases {
            let integer = |key: &str| case[key].as_i64().unwrap() as i32;
            let id = integer("control_id") as u16;
            let expected: Vec<i32> = case["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let laid = layout_pass_in_game_options(
                &desc,
                integer("width"),
                integer("height"),
                options_test_anchor(),
            );
            let actual = rect_for(&laid, id);
            assert_eq!(
                [actual.x, actual.y, actual.w, actual.h].as_slice(),
                expected,
                "{case}"
            );
        }
    }

    /// 5a-ii: owner-draw buttons render at the SIDEBTTN 125x25 canvas, right-edge
    /// anchored at `screen_w - 147` (NOT their DLU rect), with the sidebar-derived
    /// 25-px row stack (Sound top, Keyboard +25, Back bottom-anchored).
    #[test]
    fn in_game_options_buttons_right_edge_sidebttn_size() {
        let desc = build_in_game_options_descriptor();
        let at800 = layout_pass_in_game_options(&desc, 800, 600, options_test_anchor());
        let back = rect_for(&at800, control::BACK);
        assert_eq!((back.x, back.y, back.w, back.h), (800 - 147, 540, 125, 25));
        let sound = rect_for(&at800, control::SOUND);
        assert_eq!((sound.x, sound.y), (800 - 147, 200));
        let keyboard = rect_for(&at800, control::KEYBOARD);
        assert_eq!((keyboard.x, keyboard.y), (800 - 147, 225));
        // Buttons track the right edge on a wider screen; the centered ordinary
        // offset never moves them.
        let at1024 = layout_pass_in_game_options(&desc, 1024, 768, options_test_anchor());
        assert_eq!(rect_for(&at1024, control::BACK).x, 1024 - 147);
    }
}
