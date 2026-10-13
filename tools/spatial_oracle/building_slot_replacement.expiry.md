# Building animation expiry callback

This additive mode of [building_slot_replacement.py](building_slot_replacement.py)
executes original depot animation completion and the shared Building expiry
callback. [The payload](building_slot_replacement.expiry.json) contains 64 expiry
rows, two separate stock body/readiness controls and five shared-decoder
validation controls;
[the sidecar](building_slot_replacement.expiry.meta.json) records original bytes,
assumptions and substitutions. The historical 420-row
replacement payload and its metadata remain byte-identical.

Original `gamemd.exe` SHA-256:
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The native environment uses Unicorn 2.1.4 and x87 control word `0x0E7F`.
Every row asserts unchanged original executable sections and Building,
BuildingType and Anim vtables. Numeric values, frame histories, RNG states and
callback ordering come from original execution; no Rust values produce goldens.

## Reproduce

Use the executable/environment selection in [native_oracle.md](../native_oracle.md).
Supply the selected scenario files `RULESMD.INI`, optional `LANGRULE.INI`,
`MPBattleMD.ini` and `XMP03T4.MAP` under `VERA20K_DEPOT_SERVICE_INPUTS`.
`ARTMD.INI` is taken from that directory when present, otherwise `ini/ARTMD.INI`.
The ART hash is `e1f0378394313c04ebbd5073f47785ee3e46f1b3c62d65724e8f3c310ee7ba31`.

Extract `NGDEPT_B.SHP`, `NGDEPT_C.SHP`, `GGDEPT_A.SHP`, `GGDEPT_B.SHP`,
`GGDEPT_C.SHP` and `GGDEPT_D.SHP` with the existing
[asset owner](../asset_browser/README.md), using `asset extract <NAME> --out <DIR>`
or its `asset_extract` tool. The six physical files come from
`ra2.mix -> generic.mix`. Point `VERA20K_BUILDING_SLOT_EXPIRY_ASSETS` at that
directory; its default is `target/asset/building-slot-expiry/extract`. The payload
retains each full-file hash/size, native filename/header reads and normalized
extraction receipts when `../receipts.json` is present.

The retained run used the following commands from the worktree:

```sh
source ~/Documents/vera20k-dev/env.sh
export VERA20K_DEPOT_SERVICE_INPUTS=.local-audit/war-miner-attack/inputs/extract
export VERA20K_BUILDING_SLOT_EXPIRY_ASSETS=.local-audit/depot-service/animation-assets/extract
python -m tools.spatial_oracle.building_slot_replacement --expiry --write
python -m tools.spatial_oracle.building_slot_replacement --expiry --check
python -m tools.spatial_oracle.building_slot_replacement --check
```

The mode composes the existing lexical/cache `Reader`, original constructor
and scalar-reader bodies, `JoinedFixture`, map/House fixture, allocator and RNG
owners. It adds observers and supplied field controls; it does not copy their
math, parsing, radio or native behavior into another implementation.

## Inputs and executed boundaries

Original BuildingType `45DD90`, Rules `665650` and AnimType `427530` constructors
execute. Selected Strength `5F94D3..5F94F3`, UnitRepair `460906..46092F`, Grinding
`460968..460982` and ConditionYellow `66B35E..66B385` reads execute in
RULESMD → optional LANGRULE → MPBattleMD → XMP03T4 order. Both depots retain
Strength 1200, UnitRepair true, Grinding false and ConditionYellow 0.5. Constructor
Strength is zero and the three family gates are false.

ART is unlayered. Its original IsAnimDelayedFire `4611A3..4611C0`, body/slot
`4615CA..46421A` and full AnimType `427D00` readers execute over physical selected
strings and the six actual SHPs. Both depots leave IsAnimDelayedFire false and
their idle body control is `{0,1,0}`. The 16 selected AnimTypes are allocated in
the physical RULESMD Animations order; numeric IDs and pointers are fixture
identities. Their full original fields and bytes are retained in `inputs`.

The Building/House/map admission is supplied by `JoinedFixture`. Original empty
Techno vector stores `6F3041..6F30D6`, Building constructor stores
`43B68D..43B71F`, listener initializer `7254D0..725506` and constructor append
`43BB81..43BBD4` establish an original-vtable Building listener. Creation runs
whole PlayAnim `451750`, allocator `451890`, Anim constructor `421EA0`,
Unlimbo/Start/Middle and the real arena allocator boundary. Expiry runs either
whole AnimAI `423AC0` per frame or whole Building PointerExpired `44E8F0` with
declared controls. Scalar-clear runs whole `451E40` and its real deleting
destructor `426590`/`4228E0`.

The existing fixture supplies GameOptions game speed 3 at `0xA8EB60`. Each expiry
and stock readiness row records the actual native `read32(0xA8EB60)` as
`game_speed`; the sidecar identifies its shared fixture owner. Native option/UI
loading is outside this comparison.

## Native results

Normal completion `+179` is separate from damage. Constructor `421FB0` clears
it; the default/load constructor's `422803` clear is also pinned as original
bytes. On the selected MakeInfantry=-1 terminal path, `424B31` sets it before
virtual UnInit `4255B0`. The executed `+19B` cancellation control expires at
frame 200 while completion remains false and creates no retraction successor.

