//! Kratos `CrawlingFLH` reads: the prone defaults and the elite fallback that
//! Kratos PP v0.2.4 (`CrawlingFLHData.h::Read`) applies.

use super::weapon_offsets::WeaponOffsets;
use crate::rules::ini_parser::IniFile;

/// The piece as one ART section leaves it.
fn read(art: &str) -> WeaponOffsets {
    let art = IniFile::from_str(art);
    let mut offsets = WeaponOffsets::default();
    offsets.read_kratos_infantry_flh(art.section("GI").unwrap());
    offsets
}

/// A standing key the ART does not override becomes the prone one at Z 20, so
/// an infantryman writes only `*FireFLH` and still has a prone muzzle.
#[test]
fn prone_offsets_default_to_the_standing_pair_at_z_20() {
    let offsets = read("[GI]\nPrimaryFireFLH=32,8,60\nSecondaryFireFLH=40,4,70\n");
    let piece = offsets.kratos_crawling();

    assert_eq!(piece.flh(0, false, true), [32, 8, 20].into());
    assert_eq!(piece.flh(1, false, true), [40, 4, 20].into());
    assert_eq!(piece.flh(0, false, false), [32, 8, 60].into());
    assert_eq!(piece.flh(1, false, false), [40, 4, 70].into());
}

/// Elite rank reads its own fire keys and an explicit `ElitePrimaryCrawlingFLH`
/// wins. The second slot has no elite crawling key: Kratos re-reads
/// `EliteSecondaryFireFLH`, so its explicit height survives, and only an absent
/// key falls back to the prone default.
#[test]
fn elite_rank_reads_its_own_keys_and_the_second_slot_re_reads_the_fire_coordinate() {
    let offsets = read(
        "[GI]\nPrimaryFireFLH=32,8,60\nElitePrimaryFireFLH=50,0,80\n\
         ElitePrimaryCrawlingFLH=11,1,13\nEliteSecondaryFireFLH=60,2,90\n",
    );
    let piece = offsets.kratos_crawling();

    assert_eq!(piece.flh(0, true, true), [11, 1, 13].into());
    assert_eq!(piece.flh(0, true, false), [50, 0, 80].into());
    // The re-read key is present, so its height is not replaced by Z 20.
    assert_eq!(piece.flh(1, true, true), [60, 2, 90].into());
    assert_eq!(piece.flh(1, true, false), [60, 2, 90].into());
    // Normal rank keeps its own defaults when only elite keys are written.
    assert_eq!(piece.flh(0, false, true), [32, 8, 20].into());

    let offsets = read("[GI]\nPrimaryFireFLH=32,8,60\nSecondaryFireFLH=40,4,70\n");
    let absent = offsets.kratos_crawling();
    assert_eq!(absent.flh(1, true, true), [40, 4, 20].into());
    assert_eq!(absent.flh(1, true, false), [40, 4, 70].into());
}

/// Two different slots are independent: writing one prone key leaves the other
/// slot's prone default alone.
#[test]
fn each_slot_reads_its_own_prone_key() {
    let offsets = read(
        "[GI]\nPrimaryFireFLH=32,8,60\nSecondaryFireFLH=40,4,70\n\
         SecondaryCrawlingFLH=7,7,7\n",
    );
    let piece = offsets.kratos_crawling();

    assert_eq!(piece.flh(0, false, true), [32, 8, 20].into());
    assert_eq!(piece.flh(1, false, true), [7, 7, 7].into());
}
