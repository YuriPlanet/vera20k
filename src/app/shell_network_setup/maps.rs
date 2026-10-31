//! Shared network-map metadata, generated staging and verified-source admission.
//! Transports supply physical bytes; source/catalog/session remain authoritative
//! for decoding, effective rules, map identity and retained loading sources.

use crate::app::AppState;
use crate::map::source::{
    LoadedMapIni, LoadedMapSource, LoadedTransferAsset, TransferContent, TransferResource,
};
use anyhow::{Result, anyhow, ensure};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetMetadata {
    pub source_sha256: String,
    pub byte_length: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratedMapManifest {
    pub start_staging: String,
    pub preview: AssetMetadata,
}

/// The native W parser remains authoritative. Network admission also requires a
/// complete canonical table for the selected seed, before any start is ready.
pub(crate) fn checked_staging(
    text: &str,
    capacity: i32,
) -> Result<crate::map::preview::ScenarioStartStaging> {
    ensure!(text.len() <= 512, "generated start staging is too large");
    let staging = crate::map::preview::ScenarioStartStaging::parse_w_text(text)
        .ok_or_else(|| anyhow!("generated start staging is missing"))?;
    ensure!(
        staging.to_w_text() == text
            && (2..=8).contains(&staging.count)
            && staging.count == capacity
            && staging.width > 0
            && staging.height > 0
            && staging.cells[..staging.count as usize]
                .iter()
                .all(|cell| *cell != (0, 0)),
        "generated start staging differs from seed capacity"
    );
    Ok(staging)
}

pub(crate) fn host_generated_manifest(state: &AppState) -> Result<Option<GeneratedMapManifest>> {
    let Some(launch) = state.match_state.session.network_setup() else {
        return Ok(None);
    };
    if !launch
        .selected_map_file
        .as_deref()
        .is_some_and(|file| file.eq_ignore_ascii_case("RandMap.Sed"))
    {
        return Ok(None);
    }
    let capacity = state
        .match_state
        .session
        .network_map_metadata()
        .ok_or_else(|| anyhow!("seed metadata missing"))?
        .player_capacity;
    let text = state
        .frontend
        .multiplayer_shell_runtime
        .start_staging()
        .to_w_text();
    checked_staging(&text, capacity)?;
    let root = &state
        .platform
        .game_config
        .as_ref()
        .ok_or_else(|| anyhow!("no asset directory"))?
        .paths
        .ra2_dir;
    let source = crate::map::source::transfer_file_source(
        &root.join("RandMap.img"),
        TransferResource::Preview,
    )?;
    let (length, digest) = source.physical_identity().expect("physical preview");
    Ok(Some(GeneratedMapManifest {
        start_staging: text,
        preview: AssetMetadata {
            source_sha256: digest.to_owned(),
            byte_length: length,
        },
    }))
}

pub(crate) fn accept_generated_manifest(
    state: &mut AppState,
    previous: Option<&GeneratedMapManifest>,
    generated: Option<&GeneratedMapManifest>,
) -> Result<()> {
    let seed = state
        .match_state
        .session
        .network_setup()
        .and_then(|s| s.selected_map_file.as_deref())
        .is_some_and(|s| s.eq_ignore_ascii_case("RandMap.Sed"));
    ensure!(
        seed == generated.is_some(),
        "generated map manifest does not match selected map"
    );
    if let Some(manifest) = generated {
        let capacity = state
            .match_state
            .session
            .network_map_metadata()
            .ok_or_else(|| anyhow!("seed metadata missing"))?
            .player_capacity;
        let staging = checked_staging(&manifest.start_staging, capacity)?;
        ensure!(
            crate::map::source::valid_source_sha256(&manifest.preview.source_sha256)
                && manifest.preview.byte_length > 0
                && manifest.preview.byte_length <= TransferResource::Preview.limit(),
            "invalid generated preview identity"
        );
        // Existing Scenario+11C0 owner, also read by commit_start's staging token.
        state
            .frontend
            .multiplayer_shell_runtime
            .stage_generated_starts(staging);
    }
    if previous != generated {
        state.match_state.session.clear_network_random_map_preview();
    }
    Ok(())
}

pub(crate) fn staging_digest(state: &AppState) -> String {
    if state
        .match_state
        .session
        .network_setup()
        .and_then(|s| s.selected_map_file.as_deref())
        .is_some_and(|s| s.eq_ignore_ascii_case("RandMap.Sed"))
    {
        crate::util::sha256::sha256_hex(
            state
                .frontend
                .multiplayer_shell_runtime
                .start_staging()
                .to_w_text()
                .as_bytes(),
        )
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_prefetch_preserves_local_preview_and_retires_legacy_waits() {
        let dialog = crate::ui::multiplayer_setup::SetupDialog {
            host: false,
            flavor: crate::ui::multiplayer_setup::SetupFlavor::Lan,
            modern: false,
        };
        let mut local = super::super::NetworkSetupProtocol::new(dialog, false);
        local.local_map_verified = true;
        local.transfer_progress = Some((0, 100));
        local.awaiting_preview_file = true;
        local.preview_offer = Some(super::super::PreviewOffer {
            guests: vec!["Guest".into()],
            acknowledged: Vec::new(),
            preview: crate::map::preview::DecodedPreview {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            },
            deadline: std::time::Instant::now(),
        });
        local.accept_verified_map_source();
        assert!(
            local.local_preview(),
            "local catalog preview was incorrectly replaced by an absent received preview"
        );
        assert!(
            local.transfer_progress.is_none()
                && !local.awaiting_preview_file
                && local.preview_offer.is_none()
        );
        let mut received = super::super::NetworkSetupProtocol::new(dialog, false);
        received.accept_verified_map_source();
        assert!(
            !received.local_preview(),
            "a received physical source must not borrow a stale chooser thumbnail"
        );
    }

    #[test]
    fn unchanged_host_map_selection_retains_verified_preview_bounds() {
        use crate::app::match_runtime::session::{
            MatchSession, NetworkMapMetadata, NetworkReturnContext,
        };
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(fixture["launch"].clone()).unwrap();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch.clone(), true, None, false)
            .unwrap();
        let mut metadata = NetworkMapMetadata {
            display_name: "Authored map".into(),
            player_capacity: 8,
            official: false,
            byte_length: 1234,
            digest: "native digest".into(),
            source_sha256: "a".repeat(64),
            effective_digest: "b".repeat(64),
        };
        session
            .accept_network_map_metadata(metadata.clone(), None, true)
            .unwrap();
        let bounds = crate::map::preview::PreviewSourceBounds {
            origin_x: 200,
            origin_y: 10,
            width: 80,
            height: 90,
            start_points: vec![crate::map::preview::PreviewStartPoint { x: 220, y: 30 }],
        };
        let mut protocol = super::super::NetworkSetupProtocol::new(
            crate::ui::multiplayer_setup::SetupDialog {
                host: true,
                flavor: crate::ui::multiplayer_setup::SetupFlavor::Wol,
                modern: false,
            },
            false,
        );
        protocol.received_preview = Some((
            1,
            crate::map::preview::DecodedPreview {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            },
        ));
        protocol.accept_preview_bounds(Some(bounds.clone()));
        let mut selected = launch;
        selected.options.short_game = !selected.options.short_game;
        let changed = session
            .network_map_requires_verification(&selected, &metadata)
            .unwrap();
        assert!(!changed);
        protocol.invalidate_received_preview(changed);
        assert_eq!(protocol.received_preview_bounds(), Some(&bounds));
        assert!(
            protocol.received_preview().is_some(),
            "same-source Use Map lost the completed preview"
        );
        metadata.effective_digest = "c".repeat(64);
        let changed = session
            .network_map_requires_verification(&selected, &metadata)
            .unwrap();
        assert!(
            changed,
            "changed include closure must invalidate the image and markers"
        );
        protocol.invalidate_received_preview(changed);
        assert!(
            protocol.received_preview_bounds().is_none() && protocol.received_preview().is_none()
        );
    }

    #[test]
    fn generated_staging_requires_canonical_complete_seed_capacity() {
        let mut stage = crate::map::preview::ScenarioStartStaging {
            width: 80,
            height: 80,
            count: 2,
            ..Default::default()
        };
        stage.cells[0] = (20, 24);
        stage.cells[1] = (42, 46);
        assert!(checked_staging(&stage.to_w_text(), 2).is_ok());
        assert!(checked_staging(&stage.to_w_text(), 3).is_err());
        stage.cells[1] = (0, 0);
        assert!(checked_staging(&stage.to_w_text(), 2).is_err());
    }
}

pub(crate) fn local_transfer_source(
    state: &mut AppState,
    resource: TransferResource,
) -> Result<Option<LoadedMapSource>> {
    let config = state
        .platform
        .game_config
        .as_ref()
        .ok_or_else(|| anyhow!("no asset directory"))?;
    if resource == TransferResource::Preview {
        return if state.match_state.session.host() {
            Ok(Some(crate::map::source::transfer_file_source(
                &config.paths.ra2_dir.join("RandMap.img"),
                resource,
            )?))
        } else {
            Ok(None)
        };
    }
    let assets = state
        .process_assets
        .manager()
        .ok_or_else(|| anyhow!("assets missing"))?;
    let file = state
        .match_state
        .session
        .network_setup()
        .and_then(|setup| setup.selected_map_file.as_deref())
        .ok_or_else(|| anyhow!("selected map missing"))?;
    let metadata = state
        .match_state
        .session
        .network_map_metadata()
        .ok_or_else(|| anyhow!("map metadata missing"))?;
    let loaded = state.frontend.scenario_catalog.verify_network_map_input(
        file,
        metadata,
        &config.paths.ra2_dir,
        assets,
    );
    if resource == TransferResource::Map {
        if let Some(loaded) = &loaded {
            install_authored_preview(state, loaded)?;
        }
    }
    Ok(loaded.map(|loaded| loaded.source))
}

pub(crate) fn accept_transfer_asset(
    state: &mut AppState,
    loaded: LoadedTransferAsset,
    resource: TransferResource,
    generated: Option<&GeneratedMapManifest>,
) -> Result<()> {
    match loaded.content {
        TransferContent::Preview(image) => {
            ensure!(
                resource == TransferResource::Preview,
                "preview received as scenario"
            );
            let manifest = generated.ok_or_else(|| anyhow!("preview has no generated manifest"))?;
            ensure!(
                loaded.source.physical_identity()
                    == Some((
                        manifest.preview.byte_length,
                        manifest.preview.source_sha256.as_str()
                    )),
                "preview differs from the selected generated map"
            );
            let digest = state
                .match_state
                .session
                .network_map_metadata()
                .ok_or_else(|| anyhow!("seed selection ended"))?
                .source_sha256
                .clone();
            super::accept_network_random_map_preview(state, &digest, loaded.source, image)?;
        }
        TransferContent::Scenario(ini) => {
            ensure!(
                resource != TransferResource::Preview,
                "scenario received as preview"
            );
            let assets = state
                .process_assets
                .manager()
                .ok_or_else(|| anyhow!("assets missing"))?;
            let file = state
                .match_state
                .session
                .network_setup()
                .and_then(|setup| setup.selected_map_file.as_deref())
                .ok_or_else(|| anyhow!("selected map missing"))?;
            ensure!(
                (resource == TransferResource::Seed) == crate::map::rmg::is_seed_selection(file),
                "scenario resource differs from selected map kind"
            );
            let loaded = crate::map::source::resolve_network_map_ini(
                LoadedMapIni {
                    source: loaded.source,
                    ini,
                },
                assets,
                resource == TransferResource::Seed,
                file,
            )?;
            let expected = state
                .match_state
                .session
                .network_map_metadata()
                .ok_or_else(|| anyhow!("map selection ended"))?;
            ensure!(
                crate::app::scenario_catalog::ScenarioCatalog::verify_transferred_network_map(
                    &loaded,
                    expected,
                    resource == TransferResource::Seed
                ),
                "downloaded map differs"
            );
            let effective = expected.effective_digest.clone();
            if resource == TransferResource::Map {
                install_authored_preview(state, &loaded)?;
            }
            state
                .match_state
                .session
                .accept_verified_network_map_source(loaded.source, effective)?;
        }
    }
    Ok(())
}

/// A transferred authored map keeps derived marker bounds with its received
/// preview, without adding a chooser record. Header metadata uses its existing
/// reader; maps without a Header reuse the loading compositor's sole cell and
/// playfield projection over the admitted MapFile.
fn install_authored_preview(state: &mut AppState, loaded: &LoadedMapIni) -> Result<()> {
    let bounds = authored_preview_bounds(loaded)?;
    match crate::map::preview::decode_preview_image_from_ini(&loaded.ini) {
        Ok(Some(image)) => super::service::install_received_preview_image(state, image)?,
        Ok(None) => {}
        Err(error) => log::warn!("Received map preview did not decode: {error}"),
    }
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.accept_preview_bounds(bounds);
    }
    Ok(())
}

