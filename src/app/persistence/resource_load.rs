//! Incoming resource preparation for the shared saved-game restore.
//!
//! Native LoadContent67E730 restores Rules675210 and Mouse5BDF70 before
//! hierarchy/postload. Saved VERA records carry these existing owners' inputs;
//! they never replay Full_Init, INI layering, map objects or AI installation.

use super::{CommittedLoadPresentation, PreparedLoad, PreparedLoadError, SaveRepository};
use crate::app::input::state::PreparedSavedLocalView;
use crate::app::loading::{init, init_helpers};
use crate::app::process_assets::ProcessAssets;
use crate::assets::asset_manager::AssetManager;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, SharedCellDummy};
use crate::map::terrain::{TerrainGrid, build_terrain_grid_from_resolved};
use crate::map::theater::{self, BridgeMiddleTiles};
use crate::map::tile_variant_selector::TileVariantSelectorCache;
use crate::rules::native_processing::ProcessedRulesLayers;
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::sim::rng::{MainRng, MainRngDraws};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::scenario_resources::SavedMapMetadata;
use crate::sim::snapshot::{GameSnapshot, SnapshotError};
use crate::sim::world::{Simulation, SnapshotProcessState};
use crate::ui::sidebar::power_bar_anim::PowerBarAnimState;

/// Borrowed preparation capabilities. Outgoing app presentation/input is not
/// available to the fallible candidate; both menu and game supply this view.
pub(crate) struct SavedGamePreparationView<'a> {
    pub(crate) repository: &'a SaveRepository,
    pub(crate) current_simulation: Option<&'a Simulation>,
    pub(crate) frontend_main: &'a MainRng,
    pub(crate) rules_owner: &'a NativeRulesProcessOwner,
    pub(crate) tile_cache: &'a TileVariantSelectorCache,
    pub(crate) load_clock: &'a dyn Fn() -> u64,
}

pub(crate) struct PreparedSavedGame {
    candidate: PreparedSavedResources,
    presentation: init::PresentationLoadAssets,
    terrain_grid: Option<TerrainGrid>,
}

/// Move-only candidate before GPU allocation. Production and the connected
/// retail restore tests use this exact preparation phase.
struct PreparedSavedResources {
    world: PreparedLoad,
    resources: SimResources,
    processed_rules: ProcessedRulesLayers,
    map: SavedMapMetadata,
    local_view: PreparedSavedLocalView,
    power_bar_anim: PowerBarAnimState,
    tile_cache: TileVariantSelectorCache,
    theater: theater::TheaterData,
    elapsed_clock: crate::app::match_runtime::frame_pacer::ScenarioElapsedClock,
}

pub(crate) struct CommittedSavedGame {
    pub(crate) map: SavedMapMetadata,
    pub(crate) presentation: init::PresentationLoadAssets,
    pub(crate) terrain_grid: Option<TerrainGrid>,
    pub(crate) local_view: PreparedSavedLocalView,
    pub(crate) power_bar_anim: PowerBarAnimState,
    pub(crate) details: CommittedLoadPresentation,
    pub(crate) elapsed_clock: crate::app::match_runtime::frame_pacer::ScenarioElapsedClock,
}

impl PreparedSavedGame {
    pub(crate) fn from_repository(
        view: SavedGamePreparationView<'_>,
        assets: &mut AssetManager,
        path: &std::path::Path,
        gpu: &crate::render::gpu::GpuContext,
        batch: &crate::render::batch::BatchRenderer,
        detail_level: u32,
    ) -> Result<Self, PreparedLoadError> {
        let bytes = view
            .repository
            .read(path)
            .map_err(PreparedLoadError::ReadFile)?;
        assets.prepare_saved_load(|assets| {
            Self::prepare(&bytes, &view, assets, gpu, batch, detail_level)
        })
    }

