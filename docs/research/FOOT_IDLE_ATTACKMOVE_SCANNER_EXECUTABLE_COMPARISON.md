# Foot idle, retained AttackMove and target-scanner evidence

These executable corpora preserve native receiver order needed to replace the
competing order-intent and passive-acquisition paths. They are evidence
prerequisites. They do not implement or certify the Rust migration, full native
class callbacks, or a complete game loop.

All runs require the original retail `gamemd.exe`, SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The [shared runner](../../tools/native_oracle.md) verifies the same immutable
image bytes it maps. Each corpus has adjacent Python source and provenance JSON.
Original instructions remain unchanged. Declared external machine-code callbacks
run with the original CALL frames; copied fixture vtables redirect only documented
slots. Hooks observe execution and writes. Harnesses verify native byte integrity,
execution boundaries, stack balance and nonvolatile registers.

## Foot EnterIdle and AI admission

[`foot_enter_idle.py`](../../tools/spatial_oracle/foot_enter_idle.py) executes the
complete original Foot receiver `4D82B0` and QueryInterface helper `45AF20` under
explicitly bounded fixture state. Its **281 cases** comprise 192 latch/argument/
Scatter/END/queue combinations, 54 queue mutations, 16 recursive original Foot
calls, three queue changes in the Techno callback, and 16 Foot AI entry slices.

The receiver publishes its `+6B3` recursion latch before callbacks. The corpus
captures callback order, live queue storage/count reloads, END restoration and
speed-setter gates. Queue addresses carry original Cell, Unit or Building vtables,
but the Foot body transports these identities without executing category-specific
destination semantics.

The AI slices execute `4DA530..4DA554` with a declared Techno AI callback and the
original `4DAF00` dead-return epilogue. They distinguish entry values from the
post-callback `Object+90` admission and `Foot+6B3` reset. They do not execute the
later Foot body or enclosing class AI continuation.

Excluded: actual Techno idle effects (Temporal, retained AttackMove and PlanMgr),
Scatter and destination/speed receivers, real COM lifetime/policy, legacy planning
with `+520 != -1`, Infantry archive pursuit, and enclosing class idle leaves.

## Retained AttackMove

[`foot_attack_move.py`](../../tools/spatial_oracle/foot_attack_move.py) records
the four independent retained facts: signed discriminator `+5C4`, saved destination
`+5C8`, saved target `+5CC`, and engagement byte `+5D1`. Saved destination and target
can coexist. Destination wins on resume. Its **638 cases** comprise 366 clear/
predicate/resume/acquisition/idle cases, 176 setup cases, 12 coordinate getters,
48 target conversions and 36 bounded saved-pointer expiry slices. The original
366-row prefix is preserved unchanged by the setup/conversion extension.

The original clear, predicates, acquisition, resume and idle-resume bodies retain
their native virtual calls and effective-mission getter. Resume reloads its saved
pointer after QueueMission. Acquisition reloads saved target after a failed scan.
The idle receiver can perform two live effective-mission reads. Callback results
do not replace these owner-memory observations. Setup preserves the unselected
saved pointer, distinguishes tagged null resolution from absent tags, and returns
the signed event selector on ordinary commands. Normal setup rows execute original
class/type eligibility and token resolution on supplied type/index/map state.
Conversions use original signed division/narrowing and map lookup. Expiry slices
execute `4D9AC9` through stop-before `4D9B43`, including comparison of the newly
written destination to the expired pointer; these are not full detach calls.

External target predicates, scanners, mission queue and destination/target setters
are declared fixtures. Their bodies, the command executor, production scheduling,
index registration, map/object lifetimes, whole-class detach and persistence
require separate implementation evidence. A supplied false predicate for a null
target is a boundary stress case: native `6F77B0` returns true for null targets.

## Common target scanner

[`techno_target_scan.py`](../../tools/spatial_oracle/techno_target_scan.py) executes
the complete original `709820..7099CC` body. Its **171 cases** comprise 80 existing-
target/passive/error/spawn combinations, 32 installation/weapon/projectile-flag combinations,
45 mission/jitter/mask combinations, four wrapping health-debit cases, and ten
callback mutations.

Observed native order includes the unconditional frame stamp and RNG draw; delay
selection from committed mission; passive-target error gates; the code-6 spawn
callback; threat selection before DistributedFire dispatch; live target reloads;
and the signed-width subtraction from the retained scan result's estimated health.
The selected result and current target remain distinct across callbacks. One row
explicitly proves that GetFireError receives a target changed by the preceding
weapon-selector callback.

The debit exclusion reads `WeaponType+A0 -> BulletType+2A2`. `WeaponType::ReadINI`
pushes the `Projectile` key at `77298A`, then resolves and stores that type at
`7729A5..7729AA`; the warhead pointer
is a separate `WeaponType+AC` field. The semantic INI name of `BulletType+2A2` is
not established by this corpus.

The timer's inactive member receives a seeded uninitialised stack local in this
fixture; its numeric value is not a universal native invariant. Actual RNG,
GetFireError, threat selection, weapon resolution, damage estimation, SpawnManager,
DistributedFire and class AssignTarget effects are substituted and remain outside
this corpus. In particular, this corpus does not supply a complete estimated-health
lifecycle or justify reconstructing estimated health from actual health.

## Reproduction and acceptance

Set `VERA20K_GAMEMD_EXE` to the pinned local executable, then run from the repository:

```text
python -m tools.spatial_oracle.foot_enter_idle --check
python -m tools.spatial_oracle.foot_attack_move --check
python -m tools.spatial_oracle.techno_target_scan --check
```

`--check` does not rewrite references. `--write` deliberately regenerates them and
requires review of source, native boundaries, callback declarations and output.
Matching corpora demonstrate bounded native execution reproducibility; Rust parity
requires production-path comparisons against these contracts plus completion of
the excluded dependencies. No Rust behavior, save schema or lockstep hash changes
are included in this evidence prerequisite.
