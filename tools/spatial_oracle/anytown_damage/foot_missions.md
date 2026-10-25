# Selected Foot missions, ground Infantry firing, stage clocks and frame selection

Run the existing mission owner with the separate mode:

```sh
source ~/Documents/vera20k-dev/env.sh
VERA20K_SHRAPNEL_INPUTS=/path/to/physical/shrapnel/extract \
VERA20K_ANYTOWN_INPUTS=/path/to/physical/anytown/extract \
VERA20K_FOOT_EMISSION_INPUTS=/path/to/physical/foot-emission/extract \
python -m tools.spatial_oracle.anytown_damage.mission --foot-missions --check
```

The input roots follow [the existing mission owner](mission.md) and
[the Anytown attack setup](README.md). They must contain its physical INI, map, TMP and
attack SHP dependencies. The additive emission receipt uses its separate physical
GI/MGUN/PIFFPIFF root and extraction manifest, described below; the historical
sparse roots retain their exact original asset coverage.
`--write` deliberately creates a new reference; it never derives outputs from
Rust. The default mission, counter, interleaving and projected native corpora
retain their bytes.

[foot_missions.py](foot_missions.py) extends `Mission` in
[mission.py](mission.py). It reuses that owner, its physical map/retail readers,
allocator and runtime boundaries. It executes original MissionControl static
construction and layered mission reads, `Rules66D150` for `[ElevationModel]`,
actual E1 type and selected weapon/child reads, original Unit/Infantry/Foot/Techno
constructors and actual Drive/Walk factories. The four actors undergo original
Unlimbo, map marking and Display/Logic registration. The preserved208 rows use explicit `51D6F0` Ready as their fixture baseline.
The additive initialization receipt executes before that explicit call and records
the native constructor/Unlimbo `Doing=-1` and original sequencer transition to Ready. Houses are explicit supplied storage and links with valid Unit
and Infantry counter vectors; full House construction is excluded.

The selected handlers use the original concrete vtables. Unit `744100` and
Infantry `51F640` call Foot `4D6AA0`. Both concrete Rescue virtuals dispatch to
`4DDF90`. Several rows enter original `5B3060`, so their timer epilogues also
execute. The full `.text` and original vtable bytes must remain unchanged.

The JSON schema retains `setup`, physical `inputs`/`world`/`mission_inputs`, constructor
and placement results, and the original82 `rows` with unchanged values. It adds
`rules_reader_receipts`,36 `retail_idle_rows`, `greatest_threat_registration` and
86 `greatest_threat_rows`. Separate additions are `initial_action_receipt`, two
`navigation_rows` and two `empty_rescue_rows`. The additive `weapon_reader_receipts`
and12 `retail_weapon_rows` preserve all208 earlier row values, including their RNG.
Each row has explicit `input`, actor `before`
and `after`, raw `returned_eax`, full three-stream `rng_before`/`rng_after`, ordered
`events`, field `writes`, inherited runtime `callback_events`, native `stop` and
`returned`. `instruction` numbers place events and writes in the same original
instruction order. `physical_getcoords` records actual +48 dispatch results;
`xyz_cell`/`cell_lookup` retain actual Map query arguments. `math` records native
range/rate/distance conversions. `chosen_leash` records the post-flag override.
`score_before_vhp` and `evaluate.score_result` are native integer scores; raw EAX
at an x87 or void return is only register data. Raw middle timer words are copied
stack data, not a simulation timer authority.

The 82 rows cover ordinary MTNK and E1 Rescue state2 cadence; state1 with and without
NavCom; direct Commence to AreaGuard followed by its new current mission rate;
existing-target Approach(false); real archive-point target scans and empty
scans; current Cell ground anchors while the actor stands deck+416; archived
entity ground/deck points; different retained locomotor head/unrelated NavCom;
scan-latch+688 reset; zero versus nearly zero coordinate sentinel; strict
1.5-range admission; AreaGuard default post, passive scan, approach, idle/cadence,
1.1 Cell leash, Foot-anchor GuardModeStray and firing/NavCom leash bypasses.
Scenario seeds0/1/31 are explicit. Raw and ranged RNG calls and final state are
recorded, including the Infantry idle draws that precede AreaGuard cadence.

Full scanner rows run original concrete wrappers, `4D9920`, `6F8DF0`,
`6F7CA0` and `70CD10` against the actually placed hostile MTNK. The separate
`supplied_scanner_return` rows stop before virtual call `4DE056`, record its
original receiver/vtable and three pushed arguments, then supply only a declared
scanner result and stack return protocol. The original caller `4DE05C` executes
its coordinates, distance, strict comparison and concrete setter unchanged.
These rows establish the caller gate, not acquisition. Every no-target/refused Rescue row in the preserved82 stops at `4DE12D`,
before the status1 write, `House500200`, home selection, destination/archive cleanup
and tail RNG. The two additive `empty_rescue_rows` execute that suffix under the
explicit House and crop premises below.

Native `4CAC40` converts the squared distance to a float and indexes table
`8650BC`; it is an approximate square root. Raw axis distance513 can become512,
2817 becomes2816, and2253 becomes2252. The corpus therefore includes several
consecutive geometric inputs across each actual leash boundary. The comparison
uses the native converted distance, not an integer square-root reconstruction.
Native `4D6E74..4D6E89` substitutes retail `[General]GuardModeStray=512` for the
1.1 range when the archived object has Foot flag14 bit2. The original reader is
`670EBD..670ED7`, using the distance parser `474620`.

