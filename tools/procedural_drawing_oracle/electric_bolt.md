# Ordinary Tesla electric bolt

`electric_bolt.py` executes the original YR image. It extends the
existing Building, cached-INI, palette and RGB565 surface fixtures. Python
records inputs and native outputs; it contains no bolt subdivision, RNG,
projection, Spark, line raster or light-mask implementation.

From the repository root with the configured retail installation:

```sh
python -m tools.procedural_drawing_oracle.electric_bolt --check
```

`--write` regenerates the corpus and provenance sidecar. Changes to those native
references require review. The sidecar records the producer
sources, command, original entry points and supplied boundaries.

## Corpus boundaries

| Group | Original execution and supplied inputs |
| --- | --- |
| `type_inputs` | Existing BuildingType constructor and physical TESLA/NATSLA and GAPOWR rules/ART reader slices: Image, foundation, FLH, pixel offsets, target offset and relevant flags. |
| `weapon_inputs` | Original CoilBolt constructor/readers and Invisible reader, physical RULESMD, optional LANGRULE, MPBattleMD and XMP03T4 strings. The prepared scene relocates the reader-produced projectile pointer. |
| `birth_cases` | FireAt `6FF4CC..6FF656`, real selector/GetWeapon/GetFLH, Create `6FD460`, EBolt ctor and Init. ParticleSystem construction is an explicitly recorded boundary here. Effective flags, slots, FLH, foundation and native seed are recorded, including two-slot reselection, laser precedence, electric gate and allocation controls. |
| `spark_cases` | The original birth's endpoints feed original EBolt Init in the cached-reader/flat-world fixture. The real Spark system constructor, native ID producer, Unlimbo, DisplaySubmit, LogicRegister, whole system/particle AI and checksum run. Two visits of the original live Logic object loop execute. Detail/size/one-frame-light and wrapping-ID controls are explicit. |
| `live_logic_cases` | Actual physical Invisible Bullet constructor/configure/Fire precedes EBolt/Spark construction. Whole original Bullet AI, impact, AreaDamage against empty receivers, physical TSTIMPCT construction/AI, unregister/compaction, deferred Bullet destruction and two live Logic passes execute. A second row supplies physical GAPOWR contact fields and cell occupancy before Spark AI, then executes child/system retirement and deferred destruction. The source Building AI, placement, populated damage receiver and complete FireAt caller are outside the component. |
| `charge_logic_cases` | Physical NATSLA_B/NATSLA_BD constructor and uninterrupted original AI at supplied birth frame9 or10 precede Bullet/EBolt/Spark admission at36. The same live loop executes charge expiry/unregistration, Bullet and Spark in the resulting order, with a deferred drain between passes. Source Building AI and its expiry listener are outside this component; generic delayed-fire Active3 restoration has separate `building_slot_replacement.expiry` evidence. |
| `draw_cases` | Original retained EBolt manager, clip gate, subdivision and packed line primitive. Draw calls retain destination/RNG/bolt state across visits. Controls include ordinary retail endpoints, offscreen aging, zero length, zero/last decay, same-frame repeated visits, reverse insertion order, signed phase, length thresholds and subdivision depth. |
| `mixed_cases` | Original Laser manager, EBolt manager and LineTrail manager draw into one destination, in native Tactical order. Original Trail ctor/update produces the ring from two supplied owner positions. Every primitive argument and each family boundary's destination hash is retained. |
| `light_indices` | Original light draw prefix runs through projection, gates and signed index arithmetic; execution stops before the unchecked array lookup at `5FF93F`. Oversized controls establish arithmetic only. |
| `light_admission` | Original reverse light manager and the complete draw admission prefix run with explicit FPS, minimum, hysteresis buffer/latch, flags and coordinates. The admitted path redirects to the original epilogue at `5FF8B8`, before the mask body; entries record every threshold query and latch transition in native order. |
| `light_draw_cases` | Original `5FF420` constructs the fifteen mask slots reached by stock Spark Size15. Original reverse light manager and complete RGB565 light drawing run for each persistent stage against the shared surface. Each stage begins from the recorded background. |

