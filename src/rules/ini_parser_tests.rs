//! Unit tests for the INI parser (IniFile + IniSection).

use super::*;
use crate::util::save_codec;

#[test]
fn saved_standalone_reader_body_rejects_inconsistent_indexes() {
    let ini = IniFile::from_str("[E1]\nCost=100\nStrength=125\n");
    let section = ini.section("E1").unwrap();
    let bytes = save_codec::serialize(section).unwrap();
    assert_eq!(
        save_codec::deserialize::<IniSection>(&bytes).unwrap(),
        *section
    );

    let mut duplicate_order = section.clone();
    duplicate_order.key_order[1] = duplicate_order.key_order[0].clone();
    let mut missing_order = section.clone();
    missing_order.key_order.pop();
    let mut orphan_history = section.clone();
    orphan_history
        .projected_values
        .insert("Absent".into(), vec!["1".into()]);
    for invalid in [duplicate_order, missing_order, orphan_history] {
        let bytes = save_codec::serialize(&invalid).unwrap();
        let error = save_codec::deserialize::<IniSection>(&bytes).unwrap_err();
        assert!(error.to_string().contains("invalid saved INI key order"));
    }
}

#[test]
fn test_raw_lookup_is_case_sensitive() {
    let ini: IniFile = IniFile::from_str("[VIRUS]\nName=Infantry\n[Virus]\nName=Warhead\n");

    assert_eq!(ini.section("VIRUS").unwrap().get("Name"), Some("Infantry"));
    assert_eq!(ini.section("Virus").unwrap().get("Name"), Some("Warhead"));
    assert!(ini.section("virus").is_none());
    assert!(ini.section("VIRUS").unwrap().get("name").is_none());
}

#[test]
fn section_header_preserves_spaces_inside_brackets() {
    let ini = IniFile::from_str("  [ Name ]  \nKey=Value\n");

    assert!(ini.section("Name").is_none());
    assert_eq!(ini.section(" Name ").unwrap().get("Key"), Some("Value"));
}

#[test]
fn empty_section_name_is_retained() {
    let ini = IniFile::from_str("[]\nKey=Value\n");

    assert_eq!(ini.section("").unwrap().get("Key"), Some("Value"));
}

#[test]
fn entryless_physical_sections_are_discarded() {
    let ini = IniFile::from_str(
        "[HeaderOnly]\n[CommentOnly]\n; no accepted entries\n\
         [EmptyValueOnly]\nKey=\n[Kept]\nKey=Value\n",
    );

    assert!(ini.section("HeaderOnly").is_none());
    assert!(ini.section("CommentOnly").is_none());
    assert!(ini.section("EmptyValueOnly").is_none());
    assert_eq!(ini.section_names(), vec!["Kept"]);
}

#[test]
fn retail_parabomb_empty_body_does_not_hide_later_definition() {
    // ARTMD has an entryless PARABOMB occurrence before this populated body.
    let ini = IniFile::from_str("[PARABOMB]\n\n[PARABOMB]\nRate=200\nLoopStart=7\nLoopCount=15\n");

    let parabomb = ini.section("PARABOMB").expect("populated PARABOMB body");
    assert_eq!(parabomb.get("Rate"), Some("200"));
    assert_eq!(parabomb.get("LoopStart"), Some("7"));
    assert_eq!(parabomb.get("LoopCount"), Some("15"));
}

#[test]
fn malformed_header_falls_through_to_key_value_parsing() {
    let ini = IniFile::from_str("[S]\n[foo=bar\n");

    assert_eq!(ini.section("S").unwrap().get("[foo"), Some("bar"));
}

#[test]
fn test_comments_and_blank_lines() {
    let text: &str = "\
; This is a comment
# This is ordinary junk

[Section1]
Key1=Value1
#Key=Visible

; Another comment
Key2=Value2
";
    let ini: IniFile = IniFile::from_str(text);

    assert_eq!(ini.section_count(), 1);
    let section: &IniSection = ini.section("Section1").unwrap();
    assert_eq!(section.get("Key1"), Some("Value1"));
    assert_eq!(section.get("Key2"), Some("Value2"));
    assert_eq!(section.get("#Key"), Some("Visible"));
    assert_eq!(section.entry_count(), 3);
}

#[test]
fn duplicate_nonempty_section_bodies_are_retained_in_source_order() {
    let text: &str = "\
[General]
Key1=First
Key2=Original

[General]
Key2=Override
Key3=New
";
    let ini: IniFile = IniFile::from_str(text);

    assert_eq!(ini.section_count(), 2);

    assert_eq!(ini.section_names(), vec!["General", "General"]);
    assert_eq!(ini.sections[0].get("Key1"), Some("First"));
    assert_eq!(ini.sections[1].get("Key3"), Some("New"));
}

