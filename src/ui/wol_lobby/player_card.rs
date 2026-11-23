//! VERA's player card in the online lobby (no gamemd dialog): a click on a
//! name in the modern player list opens it over the lobby, on the online
//! Create Game box's art. It shows what the lobby knows of the player and
//! offers Message, Add or Remove Buddy, Full Profile and Close. The classic
//! page keeps gamemd's click, which selects the name for private chat.
//! Your own card offers Change Country in Message's place, which drops the
//! country list of `0xFE` below the Country row: a combo in the card's
//! dialog whose face never shows.

use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

use super::WolLobbyState;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::PLAYER_CARD_ROWS;
use crate::ui::shell::page;
use crate::ui::wol_persona::LocaleRow;
use vera20k_gui::shell::combobox::ComboPopup;

/// The card's buttons, in VERA's own control range, on a modal dialog in
/// the lobby's widget tree.
pub(crate) const CARD_MESSAGE: u16 = 0x7FF2;
pub(crate) const CARD_BUDDY: u16 = 0x7FF3;
pub(crate) const CARD_PROFILE: u16 = 0x7FF4;
pub(crate) const CARD_CLOSE: u16 = 0x7FF5;
/// Your own card's Change Country, in Message's place.
pub(crate) const CARD_COUNTRY: u16 = 0x7FF6;
/// The country list's combo, on the Country row's value.
pub(crate) const CARD_COUNTRIES: u16 = 0x7FF7;

/// The card's rows, which give their help under the pointer: statics from
/// here on, one per row of the card's layout.
pub(crate) const CARD_ROWS: u16 = 0x7FE0;

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
    /// The country list's rows ([`crate::ui::wol_persona::locale_rows`]).
    pub(crate) countries: Vec<LocaleRow>,
}

/// The card dialog's widgets: the four buttons' places and the country combo.
pub(super) fn card_spec() -> NodeSpec {
    let buttons = [
        CARD_MESSAGE,
        CARD_COUNTRY,
        CARD_BUDDY,
        CARD_PROFILE,
        CARD_CLOSE,
    ]
    .map(|id| {
        NodeSpec::new(NodeKind::Button)
            .id(id.into())
            .with(Prop::Cancel, Value::Bool(id == CARD_CLOSE))
    });
    let countries = NodeSpec::new(NodeKind::Combo)
        .id(CARD_COUNTRIES.into())
        .with(Prop::VisibleRows, Value::Int(CARD_COUNTRY_ROWS as i32))
        .with(Prop::Enabled, Value::Bool(false));
    // Hover only: the host paints them and gives their help.
    let rows = (0..PLAYER_CARD_ROWS as u16).map(|row| {
        NodeSpec::new(NodeKind::Text)
            .id((CARD_ROWS + row).into())
            .tooltip("")
    });
    NodeSpec::new(NodeKind::Dialog)
        .children(buttons)
        .child(countries)
        .children(rows)
}

impl WolLobbyState {
    /// The card's country combo.
    fn countries_node(&self) -> Option<vera20k_gui::NodeId> {
        let (_, root) = self.card_dialog?;
        self.gui.find_id(root, CARD_COUNTRIES.into())
    }

    /// The open country list.
    pub(crate) fn countries_drop_down(&self) -> Option<vera20k_gui::ComboDropDown> {
        self.gui.combo_drop_down(self.countries_node()?)
    }

    pub(crate) fn countries_open(&self) -> bool {
        self.countries_drop_down().is_some()
    }

    /// Opens the country list on `rows` with the row of `locale` lit and,
    /// unlike gamemd's combos, scrolled to it where the list allows; its
    /// face never shows, so it hangs from `value`, the Country row's value.
    pub(crate) fn open_countries(&mut self, rows: Vec<LocaleRow>, locale: i32, value: RectPx) {
        let Some(node) = self.countries_node() else {
            return;
        };
        let Some(card) = self.card.as_mut() else {
            return;
        };
        let selected = rows.iter().position(|row| row.locale == locale);
        let top = selected
            .unwrap_or(0)
            .min(rows.len().saturating_sub(CARD_COUNTRY_ROWS));
        let items = rows.iter().map(|row| row.text.clone()).collect();
        card.countries = rows;
        // The list takes the pointer: no card button stays lit.
        self.release_buttons();
        let selected = Value::Int(selected.map_or(-1, |row| row as i32));
        let _ = self.gui.set(node, Prop::Items, Value::Items(items));
        let _ = self.gui.set(node, Prop::Selected, selected);
        self.place_countries(value);
        let _ = self.gui.open_popup(node, top);
    }

