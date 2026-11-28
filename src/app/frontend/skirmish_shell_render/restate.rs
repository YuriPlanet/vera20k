//! Mission restate briefing 0x10C over the active-game shell, as Diplomacy
//! 0x73: title, Return and status line in the right-panel places, and the
//! briefing as a plain left/top static wrapped and clipped to its rect
//! (`OwnerDraw_Static` 0x006153E0).
//!
//! Return and the briefing paint from 0x10C's `vera20k-gui` nodes
//! ([`super::gui_paint`]), made current first every frame.

use super::gui_paint::GuiStyle;
use crate::app::frontend::state::FrontendState;
use crate::app::input::restate::DIALOG;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::shell::restate::RestateControl;

pub(crate) fn render_restate_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::input::restate::sync(platform, renderer, frontend, match_state, persistence)
        .expect("active 0x10C geometry");
    let ui = (DIALOG.ui)(&match_state.match_presentation).expect("the open dialog's controls");
    let at_start = match_state.match_presentation.briefing_at_start;
    // Phobos `BriefingDialog_ShowBriefing @ 0x65F764` replaces dialog item
    // 1059 (0x423)'s CSF key only while `BriefingTemp::ShowBriefing` is set.
    // The F2 restate path keeps the stock `GUI:Return` caption.
    let button_key = match_state
        .match_presentation
        .briefing_dialog
        .button_label_key(at_start, crate::ui::shell::restate::RETURN_LABEL.0)
        .to_owned();
    let button_text =
        process_assets.csf_label(&button_key, crate::ui::shell::restate::RETURN_LABEL.1);
    // `GetDialogUIStatusLabels_ShowBriefing @ 0x604985` returns the authored
    // status label for the resume button on the scenario-start path.
    let status_text = crate::app::input::dialog_page::help(ui, None).map(|key| {
        let key = if key
            == crate::ui::shell::restate::RestateControl::Return
                .help_key()
                .unwrap_or_default()
        {
            match_state
                .match_presentation
                .briefing_dialog
                .status_label_key(at_start, key)
                .to_owned()
        } else {
            key.to_owned()
        };
        process_assets.csf_label(&key, (DIALOG.help_fallback)(&key))
    });
    let text_of = |id: i32| match u16::try_from(id).ok()? {
        id if id == RestateControl::Return.id() => Some(button_text.clone()),
        crate::ui::shell::in_game_shell::FOOTER => status_text.clone(),
        _ => None,
    };
    let frame = super::in_game_shell::paint_dialog_with_text(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        &DIALOG,
        &text_of,
        |paint, ui| {
            let (ime, bit_font) = (&platform.ime, &renderer.bit_font);
            let content = RestateControl::Content.id();
            // The briefing 0x6E7 is SS_LEFT, top aligned (its node's `Align`),
            // its text the node's own (`input::restate::sync`), borrowed.
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &[content],
                GuiStyle::default(),
            );
        },
    );
    super::in_game_shell::render_in_game_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        destination,
        frame,
    )
}
