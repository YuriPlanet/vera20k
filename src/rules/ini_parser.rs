//! Physical INI representation for Westwood data: the `INIClass` analog.
//! Values are read through the `read_*` readers in `rules::ini_value`.
//!
//! Only the store and its readers see raw value text: `ini_value` is a child
//! module, and a private item is visible to its module's descendants alone.
//! Everyone else reads through a reader or tests presence with
//! [`IniSection::is_present`]. Two walks hand out stored text:
//! [`IniSection::raw_entries`], to copy or show entries, and
//! [`IniSection::registry_ids`], for registries `native_processing` rewrote;
//! `architecture_guards` pins their production callers. Tests inspect storage
//! through the test-only `get_for_test`.
//!
//! Active `gamemd.exe` treats raw section and key names as case-sensitive.
//! A fresh load retains duplicate nonempty section bodies. Empty keys, values,
//! and physical section bodies are not inserted. Arbitrary duplicate-name
//! lookup remains a native CRC/qsort exactification residual.
//! Native registry allocation and ordered RulesClass passes live in
//! `rules::native_processing`; typed gameplay projection lives in `ruleset`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use crate::rules::error::RulesError;

#[path = "ini_value.rs"]
pub mod ini_value;

use self::ini_value::strtrim_ascii;

const READ_LINE_PAYLOAD: usize = 511;

/// One physical section occurrence in an INI file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniSection {
    /// Exact section spelling from the file.
    pub name: String,
    /// Exact keys and their values. Initial duplicate keys are first-wins.
    entries: HashMap<String, String>,
    /// Exact keys in their first insertion order.
    key_order: Vec<String>,
    /// Values presented by successive `RulesClass::Process` passes.
    ///
    /// Raw INIs leave this empty. The compatibility projection retains it so
    /// typed readers can use the current live field as the next pass default.
    projected_values: HashMap<String, Vec<String>>,
}

impl IniSection {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            entries: HashMap::new(),
            key_order: Vec::new(),
            projected_values: HashMap::new(),
        }
    }

    pub(super) fn overlay_rules_pass(&mut self, patch: &IniSection) {
        for key in patch.keys() {
            if let Some(value) = patch.get(key) {
                self.set_projected(key, value);
            }
        }
    }

    fn set_projected(&mut self, key: &str, value: &str) {
        if !self.entries.contains_key(key) {
            self.key_order.push(key.to_string());
        }
        self.entries.insert(key.to_string(), value.to_string());
        self.projected_values
            .entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }

    /// Insert during a fresh file load. The Rust compatibility lookup keeps
    /// the first exact duplicate; native multi-duplicate CRC/qsort selection
    /// is intentionally outside the ordinary-retail contract.
    fn insert_initial(&mut self, key: &str, value: &str) {
        if self.entries.contains_key(key) {
            return;
        }
        self.key_order.push(key.to_string());
        self.entries.insert(key.to_string(), value.to_string());
    }

    /// Apply a later load/rules pass. Existing exact keys are replaced and new
    /// keys retain layer source order.
    pub(crate) fn set(&mut self, key: &str, value: &str) {
        if !self.entries.contains_key(key) {
            self.key_order.push(key.to_string());
        }
        self.entries.insert(key.to_string(), value.to_string());
        self.projected_values.remove(key);
    }

    /// [`Self::set`] each of a later layer's entries in its source order.
    pub(crate) fn overlay(&mut self, patch: &IniSection) {
        for key in patch.keys() {
            if let Some(value) = patch.get(key) {
                self.set(key, value);
            }
        }
    }

    /// The section every reader sees when the named section is absent: no
    /// entries, so each `read_*` returns its default, as a native reader does
    /// for a missing section.
    pub fn empty() -> &'static IniSection {
        static EMPTY: LazyLock<IniSection> = LazyLock::new(|| IniSection::new(String::new()));
        &EMPTY
    }

    /// The raw stored text of an exact-case key, private to the store and its
    /// readers.
    fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Test builds only: the raw stored text, for tests that inspect storage.
    #[cfg(test)]
    pub(crate) fn get_for_test(&self, key: &str) -> Option<&str> {
        self.get(key)
    }

    /// Whether an exact-case key is stored, whatever its value.
    pub fn is_present(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    fn projected_values(&self, key: &str) -> Option<&[String]> {
        self.projected_values.get(key).map(Vec::as_slice)
    }

    /// The type IDs of a registry section `native_processing` rewrote with
    /// native stored IDs, such as `[OverlayTypes]` or `[Animations]`, in
    /// source order. A raw INI registry walk reads each entry through
    /// [`Self::read_name`] with its native capacity instead.
    /// `architecture_guards` pins the production callers.
    pub fn registry_ids(&self) -> Vec<&str> {
        self.key_order
            .iter()
            .filter_map(|key| self.entries.get(key).map(String::as_str))
            .collect()
    }

    /// Every entry's key and stored text in source order, for copying entries
    /// between stores and for diagnostic display. Never interpret the text:
    /// read values through the `ini_value` readers. `architecture_guards`
    /// pins the production callers.
    pub fn raw_entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.key_order
            .iter()
            .filter_map(|key| Some((key.as_str(), self.entries.get(key)?.as_str())))
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.key_order.iter().map(String::as_str)
    }
}

