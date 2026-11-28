//! The connection dialog 0xEA over the active-game shell (WM_PAINT
//! 0x00621E90, then the per-node bars 0x0064A7B0): Quit Game and the status
//! line in the right-panel places, the row buttons, bars, countdown and list
//! as ordinary children.
//!
//! Quit, VERA's paging buttons and page counter, the row buttons (MNBTTN,
//! type-2 captions), the bars, the countdown and the list paint from 0xEA's
//! `vera20k-gui` nodes ([`super::gui_paint`]), made current first every
//! frame.

use super::gui_paint::{ButtonArt, GuiStyle, HostRows};
use super::in_game_shell;
use crate::app::frontend::state::FrontendState;
use crate::app::input::connection::DIALOG;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::shell::connection::{CONNECTION_ROWS, ConnectionControl, PAINT_ORDER};
use crate::ui::shell::paging;

/// 0xEA can be drawn: it needs the same art as every active-game page.
pub(crate) fn connection_shell_drawable(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> bool {
    frontend.screen == crate::app::GameScreen::InGame
        && match_state.network_wait.dialog_open()
        && frontend.skirmish_shell_chrome.is_some()
        && crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state)
            .is_some_and(|atlas| atlas.in_game_shell.panel_tile.is_some())
        && in_game_shell::current_in_game_shell_layout(
            platform,
            renderer,
            frontend,
            match_state,
            persistence,
        )
        .is_some()
}

