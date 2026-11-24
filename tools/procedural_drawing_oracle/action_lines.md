# Selected ground Unit action lines

`action_lines.py` executes original retail `gamemd.exe` for the selected local
ordinary ground Unit Move and Attack routes. `action_lines.json` is the executable result;
`action_lines.meta.json` records its scope, assumptions and replay command. It is
native evidence for bounded prepared inputs, not a native Scenario or production
rendering capture. The implementation and production validation remain separate.

Run from the repository root with the native-oracle environment described in
[`../native_oracle.md`](../native_oracle.md):

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.action_lines --check
```

`--write` deliberately regenerates both payload and metadata. No native
instruction is replaced by this adapter. Surface, palette and destination setup
reuse `Rally`, `PaletteReader` and `track_destination.make_destination_fixture`.
Do not create another rasterizer or terrain/actor fixture for these cases.

The appended `attack_cases` use this same surface and primitive owner for the
ordinary ground Unit Attack branch. The separate reproducible prerequisite
mode composes the existing retail `anytown_damage.mission.Mission` world:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.action_lines --attack-prerequisites --check
```

It requires the retained original asset inputs used by Mission, through
`VERA20K_SHRAPNEL_INPUTS` and `VERA20K_ANYTOWN_INPUTS`. It produces
`action_lines_attack_prerequisites.json` and its metadata with the same
`finish_vectors` check/write protocol. Physical inputs and reused source
identities are recorded; no research-path imports are used. The earlier289
Move/control/production rows and all their prior top-level values are preserved.

## Ordinary ground Unit Attack extension

Nonnull TarCom takes priority over NavCom and the whole queued-cell vector at
`4DC0B3..4DC1A7`. Actual Unit virtual `+300` resolves to `6F3D60`: this is a body
pivot using Drive's drawing matrix and type `TurretOffset`, not PrimaryFireFLH.
Unit `+AC=41BE00` delegates to raw Object coordinates. Aim `70BCB0` starts from
TarCom `+58=410540`; a moving Unit uses real Drive admission, live Foot speed,
current weapon, raw object distance and current body facing. The branch then
uses palette index8 (`0xA800`) and the existing original `7049C0`/solid leaves.

Thirty-one prepared raster rows cover eight endpoint directions with stationary
and moving targets, simultaneous NavCom/queue priority, no NavCom, timer24/25,
target-null controls, body/turret headings, nonstock pivot80, and target live
fraction/veteran/crate inputs. `weapon_inputs` explicitly supplies stored
speed95/ROT0/Floaterfalse/Gravity6 from the physical105mm reader/postpass packet.
Its authored Speed40 reads102 first; the later Range5 ballistic postpass writes95.
Both authored values are retained beside the prepared stored value. The actual
`773070` calculation still runs and derives ROT0 speed from distance. Every raw
speed17 has authored Speed7 alongside it. Existing `track_speed_native` owns the
supplied getter fields; the physical full Unit reader in the prerequisite
packet independently establishes Speed7 ->17. All original actor, locomotor
and timer bytes remain unchanged across drawing, with no observed RNG, restart
or detach entry. These rows use a flat, unrocked source; wider draw-matrix
variants are separate required mechanisms.

Prepared Unit deploy state `+6D8=-1` matches the existing destination fixture
and the recorded original constructors. Each requested moving row calls the
actual Drive Move_To; an independent original Is_Moving call asserts its
admission before drawing. This prevents a cold zero deploy field from silently
turning a moving control into a stationary one.

Four `attack_production_cases` replay captured stationary/moving Attack inputs
and their unselected controls through the existing Tactical batch loop. The
matched release receipts supply source1374 and target1386 MTNK coordinates,
selection, TarCom/NavCom, timer frames and raw opt-in `action_line_inputs`.
Profile, receipt, BGRA frame and VERA release-build hashes are pinned. The prepared
native current-speed queries reproduce source17/target0 for the stationary
pair and source5/target17 for the moving pair. The source's latter applied
fraction is exactly19661/65536; Housef32 and cratef64 inputs retain their bits.
Original Drive Move_To and IsMoving establish both captured motion flags.

