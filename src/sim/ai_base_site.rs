//! Where a computer house puts a building: `HouseClass::FindBaseBuildingSite
//! @ 0x005060B0`. The building exit (`sim::ai_base_building::exit_building`,
//! `0x00444FE1` and `0x004450BD`) calls it with the ordinary key `0x00505F80`,
//! the base defense choice (`sim::ai_base_defense`, `0x00507A20`) with the
//! defense key `0x00505FD0` ([`SiteKey`]). A `Naval=` type takes the naval
//! branch (`sim::naval_base_placement`); any other type walks the house's base
//! perimeter (`HouseClass+0x5724`) in key order.
//!
//! The ordinary branch (`0x0050623D..0x00506B0E`):
//! - Without a plan centre the answer is the house's alternate base cell
//!   (`+0x5494`), else its primary one (`+0x5490`).
//! - Each perimeter cell is keyed (`0x0050631F`) and the cells are sorted by
//!   the retail qsort (`0x007C8B48`, comparator `0x005108F0`: signed keys).
//!   The ordinary key is `index + 1000 * max(|dx|, |dy|)` from the plan
//!   centre, so the nearest cells come first.
//! - For each cell in that order next to any of the house's base reservations
//!   (`CellClass+0xDC`), the steps towards those neighbours sum to a vector
//!   into the base. The direction away from it (`DirStruct`, eight ways)
//!   offsets the foundation clear of the reserved cells by a border of
//!   `AIBaseSpacing=`, one more for a `ProtectWithWall=` or `WantsExtraSpace=`
//!   type, and three sites are tried along the perimeter; then three more one
//!   cell back towards the base with the border less `AIBaseSpacing=`.
//! - A site is taken when the bordered foundation is clear
//!   (`CellRect::CheckOccupancy @ 0x00586780`), the type may stand there
//!   (`sim::build_site`), its level is within two of the plan centre's and a
//!   reservation of the house lies near it ([`reserved_near`]).
//! - Nothing found answers (0, 0). The body then repeats the whole search with
//!   the same inputs and nothing written in between, which finds nothing new;
//!   it runs once here.
//!
//! Evidence: instruction reading, and native execution
//! (`tools/ai_base_building_oracle.py`, compared in `ai_base_site_tests.rs`)
//! of the key, the sort, the direction for every neighbour sum, the
//! reserved-near bounds and 68 whole searches on synthetic maps, whose every
//! cell lookup, occupancy test and placement test [`ordinary_site`] repeats
//! in order through [`SiteWorld`]; each failed search's second pass repeated
//! its first. `tools/ai_base_defense_oracle.py` adds 34 whole searches with
//! the defense key. The map answers themselves are the owners'
//! (`sim::cell_rect`, `sim::build_site`).
//!
//! RESIDUAL: a `CloakGenerator=` type keys cells by their 3D distance from the
//! house's base coordinate instead (`0x00506329..0x0050640D`). No retail
//! BuildingType sets the key (`ai_base_building_tests.rs` checks the retail
//! rules), so every type takes the ordinary key.

use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_base_defense::CoverageGrid;
use crate::sim::base_plan::unpack_base_plan_cell;
use crate::sim::cell_rect::{
    CellRect, CellRectOccupancyContext, check_occupancy_rect, get_cellclass_fallback,
};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::direction_tables::{dir_from_facing16, facing16_from_delta};

/// `0x00A8EF78`, built on first use (`0x0050670E..0x00506807`): per direction
/// away from the base, the step from the offset corner onto the first site.
const FIRST_STEP: [(i16, i16); 8] = [
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
];

