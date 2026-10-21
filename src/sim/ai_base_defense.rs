//! The computer's base defense choice: `HouseClass::AI_ChooseNextProduction
//! @ 0x00506EF0`, which `AI_Choose_Building` (`sim::ai_base_building`) calls
//! for a `-1` node, or a `WallTower=` node without a cell, when its wall draw
//! builds no walls (`0x004FE633`).
//!
//! Without a perimeter vector, as every VERA plan is (module residual):
//! - Coverage (`0x005070D5..0x0050722F`): each building of the house
//!   (`HouseClass+0x6C`, in order) off the plan centre (`+0x5750`) whose
//!   `AntiAirValue=`, `AntiArmorValue=` and `AntiInfantryValue=` (BuildingClass
//!   `vt+0x2D4`, `+0x2D8`, `+0x2DC`) sum above zero adds them to its quadrant
//!   ([`quadrant`]) and spreads each into its own grid over the house's
//!   reserved bounds (`+0x5754`, [`CoverageGrid::spread`]).
//! - The weakest quadrant (`0x00507236..0x0050727B`): the first whose three
//!   sums total least, below 999999.
//! - The threat (`AI_UpdateEnemyThreatRatios @ 0x00508150`,
//!   [`threat_ratios`]): the designated enemy's (`+0x5600`) force values
//!   (`house_tracking`), each moved by a draw within
//!   `AIForcePredictionFudge=` percent, as shares of their total.
//! - The category (`0x0050738C..0x00507494`): each threat share less the
//!   weakest quadrant's share of that value (x87, 53-bit chop, stored as
//!   floats); air when its gap is the largest, else armor when its gap exceeds
//!   infantry's, else infantry.
//! - The candidates ([`DefenseChoice::candidates`], `0x00507B80`,
//!   `0x00507D70`, `0x00507F60`): the side's `[AI] *BaseDefenses=` list, last
//!   to first. An empty category falls back to armor, infantry, then air; with
//!   none the choice fails (`0x00507499..0x00507519`).
//! - The pick: a single candidate, else a `RandomRanged(1, total)` draw over
//!   the category values (`0x00507759`), cumulative (unsigned).
//! - The site: `FindBaseBuildingSite` with the defense key and twice the
//!   weakest quadrant (`0x00507A0B..0x00507A20`, `sim::ai_base_site`), for the
//!   node's own type when it has one (the WallTower), else the pick. No site
//!   fails the choice.
//! - The writes ([`crate::sim::base_plan::BasePlanState::place_defense`],
//!   `0x00507A4C..0x00507B7B`).
//!
//! The grids, the threat shares (`+0x1609C..+0x160A4`, which only this call
//! writes before reading) and the grid pointer (`+0x16060`, cleared on every
//! return at `0x0050754F`) live only for the call.
//!
//! Evidence: instruction reading, and native execution
//! (`tools/ai_base_defense_oracle.py`, compared in `ai_base_defense_tests.rs`)
//! of the spread, the key, the threat shares with their draws, and 57 whole
//! choices with the draws and the site search call.
//!
//! RESIDUALS:
//! - The perimeter vector variant (`HouseClass+0x5738`, passed when `+0x5748`
//!   is positive, `0x004FE61D..0x004FE62F`) is not ported: only the `-3`
//!   perimeter scan `0x005082C0` fills it, and VERA removes `-3` nodes without
//!   it (`sim::ai_base_building`). Trigger: a map plan with a `-3` node.
//!   Effect: its defenses are keyed by the whole perimeter rather than the
//!   weakest quadrant's cells.
//! - Native reads VERA defines, none reachable with retail rules: a quadrant
//!   total of 999999 or more in all four quadrants leaves the weakest at -1,
//!   whose sums are read from the stack (VERA reads 0); the fudge vector is
//!   indexed unchecked by the difficulty (VERA reads 0 past its end); a key
//!   cell outside the grid buffer reads the heap around it (VERA reads 0;
//!   the perimeter lies in the reserved bounds unless a reservation started
//!   at row or column 0, which the bounds writer mistakes for unset); the
//!   WallTower's successor is read past the node count (VERA writes nothing
//!   there, nor is anything past the count ever read). A negative grid size
//!   fails the allocation natively (VERA: an empty grid).

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_buildable::owner_allows;
use crate::sim::base_plan::pack_base_plan_cell;
use crate::sim::base_plan_generation::prerequisites_satisfied;
use crate::sim::house_tracking::ForceValues;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::direction_tables::{facing16_from_delta, round_facing16_to_8};
use crate::util::native_x87::{
    MaskedX87Chop53 as X87, MaskedX87Ordering, MaskedX87Value, NativeF32Bits, NativeF64Bits,
    X87Chop53, sqrt_approx_f32,
};

