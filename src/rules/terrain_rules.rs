//! Terrain land-type semantics parsed from rules.ini / rulesmd.ini.
//!
//! This module converts raw TMP terrain bytes to the twelve YR `LandType`s and,
//! when available, parses each land type's rules section for buildability and
//! per-SpeedType costs. A cell keeps only its `LandType` (`CellClass+0xEC`);
//! everything else about its land is asked of that value or of this table.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::locomotor_type::SpeedType;
use crate::util::fixed_math::{SIM_ONE, SimFixed};

/// Canonical YR land types in their native numeric order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum LandType {
    Clear = 0,
    Road = 1,
    Water = 2,
    Rock = 3,
    Wall = 4,
    Tiberium = 5,
    Beach = 6,
    Rough = 7,
    Ice = 8,
    Railroad = 9,
    Tunnel = 10,
    Weeds = 11,
}

impl LandType {
    pub const ALL: [Self; 12] = [
        Self::Clear,
        Self::Road,
        Self::Water,
        Self::Rock,
        Self::Wall,
        Self::Tiberium,
        Self::Beach,
        Self::Rough,
        Self::Ice,
        Self::Railroad,
        Self::Tunnel,
        Self::Weeds,
    ];

    pub const fn as_index(self) -> u8 {
        self as u8
    }

    pub const fn section_name(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Road => "Road",
            Self::Water => "Water",
            Self::Rock => "Rock",
            Self::Wall => "Wall",
            Self::Tiberium => "Tiberium",
            Self::Beach => "Beach",
            Self::Rough => "Rough",
            Self::Ice => "Ice",
            Self::Railroad => "Railroad",
            Self::Tunnel => "Tunnel",
            Self::Weeds => "Weeds",
        }
    }

    pub const fn from_index(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Clear,
            1 => Self::Road,
            2 => Self::Water,
            3 => Self::Rock,
            4 => Self::Wall,
            5 => Self::Tiberium,
            6 => Self::Beach,
            7 => Self::Rough,
            8 => Self::Ice,
            9 => Self::Railroad,
            10 => Self::Tunnel,
            11 => Self::Weeds,
            _ => return None,
        })
    }
}

/// Native TMP terrain byte to canonical land-type table.
///
/// Retail provenance: subtile land type — `IsometricTileTypeClass::GetSubtileLandType`
/// @ `0x00544BE0`, indexing the 16-dword table at `0x008288E4` with the subtile
/// record's byte `+0x29`. Entry-for-entry identical to this array
/// (`[0, 8, 8, 8, 8, 10, 9, 3, 3, 2, 6, 1, 1, 0, 7, 3]` in `LandType` indices).
///
/// VERA-internal, gamemd has no equivalent: the bounds check in
/// [`tmp_terrain_to_land_type`]. Native reads the byte with `MOVSX` at
/// 0x00544C05 and indexes without a range test, so a terrain byte outside
/// 0..=15 — including `0x80`..=`0xFF`, which index *backwards* from the table —
/// reads whatever lies around it. Whether any retail `.tmp` carries such a byte
/// is UNCHECKED; the guard makes VERA answer `Clear` where gamemd would answer
/// with adjacent data.
pub const TMP_TERRAIN_TO_LAND_TYPE: [LandType; 16] = [
    LandType::Clear,
    LandType::Ice,
    LandType::Ice,
    LandType::Ice,
    LandType::Ice,
    LandType::Tunnel,
    LandType::Railroad,
    LandType::Rock,
    LandType::Rock,
    LandType::Water,
    LandType::Beach,
    LandType::Road,
    LandType::Road,
    LandType::Clear,
    LandType::Rough,
    LandType::Rock,
];

