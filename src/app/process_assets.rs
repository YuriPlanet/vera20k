//! Process-wide asset ownership (F11): one `AssetManager` for the process,
//! leased to the loading pipeline and always returned.
//!
//! Retail keeps one process-global MIX list plus the sticky
//! `LoadFileFromMIX` CRC cache and the active theater archive group. All of
//! that state lives on the manager, so dropping or reconstructing it
//! mid-process silently discards process-sticky semantics. The slot makes the
//! lifecycle explicit:
//!
//! - `Available` — the manager is home; shell, audio, render, and capture
//!   borrow it in place.
//! - `Loading` — leased into the loading job; every loading outcome
//!   (success, failure, cancellation) must route the manager back through
//!   `return_from_loading`.
//! - Absent — startup could not open the retail archives; lookups no-op.
//!
//! A double return keeps the resident manager and logs instead of silently
//! replacing process-sticky state.

use crate::assets::asset_manager::AssetManager;

pub(crate) struct ProcessAssets {
    manager: Option<AssetManager>,
    leased: bool,
    /// One process-resident native Rules registry authority. Unlike the MIX
    /// manager it never leaves this owner: scenario transitions mutate it
    /// synchronously in place, so later failures cannot drop or roll it back.
    native_rules: Option<crate::rules::process_owner::NativeRulesProcessOwner>,
    /// Shell/read-only compatibility projection built once from the same
    /// startup-selected RULESMD/LANGRULE/ARTMD snapshots.
    startup_rules_projection: Option<crate::rules::ini_parser::IniFile>,
    /// MapClass's one process-global fallback CellClass (`0x00ABDC50`). Map
    /// reloads bind their resolved grid to this same live identity.
    pub(crate) shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummy,
    /// CSF string table — localized display names for units, buildings, UI
    /// text. Loaded once at startup from the retail archives; process-wide.
    pub(crate) csf: Option<crate::assets::csf_file::CsfFile>,
    /// Process-persistent terrain-load cache. Scenario teardown, failed
    /// loads, reseeds, and save transitions must not clear it.
    pub(crate) tile_variant_selector_cache:
        crate::map::tile_variant_selector::TileVariantSelectorCache,
}

impl ProcessAssets {
    pub(crate) fn from_startup(
        manager: Option<AssetManager>,
        csf: Option<crate::assets::csf_file::CsfFile>,
        native_rules: Option<crate::rules::process_owner::NativeRulesProcessOwner>,
        startup_rules_projection: Option<crate::rules::ini_parser::IniFile>,
    ) -> Self {
        Self {
            manager,
            leased: false,
            native_rules,
            startup_rules_projection,
            shared_cell_dummy: Default::default(),
            csf,
            tile_variant_selector_cache: Default::default(),
        }
    }

    pub(crate) fn startup_rules_projection(
        &self,
    ) -> Option<&crate::rules::ini_parser::IniFile> {
        self.startup_rules_projection.as_ref()
    }

    pub(crate) fn has_native_rules(&self) -> bool {
        self.native_rules.is_some()
    }

    /// Split the two process-resident mutable authorities used by a synchronous
    /// scenario load. The leased MIX manager lives in `LoadingJob`; the native
    /// Rules registry remains here so every `?` after its destructive reset
    /// preserves the actual live state.
    pub(crate) fn native_rules_mut_with_tile_cache(
        &mut self,
    ) -> (
        Option<&mut crate::rules::process_owner::NativeRulesProcessOwner>,
        &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    ) {
        (
            self.native_rules.as_mut(),
            &mut self.tile_variant_selector_cache,
        )
    }

    /// Shared borrow while the manager is home (`Available`).
    pub(crate) fn manager(&self) -> Option<&AssetManager> {
        self.manager.as_ref()
    }

    /// Field-level split borrow: the resident manager and the terrain-variant
    /// cache live on the same owner, and the RMG preview path needs both at
    /// once.
    pub(crate) fn manager_mut_with_tile_cache(
        &mut self,
    ) -> (
        Option<&mut AssetManager>,
        &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    ) {
        (self.manager.as_mut(), &mut self.tile_variant_selector_cache)
    }

    pub(crate) fn is_available(&self) -> bool {
        self.manager.is_some()
    }

    /// True while a lease is outstanding — distinguishes `Loading` (a manager
    /// existed and went out) from Absent (startup never constructed one), so
    /// the reconstruction path only warns about a real loss.
    pub(crate) fn is_leased(&self) -> bool {
        self.leased
    }

