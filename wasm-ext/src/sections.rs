//! The `vera.vars`, `vera.actions`, `vera.notify` and `vera.delivery`
//! custom sections (appendix "Custom sections"), read once at load. A module
//! that breaks their rules never loads.
//!
//! Each section is little endian and unpadded and starts with a format byte
//! (1); names are a `u16` length and ASCII bytes, compared with ASCII case
//! folding. An unknown format, truncation, trailing bytes, a non-ASCII name,
//! a duplicate name or a repeated section rejects the module. Linkers
//! concatenate same-named custom sections from separate objects, so two
//! declaration blocks reach the engine as one section with trailing bytes
//! and are rejected: a module declares each section in one place.

/// The custom sections' names.
pub const VARS: &str = "vera.vars";
pub const ACTIONS: &str = "vera.actions";
pub const DELIVERY: &str = "vera.delivery";
pub const NOTIFY: &str = "vera.notify";

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

/// Where `on_events` batches are written in module memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub offset: u32,
    pub bytes: u32,
}

/// What a module's custom sections declare.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sections {
    /// `vera.vars`, in declaration order.
    pub vars: Vec<VarDecl>,
    /// `vera.actions`: (name, dispatch key), in section order.
    pub actions: Vec<(String, i32)>,
    pub delivery: Option<Delivery>,
    /// `vera.notify`: the notification names the module sends, with their
    /// IDs ([`notify_id`]), in section order (RFC 0001 section 10).
    pub notify: Vec<(String, i32)>,
}

impl Sections {
    /// Reads every `vera.vars`, `vera.actions`, `vera.notify` and
    /// `vera.delivery` section of a module that already passed validation.
    pub fn read(module: &[u8]) -> Result<Self, String> {
        let mut sections = Self::default();
        let (mut saw_vars, mut saw_actions, mut saw_delivery) = (false, false, false);
        let mut saw_notify = false;
        for payload in wasmparser::Parser::new(0).parse_all(module) {
            let wasmparser::Payload::CustomSection(section) = payload.map_err(|e| e.to_string())?
            else {
                continue;
            };
            let name = section.name();
            let seen = match name {
                VARS => &mut saw_vars,
                ACTIONS => &mut saw_actions,
                DELIVERY => &mut saw_delivery,
                NOTIFY => &mut saw_notify,
                _ => continue,
            };
            if std::mem::replace(seen, true) {
                return Err(format!("{name} appears twice"));
            }
            let data = section.data();
            let parsed = match name {
                VARS => parse_vars(data).map(|vars| sections.vars = vars),
                ACTIONS => parse_actions(data).map(|actions| sections.actions = actions),
                NOTIFY => parse_notify(data).map(|notify| sections.notify = notify),
                _ => parse_delivery(data).map(|delivery| sections.delivery = Some(delivery)),
            };
            parsed.map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(sections)
    }

    /// The dispatch key `vera.actions` gives `name`, ASCII case-insensitive.
    pub fn action_key(&self, name: &str) -> Option<i32> {
        self.actions
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, key)| *key)
    }

    /// The first name `vera.actions` gives dispatch key `key`.
    pub fn action_name(&self, key: i32) -> Option<&str> {
        self.actions
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(name, _)| name.as_str())
    }

    /// The ID of notification `name` if `vera.notify` declares it (ASCII
    /// case-insensitive), as `lookup(Notify)` answers it.
    pub fn notify_lookup(&self, name: &str) -> Option<i32> {
        self.notify
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, id)| *id)
    }

    /// The declared name with ID `id`.
    pub fn notify_name(&self, id: i32) -> Option<&str> {
        self.notify
            .iter()
            .find(|(_, i)| *i == id)
            .map(|(name, _)| name.as_str())
    }
}

/// Notifications one module may declare, as for `vera.actions` (RFC 15.4).
pub const MAX_NOTIFY_NAMES: usize = 4096;

/// A notification name's ID (RFC 0001 section 10, appendix 8.1 `Notify`):
/// the 32-bit FNV-1a hash of its UTF-8 bytes with ASCII `A`..`Z` folded to
/// lowercase (nothing else folded), as the bits of an `i32`. The same on
/// every machine and in both worlds. A name hashing to −1 cannot be
/// declared ([`Sections::read`] rejects it).
pub fn notify_id(name: &str) -> i32 {
    name.bytes()
        .map(|b| b.to_ascii_lowercase())
        .fold(0x811C_9DC5u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        }) as i32
}

