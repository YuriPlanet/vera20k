//! Parts a layout adds to the interface: a button that gives one named
//! command when clicked, and a text that shows one named value.
//!
//! A layout names a new part in the `Parts=` of the sidebar (`Side`) or of
//! the battlefield (`Field`) and gives it a section with `Type=`. The fixed
//! lists a part may name are the original's own: a command is one a key or a
//! command-bar button gives, a value is something the player may see.

use crate::ui::field_layout::PartBox;

/// The part an added part sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// The sidebar: sidebar pixels from its corner, scaled with it.
    Side,
    /// The battlefield: frame pixels, as its own pieces are placed.
    Field,
}

/// A value a text part can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextValue {
    /// The money number as the sidebar counts it.
    Credits,
    /// The power the player's base makes.
    Power,
    /// The power the player's base uses.
    Drain,
    /// The player's name.
    Name,
    /// How long the match has run.
    Time,
    /// What the player has destroyed.
    Kills,
    /// What the player has lost.
    Losses,
}

impl TextValue {
    pub const ALL: [Self; 7] = [
        Self::Credits,
        Self::Power,
        Self::Drain,
        Self::Name,
        Self::Time,
        Self::Kills,
        Self::Losses,
    ];

    /// The value's name in a layout file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Credits => "Credits",
            Self::Power => "Power",
            Self::Drain => "Drain",
            Self::Name => "Name",
            Self::Time => "Time",
            Self::Kills => "Kills",
            Self::Losses => "Losses",
        }
    }

    /// The value a layout file names, without regard to capitals.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|value| value.name().eq_ignore_ascii_case(name))
    }
}

/// What an added part is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddedKind {
    /// A box with `label` in it that gives `command`: a hotkey's name from
    /// the keyboard file or a command-bar button's from `uimd.ini`.
    Button {
        command: String,
        label: String,
    },
    Text {
        value: TextValue,
    },
}

/// One part a layout adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedPart {
    /// Its section in the file, for messages about it.
    pub section: String,
    pub holder: Holder,
    pub kind: AddedKind,
    /// Where it sits in its holder. A size left out is the part's own: its
    /// text's, with a button's padding.
    pub place: PartBox,
}

impl AddedPart {
    /// The part's rectangle in `area`, the holder's (`x, y, width, height`),
    /// when its own size is `natural`.
    pub fn rect_in(&self, area: [i32; 4], natural: [i32; 2]) -> [i32; 4] {
        PartBox {
            width: Some(self.place.width.unwrap_or(natural[0])),
            height: Some(self.place.height.unwrap_or(natural[1])),
            ..self.place
        }
        .in_area(area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::field_layout::Hold;

    #[test]
    fn an_added_part_keeps_its_own_size_in_the_corner_it_holds() {
        let part = |place| AddedPart {
            section: "Mine.Stop".to_owned(),
            holder: Holder::Field,
            kind: AddedKind::Text {
                value: TextValue::Credits,
            },
            place,
        };
        let field = [168, 32, 632, 568];
        // From the top left by default, as big as its text.
        let plain = part(PartBox {
            x: 10,
            y: 4,
            ..PartBox::WHOLE
        });
        assert_eq!(plain.rect_in(field, [40, 13]), [178, 36, 40, 13]);
        // In from the bottom right, with a width of its own.
        let corner = part(PartBox {
            hold: Hold::named("BottomRight").unwrap(),
            x: 6,
            y: 6,
            width: Some(80),
            height: None,
        });
        assert_eq!(corner.rect_in(field, [40, 13]), [714, 581, 80, 13]);
        assert_eq!(TextValue::named("credits"), Some(TextValue::Credits));
        assert_eq!(TextValue::named("Gold"), None);
    }
}
