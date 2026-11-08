//! Include and inheritance traversal over the native INI store.
//!
//! Native references:
//! - Ares.dll exports `CCINIClass_ReadCCFile1` and `CCINIClass_ReadCCFile2`,
//!   hooked at gamemd `0x00474200` and `0x00474314` (the start and return tail
//!   of `CCINIClass::ReadCCFile` `0x004741F0`). The first records the name of
//!   every file read during the root load. The tail reads the `[#include]` keys
//!   through `INIClass::GetKeyCount` `0x00526960`, `INIClass::GetKeyName`
//!   `0x00526CC0` and `INIClass::ReadString` `0x00528A10` in stored entry order
//!   and calls `ReadCCFile` into the same INI for each existing file whose name
//!   is not yet recorded (case-insensitive `_stricmp` `0x007CDA90`). A missing
//!   file is skipped. The record clears only when the outermost read returns,
//!   so each file is read once per root load.
//! - Phobos `src/Misc/Hooks.INIInheritance.cpp` hooks `CCINIClass_Load_Inheritance`
//!   at `0x00474230`: after the current file loads, every `$Include` entry is
//!   merged with `ReadCCFile(file, false, false)`. Each physical file name is
//!   included once per root load (`SavedIncludes`), and a missing file calls
//!   `Debug::FatalErrorAndExit`. `src/Phobos.cpp` restores the vanilla bytes at
//!   `0x474200`/`0x474314` for `-Include`, so a launch that uses `$Include`
//!   leaves `[#include]` inert.
//!
//! VERA reads the current document before its included files and recurses
//! depth-first in stored list order; a later read overwrites earlier values.
//! An Ares build loads each file into the root INI as `ReadCCFile` does,
//! through `LoadFromStraw`'s merge arm and Ares's parser hooks
//! (`IniFile::merge_load`); other builds compose with `IniFile::merge`.
//!
//! RESIDUAL: natively every included file merges into the same INI, so its
//! list joins one shared include section. Ares walks that section with one
//! index shared by every nesting level (Ares.dll `CCINIClass_ReadCCFile2`
//! `0x10064660`), and Phobos re-walks it skipping saved names, so a parent's
//! remaining entries are read before a child's own list, and a child entry
//! that reuses a parent's key name resolves through the shared section.
//! Phobos also iterates the section's `EntryIndex`, gamemd's CRC lookup index,
//! where VERA keeps stored order. VERA reads each file's own list instead.
//! Trigger: nested include lists; effect: which sibling or descendant value
//! wins; frequency: mods that nest includes; no RNG, timer or detach.
//!
//! `$Inherits` expansion runs after includes; its policy here is the VERA
//! adaptation from baixiong443's ra2-mono `IniFile`, not a native directive
//! claim. The raw byte parser and deserializer never load other files:
//! expansion is an opted-in source-selection step, selected by compile
//! features for gameplay and available to read-only probes for both dialects.
//! Copies are charged against one aggregate expanded-text budget per root load
//! before they are made, so a small document cannot grow its retained store by
//! inheriting one parent into many children.

use super::{IniFile, IniSection};
use std::collections::HashSet;

const ARES: &str = "#include";
const PHOBOS: &str = "$Include";
const INHERITS: &str = "$Inherits";
/// Ares `CCINIClass_ReadCCFile2` reads each include name through `ReadString`
/// `0x00528A10` into a `0x80`-byte buffer, so longer names keep 127 bytes.
const ARES_INCLUDE_NAME_CAPACITY: usize = 0x80;
// Extension safety policy: native has no depth limit, and depth alone cannot
// bound a wide/repeated DAG.
const MAX_INCLUDE_REFERENCES: usize = 1024;
// Extension safety policy: `$Inherits` repeats a parent's entries once per
// child, so a shallow, acyclic document can still expand a small parent into an
// arbitrarily large store. Copies are bounded by the text the composed document
// itself holds, which is what a legitimately wide inheritance can afford,
// clamped so small documents still get room and large ones never license more
// than the same 32 MiB the include loader allows. Independent of the depth and
// cycle limits.
const MAX_INHERITED_TEXT_RATIO: usize = 64;
const MIN_INHERITED_TEXT_BYTES: usize = 1024 * 1024;
const MAX_INHERITED_TEXT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IncludeDialect {
    Ares,
    Phobos,
}

