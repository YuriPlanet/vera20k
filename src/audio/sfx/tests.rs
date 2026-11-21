//! General `SfxPlayer` regression tests: spatial gain and pan, playout
//! selection, output gains and EVA stream routing, service-clock admission
//! and WAV decoding through the
//! production owners. Subject-specific suites live in sibling test files.

use super::*;

/// Drive the production clock/service owners without app/window setup.
/// This supplies neither sample choices nor independent RNG logic.
struct TestAudioService {
    clock: arbiter::AudioServiceClock,
    main: crate::sim::rng::MainRng,
}
impl Default for TestAudioService {
    fn default() -> Self {
        Self {
            clock: arbiter::AudioServiceClock::default(),
            main: crate::sim::rng::MainRng::new(31),
        }
    }
}
impl TestAudioService {
    fn pump(
        &mut self,
        player: &mut SfxPlayer,
        now: u64,
        registry: &SoundRegistry,
        eva_registry: &EvaRegistry,
        assets: &AssetManager,
        index: Option<&crate::assets::audio_bag::AudioIndex>,
    ) {
        let main = self.main.draws();
        let mut draw = PlaybackDraws::new(move |low, high| main.ranged(low, high));
        player.service_device_outputs(now);
        if self.clock.admit(now) {
            player.service_events(now, registry, eva_registry, assets, index, &mut draw);
        }
    }
}

/// About 10 ms of frames, below the 33 ms arbiter pump, clamped to what
/// the device supports.
#[test]
fn sfx_buffer_period_stays_below_the_pump_interval() {
    use rodio::cpal::SupportedBufferSize;
    assert_eq!(
        sfx_buffer_frames(48_000, &SupportedBufferSize::Unknown),
        512
    );
    assert_eq!(
        sfx_buffer_frames(44_100, &SupportedBufferSize::Unknown),
        512
    );
    let range = SupportedBufferSize::Range {
        min: 1024,
        max: 4096,
    };
    assert_eq!(sfx_buffer_frames(48_000, &range), 1024);
    let range = SupportedBufferSize::Range { min: 64, max: 256 };
    assert_eq!(sfx_buffer_frames(48_000, &range), 256);
}
use crate::rules::ini_parser::IniFile;

/// A scripted RNG: hands out the listed draws in order and records every
/// request so a test can pin the native draw sequence.
struct ScriptedRng {
    draws: Vec<i32>,
    requests: Vec<(i32, i32)>,
}

impl ScriptedRng {
    fn new(draws: &[i32]) -> Self {
        Self {
            draws: draws.iter().rev().copied().collect(),
            requests: Vec::new(),
        }
    }
}

impl SampleRng for ScriptedRng {
    fn ranged(&mut self, low: i32, high: i32) -> i32 {
        if low == high {
            return low;
        }
        self.requests.push((low, high));
        self.draws.pop().unwrap_or(low)
    }
}

/// `[SoundList]` is what registers an id (`VocClass::ReadSoundListINI @
/// 0x007510D0`), so the fixture names `[T]` there the way the stock file
/// names every sound.
fn entry(body: &str) -> SoundEntry {
    let registry =
        SoundRegistry::from_ini(&IniFile::from_str(&format!("[SoundList]\n1=T\n{body}")));
    registry.get("T").expect("entry").clone()
}

const LISTENER_W: f32 = 1024.0;
const LISTENER_H: f32 = 600.0;

fn source(range_cells: i32, type_flags: u32, min_volume: f32) -> SpatialSource {
    SpatialSource {
        range_cells,
        type_flags,
        min_volume,
    }
}

fn gain(client_x: i32, client_y: i32, src: SpatialSource) -> Option<SpatialGain> {
    calc_volume_and_pan(client_x, client_y, LISTENER_W, LISTENER_H, src, false)
}

/// Vectors walked through the `0x00750AC0` transcription: a 1024x600
/// tactical view (halfW 512, halfH 300, fullW 1024) and Range 10
/// (maxRange 600).
#[test]
fn calc_volume_and_pan_screen_type_matches_the_native_transcription() {
    let screen = source(10, sound_type::SCREEN, 0.5);
    // View centre: on screen is full volume, centred pan.
    assert_eq!(
        gain(512, 300, screen),
        Some(SpatialGain {
            volume: 1.0,
            pan: PAN_CENTRE
        })
    );
    // Anywhere on screen is still full volume; pan follows the offset.
    let edge = gain(1023, 599, screen).unwrap();
    assert_eq!(edge.volume, 1.0);
    assert_eq!(
        edge.pan,
        X87Chop53::ftol_f64_low_masked(511.0 * 8192.0 / 1024.0 + 8192.0)
    );
    // 300 px right of the view edge: half volume, pan 8192 + 812 * 8.
    assert_eq!(
        gain(1324, 300, screen),
        Some(SpatialGain {
            volume: 0.5,
            pan: 14688
        })
    );
    // Y distances count double: 100 px below the view edge is 200.
    assert_eq!(gain(512, 700, screen).unwrap().volume, 400.0 / 600.0);
    // The larger axis wins: 200 px right (200) vs 150 px below (300).
    assert_eq!(gain(1224, 750, screen).unwrap().volume, 300.0 / 600.0);
    // Below the 0.05 cutoff is silent; just above is not.
    assert_eq!(gain(1600, 300, screen), None);
    assert_eq!(gain(1584, 300, screen).unwrap().volume, 40.0 / 600.0);
    // At and beyond maxRange the volume is exactly 0 (silent).
    assert_eq!(gain(1624, 300, screen), None);
    assert_eq!(gain(-1000, 300, screen), None);
    // A zero range is never audible.
    assert_eq!(gain(512, 300, source(0, sound_type::SCREEN, 0.0)), None);
}

#[test]
fn calc_volume_and_pan_local_global_and_shroud_gates() {
    // LOCAL skips the half-view subtraction: 100 px from centre is 100.
    let local = source(10, sound_type::LOCAL, 0.5);
    assert_eq!(gain(612, 300, local).unwrap().volume, 500.0 / 600.0);
    // LOCAL with a large range reaches the pan clamps at both ends.
    let far = source(100, sound_type::LOCAL, 0.0);
    assert_eq!(gain(-1500, 300, far).unwrap().pan, 0);
    assert_eq!(gain(3000, 300, far).unwrap().pan, PAN_SCALE);
    // GLOBAL raises a 10/600 volume to the MinVolume floor; SCREEN alone
    // falls below the cutoff instead.
    let global = source(10, sound_type::SCREEN | sound_type::GLOBAL, 0.5);
    assert_eq!(gain(1614, 300, global).unwrap().volume, 0.5);
    assert_eq!(gain(1614, 300, source(10, sound_type::SCREEN, 0.5)), None);
    // GLOBAL keeps a louder computed volume.
    assert_eq!(gain(1324, 300, global).unwrap().volume, 0.5);
    assert_eq!(gain(1224, 300, global).unwrap().volume, 400.0 / 600.0);
    // SHROUD silences a shrouded cell only when the flag is set.
    let shroud = source(10, sound_type::SCREEN | sound_type::SHROUD, 0.5);
    assert_eq!(
        calc_volume_and_pan(512, 300, LISTENER_W, LISTENER_H, shroud, true),
        None
    );
    assert!(calc_volume_and_pan(512, 300, LISTENER_W, LISTENER_H, shroud, false).is_some());
    let unshrouded = source(10, sound_type::SCREEN | sound_type::UNSHROUD, 0.5);
    assert!(calc_volume_and_pan(512, 300, LISTENER_W, LISTENER_H, unshrouded, true).is_some());
}

/// Odd view widths put the centre on a half pixel; the distances are
/// `ftol`-truncated before `abs`, the pan offset is not.
#[test]
fn calc_volume_and_pan_truncates_distances_like_ftol() {
    let src = source(10, sound_type::LOCAL, 0.0);
    // clientX - 511.5 = 0.5 -> ftol 0 -> full volume; pan keeps the 0.5.
    let g = calc_volume_and_pan(512, 300, 1023.0, LISTENER_H, src, false).unwrap();
    assert_eq!(g.volume, 1.0);
    assert_eq!(
        g.pan,
        X87Chop53::ftol_f64_low_masked(0.5 * 8192.0 / 1023.0 + 8192.0)
    );
    // clientX - 511.5 = -0.5 -> ftol 0 as well (truncation toward zero).
    let g = calc_volume_and_pan(511, 300, 1023.0, LISTENER_H, src, false).unwrap();
    assert_eq!(g.volume, 1.0);
    assert_eq!(
        g.pan,
        X87Chop53::ftol_f64_low_masked(-0.5 * 8192.0 / 1023.0 + 8192.0)
    );
}

fn zoom_listener(zoom: f32) -> SpatialListener {
    SpatialListener {
        tactical_width: LISTENER_W as i32,
        tactical_height: LISTENER_H as i32,
        origin_x: 0.0,
        origin_y: 0.0,
        zoom,
    }
}

