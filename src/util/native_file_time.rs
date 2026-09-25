//! Retail saved-list timestamp formatting. Native 0x005596A0 uses the active
//! Windows locale through GetDateFormatA / GetTimeFormatA, then converts ACP.
//! Other platforms format the same local time with the user's locale short
//! date and time (`strftime_l` `%x` / `%X`); the strings follow that
//! platform's locale data, as the Win32 ones follow Windows'.

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct NativeFileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct NativeSystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

const WINDOWS_EPOCH_SECONDS: u64 = 11_644_473_600;
const TICKS_PER_SECOND: u64 = 10_000_000;

/// Saved-list 0x005596A0 omits both columns for either sentinel DWORD.
fn is_sentinel(ticks: u64) -> bool {
    ticks as u32 == u32::MAX || (ticks >> 32) as u32 == u32::MAX
}

#[cfg(any(windows, unix))]
pub(crate) fn format_timestamp_parts(unix_secs: u64) -> Option<(String, String)> {
    let ticks = unix_secs
        .checked_add(WINDOWS_EPOCH_SECONDS)?
        .checked_mul(TICKS_PER_SECOND)?;
    format_file_time_parts(ticks)
}

/// Format native FILETIME without discarding its epoch or fractional fields.
#[cfg(windows)]
pub(crate) fn format_file_time_parts(ticks: u64) -> Option<(String, String)> {
    if is_sentinel(ticks) {
        return None;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToLocalFileTime(
            source: *const NativeFileTime,
            local: *mut NativeFileTime,
        ) -> i32;
        fn FileTimeToSystemTime(
            file_time: *const NativeFileTime,
            system_time: *mut NativeSystemTime,
        ) -> i32;
    }

    let source = NativeFileTime {
        low: ticks as u32,
        high: (ticks >> 32) as u32,
    };
    let mut local = NativeFileTime { low: 0, high: 0 };
    let mut system = NativeSystemTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        milliseconds: 0,
    };
    // SAFETY: all pointers refer to live, correctly laid-out Win32 structs.
    unsafe {
        if FileTimeToLocalFileTime(&source, &mut local) == 0
            || FileTimeToSystemTime(&local, &mut system) == 0
        {
            return None;
        }
    }
    format_local_system_time(&system)
}

#[cfg(windows)]
fn format_local_system_time(system: &NativeSystemTime) -> Option<(String, String)> {
    const LOCALE_USER_DEFAULT: u32 = 0x0400;
    const DATE_SHORTDATE: u32 = 1;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDateFormatA(
            locale: u32,
            flags: u32,
            date: *const NativeSystemTime,
            format: *const u8,
            output: *mut u8,
            output_len: i32,
        ) -> i32;
        fn GetTimeFormatA(
            locale: u32,
            flags: u32,
            time: *const NativeSystemTime,
            format: *const u8,
            output: *mut u8,
            output_len: i32,
        ) -> i32;
    }

    let mut date = [0u8; 128];
    let mut time = [0u8; 128];
    // SAFETY: Win32 receives a valid SYSTEMTIME and fixed 128-byte outputs,
    // exactly matching the native load/save dialog.
    let (date_len, time_len) = unsafe {
        (
            GetDateFormatA(
                LOCALE_USER_DEFAULT,
                DATE_SHORTDATE,
                system,
                std::ptr::null(),
                date.as_mut_ptr(),
                date.len() as i32,
            ),
            GetTimeFormatA(
                LOCALE_USER_DEFAULT,
                0,
                system,
                std::ptr::null(),
                time.as_mut_ptr(),
                time.len() as i32,
            ),
        )
    };
    if date_len <= 0 || time_len <= 0 {
        return None;
    }
    let date_payload = &date[..date_len.saturating_sub(1) as usize];
    let time_payload = &time[..time_len.saturating_sub(1) as usize];
    Some((
        crate::util::native_string::acp_decode(date_payload),
        crate::util::native_string::acp_decode(time_payload),
    ))
}

