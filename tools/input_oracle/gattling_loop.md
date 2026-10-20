# Stock YTNK Loop1 native audio comparison

[`gattling_loop.py`](gattling_loop.py) composes the existing
[`unit_voice_playback`](unit_voice_playback.md), `EngineerJoinedFixture`,
`NativeAudioPlatform`, `NativePcmObserver` and `tools.native_oracle` owners.
It adds selected YTNK readers/trigger inputs and read-only observations, rather
than a second native VM, codec, channel pool or worker driver. Reference values
are original execution outputs or physical retail bytes.

The exact commands, field mapping, source seals and coverage
limits are in [`gattling_loop.meta.json`](gattling_loop.meta.json).
Its labels are leads; function entry and ABI below follow original instructions.

## Readers and trigger boundary

The original SoundList `7510D0`, sound reader `750440`, UnitType constructor/read
`7470D0`/`747620` and WeaponType `772080` consume selected physical sections.
Unit/weapon layers are RULESMD, absent LANGRULE, MPBattleMD and Hills; ARTMD and
SOUNDMD are fixed, separate inputs. Resulting Report IDs, Gattling rates/thresholds,
constructor defaults and parsed sound fields remain in the corpus. Hills is an
inherited reader fixture, not the production scenario/map used by Rust captures.

The stock Loop1 Sounds list is `vgatlo1a vgatlo2a vgatlo2b vgatlo2c vgatlo3a`;
Control is `attack loop random all decay`, Limit2 and Volume60. The five actual
AudioIndex entries and complete BAG spans are pinned by name, sorted index,
offset, size and SHA256. Original loading creates the actual cached samples.
They are mono16-bit PCM at22050Hz, native cached format words
`[4,0,22050,1,2,0,0,0]`. Word1 selects raw callback `409D90`; it is not the
annotated first format word. Interleaved GIMove selects actual IMA bytes and
format `[4,1,22050,1,2,0,512,0]`, through `409DE0`/`40AA70`.

Physical YTNK/YTNKTUR VXL/HVA bytes are supplied to original type/ART readers.
YTNKBARL VXL/HVA are physically absent. Explicit `None` at the existing RawFile
OS seam allows the original availability/failure branch to execute. This input
does not turn an actual retail omission into a fictitious successful resource.

Original Unit constructor `7353C0` receives NULL House and constructs Drive COM.
The fixture then declares House fields `21C`/`14C` and stationary coordinate
`3456,3712,0`. It supplies admitted fire code2 to the original suffix
`737063..7370AE`, which reaches Increase `70DE70`, the real Main report pick and
positional Play `7509E0`. Whole Unit `736DF0` reaches the no-target release path.
No gameplay return, endpoint, decoder output or RNG value is substituted.

These are bounded prerequisites: no Unlimbo, complete House/world registration,
target search, GreatestThreat, FindPath, full projectile/damage or Scenario
schedule is established. The sound chain itself executes original bodies.

## Start, ring fill and cleanup

Increase reaches the stage-change Stop at `70DF74` and the report-entry Stop at
`70DF9C` before its raw Main draw at `70DFC6`. Stop `405D40` finishes driver stop,
endpoint and handle clear synchronously. The driver and worker use the same
critical section `87E880`; Windows thread scheduling itself is not replayed.
The Rust frame borrows a sim-defined sound boundary implemented by the existing
app/SfxPlayer owner. It delivers reached Stops before the raw draw, releases
before Update/Limbo owner writes, and then delivers only the remaining frame
sound suffix. Other frame consumers retain all facts. This capability and its
frame rules are never stored in Simulation. Device-free regressions compare
real retained-source controls/Main against this corpus; production captures
provide the separate app/device integration evidence.

Original SoundService `4041D0` calls Prepare at `4045D1` and `404673`. Each Prepare
rebuilds the remaining middle list and selects one next source; it does not draw
a complete permutation. Initial audible Play leaves flags8 clear, so both calls
return the attack without a Main draw. Only reallocation `750D40` reaches
SkipAttack `4052E0`; its two prepares each draw from the reset middle list.
This corrects issue1316's proposed two-draw initial-start interpretation.

Initial DriverStart `40A340` fills the ring synchronously inside each ready
event's start. In the two-event row, Loop1's Load/Prepare/initial ring advances
precede GIMove's sample-load selection. Admission pitch/volume for both requests
has already happened. The actual GI caller boundary is global Play `750920`
(ECX index, EDX pan, two stack words), not positional Play `7509E0`.

DriverStart arithmetic `40A4A6..40A551` derives block alignment from channels and
sample bytes, then average bytes/sec from sample rate. For positive stock inputs,
its quarter is `round_up_1024((u32(avg_bytes_per_second * 3) / 100) * 8)`;
the ring is four quarters. Original unsigned division is MUL `0x51EB851F` and
SHR5 at `40A519..40A524`, followed by the signed1024-rounding instructions.
Execution yields quarter11264 and ring45056 bytes. This is not a formula fitted
to those numbers.

Original Fill entry is `409880`; `4098C0` lies inside that body. The initial
45056-byte ring contains attack30392, body9442 and first5222 bytes of another
body. Advance's Main choices occur when filling this lookahead, before those
bytes are audibly consumed. A caller selecting only at audible clip boundaries
cannot reproduce this RNG order. Filling exactly the current source remainder
does not advance the playlist; the following positive-capacity fill does.

