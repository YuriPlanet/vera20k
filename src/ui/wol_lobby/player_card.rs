//! VERA's player card in the online lobby (no gamemd dialog): a click on a
//! name in the modern player list opens it over the lobby, on the online
//! Create Game box's art. It shows what the lobby knows of the player and
//! offers Message, Add or Remove Buddy, Full Profile and Close. The classic
//! page keeps gamemd's click, which selects the name for private chat.

use crate::ui::shell::button::ShellButtonInteraction;

/// The card's buttons, in VERA's own control range.
pub(crate) const CARD_MESSAGE: u16 = 0x7FF2;
pub(crate) const CARD_BUDDY: u16 = 0x7FF3;
pub(crate) const CARD_PROFILE: u16 = 0x7FF4;
pub(crate) const CARD_CLOSE: u16 = 0x7FF5;

/// What a released card button asks of the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolCardAction {
    /// The page dialog `0xCF`, addressed to the player.
    Message,
    /// Add the player to the buddy list, or remove a buddy.
    Buddy,
    /// The player's profile on vera20k.org, in the browser.
    Profile,
    Close,
}

impl WolCardAction {
    pub(crate) fn of(control: u16) -> Option<Self> {
        Some(match control {
            CARD_MESSAGE => Self::Message,
            CARD_BUDDY => Self::Buddy,
            CARD_PROFILE => Self::Profile,
            CARD_CLOSE => Self::Close,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WolPlayerCard {
    /// The player's name as the list shows it.
    pub(crate) name: String,
    pub(crate) buttons: ShellButtonInteraction<u16>,
}

impl WolPlayerCard {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            buttons: ShellButtonInteraction::default(),
        }
    }
}
