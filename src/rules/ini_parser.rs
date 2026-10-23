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
//! A fresh load retains duplicate nonempty section bodies, and later copies
//! of a key for `SaveToPipe`. Fresh entry reads use the native CRC index;
//! compatibility projections keep their explicit exact-key overlay policy.
//! Empty keys, values, and physical section bodies are not inserted.
//! Native registry allocation and ordered RulesClass passes live in
//! `rules::native_processing`; typed gameplay projection lives in `ruleset`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use crate::rules::error::RulesError;

#[path = "ini_value.rs"]
pub mod ini_value;

/// Opt-in file directives, expanded at source selection, never by raw parsing
/// or deserialization. No directive means the physical INI is left unchanged.
#[path = "ini_directives.rs"]
pub mod directives;

use self::ini_value::strtrim_ascii;

const READ_LINE_PAYLOAD: usize = 511;

/// One physical section occurrence in an INI file.
#[derive(Debug, Clone, serde::Serialize)]
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
    /// Later copies of a key in a fresh load, which native keeps in its
    /// entry list (`LoadFromStraw` `0x00525A60`) and `SaveToPipe` writes:
    /// `(physical index, key, value)`. Fresh CRC lookups may select them.
    /// Saved compatibility projections do not retain this raw-file history.
    #[serde(skip)]
    duplicate_entries: Vec<(usize, String, String)>,
    /// Built once after a fresh physical load. None denotes a compatibility
    /// projection, including saved typed-reader bodies; it is not PutString's
    /// mutable index/cache. The physical payload remains in the fields above.
    #[serde(skip)]
    fresh_index: Option<Vec<(i32, PhysicalEntry)>>,
}

#[derive(Debug, Clone, Copy)]
enum PhysicalEntry {
    First(usize),
    Duplicate(usize),
}

// Source-store equality compares the physical payload. Compatibility reader
// persistence does not retain raw-file index/cache history.
impl PartialEq for IniSection {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.entries == other.entries
            && self.key_order == other.key_order
            && self.projected_values == other.projected_values
            && self.duplicate_entries == other.duplicate_entries
    }
}
impl Eq for IniSection {}

// A standalone retained Type reader body uses this same admission as a whole
// IniFile. Accept neither duplicate order entries nor orphaned reader history.
impl<'de> serde::Deserialize<'de> for IniSection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct SavedSection {
            name: String,
            entries: HashMap<String, String>,
            key_order: Vec<String>,
            projected_values: HashMap<String, Vec<String>>,
        }
        let saved = <SavedSection as serde::Deserialize>::deserialize(deserializer)?;
        let ordered: std::collections::HashSet<_> = saved.key_order.iter().collect();
        if ordered.len() != saved.key_order.len()
            || ordered.len() != saved.entries.len()
            || ordered.iter().any(|key| !saved.entries.contains_key(*key))
            || saved
                .projected_values
                .keys()
                .any(|key| !saved.entries.contains_key(key))
        {
            return Err(serde::de::Error::custom("invalid saved INI key order"));
        }
        Ok(Self {
            name: saved.name,
            entries: saved.entries,
            key_order: saved.key_order,
            projected_values: saved.projected_values,
            duplicate_entries: Vec::new(),
            fresh_index: None,
        })
    }
}

