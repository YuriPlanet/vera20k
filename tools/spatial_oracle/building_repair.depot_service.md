# Original repair-depot service comparisons

The additive `--depot-service` mode in [building_repair.py](building_repair.py)
executes original `gamemd.exe` bodies for the HTNK/NADEPT service route, with
MTNK/GADEPT and signed/boundary controls. Its outputs are
[building_repair.depot_service.json](building_repair.depot_service.json) and
[building_repair.depot_service.meta.json](building_repair.depot_service.meta.json).
The existing building self-repair payload and sidecar are unchanged.

The executable SHA-256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Unicorn 2.1.4 runs under the existing fixture's ambient x87 word `0x0E7F`
(PC53, chop). Every scene asserts that original executable text and the four
actual Unit/UnitType/Building/BuildingType vtables remain byte-identical.
No Rust values or hand arithmetic generate the expected results.

## Reproduction and preserved ownership

Use the configured native Python environment and retained retail extraction:

```sh
source ~/Documents/vera20k-dev/env.sh
VERA20K_DEPOT_SERVICE_INPUTS=.local-audit/war-miner-attack/inputs/extract \
  python -m tools.spatial_oracle.building_repair --depot-service --check
python -m tools.spatial_oracle.building_repair --check
```

`VERA20K_DEPOT_SERVICE_INPUTS` defaults to `target/asset/depot-service/extract`.
It must contain `RULESMD.INI`, `MPBattleMD.ini` and `XMP03T4.MAP`;
`LANGRULE.INI` is optional. Fixed `ARTMD.INI` comes from that directory when
present, otherwise `ini/ARTMD.INI`. The saved inputs record each physical
file's SHA/size, selected strings/source lines, original reader calls, writes,
constructor defaults and pass boundaries. `--write` deliberately regenerates
only the additive files. An independent `--check` passed after generation.

The mode reuses `refinery_dock`'s scene/Drive fixture and DockingOffset reader,
`Landing`'s allocator/CRC/INI owner, `native_oracle`'s PE/runner/provenance,
the original RNG seeder and the existing full-RNG snapshot helper. Category
controls reuse the existing building-repair cost fixture. No native math,
radio implementation, PE parser or copied native port is introduced.

## Inputs established by executable reads

Original full Rules, UnitType and BuildingType constructors run before the
selected original read/store slices. Rules passes retain their independent
order: RULESMD → optional LANGRULE → MPBattleMD → map; fixed ART is unlayered.
All selected keys use the original native readers, including exact case,
current-value defaults, native `%f` widening and signed values.
BuildingType FindOrAllocate `4653C0` calls the actual constructor `45DD90`
at `46542A`. Its dock-array receipt corrects the earlier `45D5E0` label;
the original constructor execution and produced numeric values are unchanged.

| Input | Original reader | Executed stock result |
|---|---|---|
| `RepairStep` | `670DCA..670DE9` | 8; constructor 5 |
| `RepairPercent` | `670DA3..670DCA` | 15%; constructor .25 |
| `URepairRate` | `670E30..670E57` | .016f widened; constructor binary64 .016 |
| `[Repair] Rate` | `679C92..679CAF → 5B3760` | .08f widened; service delay 71 |
| `Strength`, `Cost` | `5F94D3..5F94F3`, `71469F..7146B9` | HTNK 400/900; MTNK 300/700 |
| `UnitRepair`, `NumberOfDocks` | `460906..46092F`, `46492E..46494B` | both depots yes/1 |
| `HasStupidGuardMode` | `460EA0..460EBA` | both false; constructor true |
| `ManualReload` | `713343..71336C` | both tanks false; constructor false |
| `MovementZone` | `71605E..716090` | HTNK Destroyer=2; MTNK Normal=0 |
| `Foundation`, `DockingOffset0` | `461225..46125D`, existing native dock reader | NADEPT 4x3/(128,0,0); GADEPT 3x3/(0,0,0) |

The original AudioVisual assignment `66B323..66B337` establishes the full-health
threshold 1.0. It is an unconditional assignment, not an INI default.
Four reader controls cover constructor defaults, missing-key retention,
wrong-case keys, and signed/fractional inputs. Stage's independent Building
constructor stores at `43B7F5..43B823` execute before service.

