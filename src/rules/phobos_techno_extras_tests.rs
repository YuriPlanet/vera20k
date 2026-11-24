//! The twenty-three
//! [`crate::rules::phobos_techno_extras`].

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_shields::AffectedHouse;
use crate::rules::phobos_techno_extras::{PhobosTechnoExtras, TargetZoneScanType};

fn read(keys: &str) -> PhobosTechnoExtras {
    PhobosTechnoExtras::from_section(
        IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"),
    )
}

/// `Body.h:479`, `:509`, `:565`, `:644`, `:781`, `:803`, `:865-866`, `:727`:
/// nine unset nullables, five empty vectors, four zeroed scalars and the one
/// `-1` `SubterraneanSpeed` sentinel.
#[test]
fn the_twenty_three_keys_construct_unset() {
    let defaults = PhobosTechnoExtras::default();
    assert_eq!(read(""), defaults);
    assert_eq!(defaults.radar_jam_houses, None);
    assert_eq!(defaults.radar_jam_delay, None);
    assert!(defaults.radar_jam_affect.is_empty());
    assert!(defaults.radar_jam_ignore.is_empty());
    assert_eq!(defaults.voice_created, None);
    assert_eq!(defaults.use_disguise_movement_speed, None);
    assert_eq!(defaults.no_queue_up_to_enter, None);
    assert_eq!(defaults.no_queue_up_to_enter_board_distance, None);
    assert_eq!(defaults.no_queue_up_to_unload, None);
    assert_eq!(defaults.reload_in_transport, None);
    assert_eq!(defaults.veteran_reload, None);
    assert_eq!(defaults.veteran_empty_reload, None);
    assert!(defaults.spawns_queue.is_empty());
    assert_eq!(defaults.drop_crate, None);
    assert!(!defaults.low_selection_priority);
    assert_eq!(defaults.cameo_priority, 0);
    assert_eq!(defaults.allow_weapon_select_against_walls, None);
    assert_eq!(defaults.target_zone_scan_type, TargetZoneScanType::Same);
    assert_eq!(defaults.radar_invisible_to_house, None);
    assert_eq!(defaults.engineer_repair_amount, 0);
    assert_eq!(defaults.subterranean_speed, -1);
    assert_eq!(defaults.subterranean_height, None);
    assert_eq!(defaults.veteran_range, None);
}

/// `Body.cpp:766`, `:855`, `:909`, `:972`, `:1131`, `:1144`, `:1209-1210`,
/// `:1064`: the nine extra scalar/nullable keys read on their own.
#[test]
fn the_extra_scalars_and_nullables_read() {
    let set = read(
        "LowSelectionPriority=yes\nCameoPriority=3\nAllowWeaponSelectAgainstWalls=yes\n\
         TargetZoneScanType=Any\nRadarInvisibleToHouse=enemies\nEngineerRepairAmount=250\n\
         SubterraneanSpeed=50\nSubterraneanHeight=-100\nVeteranRange=1.25\n",
    );
    assert!(set.low_selection_priority);
    assert_eq!(set.cameo_priority, 3);
    assert_eq!(set.allow_weapon_select_against_walls, Some(true));
    assert_eq!(set.target_zone_scan_type, TargetZoneScanType::Any);
    assert_eq!(set.radar_invisible_to_house, Some(AffectedHouse::ENEMIES));
    assert_eq!(set.engineer_repair_amount, 250);
    // `ReadSpeed`: (50*256)/100 = 128.
    assert_eq!(set.subterranean_speed, 128);
    assert_eq!(set.subterranean_height, Some(-100));
    assert_eq!(set.veteran_range, Some(1.25));
}

/// `ReadSpeed` keeps the -1 sentinel for an absent key and for a negative one
/// (`Utilities/INIParser.h:157-163`), the enum read leaves `Same` on an unknown
/// token, and `<default>`/blank clear the nullables.
#[test]
fn bad_reads_keep_the_constructor_values_and_the_general_fallbacks_resolve() {
    let set = read(
        "SubterraneanSpeed=-5\nTargetZoneScanType=Bogus\n\
         AllowWeaponSelectAgainstWalls=<default>\nSubterraneanHeight=\nVeteranRange=\n",
    );
    assert_eq!(set.subterranean_speed, -1);
    assert_eq!(set.target_zone_scan_type, TargetZoneScanType::Same);
    assert_eq!(set.allow_weapon_select_against_walls, None);
    assert_eq!(set.subterranean_height, None);
    assert_eq!(set.veteran_range, None);
    assert!(set.allow_weapon_select_against_walls(true));
    assert_eq!(set.resolved_subterranean_speed(19), 19);
    assert_eq!(set.resolved_subterranean_height(-256), -256);
    assert_eq!(set.veteran_range_multiplier(1.0), 1.0);
    assert_eq!(
        set.radar_invisible_to_house_affected(AffectedHouse::ALL)
            .bits(),
        AffectedHouse::ALL.bits()
    );
}