fn authored_preview_bounds(
    loaded: &LoadedMapIni,
) -> Result<Option<crate::map::preview::PreviewSourceBounds>> {
    if let Some(bounds) =
        crate::map::scenario_menu::preview_source_bounds_from_verified_source(&loaded.ini)
    {
        return Ok(Some(bounds));
    }
    let map = crate::map::map_file::MapFile::from_ini(loaded.ini.clone())?;
    let Some(bounds) = crate::app::loading::composition::projected_playfield_bounds(&map) else {
        return Ok(None);
    };
    if bounds.extent_x <= 0 || bounds.extent_y <= 0 {
        return Ok(None);
    }
    let capacity = crate::map::waypoints::skirmish_player_capacity(&loaded.ini);
    let starts =
        crate::map::waypoints::multiplayer_start_waypoints_for_capacity(&map.waypoints, capacity);
    let start_points = starts
        .into_iter()
        .map(|waypoint| {
            let point = crate::app::loading::composition::project_waypoint(waypoint);
            crate::map::preview::PreviewStartPoint {
                x: point.x,
                y: point.y,
            }
        })
        .collect();
    Ok(Some(crate::map::preview::PreviewSourceBounds {
        origin_x: bounds.min_x,
        origin_y: bounds.min_y,
        width: bounds.extent_x as u32,
        height: bounds.extent_y as u32,
        start_points,
    }))
}
