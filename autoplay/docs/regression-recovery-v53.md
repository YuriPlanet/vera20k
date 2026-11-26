# V53 — Regression Recovery (skirmish AutoPlay)

User priority: recover ordinary AutoPlay initiative, map control and effective
combat. Do not modify or interpret the campaign harness this cycle.

## Verified historical evidence
The uploaded V52 skirmish receipts and referenced logs were re-read using
the unchanged fixed-suite runner. All 120 fixed development cases matched
the manifest and receipt/log integrity checks: 13 wins, 84 losses, 2 tick
timeouts, 21 wall-clock limits, 26 losses before tick 30000, observed
median throughput 202.12 ticks/s. V41 and V48 historical references each
had 30 wins. These results are NOT V53 gameplay results.

## Changes and intended invariants
- Cash-only economy rest is disabled. Contain/Stockpile are strategic
  advice instead of a main-army attack veto; no periodic forced retreat
  from an otherwise intact committed force. A >50% drop in recorded
  commitment strength still triggers bounded refit (not proof of deaths).
  Real recovery, mobilization, home protection, local force comparison,
  specialist ownership and factory handoff retain their existing gates.
- Remove the speculative counter and expensive-vehicle all-line saving
  stalls. Affordable real counters may still be purchased, using the same
  ledger and stock limit. Construction commitments remain protected; a
  positive remainder is not blocked again by the presence of a bank.
  Optional siege/scarce technology must be affordable or fundable within
  one minute (old: three). This shorter horizon applies to the optional
  override of the regular build plan, not all economic investment.
- A visible, unclaimed and site-admissible neutral asset or oil opportunity
  can recruit its actual native-buildable engineer from the remaining
  budget, before ordinary composition. Remove the arbitrary 1500-credit
  and scout requirements for these opportunities, use a one/two-engineer
  cap and respect target cooldown, actual native queue, emergency and
  income recovery. A clear neutral site is not subject to the additional
  remote hostile-prize escort quota. Engineers use actual weapon envelopes
  for route risk, while the endpoint/prize check and final safety review
  remain. A purchase is not a confirmed capture; a threatened or
  unreachable destination still needs clearance or a different route.
- Armour wave admission is recalibrated: minimum 3 eligible vehicles,
  initial health >=70%, forecast loss <=45%, observed remaining HP >=55%
  and at least 3 active members. Preserve concentrated formation,
  rule-derived damage/armour, cash-exchange bound, invulnerability, mind
  control, progress deadline, retry and task ownership. Remove the
  additional Stockpile-only 20% forecast ceiling. These are provisional
  risk calibration values, not empirically validated optimal values.
- Final front review can preserve an in-range attack on an operational
  defensive battery by a healthy ordinary vehicle with at least two
  nearby vehicles also able to fire at that exact target now. No new
  pursuit permission, no general immunity to danger, no exemption for
  known mind control, fragile casualties, ended charges or specialists.
  This is local readiness evidence, not a promise that allies will fire.
- If the final charge roster is too small, remove its matching attack
  proposals before ending it. Keep unrelated jobs and native safety.

## Test order
1. Compile and run the assist Rust regressions; stop on compile errors or
   failed fixtures. No Rust toolchain was available in the delivery environment.
2. Run the existing 24-case smoke and 120-case fixed dev set with the
   same actual binary, retail data, engine, runner, 16 jobs and hardware.
   The six shared regression examples (V41/V48 won, V52 lost) are 20024,
   20150, 20218, 20591, 20688, 20824; inspect them in the full dev set.
3. Compare V53 against V52 and historical V41/V48. A recovery candidate
   should first regain at least 30 wins, <=60 losses and <=30 early
   losses, with no material throughput regression versus 202.12 ticks/s
   under comparable controls. The stronger existing acceptance gate
   remains unchanged (>=36 wins, <=60 losses, <=24 early losses, <=24
   wallclock, >=192.58 median throughput, 120 valid cases).
4. Manually check tank group attacks against towers, withdrawing injured
   individuals, no unapproved lone entry, no excessive infantry loss,
   neutral oil/technology capture, occupied hostile fire lanes, no
   duplicate engineer purchases, first/last miner recovery, construction
   budget and rich-base conversion to armour. Use actual native captures
   of neutral assets where ownership evidence is available, not all
   generic engineer capture events as a proxy. Check local-counter and
   front-guard logs per elapsed tick, not raw totals alone.
5. Do not run campaign or the 982-case holdout until the development
   comparison and tactical checks justify it. Do not tune only the six
   examples or call improvement proven by source changes.

## Boundaries
The new safety and production policies need actual Rust and gameplay
verification. There is no independent feature-ablation implementation,
and no new campaign behavior/tester work. Original V41 manifest and
benchmark runner bytes are retained.
