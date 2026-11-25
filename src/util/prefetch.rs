//! A cache hint (VERA-only, sim-scale; results unchanged): ask the processor
//! to start loading memory the caller will read soon.
//!
//! A walk over many large records that live apart, such as the live object
//! pass over 20,000 units of about 2 KB each, finds every record out of the
//! cache: together they are several times the last cache level, so each
//! turn's first reads wait for main memory. Asking for a record a few turns
//! early lets that wait overlap the turns in between.
//!
//! The hint reads no value and cannot fault, so no result depends on it. A
//! target without a prefetch instruction (the browser build) does nothing.

/// The distance between the addresses asked for: one cache line (Apple's
/// processors use 128-byte lines; the others VERA runs on use 64).
const LINE: usize = if cfg!(all(target_arch = "aarch64", target_vendor = "apple")) {
    128
} else {
    64
};

/// Ask the processor to start loading all of `value`.
#[inline(always)]
pub fn prefetch<T>(value: &T) {
    let start = std::ptr::from_ref(value).cast::<u8>();
    let mut offset = 0;
    while offset < std::mem::size_of::<T>() {
        line(start.wrapping_add(offset));
        offset += LINE;
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn line(address: *const u8) {
    // SAFETY: PRFM is a hint. It reads no value the program can see, writes
    // nothing and never faults, whatever the address.
    unsafe {
        std::arch::asm!(
            "prfm pldl1keep, [{address}]",
            address = in(reg) address,
            options(nostack, preserves_flags, readonly)
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn line(address: *const u8) {
    use std::arch::x86_64::{_MM_HINT_T0, _mm_prefetch};
    // SAFETY: PREFETCHT0 is a hint. It reads no value the program can see,
    // writes nothing and never faults, whatever the address.
    #[allow(unused_unsafe, reason = "safe to call from Rust 1.87 on")]
    unsafe {
        _mm_prefetch(address.cast::<i8>(), _MM_HINT_T0);
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[inline(always)]
fn line(_address: *const u8) {}
