# TrackProcess replay regression attribution

These are Rust regression expectations, not native whole-scenario goldens. The
global and bridge fixtures compare every record/replay hash; dense folds every
position and Slice6 retains its scripted command/hash regression check. The
native evidence establishes the first changed movement/RNG branch, not all later
gameplay of the scenarios.

Baseline e111f0f1 was rebuilt from a fresh source snapshot with identical fixture
INI files, then the candidate sources were overlaid. Diagnostic hooks consume no
RNG. Candidate v8 diagnostic global final hash 0316C44FFD507F4D, all three streams,
and dense XY fingerprint 9678228745063827247 exactly matched the preserved
uninstrumented v8 executable/full run. All 600 global record/replay hashes match.
The baseline passes the old dense XY and all three global RNG pins; its Slice6,
bridge, and global state-hash pins already fail. Hash owner changes in commits
5afe4e48 (TrackProgress), d4fc1759 (Foot path replay), and 6828ed9f (Foot speed)
precede this host cutover. Historical hash probes still include those owners.

- Dense and bridge first differ at tick 2: with budget/residual zero the old
  adapter eagerly publishes raw point 0, moving subcell X128 to139. Native
  TrackProcess's unpaid gate does not perform that move; the candidate keeps128.
- Global first differs at tick 2: current raw6 point0 (-512,256), transformed by
  Turn26 flags12 (bit4 negates Y) at committed head (2688,3456), is exactly owner (2176,3200).
  Native residual code reloads CURRENT cursor (Drive4B22DC/4B22E8), transforms it
  at4B235D, and interpolates zero delta at residual1. Old adapter peeks next point
  and incorrectly moves subcell128 to129. Retail raw6 point0 is at7E6C50.
- Global first RNG difference is tick116, miner3: corrected movement still has
  live NavCom and raw1 sentinel cursor23/residual5; terminal is paid at117.
  Native ore-search4DCFE7 rejects live NavCom; Harvest73E8C3 then reaches the
  ordinary rate epilogue73EF77..73EFA2 (Scenario RandomRanged0,2). Existing Rust
  arm_rate_epilogue consumes1595561423 (reject low2bits3) then1233293580
  (accept0), moving Scenario state7866300362664327803 to15744955579425093540.
  Main and MapGen streams do not change. e111 had already arrived and took the
  no-draw readiness branch. This is changed callback/arrival timing, not rerouting.
- Slice6 has no position/RNG difference. An initial candidate missing Drive+63
  was rejected and corrected: native fresh4B46C5 publishes valid before head and
  Apply1 even with budget0. The final pin must use that corrected producer.
- The bridge fixture formerly supplied PathGrid/spawn level4 but no resolved
  terrain, so its far-bank class destination was Z0 while owner arrived at416.
  The native owner-Z arrival check correctly rejected this inconsistent input.
  The corrected fixture installs coherent resolved levels/layers and authored
  Clear speed costs and configured mode-one playfield bounds before ordinary
  constructor admission, and explicitly
  requires NavCom, destination, head, and completed path to be retired.

Native executable SHA256:
1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
Reviewed executable corpora live under tools/spatial_oracle/locomotor_track_*.json;
these cover descriptor/cursor/payment/callback/residual gates, not full replay
scenarios. The corrected bridge run passes normal constructor admission, all eight deck
cells, exit height/occupation, completed navigation, and all 200 replay hashes.
The source-reviewed flag correction changes no other entity or stream field in
all 600 global, 300 dense and 16 Slice6 ticks compared with v8.

Observed final state hashes (decimal)

| Fixture | Rebuilt e111 | Corrected candidate |
| --- | ---: | ---: |
| slice6 | 11449180164421722995 | 8518283540073462244 |
| global | 4653094266995239754 | 3346388486345361354 |
| bridge | 8434350462842020354 | 2062022440854870732 |

The corrected bridge column includes the documented fixture correction, so it
is not a same-input production-only comparison. Dense XY changes from
15130124441639639235 to9678228745063827247. Global final streams are Scenario
1037263169536102364 (e1118706727010439834519), Main4175722561206807420 and
MapGen2082941527059030371 (both unchanged). The corresponding historical hash
projections are pinned beside each fixture and still include newer state owners.

Reproduction used the same fixture seeds/INI, baseline e111f0f1 and the candidate
sources. Local receipts: track-attribution-e111-v8-comparison.json, the
track-attribution-{e111,v8,v9,v9b}-*.jsonl traces, and matching test logs under
.local/. The preserved uninstrumented v8 test executable SHA256 is
24741851fa15310002fcdb45da123a1714dcd1c7b1ec4d9505c4f042d19f364a.
These local diagnostics establish attribution; the committed tests remain the
reproducible regression checks. Unimplemented world receivers remain outside
this bounded evidence and are not certified by a stable replay hash.

## 2026-09-15 live bridge repair integration: harness re-pin attribution

Merging the live bridge repair branch onto main `595e3a88` moved every hash
projection of the Slice6, bridge-crossing and global harnesses at once. Two
causes were separated with test-only probes on both trees (main and candidate):

1. Raw infantry occupation owners are the mark-time House index
   (`InfantryClass::MarkCellOccupancy` `0x005217C0` stores Infantry virtual
   `+0x38` -> `House+0x30` into `Cell+0x54/+0x58`), no longer the Rust entity
   id. The raw occupation fold hashes that owner under every schema, so no
   historical projection can reproduce the former encoding. A probe that hashes
   the same composition with the owner fields excluded
   (`HashSchema::BeforeWithoutRawInfantryOwners(161)`) is equal on main and on
   the candidate for the bridge (`A0760B34D47612F6`) and global
   (`EFB3B35ED792E94B`) fixtures. Those two re-pins are therefore
   composition-only; the probe is asserted beside each fixture's pins.
