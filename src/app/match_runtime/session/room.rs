//! The network room's seats and people: capacity and row limits, observers
//! and spectators, the spectator queue, VERA team rooms, seat picks, people
//! limits, and the free, open and join rows the WOL slot tests scan.

use crate::skirmish_launch::{LaunchTeam, NETWORK_PLAYER_SLOT_COUNT, SkirmishLocalSlot};

use super::{
    MatchSession, NetworkPhase, NetworkRoom, NetworkRowKind, NetworkSession, NetworkVacancy,
    SeatPick, host_extra_row, mode_default_team, row_limit,
};

impl MatchSession {
    pub(crate) fn network_setup_capacity(&self) -> Option<i32> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Setup { capacity, .. } => Some(
                self.campaign_room()
                    .map_or(*capacity, |room| i32::from(room.player_limit())),
            ),
            _ => None,
        }
    }

    /// 5EB060/5E6440 service row limit (LAN 8, WOL 6 or a tournament
    /// room's 2) of the active visit.
    pub(crate) fn network_row_limit(&self) -> Option<i32> {
        let network = self.network.as_ref()?;
        Some(
            if self
                .network_setup_capacity()
                .is_some_and(|capacity| capacity > 8)
            {
                NETWORK_PLAYER_SLOT_COUNT as i32
            } else {
                row_limit(network.return_context.session_mode(), network.room)
            },
        )
    }

    /// The WOL game channel's kind; set when its setup opens, before the
    /// first map selection.
    pub(crate) fn set_network_room(&mut self, room: NetworkRoom) {
        if let Some(network) = self.network.as_mut() {
            network.room = room;
        }
    }

    pub(crate) fn network_room(&self) -> NetworkRoom {
        self.network
            .as_ref()
            .map(|network| network.room)
            .unwrap_or_default()
    }

    pub(crate) fn network_observer_row(&self) -> Option<u8> {
        match &self.network.as_ref()?.phase {
            NetworkPhase::Setup { observer_row, .. } => *observer_row,
            _ => None,
        }
    }

    /// Whether this is one of VERA's own rooms, where observers may take
    /// any number of rows (the Setup phase's `several_observers`).
    pub(crate) fn network_several_observers(&self) -> bool {
        self.network.as_ref().is_some_and(|network| {
            matches!(
                network.phase,
                NetworkPhase::Setup {
                    several_observers: true,
                    ..
                }
            )
        })
    }

    /// The row is open for an observer or holds one.
    pub(crate) fn network_row_for_observer(&self, row: u8) -> bool {
        self.network_vacancy(row) == Some(NetworkVacancy::OpenObserver)
            || self.network_setup().is_some_and(|launch| {
                launch
                    .humans
                    .iter()
                    .any(|human| human.raw_slot_index == Some(row) && human.is_observer())
            })
    }

    /// Whether the host of a VERA room keeps a spectator row open.
    pub(crate) fn set_network_spectators(&mut self, open: bool) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host opens spectator rows");
        if let Some(NetworkSession {
            phase: NetworkPhase::Setup {
                open_spectators, ..
            },
            ..
        }) = self.network.as_mut()
        {
            *open_spectators = open;
        }
        self.keep_spectator_row()
    }

    /// The most people the host of a VERA room lets in (its Create Game
    /// choice), players and spectators together.
    pub(crate) fn set_network_people_limit(&mut self, people: u8) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host limits the room");
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    people_limit,
                    several_observers: true,
                    ..
                },
            ..
        }) = self.network.as_mut()
        else {
            anyhow::bail!("only VERA's rooms limit their people");
        };
        *people_limit = people.clamp(2, NETWORK_PLAYER_SLOT_COUNT as u8);
        Ok(())
    }

    /// VERA's join queue (`launch.options.queue`): the spectators waiting
    /// for a seat, by name, first first.
    pub(crate) fn network_queue(&self) -> &[String] {
        self.network_setup()
            .map(|launch| launch.options.queue.as_slice())
            .unwrap_or_default()
    }

    /// Where participant `index` waits in the join queue, 1 for the first.
    pub(crate) fn network_queue_place(&self, index: usize) -> Option<usize> {
        let launch = self.network_setup()?;
        let name = &launch.humans.get(index)?.name;
        let place = launch
            .options
            .queue
            .iter()
            .position(|queued| queued == name)?;
        Some(place + 1)
    }

    /// Whether this VERA room queues the spectators who ask to play while
    /// no seat is free.
    pub(crate) fn network_queue_open(&self) -> bool {
        matches!(
            self.network.as_ref().map(|network| &network.phase),
            Some(NetworkPhase::Setup {
                join_queue: true,
                ..
            })
        )
    }

    /// The participant the host's join queue seats next: its first
    /// spectator, while a seat is free. The host's game seats it as the
    /// host's Seat does (`service::move_player`).
    pub(crate) fn network_queue_next(&self) -> Option<usize> {
        if !self.host || self.network_no_open_player_row() {
            return None;
        }
        let launch = self.network_setup()?;
        launch.options.queue.iter().find_map(|name| {
            launch.humans.iter().position(|human| {
                human.name == *name && human.is_observer() && human.raw_slot_index.is_some()
            })
        })
    }

    /// The online lobby refused the join queue: the room goes on without
    /// one, and its spectators keep watching. Returns whether anyone
    /// waited.
    pub(crate) fn stop_network_queue(&mut self) -> anyhow::Result<bool> {
        anyhow::ensure!(self.host, "only the host keeps the join queue");
        if let Some(NetworkSession {
            phase: NetworkPhase::Setup { join_queue, .. },
            ..
        }) = self.network.as_mut()
        {
            *join_queue = false;
        }
        let queue = &mut self.editable_network_setup()?.options.queue;
        let waited = !queue.is_empty();
        queue.clear();
        Ok(waited)
    }

    /// A spectator asking to play while no seat is free joins the end of
    /// the join queue, once.
    pub(super) fn queue_network_spectator(&mut self, index: usize) -> anyhow::Result<()> {
        if !self.network_queue_open() {
            return Ok(());
        }
        let launch = self.editable_network_setup()?;
        let name = &launch.humans[index].name;
        if !launch.options.queue.contains(name) {
            launch.options.queue.push(name.clone());
        }
        Ok(())
    }

    /// Participant `index` leaves the join queue. Returns whether it waited
    /// there.
    pub(super) fn leave_network_queue(&mut self, index: usize) -> anyhow::Result<bool> {
        let launch = self.editable_network_setup()?;
        let name = &launch.humans[index].name;
        let waited = launch.options.queue.len();
        launch.options.queue.retain(|queued| queued != name);
        Ok(launch.options.queue.len() != waited)
    }

    /// Whether this is a VERA room whose mode policy seats players on teams
    /// (`room_teams`), with allying allowed outside a clan room. Its rows list
    /// under team headers.
    pub(crate) fn network_team_room(&self) -> bool {
        let Some(NetworkPhase::Setup {
            launch,
            several_observers: true,
            ..
        }) = self.network.as_ref().map(|network| &network.phase)
        else {
            return false;
        };
        launch.mode.allies_allowed
            && launch.mode.class.policy().room_teams
            && !self.network_room().clan
    }

    /// The Teams count of a VERA room that seats players on teams
    /// ([`Self::network_team_room`]), or `None` while Teams is Off.
    pub(crate) fn network_teams(&self) -> Option<u8> {
        self.network_team_room()
            .then(|| self.network_setup()?.options.teams)
            .flatten()
    }

    /// The host sets a VERA room's Teams: Off, or a count in
    /// [`crate::skirmish_launch::ROOM_TEAMS`]. With a count every seated
    /// player then has one of its teams. Off makes the room one box,
    /// free-for-all: every player and AI gets the mode's default team and
    /// keeps it (`admit_network_team`), and a start two of them held goes
    /// back to Random. Its bases are no longer shared, so the people seated
    /// past the map's seats watch and its AI there leave.
    pub(crate) fn set_network_teams(&mut self, teams: Option<u8>) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host sets the room's teams");
        let launch = self.editable_network_setup()?;
        let teams = teams.filter(|teams| crate::skirmish_launch::ROOM_TEAMS.contains(teams));
        let was = launch.options.teams;
        launch.options.teams = teams;
        if teams.is_none() && was.is_some() {
            let default = mode_default_team(&launch.mode);
            let humans = launch.humans.iter_mut().map(|human| &mut human.slot.team);
            for team in humans.chain(launch.opponents.iter_mut().map(|ai| &mut ai.team)) {
                *team = default;
            }
            // A start Team Starts gave a team's players together.
            let seated = |human: &&mut crate::skirmish_launch::SkirmishHumanSlot| {
                human.raw_slot_index.is_some() && !human.is_observer()
            };
            let mut starts: Vec<_> = launch
                .humans
                .iter_mut()
                .filter(seated)
                .map(|human| &mut human.slot.start_position)
                .chain(launch.opponents.iter_mut().map(|ai| &mut ai.start_position))
                .collect();
            let held: Vec<_> = starts.iter().map(|start| **start).collect();
            for start in &mut starts {
                let shared = held.iter().filter(|held| **held == **start).count() > 1;
                if shared
                    && matches!(
                        **start,
                        crate::skirmish_launch::LaunchStartPosition::Position(_)
                    )
                {
                    **start = crate::skirmish_launch::LaunchStartPosition::Auto;
                }
            }
        }
        self.keep_to_map_seats()?;
        self.keep_spectator_row()?;
        self.keep_players_on_teams().map(|_| ())
    }

    /// The team a player or AI takes in a VERA room with Teams Off: the
    /// mode's default, no team unless the mode must ally.
    pub(crate) fn network_default_team(&self) -> LaunchTeam {
        self.network_setup()
            .map_or(LaunchTeam::None, |launch| mode_default_team(&launch.mode))
    }

    /// Whether a VERA room seats more people than its map has starts: with
    /// Teams set, in a mode whose players may share a start (every mode with
    /// teams), a team's people share its bases where the map runs out of
    /// starts (`teams::team_starts`), so the room's Max players, not its
    /// map, bounds who plays and no team is ever full. Its rows past the
    /// map's seats then seat people and AI too ([`Self::network_ai_seat`]).
    pub(crate) fn network_shares_bases(&self) -> bool {
        self.network_teams().is_some()
            && self
                .network_setup()
                .is_some_and(crate::skirmish_launch::SkirmishLaunchSession::shares_bases)
    }

    /// The host keeps a VERA room's players past the map's seats where they
    /// can play: while it shares bases ([`Self::network_shares_bases`]).
    /// Once it no longer does, its AI there leave and its people there watch
    /// from those rows, in a mode that has spectators.
    pub(super) fn keep_to_map_seats(&mut self) -> anyhow::Result<()> {
        if !self.host || !self.network_several_observers() {
            return Ok(());
        }
        let shares = self.network_shares_bases();
        let seats = self.network_player_row_limit();
        let gone: Vec<u8> = self
            .network_setup()
            .into_iter()
            .filter(|_| !shares)
            .flat_map(|launch| &launch.opponents)
            .map(|ai| ai.raw_slot_index)
            .filter(|row| *row >= seats)
            .collect();
        for row in gone {
            self.set_network_row_kind(row, NetworkRowKind::Open)?;
        }
        if shares || self.network_observer_row().is_none() {
            return Ok(());
        }
        let launch = self.editable_network_setup()?;
        for human in &mut launch.humans {
            if human.raw_slot_index.is_some_and(|row| row >= seats) {
                human.set_observer(true);
            }
        }
        Ok(())
    }

    /// The players on `team`: its seated people and AI.
    pub(crate) fn network_team_players(&self, team: u8) -> usize {
        let Some(launch) = self.network_setup() else {
            return 0;
        };
        let team = LaunchTeam::Team(team);
        let people = launch.humans.iter().filter(|human| {
            human.raw_slot_index.is_some() && !human.is_observer() && human.slot.team == team
        });
        people.count() + launch.opponents.iter().filter(|ai| ai.team == team).count()
    }

    /// A guest's own seat and team pick in a VERA room with Teams set. A
    /// person plays only on a team: one asking to play on none watches
    /// instead, while a spectator row is open. No team or a team past the
    /// room's count leaves it where it is, and a spectator taking a seat
    /// then joins the team with the fewest players.
    /// With Teams Off the room is free-for-all and nobody picks a team: a
    /// pick keeps the mode's default ([`Self::network_default_team`]).
    /// The host's moves (`service::move_player`) skip this. Returns whether it
    /// changed the pick, so the host republishes the room and the guest's own
    /// row shows its place again.
    pub(crate) fn admit_network_team(
        &self,
        index: usize,
        choices: &mut SkirmishLocalSlot,
        observer: &mut bool,
    ) -> bool {
        let Some(teams) = self.network_teams() else {
            if !self.network_team_room() {
                return false;
            }
            let default = self.network_default_team();
            return std::mem::replace(&mut choices.team, default) != default;
        };
        let Some(human) = self
            .network_setup()
            .and_then(|launch| launch.humans.get(index))
        else {
            return false;
        };
        if !*observer
            && choices.team == LaunchTeam::None
            && (human.is_observer() || self.network_free_row(true).is_some())
        {
            *observer = true;
            return true;
        }
        let seated = !human.is_observer();
        let picked = choices.team;
        if !matches!(picked, LaunchTeam::Team(team) if team < teams) {
            choices.team = if seated {
                human.slot.team
            } else {
                LaunchTeam::None
            };
        }
        choices.team != picked
    }

    /// A VERA room with Teams set keeps every seated player, person or AI,
    /// on one of its teams: in row order, one on no team or past the count
    /// joins the team with the fewest players, the first on a tie. Only the
    /// host seats players. Returns whether anyone moved.
    pub(super) fn keep_players_on_teams(&mut self) -> anyhow::Result<bool> {
        if !self.host {
            return Ok(false);
        }
        let Some(teams) = self.network_teams() else {
            return Ok(false);
        };
        // A count outside the room's own range is not a room to seat: it is
        // Teams Off, and the room keeps its teams as they are.
        if !crate::skirmish_launch::ROOM_TEAMS.contains(&teams) {
            return Ok(false);
        }
        let launch = self.editable_network_setup()?;
        let on_team = |team: LaunchTeam| match team {
            LaunchTeam::Team(team) if team < teams => Some(usize::from(team)),
            _ => None,
        };
        let mut players = vec![0usize; usize::from(teams)];
        let seated = launch
            .humans
            .iter()
            .filter(|human| human.raw_slot_index.is_some() && !human.is_observer())
            .map(|human| human.slot.team);
        for team in seated
            .chain(launch.opponents.iter().map(|ai| ai.team))
            .filter_map(on_team)
        {
            players[team] += 1;
        }
        let mut moved = false;
        for row in 0..NETWORK_PLAYER_SLOT_COUNT as u8 {
            let team = if let Some(human) = launch
                .humans
                .iter_mut()
                .find(|human| human.raw_slot_index == Some(row) && !human.is_observer())
            {
                &mut human.slot.team
            } else if let Some(ai) = launch
                .opponents
                .iter_mut()
                .find(|ai| ai.raw_slot_index == row)
            {
                &mut ai.team
            } else {
                continue;
            };
            if on_team(*team).is_some() {
                continue;
            }
            let smallest = (0..players.len())
                .min_by_key(|team| players[*team])
                .expect("a room has two teams or more");
            *team = LaunchTeam::Team(smallest as u8);
            players[smallest] += 1;
            moved = true;
        }
        Ok(moved)
    }

    /// The host's Team Starts and Team Colors in a VERA room with Teams
    /// set: a start or a colour for each row named, a person's or an AI's.
    pub(crate) fn set_network_seat_picks(
        &mut self,
        picks: &[(u8, SeatPick)],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.host, "only the host gives out starts and colours");
        let launch = self.editable_network_setup()?;
        for &(row, pick) in picks {
            let (start, color, random) = if let Some(human) = launch
                .humans
                .iter_mut()
                .find(|human| human.raw_slot_index == Some(row) && !human.is_observer())
            {
                let slot = &mut human.slot;
                (
                    &mut slot.start_position,
                    &mut slot.color_index,
                    &mut slot.color_random,
                )
            } else if let Some(ai) = launch
                .opponents
                .iter_mut()
                .find(|ai| ai.raw_slot_index == row)
            {
                (
                    &mut ai.start_position,
                    &mut ai.color_index,
                    &mut ai.color_random,
                )
            } else {
                anyhow::bail!("row {row} has no player");
            };
            match pick {
                SeatPick::Start(pick) => *start = pick,
                SeatPick::Color(pick) => (*color, *random) = (pick, false),
            }
        }
        Ok(())
    }

    /// A VERA room lets no one else in: it holds its host's Max players,
    /// or its people and AI take all thirty rows.
    pub(crate) fn network_room_full(&self) -> bool {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup {
                launch,
                people_limit,
                several_observers: true,
                ..
            }) => {
                launch.humans.len() >= usize::from(*people_limit)
                    || launch.humans.len() + launch.opponents.len() >= NETWORK_PLAYER_SLOT_COUNT
            }
            _ => false,
        }
    }

    /// A VERA room's Max players, players and spectators together, which
    /// its host sets in the team bar's Room size.
    pub(crate) fn network_people_limit(&self) -> Option<u8> {
        match self.network.as_ref().map(|network| &network.phase) {
            Some(NetworkPhase::Setup {
                people_limit,
                several_observers: true,
                ..
            }) => Some(*people_limit),
            _ => None,
        }
    }

    /// Whether later arrivals may watch in a VERA room: its host allowed
    /// spectators at Create Game and its mode keeps an observer row.
    fn network_spectators_welcome(&self) -> bool {
        matches!(
            self.network.as_ref().map(|network| &network.phase),
            Some(NetworkPhase::Setup {
                observer_row: Some(_),
                open_spectators: true,
                several_observers: true,
                ..
            })
        )
    }

    /// The most people a VERA room takes, the join limit its host gives the
    /// online lobby: its Max players, and when no one else may watch, no
    /// more than the people in it and the seats a newcomer can take, an
    /// AI's among them ([`Self::free_network_seat_for_newcomer`]).
    pub(crate) fn network_people_capacity(&self) -> Option<i32> {
        let Some(NetworkPhase::Setup {
            launch,
            people_limit,
            several_observers: true,
            ..
        }) = self.network.as_ref().map(|network| &network.phase)
        else {
            return None;
        };
        let limit = i32::from(*people_limit);
        if self.network_spectators_welcome() {
            // Spectators fill the rows the players and AI leave.
            let rows = NETWORK_PLAYER_SLOT_COUNT.saturating_sub(launch.opponents.len());
            return Some(limit.min(rows as i32));
        }
        let seats = (0..self.network_player_row_limit())
            .filter(|row| {
                self.network_vacancy(*row) == Some(NetworkVacancy::Open)
                    || self.network_display_ai(*row).is_some()
            })
            .count()
            + self.network_spare_seats().count();
        Some(limit.min((launch.humans.len() + seats) as i32))
    }

    /// The seat a newcomer to a VERA room under its host's Max players gets
    /// when every seat is taken: the AI's on the last row. AI never fill a
    /// VERA room.
    pub(crate) fn network_newcomer_ai_seat(&self) -> Option<u8> {
        // In a room with teams the newcomer watches first.
        let watches = self.network_team_room() && self.network_free_row(true).is_some();
        if !self.host
            || !self.network_several_observers()
            || self.network_room_full()
            || !self.network_no_open_player_row()
            || watches
        {
            return None;
        }
        let launch = self.network_setup()?;
        launch.opponents.iter().map(|ai| ai.raw_slot_index).max()
    }

    /// Frees [`Self::network_newcomer_ai_seat`] for the newcomer as the
    /// host's Remove does ([`Self::choose_network_row_kind`]), and returns it.
    pub(crate) fn free_network_seat_for_newcomer(&mut self) -> anyhow::Result<Option<u8>> {
        let Some(row) = self.network_newcomer_ai_seat() else {
            return Ok(None);
        };
        self.choose_network_row_kind(row, NetworkRowKind::Open)?;
        Ok(Some(row))
    }

    /// The host's own Player choice for a row (`0x005EC9F0`), after which a
    /// VERA room places its open spectator row again.
    pub(crate) fn choose_network_row_kind(
        &mut self,
        row: u8,
        kind: NetworkRowKind,
    ) -> anyhow::Result<Option<crate::skirmish_launch::SkirmishHumanSlot>> {
        let removed = self.set_network_row_kind(row, kind)?;
        self.keep_spectator_row()?;
        self.keep_players_on_teams()?;
        Ok(removed)
    }

    /// A VERA room's rows after a change on its host: an Open Observer row
    /// among the players opens for a player, and past them only the first
    /// free row after the last spectator stays open, while the host keeps
    /// spectators, AI playing or not.
    pub(super) fn keep_spectator_row(&mut self) -> anyhow::Result<()> {
        let Some(NetworkSession {
            phase:
                NetworkPhase::Setup {
                    launch,
                    several_observers: true,
                    ..
                },
            ..
        }) = self.network.as_ref()
        else {
            return Ok(());
        };
        if !self.host {
            return Ok(());
        }
        let open = self.network_spectators_welcome();
        // A person or, in a room sharing bases, an AI past the map's seats.
        let held = |row: u8| {
            launch
                .humans
                .iter()
                .any(|human| human.raw_slot_index == Some(row))
                || self.network_display_ai(row).is_some()
        };
        let players = self.network_player_row_limit();
        let rows = NETWORK_PLAYER_SLOT_COUNT as u8;
        let after = (players..rows)
            .filter(|row| held(*row))
            .last()
            .map_or(players, |row| row + 1);
        let spare = (open && after < rows).then_some(after);
        for row in 0..rows {
            let kind = match self.network_vacancy(row) {
                Some(NetworkVacancy::OpenObserver) if row < players => NetworkRowKind::Open,
                Some(NetworkVacancy::Waiting) | None => continue,
                Some(_) if row < players => continue,
                Some(_) if Some(row) == spare => NetworkRowKind::OpenObserver,
                Some(_) => NetworkRowKind::Closed,
            };
            self.set_network_row_kind(row, kind)?;
        }
        Ok(())
    }

    /// The rows the host's profile keeps (`0x005ED400`). A VERA room's
    /// spectator rows are kept as the Open rows a native room hides past its
    /// players, so a later map's players find them open.
    pub(crate) fn network_remembered_vacancies(
        &self,
    ) -> [Option<NetworkVacancy>; NETWORK_PLAYER_SLOT_COUNT] {
        let spectators = if self.network_several_observers() {
            self.network_player_row_limit()
        } else {
            NETWORK_PLAYER_SLOT_COUNT as u8
        };
        std::array::from_fn(|row| match self.network_vacancy(row as u8) {
            Some(NetworkVacancy::Closed | NetworkVacancy::OpenObserver)
                if row as u8 >= spectators =>
            {
                Some(NetworkVacancy::Open)
            }
            vacancy => vacancy,
        })
    }
}

