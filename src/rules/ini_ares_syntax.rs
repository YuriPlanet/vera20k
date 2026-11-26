//! INI auto-numbering and section inheritance inside `LoadFromStraw` (`0x00525A60`).
//!
//! P1 renames `+` with one signed counter (`0x10066200`/`0x10066270`),
//! cleared by `Scenario_ClearClasses` at `0x10004FF7`. VERA starts a counter
//! for each ordered source selection and passes it through distinct roots and
//! includes. Standalone probes and map reads have independent sequences.
//!
//! P1 `PreProcess` (`0x100663F0`) looks up a parent already in the same INI.
//! Its fresh copy (`0x10066500`) uses `PutString` before the child is linked;
//! its merge copy (`0x100664D0`) skips the first header's own parent suffix.
//! VERA writes fresh defaults into the child's single physical section and
//! recognizes a parent suffix on every committed header, including the first
//! in an include. The loaded-text budget bounds allocations for these copies.
//! Separate files that are not included still have separate section stores.
//!
//! Ares mode keeps its later-wins repeated-key policy (`0x10066590`). INIs
//! read with Ares Off keep their first authored value. Both modes let the
//! child replace an inherited default. See `docs/ares-ini-syntax.md` for the
//! native load boundaries. There are no RNG draws, timers or pointer detaches.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable
    )
)]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use super::ini_value::truncate_native_bytes;
use super::{
    IniFile, IniSection, NativeLine, find_crc_index_slot, ini_name_crc, lex_line, read_lines,
};

/// The section index (`INIClass` `+0x28..+0x38`) as one load changes it: its
/// pairs in their in-place order, the sorted flag, the cached pair a lookup
/// answers from first, and the capacity whose growth drops that cache.
struct LiveSectionIndex {
    pairs: Vec<(i32, usize)>,
    sorted: bool,
    cached: Option<usize>,
    capacity: usize,
}

impl LiveSectionIndex {
    fn new() -> Self {
        Self {
            pairs: Vec::new(),
            sorted: false,
            cached: None,
            capacity: 0,
        }
    }

    /// The index a finished load left, sorted as its next lookup sorted it.
    fn resume(ini: &mut IniFile) -> Self {
        let pairs = ini.fresh_section_index.take().unwrap_or_else(|| {
            let mut pairs: Vec<_> = ini
                .sections
                .iter()
                .enumerate()
                .map(|(ordinal, section)| (ini_name_crc(&section.name), ordinal))
                .collect();
            crate::util::retail_pointer_sort::sort_by(&mut pairs, |a, b| a.0.cmp(&b.0));
            pairs
        });
        // Every load appends; each growth adds the capacity, 10 the first time.
        let mut capacity = 0;
        while capacity < pairs.len() {
            capacity += capacity.max(10);
        }
        Self {
            pairs,
            sorted: true,
            cached: None,
            capacity,
        }
    }

    /// `INIClass::GetSection` (`0x00526810`), and PutString's lookup: the
    /// cached pair answers a matching CRC before an unsorted index is sorted
    /// in place (CRT qsort `0x007C8B48`, dropping the cache) and searched.
    fn find(&mut self, crc: i32) -> Option<usize> {
        if self.pairs.is_empty() {
            return None;
        }
        if let Some(slot) = self.cached
            && self.pairs[slot].0 == crc
        {
            return Some(self.pairs[slot].1);
        }
        if !self.sorted {
            crate::util::retail_pointer_sort::sort_by(&mut self.pairs, |a, b| a.0.cmp(&b.0));
            self.cached = None;
            self.sorted = true;
        }
        let slot = find_crc_index_slot(&self.pairs, crc)?;
        self.cached = Some(slot);
        Some(self.pairs[slot].1)
    }

    /// The fresh arm's section link (`0x0052630B`) and PutString's new
    /// section (`0x00528744`) append a pair; growing drops the cached pair.
    fn append(&mut self, crc: i32, ordinal: usize) {
        if self.pairs.len() == self.capacity {
            self.capacity += self.capacity.max(10);
            self.cached = None;
        }
        self.pairs.push((crc, ordinal));
        self.sorted = false;
    }

