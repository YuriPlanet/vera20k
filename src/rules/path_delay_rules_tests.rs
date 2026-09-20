//! Bounded comparisons with original Rules readers and eleven timer conversion
//! sites. See tools/spatial_oracle/path_delay_rules.{py,json,meta.json}.
//! These tests do not establish full locomotor response or timer-store parity.

use super::ini_parser::{IniFile, IniSection};
use super::ruleset::{GeneralRules, RuleSet};
use serde::Deserialize;

#[derive(Deserialize)]
struct Defaults {
    path_bits: String,
    blockage: i32,
}

#[derive(Deserialize)]
struct PathRow {
    raw: Option<String>,
    initial_bits: String,
    output_bits: String,
    ticks: i32,
}

#[derive(Deserialize)]
struct BlockageRow {
    raw: Option<String>,
    initial: i32,
    output: i32,
}

#[derive(Deserialize)]
struct Conversion {
    bits: String,
    ticks: i32,
}

#[derive(Deserialize)]
struct Corpus {
    defaults: Defaults,
    path_delay: Vec<PathRow>,
    blockage_path_delay: Vec<BlockageRow>,
    conversion_sites: Vec<String>,
    conversions: Vec<Conversion>,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/path_delay_rules.json"
    ))
    .unwrap()
}

fn bits(value: &str) -> u64 {
    u64::from_str_radix(value, 16).unwrap()
}

fn malformed_float(raw: Option<&str>) -> bool {
    matches!(raw, Some("" | "nan" | "inf" | "junk"))
}

#[test]
fn constructor_delays_match_original_rules_fields() {
    let native = corpus().defaults;
    let defaults = GeneralRules::default();
    assert_eq!(defaults.path_delay.to_bits(), bits(&native.path_bits));
    assert_eq!(defaults.blockage_path_delay_ticks, native.blockage);
    for ini in [
        "",
        "[General]\nFixtureOnly=1\n",
        "[AI]\nFixtureOnly=1\n",
        "[General]\nPathDelay=5\nBlockagePathDelay=777\n",
    ] {
        let rules = RuleSet::from_ini(&IniFile::from_str(ini)).unwrap();
        assert_eq!(rules.general.path_delay.to_bits(), bits(&native.path_bits));
        assert_eq!(rules.general.blockage_path_delay_ticks, native.blockage);
    }
}

#[test]
fn actual_ai_ini_values_match_original_readers_and_delay_conversion() {
    let native = corpus();
    for general in ["", "[General]\nPathDelay=5\nBlockagePathDelay=777\n"] {
        for row in &native.path_delay {
            let Some(raw) = row.raw.as_deref() else {
                continue; // Constructor defaults and retained absent reads have separate tests.
            };
            if malformed_float(Some(raw)) {
                continue; // Explicit caller-ABI boundary tested below; no native parity claim.
            }
            let ini = format!("{general}[AI]\nPathDelay={raw}\n");
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            assert_eq!(
                rules.general.path_delay.to_bits(),
                bits(&row.output_bits),
                "PathDelay={raw:?}, General present={}",
                !general.is_empty()
            );
            assert_eq!(rules.general.path_delay_ticks(), row.ticks, "{raw:?}");
        }
        for row in &native.blockage_path_delay {
            let Some(raw) = row.raw.as_deref().filter(|raw| !raw.is_empty()) else {
                continue; // Physical empty entries are not inserted by the INI loader.
            };
            let ini = format!("{general}[AI]\nBlockagePathDelay={raw}\n");
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            assert_eq!(
                rules.general.blockage_path_delay_ticks, row.output,
                "{raw:?}"
            );
        }
    }
}

#[test]
fn retained_scalar_defaults_and_signed_blockage_match_native_rows() {
    let native = corpus();
    for row in native.path_delay {
        if malformed_float(row.raw.as_deref()) {
            continue;
        }
        let mut section = IniSection::new("AI".to_string());
        if let Some(raw) = &row.raw {
            section.set("PathDelay", raw);
        }
        let actual = section.read_double("PathDelay", f64::from_bits(bits(&row.initial_bits)));
        assert_eq!(actual.to_bits(), bits(&row.output_bits), "{:?}", row.raw);
    }
    for row in native.blockage_path_delay {
        let mut section = IniSection::new("AI".to_string());
        if let Some(raw) = &row.raw {
            section.set("BlockagePathDelay", raw);
        }
        assert_eq!(
            section.read_int("BlockagePathDelay", row.initial),
            row.output,
            "{:?}, prior={}",
            row.raw,
            row.initial
        );
    }
}

#[test]
fn retained_double_conversion_matches_all_eleven_original_sites() {
    let native = corpus();
    // The native generator executes every site per row and refuses a corpus
    // whose eleven outputs differ. This list pins the evidence's caller scope.
    assert_eq!(
        native.conversion_sites,
        [
            "004B2850", "004B3A65", "004D4022", "00516696", "005168DC", "005B0286", "005B0CAE",
            "006A1EA0", "006A30B4", "0075AF6F", "0075B98C",
        ]
    );
    for (raw_bits, ticks) in native
        .conversions
        .iter()
        .map(|row| (&row.bits, row.ticks))
        .chain(
            native
                .path_delay
                .iter()
                .map(|row| (&row.output_bits, row.ticks)),
        )
    {
        let rules = GeneralRules {
            path_delay: f64::from_bits(bits(raw_bits)),
            ..GeneralRules::default()
        };
        assert_eq!(rules.path_delay_ticks(), ticks, "raw double={raw_bits}");
    }
}

#[test]
fn malformed_scalar_and_physical_empty_boundaries_are_explicit() {
    let native = corpus();
    for row in native.path_delay {
        if !malformed_float(row.raw.as_deref()) {
            continue;
        }
        let raw = row.raw.as_deref().unwrap();
        // ReadDouble52854D passes the section-argument slot to sscanf. Failed
        // assignment leaves the native AI pointer bits there. Rust's documented
        // shared-reader policy returns zero, without importing process addresses.
        assert_eq!(bits(&row.output_bits), 0x3810_73b4_8000_0000);
        let mut section = IniSection::new("AI".to_string());
        section.set("PathDelay", raw);
        assert_eq!(section.read_double("PathDelay", 0.016).to_bits(), 0);
        if !raw.is_empty() {
            let rules =
                RuleSet::from_ini(&IniFile::from_str(&format!("[AI]\nPathDelay={raw}\n"))).unwrap();
            assert_eq!(rules.general.path_delay.to_bits(), 0);
            assert_eq!(rules.general.path_delay_ticks(), 0);
        }
    }
    let rules =
        RuleSet::from_ini(&IniFile::from_str("[AI]\nPathDelay=\nBlockagePathDelay=\n")).unwrap();
    assert_eq!(
        rules.general.path_delay.to_bits(),
        bits(&native.defaults.path_bits)
    );
    assert_eq!(
        rules.general.blockage_path_delay_ticks,
        native.defaults.blockage
    );
}