impl MatchSession {
    /// min(`[0xA8B548]`, `[0xA8BB0C]`), the row count the WOL slot tests
    /// scan. `[0xA8B548]` is written once, by `Init_Game` (`0x0052CA7C`),
    /// from Rules+0x14D0 while it still holds the constructor's 8
    /// (`0x00667331`): `[Maximums] Players` is read only by the later
    /// scenario reset (`0x00668A11`). `[0xA8BB0C]` is the options' raw
    /// capacity, 8 before any (`0x7A5890`); the WOL row limit 6 is not in it.
    /// RESIDUAL: a host's own map selection does not set the metadata yet,
    /// so a host reads 8.
    fn network_slot_test_rows(&self) -> i32 {
        if let Some(room) = self.campaign_room() {
            return i32::from(room.player_limit());
        }
        let maximum_players = if self
            .network_setup_capacity()
            .is_some_and(|capacity| capacity > 8)
        {
            NETWORK_PLAYER_SLOT_COUNT as i32
        } else {
            8
        };
        let raw = self.network_map_metadata().map_or_else(
            || {
                if maximum_players > 8 {
                    self.network_setup_capacity().unwrap_or(8)
                } else {
                    8
                }
            },
            |metadata| metadata.player_capacity,
        );
        maximum_players.min(raw)
    }

