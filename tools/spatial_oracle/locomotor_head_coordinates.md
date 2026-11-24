# Compass and retained head-coordinate evidence

`locomotor_head_coordinates.py` owns the original executable comparison for
Drive/Ship coordinate expressions and compass initialization. Run it from the
checkout with the original `gamemd.exe` available to `tools.native_oracle`:

```sh
PYTHONPATH=. python -m tools.spatial_oracle.locomotor_head_coordinates --check
```

The metadata records original entry points, assumptions, and the
32 Python/Rust sources bound to the comparison.

The four `initializer_controls` execute Cell `49F2F0` through true RET `49F39B`
and Lepton `49F3A0` through true RET `49F413`, each with inherited FPCW `0000`
and supplied FPCW `0E7F`. Fresh fixtures use the existing original Lepton startup.
Only the selected table is poisoned with `A5`; every decoded instruction is
required. The controls record the eight packed pairs at `89F688` (`<hh`) or
`89F6D8` (`<ii`), unchanged FPCW, ESP advancement of four bytes, unchanged
original `.text`, unchanged mapped image outside the selected table, and
unchanged 16-byte guards. Index eight is an unwritten memory boundary, not a
ninth direction or an executed native accessor.

The existing eight `directions` and 288 `cases` remain unchanged. The cases
cover Drive/Ship fresh, second-node, and chain expressions for six supplied
poses, including signed wrapping and distinct retained/current XYZ. They
execute the original coordinate blocks and XYZ copy constructor where reached;
they do not substitute Python expressions for those instructions.

`src/util/direction.rs::DIRECTION_DELTAS` owns the compass vocabulary.
`direction_tables::CELL_DELTAS` exposes it and `LEPTON_DELTAS` derives its values
in leptons. Both existing dump tests now compare against the executable
initializer controls rather than a second hand-written expected table.

The compass cutover removes 18 equal production declarations from Jumpjet
flight, transport unloading, parasite release, sensor-neighbour checks, spawned
child holding, post-C4 Scatter, reservation masks, harvester Guard, miner zone
probes, wall-neighbour updates, starting-object fallback, A* expansion, and
incremental base-zone repair, authored wall initialization, and random-map
grid stepping. Foot-neighbour updates also use the shared values with their
signed-word casts before wrapping. Readers retain their indexing, iteration order,
signed-word casts before addition, wrapping or plain arithmetic, bounds, RNG
draws, timer writes, and callback coordinate rereads. The unused diagonal
Boolean in the two copied pathfinding arrays is removed. No simulation state
or decision owner is added.

Dock exit scans, aircraft exit order, base-site steps, MCV site selection,
centre-first 3×3 scans, reveal/cellspread rings, and tube sentinel handling have
different contents or indexing contracts and retain their existing owners.
Test-only literal expectations remain independent fixtures.

The initializer controls demonstrate table contents and initialization
boundaries. They do not establish complete CRT startup, every accessor, consumer
arithmetic, admission, callback lifecycle, full head publication, or whole
movement parity. Rust regressions and the selected retail scenarios validate
those represented consumers separately. Aircraft pad XYZ/docking lifecycle and
the non-WeaponsFactory search adapter remain open in issue #691.
