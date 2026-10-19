//! Process-level resource sampling for the dashboard: CPU share, working set,
//! peak working set and thread count.
//!
//! This is **process** state, not simulation state — the engine's own discipline
//! says diagnostics belong to the app's `DiagnosticsState`, and nothing here is
//! visible to (or hashed by) the simulation.
//!
//! ## Platform support
//! - **Windows**: `GetProcessTimes` (CPU time), `GetProcessMemoryInfo`
//!   (working set), a toolhelp32 thread snapshot (thread count).
//! - **Linux**: the same three facts from `/proc/self`.
//! - **macOS**: `proc_pidinfo` and `getrusage` for this process only.
//! - **Anything else**: zeros. The dashboard shows the row rather than
//!   pretending the numbers exist.
//!
//! ## Cost
//! CPU and memory reads are a couple of syscalls; the thread snapshot walks
//! every thread in the machine, so it is refreshed at most once a second while
//! CPU and memory refresh on every sample.

use std::cell::Cell;
use std::time::Instant;

use crate::sim::telemetry::ProcessStats;

/// Refresh interval for the thread count, which walks every thread in the
/// process — too expensive to repeat at snapshot rate.
const THREAD_REFRESH: std::time::Duration = std::time::Duration::from_secs(1);

/// Stateful CPU sampler: CPU share is a delta between two readings, so the
/// previous CPU time has to live somewhere. Cells rather than `&mut` because
/// the dashboard reads it through a shared `&AppState`.
#[derive(Debug, Default)]
pub struct ProcessStatsSampler {
    last_cpu_seconds: Cell<f64>,
    last_instant: Cell<Option<Instant>>,
    threads: Cell<u32>,
    last_thread_poll: Cell<Option<Instant>>,
}

