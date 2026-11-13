//! The decoded-sample cache: every start of a sound shares one decode of its
//! clip instead of decoding the file bytes again.
//!
//! gamemd keeps loaded samples in one cache as well. `SoundEvent::LoadSamples
//! @ 0x004048B0` takes each sample from `SampleTracker__LoadSample @
//! 0x00401C00`, which returns one it already holds and moves it to the front
//! of its most-recently-used list; when its pools run out it frees the least
//! recently used sample no sound event holds (`CachedSample__AddRef @
//! 0x00401F80`, `CachedSample__Free @ 0x00401FC0`). `PreparePlayout @
//! 0x00404700` and `AdvancePlaylist @ 0x004047B0` play from the cached buffer
//! (`CachedSample__GetBuffer @ 0x00402030`). gamemd keeps the file bytes and
//! decodes them as the channel plays; VERA keeps the decoded clip, so a start
//! neither reads nor decodes.
//!
//! A clip stays at its own sample rate: each play's FShift draw sets the rate
//! it plays at, and rodio's mixer converts that one span at a time
//! (`MAX_SPAN_FRAMES` in `playback.rs`), so a copy converted once to the
//! device rate would not play the same samples.

use std::collections::HashMap;

use super::{DecodedAudio, decode_archive_clip, decode_index_clip};
use crate::assets::asset_manager::AssetManager;
use crate::assets::audio_bag::AudioIndex;

/// The decoded bytes the cache keeps: 16 MiB, about 95 seconds of the retail
/// bag's 22,050 Hz mono sound once decoded to f32 stereo. That is as much
/// sound as gamemd's cache holds: `SoundSystem__Init @ 0x00403ED0` gives it
/// 1 MiB of file bytes (`0x00403F33`), and the bag's 4-bit IMA ADPCM packs
/// about two samples into a byte. Its 200-sample count (`0x00403F2E`) is not
/// copied; memory is what the bound protects. A larger clip is not kept.
const MAX_BYTES: usize = 16 << 20;

#[derive(Default)]
pub(super) struct ClipCache {
    /// The [`AudioIndex::id`] the clips came from; 0 before the first load.
    index: u64,
    clips: HashMap<String, CachedClip>,
    /// The decoded bytes in `clips`.
    bytes: usize,
    /// Loads so far; each clip keeps the count at its last load.
    loads: u64,
}

struct CachedClip {
    clip: DecodedAudio,
    last_load: u64,
}

impl ClipCache {
    /// A sample by name, decoded to f32 stereo. A clip from the audio index
    /// is decoded once and shared by every later load while the cache keeps
    /// it; a dropped clip plays on in the starts that hold it. A name the
    /// index lacks is decoded from the MIX archives or loose files each time:
    /// a scenario's theater and side archives can change which file it finds,
    /// while the index is fixed for the process.
    pub(super) fn load(
        &mut self,
        name: &str,
        assets: &AssetManager,
        index: Option<&AudioIndex>,
    ) -> Option<DecodedAudio> {
        index
            .and_then(|index| self.load_from_index(name, index))
            .or_else(|| decode_archive_clip(name, assets))
    }

    fn load_from_index(&mut self, name: &str, index: &AudioIndex) -> Option<DecodedAudio> {
        if self.index != index.id() {
            *self = Self {
                index: index.id(),
                ..Self::default()
            };
        }
        self.loads += 1;
        if let Some(cached) = self.clips.get_mut(name) {
            cached.last_load = self.loads;
            return Some(cached.clip.clone());
        }
        let clip = decode_index_clip(name, index)?;
        let bytes = size_of_val(&*clip.samples);
        if bytes <= MAX_BYTES {
            while self.bytes + bytes > MAX_BYTES && self.drop_least_recently_loaded() {}
            self.bytes += bytes;
            let cached = CachedClip {
                clip: clip.clone(),
                last_load: self.loads,
            };
            self.clips.insert(name.to_owned(), cached);
        }
        Some(clip)
    }

