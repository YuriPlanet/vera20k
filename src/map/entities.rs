//! Parser for map-placed entities: units, infantry, structures, and aircraft.
//!
//! RA2 maps store entity placements in four INI sections:
//! - `[Units]`: vehicles (14 comma-separated fields per line)
//! - `[Aircraft]`: air units (12 fields)
//! - `[Infantry]`: soldiers (14 fields, includes sub-cell position)
//! - `[Structures]`: buildings (17 fields, includes upgrades)
//!
//! Each line: `INDEX=OWNER,TYPE_ID,HEALTH,X,Y,...` with category-specific trailing fields.
//!
//! ## Dependency rules
//! - Part of map/ — depends on rules/ (IniFile/IniSection for parsing) and on
//!   the mission selector vocabulary in rules/mission_data (the `MISSION=` column is
//!   the same 32-name table the scenario reader resolves through).

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::{crt_atoi, strtok};
use crate::rules::mission_data::MissionType;

/// Which category of game object this entity represents.
/// Determines rendering approach and available behaviors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EntityCategory {
    /// Vehicles — rendered as VXL voxel models.
    Unit,
    /// Soldiers — rendered as SHP sprites, support sub-cell positioning.
    Infantry,
    /// Buildings — rendered as SHP sprites, have foundations.
    Structure,
    /// Air units — rendered as VXL voxel models, drawn above ground.
    Aircraft,
}

impl EntityCategory {
    /// AbstractFlags bit 2 (`+0x14 & 4`): the object is a FootClass.
    pub(crate) const fn is_foot(self) -> bool {
        matches!(self, Self::Unit | Self::Infantry | Self::Aircraft)
    }
}

impl From<crate::rules::object_type::ObjectCategory> for EntityCategory {
    /// The class of object a type of each registry constructs.
    fn from(category: crate::rules::object_type::ObjectCategory) -> Self {
        use crate::rules::object_type::ObjectCategory;
        match category {
            ObjectCategory::Infantry => Self::Infantry,
            ObjectCategory::Vehicle => Self::Unit,
            ObjectCategory::Aircraft => Self::Aircraft,
            ObjectCategory::Building => Self::Structure,
        }
    }
}

impl From<EntityCategory> for crate::rules::object_type::ObjectCategory {
    /// The type registry of an object's actual constructed class.
    fn from(category: EntityCategory) -> Self {
        match category {
            EntityCategory::Infantry => Self::Infantry,
            EntityCategory::Unit => Self::Vehicle,
            EntityCategory::Aircraft => Self::Aircraft,
            EntityCategory::Structure => Self::Building,
        }
    }
}

