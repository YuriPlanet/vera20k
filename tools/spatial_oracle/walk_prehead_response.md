# Walk admission response

`walk_prehead_response.py` executes the original Walk result decoder starting at
`0x0075B696`. Its 124 supplied-state controls cover return classes 0–7, both
ProcessMovement argument values, timer boundaries, ordinary Infantry callbacks,
obstacle and wall overrides, uncloaking, head selection, and 36 recursive
continuations, including 21 appended failed-retry controls, plus two retained
Dummy-land contrasts. The classes and
FindPath answers are **supplied**, not outputs of
Infantry `0x0051BF90` or AStar. This corpus establishes the caller's response to
those answers; it cannot establish that an actual map produces them.

The canonical payload SHA-256 is
`a33d873bff26c7c32ee337d39ec4791eec771369763c2bd2931ccfd9c6672c07`.
Unicorn binding 2.1.4/core 2.1.33621247 produced the checked output. The JSON
records every input, before/after state, ordered callback observations, endpoint,
and complete 1,012-byte Scenario RNG state. The `.meta.json` lists all supplied
state, substitutions and bounds.

```sh
python -m tools.spatial_oracle.walk_prehead_response --check
# Explicit regeneration, after reviewing the changed evidence:
python -m tools.spatial_oracle.walk_prehead_response --write
```

Default invocation also checks. `VERA20K_GAMEMD_EXE` or `RA2_DIR` selects the
original binary through the shared [native runner](../native_oracle.md).
The harness imports the existing repository `walk_head_occupation` fixture;
there are no external helper dependencies. Promotion from the exploratory
101-case corpus preserved its JSON byte-for-byte. The failed-retry extension
retains those first 101 row objects unchanged and adds 21 controls. The two
Dummy-land controls retain all preceding 122 row objects unchanged. An explicit
`--write` followed by an independent `--check` on 2026-09-27 validates the
124-case corpus. No Rust test or full gameplay parity is implied by that
native-only check.

## Caller and query identity

The active ILoco Process table is Walk `0x007F69F8 + 0x40 -> 0x0075AC80`.
That wrapper pushes **1** before calling ProcessMovement `0x0075AEC0` and sets
Walk `+0x35` during the call. Its three internal retry calls at `0x0075B716`,
`0x0075BABB` and `0x0075BC04` pass **0**. The JSON input key `restart` is this
original argument: true denotes the ordinary first pass; false denotes the
internal retry pass. It is not an indication that a previous turn failed.

The pre-head caller reads retained Foot path word `+0x5E0` at `0x0075B302`.
Direction 8 takes the tube arm. For the ordinary arm, `0x0075B5A7` masks a copy
with 7 solely to index the lepton vector table `0x0089F6D8`; the original
retained direction is passed unchanged to virtual `+0x1AC`. Prospective XYZ is
physical Object `+0x9C` plus that vector, rather than a cached destination.
The five arguments at `0x0075B690` are:

1. Candidate Cell from `Map::GetCellAt(CellStruct*) @ 0x005657A0`.
2. Original retained direction.
3. Physical current Cell signed `+0x11B`, plus 4 if Object OnBridge `+0x8C`.
   `0x005F5F00` obtains this through Object virtual `+0x1BC -> 0x005F6960`;
   that original receiver makes two `Map::GetCellAt(CoordStruct*)` queries.
4. Null previous-Cell pointer.
5. Literal 1.

The continued-recursion rows execute these original argument producers, then
record and substitute the admission result. They do not execute `0x0051BF90`.

## Result handling

Every nonzero class clears Walk motion byte `+0x36` and calls owner virtual
`+0x548` before class-specific work. On Infantry this is `0x00521B20`, which
changes Doing 3, 6 or 17 to -1 and leaves other Doing values unchanged. Class 0
skips this callback entirely.

