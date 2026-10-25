//! Deterministic generation of a fresh non-human House BasePlan.
//!
//! This is the bounded `AI_RecalcBuildOptions` transaction only. Runtime plan
//! insertion, selection, placement, takeover, and Recenter remain separate.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup};
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_buildable::{
    CountryBuildMasks, candidate_allowed, first_buildable_from_array,
    first_owner_compatible_harvester,
};
use crate::sim::base_plan::BasePlanState;
use crate::sim::house_state::HouseDifficulty;
use crate::sim::rng::SimRng;
use crate::sim::world::TypeTechLevels;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecalcPreflightError {
    UnknownCountry,
    MissingRefineryDifficulty,
    MissingDefenseDifficulty,
}

/// Validate only the directly indexed malformed-vector branches before an MCV
/// can be destructively removed. This is a VERA-internal safety boundary; the
/// native code performs unchecked DynamicVector indexing.
pub(crate) fn preflight_recalc(
    rules: &RuleSet,
    country_name: &str,
    side_index: i32,
    difficulty: HouseDifficulty,
) -> Result<(), RecalcPreflightError> {
    rules
        .country_index(country_name)
        .ok_or(RecalcPreflightError::UnknownCountry)?;
    let bit = rules.country_owner_mask(country_name);
    let has_harvester = first_owner_compatible_harvester(rules, bit).is_some();
    let refinery_counts = if has_harvester {
        &rules.ai_extra_refineries
    } else {
        &rules.ai_slave_miner_number
    };
    refinery_counts
        .get(difficulty.table_index())
        .ok_or(RecalcPreflightError::MissingRefineryDifficulty)?;

    let defense_counts = match side_index {
        0 => Some(&rules.allied_base_defense_counts),
        1 => Some(&rules.soviet_base_defense_counts),
        2 => Some(&rules.third_base_defense_counts),
        _ => None,
    };
    if defense_counts.is_some_and(|counts| counts.get(difficulty.table_index()).is_none()) {
        return Err(RecalcPreflightError::MissingDefenseDifficulty);
    }
    Ok(())
}

