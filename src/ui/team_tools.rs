//! VERA's Quick Team Setup and Bulk Seat Actions menus, the host's in VERA's
//! game room and the player's in Skirmish, as the VERA launcher's room has
//! them (vera20k-launcher `src/lib/teams.ts`, `src/views/RoomView.vue`).
//! Team Starts is [`crate::ui::skirmish_shell::starts_by_team`]; the quick
//! teams ([`preset_teams`]), the even split ([`balance_teams`]) and Team
//! Colors ([`colors_by_team`]) are here, and each screen applies them
//! through its own owner of seats and teams.

use crate::rules::ruleset::RuleSet;
use crate::rules::skirmish_colors::RETAIL_COLOR_COUNT;
use crate::skirmish_launch::{AiDifficulty, LaunchTeam};

/// The Quick Team Setup menu's items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamTool {
    Starts,
    Colors,
    Balance,
    Preset(TeamPreset),
}

impl TeamTool {
    /// Its value in the room's Quick Team Setup list, whose caption is 0.
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Starts => 1,
            Self::Colors => 2,
            Self::Balance => 3,
            Self::Preset(TeamPreset::PlayersVsAi) => 4,
            Self::Preset(TeamPreset::FreeForAll) => 5,
            Self::Preset(TeamPreset::AiTeams(count)) => 10 + i32::from(count),
        }
    }
    pub(crate) fn from_code(code: i32) -> Option<Self> {
        [Self::Starts, Self::Colors, Self::Balance]
            .into_iter()
            .chain(PRESETS.map(Self::Preset))
            .find(|tool| tool.code() == code)
    }
}

/// The quick teams, under Team Starts, Team Colors and Balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamPreset {
    /// Every player on Team A, every AI on Team B.
    PlayersVsAi,
    /// The AIs dealt onto this many teams, from Team A.
    AiTeams(u8),
    /// Every seat off its team.
    FreeForAll,
}

pub const PRESETS: [TeamPreset; 5] = [
    TeamPreset::PlayersVsAi,
    TeamPreset::AiTeams(2),
    TeamPreset::AiTeams(3),
    TeamPreset::AiTeams(4),
    TeamPreset::FreeForAll,
];

/// Why a Quick Team Setup item cannot be had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRefusal {
    /// Not offered here, for no reason the menu names.
    Unavailable,
    /// Team Colors and Team Starts in a room with Teams Off.
    TwoTeamsNeeded,
    NoPlayers,
    NoAi,
    /// Fewer AIs than the teams asked for.
    FewAi(u8),
    /// The Team list offers too few teams.
    NoFreeTeam,
    /// The mode needs every player on a team.
    ModeNeedsTeams,
}

/// A seat that plays, as the quick teams see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    Player(LaunchTeam),
    Ai(LaunchTeam),
}

/// A quick team's team for each of `seats` (`None`: it keeps its own), or
/// why it cannot be had. `teams` is how many teams the Team list offers.
/// Players vs AI puts every player on A and every AI on B. Splitting the AIs
/// deals them in seat order onto `count` teams of near equal size, from
/// Team A; the players keep theirs, sharing a team with its AIs. Free-for-all
/// takes every seat off its team, which a mode that must ally refuses.
pub(crate) fn preset_teams(
    preset: TeamPreset,
    seats: &[Seat],
    must_ally: bool,
    teams: u8,
) -> Result<Vec<Option<LaunchTeam>>, ToolRefusal> {
    let players = seats
        .iter()
        .filter(|seat| matches!(seat, Seat::Player(_)))
        .count();
    let ais = seats.len() - players;
    match preset {
        TeamPreset::FreeForAll if must_ally => Err(ToolRefusal::ModeNeedsTeams),
        TeamPreset::FreeForAll => Ok(vec![Some(LaunchTeam::None); seats.len()]),
        TeamPreset::PlayersVsAi if players == 0 => Err(ToolRefusal::NoPlayers),
        TeamPreset::PlayersVsAi if ais == 0 => Err(ToolRefusal::NoAi),
        TeamPreset::PlayersVsAi => Ok(seats
            .iter()
            .map(|seat| {
                Some(LaunchTeam::Team(match seat {
                    Seat::Player(_) => 0,
                    Seat::Ai(_) => 1,
                }))
            })
            .collect()),
        TeamPreset::AiTeams(count) => {
            if ais < usize::from(count) {
                return Err(ToolRefusal::FewAi(count));
            }
            if count > teams {
                return Err(ToolRefusal::NoFreeTeam);
            }
            // Team k takes the next ais / count AIs, one more while some
            // are left over.
            let mut dealt = 0;
            Ok(seats
                .iter()
                .map(|seat| {
                    let Seat::Ai(_) = seat else {
                        return None;
                    };
                    let team = (0..count)
                        .find(|&team| {
                            let (through, count) = (usize::from(team + 1), usize::from(count));
                            dealt < through * (ais / count) + through.min(ais % count)
                        })
                        .unwrap_or(count - 1);
                    dealt += 1;
                    Some(LaunchTeam::Team(team))
                })
                .collect())
        }
    }
}