The original startup initializer `45C300` constructs the outside-cell rows.
The ART post-read `461547..461570` stores `Type+ED4` at `46156A`. The scene
transfers the initializer's actual row bytes, then executes that pointer
assignment; it does not synthesize exit coordinates. Native `44EFB0` selects
the first usable exit `(6,12)` on the supplied clear map. Rally controls retain
that search and select the authored rally `(13,13)` for departure.

## Corpus and measured transitions

Schema 1 has 13 Unit getter controls, 3 category getter controls, 18 radio
controls, 16 destination controls, 30 pending-entry lifecycle controls, 7 terminal-arrival controls,
22 repair-fallback controls, 12 Guard controls, 21 Repair controls, 2 readiness prerequisites,
4 reader controls and 7 retained histories. Pointer aliases inherited from
the shared fixture are explicit: `miner` is the selected tank, `refinery` is
the selected repair depot. They are symbolic identities, not object names.

Each executed step retains its entry, measured EAX, events, writes and full
before/after state: actual/estimated health, cash/spending, missions/status,
dispatch timer, independent repair Stage `[value,changed,start,duration,rate,
increment]`, contacts/tethers, NavCom/NavComAux/archive/pending entry, Drive power/goal,
and all 250 Scenario RNG words. Guard/Repair controls enter original
`MissionAI 5B3060`, so the original dispatcher also writes their timers.

* UnitType virtual slots `+AC/+B0/+B4` execute `711EB0/7120D0/747F20`.
  Stock repair costs 2 credits and heals 8. Category controls establish that
  a Unit's base cost excludes Building `45ED50` FreeUnit/PadAircraft deductions.
* Radio `Unit737430 → Foot4D8FB0 → Techno6F4AB0` executes message `1C`.
  Insufficient funds returns 32 without mutation; paid incomplete returns 1;
  newly completed returns 33 and clamps both health values; already full
  returns 10 without payment. A nonnull Foot NavCom returns 10 before Techno.
  Negative repair steps heal 1. The signed-max addition wraps before the ratio
  test: health/estimate 1 plus `i32::MAX` become `i32::MIN`, reply 1.
* Unit destination `741970` reaches the conditional power tail at `742F48`
  for ordinary cell and NULL/force requests. It first calls actual ILoco
  IsPowered `+60 → 55A930`; an already-powered locomotor skips House and cell
  lookup. Otherwise original House `53A130` returns false, raw Object GetCell
  `5F6960` finds the ground list at the actual coordinate, and a UnitRepair or
  Bunker Building restores power through `+58 → 55A8F0` before Foot
  `4D94B0`. Actor-first lists and no radio contact still admit that power call.
  An empty/nonservice ground list or current Cell flag `0x100` preserves power
  off. Requested-depot successful handshakes, same-NavCom and deployed early
  returns skip this tail. Flag 0 reaches it. A NULL without prior navigation
  returns early; NULL with prior navigation or force reaches it. Native EAX
  is retained, while Rust's acceptance Boolean is not treated as its return
  contract.
  The Bunker-only positive control writes `+16AB`. The contrasting
  UnitReload-only control writes `+16AA` and preserves power off. This corrects
  the earlier `+16AB`/UnitReload label without changing its executed byte or
  measured result. The distinct native keys read in UnitRepair → UnitReload →
  Bunker order; UnitReload's constructor stores zero at `45E0C6`.
* Stock Guard state 0 initializes state 1 and returns 27 with the seeded
  Scenario draw. Eligible state 1 contact on Enter within native 3D distance
  `<64`, with NEED_MOVE=Roger, queues Repair and returns 1 without a draw.
  Guard alone leaves the queue pending with ready byte 0. Original DOCK_NOW
  sets ready=1 and queues the occupant Sleep; the original postdispatch
  ReadyCommence slice then promotes queued Repair. ReadyToCommence is `454250`.
* Repair state 1 performs the first paid step, starts the independent Stage
  and returns 71. A first step that completes health still enters state 2.
  State 2 advances Stage and returns 1 per dispatch; stock URepairRate's
  fractional threshold requires 15 increments. It releases on completion or
  an already-full reply. Insufficient funds resets state 2 → state 1, returns
  1, and preserves the occupant; the next state-1 retry returns 71.
  State 1 preserves a nonnull NavComAux when NavCom is null. With nonnull
  NavCom, the original StopMoving dependency clears both navigation fields
  before the paid repair; both alternatives are executed controls.