/// A single entity placement parsed from a map file.
///
/// Contains the minimum data needed to spawn an ECS entity.
/// Trigger definitions resolve the authored tag while the concrete object is
/// constructed, before Unlimbo. Remaining AI flags retain their reader bounds.
#[derive(Debug, Clone)]
pub struct MapEntity {
    /// House/faction name (e.g., "Americans", "Soviet", "Neutral").
    pub owner: String,
    /// Object type ID from rules.ini (e.g., "HTNK", "E1", "GAPOWR").
    pub type_id: String,
    /// Signed authored health fraction; 256 denotes full Strength. Class admission
    /// owns clamping, near-full snapping and minimum-health rules.
    pub health: i32,
    /// Isometric cell X coordinate.
    pub cell_x: u16,
    /// Isometric cell Y coordinate.
    pub cell_y: u16,
    /// Facing direction (0–255, where 0=north, 64=east, 128=south, 192=west).
    pub facing: u8,
    /// Entity category (determines rendering and behavior).
    pub category: EntityCategory,
    /// Sub-cell position for infantry (0–4). Always 0 for other categories.
    pub sub_cell: u8,
    /// Optional signed experience percentage, read with CRT `atoi`. Native
    /// `SetFromPercent @ 0x007500E0` stores percent * 0.01 without a clamp;
    /// absent tokens retain the constructor's accumulator. Structures have
    /// no veterancy column.
    pub veterancy: Option<i32>,
    /// Spawn on the bridge deck / high layer when the map placement marks it.
    pub high: bool,
    /// The authored `MISSION=` column, resolved through the engine's mission
    /// name table. `None` is the `-1` idle sentinel the scenario reader gets
    /// for an absent or unrecognised name — a *distinct* selector from
    /// `Sleep(0)`, so the two must not be folded together. `[Structures]` has
    /// no MISSION column at all and is always `None`.
    pub mission: Option<MissionType>,
    /// First persistent scenario recruitment-admission byte (`Techno+0x421`).
    /// Unit/Infantry/Aircraft scenario lines store it at trailing field 12;
    /// constructors and absent fields default true.
    pub recruitable_a: bool,
    /// Second independent recruitment-admission byte (`Techno+0x422`), stored
    /// at trailing field 13 and likewise constructor-default true.
    pub recruitable_b: bool,
    /// gamemd.exe Read_INI: Unit7434CA/Aircraft41B33D column9,
    /// Infantry51FD89 column10. Optional CRT atoi, no clamp; absent retains
    /// Techno constructor6F2D0F -1. Structures have no Group column.
    pub group: i32,
    /// Authored `[Structures]` upgrade type names for native slots 0..2.
    /// Slots beyond the line's declared upgrade count and unresolved `None`/
    /// `-1` entries remain empty. Non-structure categories always hold three
    /// empty slots.
    pub structure_upgrades: [Option<String>; 3],
    /// `[Structures]` field 7, AI Sellable: `BuildingClass::ReadFromINI @
    /// 0x0044F820` stores its `atoi != 0` in the building's `+0x6DC`
    /// (`0x0044FB5B`), false when the line stops short. False for other
    /// categories.
    pub structure_ai_sellable: bool,
    /// `[Structures]` field 15, AI Repairable: the same reader stores its
    /// `atoi != 0` in the building's `+0x6CB` (`0x0044FB70`), before the
    /// building's Unlimbo (`0x0044FBF9`), false when the line stops short.
    /// False for other categories.
    pub structure_ai_repairable: bool,
    /// Structures field9, default true (44FA11), CRT atoi !=0 (44FA2D).
    /// Applied after Unlimbo through the shared GoOnline/GoOffline owner.
    pub structure_online: bool,
    /// Native Unit74346A/Aircraft41B2DD/Infantry51FD29/Building44FB33:
    /// raw CRT token passed to TagType6E5E70 ID-or-display-name lookup.
    /// The loader attaches the shared live Tag before class Unlimbo.
    pub trigger_tag: Option<String>,
}

/// Field index of the `MISSION=` column in `[Units]`, `[Infantry]` and
/// `[Aircraft]` map lines. `[Units]`/`[Aircraft]` are
/// `OWNER,ID,HEALTH,X,Y,FACING,MISSION,TAG,…`; `[Infantry]` is
/// `OWNER,ID,HEALTH,X,Y,SUB_CELL,MISSION,FACING,TAG,…` — the neighbours
/// differ, the index does not.
const MISSION_FIELD_INDEX: usize = 6;

/// Resolve the `MISSION=` field at [`MISSION_FIELD_INDEX`], if the line is
/// long enough to have one.
fn parse_mission_field(fields: &[&str]) -> Option<MissionType> {
    fields
        .get(MISSION_FIELD_INDEX)
        .copied()
        .and_then(MissionType::from_map_name)
}

