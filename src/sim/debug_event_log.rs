//! Per-entity debug event log for inspecting movement and state machine history.
//!
//! Gated behind `Option<DebugEventLog>` on `GameEntity` — zero overhead when off.
//! Not included in state hashing or replay (debug-only infrastructure).
//!
//! ## Dependency rules
//! - Part of sim/ — no render/ui/sidebar/audio/net dependencies.

use std::collections::VecDeque;

/// Maximum events retained per entity before oldest are evicted.
const DEBUG_EVENT_LOG_CAPACITY: usize = 64;

#[derive(Clone, Debug)]
pub struct DebugEventLog {
    pub events: VecDeque<DebugEvent>,
}

#[derive(Clone, Debug)]
pub struct DebugEvent {
    pub tick: u32,
    pub kind: DebugEventKind,
}

#[derive(Clone, Debug)]
pub enum DebugEventKind {
    // -- Movement --
    /// Locomotor phase changed (e.g., Idle → Accelerating).
    PhaseChange {
        from: String,
        to: String,
        reason: String,
    },
    /// A* repath was triggered.
    Repath { reason: String, new_path_len: usize },
    /// Movement blocked at a cell.
    Blocked {
        by_entity: Option<u64>,
        cell: (u16, u16),
    },
    /// Stuck abort — path_stuck_counter exhausted or safety timeout.
    StuckAbort { blocked_ticks: u16 },
    /// Path segment (24 steps) completed, repathing toward final goal.
    PathSegmentComplete { final_goal: Option<(u16, u16)> },

    // -- Miner --
    /// Miner high-level state changed (e.g., SearchOre → MoveToOre).
    MinerStateChange { from: String, to: String },

    // -- Special movement --
    /// Special movement system activated (Teleport/Tunnel/Rocket/DropPod).
    SpecialMovementStart { kind: String },
    /// Phase transition within a special movement system.
    SpecialMovementPhase { phase: String },
    /// Special movement completed — returned to base locomotor.
    SpecialMovementEnd,

    // -- Locomotor override --
    /// Piggyback locomotor override started/ended (chrono miner, droppod).
    LocomotorOverride { kind: String, active: bool },
}

impl Default for DebugEventLog {
    fn default() -> Self {
        Self {
            events: VecDeque::with_capacity(DEBUG_EVENT_LOG_CAPACITY),
        }
    }
}

impl DebugEventLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push an event, evicting the oldest if at capacity.
    pub fn push(&mut self, tick: u32, kind: DebugEventKind) {
        if self.events.len() >= DEBUG_EVENT_LOG_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(DebugEvent { tick, kind });
    }
}
