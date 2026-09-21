//! Native load-time LAT (Lateral Terrain) transition fixup.
//!
//! Theater `[General]` tileset ordinals are resolved to absolute first tile
//! ids. Every cell then runs the Rough, Sand, Green, and Pave groups in that
//! fixed order, using direct N/E/S/W mask bits, followed by ramp smoothing.
//! Authored maps make two in-place recalc sweeps so a cell's slope-neighbour
//! reads observe the same progressively populated state as the native loader.
//!
//! ## Dependency rules
//! - Part of map/ -- depends on rules/ (ini_parser) and map/theater.

use std::collections::HashMap;

use crate::map::map_file::MapCell;
use crate::map::theater::TilesetLookup;
use crate::rules::ini_parser::IniFile;

const LAT_LAST_OFFSET: i32 = 0xF;
const EDGE_SENTINEL: i32 = 0;
const NO_TILE: i32 = 0xFFFF;
/// Direct bit order: N, E, S, W.
const CARDINAL_OFFSETS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
const RAMP_BASE_LAST_OFFSET: i32 = 19;
const RAMP_SMOOTH_LAST_OFFSET: i32 = 11;

const GREEN_EXEMPTIONS: [(&str, i32); 2] = [("ShorePieces", 0x29), ("WaterBridge", 1)];
const PAVE_EXEMPTIONS: [(&str, i32); 3] = [
    ("MiscPaveTile", 0xD),
    ("Medians", 0xD),
    ("PavedRoads", 0x14),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TileRange {
    first: i32,
    last: i32,
}

impl TileRange {
    fn new(first: i32, last_offset: i32) -> Self {
        Self {
            first,
            last: first + last_offset,
        }
    }

    fn contains(self, tile: i32) -> bool {
        self.first <= tile && tile <= self.last
    }
}

/// One LAT group expressed entirely as absolute tile ids.
#[derive(Debug, Clone)]
pub struct LatGroundType {
    pub name: String,
    pub base_tile: i32,
    pub lat_base: i32,
    exemptions: Vec<TileRange>,
    enable_guard: LatEnableGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatEnableGuard {
    Always,
    LatBaseDefined,
}

impl LatGroundType {
    fn is_enabled(&self) -> bool {
        self.enable_guard == LatEnableGuard::Always || self.lat_base != -1
    }

    fn contains(&self, tile: i32) -> bool {
        tile == self.base_tile
            || (self.lat_base != -1
                && (self.lat_base..=self.lat_base + LAT_LAST_OFFSET).contains(&tile))
    }

    fn exempts(&self, tile: i32) -> bool {
        self.exemptions.iter().any(|range| range.contains(tile))
    }
}

/// LAT groups in fixed Rough -> Sand -> Green -> Pave order.
#[derive(Debug, Clone)]
pub struct LatConfig {
    pub grounds: Vec<LatGroundType>,
}

/// Absolute theater tile bases consumed by the slope half of recalc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlopeFixupConfig {
    pub ramp_base: i32,
    pub ramp_smooth: i32,
}

fn tileset_bounds(
    lookup: &TilesetLookup,
    ordinal: Option<i32>,
) -> Option<&crate::map::theater::TilesetBounds> {
    let ordinal = ordinal?;
    (ordinal >= 0)
        .then(|| lookup.bounds().get(ordinal as usize))
        .flatten()
}

/// Parse and resolve the theater's load-time LAT configuration.
///
/// Missing group keys retain the native `-1` globals. Rough always executes;
/// Sand, Green, and Pave are gated only by their LAT global. Missing exemption
/// keys disable only that range. `*ConnectTo` keys are intentionally ignored.
pub fn parse_lat_config(ini_data: &[u8], lookup: &TilesetLookup) -> LatConfig {
    let Ok(ini) = IniFile::from_bytes(ini_data) else {
        return LatConfig {
            grounds: Vec::new(),
        };
    };
    let general = ini.section("General");
    if general.is_none() {
        log::info!("LAT: no [General] section in theater INI");
    }

    // The array type keeps the empty Rough/Sand slices typed without a
    // speculative shared terrain abstraction.
    let definitions: [(&str, &str, &str, &[(&str, i32)], LatEnableGuard); 4] = [
        (
            "Rough",
            "RoughTile",
            "ClearToRoughLat",
            &[],
            LatEnableGuard::Always,
        ),
        (
            "Sand",
            "SandTile",
            "ClearToSandLat",
            &[],
            LatEnableGuard::LatBaseDefined,
        ),
        (
            "Green",
            "GreenTile",
            "ClearToGreenLat",
            &GREEN_EXEMPTIONS,
            LatEnableGuard::LatBaseDefined,
        ),
        (
            "Pave",
            "PaveTile",
            "ClearToPaveLat",
            &PAVE_EXEMPTIONS,
            LatEnableGuard::LatBaseDefined,
        ),
    ];

    let mut grounds = Vec::with_capacity(definitions.len());
    for (name, base_key, lat_key, exemption_defs, enable_guard) in definitions {
        let base_tile = tileset_bounds(
            lookup,
            general.and_then(|section| section.get_i32(base_key)),
        )
        .map_or(-1, |bounds| i32::from(bounds.start));
        let lat_base = tileset_bounds(lookup, general.and_then(|section| section.get_i32(lat_key)))
            .map_or(-1, |bounds| i32::from(bounds.start));
        let exemptions = exemption_defs
            .iter()
            .filter_map(|&(key, last_offset)| {
                let bounds =
                    tileset_bounds(lookup, general.and_then(|section| section.get_i32(key)))?;
                // Retail Lunar leaves these two ordinal sections present with
                // zero tiles, while native theater init clears their effective
                // LAT globals. Do not expand their shared next-tile start into
                // bogus Green exemption spans.
                if name == "Green" && bounds.count == 0 {
                    return None;
                }
                Some(TileRange::new(i32::from(bounds.start), last_offset))
            })
            .collect::<Vec<_>>();

        grounds.push(LatGroundType {
            name: name.to_string(),
            base_tile,
            lat_base,
            exemptions,
            enable_guard,
        });
    }

    log::info!("LAT: {} ground groups configured", grounds.len());
    LatConfig { grounds }
}

fn neighbor_tile(cells: &[MapCell], by_coord: &HashMap<(u16, u16), usize>, x: i32, y: i32) -> i32 {
    if x < 0 || y < 0 || x > i32::from(u16::MAX) || y > i32::from(u16::MAX) {
        return EDGE_SENTINEL;
    }
    by_coord
        .get(&(x as u16, y as u16))
        .map_or(EDGE_SENTINEL, |&index| {
            let tile = cells[index].tile_index;
            if tile < 0 || tile == NO_TILE {
                EDGE_SENTINEL
            } else {
                tile
            }
        })
}

/// Apply the load-time LAT half in place, preserving cell and group order.
///
/// The third parameter remains for the existing map-build API; all lookup
/// data needed by the pass has already been resolved into `lat_config`.
#[cfg(test)]
pub fn apply_lat(cells: &mut [MapCell], lat_config: &LatConfig, _lookup: &TilesetLookup) {
    if lat_config.grounds.is_empty() {
        return;
    }

    let by_coord = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| ((cell.rx, cell.ry), index))
        .collect::<HashMap<_, _>>();
    let mut changes = 0u32;

    for cell_index in 0..cells.len() {
        changes += apply_lat_cell(cells, &by_coord, cell_index, lat_config);
    }

    log::info!(
        "LAT: rewrote {} group/cell transitions across {} cells",
        changes,
        cells.len()
    );
}

