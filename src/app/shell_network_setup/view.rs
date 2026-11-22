//! Read-only BC/BD control projection. Native5EB060 builds Player entries;
//!5EA180/5E9E70 gate their siblings. MatchSession is the sole setup authority.

use crate::app::AppState;
use crate::app::frontend::skirmish_shell_render::{
    combo_item_label, flag_pcx_for_side_item_data, side_item_data_for_country,
};
use crate::app::match_runtime::session::{MatchSession, NetworkVacancy};
use crate::app::shell_wol::lobby::player_badge;
use crate::skirmish_launch::{AiDifficulty, LaunchStartPosition, LaunchTeam};
use crate::skirmish_modes::mode_by_id;
use crate::ui::multiplayer_setup::*;
use crate::ui::skirmish_shell::{
    self, SkirmishComboItem, SkirmishCountryChoice, SkirmishTrackbarId,
};
use vera20k_gui::shell::trackbar::TrackbarRange;

fn label(state: &AppState, key: &str) -> String {
    state.csf_label(key, key)
}
pub(crate) fn choice(state: &AppState, item: SkirmishComboItem) -> SetupChoice {
    let value = match item {
        SkirmishComboItem::Country(SkirmishCountryChoice::Random) => -2,
        SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => {
            side_item_data_for_country(country)
        }
        SkirmishComboItem::ColorSentinel(value) => value,
        SkirmishComboItem::Color(value) => value as i32,
        SkirmishComboItem::Start(start) => start.shell_value(),
        SkirmishComboItem::Team(value) => value,
        SkirmishComboItem::AiType(value) => value.item_data(),
        // Skirmish's own lists, by the room's values for them.
        SkirmishComboItem::TeamTool { tool, .. } => tool.code(),
        SkirmishComboItem::TeamHeading => -1,
        SkirmishComboItem::SeatAction { action, .. } => action.code(),
    };
    SetupChoice {
        value,
        text: combo_item_label(state, item),
        swatch: if let SkirmishComboItem::Color(index) = item {
            Some(index)
        } else {
            None
        },
        greyed: !item.pickable(),
    }
}

/// A team's letter as the Team list shows it.
pub(super) fn team_letter(state: &AppState, team: u8) -> String {
    combo_item_label(state, SkirmishComboItem::Team(i32::from(team)))
}

/// A row of VERA's room list: a seat or spectator row, a header over the
/// rows of its group and how many players they hold, or an empty line a
/// team's box keeps ([`TEAM_BOX_ROWS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listed {
    Row(u8),
    Header(SetupGroup, usize),
    Blank,
}

/// The lines a team's box in VERA's room keeps, empty or not, so a player
/// or AI coming in fills one instead of moving the boxes under it.
const TEAM_BOX_ROWS: usize = 5;

/// VERA's online room: the help under the pointer on the name or badge of
/// a player with a badge, as the lobby's player list gives it
/// ([`crate::app::shell_wol::lobby::badge_help`]).
pub(crate) fn player_help(state: &AppState, view: &SetupView, control: u16) -> Option<String> {
    let (SetupColumn::Player, row) = SetupColumn::decode(control)? else {
        return None;
    };
    let badge = view.row_icons.get(row)?.as_ref()?.badge?;
    let row = view.combo(control)?.row;
    let launch = state.match_state.session.network_setup()?;
    let human = launch
        .humans
        .iter()
        .find(|human| human.raw_slot_index == Some(row))?;
    crate::app::shell_wol::lobby::badge_help(state, &human.name, badge)
}

/// Whether the local player could press Accept (`SetupView::accept_enabled`).
pub(super) fn accept_enabled(state: &AppState) -> bool {
    let session = &state.match_state.session;
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.accept_eligible)
        && session
            .network_setup()
            .is_some_and(|launch| !session.network_player_ready(launch.local_player_index))
        && crate::app::shell_wol::local_choices_agreed(state)
}

/// The room's map in the scenario catalog, when this player shows its own
/// copy of it. A guest shows its own copy only when it verified the map, and
/// on WOL only from a mixfile (0x005E74E0); otherwise its preview is the
/// host's Preview.bin.
pub(super) fn local_map_index(state: &AppState) -> Option<usize> {
    let session = &state.match_state.session;
    let protocol = state.frontend.network_setup_protocol.as_ref()?;
    if !session.host() && !protocol.local_preview() {
        return None;
    }
    let file = session.network_setup()?.selected_map_file.as_deref()?;
    state
        .frontend
        .scenario_catalog
        .shell_maps()
        .iter()
        .position(|map| map.file_name.eq_ignore_ascii_case(file))
}

/// The room's rows and the page of them showing: what [`view`] fills in
/// and [`crate::app::input::multiplayer_setup::layout`] places.
pub(crate) struct RoomListing {
    capacity: usize,
    observer_row: Option<usize>,
    visible_limit: usize,
    several: bool,
    add_slot: Option<u8>,
    pub(crate) grouped: bool,
    listed: Vec<Listed>,
    row_count: usize,
    /// The first row of each page.
    pages: Vec<usize>,
    first_row: usize,
    /// The rows (headers too) on the page showing.
    page_len: usize,
    /// In VERA's room, where each of them stands ([`roster_page`]).
    pub(crate) places: Vec<RosterPlace>,
    /// In VERA's room, how many places it scrolls through and the last.
    roster_scroll: (usize, usize),
}
impl RoomListing {
    /// Index in the full listing, including for a team's pinned header.
    fn shown_index(&self, visible: usize) -> Option<usize> {
        if self.roster_scroll.0 > 0 {
            self.places.get(visible).map(|place| place.item)
        } else {
            Some(self.first_row + visible)
        }
    }
    fn row_at(&self, position: usize) -> Option<Listed> {
        if self.several {
            self.listed.get(position).copied()
        } else {
            (position < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT)
                .then_some(Listed::Row(position as u8))
        }
    }
}

