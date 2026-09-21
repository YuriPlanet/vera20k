//! The EVA announcement queue — gamemd's `VoxClass`.
//!
//! Pure decision state; the stream itself lives in [`crate::audio::sfx`].
//! Native storage this mirrors (all bodies read for this module):
//!
//! | native | here |
//! |---|---|
//! | interrupt list `0xB1D3C8` (`Type=QUEUED_INTERRUPT` nodes) | `interrupt_list` |
//! | critical list `0xB1D3F0` (`Priority=CRITICAL` nodes) | `critical_list` |
//! | pending slot `0xB1D4B8` (one STANDARD/INTERRUPT node) | `pending` |
//! | four `Type=QUEUE` FIFOs `0xB1D450 + priority * 0xC` | `priority_lists` |
//! | current entry `0xB1D4C4` (+ type `0xB1D3B8`, priority `0xB1D3E0`) | `current` |
//! | node sequence counter `0xB1D4C0` (`% 100`) | `seq_counter` |
//! | 64-bit gap after the stream's end time `0xB1D4D0/0xB1D4D4` | `gap_ms` |
//! | pause depth `0xB1D428` (`PauseEVA @ 0x007535B0` / `UnpauseEVA @ 0x00753620`) | `pause_depth` |
//! | suspend depth `0xB1D3D8` (`SuspendEVA @ 0x00753570` / `ResumeEVA @ 0x00753580`) | `suspend_depth` |
//!
//! Functions: `VoxClass::QueueVoice @ 0x00752480` ([`VoxQueue::queue_voice`]),
//! `InsertIntoQueue @ 0x00752590`, `FindInQueues @ 0x00752680`,
//! `PlayNextQueued @ 0x00752760` ([`VoxQueue::take_next`] + [`VoxQueue::started`]),
//! `ClearAllQueues @ 0x00752370`, `ResetAll @ 0x007535D0`,
//! `PumpAndCheckActive @ 0x007529E0` ([`VoxQueue::is_active`]).

use std::collections::VecDeque;

use crate::rules::sound_ini::{EvaPriority, EvaType};

/// `VoxClass::PlayNextQueued @ 0x00752760`: `0x0075296D MOV [0xB1D4D0],0x1F4`
/// — the gap installed after a line starts successfully; the next line may
/// start only once `now > end_time + gap` (unsigned 64-bit compare at
/// `0x007527A1..0x007527CF`).
pub const INTER_LINE_GAP_MS: u64 = 500;

/// One queue node (native `operator_new(0x20)`: `+0x0C` entry, `+0x14`
/// priority, `+0x18` type, `+0x1C` sequence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxNode {
    /// Entry identity: the `[DialogList]` name, upper-cased for the
    /// `stricmp` match `VoxClass::PlayEVA` performs.
    pub event: String,
    /// The side column already resolved for this session (native resolves it
    /// at play time from `0xB1D4C8`; the side is fixed per session by
    /// `VoxClass::SetSide`, so resolving at queue time is equivalent).
    pub sample: String,
    pub eva_type: EvaType,
    pub priority: EvaPriority,
    /// `DAT_00B1D4C0 % 100` at insertion; bookkeeping only.
    pub seq: u32,
}

/// What `VoxClass::QueueVoice` asks the stream layer to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QueueEffect {
    /// `StreamPlayer::Stop` was called (type 2 while a line was current).
    pub stop_stream: bool,
    /// The request was inserted into a queue or the pending slot.
    pub inserted: bool,
}

/// One announcement request as `VoxClass::PlayEVA` resolves it.
#[derive(Debug, Clone, Copy)]
pub struct VoxRequest<'a> {
    pub event: &'a str,
    pub sample: &'a str,
    pub eva_type: EvaType,
    pub priority: EvaPriority,
}

