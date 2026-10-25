//! One build/features and resolved selected-mode rules agreement for both
//! multiplayer adapters. ProcessAssets retains the actual source/digest owner.

use crate::app::AppState;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MultiplayerAgreement {
    pub version: String,
    pub rules_digest: String,
}

pub(crate) fn version() -> String {
    format!(
        "{}:{}:{}{}{}",
        env!("CARGO_PKG_VERSION"),
        env!("VERA20K_NETWORK_BUILD"),
        u8::from(crate::rules::ARES_ENABLED),
        u8::from(crate::rules::PHOBOS_ENABLED),
        u8::from(crate::rules::NPATCH_ENABLED)
    )
}

pub(crate) fn local(state: &mut AppState) -> Result<MultiplayerAgreement> {
    let setup = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow!("no setup"))?;
    let file = setup
        .selected_map_file
        .as_deref()
        .ok_or_else(|| anyhow!("no selected map"))?;
    let mode = setup.mode.override_file.clone();
    let seed_options = if crate::map::rmg::is_seed_selection(file) {
        let root = &state
            .platform
            .game_config
            .as_ref()
            .ok_or_else(|| anyhow!("no asset directory"))?
            .paths
            .ra2_dir;
        let source_file = state
            .match_state
            .session
            .network_local_map_source()
            .and_then(|source| match source {
                crate::map::source::LoadedMapSource::Loose { path, .. } => path.to_str(),
                _ => None,
            })
            .unwrap_or(file);
        let assets = state
            .process_assets
            .manager()
            .ok_or_else(|| anyhow!("assets missing"))?;
        crate::map::source::load_network_map_ini(root, source_file, assets)
            .ok()
            .map(|loaded| {
                let mut options = crate::map::rmg::RmgOptions::default();
                options.apply_sed(&loaded.ini);
                options.normalize();
                options
            })
    } else {
        None
    };
    let digest = if let Some(options) = seed_options.as_ref() {
        state
            .process_assets
            .multiplayer_generated_rules_digest(&mode, options)
    } else {
        state.process_assets.multiplayer_rules_digest(&mode)
    }
    .map_err(anyhow::Error::msg)?;
    Ok(MultiplayerAgreement {
        version: version(),
        rules_digest: digest,
    })
}
