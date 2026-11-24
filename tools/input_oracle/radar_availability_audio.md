# Ordinary radar availability audio

The opt-in `unit_voice_playback.py --radar-availability-audio` profile composes
the existing joined Engineer input/runtime fixtures, `VoiceHistory`,
`NativeAudioPlatform` and `NativePcmObserver`. Historical GI, feedback and
slave-miner payloads and their default preparation are independent.

The actual fixed SOUNDMD catalogue adds RadarOn and RadarOff through original
`7510D0`; original `7514D0` resolves their fixture-relative indices. Original
AudioIndex `4011C0`, its sort/lookup and the physical IDX/BAG own sample
admission. A SoundList key is never used as a runtime Voc index.

Rules constructor `665650` supplies the reference defaults. The selected
AudioVisual region `66AAC0..66AB53` executes both exact-case 128-byte string
reads and native catalogue lookups. It includes the adjacent retained `+6EC`
store and the next read's scheduled `push 128` before the Off store at
`66AB4D`. That explicit interior stop leaves one pushed word; it is not a
whole AudioVisual call. Controls retain cold missing/unknown, valid stock,
later missing/empty/unknown, case, capacity and accepted replacement history.

The new NARADR type uses original `45DD90`, the Radar/SpySat scalar blocks
`45FF53..45FF85` and the independent Strength block `5F94D3..5F94F3`.
These selected fields and references read RULESMD, optional LANGRULE,
MPBattleMD and the selected physical XMP03T4 map in order. Historical joined
preparation retains its Hills/E1 prerequisite reads. This profile does not
load NARADR graphics or establish full type loading or placement.

The runtime supplies an admitted provider, House membership, cached power
`1/1`, no radar outage, mark/discovery/limbo, online, mission, EMP and warp
priors. Original whole House `508DF0` performs its scan and locality gates;
`656DF0`, `656CB0` and `656BE0` own availability, mode and direction. One
history starts with mode0/phase0/availabilityfalse, as stored by `652960`;
another supplies ordinary mode1. Same-value, loss, reversal, remote-House
and native-read cold-unbound-reference controls share those owners.

Every On/Off call records its original centered pan, volume and NULL handle.
The ordinary cues are independent pooled one-shots; reversal does not bind
them to Radar `+14C0` or stop an earlier cue. Original audio service, the
retained device-worker context, Stop/endpoint and later service own sample
progress and retirement. Existing explicit OS counters/cursor quanta bound
the single-thread transport. No native admission result, PCM bytes, RNG result
or completion decision is supplied by the observer.

Each fixture keeps uninterrupted Main, Scenario and MapGen states after its
existing original seed31 setup. Full states use the shared digest-interning
format; requests and raw advances remain separate observations. Radar phase,
frame/timer/handle and House outage/dirty storage are retained at boundaries;
selected native writes are recorded. Ordinary radar frame advancement,
complete House/frame scheduling, provider construction/sale/power production,
SpySat, movie/deferred modes and hardware concurrency are outside this scope.

PCM projections identify each backend through its actual native event pointer
and serial, channel `+158`, cached sample and sorted AudioIndex entry. A reused
backend with another event lifetime fails rather than combining unrelated
callbacks. Callback counts, lengths and digests are observations of returned
bytes; sample identity is never inferred from those values. Existing decoder
comparisons own detailed codec arithmetic and literal PCM controls.

Use a new flat physical cache whose membership matches the payload manifest;
do not add files to a retained historical cache. The map is the actual
`multimd.mix` entry, not a rules projection from VERA. With the original
supported executable configured:

```sh
VERA20K_RADAR_AUDIO_INPUTS=/absolute/prepared/extract \
PYTHONDONTWRITEBYTECODE=1 python -m tools.input_oracle.unit_voice_playback \
  --radar-availability-audio --check
```

`VERA20K_RADAR_AUDIO_MAP` may select another file in that same cache; it
defaults to `XMP03T4.MAP`. `--write --output /absolute/scratch/reference.json`
creates a separate source-guarded payload and binary/command sidecar.
Execution logs and validation results stay outside the repository.
