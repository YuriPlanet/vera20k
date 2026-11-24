# Unit simple deployment and shared Foot body counter

Original executable: `gamemd.exe`.
The current executable comparison is
[`unit_simple_deploy.py`](unit_simple_deploy.py), with results in
[`unit_simple_deploy.json`](unit_simple_deploy.json) and explicit fixture
boundaries in [`unit_simple_deploy.meta.json`](unit_simple_deploy.meta.json).
It extends the existing Unit/map, AnimType reader, Anim constructor and Jumpjet
fixtures; it is not a full native game or rasterization comparison.

```sh
VERA20K_SIMPLE_DEPLOY_ASSETS=/path/to/extract \
  python -m tools.spatial_oracle.unit_simple_deploy --check
```

The extract directory must contain unchanged `artmd.ini`, `schpdepl.shp`,
`schp.hva`, `schd.hva` and `spyp.hva`. The normal production `asset extract` tool supplies
these files. Output pins their identities; no Rust values generate goldens.

## State and trigger

Unit constructor `7353C0` clears `+6E0/+6E1/+6E2` at `735422/735428/73542E`.
They mean deployed, forward transition and reverse transition. Techno constructor
clears animation pointer `+130` at `6F2BB8` and landing request byte `+134` at
`6F2BBE`. These are independent of Foot `+6AD`, the forced locomotor swap flag,
and MCV Unit `+68C`.

Event DEPLOY (`EventClass::Execute 4C6CB0`, event 9, case entry `4C76BC`)
resolves the Techno through `6E6F20` and calls ClearPlanningTokens `6386E0`
before admission. It then requires a nonnull object, alive byte `+90` set,
limbo byte `+81` clear, tether byte `+418` clear, and EMP dword `+504` zero
(`4C76CF..4C7709`). There is no Health test here or in the target resolver's
RTTI filter `40DD70`. Rust's existing `dock_entered_with` represents the tether
byte; the radio owner sets it on TETHER and clears it on UNTETHER.

Off-bridge (`+8C` clear), the event queries the current Location cell through
`565730`. Only when its slope byte `+11C` is zero does it call virtual `+2B0`
(`4C770F..4C775C`). For Unit, this is `70C620`: project Location XY through
`6D6410`, then compare that signed cell pair with virtual `+1B8` (`41BEA0`),
which divides Location XY by 256 toward zero and narrows each to i16. A mismatch
refuses the event. Both helpers ignore input Z. The existing projection owner
is `find_nearby_cell::project_world_coordinate_with_lookup`; native lookup order
and shared dummy-cell stamps belong to that owner. This vtable slot is unrelated
to the Techno data pointer at offset `+2B0` used by destination assignment.

Next, current mission `+AC` rejects Construction (18) and Selling (19)
(`4C7762..4C7774`), RTTI rejects Aircraft (2), and the current cell's building
rejects `WeaponsFactory=yes` (`4C778A..4C77DE`). The admitted effect order is
radio OVER_OUT(3) at `4C77EA`, destination `(NULL,1)` at `4C77F8`, null target
at `4C7804`, then QueueMission `(Unload=16,0)` at `4C7812`. Event 9 does **not**
call the UI admission query `700D50`. These Event gates and ordering are
instruction evidence; the lifecycle corpus starts at Mission_Unload.

For self-click action 4, Unit `73FD50` rejects planning mode through `637DB0`.
With no DeploysInto, `IsSimpleDeployer` at `74000B` routes to shared
CanDeploySlashUnload `700D50` via `73FFE6`. That query refuses a tube neighborhood
(`484AE0`), EMP, or a nonnegative signed tube index `Unit+684`, and accepts a
simple deployer without passengers. It has no `+6E0/+6E1/+6E2` refusal.
`ownership.simple_deployer_admission` executes these flag/tube-index controls.

`484AE0` defines a tube cell as signed `Cell+116` within the Tube array and
LandType 10. It refuses a current tube cell, or either of the first two north
or west neighbors being a tube while its next neighbor is not. Each direction
therefore checks three neighbors, through original MapGetCell and cell-coordinate
addition; Tube.direction is not read. `tube_admission` executes all 128 patterns
of the current cell and those six neighbors on the declared valid-cell fixture.

## Mission and transition

