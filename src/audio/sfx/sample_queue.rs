//! The above-floor loader: `SoundEvent::LoadSamples @ 0x004048B0` for an
//! entry whose `Delay=` low bound is at least `0x21` (unsigned, `0x004048FB`),
//! and the queue `SoundEvent::SelectNextSample @ 0x00404BB0` keeps for the
//! event at `+0x1E8..+0x274`.
//!
//! Every start pass releases the previous buffers and loads one body sample,
//! plus the decay draw with `Control=decay`. PreparePlayout plays the first
//! loaded buffer, AdvancePlaylist adds none, and OnChannelSampleEnd draws the
//! gap before the next pass (`PlaylistCursor::sample_end`). The queue ends a
//! cue after one pass without `Control=loop`, or after `Loop=` passes.
//!
//! Executed source: tools/spatial_oracle/ambient_playout.{py,json,meta.json}.

use std::collections::BTreeMap;

use super::{DecodedAudio, LoadedPlayback, LoadedSampleIndices, SampleRng};
use crate::rules::sound_ini::{SoundEntry, control};

/// `SoundEvent+0x1E8` aQueue, `+0x268` count (the vector's length),
/// `+0x26C` pass, `+0x270` cursor and `+0x274` the RANDOM pick.
#[derive(Debug, Default, Clone)]
pub(super) struct SampleQueue {
    queue: Vec<i32>,
    pass: i32,
    cursor: i32,
    random_pick: i32,
}

impl SampleQueue {
    /// `+0x268`, which OnChannelSampleEnd and OnChannelDone read.
    pub(super) fn count(&self) -> i32 {
        self.queue.len() as i32
    }

    /// `SoundEvent::SelectNextSample @ 0x00404BB0`; `None` is its `-1`.
    /// `plays_attack` is flags8 clear.
    ///
    /// Once the cursor reaches the count it rebuilds the queue, unless a pass
    /// already ran without `Control=loop` or `Loop=` passes ran with it: the
    /// attack draw while flags8 is clear, then every body index (`all`), the
    /// first pass's RANDOM pick, or the first body index. RANDOM then takes and
    /// removes a random entry, except that the attack slot goes first. With
    /// flags8 set the attack slot can be drawn as a body.
    fn select_next(
        &mut self,
        entry: &SoundEntry,
        plays_attack: bool,
        rng: &mut impl SampleRng,
    ) -> Option<usize> {
        let count = entry.sounds.len() as i32;
        let heads_attack = entry.attack != 0 && plays_attack;
        if self.count() == self.cursor {
            let looping = entry.control & control::LOOP != 0;
            if (!looping && self.pass == 1)
                || (looping && entry.loop_count != 0 && entry.loop_count <= self.pass)
            {
                return None;
            }
            self.queue.clear();
            if heads_attack {
                self.queue.push(rng.ranged(0, entry.attack - 1));
            }
            if entry.control & control::ALL != 0 {
                self.queue.extend(entry.attack..count - entry.decay);
            } else if entry.control & control::RANDOM != 0 {
                if self.pass == 0 {
                    self.random_pick = rng.ranged(entry.attack, count - entry.decay - 1);
                }
                self.queue.push(self.random_pick);
            } else {
                self.queue.push(entry.attack);
            }
            self.cursor = 0;
            self.pass += 1;
        }
        let index = if entry.control & control::RANDOM != 0 && !(heads_attack && self.cursor < 1) {
            let pick = rng.ranged(i32::from(heads_attack), self.count() - 1);
            // VERA-internal: `.get`. Native indexes the queue unchecked; only a
            // hand-made entry whose attack/decay counts leave no body reaches
            // an out-of-range pick.
            let index = usize::try_from(pick)
                .ok()
                .filter(|pick| *pick < self.queue.len())?;
            self.queue.remove(index)
        } else {
            let index = *self.queue.get(usize::try_from(self.cursor).ok()?)?;
            self.cursor += 1;
            index
        };
        usize::try_from(index).ok()
    }
}

impl LoadedPlayback {
    /// LoadSamples4048B0 from the floor up (`0x00404ACD..0x00404B99`): the
    /// decay draw first, then SelectNextSample. A body loads with the decay
    /// after it; without one, the decay alone loads once and sets flags0x10
    /// (`decay_used`). `None` means no buffer loaded (`+0xA8 == 0`).
    pub(super) fn load_delayed(
        entry: &SoundEntry,
        queue: &mut SampleQueue,
        plays_attack: bool,
        decay_used: &mut bool,
        rng: &mut impl SampleRng,
        mut load: impl FnMut(&str) -> Option<DecodedAudio>,
    ) -> Option<Self> {
        let count = entry.sounds.len() as i32;
        let decay = (entry.decay != 0 && entry.control & control::DECAY != 0)
            .then(|| rng.ranged(count - entry.decay, count - 1));
        let body = queue.select_next(entry, plays_attack, rng);
        let mut loaded = Vec::new();
        let mut clips = BTreeMap::new();
        // A failed load leaves no buffer (`TEST ECX,ECX`), and the table stays
        // compact, as in the low-delay loader.
        let mut take = |index: i32| {
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            if let Some(clip) = entry.sounds.get(index).and_then(|name| load(name)) {
                clips.insert(index, clip);
                loaded.push(index);
            }
        };
        match (body, decay) {
            (Some(body), decay) => {
                take(body as i32);
                if let Some(decay) = decay {
                    take(decay);
                }
            }
            (None, Some(decay)) if !*decay_used => {
                take(decay);
                *decay_used = true;
            }
            (None, _) => {}
        }
        let first = *loaded.first()?;
        let selected = LoadedSampleIndices {
            attack: None,
            middle: vec![first],
            decay: (entry.control & control::DECAY != 0)
                .then(|| loaded.last().copied())
                .flatten(),
        };
        Some(Self { selected, clips })
    }
}
