//! Tooltip service driver (study S1): the ONLY wall-clock reader for tooltip
//! timing. Feeds cursor moves + button kills into `ui::tooltips`, re-syncs the
//! in-game sidebar/cameo region set per frame, and builds the in-game tooltip
//! draw instances. Pregame shell status lines use their dialog hover state
//! directly because they are not the delayed native tooltip mechanism.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::presentation::sidebar_render::{current_sidebar_theme, current_sidebar_view};
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::render::batch::SpriteInstance;
use crate::ui::game_screen::GameScreen;
use crate::ui::tooltips::{TipRect, TipRegion};

/// In-game tip ids mirror the gamemd id space: button ids as-is, cameo slots
/// at 1000+.
pub(crate) const CAMEO_TIP_ID_BASE: u32 = 1000;

/// CSF label carrying the cameo tooltip money format. Retail English value is
/// `"%s $%d"` — name, space, currency sign, cost.
///
/// gamemd has a second branch that would use `TXT_MONEY_FORMAT_1` (`"$%d"`,
/// cost only), selected by a global flag. That flag is dead in stock YR: it
/// has exactly two read sites (the cameo tooltip and the cameo status text)
/// and no write site anywhere in the image, and its stored value is 0. The
/// cost-only form is therefore unreachable and is deliberately not modelled.
const CAMEO_TIP_MONEY_FORMAT: &str = "TXT_MONEY_FORMAT_2";

/// CSF labels for the sidebar gadget tips. gamemd dispatches these by the same
/// gadget ids we use (`gadget_input::ID_*`), passing the label in ECX; the
/// numbers previously mistaken for CSF ids at these call sites are the
/// engine's `__LINE__` values.
const TIP_LABEL_SCROLL_UP: &str = "Tip:ScrollUp";
const TIP_LABEL_SCROLL_DOWN: &str = "Tip:ScrollDown";
const TIP_LABELS_TAB: [&str; 4] = ["Tip:Tab1", "Tip:Tab2", "Tip:Tab3", "Tip:Tab4"];
/// CSF label for the parenthesised suffix on a disabled tab/scroll tip.
const TIP_LABEL_DISABLED: &str = "Tip:Disabled";
/// Suffix shape for a disabled tab/scroll tip. Unlike the words, this format
/// is a wide literal compiled into the engine rather than a table entry, so it
/// is not localizable and is reproduced here.
const TIP_DISABLED_FORMAT: &str = "%s\n(%s)";

/// Tip id gamemd uses for the sidebar power meter. It is asked before the
/// sidebar gadget ids, so the region is registered first.
pub(crate) const ID_POWER_TIP: u32 = 999;
/// CSF label for the power readout. Retail English is
/// `"Power = %d\nDrain = %d"`.
const TIP_LABEL_POWER_DRAIN: &str = "TXT_POWER_DRAIN";

/// Whether an already visible tip slides after the cursor. `false` is gamemd:
/// the pop-up keeps the spot it appeared in and only a move off its region ends
/// it, so a resting hand's 1-px jitter leaves it steady. Set `true` for the
/// tracking behaviour (every move hides the tip and re-shows it at the new
/// cursor position). The in-game coordinate toggle has its own native tracking
/// branch (`CursorCheat` 724251 → 72429E) and ignores this.
pub(crate) const TIP_FOLLOWS_CURSOR: bool = false;

/// Box placement: cursor offset, flipped to the other side when the box would
/// leave the region. Both halves are VERA-local: the native placement branches
/// on a descriptor byte and its math is undecoded (plan deferred item), so this
/// offset is a tuned constant, not a gamemd value.
pub(crate) const TIP_CURSOR_OFFSET: [i32; 2] = [12, 16];
/// Popup outline weight. gamemd draws the tip as a 1 px frame in the tip's own
/// text colour around the black fill; captured from the retail client at the
/// cameo tooltip (frame RGB equals the glyph RGB, one pixel wide on all four
/// sides of an 86x38 box).
pub(crate) const TIP_BORDER_PX: f32 = 1.0;
/// Popup box size = measured text plus this much in **total** (not per side):
/// gamemd adds 4 to the measured width and 3 to the measured height.
pub(crate) const TIP_BOX_PAD: [f32; 2] = [4.0, 3.0];
/// Text draw origin inside the popup box: `+2` horizontal, `+4` vertical.
/// With the FNT cell height carrying a 1 px gap under the glyph rows, a
/// one-line tip lands exactly flush with the box bottom.
pub(crate) const TIP_TEXT_INSET: [f32; 2] = [2.0, 4.0];

