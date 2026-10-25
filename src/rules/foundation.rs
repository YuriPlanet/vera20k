//! Building foundation name table used by the original game.
//!
//! `Foundation=` is not a free-form `WxH` parser in gamemd.exe. INI values are
//! resolved through this fixed, case-insensitive table; unknown values fall back
//! to table entry 0 (`1x1`).

/// Resolved BuildingType foundation. The stock name is an input token, not a
/// free-form size. Ares owns declared dimensions, occupied cells and exit cells
/// independently; one resolved value follows the type into its existing cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Foundation {
    Stock(u8),
    #[cfg(feature = "ares")]
    Custom(CustomFoundation),
    /// A configuration the current 16-bit consumer interface cannot admit.
    /// Retained only until the process-registry validation rejects the load.
    #[cfg(feature = "ares")]
    InvalidCustom,
}

#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CustomFoundation {
    width: i32,
    height: i32,
    cells: Vec<(i16, i16)>,
    outline: Vec<(i16, i16)>,
}

impl Default for Foundation {
    fn default() -> Self {
        Self::Stock(DEFAULT_FOUNDATION_ID)
    }
}

impl From<&str> for Foundation {
    fn from(value: &str) -> Self {
        Self::Stock(foundation_id(value))
    }
}

impl From<String> for Foundation {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl std::fmt::Display for Foundation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl PartialEq<&str> for Foundation {
    fn eq(&self, other: &&str) -> bool {
        self.name() == *other
    }
}

impl Foundation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Stock(id) => FOUNDATION_TABLE[usize::from(*id)].name,
            #[cfg(feature = "ares")]
            Self::Custom(_) => "Custom",
            #[cfg(feature = "ares")]
            Self::InvalidCustom => "Custom",
        }
    }

    pub(crate) fn selector(&self) -> u8 {
        match self {
            Self::Stock(id) => *id,
            #[cfg(feature = "ares")]
            Self::Custom(_) => 127,
            #[cfg(feature = "ares")]
            Self::InvalidCustom => 127,
        }
    }

    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Stock(id) => usize::from(*id) < FOUNDATION_TABLE.len(),
            #[cfg(feature = "ares")]
            Self::Custom(custom) => {
                u16::try_from(custom.width).is_ok()
                    && u16::try_from(custom.height).is_ok()
                    && custom.width.checked_mul(custom.height).is_some()
                    && custom.outline.len() >= 11
                    && custom.outline.last() == Some(&(0x7fff, 0x7fff))
                    && custom.cells.windows(2).all(|pair| pair[0] < pair[1])
                    && !custom.cells.contains(&(0x7fff, 0x7fff))
            }
            #[cfg(feature = "ares")]
            Self::InvalidCustom => false,
        }
    }

    pub fn dimensions(&self) -> (u16, u16) {
        match self {
            Self::Stock(id) => {
                let def = &FOUNDATION_TABLE[usize::from(*id)];
                (def.width, def.height)
            }
            #[cfg(feature = "ares")]
            Self::Custom(custom) => (
                u16::try_from(custom.width).expect("validated custom foundation width"),
                u16::try_from(custom.height).expect("validated custom foundation height"),
            ),
            #[cfg(feature = "ares")]
            Self::InvalidCustom => unreachable!("invalid foundation rejected before projection"),
        }
    }

    pub fn dimensions_leptons(&self) -> [i32; 2] {
        let (width, height) = self.dimensions();
        [i32::from(width) << 8, i32::from(height) << 8]
    }

    ///45ECA0 / installed Ares100152D0: the extra row is queried only when
    /// both the caller and Bib request it. Geometry itself excludes the bib.
    pub fn height_with_bib(&self, include_bib: bool, bib: bool) -> i32 {
        i32::from(self.dimensions().1) + i32::from(include_bib && bib)
    }

    /// Type+DFC, returned by45EC20. Custom cells have the executed Ares
    /// 1000DF78 sort/unique order (X first, then Y); stock rows are unchanged.
    pub fn cell_offsets(&self) -> Vec<(i16, i16)> {
        match self {
            Self::Stock(_) => foundation_cell_offsets(self.name()),
            #[cfg(feature = "ares")]
            Self::Custom(custom) => custom.cells.clone(),
            #[cfg(feature = "ares")]
            Self::InvalidCustom => unreachable!("invalid foundation rejected before projection"),
        }
    }

    pub(crate) fn custom_outline(&self) -> Option<&[(i16, i16)]> {
        match self {
            #[cfg(feature = "ares")]
            Self::Custom(custom) => Some(&custom.outline),
            _ => None,
        }
    }

    ///4A94F0 / Ares10011980 and6D50FB: cursor bounds come from the actual
    /// terminated cell list, independently of declared width/height.
    pub fn cell_bounds(&self) -> (i16, i16, i16, i16) {
        let cells = self.cell_offsets();
        if cells.is_empty() {
            return (0, 0, 0, 0);
        }
        cells.iter().fold(
            (512, 512, -512, -512),
            |(min_x, min_y, max_x, max_y), &(x, y)| {
                (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
            },
        )
    }

    pub fn placement_origin_at_cursor(&self, cursor: (u16, u16)) -> (u16, u16) {
        let (min_x, min_y, max_x, max_y) = self.cell_bounds();
        let offset = |min: i16, max: i16| {
            //4A94F0 returns the span only;4A8C9E does not compensate for a
            // nonzero minimum. DEC/NEG are word stores, then signed /2.
            let span = (i32::from(max) - i32::from(min) + 1).max(1) as i16;
            span.wrapping_sub(1).wrapping_neg() / 2
        };
        (
            cursor.0.wrapping_add_signed(offset(min_x, max_x)),
            cursor.1.wrapping_add_signed(offset(min_y, max_y)),
        )
    }

    /// The original ART dual-read precedes the Ares suffix. Once a custom
    /// foundation is installed, that suffix reinstalls the same geometry on
    /// every pass; the intervening stock selector is no longer consumed.
    pub(crate) fn read_stock_pass(
        &mut self,
        image: &str,
        type_id: &str,
        mut read: impl FnMut(&str, u8) -> u8,
    ) {
        match self {
            Self::Stock(id) => *id = read_building_foundation(*id, image, type_id, &mut read),
            #[cfg(feature = "ares")]
            _ => {}
        }
    }

    /// BuildingType ReadINI suffix464A49/464A56 ->1000DD40. Fixed ART
    /// [effective Image], once per extension; never a layered RULES key.
    /// Native execution: tools/rules_oracle/ares_foundation.json.
    #[cfg(feature = "ares")]
    pub(crate) fn read_custom_pass(&mut self, art: &crate::rules::ini_parser::IniSection) {
        if matches!(self, Self::Custom(_) | Self::InvalidCustom)
            || !art
                .read_string("Foundation", "", 0x800)
                .eq_ignore_ascii_case("Custom")
        {
            return;
        }
        let width = art.read_int("Foundation.X", 0);
        let height = art.read_int("Foundation.Y", 0);
        let outline_len = art.read_int("FoundationOutline.Length", 0).max(10);
        // The world uses 16-bit foundation dimensions. Reject unsupported
        // resource sizes at loading, rather than changing native values. Ares
        // itself would require a contiguous outline allocation; cap that input
        // before a hostile resource can request multi-GB memory/work here.
        if u16::try_from(width).is_err()
            || u16::try_from(height).is_err()
            || width.checked_mul(height).is_none()
            || outline_len > 1_048_576
        {
            *self = Self::InvalidCustom;
            return;
        }
        let mut cells = Vec::new();
        // Invalid dimensions remain invalid state until registry validation
        // rejects the resource; no positive clamp or integer truncation.
        if u16::try_from(width).is_ok() && u16::try_from(height).is_ok() {
            for i in 0..width.checked_mul(height).unwrap_or(0) {
                let key = format!("Foundation.{i}");
                let Some([x, y]) = art.read_int_fields_present(&key, 0x800, [0, 0]) else {
                    break;
                };
                cells.push((x as i16, y as i16));
            }
        }
        cells.sort_unstable();
        cells.dedup();
        cells.truncate(
            cells
                .iter()
                .position(|&p| p == (0x7fff, 0x7fff))
                .unwrap_or(cells.len()),
        );
        let mut outline = Vec::new();
        for i in 0..outline_len {
            let key = format!("FoundationOutline.{i}");
            let pair = art
                .read_int_fields_present(&key, 0x800, [0, 0])
                .map_or((0x7fff, 0x7fff), |[x, y]| (x as i16, y as i16));
            outline.push(pair);
        }
        outline.push((0x7fff, 0x7fff));
        *self = Self::Custom(CustomFoundation {
            width,
            height,
            cells,
            outline,
        });
    }
}