/// Balance: each seat's team in `0..count` with the totals of `ratings` as
/// even as can be, keeping as many seats on their `current` team as it can.
/// Teams differ in size by one at most. Two teams of up to 16 seats have
/// every split tried; more go strongest first, each to the weakest team that
/// still has room.
pub(crate) fn balance_teams(
    ratings: &[u32],
    current: &[Option<usize>],
    count: usize,
) -> Vec<usize> {
    let seats = ratings.len();
    if count == 0 {
        return Vec::new();
    }
    let sizes: Vec<usize> = (0..count)
        .map(|team| seats / count + usize::from(team < seats % count))
        .collect();
    let kept = |teams: &[usize]| {
        teams
            .iter()
            .zip(current)
            .filter(|(team, current)| Some(**team) == **current)
            .count()
    };
    let spread = |teams: &[usize]| {
        let mut sums = vec![0u64; count];
        for (&team, &rating) in teams.iter().zip(ratings) {
            sums[team] += u64::from(rating);
        }
        sums.iter().max().unwrap_or(&0) - sums.iter().min().unwrap_or(&0)
    };
    if count == 2 && seats <= 16 {
        let mut best: Option<Vec<usize>> = None;
        for mask in 0u32..1 << seats {
            let on_a = seats - mask.count_ones() as usize;
            if on_a != sizes[0] && on_a != sizes[1] {
                continue;
            }
            let teams: Vec<usize> = (0..seats).map(|seat| (mask >> seat) as usize & 1).collect();
            let better = best.as_ref().is_none_or(|best| {
                (spread(&teams), std::cmp::Reverse(kept(&teams)))
                    < (spread(best), std::cmp::Reverse(kept(best)))
            });
            if better {
                best = Some(teams);
            }
        }
        return best.unwrap_or_default();
    }
    let mut order: Vec<usize> = (0..seats).collect();
    order.sort_by_key(|&seat| (std::cmp::Reverse(ratings[seat]), seat));
    let mut teams = vec![0; seats];
    let mut sums = vec![0u64; count];
    let mut filled = vec![0; count];
    for seat in order {
        let team = (0..count)
            .filter(|&team| filled[team] < sizes[team])
            .min_by_key(|&team| (sums[team], current[seat] != Some(team)))
            .unwrap_or(0);
        teams[seat] = team;
        sums[team] += u64::from(ratings[seat]);
        filled[team] += 1;
    }
    teams
}

/// The Bulk Seat Actions menu's items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatAction {
    /// An AI in every open seat, of this difficulty or, with `None`, one
    /// drawn for each seat.
    FillAi(Option<AiDifficulty>),
    /// Every open seat closed.
    CloseAll,
    /// Every closed seat open.
    OpenClosed,
    /// Every AI's seat open.
    RemoveAi,
}

impl SeatAction {
    const ALL: [Self; 7] = [
        Self::FillAi(Some(AiDifficulty::Easy)),
        Self::FillAi(Some(AiDifficulty::Normal)),
        Self::FillAi(Some(AiDifficulty::Hard)),
        Self::FillAi(None),
        Self::CloseAll,
        Self::OpenClosed,
        Self::RemoveAi,
    ];
    /// Its value in the room's Bulk Seat Actions list, whose caption is 0.
    pub(crate) fn code(self) -> i32 {
        Self::ALL
            .iter()
            .position(|action| *action == self)
            .map_or(0, |index| index as i32 + 1)
    }
    pub(crate) fn from_code(code: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.code() == code)
    }
}

