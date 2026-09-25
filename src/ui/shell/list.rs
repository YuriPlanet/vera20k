//! Shared native list and scrollbar geometry from 0x00618D40 / 0x0061C690.
//! Native list rows use GAME.FNT height 17 + 2; inner rectangles exclude borders.

use super::geom::RectPx;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListScrollPart {
    Up,
    Down,
    Thumb,
    Track,
}

pub const ROW_HEIGHT: i32 = 19;

/// Scrollbar capture and repeat from original gamemd61C690: button down arms
/// 500 ms at61D383..61D3B2; each timer callback rearms25 ms at61D215..61D2C8,
/// including callbacks outside the arrows. Release61D2D3..61D310 kills capture.
/// Geometry remains the shared owner of pointer admission and thumb projection.
#[derive(Debug, Clone, Default)]
pub struct ListScrollInteraction {
    captured: Option<ListScrollPart>,
    hovered: Option<ListScrollPart>,
    repeat_at: Option<Instant>,
}
impl ListScrollInteraction {
    pub fn press(
        &mut self,
        part: ListScrollPart,
        geometry: ShellListGeometry,
        top: &mut usize,
        y: i32,
        now: Instant,
    ) {
        self.captured = Some(part);
        self.hovered = Some(part);
        self.repeat_at = Some(now + Duration::from_millis(500));
        match part {
            ListScrollPart::Up | ListScrollPart::Down => Self::step(part, geometry.max_top, top),
            ListScrollPart::Track => *top = geometry.top_at_pointer(y),
            // 61D4AD..61D4C2 captures the thumb without recentering it on down.
            ListScrollPart::Thumb => {}
        }
    }
    pub fn pointer_moved(&mut self, geometry: ShellListGeometry, top: &mut usize, x: i32, y: i32) {
        self.hovered = geometry.scroll_part_at(x, y);
        if self.captured == Some(ListScrollPart::Thumb) {
            *top = geometry.top_at_pointer(y);
        }
    }
    /// One admitted timer callback, never a catch-up burst. Direction follows
    /// the current arrow, even when capture began on the track or other arrow.
    pub fn poll(
        &mut self,
        geometry: ShellListGeometry,
        top: &mut usize,
        x: i32,
        y: i32,
        now: Instant,
    ) -> bool {
        let before = (*top, self.pressed_part());
        self.hovered = geometry.scroll_part_at(x, y);
        if self.repeat_at.is_some_and(|deadline| now >= deadline) {
            if let Some(part) = self.hovered {
                Self::step(part, geometry.max_top, top);
            }
            self.repeat_at = Some(now + Duration::from_millis(25));
        }
        before != (*top, self.pressed_part())
    }
    fn step(part: ListScrollPart, max_top: usize, top: &mut usize) {
        match part {
            ListScrollPart::Up => *top = top.saturating_sub(1),
            ListScrollPart::Down => *top = top.saturating_add(1).min(max_top),
            _ => {}
        }
    }
    pub fn pressed_part(&self) -> Option<ListScrollPart> {
        match (self.captured, self.hovered) {
            (Some(ListScrollPart::Thumb) | None, _) => None,
            (_, Some(part @ (ListScrollPart::Up | ListScrollPart::Down))) => Some(part),
            _ => None,
        }
    }
    pub fn repeat_at(&self) -> Option<Instant> {
        self.repeat_at
    }
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ShellListGeometry {
    pub outer: RectPx,
    pub content: RectPx,
    pub scrollbar: Option<RectPx>,
    pub thumb: Option<RectPx>,
    pub visible_rows: usize,
    pub max_top: usize,
}
impl ShellListGeometry {
    pub fn new(outer: RectPx, count: usize, top: usize) -> Self {
        let inner_height = (outer.h - 2).max(0);
        let visible_rows = (inner_height / ROW_HEIGHT) as usize;
        let max_top = count.saturating_sub(visible_rows);
        let scrollbar =
            (max_top > 0).then(|| RectPx::new(outer.x + outer.w - 21, outer.y, 20, outer.h));
        let content = RectPx::new(
            outer.x + 1,
            outer.y + 1,
            (outer.w - 2 - if scrollbar.is_some() { 20 } else { 0 }).max(0),
            inner_height,
        );
        let thumb = scrollbar.map(|bar| {
            let height = thumb_height(bar.h, max_top);
            let span = (bar.h - 2 - 44 - height).max(1);
            let offset = (span as i64 * top.min(max_top) as i64 / max_top as i64) as i32;
            RectPx::new(bar.x + 1, bar.y + 1 + 22 + offset, 18, height)
        });
        Self {
            outer,
            content,
            scrollbar,
            thumb,
            visible_rows,
            max_top,
        }
    }
    pub fn row(&self, visible: usize) -> RectPx {
        RectPx::new(
            self.content.x,
            self.content.y + visible as i32 * ROW_HEIGHT,
            self.content.w,
            ROW_HEIGHT
                .min(self.content.h - visible as i32 * ROW_HEIGHT)
                .max(0),
        )
    }
    pub fn row_at(&self, count: usize, top: usize, x: i32, y: i32) -> Option<usize> {
        if !self.content.contains(x, y) {
            return None;
        }
        let index = top + ((y - self.content.y) / ROW_HEIGHT) as usize;
        (index < count).then_some(index)
    }
    pub fn scroll_part_at(&self, x: i32, y: i32) -> Option<ListScrollPart> {
        let bar = self.scrollbar?;
        if !bar.contains(x, y) {
            return None;
        }
        let local_y = y - bar.y;
        if x <= bar.x {
            return None;
        }
        if local_y < 22 {
            return Some(ListScrollPart::Up);
        }
        if local_y > bar.h - 2 - 22 {
            return Some(ListScrollPart::Down);
        }
        let thumb = self.thumb?;
        let native_top = thumb.y - bar.y - 1;
        if local_y >= native_top && local_y < native_top + thumb.h {
            Some(ListScrollPart::Thumb)
        } else {
            Some(ListScrollPart::Track)
        }
    }
    pub fn top_at_pointer(&self, y: i32) -> usize {
        let (Some(bar), Some(thumb)) = (self.scrollbar, self.thumb) else {
            return 0;
        };
        let span = (bar.h - 2 - 44 - thumb.h).max(1);
        let numerator = (y - bar.y - 22 - thumb.h / 2).clamp(0, span);
        (numerator as i64 * self.max_top as i64 / span as i64) as usize
    }
}

/// A line tone of the owner-draw list frame (`0x00619230`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFrameTone {
    /// `0xC5BEA7` (`0x00BBGGRR`).
    Light,
    /// `0x807A68`.
    Dark,
    /// `0xA29C87`, the two tones' average, where they meet.
    Corner,
}

