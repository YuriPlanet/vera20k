//! Load on each logical core, for the in-game FPS panel's Cores row.
//!
//! This is the whole machine's load, so other programs count too: the
//! platforms keep busy time per core, not per process and core.
//!
//! - **Windows**: `NtQuerySystemInformation(SystemProcessorPerformanceInformation)`.
//! - **Linux**: the `cpuN` lines of `/proc/stat`.
//! - **macOS**: `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`.
//! - **Anything else** (the web build): no reading, and the panel hides the row.

/// One logical core's running busy and total time, in the platform's ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CoreTicks {
    busy: u64,
    total: u64,
}

#[derive(Debug, Default)]
pub(crate) struct CoreLoadSampler {
    previous: Vec<CoreTicks>,
}

impl CoreLoadSampler {
    /// Busy percent per core since the previous call, in the platform's core
    /// order. `None` on the first call, when the core count changed, and where
    /// the platform has no per-core counters.
    pub(crate) fn sample(&mut self) -> Option<Vec<f32>> {
        let now = platform::core_ticks()?;
        let loads = core_loads(&self.previous, &now);
        self.previous = now;
        loads
    }
}

fn core_loads(previous: &[CoreTicks], now: &[CoreTicks]) -> Option<Vec<f32>> {
    if now.is_empty() || previous.len() != now.len() {
        return None;
    }
    Some(
        previous
            .iter()
            .zip(now)
            .map(|(before, after)| {
                let total = after.total.saturating_sub(before.total);
                let busy = after.busy.saturating_sub(before.busy).min(total);
                if total == 0 {
                    0.0
                } else {
                    busy as f32 * 100.0 / total as f32
                }
            })
            .collect(),
    )
}

/// The `cpuN` lines of `/proc/stat`: user, nice, system, idle, iowait, irq,
/// softirq and steal, in USER_HZ ticks. Busy is all but idle and iowait;
/// guest time is already inside user, so the sum stops at steal.
/// https://man7.org/linux/man-pages/man5/proc_stat.5.html
#[cfg(any(test, target_os = "linux"))]
fn core_ticks_from_proc_stat(stat: &str) -> Option<Vec<CoreTicks>> {
    let cores: Vec<CoreTicks> = stat
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let id = fields.next()?.strip_prefix("cpu")?;
            if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let ticks: Vec<u64> = fields
                .take(8)
                .map(|value| value.parse().ok())
                .collect::<Option<_>>()?;
            let total: u64 = ticks.iter().sum();
            let idle = ticks.get(3).copied().unwrap_or(0) + ticks.get(4).copied().unwrap_or(0);
            Some(CoreTicks {
                busy: total.saturating_sub(idle),
                total,
            })
        })
        .collect();
    (!cores.is_empty()).then_some(cores)
}

#[cfg(windows)]
mod platform {
    //! A local ABI declaration, as in `app::process_stats`, instead of another
    //! windows-sys feature group.

    /// `SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION` (winternl.h): idle, kernel
    /// (which includes idle) and user time per logical processor, in 100 ns.
    /// https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntquerysysteminformation
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct ProcessorPerformance {
        idle: i64,
        kernel: i64,
        user: i64,
        _reserved: [i64; 2],
        _reserved2: u32,
    }

    /// `SystemProcessorPerformanceInformation`.
    const SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION: u32 = 8;
    /// A processor group holds at most 64 logical processors, and the query
    /// answers for the calling thread's group.
    const GROUP_PROCESSORS: usize = 64;

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQuerySystemInformation(
            class: u32,
            info: *mut std::ffi::c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }

    pub(super) fn core_ticks() -> Option<Vec<super::CoreTicks>> {
        let mut entries = [ProcessorPerformance::default(); GROUP_PROCESSORS];
        let mut returned = 0u32;
        // SAFETY: the buffer is a live local array of the documented record and
        // its byte length is passed; only the entries the call filled are read.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
                entries.as_mut_ptr().cast(),
                std::mem::size_of_val(&entries) as u32,
                &mut returned,
            )
        };
        if status != 0 {
            return None;
        }
        let count = returned as usize / std::mem::size_of::<ProcessorPerformance>();
        Some(
            entries[..count.min(GROUP_PROCESSORS)]
                .iter()
                .map(|entry| {
                    let total =
                        (entry.kernel.max(0) as u64).saturating_add(entry.user.max(0) as u64);
                    super::CoreTicks {
                        busy: total.saturating_sub(entry.idle.max(0) as u64),
                        total,
                    }
                })
                .collect(),
        )
    }
}