/// `VoxClass` queue state.
#[derive(Debug, Default)]
pub struct VoxQueue {
    interrupt_list: VecDeque<VoxNode>,
    critical_list: VecDeque<VoxNode>,
    pending: Option<VoxNode>,
    priority_lists: [VecDeque<VoxNode>; 4],
    current: Option<VoxNode>,
    seq_counter: u32,
    gap_ms: u64,
    /// Stand-in for `StreamPlayer::GetEndTime` (`0x00408140`): the moment the
    /// last line was observed to end, reported by the stream owner.
    end_time_ms: u64,
    pause_depth: i32,
    suspend_depth: i32,
}

impl VoxQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// `VoxClass::QueueVoice @ 0x00752480`.
    ///
    /// Guards (one `if`, `0x00752480..`): the stream player exists, the index
    /// is valid, `DAT_00B1D3D8 == 0` (not suspended) and the entry is not the
    /// current one (`iVar1 != DAT_00B1D4C4`). A `-1` type/priority takes the
    /// entry's own; `PlayEVA` always passes priority `-1`, so only the type
    /// can be overridden per call site.
    ///
    /// Type 2 (INTERRUPT) **while an entry is current**: every interrupt-list
    /// node is dropped, the current entry is marked done, `StreamPlayer::Stop`,
    /// `ClearAllQueues`, and the 64-bit gap is zeroed. With no current entry
    /// a type-2 line is routed like STANDARD and existing nodes survive.
    ///
    /// Then `FindInQueues`: a node for this entry **with the same type** is a
    /// duplicate and the request is dropped (`0x00752566`); otherwise
    /// `InsertIntoQueue`. The caller runs `PlayNextQueued` afterwards.
    pub fn queue_voice(
        &mut self,
        request: VoxRequest<'_>,
        type_override: Option<EvaType>,
    ) -> QueueEffect {
        let mut effect = QueueEffect::default();
        let event = request.event.to_ascii_uppercase();
        if self.suspend_depth != 0 {
            return effect;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.event == event)
        {
            return effect;
        }
        let eva_type = type_override.unwrap_or(request.eva_type);
        let priority = request.priority;

        if self.current.is_some() && eva_type == EvaType::Interrupt {
            self.interrupt_list.clear();
            self.current = None;
            effect.stop_stream = true;
            self.clear_all_queues();
            self.gap_ms = 0;
            // VERA assumption: after `StreamPlayer::Stop` the stream's end
            // time is not in the future, so the interrupt line starts on the
            // `PlayNextQueued` that follows. gamemd `GetEndTime`-after-`Stop`
            // UNCHECKED.
            self.end_time_ms = 0;
        }

        let duplicate = self
            .find_in_queues(&event)
            .is_some_and(|node| node.eva_type == eva_type);
        if !duplicate {
            effect.inserted = self.insert_into_queue(VoxNode {
                event,
                sample: request.sample.to_string(),
                eva_type,
                priority,
                seq: self.seq_counter % 100,
            });
        }
        effect
    }

    /// `VoxClass::InsertIntoQueue @ 0x00752590`. Returns whether the node was
    /// kept.
    ///
    /// `0x007525CF CMP EDI,3` → interrupt list `0xB1D3C8`;
    /// `0x007525F4 CMP EDI,1` → `0xB1D450 + priority*0xC`;
    /// `0x00752611 CMP ECX,3` (priority CRITICAL) → critical list `0xB1D3F0`;
    /// else the pending slot `0xB1D4B8`, taken only if both the interrupt and
    /// critical lists are empty (`0x00752629..0x0075263F`) and the slot is
    /// empty or holds a **strictly lower** priority (`0x0075264A CMP
    /// [EAX+0x14],ECX ; JGE discard`). Otherwise the new node is discarded
    /// (entry state 2).
    fn insert_into_queue(&mut self, node: VoxNode) -> bool {
        self.seq_counter = self.seq_counter.wrapping_add(1);
        match node.eva_type {
            EvaType::QueuedInterrupt => {
                self.interrupt_list.push_back(node);
                true
            }
            EvaType::Queue => {
                self.priority_lists[node.priority.list_index()].push_back(node);
                true
            }
            _ if node.priority == EvaPriority::Critical => {
                self.critical_list.push_back(node);
                true
            }
            _ => {
                let slot_free = self.interrupt_list.is_empty()
                    && self.critical_list.is_empty()
                    && self
                        .pending
                        .as_ref()
                        .is_none_or(|pending| pending.priority < node.priority);
                if slot_free {
                    self.pending = Some(node);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// `VoxClass::FindInQueues @ 0x00752680`: critical list, pending slot, the
    /// four priority lists (`0xB1D474` down to `0xB1D450`), interrupt list.
    fn find_in_queues(&self, event: &str) -> Option<&VoxNode> {
        self.critical_list
            .iter()
            .find(|node| node.event == event)
            .or_else(|| self.pending.as_ref().filter(|node| node.event == event))
            .or_else(|| {
                self.priority_lists
                    .iter()
                    .rev()
                    .find_map(|list| list.iter().find(|node| node.event == event))
            })
            .or_else(|| self.interrupt_list.iter().find(|node| node.event == event))
    }

    /// The dequeue half of `VoxClass::PlayNextQueued @ 0x00752760`.
    ///
    /// Gate: `StreamPlayer::IsPlaying() == 0` (`0x00752794`), `now >
    /// end_time + gap` (`0x007527A1..0x007527CF`), `DAT_00B1D428 == 0`
    /// (`0x007527D5`). Then the current entry is marked done
    /// (`0x007527E2..0x007527F2`) and one node is taken in this order:
    /// interrupt list (`0x007527F8`) → critical list (`0x00752835`) → pending
    /// slot (`0x00752866`) → priority lists CRITICAL..LOW (`0x00752878`,
    /// `0xB1D474` stepping `-0xC` to `0xB1D450`). Taking an interrupt or
    /// critical node **frees the pending slot** (`0x00752824`/`0x00752855`).
    ///
    /// The caller tries to start the returned node and reports success with
    /// [`Self::started`]; a failed `PlayFile` leaves nothing current and the
    /// node is simply gone, as at `0x00752963 JZ`.
    pub fn take_next(&mut self, now_ms: u64, stream_playing: bool) -> Option<VoxNode> {
        if stream_playing {
            return None;
        }
        if now_ms <= self.end_time_ms.saturating_add(self.gap_ms) {
            return None;
        }
        if self.pause_depth != 0 {
            return None;
        }
        self.current = None;
        if let Some(node) = self.interrupt_list.pop_front() {
            self.pending = None;
            return Some(node);
        }
        if let Some(node) = self.critical_list.pop_front() {
            self.pending = None;
            return Some(node);
        }
        if let Some(node) = self.pending.take() {
            return Some(node);
        }
        self.priority_lists
            .iter_mut()
            .rev()
            .find_map(|list| list.pop_front())
    }

    /// `0x0075296D..0x0075298E`: after `StreamPlayer::PlayFile` succeeded —
    /// gap = 500 ms, the node becomes the current entry (state 0).
    pub fn started(&mut self, node: VoxNode) {
        self.gap_ms = INTER_LINE_GAP_MS;
        self.end_time_ms = u64::MAX - INTER_LINE_GAP_MS;
        self.current = Some(node);
    }

    /// The stream owner observed the current line end at `now_ms`
    /// (`StreamPlayer::GetEndTime` stand-in).
    pub fn stream_ended(&mut self, now_ms: u64) {
        self.end_time_ms = now_ms;
    }

    /// `VoxClass::ClearAllQueues @ 0x00752370`: the pending slot, the
    /// interrupt and critical lists and the four priority lists; the current
    /// entry is untouched.
    pub fn clear_all_queues(&mut self) {
        self.pending = None;
        self.interrupt_list.clear();
        self.critical_list.clear();
        for list in &mut self.priority_lists {
            list.clear();
        }
    }

    /// `VoxClass::ResetAll @ 0x007535D0`: current entry done, `StreamPlayer::Stop`
    /// (the caller's job — returns `true` when a line was current),
    /// `ClearAllQueues`, then `DAT_00B1D428 = 0` and `DAT_00B1D3D8 = 0`.
    pub fn reset_all(&mut self) -> bool {
        let had_current = self.current.take().is_some();
        self.clear_all_queues();
        self.pause_depth = 0;
        self.suspend_depth = 0;
        self.gap_ms = 0;
        self.end_time_ms = 0;
        had_current
    }

    /// `PauseEVA @ 0x007535B0` (`DAT_00B1D428 += 1`) / `UnpauseEVA @
    /// 0x00753620` (`if (d != 0) { d -= 1; if (d < 0) d = 0; }`), one edge
    /// per call. `GamePause::Enter @ 0x00406F00` / `Exit @ 0x00406F40` are
    /// the callers.
    pub fn set_paused(&mut self, paused: bool) {
        if paused {
            self.pause_depth += 1;
        } else if self.pause_depth != 0 {
            self.pause_depth = (self.pause_depth - 1).max(0);
        }
    }

    /// `PlayNextQueued`'s `DAT_00B1D428 == 0` gate.
    #[cfg(test)]
    pub fn dequeue_allowed(&self) -> bool {
        self.pause_depth == 0
    }

    /// `VoxClass::SuspendEVA @ 0x00753570`: `DAT_00B1D3D8 += 1`. Blocks
    /// **queueing** (`QueueVoice` guard), not playback. Stock callers:
    /// `NukeFlash::StartScreenFlash 0x0053B554`, `RadarClass::PlayRadarMovie
    /// 0x006579C0`.
    pub fn suspend(&mut self) {
        self.suspend_depth += 1;
    }

    /// `VoxClass::ResumeEVA @ 0x00753580`: `if (d != 0) { d -= 1; if (d < 0)
    /// d = 0; }`.
    pub fn resume(&mut self) {
        if self.suspend_depth != 0 {
            self.suspend_depth = (self.suspend_depth - 1).max(0);
        }
    }

    /// `VoxClass::PumpAndCheckActive @ 0x007529E0` after its `PlayNextQueued`:
    /// stream playing, or any node in the interrupt/critical lists, the
    /// pending slot or a priority list.
    pub fn is_active(&self, stream_playing: bool) -> bool {
        stream_playing || self.queued_count() > 0
    }

    /// Nodes waiting in any list or the pending slot.
    pub fn queued_count(&self) -> usize {
        self.interrupt_list.len()
            + self.critical_list.len()
            + usize::from(self.pending.is_some())
            + self.priority_lists.iter().map(VecDeque::len).sum::<usize>()
    }

    /// The entry that is current (playing or inside its post-line gap).
    pub fn current(&self) -> Option<&VoxNode> {
        self.current.as_ref()
    }

    #[cfg(test)]
    fn pending_event(&self) -> Option<&str> {
        self.pending.as_ref().map(|node| node.event.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(event: &'static str, eva_type: EvaType, priority: EvaPriority) -> VoxRequest<'static> {
        VoxRequest {
            event,
            sample: event,
            eva_type,
            priority,
        }
    }

    // Stock evamd.ini rows used below (ini/evamd.ini):
    // EVA_UnitLost STANDARD IMPORTANT; EVA_OurBaseIsUnderAttack STANDARD NORMAL;
    // EVA_UnitReady / EVA_ConstructionComplete STANDARD LOW;
    // EVA_LowPower QUEUE IMPORTANT; EVA_NewConstructionOptions QUEUE LOW;
    // EVA_YouHaveLost STANDARD NORMAL; EVA_NuclearMissileLaunched STANDARD CRITICAL;
    // EVA_BattleControlTerminated STANDARD CRITICAL (call-site type 2).
    fn unit_lost() -> VoxRequest<'static> {
        req("EVA_UnitLost", EvaType::Standard, EvaPriority::Important)
    }
    fn base_attack() -> VoxRequest<'static> {
        req(
            "EVA_OurBaseIsUnderAttack",
            EvaType::Standard,
            EvaPriority::Normal,
        )
    }
    fn unit_ready() -> VoxRequest<'static> {
        req("EVA_UnitReady", EvaType::Standard, EvaPriority::Low)
    }
    fn construction_complete() -> VoxRequest<'static> {
        req(
            "EVA_ConstructionComplete",
            EvaType::Standard,
            EvaPriority::Low,
        )
    }
    fn low_power() -> VoxRequest<'static> {
        req("EVA_LowPower", EvaType::Queue, EvaPriority::Important)
    }
    fn new_options() -> VoxRequest<'static> {
        req(
            "EVA_NewConstructionOptions",
            EvaType::Queue,
            EvaPriority::Low,
        )
    }
    fn you_have_lost() -> VoxRequest<'static> {
        req("EVA_YouHaveLost", EvaType::Standard, EvaPriority::Normal)
    }
    fn nuke_launched() -> VoxRequest<'static> {
        req(
            "EVA_NuclearMissileLaunched",
            EvaType::Standard,
            EvaPriority::Critical,
        )
    }

    /// Start whatever is next at `now` and report its event.
    fn play_next(q: &mut VoxQueue, now: u64) -> Option<String> {
        let node = q.take_next(now, false)?;
        let event = node.event.clone();
        q.started(node);
        Some(event)
    }

    /// `InsertIntoQueue 0x0075264A`: the pending slot is replaced only by a
    /// strictly higher priority; equal or lower is discarded.
    #[test]
    fn pending_slot_replacement_is_strict_priority() {
        let mut q = VoxQueue::new();
        // Occupy the stream so requests park in the pending slot.
        q.queue_voice(construction_complete(), None);
        assert_eq!(
            play_next(&mut q, 1).as_deref(),
            Some("EVA_CONSTRUCTIONCOMPLETE")
        );

        assert!(q.queue_voice(base_attack(), None).inserted);
        assert_eq!(q.pending_event(), Some("EVA_OURBASEISUNDERATTACK"));
        // NORMAL pending, IMPORTANT arrives: replaced; the base line never plays.
        assert!(q.queue_voice(unit_lost(), None).inserted);
        assert_eq!(q.pending_event(), Some("EVA_UNITLOST"));
        // IMPORTANT pending, NORMAL arrives: discarded.
        assert!(!q.queue_voice(base_attack(), None).inserted);
        assert_eq!(q.pending_event(), Some("EVA_UNITLOST"));
        // Equal priority from a different entry: `JGE` discards it too.
        assert!(
            !q.queue_voice(
                req(
                    "EVA_OtherImportant",
                    EvaType::Standard,
                    EvaPriority::Important
                ),
                None
            )
            .inserted
        );
        assert_eq!(q.queued_count(), 1);
    }

    /// A STANDARD line arriving while another plays is kept (pending slot),
    /// not dropped, and plays after the gap — the M2.1 case.
    #[test]
    fn a_standard_line_waits_in_the_pending_slot_while_a_line_plays() {
        let mut q = VoxQueue::new();
        q.queue_voice(construction_complete(), None);
        assert_eq!(
            play_next(&mut q, 1).as_deref(),
            Some("EVA_CONSTRUCTIONCOMPLETE")
        );
        assert!(q.queue_voice(unit_ready(), None).inserted);
        assert!(q.take_next(2, true).is_none(), "stream still playing");
        q.stream_ended(1_000);
        assert_eq!(play_next(&mut q, 1_501).as_deref(), Some("EVA_UNITREADY"));
    }

    /// Each `Type=QUEUE` list is FIFO, the lists drain CRITICAL..LOW, and the
    /// pending slot plays before any of them (`PlayNextQueued` order).
    #[test]
    fn queue_lists_are_fifo_per_priority_and_drain_after_the_pending_slot() {
        let mut q = VoxQueue::new();
        q.queue_voice(construction_complete(), None);
        play_next(&mut q, 1);
        // Two LOW queue lines, one IMPORTANT queue line, then a STANDARD one.
        assert!(q.queue_voice(new_options(), None).inserted);
        assert!(
            q.queue_voice(
                req("EVA_OtherLowQueue", EvaType::Queue, EvaPriority::Low),
                None
            )
            .inserted
        );
        assert!(q.queue_voice(low_power(), None).inserted);
        assert!(q.queue_voice(unit_lost(), None).inserted);
        assert_eq!(q.queued_count(), 4);

        let mut order = Vec::new();
        let mut now = 1_000;
        for _ in 0..4 {
            q.stream_ended(now);
            now += INTER_LINE_GAP_MS + 1;
            order.push(play_next(&mut q, now).unwrap());
        }
        assert_eq!(
            order,
            [
                "EVA_UNITLOST",
                "EVA_LOWPOWER",
                "EVA_NEWCONSTRUCTIONOPTIONS",
                "EVA_OTHERLOWQUEUE",
            ]
        );
        assert!(!q.is_active(false));
    }

    /// `QueueVoice 0x00752566`: a queued node for the same entry with the same
    /// type is a duplicate; a different type is not. The current entry is
    /// refused outright (`iVar1 != DAT_00B1D4C4`), even inside its gap.
    #[test]
    fn duplicate_rule_matches_entry_and_type() {
        let mut q = VoxQueue::new();
        q.queue_voice(construction_complete(), None);
        play_next(&mut q, 1);
        assert!(q.queue_voice(low_power(), None).inserted);
        assert!(
            !q.queue_voice(low_power(), None).inserted,
            "same entry, same type"
        );
        assert_eq!(q.queued_count(), 1);
        // Same entry with an overridden type is a second node.
        assert!(
            q.queue_voice(low_power(), Some(EvaType::QueuedInterrupt))
                .inserted
        );
        assert_eq!(q.queued_count(), 2);

        // The current entry is refused while current, including during the gap.
        assert!(!q.queue_voice(construction_complete(), None).inserted);
        q.stream_ended(1_000);
        assert!(q.take_next(1_200, false).is_none());
        assert!(!q.queue_voice(construction_complete(), None).inserted);
    }

    /// "You have lost" (STANDARD NORMAL) arriving while "Unit lost" plays is
    /// kept in the pending slot; a CRITICAL line goes to the critical list,
    /// is never discarded, plays first and frees the pending slot.
    #[test]
    fn outcome_and_critical_lines_are_not_dropped_while_a_line_plays() {
        let mut q = VoxQueue::new();
        q.queue_voice(unit_lost(), None);
        assert_eq!(play_next(&mut q, 1).as_deref(), Some("EVA_UNITLOST"));
        assert!(q.queue_voice(you_have_lost(), None).inserted);
        assert!(q.is_active(true));
        q.stream_ended(3_000);
        assert_eq!(play_next(&mut q, 3_501).as_deref(), Some("EVA_YOUHAVELOST"));

        let mut q = VoxQueue::new();
        q.queue_voice(unit_lost(), None);
        play_next(&mut q, 1);
        assert!(q.queue_voice(base_attack(), None).inserted);
        assert!(q.queue_voice(nuke_launched(), None).inserted);
        // With a critical node waiting, a new STANDARD line cannot take the slot.
        assert!(!q.queue_voice(unit_ready(), None).inserted);
        q.stream_ended(1_000);
        assert_eq!(
            play_next(&mut q, 1_501).as_deref(),
            Some("EVA_NUCLEARMISSILELAUNCHED")
        );
        // `0x00752855`: the pending "base under attack" was freed with it.
        assert_eq!(q.pending_event(), None);
        assert!(!q.is_active(true) || q.queued_count() == 0);
    }

    /// `0x0075296D`: 500 ms after the line's end time before the next starts;
    /// the compare is strict (`JBE` → wait when `now <= end + gap`).
    #[test]
    fn next_line_waits_five_hundred_ms_after_the_previous_ended() {
        let mut q = VoxQueue::new();
        q.queue_voice(construction_complete(), None);
        play_next(&mut q, 1);
        q.queue_voice(unit_ready(), None);
        q.stream_ended(10_000);
        assert!(q.take_next(10_499, false).is_none());
        assert!(q.take_next(10_500, false).is_none(), "equality still waits");
        assert!(q.take_next(10_501, false).is_some());
    }

    /// Type 2 while a line is current: stream stopped, every queue cleared,
    /// gap zeroed, and the interrupt line starts at once. With an idle slot a
    /// type-2 line routes like STANDARD and existing nodes survive.
    #[test]
    fn interrupt_cuts_only_when_a_line_is_current() {
        let mut q = VoxQueue::new();
        q.queue_voice(construction_complete(), None);
        play_next(&mut q, 1);
        q.queue_voice(low_power(), None);
        q.queue_voice(unit_lost(), None);
        let bct = req(
            "EVA_BattleControlTerminated",
            EvaType::Standard,
            EvaPriority::Critical,
        );
        let effect = q.queue_voice(bct, Some(EvaType::Interrupt));
        assert!(effect.stop_stream && effect.inserted);
        assert_eq!(q.queued_count(), 1);
        assert!(q.current().is_none());
        // No gap: the interrupt line starts on the very next pump.
        assert_eq!(
            play_next(&mut q, 2).as_deref(),
            Some("EVA_BATTLECONTROLTERMINATED")
        );

        let mut q = VoxQueue::new();
        q.queue_voice(low_power(), None);
        let effect = q.queue_voice(unit_lost(), Some(EvaType::Interrupt));
        assert!(!effect.stop_stream && effect.inserted);
        assert_eq!(q.queued_count(), 2, "idle slot: nothing is cleared");
        assert_eq!(q.pending_event(), Some("EVA_UNITLOST"));
    }

    /// Pausing stops the queue from advancing until every pause is lifted
    /// (`DAT_00B1D428`, a depth with a floor of 0), and `ResetAll` zeroes it.
    #[test]
    fn pausing_suspends_the_eva_queue_until_every_pause_is_lifted() {
        let mut q = VoxQueue::new();
        q.queue_voice(construction_complete(), None);
        q.set_paused(true);
        q.set_paused(true);
        assert!(q.take_next(1, false).is_none());
        q.set_paused(false);
        assert!(q.take_next(1, false).is_none());
        q.set_paused(false);
        assert!(q.dequeue_allowed());
        // An unmatched resume never drives the counter negative.
        q.set_paused(false);
        q.set_paused(true);
        assert!(!q.dequeue_allowed());

        q.reset_all();
        assert!(q.dequeue_allowed());
        assert_eq!(q.queued_count(), 0);
    }

    /// `SuspendEVA` (`0xB1D3D8`) blocks queueing, not playback.
    #[test]
    fn suspend_blocks_queueing_but_not_dequeue() {
        let mut q = VoxQueue::new();
        q.queue_voice(low_power(), None);
        q.suspend();
        assert!(!q.queue_voice(unit_lost(), None).inserted);
        assert_eq!(play_next(&mut q, 1).as_deref(), Some("EVA_LOWPOWER"));
        q.resume();
        q.resume();
        assert!(q.queue_voice(unit_lost(), None).inserted);
    }
}
