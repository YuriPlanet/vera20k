# Stock SQD MoveSound production observations

`squid-move-sound.mpr` places a retail SQD and LCRF in Sleep on the unchanged
water terrain from the projectile-trailer fixture. The map supplies units and
lighting, with no object, weapon, audio or locomotor overrides. The Battle/rules
reader selects SQD `MoveSound=SquidMove`, `IdleRate=4`, `WalkRate=2`; those keys
are authored only in RULESMD for this fixture. Fixed SOUNDMD defines SquidMove
as a random one-shot (`Control=random`, no loop), with `vsqumova`/`vsqumovb`.
The selected retail audio index/bag is loaded by the existing app asset owner.

- `squid-move-sound-idle.json`: 40 exact steps, filtered SQD state and actual
  post-Player PCM, with a bounded audio-only completion tail.
- `squid-move-sound-load.json`: the same stock scene; literal Ctrl+Shift+M
  after capture step6, Ctrl+Shift+N after step40, 60 total captured steps.
  The explicit clock segment must show a successful rewind. The restored
  MoveSound state and subsequent ordinary object visits remain observable.
- `squid-move-sound-observer-off.json`: the idle profile without the audio
  observer/tail, for comparing shared state and final pixels. Different profile
  bytes mean the strict whole-run comparison command does not accept this pair;
  compare those declared shared fields from separately validated receipts.

Materialize each profile in scratch with `launch.selected_map_file` set to the
absolute authored map path. The production loader does not resolve these
repository-relative strings against the profile's directory. Keep the runtime
working directory separate from the retained output and use a fresh copy of
`config.toml`; production quicksave/quickload then uses only scratch `saves/`.
Unset inherited `RA2_DIR` for the sealed capture wrapper; its retail path comes
from that isolated configuration.

```sh
env -u RA2_DIR python -m tools.map_observation --profile /absolute/materialized-profile.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/fresh-runtime-directory --build-label CANDIDATE_LABEL \
  --output /absolute/new-retained-run
python -m tools.map_observation export-audio --run /absolute/new-retained-run \
  --output /absolute/new-audio-export
```

`tools.map_observation` resolves the label against its checkout, independently
of the isolated runtime cwd. The real output-device witness is post-Player,
before device resampling/summing, including possible mixer initialization
prefetch; no OS recording, physical-speaker or native PCM/audio-equivalence
claim is made. The older separate presentation SFX RNG is unchanged. Native
Foot/handle controls are documented in [foot_move_sound.md](../foot_move_sound.md).
These profiles do not exercise held Ship locomotion or infer release from Stop:
stationary SQD idle cadence keeps refreshing its latch.

The separately selected device consumer check uses the same production
`play_animation_sound_spatial`/release/detach/stop methods and actual device
queue pulls, after the selected cue has produced nonzero output:

```sh
VERA20K_SFX_PCM_TEST_OUTPUT=/absolute/new-scratch-device-control.json \
python -m tools.cargo_run -- test -p vera20k --lib \
  audio::sfx::tests::retail_squid_one_shot_device_release_detach_and_hard_stop \
  -- --ignored --nocapture
```

It requires active-retail `RA2_DIR` and a working audio output device. The output
file is optional, created exclusively, and belongs in scratch. This consumer
control establishes playout after an explicit handle operation; it does not
invent a production idle-lapse trigger or emulate native device callbacks.