/// Whether the host of a VERA room has the team bar's Room size: its Max
/// players, people who play and watch together. Quick Match's matchbot game
/// has none.
pub(crate) fn room_size_shown(state: &AppState) -> bool {
    let session = &state.match_state.session;
    session.host() && session.network_people_limit().is_some() && !super::quick_match(state)
}

pub(crate) fn room_listing(state: &AppState) -> Option<RoomListing> {
    let session = &state.match_state.session;
    let launch = session.network_setup()?;
    mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)?;
    let capacity = session
        .network_setup_capacity()?
        .clamp(0, crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32)
        as usize;
    let observer_row = session.network_observer_row().map(usize::from);
    let row_limit = session.network_row_limit()?;
    let visible_limit = if observer_row == Some(0) && (capacity as i32) < row_limit {
        capacity + 1
    } else {
        capacity
    };
    // VERA's rooms list their seats, the map's players, whether taken or
    // Open, then the spectators. A seat the host removed (Closed) stays
    // hidden until the host's Add slot row, after the seats, opens it.
    let several = session.network_several_observers() && observer_row.is_some();
    let visible_limit = if several {
        usize::from(session.network_player_row_limit())
    } else {
        visible_limit
    };
    let add_slot = (several && session.host())
        .then(|| {
            (0..visible_limit.min(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT) as u8)
                .find(|row| session.network_vacancy(*row) == Some(NetworkVacancy::Closed))
        })
        .flatten();
    // In a mode with teams, VERA's rooms group their rows under headers:
    // with Teams set, one for each of its teams.
    let grouped = several && session.network_team_room() && session.campaign_room().is_none();
    let listed = if several {
        listed_rows(
            session,
            visible_limit,
            add_slot,
            grouped,
            session.network_teams(),
        )
    } else {
        Vec::new()
    };
    let row_count = if several {
        listed.len()
    } else {
        visible_limit
            .max(observer_row.map_or(0, |row| row + 1))
            .min(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT)
    };
    let mut listing = RoomListing {
        capacity,
        observer_row,
        visible_limit,
        several,
        add_slot,
        grouped,
        listed,
        row_count,
        pages: Vec::new(),
        first_row: 0,
        page_len: 0,
        places: Vec::new(),
        roster_scroll: (0, 0),
    };
    let dialog = super::dialog(state);
    let ui = state.frontend.multiplayer_setup.as_ref();
    if dialog.modern {
        let items: Vec<RosterItem> = (0..row_count)
            .map(|position| match listing.row_at(position) {
                Some(Listed::Header(group, _)) => RosterItem::Header(group),
                _ => RosterItem::Row,
            })
            .collect();
        // A room with teams, or with the host's Room size, keeps its team
        // bar under the roster.
        let frame = crate::app::input::multiplayer_setup::roster_frame(
            state,
            grouped,
            room_size_shown(state),
        );
        let scroll = roster_scroll(&items, frame);
        // Its scrollbar node keeps the place (none before the room's dialog
        // is built), shown within the places the roster has now.
        let controller = &state.frontend.shell_controller;
        let top = match controller.prop(dialog.page().dialog, ROSTER_SCROLL, vera20k_gui::Prop::Top)
        {
            Some(vera20k_gui::Value::Int(top)) => usize::try_from(*top).unwrap_or(0),
            _ => 0,
        };
        listing.first_row = top.min(scroll.max_top);
        let top = scroll.stops.get(listing.first_row).copied().unwrap_or(0);
        listing.places = roster_page(&items, top, frame);
        listing.page_len = listing.places.len();
        listing.pages = vec![0];
        listing.roster_scroll = (scroll.stops.len(), scroll.max_top);
    } else {
        let rows = dialog.rows_per_page();
        listing.pages = (0..row_count.max(1)).step_by(rows).collect();
        listing.first_row = ui.map_or(0, |ui| ui.first_row(&listing.pages));
        listing.page_len = rows.min(row_count.saturating_sub(listing.first_row));
    }
    Some(listing)
}

