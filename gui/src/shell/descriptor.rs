//! Front-end shell dialog/control descriptors.
//!
//! Render-agnostic data describing a Win32-native shell dialog (Framework B) as
//! plain Rust: the dialog's controls, their raw DLU rects, and the per-control
//! re-anchor rule the layout pass applies (contract C7). Depends only on the
//! shared geometry primitives (no sim/render/assets), so it honors the ui/
//! layering rule. The wider controller/paint/modal/slide substrate consumes
//! these (see docs/plans/2026-05-31-shell-substrate-design.md §5/§6); this slice
//! drives the main-menu (0xE2) layout off a descriptor table feeding `layout_pass`.

use super::geom::RectPx;

/// Win32 dialog resource id (e.g. `0x00E2` main menu, `0x0100` single player,
/// `0x0102` skirmish). A newtype so dialog ids never mix with control ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogId(pub u16);

/// Owner-draw control class (subclass classification, study §2.2 / contract C6).
/// Only the kinds the migrated shells use today are exercised; the remaining
/// variants are placeholders the skirmish controls (Slice 4) will fill in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Button,
    Static,
    Checkbox,
    Radio,
    Combo,
    Listbox,
    Trackbar,
    Edit,
    ScrollBar,
}

/// Per-control re-anchor rule applied by `layout_pass` after the one-shot
/// DLU->pixel conversion (contract C7). Each variant is a faithful port of a
/// current main-menu (0xE2) helper; later shells extend this as they migrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorRule {
    /// Owner-draw button snapped round-half-up to the nearest 42-px SDBTNANM
    /// row, flush to the panel right edge at `cell_w`. Uses only the DLU top.
    /// (0xE2 stacked Single Player / WW Online / Network / Movies / Options.)
    OwnerDrawButtonSnap { cell_w: i32 },
    /// Owner-draw button occupying the final complete tile immediately above the
    /// right-panel bottom cap, flush-right at `cell_w`. The resource top is not
    /// consumed. (0xE2 Exit.)
    OwnerDrawButtonBottomRow { cell_w: i32 },
    /// Right-panel static (`0x0060B1D0`): sidebar inset and the half of the
    /// screen beyond 800x600 on each axis (heading and `0x71C` monitor of 0xE2
    /// and the menu pages 0x100/0x101/0x129).
    RightAnchor,
    /// Apply a runtime size correction to the converted resource rect before
    /// right-anchoring it, then apply the final `(dy, dh)` adjustment.
    /// (0xE2 0x694 heading: +1w/+1h before anchor, then +7y/+1h.) `inset`
    /// is the control's inset override (record `+0xDC`, set by
    /// `0x0060AB71..0x0060AD16`), which replaces `(168 - w) / 2`.
    RightAnchorRuntimeAdjust {
        resource_dw: i32,
        resource_dh: i32,
        inset: Option<i32>,
        dy: i32,
        dh: i32,
    },
    /// A body control kept at its plain resource conversion ([`geom::dlu_rect`]).
    ///
    /// [`geom::dlu_rect`]: super::geom::dlu_rect
    Resource,
    /// A body control at the common resized window ([`geom::child_window`]).
    ///
    /// [`geom::child_window`]: super::geom::child_window
    ChildWindow,
    /// Owner-draw button snapped to a right-panel tile by the biased
    /// truncation of `0x100`/`0x102` ([`geom::snap_button_biased_truncate`]).
    ///
    /// [`geom::snap_button_biased_truncate`]: super::geom::snap_button_biased_truncate
    ButtonColumn { cell_w: i32 },
    /// Owner-draw button on the column's final tile, flush to the centred
    /// shell's right edge (Back of `0x100`/`0x102` and their family). The
    /// resource rect is not consumed.
    ButtonColumnLast { cell_w: i32 },
    /// Status line `0x695` outside a suspended game
    /// ([`layout::status_line_rect`]).
    ///
    /// [`layout::status_line_rect`]: super::layout::status_line_rect
    StatusLine,
}

/// The family pages' stacked owner-draw buttons (`0x100`/`0x102`).
pub const BUTTON: AnchorRule = AnchorRule::ButtonColumn {
    cell_w: super::geom::SDBTNANM_CELL_W_NARROW,
};

/// The family pages' Back button on the column's final tile.
pub const BACK_BUTTON: AnchorRule = AnchorRule::ButtonColumnLast {
    cell_w: super::geom::SDBTNANM_CELL_W_NARROW,
};

/// Right-panel heading static `0x694`: the runtime window is one pixel wider
/// and taller than the resource conversion (the compatibility correction the
/// 0xE2 heading already carried), then the fix-up pass adds `+7` y / `+1` h
/// for the right-panel shells (`0x0060BD17`).
pub const HEADING_ANCHOR: AnchorRule = AnchorRule::RightAnchorRuntimeAdjust {
    resource_dw: 1,
    resource_dh: 1,
    inset: None,
    dy: 7,
    dh: 1,
};

/// Heading `0x694` of `0xBC 0xBD 0x102 0xC2 0xC9 0xBC6 0x105 0x6B 0x113`:
/// the same window, which the fix-up pass moves only `+1` y (`0x0060BCE4`).
pub const LOW_HEADING_ANCHOR: AnchorRule = AnchorRule::RightAnchorRuntimeAdjust {
    resource_dw: 1,
    resource_dh: 1,
    inset: None,
    dy: 1,
    dh: 0,
};

/// Heading `0x694` of the WOL pages whose template is 108x11 DLU (`0x10E`,
/// `0x10F` and their sub-pages): the family heading's place, the window one
/// row taller and without the family's `+1` h fix-up.
pub const TALL_HEADING_ANCHOR: AnchorRule = AnchorRule::RightAnchorRuntimeAdjust {
    resource_dw: 1,
    resource_dh: 1,
    inset: None,
    dy: 7,
    dh: 0,
};

