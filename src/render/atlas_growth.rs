//! Shelf allocation and sub-rectangle uploads for atlas pages that grow.
//!
//! The entity and map atlases pack everything the map starts with once, at
//! load, through [`plan_initial_shelves`]. Sprites that appear later — the
//! first building or vehicle of a new type mid-match, a slope-transition
//! frame — go into free shelf space on a fixed-size page and upload only the
//! rows they occupy, so adding sprites never repacks or re-uploads the sprites
//! already resident. Both paths place through [`ShelfCursor`].

/// Gap between neighbouring sprites so sampling never bleeds across them.
pub(crate) const SPRITE_PADDING: u32 = 1;

/// Next free position on a page that is filled shelf by shelf, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShelfCursor {
    width: u32,
    height: u32,
    padding: u32,
    x: u32,
    y: u32,
    shelf_height: u32,
}

impl ShelfCursor {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self::with_padding(width, height, SPRITE_PADDING)
    }

    /// A page whose sprites and shelves sit `padding` pixels apart.
    pub(crate) fn with_padding(width: u32, height: u32, padding: u32) -> Self {
        Self {
            width,
            height,
            padding,
            x: 0,
            y: 0,
            shelf_height: 0,
        }
    }

    /// Close the current shelf, so the next placement starts on rows no
    /// earlier sprite occupies.
    pub(crate) fn start_new_shelf(&mut self) {
        if self.x > 0 {
            self.y = self.y.saturating_add(self.shelf_height + self.padding);
            self.x = 0;
            self.shelf_height = 0;
        }
    }

    /// Reserve a `width` x `height` rectangle and return its top-left corner.
    /// A full page returns `None` and keeps its cursor, so a smaller sprite
    /// can still use the current shelf.
    pub(crate) fn place(&mut self, width: u32, height: u32) -> Option<[u32; 2]> {
        if width > self.width || height > self.height {
            return None;
        }
        let (mut x, mut y, mut shelf_height) = (self.x, self.y, self.shelf_height);
        if x.saturating_add(width) > self.width {
            y = y.saturating_add(shelf_height + self.padding);
            x = 0;
            shelf_height = 0;
        }
        if y.saturating_add(height) > self.height {
            return None;
        }
        self.x = x + width + self.padding;
        self.y = y;
        self.shelf_height = shelf_height.max(height);
        Some([x, y])
    }

    /// Rows down to the bottom of the open shelf.
    pub(crate) fn used_height(&self) -> u32 {
        self.y.saturating_add(self.shelf_height)
    }
}

