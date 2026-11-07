//! A map picture's geometry and pixels, apart from the renderer: the world
//! rectangle the picture shows, the camera positions whose frames cover it,
//! the whole-factor shrink of those frames into the picture, and the part of
//! it written out.

use crate::map::preview::PreviewSourceBounds;

/// The menus' largest picture: eight times their 144 x 112 preview box, so
/// the enlarged preview (`frontend::preview_zoom`) shows it at about its own
/// size on a 1080p window; the box shrinks it.
pub(crate) const MAX_PICTURE: (u32, u32) = (1152, 896);

/// A field resource the menus' pictures mark, as map library previews do,
/// so the fields show at preview size: ore yellow, gems purple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resource {
    Ore,
    Gems,
}

impl Resource {
    fn colour(self) -> [u8; 3] {
        match self {
            Self::Ore => [255, 238, 0],
            Self::Gems => [192, 48, 224],
        }
    }
}

/// One ore or gem cell: its centre on the terrain in world pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Field {
    pub(crate) centre: (f32, f32),
    pub(crate) resource: Resource,
}

/// A cell's diamond reaches this far from its centre, in world pixels.
const CELL_HALF: (f32, f32) = (30.0, 15.0);

/// A rectangle of VERA world pixels at zoom 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorldRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

/// A world rectangle with fractional edges, in VERA world pixels at zoom 1:
/// where a map's own preview sits (`super::placement`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Place {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

impl Place {
    pub(crate) fn right(self) -> f64 {
        self.x + self.w
    }

    pub(crate) fn bottom(self) -> f64 {
        self.y + self.h
    }
}

impl From<WorldRect> for Place {
    fn from(rect: WorldRect) -> Self {
        Self {
            x: f64::from(rect.x),
            y: f64::from(rect.y),
            w: f64::from(rect.w),
            h: f64::from(rect.h),
        }
    }
}

impl WorldRect {
    /// `[Header]`'s preview rectangle, over which `DrawStartPositions
    /// 0x00640710` places the start markers
    /// (`project_preview_start_positions`). Its units are the loading
    /// projection's screen x / 60 and y / 30 (`loading::composition::
    /// project_cell`), whose screen point is the world point moved 15360 px
    /// east and up by the world's row bias. A picture of exactly this
    /// rectangle keeps the markers on their starts; the menus draw it when a
    /// map's own preview matches no place (`super::placement`).
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

