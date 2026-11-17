//! `SkirmishSetups.ini` beside the profile: VERA's last Skirmish setup
//! ([`SkirmishSavedSetup`]); gamemd has no such file. `[Last Setup]` holds the
//! page the player last left Skirmish from, and the next visit puts it back.
//! `Map=` names its map by file name, in lower case with `%XX` for every byte
//! outside `a-z`, `0-9`, `.`, `_` and `-`, so no semicolon or space reaches
//! the line. Rows hold the `[Skirmish] SlotNN` codes, then a start and a team:
//! `Player=side,colour,start,team`, and `Slot01=type,side,colour,start,team`
//! for each computer row the map shows, -2 standing for Random and for no
//! team. The options keep their `[Skirmish]` key names.
//!
//! A file from an older VERA can also hold a section per map from its Save
//! Setup; writing `[Last Setup]` leaves them as they are.

use std::fmt::Write as _;

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::crt_atoi;
use crate::skirmish_launch::{
    LaunchStartPosition, NETWORK_PLAYER_SLOT_COUNT, SkirmishLaunchOptions,
};
use crate::ui::skirmish_shell::{
    SkirmishCountryChoice, SkirmishSavedSetup, SkirmishSetupComputer, SkirmishSetupSeat,
};

use super::persistence::yes_no;
use super::{
    RANDOM_ITEM_DATA, ai_row_type_from_persisted, menu_country_from_item_data,
    menu_country_item_data, persisted_row_type,
};

pub(super) const FILE_NAME: &str = "SkirmishSetups.ini";
const LAST_SETUP_SECTION: &str = "Last Setup";

/// The page the player last left: its map's section name and its setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LastSetup {
    pub(super) map: String,
    pub(super) setup: SkirmishSavedSetup,
}

/// How `Map=` names a map file.
pub(super) fn map_key(map_file: &str) -> String {
    let mut name = String::with_capacity(map_file.len());
    for byte in map_file.to_ascii_lowercase().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            name.push(char::from(byte));
        } else {
            let _ = write!(name, "%{byte:02X}");
        }
    }
    name
}

/// The file's last setup; one without `Player` is none.
pub(super) fn read_last_setup(bytes: &[u8]) -> Option<LastSetup> {
    let ini = IniFile::from_bytes(bytes).ok()?;
    let section = ini.section(LAST_SETUP_SECTION)?;
    Some(LastSetup {
        map: section.read_string("Map", "", 0x400),
        setup: read_setup(section)?,
    })
}

/// The file with `[Last Setup]` holding `last` and every other section kept.
pub(super) fn write_last_setup(file: &[u8], last: &LastSetup) -> Vec<u8> {
    let mut values = vec![("Map".to_owned(), last.map.clone())];
    values.extend(setup_values(&last.setup));
    replace_section(file, LAST_SETUP_SECTION, values)
}

fn setup_values(setup: &SkirmishSavedSetup) -> Vec<(String, String)> {
    let mut values = vec![("Player".to_owned(), seat_value(setup.player))];
    for (index, computer) in setup.computers.iter().enumerate() {
        values.push((
            format!("Slot{:02}", index + 1),
            format!(
                "{},{}",
                persisted_row_type(computer.row_type),
                seat_value(computer.seat)
            ),
        ));
    }
    for (key, value) in [
        ("GameSpeed", setup.game_speed.to_string()),
        ("Credits", setup.credits.to_string()),
        ("UnitCount", setup.unit_count.to_string()),
        ("ShortGame", yes_no(setup.short_game).to_owned()),
        (
            "SuperWeaponsAllowed",
            yes_no(setup.super_weapons).to_owned(),
        ),
        ("BuildOffAlly", yes_no(setup.build_off_ally).to_owned()),
        ("MCVRepacks", yes_no(setup.mcv_redeploy).to_owned()),
        ("CratesAppear", yes_no(setup.crates).to_owned()),
    ] {
        values.push((key.to_owned(), value));
    }
    values
}

fn replace_section(file: &[u8], section: &str, values: Vec<(String, String)>) -> Vec<u8> {
    let values: Vec<(&str, &str)> = values
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    crate::util::ini_writer::replace_ini_section(file, section, &values)
}