/// What a screen has for Bulk Seat Actions: how many seats are open,
/// closed and hold an AI, whether open seats can be closed and AI may play.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeatCounts {
    pub open: usize,
    pub closed: usize,
    pub ai: usize,
    pub closable: bool,
    pub ai_allowed: bool,
}

/// Bulk Seat Actions' items with whether each is offered: filling and
/// closing for open seats, opening for closed ones, removing for AIs.
pub(crate) fn seat_actions(counts: SeatCounts) -> Vec<(SeatAction, bool)> {
    let mut items = Vec::new();
    if counts.open > 0 {
        let fill = [
            Some(AiDifficulty::Easy),
            Some(AiDifficulty::Normal),
            Some(AiDifficulty::Hard),
            None,
        ];
        items.extend(fill.map(|difficulty| (SeatAction::FillAi(difficulty), counts.ai_allowed)));
        if counts.closable {
            items.push((SeatAction::CloseAll, true));
        }
    }
    if counts.closed > 0 {
        items.push((SeatAction::OpenClosed, true));
    }
    if counts.ai > 0 {
        items.push((SeatAction::RemoveAi, true));
    }
    items
}

/// A difficulty for Fill with AI's random choice, from `bits`.
pub(crate) fn drawn_difficulty(bits: u8) -> AiDifficulty {
    match bits % 3 {
        0 => AiDifficulty::Easy,
        1 => AiDifficulty::Normal,
        _ => AiDifficulty::Hard,
    }
}

/// The menus' captions.
pub(crate) const TEAM_MENU_KEY: &str = "VERA:QuickTeamSetup";
pub(crate) const SEAT_MENU_KEY: &str = "VERA:BulkSeatActions";

/// The heading over the quick teams.
pub(crate) fn presets_heading() -> String {
    crate::i18n::tr("VERA:QuickTeams")
}

/// A Quick Team Setup item's text, with why it is not offered after it.
pub(crate) fn team_tool_text(tool: TeamTool, refusal: Option<ToolRefusal>) -> String {
    let text = match tool {
        TeamTool::Starts => crate::i18n::tr("VERA:TeamStarts"),
        TeamTool::Colors => crate::i18n::tr("VERA:TeamColors"),
        TeamTool::Balance => crate::i18n::tr("VERA:BalanceTeams"),
        TeamTool::Preset(TeamPreset::PlayersVsAi) => crate::i18n::tr("VERA:PlayersVsAi"),
        TeamTool::Preset(TeamPreset::AiTeams(count)) => {
            crate::i18n::tr_fill("VERA:AiTeams", &[("n", &count)])
        }
        TeamTool::Preset(TeamPreset::FreeForAll) => crate::i18n::tr("VERA:FreeForAll"),
    };
    let Some(refusal) = refusal else {
        return text;
    };
    let why = match refusal {
        ToolRefusal::Unavailable => return text,
        ToolRefusal::TwoTeamsNeeded => crate::i18n::tr("VERA:TwoTeamsNeeded"),
        ToolRefusal::NoPlayers => crate::i18n::tr("VERA:NoPlayersSeated"),
        ToolRefusal::NoAi => crate::i18n::tr("VERA:NoAiSeated"),
        ToolRefusal::FewAi(count) => crate::i18n::tr_fill("VERA:NeedsAis", &[("n", &count)]),
        ToolRefusal::NoFreeTeam => crate::i18n::tr("VERA:NoFreeTeam"),
        ToolRefusal::ModeNeedsTeams => crate::i18n::tr("VERA:ModeNeedsTeams"),
    };
    format!("{text} ({why})")
}

/// A Bulk Seat Actions item's text.
pub(crate) fn seat_action_text(action: SeatAction) -> String {
    let fill = |key: &str| crate::i18n::tr_fill("VERA:FillWithAi", &[("d", &crate::i18n::tr(key))]);
    match action {
        SeatAction::FillAi(Some(AiDifficulty::Easy)) => fill("VERA:DifficultyEasy"),
        SeatAction::FillAi(Some(AiDifficulty::Normal)) => fill("VERA:DifficultyNormal"),
        SeatAction::FillAi(Some(AiDifficulty::Hard)) => fill("VERA:DifficultyHard"),
        SeatAction::FillAi(None) => fill("VERA:RandomDifficulty"),
        SeatAction::CloseAll => crate::i18n::tr("VERA:CloseAll"),
        SeatAction::OpenClosed => crate::i18n::tr("VERA:OpenClosedSeats"),
        SeatAction::RemoveAi => crate::i18n::tr("VERA:RemoveAllAi"),
    }
}

