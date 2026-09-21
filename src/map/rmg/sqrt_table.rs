//! Exhaustive check of the generator's square-root table against retail.
//!
//! The original's square root is not `sqrt` — it is a 16384-entry lookup keyed
//! on the top 14 bits of the significand, so its results are quantised to about
//! `2^-14` relative accuracy. [`crate::map::rmg::x87::approx_sqrt`] reproduces
//! it, and it *computes* the entries rather than shipping them: the table is
//! plain arithmetic, and the retail bytes are the user's own file, not ours to
//! redistribute.
//!
//! Computing them is only safe if it produces the same 16384 words. That claim
//! used to live in a doc comment with eleven pinned spot values behind it. This
//! module turns it into a check that can actually fail: read the table out of
//! the player's `gamemd.exe` and compare every entry.
//!
//! The domain is finite and small, so this is exhaustive rather than sampled —
//! one of the few places in the generator where a bit-exactness claim can be
//! settled outright instead of waiting on the oracle. It needs `RA2_DIR`, so it
//! is `#[ignore]`d and skips loudly when the install is absent.

/// Where the table sits in the loaded image.
const TABLE_VA: u32 = 0x0086_50BC;
/// Entries, one `u32` each — the top 14 bits of a significand.
const TABLE_LEN: usize = 16384;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::retail_trig::file_offset_of;
    use crate::map::rmg::x87::sqrt_table_entry;

    fn retail_executable() -> Option<Vec<u8>> {
        let dir = std::env::var("RA2_DIR").ok()?;
        std::fs::read(std::path::Path::new(&dir).join("gamemd.exe")).ok()
    }

    #[test]
    #[ignore = "needs RA2_DIR pointing at a retail install"]
    fn the_computed_sqrt_table_matches_retail_entry_for_entry() {
        let Some(image) = retail_executable() else {
            panic!("RA2_DIR is unset or has no gamemd.exe — this check cannot run");
        };
        let base = file_offset_of(&image, TABLE_VA).expect("sqrt table is inside a mapped section");
        let bytes = image
            .get(base..base + TABLE_LEN * 4)
            .expect("sqrt table runs past the end of the image");

        let mut mismatches = Vec::new();
        for index in 0..TABLE_LEN {
            let off = index * 4;
            let retail = u32::from_le_bytes(bytes[off..off + 4].try_into().expect("four bytes"));
            let computed = sqrt_table_entry(index as u32);
            if computed != retail {
                mismatches.push((index, retail, computed));
            }
        }

        let report: Vec<String> = mismatches
            .iter()
            .take(5)
            .map(|(index, retail, computed)| {
                format!("entry {index}: retail {retail:#010x}, computed {computed:#010x}")
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "{} of {TABLE_LEN} entries differ from retail; first: {}",
            mismatches.len(),
            report.join("; ")
        );
    }
}
