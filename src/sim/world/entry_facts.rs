//! VERA-only: the facts a path batch's Can_Enter_Cell asks share, copied once
//! per batch. A batch's searches ([`crate::sim::movement::path_batch`]) all
//! read one unchanged world, and in a jam each real cell is asked about ~36
//! times a batch and each blocker read ~42 times, every time from scattered
//! terrain, occupancy and entity records. The first ask copies what Infantry
//! `0x0051BF90` / Unit `0x0073F0A0` read of the cell or blocker into a dense
//! table and later asks read the copy. The world stays the source of truth:
//! the tables are emptied before each batch, never saved or hashed, and debug
//! builds compare every reuse with a fresh read. Asks outside a batch read
//! the world directly. A batch asks about a few thousand of the map's cells,
//! so the tables mark what they keep and the reset empties only those.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::map::cell_index::NativeCellIdentity as Cell;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::terrain_rules::SpeedCostProfile;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::movement::bump_crush::CrushTarget;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::RawCellKey;
use crate::sim::overlay_grid::OverlayCell;
use crate::sim::world::Simulation;

/// One batch's [`CellFacts`] and [`PlainCell`] by real cell index and
/// [`BlockerFacts`] by entity slot, each kept from its first ask.
#[derive(Default)]
pub(crate) struct EntryFacts {
    cells: Vec<OnceLock<CellFacts>>,
    /// Each real cell's [`PlainCell`] bits, 0 until asked.
    plain: Vec<AtomicU32>,
    blockers: Vec<OnceLock<BlockerFacts>>,
    /// The real cells whose `cells` or `plain` entry this batch filled.
    cells_kept: KeptBits,
    /// The slots whose `blockers` entry this batch filled.
    blockers_kept: KeptBits,
}

/// One bit per table entry, set when an ask fills it, so a reset empties only
/// the filled entries. Searches on several threads may set one bit at once.
#[derive(Default)]
struct KeptBits(Vec<AtomicU64>);

impl KeptBits {
    fn mark(&self, index: usize) {
        let (word, bit) = (&self.0[index / 64], 1u64 << (index % 64));
        if word.load(Ordering::Relaxed) & bit == 0 {
            word.fetch_or(bit, Ordering::Relaxed);
        }
    }

    /// `empty` every marked entry and clear the marks, then fit them to `len`
    /// entries.
    fn empty_marked(&mut self, len: usize, mut empty: impl FnMut(usize)) {
        for (word_index, word) in self.0.iter_mut().enumerate() {
            let mut bits = std::mem::take(word.get_mut());
            while bits != 0 {
                empty(word_index * 64 + bits.trailing_zeros() as usize);
                bits &= bits - 1;
            }
        }
        self.0.resize_with(len.div_ceil(64), AtomicU64::default);
    }
}

impl std::fmt::Debug for EntryFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryFacts")
            .field("cells", &self.cells.len())
            .field("blockers", &self.blockers.len())
            .finish()
    }
}

impl EntryFacts {
    /// Forget every kept fact and fit the tables to `cells` real cells and
    /// `slots` entity slots, before a batch.
    pub(crate) fn reset(&mut self, cells: usize, slots: usize) {
        let (kept_cells, plain) = (&mut self.cells, &mut self.plain);
        self.cells_kept.empty_marked(cells, |index| {
            kept_cells[index].take();
            *plain[index].get_mut() = 0;
        });
        self.cells.resize_with(cells, OnceLock::new);
        self.plain.resize_with(cells, AtomicU32::default);
        let blockers = &mut self.blockers;
        self.blockers_kept.empty_marked(slots, |slot| {
            blockers[slot].take();
        });
        self.blockers.resize_with(slots, OnceLock::new);
    }

