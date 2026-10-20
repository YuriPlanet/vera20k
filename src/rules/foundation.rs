//! Building foundation name table used by the original game.
//!
//! `Foundation=` is not a free-form `WxH` parser in gamemd.exe. INI values are
//! resolved through this fixed, case-insensitive table; unknown values fall back
//! to table entry 0 (`1x1`).

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
    let def = foundation_def(value);
    (def.width, def.height)
}

/// Shared foundation-table span in world leptons. BuildingType Dimension2
/// @0x00464AF0 and XY-only consumers of cached type facts use this conversion.
pub fn foundation_dimensions_leptons(value: &str) -> [i32; 2] {
    let (width, height) = foundation_dimensions(value);
    [i32::from(width) << 8, i32::from(height) << 8]
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
    let (width, height) = foundation_dimensions(value);
    (
        cursor.0.wrapping_sub(width.saturating_sub(1) / 2),
        cursor.1.wrapping_sub(height.saturating_sub(1) / 2),
    )
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
