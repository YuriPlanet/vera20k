//! The `vera.actions` and `vera.delivery` custom sections (appendix "Custom
//! sections"), read once at load. Each starts with a format byte; names are
//! a `u16` length and ASCII bytes, compared with ASCII case folding. An
//! unknown format, truncation, trailing bytes, a non-ASCII name, a duplicate
//! name or a repeated section rejects the module.

/// Where `on_events` batches are written in module memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub offset: u32,
    pub bytes: u32,
}

/// What a module's custom sections declare.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sections {
    /// `vera.actions`: (name, dispatch key), in section order.
    pub actions: Vec<(String, i32)>,
    pub delivery: Option<Delivery>,
}

pub const ACTIONS: &str = "vera.actions";
pub const DELIVERY: &str = "vera.delivery";

impl Sections {
    /// Reads every `vera.actions` and `vera.delivery` section of a module
    /// that already passed validation.
    pub fn read(module: &[u8]) -> Result<Self, String> {
        let mut sections = Self::default();
        let (mut saw_actions, mut saw_delivery) = (false, false);
        for payload in wasmparser::Parser::new(0).parse_all(module) {
            let wasmparser::Payload::CustomSection(section) = payload.map_err(|e| e.to_string())?
            else {
                continue;
            };
            let seen = match section.name() {
                ACTIONS => &mut saw_actions,
                DELIVERY => &mut saw_delivery,
                _ => continue,
            };
            if std::mem::replace(seen, true) {
                return Err(format!("{} appears twice", section.name()));
            }
            let data = section.data();
            if section.name() == ACTIONS {
                sections.actions = parse_actions(data).map_err(|e| format!("{ACTIONS}: {e}"))?;
            } else {
                sections.delivery =
                    Some(parse_delivery(data).map_err(|e| format!("{DELIVERY}: {e}"))?);
            }
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
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn end(&self) -> Result<(), String> {
        match self.data.len() - self.at {
            0 => Ok(()),
            extra => Err(format!("{extra} trailing bytes")),
        }
    }
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

/// `format: u8 = 1, offset: u32, bytes: u32`.
fn parse_delivery(data: &[u8]) -> Result<Delivery, String> {
    let mut reader = Reader { data, at: 0 };
    reader.format()?;
    let offset = u32::from_le_bytes(reader.array()?);
    let bytes = u32::from_le_bytes(reader.array()?);
    reader.end()?;
    Ok(Delivery { offset, bytes })
}
