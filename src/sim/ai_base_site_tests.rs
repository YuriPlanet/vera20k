//! Native comparisons for the site search (`tools/ai_base_building_oracle.py`).

use super::*;
use serde_json::Value;

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/ai_base_building_oracle.json",
    ))
    .unwrap()
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i64 {
    value.as_i64().unwrap()
}

fn cell(value: &Value) -> (i16, i16) {
    (int(&value[0]) as i16, int(&value[1]) as i16)
}

#[test]
fn the_ordinary_key_matches_native() {
    let oracle = oracle();
    for row in rows(&oracle, "key") {
        let key = ordinary_key(
            int(&row["index"]) as i32,
            cell(&row["cell"]),
            cell(&row["center"]),
        );
        assert_eq!(i64::from(key), int(&row["key"]), "{row}");
    }
}

#[test]
fn the_site_sort_matches_the_retail_qsort() {
    let oracle = oracle();
    for row in rows(&oracle, "sort") {
        let mut records: Vec<(i32, usize)> = row["keys"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(tag, key)| (int(key) as i32, tag))
            .collect();
        crate::util::retail_pointer_sort::sort_by(&mut records, |a, b| a.0.cmp(&b.0));
        let order: Vec<i64> = records.iter().map(|&(_, tag)| tag as i64).collect();
        let expected: Vec<i64> = row["order"].as_array().unwrap().iter().map(int).collect();
        assert_eq!(order, expected, "{}", row["keys"]);
    }
}

#[test]
fn the_away_direction_matches_native_for_every_neighbour_sum() {
    let oracle = oracle();
    let directions = rows(&oracle, "direction");
    assert_eq!(directions.len(), 48);
    for row in directions {
        let sum = cell(&row["sum"]);
        let facing = facing16_from_delta(-i32::from(sum.0), -i32::from(sum.1));
        assert_eq!(i64::from(facing), int(&row["facing"]), "{row}");
        assert_eq!(away_direction(sum) as i64, int(&row["direction"]), "{row}");
    }
}

/// A map holding one reserved cell; any other query is a failure.
struct OneReserved {
    cell: Option<(i16, i16)>,
    lookups: usize,
}

impl SiteWorld for OneReserved {
    fn reserved(&mut self, cell: (i16, i16)) -> bool {
        self.lookups += 1;
        self.cell == Some(cell)
    }

    fn level(&mut self, _cell: (i16, i16)) -> i32 {
        unreachable!("reserved-near reads no level")
    }

    fn clear(&mut self, _rect: CellRect) -> bool {
        unreachable!("reserved-near tests no occupancy")
    }

    fn can_place(&mut self, _site: (i16, i16)) -> bool {
        unreachable!("reserved-near tests no placement")
    }
}

#[test]
fn reserved_near_covers_the_native_bounds() {
    let oracle = oracle();
    for row in rows(&oracle, "reserved_near") {
        let (width, height, spacing) = (
            int(&row["width"]) as i32,
            int(&row["height"]) as i32,
            int(&row["spacing"]) as i32,
        );
        let site = cell(&row["site"]);
        if let Some(answers) = row["answers"].as_str() {
            let first = cell(&row["first"]);
            let columns = int(&row["columns"]) as usize;
            for (slot, answer) in answers.chars().enumerate() {
                let offset = (
                    first.0 + (slot % columns) as i16,
                    first.1 + (slot / columns) as i16,
                );
                let mut world = OneReserved {
                    cell: Some((site.0 + offset.0, site.1 + offset.1)),
                    lookups: 0,
                };
                assert_eq!(
                    reserved_near_in(&mut world, spacing, width, height, site, true),
                    answer == '1',
                    "{width}x{height} spacing {spacing}, reservation at {offset:?}"
                );
            }
        } else {
            // A reservation of another house, or a campaign.
            let mut world = OneReserved {
                cell: None,
                lookups: 0,
            };
            let game_mode_nonzero = int(&row["game_mode"]) != 0;
            assert_eq!(
                reserved_near_in(&mut world, spacing, width, height, site, game_mode_nonzero),
                row["answer"].as_bool().unwrap(),
                "{row}"
            );
            assert_eq!(world.lookups as i64, int(&row["lookups"]), "{row}");
        }
    }
}

/// The native transcript of one search, answered back in order.
struct Replay<'a> {
    label: &'a str,
    events: Vec<(&'a str, Vec<i64>)>,
    next: usize,
    house: i64,
}

impl<'a> Replay<'a> {
    fn new(row: &'a Value) -> Self {
        let events = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| {
                let mut words = event.as_str().unwrap().split(' ');
                let kind = words.next().unwrap();
                (kind, words.map(|word| word.parse().unwrap()).collect())
            })
            .collect();
        Self {
            label: row["label"].as_str().unwrap(),
            events,
            next: 0,
            house: int(&row["house"]),
        }
    }

    fn take(&mut self, kind: &str) -> &[i64] {
        let Some((native, values)) = self.events.get(self.next) else {
            panic!(
                "{}: native made no {kind} call after event {}",
                self.label, self.next
            );
        };
        assert_eq!(*native, kind, "{}: event {}", self.label, self.next);
        self.next += 1;
        values
    }

    fn cell(&mut self, cell: (i16, i16)) -> (bool, i32) {
        let label = self.label;
        let at = self.next;
        let values = self.take("cell");
        assert_eq!(
            (values[0], values[1]),
            (i64::from(cell.0), i64::from(cell.1)),
            "{label}: cell lookup {at}"
        );
        (values[2] != 0, values[3] as i32)
    }
}

