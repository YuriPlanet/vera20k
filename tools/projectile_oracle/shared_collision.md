# Shared collision and line sampling boundaries

Reproduce the consumed native outputs with:

```sh
python -m tools.projectile_oracle.shared_collision --check
python -m tools.projectile_oracle.shared_collision --line-walk --check
```

`--write` deliberately replaces the selected corpus and its provenance sidecar.
The [shared fixture](collision_fixture.py) owns both layouts; neither producer
uses a second map or a substituted collision decision. The sidecars record the
replay command, FPCW, assumptions and canonical payload
hash. They pin no repository source hashes.

## Retained Bullet height

`shared_collision_vectors.json` retains the first 130 original controls and
appends six supplied downstream states. Bullet `+8C` (OnBridge) is independent of
the target's same field. The original Bullet vtable height receiver subtracts
terrain height and, for Bullet OnBridge, 416 leptons. `468C9B` admits height
`<= -416`; the Flak branch first requires raw Bullet Z below the target's virtual
`+58` aim Z (`468CE9`), then calls height again and admits strictly negative
height (`468CF8`). The controls bracket both comparisons and preserve the
short-circuit query count. Target OnBridge alone does not change Bullet height.

The fixture supplies the Bullet/type/target, retained flag and candidate position;
it does not execute the flag's FireAt writer, trajectory or first-AI admission.
These are function-boundary controls, not a demonstrated stock match mismatch.
The Rust consumer is
`sim::world::projectile_collision::tests::shared_probe_matches_original_bodies_and_receivers`.

## Signed XYZ line walking

`line_walk_vectors.json` executes the whole original `4CC100`, its reached
`4CC360` body and native Cell/House receivers. Hooks only record candidate XYZ
at each `4CC360` entry and cell query order. No register, memory or branch is
substituted. The synthetic 8x8 cell table, wall/House/type/Rules data, supplied
source/target XYZ and shared DummyCell reuse the collision fixture owner.

`4CC100` narrows endpoint CellCoords to signed16 words, derives the largest XY
cell distance as the step count, subtracts endpoint XYZ with signed32 wrapping,
and executes `CDQ/IDIV` at `4CC1E7/4CC1EE/4CC1F9`. The loop adds signed32
increments at `4CC2D7/4CC2E5/4CC2EC`; its candidate uses source plus the wrapping
accumulators (`4CC254/4CC259/4CC25F`). The source sample is included, while the
final target sample is excluded. Every unblocked sample refreshes the previous
Cell identity before advancing. Same-cell endpoints still query both cells;
flags unset return before those queries.

The 12 histories cover descending and ascending nondivisible Z, negative XY
truncation, wrapped Z subtraction and accumulation, same-cell allocated and
missing endpoints, both flags unset, wall/cliff blocking and aliased missing
endpoints. INT32 boundary values are synthetic arithmetic controls accepted by
this function; they are not claimed as stock-generated coordinates. Native
samples are the expected values; the tool does not calculate a second line walk.

`4CC360` consumes the full sampled XYZ, including native Cell `+48` center
height in the distance branch at `4CC492..4CC4EE`, and executes its major-axis,
requery and retained Cell pointer logic. The corpus records the returned Cell
coordinate or null and final shared DummyCell coordinate. Rust tests consume
these outputs through the production line walker. No full FireAt/InRange,
retail trajectory, damage, frame scheduling, RNG, timer or detach equivalence is
established by these bounded calls.