    /// The rows that admit players: the map's capacity, plus the host's
    /// observer row when the room has one more.
    ///
    /// A VERA room, and an extended room of a map with more starts than the
    /// native eight rows, seat as many players as their map has starts, and
    /// the host's own observer row is one more row beside them. A native
    /// room follows `0x005EC4A3..0x005EC4EF`: the host's observer row adds
    /// a row when the row limit was skipped or the capacity is under it (a
    /// WOL limit under six counts one more). The room list reads this same
    /// number (`shell_network_setup::view::room_listing`), so a row the room
    /// hands out is a row the list shows.
    pub(crate) fn network_player_row_limit(&self) -> u8 {
        let Some(capacity) = self.network_setup_capacity() else {
            return 0;
        };
        let allocation = if capacity > 8 || self.network_several_observers() {
            NETWORK_PLAYER_SLOT_COUNT as i32
        } else {
            8
        };
        let extra = if self.network_several_observers() || capacity > 8 {
            i32::from(self.network_observer_row() == Some(0) && capacity < allocation)
        } else {
            i32::from(self.network_host_extra_row(capacity))
        };
        capacity.saturating_add(extra).clamp(0, allocation) as u8
    }

    /// Whether native gives the room one row more for the host's observer
    /// row (`0x005EC4A3..0x005EC4EF`), for the room's mode and capacity.
    /// The host watches from row 0 (`open_host_setup`), so that row is what
    /// marks the host as the observer, for the host and for every guest the
    /// room publishes it to.
    fn network_host_extra_row(&self, capacity: i32) -> bool {
        if self.network_observer_row() != Some(0) {
            return false;
        }
        let Some(network) = self.network.as_ref() else {
            return false;
        };
        let Some(launch) = self.network_setup() else {
            return false;
        };
        host_extra_row(
            network.return_context.session_mode(),
            network.room,
            &launch.mode,
            capacity,
        )
    }

