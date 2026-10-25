//! The world's stored command representation. Native headers remain bytes;
//! semantic commands are retained only until their native producer is ported.

use crate::sim::command::{Command, CommandEnvelope, CommandRecord};
use crate::sim::command_queue::LocalCommandEvent;
use crate::sim::command_queue::{DoListEvent, SemanticEventHeader};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum QueuedCommand {
    Native {
        record: CommandRecord,
        /// Original Rust issue ordinal for diagnostic views only. The native
        /// Event+3 frame, never this value, decides dispatch and retirement.
        diagnostic_issue_tick: u64,
    },
    Semantic {
        envelope: CommandEnvelope,
        processed: bool,
    },
}

impl QueuedCommand {
    /// Unwrap the compatibility carrier before inspecting its outer owner or
    /// tick. They must never replace any part of the native EventClass header.
    pub(super) fn from_envelope(envelope: CommandEnvelope) -> Self {
        match envelope {
            CommandEnvelope {
                execute_tick,
                payload: Command::SynchronizedRecord { record },
                ..
            } => Self::Native {
                record,
                diagnostic_issue_tick: execute_tick,
            },
            envelope => Self::Semantic {
                envelope,
                processed: false,
            },
        }
    }

    pub(super) fn semantic(&self) -> Option<&CommandEnvelope> {
        match self {
            Self::Semantic { envelope, .. } => Some(envelope),
            Self::Native { .. } => None,
        }
    }

    pub(super) fn record(&self) -> Option<&CommandRecord> {
        match self {
            Self::Native { record, .. } => Some(record),
            Self::Semantic { .. } => None,
        }
    }

    pub(super) fn record_mut(&mut self) -> Option<&mut CommandRecord> {
        match self {
            Self::Native { record, .. } => Some(record),
            Self::Semantic { .. } => None,
        }
    }

    /// Lossless replay transport, including unknown opcodes, future frames,
    /// invalid actors and opaque flags. No native record is decoded here.
    pub(super) fn replay_envelope(&self) -> CommandEnvelope {
        match self {
            Self::Native {
                record,
                diagnostic_issue_tick,
            } => CommandEnvelope::synchronized(record.clone(), *diagnostic_issue_tick),
            Self::Semantic { envelope, .. } => envelope.clone(),
        }
    }

    pub(super) fn diagnostic_envelope(&self, sim: &super::Simulation) -> CommandEnvelope {
        match self {
            Self::Native {
                record,
                diagnostic_issue_tick,
            } => sim
                .decode_native_command_record(record, *diagnostic_issue_tick)
                .unwrap_or_else(|| self.replay_envelope()),
            Self::Semantic { envelope, .. } => envelope.clone(),
        }
    }

    /// Project a due gameplay command without replacing its saved metadata.
    /// Execute_DoList64C563/64C579 reads the signed native frame before the
    /// network late-event gate; the carrier's diagnostic ordinal is unrelated.
    pub(super) fn projected_envelope(&self, sim: &super::Simulation) -> Option<CommandEnvelope> {
        if self.is_processed() {
            return None;
        }
        let execute_tick = sim.session.tick().saturating_add(1);
        match self {
            Self::Native { record, .. } => {
                let native_frame = sim.session.binary_frame() as i32;
                if record.frame_stamp() > native_frame
                    || (record.frame_stamp() < native_frame
                        && sim.session.game_mode().command_queue_route()
                            == super::CommandQueueRoute::Multiplayer
                        && record.opcode() != 0x1c)
                {
                    return None;
                }
                sim.decode_native_command_record(record, execute_tick)
            }
            Self::Semantic { envelope, .. } => {
                (envelope.execute_tick <= execute_tick).then(|| envelope.clone())
            }
        }
    }
}

impl LocalCommandEvent for QueuedCommand {
    fn consumes_native_outlist_slot(&self) -> bool {
        match self {
            Self::Native { .. } => true,
            Self::Semantic { envelope, .. } => envelope.consumes_native_outlist_slot(),
        }
    }
}

impl DoListEvent for QueuedCommand {
    fn native_record(&self) -> Option<&CommandRecord> {
        self.record()
    }

    fn native_record_mut(&mut self) -> Option<&mut CommandRecord> {
        self.record_mut()
    }

    fn semantic_header(&self) -> Option<SemanticEventHeader> {
        let envelope = self.semantic()?;
        let opcode = match &envelope.payload {
            Command::PlanningRecord { record } => Some(record.opcode()),
            Command::OpenOptions => Some(crate::sim::command::OPTIONS_OPCODE),
            command if command.megamission_actor().is_some() => {
                Some(crate::sim::command::MEGAMISSION_OPCODE)
            }
            _ => None,
        };
        Some(SemanticEventHeader {
            owner: envelope.owner,
            execute_tick: envelope.execute_tick,
            opcode,
        })
    }

    fn consumes_native_dolist_slot(&self) -> bool {
        self.consumes_native_outlist_slot()
    }

    fn is_processed(&self) -> bool {
        match self {
            Self::Native { record, .. } => record.is_processed(),
            Self::Semantic { processed, .. } => *processed,
        }
    }

    fn mark_processed(&mut self) {
        match self {
            Self::Native { record, .. } => record.mark_processed(),
            Self::Semantic { processed, .. } => *processed = true,
        }
    }

    fn clear_processed(&mut self) {
        match self {
            Self::Native { record, .. } => record.clear_processed(),
            Self::Semantic { processed, .. } => *processed = false,
        }
    }
}