pub(crate) fn view(state: &AppState) -> Option<SetupView> {
    let session = &state.match_state.session;
    let launch = session.network_setup()?;
    let protocol = state.frontend.network_setup_protocol.as_ref()?;
    let mode = mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)?;
    let host = session.host();
    let host_name = protocol.wol_host.as_deref();
    let local = launch.humans.get(launch.local_player_index)?;
    let room = session.network_room();
    let dialog = super::dialog(state);
    let campaign = session.campaign_room().is_some();
    // 0x4E5C5B..0x4E5C90, 0x4E5FD5..0x4E6004: a WOL clan room hides every
    // team combo and the team header 0x794.
    let clan_room = dialog.wol() && room.clan;
    let map_index = (!campaign).then(|| local_map_index(state)).flatten();
    let listing = room_listing(state)?;
    let RoomListing {
        capacity,
        observer_row,
        visible_limit,
        several,
        add_slot,
        grouped,
        row_count,
        first_row,
        ..
    } = listing;
    let cooperative = !campaign && mode.class.uses_cooperative_setup();
    let coop = cooperative.then(|| {
        state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_setup_view()
    });
    let seats = 0..visible_limit.min(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT) as u8;
    let removed = |row: &u8| session.network_vacancy(*row) == Some(NetworkVacancy::Closed);
    let may_remove = seats.filter(|row| !removed(row)).count() > super::team_menus::KEPT_SEATS;
    // AI play in a VERA room while people watch too, unlike in gamemd's
    // (`0x5EC9F0`'s TXT_NO_AI_WITH_OBSERVER).
    let ai_allowed = !campaign && mode.class.policy().lists_in_skirmish;
    let teams = (!campaign).then(|| session.network_teams()).flatten();
    // With Teams Off a room with teams is free-for-all and nobody is on a
    // team, so, as in a clan room, it has no Team lists.
    let no_team_lists = clan_room || grouped && teams.is_none();
    // The local player's Join and Watch on the headers: a seated player
    // joins another team, a guest watching takes a free seat on any, or
    // with none free joins the queue for the next.
    let starting = super::start_phase(state).in_progress();
    let placed = local.raw_slot_index.is_some();
    let seated = placed && !local.is_observer();
    let seat_free = !session.network_no_open_player_row();
    let watch_free = session.network_free_row(true).is_some();
    let queued = session
        .network_queue_place(launch.local_player_index)
        .is_some();
    let queue = !seated && !seat_free && session.network_queue_open();
    let ai_seat = (host && ai_allowed)
        .then(|| session.network_ai_seat())
        .flatten();
    // VERA's roster holds the rows its page fits; a native page always has
    // its eight row controls, hidden past the room's rows.
    let rows = if dialog.modern {
        listing.page_len
    } else {
        dialog.rows_per_page()
    };
    let stock_mode_label = label(state, &mode.ui_name_key);
    let mut combos = super::campaign_room::setup_selectors(
        &protocol.campaign_catalog,
        session.campaign_room(),
        host,
        &stock_mode_label,
    );
    let mut row_icons = std::array::from_fn(|_| None);
    let mut headers = std::array::from_fn(|_| None);
    for visible_row in 0..rows {
        let Some(index) = listing.shown_index(visible_row) else {
            break;
        };
        let row = match listing.row_at(index) {
            None => break,
            Some(Listed::Blank) => continue,
            Some(Listed::Row(row)) => row,
            Some(Listed::Header(group, count)) => {
                let fill = |key: &str| crate::i18n::tr_fill(key, &[("count", &count)]);
                let joins = |team: LaunchTeam| {
                    placed
                        && if seated {
                            local.slot.team != team
                        } else {
                            true
                        }
                };
                // With no seat free, a spectator's Join queues it, once.
                let join = || {
                    if queue {
                        (crate::i18n::tr("VERA:JoinQueue"), !queued)
                    } else {
                        (crate::i18n::tr("VERA:Join"), seated || seat_free)
                    }
                };
                let (caption, button) = match group {
                    SetupGroup::Team(team) => (
                        crate::i18n::tr_fill(
                            "VERA:TeamGroup",
                            &[("letter", &team_letter(state, team))],
                        ),
                        joins(LaunchTeam::Team(team)).then(join),
                    ),
                    // The one box of a room with Teams Off has no caption;
                    // a spectator's Join seats it there.
                    SetupGroup::Players => (String::new(), (placed && !seated).then(join)),
                    // Only an older host seats players on no team.
                    SetupGroup::NoTeam => (fill("VERA:NoTeamGroup"), None),
                    SetupGroup::ClosedSeats => (fill("VERA:ClosedSeatsGroup"), None),
                    SetupGroup::Spectators => (
                        fill("VERA:SpectatorsGroup"),
                        if queued {
                            Some((crate::i18n::tr("VERA:LeaveQueue"), true))
                        } else {
                            (seated && watch_free).then(|| (crate::i18n::tr("VERA:Watch"), true))
                        },
                    ),
                };
                // Team members follow their header in the full listing; the
                // visible page may begin or end halfway through them.
                let member_colors = if matches!(group, SetupGroup::Team(_)) {
                    listing
                        .listed
                        .iter()
                        .skip(index + 1)
                        .take(count)
                        .filter_map(|item| {
                            let Listed::Row(row) = *item else { return None };
                            let slot = session.network_row_choices(row)?;
                            (!slot.color_random).then_some(slot.color_index)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                headers[visible_row] = Some(SetupHeader {
                    group,
                    member_colors,
                    caption,
                    join_enabled: button.as_ref().is_some_and(|(_, enabled)| *enabled) && !starting,
                    join: button.map(|(label, _)| label),
                });
                // The host's Add AI in a team's box or the one box fills
                // the room's AI seat, past the map's own where it shares
                // bases.
                let room = matches!(group, SetupGroup::Team(_) | SetupGroup::Players);
                if let (true, Some(seat)) = (room, ai_seat) {
                    let item = |value: i32, text: String| SetupChoice {
                        value,
                        text,
                        swatch: None,
                        greyed: false,
                    };
                    combos.push(SetupCombo {
                        id: team_add_ai(visible_row),
                        row: seat,
                        enabled: !starting,
                        visible: true,
                        selected: Some(0),
                        choices: vec![
                            item(0, crate::i18n::tr("VERA:AddAi")),
                            item(4, label(state, "GUI:AIEasy")),
                            item(5, label(state, "GUI:AINormal")),
                            item(6, label(state, "GUI:AIHard")),
                        ],
                        list_w: 0,
                    });
                }
                continue;
            }
        };
        let human = launch
            .humans
            .iter()
            .enumerate()
            .find(|(_, human)| human.raw_slot_index == Some(row));
        let ai = (!campaign)
            .then(|| session.network_display_ai(row))
            .flatten();
        let vacancy = session.network_vacancy(row);
        let is_local = local.raw_slot_index == Some(row);
        let observer = human.is_some_and(|(_, human)| human.is_observer())
            || vacancy == Some(NetworkVacancy::OpenObserver);
        let visible =
            several || usize::from(row) < visible_limit || observer_row == Some(usize::from(row));
        // A VERA room's Open, Add slot and Waiting rows show only their
        // Player combo.
        let adding = Some(row) == add_slot;
        let bare = several && human.is_none() && ai.is_none();
        // VERA's room accepts for its guest (`service::auto_accept`), so the
        // guest's row stays open: the host's next options take it back.
        let accepted = !dialog.modern && session.network_player_ready(launch.local_player_index);
        let editable = ((is_local && (host || !accepted)) || host && ai.is_some()) && !observer;
        // A VERA room's guest picks Observer in its Side list to watch while
        // a spectator row is open, and a side to play while a player row is.
        let switching = several && is_local && !host && !accepted;
        // VERA's host sets a seated guest's side, colour, start and team
        // (`service::place_player`).
        let host_moves = several && host && !is_local && human.is_some() && !observer;
        let choices = session.network_row_choices(row)?;
        let country = choices.country_shell_value(observer);
        let color = choices.color_shell_value();
        let start = choices.start_position;
        let team = choices.team.shell_value();
        let slot_value = if human.is_some() {
            0
        } else if let Some(ai) = ai {
            match ai {
                AiDifficulty::Easy => 4,
                AiDifficulty::Normal => 5,
                AiDifficulty::Hard => 6,
            }
        } else {
            match vacancy.unwrap_or(NetworkVacancy::Closed) {
                NetworkVacancy::Open => 2,
                NetworkVacancy::Closed => 3,
                NetworkVacancy::OpenObserver => 7,
                NetworkVacancy::Waiting => 1,
            }
        };
        // A spectator in the join queue shows its place before its name.
        let slot_label = human
            .map(|(index, human)| match session.network_queue_place(index) {
                Some(place) => crate::i18n::tr_fill(
                    "VERA:QueuedName",
                    &[("place", &place), ("name", &human.name)],
                ),
                None => human.name.clone(),
            })
            .unwrap_or_else(|| {
                label(
                    state,
                    match slot_value {
                        2 => "GUI:Open",
                        3 => "GUI:Closed",
                        4 => "GUI:AIEasy",
                        5 => "GUI:AINormal",
                        6 => "GUI:AIHard",
                        7 => "GUI:OpenObserver",
                        _ => "GUI:Waiting",
                    },
                )
            });
        let ai_choices = [(4, "GUI:AIEasy"), (5, "GUI:AINormal"), (6, "GUI:AIHard")];
        let mut players = Vec::new();
        if several {
            // VERA's host kicks a person to an Open seat, puts an AI in an
            // Open seat or retunes one, removes a seat (3, Closed), and
            // opens a removed one from the Add slot row.
            let item = |value: i32, text: String| SetupChoice {
                value,
                text,
                swatch: None,
                greyed: false,
            };
            let seat = adding || ai.is_some() || vacancy == Some(NetworkVacancy::Open);
            if adding {
                players.push(item(3, crate::i18n::tr("VERA:AddSlot")));
            }
            if !(host && ai.is_some()) {
                players.push(item(
                    if adding { 2 } else { slot_value },
                    if adding {
                        label(state, "GUI:Open")
                    } else {
                        slot_label.clone()
                    },
                ));
            }
            if host && seat && (ai_allowed || ai.is_some()) {
                for (value, key) in ai_choices {
                    players.push(item(value, label(state, key)));
                }
            }
            if host && ai.is_some() {
                players.push(item(2, label(state, "GUI:Open")));
            }
            if host && human.is_some() && !is_local {
                // VERA's host seats a spectator, or moves a player to the
                // spectators, as their own Side list would.
                if observer && seat_free {
                    players.push(item(GIVE_SEAT, crate::i18n::tr("VERA:GiveSeat")));
                }
                if !observer && watch_free {
                    players.push(item(
                        MOVE_TO_SPECTATORS,
                        crate::i18n::tr("VERA:MoveToSpectators"),
                    ));
                }
                players.push(item(2, crate::i18n::tr("VERA:Kick")));
            }
            if host
                && !adding
                && (ai.is_some() || vacancy == Some(NetworkVacancy::Open))
                && may_remove
            {
                players.push(item(3, crate::i18n::tr("VERA:RemoveSlot")));
            }
        } else {
            if human.is_some() || !host {
                players.push(SetupChoice {
                    value: slot_value,
                    text: slot_label.clone(),
                    swatch: None,
                    greyed: false,
                });
            }
            if host && !is_local {
                let observer_only = observer_row == Some(usize::from(row))
                    && usize::from(row) > capacity
                    && ai.is_none()
                    && vacancy != Some(NetworkVacancy::Open);
                if !observer_only {
                    players.push(SetupChoice {
                        value: 2,
                        text: label(state, "GUI:Open"),
                        swatch: None,
                        greyed: false,
                    });
                }
                if observer_row == Some(usize::from(row)) {
                    players.push(SetupChoice {
                        value: 7,
                        text: label(state, "GUI:OpenObserver"),
                        swatch: None,
                        greyed: false,
                    });
                }
                players.push(SetupChoice {
                    value: 3,
                    text: label(state, "GUI:Closed"),
                    swatch: None,
                    greyed: false,
                });
                //5EB060 asks the same vtable+40 used by the Skirmish mode list.
                if mode.class.policy().lists_in_skirmish && !observer_only {
                    for (value, key) in ai_choices {
                        players.push(SetupChoice {
                            value,
                            text: label(state, key),
                            swatch: None,
                            greyed: false,
                        });
                    }
                }
            }
        }
        let selected = players.iter().position(|item| item.value == slot_value);
        combos.push(SetupCombo {
            id: SetupColumn::Player.control(visible_row),
            row,
            enabled: host || is_local,
            visible,
            selected,
            choices: players,
            list_w: 0,
        });
        if campaign {
            if let Some((_, human)) = human.filter(|(_, human)| !human.is_observer()) {
                let (choices, selected) = super::campaign_room::role_choices(state, &human.name);
                combos.push(SetupCombo {
                    id: SetupColumn::Country.control(visible_row),
                    row,
                    enabled: host && choices.iter().any(|choice| choice.value >= 0),
                    visible,
                    selected,
                    choices,
                    list_w: 0,
                });
            }
            continue;
        }
        // VERA's room bans leave their countries off the list.
        let banned = super::bans::banned_countries(state, launch.options.bans);
        let mut countries: Vec<_> = state
            .rules()
            .map(skirmish_shell::country_choices)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                !matches!(item, SkirmishComboItem::Country(SkirmishCountryChoice::Country(country))
                    if banned.contains(country))
            })
            .map(|item| choice(state, item))
            .collect();
        if let Some(player_countries) = coop.as_ref().and_then(|coop| coop.player_countries) {
            //5C23B0 rebuilds country entries in the campaign's authored order.
            let mut allowed = Vec::new();
            if player_countries.len() > 1 {
                allowed.push(SetupChoice {
                    value: -2,
                    text: label(state, "GUI:Random"),
                    swatch: None,
                    greyed: false,
                });
            }
            for country in state
                .frontend
                .multiplayer_shell_runtime
                .cooperative_player_countries()
                .unwrap_or_default()
            {
                allowed.push(choice(
                    state,
                    SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)),
                ));
            }
            countries = allowed;
        }
        // A spectator in a room with teams plays by joining a team.
        let sides = if switching && observer {
            !grouped && !session.network_no_open_player_row()
        } else {
            !observer
        };
        if !sides {
            countries.clear();
        }
        if observer || switching && session.network_free_row(true).is_some() {
            countries.push(SetupChoice {
                value: -3,
                text: label(state, "GUI:Observer"),
                swatch: None,
                greyed: false,
            });
        }
        let color_claims = launch
            .humans
            .iter()
            .filter(|human| {
                //5E9C00 checks visibleA8DA90 pointers, not the source vector.
                // A kicked peer remains admitted until type10 but has no row.
                human.raw_slot_index.is_some()
                    && human.raw_slot_index != Some(row)
                    && !human.slot.color_random
                    && !human.is_observer()
            })
            .map(|human| usize::from(human.slot.color_index))
            .chain(
                launch
                    .opponents
                    .iter()
                    .filter(|ai| ai.raw_slot_index != row && !ai.color_random)
                    .map(|ai| usize::from(ai.color_index)),
            );
        let lobby_colors = state.lobby_colors();
        let color_claims = color_claims.filter(|_| {
            launch.humans.len() + launch.opponents.len() <= lobby_colors.lobby_color_count()
        });
        let colors = skirmish_shell::available_color_choices(
            lobby_colors,
            usize::try_from(color).ok(),
            color_claims,
        )
        .into_iter()
        .map(|item| choice(state, item))
        .collect::<Vec<_>>();
        // VERA shared control: when the mode policy allows shared start
        // positions, a player may pick a start another player holds to join
        // that player's House. Such starts stay listed, marked "+".
        let shares = human.is_some() && mode.class.policy().shared_start_positions;
        let human_starts = launch
            .humans
            .iter()
            .filter(|human| {
                human.raw_slot_index.is_some()
                    && human.raw_slot_index != Some(row)
                    && !human.is_observer()
            })
            .map(|human| human.slot.start_position)
            .collect::<Vec<_>>();
        let start_claims = human_starts.iter().copied().filter(|_| !shares).chain(
            launch
                .opponents
                .iter()
                .filter(|ai| ai.raw_slot_index != row)
                .map(|ai| ai.start_position),
        );
        let starts = skirmish_shell::available_start_choices(Some(start), start_claims, capacity)
            .into_iter()
            .map(|item| {
                let mut choice = choice(state, item);
                if shares
                    && let SkirmishComboItem::Start(held @ LaunchStartPosition::Position(_)) = item
                    && human_starts.contains(&held)
                {
                    choice.text.push('+');
                }
                choice
            })
            .collect::<Vec<_>>();
        // With Teams set, the room's teams only. A room with teams has no
        // "no team": a player on none watches.
        let team_items = match teams {
            Some(count) => (0..count)
                .map(|team| SkirmishComboItem::Team(i32::from(team)))
                .collect(),
            None => skirmish_shell::team_choices(
                mode.must_ally || grouped,
                crate::skirmish_launch::NATIVE_TEAMS,
            ),
        };
        let team_items = team_items
            .into_iter()
            .map(|item| choice(state, item))
            .collect::<Vec<_>>();
        for (column, value, choices) in [
            (SetupColumn::Country, country, countries),
            (SetupColumn::Color, color, colors),
            (SetupColumn::Start, start.shell_value(), starts),
            (SetupColumn::Team, team, team_items),
        ] {
            let selected = choices.iter().position(|item| item.value == value);
            combos.push(SetupCombo {
                id: column.control(visible_row),
                row,
                enabled: (editable || switching && column == SetupColumn::Country || host_moves)
                    && (column != SetupColumn::Team || mode.allies_allowed),
                visible: visible
                    && !bare
                    && (column == SetupColumn::Country || !observer)
                    && !(column == SetupColumn::Team && no_team_lists),
                selected,
                choices,
                list_w: 0,
            });
        }
        if visible && !bare {
            let flag = match u16::try_from(country)
                .ok()
                .and_then(|index| state.process_assets.country_flag_surface(index))
            {
                Some(flag) => Some(flag),
                None => flag_pcx_for_side_item_data(country).map(str::to_owned),
            };
            // RESIDUAL: C2/C9's ping image (7A0E50 from 5EA3E0, 007A0EA0),
            // the latency tooltip 0078F5C0 (TXT_LATENCY_ICON_TOOLTIP_*) that
            // reads the same table, and the host's ping timer 0x14AF8 that
            // fills it are not ported: the lobby carries no per-user latency
            // to the room.
            row_icons[visible_row] = Some(SetupRowIcon {
                country: flag,
                // 0x5EA3E0: wolhost.pcx on the human row named [0xA8D610];
                // a WOL guest learns it from the user list, otherwise the
                // host is row 0.
                host: human.is_some_and(|(_, human)| match host_name {
                    Some(host) => human.name == host,
                    None => row == 0,
                }),
                accepted: ai.is_some()
                    || human.is_some_and(|(index, _)| session.network_player_ready(index)),
                // VERA's online room shows each player's badge, as the
                // lobby's player list does.
                badge: human
                    .filter(|_| dialog.modern && dialog.wol())
                    .and_then(|(_, human)| player_badge(state, &human.name)),
                local: is_local,
            });
        }
    }
    let bounds = state.frontend.skirmish_shell_state.trackbar_bounds;
    let options = &launch.options;
    let trackbars = vec![
        SetupTrackbar {
            id: SPEED,
            value: crate::ui::shell::game_speed::SpeedContext::Network
                .visual_position(options.game_speed),
            range: bounds.range(SkirmishTrackbarId::GameSpeed0x529),
            enabled: host,
            visible: true,
        },
        SetupTrackbar {
            id: CREDITS,
            value: bounds.credits_visual_position(options.starting_credits),
            range: bounds.visual_range(SkirmishTrackbarId::Credits0x511),
            enabled: host,
            visible: !campaign,
        },
        SetupTrackbar {
            id: UNITS,
            value: options.unit_count,
            range: bounds.range(SkirmishTrackbarId::UnitCount0x50c),
            enabled: host,
            visible: !campaign,
        },
        SetupTrackbar {
            id: DIFFICULTY,
            value: super::campaign_room::difficulty(state)
                .unwrap_or_else(|| coop.as_ref().map_or(0, |coop| coop.difficulty)),
            range: TrackbarRange::new(0, 2, 1),
            enabled: host,
            visible: cooperative || campaign,
        },
        // The Teams box's count: 1 is Off, then the teams.
        SetupTrackbar {
            id: TEAM_COUNT,
            value: options.teams.map_or(1, i32::from),
            range: TrackbarRange::new(1, i32::from(*crate::skirmish_launch::ROOM_TEAMS.end()), 1),
            enabled: host && grouped,
            visible: dialog.modern && grouped,
        },
        // The host's Room size beside it: its Max players, spectators too.
        SetupTrackbar {
            id: ROOM_SIZE,
            value: session
                .network_people_limit()
                .map_or(crate::ui::room_limits::PLAYERS.max, i32::from),
            range: crate::ui::room_limits::PLAYERS,
            enabled: host,
            visible: dialog.modern && room_size_shown(state),
        },
        // The Superweapon Limit, in VERA's Game Options: 1 is Off.
        SetupTrackbar {
            id: SUPERWEAPON_LIMIT,
            value: i32::from(options.host_rules.superweapon_limit.max(1)),
            range: crate::ui::skirmish_shell::game_options::SUPERWEAPON_LIMIT_RANGE,
            enabled: host,
            visible: dialog.modern && !campaign,
        },
        // VERA's Game Options also shows the room's Tech Level.
        SetupTrackbar {
            id: TECH_LEVEL,
            // The host's value, which may come from other rules or another
            // game, sits within the slider.
            value: options.tech_level.clamp(
                crate::ui::skirmish_shell::game_options::TECH_LEVEL_RANGE.min,
                crate::ui::skirmish_shell::game_options::TECH_LEVEL_RANGE.max,
            ),
            range: crate::ui::skirmish_shell::game_options::TECH_LEVEL_RANGE,
            enabled: host,
            visible: dialog.modern && !campaign,
        },
    ];
    // The host drags people, itself too, and AI between the boxes of a room
    // with teams; only people go to watch.
    let mut box_of = None;
    let drag_rows = listing
        .listed
        .iter()
        .filter(|_| host && grouped)
        .filter_map(|listed| {
            let row = match *listed {
                Listed::Header(group, _) => {
                    box_of = Some(group);
                    return None;
                }
                Listed::Row(row) => row,
                Listed::Blank => return None,
            };
            let group = box_of.filter(|group| *group != SetupGroup::ClosedSeats)?;
            let person = launch
                .humans
                .iter()
                .position(|human| human.raw_slot_index == Some(row));
            let may_watch = match person {
                Some(_) => true,
                None => session.network_display_ai(row).map(|_| false)?,
            };
            Some(DragRow {
                row,
                group,
                may_watch,
            })
        })
        .collect();
    combos.extend(super::team_menus::combos(
        state, grouped, ai_allowed, starting,
    ));
    // VERA's roster opens each list as wide as its widest text needs.
    if dialog.modern {
        let font = &state.renderer.bit_font;
        for combo in &mut combos {
            combo.list_w = roster_list_width(&combo.choices, |text| font.text_width(text));
        }
    }
    Some(SetupView {
        campaign,
        first_row,
        row_count,
        page_len: listing.page_len,
        roster_scroll: listing.roster_scroll,
        pages: listing.pages,
        places: listing.places,
        dialog,
        locked: protocol.locked,
        // C2's lockout (0x78F85E..0x78F907, 0x79132A..0x7913D1): Short Game
        // and Bases checked and disabled in a tournament or clan room.
        short_game_locked: host && dialog.wol() && room.tournament_or_clan(),
        team_header: !no_team_lists,
        accept_enabled: accept_enabled(state),
        start_requested: protocol.start_requested
            && !session.network_player_ready(launch.local_player_index),
        combos,
        trackbars,
        checks: [
            options.short_game,
            options.mcv_redeploy,
            options.crates,
            options.super_weapons,
            options.build_off_ally,
        ],
        session_checks: [
            options.bases,
            options.fog_of_war,
            options.multi_engineer,
            options.harvester_truce,
        ],
        room_checks: [
            options.bridges_destroyable,
            options.ally_change_allowed,
            options.bans.yuri,
            options.bans.france,
            options.bans.spy,
            options.bans.stolen_tech,
            options.host_rules.terrain_deformation,
            options.host_rules.building_production_queue,
        ],
        row_icons,
        headers,
        lobby_enabled: dialog.wol() && crate::app::shell_wol::lobby_visits(state),
        drag_rows,
        mode_label: if launch.selected_map_file.is_none() {
            label(state, "GUI:None")
        } else {
            stock_mode_label
        },
        map_label: super::campaign_room::stage_label(state).unwrap_or_else(|| {
            super::map_display_name(state).map_or_else(|| label(state, "GUI:None"), str::to_owned)
        }),
        selected_map_index: map_index,
        cooperative,
        cooperative_description: coop.as_ref().and_then(|coop| coop.description).map(
            |(campaign, stage)| {
                label(
                    state,
                    &format!("NAME:COOPDESC{}{}", campaign + 1, stage + 1),
                )
            },
        ),
    })
}