/// The colours a lobby lists ([`crate::rules::skirmish_colors::SkirmishColorMode`]),
/// YR's eight first, each with the hue of its `[Colors]` entry.
pub(crate) fn listed_hues(rules: &RuleSet) -> Vec<(u8, u8)> {
    let colors = rules.skirmish_colors();
    colors
        .lobby_priorities()
        .filter_map(|color| {
            let entry = colors.entry_for_priority(i32::from(color));
            Some((color, rules.color_schemes.get(entry)?.hsv[0]))
        })
        .collect()
}

/// `[Colors]` hues run 0..=254 round the circle, and 255 is red again, as
/// `HSV_To_RGB` (`0x00517440`, [`crate::rules::color_scheme::hsv_to_rgb`])
/// reads them.
const HUES: i32 = 255;

/// Team Colors: each seat's colour, given each seat's team in row order and
/// the lobby's colours with their hues (`listed`, YR's eight first). The
/// colours, sorted round the hue circle, are cut into one range per team
/// ([`HueCircle`]), and each team's players get colours spread across its
/// range. While every team fits in its range of YR's eight, only the eight
/// are used, as a Random colour prefers them; a bigger room uses the whole
/// list. A team with more players than its range takes the free colours
/// nearest it, and once none is free, teammates share. `None` is a seat on
/// none of the `count` teams, or a lobby without colours.
pub(crate) fn colors_by_team(listed: &[(u8, u8)], teams: &[u8], count: u8) -> Vec<Option<u8>> {
    let count = usize::from(count);
    let mut players = vec![0; count];
    for &team in teams {
        if let Some(players) = players.get_mut(usize::from(team)) {
            *players += 1;
        }
    }
    let fits = |circle: &HueCircle| {
        circle.ranges.len() == count
            && circle
                .ranges
                .iter()
                .zip(&players)
                .all(|(&(_, len), &players)| players <= len)
    };
    let eight: Vec<(u8, u8)> = listed
        .iter()
        .copied()
        .filter(|&(color, _)| usize::from(color) < RETAIL_COLOR_COUNT)
        .collect();
    let circle = Some(HueCircle::new(&eight, count))
        .filter(fits)
        .unwrap_or_else(|| HueCircle::new(listed, count));
    let given = circle.hand_out(&players);
    let mut next = vec![0; count];
    teams
        .iter()
        .map(|&team| {
            let team = usize::from(team);
            let place = *given.get(team)?.get(next[team])?;
            next[team] += 1;
            Some(circle.colors[place].0)
        })
        .collect()
}

/// A lobby's colours sorted round the hue circle, with each team's range of
/// them: its first place and how many it holds. The ranges hold the same
/// number of colours, the first teams one more while they do not divide
/// evenly, and follow each other round the circle from the first team's,
/// whose middle is nearest red: two teams get warm against cool.
struct HueCircle {
    colors: Vec<(u8, i32)>,
    ranges: Vec<(usize, usize)>,
}

impl HueCircle {
    fn new(listed: &[(u8, u8)], count: usize) -> Self {
        let mut colors: Vec<(u8, i32)> = listed
            .iter()
            .map(|&(color, hue)| (color, i32::from(hue) % HUES))
            .collect();
        colors.sort_by_key(|&(_, hue)| hue);
        let mut circle = Self {
            colors,
            ranges: Vec::new(),
        };
        let places = circle.colors.len();
        if places == 0 || count == 0 {
            return circle;
        }
        let lengths: Vec<usize> = (0..count)
            .map(|team| places / count + usize::from(team < places % count))
            .collect();
        let first = lengths.first().copied().unwrap_or_default();
        let start = (0..places)
            .min_by_key(|&start| hue_gap(circle.middle(start, first), 0))
            .unwrap_or_default();
        let mut place = start;
        circle.ranges = lengths
            .into_iter()
            .map(|len| {
                let range = (place, len);
                place = (place + len) % places;
                range
            })
            .collect();
        circle
    }

    /// The middle hue of the `len` colours round the circle from `start`,
    /// in half hue steps.
    fn middle(&self, start: usize, len: usize) -> i32 {
        let hue = |place: usize| self.colors[place % self.colors.len()].1;
        let first = hue(start);
        let last = hue(start + len.max(1) - 1);
        2 * first + (last - first).rem_euclid(HUES)
    }

