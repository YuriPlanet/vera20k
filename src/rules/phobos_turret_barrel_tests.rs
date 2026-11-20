//! The RA1 multi-turret / multi-barrel block of
//! [`crate::rules::phobos_turret_barrel`]: the indexed loops, the two count
//! clamps and the gated `BurstPerTurret=`.

use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_turret_barrel::PhobosTurretBarrel;
use crate::rules::ruleset::RuleSet;

fn read(keys: &str) -> PhobosTurretBarrel {
    PhobosTurretBarrel::from_section(
        IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"),
    )
}

fn project(rules_keys: &str, art_sections: &str) -> PhobosTurretBarrel {
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nTurret=yes\n{rules_keys}"
        )),
        &IniFile::from_str(art_sections),
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(art_sections)));
    rules.object("MTNK").unwrap().phobos_turret_barrel.clone()
}

/// `Ext/UnitType/Body.cpp:100-134`: every key comes from the ART section, and
/// `RuleSet::project_art_data` copies the block onto the Vehicle type.
#[test]
fn projection_reads_the_multi_turret_block_from_art() {
    let set = project(
        "",
        "[MTNK]\nBarrelOverTurret=yes\nBarrelOffset=16\n\
         ExtraBarrelCount=1\nExtraBarrelOffset0=-4\n\
         ExtraTurretCount=2\nExtraTurretOffset0=1,2,3\n\
         ExtraTurretOffset1=-4,5,-6\nBurstPerTurret=2\n",
    );
    assert_eq!(set.barrel_over_turret, Some(true));
    assert_eq!(set.barrel_offset, 16);
    assert_eq!(set.extra_barrel_count, 1);
    assert_eq!(set.extra_barrel_offsets, [-4]);
    assert_eq!(set.extra_turret_count, 2);
    assert_eq!(set.extra_turret_offsets, [[1, 2, 3], [-4, 5, -6]]);
    assert_eq!(set.burst_per_turret, 2);
}

/// Native `UnitTypeExt` reads ART only: the same keys in the rule section
/// leave the constructor defaults in place.
#[test]
fn rules_section_keys_do_not_supply_the_multi_turret_block() {
    let set = project(
        "ExtraTurretCount=2\nExtraTurretOffset0=1,2,3\n\
         ExtraBarrelCount=1\nExtraBarrelOffset0=9\nBurstPerTurret=2\n",
        "[MTNK]\n",
    );
    assert_eq!(set, PhobosTurretBarrel::default());
}

/// `Image=` selects the ART section: keys under the unrenamed type ID are not
/// consulted.
#[test]
fn the_image_override_selects_the_art_section() {
    let set = project(
        "Image=MTNKX\n",
        "[MTNKX]\nExtraTurretCount=1\nExtraTurretOffset0=7,8,9\n\
         [MTNK]\nExtraTurretCount=2\nExtraTurretOffset0=1,2,3\n",
    );
    assert_eq!(set.extra_turret_count, 1);
    assert_eq!(set.extra_turret_offsets, [[7, 8, 9]]);
}

/// `Ext/UnitType/Body.h:135-141`: one unset `Nullable` and five zeros.
#[test]
fn the_block_constructs_at_its_native_defaults() {
    let defaults = PhobosTurretBarrel::default();
    assert_eq!(read(""), defaults);
    assert_eq!(defaults.barrel_over_turret, None);
    assert_eq!(defaults.barrel_offset, 0);
    assert_eq!(defaults.extra_barrel_count, 0);
    assert!(defaults.extra_barrel_offsets.is_empty());
    assert_eq!(defaults.extra_turret_count, 0);
    assert!(defaults.extra_turret_offsets.is_empty());
    assert_eq!(defaults.burst_per_turret, 0);
    assert_eq!(defaults.barrel_total(), 1);
    assert_eq!(defaults.turret_total(), 1);
}

/// `Body.cpp:104-117`: the count decides how many `ExtraBarrelOffset<n>=` keys
/// are read, and index order is preserved.
#[test]
fn the_barrel_offsets_follow_the_count() {
    let set = read(
        "ExtraBarrelCount=3\nExtraBarrelOffset0=-4\nExtraBarrelOffset1=8\nExtraBarrelOffset2=12\nExtraBarrelOffset3=99\n",
    );
    assert_eq!(set.extra_barrel_count, 3);
    assert_eq!(set.extra_barrel_offsets, [-4, 8, 12]);
    assert_eq!(set.barrel_total(), 4);
}

/// `Body.cpp:114-118`, `:132-135`: a non-positive count clamps itself to zero
/// and reads no offsets, so a negative count never yields an offset vector.
#[test]
fn a_non_positive_count_clamps_to_zero() {
    let set = read(
        "ExtraBarrelCount=-2\nExtraBarrelOffset0=5\nExtraTurretCount=-1\nExtraTurretOffset0=1,2,3\n",
    );
    assert_eq!(set.extra_barrel_count, 0);
    assert!(set.extra_barrel_offsets.is_empty());
    assert_eq!(set.extra_turret_count, 0);
    assert!(set.extra_turret_offsets.is_empty());
}

/// `Body.cpp:121-131`: the turret offsets are `CoordStruct` triples, and
/// `BurstPerTurret=` is read only inside the positive-count arm.
#[test]
fn the_turret_block_reads_triples_and_gates_burst_per_turret() {
    let set = read(
        "ExtraTurretCount=2\nExtraTurretOffset0=1,2,3\nExtraTurretOffset1=-4,5,-6\nBurstPerTurret=2\n",
    );
    assert_eq!(set.extra_turret_count, 2);
    assert_eq!(set.extra_turret_offsets, [[1, 2, 3], [-4, 5, -6]]);
    assert_eq!(set.burst_per_turret, 2);
    assert_eq!(set.turret_total(), 3);

    // The same key without a positive turret count never reaches the reader.
    assert_eq!(read("BurstPerTurret=2\n").burst_per_turret, 0);
}

/// `Body.cpp:100-101`: `BarrelOverTurret=` is a `Nullable<bool>` and
/// `BarrelOffset=` a plain int.
#[test]
fn the_barrel_switch_and_offset_read_independently() {
    let set = read("BarrelOverTurret=yes\nBarrelOffset=16\n");
    assert_eq!(set.barrel_over_turret, Some(true));
    assert_eq!(set.barrel_offset, 16);
    assert_eq!(
        read("BarrelOverTurret=<default>\n").barrel_over_turret,
        None
    );
}