/// Parse all entity placements from a map's INI data.
///
/// Reads [Units], [Aircraft], [Infantry], and [Structures] in the scenario
/// loader's fixed order, independent of their order in the file.
/// Malformed lines are skipped with a warning log. Returns an empty Vec
/// if none of these sections exist (e.g., empty skirmish maps).
pub fn parse_map_entities(ini: &IniFile) -> Vec<MapEntity> {
    let mut entities: Vec<MapEntity> = Vec::new();

    if let Some(section) = ini.section("Units") {
        parse_units_section(section, &mut entities);
    }
    if let Some(section) = ini.section("Aircraft") {
        parse_aircraft_section(section, &mut entities);
    }
    if let Some(section) = ini.section("Infantry") {
        parse_infantry_section(section, &mut entities);
    }
    if let Some(section) = ini.section("Structures") {
        parse_structures_section(section, &mut entities);
    }

    log::info!(
        "Parsed {} map entities ({} units, {} aircraft, {} infantry, {} structures)",
        entities.len(),
        entities
            .iter()
            .filter(|e| e.category == EntityCategory::Unit)
            .count(),
        entities
            .iter()
            .filter(|e| e.category == EntityCategory::Aircraft)
            .count(),
        entities
            .iter()
            .filter(|e| e.category == EntityCategory::Infantry)
            .count(),
        entities
            .iter()
            .filter(|e| e.category == EntityCategory::Structure)
            .count(),
    );

    entities
}

/// One object line as the scenario readers see it: `[Units]` (`0x00743270`),
/// `[Aircraft]` (`0x0041B110`), `[Infantry]` (`0x0051FB00`) and `[Structures]`
/// (`BuildingClass::ReadFromINI @ 0x0044F820`) each ReadString an entry into
/// 0x80 bytes and walk it with `strtok(",")`, numeric fields through CRT
/// `atoi`. Native reads past the last token as `atoi(NULL)`; Rust rejects a
/// line shorter than its required columns instead.
fn object_line_fields<'a>(section: &'a IniSection, key: &str) -> Vec<&'a str> {
    section
        .read_name(key, 0x80)
        .map_or_else(Vec::new, |value| strtok(value, &[',']).collect())
}

/// Parse [Units] section: INDEX=OWNER,ID,HEALTH,X,Y,FACING,MISSION,TAG,...
/// Minimum 6 fields needed (owner, id, health, x, y, facing).
fn parse_units_section(section: &IniSection, entities: &mut Vec<MapEntity>) {
    for key in section.keys() {
        let fields = object_line_fields(section, key);
        if fields.len() < 6 {
            log::warn!(
                "[Units] key {}: expected >= 6 fields, got {}",
                key,
                fields.len()
            );
            continue;
        }
        let Some(entity) = parse_common_fields(&fields, EntityCategory::Unit, key) else {
            continue;
        };
        entities.push(entity);
    }
}

/// Parse [Infantry] section: INDEX=OWNER,ID,HEALTH,X,Y,SUB_CELL,MISSION,FACING,...
/// Note: infantry has SUB_CELL at index 5 and FACING at index 7 (different from units).
fn parse_infantry_section(section: &IniSection, entities: &mut Vec<MapEntity>) {
    for key in section.keys() {
        let fields = object_line_fields(section, key);
        if fields.len() < 8 {
            log::warn!(
                "[Infantry] key {}: expected >= 8 fields, got {}",
                key,
                fields.len()
            );
            continue;
        }
        let owner: String = fields[0].to_string();
        let type_id: String = fields[1].to_string();
        let health: i32 = crt_atoi(fields[2]);
        let Some(cell_x) = cell_coordinate(fields[3]) else {
            log::warn!("[Infantry] key {}: invalid X '{}'", key, fields[3]);
            continue;
        };
        let Some(cell_y) = cell_coordinate(fields[4]) else {
            log::warn!("[Infantry] key {}: invalid Y '{}'", key, fields[4]);
            continue;
        };
        let sub_cell: u8 = u8::try_from(crt_atoi(fields[5])).unwrap_or(0).min(4);
        // Infantry facing is at field index 7 (after MISSION at index 6).
        let facing: u8 = facing_field(fields[7]);
        let veterancy = veterancy_field(fields.get(9).copied());

        entities.push(MapEntity {
            owner,
            type_id,
            health,
            cell_x,
            cell_y,
            facing,
            category: EntityCategory::Infantry,
            sub_cell,
            veterancy,
            high: atoi_flag(fields.get(11).copied()),
            mission: parse_mission_field(&fields),
            recruitable_a: recruitment_flag(fields.get(12).copied()),
            recruitable_b: recruitment_flag(fields.get(13).copied()),
            group: fields.get(10).map_or(-1, |value| crt_atoi(value)),
            structure_upgrades: [None, None, None],
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
            trigger_tag: fields.get(8).map(|value| (*value).to_owned()),
        });
    }
}