/// The listener rect and the sound positions must stay in one frame at
/// every zoom. VERA projects `device = (world - camera) * zoom`, so a
/// 1024x600 device viewport covers `1024/zoom` x `600/zoom` world pixels;
/// `SpatialListener::view_extent` is what puts the rect in the world frame
/// the positions already use.
///
/// gamemd has no zoom, so nothing here is a native golden. What the
/// vectors pin is that zoom behaves like the one zoom-shaped thing gamemd
/// *does* have — a resolution change, which alters how much world the view
/// rect covers while `VocClass::CalcVolumeAndPan @ 0x00750AC0` keeps
/// `Range * 0x3C` as a fixed cell distance.
#[test]
fn spatial_gain_measures_one_frame_at_every_zoom() {
    let src = source(10, sound_type::SCREEN, 0.5);
    // At zoom 1.0 the world frame *is* the device frame, so the listener
    // must reproduce the raw-device-size call bit for bit.
    for x in [0.0f32, 256.0, 512.0, 1324.0, 1584.0] {
        assert_eq!(
            spatial_gain(src, x, 300.0, &zoom_listener(1.0), false),
            calc_volume_and_pan(x as i32, 300, LISTENER_W, LISTENER_H, src, false),
            "zoom 1.0 must equal the native device-pixel call at x={x}"
        );
    }

    // `Range=10` is 600 world pixels — 10 cells — at every zoom. A sound
    // exactly 300 world pixels past the right edge of the visible area is
    // half volume whether the view covers 1024, 512 or 2048 world pixels.
    for (zoom, view_w, centre_y) in [
        (1.0f32, 1024.0f32, 300.0f32),
        (2.0, 512.0, 150.0),
        (0.5, 2048.0, 600.0),
    ] {
        let gain = spatial_gain(src, view_w + 300.0, centre_y, &zoom_listener(zoom), false)
            .unwrap_or_else(|| panic!("audible at zoom {zoom}"));
        assert_eq!(gain.volume, 0.5, "zoom {zoom} falloff");
    }

    // Pan is the position *within* the view, so a sound a quarter of the
    // way across the visible area sits at the same 6144 at every zoom —
    // and stays inside the view, hence full volume.
    for (zoom, view_w, centre_y) in [
        (1.0f32, 1024.0f32, 300.0f32),
        (2.0, 512.0, 150.0),
        (0.5, 2048.0, 600.0),
    ] {
        let gain = spatial_gain(src, view_w * 0.25, centre_y, &zoom_listener(zoom), false)
            .unwrap_or_else(|| panic!("audible at zoom {zoom}"));
        assert_eq!(
            (gain.volume, gain.pan),
            (1.0, 6144),
            "zoom {zoom} quarter-width pan"
        );
    }

    // The regression the frame mismatch hid: one fixed world position must
    // move with the zoom. Ignoring zoom made all three of these 0.5/14688.
    let fixed = (1324.0f32, 300.0f32);
    assert_eq!(
        spatial_gain(src, fixed.0, fixed.1, &zoom_listener(1.0), false),
        Some(SpatialGain {
            volume: 0.5,
            pan: 14688
        })
    );
    // Zoomed 2x the view covers 512x300 world px, so the same point is
    // 812 px outside it — past `Range * 60` and silent.
    assert_eq!(
        spatial_gain(src, fixed.0, fixed.1, &zoom_listener(2.0), false),
        None
    );
    // Zoomed out to 0.5x the view covers 2048x1200 world px, so the point
    // is on screen: full volume, panned by its offset from the centre.
    assert_eq!(
        spatial_gain(src, fixed.0, fixed.1, &zoom_listener(0.5), false),
        Some(SpatialGain {
            volume: 1.0,
            pan: 9392
        })
    );

    // A degenerate zoom must neither divide by zero (the VERA-internal
    // EPSILON floor) nor panic: the resulting half-view saturates the
    // `ftol` cast to `i32::MIN`, which `native_abs` wraps the way
    // `CDQ; XOR; SUB` does instead of trapping like `i32::abs`.
    assert!(spatial_gain(src, 0.0, 0.0, &zoom_listener(0.0), false).is_some());
}

#[test]
fn spatial_listener_projects_world_pixels_to_client_points() {
    let listener = SpatialListener {
        tactical_width: LISTENER_W as i32,
        tactical_height: LISTENER_H as i32,
        origin_x: 1000.25,
        origin_y: 2000.0,
        zoom: 1.0,
    };
    assert_eq!(listener.client_point(1512.25, 2300.0), (512, 300));
    assert_eq!(listener.client_point(999.75, 1999.0), (0, -1));
    let src = source(10, sound_type::SCREEN, 0.5);
    assert_eq!(
        spatial_gain(src, 1512.25, 2300.0, &listener, false),
        Some(SpatialGain {
            volume: 1.0,
            pan: PAN_CENTRE
        })
    );
}

/// The `0x00816380` table (machine-read) against `round(1000 * log2(i /
/// 100))` and the index arithmetic of `FUN_0040A6D0`.
#[test]
fn dsound_attenuation_table_and_index_follow_the_binary() {
    for (i, &entry) in DSOUND_ATTENUATION_TABLE.iter().enumerate().skip(1) {
        let expected = (1000.0 * (i as f64 / 100.0).log2()).round() as i16;
        assert_eq!(entry, expected, "table[{i}]");
    }
    assert_eq!(DSOUND_ATTENUATION_TABLE[0], -10000);
    assert_eq!(DSOUND_ATTENUATION_TABLE[100], 0);
    assert_eq!(native_volume_amplitude(VOLUME_SCALE), 1.0);
    assert!((native_volume_amplitude(8192) - 10f32.powf(-0.5)).abs() < 1e-6);
    assert_eq!(native_volume_amplitude(0), 0.0);
    // 16383 * 25 >> 12 = 99, so anything short of full scale loses 0.14 dB.
    assert!((native_volume_amplitude(16383) - 10f32.powf(-14.0 / 2000.0)).abs() < 1e-6);
    assert_eq!(combine_linear(13107, VOLUME_SCALE), 13107);
    assert_eq!(combine_linear(8192, 8192), 4096);
    assert_eq!(combine_linear(VOLUME_SCALE, VOLUME_SCALE), VOLUME_SCALE);
}

#[test]
fn pan_channel_gains_attenuate_the_far_side_through_the_table() {
    assert_eq!(pan_channel_gains(PAN_CENTRE), (1.0, 1.0));
    // Full right: p = 100, the left channel takes table[0].
    assert_eq!(pan_channel_gains(PAN_SCALE), (0.0, 1.0));
    assert_eq!(pan_channel_gains(0), (1.0, 0.0));
    // 12288 -> 150 - 100 = 50 -> left attenuated by table[50] = -10 dB.
    let (left, right) = pan_channel_gains(12288);
    assert!((left - 10f32.powf(-0.5)).abs() < 1e-6);
    assert_eq!(right, 1.0);
    let (left, right) = pan_channel_gains(4096);
    assert_eq!(left, 1.0);
    assert!((right - 10f32.powf(-0.5)).abs() < 1e-6);
    let samples: Vec<f32> = playback::ClipSource::new(
        vec![1.0, 1.0, 0.5, 0.5].into(),
        NonZero::new(2).unwrap(),
        NonZero::new(22050).unwrap(),
    )
    .panned(12288)
    .collect();
    assert!((samples[0] - 10f32.powf(-0.5)).abs() < 1e-6);
    assert_eq!(samples[1], 1.0);
    assert_eq!(samples[3], 0.5);
}

#[test]
fn play_shifts_draw_fshift_vshift_then_predelay_in_native_order() {
    let e =
        entry("[T]\nSounds=a\nFShift= -10 10\nVShift=20\nControl= random predelay\nDelay=0 400\n");
    let mut rng = ScriptedRng::new(&[7, 13, 250]);
    let shifts = PlayShifts::draw(&e, &mut rng);
    assert_eq!(rng.requests, vec![(-10, 10), (0, 20), (0, 400)]);
    assert_eq!(shifts.frequency_pct, 107);
    assert_eq!(shifts.volume_shift_pct, 13);
    // 0x4000 - (13 << 14) / 100 = 16384 - 2129.
    assert_eq!(shifts.volume_linear(), 16384 - 2129);
    assert_eq!(shifts.shifted_sample_rate(22050), 22050 * 107 / 100);
    // The draw is carried, not discarded: the arbiter parks the event in
    // state 2 for this long.
    assert_eq!(shifts.predelay_ms, 250);

    // AMBIENT pre-delays draw from 0x21 regardless of Delay.min.
    let ambient = entry("[T]\nSounds=a\nControl= loop ambient\nDelay=5000 8000\n");
    let mut rng = ScriptedRng::new(&[6000]);
    let shifts = PlayShifts::draw(&ambient, &mut rng);
    assert_eq!(rng.requests, vec![(0x21, 8000)]);
    assert_eq!(shifts.frequency_pct, 100);
    assert_eq!(shifts.volume_linear(), VOLUME_SCALE);
    assert_eq!(shifts.shifted_sample_rate(22050), 22050);
    assert_eq!(shifts.predelay_ms, 6000);

    // No shifts, no pre-delay control: nothing is drawn.
    let plain = entry("[T]\nSounds=a\nDelay=0 400\n");
    let mut rng = ScriptedRng::new(&[]);
    let shifts = PlayShifts::draw(&plain, &mut rng);
    assert!(rng.requests.is_empty());
    assert_eq!(shifts.predelay_ms, 0);
}

/// The registry facts the arbiter arbitrates on come straight off the
/// `[SoundList]` entry, and a name that is not one falls back to
/// `[Defaults]`.
#[test]
fn entry_facts_carry_the_registry_priority_limit_control_and_loop() {
    let registry = SoundRegistry::from_ini(&IniFile::from_str(
        "[Defaults]\nLimit=5\nPriority=NORMAL\n\
         [SoundList]\n1=RocketeerMoveLoop\n\
         [RocketeerMoveLoop]\nSounds=a b c d e\n\
         Control= loop random all decay attack\nPriority=Low\nLimit=3\nVolume=25\n",
    ));
    let facts = entry_facts("RocketeerMoveLoop", &registry);
    assert_eq!(facts.priority, 1);
    assert_eq!(facts.limit, 3);
    assert_eq!(facts.loop_count, 0);
    assert!(facts.control & control::LOOP != 0);
    // `Loop=` absent with `Control=loop` is the owner-driven sustain that
    // `AudioEventClass::IsLoopable @ 0x00406650` reports.
    assert!(facts.is_loopable());

    // `[Defaults] Limit=5` reaches a raw bag name through the fallback.
    let bag = entry_facts("ImNotAnEntry", &registry);
    assert_eq!(bag.limit, 5);
    assert_eq!(bag.priority, 2);
    assert!(!bag.is_loopable());
}

