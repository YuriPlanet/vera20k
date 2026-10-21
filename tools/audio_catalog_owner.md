# Process audio catalog ownership

`ProcessAssets` selects immutable `AudioDefinitions` and one sample lookup
owner when it first acquires retail assets. Stock builds select one BAG pair;
`--features ares` admits the optional ordered multi-BAG catalog described in
[the bounded Ares evidence](rules_oracle/ares_audio_bags.md).
`NativeRulesProcessOwner` binds sound
references through the exact same `Arc<SoundRegistry>` as app playback. Native
Rules remains resident while the asset manager is leased. Map success, failure,
cancellation and replacement retain the catalogs; `MatchAudioState` still owns
and resets match events. Output players, Theme state and channel/RNG scheduling
remain with their existing audio owners.

Assetless startup retains its launch index policy. The first recovered manager
initializes catalogs and missing native Rules **before** request preparation.
If Rules selection fails, the manager returns through normal job retirement and
selected audio remains resident. Empty/missing audio counts as selected, so a
later map cannot replace it. A lost manager lease never resets live Rules.

## Native evidence and bounds

These lifecycle/source decisions are established from original instructions,
not from executing Windows startup or interpreting Ghidra labels:

- `48CCC0` calls `Init_Game52BA60` at `48CCCF`. Its successful ordinary
  shell/scenario route reaches `52D9A0` at `48CDD3`; returning from a game
  reaches `52D9A0` at `48CFAA` and loops `48CDE5`, without repeating Init_Game.
- Init_Game opens SOUNDMD.INI (`825E50`), calls physical INI wrapper `4741F0`
  at `52C763`, clears sounds through `7513F0`, and reads the list through
  `7510D0` at `52C796`. It then opens EVAMD.INI (`825DF0`), uses the wrapper
  at `52C843`, clears through `7531A0`, and reads `753000` at `52C8A0`.
  Wrapper `47422B` calls physical reader `525A60`. Neither path opens the RA2
  base INI. Rust uses the existing `select_ini -> IniFile::from_bytes` owner.
- `7510D0` reads Defaults, then ordered SoundList entries; duplicate names reuse
  the case-insensitive entry and call `750440` again. Registry reader behavior
  is unchanged by sharing its allocation.
- Startup `52BBC7 -> 406B10` receives the audio flag. `-noaudio` clears it at
  `52F6DA`. Disabled/device-failure paths skip index construction but join
  `406CF8 -> 403ED0`, which sets the factory-ready flag `87E2A0` at `403F1B`.
  Definitions still exist; `4064A0` skips admitted sample slots if the
  AudioIndex pointer `87E294` is null. Rust lexical names are not admitted slots.
- `406C43` probes AUDIOMD.MIX and mounts either it (`406CAB`) or AUDIO.MIX
  (`406CCF`). It constructs **one** index (`4011C0`) at `406CE8` with basename
  `audio`. This is archive fallback, not per-sample fallback.
- `401262/40126C` selects `audio.idx`; `4012DB/4012EF` independently selects
  `audio.bag` through CCFile. Actual read-open `473D10` tries readable raw
  files first (`473D2D`), then MIX lookup (`473D68 -> 5B4430`), whose first
  CRC hit wins in registration order. MIX construction appends at
  `5B3DE2..5B3E00`. Availability checks alone do not establish read priority.
- These files are selected after early archives (`52BB64 -> 5301A0`) and the
  chosen audio mount, before bulk archives (`52C59C -> 530460`). AssetManager
  freezes its existing resolver's two winners at this boundary; it does not
  maintain another search list. Retained handles survive map archive changes.
  Loose winners use the manager's existing snapshot storage. Enabled index
  loading parses IDX and copies BAG once into the retained AudioIndex;
  disabled loading does not make that index-owned copy.
