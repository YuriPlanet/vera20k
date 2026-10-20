//! Every viewer's sight plane, and the source sight writes the object pass
//! leaves for later.

use std::collections::BTreeMap;
use std::ops::Range;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{InternedId, OwnerVisibility};
use crate::map::resolved_terrain::NativeCellQuery;

/// Every viewer's [`OwnerVisibility`], keyed by viewer.
///
/// The object pass moves thousands of sight sources a frame, and each move
/// walks its viewers' planes cell by cell. Between [`Self::defer`] and
/// [`Self::settle`], the source writes ([`PlaneWrite`]) to every plane but the
/// kept viewers' wait on their plane, and the settle applies each plane's in
/// the order they were made, the planes in parallel. The planes end as the
/// writes would have left them at once: a waiting write reads and writes only
/// its own plane, keeps its inputs, and nothing reaches the plane before it
/// applies. Every other use of a plane applies its waiting writes first, and a
/// read of a plane with writes waiting panics. The kept viewers are the ones
/// the pass reads: the human players and the current house, whose map the
/// app's sound boundary also reads (`advance_live_object_pass`).
#[derive(Debug, Clone, Default)]
pub struct Planes {
    planes: BTreeMap<InternedId, Plane>,
    /// While deferring, the viewers whose writes apply at once.
    kept: Option<Vec<InternedId>>,
}

#[derive(Debug, Clone)]
struct Plane {
    view: OwnerVisibility,
    /// Writes waiting to apply, in order, each over its range of `cells`.
    waiting: Vec<(PlaneWrite, Range<usize>)>,
    cells: Vec<(u16, u16)>,
}

/// A sight source's write to one plane, over a list of cells.
#[derive(Debug, Clone, Copy)]
pub(super) enum PlaneWrite {
    /// [`OwnerVisibility::reveal_source_cells`]: a source's sight walk.
    Reveal { final_pass: bool },
    /// [`OwnerVisibility::refresh_ground_frames`]: a sight border's refresh.
    Border,
    /// [`OwnerVisibility::count_sources`]: a retained footprint's source
    /// counts, `local` for its owner's own plane.
    Sources { local: bool, add: bool },
}

impl OwnerVisibility {
    fn apply(
        &mut self,
        write: PlaneWrite,
        cells: &[(u16, u16)],
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        match write {
            PlaneWrite::Reveal { final_pass } => {
                self.reveal_source_cells(cells, final_pass, native_cells);
            }
            PlaneWrite::Border => self.refresh_ground_frames(cells, native_cells),
            PlaneWrite::Sources { local, add } => self.count_sources(cells, local, add),
        }
    }
}

impl Plane {
    fn new(view: OwnerVisibility) -> Self {
        Self {
            view,
            waiting: Vec::new(),
            cells: Vec::new(),
        }
    }

    fn view(&self) -> &OwnerVisibility {
        assert!(
            self.waiting.is_empty(),
            "a sight plane read while its writes wait"
        );
        &self.view
    }

    /// The plane, its waiting writes applied.
    fn settled(&mut self) -> &mut OwnerVisibility {
        for (write, cells) in self.waiting.drain(..) {
            // Every waiting walk's cells are interior, where it looks no
            // cell up ([`Planes::write`]).
            self.view.apply(write, &self.cells[cells], None);
        }
        self.cells.clear();
        &mut self.view
    }
}

impl Planes {
    /// `viewer`'s plane. Panics if writes wait on it.
    pub fn get(&self, viewer: &InternedId) -> Option<&OwnerVisibility> {
        self.planes.get(viewer).map(Plane::view)
    }

    /// `viewer`'s plane, its waiting writes applied.
    pub fn get_mut(&mut self, viewer: &InternedId) -> Option<&mut OwnerVisibility> {
        self.planes.get_mut(viewer).map(Plane::settled)
    }

