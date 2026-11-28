//! Paint the score dialog `0x108`: the paint-mode-1 dialog paint (the right
//! panel, then the side art at the dialog origin `[0xB0FC1C]`), the proc's
//! shaded rectangle (`0x005CA07F..0x005CA0FB`), and the children: the band
//! bars, the table texts, the heading, the monitor, the status line and
//! Continue.
//!
//! While the entry slide runs the bars and the monitor already show, and the
//! kind-1 texts wait for its end (retail still `g2-s2.png` mid-slide). The
//! teardown slide starts with a full dialog repaint (`0x00622C4F`) whose
//! shaded rectangle follows only once the slide has ended (the proc paints
//! after the common handler returns), and pumps no messages: the art and the
//! column. The panel, monitor, column, Continue and status line are the
//! family compositor's ([`encode_family_page`]); the heading the proc sets,
//! the table's statics, VERA's page counter and the band bars paint from
//! their nodes ([`crate::ui::score_shell::nodes`]).

use anyhow::Result;
use vera20k_gui::Prop;

use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, encode_family_page, family_art_ready, page_panel_state, page_tree,
};
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::skirmish_shell_render::gui_paint::{GuiStyle, PageArt, paint_gui_into};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_paint::{
    PARENT_BACKGROUND_DEPTH, SHELL_TEXT_RGB_ENABLED, push_entry_crop, push_entry_native,
    push_entry_scaled, push_entry_window_crop,
};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::score_shell::ScorePage;
use crate::ui::score_shell::{
    BAND_IDS, CONTINUE_BUTTON, HEADING, NEXT_PAGE, PREVIOUS_PAGE, SCORE_PAGE, ScoreStatics,
    sync_static_texts,
};
use crate::ui::shell::geom::dialog_origin;
use crate::ui::shell::{menu_page, paging};
use crate::util::time::Instant;

/// The shaded part of the art sits over the art.
const SHADE_DEPTH: f32 = PARENT_BACKGROUND_DEPTH - 0.00001;

/// A score dialog's kind-1 static `id` (`0x108`, `0x103`, `0xBC7`) as it
/// reveals this recomposition: in its `0x498` colour, else the shell's
/// yellow, trailing the white highlight; `None` while it is hidden.
pub(crate) fn static_reveal(statics: &ScoreStatics, id: u16) -> Option<PathAReveal> {
    let (window, rgb) = statics.shown(id)?;
    Some(PathAReveal {
        count: window.count,
        range: window.range,
        base_rgb: rgb.unwrap_or([255, 255, 0]),
        highlight_rgb: [255, 255, 255],
    })
}

/// The heading `0x694` the proc sets (`0x005C9BEC..0x005C9C34`).
pub(crate) fn score_title_text(
    score_page: &Option<ScorePage>,
    process_assets: &ProcessAssets,
) -> String {
    score_page
        .as_ref()
        .map(|page| process_assets.csf_text(page.model.title_key))
        .unwrap_or_default()
}

