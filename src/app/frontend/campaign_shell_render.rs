//! Render glue for the campaign selection dialog `0x94`
//! (`ui::campaign_shell`): the dialog paint (background art and right panel),
//! the faction emblems, their captions, the difficulty slider and its labels,
//! the heading, monitor, status line and Back.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::{CampaignShellArt, MainMenuShellChromeEntry};
use crate::render::shell_paint::{
    self, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel, SHELL_TEXT_RGB_ENABLED,
    STATIC_IMAGE_DEPTH,
};
use crate::render::shell_text::ShellAlign;
use crate::ui::campaign_shell::{
    CAMPAIGN_PAGE, CampaignLayout, CampaignSide, DIFFICULTY_MAX, DIFFICULTY_SLIDER, compute_layout,
};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::trackbar::thumb_left;

/// Frame counts of the two emblems (FSALG / FSSLG, or the 640 set).
pub(crate) fn emblem_frame_counts(state: &AppState) -> [usize; 2] {
    state
        .frontend
        .campaign_art
        .as_ref()
        .map_or([0, 0], |art| [art.allied.len(), art.soviet.len()])
}

fn push_entry(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    x: i32,
    y: i32,
    depth: f32,
) {
    out.push(SpriteInstance {
        position: [x as f32, y as f32],
        size: entry.pixel_size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

/// Kind-4 paint centres the frame in the static's window along an axis where
/// the window is larger (`0x0061595E..0x0061597E`).
fn centred_origin(window: RectPx, entry: MainMenuShellChromeEntry) -> (i32, i32) {
    let w = entry.pixel_size[0].round() as i32;
    let h = entry.pixel_size[1].round() as i32;
    let x = if window.w > w {
        window.x + (window.w - w) / 2
    } else {
        window.x
    };
    let y = if window.h > h {
        window.y + (window.h - h) / 2
    } else {
        window.y
    };
    (x, y)
}

/// Part `rect` of `entry` drawn with its origin at `origin`.
fn push_entry_crop(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    origin: (i32, i32),
    rect: RectPx,
    depth: f32,
) {
    let left = rect.x.max(origin.0);
    let top = rect.y.max(origin.1);
    let right = (rect.x + rect.w).min(origin.0 + entry.pixel_size[0] as i32);
    let bottom = (rect.y + rect.h).min(origin.1 + entry.pixel_size[1] as i32);
    if right <= left || bottom <= top {
        return;
    }
    let u_per_px = entry.uv_size[0] / entry.pixel_size[0];
    let v_per_px = entry.uv_size[1] / entry.pixel_size[1];
    out.push(SpriteInstance {
        position: [left as f32, top as f32],
        size: [(right - left) as f32, (bottom - top) as f32],
        uv_origin: [
            entry.uv_origin[0] + (left - origin.0) as f32 * u_per_px,
            entry.uv_origin[1] + (top - origin.1) as f32 * v_per_px,
        ],
        uv_size: [
            (right - left) as f32 * u_per_px,
            (bottom - top) as f32 * v_per_px,
        ],
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

/// The difficulty trackbar (`0x0061D950` paint): the area it saved under
/// itself, one unit darker, over (w+1)x(h+1); the TRAKGRIP thumb; then the
/// two bevel boxes (`0x0061E204`, `0x0061E269`) over both.
fn push_slider(
    out: &mut Vec<SpriteInstance>,
    art: &CampaignShellArt,
    layout: &CampaignLayout,
    background_origin: (i32, i32),
    position: u8,
) {
    let slider = layout.slider;
    if let Some(dark) = art.background_dark {
        push_entry_crop(
            out,
            dark,
            background_origin,
            RectPx::new(slider.x, slider.y, slider.w + 1, slider.h + 1),
            PARENT_BACKGROUND_DEPTH - 0.00001,
        );
    }
    if let Some(thumb) = art.slider_thumb {
        let x = slider.x + thumb_left(position, slider.w, 0, DIFFICULTY_MAX);
        let y = slider.y + (slider.h - 22) / 2;
        push_entry(out, thumb, x, y, STATIC_IMAGE_DEPTH - 0.00001);
    }
    if let Some(frame) = art.slider_frame {
        push_entry(
            out,
            frame,
            slider.x - 2,
            slider.y - 2,
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
    // Background_Overlay (`0x0072E730`) at `[0x00B0FC1C]`: centred only from
    // 1024x768 up (`0x0072EC70`).
    let background_origin = (
        if layout.page.screen.w >= 1024 {
            (layout.page.screen.w - 800) / 2
        } else {
            0
        },
        if layout.page.screen.h >= 768 {
            (layout.page.screen.h - 600) / 2
        } else {
            0
        },
    );
    if let Some(background) = art.background {
        push_entry(
            &mut out,
            background,
            background_origin.0,
            background_origin.1,
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
    for side in CampaignSide::ALL {
        let frames = match side {
            CampaignSide::Allied => &art.allied,
            CampaignSide::Soviet => &art.soviet,
        };
        let Some(entry) = frames.get(campaign.emblem(side).shown_frame()).copied() else {
            continue;
        };
        let (x, y) = centred_origin(layout.emblem(side), entry);
        push_entry(&mut out, entry, x, y, STATIC_IMAGE_DEPTH);
    }
    out
}

fn campaign_labels<'a>(
    state: &'a AppState,
    layout: &CampaignLayout,
    children: bool,
) -> Vec<PaintLabel<'a>> {
    let mut out = Vec::new();
    let Some(campaign) = state.frontend.campaign.as_ref().filter(|_| children) else {
        return out;
    };
    for side in CampaignSide::ALL {
        out.push(PaintLabel {
            text: resolve_csf(state, side.caption_key()),
            rect: layout.caption(side),
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    out.push(PaintLabel {
        text: resolve_csf(state, "GUI:Difficulty"),
        rect: layout.difficulty_label,
        align: ShellAlign::NONE,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    out.push(PaintLabel {
        text: resolve_csf(state, campaign.difficulty_label_key()),
        rect: layout.difficulty_value,
        align: ShellAlign::H_RIGHT,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    out
}

/// Hover help of `0x94` for its status line `0x695`.
fn campaign_status_key(state: &AppState) -> Option<&'static str> {
    let controller = &state.frontend.shell_controller;
    if controller.top_id() != Some(CAMPAIGN_PAGE.dialog) {
        return None;
    }
    let hovered = controller.hovered()?;
    if let Some(side) = CampaignSide::from_emblem(hovered) {
        return Some(side.caption_key());
    }
    if hovered == DIFFICULTY_SLIDER {
        return Some("STT:CampaignSliderDifficulty");
    }
    CAMPAIGN_PAGE
        .button(hovered)
        .map(|button| button.tooltip_key)
}

/// Paint dialog `0x94`. Returns `false` when its art is missing.
pub(crate) fn render_campaign_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.main_menu_shell_chrome.is_none()
        || !crate::app::App::ensure_campaign_art(state)
    {
        return Ok(false);
    }
    let layout = compute_layout(
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: every child stays blank.
    let exit_wave =
        crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::Campaign)
            .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| state.frontend.shell_first_paint_slide.clone());
    let monitor_frame = if leaving {
        None
    } else {
        crate::app::frontend::menu_page_render::paint_shell_monitor(state)
    };
    let (title_window, status_label) = if leaving {
        (None, None)
    } else {
        let title_window = state
            .frontend
            .shell_page_title
            .paint(std::time::Instant::now());
        let status_text = campaign_status_key(state)
            .map(|key| resolve_csf(state, key).into_owned())
            .unwrap_or_default();
        let status_label = crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            layout.page.status_help,
        );
        (title_window, status_label)
    };

    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    let art = state
        .frontend
        .campaign_art
        .as_ref()
        .expect("ensured before render");
    let children = !leaving;
    let art_sprites = campaign_sprites(state, art, &layout, children);
    let mut chrome_sprites = shell_paint::paint_chrome(
        chrome,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        layout.page.screen.w,
    );
    chrome_sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.page.warning_monitor, frame)
    }));
    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(CAMPAIGN_PAGE.dialog);
    let pressed = active.then(|| controller.pressed()).flatten();
    let hovered = active.then(|| controller.hovered()).flatten();
    // While a slide runs the engine draws the whole tile column in place of
    // the buttons (`0x006071E0`).
    let buttons: Vec<PaintButton> = match wave.as_ref() {
        Some(wave) => {
            chrome_sprites.extend(shell_paint::paint_slide_column(
                chrome,
                layout.page.right_panel,
                &wave.button_draws(),
            ));
            Vec::new()
        }
        None => layout
            .page
            .buttons
            .iter()
            .map(|button| PaintButton {
                rect: button.rect,
                pressed: pressed == Some(button.id),
                hovered: hovered == Some(button.id),
                enabled: true,
            })
            .collect(),
    };
    let button_sprites = shell_paint::paint_buttons(
        chrome,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        std::time::Instant::now(),
        None,
    );
    let mut labels = campaign_labels(state, &layout, children);
    for button in layout.page.buttons.iter().filter(|_| wave.is_none()) {
        let Some(spec) = CAMPAIGN_PAGE.button(button.id) else {
            continue;
        };
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.csf_key),
            rect: owner_draw_button_label_rect(button.rect, pressed == Some(button.id)),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    if let Some(window) = title_window {
        labels.push(PaintLabel {
            text: resolve_csf(state, CAMPAIGN_PAGE.title_key),
            rect: layout.page.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    labels.extend(status_label);
    let text = shell_paint::paint_labels(&state.renderer.bit_font, &labels);
    let draws = [
        TexturedDraw {
            texture: &art.texture,
            instances: art_sprites,
        },
        TexturedDraw {
            texture: &chrome.texture,
            instances: chrome_sprites,
        },
        TexturedDraw {
            texture: &chrome.texture,
            instances: button_sprites,
        },
    ];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Campaign Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
        },
    );
    Ok(true)
}
