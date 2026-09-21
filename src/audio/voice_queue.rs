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
    /// carries the id string and compares it the same way.
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
    /// upstream in VERA: the app input layer issues selection and order voices
    /// only for the local player's own objects, and only one object per
    /// dispatch batch speaks.
    pub fn queue(&mut self, owner: u64, sound_id: &str) {
        if sound_id.is_empty() {
            return;
        }
        self.pending.insert(owner, sound_id.to_string());
    }

    /// `TechnoClass::AI_Update @ 0x006F9EBB` — drain every latched voice.
    ///
    /// `handle_live(owner)` is `VocHandle::ValidateOrClear @ 0x00406130` for
    /// that object: true while the event the handle names is still the same
    /// event playing the same entry.
    ///
    /// Native visits objects in the active-object scheduler's order; VERA
    /// walks the map in ascending stable-id order so the pass is deterministic.
    /// The two only differ when two objects both have a voice latched in the
    /// same pass, which the one-voice-per-batch latch above already prevents
    /// for player input.
    pub fn drain(&mut self, mut handle_live: impl FnMut(u64) -> bool) -> Vec<VoiceDecision> {
        // VERA-internal, gamemd has no counterpart: native stores the playing
        // index inside the techno (`+0x4F4`), so it dies with the object and a
        // stale value is harmless — `0x006F9F03` only reads it while the
        // handle is live. VERA's map would otherwise keep one entry per object
        // that ever spoke, so entries whose handle has gone are dropped at the
        // top of the pass, before any of them can be compared.
        self.playing.retain(|&owner, _| handle_live(owner));

        let mut decisions = Vec::new();
        let mut settled = Vec::new();

        for (&owner, sound_id) in &self.pending {
            if handle_live(owner) {
                // `0x006F9EF7`: same index -> clear pending and let the line
                // finish; different index -> leave it latched for next pass.
                if self
                    .playing
                    .get(&owner)
                    .is_some_and(|live| live == sound_id)
                {
                    settled.push(owner);
                }
            } else {
                // `0x006F9ED9`: the handle is free, so this one starts now.
                self.playing.insert(owner, sound_id.clone());
                decisions.push(VoiceDecision {
                    owner,
                    sound_id: sound_id.clone(),
                });
                settled.push(owner);
            }
        }

        for owner in settled {
            self.pending.remove(&owner);
        }

        decisions
    }

    /// Forget one object entirely (removal, or a hard voice-slot reset).
    #[cfg(test)]
    pub fn forget(&mut self, owner: u64) {
        self.pending.remove(&owner);
        self.playing.remove(&owner);
    }

    /// The id latched for `owner`, if any — `TechnoClass+0x4F0`.
    #[cfg(test)]
    pub fn pending_for(&self, owner: u64) -> Option<&str> {
        self.pending.get(&owner).map(String::as_str)
    }

    /// The id this object's handle is playing — `TechnoClass+0x4F4`.
    #[cfg(test)]
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

        let decisions = queue.drain(|_| false);
        assert_eq!(
            decisions,
            vec![VoiceDecision {
                owner: 7,
                sound_id: "GIMove".to_string()
            }]
        );
        assert_eq!(queue.pending_for(7), None, "0x006F9F07 clears the latch");
    }

    #[test]
    fn the_same_line_while_it_is_still_playing_is_dropped_not_restarted() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GIMove");
        assert_eq!(queue.drain(|_| false).len(), 1);

        // Second click on the same unit while its line is mid-word.
        queue.queue(7, "GIMove");
        let decisions = queue.drain(|owner| owner == 7);
        assert!(
            decisions.is_empty(),
            "0x006F9F03 same-index path must not start the line again"
        );
        assert_eq!(queue.pending_for(7), None, "and it clears the latch");
        assert_eq!(queue.playing_for(7), Some("GIMove"));
    }

    #[test]
    fn a_different_line_waits_for_the_live_one_instead_of_cutting_it() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GISelect");
        assert_eq!(queue.drain(|_| false).len(), 1);

        queue.queue(7, "GIMove");
        // Handle still live with a different index: hold, do not play.
        assert!(queue.drain(|owner| owner == 7).is_empty());
        assert_eq!(
            queue.pending_for(7),
            Some("GIMove"),
            "0x006F9F05 leaves the latch set so the next pass retries"
        );

        // The line finishes; the retry starts the held one.
        let decisions = queue.drain(|_| false);
        assert_eq!(
            decisions,
            vec![VoiceDecision {
                owner: 7,
                sound_id: "GIMove".to_string()
            }]
        );
        assert_eq!(queue.playing_for(7), Some("GIMove"));
    }

    #[test]
    fn a_second_latch_before_the_drain_replaces_the_first() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GISelect");
        queue.queue(7, "GIMove");
        assert_eq!(queue.pending_for(7), Some("GIMove"));
        let decisions = queue.drain(|_| false);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].sound_id, "GIMove");
    }

    #[test]
    fn an_empty_id_is_the_native_minus_one_slot_and_latches_nothing() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "");
        assert_eq!(queue.pending_for(7), None);
        assert!(queue.drain(|_| false).is_empty());
    }

    #[test]
    fn two_objects_drain_in_ascending_stable_id_order() {
        let mut queue = VoiceQueue::new();
        queue.queue(9, "DogMove");
        queue.queue(3, "GIMove");
        let decisions = queue.drain(|_| false);
        assert_eq!(
            decisions.iter().map(|d| d.owner).collect::<Vec<_>>(),
            vec![3, 9]
        );
    }

    #[test]
    fn the_playing_map_does_not_grow_past_the_live_handles() {
        let mut queue = VoiceQueue::new();
        for owner in 0..8u64 {
            queue.queue(owner, "GIMove");
        }
        queue.drain(|_| false);
        // Nothing is live afterwards, so the bookkeeping empties out.
        queue.drain(|_| false);
        for owner in 0..8u64 {
            assert_eq!(queue.playing_for(owner), None);
        }
    }

    #[test]
    fn forget_drops_both_halves() {
        let mut queue = VoiceQueue::new();
        queue.queue(7, "GIMove");
        queue.drain(|_| false);
        queue.queue(7, "GISelect");
        queue.forget(7);
        assert_eq!(queue.pending_for(7), None);
        assert_eq!(queue.playing_for(7), None);
    }
}