/// Convert a raw TMP terrain byte to its canonical YR land type.
pub fn tmp_terrain_to_land_type(tmp_terrain_type: u8) -> LandType {
    TMP_TERRAIN_TO_LAND_TYPE
        .get(tmp_terrain_type as usize)
        .copied()
        .unwrap_or(LandType::Clear)
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct SpeedCostProfile {
    pub foot: Option<u8>,
    pub track: Option<u8>,
    pub wheel: Option<u8>,
    pub float: Option<u8>,
    pub amphibious: Option<u8>,
    pub float_beach: Option<u8>,
    pub hover: Option<u8>,
}

impl SpeedCostProfile {
    /// Every authored SpeedType row at 0.0, as the native table holds an
    /// unauthored land type.
    pub const ZERO: Self = Self {
        foot: Some(0),
        track: Some(0),
        wheel: Some(0),
        float: Some(0),
        amphibious: Some(0),
        float_beach: Some(0),
        hover: Some(0),
    };

    pub fn cost_for_speed_type(&self, speed_type: SpeedType) -> Option<u8> {
        match speed_type {
            // Keep the native parser's invalid index unavailable to live entry
            // queries. Reading memory before the native table is not modeled.
            SpeedType::Invalid => None,
            SpeedType::Foot => self.foot,
            SpeedType::Track => self.track,
            SpeedType::Wheel => self.wheel,
            SpeedType::Float => self.float,
            SpeedType::Amphibious => self.amphibious,
            SpeedType::FloatBeach => self.float_beach,
            SpeedType::Hover => self.hover,
            SpeedType::Winged => Some(100),
        }
    }

    /// Runtime speed multiplier for a given SpeedType.
    ///
    /// Converts the capped INI percentage to a SimFixed fraction (0.0–1.0).
    /// Zero remains zero and a missing row defaults to full speed.
    pub fn speed_multiplier_for(&self, speed_type: SpeedType) -> SimFixed {
        match self.cost_for_speed_type(speed_type) {
            Some(pct) => {
                let clamped = pct.min(100);
                SimFixed::from_num(clamped) / SimFixed::from_num(100u8)
            }
            None => SIM_ONE,
        }
    }
}

/// One land type's rules row: its `Buildable=` and SpeedType rows as the
/// native reader stores them, plus VERA's ground-walk block for that land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandTypeSemantics {
    pub buildable: bool,
    /// VERA's ground-walk block (`ResolvedTerrainCell::ground_walk_blocked`):
    /// true for Water, Rock and Wall. Neither an INI key nor a gamemd field;
    /// gamemd's movers read the SpeedType rows instead.
    pub ground_blocked: bool,
    pub speed_costs: SpeedCostProfile,
}

impl LandTypeSemantics {
    pub fn cost_for_speed_type(&self, speed_type: SpeedType) -> Option<u8> {
        self.speed_costs.cost_for_speed_type(speed_type)
    }

    /// The row of a land type no rules layer authors: the native reader skips
    /// it (see [`parse_speed_costs`]), so it keeps the image's zeros, every
    /// SpeedType row 0.0 and not `Buildable`. `ground_blocked` keeps its land
    /// value. The native Winged slot is 0.0 here too, where
    /// [`SpeedCostProfile::cost_for_speed_type`] still answers Winged with the
    /// authored rows' constant.
    fn zero(land_type: LandType) -> Self {
        Self {
            buildable: false,
            ground_blocked: ground_blocked(land_type),
            speed_costs: SpeedCostProfile::ZERO,
        }
    }
}

/// VERA's ground-walk block for a land type: Water, Rock and Wall.
const fn ground_blocked(land_type: LandType) -> bool {
    matches!(land_type, LandType::Water | LandType::Rock | LandType::Wall)
}

/// The scenario's land-type table (`0x0089EA40`): one row per `LandType`,
/// read from its section or, for an unauthored section, the zero row.
#[derive(Debug, Clone)]
pub struct TerrainRules {
    rows: [LandTypeSemantics; LandType::ALL.len()],
}

impl Default for TerrainRules {
    /// The table before any rules layer is read: every row zero.
    fn default() -> Self {
        Self {
            rows: LandType::ALL.map(LandTypeSemantics::zero),
        }
    }
}

impl TerrainRules {
    pub fn from_ini(ini: &IniFile) -> Self {
        Self {
            rows: LandType::ALL.map(|land_type| {
                ini.section(land_type.section_name()).map_or_else(
                    || LandTypeSemantics::zero(land_type),
                    |section| build_semantics(land_type, section),
                )
            }),
        }
    }

    /// The row of `land_type`: its section's, or the zero row.
    pub fn row(&self, land_type: LandType) -> &LandTypeSemantics {
        &self.rows[usize::from(land_type.as_index())]
    }

    /// [`Self::row`] for a `CellClass+0xEC` value; `None` only for a value
    /// outside the twelve land types.
    pub fn semantics_for_land_type(&self, land_type: u8) -> Option<&LandTypeSemantics> {
        LandType::from_index(land_type).map(|land_type| self.row(land_type))
    }
}