The native stationary line stores188 red pixels; the moving line stores101,
with led aim `[8233,23679,416]`. Both controls call no Foot drawing and store
no pixels. A192x160 crop at production origin `[176,336]` fits the existing
guarded64KiB surface reservation. Native camera `[-1924,1747]` follows the
previously established camera-plus-crop-minus15Y convention. Every original
endpoint rectangle and solid segment remains unclipped. Existing
[`rally_production.py`](rally_production.py) owns the separate receipt/archive
and production pixel comparison; native background is only a write sentinel.
All320 earlier rows and every earlier top-level payload value are preserved.

The source-basis boundary is explicit in `attack_production_reference`.
Moving actors occupy Cells29,92 and33,92, which were not among the observed
terrain cells. Exact physicalZ416, zero GTNK TurretOffset and no rocking owner
are supplied. The observed Cell34,92 fields belong to the competing NavCom or
unused fallback. This packet does not label the moving cells capture-proven
flat or reproduce their terrain/track history.

The equivalent zero-offset origin is supported by the existing native slope
owner: all21 initialized slope matrices have zero translation. Twenty-one
stable original pivot controls retain the source origin. One active timer
control also enters Drive's zero-angle translation arithmetic with supplied
half sizes15.5/20.5 and retains that origin. Its equal source/destination slope
uses original755A40; differing-slope quaternion conversion is instruction
evidence (`646A39/646A41/646A49` clear translation). This establishes the
origin boundary, not general matrix parity, a VXL reader or nonzero rocking.
The same drawing purity guard covers direct Foot and Tactical replay without
duplicating the surface, rasterizer or actor/Drive ownership.

The prerequisite mode executes original source/target Unit constructors and
Unlimbo, then real enemy-object query `73FD50 ->4DDED0 ->6FFEC0` with no modifier
keys. Actual Unit/Techno FireError returns an admissible result and the query
returns Attack5. This required two existing prerequisites absent from the
earlier Cell-target Mission fixture: original Object scalar initialization
(`8141D8` via `object_flight_height.initialize_object_scalars`) and physical
layered `[ElevationModel]` reader `66D150`. Cold Object constants made the
target appear airborne; the missing elevation divisor later faulted at
`6F705A`. Those exploratory failures remain in ignored research logs and are
not goldens or alternate gameplay behavior. Existing OS key transport supplies
GetKeyState SHORT0 at the import boundary; no action result is substituted.

The admitted query is followed by original Attack Event construction/delivery,
QueueMission, AssignTarget and Unit destination setter. The retained target
then runs real Move destination, fraction, Facing and veteran setters before
each `70BCB0` query. Separate cases execute a missing current-weapon control,
real Stop, and concrete pointer expiry with supplied dead-target state. The
last boundary establishes target clearing and passive-scan RNG/timer effects;
it does not execute damage or the target's entire destruction/retirement loop.
Event delivery does not restart the action-line timer; the already-established
Display caller owns that restart. Whole mouse picking, event scheduling and
Scenario load remain outside the composition.

Two full original `6FDD50` controls distinguish its explicit Cell argument from
TarCom. The first launches using the argument distance/speed, then aims using a
different moving veteran Unit TarCom. The second retains the same valid
argument after Stop; `70BCB0` returns zero, and later bullet placement fails.
It must not be described as FireAt refusing a null TarCom at entry. The packet
records original calls, bullet origin/velocity bits, and RNG per mutation step;
direct aim reads leave actor/Drive/RNG unchanged. FireAt animation and concrete
expiry may consume Scenario RNG independently of that read-only query.

Original UnitType construction and exact ART `TurretOffset` reader establish
default0 and physical GTNK omission0; retained/default/exact-case/wrap controls
are executable. The original AP constructor and full layered Warhead reader
establish `Rocker=false`, `DirectRocker=false` for the physical105mm/AP duel.
This is why incoming AP fire does not require the broader Rocker producer for
this bounded common path. It does not establish the other warheads, positive
rocking angles, slopes, airborne targets, planning, force-attack Cell input or
multiturret selection. Production output/performance validation remains the
implementation owner's separate evidence.