    /// Store the index for readers, sorted as the next lookup sorts it.
    fn store(mut self, ini: &mut IniFile) {
        if !self.sorted {
            crate::util::retail_pointer_sort::sort_by(&mut self.pairs, |a, b| a.0.cmp(&b.0));
        }
        ini.fresh_section_index = Some(self.pairs);
    }
}

/// Each written section's key by entry CRC, the lookup both PutString and the
/// override make in the section's entry index. Built at a section's first
/// write; its keys are the sections a load changed.
#[derive(Default)]
struct WrittenSections(HashMap<usize, HashMap<i32, String>>);

/// Delete the entry whose key has `key`'s CRC (its index pair and its list
/// node), then append `key=value` last.
fn replace_entry(
    section: &mut IniSection,
    by_crc: &mut HashMap<i32, String>,
    key: &str,
    value: &str,
) {
    if let Some(old) = by_crc.insert(ini_name_crc(key), key.to_owned()) {
        section.entries.remove(&old);
        section.key_order.retain(|stored| *stored != old);
    }
    section.key_order.push(key.to_owned());
    section.entries.insert(key.to_owned(), value.to_owned());
}

/// `INIClass::PutString` (`0x00528660`) with a nonempty value: the section
/// the index answers for `section`, or a new one linked last; then
/// [`replace_entry`].
fn put_string(
    ini: &mut IniFile,
    index: &mut LiveSectionIndex,
    written: &mut WrittenSections,
    section: &str,
    key: &str,
    value: &str,
) {
    let crc = ini_name_crc(section);
    let at = match index.find(crc) {
        Some(at) => at,
        None => {
            let at = ini.sections.len();
            ini.sections.push(IniSection::new(section.to_owned()));
            index.append(crc, at);
            at
        }
    };
    let body = &mut ini.sections[at];
    let by_crc = written.0.entry(at).or_insert_with(|| {
        body.key_order
            .iter()
            .map(|key| (ini_name_crc(key), key.clone()))
            .collect()
    });
    replace_entry(body, by_crc, key, value);
}

/// `PreProcess`'s blanks: signed chars below `!`, so bytes from 0x80 too.
fn is_blank(c: char) -> bool {
    matches!(u32::from(c), 0x01..=0x20 | 0x80..=0xFF)
}

/// `IniSectionIncludes::PreProcess` (Ares.dll `10065470`) over the text after
/// a header's `]`: blanks, `:`, blanks, `[`, blanks, the base name, blanks,
/// `]`, where a `;` first ends the header. The INI being loaded answers.
fn inheritance_base(tail: &str) -> Option<&str> {
    let rest = tail.trim_start_matches(is_blank).strip_prefix(':')?;
    let rest = rest.trim_start_matches(is_blank).strip_prefix('[')?;
    let rest = rest.trim_start_matches(is_blank);
    let end = rest.find([']', ';'])?;
    let name = rest[..end].trim_end_matches(is_blank);
    if !rest[end..].starts_with(']') || name.is_empty() {
        return None;
    }
    Some(name)
}

/// VERA's budget on the text `[Child]:[Base]` copies add to one INI, across
/// its fresh load and every file merged into it: the share of the loaded text
/// that `$Inherits` allows (`directives::copied_text_budget`). Ares has none.
#[derive(Debug, Clone, Default)]
pub(super) struct CopyBudget {
    loaded: usize,
    copied: usize,
}

impl CopyBudget {
    fn load(&mut self, bytes: usize) {
        self.loaded = self.loaded.saturating_add(bytes);
    }

    /// Whether a copy of `bytes` fits, charging it if so.
    fn charge(&mut self, bytes: usize) -> bool {
        let copied = self.copied.saturating_add(bytes);
        if copied > super::directives::copied_text_budget(self.loaded) {
            return false;
        }
        self.copied = copied;
        true
    }
}

