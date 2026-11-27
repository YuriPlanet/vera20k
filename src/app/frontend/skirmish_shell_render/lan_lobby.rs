//! BB LAN lobby composition: native5DDBD0 with ordinary shell60CF00,
//! NewEdit614B30, list619230 and scrollbar61C690. Discovery rows are UI views.

use super::SHELL_CONTROL_DEPTH;
use super::chrome::*;
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_lan::LanShellRuntime;
use crate::app::state::PlatformState;
use crate::render::hud_paint::Material;
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::lan_shell::{CHAT_LOG, GAMES, JOIN, PAGE, PLAYERS, STATUS, TITLE};
use crate::ui::shell::controller::DialogController;
use crate::util::time::Instant;

/// Whether box button `id` shows pushed (612B70).
fn box_pressed(shell_controller: &DialogController, id: i32) -> bool {
    let gui = shell_controller.gui();
    gui.dialogs()
        .last()
        .and_then(|d| gui.find_id(gui.dialog_root(d)?, id))
        .is_some_and(|node| gui.shows_pressed(node))
}

/// VERA's Create Game box (`LanShellRuntime::creating`): the online box's
/// art, then its buttons and its room's limits as its widgets hold them.
fn paint_create_box(
    ime: &ImeState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: &crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas,
    overlay: &mut ShellFrameOverlay,
) {
    use crate::app::shell_lan::{
        CANCEL, MODAL, OK, ROOM_PLAYERS, ROOM_PLAYERS_CAPTION, ROOM_SPECTATORS,
    };
    let layout = crate::app::shell_lan::create_box_layout(&renderer.gpu);
    overlay.controls.extend(super::wol_lobby::online_box(
        atlas,
        layout.dialog,
        layout.scale,
    ));
    let controller = &frontend.shell_controller;
    let Some(root) = controller.root_in(MODAL) else {
        return;
    };
    let ids = [
        OK,
        CANCEL,
        ROOM_PLAYERS,
        ROOM_PLAYERS_CAPTION,
        ROOM_SPECTATORS,
    ];
    let style = super::gui_paint::GuiStyle::popup();
    let widgets = super::gui_paint::paint_gui_ids(
        ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
        controller.gui(),
        root,
        ids,
        layout.scale,
        style,
    );
    overlay
        .controls
        .extend(widgets.lane(Material::Controls).copied());
    overlay.texts.extend(widgets.texts);
}

/// BB's message box as it shows, when one is up.
fn lan_notice(
    lan: &Option<LanShellRuntime>,
    process_assets: &ProcessAssets,
) -> Option<crate::ui::shell::modal::ShellBox> {
    use crate::app::shell_lan::{MODAL, OK};
    let runtime = lan.as_ref()?;
    let key = runtime.modal?;
    let body = (runtime.modal_body.clone()).unwrap_or_else(|| process_assets.csf_label(key, key));
    let ok = process_assets.csf_label("TXT_OK", "OK");
    Some(crate::ui::shell::modal::ShellBox::new(MODAL, body).button(OK, ok))
}

