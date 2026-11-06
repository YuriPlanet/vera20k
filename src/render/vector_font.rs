//! Optional modern UI font behind `[VERA] UiFont=`: TrueType/OpenType
//! rasterisation with an installed-family fallback chain.
//!
//! VERA extension, presentation only. `GAME.FNT` stays the default and the
//! only font when the key is absent, so the retail look is unchanged. Each
//! entry is an installed family name or a font file: an absolute path loads
//! from disk, and a relative path resolves against the retail game folder, so
//! a font can ship beside the game. Each character is resolved through the
//! configured entries in order and then through [`FALLBACK_FAMILIES`], which
//! is what gives Latin, Cyrillic, Greek and CJK text a covering face even when
//! a single font does not carry it.
//! There is no complex-script shaping and no bidirectional reordering: Arabic
//! and indic text draws with isolated, unjoined glyphs.
//!
//! Glyphs are rasterised lazily at [`PX_SIZE`] and parked in one
//! [`ATLAS_SIZE`] square RGBA atlas. The atlas texture is created empty and
//! written piecewise with `Queue::write_texture`, so nothing has to hold a
//! zeroed 16 MiB image in memory; [`VectorFont::flush`] uploads the glyphs
//! rasterised since the previous frame, before the frame's command buffer is
//! submitted.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use swash::scale::image::Image;
use swash::scale::{Render, ScaleContext, Source};
use swash::{FontRef, GlyphId};

use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::bit_font::{CELL_HEIGHT, LineSpan, WrapLayout};
use crate::render::shell_text_reveal::{PathAReveal, encoded_unit_rgb};

/// Rasterised em size. `GAME.FNT` draws 16 rows, so a 16 px em keeps existing
/// layouts and baselines usable.
const PX_SIZE: f32 = 16.0;
/// Baseline distance from the top of the cell. `GAME.FNT`'s glyph cell is 16
/// rows plus a one-pixel gap; 15 px puts a 16 px CJK glyph's top on the cell's
/// first row and still leaves the usual descender room.
const BASELINE: f32 = 15.0;
/// Atlas edge. RGBA8 at 16 px holds about 12k CJK-sized glyphs, enough for a
/// game's UI strings; when it fills, later characters fall back to `?`.
const ATLAS_SIZE: u32 = 2048;
/// Transparent gutter on each side of a glyph, so linear sampling never
/// bleeds a neighbour into an edge pixel.
const GLYPH_PADDING: u32 = 1;
/// Tab stops are layout, not glyphs; reuse the FNT constants so tabbed text
/// lands in the same columns.
const TAB_WIDTH: u32 = crate::render::bit_font::TAB_WIDTH;
const TAB_ORIGIN: u32 = crate::render::bit_font::TAB_ORIGIN;

/// Families tried after the configured ones. Every entry is looked up through
/// the system font database and silently skipped when it is not installed, so
/// this list only has to name the usual pan-script faces of Windows, macOS and
/// Linux.
const FALLBACK_FAMILIES: &[&str] = &[
    // Latin, Greek, Cyrillic and the common symbols.
    "Segoe UI",
    "Noto Sans",
    "DejaVu Sans",
    "Arial",
    // CJK: Windows, macOS, then the common Linux desktop fonts.
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "SimSun",
    "Malgun Gothic",
    "Yu Gothic",
    "MS Gothic",
    "PingFang SC",
    "Hiragino Sans GB",
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "WenQuanYi Micro Hei",
    // Symbols that Latin text commonly reaches.
    "Segoe UI Symbol",
    "Segoe UI Historic",
    // South Asian and Thai.
    "Nirmala UI",
    "Noto Sans Devanagari",
    "Noto Sans Bengali",
    "Noto Sans Tamil",
    "Leelawadee UI",
    "Noto Sans Thai",
    // Arabic and Hebrew.
    "Noto Sans Arabic",
    "Noto Sans Hebrew",
    "Geeza Pro",
    "Noto Sans Symbols 2",
];

/// One resolved face: which font database entry and which face inside it.
struct FaceRef {
    id: fontdb::ID,
    index: u32,
    family: String,
}

/// One configured `[VERA] UiFont=` entry: an installed family or a font file.
enum FontEntry<'a> {
    File(PathBuf),
    Family(&'a str),
}

/// A character's placement data, resolved once per character.
#[derive(Clone, Copy)]
struct CharMetrics {
    face: usize,
    glyph: GlyphId,
    advance: f32,
}