    /// `0x005E9510`: no Open player row, including the host-observer row0
    /// exception. Apply the same admitted row range as the actual allocator,
    /// a room sharing bases' spare rows too.
    pub(crate) fn network_no_open_player_row(&self) -> bool {
        self.network_setup().is_some() && self.network_free_row(false).is_none()
    }

    /// `0x005E9240`'s row for a new node: the first Open row for a player,
    /// the first open observer row for an observer, over all eight rows
    /// (all thirty in VERA's rooms, whose spectator rows follow the players).
    /// A room sharing bases then seats a player in a spare row past the
    /// map's seats ([`Self::network_spare_seats`]).
    pub(crate) fn network_free_row(&self, observer: bool) -> Option<u8> {
        if !observer {
            return self
                .network_open_seat()
                .or_else(|| self.network_spare_seats().next());
        }
        let rows = if self.network_several_observers() {
            NETWORK_PLAYER_SLOT_COUNT as u8
        } else {
            self.network_setup_capacity()
                .unwrap_or(0)
                .max(8)
                .max(
                    self.network_observer_row()
                        .map_or(0, |row| i32::from(row) + 1),
                )
                .min(NETWORK_PLAYER_SLOT_COUNT as i32) as u8
        };
        (0..rows).find(|row| self.network_vacancy(*row) == Some(NetworkVacancy::OpenObserver))
    }