/// The float the threat shares take without an enemy or forces
/// (`0x0050826C`, 0.33).
const THIRD: NativeF32Bits = NativeF32Bits::from_bits(0x3EA8_F5C3);
/// `[0x007EAAE0]`, the float 0.01 that scales the fudge percent.
const PERCENT: NativeF32Bits = NativeF32Bits::from_bits(0x3C23_D70A);
/// What `0x00508150` adds to each enemy force value (`0x0050818E`).
const FORCE_FLOOR: i32 = 3000;

/// The three defense values, in the order the grids and lists take them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    /// `AntiAirValue=` (`+0x1524`, `vt+0x2D4`, list `0x00507B80`).
    Air,
    /// `AntiArmorValue=` (`+0x1528`, `vt+0x2D8`, list `0x00507D70`).
    Armor,
    /// `AntiInfantryValue=` (`+0x152C`, `vt+0x2DC`, list `0x00507F60`).
    Infantry,
}

impl Category {
    const ALL: [Self; 3] = [Self::Air, Self::Armor, Self::Infantry];

    fn value(self, ty: &ObjectType) -> i32 {
        match self {
            Self::Air => ty.anti_air_value,
            Self::Armor => ty.anti_armor_value,
            Self::Infantry => ty.anti_infantry_value,
        }
    }
}

/// One coverage grid over the house's reserved bounds (`HouseClass+0x5754`
/// left, top, width, height), zeroed for each choice
/// (`0x00506F65..0x00506FDD`).
pub(crate) struct CoverageGrid {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    cells: Vec<i32>,
}

