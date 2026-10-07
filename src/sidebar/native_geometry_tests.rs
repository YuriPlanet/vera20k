//! Production geometry compared with original executable outputs.
use super::*;
use crate::rules::object_type::ObjectCategory;
use crate::sim::intern::StringInterner;
use crate::sim::production::BuildOption;
use serde::Deserialize;

#[derive(Deserialize)]
struct LayoutCase {
    screen: [i32; 2],
    side: usize,
    tactical: [i32; 4],
    body: [i32; 4],
    globals: [i32; 15],
}
#[derive(Deserialize)]
struct LayoutPacket {
    cases: Vec<LayoutCase>,
}
#[derive(Deserialize)]
struct ScrollCase {
    screen: [i32; 2],
    side: usize,
    count: usize,
    initial: u8,
    disabled: [u8; 2],
}
#[derive(Deserialize)]
struct ScrollPacket {
    cases: Vec<ScrollCase>,
}
fn spec(side: usize) -> SidebarChromeLayoutSpec {
    SidebarChromeLayoutSpec::for_theme(
        [
            SidebarTheme::Allied,
            SidebarTheme::Soviet,
            SidebarTheme::Yuri,
        ][side],
    )
}
fn view(screen: [i32; 2], side: usize, count: usize, scroll: usize) -> SidebarView {
    let mut interner = StringInterner::new();
    let options: Vec<_> = (0..count)
        .map(|i| BuildOption {
            type_id: interner.intern(&format!("T{i}")),
            display_name: format!("T{i}"),
            cost: 1,
            object_category: ObjectCategory::Building,
            queue_category: ProductionCategory::Building,
            enabled: true,
            reason: None,
        })
        .collect();
    let allied = side == 0;
    let gadget = gadget_flash::SidebarGadgetState::default();
    build_sidebar_view_with_spec(
        spec(side),
        screen[0] as f32,
        screen[1] as f32,
        SidebarTab::Building,
        100,
        0,
        0,
        Some(if allied { [28., 27.] } else { [32., 28.] }),
        &[],
        &options,
        &[],
        None,
        &[],
        scroll,
        Some(&interner),
        &[],
        &gadget,
        Some(if allied { [64., 31.] } else { [52., 32.] }),
        Some(if allied { [64., 31.] } else { [52., 32.] }),
        Some(if allied { [46., 25.] } else { [46., 27.] }),
        Some(if allied { [46., 25.] } else { [46., 27.] }),
        [Some([72., 18.]); 2],
        [0; 4],
    )
}
#[test]
fn native_21_screen_and_theme_layouts_drive_actual_view() {
    let packet: LayoutPacket = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/geometry.json",
    ))
    .unwrap();
    assert_eq!(packet.cases.len(), 21);
    for c in packet.cases {
        let v = view(c.screen, c.side, 100, 0);
        let l = v.layout;
        let g = c.globals;
        assert_eq!(
            [
                l.sidebar_x as i32,
                l.side1_y as i32,
                v.panel_rect.w as i32,
                (v.panel_rect.h - l.side1_y) as i32
            ],
            c.body
        );
        let (tw, th) = crate::app::input::camera::tactical_viewport_size_px(
            c.screen[0] as u32,
            c.screen[1] as u32,
        );
        assert_eq!([0, 0, tw as i32, th as i32], c.tactical);
        assert_eq!(
            [
                v.repair_button.rect.x as i32,
                v.repair_button.rect.y as i32,
                (v.sell_button.rect.x - v.repair_button.rect.x) as i32,
                v.tabs[0].rect.x as i32,
                v.tabs[0].rect.y as i32,
                (v.tabs[1].rect.x - v.tabs[0].rect.x) as i32,
                v.items[0].rect.x as i32,
                l.cameo_grid_top as i32,
                (v.items[1].rect.x - v.items[0].rect.x) as i32,
                (v.items[2].rect.y - v.items[0].rect.y) as i32,
                (l.cameo_grid_bottom - l.cameo_grid_top) as i32,
                v.scroll_down_button.rect.x as i32,
                v.scroll_down_button.rect.y as i32,
                (v.scroll_up_button.rect.x - v.scroll_down_button.rect.x) as i32,
                50
            ],
            g,
            "{:?}/{}",
            c.screen,
            c.side
        );
        assert_eq!(v.items[0].rect.y, g[7] as f32 + 1.0);
        assert_eq!([v.items[0].rect.w, v.items[0].rect.h], [60., 48.]);
        assert!(
            !v.items[0]
                .rect
                .contains(v.items[0].rect.x + 60., v.items[0].rect.y)
        );
        assert!(
            !v.items[0]
                .rect
                .contains(v.items[0].rect.x, v.items[0].rect.y + 48.)
        );
        let empty = view(c.screen, c.side, 0, usize::MAX);
        assert_eq!(empty.layout.side2_tile_count, l.side2_tile_count);
        assert_eq!(empty.scroll_rows, 0);
        assert!(!empty.cycle_owner_button.rect.contains(
            empty.cycle_owner_button.rect.x,
            empty.cycle_owner_button.rect.y
        ));
    }
}
#[test]
fn native_126_scroll_cases_drive_both_disabled_frames() {
    let packet: ScrollPacket = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/scroll.json",
    ))
    .unwrap();
    assert_eq!(packet.cases.len(), 126);
    for c in packet.cases {
        let v = view(c.screen, c.side, c.count, usize::MAX);
        for (button, disabled) in [&v.scroll_down_button, &v.scroll_up_button]
            .into_iter()
            .zip(c.disabled)
        {
            assert_eq!(
                u8::from(button.disabled),
                disabled,
                "{:?}/{} count{} initial{}",
                c.screen,
                c.side,
                c.count,
                c.initial
            );
            assert_eq!(button.frame_index, if disabled != 0 { 2 } else { 0 });
        }
    }
}
#[test]
fn retail_allied_control_capture_anchors_connect_to_hit_rects() {
    let v = view([800, 600], 0, 0, 0);
    assert_eq!(
        v.top_buttons[0].rect,
        Rect {
            x: 643.,
            y: 20.,
            w: 72.,
            h: 18.
        }
    );
    assert_eq!(
        v.top_buttons[1].rect,
        Rect {
            x: 715.,
            y: 20.,
            w: 72.,
            h: 18.
        }
    );
    assert_eq!(
        v.repair_button.rect,
        Rect {
            x: 652.,
            y: 166.,
            w: 64.,
            h: 31.
        }
    );
    assert_eq!(
        v.sell_button.rect,
        Rect {
            x: 716.,
            y: 166.,
            w: 64.,
            h: 31.
        }
    );
    assert_eq!(
        v.scroll_down_button.rect,
        Rect {
            x: 671.,
            y: 534.,
            w: 46.,
            h: 25.
        }
    );
    assert_eq!(
        v.scroll_up_button.rect,
        Rect {
            x: 717.,
            y: 534.,
            w: 46.,
            h: 25.
        }
    );
    assert_eq!(
        radar_minimap_rect(800.),
        Rect {
            x: 648.,
            y: 49.,
            w: 140.,
            h: 108.
        }
    );
}
