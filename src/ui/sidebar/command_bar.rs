//! Bottom command-bar layout and command identities.
//!
//! Original layout `72FC60`, draw `6D0A20`, and UIMD ButtonList reader
//! `674650`. Assets own dimensions; the bar tiles at integer native pixels.
//! TabClass::One_Time `6D0260` (a jump to SidebarClass's) and GetTooltipText
//! `6D1800` only forward to SidebarClass and add no TabClass behavior.

use super::Rect;
use crate::rules::ini_parser::IniSection;

pub const COMMAND_NAMES: [&str; 11] = [
    "Team01",
    "Team02",
    "Team03",
    "TypeSelect",
    "Deploy",
    "AttackMove",
    "Guard",
    "Beacon",
    "Stop",
    "PlanningMode",
    "Cheer",
];

/// UIMD.INI maps command identities to slots, rather than numbering artwork
/// by slot. `674650` reads `ButtonList` into 0x200 bytes and gives each
/// `strtok(",")` token the next slot; the name search `6CFCC0` is an exact
/// `strcmp`, and an unknown name leaves its slot unused.
pub fn read_button_list(section: &IniSection) -> Vec<Option<usize>> {
    let mut slots: Vec<_> = section
        .read_list("ButtonList", 0x200)
        .unwrap_or_default()
        .into_iter()
        .map(|name| COMMAND_NAMES.iter().position(|known| *known == name))
        .collect();
    // 674650 stores command -> slot; a repeated name replaces its earlier slot.
    for i in 0..slots.len() {
        if slots[i].is_some() && slots[i + 1..].contains(&slots[i]) {
            slots[i] = None;
        }
    }
    slots
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommandBarLayout {
    pub bounds: Rect,
    pub left_cap: Rect,
    pub background: Rect,
    pub right_cap: Rect,
    pub tile_count: usize,
}

impl CommandBarLayout {
    pub fn new(
        screen: [u32; 2],
        sidebar_width: u32,
        left: [u32; 2],
        tile: [u32; 2],
        right: [u32; 2],
        open: bool,
    ) -> Option<Self> {
        let width = screen[0].checked_sub(sidebar_width)?;
        let available = width.checked_sub(left[0])?.checked_sub(right[0])?;
        if tile[0] == 0 || tile[1] == 0 || screen[1] < tile[1] {
            return None;
        }
        let count = available / tile[0];
        let right_x = width - right[0];
        let first_x = right_x - count * tile[0];
        let y = screen[1] - tile[1];
        let rect = |x, size: [u32; 2]| Rect {
            x: x as f32,
            y: y as f32,
            w: size[0] as f32,
            h: size[1] as f32,
        };
        Some(Self {
            bounds: rect(0, [width, tile[1]]),
            left_cap: rect(
                if open {
                    first_x - left[0]
                } else {
                    right_x - left[0]
                },
                left,
            ),
            background: rect(first_x, tile),
            right_cap: rect(right_x, right),
            tile_count: if open { count as usize } else { 0 },
        })
    }

    /// This layout moved by `(dx, dy)`: `new` lays the bar out from x 0 along
    /// the screen's bottom, and a layout may put it elsewhere.
    pub fn moved(self, dx: f32, dy: f32) -> Self {
        let moved = |rect: Rect| Rect {
            x: rect.x + dx,
            y: rect.y + dy,
            ..rect
        };
        Self {
            bounds: moved(self.bounds),
            left_cap: moved(self.left_cap),
            background: moved(self.background),
            right_cap: moved(self.right_cap),
            ..self
        }
    }

    pub fn slot(self, slot: usize) -> Option<Rect> {
        (slot < self.tile_count).then_some(Rect {
            x: self.background.x + slot as f32 * self.background.w,
            ..self.background
        })
    }

    /// The open bar starting at the frame's left edge, which gamemd's reaches
    /// only when whole tiles fill the strip (640 wide). `72FC60` keeps the
    /// tiles against the right cap, so what they leave of the strip stays bare
    /// rail left of the cap: 4 pixels at 800 wide, 32 at 1920, at most 51.
    /// VERA moves the cap and the tiles to the edge and gives the leftover to
    /// [`Self::filler`], among the empty tiles beside the right cap. This is a
    /// deliberate difference: the buttons sit that many pixels left of native.
    pub fn flush_left(self) -> Self {
        if self.tile_count == 0 {
            return self;
        }
        Self {
            left_cap: Rect {
                x: self.bounds.x,
                ..self.left_cap
            },
            background: Rect {
                x: self.bounds.x + self.left_cap.w,
                ..self.background
            },
            ..self
        }
    }

    /// The strip between the last whole tile and the right cap, where
    /// [`Self::flush_left`] leaves room for part of a tile.
    pub fn filler(self) -> Option<Rect> {
        let x = self.background.x + self.tile_count as f32 * self.background.w;
        let w = self.right_cap.x - x;
        (self.tile_count > 0 && w > 0.0).then_some(Rect {
            x,
            w,
            ..self.background
        })
    }
}

/// One piece of LSPACER along the bar's strip: source columns
/// `source_x..source_x + source_w` drawn over `x..x + w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpacerPiece {
    pub source_x: u32,
    pub source_w: u32,
    pub x: u32,
    pub w: u32,
}

