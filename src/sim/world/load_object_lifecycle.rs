//! Authored and runtime OverlayClass registry and deferred-destruction owner.
//!
//! Active YR temporarily keeps authored OverlayClass objects in five registries
//! while their synchronous Mark effects mutate CellClass state. Ordinary and
//! wall paths die and enter the deferred queue; Terrain and steep-slope rejects
//! remain registered until scene teardown. The authored reader and admitted
//! Main_Tick tail share the substrate's one ordered deferred queue. Registry
//! records are saved with retained Overlay tickets; they are not LogicClass
//! entities. Their retained lifetime/registry state joins the Simulation hash;
//! cell gameplay remains with the canonical CellClass authority.

use super::substrate::DeferredDeleteQueue;
use std::collections::BTreeMap;
use std::hash::Hash;

/// Collision-free Rust identity for one transient load OverlayClass analogue.
/// Native numeric IDs are stored separately and may duplicate.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct LoadOverlayHandle(u64);

/// Ordered constructor effect emitted by a synchronous cell mutation. Cell
/// state is already published; the shared registry/identity owner must consume
/// this effect before any later gameplay constructor or object turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverlayConstructionReceipt {
    cell: (u16, u16),
    terrain_refusal: bool,
}

impl OverlayConstructionReceipt {
    pub(crate) fn new(cell: (u16, u16), terrain_refusal: bool) -> Self {
        Self {
            cell,
            terrain_refusal,
        }
    }

    pub(crate) fn cell(self) -> (u16, u16) {
        self.cell
    }
}

