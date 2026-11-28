//! Render glue for the campaign selection dialog `0x94`
//! (`ui::campaign_shell`): the dialog paint (background art and right panel),
//! the faction emblems, their captions, the difficulty slider and its labels,
//! the heading, monitor, status line and Back.

use anyhow::Result;

use crate::app::frontend::menu_page_render::{
    FamilyFrame, FamilyPage, encode_family_page, family_art_ready, page_panel_state,
};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::CampaignShellArt;
use crate::render::shell_paint::{
    PARENT_BACKGROUND_DEPTH, PaintLabel, SHELL_TEXT_RGB_ENABLED, STATIC_IMAGE_DEPTH,
    push_entry_crop_scaled, push_entry_scaled,
};
use crate::ui::campaign_shell::CampaignShellState;
use crate::ui::campaign_shell::{CAMPAIGN_PAGE, CampaignLayout, CampaignSide, DIFFICULTY_MAX};
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{RectPx, centred_in_window};
use vera20k_gui::shell::trackbar::TrackbarRange;

/// Frame counts of the emblems (Allied / Soviet / Boot Camp). Boot Camp is
/// empty under YR, which has only two emblems.
pub(crate) fn emblem_frame_counts(campaign_art: &Option<CampaignShellArt>) -> [usize; 3] {
    campaign_art.as_ref().map_or([0, 0, 0], |art| {
        [art.allied.len(), art.soviet.len(), art.bootcamp.len()]
    })
}

/// The difficulty trackbar (`0x0061D950` paint): the area it saved under
/// itself, one unit darker, over (w+1)x(h+1); the TRAKGRIP thumb; then the
/// two bevel boxes (`0x0061E204`, `0x0061E269`) over both.
fn push_slider(
    out: &mut Vec<SpriteInstance>,
    art: &CampaignShellArt,
    layout: &CampaignLayout,
    background_origin: (i32, i32),
    position: i32,
) {
    let scale = layout.scale;
    // The native metrics below (the one-pixel saved area, the 22-pixel thumb and
    // the frame's two-pixel border) are authored numbers: the window maps back
    // first and the art is stretched, so paint and press track the same rail.
    let slider = scale.base_rect(layout.slider);
    if let Some(dark) = art.background_dark {
        push_entry_crop_scaled(
            out,
            dark,
            background_origin,
            RectPx::new(slider.x, slider.y, slider.w + 1, slider.h + 1),
            scale,
            PARENT_BACKGROUND_DEPTH - 0.00001,
        );
    }
    if let Some(thumb) = art.slider_thumb {
        let range = TrackbarRange::new(0, DIFFICULTY_MAX, 1).with_plaque(false);
        let x = slider.x + range.thumb_left(position, slider.w);
        let y = slider.y + (slider.h - 22) / 2;
        push_entry_scaled(out, thumb, x, y, scale, STATIC_IMAGE_DEPTH - 0.00001);
    }
    if let Some(frame) = art.slider_frame {
        push_entry_scaled(
            out,
            frame,
            slider.x - 2,
            slider.y - 2,
            scale,
            STATIC_IMAGE_DEPTH - 0.00002,
        );
    }
}

/// The dialog's own paint and the children it shows.
fn campaign_sprites(
    campaign: &Option<CampaignShellState>,
    art: &CampaignShellArt,
    layout: &CampaignLayout,
    children: bool,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    let scale = layout.scale;
    // Background_Overlay (`0x0072E730`) at `[0x00B0FC1C]`: centred only from
    // 1024x768 up (`0x0072EC70`). A scaled dialog starts at the authored
    // origin instead, which the scale places in the window.
    let background_origin = if scale.is_unscaled() {
        crate::ui::shell::geom::dialog_origin(layout.page.screen.w, layout.page.screen.h)
    } else {
        (0, 0)
    };
    if let Some(background) = art.background {
        push_entry_scaled(
            &mut out,
            background,
            background_origin.0,
            background_origin.1,
            scale,
            PARENT_BACKGROUND_DEPTH,
        );
    }
    let Some(campaign) = campaign.as_ref().filter(|_| children) else {
        return out;
    };
    push_slider(
        &mut out,
        art,
        layout,
        background_origin,
        campaign.difficulty(),
    );
    for &side in CampaignSide::active() {
        let frames = match side {
            CampaignSide::Allied => &art.allied,
            CampaignSide::Soviet => &art.soviet,
            CampaignSide::BootCamp => &art.bootcamp,
        };
        let Some(entry) = frames.get(campaign.emblem(side).shown_frame()).copied() else {
            continue;
        };
        let (w, h) = entry.native_size();
        let (x, y) = centred_in_window(scale.base_rect(layout.emblem(side)), w, h);
        push_entry_scaled(&mut out, entry, x, y, scale, STATIC_IMAGE_DEPTH);
    }
    out
}

