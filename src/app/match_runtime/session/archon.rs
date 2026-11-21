//! LAN Archon groups use the room's team choices. At launch each group gets
//! one explicit start, which the existing shared-House bootstrap consumes.

use super::SeatPick;
use crate::skirmish_launch::{
    LaunchStartPosition, LaunchTeam, NETWORK_PLAYER_SLOT_COUNT, SkirmishHumanSlot,
    SkirmishLaunchMode, SkirmishLaunchSession, SkirmishLocalSlot,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchonRoom {
    #[serde(deserialize_with = "deserialize_limit")]
    max_controllers: u8,
}

impl Default for ArchonRoom {
    fn default() -> Self {
        Self { max_controllers: 2 }
    }
}

fn deserialize_limit<'de, D: serde::Deserializer<'de>>(value: D) -> Result<u8, D::Error> {
    let limit = u8::deserialize(value)?;
    ArchonRoom::new(limit)
        .map(ArchonRoom::max_controllers)
        .map_err(serde::de::Error::custom)
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ArchonStartError {
    #[error("an active player has no Archon group or roster seat")]
    MissingGroup,
    #[error("an Archon group exceeds the room's controller limit")]
    GroupFull,
    #[error("no Archon group has at least two controllers")]
    NoSharedGroup,
    #[error("the match has no opposing House")]
    NoOpponent,
    #[error("two Houses reserve the same start")]
    StartConflict,
    #[error("the map does not have enough valid starts for these Houses")]
    MapStarts,
}

impl ArchonStartError {
    pub(crate) const fn message_key(&self) -> &'static str {
        match self {
            Self::MissingGroup => "VERA:ArchonMissingGroup",
            Self::GroupFull => "VERA:ArchonGroupFull",
            Self::NoSharedGroup => "VERA:ArchonNoSharedGroup",
            Self::NoOpponent => "VERA:ArchonNoOpponent",
            Self::StartConflict => "VERA:ArchonStartConflict",
            Self::MapStarts => "VERA:ArchonMapStarts",
        }
    }
}

impl ArchonRoom {
    pub(crate) const GROUP_COUNT: u8 = 8;
    pub(crate) const CONTROLLER_LIMITS: std::ops::RangeInclusive<u8> =
        2..=NETWORK_PLAYER_SLOT_COUNT as u8;

    pub(crate) fn supports_mode(mode: &SkirmishLaunchMode) -> bool {
        mode.class == crate::skirmish_modes::MpModeClass::Battle && mode.allies_allowed
    }

    pub(crate) fn new(max_controllers: u8) -> anyhow::Result<Self> {
        anyhow::ensure!(
            Self::CONTROLLER_LIMITS.contains(&max_controllers),
            "Archon group size is outside room bounds"
        );
        Ok(Self { max_controllers })
    }

    pub(crate) const fn max_controllers(self) -> u8 {
        self.max_controllers
    }

    pub(crate) fn may_join(
        self,
        launch: &SkirmishLaunchSession,
        row: u8,
        team: LaunchTeam,
    ) -> bool {
        valid_team(launch, team).is_some()
            && seated_players(launch)
                .filter(|(other, player)| *other != row && player.slot.team == team)
                .count()
                < usize::from(self.max_controllers)
    }

    pub(super) fn available_group(
        self,
        launch: &SkirmishLaunchSession,
        row: u8,
    ) -> Option<LaunchTeam> {
        (0..launch.options.teams?)
            .map(LaunchTeam::Team)
            .find(|&team| self.may_join(launch, row, team))
    }

    pub(crate) fn color_in_use(launch: &SkirmishLaunchSession, color: u8, listed: usize) -> bool {
        let owners = (0..launch.options.teams.unwrap_or(0))
            .filter_map(|group| group_leader(launch, LaunchTeam::Team(group)))
            .map(|leader| (leader.slot.color_index, leader.slot.color_random))
            .chain(
                launch
                    .opponents
                    .iter()
                    .map(|ai| (ai.color_index, ai.color_random)),
            );
        usize::from(color) >= listed
            || (owners.clone().count() <= listed
                && owners
                    .into_iter()
                    .any(|(index, random)| !random && index == color))
    }

    pub(crate) fn plan_starts(
        self,
        launch: &SkirmishLaunchSession,
        start_count: usize,
    ) -> Result<Vec<(u8, SeatPick)>, ArchonStartError> {
        if launch
            .humans
            .iter()
            .any(|player| !player.is_observer() && player.raw_slot_index.is_none())
        {
            return Err(ArchonStartError::MissingGroup);
        }
        let mut groups: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
        for (row, player) in seated_players(launch) {
            let group =
                valid_team(launch, player.slot.team).ok_or(ArchonStartError::MissingGroup)?;
            groups.entry(group).or_default().push(row);
        }
        if groups
            .values()
            .any(|rows| rows.len() > usize::from(self.max_controllers))
        {
            return Err(ArchonStartError::GroupFull);
        }
        if groups.values().all(|rows| rows.len() < 2) {
            return Err(ArchonStartError::NoSharedGroup);
        }
        if groups.len() == 1
            && launch
                .opponents
                .iter()
                .all(|ai| matches!(ai.team, LaunchTeam::Team(team) if groups.contains_key(&team)))
        {
            return Err(ArchonStartError::NoOpponent);
        }

        let mut houses = Vec::with_capacity(groups.len() + launch.opponents.len());
        for (group, rows) in groups {
            let leader = group_leader(launch, LaunchTeam::Team(group))
                .ok_or(ArchonStartError::MissingGroup)?;
            houses.push((rows, leader.slot.start_position));
        }
        let mut ais: Vec<_> = launch.opponents.iter().collect();
        ais.sort_unstable_by_key(|ai| ai.raw_slot_index);
        houses.extend(
            ais.into_iter()
                .map(|ai| (vec![ai.raw_slot_index], ai.start_position)),
        );

        let mut reserved = BTreeSet::new();
        for (_, start) in &houses {
            if let LaunchStartPosition::Position(start) = *start {
                if usize::from(start) >= start_count {
                    return Err(ArchonStartError::MapStarts);
                }
                if !reserved.insert(start) {
                    return Err(ArchonStartError::StartConflict);
                }
            }
        }
        let mut available = (0..=u8::MAX)
            .take(start_count)
            .filter(|start| !reserved.contains(start));
        let mut picks = Vec::new();
        for (rows, start) in houses {
            let position = match start {
                LaunchStartPosition::Position(position) => position,
                LaunchStartPosition::Auto => available.next().ok_or(ArchonStartError::MapStarts)?,
            };
            let pick = SeatPick::Start(LaunchStartPosition::Position(position));
            picks.extend(rows.into_iter().map(|row| (row, pick)));
        }
        Ok(picks)
    }
}

fn valid_team(launch: &SkirmishLaunchSession, team: LaunchTeam) -> Option<u8> {
    match team {
        LaunchTeam::Team(index) if launch.options.teams.is_some_and(|count| index < count) => {
            Some(index)
        }
        _ => None,
    }
}

fn seated_players(
    launch: &SkirmishLaunchSession,
) -> impl Iterator<Item = (u8, &SkirmishHumanSlot)> {
    launch
        .humans
        .iter()
        .filter(|player| !player.is_observer())
        .filter_map(|player| Some((player.raw_slot_index?, player)))
}

/// Select by the admitted roster row rather than the client's local-first
/// source vector. Both peers then project the same leader for each group.
pub(crate) fn group_leader(
    launch: &SkirmishLaunchSession,
    team: LaunchTeam,
) -> Option<&SkirmishHumanSlot> {
    valid_team(launch, team)?;
    seated_players(launch)
        .filter(|(_, player)| player.slot.team == team)
        .min_by_key(|(row, _)| *row)
        .map(|(_, player)| player)
}

/// An edit can move the front row into or out of a group. Keep its selections
/// with the group while there is still a controller to hold them.
#[derive(Default)]
pub(super) struct ArchonGroupChoices(BTreeMap<u8, (String, SkirmishLocalSlot)>);

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::super::tests::network_fixture;
    use super::super::{MatchSession, NetworkReturnContext};
    use super::*;
    use crate::skirmish_launch::{AiDifficulty, SkirmishAiSlot};

    fn room_launch() -> SkirmishLaunchSession {
        let mut launch = network_fixture();
        launch.options.teams = Some(ArchonRoom::GROUP_COUNT);
        launch.humans[0].slot.team = LaunchTeam::Team(0);
        launch.humans[1].slot.team = LaunchTeam::Team(0);
        launch.humans[0].slot.start_position = LaunchStartPosition::Position(2);
        launch.humans[1].slot.start_position = LaunchStartPosition::Auto;
        let mut opponent = launch.humans[1].clone();
        opponent.name = "Opponent".into();
        opponent.raw_slot_index = Some(2);
        opponent.slot.team = LaunchTeam::Team(1);
        launch.humans.push(opponent);
        launch
    }

    #[test]
    fn shared_players_use_the_group_start_with_free_map_starts() {
        let mut launch = room_launch();
        launch.opponents.push(SkirmishAiSlot {
            raw_slot_index: 3,
            country: launch.humans[0].slot.country,
            country_random: false,
            color_index: 3,
            color_random: false,
            start_position: LaunchStartPosition::Position(0),
            team: LaunchTeam::None,
            difficulty: AiDifficulty::Easy,
        });
        let mut picks = ArchonRoom::default().plan_starts(&launch, 5).unwrap();
        picks.sort_by_key(|(row, _)| *row);
        assert_eq!(
            picks,
            [2, 2, 1, 0]
                .into_iter()
                .enumerate()
                .map(|(row, start)| (
                    row as u8,
                    SeatPick::Start(LaunchStartPosition::Position(start)),
                ))
                .collect::<Vec<_>>(),
        );
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, true, None, true)
            .unwrap();
        session.set_network_seat_picks(&picks).unwrap();
        let leaders: Vec<_> = session
            .network_setup()
            .unwrap()
            .humans_with_house_leaders()
            .map(|(_, _, leader)| leader)
            .collect();
        assert_eq!(leaders, [0, 0, 2]);
    }

    #[test]
    fn a_full_group_keeps_its_own_players_but_refuses_another() {
        let launch = room_launch();
        let room = ArchonRoom::default();
        assert!(room.may_join(&launch, 1, LaunchTeam::Team(0)));
        assert!(!room.may_join(&launch, 2, LaunchTeam::Team(0)));
        assert!(
            ArchonRoom::new(3)
                .unwrap()
                .may_join(&launch, 2, LaunchTeam::Team(0))
        );
    }

    #[test]
    fn groups_cannot_claim_the_same_map_start() {
        let mut launch = room_launch();
        launch.humans[2].slot.start_position = LaunchStartPosition::Position(2);
        assert_eq!(
            ArchonRoom::default().plan_starts(&launch, 5),
            Err(ArchonStartError::StartConflict),
        );
    }

    #[test]
    fn group_choices_survive_member_moves_and_departures() {
        let mut launch = room_launch();
        launch.local_player_index = 2;
        launch.humans[2].slot.color_index = 3;
        launch.humans[2].slot.country_random = true;
        launch.humans[2].slot.start_position = LaunchStartPosition::Position(4);
        let first_group = launch.humans[0].slot.clone();
        let second_group = launch.humans[2].slot.clone();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, true, None, true)
            .unwrap();
        session
            .accept_archon_room(Some(ArchonRoom::default()))
            .unwrap();

        let mut moving = first_group.clone();
        moving.team = LaunchTeam::Team(1);
        session
            .accept_lan_peer_choices(0, moving, false, true)
            .unwrap();
        assert_eq!(session.network_row_choices(1), Some(first_group));
        assert_eq!(session.network_row_choices(0), Some(second_group.clone()));

        session.remove_network_participant(0).unwrap();
        assert_eq!(session.network_row_choices(2), Some(second_group.clone()));
        let mut edited = second_group;
        edited.start_position = LaunchStartPosition::Position(3);
        session.set_network_row_choices(2, edited.clone()).unwrap();
        assert_eq!(session.network_row_choices(2), Some(edited));
    }
}
impl super::MatchSession {
    pub(crate) fn available_archon_group(&self, row: u8) -> Option<LaunchTeam> {
        self.archon_room()?
            .available_group(self.network_setup()?, row)
    }

