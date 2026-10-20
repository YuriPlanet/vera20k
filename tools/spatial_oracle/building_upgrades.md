# Ordinary building upgrade controls

Run against a supported original executable:

```sh
VERA20K_GAMEMD_EXE='<original-gamemd.exe>' python -m tools.spatial_oracle.building_upgrades --check
```

The corpus contains 15 admission/install, 6 power and 4 removal controls. It uses
the existing `building_art_transition.ArtFixture`, original executable mapping,
and checked Unicorn execution. The metadata records the actual executable SHA,
original byte slices, fixture priors and substituted callout boundaries. No game
assets, mod archives or local paths are needed or included.

## Native behavior established

`BuildingClass` vtable `0x7E3EBC` stores Unlimbo `0x440580` at `+0xD8`, Limbo
`0x445880` at `+0xD4`, and Object UnInit `0x5F65F0` at `+0xF8`.

- `0x452670` requires identical owner pointers and compares the incoming
  `PowersUpBuilding` text (`type+0xE88`) with the host's type ID (`type+0x24`)
  using the original ASCII case-insensitive CRT comparison `0x7C8D20`.
- `PowersUpToLevel=-1` accepts a host count (`building+0x702`, signed byte)
  below its `Upgrades` value (`type+0x14E0`). If that fails, only count zero is
  accepted. Explicit levels 1, 2 or 3 also require count zero. Other levels fail.
  This is an admission predicate, not an arbitrary empty-slot query.
- Unlimbo repeats those gates at `0x4408DF..0x440951`. Before PowerUp it marks
  House bytes `+0x5779/+0x5778` and copies the incoming type image name into the
  host type's old-count animation name when it differs, bounded to 16 bytes.
- `0x4409C7..0x4409D3` invokes actual `PowerUp @ 0x451400` once for level -1,
  or repeatedly for a positive explicit level. PowerUp first restores host HP
  to its type Strength, refreshes damaged art, and only then checks the ordinary
  host's count against its configured capacity. Successful increments create
  the corresponding power-up animation. The install caller ignores a failed
  PowerUp return, so level 3 on capacity 1 ends at count 1.
- `0x4409D5..0x4409E2` stores the incoming **type pointer** at
  `host+0x5E8+4*new_count`. It does not retain a second live building. The caller
  refreshes house superweapon eligibility (`0x50B1D0`), marks House `+0x1FC`,
  optionally refreshes satellite visibility, and invokes the temporary object's
  UnInit through `+0xF8` before returning success.
- `Power_Output @ 0x44E7B0` adds Power from all three nonnull retained type
  slots (`+0x5EC/+0x5F0/+0x5F4`) when count is nonzero, then applies the host's
  health ratio to the combined positive output. `Power_Drain @ 0x44E880` adds
  all three PowerDrain values without health scaling. The sparse-slot controls
  distinguish scanning all three pointers from iterating only `0..count`.

The malformed `Upgrades=0`, count-zero case is retained as evidence: native
admission succeeds and consumes the temporary object, PowerUp restores HP but
does not increment count, and the final type store targets `+0x5E8`, before the
normal three-pointer array. The corpus does not license out-of-bounds memory
writes in Rust. Larger-than-three capacities and overflowed runtime counts are
outside its ordinary host coverage and require an explicit safety residual.

## Connected callers and lifecycle evidence

Original direct capacity callers include the placement-preview branch
`0x47EE61`, placement-flush `0x45EF68`, local placement UI `0x4ABA94`, and house
site search `0x506C2E`. The `0x47EE61` branch is distinct from ordinary
`CellClass::Is_Clear_To_Build @ 0x47C620`: that whole cell-admission body has no
`PowersUpBuilding`/`0x452670` bypass. For an ordinary upgrade type, an occupied
host still fails its object check, while empty ground can continue through
occupation bits, playfield, overlay and land/slope checks. The UI's independent
`0x4ABA94..0x4ABA9D` host admission overrides Display `+0x1180` to true;
`+0x1181` visibility remains required. Host lookup and the placement event use
the same clicked cell plus the Display foundation offset, with signed16 wrap.

Flush checks each cell's overlay before its first object. An admitted Building
at `0x45EF68` jumps back to the foundation-cell loop (`0x45EEAE`), continuing
the remaining cells instead of returning Clear immediately. Rejection blocks.
Actual ordinary upgrade installation enters `0x440871` before normal
Techno/Object Unlimbo admission: the initial class query at `0x4405A5` stores
its result but does not refuse this ordinary upgrade branch. These are checked
original instruction reads, not new executions of the complete
input/factory/site-search chains or of the Unlimbo prefix.

The map reader `0x44FD50..0x44FDC3` processes the authored upgrade prefix in
order. For each resolved type it calls the original Building constructor
`0x43B740`, then the same virtual Unlimbo at the host coordinates and facing.
It does **not** overwrite `PowersUpToLevel` with the authored list ordinal.
The constructor path allocates a normal object identity and inherits the
Techno constructor's one RNG draw at `0x6F3254`; existing constructor evidence
owns those values. Map upgrades do not receive the main building's tag binding.

The real Object UnInit (`0x5F65F0`) calls global pointer expiry (`0x7258D0`,
detach flag 1), virtual Limbo, clears alive and queues deferred disposal. The
shared existing lifecycle must retain that order rather than silently dropping
the temporary object. The corpus records this UnInit call boundary; it does not
execute or certify full pointer listeners, destructor counters or disposal.
Neither the inspected install arm nor PowerUp itself introduces an RNG call or
timer write. Construction, animation allocation and lifecycle have their own
existing RNG/timer effects and are not covered by that narrower statement.

An ordinary sale with upgrades follows a separate stage-zero arm
(`0x44AAB5..0x44AB44`): take the last retained type, construct a temporary
building through type virtual `+0x8C`, compute its refund through virtual
`+0x2BC`, credit the house, and directly destroy that temporary object through
scalar destructor `+0x20(1)`. This destruction is distinct from install's
UnInit/deferred disposal. Next call `PowerDown @ 0x451690`, mark house power
refresh and request Guard through virtual `+0x1E8`. This is the real shared
Queue_Mission `0x5B35E0`: its `0x5B35F7` Selling guard refuses the request.
The upgrade branch returns delay 1 at `0x44AB4D`, before the ordinary stage
write `+0xBC=1` at `0x44ABAC`. Thus it retains Selling stage 0 for the next
visit, removing another upgrade then; count zero proceeds with the normal
host sale. It does not cancel the Selling mission. Instructions establish this ordering;
the new corpus does not supply or execute the complete factory/refund/destructor
chain. Existing shared constructors and refund comparisons own their numeric
results. Four new PowerDown controls execute the actual empty-animation release
helper and count/type-pointer mutations: count 1 clears slot 0; sparse count 3
clears slot 2 and becomes count 2; a null last slot still decrements; count zero
does nothing. No pointer compaction occurs. Cloaking and superweapon PowerDown
arms remain outside these ordinary power-up controls.

## Coverage limits

The controls supply an already resolved ordinary host and incoming building,
not physical INI loading, world lookup or Building construction. Original
admission, name comparison, PowerUp, health ratio and conversion execute.
Animation allocation, house refresh and temporary-object UnInit are named
recorded boundaries; no expected gameplay decision is supplied at those hooks.
WallTower, weapon/superweapon upgrades, satellite upgrades, absorption,
overpower, complete selling/capture/destruction and rendered animation output
are not certified. Rust regressions consuming these goldens must retain those
limits rather than claim whole upgrade parity.