    /// Each team's places on the circle, one per player: spread across its
    /// range, then the free places nearest its range, then its own again.
    fn hand_out(&self, players: &[usize]) -> Vec<Vec<usize>> {
        let places = self.colors.len();
        let mut taken = vec![false; places];
        let mut given = vec![Vec::new(); self.ranges.len()];
        let teams = || self.ranges.iter().zip(players);
        for (own, (&(start, len), &players)) in given.iter_mut().zip(teams()) {
            let wanted = players.min(len);
            for player in 0..wanted {
                let place = (start + (2 * player + 1) * len / (2 * wanted)) % places;
                taken[place] = true;
                own.push(place);
            }
        }
        for (own, (&(start, len), &players)) in given.iter_mut().zip(teams()) {
            let middle = self.middle(start, len);
            while own.len() < players {
                let Some(place) = (0..places)
                    .filter(|&place| !taken[place])
                    .min_by_key(|&place| hue_gap(2 * self.colors[place].1, middle))
                else {
                    break;
                };
                taken[place] = true;
                own.push(place);
            }
        }
        for (own, (&(start, len), &players)) in given.iter_mut().zip(teams()) {
            let distinct = own.len();
            for player in distinct..players {
                let place = match distinct {
                    0 => (start + len / 2) % places,
                    _ => own[player % distinct],
                };
                own.push(place);
            }
        }
        given
    }
}

