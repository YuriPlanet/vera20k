//! Front-end shell dialog ids and the per-control re-anchor rules the layout
//! pass applies (contract C7).

/// Win32 dialog resource id (e.g. `0x00E2` main menu, `0x0100` single player,
/// `0x0102` skirmish). A newtype so dialog ids never mix with control ids, nor
/// with the GUI crate's stack handle `vera20k_gui::DialogId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogResourceId(pub u16);

/// Per-control re-anchor rule applied after the one-shot
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
