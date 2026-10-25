//! RingSource through rodio's own mixer conversion at a high device rate.
//!
//! `Mixer::add` wraps every Player source in `UniformSourceIterator`
//! (rodio 0.22.2 `mixer.rs:62`). Its `SampleRateConverter` multiplies two
//! `u32` chunk counters (`conversions/sample_rate.rs:158`, `:173`); one
//! converter instance lives for one source span. A pitch-shifted rate coprime
//! with a 192 kHz device keeps the reduced ratio large, so a one-span cue
//! longer than about a second overflows those counters.

use super::*;
use crate::rules::ini_parser::IniFile;
use std::num::NonZero;

/// A loaded one-sample `Test` cue of `frames` stereo frames at 22 050 Hz,
/// played with `shifts` through the production ring.
fn ring_source(frames: usize, shifts: PlayShifts) -> RingSource {
    let registry = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n1=Test\n[Test]\nSounds=body\n",
    ));
    let entry = registry.get("Test").unwrap();
    let mut draws = PlaybackDraws::new(|low, _| low);
    let loaded = LoadedPlayback::load(entry, &mut draws, |_| {
        Some(DecodedAudio {
            samples: vec![0.25; frames * 2],
            sample_rate: 22_050,
            channels: 2,
            native_channels: 1,
            native_sample_bytes: 2,
        })
    })
    .unwrap();
    let mut cursor = PlaylistCursor::new(loaded.selected.clone(), entry.control, (0, 0), 1);
    let initial = cursor.prepare(&mut draws, true).unwrap();
    RingSource::new(loaded, cursor, initial, draws, shifts, 0x2000, None)
        .unwrap()
        .0
}

/// Pull a 192 kHz stereo mixer in 2048-frame device periods (the SFX sink's
/// period at that rate) until the Player reports empty, returning the device
/// seconds it took.
fn seconds_until_empty(frames: usize, shifts: PlayShifts) -> Option<f64> {
    const DEVICE: u32 = 192_000;
    let (mixer, mut device) =
        rodio::mixer::mixer(NonZero::new(2).unwrap(), NonZero::new(DEVICE).unwrap());
    let player = rodio::Player::connect_new(&mixer);
    player.append(ring_source(frames, shifts));
    for period in 1..=DEVICE as usize * 3 / 2048 {
        device.by_ref().take(2048 * 2).for_each(drop);
        if player.empty() {
            return Some((period * 2048) as f64 / f64::from(DEVICE));
        }
    }
    None
}

#[test]
fn shifted_cues_empty_their_player_on_a_192_khz_device() {
    // FShift +3: 103% of 22 050 Hz is 22 711 Hz, coprime with 192 000.
    let shifts = PlayShifts {
        frequency_pct: 103,
        volume_shift_pct: 0,
        predelay_ms: 0,
    };
    let rate = shifts.shifted_sample_rate(22_050);
    assert_eq!(rate, 22_711);
    // A CreditUp-length 24 ms tick and a 1.5 s cue, through the production
    // Player queue and mixer. Before the span bound the mixer's converter
    // outlived the cue (the Player queue keeps it alive with filler), so an
    // overflow-checked build panicked on its multiply after about a second
    // of output and a release build spun about 2^32 steps in the callback.
    // Each cue plays its ring's zero-padded quarters, then ends.
    for (frames, limit_s) in [(529, 0.5), (33_075, 2.0)] {
        let seconds = seconds_until_empty(frames, shifts);
        assert!(
            seconds.is_some_and(|s| s <= limit_s),
            "{frames}-frame cue at {rate} Hz emptied after {seconds:?} device seconds"
        );
    }
}
