//! VERA-only, experimental picture art (`Picture=`,
//! `docs/picture-model-art.md`): the header facts of a PNG, APNG or WebP
//! (still or animated) that the simulation takes where it takes an SHP
//! header's, read without decoding a pixel, its frames composited one at a
//! time ([`FrameStream`]) or all together ([`PictureFrames`]), and the one
//! lookup that finds a picture file by name ([`find_picture`]).

use std::borrow::Cow;

/// A picture's canvas, in its own pixels, and its frame count: the APNG's
/// animation frames (`acTL`) or the animated WebP's (`ANMF`), or 1 for a
/// plain PNG or a still WebP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureHeader {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum PictureError {
    #[error("not a readable PNG: {0}")]
    Png(#[from] png::DecodingError),
    #[error("not a readable WebP: {0}")]
    Webp(#[from] image_webp::DecodingError),
    #[error("an APNG with no frames")]
    NoFrames,
    #[error("{0} decoded bytes exceeds the picture storage limit of {PICTURE_BYTES_MAX}")]
    TooLarge(u64),
    #[error("a canvas too large to decode on this platform")]
    CanvasTooLarge,
    #[error("a frame region outside the canvas")]
    FrameOutside,
    #[error("a WebP animation frame that is not its ANMF size")]
    FrameSize,
}

/// Maximum bytes for a stream's working canvases or one retained set of
/// decoded frames. A streamed animation may decode more bytes over its lifetime.
pub const PICTURE_BYTES_MAX: u64 = 512 << 20;

pub fn check_picture_storage(bytes: u64) -> Result<(), PictureError> {
    if bytes > PICTURE_BYTES_MAX {
        Err(PictureError::TooLarge(bytes))
    } else {
        Ok(())
    }
}

/// A picture's frames composited onto its canvas, as an APNG viewer shows
/// them: straight RGBA, row by row, `width * height * 4` bytes each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureFrames {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
}

/// The frames of a PNG, APNG or WebP composited one at a time, in order,
/// onto one reused canvas, as a viewer shows them. An APNG's default image
/// that is not part of its animation is skipped; each animation frame is
/// drawn onto the canvas by its blend operation and disposed of by its
/// dispose operation (APNG 1.0, `fcTL`; WebP `ANMF`) just before the next
/// frame is drawn. The format is told by the bytes' signature, not the file
/// name, so a picture named `.png` that holds a WebP still reads.
///
/// It holds the canvas, the decoder's output buffer for one frame (at most a
/// canvas), and, only while a `Previous`-disposed APNG frame shows, the
/// region that frame covers. So memory stays at about two canvases whatever
/// the frame count. Frames come strictly in order: compositing depends on
/// every frame before, so reaching frame `n` means drawing frames `0..n`.
pub struct FrameStream<'a> {
    decoder: Decoder<'a>,
    width: u32,
    height: u32,
    frame_count: usize,
    /// The frame `next_frame` draws next.
    next: usize,
    canvas: Vec<u8>,
}

/// The format's own reader and what it keeps between frames. One lives in
/// each stream, never in a collection, so the variants' sizes need not match.
#[allow(clippy::large_enum_variant)]
enum Decoder<'a> {
    Png(PngFrames<'a>),
    Webp(WebpFrames<'a>),
}

/// A PNG or APNG being read: the frames are composited here
/// ([`PngFrames::draw_next`]), since the `png` crate gives each frame's own
/// region only.
struct PngFrames<'a> {
    reader: png::Reader<std::io::Cursor<&'a [u8]>>,
    /// The decoder's output for one frame, its own region's samples.
    buffer: Vec<u8>,
    /// What the frame on the canvas leaves when it is disposed of.
    pending: Dispose,
    /// The canvas region under a `Previous`-disposed frame, before it drew.
    saved: Vec<u8>,
}

/// A WebP being read. An animation's frames are composited here
/// ([`WebpFrames::draw_next`]): `image-webp` 0.2.4's own compositing clears
/// a disposed frame's region with RGB strides when the next frame has no
/// alpha, on an RGBA canvas, so pixels of the frame before stay. Each `ANMF`
/// frame's bitstream is decoded by `image-webp` as a still image instead.
struct WebpFrames<'a> {
    bytes: &'a [u8],
    /// The `ANMF` frames, in order; empty for a still WebP.
    frames: Vec<AnmfFrame>,
    /// The decoder's output for one frame (RGB or RGBA), at most a canvas.
    buffer: Vec<u8>,
    /// The region the frame on the canvas clears to transparent before the
    /// next frame draws (WebP "dispose to background", transparent as an
    /// APNG's Background is).
    pending: Option<[u32; 4]>,
}

/// One `ANMF` chunk: its region on the canvas, its blend and dispose bits,
/// and where its frame bitstream (`ALPH` + `VP8 `, or `VP8L`) lies.
#[derive(Debug, Clone)]
struct AnmfFrame {
    region: [u32; 4],
    blend: bool,
    dispose: bool,
    payload: std::ops::Range<usize>,
}