    /// Hangs the country list below `value`, the Country row's value.
    pub(crate) fn place_countries(&mut self, value: RectPx) {
        let (Some(card), Some(node)) = (self.card.as_ref(), self.countries_node()) else {
            return;
        };
        let rows = card.countries.len();
        let popup = ComboPopup::new(value, self.scale, rows, CARD_COUNTRY_ROWS, 0);
        let _ = self.gui.set(node, Prop::PopupRect, Value::Rect(popup.rect));
    }

    /// The locale of the country list's row `row`.
    pub(crate) fn country_locale(&self, row: usize) -> Option<i32> {
        Some(self.card.as_ref()?.countries.get(row)?.locale)
    }

    /// Puts the card on `dialog` (a press outside it closes the card), its
    /// rows on `rows` and its buttons on their rects (`None`: hidden), and
    /// sets which take a press.
    pub(crate) fn sync_card(
        &mut self,
        dialog: RectPx,
        rows: &[RectPx],
        place: impl Fn(u16) -> Option<(RectPx, bool)>,
    ) {
        let Some((_, root)) = self.card_dialog else {
            return;
        };
        let _ = self.gui.set(root, Prop::Rect, Value::Rect(dialog));
        let buttons = [
            CARD_MESSAGE,
            CARD_COUNTRY,
            CARD_BUDDY,
            CARD_PROFILE,
            CARD_CLOSE,
        ];
        page::sync_rects(&mut self.gui, root, |id| match id {
            // The combo keeps its list where it opened.
            CARD_COUNTRIES => Some(RectPx::default()),
            id if (CARD_ROWS..CARD_ROWS + PLAYER_CARD_ROWS as u16).contains(&id) => {
                rows.get(usize::from(id - CARD_ROWS)).copied()
            }
            id => place(id).map(|(rect, _)| rect),
        });
        for id in buttons {
            let enabled = place(id).is_none_or(|(_, enabled)| enabled);
            page::set(&mut self.gui, root, id, Prop::Enabled, Value::Bool(enabled));
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::super::*;
    use crate::ui::wol_persona::{LOCALE_COUNT, locale_rows};

    /// Your card's list lights your country and starts at it, short of the
    /// last page; without one it starts at Unknown. A press on a row picks
    /// it and closes the list.
    #[test]
    fn the_country_list_opens_at_your_country() {
        let rows = || locale_rows((0..LOCALE_COUNT).map(|locale| (locale, format!("{locale:02}"))));
        let mut lobby = WolLobbyState::new(false, true);
        lobby.replace_players(vec![WolPlayerRow {
            name: "Boris".into(),
            icon: None,
            clan: None,
            rank: None,
            score_icons: Vec::new(),
        }]);
        lobby.press_player(0);
        let value = RectPx::new(100, 100, 150, 12);
        for (locale, lit, top) in [(0, 0, 0), (17, 17, 17), (37, 37, 31)] {
            lobby.open_countries(rows(), locale, value);
            let list = lobby.countries_drop_down().expect("open");
            assert_eq!(list.hot_row, Some(lit), "{locale}");
            assert_eq!(list.popup.first, top, "{locale}");
        }
        let list = lobby.countries_drop_down().unwrap();
        let row = list.popup.row_rect(list.popup.first).unwrap();
        let effects = lobby.pointer(row.x + 1, row.y + 1, Some(true));
        assert!(
            effects.contains(&WolLobbyEffect::CardCountry(31)),
            "{effects:?}"
        );
        assert!(!lobby.countries_open());
    }
}
