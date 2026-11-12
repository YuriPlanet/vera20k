//! VERA extension: the Extras of a random map, the opt-in choices the
//! Generate Map dialog's Extras view offers (offline Skirmish only).
//!
//! Each [`Extra`] is a small number: `0` is the generator's own behaviour,
//! so a map with every Extra at `0` generates exactly as it did before the
//! Extra existed. A value is saved in the `.SED` under the Extra's key only
//! when it is not `0`, and rides in the share code
//! ([`RmgOptions::share_code`](super::options::RmgOptions::share_code)).
//! What the Extras place and the light of the moods are `RMGMD.INI
//! [Extras]` keys ([`ExtrasSettings`]); `docs/random-map-extras.md` lists them.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_value::strtrim_ascii;

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
    /// The light of the map: not 0 is one of [`MOODS`] in place of the
    /// light of the time of day.
    Mood,
    /// Town and farm buildings start damaged: 1 some of them, 2 most.
    Ruins,
    /// Farmsteads over open ground: 1 some, 2 many.
    Countryside,
    /// One monument at the middle of the map.
    Landmark,
    /// Fence along the sides of town buildings.
    Fences,
    /// A gem field at the middle of the map: 1 small, 2 large.
    CentreGems,
    /// One tech building at the same distance from every start: an oil
    /// derrick unless `FairTech=` names another.
    FairTech,
    /// A paved track from every start to the middle of the map.
    Roads,
    /// The trees round every start go, and the seed search weighs the
    /// building room of the tightest start.
    BaseRoom,
    /// The seed search weighs the water near the driest start.
    SeaAccess,
}

/// The moods of [`Extra::Mood`] from value 1, by their `RMGMD.INI
/// [Extras]` key, with the light each gives where its key is absent: ambient
/// light, then red, green and blue, in the percents `RMGMD.INI [General]`
/// gives a time of day (`TemperateAmbientLight`, `TemperateAmbientRed`..).
pub const MOODS: [(&str, i32, [i32; 3]); 5] = [
    ("GoldenHour", 90, [125, 98, 55]),
    ("RedDusk", 62, [135, 70, 55]),
    ("Moonlight", 52, [55, 85, 130]),
    ("Overcast", 68, [88, 92, 98]),
    ("ToxicHaze", 70, [80, 120, 60]),
];

/// `Farms=` where absent: houses and barns (2x2), then the one-cell silo,
/// outhouse and water tower.
const FARMS: [&str; 7] = [
    "CAFARM01", "CAFRMA", "CABARN02", "CATS01", "CAFARM02", "CAFRMB", "CAWT01",
];
/// `Landmarks=` where absent: none is capturable or a tech building, and
/// each stands whole as one type.
const LANDMARKS: [&str; 8] = [
    "CAWSH12", "CAPARS11", "CAEURO05", "CASTL04", "CAWASH19", "CAWASH14", "CAWASH15", "CAMEX01",
];
/// The ReadString buffer of a `[Extras]` value, as Ares gives `[Urban]`.
const VALUE_CAPACITY: usize = 0x800;

/// VERA extension: `RMGMD.INI [Extras]`, what the Extras place and the light
/// of the moods. An absent key keeps its default, today's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtrasSettings {
    /// `Farms=`: the buildings of the Countryside Extra. Those of more than
    /// one cell are the farmhouses a farmstead is built round.
    pub farms: Vec<String>,
    /// `Landmarks=`: the monuments the Landmark Extra draws one from.
    pub landmarks: Vec<String>,
    /// `FairTech=`: the neutral tech building (`[AI] NeutralTechBuildings=`)
    /// the Fair Tech Extra stands by every start.
    pub fair_tech: String,
    /// `FarmFence=` and `TownFence=`: the fence overlays of the Countryside
    /// and Fences Extras.
    pub farm_fence: String,
    pub town_fence: String,
    /// Each mood's ambient light and red, green and blue, as [`MOODS`].
    pub moods: [(i32, [i32; 3]); MOODS.len()],
}

impl Default for ExtrasSettings {
    fn default() -> Self {
        Self {
            farms: FARMS.map(String::from).to_vec(),
            landmarks: LANDMARKS.map(String::from).to_vec(),
            fair_tech: "CAOILD".into(),
            farm_fence: "CAFNCW".into(),
            town_fence: "CAFNCB".into(),
            moods: MOODS.map(|(_, ambient, rgb)| (ambient, rgb)),
        }
    }
}

impl ExtrasSettings {
    /// Read `[Extras]` over the values held. Lists split on commas, as Ares'
    /// `[Urban]` lists do, and each name drops the blanks round it; a mood
    /// changes only when all four of its numbers read.
    pub(super) fn apply(&mut self, section: &IniSection) {
        let names = |key: &str, current: &mut Vec<String>| {
            if let Some(list) = section.read_list(key, VALUE_CAPACITY) {
                *current = list
                    .into_iter()
                    .map(strtrim_ascii)
                    .filter(|name| !name.is_empty())
                    .map(String::from)
                    .collect();
            }
        };
        names("Farms", &mut self.farms);
        names("Landmarks", &mut self.landmarks);
        for (key, name) in [
            ("FairTech", &mut self.fair_tech),
            ("FarmFence", &mut self.farm_fence),
            ("TownFence", &mut self.town_fence),
        ] {
            *name = section.read_string(key, name, VALUE_CAPACITY);
        }
        for ((key, ..), mood) in MOODS.iter().zip(&mut self.moods) {
            if let Some([ambient, red, green, blue]) =
                section.read_int_fields_complete::<4>(key, VALUE_CAPACITY)
            {
                *mood = (ambient, [red, green, blue]);
            }
        }
    }
}

impl Extra {
    /// Every Extra, in the order the dialog lists them and the share code
    /// carries them. New ones go at the end, so older codes still read.
    pub const ALL: [Self; 12] = [
        Self::OreVariant,
        Self::TownVariant,
        Self::Mood,
        Self::Ruins,
        Self::Countryside,
        Self::Landmark,
        Self::Fences,
        Self::CentreGems,
        Self::FairTech,
        Self::Roads,
        Self::BaseRoom,
        Self::SeaAccess,
    ];

    const fn index(self) -> usize {
        self as usize
    }

    /// The `.SED` key of this Extra.
    pub const fn key(self) -> &'static str {
        match self {
            Self::OreVariant => "VeraOreVariant",
            Self::TownVariant => "VeraTownVariant",
            Self::Mood => "VeraMood",
            Self::Ruins => "VeraRuins",
            Self::Countryside => "VeraCountryside",
            Self::Landmark => "VeraLandmark",
            Self::Fences => "VeraFences",
            Self::CentreGems => "VeraCentreGems",
            Self::FairTech => "VeraFairTech",
            Self::Roads => "VeraRoads",
            Self::BaseRoom => "VeraBaseRoom",
            Self::SeaAccess => "VeraSeaAccess",
        }
    }

    /// How many values it takes, `0` first.
    pub const fn choices(self) -> u8 {
        match self {
            Self::OreVariant | Self::TownVariant => 10,
            Self::Mood => MOODS.len() as u8 + 1,
            Self::Ruins | Self::Countryside | Self::CentreGems => 3,
            Self::Landmark
            | Self::Fences
            | Self::FairTech
            | Self::Roads
            | Self::BaseRoom
            | Self::SeaAccess => 2,
        }
    }

    /// Whether the Extra places buildings from VERA's catalogue, which the
    /// app then resolves as it does for the Buildings slider.
    pub const fn needs_catalog(self) -> bool {
        matches!(self, Self::Countryside | Self::Landmark)
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