/// `TargetZoneScanType` is an `EnumReader` over `Same`/`Any`/`InRange`
/// (`Utilities/Enum.h:251-256`), matched case-insensitively.
#[test]
fn target_zone_scan_type_parses_its_three_names() {
    assert_eq!(
        read("TargetZoneScanType=same\n").target_zone_scan_type,
        TargetZoneScanType::Same
    );
    assert_eq!(
        read("TargetZoneScanType=INRANGE\n").target_zone_scan_type,
        TargetZoneScanType::InRange
    );
    assert_eq!(
        read("TargetZoneScanType=Any\n").target_zone_scan_type,
        TargetZoneScanType::Any
    );
}

#[test]
fn every_key_reads_on_its_own() {
    let set = read(
        "RadarJamHouses=enemies\nRadarJamDelay=12\nRadarJamAffect=BARRACKS\nRadarJamIgnore=GAPILE\n\
         VoiceCreated=UnitCreated\nUseDisguiseMovementSpeed=yes\n\
         NoQueueUpToEnter=yes\nNoQueueUpToEnter.BoardDistance=512\nNoQueueUpToUnload=yes\nReloadInTransport=yes\n\
         VeteranReload=0.75\nVeteranEmptyReload=1.5\nSpawns.Queue=HORNET,ASW\n\
         DropCrate=Firepower\n",
    );
    assert_eq!(set.radar_jam_houses, Some(AffectedHouse::ENEMIES));
    assert_eq!(set.radar_jam_delay, Some(12));
    assert_eq!(set.radar_jam_affect, ["BARRACKS"]);
    assert_eq!(set.radar_jam_ignore, ["GAPILE"]);
    assert_eq!(set.voice_created.as_deref(), Some("UnitCreated"));
    assert_eq!(set.use_disguise_movement_speed, Some(true));
    assert_eq!(set.no_queue_up_to_enter, Some(true));
    assert_eq!(set.no_queue_up_to_enter_board_distance, Some(512));
    assert_eq!(set.no_queue_up_to_unload, Some(true));
    assert_eq!(set.reload_in_transport, Some(true));
    assert_eq!(set.veteran_reload, Some(0.75));
    assert_eq!(set.veteran_empty_reload, Some(1.5));
    assert_eq!(set.spawns_queue, ["HORNET", "ASW"]);
    // `POWERUP_NAMES[11] == "Firepower"`.
    assert_eq!(set.drop_crate, Some(11));
}

/// `Utilities/TemplateDef.h:1833-1849`: `Nullable::Read` resets on `<default>`
/// and on a blank read, and only those.
#[test]
fn a_default_or_blank_read_clears_the_nullables() {
    let cleared = read(
        "RadarJamDelay=<default>\nUseDisguiseMovementSpeed=\nVeteranReload=<default>\n\
         NoQueueUpToEnter.BoardDistance=\nNoQueueUpToUnload=<default>\nReloadInTransport=\n",
    );
    assert_eq!(cleared.radar_jam_delay, None);
    assert_eq!(cleared.use_disguise_movement_speed, None);
    assert_eq!(cleared.veteran_reload, None);
    assert_eq!(cleared.no_queue_up_to_enter_board_distance, None);
    assert_eq!(cleared.no_queue_up_to_unload, None);
    assert_eq!(cleared.reload_in_transport, None);
}

/// `AffectedHouse` parses `none` as a value, so `RadarJamHouses=none` means
/// "jam nobody" and must not fall back to the `[General]` default. Only a
/// blank read or an unknown token leaves the key unset.
#[test]
fn radar_jam_houses_parses_none_as_a_value() {
    assert_eq!(
        read("RadarJamHouses=none\n").radar_jam_houses,
        Some(AffectedHouse::NONE)
    );
    assert_eq!(read("RadarJamHouses=\n").radar_jam_houses, None);
    assert_eq!(
        read("RadarJamHouses=enemies,allies\n")
            .radar_jam_houses
            .map(AffectedHouse::bits),
        Some(AffectedHouse::ENEMIES.bits() | AffectedHouse::ALLIES.bits())
    );
    assert_eq!(read("RadarJamHouses=bogus\n").radar_jam_houses, None);
}

/// `detail::read<Powerup>` (`Utilities/TemplateDef.h:648-676`) is an exact
/// scan of the fixed `[Powerups]` table: an unknown name is a parse failure,
/// and the match is case-insensitive.
#[test]
fn drop_crate_resolves_a_powerup_slot_and_rejects_unknown_names() {
    assert_eq!(read("DropCrate=money\n").drop_crate, Some(0));
    assert_eq!(read("DropCrate=HealBase\n").drop_crate, Some(2));
    assert_eq!(read("DropCrate=Pod\n").drop_crate, Some(18));
    assert_eq!(read("DropCrate=Bogus\n").drop_crate, None);
}

/// `ValueableVector::Read` is one `ReadString`, then a comma `strtok`; the
/// empty tokens the leading/trailing/double commas produce never reach the
/// vector.
#[test]
fn the_two_type_lists_split_on_commas_only() {
    let lists = read("RadarJamAffect=,AAA, BBB ,,CCC,\nRadarJamIgnore=DDD\n");
    assert_eq!(lists.radar_jam_affect, ["AAA", " BBB ", "CCC"]);
    assert_eq!(lists.radar_jam_ignore, ["DDD"]);
}
