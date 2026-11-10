//! The sidebar's place and size in a match frame.
//!
//! Like gamemd's `g_SidebarSurface`, the sidebar lays out and draws in its own
//! native pixels, and so does the command bar along the frame's bottom. Both
//! lay out on the frame at the player's Sidebar size (Options): the frame in
//! sidebar pixels, rounded up. The frame shows the sidebar's column flush with
//! its top and right edges, and the bar's strip flush with its bottom edge and
//! the column, each scaled by the Sidebar size; the battlefield takes the
//! rest. At Normal it is all the frame's own pixels.
//!
//! Layout space is that frame in sidebar pixels: the column's left edge at
//! `layout width - column width` (168 in the original; a layout may give
//! another), the strip's top at `layout height - 32`. At Normal it
//! is the frame space itself, so only Large and XL map points and rects
//! between the two.
//!
//! A zoomed match enlarges the whole frame into the window. The surfaces then
//! draw at the window's pixels instead (`in_window`): each sidebar pixel as a
//! block of as many whole window pixels as it covers. The frame still gets
//! them at its own size, and the window copy (`app::presentation::
//! sidebar_surface`) shows them over the zoomed frame. Layout, hit tests and
//! the native coordinates stay in sidebar pixels.

use super::Rect;

/// The sidebar column's native width (`6A5090`), in sidebar pixels.
pub const SIDEBAR_WIDTH_PX: u32 = 168;

/// Stock YR reserves the right sidebar and a fixed 32-pixel bottom strip, the
/// command bar's, when it builds the tactical view rectangle. In sidebar
/// pixels.
pub const BOTTOM_STRIP_HEIGHT_PX: u32 = 32;

/// A screen edge a panel sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Where the sidebar's column and the command bar sit in the frame. The
/// original's: the column on the right, the bar along the bottom of what the
/// column leaves, and the battlefield in the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelPlaces {
    /// The column's edge: `Left`, or anything else for the right.
    pub side: Edge,
    /// The column lies over the battlefield and takes no room from it or
    /// from the bar.
    pub side_floats: bool,
    /// The bar's edge: `Top`, anything else for the bottom, or `None` for a
    /// layout without a bar.
    pub bar: Option<Edge>,
}

impl PanelPlaces {
    /// The original interface's places.
    pub const ORIGINAL: Self = Self {
        side: Edge::Right,
        side_floats: false,
        bar: Some(Edge::Bottom),
    };
}

