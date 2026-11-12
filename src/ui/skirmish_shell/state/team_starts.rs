//! VERA's Team Starts; gamemd's `0x102` has no counterpart. gamemd gives a
//! player without a start the free one farthest from the starts already
//! taken (`0x005D6890`), whatever the teams, so allies can land on opposite
//! sides of the map. Team Starts fills the page's Start column instead:
//! each team's players take starts next to each other, and the teams keep
//! as far from each other as the map allows. The picks are ordinary Start
//! choices, shown on the page for the player to change, and the launch
//! reads them as it reads any other.

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::LaunchStartPosition;

use super::combos::apply_combo_selection;
use super::saved_setup::{computer_rows, pick, seat_shown};
use super::{SkirmishAiRowType, SkirmishComboId, SkirmishComboItem, SkirmishShellState};

/// The Team combo's item data for a row on no team.
const NO_TEAM: i32 = -2;

/// Maps with at most this many starts have every seating tried: eight starts
/// give at most 8! = 40,320. A bigger map improves the first guess instead.
const TRY_EVERY_SEATING_UP_TO: usize = 8;

/// A start for each seat, by seat: `starts` are the map's start cells by
/// start index and `teams` each seat's team (`NO_TEAM` plays alone).
///
/// A seating is judged by each player's margin: how much farther their
/// nearest enemy starts than their nearest ally (a player on no team: how
/// far their nearest enemy starts). The pick raises the smallest margin as
/// far as the map allows, then the next smallest, and so on, so that a
/// player starts nearer an ally than an enemy wherever the map has room for
/// it. On a map of up to eight starts every seating is tried; on a bigger
/// one the first guess is improved by swapping two rivals' starts, or moving
/// a player to a free start, while that raises the margins. Among equal
/// seatings the first guess stays. Seats past the map's starts get none.
pub fn starts_by_team(starts: &[(i32, i32)], teams: &[i32]) -> Vec<Option<usize>> {
    let guess = first_guess(starts, teams);
    // The seats the guess gives a start, their teams and those starts.
    let seats: Vec<usize> = (0..teams.len())
        .filter(|&seat| guess[seat].is_some())
        .collect();
    let seated_teams: Vec<i32> = seats.iter().map(|&seat| teams[seat]).collect();
    let mut seating: Vec<usize> = guess.iter().flatten().copied().collect();
    let apart: Vec<Vec<i64>> = starts
        .iter()
        .map(|&(ax, ay)| {
            starts
                .iter()
                .map(|&(bx, by)| {
                    let (dx, dy) = (i64::from(ax) - i64::from(bx), i64::from(ay) - i64::from(by));
                    (dx * dx + dy * dy).isqrt()
                })
                .collect()
        })
        .collect();
    if starts.len() <= TRY_EVERY_SEATING_UP_TO {
        let mut best = (margins(&apart, &seated_teams, &seating), seating.clone());
        try_every_seating(&apart, &seated_teams, &mut Vec::new(), &mut best);
        seating = best.1;
    } else {
        improve(&apart, &seated_teams, &mut seating);
    }
    let mut assigned = vec![None; teams.len()];
    for (&seat, start) in seats.iter().zip(seating) {
        assigned[seat] = Some(start);
    }
    assigned
}

/// Whether seated players `a` and `b` are on the same team.
fn allied(teams: &[i32], a: usize, b: usize) -> bool {
    teams[a] != NO_TEAM && teams[a] == teams[b]
}

/// Each seated player's margin, smallest first: the distance to their
/// nearest enemy less the distance to their nearest ally (0 without one).
/// `apart` holds the distances between starts and `seating` each seated
/// player's start. A player with no enemy has no margin.
fn margins(apart: &[Vec<i64>], teams: &[i32], seating: &[usize]) -> Vec<i64> {
    let mut margins: Vec<i64> = (0..seating.len())
        .filter_map(|a| {
            let (mut ally, mut enemy) = (None::<i64>, None::<i64>);
            for b in (0..seating.len()).filter(|&b| b != a) {
                let distance = apart[seating[a]][seating[b]];
                let nearest = if allied(teams, a, b) {
                    &mut ally
                } else {
                    &mut enemy
                };
                *nearest = Some(nearest.map_or(distance, |near| near.min(distance)));
            }
            Some(enemy? - ally.unwrap_or(0))
        })
        .collect();
    margins.sort_unstable();
    margins
}