    /// `viewer`'s plane, `new` if it has none, its waiting writes applied.
    pub fn get_or_insert_with(
        &mut self,
        viewer: InternedId,
        new: impl FnOnce() -> OwnerVisibility,
    ) -> &mut OwnerVisibility {
        self.planes
            .entry(viewer)
            .or_insert_with(|| Plane::new(new()))
            .settled()
    }

    /// Replace `viewer`'s plane; the old one, its waiting writes applied.
    pub fn insert(&mut self, viewer: InternedId, view: OwnerVisibility) -> Option<OwnerVisibility> {
        let mut old = self.planes.insert(viewer, Plane::new(view))?;
        old.settled();
        Some(old.view)
    }

    pub fn contains_key(&self, viewer: &InternedId) -> bool {
        self.planes.contains_key(viewer)
    }

    pub fn len(&self) -> usize {
        self.planes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.planes.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &InternedId> {
        self.planes.keys()
    }

    /// Every plane by viewer. Panics if writes wait on one.
    pub fn iter(&self) -> impl Iterator<Item = (&InternedId, &OwnerVisibility)> {
        self.planes
            .iter()
            .map(|(viewer, plane)| (viewer, plane.view()))
    }

    /// Every plane. Panics if writes wait on one.
    pub fn values(&self) -> impl Iterator<Item = &OwnerVisibility> {
        self.planes.values().map(Plane::view)
    }

    /// Every plane by viewer, its waiting writes applied.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&InternedId, &mut OwnerVisibility)> {
        self.planes
            .iter_mut()
            .map(|(viewer, plane)| (viewer, plane.settled()))
    }

    /// Every plane, its waiting writes applied.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut OwnerVisibility> {
        self.planes.values_mut().map(Plane::settled)
    }

    pub fn clear(&mut self) {
        self.planes.clear();
    }

    /// Leave the source writes to every plane but `kept`'s waiting until
    /// [`Self::settle`].
    pub(super) fn defer(&mut self, kept: Vec<InternedId>) {
        self.kept = Some(kept);
    }

    /// Stop deferring, and apply every plane's waiting writes, the planes in
    /// parallel.
    pub(super) fn settle(&mut self) {
        self.kept = None;
        let mut waiting: Vec<&mut Plane> = self
            .planes
            .values_mut()
            .filter(|plane| !plane.waiting.is_empty())
            .collect();
        crate::sim::parallel::for_each_mut(&mut waiting, |plane| {
            plane.settled();
        });
    }

    /// Apply `write` over `cells` to `viewer`'s plane, made `width` by
    /// `height` if it has none, or while deferring leave it to wait. A walk
    /// or a border refresh waits only when all its cells are interior
    /// ([`OwnerVisibility::interior_index`]): there it looks no cell up, so
    /// it neither reads nor stamps the shared Dummy, and gives the same
    /// result without `native_cells`.
    pub(super) fn write(
        &mut self,
        viewer: InternedId,
        (width, height): (u16, u16),
        write: PlaneWrite,
        cells: &[(u16, u16)],
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        let defer = self
            .kept
            .as_ref()
            .is_some_and(|kept| !kept.contains(&viewer));
        let plane = self
            .planes
            .entry(viewer)
            .or_insert_with(|| Plane::new(OwnerVisibility::new(width, height)));
        let looks_up = matches!(write, PlaneWrite::Reveal { .. } | PlaneWrite::Border);
        if defer
            && (!looks_up
                || cells
                    .iter()
                    .all(|&cell| plane.view.interior_index(cell, native_cells).is_some()))
        {
            let start = plane.cells.len();
            plane.cells.extend_from_slice(cells);
            plane.waiting.push((write, start..plane.cells.len()));
        } else {
            plane.settled().apply(write, cells, native_cells);
        }
    }
}

impl std::ops::Index<&InternedId> for Planes {
    type Output = OwnerVisibility;

    /// `viewer`'s plane. Panics if it has none or writes wait on it.
    fn index(&self, viewer: &InternedId) -> &OwnerVisibility {
        self.get(viewer).expect("viewer has a sight plane")
    }
}

