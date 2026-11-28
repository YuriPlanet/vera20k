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
//!
//! VERA extension: a `<name>.opus` file replaces the sample `name` wherever
//! it is found ([`ClipCache::load_override`], docs/opus-audio.md). Whether a
//! name has one is looked up once per name and kept; retail data has none,
//! so a stock game pays one name-hash miss per sample name and decodes what
//! it always did.

use std::borrow::Cow;
use std::collections::HashMap;

use super::{DecodedAudio, decode_archive_clip, decode_index_clip, decode_opus_clip};
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
    /// Whether each sample name looked up so far has a playable `.opus`
    /// override. A name found here as `false` takes the retail path with no
    /// further lookup.
    overrides: HashMap<String, bool>,
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
    /// while the index is fixed for the process. A `.opus` override comes
    /// before all of them.
    pub(super) fn load(
        &mut self,
        name: &str,
        assets: &AssetManager,
        index: Option<&AudioIndex>,
    ) -> Option<DecodedAudio> {
        if let Some(clip) = self.load_override(name, |file| assets.get_ref(file).map(Cow::Borrowed))
        {
            return Some(clip);
        }
        index
            .and_then(|index| self.load_from_index(name, index))
            .or_else(|| decode_archive_clip(name, assets))
    }

    /// The `.opus` override of `name` ([`crate::assets::opus_file::override_name`])
    /// that `read` finds, decoded once and kept like an index clip. The first
    /// load of a name looks it up; a name without one, or whose file does not
    /// decode, is remembered as such and never looked up again, so the retail
    /// path that follows is the one it always was.
    pub(super) fn load_override<'a>(
        &mut self,
        name: &str,
        read: impl FnOnce(&str) -> Option<Cow<'a, [u8]>>,
    ) -> Option<DecodedAudio> {
        let decode = |name: &str| {
            let file = read(&crate::assets::opus_file::override_name(name))?;
            decode_opus_clip(&file, name)
        };
        match self.overrides.get(name).copied() {
            Some(false) => None,
            Some(true) => {
                let clip = self.cached_or_decode(name, decode);
                if clip.is_none() {
                    self.overrides.insert(name.to_owned(), false);
                }
                clip
            }
            None => {
                let clip = decode(name);
                self.overrides.insert(name.to_owned(), clip.is_some());
                if let Some(clip) = &clip {
                    log::info!("Sound {name} plays its .opus override");
                    self.loads += 1;
                    self.keep(name, clip);
                }
                clip
            }
        }
    }

    fn load_from_index(&mut self, name: &str, index: &AudioIndex) -> Option<DecodedAudio> {
        if self.index != index.id() {
            // The overrides come from the archives, not the index: they stay.
            *self = Self {
                index: index.id(),
                overrides: std::mem::take(&mut self.overrides),
                ..Self::default()
            };
        }
        self.cached_or_decode(name, |name| decode_index_clip(name, index))
    }

    /// The kept clip of `name`, else `decode`'s, kept while it fits.
    fn cached_or_decode(
        &mut self,
        name: &str,
        decode: impl FnOnce(&str) -> Option<DecodedAudio>,
    ) -> Option<DecodedAudio> {
        self.loads += 1;
        if let Some(cached) = self.clips.get_mut(name) {
            cached.last_load = self.loads;
            return Some(cached.clip.clone());
        }
        let clip = decode(name)?;
        self.keep(name, &clip);
        Some(clip)
    }

    /// Keep `clip` as `name`'s at the current load count, dropping the least
    /// recently loaded clips to fit; a clip larger than the bound is not kept.
    fn keep(&mut self, name: &str, clip: &DecodedAudio) {
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

#[cfg(all(test, tests_audio))]
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

    /// A `<name>.opus` file plays in place of the bag's `name`, at 48 kHz;
    /// a name without one decodes from the bag as before; and with no
    /// audio.idx mounted (a miniHD package) the override still plays.
    #[test]
    fn an_opus_file_overrides_its_sample_with_or_without_the_bag() {
        let dir = TestDirectory::new("clip-cache-opus");
        let opus = crate::test_fixture::bytes("tests/fixtures/audio/sine440.opus");
        std::fs::write(dir.path().join("Shot.opus"), opus).unwrap();
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let index = index_of(&[("shot", 64), ("other", 32)]);

        let mut cache = ClipCache::default();
        let shot = cache.load("shot", &assets, Some(&index)).unwrap();
        let layout = (
            shot.sample_rate,
            shot.native_channels,
            shot.native_sample_bytes,
        );
        assert_eq!(layout, (48_000, 1, 2));
        assert_eq!(shot.samples.len(), 9_600 * 2, "mono upmixed to stereo");
        assert!(Arc::ptr_eq(
            &shot.samples,
            &cache.load("shot", &assets, Some(&index)).unwrap().samples
        ));

        let other = cache.load("other", &assets, Some(&index)).unwrap();
        let bag = super::super::decode_index_clip("other", &index).unwrap();
        assert_eq!(
            (other.sample_rate, &*other.samples),
            (22_050, &*bag.samples)
        );

        let mut cache = ClipCache::default();
        assert_eq!(
            cache.load("SHOT", &assets, None).unwrap().samples.len(),
            9_600 * 2
        );
        assert!(cache.load("other", &assets, None).is_none());
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
