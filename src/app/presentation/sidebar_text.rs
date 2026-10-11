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
#[cfg(test)]
use crate::render::sidebar_text::{CREDITS_DEPTH, CREDITS_SURFACE_Y, format_credits};
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

    #[test]
    fn credits_anchor_is_sidebar_surface_centre_at_row_two() {
        let font = digit_font();
        let rect = panel();
        let inst = build_credits_instances(&font, 1250, rect, 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);

        // gamemd: x = surface_width / 2 with the h-centre flag, y = the
        // literal 2. Both literals are asserted here, not read back out of the
        // constants, so a change to either fails.
        assert_eq!(CREDITS_SURFACE_Y, 2.0);
        let measured = font.text_width("1250") as i32;
        let expected_x = 632.0 + (168 / 2 - measured / 2) as f32;
        assert_eq!(inst[0].position[0], expected_x);
        for q in &inst {
            assert_eq!(q.position[1], 2.0, "credits sit on surface row 2");
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

    #[test]
    fn credits_depth_is_the_sidebar_text_layer() {
        let font = digit_font();
        let inst = build_credits_instances(&font, 500, panel(), 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);
        // The `sidebar_text` lane depth. Above the chrome (0.00048) and the
        // cameo overlays (0.00043), matching the cameo status labels.
        assert_eq!(CREDITS_DEPTH, 0.00042);
        for q in &inst {
            assert_eq!(q.depth, 0.00042);
        }
    }

    /// The colour *decision*, not just its propagation: the credits take the
    /// side-dependent sidebar text colour, and specifically not the fixed
    /// `(230,240,255)` the retired egui painter used.
    #[test]
    fn credits_use_the_side_text_colour_not_a_fixed_rgb() {
        use crate::render::sidebar_chrome::SidebarTheme;

        const RETIRED_EGUI_RGB: [f32; 3] = [230.0 / 255.0, 240.0 / 255.0, 255.0 / 255.0];
        let allied = credits_tint(SidebarTheme::Allied);
        let soviet = credits_tint(SidebarTheme::Soviet);
        assert_eq!(allied, [164.0 / 255.0, 210.0 / 255.0, 1.0]);
        assert_eq!(soviet, [1.0, 1.0, 0.0]);
        assert_eq!(credits_tint(SidebarTheme::Yuri), soviet);
        assert_ne!(allied, RETIRED_EGUI_RGB);
        assert_ne!(soviet, allied, "the colour must depend on the side");

        let font = digit_font();
        for theme in [SidebarTheme::Allied, SidebarTheme::Soviet] {
            let tint = credits_tint(theme);
            let inst = build_credits_instances(&font, 500, panel(), 1.0, tint, [0.0, 0.0]);
            assert!(!inst.is_empty());
            for q in &inst {
                assert_eq!(q.tint, tint);
                assert_eq!(q.alpha, 1.0);
            }
        }
    }

    #[test]
    fn credits_scale_with_the_ui_scale() {
        let font = digit_font();
        let rect = Rect {
            x: 1264.0,
            y: 0.0,
            w: 336.0,
            h: 1200.0,
        };
        let inst = build_credits_instances(&font, 1250, rect, 2.0, [1.0, 1.0, 0.0], [0.0, 0.0]);
        assert_eq!(inst[0].size[0], 6.0, "'1' at 2x");
        // Native row 2 scaled by the UI factor: 2 * 2 = 4.
        assert_eq!(inst[0].position[1], 4.0);
        let measured = (font.text_width("1250") as f32 * 2.0) as i32;
        assert_eq!(
            inst[0].position[0],
            1264.0 + (336 / 2 - measured / 2) as f32
        );
    }

    #[test]
    fn format_credits_matches_the_native_percent_ld() {
        assert_eq!(format_credits(0), "0");
        assert_eq!(format_credits(10000), "10000");
        // `%ld` is signed: a negative total prints its minus sign, and the
        // glyph table has a '-' so it renders rather than falling through to
        // the missing-glyph path.
        assert_eq!(format_credits(-25), "-25");
        let font = digit_font();
        let inst = build_credits_instances(&font, -25, panel(), 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);
        assert_eq!(inst.len(), 3);
        assert_eq!(inst[0].size[0], 4.0, "leading '-' glyph");
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
            &[],
            None,
            &[],
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

    #[test]
    fn credits_follow_the_camera_offset_convention() {
        let font = digit_font();
        let plain = build_credits_instances(&font, 500, panel(), 1.0, [1.0, 1.0, 0.0], [0.0, 0.0]);
        let offset =
            build_credits_instances(&font, 500, panel(), 1.0, [1.0, 1.0, 0.0], [13.25, -7.5]);
        assert_eq!(plain.len(), offset.len());
        for (p, o) in plain.iter().zip(offset.iter()) {
            assert_eq!(o.position[0] - p.position[0], 13.25);
            assert_eq!(o.position[1] - p.position[1], -7.5);
        }
    }
}
