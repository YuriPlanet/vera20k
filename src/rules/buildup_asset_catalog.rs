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
        // `_ftol` of an unrepresentable value leaves the integer indefinite.
        .map_or(i32::MIN, |rate| rate as i32);
    [0, count, rate]
}

/// The two names the Buildup lookup tries (`0x005F96B0`, then `0x005F9710` on
/// the same buffer): the theater letter substituted into a name whose first
/// two letters are `G/N/C/Y` and `A/T` (case-insensitive), else the name
/// itself; then that name with `G` as its second letter.
pub fn buildup_shp_candidates(buildup: &str, theater_letter: Option<char>) -> [String; 2] {
    let mut name: Vec<char> = buildup.to_ascii_uppercase().chars().collect();
    if let (Some(letter), [first, second, ..]) = (theater_letter, name.as_slice())
        && matches!(first, 'G' | 'N' | 'C' | 'Y')
        && matches!(second, 'A' | 'T')
    {
        name[1] = letter;
    }
    let first: String = name.iter().collect();
    if name.len() >= 2 {
        name[1] = 'G';
    }
    let second: String = name.iter().collect();
    [format!("{first}.SHP"), format!("{second}.SHP")]
}

/// The construction control of every building type whose Buildup SHP the
/// active theater resolves. Keys are uppercase type IDs; a `BTreeMap` keeps
/// binding and hashing order deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct BuildupAssetCatalog {
    controls: BTreeMap<String, [i32; 3]>,
}

impl BuildupAssetCatalog {
    /// Bind every BuildingType's Buildup SHP. A type whose art names none, or
    /// whose SHP the theater lacks, keeps [`NO_BUILDUP`].
    pub fn bind(rules: &RuleSet, asset_manager: &AssetManager, theater_name: &str) -> Self {
        let mut catalog = Self::default();
        let buildup_time = NativeF64Bits::from_bits(rules.general.buildup_time.to_bits());
        let theater_letter = crate::rules::art_data::theater_letter(theater_name);
        for object in rules
            .all_objects()
            .filter(|object| object.category == ObjectCategory::Building)
        {
            let Some(art) = rules
                .art_registry
                .resolve_metadata_entry(&object.id, &object.image)
            else {
                continue;
            };
            let Some(buildup) = art.buildup.as_deref() else {
                continue;
            };
            let Some(data) = buildup_shp_candidates(buildup, theater_letter)
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
            catalog.controls.insert(
                object.id.to_ascii_uppercase(),
                buildup_control(raw, gate_stages, buildup_time),
            );
        }
        catalog
    }

    /// The construction control of `type_id` ([`NO_BUILDUP`] when unbound).
    pub fn control(&self, type_id: &str) -> [i32; 3] {
        self.controls
            .get(&type_id.to_ascii_uppercase())
            .copied()
            .unwrap_or(NO_BUILDUP)
    }

    /// Install one control directly (test fixtures without assets).
    #[cfg(test)]
    pub(crate) fn insert_for_test(&mut self, type_id: &str, control: [i32; 3]) {
        self.controls.insert(type_id.to_ascii_uppercase(), control);
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
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_construction.json"
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

    #[test]
    fn buildup_names_take_the_theater_letter_then_the_generic_one() {
        assert_eq!(
            buildup_shp_candidates("YAREFNMK", Some('T')),
            ["YTREFNMK.SHP".to_string(), "YGREFNMK.SHP".to_string()]
        );
        assert_eq!(
            buildup_shp_candidates("gacnstmk", Some('A')),
            ["GACNSTMK.SHP".to_string(), "GGCNSTMK.SHP".to_string()]
        );
        // Not a G/N/C/Y + A/T name: tried as it is, then with G.
        assert_eq!(
            buildup_shp_candidates("NXPOWRMK", Some('T')),
            ["NXPOWRMK.SHP".to_string(), "NGPOWRMK.SHP".to_string()]
        );
        // No theater: no substitution.
        assert_eq!(
            buildup_shp_candidates("GTCNSTMK", None),
            ["GTCNSTMK.SHP".to_string(), "GGCNSTMK.SHP".to_string()]
        );
    }
}