#[cfg(all(test, feature = "ares"))]
#[path = "foundation_custom_tests.rs"]
pub(crate) mod custom_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundationDef {
    pub id: u8,
    pub name: &'static str,
    pub width: u16,
    pub height: u16,
}

pub const DEFAULT_FOUNDATION_ID: u8 = 0;

pub const FOUNDATION_TABLE: [FoundationDef; 22] = [
    FoundationDef {
        id: 0,
        name: "1x1",
        width: 1,
        height: 1,
    },
    FoundationDef {
        id: 1,
        name: "2x1",
        width: 2,
        height: 1,
    },
    FoundationDef {
        id: 2,
        name: "1x2",
        width: 1,
        height: 2,
    },
    FoundationDef {
        id: 3,
        name: "2x2",
        width: 2,
        height: 2,
    },
    FoundationDef {
        id: 4,
        name: "2x3",
        width: 2,
        height: 3,
    },
    FoundationDef {
        id: 5,
        name: "3x2",
        width: 3,
        height: 2,
    },
    FoundationDef {
        id: 6,
        name: "3x3",
        width: 3,
        height: 3,
    },
    FoundationDef {
        id: 7,
        name: "3x5",
        width: 3,
        height: 5,
    },
    FoundationDef {
        id: 8,
        name: "4x2",
        width: 4,
        height: 2,
    },
    FoundationDef {
        id: 9,
        name: "3x3Refinery",
        width: 3,
        height: 3,
    },
    FoundationDef {
        id: 10,
        name: "1x3",
        width: 1,
        height: 3,
    },
    FoundationDef {
        id: 11,
        name: "3x1",
        width: 3,
        height: 1,
    },
    FoundationDef {
        id: 12,
        name: "4x3",
        width: 4,
        height: 3,
    },
    FoundationDef {
        id: 13,
        name: "1x4",
        width: 1,
        height: 4,
    },
    FoundationDef {
        id: 14,
        name: "1x5",
        width: 1,
        height: 5,
    },
    FoundationDef {
        id: 15,
        name: "2x6",
        width: 2,
        height: 6,
    },
    FoundationDef {
        id: 16,
        name: "2x5",
        width: 2,
        height: 5,
    },
    FoundationDef {
        id: 17,
        name: "5x3",
        width: 5,
        height: 3,
    },
    FoundationDef {
        id: 18,
        name: "4x4",
        width: 4,
        height: 4,
    },
    FoundationDef {
        id: 19,
        name: "3x4",
        width: 3,
        height: 4,
    },
    FoundationDef {
        id: 20,
        name: "6x4",
        width: 6,
        height: 4,
    },
    FoundationDef {
        id: 21,
        name: "0x0",
        width: 0,
        height: 0,
    },
];

