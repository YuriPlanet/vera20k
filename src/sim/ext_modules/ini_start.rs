//! The start-of-match records of RFC 0001 section 6h (INI registration):
//! before a module's
//! `on_scenario_start`, every entry of its registries (`IniEntry`), then
//! every section that carries one of its keys (`IniKey`), each in
//! registration order, read from the merged rules text after every layer.
//! They go to `on_events` with the handler's dispatch key, outside the
//! per-frame record limits, and count against `on_scenario_start`'s
//! one-time budget.

use vera20k_wasm_ext::triggers::{EVENT_INI_ENTRY, EVENT_INI_KEY};
use vera20k_wasm_ext::{
    EventRecord, Fault, INI_KEY, INI_REGISTRY, NAME_COUNTRY, NAME_PROJECTILE, NAME_SUPER_WEAPON,
    NAME_TECHNO_TYPE, NAME_WARHEAD, NAME_WEAPON, SimModule,
};

use super::{REF_GLOBAL, REF_KIND_SHIFT, rules_id_name, type_ref};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, Simulation};

/// `IniSection` 6: the one `[General]` section.
const SECTION_GENERAL: i32 = 6;

/// The name kind of each `IniSection` but `General`, in enum order.
const SECTION_KINDS: [i32; 6] = [
    NAME_TECHNO_TYPE,
    NAME_WEAPON,
    NAME_WARHEAD,
    NAME_PROJECTILE,
    NAME_SUPER_WEAPON,
    NAME_COUNTRY,
];

/// A key's trimmed value in `[section]`; `None` when absent or empty.
fn value_in(ini: &IniFile, section: &str, key: &str) -> Option<String> {
    let value = ini.section(section)?.raw_value(key)?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The sections of `IniSection` `section` with their type references, in
/// ID order.
fn sections_of(rules: &RuleSet, section: i32) -> Vec<(String, i64)> {
    if section == SECTION_GENERAL {
        return vec![("General".to_owned(), REF_GLOBAL << REF_KIND_SHIFT)];
    }
    let Some(&kind) = usize::try_from(section)
        .ok()
        .and_then(|i| SECTION_KINDS.get(i))
    else {
        return Vec::new();
    };
    (0u32..)
        .map_while(|id| {
            let name = rules_id_name(rules, kind, id as i32)??;
            Some((name.to_owned(), type_ref(kind, id)))
        })
        .collect()
}

impl Simulation {
    /// The `IniEntry` and `IniKey` records of module `index`'s source, in
    /// delivery order. Interns the entry names and values.
    fn ini_start_records(&mut self, rules: &RuleSet, index: usize) -> Vec<EventRecord> {
        let source = self.ext_modules.slots[index].source;
        let Some(merged) = self.ext_modules.rules_ini.take() else {
            return Vec::new();
        };
        let mut records = Vec::new();
        let frame = self.session.binary_frame() as i32;
        let registries: Vec<(String, i32)> = self
            .ext_modules
            .ini
            .of(source, INI_REGISTRY)
            .map(|r| (r.name.clone(), r.key))
            .collect();
        for (list, key) in registries {
            let Some(section) = merged.section(&list) else {
                continue;
            };
            let mut seen: Vec<String> = Vec::new();
            for entry_key in section.keys() {
                let Some(entry) = value_in(&merged, &list, entry_key) else {
                    continue;
                };
                if seen.iter().any(|s| s.eq_ignore_ascii_case(&entry)) {
                    continue;
                }
                let args = self.ext_modules.ini.intern(&entry);
                records.push(EventRecord {
                    key,
                    value: seen.len() as i64,
                    frame,
                    args,
                    ..EventRecord::empty(EVENT_INI_ENTRY)
                });
                seen.push(entry);
            }
        }
        let keys: Vec<(String, i32, i32)> = self
            .ext_modules
            .ini
            .of(source, INI_KEY)
            .map(|r| (r.name.clone(), r.section, r.key))
            .collect();
        for (name, section, key) in keys {
            for (section_name, subject) in sections_of(rules, section) {
                let Some(value) = value_in(&merged, &section_name, &name) else {
                    continue;
                };
                let args = self.ext_modules.ini.intern(&value);
                records.push(EventRecord {
                    key,
                    subject,
                    frame,
                    args,
                    ..EventRecord::empty(EVENT_INI_KEY)
                });
            }
        }
        self.ext_modules.rules_ini = Some(merged);
        records
    }

    /// Hands module `index` its start records; returns the operations they
    /// used, which `on_scenario_start`'s one-time budget then lacks.
    pub(super) fn deliver_ini_start_records(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
        index: usize,
        budget: u64,
    ) -> Result<u64, Fault> {
        let records = self.ini_start_records(rules, index);
        if records.is_empty() {
            return Ok(0);
        }
        let capacity = self.ext_modules.slots[index].module.batch_capacity();
        if capacity == 0 {
            return Err(Fault::Invalid(format!(
                "{} INI start records need on_events and a vera.delivery area",
                records.len()
            )));
        }
        let mut used = 0u64;
        for batch in records.chunks(capacity) {
            let left = budget.checked_sub(used).filter(|&left| left > 0);
            let Some(left) = left else {
                return Err(Fault::Budget);
            };
            used += self.with_ext_module(
                rules,
                overlay_registry,
                frame_effects,
                index,
                |module: &mut SimModule, host| module.deliver(batch, left, host),
            )?;
        }
        Ok(used)
    }
}