impl IncludeDialect {
    fn section(self) -> &'static str {
        match self {
            Self::Ares => ARES,
            Self::Phobos => PHOBOS,
        }
    }

    fn reference(self) -> Reference {
        match self {
            Self::Ares => Reference::AresInclude,
            Self::Phobos => Reference::PhobosInclude,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DirectiveOptions {
    /// Reference edges below the root on the active path, not a visited set.
    pub max_depth: usize,
    pub strict: bool,
    /// Execute Ares `[#include]`. Ares reads the list after the document loads.
    pub ares: bool,
    /// Execute Phobos `[$Include]`. Phobos `-Include` replaces the Ares hooks,
    /// so a document that uses `$Include` leaves its `[#include]` inert.
    pub phobos: bool,
}

impl Default for DirectiveOptions {
    fn default() -> Self {
        Self {
            max_depth: 64,
            strict: false,
            ares: true,
            phobos: true,
        }
    }
}

impl DirectiveOptions {
    /// Dialects this binary can execute. Natively one launch runs one dialect:
    /// Phobos `-Include` swaps `$Include` in for the Ares hooks. VERA has no
    /// such switch, so a combined build keeps both, and a document that uses
    /// `$Include` takes over from its `[#include]`.
    pub fn production() -> Self {
        Self {
            ares: crate::rules::ARES_ENABLED,
            phobos: crate::rules::PHOBOS_ENABLED,
            ..Self::default()
        }
    }

    /// Whether these options expand any include list `ini` carries.
    pub(crate) fn expands_includes(self, ini: &IniFile) -> bool {
        select_include_dialect(ini, self).is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    InvalidDepth,
    IncludeCycle,
    IncludeDepthLimit,
    IncludeWorkLimit,
    IncludeMissing,
    IncludeLoadFailed,
    InheritanceCycle,
    InheritanceDepthLimit,
    InheritanceParentMissing,
    /// Inheritance copies would exceed the aggregate expanded-text budget.
    InheritanceWorkLimit,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DirectiveDiagnostic {
    pub kind: DiagnosticKind,
    pub source_name: String,
    pub reference_chain: Vec<String>,
    pub message: String,
}

impl std::fmt::Display for DirectiveDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} in {}: {} (chain: {})",
            self.kind,
            self.source_name,
            self.message,
            self.reference_chain.join(" -> ")
        )
    }
}

impl std::error::Error for DirectiveDiagnostic {}

pub struct ExpandedIni {
    pub ini: IniFile,
    pub diagnostics: Vec<DirectiveDiagnostic>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reference {
    AresInclude,
    PhobosInclude,
    Inheritance,
}

impl Reference {
    fn identity(self, name: &str) -> String {
        match self {
            Self::AresInclude | Self::PhobosInclude => include_identity(name),
            Self::Inheritance => name.to_owned(),
        }
    }

    fn cycle_kind(self) -> DiagnosticKind {
        match self {
            Self::AresInclude | Self::PhobosInclude => DiagnosticKind::IncludeCycle,
            Self::Inheritance => DiagnosticKind::InheritanceCycle,
        }
    }

    fn depth_kind(self) -> DiagnosticKind {
        match self {
            Self::AresInclude | Self::PhobosInclude => DiagnosticKind::IncludeDepthLimit,
            Self::Inheritance => DiagnosticKind::InheritanceDepthLimit,
        }
    }
}

struct Traversal<'a> {
    source: &'a str,
    options: DirectiveOptions,
    path: Vec<String>,
    identities: Vec<String>,
    /// Every include name read during this root load: Ares
    /// `CCINIClass_ReadCCFile1`'s record, Phobos `SavedIncludes`.
    included: HashSet<String>,
    diagnostics: Vec<DirectiveDiagnostic>,
    include_references: usize,
    /// Key and value bytes copied into children by inheritance so far.
    inherited_text_bytes: usize,
    /// What those copies may aggregate to. Includes never charge it, so it is
    /// set from the composed document after include expansion.
    text_budget: usize,
}