/// A one-shot `Control= random predelay` entry is not loopable, which is
/// what keeps a Grizzly's `MoveSound` a single start-up sample rather
/// than an engine hum: `[GTNK] MoveSound=GrizzlyTankMoveStart` and
/// `[GrizzlyTankMoveStart] Control= random predelay`, `Delay=0 400`.
#[test]
fn a_random_predelay_move_sound_is_not_a_sustained_loop() {
    let registry = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n1=GrizzlyTankMoveStart\n\
         [GrizzlyTankMoveStart]\nSounds=vgristaa vgristab vgristac\n\
         Control= random predelay\nDelay=0 400\nPriority=low\nVolume=40\n",
    ));
    let facts = entry_facts("GrizzlyTankMoveStart", &registry);
    assert!(facts.control & control::LOOP == 0);
    assert!(!facts.is_loopable());
    assert_eq!(facts.delay_ms, (0, 400));
}

/// `VocClass::AddSample @ 0x004064A0`: a `Sounds=` name the audio index
/// lacks stores nothing and clears the entry's `+0x0C` (`0x00406510`), so
/// `AllocateFromPool @ 0x00405190` refuses every play (`0x004051B6`) even
/// though another name resolved. A 33rd token is refused before its lookup.
/// Read the stock way: Ares' AddSample (`0x1005AFA0`) never clears `+0x0C`.
#[test]
fn one_unresolvable_sample_name_silences_the_whole_entry() {
    use crate::assets::audio_bag::{AudioIndex, test_support::build_idx};
    use crate::rules::sound_ini::MAX_SAMPLES;
    let index =
        AudioIndex::from_idx_bag(&build_idx(&[("known", 0, 0, 22050, 0)]), Vec::new()).unwrap();
    let full = vec!["known"; MAX_SAMPLES].join(" ");
    let registry = SoundRegistry::from_ini_admitting(
        &IniFile::from_str(&format!(
            "[SoundList]\n1=Mixed\n2=Clean\n3=Full\n[Mixed]\nSounds=$known missing\n\
             [Clean]\nSounds=#KNOWN\n[Full]\nSounds={full} missing\n"
        )),
        Some(&index),
    );
    assert_eq!(registry.get("Mixed").unwrap().sounds, ["known"]);
    let mut arbiter = SoundArbiter::new(0);
    let mut play = |id: &str| {
        let request = PlayRequest {
            key: registry_key(id),
            facts: entry_facts(id, &registry),
            volume_linear: VOLUME_SCALE,
            pan: VOLUME_SCALE / 2,
        };
        arbiter.submit(&request, 0).is_some()
    };
    assert!(!play("Mixed"));
    assert!(play("Clean"));
    assert!(play("Full"));
}

/// `Control=random` over three samples: one `RandomRanged(0, 2)` draw and
/// the equal-bounds pick that follows draws nothing.
#[test]
fn select_playout_random_picks_one_body_sample() {
    let e = entry("[T]\nSounds=a b c\nControl=random\n");
    let mut rng = ScriptedRng::new(&[1]);
    assert_eq!(select_playout(&e, &mut rng), vec![1]);
    assert_eq!(rng.requests, vec![(0, 2)]);
}

/// Without `random` the first body sample always plays — there is no
/// round-robin — and `all` plays the whole body in order.
#[test]
fn select_playout_sequential_forms_play_first_or_all() {
    let first = entry("[T]\nSounds=a b c\n");
    let mut rng = ScriptedRng::new(&[]);
    assert_eq!(select_playout(&first, &mut rng), vec![0]);
    assert_eq!(select_playout(&first, &mut rng), vec![0]);
    assert!(rng.requests.is_empty());

    let all = entry("[T]\nSounds=a b c\nControl=all\n");
    assert_eq!(select_playout(&all, &mut rng), vec![0, 1, 2]);
    assert!(rng.requests.is_empty());
}

/// `random all`: the body is loaded whole and played in pick-and-remove
/// order, `RandomRanged(0, remaining - 1)` each step.
#[test]
fn select_playout_random_all_shuffles_by_pick_and_remove() {
    let e = entry("[T]\nSounds=a b c\nControl= random all\n");
    let mut rng = ScriptedRng::new(&[2, 0]);
    assert_eq!(select_playout(&e, &mut rng), vec![2, 0, 1]);
    assert_eq!(rng.requests, vec![(0, 2), (0, 1)]);
}

/// Attack/decay envelopes: a random attack sample first, the body, and a
/// random decay sample last, with the native draw order.
#[test]
fn select_playout_attack_body_decay_order_and_draws() {
    let e = entry(
        "[T]\nSounds=a1 a2 a3 b1 b2 d1 d2 d3\nControl= random all attack decay\nAttack=3\nDecay=3\n",
    );
    let mut rng = ScriptedRng::new(&[1, 6, 1]);
    assert_eq!(select_playout(&e, &mut rng), vec![1, 4, 3, 6]);
    assert_eq!(rng.requests, vec![(0, 2), (5, 7), (0, 1)]);

    let single = entry("[T]\nSounds=a b1 b2 d\nControl= random attack decay\nAttack=1\nDecay=1\n");
    // Attack 1 and Decay 1 are equal-bounds draws: only the body draws.
    let mut rng = ScriptedRng::new(&[2]);
    assert_eq!(select_playout(&single, &mut rng), vec![0, 2, 3]);
    assert_eq!(rng.requests, vec![(1, 2)]);
}

fn clip(value: f32, frames: usize, sample_rate: u32) -> DecodedAudio {
    DecodedAudio {
        samples: vec![value; frames * 2].into(),
        sample_rate,
        channels: 2,
        native_channels: 1,
        native_sample_bytes: 2,
    }
}

#[test]
fn resolve_entry_playback_chains_samples_and_applies_the_shifts() {
    let e = entry("[T]\nSounds=a b c\nVolume=80\nControl= all\nFShift=10 10\nVShift=50\n");
    let mut rng = ScriptedRng::new(&[50]);
    let resolved = resolve_entry_playback(&e, &mut rng, |name| match name {
        "a" => Some(clip(0.1, 2, 22050)),
        "b" => None,
        "c" => Some(clip(0.3, 1, 22050)),
        _ => unreachable!(),
    })
    .expect("resolved");
    assert_eq!(*resolved.decoded.samples, [0.1, 0.1, 0.1, 0.1, 0.3, 0.3]);
    assert_eq!(resolved.decoded.sample_rate, 22050 * 110 / 100);
    assert_eq!(rng.requests, vec![(0, 50)]);

    // A rate mismatch chains at the first clip's rate (Fill409880 sets no
    // new frequency).
    let mut rng = ScriptedRng::new(&[0]);
    let resolved = resolve_entry_playback(&e, &mut rng, |name| match name {
        "a" => Some(clip(0.1, 1, 22050)),
        _ => Some(clip(0.2, 1, 11025)),
    })
    .expect("resolved");
    assert_eq!(*resolved.decoded.samples, [0.1, 0.1, 0.2, 0.2, 0.2, 0.2]);
    assert_eq!(resolved.decoded.sample_rate, 22050 * 110 / 100);

    // Nothing loadable resolves to nothing.
    assert!(resolve_entry_playback(&e, &mut rng, |_| None).is_none());
}

#[test]
fn cloak_sound_registered_resolution_rejects_raw_sample_and_invalid_names() {
    let registry = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n1=NavalUnitEmerge\n[NavalUnitEmerge]\nSounds=vnavupa\nVolume=55\n",
    ));
    let entry = registered_entry("NavalUnitEmerge", &registry)
        .expect("the retail Voc/event identity resolves to its registered entry");
    assert_eq!(entry.sounds, vec!["vnavupa"]);
    assert_eq!(entry.volume_linear, 9011); // X87Chop53::ftol_f64_low_masked(0.55f * 16384)
    for invalid in ["", "MissingCloakEvent", "vnavupa"] {
        assert!(
            registered_entry(invalid, &registry).is_none(),
            "an invalid Voc identity must not become a raw-bag fallback: {invalid}"
        );
    }
}

fn test_decoded_audio() -> DecodedAudio {
    DecodedAudio {
        samples: vec![0.0, 0.0].into(),
        sample_rate: 22_050,
        channels: 2,
        native_channels: 1,
        native_sample_bytes: 2,
    }
}

fn test_output_scales(
    sound_volume: f32,
    voice_volume: f32,
    sound_master_linear: i32,
    focus_output_scale: f32,
) -> SfxOutputScales {
    SfxOutputScales {
        sound_volume,
        voice_volume,
        sound_master_linear,
        eva_master_linear: VOLUME_SCALE,
        focus_output_scale,
        // The limiter idles at full scale until the summed authored
        // `Volume=` budget of the live events passes 100.
        many_sounds_linear: VOLUME_SCALE,
    }
}

#[test]
fn gsi_01_02_focus_gate_restores_distinct_live_sfx_gains_absolutely() {
    let inactive = test_output_scales(1.0, 0.4, VOLUME_SCALE, 0.0);
    let quiet = prepare_normal_sfx_output(test_decoded_audio(), 3277, inactive);
    let loud = prepare_normal_sfx_output(test_decoded_audio(), 12288, inactive);

    assert_eq!(quiet.initial_volume, 0.0);
    assert_eq!(loud.initial_volume, 0.0);
    let active = test_output_scales(1.0, 0.4, VOLUME_SCALE, 1.0);
    assert_eq!(quiet.gain.effective(active), native_volume_amplitude(3277));
    assert_eq!(loud.gain.effective(active), native_volume_amplitude(12288));
}

