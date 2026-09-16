# Options Profile Transaction Design

Date: 2026-08-31

Phase: 14, rows 299-300 (`GSI-03.02`, `GSI-17.06`)

Status: APPROVED by final corrective `/design-review`; implementation-ready

September 12 correction: this historical design incorrectly equated the early
configured game pair with the frontend window pair. Fresh original instructions
establish separate sizes and a post-load mode change. The sizing claims below
are superseded by [the shell resolution lifecycle evidence](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/2026-09-12-shell-resolution-lifecycle.md);
the profile read/write and audio contracts remain separate concerns.

## Goal

Replace VERA20k's fragmented startup/options/audio persistence with one process-lifetime, Rust-native profile transaction that matches the active-YR `OptionsClass` defaults, typed load, consumer application, accepted-dialog ordering, and coherent write boundary.

This mechanism closes only the Options/Video/Audio profile transaction established by:

- [docs/research/OPTIONS_PROFILE_TRANSACTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OPTIONS_PROFILE_TRANSACTION_GHIDRA_REPORT.md)
- `docs/contracts/2026-08-31-options-profile-transaction-implementation-contract.md`
- `docs/gap-scans/2026-08-31-disparity-scan-phase-14.md`

It does not implement launcher/dialog visuals, keyboard editing, display-mode enumeration, Network semantics, campaign surfaces, or the remaining Phase 14 rows.

## Architecture Context

The active binary has one process-owned `OptionsClass`. Rust currently has four partial authorities:

- `RetailStartupOptions` parses screen switches but `App::new` discards the pair.
- `InGameOptionsState` owns six UI fields under match presentation; startup hydrates only ScrollRate and DetailLevel.
- `MusicPlayer` independently reads and writes ScoreVolume.
- `SfxPlayer` has one master for both ordinary SFX and unit/EVA voice.

The app already has the right ownership domains. `PersistenceState` is process-lifetime and explicitly owns options persistence; `PlatformState` owns the process-local window projection; `MatchPresentationState` owns transient options controls and live visual gates; `AppAudioRuntime` owns output players. The design uses these owners rather than adding a global service or putting settings into deterministic simulation state.

## Impact Analysis

| Surface | Current role | Designed change |
|---|---|---|
| `src/app/persistence/options_profile.rs` | absent | new pure profile/default/load/format/commit module |
| `src/app/persistence/mod.rs` | process save state | owns the one `RetailOptionsProfile` instance |
| `src/app/frontend/startup_options.rs` | argv parsing plus unused RA2MD video helper | remains argv-only; profile module becomes the only RA2MD settings loader/resolver |
| `src/app/mod.rs`, `handler.rs`, `initialize.rs` | retain only audio disposition; fixed 800x600 before config load | retain full startup options, prepare profile before window creation, retain effective shell size |
| `src/app/state/platform.rs` | window/config/pacing | stores effective shell client size as a projection, including capture override |
| `src/app/persistence/options.rs` | partial Options read/write/apply | projects profile into UI, accepts UI changes into profile, applies consumers, commits full profile |
| `src/ui/shell/in_game_options_state.rs` | six controls plus transient state | stays a UI projection; does not become persistence authority |
| `src/ui/tooltips.rs`, target-line state | live gates exist | seeded from profile at boot and updated on accepted apply |
| `src/app/presentation/render/build_instances.rs` | ore/gem sparkle gate reads `GameConfig.graphics.extra_animations` | reads the profile-projected DetailLevel instead; RA2MD is the interactive authority |
| `src/audio/sfx.rs` | one combined SFX/voice master | distinct Sound and Voice masters with channel-tagged live and queued outputs |
| `src/audio/music.rs` | Score consumer plus duplicate profile I/O | remains Score consumer; standalone RA2MD read/write helpers are removed |
| `src/app/in_game.rs` | quit writes only live Score | confirmed quit commits the retained full profile once |

No `sim/`, snapshot, replay, hash, save-game, rules, or network representation changes.

## Player-Experience Detail Ledger

