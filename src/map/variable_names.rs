//! Map local variable definitions from `[VariableNames]`.
//!
//! The map file can define local boolean variables with optional initial set
//! state. Trigger events can test these locals, and trigger actions can mutate
//! them during runtime.

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{crt_atoi, strtok};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVariable {
    pub index: u32,
    pub name: String,
    /// No second token leaves the Scenario's prior value unchanged.
    pub initially_set: Option<bool>,
}

pub type LocalVariableMap = HashMap<u32, LocalVariable>;

/// The scenario's local-variable read (`0x00689B42`): at most 100 entries by
/// index, each slot `atoi` of the entry name, the value a 0x80-byte
/// ReadString whose first `strtok(",")` token is the name and whose second,
/// when present, sets the flag by `atoi != 0`. Native does not bound the slot
/// against its 100-entry table; Rust keeps any nonnegative slot and skips a
/// negative one.
pub fn parse_local_variables(ini: &IniFile) -> LocalVariableMap {
    let Some(section) = ini.section("VariableNames") else {
        return HashMap::new();
    };

    let mut locals = LocalVariableMap::new();
    for key in section.keys().take(100) {
        let Ok(index) = u32::try_from(crt_atoi(key)) else {
            continue;
        };
        let mut tokens = strtok(section.read_name(key, 0x80).unwrap_or(""), &[',']);
        let name = tokens.next().unwrap_or("").to_string();
        let initially_set = tokens.next().map(|value| crt_atoi(value) != 0);
        // ReadLocal689C14 skips the value write when the token is absent.
        // Different INI keys can alias one atoi slot within this same pass.
        let variable = locals.entry(index).or_insert(LocalVariable {
            index,
            name: String::new(),
            initially_set: None,
        });
        variable.name = name;
        if initially_set.is_some() {
            variable.initially_set = initially_set;
        }
    }

    if !locals.is_empty() {
        log::info!(
            "Parsed {} local variables from [VariableNames]",
            locals.len()
        );
    }
    locals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_local_variables() {
        let ini = IniFile::from_str("[VariableNames]\n0=BridgeFixed,1\n7=SpyEntered,0\n");
        let vars = parse_local_variables(&ini);
        assert_eq!(vars.len(), 2);
        assert_eq!(
            vars.get(&0),
            Some(&LocalVariable {
                index: 0,
                name: "BridgeFixed".to_string(),
                initially_set: Some(true),
            })
        );
        assert_eq!(
            vars.get(&7),
            Some(&LocalVariable {
                index: 7,
                name: "SpyEntered".to_string(),
                initially_set: Some(false),
            })
        );
    }

    /// The slot is `atoi` of the entry name, so a non-numeric name is slot 0.
    #[test]
    fn non_numeric_entry_names_write_slot_zero() {
        let ini = IniFile::from_str("[VariableNames]\n0=BridgeFixed,1\njunk=Replaced,2\n");
        let vars = parse_local_variables(&ini);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[&0].name, "Replaced");
        assert_eq!(vars[&0].initially_set, Some(true));
    }
}