impl CoverageGrid {
    pub(crate) fn new((left, top, width, height): (i32, i32, i32, i32)) -> Self {
        let count = usize::try_from(width.wrapping_mul(height)).unwrap_or(0);
        Self {
            left,
            top,
            width,
            height,
            cells: vec![0; count],
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn cells(&self) -> &[i32] {
        &self.cells
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn cells_mut(&mut self) -> &mut [i32] {
        &mut self.cells
    }

    /// `HouseClass @ 0x00506D50`: spread `value` (1 when not positive) from
    /// the building at `cell` over the grid cells within its 13x13 window
    /// whose `Sqrt_Approx` distance is below 6, each taking
    /// `ftol(value / ((max(distance, 1) - 1) * 0.1 + 1) + cell)`.
    pub(crate) fn spread(&mut self, cell: (i16, i16), value: i32) {
        let value = if value > 0 { value } else { 1 };
        let one = X87::load_f64(NativeF64Bits::ONE);
        let six = X87::load_f64(NativeF64Bits::from_bits(0x4018_0000_0000_0000));
        let tenth = X87::load_f64(NativeF64Bits::from_bits(0x3FB9_9999_9999_999A));
        let (x, y) = (i32::from(cell.0), i32::from(cell.1));
        let x_start = (x - 6).max(self.left);
        let x_end = self.left.wrapping_add(self.width).min(x + 6);
        let y_start = (y - 6).max(self.top);
        let y_end = self.top.wrapping_add(self.height).min(y + 6);
        for row in y_start..y_end {
            let dy = row - y;
            for column in x_start..x_end {
                let dx = column - x;
                let squared = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
                let root = sqrt_approx_f32(X87Chop53::load_i32(squared))
                    .expect("a squared window distance is a small finite value");
                let distance = X87::load_f32(root);
                if X87::compare(distance, six) != MaskedX87Ordering::Less {
                    continue;
                }
                let distance = if X87::compare(distance, one) == MaskedX87Ordering::Less {
                    one
                } else {
                    distance
                };
                let falloff = X87::add(X87::mul(X87::sub(distance, one), tenth), one);
                let index = self
                    .width
                    .wrapping_mul(row - self.top)
                    .wrapping_add(column - self.left) as usize;
                let sum = X87::add(
                    X87::div(X87::load_i32(value), falloff),
                    X87::load_i32(self.cells[index]),
                );
                self.cells[index] = X87::ftol_i32_low_masked(sum);
            }
        }
    }

    /// The defense key `0x00505FD0` for the perimeter cell at `index`:
    /// `index + 1000 * ((coverage << 7) + turn)`, where `turn` is 0 for
    /// argument -1, else the distance (of 256 steps around, folded to at
    /// most 128) between the cell's direction from `center` and the
    /// argument's (`argument * 32`).
    pub(crate) fn key(
        &self,
        index: i32,
        cell: (i16, i16),
        center: (i16, i16),
        argument: i32,
    ) -> i32 {
        let coverage = self.value_at(cell);
        let turn = if argument == -1 {
            0
        } else {
            let dx = i32::from(cell.0) - i32::from(center.0);
            let dy = i32::from(cell.1) - i32::from(center.1);
            let direction = i32::from(round_facing16_to_8(facing16_from_delta(dx, dy)));
            let turn = argument
                .wrapping_shl(5)
                .wrapping_sub(direction)
                .wrapping_abs();
            if turn >= 0x80 { 0xFF - turn } else { turn }
        };
        index.wrapping_add(
            coverage
                .wrapping_shl(7)
                .wrapping_add(turn)
                .wrapping_mul(1000),
        )
    }

    /// The key's unchecked read `(y - top) * width - left + x`
    /// (`0x00505FDA..0x00506002`); outside the buffer, 0 (module residual).
    fn value_at(&self, cell: (i16, i16)) -> i32 {
        let index = (i32::from(cell.1) - self.top)
            .wrapping_mul(self.width)
            .wrapping_sub(self.left)
            .wrapping_add(i32::from(cell.0));
        usize::try_from(index)
            .ok()
            .and_then(|index| self.cells.get(index))
            .copied()
            .unwrap_or(0)
    }
}

/// The quadrant of a building `delta` cells from the plan centre: its native
/// facing (`atan2` of `-dy, dx` through `0x004CAE30`, as `DirStruct`), in
/// four steps (`0x00507168..0x0050717B`).
pub(crate) fn quadrant(delta: (i16, i16)) -> usize {
    let facing = u32::from(facing16_from_delta(i32::from(delta.0), i32::from(delta.1)));
    ((((facing >> 13) + 1) >> 1) & 3) as usize
}

/// The threat shares `AI_UpdateEnemyThreatRatios` stores (`+0x1609C`
/// vehicles, `+0x160A0` air, `+0x160A4` infantry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThreatRatios {
    pub vehicles: NativeF32Bits,
    pub air: NativeF32Bits,
    pub infantry: NativeF32Bits,
}

/// `HouseClass::AI_UpdateEnemyThreatRatios @ 0x00508150`: without an enemy a
/// third each; otherwise each of the enemy's air, vehicle and infantry values
/// plus 3000, in that order, moved by `RandomRanged(-r, r)` with
/// `r = ftol(fudge * 0.01f * value)`, as its share of their sum (a third each
/// when the sum is not positive).
pub(crate) fn threat_ratios(
    enemy: Option<ForceValues>,
    fudge: i32,
    mut draw: impl FnMut(i32, i32) -> i32,
) -> ThreatRatios {
    let thirds = ThreatRatios {
        vehicles: THIRD,
        air: THIRD,
        infantry: THIRD,
    };
    let Some(enemy) = enemy else {
        return thirds;
    };
    let mut air = enemy.air.wrapping_add(FORCE_FLOOR);
    let mut vehicles = enemy.vehicles.wrapping_add(FORCE_FLOOR);
    let mut infantry = enemy.infantry.wrapping_add(FORCE_FLOOR);
    for value in [&mut air, &mut vehicles, &mut infantry] {
        let range = X87::ftol_i32_low_masked(X87::mul(
            X87::mul(X87::load_i32(fudge), X87::load_f32(PERCENT)),
            X87::load_i32(*value),
        ));
        *value = value.wrapping_add(draw(range.wrapping_neg(), range));
    }
    let total = air.wrapping_add(vehicles).wrapping_add(infantry);
    if total <= 0 {
        return thirds;
    }
    let share = |value: i32| {
        X87::store_f32_masked_chop(X87::div(X87::load_i32(value), X87::load_i32(total)))
    };
    ThreatRatios {
        vehicles: share(vehicles),
        air: share(air),
        infantry: share(infantry),
    }
}

/// A House's inputs to one choice.
pub(crate) struct DefenseChoice<'r> {
    pub rules: &'r RuleSet,
    /// `HouseClass+0x5750`, the plan centre.
    pub center: (i16, i16),
    /// `HouseClass+0x5754..+0x5760`.
    pub bounds: (i32, i32, i32, i32),
    /// `HouseClass+0x6C`: each building's cell (`vt+0x1B8`) and type.
    pub buildings: Vec<((i16, i16), &'r ObjectType)>,
    /// `HouseTypeClass+0xBC`.
    pub side_index: i32,
    /// `1 << FindIndexOfName(HouseTypeClass+0x98)` (`0x00507B92`); retail
    /// leaves every country's `ParentCountry=` at its own name.
    pub country_bit: u32,
    /// `HouseClass+0x1D4`.
    pub tech_level: i32,
    /// Per-choice immutable facts, aligned with the side's defense list.
    /// Gathered from the sole live type owner before this call's RNG draws;
    /// rebuilt for every choice, so no trigger needs to invalidate a cache.
    candidate_tech_levels: Vec<Option<i32>>,
    /// The enemy's force values; `None` for `+0x5600 == -1`.
    pub enemy: Option<ForceValues>,
    /// `AIForcePredictionFudge=` at the house's difficulty (`+0x184`).
    pub fudge: i32,
    /// The node's type: -1, or the `WallTower=` index.
    pub node_type: i32,
}

/// What the choice settled before its site search.
pub(crate) struct DefensePick<'r> {
    /// The chosen base defense.
    pub defense: &'r ObjectType,
    /// The BuildingType the site is sought for: the node's own for a
    /// WallTower node, else the defense (`0x005077DA..0x005077F7`).
    pub place: &'r ObjectType,
    /// The chosen category's grid, which the defense key reads.
    pub grid: CoverageGrid,
    /// The key's argument: twice the weakest quadrant.
    pub argument: i32,
}

impl<'r> DefenseChoice<'r> {
    /// The inputs of house `owner` for its node at `index`.
    /// `AI_ChooseNextProduction @ 0x00506EF0` reads each building's cell through
    /// `vt+0x1B8` in its coverage pass (`0x005070D5..0x0050722F`).
    fn of_house(
        sim: &Simulation,
        rules: &'r RuleSet,
        owner: InternedId,
        index: usize,
    ) -> Option<Self> {
        let house = sim.houses.get(&owner)?;
        let node_type = house.base_plan().nodes().get(index)?.type_or_control;
        let buildings = house
            .base_projection()
            .buildings()
            .iter()
            .filter_map(|&id| {
                let building = sim.substrate.entities.get(id)?;
                let ty = sim.object_type(building.type_ref(), rules)?;
                Some((
                    (building.position.rx() as i16, building.position.ry() as i16),
                    ty,
                ))
            })
            .collect();
        let country_bit = rules.country_owner_mask(sim.interner.resolve(house.house_type_id()));
        let enemy = house
            .enemy_house()
            .and_then(|enemy| sim.houses.get(&enemy))
            .map(|enemy| enemy.tracking().force_values());
        let fudge = house.difficulty_value(&rules.general.ai_force_prediction_fudge);
        let (x, y) = house.base_plan_center();
        Some(Self {
            rules,
            center: (x as i16, y as i16),
            bounds: house.base_reservation().bounds(),
            buildings,
            side_index: house.side_index(),
            country_bit,
            tech_level: house.tech_level(),
            candidate_tech_levels: rules
                .base_defense_types(house.side_index())
                .iter()
                .map(|id| {
                    rules
                        .object_in_category(ObjectCategory::Building, id)
                        .map(|object| sim.object_tech_level(object))
                })
                .collect(),
            enemy,
            fudge,
            node_type,
        })
    }