impl ListFrameTone {
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            Self::Light => [0xA7, 0xBE, 0xC5],
            Self::Dark => [0x68, 0x7A, 0x80],
            Self::Corner => [0x87, 0x9C, 0xA2],
        }
    }
}

/// The two offset frame rings around list window `window` (`0x00619230`,
/// measured on the retail `0x129` and `0x6B` lists), in paint order. With
/// `R = x + w` and `B = y + h`, the outer ring spans `x-1..=R+1` by
/// `y-1..=B+1` (light top/left, dark bottom/right) and the inner ring spans
/// `x..=R` by `y..=B` with the tones swapped. Each ring's top-right and
/// bottom-left corner takes the average tone.
pub fn list_frame_lines(window: RectPx) -> [(RectPx, ListFrameTone); 12] {
    let ring = |grow: i32, top_left: ListFrameTone, bottom_right: ListFrameTone| {
        let (x0, y0) = (window.x - grow, window.y - grow);
        let (x1, y1) = (window.x + window.w + grow, window.y + window.h + grow);
        [
            (RectPx::new(x0, y0, x1 - x0, 1), top_left),
            (RectPx::new(x0, y0, 1, y1 - y0), top_left),
            (RectPx::new(x1, y0, 1, 1), ListFrameTone::Corner),
            (RectPx::new(x0, y1, 1, 1), ListFrameTone::Corner),
            (RectPx::new(x1, y0 + 1, 1, y1 - y0), bottom_right),
            (RectPx::new(x0 + 1, y1, x1 - x0, 1), bottom_right),
        ]
    };
    let [a, b, c, d, e, f] = ring(1, ListFrameTone::Light, ListFrameTone::Dark);
    let [g, h, i, j, k, l] = ring(0, ListFrameTone::Dark, ListFrameTone::Light);
    [a, b, c, d, e, f, g, h, i, j, k, l]
}

/// The scrollbar child's left edge inside the list frame (`0x0061C690`,
/// measured on the retail 17-row movie list): a light line at `bar.x` and a
/// dark line at `bar.x + 1` between the list's top and bottom rings, then
/// the corner tone painted over each crossing with a ring of the other tone.
pub fn scrollbar_edge_lines(bar: RectPx) -> [(RectPx, ListFrameTone); 5] {
    let (top, bottom) = (bar.y, bar.y + bar.h - 1);
    [
        (
            RectPx::new(bar.x, top, 1, bottom - top),
            ListFrameTone::Light,
        ),
        (
            RectPx::new(bar.x + 1, top - 1, 1, bottom - top + 1),
            ListFrameTone::Dark,
        ),
        (RectPx::new(bar.x, top, 1, 1), ListFrameTone::Corner),
        (RectPx::new(bar.x + 1, top - 1, 1, 1), ListFrameTone::Corner),
        (RectPx::new(bar.x + 1, bottom, 1, 1), ListFrameTone::Corner),
    ]
}

