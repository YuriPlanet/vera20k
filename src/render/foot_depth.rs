//! Native Foot depth selection, independent of simulation and draw-list ownership.
//!
//! `FootClass::GetZAdjustment` (0x004DAFC0..0x004DB09E) combines the
//! locomotor adjustment, 0x00704350's terrain adjustment, and the signed maximum
//! of four type-specific terms. The lookup adapter supplies authoritative cell
//! state and already resolved TMP dimensions; this module never mutates the map.
//! Evidence: fresh retail gamemd.exe assembly, 2026-09-08. The executable oracle
//! lives in tools/render_depth_oracle.py; source tests below are regressions
//! unless explicitly identified as oracle comparisons.

use crate::util::native_x87::adjust_for_z_standard;

pub(crate) type CellCoord = [i16; 2];

/// Values read by the native depth helpers. `coord` is the resolved CellClass's
/// stored coordinate, which can differ from the request under fixed-slot aliasing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DepthCell {
    pub coord: CellCoord,
    pub level: i8,
    pub ramp: u8,
    pub flags: u32,
    pub iso_tile_index: i32,
    pub low_bridge: bool,
    /// None is native OverlayTypeIndex=-1; Some(false) is a present non-rock.
    pub overlay_rock: Option<bool>,
    /// 0x00547150's height, including clear-tile fallback, not atlas canvas height.
    pub tmp_height: i32,
    /// The shared dummy returned by native 0x005657A0 on a missing fixed slot.
    pub is_dummy: bool,
}

