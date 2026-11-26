# Matrix evaluation: many factions, many opponents, many maps

> For V42.1 and later acceptance, use the [manifest-based benchmark](benchmark-v42.1.md).
> The legacy matrix mode below is for ad hoc use; its batch CSV/history output
> is not a durable interruption-safe receipt and does not guard version identity.

`tools/autoplay_check.py` runs one pinned match. This page is the other
shape of the same question: a whole matrix, thousands of matches, one row
each, and the pivot tables that say which cell moved.

The driver is the same script. Nothing was forked: the scratch driver that
produced the v41 numbers lived in `_trash/` and is now folded into
`tools/autoplay_check.py` behind `--csv`, `--resume` and `--report`.

## Quick start

```bash
# the pinned single match (unchanged behaviour)
python -m tools.autoplay_check

# a matrix: 10 factions x 10 opponents x 40 maps, 1W opening, no units
python -m tools.autoplay_check \
    --maps "$(python -c 'import glob;print(",".join(p.split("\\")[-1] for p in glob.glob(r"I:/SteamLibrary/steamapps/common/Command & Conquer Red Alert II/*.mmx")))")" \
    --local america --countries russia,yuri,iraq,cuba,libya,germany,france,greatbritain,korea,america \
    --credits 10000 --units 0 --difficulty hard --opponents 1 \
    --ticks 200000 --wall-seconds 900 --jobs 16 \
    --csv target/autoplay-results/matrix.csv --resume --skip-build --skip-tests

# re-read a finished matrix without running anything
python -m tools.autoplay_check --report target/autoplay-results/matrix.csv
```

`RA2_DIR` must point at a retail install or every match is skipped.

## Flags

| Flag | What it does |
| --- | --- |
| `--local` | the faction the assist brain drives (default `america`) |
| `--countries` | **the computer's** factions. Kept for compatibility; `--opponent-countries` is the honest name and wins when both are given |
| `--opponent-countries` | comma separated computer factions; defaults to `--countries` |
| `--opponents` | how many computer houses per match (1 for a duel) |
| `--credits` | opening credits; omitted means the match binary's own default |
| `--units` | opening units per side; `0` is the build yard alone |
| `--difficulty` | `easy` / `normal` / `hard` (冷酷) |
| `--ticks` | frame cap. `200000` is "no timeout" in practice |
| `--wall-seconds` | per-match wall clock cap (default 900). A capped match is recorded as `wallclock`, never as a result. `0` removes the cap |
| `--jobs` | matches at once. A match is itself multi-threaded; `cores/3` is the measured optimum, not one per core |
| `--maps` / `--map` | comma separated maps, or one map |
| `--seeds` | comma separated seeds, run side by side. Hex is accepted and converted |
| `--csv` | append one row per match here |
| `--resume` | with `--csv`, skip the cells that file already holds |
| `--report` | print the pivot tables from a CSV and run nothing |
| `--skip-build` / `--skip-tests` | use the binary as it is / skip the assist test group |
| `--history` / `--baseline` / `--compare-only` | the one-change-per-measurement loop, unchanged |

## The CSV

One row per match, appended, so a long matrix survives a stop:

```
map, local, opponent, seed, outcome, tick, own, enemy, credits, power, seconds
```

- `outcome` is `won`, `lost`, `timeout` or `wallclock`.
- `own` / `enemy` are the last census counts; `credits` / `power` are the
  last status line's readings.
- `tick` is where the match ended, or where the wall cap caught it.

`--resume` reads the `(map, local, opponent, seed)` cells the file already
holds, so a matrix that was stopped at 08:00 can be continued at 09:00
without replaying 1000 matches.

## The report

`--report` prints four pivots and the pairing extremes:

```
matches 1102 decided 1002 won 293 wallclock 100
=== by local faction ===   === by opponent ===
=== by map ===             === by pairing (n>=8) ===
```

Read them in that order. A single win rate hides which part of the matrix
moved it, and the parts disagree: a faction that is 60% against one opponent
and 10% against another is not a strong faction.

Two columns do the diagnostic work:

- **win rate next to average tick.** A low rate with a low tick is a house
  that loses fast (an opening hole). A low rate with a high tick is a house
  that is ground down (a sustain hole). v41's Russia read 21.1% at 40445
  ticks — fast losses — while Cuba read 15.3% at 39962.
- **credits at the end of a loss.** A loss with money in the bank is a
  production or supply break, not a shortage. In the v41 matrix the median
  loss ended with 8295 credits.

## Reading a matrix without fooling yourself

- **A wall cap is not a result.** `wallclock` rows are excluded from the win
  rate. They are reported separately because a high wallclock rate is a
  finding in itself: v41 left 9.1% of matches undecided.
- **Seeds are decimal.** `--seed 0x1001` fails to parse; the script converts
  hex for you, but a hand-written `--seed` argument to the match binary must
  be decimal.
- **The map list is the retail install's, not the repository's.** The `.mmx`
  files live beside `RA2_DIR`. A Mental Omega install answers to the same
  directory and its `RA2MD.INI` is not retail, so the retail-data tests fail
  there while the matches still run.
- **`--countries` is the opponent.** This is a wart kept for compatibility.
  New calls should pass `--local` and `--opponent-countries`.
- **Pass a Windows path to `--csv`.** An MSYS path such as
  `/tmp/matrix.csv` is translated by the shell into a temp file with a
  different name, and the next `--resume` will not find it. A relative path
  (`target/autoplay-results/matrix.csv`) or a `E:/...` path is passed through
  unchanged.
- **One match per process, and the pool is the matrix.** Do not raise `--jobs`
  past `cores/3`; the match itself uses the cores it can get.

## The v41 baseline (1002 matches, 2026-11-25)

Kept here so a later matrix has something to be compared against. 1W
opening, 0 units, hard, superweapons on, 16 jobs, 900 s wall cap.

| Cut | Result |
| --- | --- |
| overall | 29.2% won (293/1002), mean tick 52716, 9.1% undecided |
| local faction | america 51.5% … libya 18.4% |
| opponent | korea 67.7% (easiest) … yuri 9.0% (hardest) |
| map | BayOPigs 54.5%, Death 51.4% … Arena 8.1% |
| loss shape | median loss ends with 8295 credits and power 0 |

The full write-up, with the turning points read off individual logs, is
`G:\Documents\autoplay-v41-对战评估记录.md`.