fn apply_lat_cell(
    cells: &mut [MapCell],
    by_coord: &HashMap<(u16, u16), usize>,
    cell_index: usize,
    lat_config: &LatConfig,
) -> u32 {
    let x = i32::from(cells[cell_index].rx);
    let y = i32::from(cells[cell_index].ry);
    let cardinal = CARDINAL_OFFSETS.map(|(dx, dy)| neighbor_tile(cells, by_coord, x + dx, y + dy));
    let old_tile = cells[cell_index].tile_index;
    let new_tile = lat_fixed_tile(old_tile, cardinal, lat_config);
    cells[cell_index].tile_index = new_tile;
    u32::from(old_tile != new_tile)
}

/// Runtime form of the LAT half for one CellClass. Native changes only the tile
/// identity here; the owning caller decides whether/when RecalcAttributes runs.
pub(crate) fn lat_fixed_tile(
    tile: i32,
    cardinal_tiles: [i32; 4],
    lat_config: &LatConfig,
) -> i32 {
    lat_fixed_tile_live(tile, lat_config, || cardinal_tiles)
}

/// Live-neighbor form of `CellClass__ApplyLAT_and_SlopeFixup @ 0x0047CA80`.
/// Native checks each Rough/Sand/Green/Pave receiver guard before issuing that
/// group's N/E/S/W `GetCell` calls, so the callback must not be evaluated for
/// a group that does not own the current tile.
pub(crate) fn lat_fixed_tile_live(
    mut tile: i32,
    lat_config: &LatConfig,
    mut cardinal_tiles: impl FnMut() -> [i32; 4],
) -> i32 {
    for ground in &lat_config.grounds {
        if !ground.is_enabled() || !ground.contains(tile) {
            continue;
        }
        let cardinal_tiles = cardinal_tiles();
        let mut mask = 0u8;
        for (bit, neighbor) in cardinal_tiles.into_iter().enumerate() {
            if !ground.contains(neighbor) && !ground.exempts(neighbor) {
                mask |= 1u8 << bit;
            }
        }
        tile = if mask == 0 {
            ground.base_tile
        } else {
            ground.lat_base + i32::from(mask)
        };
    }
    tile
}

