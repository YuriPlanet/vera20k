//! Where the pieces drawn over the battlefield sit: the message list, the
//! timers, the win or lose banner, the tooltip and Phobos' column of
//! superweapon buttons.
//!
//! Each of the first three has a box inside the battlefield and keeps its own
//! corner of it: messages start at the box's top left, timers stack up from
//! its bottom right, the banner sits in its middle. In the original interface
//! every box is the whole battlefield, which is the default here; a side's
//! layout (`ui::match_layout`) moves and sizes them. Numbers are frame
//! pixels: these pieces do not scale with the Sidebar size.

/// A piece the battlefield holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPart {
    Messages,
    Timers,
    Banner,
    /// Phobos' column of superweapon buttons.
    Powers,
}

impl FieldPart {
    /// The original battlefield's parts, in its `Parts=` order.
    pub const ALL: [Self; 4] = [Self::Messages, Self::Timers, Self::Banner, Self::Powers];

    /// The part's name in a layout file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Messages => "Messages",
            Self::Timers => "Timers",
            Self::Banner => "Banner",
            Self::Powers => "Powers",
        }
    }

    /// The part a layout file names, without regard to capitals.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|part| part.name().eq_ignore_ascii_case(name))
    }
}

/// Where along one axis a box holds on to the area it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Along {
    /// The left or the top edge.
    #[default]
    Start,
    Middle,
    /// The right or the bottom edge.
    End,
}

/// The corner or edge of its area a box is measured from. The default is the
/// top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hold {
    pub x: Along,
    pub y: Along,
}

impl Hold {
    /// The nine words a layout file may write, without regard to capitals.
    pub fn named(word: &str) -> Option<Self> {
        use Along::{End, Middle, Start};
        [
            ("TopLeft", Start, Start),
            ("Top", Middle, Start),
            ("TopRight", End, Start),
            ("Left", Start, Middle),
            ("Middle", Middle, Middle),
            ("Right", End, Middle),
            ("BottomLeft", Start, End),
            ("Bottom", Middle, End),
            ("BottomRight", End, End),
        ]
        .into_iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(word))
        .map(|(_, x, y)| Self { x, y })
    }
}

/// A part's box: the edge it holds, how far in from there it starts, and its
/// size. `None` is "what is left": from the offset to the far edge, or the
/// whole area when it holds the middle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartBox {
    pub hold: Hold,
    pub x: i32,
    pub y: i32,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

impl PartBox {
    /// The whole area: from its top left, as big as it is. The default.
    pub const WHOLE: Self = Self {
        hold: Hold {
            x: Along::Start,
            y: Along::Start,
        },
        x: 0,
        y: 0,
        width: None,
        height: None,
    };

    /// The box in the pixels `area` (`x, y, width, height`) is given in. The
    /// default box is the area itself. A box keeps at least one pixel.
    pub fn in_area(self, area: [i32; 4]) -> [i32; 4] {
        let axis = |along: Along, start: i32, extent: i32, offset: i32, size: Option<i32>| {
            let size = size
                .unwrap_or(match along {
                    Along::Middle => extent,
                    Along::Start | Along::End => extent.saturating_sub(offset),
                })
                .max(1);
            let at = match along {
                Along::Start => start.saturating_add(offset),
                Along::Middle => start
                    .saturating_add((extent - size) / 2)
                    .saturating_add(offset),
                Along::End => start
                    .saturating_add(extent)
                    .saturating_sub(size)
                    .saturating_sub(offset),
            };
            (at, size)
        };
        let (x, width) = axis(self.hold.x, area[0], area[2], self.x, self.width);
        let (y, height) = axis(self.hold.y, area[1], area[3], self.y, self.height);
        [x, y, width, height]
    }
}

/// Where a tooltip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooltipPlace {
    /// The corner of the screen it stays in, or `None` to follow the cursor.
    pub hold: Option<Hold>,
    /// How far it is from the cursor, or in from the corner it holds.
    pub x: i32,
    pub y: i32,
}

impl Default for TooltipPlace {
    fn default() -> Self {
        Self::ORIGINAL
    }
}

impl TooltipPlace {
    /// The original's: 12 right of the cursor and 16 under it.
    pub const ORIGINAL: Self = Self {
        hold: None,
        x: 12,
        y: 16,
    };