/// How LSPACER covers the bottom strip from its left to `cap_x`, the bar's
/// left cap, past which the caps (and an open bar's tiles) cover the strip.
/// gamemd draws the canvas once at its own size (`6D0A20`, and `72F540` under
/// the in-battle menus), which reaches a closed bar's left cap only in frames
/// up to 1080 pixels wide, beyond which a gap stays black. VERA's frames take
/// any size, so a cap beyond the canvas keeps the left half, the rail's
/// riveted end, at its size and stretches the right half, plain rail, to the
/// cap. Frames up to 1080 pixels wide and the open bar draw it as native.
pub fn spacer_pieces(cap_x: u32, spacer_w: u32) -> [Option<SpacerPiece>; 2] {
    if cap_x <= spacer_w {
        let whole = SpacerPiece {
            source_x: 0,
            source_w: spacer_w,
            x: 0,
            w: spacer_w,
        };
        return [Some(whole), None];
    }
    let half = spacer_w / 2;
    [
        Some(SpacerPiece {
            source_x: 0,
            source_w: half,
            x: 0,
            w: half,
        }),
        Some(SpacerPiece {
            source_x: half,
            source_w: spacer_w - half,
            x: half,
            w: cap_x - half,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_identity_keeps_unknown_slots_and_last_duplicate() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[AdvancedCommandBar]\nButtonList=Team01,unknown,,Deploy,team01,Team01\n",
        );
        assert_eq!(
            read_button_list(ini.section_or_empty("AdvancedCommandBar")),
            [None, None, Some(4), None, Some(0)]
        );
    }
    #[test]
    fn original_800_by_600_command_bar_layout() {
        // Original complete72FC60, retail canvas headers: tools/sidebar_oracle/command_bar/layout-native.json.
        let layout =
            CommandBarLayout::new([800, 600], 168, [28, 32], [52, 32], [28, 32], true).unwrap();
        assert_eq!(layout.tile_count, 11);
        assert_eq!(
            layout.bounds,
            Rect {
                x: 0.,
                y: 568.,
                w: 632.,
                h: 32.
            }
        );
        assert_eq!(layout.left_cap.x, 4.);
        assert_eq!(layout.background.x, 32.);
        assert_eq!(layout.right_cap.x, 604.);
        assert_eq!(
            CommandBarLayout::new([800, 600], 168, [28, 32], [52, 32], [28, 32], false)
                .unwrap()
                .left_cap
                .x,
            576.
        );
    }
    /// Nothing moves where gamemd's own bar already starts at the left edge
    /// (640 wide, `layout-native.json`), or on a closed bar.
    #[test]
    fn flush_left_keeps_bars_that_need_no_move() {
        let bar = |width, open| {
            CommandBarLayout::new([width, 600], 168, [28, 32], [52, 32], [28, 32], open).unwrap()
        };
        assert_eq!(bar(640, true).flush_left(), bar(640, true));
        assert_eq!(bar(640, true).filler(), None);
        assert_eq!(bar(1920, false).flush_left(), bar(1920, false));
        assert_eq!(bar(1920, false).filler(), None);
    }
}
