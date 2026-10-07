//! CRT `atoi`/`strtok` and ReadString/ReadInt/ReadBool against the shared
//! readers. Native rows: tools/rules_oracle/ini_token_readers.{py,json,meta.json}.

use super::ini_parser::IniSection;
use super::ini_value::{crt_atoi, strtok};

#[derive(Debug, serde::Deserialize)]
struct Native {
    atoi: Vec<AtoiRow>,
    strtok: Vec<StrtokRow>,
    read_string: Vec<ReadStringRow>,
    read_int: Vec<ReadIntRow>,
    read_bool: Vec<ReadBoolRow>,
}

#[derive(Debug, serde::Deserialize)]
struct AtoiRow {
    raw: String,
    output: i32,
}

#[derive(Debug, serde::Deserialize)]
struct StrtokRow {
    raw: String,
    tokens: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ReadStringRow {
    raw: Option<String>,
    default: String,
    capacity: usize,
    output: String,
    /// ReadString's return value: the copied length.
    length: usize,
}

#[derive(Debug, serde::Deserialize)]
struct ReadIntRow {
    raw: Option<String>,
    default: i32,
    output: i32,
}

#[derive(Debug, serde::Deserialize)]
struct ReadBoolRow {
    raw: Option<String>,
    default: bool,
    output: bool,
}

fn native() -> Native {
    serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/ini_token_readers.json",
    ))
    .unwrap()
}

/// The oracle's cached entry holds the value exactly as given, as `set` does.
fn section(raw: Option<&str>) -> IniSection {
    let mut section = IniSection::new("TEST".to_string());
    if let Some(raw) = raw {
        section.set("Key", raw);
    }
    section
}

#[test]
fn crt_atoi_matches_original() {
    for row in native().atoi {
        assert_eq!(crt_atoi(&row.raw), row.output, "{row:?}");
    }
}

#[test]
fn strtok_matches_original() {
    for row in native().strtok {
        assert_eq!(
            strtok(&row.raw, &[',']).collect::<Vec<_>>(),
            row.tokens,
            "{row:?}"
        );
    }
}

#[test]
fn read_string_matches_original() {
    for row in native().read_string {
        let section = section(row.raw.as_deref());
        let output = section.read_string("Key", &row.default, row.capacity);
        assert_eq!(output, row.output, "{row:?}");
        // Each Latin-1 value byte is one char.
        assert_eq!(output.chars().count(), row.length, "{row:?}");
    }
}

/// `read_name` is `if (ReadString(key, "", ...))`: `None` exactly where the
/// empty-default copy returns 0.
#[test]
fn read_name_follows_the_read_string_return_value() {
    let mut empty_copies = 0;
    for row in native().read_string {
        if row.raw.is_none() && !row.default.is_empty() {
            continue;
        }
        let section = section(row.raw.as_deref());
        let expected = (row.length != 0).then_some(row.output.as_str());
        assert_eq!(section.read_name("Key", row.capacity), expected, "{row:?}");
        empty_copies += usize::from(row.length == 0);
    }
    assert_eq!(empty_copies, 7);
}

#[test]
fn read_int_matches_original() {
    for row in native().read_int {
        let section = section(row.raw.as_deref());
        assert_eq!(section.read_int("Key", row.default), row.output, "{row:?}");
    }
}

#[test]
fn read_bool_matches_original() {
    for row in native().read_bool {
        let section = section(row.raw.as_deref());
        assert_eq!(section.read_bool("Key", row.default), row.output, "{row:?}");
    }
}