/// The `ANMF` frames of the WebP `bytes`, from its chunk list (RIFF, each
/// chunk padded to even length).
fn anmf_frames(bytes: &[u8]) -> Result<Vec<AnmfFrame>, PictureError> {
    let truncated = || PictureError::Webp(image_webp::DecodingError::InvalidChunkSize);
    let u24 = |b: &[u8]| u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16;
    let mut frames = Vec::new();
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let start = at + 8;
        let end = start
            .checked_add(size)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(truncated)?;
        if &bytes[at..at + 4] == b"ANMF" {
            if size < 16 {
                return Err(truncated());
            }
            let header = &bytes[start..start + 16];
            frames.push(AnmfFrame {
                region: [
                    u24(&header[0..3]) * 2,
                    u24(&header[3..6]) * 2,
                    u24(&header[6..9]) + 1,
                    u24(&header[9..12]) + 1,
                ],
                blend: header[15] & 0b10 == 0,
                dispose: header[15] & 0b01 != 0,
                payload: start + 16..end,
            });
        }
        at = end + (size & 1);
    }
    Ok(frames)
}

/// The `ANMF` frame bitstream `payload` (`w` x `h`) as a still WebP file:
/// with a `VP8X` header saying alpha when it starts with an `ALPH` chunk.
fn still_webp(payload: &[u8], [w, h]: [u32; 2]) -> Vec<u8> {
    let alpha = payload.starts_with(b"ALPH");
    let mut body = b"WEBP".to_vec();
    if alpha {
        body.extend_from_slice(b"VP8X");
        body.extend_from_slice(&10u32.to_le_bytes());
        body.extend_from_slice(&[0x10, 0, 0, 0]);
        body.extend_from_slice(&(w - 1).to_le_bytes()[..3]);
        body.extend_from_slice(&(h - 1).to_le_bytes()[..3]);
    }
    body.extend_from_slice(payload);
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

/// The disposal still owed by the frame on the canvas: its region and op.
#[derive(Debug, Clone, Copy)]
struct Dispose {
    region: [u32; 4],
    op: png::DisposeOp,
}

/// Whether `bytes` are a WebP (`RIFF` size `WEBP`); anything else is read as
/// a PNG, so a PNG costs one signature compare more than before.
fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes[..4] == *b"RIFF" && bytes[8..12] == *b"WEBP"
}

impl<'a> FrameStream<'a> {
    /// Start reading the PNG, APNG or WebP `bytes`: its header, and past an
    /// APNG default image that is not an animation frame.
    pub fn new(bytes: &'a [u8]) -> Result<Self, PictureError> {
        if is_webp(bytes) {
            return Self::new_webp(bytes);
        }
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info()?;
        let info = reader.info();
        let (width, height) = (info.width, info.height);
        let animation = info.animation_control;
        let frame_count = animation.map_or(1, |control| control.num_frames);
        if frame_count == 0 {
            return Err(PictureError::NoFrames);
        }
        let default_in_animation = animation.is_none() || info.frame_control.is_some();
        let canvas_bytes = (u64::from(width) * u64::from(height)).saturating_mul(4);
        let buffer_len = reader
            .output_buffer_size()
            .ok_or(PictureError::CanvasTooLarge)?;
        // The decoder's output, composited canvas and a possible Previous
        // disposal backup can coexist. Check the header before reserving any.
        check_picture_storage(
            canvas_bytes
                .saturating_mul(2)
                .saturating_add(buffer_len as u64),
        )?;
        let canvas_len = usize::try_from(canvas_bytes).map_err(|_| PictureError::CanvasTooLarge)?;
        let mut buffer = vec![0; buffer_len];
        if !default_in_animation {
            reader.next_frame(&mut buffer)?;
        }
        Ok(Self {
            decoder: Decoder::Png(PngFrames {
                reader,
                buffer,
                pending: Dispose {
                    region: [0; 4],
                    op: png::DisposeOp::None,
                },
                saved: Vec::new(),
            }),
            width,
            height,
            frame_count: usize::try_from(frame_count).map_err(|_| PictureError::CanvasTooLarge)?,
            next: 0,
            canvas: vec![0; canvas_len],
        })
    }