/// `0x00A8EFA8` (`0x00506811..0x005068F1`): the step to the next site.
const NEXT_STEP: [(i16, i16); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// What the search reads of the map for one house and BuildingType, in the
/// order it reads it. Each cell query is one `MapClass::operator[] @
/// 0x005657A0` lookup.
pub(crate) trait SiteWorld {
    /// The cell holds the house's base reservation bit (`CellClass+0xDC`).
    fn reserved(&mut self, cell: (i16, i16)) -> bool;
    /// The cell's signed level (`CellClass+0x11B`).
    fn level(&mut self, cell: (i16, i16)) -> i32;
    /// `CellRect::CheckOccupancy @ 0x00586780` for the house.
    fn clear(&mut self, rect: CellRect) -> bool;
    /// `BuildingTypeClass::CanPlaceAt` (`vt+0xA8`) for the house.
    fn can_place(&mut self, site: (i16, i16)) -> bool;
}

/// How the ordinary branch keys a perimeter cell: the key function and
/// argument `FindBaseBuildingSite` receives.
#[derive(Clone, Copy)]
pub(crate) enum SiteKey<'a> {
    /// `0x00505F80`, the building exit's.
    Ordinary,
    /// `0x00505FD0` with its argument, the base defense choice's
    /// ([`CoverageGrid::key`]); the grid is the one the choice leaves at
    /// `HouseClass+0x16060` for the search.
    Defense {
        grid: &'a CoverageGrid,
        argument: i32,
    },
}

impl SiteKey<'_> {
    fn key(self, index: i32, cell: (i16, i16), center: (i16, i16)) -> i32 {
        match self {
            Self::Ordinary => ordinary_key(index, cell, center),
            Self::Defense { grid, argument } => grid.key(index, cell, center, argument),
        }
    }
}

/// The House and BuildingType inputs of one ordinary search.
pub(crate) struct SiteSearch<'a> {
    /// `HouseClass+0x5750`, the plan centre.
    pub center: (i16, i16),
    /// The answer without a plan centre: the base centre
    /// ([`HouseState::base_origin`](crate::sim::house_state::HouseState::base_origin),
    /// `+0x5494` unless `(0, 0)`, else `+0x5490`).
    pub origin: (i16, i16),
    /// `HouseClass+0x5724`, the base perimeter.
    pub perimeter: &'a [(i16, i16)],
    /// `[AI] AIBaseSpacing=` (`RulesClass+0x1460`).
    pub spacing: i32,
    /// `Width @ 0x0045EC90` and `Height(0) @ 0x0045ECA0`.
    pub width: i32,
    pub height: i32,
    /// `ProtectWithWall=` (`+0x1765`) or `WantsExtraSpace=` (`+0x1578`).
    pub extra_border: bool,
    /// `GameMode` (`0x00A8B238`) is not a campaign.
    pub game_mode_nonzero: bool,
    pub key: SiteKey<'a>,
}

/// `HouseClass::FindBaseBuildingSite @ 0x005060B0` for BuildingType `ty` of
/// house `owner` with `key`: the site's top-left cell, or (0, 0).
pub(crate) fn find_base_building_site(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    ty: &ObjectType,
    key: SiteKey,
    registry: Option<&OverlayTypeRegistry>,
) -> (i16, i16) {
    if ty.naval {
        return crate::sim::naval_base_placement::find_naval_base_placement(sim, rules, owner)
            .map_or((0, 0), |(x, y)| (x as i16, y as i16));
    }
    let Some(house) = sim.houses.get(&owner) else {
        return (0, 0);
    };
    let mut world = SimSiteWorld::new(sim, rules, owner, ty, registry);
    let perimeter: Vec<(i16, i16)> = house
        .base_reservation()
        .perimeter_cells()
        .iter()
        .map(|&packed| unpack_base_plan_cell(packed))
        .collect();
    let (width, height) = foundation_size(ty);
    ordinary_site(
        &mut world,
        &SiteSearch {
            center: signed(house.base_plan_center()),
            origin: signed(house.base_origin()),
            perimeter: &perimeter,
            spacing: rules.ai_base_spacing,
            width,
            height,
            extra_border: ty.protect_with_wall || ty.wants_extra_space,
            game_mode_nonzero: sim.session.game_mode_nonzero(),
            key,
        },
    )
}

/// `HouseClass @ 0x0050B760`: in a campaign always; otherwise whether a base
/// reservation of house `owner` lies within `AIBaseSpacing=` of the foundation
/// of `ty` at `cell`.
pub(crate) fn reserved_near(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    ty: &ObjectType,
    cell: (i16, i16),
) -> bool {
    if !sim.session.game_mode_nonzero() {
        return true;
    }
    let mut world = SimSiteWorld::new(sim, rules, owner, ty, None);
    let (width, height) = foundation_size(ty);
    reserved_near_in(&mut world, rules.ai_base_spacing, width, height, cell, true)
}