/// One rasterised glyph inside the atlas.
#[derive(Clone, Copy)]
struct RasterGlyph {
    /// `[x, y, width, height]` in atlas pixels; width or height is zero for a
    /// blank glyph such as the space.
    rect: [u32; 4],
    /// Top-left corner relative to the pen position on the baseline.
    bearing: [f32; 2],
}

/// A glyph rectangle waiting for the next [`VectorFont::flush`].
struct PendingUpload {
    origin: [u32; 2],
    size: [u32; 2],
    rgba: Vec<u8>,
}

/// Shelf packer for the lazily filled atlas.
struct Shelf {
    next_x: u32,
    next_y: u32,
    row_height: u32,
    full: bool,
}

impl Shelf {
    fn new() -> Self {
        Self {
            next_x: 0,
            next_y: 0,
            row_height: 0,
            full: false,
        }
    }

    /// Reserve a `width`x`height` rectangle and answer its top-left corner.
    fn place(&mut self, width: u32, height: u32) -> Option<[u32; 2]> {
        if self.full {
            return None;
        }
        let cell_w = width + GLYPH_PADDING * 2;
        let cell_h = height + GLYPH_PADDING * 2;
        if self.next_x + cell_w > ATLAS_SIZE {
            self.next_x = 0;
            self.next_y += self.row_height;
            self.row_height = 0;
        }
        if self.next_y + cell_h > ATLAS_SIZE {
            self.full = true;
            log::warn!(
                "[VERA] UiFont= atlas {ATLAS_SIZE}x{ATLAS_SIZE} is full; later glyphs draw as '?'"
            );
            return None;
        }
        let origin = [self.next_x + GLYPH_PADDING, self.next_y + GLYPH_PADDING];
        self.next_x += cell_w;
        self.row_height = self.row_height.max(cell_h);
        Some(origin)
    }
}

struct Atlas {
    shelf: Shelf,
    pending: Vec<PendingUpload>,
}

/// Rasterised multi-family UI font. Owned by [`crate::render::bit_font::BitFont`]
/// as an optional replacement for its bitmap glyph table.
pub struct VectorFont {
    db: fontdb::Database,
    faces: Vec<FaceRef>,
    /// Face binaries, loaded on first use: a Latin-only UI never pays for the
    /// CJK faces in the chain.
    data: RefCell<HashMap<usize, Rc<Vec<u8>>>>,
    /// Per-character resolution, including `None` for a character no face
    /// covers (so the scan runs once).
    metrics: RefCell<HashMap<char, Option<CharMetrics>>>,
    /// Per-glyph rasterisation; `None` when swash cannot draw it.
    glyphs: RefCell<HashMap<(usize, GlyphId), Option<RasterGlyph>>>,
    context: RefCell<ScaleContext>,
    atlas: RefCell<Atlas>,
    texture: wgpu::Texture,
    batch_texture: BatchTexture,
    ascent: f32,
    cell_height: u32,
}

