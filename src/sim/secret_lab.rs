//! Secret Lab startup reward and House admission. Original gamemd.exe
//! AssignSecretLabProduction68C050, GetSecretProduction459840 and
//! CanBuild4F78E4..4F7925. Executed controls: tools/can_build_secret_lab.json.
//! With `ares`, a lab also rolls its reward when it is placed (Ares 3.0
//! UpdateSecretLab 10013700, hook 0x445F80): docs/ares-reverse-engineering.md.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::StringInterner;
use crate::sim::rng::SimRng;
use crate::sim::world::ObjectSubstrate;
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
    pub(crate) fn assign_secret_lab_production(
        interner: &mut StringInterner,
        scenario_rng: &mut SimRng,
        substrate: &mut ObjectSubstrate,
        rules: &RuleSet,
    ) {
        let labs = substrate.entities.par_select(|_, entity| {
            entity.category() == EntityCategory::Structure
                && interner
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
            let drawn = scenario_rng.next_range_u32_inclusive(0, high) as usize;
            let (category, name) = choices[drawn];
            let id = interner.intern(name);
            substrate
                .entities
                .get_mut(lab)
                .expect("registered lab")
                .assign_secret_lab_fallback((category, id));
        }
    }
}

#[cfg(feature = "ares")]
impl Simulation {
    /// Ares 3.0 `BuildingExt::ExtData::UpdateSecretLab @ 10013700`, which
    /// BuildingClass_Place (`100100B0`, hook `0x445F80`) calls for a
    /// `SecretLab=` type at every Grand_Opening entry, captures included,
    /// before it grants factory plans. The lab of a House whose Country is
    /// not `MultiplayPassive`, of a type without a fixed `SecretInfantry=`,
    /// `SecretUnit=` or `SecretBuilding=`, rolls while it has not rolled
    /// (BuildingExt `+0x50`), or at every call with
    /// `SecretLab.GenerateOnCapture`. The options are
    /// `SecretLab.PossibleBoons` when set, else the Rules SecretInfantry,
    /// SecretUnits and SecretBuildings in that order, each kept when its
    /// `SecretLab.RequiredHouses`/`.ForbiddenHouses` admit the owner's
    /// Country and RequirementsMet (`10021FF0`) answers Forbidden or
    /// Incomplete (AddToOptions `10011340`). One Scenario RandomRanged(0,
    /// count - 1) picks Building+6F4 and sets `+0x50`; no option draws
    /// nothing and leaves the lab unrolled. The stock startup draw
    /// (`68C050`) runs unchanged, so a lab opened before it is overwritten.
    pub(crate) fn ares_update_secret_lab(&mut self, id: u64, rules: &RuleSet) {
        use crate::sim::production::RequirementStatus;
        let sim: &Simulation = self;
        let Some(lab) = sim.substrate.entities.get(id) else {
            return;
        };
        if lab.category() != EntityCategory::Structure {
            return;
        }
        let Some(object) = sim.object_type(lab.type_ref(), rules) else {
            return;
        };
        if !object.secret_lab().enabled() {
            return;
        }
        let owner = lab.owner();
        let Some(house) = sim.houses.get(&owner) else {
            return;
        };
        if house.multiplay_passive() || object.secret_lab().production_override().is_some() {
            return;
        }
        let ext = object.building_type_ext();
        if lab.ares_secret_lab_placed() && !ext.secret_lab_generate_on_capture() {
            return;
        }
        let house_bit = crate::sim::ai_buildable::house_country_bit(
            rules,
            sim.interner.resolve(house.house_type_id()),
        );
        let candidates: Vec<&ObjectType> = match ext.secret_lab_possible_boons() {
            Some(boons) => boons
                .iter()
                .filter_map(|boon| rules.object_in_category(boon.category, &boon.id))
                .collect(),
            None => [
                ObjectCategory::Infantry,
                ObjectCategory::Vehicle,
                ObjectCategory::Building,
            ]
            .into_iter()
            .zip(rules.general.secret_lab_choices())
            .flat_map(|(category, list)| {
                list.iter()
                    .filter_map(move |name| rules.object_in_category(category, name))
            })
            .collect(),
        };
        let options: Vec<&ObjectType> = candidates
            .into_iter()
            .filter(|option| {
                option
                    .techno_combat()
                    .secret_lab_houses()
                    .admit(rules, house_bit)
                    && matches!(
                        crate::sim::production::requirements_met(sim, rules, owner, house, option),
                        RequirementStatus::Forbidden | RequirementStatus::Incomplete
                    )
            })
            .collect();
        let Some(high) = options.len().checked_sub(1) else {
            return;
        };
        let high = u32::try_from(high).expect("secret lab options fit native count");
        let boon = options[self.scenario_rng.next_range_u32_inclusive(0, high) as usize];
        let boon = (boon.category, self.interner.intern(&boon.id));
        let lab = self.substrate.entities.get_mut(id).expect("rolled lab");
        lab.assign_secret_lab_fallback(boon);
        lab.mark_ares_secret_lab_placed();
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
        let corpus = native();
        for row in corpus["assignment"]
            .as_array()
            .unwrap()
            .iter()
            .chain(corpus["authored_startup"].as_array().unwrap())
        {
            let mut ini = String::from("[LAB]\nStrength=100\nSecretLab=yes\n[General]\n");
            // The choices are registered types, which Ares's Find-only read
            // (Buf_General 1005BB00, hook 0x66D55E) needs.
            let mut registry = String::new();
            for (slot, (key, prefix, list)) in [
                ("SecretInfantry", "infantry", "InfantryTypes"),
                ("SecretUnits", "unit", "VehicleTypes"),
                ("SecretBuildings", "building", "BuildingTypes"),
            ]
            .into_iter()
            .enumerate()
            {
                let count = row["counts"][slot].as_u64().unwrap();
                let names: Vec<_> = (0..count).map(|index| format!("{prefix}{index}")).collect();
                let first = usize::from(list == "BuildingTypes");
                registry += &format!("[{list}]\n");
                if first == 1 {
                    registry += "0=LAB\n";
                }
                for (index, name) in names.iter().enumerate() {
                    registry += &format!("{}={name}\n", index + first);
                }
                if !names.is_empty() {
                    ini += &format!("{key}={}\n", names.join(","));
                }
            }
            ini = registry + &ini;
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
            let (_, draws) = trace_draws(|| {
                if let Some(gate) = row.get("startup_gate") {
                    sim.install_session_mode(
                        crate::sim::scenario_session::ScenarioGameMode::from_game_mode(
                            gate["game_mode"].as_i64().unwrap() as i32,
                        ),
                    );
                    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
                        &IniFile::empty(),
                        None,
                    );
                    let roster = crate::map::houses::HouseRoster::default();
                    // The native control executes the admission gate then,
                    // only when admitted, the unchanged assignment body.
                    // No terrain/ore exists in either bounded fixture.
                    sim.finalize_scenario_post_map(
                        crate::sim::scenario_post_map::ScenarioPostMapInput {
                            map_width: 0,
                            map_height: 0,
                            fill_silos: false,
                            rules: &rules,
                            overlay_registry: &overlays,
                            house_roster: &roster,
                            skirmish_session: None,
                            campaign_map: None,
                        },
                    );
                } else {
                    Simulation::assign_secret_lab_production(
                        &mut sim.interner,
                        &mut sim.scenario_rng,
                        &mut sim.substrate,
                        &rules,
                    );
                }
            });
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
        Simulation::assign_secret_lab_production(
            &mut sim.interner,
            &mut sim.scenario_rng,
            &mut sim.substrate,
            &rules,
        );
        let owner = sim.interner.get("H").unwrap();
        assert!(house_grants(
            &sim,
            &rules,
            &sim.houses[&owner],
            rules.object("Q").unwrap()
        ));
    }

