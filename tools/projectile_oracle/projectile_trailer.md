# Bullet trailer animations

`projectile_trailer.py` executes the original active-retail Bullet AI header
`4666E0..4668BD`, including its Object AI predecessor, and the resulting full
`AnimClass` constructor `421EA0`. The JSON contains native outputs; Python
does not calculate cadence, RNG results or animation lifetime. Its sidecar
records the command, input environment and coverage boundaries.

The ordinary path is `SUB → SubTorpedo → Torpedo → ART[SUBT].Trailer=BBBLELRG`.
The BulletType constructor `46BBC0` sets `SpawnDelay=3`,
`ScaledSpawnDelay=0`, `Scalable=false` and a null trailer. Full reader `46BEE0`
retains current defaults through rules layers. With a nonempty current
`Image`, it reads fixed ART `Trailer` with a 128-byte string buffer, resolves
the animation through `428B80`, then reads `SpawnDelay` through native
`ReadInt5276D0` without a clamp. Existing
[`bridge_render_art_state`](bridge_render_art_state.py) owns broader retained
ART/default/read-order controls; this witness reuses that owner.

Before flight, `466808..466822` copies current world XYZ. A null trailer skips
emission. Nonzero `ScaledSpawnDelay` selects signed `IDIV` at `466844`;
otherwise signed `SpawnDelay` is used at `46687C`. The dividend is global
binary frame `A8ED84`, independent of Bullet ID and age. A zero remainder
attempts a `0x1C8` allocation and calls the original Anim constructor with
current XYZ, delay 1, loops 1, flags `0x600`, z-adjust 0 and reverse false.
Allocation failure continues without an animation. The supplied dead and
attached-wait controls return before division. Expired-wait executes its
listener removal and stops at the original explosion entry `468D80`.

The executed domain includes negative frames/delays, both signed extremes,
scaled precedence, null trailers, allocation failure and distinct positions.
Three explicitly selected controls retain native CPU exceptions: base delay
zero, and `INT_MIN / -1` in either division arm. The fixture only admits an
exception at the original selected IDIV; every unrelated execution failure
aborts publication. It does not invent a native zero-delay fallback.

One continuation runs the original Torpedo guidance to precommit `467B7A`:
the emitted animation keeps `[2688,5248,624]` while the subsequent candidate
becomes `[2720,5248,624]`. Incoming velocity, target Cell and detector setup
are supplied. This proves ordering in that continuation, without claiming
complete native launch, collision or flight parity.

## Launch placement prerequisite

`placement_controls` executes the concrete BulletType virtual `+0x6C`,
`46C4F0`, and full `BulletFire468670` through the same native world fixture.
`ObjectUnlimbo5F4EC0` invokes the type fixup at `5F4F88`, then commits it
through `SetLocation5F6940`. The fixup queries original ground owner `578080`;
signed `JG` at `46C520` preserves Z above ground. Otherwise it queries ground
again and increments it at `46C533`. XY remains unchanged. Level and live
bridge flags do not select a different fixup; the bridge control still uses
terrain ground, not deck height.

Direct controls retain the native results for ground-minus-one, equal and
ground-plus-one on levels 0 and 6, distinct XY, a live bridge and signed
input-Z extremes. No artificial `INT_MAX` ground is supplied: the mapped
terrain height and slope representation cannot produce that overflow case.

Full-Fire controls execute original Unlimbo and Display calls rather than
supplying admission. Fire first places the Bullet, copies the supplied
velocity, then retains the **raw** muzzle XYZ in `FireCoord+134`. It derives
`LastCell+14C` from raw XY using signed truncation by 256 and caches target
XYZ. `ProximitySetup4E1130` receives the **placed** Object location; the later
ROT branch normalizes velocity. A surface input Z of -1536 therefore retains
-1536 in FireCoord but places Z at 1; the native detector's distance to the
supplied target is 1536. The corpus records all coordinates, detector timers,
distance, original call/return order and unchanged RNG/native-ID cursors.
Source Unit lifecycle, GetFLH and the upstream velocity producer remain
separate boundaries. Both source and target cells are mapped. Dummy/unallocated
source/cell-target aliasing and its native query-order effects are not covered.