    /// The first Open row among the map's seats.
    fn network_open_seat(&self) -> Option<u8> {
        (0..self.network_player_row_limit())
            .find(|row| self.network_vacancy(*row) == Some(NetworkVacancy::Open))
    }

    /// The seat the host's Add AI fills: the first Open one of the map's,
    /// then, in a room sharing bases, a spare row past them. There the AI
    /// starts on a teammate's start, beside them (`teams::team_starts`). The
    /// host's options carry an AI on every row, on LAN as online.
    pub(crate) fn network_ai_seat(&self) -> Option<u8> {
        self.network_open_seat()
            .or_else(|| self.network_spare_seats().next())
    }

    /// The rows past the map's seats that seat a player in a room sharing
    /// bases ([`Self::network_shares_bases`]), those no one holds or is
    /// joining; none in any other room.
    fn network_spare_seats(&self) -> impl Iterator<Item = u8> + '_ {
        let rows = NETWORK_PLAYER_SLOT_COUNT as u8;
        let first = if self.campaign_room().is_none() && self.network_shares_bases() {
            self.network_player_row_limit()
        } else {
            rows
        };
        (first..rows).filter(|row| {
            !matches!(
                self.network_vacancy(*row),
                None | Some(NetworkVacancy::Waiting)
            )
        })
    }

    /// A joining node's row and role: [`Self::network_free_row`] for what it
    /// asked. In a VERA room a player who finds every seat taken watches
    /// while a spectator row is open, and one asking to watch where no
    /// spectator row is open takes a free seat. In one with Teams set
    /// everyone comes in watching, while a spectator row is open, and takes a
    /// seat with a team header's Join ([`Self::admit_network_team`]); with
    /// Teams Off the room is free-for-all and seats them as gamemd does.
    pub(crate) fn network_join_row(&self, observer: bool) -> Option<(u8, bool)> {
        if self.network_room_full() {
            return None;
        }
        if self.campaign_room().is_some() {
            if observer {
                return None;
            }
            let playing = self
                .network_setup()?
                .humans
                .iter()
                .filter(|human| !human.is_observer())
                .count();
            if playing >= usize::from(self.campaign_room()?.player_limit()) {
                return None;
            }
            return self.network_open_seat().map(|row| (row, false));
        }
        let observer = observer || self.network_teams().is_some();
        let other = || {
            self.network_several_observers()
                .then(|| self.network_free_row(!observer))
                .flatten()
                .map(|row| (row, !observer))
        };
        self.network_free_row(observer)
            .map(|row| (row, observer))
            .or_else(other)
    }

    /// `0x005E95E0`: the nodes, less an observer on the observer row (every
    /// observer in VERA's rooms), are more than
    /// [`Self::network_slot_test_rows`]. A room sharing bases seats more.
    pub(crate) fn network_over_capacity(&self) -> bool {
        let Some(launch) = self.network_setup() else {
            return false;
        };
        if self.network_shares_bases() {
            return false;
        }
        let limit = self.network_slot_test_rows();
        let observer_row = self.network_observer_row();
        let several = self.network_several_observers();
        let observers = launch
            .humans
            .iter()
            .filter(|human| {
                human.is_observer() && (several || human.raw_slot_index == observer_row)
            })
            .count();
        launch.humans.len() as i32 - observers as i32 > limit
    }
}