    /// Real cell `index`'s facts, kept from this batch's first ask
    /// ([`Self::keep`]).
    pub(super) fn cell(&self, index: usize, read: impl Fn() -> CellFacts) -> &CellFacts {
        let kept = &self.cells[index];
        let kept = match kept.get() {
            Some(kept) => kept,
            None => {
                self.cells_kept.mark(index);
                Self::keep(kept, read())
            }
        };
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || *kept == read(),
            "real cell {index} changed during a path batch"
        );
        kept
    }

    /// Real cell `index`'s [`PlainCell`], kept from this batch's first ask.
    /// Searches asking at once may each read it and store the same bits:
    /// every read sees the one unchanged world.
    pub(super) fn plain(&self, index: usize, read: impl Fn() -> PlainCell) -> PlainCell {
        let kept = &self.plain[index];
        let cell = match kept.load(Ordering::Relaxed) {
            0 => {
                let cell = read();
                self.cells_kept.mark(index);
                kept.store(cell.0, Ordering::Relaxed);
                cell
            }
            bits => PlainCell(bits),
        };
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || cell == read(),
            "real cell {index} changed during a path batch"
        );
        cell
    }

    /// The facts of the entity in `slot`, kept from this batch's first ask
    /// ([`Self::keep`]).
    pub(super) fn blocker(&self, slot: usize, read: impl Fn() -> BlockerFacts) -> &BlockerFacts {
        let kept = &self.blockers[slot];
        let kept = match kept.get() {
            Some(kept) => kept,
            None => {
                self.blockers_kept.mark(slot);
                Self::keep(kept, read())
            }
        };
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || *kept == read(),
            "the entity in slot {slot} changed during a path batch"
        );
        kept
    }

    /// Keep the `read` facts in `kept`, unless another search kept its own
    /// first, and return the kept ones. Each search reads before the lock,
    /// which then holds only for the copy, so searches asking at once read
    /// side by side instead of waiting for one read. Every read sees the one
    /// unchanged world, so the copies are equal and either may stay.
    fn keep<T>(kept: &OnceLock<T>, read: T) -> &T {
        let _ = kept.set(read);
        kept.get().expect("kept just now")
    }
}

/// What Can_Enter_Cell reads of one real cell. Its object lists' members, its
/// tube, the first-Building lookups and MovementRestrictedTo's land, tile and
/// overlay reads stay live; a [`PlainCell`] keeps only whether it has a tube.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CellFacts {
    pub(super) coord: (i16, i16),
    pub(super) level: i8,
    pub(super) slope: u8,
    pub(super) flags: u32,
    /// The raw ground and deck occupation bytes, each with its infantry owner.
    pub(super) ground: (u8, Option<InternedId>),
    pub(super) deck: (u8, Option<InternedId>),
    pub(super) in_playfield: bool,
    pub(super) overlay: OverlayCell,
    pub(super) terrain_object: bool,
    /// Whether the ground and deck object lists yield anything, a terrain
    /// object included.
    pub(super) ground_listed: bool,
    pub(super) deck_listed: bool,
    pub(super) speed_costs: SpeedCostProfile,
}

impl CellFacts {
    /// Real cell `index` as the world holds it now. Reads no lookup, so it
    /// never stamps the shared Dummy.
    pub(super) fn of(sim: &Simulation, index: usize) -> Self {
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("live cell-entry terrain");
        let cell = Cell::Real(index);
        let c = &terrain.cells()[index];
        let key = RawCellKey::from_native(terrain, cell);
        let raw = &sim.substrate.raw_cell_occupation;
        let terrain_object = sim.terrain_objects.cell_index().get(&(c.rx, c.ry)).copied();
        let listed = |layer| {
            sim.substrate
                .occupancy
                .cell_objects(c.rx, c.ry, layer, terrain_object)
                .next()
                .is_some()
        };
        Self {
            coord: terrain.native_cell_coord(cell),
            level: c.level as i8,
            slope: c.slope_type,
            flags: terrain.native_cell_flags(cell),
            ground: raw.occupant_at(key, MovementLayer::Ground),
            deck: raw.occupant_at(key, MovementLayer::Bridge),
            in_playfield: crate::sim::cell_rect::retained_cell_is_in_playfield(
                cell,
                sim.playfield_bounds,
                &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
            ),
            overlay: sim.cell_overlay(cell),
            terrain_object: terrain_object.is_some(),
            ground_listed: listed(MovementLayer::Ground),
            deck_listed: listed(MovementLayer::Bridge),
            speed_costs: c.speed_costs,
        }
    }

    /// Whether `layer`'s object list yields anything. Other layers' lists are
    /// walked live.
    pub(super) fn listed(&self, layer: MovementLayer) -> bool {
        match layer {
            MovementLayer::Ground => self.ground_listed,
            MovementLayer::Bridge => self.deck_listed,
            MovementLayer::Air | MovementLayer::Underground => true,
        }
    }
}

/// What a step from a flat cell onto an empty one reads of each cell
/// ([`super::plain_entry`]), packed in one word: a search floods mostly open
/// ground, and these asks then read four bytes per cell instead of its
/// [`CellFacts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PlainCell(u32);

