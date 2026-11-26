//! Paint Single Player's Load Saved Game `0xB7`. Outside a suspended game it
//! paints as a family page (`0x00621FB1..0x00621FFE`): the default parent
//! background (`0x0060D20B`, MNSCRNL), the right panel, heading `0x694`,
//! status line `0x695`, Load and Back, and the list `0x525`. The template has
//! no monitor and no movie static, and the prompt `0x40C` is hidden.

use anyhow::Result;

use crate::app::frontend::menu_page_render::{
    FamilyFrame, encode_family_page, family_art_ready, page_panel_state,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::skirmish_shell_render::gui_paint::ButtonArt;
use crate::app::frontend::skirmish_shell_render::{paint_saved_browser, push_saved_browser_box};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::hud_paint::Material;
use crate::ui::skirmish_shell::saved_seed_browser::BROWSER_DIALOG;
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

/// Paint `0xB7`. Returns `false` when the shell chrome is missing.
pub(crate) fn render_load_saved_game_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if frontend.load_saved_game.is_none()
        || !family_art_ready(
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
        )
    {
        return Ok(false);
    }
    let layout =
        crate::app::frontend::shell_layout::load_saved_game_layout(&renderer.gpu, frontend);
    let panel = page_panel_state(
        frontend,
        process_assets,
        ShellSlideKind::LoadSavedGame,
        false,
        |frontend| crate::app::App::load_saved_game_status_key(frontend.load_saved_game.as_ref()),
        layout.page.status_help,
    );
    let leaving = panel.leaving;
    let Some(browser) = frontend.load_saved_game.as_ref() else {
        return Ok(false);
    };
    // The browser's own Load and Back are the column's buttons; a message
    // box drops their press, so they paint released under it.
    let title = browser.game_title_label().0;
    let mut frame = FamilyFrame::new("Load Saved Game Shell", &layout.page, panel, title);
    let ui = &browser.ui;
    frame.buttons = ui.root_in(BROWSER_DIALOG).map(|root| (ui.gui(), root));
    frame.backdrop = true;
    if !leaving {
        let geometry = ShellListGeometry::new(
            layout.browser.list,
            browser.entries.len(),
            browser.top(),
            ListMetrics::scaled(layout.browser.scale),
        );
        frame
            .list_boxes
            .push((layout.list_window, geometry.scrollbar));
    }
    // The list and its rows, blank while the teardown slide runs, then the box.
    let mut over = paint_saved_browser(
        &platform.ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
        frontend.skirmish_shell_chrome.as_ref(),
        &layout.browser,
        browser,
        ButtonArt::Host,
        !leaving,
    );
    let atlas = frontend.skirmish_shell_chrome.as_ref();
    over.draw(Material::Controls, |sprites, texts| {
        push_saved_browser_box(
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            atlas,
            &layout.browser,
            browser,
            None,
            sprites,
            texts,
        )
    });
    frame.over = over;
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