/// Replace one empty House plan with native Recalc output.
///
/// gamemd-derived: `HouseClass__AI_RecalcBuildOptions @ 0x005054B0` and its
/// prerequisite helper `FUN_00505360 @ 0x00505360`. Refinery and defense
/// insertions consume the shared `ScenarioClass+0x218` ranged RNG in the
/// exact current-vector ranges described by the native body.
pub(crate) fn recalc_base_plan(
    plan: &mut BasePlanState,
    rules: &RuleSet,
    type_tech_levels: &TypeTechLevels,
    country_name: &str,
    side_index: i32,
    difficulty: HouseDifficulty,
    tech_level: i32,
    super_weapons: bool,
    scenario_rng: &mut SimRng,
) {
    debug_assert!(preflight_recalc(rules, country_name, side_index, difficulty).is_ok());

    let country = CountryBuildMasks::for_country(rules, country_name);
    let mut eligible: Vec<&ObjectType> = rules
        .building_ids
        .iter()
        .filter_map(|type_id| rules.object_in_category(ObjectCategory::Building, type_id))
        .filter(|candidate| {
            recalc_candidate_allowed(
                candidate,
                type_tech_levels,
                country,
                side_index,
                tech_level,
                super_weapons,
                rules,
            )
        })
        // NPSE AI_RecalcBuildOptions5056C1 -> C2C331 repeats the
        // eligible append, before seed selection505714 and prerequisites.
        .flat_map(|candidate| std::iter::repeat_n(candidate, planning_repeats(candidate)))
        .collect();
    let mut selected = vec![false; eligible.len()];
    let mut priority: Vec<&ObjectType> = Vec::new();

    if let Some(build_const) = first_buildable_from_array(
        rules,
        &rules.build_const_types,
        country_name,
        side_index,
        super_weapons,
    ) && let Some(index) = eligible
        .iter()
        .position(|candidate| candidate.base_plan_type_index == build_const.base_plan_type_index)
    {
        selected[index] = true;
        priority.push(build_const);
    }

    let build_power = first_buildable_from_array(
        rules,
        &rules.build_power_types,
        country_name,
        side_index,
        super_weapons,
    )
    .expect("active Recalc BuildPower list resolves a buildable type");
    priority.push(build_power);

    move_eligible_seed(
        &mut eligible,
        &mut selected,
        first_buildable_from_array(
            rules,
            &rules.build_barracks_types,
            country_name,
            side_index,
            super_weapons,
        ),
        0,
    );
    move_eligible_seed(
        &mut eligible,
        &mut selected,
        first_buildable_from_array(
            rules,
            &rules.build_weapons_types,
            country_name,
            side_index,
            super_weapons,
        ),
        1,
    );

    let mut remaining = (eligible.len() as i32).wrapping_sub(1);
    while remaining > 0 {
        let pass_start_len = priority.len();
        let mut progressed = false;
        let mut last_unselected = None;
        for index in 0..eligible.len() {
            if selected[index] {
                continue;
            }
            last_unselected = Some(index);
            let candidate = eligible[index];
            if !candidate.id.eq_ignore_ascii_case("GAPLUG")
                && prerequisites_satisfied(candidate, &priority[..pass_start_len], rules)
            {
                priority.push(candidate);
                selected[index] = true;
                remaining = remaining.wrapping_sub(1);
                progressed = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if remaining == 0 {
            break;
        }
        if !progressed {
            let Some(index) = last_unselected else {
                break;
            };
            priority.push(eligible[index]);
            selected[index] = true;
            remaining = remaining.wrapping_sub(1);
        }
    }

    let refinery = first_buildable_from_array(
        rules,
        &rules.build_refinery_types,
        country_name,
        side_index,
        super_weapons,
    )
    .expect("active Recalc BuildRefinery list resolves a buildable type");
    let has_harvester = first_owner_compatible_harvester(rules, country.owner_bit()).is_some();
    let duplicate_count = if has_harvester {
        rules.ai_extra_refineries[difficulty.table_index()]
    } else {
        rules.ai_slave_miner_number[difficulty.table_index()].wrapping_sub(1)
    };
    if let Some(refinery_index) = priority
        .iter()
        .take(priority.len().saturating_sub(1))
        .position(|candidate| candidate.base_plan_type_index == refinery.base_plan_type_index)
    {
        for _ in 0..duplicate_count.max(0) {
            let insert_after = scenario_rng.next_range_u32_inclusive(
                refinery_index as u32,
                priority.len().wrapping_sub(1) as u32,
            ) as usize;
            priority.insert(insert_after + 1, refinery);
        }
    }

    assert!(
        priority.len() >= 3,
        "active Recalc priority contains its three native seed entries"
    );
    let mut final_values = Vec::with_capacity(priority.len());
    final_values.extend(
        priority[..3]
            .iter()
            .map(|candidate| candidate.base_plan_type_index),
    );
    final_values.extend(
        priority[3..]
            .iter()
            .map(|candidate| candidate.base_plan_type_index),
    );

    let defense_count = match side_index {
        0 => rules.allied_base_defense_counts[difficulty.table_index()],
        1 => rules.soviet_base_defense_counts[difficulty.table_index()],
        2 => rules.third_base_defense_counts[difficulty.table_index()],
        _ => 0,
    };
    for _ in 0..defense_count.max(0) {
        let insert_after = scenario_rng
            .next_range_u32_inclusive(3, final_values.len().wrapping_sub(1) as u32)
            as usize;
        final_values.insert(insert_after + 1, -1);
    }

    plan.rebuild(final_values);
}

/// The NPSE per-type `[BuildingTypes] AIPlanningCount=` count of collection
/// nodes the candidate contributes, `1` for the stock single node. The
/// extension constructor zeroes the field (`0xC3039B`) and the reader stores
/// the `ReadInt` result through `al` (`0xC2E223..0xC2E237`). The loop's
/// byte CMP/JL (`0xC2C35A..0xC2C361`) is signed, so 0 and high-bit counts
/// keep one entry. Source: author-linked NPSE 2.0 #359 release.
fn planning_repeats(candidate: &ObjectType) -> usize {
    if crate::rules::NPATCH_ENABLED {
        (candidate.ai_planning_count as i8).max(1) as usize
    } else {
        1
    }
}

fn recalc_candidate_allowed(
    candidate: &ObjectType,
    type_tech_levels: &TypeTechLevels,
    country: CountryBuildMasks,
    side_index: i32,
    tech_level: i32,
    super_weapons: bool,
    rules: &RuleSet,
) -> bool {
    candidate_allowed(candidate, country, side_index, super_weapons, rules)
        && candidate.ai_build_this
        && type_tech_levels.get(candidate) <= tech_level
}

fn move_eligible_seed<'a>(
    eligible: &mut Vec<&'a ObjectType>,
    selected: &mut [bool],
    seed: Option<&'a ObjectType>,
    target: usize,
) {
    let Some(seed) = seed else {
        return;
    };
    let Some(source) = eligible
        .iter()
        .position(|candidate| candidate.base_plan_type_index == seed.base_plan_type_index)
    else {
        return;
    };
    if target >= eligible.len() {
        return;
    }
    eligible.swap(source, target);
    let target_selected = selected[target];
    selected[source] = target_selected;
    selected[target] = false;
}

/// `HouseClass @ 0x00505360`: every `Prerequisite=` of `candidate` is among
/// `present`, a group through any type of its `[AI]` list (`POWER` BuildPower
/// `Rules+0x8C8`, `FACTORY` BuildWeapons `+0x938`, `BARRACKS` BuildBarracks
/// `+0x900`, `RADAR` BuildRadar `+0xA34`, `TECH` BuildTech `+0x91C`, `PROC`
/// BuildRefinery `+0x8E4`; jump table `0x0050548C`). Recalc passes the types
/// it has planned; the base defense choice the house's own building types.
pub(crate) fn prerequisites_satisfied(
    candidate: &ObjectType,
    present: &[&ObjectType],
    rules: &RuleSet,
) -> bool {
    candidate.prerequisite.iter().all(|entry| {
        #[cfg(feature = "ares")]
        if let Some(group) = rules.ares_prerequisite_group(*entry) {
            // Ares3.0 hook505360 -> ListContainsAll10008370 inspects only
            // immediate building members. Runtime alternates are unread;
            // a negative member code cannot match a BuildingType index.
            return group.members().iter().any(|member| match *member {
                Prerequisite::Building(index) => {
                    present.iter().any(|ty| ty.base_plan_type_index == index)
                }
                Prerequisite::Group(_) | Prerequisite::GenericGroup(_) => false,
            });
        }
        match *entry {
            Prerequisite::Building(index) => {
                present.iter().any(|ty| ty.base_plan_type_index == index)
            }
            Prerequisite::Group(group) => {
                let family = match group {
                    PrerequisiteGroup::Power => &rules.build_power_types,
                    PrerequisiteGroup::Factory => &rules.build_weapons_types,
                    PrerequisiteGroup::Barracks => &rules.build_barracks_types,
                    PrerequisiteGroup::Radar => &rules.build_radar_types,
                    PrerequisiteGroup::Tech => &rules.build_tech_types,
                    PrerequisiteGroup::Proc => &rules.build_refinery_types,
                };
                present.iter().any(|ty| {
                    family
                        .iter()
                        .any(|type_id| type_id.eq_ignore_ascii_case(&ty.id))
                })
            }
            Prerequisite::GenericGroup(_) => false,
        }
    })
}

#[cfg(all(test, feature = "ares"))]
#[path = "base_plan_generation_ares_tests.rs"]
mod ares_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::base_plan::BasePlanNode;

    fn planner_rules(
        harvester_list: &str,
        harvester_owner: &str,
        extra_refineries: &str,
        slave_miners: &str,
        allied_defenses: &str,
        soviet_defenses: &str,
        third_defenses: &str,
        extra_registry: &str,
        extra_sections: &str,
    ) -> RuleSet {
        let text = format!(
            "[General]\n\
             HarvesterUnit={harvester_list}\n\
             AIExtraRefineries={extra_refineries}\n\
             AISlaveMinerNumber={slave_miners}\n\
             AlliedBaseDefenseCounts={allied_defenses}\n\
             SovietBaseDefenseCounts={soviet_defenses}\n\
             ThirdBaseDefenseCounts={third_defenses}\n\
             [AI]\n\
             BuildConst=CON\nBuildPower=POW\nBuildRefinery=REF\n\
             BuildBarracks=BAR\nBuildWeapons=WEAP\nBuildRadar=RAD\nBuildTech=TECH\n\
             [Countries]\n0=Americans\n1=Russians\n2=YuriCountry\n\
             [Sides]\nAllied=Americans\nSoviet=Russians\nThird=YuriCountry\n\
             [Americans]\nSide=Allied\n[Russians]\nSide=Soviet\n[YuriCountry]\nSide=Third\n\
             [VehicleTypes]\n0=HARV\n[HARV]\nOwner={harvester_owner}\nStrength=1\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=WEAP\n5=RAD\n6=TECH\n{extra_registry}\n\
             [CON]\nOwner=Americans,Russians,YuriCountry\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [POW]\nOwner=Americans,Russians,YuriCountry\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [REF]\nOwner=Americans,Russians,YuriCountry\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [BAR]\nOwner=Americans,Russians,YuriCountry\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [WEAP]\nOwner=Americans,Russians,YuriCountry\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [RAD]\nOwner=Americans,Russians,YuriCountry\nAIBuildThis=no\nFoundation=1x1\n\
             [TECH]\nOwner=Americans,Russians,YuriCountry\nAIBuildThis=no\nFoundation=1x1\n\
             {extra_sections}"
        );
        RuleSet::from_ini(&IniFile::from_str(&text)).expect("planner fixture")
    }

    fn basic_rules(extra_registry: &str, extra_sections: &str) -> RuleSet {
        planner_rules(
            "HARV",
            "Americans,Russians,YuriCountry",
            "0,0,0",
            "1,1,1",
            "0,0,0",
            "0,0,0",
            "0,0,0",
            extra_registry,
            extra_sections,
        )
    }

    fn generated_values(
        rules: &RuleSet,
        country: &str,
        side: i32,
        difficulty: HouseDifficulty,
        seed: u64,
    ) -> (BasePlanState, SimRng) {
        let mut plan = BasePlanState::default();
        plan.set_percent_built_for_test(37);
        plan.set_nodes_for_test(vec![BasePlanNode {
            type_or_control: 99,
            packed_cell: 4,
            filled: true,
            retry_count: 8,
        }]);
        let mut rng = SimRng::new(seed);
        recalc_base_plan(
            &mut plan,
            rules,
            &TypeTechLevels::default(),
            country,
            side,
            difficulty,
            10,
            true,
            &mut rng,
        );
        (plan, rng)
    }

    fn values(plan: &BasePlanState) -> Vec<i32> {
        plan.nodes()
            .iter()
            .map(|node| node.type_or_control)
            .collect()
    }

    #[test]
    fn ai_planning_count_respects_the_build_capability() {
        let rules = basic_rules(
            "7=TEST\n\
             [TEST]\n\
             Owner=Americans,Russians,YuriCountry\n\
             AIBuildThis=yes\nTechLevel=1\nFoundation=1x1\nAIPlanningCount=3\n",
            "",
        );
        let expected = if crate::rules::NPATCH_ENABLED { 3 } else { 1 };
        assert_eq!(
            rules.object("TEST").unwrap().ai_planning_count,
            if crate::rules::NPATCH_ENABLED { 3 } else { 0 },
        );
        let (plan, _) =
            generated_values(&rules, "Americans", 0, HouseDifficulty::Normal, 0xC057_0F01);
        let values = values(&plan);
        let test_index = rules.object("TEST").unwrap().base_plan_type_index;
        let first = values
            .iter()
            .position(|value| *value == test_index)
            .expect("the counted type is planned");
        assert_eq!(
            values.iter().filter(|value| **value == test_index).count(),
            expected,
            "only an NPatch build honors the requested copies"
        );
        assert!(
            values[first..first + expected]
                .iter()
                .all(|value| *value == test_index),
            "the repeats are consecutive at the type's own position"
        );
        let con_index = rules.object("CON").unwrap().base_plan_type_index;
        assert_eq!(
            values.iter().filter(|value| **value == con_index).count(),
            1,
            "a type without the key keeps the stock single node"
        );
    }

    /// NPSE C2C331..C2C367 repeats the eligible vector before the
    /// construction-yard seed marks its first entry selected (505714).
    #[test]
    fn ai_planning_count_keeps_counted_seed_candidates() {
        let mut rules = basic_rules(
            "7=COUNTED\n",
            "[COUNTED]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nAIPlanningCount=2\n",
        );
        assert_eq!(
            rules.object("COUNTED").unwrap().ai_planning_count,
            if crate::rules::NPATCH_ENABLED { 2 } else { 0 }
        );
        // Exercise the existing BuildConst selector with this reader-created
        // type, without relying on a duplicate physical [CON] INI section.
        rules.build_const_types = vec!["COUNTED".to_owned()];
        let (plan, _) = generated_values(&rules, "Americans", 0, HouseDifficulty::Normal, 17);
        let index = rules.object("COUNTED").unwrap().base_plan_type_index;
        assert_eq!(
            values(&plan)
                .iter()
                .filter(|value| **value == index)
                .count(),
            if crate::rules::NPATCH_ENABLED { 2 } else { 1 }
        );
    }

    /// ReadInt stores AL at C2E237, but the loop's byte CMP/JL at
    /// C2C35A..C2C361 is signed: a high-bit count does not repeat.
    #[test]
    fn ai_planning_count_uses_the_native_signed_byte_boundary() {
        let rules = basic_rules(
            "7=TEST\n",
            "[TEST]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nAIPlanningCount=128\n",
        );
        let (plan, _) = generated_values(&rules, "Americans", 0, HouseDifficulty::Normal, 17);
        let index = rules.object("TEST").unwrap().base_plan_type_index;
        assert_eq!(
            values(&plan)
                .iter()
                .filter(|value| **value == index)
                .count(),
            1
        );
    }

    #[test]
    fn recalc_eligibility_exact_gate_truth_table() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[AI]\nBuildTech=SWTECH\n\
             [Countries]\n0=Americans\n1=Russians\n[Sides]\nAllied=Americans\nSoviet=Russians\n\
             [Americans]\nSide=Allied\n[Russians]\nSide=Soviet\n\
             [BuildingTypes]\n0=GOOD\n1=NOOWNER\n2=REQUIRED\n3=FORBIDDEN\n4=SIDE\n5=NOAI\n6=HIGH\n7=SWBAD\n8=SWTECH\n\
             [GOOD]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=-7\n\
             [NOOWNER]\nAIBuildThis=yes\nTechLevel=1\n\
             [REQUIRED]\nOwner=Americans\nRequiredHouses=Russians\nAIBuildThis=yes\nTechLevel=1\n\
             [FORBIDDEN]\nOwner=Americans\nForbiddenHouses=Americans\nAIBuildThis=yes\nTechLevel=1\n\
             [SIDE]\nOwner=Americans\nAIBasePlanningSide=1\nAIBuildThis=yes\nTechLevel=1\n\
             [NOAI]\nOwner=Americans\nAIBuildThis=no\nTechLevel=1\n\
             [HIGH]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=6\n\
             [SWBAD]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nSuperWeapon=DISABLED\n\
             [SWTECH]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nSuperWeapon=DISABLED\n\
             [SuperWeaponTypes]\n0=DISABLED\n[DISABLED]\nType=MultiMissile\nDisableableFromShell=yes\n",
        ))
        .expect("eligibility fixture");
        let country = CountryBuildMasks::for_country(&rules, "Americans");
        let allowed = |id: &str, shell_enabled: bool| {
            recalc_candidate_allowed(
                rules
                    .object_in_category(ObjectCategory::Building, id)
                    .unwrap(),
                &TypeTechLevels::default(),
                country,
                0,
                5,
                shell_enabled,
                &rules,
            )
        };
        assert!(
            allowed("GOOD", false),
            "signed negative TechLevel has no lower gate"
        );
        for id in [
            "NOOWNER",
            "REQUIRED",
            "FORBIDDEN",
            "SIDE",
            "NOAI",
            "HIGH",
            "SWBAD",
        ] {
            assert!(!allowed(id, false), "{id}");
        }
        assert!(
            allowed("SWTECH", false),
            "BuildTech exempts shell-disabled SW"
        );
        assert!(allowed("SWBAD", true), "enabled shell bypasses the SW tail");
    }

    #[test]
    fn recalc_seeds_and_selected_byte_moves_match_native_order() {
        let rules = basic_rules("", "");
        let (plan, rng) = generated_values(&rules, "Americans", 0, HouseDifficulty::Hard, 9);
        assert_eq!(values(&plan), [0, 1, 3, 4, 2, 1]);
        assert_eq!(plan.percent_built(), 37);
        assert!(
            plan.nodes()
                .iter()
                .all(|node| { node.packed_cell == 0 && !node.filled && node.retry_count == 0 })
        );
        assert_eq!(rng.logical_state(), SimRng::new(9).logical_state());
    }

    #[test]
    fn recalc_prerequisites_cover_families_explicit_identity_and_pass_boundary() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nPrerequisitePower=POW\nPrerequisiteFactory=WEAP\n\
             PrerequisiteBarracks=BAR\nPrerequisiteRadar=RAD\nPrerequisiteTech=TECH\nPrerequisiteProc=REF\n\
             [AI]\nBuildPower=POW\nBuildWeapons=WEAP\nBuildBarracks=BAR\n\
             BuildRadar=RAD\nBuildTech=TECH\nBuildRefinery=REF\n\
             [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=WEAP\n5=RAD\n6=TECH\n\
             7=EXPLICIT\n8=ALLREQ\n9=A\n10=B\n\
             [EXPLICIT]\nOwner=Americans\nFoundation=1x1\n\
             [ALLREQ]\nOwner=Americans\nPrerequisite=POWER,FACTORY,BARRACKS,RADAR,TECH,PROC,EXPLICIT\nFoundation=1x1\n\
             [A]\nOwner=Americans\nFoundation=1x1\n\
             [B]\nOwner=Americans\nPrerequisite=A\nFoundation=1x1\n",
        )).unwrap();
        let get = |id| {
            rules
                .object_in_category(ObjectCategory::Building, id)
                .unwrap()
        };
        let present = ["POW", "WEAP", "BAR", "RAD", "TECH", "REF", "EXPLICIT"].map(get);
        assert!(prerequisites_satisfied(get("ALLREQ"), &present, &rules));

        let mut priority = vec![get("CON"), get("POW")];
        let pass_start_len = priority.len();
        priority.push(get("A"));
        assert!(
            !prerequisites_satisfied(get("B"), &priority[..pass_start_len], &rules),
            "an earlier same-pass append is invisible"
        );
        assert!(prerequisites_satisfied(get("B"), &priority, &rules));
    }

    #[test]
    fn recalc_withholds_gaplug_and_breaks_cycle_with_last_unselected() {
        // BLOCK waits on a BuildingType the planner never selects; a name no
        // BuildingType has would be dropped by the parser and block nothing.
        let rules = basic_rules(
            "7=BLOCK\n8=GAPLUG\n9=NEVER",
            "[BLOCK]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nPrerequisite=NEVER\nFoundation=1x1\n\
             [GAPLUG]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
             [NEVER]\nOwner=Americans\nTechLevel=1\nFoundation=1x1\n",
        );
        let (plan, _) = generated_values(&rules, "Americans", 0, HouseDifficulty::Hard, 11);
        assert_eq!(&values(&plan)[6..], [8, 7]);
    }

    #[test]
    fn recalc_refinery_duplicates_use_exact_branch_ranges_and_rng() {
        for (difficulty, duplicates) in [
            (HouseDifficulty::Hard, 2),
            (HouseDifficulty::Normal, 1),
            (HouseDifficulty::Easy, 0),
        ] {
            let rules = planner_rules(
                "HARV",
                "Americans",
                "2,1,0",
                "9,9,9",
                "0,0,0",
                "0,0,0",
                "0,0,0",
                "",
                "",
            );
            let seed = 0x405;
            let (plan, rng) = generated_values(&rules, "Americans", 0, difficulty, seed);
            let mut expected_values = vec![0, 1, 3, 4, 2, 1];
            let mut expected_rng = SimRng::new(seed);
            for _ in 0..duplicates {
                let drawn =
                    expected_rng.next_range_u32_inclusive(4, expected_values.len() as u32 - 1);
                expected_values.insert(drawn as usize + 1, 2);
            }
            assert_eq!(values(&plan), expected_values, "{difficulty:?}");
            assert_eq!(
                rng.logical_state(),
                expected_rng.logical_state(),
                "{difficulty:?}"
            );
        }

        let rules = planner_rules(
            "HARV", "Russians", "9,9,9", "4,3,2", "0,0,0", "0,0,0", "0,0,0", "", "",
        );
        let (plan, _) = generated_values(&rules, "Americans", 0, HouseDifficulty::Normal, 13);
        assert_eq!(values(&plan).iter().filter(|&&value| value == 2).count(), 3);
    }

    #[test]
    fn recalc_defense_sentinels_use_side_difficulty_and_shifted_ranges() {
        let cases = [
            (0, HouseDifficulty::Hard, 2),
            (0, HouseDifficulty::Normal, 1),
            (0, HouseDifficulty::Easy, 0),
            (1, HouseDifficulty::Hard, 3),
            (1, HouseDifficulty::Normal, 2),
            (1, HouseDifficulty::Easy, 1),
            (2, HouseDifficulty::Hard, 1),
            (2, HouseDifficulty::Normal, 0),
            (2, HouseDifficulty::Easy, 0),
            (9, HouseDifficulty::Hard, 0),
        ];
        for (side, difficulty, sentinels) in cases {
            let country = match side {
                1 => "Russians",
                2 => "YuriCountry",
                _ => "Americans",
            };
            let rules = planner_rules(
                "HARV",
                "Americans,Russians,YuriCountry",
                "0,0,0",
                "1,1,1",
                "2,1,0",
                "3,2,1",
                "1,0,0",
                "",
                "",
            );
            let seed = 0x5750;
            let (plan, rng) = generated_values(&rules, country, side, difficulty, seed);
            let mut expected_values = vec![0, 1, 3, 4, 2, 1];
            let mut expected_rng = SimRng::new(seed);
            for _ in 0..sentinels {
                let drawn =
                    expected_rng.next_range_u32_inclusive(3, expected_values.len() as u32 - 1);
                expected_values.insert(drawn as usize + 1, -1);
            }
            assert_eq!(values(&plan), expected_values, "side={side} {difficulty:?}");
            assert_eq!(
                rng.logical_state(),
                expected_rng.logical_state(),
                "side={side} {difficulty:?}"
            );
        }
    }

    #[test]
    fn recalc_preflight_rejects_only_the_selected_missing_vector_branch() {
        let no_vectors = planner_rules("HARV", "Americans", "", "", "", "", "", "", "");
        assert_eq!(
            preflight_recalc(&no_vectors, "Americans", 0, HouseDifficulty::Hard),
            Err(RecalcPreflightError::MissingRefineryDifficulty)
        );

        let no_defense = planner_rules("HARV", "Americans", "0,0,0", "1,1,1", "", "", "", "", "");
        assert_eq!(
            preflight_recalc(&no_defense, "Americans", 0, HouseDifficulty::Hard),
            Err(RecalcPreflightError::MissingDefenseDifficulty)
        );
        assert_eq!(
            preflight_recalc(&no_defense, "Americans", 9, HouseDifficulty::Hard),
            Ok(()),
            "an unknown side inserts zero and does not require a defense vector"
        );
    }
}
