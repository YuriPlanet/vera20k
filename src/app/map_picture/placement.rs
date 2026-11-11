//! Where a map's own preview (`[PreviewPack]`) sits in the world, so VERA's
//! picture can take its exact place in the setup screens' preview box and
//! the map stays put when the picture replaces it.
//!
//! The tools that make previews frame them differently. Matching the
//! previews of 23 maps (8 official, 15 from the map library) against VERA's
//! drawing of each whole map found three kinds:
//! - rendered pictures show LocalSize's columns from two rows above its top,
//!   either at one scale or stretched to LocalSize's bottom or to the bottom
//!   of all the camera can show;
//! - radar-style minimaps (Westwood's maps, FinalAlert 2) show 30 x 30 world
//!   pixels per pixel from the top-left of the whole map or of LocalSize,
//!   drawn flat, so VERA draws raised ground higher than they do;
//! - fixed-size thumbnails that crop a little more.
//!
//! So [`locate`] refines each kind's rectangle ([`candidates`]) by matching
//! the preview against the drawing within a tenth of its size, and keeps the
//! best match, unless the preview does not look like the map (custom
//! artwork).

use super::canvas::{Picture, Place, WorldRect};
use crate::map::map_file::MapHeader;
use crate::map::preview::DecodedPreview;

/// How far a candidate's edges may move while it is refined, as a part of
/// its size.
const REACH: f64 = 0.1;
/// The preview is matched at most this many pixels wide, shrunk by a whole
/// factor.
const MATCH_WIDTH: u32 = 256;
/// The least correlation a match needs. Custom artwork scored at most 0.06
/// at its best place, the weakest true match (a minimap) 0.38.
const LEAST_MATCH: f64 = 0.25;

/// The rectangles each kind of preview `preview` pixels in size shows on the
/// map of `header`, whose camera can show `view` (`tactical_view_area`).
pub(crate) fn candidates(header: &MapHeader, view: WorldRect, preview: (u32, u32)) -> Vec<Place> {
    let (width, height) = (f64::from(preview.0), f64::from(preview.1));
    if width < 1.0 || height < 1.0 {
        return Vec::new();
    }
    let view = Place::from(view);
    // LocalSize's top is 75 pixels below the camera's (`tactical_scroll_rect`
    // starts five half rows above it); renderers start two rows above that.
    let local_top = view.y + 75.0;
    let rendered_top = local_top - 60.0;
    let local_rows = f64::from(header.local_height) + 2.0;
    // The whole map (`[Map] Size`), LocalSize's left columns and top rows
    // out from it.
    let whole = Place {
        x: view.x - f64::from(header.local_left) * 60.0,
        y: local_top - f64::from(header.local_top) * 30.0,
        w: f64::from(header.width) * 60.0,
        h: f64::from(header.height) * 30.0,
    };
    let mut out = vec![
        Place {
            h: height * view.w / width,
            y: rendered_top,
            ..view
        },
        Place {
            y: rendered_top,
            h: local_rows * 30.0,
            ..view
        },
        Place {
            y: rendered_top,
            ..view
        },
    ];
    // A minimap's pixel is half a 60-pixel cell across and a 30-pixel row
    // down, so one larger than its rectangle by more than a cell is no
    // minimap of it.
    let minimap = (width * 30.0, height * 30.0);
    if minimap.0 <= whole.w + 60.0 && minimap.1 <= whole.h + 60.0 {
        out.push(Place {
            w: minimap.0,
            h: minimap.1,
            ..whole
        });
    }
    if minimap.0 <= view.w + 60.0 && minimap.1 <= local_rows * 30.0 + 60.0 {
        out.push(Place {
            x: view.x,
            y: local_top,
            w: minimap.0,
            h: minimap.1,
        });
    }
    // A place reaching well past the whole map, as an odd preview's size
    // makes it, would cost frames of nothing to match.
    let margin = (whole.w * REACH, whole.h * REACH);
    out.retain(|place| {
        place.x >= whole.x - margin.0
            && place.y >= whole.y - margin.1
            && place.right() <= whole.right() + margin.0
            && place.bottom() <= whole.bottom() + margin.1
    });
    out
}

/// The world rectangle a match draws: every candidate with the reach of its
/// refinement.
pub(crate) fn search_area(candidates: &[Place]) -> Option<WorldRect> {
    candidates
        .iter()
        .filter_map(|place| {
            WorldRect::covering(Place {
                x: place.x - place.w * REACH,
                y: place.y - place.h * REACH,
                w: place.w * (1.0 + 2.0 * REACH),
                h: place.h * (1.0 + 2.0 * REACH),
            })
        })
        .reduce(WorldRect::union)
}