    /// A human House `H` of Country `C0` (tech level 10) beside `C1`, and
    /// boon candidates: OK and TANK (TechLevel -1, Incomplete for a human),
    /// BUILT (Complete), RH (Forbidden by the stock RequiredHouses), NOTC0
    /// and FORB (refused by the Secret Lab house lists).
    fn country_world(lab: &str, extra: &str) -> (Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=C0\n1=C1\n\
             [InfantryTypes]\n0=OK\n1=BUILT\n2=NOTC0\n3=FORB\n4=RH\n\
             [VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=LAB\n\
             [LAB]\nStrength=100\nSecretLab=yes\n{lab}\n\
             [OK]\nTechLevel=-1\n[TANK]\nTechLevel=-1\n[BUILT]\nTechLevel=1\n\
             [NOTC0]\nTechLevel=-1\nSecretLab.RequiredHouses=C1\n\
             [FORB]\nTechLevel=-1\nSecretLab.ForbiddenHouses=C0\n\
             [RH]\nTechLevel=1\nRequiredHouses=C1\n{extra}"
        )))
        .unwrap();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("H");
        let country = sim.interner.intern("C0");
        sim.houses
            .register(HouseState::new(owner, 0, Some(country), true, 0, 10));
        (sim, rules)
    }

    /// A constructed lab marked placed: Grand_Opening runs its entry hooks,
    /// then the stock ActuallyPlaced return.
    fn placed_limbo_lab(sim: &mut Simulation, rules: &RuleSet) -> u64 {
        let lab = sim
            .construct_object_limbo_at_height(
                "LAB",
                "H",
                8,
                8,
                0,
                0,
                rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(lab)
            .unwrap()
            .building_art
            .set_actually_placed_for_test(true);
        lab
    }

    /// Grand_Opening (`captured` for ChangeOwnership's call), counting draws.
    fn open(sim: &mut Simulation, rules: &RuleSet, lab: u64, captured: bool) -> usize {
        trace_draws(|| {
            sim.grand_opening(
                lab,
                captured,
                false,
                rules,
                None,
                crate::sim::world::FrameEffects::default(),
            )
        })
        .1
        .len()
    }

    fn boon(sim: &Simulation, lab: u64) -> Option<&str> {
        sim.substrate
            .entities
            .get(lab)
            .unwrap()
            .secret_lab_fallback()
            .map(|(_, name)| sim.interner.resolve(name))
    }

    /// Stock Grand_Opening rolls no reward and Post_Map_Init's draw
    /// (68C050) assigns it in every build. Ares's Place roll (10013700)
    /// comes on top, here from the Rules lists (PossibleBoons unset).
    #[test]
    fn placement_rolls_only_with_ares_and_the_startup_draw_remains() {
        let (mut sim, rules) =
            country_world("", "[General]\nSecretInfantry=OK\nSecretUnits=TANK\n");
        let lab = placed_limbo_lab(&mut sim, &rules);
        let ares = cfg!(feature = "ares");
        assert_eq!(open(&mut sim, &rules, lab, false), usize::from(ares));
        assert_eq!(boon(&sim, lab).is_some(), ares);
        let mut expected = sim.scenario_rng.clone();
        let pick = ["OK", "TANK"][expected.next_range_u32_inclusive(0, 1) as usize];
        let ((), draws) = trace_draws(|| {
            Simulation::assign_secret_lab_production(
                &mut sim.interner,
                &mut sim.scenario_rng,
                &mut sim.substrate,
                &rules,
            )
        });
        assert_eq!(draws.len(), 1);
        assert_eq!(boon(&sim, lab), Some(pick));
    }

    /// AddToOptions (10011340) keeps the PossibleBoons its owner's Country
    /// passes and RequirementsMet (10021FF0) answers Forbidden (RH) or
    /// Incomplete (OK); a Complete type (BUILT), the house lists and an
    /// unknown name drop out, and the Rules lists are not asked. One
    /// Scenario RandomRanged(0, 1) picks.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_placement_rolls_among_possible_boons_the_owner_cannot_build() {
        let (mut sim, rules) = country_world(
            "SecretLab.PossibleBoons=OK,BUILT,NOTC0,Missing,FORB,RH",
            "[General]\nSecretUnits=TANK\n",
        );
        let lab = placed_limbo_lab(&mut sim, &rules);
        let mut expected = sim.scenario_rng.clone();
        let pick = ["OK", "RH"][expected.next_range_u32_inclusive(0, 1) as usize];
        assert_eq!(open(&mut sim, &rules, lab, false), 1);
        assert_eq!(boon(&sim, lab), Some(pick));
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            expected.native_state_hex()
        );
        assert!(
            sim.substrate
                .entities
                .get(lab)
                .unwrap()
                .ares_secret_lab_placed()
        );
    }

    /// A rolled lab (BuildingExt+0x50) rolls again on capture's
    /// Grand_Opening(1) only with `SecretLab.GenerateOnCapture`. Its owner's
    /// lab list may grant the current boon (Overridden), so three options
    /// keep at least two and the roll draws.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_capture_rolls_again_only_with_generate_on_capture() {
        for regenerate in ["no", "yes"] {
            let (mut sim, rules) = country_world(
                &format!(
                    "SecretLab.PossibleBoons=OK,RH,TANK\nSecretLab.GenerateOnCapture={regenerate}"
                ),
                "",
            );
            let lab = sim.spawn_object("LAB", "H", 10, 10, 0, &rules).unwrap();
            open(&mut sim, &rules, lab, false);
            assert!(
                sim.substrate
                    .entities
                    .get(lab)
                    .unwrap()
                    .ares_secret_lab_placed()
            );
            assert_eq!(
                open(&mut sim, &rules, lab, true) > 0,
                regenerate == "yes",
                "GenerateOnCapture={regenerate}"
            );
        }
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
