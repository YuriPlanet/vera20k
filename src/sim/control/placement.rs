//! Where a finished building goes: the native site first, a bounded sweep as
//! the fallback.
//!
//! The native answer is `HouseClass::FindBaseBuildingSite` (`0x005060B0`,
//! [`crate::sim::ai_base_site::find_base_building_site`]) — the same search the
//! computer AI makes before it places a structure. It keys off the house's base
//! reservation and plan centre, so for a house that has played as a player (no
//! `+0x5750` plan centre, and a reservation that may be empty) it can answer
//! "no site" or a cell the sidebar's build area refuses.
//!
//! Hence the second half. When the native search gives nothing admissible, the
//! sweep walks outwards from the house's Construction Yard and then its other
//! structures — ring by ring, in a fixed order — and returns the `attempt`-th
//! cell the placement gate admits. That is a policy choice, not a port, and it
//! is recorded as such: a human house has no native plan centre to ask, and the
//! retail game never needs this path because a human clicks. The `attempt`
//! parameter is how the caller moves on after the receiver refuses a cell.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_base_site::{SiteKey, find_base_building_site};
use crate::sim::behavior::view::cell_distance;
use crate::sim::control::observation::Observation;
use crate::sim::strategy::assist::trace_enabled;
use crate::sim::intern::InternedId;

/// How far from an anchor the fallback sweep looks, in cells. A power plant is
/// 2x2 and a new foundation must touch an existing `BaseNormal=` one within its
/// `Adjacent=`, so the nearest admissible ring is always well inside this.
const PLACEMENT_SCAN_RADIUS: i32 = 12;

/// How far around a candidate the openness score looks, in cells: a two-cell
/// ring catches the difference between a cell on a lane and one wedged in a
/// corner between two foundations.
const OPENNESS_RADIUS: i32 = 2;

/// How many rings past the nearest admissible one a roomier cell may be taken
/// from. Four is two buildings' width: enough to step out of the gap the last
/// foundation left — which is the only way a base that fills its first ring
/// keeps a lane — and not enough to leave the base. It was two, and a base
/// that had filled its ring had nowhere left to go but tighter.
const PLACEMENT_WINDOW_RINGS: i32 = 4;

/// The share of the roomiest admissible cell a candidate must have open around
/// it to be built on at all, as `NUMER / DENOM`: three fifths.
///
/// The floor is what stops the sweep from closing the last one-cell gap. It is
/// measured against the roomiest cell *in the window*, not a fixed number, so
/// an open map asks for room and a hemmed-in base asks for what it can get.
const OPENNESS_FLOOR_NUMER: i32 = 3;
const OPENNESS_FLOOR_DENOM: i32 = 5;

/// How many admitted cells the sweep counts before starting over: a cell the
/// gate admits but the receiver refuses must not be asked for forever.
pub const PLACEMENT_ATTEMPTS: u32 = 64;

/// The lane a foundation must leave between itself and the next building, in
/// cells.
///
/// One for a large foundation, two for a small one. The two-cell moat is what
/// keeps a lane an infantry squad can walk down, and a 1x1 or 1x2 shed needs it
/// to be worth anything. A 4x3 war factory does not: grown by two on every side
/// it needs 8x6 = 48 clear cells, and a base with fifty buildings cannot offer
/// that anywhere, so the sweep refused every cell and the build was abandoned.
///
/// Measured at `--credits 100000`, 30000 ticks, same seed:
///
///     gap 2   `no cell for GAWEAP` 59 times, 23 structures put down
///     gap 1   `no cell` 0 times, exchange 1.95 -> 2.16
///
/// The lane a factory needs is the one it drives out of, which its own footprint
/// already provides; the moat is for the buildings that have no vehicle to walk
/// out.
fn gap_for(foundation: (u16, u16)) -> i32 {
    if foundation.0.max(foundation.1) >= 3 {
        1
    } else {
        BUILDING_GAP_CELLS
    }
}

/// The Chebyshev distance from a cell to a building's footprint: 0 inside it, 1
/// on the ring around it.
fn rect_distance(cell: (u16, u16), origin: (u16, u16), foundation: (u16, u16)) -> i32 {
    let (w, h) = (
        i32::from(foundation.0.max(1)),
        i32::from(foundation.1.max(1)),
    );
    let (x, y) = (i32::from(cell.0), i32::from(cell.1));
    let (ox, oy) = (i32::from(origin.0), i32::from(origin.1));
    let dx = if x < ox {
        ox - x
    } else if x >= ox + w {
        x - (ox + w) + 1
    } else {
        0
    };
    let dy = if y < oy {
        oy - y
    } else if y >= oy + h {
        y - (oy + h) + 1
    } else {
        0
    };
    dx.max(dy)
}

/// The sectors the base grows into around its first anchor: eight, so a house
/// that has built east does not keep building east.
const SECTORS: usize = 8;