impl VectorFont {
    /// Resolve `[VERA] UiFont=` plus [`FALLBACK_FAMILIES`] and build the empty
    /// atlas. Each configured entry is an installed family name or a font file
    /// (`.ttf`, `.otf`, `.ttc`, `.otc`); a relative file path is resolved
    /// against `font_root`, the retail game folder. `None` when nothing
    /// matched, in which case the caller keeps `GAME.FNT`.
    pub fn new(
        device: &wgpu::Device,
        batch: &BatchRenderer,
        font_root: Option<&Path>,
        families: &[String],
    ) -> Option<Self> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut faces = Vec::new();
        let mut seen = HashSet::new();
        let mut loaded_files = HashSet::new();
        for entry in families {
            match classify_entry(entry, font_root) {
                None => {}
                Some(FontEntry::Family(name)) => {
                    push_family(&db, &mut faces, &mut seen, name);
                }
                Some(FontEntry::File(path)) => {
                    push_font_file(&mut db, &mut faces, &mut seen, &mut loaded_files, &path);
                }
            }
        }
        for family in FALLBACK_FAMILIES {
            push_family(&db, &mut faces, &mut seen, family);
        }
        if faces.is_empty() {
            log::warn!(
                "[VERA] UiFont= matched no installed family or font file; the retail GAME.FNT stays in use"
            );
            return None;
        }
        let (texture, batch_texture) = batch.create_blank_updatable_texture(
            device,
            ATLAS_SIZE,
            ATLAS_SIZE,
            wgpu::FilterMode::Linear,
        );
        log::info!(
            "[VERA] UiFont= vector font chain: {:?}",
            faces
                .iter()
                .map(|face| face.family.as_str())
                .collect::<Vec<_>>()
        );
        Some(Self {
            db,
            faces,
            data: RefCell::new(HashMap::new()),
            metrics: RefCell::new(HashMap::new()),
            glyphs: RefCell::new(HashMap::new()),
            context: RefCell::new(ScaleContext::new()),
            atlas: RefCell::new(Atlas {
                shelf: Shelf::new(),
                pending: Vec::new(),
            }),
            texture,
            batch_texture,
            ascent: BASELINE,
            cell_height: CELL_HEIGHT,
        })
    }

    /// The atlas bind group every text instance produced here draws from.
    pub fn texture(&self) -> &BatchTexture {
        &self.batch_texture
    }

    /// Upload every glyph rasterised since the previous call. Runs before the
    /// frame's command buffer is submitted, so the draws that reference those
    /// UVs see the texels.
    pub fn flush(&self, queue: &wgpu::Queue) {
        let pending = {
            let mut atlas = self.atlas.borrow_mut();
            std::mem::take(&mut atlas.pending)
        };
        for upload in pending {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.origin[0],
                        y: upload.origin[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload.size[0] * 4),
                    rows_per_image: Some(upload.size[1]),
                },
                wgpu::Extent3d {
                    width: upload.size[0],
                    height: upload.size[1],
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn face_data(&self, face: usize) -> Option<Rc<Vec<u8>>> {
        if let Some(data) = self.data.borrow().get(&face) {
            return Some(data.clone());
        }
        let source = self.faces.get(face)?;
        let bytes = self
            .db
            .with_face_data(source.id, |bytes, _index| bytes.to_vec())?;
        let bytes = Rc::new(bytes);
        self.data.borrow_mut().insert(face, bytes.clone());
        Some(bytes)
    }

    /// First face whose character map carries `ch`.
    fn lookup_char(&self, ch: char) -> Option<CharMetrics> {
        for (index, face) in self.faces.iter().enumerate() {
            let Some(data) = self.face_data(index) else {
                continue;
            };
            let Some(font) = FontRef::from_index(data.as_slice(), face.index as usize) else {
                continue;
            };
            let glyph = font.charmap().map(ch);
            if glyph == 0 {
                continue;
            }
            let advance = font.glyph_metrics(&[]).scale(PX_SIZE).advance_width(glyph);
            return Some(CharMetrics {
                face: index,
                glyph,
                advance,
            });
        }
        None
    }

    fn metrics_for(&self, ch: char) -> Option<CharMetrics> {
        if let Some(hit) = self.metrics.borrow().get(&ch) {
            return *hit;
        }
        let resolved = self.lookup_char(ch);
        self.metrics.borrow_mut().insert(ch, resolved);
        resolved
    }

    /// A character's metrics, using `?` from the same chain when no face
    /// covers it; the flag says the stand-in was used.
    fn metrics_or_missing(&self, ch: char) -> Option<(CharMetrics, bool)> {
        if let Some(metrics) = self.metrics_for(ch) {
            return Some((metrics, false));
        }
        if ch != '?' {
            if let Some(metrics) = self.metrics_for('?') {
                return Some((metrics, true));
            }
        }
        None
    }

    fn rasterize(&self, face: usize, glyph: GlyphId) -> Option<RasterGlyph> {
        let key = (face, glyph);
        if let Some(hit) = self.glyphs.borrow().get(&key) {
            return *hit;
        }
        let rendered = self.rasterize_uncached(face, glyph);
        self.glyphs.borrow_mut().insert(key, rendered);
        rendered
    }

    fn rasterize_uncached(&self, face: usize, glyph: GlyphId) -> Option<RasterGlyph> {
        let data = self.face_data(face)?;
        let source = self.faces.get(face)?;
        let font = FontRef::from_index(data.as_slice(), source.index as usize)?;
        let mut context = self.context.borrow_mut();
        let mut scaler = context.builder(font).size(PX_SIZE).hint(true).build();
        let mut image = Image::new();
        let sources = [Source::Outline];
        let render = Render::new(&sources);
        if !render.render_into(&mut scaler, glyph, &mut image) {
            return None;
        }
        let placement = image.placement;
        let (width, height) = (placement.width, placement.height);
        // zeno's bottom-left origin puts `top` above the baseline.
        let bearing = [placement.left as f32, -(placement.top as f32)];
        if width == 0 || height == 0 {
            return Some(RasterGlyph {
                rect: [0, 0, 0, 0],
                bearing,
            });
        }
        let expected = (width * height) as usize;
        if image.data.len() < expected {
            return None;
        }
        let origin = self.atlas.borrow_mut().shelf.place(width, height)?;
        let mut rgba = vec![0u8; expected * 4];
        for (pixel, alpha) in image.data[..expected].iter().enumerate() {
            let dst = pixel * 4;
            rgba[dst..dst + 4].copy_from_slice(&[255, 255, 255, *alpha]);
        }
        self.atlas.borrow_mut().pending.push(PendingUpload {
            origin,
            size: [width, height],
            rgba,
        });
        Some(RasterGlyph {
            rect: [origin[0], origin[1], width, height],
            bearing,
        })
    }

    fn emit_glyph(
        &self,
        out: &mut Vec<SpriteInstance>,
        metrics: CharMetrics,
        cursor_x: f32,
        baseline: f32,
        scale: f32,
        depth: f32,
        tint: [f32; 3],
        camera_offset: [f32; 2],
    ) {
        let Some(glyph) = self.rasterize(metrics.face, metrics.glyph) else {
            return;
        };
        let [x, y, width, height] = glyph.rect;
        if width == 0 || height == 0 {
            return;
        }
        out.push(SpriteInstance {
            position: [
                cursor_x + glyph.bearing[0] * scale + camera_offset[0],
                baseline + glyph.bearing[1] * scale + camera_offset[1],
            ],
            size: [width as f32 * scale, height as f32 * scale],
            uv_origin: [x as f32 / ATLAS_SIZE as f32, y as f32 / ATLAS_SIZE as f32],
            uv_size: [
                width as f32 / ATLAS_SIZE as f32,
                height as f32 / ATLAS_SIZE as f32,
            ],
            depth,
            tint,
            alpha: 1.0,
            ..Default::default()
        });
    }

    pub fn build_text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        depth: f32,
        tint: [f32; 3],
        camera_offset: [f32; 2],
    ) -> Vec<SpriteInstance> {
        let mut out = Vec::with_capacity(text.len());
        let mut cursor_x = x;
        let baseline = y + self.ascent * scale;
        for ch in text.chars() {
            match ch {
                '\r' | '\n' => continue,
                '\t' => {
                    cursor_x = tab_advance(cursor_x, x, scale);
                    continue;
                }
                _ => {}
            }
            let Some((metrics, missing)) = self.metrics_or_missing(ch) else {
                continue;
            };
            let tint = if missing {
                crate::render::bit_font::BitFont::missing_color_xor(tint)
            } else {
                tint
            };
            self.emit_glyph(
                &mut out,
                metrics,
                cursor_x,
                baseline,
                scale,
                depth,
                tint,
                camera_offset,
            );
            cursor_x += metrics.advance * scale;
        }
        out
    }

    /// Path-A variant: one reveal position per UTF-16 code unit, exactly like
    /// the bitmap path. A surrogate half is not a character, so it takes the
    /// `?` stand-in rather than being decoded.
    pub fn build_text_path_a(
        &self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        depth: f32,
        camera_offset: [f32; 2],
        already_consumed: u32,
        reveal: PathAReveal,
    ) -> (Vec<SpriteInstance>, u32) {
        let mut out = Vec::with_capacity(text.len());
        let mut cursor_x = x;
        let baseline = y + self.ascent * scale;
        let mut consumed = already_consumed;
        for unit in text.encode_utf16() {
            let Some(position) = consumed.checked_add(1) else {
                break;
            };
            let Some(encoded_rgb) = encoded_unit_rgb(position, reveal) else {
                break;
            };
            consumed = position;
            if unit == u16::from(b'\r') || unit == u16::from(b'\n') {
                continue;
            }
            if unit == u16::from(b'\t') {
                cursor_x = tab_advance(cursor_x, x, scale);
                continue;
            }
            let tint = encoded_rgb.map(|channel| f32::from(channel) / 255.0);
            let resolved = char::from_u32(u32::from(unit))
                .and_then(|ch| self.metrics_or_missing(ch))
                .or_else(|| self.metrics_for('?').map(|metrics| (metrics, true)));
            let Some((metrics, missing)) = resolved else {
                continue;
            };
            let tint = if missing {
                crate::render::bit_font::BitFont::missing_color_xor(tint)
            } else {
                tint
            };
            self.emit_glyph(
                &mut out,
                metrics,
                cursor_x,
                baseline,
                scale,
                depth,
                tint,
                camera_offset,
            );
            cursor_x += metrics.advance * scale;
        }
        (out, consumed)
    }

    pub fn text_width(&self, text: &str) -> u32 {
        let mut x: f32 = 0.0;
        for ch in text.chars() {
            match ch {
                '\r' | '\n' => continue,
                '\t' => {
                    let advanced = x as u32 + TAB_WIDTH;
                    x = (advanced - ((advanced.saturating_sub(TAB_ORIGIN)) % TAB_WIDTH)) as f32;
                    continue;
                }
                _ => {}
            }
            let Some((metrics, _)) = self.metrics_or_missing(ch) else {
                continue;
            };
            x += metrics.advance;
        }
        x.round().max(0.0) as u32
    }

    pub fn wrap_layout(&self, text: &str, max_width: u32) -> WrapLayout {
        wrap_with_advances(text, max_width, self.cell_height, |ch| {
            self.metrics_or_missing(ch)
                .map(|(metrics, _)| metrics.advance)
        })
    }
}