    pub(super) fn archon_group_choices(&self) -> ArchonGroupChoices {
        if !self.host() || self.archon_room().is_none() {
            return ArchonGroupChoices::default();
        }
        let Some(launch) = self.network_setup() else {
            return ArchonGroupChoices::default();
        };
        ArchonGroupChoices(
            (0..launch.options.teams.unwrap_or(0))
                .filter_map(|team| {
                    let leader = group_leader(launch, LaunchTeam::Team(team))?;
                    Some((team, (leader.name.clone(), leader.slot.clone())))
                })
                .collect(),
        )
    }

    pub(super) fn preserve_archon_group_choices(
        &mut self,
        previous: ArchonGroupChoices,
    ) -> anyhow::Result<bool> {
        if previous.0.is_empty() {
            return Ok(false);
        }
        let launch = self.editable_network_setup()?;
        let mut changed = false;
        for (team, (previous_leader, slot)) in previous.0 {
            let Some(leader) = group_leader(launch, LaunchTeam::Team(team))
                .filter(|leader| leader.name != previous_leader)
            else {
                continue;
            };
            let row = leader.raw_slot_index;
            if let Some(next) = launch
                .humans
                .iter_mut()
                .find(|player| player.raw_slot_index == row)
            {
                changed |= next.slot != slot;
                next.slot = slot;
            }
        }
        Ok(changed)
    }