For the old supplied weapon controls, the Cell leash first issues a destination at
2818 for MTNK and2254 for E1; the Foot-anchor leash first does so at514.
The original target clear precedes the destination setter. Rescue state1 with
no NavCom directly Commences AreaGuard and then reads its new rate: seed31 gives
36, versus15 when Rescue remains current. E1's AreaGuard post also gives36 after
native idle RNG draws; MTNK's post gives39. The original82 rows retain constructor
IdleActionFrequency (`736891ed7c3fb53f`, little-endian double bits): their input
receipt contained the AudioVisual key but the original idle key block had not
executed. Their idle timer values are bounded by that constructor frequency. These values are native row results,
bounded by the explicit seeds and state inputs.

The208 historical rows have a partial E1 weapon read context. Original TypeINI
allocates physical `Primary=M60`, `Secondary=Para`, `ElitePrimary=M60E` and
`EliteSecondary=ParaE`, but only M60 receives WeaponType `772080`. The M60 child
is `InvisibleLow`; its original `46BEE0` attempt returns AL0 because the fixture
selected `[Invisible]` instead. `InvisibleLow` therefore retains actual native
constructor fields: Inviso=false, AA=false, AG=true, image name `InvisibleLow`,
and false SubjectToCliffs/Elevation/Walls. SA receives the full physical Warhead
reader. Para, M60E and ParaE retain range0, damage0, ROF0, Burst1, Speed0 and null
projectile/warhead references. The lexical M60E section was cached but unread.
These old values are supplied reader premises, not full retail weapon behavior.

The new `weapon_reader_receipts` records these before fields and original slot
getters `7177C0/7177E0`. `7012C0` and `707E60` execute through original `70E140`
and retained concrete vtables. Before closure, rookie weapon ranges are1024/0
and threat modes0/1/2 return0/2048/2048. A separate supplied float2.0 veterancy
control selects the existing elite slots: ranges0/0, modes0/0/1792. That control
restores veterancy afterward and consumes no RNG; it does not prove veterancy
production. Names and numeric fields are semantic observations; pointer values
remain native fixture identities.

The receipt's `omitted_sections` names Para, M60E, ParaE, InvisibleLow and SSA.
The new closure runs actual `772080`, each original referenced `46BEE0/75D3A0`,
and `7729F0` across the declared physical layers. Exact section strings, line
numbers, read AL status, native reader calls, field writes and unchanged full RNG
states are saved. Afterward the rookie ranges are1024/1280, elite ranges1024/1536,
and threat modes are0/2560/2560 or0/3072/3072 respectively. InvisibleLow now reads
Inviso=yes, Image=none and all three SubjectTo flags. SA and SSA keep their own
native Verses and ProneDamage bits. The selected pass order is the existing
Anytown weapon/child/postpass convention; this is not a whole Rules Process,
registry initialization or weapon firing proof.

The12 `retail_weapon_rows` run after that closure and the physical idle-frequency
read. Seven original AreaGuard rows pin the2254 contrast and consecutive real
Cell-leash boundaries:2254 now returns36 without a destination,2817 still stays,
and2818 takes the original destination, moving/idle refusal and39 cadence. Three
Rescue rows pin the strict 1.5-range supplied-result gate near3840; those retain
their declared scanner-result seam. Two separate rows execute the actual Rescue
scanner prefix with only the two extra registered candidates explicitly supplied
limbo1/alive0 after prototype restoration. The original MTNK candidate uses the
declared `candidate_live` limbo control. These two rows reuse the existing
`navigation_control`, including its explicit prior path/queue/timer fields;
they stop before House home selection on an actual scanner miss. Actual outcomes,
field changes and RNG are native results;
the earlier2254=39 remains unchanged under its unloaded-Para premise.

Every new row names `input.reader_context=weapon_reader_receipts.after`,
`input.rules_context=handler_rules_after_physical` and
`input.registration_context=greatest_threat_registration.after`. Its
`registration_before` records all five actual registry arrays in native order.
`candidate_liveness_before` records each Techno entry's pointer, limbo/alive bytes,
physical XYZ and House immediately before entry. This avoids inferring liveness
from the label or an earlier outside-row write: the regular leash and supplied-gate
rows restore placed extra candidates limbo0/alive1, while the two real scanner
rows apply the declared inactive controls after that restore. Actual scanner
events retain `result_eax`; the miss prefix is not a full House continuation.

`reader_contexts` separately records AudioVisual chronology. The old82 `rows`
and `initial_action_receipt` use constructor IdleActionFrequency;36 idle,
86 threat, two destination, two home and12 new weapon rows use the later physical
read. `[AudioVisual]IdleActionFrequency` is the relevant key. A partial-reader
comparison must reproduce both the weapon omission and the array's AV context;
it must not change a native golden to make a full retail rules fixture fit.

The zero coordinate is established by original `6F2A50`. An archived point
`(0,0,0)` reaches `70D023` and uses scanner-to-target whole-cell distance;
`(0,0,1)` reaches `70CFBE` and uses archived-point-to-target leptonic distance.
Rescue still independently measures candidate versus archive +48 XYZ for its
strict 1.5-range gate. Default Cell anchors execute `486840/47B3A0` and retain
physical ground height/slope; an entity anchor executes `5F65A0` and retains its
physical XYZ. The retained locomotor head is not the archived point.


The additive reader receipt runs full original `665650` twice with declared
`GuardModeStray+1724` storage values0 and `0x12345678`. It records the same values
after construction and no overlapping native writes. The constructor explicitly
writes IdleActionFrequency at `667574/66757E`; it skips `+1724`. Zero in this
receipt is supplied storage, not a native allocation or constructor-default claim.
The unchanged General key block `670EBD..670EDD` calls `474620` with the current
field as default. Missing/wrong-case keys retain it, `2` reads512, `-1` is a
retain-current sentinel, `-2` reads-512, and a later missing layer retains-512.
These are executed parser controls.