| Player-visible detail | Evidence-backed target | Design response |
|---|---|---|
| Saved resolution is used on launch and after returning from a match | binary WinMain early screen-pair path | prepare profile before window creation; retain effective shell target in `PlatformState` |
| A partial screen pair falls back for the startup window, then is reread into retained state | WinMain replaces both live fields when either sentinel remains; later `Init_Game` full-read reapplies any present Video key | carry the early effective window pair separately from the later retained profile (for example width-only `640` uses 800x600 but retains/writes 640x600) |
| Capture output stays deterministic | VERA capture is an authorized sealed override | capture dimensions become the effective shell target and capture does not ingest the user's RA2MD profile |
| Scroll speed/detail work before Options is opened | full native read precedes gameplay consumers | initialize the UI projection from the loaded profile |
| Low DetailLevel suppresses ore/gem PixelFX | active `DAT_00A8EB78 != 0` gate is the Options DetailLevel field | replace the parallel config gate with `in_game_options.detail_level != 0` |
| Disabled action lines/tooltips are disabled on the first relevant frame | native read synchronizes gates | seed `TargetLineState` and `TooltipService` during initialization |
| Sound and Voice sliders do not mute each other | native owns distinct Sound and Voice gains | tag all live outputs as Sound or Voice and recompute through the corresponding master |
| Queued EVA honors the current Voice master when it starts | Voice is a mixer-level owner, not a captured per-item setting | queue base entry gain, apply Voice master at playback/live recomposition |
| Existing playing output reacts to later volume changes | later launcher controls must reuse the same owners | setters recompose live outputs from retained base gain; no multiplying an already-mastered gain |
| Accepted Back applies then saves; pump/game termination does neither | `0x004E1D00` result gate | central result gate updates profile, applies consumers, then commits; termination result 2 skips all three |
| Quit saves all modeled settings together | confirmed exit calls full native writer | replace Score-only quit persistence with one full-profile commit |
| Comments, CRLF, high bytes, Network, Skirmish, and unknown keys survive | native profile is shared with unrelated state | one raw read, three in-memory section transforms, one filesystem write |
| Booleans/floats retain retail lexical shape | writer uses lowercase `yes/no` and `%f` | common formatter emits `yes/no` and six fractional digits |
| Bad negative audio does not reach the Rust backend unsafely | native object retains negatives; backend exactness is unresolved | retain raw profile value for round-trip; clamp only at output-player setters |

## Chosen Approach

### 1. One process profile value

Add `RetailOptionsProfile` under app persistence. It is a normal Rust struct, not a C++ singleton analogue. It models every verified Options/Video/Audio value needed for defaulting, typed loading, retention, or writing:

- Options: GameSpeed, Difficulty, CampDifficulty, ScrollMethod, ScrollRate, AutoScroll, DetailLevel, SidebarCameoText, UnitActionLines, ShowHidden, ToolTips.
- Video: ScreenWidth, ScreenHeight, StretchMovies, and the three read-only Allow flags. The Allow flags are loaded/retained but never serialized or exposed speculatively.
- Audio: SoundVolume, VoiceVolume, ScoreVolume, IsScoreRepeat, IsScoreShuffle, SoundLatency, InGameMusic.

Integer fields remain signed where the native profile is signed. SoundLatency is stored as `u16` after native low-16-bit narrowing. Volumes retain native single-precision values, including negative values; output consumers perform their existing safe clamp.

`Default` encodes the verified `OptionsClass__SetDefaults` values. A constructor seeds the two screen fields from `RetailStartupOptions` before the INI load so command-line values survive missing keys while present RA2MD keys override them, matching native current-as-default reads.

### 2. One parsed snapshot, two native read phases

`App` retains the full `RetailStartupOptions` until `resumed`. `App::initialize` changes order:

