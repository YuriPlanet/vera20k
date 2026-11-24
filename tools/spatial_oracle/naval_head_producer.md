# Native west-water Ship head prerequisite

`naval_head_producer.py` executes the original ordinary AEGIS destination setter,
full Foot path search and first Ship Process. It supplies no path answer,
CanEnter answer or head coordinate.

The source is Shrapnel cell `(113,59)`, physical `(29056,15232,208)`, with a Move
request to `(117,59)` at frame26 and first Process at frame27. The actor begins
marked, with no retained head or route and stable facing raw16384 (production
facing64). Human owner, Move mission and game mode5 are explicit inputs. This
begins at the ordinary Unit destination setter boundary, after command dispatch;
the Windows game loop, command dispatch and preceding26 frames do not execute.

## Executed result

- Unit destination `741970` calls Foot `4D94B0` and Ship MoveTo `69F450`.
  It publishes destination `(30080,15232,208)` without a path or head.
- Ship Process `69FC10` calls Foot FindPath `4D3920`, wrapper `4CBBA0`, Pathfinder
  `42C900` and the actual AStar body `429A90`.
- Concrete Unit CanEnter `73F0A0` admits the relevant Water cells. Both FindPath
  and fresh-head admission perform actual Mark0/Mark1 and current-cell Recalc.
  No gameplay callable is replaced.
- FindPath returns AL1 with directions `[2,2,2,2,-1]` (eastward cells114..117).
  The raw EAX upper bytes are retained only as observation, not boolean meaning.
- Original fresh coordinate math `6A28FF`, admission and finalization publish
  head `(29312,15232,208)`. The XYZ stores finish before `6A3D09`. After the full
  first Process returns, physical XYZ is still `(29056,15232,208)`, the retained
  route is `[2,2,2,-1]`, selector18, cursor0, head-valid1 and speed fraction1.
- All three full RNG states remain unchanged. The ordered trace and native
  actor/locomotor writes retain timer changes. Native FrameTimer's middle dword
  `+644` carries raw stack bits; logical start/duration are `+640/+648`.

These head and physical coordinates match the separately captured ordinary
production Shrapnel trigger. This packet does not itself execute repair, sinking,
later paid movement or a complete loaded scenario. Those remain separate
composition witnesses in the adjacent repair, occupants and lifetime packets.

## Map, readers and setup bounds

The frozen [`shrapnel_repair/hierarchy_composition.py`](shrapnel_repair/hierarchy_composition.py) builds initial
connectivity and all three hierarchical graphs with original `56C510/581F90`.
It starts from supplied full production class/height planes checked against
physical MAP allocation/levels, rather than borrowed graph IDs or edges. Its
whole native-built map and graph buffers are copied unchanged into this fixture.
Every CanEnter query must stay within the170 supplied physical region cells;
queries outside that coverage fail. The original Pathfinder constructor,
map-array resize and hierarchy scratch allocation execute in the receiving
fixture, at `42A6D0`, `42AC00` and `42C1C0`.

Actual original selected reads consume physical RULESMD, MPBattleMD and MAP
layers through lexical INI cache seams: `[AEGIS] Speed` at `71464A`, `[AEGIS] ROT`
at `714B14`, `[General] CloseEnough` at `670EDD`, and `[AI] PathDelay` at `6739E5`.
The retained outputs are recorded per layer: Speed10, ROT1, CloseEnough576,
and PathDelay little-endian double bits `00000040e17a843f`
(`0.009999999776482582`). The original admission fixture
provides its other native AEGIS/land/warhead reads. The active MPBattleMD input
is pinned by the adjacent physical input packet and matches the recorded hash of
the former MPBattle.INI alias.

Original direction, Ship and Map-height initializers, Tactical constructor and
Foot footprint initialization execute. Source-cell membership is supplied;
full Unlimbo, visibility reveal, House registration and scenario construction
are excluded. The map's resident TMP/overlay inputs permit actual Mark/Recalc.
Successful allocation/free, CRT atexit registration and setup Tactical OS clock0
are declared environment seams. No registered exit callback runs in this interval.
The inherited FPCW0E7F is supplied, not claimed as a hardware capture.

From the repository root with the native Python environment and pinned original
executable configured:

```sh
VERA20K_SHRAPNEL_INPUTS=target/shrapnel-native-inputs/extract \
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python tools/spatial_oracle/naval_head_producer.py --check
```

Only `--write` replaces references. The default is read-only comparison.


## Replay and publication receipt

The explicit `--write` and a separate subsequent `--check` both returned exit0.
The result contains one connected composition, three stage snapshots,44 ordered
path events,45 inherited receiver observations and79 actor/type/House/locomotor/
rules/warhead writes. Ten actual CanEnter calls appear as20 entry/return events.
At interior instruction boundaries, the event field `this` is only the retained
raw ECX register; it does not assert that ECX still names a class receiver.

The repository runner imports the existing `tools.spatial_oracle.naval_occupants`
and `tools.spatial_oracle.shrapnel_repair.hierarchy_composition` owners. Physical
inputs use `VERA20K_SHRAPNEL_INPUTS`, including the active `MPBattleMD.ini`.
Promotion changes only imports, paths and corresponding metadata prose. The
published result remains byte-identical to the frozen original (SHA256
`e95fbb0ff148b9bc1be44a36f9f26cd929332382703300ff3c5d1af0c74df497`).