/// Decide whether one configured entry names a font file or an installed
/// family. An absolute path is always a file; a relative entry with a font
/// extension or a path separator resolves against the retail game folder.
fn classify_entry<'a>(entry: &'a str, font_root: Option<&Path>) -> Option<FontEntry<'a>> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    let path = Path::new(entry);
    if path.is_absolute() {
        return Some(FontEntry::File(path.to_path_buf()));
    }
    if is_font_file_name(entry) {
        let resolved = match font_root {
            Some(root) => root.join(path),
            None => path.to_path_buf(),
        };
        return Some(FontEntry::File(resolved));
    }
    Some(FontEntry::Family(entry))
}

fn is_font_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".ttf", ".otf", ".ttc", ".otc"]
        .iter()
        .any(|extension| lower.ends_with(extension))
        || name.contains('/')
        || name.contains('\\')
}

fn push_family(
    db: &fontdb::Database,
    faces: &mut Vec<FaceRef>,
    seen: &mut HashSet<fontdb::ID>,
    name: &str,
) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let Some(id) = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(name)],
        ..Default::default()
    }) else {
        return;
    };
    if !seen.insert(id) {
        return;
    }
    let Some(index) = db.face(id).map(|face| face.index) else {
        return;
    };
    faces.push(FaceRef {
        id,
        index,
        family: name.to_owned(),
    });
}

