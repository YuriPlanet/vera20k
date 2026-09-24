//! Cell481A00 pickup preparation. The production movement host and effect
//! dispatch remain under implementation; these routines do not consume crates
//! until their caller can complete the selected effect.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::{powerups::POWERUP_COUNT, ruleset::RuleSet};
use crate::sim::rng::SimRng;

/// Transient selection before multiplayer eligibility checks. Solo money is
/// native's local override (zero means the Money arm still draws its amount).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PickupSelection {
    pub powerup: usize,
    pub solo_money: i32,
}

/// Cell481AC8..481B99: stored selections below19 spend no draw. Otherwise sum
/// all nineteen signed weights and use Scenario's inclusive RandomRanged. Solo
/// image overrides then run in Silver/Wood/Water order, independently: retail
/// aliases Silver and Wood to the same OverlayType, so the later write wins.
///
/// Call only after the null/overlay/passive guards and synchronous crate trigger.
/// Multiplayer eligibility and the post-removal Squad fallback follow later.
/// Original-code comparisons: tools/spatial_oracle/crate_pickup.json.
pub(super) fn select_pickup_outcome(
    rng: &mut SimRng,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    overlay_id: u8,
    stored_selection: u8,
    multiplayer: bool,
) -> PickupSelection {
    let mut powerup = usize::from(stored_selection);
    if powerup >= POWERUP_COUNT {
        let total = rules
            .powerups
            .weights
            .iter()
            .copied()
            .fold(0_i32, i32::wrapping_add);
        let draw = rng.next_range_i32_inclusive(1, total);
        let mut cumulative = 0_i32;
        powerup = rules
            .powerups
            .weights
            .iter()
            .position(|weight| {
                cumulative = cumulative.wrapping_add(*weight);
                draw <= cumulative
            })
            .unwrap_or(POWERUP_COUNT);
    }
    let mut solo_money = 0;
    if !multiplayer && stored_selection == 0 {
        let crate_rules = &rules.crate_rules;
        solo_money = crate_rules.solo_crate_money;
        for (image, selection) in [
            (&crate_rules.crate_img, crate_rules.silver_crate),
            (&crate_rules.wood_crate_img, crate_rules.wood_crate),
            (&crate_rules.water_crate_img, crate_rules.water_crate),
        ] {
            if image.as_deref().and_then(|name| registry.id_for_name(name)) == Some(overlay_id) {
                powerup = selection;
            }
        }
    }
    PickupSelection {
        powerup,
        solo_money,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{crate_registry, crate_ruleset};
    use super::*;

    /// Entire Scenario RNG state, not merely the chosen result: rejection draws
    /// change subsequent production RNG even when two selections agree.
    #[test]
    fn selection_and_rng_match_original_pickup_prefix() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/crate_pickup.json"
        ))
        .unwrap();
        let mut compared = 0;
        for row in rows {
            if row["prepared"].as_array().unwrap().is_empty() {
                continue;
            }
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut rules = crate_ruleset("");
            rules.powerups.weights = [
                20, 20, 10, 0, 0, 0, 0, 0, 10, 10, 10, 10, 0, 0, 20, 0, 0, 0, 0,
            ];
            if let Some(weights) = input["weights"].as_array() {
                rules.powerups.weights =
                    std::array::from_fn(|i| weights[i].as_i64().unwrap() as i32);
            }
            let choices = input["solo_choices"].as_array().map_or([2, 10, 0], |v| {
                std::array::from_fn(|i| v[i].as_u64().unwrap() as usize)
            });
            rules.crate_rules.silver_crate = choices[0];
            rules.crate_rules.wood_crate = choices[1];
            rules.crate_rules.water_crate = choices[2];
            rules.crate_rules.solo_crate_money =
                input["solo_money"].as_i64().unwrap_or(2000) as i32;
            if input["different_image"] == true {
                rules.crate_rules.wood_crate_img = Some("SILVER".into());
            }
            if input["same_images"] == true {
                rules.crate_rules.crate_img = Some("WOOD".into());
                rules.crate_rules.water_crate_img = Some("WOOD".into());
            }
            let registry = crate_registry();
            let mut rng = SimRng::new(input["seed"].as_u64().unwrap_or(31));
            assert_eq!(
                rng.native_state_hex(),
                row["rng_before"].as_str().unwrap(),
                "{name}"
            );
            let selection = select_pickup_outcome(
                &mut rng,
                &rules,
                &registry,
                registry.id_for_name("WOOD").unwrap(),
                input["selection"].as_u64().unwrap_or(10) as u8,
                input["mode"].as_i64() != Some(0),
            );
            assert_eq!(
                serde_json::json!([[selection.powerup, selection.solo_money]]),
                row["prepared"],
                "{name}"
            );
            assert_eq!(
                rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{name}"
            );
            compared += 1;
        }
        assert_eq!(compared, 24);
    }
}