`Unit::Mission_Unload 73D630` dispatches passengers and other non-simple branches
before the simple-deployer branch at `73DE6E`. That branch calls `739AC0` when
`+6E0` is false, otherwise `739CD0`. It returns 1 while forward, reverse or
landing flags are set. Otherwise it queues Guard(5,0), Commences, and calls
Foot::Mission_Unload `4DA2B0`, which returns literal 450 without a cadence draw.

`739AC0` requires IsSimpleDeployer. Positive height requires DeployToLand; an
already deployed object returns. At positive height it sets `+134` and waits.
Once landing is clear, a missing DeployingAnim sets deployed immediately.
Otherwise it allocates an Anim only if `+130` is null, stores the reference,
attaches it, and starts the existing Techno Stage: value = AnimType `+2B4`,
rate/duration = `+2B0`, timer start = current frame. It preserves Stage increment
and changed byte. While forward is active and the animation exists, signed
`Stage >= wrapping(Start + End - 1)` sets deployed and clears forward.

`739CD0` returns unless deployed. It starts the same owner Stage and an optional
reverse Anim. Its completion comparison is `Stage >= wrapping(Start + End - 2)`;
it clears deployed/reverse. On this animated completion, DeployToLand calls
existing Techno::NearbyLocation `703590` with this object as anchor, resolves the
resulting cell and assigns destination `(cell,1)`. The no-animation immediate
clear omits that nearby call. NearbyLocation's existing owner coerces Winged
SpeedType to Track and uses the owner's MovementZone and OnBridge.

On each accepted undeploy visit, before UndeploySound, virtual `+C4` calls
`41C010` to read IsDisguised (`+1D8`). If set, virtual `+470` calls Unit
ClearDisguise `746720`: clear the active bit, call virtual `+49C` (`70CCF0`) to
mark RadarPosition dirty, then clear the disguise type and house references.
This is a disguise tail, not a cloak transition. The existing Rust owner is
`DisguiseRuntime::clear_unit`; stock SCHP has no disguise producer. This tail
is established by original instructions, not a separate executable case.

The owner Stage increments positively during both directions. The Anim has its
own Stage and reverse frame step. Production ordering is Mission before the
Techno Stage tick `6FABC4..6FAC31`; the pinned ordinary timeline begins at frame
200 and completes forward at 291 (owner Stage 10), reverse at 282 (Stage 9).
These are composed component executions with literal mission delay 1, not full
Logic scheduling. They record no Scenario RNG changes in these supplied states.

The Update/Undeploy sound tails call configured DeploySound/UndeploySound on each
visit, including unfinished visits. Stock SCHP leaves those unset; its effect's
StartSound is a separate animation/audio consumer.

## Landing and movement

Jumpjet hold `54BD30` processes a landing request only after IsMoving is true,
the destination XY equals current XY, and the target is null. A simple deployer
with DeployToLand and clear `+134` keeps hovering. Set `+134` permits state 4.
Descend `54C550` selects DeployDir for a simple deployer outside a forced
locomotor swap, and accepted touchdown clears `+134` at `54CA75`.
MoveTo's descending-to-ascending branch clears the same byte at `54B46D` and
abandons an active Unload mission. Deployment does not power off the locomotor.

Unit destination assignment `741970`: same NavCom with clear force-reassign
`+1F8` returns at `741A80..741A90`. Otherwise `741A9C` clears that force byte.
When deployed is false, either transition flag refuses the assignment. When
deployed is true, only nonnull Techno `+2B0` bypasses refusal; this is not Foot
`+6AD`. Refusal `743173` directly calls Foot::StopMoving `4DF0D0`, which only
clears navigation references `+5A0/+5A4`. It does not call a locomotor stop.
Thus ordinary Move does not initiate undeployment; another DEPLOY order does.

## Rules, assets and animation

DeployingAnim is a retained **rules** reference in TechnoType `+6BC`. Its null
constructor store is `711175`; `714706..71474C` reads exact key `DeployingAnim`
through ReadString `528A10` (capacity 128, empty default), then nonempty input
through AnimType FindOrAllocate `428B80`. Missing/empty/whitespace retains the
old reference; `none` and `<none>` clear it. It follows UnloadingClass. The next
rules key is InitialAmmo, not WalkRate.

