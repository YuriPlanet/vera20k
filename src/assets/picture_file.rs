//! VERA-only, experimental picture art (`Picture=`,
//! `docs/picture-model-art.md`): the header facts of a PNG or APNG that the
//! simulation takes where it takes an SHP header's, read without decoding a
//! pixel, and its frames composited one at a time ([`FrameStream`]) or all
//! together ([`PictureFrames`]).

/// A picture's canvas, in its own pixels, and its frame count: the APNG's
/// animation frames (`acTL`), or 1 for a plain PNG.
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
    #[error("an APNG with no frames")]
    NoFrames,
    #[error("{0} decoded bytes exceeds the picture storage limit of {PICTURE_BYTES_MAX}")]
    TooLarge(u64),
    #[error("a canvas too large to decode on this platform")]
    CanvasTooLarge,
    #[error("a frame region outside the canvas")]
    FrameOutside,
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

/// The frames of a PNG or APNG composited one at a time, in order, onto one
/// reused canvas, as an APNG viewer shows them. An APNG's default image that
/// is not part of its animation is skipped; each animation frame is drawn
/// onto the canvas by its blend operation and disposed of by its dispose
/// operation (APNG 1.0, `fcTL`) just before the next frame is drawn.
///
/// It holds the canvas, the decoder's output buffer for one frame (at most a
/// canvas), and, only while a `Previous`-disposed frame shows, the region
/// that frame covers: so memory stays at about two canvases whatever the
/// frame count. Frames come strictly in order: compositing depends on every
/// frame before, so reaching frame `n` means drawing frames `0..n`.
pub struct FrameStream<'a> {
    reader: png::Reader<std::io::Cursor<&'a [u8]>>,
    width: u32,
    height: u32,
    frame_count: usize,
    /// The frame `next_frame` draws next.
    next: usize,
    /// The decoder's output for one frame, its own region's samples.
    buffer: Vec<u8>,
    canvas: Vec<u8>,
    /// What the frame on the canvas leaves when it is disposed of.
    pending: Dispose,
    /// The canvas region under a `Previous`-disposed frame, before it drew.
    saved: Vec<u8>,
}

/// The disposal still owed by the frame on the canvas: its region and op.
#[derive(Debug, Clone, Copy)]
struct Dispose {
    region: [u32; 4],
    op: png::DisposeOp,
}

impl<'a> FrameStream<'a> {
    /// Start reading the PNG or APNG `bytes`: its header, and past an APNG
    /// default image that is not an animation frame.
    pub fn new(bytes: &'a [u8]) -> Result<Self, PictureError> {
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
            reader,
            width,
            height,
            frame_count: usize::try_from(frame_count).map_err(|_| PictureError::CanvasTooLarge)?,
            next: 0,
            buffer,
            canvas: vec![0; canvas_len],
            pending: Dispose {
                region: [0; 4],
                op: png::DisposeOp::None,
            },
            saved: Vec::new(),
        })
    }

    /// The canvas, `[width, height]` in picture pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    /// The animation frames (`acTL`), or 1 for a plain PNG.
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

    /// Dispose of the frame on the canvas, then decode the next one and draw
    /// it by its blend operation.
    fn draw_next(&mut self) -> Result<(), PictureError> {
        self.dispose();
        let output = self.reader.next_frame(&mut self.buffer)?;
        let control = self.reader.info().frame_control;
        let (width, height) = (self.width, self.height);
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
        let op = match (self.next, op) {
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
                    .extend_from_slice(&self.canvas[start..start + row_bytes]);
            }
        }
        let over = control.is_some_and(|control| control.blend_op == png::BlendOp::Over);
        let samples = &self.buffer[..output.buffer_size()];
        let color = output.color_type;
        for row in 0..h {
            let to = (((y + row) * width + x) * 4) as usize;
            let target = &mut self.canvas[to..to + row_bytes];
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

    /// Apply the disposal the frame on the canvas still owes.
    fn dispose(&mut self) {
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
            let start = (((y + row) * self.width + x) * 4) as usize;
            let target = &mut self.canvas[start..start + row_bytes];
            if op == png::DisposeOp::Previous {
                let from = row as usize * row_bytes;
                target.copy_from_slice(&self.saved[from..from + row_bytes]);
            } else {
                target.fill(0);
            }
        }
    }

    /// The bytes of canvases and buffers this stream holds, for tests of its
    /// memory bound.
    #[cfg(all(test, tests_assets))]
    fn held_bytes(&self) -> usize {
        self.buffer.capacity() + self.canvas.capacity() + self.saved.capacity()
    }
}

impl PictureFrames {
    /// Decode and composite every frame of the PNG or APNG `bytes`
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

/// Straight-alpha `source` over `target` (APNG `APNG_BLEND_OP_OVER`).
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

impl PictureHeader {
    /// The header of the PNG or APNG `bytes`.
    pub fn read(bytes: &[u8]) -> Result<Self, PictureError> {
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

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

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