impl From<BTreeMap<InternedId, OwnerVisibility>> for Planes {
    fn from(planes: BTreeMap<InternedId, OwnerVisibility>) -> Self {
        Self {
            planes: planes
                .into_iter()
                .map(|(viewer, view)| (viewer, Plane::new(view)))
                .collect(),
            kept: None,
        }
    }
}

/// Saved as the map of planes. No write waits between frames.
impl Serialize for Planes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.iter())
    }
}

impl<'de> Deserialize<'de> for Planes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        BTreeMap::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::{PlaneWrite, Planes};
    use crate::map::cell_index::NativeCellIdentity;
    use crate::map::resolved_terrain::{NativeCellQuery, test_flat_cell, test_grid};
    use crate::sim::intern::test_intern;

    fn square(x: u16, y: u16, radius: u16) -> Vec<(u16, u16)> {
        (y - radius..=y + radius)
            .flat_map(|y| (x - radius..=x + radius).map(move |x| (x, y)))
            .collect()
    }

    /// One viewer's writes: walks, border refreshes and source counts over
    /// interior cells, which wait while deferring, and over the unallocated
    /// cell (9, 6) and beside it, where lookups read the shared Dummy, or at
    /// the map's edge, which apply at once.
    fn script() -> Vec<(PlaneWrite, Vec<(u16, u16)>)> {
        let sources = |add| PlaneWrite::Sources { local: true, add };
        let walk = |final_pass| PlaneWrite::Reveal { final_pass };
        vec![
            (sources(true), square(5, 11, 2)),
            (walk(false), square(5, 11, 2)),
            (PlaneWrite::Border, square(5, 11, 3)),
            (walk(false), square(9, 6, 1)),
            (sources(true), square(6, 11, 2)),
            (walk(false), square(6, 11, 2)),
            (PlaneWrite::Border, square(8, 6, 2)),
            (walk(false), square(2, 3, 2)),
            (sources(false), square(5, 11, 2)),
            (walk(true), square(5, 11, 2)),
            (PlaneWrite::Border, square(6, 11, 3)),
            (walk(false), square(7, 11, 2)),
        ]
    }

    /// A kept and a deferred viewer's writes in turn on a map with a hole,
    /// with another use of the deferred plane between them: each plane as
    /// debug text, its derived lists included, and where the Dummy was last
    /// stamped.
    fn run(defer: bool) -> (Vec<String>, (i16, i16)) {
        let mut terrain = test_grid(16, 16, test_flat_cell);
        let allocated: Vec<_> = (0..16)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .filter(|&cell| cell != (9, 6))
            .collect();
        terrain.test_set_native_allocated_cells(&allocated);
        let query = NativeCellQuery::isolated(&terrain);
        let (kept, deferred) = (test_intern("Kept"), test_intern("Deferred"));
        let mut planes = Planes::default();
        if defer {
            planes.defer(vec![kept]);
        }
        for (step, (write, cells)) in script().into_iter().enumerate() {
            for viewer in [kept, deferred] {
                planes.write(viewer, (16, 16), write, &cells, Some(&query));
            }
            if step == 4 {
                planes.get_mut(&deferred);
            }
        }
        if defer {
            assert!(planes.planes[&deferred].waiting.len() > 1, "writes waited");
            assert!(planes.planes[&kept].waiting.is_empty());
        }
        planes.settle();
        (
            planes.iter().map(|plane| format!("{plane:?}")).collect(),
            query.coord(NativeCellIdentity::Dummy),
        )
    }

    #[test]
    fn waiting_writes_end_as_immediate_ones() {
        assert_eq!(run(true), run(false));
    }

    #[test]
    #[should_panic(expected = "a sight plane read while its writes wait")]
    fn a_plane_cannot_be_read_while_its_writes_wait() {
        let viewer = test_intern("Deferred");
        let mut planes = Planes::default();
        planes.defer(Vec::new());
        let count = PlaneWrite::Sources {
            local: true,
            add: true,
        };
        planes.write(viewer, (16, 16), count, &square(6, 6, 1), None);
        let _ = planes.get(&viewer);
    }
}
