//! VERA's player card in the online lobby (no gamemd dialog): a click on a
//! name in the modern player list opens it over the lobby, on the online
//! Create Game box's art. It shows what the lobby knows of the player and
//! offers Message, Add or Remove Buddy, Full Profile and Close. The classic
//! page keeps gamemd's click, which selects the name for private chat.
//! Your own card offers Change Country in Message's place, which drops the
//! country list of `0xFE` below the Country row.

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::ComboPopupInteraction;
use crate::ui::wol_persona::LocaleRow;

/// The card's buttons, in VERA's own control range.
pub(crate) const CARD_MESSAGE: u16 = 0x7FF2;
pub(crate) const CARD_BUDDY: u16 = 0x7FF3;
pub(crate) const CARD_PROFILE: u16 = 0x7FF4;
pub(crate) const CARD_CLOSE: u16 = 0x7FF5;
/// Your own card's Change Country, in Message's place.
pub(crate) const CARD_COUNTRY: u16 = 0x7FF6;

/// The country list's visible rows, which end a little below the card.
pub(crate) const CARD_COUNTRY_ROWS: usize = 7;

/// What a released card button asks of the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolCardAction {
    /// The page dialog `0xCF`, addressed to the player.
    Message,
    /// Your own card's country list.
    Country,
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
            CARD_COUNTRY => Self::Country,
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
    /// The open country list's rows ([`crate::ui::wol_persona::locale_rows`]).
    pub(crate) countries: Vec<LocaleRow>,
    /// The country list in the shared drop list, open while it has a control.
    pub(crate) country_list: ComboPopupInteraction<()>,
}

impl WolPlayerCard {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            buttons: ShellButtonInteraction::default(),
            countries: Vec::new(),
            country_list: ComboPopupInteraction::default(),
        }
    }

    pub(crate) fn countries_open(&self) -> bool {
        self.country_list.control.is_some()
    }

    /// Opens the country list on `rows` with the row of `locale` lit and,
    /// unlike gamemd's combos, scrolled to the top where the list allows.
    pub(crate) fn open_countries(&mut self, rows: Vec<LocaleRow>, locale: i32) {
        let selected = rows.iter().position(|row| row.locale == locale);
        let last_top = rows.len().saturating_sub(CARD_COUNTRY_ROWS);
        self.countries = rows;
        self.country_list.open((), selected);
        self.country_list.top_index = selected.unwrap_or(0).min(last_top);
        self.buttons.hovered = None;
    }

    pub(crate) fn close_countries(&mut self) {
        self.country_list.close();
        self.countries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::wol_persona::{LOCALE_COUNT, locale_rows};

    /// Your card's list lights your country and starts at it, short of the
    /// last page; without one it starts at Unknown.
    #[test]
    fn the_country_list_opens_at_your_country() {
        let rows = || locale_rows((0..LOCALE_COUNT).map(|locale| (locale, format!("{locale:02}"))));
        let mut card = WolPlayerCard::new("Boris".into());
        for (locale, lit, top) in [(0, 0, 0), (17, 17, 17), (37, 37, 31)] {
            card.open_countries(rows(), locale);
            assert!(card.countries_open());
            assert_eq!(card.country_list.hot_row, Some(lit), "{locale}");
            assert_eq!(card.country_list.top_index, top, "{locale}");
        }
        card.close_countries();
        assert!(!card.countries_open() && card.countries.is_empty());
    }
}