pub fn foundation_def(value: &str) -> &'static FoundationDef {
    FOUNDATION_TABLE
        .iter()
        .find(|def| def.name.eq_ignore_ascii_case(value))
        .unwrap_or(&FOUNDATION_TABLE[DEFAULT_FOUNDATION_ID as usize])
}

/// Original ReadFoundation474DA0: ReadString528A10 at char[32], using
/// the current table name as default, then a case-insensitive fixed-table scan.
/// A present invalid (including empty) name resolves0. The string reader owns
/// the byte cap and CRT trim; repeating Unicode trim here changes native inputs.
/// Executable comparisons: spatial_oracle/engineer_bridge_cursor_caller.json.
pub(crate) fn read_foundation(section: &crate::rules::ini_parser::IniSection, current: u8) -> u8 {
    foundation_id(&section.read_string(
        "Foundation",
        FOUNDATION_TABLE[usize::from(current)].name,
        0x20,
    ))
}

/// BuildingType461225..46125D reads ART by effective RULES Image and then
/// stored type ID. Both reads receive the current value; the second result
/// replaces the first only when nonzero. The rules pass processor owns this
/// field; asset metadata installation cannot repeat a native ReadINI pass.
pub(crate) fn read_building_foundation(
    current: u8,
    image: &str,
    type_id: &str,
    mut read: impl FnMut(&str, u8) -> u8,
) -> u8 {
    let first = read(image, current);
    let second = read(type_id, first);
    if second == DEFAULT_FOUNDATION_ID {
        first
    } else {
        second
    }
}

pub fn foundation_id(value: &str) -> u8 {
    foundation_def(value).id
}

pub fn foundation_name(value: &str) -> &'static str {
    foundation_def(value).name
}

pub fn foundation_dimensions(value: &str) -> (u16, u16) {
    Foundation::from(value).dimensions()
}

/// Shared foundation-table span in world leptons. BuildingType Dimension2
/// @0x00464AF0 and XY-only consumers of cached type facts use this conversion.
pub fn foundation_dimensions_leptons(value: &str) -> [i32; 2] {
    Foundation::from(value).dimensions_leptons()
}

