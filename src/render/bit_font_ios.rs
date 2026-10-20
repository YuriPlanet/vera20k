//! iOS UI glyphs matching WERHD's `Fira Sans Condensed`, weight 500, and its
//! system sans-serif Chinese fallback. CoreText resolves the bundled Medium
//! face's cascade; Apple fonts are used on-device and are never redistributed.
//!
//! GAME.FNT remains the layout authority. Only a glyph's coverage mask changes:
//! every bitmap has the original width and row count. The fitting transform
//! contains bearings, accents and descenders inside that cell, so all ordinary,
//! reveal/Path-A and sidebar text consume the same unclipped atlas.
//!
//! The small FFI surface follows the CoreText/CoreGraphics SDK declarations.
//! It needs no new crate, a font registration, or a second full CJK atlas.

use std::ffi::c_void;
use std::ptr;

type Ref = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    origin: Point,
    size: Size,
}

#[repr(C)]
struct Range {
    location: isize,
    length: isize,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: Ref);
    fn CFStringCreateWithCharacters(allocator: Ref, chars: *const u16, count: isize) -> Ref;
    fn CFStringGetCString(string: Ref, buffer: *mut u8, capacity: isize, encoding: u32) -> bool;
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    fn CTFontCreateWithGraphicsFont(font: Ref, size: f64, matrix: Ref, attributes: Ref) -> Ref;
    fn CTFontCopyPostScriptName(font: Ref) -> Ref;
    fn CTFontCreateForStringWithLanguage(
        font: Ref,
        string: Ref,
        range: Range,
        language: Ref,
    ) -> Ref;
    fn CTFontGetGlyphsForCharacters(
        font: Ref,
        chars: *const u16,
        glyphs: *mut u16,
        count: isize,
    ) -> bool;
    fn CTFontGetBoundingRectsForGlyphs(
        font: Ref,
        orientation: u32,
        glyphs: *const u16,
        bounds: *mut Rect,
        count: isize,
    ) -> Rect;
    fn CTFontGetAdvancesForGlyphs(
        font: Ref,
        orientation: u32,
        glyphs: *const u16,
        advances: *mut Size,
        count: isize,
    ) -> f64;
    fn CTFontDrawGlyphs(
        font: Ref,
        glyphs: *const u16,
        positions: *const Point,
        count: usize,
        ctx: Ref,
    );
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGDataProviderCreateWithData(
        info: Ref,
        data: Ref,
        size: usize,
        release_data: Option<unsafe extern "C" fn(Ref, Ref, usize)>,
    ) -> Ref;
    fn CGDataProviderRelease(provider: Ref);
    fn CGFontCreateWithDataProvider(provider: Ref) -> Ref;
    fn CGFontRelease(font: Ref);
    fn CGColorSpaceCreateDeviceRGB() -> Ref;
    fn CGColorSpaceRelease(space: Ref);
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: Ref,
        bitmap_info: u32,
    ) -> Ref;
    fn CGContextRelease(context: Ref);
    fn CGContextSetRGBFillColor(context: Ref, red: f64, green: f64, blue: f64, alpha: f64);
    fn CGContextSetShouldAntialias(context: Ref, enabled: bool);
    fn CGContextSetShouldSmoothFonts(context: Ref, enabled: bool);
    fn CGContextTranslateCTM(context: Ref, x: f64, y: f64);
    fn CGContextScaleCTM(context: Ref, x: f64, y: f64);
}

/// Owns one object returned by a CoreFoundation/CoreText Create function.
struct Owned(Ref);

impl Owned {
    fn new(object: Ref) -> Option<Self> {
        (!object.is_null()).then(|| Self(object))
    }

    fn string(chars: &[u16]) -> Option<Self> {
        // SAFETY: CoreFoundation copies this valid slice during the call.
        Self::new(unsafe {
            CFStringCreateWithCharacters(ptr::null(), chars.as_ptr(), chars.len() as isize)
        })
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: each non-null Create result has exactly one owning wrapper.
        unsafe { CFRelease(self.0) };
    }
}