/// Load every face of one font file and append it to the chain. A collection
/// (`.ttc`) contributes each face; a repeated path is loaded once.
fn push_font_file(
    db: &mut fontdb::Database,
    faces: &mut Vec<FaceRef>,
    seen: &mut HashSet<fontdb::ID>,
    loaded_files: &mut HashSet<PathBuf>,
    path: &Path,
) {
    if !loaded_files.insert(path.to_path_buf()) {
        return;
    }
    let before = db.len();
    if let Err(error) = db.load_font_file(path) {
        log::warn!(
            "[VERA] UiFont= cannot load font file {}: {error}",
            path.display()
        );
        return;
    }
    let loaded: Vec<(fontdb::ID, u32, String)> = db
        .faces()
        .skip(before)
        .map(|face| {
            (
                face.id,
                face.index,
                face.families
                    .first()
                    .map(|(name, _)| name.clone())
                    .unwrap_or_default(),
            )
        })
        .collect();
    if loaded.is_empty() {
        log::warn!(
            "[VERA] UiFont= {} holds no usable font face",
            path.display()
        );
    } else {
        log::info!(
            "[VERA] UiFont= loaded font file {} as {:?}",
            path.display(),
            loaded
                .iter()
                .map(|(_, _, family)| family.as_str())
                .collect::<Vec<_>>()
        );
    }
    for (id, index, family) in loaded {
        if seen.insert(id) {
            faces.push(FaceRef { id, index, family });
        }
    }
}