fn ramp_range_contains(tile: i32, base: i32, last_offset: i32) -> bool {
    let end = if base == -1 { -1 } else { base + last_offset };
    base <= tile && tile <= end
}

/// Resolve the slope half for one cell from stored cardinal-neighbour slope
/// bytes in `[N, E, S, W]` order. A tile outside both ramp ranges is retained.
pub(crate) fn slope_fixed_tile(
    tile: i32,
    slope: u8,
    cardinal_slopes: [u8; 4],
    config: SlopeFixupConfig,
) -> i32 {
    slope_fixed_tile_live(tile, slope, config, |slot| cardinal_slopes[slot])
}

/// Live-neighbor slope half of
/// `CellClass__ApplyLAT_and_SlopeFixup @ 0x0047CA80`.
/// The ramp-range guard precedes every lookup, and slopes 1..=4 read only their
/// verified ordered pair: W/E, N/S, E/W, or S/N respectively.
pub(crate) fn slope_fixed_tile_live(
    tile: i32,
    slope: u8,
    config: SlopeFixupConfig,
    mut neighbor_slope: impl FnMut(usize) -> u8,
) -> i32 {
    if !ramp_range_contains(tile, config.ramp_base, RAMP_BASE_LAST_OFFSET)
        && !ramp_range_contains(tile, config.ramp_smooth, RAMP_SMOOTH_LAST_OFFSET)
    {
        return tile;
    }

    let neighbor_pair = match slope {
        1 => Some((3, 1)), // W, E
        2 => Some((0, 2)), // N, S
        3 => Some((1, 3)), // E, W
        4 => Some((2, 0)), // S, N
        _ => None,
    };
    let mask = neighbor_pair.map_or(0, |(first, second)| {
        u8::from(neighbor_slope(first) == 0) | (u8::from(neighbor_slope(second) == 0) << 1)
    });

    if mask != 0 {
        let block_offset = match slope {
            1 => mask - 1,
            2 => mask + 2,
            3 => mask + 5,
            4 => mask + 8,
            _ => unreachable!("only slopes 1..=4 can produce a smoothing mask"),
        };
        config.ramp_smooth + i32::from(block_offset)
    } else {
        // 47D1C0..47D1C8 zero-extends11C before DWORD LEA arithmetic.
        // The subtraction does not wrap in a byte: slope0 selects base-1.
        config.ramp_base.wrapping_add(i32::from(slope)).wrapping_sub(1)
    }
}

fn neighbor_slope(slopes: &[u8], by_coord: &HashMap<(u16, u16), usize>, x: i32, y: i32) -> u8 {
    if x < 0 || y < 0 || x > i32::from(u16::MAX) || y > i32::from(u16::MAX) {
        return 0;
    }
    by_coord
        .get(&(x as u16, y as u16))
        .map_or(0, |&index| slopes[index])
}

