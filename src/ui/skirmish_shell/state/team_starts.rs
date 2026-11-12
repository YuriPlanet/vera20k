//! VERA's Team Starts; gamemd's `0x102` has no counterpart. gamemd gives a
//! player without a start the free one farthest from the starts already
//! taken (`0x005D6890`), whatever the teams, so allies can land on opposite
//! sides of the map. Team Starts fills the page's Start column instead:
//! each team's players take starts next to each other, and the teams keep
//! as far from each other as the map allows. Distance is how far a ground
//! unit drives between two starts (`sim::pathfinding::drive_distance`), so
//! water and cliffs count; where the page cannot read the map's terrain
//! (the random map) it is the straight line. The picks are ordinary Start
//! choices, shown on the page for the player to change, and the launch
//! reads them as it reads any other. VERA's game room seats its teams the
//! same way (`app::shell_network_setup::teams`).

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
/// start index, `drive` how far a ground unit drives between them (`None`
/// where no route joins two; [`start_distances`] says how they are weighed),
/// `teams` each seat's team (`NO_TEAM` plays alone) and `fixed` the start a
/// seat keeps (none past its end, or past the map's starts), as a player in
/// VERA's room keeps the start they picked.
///
/// A seating is judged by each player's margin: how much farther their
/// nearest enemy starts than their nearest ally (a player on no team: how
/// far their nearest enemy starts). The pick raises the smallest margin as
/// far as the map allows, then the next smallest, and so on, so that a
/// player starts nearer an ally than an enemy wherever the map has room for
/// it. On a map of up to eight starts every seating is tried; on a bigger
/// one the first guess is improved by swapping two rivals' starts, or moving
/// a player to a free start, while that raises the margins. A kept start
/// never moves. Among equal seatings the first guess stays. Seats past the
/// map's starts get none.
pub fn starts_by_team(
    starts: &[(i32, i32)],
    drive: Option<&[Vec<Option<u32>>]>,
    teams: &[i32],
    fixed: &[Option<usize>],
) -> Vec<Option<usize>> {
    let fixed: Vec<Option<usize>> = (0..teams.len())
        .map(|seat| {
            fixed
                .get(seat)
                .copied()
                .flatten()
                .filter(|&start| start < starts.len())
        })
        .collect();
    let guess = first_guess(starts, teams, &fixed);
    // The seats the guess gives a start, their teams, the starts they keep
    // and those starts.
    let seats: Vec<usize> = (0..teams.len())
        .filter(|&seat| guess[seat].is_some())
        .collect();
    let seated_teams: Vec<i32> = seats.iter().map(|&seat| teams[seat]).collect();
    let kept: Vec<Option<usize>> = seats.iter().map(|&seat| fixed[seat]).collect();
    let mut seating: Vec<usize> = guess.iter().flatten().copied().collect();
    let apart = start_distances(starts, drive);
    if starts.len() <= TRY_EVERY_SEATING_UP_TO {
        let mut best = (margins(&apart, &seated_teams, &seating), seating.clone());
        try_every_seating(&apart, &seated_teams, &kept, &mut Vec::new(), &mut best);
        seating = best.1;
    } else {
        improve(&apart, &seated_teams, &kept, &mut seating);
    }
    let mut assigned = vec![None; teams.len()];
    for (&seat, start) in seats.iter().zip(seating) {
        assigned[seat] = Some(start);
    }
    assigned
}

