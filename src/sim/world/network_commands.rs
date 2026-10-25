//! Borrowed network access to the world's existing local and synchronized
//! queues. Packet/cache/peer state belongs to the process network session.

use super::Simulation;
use super::queued_command::QueuedCommand;
use crate::sim::command::CommandRecord;
use crate::sim::command_queue::{
    NETWORK_COMMAND_CAPACITY, NETWORK_LOCAL_MIRROR_LIMIT, NETWORK_SOURCE_DOLIST_CAPACITY,
};

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
    /// SessionClass::Am_I_Master697E70, called by ProcessNetwork647B8A.
    /// LAN697ECD..697F0D selects the first human, undefeated House. WOL's
    /// explicit publisher697E81..697ECA survives ordinary defeat/Observe;
    /// DestroyConnection5DAA48..5DAA77 replaces it only after source loss.
    /// Source: original gamemd.exe instructions at those callers.
    ///
    /// The relay has no native direct-peer nomination664440. Its replacement
    /// is the first remaining admitted human in shared House order, after
    /// synchronized EXIT/REMOVEPLAYER retires the old timing row. Neither a
    /// local peer list nor a second cached host participates in this decision.
    pub fn network_timing_host_house(&self) -> Option<i8> {
        let timing = self.session.network_command_timing()?;
        let wol = self.session.game_mode() == crate::sim::scenario_session::ScenarioGameMode::Wol;
        let eligible = |house_id: i8| {
            timing.admits_source(house_id)
                && usize::try_from(house_id)
                    .ok()
                    .and_then(|index| self.houses.order().get(index))
                    .and_then(|owner| self.houses.get(owner))
                    .is_some_and(|house| {
                        house.is_human() && !house.left_game() && (wol || !house.is_defeated())
                    })
        };
        if wol && eligible(timing.host_house()) {
            return Some(timing.host_house());
        }
        self.houses
            .order()
            .iter()
            .enumerate()
            .filter_map(|(index, _)| i8::try_from(index).ok())
            .find(|&house| eligible(house))
    }

    /// Borrow the existing EventClass queues for a post-Logic network pass.
    /// The caller cannot replace their owners or retain the borrow across a
    /// simulation visit.
    pub fn network_command_queues(&mut self) -> NetworkCommandQueues<'_> {
        NetworkCommandQueues::new(self)
    }

    pub fn synchronized_network_command_count(&self) -> usize {
        self.pending_commands.native_len()
    }

    /// Shared read-only admission fact for the sender and diagnostics. Reading
    /// this allowance neither reserves mirrors nor changes either command queue.
    pub fn network_mirror_allowance(&self) -> usize {
        let synchronized = self.synchronized_network_command_count();
        if self.session.game_mode().is_network() {
            let source_house = self.local_command_house().unwrap_or(0);
            NETWORK_COMMAND_CAPACITY.saturating_sub(synchronized).min(
                NETWORK_SOURCE_DOLIST_CAPACITY
                    .saturating_sub(self.pending_commands.native_house_len(source_house)),
            )
        } else {
            NETWORK_LOCAL_MIRROR_LIMIT.saturating_sub(synchronized)
        }
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

    pub fn timing(&self) -> Option<&crate::sim::command_queue::NetworkCommandTiming> {
        self.sim.session.network_command_timing()
    }

    /// The same canonical publisher used when accepting EVENT_TIMING.
    pub fn timing_host_house(&self) -> Option<i8> {
        self.sim.network_timing_host_house()
    }

    pub fn install_timing(
        &mut self,
        host_house: i8,
        frame_send_rate: std::num::NonZeroU8,
        houses: impl IntoIterator<Item = i8>,
    ) {
        self.sim
            .session
            .install_network_command_timing(host_house, frame_send_rate, houses);
        // Main__PrepareSession52DAC7 resets the prior match's retime window.
        self.sim.pending_commands.set_retime_window(0, 0);
    }

    pub fn game_speed(&self) -> i32 {
        self.sim.session.game_options().game_speed
    }

    /// Modern relay policy: freeze the lobby-certified cadence before the
    /// initial world agreement. Native LAN and solo online keep their existing
    /// TIMING/PROCESS_TIME path; no later local process sample changes this clock.
    pub fn configure_sequenced_clock(&mut self, frames_per_second: u16) {
        self.sim
            .session
            .configure_sequenced_clock(frames_per_second);
    }
    pub fn fog_of_war(&self) -> bool {
        self.sim.session.game_options().fog_of_war
    }

    /// ProcessNetwork647767 / 647DB3 / 647E85 append into the same OutList as
    /// player commands. Capacity refusal never creates a second pending queue.
    pub fn issue_timing_record(&mut self, record: CommandRecord) -> bool {
        self.sim
            .issue_local_command(crate::sim::command::CommandEnvelope::synchronized(
                record,
                self.sim.session.tick(),
            ))
    }

    pub fn synchronized_len(&self) -> usize {
        self.sim.synchronized_network_command_count()
    }

    pub fn mirror_allowance(&self) -> usize {
        self.sim.network_mirror_allowance()
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
            .take(
                self.mirror_allowance()
                    .min(crate::sim::command_queue::OUT_LIST_CAPACITY),
            )
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
        self.validate_local_prefix(mirrors)?;
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

    /// VERA sequenced lockstep: an acknowledged submission leaves OutList,
    /// but is not a local mirror. Only its immutable relay commit may enter
    /// DoList, exactly as for every other sender. The retained transport batch
    /// is an unacknowledged prefix, never a second simulation command queue.
    pub fn commit_local_submission(
        &mut self,
        records: &[CommandRecord],
    ) -> Result<(), NetworkQueueError> {
        self.validate_local_prefix(records)?;
        self.sim
            .local_commands
            .take_prefix(records.len())
            .for_each(drop);
        Ok(())
    }

    fn validate_local_prefix(&self, mirrors: &[CommandRecord]) -> Result<(), NetworkQueueError> {
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
    use crate::sim::command::{
        ArchiveRecord, Command, CommandEnvelope, GameSpeedRecord, MegaMissionTarget,
    };
    use crate::sim::command_queue::DoListEvent;

    /// Building4439C5/44367B uses each actor's House; ARCHIVE4C6780
    /// occupies one OutList slot per selected producer, in click order.
    #[test]
    fn local_rally_records_preserve_each_actor_and_the_accepted_prefix() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x443860);
        sim.resolve_type_handles(rules);
        let local = sim.interner.get("Americans").unwrap();
        let foreign = sim.interner.get("Russians").unwrap();
        let local_index = sim.houses.order().iter().position(|h| *h == local).unwrap();
        let foreign_index = sim
            .houses
            .order()
            .iter()
            .position(|h| *h == foreign)
            .unwrap();
        let first = sim
            .spawn_object("GAPILE", "Americans", 8, 8, 0, rules)
            .unwrap();
        let second = sim
            .spawn_object("GAPILE", "Russians", 20, 8, 0, rules)
            .unwrap();
        sim.session.set_binary_frame_for_test(73);
        assert!(sim.issue_local_command(CommandEnvelope::new(
            local,
            41,
            Command::SetRally {
                rx: 13,
                ry: 14,
                producer_ids: vec![second, first],
            }
        )));
        {
            let queues = sim.network_command_queues();
            let records = queues
                .local_records()
                .expect("rally must be a native wire event");
            assert_eq!(records.len(), 2);
            for (record, actor, owner_index) in [
                (records[0], second, foreign_index),
                (records[1], first, local_index),
            ] {
                assert_eq!(record.opcode(), 0x1e);
                assert_eq!(record.frame_stamp(), 73);
                let event = ArchiveRecord::decode(record).unwrap();
                assert_eq!(event.entity_id, actor as i32);
                assert_eq!(event.target, MegaMissionTarget::Cell { x: 13, y: 14 });
                assert_eq!(record.house_id() as usize, owner_index);
            }
        }
        sim.take_frame_commands_for_test();
        let stop = sim.encode_stop_record(local, first).unwrap();
        for _ in 0..127 {
            assert!(sim.issue_local_command(CommandEnvelope::synchronized(stop.clone(), 41)));
        }
        assert!(!sim.issue_local_command(CommandEnvelope::new(
            local,
            41,
            Command::SetRally {
                rx: 15,
                ry: 16,
                producer_ids: vec![second, first],
            }
        )));
        let queues = sim.network_command_queues();
        let records = queues.local_records().unwrap();
        assert_eq!(records.len(), 128);
        assert_eq!(records[127].opcode(), 0x1e);
        assert_eq!(
            ArchiveRecord::decode(records[127]).unwrap().entity_id,
            second as i32
        );
    }

    #[test]
    fn network_commit_preserves_unsent_headers_and_records_only_accepted_mirrors() {
        let mut sim = Simulation::new();
        let initial_allowance = sim.network_mirror_allowance();
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
        assert_eq!(sim.synchronized_network_command_count(), 1);
        assert_eq!(sim.network_mirror_allowance(), initial_allowance - 1);
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
