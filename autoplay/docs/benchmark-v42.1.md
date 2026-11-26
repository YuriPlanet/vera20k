# AutoPlay V42.1 — reproducible V41 benchmark and recovery diagnostics

V42.1 deliberately does not tune the V42 strategy. It establishes a replayable
benchmark and makes recovery exits and native wall-product departure observable.
The old `tools.autoplay_check` remains useful for ad hoc smoke checks; use the
new manifest runner for matrix acceptance. Python 3.10+; no third-party packages.

## Evidence and suites

`autoplay/benchmarks/v41.json` was reconstructed from the original
`v41基准.zip` CSV and indexed match logs. Each row is joined to the log named
with its CSV row index, map and countries; each log header must agree with
seed `20000 + row_index`, map, 200000 tick cap and 512 commands/frame. The first
census must be `[10000, 50000, 1, 1]` (own/enemy credits and entity counts),
and the next census must be at tick 1800. A decided or tick-capped CSV row must
match the exact terminal log marker; wallclock rows must not have one. Where
the historical CSV has no wallclock tick, the last sampled tick is retained,
not a fabricated end tick. Log SHA-256 is retained for each row. The eight
ambiguous country/map log candidates noted in the investigation are resolved
by the indexed filename/header checks, not by choosing the longest game.

| Suite | Cases | Early losses | Late losses | Wins | Unresolved |
| --- | ---: | ---: | ---: | ---: | ---: |
| smoke | 24 | 6 | 6 | 6 | 6 |
| dev | 120 | 30 | 30 | 30 | 30 |
| holdout | 982 | 356 | 293 | 263 | 70 |
| all | 1102 | 386 | 323 | 293 | 100 |

Early = lost before tick 30000. The smoke suite includes the handoff anchors
(NewHghts France/Britain seed 20222; EB5 Libya/Britain 20145; Arena; Bermuda;
Deadman; BayOPigs; Death) and all three historical tick-cap matches. The rest
is deterministically filled by outcome bucket and faction/opponent/map coverage.
Both smoke and dev cover all ten local and opponent countries. `dev` contains
`smoke`; `holdout` is the complement of `dev` in `all`. The dev/holdout split
is a prospective tuning separation only: historical outcomes have already
been examined and this is not an independent unseen dataset. The diagnostic
suites are intentionally balanced, so their raw win rates are not estimates
of the full V41 distribution. Never mix them when quoting an overall win rate.

Original baseline: 293 won, 709 lost, 97 wallclock, 3 tick timeout;
29.24% decided win rate, 26.59% all-record win fraction, 9.07% unresolved.
The historical effective game options, binary and installed asset hashes,
spawn positions and exact wall-cap implementation were not preserved. A
historical comparison is therefore not a controlled same-engine experiment.

To reproduce the manifest itself:

```sh
python -m tools.autoplay_benchmark import-v41 /path/to/v41-baseline.zip
python -m unittest tools.tests.test_autoplay_benchmark tools.tests.test_autoplay_check -v
```

## Build, run, extend

Build on the actual checkout with retail assets available, as usual:

```sh
python -m tools.cargo_run -- build --profile fast --bin autoplay-match
python -m tools.cargo_run -- test -p vera20k-core --lib sim::strategy::assist
python -c "from pathlib import Path; from tools.cargo_run import resolve_binary; print(resolve_binary(Path.cwd(), 'autoplay-match', 'fast')[0])"
```

Use the explicit freshly built binary path (on Windows include `.exe`),
not a guessed target directory. All examples are one-line commands to work
in PowerShell, cmd and Bash. Set RA2_DIR or pass `--ra2-dir` explicitly.

```sh
python -m tools.autoplay_benchmark run --suite all --stage smoke --run-dir target/autoplay-results/v42.1 --label V42.1 --binary "PATH/TO/autoplay-match.exe" --ra2-dir "PATH/TO/YR" --jobs 16
python -m tools.autoplay_benchmark compare --suite smoke --run-dir target/autoplay-results/v42.1 --output target/autoplay-results/v42.1/smoke-v41.csv
```

The target suite stays `all`. Repeat the exact run command to resume, or
change only `--stage smoke` to `--stage dev`, `--stage holdout`, or
`--stage all` to expand. The completed cases are never run again. The runner
requires the same binary contents, manifest, retail top-level input file
hashes, checkout provenance, label, environment settings and jobs for a
resume. Choose the concurrency once: 16 was reported for V41; a different
machine may need fewer workers. Use a new directory if configuration changes.
It is the caller's job not to replace the binary or retail data during a run.