    pub(crate) fn set_network_archon_limit(&mut self, max_controllers: u8) -> anyhow::Result<bool> {
        anyhow::ensure!(self.host(), "only the host sets the Archon group limit");
        anyhow::ensure!(self.archon_room().is_some(), "no Archon room");
        let room = ArchonRoom::new(max_controllers)?;
        let launch = self.editable_network_setup()?;
        if launch
            .humans
            .iter()
            .filter(|human| !human.is_observer())
            .any(|human| {
                human
                    .raw_slot_index
                    .is_some_and(|row| !room.may_join(launch, row, human.slot.team))
            })
        {
            return Ok(false);
        }
        self.accept_archon_room(Some(room))?;
        Ok(true)
    }

    /// Validate an edit at the admitted LAN seat boundary. Private packets
    /// carry a whole slot, so fields belonging to the host or group leader
    /// need protection even when the UI only exposed a group pick.
    pub(crate) fn admit_archon_choices(
        &self,
        index: usize,
        choices: &mut crate::skirmish_launch::SkirmishLocalSlot,
        observer: &mut bool,
        host_edit: bool,
    ) -> bool {
        let Some(room) = self.archon_room() else {
            return false;
        };
        let Some(launch) = self.network_setup() else {
            return false;
        };
        let Some(human) = launch.humans.get(index) else {
            return false;
        };
        let Some(row) = human.raw_slot_index else {
            return false;
        };
        if *observer {
            return false;
        }
        let requested = choices.clone();
        if !room.may_join(launch, row, choices.team) {
            *observer = human.is_observer();
            *choices = human.slot.clone();
            return true;
        }
        if choices.team != human.slot.team || human.is_observer() {
            // A move does not also edit the destination House's selections.
            let team = choices.team;
            *choices = human.slot.clone();
            choices.team = team;
            choices.start_position = LaunchStartPosition::Auto;
            choices.color_random = true;
        } else if self.network_archon_group_leader(row) != Some(row) {
            *choices = human.slot.clone();
        } else if !host_edit {
            choices.start_position = human.slot.start_position;
        }
        *choices != requested
    }
}
