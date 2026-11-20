//! Phobos `TypeConvertGroup` (`New/Type/Affiliated/TypeConvertGroup.cpp`):
//! a type-pair conversion list carried by a WarheadType or SuperWeaponType.
//!
//! Parsing (`:59-125`): numbered pairs `Convert{0..}.From` (a
//! `ValueableVector<TechnoTypeClass*>`), `.To` (a `Nullable<TechnoTypeClass*>`,
//! the loop stops at the first index whose value is unset) and
//! `.AffectedHouses` (deprecated) then `.AffectsHouse` (overrides when set).
//! After the numbered loop, the unnumbered `Convert.From`/`.To` pair replaces
//! entry 0 when a list exists, else appends. An unset house takes the
//! consumer's default (Owner for SuperWeaponType, All for WarheadType).
//!
//! Applying (`:5-40`), in order: skip a pair without `To`; skip when the
//! source house is known and `CanTargetHouse(AffectsHouse, source,
//! target.Owner)` fails; an empty `From` list converts the target and stops,
//! otherwise the first `From` match converts and stops.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

/// One `Convert{i}.*` row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TypeConvertGroup {
    /// `Convert{i}.From=`: the source types this row accepts; empty converts
    /// any Foot.
    pub from: Vec<String>,
    /// `Convert{i}.To=`: the target type; `None` ends the numbered list.
    pub to: Option<String>,
    /// `Convert{i}.AffectsHouse=` (or the deprecated `.AffectedHouses`): the
    /// source-house filter, defaulted by the consumer.
    pub affected_houses: AffectedHouse,
}

/// `TypeConvertGroup::Parse`: the numbered rows then the unnumbered single
/// pair, with `default_house` for rows whose house key is unset.
pub fn parse(section: &IniSection, default_house: AffectedHouse) -> Vec<TypeConvertGroup> {
    let mut list = Vec::new();
    for index in 0.. {
        let prefix = format!("Convert{index}");
        let from = type_list(section, &format!("{prefix}.From"));
        let to = section
            .read_type_name(&format!("{prefix}.To"), 0x800)
            .map(str::to_owned);
        let mut houses = optional_house(section, &format!("{prefix}.AffectedHouses"));
        if let Some(value) = optional_house(section, &format!("{prefix}.AffectsHouse")) {
            houses = Some(value);
        }
        // `if (!convertTo.isset()) break;` — an absent or `none` value ends
        // the numbered list (the unnumbered pair below may still fill it).
        let Some(to) = to else {
            break;
        };
        list.push(TypeConvertGroup {
            from,
            to: Some(to),
            affected_houses: houses.unwrap_or(default_house),
        });
    }
    let from = type_list(section, "Convert.From");
    let to = section
        .read_type_name("Convert.To", 0x800)
        .map(str::to_owned);
    let mut houses = optional_house(section, "Convert.AffectedHouses");
    if let Some(value) = optional_house(section, "Convert.AffectsHouse") {
        houses = Some(value);
    }
    if let Some(to) = to {
        let pair = TypeConvertGroup {
            from,
            to: Some(to),
            affected_houses: houses.unwrap_or(default_house),
        };
        if list.is_empty() {
            list.push(pair);
        } else {
            // `list[0] = { ... }` with the later pair's values.
            list[0] = pair;
        }
    }
    list
}

/// `ValueableVector<TechnoTypeClass*>` tokens: `strtok(",")` keeps spaces;
/// the parse-time `Find` drops unknown names, which the consumer emulates by
/// filtering `From` against the built rules (an all-unknown list is empty and
/// converts any Foot, `TemplateDef.h:1728-1744`). `To` is likewise resolved
/// there: an unknown name ends the numbered list (`TemplateDef.h:1834-1849`).
fn type_list(section: &IniSection, key: &str) -> Vec<String> {
    section
        .read_list(key, 0x800)
        .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// `Nullable<AffectedHouse>`: absent, blank or invalid leaves `None`.
fn optional_house(section: &IniSection, key: &str) -> Option<AffectedHouse> {
    section
        .read_name(key, 2048)
        .and_then(|raw| AffectedHouse::parse(&raw))
}