DeployDir belongs to `[AudioVisual]`. Rules constructor `6656BD` writes raw 0.
`669272..66929B` reads exact key via ReadInteger `5276D0`, with signed prior raw
value shifted right 5 as default, then stores the read result shifted left 5
with dword wrapping. No clamp. Read order is PoseDir, DeployDir, DropPodPuff.
Jumpjet consumes its low byte shifted left 8. Retail authored 2 gives 0x4000.
The `readers.deploy_dir` row with `raw: ""` explicitly supplies a cached empty
value to the native scalar reader; it is not a physical empty INI line. The
physical INI loader omits empty values, so a layered `DeployDir=` line retains
the prior value instead. The Rust layered-INI comparison excludes that
cached-only row and compares the other saved reader rows.

WalkRate and IdleRate are **rules** reads at `712222..712256`, between TypeImmune
and MoveToShroud, with constructor values 1 and 0 (`710B02/710B0E`). They use
retained ReadInteger defaults and no clamp. SCHP has neither key. Type `+390`
is HoverAttack (bool reader `712553..712567`, key `8443B0`), not TurretSpins.

The original AnimType constructor/ART reader over physical SCHPDEPL bytes yields
Start 0, End/LoopEnd 11, rate 9, Shadow true. Physical SHP count 22 becomes 11
logical frames; authored ART Rate 100 becomes the native delay 9.
The deployment producer calls Anim constructor `421EA0` with type, owner XYZ,
delay 0, loops 1, flags 0x600, z-adjust 0 and reverse 0/1. Separate full native
constructor executions pin forward frame 0/step 1 and reverse frame 10/step -1,
both rate 9, first-AI guard 1, and no Scenario draws. The shared constructor
fixture observes Start; it does not execute StartSound playback or subsequent AI.

SetOwnerObject `424B50` attaches the animation to the owner. The producer's
`739C1A/739DFE` writes Anim `+D4` from virtual `+1E4`, GetRemapColour `705D70`.
This field is a **palette conversion**, not a House pointer. The getter prefers
the type custom palette indexed by owner color, otherwise disguise color when
the viewer cannot recognize the owner, otherwise owner color. Anim DrawIt reads
the conversion directly at `4232A3`.

Completion retains `+130`. AnimPointerExpired `710410` clears only an exact
matching reference at `710443..71044E`; it does not clear transition flags.
General PointerExpired `70794C..707954` also clears it. Techno destruction
`6F467F..6F4691` UnInits the referenced Anim and clears the pointer. There is no
retained-animation TechnoAI block: nearby `6FB3xx` accesses to offset 130 are
Cell fields, not this pointer.

## Body selection and counter

Unit DrawIt `73CEC0` suppresses the body when forward or reverse is active
(`73CF46/73CF54` to `73D43F`). Its temporary docked-harvester UnloadingClass
substitution occurs first (`73D2C4`). Visibility/disguise then selects a type;
deployed with nonnull actual UnloadingClass overrides it (`73D2F2..73D30C`).
Null retains the prior selection. DrawVoxelBody repeats that precedence at
`73B494..73B4D8`. The harvester temporary actual type is restored afterward.

Body frame selection `73B4DA..73B50E` uses signed IDIV: persistent Foot `+538`
modulo the selected model's HVA count. A nonzero body remainder is also the
turret frame; otherwise a present turret HVA uses Techno `+148` modulo its count.
There is no deploy/undeploy counter reset. Physical SCHP HVA has 2 frames;
SCHD has 1. `draw_frames` pins precedence and signed remainder controls.

The same Foot counter advances for voxel and SHP objects. After admitted Process,
`4DA886..4DAA01` tries a WalkRate increment when moving, or when an undeployed
Unit has a target and HoverAttack. Fallback IdleRate is enabled only when nonzero
and stationary. Final fallback increments every admitted airborne DeployToLand
tick. All increment arms reject warp-in, warp-out and Foot `+6AD`. The common
counter wraps; native drawing alone applies the selected model's modulus.
`body_cadence` pins 66 post-Process cases including negative frames and overflow.

### Concrete Aircraft and Infantry admission

`foot_class_cadence` adds bounded controls through the same
`run_foot_counter_slice`; the 66 legacy rows and their supplied motion boundary
remain unchanged. The new rows install the original concrete vtable, the
appropriate type home and the real locomotor constructor in the existing
Unit/refinery fixture. They do not construct a complete actor or execute
Process, movement producers or a world scheduler.