/// `IniSectionIncludes::CopySection` (Ares.dll `10065380`): PutString each
/// entry of `base`, in list order, under `name`, once [`CopyBudget`] admits
/// the copy.
fn inherited_entries(
    ini: &mut IniFile,
    index: &mut LiveSectionIndex,
    including: Option<&IniFile>,
    name: &str,
    tail: &str,
) -> Vec<(String, String)> {
    let Some(base) = inheritance_base(tail) else {
        return Vec::new();
    };
    // A stock include is parsed separately before its ordinary overlay. Look
    // up defaults from the accumulated root, then from earlier in this file.
    let outer = including.and_then(|document| document.section(base));
    let earlier = index.find(ini_name_crc(base)).map(|at| &ini.sections[at]);
    if outer.is_none() && earlier.is_none() {
        log::warn!(
            "An INI section inherits from section '{base}', which doesn't exist or has not been parsed yet."
        );
        return Vec::new();
    }
    let entries = || {
        outer
            .into_iter()
            .chain(earlier)
            .flat_map(IniSection::physical_entries)
    };
    let bytes = entries().map(|(key, value)| key.len() + value.len()).sum();
    if !ini.ares_copies.charge(bytes) {
        // Log only: every INI the game reads comes through this parser,
        // which cannot fail and has no channel to the player. Telling the
        // player takes a notice carried from the rules load to the loading
        // screen, which is the rules stack's to add (`retail_sources`).
        log::warn!(
            "Section '{name}' does not inherit from '{base}': the INI's inherited sections exceed VERA's copy limit."
        );
        return Vec::new();
    }
    entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// Both iterator hooks: a key that is exactly `+` becomes `var_%d` of the
/// INI's counter.
fn iterator_key<'a>(key: &'a str, counter: &mut i32) -> Cow<'a, str> {
    if key != "+" {
        return Cow::Borrowed(key);
    }
    let key = format!("var_{counter}");
    *counter = counter.wrapping_add(1);
    Cow::Owned(key)
}

/// `LoadFromStraw`'s fresh arm with Ares's hooks.
pub(super) fn load_fresh(
    text: &str,
    ares_way: bool,
    first_iterator_key: i32,
    including: Option<&IniFile>,
) -> IniFile {
    let mut ini = IniFile::empty();
    ini.next_iterator_key = first_iterator_key;
    if let Some(including) = including {
        ini.ares_copies = including.ares_copies.clone();
    }
    ini.ares_copies.load(text.len());
    let mut index = LiveSectionIndex::new();
    let mut current: Option<FreshSection> = None;
    for line in read_lines(text) {
        match lex_line(&line) {
            Some(NativeLine::Header { name, tail }) => {
                link(
                    &mut ini,
                    &mut index,
                    current.take().map(|fresh| fresh.section),
                );
                let inherited = inherited_entries(&mut ini, &mut index, including, name, tail);
                current = Some(FreshSection::new(name, inherited, ares_way));
            }
            Some(NativeLine::Entry { key, value }) => {
                if let Some(section) = &mut current {
                    let key = iterator_key(key, &mut ini.next_iterator_key);
                    section.write(&key, value, ares_way);
                }
            }
            None => {}
        }
    }
    link(&mut ini, &mut index, current.map(|fresh| fresh.section));
    index.store(&mut ini);
    for section in &mut ini.sections {
        section.build_fresh_index();
    }
    ini.rebuild_first_sections();
    ini
}

struct FreshSection {
    section: IniSection,
    by_crc: HashMap<i32, String>,
    inherited_keys: HashSet<String>,
}