/// The first pair of different names in `a` and `b` (ASCII
/// case-insensitive) with the same ID: RFC 0001 section 3, step 7. A name
/// both declare is the same notification and no collision.
pub fn notify_collision<'a>(
    a: &'a [(String, i32)],
    b: &'a [(String, i32)],
) -> Option<(&'a str, &'a str)> {
    a.iter().find_map(|(name, id)| {
        b.iter()
            .find(|(other, other_id)| other_id == id && !other.eq_ignore_ascii_case(name))
            .map(|(other, _)| (name.as_str(), other.as_str()))
    })
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let at = self.at;
        let bytes = at
            .checked_add(len)
            .and_then(|end| self.data.get(at..end))
            .ok_or_else(|| format!("truncated at byte {at}"))?;
        self.at += len;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn format(&mut self) -> Result<(), String> {
        match self.array::<1>()? {
            [1] => Ok(()),
            [other] => Err(format!("unknown format {other}")),
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let len = u16::from_le_bytes(self.array()?);
        let bytes = self.take(usize::from(len))?;
        if !bytes.is_ascii() {
            return Err(format!(
                "name at byte {} is not ASCII",
                self.at - bytes.len()
            ));
        }
        // ASCII bytes are valid UTF-8.
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn end(&self) -> Result<(), String> {
        match self.data.len() - self.at {
            0 => Ok(()),
            extra => Err(format!("{extra} trailing bytes")),
        }
    }
}

/// `format: u8 = 1, count: u32, [{ name_len: u16, name, scope: u8,
/// visibility: u8, default: i64 }; count]`, scope and visibility as in
/// appendix 8.3. Also rejects more than [`MAX_VARS`] variables, an empty
/// name and `InSight` outside object scope.
fn parse_vars(data: &[u8]) -> Result<Vec<VarDecl>, String> {
    let mut reader = Reader { data, at: 0 };
    reader.format()?;
    let count = u32::from_le_bytes(reader.array()?);
    if count as usize > MAX_VARS {
        return Err(format!("declares {count} variables, limit {MAX_VARS}"));
    }
    let mut vars: Vec<VarDecl> = Vec::with_capacity(count as usize);
    for index in 0..count {
        let name = reader.name()?;
        if name.is_empty() {
            return Err(format!("variable {index} has an empty name"));
        }
        if vars.iter().any(|v| v.name.eq_ignore_ascii_case(&name)) {
            return Err(format!("duplicate name {name}"));
        }
        let [scope, visibility] = reader.array()?;
        let scope = match scope {
            0 => VarScope::Global,
            1 => VarScope::House,
            2 => VarScope::Object,
            _ => return Err(format!("{name} has unknown scope {scope}")),
        };
        let visibility = match visibility {
            0 => Visibility::Private,
            1 => Visibility::Allies,
            2 if scope == VarScope::Object => Visibility::InSight,
            3 => Visibility::Public,
            _ => {
                return Err(format!(
                    "{name} has visibility {visibility} for scope {scope:?}"
                ));
            }
        };
        let default = i64::from_le_bytes(reader.array()?);
        vars.push(VarDecl {
            name,
            scope,
            visibility,
            default,
        });
    }
    reader.end()?;
    Ok(vars)
}

/// `format: u8 = 1, count: u32, [{ name_len: u16, name, key: i32 }; count]`.
fn parse_actions(data: &[u8]) -> Result<Vec<(String, i32)>, String> {
    let mut reader = Reader { data, at: 0 };
    reader.format()?;
    let count = u32::from_le_bytes(reader.array()?);
    let mut actions: Vec<(String, i32)> = Vec::new();
    for _ in 0..count {
        let name = reader.name()?;
        let key = i32::from_le_bytes(reader.array()?);
        if actions.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            return Err(format!("duplicate name {name}"));
        }
        actions.push((name, key));
    }
    reader.end()?;
    Ok(actions)
}

/// `format: u8 = 1, count: u32, [{ name_len: u16, name }; count]`. Also
/// rejects an empty name, more than [`MAX_NOTIFY_NAMES`] names, a name whose
/// ID is −1 and two names with the same ID (RFC 0001 section 3, step 7).
fn parse_notify(data: &[u8]) -> Result<Vec<(String, i32)>, String> {
    let mut reader = Reader { data, at: 0 };
    reader.format()?;
    let count = u32::from_le_bytes(reader.array()?);
    if count as usize > MAX_NOTIFY_NAMES {
        return Err(format!(
            "declares {count} notifications, limit {MAX_NOTIFY_NAMES}"
        ));
    }
    let mut names: Vec<(String, i32)> = Vec::with_capacity(count as usize);
    for index in 0..count {
        let name = reader.name()?;
        if name.is_empty() {
            return Err(format!("notification {index} has an empty name"));
        }
        if names.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            return Err(format!("duplicate name {name}"));
        }
        let id = notify_id(&name);
        if id == -1 {
            return Err(format!(
                "{name} hashes to -1, which no notification may have"
            ));
        }
        if let Some((other, _)) = names.iter().find(|(_, other)| *other == id) {
            return Err(format!(
                "{name} and {other} have the same notification ID {id}"
            ));
        }
        names.push((name, id));
    }
    reader.end()?;
    Ok(names)
}

/// `format: u8 = 1, offset: u32, bytes: u32`.
fn parse_delivery(data: &[u8]) -> Result<Delivery, String> {
    let mut reader = Reader { data, at: 0 };
    reader.format()?;
    let offset = u32::from_le_bytes(reader.array()?);
    let bytes = u32::from_le_bytes(reader.array()?);
    reader.end()?;
    Ok(Delivery { offset, bytes })
}

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
