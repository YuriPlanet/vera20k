# Infantry Teleport Cell destination ownership

The ordinary CLEG Cell setter is `Infantry51AA40 -> Foot4D94B0 ->
Teleport718100 -> resolver718B70 -> Cell481180 -> Infantry51BF90`. The
comparison executes the original bodies through their actual returns, including
StopDriver51DAF0, DoAction51D6F0 and the destructive raw receivers5217C0/521850.

```sh
PYTHONPATH=. python -m tools.spatial_oracle.infantry_scatter_destination --teleport-cell --check
```

`infantry_teleport_destination.json` retains the original 26 Cell/overlay cases
and 54 command boundaries, and adds 14 independent Unit NavCom resolver cases
(40 pre-Process cases and 68 boundaries), plus two bounded refusal-tail cases
(46 cases and 74 boundaries total, including four direct timer controls). Its sidecar records the original binary, body spans, startup,
fixture boundaries and source bindings. The historical Cell payload retains binary identity
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The additive controls execute the supported Steam binary
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`;
the sidecar and `transport_native_identity` identify the actual replay input.
The existing Walk18, default-motion54, CMIN50 and Jumpjet531 corpora retain their
original values.

The fixture uses the existing Mission reader controller to execute the original
CLEG type constructor5236A0 and whole reader5240A0 on physical RULESMD, absent
LANGRULE, Battle and AnyTown layers, with fixed ART. JumpJet is the byte at+D94;
MovementZone is the dword at+5B4. The reader establishes false/Infantry7,
SpeedTypeFoot0, Strength125, Crawls1, Fraidycat0 and the Teleport GUID. All42
ClegSequence records come from the original523D00 reader. House50B730 executes
on explicitly recorded mode/house flags. These controls retain the existing
asset and OS boundaries; they do not execute the whole scenario loader or
Infantry construction/unlimbo path.

The already-live command fixture separately supplies Rules+1768=22 and a
nonzero land0 speed row (nine original float entries of1.0 at89EA40). These
are declared fixture inputs, not values read from retail GeneralRules. The
Rust boundary comparison supplies the same inputs; production keeps its
retail blocked delay60. The unchanged expected boundary values caught both
missing fixture inputs before validation passed.

The selected setter preserves these responsibilities and ordering:

1. Human Doing27..30 returns before mutation. Otherwise51AB65 asks the active
   locomotor's IsMoving. Its physical Cell4834A0 query precedes the current
   Attack/same-reference exception and StopDriver. Queued Attack is not current.
   Human same-reference prone Up7 runs through the existing DoAction owner.
2. Foot owns NavCom publication, admission and accepted timers. MoveTo calls
   the resolver, which releases retained+28 XYZ, or physical XYZ when+28 is NULL.
   Stop718230 clears armed+1C and request+34 but retains+28 and its reservation.
3. The resolver selects deck only for structural100 and owner Z strictly above
   floor+3*104, independently of OnBridge. Original startup establishes104;
   equality312 selects ground and313 selects deck on these flat cells.
4. Ready521B60 and optional Commence5B3570 precede placement. The existing
   Mission authority promotes a queued mission at this call position. The
   original priority corridor needs an object NavCom; pure Cell requests use0.
5. `bump_crush::place_infantry_in_native_cell` is the shared481180 selection
   owner used by Teleport, Walk and Jumpjet. It preserves preference, priority,
   vehicle/ground-Gate guards, the centre-row Scenario draw and scan order.
   `walk_head::selected_head` owns packed XY and successful output Z. Ground
   is sampled at the original incoming point, after successful selection.
6. InfantryCanEnter reads the resolved point. Success reserves that point;
   refusal restores physical raw occupation before the recursive class NULL
   setter. Full-slot refusal draws once; vehicle refusal draws nothing. Foot's
   timer tail follows either answer. No FNPC call executes in these controls.

`TeleportRuntime` privately owns resolved XYZ, request and the retained warp
adapter in the complete locomotor payload. Active queries and Process read the
active instance. The Foot effect view can also read a suspended instance;
BEGIN/END, snapshots and hashing retain that same payload, with no entity copy
or cell-centre destination mirror. Snapshot287 changes this layout after integration with main1024’s independent depot snapshot286. The child
`teleport_cell_destination_tests` compares native states and all three full RNG
objects after each boundary; its save/load, hash, suspension and command-route
checks are separately identified Rust regressions.

The three bridge/global/retask Rust replay hash ratchets change solely because
the entity-level absent Teleport tag is removed. An isolated control restored
only that old zero tag and recovered all three prior hashes exactly; their
pose/gameplay checks and absolute RNG pins were unchanged. The control was
removed before the final candidate. These constants are Rust hash regressions,
not native gameplay goldens. Legacy supplied warp-effect fixtures retain their
active class and keep the effect in a suspended instance, rather than replacing
the class whose original body the fixture compares.

Coverage remains bounded. Unit/CMIN keeps its prior centre-resolver adapter;
the historical Cell controls exclude object/FNPC destinations and Chronosphere
class admission, reservation and cleanup. The retained Relocate Process still
uses its cell-distance delay adapter; ChronoDelay now samples a global-frame
epoch through the shared719BF0 TimerCheck owner, as bounded by the additive
timer controls below. This historical Cell corpus executes no7192F0 Process
and cannot certify that complete lifetime or
the unrepresented secondary Process byte. OpenTopped rider relocation and
incoming-bullet detach remain documented at that owner. Those triggers affect
superweapon warps or in-flight attacks, rather than the selected pre-Process
Cell requests. Legacy placement callers still supplying no ground-Gate context
can refuse an open Gate before RNG; issue683 retains that separate caller
migration. The shared selection cutover preserves their existing supplied
context and existing native occupation comparisons.


## Overlay caller repair

Four additive controls preserve the prior22 cases/46 boundaries exactly. They
start on TIB01 and make two Cell requests: one repeats resource-cell admission;
the other refuses GASAND then requests a clear cell. Original Infantry51BF90
returns0 for TIB01 and7 for GASAND. Each request draws once from Scenario,
including the wall refusal, while Main and MapGen remain byte-unchanged. All
three complete1012-byte RNG objects reproduce at each of the54 boundaries.

The original Rules reader constructs the250-entry Overlay registry. Before
whole selected5FE770 reads, the Mission harness's original class templates match
the independent constructor bytes exactly; its existing Scenario/ART/assets
supply the reader prerequisites. RULESMD, absentLANGRULE, Battle and AnyTown
passes establish GASAND index0, Wall1, DamageLevels2, Armor6 and Crushable1;
TIB01 index102, Tiberium1, Land5 and DamageLevels1. The original CLEG Primary
reader establishes NeutronRifle damage8 and ChronoBeam Wall0. Cell fixtures
still supply land0 and a nonzero speed row: resource-land recomputation and
other overlay families are outside this comparison. No native answer is
replaced to make the controls pass.

Ordinary Move, track completion and restoration/lifecycle callers lend their
existing overlay registry through the destination owner. The existing pure
availability check rejects missing overlay type inputs before mutation,
including NULL-XYZ admission following slot refusal. Unexpected retained-input
errors after raw release use the existing physical-restoration/recursive-NULL
cleanup; these Rust context errors are not native admission codes. Focused
regressions pin atomic rejection, late-error occupation cleanup and both
restoration callbacks. Native overlay boundaries are replayed through the
class setter; the two already-queued Move controls also pass through ordinary
Move commands. Original Event4C73B9 queues Move with flag0; the resolver
Ready/Commence promotes it after the class preflight. Requeuing the same
selector retains it, and after promotion the second request does not queue
its current selector (5B3601..5B3612). These supplied prestates do not execute
the complete EventClass caller.

The final original write/check pass; the preceding Guard2 write/check also
passes the advertised Walk18 check. No legacy VM graph is repeated for the
queued-Move-only extension. The external
`overlay-queued-move-terminal-v5-receipt.json` SHA256 is
`c837f25d8800c07b802798668b29c9cf8a9e7d469c5d255b7824af8fa0553764`.
It retains failed setup attempts, unchanged original code spans,79 source
bindings and full old-corpus guards. CPP SHA256 is
`3ebb007e16568e2aee53865803eecb183f6840fc0e02bb1cae364a5c211b1954`;
canonical SHA256 is
`018ac9143882ed3aaab11cb407e2eafa5503850ba7581f72e0e291fc57a707e4`.
Three older selected sidecars receive a binding-only refresh through their
existing publishers. After main1024 integration, those sidecars and this CLEG
sidecar receive40 source-pin updates; all697 tracked CPP/data files retain exact
bytes, including the incoming depot and expiry corpora. The final central
binding checks pass. These refreshes do not extend native coverage.

## Production integration and validation

The reproducible [CLEG Cell profile](../map_observation.cleg-cell-destination.example.json)
uses stock AnyTown/Battle and the ordinary human production chain. America builds
AMRADR (retail GAAIRC forbids Americans), then GATECH at `(24,89)`, clear of the
construction yard's fixed ART 4×4 foundation at `(27,87)`. The loaded registry and
production captures establish CLEG's type handle 142 and object 1635 for this seed.
Different rules or startup inputs require rediscovering those handles.

The final ordinary release observation runs 10000 exact steps and records 56403
samples under the unchanged 100000 sample limit. CLEG is created at 8001 and becomes
live at 9135. Two ordinary Cell orders execute at 9500, first `(34,94)`, then `(35,94)`.
At 9501 the latter NavCom is installed while physical XYZ is still
`[8384,23616,416]`; at 9502 the unit is at selected XYZ `[9152,24256,416]`, with
NavCom cleared. It returns to Guard at 9518 and remains at that point through 10000.
The inspected 800×600 GPU readback shows the unit at the destination. This is Rust
production integration, separate from the original pre-Process comparison above.

Release executable SHA256 is
`d786d6a2ee35cef18cbd7ac2ca50108e27bbfa63094af034252710016e1d2ff7`.
The profile SHA256 is
`b2add10d8c9145038873dadb5c5fc948f35892f873e36d169927d6e6e13c3486`;
capture/run SHA256 values are
`9c24377620ff5d9d9eb251cb0a2e9d0535ccd31d68ecfbfa6c6eb09fa77c89a3` and
`ec10f21b9f1799d9cd2c22b10a77956a9fb966720ecd9e53da1c7b50668e97b5`.
Final BGRA SHA256 is
`c692b9ac59cd7e8766c730ff229643299cc8d38a052769d31c429d1a8fc713d1`.
The capture passed the ordinary wrapper and offline validation against the actual
release binary. The final GPU bytes match the prior accepted capture exactly.

Final required-retail library validation passes 9568 tests with 0 failures and 227
intentional ignored tests; Clippy passes. A separate exact 63-test retail movement
and integration sweep passes without fixture skips on the same simulation source.
Normal and optimized Python validation each pass 110 tests: 83 observer and
27 Cargo-owner checks. The optional exact type filter is
implemented in the existing observer so this long ordinary tech-tree path fits its
fixed budget; no simulation or RNG behavior changes with filtering. Earlier failed
or incomplete profiles remain retained and are not relabelled as runtime passes.

The final external Rust validation receipt
SHA256 is `20f4efdf1ad1d54f07b761a133e993f258fd6a1299460ca411dfefee4cfea81a`.
The destination8 and restoration11 checks also pass; the garrison13 checks use
the existing Infantry fixture constructor, preserving the exact Scatter RNG
and destination assertions. One fresh critic found the missing overlay input
closure; this owner repair and the native caller comparisons resolve it.

After incoming main1024, the same-binary absent-tag control recovers its
CDA10AEE6EA0AE46 hash. All601 complete rows match after removing only
`tick_result.state_hash`; the new Rust baseline is534C7788BB49A3D8. The
[retained control](infantry_teleport_replay/main1024/README.md) supports an
independent strict row check. Main1025 subsequently changes only Ghidra
exporter tools/docs;45 tool tests pass and every Rust file plus all other
frozen Python files remain exact. No native coverage is extended by that merge.


## Unit NavCom transport resolver controls

The additive `transport_rows` invoke the same original resolver718B70 with a
supplied Unit NavCom and declared Enter7 or Guard5 mission. They execute the
untouched Unit vtable7F5C70: +2C746E20 returns RTTI1, +1B841BEA0 returns
GetMapCoords, and +4C4DBDF0 returns navigation XYZ through an original
Walk75AA90 constructor and Link55A710 with an inactive head. The Unit coordinates,
health1, active flag, same-House pointer and optional Cell content-list head
(+E4 ground or +E8 bridge, following the supplied target OnBridge) are
explicit fixture inputs. The CLEG prerequisites still come from the existing
original type constructor and physical layered reader; the inherited sparse
asset manifest remains unchanged.

For Enter7 with matching NavCom/request Cell and the target in its content
list, original718CE9..718DE3 selects priority1. Cell481180 skips occupation
selection and its centre-row RNG draw, but still snaps XY to the **preferred
subcell**. It samples ground at the original incoming point through578080,
then adds the original deck rise when applicable. Priority does not preserve
an arbitrary incoming XYZ. Executed controls include:

| Supplied request | Native resolved XYZ | Context |
| --- | --- | --- |
| `[3200,2688,0]` | `[3200,2688,0]` | Centre; target vehicle20 |
| `[3264,2752,0]` | `[3264,2752,0]` | Canonical noncentral subcell; vehicle20 |
| `[3234,2703,99]` | `[3200,2688,0]` | Arbitrary point in centre sector |
| `[3324,2773,99]` | `[3264,2752,0]` | Arbitrary point in outer sector |
| `[3234,2703,999]` | `[3200,2688,208]` | Cell level2; incoming Z discarded |
| `[3324,2773,208]` | `[3264,2752,223]` | Level2/slope8; incoming-point ground |
| `[3324,2773,624]` | `[3264,2752,624]` | Level2 structural bridge; deck208+416 |

These priority1 successes preserve all three complete seeded1012-byte RNG
objects. A requested point in a different Cell selects priority0 and draws
ScenarioRandom once: `[3456,2688,0]` resolves to `[3520,2752,0]`, with its native
indices changing from `[0,103]` to `[1,104]`. Guard5 with the same target and
vehicle20 selects priority0 and resolves NULL. Queued Enter7 is promoted by
original Ready/Commence before priority selection. A target omitted from
Cell content still selects priority1, but actual InfantryCanEnter returns2
and resolution becomes NULL. The raw3C control demonstrates preferred-slot
placement despite all supplied infantry slots and the vehicle bit being set.

The additive Rust-facing payload retains each fixture input and full prestate,
resolved XYZ/current mission, all three full before/after RNG states, the native
Unit coordinate-reader outputs, and compact placement/height/CanEnter calls and
returns. Per-call repeated snapshots, startup/type copies and other unconsumed
diagnostics stay out of the committed projection; the existing controller still
executes and observes those original calls. Every pre-existing Cell/overlay
payload key remains exact. This is bounded
native resolver execution, not complete transport boarding parity: Unit and
Infantry construction, the object class-setter admission, radio negotiation,
passenger attachment and Teleport Process are outside these controls. The
original object setter and per-cell entry chain need their separate static
and played evidence; no radio or passenger return is substituted here.

Reproduction also requires the two flat sparse roots from
[`anytown_damage/retail_manifest.json`](anytown_damage/retail_manifest.json):
`VERA20K_SHRAPNEL_INPUTS` for the reader bootstrap and
`VERA20K_ANYTOWN_INPUTS` for the selected AnyTown map/TMP/SHP inputs. The
existing `tools.sidebar_oracle.stock.mix/mix_hash` owner can extract the named
retail bytes; each file must match the manifest size and SHA256. Adding all
retail SHPs to the sparse bootstrap would change its declared asset boundary.

## Declared-refusal mobile Enter tail

`refused_enter_rows` starts at original `51A488`, after a declared negative
RadioCanEnter condition. It supplies the original `519630` saved-register/local
frame, ESI=passenger, EDI=transport, other saved registers and locals zero, reason2
and a RET4 sentinel. The preceding Radio call and complete PerCell dispatcher
are outside this boundary; no Radio answer is replaced.

Two explicitly supplied warp-in arrival states run the original class NULL
setter, QueueMission(Guard5,0), Scatter(NULL,1,1), FNPC56DC20, MoveTo718100,
Process7192F0 and TimerCheck719BF0 through the actual tail RET4. Warp-in is true,
frame100 and the timer is start100/pad0/duration30. These are arrival inputs,
not a native construction or full timer-lifetime claim. The separate direct
timer controls below establish expiry from the global epoch; these refusal
controls never reach expiry.

Both controls retain current Enter7 and queued Guard5, clear NavCom and armed/
resolved XYZ, preserve warp-in, and write movement/blocked timers `[100,0]` and
`[100,22]`. The blocked duration22 is the declared fixture Rules value. The
real FNPC candidate cannot become a new destination while warp-in remains set.
Seed31 at `[3200,2688,0]` makes one RandomRanged call and one basic Scenario draw;
seed42 at `[3264,2752,0]` makes one RandomRanged call but two basic draws because
of rejection sampling. All three complete before/after RNG states and compact
original call/return order are retained. The non-warp control reaches an
unclosed animation/CRT prerequisite and is excluded. These controls establish
the bounded refusal tail, not whole passenger/radio or post-unload parity.

## Teleport elapsed-frame timer controls

Four `chrono_timer_rows` execute original719BF0 through RET4 with supplied
warp-in1/request0/timer-state0, a complete epoch/pad/duration and current frame.
The actor's target+2B4 points to the live declared Unit; this suppresses the
no-target passive-acquisition callback without replacing its answer.

Epoch100/duration30 preserves warp-in at frame129 and clears it at frame130 and
131, without intervening Process calls. Epoch-16/frame14 demonstrates the
32-bit frame-counter wrap at elapsed30. Timer bytes and all three complete RNG
objects stay unchanged. These direct controls establish elapsed-frame expiry;
they exclude construction, Unlimbo scheduling, nonzero timer-state progression
and no-target expiry callbacks.

Instruction reading separately establishes Process publication: its interface
pointer is primary+4;7194AC selects primary+3C,7194B1 reads globalFrame,7194BE
writes the epoch, and7194C7 initializes duration0. Rules/distance branches write
the chosen duration and epoch before719579 publishes warp-in. Those arithmetic
branches are not executed by the direct TimerCheck controls.
