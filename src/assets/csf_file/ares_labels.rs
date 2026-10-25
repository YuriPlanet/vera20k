//! Ordered Ares labels. Keep the search and sort's equal-key choices: native
//! appends to an unsorted array, so even unique input files can create duplicates.
//! Source: Ares3.0 LoadAdditionalCSF100609A0 / sort10060330 and gamemd CRT
//! bsearch7C8E25, reached by both the override hook and LoadString734E93.

type Label = (String, Option<super::CsfText>);

/// CRT bsearch7C8E25 chooses the lower middle for even lengths. Rust's
/// binary_search may choose a different equal slot and requires sorted input.
pub(super) fn native_label_search(labels: &[Label], key: &str) -> Option<usize> {
    let mut first = 0;
    let mut count = labels.len();
    while count != 0 {
        let half = (count - 1) / 2;
        let mid = first + half;
        match key.cmp(labels[mid].0.as_str()) {
            std::cmp::Ordering::Equal => return Some(mid),
            std::cmp::Ordering::Less => count = half,
            std::cmp::Ordering::Greater => {
                first = mid + 1;
                count -= half + 1;
            }
        }
    }
    None
}

pub(super) fn sort_labels(labels: &mut [Label]) {
    let len = labels.len();
    sort(labels, len);
}

/// Ares10060330's MSVC introsort. Equal labels retain their slot identity
/// through this algorithm; neither a stable nor a host unstable sort substitutes.
fn sort(mut labels: &mut [Label], mut ideal: usize) {
    while labels.len() > 32 {
        if ideal == 0 {
            heap_sort(labels);
            return;
        }
        let (left, right) = partition(labels);
        ideal = (ideal >> 1) + (ideal >> 2);
        let (before, rest) = labels.split_at_mut(left);
        let after = &mut rest[right - left..];
        if before.len() < after.len() {
            sort(before, ideal);
            labels = after;
        } else {
            sort(after, ideal);
            labels = before;
        }
    }
    //100603E1..10060533: insertion sort moves only strictly greater keys.
    for next in 1..labels.len() {
        let mut hole = next;
        while hole > 0 && labels[hole].0 < labels[hole - 1].0 {
            labels.swap(hole, hole - 1);
            hole -= 1;
        }
    }
}

///1005FD50 orders three samples using strict comparisons.
fn median(labels: &mut [Label], first: usize, mid: usize, last: usize) {
    if labels[mid].0 < labels[first].0 {
        labels.swap(mid, first);
    }
    if labels[last].0 < labels[mid].0 {
        labels.swap(last, mid);
        if labels[mid].0 < labels[first].0 {
            labels.swap(mid, first);
        }
    }
}

///1005FE10..100601FE: median/ninther followed by a three-way partition.
fn partition(labels: &mut [Label]) -> (usize, usize) {
    let last = labels.len() - 1;
    let mid = labels.len() / 2;
    if last > 40 {
        let step = labels.len() / 8;
        median(labels, 0, step, 2 * step);
        median(labels, mid - step, mid, mid + step);
        median(labels, last - 2 * step, last - step, last);
        median(labels, step, mid, last - step);
    } else {
        median(labels, 0, mid, last);
    }
    let mut equal_first = mid;
    let mut equal_end = mid + 1;
    while equal_first > 0 && labels[equal_first - 1].0 == labels[equal_first].0 {
        equal_first -= 1;
    }
    while equal_end < labels.len() && labels[equal_end].0 == labels[equal_first].0 {
        equal_end += 1;
    }
    let mut scan_right = equal_end;
    let mut scan_left = equal_first;
    loop {
        while scan_right < labels.len() {
            if labels[equal_first].0 < labels[scan_right].0 {
                scan_right += 1;
            } else if labels[scan_right].0 < labels[equal_first].0 {
                break;
            } else {
                labels.swap(equal_end, scan_right);
                equal_end += 1;
                scan_right += 1;
            }
        }
        while scan_left > 0 {
            if labels[scan_left - 1].0 < labels[equal_first].0 {
                scan_left -= 1;
            } else if labels[equal_first].0 < labels[scan_left - 1].0 {
                break;
            } else {
                equal_first -= 1;
                labels.swap(equal_first, scan_left - 1);
                scan_left -= 1;
            }
        }
        if scan_left == 0 && scan_right == labels.len() {
            return (equal_first, equal_end);
        }
        if scan_left == 0 {
            if equal_end != scan_right {
                labels.swap(equal_first, equal_end);
            }
            equal_end += 1;
            labels.swap(equal_first, scan_right);
            equal_first += 1;
            scan_right += 1;
        } else if scan_right == labels.len() {
            scan_left -= 1;
            equal_first -= 1;
            labels.swap(scan_left, equal_first);
            equal_end -= 1;
            labels.swap(equal_first, equal_end);
        } else {
            scan_left -= 1;
            labels.swap(scan_right, scan_left);
            scan_right += 1;
        }
    }
}

///1005FBE0 /10060210 move the greater child up, choosing the RIGHT child on
/// equality, then push the saved value back up using strict comparisons.
fn adjust_heap(labels: &mut [Label], top: usize, value: Label) {
    let mut hole = top;
    while 2 * hole + 2 < labels.len() {
        let right = 2 * hole + 2;
        let child = if labels[right].0 < labels[right - 1].0 {
            right - 1
        } else {
            right
        };
        labels[hole] = labels[child].clone();
        hole = child;
    }
    if 2 * hole + 1 < labels.len() {
        labels[hole] = labels[2 * hole + 1].clone();
        hole = 2 * hole + 1;
    }
    while hole > top {
        let parent = (hole - 1) / 2;
        if labels[parent].0 >= value.0 {
            break;
        }
        labels[hole] = labels[parent].clone();
        hole = parent;
    }
    labels[hole] = value;
}

fn heap_sort(labels: &mut [Label]) {
    for parent in (0..labels.len() / 2).rev() {
        let value = labels[parent].clone();
        adjust_heap(labels, parent, value);
    }
    for end in (1..labels.len()).rev() {
        let value = labels[end].clone();
        labels[end] = labels[0].clone();
        adjust_heap(&mut labels[..end], 0, value);
    }
}
