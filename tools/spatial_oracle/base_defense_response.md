# Base response native execution corpus

The existing [Foot oracle](foot_navigation_coordinate.py) owns the separate
`--base-response` mode. Its [JSON](base_defense_response.json) and
[metadata](base_defense_response.meta.json) record original code execution,
supplied state, physical input identities and comparison limits. They contain
no VERA-calculated expected scores, reader results, sort order or RNG values.

```sh
export VERA20K_BASE_RESPONSE_ASSETS=/path/to/extracted/retail/inputs
export VERA20K_PROJECTILE_RENDER_ASSETS="$VERA20K_BASE_RESPONSE_ASSETS"
python -m tools.spatial_oracle.foot_navigation_coordinate --base-response --check
```

Configure Python/Unicorn and the original executable as described in
[native_oracle.md](../native_oracle.md). The supplied input directory contains
`RULESMD.INI`, optional `LANGRULE.INI`, `MPBattleMD.ini`, `Hills.map`, fixed
`ARTMD.INI`, and the assets already needed by
[bridge_target_composed.setup](bridge_target_composed.py). The mode reuses
`BulletReader`, its existing physical lexical/INI-cache producer, and that
composed fixture. `--write` deliberately replaces this separate corpus and
its metadata. `--check` does not write.

The default 44-row coordinate payload and metadata remain byte-identical.
The bridge-layer payload also remains byte-identical; only its shared-script
provenance hash changes after reexecution. All three modes can be checked
independently. Native function spans and concrete class vtables are asserted
unchanged after each recorded execution; metadata pins the source owners,
native image, Unicorn versions and canonical JSON payload hash.

## Original producers and fields

Original `Rules665650` and concrete type constructors execute. The Rules
defaults are response 3, ThreatPerOccupant 5, BaseDefenseDelay .25,
SuspendPriority 20 and SuspendDelay 2. Type constructors initialize
ThreatPosed and Speed to 0; the Building constructor initializes Bunker to
false. Original `70EFE0` returns the constructor and retained type Speed
through the concrete type accessor; no speed multiplier is supplied.

The selected physical layers are read in order. Original `673E41..673E61`
uses `[0x7F0CD4] = "AI"` for exact-case
`ComputerBaseDefenseResponse`; `670B7F..670BEC` and `67011B..67013B`
use `[0x7F0C9C] = "General"` for timers, suspension and ThreatPerOccupant.
Same-key General/AI decoys, wrong-case keys/sections and sequential synthetic
map layers execute the original readers. Physical RULESMD sets response 3,
ThreatPerOccupant 10, delay .25, suspension priority 1 and delay 2.

Original type ThreatPosed, Image, Speed and Bunker reads execute only after
original `526810` returns a non-null section pointer. That function returns
an address, so testing only AL would silently skip an address ending in 00.
Selected type fields remain private fixture state; no full Rules Process,
type discovery, physical INI/MIX loader or other type-read phase is claimed.

`71464A..71469F` reads Speed with default -1. Parsed -1 retains the previous
type field. Otherwise it clamps the signed integer to 0..100 **before**
converting by `(value << 8) / 100`, then caps the result at 255. The corpus
records the parsed integer and each clamp/conversion stage. Sequential 7
then -1 remains 17; malformed or supplied empty text becomes 0; missing
text retains; 100/101 become 255; 99 becomes 253. Decimal 2147483648
parses i32::MIN and becomes 0; 4294967295 and `$FFFFFFFF` parse -1 and
retain; 4294967296 parses 0. Canonical raw Speed in 0..100 with native -1
history and the shared conversion can represent the same reachable state.
Storing a converted value in a field whose consumers still convert would
change that state and is outside this fixture.

The empty-text control supplies an existing native cache entry with empty
value. Both the physical lexical producer and current Rust INI loader omit
empty authored values. Original physical INI loading is excluded, so this
control establishes the scalar reader's behavior but does not establish
whether an authored `Speed=` line reaches it in active retail loading.