impl Traversal<'_> {
    fn report(
        &mut self,
        kind: DiagnosticKind,
        target: Option<&str>,
        message: String,
    ) -> Result<(), DirectiveDiagnostic> {
        let mut chain = self.path.clone();
        chain.extend(target.map(str::to_owned));
        let diagnostic = DirectiveDiagnostic {
            kind,
            source_name: self.source.to_owned(),
            reference_chain: chain,
            message,
        };
        if self.options.strict {
            return Err(diagnostic);
        }
        self.diagnostics.push(diagnostic);
        Ok(())
    }

    /// Whether the surrounding load reports a missing listed file as fatal.
    /// Phobos `CCINIClass_Load_Inheritance` calls `Debug::FatalErrorAndExit`;
    /// Ares skips a missing include.
    fn missing_is_fatal(&self, reference: Reference) -> bool {
        reference == Reference::PhobosInclude
    }

    fn enter(&mut self, name: &str, reference: Reference) -> Result<bool, DirectiveDiagnostic> {
        let identity = reference.identity(name);
        // Includes skip every file this root load already read (see
        // `expand_includes`), so only inheritance can meet its own chain here.
        if reference == Reference::Inheritance && self.identities.contains(&identity) {
            self.report(
                reference.cycle_kind(),
                Some(name),
                "Circular reference skipped".into(),
            )?;
            return Ok(false);
        }
        if self.path.len() > self.options.max_depth {
            self.report(
                reference.depth_kind(),
                Some(name),
                format!(
                    "Maximum active reference depth {} exceeded",
                    self.options.max_depth
                ),
            )?;
            return Ok(false);
        }
        self.path.push(name.to_owned());
        self.identities.push(identity);
        Ok(true)
    }

    fn exit(&mut self) {
        self.path.pop();
        self.identities.pop();
    }

    /// Reserve the text a pending copy would add before that copy is made. One
    /// parent may feed many children, so this is charged per copy, and only for
    /// entries the child does not already author. Exhaustion always fails rather
    /// than publishing a partially inherited store, as the include-reference
    /// limit does.
    fn reserve_inherited_text(&mut self, bytes: usize) -> Result<(), DirectiveDiagnostic> {
        let retained = self.inherited_text_bytes.saturating_add(bytes);
        let budget = self.text_budget;
        if retained > budget {
            return Err(DirectiveDiagnostic {
                kind: DiagnosticKind::InheritanceWorkLimit,
                source_name: self.source.to_owned(),
                reference_chain: self.path.clone(),
                message: format!(
                    "Maximum expanded inheritance text {budget} bytes exceeded: {retained} bytes"
                ),
            });
        }
        self.inherited_text_bytes = retained;
        Ok(())
    }
}

/// Logical identities are independent of the host OS/cwd. Lookup still belongs
/// to the callback; all include names use the same resource root.
pub(crate) fn include_identity(name: &str) -> String {
    let name = name.replace('\\', "/");
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|part| *part != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/").to_ascii_lowercase()
}

/// Aggregate text this composed document may add by copying inherited entries:
/// a multiple of its own stored text, so legitimately wide inheritance stays
/// compatible, clamped at both ends. Includes bound their physical reads at the
/// loader; inheritance instead repeats a parent once per child.
fn inherited_text_budget(ini: &IniFile) -> usize {
    let authored: usize = ini
        .sections
        .iter()
        .map(|section| {
            section
                .key_order
                .iter()
                .map(|key| key.len() + section.entries[key].len())
                .sum::<usize>()
        })
        .sum();
    authored
        .saturating_mul(MAX_INHERITED_TEXT_RATIO)
        .clamp(MIN_INHERITED_TEXT_BYTES, MAX_INHERITED_TEXT_BYTES)
}

fn select_include_dialect(ini: &IniFile, options: DirectiveOptions) -> Option<IncludeDialect> {
    // Phobos takes priority and disables the Ares hooks, so `[#include]` runs
    // only when the caller has not selected the Phobos dialect.
    if options.phobos && ini.section(PHOBOS).is_some() {
        Some(IncludeDialect::Phobos)
    } else if options.ares && ini.section(ARES).is_some() {
        Some(IncludeDialect::Ares)
    } else {
        None
    }
}

