//! Presentation of a frozen authored LAN start. Mode-0 artwork and briefing
//! chrome are shared with the campaign loader.

use crate::app::AppState;
use crate::app::match_bootstrap::PreparedNetworkStartup;
use crate::assets::asset_manager::AssetManager;
use crate::render::loading_screen_chrome::LoadingScreenWidth;
use crate::rules::campaign_loading::CampaignLoadingMetadata;
use crate::rules::campaigns::CampaignRegistry;
use crate::rules::ini_parser::IniFile;
use crate::ui::shell::geom::ShellScale;

use super::campaign_presentation::CampaignLoadingPresentation;

/// Read the installed mission backgrounds and reuse the active battle
/// catalog when an authored stage lacks a native mission entry. An alternate
/// background supplies artwork only, never text from a different stage.
fn select_art(
    assets: &AssetManager,
    registry: Option<&CampaignRegistry>,
    mission: Option<&IniFile>,
    filename: &str,
    width: LoadingScreenWidth,
) -> CampaignLoadingMetadata {
    let Some(mission) = mission else {
        return CampaignLoadingMetadata::default();
    };
    let named = std::iter::once(filename).chain(
        registry
            .into_iter()
            .flat_map(|items| items.iter().map(|item| item.scenario())),
    );
    for name in named {
        let mut metadata = CampaignLoadingMetadata::default();
        metadata.apply_ini(mission, name);
        let background = match width {
            LoadingScreenWidth::W640 => metadata.background_name_640(),
            LoadingScreenWidth::W800 => metadata.background_name_800(),
        };
        if !background.is_empty()
            && assets.get_ref(background).is_some()
            && assets.get_ref(metadata.background_palette_name()).is_some()
        {
            return metadata;
        }
    }
    CampaignLoadingMetadata::default()
}

pub(super) fn presentation(
    state: &AppState,
    assets: &AssetManager,
    startup: &PreparedNetworkStartup,
    render_size: [u32; 2],
    scale: ShellScale,
) -> Option<CampaignLoadingPresentation> {
    let authored = startup.authored_scenario()?;
    let mission = crate::rules::retail_sources::select_ini(assets, assets.ini_roots().mission())
        .ok()
        .map(|selected| selected.ini);
    let filename = authored
        .selection
        .key
        .path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let width = LoadingScreenWidth::for_render_width(
        super::composition::loading_authored_size(render_size, scale)[0],
    );
    let metadata = select_art(
        assets,
        state.process_assets.campaigns(),
        mission.as_ref(),
        &filename,
        width,
    );
    let mut presentation =
        CampaignLoadingPresentation::layout_with_art(&metadata, render_size, scale);
    let display = authored.selection.presentation.as_deref();
    presentation.title = Some(authored.selection.title.clone());
    presentation.briefing = display
        .and_then(|display| display.briefing.clone())
        .or_else(|| {
            let section = mission.as_ref()?.section(&filename)?;
            let key = section.read_name("LSLoadBriefing", 31)?;
            state
                .process_assets
                .csf
                .as_ref()?
                .get(key)
                .map(str::to_owned)
        });
    // Reserve a readable box above the meter even when the native artwork
    // came from another stage and had a lower briefing anchor.
    let body = presentation.layout.body;
    let margin = (12.0 * scale.factor()).round() as i32;
    presentation.briefing_origin = [body.x + margin, body.y + margin];
    presentation.authored = true;
    Some(presentation)
}