/// The user's LC_TIME locale (the C locale when it cannot load), created
/// once for the process and never freed.
#[cfg(unix)]
struct TimeLocale(libc::locale_t);

// SAFETY: the handle is never mutated after creation; strftime_l only reads
// it, and POSIX allows a locale object to be used from any thread.
#[cfg(unix)]
unsafe impl Send for TimeLocale {}
#[cfg(unix)]
unsafe impl Sync for TimeLocale {}

#[cfg(unix)]
fn time_locale() -> Option<libc::locale_t> {
    static LOCALE: std::sync::OnceLock<Option<TimeLocale>> = std::sync::OnceLock::new();
    LOCALE
        .get_or_init(|| {
            // SAFETY: newlocale with a null base allocates a fresh handle.
            let locale = unsafe {
                let user = libc::newlocale(libc::LC_TIME_MASK, c"".as_ptr(), std::ptr::null_mut());
                if user.is_null() {
                    libc::newlocale(libc::LC_TIME_MASK, c"C".as_ptr(), std::ptr::null_mut())
                } else {
                    user
                }
            };
            (!locale.is_null()).then_some(TimeLocale(locale))
        })
        .as_ref()
        .map(|locale| locale.0)
}

/// FILETIME ticks as local time in the user's locale short date and time.
#[cfg(unix)]
pub(crate) fn format_file_time_parts(ticks: u64) -> Option<(String, String)> {
    if is_sentinel(ticks) {
        return None;
    }
    let seconds = (ticks / TICKS_PER_SECOND).checked_sub(WINDOWS_EPOCH_SECONDS)?;
    let seconds = libc::time_t::try_from(seconds).ok()?;
    let locale = time_locale()?;
    // SAFETY: `tm` is plain data that localtime_r fills; strftime_l writes at
    // most the buffer length and reads the live process locale.
    unsafe {
        let mut local: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&seconds, &mut local).is_null() {
            return None;
        }
        let format = |pattern: &std::ffi::CStr| {
            let mut out = [0u8; 128];
            let len = libc::strftime_l(
                out.as_mut_ptr().cast(),
                out.len(),
                pattern.as_ptr(),
                &local,
                locale,
            );
            (len > 0).then(|| String::from_utf8_lossy(&out[..len]).into_owned())
        };
        format(c"%x").zip(format(c"%X"))
    }
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn format_timestamp_parts(_unix_secs: u64) -> Option<(String, String)> {
    None
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn format_file_time_parts(_ticks: u64) -> Option<(String, String)> {
    None
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::{TICKS_PER_SECOND, WINDOWS_EPOCH_SECONDS, format_file_time_parts};

    #[test]
    fn unix_formats_a_local_short_date_and_time() {
        // 2026-07-30 13:45:12 UTC.
        let ticks = (1_785_419_112 + WINDOWS_EPOCH_SECONDS) * TICKS_PER_SECOND;
        let (date, time) = format_file_time_parts(ticks).expect("locale formatting");
        assert!(!date.is_empty() && !time.is_empty(), "{date:?} {time:?}");
        assert_eq!(format_file_time_parts(u64::from(u32::MAX)), None);
        assert_eq!(format_file_time_parts(0), None, "before 1970");
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{NativeSystemTime, format_local_system_time};

    #[test]
    fn fixed_system_time_uses_platform_short_date_and_time() {
        let fixed = NativeSystemTime {
            year: 2026,
            month: 7,
            day_of_week: 4,
            day: 30,
            hour: 13,
            minute: 45,
            second: 12,
            milliseconds: 0,
        };
        let (date, time) = format_local_system_time(&fixed).expect("Win32 locale formatting");
        assert!(!date.is_empty());
        assert!(!time.is_empty());
        assert!(!date.contains("ago"));
        assert!(!time.contains("ago"));
    }
}
