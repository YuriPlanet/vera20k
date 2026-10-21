//! Borrowed network access to the world's existing local and synchronized
//! queues. Packet/cache/peer state belongs to the process network session.

use super::Simulation;
use super::queued_command::QueuedCommand;
use crate::sim::command::CommandRecord;
use crate::sim::command_queue::NETWORK_LOCAL_MIRROR_LIMIT;

/// The immutable post-Logic values of the frame being exchanged. A resumed
/// wait uses these same values without another Logic or checksum pass.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct NetworkQueueContext {
    pub frame: u32,
    pub checksum: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkQueuePass {
    Dispatch,
    /// Protocol2 off-cadence, and frame zero after its initial send: MainTick
    /// commits but neither ExecuteDoList nor its retirement runs.
    SkipDispatch,
    /// Keep this post-Logic frame suspended while transport/UI service runs.
    Waiting,
}

/// Fatal Wait_For_Players return paths handled by its caller before MainTick
/// resumes the same frame tail (6480A7..648209). These are distinct from a
/// checksum mismatch during the later ExecuteDoList call.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, thiserror::Error,
)]
pub enum NetworkInterruption {
    #[error("network scenario initialization differs")]
    ScenariosDiffer,
    #[error("synchronized network event queue is full")]
    QueueFull,
    #[error("network connection stopped responding")]
    ConnectionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetworkQueueError {
    #[error("local event {index} has no native record")]
    UnencodedLocalEvent { index: usize },
    #[error(
        "packet consumed {count} local events, beyond the admitted mirror allowance {allowance}"
    )]
    MirrorCapacity { count: usize, allowance: usize },
    #[error("packet mirror {index} does not preserve its local event payload")]
    ChangedLocalEvent { index: usize },
}

/// One temporary borrow of the authoritative queues. The transport can inspect
/// a prefix and commit its encoder's result, but cannot keep or replace either
/// queue. Admission and replay recording stay with Simulation's existing owner.
pub struct NetworkCommandQueues<'a> {
    sim: &'a mut Simulation,
}

impl Simulation {
    /// Borrow the existing EventClass queues for a post-Logic network pass.
    /// The caller cannot replace their owners or retain the borrow across a
    /// simulation visit.
    pub fn network_command_queues(&mut self) -> NetworkCommandQueues<'_> {
        NetworkCommandQueues::new(self)
    }
}

impl<'a> NetworkCommandQueues<'a> {
    pub(super) fn new(sim: &'a mut Simulation) -> Self {
        Self { sim }
    }

    pub fn local_len(&self) -> usize {
        self.sim.local_commands.len()
    }

    pub fn max_ahead_frames(&self) -> Option<i32> {
        self.sim.session.network_max_ahead_frames()
    }

    pub fn synchronized_len(&self) -> usize {
        self.sim.pending_commands.native_len()
    }

    pub fn mirror_allowance(&self) -> usize {
        NETWORK_LOCAL_MIRROR_LIMIT.saturating_sub(self.synchronized_len())
    }

    /// Borrow the prefix the native encoder may attempt. A semantic fallback
    /// cannot silently turn into a locally executed network command: its native
    /// producer must be supplied before this transport can send it.
    pub fn local_records(&self) -> Result<Vec<&CommandRecord>, NetworkQueueError> {
        let mut records = Vec::new();
        for (index, entry) in self
            .sim
            .local_commands
            .iter()
            .take(self.mirror_allowance())
            .enumerate()
        {
            match entry.record() {
                Some(record) => records.push(record),
                None if records.is_empty() => {
                    return Err(NetworkQueueError::UnencodedLocalEvent { index });
                }
                // Let the codec consume the valid native prefix. The next
                // pass reports this semantic head without dropping it.
                None => break,
            }
        }
        Ok(records)
    }

