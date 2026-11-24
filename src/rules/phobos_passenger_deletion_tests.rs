//! `PassengerDeletion.*` reads (`Ext/TechnoType/Body.cpp:1389-1399`,
//! `New/Type/Affiliated/PassengerDeletionTypeClass.cpp`).
use super::{AffectedHouse, PassengerDeletionSettings};
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> Option<PassengerDeletionSettings> {
    PassengerDeletionSettings::from_section(IniFile::from_str(text).section_or_empty("TRUCK"))
}

/// `CanParse`: the affiliated type exists only for a positive rate or a
/// cost-derived one, so all other keys alone create nothing.
#[test]
fn the_type_exists_only_with_a_rate_or_cost_derived_rate() {
    assert!(read("").is_none());
    assert!(read("[TRUCK]\nPassengerDeletion.Soylent=yes\n").is_none());
    assert!(read("[TRUCK]\nPassengerDeletion.Rate=0\n").is_none());
    assert!(read("[TRUCK]\nPassengerDeletion.Rate=-5\n").is_none());
    assert!(read("[TRUCK]\nPassengerDeletion.Rate=1\n").is_some());
    assert!(read("[TRUCK]\nPassengerDeletion.UseCostAsRate=yes\n").is_some());
}

/// Constructor defaults from `PassengerDeletionTypeClass` / `Body.h`.
#[test]
fn the_defaults_match_the_affiliated_type_constructor() {
    let set = read("[TRUCK]\nPassengerDeletion.Rate=7\n").unwrap();
    assert_eq!(set.rate, 7);
    assert!(set.rate_size_multiply);
    assert!(!set.use_cost_as_rate);
    assert_eq!(set.cost_multiplier, 1.0);
    assert_eq!(set.cost_rate_cap, None);
    assert_eq!(set.allowed_houses, AffectedHouse::ALL);
    assert!(!set.dont_score);
    assert!(!set.soylent);
    assert_eq!(set.soylent_multiplier, 1.0);
    assert_eq!(set.soylent_allowed_houses, AffectedHouse::ENEMIES);
    assert!(!set.display_soylent);
    assert_eq!(set.display_soylent_to_houses, AffectedHouse::ALL);
    assert_eq!(set.display_soylent_offset, [0, 0]);
    assert_eq!(set.report_sound, None);
    assert!(set.anim.is_empty());
    assert!(!set.under_emp);
}

/// Every key reads independently, including the two house filters, the point
/// offset, the sound name and the anim list.
#[test]
fn every_key_reads_on_its_own() {
    let set = read(
        "[TRUCK]\nPassengerDeletion.Rate=3\nPassengerDeletion.Rate.SizeMultiply=no\n\
         PassengerDeletion.CostMultiplier=2.5\nPassengerDeletion.CostRateCap=40\n\
         PassengerDeletion.AllowedHouses=enemies\nPassengerDeletion.DontScore=yes\n\
         PassengerDeletion.Soylent=yes\nPassengerDeletion.SoylentMultiplier=0.5\n\
         PassengerDeletion.SoylentAllowedHouses=allies\nPassengerDeletion.DisplaySoylent=yes\n\
         PassengerDeletion.DisplaySoylentToHouses=owner\n\
         PassengerDeletion.DisplaySoylentOffset=5,-7\n\
         PassengerDeletion.ReportSound=CashUp\nPassengerDeletion.Anim=YELLOW,SPARK\n\
         PassengerDeletion.UnderEMP=yes\n",
    )
    .unwrap();
    assert!(!set.rate_size_multiply);
    assert_eq!(set.cost_multiplier, 2.5);
    assert_eq!(set.cost_rate_cap, Some(40));
    assert_eq!(set.allowed_houses, AffectedHouse::ENEMIES);
    assert!(set.dont_score);
    assert!(set.soylent);
    assert_eq!(set.soylent_multiplier, 0.5);
    assert_eq!(set.soylent_allowed_houses, AffectedHouse::ALLIES);
    assert!(set.display_soylent);
    assert_eq!(set.display_soylent_to_houses, AffectedHouse::OWNER);
    assert_eq!(set.display_soylent_offset, [5, -7]);
    assert_eq!(set.report_sound.as_deref(), Some("CashUp"));
    assert_eq!(set.anim, ["YELLOW", "SPARK"]);
    assert!(set.under_emp);
}

/// `PassengerDeletion.DisplaySoylentOffset=` is a Phobos `Valueable<Point2D>`:
/// `INI_EX::Read2Integers` (`Parser<int, 2>`) trims each comma-separated
/// token, writes every converted token straight into the pair, stops at the
/// first token that does not convert, and accepts the `$`/`h` integer
/// spellings. A one-token read therefore changes X and leaves Y.
#[test]
fn display_soylent_offset_follows_read2integers() {
    let offset = |value: &str| {
        read(&format!(
            "[TRUCK]\nPassengerDeletion.Rate=1\n\
             PassengerDeletion.DisplaySoylentOffset={value}\n"
        ))
        .expect("the rate creates the type")
        .display_soylent_offset
    };
    assert_eq!(offset("5,-7"), [5, -7]);
    assert_eq!(offset("5 , -7"), [5, -7], "Parser<int, 2> trims each token");
    assert_eq!(offset("5"), [5, 0], "a one-token read changes only X");
    assert_eq!(offset("5,"), [5, 0]);
    assert_eq!(offset("5,x"), [5, 0], "a failed second token keeps Y");
    assert_eq!(
        offset("x,7"),
        [0, 0],
        "the scan stops at the first failed token"
    );
    assert_eq!(offset("$10,0Ah"), [10, 10], "the Parser<int> spellings");
    assert_eq!(offset(",,,"), [0, 0]);
}
