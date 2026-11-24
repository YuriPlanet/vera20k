//! Reading retail tables out of a `gamemd.exe` image at their retail addresses,
//! for the tests that compare VERA20k's tables with the player's executable. The
//! game reads no `gamemd.exe` bytes.

/// Something went wrong reading the table out of the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrigTableError {
    NotPeFile,
    UnmappedAddress(u32),
    Truncated { need: usize, have: usize },
}

#[derive(Debug)]
struct FileSection<'a> {
    rva: u32,
    file_offset: usize,
    bytes: &'a [u8],
}

#[derive(Debug)]
struct PeSections<'a> {
    image_base: u32,
    sections: Vec<FileSection<'a>>,
}

impl<'a> PeSections<'a> {
    /// Read only initialized, file-backed section data. In particular, a
    /// section's zero-filled virtual tail and the certificate overlay are not
    /// table storage. PE32 layout: https://learn.microsoft.com/en-us/windows/win32/debug/pe-format
    fn parse(image: &'a [u8]) -> Result<Self, TrigTableError> {
        let u16_at = |bytes: &[u8], off: usize| -> Result<u16, TrigTableError> {
            let word = bytes
                .get(off..)
                .and_then(|tail| tail.get(..2))
                .ok_or(TrigTableError::NotPeFile)?;
            Ok(u16::from_le_bytes([word[0], word[1]]))
        };
        let u32_at = |bytes: &[u8], off: usize| -> Result<u32, TrigTableError> {
            let word = bytes
                .get(off..)
                .and_then(|tail| tail.get(..4))
                .ok_or(TrigTableError::NotPeFile)?;
            Ok(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        };

        if image.get(..2) != Some(b"MZ") {
            return Err(TrigTableError::NotPeFile);
        }
        let pe = image
            .get(u32_at(image, 0x3c)? as usize..)
            .ok_or(TrigTableError::NotPeFile)?;
        if pe.get(..4) != Some(b"PE\0\0") {
            return Err(TrigTableError::NotPeFile);
        }
        let section_count = usize::from(u16_at(pe, 6)?);
        let optional_size = usize::from(u16_at(pe, 20)?);
        let optional_and_sections = pe.get(24..).ok_or(TrigTableError::NotPeFile)?;
        let optional = optional_and_sections
            .get(..optional_size)
            .ok_or(TrigTableError::NotPeFile)?;
        // PE32+, or a short optional header, must never be decoded as PE32.
        if optional.len() < 96 || u16_at(optional, 0)? != 0x10b {
            return Err(TrigTableError::NotPeFile);
        }
        let image_base = u32_at(optional, 28)?;
        let headers = optional_and_sections
            .get(optional_size..)
            .and_then(|tail| tail.get(..section_count * 40))
            .ok_or(TrigTableError::NotPeFile)?;
        let mut sections = Vec::with_capacity(section_count);
        for header in headers.as_chunks::<40>().0 {
            let virtual_size = u32_at(header, 8)? as usize;
            let rva = u32_at(header, 12)?;
            let raw_size = u32_at(header, 16)? as usize;
            let file_offset = u32_at(header, 20)? as usize;
            if raw_size == 0 {
                continue;
            }
            let raw = image
                .get(file_offset..)
                .and_then(|tail| tail.get(..raw_size))
                .ok_or(TrigTableError::Truncated {
                    need: raw_size,
                    have: image.len().saturating_sub(file_offset),
                })?;
            let mapped_size = if virtual_size == 0 {
                raw_size
            } else {
                raw_size.min(virtual_size)
            };
            sections.push(FileSection {
                rva,
                file_offset,
                bytes: &raw[..mapped_size],
            });
        }
        Ok(Self {
            image_base,
            sections,
        })
    }

    fn at_va(&self, va: u32) -> Option<usize> {
        let rva = va.checked_sub(self.image_base)?;
        self.sections.iter().find_map(|section| {
            let within = rva.checked_sub(section.rva)? as usize;
            (within < section.bytes.len()).then_some(section.file_offset + within)
        })
    }
}

/// Translate a VA to initialized section bytes, excluding virtual zero-fill.
pub(crate) fn file_offset_of(image: &[u8], va: u32) -> Result<usize, TrigTableError> {
    PeSections::parse(image)?
        .at_va(va)
        .ok_or(TrigTableError::UnmappedAddress(va))
}