#[test]
fn gsi_01_02_sfx_armed_while_inactive_retains_gain_for_restore() {
    // Construction while inactive still creates/starts the Player in the
    // production path; only this independently retained output gain is 0.
    let armed_while_inactive = prepare_normal_sfx_output(
        test_decoded_audio(),
        5734,
        test_output_scales(1.0, 0.4, VOLUME_SCALE, 0.0),
    );

    assert_eq!(armed_while_inactive.initial_volume, 0.0);
    assert!(
        (armed_while_inactive
            .gain
            .effective(test_output_scales(1.0, 0.4, VOLUME_SCALE, 1.0))
            - native_volume_amplitude(5734))
        .abs()
            < f32::EPSILON
    );
}

/// The user master is chained into the linear product before the
/// DirectSound curve, so half master is -10 dB, not half amplitude.
#[test]
fn options_profile_production_routes_sound_and_eva_independently() {
    let half = native_volume_amplitude(8192);
    for (sound_volume, voice_volume, expected_sound, expected_voice) in
        [(0.0, 1.0, 0.0, half), (1.0, 0.0, half, 0.0)]
    {
        let scales = test_output_scales(sound_volume, voice_volume, VOLUME_SCALE, 1.0);
        let sound = prepare_normal_sfx_output(test_decoded_audio(), 8192, scales);
        let eva = prepare_eva_stream_output(test_decoded_audio(), 8192, scales);

        assert_eq!(sound.initial_volume, expected_sound);
        assert_eq!(eva.initial_volume, expected_voice);
    }
    let full = prepare_normal_sfx_output(
        test_decoded_audio(),
        VOLUME_SCALE,
        test_output_scales(0.5, 1.0, VOLUME_SCALE, 1.0),
    );
    assert_eq!(full.initial_volume, half);
}

/// A queued EVA node carries only the entry and its sample name; the
/// sample is resolved and the Voice master applied when `PlayNextQueued`
/// starts it, so the scales at dequeue alone decide the startup volume.
#[test]
fn options_profile_queued_eva_uses_current_voice_master_at_dequeue() {
    let started_with_voice_enabled = prepare_eva_stream_output(
        test_decoded_audio(),
        13107,
        test_output_scales(0.0, 1.0, VOLUME_SCALE, 1.0),
    );
    assert!(
        (started_with_voice_enabled.initial_volume - native_volume_amplitude(13107)).abs()
            < f32::EPSILON
    );
    let started_with_voice_muted = prepare_eva_stream_output(
        test_decoded_audio(),
        13107,
        test_output_scales(1.0, 0.0, VOLUME_SCALE, 1.0),
    );
    assert_eq!(started_with_voice_muted.initial_volume, 0.0);
}

/// The scenario exits fade the effects master `[0x0087E758]` to zero
/// before queueing their announcement (`0x00686605` then `0x00686616`).
/// That master is `ch+0x9C` of Voc channels only, inside the linear
/// product before the DirectSound curve (`0x00402220` -> `0x0040A6D0`): a
/// quarter master on a 0.8 sound is table index 19 (-23.96 dB), not a
/// quarter of index 79's amplitude. The EVA stream and the many-sounds
/// scaler do not take it.
#[test]
fn scenario_exit_master_fade_leaves_the_eva_stream_alone() {
    let full = native_volume_amplitude(13107);
    let faded = test_output_scales(1.0, 1.0, 4096, 1.0);
    let voice = prepare_normal_sfx_output(test_decoded_audio(), 13107, faded);
    assert_eq!(voice.initial_volume, native_volume_amplitude(3276));
    assert!((20.0 * voice.initial_volume.log10() - -23.96).abs() < 0.01);
    let eva = prepare_eva_stream_output(test_decoded_audio(), 13107, faded);
    assert!((eva.initial_volume - full).abs() < f32::EPSILON);

    let silenced = test_output_scales(1.0, 1.0, 0, 1.0);
    assert_eq!(
        prepare_normal_sfx_output(test_decoded_audio(), 13107, silenced).initial_volume,
        0.0
    );
    assert!(
        (prepare_eva_stream_output(test_decoded_audio(), 13107, silenced).initial_volume - full)
            .abs()
            < f32::EPSILON
    );

    let limited = SfxOutputScales {
        many_sounds_linear: VOLUME_SCALE / 2,
        ..test_output_scales(1.0, 1.0, VOLUME_SCALE, 1.0)
    };
    assert!(prepare_normal_sfx_output(test_decoded_audio(), 13107, limited).initial_volume < full);
    assert!(
        (prepare_eva_stream_output(test_decoded_audio(), 13107, limited).initial_volume - full)
            .abs()
            < f32::EPSILON
    );

    // The foreground gate is the primary buffer: it still silences both.
    let background = test_output_scales(1.0, 1.0, VOLUME_SCALE, 0.0);
    assert_eq!(
        prepare_eva_stream_output(test_decoded_audio(), 13107, background).initial_volume,
        0.0
    );
}

#[test]
fn the_taunt_stream_takes_no_master_and_no_exit_fade() {
    let silent_masters = test_output_scales(0.0, 0.0, 0, 1.0);
    let gain = SfxOutputGain::new(VOLUME_SCALE, SfxChannel::TauntStream);
    assert_eq!(
        gain.effective(silent_masters),
        native_volume_amplitude(VOLUME_SCALE)
    );
    // The foreground gate is the primary buffer: it still silences it.
    assert_eq!(
        gain.effective(test_output_scales(1.0, 1.0, VOLUME_SCALE, 0.0)),
        0.0
    );
}

/// Original action21 lookup/admission controls, through the process's
/// production retail audio reader and the actual Sfx admission owner.
/// The captured stream is paused; no hardware completion is inferred.
#[test]
fn retail_trigger_indices_and_named_eva_share_sfx_admission_including_empty_columns() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    assert_eq!(definitions.eva().len(), 470);
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in rows
        .iter()
        .filter(|row| row["case_kind"] == "eva_read_dispatch")
    {
        let index = row["value"].as_i64().unwrap() as i32;
        let name = row["raw"].as_str().unwrap().split(',').nth(2).unwrap();
        let after = &row["after"];
        let expected_count = after["critical"].as_array().unwrap().len()
            + after["interrupts"].as_array().unwrap().len()
            + usize::from(!after["pending"].is_null())
            + (0..4)
                .map(|priority| {
                    after["queues"][priority.to_string()]
                        .as_array()
                        .unwrap()
                        .len()
                })
                .sum::<usize>();
        assert!(expected_count <= 1);
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[Sides]\nGDI=ONE\nNod=TWO\nThirdSide=THREE\n",
            ))
            .unwrap();
        for side_index in 0..3 {
            player.set_eva_side(side_index, &rules);
            player.stop_all();
            player.set_paused(false, 1);
            player.set_paused(true, 1);
            assert_eq!(
                player.queue_eva(
                    index,
                    Some(EvaType::Interrupt),
                    None,
                    definitions.eva(),
                    &assets,
                    None
                ),
                expected_count != 0,
                "{} side{side_index}",
                row["raw"],
            );
            assert_eq!(player.vox.queued_count(), expected_count);
            assert!(player.eva_player.is_none());
            // The named route resolves to the same entry already queued
            // by its index, so its duplicate call cannot create a copy.
            assert!(!player.play_eva(
                name,
                Some(EvaType::Interrupt),
                definitions.eva(),
                &assets,
                None
            ));
            assert_eq!(player.vox.queued_count(), expected_count);
            player.set_paused(false, 1);
            if expected_count != 0 {
                let node = player.vox.take_next(1, false).unwrap();
                assert_eq!(
                    node.event,
                    row["entry"]["name"].as_str().unwrap().to_ascii_uppercase()
                );
                assert_eq!(node.eva_type, EvaType::Interrupt);
                assert_eq!(
                    node.priority as i32,
                    row["entry"]["priority"].as_i64().unwrap() as i32
                );
                assert_eq!(
                    player
                        .vox
                        .sample_for(&node, definitions.eva())
                        .unwrap_or(""),
                    row["entry"]["columns"][side_index.to_string()]
                        .as_str()
                        .unwrap()
                );
            }
            assert_eq!(player.vox.queued_count(), 0);
            assert!(player.eva_player.is_none());
            compared += 1;
        }
    }
    assert_eq!(compared, 45);
}

// One test per master setting, so their real-time playbacks overlap.
#[test]
fn reached_unit_voice_uses_sound_master_with_sound_muted() {
    assert_reached_unit_voice_uses_sound_master(1_000, 0.0, 1.0, false);
}

#[test]
fn reached_unit_voice_uses_sound_master_with_voice_muted() {
    assert_reached_unit_voice_uses_sound_master(2_000, 1.0, 0.0, true);
}