/// How far apart two hues lie round the circle, both in half hue steps.
fn hue_gap(a: i32, b: i32) -> i32 {
    let gap = (a - b).rem_euclid(2 * HUES);
    gap.min(2 * HUES - gap)
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::rules::skirmish_colors::FIRST_EXTRA_PRIORITY;

    const A: LaunchTeam = LaunchTeam::Team(0);
    const B: LaunchTeam = LaunchTeam::Team(1);
    const NONE: LaunchTeam = LaunchTeam::None;

    /// Five AIs in two teams: three on Team A, two on Team B, in seat
    /// order. The player keeps Team A, with its AIs. Four teams of AIs need
    /// four AIs and a Team list of four.
    #[test]
    fn ai_teams_deal_the_ais_from_team_a() {
        let seats = [
            Seat::Player(A),
            Seat::Ai(NONE),
            Seat::Ai(NONE),
            Seat::Ai(A),
            Seat::Ai(NONE),
            Seat::Ai(NONE),
        ];
        let (c, d) = (LaunchTeam::Team(2), LaunchTeam::Team(3));
        assert_eq!(
            preset_teams(TeamPreset::AiTeams(2), &seats, false, 4),
            Ok(vec![None, Some(A), Some(A), Some(A), Some(B), Some(B)])
        );
        assert_eq!(
            preset_teams(TeamPreset::AiTeams(4), &seats, false, 4),
            Ok(vec![None, Some(A), Some(A), Some(B), Some(c), Some(d)])
        );
        assert_eq!(
            preset_teams(TeamPreset::AiTeams(4), &seats, false, 3),
            Err(ToolRefusal::NoFreeTeam)
        );
        assert_eq!(
            preset_teams(TeamPreset::AiTeams(4), &seats[..4], false, 15),
            Err(ToolRefusal::FewAi(4))
        );
    }

    /// Players vs AI needs both; Free-for-all takes every seat off its team
    /// unless the mode must ally.
    #[test]
    fn players_vs_ai_and_free_for_all() {
        let seats = [Seat::Ai(B), Seat::Player(NONE), Seat::Ai(A)];
        assert_eq!(
            preset_teams(TeamPreset::PlayersVsAi, &seats, false, 4),
            Ok(vec![Some(B), Some(A), Some(B)])
        );
        assert_eq!(
            preset_teams(TeamPreset::PlayersVsAi, &seats[1..2], false, 4),
            Err(ToolRefusal::NoAi)
        );
        assert_eq!(
            preset_teams(TeamPreset::FreeForAll, &seats, true, 4),
            Err(ToolRefusal::ModeNeedsTeams)
        );
        assert_eq!(
            preset_teams(TeamPreset::FreeForAll, &seats, false, 4),
            Ok(vec![Some(NONE); 3])
        );
    }

    /// 3, 3, 2, 1, 1 on two teams: 5 against 5, sizes three and two, and of
    /// the even splits one that moves fewest seats. Three teams go strongest
    /// first onto the weakest team with room.
    #[test]
    fn balance_evens_the_totals() {
        let ratings = [3, 3, 2, 1, 1];
        let current = [Some(0), Some(0), Some(1), Some(1), Some(1)];
        let teams = balance_teams(&ratings, &current, 2);
        let total = |team| {
            teams
                .iter()
                .zip(ratings)
                .filter(|(on, _)| **on == team)
                .map(|(_, rating)| rating)
                .sum::<u32>()
        };
        assert_eq!((total(0), total(1)), (5, 5));
        assert_eq!(teams, [1, 0, 0, 1, 1]);
        assert_eq!(
            balance_teams(&[3, 3, 3, 2, 2, 2], &[None; 6], 3),
            [0, 1, 2, 0, 1, 2]
        );
    }

    /// The `[Colors]` names of the colours Team Colors gives seats on
    /// `teams`.
    fn team_color_names(
        rules: &RuleSet,
        listed: &[(u8, u8)],
        teams: &[u8],
        count: u8,
    ) -> Vec<String> {
        let colors = rules.skirmish_colors();
        colors_by_team(listed, teams, count)
            .into_iter()
            .map(|color| {
                let entry = colors.entry_for_priority(i32::from(color.expect("a team's colour")));
                rules.color_schemes[entry].name.clone()
            })
            .collect()
    }

    /// Two teams of four get YR's eight, warm against cool, as a Random
    /// colour prefers them. Five a side take the whole list: every player
    /// apart, each team's hues nearer its own end of the circle.
    #[test]
    fn team_colors_give_two_teams_warm_against_cool() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let listed = listed_hues(&rules);
        let four = team_color_names(&rules, &listed, &[0, 1, 0, 1, 0, 1, 0, 1], 2);
        let team = |names: &[String], team: usize| -> Vec<String> {
            names.iter().skip(team).step_by(2).cloned().collect()
        };
        assert_eq!(team(&four, 0), ["Magenta", "DarkRed", "Orange", "Gold"]);
        assert_eq!(
            team(&four, 1),
            ["DarkGreen", "DarkSky", "DarkBlue", "Purple"]
        );

        let teams = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
        let five: Vec<u8> = colors_by_team(&listed, &teams, 2)
            .into_iter()
            .flatten()
            .collect();
        let distinct: std::collections::BTreeSet<u8> = five.iter().copied().collect();
        assert_eq!(distinct.len(), 10);
        assert!(five.iter().any(|&color| color >= FIRST_EXTRA_PRIORITY));
        for (&team, color) in teams.iter().zip(five) {
            let (_, hue) = listed.iter().find(|(listed, _)| *listed == color).unwrap();
            // Red is 0, cyan half the circle away, in half hue steps.
            let hue = 2 * i32::from(*hue);
            let warm = hue_gap(hue, 0) < hue_gap(hue, HUES);
            assert_eq!(warm, team == 0, "colour {color} on Team {team}");
        }
    }

    /// A full room of four teams keeps all 30 players apart, though one
    /// team has a player more than its range holds. With more players than
    /// colours, as with a mod that lists only the eight, only allies share.
    #[test]
    fn team_colors_keep_players_apart_and_share_only_with_allies() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let listed = listed_hues(&rules);
        assert_eq!(listed.len(), 30);
        let teams: Vec<u8> = [(0, 8), (1, 8), (2, 8), (3, 6)]
            .into_iter()
            .flat_map(|(team, players)| std::iter::repeat_n(team, players))
            .collect();
        let full = team_color_names(&rules, &listed, &teams, 4);
        let distinct: std::collections::BTreeSet<&String> = full.iter().collect();
        assert_eq!(distinct.len(), 30);

        let teams = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
        let shared = colors_by_team(&listed[..RETAIL_COLOR_COUNT], &teams, 2);
        let team = |team: u8| -> std::collections::BTreeSet<u8> {
            teams
                .iter()
                .zip(&shared)
                .filter(|(on, _)| **on == team)
                .map(|(_, color)| color.expect("a team's colour"))
                .collect()
        };
        assert_eq!((team(0).len(), team(1).len()), (4, 4));
        assert!(team(0).is_disjoint(&team(1)));
    }
}