/// Run the two authored-map recalc sweeps and return the final stored slope
/// byte for each cell. `pristine_slope` must read variant-zero TMP metadata.
pub fn apply_load_recalc_sweeps(
    cells: &mut [MapCell],
    lat_config: &LatConfig,
    slope_config: SlopeFixupConfig,
    pristine_slope: &mut dyn FnMut(i32, u8) -> u8,
) -> Vec<u8> {
    apply_recalc_sweeps(cells, lat_config, slope_config, 2, pristine_slope)
}

fn apply_recalc_sweeps(
    cells: &mut [MapCell],
    lat_config: &LatConfig,
    slope_config: SlopeFixupConfig,
    sweep_count: usize,
    pristine_slope: &mut dyn FnMut(i32, u8) -> u8,
) -> Vec<u8> {
    let by_coord = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| ((cell.rx, cell.ry), index))
        .collect::<HashMap<_, _>>();
    let mut slopes = vec![0u8; cells.len()];

    for _ in 0..sweep_count {
        for cell_index in 0..cells.len() {
            let x = i32::from(cells[cell_index].rx);
            let y = i32::from(cells[cell_index].ry);
            slopes[cell_index] =
                pristine_slope(cells[cell_index].tile_index, cells[cell_index].sub_tile);
            apply_lat_cell(cells, &by_coord, cell_index, lat_config);
            let cardinal =
                CARDINAL_OFFSETS.map(|(dx, dy)| neighbor_slope(&slopes, &by_coord, x + dx, y + dy));
            cells[cell_index].tile_index = slope_fixed_tile(
                cells[cell_index].tile_index,
                slopes[cell_index],
                cardinal,
                slope_config,
            );
        }
    }

    slopes
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    const COUNTS: [u16; 16] = [1, 1, 16, 1, 16, 1, 16, 1, 16, 42, 2, 14, 14, 21, 1, 10];
    const FULL_GENERAL: &str = r#"
