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

/// Where the sidebar and the command bar sit in the frame. The original's:
/// the sidebar a column on the right, the bar along the bottom of what the
/// sidebar leaves, and the battlefield in the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelPlaces {
    /// The sidebar's edge. On `Left` or `Right` it is a column as tall as
    /// the frame; on `Top` or `Bottom` a band as wide as the frame.
    pub side: Edge,
    /// The sidebar lies over the battlefield and takes no room from it or
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
    /// The band's height in sidebar pixels, for a sidebar on the top or the
    /// bottom edge.
    band_height: u32,
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
            band_height: SIDEBAR_WIDTH_PX,
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

    /// This frame with a band, a sidebar on the top or the bottom edge, as
    /// tall as the side's layout makes it.
    pub fn with_band_height(self, band_height: u32) -> Self {
        Self {
            band_height: band_height.max(1),
            ..self
        }
    }

    /// This frame with the column and the bar where the side's layout puts
    /// them.
    pub fn with_places(self, places: PanelPlaces) -> Self {
        Self { places, ..self }
    }

    pub fn places(self) -> PanelPlaces {
        self.places
    }

    /// The frame's own size, in frame pixels.
    pub fn frame_size(self) -> (u32, u32) {
        (self.frame_width, self.frame_height)
    }

    fn column_on_left(self) -> bool {
        self.places.side == Edge::Left
    }

    /// Whether the sidebar is a band along the top or the bottom edge.
    pub fn is_band(self) -> bool {
        matches!(self.places.side, Edge::Top | Edge::Bottom)
    }

    fn bar_on_top(self) -> bool {
        self.places.bar == Some(Edge::Top)
    }

    /// The frame width and height the sidebar takes from the battlefield
    /// and the bar: none when it floats over them.
    fn docked_in_frame(self) -> (u32, u32) {
        if self.places.side_floats {
            (0, 0)
        } else if self.is_band() {
            (0, self.height_in_frame())
        } else {
            (self.width_in_frame(), 0)
        }
    }

    /// The frame rows a docked band takes above the bar and the battlefield,
    /// and under them.
    fn band_rows(self) -> (u32, u32) {
        let (_, rows) = self.docked_in_frame();
        match self.places.side {
            Edge::Top => (rows, 0),
            Edge::Bottom => (0, rows),
            Edge::Left | Edge::Right => (0, 0),
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

    /// The sidebar's width in frame pixels: a column's 168 at Normal in the
    /// original, the frame's own for a band.
    pub fn width_in_frame(self) -> u32 {
        if self.is_band() {
            self.frame_width
        } else {
            self.column_width * self.percent / 100
        }
    }

    /// The sidebar's height in frame pixels: the frame's own for a column.
    pub fn height_in_frame(self) -> u32 {
        if self.is_band() {
            self.band_height * self.percent / 100
        } else {
            self.frame_height
        }
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

    /// The battlefield: the frame less the sidebar and the strip. A
    /// degenerate frame keeps one pixel, so its scissor never drops every
    /// tactical draw.
    pub fn tactical_size(self) -> (u32, u32) {
        let (docked_width, docked_height) = self.docked_in_frame();
        (
            self.frame_width.saturating_sub(docked_width).max(1),
            self.frame_height
                .saturating_sub(self.strip_height_in_frame())
                .saturating_sub(docked_height)
                .max(1),
        )
    }

    /// The battlefield's place in the frame, in frame pixels: right of a
    /// column docked on the left, and under a band docked on top and a bar
    /// on top.
    pub fn tactical_rect(self) -> (u32, u32, u32, u32) {
        let (width, height) = self.tactical_size();
        let x = if self.column_on_left() {
            self.docked_in_frame().0.min(self.frame_width)
        } else {
            0
        };
        let bar = if self.bar_on_top() {
            self.strip_height_in_frame()
        } else {
            0
        };
        let y = (self.band_rows().0 + bar).min(self.frame_height);
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

    /// The screen the sidebar's native layout (`6A5130`) runs on:
    /// [`Self::layout_size`], or for a band the layout's width and the
    /// band's own height, so its rows are counted in the band and measured
    /// from its top.
    pub fn layout_screen(self) -> (f32, f32) {
        let (width, _) = self.layout_size();
        (width as f32, self.surface_size().1 as f32)
    }

    /// Layout-space x of the sidebar's left edge.
    pub fn layout_left(self) -> f32 {
        if self.places.side == Edge::Right {
            self.layout_size().0 as f32 - self.column_width as f32
        } else {
            0.0
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

    /// The sidebar's surface in its own pixels: a column's width and the
    /// layout's height, or the layout's width and a band's height.
    pub fn surface_size(self) -> (u32, u32) {
        let (width, height) = self.layout_size();
        if self.is_band() {
            (width, self.band_height.min(height))
        } else {
            (self.column_width, height)
        }
    }

    /// The bar's surface in its own pixels: the layout's width beside a
    /// column (all of it beside a band or when the sidebar floats), and the
    /// strip's native height.
    pub fn bar_surface_size(self) -> (u32, u32) {
        let beside = if self.places.side_floats || self.is_band() {
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

    /// The sidebar's left edge in frame pixels; left of the frame when the
    /// frame is narrower than the column.
    pub fn frame_left(self) -> f32 {
        if self.places.side == Edge::Right {
            self.frame_width as f32 - self.width_in_frame() as f32
        } else {
            0.0
        }
    }

    /// The sidebar's top edge in frame pixels; above the frame when the
    /// frame is shorter than a band on the bottom.
    pub fn frame_top(self) -> f32 {
        if self.places.side == Edge::Bottom {
            self.frame_height as f32 - self.height_in_frame() as f32
        } else {
            0.0
        }
    }

    /// The frame pixel where the sidebar's surface pixel (0, 0) lands.
    pub fn side_origin(self) -> (f32, f32) {
        (self.frame_left(), self.frame_top())
    }

    /// The sidebar's part of the frame, in frame pixels: where its surface
    /// is shown. A column, or a band.
    pub fn column(self) -> (u32, u32, u32, u32) {
        match self.places.side {
            Edge::Left => {
                let width = self.width_in_frame().min(self.frame_width);
                (0, 0, width, self.frame_height)
            }
            Edge::Right => {
                let x = self.frame_width.saturating_sub(self.width_in_frame());
                (x, 0, self.frame_width - x, self.frame_height)
            }
            Edge::Top => {
                let height = self.height_in_frame().min(self.frame_height);
                (0, 0, self.frame_width, height)
            }
            Edge::Bottom => {
                let y = self.frame_height.saturating_sub(self.height_in_frame());
                (0, y, self.frame_width, self.frame_height - y)
            }
        }
    }

    /// The strip's part of the frame, beside a docked sidebar, in frame
    /// pixels: where the bar's surface is shown. No rows without a bar.
    pub fn strip(self) -> (u32, u32, u32, u32) {
        let height = self.strip_height_in_frame();
        let (above, under) = self.band_rows();
        let (y, height) = if !self.has_bar() {
            (self.frame_height, 0)
        } else if self.bar_on_top() {
            let y = above.min(self.frame_height);
            (y, height.min(self.frame_height - y))
        } else {
            let bottom = self.frame_height.saturating_sub(under);
            let y = bottom.saturating_sub(height);
            (y, bottom - y)
        };
        let (x, width) = if self.places.side_floats || self.is_band() {
            (0, self.frame_width)
        } else {
            let (column_x, _, column_width, _) = self.column();
            if self.column_on_left() {
                (column_width, self.frame_width - column_width)
            } else {
                (0, column_x)
            }
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
        let (above, under) = self.band_rows();
        let y = if self.bar_on_top() {
            above as f32
        } else {
            self.frame_height as f32 - under as f32 - self.strip_height_in_frame() as f32
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

    /// A rect of the sidebar's, in layout space, in frame pixels. The
    /// sidebar's layout measures down from its own top edge.
    pub fn rect_to_frame(self, rect: Rect) -> Rect {
        let scale = self.scale();
        Rect {
            x: self.x_to_frame(rect.x),
            y: self.frame_top() + rect.y * scale,
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
            (y - self.frame_top()) / scale,
        )
    }

    /// The layout pixel a whole-layout copy shows at frame pixel (x, y): the
    /// one under its centre, as the copy samples it at the whole sizes. Hit
    /// tests on whole pixels use it to land where the layout is drawn. The
    /// whole layout starts at the frame's top, wherever the sidebar is.
    pub fn pixel_to_layout(self, x: i32, y: i32) -> (i32, i32) {
        let scale = self.scale();
        let x = self.layout_left() + (x as f32 + 0.5 - self.frame_left()) / scale;
        let y = (y as f32 + 0.5) / scale;
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

    /// A sidebar on the top or the bottom edge is a band as wide as the
    /// frame; the bar and the battlefield share what it leaves.
    #[test]
    fn a_band_takes_rows_from_the_bar_and_the_battlefield() {
        let band = |side, bar| {
            SidebarFrame::native(800, 600)
                .with_band_height(160)
                .with_places(PanelPlaces {
                    side,
                    side_floats: false,
                    bar,
                })
        };
        let bottom = band(Edge::Bottom, Some(Edge::Bottom));
        assert_eq!(bottom.column(), (0, 440, 800, 160));
        assert_eq!(bottom.strip(), (0, 408, 800, 32));
        assert_eq!(bottom.tactical_rect(), (0, 0, 800, 408));
        assert_eq!(bottom.surface_size(), (800, 160));
        assert_eq!(bottom.bar_surface_size(), (800, 32));
        // Its layout runs on the band: rows are counted in its own height
        // and measured from its top.
        assert_eq!(bottom.layout_screen(), (800.0, 160.0));
        assert_eq!(bottom.side_origin(), (0.0, 440.0));
        let radar = rect(16.0, 20.0, 140.0, 108.0);
        assert_eq!(bottom.rect_to_frame(radar), rect(16.0, 460.0, 140.0, 108.0));
        assert_eq!(bottom.point_to_layout(16.0, 460.0), (16.0, 20.0));
        assert_eq!(bottom.bar_origin(), (0.0, 408.0));
        // A whole-layout surface still starts at the frame's top.
        assert_eq!(bottom.pixel_to_layout(37, 41), (37, 41));

        let top = band(Edge::Top, Some(Edge::Top));
        assert_eq!(top.column(), (0, 0, 800, 160));
        assert_eq!(top.strip(), (0, 160, 800, 32));
        assert_eq!(top.tactical_rect(), (0, 192, 800, 408));
        assert_eq!(top.side_origin(), (0.0, 0.0));

        // Large scales the band from its edge.
        let large = SidebarFrame::new(1920, 1080, 150)
            .with_band_height(160)
            .with_places(bottom.places());
        assert_eq!(large.column(), (0, 840, 1920, 240));
        assert_eq!(large.strip(), (0, 792, 1920, 48));
        assert_eq!(large.tactical_rect(), (0, 0, 1920, 792));
        assert_eq!(large.surface_size(), (1280, 160));
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