/// Where `preview` sits in `drawn`, refined from each of `candidates`, or
/// None when no place matches well enough.
pub(crate) fn locate(
    preview: &DecodedPreview,
    drawn: &Picture,
    candidates: &[Place],
) -> Option<(Place, f64)> {
    if candidates.is_empty() {
        return None;
    }
    let shrink = preview.width.div_ceil(MATCH_WIDTH);
    let own = Grey::from_pixels(
        preview.width as usize,
        preview.height as usize,
        preview.rgba.chunks_exact(4),
    )?
    .shrunk(shrink as usize)?;
    let share = (
        f64::from(shrink) / f64::from(preview.width),
        f64::from(shrink) / f64::from(preview.height),
    );
    // Blur the drawing by about the finest candidate's preview pixel, so a
    // sample of it stands for what that pixel shows.
    let finest = candidates
        .iter()
        .map(|place| place.w * share.0)
        .fold(f64::INFINITY, f64::min);
    let radius = (finest / f64::from(drawn.factor) / 2.0).round();
    let world = Grey::from_pixels(
        drawn.width as usize,
        drawn.height as usize,
        drawn.rgb.chunks_exact(3),
    )?
    .blurred(radius as usize);
    let matcher = Matcher {
        own,
        share,
        world,
        origin: (f64::from(drawn.area.x), f64::from(drawn.area.y)),
        factor: f64::from(drawn.factor),
    };
    candidates
        .iter()
        .map(|&start| matcher.refine(start))
        .filter(|&(_, score)| score >= LEAST_MATCH)
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// A picture's brightness.
struct Grey {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Grey {
    fn from_pixels<'a>(
        width: usize,
        height: usize,
        pixels: impl Iterator<Item = &'a [u8]>,
    ) -> Option<Self> {
        let values: Vec<f32> = pixels
            .map(|pixel| {
                0.299 * f32::from(pixel[0])
                    + 0.587 * f32::from(pixel[1])
                    + 0.114 * f32::from(pixel[2])
            })
            .collect();
        (width > 0 && height > 0 && values.len() == width * height).then_some(Self {
            width,
            height,
            values,
        })
    }

    /// Each `factor` x `factor` block's mean; a partial last block is dropped.
    fn shrunk(self, factor: usize) -> Option<Self> {
        if factor <= 1 {
            return Some(self);
        }
        let (width, height) = (self.width / factor, self.height / factor);
        if width == 0 || height == 0 {
            return None;
        }
        let mut values = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let mut sum = 0.0;
                for row in y * factor..(y + 1) * factor {
                    let at = row * self.width + x * factor;
                    sum += self.values[at..at + factor].iter().sum::<f32>();
                }
                values.push(sum / (factor * factor) as f32);
            }
        }
        Some(Self {
            width,
            height,
            values,
        })
    }

    /// The mean of the (2 `radius` + 1)-wide square around each pixel, of
    /// its pixels inside the picture.
    fn blurred(self, radius: usize) -> Self {
        if radius == 0 {
            return self;
        }
        let across = box_mean(&self.values, self.width, self.height, radius, 1, self.width);
        let values = box_mean(&across, self.height, self.width, radius, self.width, 1);
        Self { values, ..self }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.values[y * self.width + x]
    }
}

/// The running mean along `lines` lines of `length` values each, `step`
/// apart along a line and `stride` apart between lines.
fn box_mean(
    values: &[f32],
    length: usize,
    lines: usize,
    radius: usize,
    step: usize,
    stride: usize,
) -> Vec<f32> {
    let mut out = vec![0.0; values.len()];
    for line in 0..lines {
        let at = |index: usize| line * stride + index * step;
        let mut sum: f32 = (0..radius.min(length)).map(|index| values[at(index)]).sum();
        let mut count = radius.min(length);
        for index in 0..length {
            if index + radius < length {
                sum += values[at(index + radius)];
                count += 1;
            }
            if index > radius {
                sum -= values[at(index - radius - 1)];
                count -= 1;
            }
            out[at(index)] = sum / count as f32;
        }
    }
    out
}

/// The preview against the drawing of the world.
struct Matcher {
    own: Grey,
    /// The part of the preview's width and height each of `own`'s pixels
    /// stands for: a partial last block of the shrink was dropped.
    share: (f64, f64),
    world: Grey,
    /// The world point at the drawing's top-left.
    origin: (f64, f64),
    /// The world pixels across each pixel of the drawing.
    factor: f64,
}

