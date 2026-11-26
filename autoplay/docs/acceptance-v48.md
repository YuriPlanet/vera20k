# AutoPlay V48 integration candidate — acceptance

V48 is the first planned consolidated gameplay acceptance after V42. It
contains the V42.1 benchmark, opening mobilization, recovery/safety continuity,
readiness, local counters, siege posts, local priorities, economy rest and
stalled-front review from the previous delivered candidates. It also checks
prospective expansion/capture sites and protects native war-factory delivery.
No gameplay acceptance or parameter calibration is claimed by this patch.

## Build gates (before a long matrix)

```sh
python -m tools.group_tests check
python -m unittest tools.tests.test_autoplay_benchmark tools.tests.test_autoplay_check -v
python -m tools.cargo_run -- build --profile fast --bin autoplay-match
python -m tools.cargo_run -- test -p vera20k-core --lib sim::strategy::assist
python -c "from pathlib import Path; from tools.cargo_run import resolve_binary; print(resolve_binary(Path.cwd(), 'autoplay-match', 'fast')[0])"
```

Review skips in retail-dependent Rust fixtures: a missing retail asset set is
not a native delivery test pass. Keep the full compiler and test output.

## Targeted native/campaign checks

These are acceptance scenarios, not measured outcomes. For manual campaign
reproduction save the map/mission, a save before the event, work switches,
unit and target identities, relevant rules/asset overrides and logs.

| Scenario | Expected observation | Failure evidence |
| --- | --- | --- |
| Land factory completes an ordinary tank with AutoPlay on | Native contact/tether keeps the new unit out of all assistant orders, squads and attachments; it can be controlled after release; next products can exit | A Move/Attack/Stop during the native handoff, persistent factory contact, product blocking the door, queue no longer delivering |
| Several vehicles, including a miner or MCV, complete in succession | Each individual handoff ends; normal harvesting/MCV ownership starts after release; no reliance on a fixed unlock timer | Unrelated units frozen, stale handoff, interrupted native track, repeated factory sale needed |
| Expansion site is initially suitable; an enemy appears after departure | The unsafe plot is invalidated and the old unsafe native destination is stopped or replaced; an available safe alternative can be chosen | Repeated travel to a rejected site, deployment into known fire, endless unnecessary detours |
| A low-risk contested ore expansion with friendly support | Legal reachable site outside effective direct weapon coverage may remain eligible; quiet ground wins equally distant ties | All non-perfect sites refused indefinitely, or unsupported direct-fire deployment |
| Engineer repairs a bridge, then finds a new prize; prize itself is exposed | Re-evaluate both route and prospective building armour/coverage; reject known direct-fire prize | Successful but immediately destroyed capture, specialist ordered through known guns, old task resurrected |
| Safe oil/tech prize and a nearby but not directly covering enemy | Consider local support, ordinary escort/mission gates, early economy; capture need not demand an entirely empty region | All useful acquisition disabled, or escort treated as immunity |
| Low-HP V3 with no repair depot | Retreat, stay available for safe stationary fire only; no new exposed pursuit; service when a native depot becomes available | Repeated retreat/attack loop or unnecessary disposal |
| Campaign with no build/economic chain, or mine-area attack | Economy-rest gate does not lock a non-economic force; visible nearby asset threats take precedence over distant siege | Repeated unnecessary kiting, unattended mine threat, futile tower rush |

`factory_handoff_end orderable=true` means the assistant observed release of
its handoff guard and a currently orderable own entity. It does not prove that
that entity moved successfully or that the next production item was delivered.
`factory_handoff_stalled` is a once-per-observed-handoff warning after 450
ticks, not a timer that permits orders. A pre-existing native jam is not cured
by forcibly clearing radio/track state; capture a save and native mission/door
trace if it persists. The new guard affects assistant commands, not the human
player's own orders or the original AI's production mechanics.

## Paired matrix

Use the same freshly built binary, assets and settings for the whole run.
The historical V41 condition is 10k own, 50k hard opponent, 200k tick cap,
900-second wall cap, 512 orders/frame. Historical engine and exact effective
settings are incomplete. Do not conflate a historical result comparison with
strict same-engine causal attribution.

```sh
python -m tools.autoplay_benchmark run --suite all --stage dev --run-dir target/autoplay-results/v48 --label V48 --binary "PATH/TO/autoplay-match.exe" --ra2-dir "PATH/TO/YR" --jobs 16
python -m tools.autoplay_benchmark compare --suite dev --run-dir target/autoplay-results/v48 --output target/autoplay-results/v48/dev-v41.csv
```

The 120-case dev subset is deliberately balanced by historical outcome, not a
representative full-matrix win rate. Review wins lost, losses saved, unresolved
cases, crashes and protocol errors. After this stage passes, change only
`--stage dev` to `--stage all` to extend the same directory to all 1102 cases;
there is no need to rerun the first 120. Compare `--suite all`. A new binary or
configuration must use a new run directory. The optional clean-engine control
patch is described in benchmark-v42.1.md; an independent local V43 checkout is
not the base of this candidate.

## Review gates

- No build/compile/test failures left unexplained. No new abnormal process
  exits or protocol mismatches. Check all native-delivery warnings and the
  targeted campaign scenarios above before attributing a win-rate change.
- Paired wins/losses and early-loss counts: investigate lost historical wins
  and repeated opening/recovery timeouts, not just the aggregate rate.
- Review wall warnings, negative power, `factory_handoff_stalled`,
  `expansion_risk_veto_reports`, economy-rest and available-force samples.
  A zero report is not proof of absence of a bug; samples and throttle-limited
  reports are not counts of all events. Historic absent fields remain missing.
- Compare observed tick/wall-second and unresolved rate only with matched
  concurrency and hardware; do not claim module-level profiling from it.
- Return run.json, results.csv, comparison CSV, results/, logs/, and any
  build/test output. Include campaign saves or repro steps separately; the
  V41 skirmish matrix cannot validate campaign-specific behavior alone.

## Boundaries

Site safety uses current legal vision, real weapon/armour compatibility and
conservative geometric envelopes; it does not prove a terrain path or predict
all future reinforcements, indirect damage, or movement. Local support is a
heuristic, not an exact fight predictor. A scarce first MCV can still need a
safe site before deployment. No rule changes, hidden enemy input, forced HP,
factory queue reset or automatic factory sale. Native unload regressions require
Rust/retail execution, and a genuinely stuck native state may need its own
engine fix. V46 profiling and calibrated strategic thresholds remain deferred
until measured V48 evidence.