pub(crate) fn now_ms(state: &AppState) -> u64 {
    state.diagnostic_presentation_ms().unwrap_or_else(|| {
        state
            .match_state
            .match_presentation
            .tooltip_epoch
            .elapsed()
            .as_millis() as u64
    })
}

/// CursorMoved feed (all screens).
pub(crate) fn on_mouse_move(state: &mut AppState) {
    let now = now_ms(state);
    if state.match_state.paused() || state.frontend.keyboard_dialog.is_some() {
        state.match_state.match_presentation.tooltips.on_button(now);
        return;
    }
    let (x, y) = state.match_state.input.cursor_px();
    if state.frontend.screen == GameScreen::InGame && state.match_state.input.cursor_coordinates {
        // Native724200 resolves the current region/text at the mouse move,
        // including ordinary sidebar tips while the coordinate toggle is on.
        sync_in_game_regions(state);
        state
            .match_state
            .match_presentation
            .tooltips
            .on_mouse_move_immediate(x, y, now);
    } else {
        let tips = &mut state.match_state.match_presentation.tooltips;
        tips.set_follow_cursor(TIP_FOLLOWS_CURSOR);
        tips.on_mouse_move(x, y, now);
    }
}

/// MouseInput feed — ANY button, press or release, kills tip + timer.
pub(crate) fn on_button_event(state: &mut AppState) {
    let now = now_ms(state);
    state.match_state.match_presentation.tooltips.on_button(now);
}

/// Per-frame update: refresh regions for the live in-game surface, then pump
/// the delayed-tooltip timer.
pub(crate) fn update(state: &mut AppState) -> u64 {
    let now = now_ms(state);
    if state.frontend.screen == GameScreen::InGame
        && !state.match_state.paused()
        && state.frontend.keyboard_dialog.is_none()
    {
        sync_in_game_regions(state);
    } else {
        state
            .match_state
            .match_presentation
            .tooltips
            .sync_regions(&[]);
    }
    state.match_state.match_presentation.tooltips.poll(now);
    now
}

fn tip_rect(r: crate::ui::sidebar::Rect) -> TipRect {
    TipRect::new(
        r.x.round() as i32,
        r.y.round() as i32,
        r.w.round() as i32,
        r.h.round() as i32,
    )
}

fn csf_text(state: &AppState, key: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text(key).into_owned())
        .unwrap_or_default()
}

/// gamemd re-formats a tab/scroll tip as `"<tip>\n(<Tip:Disabled>)"` when the
/// gadget's enable test fails. Cameo tips return before this step, and the
/// repair/sell ids never reach it. `disabled == false` returns the tip
/// unchanged, so this is the native branch, not an added gate.
fn with_disabled_suffix(state: &AppState, tip: String, disabled: bool) -> String {
    if !disabled || tip.is_empty() {
        return tip;
    }
    let suffix = csf_text(state, TIP_LABEL_DISABLED);
    format_csf(
        TIP_DISABLED_FORMAT,
        &[CsfArg::Str(&tip), CsfArg::Str(&suffix)],
    )
}

/// Cameo tooltip text for a buildable slot.
///
/// gamemd formats the localized `UIName` and the item cost through CSF
/// `TXT_MONEY_FORMAT_2`, then walks the formatted buffer and rewrites **every**
/// space as a line feed. That includes the space the format string itself puts
/// between the name and the price, so a Grizzly Battle Tank cameo tip is four
/// stacked lines — `Grizzly` / `Battle` / `Tank` / `$700` — not one line and
/// not a name/price pair.
fn cameo_tip_text(money_format: Option<&str>, name: &str, cost: Option<i32>) -> String {
    let formatted = match (money_format, cost) {
        (Some(fmt), Some(cost)) => {
            format_csf(fmt, &[CsfArg::Str(name), CsfArg::Int(i64::from(cost))])
        }
        // VERA-internal: gamemd has neither case — a failed CSF load is fatal
        // there and every buildable cameo carries a cost — so this only keeps
        // an assetless dev run readable. gamemd equivalent UNCHECKED.
        _ => name.to_string(),
    };
    formatted.replace(' ', "\n")
}

