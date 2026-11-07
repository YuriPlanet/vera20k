//! BuildingType construction controls: the BState 0 animation control
//! (`Type+0xF04`: first frame, frame count, rate) each building type derives
//! from its Buildup SHP, bound GPU-free from the active theater's assets like
//! the effect catalog, so graphical and headless matches share it.
//!
//! Native: `BuildingTypeClass` load (`0x0045F23E..0x0045F30A`; the demand-load
//! path `0x00465A53..0x00465AA8` computes the same control). The art
//! `Buildup=` name (`Type+0xE5C`) plus `.SHP` is looked up with the active
//! theater's letter in place of the second letter of a `G/N/C/Y` + `A/T` name
//! (`0x005F96B0`), then with `G` there (`0x005F9710`) when that misses. The SHP
//! header's frame word (`+6`, read signed) halved toward zero drops the shadow
//! half and is the frame count, or `GateStages + 1` for a gate (`+0x16B7`); the
//! rate is `ftol(BuildupTime * 900.0 / count)` under the process control word
//! (53-bit precision, chop), 1 for a count of 0 or less. A type without a
//! Buildup SHP keeps the constructor's `{0, 1, 0}` (`0x0045E362..0x0045E36E`):
//! rate 0, so its build-up completes at once.
//!
//! Evidence: `tools/spatial_oracle/building_construction.json` `rate` rows
//! (the slice with retail `BuildupTime=.06` and the constructor's `.05`).
//!
//! ## Dependency rules
//! - Part of rules/; depends on assets/ and util/ only.

use std::collections::BTreeMap;

use crate::assets::asset_manager::AssetManager;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::util::native_x87::{NativeF64Bits, X87Chop53};

/// The BuildingType constructor's construction control: first frame 0, one
/// frame, rate 0.
pub const NO_BUILDUP: [i32; 3] = [0, 1, 0];

/// `900.0`, the frames-per-minute factor at `0x007E27F8`.
const FRAMES_PER_MINUTE: NativeF64Bits = NativeF64Bits::from_bits(0x408C_2000_0000_0000);

/// The construction control from a Buildup SHP's raw frame word, a gate's
/// `GateStages` and `BuildupTime` (`0x0045F2B4..0x0045F30A`).
pub fn buildup_control(
    raw_frames: i16,
    gate_stages: Option<i32>,
    buildup_time: NativeF64Bits,
) -> [i32; 3] {
    // `CDQ; SUB EAX,EDX; SAR 1`: halved toward zero.
    let count = match gate_stages {
        Some(stages) => stages.wrapping_add(1),
        None => i32::from(raw_frames) / 2,
    };
    if count <= 0 {
        return [0, count, 1];
    }
    let rate = X87Chop53::load_f64(buildup_time)
        .map(|time| {
            X87Chop53::mul(
                time,
                X87Chop53::load_f64(FRAMES_PER_MINUTE).expect("900.0 is finite"),
            )
        })
        .and_then(|product| X87Chop53::div(product, X87Chop53::load_i32(count)))
        .and_then(X87Chop53::ftol_i64)
        // `_ftol` returns the low dword of its `FISTP qword` (`0x007C5F1B..
        // 0x007C5F20`): an unrepresentable value's integer indefinite
        // (`0x8000_0000_0000_0000`) leaves 0.
        .map_or(0, |rate| rate as i32);
    [0, count, rate]
}

