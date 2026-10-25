//! LAN setup payloads used by `Build_Options_String` (5DBB60), roster
//! publisher5DEAC7, options publisher5E3720 and the5DC350 GO handshake.
//!
//! This module owns formatting only. The app applies the text with the shared
//! CRT token readers and owns membership, map verification and launch state.

use super::{LanMessage, LanMessageError, LanMessageKind, c_string};
use crate::util::native_string::narrow_utf16_c_string;

const BODY_OFFSET: usize = 0x2f;
const OPTIONS_CAPACITY: usize = 400;
const ROSTER_CAPACITY: usize = 204;

pub const OPEN: &str = "@Open@";
pub const CLOSED: &str = "@Closed@";
pub const WAITING: &str = "@Waiting@";
pub const EASY_AI: &str = "@EasyAI@";
pub const MEDIUM_AI: &str = "@MediumAI@";
pub const HARD_AI: &str = "@HardAI@";
pub const OPEN_OBSERVER: &str = "@OpenObserver@";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanOptionsFields<'a> {
    pub text: &'a [u8],
    pub color: u32,
    pub game_name_hash: u32,
}

impl LanMessage {
    /// 5DEAC7..5DEB49 and5E8DC0 append every row and a trailing wide comma.
    pub fn roster(sender: &str, slots: &[&str]) -> Result<Self, LanMessageError> {
        let mut units = Vec::new();
        for name in slots {
            units.extend(name.encode_utf16().take_while(|unit| *unit != 0));
            units.push(u16::from(b','));
        }
        if units.len() >= ROSTER_CAPACITY {
            return Err(LanMessageError::TextTooLong {
                field: "roster UTF-16",
                actual: units.len(),
                maximum: ROSTER_CAPACITY - 1,
            });
        }
        let mut message = Self::with_name(LanMessageKind::Roster, sender)?;
        for (index, unit) in units.into_iter().enumerate() {
            let offset = BODY_OFFSET + index * 2;
            message.bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        Ok(message)
    }

    /// Raw UTF-16 names preserve code units; 5E8EA0 uses wcstok and skips
    /// empty comma-delimited tokens. The message itself retains all padding.
    pub fn roster_units(&self) -> Option<Vec<Vec<u16>>> {
        if self.kind() != LanMessageKind::Roster {
            return None;
        }
        let mut names = Vec::new();
        let mut name = Vec::new();
        for pair in self.bytes[BODY_OFFSET..].chunks_exact(2) {
            let unit = u16::from_le_bytes([pair[0], pair[1]]);
            if unit == 0 {
                break;
            }
            if unit == u16::from(b',') {
                if !name.is_empty() {
                    names.push(std::mem::take(&mut name));
                }
            } else {
                name.push(unit);
            }
        }
        if !name.is_empty() {
            names.push(name);
        }
        Some(names)
    }

    pub fn roster_names(&self) -> Option<Vec<String>> {
        self.roster_units().map(|names| {
            names
                .iter()
                .map(|name| String::from_utf16_lossy(name))
                .collect()
        })
    }

    /// 5E3720 copies an already-formatted narrow string. Packed description
    /// bytes are allowed; converting this payload through UTF-8 loses them.
    pub fn options(
        sender: &str,
        text: &[u8],
        color: u32,
        game_name_hash: u32,
    ) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::Options, sender)?;
        message.put_text(BODY_OFFSET, OPTIONS_CAPACITY, c_string(text), "options")?;
        message.put_u32(0x1bf, color);
        message.put_u32(0x1c3, game_name_hash);
        Ok(message)
    }

    pub fn options_fields(&self) -> Option<LanOptionsFields<'_>> {
        (self.kind() == LanMessageKind::Options).then(|| LanOptionsFields {
            text: c_string(&self.bytes[BODY_OFFSET..BODY_OFFSET + OPTIONS_CAPACITY]),
            color: self.u32(0x1bf),
            game_name_hash: self.u32(0x1c3),
        })
    }

    /// BD5E21C5..21ED formats Rcountry,color,start,team and calls5E3850.
    /// That LAN sender leaves the outgoing final eight bytes zero: its color
    /// and hash stores target the global receive buffer atA8D7F7/A8D7FB.
    pub fn player_choices(sender: &str, text: &[u8]) -> Result<Self, LanMessageError> {
        let mut message = Self::with_name(LanMessageKind::PlayerChoices, sender)?;
        message.put_text(
            BODY_OFFSET,
            OPTIONS_CAPACITY,
            c_string(text),
            "player choices",
        )?;
        Ok(message)
    }

    pub fn player_choices_text(&self) -> Option<&[u8]> {
        (self.kind() == LanMessageKind::PlayerChoices)
            .then(|| c_string(&self.bytes[BODY_OFFSET..BODY_OFFSET + OPTIONS_CAPACITY]))
    }

    /// 5DD57B..5DD5D6. The previously-published options own the seed.
    pub fn go(max_ahead: i32) -> Self {
        let mut message = Self::zeroed(LanMessageKind::Go);
        message.put_u32(BODY_OFFSET, max_ahead as u32);
        message
    }

    pub fn go_max_ahead(&self) -> Option<i32> {
        (self.kind() == LanMessageKind::Go).then(|| self.u32(BODY_OFFSET) as i32)
    }

    /// 5DD916..5DD94B: scenario verification succeeded and the guest can start.
    pub fn ready_to_go() -> Self {
        Self::zeroed(LanMessageKind::ReadyToGo)
    }

    /// 5E3630's LAN arm sends this named packet; receiver5E07A0 invokes
    /// 5E36E0, the same acceptance prompt as the options-text G arm.
    pub fn start_prompt(sender: &str) -> Result<Self, LanMessageError> {
        Self::with_name(LanMessageKind::StartPrompt, sender)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanOptionsHuman {
    pub name: String,
    pub country: i32,
    pub color: i32,
    pub start: i32,
    pub team: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanOptionsAi {
    pub difficulty: i32,
    pub country: i32,
    pub color: i32,
    pub start: i32,
    pub team: i32,
}

/// The values read by5DBB60. Slot values are the already-resolved getter
/// outputs; the app/session remains their authority. No map or rules defaults
/// are invented by the wire encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanGameOptions<const N: usize = 8> {
    pub seed: u32,
    pub current_speed: i32,
    pub game_speed: i32,
    pub credits: i32,
    pub unit_count: i32,
    pub short_game: bool,
    pub superweapons: bool,
    pub build_off_ally: bool,
    pub mcv_redeploy: bool,
    pub crates: bool,
    pub mode_index: i32,
    pub cooperative_difficulty: i32,
    pub map_description: String,
    pub map_player_capacity: i32,
    /// Map record+17C -> A8BB08 at5E7DCB; this is separate from RandMap.Sed.
    pub official_map: bool,
    pub map_file_size: i32,
    pub scenario_filename: String,
    pub map_digest: String,
    pub humans: Vec<LanOptionsHuman>,
    pub ai: [LanOptionsAi; N],
}

impl<const N: usize> LanGameOptions<N> {
    /// 5DBB60 uses signed `%d`, three literal zero `%x` fields per human,
    /// then the `@:` sentinel and all eight AI rows including their last comma.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},",
            self.seed as i32,
            self.current_speed,
            self.game_speed,
            self.credits,
            self.unit_count,
            i32::from(self.short_game),
            i32::from(self.superweapons),
            i32::from(self.build_off_ally),
            i32::from(self.mcv_redeploy),
            i32::from(self.crates),
            self.mode_index,
            self.cooperative_difficulty,
        )
        .into_bytes();
        out.extend(pack_map_description(
            &self.map_description.encode_utf16().collect::<Vec<_>>(),
        ));
        out.extend(
            format!(
                ",{},{},{},",
                self.map_player_capacity,
                i32::from(self.official_map),
                self.map_file_size
            )
            .bytes(),
        );
        out.extend(narrow_utf16_c_string(self.scenario_filename.encode_utf16()));
        out.push(b',');
        out.extend(narrow_utf16_c_string(self.map_digest.encode_utf16()));
        out.push(b':');
        for human in &self.humans {
            out.extend(narrow_utf16_c_string(human.name.encode_utf16()));
            out.extend(
                format!(
                    ",{},{},{},{},0,0,0,",
                    human.country, human.color, human.start, human.team
                )
                .bytes(),
            );
        }
        if !self.humans.is_empty() {
            *out.last_mut().expect("a human record ends in a comma") = b':';
        }
        out.extend(b"@:");
        for ai in self
            .ai
            .iter()
            .take(if self.map_player_capacity > 8 { N } else { 8 })
        {
            out.extend(
                format!(
                    "{},{},{},{},{},",
                    ai.difficulty,
                    if ai.country == -3 { -1 } else { ai.country },
                    ai.color,
                    ai.start,
                    ai.team
                )
                .bytes(),
            );
        }
        out
    }
}