    /// [`Self::new`] for a WebP: its header only, no pixel decoded yet.
    fn new_webp(bytes: &'a [u8]) -> Result<Self, PictureError> {
        let decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))?;
        let (width, height) = decoder.dimensions();
        let frames = if decoder.is_animated() {
            anmf_frames(bytes)?
        } else {
            Vec::new()
        };
        let frame_count = if decoder.is_animated() {
            frames.len()
        } else {
            1
        };
        if frame_count == 0 {
            return Err(PictureError::NoFrames);
        }
        for frame in &frames {
            let [x, y, w, h] = frame.region;
            if x.checked_add(w).is_none_or(|right| right > width)
                || y.checked_add(h).is_none_or(|bottom| bottom > height)
            {
                return Err(PictureError::FrameOutside);
            }
        }
        let canvas_bytes = (u64::from(width) * u64::from(height)).saturating_mul(4);
        // The canvas and one frame's output (at most a canvas) coexist.
        check_picture_storage(canvas_bytes.saturating_mul(2))?;
        let canvas_len = usize::try_from(canvas_bytes).map_err(|_| PictureError::CanvasTooLarge)?;
        Ok(Self {
            decoder: Decoder::Webp(WebpFrames {
                bytes,
                frames,
                buffer: Vec::new(),
                pending: None,
            }),
            width,
            height,
            frame_count: usize::try_from(frame_count).map_err(|_| PictureError::CanvasTooLarge)?,
            next: 0,
            canvas: vec![0; canvas_len],
        })
    }

    /// The canvas, `[width, height]` in picture pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// The animation frames (`acTL`, `ANMF`), or 1 for a plain PNG or a
    /// still WebP.
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// The next frame composited onto the canvas, `width * height * 4`
    /// straight RGBA bytes, row by row; None past the last frame or after an
    /// error. The bytes are the stream's own canvas, overwritten by the next
    /// call.
    pub fn next_frame(&mut self) -> Option<Result<&[u8], PictureError>> {
        if self.next >= self.frame_count {
            return None;
        }
        match self.draw_next() {
            Ok(()) => {
                self.next += 1;
                Some(Ok(self.canvas.as_slice()))
            }
            Err(error) => {
                self.next = self.frame_count;
                Some(Err(error))
            }
        }
    }

    /// Draw the next frame onto the canvas, by the format's own reader.
    fn draw_next(&mut self) -> Result<(), PictureError> {
        let size = [self.width, self.height];
        match &mut self.decoder {
            Decoder::Png(frames) => frames.draw_next(&mut self.canvas, size, self.next),
            Decoder::Webp(frames) => frames.draw_next(&mut self.canvas, size, self.next),
        }
    }

    /// The bytes of canvases and buffers this stream holds, for tests of its
    /// memory bound.
    #[cfg(all(test, tests_assets))]
    fn held_bytes(&self) -> usize {
        self.canvas.capacity()
            + match &self.decoder {
                Decoder::Png(frames) => frames.buffer.capacity() + frames.saved.capacity(),
                Decoder::Webp(frames) => frames.buffer.capacity(),
            }
    }
}

impl WebpFrames<'_> {
    /// Draw frame `index` onto `canvas` (`size`): a still WebP is the whole
    /// canvas; an animation frame clears the region the frame before
    /// disposes of, then draws its own region by its blend bit (WebP
    /// container spec: alpha-blend, or overwrite).
    fn draw_next(
        &mut self,
        canvas: &mut [u8],
        size: [u32; 2],
        index: usize,
    ) -> Result<(), PictureError> {
        let [width, _] = size;
        let (still, region, blend, dispose) = match self.frames.get(index) {
            Some(frame) => {
                let [_, _, w, h] = frame.region;
                let still = still_webp(&self.bytes[frame.payload.clone()], [w, h]);
                (Some(still), frame.region, frame.blend, frame.dispose)
            }
            None => (None, [0, 0, size[0], size[1]], false, false),
        };
        let bytes = still.as_deref().unwrap_or(self.bytes);
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))?;
        let [x, y, w, h] = region;
        if decoder.dimensions() != (w, h) {
            return Err(PictureError::FrameSize);
        }
        let channels = if decoder.has_alpha() { 4 } else { 3 };
        let len = decoder
            .output_buffer_size()
            .ok_or(PictureError::CanvasTooLarge)?;
        self.buffer.resize(len, 0);
        decoder.read_image(&mut self.buffer)?;
        if let Some([px, py, pw, ph]) = self.pending.take() {
            for row in canvas
                .chunks_exact_mut(width as usize * 4)
                .skip(py as usize)
                .take(ph as usize)
            {
                row[px as usize * 4..(px + pw) as usize * 4].fill(0);
            }
        }
        for (row, source) in self.buffer.chunks_exact(w as usize * channels).enumerate() {
            let start = ((y as usize + row) * width as usize + x as usize) * 4;
            let target = &mut canvas[start..start + w as usize * 4];
            for (dst, src) in target
                .chunks_exact_mut(4)
                .zip(source.chunks_exact(channels))
            {
                let alpha = if channels == 4 { src[3] } else { 255 };
                let source = [src[0], src[1], src[2], alpha];
                // The WebP container spec's alpha-blend is APNG's OVER.
                let drawn = if blend {
                    blend_over(source, (&*dst).try_into().unwrap())
                } else {
                    source
                };
                dst.copy_from_slice(&drawn);
            }
        }
        if dispose {
            self.pending = Some(region);
        }
        Ok(())
    }
}