impl PlainCell {
    /// Set in every kept word, so 0 is "not asked yet".
    const KNOWN: u32 = 1 << 31;
    /// No structural bridge (`0x100`) and no tube: a step into or out of the
    /// cell reads nothing else of its bridge or tube.
    const FLAT: u32 = 1 << 30;
    /// Also no raw ground occupation, nothing in the ground object list (a
    /// terrain object included), and inside the playfield.
    const EMPTY: u32 = 1 << 29;
    /// An overlay, whose type's flags the asker reads.
    const OVERLAY: u32 = 1 << 28;
    /// The SpeedTypes this word answers for, in discriminant order.
    const SPEEDS: [SpeedType; 8] = [
        SpeedType::Foot,
        SpeedType::Track,
        SpeedType::Wheel,
        SpeedType::Hover,
        SpeedType::Winged,
        SpeedType::Float,
        SpeedType::Amphibious,
        SpeedType::FloatBeach,
    ];

    /// The bits of the cell with `facts`, holding a tube when `tube`.
    pub(super) fn new(facts: &CellFacts, tube: bool) -> Self {
        let flat = facts.flags & 0x100 == 0 && !tube;
        let empty = flat && facts.ground == (0, None) && !facts.ground_listed && facts.in_playfield;
        // Bits 8..16: the land row resolved for SpeedType n; 16..24: nonzero.
        let rows = Self::SPEEDS
            .iter()
            .enumerate()
            .map(|(n, &speed)| {
                facts
                    .speed_costs
                    .cost_for_speed_type(speed)
                    .map_or(0, |cost| {
                        (1 << (8 + n)) | (u32::from(cost != 0) << (16 + n))
                    })
            })
            .fold(0, |rows, row| rows | row);
        Self(
            Self::KNOWN
                | if flat { Self::FLAT } else { 0 }
                | if empty { Self::EMPTY } else { 0 }
                | if facts.overlay.overlay_id.is_some() {
                    Self::OVERLAY
                } else {
                    0
                }
                | rows
                | u32::from(facts.level as u8),
        )
    }

    pub(super) fn flat(self) -> bool {
        self.0 & Self::FLAT != 0
    }

    pub(super) fn empty(self) -> bool {
        self.0 & Self::EMPTY != 0
    }

    pub(super) fn overlay(self) -> bool {
        self.0 & Self::OVERLAY != 0
    }

    pub(super) fn level(self) -> i8 {
        self.0 as u8 as i8
    }

    /// The row the word keeps for SpeedType `speed`, for
    /// [`Self::row_nonzero`]; `None` for one it keeps none for.
    pub(super) fn speed_row(speed: SpeedType) -> Option<u32> {
        Self::SPEEDS
            .iter()
            .position(|&known| known == speed)
            .map(|n| n as u32)
    }

    /// Whether the land row `n` ([`Self::speed_row`]) is nonzero, when the
    /// cell has one ([`CellFacts::speed_costs`]).
    pub(super) fn row_nonzero(self, n: u32) -> Option<bool> {
        (self.0 >> (8 + n) & 1 != 0).then_some(self.0 >> (16 + n) & 1 != 0)
    }
}

/// What Can_Enter_Cell reads of one blocker in a cell's object list, at one
/// frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BlockerFacts {
    pub(super) owner: InternedId,
    pub(super) type_ref: InternedId,
    /// Its crush inputs, with its category and Iron Curtain.
    pub(super) crush: CrushTarget,
    /// Its world coordinate, its cell's x and its body facing.
    pub(super) position: DriveCoord,
    pub(super) rx: u16,
    pub(super) facing: u16,
    /// `motion_query::is_moving`, and whether it has a NavCom or turns.
    pub(super) moving: Option<bool>,
    pub(super) nav_com: bool,
    pub(super) rotating: bool,
    pub(super) warped_out: bool,
    pub(super) fully_cloaked: bool,
    pub(super) disguised: bool,
    pub(super) open_gate: bool,
    pub(super) bunker_occupied: bool,
    pub(super) foot_occupation: bool,
    pub(super) occupant_slot_a4: bool,
}