| Class | Original response |
| --- | --- |
| 0 | `0x0075BC13` calls `FindSubCellDest @ 0x0075C240` with the exact prospective XYZ. Accepted head sets motion true, then facing and speed 1 if the owner is alive. Rejected head sets speed 0; it does not explicitly clear the preexisting motion byte. The normal tail clears Foot `+0x68A`. |
| 1 | `0x0075BB9D` calls Cell `0x00483480` on the candidate ground list **before** checking the first-pass argument. Then follows the same retry/retain arm as class 7. |
| 2 | `0x0075B8A0` owns blocked-latch/timer and FindPath retry handling described below. It never uses the one-shot recursive arm. |
| 3 | `0x0075BA09` calls `Map::Cell_Gates_Admit_Mover @ 0x00578AD0` with mover and candidate cell; discards its return; sets Foot retry count `+0x64C` to 10; returns. |
| 4, 5 | `0x0075BA46` queries candidate ground content through `0x0047C5A0` before checking the first-pass argument. First pass retries. Second pass may Override_Mission to Attack an enemy object or wall Cell, then invalidates path word 0, sets speed 0, Stops Walk and clears `+0x68A`. |
| 6 | `0x0075B6B8` resolves the candidate Cell. First pass retries. Second pass either cancels near the destination under all guards below, or scatters the selected cell list. |
| 7 | `0x0075BBB3` first pass retries. Second pass returns after the common motion/Doing changes and retains the existing path, destination, NavCom, moving byte and timers. There is no CloseEnough test. |

The shared first-pass retry invalidates only path word 0, assigns the null head
coordinate directly, starts the movement timer at the current frame with duration
0, and recursively invokes ProcessMovement(false) **in the same visit**. It does
not itself Stop Walk, clear NavCom, detach an object or spend an RNG draw. The
recursive empty-path request uses urgency 0. Supplied successful FindPath rows
install a ready path and continue to another admission query; its refusal cannot
start another recursive retry. The 36 continuation rows execute this control
flow, including original callbacks and speed/Stop/setter consequences. FindPath
failure controls supply only its answer, so they do not claim its wrapper's
additional side effects.

### Failed recursive no-queue continuation

After the recursive empty-path FindPath answer is false, the caller rechecks
the destination through owner `+0x2CC`. A retained zone reaches `0x0075AFEE`,
which invokes owner `+0x4F4` before measuring the physical-coordinate distance
to the **live** Walk destination. This matters for effective Hunt mission 15:
the original failure callback clears that destination, and the executed
distance changes from 202 to 3,630. A queued Hunt mission with current mission
-1 has the same effect. This suffix does not read path-reference coordinates,
retained direction, path words or NavQueue.

At `0x0075B046`, distance strictly less than CloseEnough clears NavCom through
`+0x480(NULL,1)` only when Techno radio tether byte `+0x418` is false. The
controls pin distance 202: CloseEnough 202 does not cancel; 203 cancels when
untethered and does not cancel when tethered. Otherwise, a nonzero **32-bit**
Foot retry count `+0x64C` decrements once. Entering with 1 produces 0 and goes
straight to the common speed-0/Stop tail; exhaustion work waits until a visit
that starts with zero. The controls include 0, 1, 2, 10, 256 and `0xFFFFFFFF`,
the last of which becomes signed -2 without exhaustion.

An already-zero retry count reaches `0x0075B085`. If Foot `+0x68A` was true,
call original sound entry `0x00750920` with Rules ScoldSound `+0x700`,
pan `0x2000`, volume 1.0 and trailing argument 0; then clear `+0x68A`
unconditionally. The fixture supplies ScoldSound -1, so the original quiet
return executes; audible output is not covered. Next:

1. Snapshot the current live Walk destination as a cell, using truncation
   toward zero. When `in_playfield +0x3D5` is true, obtain the owner's cell
   through `+0x4C(output,NULL)` and call Map zone compatibility `0x0056D100`.
   Arguments include type MovementZone, owner `+0xBC(NULL)` bridge state,
   destination structural bridge bit, and `+0x320` allow-destination-fringe.
   A false answer clears NavCom through `+0x480(NULL,1)`.
2. If an attack target remains, obtain its cell through
   `target+0x4C(output,actor)` even before the next `in_playfield` gate. When
   that flag is true and the target remains nonnull, repeat the zone check
   for the target. This time the owner's `+0x4C` and `+0xBC` both receive the
   actor as requester. A false answer clears the attack target through
   `+0x3C8(NULL)`.
3. The common `0x0075B2BC` tail sets speed 0 and calls Walk Stop. It clears
   the Walk destination and moving flag, without unconditionally clearing
   NavCom or the attack target.

