//! Original-executable component comparisons; scene inputs are supplied by the
//! native producer, independently of the ordinary Generate regression.
use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::Value;

use crate::map::rmg::grid::{CellRef, GridCell, RmgGrid};
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::theater::{RmgTileKeys, TheaterCliffRanges};
use crate::rng_continuation::MapGenRngContinuation;

use super::shore::{BlockPlacement, PlacementResult, SubTile, TileBlock, TileBlocks};
use super::{cliff_materialization, cliff_variants, island_passes, shore};

/// One cliff-selector `0x57B440` call: tile, origin, AL, caller out-bool.
type Placement = (i32, (i32, i32), bool, bool);

thread_local! {
    /// Placements observed while a comparison records; `None` otherwise.
    static PLACEMENTS: RefCell<Option<Vec<Placement>>> = const { RefCell::new(None) };
}

/// Test hook in the cliff selector after each shared placement call.
pub(super) fn observe_placement(tile: i32, origin: (i32, i32), result: &PlacementResult) {
    PLACEMENTS.with_borrow_mut(|log| {
        if let Some(log) = log {
            log.push((tile, origin, result.complete, result.admitted));
        }
    });
}

fn corpus() -> &'static Value {
    let doc = crate::test_fixture::json("tools/rmg_oracle/cliff_materialization.json");
    assert_eq!(doc["schema"], 2);
    doc
}

fn number(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}
fn bytes(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
fn word(raw: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap())
}
fn native_state(doc: &Value, index: &Value) -> MapGenRngContinuation {
    let raw = bytes(
        doc["rng_states_hex"][index.as_u64().unwrap() as usize]
            .as_str()
            .unwrap(),
    );
    assert_eq!(raw.len(), 0x3f4);
    let state = std::array::from_fn(|i| word(&raw, 12 + 4 * i));
    MapGenRngContinuation::from_native_parts(
        raw[0],
        state,
        word(&raw, 4) as usize,
        word(&raw, 8) as usize,
    )
}
fn compare_rng(doc: &Value, row: &Value, rng: RmgRng) {
    let mut stream = rng_from_before(doc, row);
    for draw in row["raw_draws"].as_array().unwrap() {
        assert_eq!(
            draw["stream"], "mapgen",
            "{}: original draw stream",
            row["id"]
        );
        assert_eq!(
            stream.next_u32(),
            draw["word"].as_u64().unwrap() as u32,
            "{}: ordered raw draw",
            row["id"]
        );
    }
    let actual = rng.into_continuation().into_native_parts();
    let expected = native_state(doc, &row["after_rng"]["mapgen"]).into_native_parts();
    assert_eq!(actual, expected, "{}: full MapGen state/cursor", row["id"]);
    assert_eq!(
        stream.into_continuation().into_native_parts(),
        expected,
        "{}: native raw draw count/order",
        row["id"]
    );
    for stream in ["main", "scenario"] {
        assert_eq!(
            row["before_rng"][stream], row["after_rng"][stream],
            "{}: unexpected {stream} draw",
            row["id"]
        );
    }
}
fn rng_from_before(doc: &Value, row: &Value) -> RmgRng {
    RmgRng::from_native_control(native_state(doc, &row["before_rng"]["mapgen"]))
}

