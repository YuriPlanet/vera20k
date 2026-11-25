//! Per-frame budgets: how many commands may leave, how many objects may be
//! stepped, and where the rotation starts.
//!
//! A house can own twenty thousand objects. Without a budget the frame would
//! spend itself on orders that are never drained, and the host's own per-frame
//! cap (kept from the previous design, `docs/autoplay.md`) would silently drop
//! the tail. So the budget is taken *before* a command is built, not after: an
//! issuer that gets no slot learns it was rejected instead of believing an
//! order it never issued.
//!
//! The rotation cursor makes the micro table fair without a queue: each frame
//! starts where the last one stopped, so every attached object is stepped
//! eventually and the per-frame cost is flat.

/// One frame's allowance.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    max_commands: usize,
    commands: usize,
    max_micro_steps: usize,
    /// The id the micro rotation starts at this frame.
    rotation_start: u64,
}

impl Budget {
    /// Attached objects stepped per frame. Large enough that a few dozen
    /// managed units are stepped every frame, small enough that a house with
    /// thousands attached costs a bounded slice of the frame.
    pub const DEFAULT_MICRO_STEPS: usize = 64;

    pub fn new(max_commands_per_frame: usize) -> Self {
        Self {
            max_commands: max_commands_per_frame,
            commands: 0,
            max_micro_steps: Self::DEFAULT_MICRO_STEPS,
            rotation_start: 0,
        }
    }

    /// Start a frame: the command allowance refills, the rotation does not
    /// rewind.
    pub fn begin_frame(&mut self) {
        self.commands = 0;
    }

    /// Take one command slot. `false` means this frame's allowance is spent.
    pub fn try_command(&mut self) -> bool {
        if self.commands >= self.max_commands {
            return false;
        }
        self.commands += 1;
        true
    }

    pub fn max_commands(&self) -> usize {
        self.max_commands
    }

    /// How many attached objects this frame may step.
    pub fn micro_steps(&self) -> usize {
        self.max_micro_steps
    }

    /// Where this frame's micro rotation starts.
    pub fn rotation_start(&self) -> u64 {
        self.rotation_start
    }

    /// Where the next frame's rotation starts: the id after the last one
    /// stepped, so no object starves.
    pub fn set_rotation_start(&mut self, id: u64) {
        self.rotation_start = id;
    }
}
