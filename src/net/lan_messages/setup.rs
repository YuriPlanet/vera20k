//! LAN setup packets: the options publisher5E3720's short texts, player
//! choices and the5DC350 GO handshake. A VERA host's full options and roster
//! travel in VS31 setup payloads instead ([`super::extended_setup`]); the
//! roster names below are the sentinels its roster payload keeps.
//!
//! This module owns formatting only. The app owns membership, map
//! verification and launch state.

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

use super::{LanMessage, LanMessageError, LanMessageKind, c_string};

const BODY_OFFSET: usize = 0x2f;
const OPTIONS_CAPACITY: usize = 400;

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
    /// 5E3720 copies an already-formatted narrow string; it stays bytes,
    /// not UTF-8.
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

    /// An Options message's fields, read by [`LanMessage::payload`].
    pub(super) fn setup_options_fields(&self) -> LanOptionsFields<'_> {
        LanOptionsFields {
            text: self.setup_body_text(),
            color: self.u32(0x1bf),
            game_name_hash: self.u32(0x1c3),
        }
    }

    /// The narrow body text Options and PlayerChoices carry, up to its NUL.
    pub(super) fn setup_body_text(&self) -> &[u8] {
        c_string(&self.bytes[BODY_OFFSET..BODY_OFFSET + OPTIONS_CAPACITY])
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

    /// 5DD57B..5DD5D6. The previously-published options own the seed.
    pub fn go(max_ahead: i32) -> Self {
        let mut message = Self::zeroed(LanMessageKind::Go);
        message.put_u32(BODY_OFFSET, max_ahead as u32);
        message
    }

    /// A GO message's MaxAhead, read by [`LanMessage::payload`].
    pub(super) fn setup_go_max_ahead(&self) -> i32 {
        self.u32(BODY_OFFSET) as i32
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

#[cfg(all(test, tests_net))]
#[path = "setup_tests.rs"]
mod tests;
