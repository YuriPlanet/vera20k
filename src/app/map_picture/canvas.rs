//! A map picture's geometry and pixels, apart from the renderer: the world
//! rectangle the picture shows, the camera positions whose frames cover it,
//! and the whole-factor shrink of those frames into the picture.

use crate::map::preview::PreviewSourceBounds;

/// The menus' largest picture: four times their 144 x 112 preview box, so a
/// 4K window shows it at about its own size and smaller ones shrink it.
pub(crate) const MAX_PICTURE: (u32, u32) = (576, 448);

/// A rectangle of VERA world pixels at zoom 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorldRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

impl WorldRect {
    /// `[Header]`'s preview rectangle, over which `DrawStartPositions
    /// 0x00640710` places the start markers
    /// (`project_preview_start_positions`). Its units are the loading
    /// projection's screen x / 60 and y / 30 (`loading::composition::
    /// project_cell`), whose screen point is the world point moved 15360 px
    /// east and up by the world's row bias. A picture of exactly this
    /// rectangle keeps the markers on their starts.
    pub(crate) fn of_preview_bounds(bounds: &PreviewSourceBounds) -> Option<Self> {
        let bias = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
        let x = i64::from(bounds.origin_x) * 60 - 15_360 + bias.0 as i64;
        let y = i64::from(bounds.origin_y) * 30 + bias.1 as i64;
        Some(Self {
            x: i32::try_from(x).ok()?,
            y: i32::try_from(y).ok()?,
            w: bounds.width.checked_mul(60)?,
            h: bounds.height.checked_mul(30)?,
        })
    }

    /// The world rectangle the camera can show
    /// (`input::camera::tactical_view_area`), in whole pixels.
    pub(crate) fn of_view_area((x, y, w, h): (f32, f32, f32, f32)) -> Option<Self> {
        let (w, h) = (w.round(), h.round());
        (w >= 1.0 && h >= 1.0).then(|| Self {
            x: x.round() as i32,
            y: y.round() as i32,
            w: w as u32,
            h: h as u32,
        })
    }

    fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.w)
    }

    fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.h)
    }

    /// The part both rectangles cover, if any.
    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > i64::from(x) && bottom > i64::from(y)).then(|| Self {
            x,
            y,
            w: (right - i64::from(x)) as u32,
            h: (bottom - i64::from(y)) as u32,
        })
    }
}

/// One frame of the picture: the camera's top-left to ask for, and the world
/// rectangle this frame alone fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tile {
    pub(crate) camera: (i32, i32),
    pub(crate) fills: WorldRect,
}

/// The frames that cover `wanted` (inside the camera's `view` area) with a
/// battlefield of `visible` pixels, rows of columns. Each camera position is
/// inside the clamp's limits, so `clamp_camera_to_playable_area` keeps it,
/// except on an axis the battlefield covers whole, where the clamp centres
/// the one position. Neighbouring frames fill the gaps between their
/// positions, so no world pixel is counted twice.
pub(crate) fn tiles(wanted: WorldRect, view: WorldRect, visible: (u32, u32)) -> Vec<Tile> {
    let Some(wanted) = wanted.intersect(view) else {
        return Vec::new();
    };
    let columns = axis(wanted.x, wanted.w, view.x, view.w, visible.0.max(1) as i64);
    let rows = axis(wanted.y, wanted.h, view.y, view.h, visible.1.max(1) as i64);
    let mut out = Vec::with_capacity(columns.len() * rows.len());
    for &(camera_y, fill_y, fill_h) in &rows {
        for &(camera_x, fill_x, fill_w) in &columns {
            out.push(Tile {
                camera: (camera_x, camera_y),
                fills: WorldRect {
                    x: fill_x,
                    y: fill_y,
                    w: fill_w,
                    h: fill_h,
                },
            });
        }
    }
    out
}

/// One axis of [`tiles`]: (camera position, first filled pixel, filled length).
fn axis(start: i32, length: u32, low: i32, extent: u32, visible: i64) -> Vec<(i32, i32, u32)> {
    let (start, end) = (i64::from(start), i64::from(start) + i64::from(length));
    let (low, extent) = (i64::from(low), i64::from(extent));
    if extent <= visible {
        // The clamp centres this axis, and that one frame shows all of it.
        return vec![(low as i32, start as i32, length)];
    }
    let high = low + extent - visible;
    let mut position = start.clamp(low, high);
    let mut positions = vec![position];
    while position + visible < end && position < high {
        position = (position + visible).min(high);
        positions.push(position);
    }
    positions
        .iter()
        .enumerate()
        .map(|(index, &position)| {
            let from = position.max(start);
            let to = positions
                .get(index + 1)
                .copied()
                .unwrap_or(end)
                .min(position + visible)
                .min(end);
            (position as i32, from as i32, (to - from) as u32)
        })
        .collect()
}

/// The picture being drawn: `area` shrunk by a whole factor, each pixel the
/// mean of the world pixels it covers, black where no frame reached.
pub(crate) struct Canvas {
    area: WorldRect,
    factor: u32,
    width: u32,
    height: u32,
    /// Each picture pixel's red, green and blue sums.
    sums: Vec<u32>,
}

impl Canvas {
    /// The smallest whole factor that fits `area` in `largest`.
    pub(crate) fn new(area: WorldRect, largest: (u32, u32)) -> Option<Self> {
        let factor = area
            .w
            .div_ceil(largest.0.max(1))
            .max(area.h.div_ceil(largest.1.max(1)))
            .max(1);
        let (width, height) = (area.w / factor, area.h / factor);
        (width > 0 && height > 0).then(|| Self {
            area,
            factor,
            width,
            height,
            sums: vec![0; width as usize * height as usize * 3],
        })
    }