1. Load `GameConfig` to resolve the RA2 directory.
2. Construct exact profile defaults and seed startup screen fields.
3. For an interactive launch, read RA2MD.INI once and parse one immutable `IniFile`.
4. Apply only ScreenWidth/ScreenHeight from that snapshot to the argv-seeded profile, matching the WinMain-stage read.
5. If either early screen field is `-1`, replace both live profile fields with 800x600 and capture that result as the effective startup/shell window pair. For explicit non-sentinel invalid dimensions, retain the raw value but clamp only the window projection to at least one pixel; this remains the contract's display-failure exactification residual.
6. Apply the complete Options/Video/Audio typed read from the same parsed snapshot to the post-fallback profile. This intentionally rereads ScreenWidth/ScreenHeight: a present key overrides the fallback while a missing key keeps its post-fallback current value.
7. Apply only verified full-read transforms: Difficulty `0..4`, CampDifficulty/DetailLevel `0..2`, Sound/Voice/Score upper-bound `1.0`, SoundLatency low-16 narrowing.
8. Select the effective shell client size from the early pair. Capture dimensions win over the profile and remain the target on every capture shell transition.
9. Create the window, then move the later full-read profile into `PersistenceState` and the early effective client size into `PlatformState`.

Capture initialization uses exact Options/Audio defaults plus the standard post-fallback `800x600` retained screen rather than the user's RA2MD values. This preserves the sealed capture oracle beyond dimensions: user ToolTips, DetailLevel, or action-line settings cannot contaminate a capture. Interactive startup is the only profile-load owner.

Missing GameConfig/RA2 path prevents later file commits and resolves the argv-seeded pair directly. Missing RA2MD or a read/parse error runs the fallback over defaults/current argv values but has no later values to reapply; an error logs once and does not abort launch.

### 3. Profile projections, not parallel authorities

Initialization derives these projections before building `AppState`:

- `InGameOptionsState` receives GameSpeed, ScrollRate, DetailLevel, UnitActionLines, ShowHidden, and ToolTips. The retained profile keeps exact signed GameSpeed/ScrollRate values. Their UI projection clamps to the verified `0..6` control range; DetailLevel is already load-clamped to `0..2`. Match startup may later synchronize GameSpeed from deterministic simulation through the existing seam.
- `TargetLineState` is seeded from UnitActionLines.
- `TooltipService` is seeded from ToolTips.
- The ore/gem PixelFX builder reads `in_game_options.detail_level != 0` instead of `GameConfig.graphics.extra_animations`, matching the verified native nonzero gate. This also removes the interactive second authority. The existing capture contract still requires its explicit config flag and receives the capture-default DetailLevel of 2, so sealed output does not change.
- `MusicPlayer` receives the safely bounded Score gain immediately after construction.
- `SfxPlayer` receives safely bounded Sound and Voice gains immediately after construction.

No consumer is invented for Difficulty, CampDifficulty, ScrollMethod, AutoScroll, SidebarCameoText, StretchMovies, Allow flags, repeat, shuffle, latency, or InGameMusic. Their values remain in the process profile and round-trip unchanged until a separately verified owner exists. StretchMovies retention intentionally does not claim the native hardware-capability AND gate; that bounded difference is recorded below.

`PlatformState.shell_client_size` is explicitly a projection, not another settings authority. `enter_shell_window_mode` uses it instead of constants. The profile remains owned only by `PersistenceState`.

### 4. Independent Sound and Voice gain composition

Refactor `SfxPlayer` around two masters and master-independent output gain:

- Add an internal `SfxChannel::{Sound, Voice}`.
- `SfxOutputGain` retains a base gain that contains entry/spatial factors but not a user master, plus its channel.
- Effective gain is `base * channel_master * lifecycle_scale * focus_scale`.
- Ordinary and animation SFX create Sound outputs. Unit responses and all EVA modes create Voice outputs.
- `QueuedVoice` stores base entry gain, not a gain already multiplied by VoiceVolume.
- `set_sound_volume` and `set_voice_volume` clamp at the output boundary and recompose affected live outputs. `set_volume` remains a developer compatibility operation that sets both masters in one recomposition; `volume()` continues to expose Sound and a new Voice getter exposes Voice.