    fn prepare_resources(
        bytes: &[u8],
        view: &SavedGamePreparationView<'_>,
        assets: &mut AssetManager,
    ) -> Result<PreparedSavedResources, PreparedLoadError> {
        let mut snapshot = GameSnapshot::load_with_resources(bytes)?;
        let saved = snapshot
            .take_scenario_resources()
            .ok_or(SnapshotError::MissingScenarioResources)?;
        let elapsed_clock =
            crate::app::match_runtime::frame_pacer::ScenarioElapsedClock::from_saved_accumulator(
                saved.map().elapsed_buckets(),
                (view.load_clock)(),
            );
        let mut saved_view = snapshot
            .take_local_view()
            .ok_or(SnapshotError::MissingLocalView)?;
        let power_bar_anim = PowerBarAnimState::from_saved(saved_view.take_power_bar().ok_or(
            crate::sim::snapshot::saved_local_view::SavedLocalViewError::MissingPowerBar,
        )?)?;
        if saved.fixed_art_hash() != view.rules_owner.fixed_art().content_hash() {
            return Err(PreparedLoadError::Resources(
                "fixed ARTMD differs from the saved game".into(),
            ));
        }
        saved.terrain().validate()?;
        if saved.terrain().dimensions()
            != (
                snapshot.sim.session().map_width(),
                snapshot.sim.session().map_height(),
            )
        {
            return Err(PreparedLoadError::Resources(
                "saved terrain and scenario dimensions disagree".into(),
            ));
        }
        let theater_name = snapshot.sim.session().theater().to_owned();
        let theater = theater::load_theater(assets, &theater_name).ok_or_else(|| {
            PreparedLoadError::Resources(format!("cannot load theater {theater_name}"))
        })?;
        let mut rules = view
            .rules_owner
            .project_saved_scenario_rules(saved.processed_rules())?;
        init_helpers::bind_rules_art_metadata(
            &mut rules,
            view.rules_owner.fixed_art(),
            assets,
            theater.extension,
            &theater_name,
        );
        init_helpers::bind_rules_asset_closure(
            &mut rules,
            saved.processed_rules().ini(),
            view.rules_owner.fixed_art(),
            saved.scheduler_anim_roots(),
            assets,
            theater.extension,
            &theater_name,
        )
        .map_err(|error| PreparedLoadError::Resources(error.to_string()))?;
        // Asset-dependent simulation metadata participates in this digest.
        // Compare after binding, not against the outgoing world's rules.
        let expected = rules.simulation_config_hash();
        if snapshot.rules_hash != expected {
            return Err(SnapshotError::RulesMismatch {
                expected,
                found: snapshot.rules_hash,
            }
            .into());
        }
        let overlay_registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
            saved.processed_rules().ini(),
            Some(view.rules_owner.fixed_art()),
        );
        let (mut resources, processed_rules, map) =
            saved.into_restored_parts(rules, overlay_registry, SharedCellDummy::fresh())?;
        let rules = &resources.rules;
        let overlay_types = &resources.overlay_registry;
        let terrain_speed =
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::from_general(
                rules.general.tracked_uphill,
                rules.general.tracked_downhill,
                rules.general.wheeled_uphill,
                rules.general.wheeled_downhill,
            );
        let mut tile_cache = view.tile_cache.detached();
        let mut bind_loaded_cells =
            |terrain: &mut ResolvedTerrainGrid, main: MainRngDraws, size_width: i32| {
                let mut draw = || main.next_u32();
                let mut selector = tile_cache.begin_load(&mut draw);
                terrain
                    .finish_loaded_assets(
                        &theater,
                        assets,
                        &rules.terrain_rules,
                        overlay_types,
                        true,
                        rules.general.cliff_back_impassability,
                        size_width,
                        &mut selector,
                    )
                    .map_err(|error| error.to_string())
            };
        let process = view.current_simulation.map_or(
            SnapshotProcessState::Cold(view.frontend_main),
            SnapshotProcessState::Live,
        );
        let world = PreparedLoad::prepare_snapshot(
            snapshot,
            process,
            rules,
            resources
                .terrain_template
                .as_ref()
                .expect("saved terrain restored")
                .clone(),
            terrain_speed,
            overlay_types,
            Some(&mut bind_loaded_cells),
        )?;
        resources
            .terrain_template
            .as_mut()
            .expect("saved terrain restored")
            .bind_saved_template_assets(
                &theater,
                assets,
                &rules.terrain_rules,
                overlay_types,
                true,
                rules.general.cliff_back_impassability,
                tile_cache.selector(),
            )?;
        // MouseLoad reads a different theater through 545150 (5BE647), which
        // republishes ClearTile/WaterSet at 545D27/545E17 for a later map's
        // Fill; the same theater keeps the values its own load published.
        // Like the fresh loader's, this reaches the process cache on commit.
        tile_cache.complete_theater_registry_load(
            theater.rmg_tiles.clear_tile,
            theater.rmg_tiles.water_set,
        );
        let local_view = PreparedSavedLocalView::new(saved_view, &world.simulation, rules)?;
        Ok(PreparedSavedResources {
            world,
            resources,
            processed_rules,
            map,
            local_view,
            power_bar_anim,
            tile_cache,
            theater,
            elapsed_clock,
        })
    }

    fn prepare(
        bytes: &[u8],
        view: &SavedGamePreparationView<'_>,
        assets: &mut AssetManager,
        gpu: &crate::render::gpu::GpuContext,
        batch: &crate::render::batch::BatchRenderer,
        detail_level: u32,
    ) -> Result<Self, PreparedLoadError> {
        let candidate = Self::prepare_resources(bytes, view, assets)?;
        let PreparedSavedResources {
            world,
            resources,
            processed_rules,
            map,
            theater,
            ..
        } = &candidate;
        let rules = &resources.rules;
        let theater_name = &world.simulation.session().theater();
        let terrain = world
            .simulation
            .resolved_terrain()
            .expect("prepared world terrain");
        let terrain_grid =
            build_terrain_grid_from_resolved(terrain, BridgeMiddleTiles::from_theater(theater));
        let csf = Some(
            init::load_csf(assets)
                .map_err(|error| PreparedLoadError::Resources(error.to_string()))?,
        );
        let tile_atlas = init_helpers::build_tile_atlas(
            assets,
            &theater.lookup,
            &theater.iso_palette,
            theater.extension,
            &terrain_grid,
            gpu,
            batch,
        );
        let manifest = init_helpers::build_presentation_manifest(
            &world.simulation,
            assets,
            gpu,
            batch,
            theater.extension,
            theater_name,
            Some(rules),
            &resources.overlay_registry,
            map.house_colors(),
            Some(&theater.unit_palette),
            Some(&theater.iso_palette),
        );
        let overlays = crate::app::frontend::skirmish::build_overlay_presentation(
            &world.map_restore.occupied_overlays,
            map.terrain_objects(),
            assets,
            gpu,
            batch,
            theater.extension,
            theater_name,
            processed_rules.ini(),
            rules,
            &resources.overlay_registry,
            Some(&theater.iso_palette),
            Some(&theater.unit_palette),
            Some(&theater.tiberium_palette),
        );
        let presentation = init_helpers::finish_presentation_assets(
            gpu,
            batch,
            assets,
            &world.simulation,
            rules,
            terrain,
            map.lighting(),
            detail_level,
            tile_atlas,
            manifest,
            overlays,
            map.house_colors().clone(),
            csf,
        )
        .map_err(|error| PreparedLoadError::Resources(error.to_string()))?;
        Ok(Self {
            candidate,
            presentation,
            terrain_grid: Some(terrain_grid),
        })
    }

    pub(crate) fn native_tiberium_stats(
        &self,
    ) -> crate::sim::ore_growth::NativeTiberiumRebuildStats {
        self.candidate.world.native_tiberium_stats()
    }

    /// Synchronous commit follows preparation without any intervening process
    /// mutation. Retain the live cache/dummy identities and rebind both grids;
    /// no outgoing rules, map cache or UI projection survives by accident.
    pub(crate) fn commit_into(
        self,
        process: &mut ProcessAssets,
        process_main: &MainRng,
        runtime: &mut Option<SimRuntime>,
    ) -> CommittedSavedGame {
        let PreparedSavedResources {
            world,
            mut resources,
            processed_rules,
            map,
            local_view,
            power_bar_anim,
            tile_cache,
            elapsed_clock,
            ..
        } = self.candidate;
        let dummy = process.shared_cell_dummy.clone();
        let (owner, cache, _) = process.native_rules_mut_with_tile_cache();
        cache
            .adopt_prepared(tile_cache)
            .expect("synchronous saved-load selector adoption");
        owner
            .expect("prepared process Rules owner")
            .install_saved_scenario_rules(processed_rules);
        let selector = cache.selector();
        let (mut simulation, details) = world.into_committed(dummy.clone(), process_main);
        simulation.bind_tile_variant_selector(selector.clone());
        let template = resources
            .terrain_template
            .as_mut()
            .expect("prepared saved terrain");
        template.bind_shared_cell_dummy(dummy);
        template.bind_tile_variant_selector(selector);
        *runtime = Some(SimRuntime {
            simulation,
            resources,
        });
        CommittedSavedGame {
            map,
            presentation: self.presentation,
            terrain_grid: self.terrain_grid,
            local_view,
            power_bar_anim,
            details,
            elapsed_clock,
        }
    }
}

#[cfg(test)]
#[path = "resource_load_tests.rs"]
mod tests;