| Retained history | Executed result |
|---|---|
| `paid_complete` | Heals at 200,285,300,315,330; releases 330; 133 frames |
| `first_step_complete` | Heals to full at 200; releases 285; 89 frames |
| `dock_now_state0_to_service` | State0→1 at 200/delay3, paid203/delay71, releases288; 93 frames |
| `full_release_rally` | Already-full departure to rally at 200; 4 frames |
| `insufficient_then_deposit` | Fails285→state1, retries286, native AddCredits at300, pays357, releases442; 246 frames |
| `no_funds_200_mission_visits` | 14,201 frames/201 service attempts; no payment, heal, ejection or RNG draw |
| `paid_complete_restore_seed0` | Same repair/release timing as `paid_complete`; supplied original Seed(0) boundary at 280; 133 frames |

Every history frame executes original MissionAI. Quiet dispatches omit only
duplicate full snapshots after exact before/after equality and no Guard/service
call. Their frame/EAX pairs remain, with a SHA over every full native dispatch
step. Post-release empty-contact/Guard transitions and their RNG draws remain.

The continuation history invokes original `RandomClass::Seed 65C6D0(0)` before
the frame-280 dispatch, while independent repair progress is 9. This is an
explicit supplied RNG boundary matching the existing Rust snapshot-load policy;
native disk serialization and LoadGame do not execute in this corpus. The Rust
consumer saves and loads the in-progress scene through the existing snapshot
owner, restores its caches through their owners, then compares the remaining
dispatches and the later Guard draw against this native continuation. A separate
Rust check changes only repair progress and checks that it contributes to the
state hash. Those are Rust roundtrip/hash checks, not native serialization claims.

## Pending-entry command lifecycle

The additive `pending_entry_lifecycle` field contains 6 original Unit class
controls, 10 MegaMission prefixes, 12 Stop slices and 2 Deploy class-call slices.
Every previous top-level field and result remains unchanged. Each new row reuses
`DepotService`, retaining complete before/after state, full Scenario RNG, the
existing step observations and an ordered `lifecycle_trace` of original calls,
instruction markers and raw writes. No observer changes native behavior.

These new scenes execute two bounded stores from the original Foot constructor
`4D31E0`: `4D31F1..4D31FB` sets tube index `+520=-1`, and
`4D32EC..4D3308` sets saved MegaMission `+5C4=-1`, saved target/destination
NULL and saved flag0. ESI is the actor and EBX0 follows the original `4D31EF`
initialization. The complete constructor does not run. Each row retains the
initializer register inputs, raw before/after fields and writes; the sidecar
pins the original bytes of both shared slices.
Direct Unit EnterIdle supplies ordinary arguments `(0,1)`. Its saved-order
virtual query `vt+4AC → 4DF1C0` returns AL0 for `+5C4=-1`; a prior Nav then
queues Move2, while no Nav retains the supplied Guard mission and queued-1.

The earlier lifecycle fixture's raw `+5C4=0`, tube index0 and `(0,0)` EnterIdle
call remain separate ignored negative/input-audit evidence. Original raw0
saved order makes the same query return AL1 and exits at `738998` before
Nav→Move. Every pre-lifecycle corpus section retains its inherited raw0
constructor fields and original results. The retained saved-order mechanism
and tube callbacks are outside these ordinary lifecycle comparisons.

| Entry | Executed boundary and result |
|---|---|
| Unit NULL/cell destination | Whole `741970`; prior-Nav, force and no-Nav routes preserve `+500` |
| Unit EnterIdle | Whole `738970`; reached Foot/Techno idle bodies preserve `+500` |
| MegaMission | `4C72E8..4C7385`; radio prefix precedes nonnull `+500` clear at `4C7353` |
| Stop class calls | `4C75DA..4C762A`; BREAK broadcast precedes NULL destination and target; `+500` retained |
| Stop Foot prefix | `4C757D..4C762A`; both original `+5AC` vector clears precede those class calls |
| Stop after actor admission | `4C7504..4C762A`; original admitted suffix, prior/no Nav and two contacts |
| Stop early exits | `4C7504..4C8109`; tether or current mission18/19 skips all these effects |
| Deploy class calls | `4C77F0..4C7818`; NULL destination/target then Queue(Unload16), retaining `+500` |