/// Parsed section occurrences plus Rust's deterministic exact-name lookup index
/// for the supported ordinary INI contract.
#[derive(Debug, Clone)]
pub struct IniFile {
    sections: Vec<IniSection>,
    first_section: HashMap<String, usize>,
}

impl IniFile {
    pub(crate) fn empty() -> Self {
        Self {
            sections: Vec::new(),
            first_section: HashMap::new(),
        }
    }

    /// Supply recorded native cache sections without physical parsing. Some
    /// oracle controls contain stored-empty entries or sections, which the
    /// physical reader would discard. Storage and first-section indexing stay
    /// with this owner; production projection mutators remain private.
    #[cfg(test)]
    pub(crate) fn from_sections_for_test(sections: impl IntoIterator<Item = IniSection>) -> Self {
        let mut ini = Self::empty();
        for section in sections {
            ini.replace_first_section(section);
        }
        ini
    }

    /// Parse arbitrary bytes by zero-extending each byte to a Unicode scalar.
    /// This mirrors gamemd's ordinary byte-to-wide helper; INI data is not
    /// interpreted as UTF-8, CP1252, or the Windows active code page.
    pub fn from_bytes(data: &[u8]) -> Result<Self, RulesError> {
        let text = crate::util::native_string::widen_bytes(data);
        Ok(Self::from_str(&text))
    }

    pub fn from_str(text: &str) -> Self {
        let mut ini = Self::empty();
        let mut current_section = None;

        for physical_line in text.split('\n') {
            // Straw::ReadLine removes every CR byte while consuming the line.
            let without_carriage_returns: String = physical_line
                .chars()
                .filter(|character| *character != '\r')
                .collect();
            if without_carriage_returns.is_empty() {
                continue;
            }

            // Straw::ReadLine stores at most 511 payload bytes and consumes the
            // remainder through LF. The discarded tail is never a second line.
            let chunk_end = without_carriage_returns
                .char_indices()
                .nth(READ_LINE_PAYLOAD)
                .map_or(without_carriage_returns.len(), |(index, _)| index);
            let buffered = &without_carriage_returns[..chunk_end];
            // NUL occupies a buffer byte, but subsequent C-string operations
            // make the rest of that physical line invisible to the loader.
            let visible = buffered.split_once('\0').map_or(buffered, |(head, _)| head);
            Self::parse_line(&mut ini, &mut current_section, visible);
        }

        // Retail provenance: INI lexical loading — `INIClass__LoadFromStraw` @ `0x00525A60`.
        // Active read mode destroys a candidate section unless at least one
        // accepted nonempty entry was linked into it.
        ini.discard_entryless_sections();
        ini
    }

    fn discard_entryless_sections(&mut self) {
        self.sections.retain(|section| section.entry_count() != 0);
        self.first_section.clear();
        for (index, section) in self.sections.iter().enumerate() {
            self.first_section
                .entry(section.name.clone())
                .or_insert(index);
        }
    }

    fn parse_line(ini: &mut Self, current_section: &mut Option<usize>, raw_line: &str) {
        let line = strtrim_ascii(raw_line);
        if line.is_empty() {
            return;
        }

        if line.starts_with('[')
            && let Some(end) = line.find(']')
        {
            let name = &line[1..end];
            let index = ini.sections.len();
            ini.sections.push(IniSection::new(name.to_string()));
            ini.first_section.entry(name.to_string()).or_insert(index);
            *current_section = Some(index);
            return;
        }

        // Semicolon truncation happens before the first-equals split. `#` has
        // no comment meaning in the active parser.
        let payload = strtrim_ascii(line.split_once(';').map_or(line, |(head, _)| head));
        let Some((key, value)) = payload.split_once('=') else {
            return;
        };
        let key = strtrim_ascii(key);
        let value = strtrim_ascii(value);
        if key.is_empty() || value.is_empty() {
            return;
        }
        if let Some(index) = *current_section {
            ini.sections[index].insert_initial(key, value);
        }
    }