/// Parse [Structures] section: INDEX=OWNER,ID,HEALTH,X,Y,FACING,TAG,...
/// Minimum 6 fields needed.
fn parse_structures_section(section: &IniSection, entities: &mut Vec<MapEntity>) {
    for key in section.keys() {
        let fields = object_line_fields(section, key);
        if fields.len() < 6 {
            log::warn!(
                "[Structures] key {}: expected >= 6 fields, got {}",
                key,
                fields.len()
            );
            continue;
        }
        let Some(mut entity) = parse_common_fields(&fields, EntityCategory::Structure, key) else {
            continue;
        };
        entity.structure_upgrades = parse_structure_upgrades(&fields);
        entity.structure_ai_sellable = atoi_flag(fields.get(7).copied());
        entity.structure_ai_repairable = atoi_flag(fields.get(15).copied());
        entity.structure_online = fields.get(9).is_none_or(|value| crt_atoi(value) != 0);
        entities.push(entity);
    }
}

/// Parse [Aircraft] section: INDEX=OWNER,ID,HEALTH,X,Y,FACING,MISSION,TAG,...
/// Minimum 6 fields needed.
fn parse_aircraft_section(section: &IniSection, entities: &mut Vec<MapEntity>) {
    for key in section.keys() {
        let fields = object_line_fields(section, key);
        if fields.len() < 6 {
            log::warn!(
                "[Aircraft] key {}: expected >= 6 fields, got {}",
                key,
                fields.len()
            );
            continue;
        }
        let Some(entity) = parse_common_fields(&fields, EntityCategory::Aircraft, key) else {
            continue;
        };
        entities.push(entity);
    }
}

/// Parse the common fields shared by Units, Structures, and Aircraft.
///
/// Field layout: OWNER(0), ID(1), HEALTH(2), X(3), Y(4), FACING(5). Units
/// continue MISSION(6), TAG(7), VETERANCY(8), GROUP(9), HIGH(10),
/// FOLLOWS(11), RECRUIT_A(12), RECRUIT_B(13); aircraft have no HIGH or
/// FOLLOWS column, so their recruitment bytes are fields 10 and 11
/// (`0x0041B110`).
fn parse_common_fields(fields: &[&str], category: EntityCategory, key: &str) -> Option<MapEntity> {
    let owner: String = fields[0].to_string();
    let type_id: String = fields[1].to_string();
    let health: i32 = crt_atoi(fields[2]);

    let Some(cell_x) = cell_coordinate(fields[3]) else {
        log::warn!("[{:?}] key {}: invalid X '{}'", category, key, fields[3]);
        return None;
    };
    let Some(cell_y) = cell_coordinate(fields[4]) else {
        log::warn!("[{:?}] key {}: invalid Y '{}'", category, key, fields[4]);
        return None;
    };

    let facing: u8 = facing_field(fields[5]);
    // `[Structures]` field 8 is the AI-rebuild flag; a building line has no
    // veterancy column.
    let veterancy = match category {
        EntityCategory::Structure => None,
        _ => veterancy_field(fields.get(8).copied()),
    };

    // `[Structures]` lines have no MISSION column — a building cannot be
    // map-authored onto a mission, and index 6 there is the trigger TAG.
    let mission: Option<MissionType> = match category {
        EntityCategory::Unit | EntityCategory::Aircraft => parse_mission_field(fields),
        EntityCategory::Structure | EntityCategory::Infantry => None,
    };
    let (recruitable_a, recruitable_b) = match category {
        EntityCategory::Structure => (true, true),
        EntityCategory::Aircraft => (
            recruitment_flag(fields.get(10).copied()),
            recruitment_flag(fields.get(11).copied()),
        ),
        EntityCategory::Unit | EntityCategory::Infantry => (
            recruitment_flag(fields.get(12).copied()),
            recruitment_flag(fields.get(13).copied()),
        ),
    };

    Some(MapEntity {
        owner,
        type_id,
        health,
        cell_x,
        cell_y,
        facing,
        category,
        sub_cell: 0,
        veterancy,
        high: matches!(category, EntityCategory::Unit) && atoi_flag(fields.get(10).copied()),
        mission,
        recruitable_a,
        recruitable_b,
        group: if category == EntityCategory::Structure {
            -1
        } else {
            fields.get(9).map_or(-1, |value| crt_atoi(value))
        },
        structure_upgrades: [None, None, None],
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
        trigger_tag: fields
            .get(if category == EntityCategory::Structure {
                6
            } else {
                7
            })
            .map(|value| (*value).to_owned()),
    })
}

