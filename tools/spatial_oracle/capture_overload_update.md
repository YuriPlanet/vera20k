# CaptureManager overload components

`capture_overload_update.py` executes original `gamemd.exe` instructions. The
default mode preserves the historical 22-row component corpus and its declared
callback substitutions. Its Rust consumer checks countdown, damage, voice,
spark inputs, Scenario continuation and the final sideways-rate word for live
controllers. Neither side runs the shared rocking updater in those controls;
native death callbacks remain stubbed and do not prove whole death behavior.

## Joined stock component

`--joined` adds five controls consumed by
`sim::capture_manager::tests::native_joined_overload_preserves_constructor_rng_ids_and_rate_words`.
It reuses `building_death_anims`'s physical reader and joined Unit runtime on
the same VM, with `electric_bolt.read_spark_types`, `SparkBirth` and `RngTrace`.
It creates no second emulator, INI parser or particle implementation.

Original MIND UnitType/weapon/projectile/warhead readers run in RULESMD,
optional LANGRULE, Battle and Hills.map order. Original C4 key read
`66C304..66C34C` binds Super, and `66C7A1..66C914` reads the three overload
tables through the native int-vector reader. The physical sound list and
AudioVisual reader bind the overload voice. Full native MIND constructor
`7353C0`, its CaptureManager constructor `4717D0`, and Unit Unlimbo `737BA0`
run before the comparison boundary. Native MIND VXL file loading is outside
this component; the direct rate writer has no loaded-model gate.

The **captive count alone** is supplied after construction: 3, 4 or 7. No native
captures or fake node objects are supplied or claimed. Thirty-one manager-only
visits execute the actual countdown and one check. Counts 4/7 execute full
`471A50`, actual nonlethal Unit/Techno damage `737C90`/`701900`, original
`7509E0`, five complete SparkSys constructors `62DC50`, ID assignment and
Unlimbo/Logic admission. Stock 500-HP MIND survives. A lethal row with invented
nodes would make native FreeAll invalid and is deliberately excluded; real
linked fatal release belongs to the production world regression.

The two seed controls for each damaging count cover both signs of native
`471C78`'s direct binary32 rate assignment. The four words
`328/32C/330/334` remain exact, including signed zeros. One full three-stream
before/after pair per case and compact ordered ranged/raw traces retain
rejection words; constructor checkpoints retain native ID and draw order.
The Rust consumer imports the full native states/cursor and compares all raw
words, final streams, actual ordered Spark coordinates/IDs/runtime state,
controller health, manager countdown/latch and all four rocking words.

The selected stock SparkSys constructors draw no RNG at this seam; this does
not establish the same property for other particle types. Other actor,
particle, Techno/Rocking and whole Logic visits are excluded. Original PlayAt
executes with observed raw audio input and branch PCs; inherited camera/device
priors and actual audible output are not native audio equivalence. Native
flash counter+44 is compared by the existing Rust manager consumer at each
retained boundary (including positive decrements on countdown-return visits).
This component excludes pip drawing; the production PIPS2 submission and GPU
readbacks cover that path separately. Capture link lines remain unported. The ordinary production tests own real shots/captures and the following
Techno update; connected release captures own visible MIND output.

## Reproduce

Source the machine's normal native-oracle environment and set
`PYTHONPATH=.:tools/spatial_oracle` and `PYTHONDONTWRITEBYTECODE=1`. Supply the
existing physical joined input directory through
`VERA20K_BUILDING_DEATH_ASSETS`; the Drone joined component's extracted inputs
already include the required INIs and inherited assets. No additional model
extraction is required for this component.

```sh
python -m tools.spatial_oracle.capture_overload_update --joined --check
python -m tools.spatial_oracle.capture_overload_update --check
```

Use `--joined --write` deliberately to regenerate the additive corpus after
reviewing the changed native inputs. The sidecar records the original binary
hash, replay command, boundaries and payload hash; no repository source hashes
are pinned. Run validation through the root-owned build/test workflow. These
components establish their recorded outputs, not complete MindControl or
BulletClass parity.

## Techno AI ordering component

`--techno-order` adds a separate small corpus consumed by the production
Techno/Mastermind ordering regression. It preserves the historical default and
`--joined` payloads. All controls use the existing `Fixture` or existing
`joined_inputs`/`joined_case` owners; this mode introduces no second emulator,
INI parser, allocator or gameplay implementation.

The original `TechnoClass::AI` body is `6F9E50`. Its prologue establishes
`ESI=this`, `EBP=0` and a 0x78-byte stack frame. The retained component begins
at `6FA646`, through the mission-control increment and actual ordering below,
and ends before `6FA95B`; a killing control stops before epilogue `6FAFFD`.
Earlier Techno work, later continuation, outer Unit AI and whole-world ticks
remain outside this comparison.

