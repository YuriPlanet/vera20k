# Initial prototype versus final optimized interpolation — 2026-10-20

## Direct comparison, not a sum of per-round percentages

I reran the retained pre-optimization prototype and final fifth-round executable
in one alternating comparison. Both are Release-optimized test executables:
"initial" means the interpolation implementation before its five optimization
rounds, **not** an opt-level-0 build or upstream with interpolation absent.

The primary result is now initial versus final. Individual round reports remain
useful implementation history, but their percentages are not added together.

## Method

- Intel Core i5-12600KF, Windows 11, Rust 1.97.0.
- The same ten cases, retail MTNK construction, synthetic committed transforms,
  interface-call bundle and virtual-time schedule in both executables. The timed
  probe source is unchanged; the only source addition is an independent
  `pose_read_tests` module, outside this test's timing path.
- Initial → final → final → initial, twice: four complete runs per executable.
  Each run contains ten cases with eight alternating ON/OFF pairs, 128 warmup
  and 768 measured iterations per mode.
- 1,280 records and 983,040 measured iterations retained. Every case/mode has
  32 run means per executable. Tables take the median of those run means;
  P95 and maximum rows take the median of each run's statistic unless explicitly
  labeled as the worst individual sample.
- All 640 corresponding cross-binary records have identical final authoritative
  hashes. Each executable also checks ON/OFF hashes, read-only observation and
  genuine intermediate positions rather than accidental bypass. The reducer
  checks identical case parameters, iteration counts, point counts and per-step
  commit/repeat schedule. Existing field-equivalence tests remain separate.
- No recompilation, GPU capture or parallel benchmark during measurement. Normal
  desktop background activity remains; no outliers were removed.

## Main result: identical ON workload, initial versus final

All rows below replay 200 presentations/s and 31.25 commits/s. Units are ms per
measured presentation-style iteration. This includes the declared interpolation
capture/sampling interface bundle, **not a whole game frame or actual screen FPS**.

| World / visible objects | Initial ON | Final ON | Measured CPU time reduction |
| --- | ---: | ---: | ---: |
| 100 / 100 | 0.01936 | 0.01739 | 10.2% |
| 1,000 / 250 | 0.05852 | 0.05054 | 13.6% |
| 5,000 / 500 | 0.19138 | 0.14790 | 22.7% |
| 20,000 / 500 | 0.69456 | 0.44810 | **35.5%** |
| 1,000 / 1,000 | 0.22580 | 0.20025 | 11.3% |
| 1,000 / 250 plus 320 frozen smoke/system points | 0.09172 | 0.08377 | 8.7% |

The 20k run-mean ranges are 0.68340–0.75912 ms initially and 0.44063–0.53270 ms
finally. This supports a reduction in this controlled workload, not a universal
percentage for every machine, game speed or battle.

### 20k capture and tail measurements

| Metric | Initial | Final |
| --- | ---: | ---: |
| New-logic-frame capture mean | 3.62374 ms | 2.11743 ms |
| Paired ON−OFF capture cost per commit | 3.62366 ms | 2.11738 ms |
| Capture P95 | 3.95285 ms | 2.24540 ms |
| Capture maximum, median across runs | 4.27675 ms | 2.48010 ms |
| Worst individual capture sample, all runs | 5.78890 ms | 4.77560 ms |
| Interface-bundle P95 | 3.88405 ms | 2.32805 ms |
| Interface-bundle maximum, median across runs | 4.53785 ms | 2.74295 ms |
| Worst individual interface-bundle sample, all runs | 6.05680 ms | **7.92990 ms** |

Capture mean decreased **41.6%**, saving about **1.51 ms per capture**; capture
P95 decreased 43.2%. The interface-bundle P95 decreased 40.1%. Capture occurs
only on new logic frames, so its per-commit time must not be added to the
already-amortized per-presentation mean above.

The final executable has a worse single interface-bundle outlier in this run.
It is retained and included in the maximum. I do not claim that every spike was
eliminated or that these measurements impose a worst-case latency bound.

### ON/OFF reference points also changed

The disabled path was optimized too. Initial/final ON time is the direct
implementation comparison; it is different from the cost of enabling the
feature relative to each executable's own OFF path.

For 20k/500, OFF time is 0.05433 → 0.02370 ms; the paired ON−OFF mean increment
is 0.64021 → 0.42466 ms (33.7% lower). For 1k/250, ON time falls 13.6%, but
OFF improves enough that the ON−OFF increment increases from 0.03665 to
0.04264 ms. The smoke and 1k/1k rows have a similar reference-point effect.
This is not evidence of an ON regression, nor a reason to call the remaining
interpolation overhead zero.

## Scope and limitations

This is the unchanged general interface probe, not a complete measurement of
all draw consumers. The separate SHP and aircraft local-reuse probes measure
different call combinations and are not added to this table or its percentages.

The world consists of retail-constructed objects with controlled transforms,
not a legal 20k-unit battle. AI, movement simulation, collision, pathfinding,
full instance construction, GPU, driver/VSync, actual input, frame pacing,
memory peaks and whole-game FPS/1% lows are not timed. The smoke objects are
real constructed objects with frozen state, not timed particle physics.

The frame-rate numbers describe the replay schedule, not achieved display FPS.
The normal opt-level-1 game binary is also not one of these timed Release test
executables. Visual smoothness and the Windows startup stack change have their
own acceptance evidence; neither is proved by this CPU probe.

## Retained evidence

Complete ten-case statistics are in
[`render-interpolation-cumulative-20261020.json`](render-interpolation-cumulative-20261020.json).
Local raw logs and runner/reducer scripts are retained under
`logs/interpolation-cumulative-20261020/`: `before-r1..4.log`, `after-r1..4.log`,
`runs.json`, `metadata.json`, `run-comparison.ps1` and `summarize.ps1`.
No executables or retail game assets are committed.

- Initial executable SHA256:
  `30986A3B73F804D2BEA58811A1D342DD4F8BD2D0FC2BE4E69293F1BECF99CB58`.
- Final executable SHA256:
  `798C0DC7A899D8014398C943856EB766BE337CE3875EC231D34B4C459628A096`.
- Test: `app::presentation::interpolation::perf_tests::render_interpolation_paired_cpu_probe`,
  run with `--ignored --exact --nocapture --test-threads=1` and required retail
  INI/assets checks. No test assertions or workload were weakened for this run.
