//! Sidebar credits counter, drawn with the retail GAME.FNT bitmap font.
//!
//! gamemd's credits draw formats the displayed credit total with a wide `"%ld"`
//! literal and hands it to the BitFont (GAME.FNT) text path **on the sidebar
//! surface**: the anchor is `surface_width / 2` with the horizontal-centre
//! flag, the anchor row is a literal `y = 2`, and the colour is the packed
//! side-dependent sidebar text colour. It emits exactly one pass — retail
//! paints no drop shadow, no outline and no second tinted copy behind the
//! number.
//!
//! The counter is a text node of the sidebar's text tree
//! ([`TextHud`]), drawn in the sidebar's text lane after the cameo labels.
//!
//! This replaced an egui proportional-font painter that used a fixed
//! `(230,240,255)`, a radar-relative anchor and an invented `+1,+1` black
//! shadow. Evidence: `CreditsClass::Draw`.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::borrow::Cow;

use crate::app::presentation::picture_hud::covering;
use crate::app::presentation::text_hud::TextHud;
use crate::render::bit_font::{BitFont, Fit};
use crate::render::sidebar_text::CREDITS_DEPTH;
use crate::ui::match_layout::TextAlign;
use crate::ui::sidebar::{SidebarChromeLayoutSpec, SidebarPart, SidebarView};

/// What the money counter shows: `shown` when the layout's `[Money] Value=`
/// is another value than the credits; with `None` the credits, and the
/// observer's the match's time through `time_format`
/// (`TXT_TIME_FORMAT_HOURS`).
fn credits_text<'a>(view: &SidebarView, time_format: &str, shown: Option<&'a str>) -> Cow<'a, str> {
    match shown {
        Some(text) => Cow::Borrowed(text),
        None if view.observer.is_some() => Cow::Owned(
            crate::render::sidebar_text::format_observer_clock(view.credits, time_format),
        ),
        None => Cow::Owned(crate::render::sidebar_text::format_credits(view.credits)),
    }
}

/// Where the counter's `text` starts on the sidebar surface, before the
/// camera, and how wide it measures: on the credits plate, `plate_width`
/// wide, which in the original spans the sidebar, at the layout table's
/// place for the counter (stock `[0, 2]` from the middle of the plate's top
/// edge). `align` is the layout's `[Money] Align=`: the original centres
/// the text on the plate.
///
/// Centred, gamemd's anchor is the surface midpoint and the h-centre flag
/// pulls the *measured* string half its width to the left.
/// `BitFont::text_width` is that same measurement (trailing inter-character
/// spacing on every glyph), so the half-width offset lands where retail's
/// does rather than on the visual ink centre. Both halvings are integer
/// divisions, as in the native rect helper: in float they would land on a
/// half-pixel for any odd plate width or odd measured width, and the counter
/// would resolve to either of two columns as the fractional camera moved, a
/// visible 1 px shimmer on a number that is on screen the whole match. The
/// plate's corner is rounded first and the camera is added after, so
/// panning never feeds back into the fixed UI geometry.
///
/// Residual: CreditsClass::Draw centres on the sidebar surface's width; the
/// two differ only for a plate narrower than the 168 px surface (retail
/// credits.shp is 168 px wide in SIDEC01.MIX and SIDEC02.MIX).
pub(crate) fn credits_pen(
    font: &BitFont,
    text: &str,
    view: &SidebarView,
    spec: SidebarChromeLayoutSpec,
    plate_width: f32,
    align: TextAlign,
) -> ([f32; 2], u32) {
    let credits = spec.place(SidebarPart::Credits);
    let money = spec.place(SidebarPart::Money);
    let plate_x = view.panel_rect.x + credits[0];
    let measured = font.text_width(text);
    let text_w = measured as i32;
    let x = match align {
        TextAlign::Center => {
            plate_x.round() + ((plate_width.round() as i32) / 2 - text_w / 2) as f32
        }
        TextAlign::Left => plate_x.round(),
        TextAlign::Right => (plate_x + plate_width.round() - measured as f32).round(),
    };
    (
        [x + money[0], view.panel_rect.y.round() + (credits[1] + money[1])],
        measured,
    )
}