impl IniSection {
    pub fn new(name: String) -> Self {
        Self {
            name,
            entries: HashMap::new(),
            key_order: Vec::new(),
            projected_values: HashMap::new(),
            duplicate_entries: Vec::new(),
            fresh_index: None,
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
        self.fresh_index = None;
        if !self.entries.contains_key(key) {
            self.key_order.push(key.to_string());
        }
        self.entries.insert(key.to_string(), value.to_string());
        self.projected_values
            .entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }

    /// Insert during a fresh file load, before building its lookup index.
    fn insert_initial(&mut self, key: &str, value: &str) {
        if self.entries.contains_key(key) {
            let index = self.key_order.len() + self.duplicate_entries.len();
            self.duplicate_entries
                .push((index, key.to_string(), value.to_string()));
            return;
        }
        self.key_order.push(key.to_string());
        self.entries.insert(key.to_string(), value.to_string());
    }

    /// Apply a later load/rules pass. Existing exact keys are replaced and new
    /// keys retain layer source order.
    pub fn set(&mut self, key: &str, value: &str) {
        self.fresh_index = None;
        if !self.entries.contains_key(key) {
            self.key_order.push(key.to_string());
        }
        self.entries.insert(key.to_string(), value.to_string());
        self.projected_values.remove(key);
    }

    /// [`Self::set`] each of a later layer's entries in its source order.
    pub(crate) fn overlay(&mut self, patch: &IniSection) {
        for key in patch.keys() {
            if let Some(value) = patch.entries.get(key) {
                self.set(key, value);
            }
        }
    }

    /// Edit the value the physical source actually selects, keeping unrelated
    /// duplicate-key choices intact instead of changing to dictionary lookup.
    fn overlay_source(&mut self, patch: &IniSection) {
        for key in patch.keys() {
            let Some(value) = patch.get(key) else {
                continue;
            };
            let Some(index) = self.fresh_index.as_ref() else {
                self.set(key, value);
                continue;
            };
            match find_crc_index_value(index, ini_name_crc(key)).copied() {
                Some(PhysicalEntry::First(at)) => {
                    *self.entries.get_mut(&self.key_order[at]).unwrap() = value.to_owned();
                }
                Some(PhysicalEntry::Duplicate(at)) => {
                    self.duplicate_entries[at].2 = value.to_owned();
                }
                None => {
                    let at = self.key_order.len();
                    self.insert_initial(key, value);
                    insert_source_index(
                        self.fresh_index.as_mut().unwrap(),
                        ini_name_crc(key),
                        PhysicalEntry::First(at),
                    );
                }
            }
        }
    }

    fn physical_slots(&self) -> impl Iterator<Item = PhysicalEntry> {
        let mut first = 0..self.key_order.len();
        let mut duplicates = self.duplicate_entries.iter().enumerate().peekable();
        (0..self.key_order.len() + self.duplicate_entries.len()).filter_map(move |index| {
            if duplicates.peek().is_some_and(|(_, (at, ..))| *at == index) {
                duplicates
                    .next()
                    .map(|(at, _)| PhysicalEntry::Duplicate(at))
            } else {
                first.next().map(PhysicalEntry::First)
            }
        })
    }

    fn physical_entry(&self, slot: PhysicalEntry) -> (&str, &str) {
        match slot {
            PhysicalEntry::First(at) => {
                let key = self.key_order[at].as_str();
                (key, self.entries[key].as_str())
            }
            PhysicalEntry::Duplicate(at) => {
                let (_, key, value) = &self.duplicate_entries[at];
                (key, value)
            }
        }
    }

    /// Every loaded entry in file order, without resolving lookup winners.
    fn physical_entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.physical_slots().map(|slot| self.physical_entry(slot))
    }

    /// GetEntryCount/GetEntryName's physical walk, including duplicate keys
    /// (LoadFromStraw525A60/526177). Value readers still query by name.
    pub fn physical_keys(&self) -> impl Iterator<Item = &str> {
        self.physical_entries().map(|(key, _)| key)
    }

    fn build_fresh_index(&mut self) {
        let mut index: Vec<_> = self
            .physical_slots()
            .map(|slot| (ini_name_crc(self.physical_entry(slot).0), slot))
            .collect();
        // FindEntryByNames526B10 sorts the physical pair array using signed
        // CRC only (CompareINIEntryNameHashes52B6A0), with no identity tie-break.
        crate::util::retail_pointer_sort::sort_by(&mut index, |a, b| a.0.cmp(&b.0));
        self.fresh_index = Some(index);
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
        if let Some(index) = &self.fresh_index {
            // FindINIEntryIndexPair52B6D0 returns the first equal midpoint it
            // visits. Fresh loads have no intervening mutable cache history.
            return find_crc_index_value(index, ini_name_crc(key))
                .map(|slot| self.physical_entry(*slot).1);
        }
        self.entries.get(key).map(String::as_str)
    }

    /// Raw text for a reader whose native default depends on a later asset
    /// binding step. The caller must pass the returned value through the
    /// matching `ini_value` reader rather than interpreting it directly.
    pub(crate) fn raw_value(&self, key: &str) -> Option<&str> {
        self.get(key)
    }

    /// Test builds only: the raw stored text, for tests that inspect storage.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_for_test(&self, key: &str) -> Option<&str> {
        self.get(key)
    }

    /// Whether the section's reader can find the key, whatever its value.
    pub fn is_present(&self, key: &str) -> bool {
        self.get(key).is_some()
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

/// Physical section occurrences and their native fresh-load CRC lookup order.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IniFile {
    sections: Vec<IniSection>,
    /// Exact first occurrences used by explicit compatibility projections.
    #[serde(skip)]
    first_section: HashMap<String, usize>,
    /// FindPair52B620 sorts signed hashes through52B720/CRT7C8B48. Removing
    /// a section preserves this permutation instead of sorting survivors.
    /// None denotes a compatibility projection, including saved reader views.
    #[serde(skip)]
    fresh_section_index: Option<Vec<(i32, usize)>>,
}

