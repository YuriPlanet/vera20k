//! `MapClass::RevealArea @ 0x005678E0` and `RevealShroud @ 0x005673A0`
//! against `tools/spatial_oracle/reveal_area.json` (`--check` regenerates
//! it): the cells each walk hands its leaf, in order, and the house gate.

use super::*;
use crate::sim::intern::StringInterner;
use serde_json::Value;

/// The fog grid the rows' walks land in. It holds the whole fixture `Size=`
/// diamond, so only the diamond bounds the walks.
const GRID: (u16, u16) = (128, 128);

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/reveal_area.json")
}

fn rows(corpus: &Value) -> &[Value] {
    corpus["rows"].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

/// A corpus cell list, `x,y` pairs in order.
fn cells(text: &Value) -> Vec<(u16, u16)> {
    text.as_str()
        .unwrap()
        .split_whitespace()
        .map(|pair| {
            let (x, y) = pair.split_once(',').unwrap();
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect()
}

/// The row's cell levels (`x,y=level` items) as a [`GRID`] height grid.
fn height_grid(levels: &Value) -> Vec<u8> {
    let width = usize::from(GRID.0);
    let mut grid = vec![0; width * usize::from(GRID.1)];
    for item in levels.as_str().unwrap().split_whitespace() {
        let (cell, level) = item.split_once('=').unwrap();
        let (x, y) = cell.split_once(',').unwrap();
        let (x, y): (usize, usize) = (x.parse().unwrap(), y.parse().unwrap());
        grid[y * width + x] = level.parse::<i8>().unwrap() as u8;
    }
    grid
}

/// The player's own walks: RevealArea's leaf cells (both finals) and
/// RevealShroud's Unshroud cells (FireAt's arguments), in order, against
/// [`collect_reveal_cells`] at the row's centre, height and radius in the
/// row's map `Size=`, with the line of sight when both the by-height
/// argument and `RevealByHeight=` are set. The rows cover every radius
/// (0 and 11 and more included), heights that lift the centre, centres at
/// each side of the diamond and lifted off it, and a ridge that blocks the
/// line of sight at three viewer heights.
#[test]
fn the_walk_matches_native() {
    let corpus = corpus();
    let mut compared = 0;
    for row in rows(&corpus).iter().filter(|row| row["house"] == "player") {
        let heights = height_grid(&row["levels"]);
        let walked = collect_reveal_cells(
            int(&row["center"][0]) as u16,
            int(&row["center"][1]) as u16,
            int(&row["radius"]) as u16,
            int(&row["z"]),
            row["by_height"] == true && row["reveal_by_height"] == true,
            Some(CellLevels::Plane(&heights)),
            GRID,
            Some((int(&row["size"][0]), int(&row["size"][1]))),
        );
        let native = if row["function"] == "shroud" {
            assert_eq!(row["leaf"], "", "{row}");
            cells(&row["unshroud"])
        } else {
            assert_eq!(row["unshroud"], "", "{row}");
            if !walked.is_empty() {
                assert_eq!(row["leaf_house"], "player", "{row}");
                assert_eq!(row["leaf_final"], row["final"], "{row}");
            }
            cells(&row["leaf"])
        };
        assert_eq!(walked, native, "{row}");
        compared += 1;
    }
    assert_eq!(compared, 68);
}

/// Original567CB5 sign-extends Cell+11B. A negative terrain byte must not
/// become a tall unsigned obstruction; positive4/127 controls exercise the
/// opposite branch. These are original executable walks, not fitted discs.
#[test]
fn native_trigger_reveal_signed_los_matches_original_walk() {
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let native = corpus
        .iter()
        .find(|row| row["case_kind"] == "trigger_reveal")
        .unwrap();
    let controls = native["los_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 4);
    for row in controls {
        let heights = height_grid(&row["levels"]);
        let actual = collect_reveal_cells(
            int(&row["center"][0]) as u16,
            int(&row["center"][1]) as u16,
            int(&row["radius"]) as u16,
            int(&row["z"]),
            row["by_height"] == true && row["reveal_by_height"] == true,
            Some(CellLevels::Plane(&heights)),
            GRID,
            Some((int(&row["size"][0]), int(&row["size"][1]))),
        );
        assert_eq!(actual, cells(&row["leaf"]), "{}", row["name"]);
    }
}

/// The house gate (`0x00567AB0..0x00567B12`): another house's reveal maps
/// the player's cells only when that house, asked as IsAlliedWith's `this`
/// (`0x004F9A50`), names the player its ally and `AllyReveal=` is set; the
/// leaf is then handed the player. An alliance only the player declares
/// does not admit it.
#[test]
fn the_house_gate_matches_native() {
    let corpus = corpus();
    let mut interner = StringInterner::new();
    let player = interner.intern("Player");
    let other = interner.intern("Other");
    let mut compared = 0;
    for row in rows(&corpus).iter().filter(|row| row["house"] == "other") {
        assert_eq!(
            row["calls"],
            serde_json::json!([["allied", "other", "player"]]),
            "{row}"
        );
        let leaf = cells(&row["leaf"]);
        if !leaf.is_empty() {
            assert_eq!(row["leaf_house"], "player", "{row}");
        }
        let config = VisionConfig {
            ally_reveal: row["ally_reveal"] == true,
            ..VisionConfig::default()
        };
        for (asker, named) in [("OTHER", "PLAYER"), ("PLAYER", "OTHER")] {
            let fog = FogState::default();
            let mut alliances = HouseAllianceMap::new();
            if row["allied"] == true {
                alliances.insert(asker.to_string(), [named.to_string()].into());
            }
            let admitted = direct_reveal_viewers(&fog, &alliances, other, &config, &interner)
                .contains(&player);
            let expected = !leaf.is_empty() && asker == "OTHER";
            assert_eq!(admitted, expected, "{asker} {row}");
        }
        compared += 1;
    }
    assert_eq!(compared, 4);
}