The older IFV launch witnesses share `ifv_launch`'s bounded world-admission
transport. Their duplicated raw-coordinate copies are replaced by original
Unlimbo's type getter, coordinate fixup and SetLocation. Admission guards and
post-placement world effects remain supplied there; the new full-Fire controls
execute the complete original Unlimbo instead. Original `+80/+81` admission
writes also execute. The former raw-copy boundary left constructor InLimbo set,
suppressing part of later cleanup. Native re-execution adds Conceal's
PointerExpired/DisplayRemove to impact and bridge histories. In the LineTrail
history it also moves detach from deferred ObjectDtor to Conceal's
`DetachAll5F528E→556B30`; final state is unchanged. These corrected histories
are original execution outputs, not manually adjusted events. Rust's separate
LineTrail detach-at-drain path remains a timing residual outside the unattached
BBBLELRG chain.

## Animation lifetime

Physical `BBBLELRG.SHP` supplies 15 frames; native ART reader `427D00`
converts authored `Rate=450` to runtime rate 2. Two retained histories execute
Bullet `UnInit5F65F0`, pending drain `725C70`, full Anim AI `423AC0`, native
expiry/conceal/destruction and a final drain. The bubble has no attached
Object/Bullet owner and survives projectile removal. Its same-frame first AI
only clears brand-new state; the next visit consumes delay 1 and calls Start.
With the supplied schedule it retires 30 binary frames after creation. The
host schedules those visits; the complete native Logic loop is outside this
fixture.

Header preparation first executes full original Fire/Unlimbo through the same
`fire_bullet` owner as the placement controls. The corpus retains this admission
and then explicitly supplies the in-flight position, frame and exceptional
alive/wait controls. This avoids using constructor InLimbo state as a live
Bullet. Correct admission adds the original Conceal/DetachAll expiry visit;
the unattached bubble's state, native ID, RNG, timers and lifetime are unchanged.

Both Main and Scenario streams start from bytes produced by original
`Random::Seed65C6D0(31)`. Stock emission/lifetime changes neither stream. The
authored `RandomRate=450,225` control passes through the original reader to
runtime `[2,4]`; its constructor selects rate 3 after two Scenario raw draws,
including rejection, and retires 45 frames after creation. The corpus retains
the raw words, seed bytes, per-stream cursor/hash and native-ID cursor before
and after the header. This control does not replace stock ART.

## Retail Scalable boundary

The auxiliary inventory selects physical RULESMD `Projectile` references plus
the two nuclear roots, then executes original constructors/full readers for
every selected identity. Selection is a supplied inventory, not native
Rules::Process discovery. Four selected types retain trailers:

| Projectile | Image | Trailer | SpawnDelay | Scalable |
| --- | --- | --- | ---: | --- |
| ChemMissile | MISLCHEM | SMOKEY2 | 3 | false |
| DredMissile | DREDMISS | DURASMOKE | 2 | false |
| GiantNukeUp | NKMSLUP | NUKEPUFF | 3 | false |
| Torpedo | SUBT | BBBLELRG | 3 | false |

MedusaProjectile is the sole scalable type in this inventory and has no
trailer. The production Battle/Hills registry currently includes GiantNukeUp
and Torpedo from this table; ChemMissile and DredMissile are outside that
registered inventory. This does not declare their native paths unreachable.

The separate scalable producer `Bullet::InitScalable46B280` appends to the
process vector `89DE18` and calls type setter `46C840`; actual Unit fire admits
it at `741427..741439` only for `Scalable=true`. The appended
`scalable_controls` executes the whole original producer against constructed
Bullets and native type/ART readers. It preserves the original 38 header rows,
54-type inventory and every other existing payload field.

