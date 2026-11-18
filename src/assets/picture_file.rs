//! VERA-only, experimental picture art (`Picture=`,
//! `docs/picture-model-art.md`): the header facts of a PNG or APNG that the
//! simulation takes where it takes an SHP header's, read without decoding a
//! pixel.

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
    #[error("{0} bytes of frames is more than the {PICTURE_BYTES_MAX} a picture may hold")]
    TooLarge(u64),
    #[error("a frame region outside the canvas")]
    FrameOutside,
}

/// The most decoded RGBA bytes one picture may hold, all frames together.
pub const PICTURE_BYTES_MAX: u64 = 512 << 20;

/// A picture's frames composited onto its canvas, as an APNG viewer shows
/// them: straight RGBA, row by row, `width * height * 4` bytes each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureFrames {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
}

impl PictureFrames {
    /// Decode and composite every frame of the PNG or APNG `bytes`. An APNG's
    /// default image that is not part of its animation is skipped; each
    /// animation frame is drawn onto the canvas by its blend operation and
    /// then disposed of by its dispose operation (APNG 1.0, `fcTL`).
    pub fn decode(bytes: &[u8]) -> Result<Self, PictureError> {
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
        let canvas_bytes = u64::from(width) * u64::from(height) * 4;
        let total = canvas_bytes * u64::from(frame_count);
        if total > PICTURE_BYTES_MAX {
            return Err(PictureError::TooLarge(total));
        }
        let default_in_animation = animation.is_none() || info.frame_control.is_some();
        let mut buffer = vec![
            0;
            reader
                .output_buffer_size()
                .ok_or(PictureError::TooLarge(total))?
        ];
        if !default_in_animation {
            reader.next_frame(&mut buffer)?;
        }
        let canvas_len = canvas_bytes as usize;
        let mut canvas = vec![0u8; canvas_len];
        let mut frames = Vec::with_capacity(frame_count as usize);
        for index in 0..frame_count {
            let output = reader.next_frame(&mut buffer)?;
            let control = reader.info().frame_control;
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
            let rgba = to_rgba(&buffer[..output.buffer_size()], output.color_type, w, h);
            let dispose = control.map_or(png::DisposeOp::None, |control| control.dispose_op);
            // A first frame's Previous disposal acts as Background (APNG 1.0).
            let dispose = match (index, dispose) {
                (0, png::DisposeOp::Previous) => png::DisposeOp::Background,
                (_, dispose) => dispose,
            };
            let saved = (dispose == png::DisposeOp::Previous).then(|| canvas.clone());
            let over = control.is_some_and(|control| control.blend_op == png::BlendOp::Over);
            for row in 0..h {
                for column in 0..w {
                    let from = ((row * w + column) * 4) as usize;
                    let to = (((y + row) * width + x + column) * 4) as usize;
                    let source: [u8; 4] = rgba[from..from + 4].try_into().unwrap();
                    let target = &mut canvas[to..to + 4];
                    if over {
                        let blended = blend_over(source, target.try_into().unwrap());
                        target.copy_from_slice(&blended);
                    } else {
                        target.copy_from_slice(&source);
                    }
                }
            }
            frames.push(canvas.clone());
            match dispose {
                png::DisposeOp::None => {}
                png::DisposeOp::Background => {
                    for row in y..y + h {
                        let start = ((row * width + x) * 4) as usize;
                        canvas[start..start + (w * 4) as usize].fill(0);
                    }
                }
                png::DisposeOp::Previous => canvas = saved.unwrap(),
            }
        }
        Ok(Self {
            width,
            height,
            frames,
        })
    }
}

/// `w` x `h` decoded 8-bit samples of `color` as straight RGBA.
fn to_rgba(samples: &[u8], color: png::ColorType, w: u32, h: u32) -> Vec<u8> {
    let pixels = (w * h) as usize;
    let mut rgba = Vec::with_capacity(pixels * 4);
    let pixel = |index: usize| -> [u8; 4] {
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
    };
    for index in 0..pixels {
        rgba.extend_from_slice(&pixel(index));
    }
    rgba
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
}
