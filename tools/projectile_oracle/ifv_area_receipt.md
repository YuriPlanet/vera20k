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