`ActionFixture` accepts optional `input.surface_size`; `Rally` owns the dimensions
and initializes the same original BSurface/ABuffer storage. The default remains
160x120. A160x160 production crop fits the existing64KiB reservations for each
plane, including the32-byte end guard; nonpositive/noninteger dimensions and
larger byte counts are rejected before VM initialization. No plane is relocated
or duplicated. Production rows declare their crop separately from the original
default-size corpus.

## Native route and ownership

After the second rally call at `6D46CF`, Tactical's forward Techno-array loop
reaches `6D470D..6D4756`. `House50B6F0`, cached `Planning637AA0`, selected byte
`+83`, and `UnitActionLines` global `843108` gate virtual `+438(0,0)`. There is
no alive or limbo test in this block. Non-campaign House admission is pointer
identity with the current player; campaign admission reads House `+1EC/+1ED`.
This is `Simulation::house_is_human_player`, not the distinct `50B730` predicate.

The actual Unit vtable `7F5C70+438` resolves to `Foot4DC060`. With Target `+2B4`
null, a nonnull NavCom `+5A4` is required even if the queue is nonempty. The source
is raw Location `+9C/+A0/+A4`; the attack-only `+300 ->6F3D60` turret anchor and
`70BCB0` lead are outside this route. The endpoint is NavCom when queue count
`+598` is zero, otherwise the last Cell in the `+58C` vector. Original Cell
`486840` supplies its center and terrain height. Only after original map-diamond
admission `568300`, cell lookup `565730` and flag `0x100` does Foot replace the
endpoint Z with `578080` ground height plus its own initialized bridge offset
`8B3DF4`. The original initializer chain in the payload produces `104` and
`416`; Rally's different bridge global is not substituted for this producer.

Move color uses index3 of the original global Convert `87F6C4` middle lookup row.
The physical palette bytes, archive identities and full converted row are pinned.
For the stock palette this is packed `0x0540`, passed as native RGB `[0,168,0]`.
`7049C0` projects both endpoints, draws a3x3 box at each point minus2, intersects
through `421B60`, fills through `7BB020`, then draws through `7BA5E0 ->7BA610`
and shared `7BC2B0` clipping. Ordinary solid drawing does not consume A or Z.
Horizontal/vertical solid leaves include both endpoints; diagonal leaves loop
exactly the major-axis distance, so omit the final point before the separately
drawn endpoint box. Secondary stepping requires strictly positive error.

Existing owners to extend are `render::surface_line` (shared solid/clipping),
`util::rect::clip_rect`, `app::presentation::target_lines`, the native coordinate/terrain
owners, `CdTimer`, the application target-line state and `Simulation`'s class
destination setter. The old axis-only radar viewport consumer must share the
solid owner. Existing raw physical Location remains simulation authority.

## Timer and input prerequisites

Original `70D150` starts the global timer at binary frame `A8ED84` for25 ticks.
`4DC089..4DC0B3` uses signed wrapping subtraction/comparison and treats start
`0xFFFFFFFF` as the native sentinel. `6F2AB0..6F2AC9` initializes start from the
current frame and duration0; the test stops before unrelated CRT atexit setup.
The observed middle dword is stack carry, not a timer field to reproduce.

Successful-load slice `685167..6851A7` evaluates the retained global timer
against the restored current frame, stores the remaining duration (or0 when
expired), and reanchors start to that frame. A sentinel start preserves duration
before reanchoring. The slice and its -4 stack delta execute directly.

The complete ordinary global writer `67F7E0..67F9BC` and reader
`67F9C0..67FD17` stream named fields and vectors. Their third dword is binary
frame `A8ED84`; no streamed range overlaps timer `B0EA80..B0EA8B`. Their only
delegated globals helpers `539890..539AD9` and `539AE0..539E9C` stream named
`A9FA`/`827F` fields and vectors, also excluding the timer. The saved caller
instructions establish `67D421 ->67F7E0`, `67E8B5 ->67F9C0`, and outer load
`67E659 ->67E730` before `67E685 ->685120` postload. Existing
[`load_timers.py`](../projectile_oracle/load_timers.py) independently executes
the global reader through its third read and demonstrates frame restoration.

