//! The damaged-image keys owned by [`crate::rules::phobos_damaged_image`].

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_damaged_image::PhobosDamagedImage;

fn read(keys: &str) -> PhobosDamagedImage {
    PhobosDamagedImage::from_section(
        IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"),
    )
}

/// `TechnoType/Body.h:763-765`, `UnitType/Body.h:125-126`: four unset
/// nullables and `NeedDamagedImage = false`.
#[test]
fn the_four_keys_construct_unset() {
    let defaults = PhobosDamagedImage::default();
    assert_eq!(read(""), defaults);
    assert_eq!(defaults.condition_yellow, None);
    assert_eq!(defaults.condition_red, None);
    assert_eq!(defaults.water_condition_yellow, None);
    assert_eq!(defaults.water_condition_red, None);
    assert!(!defaults.need_damaged_image());
}

/// `TechnoType/Body.cpp:1114` and `UnitType/Body.cpp:80`: the flag is the `|=`
/// of the four `isset()` tests, so any one of them alone raises it.
#[test]
fn any_single_swap_raises_the_flag() {
    let cases = [
        ("Image.ConditionYellow", 0usize),
        ("Image.ConditionRed", 1),
        ("WaterImage.ConditionYellow", 2),
        ("WaterImage.ConditionRed", 3),
    ];
    for (key, slot) in cases {
        let set = read(&format!("{key}=DAMAGED\n"));
        assert!(set.need_damaged_image(), "{key} raises the flag");
        let fields = [
            set.condition_yellow.as_deref(),
            set.condition_red.as_deref(),
            set.water_condition_yellow.as_deref(),
            set.water_condition_red.as_deref(),
        ];
        assert_eq!(fields[slot], Some("DAMAGED"), "{key} sets its own field");
        assert_eq!(
            fields.iter().filter(|value| value.is_some()).count(),
            1,
            "{key} sets only its own field"
        );
    }
}

/// Each band answers its own key, and the water pair is separate from the
/// plain one.
#[test]
fn the_bands_and_the_water_pair_are_independent() {
    let set = read(
        "Image.ConditionYellow=Y\nImage.ConditionRed=R\n\
         WaterImage.ConditionYellow=WY\nWaterImage.ConditionRed=WR\n",
    );
    assert_eq!(set.band_image(false), Some("Y"));
    assert_eq!(set.band_image(true), Some("R"));
    assert_eq!(set.water_band_image(false), Some("WY"));
    assert_eq!(set.water_band_image(true), Some("WR"));
}

/// `Nullable<...Type*>`: `<default>` and a blank read reset, and `none` is a
/// type-name sentinel that also resets.
#[test]
fn a_default_or_blank_read_clears_a_swap() {
    assert_eq!(read("Image.ConditionRed=<default>\n").condition_red, None);
    assert_eq!(read("WaterImage.ConditionRed=\n").water_condition_red, None);
    assert_eq!(read("Image.ConditionYellow=none\n").condition_yellow, None);
}