The appended controls execute connected destination and connected/disconnected
Cell attack-target checks. A disconnected target is cleared only for an
already-zero counter with `in_playfield` true; it remains when outside the
playfield or when the counter just decremented from 1. Destination rejection
in this suffix is established by the original branch and setter call; these
controls do not arrange a destination that passes the preceding `+0x2CC`
check and fails this second check. Changing only the path-reference cell from
`(9,8)` to `(-17,31)` leaves the final captured state unchanged. No new RNG
draw occurs in these 21 controls. Movement PathDelay is
armed before FindPath; the failure suffix does not rearm it, but its null
destination setter can reset it to duration 0.

The raw flags have existing shared owners. Original Techno ReceiveRadio writes
`+0x418=1` for message 24 at `0x006F4B72`, and `+0x418=0` for message 25 at
`0x006F4BA6`; this is radio tethering, not contact-list presence. Original
Techno Unlimbo `0x006F6CA0` calls Map playfield predicate `0x00578460` at
`0x006F6CF9` and stores AL in `+0x3D5` at `0x006F6CFE`. These writer identities
come from instruction reading; the response corpus supplies the flag values.
The original `+0x320 -> 0x004DA1D0` is the shared destination-fringe predicate,
not merely an IsTrain check. Zone-event booleans are byte ABI values: discard
the unused upper bits of their captured 32-bit stack arguments.

### Class 2 timers and callbacks

When Foot `+0x6B7` is false, set it true and start blocked timer `+0x668` at now
with Rules `+0x1768` duration. This happens even if the movement timer is still
active. A nonexpired movement timer `+0x640` returns immediately. Otherwise,
FindPath uses destination XY, argument 0, and urgency 2 iff the blocked latch is
set and its timer expired; urgency is 1 otherwise.

After FindPath returns, restart the movement timer using original
`trunc(Rules.PathDelay * 900)` at `0x0075B98C..0x0075B9B1`. The executed controls
pin `.01 -> 9`, `.02 -> 18`, `0 -> 0`, and `-.01 -> -9`, plus exact expiry and
paused-timer boundaries. The middle dword in captured native timers is incidental
stack data; compare logical start/duration, not that unused word.

On success, call owner `+0x4F8`. On failure, first call `+0x2CC` with the current
Walk destination: false clears destination through `+0x480(NULL,1)`; true calls
`+0x4F4`. Native Infantry entries are `+0x4F8 -> 0x00521EB0` and
`+0x4F4 -> 0x00521DD0`. The latter begins with Foot `0x004DC030`: effective
mission 15 clears attack target and destination, other missions do not. The
Infantry-specific suffixes are gated by Type `+0xD94`; this fixture supplies it
false. Those suffixes are not covered or proved inactive for all retail types.

Unit's corresponding table `0x007F5C70` differs: `+0x548 -> 0x004DBA30` is a
plain return, `+0x4F4 -> 0x004DC030`, and `+0x4F8 -> 0x0041C080` returns false.
An Infantry callback must not silently become the shared Unit behavior.

### Class 6 cancellation and scatter

The second-pass cancel arm requires all four conditions:

- Original 3-D distance from owner `+0x48` to Walk destination is strictly less
  than Rules CloseEnough.
- `Radio::In_Radio_Contact @ 0x0065AE30` finds no nonnull contact slot.
- Absolute destination-Z minus physical owner-Z is strictly less than two level
  steps.
- The current physical Cell land at `+0xEC` is not 10.

The cancel arm directly clears head, calls Walk Stop, sets speed 0, calls
`Assign_Destination(NULL,1)`, then invalidates path word 0. The fixture distance
is 202 after native truncation: CloseEnough 202 scatters; 203 cancels. Equality at
the two-level height boundary prevents cancellation.

The final land guard uses the **returned Cell**, including the retained Map
Dummy. Two appended `current_missing=true` rows remove the physical source
Cell `(9,10)` from the map table while retaining candidate Cell `(10,10)`.
Original `0x0075B6C2 -> 0x005657A0` retains that allocated candidate before the
stop-band checks. At `0x0075B7D3`, original `0x00565730` resolves the physical
coordinate to Dummy `0x00ABDC50`, updates its coordinate to `(9,10)` and retains
its supplied land. `0x0075B7D8` then reads Dummy `+0xEC`:

