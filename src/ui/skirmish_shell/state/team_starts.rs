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

/// A start for each seat, by seat: `starts` are the map's start cells by
/// start index and `teams` each seat's team (`NO_TEAM` plays alone).
///
/// Teams are seated largest first. The first takes the start farthest from
/// the middle of the map's starts; each later one the free start farthest
/// from every start taken. A team's other players then take the free starts
/// nearest its first. Seats past the map's starts get none.
pub fn starts_by_team(starts: &[(i32, i32)], teams: &[i32]) -> Vec<Option<usize>> {
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
}
