//! Native in-game menu B5/BBA/FF, sharing72F540 composition with Game
//! Controls.

use super::gui_paint::{ButtonArt, FrameArt, GuiStyle};
use super::in_game_shell::{self, InGamePaint};
use super::launcher_options;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::hud_paint::Material;
use crate::ui::shell::pause_menu::{
    FOOTER, INTERNET_CAPTIONS, INTERNET_GROUP, INTERNET_GROUP_LABEL, INTERNET_VALUES,
    InternetSlider, KEYBOARD_LABEL, PAUSE_MENU_FOOTER, PauseMenuButton, PauseMenuButtonState,
    PauseMenuTemplate, TITLE,
};

/// The app supplies every button state, including Load/Delete eligibility.
/// This renderer owns no button actions and cannot silently substitute a
/// no-op route.
///
/// The menu's buttons, title, status line and FF's group box, trackbars and
/// statics paint from its `vera20k-gui` nodes ([`super::gui_paint`]), made
/// current first every frame. The Options page's column buttons are its
/// own dialog's nodes, painted as the menu's on the side's art.
pub(crate) fn render_pause_menu_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    buttons: [PauseMenuButtonState; 8],
) -> anyhow::Result<()> {
    let (layout, page_layout) = crate::app::input::pause_menu::sync(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        &buttons,
    )
    .expect("active shell");
    let mut overlay = in_game_shell::ShellFrameOverlay::default();
    let template = layout.template;
    let label = match template {
        PauseMenuTemplate::Offline => "Active Pause Menu B5",
        PauseMenuTemplate::Network => "Active Pause Menu BBA",
        PauseMenuTemplate::Internet => "Active Pause Menu FF",
    };
    let frame = {
        let mut paint = InGamePaint::new(platform, renderer, frontend, match_state, persistence);
        let (ime, bit_font) = (&platform.ime, &renderer.bit_font);
        let ui = &match_state.match_presentation.dialog_ui;
        let pause = &match_state.match_presentation.internet_pause;
        let shared_pause = crate::app::input::pause_menu::multiplayer_control(match_state)
            .map(|control| control.caption);
        let slot = |ids: [u16; 2], id: u16| -> Option<InternetSlider> {
            let index = ids.iter().position(|of| *of == id)?;
            Some(InternetSlider::ALL[index])
        };
        let caption = |(key, fallback): (&str, &str)| process_assets.csf_label(key, fallback);
        let text_of = |id: i32| -> Option<String> {
            let id = u16::try_from(id).ok()?;
            if let Some(button) = template.button(id) {
                return Some(match button {
                    PauseMenuButton::SharedPause => {
                        shared_pause.map(crate::i18n::tr).unwrap_or_default()
                    }
                    PauseMenuButton::Resume if shared_pause.is_some() => {
                        crate::i18n::tr("VERA:ReturnToGame")
                    }
                    PauseMenuButton::Keyboard => caption(KEYBOARD_LABEL),
                    _ => caption(template.control(button)?.caption()),
                });
            }
            if let Some(slider) = slot(INTERNET_CAPTIONS, id) {
                return Some(caption(slider.caption()));
            }
            if let Some(slider) = slot(INTERNET_VALUES, id) {
                return Some(caption(pause.value_caption(slider)));
            }
            match id {
                TITLE => Some(caption(template.title().caption())),
                // The common handler shows the hovered control's 6040B0 help
                // in 0x695.
                FOOTER => Some(
                    match crate::app::input::pause_menu::hovered_help(
                        &match_state.match_presentation,
                    ) {
                        Some(key) => caption((key, "")),
                        None => caption(PAUSE_MENU_FOOTER.caption()),
                    },
                ),
                INTERNET_GROUP => Some(caption(INTERNET_GROUP_LABEL)),
                // The trackbars show their position.
                _ => None,
            }
        };
        let frame_of = |id: i32| (id == i32::from(INTERNET_GROUP)).then_some(FrameArt::Group);
        let style = GuiStyle {
            buttons: ButtonArt::Side,
            side: Some(paint.side_buttons()),
            text_of: Some(&text_of),
            frame_of: Some(&frame_of),
            ..Default::default()
        };
        // The template's buttons in their sidebar order, VERA's after them;
        // one without a rect is hidden and paints nothing.
        paint.nodes(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            ui,
            &PauseMenuButton::ALL.map(|button| template.id(button)),
            style,
        );
        paint.nodes(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            ui,
            &[TITLE, FOOTER],
            style,
        );
        let page = match_state.match_presentation.options_page.as_ref();
        if let (Some(page_layout), Some(page)) = (&page_layout, page) {
            // The page's sidebar buttons, Options or the Options column, are the
            // menu's own buttons, on this side's art; the page that shows is
            // lit. Its own controls paint only with the control art
            // (switching YR Original Style failed to rebuild it); the menu's
            // own buttons still work. The open list paints after every
            // control and text.
            let column = launcher_options::ColumnArt::Side(paint.side_buttons());
            let mut widgets = launcher_options::paint_page(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                &page.dialog,
                page_layout.scale,
                column,
                &[],
            );
            widgets.drain_lane(Material::Side, &mut paint.art);
            overlay.controls = widgets.take_lane(Material::Popup);
            widgets.drain_lane(Material::Controls, &mut paint.controls);
            paint.texts.extend(widgets.texts);
            overlay.texts = widgets.popup_texts;
        }
        // FF's group box, then each plaque trackbar with its caption and
        // value static (60B7A0), where its page shows them and the control
        // art is there.
        if frontend.skirmish_shell_chrome.is_some() {
            let [connection, speed] = InternetSlider::ALL.map(InternetSlider::id);
            let internet = [
                INTERNET_GROUP,
                connection,
                INTERNET_CAPTIONS[0],
                INTERNET_VALUES[0],
                speed,
                INTERNET_CAPTIONS[1],
                INTERNET_VALUES[1],
            ];
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &internet,
                style,
            );
        }
        paint.finish(label)
    };
    in_game_shell::render_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        destination,
        frame,
        in_game_shell::ShellFrameArt::Sidebar,
        overlay,
        in_game_shell::ShellFrameCursor::Visible,
    )
}
