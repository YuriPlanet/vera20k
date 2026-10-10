# Chosen-map production observation

`python -m tools.map_observation` launches the normal production loader through
`--tactical-capture map-observe-v1`, advances the requested exact simulation steps,
and retains a hidden-window GPU readback. This is a production observation, not a
native comparator or a gameplay/pixel parity certification.

Foot observations include `pending_entry_500`, the pending-entry owner's stable
target handle or `null`. This read-only field is independent of NavCom, mission
and admitted radio contacts. Older sealed v6 receipts remain accepted without
the field; its absence supplies no evidence about pending entry.

Actor `retask` reads the existing `suspended_target` and `suspended_nav`
references alongside current `target`, `archive`, `nav` and mission fields.
References retain their distinct tagged target/navigation representations;
`null` means the corresponding owner is empty. The observation sends no retask
or destination callback. Historical receipts may omit `retask`; omission supplies
no evidence about either suspended reference. These fixed fields remain part of
the existing one-sample actor row.

Requested terrain cells also report `local_visibility`: the current viewer's
owner name and retained `revealed`, `visible` and `gap_covered` queries. It is
`null` when no local viewer resolves. These read-only observations let a GPU
capture demonstrate building pixels crossing a frontier while its anchor is
still unexplored, without changing sight or substituting a diagnostic reveal.
Older receipts may omit the field; their absence supplies no visibility evidence.

Actor `cloak`, when present, reads the existing raw signed state/progress,
live signed Rules `CloakingStages`, actual VXL/SHP route, and original type's
`NoShadow` flag. `null` means no cloak runtime. Reading this projection sends
no transition, timer or movement callback. Historical sealed receipts may omit
it; omission supplies no cloak-state evidence. This records VERA production
inputs and does not independently establish native timing or whole-object parity.

Foot's optional `track` observation reads the installed Drive/Ship owner's
destination and head XYZ, selector, signed cursor and valid byte. It sends no
movement callback and creates no runtime. A paid head may survive Stop after
the destination becomes `null`; its cursor and physical pose show the remaining
segment. Historical sealed receipts omit `track`, which supplies no evidence
about that state. An explicit `null` means no installed track state was observed.

The explicit profile schema belongs to
`src/app/diagnostics/tactical_capture/map_observation.rs`. Start from
[`map_observation.example.json`](map_observation.example.json), whose launch is the
existing Rust `SkirmishLaunchSession` DTO. Change `launch.selected_map_file` to the
chosen retail map. Keep explicit countries, colors and distinct start slots, and
keep `pre_fill_house_roster` consistent with the opponents. Rust validates launch
admission; Python does not maintain a second launch parser or synthesize defaults.

The [factory rally profile](map_observation.rally-drawing.example.json) adds the
ordinary `Select` command to a produced barracks. Its optional v2
`cursor_position: [720, 556]` is applied once after loading, before exact steps,
and retained in the render receipt. An explicit cursor must be inside the
outermost screen pixels; normal static-arrow and idle-camera checks still apply
to every draw. Place it over a neutral sidebar area when selecting actors would
otherwise produce an animated order cursor. Profiles without this field retain
the ordinary post-load center position.

Optional v2 `observe_action_line_inputs: true` adds each actor's existing
read-only facing, active locomotor/motion, applied speed fraction, crate/house
speed factors, current speed/weapon, veterancy, turret offset and rocking angles.
Building rows contain `null`. Fixed-point and floating-point bits are named
explicitly; they are not silently converted to native values. `false` or an
absent field adds no actor fields, preserving historical observations. Version1
rejects the option, even when false; present null/nonboolean values are invalid.
The [procedural drawing comparisons](procedural_drawing_oracle/README.md)
bind these observations to separately executed native readers/getters and retain
an observer-off replay proving unchanged frame bytes and gameplay boundaries.

Optional v2 `observe_projectiles: true` and `observe_anim_types: ["BBBLELRG"]`
add an `effects` snapshot at L0 and every committed frame. Each option is
independent. Projectile rows read every live projectile in stable-ID order,
including physical XYZ, source, visual counters and Logic membership; its weapon
resolves the type name through the loaded rules. Animation rows read the global
animation owner filtered by the requested types, including attached animations,
with attachment, stored and absolute XYZ, flags, visibility, bounds and the
existing complete runtime/timer. An unavailable absolute coordinate is `null`.
Rows disappear when their owners retire them; the observer keeps no invented
projectile-to-animation parent link or additional lifecycle state. The snapshot
also reads the Scenario RNG cursor without drawing. It counts one cursor plus
every retained object toward the shared 100000-sample budget and preserves the
128 MiB receipt limit. Animation filters contain 1..256 nonempty names, distinct
under ASCII case folding. Absent options (or only `observe_projectiles: false`)
add no frame fields; v1 rejects both options.

Optional v2 `observe_lasers: true` adds a `lasers` snapshot at L0 and each
committed frame. L0 reads initial state; later snapshots follow the app's
ordered simulation handoff and FPS sample, before that frame's draw.
`live` reads retained lasers in birth
order: copied source/target leptons, birth frame, Z adjustment, width, support
flag, House-color flag, resolved RGB, duration, age and timer. `detail` reads the
process FPS counter, selected minimum/buffer, latch and sampling timer; it never
calls the mutative minimum-frame-rate query. Each observed actor also receives
`prism`: `null` for non-buildings, otherwise the support count, pending mode and
payload/countdown, and rearm timer. These are owner snapshots, not inferred
firing events or proof that a beam reached the GPU. One detail row and each live
beam count toward the shared sample budget. False/absent adds no fields; v1
rejects the option, and present null/nonboolean values are invalid.

Optional v2 `observe_audio` records device-mixer queue pulls from requested ordinary
SFX Players. For example:

```json
"observe_audio": {
  "sound_ids": ["SquidMove"],
  "max_events": 4,
  "max_samples_per_event": 262144,
  "completion_tail_ms": 4000
}
```

The production SfxPlayer enables the recorder at accepted L0. Its existing
decoder, arbiter, Player, gain/pause/stop processing and device mixer still own
playback. A forwarding Source observes the Player output that the production
device mixer pulls, **before mixer resampling/summing**, including any synchronous
initialization prefetch. It does not establish that each sample reached the DAC
or physical speakers, and is neither OS loopback nor native audio equivalence.
Both paths preserve the ordinary new-Player, mixer-add, gain-setup, append order;
unobserved output retains `Player::connect_new`. The diagnostic
callback uses preallocated atomic slots, with no per-sample diagnostic locks or
allocations; Source format queries occur at span boundaries.

Each observed submission retains the reusable arbiter slot, owner, resolved
sample identities, decoded source sample count, action/context rows, format
spans, exact little-endian f32 PCM bytes, finite/nonzero counts and SHA256.
`submission` is the capture ordinal; `event` is a reusable production pool slot.
A stopped old source can finish draining while a new submission reuses its slot.
The PCM includes any Player queue filler silence. Completion requires both the
existing SFX owner's terminal action and the device mixer pulling the queue to its end;
recording is not cut off merely because a stop/release was requested.

An action's `context` is the **last completed exact-step capture boundary**.
It remains fixed during a step, including sound submission, and refreshes after
the step receipt commits. For example, a sound emitted during native frame 0
can be submitted with context step 0/frame 0, then started by the following
audio service with context step 1/frame 1. It is not an exact action or PCM
sample timestamp. `service_ms` is SfxPlayer's last serviced wall-clock value;
actions between service passes retain that value rather than sampling a new
clock. Frame observations are post-commit state: the step 1/frame 1 row contains
the result of executing native frame 0. A literal load also refreshes the
context's loaded frame/tick while `completed_steps` remains monotonic.

Limits are 1..16 distinct ASCII sound IDs (128 bytes each), 1..16 submissions,
1..262144 samples per submission, 64 action/format rows and 128 resolved sample
names per submission. After the requested exact steps, the existing frame/audio
service continues without simulation ticks for at most the requested 1..10000 ms
tail. It stops when observed outputs end; exhaustion/timeouts fail the capture
instead of claiming complete output. Repeated tail draws are counted separately
from the exact-step draw transcript. PCM stays inside the existing sealed
`capture.json`; the 128 MiB receipt limit is unchanged. Frame `audio_state` and a
load segment's `restored_audio_state` read the existing Main/Scenario cursors and
filtered actors' body counter, MoveSound latch and countdown without advancing
any owner. Each row charges its actors and both cursors to the shared
100000-sample budget, including the immediate restored row. Sound RNG and device
timing are presentation inputs; repeated PCM captures need not be byte-identical.

Export retained PCM after the wrapper validates every byte and summary:

```sh
python -m tools.map_observation export-audio --run /absolute/retained-run \
  --output /absolute/new-audio-directory
```

Each format span becomes an IEEE-float WAV; no retail decode or mix runs again.
The export records a final incomplete channel frame, if present, as
`unframed_tail_samples`; those samples remain intact in the original receipt.
Neither export nor validation writes into a retained run.

Literal `key` gestures may optionally include a unique `modifiers` list containing
`Ctrl`, `Shift`, `Alt`, and/or `Super`. These use the shared modifier-key owner and
ordinary key edges, and release every modifier before rendering. Bare historical
gestures keep their existing receipts. The release game's Ctrl+Shift+M/N shortcuts
exercise the production quicksave/quickload owner. A quickload gesture requires
`allow_load_segments: true` and must actually restore an earlier clock. Up to 16
`load_segments` retain the triggering gesture ordinal and before/after tick,
binary frame and simulation milliseconds. Capture-step numbering remains
monotonic; draw/receipt validation follows the restored clock, including its
rewind. This is VERA same-content snapshot loading, not original SAV compatibility.
Run these profiles with a fresh scratch working directory/config: the production
save repository uses that working directory's `saves/` folder. There is no
diagnostic save backend or bypass of preparation/commit.

A v2 `command_bar` gesture names an exact loaded bar command, for example
`{"kind":"command_bar","command":"Guard"}`. Rust resolves the name through
the existing command registry, current UIMD slot layout and retained gadget,
then sends ordinary left press/release edges at its current center. A missing,
closed, disabled or inconsistent control fails capture. This does not call a
semantic command dispatcher directly. Its receipt records `command`, actual
`slot`, retained `gadget_id`, `rect` and `resolved_position`; the shared mouse
receipt checks the expected gadget captured the press and release restored
neutral input. The command-bar receipt adds one sample to the existing budget.
Profiles containing it use `map-local-gesture-v4`; historical tactical/sidebar/
key-only profiles keep their existing policy and receipt shape. It accepts no
click modifiers. Python checks receipt shape/geometry without reproducing the
runtime command registry, layout or action admission.

The Area Guard [standing discovery](input_oracle/profiles/area-guard-standing-discovery.json)
and [combat discovery](input_oracle/profiles/area-guard-combat-discovery.json)
profiles load authored object placements on unchanged terrain with retail MTNK
and E1 types. Materialize `selected_map_file` as an absolute path before capture;
relative names resolve under the retail directory. Inspect L0 actor IDs before
reusing the [literal G](input_oracle/profiles/area-guard-key.json),
[command-bar Guard](input_oracle/profiles/area-guard-bar.json), or
[mixed tank/GI selection](input_oracle/profiles/area-guard-mixed.json) profiles.
They use ordinary Select followed by the actual input route, observe tagged
queued orders and actor retask state, and retain CommandBar SFX PCM. The mixed
key profile also exposes the existing Main cursor; unit order voices use the
separate voice queue and are outside this PCM observer. Native ordered voice
requests and list draws require the separate executed input comparison.
The [in-range retask profile](input_oracle/profiles/area-guard-retask.json) places
the enemy four cells away, requests G, later presses S, then clicks empty ground
at cell `(38,55)`. The literal pixel `(46,359)` comes from the current cell-center
projection, flat authored terrain and the discovered camera `(-556,1066)` at
zoom 1. Check the actual queued Move destination and camera in each run before
using the retask observation. Its 110-step window observes attack and explicit
retask; it does not certify target destruction, pursuit or return-to-post timing.

The [idle SQD profiles](spatial_oracle/fv_cell_attack/profiles/README.md) use an
authored water map with unchanged retail types. A separate ignored real-device
SfxPlayer test exercises release/detach/hard-stop consumers; ordinary Stop on a
stationary SQD does not manufacture an idle-lapse trigger.

The [projectile trailer discovery profile](projectile_oracle/profiles/projectile-trailer-discovery.json)
loads an authored water scene with unchanged retail SUB and LCRF rules. Use it
to confirm stable actor IDs before the ordinary `Attack` in the
[trajectory profile](projectile_oracle/profiles/projectile-trailer.json).
Copy these profiles to scratch and replace `launch.selected_map_file` with the
absolute path of the tracked
[`projectile-trailer.mpr`](projectile_oracle/profiles/projectile-trailer.mpr)
in the checkout before running the capture command. Relative map names resolve
under the retail directory, not the capture checkout.
The [flight profile](projectile_oracle/profiles/projectile-trailer-flight.json)
records the first 40 steps, with a GPU readback of surviving bubbles after
impact. The longer trajectory profile follows their final cleanup.
Its [observer-off control](projectile_oracle/profiles/projectile-trailer-flight-no-effects.json)
omits only the effect fields, allowing final state and GPU bytes to be compared.
The trajectory and flight profiles follow the production `SubTorpedo` path and
filter `BBBLELRG`; neither injects a projectile or animation. Their endpoints bound a first-shot experiment;
inspect actual emission, retirement and cleanup frames before drawing a timing
conclusion. Per-frame owner snapshots and a rendered endpoint establish production
integration within that window, not original-engine pixel or whole-weapon parity.

Optional v2 `observe_super_weapons: true` adds a `super_weapons` list to each
observed House row: every Super the House holds, ordered by the interned type id
an ordinary `LaunchSuperWeapon` command names, with its grant, readiness, hold,
charge start, duration and remaining frames, and the Force Shield's fade countdown
(`SuperClass+0x50`, -1 when idle) and its coordinate. The rows count toward the sample
budget. `false` or an absent field adds nothing; version 1 rejects the option,
and present null/nonboolean values are invalid.

`render.frame_wall_mean_ms`, when present, reads the existing frame timer's last
up-to-60 intervals. It includes simulation, diagnostic observation, rendering and
presentation pacing. It is cadence metadata, not GPU duration or ordinary play
FPS, and does not participate in deterministic comparisons.

Build through the shared Cargo owner, then run from the checkout with an existing
`config.toml` pointing at retail assets. Set `graphics.upscale = false` for the
native-resolution capture. All supplied paths must be absolute without symlink
ancestors; use canonical temporary paths on macOS. The output parent must exist,
and the output directory itself must not exist.

```sh
python -m tools.cargo_run -- build -p vera20k --release --bin vera20k
env -u RA2_DIR python -m tools.map_observation \
  --profile /absolute/checkout/tools/map_observation.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/new-map-observation
```

The default executable is the unchanged release `vera20k` recorded for this
checkout by `tools.cargo_run`; it never guesses a target directory. Recorded byte
identity does not establish source freshness: build the intended revision first.
`--executable /absolute/path/to/vera20k` selects an explicit binary instead.
For preserved builds, `--build-label LABEL` asks the shared Cargo owner for that
label's verified host release `vera20k`; the two selectors are mutually exclusive.
It checks the artifact path, target/profile classification, ambiguity and actual
executable SHA, with no fallback to the latest build. For example:

