# Sim-layer scripting

VERA can run small scripts inside the deterministic simulation. A script is
bound to a TechnoType with `Script=<Name>` and reacts to two events: once per
frame while the unit is alive, and once whenever one of its projectiles
detonates.

Scripts never mutate the world directly. They queue commands (damage, lasers,
superweapon flashes) that the host executes in a fixed order after dispatch, so
saves, replays and multiplayer state hashes stay valid.

## Directory layout

```
<game>/vera20k/scripts/
├── scripts.json          optional manifest
├── example_ring.rhai     one script per file
└── ...
```

- A file's stem is its binding: `example_ring.rhai` answers `Script=example_ring`.
- `scripts.json` can bind a different name, or point at a file in a
  subdirectory:

```json
{
  "scripts": [
    { "name": "ExampleShow", "module": "example_ring.rhai" }
  ]
}
```

- A missing directory, a missing manifest or a broken file is a startup
  diagnostic. None of them prevents the game from launching.
- The file is re-read whenever its timestamp changes, so a value can be tuned
  and seen without restarting. The AST is cached per path until then.

## Entry points

```rhai
fn default_state() { #{ frames: 0 } }   // optional
fn on_update(ctx, state) { state }      // every frame
fn on_detonate(ctx, state) { state }    // one of this unit's shots went off
```

Each function returns the updated state map. A script that returns nothing
keeps the previous state. Only the functions a file defines are dispatched;
a file with neither `on_update` nor `on_detonate` is rejected at startup.

`ctx` is a read-only map:

| key | meaning |
| --- | --- |
| `event` | `"update"` or `"detonate"` |
| `frame` | simulation frame |
| `has_target` | the unit currently holds a target |
| `anchor` | `[x, y, z]` of that target in leptons (256 per cell) |
| `height` | target `GetHeight()`; the ground under an air target is `anchor[2] - height` |
| `air` | the target is airborne |
| `target` | stable id of the target, or `0` |
| `projectile` | rule name of the detonating projectile (detonate only) |
| `impact` | `[x, y, z]` of the detonation (detonate only) |

`anchor` is never the scripted unit's own position. When `has_target` is false
it is `[0, 0, 0]`, so a script that damages `anchor` cannot hit its owner.

## Host functions

| call | effect |
| --- | --- |
| `laser(from, to, width, duration)` | beam in the owner's house colour; only this arm widens with `width` |
| `laser_rgb(from, to, width, duration, [r, g, b])` | beam in an explicit colour |
| `damage(warhead, amount, coord)` | area damage only: no animation, sound, light or shake |
| `detonate(warhead, amount, coord)` | one invisible bullet's `Detonate`: shake, damage, the warhead's `AnimList` animation (and its `Report=` sound and light) and the impact flash |
| `flash(superweapon, frames)` | `ScreenNukeFlash` with that superweapon type's `Light.*` tint |
| `superweapon(name, cell_x, cell_y)` | launch a superweapon through the normal sim path |
| `ring_point(center, radius, degrees)` | point on a circle around `center`, same Z |
| `cell_center(leptons)` | `Cell2Coord(Coord2Cell(x))` for one axis |
| `cos_int(radius, degrees)` / `sin_int(radius, degrees)` | truncated integer trigonometry |
| `log(message)` | write to the engine log |

`damage` and `detonate` differ in exactly one way: `detonate` also plays the
warhead's own presentation. Use `detonate` unless the effect must be silent.

## State and determinism

- State is serialised as JSON into `ScriptState` and saved with the match. It
  is part of the state hash, so a replay reproduces it byte for byte.
- State of a dead entity is dropped; a reused stable id never inherits it.
- Scripts must stay deterministic: no wall-clock time, no filesystem reads
  during a callback, no floating-point beyond the integer helpers above.
- Dispatch order is the registry's registration order, then ascending stable
  entity id. Both are fixed.

## Limits and errors

- 200k operations per callback, 16 call levels, 64/32 expression depths. A
  runaway loop is stopped and reported instead of stalling a tick.
- A runtime error is logged once per distinct message and the callback's
  effects are discarded; the game keeps running.
- A syntax error is reported at startup with its line, and that file is
  skipped.

## Example

`docs/scripts/example_ring.rhai` draws a ring of house-coloured beams around
the current target and detonates the centre every 30 frames; the detonation
turns on the map lighting through `on_detonate`. Its tuning lives in `cfg()`:
Rhai functions do not see the file's top-level scope, so a file-scope `const`
would only fail at runtime.
