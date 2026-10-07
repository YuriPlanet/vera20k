//! The retail CRT qsort (0x007C8B48, short-sort 0x007C8C9C), the one port
//! for every caller that sorts through it. It is unstable: equal keys land in
//! a fixed permutation that callers observe. The saved-entry caller 0x005596A0
//! sorts pointers with comparator 0x00559D30, without a secondary key, which
//! decides initial selection, including New.

use std::cmp::Ordering;

/// Sort record indices, keeping the records under their existing owner.
pub(crate) fn sort_indices_by(
    indices: &mut [usize],
    mut compare: impl FnMut(usize, usize) -> Ordering,
) {
    sort_by(indices, |&left, &right| compare(left, right));
}

/// qsort over the records themselves, as native swaps them in place.
pub(crate) fn sort_by<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> Ordering) {
    if items.len() < 2 {
        return;
    }
    let mut pending = Vec::new();
    let (mut low, mut high) = (0, items.len() - 1);
    loop {
        let count = high - low + 1;
        if count <= 8 {
            // Strict greater-than retains the first equal maximum. Swapping
            // that item to the end is why even two equal rows reverse.
            for end in (low + 1..=high).rev() {
                let mut maximum = low;
                for cursor in low + 1..=end {
                    if compare(&items[cursor], &items[maximum]) == Ordering::Greater {
                        maximum = cursor;
                    }
                }
                items.swap(maximum, end);
            }
        } else {
            items.swap(low + count / 2, low);
            let mut left = low;
            let mut right = high + 1;
            loop {
                left += 1;
                while left <= high && compare(&items[left], &items[low]) != Ordering::Greater {
                    left += 1;
                }
                right -= 1;
                while right > low && compare(&items[right], &items[low]) != Ordering::Less {
                    right -= 1;
                }
                if left > right {
                    break;
                }
                items.swap(left, right);
            }
            items.swap(low, right);
            let left_count = right - low;
            let right_count = high + 1 - left;
            // The original defers the larger partition and processes the
            // smaller one immediately. Equal sizes process the right first.
            // A growable stack removes the native pointer-array storage limit.
            if left_count < right_count {
                if right_count > 1 {
                    pending.push((left, high));
                }
                if left_count > 1 {
                    high = right - 1;
                    continue;
                }
            } else {
                if left_count > 1 {
                    pending.push((low, right - 1));
                }
                if right_count > 1 {
                    low = left;
                    continue;
                }
            }
        }
        let Some((next_low, next_high)) = pending.pop() else {
            return;
        };
        low = next_low;
        high = next_high;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_entry_order_and_comparison_count_match_original_instructions() {
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/seed_order.json",
        ))
        .unwrap();
        let cases = vectors["cases"].as_object().unwrap();
        assert_eq!(cases.len(), 36);
        for (name, case) in cases {
            let times: Vec<u64> = case["input_times"]
                .as_array()
                .unwrap()
                .iter()
                .map(|time| time.as_u64().unwrap())
                .collect();
            let expected: Vec<usize> = case["output_indices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| index.as_u64().unwrap() as usize)
                .collect();
            let mut indices: Vec<usize> = (0..times.len()).collect();
            let mut comparisons = 0;
            sort_indices_by(&mut indices, |left, right| {
                comparisons += 1;
                // Native comparator negates CompareFileTime: newest first.
                times[right].cmp(&times[left])
            });
            assert_eq!(indices, expected, "{name}");
            assert_eq!(comparisons, case["comparisons"].as_u64().unwrap(), "{name}");
        }
    }
}