Original `66B3E4..66B40B` reads AudioVisual `IdleActionFrequency` through `5283D0`.
The additive physical layer receipt reads actual `RULESMD.INI` `.15` to
`000000403333c33f`; missing later layers retain those widened-float bits.
GuardModeStray physically reads `2.0` to512. The key-block seam supplies ESI Rules,
EDI CCINI and stack state; it does not execute the whole AudioVisual `6691E0`
reader or native load/allocation sequence. Source bytes, line receipts, defaults,
reader arguments and writes are saved. The VM's observed FPCW is `0x0E7F`
(53-bit precision, chop rounding).

The36 additive idle rows use the physically read frequency. They execute E1
`51CDB0`, native `5216D0` admission, `7099E0` timer admission and actual Walk
`75AB30` through ILocomotion vtable+10. The real moving byte is interface+30
(concrete Walk base+34). Before/after-state records interface, vtable, getter, field pointers and byte
value in `walk_moving`. The separate Foot+3D5 control records that byte without calling it the
Walk moving authority. Ready0, Guard1 and Tread16 admit; Doing-1/4/9/27..30,
actual Walk moving, prone+6DB, firing+68D and an unexpired idle timer refuse in
these controls. Non-null target/NavCom and a supplied animation frame admit at
the receiver; the mission caller has its own gates.

Native `51D6F0` action, `5FB2E0` GameOptions delay and `4C9300` body facing run
unchanged. The rows retain actual action arguments/return, frame+F8, sequence
words+100/+104/+108/+10C, facing raw words+388..+39F, field writes and full three
RNG states. `infantry_idle.rng_at_return` records the state before the AreaGuard
cadence draw. Direct idle rows expose `returned_al`; other EAX bits remain raw.
At seed31, native idle writes258 and selects action9; AreaGuard then returns36.
Seed0 turns through an additional facing draw to raw BAM40960. Seed11 consumes
roll0 and performs no action or turn. Timer auxiliary words are raw stack values.
Stock physical E1 idle9/10 rows take no Main RNG/audio branch; excluded sound
sinks still prevent extending that observation to other sequences or types.

**Idle GameOptions input, executed 2026-09-30.** On the native image, a fresh
existing `FootMissions` retained stored speed0 at GameOptions receiver `A8EB60`
after bootstrap, after
`initialize_companion()` and after the physical `rules_reader_receipts()`.
Replaying `E1_retail_idle_direct_seed1` reproduced the entire saved row exactly,
including all three RNG streams. Original action10 has delay byte3; its call at
`51DA07` executes `5FB2E0(this=A8EB60,3)` and returns5. The original delay3 row
at `832D4C` is `[5,4,4,3,3,2,2,1]`, so stored speed0 gives5 and speed1 gives4.
The resulting sequence timer/rate is `(start1,duration5,rate5)`.

Zero is this oracle's retained supplied storage premise. Neither an Options
constructor/settings read nor a retail configured speed0 is established here.
A Rust comparison must transport speed0 for this idle context, rather than use
its default speed1 or change the native results. The separate fresh emission
owner explicitly supplies speed4; that input does not apply to this historical
idle row. With the configured executable and physical input roots above, these
calls reproduce the proof through the existing owner without writing evidence:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python3 - <<'PY'
import json
from pathlib import Path
from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions
q = FootMissions()
speeds = [q.m.read32(0xA8EB60)]
q.initialize_companion(); speeds.append(q.m.read32(0xA8EB60))
q.rules_reader_receipts(); speeds.append(q.m.read32(0xA8EB60))
row = q.row('E1_retail_idle_direct_seed1', family='E1', mission=11,
            status=0, seed=1, idle_expired=True,
            idle_args=dict(entry='idle', doing=0))