| Original instruction | State/call and ordering role |
| --- | --- |
| `6FA64F` / `6FA655` | Increment mission-control `+C4`; call MissionClass AI `5B3060`. |
| `6FA65A..6FA6F5` | Passive timer and current mission gates, then the selected virtual acquisition owner. |
| `6FA6F5..6FA712` | Read attached Bomb `+38`; skip in limbo, test `438A70`, then call `438720`. |
| `6FA717..6FA723` | Reload SlaveManager `+2D8`; non-NULL calls its virtual `+5C`. |
| `6FA726..6FA730` | Reload CaptureManager `+2BC`; non-NULL calls original Update `471A50`. |
| `6FA735..6FA73D` | Read IsAlive `+90`; false branches to the epilogue before self-heal, cloak or Spawn. |
| `6FA743..6FA757` | Virtual self-heal admission `+294`; eligible survivor gains one HP before later repair gates. |
| `6FA941..6FA946` | Call the cloak owner at virtual `+410` with argument zero. |
| `6FA94C..6FA958` | Reload SpawnManager `+2D0`; non-NULL calls its virtual `+5C`. |

These bytes establish that the CaptureManager pointer is re-read after the
Bomb callback. Native fatal damage calls FreeAll at `702112` and `4DE5DD`;
Object UnInit at `5F65F0` defers destruction. A retained manager can therefore
still receive Update with zero captives after a fatal bomb. The IsAlive gate follows CaptureManager's callback.
Death cleanup must consequently be handled by the existing owning mechanisms;
an earlier global CaptureManager visit can create damage, sparks and RNG draws
that the original same-object sequence suppresses.

Five isolated controls execute this entire original bracket and original
`471A50` on `Fixture`'s original-constructed manager. The supplied Guard mission
and expired passive timer reach explicitly returned mission/acquisition
callbacks. Bomb, slave, spawn, type, self-heal admission and cloak are declared
callback boundaries; the corpus does not reproduce their bodies or passive
acquisition RNG. Destructive Bomb callbacks explicitly clear HP/IsAlive and either the
CaptureManager pointer or its captive count; a destructive ReceiveDamage callback subtracts the
actual native damage and clears IsAlive. Real Bomb detonation, damage death,
FreeAll and actual captive nodes belong to the production regression. Ordinary
retail MIND has neither SlaveManager nor SpawnManager; those synthetic slots
only test the common AI order.

The isolated outputs include damage-before-heal `500 -> 450 -> 451`, killing
overload `50 -> 0` with no later heal/cloak/spawn, a synthetic pointer-clear callback that skips
CaptureManager Update, a representative retained-manager/count-zero callback
that runs Update without overload damage, sparks or RNG, and countdown `1 -> 0` followed by a same-visit heal
`499 -> 500`. The Update body owns all retained countdown/flash/latch writes,
11 surviving-overload ranged requests, damage arguments and spark inputs.
Spark allocation/constructors and sound remain the historical declared
callbacks. The lean branch and Scenario RNG instructions still execute.

One additional control reuses the existing physical joined world and actual
stock MIND constructor `7353C0`, CaptureManager constructor `4717D0` and
successful Unlimbo `737BA0`. Count 4 and countdown 0 are supplied at frame 30;
the original constructor's passive delay 45 has not elapsed. Whole original
`6FA646..6FA95B` runs without additional gameplay substitutions, including
actual Mission, CaptureManager, Unit/Techno damage `737C90`/`701900`, self-heal
gates and cloak. HP becomes 450, IsAlive remains true, and all five actual
SparkSys constructors execute. It supplies no captive nodes, capture history,
fatal FreeAll, native MIND VXL load or full-match admission. Its sidecar and
embedded physical hashes retain these exact bounds.

Reproduce with the same physical input set described above:

```sh
python -m tools.spatial_oracle.capture_overload_update --techno-order --check
```

Use `--techno-order --write` only for deliberate new-reference generation. The
sidecar records the immutable executable hash, replay command, callback
boundaries and payload hash, without repository-source hashes. Original byte
reads can be repeated through `tools.native_inspect disasm 0x6FA646 --bytes
0x315` and the full function entry at `0x6F9E50`. Instructions, isolated callback
controls, joined native receiver output and connected production regression
establish different evidence bounds; none certify every TechnoClass function.

The joined row also records the actual ordinary Guard rescheduling draw before
overload: native `4D532F -> 65C7E0`, return/caller `4D5334`, Scenario range
`[0,2]`, raw `2901129938`, result `2`. The first overload offset draw follows at
caller `471B95` with raw `3879883985`, result `9`. The actual damage receiver
records `rng_request=1`, and the row retains 12 ranged requests / 15 raw words.
Thus even a non-expired passive timer does not make an earlier manager-wide
visit equivalent to the original per-object mission-before-overload order.
The killing-capture callback control retains ten spark-offset requests and
five spark callbacks after its destructive damage boundary, then skips the
lean draw; it must not be read as a complete fatal-release reproduction.
