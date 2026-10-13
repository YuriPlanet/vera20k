# Original IFV guided-step boundary controls

`ifv_guided_controls.py` executes 31 controls through the original
`0x4668BD..0x467B7A` Bullet AI body, including reached target getters,
`HomingTrack(0x5B20F0)`, floor queries and contact branches. The executable hash,
substitutions and coverage limits are recorded in the companion metadata.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
~/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_guided_controls.py --check
```

The asset root must contain the physical INIs and assets used by
`guided_step.create`; their hashes appear under `initial`. `--check` executes
again and compares the saved payload without writing. `--write` explicitly
regenerates both JSON and metadata. No temporary research script is imported.

Each `rows[]` entry has a stable `input.name`. `input` records supplied overrides;
`prepared` gives the retained native type/rules values and explicit fixture
coordinates, velocity, frame, identity and target kind. Initial latch/counter,
closing count and closing accumulator default to false, zero, zero and zero
unless present in `input`. Aircraft coordinates are `[4224,5248,1300]`.

Native outputs are `candidate`, `velocity.value`/`velocity.bits`, `mode`, `impact`,
`locked`, `counter`, `closing_count` and `closing_bits`. Velocity bit strings are
numeric 64-bit hexadecimal words; closing/rules bit strings are little-endian
byte hex. `events` preserves query order and HomingTrack target/direction/flags;
the four flags are Aircraft, Airburst, VeryHigh and Level. `snapshots` records
selected original program counters and native target/old coordinate locals.

Controls cover zero/fractional/negative/overspeed velocities, signed maximum
speed and acceleration, lock latch/counter/duration boundaries, closing cadence,
Level/Airburst/VeryHigh, null targets at safety heights 499/500/501, and an
Aircraft-shaped target. Type overrides pass through the full original reader.
No RNG call is admitted in this step; observation rejects any unexpected draw.

These are supplied incoming states, not a launch or aircraft lifecycle proof.
The original constructor/configure and physical reader setup are reused from
`guided_step`; that setup is not full Rules Process chronology. Cells are flat
level 6, source is null, and Aircraft state uses its original vtable with supplied
coordinates. The endpoint precedes world coordinate commit. Damage, scheduler,
Display admission and drawing are outside this corpus. All expectations come
from original execution with x87 control word `0x0E7F`, not Rust calculations.
