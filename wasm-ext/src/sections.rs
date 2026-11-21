//! The `vera.vars` custom section (appendix "Custom sections"), read at load
//! time. A module that breaks its rules never loads.
//!
//! Layout, little endian and unpadded: a format byte (1), a `u32` count, then
//! per variable a `u16` name length, the ASCII name, a scope byte (8.3
//! `Scope`), a visibility byte (8.3 `Visibility`) and an `i64` default.
//!
//! A module holds at most one `vera.vars` section. Linkers concatenate
//! same-named custom sections from separate objects, so two declaration
//! blocks reach the engine as one section with trailing bytes and are
//! rejected: a module declares all its variables in one place.

/// The custom section's name.
pub const VARS_SECTION: &str = "vera.vars";

/// Variables one module may declare (open question 7).
pub const MAX_VARS: usize = 4096;

/// A variable's scope (appendix 8.3 `Scope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VarScope {
    Global,
    House,
    Object,
}

/// Which clients may read a variable (appendix 8.3 `Visibility`). Only the
/// client world reads it; the simulation sees every variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Private,
    Allies,
    /// Object scope only.
    InSight,
    Public,
}

/// One declared variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VarDecl {
    /// ASCII, compared case-insensitively.
    pub name: String,
    pub scope: VarScope,
    pub visibility: Visibility,
    pub default: i64,
}

/// Reads a `vera.vars` section's content. Rejects an unknown format, a
/// truncated or overlong section, an empty or non-ASCII name, a name declared
/// twice (ignoring ASCII case), an unknown scope or visibility, and
/// `InSight` outside object scope.
pub fn parse_vars(bytes: &[u8]) -> Result<Vec<VarDecl>, String> {
    let mut reader = Reader { bytes, at: 0 };
    let format = reader.take::<1>("format")?[0];
    if format != 1 {
        return Err(format!("vera.vars format {format} is unknown"));
    }
    let count = u32::from_le_bytes(reader.take::<4>("count")?);
    if count as usize > MAX_VARS {
        return Err(format!(
            "vera.vars declares {count} variables, limit {MAX_VARS}"
        ));
    }
    let mut vars: Vec<VarDecl> = Vec::with_capacity(count as usize);
    for index in 0..count {
        let len = u16::from_le_bytes(reader.take::<2>("name length")?) as usize;
        let name = reader.slice(len, "name")?;
        if name.is_empty() || !name.is_ascii() {
            return Err(format!(
                "vera.vars variable {index} has an empty or non-ASCII name"
            ));
        }
        // ASCII bytes are valid UTF-8.
        let name = String::from_utf8_lossy(name).into_owned();
        if vars.iter().any(|v| v.name.eq_ignore_ascii_case(&name)) {
            return Err(format!("vera.vars declares {name} twice"));
        }
        let [scope, visibility] = reader.take::<2>("scope and visibility")?;
        let scope = match scope {
            0 => VarScope::Global,
            1 => VarScope::House,
            2 => VarScope::Object,
            _ => return Err(format!("vera.vars {name} has unknown scope {scope}")),
        };
        let visibility = match visibility {
            0 => Visibility::Private,
            1 => Visibility::Allies,
            2 if scope == VarScope::Object => Visibility::InSight,
            3 => Visibility::Public,
            _ => {
                return Err(format!(
                    "vera.vars {name} has visibility {visibility} for scope {scope:?}"
                ));
            }
        };
        let default = i64::from_le_bytes(reader.take::<8>("default")?);
        vars.push(VarDecl {
            name,
            scope,
            visibility,
            default,
        });
    }
    if reader.at != bytes.len() {
        return Err(format!(
            "vera.vars has {} trailing bytes",
            bytes.len() - reader.at
        ));
    }
    Ok(vars)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn slice(&mut self, len: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(len);
        let slice = end.and_then(|end| self.bytes.get(self.at..end));
        let slice = slice.ok_or_else(|| format!("vera.vars is truncated in a {what}"))?;
        self.at += len;
        Ok(slice)
    }

    fn take<const N: usize>(&mut self, what: &str) -> Result<[u8; N], String> {
        let mut out = [0; N];
        out.copy_from_slice(self.slice(N, what)?);
        Ok(out)
    }
}

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
