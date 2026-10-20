# Walk boundary Cell486920: bounded native audit

The current ordinary ENGINEER bridge repair chain does not need a vein-attack
implementation to reproduce this callback. The callback is a genuine missing
conditional Cell mechanism, not a presentation-only operation. Its first native
gate excludes the tested bridge cells; native retail type loading additionally
makes ENGINEER immune. Keep vein attack as a separate conditional mechanism
residual. This is not a claim that its entire native branch is unreachable.

## Executed evidence and reproduction

Unicorn 2.1.4, core `[2,1,33621247]`. The [harness](walk_cell_486920_audit.py)
uses adjacent [native outputs](walk_cell_486920_audit.json) and
[provenance](walk_cell_486920_audit.meta.json).
Explicit `--write`, then a separate `--check`, passed on 2026-09-27. No original
instructions were patched. The meta file lists prepared-cache/runtime seams.

From the repository root, configure the original executable as described in
[the native comparison guide](../native_oracle.md). Set
`VERA20K_PROJECTILE_RENDER_ASSETS` to a directory containing the physical
`RULESMD.INI`, `MPBattleMD.ini` and `Hills.map` inputs whose hashes are recorded
in the JSON. The measured input directory has no `LANGRULE.INI`.

```sh
PYTHONPATH=. python tools/spatial_oracle/walk_cell_486920_audit.py --check
```

Omitting `--check` also checks without writing. Use `--write` only to explicitly
regenerate the references. The harness imports repository helpers only. Promotion
preserved the originally checked harness, output and provenance bytes; a second
check from the repository path reproduced the same output.

Exactly executed:

- Original Rules overlay enumeration `668CE3..668D34`, native FindOrCreate
  `5FEC70` and full OverlayType constructor `5FE250`, all 250 physical RULESMD
  entries in declaration order. Registry indices 24/25 are BRIDGE1/BRIDGE2;
  122..125 are LOBRDGE1..4; **126 is DUMMYOLD**. Numeric INI keys are not
  registry indices: physical key 126 names LOBRDGE2, while key 129 names DUMMYOLD.
- Full InfantryType `5236A0` construction, including TechnoType construction,
  initializes ENGINEER type `+C91` to zero. Original `714C23..714C44` reads
  exact-case `ImmuneToVeins` using `5295F0`, with the current byte as default:
  RULESMD changes it to 1; MPBattleMD and Hills retain 1. LANGRULE is absent.
  This is a field-reader boundary, not a full scenario/type-loader execution.
- Original Rules constructor prefix `665650..6657C3` initializes `+E8` to null.
  Original `VeinAttack` reader `6692E4..66932C` retains null for each physical
  RULESMD, MPBattleMD and Hills layer. Missing keys were executed, not inferred
  from grep. A null rule alone is not used to certify the effect harmless.
- Eleven full Cell486920 early-gate controls: overlay -1, 24, 25 and 122..125
  return without touching the ground list; index126 controls stop for density47,
  slope1 or an existing `0x20000` flag; the remaining case reads an empty ground
  list. Each retains the whole supplied Cell. No occupant or Anim constructor
  runs in these gate controls.
- Original Techno constructor prefix `6F2B4B..6F3118`, after the Radio parent,
  executes its own XOR and `6F3112` write: `+504=0`. Original Infantry vtable
  `7EB058+37C` points to `70EFD0`; this body returns signed `+504 > 0`.
  Direct supplied timer controls: 0/-1/INT_MIN return false;
  1/255/INT_MAX return true. Positive controls do not establish arming.

The output JSON SHA256 is
`4d0a3ada14fe6c5fa8f4a09ae0ca69c718d08fabdede4a68c10b0f4b5c959f25`;
canonical payload SHA256 is
`4b626e9d985af5714681fd6838bdda958a458128a97eadc0ca7609d2a1f55fc2`.
The executable rows establish the bounded gates/defaults above; the successful
per-resident effect below is established by original instruction reading.

## Original body and active Walk boundary

Walk `75C1A7..75C1B4` calls Mark(PUT); `75C1DB` finds the Cell from current
physical coordinates; `75C1E2` calls Cell486920; `75C1EA` then clears Foot+68A.
The callback therefore belongs after membership replacement and before that
clear. Other observed original callers include `486295`, `4CE668`, `514B86`,
`5B1742`, and Techno PerCell tail `6F5183`; these were not broadened into an audit.