impl PngFrames<'_> {
    /// Dispose of the frame on `canvas` (`size`), then decode frame `index`
    /// and draw it by its blend operation.
    fn draw_next(
        &mut self,
        canvas: &mut [u8],
        size: [u32; 2],
        index: usize,
    ) -> Result<(), PictureError> {
        let [width, height] = size;
        self.dispose(canvas, width);
        let output = self.reader.next_frame(&mut self.buffer)?;
        let control = self.reader.info().frame_control;
        let region = control.map_or([0, 0, width, height], |control| {
            [
                control.x_offset,
                control.y_offset,
                control.width,
                control.height,
            ]
        });
        let [x, y, w, h] = region;
        if x.checked_add(w).is_none_or(|right| right > width)
            || y.checked_add(h).is_none_or(|bottom| bottom > height)
        {
            return Err(PictureError::FrameOutside);
        }
        let op = control.map_or(png::DisposeOp::None, |control| control.dispose_op);
        // A first frame's Previous disposal acts as Background (APNG 1.0).
        let op = match (index, op) {
            (0, png::DisposeOp::Previous) => png::DisposeOp::Background,
            (_, op) => op,
        };
        self.pending = Dispose { region, op };
        let row_bytes = (w * 4) as usize;
        if op == png::DisposeOp::Previous {
            self.saved.clear();
            for row in y..y + h {
                let start = ((row * width + x) * 4) as usize;
                self.saved
                    .extend_from_slice(&canvas[start..start + row_bytes]);
            }
        }
        let over = control.is_some_and(|control| control.blend_op == png::BlendOp::Over);
        let samples = &self.buffer[..output.buffer_size()];
        let color = output.color_type;
        for row in 0..h {
            let to = (((y + row) * width + x) * 4) as usize;
            let target = &mut canvas[to..to + row_bytes];
            let first = (row * w) as usize;
            // RGBA output replacing the canvas is copied as it is; anything
            // else goes pixel by pixel.
            if !over && color == png::ColorType::Rgba {
                target.copy_from_slice(&samples[first * 4..first * 4 + row_bytes]);
                continue;
            }
            for (column, target) in target.chunks_exact_mut(4).enumerate() {
                let source = rgba_sample(samples, color, first + column);
                let drawn = match (over, source[3]) {
                    (false, _) | (true, 255) => source,
                    (true, 0) => continue,
                    (true, _) => blend_over(source, (&*target).try_into().unwrap()),
                };
                target.copy_from_slice(&drawn);
            }
        }
        Ok(())
    }

    /// Apply the disposal the frame on `canvas`, `width` wide, still owes.
    fn dispose(&mut self, canvas: &mut [u8], width: u32) {
        let Dispose {
            region: [x, y, w, h],
            op,
        } = std::mem::replace(
            &mut self.pending,
            Dispose {
                region: [0; 4],
                op: png::DisposeOp::None,
            },
        );
        if op == png::DisposeOp::None {
            return;
        }
        let row_bytes = (w * 4) as usize;
        for row in 0..h {
            let start = (((y + row) * width + x) * 4) as usize;
            let target = &mut canvas[start..start + row_bytes];
            if op == png::DisposeOp::Previous {
                let from = row as usize * row_bytes;
                target.copy_from_slice(&self.saved[from..from + row_bytes]);
            } else {
                target.fill(0);
            }
        }
    }
}

impl PictureFrames {
    /// Decode and composite every frame of the PNG, APNG or WebP `bytes`
    /// ([`FrameStream`]), refusing a picture whose frames together are more
    /// than [`PICTURE_BYTES_MAX`]: one that big must be streamed.
    pub fn decode(bytes: &[u8]) -> Result<Self, PictureError> {
        let mut stream = FrameStream::new(bytes)?;
        let [width, height] = stream.size();
        let per_frame = u64::from(width) * u64::from(height) * 4 + size_of::<Vec<u8>>() as u64;
        check_picture_storage(per_frame.saturating_mul(stream.frame_count() as u64))?;
        let mut frames = Vec::with_capacity(stream.frame_count());
        while let Some(frame) = stream.next_frame() {
            frames.push(frame?.to_vec());
        }
        Ok(Self {
            width,
            height,
            frames,
        })
    }
}

/// Pixel `index` of decoded 8-bit samples of `color` as straight RGBA.
fn rgba_sample(samples: &[u8], color: png::ColorType, index: usize) -> [u8; 4] {
    match color {
        png::ColorType::Grayscale => {
            let v = samples[index];
            [v, v, v, 255]
        }
        png::ColorType::GrayscaleAlpha => {
            let v = samples[index * 2];
            [v, v, v, samples[index * 2 + 1]]
        }
        png::ColorType::Rgb => {
            let s = &samples[index * 3..index * 3 + 3];
            [s[0], s[1], s[2], 255]
        }
        // Indexed is expanded to RGB or RGBA by `normalize_to_color8`.
        png::ColorType::Rgba | png::ColorType::Indexed => {
            samples[index * 4..index * 4 + 4].try_into().unwrap()
        }
    }
}

/// Straight-alpha `source` over `target` (APNG `APNG_BLEND_OP_OVER`; the
/// WebP container spec's alpha-blend is the same).
fn blend_over(source: [u8; 4], target: [u8; 4]) -> [u8; 4] {
    let source_alpha = u32::from(source[3]);
    if source_alpha == 255 {
        return source;
    }
    if source_alpha == 0 {
        return target;
    }
    let target_alpha = u32::from(target[3]) * (255 - source_alpha) / 255;
    let alpha = source_alpha + target_alpha;
    let channel = |s: u8, t: u8| {
        ((u32::from(s) * source_alpha + u32::from(t) * target_alpha + alpha / 2) / alpha) as u8
    };
    [
        channel(source[0], target[0]),
        channel(source[1], target[1]),
        channel(source[2], target[2]),
        alpha as u8,
    ]
}

