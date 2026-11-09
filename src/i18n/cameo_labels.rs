//! A cameo's name in the interface language.
//!
//! Westwood drew each cameo's name into its picture, so translating one takes
//! new pixels. A language's `vera20k.<tag>.cameos` holds them: per cameo, the
//! rows from the top of its name band down, XORed with those rows of the
//! retail cameo, and a CRC-32 of the retail rows. Over the player's own retail
//! cameo they give the translated name; without it they are noise, so the
//! file carries no game art. A cameo that is not that retail picture (a mod's,
//! or a release with its own name drawn in) fails the CRC and keeps its name.
//!
//! The file, little-endian: `VCL1`, then per cameo
//!
//! ```text
//! u8   length of the file name, then its lower-case name (gtnkicon.shp)
//! u8   first row of the band
//! u32  CRC-32 of the retail band's rows
//! ..   the band's rows XOR the retail rows: 60 bytes a row, down to row 47
//! ```
//!
//! `tools/language_assets.py` writes it.

use std::collections::HashMap;
use std::sync::Arc;

use crate::assets::mix_hash::crc32;

const MAGIC: &[u8; 4] = b"VCL1";

/// Every retail cameo's canvas.
pub(crate) const CAMEO_WIDTH: usize = 60;
pub(crate) const CAMEO_HEIGHT: usize = 48;

/// One cameo's name in a language.
#[derive(Debug)]
pub struct CameoLabel {
    first_row: usize,
    retail_crc: u32,
    xor: Vec<u8>,
}

impl CameoLabel {
    /// The first row of the band the name covers.
    pub(crate) fn first_row(&self) -> usize {
        self.first_row
    }

    /// Draw the name on `canvas`, a cameo's palette indices, when it is the
    /// retail picture the label was made from. True when it was.
    pub(crate) fn apply(&self, canvas: &mut [u8]) -> bool {
        if canvas.len() != CAMEO_WIDTH * CAMEO_HEIGHT {
            return false;
        }
        let band = &mut canvas[self.first_row * CAMEO_WIDTH..];
        if crc32(band) != self.retail_crc {
            return false;
        }
        for (pixel, mask) in band.iter_mut().zip(&self.xor) {
            *pixel ^= mask;
        }
        true
    }
}

/// The labels of a `vera20k.<tag>.cameos` file, by lower-case file name; `None`
/// for a damaged file.
pub(crate) fn parse(file: &[u8]) -> Option<HashMap<String, Arc<CameoLabel>>> {
    let mut rest = file.strip_prefix(MAGIC)?;
    let mut labels = HashMap::new();
    while let Some((&name_length, tail)) = rest.split_first() {
        let (name, tail) = tail.split_at_checked(usize::from(name_length))?;
        let (&first_row, tail) = tail.split_first()?;
        let first_row = usize::from(first_row);
        let rows = CAMEO_HEIGHT.checked_sub(first_row)?;
        let (retail_crc, tail) = tail.split_first_chunk::<4>()?;
        let (xor, tail) = tail.split_at_checked(rows * CAMEO_WIDTH)?;
        labels.insert(
            std::str::from_utf8(name).ok()?.to_ascii_lowercase(),
            Arc::new(CameoLabel {
                first_row,
                retail_crc: u32::from_le_bytes(*retail_crc),
                xor: xor.to_vec(),
            }),
        );
        rest = tail;
    }
    Some(labels)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A label file for `name` turning `retail` (a whole canvas) into
    /// `translated` from `first_row` down.
    pub(crate) fn label_file(
        name: &str,
        first_row: usize,
        retail: &[u8],
        translated: &[u8],
    ) -> Vec<u8> {
        let from = first_row * CAMEO_WIDTH;
        let mut file = MAGIC.to_vec();
        file.push(name.len() as u8);
        file.extend_from_slice(name.as_bytes());
        file.push(first_row as u8);
        file.extend_from_slice(&crc32(&retail[from..]).to_le_bytes());
        file.extend(
            retail[from..]
                .iter()
                .zip(&translated[from..])
                .map(|(a, b)| a ^ b),
        );
        file
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::label_file;
    use super::*;

    const CANVAS: usize = CAMEO_WIDTH * CAMEO_HEIGHT;

    /// A label turns the retail cameo's band into the translated one and
    /// leaves the picture above it; another cameo keeps its own name.
    #[test]
    fn a_label_redraws_only_the_band_of_its_retail_cameo() {
        let retail = vec![7u8; CANVAS];
        let mut translated = retail.clone();
        translated[46 * CAMEO_WIDTH..].fill(9);
        let labels = parse(&label_file("GTNK.shp", 46, &retail, &translated)).unwrap();
        let label = &labels["gtnk.shp"];

        let mut canvas = retail.clone();
        assert!(label.apply(&mut canvas));
        assert_eq!(canvas, translated);
        assert_eq!(label.first_row(), 46);

        let mut other = vec![8u8; CANVAS];
        assert!(!label.apply(&mut other));
        assert_eq!(other, vec![8u8; CANVAS]);
    }

    #[test]
    fn a_damaged_file_gives_no_labels() {
        let retail = vec![7u8; CANVAS];
        let file = label_file("x.shp", 40, &retail, &retail);
        assert!(parse(&file).is_some());
        assert!(parse(&file[..file.len() - 1]).is_none());
        assert!(parse(b"nope").is_none());
    }
}