impl<'de> serde::Deserialize<'de> for IniFile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct SavedIni {
            sections: Vec<IniSection>,
        }
        let saved = <SavedIni as serde::Deserialize>::deserialize(deserializer)?;
        let mut first_section = HashMap::new();
        for (index, section) in saved.sections.iter().enumerate() {
            first_section.entry(section.name.clone()).or_insert(index);
        }
        Ok(Self {
            sections: saved.sections,
            first_section,
            fresh_section_index: None,
        })
    }
}

impl IniFile {
    pub fn empty() -> Self {
        Self {
            sections: Vec::new(),
            first_section: HashMap::new(),
            fresh_section_index: None,
        }
    }

    /// Supply recorded native cache sections without physical parsing. Some
    /// oracle controls contain stored-empty entries or sections, which the
    /// physical reader would discard. Storage and first-section indexing stay
    /// with this owner; production projection mutators remain private.
    #[cfg(any(test, feature = "test-support"))]
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
    ///
    /// `ReadINITextLine` (`0x0065D5C0`) flags the end of the file when it
    /// reaches it without an LF, and `LoadFromStraw` discards the line read
    /// on that call: a last line without a line feed is not loaded.
    /// Executed controls: tools/storage_oracle/ini_crc (`unterminated_last_*`).
    /// RESIDUAL: loading into an INI that already holds sections (the
    /// `0x00525BFA` arm, e.g. loose `*.PKT` files at `0x00699B58`) puts that
    /// last entry before testing the flag; VERA has no such merge load.
    pub fn from_bytes(data: &[u8]) -> Result<Self, RulesError> {
        let complete = data
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(&[][..], |end| &data[..=end]);
        let text = crate::util::native_string::widen_bytes(complete);
        Ok(Self::from_str(&text))
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "infallible constructor; FromStr would force a Result on every caller"
    )]
    pub fn from_str(text: &str) -> Self {
        let mut ini = Self::empty();
        let mut current_section = None;

        for physical_line in text.split('\n') {
            // Straw::ReadLine removes every CR byte while consuming the line.
            // Most lines end in the one CR of a CRLF pair and hold no other.
            let line = physical_line.strip_suffix('\r').unwrap_or(physical_line);
            let without_carriage_returns = if line.contains('\r') {
                std::borrow::Cow::Owned(line.replace('\r', ""))
            } else {
                std::borrow::Cow::Borrowed(line)
            };
            if without_carriage_returns.is_empty() {
                continue;
            }

            // Straw::ReadLine stores at most 511 payload bytes and consumes the
            // remainder through LF. The discarded tail is never a second line.
            // A character takes at least one byte, so shorter text is whole.
            let chunk_end = if without_carriage_returns.len() <= READ_LINE_PAYLOAD {
                without_carriage_returns.len()
            } else {
                without_carriage_returns
                    .char_indices()
                    .nth(READ_LINE_PAYLOAD)
                    .map_or(without_carriage_returns.len(), |(index, _)| index)
            };
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
        for section in &mut ini.sections {
            section.build_fresh_index();
        }
        let mut index: Vec<_> = ini
            .sections
            .iter()
            .enumerate()
            .map(|(ordinal, section)| (ini_name_crc(&section.name), ordinal))
            .collect();
        crate::util::retail_pointer_sort::sort_by(&mut index, |a, b| a.0.cmp(&b.0));
        ini.fresh_section_index = Some(index);
        ini
    }

    fn discard_entryless_sections(&mut self) {
        self.fresh_section_index = None;
        self.sections.retain(|section| section.entry_count() != 0);
        self.rebuild_first_sections();
    }

    fn rebuild_first_sections(&mut self) {
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

        // Semicolon truncation happens before the first-equals split, and
        // only a cut line is trimmed whole (`0x00525F3A`): otherwise the key
        // and value are trimmed alone, so `strtrim`'s trailing bound applies
        // to the value's own leading run. `#` has no comment meaning in the
        // active parser.
        let payload = raw_line
            .split_once(';')
            .map_or(raw_line, |(head, _)| strtrim_ascii(head));
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

    fn section_position(&self, name: &str) -> Option<usize> {
        if let Some(index) = &self.fresh_section_index {
            find_crc_index_value(index, ini_name_crc(name)).copied()
        } else {
            self.first_section.get(name).copied()
        }
    }

    /// Native fresh-file lookup, or the explicit compatibility projection.
    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.section_position(name)
            .and_then(|index| self.sections.get(index))
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

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Remove a reserved configuration section before composing gameplay data.
    pub(crate) fn remove_section(&mut self, name: &str) {
        while let Some(position) = self
            .sections
            .iter()
            .position(|section| section.name == name)
        {
            self.remove_section_at(position);
        }
    }

    /// ReadCCFile4741F0: a positive ReadUUBlock526FB0("Digest",20) removes
    /// the selected section before scenario initialization, even on mismatch.
    /// Base64Pipe42DDB0/42DF40 emits at least one byte for every nonempty raw
    /// group (Decode42FE50,42FF14); only that predicate is needed here. Short
    /// or invalid text must not be rejected through the strict map decoder.
    pub(crate) fn remove_digest_for_checked_load(&mut self) {
        let Some(position) = self.section_position("Digest") else {
            return;
        };
        if self.sections[position].read_packed_text().is_empty() {
            return;
        }
        // Clear5257C0 -> RemoveKey52B2B0 compacts the sorted pair array and
        // clears its cached pair, without resetting the sorted flag. Retain
        // the other physical sections and their lookup order unchanged.
        self.remove_section_at(position);
    }

    fn remove_section_at(&mut self, position: usize) {
        self.sections.remove(position);
        self.rebuild_first_sections();
        if let Some(index) = &mut self.fresh_section_index {
            index.retain_mut(|(_, ordinal)| {
                if *ordinal == position {
                    return false;
                }
                if *ordinal > position {
                    *ordinal -= 1;
                }
                true
            });
        }
    }

    #[cfg(test)]
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Clone a fixture with an exact key omitted from every matching section
    /// occurrence, including its retained typed-reader history.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn without_entry_for_test(&self, section: &str, key: &str) -> Self {
        let mut ini = self.clone();
        for body in &mut ini.sections {
            if body.name == section {
                body.fresh_index = None;
                body.entries.remove(key);
                body.key_order.retain(|stored| stored.as_str() != key);
                body.projected_values.remove(key);
            }
        }
        ini.discard_entryless_sections();
        ini
    }

    /// Compatibility overlay: later exact keys replace earlier ones. This is
    /// also the include extension's policy, not native PutString528660 parity.
    /// Residual: overlapping loose PKT files (699B58) can retain a cached
    /// duplicate pair across insertion, then change selection on a later sort.
    /// That separate map-catalog merge needs native index/capacity/cache history
    /// rather than this dictionary overlay. It has no own RNG/timer/detach.
    pub fn merge(&mut self, patch: &IniFile) {
        self.fresh_section_index = None;
        for (patch_index, patch_section) in patch.sections.iter().enumerate() {
            if patch.first_section.get(&patch_section.name) != Some(&patch_index) {
                continue;
            }
            self.overlay_section(patch_section);
        }
    }

    /// VERA source edits preserve native lookup winners. Later passes set
    /// `add_sections` false so an absent section still skips its native reader.
    pub(crate) fn merge_source_sections(&mut self, patch: &IniFile, add_sections: bool) {
        for (patch_index, section) in patch.sections.iter().enumerate() {
            if patch.first_section.get(&section.name) != Some(&patch_index) {
                continue;
            }
            let selected = patch.section(&section.name).unwrap();
            if let Some(index) = self.section_position(&section.name) {
                self.sections[index].overlay_source(selected);
            } else if add_sections {
                let at = self.sections.len();
                self.sections.push(selected.clone());
                self.first_section.insert(selected.name.clone(), at);
                if let Some(index) = self.fresh_section_index.as_mut() {
                    insert_source_index(index, ini_name_crc(&selected.name), at);
                }
            }
        }
    }

    /// Build the typed-reader compatibility view for one ordered rules pass.
    ///
    /// Retail provenance: sequential typed defaults — `RulesClass__Process` @ `0x00668BF0`.
    pub(super) fn merge_rules_projection(&mut self, patch: &IniFile) {
        self.fresh_section_index = None;
        for (patch_index, physical) in patch.sections.iter().enumerate() {
            if patch.first_section.get(&physical.name) != Some(&patch_index) {
                continue;
            }
            // The typed pass must consume the same section as its native
            // readers, even when the CRC index selects a later occurrence.
            let patch_section = patch.section(&physical.name).unwrap();
            if let Some(index) = self.first_section.get(&physical.name).copied() {
                self.sections[index].overlay_rules_pass(patch_section);
            } else {
                let mut section = IniSection::new(physical.name.clone());
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
            let mut copied = patch_section.clone();
            copied.fresh_index = None;
            self.sections.push(copied);
            self.first_section.insert(patch_section.name.clone(), index);
        }
        patch_section.entry_count()
    }

    /// Obtain the first compatibility section, creating it at the current tail.
    /// Native processing owns which values enter it; INI owns storage/order.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn projection_section_mut(&mut self, name: &str) -> &mut IniSection {
        self.fresh_section_index = None;
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
        self.fresh_section_index = None;
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

    /// `INIClass::SaveToPipe` (`0x00526470`) as every GetCRC input reads it:
    /// with comments off, `LoadFromStraw` leaves no column pads or inline
    /// comments, so each section is `[name]\r\n` and one `key=value\r\n`
    /// per entry, in list order, with `\r\n` before every section but the
    /// first. `skip` names a section the load removed first.
    fn save_to_pipe(&self, skip: Option<&str>) -> Vec<u8> {
        let skipped = skip.and_then(|name| self.section_position(name));
        let mut bytes = Vec::new();
        for (index, section) in self.sections.iter().enumerate() {
            if Some(index) == skipped {
                continue;
            }
            if !bytes.is_empty() {
                bytes.extend_from_slice(b"\r\n");
            }
            bytes.push(b'[');
            bytes.extend(crate::util::native_string::narrow_widened(&section.name));
            bytes.extend_from_slice(b"]\r\n");
            for (key, value) in section.physical_entries() {
                bytes.extend(crate::util::native_string::narrow_widened(key));
                bytes.push(b'=');
                bytes.extend(crate::util::native_string::narrow_widened(value));
                bytes.extend_from_slice(b"\r\n");
            }
        }
        bytes
    }

    /// `CCINIClass::GetCRC` (`0x00476D80`): `CRCEngine` (`0x004A1DE0`) over
    /// the SHA-1 digest (`0x0069E9E0`) of the INI's `SaveToPipe` bytes.
    pub fn native_crc(&self) -> u32 {
        crc_of_pipe(&self.save_to_pipe(None))
    }

    /// GetCRC after a digest-checked load (`ReadCCFile(file, 1, 0)`,
    /// `0x0047424A..0x004742E3`): `[Digest]` is removed before the hash.
    pub fn digest_checked_crc(&self) -> u32 {
        let mut checked = self.clone();
        checked.remove_digest_for_checked_load();
        checked.native_crc()
    }
}

/// CRCEngine4A1DE0 over the native name bytes, without case folding.
fn ini_name_crc(name: &str) -> i32 {
    let bytes = crate::util::native_string::narrow_widened(name);
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    crate::assets::mix_hash::crc_engine(&bytes[..end]) as i32
}

/// SectionIndex::FindPair52B620 and FindINIEntryIndexPair52B6D0 use the
/// same signed midpoint search, returning the first equal pair visited.
fn find_crc_index_value<T>(pairs: &[(i32, T)], crc: i32) -> Option<&T> {
    let mut remaining = pairs;
    while !remaining.is_empty() {
        let middle = remaining.len() / 2;
        match remaining[middle].0.cmp(&crc) {
            std::cmp::Ordering::Equal => return Some(&remaining[middle].1),
            std::cmp::Ordering::Greater => remaining = &remaining[..middle],
            std::cmp::Ordering::Less => remaining = &remaining[middle + 1..],
        }
    }
    None
}

/// Growing a sorted index changes the midpoint used for equal CRCs. Preserve
/// each existing group's selected physical entry before adding a new name;
/// the physical duplicate payload remains untouched for registry walks/CRC.
fn insert_source_index<T: Copy>(pairs: &mut Vec<(i32, T)>, crc: i32, value: T) {
    let mut selected = Vec::with_capacity(pairs.len() + 1);
    for &(existing, _) in pairs.iter() {
        if selected.last().is_none_or(|&(last, _)| last != existing) {
            selected.push((existing, *find_crc_index_value(pairs, existing).unwrap()));
        }
    }
    let at = selected.partition_point(|&(existing, _)| existing < crc);
    selected.insert(at, (crc, value));
    *pairs = selected;
}

fn crc_of_pipe(bytes: &[u8]) -> u32 {
    crate::assets::mix_hash::crc_engine(&crate::util::sha1::sha1_bytes(bytes))
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