/// The original ordinary event binds channel+94 to SoundVolume at
/// 405B91..405B96, also for Techno's unity-gain call at6F9EE5..6F9EF0.
/// Exercise actual queued/reached playback, live setters and device pulls
/// with opposite masters; equal defaults would hide a wrong group.
fn assert_reached_unit_voice_uses_sound_master(
    now: u64,
    sound_volume: f64,
    voice_volume: f64,
    audible: bool,
) {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let index = loaded_index.map(|loaded| &loaded.index);
    let mut service = TestAudioService::default();
    player.set_sound_volume(sound_volume);
    player.set_voice_volume(voice_volume);
    player
        .observe_pcm(PcmObservationConfig {
            sound_ids: vec!["GIMove".into()],
            max_events: 1,
            max_samples_per_event: 262_144,
        })
        .unwrap();
    player.queue_unit_voice(7, "GIMove");
    player.visit_unit_voice(7, definitions.sounds());
    service.pump(
        &mut player,
        now,
        definitions.sounds(),
        definitions.eva(),
        &assets,
        index,
    );
    assert!(player.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
    assert_eq!(
        player.eva_player.as_ref().unwrap().player.volume() > 0.0,
        voice_volume > 0.0,
        "EVA retains its independent VoiceVolume group"
    );
    let event = player
        .arbiter
        .validate_loop_handle(HandleOwner::UnitVoice(7))
        .expect("reached head has an admitted handle");
    let started_gain = player.live[&event].player.volume();
    assert_eq!(
        started_gain > 0.0,
        audible,
        "unit voice must use SoundVolume={sound_volume}, not VoiceVolume={voice_volume}"
    );
    player.set_voice_volume(1.0 - voice_volume);
    assert_eq!(player.live[&event].player.volume(), started_gain);
    assert_eq!(
        player.eva_player.as_ref().unwrap().player.volume() > 0.0,
        voice_volume == 0.0
    );
    player.set_voice_volume(voice_volume);
    if audible {
        player.set_sound_volume(0.0);
        assert_eq!(player.live[&event].player.volume(), 0.0);
        player.set_sound_volume(sound_volume);
        assert_eq!(player.live[&event].player.volume(), started_gain);
    }
    player.live[&event].player.sleep_until_end();
    player.report_finished_outputs();
    let report = player.finish_pcm_observation().unwrap();
    assert!(!report.truncated);
    assert_eq!(report.outputs.len(), 1);
    let output = &report.outputs[0];
    assert!(output.sample_bits.len() >= output.source_sample_count);
    assert!(output.source_sample_count > 0);
    assert!(
        output
            .sample_bits
            .iter()
            .all(|bits| f32::from_bits(*bits).is_finite())
    );
    let nonzero = output
        .sample_bits
        .iter()
        .filter(|bits| f32::from_bits(**bits) != 0.0)
        .count();
    assert_eq!(nonzero > 0, audible, "post-Player device pulls");
    eprintln!(
        "GI SoundVolume={sound_volume} VoiceVolume={voice_volume}: {} pulls, {nonzero} nonzero",
        output.sample_bits.len()
    );
    player.stop_all();
    // The same player re-arms observation and admits the voice again
    // after the hard stop.
    player
        .observe_pcm(PcmObservationConfig {
            sound_ids: vec!["GIMove".into()],
            max_events: 1,
            max_samples_per_event: 262_144,
        })
        .unwrap();
    player.queue_unit_voice(7, "GIMove");
    player.visit_unit_voice(7, definitions.sounds());
    service.pump(
        &mut player,
        now + 1_000,
        definitions.sounds(),
        definitions.eva(),
        &assets,
        index,
    );
    assert!(
        player
            .arbiter
            .validate_loop_handle(HandleOwner::UnitVoice(7))
            .is_some()
    );
    player.stop_all();
}

/// A radar movie's hold (`0x005BF580`) pauses the EVA stream and leaves a
/// playing Voc sound running; only full-screen `Play_Movie` also
/// suspends it (`0x00406EA0`).
#[test]
fn movie_hold_pauses_eva_but_not_a_playing_sound() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let index = loaded_index.map(|loaded| &loaded.index);
    let mut service = TestAudioService::default();
    player.set_volume(0.0);
    player.queue_unit_voice(7, "GIMove");
    player.visit_unit_voice(7, definitions.sounds());
    service.pump(
        &mut player,
        1_000,
        definitions.sounds(),
        definitions.eva(),
        &assets,
        index,
    );
    let event = player
        .arbiter
        .validate_loop_handle(HandleOwner::UnitVoice(7))
        .expect("reached head has an admitted handle");
    assert!(player.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));

    player.set_movie_held(true, 1_000);
    assert!(!player.live[&event].player.is_paused());
    assert!(player.eva_player.as_ref().unwrap().player.is_paused());
    player.set_movie_sounds_suspended(true, 1_000);
    assert!(player.live[&event].player.is_paused());
    player.stop_all();
}

/// A movie's stop hold (`+0x88`) refuses `StreamPlayer::Stop @
/// 0x00407F40`, so ChronoScreen's EVA stop (`0x007529A0`) leaves the
/// paused line; paused, it does not count as playing (`0x00408070`).
#[test]
fn movie_hold_keeps_the_paused_eva_line_through_a_stop() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = output_player() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let index = loaded_index.map(|loaded| &loaded.index);
    player.set_volume(0.0);
    player.set_paused(false, 1_000);
    assert!(player.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
    player.set_movie_held(true, 1_000);
    assert!(!player.voices_active());
    player.stop_eva(true);
    assert!(player.eva_player.is_some());
    player.set_movie_held(false, 1_000);
    assert!(player.voices_active());
    player.stop_eva(true);
    assert!(player.eva_player.is_none());
    player.stop_all();
}

/// `PlayFile @ 0x00407B60` does nothing under a movie's stop hold, and
/// otherwise sets the playing bit whatever the pause count `+0x80`: a
/// taunt sent during a game pause plays at once.
#[test]
fn a_taunt_waits_out_no_pause_but_a_movie_hold_drops_it() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = output_player() else {
        return;
    };
    player.set_volume(0.0);
    player.set_movie_held(true, 1_000);
    player.play_taunt(0x01, &assets, None);
    assert!(player.taunt_player.is_none());
    player.set_movie_held(false, 1_000);
    player.set_paused(true, 1_000);
    player.play_taunt(0x01, &assets, None);
    let taunt = player.taunt_player.as_ref().expect("retail TAUAM01.WAV");
    assert!(!taunt.player.is_paused());
    player.stop_all();
}

/// An output device, or `None` after a `SKIPPED` line: these tests need
/// a real stream to tell playing from paused.
fn output_player() -> Option<SfxPlayer> {
    let player = SfxPlayer::new();
    if player.is_none() {
        eprintln!("SKIPPED: no audio output device");
    }
    player
}

/// The radar pass lifts a suspend (`0x006539AE..0x006539B7`) only while
/// ChronoScreen does not hold the screen; ChronoScreen's Finish resumes
/// on its own (`0x0053B714`).
#[test]
fn the_radar_pass_lifts_an_eva_suspend_unless_chrono_screen_holds() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = output_player() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let eva = definitions.eva();
    player.set_volume(0.0);
    player.suspend_eva();
    player.set_chrono_screen_active(true);
    player.resume_eva_unless_chrono_screen();
    assert!(!player.play_eva("EVA_UnitReady", None, eva, &assets, None));
    player.set_chrono_screen_active(false);
    player.resume_eva_unless_chrono_screen();
    assert!(player.play_eva("EVA_UnitReady", None, eva, &assets, None));
    player.stop_all();
}

/// `-X…Q` (`0x00A8ED64`) stops `PlayNextQueued @ 0x00752760` before its
/// dequeue; queueing still succeeds.
#[test]
fn the_quiet_switch_queues_eva_but_never_plays_it() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = output_player() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let index = loaded_index.map(|loaded| &loaded.index);
    player.set_volume(0.0);
    player.set_paused(false, 1_000);
    player.set_eva_quiet(true);
    assert!(player.play_eva("EVA_UnitReady", None, definitions.eva(), &assets, index));
    assert!(player.eva_player.is_none());
    player.stop_all();
}

/// The done callback `0x00407A30` stamps the end time when the sample
/// runs out, so a pump that comes 600 ms late finds the 500 ms gap
/// (`0x00752794..0x007527D5`) already passed and starts the next line.
#[test]
fn a_late_pump_measures_the_eva_gap_from_the_real_end() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = output_player() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let index = loaded_index.map(|loaded| &loaded.index);
    let eva = definitions.eva();
    player.set_volume(0.0);
    let started = crate::util::time::Instant::now();
    player.set_paused(false, 1_000);
    assert!(player.play_eva("EVA_UnitReady", None, eva, &assets, index));
    assert!(player.play_eva("EVA_ConstructionComplete", None, eva, &assets, index));
    assert_eq!(player.current_eva_event(), Some("EVA_UNITREADY"));
    player.eva_player.as_ref().unwrap().player.sleep_until_end();
    std::thread::sleep(std::time::Duration::from_millis(600));
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap();
    player.set_paused(false, 1_000 + elapsed);
    player.advance_voice_queue(eva, &assets, index);
    assert_eq!(player.current_eva_event(), Some("EVA_CONSTRUCTIONCOMPLETE"));
    player.stop_all();
}

/// Original two-E1 QueueVoice708D90 -> AI6F9EBB -> PlayAtPos750920
/// admits two distinct state-0 SoundEvents before either sample starts.
#[test]
fn two_e1_acknowledgements_hold_two_shared_pool_events() {
    let Some(definitions) = crate::rules::retail_ini_fixture::retail_audio_definitions() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    player.set_volume(0.0);
    player.queue_unit_voice(3, "GIMove");
    player.queue_unit_voice(2, "GIMove");
    player.visit_unit_voice(2, definitions.sounds());
    player.visit_unit_voice(3, definitions.sounds());
    assert_eq!(
        player.arbiter.live_event_count(),
        2,
        "both Techno+4DC handles must own shared-pool events before service"
    );
    assert_eq!(player.arbiter.busy_channel_count(), 0);
    player.stop_all();
}

