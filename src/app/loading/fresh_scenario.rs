//! Typed, consumed-once admission for gameplay-equivalent fresh scenario loads.
//!
//! Physical storage, startup provenance, fresh family, signed map format, and
//! generated materialization are independent facts.  This boundary proves
//! their supported combination before any Scenario cursor or load effect is
//! installed; persistence/restore deliberately has no conversion into it.

use crate::app::frontend::list_maps::LoadedMapSource;
use crate::app::shell_random_map::AcceptedRmgStartStaging;
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::OverlayLoadSource;
use crate::match_bootstrap::LoadingStartup;
use crate::sim::scenario_bootstrap::{
    MatchLaunchDescriptor, PreFillScenarioPrefixPlan, StockOfflinePrefixProjection,
    prepare_stock_offline_scenario_prefix_plan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshMapMaterialization {
    Authored,
    AcceptedGenerated,
}

impl FreshMapMaterialization {
    pub(crate) fn overlay_load_source(self) -> OverlayLoadSource {
        match self {
            Self::Authored => OverlayLoadSource::Authored,
            Self::AcceptedGenerated => OverlayLoadSource::GeneratedMaterialized,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshScenarioFamily {
    StockOffline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshStartupProvenance {
    Accepted,
    ResolvedLegacy,
}

/// Family-specific authority that advances the sole fresh Scenario cursor.
///
/// The enum and its contained plan are deliberately non-`Clone`: the cursor
/// can cross into the staged Simulation exactly once.
#[derive(Debug)]
enum FreshScenarioPrefixReceipt {
    StockOffline {
        launch: MatchLaunchDescriptor,
        scenario_prefix: PreFillScenarioPrefixPlan,
    },
}

/// Immutable description of one admitted gameplay-equivalent fresh load.
///
/// This owner is deliberately non-`Clone`.  Loading composition may borrow its
/// draw-free projection; terminal transfer consumes the descriptor and receipt.
#[derive(Debug)]
pub(crate) struct FreshScenarioLoadContextDescriptor {
    physical_source: LoadedMapSource,
    materialization: FreshMapMaterialization,
    signed_new_ini_format: i32,
    startup_provenance: FreshStartupProvenance,
    match_seed: u32,
    prefix: FreshScenarioPrefixReceipt,
}

#[derive(Debug)]
pub(crate) struct StockOfflineFreshScenarioParts {
    pub(crate) physical_source: LoadedMapSource,
    pub(crate) materialization: FreshMapMaterialization,
    pub(crate) signed_new_ini_format: i32,
    pub(crate) startup_provenance: FreshStartupProvenance,
    pub(crate) match_seed: u32,
    pub(crate) launch: MatchLaunchDescriptor,
    pub(crate) scenario_prefix: PreFillScenarioPrefixPlan,
}

impl FreshScenarioLoadContextDescriptor {
    /// Admit the one currently supported fresh family.  This is intentionally
    /// visible only inside the loading owner: no caller can fabricate the
    /// generated arm without surrendering accepted setup staging here.
    ///
    /// gamemd provenance: `Read_Scenario @ 0x00684620` selects the concrete
    /// authored or generated input, `Read_Scenario_INI @ 0x00686730` enters the
    /// fresh reader, and `ScenarioClass::Full_Init @ 0x00686B20` owns the
    /// family-specific prefix. `ScenarioClass::Read_INI_Basic` stores signed
    /// `NewINIFormat` at `0x0068A156`; only the later pack bodies interpret it.
    pub(super) fn admit_stock_offline(
        startup: &LoadingStartup,
        map: &MapFile,
        physical_source: &LoadedMapSource,
        accepted_rmg_start_staging: &mut Option<AcceptedRmgStartStaging>,
    ) -> anyhow::Result<Self> {
        if matches!(physical_source, LoadedMapSource::LegacyFallback { .. }) {
            anyhow::bail!(
                "fresh scenario loading requires an exact Loose, MIX, or accepted generated source"
            );
        }
        let (session, match_seed, startup_provenance) = match startup {
            LoadingStartup::Accepted(prepared) => (
                prepared.session.launch_session().clone(),
                prepared.seed.value,
                FreshStartupProvenance::Accepted,
            ),
            LoadingStartup::UnverifiedLegacy { session, seed } => (
                session.clone(),
                seed.value,
                FreshStartupProvenance::ResolvedLegacy,
            ),
            LoadingStartup::Generic { .. } => {
                anyhow::bail!("Generic startup cannot enter a typed fresh scenario load")
            }
        };
        let launch = MatchLaunchDescriptor::from_resolved(session.clone())
            .map_err(|err| anyhow::anyhow!("fresh stock-offline launch is unresolved: {err}"))?;
        let selected_map = session
            .selected_map_file
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("stock offline launch has no selected map"))?;
        let selected_map_trimmed = selected_map.trim();
        if selected_map_trimmed.is_empty()
            || selected_map_trimmed.len() != selected_map.len()
            || selected_map_trimmed.eq_ignore_ascii_case("auto")
        {
            anyhow::bail!("stock offline launch has no exact selected map record");
        }
        // Validate the complete stock callback row before the accepted staging
        // token can move. Numeric mode IDs alone are not sufficient authority.
        crate::sim::scenario_bootstrap::stock_offline_start_callback_family(&session)?;
        let (materialization, staged_waypoints) = match physical_source {
            LoadedMapSource::Generated { seed_name } => {
                if !crate::map::rmg::is_seed_selection(selected_map)
                    || !crate::map::rmg::is_seed_selection(seed_name)
                {
                    anyhow::bail!(
                        "generated fresh source and selected record must both be terminal .SED names"
                    );
                }
                if !seed_name.eq_ignore_ascii_case(selected_map) {
                    anyhow::bail!(
                        "generated source {seed_name:?} does not match selected record {selected_map:?}"
                    );
                }
                if !session.mode.random_maps_allowed || !matches!(session.mode.id, 1 | 2) {
                    anyhow::bail!(
                        "accepted random-map staging is unsupported for stock mode id {}",
                        session.mode.id
                    );
                }
                let waypoints = accepted_rmg_start_staging
                    .as_ref()
                    .ok_or_else(|| {
                        anyhow::anyhow!("generated launch has no accepted setup start staging")
                    })?
                    .waypoint_table_for_admission();
                (FreshMapMaterialization::AcceptedGenerated, Some(waypoints))
            }
            LoadedMapSource::Loose { .. } | LoadedMapSource::Mix { .. } => {
                if crate::map::rmg::is_seed_selection(selected_map) {
                    anyhow::bail!("authored physical source cannot satisfy a selected .SED record");
                }
                if accepted_rmg_start_staging.is_some() {
                    anyhow::bail!(
                        "accepted random-map staging cannot attach to an authored map source"
                    );
                }
                (FreshMapMaterialization::Authored, None)
            }
            LoadedMapSource::LegacyFallback { .. } => anyhow::bail!(
                "fresh scenario loading requires an exact Loose, MIX, or accepted generated source"
            ),
        };
        let start_waypoints = staged_waypoints.as_ref().unwrap_or(&map.waypoints);
        let scenario_prefix =
            prepare_stock_offline_scenario_prefix_plan(&launch, map, start_waypoints, match_seed)?;
        if materialization == FreshMapMaterialization::AcceptedGenerated {
            let consumed_waypoints = accepted_rmg_start_staging
                .take()
                .expect("validated accepted staging remains present")
                .into_waypoint_table();
            debug_assert_eq!(
                consumed_waypoints,
                staged_waypoints.expect("generated waypoints")
            );
        }
        Ok(Self {
            physical_source: physical_source.clone(),
            materialization,
            signed_new_ini_format: map.basic.new_ini_format.unwrap_or(0),
            startup_provenance,
            match_seed,
            prefix: FreshScenarioPrefixReceipt::StockOffline {
                launch,
                scenario_prefix,
            },
        })
    }

    #[cfg(test)]
    pub(crate) fn physical_source(&self) -> &LoadedMapSource {
        &self.physical_source
    }

    #[cfg(test)]
    pub(crate) fn materialization(&self) -> FreshMapMaterialization {
        self.materialization
    }

    #[cfg(test)]
    pub(crate) fn signed_new_ini_format(&self) -> i32 {
        self.signed_new_ini_format
    }

    #[cfg(test)]
    pub(crate) fn match_seed(&self) -> u32 {
        self.match_seed
    }

    #[cfg(test)]
    pub(crate) fn authored_pack_bodies_enabled(&self) -> bool {
        self.materialization == FreshMapMaterialization::Authored && self.signed_new_ini_format > 1
    }

    #[cfg(test)]
    pub(crate) fn startup_provenance(&self) -> FreshStartupProvenance {
        self.startup_provenance
    }

    #[cfg(test)]
    pub(crate) fn family(&self) -> FreshScenarioFamily {
        match &self.prefix {
            FreshScenarioPrefixReceipt::StockOffline { .. } => FreshScenarioFamily::StockOffline,
        }
    }

    pub(crate) fn stock_offline_launch(&self) -> &MatchLaunchDescriptor {
        match &self.prefix {
            FreshScenarioPrefixReceipt::StockOffline { launch, .. } => launch,
        }
    }

    pub(crate) fn stock_offline_projection(&self) -> &StockOfflinePrefixProjection {
        match &self.prefix {
            FreshScenarioPrefixReceipt::StockOffline {
                scenario_prefix, ..
            } => scenario_prefix.projection(),
        }
    }

    /// Recheck the two independently moved terminal owners before any load
    /// effect.  This catches accidental request/context pairing without
    /// re-normalizing or cloning the family receipt.
    pub(crate) fn validate_terminal_transfer(
        &self,
        startup: &LoadingStartup,
        source: &LoadedMapSource,
        signed_new_ini_format: i32,
    ) -> anyhow::Result<()> {
        if &self.physical_source != source {
            anyhow::bail!(
                "fresh descriptor source {:?} disagrees with terminal source {source:?}",
                self.physical_source
            );
        }
        if self.signed_new_ini_format != signed_new_ini_format {
            anyhow::bail!(
                "fresh descriptor NewINIFormat {} disagrees with terminal map value {signed_new_ini_format}",
                self.signed_new_ini_format
            );
        }
        let (provenance, seed, session) = match startup {
            LoadingStartup::Accepted(prepared) => (
                FreshStartupProvenance::Accepted,
                prepared.seed.value,
                prepared.session.launch_session(),
            ),
            LoadingStartup::UnverifiedLegacy { session, seed } => {
                (FreshStartupProvenance::ResolvedLegacy, seed.value, session)
            }
            LoadingStartup::Generic { .. } => {
                anyhow::bail!("Generic startup cannot enter a typed fresh scenario load")
            }
        };
        if provenance != self.startup_provenance {
            anyhow::bail!(
                "fresh descriptor startup provenance {:?} disagrees with terminal startup {provenance:?}",
                self.startup_provenance
            );
        }
        if seed != self.match_seed {
            anyhow::bail!(
                "fresh descriptor seed 0x{:08X} disagrees with terminal seed 0x{seed:08X}",
                self.match_seed
            );
        }
        if session != self.stock_offline_launch().session() {
            anyhow::bail!("fresh descriptor launch session changed before terminal transfer");
        }
        Ok(())
    }

    pub(crate) fn into_stock_offline_parts(self) -> StockOfflineFreshScenarioParts {
        let FreshScenarioPrefixReceipt::StockOffline {
            launch,
            scenario_prefix,
        } = self.prefix;
        StockOfflineFreshScenarioParts {
            physical_source: self.physical_source,
            materialization: self.materialization,
            signed_new_ini_format: self.signed_new_ini_format,
            startup_provenance: self.startup_provenance,
            match_seed: self.match_seed,
            launch,
            scenario_prefix,
        }
    }
}