impl Matcher {
    /// The correlation of the preview's brightness with the drawing's at
    /// `place`, sampled at each preview pixel's centre; -1 when less than
    /// half the preview lies on the drawing or either side is flat.
    fn score(&self, place: Place) -> f64 {
        let (own, world) = (&self.own, &self.world);
        let step = (place.w * self.share.0, place.h * self.share.1);
        let (mut count, mut sum_own, mut sum_world) = (0.0, 0.0, 0.0);
        let (mut own_squares, mut world_squares, mut products) = (0.0, 0.0, 0.0);
        for row in 0..own.height {
            let world_y =
                (place.y + (row as f64 + 0.5) * step.1 - self.origin.1) / self.factor - 0.5;
            if !(0.0..(world.height - 1) as f64).contains(&world_y) {
                continue;
            }
            let (y, below) = (world_y as usize, world_y.fract() as f32);
            for column in 0..own.width {
                let world_x =
                    (place.x + (column as f64 + 0.5) * step.0 - self.origin.0) / self.factor - 0.5;
                if !(0.0..(world.width - 1) as f64).contains(&world_x) {
                    continue;
                }
                let (x, right) = (world_x as usize, world_x.fract() as f32);
                let top = world.at(x, y) * (1.0 - right) + world.at(x + 1, y) * right;
                let bottom = world.at(x, y + 1) * (1.0 - right) + world.at(x + 1, y + 1) * right;
                let seen = f64::from(top * (1.0 - below) + bottom * below);
                let mine = f64::from(own.at(column, row));
                count += 1.0;
                sum_own += mine;
                sum_world += seen;
                own_squares += mine * mine;
                world_squares += seen * seen;
                products += mine * seen;
            }
        }
        if count * 2.0 < (own.width * own.height) as f64 {
            return -1.0;
        }
        let (mean_own, mean_world) = (sum_own / count, sum_world / count);
        let own_variance = own_squares / count - mean_own * mean_own;
        let world_variance = world_squares / count - mean_world * mean_world;
        if own_variance <= 0.0 || world_variance <= 0.0 {
            return -1.0;
        }
        (products / count - mean_own * mean_world) / (own_variance * world_variance).sqrt()
    }