These original stream-layout instructions plus the literal timer-address scan
support retaining process-global action-line timer state across the ordinary
global save/load route, then reanchoring it; there is no action-timer field in
that stream layout to serialize. This is instruction-level persistence evidence.
It does not execute the whole save/load, prove arbitrary computed aliases, or
establish object reconstruction and malformed-save behavior. Literal scans also
find no additional scenario-reset writer for the three timer dwords.

Display's ordinary click path runs `4AE750` then unconditionally `70D150` at
`4ABFAE`, without testing whether dispatch produced a command. Move1/NoMove2
pass planning helpers `639040/639130`, both true when planning is inactive.
Normal Select7 has its own restart at `4ABE83` and bypasses that dispatcher;
local ToggleSelect8 can take the dispatcher path. Band release restarts at
`4ABCF0` before its empty-result bail. The corpus pins these instruction ranges;
it does not execute the full UI dispatcher or generate selection events.

The Move event calls AssignTarget at `4C7467`, pushes literal clear-queue1 at
`4C746F`, resolves the destination and calls virtual `+480` at `4C747C`.
`Unit741970` compares the requested NavCom first (`741A80..741A90`); same target
with force byte `+1F8` false returns before queue or timer writes. Existing
`track_destination` supplies this native class/Foot/Drive behavior. Composed
rows prove fresh, repeated, forced, queue-clear/preserve, null and bridge results
then execute action-line drawing in the same VM. Source/head/destination,
NavCom/queue and both movement timers are recorded independently of pixels.
`skip_move` is an explicit synthetic native prestated control, not a claimed
stock command state producer.

The Techno array appends at `6F31BD..6F31D2`; `6F442E..6F44E6` retains an existing
entry or appends if absent. Teardown `6F461C..6F463B` finds then removes the entry;
`63F000` shifts later entries left. Monotonic stable IDs preserve this relative
order when corresponding constructions occur in the same order. Limbo objects
remain in the array. Whole save-load reconstruction ordering is not established.

## Drawing order and finite batch comparison

Tactical calls object drawing `6D8DB0` at `6D465F`. The ordinary Unit route calls
virtual `+104` at `6D909E`, resolving through the physical Unit vtable to body
`73B0B0`. A later visible-Techno sweep calls virtual `+110` at `6D9789`, resolving
to Extras `6F5190`. Extras calls rank virtual `+454` at `6F5382`, then selected
Unit health/brackets virtual `+44C` at `6F5E27`. The health body `6F64A0` reaches
pip virtual `+450` at `6F6AB0`. These finish before object drawing returns at
`6D97C5`, hence before Tactical's combat effects, bandbox `6DA180`, second rally
and selected action lines. Existing Unit status/rank/pip consumers must retain
that order relative to an overlapping source endpoint; their pixel bodies are
not executed by this corpus. The `6DAD60` calls surrounding object drawing are
planning path overlays, not range rings. Selected-building range behavior is
outside this ordinary MTNK route.

GScreen `4F4480` calls its display-chain draw virtual at `4F4502` before Tactical
pass2 at `4F4515`. MouseClass `6D0A20` ends by calling Sidebar `6A6C30` at
`6D0E38`. Later GScreen draws gadgets/tooltip, then WWMouse virtual `+3C` at
`4F4593`. The original WWMouse constructor `7B8730` installs derived vtable
`7F7B2C` at `7B8850`; its `+3C=7B90C0` captures background and calls cursor draw
`7B93F0`, whereas `+40=7B92D0` restores background. WWMouse global `887640` is a
different object from the display-chain `this`; neither slot is MouseClass
`6D0A20`. Original instruction ranges and physical vtable entries preserve
these ordering facts. This is not an executed sidebar/cursor comparison.

