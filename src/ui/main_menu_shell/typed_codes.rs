//! Main-menu typed codes: the letter buffer `MainMenuDialog0xE2__Run`
//! (`0x00531CC0`) fills from the Keyboard queue while dialog `0xE2` runs, and
//! its table `MainMenuTypedCodeTable` (`0x00825C28`). No other shell loop reads
//! the buffer; it lives for the process, across every `0xE2` instance.
//!
//! RESIDUAL: a key whose virtual key is 0x80 or above reaches CRT `isalpha`
//! (`0x007C9A31`) sign-extended (`movsx` at `0x00531E1C`) and reads the bytes
//! before `_pctype` (`0x0087C59A`), so some of them (OEM `-`, `.`, `/`, `` ` ``)
//! append their own byte (`toupper` `0x007C97D3` keeps it) where VERA clears
//! the buffer. Either way no code matches across that key; only when the
//! 31-byte reset comes differs.

/// One `MainMenuTypedCodeTable` row (`0x00825C28`, 16 bytes each: flag pointer,
/// code text, an unread text, keep byte), in table order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuTypedCode {
    /// `PENGO` (`0x00825CC8`) toggles `0x00A8ED90`, which nothing else reads.
    Pengo,
    /// `THETEAM` (`0x00825CBC`) toggles `0x00A8ED91`. In skirmish (game mode
    /// 5) `ScenarioClass::Full_Init` reads it at `0x00687B21`, checks
    /// `TMCJ4F.INI` availability at `0x00687B2A..0x00687B58`, then Processes
    /// it at `0x00687B76`, after the AIMD/map AI readers. Only RA2's
    /// MAPS01/02.MIX carry it; stock YR mounts neither.
    TheTeam,
    /// `HIRES` (`0x00825CB4`) toggles OptionsClass+0x35 (`0x00A8EB95`), the
    /// `[Video] AllowHiResModes` byte.
    HiRes,
    /// `TOGGLE` (`0x00825CA8`) toggles `0x00A8ED63`, `[Video] AllowModeToggle`.
    Toggle,
}

impl MainMenuTypedCode {
    /// Table order, which decides the match when the buffer holds two codes.
    const TABLE: [(Self, &'static [u8]); 4] = [
        (Self::Pengo, b"PENGO"),
        (Self::TheTeam, b"THETEAM"),
        (Self::HiRes, b"HIRES"),
        (Self::Toggle, b"TOGGLE"),
    ];
}

/// The options profile's `[Video]` bytes that `HIRES` and `TOGGLE` point at.
pub struct VideoCodeFlags<'a> {
    pub allow_hi_res_modes: &'a mut bool,
    pub allow_mode_toggle: &'a mut bool,
}

/// Buffer `0x00A8F7B8` (32 bytes, NUL-terminated) and the two flags no other
/// owner holds.
#[derive(Debug, Clone, Default)]
pub struct MainMenuTypedCodes {
    buffer: [u8; 32],
    pengo: bool,
    the_team: bool,
}

impl MainMenuTypedCodes {
    /// The retained `THETEAM` flag read by Full_Init's mode-5 TMCJ4F gate
    /// (`0x00A8ED91`, `0x00687B21`).
    pub(crate) const fn the_team_enabled(&self) -> bool {
        self.the_team
    }

    /// One key-down from the Keyboard queue (`0x00531DEB..0x00531F40`); the
    /// caller skips releases (0x800, `0x00531E13`). A match toggles its flag
    /// and returns the code, which `0xE2` answers with one `OptionsChanged`.
    pub fn key_down(
        &mut self,
        virtual_key: u16,
        video: VideoCodeFlags<'_>,
    ) -> Option<MainMenuTypedCode> {
        // Only the low byte is tested (`0x00531E1C`): Shift, Ctrl and Alt
        // bits leave the letter as it is.
        let key = virtual_key as u8;
        // isalpha over the static C-locale table: exactly A-Z and a-z below
        // 0x80. `~` (`0x00531E30`) is no letter either.
        if !key.is_ascii_alphabetic() || key == b'~' {
            self.buffer = [0; 32];
            return None;
        }
        let mut len = self.len();
        // 0x00531E60..0x00531E67: a full buffer zeroes only its first byte,
        // so the new letter overwrites it and the old tail stays.
        if len >= 31 {
            len = 0;
            self.buffer[0] = 0;
        }
        self.buffer[len] = key.to_ascii_uppercase();
        // strstr `0x007CA4B0` per row: a code matches anywhere in the buffer.
        let text = &self.buffer[..self.len()];
        let (code, _) = MainMenuTypedCode::TABLE
            .iter()
            .find(|(_, code)| text.windows(code.len()).any(|window| window == *code))?;
        // 0x00531EB9..0x00531EC8: flag = (flag == 0), then clear the buffer.
        let flag = self.flag(*code, video);
        *flag = !*flag;
        self.buffer = [0; 32];
        Some(*code)
    }