impl Default for DepthCell {
    fn default() -> Self {
        Self {
            coord: [0, 0],
            level: 0,
            ramp: 0,
            flags: 0,
            iso_tile_index: 0xffff,
            low_bridge: false,
            overlay_rock: None,
            tmp_height: 30,
            is_dummy: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FootDepthContext {
    /// Native ObjectClass +0x1B8 coordinate, truncated from raw location leptons.
    pub cell: CellCoord,
    pub on_bridge: bool,
    pub facing_u16: u16,
    pub world_z_leptons: i32,
    pub locomotor_z: i32,
    pub bridge_set_base: i32,
    /// 0x00704350's Unit-only early return: -3 for an entered radio contact
    /// whose building mission is Unload, or -14 for a harvester on a refinery.
    /// The caller establishes these entity/building predicates, in that order.
    pub unit_special_adjustment: Option<i32>,
}

impl Default for FootDepthContext {
    fn default() -> Self {
        Self {
            cell: [0, 0],
            on_bridge: false,
            facing_u16: 0,
            world_z_leptons: 0,
            locomotor_z: 0,
            bridge_set_base: -1,
            unit_special_adjustment: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FootDepthCoefficients {
    pub cliff: i32,
    pub column: i32,
    pub tunnel: i32,
    pub bridge: i32,
}

impl Default for FootDepthCoefficients {
    fn default() -> Self {
        // TechnoType constructor 0x00711664..0x0071167A; Unit/Infantry inherit.
        Self {
            cliff: 10,
            column: 5,
            tunnel: 10,
            bridge: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FootDepthTerms {
    pub column_score: i32,
    pub tunnel_score: i32,
    pub cliff_score: i32,
    pub near_bridge: bool,
    pub additional_z: i32,
    pub maximum_fudge: i32,
    pub z_adjust: i32,
}

/// 0x0089F688's startup direction table, initialized at 0x0049F2F0.
const DIRECTIONS: [CellCoord; 8] = [
    [0, -1],
    [1, -1],
    [1, 0],
    [1, 1],
    [0, 1],
    [-1, 1],
    [-1, 0],
    [-1, -1],
];

fn offset(coord: CellCoord, delta: CellCoord) -> CellCoord {
    [
        coord[0].wrapping_add(delta[0]),
        coord[1].wrapping_add(delta[1]),
    ]
}

/// A render-local view of dummy identity. Native failed lookups stamp the same
/// object's coordinate, so a retained dummy pointer sees the latest request.
/// Only that stamp is reproduced locally; simulation state remains untouched.
struct Cells<F> {
    lookup: F,
    dummy_coord: CellCoord,
}

impl<F: FnMut(CellCoord) -> DepthCell> Cells<F> {
    fn get(&mut self, coord: CellCoord) -> DepthCell {
        let mut cell = (self.lookup)(coord);
        if cell.is_dummy {
            self.dummy_coord = coord;
            cell.coord = coord;
        }
        cell
    }

    fn coord(&self, cell: DepthCell) -> CellCoord {
        if cell.is_dummy {
            self.dummy_coord
        } else {
            cell.coord
        }
    }

    /// 0x00703B10 (mask 0x100), 0x00703CC0 (mask 0x400). All four
    /// neighbors are resolved before testing, in the native S,N,E,W order,
    /// so each failed lookup stamps the shared dummy.
    fn near_bridge(&mut self, context: FootDepthContext, mask: u32) -> bool {
        let current = self.get(context.cell);
        if context.on_bridge {
            return false;
        }
        let [south, north, east, west] = [4, 0, 2, 6]
            .map(|direction| self.get(offset(context.cell, DIRECTIONS[direction])).flags);
        crate::map::bridge_facts::near_bridge(
            current.flags,
            [Some(south), Some(north), Some(east), Some(west)],
            mask,
        )
    }

    /// 0x00703E70: S/E each assign one; SE increments that result.
    fn column_score(&mut self, context: FootDepthContext) -> i32 {
        self.get(context.cell);
        if !self.near_bridge(context, 0x100) && !self.near_bridge(context, 0x400) {
            return 0;
        }
        let south = self.get(offset(context.cell, DIRECTIONS[4]));
        let east = self.get(offset(context.cell, DIRECTIONS[2]));
        let southeast = self.get(offset(context.cell, DIRECTIONS[3]));
        let column = |cell: DepthCell| {
            !matches!(cell.iso_tile_index, 0xff | 0xffff)
                && (7..=16).contains(
                    &cell
                        .iso_tile_index
                        .wrapping_sub(context.bridge_set_base)
                        .wrapping_add(1),
                )
        };
        i32::from(column(south) || column(east)) + i32::from(column(southeast))
    }

    /// 0x00704000: choose a tube in current,N,W priority, then test two steps
    /// north/west. Each second step starts at the resolved first-step coordinate.
    fn tunnel_score(&mut self, context: FootDepthContext) -> i32 {
        if context.on_bridge {
            return 0;
        }
        let north = self.get(offset(context.cell, DIRECTIONS[0]));
        let west = self.get(offset(context.cell, DIRECTIONS[6]));
        let current = self.get(context.cell);
        let Some(selected) = [current, north, west]
            .into_iter()
            .find(|cell| cell.low_bridge)
        else {
            return 0;
        };
        let selected = self.coord(selected);
        // 0x006F2A40 initializes this family's B0EA50 null coordinate to (0,0).
        if selected == [0, 0] {
            return 0;
        }
        let north_one = self.get(offset(selected, DIRECTIONS[0]));
        let west_one = self.get(offset(selected, DIRECTIONS[6]));
        let north_two = self.get(offset(self.coord(north_one), DIRECTIONS[0]));
        let west_two = self.get(offset(self.coord(west_one), DIRECTIONS[6]));
        i32::from(west_two.low_bridge || north_two.low_bridge)
    }

    /// 0x00704240: the second qualifying sample overrides the first's score.
    fn cliff_score(&mut self, context: FootDepthContext) -> i32 {
        let current = self.get(context.cell);
        if context.on_bridge {
            return 0;
        }
        let first = self.get(offset(context.cell, DIRECTIONS[3]));
        let first_score = if i32::from(first.level) - i32::from(current.level) >= 4 {
            2
        } else {
            0
        };
        let second = self.get(offset(self.coord(first), DIRECTIONS[3]));
        if i32::from(second.level) - i32::from(current.level) >= 4 {
            1
        } else {
            first_score
        }
    }

    /// 0x00704350. Constants 84311C/843120=-1 and 843124=3 are retail bytes.
    fn additional_z(&mut self, context: FootDepthContext) -> i32 {
        let base = adjust_for_z_standard(context.world_z_leptons).wrapping_neg();
        let current = self.get(context.cell);
        if let Some(special) = context.unit_special_adjustment {
            return base.wrapping_add(special);
        }
        if current.ramp != 0 {
            let south = self.get(offset(context.cell, DIRECTIONS[4]));
            let east = self.get(offset(context.cell, DIRECTIONS[2]));
            let southeast = self.get(offset(context.cell, DIRECTIONS[3]));
            // The first present non-rock prevents later rocks from qualifying.
            if [south, east, southeast]
                .into_iter()
                .find_map(|cell| cell.overlay_rock)
                == Some(true)
            {
                return base.wrapping_add(2);
            }
        }
        let facing = (((u32::from(context.facing_u16) >> 12) + 1) >> 1) as usize & 7;
        let probes = [
            facing + 1,
            facing + 7,
            facing,
            facing + 4,
            facing + 3,
            facing + 5,
        ]
        .map(|direction| self.get(offset(context.cell, DIRECTIONS[direction & 7])));
        if probes.iter().any(|cell| cell.ramp != 0) || current.ramp != 0 {
            return base.wrapping_sub(1);
        }
        let chosen = match facing {
            0 | 6 => &probes[3..6],
            2 | 4 => &probes[0..3],
            _ => return base.wrapping_sub(1),
        };
        if current.flags & 0x10000 != 0 || chosen.iter().any(|cell| cell.flags & 0x10000 != 0) {
            return base.wrapping_sub(1);
        }
        if chosen.iter().any(|cell| cell.tmp_height > 36) {
            base.wrapping_sub(2)
        } else {
            base.wrapping_sub(1)
        }
    }
}

/// Evaluate selectors in the same order as 0x004DAFC0. The signed maximum
/// includes a zero bridge candidate only when the bridge predicate is false.
pub(crate) fn foot_depth_terms(
    context: FootDepthContext,
    coefficients: FootDepthCoefficients,
    lookup: impl FnMut(CellCoord) -> DepthCell,
) -> FootDepthTerms {
    let mut cells = Cells {
        lookup,
        dummy_coord: [0, 0],
    };
    let column_score = cells.column_score(context);
    let tunnel_score = cells.tunnel_score(context);
    let cliff_score = cells.cliff_score(context);
    let near_bridge = cells.near_bridge(context, 0x100);
    let maximum_fudge = coefficients
        .column
        .wrapping_mul(column_score)
        .max(coefficients.tunnel.wrapping_mul(tunnel_score))
        .max(coefficients.cliff.wrapping_mul(cliff_score))
        .max(if near_bridge { coefficients.bridge } else { 0 });
    let additional_z = cells.additional_z(context);
    let z_adjust = additional_z
        .wrapping_add(maximum_fudge)
        .wrapping_add(context.locomotor_z);
    FootDepthTerms {
        column_score,
        tunnel_score,
        cliff_score,
        near_bridge,
        additional_z,
        maximum_fudge,
        z_adjust,
    }
}

pub(crate) fn foot_z_adjust(
    context: FootDepthContext,
    coefficients: FootDepthCoefficients,
    lookup: impl FnMut(CellCoord) -> DepthCell,
) -> i32 {
    foot_depth_terms(context, coefficients, lookup).z_adjust
}

/// Unit's final composite draw at 0x73B140, before its height > 16 test.
/// The bridge arm consumes raw 0x703B10 / 0x703E70 results, independently of
/// ZFudge coefficients. The other arm is NavCom plus radio slot zero's
/// WeaponsFactory building. See docs/research/bridges/06-render-presentation-audio/
/// UNIT_COMPOSITE_BRIDGE_SPLIT_73B140_GHIDRA_REPORT.md.
pub(crate) fn unit_composite_split(
    context: FootDepthContext,
    too_big: bool,
    navigating_to_weapons_factory: bool,
    lookup: impl FnMut(CellCoord) -> DepthCell,
) -> bool {
    if !too_big {
        return false;
    }
    let mut cells = Cells {
        lookup,
        dummy_coord: [0, 0],
    };
    (cells.near_bridge(context, 0x100) && cells.column_score(context) == 0)
        || navigating_to_weapons_factory
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate(mut at: impl FnMut(CellCoord) -> DepthCell) -> FootDepthTerms {
        foot_depth_terms(
            FootDepthContext {
                cell: [10, 10],
                ..Default::default()
            },
            FootDepthCoefficients::default(),
            |coord| {
                let mut cell = at(coord);
                cell.coord = coord;
                cell
            },
        )
    }

    #[test]
    fn second_cliff_probe_is_independent_and_overrides_first() {
        for (first, second, score) in [(0, 0, 0), (4, 0, 2), (0, 4, 1), (4, 4, 1)] {
            let terms = evaluate(|coord| DepthCell {
                level: match coord {
                    [11, 11] => first,
                    [12, 12] => second,
                    _ => 0,
                },
                ..Default::default()
            });
            assert_eq!(terms.cliff_score, score);
            assert_eq!(terms.z_adjust, score * 10 - 1);
        }
    }

    #[test]
    fn cliff_uses_signed_level_and_resolved_first_coordinate() {
        let terms = foot_depth_terms(
            FootDepthContext {
                cell: [10, 10],
                ..Default::default()
            },
            FootDepthCoefficients::default(),
            |coord| match coord {
                [10, 10] => DepthCell {
                    coord,
                    level: -3,
                    ..Default::default()
                },
                [11, 11] => DepthCell {
                    coord: [20, 20],
                    level: 1,
                    ..Default::default()
                },
                [21, 21] => DepthCell {
                    coord,
                    level: 1,
                    ..Default::default()
                },
                _ => DepthCell {
                    coord,
                    level: -3,
                    ..Default::default()
                },
            },
        );
        assert_eq!(terms.cliff_score, 1);
    }

    #[test]
    fn column_neighbors_are_not_three_independent_addends() {
        let terms = evaluate(|coord| DepthCell {
            flags: if coord == [10, 10] { 0x100 } else { 0 },
            iso_tile_index: if matches!(coord, [10, 11] | [11, 10] | [11, 11]) {
                6
            } else {
                0xffff
            },
            ..Default::default()
        });
        assert_eq!(terms.column_score, 2); // base=-1, q=8
    }

    #[test]
    fn tunnel_priority_and_retained_dummy_coordinate_are_preserved() {
        let context = FootDepthContext {
            cell: [10, 10],
            ..Default::default()
        };
        let mut cells = Cells {
            lookup: |coord| DepthCell {
                coord,
                low_bridge: matches!(coord, [10, 10] | [10, 9] | [10, 7]),
                ..Default::default()
            },
            dummy_coord: [0, 0],
        };
        // Current is chosen before N. N's second step qualifies; current's does not.
        assert_eq!(cells.tunnel_score(context), 0);

        let mut probes = Vec::new();
        let mut cells = Cells {
            lookup: |coord| {
                probes.push(coord);
                DepthCell {
                    coord,
                    low_bridge: matches!(coord, [10, 10] | [9, 9]),
                    is_dummy: matches!(coord, [10, 9] | [9, 10]),
                    ..Default::default()
                }
            },
            dummy_coord: [0, 0],
        };
        // Both first steps retain the same dummy. The west lookup stamps its
        // coordinate before the north pointer is used to construct step two.
        assert_eq!(cells.tunnel_score(context), 1);
        assert_eq!(&probes[probes.len() - 2..], &[[9, 9], [8, 10]]);
    }

    #[test]
    fn first_present_overlay_wins_and_tall_tile_only_changes_cardinal_base() {
        let context = FootDepthContext {
            cell: [10, 10],
            ..Default::default()
        };
        let cells = |coord| DepthCell {
            coord,
            ramp: u8::from(coord == [10, 10]),
            overlay_rock: match coord {
                [10, 11] => Some(false),
                [11, 10] => Some(true),
                _ => None,
            },
            tmp_height: 100,
            ..Default::default()
        };
        assert_eq!(
            foot_depth_terms(context, Default::default(), cells).additional_z,
            -1
        );
        for (facing_u16, expected) in [(0, -2), (8192, -1)] {
            assert_eq!(
                foot_depth_terms(
                    FootDepthContext {
                        facing_u16,
                        ..context
                    },
                    Default::default(),
                    |coord| DepthCell {
                        coord,
                        tmp_height: 37,
                        ..Default::default()
                    }
                )
                .additional_z,
                expected
            );
        }
    }

    #[test]
    fn on_bridge_suppresses_fudges_but_keeps_exact_height_and_special_base() {
        let context = FootDepthContext {
            cell: [10, 10],
            on_bridge: true,
            world_z_leptons: 728,
            unit_special_adjustment: Some(-14),
            ..Default::default()
        };
        let result = foot_z_adjust(context, Default::default(), |coord| DepthCell {
            coord,
            flags: 0x500,
            low_bridge: true,
            level: if coord == [10, 10] { 0 } else { 8 },
            ..Default::default()
        });
        assert_eq!(result, -119);
    }

    #[test]
    fn max_is_signed_after_wrapping_multiply_and_then_wraps_addition() {
        let context = FootDepthContext {
            cell: [10, 10],
            locomotor_z: i32::MAX,
            ..Default::default()
        };
        let result = foot_depth_terms(
            context,
            FootDepthCoefficients {
                cliff: i32::MAX,
                column: 0,
                tunnel: 0,
                bridge: 0,
            },
            |coord| DepthCell {
                coord,
                level: if coord == [11, 11] { 4 } else { 0 },
                ..Default::default()
            },
        );
        assert_eq!(result.cliff_score, 2);
        assert_eq!(result.maximum_fudge, 0); // MAX*2 wraps to -2 before signed max.
        assert_eq!(result.z_adjust, i32::MAX - 1);
        let result = foot_z_adjust(context, Default::default(), |coord| DepthCell {
            coord,
            level: if coord == [11, 11] { 4 } else { 0 },
            ..Default::default()
        });
        assert_eq!(result, i32::MIN + 18);
    }

    #[test]
    fn matches_unmodified_gamemd_foot_and_selector_execution() {
        // Actual x86 function execution over the recorded fixture domain. This
        // does not certify map loading, dummy/alias cases, or final GPU pixels.
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/render_depth_vectors.json")).unwrap();
        fn integer(value: &serde_json::Value, field: &str) -> i32 {
            i32::try_from(value[field].as_i64().unwrap()).unwrap()
        }
        fn coordinate(value: &serde_json::Value) -> CellCoord {
            [
                i16::try_from(value[0].as_i64().unwrap()).unwrap(),
                i16::try_from(value[1].as_i64().unwrap()).unwrap(),
            ]
        }
        fn cell(value: &serde_json::Value, coord: CellCoord) -> DepthCell {
            DepthCell {
                coord,
                level: i8::try_from(integer(value, "level")).unwrap(),
                ramp: u8::try_from(integer(value, "ramp")).unwrap(),
                flags: u32::try_from(integer(value, "flags")).unwrap(),
                iso_tile_index: integer(value, "tile_index"),
                tmp_height: integer(value, "tmp_height"),
                ..Default::default()
            }
        }
        assert_eq!(document["schema_version"], 1);
        assert_eq!(
            document["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        let defaults = &document["defaults"];
        let cases = document["cases"].as_array().unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let context = &case["context"];
            let coefficients = &case["coefficients"];
            let context = FootDepthContext {
                cell: coordinate(&defaults["current_coords"]),
                on_bridge: context["on_bridge"].as_bool().unwrap(),
                facing_u16: u16::try_from(integer(context, "facing_u16")).unwrap(),
                world_z_leptons: integer(context, "world_z_leptons"),
                locomotor_z: integer(defaults, "locomotor_z_adjust"),
                bridge_set_base: integer(defaults, "bridge_tile_base"),
                unit_special_adjustment: None,
            };
            let coefficients = FootDepthCoefficients {
                cliff: integer(coefficients, "cliff"),
                column: integer(coefficients, "column"),
                tunnel: integer(coefficients, "tunnel"),
                bridge: integer(coefficients, "bridge"),
            };
            let overrides = case["cells"].as_array().unwrap();
            let actual = foot_depth_terms(context, coefficients, |coord| {
                assert!(
                    coord[0] >= 0 && i32::from(coord[0]) < integer(defaults, "mapped_grid_width"),
                    "{name}"
                );
                assert!(
                    coord[1] >= 0 && i32::from(coord[1]) < integer(defaults, "mapped_grid_height"),
                    "{name}"
                );
                let value = overrides
                    .iter()
                    .find(|value| coordinate(&value["coords"]) == coord)
                    .unwrap_or(&defaults["cell"]);
                cell(value, coord)
            });
            let expected = &case["native"];
            assert_eq!(
                actual.cliff_score,
                integer(expected, "cliff_multiplier"),
                "{name}: cliff"
            );
            assert_eq!(
                actual.column_score,
                integer(expected, "column_multiplier"),
                "{name}: column"
            );
            assert_eq!(
                actual.tunnel_score,
                integer(expected, "tunnel_multiplier"),
                "{name}: tunnel"
            );
            assert_eq!(
                actual.near_bridge,
                expected["near_bridge"].as_bool().unwrap(),
                "{name}: bridge"
            );
            assert_eq!(
                actual.additional_z,
                integer(expected, "base_z_adjust"),
                "{name}: base"
            );
            assert_eq!(
                actual.z_adjust,
                integer(expected, "foot_z_adjust"),
                "{name}: Foot total"
            );
        }
    }
}
