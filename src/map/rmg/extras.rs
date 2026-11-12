//! VERA extension: the Extras of a random map, the opt-in choices the
//! Generate Map dialog's Extras view offers (offline Skirmish only).
//!
//! Each [`Extra`] is a small number: `0` is the generator's own behaviour,
//! so a map with every Extra at `0` generates exactly as it did before the
//! Extra existed. A value is saved in the `.SED` under the Extra's key only
//! when it is not `0`, and rides in the share code
//! ([`RmgOptions::share_code`](super::options::RmgOptions::share_code)).

/// One opt-in choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Extra {
    /// The ore fields are drawn from a stream of their own: the land, the
    /// water and the starts stay as the seed makes them, and the stages
    /// after the ore (hills, ground patches, trees) draw the stream they
    /// drew, stepping round the ore where it has moved.
    OreVariant,
    /// The same for the towns of the Buildings slider.
    TownVariant,
}

impl Extra {
    /// Every Extra, in the order the dialog lists them and the share code
    /// carries them. New ones go at the end, so older codes still read.
    pub const ALL: [Self; 2] = [Self::OreVariant, Self::TownVariant];

    const fn index(self) -> usize {
        self as usize
    }

    /// The `.SED` key of this Extra.
    pub const fn key(self) -> &'static str {
        match self {
            Self::OreVariant => "VeraOreVariant",
            Self::TownVariant => "VeraTownVariant",
        }
    }

    /// How many values it takes, `0` first.
    pub const fn choices(self) -> u8 {
        match self {
            Self::OreVariant | Self::TownVariant => 10,
        }
    }
}

/// The value of every [`Extra`], all `0` by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RmgExtras {
    values: [u8; Extra::ALL.len()],
}

impl RmgExtras {
    pub fn get(&self, extra: Extra) -> u8 {
        self.values[extra.index()]
    }

    /// Set a value, held to the Extra's choices.
    pub fn set(&mut self, extra: Extra, value: u8) {
        self.values[extra.index()] = value.min(extra.choices() - 1);
    }

    /// Step to the next choice, round to `0` after the last.
    pub fn cycle(&mut self, extra: Extra) {
        self.set(extra, (self.get(extra) + 1) % extra.choices());
    }

    /// Whether every Extra is off.
    pub fn is_default(&self) -> bool {
        self.values.iter().all(|&value| value == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_hold_to_their_choices_and_cycle_round() {
        let mut extras = RmgExtras::default();
        assert!(extras.is_default());
        extras.set(Extra::OreVariant, 200);
        assert_eq!(extras.get(Extra::OreVariant), 9);
        extras.cycle(Extra::OreVariant);
        assert_eq!(extras.get(Extra::OreVariant), 0);
        extras.cycle(Extra::TownVariant);
        assert_eq!(extras.get(Extra::TownVariant), 1);
        assert!(!extras.is_default());
    }
}