The ordinary Building fixture has TESLA at `[9856,12416,0]` and GAPOWR at
`[11392,12416,0]`, with a 2×2 foundation. The executed getters produce source
`[9778,12288,300]`, target `[11520,12544,0]`, and depth adjustment `-41`.
The source retains the attacked building as its target. Native Building fire
facing `445E50` uses that target for a non-turret building; the explicitly
cleared-target control instead uses its zero constructor facing.
`SpawnElectricBoltEffect` re-runs the selector: the weapon that admitted the
FireAt electric arm supplies the gate, while the newly selected weapon supplies
FLH and alternate color. The ordinary path selects slot0. Its bolt has no
attached owner, native Abstract ID or Scenario RNG draw.

The shared Rust GetFLH source differs by one lepton in X (less than one pixel);
`caller_coordinate_residual` explicitly supplies `[9777,12288,300]` to original
constructor/Init/draw to bound downstream rendering and Main RNG, without
claiming upstream coordinate equality.

Init consumes Main `RandomRanged(0,256)` before constructing exactly one default
Spark system. The fixture retains all three original RNG objects, requested
bounds, raw draws (including rejection), results and full before/after bytes.
Every tactical visit also records the supplied Scenario native-ID cursor before
and after drawing, retirement or clear. A future Bullet constructor is outside
this producer; its identity consumption has its existing projectile evidence.
Spark constructors use the Scenario stream. Native system and particle IDs are
separate from container position: `630100` feeds the system's ID through
`5F6250→410410`, and every owned particle ID through `630135`. The recorded
checksum calls establish those stored-ID consumers. Native Save methods
`630090` and `62D810` call `410320`, which writes the object pointer token and
raw object bytes including ID; whole native save/load is not executed here.

`spark_cases` starts with only Spark in Logic. Its first visit therefore is the
system's first actual AI call, not the ordinary firing-frame schedule.
`live_logic_cases[0]` supplies the Bullet predecessor: FireAt calls BulletFire
at `6FF014` before electric Spawn at `6FF58A`. Original BulletFire admits the
Inviso Bullet before EBolt Init admits Spark. The native live loop increments
its cursor at `55B616`; unregister `55BAE0` shifts following entries left.
In the recorded firing-frame pass, Bullet1001 creates TSTIMPCT1003, removes
itself, and the loop visits the Anim now at index1. Spark1002 has moved to index0
and receives no AI call: lifetime remains `-1`, spawn frames1, no children.

The next pass visits Spark1002 and TSTIMPCT1003. Spark reaches lifetime `-2`,
spawn frames0 and four children1004..1007 with this row's explicit seeds.
Its child count differs from the isolated row because Bullet impact first
consumed three Scenario requests, including the cluster continuation after
Cluster1. Full three-stream states, native cursor, ordered visits and actual
requests are recorded around every boundary. Real deferred destruction runs
between passes and consumes no RNG in this case. TSTIMPCT's original ART/SHP
reader yields fourteen frames, rate1 and Normalized=true.

The joined row uses a legal supplied 64×64 map rectangle, flat cells38..48 by
46..52, a stationary 2×2 Building coordinate receiver, null firer and empty
damage receiver lists. Original getters, impact, animation, retirement and
Spark bodies run without gameplay substitutions. It establishes their joined
ordering and numeric continuation for those inputs; it does not establish the
omitted Building damage receiver, source AI, whole-shot RNG prefix or complete
Windows frame scheduler.

`inviso_bullet_before_spark_occupied_gapowr` adds four supplied ground-cell
object links immediately before the second pass, after Bullet impact. Physical
GAPOWR `LaserFence` and `UndeploysInto` readers retain false/null; its native
foundation is 2×2. The real `Cell47C520` lookup and Building `457620/465D40`
gate accept it for all four Spark candidates, whose Z values are35,41,29,30.
Original `62C6E0` marks every child for deletion and clamps its Z to ground.
Backward cleanup queues children1007,1006,1005,1004; done-and-empty system1002
then leaves Logic and enters the same queue. The native drain clears it without
RNG. The persistent stage0/size15 light was created before collision and remains.
This controls contact and cleanup without claiming occupied-world AreaDamage
or native Building placement. `systems`/`particles` retain observed allocations
for inspection; their presence after a drain does not indicate live membership.
The retired system's checksum is explicitly skipped.