impl LoadOverlayHandle {
    pub(crate) const fn from_stable_id(stable_id: u64) -> Self {
        Self(stable_id)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn stable_id(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadObjectRegistryKind {
    Object,
    PointerExpiration,
    AllAbstract,
    TagRemoval,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum LoadOverlayTerminalPath {
    Constructed,
    UnrevealedSurvivor,
    CommonQueued,
    SlopeSurvivor,
}

#[derive(Debug, Hash, serde::Serialize, serde::Deserialize)]
struct LoadOverlayObject {
    cell: (u16, u16),
    world: [i32; 3],
    native_id: Option<i32>,
    alive: bool,
    limbo: bool,
    on_map: bool,
    redraw: bool,
    expiration_broadcasts: u8,
    terminal_path: LoadOverlayTerminalPath,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoadOverlayObjectSnapshot {
    pub(crate) stable_id: u64,
    pub(crate) cell: (u16, u16),
    pub(crate) world: [i32; 3],
    pub(crate) native_id: Option<i32>,
    pub(crate) alive: bool,
    pub(crate) limbo: bool,
    pub(crate) on_map: bool,
    pub(crate) redraw: bool,
    pub(crate) expiration_broadcasts: u8,
    pub(crate) queued: bool,
    pub(crate) slope_survivor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadObjectLifecycleEvent {
    Allocate(LoadOverlayHandle),
    BaseConstruct(LoadOverlayHandle),
    Join(LoadObjectRegistryKind, LoadOverlayHandle),
    AssignNativeId(LoadOverlayHandle, i32),
    BaseUnlimbo(LoadOverlayHandle),
    TacticalDirty(LoadOverlayHandle, (u16, u16)),
    UninitBroadcast(LoadOverlayHandle),
    FullLimbo(LoadOverlayHandle),
    Queue(LoadOverlayHandle),
    SlopeSurvivor(LoadOverlayHandle),
    DestructorBroadcast(LoadOverlayHandle),
    Remove(LoadObjectRegistryKind, LoadOverlayHandle),
    ClearType(LoadOverlayHandle),
    RemoveQueuedDuplicates(LoadOverlayHandle),
    Free(LoadOverlayHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LoadOverlayLifecycleError {
    #[error("load Overlay stable handle {0} already exists")]
    DuplicateHandle(u64),
    #[error("load Overlay handle {0} does not exist")]
    UnknownHandle(u64),
    #[error("load Overlay registry {registry:?} could not grow")]
    RegistryCapacity { registry: LoadObjectRegistryKind },
    #[error("load Overlay deferred-finalization queue could not grow")]
    DeferredQueueCapacity,
    #[error("load Overlay handle {0} crossed an invalid lifecycle transition")]
    InvalidTransition(u64),
}

/// Shared owner for native OverlayClass registry membership. The historical
/// load-prefixed handle names do not restrict the lifetime to map reading.
/// It owns Overlay readiness/destruction; substrate owns every deferred ticket.
/// Gameplay and Tag/Trigger callbacks therefore retain native interleaving.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct LoadObjectLifecycle {
    objects: BTreeMap<LoadOverlayHandle, LoadOverlayObject>,
    object_registry: Vec<LoadOverlayHandle>,
    pointer_expiration_registry: Vec<LoadOverlayHandle>,
    all_abstract_registry: Vec<LoadOverlayHandle>,
    tag_removal_registry: Vec<LoadOverlayHandle>,
    overlay_registry: Vec<LoadOverlayHandle>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    events: Vec<LoadObjectLifecycleEvent>,
}

impl LoadObjectLifecycle {
    pub(crate) fn has_overlay(&self, id: u64) -> bool {
        self.objects.contains_key(&LoadOverlayHandle(id))
    }
    /// Retained physical identities participate in the shared object-ID
    /// namespace even after their Logic/cell publication has ended.
    pub(crate) fn retained_overlay_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.objects.keys().map(|handle| handle.0)
    }
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        if self.objects.is_empty() {
            return;
        }
        "RetainedOverlayRegistry".hash(hasher);
        self.objects.hash(hasher);
        self.object_registry.hash(hasher);
        self.pointer_expiration_registry.hash(hasher);
        self.all_abstract_registry.hash(hasher);
        self.tag_removal_registry.hash(hasher);
        self.overlay_registry.hash(hasher);
    }
    /// Construct the base object, join four base registries, assign the native
    /// ID, then join the Overlay registry. A growth failure is a hard load
    /// error at its exact point; the staged Simulation retains earlier effects.
    pub(crate) fn construct_overlay(
        &mut self,
        stable_id: u64,
        cell: (u16, u16),
        mut assign_native_id: impl FnMut() -> i32,
    ) -> Result<LoadOverlayHandle, LoadOverlayLifecycleError> {
        let handle = LoadOverlayHandle::from_stable_id(stable_id);
        if self.objects.contains_key(&handle) {
            return Err(LoadOverlayLifecycleError::DuplicateHandle(stable_id));
        }

        self.record(LoadObjectLifecycleEvent::Allocate(handle));
        self.objects.insert(
            handle,
            LoadOverlayObject {
                cell,
                world: [0; 3],
                native_id: None,
                alive: true,
                limbo: true,
                on_map: false,
                redraw: false,
                expiration_broadcasts: 0,
                terminal_path: LoadOverlayTerminalPath::Constructed,
            },
        );
        self.record(LoadObjectLifecycleEvent::BaseConstruct(handle));

        for registry in [
            LoadObjectRegistryKind::Object,
            LoadObjectRegistryKind::PointerExpiration,
            LoadObjectRegistryKind::AllAbstract,
            LoadObjectRegistryKind::TagRemoval,
        ] {
            self.try_join(registry, handle)?;
        }

        let native_id = assign_native_id();
        self.objects
            .get_mut(&handle)
            .expect("new load Overlay object exists")
            .native_id = Some(native_id);
        self.record(LoadObjectLifecycleEvent::AssignNativeId(handle, native_id));
        self.try_join(LoadObjectRegistryKind::Overlay, handle)?;
        Ok(handle)
    }

    /// Apply the terminal path of an ordinary constructor whose Cell effects
    /// were published by its caller. Original5FC380 checks Terrain after the
    /// ID assignment;5FC570's ordinary Mark ends in5F65F0. This reuses the same
    /// transitions as authored loading and runtime bridges, including refused
    /// live-limbo survivors and the admitted725C70 deferred drain.
    pub(crate) fn finish_cell_construction(
        &mut self,
        handle: LoadOverlayHandle,
        receipt: OverlayConstructionReceipt,
        queue: &mut DeferredDeleteQueue,
    ) -> Result<(), LoadOverlayLifecycleError> {
        if receipt.terrain_refusal {
            self.finish_unrevealed_survivor(handle)
        } else {
            self.begin_mark(handle, None)?;
            self.finish_common(handle, queue)
        }
    }

    /// Direct-base Unlimbo followed by the virtual Mark base prefix. The one
    /// tactical dirty intent precedes every derived slope/high/low/ordinary arm.
    pub(crate) fn begin_mark(
        &mut self,
        handle: LoadOverlayHandle,
        requested_world: Option<[i32; 3]>,
    ) -> Result<(u16, u16), LoadOverlayLifecycleError> {
        let object = self.object_mut_for_transition(handle)?;
        if object.terminal_path != LoadOverlayTerminalPath::Constructed {
            return Err(LoadOverlayLifecycleError::InvalidTransition(handle.0));
        }
        object.limbo = false;
        //5FC380 supplies a signed CellStruct center; Building440580 calls
        //5FD270 with its original XYZ. OverlayType5FEA30 (vt7EF600+6C)
        //copies that XYZ unchanged before Object5F4FA8 commits Location.
        //Terrain/EmptyCell rejection never reaches this position store.
        object.world = requested_world.unwrap_or_else(|| {
            [
                i32::from(object.cell.0 as i16) * 256 + 128,
                i32::from(object.cell.1 as i16) * 256 + 128,
                0,
            ]
        });
        object.on_map = true;
        object.redraw = true;
        let cell = object.cell;
        self.record(LoadObjectLifecycleEvent::BaseUnlimbo(handle));
        self.record(LoadObjectLifecycleEvent::TacticalDirty(handle, cell));
        Ok(cell)
    }

    /// Constructor5FC380 returns without Unlimbo for EmptyCell or Terrain.
    /// Allocation, native ID and registry membership survive; position remains
    /// ObjectClass's zero initialization. Evidence: bridge_constructor corpus.
    pub(crate) fn finish_unrevealed_survivor(
        &mut self,
        handle: LoadOverlayHandle,
    ) -> Result<(), LoadOverlayLifecycleError> {
        let object = self.object_mut_for_transition(handle)?;
        if object.terminal_path != LoadOverlayTerminalPath::Constructed || !object.limbo {
            return Err(LoadOverlayLifecycleError::InvalidTransition(handle.0));
        }
        object.terminal_path = LoadOverlayTerminalPath::UnrevealedSurvivor;
        Ok(())
    }

    /// Common successful Mark tail: one UnInit broadcast, alive clear, and
    /// append to the shared deferred queue while all registries remain.
    pub(crate) fn finish_common(
        &mut self,
        handle: LoadOverlayHandle,
        queue: &mut DeferredDeleteQueue,
    ) -> Result<(), LoadOverlayLifecycleError> {
        self.finish_queued(handle, false, queue)
    }

    /// Wall rejection performs its ordinary UnInit broadcast and a full-Limbo
    /// broadcast before queueing; the destructor supplies the third broadcast.
    pub(crate) fn finish_wall_reject(
        &mut self,
        handle: LoadOverlayHandle,
        queue: &mut DeferredDeleteQueue,
    ) -> Result<(), LoadOverlayLifecycleError> {
        self.finish_queued(handle, true, queue)
    }

    /// A derived steep-slope failure returns through base Unlimbo. It restores
    /// Limbo only; alive/on-map/redraw and every registry membership survive.
    pub(crate) fn finish_slope_survivor(
        &mut self,
        handle: LoadOverlayHandle,
    ) -> Result<(), LoadOverlayLifecycleError> {
        let object = self.object_mut_for_transition(handle)?;
        if object.terminal_path != LoadOverlayTerminalPath::Constructed || object.limbo {
            return Err(LoadOverlayLifecycleError::InvalidTransition(handle.0));
        }
        object.limbo = true;
        object.terminal_path = LoadOverlayTerminalPath::SlopeSurvivor;
        self.record(LoadObjectLifecycleEvent::SlopeSurvivor(handle));
        Ok(())
    }

    /// Object5F4100 readiness for the shared725C70 virtual dispatch.
    pub(super) fn queued_overlay_is_ready(&self, id: u64) -> bool {
        !self
            .objects
            .get(&LoadOverlayHandle(id))
            .expect("queued Overlay ticket requires its retained registry object")
            .alive
    }

    /// The common queue removed duplicates before this virtual destructor.
    pub(super) fn destroy_queued_overlay(&mut self, id: u64) {
        self.destroy(LoadOverlayHandle(id))
            .expect("selected Overlay ticket requires its retained registry object");
    }

    /// Release the registered constructor survivors when the scene is
    /// torn down. Iteration order is intentionally not claimed as native parity.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn release_scene_survivors(&mut self) -> Result<usize, LoadOverlayLifecycleError> {
        let survivors: Vec<_> = self
            .objects
            .iter()
            .filter_map(|(&handle, object)| {
                matches!(
                    object.terminal_path,
                    LoadOverlayTerminalPath::SlopeSurvivor
                        | LoadOverlayTerminalPath::UnrevealedSurvivor
                )
                .then_some(handle)
            })
            .collect();
        for handle in &survivors {
            self.destroy(*handle)?;
        }
        Ok(survivors.len())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn snapshot(
        &self,
        handle: LoadOverlayHandle,
        queue: &DeferredDeleteQueue,
    ) -> Option<LoadOverlayObjectSnapshot> {
        let object = self.objects.get(&handle)?;
        Some(LoadOverlayObjectSnapshot {
            stable_id: handle.0,
            cell: object.cell,
            world: object.world,
            native_id: object.native_id,
            alive: object.alive,
            limbo: object.limbo,
            on_map: object.on_map,
            redraw: object.redraw,
            expiration_broadcasts: object.expiration_broadcasts,
            queued: queue.contains_overlay(handle.0),
            slope_survivor: object.terminal_path == LoadOverlayTerminalPath::SlopeSurvivor,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn object_count(&self) -> usize {
        self.objects.len()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn registry_counts(&self) -> [usize; 5] {
        [
            self.object_registry.len(),
            self.pointer_expiration_registry.len(),
            self.all_abstract_registry.len(),
            self.tag_removal_registry.len(),
            self.overlay_registry.len(),
        ]
    }

    fn finish_queued(
        &mut self,
        handle: LoadOverlayHandle,
        full_limbo: bool,
        queue: &mut DeferredDeleteQueue,
    ) -> Result<(), LoadOverlayLifecycleError> {
        {
            let object = self.object_mut_for_transition(handle)?;
            if object.terminal_path != LoadOverlayTerminalPath::Constructed || object.limbo {
                return Err(LoadOverlayLifecycleError::InvalidTransition(handle.0));
            }
            object.expiration_broadcasts = object.expiration_broadcasts.saturating_add(1);
            object.alive = false;
            object.on_map = false;
            object.limbo = true;
            // Both UnInit paths share one queued lifetime. Wall refusal's
            // extra broadcast is already retained by its owned fields/events;
            // it does not require another serialized terminal discriminator.
            object.terminal_path = LoadOverlayTerminalPath::CommonQueued;
        }
        self.record(LoadObjectLifecycleEvent::UninitBroadcast(handle));
        if full_limbo {
            let object = self
                .objects
                .get_mut(&handle)
                .expect("queued load Overlay exists");
            object.expiration_broadcasts = object.expiration_broadcasts.saturating_add(1);
            self.record(LoadObjectLifecycleEvent::FullLimbo(handle));
            self.record(LoadObjectLifecycleEvent::UninitBroadcast(handle));
        }

        queue
            .queue_overlay(handle.0)
            .map_err(|_| LoadOverlayLifecycleError::DeferredQueueCapacity)?;
        self.record(LoadObjectLifecycleEvent::Queue(handle));
        Ok(())
    }

    fn try_join(
        &mut self,
        registry: LoadObjectRegistryKind,
        handle: LoadOverlayHandle,
    ) -> Result<(), LoadOverlayLifecycleError> {
        let entries = match registry {
            LoadObjectRegistryKind::Object => &mut self.object_registry,
            LoadObjectRegistryKind::PointerExpiration => &mut self.pointer_expiration_registry,
            LoadObjectRegistryKind::AllAbstract => &mut self.all_abstract_registry,
            LoadObjectRegistryKind::TagRemoval => &mut self.tag_removal_registry,
            LoadObjectRegistryKind::Overlay => &mut self.overlay_registry,
        };
        entries
            .try_reserve(1)
            .map_err(|_| LoadOverlayLifecycleError::RegistryCapacity { registry })?;
        entries.push(handle);
        self.record(LoadObjectLifecycleEvent::Join(registry, handle));
        Ok(())
    }

    fn destroy(&mut self, handle: LoadOverlayHandle) -> Result<(), LoadOverlayLifecycleError> {
        let object = self
            .objects
            .get_mut(&handle)
            .ok_or(LoadOverlayLifecycleError::UnknownHandle(handle.0))?;
        object.expiration_broadcasts = object.expiration_broadcasts.saturating_add(1);
        self.record(LoadObjectLifecycleEvent::DestructorBroadcast(handle));
        self.remove_from_registry(LoadObjectRegistryKind::Overlay, handle);

        let needs_limbo = self
            .objects
            .get(&handle)
            .is_some_and(|object| !object.limbo);
        if needs_limbo {
            let object = self
                .objects
                .get_mut(&handle)
                .expect("destroyed object exists");
            object.limbo = true;
            object.on_map = false;
            object.expiration_broadcasts = object.expiration_broadcasts.saturating_add(1);
            self.record(LoadObjectLifecycleEvent::FullLimbo(handle));
            self.record(LoadObjectLifecycleEvent::UninitBroadcast(handle));
        }

        self.record(LoadObjectLifecycleEvent::ClearType(handle));
        self.record(LoadObjectLifecycleEvent::RemoveQueuedDuplicates(handle));
        for registry in [
            LoadObjectRegistryKind::Object,
            LoadObjectRegistryKind::PointerExpiration,
            LoadObjectRegistryKind::AllAbstract,
            LoadObjectRegistryKind::TagRemoval,
        ] {
            self.remove_from_registry(registry, handle);
        }
        self.objects.remove(&handle);
        self.record(LoadObjectLifecycleEvent::Free(handle));
        Ok(())
    }

    fn remove_from_registry(
        &mut self,
        registry: LoadObjectRegistryKind,
        handle: LoadOverlayHandle,
    ) {
        let entries = match registry {
            LoadObjectRegistryKind::Object => &mut self.object_registry,
            LoadObjectRegistryKind::PointerExpiration => &mut self.pointer_expiration_registry,
            LoadObjectRegistryKind::AllAbstract => &mut self.all_abstract_registry,
            LoadObjectRegistryKind::TagRemoval => &mut self.tag_removal_registry,
            LoadObjectRegistryKind::Overlay => &mut self.overlay_registry,
        };
        entries.retain(|entry| *entry != handle);
        self.record(LoadObjectLifecycleEvent::Remove(registry, handle));
    }

    fn object_mut_for_transition(
        &mut self,
        handle: LoadOverlayHandle,
    ) -> Result<&mut LoadOverlayObject, LoadOverlayLifecycleError> {
        self.objects
            .get_mut(&handle)
            .ok_or(LoadOverlayLifecycleError::UnknownHandle(handle.0))
    }

    fn record(&mut self, event: LoadObjectLifecycleEvent) {
        #[cfg(any(test, feature = "test-support"))]
        self.events.push(event);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = event;
    }

    #[cfg(any(test, feature = "test-support"))]
    fn events(&self) -> &[LoadObjectLifecycleEvent] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::super::substrate::DeferredDeleteTicket;
    use super::*;
    use crate::sim::world::Simulation;

    // Supplied Overlay-only scenes still execute the sole production drain.
    fn drain_fixture(lifecycle: &mut LoadObjectLifecycle, queue: &mut DeferredDeleteQueue) {
        let mut sim = Simulation::new();
        sim.load_objects = std::mem::take(lifecycle);
        sim.substrate.pending_delete = std::mem::take(queue);
        sim.process_pending_delete_with(
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            crate::sim::world::FrameEffects::default(),
            1,
            false,
        );
        *lifecycle = sim.load_objects;
        *queue = sim.substrate.pending_delete;
    }

    fn construct(
        lifecycle: &mut LoadObjectLifecycle,
        stable_id: u64,
        native_id: i32,
    ) -> LoadOverlayHandle {
        lifecycle
            .construct_overlay(stable_id, (10, 11), || native_id)
            .unwrap()
    }

    #[test]
    fn constructor_joins_four_base_registries_then_id_then_overlay() {
        let mut lifecycle = LoadObjectLifecycle::default();
        let queue = DeferredDeleteQueue::default();
        let handle = construct(&mut lifecycle, 7, 1_010_038);

        assert_eq!(handle.stable_id(), 7);
        assert_eq!(
            lifecycle.snapshot(handle, &queue).unwrap().native_id,
            Some(1_010_038)
        );
        assert_eq!(
            lifecycle.events(),
            &[
                LoadObjectLifecycleEvent::Allocate(handle),
                LoadObjectLifecycleEvent::BaseConstruct(handle),
                LoadObjectLifecycleEvent::Join(LoadObjectRegistryKind::Object, handle),
                LoadObjectLifecycleEvent::Join(LoadObjectRegistryKind::PointerExpiration, handle,),
                LoadObjectLifecycleEvent::Join(LoadObjectRegistryKind::AllAbstract, handle,),
                LoadObjectLifecycleEvent::Join(LoadObjectRegistryKind::TagRemoval, handle),
                LoadObjectLifecycleEvent::AssignNativeId(handle, 1_010_038),
                LoadObjectLifecycleEvent::Join(LoadObjectRegistryKind::Overlay, handle),
            ]
        );
    }

    #[test]
    fn common_wall_and_slope_paths_preserve_native_terminal_states() {
        let mut lifecycle = LoadObjectLifecycle::default();
        let mut queue = DeferredDeleteQueue::default();
        let common = construct(&mut lifecycle, 1, 101);
        let wall = construct(&mut lifecycle, 2, 102);
        let slope = construct(&mut lifecycle, 3, 103);

        lifecycle.begin_mark(common, None).unwrap();
        lifecycle.finish_common(common, &mut queue).unwrap();
        lifecycle.begin_mark(wall, None).unwrap();
        lifecycle.finish_wall_reject(wall, &mut queue).unwrap();
        lifecycle.begin_mark(slope, None).unwrap();
        lifecycle.finish_slope_survivor(slope).unwrap();

        let common_before_drain = lifecycle.snapshot(common, &queue).unwrap();
        assert_eq!(
            (
                common_before_drain.alive,
                common_before_drain.limbo,
                common_before_drain.on_map,
                common_before_drain.redraw,
                common_before_drain.expiration_broadcasts,
                common_before_drain.queued,
            ),
            (false, true, false, true, 1, true)
        );
        let wall_before_drain = lifecycle.snapshot(wall, &queue).unwrap();
        assert_eq!(wall_before_drain.expiration_broadcasts, 2);
        assert!(wall_before_drain.queued);
        let slope_live = lifecycle.snapshot(slope, &queue).unwrap();
        assert_eq!(
            (
                slope_live.alive,
                slope_live.limbo,
                slope_live.on_map,
                slope_live.redraw,
                slope_live.expiration_broadcasts,
                slope_live.queued,
                slope_live.slope_survivor,
            ),
            (true, true, true, true, 0, false, true)
        );

        drain_fixture(&mut lifecycle, &mut queue);
        assert!(lifecycle.snapshot(common, &queue).is_none());
        assert!(lifecycle.snapshot(wall, &queue).is_none());
        assert!(lifecycle.snapshot(slope, &queue).is_some());
        assert_eq!(lifecycle.release_scene_survivors().unwrap(), 1);
        assert_eq!(lifecycle.object_count(), 0);
    }

    #[test]
    fn duplicate_aware_drain_rechecks_same_live_index_and_keeps_alive_entries() {
        let mut lifecycle = LoadObjectLifecycle::default();
        let mut queue = DeferredDeleteQueue::default();
        let alive_a = construct(&mut lifecycle, 1, 11);
        let dead_b = construct(&mut lifecycle, 2, 12);
        let alive_c = construct(&mut lifecycle, 3, 13);
        let dead_d = construct(&mut lifecycle, 4, 14);
        for handle in [dead_b, dead_d] {
            lifecycle.begin_mark(handle, None).unwrap();
            lifecycle.finish_common(handle, &mut queue).unwrap();
        }
        queue.clear_for_test();
        for handle in [alive_a, dead_b, dead_b, alive_c, dead_d] {
            queue.queue_overlay(handle.0).unwrap();
        }

        drain_fixture(&mut lifecycle, &mut queue);

        assert!(lifecycle.snapshot(dead_b, &queue).is_none());
        assert!(lifecycle.snapshot(dead_d, &queue).is_none());
        assert!(lifecycle.snapshot(alive_a, &queue).is_some());
        assert!(lifecycle.snapshot(alive_c, &queue).is_some());
        assert_eq!(
            queue.tickets(),
            vec![
                DeferredDeleteTicket::Overlay(alive_a.0),
                DeferredDeleteTicket::Overlay(alive_c.0)
            ]
        );
    }
}
