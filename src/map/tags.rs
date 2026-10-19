//! Map tag records.
//!
//! `[Tags]` holds each TagType record. TagType::ReadINI (`0x006E6080`) reads
//! it by the Type's own ID through the exact-case INI lookup, so records are
//! keyed by their exact key and the runtime reader in `sim::trigger_runtime`
//! takes each one by that ID.

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{crt_atoi, strtok, truncate_native_bytes};

/// One `[Tags]` record as TagType::ReadINI takes it from its 0x80-byte
/// ReadString copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapTag {
    repeat: i32,
    name: String,
    trigger: Option<String>,
}

impl MapTag {
    /// TagType+9C: `atoi` of the whole copy after the first `strtok`
    /// (`0x006E60D5`), so an empty leading field reads 0.
    pub fn repeat(&self) -> i32 {
        self.repeat
    }

    /// TagType+64: the second token cut to 48 bytes (`0x006E6101`), empty
    /// when absent.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The third token as written, which the TriggerType factory
    /// (`0x00727AA0`) resolves.
    pub fn trigger(&self) -> Option<&str> {
        self.trigger.as_deref()
    }
}

pub type TagMap = HashMap<String, MapTag>;

/// Parse `[Tags]` into exact-key records.
pub fn parse_tags(ini: &IniFile) -> TagMap {
    let Some(section) = ini.section("Tags") else {
        return HashMap::new();
    };

    let mut tags: TagMap = HashMap::new();
    for key in section.keys() {
        let Some(raw_value) = section.read_name(key, 0x80) else {
            continue;
        };
        // The first token only ends the field `atoi` reads.
        let mut tokens = strtok(raw_value, &[',']).skip(1);
        let name = tokens
            .next()
            .map_or("", |name| truncate_native_bytes(name, 0x30));
        let tag = MapTag {
            repeat: crt_atoi(raw_value),
            name: name.to_string(),
            trigger: tokens.next().map(str::to_string),
        };
        tags.insert(key.to_string(), tag);
    }

    if !tags.is_empty() {
        log::info!("Parsed {} tags from [Tags]", tags.len());
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tags() {
        let ini = IniFile::from_str("[Tags]\nTRIG_A=0,SomeName,Trig\nObj_01=2,Name\n");
        let tags = parse_tags(&ini);
        assert_eq!(tags.len(), 2);
        assert_eq!(
            tags.get("TRIG_A"),
            Some(&MapTag {
                repeat: 0,
                name: "SomeName".to_string(),
                trigger: Some("Trig".to_string()),
            })
        );
        let tag = &tags["Obj_01"];
        assert_eq!((tag.repeat(), tag.name(), tag.trigger()), (2, "Name", None));
        assert!(!tags.contains_key("OBJ_01"));
    }

    #[test]
    fn repeat_reads_the_whole_copy_and_the_name_is_cut_to_48_bytes() {
        let name = "N".repeat(60);
        let ini = IniFile::from_str(&format!("[Tags]\nT=,2,{name},X\n"));
        let tag = &parse_tags(&ini)["T"];
        assert_eq!(tag.repeat(), 0);
        assert_eq!(tag.name(), &name[..0x30]);
        assert_eq!(tag.trigger(), Some("X"));
    }

    #[test]
    fn test_missing_tags_is_empty() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        assert!(parse_tags(&ini).is_empty());
    }
}