/// Which of the eight sectors around `origin` a cell lies in: east first, then
/// clockwise. A sector is cardinal when one offset is at least twice the other,
/// so a broad wedge counts as the side it points at and only the rest is a
/// diagonal.
///
/// Integer throughout: a decision that picks where a building goes has no
/// float in it.
fn sector_of(origin: Option<(u16, u16)>, cell: (u16, u16)) -> usize {
    let Some(origin) = origin else {
        return 0;
    };
    let (dx, dy) = (
        i32::from(cell.0) - i32::from(origin.0),
        i32::from(cell.1) - i32::from(origin.1),
    );
    let (ax, ay) = (dx.abs(), dy.abs());
    match (dx >= 0, dy >= 0, ax >= 2 * ay, ay >= 2 * ax) {
        (true, _, true, _) => 0,  // east
        (false, _, true, _) => 4, // west
        (_, false, _, true) => 2, // north
        (_, true, _, true) => 6,  // south
        (true, false, ..) => 1,   // north-east
        (false, false, ..) => 3,  // north-west
        (false, true, ..) => 5,   // south-west
        (true, true, ..) => 7,    // south-east
    }
}

/// How many of the house's buildings already stand in each sector around
/// `origin`: the crowding the sweep spreads away from.
fn sector_counts(origin: Option<(u16, u16)>, anchors: &[(u16, u16)]) -> [i32; SECTORS] {
    let mut counts = [0_i32; SECTORS];
    for cell in anchors {
        counts[sector_of(origin, *cell)] += 1;
    }
    counts
}

/// One house's placement answers, for one frame.
pub struct Placement<'a, 'b> {
    obs: &'a Observation<'b>,
    rules: &'a RuleSet,
    /// Cells the caller asked to keep free of foundations: the ring a wall
    /// stands on. Not refused outright — a base with nowhere else still has to
    /// build — but ranked behind every cell that leaves the ring alone.
    reserved: BTreeSet<(u16, u16)>,
    /// Optional local base to which an outpost building is committed.
    preferred: Option<(u16, u16)>,
    /// Armed defense sites face a *legitimately observed* threat, instead of
    /// landing on the empty rear sector selected by the generic base sweep.
    /// No such bias is applied to fragile infrastructure or normal factories.
    front: Option<((u16, u16), (u16, u16))>,
    /// *Observed* ore patches for an Enslaves= resource building. Never read
    /// the hidden overlay grid here: the owning brain supplies its ore memory.
    ore_patches: Vec<((u16, u16), u32)>,
    ore_radius: i32,
}

/// A deterministic, scale-free ordering within a legal foundation window.
/// Positive projection is the enemy-facing half of the base, negative the
/// rear. Ranking never overrides the native placement/traffic legality gates.
fn front_projection(
    home: (u16, u16), enemy: (u16, u16), candidate: (u16, u16),
) -> i64 {
    let dx = i64::from(enemy.0) - i64::from(home.0);
    let dy = i64::from(enemy.1) - i64::from(home.1);
    (i64::from(candidate.0) - i64::from(home.0)) * dx
        + (i64::from(candidate.1) - i64::from(home.1)) * dy
}

/// Heuristic coverage, not a claim of real-time ore behind fog. The native
/// slave gatherer owns precise target selection. Reject distant patches; a
/// refinery in a beautiful but empty corner is worse than a crowded one next
/// to a modest known seam.
fn known_ore_coverage(
    at: (u16, u16), patches: &[((u16, u16), u32)], radius: i32,
) -> i64 {
    patches.iter().filter_map(|(cell, stock)| {
        let dist = cell_distance(at, *cell);
        (dist <= radius).then_some(
            i64::from((*stock).min(256)) * i64::from(radius + 1 - dist))
    }).sum()
}

/// How many cells a new foundation must leave between itself and any of the
/// house's other buildings.
///
/// Zero — sharing an edge — is what `Adjacent=` alone permits, and it is what
/// makes a barracks hug the yard: two foundations nose to nose are a wall of
/// concrete with no lane past it, no cell to repair from and no cell to wall.
/// One cell is the smallest gap a unit can walk through and a wall can stand
/// in, and it is a legality rule of the sweep rather than a preference: a cell
/// that leaves no gap is not a site.
const BUILDING_GAP_CELLS: i32 = 2;

/// One cell the sweep admits, with the keys it is ranked by.
struct Candidate {
    /// Its foundation would stand within [`BUILDING_GAP_CELLS`] of another of
    /// the house's buildings. Not a ranking key: the sweep drops those cells.
    too_close: bool,
    /// Its foundation would cover a cell the caller reserved. Last key first:
    /// a barracks in the ring is a wall that can never be closed there.
    covers_reserved: bool,
    /// How many of the house's buildings already stand in its sector.
    sector: i32,
    /// Walkable cells within [`OPENNESS_RADIUS`] that no foundation covers.
    open: i32,
    /// Rings from the anchor it was scanned around.
    ring: i32,
    /// How much historically observed ore the deployed slave workers can reach.
    ore_coverage: i64,
    cell: (u16, u16),
}

impl<'a, 'b> Placement<'a, 'b> {
    pub fn new(obs: &'a Observation<'b>, rules: &'a RuleSet) -> Self {
        Self {
            obs,
            rules,
            reserved: BTreeSet::new(),
            preferred: None,
            front: None,
            ore_patches: Vec::new(),
            ore_radius: 0,
        }
    }