/// Manual cursor Cell -> foundation origin, in packed 16-bit map coordinates.
/// SetActive4A8C99..4A8CDA obtains list bounds from4A94F0, decrements and
/// negates each signed short, then divides toward zero. The fixed table's
/// bounds match these dimensions; the empty list has native minimum1.
/// Thus4wide means offset-1, not-2. LeftUp4ABA16 and UpdateCenter4A91B0
/// both add that offset with word wrapping. Simulation PLACE already carries
/// an origin and must not apply this cursor conversion again.
/// Native controls: tools/spatial_oracle/placement_render.json centering.
pub fn placement_origin_at_cursor(value: &str, cursor: (u16, u16)) -> (u16, u16) {
    Foundation::from(value).placement_origin_at_cursor(cursor)
}

/// Exact sentinel-list contents for one native foundation, without the final
/// `(0x7FFF, 0x7FFF)` terminator.
///
/// All table entries are row-major rectangles except `3x3Refinery`, whose
/// native list deliberately omits `(2, 1)`. `0x0` has no cell offsets.
pub fn foundation_cell_offsets(value: &str) -> Vec<(i16, i16)> {
    let def = foundation_def(value);
    if def.id == 9 {
        return vec![
            (0, 0),
            (1, 0),
            (2, 0),
            (0, 1),
            (1, 1),
            (0, 2),
            (1, 2),
            (2, 2),
        ];
    }

    let mut offsets = Vec::with_capacity(usize::from(def.width) * usize::from(def.height));
    for dy in 0..def.height {
        for dx in 0..def.width {
            offsets.push((dx as i16, dy as i16));
        }
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "ares"))]
    #[test]
    fn stock_build_has_no_custom_foundation_extension() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::native_processing::RulesLayerStack;
        let rules = IniFile::from_str("[BuildingTypes]\n0=TEST\n[TEST]\nImage=ART\n");
        let art = IniFile::from_str(
            "[ART]\nFoundation=Custom\nFoundation.X=2\n\
            Foundation.Y=4\nFoundation.0=0,0\n",
        );
        let processed = RulesLayerStack::new(rules)
            .process_with_fixed_art(&art)
            .unwrap();
        let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.object("TEST").unwrap().foundation,
            Foundation::default()
        );
    }

    #[test]
    fn manual_cursor_origins_match_original_foundation_bounds_and_word_wrap() {
        let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/placement_render.json",
        ))
        .unwrap();
        let centering = &packet["centering"];
        let controls = centering["controls"].as_array().unwrap();
        let edges = centering["edge_controls"].as_array().unwrap();
        assert_eq!(controls.len(), FOUNDATION_TABLE.len());
        assert_eq!(edges.len(), 5);
        for row in controls.iter().chain(edges) {
            let selector = row["input"]["selector"].as_u64().unwrap();
            let named = controls
                .iter()
                .find(|control| control["input"]["selector"].as_u64() == Some(selector))
                .unwrap();
            let name = named["input"]["name"].as_str().unwrap();
            assert_eq!(u64::from(foundation_id(name)), selector);
            let packed = |value: &serde_json::Value| {
                (
                    value[0].as_i64().unwrap() as u16,
                    value[1].as_i64().unwrap() as u16,
                )
            };
            assert_eq!(
                placement_origin_at_cursor(name, packed(&row["input"]["center"])),
                packed(&row["output"]["marker_origin"]),
                "native selector {selector}, cursor {}",
                row["input"]["center"],
            );
            if let Some(bounds) = row["output"].get("bounds") {
                let def = foundation_def(name);
                assert_eq!((def.width.max(1), def.height.max(1)), packed(bounds));
            }
        }
    }

    #[test]
    fn fixed_table_is_case_insensitive() {
        assert_eq!(foundation_id("3x3refinery"), 9);
        assert_eq!(foundation_dimensions("3x3Refinery"), (3, 3));
    }

    #[test]
    fn unknown_values_fall_back_to_1x1() {
        assert_eq!(foundation_id("7x7"), 0);
        assert_eq!(foundation_dimensions("custom"), (1, 1));
    }

    #[test]
    fn gsi_04_05_hidden_foundation_offsets_match_native_special_lists() {
        assert_eq!(
            foundation_cell_offsets("3x3Refinery"),
            vec![
                (0, 0),
                (1, 0),
                (2, 0),
                (0, 1),
                (1, 1),
                (0, 2),
                (1, 2),
                (2, 2),
            ]
        );
        assert!(foundation_cell_offsets("0x0").is_empty());
        assert_eq!(
            foundation_cell_offsets("2x2"),
            vec![(0, 0), (1, 0), (0, 1), (1, 1)]
        );
    }
}
