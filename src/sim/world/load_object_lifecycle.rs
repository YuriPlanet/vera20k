//! Authored and runtime OverlayClass registry and deferred-destruction owner.
//!
//! Active YR temporarily keeps authored OverlayClass objects in five registries
//! while their synchronous Mark effects mutate CellClass state. Ordinary and
//! wall paths die and enter the deferred queue; Terrain and steep-slope rejects
//! remain registered until scene teardown. The authored reader and admitted
//! Main_Tick tail drain this same owner. None of these records are gameplay
//! entities, LogicClass objects, serialized state, or checksum authority.

use std::collections::BTreeMap;

/// Collision-free Rust identity for one transient load OverlayClass analogue.
/// Native numeric IDs are stored separately and may duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LoadOverlayHandle(u64);

impl LoadOverlayHandle {
    pub(crate) const fn from_stable_id(stable_id: u64) -> Self {
        Self(stable_id)
    }

    #[cfg(test)]
    pub(crate) const fn stable_id(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadObjectRegistryKind {
    Object,
    PointerExpiration,
    AllAbstract,
    TagRemoval,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadOverlayTerminalPath {
    Constructed,
    UnrevealedSurvivor,
    CommonQueued,
    WallQueued,
    SlopeSurvivor,
}

#[derive(Debug)]
struct LoadOverlayObject {
    overlay_id: u8,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoadOverlayObjectSnapshot {
    pub(crate) stable_id: u64,
    pub(crate) overlay_id: u8,
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
pub(crate) enum LoadOverlayLifecycleError {
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
/// Stock runtime bridge constructors publish only Cell state, not references
/// to these objects; their partitioned destruction commutes with gameplay
/// finalization at the same admitted725C70 boundary. Revisit that separation
/// before publishing Overlay handles or adding cross-owner destructor effects.
#[derive(Debug)]
pub(crate) struct LoadObjectLifecycle {
    objects: BTreeMap<LoadOverlayHandle, LoadOverlayObject>,
    object_registry: Vec<LoadOverlayHandle>,
    pointer_expiration_registry: Vec<LoadOverlayHandle>,
    all_abstract_registry: Vec<LoadOverlayHandle>,
    tag_removal_registry: Vec<LoadOverlayHandle>,
    overlay_registry: Vec<LoadOverlayHandle>,
    deferred: Vec<LoadOverlayHandle>,
    #[cfg(test)]
    events: Vec<LoadObjectLifecycleEvent>,
    #[cfg(test)]
    fail_next_registry: Option<LoadObjectRegistryKind>,
    #[cfg(test)]
    fail_next_queue: bool,
}

impl Default for LoadObjectLifecycle {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            object_registry: Vec::new(),
            pointer_expiration_registry: Vec::new(),
            all_abstract_registry: Vec::new(),
            tag_removal_registry: Vec::new(),
            overlay_registry: Vec::new(),
            deferred: Vec::new(),
            #[cfg(test)]
            events: Vec::new(),
            #[cfg(test)]
            fail_next_registry: None,
            #[cfg(test)]
            fail_next_queue: false,
        }
    }
}

impl LoadObjectLifecycle {
    /// Construct the base object, join four base registries, assign the native
    /// ID, then join the Overlay registry. A growth failure is a hard load
    /// error at its exact point; the staged Simulation retains earlier effects.
    pub(crate) fn construct_overlay(
        &mut self,
        stable_id: u64,
        overlay_id: u8,
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
                overlay_id,
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

    /// Direct-base Unlimbo followed by the virtual Mark base prefix. The one
    /// tactical dirty intent precedes every derived slope/high/low/ordinary arm.
    pub(crate) fn begin_mark(
        &mut self,
        handle: LoadOverlayHandle,
    ) -> Result<(u16, u16), LoadOverlayLifecycleError> {
        let object = self.object_mut_for_transition(handle)?;
        if object.terminal_path != LoadOverlayTerminalPath::Constructed {
            return Err(LoadOverlayLifecycleError::InvalidTransition(handle.0));
        }
        object.limbo = false;
        //5FC380 supplies the requested signed CellStruct center to5F4EC0.
        // A Terrain/EmptyCell rejection never reaches this world-position store.
        object.world = [
            i32::from(object.cell.0 as i16) * 256 + 128,
            i32::from(object.cell.1 as i16) * 256 + 128,
            0,
        ];
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
    ) -> Result<(), LoadOverlayLifecycleError> {
        self.finish_queued(handle, LoadOverlayTerminalPath::CommonQueued, false)
    }

    /// Wall rejection performs its ordinary UnInit broadcast and a full-Limbo
    /// broadcast before queueing; the destructor supplies the third broadcast.
    #[cfg(test)]
    pub(crate) fn finish_wall_reject(
        &mut self,
        handle: LoadOverlayHandle,
    ) -> Result<(), LoadOverlayLifecycleError> {
        self.finish_queued(handle, LoadOverlayTerminalPath::WallQueued, true)
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

    /// Native725C70's reader-epilogue or admitted late-frame forward scan.
    /// Alive queued entries advance;
    /// a dead entry is destroyed, all duplicate queue entries are removed
    /// stably, and the same live index is examined again.
    pub(crate) fn drain_deferred(&mut self) -> Result<(), LoadOverlayLifecycleError> {
        let mut index = 0;
        while index < self.deferred.len() {
            let handle = self.deferred[index];
            let alive = self
                .objects
                .get(&handle)
                .ok_or(LoadOverlayLifecycleError::UnknownHandle(handle.0))?
                .alive;
            if alive {
                index += 1;
                continue;
            }
            self.destroy(handle)?;
        }
        Ok(())
    }

    /// Release the registered constructor survivors when the scene is
    /// torn down. Iteration order is intentionally not claimed as native parity.
    #[cfg(test)]
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

    #[cfg(test)]
    pub(crate) fn snapshot(&self, handle: LoadOverlayHandle) -> Option<LoadOverlayObjectSnapshot> {
        let object = self.objects.get(&handle)?;
        Some(LoadOverlayObjectSnapshot {
            stable_id: handle.0,
            overlay_id: object.overlay_id,
            cell: object.cell,
            world: object.world,
            native_id: object.native_id,
            alive: object.alive,
            limbo: object.limbo,
            on_map: object.on_map,
            redraw: object.redraw,
            expiration_broadcasts: object.expiration_broadcasts,
            queued: self.deferred.contains(&handle),
            slope_survivor: object.terminal_path == LoadOverlayTerminalPath::SlopeSurvivor,
        })
    }

    #[cfg(test)]
    pub(crate) fn object_count(&self) -> usize {
        self.objects.len()
    }

    #[cfg(test)]
    pub(crate) fn registry_counts(&self) -> [usize; 5] {
        [
            self.object_registry.len(),
            self.pointer_expiration_registry.len(),
            self.all_abstract_registry.len(),
            self.tag_removal_registry.len(),
            self.overlay_registry.len(),
        ]
    }

    #[cfg(test)]
    pub(crate) fn queue_count(&self) -> usize {
        self.deferred.len()
    }

    fn finish_queued(
        &mut self,
        handle: LoadOverlayHandle,
        terminal_path: LoadOverlayTerminalPath,
        full_limbo: bool,
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
            object.terminal_path = terminal_path;
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

        self.try_queue(handle)?;
        self.record(LoadObjectLifecycleEvent::Queue(handle));
        Ok(())
    }

    fn try_join(
        &mut self,
        registry: LoadObjectRegistryKind,
        handle: LoadOverlayHandle,
    ) -> Result<(), LoadOverlayLifecycleError> {
        #[cfg(test)]
        if self.fail_next_registry == Some(registry) {
            self.fail_next_registry = None;
            return Err(LoadOverlayLifecycleError::RegistryCapacity { registry });
        }

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

    fn try_queue(&mut self, handle: LoadOverlayHandle) -> Result<(), LoadOverlayLifecycleError> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_queue) {
            return Err(LoadOverlayLifecycleError::DeferredQueueCapacity);
        }
        self.deferred
            .try_reserve(1)
            .map_err(|_| LoadOverlayLifecycleError::DeferredQueueCapacity)?;
        self.deferred.push(handle);
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
        self.deferred.retain(|queued| *queued != handle);
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
        #[cfg(test)]
        self.events.push(event);
        #[cfg(not(test))]
        let _ = event;
    }

    #[cfg(test)]
    fn fail_next_join_for_test(&mut self, registry: LoadObjectRegistryKind) {
        self.fail_next_registry = Some(registry);
    }

    #[cfg(test)]
    fn fail_next_queue_for_test(&mut self) {
        self.fail_next_queue = true;
    }

    #[cfg(test)]
    fn queue_existing_for_test(&mut self, handle: LoadOverlayHandle) {
        self.deferred.push(handle);
    }

    #[cfg(test)]
    fn events(&self) -> &[LoadObjectLifecycleEvent] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn construct(
        lifecycle: &mut LoadObjectLifecycle,
        stable_id: u64,
        native_id: i32,
    ) -> LoadOverlayHandle {
        lifecycle
            .construct_overlay(stable_id, 0x18, (10, 11), || native_id)
            .unwrap()
    }

    #[test]
    fn constructor_joins_four_base_registries_then_id_then_overlay() {
        let mut lifecycle = LoadObjectLifecycle::default();
        let handle = construct(&mut lifecycle, 7, 1_010_038);

        assert_eq!(handle.stable_id(), 7);
        assert_eq!(
            lifecycle.snapshot(handle).unwrap().native_id,
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
        let common = construct(&mut lifecycle, 1, 101);
        let wall = construct(&mut lifecycle, 2, 102);
        let slope = construct(&mut lifecycle, 3, 103);

        lifecycle.begin_mark(common).unwrap();
        lifecycle.finish_common(common).unwrap();
        lifecycle.begin_mark(wall).unwrap();
        lifecycle.finish_wall_reject(wall).unwrap();
        lifecycle.begin_mark(slope).unwrap();
        lifecycle.finish_slope_survivor(slope).unwrap();

        let common_before_drain = lifecycle.snapshot(common).unwrap();
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
        let wall_before_drain = lifecycle.snapshot(wall).unwrap();
        assert_eq!(wall_before_drain.expiration_broadcasts, 2);
        assert!(wall_before_drain.queued);
        let slope_live = lifecycle.snapshot(slope).unwrap();
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

        lifecycle.drain_deferred().unwrap();
        assert!(lifecycle.snapshot(common).is_none());
        assert!(lifecycle.snapshot(wall).is_none());
        assert!(lifecycle.snapshot(slope).is_some());
        assert_eq!(lifecycle.release_scene_survivors().unwrap(), 1);
        assert_eq!(lifecycle.object_count(), 0);
    }

    #[test]
    fn duplicate_aware_drain_rechecks_same_live_index_and_keeps_alive_entries() {
        let mut lifecycle = LoadObjectLifecycle::default();
        let alive_a = construct(&mut lifecycle, 1, 11);
        let dead_b = construct(&mut lifecycle, 2, 12);
        let alive_c = construct(&mut lifecycle, 3, 13);
        let dead_d = construct(&mut lifecycle, 4, 14);
        for handle in [dead_b, dead_d] {
            lifecycle.begin_mark(handle).unwrap();
            lifecycle.finish_common(handle).unwrap();
        }
        lifecycle.deferred.clear();
        for handle in [alive_a, dead_b, dead_b, alive_c, dead_d] {
            lifecycle.queue_existing_for_test(handle);
        }

        lifecycle.drain_deferred().unwrap();

        assert!(lifecycle.snapshot(dead_b).is_none());
        assert!(lifecycle.snapshot(dead_d).is_none());
        assert!(lifecycle.snapshot(alive_a).is_some());
        assert!(lifecycle.snapshot(alive_c).is_some());
        assert_eq!(lifecycle.deferred, vec![alive_a, alive_c]);
    }

    #[test]
    fn every_registry_growth_failure_stops_at_its_exact_join() {
        for registry in [
            LoadObjectRegistryKind::Object,
            LoadObjectRegistryKind::PointerExpiration,
            LoadObjectRegistryKind::AllAbstract,
            LoadObjectRegistryKind::TagRemoval,
            LoadObjectRegistryKind::Overlay,
        ] {
            let mut lifecycle = LoadObjectLifecycle::default();
            lifecycle.fail_next_join_for_test(registry);
            let mut id_calls = 0;
            let error = lifecycle
                .construct_overlay(1, 0x18, (1, 2), || {
                    id_calls += 1;
                    77
                })
                .unwrap_err();
            assert_eq!(
                error,
                LoadOverlayLifecycleError::RegistryCapacity { registry }
            );
            assert_eq!(
                id_calls,
                usize::from(registry == LoadObjectRegistryKind::Overlay),
                "native ID is assigned only after all four base joins"
            );
        }
    }

    #[test]
    fn queue_growth_failure_retains_committed_uninit_prefix() {
        let mut lifecycle = LoadObjectLifecycle::default();
        let handle = construct(&mut lifecycle, 1, 99);
        lifecycle.begin_mark(handle).unwrap();
        lifecycle.fail_next_queue_for_test();

        assert_eq!(
            lifecycle.finish_common(handle),
            Err(LoadOverlayLifecycleError::DeferredQueueCapacity)
        );
        let snapshot = lifecycle.snapshot(handle).unwrap();
        assert!(!snapshot.alive);
        assert!(snapshot.limbo);
        assert!(!snapshot.on_map);
        assert_eq!(snapshot.expiration_broadcasts, 1);
        assert!(!snapshot.queued);
    }
}