/// The caller supplies physical snapshots through its existing resource owner.
/// A strict failure discards the owned candidate rather than exposing partially
/// expanded state. Expand all includes before resolving missing-key inheritance.
pub fn process_directives(
    ini: IniFile,
    source_name: &str,
    mut load: impl FnMut(&str) -> Result<Option<Vec<u8>>, String>,
    options: DirectiveOptions,
) -> Result<ExpandedIni, DirectiveDiagnostic> {
    if options.max_depth == 0 {
        return Err(DirectiveDiagnostic {
            kind: DiagnosticKind::InvalidDepth,
            source_name: source_name.into(),
            reference_chain: Vec::new(),
            message: "max_depth must be positive".into(),
        });
    }
    let dialect = select_include_dialect(&ini, options);
    let inheritance = ini
        .sections
        .iter()
        .any(|section| section.is_present(INHERITS));
    if dialect.is_none() && !inheritance {
        return Ok(ExpandedIni {
            ini,
            diagnostics: Vec::new(),
        });
    }
    let mut traversal = Traversal {
        source: source_name,
        options,
        path: Vec::new(),
        identities: Vec::new(),
        included: HashSet::new(),
        diagnostics: Vec::new(),
        include_references: 0,
        inherited_text_bytes: 0,
        // Includes never charge it, so the budget follows the composed
        // document once include expansion has finished.
        text_budget: 0,
    };
    let mut ini = ini;
    if let Some(dialect) = dialect {
        // Ares `CCINIClass_ReadCCFile1` records the root file as well.
        if dialect == IncludeDialect::Ares {
            traversal.included.insert(include_identity(source_name));
        }
        traversal.enter(source_name, dialect.reference())?;
        let listed = include_list(ini.section(dialect.section()), dialect, &mut traversal)?;
        ini = including_ini(ini);
        load_listed(listed, &mut ini, dialect, &mut load, &mut traversal)?;
        traversal.exit();
    }
    traversal.text_budget = inherited_text_budget(&ini);
    let mut completed = HashSet::new();
    // Preserve the host's physical section occurrences, first-section lookup
    // and native parsing. A document without directives is byte-parser exact.
    let names: Vec<String> = ini.section_names().into_iter().map(str::to_owned).collect();
    for name in names {
        inherit(&mut ini, &name, &mut completed, &mut traversal)?;
    }
    Ok(ExpandedIni {
        ini,
        diagnostics: traversal.diagnostics,
    })
}

/// The names a document's include section lists, in the dialect's read
/// order, charged against the include-reference budget before any is loaded.
fn include_list(
    section: Option<&IniSection>,
    dialect: IncludeDialect,
    traversal: &mut Traversal<'_>,
) -> Result<Vec<String>, DirectiveDiagnostic> {
    let Some(section) = section else {
        return Ok(Vec::new());
    };
    let remaining = MAX_INCLUDE_REFERENCES - traversal.include_references;
    let includes: Vec<String> = match dialect {
        // Ares `GetKeyName` walks stored entries in file order, then
        // `ReadString` looks each key up through the native CRC index.
        IncludeDialect::Ares => section
            .physical_entries()
            .filter_map(|(key, _)| {
                section
                    .read_name(key, ARES_INCLUDE_NAME_CAPACITY)
                    .map(str::to_owned)
            })
            .take(remaining + 1)
            .collect(),
        // Phobos iterates the entry list and uses each stored value directly.
        IncludeDialect::Phobos => section
            .physical_entries()
            .filter(|(_, value)| !value.is_empty())
            .map(|(_, value)| value.to_owned())
            .take(remaining + 1)
            .collect(),
    };
    if includes.len() > remaining {
        return Err(DirectiveDiagnostic {
            kind: DiagnosticKind::IncludeWorkLimit,
            source_name: traversal.source.into(),
            reference_chain: traversal.path.clone(),
            message: format!("Maximum total include references {MAX_INCLUDE_REFERENCES} exceeded"),
        });
    }
    // Reserve the complete list before loading children, including repeated
    // references. Exhaustion always fails rather than publishing a partial INI.
    traversal.include_references += includes.len();
    Ok(includes)
}

/// The INI the root's included files load into. Both dialects apply the
/// current document first: a later read, meaning an included file or a later
/// sibling, overwrites its values. Natively that INI is the root itself, as
/// it stays in an Ares build; otherwise VERA composes the compatibility
/// projection [`IniFile::merge`] builds.
fn including_ini(root: IniFile) -> IniFile {
    #[cfg(feature = "ares")]
    let ini = root;
    #[cfg(not(feature = "ares"))]
    let ini = {
        let mut merged = IniFile::empty();
        merged.merge(&root);
        merged
    };
    ini
}

/// `ReadCCFile` reads an included file into the INI that lists it, through
/// `LoadFromStraw`'s merge arm, where an Ares launch's parser hooks act on it.
/// Returns the file's own `list` section.
fn load_included(
    merged: &mut IniFile,
    bytes: &[u8],
    list: &str,
) -> Result<Option<IniSection>, crate::rules::error::RulesError> {
    #[cfg(feature = "ares")]
    let own = Some(merged.merge_load(bytes, list));
    #[cfg(not(feature = "ares"))]
    let own = {
        let child = IniFile::from_bytes(bytes)?;
        merged.merge(&child);
        child.section(list).cloned()
    };
    Ok(own)
}