MegaMission's untethered branch calls actual `vt+274 → 65ACB0` at `4C72F8`,
sending BREAK only to slot0. Its two-contact control preserves slot1. The
tethered branch requires a nonnull slot0 contact, Object alive byte `+90`,
Building RTTI6 and DockUnload `+16B3`; its successful branch also clears the
actor tether at `4C7342`. NULL contact, non-Building contact, dead contact and
DockUnload=false controls retain contacts/tether while still clearing pending
entry. An alive1/health0 control takes the positive branch, distinguishing the
Object lifecycle gate from hit points. NULL pending observes no `+500` store.

Stop calls actual `vt+280 → 65ACE0` at `4C75E0`, broadcasting BREAK to both
supplied contacts before Unit NULL destination at `4C75ED` and Techno NULL
target at `4C75F8`. The earlier vector virtual clear at `4C7597` and Foot
`4DA1C0` at `4C759C` both reach original `4E0190`. Pending entry survives all
Stop, Deploy, destination and idle rows. Every one of the 30 controls preserves
the complete Scenario RNG, including both indices and all 250 words.

Foot `4DA030` processes the retained `+5AC` path queue; it does not clear the
vector. It has no effect here: nonnull Nav returns at `4DA03D`, and NULL Nav
with supplied queue count `+5BC=0` returns at `4DA04B`. Its original call is
retained in the lifecycle trace separately from the two Stop clear helpers.

These Event rows supply admitted interior ESI/EDI/EBX/EBP and a zeroed 128-byte
stack frame. Their exact bounds/registers and raw preconditions are saved.
Team is NULL. MegaMission ends before translation/Queue; original instruction
packets separately establish the clear before conditional TeamRemove call
`4C7380`, translation call `4C73AA` and Queue call `4C73B9`. Those later effects
are not executed by the prefix rows. The constructor zero store `6F310C` is
likewise retained as original instruction evidence; these scenes explicitly
supply their pending pointer.

DockUnload/alive/health variants are prepared scalar controls. The non-Building
row uses the unchanged original Unit vtable on the supplied contact body solely
for the RTTI gate. Full Event actor/token admission, network/input dispatch,
Team internals, later Stop/Deploy/MegaMission work, movement and subsequent
FootAI retry are outside these bounds. The shared construction slice driver
accepts an optional instruction limit: new Event rows declare 2,000,000 while
all existing construction calls retain their 10,000 default. Expiry results
and coverage remain unchanged. Older historical sidecars remain preserved.

## Original terminal arrival and idle handoff

The additive `arrival_terminal` section owns its `native_inputs` receipt and
seven controls. Every earlier top-level value is preserved exactly. Its reader
reuses `DepotInputReader` and the existing whole Anytown type-weapon block
`71284A..712A8F`, plus `DefaultToGuardArea 714F3D..714F5E`, full Rules
`ReadIQ 674240` and the existing layered MissionControl reader. Physical exact
section/key/value source lines remain available to the production Rust reader.

Original constructors establish signed `TurretCount+808=0` at `71136F` and
`DefaultToGuardArea+D39=0` at `71153C`. Layered reads produce HTNK Primary
`120mm`, MTNK `105mm`, TurretCount0, WeaponCount0 and DefaultToGuardArea0.
Full ReadIQ changes GuardAreaIQ from constructor4 to retail2. Original
MissionControl startup `4E7CF0 → 5B3700` initializes all32 entries; layered
Enter7 remains Zombie0/Paralyzed0/NoThreat0. Sleep's separate Zombie value
does not substitute for the currently committed Enter control.

Whole original `IsArmed 701120` executes through actual Unit virtual slots:
`+3F4 → 70E1A0`, `+3F8 → 70E140`, Type `+84 → 717880`, and normal
WeaponStruct selection `7177C0`. Both stock tanks return1. The scene binds
the original reader's allocated Primary identity for this nonnull test.
WeaponType callbacks, their complete data reads and firing do not execute.