impl FreshSection {
    fn new(name: &str, inherited: Vec<(String, String)>, ares_way: bool) -> Self {
        let mut fresh = Self {
            section: IniSection::new(name.to_owned()),
            by_crc: HashMap::new(),
            inherited_keys: HashSet::new(),
        };
        // P1 0x525E44 -> 0x10066500 copies before the child is linked.
        // Keep the copy on that child so an authored override and a base
        // value cannot end up in separate, indistinguishable CRC entries.
        for (key, value) in inherited {
            if ares_way {
                replace_entry(&mut fresh.section, &mut fresh.by_crc, &key, &value);
            } else {
                fresh.section.set(&key, &value);
                fresh.inherited_keys.insert(key);
            }
        }
        fresh
    }

    fn write(&mut self, key: &str, value: &str, ares_way: bool) {
        if ares_way {
            replace_entry(&mut self.section, &mut self.by_crc, key, value);
        } else if self.inherited_keys.remove(key) {
            self.section.set(key, value);
        } else {
            self.section.insert_initial(key, value);
        }
    }
}

/// The fresh arm's section end (`0x005261F7..0x0052636A`): a section with an
/// entry is linked last and indexed; an empty one is destroyed.
fn link(ini: &mut IniFile, index: &mut LiveSectionIndex, section: Option<IniSection>) {
    let Some(section) = section else {
        return;
    };
    if section.entry_count() == 0 {
        return;
    }
    let at = ini.sections.len();
    index.append(ini_name_crc(&section.name), at);
    ini.sections.push(section);
}

/// `LoadFromStraw`'s merge arm with Ares's hooks, into `ini`. Returns the
/// entries this file put under `list`, a later key replacing an earlier one.
pub(super) fn load_into(ini: &mut IniFile, data: &[u8], list: &str) -> IniSection {
    // This arm puts the entry an unterminated last line holds before it
    // tests the end flag (`0x00525D33`, `0x00525D40`).
    let complete = data
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |end| end + 1);
    let lines = [&data[..complete], &data[complete..]];
    ini.ares_copies.load(data.len());
    let mut index = LiveSectionIndex::resume(ini);
    let mut written = WrittenSections::default();
    // Ignore lines before the first header. Unlike the P1 merge loop at
    // 0x00525BFA, VERA processes the first header's own parent suffix.
    let mut section: Option<String> = None;
    let mut listed = (IniSection::new(list.to_owned()), HashMap::new());
    'load: for (part, bytes) in lines.into_iter().enumerate() {
        let text = crate::util::native_string::widen_bytes(bytes);
        for line in read_lines(&text) {
            match lex_line(&line) {
                Some(NativeLine::Header { name, tail }) => {
                    // The end flag is tested before the header is used.
                    if part == 1 {
                        break 'load;
                    }
                    // P1 0x525C28 -> 0x100664D0 copies before entries are put.
                    // Also recognize the first header in VERA's include loader.
                    for (key, value) in inherited_entries(ini, &mut index, None, name, tail) {
                        put_string(ini, &mut index, &mut written, name, &key, &value);
                        if name == list {
                            replace_entry(&mut listed.0, &mut listed.1, &key, &value);
                        }
                    }
                    section = Some(name.to_owned());
                }
                Some(NativeLine::Entry { key, value }) => {
                    let Some(name) = &section else {
                        continue;
                    };
                    // `_IteratorChar2` writes `var_%d` over the line buffer and
                    // copies the value behind it into the 0x1FF bytes left.
                    let renamed = iterator_key(key, &mut ini.next_iterator_key);
                    let value = match &renamed {
                        Cow::Owned(renamed) => truncate_native_bytes(value, 0x1FE - renamed.len()),
                        Cow::Borrowed(_) => value,
                    };
                    put_string(ini, &mut index, &mut written, name, &renamed, value);
                    if name.as_str() == list {
                        replace_entry(&mut listed.0, &mut listed.1, &renamed, value);
                    }
                }
                None => {}
            }
        }
    }
    index.store(ini);
    for at in written.0.keys() {
        ini.sections[*at].build_fresh_index();
    }
    ini.rebuild_first_sections();
    listed.0
}

#[cfg(all(test, tests_rules_a_n))]
#[path = "ini_ares_syntax_tests.rs"]
mod tests;