    /// Add the world pixels of `fills` from a frame whose top-left BGRA pixel
    /// shows world point `camera`. `frame` is `frame_width` pixels wide;
    /// only its first `battlefield` columns and rows are the battlefield.
    pub(crate) fn add(
        &mut self,
        fills: WorldRect,
        camera: (i32, i32),
        frame: &[u8],
        frame_width: u32,
        battlefield: (u32, u32),
    ) {
        let drawn = WorldRect {
            x: camera.0,
            y: camera.1,
            w: battlefield.0,
            h: battlefield.1,
        };
        let shrunk = WorldRect {
            w: self.width * self.factor,
            h: self.height * self.factor,
            ..self.area
        };
        let Some(part) = fills
            .intersect(drawn)
            .and_then(|part| part.intersect(shrunk))
        else {
            return;
        };
        let stride = frame_width as usize * 4;
        for world_y in part.y..part.y + part.h as i32 {
            let row = (world_y - camera.1) as usize * stride;
            let out_row = ((world_y - self.area.y) as u32 / self.factor) as usize;
            let row_sums = out_row * self.width as usize * 3;
            for world_x in part.x..part.x + part.w as i32 {
                let at = row + (world_x - camera.0) as usize * 4;
                let Some(bgra) = frame.get(at..at + 4) else {
                    continue;
                };
                let out = row_sums + ((world_x - self.area.x) as u32 / self.factor) as usize * 3;
                self.sums[out] += u32::from(bgra[2]);
                self.sums[out + 1] += u32::from(bgra[1]);
                self.sums[out + 2] += u32::from(bgra[0]);
            }
        }
    }

    /// The world rectangle the picture shows, from its top-left pixel.
    pub(crate) fn area(&self) -> WorldRect {
        self.area
    }

    /// The world pixels across each picture pixel.
    pub(crate) fn factor(&self) -> u32 {
        self.factor
    }

    /// The finished picture: its size and packed RGB.
    pub(crate) fn finish(self) -> (u32, u32, Vec<u8>) {
        let area = self.factor * self.factor;
        let rgb = self
            .sums
            .iter()
            .map(|sum| ((sum + area / 2) / area) as u8)
            .collect();
        (self.width, self.height, rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::preview::PreviewStartPoint;

    fn rect(x: i32, y: i32, w: u32, h: u32) -> WorldRect {
        WorldRect { x, y, w, h }
    }

    #[test]
    fn header_rectangle_puts_each_waypoint_in_its_own_world_cell() {
        // Normandy's [Header]: StartX=208, StartY=55, Width=95, Height=95.
        let bounds = PreviewSourceBounds {
            origin_x: 208,
            origin_y: 55,
            width: 95,
            height: 95,
            start_points: vec![PreviewStartPoint { x: 0, y: 0 }],
        };
        let area = WorldRect::of_preview_bounds(&bounds).unwrap();
        assert_eq!(area, rect(-2880, 1665, 5700, 2850));
        for (rx, ry) in [(30, 70), (55, 41), (90, 12)] {
            let header = crate::app::loading::composition::project_waypoint(
                crate::map::waypoints::Waypoint { index: 0, rx, ry },
            );
            let world = crate::util::lepton::absolute_leptons_to_screen(
                i32::from(rx) * 256 + 128,
                i32::from(ry) * 256 + 128,
                0,
            );
            // The marker's per-mille place over the header rectangle is the
            // waypoint's own header unit, 60 x 30 world pixels.
            assert_eq!(
                (world.0 as i32 - area.x).div_euclid(60),
                header.x - bounds.origin_x,
                "cell {rx},{ry}"
            );
            assert_eq!(
                (world.1 as i32 - area.y).div_euclid(30),
                header.y - bounds.origin_y,
                "cell {rx},{ry}"
            );
        }
    }

    #[test]
    fn tiles_cover_the_wanted_part_of_the_view_once_inside_the_clamp() {
        let view = rect(0, 0, 100, 50);
        // The wanted rectangle reaches past the view's bottom: only the view
        // can be drawn, and the clamp keeps every position at most 60, 0.
        let tiles = tiles(rect(5, 10, 95, 60), view, (40, 60));
        let cameras: Vec<_> = tiles.iter().map(|tile| tile.camera).collect();
        assert_eq!(cameras, [(5, 0), (45, 0), (60, 0)]);
        let fills: Vec<_> = tiles.iter().map(|tile| tile.fills).collect();
        assert_eq!(
            fills,
            [
                rect(5, 10, 40, 40),
                rect(45, 10, 15, 40),
                rect(60, 10, 40, 40)
            ]
        );
    }

    #[test]
    fn canvas_averages_whole_blocks_and_leaves_the_unreached_part_black() {
        let mut canvas = Canvas::new(rect(10, 20, 4, 2), MAX_PICTURE).unwrap();
        assert_eq!((canvas.width, canvas.height, canvas.factor), (4, 2, 1));
        let big = Canvas::new(rect(0, 0, 2304, 900), MAX_PICTURE).unwrap();
        assert_eq!((big.width, big.height, big.factor), (576, 225, 4));

        // A 3 x 1 battlefield in a 4-pixel-wide frame, showing world 9..12, 20.
        let frame = [
            [1, 2, 3, 255],
            [10, 20, 30, 255],
            [40, 50, 60, 255],
            [99, 99, 99, 255],
        ]
        .concat();
        canvas.add(rect(10, 20, 4, 2), (9, 20), &frame, 4, (3, 1));
        let (width, height, rgb) = canvas.finish();
        assert_eq!((width, height), (4, 2));
        assert_eq!(&rgb[..6], &[30, 20, 10, 60, 50, 40]);
        assert!(rgb[6..].iter().all(|&channel| channel == 0));
    }
}