```sh
python -m tools.cargo_run --label before-map-change -- build -p vera20k --release --bin vera20k
python -m tools.cargo_run --resolve vera20k --profile release --from-label before-map-change
env -u RA2_DIR python -m tools.map_observation \
  --build-label before-map-change \
  --profile /absolute/checkout/tools/map_observation.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/before-map-change
```

The supplied contract must match the repository contract bytes. Every environment
variable in its denylist must be absent, even if set to an empty string. The
wrapper reports denied variables and does not silently change the environment.
After sourcing the native development environment, explicitly remove `RA2_DIR`
for this command as above; asset loading uses the working directory's config.

The [factory tank exit profile](map_observation.factory-tank-exit.example.json)
exercises the ordinary human GAWEAP → MTNK production path on retail AnyTown
(`XMP03T4.MAP`). It deploys the starting MCV, builds power, barracks and refinery,
places the 5×3 factory at `(25,91)`, then queues one tank without a rally order.
Its numeric type/entity handles belong to the recorded retail roster and seed;
inspect a zero-step observation before reusing it with different rules or assets.
Use `RUST_LOG=info` with the command above to retain placement foundations in
`logs/ra2.log`. The profile requires 5600 steps to observe the complete exit.

The [Jumpjet instance discovery profile](map_observation.jumpjet-instance.example.json)
extends the AnyTown CMIN profile with that same GAWEAP production prefix and
placement, then queues stock SHAD type210 at step4600. It preserves seed305419896,
input delay2 and the original6000-step budget; the900-second timeout comes from
the factory profile. Its type filter records SHAD, GAWEAP and their prerequisites
alongside CMIN. Factory placement/timing came from the production capture below;
SHAD admission, spawn time, stable ID and current-candidate placement still require
an actual run. This profile is a discovery pass, with no guessed Move/Stop handle.
After inspecting its actual SHAD row, a second profile can append ordinary Move
and Stop commands using that literal stable ID and suitable later frame boundaries,
while preserving the discovery pass's seed, launch and production prefix. The
protocol accepts ordinary command IDs; `observe_types` only filters observations.

Current actor rows include `jumpjet`, null unless an active installed Jumpjet
runtime exists. Its retained destination XYZ, moving byte, phase, landing latch,
linked parameters and flight/facing values come from read-only value getters.
`params` preserves binary32 bits and `flight` preserves binary64 bits rather than
converting them to JSON floats. Foot rows also include `air`: independent raw
`tracker_cell_560` and `slot_cell_564`, spatial bucket/enter order, and raw slot
holder IDs at the actor's current cell, tracker cache and slot cache. NativeNull
is explicitly `[0,0]`; neither cache is reconstructed from location or membership.
These projections send no gameplay callbacks or slot/tracker writes. They are
additive v6 extensions, so sealed older receipts remain readable unchanged.

The 2026-10-02 release observation loaded MIX entry `-854728974` with map payload
SHA-256 `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`.
Factory1456 placed at step4501, became operational4551, and delivered tank1528
at5249. Unload states1/2/3/4 occurred at5250/5265/5293/5325. The tank reached the
forced track head at5321, cleared the factory footprint at5333, and returned to
Guard at5340 in Cell `(30,93)` / XYZ `[7808,23936,416]`; the factory returned to
Guard at5370. The inspected 800×600 BGRA readback has SHA-256
`adceea6c241b3e4eb16eb7039d6c1159dfdb376f4d9191bfcc4b3634127c80cb`.
The integrated release binary SHA-256 is
`ec8dfce838c10c8eb86e5107d5643c2e907584c435303d0e5e6f0995cfccd823`
(31,504,832 bytes). Its retained `capture.json` and `run.json` SHA-256 values are
`f51df731f516a72f28ab2a1bb3ec2687a9f2e95a54edc5455e1cd0e1ea270394`
and `aab418567732c72350abebc572bfd898a0bab4b27c9725dbeb2a1b5986a05bf1`.
The factory and tank pose, lifecycle, mission and NavCom trajectories match
the pre-integration release
`079f26fb33e3f97ab5571c9350bcdc619384c03116a98ebfee1be8aab07a15d2`
at every recorded step; both releases produce the final readback above.
After integrating main `03750cc7` (shared refinery/depot docking), the final
release binary SHA-256 is
`30511790b949abba1b62dd596d9cf505d8e90b0e6be5d8888f34056ea114a6b1`
(31,541,264 bytes). Its v6 bundle is valid at step5600; `capture.json` and
`run.json` have SHA-256 values
`cf0c3c24a1fc485160d3906f0a4f4b67fbca55eb2ce15891f9356560f6cf3262`
and `8fbe3b95517d3a4beedab78f65add32d0867e270f61df33a82ce2a2b18dcd405`.
All 5,601 frame boundaries match the earlier integrated release for the
factory/tank's recorded pose, lifecycle, health, mission, NavCom, archive,
target, Foot, Building and original Unit fields (11,202 actor comparisons),
and the final BGRA bytes are identical. The new v6 radio/miner observations
and incoming IFV weapon/turret fields were absent from that earlier schema;
they are excluded from this comparison. The final deterministic state hash is
`4786212690033122157`. The external
`unit-unlimbo-main1010-runtime-summary.json` retains the declared field lists,
input/artifact identities, comparison counts and endpoints.
The final zero/default ExitCoord candidate was observed on 2026-10-03 with
release binary SHA-256
`e88f7537f4b0aef9dcf4ebddb3cf8c94c56f460127116938bcb6cd641f365b43`
(31,512,752 bytes). The stock R6 bundle passes the ordinary v6 validator and
matches R5's retained initial/final state, map identity and complete observation
transcript; the final BGRA bytes remain identical. R5's executable path was
superseded by the shared release build, so the strict cross-run wrapper cannot
revalidate that historical binary. This comparison uses the retained capture
bytes and does not weaken that wrapper.

R7 uses the same release and production loader with a loose copy of the original
map bytes plus only `[GAWEAP] ExitCoord=0,0,0`. The existing asset reader confirms
that the scenario layer overrides stock `[512,256,0]` with `[0,0,0]`; the loader
reports the derived map SHA-256
`0dfbeaf28550ed9213e54ad4cca70b698488065e26e021770b263091cb7f4199`.
R7 also passes the ordinary v6 validator. Every factory1456 and product1528
actor field, including radio, miner and IFV fields, matches R6 at all 5,601
boundaries (11,202 presence/actor comparisons; 3,700 present actors), and the
final BGRA bytes are identical. The different map/rule inputs produce different
whole-state hashes; those hashes are not an equivalence assertion. The external
`unit-unlimbo-zero-final-runtime-summary-20261003.json` records these identities,
declared fields and comparison limits (SHA-256
`9d9e235d7f56d9948ffa1292fb11dc9390827115eca9a7faa007981a0feb649f`).
After incoming Grand Cannon/recoil integration and snapshot284 composition,
final R8 stock and R9 zero-map bundles both pass the ordinary v6 validator.
Release binary SHA-256 is
`d894cf0b771716e60d394c692c419b62e125920c1d0a3c12ab1c13e67c2a2f0f`
(31,640,672 bytes). Their complete factory/tank actor dictionaries match at
all 5,601 boundaries (11,202 presence/actor checks, 3,700 present), and their
final BGRA bytes retain the same `adceea6c…` identity above. The factory/tank
fields also match retained R6 after removing only incoming optional
`building.voxel_gun` from that historical schema comparison. R8/R9 retain
the stock/zero map identities and complete exit/Guard endpoints above; their
whole-state hashes remain distinct with different rule inputs. Final summary
SHA-256 is
`e503a82b7e63b1cb4c654f644a6359e41d606ed0c4708870efecea5039b1b43d`.
These are Rust production observations. Bounded original Door, Unload and Unit
comparisons are documented by the existing
[Unit Unlimbo evidence owner](spatial_oracle/anytown_damage/unit_unlimbo.md);
this profile does not establish native whole-object clock or pixel equivalence.

