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
