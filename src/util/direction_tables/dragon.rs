//! The 32-step facing frame table `0x007F4890` = `i32[32]` where
//! `table[i] = (28 - i) & 31` (study Verification Log #4, read_memory 0x007F4890
//! len128). `BulletClass::GetAnimFrame @ 0x00468000` indexes it at `0x0046805D`
//! with the [`step32_from_facing16`](super::step32_from_facing16) bucket of the
//! velocity angle when the type's inverse ART `Rotates` byte (Type+0x2A1) is
//! clear, i.e. for `Rotates=yes` bullets such as DRAGON and AAHeatSeeker2.
//! Ghidra also lists data references from `0x0043EE8E`, `0x004466BE`,
//! `0x0044FE10` and `0x0045127F`; none of those readers is ported here.

/// gamemd DRAGON 32-way frame map `0x007F4890`: `table[i] = (28 - i) & 31`,
/// i.e. `[28,27,…,1,0,31,30,29]`. `&31` on i32 wraps the negative tail correctly
/// (`-1 & 31 == 31`).
pub const DRAGON_FRAME_TABLE: [i32; 32] = {
    let mut out = [0i32; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (28 - i as i32) & 31;
        i += 1;
    }
    out
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragon_frame_table_equals_gamemd_dump() {
        // read_memory 0x007F4890 len128 (study Verification Log #4).
        let expected: [i32; 32] = [
            28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6,
            5, 4, 3, 2, 1, 0, 31, 30, 29,
        ];
        assert_eq!(DRAGON_FRAME_TABLE, expected);
        for (i, &frame) in DRAGON_FRAME_TABLE.iter().enumerate() {
            assert_eq!(frame, (28 - i as i32) & 31);
        }
    }
}