The existing `locomotor_track_cursor.OriginalCursor` reads actual TurnTrack0
at `7E7B28` as normal raw1, short0, facing0, flags0. Entry1 of the raw table at `7E7A28`
has23 real points followed by `[0,0,0]` at cursor23. A separate original
sample witness executes descriptor/sample admission with budget8 and reaches
terminal `4B1F97` with budget1. Four complete-terminal rows supply that
selector0/cursor23, valid1 and budget1, with the actor and Drive head at the
native NADEPT pad `(2176,2688,0)`.

They reproduce the original `4B0F20` prologue frame: F0 local bytes, four
saved registers, return address, and caller argument0. Live EBP is the Drive;
ESP points to that declared zeroed frame. Original terminal refund/coordinate
work runs from `4B1F97` through complete downstream class bodies and the
original RET4. Earlier speed, travel, track selection and payment are supplied.
The frame is a bounded terminal caller witness, not a whole Process_Track or
natural path-arrival comparison.

The selected row has reciprocal contacts with **both tether bytes0**, matching
the observed retail admission. The tethered counterpart measures the same
handoff. Native ordered entries and raw writes establish:

1. Reached is computed at `4B21B1`; original Drive destination/head are cleared
   before complete `Unit PerCell 739EC0(2)` from `4B220F`.
2. PerCell sends DOCK_NOW, which queues depot Repair20 and occupant Sleep0,
   sets ready1, then calls actual ILoco PowerOff `55A910` from `73A52E`.
   The observer decodes this stdcall receiver from its first stack argument.
3. After PerCell returns at `4B2215`, the reached tail calls original
   `Foot StopMoving 4DF0D0` from `4B2242`, clearing Aux at `4DF0D2` and Nav
   at `4DF0D8`. The `4B2247` snapshot records that cleanup.
4. Actual Unit `vt+504 → Foot 4DB9B0`, called from `4B228B`, sees NULL
   Nav/target, Enter7 and UnitRepair contact, and calls whole Unit EnterIdle
   `738970(0,0)` from `4DBA1B`. Armed retail inputs select Guard5; Queue
   at `738D21` overwrites queued Sleep at `5B3614`.

The final original return retains **current Enter7, queued Guard5, NavNULL and
power0**. Later current-mission promotion is a separate scheduler effect.
Both idle arguments are recorded: second flag0 bypasses retained tube resume,
while original constructor TubeIndex `+520=-1` makes that work inert. It does
not suppress the Unit mission tail or preserve queued Sleep.

| Control | Executed result |
|---|---|
| `stock_enter_terminal_untethered` | Complete terminal; Sleep0 → Guard5, both tethers0, power0 |
| `stock_enter_terminal_tethered` | Same terminal handoff, both tethers1 |
| `terminal_nonservice` | Prepared UnitRepair/UnitReload/Bunker false; vt504 uses `(0,1)`, queues Guard5 |
| `terminal_target_nonnull` | Prepared target `other`; vt504 skips idle, retains queued Sleep0 |
| `post_stop_no_contact` | Direct original vt504 over prepared post-Stop state; `(0,1)`, Guard5 |
| `post_stop_idle_args00` | Direct whole UnitIdle `(0,0)`, Guard5 |
| `post_stop_idle_args01` | Direct whole UnitIdle `(0,1)`, Guard5 |

All seven rows retain original constructor-slice receipts for TubeIndex=-1,
saved MegaMission=-1, saved pointers NULL and saved flag0 through the single
`depot_initialize_foot_inputs` owner shared with lifecycle controls. They retain
full before/interior/after state and all250 Scenario RNG words, which remain
unchanged. Original terminal refund returns the supplied budget1 to8.
`ordered_trace` records native entries/returns and writes;
`interior_snapshots` records successful DockNow, after PerCell, after StopMoving,
after the navigation gate and the return. Supplied target/idle flags, type
fields, occupation bytes, track selector/cursor/head, HouseIQ0 and the final
native frame budget/reached/restored registers are explicit. Each whole call
has a 2,000,000 instruction cap.