This preserves the existing 16-player sound pool, animation ownership, dedicated voice slot, EVA queue policy, lifecycle scaling, focus gate, and quit voice polling. It also prevents compounding and makes later verified launcher sliders reuse the same owners.

### 5. Accepted-dialog transaction

`options.rs` owns the app-level transaction because it already coordinates UI, sim command admission, live presentation, and persistence:

1. Check `ModalResult::options_persists` before mutation.
2. Copy the six accepted UI values into the retained profile; untouched fields remain loaded values.
3. Apply existing GameSpeed command behavior and live target-line/detail behavior, and add the verified Tooltip enable call.
4. Commit the full profile.
5. Perform the existing unpause, pacer reset, and cursor cleanup.

The result parameter becomes explicit at the transaction boundary even though current production Back callers pass result 1. A private production-used operations seam makes exact profile -> consumers -> write ordering and the result-2 no-op testable without constructing a GPU-backed `AppState`.

### 6. One preservation-safe commit

The profile formatter returns complete ordered key/value groups for Options, Video, and Audio. The commit path:

1. Reads RA2MD.INI once. NotFound is an empty snapshot; other read failures abort without writing.
2. Applies `set_ini_values` for Options, then Video, then Audio to the one evolving byte snapshot.
3. Writes the final bytes once.

The three Allow keys are never included. Network and every unrelated byte are untouched. A small injected read/write closure seam is used only to prove one-read/one-write behavior in tests; production calls `std::fs::read` and `std::fs::write` through the same function.

Standalone ScrollRate/DetailLevel and ScoreVolume RA2MD helpers are removed or reduced to profile-module delegates so no second filesystem owner remains. The argv module no longer parses RA2MD Video itself.

Confirmed main-menu quit calls this full commit before the existing quit cascade. OS `CloseRequested` stays unchanged because its native persistence owner remains unproven.

## Interfaces and Contracts

The exact names may adjust during implementation, but the dependency direction is fixed:

```text
RetailStartupOptions (argv only)
        |
        v
RetailOptionsProfile::from_startup + load_startup_snapshot
        |
        +--> early Video pass + fallback --> PlatformState.shell_client_size
        |
        +--> later full typed pass --------> UI/live projections
        |                                   +--> MatchPresentationState + AppAudioRuntime
        +----------------------------------> PersistenceState.options_profile
                                  |
              accepted/quit -----+--> commit_ra2md(one read, one write)
```

Required pure seams:

- exact default construction;
- apply the early screen-only pass from one parsed `IniFile`;
- sentinel-pair resolution and safe early window projection;
- apply the later complete typed pass from the same `IniFile` to the post-fallback profile;
- profile-to-in-game-UI projection;
- production-used accepted transaction operations gated by modal result;
- ordered owned-key formatting;
- raw-byte transformation independent of filesystem I/O;
- production-used normal/direct-voice/queued-voice output preparation independent of an audio device.

All filesystem failure handling stays at app/persistence boundaries. No profile method may reach into simulation or render state.

## Error Handling

- Config/root discovery failure: preserve the argv-seeded screen pair, applying the standard pair fallback only when it is incomplete; use exact Options/Audio defaults and leave commit unavailable.
- RA2MD missing: preserve the argv-seeded screen pair with the same incomplete-pair fallback and use exact Options/Audio defaults; a later accepted/quit commit may create the file.
- RA2MD unreadable or unparsable: warn, preserve the argv-seeded screen/fallback result, and retain exact Options/Audio defaults. Commit refuses an unreadable existing file rather than replacing it with a partial new file.
- Present malformed scalar: use existing native typed-reader behavior; do not fall back through ad hoc Rust parsing.
- Negative audio: retain and serialize; player setter clamps to the safe audible range.
- Invalid explicit dimensions: retain for serialization, clamp only the window projection to at least one pixel.
- Commit failure: warn and continue dialog close/quit; settings I/O cannot block the player.
- Audio device unavailable or `-NOAUDIO`: profile still loads, retains, and commits without a player.