#[cfg(target_os = "linux")]
mod platform {
    pub(super) fn core_ticks() -> Option<Vec<super::CoreTicks>> {
        super::core_ticks_from_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    // Declared here because libc marks these two deprecated in favour of mach2.
    unsafe extern "C" {
        fn mach_host_self() -> libc::mach_port_t;
        static mach_task_self_: libc::mach_port_t;
    }

    /// User, system, idle and nice ticks per core.
    /// https://developer.apple.com/documentation/kernel/1502854-host_processor_info
    pub(super) fn core_ticks() -> Option<Vec<super::CoreTicks>> {
        static HOST: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
        // SAFETY: no arguments; the process keeps this one host send right.
        let host = *HOST.get_or_init(|| unsafe { mach_host_self() });
        let mut cores: libc::natural_t = 0;
        let mut info: libc::processor_info_array_t = std::ptr::null_mut();
        let mut words: libc::mach_msg_type_number_t = 0;
        // SAFETY: the out-pointers are live locals. On success the kernel hands
        // back `words` integers, which are read and then deallocated here.
        unsafe {
            if libc::host_processor_info(
                host,
                libc::PROCESSOR_CPU_LOAD_INFO,
                &mut cores,
                &mut info,
                &mut words,
            ) != libc::KERN_SUCCESS
                || info.is_null()
            {
                return None;
            }
            let ticks = std::slice::from_raw_parts(info, words as usize);
            let state = |core: &[libc::integer_t], index: libc::c_int| {
                u64::from(core[index as usize] as u32)
            };
            let loads = ticks
                .chunks_exact(libc::CPU_STATE_MAX as usize)
                .take(cores as usize)
                .map(|core| {
                    let busy = state(core, libc::CPU_STATE_USER)
                        + state(core, libc::CPU_STATE_SYSTEM)
                        + state(core, libc::CPU_STATE_NICE);
                    super::CoreTicks {
                        busy,
                        total: busy + state(core, libc::CPU_STATE_IDLE),
                    }
                })
                .collect();
            libc::vm_deallocate(
                mach_task_self_,
                info as libc::vm_address_t,
                words as usize * std::mem::size_of::<libc::integer_t>(),
            );
            Some(loads)
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    pub(super) fn core_ticks() -> Option<Vec<super::CoreTicks>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreTicks, core_loads, core_ticks_from_proc_stat};

    #[test]
    fn core_load_is_each_cores_busy_share_between_two_samples() {
        let before = [
            CoreTicks {
                busy: 100,
                total: 1_000,
            },
            CoreTicks {
                busy: 0,
                total: 1_000,
            },
        ];
        let after = [
            CoreTicks {
                busy: 175,
                total: 1_100,
            },
            CoreTicks {
                busy: 0,
                total: 1_100,
            },
        ];
        assert_eq!(core_loads(&before, &after), Some(vec![75.0, 0.0]));
        // A core coming or going restarts the comparison instead of misreading it.
        assert_eq!(core_loads(&before, &after[..1]), None);
    }

    #[test]
    fn proc_stat_reads_each_core_and_skips_the_machine_total() {
        let stat = "cpu  10 1 5 170 5 0 0 0 4 0\n\
                    cpu0 6 1 3 85 5 0 0 0 4 0\n\
                    cpu1 4 0 2 94 0 0 0 0 0 0\n\
                    intr 12 3\n";
        assert_eq!(
            core_ticks_from_proc_stat(stat),
            Some(vec![
                CoreTicks {
                    busy: 10,
                    total: 100
                },
                CoreTicks {
                    busy: 6,
                    total: 100
                },
            ])
        );
    }
}
