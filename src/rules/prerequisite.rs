//! `Prerequisite=`, `PrerequisiteOverride=` and the `[General]
//! Prerequisite*` lists, as `Prerequisite_INI_Parser @ 0x004770E0` parses
//! them. Each comma token of the `char[128]` value is one of the six group
//! names (any case) or a BuildingType, kept as its array index
//! (`BuildingTypeClass::FindIndexByName @ 0x0045E7B0`, the first
//! case-insensitive match); a name no BuildingType has is dropped. An absent
//! or empty value keeps the list's prior value. tools/rules_oracle/
//! prerequisite_lists.py runs the original over these cases.
//!
//! The rules processing layer (`native_processing`) parses each list in every
//! rules pass where native reads it, against the BuildingTypes allocated by
//! then: the `[General]` lists in ReadGeneral (`0x0066E78C..0x0066EA6E`)
//! before its own allocations, a type's two lists in TechnoTypeClass::ReadINI
//! (`0x007141C5`, `0x0071424B`) after `Dock=` and `DeploysInto=`. `RuleSet`
//! takes the result.
//!
//! RESIDUAL: a group in `PrerequisiteOverride=` or in a `[General]` list. Both
//! readers pass its code to `CounterClass::GetItemCount @ 0x0049FAE0`, which
//! has no lower bound and reads the heap before the counter array; VERA
//! answers not met. Trigger: a rules or map INI naming a group there.
//! Effect: CanBuild's override or that group's test decides on arbitrary
//! memory natively. Frequency: no retail list does (rulesmd's three overrides
//! and six lists name BuildingTypes only).

/// The six named groups, the original's codes -1..-6. Each names a `[General]
/// Prerequisite*` list for `HouseClass::CanBuild` and an `[AI] Build*` list
/// for the base planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PrerequisiteGroup {
    Power,
    Factory,
    Barracks,
    Radar,
    Tech,
    Proc,
}

impl PrerequisiteGroup {
    /// In code order: `POWER` is -1, `PROC` -6.
    pub const ALL: [Self; 6] = [
        Self::Power,
        Self::Factory,
        Self::Barracks,
        Self::Radar,
        Self::Tech,
        Self::Proc,
    ];

    /// The token the parser matches (`strcmpi`).
    pub const fn token(self) -> &'static str {
        match self {
            Self::Power => "POWER",
            Self::Factory => "FACTORY",
            Self::Barracks => "BARRACKS",
            Self::Radar => "RADAR",
            Self::Tech => "TECH",
            Self::Proc => "PROC",
        }
    }

    /// The `[General]` key that lists the group's buildings.
    pub const fn general_key(self) -> &'static str {
        match self {
            Self::Power => "PrerequisitePower",
            Self::Factory => "PrerequisiteFactory",
            Self::Barracks => "PrerequisiteBarracks",
            Self::Radar => "PrerequisiteRadar",
            Self::Tech => "PrerequisiteTech",
            Self::Proc => "PrerequisiteProc",
        }
    }

    /// The position in [`Self::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|group| group.token().eq_ignore_ascii_case(token))
    }
}

/// One parsed entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Prerequisite {
    Group(PrerequisiteGroup),
    /// A BuildingType, by its native array index.
    Building(i32),
}