#[test]
fn omitted_fixture_key_preserves_source_order_case_and_reader_defaults() {
    let mut ini = IniFile::from_str(
        "[Before]\nKeep=before\n\
         [GAWEAP]\nKeep=first\nWeaponsFactory=yes\nweaponsfactory=no\n\
         [GAWEAP]\nWeaponsFactory=no\n\
         [GAWEAP]\nKeep=last\nWeaponsFactory=yes\n\
         [Gaweap]\nWeaponsFactory=yes\n\
         [After]\nKeep=after\n",
    );
    ini.merge_rules_projection(&IniFile::from_str("[GAWEAP]\nWeaponsFactory=yes\n"));
    ini.merge_rules_projection(&IniFile::from_str("[GAWEAP]\nWeaponsFactory=invalid\n"));
    assert!(
        ini.section("GAWEAP")
            .unwrap()
            .read_bool("WeaponsFactory", false)
    );
    let original_sections = ini.sections.clone();
    let original_lookup = ini.first_section.clone();

    let omitted = ini.without_entry_for_test("GAWEAP", "WeaponsFactory");

    assert_eq!(ini.sections, original_sections);
    assert_eq!(ini.first_section, original_lookup);
    assert_eq!(
        omitted.section_names(),
        vec!["Before", "GAWEAP", "GAWEAP", "Gaweap", "After"]
    );
    let first = omitted.section("GAWEAP").unwrap();
    assert_eq!(
        first.keys().collect::<Vec<_>>(),
        vec!["Keep", "weaponsfactory"]
    );
    assert_eq!(first.get("Keep"), Some("first"));
    assert!(!first.read_bool("WeaponsFactory", false));
    assert_eq!(first.get("weaponsfactory"), Some("no"));
    let last = &omitted.sections[2];
    assert_eq!(
        last.raw_entries().collect::<Vec<_>>(),
        vec![("Keep", "last")]
    );
    assert!(
        omitted
            .section("Gaweap")
            .unwrap()
            .read_bool("WeaponsFactory", false)
    );
    assert_eq!(omitted.section("After").unwrap().get("Keep"), Some("after"));
}

#[test]
fn fresh_duplicate_lookup_keeps_physical_bytes_and_projection_copy_separate() {
    // FindEntryByNames526B10 -> signed CRC qsort52B6A0 and midpoint52B6D0.
    // The original physical ROOT/ROOT/SHOW control selects the second ROOT;
    // SaveToPipe526470 still writes both original values, in physical order.
    let ini = IniFile::from_str("[Triggers]\nROOT=First\nROOT=Second\nSHOW=Show\n");
    let section = ini.section("Triggers").unwrap();
    assert_eq!(section.read_string("ROOT", "", 32), "Second");
    assert_eq!(
        ini.save_to_pipe(None),
        b"[Triggers]\r\nROOT=First\r\nROOT=Second\r\nSHOW=Show\r\n"
    );

    // Include copies remain the existing compatibility policy; an ordered
    // Rules pass instead projects the value the physical native reader saw.
    let mut included = IniFile::empty();
    included.merge(&ini);
    assert_eq!(
        included
            .section("Triggers")
            .unwrap()
            .read_string("ROOT", "", 32),
        "First"
    );
    let mut projected = IniFile::empty();
    projected.merge_rules_projection(&ini);
    assert_eq!(
        projected
            .section("Triggers")
            .unwrap()
            .read_string("ROOT", "", 32),
        "Second"
    );
}

#[test]
fn semicolon_truncates_before_equals_and_empty_entries_are_omitted() {
    let ini = IniFile::from_str(
        "[S]\nIgnored;Key=Value\n;Comment=Value\n=NoKey\nNoValue=\nGood=Yes;Comment\n",
    );
    let section = ini.section("S").unwrap();
    assert_eq!(section.entry_count(), 1);
    assert_eq!(section.get("Good"), Some("Yes"));
}

#[test]
fn nul_terminates_the_visible_line_but_not_the_physical_read() {
    let ini = IniFile::from_str("[S]\nGood=Yes\0Injected=No\nAfter=Seen\n");
    let section = ini.section("S").unwrap();

    assert_eq!(section.get("Good"), Some("Yes"));
    assert!(section.get("Injected").is_none());
    assert_eq!(section.get("After"), Some("Seen"));
}

#[test]
fn overlong_physical_line_discards_everything_after_511_bytes() {
    let text = format!("[S]\nA={}Injected=Yes\n", "x".repeat(509));
    let ini = IniFile::from_str(&text);
    let section = ini.section("S").unwrap();

    assert_eq!(section.get("A").unwrap().len(), 509);
    assert!(section.get("Injected").is_none());
}