/// Sidebar regions, mirroring the native registration set: tabs and scroll
/// arrows from their `Tip:*` labels (with the disabled suffix), repair/sell
/// from direct CSF keys, cameos through the money format.
fn sync_in_game_regions(state: &mut AppState) {
    let Some(view) = current_sidebar_view(state).cloned() else {
        state
            .match_state
            .match_presentation
            .tooltips
            .sync_regions(&[]);
        return;
    };
    let mut regions: Vec<TipRegion> = Vec::with_capacity(9 + view.items.len());
    // Set_View_Dimensions4A8B50 registers the complete tactical viewport as
    // dynamic region500. CursorCheat537EF0 toggles its coordinate text at
    // 4AE580, before the ordinary object/shroud tooltip lookup.
    let (x, y, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let coordinate_text = if state.match_state.input.cursor_coordinates {
        let (rx, ry) = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
            state,
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        );
        format!("({rx},{ry})")
    } else {
        String::new()
    };
    regions.push(TipRegion {
        id: 500,
        rect: TipRect::new(x as i32, y as i32, width as i32, height as i32),
        text: coordinate_text,
    });
    // Power meter first: gamemd asks the power bar for a tip before the
    // sidebar gadget ids, and registration order decides the hit here.
    let power_rect = crate::ui::sidebar::power_bar_rect(
        &view.layout,
        state.match_state.match_presentation.sidebar_layout_spec,
    );
    let power_text = state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| {
            format_csf(
                csf.text(TIP_LABEL_POWER_DRAIN).as_ref(),
                &[
                    CsfArg::Int(i64::from(view.power_produced)),
                    CsfArg::Int(i64::from(view.power_drained)),
                ],
            )
        })
        .unwrap_or_default();
    regions.push(TipRegion {
        id: ID_POWER_TIP,
        rect: tip_rect(power_rect),
        text: power_text,
    });
    for (i, tab) in view.tabs.iter().enumerate() {
        let label = TIP_LABELS_TAB
            .get(i)
            .copied()
            .unwrap_or(TIP_LABELS_TAB[TIP_LABELS_TAB.len() - 1]);
        let text = with_disabled_suffix(state, csf_text(state, label), tab.disabled);
        regions.push(TipRegion {
            id: crate::app::input::gadget_input::ID_TAB_BASE as u32 + i as u32,
            rect: tip_rect(tab.rect),
            text,
        });
    }
    regions.push(TipRegion {
        id: crate::app::input::gadget_input::ID_REPAIR as u32,
        rect: tip_rect(view.repair_button.rect),
        text: csf_text(state, "TXT_REPAIR_MODE"),
    });
    regions.push(TipRegion {
        id: crate::app::input::gadget_input::ID_SELL as u32,
        rect: tip_rect(view.sell_button.rect),
        text: csf_text(state, "TXT_SELL_MODE"),
    });
    {
        // Our scroll gadget model carries no disabled state yet, so the
        // `Tip:Disabled` branch is unreachable for this pair; the labels
        // themselves are the native ones.
        regions.push(TipRegion {
            id: crate::app::input::gadget_input::ID_SCROLL_DOWN as u32,
            rect: tip_rect(view.scroll_down_button.rect),
            text: csf_text(state, TIP_LABEL_SCROLL_DOWN),
        });
        regions.push(TipRegion {
            id: crate::app::input::gadget_input::ID_SCROLL_UP as u32,
            rect: tip_rect(view.scroll_up_button.rect),
            text: csf_text(state, TIP_LABEL_SCROLL_UP),
        });
    }
    for (slot, item) in view.items.iter().enumerate() {
        let text = if item.is_superweapon {
            // The retained view carries the same native UIName used for
            // ordering. Supers display it verbatim, without a cost suffix.
            item.display_name.clone()
        } else {
            let money_format = state
                .process_assets
                .csf
                .as_ref()
                .map(|csf| csf.text(CAMEO_TIP_MONEY_FORMAT));
            cameo_tip_text(money_format.as_deref(), &item.display_name, item.cost)
        };
        regions.push(TipRegion {
            id: CAMEO_TIP_ID_BASE + slot as u32,
            rect: tip_rect(item.rect),
            text,
        });
    }
    state
        .match_state
        .match_presentation
        .tooltips
        .sync_regions(&regions);
}