Five batch rows execute the entire original `6D46DD..6D4912` local Techno loop,
including planning, House admission and all admitted Foot producers. They cover
32,33,64 selected flat actors sharing a target, then64 with four Cell targets
and reversed Techno order. Source coordinates, target Cells, actor array,
separate Techno order and observed native call order are explicit. Health100,
local ownership, planning off, empty Target/queue and null capture/temporal fields
are prepared common MTNK inputs. The two reversed all-green batches necessarily
leave the same pixels; their observed producer order differs.

Six separate compositing rows call original `7049C0` with2,33,64 supplied XYZ
pairs, alternating native Convert indices3/8 and reversed orders. Existing
`house_color.house_color` executes original `50B840` to unpack the unchanged
Convert row at each requested index: index3 gives `[0,168,0]`/`0x0540`, index8
gives `[168,0,0]`/`0xA800`. All three reversed pairs have different final pixel
hashes. These are bounded opaque overwrite comparisons for direct and dense
consumers, not Attack source/lead or attack-admission evidence. No rasterizer,
palette conversion or projection is copied into the generator.

## Captured production inputs

`production_cases` replays twelve final surfaces from the sealed Move gesture
captures. Each row uses the same complete prepared Tactical loop with one actor,
then the original Foot timer/navigation and drawing bodies. It records ordinary
`input` fields, `size:[160,160]`, original `pixels`/`words`, and `production` with
run name, actor1374, crop origin, capture/profile/frame/executable SHA256s.
The generator contains frozen input literals and does not need ignored capture
directories. The separate production comparator must bind those literals to the
retained receipts and compare actual frame pixels; the native output itself does
not certify gesture execution or a whole native Scenario.

All runs use captured camera `[-2100,1426]`, zoom1, tactical632x568 inside800x600,
and initialized stock `UnitActionLines` defaults. The ordinary Move destination
is Cell35,94; empty-band restart emits an actual new Move to29,96. Main crop origin
is `[176,336]`, giving native camera `[-1924,1747]`; the westward empty-band route
uses origin `[80,336]` and camera `[-2020,1747]`. Both endpoint boxes and every
segment remain within their crop, checked against original clipping receipts.
Native background is a write-mask sentinel; no production background is invented.

Physical `multimd.mix` entry `0xCD0DDEF2` supplies `XMP03T4.MAP`,156269 bytes,
SHA256 `7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`.
Its `[Map] Size=0,0,80,85` and `LocalSize=5,5,70,73` match the saved
[`anytown native map packet`](../spatial_oracle/anytown_damage/next_family_native.json.gz),
whose original normalization result supplies the80x85 native Map fields. The
physical archive/file identities, literal strings and prior packet hash are
preserved under `production_reference`.

Captured source coordinates and target level4/slope0/flags0 are prepared inputs.
The initial flat grounded MTNK position and later retained exact Z agree between
the receipt's `position_world_coord` and rendering's `object_location`; this
does not establish other legacy height fallbacks. Empty Nav queue is the explicit
default plus ordinary nonshift Move/clear-queue1 boundary, with no queued-input
producer in these sealed profiles; the receipt does not serialize the queue.
For null NavCom, target35,94 is explicitly unused fixture data.

The rows cover active6, selected-only6, before-Stop7, stopped7, last-active29,
expired30, expired-next31, reselect31, unselected-empty-band5, empty-band-restart31,
arrived-control95 and arrived-reselect95. Stop clears NavCom while the timer is
still positive; arrival leaves it null even after reselect restarts the timer.
The arrival pair's tactical images agree; a sidebar hover tooltip differs after
the reselect input, so a whole-frame identity claim would be incorrect.

The optional-size change independently replayed the default Rally, shroud and
action-line corpora before refreshing their recorded source hashes. Rally's
payload stayed SHA256 `31820ffa392c4f23a0ef8fa6e97246ae1eb8f485145a9bae1e5c0a66310f67fb`;
shroud stayed `073a8d284e93e48fac78f116d2423c7ad9f5022935c758edf28c097a3522a878`.
The previous277-row action packet was
`045034052235bc531aef6d848d0866892984540f0bac34b1746e2fea616ee668`; all21 existing
top-level values are retained while the production rows/reference are appended.