    /// The top left of a tooltip box `size` big on a screen `screen`
    /// (`x, y, width, height`), when it stays in a corner; `None` when it
    /// follows the cursor.
    pub fn fixed_in(self, screen: [f32; 4], size: [f32; 2]) -> Option<[f32; 2]> {
        let hold = self.hold?;
        let axis = |along: Along, start: f32, extent: f32, offset: i32, size: f32| match along {
            Along::Start => start + offset as f32,
            Along::Middle => start + ((extent - size) / 2.0).floor() + offset as f32,
            Along::End => start + extent - size - offset as f32,
        };
        Some([
            axis(hold.x, screen[0], screen[2], self.x, size[0]),
            axis(hold.y, screen[1], screen[3], self.y, size[1]),
        ])
    }
}

/// The places of the pieces over the battlefield for one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldLayout {
    pub messages: PartBox,
    pub timers: PartBox,
    pub banner: PartBox,
    /// How far Phobos' superweapon column is moved right and down from its
    /// own place at the battlefield's left edge.
    pub powers: [i32; 2],
    /// Which of [`FieldPart::ALL`] the battlefield has.
    pub shown: [bool; 4],
    pub tooltip: TooltipPlace,
}

impl Default for FieldLayout {
    fn default() -> Self {
        Self::ORIGINAL
    }
}

impl FieldLayout {
    /// The original interface's: every box the whole battlefield.
    pub const ORIGINAL: Self = Self {
        messages: PartBox::WHOLE,
        timers: PartBox::WHOLE,
        banner: PartBox::WHOLE,
        powers: [0, 0],
        shown: [true; 4],
        tooltip: TooltipPlace::ORIGINAL,
    };

    /// Whether the battlefield has this part.
    pub fn shows(&self, part: FieldPart) -> bool {
        self.shown[part as usize]
    }

    /// The box of a part that has one, inside the battlefield `field`
    /// (`x, y, width, height`). Phobos' column has no box and gets the
    /// battlefield moved by its offset.
    pub fn part_rect(&self, part: FieldPart, field: [i32; 4]) -> [i32; 4] {
        match part {
            FieldPart::Messages => self.messages.in_area(field),
            FieldPart::Timers => self.timers.in_area(field),
            FieldPart::Banner => self.banner.in_area(field),
            FieldPart::Powers => [
                field[0].saturating_add(self.powers[0]),
                field[1].saturating_add(self.powers[1]),
                field[2],
                field[3],
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_is_the_whole_area_until_a_layout_moves_or_sizes_it() {
        let field = [168, 32, 632, 568];
        assert_eq!(PartBox::default().in_area(field), field);
        // From the top left, what is left runs to the far edges.
        let inset = PartBox {
            x: 200,
            y: 40,
            ..PartBox::default()
        };
        assert_eq!(inset.in_area(field), [368, 72, 432, 528]);
        // From the bottom right, offsets count inward and a size is kept.
        let corner = PartBox {
            hold: Hold::named("bottomright").unwrap(),
            x: 10,
            y: 20,
            width: Some(300),
            height: Some(100),
        };
        assert_eq!(corner.in_area(field), [490, 480, 300, 100]);
        // In the middle, a sized box is centred and then moved.
        let middle = PartBox {
            hold: Hold::named("Middle").unwrap(),
            y: -50,
            width: Some(200),
            height: Some(68),
            ..PartBox::default()
        };
        assert_eq!(middle.in_area(field), [384, 232, 200, 68]);
        // A size past the area keeps one pixel instead of going negative.
        let past = PartBox {
            x: 9000,
            ..PartBox::default()
        };
        assert_eq!(past.in_area(field)[2], 1);
        assert!(Hold::named("Centre").is_none());
    }

    #[test]
    fn a_tooltip_follows_the_cursor_or_stays_in_a_corner() {
        let screen = [0.0, 0.0, 800.0, 600.0];
        assert_eq!(TooltipPlace::default().fixed_in(screen, [86.0, 38.0]), None);
        let corner = TooltipPlace {
            hold: Hold::named("BottomLeft"),
            x: 8,
            y: 4,
        };
        assert_eq!(corner.fixed_in(screen, [86.0, 38.0]), Some([8.0, 558.0]));
    }
}