#[test]
fn crlf_lines_and_widened_bytes_follow_the_same_read_line_rules() {
    let mut data = b"[S]\r\nK\re\ry=V\ra\rl\rue\r\nShort=".to_vec();
    // 300 widened bytes take 600 UTF-8 bytes but only 300 of the 511 places.
    data.extend([0xE9; 300]);
    data.extend(b"\r\nLong=");
    data.extend([0xE9; 600]);
    data.extend(b"\r\n");
    let ini = IniFile::from_bytes(&data).unwrap();
    let section = ini.section("S").unwrap();

    assert_eq!(section.get("Key"), Some("Value"));
    assert_eq!(section.get("Short").unwrap(), "\u{e9}".repeat(300));
    assert_eq!(
        section.get("Long").unwrap(),
        "\u{e9}".repeat(511 - "Long=".len())
    );
}

#[test]
fn test_registry_ids_with_numeric_gaps() {
    let ini: IniFile =
        IniFile::from_str("[VehicleTypes]\n36=CMIN\n1=HTNK\n40=HARV\n2=MTNK\n5=SMIN\n");
    let section: &IniSection = ini.section("VehicleTypes").unwrap();
    let values: Vec<&str> = section.registry_ids();
    assert_eq!(values, vec!["CMIN", "HTNK", "HARV", "MTNK", "SMIN"]);
}

#[test]
fn test_registry_ids_reads_named_entries_too() {
    let ini: IniFile = IniFile::from_str("[Empty]\nName=Test\n");
    let section: &IniSection = ini.section("Empty").unwrap();
    let values: Vec<&str> = section.registry_ids();
    assert_eq!(values, vec!["Test"]);
}

/// `content_hash` is deterministic and sensitive to every value — a scalar
/// override changes it (the gap that left a registry-only rules hash blind to
/// map value overrides), while comment/whitespace-only differences do not.
#[test]
fn content_hash_is_deterministic_and_value_sensitive() {
    let a = IniFile::from_str("[General]\nBuildSpeed=.7\nFlightLevel=1500\n");
    // Same content parsed twice → identical hash (no HashMap-order drift).
    let a2 = IniFile::from_str("[General]\nBuildSpeed=.7\nFlightLevel=1500\n");
    assert_eq!(a.content_hash(), a2.content_hash());

    // One scalar value differs → hash differs.
    let b = IniFile::from_str("[General]\nBuildSpeed=.58\nFlightLevel=1500\n");
    assert_ne!(a.content_hash(), b.content_hash());

    // Comments and surrounding whitespace are stripped at parse → no effect.
    let c = IniFile::from_str("; header\n[General]\nBuildSpeed = .7   ; speed\nFlightLevel=1500\n");
    assert_eq!(a.content_hash(), c.content_hash());
}

/// Retail AIMD.INI carries `[Digest] 1=` (base64 SHA-1), which
/// `ReadCCFile(file, 1, 0)` compares at `0x004742FA` against SHA-1 of the
/// `SaveToPipe` bytes without `[Digest]`. Retail bytes, not prior Rust,
/// fix the serialization of a file without padding or comments.
#[test]
fn aimd_digest_matches_its_save_to_pipe_bytes() {
    let Some(ai) = crate::rules::retail_ini_fixture::retail_ini("aimd.ini") else {
        return;
    };
    let stored = ai.section_or_empty("Digest").read_string("1", "", 0x40);
    let digest = crate::util::sha1::sha1_bytes(&ai.save_to_pipe(Some("Digest")));
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in digest.chunks(3) {
        let bits = chunk.iter().enumerate().fold(0_u32, |bits, (index, byte)| {
            bits | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            encoded.push(if index <= chunk.len() {
                char::from(ALPHABET[(bits >> (18 - 6 * index) & 63) as usize])
            } else {
                '='
            });
        }
    }
    assert_eq!(encoded, stored);
}

/// `SaveToPipe` streams captured from native `GetCRC` on the synthetic
/// controls of tools/storage_oracle/ini_crc: duplicate keys and sections,
/// empty sections, spacing, comments and an unterminated last line.
#[test]
fn save_to_pipe_matches_the_native_streams() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/ini_crc.json",
    ))
    .unwrap();
    for case in corpus["synthetic"].as_array().unwrap() {
        if case["absent"].as_bool() == Some(true) {
            continue;
        }
        let input = case["input"].as_str().unwrap();
        let ini = IniFile::from_bytes(&crate::util::native_string::narrow_widened(input)).unwrap();
        assert_eq!(
            crate::util::native_string::widen_bytes(&ini.save_to_pipe(None)),
            case["stream"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}
