# Optional Ares audio bags

`--features ares` extends the existing process audio owner to read `audio`,
`ares`, then `audio01` through `audio99`, at the existing early audio-selection
boundary. Without that compile feature, only the original `audio.idx`/`audio.bag`
pair and strict version1/version2 reader remain. No mod names, archive aliases,
resource extraction or asset payloads are part of this implementation.

## Native behavior established

The enrolled Ares DLL identity is pinned by
[`ares_prerequisites.ARES_SHA256`](ares_prerequisites.py) and recorded in
[`ares_audio_bags.json`](ares_audio_bags.json). Its `.syhks00` record replaces
original `4011C0` with `Audio_Load1005ADD0`, returning to `401578`.
The literal stems are lower-case `audio`, `ares` and `audio%02d`;
`1005AEF8` compares the loop counter against100. The executed order control
observes101 appends, including audio01 and audio99.

`AudioBag::Open1005AA00` opens `%s.idx` and `%s.bag` separately through CCFile.
Both must be readable; absent or short input rejects that pair and the outer
loop continues. A valid empty pair remains admitted. The existing early resolver
continues to select each file independently, before bulk mounts. This change
does not introduce a new archive search order or let later mounts supplement a
missing startup selection.

The installed reader tests **header DWORD0 ==1** at `1005AB3C` to choose32-byte
entries, otherwise36. It does not choose the stride from the version in DWORD1.
Native controls establish the difference: ordinary `GABA,1` with32-byte entries
is rejected, first DWORD1 with32-byte entries is accepted, and `GABA,77` with
36-byte entries is accepted. The shared parser therefore takes a layout policy;
the original parser keeps its existing strict version admission.

`AudioLuggage::Create1005A170` iterates bags in append order. Its map comparison
uses `_stricmp10003990 -> 7C8D20`; a later equal name replaces the metadata
pointer and backing file together at `1005A28E..1005A29C`. The final sorted table
uses the original `FindSampleIndex4015C0`. Rust retains source-relative offsets
and separate BAG buffers, with a private parallel source index constructed only
by `AudioIndex`. There is no concatenation, offset rebasing, second playback
index or fallback to an earlier bag after an overriding entry fails decoding.
Ares lookup uses the exact case-insensitive name; the pre-existing stock `.aud`
extension conveniences stay confined to stock indexes.

The upstream implementation corroborates these mechanisms:
[Ares Audio.cpp, pinned source](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Misc/Audio.cpp).
Installed instructions and executed controls take priority over source labels.

## Reproduction and coverage

Set local `VERA20K_ARES_DLL` and `VERA20K_GAMEMD_EXE`, then run:

```sh
python -m tools.rules_oracle.ares_audio_bags --check
```

The producer reuses `CountryFixture`/`Probe`; it adds no VM, INI parser or game
loader. All files and entry objects supplied to controls are synthetic. Saved
results contain no mod sound samples or local installation paths.

- One actual Audio_Load loop observes stem order and bounds; bounded formatting,
  Open and Create are explicit seams in this control.
- Nine actual Open controls cover missing members, empty input, mixed-case sort,
  short reads and the installed header-policy differences. CCFile, heap and
  bounded `%s` formatting are transport seams.
- Two actual Create controls and12 original Find calls cover case-insensitive
  duplicate replacement, ordering, source-file ownership and missing names.
  Pre-opened entry arrays and file tokens are supplied; native map traversal,
  comparisons, copying and Find execute.

Rust consumes these results in `assets::audio_bag::native_tests`. The connected
`assets::asset_manager::audio_bags_tests` uses the real startup resolver,
SoundRegistry and existing PCM decoder with synthetic archives. It covers all
four priority levels, correct overriding sample bytes and metadata, a cue in an
extra bag, missing base/partial/malformed pairs, and later bulk-mount exclusion.
The same integration tests run with and without the Ares feature. The shared
test IDX writer emits retail version2 bytes accepted by both native policies.

These controls do **not** demonstrate whole-game audio parity. Hook installation,
native VFS, output driver, audible mixing, sample scheduling/RNG and full startup
are outside the emulator controls. Existing fixture guards reject RNG and detach
entries; the change adds no simulation RNG, timer or detach calls. Selected native
text is checked unchanged. High-byte sample identifiers, duplicate names within
one BAG, giant malformed allocations and platform device-failure behavior are
not certified. Existing loose WAV/AUD playback and its registration differences
are unchanged and are separate from multiple-BAG lookup.

`ProcessAudioCatalog` remains immutable across maps; the same index binds every
playback consumer. Its source receipt reports the first admitted pair and all
additional pairs in append order. Feature-off JSON has its original shape.
Only the owner’s normal process retirement releases these retained buffers.