/// Active `BuildingClass::ReadFromINI @ 0x0044F820` stores the declared
/// installed-upgrade count at retail `[Structures]` field 10; its loop at
/// `0x0044FD50..0x0044FDC3` visits type selectors 12..14 only within that
/// prefix and skips selectors resolving to `-1` before construction.
fn parse_structure_upgrades(fields: &[&str]) -> [Option<String>; 3] {
    let declared = fields
        .get(10)
        .map_or(0, |value| crt_atoi(value))
        .clamp(0, 3) as usize;
    std::array::from_fn(|slot| {
        if slot >= declared {
            return None;
        }
        let value = *fields.get(12 + slot)?;
        if value.eq_ignore_ascii_case("none") || value == "-1" {
            None
        } else {
            Some(value.to_string())
        }
    })
}

/// A cell column: `atoi` narrowed to `short` as the readers do. A negative
/// cell is off the map, where native's Unlimbo fails; Rust skips the line.
fn cell_coordinate(token: &str) -> Option<u16> {
    u16::try_from(crt_atoi(token) as i16).ok()
}

/// The facing column's `atoi`, kept to the byte range the Rust entity holds.
fn facing_field(token: &str) -> u8 {
    crt_atoi(token).clamp(0, 255) as u8
}

/// Unit743495, Infantry51FD54 and Aircraft41B308 skip SetFromPercent for
/// a null token; otherwise the original CRTatoi7C9BFD supplies a signed int.
/// Executed controls: tools/input_oracle/selection_navigation.json.
fn veterancy_field(token: Option<&str>) -> Option<i32> {
    token.map(crt_atoi)
}

/// An optional `atoi != 0` column; absent is false.
fn atoi_flag(token: Option<&str>) -> bool {
    token.is_some_and(|token| crt_atoi(token) != 0)
}

