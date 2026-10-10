//! Executed Campaign and Movie reader comparisons. Cache controls explicitly
//! include stored-empty values; physical stock inputs use the production reader.

use serde_json::Value;

use super::campaigns::{CampaignDefinition, CampaignRegistry};
use super::ini_parser::IniFile;
use super::movies::MovieRegistry;
use crate::assets::csf_file::CsfFile;

fn native() -> Value {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/campaign_start.json",
    ))
    .expect("original campaign-start corpus");
    assert_eq!(
        fixture["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    fixture
}

fn cache_ini(sections: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    for (name, entries) in sections.as_object().expect("native cached sections") {
        let section = ini.projection_section_mut(name);
        for (key, value) in entries.as_object().expect("native cached entries") {
            section.set(key, value.as_str().expect("native cached string"));
        }
    }
    ini
}

fn terminated_wide_string(hex: &str) -> String {
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).expect("native UTF16 byte")
        })
        .collect();
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn assert_campaign(actual: &CampaignDefinition, expected: &Value) {
    assert_eq!(actual.id(), expected["id"].as_str().unwrap());
    assert_eq!(actual.cd(), expected["cd"].as_i64().unwrap() as i32);
    assert_eq!(actual.scenario(), expected["scenario"].as_str().unwrap());
    assert_eq!(
        actual.final_movie(),
        expected["final_movie"].as_i64().unwrap() as i32
    );
    assert_eq!(
        actual.description(),
        terminated_wide_string(expected["description_utf16_hex"].as_str().unwrap())
    );
}

fn assert_campaigns(actual: &CampaignRegistry, expected: &Value) {
    let entries = expected.as_array().unwrap();
    for (index, row) in entries.iter().enumerate() {
        assert_campaign(actual.get(index).expect("native campaign entry"), row);
    }
    assert!(
        actual.get(entries.len()).is_none(),
        "no extra campaign entry"
    );
    assert_eq!(actual.is_empty(), entries.is_empty());
}

fn assert_movies(actual: &MovieRegistry, expected: &Value) {
    let names = expected.as_array().unwrap();
    for (index, name) in names.iter().enumerate() {
        assert_eq!(actual.name(index as i32), name.as_str());
    }
    assert_eq!(actual.name(-1), None);
    assert_eq!(actual.name(names.len() as i32), None);
}

#[test]
fn cached_movie_names_match_original_sentinel_duplicate_and_capacity_controls() {
    let native = native();
    for row in native["movies"]["controls"].as_array().unwrap() {
        let registry = MovieRegistry::from_art(&cache_ini(&row["sections"]));
        assert_movies(&registry, &row["catalog"]);
        for lookup in row["lookups"].as_array().unwrap() {
            assert_eq!(
                registry.find_index(lookup["value"].as_str().unwrap_or("")),
                lookup["result"].as_i64().unwrap() as i32,
                "control {} lookup {}",
                row["name"],
                lookup["value"]
            );
        }
    }
}

#[test]
fn physical_art_movie_catalog_and_read_movie_match_original_execution() {
    let Some(bytes) = super::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let native = native();
    assert_eq!(
        crate::util::sha256::sha256_hex(&bytes),
        native["movies"]["artmd_sha256"]
    );
    let art = IniFile::from_bytes(&bytes).unwrap();
    let movies = MovieRegistry::from_art(&art);
    assert_movies(&movies, &native["movies"]["catalog"]);
    for row in native["movies"]["reader_cases"].as_array().unwrap() {
        let mut ini = IniFile::empty();
        let section = ini.projection_section_mut("Control");
        if let Some(value) = row["value"].as_str() {
            section.set("FinalMovie", value);
        }
        assert_eq!(
            section.read_movie(
                "FinalMovie",
                row["default"].as_i64().unwrap() as i32,
                &movies
            ),
            row["result"].as_i64().unwrap() as i32,
            "ReadMovie input {} with current {}",
            row["value"],
            row["default"]
        );
    }
}

#[test]
fn physical_first_campaign_basic_readers_match_original_execution() {
    let Some((_, assets)) = super::retail_ini_fixture::retail_assets() else {
        return;
    };
    let native = native();
    let art = IniFile::from_bytes(assets.get_ref("ARTMD.INI").expect("retail ART")).unwrap();
    let movies = MovieRegistry::from_art(&art);
    for row in native["basic_start"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["alternate"].as_i64() == Some(0))
    {
        let filename = row["filename"].as_str().unwrap();
        let ini =
            IniFile::from_bytes(assets.get_ref(filename).expect("first campaign map")).unwrap();
        let basic = crate::map::basic::parse_basic_section(&ini);
        assert_eq!(
            basic.home_cell.unwrap_or(699),
            row["home_cell"].as_i64().unwrap() as i32
        );
        assert_eq!(
            basic.alt_home_cell.unwrap_or(699),
            row["alt_home_cell"].as_i64().unwrap() as i32
        );
        assert_eq!(
            movies.find_index(basic.action.as_deref().unwrap_or("")),
            row["action_movie"].as_i64().unwrap() as i32
        );
        let mut flags = crate::map::basic::parse_special_flags_section(&ini);
        flags.resolve_campaign_defaults(None);
        let word = row["special_after"].as_u64().unwrap();
        assert_eq!(flags.inert, Some(word & 0x20 != 0));
        assert_eq!(flags.tiberium_grows, Some(word & 0x40 != 0));
        assert_eq!(flags.tiberium_spreads, Some(word & 0x80 != 0));
        assert_eq!(flags.destroyable_bridges, Some(word & 0x08 != 0));
    }
}

#[test]
fn campaign_catalog_and_retained_reload_controls_match_original_execution() {
    let Some((_, assets)) = super::retail_ini_fixture::retail_assets() else {
        return;
    };
    let native = native();
    let bytes = assets.get_ref("RA2MD.CSF").expect("initialized retail CSF");
    assert_eq!(
        crate::util::sha256::sha256_hex(bytes),
        native["metadata"]["csf"]["source_sha256"]
    );
    let csf = CsfFile::from_bytes(bytes).unwrap();
    let bytes = assets
        .get_ref("BATTLEMD.INI")
        .expect("retail campaign catalog");
    assert_eq!(
        crate::util::sha256::sha256_hex(bytes),
        native["metadata"]["physical_battlemd_sha256"]
    );
    let battle = IniFile::from_bytes(bytes).unwrap();
    let movies = MovieRegistry::default();
    let mut stock = CampaignRegistry::default();
    stock.apply_ini(&battle, &movies, Some(&csf));
    assert_campaigns(&stock, &native["metadata"]["entries"]);
    for lookup in native["metadata"]["lookups"].as_array().unwrap() {
        assert_eq!(
            stock
                .find_index(lookup["request"].as_str().unwrap())
                .map_or(-1, |index| index as i32),
            lookup["index"].as_i64().unwrap() as i32,
        );
    }

    let mut retained = CampaignRegistry::default();
    for row in native["metadata_controls"].as_array().unwrap() {
        retained.apply_ini(&cache_ini(&row["sections"]), &movies, Some(&csf));
        assert_campaigns(&retained, &row["entries"]);
    }
}