    /// The body up to the site search: coverage, the weakest quadrant, the
    /// threat (its three draws), the category and the pick (its draw).
    /// `draw` answers `RandomRanged` on the Scenario RNG. `None` when no list
    /// holds a candidate.
    pub(crate) fn pick(&self, mut draw: impl FnMut(i32, i32) -> i32) -> Option<DefensePick<'r>> {
        let mut grids = Category::ALL.map(|_| CoverageGrid::new(self.bounds));
        let mut sums = [[0_i32; 4]; 3];
        for &(cell, ty) in &self.buildings {
            let delta = (
                cell.0.wrapping_sub(self.center.0),
                cell.1.wrapping_sub(self.center.1),
            );
            if delta == (0, 0) {
                continue;
            }
            let quadrant = quadrant(delta);
            let values = Category::ALL.map(|category| category.value(ty));
            if values[2].wrapping_add(values[1]).wrapping_add(values[0]) <= 0 {
                continue;
            }
            for (slot, value) in values.into_iter().enumerate() {
                sums[slot][quadrant] = sums[slot][quadrant].wrapping_add(value);
                grids[slot].spread(cell, value);
            }
        }
        let mut least = 999_999;
        let mut weakest = None;
        #[expect(
            clippy::needless_range_loop,
            reason = "sums three per-category quadrant tables in native order"
        )]
        for quadrant in 0..4 {
            let total = sums[2][quadrant]
                .wrapping_add(sums[1][quadrant])
                .wrapping_add(sums[0][quadrant]);
            if total < least {
                least = total;
                weakest = Some(quadrant);
            }
        }
        let weakest_sums = Category::ALL
            .map(|category| weakest.map_or(0, |quadrant| sums[category as usize][quadrant]));

        let threat = threat_ratios(self.enemy, self.fudge, &mut draw);

        // `0x00507361..0x005073FA`: the weakest quadrant's shares, then the
        // gaps, each stored as a float.
        let shares: [MaskedX87Value; 3] = if least != 0 {
            let total = X87::load_f32(X87::store_f32_masked_chop(X87::load_i32(least)));
            weakest_sums.map(|sum| X87::div(X87::load_i32(sum), total))
        } else {
            [X87::load_f32(NativeF32Bits::POSITIVE_ZERO); 3]
        };
        let gap = |threat: NativeF32Bits, share: MaskedX87Value| {
            X87::load_f32(X87::store_f32_masked_chop(X87::sub(
                X87::load_f32(threat),
                share,
            )))
        };
        let air = gap(threat.air, shares[0]);
        let armor = gap(threat.vehicles, shares[1]);
        let infantry = gap(threat.infantry, shares[2]);
        let greater = |lhs, rhs| X87::compare(lhs, rhs) == MaskedX87Ordering::Greater;
        let preferred = if greater(air, armor) && greater(air, infantry) {
            Category::Air
        } else if greater(armor, infantry) {
            Category::Armor
        } else {
            Category::Infantry
        };

        let owned = self.owned_types();
        let lists = Category::ALL.map(|category| self.candidates(category, &owned));
        let category = std::iter::once(preferred)
            .chain([Category::Armor, Category::Infantry, Category::Air])
            .find(|&category| !lists[category as usize].is_empty())?;
        let list = &lists[category as usize];
        let defense = match list.as_slice() {
            [only] => *only,
            _ => weighted_pick(list, category, &mut draw)?,
        };
        let place = if self.node_type >= 0 {
            self.rules.building_type_at(self.node_type)?
        } else {
            defense
        };
        let [air_grid, armor_grid, infantry_grid] = grids;
        let grid = match category {
            Category::Air => air_grid,
            Category::Armor => armor_grid,
            Category::Infantry => infantry_grid,
        };
        Some(DefensePick {
            defense,
            place,
            grid,
            argument: weakest.map_or(-2, |quadrant| 2 * quadrant as i32),
        })
    }

    /// The prerequisite vector (`0x005072B3..0x0050735F`): the types of the
    /// house's buildings that are neither `IsBaseDefense=` nor the
    /// `WallTower=`, repeats kept.
    fn owned_types(&self) -> Vec<&'r ObjectType> {
        let wall_tower = self.rules.general.building_types.wall_tower.as_deref();
        self.buildings
            .iter()
            .map(|&(_, ty)| ty)
            .filter(|ty| {
                !ty.is_base_defense
                    && !wall_tower.is_some_and(|name| name.eq_ignore_ascii_case(&ty.id))
            })
            .collect()
    }

    /// `0x00507B80` and its two siblings: the side's list, last to first,
    /// keeping each type the country may own, within the tech level, with a
    /// positive value of `category` and whose prerequisites `owned` meets.
    fn candidates(&self, category: Category, owned: &[&ObjectType]) -> Vec<&'r ObjectType> {
        self.rules
            .base_defense_types(self.side_index)
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(index, id)| {
                Some((
                    self.rules
                        .object_in_category(ObjectCategory::Building, id)?,
                    self.candidate_tech_levels[index]?,
                ))
            })
            .filter(|(ty, tech_level)| {
                owner_allows(ty, self.country_bit, self.rules)
                    && *tech_level <= self.tech_level
                    && category.value(ty) > 0
                    && prerequisites_satisfied(ty, owned, self.rules)
            })
            .map(|(ty, _)| ty)
            .collect()
    }
}

