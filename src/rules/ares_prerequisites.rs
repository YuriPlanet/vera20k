//! Ares generic-prerequisite definitions, retained by the Rules registry.
//!
//! Native identity: Ares 3.0 SHA-256
//! `93ce890f387a6c1b3240a2dd7b674f25fec5a0e96f0b0d0306796b2edf7b5b83`.
//! `GenericPrerequisite::LoadFromINI` @ 0x10008470 and `Prereqs::Parse`
//! @ 0x100085F0 read retained lists; `AddDefaults` @ 0x10007DB0 seeds the
//! original six names. Source lead: Ares `src/Enum/Prerequisites.{cpp,h}`.
//! Saved bounded execution controls and their supplied leaves are recorded in
//! `tools/rules_oracle/ares_prerequisites.{json,meta.json,md}`.
//!
//! This module owns catalog reads and immutable projections. The retained
//! `NativeRulesRegistryState` owns its lifetime and chooses reached passes;
//! simulation consumers own House/planned-building evaluation. Neither group
//! members nor alternate types are recursively expanded here.

#[cfg(feature = "ares")]
use crate::rules::ini_parser::IniFile;
#[cfg(feature = "ares")]
use crate::rules::ini_value::truncate_native_bytes;
use crate::rules::object_type::ObjectCategory;
#[cfg(feature = "ares")]
use crate::rules::prerequisite::parse_prerequisites;
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup};

#[cfg(feature = "ares")]
const READ_CAPACITY: usize = 0x800;
#[cfg(feature = "ares")]
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// A reached TechnoType identity used by a group's runtime alternate list.
/// Category distinguishes equal names admitted by different type families.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PrerequisiteAlternate {
    category: ObjectCategory,
    id: String,
}

impl PrerequisiteAlternate {
    pub(crate) fn category(&self) -> ObjectCategory {
        self.category
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

/// One entry of `Enumerable<GenericPrerequisite>::Array`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct GenericPrerequisite {
    name: String,
    members: Vec<Prerequisite>,
    alternates: Vec<PrerequisiteAlternate>,
}

impl GenericPrerequisite {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn members(&self) -> &[Prerequisite] {
        &self.members
    }

    pub(crate) fn alternates(&self) -> &[PrerequisiteAlternate] {
        &self.alternates
    }
}

/// Ordered Ares definitions. The initial six slots are the native groups;
/// custom slots use their full Array index, starting at six.
/// `NativeRulesRegistryState` owns this value; RuleSet borrows its projection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct GenericPrerequisites {
    groups: Vec<GenericPrerequisite>,
}

impl Default for GenericPrerequisites {
    fn default() -> Self {
        Self {
            groups: PrerequisiteGroup::ALL
                .into_iter()
                .map(|group| GenericPrerequisite {
                    name: group.token().to_owned(),
                    members: Vec::new(),
                    alternates: Vec::new(),
                })
                .collect(),
        }
    }
}

impl GenericPrerequisites {
    pub(crate) fn groups(&self) -> &[GenericPrerequisite] {
        &self.groups
    }

    pub(crate) fn group(&self, index: usize) -> Option<&GenericPrerequisite> {
        self.groups.get(index)
    }

    /// Native case-insensitive lookup retaining the initial spelling. The
    /// shared prerequisite parser first tries the BuildingType registry.
    #[cfg(feature = "ares")]
    pub(crate) fn group_entry(&self, token: &str) -> Option<Prerequisite> {
        let index = self
            .groups
            .iter()
            .position(|group| group.name.eq_ignore_ascii_case(token))?;
        Some(match PrerequisiteGroup::ALL.get(index) {
            Some(&group) => Prerequisite::Group(group),
            None => Prerequisite::GenericGroup(u32::try_from(index).ok()?),
        })
    }

    /// Read one reached Rules pass. The processing owner supplies existing
    /// registries and read order; this never reparses a flattened INI.
    ///
    /// Register all keys before member reads so forward references bind.
    /// Missing/empty reads retain their lists; nonempty reads replace them,
    /// including comma-only and all-unknown lists. LoadFromINI10008470 reads
    /// General members, generic members, then General alternate types.
    #[cfg(feature = "ares")]
    pub(crate) fn apply_pass(
        &mut self,
        pass: &IniFile,
        building_index: impl Fn(&str) -> Option<i32>,
        techno: impl Fn(&str) -> Option<(ObjectCategory, String)>,
    ) {
        let custom = pass.section_or_empty("GenericPrerequisites");
        for name in custom.keys() {
            // Enumerable::FindOrAllocate @ 0x10007FD0 compares the full
            // incoming key before storing Name[32] with _TRUNCATE. Distinct
            // long keys can allocate repeated truncated names: never merge
            // them by truncating before the lookup.
            if self.group_entry(name).is_none() {
                self.groups.push(GenericPrerequisite {
                    name: truncate_native_bytes(name, NAME_PAYLOAD_BYTES).to_owned(),
                    members: Vec::new(),
                    alternates: Vec::new(),
                });
            }
        }

        let general = pass.section_or_empty("General");
        for index in 0..self.groups.len() {
            let name = self.groups[index].name.clone();
            let mut general_name = name.to_ascii_lowercase();
            // LoadFromINI1000848C lowercases; 1000849A..A3 uppercases the
            // first byte only in a-z. The executed controls supply ASCII
            // CRT leaves; non-ASCII locale folding remains a residual.
            if let Some(first) = general_name.chars().next() {
                let first_upper = first.to_ascii_uppercase();
                general_name.replace_range(..first.len_utf8(), &first_upper.to_string());
            }
            let general_key = format!("Prerequisite{general_name}");
            let alternate_key = format!("{general_key}Alternate");

            let members = parse_prerequisites(
                general.read_list(&general_key, READ_CAPACITY),
                &self.groups[index].members,
                &building_index,
                Some(self),
            );
            let members = parse_prerequisites(
                custom.read_list(&name, READ_CAPACITY),
                &members,
                &building_index,
                Some(self),
            );
            self.groups[index].members = members;

            if let Some(tokens) = general.read_list(&alternate_key, READ_CAPACITY) {
                self.groups[index].alternates = tokens
                    .into_iter()
                    .filter_map(&techno)
                    .map(|(category, id)| PrerequisiteAlternate { category, id })
                    .collect();
            }
        }
    }
}

#[cfg(all(test, feature = "ares"))]
#[path = "ares_prerequisites_tests.rs"]
mod tests;
