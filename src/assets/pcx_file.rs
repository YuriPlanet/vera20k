//! Minimal PCX parser for RA2 shell owner-draw art.
//!
//! Supports the retail 8-bit, one-plane, RLE-compressed PCX files used by
//! shell controls, plus the runtime-written 3-plane direct RGB preview form.
//! The parser keeps embedded VGA palettes in 8-bit RGB.

use crate::assets::error::AssetError;

#[derive(Debug, Clone)]
pub struct PcxFile {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
    pub palette: [[u8; 3]; 256],
    direct_rgb: bool,
}

impl PcxFile {
    pub fn from_bytes(data: &[u8]) -> Result<Self, AssetError> {
        if data.len() < 128 {
            return Err(pcx_error("PCX too short"));
        }
        if data[0] != 0x0A || data[2] != 1 || data[3] != 8 {
            return Err(pcx_error("Unsupported PCX header"));
        }
        let x_min = u16::from_le_bytes([data[4], data[5]]);
        let y_min = u16::from_le_bytes([data[6], data[7]]);
        let x_max = u16::from_le_bytes([data[8], data[9]]);
        let y_max = u16::from_le_bytes([data[10], data[11]]);
        let planes = data[65];
        if planes != 1 && planes != 3 {
            return Err(pcx_error("Only 1-plane or 3-plane PCX is supported"));
        }
        let bytes_per_line = u16::from_le_bytes([data[66], data[67]]) as usize;
        let width = x_max
            .checked_sub(x_min)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| pcx_error("Invalid PCX width"))?;
        let height = y_max
            .checked_sub(y_min)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| pcx_error("Invalid PCX height"))?;
        if bytes_per_line < width as usize {
            return Err(pcx_error("Invalid PCX bytes per line"));
        }

        let expected_scan = bytes_per_line
            .checked_mul(planes as usize)
            .and_then(|v| v.checked_mul(height as usize))
            .ok_or_else(|| pcx_error("PCX scan data too large"))?;
        let mut palette = [[0u8; 3]; 256];
        let has_trailing_palette = data.len() >= 128 + 769 && data[data.len() - 769] == 0x0C;
        let encoded = if planes == 1 {
            if !has_trailing_palette {
                return Err(pcx_error("Missing PCX VGA palette"));
            }
            let pal = &data[data.len() - 768..];
            for (idx, rgb) in palette.iter_mut().enumerate() {
                rgb.copy_from_slice(&pal[idx * 3..idx * 3 + 3]);
            }
            &data[128..data.len() - 769]
        } else if has_trailing_palette {
            &data[128..data.len() - 769]
        } else {
            &data[128..]
        };
        let scan = decode_pcx_rle(encoded, expected_scan)?;
        let pixels = if planes == 1 {
            trim_paletted_scanlines(&scan, width, height, bytes_per_line)
        } else {
            decode_direct_rgb_scanlines(&scan, width, height, bytes_per_line)
        };

        Ok(Self {
            width,
            height,
            pixels,
            palette,
            direct_rgb: planes == 3,
        })
    }

    pub fn to_rgba(&self, transparent_index: Option<u8>) -> Vec<u8> {
        if self.is_direct_rgb() {
            let mut rgba = Vec::with_capacity(self.pixels.len() / 3 * 4);
            for rgb in self.pixels.chunks_exact(3) {
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            return rgba;
        }

        let mut rgba = Vec::with_capacity(self.pixels.len() * 4);
        for &idx in &self.pixels {
            let [r, g, b] = self.palette[idx as usize];
            let a = if transparent_index == Some(idx) {
                0
            } else {
                255
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
        rgba
    }

    #[cfg(test)]
    pub fn to_rgba_with_color_key(&self, transparent_rgb: [u8; 3]) -> Vec<u8> {
        if self.is_direct_rgb() {
            let mut rgba = Vec::with_capacity(self.pixels.len() / 3 * 4);
            for rgb in self.pixels.chunks_exact(3) {
                let a = if rgb == transparent_rgb { 0 } else { 255 };
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
            }
            return rgba;
        }

        let mut rgba = Vec::with_capacity(self.pixels.len() * 4);
        for &idx in &self.pixels {
            let [r, g, b] = self.palette[idx as usize];
            let a = if [r, g, b] == transparent_rgb { 0 } else { 255 };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
        rgba
    }

    fn is_direct_rgb(&self) -> bool {
        self.direct_rgb
    }
}

/// PCX header size; pixel data starts immediately after it.
const PCX_HEADER_LEN: usize = 128;
/// Active retail's writer flushes at 47 (`0x2F`), even though the PCX marker
/// could represent a run of 63.
const PCX_MAX_RUN: usize = 0x2F;
/// Top two bits set marks a run-length byte.
const PCX_RUN_MARKER: u8 = 0xC0;

/// Encode an RGB image as a 3-plane, 8-bit, RLE PCX.
///
/// This is the form the runtime preview is written in: each scanline stores a
/// full red row, then green, then blue, rather than interleaving components.
/// `rgb` is row-major, three bytes per pixel.
pub fn encode_direct_rgb(width: u16, height: u16, rgb: &[u8]) -> Result<Vec<u8>, AssetError> {
    let pixel_count = width as usize * height as usize;
    if rgb.len() != pixel_count * 3 {
        return Err(pcx_error("RGB buffer does not match the given dimensions"));
    }
    if width == 0 || height == 0 {
        return Err(pcx_error("PCX dimensions must be non-zero"));
    }

    let mut out = vec![0u8; PCX_HEADER_LEN];
    out[0] = 0x0A; // manufacturer
    out[1] = 5; // version
    out[2] = 1; // RLE encoding
    out[3] = 8; // bits per pixel per plane
    // The image rectangle is inclusive, so the maxima are one less than the size.
    out[8..10].copy_from_slice(&(width - 1).to_le_bytes());
    out[10..12].copy_from_slice(&(height - 1).to_le_bytes());
    out[12..14].copy_from_slice(&width.to_le_bytes()); // horizontal DPI
    out[14..16].copy_from_slice(&height.to_le_bytes()); // vertical DPI
    out[65] = 3; // planes
    out[66..68].copy_from_slice(&width.to_le_bytes()); // bytes per line, per plane
    out[68..70].copy_from_slice(&1u16.to_le_bytes()); // colour palette type
    out[70..72].copy_from_slice(&width.to_le_bytes()); // horizontal screen size
    out[72..74].copy_from_slice(&height.to_le_bytes()); // vertical screen size

    let width_usize = width as usize;
    let mut plane = vec![0u8; width_usize];
    for row in 0..height as usize {
        let row_start = row * width_usize * 3;
        for component in 0..3 {
            for column in 0..width_usize {
                plane[column] = rgb[row_start + column * 3 + component];
            }
            encode_rle_row(&plane, &mut out);
        }
    }
    Ok(out)
}

/// Append one RLE-encoded plane row.
///
/// A literal byte only escapes the run encoding when its top two bits are clear,
/// so any other single byte still has to be written as a one-long run.
fn encode_rle_row(row: &[u8], out: &mut Vec<u8>) {
    let mut index = 0usize;
    while index < row.len() {
        let value = row[index];
        let mut run = 1usize;
        while index + run < row.len() && row[index + run] == value && run < PCX_MAX_RUN {
            run += 1;
        }
        if run > 1 || value & PCX_RUN_MARKER == PCX_RUN_MARKER {
            out.push(PCX_RUN_MARKER | run as u8);
        }
        out.push(value);
        index += run;
    }
}

fn decode_pcx_rle(encoded: &[u8], expected_scan: usize) -> Result<Vec<u8>, AssetError> {
    let mut scan = Vec::with_capacity(expected_scan);
    let mut i = 0usize;
    while i < encoded.len() && scan.len() < expected_scan {
        let byte = encoded[i];
        i += 1;
        if byte & 0xC0 == 0xC0 {
            if i >= encoded.len() {
                return Err(pcx_error("Truncated PCX RLE run"));
            }
            let count = (byte & 0x3F) as usize;
            let value = encoded[i];
            i += 1;
            scan.extend(std::iter::repeat(value).take(count));
        } else {
            scan.push(byte);
        }
    }
    if scan.len() < expected_scan {
        return Err(pcx_error("PCX RLE stream ended early"));
    }
    scan.truncate(expected_scan);
    Ok(scan)
}

fn trim_paletted_scanlines(scan: &[u8], width: u16, height: u16, bytes_per_line: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize);
    for row in 0..height as usize {
        let start = row * bytes_per_line;
        pixels.extend_from_slice(&scan[start..start + width as usize]);
    }
    pixels
}

fn decode_direct_rgb_scanlines(
    scan: &[u8],
    width: u16,
    height: u16,
    bytes_per_line: usize,
) -> Vec<u8> {
    let width = width as usize;
    let height = height as usize;
    let row_stride = bytes_per_line * 3;
    let mut pixels = Vec::with_capacity(width * height * 3);
    for row in 0..height {
        let row_start = row * row_stride;
        let red_start = row_start;
        let green_start = red_start + bytes_per_line;
        let blue_start = green_start + bytes_per_line;
        for col in 0..width {
            pixels.extend_from_slice(&[
                scan[red_start + col],
                scan[green_start + col],
                scan[blue_start + col],
            ]);
        }
    }
    pixels
}

fn pcx_error(detail: &str) -> AssetError {
    AssetError::ParseError {
        format: "PCX".to_string(),
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::PcxFile;

    fn pcx_header(
        x_min: u16,
        y_min: u16,
        x_max: u16,
        y_max: u16,
        planes: u8,
        bytes_per_line: u16,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 128];
        data[0] = 0x0A;
        data[2] = 1;
        data[3] = 8;
        data[4..6].copy_from_slice(&x_min.to_le_bytes());
        data[6..8].copy_from_slice(&y_min.to_le_bytes());
        data[8..10].copy_from_slice(&x_max.to_le_bytes());
        data[10..12].copy_from_slice(&y_max.to_le_bytes());
        data[65] = planes;
        data[66..68].copy_from_slice(&bytes_per_line.to_le_bytes());
        data
    }

    #[test]
    fn parses_8bit_rle_pcx_with_embedded_palette() {
        let mut data = pcx_header(0, 0, 1, 1, 1, 2);
        data.extend_from_slice(&[0xC4, 1]);
        data.push(0x0C);
        let mut pal = vec![0u8; 768];
        pal[3] = 10;
        pal[4] = 20;
        pal[5] = 30;
        data.extend_from_slice(&pal);

        let pcx = PcxFile::from_bytes(&data).expect("pcx");
        assert_eq!((pcx.width, pcx.height), (2, 2));
        assert_eq!(pcx.pixels, vec![1, 1, 1, 1]);
        assert_eq!(pcx.palette[1], [10, 20, 30]);
        assert_eq!(pcx.to_rgba(None)[0..4], [10, 20, 30, 255]);
    }

    #[test]
    fn parses_3plane_direct_rgb_pcx_with_bounds_dimensions() {
        let mut data = pcx_header(10, 20, 11, 21, 3, 4);
        data.extend_from_slice(&[
            1, 2, 99, 99, // row 0 red plane plus padding
            10, 20, 99, 99, // row 0 green plane plus padding
            30, 40, 99, 99, // row 0 blue plane plus padding
            3, 4, 99, 99, // row 1 red plane plus padding
            50, 60, 99, 99, // row 1 green plane plus padding
            70, 80, 99, 99, // row 1 blue plane plus padding
        ]);

        let pcx = PcxFile::from_bytes(&data).expect("pcx");

        assert_eq!((pcx.width, pcx.height), (2, 2));
        assert_eq!(
            pcx.pixels,
            vec![
                1, 10, 30, //
                2, 20, 40, //
                3, 50, 70, //
                4, 60, 80,
            ]
        );
        assert_eq!(
            pcx.to_rgba(None),
            vec![
                1, 10, 30, 255, //
                2, 20, 40, 255, //
                3, 50, 70, 255, //
                4, 60, 80, 255,
            ]
        );
    }

    #[test]
    fn direct_rgb_color_key_applies_after_rgb_conversion() {
        let mut data = pcx_header(0, 0, 1, 0, 3, 2);
        data.extend_from_slice(&[
            0xC1, 255, 1, // red
            0, 2, // green
            0xC1, 255, 3, // blue
        ]);

        let pcx = PcxFile::from_bytes(&data).expect("pcx");

        assert_eq!(
            pcx.to_rgba_with_color_key([255, 0, 255]),
            vec![
                255, 0, 255, 0, //
                1, 2, 3, 255,
            ]
        );
    }

    #[test]
    fn rgba_color_key_uses_rgb_not_palette_index() {
        let pcx = PcxFile {
            width: 3,
            height: 1,
            pixels: vec![1, 0, 3],
            palette: {
                let mut palette = [[0u8; 3]; 256];
                palette[0] = [0, 0, 0];
                palette[1] = [255, 0, 255];
                palette[3] = [255, 0, 255];
                palette
            },
            direct_rgb: false,
        };

        assert_eq!(
            pcx.to_rgba_with_color_key([255, 0, 255]),
            vec![
                255, 0, 255, 0, //
                0, 0, 0, 255, //
                255, 0, 255, 0,
            ]
        );
    }
}

#[cfg(test)]
mod direct_rgb_roundtrip_tests {
    use super::*;

    fn gradient(width: u16, height: u16) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height as usize {
            for x in 0..width as usize {
                rgb.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]);
            }
        }
        rgb
    }

    #[test]
    fn encoded_rgb_decodes_back_to_the_same_pixels() {
        let (w, h) = (37u16, 19u16);
        let rgb = gradient(w, h);
        let encoded = encode_direct_rgb(w, h, &rgb).expect("encode");
        assert_eq!(&encoded[70..72], &w.to_le_bytes());
        assert_eq!(&encoded[72..74], &h.to_le_bytes());
        let decoded = PcxFile::from_bytes(&encoded).expect("decode");
        assert_eq!((decoded.width, decoded.height), (w, h));
        assert_eq!(decoded.pixels, rgb, "round trip is lossless");
    }

    #[test]
    fn a_flat_image_round_trips_through_long_runs() {
        // Active retail flushes runs at 47, so a wider row exercises splitting.
        let (w, h) = (200u16, 3u16);
        let rgb = vec![0x80; w as usize * h as usize * 3];
        let encoded = encode_direct_rgb(w, h, &rgb).expect("encode");
        assert!(
            encoded.len() < rgb.len(),
            "a flat image should compress, got {} bytes for {}",
            encoded.len(),
            rgb.len()
        );
        assert_eq!(PcxFile::from_bytes(&encoded).expect("decode").pixels, rgb);
    }

    #[test]
    fn literals_with_the_run_bits_set_survive() {
        // 0xC5 would be read back as a run marker if written bare.
        let rgb: Vec<u8> = vec![0xC5, 0x01, 0xC0, 0x02, 0xFF, 0x03];
        let encoded = encode_direct_rgb(2, 1, &rgb).expect("encode");
        assert_eq!(PcxFile::from_bytes(&encoded).expect("decode").pixels, rgb);
    }

    #[test]
    fn a_mismatched_buffer_is_rejected() {
        assert!(encode_direct_rgb(4, 4, &[0; 10]).is_err());
        assert!(encode_direct_rgb(0, 4, &[]).is_err());
    }
}
