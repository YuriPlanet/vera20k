//! The processors this process may run on: its affinity mask. Windows only.
//!
//! Launchers made for the original game pin it to one processor. The CnCNet
//! client, which mod clients such as Mental Omega's build on, sets the game's
//! `ProcessorAffinity` to 2, the second logical processor, right after it
//! starts it, unless the chosen renderer turns `SingleCoreAffinity` off; it
//! is on by default (`ClientGUI/GameProcessLogic.cs` and
//! `DXMainClient/Domain/DirectDrawWrapper.cs`,
//! <https://github.com/CnCNet/xna-cncnet-client>). VERA20K's own client
//! profile turns it off (`integrations/cncnet/profile/Renderers.ini`).
//!
//! gamemd.exe runs its game on one thread. VERA20K runs the simulation thread,
//! the frame hand-off thread and a worker pool beside the main thread, and
//! Rust sizes that pool by the processors Windows has (`GetSystemInfo`), not
//! by the mask. Pinned, all of them share one processor: the game uses one
//! core while the others idle. [`undo_single_core_pin`] gives the process
//! the system's processors back.

/// The logical processors this process may run on, then the system's: a
/// launcher, a tool or a compatibility setting can leave a game fewer than
/// the PC has. Windows only; `None` elsewhere.
pub(crate) fn allowed_cores() -> Option<(u32, u32)> {
    #[cfg(windows)]
    return platform::masks().map(|(process, system)| (process.count_ones(), system.count_ones()));
    #[cfg(not(windows))]
    None
}

/// Run on every processor the system has when this process is pinned to
/// one of them, as a launcher pins the original game. Only the first call
/// does anything: the first match start, long after a launcher that starts
/// the game set its pin. A restriction to several processors stays, as does
/// one set later, in Task Manager say. Windows only.
pub(crate) fn undo_single_core_pin() {
    #[cfg(windows)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let Some(mask) =
                platform::masks().and_then(|(process, system)| unpinned_mask(process, system))
            else {
                return;
            };
            let processors = mask.count_ones();
            if platform::set_mask(mask) {
                log::info!(
                    "The program that started VERA20K pinned it to one of {processors} \
                     processors, as launchers do for the original game; it now runs on all \
                     {processors}"
                );
            } else {
                log::warn!(
                    "The program that started VERA20K pinned it to one of {processors} \
                     processors, and Windows refused to undo that"
                );
            }
        });
    }
}

/// The mask for a process pinned to one processor: the system's, when the
/// system has more than that one. `None` for every other mask.
#[cfg(any(windows, test))]
fn unpinned_mask(process: usize, system: usize) -> Option<usize> {
    (process.count_ones() == 1 && process & system == process && system.count_ones() > 1)
        .then_some(system)
}

#[cfg(windows)]
mod platform {
    //! A local ABI declaration, as in `app::process_stats`, instead of another
    //! windows-sys feature group.

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessAffinityMask(
            process: isize,
            process_mask: *mut usize,
            system_mask: *mut usize,
        ) -> i32;
        fn SetProcessAffinityMask(process: isize, process_mask: usize) -> i32;
    }

    /// The process's affinity mask and the system's. A process in several
    /// processor groups reads zero masks: no answer.
    /// https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getprocessaffinitymask
    pub(super) fn masks() -> Option<(usize, usize)> {
        let (mut process, mut system) = (0usize, 0usize);
        // SAFETY: the pseudo-handle needs no closing, and both out-pointers
        // are live locals of DWORD_PTR's size.
        let read =
            unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut process, &mut system) };
        (read != 0 && process != 0 && system != 0).then_some((process, system))
    }

    /// Set the mask for every thread of the process, those running already
    /// included. `mask` must be a subset of the system's.
    /// https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setprocessaffinitymask
    pub(super) fn set_mask(mask: usize) -> bool {
        // SAFETY: the pseudo-handle carries PROCESS_SET_INFORMATION and needs
        // no closing; the mask is passed by value.
        unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask) != 0 }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::unpinned_mask;

    #[test]
    fn only_a_pin_to_one_processor_is_undone() {
        // The CnCNet client's pin to the second of eight processors.
        assert_eq!(unpinned_mask(0b10, 0xFF), Some(0xFF));
        // Several processors, a one-processor PC and no restriction stay.
        assert_eq!(unpinned_mask(0x0F, 0xFF), None);
        assert_eq!(unpinned_mask(0b1, 0b1), None);
        assert_eq!(unpinned_mask(0xFF, 0xFF), None);
    }
}
