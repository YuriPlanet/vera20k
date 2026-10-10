//! Per-object unit-voice latch and drain — the repeat guard.
//!
//! gamemd keeps three fields on every techno for its acknowledgement line:
//! a **pending** voice index at `TechnoClass+0x4F0` (sentinel `-1`), a live
//! **handle** at `+0x4DC`, and the index the handle is **playing** at `+0x4F4`.
//!
//! `TechnoClass::Queue_Voice @ 0x00708D90` only *latches*:
//!
//! ```text
//! 00708d90  MOV  AL,[0x00822cf2]        ; g_SelectionVoice_Enable
//! 00708d96  TEST AL,AL ; JZ  -> return  ; voice disabled
//! 00708da1  CMP  EDI,-1 ; JZ  -> return ; no voice for this slot
//! 00708da6  MOV  ECX,[ESI+0x21c]        ; owner house
//! 00708dac  CALL 0x0050b6f0             ; HouseClass::IsHumanPlayer
//! 00708db3  JZ   -> return              ; not the human player
//! 00708db5  MOV  [ESI+0x4f0],EDI        ; latch, overwriting any prior pending
//! ```
//!
//! `TechnoClass::AI_Update @ 0x006F9EBB` drains it once per object AI pass:
//!
//! ```text
//! 006f9ebb  MOV  EAX,[ESI+0x4f0] ; CMP EAX,-1 ; JZ  -> nothing pending
//! 006f9ec8  LEA  EDI,[ESI+0x4dc] ; CALL 0x00406130   ; VocHandle::ValidateOrClear
//! 006f9ed7  JNZ  0x006f9ef7                          ; handle still live
//!           ; handle free:
//! 006f9eea  MOV  [ESI+0x4f4],ECX ; CALL 0x00750920   ; VocClass::PlayAtPos,
//!                                                    ; volume 1.0f, pan 0x2000
//! 006f9ef5  JMP  0x006f9f07                          ; then clear pending
//! 006f9ef7  MOV  EDX,[ESI+0x4f4] ; MOV EAX,[ESI+0x4f0] ; CMP EDX,EAX
//! 006f9f05  JNZ  0x006f9f0d                          ; DIFFERENT -> keep pending
//! 006f9f07  MOV  [ESI+0x4f0],-1                      ; SAME -> drop the repeat
//! ```
//!
//! So it is **not a timer**. Three outcomes, and only the middle one is what
//! players hear when they click the same unit twice:
//!
//! | handle | pending vs playing | outcome |
//! |---|---|---|
//! | free | — | play, remember the index, clear pending |
//! | live | same | **drop** — the line keeps going, it does not restart |
//! | live | different | **hold** — retry on the next pass, do not cut the line |
//!
//! This module is the decision half only: it owns no audio device and no
//! decoded samples, so the whole guard is testable without one. The caller
//! supplies the single audio fact it needs — whether an owner's handle is
//! still live — and applies the returned [`VoiceDecision`]s.

use std::collections::BTreeMap;

/// One object's voice work for this pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceDecision {
    /// Stable id of the object whose line this is (native: the `TechnoClass`).
    pub owner: u64,
    /// The `sound(md).ini` id to start. gamemd stores the Voc index; VERA
    /// carries the registry's canonical identity string.
    pub sound_id: String,
}

/// The per-object pending / playing pair, without the audio device.
#[derive(Debug, Default)]
pub struct VoiceQueue {
    /// `TechnoClass+0x4F0`. Absent == the native `-1` sentinel.
    pending: BTreeMap<u64, String>,
    /// `TechnoClass+0x4F4`, the index the object's live handle is playing.
    playing: BTreeMap<u64, String>,
}