/// A recruitment byte: `atoi != 0` when present, else the constructor's true.
fn recruitment_flag(token: Option<&str>) -> bool {
    token.is_none_or(|token| crt_atoi(token) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn authored_health_preserves_signed_dword_before_class_admission() {
        for section in ["Units", "Aircraft", "Infantry", "Structures"] {
            for health in [i32::MIN, -1, 0, 256, 512, i32::MAX] {
                let ini = IniFile::from_str(&format!(
                    "[{section}]\n0=Americans,TYPE,{health},30,40,0,Guard,0,None,0,-1,false,true,false\n"
                ));
                let entities = parse_map_entities(&ini);
                assert_eq!(entities.len(), 1, "{section}");
                assert_eq!(entities[0].health, health, "{section}");
            }
        }
    }

    #[test]
    fn test_parse_units() {
        let ini: IniFile = IniFile::from_str(
            "[Units]\n\
             0=Americans,MTNK,256,30,40,64,Guard,None,0,-1,0,-1,1,0\n\
             1=Soviet,HTNK,200,50,60,128,Guard,None,100,-1,0,-1,0,0\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);
        assert_eq!(entities.len(), 2);

        assert_eq!(entities[0].owner, "Americans");
        assert_eq!(entities[0].type_id, "MTNK");
        assert_eq!(entities[0].health, 256);
        assert_eq!(entities[0].cell_x, 30);
        assert_eq!(entities[0].cell_y, 40);
        assert_eq!(entities[0].facing, 64);
        assert_eq!(entities[0].category, EntityCategory::Unit);
        assert_eq!(entities[0].veterancy, Some(0));
        assert!(!entities[0].high);
        assert!(entities[0].recruitable_a);
        assert!(!entities[0].recruitable_b);

        assert_eq!(entities[1].owner, "Soviet");
        assert_eq!(entities[1].type_id, "HTNK");
        assert_eq!(entities[1].health, 200);
        assert_eq!(entities[1].facing, 128);
        assert_eq!(entities[1].veterancy, Some(100));
        assert!(!entities[1].high);
        assert!(!entities[1].recruitable_a);
        assert!(!entities[1].recruitable_b);
    }

    #[test]
    fn structures_online_token_uses_native_default_and_crt_flags() {
        //44FA11 defaults true;44FA2D uses atoi!=0 after native strtok.
        let ini = IniFile::from_str(
            "[Structures]\n0=A,B,256,1,1,0\n1=A,B,256,2,1,0,None,0,0,0\n2=A,B,256,3,1,0,None,0,0,-7tail\n3=A,B,256,4,1,0,None,0,0,bogus\n",
        );
        assert_eq!(
            parse_map_entities(&ini)
                .iter()
                .map(|e| e.structure_online)
                .collect::<Vec<_>>(),
            [true, false, true, false]
        );
    }

    /// `BuildingClass::ReadFromINI @ 0x0044F820` stores field 7's `atoi != 0`
    /// as the AI sale byte (`0x0044FB5B`); a line that stops short leaves it
    /// false.
    #[test]
    fn structures_carry_their_ai_sellable_and_ai_repairable_fields() {
        let ini = IniFile::from_str(
            "[Structures]\n\
             0=Americans,GAPOWR,256,15,25,0,None,1,0,1,0,0,None,None,None,1,0\n\
             1=Americans,GAPOWR,256,17,25,0,None,0,1,1,0,0,None,None,None,1,0\n\
             2=Americans,GAPOWR,256,19,25,0,None\n\
             3=Americans,GAPOWR,256,21,25,0,None,true\n",
        );
        let entities = parse_map_entities(&ini);
        let sellable: Vec<bool> = entities
            .iter()
            .map(|entity| entity.structure_ai_sellable)
            .collect();
        assert_eq!(sellable, [true, false, false, false]);
        let repairable: Vec<bool> = entities
            .iter()
            .map(|entity| entity.structure_ai_repairable)
            .collect();
        assert_eq!(repairable, [true, true, false, false]);
        // Field 8 (AI rebuild, 1 on line 1) is no veterancy.
        assert!(entities.iter().all(|entity| entity.veterancy.is_none()));
    }

    #[test]
    fn techno_constructor_structure_upgrades_follow_declared_slot_prefix() {
        let ini = IniFile::from_str(
            "[Structures]\n\
             0=Americans,GAPOWR,256,15,25,0,None,1,0,1,2,0,GAPOWRUP,None,IGNORED,0,1\n",
        );

        let entities = parse_map_entities(&ini);

        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].structure_upgrades,
            [Some("GAPOWRUP".to_string()), None, None]
        );
    }

    /// Aircraft lines have no HIGH/FOLLOWS columns: `0x0041B110` reads the
    /// recruitment bytes from fields 10 and 11 (retail `mapsmd03.mix`
    /// `[Aircraft]` lines end `0,-1,1,1`).
    #[test]
    fn test_parse_aircraft() {
        let ini: IniFile = IniFile::from_str(
            "[Aircraft]\n\
             0=Soviet,DRON,256,50,50,0,Guard,None,0,-1,1,1\n\
             1=Soviet,DRON,256,51,50,0,Guard,None,0,-1,1,0\n\
             2=Soviet,DRON,256,52,50,0,Guard,None,0,-1\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);
        assert_eq!(entities.len(), 3);
        assert_eq!(entities[0].type_id, "DRON");
        assert_eq!(entities[0].category, EntityCategory::Aircraft);
        assert!(!entities[0].high);
        let recruitment: Vec<(bool, bool)> = entities
            .iter()
            .map(|entity| (entity.recruitable_a, entity.recruitable_b))
            .collect();
        assert_eq!(recruitment, [(true, true), (true, false), (true, true)]);
    }

    #[test]
    fn test_parse_high_for_units_and_infantry() {
        let ini: IniFile = IniFile::from_str(
            "[Units]\n\
             0=Americans,MTNK,256,30,40,64,Guard,None,0,-1,1,-1,0,0\n\
             [Infantry]\n\
             0=Soviet,E1,256,10,20,2,Guard,192,None,200,-1,1,0\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);
        assert_eq!(entities.len(), 2);
        assert!(entities[0].high);
        assert!(entities[1].high);
    }

    #[test]
    fn test_unit_high_uses_atoi_nonzero_semantics() {
        let ini: IniFile = IniFile::from_str(
            "[Units]\n\
             0=Americans,MTNK,256,30,40,64,Guard,None,0,-1,yes,-1,false,false\n\
             1=Americans,MTNK,256,31,40,64,Guard,None,0,-1,1,-1,false,false\n\
             2=Americans,MTNK,256,32,40,64,Guard,None,0,-1,-1,-1,false,false\n\
             3=Americans,MTNK,256,33,40,64,Guard,None,0,-1\n\
             4=Americans,MTNK,256,34,40,64,Guard,None,0,-1,garbage,-1,false,false\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);
        assert_eq!(entities.len(), 5);

        assert!(!entities[0].high, "High=yes parses as atoi 0");
        assert!(entities[1].high, "High=1 parses as nonzero");
        assert!(entities[2].high, "High=-1 parses as nonzero");
        assert!(!entities[3].high, "missing High defaults false");
        assert!(!entities[4].high, "nonnumeric High parses as atoi 0");
    }

    #[test]
    fn gsi_04_05_missing_recruitment_tail_defaults_both_bytes_true() {
        let ini = IniFile::from_str(
            "[Units]\n0=Americans,MTNK,256,30,40,64,Guard,None,0,-1\n\
             [Infantry]\n0=Soviet,E1,256,10,20,2,Guard,192\n",
        );
        let entities = parse_map_entities(&ini);
        assert_eq!(entities.len(), 2);
        for entity in entities {
            assert!(entity.recruitable_a);
            assert!(entity.recruitable_b);
        }
    }

    /// Literal retail lines: `[Units]` from `EB4.mmx` and `[Infantry]` from
    /// `Arena.mmx`, both `GameMode=standard` skirmish maps. The facing and
    /// sub-cell assertions are the tripwire proving the column indices did not
    /// shift when MISSION started being read.
    #[test]
    fn retail_lines_resolve_their_authored_mission() {
        let ini: IniFile = IniFile::from_str(
            "[Units]\n\
             0=Neutral,PTRUCK,256,123,85,0,Sticky,None,0,-1,0,-1,1,1\n\
             1=Neutral,TRUCKA,256,69,101,0,Guard,None,0,-1,0,-1,1,1\n\
             [Infantry]\n\
             0=Neutral,CIVBTM,256,113,68,2,Sticky,192,None,0,-1,0,1,1\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);
        assert_eq!(entities.len(), 3);

        assert_eq!(entities[0].type_id, "PTRUCK");
        assert_eq!(entities[0].mission, Some(MissionType::Sticky));
        assert_eq!(entities[0].facing, 0);
        assert_eq!(entities[1].mission, Some(MissionType::Guard));

        let infantry = &entities[2];
        assert_eq!(infantry.category, EntityCategory::Infantry);
        assert_eq!(infantry.mission, Some(MissionType::Sticky));
        assert_eq!(infantry.sub_cell, 2);
        assert_eq!(infantry.facing, 192);
    }

    /// `Mission_From_Name` is a case-insensitive table scan returning `-1` for
    /// anything absent or unrecognised — and `-1` is not `Sleep(0)`. The
    /// spaced `Area Guard` is the name a naive lookup drops.
    #[test]
    fn map_mission_column_matches_mission_from_name() {
        let ini: IniFile = IniFile::from_str(
            "[Units]\n\
             0=Neutral,HTNK,256,50,50,0,Sleep,None,0,-1,0,-1,1,1\n\
             1=Neutral,HTNK,256,51,50,0,Area Guard,None,0,-1,0,-1,1,1\n\
             2=Neutral,HTNK,256,52,50,0,Nonsense,None,0,-1,0,-1,1,1\n\
             3=Neutral,HTNK,256,53,50,0,sticky,None,0,-1,0,-1,1,1\n\
             4=Neutral,HTNK,256,54,50,0\n\
             [Aircraft]\n\
             0=Soviet,DRON,256,60,60,0,Sleep,None,0,-1,false,false\n\
             [Structures]\n\
             0=Americans,GAPOWR,256,15,25,0,None,true,false,true,0,0,None,None,None,false,true\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);

        let units: Vec<&MapEntity> = entities
            .iter()
            .filter(|e| e.category == EntityCategory::Unit)
            .collect();
        assert_eq!(units.len(), 5);
        assert_eq!(units[0].mission, Some(MissionType::Sleep));
        assert_eq!(
            units[1].mission,
            Some(MissionType::AreaGuard),
            "the spaced name must resolve"
        );
        assert_eq!(
            units[2].mission, None,
            "an unknown name is the -1 sentinel, not Sleep(0)"
        );
        assert_eq!(units[3].mission, Some(MissionType::Sticky));
        assert_eq!(units[4].mission, None, "a line with no MISSION field");

        let aircraft = entities
            .iter()
            .find(|e| e.category == EntityCategory::Aircraft)
            .expect("aircraft parsed");
        assert_eq!(aircraft.mission, Some(MissionType::Sleep));

        let structure = entities
            .iter()
            .find(|e| e.category == EntityCategory::Structure)
            .expect("structure parsed");
        assert_eq!(
            structure.mission, None,
            "[Structures] has no MISSION column; index 6 there is the TAG"
        );
    }

    #[test]
    fn test_malformed_lines_skipped() {
        let ini: IniFile = IniFile::from_str(
            "[Units]\n\
             0=Americans,MTNK\n\
             1=Soviet,HTNK,256,50,60,128,Guard,None,0,-1,false,-1,false,false\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);
        // First line has only 2 fields (< 6 minimum), should be skipped.
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].type_id, "HTNK");
    }

    #[test]
    fn map_entities_follow_native_loader_order_not_file_section_order() {
        let ini: IniFile = IniFile::from_str(
            "[Structures]\n\
             0=Americans,GAPOWR,256,15,25,0,None,1,0,1,0,0,None,None,None,0,1\n\
             [Infantry]\n\
             0=Soviet,E1,256,10,20,0,Guard,0,None,0,-1,0,1,0\n\
             [Aircraft]\n\
             0=Soviet,DRON,256,50,50,0,Guard,None,0,-1,1,1\n\
             [Units]\n\
             0=Americans,MTNK,256,30,40,64,Guard,None,0,-1,0,-1,1,0\n",
        );
        let entities: Vec<MapEntity> = parse_map_entities(&ini);
        assert_eq!(entities.len(), 4);
        assert_eq!(
            entities
                .iter()
                .map(|entity| (entity.category, entity.type_id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (EntityCategory::Unit, "MTNK"),
                (EntityCategory::Aircraft, "DRON"),
                (EntityCategory::Infantry, "E1"),
                (EntityCategory::Structure, "GAPOWR"),
            ]
        );
    }
}