/// The ordinary branch (`0x0050623D..0x00506B0E`), reading the map through
/// `world`.
pub(crate) fn ordinary_site(world: &mut impl SiteWorld, search: &SiteSearch) -> (i16, i16) {
    if search.center == (0, 0) {
        return search.origin;
    }
    let mut cells: Vec<(i32, (i16, i16))> = search
        .perimeter
        .iter()
        .enumerate()
        .map(|(index, &cell)| (search.key.key(index as i32, cell, search.center), cell))
        .collect();
    crate::util::retail_pointer_sort::sort_by(&mut cells, |a, b| a.0.cmp(&b.0));

    let center_level = world.level(search.center);
    let border = search.spacing.wrapping_add(i32::from(search.extra_border));
    for &(_, cell) in &cells {
        // `0x0050656C..0x005065E0`.
        let mut into_base = (0_i16, 0_i16);
        for (dx, dy) in DIRECTION_DELTAS {
            let (dx, dy) = (dx as i16, dy as i16);
            if world.reserved((cell.0.wrapping_add(dx), cell.1.wrapping_add(dy))) {
                into_base = (into_base.0.wrapping_add(dx), into_base.1.wrapping_add(dy));
            }
        }
        if into_base == (0, 0) {
            continue;
        }
        let away = away_direction(into_base);
        // `0x005066A5..0x00506706`: the words truncate.
        let offset_x = match away {
            1..=3 => border,
            5..=7 => 1 - search.width - border,
            _ => 0,
        } as i16;
        let offset_y = match away {
            3..=5 => border + 1,
            0 | 1 | 7 => 1 - search.height - border,
            _ => 0,
        } as i16;
        let mut probe_border = border;
        for phase in 0..2 {
            // `0x005068F4..0x0050697E`.
            let mut site = (
                cell.0
                    .wrapping_add(offset_x)
                    .wrapping_add(FIRST_STEP[away].0),
                cell.1
                    .wrapping_add(offset_y)
                    .wrapping_add(FIRST_STEP[away].1),
            );
            if phase == 1 {
                let (dx, dy) = DIRECTION_DELTAS[(away + 4) & 7];
                site = (
                    site.0.wrapping_add(dx as i16),
                    site.1.wrapping_add(dy as i16),
                );
                probe_border = probe_border.wrapping_sub(search.spacing);
            }
            for _ in 0..3 {
                if site_admits(world, search, site, probe_border, center_level) {
                    return site;
                }
                site = (
                    site.0.wrapping_add(NEXT_STEP[away].0),
                    site.1.wrapping_add(NEXT_STEP[away].1),
                );
            }
        }
    }
    (0, 0)
}

/// `0x0050B760` past its campaign answer: a reservation in the foundation at
/// `cell` grown by `spacing`, one more cell up and left
/// (`0x0050B7C2..0x0050B82D`, columns outside rows, first hit).
pub(crate) fn reserved_near_in(
    world: &mut impl SiteWorld,
    spacing: i32,
    width: i32,
    height: i32,
    cell: (i16, i16),
    game_mode_nonzero: bool,
) -> bool {
    if !game_mode_nonzero {
        return true;
    }
    let (x, y) = (i32::from(cell.0), i32::from(cell.1));
    let x_end = x
        .wrapping_add(width.wrapping_add(spacing.wrapping_mul(2)))
        .wrapping_add(1);
    let y_end = y
        .wrapping_add(height.wrapping_add(spacing.wrapping_mul(2)))
        .wrapping_add(1);
    let mut column = x.wrapping_sub(spacing).wrapping_sub(1);
    while column < x_end {
        let mut row = y.wrapping_sub(spacing).wrapping_sub(1);
        while row < y_end {
            // The lookup's CellStruct takes the low words.
            if world.reserved((column as i16, row as i16)) {
                return true;
            }
            row += 1;
        }
        column += 1;
    }
    false
}

/// The ordinary key `0x00505F80`.
fn ordinary_key(index: i32, cell: (i16, i16), center: (i16, i16)) -> i32 {
    let dx = (i32::from(cell.0) - i32::from(center.0)).abs();
    let dy = (i32::from(cell.1) - i32::from(center.1)).abs();
    index.wrapping_add(dx.max(dy).wrapping_mul(1000))
}