impl SiteWorld for Replay<'_> {
    fn reserved(&mut self, cell: (i16, i16)) -> bool {
        self.cell(cell).0
    }

    fn level(&mut self, cell: (i16, i16)) -> i32 {
        self.cell(cell).1
    }

    fn clear(&mut self, rect: CellRect) -> bool {
        let (label, at, house) = (self.label, self.next, self.house);
        let values = self.take("clear");
        assert_eq!(
            &values[..5],
            &[
                i64::from(rect.x),
                i64::from(rect.y),
                i64::from(rect.width),
                i64::from(rect.height),
                house
            ],
            "{label}: occupancy test {at}"
        );
        values[5] != 0
    }

    fn can_place(&mut self, site: (i16, i16)) -> bool {
        let (label, at) = (self.label, self.next);
        let values = self.take("place");
        assert_eq!(
            (values[0], values[1]),
            (i64::from(site.0), i64::from(site.1)),
            "{label}: placement test {at}"
        );
        values[2] != 0
    }
}

/// The row's base centre, chosen by the house as the search reads it.
fn origin(row: &Value) -> (i16, i16) {
    let unsigned = |cell: (i16, i16)| (cell.0 as u16, cell.1 as u16);
    let mut house = crate::sim::house_state::HouseState::new(
        crate::sim::intern::InternedId::default(),
        0,
        None,
        false,
        0,
        10,
    );
    house.alternate_base_center = unsigned(cell(&row["alternate"]));
    house.base_center = Some(unsigned(cell(&row["base"])));
    signed(house.base_origin())
}

/// Replays each search row with `key`; the number of failed searches.
fn replay_searches(searches: &[Value], key: impl Fn(&Value) -> Option<CoverageGrid>) -> usize {
    let mut failed = 0;
    for row in searches {
        let perimeter: Vec<(i16, i16)> = row["perimeter"]
            .as_array()
            .unwrap()
            .iter()
            .map(cell)
            .collect();
        let grid = key(row);
        let search = SiteSearch {
            center: cell(&row["center"]),
            origin: origin(row),
            perimeter: &perimeter,
            spacing: int(&row["spacing"]) as i32,
            width: int(&row["width"]) as i32,
            height: int(&row["height"]) as i32,
            extra_border: row["protect"].as_bool().unwrap() || row["extra"].as_bool().unwrap(),
            game_mode_nonzero: int(&row["game_mode"]) != 0,
            key: grid
                .as_ref()
                .map_or(SiteKey::Ordinary, |grid| SiteKey::Defense {
                    grid,
                    argument: int(&row["argument"]) as i32,
                }),
        };
        let mut replay = Replay::new(row);
        let site = ordinary_site(&mut replay, &search);
        assert_eq!(site, cell(&row["answer"]), "{}", replay.label);
        assert_eq!(
            replay.next,
            replay.events.len(),
            "{}: native made more calls",
            replay.label
        );
        // A failed search ran its second, identical pass natively.
        if site == (0, 0) && search.center != (0, 0) {
            assert_eq!(int(&row["passes"]), 2, "{}", replay.label);
            failed += 1;
        }
    }
    failed
}

#[test]
fn whole_searches_repeat_the_native_transcripts() {
    let oracle = oracle();
    let searches = rows(&oracle, "site");
    let failed = replay_searches(searches, |_| None);
    assert!(
        searches.len() >= 60 && failed >= 10,
        "{} searches, {failed} failed",
        searches.len()
    );
}

/// The same searches with the base defense key: the coverage grid and the
/// quadrant argument order the perimeter
/// (`tools/ai_base_defense_oracle.py`).
#[test]
fn defense_key_searches_repeat_the_native_transcripts() {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/ai_base_defense_oracle.json",
    ))
    .unwrap();
    let searches = rows(&oracle, "site");
    let failed = replay_searches(searches, |row| {
        let bounds = row["rect"].as_array().unwrap();
        let bounds = (
            int(&bounds[0]) as i32,
            int(&bounds[1]) as i32,
            int(&bounds[2]) as i32,
            int(&bounds[3]) as i32,
        );
        let mut grid = CoverageGrid::new(bounds);
        for (slot, value) in row["grid"].as_array().unwrap().iter().enumerate() {
            grid.cells_mut()[slot] = int(value) as i32;
        }
        Some(grid)
    });
    assert!(
        searches.len() >= 30 && failed >= 1,
        "{} searches, {failed} failed",
        searches.len()
    );
}
