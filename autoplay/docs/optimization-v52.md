# V52 — Integrated Optimization / acceptance candidate

V52 intentionally limits changes to the V49–V51 integration and test gate.
No native combat/production rule changes or new strategic capability.

## Changes
- Optional siege/scarce technology can override regular construction only if
  its actual native permission, producer, placement-backoff and 3-minute
  funding test agree. Otherwise ordinary construction may continue.
- Counter funding and actual queue submission share one chosen category and
  type. The request yields to critical recovery, an existing building bank,
  affordable additional mining, opening, active home raid, and economic rest.
  On a saving frame the repair pass sees the same bank. On an affordable
  frame the counter is committed before ordinary line purchases; only
  native-legal, stock-limited, idle-line candidates may use this priority.
  The uncapped `fighter` need-siege shortcut is removed. Normal composition
  can still rank artillery on its merits; the special quota is not a global
  cap on every unit of that type.
- A weaker failed assault cannot reduce the recorded reinforcement reference.
  Two or more recent failed assaults with less than 20% observed best HP
  reduction defer the blocked lane for 1200 ticks using the existing
  front-review selector. The all-candidate fallback remains intact.
- Final wave ownership drops detached members immediately; old matching
  attack proposals are removed, and a different tracked primary is not
  blindly erased. Native safety, recovery and factory handoff remain final
  authorities. Execution avoids cloning the full original-cohort ledger and
  repeated per-member full census scans. Performance still needs measurement.
- Independent `tools.autoplay_acceptance` checks fixed dev results and
  normalized reports, preserving the benchmark runner's fingerprint.
  Test-only synthetic Python children skip unrelated site startup hooks.

## Numerical dev gate (preliminary, previously proposed targets)
120 valid fixed seeds, wins >=36, losses <=60, losses before tick 30000 <=24,
wallclock <=24, median observed whole-match tick/s >=192.58. Require opening
match and log hashes; no crash, missing or protocol-error receipt can pass.
Diagnostics with absent observations remain unknown, not zero. Rate reports
are per 10000 recorded simulated ticks, not accepted orders/kills. A numerical
pass alone does not establish stable gameplay. Historical V41/V48 are not
strict same-engine controls. Compare on same hardware, 16 jobs, same data.

## Local verification
1. Apply exactly one patch for the matching source. Build the actual binary
   and run Rust tests (check any resource-related skipped fixtures).
2. Freeze policy and run fixed smoke (24), then dev (120) in the same new
   target suite directory. Resume receipts, do not edit their fingerprints.
3. Run gate and compare. Inspect wins lost, front guard, per-tick siege
   requests, spending, actual combat output and casualty/repair behavior.
4. Reproduce user scenarios: tank/infantry tower assault, engineering and
   expansion risk, crate vehicle recovery, factory handoff, miner alarm,
   low income and campaign/no-economy cases.
5. Run remaining 982 holdout cases only if dev and qualitative checks pass.
   If regression occurs, compare V48->V49->V51->V52 with the same runner and
   engine controls; this is sequential attribution, not full factorial
   independent feature ablation. No full ablation switches are implemented.

Commands (from repository root, replace executable and retail paths):

    python -m tools.group_tests check
    python -m unittest tools.tests.test_autoplay_benchmark tools.tests.test_autoplay_check tools.tests.test_autoplay_acceptance -v
    python -m tools.cargo_run -- build --profile fast --bin autoplay-match
    python -m tools.cargo_run -- test -p vera20k-core --lib sim::strategy::assist
    python -m tools.autoplay_benchmark run --suite all --stage smoke --run-dir target/autoplay-results/v52 --label V52 --binary "ACTUAL_BINARY" --ra2-dir "YR_DIRECTORY" --jobs 16
    python -m tools.autoplay_benchmark run --suite all --stage dev --run-dir target/autoplay-results/v52 --label V52 --binary "ACTUAL_BINARY" --ra2-dir "YR_DIRECTORY" --jobs 16
    python -m tools.autoplay_acceptance --run-dir target/autoplay-results/v52 --output target/autoplay-results/v52/dev-gate.json
    python -m tools.autoplay_benchmark compare --suite dev --run-dir target/autoplay-results/v52 --output target/autoplay-results/v52/dev-v41.csv

Unverified: Rust compilation and tests, actual game performance, tactical
calibration, full cross-task financial optimization and independent feature
ablation. Keep V48 control and do not infer a win-rate improvement from code.