2. Slice6 additionally changes behavior at tick 9, when E1 receives `Attack`
   on the Soviet tank: Walk now defers FindPath to the next Process
   (`0x0075AFC5`), the route goal is the target's own Cell rather than an
   approach cell, and head sub-cell selection draws Scenario RNG
   (`0x004ACA10`). Per-tick dumps of all three entities (position, facing,
   movement target, NavCom, locomotor kind, RNG state, health) diverge only at
   E1 from tick 9 (`.local/harness-repin-20260915/slice6-dump.diff`); the
   dumped MTNK fields and the Drive movement are identical. Its owner-excluded probe is
   `1906B69879B595DE` on the candidate versus `CA5C843EAE4A6A3A` on main.

These remain Rust-versus-prior-Rust pins. The Walk behaviors above carry their
own native corpora (`walk_first_path`, `walk_head_occupation`,
`walk_move_admission`); the pins do not certify whole-scenario native goldens.

## Fresh command/admission ownership: dense comparison against bd5928e6

The isolated bd5928e67db88b452f959f712203d7b1d7a513b9 baseline passed the
existing dense fingerprint9678228745063827247. Diagnostic serialization after
each real `advance_tick` captured the same6000 tick/entity keys on baseline and
the authority candidate (library07). Only XY enters this fingerprint; save/hash
schema changes cannot explain it.

IDs1..10 have identical XY for all300 ticks. IDs11..20 differ at ticks5..175
(1710 rows), and their candidate XY at every tick2..300 equals baseline tick-1.
First difference: tick5/id11, baseline subX127 versus candidate128. At tick2 the
old command has already requested west-facing192; the candidate stores the
destination. At tick3 baseline installs selector54/head(39,5), whereas the
candidate requests the turn and installs that track at tick4. This accounts for
the observed one-tick translation; it is not a whole-game native trajectory proof.

Original Drive SetDestination4AFD40 writes destination and bridge adjustment
after owner guards; it does not invoke Do_Turn. The fresh gate4B3408 samples
Facing::Current4C93D0 and compares all16 bits with `path[0] << 13`. A mismatch
calls virtual Do_Turn4B0EF0 (Facing::Set4C9220), then executes RET0xC with AL1
before admission. Ship has the same corridor6A2A57..6A2AAA. Thus AL1 does not
mean a track was installed. Even ROT0 must return, although its new facing is
already visible in that call.

`tools/spatial_oracle/drive_fresh_turn.py` executes the unchanged Drive gate,
vtable, setter and sampler for120 cases/240 calls, with hashes/provenance in its
sidecar. Eight initial angles include one-bit mismatches hidden by an8-bit cache;
three path directions and five signed raw rates cover instant and finite turns.
The negative rate is supplied raw state, not an asserted rules producer. The
second interior call while rotating does not certify reachability through the
outer Process gates. Aligned cases stop before admission; no world callbacks or
movement execute in this corpus.

This also exposed a candidate defect separate from the intentional admission
delay: `prepare_native_track` set the target after the frame's rotation handler,
delaying the Facing setter. It now returns an explicit TurnFirst continuation;
the world caller uses the same rotation owner immediately and still does not
admit a track. The planner receives a16-bit live sample. Native ROT5 half-turn
starts at frame2, duration25, and samples0x3FEE immediately; integer division
does not require that initial sample equal the stored origin0x4000.

Validation: `fresh_heading_gate_and_facing_setter_match_original_native_rows`
compares native gate/sample outputs; the production test
`fresh_drive_turn_publishes_on_request_frame_and_restores_before_admission`
checks Move through `advance_tick`, same-frame Facing, deferred admission and
save/restore continuation for ROT0/5. Library08 passed these tests, the low-bit
rotation-to-admission regression and configured teleporter restoration: 9,132
passed, four existing replay failures, 134 ignored. It compiled in 5m33s and
tested in 34.8s. Comparing every entity field in all 6,000 library07/08 records
found only ten differences, all at tick3: westbound facing64 becomes192 and
target192 becomesNone. XY and all other entity fields are unchanged by that
setter-timing correction.

Independent read-only review repeated the full baseline/07/08 comparison and
accepted the dense Rust regression pin17756851045285605503
(`0xF66D_01F2_23C5_B07F`). The temporary trace was removed. The XY change belongs
to moving turn/admission ownership out of commands, not the later same-frame
Facing repair. This establishes neither native command/Event/outer Process
parity nor a native whole-scenario golden. Full Facing lifecycle/precision
ownership and the three other replay failures remain unresolved.

The final untraced exact dense test passed (one test, 0.80s;
`dense-final-validation.log`, compile4m50s). A subsequent read-only native
`drive_fresh_turn --check` also passed with the corrected120-row metadata.

Local receipts: `dense-baseline.log`, `foot-coordinate-library-07.log`,
`foot-coordinate-library-08.log`, `dense-baseline08-comparison.json`,
`dense-07to08-comparison.json`, and `dense-07to08-state-diff.json`. Reproduce
the native gate with `python -m tools.spatial_oracle.drive_fresh_turn --check`
under the pinned retail executable configuration from `tools/native_oracle.md`.
