# V50/V51: economic discipline and observed assault feedback

Development candidate on the delivered V49. All thresholds remain provisional.
No rules, enemy economy, ownership, native production or fog-of-war mutations.

## V50: shared money accounting

* Read the active native factory's unpaid balance and the current prices of
  actual queued types across all six production categories. A paid product
  waiting for placement or delivery owes zero. Queued future prices are
  estimates; the native factory establishes the final price when promoted.
  Include the estimated remaining building-wrench cost, even for a repair
  started by the player. Native vehicle servicing and other incidental future
  charges are not exhaustively forecast.
* A same-frame production proposal is a commitment. Building, defence, wall,
  vehicle, infantry, aircraft, naval and new building-wrench requests consume
  one shared available-cash snapshot. Urgent decisions may release optional
  reserves but cannot re-spend the unpaid native balance. Wait for queued
  native production commands to become observable before ordering another
  batch. Do not cancel or modify existing production to create cash.
* The yard owns the selected construction investment. When a lab/refinery
  or other selected strategic investment is underfunded, protect the full
  intended price, not merely the difference from the current balance.
  Example: 1900 available toward a 2000-credit purchase must not permit
  another 1800 credits of discretionary production. A fully funded and
  queued purchase becomes debt, not a second saving hold. Release a hold
  when its selection or native feasibility disappears. Optional banks require
  a funded purchase or enough measured positive income to cover the gap in
  three game minutes. This forecast never creates spendable credits.
* Prioritise the last miner/MCV and the first operational economy; reserve a
  normal combat vehicle cycle before optional spending on a thin economy.
  The ordinary infantry share ceiling applies when a real vehicle line
  exists, rather than being bypassed by every local counter request. A rich
  wallet with adequate measured income can spend surplus; native urgent
  base defence retains exceptions. Preserve mission-specific specialists.
* Count live aircraft stock (including landed/rearming aircraft) against
  observed completed aircraft-factory dock capacity, in addition to native
  build admission. Counter procurement considers owned stock, not only
  currently available combatants. No new planes solely because existing
  ones are returning. Unknown mod pad capacity defers to native admission.
* Economy rest limits new elective units, construction, wall segments and
  expansion while allowing indispensable recovery, affordable next miners,
  and urgent defence. Already-running factories and repairs finish normally.
  A new building repair is budgeted for the missing HP at native repair-step
  cost. Prefer infrastructure among repairs; never toggle a running wrench.
* Optional second-base investment requires working income, a genuine legal
  maker, a usable expansion site and sufficient force/miners/ore in an
  initialized match; avoid holding the bank for a busy vehicle line.

## V51: targeted observed feedback

* Preserve the original exact assault cohort independently of changing
  operational membership. At exit, count still-owned current health,
  missing original ownership, an estimated replacement value for missing
  units, and net HP-based damage value for surviving units. A unit moving
  into repair or temporarily detached is not automatically a death. Missing
  ownership can include capture; neither value is actual cash spent. Repairs
  between samples are not counted as additional damage. Target progress is
  best observed HP reduction while visible; disappearance is not a kill.
* Failed assault waves on a target retain escalating retry delays of
  900/1800/2700/3600 ticks with bounded memory (7200 ticks since the last
  review). Early reconsideration requires stronger cohort HP (+25%, +50%,
  then +75%) or observed target HP down by at least 25%, and still passes the
  V49 forecast. A strategic interruption or vision loss is not a new failed
  assault; preserve existing stronger failure memory across interruptions.
* This is a narrow feedback loop for committed group assault, not a general
  economic exchange predictor or cross-domain AI learning. The original
  engagement forecast remains heuristic; do not infer hidden target HP.

## Diagnostics and acceptance

The periodic `economy_budget` marker records cash, native cumulative spent
credits, estimated outstanding debt, strategic bank, proposed purchase cost
and rest. The parser reports sample counts and observed spent-counter delta,
not a full lifetime expenditure receipt or a count of completed purchases.
`assault_feedback` records cohort exit observations, and `combat_feedback`
marks availability of this telemetry even in a game with no assault exits.
Historical logs without new markers retain missing fields rather than zeros.

V52 should compare the exact delivered V48 control, V49 combat candidate,
and V51 economic+feedback candidate on the same engine, with same resources,
seed and wall-clock settings. These are sequential contrasts, not a complete
2x2 factorial ablation: V51 contains V49 and the economic/feedback changes
share decision paths. Keep the immutable V41 1102-case manifest and old
120-case V48 results. Do not overwrite old receipts. Use the same current
runner for newly generated comparisons. First verify compilation and all
relevant Rust tests including fixture availability. Then run targeted native
factory, tower and repair/production checks, followed by the fixed 120-case
suite. Only expand to all 1102 cases if the developer suite justifies it.

Provisional acceptance: exceed the 30 V48 wins (target >=36), at most 60
losses and 24 early losses, no obvious regression in native delivery or
protected specialists, less repeated nonproductive movement/vetoing, actual
useful purchases under a sustainable cash balance, and no marked throughput
regression. Compare per-seed outcome transitions and actual target damage;
new logs/less spending alone cannot prove improvement. Campaign saves are
still needed to reproduce the user-reported campaign behaviours.

## Known limitations

No Rust compiler or retail environment on the assistant host; no compiled
or actual gameplay result. Native debt is a conservative estimate and not
an exhaustive engine financial accounting model. The three-minute savings
horizon, stock counts, reserves and retry thresholds may overconstrain
production or attacks. Higher observation cost is possible. A full
factorial feature-ablation configuration, CPU profiling and calibration
remain V52 work; no stable-release claim until actual verification.