/// In-game tooltip draw: `(fill, outline, text)` instances, drawn between the
/// chat overlay and the software cursor (study O10).
pub(crate) fn build_tooltip_instances(
    state: &AppState,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>, Vec<SpriteInstance>) {
    let Some(tip) = state.match_state.match_presentation.tooltips.active() else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    if state.frontend.screen != GameScreen::InGame {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    // gamemd draws the tip in the current sidebar text colour, so the same
    // side-dependent colour the cameo labels use — not a fixed yellow.
    let tint = crate::render::sidebar_text::side_highlight_color(current_sidebar_theme(state));
    tooltip_quads(
        &state.renderer.bit_font,
        &tip.text,
        [tip.x, tip.y],
        // The whole render surface, not the tactical viewport: gamemd lets the
        // pop-up run over the sidebar chrome and only moves it when the screen
        // itself would clip it.
        [
            0.0,
            0.0,
            state.render_width() as f32,
            state.render_height() as f32,
        ],
        tint,
        // All three tooltip lanes are drawn by `draw_pooled_ui`, which binds
        // the UI camera. That uniform still carries the rounded *world* camera
        // position and the shader subtracts it, so every screen-space UI lane
        // has to add it back — the sidebar text lane and the software cursor
        // both do. `tip.x/y` are cursor coordinates, already screen space, so
        // without this the popup is displaced by the whole camera offset and
        // leaves the screen as soon as the player scrolls.
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
        // The darken strip is only the "a real FNT loaded" sentinel here (the
        // 5x7 fallback builds none); the pop-up itself no longer draws with it.
        state.renderer.bit_font.darken_texture().is_some(),
    )
}

/// Geometry half of [`build_tooltip_instances`], split out so the box metrics
/// and the UI-camera compensation are reachable from a unit test.
///
/// `region` is the `(x, y, width, height)` box the popup must stay inside — the
/// render surface, so a tip called from the sidebar may overlap the chrome and
/// only moves when the screen itself would clip it.
///
/// Returns `(fill quads, outline quads, text quads)`. `camera_offset` must be
/// the live world camera: the UI pipeline subtracts it in the shader, so
/// passing zero here walks the popup off screen.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tooltip_quads(
    font: &crate::render::bit_font::BitFont,
    text: &str,
    tip_xy: [i32; 2],
    region: [f32; 4],
    tint: [f32; 3],
    camera_offset: [f32; 2],
    with_fill: bool,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>, Vec<SpriteInstance>) {
    // Region the popup is sized and placed against. gamemd measures with the
    // selected region's width; which region record it picks is UNCHECKED, so
    // the render surface width is used here.
    let (layout, [box_w, box_h]) = size_tip_box(font, text, region[2] as u32);
    let bx = place_tip(tip_xy[0] as f32, TIP_CURSOR_OFFSET[0] as f32, box_w, region[0], region[2]);
    let by = place_tip(tip_xy[1] as f32, TIP_CURSOR_OFFSET[1] as f32, box_h, region[1], region[3]);

    let mut fill = Vec::with_capacity(1);
    if with_fill {
        fill.push(SpriteInstance {
            position: [bx + camera_offset[0], by + camera_offset[1]],
            size: [box_w, box_h],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: 0.00021,
            // Solid black, not the sidebar darken strip: the retail tip fill
            // hides whatever it lands on instead of dimming it, so the pop-up
            // stays legible over bright cameo art and the tactical map.
            tint: [0.0, 0.0, 0.0],
            alpha: 1.0,
            ..Default::default()
        });
    }
    let outline = [
        (bx, by, box_w, TIP_BORDER_PX),
        (bx, by + box_h - TIP_BORDER_PX, box_w, TIP_BORDER_PX),
        (bx, by, TIP_BORDER_PX, box_h),
        (bx + box_w - TIP_BORDER_PX, by, TIP_BORDER_PX, box_h),
    ]
    .map(|(x, y, w, h)| SpriteInstance {
        position: [x + camera_offset[0], y + camera_offset[1]],
        size: [w, h],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: 0.000205,
        tint,
        alpha: 1.0,
        ..Default::default()
    })
    .to_vec();
    let line_advance = font.cell_height();
    let mut out = Vec::new();
    for (i, span) in layout.lines.iter().enumerate() {
        let line = &text[span.start_byte..span.end_byte];
        out.extend(crate::render::sidebar_text::build_text(
            font,
            line,
            bx + TIP_TEXT_INSET[0],
            by + TIP_TEXT_INSET[1] + i as f32 * line_advance,
            1.0,
            0.00020,
            tint,
            camera_offset,
        ));
    }
    (fill, outline, out)
}

