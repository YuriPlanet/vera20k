//! Where the map ends: the ground the house holds, the ground the enemy holds,
//! and the line between them.
//!
//! Every layer here has been answering a spatial question on its own —
//! `advance` asks "is the target's base closer than mine", `avoidance` asks
//! "is this cell under a gun", `expand` asks "is there room for a second yard"
//! — and each invented its own reading of the map. That is why the same map
//! produced a base with a captured refinery as its centre, an army that walked
//! into the one defended corner, and a second-base attempt that tried to build
//! inside the enemy's base.
//!
//! This module is the one reading. It does not replace those layers: it gives
//! them a shared vocabulary so their answers agree.
//!
//! ## The four answers
//!
//! - **Held** — ground the house stands on: its own buildings and the cells
//!   around them. A defensive line drawn outward from these is the ground the
//!   house already owns.
//! - **Contested** — ground the house can see the enemy standing on now. The
//!   vision is the projection's (`fog`-checked), so this is what a player
//!   could see, not what the engine knows.
//! - **Enemy** — ground the enemy's *buildings* stand on. Distinct from
//!   contested on purpose: a raid walks through contested ground, and what it
//!   must not walk through is a base. This is what "do not build here" means.
//! - **Safe** — held ground far enough from everything contested or enemy.
//!   Where a second base may go and where a hurt unit may stand.
//!
//! ## Why distance bands and not a grid
//!
//! A per-cell threat grid is the textbook answer and M28AI does not have one
//! either: its threat is aggregated per pathing zone (5x5 world units), far
//! too coarse for a RA2 skirmish. A RA2 map is at most 256x256, so a grid is
//! affordable — but a grid alone does not answer "may I build here", because
//! that is a question about *what the enemy can reach*, not about the cell.
//!
//! So the reading is distance-based: every question is "how far is the nearest
//! thing of kind K", answered over a short list of cells. That is cheap enough
//! to run every term, deterministic (integer cells, squared distances, storage
//! order tie-breaks), and it composes: a second base wants a cell that is far
//! from enemy buildings and near ore, which is two distances and not two maps.
//!
//! ## The line
//!
//! [`Front`] is the push's own reading: the direction from the house's base
//! toward the enemy's, and how far along it the army has to travel. `advance`
//! and `expand` both need it, and neither should compute it twice.

use std::collections::BTreeSet;

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::advance::cell_distance;

/// How near its own buildings ground counts as the house's, in cells.
///
/// One foundation's width plus the gap a unit needs: a base is its buildings
/// and the apron they defend, not a single point.
pub(super) const HELD_RADIUS_CELLS: i32 = 6;

/// How far from anything contested or enemy ground still counts as safe, in
/// cells. Long enough that a defence cannot shell it and a raid cannot reach
/// it in one dash; short enough that a second base is not on the far side of
/// the map from the first.
pub(super) const SAFE_DISTANCE_CELLS: i32 = 14;

/// What the house may do with a piece of ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Ground {
    /// The house's own buildings and the apron around them.
    Held,
    /// Far from everything the enemy is or stands on: where a second base goes.
    Safe,
    /// The enemy can see it, or its army is on it.
    Contested,
    /// The enemy's own buildings stand on it, or near enough to shell it.
    Enemy,
}

impl Ground {
    /// Whether a building may be placed here. A second yard in contested
    /// ground is a yard the enemy destroys for free; one inside their base is
    /// a yard they never let stand at all.
    pub(super) const fn buildable(self) -> bool {
        matches!(self, Self::Held | Self::Safe)
    }

    /// Whether a hurt unit may recover here.
    pub(super) const fn recoverable(self) -> bool {
        matches!(self, Self::Held | Self::Safe)
    }
}

/// The line between the two sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Front {
    /// Where the house's base is.
    pub(super) home: (u16, u16),
    /// Where the enemy's nearest building stands, when one has been seen.
    pub(super) their_base: Option<(u16, u16)>,
}

impl Front {
    /// How far the army has to travel to reach the enemy's base.
    pub(super) fn length(self) -> Option<i32> {
        self.their_base.map(|base| cell_distance(self.home, base))
    }

    /// Whether `cell` is on the enemy's half: past the midpoint of the line,
    /// or inside their base.
    ///
    /// This is the test `expand` asks before it sends a yard maker out, and the
    /// one the wishlist's "new buildings appear in the enemy's face" is about.
    pub(super) fn beyond_the_line(self, cell: (u16, u16)) -> bool {
        let Some(their_base) = self.their_base else {
            return false;
        };
        let home_to_cell = cell_distance(self.home, cell);
        let home_to_them = cell_distance(self.home, their_base);
        home_to_cell > home_to_them / 2
    }
}

