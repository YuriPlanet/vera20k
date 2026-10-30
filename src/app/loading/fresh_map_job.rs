//! Main-thread continuations for the existing fresh-map funnel. Each slice
//! borrows the same process authorities anew; suspended slices retain only
//! their owned scenario construction data, never references into AppState.

use super::*;

pub(crate) struct FreshMapLoadServices<'a> {
    pub(crate) gpu: &'a GpuContext,
    pub(crate) batch: &'a BatchRenderer,
    pub(crate) asset_manager: &'a mut AssetManager,
    pub(crate) native_rules_owner: &'a mut crate::rules::process_owner::NativeRulesProcessOwner,
    pub(crate) the_team_enabled: bool,
    #[cfg(feature = "ares")]
    pub(crate) prepared_noncampaign_prefix:
        &'a mut Option<crate::rules::process_owner::NativeNonCampaignPrefixPreparation>,
    pub(crate) audio: Option<&'a crate::app::process_assets::ProcessAudioCatalog>,
    pub(crate) themes: Option<&'a crate::rules::theme_ini::ThemeCatalog>,
    pub(crate) shared_cell_dummy: crate::map::resolved_terrain::SharedCellDummy,
    pub(crate) tile_variant_selector_cache:
        &'a mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    pub(crate) progress: &'a mut dyn crate::app::loading::pump::LoadingProgressSink,
    /// `[VERA] HouseColorStrength=`: how strongly this battle's `[Colors]`
    /// team-color bands are boosted. Presentation only, read once while the
    /// world's colours are built, so the atlas, the voxel ramp texture, the
    /// radar dots and the target lines all share one table.
    pub(crate) house_color_strength: u8,
}

pub(super) struct FreshMapLoadContinuation {
    label: &'static str,
    run: Box<dyn for<'a> FnOnce(FreshMapLoadServices<'a>) -> Result<FreshMapLoadStep>>,
}

pub(super) enum FreshMapLoadStep {
    Continue(FreshMapLoadContinuation),
    Complete(Box<MapLoadResult>),
}

impl FreshMapLoadContinuation {
    pub(super) fn new(
        label: &'static str,
        run: impl for<'a> FnOnce(FreshMapLoadServices<'a>) -> Result<FreshMapLoadStep> + 'static,
    ) -> Self {
        Self {
            label,
            run: Box::new(run),
        }
    }
}

pub(crate) struct FreshMapLoadJob {
    /// Immutable observations for loading art/failure routing. The consuming
    /// startup and source remain in the single continuation; these copies
    /// neither construct a world nor admit another runtime.
    startup: LoadingStartup,
    map_source: LoadedMapSource,
    theater_cache_mismatch: bool,
    next: Option<FreshMapLoadContinuation>,
}

impl FreshMapLoadJob {
    pub(crate) fn new(
        initial: MapLoadInitial,
        startup: LoadingStartup,
        context: FreshScenarioLoadContextDescriptor,
        theater_cache_mismatch: bool,
        runtime_color_scheme_count: usize,
    ) -> Self {
        let map_source = initial.map_source().clone();
        Self {
            startup: startup.clone(),
            map_source,
            theater_cache_mismatch,
            next: Some(FreshMapLoadContinuation::new("theater", move |services| {
                super::load_map_first_step(
                    services,
                    initial,
                    startup,
                    context,
                    theater_cache_mismatch,
                    runtime_color_scheme_count,
                )
            })),
        }
    }

    pub(crate) fn startup(&self) -> &LoadingStartup {
        &self.startup
    }
    pub(crate) fn map_source(&self) -> &LoadedMapSource {
        &self.map_source
    }
    pub(crate) fn theater_cache_mismatch(&self) -> bool {
        self.theater_cache_mismatch
    }

    /// A slice ends only after a complete native operation; in particular,
    /// Rules load_scenario restores its registry before any event-loop yield.
    /// Callers cannot resume a failed or completed transaction a second time.
    pub(crate) fn step(
        &mut self,
        services: FreshMapLoadServices<'_>,
    ) -> Result<Option<MapLoadResult>> {
        let continuation = self
            .next
            .take()
            .ok_or_else(|| anyhow::anyhow!("fresh-map continuation already consumed"))?;
        log::debug!(target: "vera20k::loading_attempt", "phase_begin {}", continuation.label);
        match (continuation.run)(services)? {
            FreshMapLoadStep::Continue(next) => {
                self.next = Some(next);
                Ok(None)
            }
            FreshMapLoadStep::Complete(result) => Ok(Some(*result)),
        }
    }
}