    /// `Available -> Loading`. Returns `None` when the slot is absent or the
    /// manager is already leased out.
    pub(crate) fn lease_for_loading(&mut self) -> Option<AssetManager> {
        let leased = self.manager.take();
        if leased.is_some() {
            self.leased = true;
        }
        leased
    }

    /// `Loading -> Available`. Every loading outcome must come back through
    /// here so the sticky CRC cache and active theater identity survive the
    /// process. A double return keeps the resident manager and logs.
    pub(crate) fn return_from_loading(&mut self, manager: AssetManager) {
        self.leased = false;
        if self.manager.is_some() {
            log::error!(
                "ProcessAssets: manager returned while one is already resident; \
                 keeping the resident manager (process-sticky state preserved)"
            );
            return;
        }
        self.manager = Some(manager);
    }

    /// A lease ended without a manager to return (the loading job lost it or
    /// reconstructed elsewhere). Clears the lease so the slot can lease again;
    /// the loss itself is the caller's anomaly to log.
    pub(crate) fn note_lease_ended_without_return(&mut self) {
        self.leased = false;
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessAssets;
    use crate::assets::asset_manager::AssetManager;

    fn test_manager(label: &str) -> AssetManager {
        let dir = std::env::temp_dir().join(format!(
            "vera20k-process-assets-{}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test asset dir");
        AssetManager::from_loose_root_for_test(&dir)
    }

    /// F11: the slot's lease/return transitions cover every loading outcome —
    /// success and failure/cancel both return the same manager, a double
    /// return keeps the resident one, and an absent slot leases nothing.
    #[test]
    fn asset_manager_lease_returns_on_success_failure_and_cancel() {
        // Absent slot: nothing to lease, nothing available.
        let mut absent = ProcessAssets::from_startup(None, None, None, None);
        assert!(!absent.is_available());
        assert!(absent.lease_for_loading().is_none());

        // Success-shaped cycle: lease out, manager comes home.
        let mut assets =
            ProcessAssets::from_startup(Some(test_manager("cycle")), None, None, None);
        assert!(assets.is_available());
        let leased = assets.lease_for_loading().expect("available manager leases");
        assert!(!assets.is_available(), "leased slot has no resident manager");
        assert!(
            assets.lease_for_loading().is_none(),
            "a leased slot cannot lease again"
        );
        assets.return_from_loading(leased);
        assert!(assets.is_available(), "success returns the manager");

        // Failure/cancel-shaped cycle: identical return path.
        let leased = assets.lease_for_loading().expect("re-lease after return");
        assets.return_from_loading(leased);
        assert!(assets.is_available(), "failure/cancel returns the manager");

        // Double return: the resident manager is kept, not replaced.
        assets.return_from_loading(test_manager("stray"));
        assert!(assets.is_available());

        // A lost lease clears the lease flag so the slot can lease again.
        let _ = assets.lease_for_loading().expect("lease");
        assets.note_lease_ended_without_return();
        assert!(!assets.is_available());
        assert!(assets.lease_for_loading().is_none(), "manager is truly gone");
    }

    #[test]
    fn gsi_04_01_process_owner_binds_grid_clones_and_map_reloads() {
        let assets = ProcessAssets::from_startup(None, None, None, None);
        let process_dummy = assets.shared_cell_dummy.clone();
        let mut first = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            0,
            0,
            Vec::new(),
        );
        first.bind_shared_cell_dummy(process_dummy.clone());
        let first_clone = first.clone();
        first.stamp_dummy_cell_requested_coord(7, 9);
        assert!(first
            .shared_cell_dummy()
            .same_identity(&first_clone.shared_cell_dummy()));
        assert_eq!(first_clone.dummy_cell_requested_coord(), (7, 9));

        process_dummy.set_level_slope(-7, 11);
        process_dummy.reconstruct_for_map_resize();
        let mut reloaded = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            0,
            0,
            Vec::new(),
        );
        reloaded.bind_shared_cell_dummy(assets.shared_cell_dummy.clone());
        assert!(reloaded
            .shared_cell_dummy()
            .same_identity(&first.shared_cell_dummy()));
        assert_eq!(reloaded.dummy_cell_requested_coord(), (0, 0));
        assert_eq!(reloaded.dummy_cell_level_slope(), (0, 0));

        let headless_process = crate::map::resolved_terrain::SharedCellDummy::fresh();
        assert!(!headless_process.same_identity(&assets.shared_cell_dummy));
        assert_eq!(headless_process.snapshot().coord, (0, 0));
    }
}
