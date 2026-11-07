//! Pictures resized on the CPU by area: each output pixel is the mean of the
//! part of the source it covers, in linear light with premultiplied alpha, so
//! a shrink neither aliases nor darkens its edges. For pictures drawn pixel
//! for pixel at a size the GPU's linear filter, which reads only the four
//! nearest texels, would shrink them to badly. Presentation only.

/// `rgba`, `width` x `height` pixels with straight alpha, at `out_width` x
/// `out_height`.
pub fn scale(
    rgba: &[u8],
    width: usize,
    height: usize,
    out_width: usize,
    out_height: usize,
) -> Vec<u8> {
    assert_eq!(rgba.len(), width * height * 4);
    if width == 0 || height == 0 {
        return vec![0; out_width * out_height * 4];
    }
    let decode: [f32; 256] = std::array::from_fn(|value| srgb_to_linear(value as f32 / 255.0));
    let source: Vec<[f32; 4]> = rgba
        .chunks_exact(4)
        .map(|pixel| {
            let alpha = f32::from(pixel[3]) / 255.0;
            let [r, g, b] = [0, 1, 2].map(|channel| decode[usize::from(pixel[channel])] * alpha);
            [r, g, b, alpha]
        })
        .collect();
    let columns = shares(width, out_width);
    let rows = shares(height, out_height);
    let mut across = Vec::with_capacity(out_width * height);
    for line in source.chunks_exact(width) {
        across.extend(
            columns
                .iter()
                .map(|covered| mean(covered.iter().map(|&(x, share)| (line[x], share)))),
        );
    }
    let mut out = Vec::with_capacity(out_width * out_height * 4);
    for covered in &rows {
        for x in 0..out_width {
            let [r, g, b, alpha] = mean(
                covered
                    .iter()
                    .map(|&(y, share)| (across[y * out_width + x], share)),
            );
            if alpha <= 0.0 {
                out.extend_from_slice(&[0; 4]);
                continue;
            }
            out.extend(
                [r, g, b].map(|channel| (linear_to_srgb(channel / alpha) * 255.0).round() as u8),
            );
            out.push((alpha * 255.0).round() as u8);
        }
    }
    out
}

/// For each of `to` pixels across a line of `from`, the source pixels it
/// covers and the share of it each covers.
fn shares(from: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
    let step = from as f32 / to as f32;
    (0..to)
        .map(|pixel| {
            let (start, end) = (pixel as f32 * step, (pixel + 1) as f32 * step);
            (start as usize..(end.ceil() as usize).min(from))
                .map(|source| {
                    let covered = end.min(source as f32 + 1.0) - start.max(source as f32);
                    (source, covered / step)
                })
                .filter(|&(_, share)| share > 0.0)
                .collect()
        })
        .collect()
}

fn mean(samples: impl Iterator<Item = ([f32; 4], f32)>) -> [f32; 4] {
    samples.fold([0.0; 4], |sum, (pixel, share)| {
        std::array::from_fn(|channel| sum[channel] + pixel[channel] * share)
    })
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shrink_averages_light_and_keeps_an_edge_colour() {
        // White beside black is half the light, sRGB 188, not 128.
        assert_eq!(
            scale(&[255, 255, 255, 255, 0, 0, 0, 255], 2, 1, 1, 1),
            [188, 188, 188, 255]
        );
        // Red beside a clear pixel is red, partly clear: the clear pixel's
        // black never darkens it.
        let red_then_clear = [255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255];
        assert_eq!(
            scale(&red_then_clear, 3, 1, 2, 1),
            [255, 0, 0, 170, 255, 0, 0, 170]
        );
    }
}
