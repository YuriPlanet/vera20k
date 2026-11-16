//! CPU time per game thread, for the in-game performance panel's Threads row.
//!
//! The Cores row is the whole machine's load per core, and the system moves
//! threads between cores, so it cannot say which part of the game fills one.
//! This reads the time the main thread, the simulation thread and the whole
//! process have run, as the system counts it:
//!
//! - **Windows**: `GetThreadTimes` and `GetProcessTimes`.
//! - **Unix** (Linux, macOS, Android): `clock_gettime` with
//!   `CLOCK_THREAD_CPUTIME_ID` and `CLOCK_PROCESS_CPUTIME_ID`.
//! - **Anything else** (the web build): no reading, and the panel hides the row.
//!
//! A thread's clock is read from that thread: the panel reads the main
//! thread's when it closes a window, and the simulation thread notes its own
//! after each job ([`note_simulation_thread`]). A job still running when a
//! window closes therefore counts toward the next window, and toward Others
//! in this one.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::util::time::{Duration, Instant};

/// The simulation thread's CPU time at the end of its last job, in
/// nanoseconds.
static SIMULATION_THREAD_NS: AtomicU64 = AtomicU64::new(0);

/// Called by the simulation thread after each job.
pub(crate) fn note_simulation_thread() {
    if let Some(ns) = platform::thread_cpu_ns() {
        SIMULATION_THREAD_NS.store(ns, Ordering::Relaxed);
    }
}

/// CPU time used so far, in nanoseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CpuTimes {
    main: u64,
    simulation: u64,
    process: u64,
}

/// Each thread's share of one core between two samples, in percent: a thread
/// that never rests reads 100. `others` is the rest of the process: the
/// worker pool, the frame hand-off, audio and the graphics driver's threads,
/// so it passes 100 when several of them run at once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ThreadLoads {
    pub(crate) main: f32,
    pub(crate) simulation: f32,
    pub(crate) others: f32,
}

#[derive(Debug, Default)]
pub(crate) struct ThreadLoadSampler {
    previous: Option<(Instant, CpuTimes)>,
}

impl ThreadLoadSampler {
    /// The loads since the previous call. Call it on the main thread. `None`
    /// on the first call and where the platform has no thread clocks.
    pub(crate) fn sample(&mut self, now: Instant) -> Option<ThreadLoads> {
        let times = CpuTimes {
            main: platform::thread_cpu_ns()?,
            simulation: SIMULATION_THREAD_NS.load(Ordering::Relaxed),
            process: platform::process_cpu_ns()?,
        };
        let (then, before) = self.previous.replace((now, times))?;
        thread_loads(before, times, now.duration_since(then))
    }
}

fn thread_loads(before: CpuTimes, now: CpuTimes, wall: Duration) -> Option<ThreadLoads> {
    let wall_ns = wall.as_nanos();
    if wall_ns == 0 {
        return None;
    }
    let percent = |ns: u64| (ns as f64 * 100.0 / wall_ns as f64) as f32;
    // A new simulation thread starts its clock over: no load that window.
    let main = now.main.saturating_sub(before.main);
    let simulation = now.simulation.saturating_sub(before.simulation);
    let process = now.process.saturating_sub(before.process);
    Some(ThreadLoads {
        main: percent(main),
        simulation: percent(simulation),
        others: percent(process.saturating_sub(main.saturating_add(simulation))),
    })
}

