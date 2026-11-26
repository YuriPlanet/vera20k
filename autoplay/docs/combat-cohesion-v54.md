# V54 — Combat Cohesion & Map Control
## Scope
Skirmish AutoPlay only. Cumulative V53 regression recovery plus the
source-level integration changes below. No campaign tester/mission work.
No V53 or V54 gameplay data yet; do not infer a win-rate improvement.

## Combat continuity
- On the existing charge admission cadence, choose the densest local
  eligible vehicle cluster among at most 48 nearest candidates, rather
  than always using the nearest vehicle as the leader. The actual charge
  still uses at most 24 actors and the V53 forecast/risk/retry gates.
- During ordinary siege staging (not strategic refit or the support screen
  of an existing charge), use a common approach side for the local armoured
  group. Formation direction is chosen from at most 16 nearby candidates.
  Individual posts retain ID-derived lateral offsets. Validate the native
  movement zone, actual known gun envelopes and safe endpoint. Fall back to
  the actor's own side when the common direction is unsafe. Retain a valid
  current destination rather than continuously reassigning it.
- Ordinary siege staging and final front review share the same local
  ready-to-fire criterion. An ordinary healthy vehicle (>=70% HP) may
  retain or request a direct shot on a live defensive battery if two other
  eligible healthy vehicles nearby can already fire at that target. This
  grants no pursuit or AttackMove permission. Known mobile and fixed mind
  controllers veto this exception for susceptible actors. Native service,
  task ownership, strategic recovery and final safety remain authoritative.
  The allies' readiness is not evidence of actual accepted shots.
- The artillery firing-annulus check permits an outward exit from current
  exposure if the route leaves the known envelopes and the destination is
  outside all of them. No extra permission to enter another gun's range.

## Map control
- Existing bounded asset clearance and spare-defender muster may now
  choose nearby income assets and technology buildings, not just factories
  and yards. Provisional priority range: 28 cells for income/technology,
  20 for other low-tier public assets; higher-value factory/yard eligibility
  retains its earlier policy. Existing home guard reserve, main-fist
  reserve, local force comparison, safe assembly, retry and operation
  deadlines remain. No separate operation owner.
- Harmless mobile objects no longer keep the clearance operation in
  `Clear` merely by existing near the prize; armed/mobile special threats
  still count. Actual site/engineer route safety remains a separate gate.
- Record actual ownership transitions of surveyed assets with
  `asset_control_confirmed`, including the previous observed hostile status
  and income flag. This is a lower-bound observation of acquisitions, not
  all native engineer captures, not a count of capture orders, and not proof
  the building is still held. Old uninstrumented results are unknown.

## Economic conversion
- A measured zero-income state releases the normal discretionary reserve
  instead of holding an irreplenishable wallet. Existing committed costs,
  last-miner and essential construction recovery remain protected.
- If a second discretionary line bank cannot plausibly fund a vehicle
  within the existing saving horizon on measured income, don't freeze all
  remaining optional purchases with that bank. Opening protection when
  income is unknown remains intact.
- Apply the V53 one-minute optional construction funding gate to the
  regular build-list optional bank too, not just the optional technology
  override. Essential recovery and focused refinery savings are unchanged.
- These are policy candidates, not confirmed optimal economy parameters.

## Testing
The benchmark runner and fixed 1102-case manifest are unchanged.
New independent recovery report verifies receipt/log hashes with the
existing runner, reports missing/invalid cases separately, checks the
fixed dev gate, and reports six previously shared regression seeds:
20024, 20150, 20218, 20591, 20688, 20824. Optional --before compares only
paired valid results under matching recorded controls; a changed HEAD
still prevents exact single-policy attribution.

Use the actual newly built binary and unchanged retail/engine controls.
Compile and run the Rust fixtures first. Then run the existing 24 smoke
cases, continue the same run directory to 120 dev cases if suitable, and
review actual combat/asset behavior. Do not run the 982 holdout cases until
the dev results recover. The V41 recovery reference is 30 wins/60 losses,
with the stronger formal 36-win gate unchanged.

## Verification limits
Rust compiler, Rust fixtures and actual V54 game runs unavailable in this
delivery environment. The 35 Python tests and V52 reference report do not
validate the Rust behavior. Patches are checked against exact affected-path
predecessor snapshots. Do not infer same-engine causality from comparisons
with the historical V41/V48 binaries. No campaign modification or new
independent feature-ablation mechanism.