/// VERA's room list: its seats, the host's Add slot row, then the
/// spectators, those in the join queue first. In a mode with teams
/// (`grouped`) a header goes over each group: with Teams set (`fixed`), one
/// for each of its teams, empty or not and at least [`TEAM_BOX_ROWS`]
/// lines high, then the players on no team; with
/// Teams Off, one box of every seated player. The Add slot row and the
/// spectators follow, each under its header.
fn listed_rows(
    session: &MatchSession,
    visible_limit: usize,
    add_slot: Option<u8>,
    grouped: bool,
    fixed: Option<u8>,
) -> Vec<Listed> {
    let Some(launch) = session.network_setup() else {
        return Vec::new();
    };
    let rows = 0..crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u8;
    let watching = |row: u8| match launch
        .humans
        .iter()
        .find(|human| human.raw_slot_index == Some(row))
    {
        Some(human) => Some(human.is_observer()),
        None if session.network_display_ai(row).is_some() => Some(false),
        None => match session.network_vacancy(row) {
            Some(NetworkVacancy::Waiting) => Some(usize::from(row) >= visible_limit),
            Some(NetworkVacancy::Open) if usize::from(row) < visible_limit => Some(false),
            _ => None,
        },
    };
    let seated = rows.clone().filter(|row| watching(*row) == Some(false));
    // The spectators in the join queue come first, in its order.
    let place = |row: &u8| {
        launch
            .humans
            .iter()
            .position(|human| human.raw_slot_index == Some(*row))
            .and_then(|index| session.network_queue_place(index))
            .unwrap_or(usize::MAX)
    };
    let mut spectators: Vec<u8> = rows.filter(|row| watching(*row) == Some(true)).collect();
    spectators.sort_by_key(place);
    if !grouped {
        return seated
            .chain(add_slot)
            .chain(spectators)
            .map(Listed::Row)
            .collect();
    };
    let held = |row: &u8| {
        launch
            .humans
            .iter()
            .any(|human| human.raw_slot_index == Some(*row))
            || session.network_display_ai(*row).is_some()
    };
    let team_of = |row: &u8| {
        session
            .network_row_choices(*row)
            .map_or(LaunchTeam::None, |slot| slot.team)
    };
    // The free seats show no group: players take them by their box's Join,
    // and the host fills one with a box header's Add AI.
    let players: Vec<u8> = seated.filter(held).collect();
    let mut listed = Vec::new();
    // A header, how many players it counts, then its rows; a team's box
    // fills up to its lines with empty ones.
    let mut group = |header, count: usize, rows: Vec<u8>| {
        let lines = if matches!(header, SetupGroup::Team(_)) {
            TEAM_BOX_ROWS
        } else {
            0
        };
        let blank = lines.saturating_sub(rows.len());
        listed.push(Listed::Header(header, count));
        listed.extend(rows.into_iter().map(Listed::Row));
        listed.extend(std::iter::repeat_n(Listed::Blank, blank));
    };
    match fixed {
        // With Teams Off the room is one box, every player for themselves
        // unless they pick an ally letter in the Team list.
        None => group(SetupGroup::Players, players.len(), players),
        Some(count) => {
            for team in 0..count {
                let members: Vec<u8> = players
                    .iter()
                    .copied()
                    .filter(|row| team_of(row) == LaunchTeam::Team(team))
                    .collect();
                group(SetupGroup::Team(team), members.len(), members);
            }
            // Only an older host's room seats players on no team.
            let loners: Vec<u8> = players
                .iter()
                .copied()
                .filter(|row| !matches!(team_of(row), LaunchTeam::Team(team) if team < count))
                .collect();
            if !loners.is_empty() {
                group(SetupGroup::NoTeam, loners.len(), loners);
            }
        }
    }
    // The host's Add slot opens a closed seat again.
    if let Some(row) = add_slot {
        let closed = (0..visible_limit.min(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT)
            as u8)
            .filter(|row| session.network_vacancy(*row) == Some(NetworkVacancy::Closed))
            .count();
        group(SetupGroup::ClosedSeats, closed, vec![row]);
    }
    if !spectators.is_empty() || session.network_free_row(true).is_some() {
        group(SetupGroup::Spectators, spectators.len(), spectators);
    }
    listed
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// A VERA room on FOUR.MAP, with spectators: the host in row 0, a guest
    /// in row 1, rows 2 and 3 open.
    fn vera_room() -> MatchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        let mut guest = launch.local_player().clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        let mode = launch.mode.clone();
        let mut session = MatchSession::default();
        session.enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch, true, Some(7), true)
            .unwrap();
        session
            .select_network_map(mode, "FOUR.MAP".into(), 4)
            .unwrap();
        session.set_network_spectators(true).unwrap();
        session
    }

    /// In a mode with teams and Teams Off, every seated player lists in one
    /// box, then the spectators; the free seats show no group. A guest who
    /// starts watching moves under Spectators.
    #[test]
    fn a_room_with_teams_off_lists_its_players_in_one_box() {
        use Listed::{Header, Row};
        use SetupGroup::{Players, Spectators};
        let mut session = vera_room();
        let limit = usize::from(session.network_player_row_limit());
        assert_eq!(
            listed_rows(&session, limit, None, true, None),
            [Header(Players, 2), Row(0), Row(1), Header(Spectators, 0)]
        );
        let guest = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, guest, true, None)
            .unwrap();
        assert_eq!(
            listed_rows(&session, limit, None, true, None),
            [Header(Players, 1), Row(0), Header(Spectators, 1), Row(4)]
        );
        // A mode without teams lists its rows as before.
        assert_eq!(
            listed_rows(&session, limit, None, false, None),
            [Row(0), Row(1), Row(2), Row(3), Row(4)]
        );
    }

    /// With Teams set, each of its teams has a header, an empty one too,
    /// and room for five under it. A closed seat lists the host's Add slot
    /// row under its own header.
    #[test]
    fn a_room_with_teams_set_lists_every_team() {
        use crate::app::match_runtime::session::NetworkRowKind;
        use Listed::{Blank, Header, Row};
        use SetupGroup::{ClosedSeats, Spectators, Team};
        let mut session = vera_room();
        session.set_network_teams(Some(3)).unwrap();
        let limit = usize::from(session.network_player_row_limit());
        assert_eq!(
            listed_rows(&session, limit, None, true, session.network_teams()),
            [
                [Header(Team(0), 1), Row(0)].as_slice(),
                &[Blank; 4],
                &[Header(Team(1), 1), Row(1)],
                &[Blank; 4],
                &[Header(Team(2), 0)],
                &[Blank; 5],
                &[Header(Spectators, 0)],
            ]
            .concat()
        );
        session
            .choose_network_row_kind(3, NetworkRowKind::Closed)
            .unwrap();
        let listed = listed_rows(&session, limit, Some(3), true, session.network_teams());
        assert_eq!(
            listed[18..],
            [Header(ClosedSeats, 1), Row(3), Header(Spectators, 0)]
        );
    }

    /// The spectators in the join queue list first, in its order.
    #[test]
    fn a_room_lists_its_queued_spectators_first() {
        use crate::app::match_runtime::session::NetworkRowKind;
        let mut session = vera_room();
        let slot = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, slot.clone(), true, None)
            .unwrap();
        let mut third = session.network_setup().unwrap().humans[1].clone();
        third.name = "Third".into();
        third.raw_slot_index = Some(5);
        session.add_network_participant(third).unwrap();
        for row in 1..4 {
            session
                .choose_network_row_kind(row, NetworkRowKind::Closed)
                .unwrap();
        }
        session
            .accept_network_peer_choices(2, slot, false, None)
            .unwrap();
        let limit = usize::from(session.network_player_row_limit());
        assert_eq!(
            listed_rows(&session, limit, None, false, None),
            [Listed::Row(0), Listed::Row(5), Listed::Row(4)]
        );
    }
}