/// Ares 3.0 `BuildingTypeExt::ExtData::UpdateBuildupFrames @ 1000F450`,
/// which replaces the rate computation at the three Buildup loads
/// (`0x0045F2B4` -> `10010210`, `0x00465A48` -> `10010100`, `0x0045EAA5` ->
/// `10010270`) and also yields the sale's frame rate (ExtData `+0x228`):
/// the same frame count, then `_ftol2(time / count * 900.0)` (Ares' order,
/// `1000F4D5..1000F4F0`) for the build-up and the sell time, both 1 for a
/// count below 1. The times are the type's `BuildupTime=`/`SellTime=`
/// fallbacks ([`crate::rules::ares_building_type::BuildingTypeExtRules`]).
#[cfg(feature = "ares")]
pub fn ares_buildup_frames(
    raw_frames: i16,
    gate_stages: Option<i32>,
    buildup_time: NativeF64Bits,
    sell_time: NativeF64Bits,
) -> ([i32; 3], i32) {
    let count = match gate_stages {
        Some(stages) => stages.wrapping_add(1),
        None => i32::from(raw_frames) / 2,
    };
    if count < 1 {
        return ([0, count, 1], 1);
    }
    let rate = |time: NativeF64Bits| {
        X87Chop53::load_f64(time)
            .and_then(|time| X87Chop53::div(time, X87Chop53::load_i32(count)))
            .map(|per_frame| {
                X87Chop53::mul(
                    per_frame,
                    X87Chop53::load_f64(FRAMES_PER_MINUTE).expect("900.0 is finite"),
                )
            })
            .and_then(X87Chop53::ftol_i64)
            .ok()
            // `_ftol2` (`100772B0`) answers the integer indefinite for a
            // value outside int32.
            .and_then(|rate| i32::try_from(rate).ok())
            .unwrap_or(i32::MIN)
    };
    ([0, count, rate(buildup_time)], rate(sell_time))
}

/// The construction control of every building type whose Buildup SHP the
/// active theater resolves. Keys are uppercase type IDs; a `BTreeMap` keeps
/// binding and hashing order deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct BuildupAssetCatalog {
    controls: BTreeMap<String, [i32; 3]>,
    /// Ares `SellFrameDuration` (BuildingTypeExt `+0x228`) of each bound
    /// type; an unbound type keeps the constructor's 0.
    #[cfg(feature = "ares")]
    sell_rates: BTreeMap<String, i32>,
}

impl BuildupAssetCatalog {
    /// Bind every BuildingType's Buildup SHP. A type whose art names none, or
    /// whose SHP the theater lacks, keeps [`NO_BUILDUP`].
    pub fn bind(rules: &RuleSet, asset_manager: &AssetManager, theater_name: &str) -> Self {
        let mut catalog = Self::default();
        let buildup_time = NativeF64Bits::from_bits(rules.general.buildup_time.to_bits());
        for object in rules
            .all_objects()
            .filter(|object| object.category == ObjectCategory::Building)
        {
            let Some(art) = rules
                .art()
                .resolve_metadata_entry(&object.id, &object.image)
            else {
                continue;
            };
            let Some(buildup) = art.buildup.as_deref() else {
                continue;
            };
            let Some(data) = crate::rules::art_data::building_shp_names(buildup, theater_name)
                .iter()
                .find_map(|candidate| asset_manager.get_ref(candidate))
            else {
                continue;
            };
            let Some(raw) = data
                .get(6..8)
                .map(|word| i16::from_le_bytes([word[0], word[1]]))
            else {
                continue;
            };
            let gate_stages = object.gate.then_some(art.building_gate_stages);
            #[cfg(not(feature = "ares"))]
            let control = buildup_control(raw, gate_stages, buildup_time);
            #[cfg(feature = "ares")]
            let control = {
                let (buildup, sell) = object
                    .building_type_ext()
                    .buildup_and_sell_time(buildup_time);
                let (control, sell_rate) = ares_buildup_frames(raw, gate_stages, buildup, sell);
                catalog
                    .sell_rates
                    .insert(object.id.to_ascii_uppercase(), sell_rate);
                control
            };
            catalog
                .controls
                .insert(object.id.to_ascii_uppercase(), control);
        }
        catalog
    }

    /// Ares: the sale's frame rate of `type_id` (`SellFrameDuration`, 0 when
    /// its Buildup SHP is unbound).
    #[cfg(feature = "ares")]
    pub fn sell_rate(&self, type_id: &str) -> i32 {
        self.sell_rates
            .get(&type_id.to_ascii_uppercase())
            .copied()
            .unwrap_or(0)
    }

    /// The construction control of `type_id` ([`NO_BUILDUP`] when unbound).
    pub fn control(&self, type_id: &str) -> [i32; 3] {
        self.controls
            .get(&type_id.to_ascii_uppercase())
            .copied()
            .unwrap_or(NO_BUILDUP)
    }