Physical fields/ranges for the responder types are:

| Type | ThreatPosed | Raw Speed | Native Speed | Primary | Native range |
| --- | ---: | ---: | ---: | --- | ---: |
| E1 | 10 | 4 | 10 | M60 | 1024 |
| MTNK | 15 | 7 | 17 | 105mm | 1280 |
| HTNK | 40 | 6 | 15 | 120mm | 1472 |

Primary pointer binding is an explicit fixture boundary from physical names.
Original Weapon `771C70` constructor and Range `474620` reader establish
range independently. Original weapon/range getters run in the scorer.
Original E1 Image resolves to GI and `523D00` reads physical GISequence,
including Ready `[0,1,1,-1,0,0,0,0,0]`, before the actual infantry setter.

## ThreatPosed and Tank Bunker identity

Original `708B40` first obtains the object's own type. For a Building with
positive garrison count, it calls actual `4581F0` twice and returns the
wrapped signed count times Rules+DF4. Otherwise it checks Building+2E4,
returns that linked object's type ThreatPosed, then falls back to its own
type. Unit+2E4 does not redirect the Unit's ThreatPosed. A null own type
returns 0 before the Building branches.

Building+2E4 is the reciprocal Tank Bunker occupant link. Original active
Building mission `44B780`, at `44B791..44B7A3`, checks
BuildingType+16AB Bunker and calls `458E50`. Its state 5 writes the Building
link at `459301` and Unit reciprocal link at `45930F`. `4593A0` clears the
Unit link at `459450` and Building link at `45945C`; normal release
`4595C0` clears them at `4596E6` and `459814`. These unmodified body/caller
spans and the actual Bunker reader/constructor establish the identity;
the corpus supplies installed links and does not execute installation or
release. Physical NATBNK has Bunker=yes, ThreatPosed=0; its positive garrison
and tank-occupancy mechanisms remain separate.

Controls include own threat 31, linked Unit threat 7/0, positive/zero/negative
garrison counts and null own type. Two occupants with per-occupant i32::MAX
produce -2 through the original multiplication; no enormous passenger
allocation is needed to compare that arithmetic.

## Original scoring, selection and ordering

Each `scoring` row runs full original `4D97A0`. Its actual object distance
`5F6360` uses concrete +48 coordinates, wrapping coordinate differences,
`(dx² + dz²) + dy²`, original `4CAC40` approximation and `7C5F00` conversion.
For the admitted nonbuilding attacker route the Building foundation branch
is absent. Native scores at source `(1280,1280,416)` and attacker
`(3280,1280,0)` are E1=101, MTNK=349 and HTNK=1077, with distance 2042.

Original distance minus range wraps **before** the signed <=0 test.
ThreatPosed<<10 wraps. Speed's first gate is unsigned <=1; otherwise a
second getter whose signed value is <=0 sets travel to 1. Synthetic negative
resident Speed is therefore distinct from Speed0/1, while the native reader
can only produce 0..255. Zero/negative threats, current target, Harvest,
Team/BaseDefense, null attacker, shift/negation/range overflow and exact
post-subtraction boundaries are separately executed and qualified.

`selection` rows supply scores after the earlier caller scoring/admission
gate, entering original `708351` or `7085AD`; they are not claimed full scan
producer executions. Infantry self-anchor multiplies even a negative score
by 100, while Unit negatives debit before its positive x10 anchor path.
Minimum starts at 0. A full list does not first populate it; the first
positive overflow candidate can only establish the old minimum. A later
candidate replaces **every** slot equal to that minimum with the same
pointer. The corpus includes ties, negative budget debits, both class
anchors, multiplication wrap and repeated-pointer replacement.

`sort` executes original `708647..7086AF`: each earlier slot exchanges with
every later strictly larger signed score. Displaced equal scores can reorder.
Scores `[1,1,2]` produce IDs `[3,2,1]`, not `[3,1,2]`; `[4,9,4,7]` produces
IDs `[2,4,3,1]`. This is distinct from a stable bubble sort.