#[cfg(windows)]
mod platform {
    //! Local ABI declarations, as in `core_load`, instead of another
    //! windows-sys feature group.

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> isize;
        fn GetCurrentProcess() -> isize;
        fn GetThreadTimes(
            thread: isize,
            creation_time: *mut u64,
            exit_time: *mut u64,
            kernel_time: *mut u64,
            user_time: *mut u64,
        ) -> i32;
        fn GetProcessTimes(
            process: isize,
            creation_time: *mut u64,
            exit_time: *mut u64,
            kernel_time: *mut u64,
            user_time: *mut u64,
        ) -> i32;
    }

    /// Kernel plus user time, when the call read them. FILETIME counts
    /// 100 ns intervals.
    fn nanoseconds(read: i32, kernel: u64, user: u64) -> Option<u64> {
        (read != 0).then(|| kernel.saturating_add(user).saturating_mul(100))
    }

    /// https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes
    pub(super) fn thread_cpu_ns() -> Option<u64> {
        let (mut creation, mut exit, mut kernel, mut user) = (0u64, 0u64, 0u64, 0u64);
        // SAFETY: the pseudo-handle needs no closing, and every out-pointer
        // is a live local of FILETIME's size.
        let read = unsafe {
            GetThreadTimes(
                GetCurrentThread(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        nanoseconds(read, kernel, user)
    }

    /// https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes
    pub(super) fn process_cpu_ns() -> Option<u64> {
        let (mut creation, mut exit, mut kernel, mut user) = (0u64, 0u64, 0u64, 0u64);
        // SAFETY: as in `thread_cpu_ns`.
        let read = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        nanoseconds(read, kernel, user)
    }
}

#[cfg(unix)]
mod platform {
    /// https://man7.org/linux/man-pages/man3/clock_gettime.3.html
    fn clock_ns(clock: libc::clockid_t) -> Option<u64> {
        // SAFETY: timespec is plain integers, for which zero is a value.
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        // SAFETY: the out-pointer is a live local.
        let read = unsafe { libc::clock_gettime(clock, &mut time) };
        (read == 0).then(|| {
            (time.tv_sec as u64)
                .saturating_mul(1_000_000_000)
                .saturating_add(time.tv_nsec as u64)
        })
    }

    pub(super) fn thread_cpu_ns() -> Option<u64> {
        clock_ns(libc::CLOCK_THREAD_CPUTIME_ID)
    }

    pub(super) fn process_cpu_ns() -> Option<u64> {
        clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID)
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    pub(super) fn thread_cpu_ns() -> Option<u64> {
        None
    }

    pub(super) fn process_cpu_ns() -> Option<u64> {
        None
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::{CpuTimes, ThreadLoadSampler, thread_loads};
    use crate::util::time::{Duration, Instant};

    #[test]
    fn a_threads_load_is_its_cpu_time_as_a_share_of_one_core() {
        let before = CpuTimes {
            main: 1_000_000_000,
            simulation: 5_000_000_000,
            process: 9_000_000_000,
        };
        let now = CpuTimes {
            main: 1_900_000_000,
            simulation: 5_100_000_000,
            process: 11_500_000_000,
        };
        let loads = thread_loads(before, now, Duration::from_secs(1)).expect("a window");
        // 0.9 s and 0.1 s of one second; the other 1.5 s is the rest's.
        assert_eq!(
            (loads.main, loads.simulation, loads.others),
            (90.0, 10.0, 150.0)
        );
        // A new simulation thread's clock starts over: no load, not a wrap.
        let restarted = CpuTimes {
            simulation: 1,
            ..now
        };
        let loads = thread_loads(now, restarted, Duration::from_secs(1)).expect("a window");
        assert_eq!(
            (loads.main, loads.simulation, loads.others),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(thread_loads(before, now, Duration::ZERO), None);
    }

    /// The platform's clocks through the sampler: the 20 ms this thread
    /// works between two samples are in its load.
    #[test]
    #[cfg(any(windows, unix))]
    fn the_sampler_reads_the_calling_threads_own_work() {
        let clock = || super::platform::thread_cpu_ns().expect("a thread clock");
        let mut sampler = ThreadLoadSampler::default();
        let started = Instant::now();
        assert_eq!(sampler.sample(started), None);
        let worked = clock() + 20_000_000;
        let mut spun = 0u64;
        while clock() < worked {
            spun = std::hint::black_box(spun.wrapping_add(1));
        }
        let now = Instant::now();
        let loads = sampler.sample(now).expect("a second sample");
        // Percent of a core times seconds: 20 ms of work is 2.
        let wall = now.duration_since(started).as_secs_f32();
        assert!(loads.main * wall >= 1.9, "{loads:?} over {wall} s");
    }
}