    /// Restrict site search to a committed outpost. The primary base's native
    /// answer must not steal a refinery paid for by the distant ore base.
    /// Return None rather than silently constructing it at home when the
    /// remote foundation is temporarily blocked.
    pub fn preferring(mut self, anchor: (u16, u16)) -> Self {
        self.preferred = Some(anchor);
        self
    }

    /// Bias an armed defense toward the observed attack corridor. Buildings
    /// without weapons must continue using the ordinary secure base layout.
    pub fn defending_front(mut self, home: (u16, u16), enemy: (u16, u16)) -> Self {
        if home != enemy {
            self.front = Some((home, enemy));
        }
        self
    }

    /// Favour observed mineral coverage when choosing a deployed Enslaves=
    /// refinery's foundation. This is a preference inside native build area,
    /// never a permission to build on an otherwise illegal site. The caller
    /// supplies only legitimately observed ore; a hidden new deposit remains
    /// unknown until a scout sees it.
    pub fn serving_known_ore(
        mut self,
        patches: impl IntoIterator<Item = ((u16, u16), u32)>,
        radius: i32,
    ) -> Self {
        self.ore_patches = patches.into_iter().collect();
        self.ore_radius = radius.clamp(1, 48);
        self
    }

    /// Keep `cells` free of foundations when anything else fits.
    ///
    /// The wall plan closes a ring around every `ProtectWithWall=yes` building,
    /// and a foundation that covers a cell of that ring is a permanent gap in
    /// it: the wall is an overlay and cannot build where a building stands, and
    /// the building cannot be moved. Reserving the ring is cheaper than a wall
    /// with a hole in it.
    pub fn reserving(mut self, cells: BTreeSet<(u16, u16)>) -> Self {
        self.reserved = cells;
        self
    }

    /// The cells covered by the house's own building foundations, grown by
    /// [`BUILDING_GAP_CELLS`]: what a new foundation may not touch.
    /// The lane a foundation must leave between itself and the next building, in
    /// cells, for the smaller of the two: the wider footprint gets the tighter lane.
    fn gap_between(a: (u16, u16), b: (u16, u16)) -> i32 {
        gap_for(a).min(gap_for(b))
    }

    /// The cells a building's footprint grows by: what a new foundation may not
    /// touch.
    fn grown(cell: (u16, u16), foundation: (u16, u16), gap: i32, out: &mut BTreeSet<(u16, u16)>) {
        let (w, h) = (
            i32::from(foundation.0.max(1)),
            i32::from(foundation.1.max(1)),
        );
        for dy in -gap..h + gap {
            for dx in -gap..w + gap {
                let (Some(x), Some(y)) = (
                    u16::try_from(i32::from(cell.0) + dx).ok(),
                    u16::try_from(i32::from(cell.1) + dy).ok(),
                ) else {
                    continue;
                };
                out.insert((x, y));
            }
        }
    }

    /// The house's own building footprints grown by the lane each one demands,
    /// as cell -> the foundation that owns it.
    ///
    /// The lane is a property of the *pair*: a shed needs two cells of clearance
    /// against a shed, and a 4x3 needs one against either. A flat set grown by
    /// the widest lane cannot express that — it either over-blocks the big
    /// foundation or under-blocks the small one — so the owner's size is carried
    /// with the cell and `too_close` asks the pair.
    fn crowd(&self) -> BTreeMap<(u16, u16), ((u16, u16), (u16, u16))> {
        let mut cells: BTreeMap<(u16, u16), ((u16, u16), (u16, u16))> = BTreeMap::new();
        for entity in self.obs.own_entities() {
            let Some(ty) = self.obs.object_type(self.rules, entity.type_id) else {
                continue;
            };
            if ty.category != ObjectCategory::Building || ty.wall {
                continue;
            }
            let foundation = ty.foundation.dimensions();
            // Grown by the widest lane this building could ever demand, so the
            // map is a superset; the pair's own number is applied on lookup.
            let gap = gap_for((1, 1));
            for dy in -gap..i32::from(foundation.1.max(1)) + gap {
                for dx in -gap..i32::from(foundation.0.max(1)) + gap {
                    let (Some(x), Some(y)) = (
                        u16::try_from(i32::from(entity.cell.0) + dx).ok(),
                        u16::try_from(i32::from(entity.cell.1) + dy).ok(),
                    ) else {
                        continue;
                    };
                    cells.insert((x, y), (entity.cell, foundation));
                }
            }
        }
        cells
    }