`run.json` records the run identity and config. `results/<seed>.json` is the
canonical atomic receipt and links to a SHA-256-checked raw log. `results.csv`
is rebuilt from completed receipts after every result; an interruption
between receipt and CSV update is repaired on resume. Ctrl+C cancels queued
work, kills active match processes, keeps their unfinished logs without
claiming a result, and retains completed receipts. A hard process/machine
kill may leave `RUNNING.lock`; only remove it after verifying all runner and
match processes have stopped. Never run two processes into one run directory.

Only a clean exit, matching header and valid terminal marker can count as a
win/loss or tick timeout. Other states are `wallclock` (valid match reached
wall cap), `crash` (signal/Windows exception), `nonzero_exit`,
`protocol_error`, and `launch_error`. The runner reports a nonzero CLI exit
for errors or opening mismatch. These are recorded rather than automatically
retried; investigate and create a fresh run if necessary. The `opening_matches`
flag requires the observed opening signature of the historical run. This is
not proof of identical positions/initial RNG state across engine versions.
The top-level asset inventory is not a full digest of every engine setting.

## Same-engine control

The supplied V42 ZIP has HEAD `40d89f9` and uncommitted V42 work. V41's
historical `ff55d1105` is not in the shallow history. Build a separate worktree
at `40d89f9` and apply only the supplied `same-engine-control-driver.patch`
(the V42 launch CLI flags and V42.1 launch logging). No V42 strategy changes
are in this control patch. Preserve both binaries. Run the benchmark tool
from this candidate checkout for both binaries, with `--source-dir` pointing
to the actual control build checkout for the control and the candidate
checkout for the candidate. Do not copy the entire dirty V42 tree onto the
control. Example from the candidate checkout:

```sh
git worktree add --detach ../vera20k-control-40d89f9 40d89f9
```

In the control checkout, apply the control-driver patch and build using the
same cargo wrapper/profile. In the candidate checkout, run:

```sh
python -m tools.autoplay_benchmark run --suite all --stage smoke --run-dir target/autoplay-results/control-40d89f9 --label CONTROL-40d89f9 --source-dir ../vera20k-control-40d89f9 --binary "CONTROL_BINARY_PATH" --ra2-dir "PATH/TO/YR" --jobs 16
python -m tools.autoplay_benchmark compare --suite smoke --before target/autoplay-results/control-40d89f9 --run-dir target/autoplay-results/v42.1 --output target/autoplay-results/v42.1/smoke-ab.csv
```

The runner compares the exact requested subset; incompatible run configs are
rejected. It reports a four-way outcome transition matrix, missing/error/drift
cells, early defeats, won-to-lost and lost-to-won seeds, paired groups and
sampled diagnostics. The launch receipt logs the requested options (before
engine load); different options in two new runs are excluded from pairing.
An identical Git HEAD or source path is not proof of identical engine code;
verify the actual worktree diff and build provenance before attributing gains.
Use one machine and identical concurrency. Historical comparisons are useful
triage; the same-engine control is the causal test of the V42 package.

## What to inspect in the new logs

- `force_recovery` / `force_recovery_exit`: original V42 entry rule unchanged;
  exit logs include elapsed time, recent peak, remaining points and whether
  the existing threshold or timeout caused the exit. A threshold exit is not
  proof the army is actually safe. Inspect recovery before/after each case.
- `site_release`: a cancellation request, not proof of success.
  `site_release_observed`: the tracked lease was later lost; records whether
  the native held product actually departed, the current line and credits.
  This is not proof of a specific refund; product placement, player queue
  changes or factory destruction can also end a lease. The existing focused
  Rust test checks the native cancellation/refund and now checks lease cleanup.
- `negative_power_samples`, `peak_pts`, and `last_yards`: sampled diagnostics,
  not exact intervals, casualty values, or terminal asset counts. Aggregate
  counts depend on run length. Missing historical event fields remain null.

## Decision gate before V43

1. Run candidate and control smoke; no protocol errors, crashes, or opening
   drift. Examine EB5 (668 V41 warnings) and NewHghts (heavy combat loss);
   confirm the native lifecycle rather than trusting an intent log.
2. Compare all 24 outcomes, preserve good games, inspect each regression.
   Check long forced recoveries and premature threshold exits. Only then
   change recovery policy if evidence justifies it, one change at a time.
3. Extend dev to 120 and report the paired matrix; tune using dev only.
   Do not treat its artificially balanced raw victory rate as the full rate.
4. Freeze parameters, run remaining 982 or the full 1102 (without replaying
   completed cases), and report all three gameplay outcomes plus errors.
   If early losses remain the major gap, begin V43 opening/mobilization.

This release was not Rust-compiled or played on the development assistant's
host (no Rust toolchain or full retail install there). The Python tests and
historical import were executed; that is tooling validation, not gameplay
acceptance. Do not report a new victory rate until the matches are run.
