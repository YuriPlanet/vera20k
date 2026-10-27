//! Ares 3.0 `[Country] AI.PowerPlants=` (HouseTypeExt), read by the common
//! country reader `0x1001C5D0` on every pass, and `GetPowerplants @
//! 0x1001BD20`, which answers it while it is non-empty, else the side's
//! `[General]` plants.

use super::*;
use crate::rules::ruleset::RuleSet;

fn processed(ini: &str) -> RuleSet {
    let processed = RulesLayerStack::new(IniFile::from_str(ini))
        .process()
        .expect("country power plant rules parse");
    RuleSet::from_processed_rules(&processed).expect("country power plant projection")
}

const REGISTRY: &str = "\
[General]\nGDIPowerPlant=GAPOWR\nNodRegularPower=NAPOWR\nNodAdvancedPower=NAAPWR\n\
[BuildingTypes]\n0=GAPOWR\n1=NAAPWR\n2=NAPOWR\n\
[GAPOWR]\n[NAAPWR]\n[NAPOWR]\n";

fn plants(rules: &RuleSet, names: &[&str]) -> Vec<i32> {
    names
        .iter()
        .map(|name| rules.building_type_index(name).unwrap())
        .collect()
}

#[test]
fn ares_country_power_plants_keep_registered_building_identities() {
    let rules = processed(&format!(
        "{REGISTRY}[Countries]\n0=Greece\n\
         [Greece]\nAI.PowerPlants=NAAPWR,UNKNOWN,NAPOWR\n"
    ));
    assert_eq!(
        rules.ares_power_plants("Greece", 0),
        plants(&rules, &["NAAPWR", "NAPOWR"])
    );
}

/// `0x1001BD45..0x1001BDB7`: an empty list answers the side's plants, side 1
/// both Nod plants in slot order; side 2's `ThirdPowerPlant=` is unset here
/// and side 3 has none.
#[test]
fn ares_country_without_power_plants_answers_the_side_plants() {
    let rules = processed(&format!("{REGISTRY}[Countries]\n0=Greece\n[Greece]\n"));
    assert_eq!(
        rules.ares_power_plants("Greece", 0),
        plants(&rules, &["GAPOWR"])
    );
    assert_eq!(
        rules.ares_power_plants("Greece", 1),
        plants(&rules, &["NAPOWR", "NAAPWR"])
    );
    assert!(rules.ares_power_plants("Greece", 2).is_empty());
    assert!(rules.ares_power_plants("Nowhere", 3).is_empty());
}

/// `0x1001C5D0` copies the parent (`0x1001BDC0`) before it reads
/// `AI.PowerPlants` (`0x1001C60C`), so a child's own list wins over the
/// inherited one, and it reads on every pass, so a map sets it too.
#[test]
fn ares_country_power_plants_follow_the_parent_copy_on_every_pass() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(&format!(
        "{REGISTRY}[Countries]\n0=Greece\n1=Rome\n2=Sparta\n\
         [Greece]\nAI.PowerPlants=NAAPWR\n\
         [Rome]\nParentCountry=Greece\n\
         [Sparta]\nParentCountry=Greece\nAI.PowerPlants=NAPOWR\n"
    )));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Greece]\nAI.PowerPlants=GAPOWR\n"),
    );
    let processed = layers.process().expect("country power plant map pass");
    let rules = RuleSet::from_processed_rules(&processed).expect("country power plant projection");
    assert_eq!(
        rules.ares_power_plants("Rome", 0),
        plants(&rules, &["NAAPWR"])
    );
    assert_eq!(
        rules.ares_power_plants("Sparta", 0),
        plants(&rules, &["NAPOWR"])
    );
    assert_eq!(
        rules.ares_power_plants("Greece", 0),
        plants(&rules, &["GAPOWR"])
    );
}