## Testing Strategy

Focused `--lib` suites will cover:

1. Exact defaults for every modeled field.
2. One physical snapshot with an early screen-only pass and later full typed pass, including missing keys, malformed decimal, first-character bool, percent float, clamps, narrowing, and unsupported-field retention.
3. Startup precedence and shell reuse through production-used seams: complete 640x480 selects/retains/re-enters 640x480; width-only `640` selects/re-enters 800x600 while retaining 640x600; height-only `480` selects/re-enters 800x600 while retaining 800x480; argv seeds missing keys; explicit invalid values remain safe.
4. Boot projection of all six UI fields plus target-line/tooltip gates.
5. DetailLevel `0` suppresses PixelFX and `1/2` enable it through the production render input; config.toml no longer acts as the interactive toggle.
6. Production-used normal-SFX, direct unit/EVA voice, and queued-EVA output preparation under Sound/Voice `(0,1)` and `(1,0)`, including a Voice master change before dequeue, live-output recomposition, and lifecycle/focus scaling.
7. Golden profile formatting (`yes/no`, six decimals, key order, no Allow keys).
8. CRLF/high-byte/comment/Network/Skirmish preservation with an instrumented one-read/one-write seam.
9. Accepted result ordering and result-2 pump-termination no-op through the same operations dispatcher used by `AppState` production close.
10. Both confirmed main-menu quit owners through their production-used dismiss -> persist -> cascade wrappers, with one full-profile persist per owner and no Score-only writer.
11. Capture isolation through the production pre-window selection seam: with explicit capture dimensions and a conflicting full user-profile loader, the loader is never invoked; Options/Audio and their UI/audio projections remain exact defaults, the retained screen is the post-fallback `800x600` pair, and only the explicit hidden capture size becomes the shell target.
12. Existing startup, in-game-options, tooltip, target-line, SFX queue, music, capture, and INI-writer focused regressions.

Every Cargo invocation uses `cargo test -p vera20k --lib <focused-filter>` after checking that no other session owns Cargo. The repository-wide `cargo test -p vera20k --lib` remains reserved for the single final Phase 14 certification run.

## Architectural Decisions

1. `PersistenceState`, not UI or audio, owns the process profile.
2. The window stores only an effective shell-size projection.
3. Interactive profile preparation happens inside `initialize`, after root discovery and before window creation; `App::new` remains filesystem-free.
4. Capture uses exact Options/Audio defaults, the standard post-fallback screen state, and its explicit hidden size, preventing user-profile contamination.
5. Unsupported fields are retained without speculative consumers.
6. Sound/Voice independence is implemented at gain composition, not by stacking a new master over already-mastered values.
7. One complete profile write replaces every partial settings writer.
8. Native negative-audio and invalid-display backend accidents remain explicit safety-boundary residuals.
9. OS close persistence remains unchanged until separately proven.

## Bounded Residual Ledger