impl VoiceQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// `TechnoClass::Queue_Voice @ 0x00708D90` — latch, do not play.
    ///
    /// An empty id stands in for the native `-1` slot (`0x00708DA1`) and is
    /// ignored. A second latch before the drain overwrites the first, exactly
    /// as `MOV [ESI+0x4F0],EDI` does.
    ///
    /// The two gates ahead of the latch — `g_SelectionVoice_Enable @
    /// 0x00822CF2` and `HouseClass::IsHumanPlayer @ 0x0050B6F0` — are already
    /// upstream in VERA. Selection uses app sound_dispatch's native Select /
    /// VoiceSelect admission; ordinary mouse batches admit the first success,
    /// whereas other native selection commands have their own latch scope.
    pub fn queue(&mut self, owner: u64, sound_id: &str) {
        if sound_id.is_empty() {
            return;
        }
        self.pending.insert(owner, sound_id.to_string());
    }

    /// The current pending owners, for a derived per-frame interest set.
    /// Reading this view never consumes a latch or supplies visitation order.
    pub fn pending_owners(&self) -> impl Iterator<Item = u64> + '_ {
        self.pending.keys().copied()
    }

    /// One reached `TechnoClass::AI` voice slot6F9EBB..6F9F0D.
    ///
    /// The live Logic cursor, not this map's key order, chooses the caller.
    /// `handle_live` comes from shared VocHandle406130 validation: a queued
    /// event is live even before it acquires a channel or device output.
    /// Audio servicing alone never visits this owner or consumes its latch.
    pub fn visit(&mut self, owner: u64, handle_live: bool) -> Option<VoiceDecision> {
        let pending = self.pending.get(&owner)?;
        if handle_live {
            if self.playing.get(&owner) == Some(pending) {
                self.pending.remove(&owner);
            }
            return None;
        }
        let sound_id = self.pending.remove(&owner).expect("pending owner remains");
        //6F9EEA writes the playing identity before PlayAtPos; even a failed
        //play consumes this request. The next reached visit probes the handle.
        self.playing.insert(owner, sound_id.clone());
        Some(VoiceDecision { owner, sound_id })
    }

    /// Forget one object entirely (removal, or a hard voice-slot reset).
    pub fn forget(&mut self, owner: u64) {
        self.pending.remove(&owner);
        self.playing.remove(&owner);
    }

    /// The id latched for `owner`, if any — `TechnoClass+0x4F0`.
    pub fn pending_for(&self, owner: u64) -> Option<&str> {
        self.pending.get(&owner).map(String::as_str)
    }

    /// The id this object's handle is playing — `TechnoClass+0x4F4`.
    pub fn playing_for(&self, owner: u64) -> Option<&str> {
        self.playing.get(&owner).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_handle_plays_and_clears_the_pending_slot() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GIMove");
        assert_eq!(queue.pending_for(7), Some("GIMove"));

        let decision = queue.visit(7, false);
        assert_eq!(
            decision,
            Some(VoiceDecision {
                owner: 7,
                sound_id: "GIMove".to_string()
            })
        );
        assert_eq!(queue.pending_for(7), None, "0x006F9F07 clears the latch");
    }

    #[test]
    fn the_same_line_while_it_is_still_playing_is_dropped_not_restarted() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GIMove");
        assert!(queue.visit(7, false).is_some());

        // Second click on the same unit while its line is mid-word.
        queue.queue(7, "GIMove");
        let decision = queue.visit(7, true);
        assert!(
            decision.is_none(),
            "0x006F9F03 same-index path must not start the line again"
        );
        assert_eq!(queue.pending_for(7), None, "and it clears the latch");
        assert_eq!(queue.playing_for(7), Some("GIMove"));
    }

    #[test]
    fn a_different_line_waits_for_the_live_one_instead_of_cutting_it() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GISelect");
        assert!(queue.visit(7, false).is_some());

        queue.queue(7, "GIMove");
        // Handle still live with a different index: hold, do not play.
        assert!(queue.visit(7, true).is_none());
        assert_eq!(
            queue.pending_for(7),
            Some("GIMove"),
            "0x006F9F05 leaves the latch set so the next pass retries"
        );

        // The line finishes; the retry starts the held one.
        let decision = queue.visit(7, false);
        assert_eq!(
            decision,
            Some(VoiceDecision {
                owner: 7,
                sound_id: "GIMove".to_string()
            })
        );
        assert_eq!(queue.playing_for(7), Some("GIMove"));
    }

    #[test]
    fn a_second_latch_before_the_drain_replaces_the_first() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GISelect");
        queue.queue(7, "GIMove");
        assert_eq!(queue.pending_for(7), Some("GIMove"));
        let decision = queue.visit(7, false).expect("reached pending owner");
        assert_eq!(decision.sound_id, "GIMove");
    }

    #[test]
    fn an_empty_id_is_the_native_minus_one_slot_and_latches_nothing() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "");
        assert_eq!(queue.pending_for(7), None);
        assert!(queue.visit(7, false).is_none());
    }

    #[test]
    fn two_objects_follow_reached_ai_order_instead_of_stable_id_order() {
        let mut queue = VoiceQueue::new();
        queue.queue(9, "DogMove");
        queue.queue(3, "GIMove");
        // Supplied live Logic order is 9, 3. Both owners reached the common
        // Techno voice slot6F9EBB; sorting IDs changes their admission order.
        let decisions: Vec<_> = [9, 3]
            .into_iter()
            .filter_map(|owner| queue.visit(owner, false))
            .collect();
        assert_eq!(
            decisions.iter().map(|d| d.owner).collect::<Vec<_>>(),
            vec![9, 3]
        );
    }

    #[test]
    fn no_visit_keeps_pending_and_other_visits_do_not_drain_it() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GIMove");
        queue.queue(9, "GISelect");
        // Paused/no-AI frames may inspect interest, but never drain an owner.
        assert_eq!(queue.pending_owners().collect::<Vec<_>>(), vec![7, 9]);
        assert!(queue.visit(42, false).is_none());
        assert!(queue.visit(9, false).is_some());
        assert_eq!(queue.pending_owners().collect::<Vec<_>>(), vec![7]);
        assert_eq!(queue.pending_for(7), Some("GIMove"));
        assert_eq!(queue.playing_for(7), None);
    }

    #[test]
    fn destroyed_before_visit_cannot_admit_its_pending_voice() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GIMove");
        queue.forget(7);
        assert!(queue.visit(7, false).is_none());
        assert!(queue.pending_owners().next().is_none());
    }

    #[test]
    fn forget_drops_both_halves() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GIMove");
        queue.visit(7, false);
        queue.queue(7, "GISelect");
        queue.forget(7);
        assert_eq!(queue.pending_for(7), None);
        assert_eq!(queue.playing_for(7), None);
    }
}