pub(super) struct Rasterizer {
    latin: Owned,
    chinese: Owned,
    language: Owned,
    color_space: Ref,
    rows: u32,
}

impl Drop for Rasterizer {
    fn drop(&mut self) {
        // SAFETY: this object owns the successful CreateDeviceRGB result.
        unsafe { CGColorSpaceRelease(self.color_space) };
    }
}

impl Rasterizer {
    pub(super) fn face_names(&self) -> (String, String) {
        (face_name(self.latin.0), face_name(self.chinese.0))
    }

    pub(super) fn new(rows: u32) -> Option<Self> {
        if rows == 0 {
            return None;
        }
        // Keep the complete OFL notice alongside the embedded font bytes in
        // shipping binaries, even when a packager does not copy data files.
        std::hint::black_box(include_str!("../../art/fonts/FiraSansCondensed-OFL"));
        // Static bytes outlive the provider and every font derived from it.
        let bytes = include_bytes!("../../art/fonts/FiraSansCondensed-Medium.ttf");
        // SAFETY: all Create calls receive live buffers; temporary CG owners are
        // released after the CoreText font has retained its graphics font.
        let latin = unsafe {
            let provider =
                CGDataProviderCreateWithData(ptr::null(), bytes.as_ptr().cast(), bytes.len(), None);
            if provider.is_null() {
                return None;
            }
            let graphics = CGFontCreateWithDataProvider(provider);
            CGDataProviderRelease(provider);
            if graphics.is_null() {
                return None;
            }
            let font =
                CTFontCreateWithGraphicsFont(graphics, f64::from(rows), ptr::null(), ptr::null());
            CGFontRelease(graphics);
            Owned::new(font)?
        };
        let language = Owned::string(&"zh-Hans".encode_utf16().collect::<Vec<_>>())?;
        let sample = Owned::string(&[0x4e2d])?; // 中
        // The Fira Medium cascade resolves PingFangSC-Medium on Apple Chinese
        // systems. Use public matching, never a private system font name.
        let chinese = Owned::new(unsafe {
            CTFontCreateForStringWithLanguage(
                latin.0,
                sample.0,
                Range {
                    location: 0,
                    length: 1,
                },
                language.0,
            )
        })?;
        // SAFETY: returned color space is retained until this rasterizer drops.
        let color_space = unsafe { CGColorSpaceCreateDeviceRGB() };
        if color_space.is_null() {
            return None;
        }
        Some(Self {
            latin,
            chinese,
            language,
            color_space,
            rows,
        })
    }

