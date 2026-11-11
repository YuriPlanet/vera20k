//! Spans of bits in a bitset kept as `u64` words, bit `i` at
//! `words[i / 64] >> (i % 64) & 1`.

/// The words that hold bits `first..=last`, in order, each with the mask of
/// those bits in it.
pub(crate) fn span_words(first: usize, last: usize) -> impl Iterator<Item = (usize, u64)> {
    debug_assert!(first <= last, "an empty span");
    (first / 64..=last / 64).map(move |word| {
        let low = if word == first / 64 { first % 64 } else { 0 };
        let high = if word == last / 64 { last % 64 } else { 63 };
        (word, (u64::MAX << low) & (u64::MAX >> (63 - high)))
    })
}

#[cfg(test)]
mod tests {
    use super::span_words;

    #[test]
    fn a_span_masks_its_bits_in_each_word_it_crosses() {
        assert_eq!(span_words(5, 5).collect::<Vec<_>>(), [(0, 1 << 5)]);
        assert_eq!(
            span_words(62, 128).collect::<Vec<_>>(),
            [(0, 0b11 << 62), (1, u64::MAX), (2, 1)]
        );
    }
}
