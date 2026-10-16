# Native AreaDamage receipt and isolation controls

`ifv_area_receipt` executes 22 controls through full `AreaDamage(0x489280)`.
The original cell-spread initializer `0x561910` establishes the ordered offsets;
native collection, receiver admission, Iron Curtain timer and return logic run.
The Unit ReceiveDamage body is an explicit supplied-result boundary, so these
results establish dispatch receipts rather than damage or kill outcomes.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
~/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_area_receipt.py --check
```

`rows[].input.name` identifies each control. `result` is the original full
function return, `dispatch` records actual admitted calls and their supplied
receiver result, and `flags` records the native isolation/dispatch locals.
All rows preserve Scenario RNG.

- Return **0** means a receiver was dispatched, regardless of its supplied
  return 0, 1, 2 or 4. The caller sets its receipt byte immediately after the
  virtual call; it does not interpret the receiver's result.
- Return **1** covers no dispatched receiver and early damage-zero,
  Scenario-no-damage or null-warhead exits. Dead, zero-health, limbo and unmarked
  controls exercise original admission gates.
- Return **2** comes from near-center Iron Curtain isolation. Native
  CellSpread 0.5 admits this arm; 0.5001 does not. Distance 84 admits, 85 does not.
  The original timer returns active before its expiry and false at equality;
  receiver kind `+1C4=1` suppresses isolation. The return-2 arm skips the later
  bridge/effect continuation inside AreaDamage.

Physical HE is read through its full native reader; CellSpread overrides also
use that reader. Health, occupancy, XYZ and timer state are supplied incoming
inputs. This is not a Unit constructor, occupancy or receiver mutation proof.
Companion metadata pins the executable and output payload; `--check` independently
reproduces without writing, while `--write` explicitly regenerates.


## Area-rocker numeric component

`--rocker --check` consumes the additive `area_rocker.json` corpus. Use
`VERA20K_PROJECTILE_RENDER_ASSETS` with physical GTNK.VXL, GTNK.HVA,
VOXELS.VPL, the original rules/art/mode/map and the existing IFV image inputs.
The existing physical reader/VM runs the full BlimpHE reader; spread0 is an
explicit reader override. Nine rows enter the declared post-receiver
`489B49` frame and stop before bridge continuation `489E87`. They cover the
coupled center source, pure vertical source, absent/equal source, spread0,
30/31 threshold and400/401 cap. The vertical source is ground624 plus
retail ZEP JumpjetHeight750, an explicit component prior.

Existing `voxel_oracle.raster` executes the physical GTNK loader/postpass.
Its actual header/error flag and half-sizes are supplied to the Area VM as
model inputs, matching the joined Drone boundary. Original Unit vtable,
getters, IsVoxel and whole ApplyRocker70B280 execute; native Type constructor
Weight1 is observed. This is not full joined TypeReadVXL binding or actor
construction/placement proof. Inputs retain the actual FV-shaped type
identity used by the existing IFV fixture, rather than claiming a native
MTNK actor was constructed.

The corpus retains ordered visits, actual Apply points/force/no_dampen,
four state words and native stores, plus unchanged full Main/Scenario/Mapgen
states seeded31/32/33. No getter, cast, impulse or RNG return is supplied.
Pure vertical and zero total distance remain distinct controls; signed zero
is retained. Actual receiver/death/expiry behavior, the choice of layer
before receivers, upstream whole FireAt/impact, bridge continuation,
subsequent AI and scene pixels are excluded. They belong to production
integration tests and the existing owners. Historical default corpus bytes
and behavior remain unchanged.