fn read_setup(section: &IniSection) -> Option<SkirmishSavedSetup> {
    let player = read_seat(&section.read_list("Player", 0x200)?);
    let computers = (1..NETWORK_PLAYER_SLOT_COUNT)
        .map_while(|number| section.read_list(&format!("Slot{number:02}"), 0x200))
        .map(|tokens| SkirmishSetupComputer {
            row_type: ai_row_type_from_persisted(tokens.first().map_or(1, |type_| crt_atoi(type_))),
            seat: read_seat(tokens.get(1..).unwrap_or_default()),
        })
        .collect();
    // Load checks every value against the page; a missing key reads as the
    // stock option.
    let stock = SkirmishLaunchOptions::default();
    Some(SkirmishSavedSetup {
        player,
        computers,
        game_speed: section.read_int("GameSpeed", stock.game_speed),
        credits: section.read_int("Credits", stock.starting_credits),
        unit_count: section.read_int("UnitCount", stock.unit_count),
        short_game: section.read_bool("ShortGame", stock.short_game),
        super_weapons: section.read_bool("SuperWeaponsAllowed", stock.super_weapons),
        build_off_ally: section.read_bool("BuildOffAlly", stock.build_off_ally),
        mcv_redeploy: section.read_bool("MCVRepacks", stock.mcv_redeploy),
        crates: section.read_bool("CratesAppear", stock.crates),
    })
}

/// `side,colour,start,team`; a missing number reads as Random or no team.
fn read_seat(tokens: &[&str]) -> SkirmishSetupSeat {
    let number = |index: usize| {
        tokens
            .get(index)
            .map_or(RANDOM_ITEM_DATA, |token| crt_atoi(token))
    };
    SkirmishSetupSeat {
        country: menu_country_from_item_data(number(0)).map_or(
            SkirmishCountryChoice::Random,
            SkirmishCountryChoice::Country,
        ),
        color: usize::try_from(number(1)).ok(),
        start: u8::try_from(number(2))
            .map_or(LaunchStartPosition::Auto, LaunchStartPosition::Position),
        team: number(3),
    }
}

fn seat_value(seat: SkirmishSetupSeat) -> String {
    let country = match seat.country {
        SkirmishCountryChoice::Random => RANDOM_ITEM_DATA,
        SkirmishCountryChoice::Country(country) => menu_country_item_data(country),
    };
    let color = seat
        .color
        .and_then(|color| i32::try_from(color).ok())
        .unwrap_or(RANDOM_ITEM_DATA);
    let start = match seat.start {
        LaunchStartPosition::Auto => RANDOM_ITEM_DATA,
        LaunchStartPosition::Position(index) => i32::from(index),
    };
    format!("{country},{color},{start},{}", seat.team)
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::skirmish_launch::LaunchCountry;
    use crate::ui::skirmish_shell::SkirmishAiRowType;

    /// The last setup reads back with its map and every row. Writing it again
    /// replaces `[Last Setup]`, and an older file's map sections stay.
    #[test]
    fn the_last_setup_reads_back_beside_older_map_sections() {
        let seat = |country, color, start, team| SkirmishSetupSeat {
            country,
            color,
            start,
            team,
        };
        let last = LastSetup {
            map: map_key("[TW] Death Trap;2.MAP"),
            setup: SkirmishSavedSetup {
                player: seat(
                    SkirmishCountryChoice::Country(LaunchCountry::from_country_index(8)),
                    Some(2),
                    LaunchStartPosition::Position(1),
                    0,
                ),
                computers: vec![
                    SkirmishSetupComputer {
                        row_type: SkirmishAiRowType::Hard,
                        seat: seat(
                            SkirmishCountryChoice::Random,
                            None,
                            LaunchStartPosition::Auto,
                            -2,
                        ),
                    },
                    SkirmishSetupComputer {
                        row_type: SkirmishAiRowType::None,
                        seat: seat(
                            SkirmishCountryChoice::Random,
                            None,
                            LaunchStartPosition::Auto,
                            3,
                        ),
                    },
                ],
                game_speed: 2,
                credits: 7500,
                unit_count: 4,
                short_game: false,
                super_weapons: false,
                build_off_ally: true,
                crates: false,
                mcv_redeploy: true,
            },
        };
        // Older files name maps this way; a semicolon would end the line.
        assert_eq!(last.map, "%5Btw%5D%20death%20trap%3B2.map");

        let older = b"[xmp27t8.map]\r\nPlayer=-2,-2,-2,-2\r\n";
        let file = write_last_setup(older, &last);
        assert_eq!(read_last_setup(&file), Some(last.clone()));

        let file = write_last_setup(&file, &last);
        let text = String::from_utf8(file).unwrap();
        assert_eq!(text.matches("[Last Setup]").count(), 1, "{text}");
        assert!(text.contains("[xmp27t8.map]"), "{text}");
    }
}