/// `UnitClass::Deploy @ 0x00739971..0x007399B2` gives the building the
/// unit's voice handle and latch before the unit's destructor
/// (`0x006F4607` -> `0x00405C00`) runs, so the line an MCV is speaking
/// plays on and a held line still waits behind it.
#[test]
fn a_deploy_conversion_keeps_the_speaking_units_line() {
    let Some(definitions) = crate::rules::retail_ini_fixture::retail_audio_definitions() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    player.set_volume(0.0);
    player.queue_unit_voice(3, "GISelect");
    player.visit_unit_voice(3, definitions.sounds());
    player.queue_unit_voice(3, "GIMove");
    player.visit_unit_voice(3, definitions.sounds());
    let line = player
        .arbiter
        .validate_loop_handle(HandleOwner::UnitVoice(3));
    assert!(line.is_some());

    player.move_unit_voice(3, 9);
    player.destroy_unit_voice(3);

    assert_eq!(
        player
            .arbiter
            .validate_loop_handle(HandleOwner::UnitVoice(9)),
        line,
        "the unit's destructor must not stop the line the building took"
    );
    assert_eq!(player.voice_queue.playing_for(9), Some("GISELECT"));
    assert_eq!(player.voice_queue.pending_for(9), Some("GIMOVE"));
    assert_eq!(player.pending_unit_voice_owners(), BTreeSet::from([9]));
    player.stop_all();
}

/// A unit acknowledgement starting while an announcement sounds leaves
/// the announcement playing: native's `StreamPlayer` is not one of the Voc
/// channels `VocClass::PlayAtPos @ 0x00750920` takes, and only
/// `VoxClass` itself (`QueueVoice` type 2, `ResetAll`) stops it. Needs
/// retail assets and an audio device; skips without either.
#[test]
fn unit_voice_does_not_cut_eva_stream() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    player.set_volume(0.0);
    let mut service = TestAudioService::default();
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let audio_index = loaded_index.map(|loaded| &loaded.index);
    // `PlayNextQueued` needs `now > end_time + gap`, so the clock must
    // have moved off zero before the first line can start.
    service.pump(
        &mut player,
        1_000,
        definitions.sounds(),
        definitions.eva(),
        &assets,
        audio_index,
    );
    assert!(player.play_eva(
        "EVA_UnitReady",
        None,
        definitions.eva(),
        &assets,
        audio_index
    ));
    assert!(player.eva_stream_busy());
    let eva_current = player.vox.current().cloned();
    assert!(eva_current.is_some());

    player.queue_unit_voice(7, "GISelect");
    player.visit_unit_voice(7, definitions.sounds());
    assert!(
        player
            .arbiter
            .validate_loop_handle(HandleOwner::UnitVoice(7))
            .is_some()
    );
    assert_eq!(player.voice_queue.playing_for(7), Some("GISELECT"));
    assert!(player.eva_stream_busy(), "unit voice cut the EVA stream");
    assert_eq!(player.vox.current().cloned(), eva_current);
    assert!(player.voices_active());

    // The queue's own stop path is the only one that cuts the stream, and
    // it leaves the unit line alone.
    player.stop_eva_stream();
    assert!(!player.eva_stream_busy());
    assert!(
        player
            .arbiter
            .validate_loop_handle(HandleOwner::UnitVoice(7))
            .is_some()
    );
    player.stop_all();
}

/// The time jump's `Speech__StopPlayback` (`0x007529A0`, from
/// `0x0053B508`) stops a playing line; `StreamPlayer::Stop` stamps the
/// stop moment as its end (`0x00407A60`), so a line queued next starts
/// once `PlayNextQueued`'s 500 ms gap has run (`0x007527A7..0x007527CF`).
/// SOV01's radar-movie announcement waited for the next jump without it.
/// Needs retail assets and an audio device; skips without either.
#[test]
fn stopped_eva_line_lets_the_queue_resume_after_the_gap() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    player.set_volume(0.0);
    let mut service = TestAudioService::default();
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let audio_index = loaded_index.map(|loaded| &loaded.index);
    let mut pump = |player: &mut SfxPlayer, now| {
        service.pump(
            player,
            now,
            definitions.sounds(),
            definitions.eva(),
            &assets,
            audio_index,
        );
    };
    pump(&mut player, 1_000);
    assert!(player.play_eva(
        "EVA_UnitReady",
        None,
        definitions.eva(),
        &assets,
        audio_index
    ));
    assert!(player.eva_stream_busy());
    pump(&mut player, 1_100);
    player.stop_eva(true);
    assert!(player.play_eva(
        "EVA_LowPower",
        None,
        definitions.eva(),
        &assets,
        audio_index
    ));
    pump(&mut player, 1_600);
    assert!(player.vox.current().is_none(), "the gap runs from the stop");
    pump(&mut player, 1_700);
    assert_eq!(player.current_eva_event(), Some("EVA_LOWPOWER"));
    player.stop_all();
}

/// Whole stock UnitReady waveform from original40AA70 callback returns,
/// literal sample writes and409DE0 copy ranges. The portable factory oracle
/// selects those bytes once; this drives the actual EVAMD/MIX/index/decoder
/// path before presentation gain, with no second decoder or Rust golden.
#[test]
fn retail_unit_ready_pcm_matches_original_decode_and_buffer_writes() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let fixture = crate::rules::retail_ini_fixture::factory_unit_ready_native();
    let native = &fixture["controls"]["buffer"]["decoded_pcm"];
    let selected = &fixture["controls"]["buffer"]["registered_prior"]["notification_suffix"][0]["after"]
        ["selected"][0];
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let entry = definitions.eva().entry("EVA_UnitReady").unwrap();
    let sample = entry.column(EvaSide::Allied).unwrap();
    assert_eq!(sample, selected["allied"].as_str().unwrap());
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let decoded = load_sfx(sample, &assets, loaded_index.map(|loaded| &loaded.index))
        .expect("production stock UnitReady decoder");
    assert_eq!(
        u64::from(decoded.sample_rate),
        native["samples_per_second"].as_u64().unwrap()
    );
    assert_eq!(native["channels"], 1, "original mono source");
    assert_eq!(native["sample_bytes"], 2, "original signed PCM16 output");
    assert_eq!(decoded.channels, 2, "presentation upmix");
    assert_eq!(decoded.samples.len() % 2, 0);
    // The production owner represents each i16 exactly as f32/32768 and
    // duplicates mono into stereo. Undo only that lossless representation;
    // the expected length and every expected sample remain native outputs.
    let actual: Vec<u8> = decoded
        .samples
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|pair| {
            assert_eq!(pair[0].to_bits(), pair[1].to_bits(), "mono upmix");
            let scaled = pair[0] * 32768.0;
            let sample = scaled as i16;
            assert_eq!(scaled, f32::from(sample), "lossless PCM representation");
            sample.to_le_bytes()
        })
        .collect();
    let hex = native["bytes_hex"].as_str().unwrap();
    assert_eq!(hex.len() % 2, 0);
    let expected: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
        .collect();
    assert_eq!(actual.len() as u64, native["bytes"].as_u64().unwrap());
    assert_eq!(actual, expected, "all original callback/copy PCM bytes");
    assert_eq!(
        crate::util::sha256::sha256_hex(&actual),
        native["sha256"].as_str().unwrap()
    );
}