/// How an atlas packs the sprites it starts with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InitialShelfPolicy {
    /// Gap between neighbouring sprites and shelves.
    pub(crate) padding: u32,
    /// GPU texture limit. Pages never grow wider than this.
    pub(crate) max_dim: u32,
    /// Open another page when the shelves outgrow `max_dim` rows. Without it
    /// the single page reports its full height and the caller clamps or
    /// refuses it.
    pub(crate) split_pages: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShelfPlacement {
    /// Position of the sprite in the planned `sizes`.
    pub(crate) index: usize,
    pub(crate) page: usize,
    pub(crate) origin: [u32; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InitialShelfPlan {
    /// Width of every page.
    pub(crate) width: u32,
    /// Each page's used rows: the bottom edge of its lowest sprite.
    pub(crate) page_heights: Vec<u32>,
    /// Tallest first, so each page's shelves come top to bottom.
    pub(crate) placements: Vec<ShelfPlacement>,
}

impl InitialShelfPlan {
    /// Height of a single-page plan; 1 when nothing was placed.
    pub(crate) fn single_page_height(&self) -> u32 {
        self.page_heights.first().copied().unwrap_or(1)
    }
}

const MIN_INITIAL_PAGE_WIDTH: u32 = 64;

/// Shelf-pack `sizes` (`[width, height]`) for an atlas's initial load.
///
/// Sprites go tallest first; equal heights keep their order in `sizes`, so a
/// caller sets its tie order by ordering its input. The page width starts at
/// the square root of the padded area, at least 64 px and never narrower than
/// the widest sprite, and doubles until one page's shelves fit `max_dim` rows
/// or the width reaches `max_dim`. A sprite wider than `max_dim` (or, with
/// `split_pages`, taller) is left out; callers that must not drop sprites
/// reject them first.
pub(crate) fn plan_initial_shelves(
    sizes: &[[u32; 2]],
    policy: InitialShelfPolicy,
) -> InitialShelfPlan {
    let InitialShelfPolicy {
        padding,
        max_dim,
        split_pages,
    } = policy;
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|&a, &b| sizes[b][1].cmp(&sizes[a][1]));

    let padded_area: u64 = sizes
        .iter()
        .map(|&[width, height]| {
            (u64::from(width) + u64::from(padding)) * (u64::from(height) + u64::from(padding))
        })
        .sum();
    let widest = sizes.iter().map(|&[width, _]| width).max().unwrap_or(0);
    // A page narrower than its widest sprite would place it partly outside.
    let mut width = ((padded_area as f64).sqrt().ceil() as u32)
        .max(MIN_INITIAL_PAGE_WIDTH.min(max_dim))
        .max(widest)
        .min(max_dim);
    let one_page_height = |width: u32| {
        let mut shelf = ShelfCursor::with_padding(width, u32::MAX, padding);
        for &index in &order {
            let [sprite_width, sprite_height] = sizes[index];
            shelf.place(sprite_width, sprite_height);
        }
        shelf.used_height()
    };
    while one_page_height(width) > max_dim && width < max_dim {
        width = width.saturating_mul(2).min(max_dim);
    }

    let page_rows = if split_pages { max_dim } else { u32::MAX };
    let mut shelf: Option<ShelfCursor> = None;
    let mut page_heights: Vec<u32> = Vec::new();
    let mut placements = Vec::with_capacity(sizes.len());
    for &index in &order {
        let [sprite_width, sprite_height] = sizes[index];
        if sprite_width > width || sprite_height > page_rows {
            continue;
        }
        let origin = match shelf
            .as_mut()
            .and_then(|shelf| shelf.place(sprite_width, sprite_height))
        {
            Some(origin) => origin,
            None => {
                let mut page = ShelfCursor::with_padding(width, page_rows, padding);
                let origin = page
                    .place(sprite_width, sprite_height)
                    .expect("a fresh page holds a sprite within its limits");
                shelf = Some(page);
                page_heights.push(0);
                origin
            }
        };
        let page = page_heights.len() - 1;
        page_heights[page] = page_heights[page].max(origin[1] + sprite_height);
        placements.push(ShelfPlacement {
            index,
            page,
            origin,
        });
    }
    if placements.len() < sizes.len() {
        log::warn!(
            "{} sprites exceed the {max_dim} px texture limit and are left out of the atlas",
            sizes.len() - placements.len()
        );
    }
    InitialShelfPlan {
        width,
        page_heights,
        placements,
    }
}

/// The page an atlas appends to, and its shelf.
pub(crate) struct GrowthShelf {
    pub(crate) page: usize,
    pub(crate) shelf: ShelfCursor,
}

/// One refresh's sprites on one page. Their rows hold no earlier sprite, so
/// each texture of the page takes them in a single [`write_band`].
pub(crate) struct PagePlacements<T> {
    pub(crate) page: usize,
    pub(crate) sprites: Vec<([u32; 2], T)>,
}

/// Place one refresh's sprites, tallest first, on the current growth page,
/// opening a page with `open_page` whenever it is full. `open_page` gets the
/// size the page must at least have and returns the new page's index and
/// size. The refresh starts on a fresh shelf, so the rows it fills hold no
/// resident sprite. Sprites wider or taller than `max_dim` are returned
/// unplaced.
pub(crate) fn place_on_growth_pages<T>(
    growth: &mut Option<GrowthShelf>,
    mut sprites: Vec<T>,
    size: impl Fn(&T) -> [u32; 2],
    max_dim: u32,
    mut open_page: impl FnMut([u32; 2]) -> (usize, [u32; 2]),
) -> (Vec<PagePlacements<T>>, Vec<T>) {
    let mut placed: Vec<PagePlacements<T>> = Vec::new();
    let mut unplaced = Vec::new();
    if sprites.is_empty() {
        return (placed, unplaced);
    }
    sprites.sort_by_key(|sprite| std::cmp::Reverse(size(sprite)[1]));
    if let Some(growth) = growth.as_mut() {
        growth.shelf.start_new_shelf();
    }
    for sprite in sprites {
        let [width, height] = size(&sprite);
        if width > max_dim || height > max_dim {
            unplaced.push(sprite);
            continue;
        }
        let spot = growth
            .as_mut()
            .and_then(|growth| Some((growth.page, growth.shelf.place(width, height)?)));
        let (page, origin) = match spot {
            Some(spot) => spot,
            None => {
                let (page, [page_width, page_height]) = open_page([width, height]);
                let mut shelf = ShelfCursor::new(page_width, page_height);
                let origin = shelf.place(width, height);
                *growth = Some(GrowthShelf { page, shelf });
                match origin {
                    Some(origin) => (page, origin),
                    None => {
                        unplaced.push(sprite);
                        continue;
                    }
                }
            }
        };
        match placed.last_mut() {
            Some(group) if group.page == page => group.sprites.push((origin, sprite)),
            _ => placed.push(PagePlacements {
                page,
                sprites: vec![(origin, sprite)],
            }),
        }
    }
    (placed, unplaced)
}

/// Rectangles `(origin, size)` gathered into the band of rows they span, from
/// column 0 to the rightmost texel: returns the band's origin, size and
/// tightly packed texels. `texels(i)` gives rectangle `i`'s tightly packed
/// texels as it is copied in, so a caller can produce them one at a time.
/// Texels no rectangle covers are zero.
pub(crate) fn gather_band<T: AsRef<[u8]>>(
    rects: &[([u32; 2], [u32; 2])],
    bytes_per_texel: u32,
    mut texels: impl FnMut(usize) -> T,
) -> Option<([u32; 2], [u32; 2], Vec<u8>)> {
    let top = rects.iter().map(|(origin, _)| origin[1]).min()?;
    let bottom = rects
        .iter()
        .map(|(origin, size)| origin[1] + size[1])
        .max()?;
    let right = rects
        .iter()
        .map(|(origin, size)| origin[0] + size[0])
        .max()?;
    let texel = bytes_per_texel as usize;
    let stride = right as usize * texel;
    let mut band = vec![0u8; stride * (bottom - top) as usize];
    for (i, (origin, size)) in rects.iter().enumerate() {
        let texels = texels(i);
        let texels = texels.as_ref();
        let row = size[0] as usize * texel;
        for y in 0..size[1] as usize {
            let start =
                (origin[1] - top) as usize * stride + y * stride + origin[0] as usize * texel;
            band[start..start + row].copy_from_slice(&texels[y * row..(y + 1) * row]);
        }
    }
    Some(([0, top], [right, bottom - top], band))
}

/// Upload rectangles placed on rows no resident sprite occupies (a fresh
/// shelf, see [`ShelfCursor::start_new_shelf`]) with one write, however many
/// there are; per-rectangle writes cost a staging allocation each.
pub(crate) fn write_band<T: AsRef<[u8]>>(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    bytes_per_texel: u32,
    rects: &[([u32; 2], [u32; 2])],
    texels: impl FnMut(usize) -> T,
) {
    if let Some((origin, size, band)) = gather_band(rects, bytes_per_texel, texels) {
        write_texels(queue, texture, origin, size, bytes_per_texel, &band);
    }
}

/// Upload one tightly packed rectangle of texels into `texture` at `origin`.
pub(crate) fn write_texels(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    origin: [u32; 2],
    size: [u32; 2],
    bytes_per_texel: u32,
    texels: &[u8],
) {
    let [width, height] = size;
    if width == 0 || height == 0 {
        return;
    }
    debug_assert_eq!(
        texels.len(),
        (width * height * bytes_per_texel) as usize,
        "texel rectangle must be tightly packed"
    );
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_texel),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_fill_left_to_right_then_wrap_below_the_tallest_sprite() {
        let mut shelf = ShelfCursor::new(10, 10);
        assert_eq!(shelf.place(4, 3), Some([0, 0]));
        assert_eq!(shelf.place(4, 2), Some([5, 0]));
        // 10 + 4 > 10: wraps below the first shelf's tallest sprite plus padding.
        assert_eq!(shelf.place(4, 2), Some([0, 4]));
    }

    #[test]
    fn a_full_page_refuses_without_losing_its_current_shelf() {
        let mut shelf = ShelfCursor::new(10, 6);
        assert_eq!(shelf.place(4, 4), Some([0, 0]));
        // A new shelf would start at y = 5 and cannot hold 4 rows.
        assert_eq!(shelf.place(8, 4), None);
        // The refusal did not wrap the cursor: the first shelf still has room.
        assert_eq!(shelf.place(4, 4), Some([5, 0]));
        assert_eq!(shelf.place(11, 1), None, "wider than the page");
    }

    #[test]
    fn a_new_shelf_starts_below_every_placed_sprite() {
        let mut shelf = ShelfCursor::new(10, 10);
        assert_eq!(shelf.place(2, 3), Some([0, 0]));
        shelf.start_new_shelf();
        assert_eq!(shelf.place(2, 1), Some([0, 4]));
        shelf.start_new_shelf();
        shelf.start_new_shelf();
        assert_eq!(
            shelf.place(2, 1),
            Some([0, 6]),
            "an empty shelf is not skipped"
        );
    }

    #[test]
    fn a_band_gathers_rectangles_into_the_rows_they_span() {
        let tall = [1u8, 2, 3, 4, 5, 6];
        let wide = [7u8, 8, 9];
        let texels = [&tall[..], &wide[..]];
        let (origin, size, band) =
            gather_band(&[([0, 4], [2, 3]), ([3, 4], [3, 1])], 1, |i| texels[i])
                .expect("two rectangles");
        assert_eq!((origin, size), ([0, 4], [6, 3]));
        #[rustfmt::skip]
        assert_eq!(band, vec![
            1, 2, 0, 7, 8, 9,
            3, 4, 0, 0, 0, 0,
            5, 6, 0, 0, 0, 0,
        ]);
        assert!(gather_band(&[], 4, |_| [0u8; 0]).is_none());
    }

    fn bottom(placed: &[PagePlacements<[u32; 2]>]) -> u32 {
        placed
            .iter()
            .flat_map(|group| &group.sprites)
            .map(|(origin, size)| origin[1] + size[1])
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn a_refresh_band_never_reaches_rows_an_earlier_refresh_filled() {
        let mut growth = None;
        let mut opened = Vec::new();
        let mut open_page = |needed: [u32; 2]| {
            opened.push(needed);
            (opened.len() + 1, [16, 16])
        };
        let (first, unplaced) = place_on_growth_pages(
            &mut growth,
            vec![[5, 2], [3, 4]],
            |s| *s,
            64,
            &mut open_page,
        );
        assert!(unplaced.is_empty());
        assert_eq!(first.len(), 1, "one page");
        // The earlier shelf still has room for a 3x1 sprite at x = 10; a
        // refresh must not use it, since its band would zero those rows.
        let (second, _) =
            place_on_growth_pages(&mut growth, vec![[3, 1]], |s| *s, 64, &mut open_page);
        let top = second[0]
            .sprites
            .iter()
            .map(|(origin, _)| origin[1])
            .min()
            .unwrap();
        assert!(
            top >= bottom(&first),
            "band top {top} below {}",
            bottom(&first)
        );
        assert_eq!(second[0].page, first[0].page);
        assert_eq!(
            opened,
            vec![[3, 4]],
            "the first refresh opened the only page"
        );
    }

    #[test]
    fn a_full_page_opens_the_next_and_oversized_sprites_stay_unplaced() {
        let mut growth = None;
        let mut pages = 0;
        let (placed, unplaced) = place_on_growth_pages(
            &mut growth,
            vec![[8, 8], [8, 8], [20, 1]],
            |s| *s,
            16,
            |_| {
                pages += 1;
                (pages, [8, 8])
            },
        );
        assert_eq!(unplaced, vec![[20, 1]]);
        let pages_used: Vec<usize> = placed.iter().map(|group| group.page).collect();
        assert_eq!(pages_used, vec![1, 2], "the second 8x8 needs its own page");
        let (none, unplaced) = place_on_growth_pages(
            &mut growth,
            Vec::<[u32; 2]>::new(),
            |s| *s,
            16,
            |_| unreachable!(),
        );
        assert!(none.is_empty() && unplaced.is_empty());
    }

    fn placed(plan: &InitialShelfPlan) -> Vec<(usize, usize, [u32; 2])> {
        plan.placements
            .iter()
            .map(|placed| (placed.index, placed.page, placed.origin))
            .collect()
    }

    /// Expected plans come from running the former per-atlas loops on these
    /// inputs: `unit_atlas::plan_sprite_pages` and `sprite_atlas::
    /// plan_shp_pages` (identical here) for split pages, and
    /// `sidebar_cameo_atlas::pack_cameos` for one page with 2 px gaps. Both
    /// widen the page once; equal heights keep their input order.
    #[test]
    fn initial_shelves_keep_the_former_loops_rectangles() {
        let sprites = [
            [35, 50],
            [20, 12],
            [35, 50],
            [35, 50],
            [10, 30],
            [35, 50],
            [64, 5],
            [12, 12],
        ];
        let paged = plan_initial_shelves(
            &sprites,
            InitialShelfPolicy {
                padding: 1,
                max_dim: 100,
                split_pages: true,
            },
        );
        assert_eq!((paged.width, &paged.page_heights[..]), (100, &[50, 63][..]));
        assert_eq!(
            placed(&paged),
            [
                (0, 0, [0, 0]),
                (2, 0, [36, 0]),
                (3, 1, [0, 0]),
                (5, 1, [36, 0]),
                (4, 1, [72, 0]),
                (1, 1, [0, 51]),
                (7, 1, [21, 51]),
                (6, 1, [34, 51]),
            ]
        );

        let cameos = [
            [60, 48],
            [40, 20],
            [60, 48],
            [25, 48],
            [90, 10],
            [40, 20],
            [60, 48],
            [8, 8],
        ];
        let single = plan_initial_shelves(
            &cameos,
            InitialShelfPolicy {
                padding: 2,
                max_dim: 160,
                split_pages: false,
            },
        );
        assert_eq!((single.width, single.single_page_height()), (160, 110));
        assert_eq!(
            placed(&single),
            [
                (0, 0, [0, 0]),
                (2, 0, [62, 0]),
                (3, 0, [124, 0]),
                (6, 0, [0, 50]),
                (1, 0, [62, 50]),
                (5, 0, [104, 50]),
                (4, 0, [0, 100]),
                (7, 0, [92, 100]),
            ]
        );
    }

    #[test]
    fn a_sprite_wider_than_the_estimated_page_widens_it() {
        // The padded area alone suggests a 175 px page; the former loops
        // placed this sprite at (0, 1), running 125 px past the page edge.
        let plan = plan_initial_shelves(
            &[[300, 100]],
            InitialShelfPolicy {
                padding: 1,
                max_dim: 4096,
                split_pages: false,
            },
        );
        assert_eq!(plan.width, 300);
        assert_eq!(placed(&plan), [(0, 0, [0, 0])]);
    }
}
