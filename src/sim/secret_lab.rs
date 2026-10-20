//! Secret Lab startup reward and House admission. Original gamemd.exe
//! AssignSecretLabProduction68C050, GetSecretProduction459840 and
//! CanBuild4F78E4..4F7925. Executed controls: tools/can_build_secret_lab.json.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

/// Type overrides have Infantry → Unit → Building priority, then Building+6F4.
/// Neither this getter nor the House list walk filters health, limbo or power.
fn production<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    lab: &GameEntity,
) -> Option<(ObjectCategory, &'a str)> {
    sim.object_type(lab.type_ref(), rules)?
        .secret_lab()
        .production_override()
        .or_else(|| {
            lab.secret_lab_fallback()
                .map(|(category, id)| (category, sim.interner.resolve(id)))
        })
}

pub(crate) fn house_grants(
    sim: &Simulation,
    rules: &RuleSet,
    house: &HouseState,
    requested: &ObjectType,
) -> bool {
    house.base_projection().secret_labs().iter().any(|&id| {
        sim.substrate
            .entities
            .get(id)
            .and_then(|lab| production(sim, rules, lab))
            .is_some_and(|(category, name)| {
                category == requested.category && name.eq_ignore_ascii_case(&requested.id)
            })
    })
}

impl Simulation {
    /// Fresh scenario only, after Post_Map_Init's alliances. Type initialization
    /// registers every SecretLab in global8B41E0, including limbo buildings;
    /// destruction removes it. Stored constructor IDs preserve that order.
    pub(crate) fn assign_secret_lab_production(&mut self, rules: &RuleSet) {
        let labs = self.substrate.entities.par_select(|_, entity| {
            entity.category() == EntityCategory::Structure
                && self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.secret_lab().enabled())
        });
        let lists = rules.general.secret_lab_choices();
        let total: usize = lists.iter().map(Vec::len).sum();
        if labs.is_empty() || total < labs.len() {
            return;
        }
        let choices: Vec<_> = [
            ObjectCategory::Infantry,
            ObjectCategory::Vehicle,
            ObjectCategory::Building,
        ]
        .into_iter()
        .zip(lists)
        .flat_map(|(category, list)| list.iter().map(move |name| (category, name)))
        .collect();
        // Native removes a slot from a temporary index vector but then uses
        // the drawn POSITION, not its value, against the original type arrays
        // (68C112..68C1B5). Thus only the shrinking count matters, and duplicate
        // grants are possible. Override types still spend this startup draw.
        for (index, lab) in labs.into_iter().enumerate() {
            let high =
                u32::try_from(total - index - 1).expect("secret production list fits native count");
            let drawn = self.scenario_rng.next_range_u32_inclusive(0, high) as usize;
            let (category, name) = choices[drawn];
            let id = self.interner.intern(name);
            self.substrate
                .entities
                .get_mut(lab)
                .expect("registered lab")
                .assign_secret_lab_fallback((category, id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::rng::{SimRng, trace_draws};
    use serde_json::Value;

    fn native() -> Value {
        serde_json::from_str(crate::test_fixture::text("tools/can_build_secret_lab.json")).unwrap()
    }

    fn world(rules: &RuleSet, earlier_names: &[&str]) -> Simulation {
        let mut sim = Simulation::new();
        for name in earlier_names {
            sim.interner.intern(name);
        }
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let owner = sim.interner.intern("H");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 1000, 10));
        sim
    }

    #[test]
    fn secret_lab_assignment_matches_original_rewards_and_all_three_full_rng_states() {
        for row in native()["assignment"].as_array().unwrap() {
            let mut ini = String::from(
                "[BuildingTypes]\n0=LAB\n[LAB]\nStrength=100\nSecretLab=yes\n[General]\n",
            );
            for (slot, (key, prefix)) in [
                ("SecretInfantry", "infantry"),
                ("SecretUnits", "unit"),
                ("SecretBuildings", "building"),
            ]
            .into_iter()
            .enumerate()
            {
                let count = row["counts"][slot].as_u64().unwrap();
                let names: Vec<_> = (0..count).map(|index| format!("{prefix}{index}")).collect();
                if !names.is_empty() {
                    ini += &format!("{key}={}\n", names.join(","));
                }
            }
            if let Some(overridden) = row["override"].as_array() {
                let family = overridden[0].as_u64().unwrap() as usize;
                let (key, prefix) = [
                    ("SecretInfantry", "infantry"),
                    ("SecretUnit", "unit"),
                    ("SecretBuilding", "building"),
                ][family];
                ini = ini.replacen(
                    "[General]",
                    &format!("{key}={prefix}{}\n[General]", overridden[1]),
                    1,
                );
            }
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            let mut sim = world(&rules, &[]);
            let labs: Vec<_> = (0..row["lab_count"].as_u64().unwrap())
                .map(|index| {
                    sim.spawn_object("LAB", "H", 10 + index as u16, 10, 0, &rules)
                        .unwrap()
                })
                .collect();
            // Startup's global list includes constructed limbo buildings.
            if let Some(&first) = labs.first() {
                sim.techno_limbo_with_rules(first, &rules, None);
            }
            if let Some(retained) = row["initial_fallback"].as_str() {
                let id = sim.interner.intern(retained);
                for &lab in &labs {
                    sim.substrate
                        .entities
                        .get_mut(lab)
                        .unwrap()
                        .assign_secret_lab_fallback((ObjectCategory::Infantry, id));
                }
            }
            sim.scenario_rng = SimRng::from_native_state_hex_for_test(
                row["rng_before"]["scenario"].as_str().unwrap(),
            );
            sim.main_rng =
                SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                    .into();
            sim.mapgen_rng = SimRng::from_native_state_hex_for_test(
                row["rng_before"]["mapgen"].as_str().unwrap(),
            );
            let (_, draws) = trace_draws(|| sim.assign_secret_lab_production(&rules));
            assert_eq!(
                draws.len() as u64,
                row["raw_store_count"].as_u64().unwrap(),
                "{}",
                row["name"]
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"]["scenario"],
                "{}",
                row["name"]
            );
            assert_eq!(
                sim.main_rng.native_state_hex(),
                row["rng_after"]["main"],
                "{}",
                row["name"]
            );
            assert_eq!(
                sim.mapgen_rng.native_state_hex(),
                row["rng_after"]["mapgen"],
                "{}",
                row["name"]
            );
            for (index, &lab) in labs.iter().enumerate() {
                let lab = sim.substrate.entities.get(lab).unwrap();
                let fallback = lab
                    .secret_lab_fallback()
                    .map(|(_, id)| sim.interner.resolve(id));
                assert_eq!(
                    fallback,
                    row["assignments"][index].as_str(),
                    "{} lab{index}",
                    row["name"]
                );
                assert_eq!(
                    production(&sim, &rules, lab).map(|(_, name)| name),
                    row["getters"][index].as_str(),
                    "{} lab{index}",
                    row["name"]
                );
            }
        }
    }

    /// Authored map strings can enter the interner before canonical rule IDs.
    /// Original pointer comparison4F791B is independent of that display spelling;
    /// native case-insensitive factory lookup is executed by the reader corpus.
    #[test]
    fn secret_lab_fallback_grants_after_an_earlier_lowercase_map_identity() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nSecretInfantry=Q\n[InfantryTypes]\n0=Q\n[Q]\nTechLevel=-1\n\
             [BuildingTypes]\n0=LAB\n[LAB]\nStrength=100\nSecretLab=yes\n",
        ))
        .unwrap();
        let mut sim = world(&rules, &["q"]);
        sim.spawn_object("LAB", "H", 10, 10, 0, &rules).unwrap();
        sim.assign_secret_lab_production(&rules);
        let owner = sim.interner.get("H").unwrap();
        assert!(house_grants(
            &sim,
            &rules,
            &sim.houses[&owner],
            rules.object("Q").unwrap()
        ));
    }

    #[test]
    fn secret_lab_getter_matches_original_pointer_priority() {
        for row in native()["getter"].as_array().unwrap() {
            let mut ini =
                String::from("[BuildingTypes]\n0=LAB\n[LAB]\nStrength=100\nSecretLab=yes\n");
            for (key, field) in [
                ("SecretInfantry", "infantry"),
                ("SecretUnit", "unit"),
                ("SecretBuilding", "building"),
            ] {
                if let Some(value) = row["overrides"][field].as_str() {
                    ini += &format!("{key}={value}\n");
                }
            }
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            let mut sim = world(&rules, &[]);
            let lab = sim.spawn_object("LAB", "H", 10, 10, 0, &rules).unwrap();
            if let Some(fallback) = row["fallback"].as_str() {
                let id = sim.interner.intern(fallback);
                sim.substrate
                    .entities
                    .get_mut(lab)
                    .unwrap()
                    .assign_secret_lab_fallback((ObjectCategory::Infantry, id));
            }
            assert_eq!(
                production(&sim, &rules, sim.substrate.entities.get(lab).unwrap())
                    .map(|(_, name)| name),
                row["result"].as_str(),
                "{}",
                row["name"]
            );
        }
    }
}