| Input `dummy_land` | Native outcome |
| --- | --- |
| 0 | Cancels, clears NavCom and Walk destination/moving, invalidates path word 0, and resets movement/blockage timers through the original setter. |
| 10 | Scatters the retained allocated candidate's ground list; preserves NavCom, Walk destination/moving, path and timers. |

Both rows use physical `(2496,2624,260)`, destination `(2688,2688,260)`,
CloseEnough 1000 and ProcessMovement(false). Their candidate remains allocated
and does not alias Dummy. The ordinary allocated-current-Cell land-10 contrast
was already present. New-row events capture actual candidate and stop-band
receivers; before/after state records Dummy's coordinate and retained land.
Neither row draws RNG. As elsewhere, class 6 is supplied at the decoder entry;
these rows do not establish original admission on a missing current Cell.

Otherwise, call `Cell::Scatter_Objects @ 0x00481670` with null coordinate,
firstFlag=1, dispatchAll=1. Select the bridge list iff candidate structural bit
`0x100` is set and
`abs(trunc(currentPhysicalZ / LevelStep) - candidateSignedLevel) > 2`.
OnBridge alone does not choose this list. The empty-list controls execute the
original scatter body and spend no RNG draws. Nonempty recipients and their
class-specific scatter bodies require their own evidence.

### Classes 4 and 5 obstacle selection

The original candidate content query receives point `(0,0)` and ground selector
false. A found object that is not allied invokes
`Foot::Override_Mission @ 0x004D8F40` with `(Attack=1, object, NULL)`; an allied
object prevents this override and also prevents the wall fallback. When no
object is found, a nonnegative overlay whose type `+0x2A8` is set invokes the
same override with the Cell as attack target. The native corpus executes the
original content picker, alliance test, mission override, target setter, null
destination setter and Stop. It pins suspended mission/NavCom and the new target.
Deck-only objects are excluded by this ground query.

### Class 1 uncloaking

Cell `0x00483480` walks `Cell+0xE4`, invokes each object's virtual `+0xFC`, then
reads that same object's `+0x30` next link **after** its callback. It does not
snapshot next pointers or filter by alive/category/Techno flags. Infantry, Unit,
Building and Aircraft all use `+0xFC -> 0x00703850`, which calls
`+0x45C -> Techno::StartUncloaking @ 0x007036C0` with argument false. Terrain
uses `+0xFC -> 0x005F4310`, a plain return. These identities were checked against
the original vtable slots and their RTTI type descriptors, not Ghidra labels.

The original transition changes cloak 1 or 2 to 3, sets depth
`CloakingStages-1`, stores the current frame and type CloakingSpeed in its timer,
sets step -1, then invokes `Voc::PlayAt @ 0x007509E0` with Rules CloakSound and
the recipient's exact physical XYZ. Cloak 0 and 3 remain unchanged. The controls
execute this transition, including a dead Unit and a deck-only contrast. A
supplied CloakSound of -1 makes the original sound entry return without audible
output. The active first-pass class-1 control invokes this before repathing and
again if the second class is 1; only the first successful transition emits the
sound call.

## Bounds and outstanding evidence

- No class in this corpus is an executed Infantry CanEnter result. Actual
  bridge/hut admission, physical terrain and path output need separate native
  comparisons and production validation.
- Gate controls reach the original gate method with an empty object list;
  nonempty gate lifecycle is outside this corpus. Scatter lists are also empty.
- The inherited head fixture supplies owner index, unrelated false predicates,
  absent gate lookup and a no-op stopped callback, all recorded in metadata.
- No full object/type/map construction, retail INI loading, arbitrary target
  ordering, nonordinary Type `+0xD94`, tube traversal, paid movement, combat
  continuation or persistence claim is made.
- Bounded inspection of the Walk bodies `0x0075AA90..0x0075D08F`, inherited
  locomotor methods and virtual entries found only constructor and Process
  writes of `+0x35`, no gameplay read. IsMoving reads `+0x34`; IsMovingNow also
  requires positive Foot speed and a nonnull head; IsMovingHere reads `+0x36`.
  This is not a universal proof that unrelated native code never reads `+0x35`.
- Noncenter head selection leaves Scenario RNG unchanged; the center control
  executes original RandomRanged and records the full changed state. No decoder
  refusal itself draws RNG in these supplied cases. There is no direct detach
  call in the result decoder; reachable override/setter/scatter consumers retain
  their own lifecycle contracts.