    /// Commit only records actually serialized by64B660. Native rewrites the
    /// source's issue header, mirrors it at64BA43, then removes that accepted
    /// OutList prefix at64BD37. Unsent entries retain their original headers.
    /// Validate before removing anything so a broken encoder cannot corrupt
    /// the source queue. Decoder admission uses the separate full DoList cap.
    pub fn commit_local_mirrors(
        &mut self,
        mirrors: &[CommandRecord],
    ) -> Result<(), NetworkQueueError> {
        let allowance = self.mirror_allowance().min(self.local_len());
        if mirrors.len() > allowance {
            return Err(NetworkQueueError::MirrorCapacity {
                count: mirrors.len(),
                allowance,
            });
        }
        for (index, (source, mirror)) in self.sim.local_commands.iter().zip(mirrors).enumerate() {
            let source = source
                .record()
                .ok_or(NetworkQueueError::UnencodedLocalEvent { index })?;
            if source.opcode() != mirror.opcode() || source.payload() != mirror.payload() {
                return Err(NetworkQueueError::ChangedLocalEvent { index });
            }
        }
        let sources: Vec<_> = self.sim.local_commands.take_prefix(mirrors.len()).collect();
        for (source, mirror) in sources.into_iter().zip(mirrors) {
            let QueuedCommand::Native {
                diagnostic_issue_tick,
                ..
            } = source
            else {
                unreachable!("validated the complete prefix before removal")
            };
            let accepted = self.sim.admit_queued_command(
                QueuedCommand::Native {
                    record: mirror.clone(),
                    diagnostic_issue_tick,
                },
                true,
            );
            assert!(
                accepted,
                "the reserved network mirror prefix must fit DoList"
            );
        }
        Ok(())
    }

    /// Decoder64BDD0 admits each expanded event immediately. A false return
    /// preserves the already admitted prefix and lets the decoder report its
    /// native queue-full failure; no rollback or silent eviction is permitted.
    pub fn admit_received(&mut self, record: CommandRecord) -> bool {
        self.sim.admit_native_command_record(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::command::{Command, CommandEnvelope, GameSpeedRecord};
    use crate::sim::command_queue::DoListEvent;

    #[test]
    fn network_commit_preserves_unsent_headers_and_records_only_accepted_mirrors() {
        let mut sim = Simulation::new();
        let records: Vec<_> = [3, 4, 5]
            .into_iter()
            .map(|speed| {
                GameSpeedRecord {
                    house_id: 0,
                    frame: 7,
                    speed,
                }
                .encode()
                .unwrap()
            })
            .collect();
        for record in &records {
            assert!(sim.issue_local_command(CommandEnvelope::synchronized(record.clone(), 9)));
        }
        let mut mirror = records[0].clone();
        mirror.set_issue_header(mirror.opcode(), 2, 25);
        let mut queues = NetworkCommandQueues::new(&mut sim);
        assert_eq!(
            queues.local_records().unwrap(),
            records.iter().collect::<Vec<_>>()
        );
        queues.commit_local_mirrors(&[mirror.clone()]).unwrap();
        assert_eq!(queues.local_len(), 2);
        assert_eq!(queues.synchronized_len(), 1);
        assert_eq!(
            queues.local_records().unwrap(),
            records[1..].iter().collect::<Vec<_>>()
        );
        assert_eq!(
            sim.pending_commands.iter().next().unwrap().record(),
            Some(&mirror)
        );
        assert_eq!(sim.frame_command_admissions.len(), 1);
        assert!(matches!(&sim.frame_command_admissions[0].payload,
            Command::SynchronizedRecord { record } if record == &mirror));
    }

    #[test]
    fn bad_mirror_does_not_remove_the_local_prefix() {
        let mut sim = Simulation::new();
        let record = GameSpeedRecord {
            house_id: 0,
            frame: 7,
            speed: 3,
        }
        .encode()
        .unwrap();
        sim.issue_local_command(CommandEnvelope::synchronized(record.clone(), 9));
        let bad = GameSpeedRecord {
            house_id: 0,
            frame: 25,
            speed: 6,
        }
        .encode()
        .unwrap();
        let mut queues = NetworkCommandQueues::new(&mut sim);
        assert_eq!(
            queues.commit_local_mirrors(&[bad]),
            Err(NetworkQueueError::ChangedLocalEvent { index: 0 })
        );
        assert_eq!(queues.local_records().unwrap(), [&record]);
        assert_eq!(queues.synchronized_len(), 0);
    }

    #[test]
    fn received_records_use_the_full_queue_without_spending_local_mirror_allowance() {
        let mut sim = Simulation::new();
        let record = GameSpeedRecord {
            house_id: 0,
            frame: 7,
            speed: 3,
        }
        .encode()
        .unwrap();
        let mut queues = NetworkCommandQueues::new(&mut sim);
        for _ in 0..crate::sim::command_queue::DO_LIST_CAPACITY {
            assert!(queues.admit_received(record.clone()));
        }
        assert_eq!(queues.mirror_allowance(), 0);
        assert!(!queues.admit_received(record));
        assert_eq!(
            queues.synchronized_len(),
            crate::sim::command_queue::DO_LIST_CAPACITY
        );
        assert!(
            sim.pending_commands
                .iter()
                .all(|entry| !entry.is_processed())
        );
    }
}

#[cfg(test)]
#[path = "network_wait_tests.rs"]
mod network_wait_tests;