    /// The smallest rectangle covering both.
    pub(crate) fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            w: (self.right().max(other.right()) - i64::from(x)) as u32,
            h: (self.bottom().max(other.bottom()) - i64::from(y)) as u32,
        }
    }

    /// The whole world pixels `place` touches, if they fit.
    pub(crate) fn covering(place: Place) -> Option<Self> {
        let (x, y) = (place.x.floor(), place.y.floor());
        let (right, bottom) = (place.right().ceil(), place.bottom().ceil());
        Some(Self {
            x: i32::try_from(x as i64).ok()?,
            y: i32::try_from(y as i64).ok()?,
            w: u32::try_from((right - x) as i64).ok()?.max(1),
            h: u32::try_from((bottom - y) as i64).ok()?.max(1),
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

/// The frames that cover `wanted` with a battlefield of `visible` pixels,
/// rows of columns. A map picture's camera goes where it is put, past the
/// playable area's edges too (`clamp_camera_point_for_state`): a map's own
/// preview can show rows the tactical view never scrolls to. Neighbouring
/// frames fill the gaps between their positions, so no world pixel is
/// counted twice.
pub(crate) fn tiles(wanted: WorldRect, visible: (u32, u32)) -> Vec<Tile> {
    let columns = axis(wanted.x, wanted.w, visible.0.max(1) as i64);
    let rows = axis(wanted.y, wanted.h, visible.1.max(1) as i64);
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
fn axis(start: i32, length: u32, visible: i64) -> Vec<(i32, i32, u32)> {
    let (start, end) = (i64::from(start), i64::from(start) + i64::from(length));
    // The last frame ends at the wanted edge rather than past it.
    let high = (end - visible).max(start);
    let mut position = start;
    let mut positions = vec![position];
    while position + visible < end {
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

    /// The finished picture.
    pub(crate) fn finish(self) -> Picture {
        let block = self.factor * self.factor;
        let rgb = self
            .sums
            .iter()
            .map(|sum| ((sum + block / 2) / block) as u8)
            .collect();
        Picture {
            area: self.area,
            factor: self.factor,
            width: self.width,
            height: self.height,
            rgb,
        }
    }
}

/// A finished picture: `width` x `height` packed RGB pixels whose top-left
/// one starts at `area`'s top-left, each `factor` world pixels across.
pub(crate) struct Picture {
    pub(crate) area: WorldRect,
    pub(crate) factor: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgb: Vec<u8>,
}

impl Picture {
    /// Tint each pixel whose centre lies on a field's cell, the 60 x 30
    /// diamond around its centre: three fifths the resource's colour, two
    /// the ground's, so the ground's shading still shows through.
    pub(crate) fn mark_fields(&mut self, fields: &[Field]) {
        let factor = self.factor as f32;
        let (half_w, half_h) = (CELL_HALF.0 / factor, CELL_HALF.1 / factor);
        // A pixel on the edge two cells share takes one mark, not two.
        let mut marks = vec![None; self.width as usize * self.height as usize];
        for field in fields {
            let x = (field.centre.0 - self.area.x as f32) / factor;
            let y = (field.centre.1 - self.area.y as f32) / factor;
            let columns = (x - half_w).floor().max(0.0) as u32
                ..((x + half_w).ceil().max(0.0) as u32).min(self.width);
            let rows = (y - half_h).floor().max(0.0) as u32
                ..((y + half_h).ceil().max(0.0) as u32).min(self.height);
            for row in rows {
                let dy = (row as f32 + 0.5 - y).abs() / half_h;
                for column in columns.clone() {
                    if (column as f32 + 0.5 - x).abs() / half_w + dy <= 1.0 {
                        marks[row as usize * self.width as usize + column as usize] =
                            Some(field.resource);
                    }
                }
            }
        }
        for (pixel, mark) in self.rgb.chunks_exact_mut(3).zip(marks) {
            if let Some(resource) = mark {
                for (channel, colour) in pixel.iter_mut().zip(resource.colour()) {
                    *channel = ((2 * u16::from(*channel) + 3 * u16::from(colour) + 2) / 5) as u8;
                }
            }
        }
    }

    /// The part showing `place`, as packed RGB of the largest size that fits
    /// `largest` with `shape`'s proportions (the part's own when None),
    /// never more pixels than the part has: the whole pixels the place
    /// covers, resized by area. None when the place shows none of them.
    pub(crate) fn part(
        &self,
        place: Place,
        shape: Option<(u32, u32)>,
        largest: (u32, u32),
    ) -> Option<(u32, u32, Vec<u8>)> {
        let edge = |world: f64, origin: i32, pixels: u32| {
            let at = ((world - f64::from(origin)) / f64::from(self.factor)).round();
            at.clamp(0.0, f64::from(pixels)) as usize
        };
        let (left, right) = (
            edge(place.x, self.area.x, self.width),
            edge(place.right(), self.area.x, self.width),
        );
        let (top, bottom) = (
            edge(place.y, self.area.y, self.height),
            edge(place.bottom(), self.area.y, self.height),
        );
        let (columns, rows) = (right.checked_sub(left)?, bottom.checked_sub(top)?);
        if columns == 0 || rows == 0 {
            return None;
        }
        let row_bytes = self.width as usize * 3;
        let mut rgba = Vec::with_capacity(columns * rows * 4);
        for row in self.rgb.chunks_exact(row_bytes).skip(top).take(rows) {
            for pixel in row[left * 3..right * 3].chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        let (shape_w, shape_h) = shape.unwrap_or((columns as u32, rows as u32));
        let bound = (
            largest.0.min(columns as u32).max(1),
            largest.1.min(rows as u32).max(1),
        );
        let scale = (f64::from(bound.0) / f64::from(shape_w.max(1)))
            .min(f64::from(bound.1) / f64::from(shape_h.max(1)));
        let width = ((f64::from(shape_w) * scale).round() as u32).clamp(1, bound.0);
        let height = ((f64::from(shape_h) * scale).round() as u32).clamp(1, bound.1);
        let rgba =
            crate::render::area_scale::scale(&rgba, columns, rows, width as usize, height as usize);
        let rgb = rgba
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        Some((width, height, rgb))
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
    fn tiles_cover_the_wanted_rectangle_once_ending_at_its_edges() {
        // 95 columns take three 40-wide frames, the last ending at the
        // rectangle's right edge; 60 rows fit one frame.
        let tiles = tiles(rect(5, 10, 95, 60), (40, 60));
        let cameras: Vec<_> = tiles.iter().map(|tile| tile.camera).collect();
        assert_eq!(cameras, [(5, 10), (45, 10), (60, 10)]);
        let fills: Vec<_> = tiles.iter().map(|tile| tile.fills).collect();
        assert_eq!(
            fills,
            [
                rect(5, 10, 40, 60),
                rect(45, 10, 15, 60),
                rect(60, 10, 40, 60)
            ]
        );
    }

    #[test]
    fn part_takes_the_covered_pixels_in_the_asked_shape() {
        // A 4 x 2 picture of world (100, 50) at 10 world pixels a pixel.
        let rgb: Vec<u8> = [0, 255, 0, 0, 255, 255, 255, 255]
            .iter()
            .flat_map(|&value| [value; 3])
            .collect();
        let picture = Picture {
            area: rect(100, 50, 40, 20),
            factor: 10,
            width: 4,
            height: 2,
            rgb,
        };
        // World x 114..134 and y 49..57 round to pixels 1 and 2 of the top row.
        let place = Place {
            x: 114.0,
            y: 49.0,
            w: 20.0,
            h: 8.0,
        };
        assert_eq!(
            picture.part(place, None, MAX_PICTURE),
            Some((2, 1, vec![255, 255, 255, 0, 0, 0]))
        );
        // A 1:2 shape takes no more pixels than the 4 x 2 part has.
        let whole = Place::from(rect(100, 50, 40, 20));
        let (width, height, _) = picture.part(whole, Some((1, 2)), MAX_PICTURE).unwrap();
        assert_eq!((width, height), (1, 2));
        assert_eq!(
            picture.part(Place { x: 200.0, ..whole }, None, MAX_PICTURE),
            None
        );
    }

    #[test]
    fn fields_tint_their_cell_diamonds_once_over_the_ground() {
        // A grey 12 x 6 picture of world (0, 0) at 10 world pixels a pixel:
        // a cell's diamond is 6 x 3 pixels around its centre.
        let mut picture = Picture {
            area: rect(0, 0, 120, 60),
            factor: 10,
            width: 12,
            height: 6,
            rgb: vec![100; 12 * 6 * 3],
        };
        let ore = Field {
            centre: (60.0, 30.0),
            resource: Resource::Ore,
        };
        let gems = Field {
            centre: (15.0, 15.0),
            resource: Resource::Gems,
        };
        // The same cell twice still takes one mark.
        picture.mark_fields(&[ore, ore, gems]);
        let pixel = |x: usize, y: usize| &picture.rgb[(y * 12 + x) * 3..][..3];
        assert_eq!(pixel(6, 3), [193, 183, 40]);
        assert_eq!(pixel(7, 2), [193, 183, 40], "inside, near its right point");
        assert_eq!(pixel(8, 3), [100, 100, 100], "past its right point");
        assert_eq!(pixel(6, 4), [100, 100, 100], "below its lower point");
        assert_eq!(pixel(1, 1), [155, 69, 174]);
    }

    #[test]
    fn canvas_averages_whole_blocks_and_leaves_the_unreached_part_black() {
        let mut canvas = Canvas::new(rect(10, 20, 4, 2), MAX_PICTURE).unwrap();
        assert_eq!((canvas.width, canvas.height, canvas.factor), (4, 2, 1));
        let big = Canvas::new(rect(0, 0, 4608, 900), MAX_PICTURE).unwrap();
        assert_eq!((big.width, big.height, big.factor), (1152, 225, 4));

        // A 3 x 1 battlefield in a 4-pixel-wide frame, showing world 9..12, 20.
        let frame = [
            [1, 2, 3, 255],
            [10, 20, 30, 255],
            [40, 50, 60, 255],
            [99, 99, 99, 255],
        ]
        .concat();
        canvas.add(rect(10, 20, 4, 2), (9, 20), &frame, 4, (3, 1));
        let picture = canvas.finish();
        assert_eq!((picture.width, picture.height), (4, 2));
        assert_eq!(&picture.rgb[..6], &[30, 20, 10, 60, 50, 40]);
        assert!(picture.rgb[6..].iter().all(|&channel| channel == 0));
    }
}
