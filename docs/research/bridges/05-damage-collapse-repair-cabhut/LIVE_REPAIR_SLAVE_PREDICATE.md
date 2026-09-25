# Shared slave predicate during bridge repair and Walk head selection

## Evidence and scope

Original active-retail `gamemd.exe`, x86 little-endian, image base `0x400000`,
SHA-256 `1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Original bodies, callers, virtual slots and retail data were inspected on
2026-09-13. An independent read-only critic accepted the bounded native evidence
and verified the lookup-order correction recorded below. This is not a native
execution corpus, a Rust test result, or acceptance of the whole repair host.

The consumers are the Infantry repair admission call in
[bridge_repair_admission.rs](../../../../src/sim/world/bridge_repair_admission.rs)
and fresh Walk head selection in
[walk_head.rs](../../../../src/sim/movement/walk_head.rs). Both need the same
manager-membership/deposit-cell predicate, `SlaveManager 6B0880`. Existing
[slave_manager.rs](../../../../src/sim/slave_manager.rs) owns the manager and the
slaves' SlaveOwner links; no harvest-state or cargo authority is needed for this query.

## Deposit cells, lookup order and membership

`6B0880(slave, queriedCell)` reads the manager's master at `+24`.
Its primary coordinate is the master's packed cell (`+1B8`), except for a
Building master, where the offset is `(width - 1, trunc(height / 2))`.
`45EC90` supplies Foundation width; `45ECA0(false)` supplies height without Bib.
Coordinate components use native word arithmetic.

The instruction order matters even when the final Boolean answer is unchanged:

1. Compute the primary coordinate (`6B0887..6B091B`).
2. For a Building, compute the candidate secondary coordinate via `6B0690`,
   offset it by `(0,-1)`, look it up with `5657A0`, and call `47C520` to find its
   first Building (`6B092D..6B097F`). Keep that coordinate only if the selected
   Building is the manager's master. Otherwise use the empty sentinel.
3. Look up the primary coordinate (`6B0990..6B099A`) and compare the returned
   Cell identity with the supplied queried Cell (`6B09A3`).
4. On mismatch, and only if the secondary coordinate differs from the empty
   sentinel, look up the secondary coordinate **again** (`6B09C3..6B09CD`) and
   compare Cell identities. If neither matches, return false.
5. Only after a Cell match, scan manager records from `count - 1` down to zero
   (`6B09E0..6B09FC`). The vector is at `+3C`, count at `+48`; record `+0` must
   equal the exact slave pointer. A match returns true; an empty manager does not.

The empty packed coordinate `B0B5B8` is `(0,0)`, initialized by
`6AF0B0..6AF0BE` through CRT entry `814B64`. It is not `(32767,32767)`.
Secondary eligibility lookup happens before the primary lookup. Replacing this
sequence with coordinate equality, collapsing repeated lookups, or checking
membership first can lose shared-Dummy identity and coordinate-stamping effects.
There is no harvest-state, carried-ore or deposit-state-enum gate in this body.
Distinguish a present manager with zero records from an absent manager.

## The two callers consume the result differently

Infantry admission `51BF90`, as called by repair, reaches `51C29E` while walking
the cell object list. It requires mover `+2DC` master, master `+2D8` manager, and
the current blocker equal to that master. If `6B0880` returns true, `51C2C5`
clears the **local** unit-occupation byte at `[ESP+12]`, then `51C2CA` continues
at the next list entry (`51C70F`). It does not clear the world's raw bit and does
not return unconditional admission. Earlier soft-result state and later blockers
still affect the final answer.

Fresh Walk producer `75C240` first clears its raw reservation/current mark, then
performs its target and slave-priority queries. The slave arm at `75C434..75C4B9`
requires master and manager, and `47C3D0` must select that master using query
subcoordinate `(0,0)`, ground list, and no excluded object. A true `6B0880`
then supplies priority to placement `481180`. Preserve the caller's repeated map
lookups after raw clear; do not move them ahead of the clear while shortening
entity borrows. The committed no-slave Walk oracle does not prove this arm.

## Nearest-Techno coordinates

`47C3D0` walks `E4` for the ground query, tests AbstractFlags `+14` bit 0 for
Techno identity, skips only the explicit ignored pointer, and invokes each
candidate's `+48` coordinate query. It does not pick the first Building or the
first Techno. Verified virtual bindings are:

| Candidate | Slot address | Body and coordinates |
| --- | --- | --- |
| Unit | `7F5CB8` | `5F65A0`: copy Object `+9C` XYZ |
| Infantry | `7EB0A0` | `5F65A0`: copy Object `+9C` XYZ |
| Aircraft | `7E22EC` | `5F65A0`: copy Object `+9C` XYZ |
| Building | `7E3F04` | `447AC0`: Object XYZ plus `((width<<7)-128, (height(false)<<7)-128, 0)` |

Only after this call does `47C448..47C45E` mask each X/Y to its low byte and
subtract seven times the corresponding query subcoordinate. It squares and
sums the signed deltas, calls original approximate square root `4CAC40`, then
integer conversion `7C5F00`. Comparison `47C499..47C49D` rejects equal as well
as larger distances, so ties retain cell-list order. Candidate Z is copied but
not used in the distance calculation.

Foot's separate `+4C` coordinate owner may use locomotor state; substituting it
for `+48` changes this query. Conversely, a raw-position nearest helper is not
generally equivalent for Buildings: a 2x2 Foundation adds 128 to each axis before
the low-byte extraction.

## Retail inputs and lifetime limits

Winning rules SHA-256:
`3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef`.
Winning art SHA-256:
`e1f0378394313c04ebbd5073f47785ee3e46f1b3c62d65724e8f3c310ee7ba31`.
SLAV has `Slaved=yes`; SMIN and YAREFN have `Enslaves=SLAV`, `SlavesNumber=5`.
YAREFN's art Foundation is 2x2.

The native transfer path `449C30 -> 6AF580` transfers the old manager and rewrites
surviving slaves' master pointers at `+2DC`. It retires the fresh new owner's
initial pool, not the old owner's surviving children. The Rust manager
(`sim::slave_manager`, `GameEntity::slave_manager` and the slave's
`SlaveLink::owner`) likewise survives SMIN/YAREFN transfer
(`transfer_slave_manager`). This supports reusing that authority; it does not
certify all transfer or slave-harvesting behavior.

Stock type existence and manager transfer alone do not prove that every proposed
master/deposit arrangement can occur inside a bridge-repair callback. In
particular, a Building-master repair-footprint scene remains unproved here.
Hand-authored Rust regressions can test integration and ordering, but must not be
presented as native goldens. Required source review, native-comparison coverage
and whole-host validation remain separate evidence.