## Actual dispatch and whole-caller receipt

`dispatch` starts at `708647` with declared selected arrays, scores, budget,
timer backing, absent linked effects and retained object state. It runs
actual Scenario `65C6D0` seed and `65C7E0` ranged RNG, original mission queue
`5B35E0`, Unit `6FCDB0` or Infantry `51B1F0`, and a fresh `708B40` after each
setter. Queue, archive, target, passive flag, infantry action/path and timer
writes are recorded in native order. No scoring, dispatch, action or RNG
return is answered by a hook. Existing heap/TLS/archive boundaries stay
explicit in metadata.

Every inlined `65C84B` raw XOR draw, its Scenario+218 indices and the masked
candidate at `65C880` are recorded, including rejected candidates. Complete
native RNG bytes before and after dispatch independently preserve draw count
and stream progression.

For seed31, A/B=MTNK Threat15, C=HTNK Threat40 and original within-range
scores `[15360,15360,40960]`, recipients are C,B,A. Those scores come from
actual `4D97A0` calls stored alongside each dispatch, not a hand-calculated
or Rust replacement. The artificial `[1,1,2]` sort witness is retained in
separate rows.

| Budget | Dispatched recipients | Draws | Missions | Assigned | Cooldown start/duration |
| ---: | --- | --- | --- | ---: | --- |
| 40 | C,B | 83,21 | 11,21 | 55 | 41/225 |
| 55 | C,B,A | 83,21,24 | 11,21,21 | 70 | 41/225 |
| 70 | C,B,A | 83,21,24 | 11,21,21 | 70 | unchanged |

Equality continues; only signed strict overshoot writes cooldown, conditional
on the attacker's then-current Foot flag. Each selected slot consumes a draw,
including duplicate pointers and BaseDefense-team responders. Seed3 yields
65 and mission21; seed15 yields66 and mission11. BaseDefense with draw65
still queues11. Physical .25 delay converts to225 through the actual x87
multiply and ftol; original-read .001 float promoted to double converts to0.
Tail timer+654 copies an explicit stack sentinel and is not initialized to0.

The separate `whole_response` row executes original `708080` from entry to
return using existing `bridge_target_composed.setup`. Four supplied FV Unit
payloads have fresh original Drive constructors, a computer House, empty
Infantry/Team arrays and supplied native zone state. Original admission,
FireError, navigation/layer queries, `56D100`, scorer, selection, sort,
queue/setters and cooldown all execute. The inherited FV fixture does not
read Speed, so this row explicitly retains constructor Speed0 and does not
claim stock FV tuning. It bypasses the damage/ToProtect trigger and uses
flat cells without bridge flags.

Native caller stack+4C retains A5 through the prologue and empty TeamSuspend.
Unit victim coordinate `4DBDF0` writes navigation Y5248 there before sort;
cooldown copies5248. With default response3 and attacker threat10, budget30
dispatches four Threat10 candidates, draws83/21/24/38, and writes timer
`[41,5248,225]`. This establishes the supplied complete caller's auxiliary
provenance, not all class-array mixes or global world initialization.

## Production comparison boundary

### Full retail E1 target setter

The appended `infantry_assignment` section reuses `BaseResponseNative` in a
separate instance. Its 61 rows execute full `51B1F0`, the actual `51D6F0`
idle request and the actual common `6FCDB0` setter. The earlier response
sections remain identical. Actual physical GISequence's 42 nine-integer
records and the original four-byte action flags at `7EAF7C` are saved.
All executable PE sections, class vtables and the action flag table remain
unchanged after each row. No callback return supplies acceptance, target,
Doing, sequence frame, timer or RNG results.