/// Paint dialog `0x108`. Returns `false` when its chrome or art is missing.
pub(crate) fn render_score_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if frontend.score_page.is_none()
        || frontend.score_art.is_none()
        || !family_art_ready(
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
        )
    {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::score_layout(&renderer.gpu);
    let (screen, scale, dialog) = (layout.page.screen, layout.page.scale, SCORE_PAGE.dialog);
    let paged = frontend
        .score_page
        .as_ref()
        .is_some_and(|page| page.page_count() > 1);
    let rect_of = |id| layout.node_rect(id, paged);
    let mut tree = page_tree(
        &mut frontend.shell_controller,
        dialog,
        crate::ui::score_shell::nodes,
        rect_of,
        &[],
    );
    // Only Continue has status help; the paging pair has none.
    let mut panel = page_panel_state(
        frontend,
        process_assets,
        ShellSlideKind::Score,
        true,
        |frontend| {
            let controller = &frontend.shell_controller;
            (controller.top_id() == Some(dialog))
                .then(|| menu_page::hovered_help(controller, dialog))
                .flatten()
        },
        layout.page.status_help,
    );
    // Children paint until the teardown repaint covers them; the kind-1
    // texts stay hidden until the entry slide's end starts them. The
    // heading reveals the proc's title in place of the family's.
    let leaving = panel.leaving;
    let title = panel.title.take().map(shell_reveal_path_a);
    let column = panel.column.is_some();
    {
        let page = frontend.score_page.as_mut().expect("checked before render");
        page.statics.paint(Instant::now(), !leaving);
        let page = &*page;
        // The statics carry the template windows `0x0060B950` keeps; on a
        // stretched page they map onto the window with the table. A row
        // past the result's shows nothing.
        let text_of = |id: u16| match id {
            HEADING => Some((Prop::Label, page.model.title_key)),
            paging::COUNTER => None,
            id => Some((Prop::Text, page.statics.text(id).unwrap_or_default())),
        };
        let controller = &mut frontend.shell_controller;
        match tree.as_mut() {
            Some((gui, root)) => sync_static_texts(gui, *root, text_of),
            None => {
                if let Some(root) = controller.root_in(dialog) {
                    sync_static_texts(controller.gui_mut(), root, text_of);
                }
            }
        }
    }
    let page = frontend.score_page.as_ref().expect("checked before render");
    let art = frontend.score_art.as_ref().expect("checked before render");
    let rows = page.visible_range().len();

    // The heading, the statics while they show, the page counter beside
    // the paging pair (the slide engine draws the whole column while either
    // slide runs, `0x006071E0`), and the bands: 0 and 1 always carry a bar,
    // player bands only up to the row count (`0x005C9D82..0x005C9DD1`,
    // `0x005C9E4C..0x005C9E7A`).
    let bands = if leaving { 0 } else { 2 + rows };
    let text_of = |id: i32| match u16::try_from(id).ok()? {
        HEADING => title.is_none().then(String::new),
        paging::COUNTER => Some(match column {
            true => String::new(),
            false => paging::counter_text(page.page_index(), page.page_count()),
        }),
        id => page.statics.shown(id).is_none().then(String::new),
    };
    let reveal_of = |id: i32| match u16::try_from(id).ok()? {
        HEADING => title,
        id => static_reveal(&page.statics, id),
    };
    let page_art_of = |id: i32| {
        let band = BAND_IDS.iter().position(|band| i32::from(*band) == id)?;
        art.bars[band].filter(|_| band < bands).map(PageArt::Pcx)
    };
    let style = GuiStyle {
        text_of: Some(&text_of),
        reveal_of: Some(&reveal_of),
        page_art_of: Some(&page_art_of),
        rgb: SHELL_TEXT_RGB_ENABLED,
        ..Default::default()
    };
    let controller = &frontend.shell_controller;
    let nodes = match &tree {
        Some((gui, root)) => Some((gui, *root)),
        None => controller
            .root_in(dialog)
            .map(|root| (controller.gui(), root)),
    };
    let mut paint = HudPaint::default();
    if let Some((gui, root)) = nodes {
        paint_gui_into(
            &mut paint,
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            None,
            gui,
            root,
            scale,
            &[],
            style,
        );
    }

    // Paint mode 1 (`0x00621E90`): the right panel, then the dialog's own
    // background at the dialog origin.
    let origin = dialog_origin(screen.w, screen.h);
    let mut art_sprites = Vec::new();
    if let Some(background) = art.background {
        if scale.is_unscaled() {
            push_entry_native(
                &mut art_sprites,
                background,
                origin.0,
                origin.1,
                PARENT_BACKGROUND_DEPTH,
            );
        } else {
            push_entry_scaled(
                &mut art_sprites,
                background,
                0,
                0,
                scale,
                PARENT_BACKGROUND_DEPTH,
            );
        }
    }
    if !leaving {
        match (art.background_shaded, layout.shaded_rect(rows)) {
            (Some(shaded), Some(rect)) if scale.is_unscaled() => {
                push_entry_crop(&mut art_sprites, shaded, origin, rect, SHADE_DEPTH)
            }
            (Some(shaded), Some(rect)) => {
                let window = scale.dialog_rect();
                push_entry_window_crop(&mut art_sprites, shaded, window, rect, SHADE_DEPTH)
            }
            _ => {}
        }
    }
    // The bars over the shade, on the art's texture.
    art_sprites.extend(paint.lane(Material::PageArt).copied());

    // The shell pass draws in array order with depth off, so paint mode 1's
    // panel goes under the background. A country card (`[EndGameScreens]
    // Score*Art=`) is opaque over the whole dialog and would hide the panel,
    // Continue and the lower strip, so it goes under the chrome instead.
    let art_draw = TexturedDraw {
        texture: &art.texture,
        instances: art_sprites,
    };
    let title = SCORE_PAGE.title_key;
    let mut frame = FamilyFrame::new("Score Shell", &layout.page, panel, title).on(
        &frontend.shell_controller,
        dialog,
        &tree,
    );
    frame.ids = vec![CONTINUE_BUTTON, PREVIOUS_PAGE, NEXT_PAGE];
    frame.over = paint;
    if page.model.background.is_some() {
        frame.under.push(art_draw);
    } else {
        frame.over_panel_art.push(art_draw);
    }
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