Runtime `inactive` is Anim `+19B`, cleared at `422003` and read by the AI cancel
test at `42435F`. The independent `+198` byte suppresses sound, with constructor
clear `421FF1` and UnInit sound gate `4255DA`. Five `observer_validation` controls
execute original AI or direct UnInit over asymmetric byte inputs using the
shared `anim_bouncer_launch.constructor_state` decoder. Cancel-only retires and
preserves `+19B=1`; sound-only remains live. Direct UnInit preserves both bytes.
These controls validate the observer and do not assert Rust sound behavior.

Full PointerExpired calls inherited `7077C0`. The Anim-specific tail first
checks damage-fire references, then Anim slot marker `+118`, then calls `451B40`.
That leaf requires Building ObjectAlive `+90`, scans the 21 slots, and clears
the matched pointer at `451B70` before creating its successor.

| Matched slot | Leaf successor and gates |
| --- | --- |
| 10, UnitRepair | Any contact and effective Mission 20 → Work 11; otherwise Idle 18. Completion is ignored. |
| 10, UnitRepair false | IsAnimDelayedFire and completion true → Active 3; otherwise clear only. |
| 12 | UnitRepair and completion true → Idle 18; otherwise clear only. |
| 15 | Completion true → SuperAnimTwo 16. |
| 17 | Completion true → SuperAnimThree 14. |
| Other slots | Clear only. |

Every successor requires a nonempty chosen string. Original current-health
ratio `5F5C60` at or below ConditionYellow chooses damaged. Health 599/600/601,
health changed after creation, normal/damaged empty names and garrisoned Active
controls distinguish those choices. The garrison getter `4581F0` returns
Building `+694`: healthy Active chooses garrisoned only for a positive count;
damaged Active chooses its damaged name regardless of count.

Effective Mission `5B3040` uses current unless it is -1, then queued. Contact
`65AE30` accepts any nonnull slot within its count. The current/queued and later
sparse/count-zero controls retain those actual outcomes.

The outer listener has additional branches before the leaf: matched slot 8
creates Idle 18 at `44E99F`; Grinding with matched slot 10 creates Active 3 at
`44EA02`. They run without the leaf's ObjectAlive or slot-marker gates. Shared
8/15/17, Grinding and delayed-fire controls explicitly supply names/gates over
retail-created AnimTypes; those branches are not active selected depot routes.

| Created at frame 200, whole AnimAI each frame | Terminal frame | Fresh successor |
| --- | --- | --- |
| NADEPT extension 10 | 244 | NADEPT_C2 / C5, Work 11 |
| NADEPT retraction 12 | 242 | NADEPT_B / BD, Idle 18 |
| GADEPT extension 10 | 240 | GADEPT_B / BD, Work 11 |
| GADEPT retraction 12 | 240 | GADEPT_D / DD, Idle 18 |

The frame arrays retain stage, all three timer words, loop, first-AI, completion
and alive values. Replacement constructors produce fresh timer/loop/first-AI
state. Expiry rows preserve all bytes of body `+F8`, independent repair progress
`+620` and ready `+6DD`, with no writes to those regions.

Normal UnInit broadcasts twice: ObjectUnInit `5F6616`, then Limbo DetachAll
`5F4D61`/announcement `5F5311`, before Display and Logic removal. Ordinary
selected rows clear on the first callback and create no second successor.
The alive-false outer-slot8/Grinding controls execute two constructors. Scalar
ClearAnim preclears its slot before destructor announcement, creating no
successor even with completion supplied true. Both complete RNG buffers are
unchanged and no requested/raw draw occurs in any retained row.

The two separate body controls run BeginMode `447780(1)` then whole
UpdateAnimation `4509D0`. The stock idle rate is zero: BeginMode leaves ready
false; UpdateAnimation writes ready true at `451218`. ReadyToCommence `454250`
is a BOOL in AL; raw upper EAX is retained without treating it as return authority.

## Schema, limits and consumers

`inputs` holds constructor, layered key reads, ART body/slot reads, 16 AnimType
receipts and physical SHP provenance. Each `rows[]` item has input, expired
identity, constructor completion default, before/after `JoinedFixture` snapshots,
game speed, every live frame, creation events, ordered callback/slot/marker writes, native
events, retained clocks and complete RNG buffers. `stock_body_readiness[]` is
separate because it deliberately runs the body producer excluded from expiry.
`observer_validation[]` separately retains raw `+198/+19B/+179/+118`, canonical
constructor/runtime observations, original calls and complete RNG buffers.

Live rows stop at deferred UnInit. Global Logic scheduling, subsequent storage
retirement, full class/map admission, physical INI/MIX loading, native disk
serialization, rendered pixels and arbitrary mods are outside these comparisons.
Direct receiver rows supply private gates or names explicitly; they do not
establish a natural producer for those synthetic combinations.

The shared Rust consumers are [building_art.rs](../../src/sim/building_art.rs)
and [anim_class.rs](../../src/sim/anim_class.rs). Rust validation is owned by the
mechanism owner; this report establishes and retains native execution, not a
standalone claim that all gameplay paths are equivalent.

The independent native evidence worker's 27 overlapping controls agree with
this corpus on canonical successor names, every frame/runtime/timer, retained
clocks and both complete RNG states. The payload and legacy checks are reproduced
by the commands above.