saved = json.loads(Path('tools/spatial_oracle/anytown_damage/foot_missions.json').read_bytes())
expected = next(r for r in saved['retail_idle_rows'] if r['input']['name'] == row['input']['name'])
calls = [e for e in row['events'] if e['kind'] == 'game_options_delay']
print(speeds, row['after']['sequence_timer_words'], calls, row == expected)
assert speeds == [0, 0, 0] and row == expected
PY
```

Two rows enter original `520AE0` to follow Idle1/Idle2 completion. They first
execute actual `51D6F0` action9/10 from the native Ready baseline and record
`completion_setup`: actual action return, before/after state, full RNG, real
GI sequence pointer/bytes and frame count. They then supply stage+F8 at that
count and run the full original sequencer through `520D1B` and its forced
DoAction(Ready0,1,0). Walk moving and prone are false. The original native
ordering, resulting Doing/frame/sequence timer/facing and RNG are captured.
These are sequence-end continuations; the absolute-stage clock and the producer
that advances stage to the count remain excluded.

The86 additive threat rows call original `4D9920` directly with literal masks
0/1/2 and separately call the original concrete `743190/51E140` wrappers with
those inputs. Concrete wrappers add their original weapon category bits, giving
Foot masks184/185/186 in these fixtures. With scan latch+688 set, Foot rewrites
only AL by clearing bit1 and setting bit0, so186 becomes185. The actual scanner
runs; no target or score result is supplied. A non-null result retains the latch;
a null result clears it. Literal Foot masks0/1/2 lack the concrete target category
bits here and produce misses even with a declared live MTNK. The requested,
Foot-entry and Techno-entry masks are recorded separately.

Archive/supplied points differ from the scanner's physical position. `(0,0,0)`
and `(0,0,1)` exercise the native zero-coordinate distance branches; latch-set
local scan controls can reject both distant points before any global-array visit.
The separate global scan rows record the score difference rather than claiming
that both scans have the same admission path. Ground/deck supplied archive points
retain their actual Z; no navigation-head or bridge-layer result replaces them.

The registration receipt observes native Techno, Unit and Infantry arrays after
actual constructors and Logic/ground Display arrays after actual Unlimbo.
Constructors append to the Techno registry. Logic appends for the supplied sort0
contract; ground Display has its own sorted order. Global scanner visits
`6F9C86` follow the actual Techno array. The two actual enemy MTNK candidates are
indices3 and5 in this fixture. Their supplied midpoint `(22656,13056,416)` yields
equal scores98570 for the MTNK scanner and98560 for E1. Native comparison at
`6F9D87` and `JLE` at `6F9D89` retain the earlier candidate; the second produces
no `global_best_update` event. This bounds the tie result to these actual scores
and registry inputs, not an invented whole-match construction order.

The extra enemy E1 exposes an old crop fixture boundary: empty cells had supplied
owner fields0, which native Infantry CanEnter treats as owned by House0. After
the preserved82 rows, the additive fixture executes original Cell `47BBF0` on a
donor and supplies only its native empty+54/+58 values to cells whose ground and
upper object heads are both null. This enables original enemy E1 Unlimbo; its
attempts, result, actual pose and registration are recorded. No full map/Cell
construction or arbitrary occupancy relocation is claimed. Extra candidate live
versus limbo bytes are explicit supplied controls after native placement.

The additive `initial_action_receipt` captures real E1 constructor `517A50` and
Walk `75AA90`, then successful `51DFF0` Unlimbo before this companion's explicit
Ready. Both retain `Doing=-1`; the actual Walk destination/head and moving byte
are zero. Full original `520AE0` executes with that initialized stationary actor
and native speed fraction0. It calls actual COM `75AB30` with the retained Walk
interface, obtains AL0, then requests `51D6F0(Ready0,force1,aux0)`. The sole Doing
write is native `51D9D2`; Doing becomes0, with all three full RNG streams unchanged.
Raw animation stage, sequence timer and body facing are recorded. Probe writes and
CPU/x87 are restored before the existing explicit Ready so every earlier row keeps
its exact value. This bounds initialization admission to the stated stationary
receiver; first whole InfantryAI, movement admission, absolute-stage clock and
whole ScenarioLoad remain excluded.

Two additive `navigation_rows` execute original AreaGuard's Foot-anchor stray
branch at supplied axis distance515. They retain the archived Foot reference as
NavCom and call actual Unit `741970` or Infantry `51AA40`, Foot `4D94B0` and Drive
`4AFD40` or Walk `75ACB0`. Both publish the archived Foot's native +4C XYZ to the
locomotor and original ILocomotion +10 reports moving. The original ctor head
remains NullCoord. Only the first path DWORD becomes-1; all23 suffix words and
reference cell remain. Unit clears NavQueue2 to0; Infantry retains2. Aux clears,
blocked becomes0, retry7 remains, accepted movement timer(start,duration) is(1,0)
and blocked timer is(1,60), using physical RepathDelay60. The copied timer middle
words remain raw stack data. New rows supply the nonempty path, externally owned
queue backing, reference/Aux/timers/blocked prestate; they do not validate those
producers. Setter entry/return full RNG states identify its effect independently
from the later AreaGuard(1,5) cadence draw and return39.

`destination_input_readback` retains the target Foot's physical XYZ, OnBridge,
head/destination and actual IsMoving, declared frame and physical RepathDelay.
Original `navigation_coordinate` events capture actual +4C output and requester
arguments. Native `xyz_cell`/`cell_lookup` events retain the returned Cell pointer,
packed XY, level/slope/flags/land, including the shared Dummy when selected.
Before/after navigation snapshots capture the real locomotor interface/vtable,
MoveTo/getter addresses, actual IsMoving, destination/head, NavCom/Aux, all24 path
words/reference, NavQueue header/backing/entries, timers, retry, blocked, animation
stage/sequence and facing. The original getter is executed only for readback; its
writes and CPU are restored. No geometry or moving decision is reconstructed.

Two additive `empty_rescue_rows` execute full original `4DDF90` with native empty
scanner results. They supply existing House storage fields primary cell(87,50) at
+5490, alternate(0,0) at+5494 and radius1024 at+5498, following the physical retained
field contract in [house_base_projection.py](../house_base_projection.py).
The original `4DE139` status1 write precedes unchanged House `500200`; `501AC0`
variant0, `50DEF0`, direction `49F420`, native map/zone `56D230` and FNPC `56DC20`
execute. For seed31, native ranged(0,1024) consumes2026076499 and returns339;
direction consumes2287577493; the selected Cell is(86,51). The concrete setter
publishes that Cell NavCom, then `70C610` clears ArchiveTarget. The final Rescue
rate draw(0,2) consumes2225548056 and returns0, yielding cadence14. Full three RNG
states and ordered query/output records retain that chronology. Final Dummy
coordinates/fields and every native Dummy write are recorded; no write occurs in
these selected in-crop home queries. House construction/base/radius producers and
whole navigation initialization remain excluded. FNPC uses the inherited supplied
crop zone planes; no home, FNPC or other gameplay result is supplied.

The four downstream rows run only after all204 historical rows have been captured.
The actually registered additional enemy E1/MTNK tie candidates and original enemy
candidate are explicitly supplied inactive with limbo+81=1 and alive+90=0 for
empty scans, after each row restores their original placement baselines. An
earlier diagnostic set limbo before calling the row runner; prototype restoration
reset the candidate to limbo0/alive1 before native evaluation, explaining its
selection. Original `6F7D90/6F7D96/6F7D98` rejects a nonzero candidate limbo byte;
the preserved Greatest_Threat miss controls agree. Both bytes remain declared and
recorded premises for these four downstream rows. This preserves original
registration history while supplying the no-target state. These rows do not
execute candidate UnInit, future locomotor Process, occupancy relocation or
bridge producers.

The220 handler/idle/threat/navigation rows start from declared prior mission state after original construction and
placement. Pose, bridge flags, OnBridge, retained heads, timer expiry and NullPoint
controls do not prove their producers, relocation/occupancy or native scenario
loading. Declaring the placed candidate limbo supplies an empty-scan state; it
does not execute UnInit. The existing cropped map, source-order INI cache,
successful heap and imported OS boundaries remain. Sound/radar/visual sinks and
the inherited hierarchy callback are declared in metadata and observed per row.
Those220 rows do not run a complete UnitAI/FootAI tick, other match objects, bridge
placement/destruction/repair, later locomotor movement, rendering, audio Main RNG,
whole-world scheduling or persistence. Native replay alone is not a Rust or
production parity claim.

The additive `ground_firing_receipt` creates a fresh `FootMissions` VM after
all220 frozen rows and earlier receipts are collected. `preserved_payload` pins
their canonical projection,7,335,235 bytes with SHA256
`a7f520feedbcc1adbc479951356a0bd6049d3626bb6003952b48fc51b013c995`.
The old82/86/36/2/2/12 rows and every earlier receipt/RNG value remain unchanged.
Its setup repeats the original E1/MTNK constructors, Drive/Walk factories,
Unlimbo and registration, the existing Rules key-block receipts, and the full
physical M60/Para/M60E/ParaE, Projectile and Warhead closure. Its nested
`reader_context` retains those executed layers/fields/RNG. The existing explicit
Ready fixture baseline follows Unlimbo; whole first-AI initialization from
Doing-1 is not claimed by this new receipt.

Eight `cases` expose `input`, actual ordered `registration_before`, all registered
actor `candidate_liveness_before`, `actor_before`, `target_before`/`target_after`,
cell/pose readback, `command`, optional `expiry`, `frames`, `final` and full
three-stream RNG. Original Infantry AssignTarget `51B1F0` admits the placed enemy
MTNK and `5B35E0(Attack1,0)` queues the mission. The original InfantryAI
`51BAB0` handles promotion. Every frame has explicit absolute `native_frame`,
before/after raw state, ordered `events`/`field_writes`, runtime `callback_events`,
full `rng_before`/`rng_after`, `stop` and `returned`. Stage+F8 is a signed DWORD,
stage-changed+FC and firing+68D are bytes. `stage_timer_words` retains
`+100/+104/+108/+10C/+110`; rearm, dispatch, targeting and idle timers and full
facing/actual Walk state are also read. Copied auxiliary words remain raw stack
data, not semantic timer inputs.

The four accepted cases use native ground placement, repeated absolute frame,
an elapsed gap, and an explicitly supplied deck pose. Original `51BAB0` calls
Foot `4DA530`, Techno `6F9E50`, Mission `5B3060`, then the stage block
`6FABC4..6FAC31`, before Infantry firing `5206B0` at `51BF59`, sequencer
`520AE0` at `51BF6A`, and movement actions `520F40` at `51BF7B`.
SelectWeapon `5218E0`, GetFireError `51C8B0/6FC0B0`, actual Walk `55AD00`,
DoAction `51D6F0` and actual sequence data execute. Frame1 starts FireUp4 at
stage0/firing1. Frame2 advances to stage1. At frame3 stage2 reaches actual fire
wrapper `51DF60`. Repeating frame1 keeps stage0; jumping from1 to20 advances
only once to stage1 and writes20 as the new timer start, then21 reaches stage2.
Mission cadence/RNG when due remain native observations.

Accepted fire stops before executing the real `6FDD50` entry. Original
`51DF60` has already pushed the actual target/weapon0, cleared firing+68D at
`51DF70` and called the base at `51DF77`. The final frame's `base_fire_entry`
records that receiver, actual return PC/arguments, stage2 and firing0, with
`unexecuted=true`. No return value or stack return is supplied; that final AI,
FireAtTarget and sequencer tail have not returned. Across the six AI cases there
are16 visits:12 original AI returns and4 pre-launch entry stops. This is prefix
evidence; it does not establish a complete shot, base launch, MGUN/PIFF/Report
asset/sound closure, projectile, rearm after firing, impact or bridge damage.

Two bridge controls execute original `51C8B0` admission only. Supplied
opposite OnBridge bytes, deck XYZ+416 and real Cell HasBridge+140 bit0x100
produce native ILLEGAL5 through `6FCBE6..6FCC5A`. The supplied deck accepted
case uses the same explicit fields on both actors. Original map queries return
the retained Cell storage; no firing/map result is substituted. Cell object heads
retain their original ground/upper lists, which are saved. These are layer-state
consumer controls, not native high-bridge placement, occupancy relocation,
locomotor head production or geometry initialization. Continuing opposite-layer
Attack through AI reaches Foot pursuit `4D5690` and path estimate `42D170`;
its `42A5B0` storage is uninitialized in this crop. The existing
[navigation owner](navigation.py) supplies original `42C1C0` and hierarchy
setup. This receipt excludes that continuation and supplies no path answer or
timer to bypass it.

Two controls invoke complete original concrete PointerExpired `51AA10`, read
from Infantry vtable+28, with the live target pointer and supplied control1.
One runs before the first AI visit; the other runs after actual frame1 started
FireUp4/firing1. Native Foot `4D9960` and Techno `7077C0`, an original Scenario
RNG draw, Infantry AssignTarget `51B1F0`, DoAction and Techno target assignment
execute in recorded order. The pending action becomes Ready0 and firing0 before
the target reference clears. The next original InfantryAI returns without
launch. This is the supplied expired-pointer notification receiver, not full
target death/UnInit/broadcast; `target_after` still records the live object.
The existing [expiry owner](../foot_attack_move.py) remains the shared class-chain
reference. All eight controls restore native writes, CPU/x87, observer state and
all RNG before the next case. Original `.text`, class tables and actual Drive/Walk
tables must remain byte-identical. Whole Logic order, other actors and the
inherited runtime/presentation boundaries remain excluded.

The additive `stage_clock_receipt` appends only after the entire prior payload,
including `ground_firing_receipt`, retains its canonical9,101,088-byte identity:
`0fc230b21725dd9c13a84c548c76423f173b481057239d5e2b7899a1ad9f0dba`.
It reuses the restored original-constructor VM from that firing receipt. The
220 rows, eight firing controls, earlier receipts and every earlier RNG value
remain unchanged.

Its36 `clock_rows` run eighteen declared input controls for each of two original
instruction regions. Techno enters `6FABC4` with the original constructed E1
receiver in ESI and explicit EBP0/ESP, then stops before `6FAC31`. Building
enters `4509DE` on supplied zero-initialized scalar storage, executes original
timer helper `426630`, and stops before `450A38`. The admitted Building branch
jumps directly to `450A38`; `450A33` belongs to the refused branch and is not a
common boundary. No Building constructor, vtable or downstream virtual receiver
is executed by this scalar seam. Exact image/text/span identities and instruction
bytes are recorded in metadata and per-frame traces.

Each row declares `input` stage value, changed byte, timer start/raw auxiliary/
duration, repeat rate, signed increment, absolute frame sequence, receiver,
registers and caller stack premises. `frames` retain the original writes between
successive calls, recording native before/after signed state, registers, ordered
`instructions`, field writes, Building helper return, native stop and full three
RNG states. The caller auxiliary is supplied at ESP+2C for Techno or ESP+18 for
Building; the Building caller changed-local byte at ESP+E starts with canaryA5
and records the original write. The middle timer word and stack-local byte are
raw observations. They are not timer authorities.

The controls include repeated absolute frames and an elapsed gap; a timer before
and at expiry; zero and negative rates; paused timers with zero, positive and
negative durations; zero and negative increments; signed stage addition wrap;
elapsed-delta and absolute-frame wrap; and a frame preceding its supplied start.
No Python stage/timer decision or expected gameplay value supplies the results.
All native writes, CPU/x87, inherited observers and RNG are restored after each
control; original `.text` and class vtable bytes remain unchanged.

The58 native frame visits agree across the two regions. A repeated frame with a
positive running duration leaves the value unchanged and clears changed; an
elapsed gap advances once and restarts at the supplied current frame. A zero
increment still sets changed1 and restarts the timer when admitted. Zero rate
refuses advancement. A paused zero-duration timer can advance when its rate is
nonzero, while paused positive or negative duration refuses. Negative rate can
advance again on the same frame after restarting with negative duration. Signed
addition and frame-delta wrap are retained as original instruction results.
All three RNG streams remain unchanged throughout these scalar controls.

The separate two `action_restart_rows` run original physical GI DoAction
`51D6F0(4,0,0)` to produce FireUp4, then supply prior stage31, changedFC1,
timer99/aux2468ACE0/duration9, rate9 and increment7 at absolute frame1337.
Original `51D6F0(0,1,0)` exercises the forced Ready transition and its admitted
clock writes. The separate same-FireUp request `51D6F0(4,1,0)` exercises native
unchanged-action refusal at `51D90B/51D913` despite force1. Both record actual
return, Doing, stage/timer fields, ordered writes, physical GI sequence bytes and
full RNG before/after. No restart or refusal outcome is supplied after entry.
The admitted Ready transition returns AL1, resets stage31 to0, sets timer start
1337 and duration/rate0, and preserves changed1/increment7. The same-FireUp
request returns AL0 without stage writes. All three RNG streams remain unchanged
in both controls and their original action setups.

These are scalar clock and full DoAction restart controls. They do not establish
complete Building/Techno AI, first object initialization, whole Logic chronology,
future ground animation rendering or persistence. Their uncommon retained
stage/rate/timer/increment and caller contexts are explicit supplied inputs;
native producers of those unusual states are outside this receipt.

The additive `ground_emission_receipt` starts only after the entire prior
10,895,933-byte canonical payload, including `stage_clock_receipt`, retains SHA256
`35c0e43f412d76cf9cdd1dd65a4cb13801e7e979d672314de39f15574e4e6fb7`.
It does not change any earlier row, receipt or RNG value. Four fresh instances of
the existing `FootMissions` owner execute the declared accepted frame schedules:
`1,2,3`, repeated `1,1,2,3`, gap `1,20,21`, and the supplied deck pose at `1,2,3`.

This addition requires `VERA20K_FOOT_EMISSION_INPUTS` for a separate extracted
asset root. The earlier sparse asset roots remain unchanged. This root contains
`GI.SHP`, `MGUN-N.SHP`, `MGUN-NE.SHP`, `MGUN-E.SHP`, `MGUN-SE.SHP`, `MGUN-S.SHP`,
`MGUN-SW.SHP`, `MGUN-W.SHP`, `MGUN-NW.SHP` and `PIFFPIFF.SHP`. Its parent contains
`extraction_receipt.json`: the recorded tool `label`, `binary_sha256`,
`manifest_sha256`, and ordered `rows` with `name` and original asset extractor
`result.source_archive`/`result.entry_id`. It supplies extraction provenance,
not a native gameplay value. All actual asset bytes are read and hashed again.
The captured root is `/tmp/vera-e1-emission-retail-20260930/extract`; its manifest
was produced by the existing repository `asset extract` command using the
verified preserved build `audio-owner-after-20260928`. The asset executable SHA
is `da347efe61bb1a79009aee2b70589fc54cd20915f2d54dc1b42731865dc1b`, and its
recorded manifest SHA is
`e6242d96051e5e24400b6469c7e1e94db28d6768d4fea52982cd635827bf772e`.
The existing resolver is:

```sh
python -m tools.cargo_run --resolve asset --profile release \
  --from-label audio-owner-after-20260928
