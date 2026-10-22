//! Include/inheritance traversal adapted from baixiong443's ra2-mono IniFile.
//! This is a VERA extension policy, not executable Ares/Phobos equivalence.
//! Keep the native byte parser and ordered Rules processing as their owners.

use super::IniFile;
use std::collections::HashSet;

const ARES: &str = "#include";
const PHOBOS: &str = "$Include";
const INHERITS: &str = "$Inherits";

#[derive(Clone, Copy, Debug)]
pub struct DirectiveOptions {
    /// Reference edges below the root on the active path, not a visited set.
    pub max_depth: usize,
    pub strict: bool,
}

impl Default for DirectiveOptions {
    fn default() -> Self {
        Self {
            max_depth: 64,
            strict: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    InvalidDepth,
    IncludeCycle,
    IncludeDepthLimit,
    IncludeMissing,
    IncludeLoadFailed,
    InheritanceCycle,
    InheritanceDepthLimit,
    InheritanceParentMissing,
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

struct Traversal<'a> {
    source: &'a str,
    options: DirectiveOptions,
    path: Vec<String>,
    identities: Vec<String>,
    diagnostics: Vec<DirectiveDiagnostic>,
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

    fn enter(&mut self, name: &str, includes: bool) -> Result<bool, DirectiveDiagnostic> {
        let identity = if includes {
            include_identity(name)
        } else {
            name.to_owned()
        };
        if self.identities.contains(&identity) {
            self.report(
                if includes {
                    DiagnosticKind::IncludeCycle
                } else {
                    DiagnosticKind::InheritanceCycle
                },
                Some(name),
                "Circular reference skipped".into(),
            )?;
            return Ok(false);
        }
        if self.path.len() > self.options.max_depth {
            self.report(
                if includes {
                    DiagnosticKind::IncludeDepthLimit
                } else {
                    DiagnosticKind::InheritanceDepthLimit
                },
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

/// The caller supplies physical snapshots through its existing resource owner.
/// Later includes override earlier ones. Ares `#include` loads this document
/// first, so included values win; `$Include` retains the mono current-file-wins
/// policy, without claiming that policy is executable Phobos equivalence.
/// Expand all includes before resolving missing-key inheritance. Strict failure
/// discards the owned candidate rather than exposing partially expanded state.
pub fn process_directives(
    ini: IniFile,
    source_name: &str,
    mut load: impl FnMut(&str) -> Result<Option<IniFile>, String>,
    options: DirectiveOptions,
) -> Result<ExpandedIni, DirectiveDiagnostic> {
    let mut traversal = Traversal {
        source: source_name,
        options,
        path: Vec::new(),
        identities: Vec::new(),
        diagnostics: Vec::new(),
    };
    if options.max_depth == 0 {
        return Err(DirectiveDiagnostic {
            kind: DiagnosticKind::InvalidDepth,
            source_name: source_name.into(),
            reference_chain: Vec::new(),
            message: "max_depth must be positive".into(),
        });
    }
    if !ini.sections.iter().any(|section| {
        matches!(section.name.as_str(), ARES | PHOBOS) || section.is_present(INHERITS)
    }) {
        return Ok(ExpandedIni {
            ini,
            diagnostics: Vec::new(),
        });
    }
    traversal.enter(source_name, true)?;
    let mut ini = expand_includes(ini, &mut load, &mut traversal)?;
    traversal.exit();
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

fn expand_includes(
    ini: IniFile,
    load: &mut impl FnMut(&str) -> Result<Option<IniFile>, String>,
    traversal: &mut Traversal<'_>,
) -> Result<IniFile, DirectiveDiagnostic> {
    // Retain mono's single-section priority, not a concatenation of dialects.
    let Some(section) = ini.section(PHOBOS).or_else(|| ini.section(ARES)) else {
        return Ok(ini);
    };
    let current_file_wins = section.name == PHOBOS;
    let mut includes: Vec<_> = section
        .keys()
        .filter_map(|key| {
            section.read_name(key, usize::MAX).map(|name| {
                (
                    key.parse::<i32>().unwrap_or(i32::MAX),
                    key.to_owned(),
                    name.to_owned(),
                )
            })
        })
        .collect();
    includes.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut merged = IniFile::empty();
    if !current_file_wins {
        // Ares documents root Strength=200 + included Strength=300 as 300.
        // Read this document before its depth-first descendants, not after.
        // https://ares-developers.github.io/Ares-docs/new/misc/include.html
        merged.merge(&ini);
    }
    for (_, _, name) in includes {
        if !traversal.enter(&name, true)? {
            continue;
        }
        // Exiting happens on every non-strict path, including failed loads.
        match load(&name) {
            Ok(Some(child)) => {
                let child = expand_includes(child, load, traversal)?;
                merged.merge(&child);
            }
            Ok(None) => traversal.report(
                DiagnosticKind::IncludeMissing,
                None,
                format!("Included file '{name}' is missing or empty"),
            )?,
            Err(reason) => traversal.report(
                DiagnosticKind::IncludeLoadFailed,
                None,
                format!("Unable to load '{name}': {reason}"),
            )?,
        }
        traversal.exit();
    }
    if current_file_wins {
        merged.merge(&ini);
    }
    Ok(merged)
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
    if !traversal.enter(name, false)? {
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
        let parent = ini.section(&parent).unwrap().clone();
        let index = ini.section_position(name).unwrap();
        let child = &mut ini.sections[index];
        for key in &parent.key_order {
            if !matches!(key.as_str(), INHERITS | PHOBOS | ARES) && !child.is_present(key) {
                child.set(key, &parent.entries[key]);
            }
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