/// Try every seating that extends `seating`, keeping in `best` the first
/// whose margins beat it.
fn try_every_seating(
    apart: &[Vec<i64>],
    teams: &[i32],
    seating: &mut Vec<usize>,
    best: &mut (Vec<i64>, Vec<usize>),
) {
    if seating.len() == teams.len() {
        let tried = margins(apart, teams, seating);
        if tried > best.0 {
            *best = (tried, seating.clone());
        }
        return;
    }
    for start in 0..apart.len() {
        if !seating.contains(&start) {
            seating.push(start);
            try_every_seating(apart, teams, seating, best);
            seating.pop();
        }
    }
}

/// Swap two rivals' starts, or move a player to a free start, while either
/// raises the margins.
fn improve(apart: &[Vec<i64>], teams: &[i32], seating: &mut [usize]) {
    let mut score = margins(apart, teams, seating);
    loop {
        let mut improved = false;
        for a in 0..seating.len() {
            for b in a + 1..seating.len() {
                if allied(teams, a, b) {
                    continue;
                }
                seating.swap(a, b);
                let tried = margins(apart, teams, seating);
                if tried > score {
                    (score, improved) = (tried, true);
                } else {
                    seating.swap(a, b);
                }
            }
            for start in 0..apart.len() {
                if seating.contains(&start) {
                    continue;
                }
                let held = std::mem::replace(&mut seating[a], start);
                let tried = margins(apart, teams, seating);
                if tried > score {
                    (score, improved) = (tried, true);
                } else {
                    seating[a] = held;
                }
            }
        }
        if !improved {
            return;
        }
    }
}

/// The first guess: teams are seated largest first. The first takes the
/// start farthest from the middle of the map's starts; each later one the
/// free start farthest from every start taken. A team's other players then
/// take the free starts nearest its first. Seats past the map's starts get
/// none.
fn first_guess(starts: &[(i32, i32)], teams: &[i32]) -> Vec<Option<usize>> {
    let distance = |a: usize, b: (i64, i64)| {
        let (dx, dy) = (i64::from(starts[a].0) - b.0, i64::from(starts[a].1) - b.1);
        dx * dx + dy * dy
    };
    let cell = |index: usize| (i64::from(starts[index].0), i64::from(starts[index].1));

    // The seats of each team, in seat order; a seat on no team is its own.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (seat, &team) in teams.iter().enumerate() {
        let joined = (team != NO_TEAM)
            .then(|| groups.iter().position(|group| teams[group[0]] == team))
            .flatten();
        match joined {
            Some(group) => groups[group].push(seat),
            None => groups.push(vec![seat]),
        }
    }
    // Largest first; the sort is stable, so equal teams keep seat order.
    groups.sort_by_key(|group| std::cmp::Reverse(group.len()));

    let mut taken = vec![false; starts.len()];
    let mut assigned = vec![None; teams.len()];
    let count = starts.len().max(1) as i64;
    let middle = (
        starts.iter().map(|start| i64::from(start.0)).sum::<i64>() / count,
        starts.iter().map(|start| i64::from(start.1)).sum::<i64>() / count,
    );
    for group in groups {
        let free = |taken: &[bool]| {
            (0..starts.len())
                .filter(|&index| !taken[index])
                .collect::<Vec<_>>()
        };
        // The team's first start: away from the starts taken, or from the
        // middle while none is.
        let candidates = free(&taken);
        let Some(&first) = candidates.iter().max_by_key(|&&index| {
            let apart = (0..starts.len())
                .filter(|&other| taken[other])
                .map(|other| distance(index, cell(other)))
                .min()
                .unwrap_or_else(|| distance(index, middle));
            (apart, std::cmp::Reverse(index))
        }) else {
            break;
        };
        taken[first] = true;
        assigned[group[0]] = Some(first);
        for &seat in &group[1..] {
            let Some(&next) = free(&taken)
                .iter()
                .min_by_key(|&&index| (distance(index, cell(first)), index))
            else {
                break;
            };
            taken[next] = true;
            assigned[seat] = Some(next);
        }
    }
    assigned
}