impl ProcessStatsSampler {
    /// Sample CPU share, memory and thread count.
    ///
    /// `cpu_percent` is the share of the **whole machine** (all cores), so a
    /// single busy core on a 4-core machine reads 25%. The first call has no
    /// previous reading and reports zero.
    pub fn sample(&self) -> ProcessStats {
        let cores = logical_cores();
        let now = Instant::now();
        let (cpu_seconds, working_set_bytes, peak_working_set_bytes) = platform::read_cpu_memory();

        let cpu_percent = match (self.last_instant.get(), self.last_cpu_seconds.get()) {
            (Some(previous_instant), previous_cpu) => {
                let wall = now.duration_since(previous_instant).as_secs_f64();
                let used = cpu_seconds - previous_cpu;
                if wall > 0.0 && used >= 0.0 {
                    ((used / wall / cores as f64 * 100.0) as f32).clamp(0.0, 100.0)
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        self.last_cpu_seconds.set(cpu_seconds);
        self.last_instant.set(Some(now));

        // Thread count is a whole-machine walk; hold it at 1 Hz.
        let last_poll = self.last_thread_poll.get();
        if last_poll.is_none_or(|previous| now.duration_since(previous) >= THREAD_REFRESH) {
            self.threads.set(platform::thread_count());
            self.last_thread_poll.set(Some(now));
        }

        ProcessStats {
            cpu_percent,
            cpu_cores: cores,
            working_set_mb: bytes_to_mb(working_set_bytes),
            peak_working_set_mb: bytes_to_mb(peak_working_set_bytes),
            threads: self.threads.get(),
            gpu_adapter: None,
            gpu_texture_mb: 0.0,
        }
    }
}

fn logical_cores() -> u32 {
    static CORES: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CORES.get_or_init(|| {
        std::thread::available_parallelism()
            .map(|value| value.get() as u32)
            .unwrap_or(1)
            .max(1)
    })
}

fn bytes_to_mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

// ---------------------------------------------------------------- windows

#[cfg(windows)]
mod platform {
    //! Read-only Win32 queries; local ABI declarations avoid enabling additional
    //! windows-sys feature groups solely for these process statistics.

    const TH32CS_SNAPTHREAD: u32 = 0x0000_0004;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    /// `THREADENTRY32`: four DWORDs then three LONGs — no pointer-sized fields,
    /// so the layout is identical on 32- and 64-bit.
    #[repr(C)]
    struct ThreadEntry32 {
        size: u32,
        usage: u32,
        thread_id: u32,
        owner_process_id: u32,
        base_priority: i32,
        delta_priority: i32,
        flags: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessId(process: isize) -> u32;
        fn GetProcessTimes(
            process: isize,
            creation_time: *mut u64,
            exit_time: *mut u64,
            kernel_time: *mut u64,
            user_time: *mut u64,
        ) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> isize;
        fn Thread32First(snapshot: isize, entry: *mut ThreadEntry32) -> i32;
        fn Thread32Next(snapshot: isize, entry: *mut ThreadEntry32) -> i32;
        fn CloseHandle(object: isize) -> i32;
    }

    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    /// CPU seconds used so far, plus the current and peak working set.
    pub(super) fn read_cpu_memory() -> (f64, u64, u64) {
        // SAFETY: every pointer handed to these calls points at a live, correctly
        // sized local; the return values are checked before they are used.
        unsafe {
            let process = GetCurrentProcess();

            let mut creation = 0u64;
            let mut exit = 0u64;
            let mut kernel = 0u64;
            let mut user = 0u64;
            let cpu_seconds = if GetProcessTimes(
                process,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            ) != 0
            {
                // FILETIME counts 100 ns intervals; the two clocks add up.
                ((kernel + user) as f64) / 1.0e7
            } else {
                0.0
            };

            let mut counters = ProcessMemoryCounters {
                cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
                page_fault_count: 0,
                peak_working_set_size: 0,
                working_set_size: 0,
                quota_peak_paged_pool_usage: 0,
                quota_paged_pool_usage: 0,
                quota_peak_non_paged_pool_usage: 0,
                quota_non_paged_pool_usage: 0,
                pagefile_usage: 0,
                peak_pagefile_usage: 0,
            };
            let (working, peak) = if GetProcessMemoryInfo(
                process,
                &mut counters,
                std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ) != 0
            {
                (
                    counters.working_set_size as u64,
                    counters.peak_working_set_size as u64,
                )
            } else {
                (0, 0)
            };

            (cpu_seconds, working, peak)
        }
    }

    /// Live thread count of this process.
    pub(super) fn thread_count() -> u32 {
        // SAFETY: the snapshot handle is closed on every path; `entry` is a live
        // local whose `size` field the toolhelp API requires to be preset.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE || snapshot == 0 {
                return 0;
            }
            let own_pid = GetProcessId(GetCurrentProcess());
            let mut entry = ThreadEntry32 {
                size: std::mem::size_of::<ThreadEntry32>() as u32,
                usage: 0,
                thread_id: 0,
                owner_process_id: 0,
                base_priority: 0,
                delta_priority: 0,
                flags: 0,
            };
            let mut count = 0u32;
            if Thread32First(snapshot, &mut entry) != 0 {
                loop {
                    if entry.owner_process_id == own_pid {
                        count += 1;
                    }
                    if Thread32Next(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            count
        }
    }
}

// ----------------------------------------------------------------- linux

#[cfg(target_os = "linux")]
mod platform {
    /// CPU seconds (`utime + stime` from `/proc/self/stat`), working set and
    /// peak working set (resident pages, page size from `/proc/self/statm`).
    pub(super) fn read_cpu_memory() -> (f64, u64, u64) {
        let cpu_seconds = std::fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|stat| {
                let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
                cpu_seconds_from_stat(&stat, hz)
            })
            .unwrap_or(0.0);
        // sysconf units vary across systems; do not assume 4 KiB pages or 100 Hz.
        // https://man7.org/linux/man-pages/man3/sysconf.3.html
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
        let resident_pages = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|statm| {
                statm
                    .split_whitespace()
                    .nth(1)
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .unwrap_or(0);
        let working = resident_pages.saturating_mul(page_size);
        let peak_bytes = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    let rest = line.strip_prefix("VmHWM:")?;
                    rest.split_whitespace().next()?.parse::<u64>().ok()
                })
            })
            .map(|kib| kib.saturating_mul(1024))
            .unwrap_or(working);
        (cpu_seconds, working, peak_bytes.max(working))
    }