The no-contact row intentionally executes only the post-Stop navigation gate:
broken-contact whole PerCell reaches additional path/zone work unavailable in
this sparse supplied scene. The failed exploratory call remains ignored audit
evidence; no expected output is invented for it. Retained saved-order/tube work,
waiter admission, earlier real-track occupation and full-frame scheduling are
outside this section. Existing depot histories, legacy building self-repair,
legacy420 slot rows and current expiry results are preserved; the replay packages
are in git at 1b1a8b43e.
Native `--depot-service --write` followed by independent `--check` passed.
Legacy building-repair, legacy slot-replacement and current expiry checks also
passed. Removing only `arrival_terminal` reproduces the complete previous depot
JSON bytes, SHA-256 `773218f6a5333a6d7d0625650800c94c148b3043f273703b8381639c7d72d6e5`.
Before the repair-fallback addition, the arrival-bearing payload SHA-256 is
`fc9d418aaa0baef5bf89e3565d8b6cedc6a7cb5a850284494903b26ed24cabb3`.
The expiry payload remains byte-identical, SHA-256
`21f851dbcb3f964921ba1c06cec3cbfe131609890c1429deed8a2d3a48cb00f7`.

## Repair fallback power and readiness

The additive `repair_fallback` field has22 rows over the existing scene and
original `MissionAI 5B3060 → MissionRepairAndProduce 44B780`. It retains every
earlier field and result. Eighteen linked controls supply state0/1, coordinate
offsets99/100/199/200, stopped or moving/Nav state and power0/1. Two of those
controls isolate state0 NEED_MOVE rejection at offset99. Four no-contact
controls supply state0/1 and ready0/1. The new optional `ready` input writes
Building `+6DD` only when declared; old scenes retain their prior initialization.

`fallback_trace` observes actual distance getter `vt+4D8 → 447E00`, radio
NEED_MOVE `13`, House `53A130`, PowerOn/Off/IsPowered/IsMoving and raw
power/readiness/navigation/queue/status/timer writes. ILoco methods use the
first stack argument as their receiver. State0 separately obtains IPersist
through original `45AEA0`: GetClassID `4B4830` and Release `4B4CC0` use its
base-interface vtable, distinct from ILoco at Drive+4. No reply, distance,
power or readiness function is substituted. The sidecar pins relevant original
instruction ranges and class-comparison constants.

Original stock Drive GetClassID returns bytes
`4127584a3998d111b70900a024ddafd1`; the state0 comparison retains the default
threshold100. At measured distance99, NEED_MOVE=Roger1 enters state1 and the
original dispatcher sets delay3. At measured distance100 and beyond, fallback
`44C7E5..44C808` calls ILoco `+58 → PowerOn55A8F0`, including an already
powered contact, and returns the stock mission delay71. Supplied offset199
measures **198** through original `447E00`; that executed result is retained.
Offset200 measures200. They are coordinate inputs and measured distances,
not interchangeable labels.

The two state0 Nav+moving rows at offset99 return NEED_MOVE=Negatory10, skip
the distance call and take the same PowerOn fallback. This establishes the
radio-rejection side of the admission independently of the distance threshold.

State1 compares distance against200 first. Below200, an unpowered contact with
nonnull Nav reaches original StopMoving, clears Aux/Nav, gets NEED_MOVE=Roger,
performs one paid repair and enters state2/delay71. A powered moving contact
instead returns delay1 before NEED_MOVE. At distance200, that prior cleanup is
skipped: NEED_MOVE returns10 with Nav intact. Fallback `44C5AB..44C61B` calls
IsPowered `55A930`, then powers on an unpowered contact; an already-powered
contact skips PowerOn. Both remain state1 with delay71.

No-contact queue/promotion differences are also executed:

| Supplied state/ready | Original effect | Final mission/queue/ready |
|---|---|---|
| state0, ready0 | `44C623..44C6EA` Queue(Guard5,1), no ready store | Repair20 / Guard5 / 0 |
| state0, ready1 | Same Queue, preserves ready1 and promotes | Guard5 / -1 / 1 |
| state1, ready0 | `44C18E` stores1 before Queue(Guard5,1) | Guard5 / -1 / 1 |
| state1, ready1 | Stores1 before the same Queue | Guard5 / -1 / 1 |