fn tab_advance(cursor_x: f32, origin: f32, scale: f32) -> f32 {
    let cell_x = ((cursor_x - origin) / scale) as u32;
    let advanced = cell_x + TAB_WIDTH;
    let next_cell = advanced - ((advanced.saturating_sub(TAB_ORIGIN)) % TAB_WIDTH);
    origin + next_cell as f32 * scale
}

/// Wrapped layout over fractional advances.
///
/// Latin text breaks at its last space, like the native measure/wrap pair.
/// With no space to break at, the line ends *before* the overflowing
/// character, which is how CJK prose without spaces wraps. Kept separate from
/// the glyph lookup so the break rules can be tested without a GPU or an
/// installed font.
fn wrap_with_advances(
    text: &str,
    max_width: u32,
    cell_height: u32,
    advance: impl Fn(char) -> Option<f32>,
) -> WrapLayout {
    if text.is_empty() {
        return WrapLayout::default();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut lines: Vec<LineSpan> = Vec::new();
    let mut line_start_byte: usize = 0;
    let mut line_x: f32 = 0.0;
    let mut chars_on_line: u32 = 0;
    let mut max_line_width: f32 = 0.0;
    let mut last_space: Option<(usize, f32)> = None;
    let mut prev_char: Option<char> = None;
    let mut y_lines: u32 = 1;
    let mut i = 0;
    while i < chars.len() {
        let (byte_off, ch) = chars[i];
        let next_byte_off = chars.get(i + 1).map(|c| c.0).unwrap_or(text.len());
        match ch {
            '\t' => {
                let advanced = line_x as u32 + TAB_WIDTH;
                line_x = (advanced - ((advanced.saturating_sub(TAB_ORIGIN)) % TAB_WIDTH)) as f32;
                prev_char = Some('\t');
                i += 1;
                continue;
            }
            '\r' | '\n' => {
                if ch == '\n' && prev_char == Some('\r') {
                    prev_char = Some('\n');
                    i += 1;
                    continue;
                }
                lines.push(LineSpan {
                    start_byte: line_start_byte,
                    end_byte: byte_off,
                    width: line_x.round() as u32,
                });
                max_line_width = max_line_width.max(line_x);
                line_start_byte = next_byte_off;
                line_x = 0.0;
                chars_on_line = 0;
                last_space = None;
                y_lines += 1;
                prev_char = Some(ch);
                i += 1;
                continue;
            }
            ' ' => last_space = Some((i, line_x)),
            _ => {}
        }
        let Some(glyph_advance) = advance(ch) else {
            prev_char = Some(ch);
            i += 1;
            continue;
        };
        let next_x = line_x + glyph_advance;
        if max_width == 0 || next_x <= max_width as f32 {
            line_x = next_x;
            chars_on_line += 1;
            prev_char = Some(ch);
            i += 1;
        } else if chars_on_line == 0 {
            line_x = next_x;
            chars_on_line = 1;
            prev_char = Some(ch);
            i += 1;
        } else if let Some((space_index, space_x)) = last_space {
            lines.push(LineSpan {
                start_byte: line_start_byte,
                end_byte: chars[space_index].0,
                width: space_x.round() as u32,
            });
            max_line_width = max_line_width.max(space_x);
            i = space_index + 1;
            line_start_byte = chars.get(i).map(|c| c.0).unwrap_or(text.len());
            line_x = 0.0;
            chars_on_line = 0;
            last_space = None;
            prev_char = Some(' ');
            y_lines += 1;
        } else {
            lines.push(LineSpan {
                start_byte: line_start_byte,
                end_byte: byte_off,
                width: line_x.round() as u32,
            });
            max_line_width = max_line_width.max(line_x);
            line_start_byte = byte_off;
            line_x = 0.0;
            chars_on_line = 0;
            last_space = None;
            y_lines += 1;
            // Retry this character on the new line.
        }
    }
    lines.push(LineSpan {
        start_byte: line_start_byte,
        end_byte: text.len(),
        width: line_x.round() as u32,
    });
    max_line_width = max_line_width.max(line_x);
    WrapLayout {
        width: max_line_width.round() as u32,
        height: cell_height * y_lines,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text<'a>(text: &'a str, line: &LineSpan) -> &'a str {
        &text[line.start_byte..line.end_byte]
    }

    #[test]
    fn cjk_text_breaks_between_characters_without_spaces() {
        let text = "你好世界";
        let layout = wrap_with_advances(text, 32, CELL_HEIGHT, |_| Some(16.0));
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(line_text(text, &layout.lines[0]), "你好");
        assert_eq!(line_text(text, &layout.lines[1]), "世界");
        assert_eq!(layout.width, 32);
        assert_eq!(layout.height, CELL_HEIGHT * 2);
    }

    #[test]
    fn latin_text_breaks_at_its_last_space() {
        let text = "hello world";
        let layout = wrap_with_advances(text, 40, CELL_HEIGHT, |_| Some(8.0));
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(line_text(text, &layout.lines[0]), "hello");
        assert_eq!(line_text(text, &layout.lines[1]), "world");
    }

    #[test]
    fn mixed_text_keeps_latin_words_whole_before_cjk_breaking() {
        // "ab 你好": the space is the preferred break, so the CJK run starts
        // the next line instead of splitting "ab".
        let text = "ab 你好";
        let layout = wrap_with_advances(text, 32, CELL_HEIGHT, |ch| {
            Some(if ch.is_ascii() { 8.0 } else { 16.0 })
        });
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(line_text(text, &layout.lines[0]), "ab");
        assert_eq!(line_text(text, &layout.lines[1]), "你好");
    }

    #[test]
    fn explicit_newlines_end_lines_immediately() {
        let text = "a\nb";
        let layout = wrap_with_advances(text, 0, CELL_HEIGHT, |_| Some(8.0));
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(line_text(text, &layout.lines[0]), "a");
        assert_eq!(line_text(text, &layout.lines[1]), "b");
        assert_eq!(layout.height, CELL_HEIGHT * 2);
    }

    #[test]
    fn shelf_places_rows_and_reports_a_full_atlas() {
        let mut shelf = Shelf::new();
        let first = shelf.place(ATLAS_SIZE - GLYPH_PADDING * 2, 10).unwrap();
        assert_eq!(first, [GLYPH_PADDING, GLYPH_PADDING]);
        // The run does not fit beside the first cell, so it starts a new row.
        let second = shelf.place(16, 10).unwrap();
        assert_eq!(second[0], GLYPH_PADDING);
        assert_eq!(second[1], GLYPH_PADDING + 10 + GLYPH_PADDING * 2);
        assert!(!shelf.full);
    }

    #[test]
    fn configured_entries_classify_as_families_or_font_files() {
        let root = Path::new("H:/RA2");
        assert!(matches!(
            classify_entry("Segoe UI", Some(root)),
            Some(FontEntry::Family("Segoe UI"))
        ));
        match classify_entry("Fonts/ui.ttf", Some(root)) {
            Some(FontEntry::File(path)) => {
                assert!(path.is_absolute() == Path::new("H:/RA2").is_absolute());
                assert!(path.ends_with("Fonts/ui.ttf") || path.ends_with("Fonts\\ui.ttf"));
            }
            _ => panic!("a relative font file should resolve against the game folder"),
        }
        match classify_entry(r"C:\Fonts\ui.OTF", None) {
            Some(FontEntry::File(path)) => assert_eq!(path, Path::new(r"C:\Fonts\ui.OTF")),
            _ => panic!("an absolute font file should load as given"),
        }
        assert!(classify_entry("   ", Some(root)).is_none());
    }

    #[test]
    #[ignore = "requires a GPU and an installed system font; checks the swash path end to end"]
    fn rasterises_an_installed_family_through_swash() {
        let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
        let batch = BatchRenderer::new_with_device(
            &gpu.device,
            &gpu.queue,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let Some(font) = VectorFont::new(&gpu.device, &batch, None, &["Segoe UI".to_owned()])
        else {
            return;
        };
        let Some(metrics) = font.metrics_for('A') else {
            return;
        };
        let glyph = font
            .rasterize(metrics.face, metrics.glyph)
            .expect("an outline font draws 'A'");
        assert!(glyph.rect[2] > 0 && glyph.rect[3] > 0);
        // A CJK character exercises the fallback chain when the system has a
        // CJK face; a machine without one is not a failure.
        let cjk = font.metrics_for('你').map(|metrics| {
            (
                font.faces[metrics.face].family.clone(),
                font.rasterize(metrics.face, metrics.glyph)
                    .map(|glyph| glyph.rect),
            )
        });
        if let Some((_, Some(rect))) = cjk {
            assert!(rect[2] > 0 && rect[3] > 0);
        }
        font.flush(&gpu.queue);
    }
}