    /// Fields 14 and 15 (`utime`, `stime`) in clock ticks; the second field is
    /// the command name in parentheses and may contain spaces.
    fn cpu_seconds_from_stat(stat: &str, ticks_per_second: libc::c_long) -> Option<f64> {
        if ticks_per_second <= 0 {
            return None;
        }
        let after_comm = stat.rsplit_once(") ")?.1;
        let fields: Vec<&str> = after_comm.split_whitespace().collect();
        // `fields[0]` is `state`, which is field 3, so field 14 is index 11.
        let utime: f64 = fields.get(11)?.parse().ok()?;
        let stime: f64 = fields.get(12)?.parse().ok()?;
        Some((utime + stime) / ticks_per_second as f64)
    }

    pub(super) fn thread_count() -> u32 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    let rest = line.strip_prefix("Threads:")?;
                    rest.trim().parse::<u32>().ok()
                })
            })
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod platform {
    fn task_info() -> Option<libc::proc_taskinfo> {
        let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        // SAFETY: proc_pidinfo writes at most the supplied buffer size. Only a
        // complete, successful query of our own PID is read as initialized.
        let written = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDTASKINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        (written == size).then(|| unsafe { info.assume_init() })
    }

    pub(super) fn read_cpu_memory() -> (f64, u64, u64) {
        let working = task_info().map_or(0, |info| info.pti_resident_size);
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: getrusage initializes the provided record on success.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return (0.0, working, working);
        }
        let usage = unsafe { usage.assume_init() };
        let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
        // Darwin's calcru copies resident_size_max directly: ru_maxrss is bytes,
        // unlike Linux's KiB. XNU bsd/kern/kern_resource.c, calcru/getrusage:
        // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_resource.c
        (
            seconds(usage.ru_utime) + seconds(usage.ru_stime),
            working,
            (usage.ru_maxrss.max(0) as u64).max(working),
        )
    }

    pub(super) fn thread_count() -> u32 {
        // proc_taskinfo reports only our task's threads and acquires no handles.
        // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h
        task_info().map_or(0, |info| info.pti_threadnum.max(0) as u32)
    }
}

// --------------------------------------------------------------- fallback

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    pub(super) fn read_cpu_memory() -> (f64, u64, u64) {
        (0.0, 0, 0)
    }

    pub(super) fn thread_count() -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_reports_a_non_negative_cpu_share() {
        let sampler = ProcessStatsSampler::default();
        let first = sampler.sample();
        assert!(first.cpu_percent >= 0.0);
        assert!(first.cpu_cores >= 1);
        // Busy-wait so the second reading has a real delta to divide.
        let start = Instant::now();
        while start.elapsed().as_millis() < 30 {
            std::hint::spin_loop();
        }
        let second = sampler.sample();
        assert!(second.cpu_percent >= 0.0, "cpu share never goes negative");
        assert!(second.cpu_percent <= 100.0, "capped at the machine total");
    }

    #[test]
    fn memory_is_reported_in_mebibytes() {
        let sampler = ProcessStatsSampler::default();
        let stats = sampler.sample();
        assert!(stats.working_set_mb >= 0.0);
        assert!(
            stats.peak_working_set_mb >= stats.working_set_mb,
            "the peak working set never drops below the current one"
        );
    }

    #[test]
    fn thread_count_is_refreshed_and_nonzero_on_supported_platforms() {
        let sampler = ProcessStatsSampler::default();
        let stats = sampler.sample();
        if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            assert!(stats.threads > 0, "the main thread alone counts as one");
        }
    }
}