Aircraft constructor `413D87` installs vtable `7E22A4`; its `+2C` slot calls
`41C180` and returns RTTI2. Infantry constructor `517ACC` installs `7EB058`,
whose `+2C` calls `523340` and returns RTTI15. Unit's `7F5C70+2C` calls
`746E20` and returns RTTI1. Their actual type getters execute: Aircraft
`41C200` reads `+6C4`, whereas Infantry `51FAF0` reads `+6C0`; Infantry's
`+6C4` is Doing. Foot's `4DA88E..4DA89E` converts only RTTI1 to the Unit
pointer used at `4DA8ED..4DA8F3`. That test belongs only to the target /
HoverAttack / Unit-IsDeployed arm. Ordinary moving, idle and DeployToLand
counter paths remain shared. Non-Unit hover controls assert that original
Unit predicate `746D80` is never called.

The Fly rows construct `4CC9A0`. It installs ILocomotion at complete object
`+4`, vptr `7E89F4`; its `+80` slot is original `4CCAC0`. The latter reads
the interface pointer at entry stack `+4`, loads binary64 **interface+44 =
complete Fly+48**, compares it with original zero at `7E2800`, returns AL and
pops its four-byte receiver. The retained movement-request byte, destination
and target speed do not supply this answer. Finite nonzero speeds are true;
both signed zeroes are false. Declared Q16-exact current-speed priors include
one quantum and half speed. No callback supplies IsMovingNow or a counter
delta in these new rows.

The Infantry rows construct original Walk `75AA90` and execute its real
`+80` query `75AB40`: request, positive Foot speed fraction and a nonnull
retained head are independent component inputs. They cover each refusal,
ordinary moving cadence, idle cadence and the non-Unit HoverAttack refusal.
The paired Unit controls retain the existing stopped Drive query and its
actual deployed byte. Existing signed-frame and wrap controls remain, and
the Aircraft controls also cover the native signed-frame/counter boundaries,
warp-in, warp-out and locomotor swap. All new rows require unchanged native
text/vtables, all three seeded RNG states, locomotor/type bytes and actor
bytes outside `+538`. The inherited lifecycle observer boundaries must be
unvisited. No timer, RNG draw or detach belongs to this counter slice; the
adjacent MoveSound tail is outside it and retains its own existing owner.

`aircraft_draw_frames` executes original `4145B0..4145C0`, after selected
model admission: persistent `+538`, selected Type+B4 HVA header and signed
`CDQ; IDIV [HVA+8]`. Its seven signed-counter controls use the frame count
from unchanged physical SPYP.HVA (two frames/four sections). They complement
the 56 existing Unit body/turret/disguise/deployed-model rows; the latter
remain exact. The file header count at `+16` is copied to the declared native
loaded-header field `+8`; native HVA loading and model admission are not
executed by this arithmetic slice. The common production frame consumer is
`app/presentation/instances/units.rs::unit_animation_frames`.

Aircraft has a separate positive shadow suffix: `4148A0` calls Foot
`4DB0D0`, then Techno `706BD0`, whose actual-type `NoShadow` check is at
`706BF3`. Fly's HasShadow slot returns true. The `+538` remainder contributes
to its cache key, while original uncached shadow `707280` starts HVA frame
zero and takes the separate `ShadowIndex` section. The body-counter and
signed-remainder controls do not certify that shadow's geometry, cache,
matrix, admission, positioning or pixels. VERA's existing absent Aircraft
shadow remains a separate rendering mechanism; these controls establish no
whole Aircraft draw or native whole-flight RGB parity.

The legacy Rust deploy-state consumers in SpawnManager and cloak were unrelated:
SpawnManager `6B737C` tests Foot `+6AD`, while cloak head `6FB757..6FB79B` and
ShouldUncloak `6FBC90` have no deployment predicate. They must not become new
consumers of Unit `+6E0/+6E1/+6E2` during state migration.

## Coverage limits

Nearby search and destination locomotor work are explicit observers in the
lifecycle corpus; their existing native/Rust owners remain responsible. Type
reader caches are supplied from selected physical lexical strings, not full
native INI loading or all scenario layers. Full animation lifetime, sound
playback, custom palette rendering, terrain-edge/dummy-cell aliases, forced
Magnetron source `+2B0`, and native rasterization require separate native evidence.
The [production observation](../map_observation.md#siege-chopper-deployment-observation)
and receipt (`tools/map_observation.siege-chopper.validation.json`, in git at 1b1a8b43e) separately establish
the stock landing, visible forward animation, SCHD model, reverse animation and
return to flight in VERA's release app. They do not extend the native component
claims above.