/// Fill the Start column by team for the rows that play: the player's row
/// and every computer row with a player type. `starts` are the selected
/// map's start cells by start index; fewer than two leaves the page as it is.
pub fn apply_team_starts(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    starts: &[(i32, i32)],
) {
    if starts.len() < 2 {
        return;
    }
    let rows: Vec<usize> = std::iter::once(0)
        .chain(
            computer_rows(state, maps)
                .filter(|&idx| state.opponents[idx].row_type != SkirmishAiRowType::None)
                .map(|idx| idx + 1),
        )
        .collect();
    let teams: Vec<i32> = rows
        .iter()
        .map(|&row| seat_shown(state, row).team)
        .collect();
    // Free every start first, so no claim left on the page blocks a new one.
    for &row in &rows {
        apply_combo_selection(
            state,
            SkirmishComboId::Start(row),
            SkirmishComboItem::Start(LaunchStartPosition::Auto),
        );
    }
    for (&row, start) in rows.iter().zip(starts_by_team(starts, &teams)) {
        if let Some(start) = start.and_then(|start| u8::try_from(start).ok()) {
            pick(
                state,
                maps,
                SkirmishComboId::Start(row),
                SkirmishComboItem::Start(LaunchStartPosition::Position(start)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two teams of two on a square of four starts: allies share a side, and
    /// the teams take opposite sides.
    #[test]
    fn allies_take_neighbouring_starts_and_teams_keep_apart() {
        // West pair (0, 1) and east pair (2, 3), far apart.
        let starts = [(10, 10), (10, 30), (90, 10), (90, 30)];
        let seats = starts_by_team(&starts, &[0, 1, 0, 1]);
        let seat = |index: usize| seats[index].expect("every seat has a start");
        let west = |start: usize| start < 2;
        assert_eq!(west(seat(0)), west(seat(2)), "team A shares a side");
        assert_eq!(west(seat(1)), west(seat(3)), "team B shares a side");
        assert_ne!(west(seat(0)), west(seat(1)), "the teams face each other");
        let mut taken: Vec<usize> = (0..4).map(seat).collect();
        taken.sort_unstable();
        assert_eq!(taken, [0, 1, 2, 3], "no start is given twice");
    }

    /// Players on no team each stand alone, and seats past the map's starts
    /// are left for the launch to place.
    #[test]
    fn lone_players_spread_out_and_extra_seats_stay_unplaced() {
        let starts = [(10, 10), (12, 10), (90, 90)];
        let seats = starts_by_team(&starts, &[NO_TEAM, NO_TEAM, NO_TEAM, NO_TEAM]);
        assert!(seats[..3].iter().all(Option::is_some));
        assert_eq!(seats[3], None);
        // The first seat takes the start away from the middle, the second
        // the one farthest from it.
        assert_eq!((seats[0], seats[1]), (Some(2), Some(0)));
    }

    /// Whether no seated player starts nearer an enemy than their nearest ally.
    fn allies_nearest(starts: &[(i32, i32)], teams: &[i32], seats: &[Option<usize>]) -> bool {
        let cell = |seat: usize| starts[seats[seat].expect("every seat has a start")];
        let apart = |a: usize, b: usize| {
            let ((ax, ay), (bx, by)) = (cell(a), cell(b));
            (ax - bx).pow(2) + (ay - by).pow(2)
        };
        (0..teams.len()).all(|a| {
            let others = || (0..teams.len()).filter(move |&b| b != a);
            let ally = others()
                .filter(|&b| teams[b] == teams[a])
                .map(|b| apart(a, b))
                .min();
            let enemy = others()
                .filter(|&b| teams[b] != teams[a])
                .map(|b| apart(a, b))
                .min();
            ally <= enemy
        })
    }

    /// 3v3 on Kaliforn.mmx's six starts (waypoints 0-5): the two nearest
    /// starts are 46 cells apart, and filling a team from its first start
    /// gave them to rivals. Cutting the ring at its two widest gaps leaves
    /// every player nearest an ally.
    #[test]
    fn three_against_three_gives_no_player_a_nearer_enemy() {
        let starts = [
            (104, 109),
            (141, 66),
            (96, 22),
            (55, 59),
            (21, 97),
            (70, 140),
        ];
        let teams = [0, 1, 0, 1, 0, 1];
        assert!(allies_nearest(
            &starts,
            &teams,
            &starts_by_team(&starts, &teams)
        ));
    }

    /// A map past eight starts is too big to try every seating: 5v5 on a
    /// ring of ten uneven gaps still ends with every player nearest an ally,
    /// by swaps from the first guess.
    #[test]
    fn five_against_five_on_ten_starts_swaps_rivals_apart() {
        let starts = [
            (165, 147),
            (92, 180),
            (60, 169),
            (22, 117),
            (22, 83),
            (60, 31),
            (108, 20),
            (140, 31),
            (173, 67),
            (180, 100),
        ];
        let teams = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
        assert!(allies_nearest(
            &starts,
            &teams,
            &starts_by_team(&starts, &teams)
        ));
    }
}
