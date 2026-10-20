//! Smudge type definitions parsed from rulesmd.ini.
//!
//! [SmudgeTypes] numeric list maps to per-name sections (e.g. [CRATER01]).
//! Each type carries the four INI keys that gate spawn behavior:
//! Crater, Burn, Width, Height.
//!
//! Dependency rules: depends on rules/ini_parser only. No sim dependency.

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;

#[derive(Debug, Clone)]
pub struct SmudgeTypeDef {
    pub name: String,
    pub crater: bool,
    pub burn: bool,
    pub width: u8,
    pub height: u8,
    pub image_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SmudgeTypeRegistry {
    types: Vec<SmudgeTypeDef>,
    by_name: HashMap<String, u16>,
}

impl SmudgeTypeRegistry {
    pub fn from_rules_ini(ini: &IniFile) -> Self {
        let mut types: Vec<SmudgeTypeDef> = Vec::new();
        let mut by_name: HashMap<String, u16> = HashMap::new();

        let Some(list_section) = ini.section("SmudgeTypes") else {
            return Self::default();
        };

        // Matches the canonical [XxxTypes] registry pattern used by ruleset:
        // entry values are consumed in declaration order regardless of key text.
        for source_name in list_section.registry_ids() {
            let name_upper: String = source_name.to_uppercase();
            if name_upper.is_empty() {
                continue;
            }
            if by_name.contains_key(&name_upper) {
                continue;
            }
            // Rules668E19 -> FindOrAllocate6B5910 registers every declaration
            // before ReadTypeData679BF9 ignores a false ReadINI return. The
            // stock native comparison retains 46 types, including 22 legacy
            // empty-header defaults (Smudge ctor6B5269..6B5294).
            // Evidence: building_death_anims_joined_stock_smudges.json.
            let section = ini.section(source_name);
            let crater: bool = section.is_some_and(|s| s.read_bool("Crater", false));
            let burn: bool = section.is_some_and(|s| s.read_bool("Burn", false));
            // ReadInt over the constructor's 1 (`0x006B5281`, `0x006B573F`,
            // `0x006B5759`). RESIDUAL: native keeps the int unclamped; VERA
            // clamps it into 1..=255. Retail sizes are 1 and 2.
            let width: u8 = section.map_or(1, |s| s.read_int("Width", 1)).clamp(1, 255) as u8;
            let height: u8 = section.map_or(1, |s| s.read_int("Height", 1)).clamp(1, 255) as u8;
            // ObjectTypeClass `Image=`, ReadString 0x19 (`0x005F933B`).
            let image_name: Option<String> = section
                .and_then(|s| s.read_name("Image", 0x19))
                .map(str::to_string);

            let id: u16 = types.len() as u16;
            by_name.insert(name_upper.clone(), id);
            types.push(SmudgeTypeDef {
                name: name_upper,
                crater,
                burn,
                width,
                height,
                image_name,
            });
        }

        Self { types, by_name }
    }

    pub fn get(&self, id: u16) -> Option<&SmudgeTypeDef> {
        self.types.get(id as usize)
    }

    pub fn find_by_name(&self, name: &str) -> Option<u16> {
        self.by_name.get(&name.to_uppercase()).copied()
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn iter_with_id(&self) -> impl Iterator<Item = (u16, &SmudgeTypeDef)> {
        self.types.iter().enumerate().map(|(i, t)| (i as u16, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ini(s: &str) -> IniFile {
        IniFile::from_bytes(s.as_bytes()).unwrap()
    }

    #[test]
    fn parses_smudge_types_list_with_per_name_sections() {
        let ini = parse_ini(
            "[SmudgeTypes]\n\
             1=CR1\n\
             2=BURN01\n\
             \n\
             [CR1]\n\
             Crater=yes\n\
             Width=1\n\
             Height=1\n\
             \n\
             [BURN01]\n\
             Burn=yes\n\
             Width=2\n\
             Height=2\n",
        );
        let reg = SmudgeTypeRegistry::from_rules_ini(&ini);
        assert_eq!(reg.len(), 2);
        let cr1 = reg.get(0).unwrap();
        assert_eq!(cr1.name, "CR1");
        assert!(cr1.crater);
        assert!(!cr1.burn);
        assert_eq!(cr1.width, 1);
        let burn01 = reg.get(1).unwrap();
        assert!(burn01.burn);
        assert_eq!(burn01.width, 2);
        assert_eq!(burn01.height, 2);
    }

    #[test]
    fn declared_type_survives_absent_read_ini_section() {
        let ini = parse_ini("[SmudgeTypes]\n1=DOES_NOT_EXIST\n");
        let reg = SmudgeTypeRegistry::from_rules_ini(&ini);
        let def = reg.get(0).unwrap();
        assert_eq!(def.name, "DOES_NOT_EXIST");
        assert!(!def.crater && !def.burn);
        assert_eq!((def.width, def.height), (1, 1));
    }

    #[test]
    fn find_by_name_case_insensitive() {
        let ini = parse_ini("[SmudgeTypes]\n1=CR1\n[CR1]\nCrater=yes\n");
        let reg = SmudgeTypeRegistry::from_rules_ini(&ini);
        assert_eq!(reg.find_by_name("cr1"), Some(0));
        assert_eq!(reg.find_by_name("CR1"), Some(0));
        assert_eq!(reg.find_by_name("nope"), None);
    }
}