impl PictureFrames {
    /// Frame `frame` shrunk by [`shrunk_region`]. None for a frame past the
    /// end or a region outside the canvas.
    pub fn shrunk_region(&self, frame: usize, scale: u32, size: [u32; 2]) -> Option<Vec<u8>> {
        shrunk_region(
            self.frames.get(frame)?,
            [self.width, self.height],
            scale,
            size,
        )
    }

    /// Frame `frame`'s [`mean_color`]. None for a frame past the end or with
    /// nothing drawn.
    pub fn mean_color(&self, frame: usize) -> Option<[u8; 3]> {
        mean_color(self.frames.get(frame)?)
    }
}

/// The top-left `size` game pixels of one composited frame `rgba` (a
/// `canvas[0]` x `canvas[1]` picture, as [`FrameStream::next_frame`] gives),
/// `scale` picture pixels per game pixel on each axis, as straight RGBA: a
/// game pixel is drawn (alpha 255) when at least half its picture pixels are
/// (alpha at least half), its colour their mean; else it is a hole (all 0).
/// The coverage rule `render::picture_atlas` shrinks a picture by, without
/// its mask. None for a region outside the canvas.
pub fn shrunk_region(rgba: &[u8], canvas: [u32; 2], scale: u32, size: [u32; 2]) -> Option<Vec<u8>> {
    let [width, height] = canvas;
    if scale == 0 || size[0] * scale > width || size[1] * scale > height {
        return None;
    }
    let mut out = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let (mut covered, mut sum) = (0u32, [0u32; 3]);
            for dy in 0..scale {
                for dx in 0..scale {
                    let at = (((y * scale + dy) * width + x * scale + dx) * 4) as usize;
                    if u32::from(rgba[at + 3]) * 2 >= 255 {
                        covered += 1;
                        for channel in 0..3 {
                            sum[channel] += u32::from(rgba[at + channel]);
                        }
                    }
                }
            }
            if covered * 2 >= scale * scale {
                let mean = |channel: usize| ((sum[channel] + covered / 2) / covered) as u8;
                out.extend_from_slice(&[mean(0), mean(1), mean(2), 255]);
            } else {
                out.extend_from_slice(&[0; 4]);
            }
        }
    }
    Some(out)
}

/// The mean colour of one composited frame's drawn pixels (alpha at least
/// half), where an SHP frame header holds the radar colour the art author
/// chose (`+0x0C..0x0E`). None with nothing drawn.
pub fn mean_color(rgba: &[u8]) -> Option<[u8; 3]> {
    let (mut count, mut sum) = (0u64, [0u64; 3]);
    for pixel in rgba.chunks_exact(4) {
        if u32::from(pixel[3]) * 2 >= 255 {
            count += 1;
            for channel in 0..3 {
                sum[channel] += u64::from(pixel[channel]);
            }
        }
    }
    (count > 0).then(|| sum.map(|total| ((total + count / 2) / count) as u8))
}

/// VERA-only, experimental (docs/picture-model-art.md): the size, in game
/// pixels, of the rectangle frame `frame` of the picture `bytes` draws on its
/// `canvas` (game pixels; the picture is a whole multiple of it), by
/// [`shrunk_region`]'s coverage rule: what an SHP frame header stores as its
/// own width and height (`+0x08`, `+0x0A`), the rectangle of its drawn
/// pixels. `[0, 0]` for a frame with nothing drawn. None when the picture
/// cannot be read, has no such frame or is not a whole multiple of
/// `canvas`. Frames up to `frame` are composited and dropped
/// ([`FrameStream`]).
pub fn drawn_size(bytes: &[u8], frame: usize, canvas: [i32; 2]) -> Option<[i32; 2]> {
    let mut stream = FrameStream::new(bytes).ok()?;
    let [width, height] = stream.size();
    let [canvas_width, canvas_height] = canvas.map(|side| u32::try_from(side).unwrap_or(0));
    let scale = width.checked_div(canvas_width)?;
    if scale == 0 || canvas_width * scale != width || canvas_height * scale != height {
        return None;
    }
    for _ in 0..frame {
        stream.next_frame()?.ok()?;
    }
    let rgba = stream.next_frame()?.ok()?;
    let shrunk = shrunk_region(rgba, [width, height], scale, [canvas_width, canvas_height])?;
    let mut drawn = shrunk
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, pixel)| pixel[3] != 0)
        .map(|(index, _)| (index as u32 % canvas_width, index as u32 / canvas_width));
    let Some((x, y)) = drawn.next() else {
        return Some([0, 0]);
    };
    let [left, top, right, bottom] = drawn.fold([x, y, x, y], |[l, t, r, b], (x, y)| {
        [l.min(x), t.min(y), r.max(x), b.max(y)]
    });
    Some([(right - left + 1) as i32, (bottom - top + 1) as i32])
}