impl BlockerFacts {
    /// VERA-only: a per-batch read-only copy of each blocker's category,
    /// owner, type, cloak, crush target, motion, facing and position, with
    /// its warp, disguise, gate, bunker, occupation and track-slot bits, for
    /// classify_foot_entry (Infantry `0x0051BF90` / Unit `0x0073F0A0`); the
    /// entity records are the source of truth, rebuilt every batch. An ask
    /// outside a batch reads it fresh.
    pub(super) fn of(b: &GameEntity, frame: u32) -> Self {
        Self {
            owner: b.owner(),
            type_ref: b.type_ref(),
            crush: CrushTarget::from_entity(b, frame),
            position: crate::sim::movement::ground_pose::position_world_coord(&b.position),
            rx: b.position.rx(),
            facing: b.body_facing_current(frame),
            moving: crate::sim::movement::motion_query::is_moving(b),
            nav_com: b.navigation.nav_com().is_some(),
            rotating: b.body_facing.is_rotating(frame),
            warped_out: b.is_warped_out(),
            fully_cloaked: b.cloak.as_ref().is_some_and(|s| s.is_fully_cloaked()),
            disguised: b.disguise.as_ref().is_some_and(|s| s.is_disguised()),
            open_gate: b.is_open_gate(),
            bunker_occupied: b.bunker.occupant().is_some(),
            foot_occupation: b.foot_occupation_enabled(),
            occupant_slot_a4: crate::sim::movement::drive_track::occupant_slot_a4_answers_true(b),
        }
    }

    pub(super) fn category(&self) -> EntityCategory {
        self.crush.category
    }

    /// Iron Curtain or Force Shield at that frame.
    pub(super) fn invulnerable(&self) -> bool {
        self.crush.iron_curtained
    }
}

#[cfg(any(test, feature = "test-support"))]
impl EntryFacts {
    /// An empty table fitted to `sim`, as a batch starts.
    pub(crate) fn fitted(sim: &Simulation) -> Self {
        let mut facts = Self::default();
        facts.reset(
            sim.resolved_terrain
                .as_ref()
                .map_or(0, |terrain| terrain.cells().len()),
            sim.substrate.entities.slot_count(),
        );
        facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_facts(level: i8) -> CellFacts {
        CellFacts {
            coord: (1, 2),
            level,
            slope: 0,
            flags: 0,
            ground: (0, None),
            deck: (0, None),
            in_playfield: true,
            overlay: OverlayCell::default(),
            terrain_object: false,
            ground_listed: false,
            deck_listed: false,
            speed_costs: SpeedCostProfile::default(),
        }
    }

    fn blocker(id: u64, rx: u16) -> BlockerFacts {
        BlockerFacts::of(&GameEntity::test_default(id, "MTNK", "Americans", rx, 0), 0)
    }

    /// A reset empties what the last batch kept on both sides of a mark-word
    /// boundary, and a slot kept before the table shrinks comes back empty
    /// when it regrows.
    #[test]
    fn a_reset_forgets_every_fact_the_last_batch_kept() {
        let mut facts = EntryFacts::default();
        facts.reset(130, 70);
        for index in [0, 63, 64, 129] {
            facts.cell(index, || cell_facts(1));
            facts.plain(index, || PlainCell(PlainCell::KNOWN | 1));
        }
        facts.blocker(69, || blocker(1, 5));
        facts.reset(130, 10);
        facts.reset(130, 70);
        for index in [0, 63, 64, 129] {
            assert_eq!(facts.cell(index, || cell_facts(2)).level, 2);
            assert_eq!(
                facts.plain(index, || PlainCell(PlainCell::KNOWN | 2)),
                PlainCell(PlainCell::KNOWN | 2)
            );
        }
        assert_eq!(facts.blocker(69, || blocker(2, 9)).rx, 9);
    }

    /// Two searches asking about one cell, or one blocker, at once both
    /// read it, neither waiting for the other's read, and both get the facts
    /// the table keeps.
    #[test]
    fn searches_asking_at_once_read_side_by_side() {
        use std::sync::atomic::AtomicUsize;
        let mut facts = EntryFacts::default();
        facts.reset(1, 1);
        let reading = AtomicUsize::new(0);
        // Each search's first read waits until the other's has started.
        let side_by_side = || {
            reading.fetch_add(1, Ordering::SeqCst);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while reading.load(Ordering::SeqCst) % 2 != 0 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "a search waited for the other's read"
                );
                std::thread::yield_now();
            }
        };
        let at_once = |ask: &(dyn Fn(&dyn Fn()) + Sync)| {
            std::thread::scope(|scope| {
                for _ in 0..2 {
                    scope.spawn(|| ask(&side_by_side));
                }
            });
        };
        at_once(&|wait| {
            let first = std::cell::Cell::new(true);
            let kept = facts.cell(0, || {
                if first.replace(false) {
                    wait();
                }
                cell_facts(3)
            });
            assert_eq!(*kept, cell_facts(3));
        });
        at_once(&|wait| {
            let first = std::cell::Cell::new(true);
            let kept = facts.blocker(0, || {
                if first.replace(false) {
                    wait();
                }
                blocker(1, 5)
            });
            assert_eq!(*kept, blocker(1, 5));
        });
    }
}