    /// Whether a foundation at `cell` would stand inside the grown footprint of
    /// another building: no lane between the two.
    fn too_close(
        &self,
        cell: (u16, u16),
        foundation: (u16, u16),
        crowd: &BTreeMap<(u16, u16), ((u16, u16), (u16, u16))>,
    ) -> bool {
        if crowd.is_empty() {
            return false;
        }
        for dy in 0..i32::from(foundation.1.max(1)) {
            for dx in 0..i32::from(foundation.0.max(1)) {
                let (Some(x), Some(y)) = (
                    u16::try_from(i32::from(cell.0) + dx).ok(),
                    u16::try_from(i32::from(cell.1) + dy).ok(),
                ) else {
                    continue;
                };
                if let Some((owner_cell, owner_foundation)) = crowd.get(&(x, y)) {
                    // How far this cell is from that building's own rectangle,
                    // and the lane this particular pair needs. A cell merely
                    // being in the grown map is not enough: the map is grown by
                    // the widest lane any neighbour could want.
                    let gap = Self::gap_between(foundation, *owner_foundation);
                    if rect_distance((x, y), *owner_cell, *owner_foundation) <= gap {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Whether a foundation at `cell` would cover a reserved cell.
    fn covers_reserved(&self, cell: (u16, u16), foundation: (u16, u16)) -> bool {
        if self.reserved.is_empty() {
            return false;
        }
        for dy in 0..i32::from(foundation.1.max(1)) {
            for dx in 0..i32::from(foundation.0.max(1)) {
                let (Some(x), Some(y)) = (
                    u16::try_from(i32::from(cell.0) + dx).ok(),
                    u16::try_from(i32::from(cell.1) + dy).ok(),
                ) else {
                    continue;
                };
                if self.reserved.contains(&(x, y)) {
                    return true;
                }
            }
        }
        false
    }

    /// The `attempt`-th *different* site that meets the native gate.
    ///
    /// The native search is allowed only on attempt zero: it frequently
    /// returns the same cell until a completed product leaves the factory.
    /// Previously every retry chose it again, then abandoned the build after
    /// 64 supposedly different attempts. Subsequent attempts enumerate the
    /// sweep with the native candidate excluded, so no site repeats.
    pub fn cell_for(&self, type_id: InternedId, attempt: u32) -> Option<(u16, u16)> {
        self.cell_for_where(type_id, attempt, |_| true)
    }

    /// As `cell_for`, but with a caller's policy constraint (e.g. keeping a
    /// fragile structure behind the threat line) applied *during selection*.
    /// Rejecting an unsafe candidate only after the sweep selected it used to
    /// spend a whole retry on the same unsafe native cell.
    pub fn cell_for_where(
        &self,
        type_id: InternedId,
        attempt: u32,
        allowed: impl Fn((u16, u16)) -> bool,
    ) -> Option<(u16, u16)> {
        let ty = self.obs.object_type(self.rules, type_id)?;
        let foundation = ty.foundation.dimensions();
        let crowd = self.crowd();
        // The computer's native base-site suggestion does not consider the
        // player's observed gun line. For a directional defense, include all
        // admissible positions in the same sweep instead of pinning the first
        // attempt to a possibly rear-facing native cell.
        let native = self.native_site(ty).filter(|&cell| {
            self.front.is_none() && self.ore_patches.is_empty()
                &&
            allowed(cell)
                && self.preferred.is_none_or(|anchor| cell_distance(cell, anchor) <= PLACEMENT_SCAN_RADIUS)
                && self.obs.placement_admits(self.rules, ty, cell.0, cell.1)
                && (ty.wall || !self.too_close(cell, foundation, &crowd))
        });
        if attempt == 0 {
            if let Some(cell) = native {
                log::info!("auto play: site for {} from native search at {cell:?}", ty.id);
                return Some(cell);
            }
        }
        let index = if native.is_some() {
            attempt.checked_sub(1)?
        } else {
            attempt
        };
        let site = self.sweep_where(ty, index, native, &allowed);
        if trace_enabled("AUTOPLAY_TRACE_SITE") {
            let near = site.and_then(|cell| {
                self.obs.ore_cells(self.rules)
                    .iter().map(|ore| cell_distance(cell, *ore)).min()
            });
            log::info!("site trace: type={} cell={site:?} ore={near:?}", ty.id);
        }
        log::info!(
            "auto play: site for {} from sweep at {site:?} (attempt {attempt})",
            ty.id
        );
        site
    }

    /// An eligible structure could be placed if all own mobile units currently
    /// standing on the candidate foundation left it. Called only after the
    /// ordinary search has no available site. The combined lifted-unit probe
    /// proves that clearing exactly these blockers is sufficient; it does not
    /// scatter unrelated units or assume that moving one blocker is enough.
    pub fn site_blocked_by_own_units(
        &self,
        type_id: InternedId,
        allowed: impl Fn((u16, u16)) -> bool,
    ) -> Option<((u16, u16), Vec<u64>)> {
        let ty = self.obs.object_type(self.rules, type_id)?;
        if ty.wall {
            return None;
        }
        let (width, height) = ty.foundation.dimensions();
        let crowd = self.crowd();
        let units: Vec<_> = self.obs.own_entities()
            .filter(|unit| unit.orderable && !self.obs.owned_by_hand(unit.entity_id))
            .filter(|unit| self.obs.object_type(self.rules, unit.type_id).is_some_and(|kind| {
                matches!(kind.category, ObjectCategory::Infantry | ObjectCategory::Vehicle)
            }))
            .collect();
        for anchor in self.anchors() {
            for ring in 1..=PLACEMENT_SCAN_RADIUS {
                for dy in -ring..=ring {
                    for dx in -ring..=ring {
                        if dx.abs().max(dy.abs()) != ring { continue; }
                        let (Some(rx), Some(ry)) = (
                            u16::try_from(i32::from(anchor.0) + dx).ok(),
                            u16::try_from(i32::from(anchor.1) + dy).ok(),
                        ) else { continue; };
                        if !allowed((rx, ry))
                            || self.preferred.is_some_and(|anchor| cell_distance((rx, ry), anchor) > PLACEMENT_SCAN_RADIUS)
                            || self.too_close((rx, ry), (width, height), &crowd)
                        {
                            continue;
                        }
                        let occupying: Vec<_> = units.iter().filter(|unit| {
                            i32::from(unit.cell.0) >= i32::from(rx)
                                && i32::from(unit.cell.0) < i32::from(rx) + i32::from(width)
                                && i32::from(unit.cell.1) >= i32::from(ry)
                                && i32::from(unit.cell.1) < i32::from(ry) + i32::from(height)
                        }).collect();
                        if occupying.is_empty() { continue; }
                        let blockers: Vec<u64> = occupying.iter()
                            .map(|unit| unit.entity_id)
                            .collect();
                        if self.obs.placement_admits_after_units_leave(
                            self.rules, ty, rx, ry, &blockers,
                        ) {
                            return Some(((rx, ry), blockers));
                        }
                    }
                }
            }
        }
        None
    }

    /// `HouseClass::FindBaseBuildingSite` for the ordinary branch. `None` when
    /// it found nothing (it answers `(0, 0)`) or the cell does not fit `u16`.
    fn native_site(&self, ty: &ObjectType) -> Option<(u16, u16)> {
        let (x, y) = find_base_building_site(
            self.obs.simulation(),
            self.rules,
            self.obs.house(),
            ty,
            SiteKey::Ordinary,
        );
        if x <= 0 || y <= 0 {
            return None;
        }
        Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?))
    }

    /// The `attempt`-th best cell, from a sweep outwards from the house's
    /// buildings: the Construction Yard first, then everything else in entity
    /// order.
    ///
    /// "Best" is the roomiest cell in the emptiest sector of the nearest rings
    /// that is not a gap: the sweep keeps the first rings that admit anything at
    /// all plus [`PLACEMENT_WINDOW_RINGS`], drops the cells that have less than
    /// three fifths of the roomiest one's open ground, and ranks what is left by
    /// sector, then openness, then ring, then distance to the yard.
    ///
    /// Building in ring order alone wedges new foundations into the one-cell
    /// gaps between existing ones — the densest possible base, and the one that
    /// walls its own yard in; ranking by openness alone walks the base out onto
    /// the open ground and leaves the refinery sixteen cells from the yard,
    /// where nothing can defend it. Both the floor and the order are policy
    /// choices, like the sweep itself: a human places by eye, and an eye keeps a
    /// gap without crossing the map for it.
    ///
    /// A building retail marks `ProtectWithWall=yes` is then kept on the side
    /// no attack comes from ([`Self::keep_safe_side`]).
    ///
    /// Deterministic: the anchor order, the ring order and the sort keys are
    /// all fixed, and no collection with a random iteration order is involved.
    fn sweep(&self, ty: &ObjectType, attempt: u32) -> Option<(u16, u16)> {
        self.sweep_where(ty, attempt, None, &|_| true)
    }

    fn sweep_where(
        &self,
        ty: &ObjectType,
        attempt: u32,
        skip: Option<(u16, u16)>,
        allowed: &impl Fn((u16, u16)) -> bool,
    ) -> Option<(u16, u16)> {
        let blocked = self.occupied_cells();
        let mut anchors = self.anchors();
        if let Some(anchor) = self.preferred {
            anchors.retain(|cell| *cell != anchor);
            anchors.insert(0, anchor);
        }
        let foundation = ty.foundation.dimensions();
        let crowd = self.crowd();
        let mut seen: BTreeSet<(u16, u16)> = BTreeSet::new();
        let mut admitted: Vec<Candidate> = Vec::new();
        // How much of the base already stands in each sector around the first
        // anchor: the yard, which is what everything else is built out from.
        let crowded = sector_counts(anchors.first().copied(), &anchors);
        for anchor in &anchors {
            for ring in 1..=PLACEMENT_SCAN_RADIUS {
                for dy in -ring..=ring {
                    for dx in -ring..=ring {
                        if dx.abs().max(dy.abs()) != ring {
                            continue;
                        }
                        let (Some(rx), Some(ry)) = (
                            u16::try_from(i32::from(anchor.0) + dx).ok(),
                            u16::try_from(i32::from(anchor.1) + dy).ok(),
                        ) else {
                            continue;
                        };
                        if !seen.insert((rx, ry))
                            || skip == Some((rx, ry))
                            || !allowed((rx, ry))
                            || self.preferred.is_some_and(|anchor| cell_distance((rx, ry), anchor) > PLACEMENT_SCAN_RADIUS)
                            || !self.obs.placement_admits(self.rules, ty, rx, ry)
                        {
                            continue;
                        }
                        let sector = sector_of(anchors.first().copied(), (rx, ry));
                        admitted.push(Candidate {
                            too_close: !ty.wall && self.too_close((rx, ry), foundation, &crowd),
                            covers_reserved: self.covers_reserved((rx, ry), foundation),
                            sector: crowded[sector],
                            open: self.openness(rx, ry, &blocked),
                            ring,
                            ore_coverage: known_ore_coverage((rx, ry),
                                &self.ore_patches, self.ore_radius),
                            cell: (rx, ry),
                        });
                    }
                }
            }
        }
        // Reject cramped foundations *before* defining the near-base window.
        // Previously the closest few rings might consist solely of sites too
        // close to the yard. Taking a window around those bad candidates then
        // dropped every actually legal site farther out and the AI reported
        // "no cell" (or packed a new building into the only remaining lane).
        // If no legal site exists in the whole scan, keep the old emergency
        // fallback: a tight but native-valid position beats a permanently
        // blocked finished production line.
        let legal = admitted.iter().filter(|site| !site.too_close).count();
        if legal > 0 {
            admitted.retain(|site| !site.too_close);
        } else if !admitted.is_empty() {
            log::warn!(
                "placement: no site keeps {} cells from another building for {}; \
                 {} native-valid tight candidates, trying the closest",
                BUILDING_GAP_CELLS, ty.id, admitted.len()
            );
        }
        let Some(nearest) = admitted.iter().map(|site| site.ring).min() else {
            return None;
        };
        // A slave-work refinery is not a generic factory: a small extra walk
        // away from the yard can be the difference between five productive
        // workers and five idle ones. Allow the whole legal base scan when
        // known ore provides a real placement preference; still honour lane,
        // power, adjacency and the native foundation admission tests.
        let window = if self.ore_patches.is_empty() {
            nearest + PLACEMENT_WINDOW_RINGS
        } else {
            PLACEMENT_SCAN_RADIUS
        };
        admitted.retain(|site| site.ring <= window);
        if ty.protect_with_wall {
            self.keep_safe_side(&mut admitted, &anchors);
        }
        // Of the cells the window left, only the roomy ones are built on. A
        // foundation that *fits* is not a foundation that works: ranked by ring
        // alone every building lands in the last one-cell gap between the
        // others, and the base ends up a solid block with no lane through it —
        // a yard walled in by its own refineries, and a wall that cannot be
        // closed because the perimeter is the buildings.
        let roomiest = admitted
            .iter()
            .map(|candidate| candidate.open)
            .max()
            .unwrap_or(0);
        if roomiest > 0 {
            let floor = roomiest * OPENNESS_FLOOR_NUMER / OPENNESS_FLOOR_DENOM;
            admitted.retain(|candidate| candidate.open >= floor);
        }
        // A cell whose foundation would cover the wall's ring is taken only
        // when the base has nowhere else: the ring is where a wall can stand,
        // and a barracks dropped on it is a gap no later decision can close.
        //
        // The emptiest sector around the yard next, then the roomiest cell in
        // it, then the nearer ring, and the yard's own distance last.
        //
        // The sector is what stops the base stacking on the side it started on,
        // and the openness is what keeps a lane between what it has already
        // built; the ring and the distance are what keep it a base at all, so a
        // roomier cell three rings out cannot outrank a tight one beside the
        // yard for long. Distance used to be the first key, which is how the
        // sweep filled every cell touching the yard before it used the ring
        // outside it.
        let yard = anchors.first().copied();
        let distance =
            |candidate: &Candidate| yard.map_or(0, |yard| cell_distance(candidate.cell, yard));
        admitted.sort_by(|a, b| {
            a.covers_reserved
                .cmp(&b.covers_reserved)
                .then_with(|| self.front.map_or(std::cmp::Ordering::Equal, |(home, enemy)| {
                    front_projection(home, enemy, b.cell)
                        .cmp(&front_projection(home, enemy, a.cell))
                }))
                .then(b.ore_coverage.cmp(&a.ore_coverage))
                .then(a.sector.cmp(&b.sector))
                .then(b.open.cmp(&a.open))
                .then(a.ring.cmp(&b.ring))
                .then(distance(a).cmp(&distance(b)))
                .then(a.cell.1.cmp(&b.cell.1))
                .then(a.cell.0.cmp(&b.cell.0))
        });
        let index = usize::try_from(attempt).ok()?;
        admitted.get(index).map(|candidate| {
            // The one line a play test needs to replay a layout: what was
            // placed, where, and the keys it won on.
            log::debug!(
                "placement: {} attempt {attempt} -> {:?} ring={} reserved={} open={} sector={}",
                ty.id,
                candidate.cell,
                candidate.ring,
                candidate.covers_reserved,
                candidate.open,
                candidate.sector
            );
            candidate.cell
        })
    }

    /// Keep the cells on the side no attack comes from, when there are any.
    ///
    /// `ProtectWithWall=yes` is retail's own list of what a house cannot afford
    /// to lose — the Construction Yard, the tech lab, the superweapons — the
    /// same list [`crate::sim::strategy::assist::walls`] closes a wall around.
    /// Putting one of those on the enemy's side of the base is putting the tree
    /// it unlocks in front of the first tank that gets through; the sweep's
    /// openness ranking alone does not care which way the base faces.
    ///
    /// The direction is the one measured from the base's own buildings to the
    /// nearest hostile object, so "the back" follows the same enemy the walls
    /// face. A house that has seen no enemy has no front, and keeps every cell.
    fn keep_safe_side(&self, admitted: &mut Vec<Candidate>, anchors: &[(u16, u16)]) {
        let Some((centre, direction)) = self.enemy_side(anchors) else {
            return;
        };
        let safe: Vec<Candidate> = admitted
            .iter()
            .filter(|candidate| {
                let dx = i32::from(candidate.cell.0) - i32::from(centre.0);
                let dy = i32::from(candidate.cell.1) - i32::from(centre.1);
                dx * direction.0 + dy * direction.1 <= 0
            })
            .map(|candidate| Candidate {
                too_close: candidate.too_close,
                covers_reserved: candidate.covers_reserved,
                sector: candidate.sector,
                open: candidate.open,
                ring: candidate.ring,
                ore_coverage: candidate.ore_coverage,
                cell: candidate.cell,
            })
            .collect();
        if !safe.is_empty() {
            *admitted = safe;
        }
    }

    /// The centre of the house's own buildings and the direction, in cells, its
    /// nearest hostile object lies in. `None` with nothing to anchor on and
    /// nothing coming.
    fn enemy_side(&self, anchors: &[(u16, u16)]) -> Option<((u16, u16), (i32, i32))> {
        let (mut sx, mut sy, mut count) = (0_i64, 0_i64, 0_i64);
        for cell in anchors {
            sx += i64::from(cell.0);
            sy += i64::from(cell.1);
            count += 1;
        }
        if count == 0 {
            return None;
        }
        let centre = (
            u16::try_from(sx / count).ok()?,
            u16::try_from(sy / count).ok()?,
        );
        let enemy = self
            .obs
            .visible_enemy_entities()
            .map(|entity| entity.cell)
            .min_by_key(|cell| cell_distance(*cell, centre))?;
        Some((
            centre,
            (
                i32::from(enemy.0) - i32::from(centre.0),
                i32::from(enemy.1) - i32::from(centre.1),
            ),
        ))
    }

    /// How much open ground surrounds a cell: walkable cells within
    /// [`OPENNESS_RADIUS`] that no building's foundation covers.
    ///
    /// A cell in a corner between two structures scores low and is only taken
    /// when nothing roomier is admissible, which is what keeps lanes between
    /// buildings instead of filling every gap the moment it appears.
    fn openness(&self, x: u16, y: u16, blocked: &BTreeSet<(u16, u16)>) -> i32 {
        let mut open = 0;
        for dy in -OPENNESS_RADIUS..=OPENNESS_RADIUS {
            for dx in -OPENNESS_RADIUS..=OPENNESS_RADIUS {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (Some(cx), Some(cy)) = (
                    u16::try_from(i32::from(x) + dx).ok(),
                    u16::try_from(i32::from(y) + dy).ok(),
                ) else {
                    continue;
                };
                if blocked.contains(&(cx, cy)) {
                    continue;
                }
                if self.obs.is_ground_walkable(cx, cy) {
                    open += 1;
                }
            }
        }
        open
    }

    /// Every cell any building's foundation stands on, own and hostile.
    ///
    /// The path grid already refuses a foundation, but not the cells *beside*
    /// one, and a lane is made of the cells beside one.
    fn occupied_cells(&self) -> BTreeSet<(u16, u16)> {
        let mut cells = BTreeSet::new();
        for (cell, type_id) in self
            .obs
            .own_entities()
            .map(|entity| (entity.cell, entity.type_id))
            .chain(
                self.obs
                    .visible_enemy_entities()
                    .map(|entity| (entity.cell, entity.type_id)),
            )
        {
            let Some(ty) = self.obs.object_type(self.rules, type_id) else {
                continue;
            };
            if ty.category != ObjectCategory::Building {
                continue;
            }
            let (width, height) = ty.foundation.dimensions();
            for dy in 0..i32::from(height.max(1)) {
                for dx in 0..i32::from(width.max(1)) {
                    let (Some(x), Some(y)) = (
                        u16::try_from(i32::from(cell.0) + dx).ok(),
                        u16::try_from(i32::from(cell.1) + dy).ok(),
                    ) else {
                        continue;
                    };
                    cells.insert((x, y));
                }
            }
        }
        cells
    }

    /// The sweep's anchors: every Construction Yard maker's cell, then the
    /// house's other structures, in entity order.
    fn anchors(&self) -> Vec<(u16, u16)> {
        let mut anchors: Vec<(u16, u16)> = Vec::new();
        let mut rest: Vec<(u16, u16)> = Vec::new();
        for entity in self.obs.own_entities() {
            let Some(ty) = self.obs.object_type(self.rules, entity.type_id) else {
                continue;
            };
            if ty.category != crate::rules::object_type::ObjectCategory::Building {
                continue;
            }
            if ty.build_const_eligible() {
                anchors.push(entity.cell);
            } else if !(ty.capturable || ty.can_be_occupied) {
                // Ground the yard grows on is ground the house built. A
                // captured derrick or the map's civilian house is
                // `Capturable=`/`CanBeOccupied=`, and treating one as an
                // anchor put new structures out at the oil well while the
                // yard stood empty. A captured enemy *building* is not
                // either, and stays an anchor: that one really is a base.
                rest.push(entity.cell);
            }
        }
        for cell in rest {
            if !anchors.contains(&cell) {
                anchors.push(cell);
            }
        }
        anchors
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;

    #[test]
    fn defenses_prefer_the_enemy_side_not_the_empty_rear() {
        let home = (50, 50);
        let enemy = (80, 50);
        assert!(front_projection(home, enemy, (56, 52))
            > front_projection(home, enemy, (44, 52)));
        assert!(front_projection(home, (20, 50), (44, 52))
            > front_projection(home, (20, 50), (56, 52)));
    }

    /// Whether a foundation at `cell` covers any of `cells`.
    fn covers(cells: &BTreeSet<(u16, u16)>, cell: (u16, u16), foundation: (u16, u16)) -> bool {
        (0..foundation.1)
            .any(|dy| (0..foundation.0).any(|dx| cells.contains(&(cell.0 + dx, cell.1 + dy))))
    }

    /// A war factory is five by three, so `placement_admits` — which asks about
    /// the one cell a click names — could put it two cells out and still have
    /// its footprint touch the yard's. Sharing an edge is what "the barracks
    /// hugs the yard" is: no lane between the two, and no cell for the wall
    /// that should stand there. The sweep refuses a cell that leaves no lane.
    #[test]
    fn a_foundation_does_not_take_the_wall_ring() {
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[BuildingTypes]\n0=GACNST\n1=GAWEAP\n\
             [GACNST]\nStrength=1500\nFoundation=4x4\nBaseNormal=yes\n\
             Adjacent=4\nProtectWithWall=yes\n\
             [GAWEAP]\nStrength=1000\nFoundation=5x3\nBaseNormal=yes\nAdjacent=4\n",
        );
        let art = crate::rules::ini_parser::IniFile::from_str(
            "[GACNST]\nFoundation=4x4\n[GAWEAP]\nFoundation=5x3\n",
        );
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("base rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 14, 14, 255, &rules)
            .expect("a yard");
        let obs = Observation::new(&sim, owner);
        let yard = rules.object("GACNST").expect("the yard");
        let (width, height) = yard.foundation.dimensions();
        // The ring the wall stands on: one cell out from the yard's foundation.
        let mut ring: BTreeSet<(u16, u16)> = BTreeSet::new();
        for y in 13..=(14 + height) {
            for x in 13..=(14 + width) {
                let inside = x >= 14 && x < 14 + width && y >= 14 && y < 14 + height;
                if !inside {
                    ring.insert((x, y));
                }
            }
        }

        let factory = rules.object("GAWEAP").expect("a war factory");
        let (fw, fh) = factory.foundation.dimensions();
        // The ring is exactly the yard's foundation grown by one cell, so the
        // lane rule alone already keeps a foundation off it. Asked of
        // `cell_for`, the entry point `build` uses, so the native site answer
        // is held to the same rule as the sweep.
        let factory_id = obs.type_id("GAWEAP").expect("the factory's id");
        let plain = Placement::new(&obs, &rules)
            .cell_for(factory_id, 0)
            .expect("a cell for the factory");
        assert!(
            !covers(&ring, plain, (fw, fh)),
            "the factory keeps a lane from the yard: {plain:?} {fw}x{fh}"
        );

        let keeping = Placement::new(&obs, &rules).reserving(ring.clone());
        let kept = keeping.sweep(factory, 0).expect("a cell for the factory");
        assert!(
            !covers(&ring, kept, (fw, fh)),
            "the ring is kept for the wall: {kept:?}"
        );
    }

    /// The native builder can answer the same site on every probe until the
    /// building has actually been placed. A second attempt must therefore
    /// address a different legal foundation, never repeat the first cell.
    #[test]
    fn a_rejected_site_advances_to_a_distinct_candidate() {
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[BuildingTypes]\n0=GACNST\n1=GAPOWR\n\
             [GACNST]\nStrength=1500\nFoundation=3x3\nBaseNormal=yes\nAdjacent=4\n\
             [GAPOWR]\nStrength=500\nFoundation=2x2\nBaseNormal=yes\nAdjacent=4\n",
        );
        let art = crate::rules::ini_parser::IniFile::from_str(
            "[GACNST]\nFoundation=3x3\n[GAPOWR]\nFoundation=2x2\n",
        );
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
            .expect("placement fixture");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 14, 14, 255, &rules)
            .expect("a construction yard");
        let obs = Observation::new(&sim, owner);
        let ty = obs.type_id("GAPOWR").expect("the power plant type");
        let placement = Placement::new(&obs, &rules);
        let first = placement.cell_for(ty, 0).expect("the first site");
        let second = placement.cell_for(ty, 1).expect("a second site");
        assert_ne!(first, second, "each attempt must really try another cell");
        assert!(obs.placement_admits(&rules, rules.object("GAPOWR").unwrap(), second.0, second.1));
        let filtered = placement.cell_for_where(ty, 0, |cell| cell != first)
            .expect("a policy-filtered alternative");
        assert_ne!(filtered, first, "a policy cannot reselect an ineligible native site");
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod v28_known_resource_placement_tests {
    use super::known_ore_coverage;

    #[test]
    fn slave_refinery_favours_legitimately_known_minerals_within_scan() {
        let patches = [((40, 40), 85), ((10, 10), 4)];
        assert!(known_ore_coverage((38, 40), &patches, 14)
            > known_ore_coverage((12, 10), &patches, 14));
        assert_eq!(known_ore_coverage((80, 80), &patches, 14), 0);
    }
}