/// The one reading of the map every layer shares.
#[derive(Debug, Clone, Default)]
pub(super) struct Space {
    /// The house's own building cells.
    held: Vec<(u16, u16)>,
    /// Cells the enemy's buildings stand on.
    enemy_ground: Vec<(u16, u16)>,
    /// Cells the house can see the enemy standing on right now.
    contested: Vec<(u16, u16)>,
    /// The line, when both ends are known.
    front: Option<Front>,
}

impl Space {
    /// Read the map from the same projection every other layer reads.
    ///
    /// Own buildings come from the census, not from a second scan: the census
    /// already visits every own entity and already knows which are structures.
    /// The enemy's buildings and the contested cells both come from the
    /// fog-checked enemy list, so neither can see through the shroud.
    pub(super) fn read(
        obs: &Observation<'_>,
        rules: &RuleSet,
        held: &[(u16, u16)],
        base: Option<(u16, u16)>,
    ) -> Self {
        let held_cells: Vec<(u16, u16)> = held.to_vec();
        // Two different lists, because they answer different questions: an
        // enemy *building* is a place not to build, an enemy *unit* is a place
        // an army is standing.
        let mut enemy_ground = Vec::new();
        let mut contested = Vec::new();
        for enemy in obs.visible_enemy_entities() {
            let is_building = obs
                .object_type(rules, enemy.type_id)
                .is_some_and(|ty| ty.category == ObjectCategory::Building);
            contested.push(enemy.cell);
            if is_building {
                enemy_ground.push(enemy.cell);
            }
        }
        let their_base = enemy_ground
            .iter()
            .copied()
            .min_by_key(|cell| base.map_or(i32::MAX, |home| cell_distance(home, *cell)));
        let front = base.map(|home| Front { home, their_base });
        Self {
            held: held_cells,
            enemy_ground,
            contested,
            front,
        }
    }

    /// What the house may do with `cell`.
    ///
    /// Ordered worst-first so a cell that is both worn by the enemy and near
    /// their base answers `Enemy`: the stronger statement wins, because a cell
    /// the enemy's buildings cover is not made safer by having no unit on it.
    pub(super) fn ground_at(&self, cell: (u16, u16)) -> Ground {
        if self
            .enemy_ground
            .iter()
            .any(|enemy| cell_distance(*enemy, cell) <= SAFE_DISTANCE_CELLS)
        {
            return Ground::Enemy;
        }
        if self
            .contested
            .iter()
            .any(|enemy| cell_distance(*enemy, cell) <= SAFE_DISTANCE_CELLS)
        {
            return Ground::Contested;
        }
        if self
            .held
            .iter()
            .any(|held| cell_distance(*held, cell) <= HELD_RADIUS_CELLS)
        {
            return Ground::Held;
        }
        Ground::Safe
    }

    /// Whether the house already holds the ground around `cell`.
    pub(super) fn held_near(&self, cell: (u16, u16), cells: i32) -> bool {
        self.held
            .iter()
            .any(|held| cell_distance(*held, cell) <= cells)
    }

    /// The line between the two sides, when both ends are known.
    pub(super) const fn front(&self) -> Option<Front> {
        self.front
    }

    /// Cells the house may expand onto: safe ground it does not already hold,
    /// nearest home first.
    ///
    /// The cells come from a ring around the house's own buildings rather than
    /// from a map scan: a second base grows out from the first, and a cell
    /// across the map is a base with no supply line.
    pub(super) fn expansion_cells(
        &self,
        reach: i32,
        step: i32,
        extent: (u16, u16),
    ) -> Vec<(u16, u16)> {
        let Some(home) = self.front.map(|front| front.home) else {
            return Vec::new();
        };
        let mut seen: BTreeSet<(u16, u16)> = BTreeSet::new();
        let mut cells = Vec::new();
        let mut radius = HELD_RADIUS_CELLS;
        while radius <= reach {
            for dx in (-radius..=radius).step_by(step.max(1) as usize) {
                for dy in (-radius..=radius).step_by(step.max(1) as usize) {
                    if dx.abs() != radius && dy.abs() != radius {
                        continue;
                    }
                    let Some(x) = offset_cell(home.0, dx, extent.0) else {
                        continue;
                    };
                    let Some(y) = offset_cell(home.1, dy, extent.1) else {
                        continue;
                    };
                    if !seen.insert((x, y)) {
                        continue;
                    }
                    if self.ground_at((x, y)).buildable()
                        && !self.held_near((x, y), HELD_RADIUS_CELLS)
                    {
                        cells.push((x, y));
                    }
                }
            }
            radius += step.max(1);
        }
        cells.sort_by_key(|cell| (cell_distance(home, *cell), *cell));
        cells
    }
}

/// `base + delta`, or `None` when that leaves the map.
fn offset_cell(base: u16, delta: i32, extent: u16) -> Option<u16> {
    let value = i32::from(base).checked_add(delta)?;
    (0..i32::from(extent))
        .contains(&value)
        .then(|| u16::try_from(value).ok())
        .flatten()
}
