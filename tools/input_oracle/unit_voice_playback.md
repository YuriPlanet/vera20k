# GI order acknowledgement playback

`unit_voice_playback.py` composes the existing `GIMoveHistory`,
`EngineerJoinedFixture`, `NativeAudioPlatform` and `NativePcmObserver` owners.
It executes original G (`536D00`), the selection loop (`730D60`), voice request
(`6FFBE0` / `708D90`), a complete original Logic object loop
(`55B5FF..55B61B`), reached Infantry/Foot/Techno AI, the voice head
(`6F9EBB..6F9F0D`), and the shared sound service (`406F70` / `4041D0`).
No native gameplay function or arithmetic result is replaced.

The ordinary two-E1 history retains selection order, actual Logic visits,
voice handles, pending and last IDs, pool/channel state, every Main request,
all raw advances and complete Main/Scenario/MapGen state. Sound event order
is separate from unit storage order. Service boundaries supply an OS clock and
record original strict `>33ms` admission. Cached-sample identities come from
original `401D11`'s index at sample `+14`, through its cache at `+3C` to the
original sorted AudioIndex table. Decoder backend and actual device buffer
identities associate native returned PCM with its pool event; length or digest
similarity is never used to guess the selected sample.

Original Vox `752290` and Speech `752AD0` initialization, called by
AudioSystem `406D14/406D19`, each construct a stream through `407860`.
Original Theme constructor `720960` and complete ReadINI `720590` run with
a declared empty ThemeControl catalog; the stream allocation at `72072B`
is outside the catalog loop. The snapshots retain all three reservations
through `4036C0` / `402440`, including the original group and flags writes.
This leaves 13 of the 16 channels in ordinary group 1. The empty catalog
isolates the startup allocation; it does not establish the retail music
catalog or music playback. Channel rejection and preemption controls use
this initialized shared pool.

The actual thread entry `4095B0` created by original `409511` is resumed with
its original context and a separate mapped stack. Per-buffer OS status and
cursor quanta are declared inputs. Original worker advancement consumes each
sample and invokes its own Stop and endpoint; the driver does not call an
endpoint to manufacture completion. `405A00` completion and the later service
retirement are separate observed boundaries. The worker's OS scheduling is a
controlled sequence, not a real concurrent Windows-device or audible-output
comparison.

The input owner reads selected physical E1 rules through RULESMD, optional
LANGRULE, MPBattleMD and Hills, with physical ART entries; fixed SOUNDMD and
its audio index/bag use the existing original readers. Constructor defaults
and layered VoiceFeedback list controls execute original `710AF0` and
`712D99..712E03`. Feedback component rows enter the original result-2 branch
`702695..7027F7`, retaining percent/list/House ordering, actual positional
sound calls and subsequent Main continuation. The health transition admitting
that branch is supplied and is not established by these rows.

Theme component rows execute original constructor `720960`, NextSong
`720A80` and IsAllowed `721140` with declared resolved catalog fields. They
retain inclusive range requests, rejected raw words, repeat/no-draw and the
1000-attempt fallback, then one actual raw Main continuation. They do not
establish physical Theme parsing or complete music playback.

The outer world is bounded: supplied House/Cells/visibility, an explicit
16-by-16 native map extent, and the existing radar presentation boundaries
are prerequisites. G's actual events remain pending during the complete idle
Logic pass; original event delivery is a later explicit boundary. The reached
post-delivery Infantry head stops before held FindPath `4D3920`. Ordinary
whole-frame event timing, movement, a complete scenario/load stream, real
hardware concurrency, ambient/high-delay loops and invalid/empty sample
admission are outside this comparison.

The one-shot GI controls load one middle sample. They do not cover random
multi-sample playlist preparation or sustaining loops. Original start calls
PreparePlayout at `4045D1` and `404673`; initial attack and flags8 reallocation
take different branches. The additive [stock YTNK comparison](gattling_loop.md)
executes both, native ring lookahead, release/decay and retirement, including a
Loop1/GIMove service-order control. Its corpus extends these shared owners;
these GI goldens and their original coverage remain unchanged.

Use the exact physical input directory named by the payload's `physical`
manifest. The existing joined-input preparation owns archive extraction; this
producer does not modify retail files. Select a supported executable using
`VERA20K_GAMEMD_EXE` or `RA2_DIR`, then run from the checkout:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.input_oracle.unit_voice_playback --check
```

`VERA20K_UNIT_VOICE_INPUTS` selects the prepared joined input directory.
`--write --output /absolute/scratch/unit_voice_playback.json` creates a separate
source-guarded candidate and meta sidecar. Native output, not a Rust result or
hand calculation, supplies every comparison value. Run results and validation
receipts belong outside the repository.

## Stock E1 damage feedback

The additive `stock_e1_feedback.json` comparison joins four complete original
Infantry receivers (`517FA0` → `4D7330` → `701900` → `5F5390`) to the existing
feedback and audio owners. Native physical E1 Strength125 and 105mm/AP packets
produce `125→109→93→77→61`, with result2 only on the fourth call. Result2 uses
the Object receiver's signed `Strength SAR1`, rather than ConditionYellow.
The prior component feedback rows and historical payload remain unchanged.

Three histories retain admitted seed2, rejected seed17, and started unbound
seed2 playback across original Foot UnInit/deferred destruction. The feedback
arm's `70270E` jump joins common `7027F7`, before uncloak at `70281D` and the
remaining receiver tail. Feedback is not an early ReceiveDamage return.

`hits[].rng_before/rng_after` are complete receiver boundaries **before audio
service**. Their main/scenario/mapgen references resolve through the existing
`complete_rng_states` format (`stream`, `sha256`, full `bytes`). Requests retain
stream, original callsite, arguments and results. Scoped native member writes
include receiver timer storage; opaque words are retained as raw values, not
assigned invented semantics. The actual removal calls and all subsequent
service RNG states remain separate from that Core comparison seam.

The live removal hook runs after original playback reaches state3 with a real
channel. Actor destruction leaves the same handle0 feedback event running;
the original worker supplies further PCM and later endpoint/service retirement.
Native cached AudioIndex identities identify the sample. Only returned PCM
lengths, hashes and nonzero sample counts are retained here; existing codec
and service controls own detailed decoder validation. Detailed scratch traces
and raw PCM are not committed.

Four direct packets share native frame0, with no inter-hit AI or reseeding.
They establish a receiver/feedback history, not FireAt, prone recovery or ROF
timing. The existing joined fixture supplies clear16x16 Cells/House/visibility,
selected physical Hills rules, a declared640x480 listener and explicit OS
clock/cursor inputs. Whole scenario loading, concurrent device behavior,
audible equivalence and the complete E1 lifecycle remain outside this witness.
Separate ordinary production combat captures cover firing and removal timing.

Use a freshly prepared directory matching the new payload's `physical`
manifest, including real `Hills.map`, its container and selected audio.idx/bag:

```sh
VERA20K_STOCK_E1_FEEDBACK_INPUTS=/absolute/prepared/extract \
PYTHONDONTWRITEBYTECODE=1 python -m tools.input_oracle.unit_voice_playback \
  --stock-e1-feedback --check
```

`--write --output /absolute/scratch/stock_e1_feedback.json` creates a separate
candidate and binary/command meta sidecar. The existing VM, native readers,
sound registry, RNG, worker and PCM observation owners are reused throughout.
