# Occupied wooden hut destruction

`hut_joined.py` executes both authored XShrapnel huts, `(117,56)` and
`(113,62)`, through original `MapClass` entry `0x574C20`, selector `0x574780`
and physical Y walker `0x575540`. Original call sites `0x43896A` in
`BombClass::Detonate` and `0x440301` in Building update select this wooden entry;
the concrete entry is `0x574000`. The caller bytes and checksums are retained in
`hut_joined.json.gz`. The Bomb/Building prefixes themselves are outside this
fixture.

The fixture reuses `occupants_joined.Joined` for MTNK/Drive initialization,
admission, damage, death, list cleanup and original animation constructors. Its
opt-in `bridge_explosions` input executes the existing native list reader over
physical RULESMD, absent LANGRULE, MPBattleMD and XShrapnel layers, then the
existing ART/SHP owner reads every selected animation type. No VM, native
gameplay body, asset decoder or Rust projection is copied.

Both cases start after the same original repair through `(117,56)`, with one
supplied live MTNK at `(115,59)`, ground Z208, frame1000 and game speed4. Main,
Scenario and MapGen each start from original seed0. This is a supplied active
world boundary, not a native scenario load or replay of actor admission.

`ResidentRepair(..., resident_cells=...)` binds physical TMP heads and executes
original Recalc for all33 cells in x114..116/y54..64. This includes the outer
shore cells that hut destruction reaches beyond the older nine-cell damage
fixture. The measured call asserts that every Recalc remains inside this
loaded selection. Omitting the parameter preserves the original nine-cell
binding and Recalc order. The packet retains all10 selected physical TMP
identities and the complete initial Recalc results.

## Executed results

Both supplied huts kill the resident: health300→0, Alive0, Limbo1, Marked0,
one House loss and zero remaining active units. Original Super receives damage300,
distance0, ignore-defenses1, arg6=1 and null attacker/source House. Native
RadarCombatFlash stores start1000 and duration49 before synchronous cleanup;
the raw adjacent +178 store is padding, not another timer field.

For both huts the first constructors are TWLT070, TWLT036 and TWLT050. These
three return before the first `57BAA0` attempt. The first cell needs two damage
attempts. Its lethal receiver then constructs DBRIS4LG and S_TUMU60 before the
walker advances to the next three bridge explosions. There are12 walker
explosions and2 death effects in total. All14 effects remain in the same ordered
Logic, Display layer3 and Anim registry; the tank is removed. The full packet
retains request coordinates/delays, original constructor returns and Bouncer
state. The native identity cursor advances26→40, with consecutive relative
effect IDs1..14; the projection compares relative IDs without claiming native
whole-scenario ID initialization parity.

Each complete hut call executes54 Scenario ranged requests, using63 raw draws,
plus4 Scenario Next requests. Final indices are Scenario67/170, Main1/104 and
MapGen0/103. All250 state words and both indices for every stream are retained
and mechanically extracted; matching request counts alone is not the test.

The `(117,56)` hut collapses center rows59..62; `(113,62)` collapses56..59.
The equal RNG results and effect types accompany opposite sweep directions and
different coordinates. Original Recalc exposes Water2, Beach6 and Rough7 beneath
different collapsed cells; retained boundary overlays still give Road1. This
does not assume an all-water substrate. All33 final overlay/land pairs are
preserved for comparison.

## Reproduction and limits

Use the native executable and asset environment described in
[`occupants.md`](occupants.md). The physical input directory additionally needs
TWLT026/036/050.SHP and Shore04.sno, Shore05.sno, Shore18.sno, green01.sno and
glat05.sno. The SHPs come from the existing archive-input owner; the TMPs come
from `navigation_inputs.extract_tiles` with the same SNOW archive selection as
`shrapnel_damage.navigation.prepare_inputs`. Their bytes are identified in the
native packet, not included in the repository.

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.hut_joined --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.hut_joined_test_vectors --check
```

The first command independently executes both native cases and compares the
immutable publication payload and its sidecar. The second mechanically
extracts Rust values using `project_joined_cases`, preserving every receiver,
timer, constructor, membership and RNG result. It adds observed hut calls,
initial span and final Display dirty byte. Display is already dirty before this
fixture; the before/after bytes alone do not establish its write timing.

Allocation/CRT/OS, lexical INI caches, physical SHP IO, connectivity/hierarchy
and presentation extents retain their declared shared fixture boundaries.
Positional sound stops at its named request. This packet does not execute
AnimAI, later debris flight/contact/damage/expiry, deferred-delete drain,
complete Logic scheduling, the other-axis walker or the full hut/Bomb lifecycle.
The two cases are not exhaustive hut, animation or bridge parity.