    pub(super) fn glyph(&mut self, codepoint: u16, width: u32) -> Option<Vec<u8>> {
        if width == 0 || (0xd800..=0xdfff).contains(&codepoint) {
            return None;
        }
        let fallback;
        let (font, glyph) = if let Some(glyph) = lookup(self.latin.0, codepoint) {
            (self.latin.0, glyph)
        } else if let Some(glyph) = lookup(self.chinese.0, codepoint) {
            (self.chinese.0, glyph)
        } else {
            let string = Owned::string(&[codepoint])?;
            // SAFETY: matching uses live owned font/string/language objects.
            fallback = Owned::new(unsafe {
                CTFontCreateForStringWithLanguage(
                    self.latin.0,
                    string.0,
                    Range {
                        location: 0,
                        length: 1,
                    },
                    self.language.0,
                )
            })?;
            (fallback.0, lookup(fallback.0, codepoint)?)
        };
        let mut bounds = Rect::default();
        let mut advance = Size::default();
        // SAFETY: the two output records and glyph buffer each hold one item.
        unsafe {
            CTFontGetBoundingRectsForGlyphs(font, 1, &glyph, &mut bounds, 1);
            CTFontGetAdvancesForGlyphs(font, 1, &glyph, &mut advance, 1);
        }
        let (scale, origin) = fit_cell(bounds, advance.width, width, self.rows)?;
        let mut rgba = vec![
            0u8;
            (width as usize)
                .checked_mul(self.rows as usize)?
                .checked_mul(4)?
        ];
        // kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big is RGBA8.
        // The destination slice cannot move or be read until ContextRelease.
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                width as usize,
                self.rows as usize,
                8,
                width as usize * 4,
                self.color_space,
                1 | (4 << 12),
            )
        };
        if context.is_null() {
            return None;
        }
        // SAFETY: the bitmap context, font, glyph and point are live. Quartz
        // stores this bitmap top row first while glyph coordinates point up.
        unsafe {
            CGContextSetShouldAntialias(context, true);
            CGContextSetShouldSmoothFonts(context, false);
            CGContextSetRGBFillColor(context, 1.0, 1.0, 1.0, 1.0);
            CGContextTranslateCTM(context, origin.x, origin.y);
            CGContextScaleCTM(context, scale.width, scale.height);
            CTFontDrawGlyphs(font, &glyph, &Point::default(), 1, context);
            CGContextRelease(context);
        }
        // Quartz emitted premultiplied white. The sprite shader expects a
        // straight-alpha coverage mask; retaining premultiplied RGB would make
        // every antialiased stroke darker and thinner after a second multiply.
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[..3].fill(255);
        }
        Some(rgba)
    }
}

fn lookup(font: Ref, codepoint: u16) -> Option<u16> {
    let mut glyph = 0;
    // SAFETY: input/output each point to exactly one UTF-16 code unit/glyph.
    let found = unsafe { CTFontGetGlyphsForCharacters(font, &codepoint, &mut glyph, 1) };
    (found && glyph != 0).then_some(glyph)
}