All four no-contact rows return delay1; the original queue/commence owners
perform the immediate promotion where admitted. Every row retains full
before/interior/after state, actual EAX/AL returns, full Scenario RNG and the
original dispatch timer writes. All22 preserve both RNG indices and all250
words, and original executable text/four class vtables remain unchanged.
These supplied scenes retain the existing raw Foot constructor fields and
empty animation-slot objects. Earlier contact/travel producers, live animation
construction and subsequent object visits remain outside the comparison.
The additive regeneration and independent depot check passed; legacy repair,
legacy420 slot and current expiry checks also passed. Removing only
`repair_fallback` reconstructs the prior arrival-bearing payload byte-for-byte,
SHA-256 `fc9d418aaa0baef5bf89e3565d8b6cedc6a7cb5a850284494903b26ed24cabb3`.
The expanded payload SHA-256 is
`76614921afb7ac9e87c94c7644588a69aa03b4f1069de911982ed08bbfd19172`.
Expiry results remain byte-identical. Legacy payloads and sidecars remain
byte-identical.

## Coverage boundaries and Rust consumers

Scene loading, object/House membership, contact arrays and physical arrival
are supplied. The 32x32 cell grid has supplied playable width/height16 and a
clear exit. Native map/enter-cell search executes over those cells; complicated
occupancy, path traversal and blocked-exit alternatives are not established.

Stock NADEPT GetCoords is `(2048,2688,0)`, while GetDockCoords is
`(2176,2688,0)`. A tank supplied at the latter point fails Guard's center
distance `<64`; a supplied center admits. The corpus proves both decisions,
and does not prove a naturally occurring Drive/Enter arrival satisfying both
Guard admission and Repair state-0 docking distance. The additive arrival
section establishes the complete original terminal handoff over a supplied
pad/frame; the production runtime must establish earlier path reachability
and the scheduler's later mission promotion separately.

Wallet storage is empty, with no silos, parasite or live smoke object. Signed
receiver controls are prepared arithmetic states, not healthy in-play stock
units. IDIV zero divisors and signed division overflow are excluded from the
callable cost controls. Presentation animation/EVA calls are observed sinks;
the inherited scene supplies field recalculation and zone-reachability services.
The Landing reader retains its allocation/delete/TLS/Interlocked services and
supplied cached INI indexes. Register/stack prerequisites of each slice are
explicit in the harness, with stack reset between partially pushed reader tails.

The mission histories exclude the rest of Building/Techno AI: the common C4
visit counter, passive work, animation producers and the general Techno Stage.
They retain the separate Building `+620` repair Stage. Arithmetic, money, radio,
mission timers, idle/Guard, queue/destination/archive, exit search and contact
removal execute unchanged within the represented calls.

The destination rows explicitly supply the raw current-cell ground list,
Cell flags, UnitRepair/Bunker/UnitReload type flags and deployment/force bytes. These
are controls over the setter dependency; their placement, deployment and bridge
producers are not executed. Their actual ILoco vtable and function identities
are saved. The destination observer reads COM receivers from the stack and
records `742F48` as an interior label, without inventing a call boundary. No
destination, power, House or raw-cell function is substituted, and these rows
do not advance movement.

[building_repair_service_oracle_tests.rs](../../src/sim/docking/building_repair_service_oracle_tests.rs)
consumes these native goldens through production readers, the Unit radio
receiver and the canonical Building `MissionAI` dispatcher for Guard/Repair
controls, the22 fallback rows and every history frame. Both sides exclude the Building AI
`UpdateAnimation` prefix. It compares wallet, estimates, Stage/timers, contact/navigation cleanup
and full RNG. Destination controls call the existing shared
`set_unit_destination` and `set_unit_null_destination` owners; they compare
the same retained state plus force/deployment bytes. The pending-entry consumer
compares 28 rows through canonical command/retask and class owners; the two
post-admission Deploy slices remain native-only because stock HTNK cannot enter
that production command.
[track_host_tests.rs](../../src/sim/movement/track_host_tests.rs) consumes the
additive arrival section through the existing canonical host, PerCell,
navigation/idle gates and production INI/Primary/IsArmed owners. Its supplied
terminal frame excludes earlier path producers, matching this corpus boundary.
Executable native
comparison has passed; Rust test results and
ordinary runtime validation are reported by the chain owner after Cargo and
the retail-map run. This corpus does not certify an entire match or rendering.