/// Load each listed file into `merged`, each followed depth-first by the
/// files it lists in turn, which override it.
fn load_listed(
    includes: Vec<String>,
    merged: &mut IniFile,
    dialect: IncludeDialect,
    load: &mut impl FnMut(&str) -> Result<Option<Vec<u8>>, String>,
    traversal: &mut Traversal<'_>,
) -> Result<(), DirectiveDiagnostic> {
    let reference = dialect.reference();
    for name in includes {
        // Each file is read once per root load: Phobos `SavedIncludes` records
        // a listed name before its existence check, and Ares records each file
        // it reads (below).
        let identity = include_identity(&name);
        let repeated = match dialect {
            IncludeDialect::Phobos => !traversal.included.insert(identity.clone()),
            IncludeDialect::Ares => traversal.included.contains(&identity),
        };
        if repeated {
            // VERA reports an Ares cycle; Phobos skips saved names silently.
            if dialect == IncludeDialect::Ares && traversal.identities.contains(&identity) {
                traversal.report(
                    reference.cycle_kind(),
                    Some(&name),
                    "Circular reference skipped".into(),
                )?;
            }
            continue;
        }
        if !traversal.enter(&name, reference)? {
            continue;
        }
        match load(&name) {
            Ok(Some(bytes)) => {
                traversal.included.insert(identity);
                match load_included(merged, &bytes, dialect.section()) {
                    Ok(own) => {
                        let listed = include_list(own.as_ref(), dialect, traversal)?;
                        load_listed(listed, merged, dialect, load, traversal)?;
                    }
                    Err(error) => traversal.report(
                        DiagnosticKind::IncludeLoadFailed,
                        None,
                        format!("Unable to load '{name}': {error}"),
                    )?,
                }
            }
            Ok(None) if traversal.missing_is_fatal(reference) => {
                // `enter` already placed the target at the end of the chain.
                let reference_chain = traversal.path.clone();
                let message = format!("Included file '{name}' is missing");
                return Err(DirectiveDiagnostic {
                    kind: DiagnosticKind::IncludeMissing,
                    source_name: traversal.source.to_owned(),
                    reference_chain,
                    message,
                });
            }
            Ok(None) => traversal.report(
                DiagnosticKind::IncludeMissing,
                None,
                format!("Included file '{name}' is missing"),
            )?,
            Err(reason) => traversal.report(
                DiagnosticKind::IncludeLoadFailed,
                None,
                format!("Unable to load '{name}': {reason}"),
            )?,
        }
        traversal.exit();
    }
    Ok(())
}

fn inherit(
    ini: &mut IniFile,
    name: &str,
    completed: &mut HashSet<String>,
    traversal: &mut Traversal<'_>,
) -> Result<bool, DirectiveDiagnostic> {
    if completed.contains(name) {
        return Ok(true);
    }
    if !traversal.enter(name, Reference::Inheritance)? {
        return Ok(false);
    }
    let parents = ini
        .section(name)
        .and_then(|section| section.read_list(INHERITS, usize::MAX))
        .unwrap_or_default()
        .into_iter()
        .map(|name| name.trim().to_owned())
        .collect::<Vec<_>>();
    let mut success = true;
    for parent in parents {
        if ini.section(&parent).is_none() {
            traversal.report(
                DiagnosticKind::InheritanceParentMissing,
                Some(&parent),
                format!("Parent section '{parent}' is missing"),
            )?;
            continue;
        }
        if !inherit(ini, &parent, completed, traversal)? {
            success = false;
            continue;
        }
        // Copy only after a parent completes. Never copy a cyclic/depth-cut
        // parent's partially resolved branch. The first parent supplying a
        // key wins; child-authored values always win, even for later parents.
        // Charge the copy before making it: a parent shared by many children
        // repeats this text once per child. Only keys the child does not author
        // are copied, and directive keys never are.
        let index = ini.section_position(name).unwrap();
        let (keys, copied) = {
            let parent = ini.section(&parent).unwrap();
            let child = &ini.sections[index];
            let mut keys = Vec::new();
            let mut copied = 0;
            for key in &parent.key_order {
                if matches!(key.as_str(), INHERITS | PHOBOS | ARES) || child.is_present(key) {
                    continue;
                }
                copied += key.len() + parent.entries[key].len();
                keys.push(key.clone());
            }
            (keys, copied)
        };
        traversal.reserve_inherited_text(copied)?;
        let parent = ini.section(&parent).unwrap().clone();
        let child = &mut ini.sections[index];
        for key in keys {
            child.set(&key, &parent.entries[&key]);
        }
    }
    if success {
        completed.insert(name.to_owned());
    }
    traversal.exit();
    Ok(success)
}

#[cfg(test)]
#[path = "ini_directives_tests.rs"]
mod tests;
