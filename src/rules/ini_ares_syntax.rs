//! Ares 3.0's syntax inside `INIClass::LoadFromStraw` (`0x00525A60`).
//!
//! An Ares launch hooks the text reader every INI load goes through. It has
//! two arms, chosen once per load by whether the INI already holds a section
//! (`0x00525A8E`):
//! - the fresh arm builds each section object and links it once the section
//!   ends (`0x005261F7`): [`load_fresh`], which is `IniFile::from_str` in an
//!   Ares build;
//! - the merge arm (`0x00525BFA`) writes every entry through
//!   `INIClass::PutString` (`0x00528660`). `ReadCCFile` takes it for each
//!   Ares `[#include]` or Phobos `$Include` file, which loads into the INI that
//!   lists it: [`load_into`]. It also hands back the file's own entries of its
//!   include section, the list VERA's traversal follows next.
//!
//! Ares.dll hooks in both arms (`Misc/IniIteratorChar.cpp`,
//! `Misc/IniSectionIncludes.cpp`):
//! - A key `+` becomes `var_<n>` from one counter both arms share:
//!   `INIClass_Parse_IteratorChar1` (`10065280`, fresh arm `0x005260A2`) and
//!   `_IteratorChar2` (`100652F0`, merge arm `0x00525D23`). Only `Ares::Clear`
//!   (`10004D90`, reached from `Scenario_ClearClasses` `0x00685659`) resets it.
//! - `[Child]:[Base]`: `_IniSectionIncludes_PreProcess1` (`100655B0`, merge arm
//!   `0x00525CA5`) and `_PreProcess2` (`100655E0`, fresh arm `0x00525DDB`) read
//!   the base name after the header's `]` and look it up in the INI being
//!   loaded (`IniSectionIncludes::PreProcess` `10065470`). `_CopySection1`
//!   (`10065550`, `0x00525C28`) and `_CopySection2` (`10065580`, `0x00525E44`)
//!   then `WriteString` every base entry, in list order, under the new
//!   section's name (`IniSectionIncludes::CopySection` `10065380`). A base that
//!   is missing or defined later is logged and copies nothing. A base in
//!   another file is found when that file loaded into the same INI first, as an
//!   including file has for the files it includes; rules, mode and map layers
//!   are separate INIs, so a map cannot inherit a rules section.
//! - `INIClass_Parse_Override` (`10065610`, fresh arm `0x005260D9`): a key
//!   whose CRC an earlier entry of the section has deletes that entry, so the
//!   later one wins and is listed last. The merge arm's PutString does so
//!   already.
//!
//! The fresh arm copies before the new section object is linked, so PutString
//! finds an earlier section of that name or appends one of its own: a section
//! that inherits and has entries of its own becomes two sections of one name,
//! and readers see whichever the CRC index answers (Ares bug 895552: in the
//! main rules file the child loses either its own keys or the base's). In an
//! included file the copy and the child's entries share one section.
//!
//! RESIDUALS:
//! - VERA numbers `+` keys from 0 per INI and continues through its included
//!   files; native numbering continues across every INI loaded since the
//!   last scenario clear. Keys stay unique within an INI, which is all a
//!   registry walk needs. A mod key literally named `var_<n>`, and GetCRC over
//!   the renamed keys (network hashes against a native peer), can differ.
//! - The section index's cached pair is followed within one load only. It
//!   decides between same-named sections when a later read asks for the last
//!   name a load looked up; VERA's readers search the sorted index.
//! - A header on the merge arm's unterminated last line leaves its base set
//!   for the next load's first header (`0x00525D4D` ends the load before
//!   `0x00525C28` copies); VERA drops it.
//! None of these draws RNG, writes a timer or detaches an object.