Unit release calls `406060`, flags`0xA -> 0x6A`, and clears its handle/latch.
Next service stops the old driver. That driver's `40A63B` callback reaches
`405A00` before decay is restarted. Advance queues decay and sets bit`0x10`;
restart flags are`0x3A`. Prepare retains bit`0x10`; it does not reset the native
decay-used flag. The decay ring contains36876 actual bytes and8180 native zero
bytes, observed at silence writers `409B0E`/`409B15`.

The actual worker subtracts valid playback bytes by consumed quarters. Its
valid counter reaches`-8180`, invokes its own Stop, and on the next visit invokes
the endpoint. Endpoint flags`0x38`/state4 precede subsequent pool retirement.
Preload EOF, driver stop/replacement, worker endpoint and service removal are
separate boundaries. These values belong to the stated row inputs.

## Corpus and controls

| Row | Native coverage |
| --- | --- |
| `stock_initial_audible_loop1_soft_release` | Initial attack, body permutation/restart, release, decay, endpoint and retirement |
| `stock_inaudible_then_loop1_reallocation` | Inaudible request, actual reallocation/flags8, both preparations and complete cleanup |
| `stock_loop1_exact_source_end_fill_control` | Supplied fill8966 then2; exact completion versus next-source selection, then cleanup |
| `stock_loop1_hard_stage_stop_then_loop2_control` | Supplied Increase ticks199; original hard stop/new stock Loop2 report and decay cleanup |
| `nonretail_unresolved_middle_index_native_slot_control` | One supplied resolved index-1; original failed cache load compacts success slots before role exclusion |
| `stock_loop1_then_GIMove_same_service_order_control` | Two requests in one service; original per-event initial-fill/Main order and actual IMA output |
| `declared_Loop3_original_reader_release_and_budget_control` | Only Loop=3 authored through original reader; handle release, finite passes, native decay and retirement |

All physical audio clips remain present in the unresolved-index control. It is
a component failure input, not a stock missing-file claim. The Loop3 row is the
sound reader's counted Loop property, not stock Gattling stage3. Counted release
retains event/serial while clearing handle type; it does not set owner-loop
release bits. Both controls execute original downstream behavior.

`shared_one_shot_controls` adds four stock GIMove controls: global `750920` or
positional `7509E0`, each followed by Release `406060` or generic Detach
`405FD0`. The existing original GI/Walk construction and Unlimbo fixture owns
the live handle at `4DC`; only the request and cleanup call boundaries are
supplied. All select the physical `igimod` clip, decoded by original IMA output
to34578 bytes. Release preserves playing state3/flags`0xA`, clears only handle
type, and lets the worker reach its actual endpoint. Generic Detach sets
flags`0x6A` and clears handle event/type while retaining serial. On the next
service, original UpdateState `4055C0` stops the driver before the DECAY tests;
the driver's own endpoint produces state4/flags`0x28`. Later service retires
the pool entry. The Released-bit test at `405894` applies to playing one-shots
as well as loops. These controls distinguish generic Detach from the Loop3
row's Release; none supplies a fake endpoint or claims hardware timing parity.

`rows[].boundaries` supplies literal before/after Unit, pool/event/channel/driver
state, inputs and ordered RNG requests/advances. `complete_rng_states` stores
all deduplicated1012-byte Main/Scenario/MapGen buffers referenced by snapshots,
fills and callbacks. `native_ring_fills` retains requested capacities, actual
ring hashes, driver before/after and silence-write ranges. `raw_pcm_copies` and
`ima_pcm_copies` retain native cached identity, source offset/count, actual PCM
count/hash and both RNG boundaries. `physical_clips` keeps one physical PCM bank;
IMA PCM is concatenated only from executed original output callbacks, including
the final decoder flush. Repeated ring/callback hex is mechanically omitted
after physical-source and original-write validation.

Vox/Speech and an empty ThemeControl initialize original shared reservations.
The actual worker entry `4095B0`, created by native `409511`, retains its context
and separate stack across Sleep. Win32/DirectSound clocks, status, cursor quarters
and successful Play status are explicit single-thread OS inputs. Scheduled visits
are bounded test inputs, not Windows concurrency, exact whole-frame timing,
resampling, final device gain/pan, audible output or full engine parity.

## Reproduction

Use the exact prepared input manifests from the payload. The same existing GI
input root has36 prepared files, including its retained `receipts.json`; the
separate voxel root has four pinned physical files
and the two declared absences. Set the supported executable and directories:

```sh
export VERA20K_GAMEMD_EXE='/absolute/retail/gamemd.exe'
export VERA20K_GATTLING_INPUTS='/absolute/prepared/joined-inputs'
export VERA20K_GATTLING_VOXELS='/absolute/prepared/voxels'
PYTHONDONTWRITEBYTECODE=1 python -m tools.input_oracle.gattling_loop --check
```

Checks write no files. `--write --output /absolute/scratch/gattling_loop.json`
deliberately generates a separate corpus and sidecar. Normal/optimized import
and `--help` are inert without retail data. Retained receipts, disassembly
packets, raw audit output and test results remain outside the repository.