/// The scrollbar's inside, right of its edge lines: the arrows sit at its
/// top and bottom and the grip in its columns. The parent background shows
/// through it, without the list's darkening.
pub fn scrollbar_interior(bar: RectPx) -> RectPx {
    RectPx::new(bar.x + 2, bar.y + 1, bar.w - 2, bar.h - 2)
}

/// Where the scrollbar's arrows go: at the interior's left, the up arrow at
/// its top and the down arrow (`arrow_h` tall) at its bottom.
pub fn scrollbar_arrow_origins(bar: RectPx, arrow_h: i32) -> [(i32, i32); 2] {
    let interior = scrollbar_interior(bar);
    [
        (interior.x, interior.y),
        (interior.x, interior.y + interior.h - arrow_h),
    ]
}

/// The grip middle's tiles: `tile_h` rows each from the thumb top, the last
/// one cut at the thumb bottom, as `(y, h)`. The 2-row caps go over them.
pub fn grip_tiles(thumb: RectPx, tile_h: i32) -> Vec<(i32, i32)> {
    if tile_h <= 0 {
        return Vec::new();
    }
    (thumb.y..thumb.y + thumb.h)
        .step_by(tile_h as usize)
        .map(|y| (y, tile_h.min(thumb.y + thumb.h - y)))
        .collect()
}

/// Whether a list press is the second click of a double-click: within the
/// host double-click time and rectangle of the previous press anywhere in
/// the list. Windows then sends `WM_LBUTTONDBLCLK`, which the list subclass
/// only forwards as `LBN_DBLCLK` (`0x0061A904..0x0061A945`): no selection,
/// no sound. A double-click ends the sequence, so the next press starts a
/// new one.
pub fn is_double_click(
    last_press: &mut Option<(Instant, i32, i32)>,
    now: Instant,
    x: i32,
    y: i32,
    (time, width, height): (Duration, i32, i32),
) -> bool {
    let double = last_press.is_some_and(|(last, px, py)| {
        now.duration_since(last) <= time
            && (x - px).abs() * 2 <= width
            && (y - py).abs() * 2 <= height
    });
    *last_press = if double { None } else { Some((now, x, y)) };
    double
}

