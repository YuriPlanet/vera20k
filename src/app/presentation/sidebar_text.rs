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
//! This replaced an egui proportional-font painter that used a fixed
//! `(230,240,255)`, a radar-relative anchor and an invented `+1,+1` black
//! shadow. Evidence: `docs/gap-scans/2026-08-02-phase4/survey-GSI-02.14.md`
//! R12a (`CreditsClass::Draw`, decompiled 2026-08-02) and ranked gap 1.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::ui::sidebar::SidebarView;
// Generic credit glyph generation is render-owned (F06); this module keeps
// only the sidebar-view adapter below. Re-exported for existing callers.
pub use crate::render::sidebar_text::{build_credits_instances, credits_tint};
/// Per-frame wrapper over [`build_credits_instances`] taking the sidebar view.
pub fn build_sidebar_credits_instances(
    font: &BitFont,
    view: &SidebarView,
    ui_scale: f32,
    tint: [f32; 3],
    camera_offset: [f32; 2],
) -> Vec<SpriteInstance> {
    build_credits_instances(
        font,
        view.credits,
        view.panel_rect,
        ui_scale,
        tint,
        camera_offset,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::bit_font::CHAR_SPACING;
    use crate::render::bit_font::tests::make_test_font;
    use crate::ui::sidebar::Rect;

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

    fn panel() -> Rect {
        Rect {
            x: 632.0,
            y: 0.0,
            w: 168.0,
            h: 600.0,
        }
    }

    #[test]
    fn credits_render_through_the_retail_font_glyph_table() {
        let font = digit_font();
        let inst = build_credits_instances(&font, 1250, panel(), 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);

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
        }
    }

    /// Native halves the surface width and the measured width with integer
    /// division. An odd/odd pair in float lands on a half pixel and the
    /// counter flips between two columns as the camera pans.
    #[test]
    fn credits_centre_uses_integer_halving() {
        let font = digit_font();
        // Odd surface width, and "1" measures 3 + 1 spacing = 4.
        let rect = Rect {
            x: 100.0,
            y: 0.0,
            w: 167.0,
            h: 600.0,
        };
        let inst = build_credits_instances(&font, 1, rect, 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);
        // 167/2 = 83, 4/2 = 2 → 100 + 81. Float halving would give 100 + 81.5.
        assert_eq!(inst[0].position[0], 181.0);
        assert_eq!(inst[0].position[0].fract(), 0.0);

        // A fractional panel origin still resolves to a whole pixel.
        let skewed = Rect { x: 100.4, ..rect };
        let inst = build_credits_instances(&font, 1, skewed, 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);
        assert_eq!(inst[0].position[0].fract(), 0.0);
    }

    /// The production wrapper must read the *displayed* credit total and the
    /// sidebar panel rect off a real `SidebarView`, and land the glyphs inside
    /// the sidebar column. This is what `render::build_instances` appends
    /// to the `sidebar_text` lane, which is drawn from the GAME.FNT atlas.
    #[test]
    fn view_wrapper_places_credits_inside_the_sidebar_panel() {
        use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
        use crate::ui::sidebar::{
            SidebarChromeLayoutSpec, SidebarTab, build_sidebar_view_with_spec,
        };

        let view = build_sidebar_view_with_spec(
            SidebarChromeLayoutSpec::stock(),
            1024.0,
            768.0,
            SidebarTab::Building,
            1250,
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
        );
        let font = digit_font();
        let inst = build_sidebar_credits_instances(&font, &view, 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);

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
    }
}