| Residual | Trigger and frequency | Player effect | Downstream risk / disposition |
|---|---|---|---|
| Negative audio backend behavior | Hand-edited negative Sound/Voice/Score value; expert-only and absent from the active profile | Native forwards a negative scaled integer; Rust safely mutes at the player boundary while retaining and reserializing the negative profile value | Audio-output exactification only; no sim/state effect. Requires a dedicated legacy-backend investigation before claiming exact output parity. |
| Explicit invalid non-sentinel screen size | Hand-edited zero/negative width or height; expert-only and absent from the active profile | Native later display failure/recovery is unproven; Rust creates a minimum 1-pixel projection and retains the raw stored value | Window-only. It cannot enter render/sim data; exactify only after tracing native mode-failure recovery. |
| Out-of-control-range GameSpeed/ScrollRate | Hand-edited signed value outside `0..6`; expert-only and absent from the active profile | Profile retains the raw value, while Rust UI/camera projection clamps to the control range. An accepted dialog replaces it with the selected bounded value. | GameSpeed cannot bypass sim command/sync authority; ScrollRate is presentation-only. A narrow native consumer trace is needed for malformed-profile exactification. |
| Case variants or duplicate sections/keys | Modded profile with non-retail spelling/duplicates; expert-only; active profile has exact spelling and no duplicates | Rust may choose a different occurrence than native | Confined to profile selection. Do not assert behavior or add tests until the INI normalization/CRC ordering investigation runs. |
| StretchMovies capability AND | `StretchMovies=yes` on a native host whose capability byte is false; expert/legacy-hardware edge and active profile is `no` | Rust retains `yes` because no movie-stretch consumer or equivalent capability probe exists; native would retain/write the effective false value | No current presentation effect. Future Phase 14 media work must introduce a verified capability projection before consuming this field. |
| Low-video-memory 640x480 fallback | Native adapter reports at most 2 MiB on the ordinary platform branch; effectively unreachable on supported wgpu hosts | Rust chooses 800x600 instead of 640x480 when the pair is unset | Startup-window exactification only; no gameplay dependency. |
| OS window-close persistence | Player closes the window through the title-bar X; ordinary but settings already commit on accepted in-game Back, and no launcher mutation exists yet | Rust exits without the confirmed-quit full-profile write; native ownership is untraced | Unknown-risk reverse-audit target. Do not guess a write owner; investigate before Phase 14 completion if the path can lose an in-scope mutation. |

## Alternatives Considered

### Minimal extension of the existing partial helpers

Add the missing fields to `InGameOptionsState`, add two audio readers, and keep separate writers. This is initially smaller but leaves UI/audio/startup as competing authorities, cannot guarantee an atomic complete snapshot, and makes launcher controls reconcile several layers later. Rejected.

### Put the profile in `PlatformState`

This makes pre-window use visually convenient, but settings persistence and accepted-dialog transactions are not platform lifecycle concerns. It would also make UI/audio mutation reach through the window owner. Rejected; only the resolved shell-size projection belongs there.

### Add a global preferences service

A global service would resemble the native singleton, but it introduces hidden filesystem access and shared mutation into an app that already has explicit process owners. It also complicates capture isolation and tests. Rejected.

## Adversarial Self-Review

### Why approve this design?

It maps the binary's one-object transaction onto existing Rust ownership boundaries without importing native memory layout. All load-bearing behavior has one named owner, all previously partial writers collapse into one commit, and every required consumer is initialized before it can be observed.

### What could make an ordinary stock skirmish feel wrong?

- User profile values could contaminate capture or test output. Resolved by suppressing profile ingestion for capture.
- A saved 640x480 shell could revert to 800x600 after a match. Resolved by retaining the effective shell target in `PlatformState` and using it on every shell re-entry.
- Unit voices could still follow SoundVolume, or queued EVA could capture stale VoiceVolume. Resolved by channel-tagged base gains and playback/live recomposition.
- Tooltips/action lines could begin enabled for one frame. Resolved by constructing their gates from profile values before `AppState` becomes active.
- Profile GameSpeed could bypass deterministic command admission. Resolved by keeping it presentation/profile state and preserving the existing match sync/command seam.
- A malformed or unsupported setting could crash window/audio creation. Resolved by explicit projection-boundary clamps while retaining round-trip values.

### What could create expensive rework later?

- Leaving RA2MD parsing in startup, UI, and music modules would require another ownership migration. Those helpers are removed/delegated now.
- Multiplying masters into gains at enqueue/start time would make live launcher sliders require reconstructing lost base values. Base gain and channel are retained now.
- Dropping unsupported fields would cause later controls to overwrite user values. Every verified field is modeled and round-tripped now.
- Treating capture as ordinary interactive startup would force future capture fixtures to sanitize user files. Capture isolation is explicit now.
- Putting settings into match/sim state would create save/hash/replay migration debt. The profile stays process/presentation-only.

### Decision

**APPROVE.** The design closes the common player-visible profile transaction, preserves the current architecture, and records all unproven edges instead of expanding scope. The corrected design-review found no remaining implementation blocker.
