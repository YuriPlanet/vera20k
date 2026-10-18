//! UnitType `SpeedType` defaults. The UnitType constructor seeds -1
//! (0x007470D7); `UnitTypeClass::ReadINI` 0x00747620 runs TechnoType's
//! reader first (call site 0x0074763F, `Crusher=` stored at +0xD28), the
//! still--1 seed resolves Track when `Crusher=` is set and Wheel otherwise
//! (0x007476D3..0x007476EB), and `SpeedType=` is read after that, against
//! the derived value. The derivation lives in UnitTypeClass::ReadINI only:
//! Infantry/Building keep the Foot default (InfantryType5236A0 /
//! BuildingType45DD9D pass Foot0 to TechnoType710AF0).

use super::*;
use crate::rules::ini_parser::IniFile;

fn parsed_speed_type(section_text: &str, category: ObjectCategory) -> SpeedType {
    let ini = IniFile::from_str(section_text);
    ObjectType::from_ini_section("TYPE", ini.section("TYPE").unwrap(), category).speed_type
}

#[test]
fn unit_without_speed_type_and_crusher_defaults_to_wheel() {
    assert_eq!(
        parsed_speed_type("[TYPE]\nStrength=100\n", ObjectCategory::Vehicle),
        SpeedType::Wheel
    );
}

#[test]
fn unit_without_speed_type_with_crusher_defaults_to_track() {
    assert_eq!(
        parsed_speed_type("[TYPE]\nCrusher=yes\n", ObjectCategory::Vehicle),
        SpeedType::Track
    );
}

#[test]
fn unit_explicit_speed_type_overrides_the_crusher_derivation() {
    assert_eq!(
        parsed_speed_type("[TYPE]\nSpeedType=Track\n", ObjectCategory::Vehicle),
        SpeedType::Track
    );
    assert_eq!(
        parsed_speed_type(
            "[TYPE]\nCrusher=yes\nSpeedType=Wheel\n",
            ObjectCategory::Vehicle
        ),
        SpeedType::Wheel
    );
}

#[test]
fn foot_default_categories_do_not_derive_from_crusher() {
    assert_eq!(
        parsed_speed_type("[TYPE]\nStrength=100\n", ObjectCategory::Infantry),
        SpeedType::Foot
    );
    assert_eq!(
        parsed_speed_type("[TYPE]\nStrength=100\n", ObjectCategory::Building),
        SpeedType::Foot
    );
    assert_eq!(
        parsed_speed_type("[TYPE]\nCrusher=yes\n", ObjectCategory::Building),
        SpeedType::Foot
    );
}