Cell486920 requires `Cell+44 == 126`, unsigned `Cell+11E >= 48`, zero slope
`Cell+11C`, and raw `Cell+140 & 0x20000 == 0`. It walks the **ground** list
`Cell+E4`, not the deck list. Per resident, it calls virtual `+1C8`, rejects a
signed result greater than 5, requires Abstract category byte `+14 & 1`, gets
the type through `+84`, rejects nonzero type `+C91`, then rejects ability8
through `70D0D0` (the existing VeinProof enum). This order matters.

The `+1C8` query is **height, not RTTI**: Infantry's slot is `5F5F40`, which
subtracts the map floor from physical Z and also subtracts the bridge offset
when OnBridge. Negative heights satisfy the signed `<=5` comparison. No
alternative object-category interpretation is supported by these instructions.

Each eligible resident allocates `0x1C8` bytes and constructs an Anim at
`421EA0`, with type `Rules+E8`, cell-origin XY (`signed cell * 256`), Z sampled
at the Cell center by `47B3A0`, delay0, loops1, flags `0x600`, z-adjust0 and
reverse0. At `486A4B` it sets raw Cell flag `0x20000`, **even if allocation
failed**. The flag gate is only at entry: several eligible residents can
construct several animations in one visit. The next resident `+30` is read
after the callback, rather than cached before it.

There is no direct RNG draw, timer write or detach call in the Cell body.
The child Anim constructor/lifecycle is a required dependency of implementing
the successful arm; it was not executed here. Its optional RandomRate draws
and immediate middle processing must not be erased by calling this effect
presentation-only. An Engineer can also cause this traversal of *other*
residents, so Engineer immunity alone does not make arbitrary cells inert.

## Current owners and bounded residuals

`src/sim/movement/walk_host.rs::run_walk_boundary` currently performs Remove,
coordinate/layer/height/subcell updates and Put, then clears the latch. It has
no corresponding Cell486920 call. No sim vein-effect owner was found.
`src/rules/native_processing.rs` allocates the VeinAttack animation reference;
`src/rules/object_type.rs` contains ability8; `src/rules/art_data.rs` retains IsVeins.
Those declarations do not implement the effect or ImmuneToVeins state.

If selected later, put the predicate and per-resident dispatch in one shared
Cell effect owner using canonical raw Cell flags and ground membership. Port
the type immunity reader/default and use the Anim lifecycle owner in native
list order, with persistent flag lifecycle. Wire the Walk call at its original
boundary and migrate other required consumers. Do not insert a one-off visual
emitter into Walk.

For the ordinary Engineer, `+37C` is false from actual constructor state.
Walk `75BF85/75BF87` calls it; a true result at `75BF8F` clears Walk motion
`+36` and Foot latch `+68A` at `75BF9B`, then returns. Existing Rust
`src/sim/world/techno_ai_cloak.rs` explicitly records the missing EMP state/mechanism;
`src/sim/combat/fire_error.rs` and `src/sim/power_system.rs` expose supplied facts, not an entity
timer owner. Native `EMPulse::Apply` writes `+504` at `4C5718/4C5829`, and
Techno AI decrements a positive timer at `6FAF1E`. Native active-retail arming
and loaded positive states are not certified by this bounded audit. Keep that
conditional mechanism unresolved rather than adding an invented timer writer
to the ordinary Engineer bridge chain.

## Corrected instruction identities

The historical A6 D5 finding
previously described RTTI<6, left index126 unchecked and implied one animation
per cell per call. The corrected identities are:

- `486920`: conditional legacy VeinAttack ground-list callback; index126 is
  DUMMYOLD for the declared active retail registry.
- `48697A`: Infantry receiver `5F5F40`; signed floor-relative height<=5, not RTTI.
- `486A4B`: latch after each eligible resident, including allocation failure;
  entry-only latch gate does not stop the current loop.
- `75C1E2`: callback after Mark(PUT), before Foot+68A clear; gate excludes the
  demonstrated high/low bridge indices and overlay -1 side cells.
- `75BF87` / `70EFD0`: Infantry +37C is signed Techno+504>0;
  native constructor write `6F3112` initializes zero. This is not a generic
  death/limbo/falling predicate or proof of global active-retail unreachability.