impl Default for PanelPlaces {
    fn default() -> Self {
        Self::ORIGINAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarFrame {
    frame_width: u32,
    frame_height: u32,
    /// Frame pixels per 100 sidebar pixels: 100 at Normal.
    percent: u32,
    /// The column's width in sidebar pixels: [`SIDEBAR_WIDTH_PX`] unless the
    /// side's layout gives another.
    column_width: u32,
    /// Where the side's layout puts the column and the bar.
    places: PanelPlaces,
    /// Surface pixels per sidebar pixel (`in_window`): 1 unless zoomed.
    draw_scale: u32,
    /// Window pixels per 100 frame pixels: the match zoom, 100 unzoomed.
    zoom_percent: u32,
}

impl SidebarFrame {
    pub fn new(frame_width: u32, frame_height: u32, percent: u32) -> Self {
        Self {
            frame_width,
            frame_height,
            percent: percent.max(100),
            column_width: SIDEBAR_WIDTH_PX,
            places: PanelPlaces::default(),
            draw_scale: 1,
            zoom_percent: 100,
        }
    }

    /// This frame with the column as wide as the side's layout makes it.
    pub fn with_column_width(self, column_width: u32) -> Self {
        Self {
            column_width: column_width.max(1),
            ..self
        }
    }

    /// The column's width in sidebar pixels.
    pub fn column_width(self) -> u32 {
        self.column_width
    }

    /// This frame with the column and the bar where the side's layout puts
    /// them.
    pub fn with_places(self, places: PanelPlaces) -> Self {
        Self { places, ..self }
    }

    pub fn places(self) -> PanelPlaces {
        self.places
    }

    fn column_on_left(self) -> bool {
        self.places.side == Edge::Left
    }

    fn bar_on_top(self) -> bool {
        self.places.bar == Some(Edge::Top)
    }

    /// The frame width the column takes from the battlefield and the bar:
    /// none when it floats over them.
    fn docked_width_in_frame(self) -> u32 {
        if self.places.side_floats {
            0
        } else {
            self.width_in_frame()
        }
    }

    /// This frame as a window shows it at `zoom_percent`: the surfaces draw
    /// with as many whole window pixels per sidebar pixel as the zoom and the
    /// size give, at least one. Large keeps its own pixels unless `sharpen`
    /// (the HD radar or the smooth sidebar, which draw at the window's): in
    /// the window's, the seams it blends between pixels would turn sharp.
    pub fn in_window(self, zoom_percent: u32, sharpen: bool) -> Self {
        let zoom_percent = zoom_percent.max(100);
        let draw_scale = if self.is_sharp() || sharpen {
            (self.percent * zoom_percent / 10_000).max(1)
        } else {
            1
        };
        Self {
            draw_scale,
            zoom_percent,
            ..self
        }
    }

    /// The native size, Normal: what captures and native comparisons use.
    pub fn native(frame_width: u32, frame_height: u32) -> Self {
        Self::new(frame_width, frame_height, 100)
    }

    /// Frame pixels per sidebar pixel.
    pub fn scale(self) -> f32 {
        self.percent as f32 / 100.0
    }

    /// Whether every sidebar pixel is a whole block of frame pixels.
    pub fn is_sharp(self) -> bool {
        self.percent.is_multiple_of(100)
    }

    /// The column's width in frame pixels: 168 at Normal in the original.
    pub fn width_in_frame(self) -> u32 {
        self.column_width * self.percent / 100
    }

    /// The bar's strip height in frame pixels: 32 at Normal, none without a
    /// bar.
    pub fn strip_height_in_frame(self) -> u32 {
        if self.places.bar.is_some() {
            BOTTOM_STRIP_HEIGHT_PX * self.percent / 100
        } else {
            0
        }
    }

    /// The battlefield: the frame less the column and the strip. A degenerate
    /// frame keeps one pixel, so its scissor never drops every tactical draw.
    pub fn tactical_size(self) -> (u32, u32) {
        (
            self.frame_width
                .saturating_sub(self.docked_width_in_frame())
                .max(1),
            self.frame_height
                .saturating_sub(self.strip_height_in_frame())
                .max(1),
        )
    }

    /// The battlefield's place in the frame, in frame pixels: right of a
    /// column docked on the left and under a bar on top.
    pub fn tactical_rect(self) -> (u32, u32, u32, u32) {
        let (width, height) = self.tactical_size();
        let x = if self.column_on_left() {
            self.docked_width_in_frame().min(self.frame_width)
        } else {
            0
        };
        let y = if self.bar_on_top() {
            self.strip_height_in_frame().min(self.frame_height)
        } else {
            0
        };
        (x, y, width, height)
    }

    /// The frame in sidebar pixels, rounded up so the sidebar and the bar
    /// reach the frame's edges.
    pub fn layout_size(self) -> (u32, u32) {
        (
            (self.frame_width * 100).div_ceil(self.percent).max(1),
            (self.frame_height * 100).div_ceil(self.percent).max(1),
        )
    }

    /// [`Self::layout_size`] as the screen the native layout (`6A5130`) runs
    /// on.
    pub fn layout_screen(self) -> (f32, f32) {
        let (width, height) = self.layout_size();
        (width as f32, height as f32)
    }

    /// Layout-space x of the column's left edge.
    pub fn layout_left(self) -> f32 {
        if self.column_on_left() {
            0.0
        } else {
            self.layout_size().0 as f32 - self.column_width as f32
        }
    }

    /// Layout-space y of the strip's top edge.
    pub fn strip_layout_top(self) -> f32 {
        if self.bar_on_top() {
            0.0
        } else {
            self.layout_size().1 as f32 - BOTTOM_STRIP_HEIGHT_PX as f32
        }
    }

    /// Layout-space x of the strip's left edge: past a column docked on the
    /// left.
    pub fn bar_layout_left(self) -> f32 {
        if self.column_on_left() && !self.places.side_floats {
            self.column_width as f32
        } else {
            0.0
        }
    }

    /// Whether the layout has the command bar.
    pub fn has_bar(self) -> bool {
        self.places.bar.is_some()
    }

    /// The sidebar's surface in its own pixels: the column's width and the
    /// layout's height.
    pub fn surface_size(self) -> (u32, u32) {
        (self.column_width, self.layout_size().1)
    }

    /// The bar's surface in its own pixels: the layout's width beside the
    /// column (all of it when the column floats), and the strip's native
    /// height.
    pub fn bar_surface_size(self) -> (u32, u32) {
        let beside = if self.places.side_floats {
            0
        } else {
            self.column_width
        };
        (
            self.layout_size().0.saturating_sub(beside).max(1),
            BOTTOM_STRIP_HEIGHT_PX,
        )
    }

    /// Surface pixels per sidebar pixel.
    pub fn draw_scale(self) -> u32 {
        self.draw_scale
    }

    /// [`Self::surface_size`] in surface pixels: what the sidebar draws into.
    pub fn surface_draw_size(self) -> (u32, u32) {
        let (width, height) = self.surface_size();
        (width * self.draw_scale, height * self.draw_scale)
    }

    /// [`Self::bar_surface_size`] in surface pixels.
    pub fn bar_draw_size(self) -> (u32, u32) {
        let (width, height) = self.bar_surface_size();
        (width * self.draw_scale, height * self.draw_scale)
    }

    /// The copy into the frame: frame pixels per surface pixel, below one
    /// when the surfaces draw at the window's pixels.
    pub fn copy_scale(self) -> f32 {
        self.percent as f32 / (100 * self.draw_scale) as f32
    }

    /// Whether every surface pixel is a whole block of frame pixels.
    pub fn copy_is_sharp(self) -> bool {
        self.percent.is_multiple_of(100 * self.draw_scale)
    }

    /// Whether the surfaces hold more pixels than the frame shows of them:
    /// then the window copy shows them over the zoomed frame.
    pub fn has_window_copy(self) -> bool {
        100 * self.draw_scale > self.percent
    }

    /// The window copy: window pixels per surface pixel, 1 when the size
    /// and the zoom give whole window pixels per sidebar pixel.
    pub fn window_copy_scale(self) -> f32 {
        (self.percent * self.zoom_percent) as f32 / (10_000 * self.draw_scale) as f32
    }

    /// Whether every surface pixel is a whole block of window pixels.
    pub fn window_copy_is_sharp(self) -> bool {
        (self.percent * self.zoom_percent).is_multiple_of(10_000 * self.draw_scale)
    }

    /// Whether every surface pixel shows as one window pixel: what the
    /// surfaces draw is at the window's pixels.
    pub fn draws_window_pixels(self) -> bool {
        self.percent * self.zoom_percent == 10_000 * self.draw_scale
    }

    /// The column's left edge in frame pixels; left of the frame when the
    /// frame is narrower than the column.
    pub fn frame_left(self) -> f32 {
        if self.column_on_left() {
            0.0
        } else {
            self.frame_width as f32 - self.width_in_frame() as f32
        }
    }

    /// The column's part of the frame, in frame pixels: where the sidebar's
    /// surface is shown.
    pub fn column(self) -> (u32, u32, u32, u32) {
        if self.column_on_left() {
            let width = self.width_in_frame().min(self.frame_width);
            return (0, 0, width, self.frame_height);
        }
        let x = self.frame_width.saturating_sub(self.width_in_frame());
        (x, 0, self.frame_width - x, self.frame_height)
    }

    /// The strip's part of the frame, beside the column, in frame pixels:
    /// where the bar's surface is shown. No rows without a bar.
    pub fn strip(self) -> (u32, u32, u32, u32) {
        let height = self.strip_height_in_frame();
        let (y, height) = if !self.has_bar() {
            (self.frame_height, 0)
        } else if self.bar_on_top() {
            (0, height.min(self.frame_height))
        } else {
            let y = self.frame_height.saturating_sub(height);
            (y, self.frame_height - y)
        };
        let (column_x, _, column_width, _) = self.column();
        let (x, width) = if self.places.side_floats {
            (0, self.frame_width)
        } else if self.column_on_left() {
            (column_width, self.frame_width - column_width)
        } else {
            (0, column_x)
        };
        (x, y, width, height)
    }

    /// The frame pixel where the bar's surface pixel (0, 0) lands; above the
    /// frame when the frame is shorter than the strip.
    pub fn bar_origin(self) -> (f32, f32) {
        // Beside a column docked on the left the bar starts at its edge.
        let x = if self.column_on_left() && !self.places.side_floats {
            self.column().2 as f32
        } else {
            self.x_to_frame(0.0)
        };
        let y = if self.bar_on_top() {
            0.0
        } else {
            self.frame_height as f32 - self.strip_height_in_frame() as f32
        };
        (x, y)
    }

    /// The frame pixel where layout pixel (0, 0) lands for a surface laid out
    /// on the whole layout, an active-game shell: its column on the
    /// sidebar's. Left of the frame when the layout rounded up.
    /// [`Self::point_to_layout`] is its inverse.
    pub fn layout_origin(self) -> (f32, f32) {
        (self.x_to_frame(0.0), 0.0)
    }

    /// Layout x in frame pixels: the column and the bar are flush with the
    /// frame's right edge.
    fn x_to_frame(self, x: f32) -> f32 {
        self.frame_left() + (x - self.layout_left()) * self.scale()
    }

    /// A rect of the sidebar's, in layout space, in frame pixels.
    pub fn rect_to_frame(self, rect: Rect) -> Rect {
        let scale = self.scale();
        Rect {
            x: self.x_to_frame(rect.x),
            y: rect.y * scale,
            w: rect.w * scale,
            h: rect.h * scale,
        }
    }

    /// A rect of the bar's, in layout space, in frame pixels.
    pub fn bar_rect_to_frame(self, rect: Rect) -> Rect {
        let scale = self.scale();
        let (origin_x, origin_y) = self.bar_origin();
        Rect {
            x: origin_x + (rect.x - self.bar_layout_left()) * scale,
            y: origin_y + (rect.y - self.strip_layout_top()) * scale,
            w: rect.w * scale,
            h: rect.h * scale,
        }
    }

    /// A frame point in the sidebar's layout space.
    pub fn point_to_layout(self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.scale();
        (
            self.layout_left() + (x - self.frame_left()) / scale,
            y / scale,
        )
    }

    /// The layout pixel a whole-layout copy shows at frame pixel (x, y): the
    /// one under its centre, as the copy samples it at the whole sizes. Hit
    /// tests on whole pixels use it to land where the layout is drawn.
    pub fn pixel_to_layout(self, x: i32, y: i32) -> (i32, i32) {
        let (x, y) = self.point_to_layout(x as f32 + 0.5, y as f32 + 0.5);
        (x.floor() as i32, y.floor() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn normal_is_the_frame_itself() {
        let frame = SidebarFrame::new(800, 600, 100);
        assert_eq!(frame.width_in_frame(), 168);
        assert_eq!(frame.column(), (632, 0, 168, 600));
        assert_eq!(frame.strip(), (0, 568, 632, 32));
        assert_eq!(frame.tactical_size(), (632, 568));
        assert_eq!(frame.surface_size(), (168, 600));
        assert_eq!(frame.bar_surface_size(), (632, 32));
        assert_eq!(frame.layout_screen(), (800.0, 600.0));
        assert_eq!(frame.bar_origin(), (0.0, 568.0));
        let cameo = rect(648.0, 49.0, 140.0, 108.0);
        assert_eq!(frame.rect_to_frame(cameo), cameo);
        let button = rect(32.0, 568.0, 52.0, 32.0);
        assert_eq!(frame.bar_rect_to_frame(button), button);
        assert_eq!(frame.point_to_layout(700.5, 99.5), (700.5, 99.5));
        assert_eq!(frame.layout_origin(), (0.0, 0.0));
        assert!(frame.is_sharp());
    }

    #[test]
    fn larger_sizes_scale_the_sidebar_and_bar_from_the_corner() {
        let large = SidebarFrame::new(1920, 1080, 150);
        assert_eq!(large.width_in_frame(), 252);
        assert_eq!(large.strip_height_in_frame(), 48);
        assert_eq!(large.column(), (1668, 0, 252, 1080));
        assert_eq!(large.strip(), (0, 1032, 1668, 48));
        assert_eq!(large.tactical_size(), (1668, 1032));
        assert_eq!(large.layout_screen(), (1280.0, 720.0));
        assert_eq!(large.surface_size(), (168, 720));
        assert_eq!(large.bar_surface_size(), (1112, 32));
        assert!(!large.is_sharp());
        // The whole layout column and strip become the frame's.
        assert_eq!(
            large.rect_to_frame(rect(1112.0, 0.0, 168.0, 720.0)),
            rect(1668.0, 0.0, 252.0, 1080.0)
        );
        assert_eq!(
            large.bar_rect_to_frame(rect(0.0, 688.0, 1112.0, 32.0)),
            rect(0.0, 1032.0, 1668.0, 48.0)
        );
        assert_eq!(large.point_to_layout(1818.0, 300.0), (1212.0, 200.0));
        assert_eq!(large.layout_origin(), (0.0, 0.0));

        // An odd frame rounds the layout up: the column reaches the bottom
        // edge, and the bar still sits on it.
        let xl = SidebarFrame::new(1921, 1081, 200);
        assert_eq!(xl.layout_size(), (961, 541));
        assert_eq!(xl.surface_size(), (168, 541));
        assert_eq!(xl.strip(), (0, 1017, 1585, 64));
        assert_eq!(xl.bar_origin(), (-1.0, 1017.0));
        // A whole-layout surface shares the column's corner, and the frame's
        // pixels map back onto its own.
        assert_eq!(xl.layout_origin(), (-1.0, 0.0));
        assert_eq!(xl.point_to_layout(-1.0, 0.0), (0.0, 0.0));
        assert_eq!(xl.point_to_layout(1920.0, 1080.0), (960.5, 540.0));
        // A frame shorter than the strip keeps a one-pixel battlefield.
        let tiny = SidebarFrame::native(100, 20);
        assert_eq!(tiny.tactical_size(), (1, 1));
        assert_eq!(tiny.strip(), (0, 0, 0, 20));
        assert_eq!(tiny.bar_origin(), (0.0, -12.0));
        assert_eq!(
            xl.bar_rect_to_frame(rect(10.0, 509.0, 52.0, 32.0)),
            rect(19.0, 1017.0, 104.0, 64.0)
        );
        assert!(xl.is_sharp());
    }

    /// A column on the left and a bar on top move the battlefield; a floating
    /// column and no bar give it the whole frame.
    #[test]
    fn the_column_and_the_bar_can_change_edges_or_leave_the_battlefield_whole() {
        let left = SidebarFrame::native(800, 600).with_places(PanelPlaces {
            side: Edge::Left,
            side_floats: false,
            bar: Some(Edge::Top),
        });
        assert_eq!(left.column(), (0, 0, 168, 600));
        assert_eq!(left.strip(), (168, 0, 632, 32));
        assert_eq!(left.tactical_rect(), (168, 32, 632, 568));
        assert_eq!((left.layout_left(), left.frame_left()), (0.0, 0.0));
        assert_eq!(left.bar_origin(), (168.0, 0.0));
        // The bar lays out from the column's edge and the frame's top.
        let button = rect(200.0, 0.0, 52.0, 32.0);
        assert_eq!(left.bar_rect_to_frame(button), button);
        assert_eq!(left.bar_surface_size(), (632, 32));
        // Large scales both from the corner they sit in.
        let large = SidebarFrame::new(1920, 1080, 150).with_places(left.places());
        assert_eq!(large.column(), (0, 0, 252, 1080));
        assert_eq!(large.tactical_rect(), (252, 48, 1668, 1032));
        assert_eq!(
            large.bar_rect_to_frame(rect(168.0, 0.0, 1112.0, 32.0)),
            rect(252.0, 0.0, 1668.0, 48.0)
        );

        let over = SidebarFrame::native(800, 600).with_places(PanelPlaces {
            side: Edge::Right,
            side_floats: true,
            bar: None,
        });
        assert_eq!(over.tactical_rect(), (0, 0, 800, 600));
        assert_eq!(over.column(), (632, 0, 168, 600));
        assert_eq!(over.strip().3, 0);
        // The original's places are the default.
        let stock = SidebarFrame::native(800, 600);
        assert_eq!(stock.with_places(PanelPlaces::default()), stock);
        assert_eq!(stock.tactical_rect(), (0, 0, 632, 568));
    }

    #[test]
    fn a_whole_layout_surface_puts_its_column_on_the_sidebar() {
        for (width, height) in [(800, 600), (1366, 768), (1920, 1080), (1921, 1081)] {
            for percent in [100, 150, 200] {
                let frame = SidebarFrame::new(width, height, percent);
                let (x, y) = frame.layout_origin();
                assert_eq!(x + frame.layout_left() * frame.scale(), frame.frame_left());
                assert_eq!(y, 0.0);
                assert_eq!(frame.point_to_layout(x, y), (0.0, 0.0));
            }
        }
    }

    /// Zoomed frames draw the sidebar in whole window pixels per sidebar
    /// pixel and shrink it into the frame; Large keeps its blended look.
    #[test]
    fn zoomed_frames_draw_the_surfaces_at_the_window_pixels() {
        // Normal at 4K Auto (300%): 3x3 window pixels per sidebar pixel.
        let normal = SidebarFrame::new(1280, 720, 100).in_window(300, false);
        assert_eq!(normal.draw_scale(), 3);
        assert_eq!(normal.surface_draw_size(), (504, 2160));
        assert_eq!(normal.bar_draw_size(), (3336, 96));
        assert_eq!(normal.copy_scale(), 1.0 / 3.0);
        assert!(!normal.copy_is_sharp() && normal.has_window_copy());
        assert_eq!(normal.window_copy_scale(), 1.0);
        assert!(normal.window_copy_is_sharp() && normal.draws_window_pixels());
        // A zoom between whole steps blends in the window, as the zoom did.
        let between = SidebarFrame::new(1024, 576, 100).in_window(250, false);
        assert_eq!(
            (between.draw_scale(), between.window_copy_scale()),
            (2, 1.25)
        );
        assert!(!between.window_copy_is_sharp() && !between.draws_window_pixels());
        // Unzoomed XL draws 2x and copies 1:1; nothing more to show.
        let xl = SidebarFrame::new(1920, 1080, 200).in_window(100, false);
        assert_eq!((xl.draw_scale(), xl.copy_scale()), (2, 1.0));
        assert!(xl.copy_is_sharp() && !xl.has_window_copy() && xl.draws_window_pixels());
        // Large keeps its own pixels, unless asked to sharpen.
        let large = SidebarFrame::new(1280, 720, 150);
        assert_eq!(large.in_window(200, false).draw_scale(), 1);
        assert!(!large.in_window(200, false).has_window_copy());
        assert!(!large.in_window(200, false).draws_window_pixels());
        assert!(!large.in_window(100, true).draws_window_pixels());
        let sharp = large.in_window(200, true);
        assert_eq!((sharp.draw_scale(), sharp.copy_scale()), (3, 0.5));
        assert!(sharp.window_copy_is_sharp() && sharp.draws_window_pixels());
        // Layout stays in sidebar pixels.
        assert_eq!(
            normal.layout_size(),
            SidebarFrame::native(1280, 720).layout_size()
        );
        assert_eq!(normal.surface_size(), (168, 720));
    }

    #[test]
    fn frame_pixels_hit_the_layout_pixel_the_copy_shows() {
        let normal = SidebarFrame::native(800, 600);
        assert_eq!(normal.pixel_to_layout(37, 41), (37, 41));
        // XL shows each layout pixel as a 2x2 block from the corner.
        let xl = SidebarFrame::new(1920, 1080, 200);
        assert_eq!(xl.pixel_to_layout(38, 40), (19, 20));
        assert_eq!(xl.pixel_to_layout(39, 41), (19, 20));
        assert_eq!(xl.pixel_to_layout(40, 42), (20, 21));
        // An odd frame starts the blocks one pixel left of the frame.
        let odd = SidebarFrame::new(1921, 1081, 200);
        assert_eq!(odd.pixel_to_layout(0, 0), (0, 0));
        assert_eq!(odd.pixel_to_layout(1, 1), (1, 0));
        assert_eq!(odd.pixel_to_layout(2, 2), (1, 1));
        // Large gives a seam pixel to the layout pixel under its centre.
        let large = SidebarFrame::new(1920, 1080, 150);
        assert_eq!(large.pixel_to_layout(0, 0), (0, 0));
        assert_eq!(large.pixel_to_layout(1, 1), (1, 1));
        assert_eq!(large.pixel_to_layout(2, 2), (1, 1));
        assert_eq!(large.pixel_to_layout(3, 3), (2, 2));
    }
}