use std::borrow::Cow;
use std::collections::HashMap;

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
fn pre_process(index: &mut LiveSectionIndex, tail: &str) -> Option<usize> {
    let rest = tail.trim_start_matches(is_blank).strip_prefix(':')?;
    let rest = rest.trim_start_matches(is_blank).strip_prefix('[')?;
    let rest = rest.trim_start_matches(is_blank);
    let end = rest.find([']', ';'])?;
    let name = rest[..end].trim_end_matches(is_blank);
    if !rest[end..].starts_with(']') || name.is_empty() {
        return None;
    }
    let base = index.find(ini_name_crc(name));
    if base.is_none() {
        log::warn!(
            "An INI section inherits from section '{name}', which doesn't exist or has not been parsed yet."
        );
    }
    base
}

/// `IniSectionIncludes::CopySection` (Ares.dll `10065380`): PutString each
/// entry of `base`, in list order, under `name`.
fn copy_section(
    ini: &mut IniFile,
    index: &mut LiveSectionIndex,
    written: &mut WrittenSections,
    base: usize,
    name: &str,
) {
    let entries: Vec<(String, String)> = ini.sections[base]
        .physical_entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    for (key, value) in &entries {
        put_string(ini, index, written, name, key, value);
    }
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
pub(super) fn load_fresh(text: &str) -> IniFile {
    let mut ini = IniFile::empty();
    let mut index = LiveSectionIndex::new();
    let mut written = WrittenSections::default();
    // The section being read, linked only once it ends.
    let mut current: Option<(IniSection, HashMap<i32, String>)> = None;
    for line in read_lines(text) {
        match lex_line(&line) {
            Some(NativeLine::Header { name, tail }) => {
                link(&mut ini, &mut index, current.take());
                // `_PreProcess2`, then `_CopySection2` once the new section
                // object exists, still unlinked.
                if let Some(base) = pre_process(&mut index, tail) {
                    copy_section(&mut ini, &mut index, &mut written, base, name);
                }
                current = Some((IniSection::new(name.to_owned()), HashMap::new()));
            }
            Some(NativeLine::Entry { key, value }) => {
                if let Some((section, by_crc)) = &mut current {
                    // `_IteratorChar1` renames the new entry's key, then the
                    // override deletes the entry it repeats.
                    let key = iterator_key(key, &mut ini.next_iterator_key);
                    replace_entry(section, by_crc, &key, value);
                }
            }
            None => {}
        }
    }
    link(&mut ini, &mut index, current);
    index.store(&mut ini);
    for section in &mut ini.sections {
        section.build_fresh_index();
    }
    ini.rebuild_first_sections();
    ini
}

/// The fresh arm's section end (`0x005261F7..0x0052636A`): a section with an
/// entry is linked last and indexed; an empty one is destroyed.
fn link(
    ini: &mut IniFile,
    index: &mut LiveSectionIndex,
    section: Option<(IniSection, HashMap<i32, String>)>,
) {
    let Some((section, _)) = section else {
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
    let mut index = LiveSectionIndex::resume(ini);
    let mut written = WrittenSections::default();
    // The lines before the first header are skipped, and that header is read
    // before the loop the hooks sit in (`0x00525BFA`), so it never inherits.
    let mut section: Option<String> = None;
    let mut first_header = true;
    let mut listed = (IniSection::new(list.to_owned()), HashMap::new());
    'load: for (part, bytes) in lines.into_iter().enumerate() {
        let text = crate::util::native_string::widen_bytes(bytes);
        for line in read_lines(&text) {
            match lex_line(&line) {
                Some(NativeLine::Header { name, tail }) => {
                    let base = if first_header {
                        None
                    } else {
                        // `_PreProcess1`.
                        pre_process(&mut index, tail)
                    };
                    first_header = false;
                    // The end flag is tested before the header is used.
                    if part == 1 {
                        break 'load;
                    }
                    // `_CopySection1`, before the name is copied for PutString.
                    if let Some(base) = base {
                        copy_section(ini, &mut index, &mut written, base, name);
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

#[cfg(test)]
#[path = "ini_ares_syntax_tests.rs"]
mod tests;