fn build_semantics(land_type: LandType, section: &IniSection) -> LandTypeSemantics {
    LandTypeSemantics {
        buildable: section.read_bool("Buildable", false),
        ground_blocked: ground_blocked(land_type),
        speed_costs: parse_speed_costs(section),
    }
}

/// The seven authored per-SpeedType rows of one land-type section.
///
/// Retail provenance: land-type speed table —
/// `RulesClass::ReadSpeedTypeLandTypeTable` @ `0x00674000`, filling the twelve
/// 36-byte rows at `0x0089EA40`. The native row is nine dwords: `Foot`, `Track`,
/// `Wheel`, `Hover`, a **`Winged` slot hard-stored as `1.0`** (`MOV dword ptr
/// [EBX + 0xC], 0x3F800000` @ `0x00674148`, never read from INI — which is why [`SpeedCostProfile::cost_for_speed_type`]
/// answers `Winged` with a constant), `Float`, `Amphibious`, `FloatBeach`, and
/// the `Buildable` byte. Every value goes through `CCINIClass::ReadDouble`
/// @ `0x005283D0` — `sscanf("%f")`, then `× 0.01` when `strchr` finds a `%`
/// (0x00528576) — with a default of `1.0`, and is then capped by
/// `if (1.0 <= value) value = 1.0`. A missing key inside a present section
/// resolves to full speed.
///
/// A missing *section* does not: the native loop skips the whole row when
/// `INIClass::FindSectionByName` returns 0 (`JZ` at `0x00674022`), and the
/// block at `0x0089EA40` is zero-filled in the image, so an unauthored land
/// type is impassable to every SpeedType and not `Buildable`.
/// [`TerrainRules`] answers that zero row for it.
fn parse_speed_costs(section: &IniSection) -> SpeedCostProfile {
    SpeedCostProfile {
        foot: parse_cost(section, "Foot"),
        track: parse_cost(section, "Track"),
        wheel: parse_cost(section, "Wheel"),
        float: parse_cost(section, "Float"),
        amphibious: parse_cost(section, "Amphibious"),
        float_beach: parse_cost(section, "FloatBeach"),
        hover: parse_cost(section, "Hover"),
    }
}