/// One list from `tokens`, the key's `read_list(key, 0x80)`; `None` keeps
/// `prior`. A group name wins over a BuildingType of the same name.
pub(crate) fn parse_prerequisites(
    tokens: Option<Vec<&str>>,
    prior: &[Prerequisite],
    building_index: impl Fn(&str) -> Option<i32>,
) -> Vec<Prerequisite> {
    let Some(tokens) = tokens else {
        return prior.to_vec();
    };
    tokens
        .into_iter()
        .filter_map(|token| match PrerequisiteGroup::from_token(token) {
            Some(group) => Some(Prerequisite::Group(group)),
            None => building_index(token).map(Prerequisite::Building),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use serde_json::Value;

    fn code(entry: Prerequisite) -> i64 {
        match entry {
            Prerequisite::Group(group) => -(group.index() as i64) - 1,
            Prerequisite::Building(index) => i64::from(index),
        }
    }

    fn from_code(code: i64) -> Prerequisite {
        if code < 0 {
            Prerequisite::Group(PrerequisiteGroup::ALL[(-code - 1) as usize])
        } else {
            Prerequisite::Building(i32::try_from(code).unwrap())
        }
    }

    /// Each type's lists and the `[General]` lists resolve against the
    /// BuildingType registry, where a listed name without a section still
    /// has its index; a group in a `[General]` list keeps its code.
    #[test]
    fn rules_parse_each_list_against_the_building_registry() {
        use crate::rules::ruleset::RuleSet;
        use Prerequisite::{Building, Group};
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=GAPOWR\n1=GAWEAP\n2=NAMED\n\
             [VehicleTypes]\n0=MTNK\n\
             [General]\nPrerequisitePower=gapowr,POWER,NOPE\nPrerequisiteFactory=GAWEAP\n\
             [GAPOWR]\nStrength=750\n[GAWEAP]\nStrength=1000\n\
             [MTNK]\nPrerequisite=gaweap,Proc,NOPE,NAMED\nPrerequisiteOverride=GAPOWR\n",
        ))
        .unwrap();
        let mtnk = rules.object("MTNK").unwrap();
        assert_eq!(
            mtnk.prerequisite,
            [Building(1), Group(PrerequisiteGroup::Proc), Building(2)]
        );
        assert_eq!(mtnk.prerequisite_override, [Building(0)]);
        assert_eq!(
            rules.prerequisite_list(PrerequisiteGroup::Power),
            [Building(0), Group(PrerequisiteGroup::Power)]
        );
        assert_eq!(
            rules.prerequisite_list(PrerequisiteGroup::Factory),
            [Building(1)]
        );
        assert!(rules.prerequisite_list(PrerequisiteGroup::Tech).is_empty());
    }

    /// Each rules pass parses the lists again with the prior list as the
    /// default, against the BuildingTypes allocated by then: an absent key
    /// keeps the list, a value of commas or of unknown names empties it, and
    /// a BuildingType that a vehicle's `DeploysInto=` first allocates later
    /// in the pass is not found by a building's read.
    #[test]
    fn each_rules_pass_parses_the_lists_again() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        use Prerequisite::Building;
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=GAPOWR\n1=GAWEAP\n\
             [VehicleTypes]\n0=MTNK\n1=AMCV\n\
             [General]\nPrerequisitePower=GAPOWR\nPrerequisiteFactory=GAWEAP\n\
             PrerequisiteTech=GAPOWR\n\
             [GAPOWR]\nStrength=750\n[GAWEAP]\nStrength=1000\n\
             [MTNK]\nPrerequisite=GAWEAP\n[AMCV]\nStrength=1000\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[General]\nPrerequisitePower=,,,\nPrerequisiteFactory=NOPE\n\
                 [GAWEAP]\nPrerequisite=NEWB,GAPOWR\n\
                 [MTNK]\nStrength=400\n[AMCV]\nDeploysInto=NEWB\n",
            ),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert!(rules.prerequisite_list(PrerequisiteGroup::Power).is_empty());
        assert!(
            rules
                .prerequisite_list(PrerequisiteGroup::Factory)
                .is_empty()
        );
        assert_eq!(
            rules.prerequisite_list(PrerequisiteGroup::Tech),
            [Building(0)]
        );
        assert_eq!(rules.object("MTNK").unwrap().prerequisite, [Building(1)]);
        assert_eq!(rules.building_type_index("NEWB"), Some(2));
        assert_eq!(rules.object("GAWEAP").unwrap().prerequisite, [Building(0)]);
    }

    #[test]
    fn parse_matches_the_original() {
        let data: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/prerequisite_lists.json",
        ))
        .unwrap();
        let buildings: Vec<&str> = data["buildings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        let index = |token: &str| {
            buildings
                .iter()
                .position(|name| name.eq_ignore_ascii_case(token))
                .map(|index| index as i32)
        };
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 11);
        for row in rows {
            let ini = match row["value"].as_str() {
                Some(value) => format!("[T]\nPrerequisite={value}\n"),
                None => "[T]\nName=T\n".to_string(),
            };
            let ini = IniFile::from_str(&ini);
            let prior: Vec<Prerequisite> = row["default"]
                .as_array()
                .unwrap()
                .iter()
                .map(|code| from_code(code.as_i64().unwrap()))
                .collect();
            let parsed = parse_prerequisites(
                ini.section("T").unwrap().read_list("Prerequisite", 0x80),
                &prior,
                index,
            );
            let expected: Vec<i64> = row["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|code| code.as_i64().unwrap())
                .collect();
            assert_eq!(
                parsed.into_iter().map(code).collect::<Vec<_>>(),
                expected,
                "{}",
                row["name"]
            );
        }
    }
}