/// 0x0061C818 uses natural log with the stored binary64 0.2; 0x007C5F00
/// truncates the result to integer, then the caller enforces the 14 px minimum.
/// f64 arithmetic preserves ordinary control geometry; x87 rounding-edge cases
/// remain outside the sampled native comparison coverage.
pub fn thumb_height(height: i32, range: usize) -> i32 {
    let track = f64::from(height - 2 - 44);
    (track - ((range + 1) as f64).ln() * track * 0.2)
        .trunc()
        .max(14.0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    // Timer expectations are from the61C690 instructions cited above; geometry
    // separately replays the saved native scrollbar fixture below.
    #[test]
    fn held_arrow_waits_500ms_then_steps_each_25ms_without_catch_up() {
        let geometry = ShellListGeometry::new(RectPx::new(10, 20, 400, 255), 60, 10);
        let bar = geometry.scrollbar.unwrap();
        let (x, y) = (bar.x + 10, bar.y + bar.h - 10);
        let now = Instant::now();
        let mut interaction = ListScrollInteraction::default();
        let mut top = 10;
        interaction.press(ListScrollPart::Down, geometry, &mut top, y, now);
        assert_eq!(top, 11);
        assert_eq!(interaction.pressed_part(), Some(ListScrollPart::Down));
        for (elapsed, expected) in [(499, 11), (500, 12), (524, 12), (525, 13), (900, 14)] {
            interaction.poll(
                geometry,
                &mut top,
                x,
                y,
                now + Duration::from_millis(elapsed),
            );
            assert_eq!(top, expected, "at {elapsed}ms");
        }
        assert_eq!(
            interaction.repeat_at(),
            Some(now + Duration::from_millis(925))
        );
    }
    #[test]
    fn leaving_arrow_keeps_timer_cadence_and_capture_until_release_or_focus_loss() {
        let geometry = ShellListGeometry::new(RectPx::new(10, 20, 400, 255), 60, 10);
        let bar = geometry.scrollbar.unwrap();
        let (x, y) = (bar.x + 10, bar.y + bar.h - 10);
        let now = Instant::now();
        let mut interaction = ListScrollInteraction::default();
        let mut top = 10;
        interaction.press(ListScrollPart::Down, geometry, &mut top, y, now);
        interaction.pointer_moved(geometry, &mut top, 0, 0);
        assert_eq!(interaction.pressed_part(), None);
        interaction.poll(geometry, &mut top, 0, 0, now + Duration::from_millis(500));
        assert_eq!(top, 11);
        assert_eq!(
            interaction.repeat_at(),
            Some(now + Duration::from_millis(525))
        );
        interaction.pointer_moved(geometry, &mut top, x, y);
        assert_eq!(interaction.pressed_part(), Some(ListScrollPart::Down));
        interaction.poll(geometry, &mut top, x, y, now + Duration::from_millis(510));
        assert_eq!(top, 11);
        interaction.poll(geometry, &mut top, x, y, now + Duration::from_millis(525));
        assert_eq!(top, 12);
        // Both button release and shell focus loss call this same cleanup.
        interaction.cancel();
        assert_eq!(interaction.repeat_at(), None);
        interaction.poll(geometry, &mut top, x, y, now + Duration::from_secs(2));
        assert_eq!(top, 12);
        assert_eq!(interaction.pressed_part(), None);
    }
    #[test]
    fn repeat_follows_current_arrow_and_clamps_at_each_end() {
        let geometry = ShellListGeometry::new(RectPx::new(10, 20, 400, 255), 60, 0);
        let bar = geometry.scrollbar.unwrap();
        let x = bar.x + 10;
        let now = Instant::now();
        let mut interaction = ListScrollInteraction::default();
        let mut top = 0;
        interaction.press(ListScrollPart::Up, geometry, &mut top, bar.y + 10, now);
        assert_eq!(top, 0);
        interaction.poll(
            geometry,
            &mut top,
            x,
            bar.y + bar.h - 10,
            now + Duration::from_millis(500),
        );
        assert_eq!(top, 1);
        assert_eq!(interaction.pressed_part(), Some(ListScrollPart::Down));
        top = geometry.max_top;
        interaction.poll(
            geometry,
            &mut top,
            x,
            bar.y + bar.h - 10,
            now + Duration::from_millis(525),
        );
        assert_eq!(top, geometry.max_top);
    }
    #[test]
    fn thumb_does_not_jump_on_press_and_track_capture_can_reach_an_arrow() {
        let geometry = ShellListGeometry::new(RectPx::new(10, 20, 400, 255), 60, 10);
        let bar = geometry.scrollbar.unwrap();
        let now = Instant::now();
        let mut interaction = ListScrollInteraction::default();
        let mut top = 10;
        interaction.press(
            ListScrollPart::Thumb,
            geometry,
            &mut top,
            geometry.thumb.unwrap().y + 1,
            now,
        );
        assert_eq!(top, 10);
        interaction.pointer_moved(geometry, &mut top, bar.x + 10, 1000);
        assert_eq!(top, geometry.max_top);
        interaction.cancel();
        interaction.press(ListScrollPart::Track, geometry, &mut top, bar.y + 40, now);
        let track_top = top;
        interaction.pointer_moved(geometry, &mut top, 0, 0);
        assert_eq!(top, track_top);
        interaction.poll(
            geometry,
            &mut top,
            bar.x + 10,
            bar.y + bar.h - 10,
            now + Duration::from_millis(500),
        );
        assert_eq!(top, (track_top + 1).min(geometry.max_top));
    }
    #[test]
    fn thumb_height_matches_native_sampled_geometry() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/saved_scrollbar.json"
        ))
        .unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 450);
        for case in cases {
            assert_eq!(
                thumb_height(
                    case["height"].as_i64().unwrap() as i32,
                    case["range"].as_u64().unwrap() as usize
                ),
                case["thumb"].as_i64().unwrap() as i32
            );
        }
    }
    #[test]
    fn border_and_scroll_range_share_one_geometry_for_hit_and_paint() {
        let list = ShellListGeometry::new(RectPx::new(10, 20, 400, 255), 23, 10);
        assert_eq!(list.visible_rows, 13);
        assert_eq!(list.max_top, 10);
        assert_eq!(list.scrollbar, Some(RectPx::new(389, 20, 20, 255)));
        assert_eq!(list.row_at(23, 10, 13, 22), Some(10));
        assert_eq!(list.top_at_pointer(2000), 10);
        assert_eq!(list.top_at_pointer(-1), 0);
    }
}