- Missing/short input in `4011C0` reaches cleanup `401506` and null `40156C`;
  lookup `4015C0` returns -1 for a missing sample without another index.
  The constructed index retains BAG for `4016F0`. Constructor EDX=0 leaves
  optional per-sample directory fallback disabled (`4013C8..4013CE`).

Reinspect using the indexed [native inspector](native_inspect.md), e.g.
`python -m tools.native_inspect disasm 0x48CCC0 --bytes 524`, and the bounded
functions/addresses above. A direct-call sweep is not proof of universal
unreachability: indirect calls and undecoded bytes remain. Current physical
retail IDX and BAG both resolve to `langmd.mix -> audiomd.mix`.

### Retained differences and exclusions

Missing SOUNDMD/EVAMD fails native startup. VERA retains its tolerant empty
catalog policy, including disabled-output diagnostics and assetless recovery.
An enabled launch still selects a Rust index if output construction fails;
native skips its index on device failure. No device-output parity is claimed.
Existing standalone WAV/AUD playback fallback remains; the removed behavior is
second-index/base-INI fallback. Malformed IDX semantics, Windows search-directory
population, waveform mixing, full EVA reader equivalence, and audible output
are not certified by this change. RNG, timer, detach and event-ordering code is
unchanged; match reset remains at its existing boundary. Removing an invalid
fallback can suppress playback and its downstream presentation RNG work. That
case follows native source admission, but its full audio RNG trace is not
executed by the selected native witness; do not infer stream equivalence.

## Reproduction and validation

Use [the shared build runner](README.md) with retail flags for Rust tests.
Relevant owners are `rules::audio_sources::tests`, `app::process_assets::tests`,
`app::loading::pump::tests`, `assets::asset_manager::tests`,
`rules::sound_ini::tests`, `rules::process_owner::sinking_sound_tests`,
`audio::sfx::tests` and `app::match_audio::tests`.

For the existing checked native sound witness, extract SOUNDMD.INI, ARTMD.INI,
SMOKEY2.SHP, TWLT026.SHP and TWLT036.SHP through `asset extract` into one input
directory, then set both `VERA20K_BRIDGE_CHILD_SOUND_ASSETS` and
`VERA20K_BRIDGE_ANIM_ASSETS` to that directory and run:

```sh
python -m tools.rules_oracle.bridge_child_sound --check
```

That check executes original `7510D0/4063B0/750440` for physical Defaults and
Explosion06/ExplosionShard, plus Anim Report binding/release/stop. It supplies
cached INI nodes, allocator/TLS, ready flag=1 and fixture-local sample indices.
It does **not** execute physical INI IO, archive selection, complete registry
startup, device-disabled startup or mixing. No goldens are regenerated.

Production `asset sound` uses the same selected-pair loader and reports consumed
IDX/BAG identities. App startup logs its consumed SOUNDMD/EVAMD and sample-index
identities once. Headless digests and [map captures](map_observation.md) establish
scenario/render regression evidence; images cannot establish sound parity.

### Validated candidate

The validation receipt (`tools/audio_catalog_owner.validation.json`, in git at 1b1a8b43e) records the exact
Rust file hashes, preserved release manifest, native packet commands/hashes and
checked witness metadata. Strict retail library tests passed (9,592; 213
explicitly ignored), the additional retail loading-preparation test passed,
and clippy passed. No golden bytes changed.

The physical default index lists 2,285 entries, exactly the previous explicit
AUDIOMD listing. The old default union added APROTR1..5, GEXP05A and GEXP10A;
these seven are now absent by default and remain available through explicit
`--bag audio`. ABIRJ01A and GEXP06A exported WAV bytes match the prior build.
Hills/Battle headless output for 30 ticks is byte-identical and the production
capture comparison is `MATCH`. The captured app log records exactly one catalog
selection with the same consumed IDX/BAG hashes as the diagnostic tool. Captures
use their ordinary enabled-audio startup; disabled-index lifetime is tested
without a host device in the app owner regressions.
