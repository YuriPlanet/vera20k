# Stock Scenario.Side and EVA selection

Run from the repository root with the configured original game executable:

```text
python -m tools.spatial_oracle.trigger_action_values --eva-selection --check
```

`--write` deliberately regenerates this independent corpus. It does not read
retail sound assets or accept `--inputs`. The default and other optional modes
retain their existing generators and references.

The existing `ActionFixture` and `initialize_eva` execute the original registry
reader `753000` on one synthetic event with distinct Allied/Russian/Yuri
columns. Existing `native_id_snapshot.ScenarioFixture` owns the raw Scenario
stream transport. This mode adds observations and composition, without another
VM implementation, INI parser or queue implementation.

`eva_selection.json` has five sections:

- `synthetic_ini`: exact original-reader input, independently usable by the
  production EVA registry reader.
- `cold`: mapped cold voice selection and empty initialized queue state.
- `history`: sixteen ordered steps with an `operation`, `before`/`after`,
  observed RNG entries and unchanged-state result. Two steps also record
  `dequeue_filename`. Node fields are event identity, type, priority and
  sequence; no guest heap addresses are expected inputs.
- `scenario_round_trips`: four raw signed Side inputs, saved values,
  overwritten pre-load values and actual restored outputs. A `bind_restored`
  history step references its row by `round_trip_index`.
- `pending_eviction_controls`: four independent reader/QueueVoice histories.
  `before/after.entries[event]` records native type, priority and state; the
  same snapshots retain pending and list-node order. `queue_inputs` gives the
  original request order and overrides. Two controls record the actual
  `selected_event` and `dequeue_filename`; the other two call ClearAllQueues
  and ResetAll with no current event.

The controls distinguish explicit state binding from column lookup. Original
`SetSide 7534E0` stores every signed input unchanged except -1 becomes0.
`GetFilename 753380` and dequeue `7528E8..752901` select0=Allied,1=Russian,
other=Yuri from the cached value. They do not query the live House or Scenario
at each request. Queueing under Allied and binding Soviet before dequeue
selects `SOV_B.WAV`; changing only Scenario.Side while Soviet is bound still
selects that same file. `lookup_filename` is the bare column string, while
`dequeue_filename` includes the native `.WAV` suffix. Original event spelling
is retained in observations; Rust may canonicalize identity for its existing
case-insensitive lookup.

Interrupt-list type3 and critical-priority requests take precedence over a
distinct pending event. Both actual dequeue controls remove the pending node
while leaving its event state1; the selected event becomes2 at7528E1, before
filename selection and PlayFile. All valid selected-list paths converge at
7528A7. The failed PlayFile branch752963 leaves2; successful playback later
writes0 at75298E. Those playback continuations are inspected instructions,
not executed physical output in this corpus.

This pending discard differs from explicit cleanup. ClearAllQueues marks
pending2 at752391, critical2 at7523C8, interrupt2 at7523FE and regular-list
entries2 at752436 before freeing their nodes. Both direct cleanup controls
actually observe all four events change from1 to2 and all containers clear;
ResetAll preserves the selected voice. The controls have no current event,
so they do not newly certify an active stream's stop/completion behavior.

Original `ResetAll 7535D0` clears queues/current and pause/suspend depths but
preserves selected voice. The executed `68389F..6838A5` default store resets
Scenario+34B8 independently of that process cache. Its EBP receiver and EBX0
frame come from constructor `6833DB -> SetDefaults683610`, whose zero register
is established at68365A; the rest of the constructor/reset does not execute in
this control.

Scenario.Save `689310` and Load `689470` retain +34B8 in their raw3740-byte
block. The signed -1 control remains -1 in Scenario, then becomes0 only when
the actual restored result is passed to SetSide. CurrentHouse is separate:
LoadContent loads Scenario at67E745, reads saved Side at67E75B and binds at
67E761 through InitSideMixFiles534FA0 before the later globals/currentHouse
load67F9C0. Vox.Load `67F781 ->7533F0` subsequently clears its queues without
overwriting the selection. Ordinary Vox.Save `67E410 ->7533B0` emits only its
start/end tags, with no nodes or selected-voice value. These outer caller and
ordering facts are instruction-level evidence, not a whole-load replay.

Fresh campaign binding uses original ReadString528A10 twice around
Read_INI_Basic: `[Basic] Player`, capacity32, literal default `Americans`,
House lookup50C170, then House.Type+34 ->Country.Side+BC. Noncampaign binding
uses the first native session node's Country. CreateHouses687F66..687F95
selects construction order by color using a visited bitmap; it does not sort
the source vector in place. At6880F8..68810F the original source index0 alone
becomes CurrentHouse. A host-ordered launch representation with a separate
local-player index must preserve this local identity mapping rather than
selecting the first House after color ordering. Fresh source materialization
and the complete resource/adoption caller are outside this corpus.

Two admitted dequeue slices run `7527F8..75295C`: real FIFO removal, event
identity, cached column selection and filename assembly. Device availability,
current-stream/end-time/gap and pause admission are explicit priors. They stop
before `StreamPlayer::PlayFile407B60`; no playback return, successful500ms
timer write, device output or audible parity is asserted. Queue requests use
the existing paused stream-shell premise. Raw Scenario round trips use their
existing stopped timer, empty arrays, clock and IStream transport, not a full
savefile or pointer-swizzling replay. The original ID continuation executed
by that shared fixture is not used as an EVA expected value.

Each EVA history step observes65C780/65C7E0 and compares all three seeded RNG
states. The raw Scenario controls observe draw entries separately; post-read
initialization may reseed their state and is not asserted unchanged. Full
audio cadence, legacy Vox node records, failed-load effects, Ares tags and
extra voice columns remain outside this comparison. Entry points and these
boundaries are retained in `eva_selection.meta.json`.