The charge animation is an earlier Logic entry in an ordinary Tesla shot.
`charge_logic_cases` establishes that its expiry can reverse the Bullet/Spark
ordering above. Original AnimType readers consume physical `NATSLA_B` ART and
`NGTSLA_B.SHP`: rate3, start0, end9, and Normalized=false. Original Anim
construction uses Building451970's delay0, one loop and flags0x1600; the supplied
Building marker at Anim+118 reproduces the store at45199B. Birth at frame9 and
27 uninterrupted live passes9..35 produce frame8 with timer start33/duration3.
No animation frame, timer result, completion or unregister result is supplied.

At frame36 the actual vector is `[charge1002, Bullet1003, Spark1004]`.
Charge AI writes normal completion at424B31 and runs UnInit4255B0,
AnnounceExpiry7258D0 and LogicUnregister55BAE0. The vector becomes
`[Bullet1003, Spark1004]`; the unconditional cursor increment reaches Spark at
index1. Bullet has no AI visit that frame, so it remains alive with no impact
Anim. Spark creates its light and five children for this seed. Frame37 visits
Bullet, creates TSTIMPCT1010 and removes Bullet; Spark is skipped on that pass.
The physical charge's own AnimType construction spends native ID1001 before
these objects; this is not a whole-Scenario ID sequence.

The paired birth-frame10 control reaches frame36 still alive. Its loop visits
charge, Bullet and TSTIMPCT1005, skipping Spark after Bullet compaction. Charge
expires at37 and its removal skips Spark again. The third row uses damaged
`NATSLA_BD` at birth9: its original reader yields Start10/End19/Rate3, but the
original constructor starts current frame0. It remains live at frame9 during
both observed passes36/37. Bullet impacts at36 and Spark receives first AI at37.
Thus neither same-frame damage nor a retained unvisited Spark system is a
universal bolt-birth invariant.
RNG histories belong to the supplied component inputs and omit the whole
FireAt/Building prefix. The target is unoccupied and takes no damage. These
rows do not execute the Building expiry listener that restores Active3; its
original generic delayed-fire branch is separately covered by
`building_slot_replacement.expiry.json` cases `delayed_fire_normal`,
`delayed_fire_damaged` and the garrison controls.

At detail2 the first successful ordinary Spark burst creates a persistent
SpotLight at the target with stage0, size15 and flags0. It consumes no native ID
or RNG. The creation site is inside the admitted burst and additionally checks
current SpawnFrames against the type value, positive signed LightSize and
`OneFrameLight=false`. The next Logic visit creates no second persistent light.
Original UpdateAll advances stages by8 and removes the light at80. Draw uses a
signed32 wrapping multiplication and truncating division by64; there is no byte
narrowing or range clamp before its native surface lookup. Only stock size15
indices are rendered by this corpus.

Light drawing calls the existing minimum-FPS owner once per reverse-ordered
light when none of flags bits1..3 is set. Bit0 alone still queries and can be
rejected; bit1 bypasses that query. The corpus includes repeated queries with
unsigned threshold overflow, preserving the shared hysteresis latch. The
optional scenario-flag `0x1000` fog call executes original `5865E0`, whose active
retail body is `XOR AL,AL; RET4`; it cannot reject. It is not a FogState test.

## Drawing and lifecycle details

The physical startup palette conversion yields ordinary index10, alternate
index5 and center index15 through the original converter's `+174` packed table.
The RGB565 words and complete table are stored in `palette`; they are not
reconstructed from conventional white or blue constants.

The manager walks the bolt registry backwards. For nonzero decay it projects
and tests clipping, passes the original raw endpoints to subdivision, then
increments phase even when clipping rejects or length is zero. It arithmetically
shifts decay and deletes on zero. A zero initial decay skips projection and phase
advancement. Clear controls execute the original scene-clear call at
`53493F→4C29E0`, including forward deletion and registry reset.

Subdivision `4C1F20` uses the original explicit stack and emits strand1,
strand2, then center. In particular, the sine result is multiplied by the saved
initial displacement at `4C2151` before the integer conversion; a decompile that
omits that x87 multiply is insufficient evidence. The corpus executes original
distance, trigonometry, float conversion, projection, clipping and `4BFD30`.
Each visit records packed arguments, sparse changed pixels, full destination
hash, immutable Z/alpha, guards and text-image checks.

These are executable component comparisons. They do not claim a complete
Scenario loader, original executable whole-shot run, Windows frame scheduler,
audio scheduler, complete save/load, attached infantry bolts, overpowered coils,
movement or aircraft parity. The production integration and its receiver,
Spark drawing, transient light and shared GPU composition remain separate
consumers of these native bounds.