pub(crate) fn render_lan_lobby(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::shell_lan::sync_gui(renderer, frontend);
    // The message box on its widgets (built as it opened) before it paints.
    let notice = lan_notice(&frontend.lan, process_assets);
    let size = (
        renderer.render_width(&frontend.screen) as i32,
        renderer.render_height(&frontend.screen) as i32,
    );
    let notice_parts = notice
        .as_ref()
        .map(|notice| super::shell_box::skirmish_parts(notice, size));
    if let (Some(notice), Some(parts)) = (notice.as_ref(), notice_parts.as_ref()) {
        let controller = &mut frontend.shell_controller;
        if let Some(root) = controller.root_in(notice.dialog) {
            notice.sync(controller.gui_mut(), root, parts);
        }
    }
    let now = Instant::now();
    let layout = crate::app::shell_lan::layout(&renderer.gpu)
        .with_touch_send(platform.touch_send);
    let scale = layout.page.scale;
    let width = renderer.render_width(&frontend.screen);
    let height = renderer.render_height(&frontend.screen);
    let exit = crate::app::frontend::shell_transition::shell_exit_wave(
        &frontend.shell_exit,
        ShellSlideKind::LanLobby,
    )
    .cloned();
    let leaving = exit.is_some();
    let wave = exit.or_else(|| frontend.shell_first_paint_slide.clone());
    let sliding = wave.is_some();
    let title = (!sliding)
        .then(|| frontend.shell_page_title.paint(now))
        .flatten();
    let frames = frontend
        .skirmish_shell_chrome
        .as_ref()
        .map_or(0, |atlas| atlas.launcher_warning_frames.len());
    let monitor = if leaving {
        None
    } else {
        frontend.shell_monitor.paint(now, frames, !sliding)
    };
    let status = if sliding {
        None
    } else {
        let gui = frontend.shell_controller.gui();
        // VERA's send key answers the emote button's hint, whose tile it borrows.
        let send_help = layout.touch_send && {
            let (px, py) = match_state.input.cursor_px();
            layout.chat_send().is_some_and(|rect| {
                px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
            })
        };
        let help = if send_help {
            process_assets.csf_text("STT:EmoteButton")
        } else {
            crate::ui::shell::page::hovered_key(gui)
                .map(|key| process_assets.csf_label(key, ""))
                .unwrap_or_default()
        };
        crate::app::frontend::menu_page_render::paint_shell_status_line(
            &mut frontend.shell_status_line,
            help,
            layout.page.status_help,
        )
    };
    let runtime = frontend.lan.as_ref().expect("active BB");
    let lobby = &runtime.lobby;
    let atlas = frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("BB control art");
    // BB's native background is precisely the ordinary MNSCRN shell family.
    let mut art =
        super::launcher_options::launcher_background_instances(atlas, width, height, scale);
    if let Some(frame) = monitor.and_then(|frame| atlas.launcher_warning_frames.get(frame)) {
        push_flag_entry_clipped_centered(
            &mut art,
            *frame,
            layout.page.warning_monitor,
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }
    if let Some(wave) = &wave {
        push_slide_column(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(scale, width, height),
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    }
    let mut controls = Vec::new();
    let mut texts = Vec::new();
    let mut overlay = ShellFrameOverlay::default();
    // VERA's Create Game box shows over the bare backdrop, as the online
    // lobby's does.
    let gui = frontend.shell_controller.gui();
    let root = frontend.shell_controller.root_in(PAGE.dialog);
    if !leaving {
        let observer = frontend.multiplayer_shell_runtime.lan_observer();
        // The column's buttons (left to the slide while it runs), the lists
        // with their rows as their widgets hold them and the observer box.
        let rows_of = |id: i32| -> Option<super::gui_paint::HostRows> {
            let yellow = |text: String| (text, u32::MAX);
            Some(match u16::try_from(id).ok()? {
                GAMES => (0..lobby.game_count())
                    .map(|index| match index.checked_sub(1) {
                        None => yellow(process_assets.csf_label("TXT_LOBBY", "Lobby")),
                        Some(game) => {
                            let game = &lobby.games[game];
                            let format =
                                process_assets.csf_label(game.label_key(), game.label_key());
                            yellow(format.replacen("%s", &game.name, 1))
                        }
                    })
                    .collect(),
                // Active BB has no custom columns: native registry record+F8
                // is null (selected-game GUI probe).5DAFE0's4A8 color/side/status
                // writes therefore return-1 in618D40; only the name is visible.
                PLAYERS => lobby
                    .players
                    .iter()
                    .map(|player| yellow(player.name.clone()))
                    .collect(),
                CHAT_LOG => {
                    let line = |line: &crate::ui::lan_shell::LanChatLine| {
                        (String::from_utf16_lossy(&line.text), line.color)
                    };
                    lobby.chat.lines.iter().map(line).collect()
                }
                _ => return None,
            })
        };
        // The column's captions, Join's Observe for an observer; the heading
        // and the status line while they reveal.
        let text_of = |id: i32| {
            let id = u16::try_from(id).ok()?;
            let key = match id {
                TITLE => {
                    return title.map(|_| process_assets.csf_label(PAGE.title_key, "Lobby"));
                }
                STATUS => return status.as_ref().map(|status| status.text.to_string()),
                JOIN if observer => "GUI:Observe",
                id => PAGE.button(id)?.csf_key,
            };
            Some(process_assets.csf_label(key, key))
        };
        let reveal_of = |id: i32| match u16::try_from(id).ok()? {
            TITLE => title.map(|reveal| PathAReveal {
                count: reveal.count,
                range: reveal.range,
                base_rgb: [255, 255, 0],
                highlight_rgb: [255; 3],
            }),
            STATUS => status.as_ref()?.path_a_reveal,
            _ => None,
        };
        let enabled_of = |_| Some(lobby.enabled);
        // The observer image shows its eye, not a box.
        let picture_of = |id: i32| {
            (id == i32::from(crate::ui::lan_shell::OBSERVER_IMAGE))
                .then_some(super::gui_paint::Picture::Observer { on: observer })
        };
        // An edit's caret blinks while it has BB's focus, under the message
        // box too.
        let controller = &frontend.shell_controller;
        let caret_of = |node| lobby.enabled && controller.caret_visible_in(PAGE.dialog, node);
        let style = super::gui_paint::GuiStyle {
            caret_of: Some(&caret_of),
            buttons: match sliding {
                true => super::gui_paint::ButtonArt::Host,
                false => super::gui_paint::ButtonArt::Panel,
            },
            text_of: Some(&text_of),
            enabled_of: Some(&enabled_of),
            rows_of: Some(&rows_of),
            control_lists: true,
            reveal_of: Some(&reveal_of),
            picture_of: Some(&picture_of),
            ..Default::default()
        };
        if let Some(root) = root {
            let paint = super::gui_paint::paint_gui_styled(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                gui,
                root,
                scale,
                &[],
                style,
            );
            controls.extend(paint.lane(Material::Controls).copied());
            texts.extend(paint.texts);
        }
        if let (Some(notice), Some(parts)) = (notice.as_ref(), notice_parts.as_ref()) {
            use crate::app::shell_lan::OK;
            let pressed = box_pressed(&frontend.shell_controller, OK.into());
            let look = super::BoxLook::skirmish(atlas.mode_two_dialog_background(None));
            let (sprites, texts) = (&mut overlay.controls, (&mut overlay.texts, false));
            let pressed = |id| pressed && id == OK;
            let tree = super::controller_tree(&frontend.shell_controller, notice.dialog);
            super::shell_box::push_skirmish_box(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                Some(atlas),
                tree,
                notice,
                parts,
                pressed,
                look,
                sprites,
                texts,
            );
        }
    }
    if runtime.creating {
        paint_create_box(
            &platform.ime,
            renderer,
            frontend,
            match_state,
            process_assets,
            atlas,
            &mut overlay,
        );
    }
    // VERA's send key beside the chat box. The host draws it, so it has to go
    // with the page's slide: otherwise it hangs over the next menu's entrance.
    if !leaving && layout.touch_send && let Some(rect) = layout.chat_send() {
        super::push_chat_send_key(&mut overlay.controls, atlas, scale, rect);
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
            label: "LAN Lobby BB",
        },
        ShellFrameArt::Launcher,
        overlay,
        ShellFrameCursor::Visible,
    )?;
    platform.window.request_redraw();
    Ok(())
}
