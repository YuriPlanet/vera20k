//! Shared production reader setup for original particle-family controls.
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn rules_from_layers(layers: &Value, input: &Value, registry: &str) -> RuleSet {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for layer in layers.as_array().unwrap() {
        if let Some(rows) = layer["sections"].as_object() {
            for (section, values) in rows {
                for (key, value) in values.as_object().unwrap() {
                    sections
                        .entry(section.clone())
                        .or_default()
                        .insert(key.clone(), value.as_str().unwrap().into());
                }
            }
        }
    }
    if let Some(overrides) = input["type_overrides"].as_object() {
        for (section, values) in overrides {
            for (key, value) in values.as_object().unwrap() {
                sections
                    .entry(section.clone())
                    .or_default()
                    .insert(key.clone(), value.as_str().unwrap().into());
            }
        }
    }
    if let Some(wind) = input["wind"].as_i64() {
        sections
            .entry("General".into())
            .or_default()
            .insert("WindDirection".into(), wind.to_string());
    }
    let mut ini = registry.to_owned();
    for (section, values) in sections {
        ini.push_str(&format!("[{section}]\n"));
        for (key, value) in values {
            ini.push_str(&format!("{key}={value}\n"));
        }
    }
    RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap()
}