pub(crate) fn render_connection_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::input::connection::sync(platform, renderer, frontend, match_state, persistence)
        .expect("active 0xEA geometry");
    let frame = in_game_shell::paint_dialog(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        &DIALOG,
        |paint, ui| {
            let (ime, bit_font) = (&platform.ime, &renderer.bit_font);
            let page = match_state.network_wait.page().expect("open 0xEA");
            // Paging, shown only where there is more than one page, paints on the
            // side art and the texts only: before the captions, as it always did.
            let counter = |id: i32| {
                (id == i32::from(paging::COUNTER)).then(|| {
                    let index = page.page.index(page.rows.len(), CONNECTION_ROWS);
                    paging::counter_text(index, page.page_count())
                })
            };
            let style = GuiStyle {
                buttons: ButtonArt::Side,
                side: Some(paint.side_buttons()),
                text_of: Some(&counter),
                ..Default::default()
            };
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &[paging::PREVIOUS, paging::NEXT, paging::COUNTER],
                style,
            );
            let first = page.visible_range().start;
            let text_of = |id: i32| -> Option<String> {
                match ConnectionControl::from_id(u16::try_from(id).ok()?)? {
                    ConnectionControl::Player(slot) => {
                        Some(page.rows.get(first + slot)?.name.clone())
                    }
                    ConnectionControl::Countdown => Some(match &page.countdown {
                        Some(countdown) => countdown.clone(),
                        None => process_assets.csf_label("GUI:TimeRemaining", "Time Remaining:"),
                    }),
                    _ => None,
                }
            };
            // List 0x651's lines in the label colour; those above its top are
            // never shown and stay empty.
            let rows_of = |id: i32| -> Option<HostRows> {
                (id == i32::from(ConnectionControl::List.id())).then(|| {
                    let skipped =
                        (0..page.top.min(page.lines.len())).map(|_| (String::new(), u32::MAX));
                    let shown = page.lines.iter().skip(page.top);
                    skipped
                        .chain(shown.map(|line| (String::from_utf16_lossy(&line.text), u32::MAX)))
                        .collect()
                })
            };
            // 0x00609FC7..0x00609FE2 / 0x0060A330: the row buttons are type3,
            // MNBTTN, with the type-2 caption.
            let style = GuiStyle {
                buttons: ButtonArt::ModalType2,
                text_of: Some(&text_of),
                rows_of: Some(&rows_of),
                ..Default::default()
            };
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &PAINT_ORDER,
                style,
            );
        },
    );
    in_game_shell::render_in_game_shell_frame(
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

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::super::chrome::{push_rect_outline_px, push_solid_rect_px};
    use super::super::gui_paint::{BarLook, push_bar};
    use super::super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH};
    use crate::render::batch::SpriteInstance;
    use crate::render::shell_paint::{self, ModalButton, ModalButtonFrames, ModalDepths};
    use crate::render::skirmish_shell_chrome::SkirmishShellChromeEntry;
    use crate::ui::shell::connection::{
        CONNECTION_ROWS, ConnectionPage, ConnectionRow, PAINT_ORDER, bar_paint,
        connection_layout, spec, sync_nodes,
    };
    use crate::ui::shell::geom::{RETAIL_SCALE, RectPx};
    use vera20k_gui::shell::bar::OUTLINE_RGB as BAR_OUTLINE_RGB;
    use vera20k_gui::{Gui, NodeKind, Prop, Value};

    fn entry(u: f32) -> SkirmishShellChromeEntry {
        SkirmishShellChromeEntry {
            uv_origin: [u, 0.0],
            uv_size: [0.1, 0.1],
            pixel_size: [83.0, 15.0],
        }
    }

    fn rgb(unit: [u8; 3]) -> [f32; 3] {
        unit.map(|channel| f32::from(channel) / 255.0)
    }

    fn timing(source: usize) -> (u32, u32) {
        [
            (0, 0),
            (100, 0),
            (300, 0),
            (600, 100),
            (600, 900),
            (2000, 9000),
        ][source % 6]
    }

    /// The row buttons and bars the painter draws from 0xEA's nodes (one
    /// MNBTTN per Button node in `PAINT_ORDER`, then `push_bar` from each
    /// Bar node's props) are the sprites the hand painter pushed from the
    /// page, in its order, on both pages and with a button held.
    #[test]
    fn row_buttons_and_bars_paint_from_their_nodes_as_by_hand() {
        let shell = crate::ui::shell::in_game_shell::allied_fixture_layout(800, 600);
        let layout = connection_layout(800, 600, shell, [125, 25]);
        let frames = ModalButtonFrames {
            up: Some(entry(0.1)),
            disabled: Some(entry(0.2)),
            pressed: Some(entry(0.3)),
        };
        let white = Some(entry(0.9));
        let depths = ModalDepths {
            background: SHELL_CONTROL_DEPTH,
            button: SHELL_CONTROL_DEPTH,
            text: SHELL_CONTROL_TEXT_DEPTH,
        };
        let mut page = ConnectionPage {
            rows: (0..11)
                .map(|row| ConnectionRow {
                    source_index: 10 - row,
                    name: format!("player {row}"),
                })
                .collect(),
            ..Default::default()
        };
        for next in [false, true] {
            if next {
                assert!(page.change_page(true));
            }
            let visible = page.visible_range();
            let held_slot = 1;
            // connection.rs before the nodes.
            let buttons: Vec<_> = (0..visible.len())
                .map(|slot| ModalButton {
                    rect: layout.players[slot],
                    pressed: slot == held_slot,
                    enabled: true,
                })
                .collect();
            let screen = RectPx::new(0, 0, 800, 600);
            let mut hand = shell_paint::paint_modal_sprites(
                None,
                frames,
                screen,
                &buttons,
                RETAIL_SCALE,
                depths,
            );
            for (slot, row) in visible.clone().enumerate() {
                let source = page.row_source(row).unwrap();
                let (lag, age) = if row == 0 { (0, 0) } else { timing(source) };
                let bar = bar_paint(layout.bars[slot], lag, age);
                push_rect_outline_px(
                    &mut hand,
                    white,
                    bar.outline,
                    rgb(BAR_OUTLINE_RGB),
                    SHELL_CONTROL_DEPTH,
                );
                push_solid_rect_px(
                    &mut hand,
                    white,
                    bar.fill,
                    rgb(bar.fill_rgb),
                    SHELL_CONTROL_DEPTH - 0.00001,
                );
            }
            // From the nodes, as gui_paint walks PAINT_ORDER.
            let mut gui = Gui::new(64);
            let root = gui.create(NodeKind::Dialog, None).unwrap();
            crate::ui::shell::page::ensure_built(&mut gui, root, spec);
            crate::ui::shell::page::sync_rects(&mut gui, root, |id| page.rect(&layout, id));
            sync_nodes(&mut gui, root, &page, timing);
            let mut sprites: Vec<SpriteInstance> = Vec::new();
            let mut slot = 0;
            for id in PAINT_ORDER {
                let node = gui.find_id(root, id.into()).unwrap();
                if gui.get(node, Prop::Visible).unwrap() != Some(&Value::Bool(true)) {
                    continue;
                }
                let rect = gui.tree().screen_rect(node).unwrap();
                match gui.tree().kind(node).unwrap() {
                    NodeKind::Button => {
                        let button = ModalButton {
                            rect,
                            pressed: slot == held_slot,
                            enabled: true,
                        };
                        slot += 1;
                        sprites.extend(shell_paint::paint_modal_sprites(
                            None,
                            frames,
                            rect,
                            &[button],
                            RETAIL_SCALE,
                            depths,
                        ));
                    }
                    NodeKind::Bar => {
                        let Ok(Some(Value::Int(percent))) = gui.get(node, Prop::Fill) else {
                            panic!("a bar without its fill");
                        };
                        let Ok(Some(Value::Rgba(c))) = gui.get(node, Prop::Color) else {
                            panic!("a bar without its colour");
                        };
                        let look = BarLook {
                            percent: *percent,
                            outline: rgb(BAR_OUTLINE_RGB),
                            fill: rgb([c.0, c.1, c.2]),
                        };
                        push_bar(&mut sprites, white, rect, look);
                    }
                    _ => {}
                }
            }
            assert_eq!(slot, visible.len().min(CONNECTION_ROWS));
            assert_eq!(sprites.len(), hand.len());
            assert_eq!(
                bytemuck::cast_slice::<SpriteInstance, u8>(&sprites),
                bytemuck::cast_slice::<SpriteInstance, u8>(&hand)
            );
        }
    }
}