    /// `Main__PrepareSession` clears every flag whose row keep byte (+0xC) is
    /// 0, all but `TOGGLE`, before a WOL (`0x0052E21E`) or LAN (`0x0052E35D`)
    /// session.
    pub fn clear_for_network_session(&mut self, video: VideoCodeFlags<'_>) {
        self.pengo = false;
        self.the_team = false;
        *video.allow_hi_res_modes = false;
    }

    fn flag<'a>(&'a mut self, code: MainMenuTypedCode, video: VideoCodeFlags<'a>) -> &'a mut bool {
        match code {
            MainMenuTypedCode::Pengo => &mut self.pengo,
            MainMenuTypedCode::TheTeam => &mut self.the_team,
            MainMenuTypedCode::HiRes => video.allow_hi_res_modes,
            MainMenuTypedCode::Toggle => video.allow_mode_toggle,
        }
    }

    fn len(&self) -> usize {
        // Writes stop at index 30, so byte 31 always ends the string.
        self.buffer.iter().position(|&byte| byte == 0).unwrap_or(31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Video {
        hi_res: bool,
        toggle: bool,
    }

    impl Video {
        fn flags(&mut self) -> VideoCodeFlags<'_> {
            VideoCodeFlags {
                allow_hi_res_modes: &mut self.hi_res,
                allow_mode_toggle: &mut self.toggle,
            }
        }
    }

    fn type_text(
        codes: &mut MainMenuTypedCodes,
        video: &mut Video,
        text: &[u8],
    ) -> Vec<MainMenuTypedCode> {
        text.iter()
            .filter_map(|&key| codes.key_down(u16::from(key), video.flags()))
            .collect()
    }

    #[test]
    fn codes_match_inside_the_buffer_once_and_other_keys_clear_it() {
        let mut codes = MainMenuTypedCodes::default();
        let mut video = Video::default();
        // A modifier bit leaves the letter; F1 (VK 0x70, 'p') is a letter too.
        codes.key_down(0x100 | u16::from(b'X'), video.flags());
        codes.key_down(0x70, video.flags());
        assert_eq!(
            type_text(&mut codes, &mut video, b"ENGO"),
            [MainMenuTypedCode::Pengo],
            "a match plays one cue"
        );
        assert!(codes.pengo);
        // The match emptied the buffer: GO alone is no code.
        assert!(type_text(&mut codes, &mut video, b"GO").is_empty());
        // Shift (VK 0x10) is a queued non-letter and clears the buffer.
        type_text(&mut codes, &mut video, b"HI");
        codes.key_down(0x10, video.flags());
        assert!(type_text(&mut codes, &mut video, b"RES").is_empty());
        assert!(!video.hi_res);
        type_text(&mut codes, &mut video, b"HIRESTOGGLE");
        assert!(video.hi_res && video.toggle);
        // A LAN or WOL session keeps only TOGGLE.
        codes.clear_for_network_session(video.flags());
        assert!(!codes.pengo && !video.hi_res && video.toggle);
    }

    #[test]
    fn a_full_buffer_rewrites_only_its_first_byte() {
        let mut codes = MainMenuTypedCodes::default();
        let mut video = Video::default();
        // 31 letters fill the buffer; the 32nd overwrites byte 0 only, so the
        // P completes PENGO from the old tail.
        let full = b"QENGOQQQQQQQQQQQQQQQQQQQQQQQQQQ";
        assert_eq!(full.len(), 31);
        assert!(type_text(&mut codes, &mut video, full).is_empty());
        assert_eq!(
            type_text(&mut codes, &mut video, b"P"),
            [MainMenuTypedCode::Pengo]
        );
    }
}