fn face_name(font: Ref) -> String {
    // SAFETY: the live font supplies a retained CFString; the output buffer is
    // writable for its entire advertised capacity. Encoding is UTF-8.
    let Some(name) = Owned::new(unsafe { CTFontCopyPostScriptName(font) }) else {
        return "unknown".to_string();
    };
    let mut bytes = [0u8; 256];
    if !unsafe {
        CFStringGetCString(
            name.0,
            bytes.as_mut_ptr(),
            bytes.len() as isize,
            0x0800_0100,
        )
    } {
        return "unknown".to_string();
    }
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Preserve a common baseline unless an accent/descender needs containment.
/// Advance remains native. A narrow bitmap slot compresses only the horizontal
/// outline: tying vertical scale to that slot made the 4px digit slots render
/// at half height. Only a glyph taller than the bitmap cell shrinks vertically.
fn fit_cell(bounds: Rect, advance: f64, width: u32, rows: u32) -> Option<(Size, Point)> {
    if width == 0
        || rows == 0
        || ![
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
            advance,
        ]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let margin = 0.25;
    let available_width = (f64::from(width) - 2.0 * margin).max(0.5);
    let available_height = (f64::from(rows) - 2.0 * margin).max(0.5);
    let source_width = advance.max(bounds.size.width).max(0.5);
    let source_height = bounds.size.height.max(0.5);
    let scale = Size {
        width: (available_width / source_width).min(1.0),
        height: (available_height / source_height).min(1.0),
    };
    let mut origin = Point {
        x: (f64::from(width) - advance * scale.width) / 2.0,
        y: f64::from(rows) * 0.22,
    };
    let left = origin.x + bounds.origin.x * scale.width;
    let bottom = origin.y + bounds.origin.y * scale.height;
    origin.x += margin
        .max(left)
        .min(f64::from(width) - margin - bounds.size.width * scale.width)
        - left;
    origin.y += margin
        .max(bottom)
        .min(f64::from(rows) - margin - bounds.size.height * scale.height)
        - bottom;
    Some((scale, origin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearings_accents_and_descenders_fit_the_native_cell() {
        // Bounds observed from Fira/PingFang Medium at 16px, including negative
        // left bearings and a descender taller than GAME.FNT's 16-row cell.
        for (x, y, w, h, advance, width) in [
            (-0.496, -3.472, 3.776, 16.192, 4.24, 4), // j
            (-0.048, 0.0, 8.656, 14.912, 8.48, 8),    // Á
            (0.656, -1.744, 14.88, 14.624, 16.0, 14), // 设
        ] {
            let bounds = Rect {
                origin: Point { x, y },
                size: Size {
                    width: w,
                    height: h,
                },
            };
            let (scale, origin) = fit_cell(bounds, advance, width, 16).unwrap();
            assert!(origin.x + x * scale.width >= 0.249);
            assert!(origin.y + y * scale.height >= 0.249);
            assert!(origin.x + (x + w) * scale.width <= f64::from(width) - 0.249);
            assert!(origin.y + (y + h) * scale.height <= 15.751);
        }
    }

    #[test]
    fn latin_and_chinese_medium_masks_keep_native_dimensions_and_alpha() {
        let mut font = Rasterizer::new(16).expect("bundled Fira + system Chinese cascade");
        let (latin, chinese) = font.face_names();
        assert_eq!(latin, "FiraSansCondensed-Medium");
        assert!(
            chinese.ends_with("-Medium"),
            "Chinese cascade face: {chinese}"
        );
        for (ch, width) in [
            ('W', 12),
            ('j', 4),
            ('Á', 8),
            ('遭', 14),
            ('设', 16),
            ('，', 16),
        ] {
            let rgba = font.glyph(ch as u16, width).unwrap();
            assert_eq!(rgba.len(), width as usize * 16 * 4);
            assert!(rgba.chunks_exact(4).all(|pixel| pixel[..3] == [255; 3]));
            assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
            assert!(
                rgba.chunks_exact(4)
                    .any(|pixel| (1..255).contains(&pixel[3]))
            );
        }
        assert!(font.glyph(0xd800, 14).is_none());
        assert!(font.glyph(b'W' as u16, 0).is_none());
    }

    #[test]
    fn narrow_native_slots_keep_digit_and_latin_height() {
        fn visible_rows(rgba: &[u8], width: u32) -> (usize, usize) {
            let mut rows = rgba
                .chunks_exact(width as usize * 4)
                .enumerate()
                .filter_map(|(row, pixels)| {
                    pixels.chunks_exact(4).any(|p| p[3] >= 32).then_some(row)
                });
            let first = rows.next().expect("visible glyph");
            (first, rows.last().unwrap_or(first))
        }

        let mut font = Rasterizer::new(16).unwrap();
        for ch in '0'..='9' {
            for width in [4, 5, 6] {
                let rgba = font.glyph(ch as u16, width).unwrap();
                let (top, bottom) = visible_rows(&rgba, width);
                assert!(
                    bottom + 1 - top >= 10,
                    "digit {ch}, native width {width}: rows {top}..={bottom}"
                );
            }
        }
        // Punctuation keeps its natural small height; it must not shrink further
        // merely because its original bitmap advance is narrower than Fira's.
        for (ch, width) in [
            ('B', 5),
            ('X', 5),
            ('Y', 5),
            ('.', 2),
            (',', 2),
            (':', 2),
            (';', 2),
            ('!', 2),
            ('?', 4),
            ('-', 3),
        ] {
            let wide = visible_rows(&font.glyph(ch as u16, 16).unwrap(), 16);
            let narrow = visible_rows(&font.glyph(ch as u16, width).unwrap(), width);
            assert!(
                wide.0.abs_diff(narrow.0) <= 1 && wide.1.abs_diff(narrow.1) <= 1,
                "{ch}, native width {width}: wide rows {wide:?}, narrow rows {narrow:?}"
            );
        }
    }
}
