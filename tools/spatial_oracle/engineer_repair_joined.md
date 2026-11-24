# Ordinary Engineer building entry: native joined comparison

`engineer_repair_admission.py --joined` adds nine executable histories for an
untagged ordinary `ENGINEER` approaching an already admitted damaged `GAPOWR`.
The older 22 interior admission rows and metadata remain independent and
unchanged. All expected results come from the active-retail executable.

The harness composes the existing Construction arena/map/ART/Anim/RNG fixture,
Sound and Reader input owners, and Mission GUID transport. Original instructions
perform command admission, event delivery, Walk arrival, Map lookup, PerCell,
repair or ownership transfer, sound scheduling, health/power publication and
Engineer retirement. Hooks supply the declared platform or prior-state bounds
below, and observe native effects. No gameplay decision is answered.

## Reproduction

Use the native environment described in [the shared oracle guide](../native_oracle.md).
Extract the physical files named in the payload's `retail_files` into one
directory, then run from the repository root:

```sh
export VERA20K_ENGINEER_REPAIR_ASSETS=/path/to/engineer-repair/extract
python -m tools.spatial_oracle.engineer_repair_admission --joined --check
python -m tools.spatial_oracle.engineer_repair_admission --check
```

The additive payload is `engineer_repair_joined.json`, with provenance in
`engineer_repair_joined.meta.json`. `--write` is only for deliberately recording
native execution; never generate expected values from Rust. The shared
`finish_vectors` owner refuses to publish if a declared source changes during
generation.

Inputs are `RULESMD.INI`, `ARTMD.INI`, `SOUNDMD.INI`, `MPBattleMD.ini`, `Hills.mmx`,
`ENGINEER.SHP`, `GGCNST_A.SHP`, `GGCNST_B.SHP`, `GGPILE_A.SHP`, `GGPOWR_A.SHP`,
`GTCNSTMK.SHP`, `GTPILEMK.SHP`, `GTPOWRMK.SHP`, `audio.idx` and `audio.bag`.
`LANGRULE.INI` is absent in this retained retail input set. The Constructor and
Building readers reuse Construction's selected inputs, including its unused yard
and barracks prerequisite controls; this does not add those objects to the repair
coverage claim. SHP filenames and metadata execute natively. Missing icon and
other body requests are recorded explicitly, outside this gameplay comparison.

The winning audio files are `langmd.mix/audiomd.mix`. Original `4011C0` reads and
sorts all 2,285 physical IDX entries with the original CRT `qsort`; original
`4015C0` selects `UREPAIR` at sorted index 1,394. The selected SoundList uses actual
physical keys and source order for Dummy and BuildingRepaired. Sound registry
IDs are fixture relative, while the audio sample ID is from the full physical
index. Each file's length and SHA256 is pinned in the payload.

## Executed inputs and route

Original Rules constructor `665650`, BuildingType constructor/reader
`45DD90/45FE50`, AnimType constructor/reader `427530/427D00`, Sound list and Voc
readers `7510D0/750440`, InfantryType constructor/reader `5236A0/5240A0`, Land
reader `674000`, Country constructor `5113F0` and the bounded passive-country
read `511A75..511A95` execute. Rules are read in RULESMD, optional LANGRULE,
Battle mode, Hills order. ARTMD and SOUNDMD are independent physical inputs.
The full Country/Side and full Rules::Process startup are excluded.

The original Object translation-unit CRT table at `8141D8` runs all 14 entries
in source order, reusing the startup exercised by `object_flight_height.py`.
Original `5F37C0` computes level height `AC13C8=104`; `5F3860` computes the
416-lepton deck scalar `AC13BC`. The table and before/after scalar states are
recorded for each route. An earlier fixture omitted this startup and retained
cold zero: a marked building at height zero then passed `5F6B90`'s high-flight
test, returned layer AIR from `5F4260`, and skipped ground reinsertion in
`5683C0` during ChangeOwner. That payload is superseded fixture evidence; its
empty evacuation draw list did not prove ordinary capture admission.