## Payload and coverage

- `cases`:103 full Foot producer rows. First57 promote the preserved research
  input objects and pixel/surface hashes unchanged. Additional rows cover three
  levels, all16 active slopes, deck `0x100`, source subcell/Z, out-of-map deck
  rejection, null NavCom, queue override and last-queued-Cell bridge input.
- `leaf_cases`:77 original solid-leaf rows across octants, reversals, axes,
  coincident points and viewport/interior clipping.
- `clip_rect_cases`:45 original rectangle intersections including3x3 endpoints,
  empty/negative source sizes, empty clips and optional offset adjustment.
- `timer_prerequisites`:3 static-default and8 successful-load slice controls;
  the original57 also include25 restart/expiry/signed/unsigned-wrap controls.
- `tactical_gate_cases`:20 prepared owner/selection/option/planning/campaign
  controls executing the original gate and selected virtual when admitted.
- `destination_cases`:10 composed original Unit setter and Foot drawing rows.
- `batch_cases`:5 full prepared Tactical loop rows at32/33/64 actors, shared
  and multiple target Cells, with explicit source array and native call order.
- `compositing_cases`:6 ordered original7049C0 rows at2/33/64 calls, with two
  native colors and forward/reversed control pairs.
- `compositing_color_controls`: original House color unpack at supplied lookup
  indices3/8 of the unchanged physical-palette Convert row.
- `ordering_vtables`: physical Unit and WWMouse entries used by saved callers.
- `production_cases`:12 capture-bound single-actor Tactical/Foot replays on the
  optional160x160 surface; `production_reference` records physical map identity,
  crop/camera convention and the prepared-input limitations.
- `instruction_evidence`: original bytes/disassembly for caller, leaves, timer,
  option, event, same-destination admission, stable array ordering, and complete
  ordinary/delegated global stream layouts plus save/load/postload callers;
  Unit body/Extras and outer tactical/sidebar/cursor ordering ranges.

Drawing rows contain `input`, named original `calls`, packed `pixels` triples
`[x,y,RGB565]`, `pixel_count`, `full_surface_sha256`, guards and palette integrity.
`action_line` calls expose source/target coordinates and literal call arguments.
Direct rectangles contain `rectangle` and `offset`. Timer rows record
`native_timer`; destination rows record `setter_result`. Physical palette input
for production Rust tests is `physical_inputs.palettes['PALETTE.PAL'].hex`.
Batch rows add `actor_call_order` and `action_calls` with `actor_index` and
native coordinates/arguments. Their inputs separate `actors` from `techno_order`.
Compositing rows instead carry `input.primitives`, `primitive_order` and
`action_flags`; observed action calls identify each `primitive_index`.

The promoted source corpus is preserved under ignored
`logs/procedural-drawing/selected-unit-move-line-expanded.*`; its JSON SHA256 is
`529e2d2ca8c6bd1fc065e6569b38a56aebd01f731fb26579551c67e6a7d9c1e9`.
The new generator has no runtime dependency on those ignored files.

## Limits and residuals

Actor, target, camera, map state and Tactical gates are prepared; there is no
whole MTNK construction/placement, mouse picking, event execution, pathfinding,
locomotor progress, reveal history or native full-frame capture in this corpus.
Physical MTNK/GTNK strings are context, not a new complete rules reader. Existing
rules, map and input owners must establish production-supplied prerequisites.
Ground/slope/bridge samples do not certify all coordinates or integer overflow.

Known clipping chop53 versus host-nearest f64 can shift a clipped endpoint by1px;
this presentation residual is retained, not fitted away. Attack lead/FLH,
planning preview, aircraft/jumpjet and special Move consumers are later chains.
No known RNG entry is reached during the observed ordinary drawing/setter phase;
fixture RNG construction occurs before that phase. Full command RNG, detach and
mission/terrain lifecycle are not established by these prepared drawing rows.

Native `--write` plus independent `--check` establishes reproducibility. Rust
comparisons, production output and performance must be reported separately by
the implementation owner; this document makes no whole-chain completion claim.
