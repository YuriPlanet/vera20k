//! Scenario catalog (F11): one owner for the skirmish scenario records and
//! the shell map entries projected from them.
//!
//! Before this owner, `skirmish_scenario_records` and `skirmish_shell_maps`
//! were two parallel `AppState` vectors reconciled by hand: the random-map
//! flow mutated the records and then patched the projected list separately by
//! name-position, and nothing else re-projected — any future record mutation
//! silently broke the projection. Here the projection is derived state: reads
//! are borrows, and the only mutable access re-projects when it drops, so the
//! two lists cannot drift.

use crate::app::loading::init::MapMenuEntry;
use crate::map::skirmish_scenarios::SkirmishScenarioRecord;

pub(crate) struct ScenarioCatalog {
    records: Vec<SkirmishScenarioRecord>,
    /// Always exactly `records.iter().map(to_map_menu_entry)`.
    shell_maps: Vec<MapMenuEntry>,
}

impl ScenarioCatalog {
    pub(crate) fn from_records(records: Vec<SkirmishScenarioRecord>) -> Self {
        let mut catalog = Self {
            records,
            shell_maps: Vec::new(),
        };
        catalog.reproject();
        catalog
    }

    pub(crate) fn records(&self) -> &[SkirmishScenarioRecord] {
        &self.records
    }

    pub(crate) fn shell_maps(&self) -> &[MapMenuEntry] {
        &self.shell_maps
    }

    /// Mutable access to the records. The guard re-projects the shell map
    /// entries when it drops, so every mutation path — including the modal's
    /// random-map sentinel upsert — keeps the projection exact.
    pub(crate) fn records_mut(&mut self) -> RecordsMut<'_> {
        RecordsMut { catalog: self }
    }

    fn reproject(&mut self) {
        self.shell_maps = self
            .records
            .iter()
            .map(SkirmishScenarioRecord::to_map_menu_entry)
            .collect();
    }
}

pub(crate) struct RecordsMut<'a> {
    catalog: &'a mut ScenarioCatalog,
}

impl std::ops::Deref for RecordsMut<'_> {
    type Target = Vec<SkirmishScenarioRecord>;
    fn deref(&self) -> &Self::Target {
        &self.catalog.records
    }
}

impl std::ops::DerefMut for RecordsMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.catalog.records
    }
}

impl Drop for RecordsMut<'_> {
    fn drop(&mut self) {
        self.catalog.reproject();
    }
}

#[cfg(test)]
mod tests {
    use super::ScenarioCatalog;
    use crate::map::skirmish_scenarios::upsert_random_map_sentinel;

    /// F11 design test: the shell map projection cannot drift from the
    /// records — every mutation path re-projects, so indices into the
    /// records and the projected entries always describe the same scenario.
    #[test]
    fn scenario_catalog_indices_cannot_drift() {
        // Records from the production scan when a retail install is present;
        // the projection invariant below holds for an empty list too.
        let records = crate::util::config::GameConfig::load()
            .ok()
            .and_then(|config| {
                let mut assets =
                    crate::assets::asset_manager::AssetManager::new(&config.paths.ra2_dir).ok()?;
                crate::app::frontend::list_maps::list_skirmish_scenario_records_with_assets(
                    &config.paths.ra2_dir,
                    &mut assets,
                    None,
                )
                .ok()
            })
            .unwrap_or_default();
        let mut catalog = ScenarioCatalog::from_records(records);
        let assert_projection = |catalog: &ScenarioCatalog| {
            assert_eq!(catalog.records().len(), catalog.shell_maps().len());
            for (record, entry) in catalog.records().iter().zip(catalog.shell_maps()) {
                assert_eq!(record.file_name, entry.file_name);
                assert_eq!(record.display_name, entry.display_name);
            }
        };
        assert_projection(&catalog);

        // Mutating through the guard (the random-map sentinel upsert the
        // modal performs) re-projects on drop — twice, to cover both the
        // append and in-place-refresh shapes.
        let index = {
            let mut records = catalog.records_mut();
            upsert_random_map_sentinel(&mut records, "Random Map", 2)
        };
        assert_projection(&catalog);
        assert_eq!(
            catalog.shell_maps()[index].display_name,
            catalog.records()[index].display_name
        );

        let refreshed = {
            let mut records = catalog.records_mut();
            upsert_random_map_sentinel(&mut records, "Random Map (4)", 4)
        };
        assert_eq!(index, refreshed, "sentinel refreshes in place");
        assert_projection(&catalog);
        assert_eq!(
            catalog.shell_maps()[refreshed].display_name,
            "Random Map (4)"
        );
    }
}