`BuildingRepairedSound` reads at `669C59..669CA9`: exact-case section AudioVisual,
literal key at `83A8CC`, constructor ID -1, ReadString `528A10` capacity 128 and
Voc lookup `7514D0`. Stock and mixed-case sound names bind; absent, empty and
unknown values retain the preceding ID. Wrong key/section case does not replace
the ID. Native strings of 127, 128 and 129 characters return length 127 and a
127-character buffer. Unknown lookup returns -1 and retains the old reference.

`EngineerCaptureLevel` reads at `671DF1..671E16`: literal key at `83B414`,
constructor f32 1.0, prior f32 converted to the default double for original
`5283D0`, then native store to f32. Stock layered inputs retain 1.0. The executed
0.5 control produces bits `0000003f`: enemy HP 374/750 and 375/750 resolve action
9; 376/750 and 750/750 resolve action 28. The ordinary own damaged building
resolves action 29. These are original `51E49E` suffix results; the upstream
cursor/Foot selector is supplied, and no whole cursor claim follows.

The original resolved action dispatch `51F1A4` runs through Capture command
`4D74E0`, MegaMission `6FFBE0`, event construction `4C6860`, delivery `4C6CB0` and
Commence `5B3570`. Both native Repair action 29 and Capture action 9 use Capture
mission 8. Engineer constructor `517A50` and Unlimbo `51DFF0` run. COM transport
routes the original parsed locomotor GUID to original Walk factory `6C4790`.

Whole Walk `75AEC0` receives an already-paid final head within 16 leptons, commits
position, calls physical Map lookup `565730`, and enters whole PerCell `519630`
with reason 2. The map is an explicit flat visible 32x32 physical-cell prior,
the target an already admitted ordinary building with registered UID 1000.
Original Foundation readers `45EC90/45ECA0` produce 2x2 from the retail type's
index 3; the supplied physical membership uses those dimensions. Location is
`2688,2688,0`, while original Building GetCoords `447AC0` produces the center
`2816,2816,0`. Both coordinates and dimensions are saved in state snapshots.
Path search/payment and travel before this final head are excluded. Initial
actual HP 374, estimated HP 611 and sampled HP 374 deliberately distinguish the
health authorities.

Runtime mode `A8B238=1` is supplied by the reused `make_dock_fixture` default,
and the actual readback is retained as snapshot `game_mode`. Mode selection and
startup are outside this comparison; the runtime CellPUT, discovery and House
predicates consume that inherited value unchanged.

## Native outcomes

| Route | Arrival outcome |
| --- | --- |
| `own_damaged` | Actual and estimated HP become 750; Engineer is consumed. |
| `allied_damaged` | Same repair/consumption; current allied owner is retained. |
| `stale_second_engineer` | Both orders were issued while damaged. First repairs; second reaches full HP and survives terminal Scatter. |
| `repair_order_enemy_owner_race` | Current enemy ownership is reevaluated at arrival; full ChangeOwner captures the building and consumes the Engineer. HP remains 374/611. |
| `enemy_selling_refusal` | Selling current enemy target is refused; Engineer survives Scatter. |
| `enemy_warped_refusal` | Warped predicate `70C5B0` reads Building byte +270; target is refused and Engineer survives Scatter. |
| `missing_physical_building` | Cached order alone does not admit repair; original Foot PerCell tail runs. |
| `different_first_physical_building` | A different first ground building rejects admission even when the ordered building is next in the same physical list. |
| `enemy_noncapturable_consumes` | An explicit asymmetric false Capturable prior consumes the Engineer without repair or capture. This is not a stock key/default claim. |