```

The actual extraction commands use that verified executable, `extract <name>`,
`--ra2-dir "$RA2_DIR"` and `--out /tmp/vera-e1-emission-retail-20260930`.
The saved native input receipt records all ten entries from
`ra2.mix -> conquer.mix`, their IDs, hashes, byte lengths and raw headers.
Native readers retain GI744 frames, every directional MGUN6 frames, and
PIFFPIFF12 frames. This extends the existing physical reader; no competing
extractor or SHP/ART scalar reader is added.

`ground_emission_receipt.inputs` records the original GI image, all nine actual
AnimType `427D00` results and SHP loads, eight muzzle references and the two
physical SA `PIFFPIFF` impact references. The existing `Sound` cache owner and
original `7510D0`/`7514D0` read and bind `GIAttack`. Native M60 `Report` retains
that fixture-relative index; sample-name lookup records `igiat1a/b/c`. All E1
normal/elite Weapon, Projectile and Warhead readers run through the existing
full closure. Every fresh VM must reproduce the same input receipt. The original
file-buffer and INI-cache premises still exclude native archive/INI loading;
GI sequence/image reading is original, but this is not a rendered-frame proof.

Each `cases[]` row records explicit `input`, `initial`, optional supplied `poses`,
`original_actor_prefix`, `command`, full `ai_visits`, `logic_suffix`, ordered
`events`, `field_writes`, `final`, constructed identities and all three complete
`rng_before`/`rng_after` states. Per-AI and per-suffix snapshots retain actual
source Doing/F8/FC/firing/timers, target health/mission/target, and complete
Logic/Display/Bullet/Anim/deferred memberships. Events read actual registers,
callers and stack arguments, class damage-pointer changes, Bullet state and
Anim constructor/AI state. The existing Anim constructor decoder supplies only
memory readback; its separate stubbed constructor fixture is not called.
Original `.text` and source/locomotor tables remain unchanged. The emitted
Bullet and Anim tables are also compared directly with original retail-image
bytes and checked again after retirement.

Whole original `51BAB0` runs through `51DF60` and actual `6FDD50`, rather than
stopping at the earlier prefix boundary. Original Infantry FLH `523250`,
Bullet factory/create/construct/fire, Firestorm `5880A0`, cliff/wall `4CC100`,
ROF `6FCFA0`, rearm and MGUN constructor/attachment execute and return. Source
FireUp/firing begins on frame1; stage advances to1 on frame2; stage2 executes
launch on frame3 (or frame21 for the gap). Original `51DF60` clears firing before
base FireAt. Original per-source InfantryAI subsequently completes FireUp to
Ready0. No return, sequence, weapon selection or firing result is supplied.

After launch, the original dynamic Logic suffix `55B608..55B61B` runs with EDI
`87F778`, ESP at the inherited stack, and supplied ESI equal to the recorded
initial actor count4. Original Bullet/Anim registrations and list mutations
supply the remaining order. Original `55DE73..55DE87` commits the frame, then
full `725C70` drains deferred destruction. Whole source InfantryAI precedes
subsequent suffix passes. The four preexisting actor AI visits and other global
Logic phases are excluded, so this is a declared per-source and emitted-object
continuation, not whole-match timing or ordinary full `advance_tick` parity.

The three ground controls run full `4666E0` BulletAI through `4690B0`,
`489280`, real Unit `737C90`, Foot `4D7330`, Techno `701900` and Object `5F5390`.
Native incoming SA damage15 becomes3 in the shared damage pointer; MTNK health
changes300 to297. Its original retaliation leaves Attack1 and targetE1.
Original `48A4F0` selects PIFFPIFF and its actual constructor runs. During that
first suffix, the Bullet at index4 removes itself, compacting MGUN-S to4; newly
appended PIFFPIFF occupies5. The original loop increments to5 and visits it,
so MGUN-S first receives AI on the next supplied pass. This order comes from
the executable loop, not a Python event schedule. Fourteen suffix passes retire
both actual effects and return Logic to its original four actors; Bullet,
Anim and deferred arrays become empty through original cleanup/destructors.

The supplied deck control adds416 to source/target XYZ and sets OnBridge and
physical CellHasBridge after original ground Unlimbo. Its ground occupancy is
retained and its upper head is empty. Original launch/impact/effects execute,
but the original layer query applies no target damage: health remains300.
That result establishes the declared layer-consumer continuation. It does not
establish genuine native bridge placement, upper occupancy or damage to a
properly registered deck object. Bridge destruction/collapse and target death
are not triggered. Earlier target-expiry and opposite-layer refusal controls
remain separately bounded by `ground_firing_receipt` and are unchanged.

The actual GIAttack sound request reaches inherited `7509E0` with its name and
position. This remains a recording boundary before playback, audio-device work
and Main audio RNG. Main/MapGen remain unchanged within that boundary, while the
captured Scenario advances from index5 to11 in each full-shot control. Full
states and every original ranged/raw draw are recorded; this is not an audio
RNG parity claim. Supplied Houses/counters, crop, heap/OS, radar/hierarchy and
other inherited runtime boundaries remain explicit. No native text or gameplay
vtable is changed, and no gameplay decision or outcome is substituted.

The separate additive `draw_stage_modulo_receipt` executes only original
`518E08..518E21`. Eight signed Stage controls cross ten count controls, including
the physically read GI FireUp count6, for80 rows. Counts include negative
extrema, -7/-1, zero, one,2/3,65537 and2147483647; stages include signed extrema,
-65537/-1, zero/one,65535 and65536. Caller EAX/EBX/EBP/ESP, ECX/EDX canaries,
stack marker and signed count slot are explicit supplied inputs. The original
MOV reads the count; count<=1 clamps ECX to1; actual CDQ/IDIV produces the saved
signed EAX quotient and EDX remainder. No Python expected value is calculated.
For example, stage-2147483648/count3 yields remainder-2, while
stage65536/count65537 retains remainder65536. All three full RNG streams and
original text/vtable bytes remain unchanged. The seam stops before `518E21`:
remaining facing/frame-index composition, SHP access and drawing are excluded.
It therefore does not certify full renderer or pixel parity.

## Original Infantry frame selection

`infantry_frame_selection_receipt` is appended after the entire earlier payload,
including the80 modulo controls, retains its17,535,517-byte canonical SHA256
`94db9dcd2cb08ecbc23867f5e11f09f5cb1b2d1e3db7f29eebf491dbb59a8a09`.
A fresh existing `FootMissions` VM executes whole original `518D80` through its
`518F88` return; the receipt records its original521-byte body. No
partial-function seam or semantic stand-in is used.

The receipt reads all42 actual GI records produced by the existing physical ART
and original E1 type/sequence readers. It contains336 `physical_rows` covering
every Doing0..41 at eight body facings and Stage7; Ready0 and Guard1 are distinct
controls, even though their physical first-three record fields are both0,1,1.
Forty `facing_rows` cover all32 native lookup indices and eight rounding/edge
inputs. Its136 `scalar_rows` supply only signed Start/Count/Stride fields in the
real selected bank and signed Stage, including count<=1, negative/extreme values,
values aboveu16 and multiplication/addition wrap. They execute the original
clamp/CDQ/IDIV, only-positive-Stride branch and DWORD IMUL/ADD. Outputs are read
from native registers; Python computes no expected frame arithmetic.

Each row has `input`, `before`, `native_observations`, `native_instructions`,
`native_world_writes`, `output`, `callback_events` and all three complete
`rng_before`/`rng_after` states. `input.record_override`, when present, gives
the first three signed record fields; the remaining physical record bytes stay
intact. `selected_action_bank` and `selected_record` observations identify the
actual native Doing, selected type, record address and fields. `signed_division`,
`facing_current_return`, `direction_index`, `direction_lookup`, `stride_product`,
`stride_sum` and `frame_sum` expose actual intermediate results. Final
`output.frame_u32` and `frame_i32` retain the same returned DWORD in both views.
Original `4C9300` installs the explicit body-facing input; actual `4C93D0`
reads the actor's `+388` facing. These controls retain that setter's duration0
state and do not establish in-flight rotation.

Four `default_rows` set Doing-1 and run the real physical-coordinate virtual
and `Map5657A0`. They retain current XYZ `(22272,12544,416)` and concrete Cell
`0x40003A00`, whose original land type is1. Ground1 with either supplied
OnBridge0/1 chooses Ready0 and returns frame3 at BAM8000. Supplied Cell land2
with OnBridge0 chooses Tread16 and returns frame0; land2/OnBridge1 chooses
Ready0 and returns frame3. `map_cell_return` records the actual lookup result
and land field. These are raw draw-input controls: no native water transition,
bridge placement, layer production or occupancy relocation is claimed.

Original observer gate `70EE30` executes and returns AL1 under the retained
supplied House/current-player premise; the selected bank remains real E1.
Actual E1 Type+D94 is0, so JumpJet CLSID/target-facing and alternate observer or
disguise banks are excluded, along with invalid Doing outside-1/0..41.
The row supplies the declared caller registers, return address and stack fill;
the native function returns normally and preserves its callee-saved registers.
Every input/native memory write and CPU/x87 context is restored, the actor/type/
bank/Cell/Walk/global snapshots compare unchanged, and complete RNG, native text
and actual class vtables remain identical. No inherited runtime callback or
gameplay-result substitution is reached. This proves the selected native frame
calculation inputs and outputs, not SHP drawing or renderer/pixel parity.

Reproduce through the same owner and already extracted physical assets:

```sh
source ~/Documents/vera20k-dev/env.sh
VERA20K_SHRAPNEL_INPUTS=/tmp/vera20k-rescue-shrapnel-inputs-20260930 \
VERA20K_ANYTOWN_INPUTS=/tmp/vera20k-rescue-anytown-inputs-20260930 \
VERA20K_FOOT_EMISSION_INPUTS=/tmp/vera-e1-emission-retail-20260930/extract \
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. \
python -m tools.spatial_oracle.anytown_damage.mission --foot-missions --check
```
