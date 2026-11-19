# Handoff

Where the v2 policy stands, 2026-11-04 07:40. The chronicle is `status.md`; the
roadmap is `plan.md`; what a tester should look at is `playtest-plan.md`. This is
what to read first and what to do next.

## The state

Branch `feature/autoplay-v2`, working tree clean at `71f7389f7`.

- **Tests: 34 of 34 pass.** (`cargo test -p vera20k-autoplay --lib`)
- **Harness: 14 tests pass.** (`python -m unittest tools.tests.test_autoplay_check`)
- **1v1 easy, six cells, 12000 ticks** — own 45-57, `stuck` 0, six of six survive.
- **1v7 easy, Death.mmx seed 0x5EED, 24000 ticks** — own 84, enemy 274, `lost 1:9`,
  survives. Three seeds: own 84 / 90 / 58.
- **Nothing wins.** Every lane ends `timeout` with no enemy base destroyed.

## The two readings that did not exist before this session

Both are in `policy.rs` and both were added because every earlier judgement ran
without them.

1. **Per-type losses.** `policy v2 losses: tick=… so_far={ADOG: 14, FV: 11, …}`,
   printed every 3600 ticks. First reading: 40 objects lost in a 1v7 match, of which
   **25 were attack dogs and IFVs** against three tanks. See `losses.md`.
2. **Unit position and base.** The unit line carries `at=(x,y)`; the military line
   carries `home=` and `door=`. First reading: the army's whole footprint stayed
   within about thirty cells of a base at (69-80, 155-165). See `no-sortie.md`.

**Both readings have a known defect**: `at=(0,0)` appears for units that also appear
elsewhere, so the origin is an invalid value and not a position. Any conclusion read
from positions must exclude it first. Cause uninvestigated.

## What was tried and reverted this session

Nine attempts, all measured, all reverted. The list with numbers is `excluded.md`;
the short version:

| attempt | 1v7 own |
|---|---|
| per-unit target selection | 13 |
| narrow pairing fix (only the dog re-aimed) | 26 |
| garrison, priority 30 | 17 |
| garrison, gated | 17 |
| second war factory | 39 |
| return-home leash 20 → 40 cells | 58 |
| home = densest cluster + quiet at home | 55 |
| baseline | **84** |

`excluded.md` draws the conclusion: the baseline sits on a narrow balance — column
concentrated, soldiers at home, army not roaming, one factory — and moving any of
those four takes the house apart.

## What is live

- **Capture works.** `capture: engineer 2778 took building 2608 for Player` appears in
  the log; `Capture` fires 1-3 times per cell. Nearest-first ordering is what made it
  land (`44f945c2e`).
- **The engine reports captures.** One line at the ownership change makes "captured"
  and "died walking" distinguishable (`cae13c2b0`).
- **Garrison is implemented and disabled** at the call site, because seven garrison
  jobs on the 1v7 lane cost the base (`0720ec741`).

## What to do next, in order

1. **Fix the `at=(0,0)` defect.** It undermines every position reading, including the
   one that produced this session's central finding.
2. **Find why going out costs more than staying.** Correcting `home` (a captured
   building had made it point at the enemy) and letting the column march when the base
   was quiet both cost `own` — 84 → 55 — even though `march=true` finally appeared.
   That is the opposite of what the wish list wants and it is uninvestigated.
3. **Do not** add factories, add income, widen the leash, or re-aim units. All four
   are measured and all four are worse. See `excluded.md`.

## Still open from the wish list

Ship, breach, infiltration, superweapon use, building abilities, combinations, kiting,
retreat, ammunition sharing. `plan.md` has them in four batches with the criteria each
would be judged by.

## A caution about this session's own record

Two claims were overstated and corrected in place:

- "the army never left the base" — units did reach thirty cells out; the accurate
  statement is that its range was small.
- the loss reading was treated as clean before the origin defect was found.

Treat the numbers in these docs as measurements taken with the tools described, not as
established fact. The tools have been wrong twice.