    /// `start` moved and resized while that improves the match, in steps from
    /// a 32nd of its width down to a world pixel, each edge staying within
    /// [`REACH`] of where it started; the place and its score.
    fn refine(&self, start: Place) -> (Place, f64) {
        let within = |place: Place| {
            (place.x - start.x).abs() <= start.w * REACH
                && (place.y - start.y).abs() <= start.h * REACH
                && (place.right() - start.right()).abs() <= start.w * REACH
                && (place.bottom() - start.bottom()).abs() <= start.h * REACH
        };
        let (mut place, mut score) = (start, self.score(start));
        let mut step = start.w / 32.0;
        while step >= 1.0 {
            let mut moved = true;
            while moved {
                moved = false;
                let Place { x, y, w, h } = place;
                let moves = [
                    // The whole place.
                    Place {
                        x: x - step,
                        ..place
                    },
                    Place {
                        x: x + step,
                        ..place
                    },
                    Place {
                        y: y - step,
                        ..place
                    },
                    Place {
                        y: y + step,
                        ..place
                    },
                    // Its left and top edges.
                    Place {
                        x: x - step,
                        w: w + step,
                        ..place
                    },
                    Place {
                        x: x + step,
                        w: w - step,
                        ..place
                    },
                    Place {
                        y: y - step,
                        h: h + step,
                        ..place
                    },
                    Place {
                        y: y + step,
                        h: h - step,
                        ..place
                    },
                    // Its right and bottom edges.
                    Place {
                        w: w - step,
                        ..place
                    },
                    Place {
                        w: w + step,
                        ..place
                    },
                    Place {
                        h: h - step,
                        ..place
                    },
                    Place {
                        h: h + step,
                        ..place
                    },
                ];
                for candidate in moves {
                    if !within(candidate) {
                        continue;
                    }
                    let candidate_score = self.score(candidate);
                    if candidate_score > score + 1e-6 {
                        (place, score) = (candidate, candidate_score);
                        moved = true;
                    }
                }
            }
            step /= 2.0;
        }
        (place, score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(size: (u32, u32), local: [u32; 4]) -> MapHeader {
        MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: size.0,
            height: size.1,
            local_left: local[0],
            local_top: local[1],
            local_width: local[2],
            local_height: local[3],
        }
    }

    fn view(x: i32, y: i32, w: u32, h: u32) -> WorldRect {
        WorldRect { x, y, w, h }
    }

    #[test]
    fn candidates_hold_the_places_measured_on_retail_and_library_maps() {
        // Normandy (1-4 vs 5-8) v1.4.4: a 400 x 200 render matched world
        // (-2880, 1575) 5760 x 2880, LocalSize's columns from two rows up.
        let normandy = candidates(
            &header((100, 100), [2, 4, 96, 94]),
            view(-2880, 1560, 5760, 2835),
            (400, 200),
        );
        assert_eq!(
            normandy[0],
            Place {
                x: -2880.0,
                y: 1575.0,
                w: 5760.0,
                h: 2880.0
            }
        );
        // Too large for a minimap of either rectangle.
        assert_eq!(normandy.len(), 3);
        // Arena: a 158 x 79 minimap of the whole 80 x 80 map, 30 world
        // pixels a pixel from its top-left (-2400, 1215).
        let arena = candidates(
            &header((80, 80), [5, 5, 70, 69]),
            view(-2100, 1290, 4200, 2205),
            (158, 79),
        );
        assert!(arena.contains(&Place {
            x: -2400.0,
            y: 1215.0,
            w: 4740.0,
            h: 2370.0
        }));
    }

    /// Smooth brightness without repeats: value noise at two scales.
    fn ground(x: f64, y: f64) -> u8 {
        fn noise(x: f64, y: f64, cell: f64) -> f64 {
            let hash = |ix: i64, iy: i64| {
                let mut h = (ix.wrapping_mul(374_761_393) ^ iy.wrapping_mul(668_265_263)) as u64;
                h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
                ((h ^ (h >> 16)) & 0xFFFF) as f64 / 65_535.0
            };
            let (gx, gy) = (x / cell, y / cell);
            let (ix, iy) = (gx.floor() as i64, gy.floor() as i64);
            let (tx, ty) = (gx - gx.floor(), gy - gy.floor());
            let top = hash(ix, iy) * (1.0 - tx) + hash(ix + 1, iy) * tx;
            let bottom = hash(ix, iy + 1) * (1.0 - tx) + hash(ix + 1, iy + 1) * tx;
            top * (1.0 - ty) + bottom * ty
        }
        (255.0 * (0.6 * noise(x, y, 90.0) + 0.4 * noise(x, y, 30.0))) as u8
    }

    fn drawing(area: WorldRect, factor: u32) -> Picture {
        let (width, height) = (area.w / factor, area.h / factor);
        let mut rgb = Vec::new();
        for row in 0..height {
            for column in 0..width {
                let x = f64::from(area.x) + (f64::from(column) + 0.5) * f64::from(factor);
                let y = f64::from(area.y) + (f64::from(row) + 0.5) * f64::from(factor);
                rgb.extend([ground(x, y); 3]);
            }
        }
        Picture {
            area,
            factor,
            width,
            height,
            rgb,
        }
    }

    fn preview_of(
        place: Place,
        size: (u32, u32),
        shade: impl Fn(f64, f64) -> u8,
    ) -> DecodedPreview {
        let mut rgba = Vec::new();
        for row in 0..size.1 {
            for column in 0..size.0 {
                let x = place.x + (f64::from(column) + 0.5) * place.w / f64::from(size.0);
                let y = place.y + (f64::from(row) + 0.5) * place.h / f64::from(size.1);
                let value = shade(x, y);
                rgba.extend([value, value, value, 255]);
            }
        }
        DecodedPreview {
            width: size.0,
            height: size.1,
            rgba,
        }
    }

    #[test]
    fn locate_finds_a_stretched_preview_from_a_nearby_start() {
        let drawn = drawing(view(-1000, -500, 2000, 1000), 4);
        let truth = Place {
            x: -700.0,
            y: -300.0,
            w: 1200.0,
            h: 640.0,
        };
        // Stretched taller, and shrunk by 3 to match, which drops its last
        // two rows: 13 world pixels at the bottom.
        let preview = preview_of(truth, (540, 98), ground);
        let start = Place {
            x: -660.0,
            y: -330.0,
            w: 1150.0,
            h: 620.0,
        };
        let (found, score) = locate(&preview, &drawn, &[start]).expect("a match");
        assert!(score > 0.9, "{score}");
        for (edge, expected) in [
            (found.x, truth.x),
            (found.y, truth.y),
            (found.right(), truth.right()),
            (found.bottom(), truth.bottom()),
        ] {
            assert!((edge - expected).abs() <= 8.0, "{found:?}");
        }
    }

    #[test]
    fn locate_keeps_no_place_for_a_preview_unlike_the_map() {
        let drawn = drawing(view(-1000, -500, 2000, 1000), 4);
        let place = Place {
            x: -700.0,
            y: -300.0,
            w: 1200.0,
            h: 640.0,
        };
        // Artwork: the same kind of ground, but somewhere else entirely.
        let artwork = preview_of(place, (150, 100), |x, y| ground(x + 50_000.0, y - 70_000.0));
        assert_eq!(locate(&artwork, &drawn, &[place]), None);
    }
}