#[test]
#[ignore = "requires active-retail RA2_DIR and an audio output device"]
fn retail_power_on_position_stop_precedes_one_shot_release() {
    use std::time::{Duration, Instant};

    // Building43FC34 calls750D40 before destructor43BD1A Release. The
    // former hard-stops a live positional one-shot at zero gain; the
    // latter alone leaves its current buffer playing (406096..4060E7).
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions().unwrap();
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let audio_index = loaded_index.map(|loaded| &loaded.index);
    let entry = definitions.sounds().get("PowerOn").unwrap();
    assert_eq!(entry.control, 0);
    assert_eq!(entry.sounds, ["gpowon"]);
    let mut player = SfxPlayer::new().expect("selected regression requires audio output");
    let mut service = TestAudioService::default();
    let epoch = Instant::now();
    for position_first in [false, true] {
        player.stop_all();
        player
            .observe_pcm(PcmObservationConfig {
                sound_ids: vec!["PowerOn".into()],
                max_events: 1,
                max_samples_per_event: 262_144,
            })
            .unwrap();
        assert!(player.play_animation_sound_spatial(
            7,
            "PowerOn",
            SpatialGain::CENTRED_FULL,
            definitions.sounds(),
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            service.pump(
                &mut player,
                epoch.elapsed().as_millis() as u64,
                definitions.sounds(),
                definitions.eva(),
                &assets,
                audio_index,
            );
            if player.pcm_observer.as_ref().unwrap().pulled_nonzero() {
                break;
            }
            assert!(Instant::now() < deadline, "the device did not pull PowerOn");
            std::thread::sleep(Duration::from_millis(10));
        }
        let before = player.pcm_observer.as_ref().unwrap().recorded_samples();
        if position_first {
            player.update_looping_sound(7, None, definitions.sounds());
        }
        player.release_animation_sound(7);
        assert!(player.handle_sound_id(7).is_none());
        while !player.observed_pcm_settled() {
            service.pump(
                &mut player,
                epoch.elapsed().as_millis() as u64,
                definitions.sounds(),
                definitions.eva(),
                &assets,
                audio_index,
            );
            assert!(
                Instant::now() < deadline,
                "the device did not finish PowerOn"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let report = player.finish_pcm_observation().unwrap();
        assert!(!report.truncated);
        assert_eq!(report.outputs.len(), 1);
        let output = &report.outputs[0];
        assert!(output.source_ended && !output.truncated);
        assert_eq!(output.resolved_samples, ["gpowon"]);
        if position_first {
            assert!(
                output.sample_bits.len() < output.source_sample_count,
                "zero-gain position must cut the actual source short before Release"
            );
        } else {
            assert!(output.sample_bits.len() >= output.source_sample_count);
            assert!(
                output.sample_bits[before..]
                    .iter()
                    .any(|bits| f32::from_bits(*bits) != 0.0),
                "Release alone leaves the current one-shot audible"
            );
            assert_eq!(output.actions.last().unwrap().kind, "completed");
        }
        eprintln!(
            "PowerOn position_first={position_first}: before={before} pulled={} decoded={}",
            output.sample_bits.len(),
            output.source_sample_count
        );
    }
    player.stop_all();
}

#[test]
#[ignore = "requires active-retail RA2_DIR and an audio output device"]
fn retail_squid_one_shot_device_release_detach_and_hard_stop() {
    let mut service = TestAudioService::default();
    use std::io::Write;
    use std::time::{Duration, Instant};

    // Original gattling_loop shared one-shot controls establish Release
    // 406060 finishing the current buffer, while generic405FD0 requests
    // DriverStop at the next state3 service. Stop405D40 acts immediately.
    // This is the production device consumer, not a supplied native channel
    // callback or an invented idle-lapse trigger. Sample RNG stays untouched.
    let (_root, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("explicit device witness requires active-retail assets");
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let audio_index = loaded_index.map(|loaded| &loaded.index);
    let entry = definitions.sounds().get("SquidMove").unwrap();
    assert_eq!(entry.control, control::RANDOM);
    assert_eq!(entry.loop_count, 0);
    assert_eq!(entry.sounds, ["vsqumova", "vsqumovb"]);
    let mut player = SfxPlayer::new().expect("explicit witness requires an output device");
    let epoch = Instant::now();
    let mut reports = Vec::new();
    for operation in ["release", "detach", "hard_stop"] {
        player.stop_all();
        player
            .observe_pcm(PcmObservationConfig {
                sound_ids: vec!["SquidMove".to_owned()],
                max_events: 1,
                max_samples_per_event: 262_144,
            })
            .unwrap();
        assert!(player.play_animation_sound_spatial(
            7_u64,
            "SquidMove",
            SpatialGain::CENTRED_FULL,
            definitions.sounds()
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            service.pump(
                &mut player,
                epoch.elapsed().as_millis() as u64,
                definitions.sounds(),
                definitions.eva(),
                &assets,
                audio_index,
            );
            if player.pcm_observer.as_ref().unwrap().pulled_nonzero() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{operation}: real device did not pull nonzero PCM"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let before = player.pcm_observer.as_ref().unwrap().recorded_samples();
        match operation {
            "release" => player.release_animation_sound(7_u64),
            "detach" => player.detach_animation_sound(7_u64),
            _ => player.stop_animation_sound(7_u64),
        }
        assert!(player.handle_sound_id(7).is_none());
        while !player.observed_pcm_settled() {
            service.pump(
                &mut player,
                epoch.elapsed().as_millis() as u64,
                definitions.sounds(),
                definitions.eva(),
                &assets,
                audio_index,
            );
            assert!(
                Instant::now() < deadline,
                "{operation}: real device queue did not end"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let report = player.finish_pcm_observation().unwrap();
        assert!(!report.truncated);
        assert_eq!(report.outputs.len(), 1);
        let output = &report.outputs[0];
        assert!(output.source_ended && !output.truncated);
        assert_eq!(output.owner, Some(7));
        assert_eq!(output.resolved_samples.len(), 1);
        assert!(entry.sounds.contains(&output.resolved_samples[0]));
        assert!(
            output
                .sample_bits
                .iter()
                .all(|bits| f32::from_bits(*bits).is_finite())
        );
        if operation != "release" {
            assert!(
                output.sample_bits.len() < output.source_sample_count,
                "device continued the entire one-shot after {operation}"
            );
            assert!(
                matches!(output.actions.last().unwrap().kind, "stopped" | "completed"),
                "device completion and next state3 retirement may race"
            );
        } else {
            assert!(output.sample_bits.len() >= output.source_sample_count);
            assert!(
                output.sample_bits[before..]
                    .iter()
                    .any(|bits| f32::from_bits(*bits) != 0.0),
                "current playout must remain audible after {operation}"
            );
            assert_eq!(output.actions.last().unwrap().kind, "completed");
        }
        eprintln!(
            "SquidMove {operation}: sample={} before={before} pulled={} decoded={} source_ended={}",
            output.resolved_samples[0],
            output.sample_bits.len(),
            output.source_sample_count,
            output.source_ended
        );
        reports.push(
            serde_json::json!({"operation": operation, "samples_before_operation": before,
            "observation": report.into_json()}),
        );
    }
    player.queue_unit_voice(7, "SquidSelect");
    player.stop_all();
    assert!(player.voice_queue.pending_for(7).is_none());
    assert!(player.voice_queue.playing_for(7).is_none());
    assert!(player.looping_owners().is_empty());
    if let Some(path) = std::env::var_os("VERA20K_SFX_PCM_TEST_OUTPUT") {
        let path = std::path::PathBuf::from(path);
        assert!(
            path.is_absolute(),
            "witness output must name a scratch absolute path"
        );
        let bytes = serde_json::to_vec(
            &serde_json::json!({"schema_version": "vera20k.sfx-device-control.v1",
            "point": "post_player_pre_device_mixer", "controls": reports}),
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
    }
}

/// An above-floor entry through the production player and device. The
/// first start pass parks the event's sample record in its live output.
/// The ring's sample-end lookahead draws OnChannelSampleEnd's gap
/// (`0x00405AC0`). The drained output hands it to OnChannelDone
/// (`0x00405A00`), which leaves the event waiting in state 2 with the
/// record back in `pending`. The next start pass then plays the record's
/// next queued sample on the same token. Retail `_Amb_OceanLight` is
/// RANDOM|ALL with four samples, so its first pass leaves three.
/// Device-backed; ordinary lib CI has no device.
#[test]
#[ignore = "requires active-retail RA2_DIR and an audio output device"]
fn retail_above_floor_gap_parks_the_record_until_the_next_start_pass() {
    use std::time::{Duration, Instant};
    let mut service = TestAudioService::default();
    let (_root, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("explicit device witness requires active-retail assets");
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let audio_index = loaded_index.map(|loaded| &loaded.index);
    let sounds = definitions.sounds();
    let entry = sounds.get("_Amb_OceanLight").unwrap();
    assert_eq!(
        entry.control,
        control::RANDOM | control::LOOP | control::ALL | control::AMBIENT
    );
    assert_eq!((entry.delay_ms, entry.sounds.len()), ((2000, 4000), 4));
    let mut player = SfxPlayer::new().expect("explicit witness requires an output device");
    assert!(player.play_animation_sound_spatial(
        7_u64,
        "_Amb_OceanLight",
        SpatialGain::CENTRED_FULL,
        sounds
    ));
    let event = player
        .arbiter
        .validate_loop_handle(HandleOwner::Positional(7))
        .unwrap();
    let token = player.arbiter.event_token(event).unwrap();
    let epoch = Instant::now();
    let deadline = epoch + Duration::from_secs(40);
    let mut pump = |player: &mut SfxPlayer| {
        service.pump(
            player,
            epoch.elapsed().as_millis() as u64,
            sounds,
            definitions.eva(),
            &assets,
            audio_index,
        );
        assert!(Instant::now() < deadline, "the device did not progress");
        std::thread::sleep(Duration::from_millis(5));
    };
    let parked_queue = |player: &SfxPlayer| {
        let output = &player.live[&event];
        assert_eq!(output.token, Some(token));
        output.parked.as_ref().unwrap().queue.count()
    };

    while !player.live.contains_key(&event) {
        pump(&mut player);
    }
    assert!(!player.pending.contains_key(&event));
    assert_eq!(parked_queue(&player), 3);

    while player.live.contains_key(&event) {
        pump(&mut player);
    }
    assert_eq!(
        player.arbiter.native_event_snapshot(event).unwrap()["state"],
        2,
        "OnChannelDone keeps the event through its gap"
    );
    assert_eq!(player.pending[&event].token, token);

    while !player.live.contains_key(&event) {
        pump(&mut player);
    }
    assert_eq!(parked_queue(&player), 2);
    player.stop_all();
}

/// Original AudioSystem406F70 with physical EVAMD DialogList62 and
/// ceva062.WAV: paused queue at 1000, Unpause753620, no dequeue at 1033,
/// actual PlayNext752760/PlayFile407B60 at 1034, payload completion,
/// the strict inter-line gap and cleanup. All compared outcomes come from
/// the factory oracle's single original-byte consumer selector.
/// Device-backed production consumer check; ordinary lib CI has no device.
#[test]
#[ignore = "requires active-retail RA2_DIR and an audio output device"]
fn retail_unit_ready_waits_for_the_native_periodic_audio_service() {
    let mut service = TestAudioService::default();
    let fixture = crate::rules::retail_ini_fixture::factory_unit_ready_native();
    let step = |control: &str, label: &str| {
        fixture["controls"][control]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["label"] == label)
            .expect("selected original consumer boundary")
    };
    // Adapt this control's explicit OS clock input to milliseconds. The
    // original clock initializer/division and its returned value are
    // independently retained in registered_prior; this supplies inputs,
    // never a playback, queue or completion answer.
    let now_ms = |row: &serde_json::Value| {
        let prior = &row["clock_prior"];
        assert_eq!(prior["frequency"], 1_000_000);
        prior["counter"].as_u64().unwrap() / 1_000
    };
    let compare = |player: &SfxPlayer, row: &serde_json::Value, registry: &EvaRegistry| {
        let after = &row["after"];
        let label = row["label"].as_str().unwrap();
        assert_eq!(
            player.vox.queued_count(),
            usize::from(after["pending_standard_is_set"].as_bool().unwrap()),
            "{label}: native pending slot"
        );
        assert_eq!(
            player.vox.current().is_some(),
            after["current_is_set"].as_bool().unwrap(),
            "{label}: native current entry"
        );
        assert_eq!(
            player.vox.dequeue_allowed(),
            after["pause_depth"] == 0,
            "{label}: native pause depth"
        );
        let stream_open = after["stream"]["flags"].as_u64().unwrap() != 0;
        assert_eq!(
            player.eva_player.is_some(),
            stream_open,
            "{label}: native stream"
        );
        let selected = &after["selected"][0];
        if let Some(current) = player.vox.current() {
            assert_eq!(
                current.event,
                selected["name"].as_str().unwrap().to_ascii_uppercase()
            );
            assert_eq!(
                player.vox.sample_for(current, registry).unwrap_or(""),
                selected["allied"].as_str().unwrap()
            );
        }
        // The stream is not a Voc channel: the shared Voc pool stays free.
        assert!(
            player.arbiter.live_event_count() == 0,
            "{label}: EVA line in the Voc pool"
        );
    };
    let selected = &fixture["controls"]["cadence"]["registered_prior"]["clock_steps"][0]["after"]["selected"]
        [0];
    let (_root, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("this explicit consumer check requires active-retail assets");
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    let loaded_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let audio_index = loaded_index.map(|loaded| &loaded.index);
    let entry = definitions.eva().entry("EVA_UnitReady").unwrap();
    assert_eq!(entry.column(EvaSide::Allied), Some("ceva062"));
    assert_eq!(entry.eva_type, EvaType::Standard);
    assert_eq!(entry.priority, crate::rules::sound_ini::EvaPriority::Low);
    assert_eq!(entry.name, selected["name"].as_str().unwrap());
    assert_eq!(
        entry.volume.to_bits(),
        selected["volume_f32_bits"].as_u64().unwrap() as u32
    );
    let (wav, source) = assets.get_with_source("ceva062.wav").unwrap();
    assert_eq!(wav.len(), 9354, "physical native consumer input");
    assert_eq!(
        crate::util::sha256::sha256_hex(&wav),
        "6567dae06a071ab4985cad0b20b0c2596f923baf04a6bec097c8a9f0d937ee67"
    );
    eprintln!(
        "UnitReady WAV source={source}, sha256={}",
        crate::util::sha256::sha256_hex(&wav)
    );

    let mut player = SfxPlayer::new().expect("this explicit check requires an audio device");
    // Keep the test's private output inaudible; it still creates the real
    // decoder, mixer Player and stream queue without changing app settings.
    player.set_voice_volume(0.0);
    let paused = step("cadence", "paused_whole_periodic_service_1000ms");
    player.set_paused(true, now_ms(paused));
    assert!(player.play_eva(
        "EVA_UnitReady",
        None,
        definitions.eva(),
        &assets,
        audio_index
    ));
    service.pump(
        &mut player,
        now_ms(paused),
        definitions.sounds(),
        definitions.eva(),
        &assets,
        audio_index,
    );
    compare(&player, paused, definitions.eva());
    let unpaused = step("cadence", "original_unpause_keeps_native_pending");
    player.set_paused(false, now_ms(unpaused));
    compare(&player, unpaused, definitions.eva());
    for label in [
        "whole_periodic_service_exact_33ms",
        "whole_periodic_service_34ms_real_stock_wave",
    ] {
        let native = step("cadence", label);
        service.pump(
            &mut player,
            now_ms(native),
            definitions.sounds(),
            definitions.eva(),
            &assets,
            audio_index,
        );
        compare(&player, native, definitions.eva());
    }
    // Let the actual stock decoder/mixer queue finish. The controlled
    // wall-clock input2089 matches the native completed-stream witness;
    // no simulated playback-completion answer is supplied to this owner.
    player.eva_player.as_ref().unwrap().player.sleep_until_end();
    assert!(!player.eva_stream_busy());
    // This test's clock is the native capture's, not real time: its
    // worker saw the end at the next 2089 ms visit, so drop the
    // real-time stamp rather than back-date into the synthetic clock.
    player.eva_ran_out = None;
    for label in [
        "original_payload_worker_2089ms",
        "original_payload_end_exact_gap_queue_visit",
    ] {
        let native = step("buffer", label);
        service.pump(
            &mut player,
            now_ms(native),
            definitions.sounds(),
            definitions.eva(),
            &assets,
            audio_index,
        );
        compare(&player, native, definitions.eva());
    }
    // The explicit voice wait-loop caller advances independently of the
    // periodic pass. Native752760 at2590 retires current/state2 even though
    // that is only1ms after the last periodic pass.
    let retired = step("buffer", "original_payload_end_after_gap_queue_visit");
    player.set_paused(false, now_ms(retired));
    assert!(!player.pump_and_check_voices(definitions.eva(), &assets, audio_index));
    compare(&player, retired, definitions.eva());
    player.stop_all();
    assert_eq!(player.vox.queued_count(), 0);
    assert!(player.vox.current().is_none());
    assert!(!player.voices_active());
    assert!(player.arbiter.live_event_count() == 0);
}

pub(super) fn build_test_wav(
    sample_rate: u32,
    bits: u16,
    channels: u16,
    samples: &[u8],
) -> Vec<u8> {
    let data_size: u32 = samples.len() as u32;
    let fmt_size: u32 = 16;
    let byte_rate: u32 = sample_rate * channels as u32 * bits as u32 / 8;
    let block_align: u16 = channels * bits / 8;
    let riff_size: u32 = 4 + (8 + fmt_size) + (8 + data_size);

    let mut wav: Vec<u8> = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&fmt_size.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(samples);
    wav
}

#[test]
fn test_decode_wav_16bit_mono() {
    // 4 samples of 16-bit mono silence — upmixed to 8 stereo f32 values.
    let pcm: Vec<u8> = vec![0, 0, 0, 0, 0, 0, 0, 0];
    let wav = build_test_wav(22050, 16, 1, &pcm);
    let decoded = decode_wav(&wav, "test.wav").expect("should decode");
    assert_eq!(decoded.sample_rate, 22050);
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.samples.len(), 8); // 4 mono → 4 stereo pairs
}

/// Build a `wFormatTag == 0x11` (IMA ADPCM) WAV around a raw block payload.
fn build_test_ima_wav(sample_rate: u32, channels: u16, block_align: u16, blocks: &[u8]) -> Vec<u8> {
    let data_size: u32 = blocks.len() as u32;
    let fmt_size: u32 = 20; // WAVEFORMATEX + cbSize(2) + wSamplesPerBlock(2)
    let riff_size: u32 = 4 + (8 + fmt_size) + (8 + data_size);
    let samples_per_block: u16 = 1 + (block_align - 4 * channels) * 2 / channels;

    let mut wav: Vec<u8> = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&fmt_size.to_le_bytes());
    wav.extend_from_slice(&0x0011u16.to_le_bytes()); // wFormatTag
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate / 2).to_le_bytes()); // nAvgBytesPerSec
    wav.extend_from_slice(&block_align.to_le_bytes()); // psVar6[6] @ 0x00408610
    wav.extend_from_slice(&4u16.to_le_bytes()); // wBitsPerSample
    wav.extend_from_slice(&2u16.to_le_bytes()); // cbSize
    wav.extend_from_slice(&samples_per_block.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(blocks);
    wav
}

/// The retail final block of AUDIOMD `ABIRJ01A` (bag offset 11,770,646 +
/// 28 x 512): the same bytes the bag path decodes, run through the WAV path.
const ABIRJ01A_TAIL_BLOCK: &[u8] = &[
    0xf8, 0xff, 0x01, 0x00, 0x17, 0xbc, 0x33, 0xa1, 0x1b, 0x00, 0x90, 0x3a, 0xb5, 0x2c, 0x92, 0x10,
    0xb2, 0x9c, 0x32, 0xb3, 0x2d, 0x21, 0xcb, 0x79, 0xa1, 0x0a, 0x31, 0xc0, 0x1a, 0x23, 0xbb, 0x4b,
    0xb2, 0x1b, 0x13, 0x11, 0xd9, 0x39, 0x92, 0xa9, 0x30, 0x90, 0x0a, 0x11, 0x11, 0x1a, 0x91, 0x09,
    0x11, 0x09, 0x00, 0x00,
];

#[test]
fn decode_wav_ima_adpcm_mono_matches_the_shared_block_decoder() {
    let wav = build_test_ima_wav(22050, 1, 52, ABIRJ01A_TAIL_BLOCK);
    let decoded = decode_wav(&wav, "ima.wav").expect("format tag 0x11 must decode");
    assert_eq!(decoded.sample_rate, 22050);
    assert_eq!(decoded.channels, 2, "mono is upmixed for playback");
    // 97 mono samples duplicated into stereo pairs.
    assert_eq!(decoded.samples.len(), 97 * 2);
    let want = crate::assets::ima_adpcm::decode_blocks(ABIRJ01A_TAIL_BLOCK, 1, 52);
    for (i, &s) in want.iter().enumerate() {
        let f = s as f32 / 32768.0;
        assert_eq!(decoded.samples[i * 2], f, "left at {i}");
        assert_eq!(decoded.samples[i * 2 + 1], f, "right at {i}");
    }
}

#[test]
fn decode_wav_ima_adpcm_stereo_keeps_the_channels_apart() {
    // Two channels whose preambles differ, so a collapsed or swapped
    // interleave cannot pass. 8-byte preamble pair + one 8-byte L/R group.
    let mut block: Vec<u8> = vec![0x00, 0x10, 0x00, 0x00, 0x00, 0xF0, 0x00, 0x00];
    block.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x00]);
    let wav = build_test_ima_wav(22050, 2, 16, &block);
    let decoded = decode_wav(&wav, "ima2.wav").expect("stereo IMA must decode");
    assert_eq!(decoded.channels, 2);
    // 1 preamble frame + 8 group frames = 9 frames.
    assert_eq!(decoded.samples.len(), 18);
    assert_eq!(decoded.samples[0], 0x1000 as f32 / 32768.0);
    assert_eq!(decoded.samples[1], -4096.0 / 32768.0);
    let want = crate::assets::ima_adpcm::decode_blocks(&block, 2, 16);
    let got: Vec<i16> = decoded
        .samples
        .iter()
        .map(|&f| (f * 32768.0) as i16)
        .collect();
    assert_eq!(got, want);
}