The main history enrolls eleven Bullets using authored Scalable Torpedo
(base delay 3) and a second authored type (base delay 7). It records ordered
membership, the global count and both retained type overrides at each action.
The original count-5 clamp and signed multiply-high division at
`46B2D6..46B2F7` produce the override; no Python implementation computes an
expected delay. Header calls use that output directly: count 8 changes the
first type's cadence to 4, while count 11 changes the second type's to 9.
Changing one type leaves the other type's override retained. Two additional
histories use native ART readers for signed extreme base delays and eight whole
enrollments each, observing the wrapping base-plus-count addition.

Direct original `PointerExpired4684E0` controls distinguish false/true control,
first matching pointer removal, stable compaction and repeated expiry. Full
`UnInit5F65F0` and `Drain725C70` execute separately. The sole-member control
records its self callbacks: membership empties during UnInit before deferred
destruction and remains empty after the destructor's later callback. Removal
does not recalculate type overrides; re-enrollment of the second type after
removal changes its override from 9 to 8. No list operation draws RNG or allocates
an Abstract ID. The existing Anim owner performs real header emissions and
records their constructor state, IDs and both RNG streams.

The stock Medusa control executes its full selected-layer type readers, enrolls
it in the same process list and executes the header. Its retained null Trailer
skips emission before cadence. Its generic configured weapon/target, supplied
incoming pose and direct InitScalable invocation are fixture inputs: this is not
an original AEGIS/UnitFire launch or a demonstrated stock visual mismatch. The
production Rust UnitFire control owns that upstream integration separately.

`initialize_scalable_vector` executes original `4662E0..466316` only in these
new histories, stopping before CRT exit registration. The existing guided-world
initializer slice ends before pointer `81282C`; it does not initialize this
vector. Native vector growth executes against the existing allocator transport.
Full CRT startup/atexit, allocation-failure recovery, complete Logic scheduling,
native save/load graph and full retail match parity remain outside this corpus.
No scalable admission, count, cadence or cleanup result is supplied by a hook.

Native persistence differs between the two states: type `+2E8` lies in the raw
`2F8` bytes written by `46C730→5F9950→410320`; `46C6A0` restores it without
reapplying normal subclass defaults. The process membership list is not part of
that type or Bullet raw data, and Bullet Load `46AE70` does not re-enroll it.
Normal ClearScene `6851F0→534450` retires old Bullets before types; new loaded
Bullets therefore start outside the list while saved type overrides remain.
This is original body/caller evidence, not an executed full SaveGame comparison.

## Reproduce

Use the supported executable via `RA2_DIR` or `VERA20K_GAMEMD_EXE`, plus an
extracted directory in `VERA20K_PROJECTILE_RENDER_ASSETS` containing
`RULESMD.INI`, optional `LANGRULE.INI`, `MPBattleMD.ini`, `ARTMD.INI`,
`Hills.map`, `BBBLELRG.SHP` and `dragon.shp`. DRAGON belongs to the reused
guided world setup. Physical file hashes are recorded in `sources`.
`Hills.map` must be the 144458-byte inner entry `D5FE80AC` of loose
`Hills.mmx`, SHA256
`780d5d6e6d3c81ac0d510a5df326848dd426b3d840ac290114f44faf8eab9e9e`.
The shared `tools.sidebar_oracle.stock.mix` reader extracts that entry;
renaming the MMX container does not extract the map.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.projectile_oracle.projectile_trailer --check
```

Default invocation also checks without writing. `--write` explicitly replaces
the corpus and sidecar. Imports and `--help` do not execute native code or
write output. Existing guided/impact witnesses are re-executed separately;
the corrected admission histories are described above. Those replays are
distinct from this new corpus and from Rust and production-rendering
validation. No retail file bodies or validation receipts belong in this
directory.