/// `0x005065E6..0x0050664C`: the eight-way direction (`DirStruct` of
/// `atan2`) of the negated sum of the steps towards the reserved neighbours.
fn away_direction(into_base: (i16, i16)) -> usize {
    usize::from(dir_from_facing16(facing16_from_delta(
        -i32::from(into_base.0),
        -i32::from(into_base.1),
    )))
}

/// The four tests of one site (`0x0050698C..0x00506A3A`), in order.
fn site_admits(
    world: &mut impl SiteWorld,
    search: &SiteSearch,
    site: (i16, i16),
    border: i32,
    center_level: i32,
) -> bool {
    let rect = CellRect::new(
        i32::from(site.0).wrapping_sub(border),
        i32::from(site.1).wrapping_sub(border),
        search.width.wrapping_add(border.wrapping_mul(2)),
        search.height.wrapping_add(border.wrapping_mul(2)),
    );
    if !world.clear(rect) || !world.can_place(site) {
        return false;
    }
    let level = world.level(site);
    (center_level - level).abs() < 3
        && reserved_near_in(
            world,
            search.spacing,
            search.width,
            search.height,
            site,
            search.game_mode_nonzero,
        )
}

/// The simulation's map for house `owner` placing `ty`. A house outside the
/// scenario's house order (fixtures only; every native house has an index)
/// holds no reservation and finds no clear site.
struct SimSiteWorld<'a> {
    sim: &'a Simulation,
    rules: &'a RuleSet,
    owner: InternedId,
    house_index: Option<i32>,
    ty: &'a ObjectType,
    registry: Option<&'a OverlayTypeRegistry>,
}

impl<'a> SimSiteWorld<'a> {
    fn new(
        sim: &'a Simulation,
        rules: &'a RuleSet,
        owner: InternedId,
        ty: &'a ObjectType,
        registry: Option<&'a OverlayTypeRegistry>,
    ) -> Self {
        Self {
            sim,
            rules,
            owner,
            house_index: sim.base_reservation_house_index(owner),
            ty,
            registry,
        }
    }
}

impl SiteWorld for SimSiteWorld<'_> {
    fn reserved(&mut self, cell: (i16, i16)) -> bool {
        self.house_index.is_some_and(|house_index| {
            self.sim.substrate.base_reservations.has_reservation(
                self.sim.resolved_terrain.as_ref(),
                cell.0.into(),
                cell.1.into(),
                house_index,
            )
        })
    }

    fn level(&mut self, cell: (i16, i16)) -> i32 {
        i32::from(
            get_cellclass_fallback(
                self.sim.resolved_terrain.as_ref(),
                cell.0.into(),
                cell.1.into(),
            )
            .signed_level(),
        )
    }

    fn clear(&mut self, rect: CellRect) -> bool {
        let sim = self.sim;
        let Some(house_index) = self.house_index else {
            return false;
        };
        check_occupancy_rect(CellRectOccupancyContext {
            native_cells: None,
            rect,
            reservation_arg: house_index,
            reservations: Some(&sim.substrate.base_reservations),
            occupancy: Some(&sim.substrate.occupancy),
            terrain_object_cells: Some(sim.terrain_objects.cell_index()),
            resolved_terrain: sim.resolved_terrain.as_ref(),
            overlay_grid: sim.overlay_grid.as_ref(),
            playfield_bounds: sim.playfield_bounds(),
        })
    }

    fn can_place(&mut self, site: (i16, i16)) -> bool {
        crate::sim::build_site::can_place_building_at(
            self.sim,
            self.rules,
            self.registry,
            self.ty,
            site,
            Some(self.owner),
            None,
            None,
        )
    }
}

/// `BuildingTypeClass::Width @ 0x0045EC90` and `Height(0) @ 0x0045ECA0`.
pub(crate) fn foundation_size(ty: &ObjectType) -> (i32, i32) {
    let (width, height) = ty.foundation.dimensions();
    (i32::from(width), i32::from(height))
}

/// A House cell field as its native signed `CellStruct` words.
fn signed(cell: (u16, u16)) -> (i16, i16) {
    (cell.0 as i16, cell.1 as i16)
}

#[cfg(test)]
#[path = "ai_base_site_tests.rs"]
mod tests;
