//! `SpawnsCrate*` reader (`Ext/WarheadType/Body.cpp:520-560`).

use super::*;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosSpawnsCrate {
    let ini = IniFile::from_str(text);
    PhobosSpawnsCrate::from_section(ini.section_or_empty("WH"))
}

#[test]
fn numbered_lists_stop_at_a_gap_and_default_the_weight() {
    let value = read(
        "[WH]\nSpawnsCrate.Type=Money\nSpawnsCrate1.Type=Veteran\n\
         SpawnsCrate1.Weight=3\nSpawnsCrate2.Weight=9\n",
    );
    assert_eq!(
        value.types,
        vec![
            powerup_index_from_name("Money").unwrap(),
            powerup_index_from_name("Veteran").unwrap()
        ],
        "the unnumbered alias feeds entry 0; the missing Type2 stops the list"
    );
    assert_eq!(value.weights, vec![1, 3], "an unset weight defaults to 1");
}

#[test]
fn an_unknown_type_or_clear_token_stops_the_list() {
    assert!(
        read("[WH]\nSpawnsCrate0.Type=NotAPowerup\n")
            .types
            .is_empty(),
        "Parser<Powerup> rejects an unknown name"
    );
    assert!(
        read("[WH]\nSpawnsCrate0.Type=<default>\n").types.is_empty(),
        "<default> resets the nullable"
    );
    let value = read("[WH]\nSpawnsCrate0.Type=Money\nSpawnsCrate0.Weight=4\n");
    assert_eq!(value.weights, vec![4], "entry 0 reads its own weight first");
}
