//! Render glue for the campaign selection dialog `0x94`
//! (`ui::campaign_shell`): the dialog paint (background art and right panel),
//! the faction emblems, their captions, the difficulty slider and its labels,
//! the heading, monitor, status line and Back.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, FamilyPage, encode_family_page, family_art_ready, page_panel_state,
};
use crate::app::frontend::shell_layout;
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::CampaignShellArt;
use crate::render::shell_paint::{
    PARENT_BACKGROUND_DEPTH, PaintLabel, SHELL_TEXT_RGB_ENABLED, STATIC_IMAGE_DEPTH,
    push_entry_crop_scaled, push_entry_scaled,
};
use crate::ui::campaign_shell::{CAMPAIGN_PAGE, CampaignLayout, CampaignSide, DIFFICULTY_MAX};
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{RectPx, centred_in_window};
use vera20k_gui::shell::trackbar::thumb_left;

/// Frame counts of the emblems (Allied / Soviet / Boot Camp). Boot Camp is
/// empty under YR, which has only two emblems.
pub(crate) fn emblem_frame_counts(state: &AppState) -> [usize; 3] {
    state.frontend.campaign_art.as_ref().map_or([0, 0, 0], |art| {
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
        let x = slider.x + thumb_left(position, slider.w, 0, DIFFICULTY_MAX);
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
    state: &AppState,
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
    let Some(campaign) = state.frontend.campaign.as_ref().filter(|_| children) else {
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
    state: &'a AppState,
    layout: &CampaignLayout,
    children: bool,
) -> Vec<PaintLabel<'a>> {
    let Some(campaign) = state.frontend.campaign.as_ref().filter(|_| children) else {
        return Vec::new();
    };
    let label = |key, rect, align| PaintLabel {
        text: state.csf_text(key).into(),
        rect,
        align,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    };
    // Original RA2 shows a third Boot Camp caption; YR shows only the two
    // campaign emblems. `active()` returns three under `-ra2`, two otherwise.
    let captions = CampaignSide::active().iter().map(|side| {
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
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if !family_art_ready(state) || !crate::app::App::ensure_campaign_art(state) {
        return Ok(false);
    }
    let layout = shell_layout::campaign_layout(state);
    let page = FamilyPage::Campaign;
    let tree = page.tree(state);
    let panel = page_panel_state(
        state,
        ShellSlideKind::Campaign,
        true,
        |state| page.status_key(state),
        layout.page.status_help,
    );
    // The teardown slide's repaint leaves every child blank.
    let children = !panel.leaving;
    let art = state
        .frontend
        .campaign_art
        .as_ref()
        .expect("ensured before render");
    let title = CAMPAIGN_PAGE.title_key;
    let mut frame = FamilyFrame::new("Campaign Shell", &layout.page, panel, title).on(
        state,
        page.dialog(),
        &tree,
    );
    frame.under.push(TexturedDraw {
        texture: &art.texture,
        instances: campaign_sprites(state, art, &layout, children),
    });
    frame.labels = campaign_labels(state, &layout, children);
    encode_family_page(state, encoder, destination, frame);
    Ok(true)
}