fn campaign_labels<'a>(
    campaign: &'a Option<CampaignShellState>,
    process_assets: &'a ProcessAssets,
    layout: &CampaignLayout,
    children: bool,
    ra2_mode: bool,
) -> Vec<PaintLabel<'a>> {
    let Some(campaign) = campaign.as_ref().filter(|_| children) else {
        return Vec::new();
    };
    let label = |key, rect, align| PaintLabel {
        text: process_assets.csf_text(key).into(),
        rect,
        align,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    };
    // game.exe RT_DIALOG 0x94 (0x00BBE210) has no emblem caption statics.
    // gamemd.exe has 0x7A7 and 0x7A8 for the Allied and Soviet captions.
    let caption_sides: &[CampaignSide] = if ra2_mode { &[] } else { &CampaignSide::ALL };
    let captions = caption_sides.iter().map(|side| {
        label(
            side.caption_key(),
            layout.caption(*side),
            ShellAlign::H_CENTER,
        )
    });
    let difficulty = [
        label("GUI:Difficulty", layout.difficulty_label, ShellAlign::NONE),
        label(
            campaign.difficulty_label_key(),
            layout.difficulty_value,
            ShellAlign::H_RIGHT,
        ),
    ];
    captions.chain(difficulty).collect()
}

/// Paint dialog `0x94`. Returns `false` when its art is missing.
pub(crate) fn render_campaign_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if !family_art_ready(
        renderer,
        frontend,
        process_assets,
        &persistence.options_profile,
    ) || !crate::app::App::ensure_campaign_art(renderer, frontend, process_assets)
    {
        return Ok(false);
    }
    let layout = shell_layout::campaign_layout(&renderer.gpu);
    let page = FamilyPage::Campaign;
    let tree = page.tree(&renderer.gpu, frontend);
    let panel = page_panel_state(
        frontend,
        process_assets,
        ShellSlideKind::Campaign,
        true,
        |frontend| page.status_key(&frontend.shell_controller),
        layout.page.status_help,
    );
    // The teardown slide's repaint leaves every child blank.
    let children = !panel.leaving;
    let art = frontend
        .campaign_art
        .as_ref()
        .expect("ensured before render");
    let title = CAMPAIGN_PAGE.title_key;
    let mut frame = FamilyFrame::new("Campaign Shell", &layout.page, panel, title).on(
        &frontend.shell_controller,
        page.dialog(),
        &tree,
    );
    frame.under.push(TexturedDraw {
        texture: &art.texture,
        instances: campaign_sprites(&frontend.campaign, art, &layout, children),
    });
    frame.labels = campaign_labels(
        &frontend.campaign,
        process_assets,
        &layout,
        children,
        crate::util::ra2_launch::enabled(),
    );
    encode_family_page(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        encoder,
        destination,
        frame,
    );
    Ok(true)
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::assets::asset_manager::MediaArchiveMode;

    #[test]
    fn campaign_labels_follow_the_launch_mode() {
        let campaign = Some(CampaignShellState::open(1));
        let assets = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, false);

        let layout = crate::ui::campaign_shell::compute_layout(800, 600);
        let ra2_labels = campaign_labels(&campaign, &assets, &layout, true, true);
        let yr_labels = campaign_labels(&campaign, &assets, &layout, true, false);

        assert_eq!(
            ra2_labels
                .iter()
                .map(|label| label.rect)
                .collect::<Vec<_>>(),
            [layout.difficulty_label, layout.difficulty_value]
        );
        assert_eq!(
            yr_labels.iter().map(|label| label.rect).collect::<Vec<_>>(),
            [
                layout.caption(CampaignSide::Allied),
                layout.caption(CampaignSide::Soviet),
                layout.difficulty_label,
                layout.difficulty_value,
            ]
        );
    }
}