impl PictureHeader {
    /// The header of the PNG, APNG or WebP `bytes`.
    pub fn read(bytes: &[u8]) -> Result<Self, PictureError> {
        if is_webp(bytes) {
            // The header and the chunk index only; no frame is decoded.
            let decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))?;
            let (width, height) = decoder.dimensions();
            let frames = if decoder.is_animated() {
                decoder.num_frames()
            } else {
                1
            };
            if frames == 0 {
                return Err(PictureError::NoFrames);
            }
            return Ok(Self {
                width,
                height,
                frames,
            });
        }
        let reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info()?;
        let info = reader.info();
        let frames = info
            .animation_control
            .map_or(1, |control| control.num_frames);
        if frames == 0 {
            return Err(PictureError::NoFrames);
        }
        Ok(Self {
            width: info.width,
            height: info.height,
            frames,
        })
    }
}

/// The picture file `name` names, through `lookup` (the caller's own asset
/// lookup), with the name it was found by: `name` itself, else, for a
/// `NAME.png` (`NAME.mask.png` too) that is not there, its WebP twin
/// `NAME.webp`. An art pack may ship every picture as WebP to be small,
/// and art INIs keep naming them `.png`. A name that is there costs one
/// lookup, as before; only a miss asks for the twin. Every lookup of a
/// picture file by name goes through here, so the rule is one.
pub fn find_picture<'n, T>(
    name: &'n str,
    mut lookup: impl FnMut(&str) -> Option<T>,
) -> Option<(Cow<'n, str>, T)> {
    if let Some(found) = lookup(name) {
        return Some((Cow::Borrowed(name), found));
    }
    let twin = webp_twin(name)?;
    let found = lookup(&twin)?;
    Some((Cow::Owned(twin), found))
}

/// `NAME.webp` for `NAME.png` (in the extension's case when it is all one
/// case); None for any other name.
fn webp_twin(name: &str) -> Option<String> {
    let (stem, extension) = name.rsplit_once('.')?;
    if !extension.eq_ignore_ascii_case("png") {
        return None;
    }
    let webp = if extension == "PNG" { "WEBP" } else { "webp" };
    Some(format!("{stem}.{webp}"))
}

/// Encode `frames`, each `width * height * 4` straight RGBA bytes, as a PNG
/// (one frame) or an APNG of whole-canvas frames that replace the canvas.
pub fn encode_frames(
    width: u32,
    height: u32,
    frames: &[Vec<u8>],
) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if frames.len() > 1 {
        let count = u32::try_from(frames.len()).unwrap_or(u32::MAX);
        encoder.set_animated(count, 0)?;
    }
    let mut writer = encoder.write_header()?;
    for frame in frames {
        writer.write_image_data(frame)?;
    }
    writer.finish()?;
    Ok(bytes)
}

/// An RGBA PNG of `width` x `height`, with `frames` APNG frames when some,
/// for tests of picture readers.
#[cfg(test)]
pub(crate) fn test_picture(width: u32, height: u32, frames: Option<u32>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Some(frames) = frames {
        encoder.set_animated(frames, 0).unwrap();
    }
    let mut writer = encoder.write_header().unwrap();
    let pixels = vec![0u8; (width * height * 4) as usize];
    for _ in 0..frames.unwrap_or(1) {
        writer.write_image_data(&pixels).unwrap();
    }
    writer.finish().unwrap();
    bytes
}