/// One authored row, as a whole-percent 0..=100.
///
/// The `1.0` default and the upper cap are the native reader's own (see
/// [`parse_speed_costs`]). Two VERA-internal differences, both inert on every
/// stock value — retail authors these only as whole percents in `[Clear]`
/// through `[Weeds]`:
///
/// - **the lower clamp.** gamemd caps the high side only, so a negative row
///   would reach the movement chain as a negative multiplier; the `u8` storage
///   here cannot carry one and floors it at zero instead. Trigger: a mod INI
///   authoring a negative percentage. Player effect: none in stock play. In a
///   mod, `CheckCellPassability @ 0x004834A0` then refuses the cell, where
///   gamemd's compare against 0.0 admits it.
/// - **whole-percent quantisation.** gamemd keeps each row as an `f32`; this
///   truncates to a whole percent, so a modded `Track=72.5%` becomes 72.
///   Trigger: a fractional percentage. Player effect: none in stock play, where
///   every authored value is a multiple of five percent. In a mod, a positive
///   row below 1% becomes 0, which `0x004834A0` refuses where gamemd admits.
fn parse_cost(section: &IniSection, key: &str) -> Option<u8> {
    let multiplier = (section.read_double(key, 1.0) as f32).clamp(0.0, 1.0);
    Some((multiplier * 100.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sixteen dwords read out of `g_nTmpTerrainToLandTypeTable` at
    /// `0x008288E4`, in order, as raw `LandType` indices.
    const NATIVE_TMP_TERRAIN_TABLE: [u8; 16] = [0, 8, 8, 8, 8, 10, 9, 3, 3, 2, 6, 1, 1, 0, 7, 3];

    #[test]
    fn tmp_terrain_table_matches_the_native_dwords() {
        for (tmp_byte, native_index) in NATIVE_TMP_TERRAIN_TABLE.iter().copied().enumerate() {
            assert_eq!(
                tmp_terrain_to_land_type(tmp_byte as u8).as_index(),
                native_index,
                "TMP terrain byte {tmp_byte}"
            );
        }
    }

    #[test]
    fn winged_ignores_the_authored_rows() {
        // `RulesClass::ReadSpeedTypeLandTypeTable` stores 1.0 into the Winged
        // slot without reading INI, so no land-type section can slow an
        // aircraft down — including a section that zeroes every other row.
        let ini = IniFile::from_str("[Rock]\nFoot=0%\nTrack=0%\nWheel=0%\nHover=0%\nFloat=0%\n");
        let rock = TerrainRules::from_ini(&ini)
            .semantics_for_land_type(LandType::Rock.as_index())
            .copied()
            .expect("rock semantics");
        assert_eq!(rock.cost_for_speed_type(SpeedType::Foot), Some(0));
        assert_eq!(rock.cost_for_speed_type(SpeedType::Winged), Some(100));
        assert_eq!(
            rock.speed_costs.speed_multiplier_for(SpeedType::Winged),
            SIM_ONE
        );
    }

    #[test]
    fn an_authored_row_above_full_speed_caps_at_full_speed() {
        // The native reader's own cap: `if (1.0 <= value) value = 1.0`.
        let ini = IniFile::from_str("[Road]\nFoot=250%\nTrack=100%\n");
        let road = TerrainRules::from_ini(&ini)
            .semantics_for_land_type(LandType::Road.as_index())
            .copied()
            .expect("road semantics");
        assert_eq!(road.cost_for_speed_type(SpeedType::Foot), Some(100));
        // And an unauthored row defaults to full speed, not to zero.
        assert_eq!(road.cost_for_speed_type(SpeedType::Hover), Some(100));
    }

    #[test]
    fn terrain_rules_parse_known_sections_and_buildability() {
        let ini = IniFile::from_str(
            "[Clear]\nBuildable=yes\nFoot=100%\nTrack=100%\nWheel=100%\n\
             [Rough]\nBuildable=yes\nFoot=90%\nTrack=75%\nWheel=60%\n\
             [Water]\nBuildable=no\nFoot=0%\nTrack=0%\nFloat=100%\nHover=100%\n\
             [Rock]\nBuildable=no\nFoot=0%\nTrack=0%\n",
        );
        let terrain_rules = TerrainRules::from_ini(&ini);

        let clear = terrain_rules
            .semantics_for_land_type(0)
            .expect("clear semantics");
        assert!(clear.buildable);
        assert!(!clear.ground_blocked);
        assert_eq!(clear.cost_for_speed_type(SpeedType::Track), Some(100));

        let rough = terrain_rules
            .semantics_for_land_type(LandType::Rough.as_index())
            .expect("rough semantics");
        assert!(!rough.ground_blocked);
        assert_eq!(rough.cost_for_speed_type(SpeedType::Wheel), Some(60));

        let water = terrain_rules
            .semantics_for_land_type(LandType::Water.as_index())
            .expect("water semantics");
        assert!(water.ground_blocked);
        assert!(!water.buildable);
        assert_eq!(water.cost_for_speed_type(SpeedType::Float), Some(100));
        assert_eq!(water.cost_for_speed_type(SpeedType::Track), Some(0));

        let rock = terrain_rules
            .semantics_for_land_type(LandType::Rock.as_index())
            .expect("rock semantics");
        assert!(rock.ground_blocked);
        assert_eq!(rock.cost_for_speed_type(SpeedType::Foot), Some(0));
    }

    #[test]
    fn a_missing_section_is_the_zero_row() {
        // `RulesClass::ReadSpeedTypeLandTypeTable @ 0x00674000` skips an
        // unauthored section (`JZ` at `0x00674022`), leaving the image's
        // zeros at `0x0089EA40`: every SpeedType row 0.0, not Buildable.
        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str("[Clear]\nBuildable=yes\n"));
        let zero = SpeedCostProfile {
            foot: Some(0),
            track: Some(0),
            wheel: Some(0),
            float: Some(0),
            amphibious: Some(0),
            float_beach: Some(0),
            hover: Some(0),
        };
        for land_type in LandType::ALL {
            let name = land_type.section_name();
            let row = terrain_rules
                .semantics_for_land_type(land_type.as_index())
                .unwrap_or_else(|| panic!("{name} row"));
            if land_type == LandType::Clear {
                // An authored section reads 1.0 for each absent key.
                assert!(row.buildable);
                assert_eq!(row.cost_for_speed_type(SpeedType::Track), Some(100));
                continue;
            }
            assert_eq!(row.speed_costs, zero, "{name}");
            assert!(!row.buildable, "{name}");
            assert_eq!(
                row.ground_blocked,
                matches!(land_type, LandType::Water | LandType::Rock | LandType::Wall),
                "{name}"
            );
        }
        assert!(terrain_rules.semantics_for_land_type(12).is_none());
    }

    #[test]
    fn beach_costs_from_ini() {
        let ini = IniFile::from_str(
            "[Beach]\nFoot=0%\nTrack=0%\nWheel=0%\nFloat=0%\n\
             FloatBeach=100%\nHover=75%\nAmphibious=60%\nBuildable=no\n",
        );
        let terrain_rules = TerrainRules::from_ini(&ini);
        let beach = terrain_rules
            .semantics_for_land_type(LandType::Beach.as_index())
            .expect("beach semantics");
        assert!(!beach.buildable);
        assert_eq!(beach.cost_for_speed_type(SpeedType::Foot), Some(0));
        assert_eq!(beach.cost_for_speed_type(SpeedType::Track), Some(0));
        assert_eq!(beach.cost_for_speed_type(SpeedType::FloatBeach), Some(100));
        assert_eq!(beach.cost_for_speed_type(SpeedType::Hover), Some(75));
        assert_eq!(beach.cost_for_speed_type(SpeedType::Amphibious), Some(60));
    }

    #[test]
    fn tunnel_and_railroad_bytes_map_correctly() {
        let ini = IniFile::from_str(
            "[Tunnel]\nFoot=100%\nTrack=100%\nBuildable=no\n\
             [Railroad]\nFoot=90%\nTrack=100%\nBuildable=no\n",
        );
        let terrain_rules = TerrainRules::from_ini(&ini);
        let tunnel = terrain_rules
            .semantics_for_land_type(LandType::Tunnel.as_index())
            .expect("tunnel semantics");
        assert_eq!(tunnel.cost_for_speed_type(SpeedType::Foot), Some(100));

        let railroad = terrain_rules
            .semantics_for_land_type(LandType::Railroad.as_index())
            .expect("railroad semantics");
        assert_eq!(railroad.cost_for_speed_type(SpeedType::Foot), Some(90));
    }

    #[test]
    fn terrain_rules_preserve_foot_when_track_is_zero() {
        let ini = IniFile::from_str("[Rock]\nTrack=0%\nFoot=50%\n");
        let terrain_rules = TerrainRules::from_ini(&ini);
        let rock = terrain_rules
            .semantics_for_land_type(LandType::Rock.as_index())
            .expect("rock semantics");
        assert_eq!(rock.cost_for_speed_type(SpeedType::Track), Some(0));
        assert_eq!(rock.cost_for_speed_type(SpeedType::Foot), Some(50));
    }

    #[test]
    fn gsi_04_04_reads_exactly_twelve_canonical_rows_with_native_defaults() {
        let mut text = String::new();
        for land_type in LandType::ALL {
            text.push_str(&format!(
                "[{}]\nFoot=0%\nTrack=125%\n{}",
                land_type.section_name(),
                if land_type == LandType::Clear {
                    ""
                } else {
                    "Buildable=yes\n"
                },
            ));
        }
        text.push_str("[Cliff]\nFoot=25%\nBuildable=yes\n");

        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(&text));
        for land_type in LandType::ALL {
            let row = terrain_rules
                .semantics_for_land_type(land_type.as_index())
                .unwrap_or_else(|| panic!("missing {} row", land_type.section_name()));
            assert_eq!(row.cost_for_speed_type(SpeedType::Foot), Some(0));
            assert_eq!(row.cost_for_speed_type(SpeedType::Track), Some(100));
            assert_eq!(row.cost_for_speed_type(SpeedType::Wheel), Some(100));
            assert_eq!(row.cost_for_speed_type(SpeedType::Hover), Some(100));
            assert_eq!(row.cost_for_speed_type(SpeedType::Winged), Some(100));
            assert_eq!(row.cost_for_speed_type(SpeedType::Float), Some(100));
            assert_eq!(row.cost_for_speed_type(SpeedType::Amphibious), Some(100));
            assert_eq!(row.cost_for_speed_type(SpeedType::FloatBeach), Some(100));
            assert_eq!(row.buildable, land_type != LandType::Clear);
        }

        let clear = terrain_rules
            .semantics_for_land_type(LandType::Clear.as_index())
            .expect("clear row");
        assert_eq!(
            clear.speed_costs.speed_multiplier_for(SpeedType::Foot),
            SimFixed::from_num(0),
        );
    }
}