/// `0x0050767B..0x00507787`: a `RandomRanged(1, total)` draw over the
/// candidates' `category` values, the first whose running (unsigned) sum
/// reaches it; no draw and no pick for a zero total.
fn weighted_pick<'r>(
    list: &[&'r ObjectType],
    category: Category,
    draw: &mut impl FnMut(i32, i32) -> i32,
) -> Option<&'r ObjectType> {
    let total = list
        .iter()
        .fold(0_i32, |total, ty| total.wrapping_add(category.value(ty)));
    if total == 0 {
        return None;
    }
    let roll = draw(1, total) as u32;
    let mut running = 0_u32;
    list.iter().copied().find(|ty| {
        running = running.wrapping_add(category.value(ty) as u32);
        running >= roll
    })
}

/// `HouseClass::AI_ChooseNextProduction @ 0x00506EF0` for house `owner`'s
/// node at `index`, without a perimeter vector: whether the node now holds
/// a defense (or the WallTower its cell).
pub(crate) fn choose_next_production(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    index: usize,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let Some(choice) = DefenseChoice::of_house(sim, rules, owner, index) else {
        return false;
    };
    let Some(pick) = choice.pick(|low, high| sim.scenario_rng.next_range_i32_inclusive(low, high))
    else {
        return false;
    };
    let site = crate::sim::ai_base_site::find_base_building_site(
        sim,
        rules,
        owner,
        pick.place,
        crate::sim::ai_base_site::SiteKey::Defense {
            grid: &pick.grid,
            argument: pick.argument,
        },
        registry,
    );
    if site == (0, 0) {
        return false;
    }
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.base_plan_mut().place_defense(
            index,
            pick.defense.base_plan_type_index,
            pack_base_plan_cell(i32::from(site.0), i32::from(site.1)),
        );
    }
    true
}

#[cfg(test)]
#[path = "ai_base_defense_tests.rs"]
mod tests;