pub(super) fn compare_variants(select: fn(i32, i32, &mut RmgRng) -> i32) {
    let doc = corpus();
    let base = number(&doc["inputs"]["bases"]["cliff"]);
    let mut compared = 0;
    for row in doc["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["input"]["kind"] == "variant")
    {
        if row["input"]["locked"] == true {
            // RmgRng models the unlocked generation stream. This native-only
            // control records the separate disabled-state boundary, not a
            // claim that Rust supports a locked generation cursor.
            assert_eq!(row["before_rng"], row["after_rng"]);
            assert!(row["raw_draws"].as_array().unwrap().is_empty());
            continue;
        }
        let mut random = rng_from_before(doc, row);
        let result = select(
            base.wrapping_add(number(&row["input"]["offset"])),
            base,
            &mut random,
        );
        assert_eq!(result, number(&row["eax"]), "{}", row["id"]);
        compare_rng(doc, row, random);
        compared += 1;
    }
    assert!(compared >= 40);
}

#[test]
fn scaled_range_matches_original_values_and_full_draw_states() {
    let doc = corpus();
    for row in doc["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["input"]["kind"] == "range")
    {
        let mut random = rng_from_before(doc, row);
        let bounds = &row["input"]["bounds"];
        assert_eq!(
            random.uniform(number(&bounds[0]), number(&bounds[1])),
            number(&row["eax"]),
            "{}",
            row["id"]
        );
        compare_rng(doc, row, random);
    }
}

fn ranges(doc: &Value, input: &Value) -> TheaterCliffRanges {
    let mut globals = doc["inputs"]["family_globals"].as_object().unwrap().clone();
    if input["all_unset"] == true {
        for value in globals.values_mut() {
            *value = Value::from(u32::MAX);
        }
    }
    if let Some(overrides) = input["family_globals"].as_object() {
        for (key, value) in overrides {
            globals.insert(key.clone(), value.clone());
        }
    }
    let base = |address: &str| u16::try_from(number(&globals[address])).ok();
    TheaterCliffRanges {
        cliff_set: base("00AA1020"),
        cliff_ramps: base("00ABBEBC"),
        water_cliffs: base("00AA101C"),
        destroyable_cliffs: base("00ABC2C8"),
        bridge_set: base("00AA0E28"),
        wood_bridge_set: base("00ABAD1C"),
        water_caves: base("00ABAD24"),
        waterfall_east: base("00AA073C"),
        waterfall_west: base("00ABB110"),
        waterfall_north: base("00AA10A0"),
        waterfall_south: base("00AA1050"),
    }
}

#[test]
fn shared_family_predicates_match_original_signed_and_terminal_cases() {
    let doc = corpus();
    for row in doc["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["input"]["kind"].as_str().unwrap().starts_with("family_"))
    {
        let input = &row["input"];
        let families = ranges(doc, input);
        let tile = input["tile"].as_i64().map(|v| v as i32).unwrap_or_else(|| {
            let key = format!("{:08X}", number(&input["family_global"]) as u32);
            number(&doc["inputs"]["family_globals"][key]).wrapping_add(number(&input["offset"]))
        });
        let sub = input["subtile"].as_i64().unwrap_or(0) as i32;
        let actual = if input["kind"] == "family_broad" {
            families.is_special_terrain_tile(tile, sub as u8)
        } else {
            families.is_on_bridge_ramp_tile(tile, sub)
        };
        assert_eq!(actual, number(&row["al"]) != 0, "{}", row["id"]);
    }
}

struct Blocks(BTreeMap<i32, TileBlock>);
impl TileBlocks for Blocks {
    fn block(&self, tile: i32) -> Option<&TileBlock> {
        self.0.get(&tile)
    }
}
fn blocks(doc: &Value) -> Blocks {
    Blocks(
        doc["inputs"]["registered_tiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tile| {
                let subtiles = tile["subtiles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| {
                        if s.is_null() {
                            None
                        } else {
                            Some(SubTile {
                                height: number(&s["height"]) as u8,
                                terrain: number(&s["terrain"]) as u8,
                                slope: number(&s["slope"]) as u8,
                            })
                        }
                    })
                    .collect();
                (
                    number(&tile["tile"]),
                    TileBlock {
                        width: number(&tile["width"]),
                        height: number(&tile["height"]),
                        subtiles,
                    },
                )
            })
            .collect(),
    )
}
fn xy(value: &Value) -> (i32, i32) {
    (number(&value[0]), number(&value[1]))
}
fn cell_fields(cell: GridCell) -> (i32, u8, u8, u8) {
    (cell.tile, cell.sub_tile, cell.level, cell.slope)
}
/// Native Cell+38/+11A/+11B/+11C as projected by the producer.
fn native_fields(value: &Value) -> (i32, u8, u8, u8) {
    (
        number(&value["tile"]),
        number(&value["subtile"]) as u8,
        number(&value["level"]) as u8,
        number(&value["slope"]) as u8,
    )
}
/// The shared border cell's fields and its last stamped coordinate.
fn border(grid: &RmgGrid) -> ((i32, u8, u8, u8), (i32, i32)) {
    (
        cell_fields(*grid.ref_cell(CellRef::Border)),
        grid.border_coord(),
    )
}
fn native_border(value: &Value) -> ((i32, u8, u8, u8), (i32, i32)) {
    (native_fields(value), xy(&value["coords"]))
}

#[test]
fn cliff_passes_match_original_cells_regions_border_and_rng() {
    let doc = corpus();
    let input = &doc["inputs"];
    let scene = &input["scene"];
    let width = number(&scene["scratch_stride"]) as usize;
    let map_min = number(&scene["map_size"][0]);
    let map_max = map_min + 2 * number(&scene["map_size"][1]);
    let blocks = blocks(doc);
    let ids = TileIds {
        cliffs: ranges(doc, &Value::Null),
        shore: number(&input["bases"]["shore"]),
        ..TileIds::from_keys(&RmgTileKeys::default())
    };
    let mut compared = 0;
    for row in doc["rows"].as_array().unwrap() {
        let prior = &row["input"];
        let kind = prior["kind"].as_str().unwrap();
        if ![
            "mask", "select", "stamp", "jitter", "restamp", "sweep", "joined",
        ]
        .contains(&kind)
            || prior["rust_grid_comparison"] == false
        {
            continue;
        }
        if kind == "stamp" && prior["scratch"] == false {
            // Standalone null-scratch57B440 is outside the borrowed-scratch
            // production API. The full materializer's local allocation is
            // compared below after original578E60 constructs its scratch.
            assert_eq!(row["scratch_final_pointer"], 0);
            continue;
        }
        let mut grid = RmgGrid::new(width, map_min, map_max);
        let actual_members: std::collections::BTreeSet<_> = grid.native_cells().collect();
        let native_members: std::collections::BTreeSet<_> = scene["admitted_cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(xy)
            .collect();
        assert_eq!(actual_members, native_members, "original568300 membership");
        let mut scratch = RmgScratch::new(width, map_min, map_max);
        for (x, y) in grid.native_cells() {
            let cell = grid.cell_native_mut(x, y);
            cell.tile = number(&scene["common"]["tile"]);
            cell.sub_tile = number(&scene["common"]["subtile"]) as u8;
            cell.level = number(&scene["common"]["level"]) as u8;
            cell.slope = number(&scene["common"]["slope"]) as u8;
        }
        if prior["tag_regions"] == true {
            for tag in scene["tagged_regions"].as_array().unwrap() {
                let (x, y) = xy(&tag["coords"]);
                scratch.get_mut(x, y).region = number(&tag["region"]);
            }
        }
        for item in prior["cells"].as_array().unwrap() {
            let (x, y) = xy(&item["coords"]);
            let Some(cell) = grid.get_mut(x, y) else {
                continue;
            };
            if let Some(family) = item["family"].as_str() {
                cell.tile =
                    number(&input["bases"][family]) + item["offset"].as_i64().unwrap_or(0) as i32;
            }
            for (field, target) in [
                ("subtile", &mut cell.sub_tile),
                ("level", &mut cell.level),
                ("slope", &mut cell.slope),
            ] {
                if let Some(value) = item[field].as_i64() {
                    *target = value as u8;
                }
            }
            if let Some(region) = item["region"].as_i64() {
                scratch.get_mut(x, y).region = region as i32;
            }
        }
        if let Some(disabled) = prior["disabled"].as_array() {
            for coords in disabled {
                let (x, y) = xy(coords);
                scratch.get_mut(x, y).shore_enable = false;
            }
        }
        if kind == "stamp" {
            if let Some(region) = prior["region"].as_i64() {
                for y in 16..19 {
                    for x in 16..18 {
                        scratch.get_mut(x, y).region = region as i32;
                    }
                }
            }
            if let Some(block) = prior["block"].as_str() {
                grid.cell_native_mut(16, 17).tile = number(&input["bases"]["cliff"]) + 1;
                scratch.get_mut(16, 17).region = if block == "hard" { 99 } else { 7 };
            }
        }
        let mut expected_cells: BTreeMap<_, _> = grid
            .native_cells()
            .map(|coords @ (x, y)| (coords, *grid.get(x, y).unwrap()))
            .collect();
        // The producer refuses any native scratch change outside region+38,
        // so every other modeled scratch field must stay as supplied.
        let mut expected_scratch = scratch.cells().to_vec();
        for change in row["changed_regions"].as_array().unwrap() {
            let (cx, cy) = xy(&change["coords"]);
            assert_eq!(
                scratch.get(cx, cy).region,
                number(&change["before"]),
                "{}: supplied region prior",
                row["id"]
            );
            expected_scratch[scratch.index(cx, cy)].region = number(&change["after"]);
        }
        for changed in row["changed_cells"].as_array().unwrap() {
            let cell = expected_cells.get_mut(&xy(&changed["coords"])).unwrap();
            assert_eq!(
                cell_fields(*cell),
                native_fields(&changed["before"]),
                "{}: supplied Cell prior",
                row["id"]
            );
            (cell.tile, cell.sub_tile, cell.level, cell.slope) = native_fields(&changed["after"]);
        }
        assert_eq!(
            border(&grid),
            native_border(&row["dummy"]["before"]),
            "{}: supplied shared border prior",
            row["id"]
        );
        PLACEMENTS.with_borrow_mut(|log| *log = Some(Vec::new()));
        let mut random = rng_from_before(doc, row);
        let (x, y) = prior["target"]
            .as_array()
            .map(|_| xy(&prior["target"]))
            .unwrap_or((16, 16));
        match kind {
            "mask" => assert_eq!(
                island_passes::ring_mask(&mut grid, &scratch, &ids, x, y),
                number(&row["al"]) as u8,
                "{}",
                row["id"]
            ),
            "select" => assert_eq!(
                cliff_materialization::select_cell(
                    &mut grid,
                    &mut scratch,
                    &ids,
                    &blocks,
                    &mut random,
                    x,
                    y,
                    7
                ),
                number(&row["al"]) != 0,
                "{}",
                row["id"]
            ),
            "stamp" => {
                let result = shore::place_block(
                    &mut grid,
                    &mut scratch,
                    &ids,
                    &blocks,
                    BlockPlacement {
                        tile: number(&input["bases"]["cliff"]),
                        origin: (x, y),
                        level: 0,
                        window: (0, 0),
                        region: prior["request_region"].as_i64().unwrap_or(7) as i32,
                        keep: prior["keep"] == true,
                    },
                );
                assert_eq!(
                    (result.complete, result.admitted),
                    (number(&row["al"]) != 0, number(&row["outbool"]) != 0),
                    "{}",
                    row["id"]
                );
            }
            "jitter" => cliff_materialization::jitter(&mut grid, &mut scratch, &ids, &mut random),
            "restamp" => cliff_variants::run(
                &mut grid,
                &mut scratch,
                &blocks,
                &mut random,
                number(&input["bases"]["cliff"]),
            ),
            "sweep" | "joined" => {
                let result = cliff_materialization::materialize(
                    &mut grid,
                    &mut scratch,
                    &ids,
                    &blocks,
                    &mut random,
                    -1,
                );
                if kind == "sweep" {
                    assert_eq!(result, number(&row["al"]) != 0, "{}", row["id"]);
                } else {
                    let phase = &row["phases"][0];
                    assert_eq!(phase["name"], "after_materialize");
                    for observed in phase["cells"].as_array().unwrap() {
                        let (px, py) = xy(&observed["coords"]);
                        let expected = (
                            number(&observed["tile"]),
                            number(&observed["subtile"]) as u8,
                            number(&observed["level"]) as u8,
                            number(&observed["slope"]) as u8,
                        );
                        assert_eq!(
                            cell_fields(*grid.get(px, py).unwrap()),
                            expected,
                            "{}: native materialize boundary",
                            row["id"]
                        );
                        assert_eq!(
                            scratch.get(px, py).region,
                            number(&observed["region"]),
                            "{}: native materialize region",
                            row["id"]
                        );
                    }
                    assert_eq!(
                        random.clone().into_continuation().into_native_parts(),
                        native_state(doc, &phase["rng"]["mapgen"]).into_native_parts(),
                        "{}: native materialize RNG boundary",
                        row["id"]
                    );
                    cliff_variants::run(
                        &mut grid,
                        &mut scratch,
                        &blocks,
                        &mut random,
                        number(&input["bases"]["cliff"]),
                    );
                }
            }
            _ => unreachable!(),
        }
        let placements = PLACEMENTS.with_borrow_mut(Option::take).unwrap();
        if kind != "stamp" {
            // Direct stamp rows compare AL/out-bool above; every other row
            // compares each selector placement, including soft stops.
            let expected: Vec<Placement> = row["stamps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|stamp| {
                    // 579B3A stores 1, so out-bool after means admitted.
                    assert_eq!(stamp["outbool_before"], 1, "{}", row["id"]);
                    (
                        number(&stamp["tile"]),
                        xy(&stamp["origin"]),
                        number(&stamp["al"]) != 0,
                        number(&stamp["outbool"]) != 0,
                    )
                })
                .collect();
            assert_eq!(
                placements, expected,
                "{}: ordered 57B440 results",
                row["id"]
            );
        }
        for ((cx, cy), expected) in expected_cells {
            assert_eq!(
                *grid.get(cx, cy).unwrap(),
                expected,
                "{} cell({cx},{cy})",
                row["id"]
            );
        }
        assert_eq!(
            scratch.cells(),
            expected_scratch,
            "{} scratch fields and untouched state",
            row["id"]
        );
        assert_eq!(
            border(&grid),
            native_border(&row["dummy"]["after"]),
            "{} shared border fields and coordinates",
            row["id"]
        );
        compare_rng(doc, row, random);
        compared += 1;
    }
    assert!(compared > 40);
}