/// 7B82E0 ->4335E0: encode little-endian UTF-16 including its NUL as groups
/// of seven payload bits; every nonfinal group receives a high marker bit.
/// The enclosing native narrow-string constructor stops at the first zero.
pub fn pack_map_description(units: &[u16]) -> Vec<u8> {
    let units: Vec<_> = units
        .iter()
        .copied()
        .take_while(|unit| *unit != 0)
        .chain(std::iter::once(0))
        .collect();
    let bit_count = units.len() * 16;
    let mut packed = vec![0; bit_count.div_ceil(7)];
    for bit in 0..bit_count {
        let value = ((units[bit / 16] >> (bit % 16)) & 1) as u8;
        packed[bit / 7] |= value << (bit % 7);
        if bit > 0 && bit % 7 == 0 {
            packed[bit / 7 - 1] |= 0x80;
        }
    }
    packed.truncate(
        packed
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(packed.len()),
    );
    packed
}

/// 7B83E0 ->433770: skip each eighth bit, reconstruct UTF-16 and stop at
/// its NUL. Native sizes its destination from the narrow strlen and always
/// reserves/clears the final wide unit; malformed input is bounded the same way.
pub fn unpack_map_description(packed: &[u8]) -> Vec<u16> {
    let packed = c_string(packed);
    let capacity = (((packed.len() + 1) * 7 / 8) + 1) / 2;
    let mut out = Vec::new();
    for index in 0..capacity.saturating_sub(1) {
        let mut unit = 0;
        for offset in 0..16 {
            let bit = index * 16 + offset;
            let value = packed.get(bit / 7).copied().unwrap_or(0);
            unit |= u16::from((value >> (bit % 7)) & 1) << offset;
        }
        if unit == 0 {
            break;
        }
        out.push(unit);
    }
    out
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