Current/queued mission, Doing, destination/target, positions, Walk head/moving
state, native mission timer fields,
actual/estimated/sampled HP, ART objects, House lists/counters/dirty flags and
Building discovery bytes `+41A/+41B/+41C`, House discovery latch `+1F4` and
complete RNG buffers are saved before and after each arrival. Terminal Scatter
retains current Capture 8 and queues Move 2 in these histories; it does not
invent Guard or consume the surviving Engineer. Its Scenario draw is
`RandomRanged(0,4)` at callsite `51D2BA`, with result 3 in the stale second-arrival
history and 0 in the two enemy refusal histories. Repair/capture arrival draws
none. Every request, underlying raw advance and full state is recorded.

Doing is the original constructor's retained -1 in the supplied pre-arrival
state; the fixture does not claim the preceding whole Infantry animation/travel
loop. The stale/refusal controls clear the destination before Scatter, and their
real coordinate source is the native Building GetCoords above.

The bounded Rust fixture supplies recorded retail `ENGINEER/Sight=4`, registers
the selected `GAPOWR_A`/`GAPOWR_AD` types and binds the native-selected Voc catalog
through the existing production readers. These are prerequisites: Infantry
Unlimbo `51E0EF` clears discovery history for Sight0, and an ART section alone
does not register an animation type. No device or sample output is inferred
from the fixture's catalog identity binding. The focused Engineer/House tests
pass all nine routes/ten arrivals and the106 House boundaries; broader final
candidate and production results belong to the chain validation record.

The production receipt (`tools/spatial_oracle/engineer_repair.production.json`, in git at 1b1a8b43e) retains the final strict
9445-pass Rust suite, clippy, 460-test Python suite, physical retail input
comparison and repeated normal release observation. Ordinary production commands
construct and deliver the target/Engineer before MTNK damage and paid repair;
Engineer arrival at frame1602 changes actual HP304→750, replaces damaged ART and
retires the Engineer. All1650 observed steps and full Metal bytes repeat exactly.
This demonstrates the Rust production composition; these whole-world timings,
state fingerprints and pixels are not native goldens or whole-object parity.

### Tag ordering and bound

Original `519B43` null Building Tag gate precedes the Engineer/type/alliance/HP
branch; its nonnull receiver call for event 1 is `519B53`. Enemy capture has the
second Building Tag gate `519F34`, event 1 call `519F44`. After repair or capture,
Engineer Tag gate `51A015` would call event 48 at `51A025`, followed by unconditional
UnInit at `51A02E`. The payload observes these original null gates; it does not
pretend notification calls ran. Tags are NULL in every route, including the
native-constructed Engineer. Full tagged receivers and their live
Tag/Trigger/TEvent/TAction/expiry behavior are a separate unresolved mechanism.

### Capture consumers

Whole Building ChangeOwner `448260` and Techno ChangeOwner `7014A0` execute.
Before the owner store `701735`, null-source RecordKill `702D40` books the old
House building loss through `70305C`. Native Building CostOf `45EDD0` and Techno
CostOf `711F00` price the target using the old House. Existing native `50BF60`
with explicit empty FactoryPlant membership produces neutral House cost factors;
Country constructor produces neutral Country factors. The race therefore awards
800 to the new House at `7015D2`, increments its kill array +5438 at old House
index 1 through `70164D`, and records the old House captured latch. Vtable +38
is `6F9DB0`, reading current House +30; the array is indexed by House, not Country.
Owned/live/tracked counts and the real Building list migrate from old 1 to 0 and
new 0 to 1 through `4FF550/4FF700`, `5025F0/502A80`, `4FF980/4FFA50` and `4FD150`.
Both Houses' next dirty gates execute. Paid repair stops without a ToggleRepair
sound. Neither actual nor estimated HP is repaired on this owner-race route.

ChangeOwner's `701674` and `701691` call Building Mark `43F180(0)` and `(3)`.
With original startup initialized, Map PlaceDown `5683C0` restores the primary
ground cell before Building `4576F0` scans the Infantry registry.