/// How far apart each two starts are, by start index, in cells: the drive
/// where one was measured, the straight line where none was (a malformed
/// `drive` counts as none). A start no ground unit can drive to from another
/// counts as farther from it than any start one can, and such starts are
/// then ordered by the straight line.
fn start_distances(starts: &[(i32, i32)], drive: Option<&[Vec<Option<u32>>]>) -> Vec<Vec<i64>> {
    let straight = |a: usize, b: usize| {
        let (dx, dy) = (
            i64::from(starts[a].0) - i64::from(starts[b].0),
            i64::from(starts[a].1) - i64::from(starts[b].1),
        );
        (dx * dx + dy * dy).isqrt()
    };
    let drive = drive.filter(|drive| {
        drive.len() == starts.len() && drive.iter().all(|row| row.len() == starts.len())
    });
    let beyond_every_drive = 1 + drive.map_or(0, |drive| {
        drive
            .iter()
            .flatten()
            .flatten()
            .max()
            .map_or(0, |&d| i64::from(d))
    });
    (0..starts.len())
        .map(|a| {
            (0..starts.len())
                .map(|b| match drive.map(|drive| drive[a][b]) {
                    None => straight(a, b),
                    Some(Some(cells)) => i64::from(cells),
                    Some(None) => beyond_every_drive + straight(a, b),
                })
                .collect()
        })
        .collect()
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
/// whose margins beat it. A seated player with a `kept` start keeps it, and
/// no other takes it.
fn try_every_seating(
    apart: &[Vec<i64>],
    teams: &[i32],
    kept: &[Option<usize>],
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
    if let Some(start) = kept[seating.len()] {
        seating.push(start);
        try_every_seating(apart, teams, kept, seating, best);
        seating.pop();
        return;
    }
    for start in 0..apart.len() {
        if !seating.contains(&start) && !kept.contains(&Some(start)) {
            seating.push(start);
            try_every_seating(apart, teams, kept, seating, best);
            seating.pop();
        }
    }
}

/// Swap two rivals' starts, or move a player to a free start, while either
/// raises the margins. A player with a `kept` start stays on it.
fn improve(apart: &[Vec<i64>], teams: &[i32], kept: &[Option<usize>], seating: &mut [usize]) {
    let mut score = margins(apart, teams, seating);
    loop {
        let mut improved = false;
        for a in (0..seating.len()).filter(|&a| kept[a].is_none()) {
            for b in (a + 1..seating.len()).filter(|&b| kept[b].is_none()) {
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

/// The first guess: the seats with a `fixed` start keep it, then teams are
/// seated largest first. A team with a kept start gathers round it; the
/// first other takes the start farthest from the middle of the map's starts
/// while none is taken, and each later one the free start farthest from
/// every start taken. A team's other players then take the free starts
/// nearest its first. Seats past the map's starts get none.
fn first_guess(
    starts: &[(i32, i32)],
    teams: &[i32],
    fixed: &[Option<usize>],
) -> Vec<Option<usize>> {
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
    for &start in fixed.iter().flatten() {
        taken[start] = true;
    }
    let mut assigned = fixed.to_vec();
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
        // The team's first start: one of its players keeps, or away from
        // the starts taken, or from the middle while none is.
        let first = match group.iter().find_map(|&seat| assigned[seat]) {
            Some(kept) => kept,
            None => {
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
                first
            }
        };
        for &seat in &group {
            if assigned[seat].is_some() {
                continue;
            }
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
/// map's start cells by start index and `drive` how far a ground unit drives
/// between them, where the page could measure it; fewer than two starts
/// leaves the page as it is.
pub fn apply_team_starts(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    starts: &[(i32, i32)],
    drive: Option<&[Vec<Option<u32>>]>,
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
    for (&row, start) in rows.iter().zip(starts_by_team(starts, drive, &teams, &[])) {
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
        let seats = starts_by_team(&starts, None, &[0, 1, 0, 1], &[]);
        let seat = |index: usize| seats[index].expect("every seat has a start");
        let west = |start: usize| start < 2;
        assert_eq!(west(seat(0)), west(seat(2)), "team A shares a side");
        assert_eq!(west(seat(1)), west(seat(3)), "team B shares a side");
        assert_ne!(west(seat(0)), west(seat(1)), "the teams face each other");
        let mut taken: Vec<usize> = (0..4).map(seat).collect();
        taken.sort_unstable();
        assert_eq!(taken, [0, 1, 2, 3], "no start is given twice");
    }

    /// A player who picked a start keeps it, and the others are seated round
    /// it: team B's player holds a west start, so team B takes the west.
    #[test]
    fn a_kept_start_stays_and_its_team_gathers_round_it() {
        let starts = [(10, 10), (10, 30), (90, 10), (90, 30)];
        let seats = starts_by_team(&starts, None, &[0, 1, 0, 1], &[None, Some(1)]);
        assert_eq!(seats[1], Some(1));
        let west = |seat: usize| seats[seat].expect("every seat has a start") < 2;
        assert!(west(3), "team B joins its player in the west");
        assert!(!west(0) && !west(2), "team A takes the east");
    }

    /// Players on no team each stand alone, and seats past the map's starts
    /// are left for the launch to place.
    #[test]
    fn lone_players_spread_out_and_extra_seats_stay_unplaced() {
        let starts = [(10, 10), (12, 10), (90, 90)];
        let seats = starts_by_team(&starts, None, &[NO_TEAM, NO_TEAM, NO_TEAM, NO_TEAM], &[]);
        assert!(seats[..3].iter().all(Option::is_some));
        assert_eq!(seats[3], None);
        // The first seat takes the start away from the middle, the second
        // the one farthest from it.
        assert_eq!((seats[0], seats[1]), (Some(2), Some(0)));
    }

    /// Two islands of two starts each, with less water between them than
    /// land on either: allies take an island each, where straight lines
    /// would pair the starts facing each other across the water.
    #[test]
    fn allies_share_an_island_across_narrow_water() {
        // Island A holds starts 0 and 1, island B starts 2 and 3; 0 faces 2
        // and 1 faces 3 across 20 cells of water, and no route joins them.
        let starts = [(10, 10), (10, 70), (30, 10), (30, 70)];
        let drive = [
            vec![Some(0), Some(60), None, None],
            vec![Some(60), Some(0), None, None],
            vec![None, None, Some(0), Some(60)],
            vec![None, None, Some(60), Some(0)],
        ];
        let seats = starts_by_team(&starts, Some(&drive), &[0, 1, 0, 1], &[]);
        let island = |seat: usize| seats[seat].expect("every seat has a start") / 2;
        assert_eq!(island(0), island(2), "team A shares an island");
        assert_eq!(island(1), island(3), "team B shares an island");
        assert_ne!(island(0), island(1));
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
            &starts_by_team(&starts, None, &teams, &[])
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
            &starts_by_team(&starts, None, &teams, &[])
        ));
    }
}
