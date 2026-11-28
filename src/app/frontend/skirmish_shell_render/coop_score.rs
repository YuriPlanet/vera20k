//! Paint the LAN co-op result dialog `0xBC6`. `UI__RegisterWindow`
//! (`0x00622820`) gives it BC's D5/D6 chrome (SDTP frame 1 and SDMPBTN)
//! and no RadarBackground (D7); `0x0060CF00` gives it `SCRBK` for its
//! background. The children are the setup page's native widgets.
//! RESIDUAL: the statics paint at once, without their kind-1 reveal, and
//! the page has no entry or exit slide.

use super::SHELL_PARENT_BACKGROUND_DEPTH;
use super::chrome::*;
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::preview::{build_preview_surface_instance, ensure_map_preview_texture};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::hud_paint::Material;
use crate::ui::campaign_score_shell::campaign_score_background_rect;
use crate::ui::coop_score_shell::*;

pub(crate) fn render_coop_score(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<bool> {
    if frontend.skirmish_shell_chrome.is_none() {
        return Ok(false);
    }
    let Some(view) = crate::app::cooperative_round::view(frontend, match_state, process_assets)
    else {
        return Ok(false);
    };
    crate::app::input::coop_score::update_geometry(renderer, frontend, match_state, process_assets);
    let layout = crate::app::input::coop_score::layout(&renderer.gpu);
    let scale = layout.page.scale;
    let (width, height) = (
        renderer.render_width(&frontend.screen),
        renderer.render_height(&frontend.screen),
    );
    let fills = crate::app::frontend::shell_layout::fills_window(&renderer.gpu);
    let (can_continue, map_index) = {
        let model = &frontend.coop_score_page.as_ref().expect("view").model;
        (model.can_continue, model.map_index)
    };
    match map_index.filter(|_| can_continue) {
        Some(index) => {
            let preview = layout.control_rect(PREVIEW).unwrap_or_default();
            ensure_map_preview_texture(
                platform,
                renderer,
                frontend,
                process_assets,
                persistence,
                index,
                preview,
            )
        }
        None => frontend.skirmish_preview_texture = None,
    }
    let shown = |id: u16| can_continue || !hidden_on_final_mission(id);
    let controller = &frontend.shell_controller;
    let hovered = crate::ui::shell::menu_page::hovered_id(controller, COOP_SCORE_PAGE.dialog);
    let status = hovered
        .and_then(help_key)
        .map(|key| process_assets.csf_label(key, ""))
        .unwrap_or_default();
    let page = frontend.coop_score_page.as_ref().expect("view");
    let model = &page.model;
    let ui = &page.ui;
    let atlas = frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("checked above");
    let rgb = |rgb: [u8; 3]| rgb.map(|channel| f32::from(channel) / 255.0);

    // Paint mode 1: RightPanel__Draw, the dialog background, then D5/D6.
    let chrome_layout = crate::ui::skirmish_shell::compute_layout_on(scale, width, height);
    let mut art = Vec::new();
    push_right_panel_base_instances(&mut art, atlas, &chrome_layout, false);
    push_lower_strip_instance(&mut art, atlas, &chrome_layout);
    push_steady_optional_chrome_instances(
        &mut art,
        atlas,
        &chrome_layout,
        super::draw_order::ShellDialogChromeProfile::SkirmishSetup0x102,
    );
    let score_backdrop = frontend.campaign_score_art.as_ref().map(|scores| {
        let (aw, ah) = if fills {
            (800, 600)
        } else {
            (width as i32, height as i32)
        };
        let rect = scale.scale_rect(campaign_score_background_rect(aw, ah));
        let mut sprites = Vec::with_capacity(1);
        crate::render::shell_paint::push_entry_sized(
            &mut sprites,
            scores.background,
            rect.x as f32,
            rect.y as f32,
            [rect.w as f32, rect.h as f32],
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
        sprites[0]
    });

    // The body from its nodes, in the order it paints: the heading and
    // status line, the statics, the score cells in their row's colour, each
    // slot's icons and lists (shown, never opened), Difficulty, the chat
    // edit in its one-pixel frame with its caret steady while it has BC6's
    // focus, and the transcript.
    let dialog = COOP_SCORE_PAGE.dialog;
    let gui = controller.gui();
    let text_of = |id: i32| {
        let id = u16::try_from(id).ok()?;
        let slot = |ids: [u16; 2]| view.slots[ids.iter().position(|each| *each == id)?].as_ref();
        let pick = |choice: &crate::ui::multiplayer_setup::SetupChoice| {
            choice.swatch.is_none().then(|| choice.text.clone())
        };
        if let Some((row, column)) = ROW_CELLS
            .iter()
            .enumerate()
            .find_map(|(row, cells)| Some((row, cells.iter().position(|cell| *cell == id)?)))
        {
            let row = model.rows.get(row)?;
            return Some(match column {
                0 => row.name.clone(),
                1 => row.kills.to_string(),
                2 => row.losses.to_string(),
                3 => row.built.to_string(),
                _ => row.score.to_string(),
            });
        }
        match id {
            TITLE => {
                Some(process_assets.csf_label(COOP_SCORE_PAGE.title_key, COOP_SCORE_PAGE.title_key))
            }
            STATUS => Some(status.clone()),
            TIME => Some(model.time.clone()),
            NEXT_MISSION => Some(model.next_mission.clone()),
            DESCRIPTION => Some(model.description.clone()),
            MODE_LABEL => Some(model.mode_label.clone()),
            MAP_LABEL => Some(model.map_label.clone()),
            id if NAMES.contains(&id) => slot(NAMES).map(|slot| slot.name.clone()),
            id if COUNTRIES.contains(&id) => pick(&slot(COUNTRIES)?.country),
            id if COLORS.contains(&id) => pick(&slot(COLORS)?.color),
            id if STARTS.contains(&id) => pick(&slot(STARTS)?.start),
            id => {
                let spec = COOP_SCORE_CONTROLS.iter().find(|spec| spec.id == id)?;
                spec.caption_key
                    .map(|key| process_assets.csf_label(key, key))
            }
        }
    };
    // The cells take their row's colour.
    let rgb_of = |id: i32| {
        let id = u16::try_from(id).ok()?;
        let row = ROW_CELLS.iter().position(|cells| cells.contains(&id))?;
        Some(rgb(model.rows.get(row)?.rgb))
    };
    // 0x0046FA20: wolhost.pcx on slot 0 and wolacpt.pcx on slot 1 while
    // ready, on human slots only.
    let picture_of = |id: i32| {
        let row = READY_ICONS.iter().position(|icon| i32::from(*icon) == id)?;
        let ready = view.slots[row].as_ref()?.ready;
        Some(super::gui_paint::Picture::Status {
            host: row == 0 && ready,
            accepted: row == 1 && ready,
        })
    };
    // 0x0046DCC0 disables the other human's combos; the name combo has no
    // handler and stays disabled for both. EnableWindow(0x732) at
    // `0x0046DDF3..0x0046DE10`.
    let enabled_of = |id: i32| {
        let id = u16::try_from(id).ok()?;
        if id == DIFFICULTY {
            return Some(view.host);
        }
        if NAMES.contains(&id) {
            return Some(false);
        }
        let row = [COUNTRIES, COLORS, STARTS]
            .iter()
            .find_map(|ids| ids.iter().position(|each| *each == id))?;
        Some(view.slots[row].as_ref()?.enabled)
    };
    let focused = |_: vera20k_gui::NodeId| controller.focused_control(dialog) == Some(CHAT);
    // The transcript's rows in view, each in its colour.
    let log = controller.control(dialog, CHAT_LOG).and_then(|node| {
        Some((
            gui.list_view(node)?.top,
            gui.list_geometry(node)?.visible_rows,
        ))
    });
    let rows_of = |id: i32| {
        let (top, visible) = log.filter(|_| id == i32::from(CHAT_LOG))?;
        let line = |(index, line): (usize, &crate::ui::lan_shell::LanChatLine)| {
            let text = match (top..top + visible).contains(&index) {
                true => String::from_utf16_lossy(&line.text),
                false => String::new(),
            };
            (text, line.color)
        };
        Some(ui.chat.lines.iter().enumerate().map(line).collect())
    };
    let style = super::gui_paint::GuiStyle {
        ime: false,
        text_of: Some(&text_of),
        rgb_of: Some(&rgb_of),
        picture_of: Some(&picture_of),
        enabled_of: Some(&enabled_of),
        focus_of: Some(&focused),
        caret_of: Some(&focused),
        rows_of: Some(&rows_of),
        control_lists: true,
        clipped_faces: true,
        edit_inset: Some(1),
        ..Default::default()
    };
    let mut body = match controller.root_in(dialog) {
        Some(root) => super::gui_paint::paint_gui_with(
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            atlas,
            gui,
            root,
            scale,
            &[],
            style,
        ),
        None => Default::default(),
    };
    let controls = body.take_lane(Material::Controls);
    let mut texts = body.texts;
    let mut overlay = ShellFrameOverlay {
        score_backdrop,
        ..Default::default()
    };
    if let (Some(preview), Some(rect)) = (
        frontend.skirmish_preview_texture.as_ref(),
        layout.control_rect(PREVIEW),
    ) {
        overlay.preview = build_preview_surface_instance(rect, preview.width, preview.height);
    }
    if let Some(root) = controller.root_in(COOP_SCORE_PAGE.dialog) {
        let enabled_of = |id: i32| match u16::try_from(id).ok()? {
            CONTINUE => Some(view.continue_enabled),
            _ => Some(view.quit_enabled),
        };
        let style = super::gui_paint::GuiStyle {
            buttons: super::gui_paint::ButtonArt::Panel,
            enabled_of: Some(&enabled_of),
            ..Default::default()
        };
        let ids = layout.page.buttons.iter().map(|button| button.id);
        let buttons = super::gui_paint::paint_gui_ids(
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            controller.gui(),
            root,
            ids.filter(|id| shown(*id)),
            scale,
            style,
        );
        art.extend(buttons.lane(Material::Controls).copied());
        texts.extend(buttons.texts);
    }
    in_game_shell::render_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "LAN Co-op Result BC6",
        },
        ShellFrameArt::Launcher,
        overlay,
        ShellFrameCursor::Visible,
    )?;
    platform.window.request_redraw();
    Ok(true)
}