During that Mark3, original `43F691 ->5683C0 ->5684BB ->47E8A0` reaches the
CellPUT virtual discovery call `47E9DC`, with the Building as receiver and
PlayerPtr as the explicit viewer. Whole Techno `6F4960` executes before owner
store `701735`. The race's first call has discovery bytes000 and the old enemy
owner still in `+21C`: `6F49D8` sets current-viewer discovery `+41B`,
`6F49DE/6F49EA` dirties that old House's power/radar and `6F4A25` sets its
`+1F4` latch because `+41A` is zero. The Tag remains NULL. Subsequent PUT
calls return at `6F497E` because `+41B` is already set. The viewer equals
PlayerPtr here even though the receiver's owner is still foreign; ownership
does not substitute for the viewer argument. These entry priors, writes and
before/after discovery states are retained in the joined payload.

The evacuation's original
Foot coordinate call `457716` returns `2688,2688,0`: Walk has already cleared
the head to NULL and uses current XYZ. Map `565730` selects cell 10,10 and first
Building `47C520` returns this target; alive is true and NavCom still matches.
The real `51D0D0(NULL,true,true)` call therefore runs before Engineer UnInit.
Original Walk IsMoving `75AB30` still returns 1 during this callback, so `51D172`
demotes the force flag to zero. Capture's native MissionControl Scatter byte is
zero and `51D184` returns before direction selection or RNG. The payload records
these admission boundaries, rather than inferring them from absent trace names.

### Repair, sound and cleanup

Full `701410` marks the target, writes actual and estimated HP, calls
ToggleRepair `446FF0(0)`, and replaces `GAPOWR_AD` with `GAPOWR_A` through original
`451EE0` and complete Anim allocation/Unlimbo/Start/delete lifecycle. It creates
the original BuildingRepaired sound event through `7509E0/405190` before Engineer
UnInit. No repair RNG draw occurs.

Audio registry `402940`, device `402AF0/409470`, actual primary-format/parent and
16-channel caller `406BD9..406C3A`, and pools `403ED0` execute. Whole scheduler
`4041D0` enters the real channel/sample selection and backend. Stock single-sample
normal sound draws twice from Main `RandomRanged(0,0)` at `405684` and `40569C`;
neither advances the underlying stream. Native BAG reads use the physical sample
offset 10,135,498 and 18,828 bytes, original IMA decoding runs, and the event
reaches playing state 3. DirectSound buffers provide byte storage and initial
playback cursors 0; Win32 file/clock/critical-section/thread/device transport are
explicit boundaries. Later device progression and audible waveform equivalence
are excluded.

Full UnInit `4DE5D0/5F65F0`, expiry `7258D0`, native Infantry/Foot/Techno/Object
limbo and deferred `725C70` drain/destructors run. Consumed Engineers first become
dead/limbo with one queued deletion, then leave the native Infantry and UID
registries. The arena retains freed bytes for observation; post-drain raw object
storage is diagnostic, not a claim the object remains live.

### Delayed health and power

`701410` leaves sampled Building HP +544 at 374 and does not dirty House
+5778/+5779. Existing native output `508C30/44E7B0` is 99 for 374/750 health and
Power 200. The next House gate `4F84D9..4F84F1` before the Building sample still
publishes 99. Original Building sample `440042..440074` changes +544 to 750 and
sets both dirty flags. The following House gate runs whole power recomputation
and publishes 200. All Houses execute the gates in supplied House order.

The two sampling/gate pieces are exact original interiors. Full Building AI,
House AI and full Logic/Scenario scheduling are outside the fixture. Their live
integration must be validated separately; this corpus establishes the required
ordering and consumer bodies without supplying a Rust result.

## Coverage limits

This is an executable comparison of the nine supplied common-entry histories.
It does not certify complete Engineer or power-plant objects. Full travel/cursor
selection, broader Building/House startup and global scheduling, tagged routes,
alternate MultiEngineer/passive-country occupation mechanisms, garrison,
campaign and other type-specific branches remain outside this corpus. Original
presentation calls inherited from Construction are argument-cleanup sinks.
Native scalar/RNG/timer/lifecycle decisions in this route execute; rendering and
later hardware playback are not compared. Original executable sections are
checked unchanged, and execution coverage is pinned by instruction count/hash.