/// Put the box forward-down of the cursor, flipping it to back-up when it would
/// overrun the region, and keep its leading edge inside the region otherwise.
fn place_tip(cursor: f32, offset: f32, size: f32, start: f32, extent: f32) -> f32 {
    let forward = cursor + offset;
    let pos = if forward + size > start + extent {
        cursor - offset - size
    } else {
        forward
    };
    pos.max(start)
}

/// Native popup sizing: measure the tip with the region width as the wrap
/// limit, add `+4` width and `+3` height, and if that box is not narrower than
/// the region, measure again at `region_width - 4` and re-pad. Returns the
/// layout the draw pass must use together with the box size.
pub(crate) fn size_tip_box(
    font: &crate::render::bit_font::BitFont,
    text: &str,
    region_w: u32,
) -> (crate::render::bit_font::WrapLayout, [f32; 2]) {
    let mut layout = font.wrap_layout(text, region_w);
    let mut box_w = layout.width as f32 + TIP_BOX_PAD[0];
    if region_w > 0 && box_w >= region_w as f32 {
        layout = font.wrap_layout(text, region_w.saturating_sub(TIP_BOX_PAD[0] as u32));
        box_w = layout.width as f32 + TIP_BOX_PAD[0];
    }
    let box_h = layout.height as f32 + TIP_BOX_PAD[1];
    (layout, [box_w, box_h])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::bit_font::tests::make_test_font;

    /// Retail English `TXT_MONEY_FORMAT_2`.
    const RETAIL_MONEY_FORMAT_2: &str = "%s $%d";

    #[test]
    fn cameo_tip_splits_every_space_including_the_formats_own() {
        // gamemd rewrites every 0x20 in the formatted buffer as 0x0A, so the
        // name breaks per word AND the price lands on its own line.
        assert_eq!(
            cameo_tip_text(
                Some(RETAIL_MONEY_FORMAT_2),
                "Grizzly Battle Tank",
                Some(700)
            ),
            "Grizzly\nBattle\nTank\n$700"
        );
        assert_eq!(
            cameo_tip_text(Some(RETAIL_MONEY_FORMAT_2), "Tesla Reactor", Some(600)),
            "Tesla\nReactor\n$600"
        );
    }

    #[test]
    fn cameo_tip_currency_text_comes_from_the_csf_format_not_rust() {
        // A localized table that puts the currency word after the amount must
        // survive verbatim; nothing in the Rust may re-add a '$'.
        let localized = "%s %d kr";
        assert_eq!(
            cameo_tip_text(Some(localized), "Grizzly Tank", Some(700)),
            "Grizzly\nTank\n700\nkr"
        );
        assert!(!cameo_tip_text(Some(localized), "Grizzly", Some(700)).contains('$'));
    }

    #[test]
    fn cameo_tip_without_cost_is_the_name_alone() {
        assert_eq!(
            cameo_tip_text(Some(RETAIL_MONEY_FORMAT_2), "Grizzly Battle Tank", None),
            "Grizzly\nBattle\nTank"
        );
    }

    #[test]
    fn disabled_tip_appends_the_parenthesised_csf_word() {
        assert_eq!(
            format_csf(
                TIP_DISABLED_FORMAT,
                &[CsfArg::Str("Structures Tab"), CsfArg::Str("Disabled")]
            ),
            "Structures Tab\n(Disabled)"
        );
    }

    #[test]
    fn tip_box_is_measured_text_plus_four_by_three() {
        let font = make_test_font(&[(b'x' as u16, 6), (b'y' as u16, 6)], 4);
        // Two lines: measured height is 2 * cell_height.
        let (layout, size) = size_tip_box(&font, "xx\nxy", 0);
        assert_eq!(layout.lines.len(), 2);
        let measured_w = layout.width as f32;
        let measured_h = layout.height as f32;
        assert_eq!(measured_h, font.cell_height() * 2.0);
        assert_eq!(size[0], measured_w + 4.0, "width pad is +4 total");
        assert_eq!(size[1], measured_h + 3.0, "height pad is +3 total");
    }

    #[test]
    fn tip_box_remeasures_at_region_width_minus_four_when_too_wide() {
        // Each 'x' measures 6 px of ink plus 1 px trailing spacing = 7.
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let region_w = 31u32;
        // First pass at the region width wraps after 4 glyphs (28 px); with the
        // +4 padding that box is not narrower than the 31 px region, so gamemd
        // measures again at region_width - 4 = 27 and re-pads.
        assert_eq!(font.wrap_layout("xxxxx", region_w).width, 28);
        let (layout, size) = size_tip_box(&font, "xxxxx", region_w);
        assert_eq!(layout.width, 21, "second measure used region_width - 4");
        assert_eq!(size[0], 25.0, "box is the re-measured width + 4");
        assert!(size[0] < region_w as f32);
    }

    #[test]
    fn tip_box_keeps_the_first_measure_when_it_already_fits() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        // 3 glyphs = 21 px, box 25 px, comfortably inside a 100 px region.
        let (layout, size) = size_tip_box(&font, "xxx", 100);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(size[0], 25.0);
        assert_eq!(size[1], font.cell_height() + 3.0);
    }

    /// The UI pipeline binds a camera uniform carrying the live world camera
    /// and the shader subtracts it, so screen-space lanes must add it back.
    /// Passing zero here put the whole popup off screen at any normal camera
    /// position — the regression this pins.
    #[test]
    fn tooltip_quads_compensate_for_the_ui_camera() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let region = [0.0, 0.0, 800.0, 600.0];
        let at_origin = tooltip_quads(
            &font,
            "xxx",
            [100, 100],
            region,
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        let panned = tooltip_quads(
            &font,
            "xxx",
            [100, 100],
            region,
            [1.0, 1.0, 0.0],
            [640.0, 480.0],
            true,
        );

        assert_eq!(at_origin.0.len(), 1);
        assert_eq!(at_origin.1.len(), 4);
        assert_eq!(at_origin.2.len(), 3);
        assert_eq!(panned.0.len(), at_origin.0.len());
        assert_eq!(panned.1.len(), at_origin.1.len());
        assert_eq!(panned.2.len(), at_origin.2.len());

        // Every quad — fill, outline and text — shifts by exactly the camera
        // offset the shader will subtract, leaving the popup at the cursor on
        // screen.
        for (a, b) in at_origin.0.iter().zip(panned.0.iter()) {
            assert_eq!(b.position[0] - a.position[0], 640.0);
            assert_eq!(b.position[1] - a.position[1], 480.0);
        }
        for (a, b) in at_origin.1.iter().zip(panned.1.iter()) {
            assert_eq!(b.position[0] - a.position[0], 640.0);
            assert_eq!(b.position[1] - a.position[1], 480.0);
        }
        for (a, b) in at_origin.2.iter().zip(panned.2.iter()) {
            assert_eq!(b.position[0] - a.position[0], 640.0);
            assert_eq!(b.position[1] - a.position[1], 480.0);
        }

        // And with no pan the box sits at cursor + the native cursor offset,
        // with the text at the +2/+4 inset inside it.
        assert_eq!(
            at_origin.0[0].position,
            [
                100.0 + TIP_CURSOR_OFFSET[0] as f32,
                100.0 + TIP_CURSOR_OFFSET[1] as f32
            ]
        );
        assert_eq!(
            at_origin.2[0].position,
            [
                100.0 + TIP_CURSOR_OFFSET[0] as f32 + 2.0,
                100.0 + TIP_CURSOR_OFFSET[1] as f32 + 4.0
            ]
        );
    }

    /// gamemd keeps the box to the right of and below the cursor, overlapping
    /// the sidebar chrome it describes, and only moves it when the screen itself
    /// would clip it. Measuring against the tactical viewport instead — the
    /// previous behaviour — flipped every sidebar tip to the cursor's left.
    #[test]
    fn sidebar_tip_only_flips_when_the_screen_would_clip_it() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let screen = [0.0, 0.0, 1024.0, 768.0];
        // "xxx" measures 21 px, so the box is 25 wide.
        let (fill, _, text) = tooltip_quads(
            &font,
            "xxx",
            [900, 300],
            screen,
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        assert_eq!(
            fill[0].position,
            [912.0, 316.0],
            "a cameo tip must not flip while the screen still has room"
        );
        assert_eq!(text[0].position, [914.0, 320.0]);

        // One cursor pixel further right leaves no room for the box, so it
        // lands back-up of the cursor instead: 1010 - 12 - 25.
        let (flipped, _, _) = tooltip_quads(
            &font,
            "xxx",
            [1010, 300],
            screen,
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        assert_eq!(flipped[0].position, [973.0, 316.0]);
        assert_eq!(flipped[0].size, [25.0, font.cell_height() + 3.0]);
    }

    #[test]
    fn tip_outline_frames_the_box_in_the_text_colour() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let tint = [0.4, 0.6, 1.0];
        let (fill, border, _) = tooltip_quads(
            &font,
            "xxx",
            [100, 100],
            [0.0, 0.0, 800.0, 600.0],
            tint,
            [0.0, 0.0],
            true,
        );
        let [bx, by] = fill[0].position;
        let [bw, bh] = fill[0].size;
        let expected = [
            [bx, by, bw, 1.0],
            [bx, by + bh - 1.0, bw, 1.0],
            [bx, by, 1.0, bh],
            [bx + bw - 1.0, by, 1.0, bh],
        ];
        assert_eq!(border.len(), expected.len());
        for (quad, [x, y, w, h]) in border.iter().zip(expected) {
            assert_eq!(quad.position, [x, y]);
            assert_eq!(quad.size, [w, h]);
            assert_eq!(quad.tint, tint, "outline follows the side colour");
        }
    }

    /// The retail tip's interior is solid black — captured over bright cameo art
    /// that reads through nowhere. The sidebar darken strip (0,0,0 at 175/255)
    /// dimmed instead of hiding, so a white-tinted darken quad is a regression.
    #[test]
    fn tip_background_is_opaque_black_not_the_darken_strip() {
        let font = make_test_font(&[(b'x' as u16, 6)], 4);
        let (fill, _, _) = tooltip_quads(
            &font,
            "xxx",
            [100, 100],
            [0.0, 0.0, 800.0, 600.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
            true,
        );
        assert_eq!(fill[0].tint, [0.0, 0.0, 0.0]);
        assert_eq!(fill[0].alpha, 1.0);
    }

    #[test]
    fn tip_text_inset_is_two_by_four() {
        // Pins the native draw origin inside the popup box; the previous VERA
        // values were (4, 3).
        assert_eq!(TIP_TEXT_INSET, [2.0, 4.0]);
        assert_eq!(TIP_BOX_PAD, [4.0, 3.0]);
    }

    #[test]
    fn sidebar_gadget_tip_labels_are_the_native_ones() {
        // The ids are gamemd's, so the label table must line up with them:
        // tabs 0xCB..0xCE are Tab1..Tab4 in sidebar tab order.
        assert_eq!(crate::app::input::gadget_input::ID_TAB_BASE, 0x00CB);
        assert_eq!(crate::app::input::gadget_input::ID_SCROLL_UP, 0x00C8);
        assert_eq!(crate::app::input::gadget_input::ID_SCROLL_DOWN, 0x00C9);
        assert_eq!(
            TIP_LABELS_TAB,
            ["Tip:Tab1", "Tip:Tab2", "Tip:Tab3", "Tip:Tab4"]
        );
        assert_eq!(TIP_LABEL_SCROLL_UP, "Tip:ScrollUp");
        assert_eq!(TIP_LABEL_SCROLL_DOWN, "Tip:ScrollDown");
        assert_eq!(TIP_LABEL_DISABLED, "Tip:Disabled");
    }
}
