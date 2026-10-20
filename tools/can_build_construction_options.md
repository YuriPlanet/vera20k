# Construction-option admission controls

From a checkout configured for the original retail `gamemd.exe`:

```sh
python -m tools.can_build_oracle --construction-options --check
```

`--write` replaces this profile's separate JSON and metadata. The ordinary
CanBuild and `--secret-lab` profiles retain their own corpora. The production
sidebar-admission tests consume `rows[].results`; each result records actual
CanBuild calls and attempted AddCameo identities in the original registry order.

The complete original `BuildingClass::UpdateConstructionOptions`
(`0x004456D0..0x0044583F`, original Building vtable `0x007E3EBC` slot `+0x4E0`)
requires current-player ownership, non-limbo `+0x81`, discovered-by-current-player
`+0x41B`, and online `+0x660`. It dispatches `BuildingType +0xEB8` Factory RTTI:
BuildingType 7, UnitType 40, InfantryType 16 and AircraftType 3. It then walks
that family registry by increasing index, calls original
`HouseClass::CanBuild(type, false, true)` (`0x004F7870`), and passes each nonzero
answer to `SidebarClass::AddCameo(kind, index)` (`0x006A6300`). In particular,
the build-limit answer `-1` still attempts insertion. Unit entries are not
filtered by their `Naval` flag in this Building function.

This admission differs from retaining an existing cameo. Original
`StripClass::Recalculate` (`0x006AA600`, `0x006AA75F..0x006AA788`) asks
`FindFactory(1, 0, 0)` before the same CanBuild arguments. That lookup does not
require the factory to be online. An existing cameo may therefore survive a
warp even when that factory cannot insert a newly granted option.

The profile extends the existing `BuildEmu`; it supplies current-player state,
the Building admission bytes, sized House counters, original array membership,
and registered Type identities. Original CanBuild and its build-limit check
execute. The phased control calls the complete original Building pause
`0x004521C0` and resume `0x00452210` with empty animation slots. A supplied
captured-lab membership and instance fallback then exercise original
`GetSecretProduction` (`0x00459840`) inside CanBuild. Lab lifecycle, assignment,
the Temporal attack and the House update loop are not executed by this fixture.

The only gameplay sink substituted here is AddCameo: the hook records the two
caller-supplied arguments and resolves their identity from the declared native
array. Actual duplicate rejection, retained strip membership/order, insertion
timers, EVA and pruning remain owned by
[`sidebar_oracle/cameo_order.md`](sidebar_oracle/cameo_order.md) and the production
sidebar. The caller ignores the supplied sink return value. These controls
establish attempts rather than a second implementation of that mechanism.

Native `0x004456D0`, CanBuild, the getter and the pause/resume helpers perform
no RNG draws or detach calls in these controls. The profile initializes all
three RNG buffers with original `0x0065C6D0` and checks them unchanged. It
does not execute the insertion's progress-timer initialization: the existing
sidebar corpus owns that output. The House `+0x1FC` request is a latch, not a
timer.

The active current-player House route (`0x004F9265..0x004F9302`) clears `+0x1FC`,
walks its stored Buildings vector, skips non-live/zero-health/Selling/foreign
entries, invokes Building `+0x4E0`, and only then recalculates strips. The
scenario initialization route at `0x00684C30..0x00684C66` invokes the same slot
over the global Building array under the initialization counter. Those outer
loops are instruction-level evidence; these controls execute their Building
callee and do not claim whole-loop execution.

An active-retail skirmish route is a Chrono Legionnaire warping the sole
infantry factory while an engineer captures a SecretLab that grants a new
infantry type. The Temporal start calls pause at `0x0071B12F` and requests the
House recheck at `0x0071B13A`; release resumes at `0x0071AC3D` and requests it at
`0x0071AC49`. The captured lab grant can pass CanBuild while the factory's
online latch still blocks insertion. Previously retained cameos survive;
release permits the new option on the next House pass. Ordinary insufficient
House power does not establish that online latch.

The metadata pins the replay command, bounded
entry points and supplied seams. It contains no repository-source hashes.