/// Game type `0x6EC` and map name `0x5A8` of `0xBC 0xBD 0xC2 0xC9 0x102
/// 0xBC6`: the one-pixel window correction and the inset override 14
/// (`0x0060ACC2..0x0060AD04`).
pub const MAP_TEXT_ANCHOR: AnchorRule = AnchorRule::RightAnchorRuntimeAdjust {
    resource_dw: 1,
    resource_dh: 1,
    inset: Some(14),
    dy: 0,
    dh: 0,
};

/// Monitor static `0x71C`: the same one-pixel window correction as the
/// heading and its inset override 37, set in the family dialogs that carry it
/// (`0x0060AB71..0x0060ACA5`): x 670 at 800x600 with the 93x55 window, and the
/// 92x53 SDWRNANM frame centers at `(670, 48)`.
pub const MONITOR_ANCHOR: AnchorRule = AnchorRule::RightAnchorRuntimeAdjust {
    resource_dw: 1,
    resource_dh: 1,
    inset: Some(37),
    dy: 0,
    dh: 0,
};

/// A right-panel child with the one-pixel window correction and no fix-up:
/// the WOL lobby's heading `0x694` (y 7 at 800x600, not the `+1` of
/// [`LOW_HEADING_ANCHOR`]) and its lobby list.
pub const PANEL_CHILD_ANCHOR: AnchorRule = AnchorRule::RightAnchorRuntimeAdjust {
    resource_dw: 1,
    resource_dh: 1,
    inset: None,
    dy: 0,
    dh: 0,
};

impl AnchorRule {
    /// The rule a layout table names (the shell's `shell_layout.ini`).
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "Resource" => Self::Resource,
            "ChildWindow" => Self::ChildWindow,
            "StatusLine" => Self::StatusLine,
            "RightAnchor" => Self::RightAnchor,
            "Button" => BUTTON,
            "BackButton" => BACK_BUTTON,
            "Heading" => HEADING_ANCHOR,
            "LowHeading" => LOW_HEADING_ANCHOR,
            "TallHeading" => TALL_HEADING_ANCHOR,
            "PanelChild" => PANEL_CHILD_ANCHOR,
            "MapText" => MAP_TEXT_ANCHOR,
            "Monitor" => MONITOR_ANCHOR,
            _ => return None,
        })
    }
}

/// Background composition mode (study §C8): mode-1 right-panel shells vs mode-2
/// SHP modal. Only `RightPanelShell` is exercised this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgKind {
    /// Right-panel chrome + MNSCRN background (front-end shells 0xE2/0x100/0x102).
    RightPanelShell,
    /// PUDLGBGN.SHP modal panel + DIALOGN.PAL (roadmap; Slice 5).
    ModalShp,
}

/// Reposition policy (contract C7 include-set gating). Include-set dialogs
/// (0xE2/0x6B/0x100/0x102) expand the parent fullscreen and re-anchor
/// allow-listed children; modal-centered dialogs (0xCE/0x120) do NOT re-anchor
/// (study §3 DRIFT-CORRECTED: those ids are excluded from the reposition pass).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositionPolicy {
    IncludeSetReanchor,
    ModalCentered,
}

/// One control inside a shell dialog template.
#[derive(Debug, Clone)]
pub struct ControlDescriptor {
    /// Win32 control resource id (e.g. `0x0683` Single Player).
    pub id: u16,
    pub kind: ControlKind,
    /// Raw resource rect in dialog DLUs (pre-conversion). For the owner-draw
    /// button anchors only the DLU top is consumed; the x/w/h carry the template
    /// client rect the SDBTNANM cell replaces.
    pub dlu_rect: RectPx,
    pub anchor: AnchorRule,
    /// CSF label key for the control's caption (`None` for art-only controls).
    pub csf_key: Option<&'static str>,
    /// CSF key for the hover tooltip/status line (`None` if the control has none).
    pub tooltip_key: Option<&'static str>,
    /// Slide/paint group marker (study +0xD5). `0` = ungrouped.
    pub group: u8,
    /// Resource-template default. Runtime enable/disable (e.g. the single-player
    /// LoadSavedGame guard) layers over this in the controller, not here.
    pub enabled: bool,
    /// Resource-template `WS_VISIBLE`. A `false` control is created hidden and the
    /// dialog proc never shows it, so it is not emitted or hit-tested. (Active
    /// `0xBBB` hides the VisualDetails trackbar + its caption/value-label.)
    pub visible: bool,
}

impl ControlDescriptor {
    /// A visible, enabled, ungrouped control without caption or status help.
    pub const fn new(id: u16, kind: ControlKind, dlu_rect: RectPx, anchor: AnchorRule) -> Self {
        Self {
            id,
            kind,
            dlu_rect,
            anchor,
            csf_key: None,
            tooltip_key: None,
            group: 0,
            enabled: true,
            visible: true,
        }
    }

    /// The same control with caption `csf_key` and status help `tooltip_key`.
    pub const fn labelled(self, csf_key: &'static str, tooltip_key: &'static str) -> Self {
        Self {
            csf_key: Some(csf_key),
            tooltip_key: Some(tooltip_key),
            ..self
        }
    }
}

/// A full shell dialog: its controls plus the dialog-level composition, slide,
/// and reposition policy.
#[derive(Debug, Clone)]
pub struct DialogDescriptor {
    pub id: DialogId,
    pub controls: Vec<ControlDescriptor>,
    pub bg_kind: BgKind,
    pub slide_eligible: bool,
    pub reposition_policy: RepositionPolicy,
}
