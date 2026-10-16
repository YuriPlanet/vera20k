//! VERA-only: the facts a path batch's Can_Enter_Cell asks share, copied once
//! per batch. A batch's searches ([`crate::sim::movement::path_batch`]) all
//! read one unchanged world, and in a jam each real cell is asked about ~36
//! times a batch and each blocker read ~42 times, every time from scattered
//! terrain, occupancy and entity records. The first ask copies what Infantry
//! `0x0051BF90` / Unit `0x0073F0A0` read of the cell or blocker into a dense
//! table and later asks read the copy. The world stays the source of truth:
//! the tables are emptied before each batch, never saved or hashed, and debug
//! builds compare every reuse with a fresh read. Asks outside a batch read
//! the world directly.

use std::sync::OnceLock;

use crate::map::cell_index::NativeCellIdentity as Cell;
use crate::map::entities::EntityCategory;
use crate::rules::terrain_rules::SpeedCostProfile;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::movement::bump_crush::CrushTarget;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::RawCellKey;
use crate::sim::overlay_grid::OverlayCell;
use crate::sim::world::Simulation;

/// One batch's [`CellFacts`] by real cell index and [`BlockerFacts`] by
/// entity slot, each kept from its first ask.
#[derive(Default)]
pub(crate) struct EntryFacts {
    cells: Vec<OnceLock<CellFacts>>,
    blockers: Vec<OnceLock<BlockerFacts>>,
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
        crate::sim::parallel::for_each_mut(&mut self.cells, |kept| {
            kept.take();
        });
        self.cells.resize_with(cells, OnceLock::new);
        crate::sim::parallel::for_each_mut(&mut self.blockers, |kept| {
            kept.take();
        });
        self.blockers.resize_with(slots, OnceLock::new);
    }

    /// Real cell `index`'s facts, kept from this batch's first ask.
    pub(super) fn cell(&self, index: usize, read: impl Fn() -> CellFacts) -> &CellFacts {
        let kept = self.cells[index].get_or_init(&read);
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || *kept == read(),
            "real cell {index} changed during a path batch"
        );
        kept
    }

    /// The facts of the entity in `slot`, kept from this batch's first ask.
    pub(super) fn blocker(&self, slot: usize, read: impl Fn() -> BlockerFacts) -> &BlockerFacts {
        let kept = self.blockers[slot].get_or_init(&read);
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || *kept == read(),
            "the entity in slot {slot} changed during a path batch"
        );
        kept
    }
}

/// What Can_Enter_Cell reads of one real cell. Its object lists' members, its
/// tube, the first-Building lookups and MovementRestrictedTo's land, tile and
/// overlay reads stay live.
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
        let terrain_object = sim
            .production
            .terrain_object_cells
            .get(&(c.rx, c.ry))
            .copied();
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