/// A lossless WebP of `frames`, each `[x, y, width, height]` (`x` and `y`
/// even), its `ANMF` flags (bit 1 no blending, bit 0 dispose to the
/// background) and its straight RGBA: a still WebP for one whole-canvas
/// frame with no flags, else an animation on a `canvas` with alpha. For
/// tests of picture readers; `image-webp` encodes stills only, so the
/// animation container is assembled here (WebP container spec, `VP8X`,
/// `ANIM`, `ANMF`).
#[cfg(test)]
pub(crate) fn test_webp(canvas: [u32; 2], frames: &[([u32; 4], u8, Vec<u8>)]) -> Vec<u8> {
    let still = |[_, _, width, height]: [u32; 4], rgba: &[u8]| {
        let mut bytes = Vec::new();
        image_webp::WebPEncoder::new(&mut bytes)
            .encode(rgba, width, height, image_webp::ColorType::Rgba8)
            .unwrap();
        bytes
    };
    if let [(region, 0, rgba)] = frames
        && region[2..] == canvas[..]
    {
        return still(*region, &rgba[..]);
    }
    let chunk = |name: &[u8; 4], data: &[u8]| {
        let mut bytes = name.to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        if data.len() % 2 == 1 {
            bytes.push(0);
        }
        bytes
    };
    let u24 = |value: u32| value.to_le_bytes()[..3].to_vec();
    // Flags: alpha (0x10) and animation (0x02).
    let mut header = vec![0x12, 0, 0, 0];
    header.extend(u24(canvas[0] - 1));
    header.extend(u24(canvas[1] - 1));
    let mut body = b"WEBP".to_vec();
    body.extend(chunk(b"VP8X", &header));
    // A background colour and loop count the decoder is told to ignore.
    body.extend(chunk(b"ANIM", &[0xFF, 0xFF, 0xFF, 0xFF, 0, 0]));
    for &([x, y, width, height], flags, ref rgba) in frames {
        let mut frame = [
            u24(x / 2),
            u24(y / 2),
            u24(width - 1),
            u24(height - 1),
            u24(100),
        ]
        .concat();
        frame.push(flags);
        // The still's VP8L chunk, past its `RIFF` size `WEBP` header.
        frame.extend_from_slice(&still([x, y, width, height], &rgba[..])[12..]);
        body.extend(chunk(b"ANMF", &frame));
    }
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    /// A still WebP reads as one frame with its pixels and alpha as stored,
    /// by its signature, through the same stream a PNG takes.
    #[test]
    fn a_still_webp_streams_one_frame_with_its_alpha() {
        let rgba = [
            [255, 0, 0, 255],
            [0, 0, 0, 0],
            [10, 20, 30, 128],
            [0, 255, 0, 255],
        ]
        .concat();
        let bytes = test_webp([2, 2], &[([0, 0, 2, 2], 0, rgba.clone())]);
        assert_eq!(
            PictureHeader::read(&bytes).unwrap(),
            PictureHeader {
                width: 2,
                height: 2,
                frames: 1
            }
        );
        let mut stream = FrameStream::new(&bytes).unwrap();
        assert_eq!((stream.size(), stream.frame_count()), ([2, 2], 1));
        assert_eq!(stream.next_frame().unwrap().unwrap(), &rgba[..]);
        assert!(stream.next_frame().is_none());
    }

    /// An animated WebP's frames composite onto the whole canvas: disposal
    /// clears to transparent, a frame without blending replaces, one with
    /// blending keeps what its transparent pixels cover.
    #[test]
    fn an_animated_webp_composites_by_blend_and_dispose() {
        const RED: [u8; 4] = [255, 0, 0, 255];
        const GREEN: [u8; 4] = [0, 255, 0, 255];
        const CLEAR: [u8; 4] = [0; 4];
        let bytes = test_webp(
            [4, 1],
            &[
                ([0, 0, 4, 1], 0b11, RED.repeat(4)),
                ([2, 0, 2, 1], 0b10, GREEN.repeat(2)),
                ([2, 0, 2, 1], 0b00, [CLEAR, RED].concat()),
            ],
        );
        assert_eq!(PictureHeader::read(&bytes).unwrap().frames, 3);
        let picture = PictureFrames::decode(&bytes).unwrap();
        assert_eq!((picture.width, picture.height), (4, 1));
        assert_eq!(picture.frames[0], RED.repeat(4));
        assert_eq!(picture.frames[1], [CLEAR, CLEAR, GREEN, GREEN].concat());
        assert_eq!(
            &picture.frames[2][..12],
            &[CLEAR, CLEAR, GREEN].concat()[..]
        );
        // Opaque red blended over green, within the decoder's rounding.
        let last = &picture.frames[2][12..];
        assert!(last[0] >= 254 && last[1..] == [0, 0, 255], "{last:?}");
    }

    /// A lossy frame without alpha after a disposed frame shows its own
    /// pixels only: the disposed frame's region is cleared whole (here a
    /// libwebp file; `image-webp` 0.2.4's compositing leaves part of it).
    #[test]
    fn a_disposed_webp_frame_clears_before_an_opaque_lossy_frame() {
        let bytes = include_bytes!("../../tests/fixtures/pictures/dispose_then_opaque.webp");
        let picture = PictureFrames::decode(bytes).unwrap();
        assert_eq!(
            (picture.width, picture.height, picture.frames.len()),
            (16, 16, 2)
        );
        let shown: Vec<(usize, usize)> = picture.frames[1]
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, pixel)| pixel[3] != 0)
            .map(|(index, _)| (index % 16, index / 16))
            .collect();
        let square: Vec<(usize, usize)> =
            (6..10).flat_map(|y| (6..10).map(move |x| (x, y))).collect();
        assert_eq!(shown, square);
        // Frame 0's band is still read, with its alpha.
        assert!(
            picture.frames[0]
                .chunks_exact(4)
                .filter(|p| p[3] == 255)
                .count()
                >= 120
        );
    }

    /// A `.png` name that is missing finds its `.webp` twin, a mask's too;
    /// a `.png` that is there wins and is the only name asked.
    #[test]
    fn a_missing_png_finds_its_webp_twin() {
        let files = ["a.png", "a.webp", "b.webp", "b.mask.webp"];
        let asked = std::cell::RefCell::new(Vec::new());
        let lookup = |name: &str| {
            asked.borrow_mut().push(name.to_owned());
            files.contains(&name).then_some(name.len())
        };
        let found = |name: &str| find_picture(name, &lookup).map(|(found, _)| found.into_owned());
        assert_eq!(found("a.png").as_deref(), Some("a.png"));
        assert_eq!(*asked.borrow(), ["a.png"]);
        assert_eq!(found("b.png").as_deref(), Some("b.webp"));
        assert_eq!(found("b.mask.png").as_deref(), Some("b.mask.webp"));
        assert_eq!(found("c.webp"), None);
        assert_eq!(found("c.shp"), None);
    }

    #[test]
    fn an_apng_counts_its_animation_frames_and_a_png_one() {
        let apng = PictureHeader::read(&test_picture(8, 6, Some(5))).unwrap();
        assert_eq!(
            apng,
            PictureHeader {
                width: 8,
                height: 6,
                frames: 5
            }
        );
        assert_eq!(
            PictureHeader::read(&test_picture(3, 4, None))
                .unwrap()
                .frames,
            1
        );
        assert!(PictureHeader::read(b"not a png").is_err());
    }

    /// Frames that cover part of the canvas composite as an APNG viewer
    /// shows them: Source replaces, Over blends, Background clears after.
    #[test]
    fn apng_frames_composite_by_blend_and_dispose() {
        const RED: [u8; 4] = [255, 0, 0, 255];
        const GREEN: [u8; 4] = [0, 255, 0, 255];
        const CLEAR: [u8; 4] = [0; 4];
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 4, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_animated(3, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        // Frame 0: the whole row red, cleared to nothing after it shows.
        writer.set_dispose_op(png::DisposeOp::Background).unwrap();
        writer.write_image_data(&RED.repeat(4)).unwrap();
        // Frame 1: green over the right half only.
        writer.set_dispose_op(png::DisposeOp::None).unwrap();
        writer.set_frame_dimension(2, 1).unwrap();
        writer.set_frame_position(2, 0).unwrap();
        writer.write_image_data(&GREEN.repeat(2)).unwrap();
        // Frame 2: half-transparent red blended over pixel 3.
        writer.set_blend_op(png::BlendOp::Over).unwrap();
        writer.set_frame_dimension(1, 1).unwrap();
        writer.set_frame_position(3, 0).unwrap();
        writer.write_image_data(&[255, 0, 0, 128]).unwrap();
        writer.finish().unwrap();

        let picture = PictureFrames::decode(&bytes).unwrap();
        assert_eq!((picture.width, picture.height), (4, 1));
        assert_eq!(picture.frames[0], RED.repeat(4));
        assert_eq!(picture.frames[1], [CLEAR, CLEAR, GREEN, GREEN].concat());
        assert_eq!(
            &picture.frames[2][..12],
            &[CLEAR, CLEAR, GREEN].concat()[..]
        );
        assert_eq!(&picture.frames[2][12..], &[128, 127, 0, 255]);
    }

    /// Every blend and dispose variant, Previous after a frame that stays
    /// and after one that is cleared, composites the same one frame at a
    /// time as all together.
    #[test]
    fn a_stream_composites_as_decode_does() {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 3, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_animated(5, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        let frame = |seed: u8, pixels: usize| -> Vec<u8> {
            (0..pixels * 4)
                .map(|i| {
                    if i % 4 == 3 {
                        [255, 90, 0][i / 4 % 3]
                    } else {
                        seed ^ i as u8
                    }
                })
                .collect()
        };
        writer.set_dispose_op(png::DisposeOp::Previous).unwrap();
        writer.write_image_data(&frame(10, 6)).unwrap();
        writer.set_dispose_op(png::DisposeOp::None).unwrap();
        writer.set_blend_op(png::BlendOp::Over).unwrap();
        writer.write_image_data(&frame(20, 6)).unwrap();
        writer.set_dispose_op(png::DisposeOp::Previous).unwrap();
        writer.set_frame_dimension(2, 1).unwrap();
        writer.set_frame_position(1, 1).unwrap();
        writer.write_image_data(&frame(30, 2)).unwrap();
        writer.set_dispose_op(png::DisposeOp::Background).unwrap();
        writer.set_blend_op(png::BlendOp::Source).unwrap();
        writer.set_frame_position(0, 0).unwrap();
        writer.write_image_data(&frame(40, 2)).unwrap();
        writer.set_dispose_op(png::DisposeOp::None).unwrap();
        writer.set_blend_op(png::BlendOp::Over).unwrap();
        writer.set_frame_dimension(3, 2).unwrap();
        writer.write_image_data(&frame(50, 6)).unwrap();
        writer.finish().unwrap();

        let picture = PictureFrames::decode(&bytes).unwrap();
        let mut stream = FrameStream::new(&bytes).unwrap();
        assert_eq!(stream.size(), [3, 2]);
        assert_eq!(stream.frame_count(), 5);
        let mut streamed = Vec::new();
        while let Some(frame) = stream.next_frame() {
            streamed.push(frame.unwrap().to_vec());
        }
        assert_eq!(streamed, picture.frames);
        // Frame 0's Previous acts as Background, so frame 1 blends over
        // nothing; frame 2's Previous puts frame 1's pixels back.
        assert_eq!(&picture.frames[3][3 * 4..], &picture.frames[1][3 * 4..]);
    }

    /// However many frames a picture has, a stream holds two canvases.
    #[test]
    fn a_long_picture_streams_in_two_canvases() {
        let (width, height) = (16, 8);
        let canvas = (width * height * 4) as usize;
        let bytes = test_picture(width, height, Some(300));
        let header = PictureHeader::read(&bytes).unwrap();
        let mut stream = FrameStream::new(&bytes).unwrap();
        assert_eq!(stream.size(), [header.width, header.height]);
        assert_eq!(stream.frame_count(), header.frames as usize);
        let mut count = 0;
        while let Some(frame) = stream.next_frame() {
            assert_eq!(frame.unwrap().len(), canvas);
            count += 1;
        }
        assert_eq!(count, header.frames as usize);
        assert!(stream.held_bytes() <= 2 * canvas);
    }
}