Infantry Type+6AC is **DeployFire**. Actual InfantryType constructor `5236A0`
over A5 backing writes false at `711144`; original `7147E8..714802` passes
that retained byte as default to `5295F0`, reads literal `843AA0` =
`DeployFire`, and stores AL at `7147FC`. Physical RULESMD's E1 has yes;
LANGRULE is absent and mode/map omit E1. Missing, wrong-case, malformed and
synthetic map overrides execute the same reader. The false-DeployFire
setter controls are explicit overrides rather than stock E1 tuning.

Native order for a **changed** target at positive health is:

1. Clear byte+68D firing latch at `51B20E`.
2. Request DoAction28 for current Doing27..30, else2 when prone, else0,
   with force0 and randomStart0. Actual unforced acceptance/refusal occurs
   before any common target setter.
3. Clear a matching supplied reciprocal+2A8 pair at `51B267..51B26D`.
4. Read the resulting Doing. If still27..30 and DeployFire is false, return
   immediately. Earlier action/frame/latch/pair changes remain applied.
5. Write **dword**+5E0=-1 at `51B2A2`, then call `6FCDB0`: it clears
   byte+50C before its same-target early-out or changed-target commit.
6. Ordinary E1 clears byte+68E at `51B33F`. Special CanC4/Engineer navigation
   branches are excluded by the supplied nonbuilding target route.

The supplied+2A8 pair is intentionally unnamed: these receipts establish
its exact cleanup order, not a larger parasite or temporal lifecycle.

For physical E1, Ready0, Prone2, FireUp4 and FireProne8 are interruptible;
Deploy27 and Undeploy31 are not. Deployed28, DeployedFire29 and
DeployedIdle30 are interruptible, although physical GISequence's record30
has zero frames and that current state is explicitly synthetic. Deploy27
refuses the request for28 and keeps27. Current28 refuses unchanged28.
Current29/30 accepts28. Current31 or noninterruptible death11 refuses an
idle request, while FireUp4 accepts Ready or Prone. Death11 with positive
health is also a supplied branch control.

Accepted idle actions reset Doing, native frame+F8 to0, action timer start
to100 and timer/repeat duration from the native action flag's fourth byte:
Ready0 gives0, Prone2 gives6, Deployed28 gives1. The copied timer+104 stack
word is recorded without a gameplay interpretation. Unchanged or refused
actions preserve frame7 and the supplied old timer. For example, changed
Doing29 with DeployFire=false still becomes28 and resets its frame, but
then preserves target, +5E0=123, passive+50C=1 and byte+68E=1.

Assigning the **same** target bypasses firing-latch clearing and idle action;
the ordinary path still clears +5E0, passive+50C and byte+68E. The false
DeployFire gate can block even that same-target path before those clears.
Health0/-1 skips latch/action work but still takes the ordinary target path.
Null/replace/same-null, prone, falling Paradrop and reciprocal-pointer
contrasts are included. All rows preserve the complete Scenario RNG state;
the original idle calls pass randomStart0 and request0/2/28.

These are supplied object/lifecycle states, with object byte74=0, no carry
link, NavCom or air/water remap. Full Infantry construction, locomotor
producers, stage advancement/rendered output and engineer navigation remain
outside this setter comparison. The existing Infantry DoAction owner and
shared concrete target owner should consume the results; a response-specific
setter would duplicate the native function.

Rust regression consumers live in `src/rules/ruleset.rs`, the authoritative
ObjectType reader, `src/sim/combat/base_defense_response.rs` and its focused
tests, plus the shared ThreatPosed and target-assignment owners. Saved native
results establish the bounded original behavior above. Actual Rust test and
production validation results belong to the corresponding mechanism PR;
this native-only artifact does not certify its implementation or the entire
bridge response chain.

The separate [bridge query corpus](foot_bridge_layer.md) covers retained-head
and bridge-dependent admission seams. Full world/map/bridge producers,
populated Team suspension/removal, temporal/animation/spawn detach effects,
infantry engineer navigation, global object expiry, saves and class-array
construction order need their owner evidence. Missing required producers or
consumers keep the whole-bridge goal open.
