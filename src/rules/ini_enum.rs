//! Generic enum-by-name table helper — the gamemd enum round-trip (P10).
//!
//! gamemd's enum readers (Foundation, MovementZone, SpeedType, Layer, Action)
//! ReadString into a fixed buffer (default = the default entry's NAME), then do a
//! WHOLE-STRING case-insensitive compare against a static `{name,id}` table and
//! return the matched id, else the table default id. A substring does NOT match.
//!
//! This generalizes the shape `foundation.rs:foundation_def` already implements
//! correctly. Added this slice but not yet consumed (additive / shadow).
//!
//! ## Dependency rules
//! - rules/ only; no other module dependency. Pure function over a static table.

/// 0x20 = ASCII space; gamemd `strtrim` strips bytes <= 0x20 (space + all ASCII
/// control) at BOTH ends before the enum name compare. ASCII-only by design.
#[cfg(test)]
const STRTRIM_MAX: u32 = 0x20;

/// One name->id row in an enum table.
#[derive(Debug, Clone, Copy)]
pub struct EnumByName {
    pub name: &'static str,
    pub id: i32,
}

/// Resolve `value` against `table` (whole-string, case-insensitive). Returns the
/// matched id, else `default_id`.
///
/// Per-table defaults differ in gamemd (Foundation -> 0 = "1x1";
/// MovementZone -> -1; Action -> 0) — the CALLER passes the right `default_id`;
/// this helper does not bake one in. The trim mirrors the gamemd enum readers,
/// which run strtrim (bytes <= 0x20 both ends) before the `_stricmp` compare.
#[cfg(test)]
pub fn enum_by_name(value: &str, table: &[EnumByName], default_id: i32) -> i32 {
    let trimmed = value.trim_matches(|c: char| (c as u32) <= STRTRIM_MAX);
    table
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(trimmed))
        .map(|e| e.id)
        .unwrap_or(default_id)
}

#[cfg(test)]
mod tests {
    use super::{EnumByName, enum_by_name};

    const FOUNDATION: &[EnumByName] = &[
        EnumByName { name: "1x1", id: 0 },
        EnumByName {
            name: "3x3refinery",
            id: 9,
        },
    ];

    #[test] // P10 whole-string, case-insensitive, table default
    fn test_enum_by_name() {
        assert_eq!(enum_by_name("3x3Refinery", FOUNDATION, 0), 9);
        assert_eq!(enum_by_name("1X1", FOUNDATION, 0), 0);
        assert_eq!(enum_by_name("unknown", FOUNDATION, 0), 0); // miss -> default
        assert_eq!(enum_by_name("3x3", FOUNDATION, 0), 0); // substring NO match
    }
}