/// The money counter, as the frame's next text of `hud` (in the part
/// `Money`): [`credits_text`] from [`credits_pen`], in `tint`, one pass
/// with no shadow or outline. `camera_offset` is what the lane's shader
/// subtracts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_credits(
    hud: &mut TextHud,
    font: &BitFont,
    view: &SidebarView,
    spec: SidebarChromeLayoutSpec,
    plate_width: f32,
    time_format: &str,
    shown: Option<&str>,
    align: TextAlign,
    tint: [f32; 3],
    camera_offset: [f32; 2],
) {
    let text = credits_text(view, time_format, shown);
    let ([x, y], width) = credits_pen(font, &text, view, spec, plate_width, align);
    hud.part(SidebarPart::Money.name());
    hud.fitted_text(
        covering(x, y, width as f32, font.cell_height()),
        &text,
        [x + camera_offset[0], y + camera_offset[1]],
        tint,
        CREDITS_DEPTH,
        Fit::NONE,
    );
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;
    use crate::render::batch::SpriteInstance;
    use crate::render::bit_font::CHAR_SPACING;
    use crate::render::bit_font::tests::make_test_font;

    /// Digit widths standing in for the GAME.FNT table: deliberately unequal so
    /// a proportional/system-font layout cannot reproduce the advances.
    fn digit_font() -> BitFont {
        make_test_font(
            &[
                (b'0' as u16, 7),
                (b'1' as u16, 3),
                (b'2' as u16, 6),
                (b'5' as u16, 6),
                (b'-' as u16, 4),
            ],
            4,
        )
    }

    /// A stock 1024x768 view whose counter shows `credits`.
    fn stock_view(credits: i32) -> SidebarView {
        use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
        use crate::ui::sidebar::{SidebarTab, build_sidebar_view_with_spec};

        build_sidebar_view_with_spec(
            SidebarChromeLayoutSpec::stock(),
            crate::ui::sidebar::SidebarFrame::native(1024, 768),
            SidebarTab::Building,
            credits,
            0,
            0,
            Some([28.0, 27.0]),
            &[],
            &[],
            &[],
            None,
            None,
            &[],
            &SidebarGadgetState::new(),
            None,
            None,
            None,
            None,
            [None; 2],
            &crate::ui::sidebar::cameo_order::CameoStrips::default(),
        )
    }

    /// The counter's glyphs as the sidebar's text tree draws them.
    fn drawn(
        font: &BitFont,
        view: &SidebarView,
        plate_width: f32,
        time_format: &str,
        align: TextAlign,
    ) -> Vec<SpriteInstance> {
        let mut hud = TextHud::default();
        hud.begin();
        draw_credits(
            &mut hud,
            font,
            view,
            SidebarChromeLayoutSpec::stock(),
            plate_width,
            time_format,
            None,
            align,
            [1.0, 1.0, 0.0],
            [0.0, 0.0],
        );
        hud.finish(font);
        hud.glyphs().copied().collect()
    }

    #[test]
    fn credits_render_through_the_retail_font_glyph_table() {
        let font = digit_font();
        let inst = drawn(&font, &stock_view(1250), 168.0, "", TextAlign::Center);

        // One quad per digit — no shadow pass, no outline pass.
        assert_eq!(inst.len(), 4, "expected exactly one quad per digit");

        // Advances are retail glyph widths plus the trailing inter-character
        // spacing, not any proportional metric.
        let spacing = CHAR_SPACING as f32;
        assert_eq!(inst[0].size[0], 3.0, "'1' keeps its narrow FNT width");
        assert_eq!(inst[1].size[0], 6.0, "'2' keeps its FNT width");
        assert_eq!(inst[2].size[0], 6.0);
        assert_eq!(inst[3].size[0], 7.0, "'0' keeps its wide FNT width");
        assert_eq!(inst[1].position[0] - inst[0].position[0], 3.0 + spacing);
        assert_eq!(inst[2].position[0] - inst[1].position[0], 6.0 + spacing);
        assert_eq!(inst[3].position[0] - inst[2].position[0], 6.0 + spacing);

        // Glyph height is the FNT bitmap row count, not a point size.
        for q in &inst {
            assert_eq!(q.size[1], font.glyph_height());
            assert_eq!(q.tint, [1.0, 1.0, 0.0]);
        }
    }

    /// Native halves the plate width and the measured width with integer
    /// division. An odd/odd pair in float lands on a half pixel and the
    /// counter flips between two columns as the camera pans.
    #[test]
    fn credits_centre_uses_integer_halving() {
        let font = digit_font();
        let mut view = stock_view(1);
        view.panel_rect.x = 100.0;
        // Odd plate width, and "1" measures 3 + 1 spacing = 4.
        let spec = SidebarChromeLayoutSpec::stock();
        let plate_x = 100.0 + spec.place(SidebarPart::Credits)[0];
        let ([x, _], width) = credits_pen(&font, "1", &view, spec, 167.0, TextAlign::Center);
        assert_eq!(width, 4);
        // 167/2 = 83, 4/2 = 2 → 81. Float halving would give 81.5.
        assert_eq!(x, plate_x.round() + 81.0 + spec.place(SidebarPart::Money)[0]);
        assert_eq!(x.fract(), 0.0);

        // A fractional panel origin still resolves to a whole pixel.
        view.panel_rect.x = 100.4;
        let ([x, _], _) = credits_pen(&font, "1", &view, spec, 167.0, TextAlign::Center);
        assert_eq!(x.fract(), 0.0);
    }

    /// The counter reads the *displayed* credit total and the sidebar panel
    /// rect off a real `SidebarView` and lands inside the sidebar column, in
    /// the `sidebar_text` lane's depth.
    #[test]
    fn the_counter_sits_inside_the_sidebar_panel() {
        let view = stock_view(1250);
        let font = digit_font();
        let inst = drawn(&font, &view, 168.0, "", TextAlign::Center);

        assert_eq!(inst.len(), 4, "one quad per digit of the displayed total");
        for q in &inst {
            assert!(
                q.position[0] >= view.panel_rect.x
                    && q.position[0] + q.size[0] <= view.panel_rect.x + view.panel_rect.w,
                "credits glyph outside the sidebar column: {:?}",
                q.position
            );
            assert_eq!(q.position[1], 2.0);
            assert_eq!(q.depth, 0.00042);
        }
        // Sidebar column is 168 px at 1x; the string measures 26 px.
        assert_eq!(view.panel_rect.w, 168.0);
        let expected_x = view.panel_rect.x + (168 / 2 - 26 / 2) as f32;
        assert_eq!(inst[0].position[0], expected_x);
        // At the plate's edges when the layout says so.
        let left = drawn(&font, &view, 168.0, "", TextAlign::Left);
        assert_eq!(left[0].position[0], view.panel_rect.x);
        let right = drawn(&font, &view, 168.0, "", TextAlign::Right);
        assert_eq!(right[0].position[0], view.panel_rect.x + 168.0 - 26.0);
    }

    /// The observer's counter shows the match's time, anything past 99 hours
    /// as 99:59:59 (`CreditsClass::Draw @ 0x004A2370`, `0x004A2437..0x004A24A4`).
    #[test]
    fn the_observers_counter_shows_the_time_capped_at_99_hours() {
        let widths = [(b'0', 7), (b'1', 3), (b'5', 5), (b'9', 6), (b':', 2)];
        let font = make_test_font(&widths.map(|(glyph, w)| (u16::from(glyph), w)), 4);
        let mut view = stock_view(0);
        view.observer = Some(Vec::new());
        for (seconds, shown) in [(3_661, "01:01:01"), (360_000, "99:59:59")] {
            view.credits = seconds;
            let drawn: Vec<f32> = drawn(&font, &view, 168.0, "%02d:%02d:%02d", TextAlign::Center)
                .iter()
                .map(|glyph| glyph.size[0])
                .collect();
            let expected: Vec<f32> = shown
                .bytes()
                .map(|c| widths.iter().find(|(glyph, _)| *glyph == c).unwrap().1 as f32)
                .collect();
            assert_eq!(drawn, expected, "{seconds} s shows {shown}");
        }
    }
}