    /// Exact raw INI lookup.
    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.first_section
            .get(name)
            .and_then(|index| self.sections.get(*index))
    }

    /// [`Self::section`] for reading: an absent section is
    /// [`IniSection::empty`], so every reader returns its default.
    pub fn section_or_empty(&self, name: &str) -> &IniSection {
        self.section(name).unwrap_or(IniSection::empty())
    }

    pub fn section_names(&self) -> Vec<&str> {
        self.sections
            .iter()
            .map(|section| section.name.as_str())
            .collect()
    }

    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Clone a fixture with an exact key omitted from every matching section
    /// occurrence, including its retained typed-reader history.
    #[cfg(test)]
    pub(crate) fn without_entry_for_test(&self, section: &str, key: &str) -> Self {
        let mut ini = self.clone();
        for body in &mut ini.sections {
            if body.name == section {
                body.entries.remove(key);
                body.key_order.retain(|stored| stored.as_str() != key);
                body.projected_values.remove(key);
            }
        }
        ini.discard_entryless_sections();
        ini
    }

    /// Load another INI into an already populated INI object. This models the
    /// native PutString path: later nonempty exact keys replace earlier ones.
    pub fn merge(&mut self, patch: &IniFile) {
        for (patch_index, patch_section) in patch.sections.iter().enumerate() {
            if patch.first_section.get(&patch_section.name) != Some(&patch_index) {
                continue;
            }
            self.overlay_section(patch_section);
        }
    }

    /// Build the typed-reader compatibility view for one ordered rules pass.
    ///
    /// Retail provenance: sequential typed defaults — `RulesClass__Process` @ `0x00668BF0`.
    pub(super) fn merge_rules_projection(&mut self, patch: &IniFile) {
        for (patch_index, patch_section) in patch.sections.iter().enumerate() {
            if patch.first_section.get(&patch_section.name) != Some(&patch_index) {
                continue;
            }
            if let Some(index) = self.first_section.get(&patch_section.name).copied() {
                self.sections[index].overlay_rules_pass(patch_section);
            } else {
                let mut section = IniSection::new(patch_section.name.clone());
                section.overlay_rules_pass(patch_section);
                let index = self.sections.len();
                self.first_section.insert(section.name.clone(), index);
                self.sections.push(section);
            }
        }
    }

    fn overlay_section(&mut self, patch_section: &IniSection) -> usize {
        if let Some(index) = self.first_section.get(&patch_section.name).copied() {
            self.sections[index].overlay(patch_section);
        } else {
            let index = self.sections.len();
            self.sections.push(patch_section.clone());
            self.first_section.insert(patch_section.name.clone(), index);
        }
        patch_section.entry_count()
    }

    /// Obtain the first compatibility section, creating it at the current tail.
    /// Native processing owns which values enter it; INI owns storage/order.
    pub(super) fn projection_section_mut(&mut self, name: &str) -> &mut IniSection {
        let index = if let Some(&index) = self.first_section.get(name) {
            index
        } else {
            let index = self.sections.len();
            self.sections.push(IniSection::new(name.to_string()));
            self.first_section.insert(name.to_string(), index);
            index
        };
        &mut self.sections[index]
    }

    pub(super) fn replace_first_section(&mut self, section: IniSection) {
        if let Some(index) = self.first_section.get(&section.name).copied() {
            self.sections[index] = section;
        } else {
            let index = self.sections.len();
            self.first_section.insert(section.name.clone(), index);
            self.sections.push(section);
        }
    }

    /// Deterministic hash over native section occurrence and entry order.
    pub fn content_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for section in &self.sections {
            section.name.hash(&mut hasher);
            for key in section.keys() {
                key.hash(&mut hasher);
                if let Some(value) = section.get(key) {
                    value.hash(&mut hasher);
                }
            }
        }
        hasher.finish()
    }
}

/// Whether a type factory answers null for this exact name.
///
/// `UnitTypeClass__FindOrAllocate @ 0x007480D0` (reached for
/// `UndeploysInto=` by `TechnoTypeClass__ReadINI @ 0x00712170` at
/// `0x0071329D..0x007132E4`) and its sibling factories `_stricmp` the name
/// against `<none>` (`0x00817474`) and `none` (`0x00817694`) before lookup
/// or allocation. The name is compared as given: a list token keeps its
/// spaces, so ` none` is an ordinary type name.
pub(crate) fn is_native_none_type_name(value: &str) -> bool {
    value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("<none>")
}

#[cfg(test)]
#[path = "ini_parser_tests.rs"]
mod tests;