PaveTile=7
ClearToPaveLat=8
GreenTile=5
ClearToGreenLat=6
SandTile=3
ClearToSandLat=4
RoughTile=1
ClearToRoughLat=2
ShorePieces=9
WaterBridge=10
MiscPaveTile=11
Medians=12
PavedRoads=13
RoughConnectTo=14
"#;

    fn fixture(general: &str, counts: &[u16]) -> (TilesetLookup, LatConfig) {
        let mut text = format!("[General]\n{general}");
        for (index, count) in counts.iter().copied().enumerate() {
            let name = if index == 15 { "Water" } else { "Synthetic" };
            writeln!(
                text,
                "\n[TileSet{index:04}]\nSetName={name}\nFileName=t{index}\nTilesInSet={count}"
            )
            .unwrap();
        }
        let lookup = crate::map::theater::parse_tileset_ini(text.as_bytes(), "tem").unwrap();
        let config = parse_lat_config(text.as_bytes(), &lookup);
        (lookup, config)
    }

    fn dummy_lookup() -> TilesetLookup {
        fixture("", &[1]).0
    }

    fn ground(name: &str, base: i32, lat: i32, ranges: &[(i32, i32)]) -> LatGroundType {
        LatGroundType {
            name: name.to_string(),
            base_tile: base,
            lat_base: lat,
            exemptions: ranges
                .iter()
                .map(|&(first, last_offset)| TileRange::new(first, last_offset))
                .collect(),
            enable_guard: LatEnableGuard::Always,
        }
    }

    fn cell(rx: u16, ry: u16, tile_index: i32) -> MapCell {
        MapCell {
            rx,
            ry,
            tile_index,
            sub_tile: 0,
            z: 0,
        }
    }

    fn center_cells(center: i32, neighbors: [i32; 4]) -> Vec<MapCell> {
        vec![
            cell(10, 10, center),
            cell(10, 9, neighbors[0]),
            cell(11, 10, neighbors[1]),
            cell(10, 11, neighbors[2]),
            cell(9, 10, neighbors[3]),
        ]
    }

    fn center_result(center: i32, neighbors: [i32; 4], ground: LatGroundType) -> i32 {
        let mut cells = center_cells(center, neighbors);
        apply_lat(
            &mut cells,
            &LatConfig {
                grounds: vec![ground],
            },
            &dummy_lookup(),
        );
        cells[0].tile_index
    }

    #[test]
    fn gsi_04_03a_config_uses_absolute_ids_fixed_order_and_exact_ranges() {
        let (_, config) = fixture(FULL_GENERAL, &COUNTS);
        assert_eq!(
            config
                .grounds
                .iter()
                .map(|ground| (ground.name.as_str(), ground.base_tile, ground.lat_base))
                .collect::<Vec<_>>(),
            vec![
                ("Rough", 1, 2),
                ("Sand", 18, 19),
                ("Green", 35, 36),
                ("Pave", 52, 53)
            ]
        );
        assert!(config.grounds[0].exemptions.is_empty());
        assert!(config.grounds[1].exemptions.is_empty());
        assert_eq!(
            config.grounds[2].exemptions,
            vec![TileRange::new(69, 0x29), TileRange::new(111, 1)]
        );
        assert_eq!(
            config.grounds[3].exemptions,
            vec![
                TileRange::new(113, 0xD),
                TileRange::new(127, 0xD),
                TileRange::new(141, 0x14)
            ]
        );

        let only_shore = "GreenTile=5\nClearToGreenLat=6\nShorePieces=9\n";
        let (_, missing_key) = fixture(only_shore, &COUNTS);
        assert_eq!(
            missing_key.grounds[2].exemptions,
            vec![TileRange::new(69, 0x29)],
            "missing WaterBridge disables only that exemption"
        );
    }

    #[test]
    fn gsi_04_03a_zero_length_lunar_green_exemptions_are_disabled() {
        let mut counts = COUNTS;
        counts[9] = 0;
        counts[10] = 0;
        let general = "GreenTile=5\nClearToGreenLat=6\nShorePieces=9\nWaterBridge=10\n";
        let (lookup, config) = fixture(general, &counts);
        assert!(config.grounds[2].exemptions.is_empty());

        // Both zero-length sections resolve to the next real tile's start.
        // It must differ from Green rather than becoming a bogus exemption.
        let shared_start = i32::from(lookup.bounds()[9].start);
        let mut cells = center_cells(35, [shared_start, 35, 35, 35]);
        apply_lat(&mut cells, &config, &lookup);
        assert_eq!(cells[0].tile_index, 37);
    }

    #[test]
    fn gsi_04_03a_rough_runs_with_missing_lat_global_and_uses_minus_one_arithmetic() {
        let (lookup, config) = fixture("RoughTile=1\n", &COUNTS);
        assert_eq!(
            config
                .grounds
                .iter()
                .map(|ground| (
                    ground.name.as_str(),
                    ground.base_tile,
                    ground.lat_base,
                    ground.is_enabled(),
                ))
                .collect::<Vec<_>>(),
            vec![
                ("Rough", 1, -1, true),
                ("Sand", -1, -1, false),
                ("Green", -1, -1, false),
                ("Pave", -1, -1, false),
            ]
        );
        let mut cells = center_cells(1, [0, 1, 1, 1]);
        apply_lat(&mut cells, &config, &lookup);
        assert_eq!(
            cells[0].tile_index, 0,
            "missing LAT base writes -1 + mask 1"
        );
    }

    #[test]
    fn gsi_04_03a_nonrough_groups_gate_only_on_lat_global() {
        let (_, missing_ground) = fixture("ClearToSandLat=2\n", &COUNTS);
        let sand = missing_ground.grounds[1].clone();
        assert_eq!(
            (sand.base_tile, sand.lat_base, sand.is_enabled()),
            (-1, 2, true)
        );
        assert_eq!(
            center_result(2, [0, 2, 2, 2], sand),
            3,
            "defined LAT range remains active when the ground base is missing"
        );

        for name in ["Sand", "Green", "Pave"] {
            let guarded = LatGroundType {
                name: name.to_string(),
                base_tile: 100,
                lat_base: -1,
                exemptions: Vec::new(),
                enable_guard: LatEnableGuard::LatBaseDefined,
            };
            assert!(!guarded.is_enabled());
            assert_eq!(center_result(100, [0, 100, 100, 100], guarded), 100);
        }
    }

    #[test]
    fn native_load_lat_contract() {
        let rough = || ground("Rough", 700, 710, &[]);
        for (different, expected) in [(0, 711), (1, 712), (2, 714), (3, 718)] {
            let mut neighbors = [700; 4];
            neighbors[different] = 0;
            assert_eq!(center_result(700, neighbors, rough()), expected);
        }
        assert_eq!(center_result(715, [700; 4], rough()), 700);
        assert_eq!(
            center_result(700, [-1, NO_TILE, 700, 700], rough()),
            713,
            "no-tile N+E set direct bits 0+1"
        );

        let green = ground("Green", 900, 910, &[(400, 0x29), (450, 1)]);
        assert_eq!(center_result(900, [441, 451, 900, 0], green.clone()), 918);
        assert_eq!(center_result(900, [442, 452, 900, 900], green), 913);
        let pave = ground(
            "Pave",
            1000,
            1010,
            &[(1100, 0xD), (1300, 0xD), (1200, 0x14)],
        );
        assert_eq!(
            center_result(1000, [1113, 1313, 1220, 0], pave.clone()),
            1018
        );
        assert_eq!(center_result(1000, [1114, 1314, 1221, 1000], pave), 1017);

        let (lookup, config) = fixture(FULL_GENERAL, &COUNTS);
        assert!(lookup.is_water(163));
        let mut water = center_cells(35, [163, 35, 35, 35]);
        apply_lat(&mut water, &config, &lookup);
        assert_eq!(water[0].tile_index, 37, "generic water is not exempt");
        let mut connect_to = center_cells(1, [162, 1, 1, 1]);
        apply_lat(&mut connect_to, &config, &lookup);
        assert_eq!(connect_to[0].tile_index, 3, "RoughConnectTo is ignored");

        let mut edge = vec![cell(0, 0, 700), cell(1, 0, 700), cell(0, 1, 700)];
        apply_lat(
            &mut edge,
            &LatConfig {
                grounds: vec![rough()],
            },
            &lookup,
        );
        assert_eq!(edge[0].tile_index, 719, "off-map N+W set bits 0+3");

        let mut preserved = center_cells(700, [0, 700, 700, 700]);
        preserved[0].sub_tile = 7;
        let rough_config = LatConfig {
            grounds: vec![rough()],
        };
        apply_lat(&mut preserved, &rough_config, &lookup);
        assert_eq!((preserved[0].tile_index, preserved[0].sub_tile), (711, 7));
        let after_first = preserved
            .iter()
            .map(|cell| cell.tile_index)
            .collect::<Vec<_>>();
        apply_lat(&mut preserved, &rough_config, &lookup);
        assert_eq!(
            preserved
                .iter()
                .map(|cell| cell.tile_index)
                .collect::<Vec<_>>(),
            after_first,
            "second application is idempotent"
        );
    }

    fn cardinal_slopes_for_mask(slope: u8, mask: u8) -> [u8; 4] {
        let (first, second) = match slope {
            1 => (3, 1),
            2 => (0, 2),
            3 => (1, 3),
            4 => (2, 0),
            _ => panic!("mask helper requires a directional slope"),
        };
        let mut neighbors = [9; 4];
        if mask & 1 != 0 {
            neighbors[first] = 0;
        }
        if mask & 2 != 0 {
            neighbors[second] = 0;
        }
        neighbors
    }

    #[test]
    fn gsi_04_03a_slopes_one_through_four_cover_all_flat_neighbor_masks() {
        let config = SlopeFixupConfig {
            ramp_base: 100,
            ramp_smooth: 500,
        };
        for slope in 1..=4 {
            for mask in 0..=3 {
                let expected = if mask == 0 {
                    100 + i32::from(slope - 1)
                } else {
                    let block = match slope {
                        1 => mask - 1,
                        2 => mask + 2,
                        3 => mask + 5,
                        4 => mask + 8,
                        _ => unreachable!(),
                    };
                    500 + i32::from(block)
                };
                assert_eq!(
                    slope_fixed_tile(100, slope, cardinal_slopes_for_mask(slope, mask), config,),
                    expected,
                    "slope {slope}, mask {mask}",
                );
            }
        }
    }

    #[test]
    fn gsi_04_03a_zero_and_unsigned_high_slopes_use_dword_fallback() {
        let config = SlopeFixupConfig {
            ramp_base: 100,
            ramp_smooth: 500,
        };
        for slope in [0, 5, 16, 127, 128, 255] {
            assert_eq!(
                slope_fixed_tile(100, slope, [0; 4], config),
                100 + i32::from(slope) - 1,
                "slope {slope}",
            );
        }
    }

    #[test]
    fn gsi_04_03a_ramp_guards_are_inclusive_and_reject_adjacent_tiles() {
        let config = SlopeFixupConfig {
            ramp_base: 100,
            ramp_smooth: 500,
        };
        for (tile, expected) in [
            (99, 99),
            (100, 99),
            (119, 99),
            (120, 120),
            (499, 499),
            (500, 99),
            (511, 99),
            (512, 512),
        ] {
            assert_eq!(slope_fixed_tile(tile, 0, [9; 4], config), expected);
        }
    }

    #[test]
    fn gsi_04_03a_missing_ramp_bases_retain_native_minus_one_arithmetic() {
        assert_eq!(
            slope_fixed_tile(
                500,
                0,
                [9; 4],
                SlopeFixupConfig {
                    ramp_base: -1,
                    ramp_smooth: 500,
                },
            ),
            -2,
            "missing RampBase still owns fallback arithmetic",
        );
        assert_eq!(
            slope_fixed_tile(
                100,
                1,
                [0; 4],
                SlopeFixupConfig {
                    ramp_base: 100,
                    ramp_smooth: -1,
                },
            ),
            1,
            "missing RampSmooth still owns smoothing arithmetic",
        );
        assert_eq!(
            slope_fixed_tile(
                -1,
                1,
                [0; 4],
                SlopeFixupConfig {
                    ramp_base: -1,
                    ramp_smooth: -1,
                },
            ),
            1,
            "the missing-key sentinel is not protected by an extra guard",
        );
    }

    #[test]
    fn gsi_04_03a_off_map_neighbors_are_flat_for_slope_smoothing() {
        let config = SlopeFixupConfig {
            ramp_base: 100,
            ramp_smooth: 500,
        };
        assert_eq!(slope_fixed_tile(100, 1, [0; 4], config), 502);
    }

    #[test]
    fn gsi_04_03a_second_sweep_converges_after_future_neighbor_state_arrives() {
        let cells = vec![cell(9, 10, 100), cell(10, 10, 100), cell(11, 10, 100)];
        let lat_config = LatConfig {
            grounds: Vec::new(),
        };
        let slope_config = SlopeFixupConfig {
            ramp_base: 100,
            ramp_smooth: 500,
        };
        let mut slope_from_pristine = |tile: i32, _sub_tile: u8| {
            u8::from((100..=119).contains(&tile) || (500..=511).contains(&tile))
        };

        let mut one_sweep = cells.clone();
        apply_recalc_sweeps(
            &mut one_sweep,
            &lat_config,
            slope_config,
            1,
            &mut slope_from_pristine,
        );
        assert_eq!(one_sweep[1].tile_index, 501);

        let mut two_sweeps = cells;
        let slopes = apply_load_recalc_sweeps(
            &mut two_sweeps,
            &lat_config,
            slope_config,
            &mut slope_from_pristine,
        );
        assert_eq!(two_sweeps[1].tile_index, 100);
        assert_eq!(slopes, vec![1, 1, 1]);
    }

    #[test]
    fn live_lat_callback_runs_once_per_matching_group_and_never_for_a_nonmatch() {
        let config = LatConfig {
            grounds: vec![ground("First", 0, 10, &[]), ground("Second", 0, 30, &[])],
        };
        let mut calls = 0;
        assert_eq!(
            lat_fixed_tile_live(0, &config, || {
                calls += 1;
                [0; 4]
            }),
            0
        );
        assert_eq!(calls, 2);

        assert_eq!(lat_fixed_tile_live(99, &config, || panic!("guarded lookup")), 99);
    }

    #[test]
    fn live_slope_callback_uses_only_the_native_ordered_pair() {
        let config = SlopeFixupConfig {
            ramp_base: 100,
            ramp_smooth: 500,
        };
        for (slope, expected) in [
            (1, vec![3, 1]),
            (2, vec![0, 2]),
            (3, vec![1, 3]),
            (4, vec![2, 0]),
        ] {
            let mut order = Vec::new();
            slope_fixed_tile_live(100, slope, config, |slot| {
                order.push(slot);
                9
            });
            assert_eq!(order, expected, "slope {slope}");
        }

        for (tile, slope) in [(99, 1), (100, 0), (100, 5)] {
            slope_fixed_tile_live(tile, slope, config, |_| panic!("guarded slope lookup"));
        }
    }
}