The [CLEG Cell destination profile](map_observation.cleg-cell-destination.example.json)
uses the same retail start, builds America's AMRADR and a Battle Lab, then produces
CLEG 1635. Two ordinary Cell orders share step 9500, with the second replacing the
first before Process. The optional type filter keeps its 10000-step capture within
the fixed sample budget. The [destination evidence owner](spatial_oracle/infantry_teleport_destination.md#production-integration-and-validation)
records the observed arrival, NavCom cleanup, Guard return and GPU/input identities.

A new wrapper v6 bundle contains sealed `profile.json`, `config.toml` and
`contract.json` copies, plus `stdout.log`, `stderr.log`, `run.json` and the child's
atomically published `child-output/{capture.json,frame.bgra}`. Runtime still reads
the supplied original paths; retaining copies does not redirect the game loader.
Original files and retained copies must remain unchanged during capture.
`run.json` records exact input hashes, command, child PID/status, timeout, receipt
validation and capture artifact identities. A valid observation requires unchanged
profile/config/executable/contract files, matching profile and contract receipts,
a v6 child manifest with resident UnitAtlas statistics, a checked presentation-clock
transcript and neutral-input evidence, zero initial tick/frame/time,
the requested final tick/frame and endpoint step
receipts, a loaded loose/MIX map digest, hidden unfocused rendering without input
violations, and correctly sized/hashed BGRA bytes. Simulation time must advance
for nonzero steps; its scheduling formula remains owned by Rust. Zero steps must
retain the initial fingerprint. Map hashes attest the bytes reported consumed by
the loader; this wrapper does not independently extract MIX entries or reimplement
the loader. Compare deterministic fingerprints separately from presentation pixels.

The v4 `render.presentation_clock` records actual consumed presentation times:

```json
{
  "policy": "map-exact-step-presentation-v1",
  "origin_ms": 0,
  "interval_ms": 22,
  "draws": [
    {"completed_steps": 1, "radar_ms": 22, "tooltip_ms": 22, "message_ms": 22}
  ]
}
```

For a positive budget N, exactly N game draws occur at completed steps 1 through
N. A zero-step observation has one draw at step 0, with all three times zero.
Every row must contain the exact radar tick, tooltip poll and adjusted message
management times consumed by that draw; each must equal its completed step × 22.
The budget is bounded at 100000 steps. Unknown fields/policies, missing or extra
rows, reordered/repeated steps, wrong integer types and any time discrepancy fail
validation. The wrapper retains the validated transcript as
`capture.presentation_clock`; it does not reconstruct a missing transcript.

The required v4 `render.neutral_input` has exactly
`{"static_default_cursor":true,"camera_input_idle":true}`. Rust verifies these
prerequisites and render readiness on every draw before publishing the final
receipt; the wrapper checks their declared types/values and retains them as
`capture.neutral_input`. This bounds the capture to neutral input and a static
default cursor. It does not establish arbitrary animated-cursor reproducibility.

This supplied clock is a diagnostic reproducibility policy, not native wall-time
cadence. Ordinary play and other capture profiles retain their existing wall
clocks. The clock does not change simulation scheduling or provide evidence for
native pixels, audio playback, menus or outcome timing. Full-frame comparison
remains exact: no radar masks, channel tolerances or skipped pixels are applied.

## Sidebar input and retained-order observation

The [retail opening profile](map_observation.sidebar-opening.example.json)
uses stock AnyTown and ordinary MCV deployment/production before clicking the GI.
Numeric command handles belong to that seeded launch; rediscover them with a
zero-step observation after population changes. The
[base-input profile](map_observation.sidebar-order.example.json) uses an
[authored clear-ground base](map_observation/examples/sidebar_order.map) with
unchanged retail type rules. It visits all four tabs, scrolls the infantry,
defense and building strips, and clicks MTNK and the revealed CLEG. For this
loose map, copy the profile and set `launch.selected_map_file` to the absolute
path of the tracked map; relative map names resolve under the retail root.
Pass absolute profile/contract paths to the wrapper.

The [removal profile](map_observation.sidebar-removal.example.json) uses the
same unchanged base at 640 by 530 pixels. It scrolls the defense strip,
issues the existing sale command for its Weather Control Device, and later
clicks the wall cameo through the resulting view. The native removal row
adjustment has separate executed controls in the sidebar oracle. This profile
observes the removal's sidebar consumer, not native sale or grant timing.

A v2 profile may opt in to `observe_sidebar_steps: [0, 1, 100]`. Each listed
capture completed step records the local retained sidebar view: tabs, ordered
visible type/name/cost/queue rows, scroll position and hit rectangles. These
steps remain monotonic across quickload; a restored simulation tick does not
repeat an earlier sidebar sample or select a different requested row. At
rendered steps, the harness checks that the actual `GameRenderOutput.sidebar_view` matches the
retained projection. Step 0 in a positive-step run is explicitly retained-only.

Sidebar gestures resolve their coordinates from that current view and dispatch
ordinary left mouse press/release edges through the production gadget router:

```json
{
  "issue_after_step": 100,
  "gesture": {"kind": "sidebar", "target": {"kind": "tab", "tab": "vehicle"}}
}
```

Other targets are `{"kind":"cameo","type_id":"MTNK"}`,
`{"kind":"scroll_down"}`, `{"kind":"scroll_up"}`, `{"kind":"repair"}` and
`{"kind":"sell"}`. Repair/sell targets click their actual retained toggle
buttons, not a guessed keyboard shortcut. Their gesture receipts add `toggle`
before/after rectangles, disabled bits and active latches; historical strip and
frame receipt shapes remain unchanged. Cameos must already
be visible; scroll controls must be enabled. Each receipt records the target's
resolved coordinates and before/after views. Commands created by a cameo click
still use the ordinary synchronized command scheduler. This observes Rust
production integration; native ordering comparisons are owned by the
[sidebar oracle](sidebar_oracle/cameo_order.md), with their separate bounds.

## Literal keyboard gestures

A v2 profile can send a literal key press and release through the same in-game
keyboard edge owner used by `WindowEvent::KeyboardInput`:

```json
{
  "issue_after_step": 10,
  "gesture": {"kind": "key", "key": "N"}
}
```

Keys are one ASCII letter or digit, or `Escape`. They carry no modifiers and
are not command names. The loaded `KEYBOARDMD.INI` bindings and registered
startup defaults resolve them in production. Each keyboard receipt reports the
production `encoded_key`, resolved registered `binding_command` (or `null`),
and observed held-key press/release state. The capture never dispatches a
semantic hotkey command directly. `Escape` can cancel ordinary world modes;
an input that opens a menu leaves the capture's admitted in-game state and fails.
The gesture transcript also records the startup `keyboard_bindings` table in
registered command order: each INI command name and its loaded `first_key`, or
`null` when unbound. This uses the production binding owner rather than parsing
the retail file again. Individual key receipts resolve their actual binding,
including any additional binding that is not the command's first key.

Multiple key gestures with the same `issue_after_step` execute in profile order
before another simulation step. For example, two `N` rows then an `M` row can
observe immediate pending selection changes without a command-drain tick between
them. Every gesture releases its key before the next one; OS autorepeat and held
keys across frames are outside this profile's coverage. The sealed neutral cursor
and all ordinary idle-input checks still apply. A neutral sidebar location can
keep placement or repair cursors out of the tactical viewport during observations.

Profiles containing a key gesture use `map-local-gesture-v3`. Their existing
per-frame and before/after input receipts add `local_input`: current camera
top-left and zoom, follow target, repair/sell modes, targeting kind and type,
`main_rng_cursor: [index_a, index_b]`, the selection-voice gate, and ordered
pending `UnitSelected` voice requests. The required cursor pair is read from
`Simulation::rng_views().main` without advancing or copying the RNG state. Its
before/after values can witness immediate Main stream draws during an input
gesture; they do not establish equality of the whole RNG state. These values
come from the existing input/sidebar/simulation/audio owners. Voice requests
are observed without draining the queue and do not prove audible playback.
They count toward the retained-sample budget. Ordered `selected_ids` and
`selection_pending` continue to report the existing optimistic input selection,
separately from commands queued for the next simulation step.

Mouse-only profiles retain their previous policies and receipt shapes. Keyboard
and sidebar/tactical gestures may share one profile; all its input observations
then include the extended local state. No save/load state replacement is added
to this harness.

The selection-navigation profiles use authored clear-ground maps with unchanged
retail type rules and assets. They disable starting MCVs and starting units, place
a construction yard, power plant and barracks for the local player, and retain an
opponent power plant. No refinery or automatic starting army adds another local
selection candidate. The ordinary map reader creates one MTNK and two E1s at
distinct cells; the singleton variant creates just one local E1 and one distant
enemy MTNK. The enemy supplies the voxel atlas required by the capture resource
check while remaining outside local selection eligibility. The sealed cursor
`[620,250]` lies inside the retained sidebar panel; its right edge x630 is excluded.

| Profile | Input route and observations to check |
| --- | --- |
| [Selection and modes](input_oracle/profiles/selection-navigation.json) | Sequential N/M, then N,N,M at step 4 before another tick; real Repair then N at step 5 and Sell then M at step 6; both directions wrap. Match immediate ordered selection, queued commands, camera and voice requests to the actual local actors. No building is sold. |
| [Singleton Follow](input_oracle/profiles/selection-navigation-singleton.json) | N selects the sole E1, F arms Follow, and N reselects it; F then M repeats the cleanup. The selected identity stays the same while Follow clears. |
| [Pending placement](input_oracle/profiles/selection-navigation-placement.json) | Defense tab and GAWALL cameo start normal production. After a conservative wait, N selects, F follows, and the ready cameo arms placement at step 500. N then M at that same step must leave placement armed, clear selection and Follow, and add no selection voice. Escape then cancels placement before a final N. |

Run the selection-and-modes profile first. Its loaded `keyboard_bindings` and
per-key resolutions must establish N→NextObject and M→PreviousObject. **F is a
provisional literal** in the other two profiles: inspect the loaded binding
catalog and require its receipt to resolve to Follow before counting that case.
Likewise require Escape→Options before counting cancellation. If a local binding
differs, change the literal in a scratch profile; do not dispatch the command
directly or modify the binding to force the result.

Copy each profile to scratch and replace `launch.selected_map_file` with the
absolute path of its tracked `.mpr` before running the ordinary capture command.
The placement witness counts only if the second GAWALL click observes
`is_ready=true` and its after receipt shows `building_placement` for GAWALL.
The wait is a capture budget, not a native production-time golden. Every gesture
receipt has immediate before/after state; a requested frame at step N precedes
the gestures scheduled after that step. The profile schema has no expected-result
field, so a structurally valid receipt alone does not establish these outcomes.

These are production integration witnesses. The independent
[native selection corpus](input_oracle/selection_navigation.md) supplies the
executed search, command-history and voice references. Production actor IDs and
camera pixels must come from the loaded map, not the oracle's fixture identities.

## War Miner Attack return observation

[`map_observation.war-miner-attack.example.json`](map_observation.war-miner-attack.example.json)
uses stock Russia/Battle/AnyTown, ordinary MCV deployment and building production,
and the refinery's free War Miner. At step2500 it ForceAttacks a stationary
friendly conscript. Actor and terrain samples show Attack, combat target removal,
the idle return and later ore consumption. The conscript belongs to the local
house and remains stationary. The command bypasses UI click resolution,
using the existing synchronized command scheduler.

Numeric handles are tied to that launch and should be rechecked after population
changes. This observes production behavior; it does not certify native combat,
scheduler, RNG or rendering parity. See the [mission ownership notes](../src/sim/miner/README.md)
and [original executable packet](spatial_oracle/harvest_attack_return.md).

## Unloading miner and parked aircraft bodies

[`map_observation.war-miner-unload.example.json`](map_observation.war-miner-unload.example.json)
is the War Miner launch above without the attack, stopped at step3775 with the
camera on the refinery pad. In this launch the free War Miner's Unload mission
dumps from step3762 until Harvest resumes at step3794, so step3775 draws its
body from `UnloadingClass=HORV`.
[`map_observation.docked-aircraft.example.json`](map_observation.docked-aircraft.example.json)
uses stock France, builds GAPOWR, GAREFN, GAAIRC and one ORCA by ordinary
production and stops at step5400 with the Harrier parked on the Air Force Command.

Both frames show whether a body is drawn from the sprites its model is seeded with
(`draws_turret_parts` in [`voxel_frame_catalog.rs`](../src/sim/voxel_frame_catalog.rs)).
Release captures of `6dab3753` drew neither body, only its health pips. With the
turret split following the drawn model both are drawn: the simulation fingerprints
are equal and the frames differ only inside x287..349/y246..298 (miner) and
x295..335/y264..293 (Harrier). The same holds in flight (x296..339/y65..101): add
`{"Move": {"entity_id": 1489, "target_rx": 44, "target_ry": 98, "queue": false}}`
at step5200 and stop at step5300 with the camera on (41,95). These observe
production output; no gamemd frame was compared. Numeric handles are tied to
each launch.

The [validation receipt](map_observation.unit-body-draw.validation.json) records
the source and executable identities, frame hashes, changed-pixel bounds and
test results. The step3745 frame immediately before dumping is byte-identical.
Original runs, raw frames, profiles, native disassembly and logs are retained
in the receipt's local evidence archive; executables remain with the shared
build owner. Siege Chopper deployment is tracked separately below; aircraft
shadows remain open.

## IFV passenger turret observation

[`map_observation.ifv-turret.example.json`](map_observation.ifv-turret.example.json)
uses ordinary America/Battle startup, retail rules and an authored map with four
IFVs, a Prism Tank, and GI, Engineer and Tesla Trooper passengers. The command
schedule boards at step 10, unloads at 160 and boards again at 300; it ends at 400.
The empty IFV is a control. Commands pass through the existing synchronized input
queue and gameplay admission; the observation adds no passenger or turret state.

Generate the exact map and a profile copy using the existing fixture owner:

```sh
python - /absolute/evidence/ifv-turret-switching-owned.map /absolute/evidence/ifv-profile.json <<'PYGEN'
from pathlib import Path
import json, re, sys
from tools.render_depth_fixture import build_fixture

map_path, profile_path = map(Path, sys.argv[1:])
assert all(path.is_absolute() and not path.exists() for path in (map_path, profile_path))
mission = build_fixture(walls_only=True).replace(
    'Name=Depth Continuation - Walls and Cliff', 'Name=IFV Turret Switching')
units = [('FV',46,46), ('FV',51,46), ('FV',46,51), ('FV',51,51), ('SREF',56,48)]
infantry = [('E1',50,46), ('ENGINEER',45,51), ('SHK',50,51)]
sections = {
    'Structures': '',
    'Units': '\n'.join(f'{i}=VERA-OBSERVER,{t},256,{x},{y},64,Guard,None,0,-1,0,-1,1,1'
                      for i,(t,x,y) in enumerate(units)),
    'Infantry': '\n'.join(f'{i}=VERA-OBSERVER,{t},256,{x},{y},2,Guard,64,None,0,-1,0,1,1'
                         for i,(t,x,y) in enumerate(infantry)),
}
for name, body in sections.items():
    mission, count = re.subn(
        r'(?ms)^\[' + re.escape(name) + r'\]\n.*?(?=^\[|\Z)',
        lambda match: f'[{name}]\n{body}\n\n', mission)
    assert count == 1, (name, count)
map_path.write_text(mission, encoding='ascii')
profile = json.loads(Path('tools/map_observation.ifv-turret.example.json').read_text())
profile['launch']['selected_map_file'] = str(map_path)
profile_path.write_text(json.dumps(profile, indent=2) + '\n')
PYGEN
```

The map SHA-256 is
`7116830f605a9dc9c4be383bad0152b5ac11243f1813f3b2c04f6a69c85af2cd`.
It retains the fixture's terrain and walls and supplies no type/rules overrides.
Recorded unit IDs are IFVs 13–16 and SREF 17; passengers 18–20. Recheck L0 IDs
after changing a launch or fixture input. Shorter captures set `ticks` to the
chosen completed step and retain only commands with `issue_after_step < ticks`.

Unit observations expose the existing owner words `current_weapon_138` and
`current_turret_124` as paired signed integers. Historical rows without both
remain readable. They establish selected state; retained GPU frames separately
establish rendered output. Neither establishes native raster parity.

The [validation receipt](map_observation.ifv-turret.validation.json) records the
native corpus, build/source identities, capture hashes and inspected images.
All three passengers board at step 12: GI selects weapon/turret `(2,1)`, Engineer
`(1,2)`, and Tesla Trooper `(6,3)`. Departure restores `(0,0)` at step 164;
Engineer and Tesla board again at 309 and GI at 322. The empty control remains
`(0,0)`. These timings are production observations, not native cadence goldens.

Retained GPU frames show the initial empty models at step 10, all four models
at 100, the reset models and departed passengers at 280, and restored passenger
models at 400. The empty frame is byte-identical to the preceding build. A second
full cycle matches all 401 observation rows, fingerprints and final BGRA bytes;
each shorter phase matches the corresponding full-cycle trajectory prefix.

The shared charge-model prerequisite is exercised by replacing the commands
with `ForceAttackCell { attacker_id: 17, target_rx: 60, target_ry: 48 }` at step
10 and `Stop { entity_id: 17 }` at 30, ending at 150. SREF selects index 0 at step
1, 3 at 13, 2 at 38, 1 at 63 and 0 at 88. Separate frames at steps 25 and 50 show
the selected models; their trajectories match the full charge run's prefixes.
This establishes model selection through the existing fire/rearm path, not
whole Prism weapon or combat parity.

At step 25, the preceding build still draws SREFTUR. Selecting SREFTUR3 changes
156 pixels, all inside the Prism Tank's rectangle; all previously exposed
command/actor/terrain observations match across the 26 retained boundaries.

The fixture's resident atlas grows from 5,568 to 8,832 sprites and from
23,394,168 to 34,689,699 R8Uint texel bytes to retain the six additional gun
models. Bodies and shadows remain shared. These are measured atlas resources,
not whole-process GPU memory or a 20,000-unit performance result.

The [native component comparison](spatial_oracle/ifv_turret_switching.md) covers
rules, selection, cargo-head reset, draw/load decisions and shared charge-index
arithmetic. Rust passenger regressions additionally cover Guardian GI, failed
exit, save/load and selected-weapon firing. Existing Temporal gunner reload
handover, malformed custom content and whole Prism combat remain outside this
bounded ordinary IFV cycle.


## Grand Cannon barrel observation

[`map_observation.grand-cannon.example.json`](map_observation.grand-cannon.example.json)
starts France/Battle with stock rules and assets on an
[authored clear-ground map](map_observation/examples/grand_cannon_barrel.map).
The fixture contains a Grand Cannon, a Patriot control and power plants, with no
type overrides. The cannon is actor 1. An ordinary `ForceAttackCell` command at
step 25 targets `(48,59)`; the profile ends at step 120.

Run the profile through the capture command above, from the repository root.
For an external profile copy, set `launch.selected_map_file` to the absolute path
of the tracked map. The [validation receipt](map_observation.grand-cannon.validation.json)
records exact inputs, binaries, native corpora, capture hashes and test results.
The same release build also loaded the unchanged retail-directory `Hills.mmx`
and completed 20 steps with a valid GPU readback. That load uses the idle profile,
selects `Hills.mmx`, and omits the fixture camera and terrain-cell requests.

The idle case removes commands and ends at step 20. The opposite-facing case
changes the target to `(48,37)` and ends at step 160. Shorter copies ending at
98 and 141 capture peak recoil and the return to rest. Their entire observation
trajectories match the corresponding prefixes of the 160-step run. That run's
second execution matches every observation, final simulation hash and BGRA byte.

The inspected Metal readbacks show the restored barrel at rest, aimed in both
directions and during recoil. The opposite-facing shot starts at step 95, reaches
travel 8 at step 98 and returns to exactly 0 at step 141: 46 admitted AI updates,
matching the stock native recoil history. `building.voxel_gun` exposes the
existing primary facing, elevation, HVA counter and immutable recoil state; it
does not supply gameplay state to the simulation.

Before/after captures at steps 20, 120 and 160 have equal final simulation hashes
and identical pre-existing command/actor/terrain observations at every recorded
boundary. This excludes only the newly added `voxel_gun` diagnostic, which the
old binary did not expose. Their changed pixels all lie within the cannon:
372, 446 and 368 pixels respectively. These are endpoint full-state hashes and
trajectory observations, not per-tick full-world hash comparisons.

The fixture's resident atlas grows from 2,304 to 2,400 sprites and from
14,089,600 to 14,319,788 R8Uint texel bytes. Recoil uses the existing rasterized
parts; the resident and last-build sprite counts stay at 2,400 throughout these
captures. These measurements exclude other caches and driver allocations.

The [building comparison](voxel_oracle/building_barrel.md) executes original
loader and draw instructions; the [recoil comparison](voxel_oracle/recoil.md)
executes readers, firing gates and complete update histories. These bounded
component comparisons and production captures do not establish native pixel
parity. Native cache-pool lifetime, custom barrel overrides, upgrade-provided
turrets and EMP cannon missions remain separate mechanisms. The retained
three-pixel drawing anchor and measured subpixel float rounding remain visual
residuals.

## V3 rocket pitch observation

[`map_observation.v3-rocket-pitch.example.json`](map_observation.v3-rocket-pitch.example.json)
starts Russia/Battle with stock rules and assets on an
[authored clear-ground map](map_observation/examples/v3_rocket_pitch.map): the
Siege Chopper fixture's terrain with a pre-placed V3 (actor 1) at (42,48)
facing east, and no type overrides. An ordinary `ForceAttackCell` at step 10
targets (54,48). Set `launch.selected_map_file` to the tracked map's absolute
path in an external profile copy; the loader resolves a relative path against
the retail root.

The observation rows show the V3ROCKET (actor 2) launched at step 31, tilting
on the rail through 95, climbing, at its apex (Z 1393) near 204, diving, and
gone after 253. Copies ending at 60, 150, 204 and 235 (`camera_cell` (48,47))
were captured with release binaries built from `e8502be0e` (SHA-256
`91dc808361fe4255136bd5c993660e4eb05d15b2577c0a4130d69edb90937a65`) and with
the Rocket draw arm (SHA-256
`08ae22351e32a102878c6dc82dbcc9ecb8f5a4526a8706726d45df559db3657d`), map
SHA-256 `8a1a33d9a2705b2f99252b2a94f1b16cffd804b1bafab6b58fa7e966229aa3c5`.
Each pair ends with the same simulation hash. The changed pixels, 823, 854, 292
and 1035, all lie in one box around the missile at most 51 by 66 pixels; the
old binary draws it level, the new one along its pitch. A rerun of the 235-step
copy from the tracked map gives the same BGRA bytes
(`760c19390bf64dcda8d82e9392c06c76cb9e445faa30c04a3834b211eb407ecf`) and the
same actor rows at every step; its state hash differs from frame 0 on, because
the hash covers the map path.

[`rocket_oracle/draw_matrix.py`](rocket_oracle/draw_matrix.py) executes the
original Draw_Matrix and camera product; rasterization is the existing native
port. These captures are production observations, not native pixel parity.

## Nuclear missile observation

[`map_observation.nuclear-missile.example.json`](map_observation.nuclear-missile.example.json)
starts Russia/Battle with stock rules and assets on an
[authored clear-ground map](map_observation/examples/nuclear_missile.map), the
Grand Cannon fixture's terrain with a pre-placed NAMISL, two NAPOWR and three
Neutral MTNK at (40,62), (42,62) and (40,64), and no type overrides.
`IgnoreGlobalAITriggers=yes` keeps the Yuri opponent from forming attack teams.
`observe_super_weapons` adds the Super rows: NukeSpecial is granted on the
first step (interned id 28, charge start 0, 9000 frames) and is ready from step
9001. An ordinary `LaunchSuperWeapon` at step 9010 targets (41,63). Run it
from the repository root; for an external profile copy, set
`launch.selected_map_file` to the absolute path of the tracked map.

With release binary SHA-256
`d39a897f33bc67f4fa06a7a411687bf40fe8276290f3eed37f133e8f6a866fe1`
(31,950,128 bytes) and map SHA-256
`0a3861e8bf90a61cc122ed0233e46cc8764e0074eb9ab852aa76dc48b1883499`, tick 9010
restarts the charge (start 9010) and the silo runs Missile (mission 22) until it
returns to Guard at 9019. The warhead strikes the ground in the AI of frame 9412:
the screen flash starts, the radar marks the cell and the warhead waits on
NUKEBALL, which lives through frame 9432. The AI of 9433 detonates the warhead,
and the three tanks are gone from step 9434 (9413 before the impact was ported);
the silo keeps its 1000 health. The 9700-step run ends with state hash
`17915404168070965052` and BGRA SHA-256
`c95def92a35ce8462b070bf831abd09a0aa0f5555bda795a90419ac9692dd280`. Shorter
copies end at 9414 (the flash fading in over NUKEBALL and the intact tanks, BGRA
`bccdc04430c61d48a6742d1f066e4d5455711abf7786fb536da2ababdb8b044c`) and 9436
(the explosion over the wrecks, BGRA
`5b12621cc428d70a1f9293f021b12edd8e6c9248b271ce79cc8d816525b1c877`). The same
binary loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`, SHA-256
`7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`) and
completed 300 steps (state hash `8142462839629644773`). These are Rust
production observations: the chain's native comparisons are the
`tools.superweapon_oracle` rows, and no whole-run timing or pixel equivalence
with gamemd is claimed.

## Computer nuclear missile observation

[`map_observation.ai-nuclear-missile.example.json`](map_observation.ai-nuclear-missile.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_nuclear_missile.map):
the nuclear-missile fixture's terrain with the computer's pre-placed NAMISL and two
NAPOWR, the observer's NACNST at (40,62), and no commands. `observe_super_weapons`
adds the Super rows; NACNST is the only observed type, which keeps the 9600 steps
under the 100,000-sample budget (the computer's own yard, deployed from its MCV at
(61,61), is observed too). The loader looks a relative map name up in the retail
root: run a profile copy whose `launch.selected_map_file` is the tracked map's
absolute path.

With release binary SHA-256
`d39a897f33bc67f4fa06a7a411687bf40fe8276290f3eed37f133e8f6a866fe1`
(31,950,128 bytes) and map SHA-256
`3cc2686b370b82a91766b5c848bc34f2cd35ba60f772d5587f583778004f9ae7`, the computer's
NukeSpecial is granted on the first step (charge start 0, 9000 frames) and is ready
from step 9001. Its Strategy tick fires it at frame 9018 (the charge restarts there):
AI_TryFireSW aims at the observer's construction yard, the enemy object it values
most, which drops from 1000 to 358 health at step 9442, after the warhead's wait on
NUKEBALL, while the computer's own yard keeps 1000. The radar the computer builds
grants SpyPlaneSpecial at 4013; it is ready from 7613 and the computer launches it at
7720. The run ends with state hash `9108037398604023490` and BGRA SHA-256
`6c601b91bce400dbd3aae22dce724cc486d1286d2125b3dea2b3d71d69ee5985`. The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps
(state hash `8142462839629644773`). These are Rust production observations: the
chain's native comparisons are the `tools.superweapon_oracle` `ai_*` rows, and no
whole-run timing or pixel equivalence with gamemd is claimed.

## Chronosphere observation

[`map_observation.chronosphere.example.json`](map_observation.chronosphere.example.json)
starts America/Battle against a Yuri computer opponent (Easy) with stock rules and
assets on an [authored map](map_observation/examples/chronosphere.map): the
nuclear-missile fixture's terrain with the observer's GACSPH at (46,42) and two GAPOWR,
its MTNK at (40,62) and (42,62), CLEG at (41,63) and E1 at (40,64), and a Neutral HTNK
at (44,54) beside a Neutral GAPOWR at (46,54). `observe_super_weapons` adds the Super
rows: ChronoSphereSpecial is granted on the first step (interned id 32, charge start 0,
6300 frames) and is ready from step 6301. An ordinary `LaunchSuperWeapon` at step 6310
aims the Chronosphere at (41,63); its launch selects the Chrono Warp, and a tactical
click at the view's centre, (316,284) with `camera_cell` (45,55), fires it at step 6320
through ordinary input. The loader looks a relative map name up in the retail root: run
a profile copy whose `launch.selected_map_file` is the tracked map's absolute path.

With release binary SHA-256
`99f036abd074c37628968852dc65e89b273b8d89eb76793129a27504522b0498`
(31,961,040 bytes) and map SHA-256
`803e3f904b9e80c1ffd934f1a0fa4435224d7dd0356ac0ef85c4b1a3f6aa2f96`, the click queues
ChronoWarpSpecial at (45,55). The Chronosphere's charge drops at tick 6310, its expired
timer readies it again the next tick, and the warp clears it and restarts the recharge
(start 6320). The E1 dies at the warp (step 6321). At step 6383 the MTNK from (40,62)
stands at (44,54), where the HTNK is gone, and the CLEG at (45,55); the MTNK from
(42,62), whose cell (46,54) holds the GAPOWR, stands beside it at (47,53) from step
6384. The run ends with state hash `1787085792753166979` and BGRA SHA-256
`c6b8215386fa5806fceab04abe014ae4c9f29d33f2c4da6b0656723a578d269f`. The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps
(state hash `17265848597308621850`). These are Rust production observations: the
chain's native comparisons are the `tools.superweapon_oracle` `chrono_*` rows, and no
whole-run timing or pixel equivalence with gamemd is claimed.

## Psychic Dominator observation

[`map_observation.psychic-dominator.example.json`](map_observation.psychic-dominator.example.json)
starts America/Battle against a Yuri computer opponent (Easy) with stock rules and
assets on an [authored map](map_observation/examples/psychic_dominator.map): the
Chronosphere fixture's terrain with the observer's YAPPET at (46,42) and two GAPOWR, its
MTNK at (43,53), a Neutral HTNK at (44,54), a Neutral E1 at (45,55), a Neutral HTNK at
(40,54) and a Neutral GAPOWR at (46,54). `observe_super_weapons` adds the Super rows:
PsychicDominatorSpecial is granted on the first step (interned id 32, charge start 0,
9000 frames) and is ready from step 9001. An ordinary `LaunchSuperWeapon` at step 9010
aims it at (44,54). The loader looks a relative map name up in the retail root: run a
profile copy whose `launch.selected_map_file` is the tracked map's absolute path.

With release binary SHA-256
`e9af53f1ab23f251bfa5f9fe54a4ac195132d3280e930035c5f48505560e6cab`
(31,986,528 bytes) and map SHA-256
`df5895378341e5c89fa97a215037ee14bf0acfe4d9db6eaeb30657f00e11d99e`, the charge restarts
at tick 9010. At step 9048, 38 frames later, the strike lands: the Neutral HTNK and E1
in the 3x3 block (`DominatorCaptureRange=1`) become the observer's, and the observer's
own MTNK in the block stays its own. The Neutral HTNK three cells off stays Neutral. A
copy that ends at 9060 shows the Dominator tint, the head over the target and the
strike on the cell (state hash `3819314389922297142`, BGRA SHA-256
`43f40774b0ec72121450645a22bec90d51c3a044788e93bc1c134f3834419911`). The 9200-step run
ends under the ordinary lighting, with the captives still the observer's, ringed, and
the Neutral GAPOWR in rubble (state hash `16683750687468866032`, BGRA SHA-256
`18ba69648a0fdda1052dfac0e97de7cd6c13aa1c108ece577f035c51cc0d10c6`). The map is flat,
so these frames don't show a full relight's NukeLevel top, which only raised cells
reach. Before the head and ring anims were binder roots, the same run struck at step
9013. The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps
(state hash `17265848597308621850`). These are Rust production observations: the
chain's native comparisons are the `tools.superweapon_oracle` `psydom_*`,
`update_lighting`, `ambient_step`, `dominator_lighting_read` and `relight` rows, and no
whole-run timing or pixel equivalence with gamemd is claimed.

## Computer Psychic Dominator observation

[`map_observation.ai-psychic-dominator.example.json`](map_observation.ai-psychic-dominator.example.json)
starts America/Battle against a Yuri computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_psychic_dominator.map):
the Psychic Dominator fixture's terrain with the computer's pre-placed YAPPET at (46,42)
and two GAPOWR, the observer's GACNST at (40,62), observer MTNK at (43,53), (44,54) and
(45,55) and a fourth at (52,62), and no commands. `observe_super_weapons` adds the Super
rows; MTNK is the only observed type. The loader looks a relative map name up in the
retail root: run a profile copy whose `launch.selected_map_file` is the tracked map's
absolute path.

With release binary SHA-256
`663c9daa0318802c1f5b59a6632b4023eeda38089f8db82232a4e4d7a6ba99ef`
(32,000,368 bytes) and map SHA-256
`ef6f27be8492ec94ef547460a6afe41f55ae0fd7bd0614e375fa6c9556ce9aab`, the computer's
PsychicDominatorSpecial is granted on the first step (charge start 0, 9000 frames) and
is ready from step 9001. Its Strategy tick fires it at frame 9067 (the charge restarts
there). Each grouped tank counts all three in its 38 cells, and the last-to-first scan
keeps the last of them, (45,55): at step 9105, 38 frames after the launch, the tanks at
(44,54) and (45,55) in the 3x3 block (`DominatorCaptureRange=1`) become Computer1's and
hunt, while the tank at (43,53), outside the block, and the one at (52,62) stay the
observer's. A copy that ends at 9117 shows the Dominator tint, the head and the strike on
(45,55) (state hash `1609889408460178425`, BGRA SHA-256
`f02e8c715978d9cd6dd28c61041cc9a35bb684f1aa9579ddcd3a39da366a863d`). The 9200-step run
ends with the two captives fighting the observer's tank at (43,53) (state hash
`15283598945734857348`, BGRA SHA-256
`8601e5c851079d6afe783bd0d68bcb374776bd4e885a6d403ba4e8a70dd981fd`). The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps
(state hash `17265848597308621850`). These are Rust production observations: the
chain's native comparisons are the `tools.superweapon_oracle` `ai_psydom` rows and the
`tools.ai_strategy_oracle` `all_to_hunt` rows. The run never reaches All_To_Hunt, and no
whole-run timing or pixel equivalence with gamemd is claimed.

## Spy Plane observation

[`map_observation.spy-plane.example.json`](map_observation.spy-plane.example.json)
starts Russia/Battle against a Yuri computer opponent (Easy) with stock rules and
assets and the shroud on, on an [authored map](map_observation/examples/spy_plane.map):
the nuclear-missile fixture's terrain with the observer's NARADR at (46,42) and two
NAPOWR, and no other objects. `observe_super_weapons` adds the Super rows:
SpyPlaneSpecial is granted on the first step (interned id 27, charge start 0, 3600
frames) and is ready from step 3601. An ordinary `LaunchSuperWeapon` at step 3610
aims it at (61,61), where the computer's YACNST stands from step 14 under the
observer's shroud. The loader looks a relative map name up in the retail root: run a
profile copy whose `launch.selected_map_file` is the tracked map's absolute path.

With release binary SHA-256
`c9a45150d4fd1f749f5c468151048854494e4ccf241f0dc5bac727512f05d9e3`
(31,962,592 bytes) and map SHA-256
`b647901c17d20a14ffa642e1c6330d94cba06406c75f8e1f3881a57871b09c57`, the charge
restarts at tick 3610 and one SPYP appears at step 3611 at (80,36), on the observer's
East edge, in Spyplane Approach (mission 30). It turns to Spyplane Overfly (31) at step
3852 at (62,60), crosses the target, flies on straight to (45,92) and is gone at step
4099, past the map's `Size=`. A copy that ends at 3858 shows the plane over the yard
and the first snapshots revealed above it (state hash `8840531564177474398`, BGRA
SHA-256 `62870ae8d39282529b8219a50dffc53b48eae99d346f2c852a58700d3fe2c5d7`). The
4300-step run ends with the computer's base revealed in a band along the flight
(SpyCameraWeapon `Range=20`, `Damage=6`) and the rest of its side still shrouded
(state hash `11954129574118268278`, BGRA SHA-256
`27459b63f5e34ad3bfed8498ff38f3f5ec1b41792397af0ff9575cb946c96041`). The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps
(state hash `17265848597308621850`). These are Rust production observations: the
chain's native comparisons are the `tools.superweapon_oracle` `spy_plane_launch`,
`send_spy_planes`, `spyplane_missions` and `aircraft_leave_map` rows, and no whole-run
timing or pixel equivalence with gamemd is claimed.

## Computer Spy Plane observation

[`map_observation.ai-spy-plane.example.json`](map_observation.ai-spy-plane.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_spy_plane.map): the
computer nuclear-missile fixture with the computer's NARADR in place of its NAMISL,
beside its two NAPOWR, the observer's NACNST at (40,62), and no commands.
`observe_super_weapons` adds the Super rows; SPYP and NACNST are the observed types.
The loader looks a relative map name up in the retail root: run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

With the same binary and map SHA-256
`908edb41ffd794c8d40c6a0c821adba675def5713dd3508e9aacac8224a1cbfa`, the computer's
SpyPlaneSpecial is granted on the first step (charge start 0, 3600 frames) and is ready
from step 3601. Its Strategy tick fires it at frame 3703 (the charge restarts there),
and one SPYP appears at step 3704 at (51,84), on the computer's South edge. It flies
toward the observer's start at (59,33), the base AI_GroundRallyPoint seeds its search
with, turns to Overfly at step 4025 at (61,38), flies on straight and is gone at step
4162. The run ends with state hash `11400018388069219856` and BGRA SHA-256
`a5d57cf3d7fcba530ea7475d607252976121a2e10e11fdaa66d8029e3d67844f`. These are Rust
production observations: the chain's native comparisons are the
`tools.superweapon_oracle` rows named above and the `ai_*` rows, and no whole-run
timing or pixel equivalence with gamemd is claimed.

## Paradrop observation

[`map_observation.paradrop.example.json`](map_observation.paradrop.example.json)
starts America/Battle against a Yuri computer opponent (Easy) with stock rules and
assets on an [authored map](map_observation/examples/paradrop.map): the Spy Plane
fixture with the observer's AMRADR at (46,42), GAPOWR at (40,44) and CAAIRP at (42,38)
in place of its NARADR and two NAPOWR, and the shroud off. `observe_super_weapons` adds
the Super rows: AmericanParaDropSpecial (interned id 29) and ParaDropSpecial (35) are
granted on the first step (charge start 0, 3600 frames) and are ready from step 3601.
Ordinary `LaunchSuperWeapon` commands aim the American paradrop at (52,50) at step 3610
and the paradrop at (48,54) at step 3620. PDPLANE and E1 are the observed types. The
loader looks a relative map name up in the retail root: run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

With release binary SHA-256
`394b6a6e8b17493a1ffa259d3b679cbc80c13e3a82979af102f3cfef8549413d` (32,111,248 bytes)
and map SHA-256 `3e950e89c99852abfeacef0948c9be940297add2735083653800bbcb5e2b2499`,
each launch restarts its charge in its own tick. The run then goes:

- **The planes.** One PDPLANE appears at step 3611 at (63,18) and another at step 3621
  at (80,35), both on the observer's East edge, in Paradrop Approach (mission 26).
- **The drops.** The first turns to Paradrop Overfly (27) at step 3867 at (52,47) and
  drops its eight E1 (`AmerParaDropNum=`) at steps 3868..3903, one every five frames
  (Overfly's return); the second turns at step 3871 at (51,52) and drops its six
  (`AllyParaDropNum=`) at steps 3872..3897. Each paratrooper starts at the plane's 1500
  leptons with Guard queued, its house being human.
- **The exit.** With their cargo gone the planes queue Retreat (mission 4) at steps 3908
  and 3902, fly for cells on the East edge and are last seen at step 4266 at (49,8) and
  step 4216 at (78,35).
- **The landing.** The paratroopers land about 500 frames after they leave the plane,
  from step 4370, and stand on Guard.
- **The losses.** The computer's YAGGUN at (61,58) targets the falling E1 from step 4093
  (a copy to step 4130 that also observes the computer's types: 30 points a hit on the
  first, from 870 leptons up). Three die in the air; each explodes as InfDeath 3 and is
  removed (last seen at steps 4148, 4197 and 4252), as `InfantryClass::ReceiveDamage`
  treats an infantryman still in its Paradrop action (`0x0051836F..0x0051842F`).

The 5000-step run ends with the eleven survivors on Guard (state hash
`3709965344246736099`, BGRA SHA-256
`95cd274f1a661158d569ef4ce0a1c9a4c231b2d0064f66df6591eb2cdacb69bd`); a copy that ends
at 3900 shows both planes over the drop zone and the paratroopers under their canopies
(state hash `17481995937663332501`, BGRA SHA-256
`2dcd177f5550e2f8679411baf017009536fea690e2b881847d8f00d1466e8ca4`). The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps
(state hash `8142462839629644773`). These are Rust production observations: the chain's
native comparisons are the `tools.superweapon_oracle` `paradrop_launch`,
`send_paradrop_planes`, `paradrop_missions`, `drop_payload` and `spawn_parachuted` rows,
and no whole-run timing or pixel equivalence with gamemd is claimed.

## Computer paradrop observation

[`map_observation.ai-paradrop.example.json`](map_observation.ai-paradrop.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_paradrop.map): the
computer Spy Plane fixture with the computer's CAAIRP at (42,38) and its two NAPOWR at
(40,44) and (46,42), the observer's NACNST at (40,62), and no commands.
`observe_super_weapons` adds the Super rows; PDPLANE, E2 and NACNST are the observed
types. Run a profile copy whose `launch.selected_map_file` is the tracked map's
absolute path.

With the same binary and map SHA-256
`d62eabfda8c7f5a0bb6f9508598b6052428a818eb4da6ed8aa83fafae3515e9a`, the computer's
ParaDropSpecial is granted on the first step (charge start 0, 3600 frames) and is ready
from step 3601. Its Strategy tick fires it at frame 3700 (the charge restarts there) at
(60,36), AI_GroundRallyPoint's cell near the observer's start (59,33)
(`superweapon/ai_fire.rs`), and one PDPLANE appears at step 3701 at (50,85), on the
computer's South edge. It turns to Overfly at step 4014 at (59,39), drops nine E2
(`SovParaDropNum=`) at steps 4015..4055, each with Hunt queued, its house being a
computer's, queues Retreat at step 4060 and is last seen at step 4518 at (47,87). The
conscripts land from step 4517 and set off hunting. The 5000-step run ends with state
hash `6357662348380346706` and BGRA SHA-256
`cc27e7523d0575a80238ae912e71380d80d579b3815487703ccfca406e793f97` (camera (60,36)).
These are Rust production observations: the chain's native comparisons are the
`tools.superweapon_oracle` rows named above and the `ai_*` rows, and no whole-run
timing or pixel equivalence with gamemd is claimed.

## Iron Curtain observation

[`map_observation.iron-curtain.example.json`](map_observation.iron-curtain.example.json)
starts Russia/Battle against a Yuri computer opponent (Easy) with stock rules and assets
on an [authored map](map_observation/examples/iron_curtain.map): the Lightning Storm
fixture with the observer's NAIRON at (46,42) and two NAPOWR in place of its GAWEAT and
GAPOWR, and the observer's HTNK at (44,54) with an E2 at (45,55) beside it and a second
HTNK at (44,58), outside the curtain's block. `observe_super_weapons` adds the Super
rows: IronCurtainSpecial is granted on the first step (interned id 30, charge start 0,
4500 frames) and is ready from step 4501. An ordinary `ForceAttack` at step 4440 sets
the second HTNK (id 2) on the first (id 1), and an ordinary `LaunchSuperWeapon` at step
4510 aims the curtain at (44,54). Run a profile copy whose `launch.selected_map_file` is
the tracked map's absolute path.

With release binary SHA-256
`e4c3d1deb17ea073d26d22176eeac4fee36c4c6ba5cac35b6308e374b8b9bf06` (32,092,672 bytes)
and map SHA-256 `5f279999b3bb2f19e157c9133f06eef65a8c161b966a6f5d3ce2db72c257ff16`, the
first shell drops the target from 400 to 310 health at step 4451. Tick 4510 restarts the
charge (start 4510), and the E2 dies at once (125 to 0 at step 4511: its IronCurtain
deals it its Strength). The curtained HTNK takes no damage until step 5307, after the
curtain's 750 frames; the shells then bring it to 220, 130 and 40, and it is gone at
step 5504. The 5600-step run ends with state hash `16791124723229977466` and BGRA
SHA-256 `74fb1550f11b8bb56d06604257649724358d55cd685c948f57193a818ab456ae`; a copy that
ends at 4520 shows the invoke anim over the curtained HTNK and the E2's death (state hash
`7965985861798852240`, BGRA SHA-256
`174b8c4b6f929e607c5d51c483bde2a7fac6aadaa95af1726f89d87d636366c4`). The same binary ran
the computer observation below to its recorded state hash (the computer fires at frame
7512), and loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300
steps (state hash `8142462839629644773`). These are Rust production observations: the
chain's native comparisons are the `tools.superweapon_oracle` `iron_curtain_launch` and
`curtain_overrides` rows, and no whole-run timing or pixel equivalence with gamemd is
claimed.

## Computer Iron Curtain observation

[`map_observation.ai-iron-curtain.example.json`](map_observation.ai-iron-curtain.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_iron_curtain.map): the
computer's pre-placed NAIRON at (55,62), NAPOWR at (66,56) and (66,52), NAWEAP at (56,56)
and six HTNK at (50..52,58..59) near its start (62,62); the observer's NACNST at (32,33)
near its start (37,37); no commands. The HTNK lines end `1,1`, the two recruit flags
(`Techno+0x421`/`+0x422`): with `0,0` no Autocreate team may take them. The observer's
yard stands far from the computer's tanks, because the AI triggers' zone test reads the
enemy's base centre, which a house without buildings lacks. `observe_super_weapons` adds
the Super rows; HTNK and NAIRON are the observed types. The loader looks a relative map
name up in the retail root: run a profile copy whose `launch.selected_map_file` is the
tracked map's absolute path.

With release binary SHA-256
`8b44496850c4fe123a5db50821e82aad3e3cf34e84d484179f15b2e6d93bac58`
(31,917,728 bytes) and map SHA-256
`2db0b369a31466407936ed3a887be7cc9dfc2068b0ff9ddee732f215ad17b97c`, the computer's
IronCurtainSpecial is granted on the first step (charge start 0, 4500 frames) and is
ready from step 4501. Its first team pass with an enemy, at frame 3675, creates the
base-defense team `UseMinDefenseRule=` asks for; the next, at frame 7175, creates the
Soviet Iron Curtain Team (`0ACDAEFC-G`, six HTNK, AI trigger condition 5). The six leave
at steps 7183..7185 for (51,51), 20 cells (`AISafeDistance=`) from the observer's base
towards the computer's (script action 53), guard (action 5), and at frame 7512 the house
fires its Iron Curtain at the team's centre (action 55; the charge restarts there). The
team then attacks the observer's yard under the curtain. Ten frames into the curtain each
HTNK draws its tint number from the Scenario stream (`TechnoClass::UpdateIronTint`).
The 8000-step run ends with state hash `14395597863552765605` and BGRA SHA-256
`04134b470752b79a8cbdb41144210b6367884c3e2b42399190866925eac92704`. A copy that ends at
7530 with `camera_cell` (51,51) shows the team at its gathering point among the
curtain's anims (state hash `3678630503708879027`, BGRA SHA-256
`2d549f944794aef1a46bfcd70448e4adf4e896eebc49bb211e2c53a8e73d33c2`).

With release binary SHA-256
`31784beaed31329ca8d320cd64bfea7dcc55ae9c220c30658de91bb93fe5449e`
(31,966,848 bytes), copies with `camera_cell` (51,51) show the curtained HTNK
drawn at their tint (UnitClass::DrawVoxelBody's curtain arm). At 7511, before
the curtain, they draw at their cell's light (state hash `2002945357488284095`, BGRA
`bce9edd0039a620a3d1c2594d6c8b91cee0f463e2335fc62f12d15746fa8ebbb`); at 7520 they
draw washed bright, in tint stage 2's double intensity (`14596100042634130443`,
`29561032b9f077b98fbdefc17785b3c834a1917e657b4c07185badc89f8250fb`); at 7560,
moving off in the pulsing stages 4 and 5, they draw dark (`4673479376561202833`,
`bbf3a258e2c1d021d85d7ee75e7c61712874f781599c2f088a3352560333fca4`).

## Computer Chronosphere observation

[`map_observation.ai-chronosphere.example.json`](map_observation.ai-chronosphere.example.json)
starts America/Battle against an America computer opponent (`Computer1`, Easy) on an
[authored map](map_observation/examples/ai_chronosphere.map) laid out as the Iron Curtain
one: the computer's GACSPH at (55,62), GAPOWR at (66,56) and (66,52), GAWEAP at (56,56)
and three recruitable MTNK at (50..52,58); the observer's GACNST at (32,33), GAPOWR at
(38,31) and a GAPILE at (66,30) that keeps it in the game. MTNK is the observed type.

With the same binary and map SHA-256
`561e0c682f54e8e84d621cd7683cf87e40cd1bb3137a22ecff550db2ed0171ac`, the computer's
ChronoSphereSpecial is granted on the first step (6300 frames) and is ready from step
6301. The team pass at frame 7175 creates the Allied Chrono Unit Easy team
(`0D2701DC-G`, three MTNK, condition 6). It regroups 20 cells from the computer's base
towards the observer's (action 54), and at frame 7340 the house fires its Chronosphere at
the team's centre and its Chrono Warp, which no building grants, at the observer's
GAPOWR (action 57, quarry 9). The two MTNK in the 3x3 block appear at (39,33), under the
power plant, at steps 7402 and 7403; the third, outside the block, drives after them. The
two share one spot until they move off: the second's own landing cell, (39,32), lies
under the plant, and the warp's blocked search moves it to the cell where the first had
landed (read from `movement/teleport_chrono.rs`, not compared with gamemd). The team then
destroys the observer's yard and moves on to its barracks. The 9000-step run
ends with state hash `11801351645231858838` and BGRA SHA-256
`dc0794bbf93373d106354519149a88a068751905eb0f13834d08fe536b44a409`; a copy that ends at
7405 with `camera_cell` (41,36) shows the arrival (state hash `11728301622766041197`,
BGRA SHA-256 `62ad0af511ef1c65ab607fad0e7f1ac878a9daf65a7cf979eff24a31707390a6`). The
same binary loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300
steps (state hash `8142462839629644773`). These are Rust production observations: the
chains' native comparisons are the `tools.superweapon_oracle` `team_super_actions` and
`iron_tint` rows, and no whole-run timing or pixel equivalence with gamemd is claimed.

## Computer Force Shield observation

[`map_observation.ai-force-shield.example.json`](map_observation.ai-force-shield.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Hard) with stock
rules and assets on an [authored map](map_observation/examples/ai_force_shield.map) laid
out as the Iron Curtain one: the computer's NACNST at (54,54), NATECH at (58,54), NAPOWR
at (54,58) and (58,58) and NAWEAP at (64,52); the observer's NACNST at (32,33), NAMISL
at (37,33) and NAPOWR at (32,38) and (36,38). `IgnoreGlobalAITriggers=yes` keeps the
computer from forming attack teams, and Hard gives it `AISuperDefenseProbability=` 90.
An ordinary `LaunchSuperWeapon` at step 9010 fires the observer's NukeSpecial (interned
id 35 on this map) at (56,56), on the computer's yard. `observe_super_weapons` adds the
Super rows; NATECH and NAMISL are the observed types (a yard's anim slots would
overrun the sample budget). Run a profile copy whose `launch.selected_map_file` is the
tracked map's absolute path.

With release binary SHA-256
`e65fac54591a8405192a8d9198eb8a08535da3a0ba330d033f0435e51bb7f857`
(31,948,912 bytes) and map SHA-256
`c5b3433fc335d7e2c79367678e7ab0ae886aeb4ff4d640fb748dea85abd434ab`, the computer's
ForceShieldSpecial is ready from step 4501 and the observer's NukeSpecial from 9001.
The launch at 9010 alerts the computer (`superweapon/fire.rs`), which defends its first
yard's cell, (56,56); at frame 9013 its Strategy tick fires the Force Shield there, and
the four pre-placed buildings within its radius take it (`logs/ra2.log`: "4 buildings
protected"). The computer's Super is on hold from step 9016 (the shield's blackout).
Copies with `camera_cell` (57,57) show the shielded buildings beside the computer's
unshielded ones. At 9012, before the shield, all draw at their cell's light (state hash
`17972650168677372902`, BGRA
`d3432ae1d9f0813391cc7e84a12b5227a44c26e8239f9d1dc28002fc709510e5`). At 9020 the four
draw washed bright and blue: tint stage 2's doubled intensity, with
`ForceShieldColor=`'s HighBlue word ORed into each pixel (`13441131792540220492`,
`06c2d859bbcbec2c5ef0ec7fd613660642ae7bb983490f4828302b8a5d66e5f7`). At 9100, in the
dark stages, they draw as blue silhouettes (`10592497185235827165`,
`23f5121b7bf1ec5ef501ef6742fee69929f56ad2aa02d7cb06466c1752083a0a`). At 9440 the
warhead has struck under the shield and NATECH keeps its 500 health
(`2335849682608548987`,
`55f9dd93a2cd06d6670da38845f5721b9835b5f3f574e3bc0dd64e9750881506`). The 9600-step
run ends after the shield's 500 frames, with the four at their cell's light again
(`13642096462844057233`,
`442c7cb04d708ef84db95eccce94c12be2227f7be5e9e5d6c878165f63119c07`). The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps
(state hash `8142462839629644773`). These are Rust production observations: the
chain's native comparisons are the
`tools.superweapon_oracle` `drawshp_curtain_arm`, `building_colour_word`,
`anim_colour_word`, `building_anim_light`, `blit_pickers` and `blitters` rows, and no
pixel equivalence with gamemd is claimed.

With release binary SHA-256
`19790e84f6d54d78c8bf5d03510e89f53d11d6b6f41f514e050eea0319835e80`
(31,998,688 bytes), Launch case 10's native building walk (`superweapon/force_shield.rs`)
shields four buildings again ("4 buildings shielded"), and the 9020 and 9600 frames are
byte-identical to the earlier binary's (`06c2d859…`, `442c7cb0…`). The Super rows carry
the fade countdown (`SuperClass+0x50`): idle (-1) through step 9013; 425 at
(14464, 14464, 0), the cell's centre, once the launch step completes (step 9014); 1
after step 9438; idle again after step 9439. So `ForceShieldFading` plays in frame 9438,
425 frames after the launch frame: the 425th SuperClass::AI call, as the
`tools.superweapon_oracle` `super_fade` rows execute natively. The countdown is hashed,
so the state hashes are now `12557075081865037104` at 9020 and `2418361531941558131` at
9600. `XMP03T4.MAP` again completed 300 steps (`8142462839629644773`).

## Lightning Storm observation

[`map_observation.lightning-storm.example.json`](map_observation.lightning-storm.example.json)
starts America/Battle against a Yuri computer opponent (Easy) with stock rules and
assets on an [authored map](map_observation/examples/lightning_storm.map): the Psychic
Dominator fixture with the observer's GAWEAT in place of its YAPPET at (46,42).
`observe_super_weapons` adds the Super rows: LightningStormSpecial is granted on the
first step (interned id 31, charge start 0, 9000 frames) and is ready from step 9001. An
ordinary `LaunchSuperWeapon` at step 9010 aims it at the Neutral HTNK's cell (44,54).
The loader looks a relative map name up in the retail root: run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

With release binary SHA-256
`ace0d7f2bb531086b82792aea3ee659fd8e6717c184b3161352ba415dd015434` (32,042,016 bytes)
and map SHA-256 `6d301f5350faa4100c136315c189bce5c72ed4f00508c378ae4be42a7ec3bea0`, the
charge restarts at tick 9010. The first strike lands at step 9329: the Neutral HTNK
drops from 400 to 150, the observer's MTNK beside it from 300 to 139, and the Neutral E1
dies. Both tanks are gone at step 9334. Scattered strikes then wear down the Neutral
HTNK at (40,54) (275 at step 9337, 88 at 9369), and it is gone at step 9460. A copy that
ends at 9330 shows the Ion lighting, the clouds, a bolt and its explosion on the target
and the storm's line (state hash `5188934786816000448`, BGRA SHA-256
`2c56197586e59652d8747d2919b07edc9a32863442391bcebf8fee9199afdcd8`). The 9700-step run
ends under the ordinary lighting, the struck ground cratered and the Neutral GAPOWR gone
(state hash `692659393727655047`, BGRA SHA-256
`4aef64141d85e1ea534ae1c2974f8f2b6f038f8c4f8803e3764e9093700d34e8`). The same binary
loaded the unchanged retail `XMP03T4.MAP` (`multimd.mix`) and completed 300 steps (state
hash `8142462839629644773`). These are Rust production observations: the chain's native
comparisons are the `tools.superweapon_oracle` `storm_*` and `radar_outage` rows, and no
whole-run timing or pixel equivalence with gamemd is claimed.

## Computer Lightning Storm observation

[`map_observation.ai-lightning-storm.example.json`](map_observation.ai-lightning-storm.example.json)
starts Russia/Battle against an America computer opponent (`Computer1`, Easy) with stock
rules and assets on an [authored map](map_observation/examples/ai_lightning_storm.map):
the computer-nuke fixture with the computer's pre-placed GAWEAT and two GAPOWR in place
of its NAMISL and NAPOWR, the observer's NACNST at (40,62), `FreeRadar=yes`, and no
commands. `observe_super_weapons` adds the Super rows; NACNST is the only observed type.
Run a profile copy whose `launch.selected_map_file` is the tracked map's absolute path.

With the release binary above and map SHA-256
`ce455951310a682a502868ad79ab04dc06d7ef4760b213abb0d757dd541a578b`, the computer's
LightningStormSpecial is granted on the first step (charge start 0, 9000 frames) and is
ready from step 9001. Its Strategy tick fires it at frame 9060 (the charge restarts
there; its AmericanParaDropSpecial, granted at step 5357, fires in the same frame).
Strikes wear the observer's yard down from 1000 health at step 9382 to 330 at step 9552.
A copy that ends at 9400 shows the storm over the yard and the observer's radar closed
by the storm's outage, though the map grants it free radar (state hash
`11635672970713069298`, BGRA SHA-256
`006c3fae3ef0bc754e0532432c3d93da71128616606b18f2c4b55c610320dddd`). The 9700-step run
ends under the ordinary lighting with the radar back (state hash `6127425217724816917`,
BGRA SHA-256 `b764e7437e7ccfa39b017c3942af26637092d82f18adbb7b52449599dcc4574f`). These
are Rust production observations: the chain's native comparisons are the
`tools.superweapon_oracle` `storm_*`, `radar_outage` and `ai_*` rows, and no whole-run
timing or pixel equivalence with gamemd is claimed.

## Computer V3 bombard observation

[`map_observation.ai-v3-bombard.example.json`](map_observation.ai-v3-bombard.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Normal) with stock
rules and assets on an [authored map](map_observation/examples/ai_v3_bombard.map) laid
out as the Iron Curtain one, with these buildings and units:

- the computer's NARADR at (55,62), NAPOWR at (66,56) and (66,52) and NAWEAP at (56,56);
- two V3 at (50,58) and (51,58) and two HTK at (52,58) and (50,59), all recruitable
  (`1,1`);
- the observer's NACNST at (32,33), NAPOWR at (38,31) and NALASR at (34,38).

There are no commands. The map's `[AITriggerTypes]` raises the retail Soviet Bombard
Medium trigger (`0CA24D8C-G`, Normal only, the computer owning a NARADR) to the priority
weight 5000 (`0x006F0C72`), so the first team pass with an enemy picks it. Its TeamType
(`0ACDDB6C-G`), TaskForce (2 V3, 2 HTK) and script (`0607B8FC-G`) are retail. V3,
V3ROCKET, NALASR and NACNST are the observed types. Run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

With release binary SHA-256
`52e75b34f4a77077bb93dc50b74442e733075b7f3855f9a896e7f3092ccf5eb9`
(31,981,648 bytes) and map SHA-256
`0b6d68b6f0aecaf8eb19aefc211dcdc13e7c73a54a578edea2b962eac5bc8def`, the team pass at
frame 5175 creates the Soviet Bombard team from the four pre-placed units. The run
then goes:

- **Gathering.** The script regroups the team at its base and gathers it toward the
  enemy's (actions 54 and 53). The two V3 (actors 1 and 3) move out at step 5180 and
  again at 5584, and are on Guard by 5805.
- **The Tesla Coil.** At 5807 action 0 with quarry 7 (base defenses) orders both to
  attack the observer's NALASR (actor 20). Their rockets launch at 5971 and 6001 and
  strike the coil at 6242 (336 → 136) and 6259, which destroys it.
- **The yard.** At 6262 the script's next attack (quarry 2, buildings) gives both V3
  the observer's NACNST (actor 15). The rebuilt rockets (constructed at 6431 and 6461,
  launched at 6451 and 6481) hit it at 6722 (884 → 648) and 6759 (642 → 411). The HTK
  shell it in between.

The 7000-step run ends with state hash `17689816281688038612` and BGRA SHA-256
`43947b2003bf06a2ef9b6285942cfca8fb40d99f70770d50974935f408d911c1`. The camera on
(34,36) shows the yard, the coil's crater and a third pair of rockets leaving.

These are Rust production observations. The V3's native comparisons are
`tools/rocket_oracle`'s (flight, spawn manager, kamikaze tracker); the team's are
`tools/ai_team_oracle.py`'s. No whole-run timing or pixel equivalence with gamemd is
claimed.

## Computer Dreadnought bombard observation

[`map_observation.ai-dred-bombard.example.json`](map_observation.ai-dred-bombard.example.json)
starts Russia/Battle against a Russia computer opponent (`Computer1`, Normal) with stock
rules and assets on an [authored map](map_observation/examples/ai_dred_bombard.map)
laid out as the Iron Curtain one. A lake of 234 `water09` cells (TEMPERATE tile 322, a
1x1 template) covers x 30..55, y 44..52. The map holds:

- the computer's NATECH at (55,62), NAPOWR at (66,56) and (66,52), NAWEAP at (56,56) and
  NAYARD at (52,48) in the lake;
- its DRED at (48,46), HYD at (46,45) and (46,50) and SQD at (44,48), recruitable;
- the observer's NACNST at (32,33) and NAYARD at (31,45), at the lake's west end.

There are no commands. The map's `[AITriggerTypes]` raises the retail Soviet Navy
Bombard trigger (`0C32D84C-G`, Normal and Hard, the computer owning a NATECH) to weight
5000. Its TeamType (`0EC2038C-G`), TaskForce (DRED, 2 HYD, SQD) and script (`0A869C8C-G`:
attack factories, 49, attack anything) are retail.

Two native rules decide the layout:

- **The computer needs a NAYARD.** AI trigger eligibility needs a factory of the house
  for every TaskForce type (`0x00509610`).
- **The observer's factory must be the NAYARD on the same lake.** A team's attack quarry
  is its leader's `Greatest_Threat` flat walk (`Quarry_To_Threat @ 0x00645BB0` sets
  neither bit 0 nor bit 1). That walk passes the leader's zone (`0x006F8E44..0x006F8EC4`,
  `0x006F9D69`) to `Evaluate_Candidate`'s movement-zone gate
  (`0x006F7E32..0x006F7E9C`), which admits only targets in the leader's Water zone. A
  yard on land is out of reach.

DRED, DMISL and NAYARD are the observed types. Run a profile copy whose
`launch.selected_map_file` is the tracked map's absolute path.

With the same binary and map SHA-256
`7bc2b2e28bf541b993fdc7c7d2d55bdcfc9bbfd718f70d4659ee166e6b595811`, the team pass at
frame 5175 creates the Navy Bombard team. The run then goes:

- **The attack order.** At step 5181 the Dreadnought (actor 1) attacks the observer's
  NAYARD (actor 21) from where it lies, 15 cells off.
- **The volleys.** They launch at 5281/5301, 5481/5501, 5681/5701 and 5881/5901, each
  pair 20 frames apart. The pool refills 160 frames after each volley's second launch,
  80 for the KamikazeWait and 80 for `SpawnRegenRate=`; new DMISL appear at 5461, 5661,
  5861 and 6061.
- **The hits.** DMISL take the yard from 1464 to 1196 (step 5493), 931 (5511), 641
  (5691), 376 (5710) and 74 (5891), and destroy it at 5909. The HYD shell it in between,
  and their flak splash costs some descending missiles 25 health.
- **The end.** The Dreadnought's target clears at 5909 and it returns to Guard at 5913.

The 6500-step run ends with state hash `9448992596267329062` and BGRA SHA-256
`0b42f21982939841debf3cb4837c69b668a383b18c36db0c385cb75520593721` (camera (33,47)).
The native comparisons are as for the V3 observation, with
`spawn_manager::oracle_tests` covering the volley; no whole-run equivalence is claimed.

## Siege Chopper deployment observation

[`map_observation.siege-chopper.example.json`](map_observation.siege-chopper.example.json)
uses a normal Russia/Battle launch and an authored map containing one local SCHP.
It keeps stock rules and assets. The ordinary command schedule moves actor 1 to
(50,50) at step 10, issues `DeployMcv` at steps 200 and 500, then moves it back to
(48,48) at step 650. The legacy command name also carries simple deployment and
undeployment. The run ends at step 750. An enqueue receipt establishes neither
admission nor completion; this route also bypasses self-click and deploy-key input.

Create the map and a profile copy from the checkout, using new absolute paths.
The existing fixture owner supplies the map encoding and clear Temperate terrain;
the edits below author map objects and Houses without changing rules or runtime
state. This is the portable recipe retained in the local evidence archive's
`captures/fixture-reproduction.md`.

```sh
python - /absolute/evidence/siege-chopper.map /absolute/evidence/siege-profile.json <<'PYGEN'
from pathlib import Path
import json, re, sys
from tools.render_depth_fixture import build_fixture, pack_lcw

map_path, profile_path = map(Path, sys.argv[1:])
assert all(path.is_absolute() and not path.exists() for path in (map_path, profile_path))
mission = build_fixture(walls_only=True)
mission = mission.replace('Name=Depth Continuation - Walls and Cliff',
                          'Name=Siege Chopper Deployment Observation')
mission = mission.replace('Americans', 'VERA-OBSERVER')
mission = mission.replace('Country=VERA-OBSERVER', 'Country=Russians')
sections = {
    'Structures': '',
    'Infantry': '',
    'Units': '0=VERA-OBSERVER,SCHP,256,48,48,64,Guard,None,0,-1,0,-1,1,1',
    'OverlayPack': pack_lcw(bytes([255]) * 262144),
    'OverlayDataPack': pack_lcw(bytes(262144)),
}
for name, body in sections.items():
    mission, count = re.subn(
        r'(?ms)^\[' + re.escape(name) + r'\]\n.*?(?=^\[|\Z)',
        lambda match: f'[{name}]\n{body}\n\n', mission)
    assert count == 1, (name, count)
map_path.write_text(mission, encoding='ascii')
profile = json.loads(Path('tools/map_observation.siege-chopper.example.json').read_text())
profile['launch']['selected_map_file'] = str(map_path)
profile_path.write_text(json.dumps(profile, indent=2) + '\n')
PYGEN
```

The recorded map SHA-256 is
`40972558716058e1f0f5b1481824457b5643bc5425e3fa3a09689abe7f1d5cf9`.
Recheck L0 actor IDs after changing any launch or fixture input. The recorded
fixture has SCHP 1 at (48,48) and an opposing MCV far away; it does not induce
combat. Run the copied profile with the build, environment and path requirements
at the start of this document. Retain a full cycle first, then choose phase
captures from its observed trajectory. For a shorter capture, set `ticks` to the
chosen completed step and keep only commands whose `issue_after_step < ticks`.
Each run retains the final GPU frame; earlier trajectory rows alone are not images.

New actor rows include optional `unit` observations: raw deployment bytes
`deployed_6e0`, `deploying_6e1`, `undeploying_6e2`, the landing request
`landing_for_deploy_134`, owner `stage_f8`, persistent `body_counter_538`, and
`deploy_anim_130`. The last is null or contains the retained stable ID and a
nullable `live` body with type, frame and owner identity. A retained ID with no
live Anim stays explicit. Non-Unit rows use null. The wrapper checks types and
ranges, accepts older rows without this extension and preserves them unchanged;
it does not turn these values into gameplay or parity assertions.

The baseline ignores the two deploy inputs and still draws the flying SCHP at
step 400. The candidate release loads the fixture through the production app
and draws each phase below. All eight wrapper receipts are valid; each phase
trajectory matches the corresponding prefix of the full cycle.

| Phase to inspect | Actual completed step and retained GPU frame |
| --- | --- |
| Airborne SCHP before deployment | 190; SCHP visible at Z=500 |
| Landing requested by deployment | 230; SCHP visible at Z=230, landing request set |
| Forward SCHPDEPL with the unit body hidden | 300; attached animation frame 5 visible |
| Deployed SCHD body | 400; deployed flag set, animation reference cleared |
| Reverse SCHPDEPL with the unit body hidden | 550; attached reverse animation frame 5 visible |
| SCHP returned to flight | 640; SCHP visible at Z=500 before the later Move order |

The [validation receipt](map_observation.siege-chopper.validation.json) records
source/build/map/profile identities, every retained frame hash and inspected
image, and the exact transition steps. Landing starts at 202, touchdown clears
the request at 253, forward animation starts at 254, and deployed state begins
at 345. Reverse animation starts at 502, undeployment assigns its nearby
destination at 584, and automatic flight restores Z=500 at 635. The final Move
reaches (48,48) by 750. A repeated full cycle matches all 751 observation rows,
initial/final fingerprints and final BGRA bytes exactly.

The first candidate exposed a missing atlas refresh: its simulation advanced
the transition but the selected palette frames were absent, making SCHPDEPL
invisible. The final build uses the existing atlas owner to service live Anim
palette demands on committed ticks. Against that failed rendering candidate,
all eight runs retain identical simulation observations and fingerprints; only
the forward/reverse frames change, each by 1,561 pixels inside the chopper's
bounds. Both candidates and the baseline remain in the local evidence archive.
These are production observations; no gamemd raster comparison is claimed.

The [native evidence](spatial_oracle/unit_simple_deploy.md) covers the original
transition and Stage bodies, Jumpjet handler branches, deployment admission,
body selection and selected-HVA frame arithmetic. Its
[fixture limits](spatial_oracle/unit_simple_deploy.meta.json) describe supplied
callbacks and the component timeline; they do not establish a full native flight
or world scheduler. Aircraft shadows, type-specific custom palettes,
viewer-dependent disguise palette selection, the forced Magnetron source and
EMP admission remain outside this stock observation. Full native animation
lifetime, audio playback and raster parity are also unclaimed.

## War Miner refinery-cycle observation

[`map_observation.refinery-docking.example.json`](map_observation.refinery-docking.example.json)
uses the same stock Russia/Battle/AnyTown launch and first five ordinary deployment
and production commands as the Attack profile. It issues no combat order and
advances 7200 steps so the free War Miner can fill, dock, deposit and harvest again.
The profile requests no terrain rows to stay within the existing retained-sample
budget. Its numeric command handles belong to that exact launch and must be
rechecked when launch inputs change.

Child/run v6 adds actor `miner` and `radio` projections and frame `houses` rows.
It preserves the existing optional `unit` deployment projection alongside them.
A miner row contains `cargo_bales`, `capacity_bales`, `unload_active` and
`harvesting`; other actors have null `miner`. Radio rows preserve the complete
contact-slot array, including null holes, and the optional `dock_entered_with`
stable ID on both the miner and refinery. House rows follow requested-owner order
and expose the existing economy's `credits`, `spent_credits` and `score`. A
missing House has null economy; the observer never creates a wallet. The score is
House+0x54E8, fed by refinery deposits (x5 per bale), kills and captures, rather
than spendable cash. These immutable reads never send
radio queries, advance timers or change cargo.

Capture manifests and sealed run receipts use compact JSON with the same fields
and a trailing newline.
The map observer and transactional publisher use the same encoder for the
128 MiB receipt limit; indentation previously made the complete 7,200-step
refinery observation exceed that limit. Readers also accept historical indented
receipts.

Use the existing runner with that profile:

```sh
env -u RA2_DIR python -m tools.map_observation \
  --build-label refinery-docking-reviewed-20261002 \
  --profile /absolute/checkout/tools/map_observation.refinery-docking.example.json \
  --contract /absolute/checkout/src/app/diagnostics/tactical_capture/contract.v2.json \
  --cwd /absolute/checkout \
  --output /absolute/evidence/refinery-docking
```

The release observation can show cargo draining alongside the actual House payout,
contact release and resumed harvesting. It is production integration evidence;
native scheduler, movement, audio and pixels require separate comparisons. Native
coverage is recorded in [`spatial_oracle/refinery_dock.md`](spatial_oracle/refinery_dock.md).

The [refinery validation receipt](spatial_oracle/refinery_dock.validation.json)
records two observed deposits at steps 3778 and 5670, release and Harvest at
3794 and 5686, then new cargo at 4229 and 6154. Each deposit paid 1,000 credits
while spent credits stayed at 2,600. Both complete compact receipts are about
59 MB and retain all 7,201 observed boundaries within the 128 MiB limit.

## Repair-depot service observation

[The depot profile](map_observation.depot-repair.example.json) uses stock
Russia/Battle/AnyTown and ordinary Deploy, Build, ForceAttack, Stop,
RepairAtDepot and SellBuilding commands. It damages one starting HTNK, spends
the initial 5900 credits on prerequisites, stops the produced HARV before it
harvests, orders repair, then sells the barracks to fund the retained occupant.
No administrative health, wallet or contact edits are used.

The final release observation on 2026-10-03 reconfirmed HTNK 1375,
NADEPT 1811, barracks 1536 and HARV 1754 before sealing the 6900-step profile.
The tank arrives at the pad at 5484 with 40 HP and zero credits. It retains
the contact through 607 unfunded frames until the sale refund at 6091.
There are 45 paid steps of 8 HP/2 credits: first 6126, second 6211, then 15-frame
intervals. Health reaches 400 and both contacts clear at 6856. Physical movement
starts 6869, the tank leaves the pad cell at 6876 and reaches (35,94) on Guard
by 6900. Final cash is 160, spent credits 5990 and harvested credits 0.

The capture and offline validator both returned `VALID`, retaining 99,286
samples and 63,016,494 run-receipt bytes. The final Metal readback and executable,
source, map, config, profile and validation identities are recorded in
[the validation receipt](map_observation.depot-repair.validation.json).
[The native comparisons and coverage report](spatial_oracle/building_repair.depot_service.validation.md)
separately establish the service arithmetic, timers, radio/arrival and animation
dependencies. These measured runtime IDs/timings are Rust integration evidence;
they are not native whole-match or pixel goldens. Reconfirm IDs before adapting
the profile to different inputs.

## Allied depot waiters

[The Allied waiter profile](map_observation.depot-waiters.example.json) constructs
a stock GADEPT through ordinary Queue/Place and damages three starting MTNKs
through ForceAttack/Stop before repair orders. It starts with 20,000 credits;
construction spends 6,100. One ordinary Move stages the third tank south of
the pad before repair. All three repair from40 to300 HP in33 paid steps/66 credits
each, release at6620/7250/7878 and leave the foundation by7903. The last tank is
outside on Guard with no NavCom, contact or pending entry by7908. Allied A/B/C/D
animations are observed and the final render is inspected. The incidental CMIN
income is retained; repair cost uses the spent-credit delta, not cash alone.

[The crowded profile](map_observation.depot-waiters-crowded.example.json) omits
that staging Move. Its second and third tanks initially select the same outside
parking Cell `(37,94)`. The third stops at `(37,93)` inside the marked foundation,
retains pending1985 through7242 and clears it7243 after the second tank releases.
It stays40 HP without a persistent contact. The capture does not expose the
intra-frame HELLO/CAN_LOAD/BREAK sequence. Both clean waiters also initially
share one goal `(34,92)`, then rest outside the footprint and both receive service;
shared goals alone therefore do not establish a queue bug.

Both profiles advance8,500 ordinary frames through the production retail loader.
Their exact inputs, source/binary identities, validation and bounded observations
are in the `allied_waiters` section of
[the depot validation receipt](map_observation.depot-repair.validation.json).
Numeric handles belong to this saved roster/seed; rediscover them for changed
inputs. [Original executable comparisons](spatial_oracle/building_repair.depot_waiters.md)
separately reproduce marked-cell admission, parking, nearby ally stop and
foundation refusal. They do not certify the full native crowded travel path.

## Natural ore-spread observation

[`map_observation.ore-spread.example.json`](map_observation.ore-spread.example.json)
loads stock AnyTown in Battle mode and advances one ordinary simulation frame.
Its 213 terrain observations are initially empty cells next to raw-map ore,
selected from the same `XMP03T4.MAP` payload recorded by the production loader
(SHA256 `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`).
Coordinates were read with the existing shrapnel-repair map-facts decoder; the
profile does not create ore, alter queues or provide gameplay results. The final
integrated release run witnessed 16 new ore cells at density 3. This is a
production integration observation, not a gamemd gameplay/pixel comparison.
Original executable queue/RNG/Mark comparisons and the final run identities are
in [`spatial_oracle/ore_queue.md`](spatial_oracle/ore_queue.md) and its
[`validation receipt`](spatial_oracle/ore_queue.validation.json).

## Ordinary scheduled commands and actor trajectories

[`map_observation.engineer-repair.example.json`](map_observation.engineer-repair.example.json)
uses ordinary Battle/AnyTown orders to deploy the MCV, construct GAPOWR/GAPILE,
deliver an ENGINEER, damage the power plant by MTNK ForceAttack, stop the tank,
enable paid repair and send the Engineer through CaptureBuilding. The
[production receipt](spatial_oracle/engineer_repair.production.json) records the
1650-step release capture and exact repeat: all fingerprints, complete observed
trajectories and full Metal BGRA bytes MATCH. At frame1602, actual health changes
304→750, damaged slot1581 is replaced by healthy slot1742 and Engineer1557 is
absent after deferred cleanup. Separate frame1601 and1650 GPU images were inspected.
These are observed production timings/IDs, not native goldens. The profile's
numeric handles/IDs belong to this exact stock launch; discover them again when
changing launch inputs. Native fixtures and the physical retail input test cover
the health sample, House consumers and sound request that the map observer does
not expose. Native whole-clock, pixel and audible output remain outside this run.

For TIBTRE observation, use
[`map_observation.tibtre.example.json`](map_observation.tibtre.example.json).
It loads retail AnyTown (`XMP03T4.MAP`), advances 800 simulation steps and looks
at the two TIBTRE02 cells `(74,32)` and `(76,27)` plus their eight neighbors.
Requested terrain rows also retain `overlay: {id, density}` and
`terrain_object: {name, frame, active}` when those owners exist. A nonanimated
object has null frame/active. These are immutable reads of the live owners;
they do not inject ore or advance an animation. The validator accepts old rows
without these fields and requires both fields together on new rows. Native
cadence and admission comparisons are recorded in
[`spatial_oracle/tibtre.md`](spatial_oracle/tibtre.md).

Profile v1 remains accepted and retains its original JSON projection. It cannot
declare the following extension fields, even as empty arrays. Profile v2 adds
optional `commands`, `observe_owners`, `observe_types`, `camera_cell` and
`terrain_cells`; omitted
fields stay omitted in the sealed request, and explicit null is rejected.
The existing v1 example is unchanged. Start retail bridge discovery with
[`map_observation.bridge-response.example.json`](map_observation.bridge-response.example.json):
an accepted Battle launch on stock AnyTown (`XMP03T4.MAP`), with ordinary starting
forces, an allied Computer1 and hostile Computer2. It requests zero steps, observes
both AI Houses and centers the camera on the concrete low bridge near (87,53).
The physical span occupies x86..88/y51..57 at level4 with LOBRDB overlays;
it has no structural elevated deck stamp. Verify those facts in the L0 terrain
receipt before choosing orders. A high-deck response needs a separate map site.
Its disabled shroud is an explicit launch option for this diagnostic observation.
No actor IDs or gameplay state are granted by the profile.

For a long production run, `observe_types` can narrow actor discovery to 1..256
unique, nonempty names from the loaded rule registry, for example `["CLEG"]`.
Names are literal and case sensitive. Discovery requires both a requested owner
and a requested type; omitting the field keeps the existing owner-only behavior.
Once discovered, a stable ID remains observed through owner/type changes, and
its disappearance produces a missing-ID row. The transcript includes
`type_filter` only when requested. House and terrain observations are unchanged,
and filtering never changes the simulation or increases the sample budget.

First retain L0 to discover the actual generated AMCV, MTNK and E1 stable IDs and
positions. A short ordinary deployment probe then discovers the actual GACNST
created by the AI MCV. Seal the final profile with those IDs and observed timings:

```json
{
  "commands": [
    {"issue_after_step": 0, "owner": "Computer1",
     "payload": {"DeployMcv": {"entity_id": 120}}},
    {"issue_after_step": 40, "owner": "Computer1",
     "payload": {"Move": {"entity_id": 121, "target_rx": 87, "target_ry": 53, "queue": false}}},
    {"issue_after_step": 200, "owner": "Computer2",
     "payload": {"Attack": {"attacker_id": 130, "target_id": 140}}}
  ],
  "observe_owners": ["Computer1", "Computer2"],
  "camera_cell": [87, 53],
  "terrain_cells": [[87, 53]]
}
```

These IDs and timings illustrate syntax; obtain actual values from the production
probes. Command payloads are the existing Rust serde `Command`, with no separate
order translator. Supported orders are Move, Stop, Attack, ForceAttack, Guard,
DeployMcv, ForceAttackCell, QueueProduction, PlaceReadyBuilding and
CaptureBuilding (the resolved Engineer repair/capture mission), ToggleRepair,
EnterTransport, UnloadPassengers, RepairAtDepot and SellBuilding. RepairAtDepot
uses the ordinary damaged-vehicle order with `entity_id` and `depot_id` stable
handles. SellBuilding uses `entity_id` and the production sale lifecycle; it can
restore funds for a repair experiment without writing a House wallet.
Rust rejects
ignored payload fields or argument
types. Python checks diagnostic structure and the exact typed request/receipt;
it does not duplicate the gameplay command parser or admissions.

Child/run v5 and later record `observations.rule_types` at L0. These are the actual
rules-owned Infantry, Unit, Aircraft and Structure list names, their categories,
and their existing `interned_id` numeric handles from the loaded simulation.
The observer never interns a name. Obtain the intended type's handle from that
same launch before sealing a production profile; handles are local to that
world's interner and must not be copied between different launches or fixtures.
The numeric `type_id` below illustrates serde syntax, not a stock handle:

```json
{
  "commands": [
    {"issue_after_step": 100, "owner": "Human1",
     "payload": {"QueueProduction": {"type_id": 41}}},
    {"issue_after_step": 500, "owner": "Human1",
     "payload": {"PlaceReadyBuilding": {"type_id": 41, "rx": 87, "ry": 53}}}
  ]
}
```

Use the actual loaded House name, discovered type handle, placement site and
observed ready timing. These orders use the ordinary production queue and
placement admission. Recording an enqueued command does not imply the House
could build it, had sufficient credits, had a ready object, or accepted the cell.

Rows must be nondecreasing by `issue_after_step`, retaining input order for ties,
and occur before the final step. An issue at step N calls the ordinary
`try_schedule_command` producer at simulation tick N, before advancing frame N+1.
The producer owns encoding and queuing; the diagnostic adds no input-delay offset.
Move carries an already resolved semantic destination, as synchronized/replay
orders do; it does not claim to reproduce the preceding cell-click resolver.
The receipt records enqueue success, not gameplay admission or completion.
Ordinary actor/House checks remain in the command drain; verify their actual effects
in the trajectory. Nonlocal-House envelopes are explicit diagnostic commands,
not authority for the local player's UI to control an AI opponent.

`observations.frames` contains L0 and every committed simulation frame. Each
requested House contributes all its represented entities, including inactive
objects. Rows retain stable IDs through capture and record disappeared IDs instead
of silently rebinding them. Actor rows contain physical lepton XYZ, cell, bridge
layer, health/lifecycle, raw Mission current/queued/suspended/effective/handler and
timer state, exact tagged target/ArchiveTarget/NavCom, Foot +688/+68D, Infantry
Doing and the existing virtual +4C coordinate owner's result. An unavailable
coordinate is explicit; observation does not invent one or initialize gameplay.
Foot fields are null for structures. Terrain rows read only allocated real cells,
report current tile/subtile, presentation tile, level/slope and bridge fields, and
report unallocated cells explicitly without stamping the shared Dummy.

V5 actor rows add `building`, null for Foot objects. Structures read their sole
body state, queued body request, construction control, complete serialized
StageClass, ready latch, ActuallyPlaced and the last sampled operational byte.
No diagnostic field advances a timer or calculates a replacement gameplay state.
`building.animation_slots` contains occupied slots in native slot order, with
their actual animation stable ID and the live animation's canonical type name,
interned type ID, native object ID, world coordinates, attachment, Logic membership
and complete retained runtime/timer. A stale occupied slot retains its ID with
`animation: null`; it is never silently omitted or replaced. These rows allow
construction completion and the following operational visit's allocation,
replacement and cleanup to be inspected through the normal match path.

The camera calls the existing presentation centering/clamp owner once at L0.
`render.camera` records its requested cell and actual final top-left/zoom. The
existing neutral-input and exact draw requirements remain in force. A terrain
receipt establishes CPU state; the separate retained BGRA frame establishes the
production rendered output. Neither is a native comparison.

Profiles are bounded to 1024 commands, 30 observed House names and 256 terrain
cells. Captures retain at most 100000 combined actor, missing-ID and terrain
samples, including occupied animation-slot samples since v5 and House rows since
v6. Child/run/report JSON
is limited to 128 MiB on read and publication;
the shared JSON owner keeps its 16 MiB default for other tools. Observe only the
Houses needed by the experiment and choose a bounded step budget. Comparison checks
the complete command/actor/terrain trajectory and camera as well as fingerprints
and pixels; large transcripts remain referenced in their sealed source bundles
rather than being copied twice into the comparison report.

For the response chain, attack Computer1's **deployed GACNST** with Computer2's
ordinary MTNK. The yard exercises Building ReceiveDamage's sourced positive-hit
prelude (`44227E..4422BC` -> `708080`); retail GACNST and AMCV both lack
`ToProtect=yes`. The separate generic Techno damage response uses that flag,
with retail protected miners as controls. Computer1's own E1/MTNK defenders are
the response candidates; an allied local House does not substitute for that
ownership gate. For this map site, verify an actual low-bridge defender and an
ordinary ground control, yard damage, Rescue/AreaGuard dispatch, archived victim,
movement and cleanup. Validate an elevated deck separately. Do not presume
deployment clearance, bridge arrival or random mission selection from this example.
The runtime profile establishes production integration, not native whole-world
equivalence. Visible reproduction uses the normal release shell and the same
Battle choices; this capture route remains hidden. `RA2_QUICKPLAY` has no ordinary
AI starting forces and is unsuitable for this response fixture. Campaign-map
sandboxes remain separate from campaign startup validation.

The shared `tools.child_process` owner bounds spawn/wait/cleanup and collects
finite output snapshots. On timeout it kills only its exact child. Existing
outputs are never overwritten. A failed child, failed manifest, missing artifact,
changed input or invalid receipt produces an invalid report when publication is
possible. Exit codes: `0` valid observation, `1` retained invalid observation,
`2` invalid inputs or publication failure. This tool never claims native parity.

Portable validation:

```sh
python -m unittest tools.tests.test_map_observation tools.tactical_certification.tests.test_core tools.tests.test_child_process
```

The saved [validation receipt](map_observation.validation.json) records the checked
release binary and source snapshot, repeated MIX-map state, loose-map and zero-step
captures, missing-map diagnostics, and the explicit coverage limits. It is a run
summary, not a native oracle golden.

## Validate and compare saved observations

Use the same owner to recheck an existing run or compare before/after captures.
These commands read evidence and write one new JSON report outside the run
directories; they do not launch a game or change the captures.

```sh
python -m tools.map_observation validate \
  --run /absolute/evidence/before-map-change \
  --output /absolute/evidence/before-validation.json
python -m tools.map_observation compare \
  --before /absolute/evidence/before-map-change \
  --after /absolute/evidence/after-map-change \
  --output /absolute/evidence/comparison.json
```

Validation reads the fixed artifact paths under the supplied run, checks actual
profile/config/contract/frame/log bytes and child receipt semantics, then
cross-checks the wrapper's copied evidence and original input identities. It does
not trust a stored `VALID` verdict or matching digest strings. The executable must
still exist at its recorded original path and match its recorded length and SHA;
a labeled preserved build makes that requirement durable. The report marks this
as `EXTERNALLY_REVALIDATED`. It cannot validate a deleted or replaced executable
from its old receipt alone.

A new wrapper v6 run validates its `SEALED_COPY` inputs without requiring the
original profile, config or contract files to remain available. It validates the
retained contract's v2 rules without requiring today's checkout to have identical
contract bytes. Offline checking does not apply the current process environment
denylist because it does not launch a child.

Both runs must validate before comparison. Profile, config and contract **bytes**
must match, including formatting; differing inputs make the comparison `INVALID`.
Executable bytes may intentionally differ, and both verified identities appear in
the report. Comparison checks complete initial/final fingerprints, map source,
exact steps, presentation-clock policy and consumed schedule, resident atlas
statistics, surface format and actual BGRA bytes. Clock policies must match; a
legacy wall-clock observation and a diagnostic-clock observation are `INVALID`
together even when their pixels match.
Differences name precise field paths and before/after values. There are no pixel
tolerances or omitted atlas fields.
Current v6 pairs also compare the complete command, rule-handle inventory,
actor/building/animation/unit/miner/radio/House/terrain transcript and camera. Historical
v5 pairs retain their original building/animation and rule-handle transcript,
including optional `unit` deployment observations when recorded. V4 pairs
compare their original command and actor/terrain transcript and camera, while
v3 pairs retain their original comparison fields.

`MATCH` means these checked observations are exactly equal for the compared
fields; it neither establishes independent execution nor certifies native parity.
`MISMATCH` means valid, comparable observations differ. `INVALID` means an input,
artifact, identity or receipt check failed. `native_comparator` and
`parity_certification` remain `NONE`. Compare exits `0` for `MATCH`, `1` for
`MISMATCH`, and `2` for `INVALID` or publication failure; validate exits `0` for
`VALID` and `2` otherwise. Reports are never overwritten.

Run bundles currently must remain at their original absolute location: command
output and wrapper artifact identity paths are cross-checked against the supplied
run. A copied or relocated bundle is rejected, and validation never follows its
stored artifact paths to read some other bundle. Same-directory and symlink-alias
comparisons are rejected. Keep reports outside both evidence directories.

### Historical captures

Offline `validate` and `compare` reject old wall-clock observations by default.
`--allow-legacy-clock` permits child v2 with sealed wrapper v2, preserving its
original `run.capture` projection. Validation identifies its clock separately as
`legacy-wall-clock`; it does not invent a diagnostic transcript or neutral-input
guarantee. Live capture accepts only child v6 and never offers this override.
Wrapper and child generations must correspond: v6/v6, v5/v5, v4/v4, v3/v3, v2/v2,
or v1/v2. Historical v5 remains readable with its original policy, production
commands, rule handles and building fields, including the optional `unit` extension
when present, without miner/radio/House projections. Older rows without `unit`
remain unchanged.
V5 and v6 comparisons are invalid because their observation policies differ.
Historical v4 remains readable with its original observation policy and profile
v2 trajectory, without production orders, rule-handle inventory or building
fields. V4 and v5/v6 comparisons are invalid because their observation policies
differ. No historical receipt is upgraded or supplied with missing state.
Sealed wrapper/child v3 remains readable without a clock override, with profile
v1 only and its original projection. It cannot declare v4 actor/command/camera
receipts. Comparisons between v3 and v4/v5/v6 are invalid because their observation
policies differ; no missing trajectory is reconstructed.

Wrapper v1 also retained only `profile.json`. It additionally requires
`--allow-legacy-inputs` to reread the original config and contract paths and check
their actual lengths/hashes against the old receipt. These inputs are reported as
`EXTERNALLY_REVALIDATED_UNSEALED`, never as retained copies. Missing or changed
originals fail validation; the profile copy and original executable are checked
as above. Neither legacy flag grants the other permission.

For a comparison between two old wall-clock captures, where one uses wrapper v1:

```sh
python -m tools.map_observation compare \
  --before /absolute/evidence/old-wrapper-capture \
  --after /absolute/evidence/sealed-wall-clock-capture \
  --allow-legacy-inputs --allow-legacy-clock \
  --output /absolute/evidence/historical-comparison.json
```

Same-policy legacy runs may compare when their actual input bytes match. A
legacy/v3, legacy/v4, legacy/v5 or legacy/v6 comparison remains `INVALID` with both flags,
including when frame
bytes are equal. Child v1 remains unsupported historical evidence: it lacks atlas
statistics. No command rewrites historical receipts or adds evidence they did not
record.

Focused portable checks, including malformed/tampered packages, input provenance,
exact mismatches, legacy policy and binary selection:

```sh
python -m unittest tools.tests.test_map_observation tools.tests.test_cargo_run
python -O -m unittest tools.tests.test_map_observation tools.tests.test_cargo_run
```

The [checked comparison validation](map_comparison.validation.json) records
whole-suite checks, preserved-label selection, revalidated historical pairs and
new sealed production captures for this workflow. The earlier capture validation
receipt above remains historical evidence for its recorded source revision.

### Checked diagnostic-clock captures

The [presentation-clock validation](map_presentation_clock.validation.json)
records fifteen independent release captures and eight exact full-frame
comparisons on Apple M4/Metal: three Allied 30-step runs, and two each at Allied
zero/200 steps, Soviet 30 steps, and message steps 1/182/183. All same-profile
pairs matched their complete BGRA bytes, simulation fingerprints, atlas evidence
and consumed clock transcript. The announcement was visible at steps 1 and 182
and absent at 183, preserving its current 4000 ms lifetime from its committed
step-1 timestamp. This is a production regression, not native trigger or message
timeout parity.

The receipt retains full profiles, fixture construction, binary/source identities
and commands. Reproduce the base map with the existing
`tools.render_depth_fixture.build_fixture(cliff_back=True)` owner; append the
receipt's literal trigger for the message fixture. Point each retained profile's
`launch.selected_map_file` at the corresponding new absolute map path, then run
and compare fresh output directories with the commands above. The trigger uses
the current announcement action without ending the scenario.

The original historical same-binary pair still reports `MISMATCH` with
`--allow-legacy-clock`: 11032 radar pixels differed. No pixels are excluded to
obtain the new matches, and historical receipts are preserved. New and legacy
policies intentionally cannot compare. The current 600-case native radar timer
check, retained-surface native check, four explicit retail GPU tests and one
retail radar-history test passed with their existing goldens unchanged.

Ordinary `radar-online-v2` live capture remains untested on this macOS host: its
sealed Windows Verdana path fails the existing POSIX absolute-path checks, and
the installed macOS font does not match its sealed bytes. The diagnostic captures
and native/GPU checks do not replace that production coverage.

## Resident unit-atlas measurement

The final rendered frame records `render.unit_atlas` in the v6 child manifest;
the validated wrapper retains it as `capture.unit_atlas` in `run.json`. The
`UnitAtlas` owner reads actual wgpu texture descriptors and resident entries.
It reports resident sprite count, the last actual build's rasterized sprite count,
and each page's extent, format, dimensions, mip/sample counts and texel payload
bytes, plus their total. Page count is the length of `pages`.

Pages are currently single-layer, single-mip, single-sample D2 `R8Uint`, so the
payload is width × height bytes, including unused page space. Unsupported
descriptors fail observation rather than retaining stale byte arithmetic. The
last-build count includes rasterized shadow sprites, can differ from admitted
resident entries, and stays unchanged on a no-op refresh.

This is resident UnitAtlas texture payload for the captured scenario. It excludes
CPU caches, `VxlPoseFrameCache`, `VxlSlopeTransitionCache`, other atlases, palettes,
driver overhead and peak allocation; it does not establish 30-player saturation.
Statistics are captured with the final render evidence before readback and kept
outside deterministic simulation fingerprints. GPU allocation may differ across
adapters even for identical simulation state.

This replaces the retired `measure-atlas` binary, which estimated a hardcoded
roster and tile sizes without constructing an atlas. The unused
`bridge-oracle-compare` binary was also retired: its trace schema had no repository
producer, and its five bin-only tests covered that abandoned comparison schema,
not active bridge behavior. Current native bridge comparison owners remain in
[`anytown_damage`](spatial_oracle/anytown_damage/README.md) and
[`shrapnel_damage`](spatial_oracle/shrapnel_damage/navigation.md); their native
outputs and Rust regression witnesses are preserved.

For a headless retail growth/no-op/fresh-pack comparison on a real GPU:

```sh
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test -p vera20k --lib --release \
  render::atlas_refresh_retail_tests::retail_atlas_refresh_costs -- --ignored --nocapture
```

Set `RA2_DIR` or provide the local config for that test. It emits the same owner's
statistics at each allocation stage; fresh and grown totals need not match.

The [atlas observation validation](atlas_observation.validation.json) records
before/after production captures and the explicit retail GPU atlas refresh test.
The older `map_observation.validation.json` remains historical v1 evidence; it
has not been retroactively given statistics or new source identities.

Final PR1018 integration includes main2e05101f and snapshot285. Stock R10 and
scenario-zero R11 ordinary v6 wrappers both finish5,600 ticks/exit0 using the
same release binary `9a3cbe2f2e1df8ea8763088bbfffbf694a30efc45193b1b0a4c33ce599e6104c`
(31,667,312 bytes). All11,202 complete factory/tank actor and presence comparisons
match across5,601 boundaries, with3,700 present samples and both objects ending
in Guard at the retained cleared-footprint poses. Final800x600 BGRA bytes remain
`adceea6c241b3e4eb16eb7039d6c1159dfdb376f4d9191bfcc4b3634127c80cb`;
the lossless preview was visually inspected outside the sealed bundles.
`unit-unlimbo-pr1018-final-runtime-summary-20261003.json` retains current wrapper
validation and complete comparisons, SHA-256
`92a0266bebb2298fd35d3821c06e23718c49cdd13b083608162c9c0e3222e8d0`.
The earlier R8 actor data also matches; its overwritten executable basename is
historical evidence and is not newly strict-validated. Stock/zero rule inputs
differ, so this actor/pixel comparison is not whole-run equality or native parity.

After main2909aba80 failure-diagnostic integration, candidate08005785's release
build retains exactly the recorded R10/R11 executable SHA-256
`9a3cbe2f2e1df8ea8763088bbfffbf694a30efc45193b1b0a4c33ce599e6104c`.
Both existing runs revalidate with ordinary v6 against those same current
bytes. This is same-executable evidence continuity, not a new execution.
External `pr1018-diagnostics-release-continuity-20261003.json` records the
identity and validations (SHA-256
`d6413cf7fc50e5e763aeacec6f34ef53e9994ad1dd03a4050b8d88854af07cfd`).
The previous bounded actor/GPU comparisons and native-parity limits remain.