    /// Whether `type_id`'s Buildup SHP is bound: `BuildingClass::Init_Managers`
    /// sets `+0x6E9` (`0x00442CCF`) exactly when the type's Buildup data
    /// (`vt+0xC0`, `0x00465960`) is loaded, and a building without it cannot
    /// be sold (`Sell_Back @ 0x00447110`, `CanSell @ 0x004494C0`).
    pub fn has_buildup(&self, type_id: &str) -> bool {
        self.controls.contains_key(&type_id.to_ascii_uppercase())
    }

    /// Install one control directly (test fixtures without assets).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn insert_for_test(&mut self, type_id: &str, control: [i32; 3]) {
        self.controls.insert(type_id.to_ascii_uppercase(), control);
    }

    /// Install one Ares sale rate directly (test fixtures without assets).
    #[cfg(all(feature = "ares", any(test, feature = "test-support")))]
    pub(crate) fn insert_sell_rate_for_test(&mut self, type_id: &str, rate: i32) {
        self.sell_rates.insert(type_id.to_ascii_uppercase(), rate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The native slice over retail `BuildupTime=.06` (as `ReadDouble`
    /// stores it, a widened float), the constructor's `.05` and a gate's
    /// `GateStages + 1` (`tools/spatial_oracle/building_construction.json`
    /// `rate` rows).
    #[test]
    fn buildup_control_matches_the_original_rate_slice() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_construction.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["rate"].as_array().unwrap() {
            let bits = match row["buildup_time"].as_str().unwrap() {
                "retail" => 0x3FAE_B851_E000_0000,
                "default" => 0x3FA9_9999_9999_999A,
                other => panic!("buildup time {other}"),
            };
            let expected: [i32; 3] = serde_json::from_value(row["control"].clone()).unwrap();
            let actual = match row["frames"].as_i64() {
                // No SHP: the constructor's control stays.
                None => NO_BUILDUP,
                Some(frames) => {
                    let gate =
                        (row["gate"] == true).then(|| row["stages"].as_i64().unwrap() as i32);
                    buildup_control(frames as i16, gate, NativeF64Bits::from_bits(bits))
                }
            };
            assert_eq!(actual, expected, "{row}");
            compared += 1;
        }
        assert_eq!(compared, 24);
    }

    /// Ares divides before it multiplies under the process's chop: the
    /// constructor's `.05` over 3 frames gives 14 where the original order
    /// gives 15, and a 2-minute `SellTime` 599, not 600 (`1000F450`).
    #[cfg(feature = "ares")]
    #[test]
    fn ares_buildup_frames_divide_first_and_rate_the_sale() {
        let default = NativeF64Bits::from_bits(0x3FA9_9999_9999_999A);
        assert_eq!(buildup_control(6, None, default), [0, 3, 15]);
        let two_minutes = NativeF64Bits::from_bits(2.0f64.to_bits());
        assert_eq!(
            ares_buildup_frames(6, None, default, two_minutes),
            ([0, 3, 14], 599)
        );
        assert_eq!(
            ares_buildup_frames(1, None, default, two_minutes),
            ([0, 0, 1], 1)
        );
    }

    #[test]
    fn buildup_names_take_the_theater_letter_then_the_generic_one() {
        assert_eq!(
            crate::rules::art_data::building_shp_names("YAREFNMK", "TEMPERATE"),
            ["YTREFNMK.SHP".to_string(), "YGREFNMK.SHP".to_string()]
        );
        assert_eq!(
            crate::rules::art_data::building_shp_names("gacnstmk", "SNOW"),
            ["GACNSTMK.SHP".to_string(), "GGCNSTMK.SHP".to_string()]
        );
        // Not a G/N/C/Y + A/T name: tried as it is, then with G.
        assert_eq!(
            crate::rules::art_data::building_shp_names("NXPOWRMK", "TEMPERATE"),
            ["NXPOWRMK.SHP".to_string(), "NGPOWRMK.SHP".to_string()]
        );
        // An unknown theater: no substitution.
        assert_eq!(
            crate::rules::art_data::building_shp_names("GTCNSTMK", "NONE"),
            ["GTCNSTMK.SHP".to_string(), "GGCNSTMK.SHP".to_string()]
        );
    }
}
