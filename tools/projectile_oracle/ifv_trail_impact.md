# Native LineTrail producer joined to deferred Bullet destruction

Run with the same physical asset root and Python environment as `ifv_impact`:

```sh
source /Users/halvor/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
/Users/halvor/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_trail_impact.py --check
```

The shared baseline executes its selected rise-live case. After launch, this
companion enters the original already-admitted Unlimbo tail
`0x5F514B..0x5F5210`. Native DRAGON UseLineTrail, color and decrement fields drive
allocation, `LineTrail(0x556A20)` and owner attachment. Original zero-coordinate,
registry and Options constructor initializers execute; DetailLevel is 2.
Prior world admission remains supplied.

`row.events` preserves the actual retirement order:

1. UnInit → PointerExpired → Conceal → DetachAll → TrailDetach → pending queue.
2. Pending drain → Bullet destructor → PointerExpired → Object destructor.

`DetachAll` instruction `0x5F528E` calls `TrailDetach(0x556B30)` during Conceal
and clears the Bullet's owner slot. Object destructor `0x5F3D56` consequently
has no remaining trail to detach. The earlier constructor-Limbo fixture
suppressed Conceal and incorrectly attributed detach to the destructor.
After drain, `row.trail_after` records owner zero, Bullet slot zero and registry
count one. The detached trail remains for its later fade. VERA's deletion-time
notification remains a separate mismatch tracked in
[#1257](https://github.com/YuriPlanet/vera20k/issues/1257). The baseline's single
impact animation still runs its complete original lifetime afterward.

The companion metadata records original binary identity and payload hash.
`--check` executes without writing; `--write` explicitly regenerates. No temporary
script is imported. This establishes one successful admitted producer and normal
impact/destructor path; it does not establish failed/re-Fire paths, subsequent
trail fade/drawing or whole Display/Logic scheduling.
