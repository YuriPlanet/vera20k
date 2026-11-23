# Rad Eruption kind2 births

`rad_eruption.py` executes original `TechnoClass::SpawnRadEruption` (`6FD800`)
for two controls and records its eight RadBeam births and the Main RNG object
before and after. `radiation_beam_tests.rs` feeds the same inputs through the
production `radiation_beam::erupted`.

```sh
source "$HOME/Documents/vera20k-dev/env.sh"
export PYTHONPATH=".:tools/spatial_oracle" PYTHONDONTWRITEBYTECODE=1
python -B -m tools.procedural_drawing_oracle.rad_eruption --write
python -B -m tools.procedural_drawing_oracle.rad_eruption --check
```

Supplied: a bare Infantry receiver (vtable `7EB058`, GetCoords `5F65A0` reads
`+0x9C`), sparse cells with level/slope, a fresh dummy, RULESMD RadColor at
`Rules+0x1830`, the caller's ftol spread, and Main/Scenario/Mapgen seeded by
native `Random__Seed` through `RngTrace`. Operator new `7C8E17` is a bump
allocator. Everything else runs unchanged: Map lookup `5657A0`, Cell centre
`480A30`/`47B3A0`, the five Main draws per beam, allocation `659110`,
constructor `6593F0` and the setters.

The FireAt arm (`6FF5D0..6FF5EE`), the CellSpread ftol, drawing and lifetime
are outside this corpus; [rad_beam.md](rad_beam.md) covers kind1 drawing.