    /// Drop the clip whose last load is the oldest; false when none is kept.
    fn drop_least_recently_loaded(&mut self) -> bool {
        let Some(name) = self
            .clips
            .iter()
            .min_by_key(|(_, cached)| cached.last_load)
            .map(|(name, _)| name.clone())
        else {
            return false;
        };
        if let Some(cached) = self.clips.remove(&name) {
            self.bytes -= size_of_val(&*cached.clip.samples);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::super::{
        PAN_CENTRE, PendingPlayback, PendingSampleService, PlayRequest, PlaybackDraws,
        PlaybackService, SfxChannel, SfxOutputScales, SoundArbiter, SoundRegistry, VOLUME_SCALE,
        entry_facts, load_sfx, registry_key,
    };
    use super::*;
    use crate::assets::asset_manager::test_support::TestDirectory;
    use crate::assets::audio_bag::test_support::build_idx;
    use crate::rules::ini_parser::IniFile;

    /// An index of 8-bit mono PCM clips, each `frames` long and filled with
    /// its own byte.
    fn index_of(clips: &[(&str, usize)]) -> AudioIndex {
        let mut bag = Vec::new();
        let mut entries = Vec::new();
        for (fill, &(name, frames)) in (1u8..).zip(clips) {
            entries.push((name, bag.len() as u32, frames as u32, 22_050, 0));
            bag.resize(bag.len() + frames, fill);
        }
        AudioIndex::from_idx_bag(&build_idx(&entries), bag).unwrap()
    }

    /// Two starts of one sound play one decode: the second LoadSamples4048B0
    /// pass shares the samples of the first, which are a fresh decode's. A
    /// start against another index decodes that index's clip.
    #[test]
    fn a_second_start_of_a_sound_plays_the_first_decode() {
        let registry = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n1=Shot\n[Shot]\nSounds=shot\n",
        ));
        let dir = TestDirectory::new("clip-cache-start");
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let mut arbiter = SoundArbiter::new(0);
        let mut pending = BTreeMap::new();
        let mut clips = ClipCache::default();
        let mut start = |index: &AudioIndex| {
            let request = PlayRequest {
                key: registry_key("Shot"),
                facts: entry_facts("Shot", &registry),
                volume_linear: VOLUME_SCALE,
                pan: PAN_CENTRE,
            };
            let event = arbiter.submit(&request, 0).unwrap();
            let token = arbiter.event_token(event).unwrap();
            pending.insert(
                event,
                PendingPlayback::new(token, request.key, SfxChannel::Sound),
            );
            let mut service = PendingSampleService {
                pending: &mut pending,
                live: &BTreeMap::new(),
                scales: SfxOutputScales {
                    sound_volume: 1.0,
                    voice_volume: 1.0,
                    sound_master_linear: VOLUME_SCALE,
                    eva_master_linear: VOLUME_SCALE,
                    focus_output_scale: 1.0,
                    many_sounds_linear: VOLUME_SCALE,
                },
                registry: &registry,
                assets: &assets,
                audio_index: Some(index),
                clips: &mut clips,
                rng: &mut PlaybackDraws::new(|low, _| low),
                observer: None,
            };
            assert!(service.load_samples(event, true));
            pending[&event].loaded.as_ref().unwrap().clips[&0]
                .samples
                .clone()
        };
        let index = index_of(&[("shot", 64)]);
        let first = start(&index);
        assert!(Arc::ptr_eq(&first, &start(&index)), "one decode");
        let fresh = load_sfx("shot", &assets, Some(&index)).unwrap();
        assert_eq!(*first, *fresh.samples);

        let other = index_of(&[("other", 8), ("shot", 32)]);
        let fresh = load_sfx("shot", &assets, Some(&other)).unwrap();
        assert_eq!(*start(&other), *fresh.samples);
    }

    /// Past [`MAX_BYTES`] the cache drops the clip whose last load is the
    /// oldest and keeps the rest.
    #[test]
    fn past_its_bytes_the_cache_drops_the_least_recently_loaded_clip() {
        // Each clip decodes to three eighths of the bound, 8 bytes a frame.
        let frames = MAX_BYTES * 3 / 8 / 8;
        let index = index_of(&[("a", frames), ("b", frames), ("c", frames)]);
        let dir = TestDirectory::new("clip-cache-bound");
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let mut cache = ClipCache::default();
        let mut load = |name| cache.load(name, &assets, Some(&index)).unwrap().samples;
        let a = load("a");
        let b = load("b");
        load("a");
        load("c");
        assert!(Arc::ptr_eq(&a, &load("a")), "a stays");
        assert!(!Arc::ptr_eq(&b, &load("b")), "b was loaded longest ago");
    }
}
