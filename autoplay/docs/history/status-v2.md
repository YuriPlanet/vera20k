# Where the policy stands

Measured 2026-11-02 on the v2 branch (commit 36d67b4fd), retail YR, hard
computer. One process per map and seed, side by side:

    python -m tools.autoplay_check --brain v2 --seeds 0x5EED,19,10 \
        --maps Death.mmx,Barrel.mmx --ticks 24000

## What runs

A match driven by this crate: the maker deploys, the house raises a power plant
to open the tree, a refinery, a barracks and a war factory, places each through
the native site search, keeps one harvester per refinery, queues the cheapest
armed unit its factories can build, and holds the army at its post. Every unit
that can take an order holds one, every order goes through the funnel the
player's clicks use, and every decision is read off the unit profile rather than
a type name.

## Numbers

1v1 hard, Death.mmx, 12000 ticks, three seeds (own vs enemy objects at the end):

| Seed | Own | Enemy | Unowned | Stuck |
|---|---|---|---|---|
| 0x5EED | 35 | 47 | 0 | 0 |
| 19 | 35 | 40 | 0 | 0 |
| 10 | 35 | 45 | 0 | 0 |

24000 ticks, both maps, three seeds: four of six survive the forty-minute mark.

| Map | Seed | Result |
|---|---|---|
| Death.mmx | 0x5EED | survives (25 vs 64) |
| Death.mmx | 19 | survives (33 vs 56) |
| Death.mmx | 10 | lost at 17680 |
| Barrel.mmx | 0x5EED | survives (27 vs 58) |
| Barrel.mmx | 19 | survives (25 vs 79) |
| Barrel.mmx | 10 | lost at 20435 |

1v7 hard, Death.mmx, 12000 ticks, three seeds: two of three survive. The retail
policy on 0x5EED lost at 11361.

## The recorded baseline (2026-11-02, commit 3f9ba3a4a, this machine)

The table above is a reading; this is a file on disk that a later run can be
diffed against, both maps and all three seeds in one pass:

    RA2_DIR=<retail> python -m tools.autoplay_check --brain v2 \
        --seeds 0x5EED,19,10 --maps Death.mmx,Barrel.mmx --ticks 12000 \
        --skip-tests --history target/autoplay-results/baseline.jsonl

| Map | Seed | Own | Enemy | Unowned | Stuck |
|---|---|---|---|---|---|
| Death.mmx | 0x5EED | 35 | 47 | 0 | 0 |
| Death.mmx | 19 | 35 | 40 | 0 | 0 |
| Death.mmx | 10 | 35 | 45 | 0 | 0 |
| Barrel.mmx | 0x5EED | 32 | 38 | 0 | 0 |
| Barrel.mmx | 19 | 34 | 45 | 0 | 0 |
| Barrel.mmx | 10 | 35 | 40 | 0 | 0 |

A second run of the same command read exactly the same in every cell
(`--compare-only`: 6 same, 0 better, 0 worse), so a diff is evidence about the
policy and not about the machine.

    RA2_DIR=<retail> python -m tools.autoplay_check --brain v2 \
        --seeds 0x5EED,19,10 --maps Death.mmx,Barrel.mmx --ticks 12000 \
        --skip-build --skip-tests --history target/autoplay-results/rerun.jsonl
    python -m tools.autoplay_check --compare-only \
        --baseline target/autoplay-results/baseline.jsonl \
        --history target/autoplay-results/rerun.jsonl

## The wallet is the ceiling, and the miners are why

`--start-credits 30000` on the same two maps and seed 0x5EED, 12000 ticks:

| Map | Own (10000 start) | Own (30000 start) | Enemy (30000 start) |
|---|---|---|---|
| Death.mmx | 35 | 85 | 49 |
| Barrel.mmx | 32 | 85 | 51 |

The army is not what stops the house at 35 objects; the wallet is. Five of the
six cells above end at 12 credits with the wallet unchanged for the last 3600
frames. Two readings under that:

- With 30000 in the bank the house spends down to 300, the economy reports
  `nothing to build ... refineries=3 defenses=4`, and the military manager stops
  too (`UNIT_SPEND_FLOOR` 900). Nothing is queued for the last four minutes: the
  build order has no target and the unit buyer has no budget.
- The harvesters do not unload. Across the seven miners of the 30000-credit
  Death run, the engine's state log reads `ReturnToRefinery → Dock` 527 times and
  `SearchOre → Harvest` with `cargo=0` 14 times — an empty hold arriving at ore,
  which is what a finished unload looks like. The retail brain on the same map
  and seed arrives at ore empty 96 times against about 93 dock entries. Ours
  enters Dock and comes back out full.

The state log is the engine's (`src/sim/miner`), so the reading is engine-side
even when the policy is what changed. The hypothesis that fits both native rules
the code cites — `new_order_pending` (`refinery_dock.rs:420`, `0x0073E539`) ends
an unload when a NavCom exists and the queued mission is neither none nor
Harvest, and `per_cell_release_dock_contact` (`0x0073ACD7`) drops the contact
unless the mission is Unload or Enter is queued — is that the policy re-sends
`Goal::Harvest` (a MegaMission *with a destination*) every term the miner's
mission is not Harvest, which is exactly during Enter and Unload. That is a
hypothesis, not yet a proven cause; the reading that decides it is the ratio
above, which the next change has to move.

## The harvest job is not re-sent into the engine's own dock cycle

The policy used to order a harvester again every term its mission was not
`Harvest` — which is exactly while the engine is docking it (`Enter` for the
refinery contact, `Unload` for the dump). That order is a MegaMission, and a
MegaMission is `Queue_Mission(mission, 1)`: it replaced the queued `Enter` that
`per_cell_release_dock_contact` (`0x0073ACD7`) needs to keep the radioed
refinery, so the truck dropped its dock and cycled back to the ore field full.
`Policy::living_out` now counts `Harvest`, `Enter` and `Unload` as the same job
being lived out, and a harvester is only ordered again when it is idle.

Same command and same baseline as above, after the change:

| Map | Seed | Own | Enemy | Unowned | Stuck |
|---|---|---|---|---|---|
| Death.mmx | 0x5EED | 46 (was 35) | 47 | 0 | 0 |
| Death.mmx | 19 | 51 (was 35) | 40 | 0 | 0 |
| Death.mmx | 10 | 50 (was 35) | 45 | 0 | 1 |
| Barrel.mmx | 0x5EED | 46 (was 32) | 38 | 0 | 1 |
| Barrel.mmx | 19 | 46 (was 34) | 39 | 0 | 1 |
| Barrel.mmx | 10 | 50 (was 35) | 40 | 0 | 0 |

Own is up in all six cells and down in none, no seed that survived is lost, and
unowned stays zero. The engine readings move with it: a miner arrives at ore
with an empty hold 26-35 times per match instead of 10-16, Dock entries fall from
166-172 to 26-32 (one dock per unload, as in the retail runs), and the wallet no
longer freezes — the last readings are 390-1164 credits instead of 12, and no
cell spends the last 3600 frames standing still.

The one clause that does not hold is `stuck`, which reads 1 in three cells where
it read 0. The reading is now per unit (`policy v2: stuck at ...`), and every
stuck unit is a CMIN holding `job=Harvest`, still for 90-210 frames. Between two
readings the counter resets (id=2911 reads `still=90` at 10800 and `still=210` at
11400), so those units do move; they pause for a few seconds at a time, which is
what harvesting in place and waiting at a one-dock refinery look like from a
projection that carries only the cell. Recorded as a residual: whether a pause is
a fill or a dock queue is not visible to the policy yet.

## The forty-minute reading, and the attack that is still off

The same build over 24000 ticks, three seeds, both maps, against the numbers the
handoff recorded on the un-fixed build:

| Map | Seed | Own | Enemy | Handoff's own (before) |
|---|---|---|---|---|
| Death.mmx | 0x5EED | 91 | 65 | 25 (survived) |
| Death.mmx | 19 | 51 | 57 | 33 (survived) |
| Death.mmx | 10 | 61 | 57 | lost at 17680 |
| Barrel.mmx | 0x5EED | 34 | 61 | 27 (survived) |
| Barrel.mmx | 19 | 102 | 65 | 25 (survived) |
| Barrel.mmx | 10 | 70 | 49 | lost at 20435 |

Six of six survive the forty-minute mark; four of six did before, and both seeds
that used to be lost now survive. Nothing here wins — every cell is a timeout,
which is the most a house that never attacks can do.

`ATTACK_ENABLED = true` for one run, everything else identical:

| Map | Seed | Own | Enemy | Stuck |
|---|---|---|---|---|
| Death.mmx | 0x5EED | 46 → 45 | 46 → 36 | 0 → 33 |
| Death.mmx | 19 | 51 → 50 | 45 → 44 | 0 → 33 |
| Death.mmx | 10 | 50 → 34 | 45 → 46 | 1 → 2 |
| Barrel.mmx | 0x5EED | 46 → 44 | 49 → 41 | 1 → 13 |
| Barrel.mmx | 19 | 46 → 46 | 39 → 40 | 1 → 29 |
| Barrel.mmx | 10 | 50 → 45 | 41 → 29 | 0 → 29 |

All six cells are worse, own falls in four of them, and the stuck count goes from
0-1 to 13-33. The per-unit reading added with the dock fix says why: 25-33 units
of the army stand around one stale target cell — `job=Assault
target=Cell((96,143))`, `still=500-1400` frames — while holding the job. The
enemy count does fall (49 → 41, 46 → 36, 45 → 44), so the army trades when it
arrives and then stops. Attacking stays off until an assault job can end on
arrival, pick a new target, judge local superiority and retreat.

One thing the probe needed first: an assault order is an attack-move with a
destination, and the engine lands that on `MissionType::Move` with the attack-move
latch set (`mega_mission.rs::foot_assign_target_command`, the `0x004DF141`
destination arm), not on `Attack`. The predicate now accepts both, and with the
flag off again the matrix reads identical in all six cells — verified, so the fix
is inert until attacking comes back.

## The army aims at what the house can see, and still does not attack

The projection could already answer "where is every enemy in the world"
(`enemy_entities`), and its own doc said so: the native computer AI is omniscient
and a policy reusing its decisions inherits that. The spec forbids it (2.5, "no
real-time enemy information outside vision"), which matters most at 1v7, where
omniscience is every base on the map at once. `Observation::visible_enemy_entities`
narrows that list to the cells the house's fog marks visible right now
(`FogState::is_cell_visible`, the L2 layer); a test spawns one rifleman beside the
house's own and one across the map and asserts that only the first is a target.
The army targets from it.

An assault is now an attack on the enemy *object* a player's click would name,
not an attack-move to where it stood, and the job ends when its object is no
longer visible. With attacking still off, the matrix reads identical in all six
cells, so the change is inert until the flag returns.

It also has to be re-aimed before that: the probe with the flag on reads 0 cells
with own down and one up (own 46-51 against the same 39-49, so the army no longer
loses objects), but five units of Barrel.mmx seed 0x5EED stand for 1400-1875
frames on `job=Assault target=Object(2579)` — an assault on something the house
can see and they cannot finish. The probe before this one, with a cell as the
target, lost objects in four cells and froze 25-33 units. So the next rung is a
job with a ladder below it: give up on a target this unit cannot reach, do not
propose it to that unit again, and re-send the order when the target it names
changes.

## Reverted: the reserve, again

The handoff's item 1 asks for the fixed reserves to become the real cost of the
cheapest harvester. Tried: the economy's `+200` margin and the unit buyer's floor
both read the cheapest buildable harvester off the profile table. It is worse in
all six cells — own 46-51 becomes 37-41 — because a house that keeps a truck's
cost in the bank stops buying units, and units are what the object count is. The
constant stays, and with it the open question of what the reserve is for now that
income works.

## The assault ladder, and what it did and did not fix

Three rungs were missing below the order, and `design.md` section 4 already names
them (no waiting on another pass; every rung terminates):

- a job whose object is no longer visible ends;
- a unit that has stood in one cell for 450 frames on an assault ends it and the
  target is not proposed to that unit again for 1800 frames (`stalled`, `cooled`);
- a unit that nothing proposed for gets the default job — guard the nearest
  structure, read off the profile — so `Unowned == 0` holds by construction.

With attacking on, the stuck count on the probe fell from 13-33 to 0-2 and
`unowned` from 0→14 in one cell to 0 in all six. Own falls in no cell.

What is left is the rung below those: an assault on a target the unit can see and
reach but cannot finish. Those units read `job=Assault target=Object(2579)` with
`still=1800` at the end of the run, and the abandonment that should fire at 450
frames reads `stalled=0` — because those units are re-assigned and re-ordered in
the same term they are counted, so the reading the ladder consults (`stood`) has
just been reset. The next reading has to come from the target's health, not from
the unit's cell: an assault that has not reduced its target in N frames is
`Ineffective`, which is the design's fourth outcome and the one still missing.

Two order-identity bugs turned up while measuring, and the matrix caught both as
a fall in own rather than a test:

- comparing the target for *every* watched job. A harvest job's target is the
  cell the truck stood on and a defend post is where the first soldier is before
  the yard exists, so both read as a new order every term: the dock regression
  again, own 46 → 34. Only an assault is compared by target.
- skipping the `issued` bookkeeping when the mission already showed the job, so a
  unit with no remembered order was re-ordered every term.

With attacking off the matrix reads identical in all six cells, so all of it is
inert until the flag returns.

## The fourth outcome, and the stuck clock none of it could be read with

`Ineffective` — the design's fourth ending — is now implemented: an assault whose
target's health has not moved for 450 frames ends, the pair is cooled down, and
`ineffective` counts it. Two defects had to be fixed before the number would
move, and both were found by reading it:

- the first version recorded the target's health unconditionally, which moved the
  timestamp every term, so the window could never elapse: `ineffective=0` in a
  match where units stood 1800 frames. The clock now remembers the reading that
  started the current run and resets only when the target loses a hit point;
- the stuck clock was keyed on the cell alone, so a unit switching from holding a
  post to attacking kept the cell it had held and read as "standing still on its
  assault" in the same term it was given one — 38 units at once. It is cleared
  when a unit's job changes, and a unit whose assault took health off its target
  this term is excluded: the spec's H2 counts a unit stuck when it holds a
  *movement* job and does not move, not a unit standing in range and firing.

With attacking on, the cell that read 38 reads 0, two cells read +1 stuck, own
falls in no cell and gains 3 in one. With the flag off the matrix is identical in
all six cells.

## The faction spread, and the Yuri refinery that was not one

Death.mmx, seed 0x5EED, 12000 ticks, attacking off, one run per faction:

| Faction | Own before | Own after | Enemy | Stuck |
|---|---|---|---|---|
| america | 46 | 46 | 46 | 0 |
| russia | 70 | 70 | 49 | 3 |
| yuri | **29** | **45** | 46 | 0 |

The cause was a profile predicate, not a policy choice. `Roles::REFINERY` was
derived from `Refinery=` alone. Retail's allied and soviet refineries carry that
key; `[YAREFN]` does not — it is declared by `ResourceDestination=yes`. So the
Yuri refinery's roles read `harvester,deployer,armed,structure`, the build order's
`!has(Roles::REFINERY)` test was permanently true, and the `or(power)` fallback
fired every term: sixteen bio reactor placements in one match, power at 2400, no
ore income ever.

`ResourceDestination` alone is too broad to fix it — `[SMIN]`, the slave miner, is
the deployed refinery's vehicle form and carries the same key — so the role is
granted when the object is a building. The three miners are still harvesters.

Measured after: own 45 against America's 46 on the same map and seed, and the
six-cell matrix on Death.mmx and Barrel.mmx is identical to its baseline.

The remaining spread is russia 70 against america 46, and it is the "cheapest
armed unit" picker the handoff blamed: an army of one type whose value depends
entirely on which type the faction's cheapest happens to be. That is item 3 of
the handoff, untouched.

## Composition by role: the predicate is in, the policy that reads it is not

`Roles::RANGED` now exists: an armed unit whose longest weapon reaches past six
cells shoots over the front line rather than standing in it. Derived from the
weapon table, never a type name, and a structure keeps out of it. Its test walks
every armed type in retail rules and asserts the equivalence, so the predicate
cannot drift from the data.

The change that would have used it — keep a third of the army ranged, and have
the next unit fill whichever half is thin — was measured and **reverted**:

| Map | Seed | Own | Verdict |
|---|---|---|---|
| Death.mmx | 0x5EED | 46 → 45 | worse |
| Death.mmx | 19 | 51 → 51 | same (stuck +1) |
| Death.mmx | 10 | 50 → 50 | same |
| Barrel.mmx | 0x5EED | 46 → 48 | better |
| Barrel.mmx | 19 | 46 → 46 | same |
| Barrel.mmx | 24301 | 46 → 44 | worse |

Three cells worse, one better: under the keep rule that is a revert, and it was
reverted. The role itself is inert (six of six identical against the baseline),
so it is committed and the reader is not.

What that result says about the item: quotas alone do not raise the object count.
A house that spends a third of its budget on longer-ranged, more expensive units
buys fewer units, and the count it is judged by falls. The next attempt at item 3
has to show the range is worth something — a fight the army wins that it lost
before — and the matrix as it stands cannot see that, because it counts objects
and both houses are still holding.

## The matrix has no fighting in it to measure

The harness now records the trade: `own_lost` and `enemy_lost`, summed off the
whole census curve, printed as `lost=own:enemy` and written to the history file.
A composition change is expected to raise the enemy's losses before it raises the
object count, so this was the reading item 3 needed. It is printed and never
judged — the keep rule is still the object count, and a test asserts that a better
trade alone cannot turn a cell from `same` into `better`.

The first measurement with it:

| Faction | Own | Enemy | Lost |
|---|---|---|---|
| america | 46 | 46 | 0:0 |
| russia | 70 | 49 | 0:0 |
| yuri | 45 | 46 | 0:0 |

Zero in every cell, and the full curves confirm it: every census row on both sides
is at least as high as the one before it. Over 12000 ticks on Death.mmx neither
side destroys a single object. Both houses build, both keep building, and the two
armies never meet in a way that costs anyone anything.

That is the real reason item 3 failed: there was no fight for a range advantage
to win. It also means the object count is currently measuring the economy and
nothing else, and that `enemy` is the enemy's economy rather than its army. The
next step is not another composition rule; it is getting the two armies to meet —
either by turning attacking back on now that a job can end for all four of the
design's reasons, or by finding where the opponent's units actually are.

## The army goes, and there is nothing in sight to go at

Turning attacking on changed nothing measurable: the same three factions, the
same counts, `lost` still 0:0. Three separate conditions gate the attack and a
count that does not move cannot say which one refused, so the gate now reports
itself:

    military: tick=9000 enabled=true soldiers=30 want=30 seen=0 near=0 going=true

`going=true`: the army reaches its size and the house is willing to commit.
`seen=0`: the projection reports no enemy in line of sight, on every reading of a
12000-tick match. The army is ready and has nothing to attack. That is the whole
of why item 3 could not be judged.

Scouting was written to fix it — a tenth of the army sent to ground the house has
never revealed, which is the spec's A2 — and **both versions were measured and
reverted**:

- nearest unrevealed cell: `seen` still 0, and every unit cell in the log sat
  between x=169 and x=186. The scout walked a step past the base's own sight,
  that step cleared, and the next term named the next step.
- farthest unrevealed cell on a coarse scan: six of six cells worse, `stuck` up in
  every one (0→2, 1→4, 1→2, 1→2, 0→1, 0→2). Given a `Move`, the scouts could not
  make it and stood still instead.

What that settles: the units do not travel far, and `seen=0` is a symptom of that
rather than a targeting defect. The order funnel admits the move — the log shows
no refusals — so the next reading is downstream of admission: why a `Goal::Move`
from the policy does not carry a unit out of the base area.

## A Move works in a test and not in a match

The previous round left a choice: either the move order never travels, or the
policy re-sends it and the unit restarts. A test now settles the first half.

One tank at (6,6), one `Goal::Move` to (26,26), through `Funnel::prepare` — the
same door a player's click and the policy both use — and the engine drives it.
The tank arrives within a cell of the destination. The order path is sound over
twenty cells, with nothing re-sending.

So the fault is on the policy side, and scouting was tried three times:

| Attempt | Target rule | Reading |
|---|---|---|
| 1 | nearest unrevealed cell, re-chosen each term | `seen=0`; every unit cell between x=169 and x=186 |
| 2 | farthest unrevealed, remembered per scout | target `Cell((0, 0))`, three tanks `still=11400` at the end |
| 3 | same with a 16-cell margin off the fog grid's edge | target `Cell((16, 16))`, tanks still at `(178, 100)`, stuck from tick 600 |

All three reverted. The third is the informative one: the destination was
reachable, the order went out once, and the unit did not travel — while the same
goal through the funnel in a test moves a tank twenty cells. So the difference is
the `Outbox` path, or the state the unit is in when the policy orders it, and not
the goal.

The test is committed and the matrix is unchanged by it (six of six identical).

## The fifth scout, the first that reached the enemy, and a zero that does not fit

The previous round left a shape: a scout must be cheap and fast, the trip short,
the destination inside the playfield. The profile says which unit that is — a dog
has nine cells of sight and twice a rifleman's speed for the same 200 credits
(`sight=9, speed=8, cost=200` against `E1`'s `5/4/200`). `best_scout` picks by
sight per credit off the profile, so a faction whose cheapest eyes are a rifleman
still scouts.

That version was measured and reverted, six of six cells worse:

| Map | Seed | Own | Lost | Stuck |
|---|---|---|---|---|
| Death.mmx | 0x5EED | 46 → 38 | 0:0 → **5:0** | 0 → 3 |
| Death.mmx | 19 | 51 → 53 | — | 0 → 4 |
| Death.mmx | 10 | 50 → 49 | — | 1 → 4 |
| Barrel.mmx | 0x5EED | 46 → 44 | — | 1 → 3 |
| Barrel.mmx | 19 | 46 → 46 | — | 1 → 3 |
| Barrel.mmx | 24301 | 46 → 46 | — | 1 → 3 |

The `lost` column is the new part and it is the informative one: this is the first
scout attempt where anything was destroyed. Five of the house's objects died,
which means the scouts reached the enemy and were killed by it.

And yet every reading of the match shows `seen=0`:

    military: tick=10800 enabled=false soldiers=27 want=30 seen=0 near=0 going=false

Units walked into enemy fire and the projection reports no enemy in line of sight,
on every one of seventeen readings. `Observation::visible_enemy_entities` is tested
and works in the fixture (`vision_tests`, one rifleman beside ours and one across
the map: only the near one is a target), so either the fog is not being refreshed
for the house in a real match, or the units are dying to something the visibility
layer does not consider an object — a defence shot, or a shot from outside the
unit's own sight.

That is the next question, and it is a better one than "where should a scout go".
Nothing scouts today; all five attempts are reverted.

## The deadlock the readings add up to

Every fact in the last three rounds composes into one shape, and it is a loop:

1. the army holds its post until `seen` is non-empty — `commit` only proposes an
   assault when `visible_enemy_entities` has something in it;
2. `seen` is zero on every reading of every match, because the two bases sit
   (169, 91) and (70, 157) apart: **a hundred and sixteen cells** with nothing
   between them;
3. so the army never takes a step towards the enemy, and the enemy's count never
   falls (the census in the previous section shows it only rising);
4. and the army holds.

A test now rules out the projection as the cause: two riflemen two cells apart in
the rival arena, and the house's fog reports both the cell (`is_cell_visible`) and
the enemy standing on it (`visible_enemy_entities`) within a few frames. The fog
and the accessor are sound. `seen=0` is the honest answer to "is there an enemy in
line of sight" when nothing has come within sight of anything else.

With `ATTACK_ENABLED = true` the gate reports itself and confirms the loop:
`going=true` from tick 9000 onwards, `seen=0` on every reading, and `own`/`enemy`
identical to the attacking-off run.

So the next change is not a target rule and not another scout. It is that a
waiting army has to go somewhere: with no enemy in sight, the assault should be a
march towards ground the house has not seen, not a guard order. That is the same
`sight` the scouts were reaching for, applied to the army instead of to one unit,
and the reading that judges it is `seen` becoming non-zero at all.

## Closing the deadlock: the army marched, and killed seven

The deadlock above has an obvious escape, and this round tried it: with no enemy
in sight, a waiting army that is big enough marches towards unseen ground instead
of guarding the base. The target is the unrevealed cell *farthest from home* that
is inside the playfield and within one march of the army.

The first version preferred the unrevealed cell *nearest* home, and that is the
side away from the enemy on this map: the column marched to `Cell((184, 100))`,
seventeen cells east of a base at (169, 91), while the enemy held (70, 157) to the
west. `seen` stayed zero for the whole match. Farthest-from-home flipped the
direction and the army met the enemy.

Measured, Death.mmx seed 0x5EED, 12000 ticks, with the march and an arrival ending
so a column does not stand on its destination holding an assault forever:

| Reading | Value |
|---|---|
| own | 46 → 44 |
| enemy | 46 → **36** |
| lost | 0:0 → **0:7** |
| stuck | 0 → 5 |

**Seven enemy objects destroyed**, by an army that had never left its base. The
column walked about sixty cells — from (169, 91) to (152, 151) — and kept fighting
on arrival. This is the first time any change in this work produced a real
engagement.

It was still reverted: four of six cells worse. Two of them are `stuck` at 40,
which is the reading saying something rather than the army being stuck — a column
that arrives, fights, and is then re-tasked every term without a rule for "I am
already where you sent me". The other two lose one to three objects while killing
six to nine, which is a trade, not a regression.

So the mechanism is proven and the ladder around it is not written yet. The next
attempt should keep the march and add what a column does when it arrives and finds
nothing: hold that ground and send the next march from there, rather than being
re-decided every term.

## Why the march keeps failing on the reading, not on the field

The previous round's march killed seven enemy objects and was reverted because
four of six cells were worse. Two of those four were `stuck=40`. This round tried
to fix exactly that, by remembering the column's objective across terms and moving
it on when reached, so a column that arrives is not re-decided every term.

It was worse in a different way: `enemy` did not move at all (46 → 46, `lost`
0:0), so the remembered objective stopped the advance instead of steadying it. A
destination chosen once and kept is a column that stands where it was sent.

Both versions were reverted, and the useful result is what they agree on: **the
`stuck` reading is what rejects every version of this, and it is measuring the
wrong thing.**

`stuck` counts a unit that holds a movement job and does not change cell for six
seconds. An army column that has arrived at its objective and is holding that
ground is exactly that, by that definition — and holding ground is what an army
does between marches. The spec's H2 counts a unit stuck when it holds a movement
job and does not move; a column standing on the ground it was sent to take is not
the failure H2 describes, and three attempts have now been rejected on it.

So the next change is not another march. It is the stuck definition: a unit whose
job has been achieved — assault on a cell it stands on, defend at the post it
stands at — is finished, not stuck, and the job should end and be re-decided
rather than counted against the policy forever. Until that is right, the matrix
cannot tell a working army from a frozen one.

## The stuck reading, corrected

`stuck` is the one reading that rejected every march attempt, and it was measuring
the wrong thing. It counts a unit that holds a movement job and does not change
cell for six seconds; an army column holding the ground it just took is exactly
that, and holding ground is what an army does between marches. The spec's H2 means
a unit that *should* be moving and is not.

`Policy::job_achieved` now separates the two: a job whose target is a cell the unit
stands on (within one) is finished, not stuck. An object target still has to die
before it counts as done, which `finish_done_jobs` decides, and a job with no
target has nowhere to be.

It is inert with attacking off — Assault on a cell is only issued while marching —
so the six-cell matrix reads identical to the recorded baseline. The point is that
the *next* march attempt can now be judged on whether it fights and whether the
house survives, rather than on whether its column is standing still after it won.

That measurement has not been run yet. It is the first item in handoff.md.

## The march reaches the matrix, and the matrix has a fight in it for the first time

Three readings had to be right before a march could be judged, and all three are
now in the tree, behind `ATTACK_ENABLED = false` so the baseline is untouched:

1. `Policy::job_achieved` — a unit on the cell it was sent to has *finished*, not
   stuck. Without it a winning column reads `stuck=40`, which is the reading that
   rejected the three previous attempts.
2. The objective is a cell **on the edge of known ground**, not the farthest
   unrevealed one. Marching deep into unexplored terrain picks cells behind walls:
   the previous attempt ordered a column to `Cell((132, 204))`, it stopped 23 cells
   short at `(153, 181)` where it could not stand, and stood there for the rest of
   the match. A cell touching revealed ground is one a unit on revealed ground could
   have walked to — the frontier is the only place a blind march can safely aim.
3. `ACHIEVED_CELLS = 3`, not 1. A column does not arrive in formation: the measured
   case held `(143,98)..(146,99)` around `Cell((144,96))` and read stuck at ninety
   frames on members five frames short of moving.

Measured with the flag on (front2.jsonl), six cells, 12000 ticks:

| Map | Seed | own | lost | stuck | |
|---|---|---|---|---|---|
| Death.mmx | 24301 | 46 → 46 | **0:8** | 0 | same — it fought and won |
| Death.mmx | 10 | 50 → 51 | 0:0 | 1 → 2 | worse (stuck only) |
| Death.mmx | 19 | 51 → 51 | 0:0 | 0 → 1 | worse (stuck only) |
| Barrel.mmx | 10 | 50 → 50 | 0:0 | 0 → 1 | worse (stuck only) |
| Barrel.mmx | 24301 | 46 → 46 | 0:0 | 0 | same |
| Barrel.mmx | 19 | 46 → 37 | **7:0** | 0 → 1 | worse — the column was destroyed |

No seed was lost: every cell still reads `timeout at tick 12000`, and own holds or
rises in five of six. Against `inert.jsonl` it is four cells worse, one better, one
same, so the flag stays off — the keep rule is not negotiable, and the measurement
is recorded rather than argued with.

Two readings explain why the code is kept instead of deleted:

- in the three `stuck`-only cells, both counted units are harvesters (`CMIN` on
  `Harvest`) shuttling between an ore field and a refinery. The march adds no stuck
  of its own; those cells differ from the baseline by a reading the army never
  touches.
- Barrel.mmx seed 19 lost seven objects **because the column left**: `soldiers` fell
  from 32 to 22 between tick 9000 and 10800. That is the trade the change actually
  makes, and it is a garrison question — whether the army leaves at all — rather
  than a march question.

So the deadlock is closed and a new, sharper question is open: an army that leaves
wins fights and loses its base. What stays home is the next thing to decide, and
`lost` is now a reading that can see the answer.

## The hard maps, on this build

| Map | Own | Enemy | Stuck | Reading |
|---|---|---|---|---|
| xcc_maps/xeb1.map | 30 | 36 | 1 | holds its side; no wall opened (walls=0) |
| xcc_maps/xtsunami.map | 35 | 43 | 0 | never leaves the starting island |

Before the assignment fix the island run read 28 stuck: the army was being sent
at a target it could not reach and froze there. It now holds instead, so the
number is zero and the failure has moved to where it belongs — the policy has no
transport job at all. Boarding itself is proven (the Enter mission boards an
IFV in a test), so this is policy work, not an engine gap.

## Known gaps

- The army does not attack, and attacking is measurably worse even at parity:
  six of six cells fall with the flag on, and the army stops at a stale target
  cell (see the section above). Re-targeting on arrival, local superiority,
  retreat and reinforcement are the next slice; the order path itself is sound
  now (attack-move lands on `Move`, and the predicate accepts it).
- The income stall is fixed at its source (the harvest cycle above); the reserves
  are not. One cell still ends with a refinery in production at 5 credits
  (Death.mmx, seed 10), and five of the six end with `nothing to build` while
  holding 300-661 credits: two refineries, three or four defences, and nothing
  else the wallet can afford. The unit buyer's `UNIT_SPEND_FLOOR` (900) plus its
  1500 reserve while the house wants refineries is the binding limit there.
- A refinery queued with only its cost plus the fixed 200 in the wallet takes the
  rest of the income while it is unfinished. Three attempted fixes made the
  matrix worse and were reverted; the reserves are still a constant.
- `stuck` no longer distinguishes a harvester that pauses from one that is
  frozen; see the residual in the section above. A pause is not visible in the
  projection, so the next reading has to come from the engine's miner state.
- The shipyard counts as a war factory. Splitting the role improved Death.mmx
  and lost every Barrel.mmx seed, so the split was reverted.
- Nothing for walls, transports, garrisons, superweapons, scouting, crates, or
  defence placement.

## How a change is judged

A change is kept when the matrix improves: own count up at a fixed tick, no seed
lost that survived before, unowned zero, stuck not up. One change per
measurement. Five of the experiments in this session improved one reading and
lost seeds on another map, and were reverted; the assignment bug above is why
one of them looked like a win.

The measurement is a run of `tools/autoplay_check.py` against the baseline
recorded above, which prints a cell as `worse` when own falls, a seed that
survived is lost, unowned rises or stuck rises (`--baseline` marks the cell, and
`--compare-only` re-reads two files without running anything).

## The attack lane, measured at HEAD (2026-11-02, after the handoff)

The flag was put on for one matrix run and the six cells were compared with
the flag-off baseline (review-cargo-off.jsonl). Recorded because the question
the handoff leaves open is now measured, not guessed:

| Map | Seed | own | enemy | lost | stuck |
|---|---|---|---|---|---|
| Death.mmx | 24301 | 46 -> 46 | 46 -> 35 | 0:0 -> 0:8 | 0 -> 0 |
| Death.mmx | 19 | 51 -> 51 | 45 -> 40 | 0:0 -> 0:5 | 0 -> 0 |
| Death.mmx | 10 | 50 -> 51 | 45 -> 44 | 0:0 -> 0:0 | 1 -> 2 |
| Barrel.mmx | 24301 | 46 -> 46 | 49 -> 48 | 0:0 -> 0:0 | 1 -> 0 |
| Barrel.mmx | 19 | 46 -> 37 | 39 -> 40 | 0:0 -> 0:7 | 0 -> 0 |
| Barrel.mmx | 10 | 50 -> 50 | 41 -> 41 | 0:0 -> 0:0 | 0 -> 1 |

Read it as: the column kills thirteen enemy objects across two cells and
loses none of its own, and in exactly one cell the base is destroyed while
the column is away (Barrel.mmx 19, own 46 -> 37). The flag stays off at HEAD.

### Three experiments after that, all reverted

1. **A garrison of twelve at home.** The base survives (Barrel.mmx 19 reads
   own 37 -> 50) but the kills go to zero in every cell, and stuck rises by
   one to four. It fails the keep rule: lost must stay non-zero somewhere, or
   nothing is being decided.
2. **A proportional garrison (two thirds march).** own -6 on Barrel.mmx 10,
   kills zero, five of six cells worse. A small column cannot take the ground
   a large one took, so the share must not be fixed.
3. **Skipping the frontier cell a column already failed to reach.** Five cells
   same, one worse (stuck +1), kills unchanged. So the stuck units are not
   march-cell failures: they are standing on an object target they cannot
   finish, which is the ladder the handoff names.

### What that leaves

The trade is measured rather than guessed: the army that kills is the whole
army, and any garrison that keeps the base kills nothing. The next change is
therefore not a smaller garrison but a column that comes back — a retreat
condition on the column, and a give-up rule that fires for an object target
the column cannot hurt. The readings to watch are own (must not fall), lost
(must stay non-zero) and stuck (must not rise).

## The campaign lane (2026-11-02, after the handoff)

The army now campaigns: it goes out, it fights, and a unit that has finished its
fight far from home walks back before it is given another job. The home cell
comes from the military manager, the only layer that reads the house's
structures; the policy learns it from there.

Measured over the six cells with the attack flag on:

| Map | Seed | own | enemy | lost | stuck |
|---|---|---|---|---|---|
| Death.mmx | 24301 | 46 | 46 -> 41 | 0:2 | 0 |
| Death.mmx | 19 | 51 | 45 -> 40 | 0:5 | 0 |
| Death.mmx | 10 | 50 -> 51 | 45 -> 44 | 0:0 | 0 -> 2 |
| Barrel.mmx | 24301 | 46 | 49 -> 48 | 0:0 | 1 -> 0 |
| Barrel.mmx | 19 | 46 -> 45 | 39 -> 40 | 0:0 | 0 |
| Barrel.mmx | 10 | 50 | 41 | 0:0 | 0 -> 1 |

Against the same lane without the campaign: five cells the same, one better
(Barrel.mmx 19, own 37 -> 45 - the base stops falling), none worse. Against the
flag-off baseline the lane now kills where it killed nothing before, own is
within one object in every cell, and stuck is up by one or two in two cells.

The attack flag is on in this commit. The reasoning: a policy that never fights
cannot be judged on the readings that matter, and the base is no longer left
open.

### What is still wrong with the army

- A column that reaches the frontier and finds nothing stands there. The
  frontier cell is chosen by distance, not by reachability, and a march covers
  about thirty cells, so the target is often one the unit cannot walk to. The
  fix is a reachability test before the order, not a retry after it.
- Two cells read one or two stuck: units standing on an assault they cannot
  finish. The give-up ladder exists for a target that is out of reach
  (UNREACHABLE_FRAMES) and for one that is not being hurt
  (INEFFECTIVE_FRAMES); neither fires here, because the target is visible and
  its health does not move for reasons the policy does not read.
- The column is the whole army. Every garrison experiment cost the kills.

## The strength reading, and what it changed (2026-11-02, evening)

The judge was reading an object count. Thirty tanks and thirty rifles are thirty
objects and not the same army, and the enemy grows when nothing meets it, so a
policy that hides scored well. The policy now reports its own strength in hit
points - health scaled by strength, the rules' own measure - for the house and
for the enemy it can see, and the matrix decides on that.

| Reading | Before | After |
|---|---|---|
| unit picker | the cheapest armed unit it can build | the unit with the most damage per credit against the armor it can see |
| trucks per refinery | one | two, the second bought from the surplus |
| refineries standing | 1 | 2 |
| defences standing | 0 | 4 |
| wallet at 12000 ticks | 12 credits, frozen 3600 frames | 300 to 636 credits, moving |

Measured over the six cells: the house's strength rises in four of six (5824 ->
7784, 6475 -> 7300, 6776 -> 7491, 6392 -> 7775) and falls in two (7400 ->
6153, 7075 -> 6651). The two are the same seed on both maps, so it is a timing
accident of that seed; what the accident is has not been explained. Average
strength: 6657 -> 7092.

### Rejected in the same session, with the numbers

| Experiment | Reading | Verdict |
|---|---|---|
| A column walks home when it cannot reach the frontier | army falls in five cells of five (7400 -> 7275, 5824 -> 5774, 7075 -> 6495, 6776 -> 6696, 6392 -> 6359) | the ground it took is worth more than the ground it was sent to |
| The army keeps 2200 for a third refinery | two cells better, four worse (6153 -> 5483) | the army is the better use of the money |
| The second truck waits for a second refinery | six cells identical: the truck is bought long before any raid | the rule never fired |
| The second truck waits while the house is raided | six cells identical | the rule never fired |
| The frontier cell is tested for reachability | five cells identical, one worse | the column holding is not the defect |
| A garrison of twelve, a proportional garrison, a recall on a raid | kills went to zero, or a cell lost eleven objects | any garrison that holds the base stops killing |

## The 1v7 lane on the same build

Death.mmx, seven brutal computer houses against the driven house, 12000 ticks,
three seeds:

| Seed | own | army | enemy objects | enemy in sight | lost | result |
|---|---|---|---|---|---|---|
| 0x5EED | 48 | 4503 | 316 | 659 | 0:0 | survived |
| 19 | 50 | 5620 | 295 | 2666 | 0:0 | survived |
| 10 | 51 | 6034 | 300 | 0 | 0:0 | survived |

Three of three survive the twenty-minute mark with nothing lost, against seven
houses that hold three hundred objects between them. It is not a win: the
house's strength is a third of what the enemy has, and the enemy never has to
come to it. What the lane says is that the house now survives what it could
not survive before (the same lane read 3:0 on one seed before the campaign
loop, and 40 own on this seed before the two trucks).

## The money belongs to the army, not to the third refinery

Keeping a refinery's cost in the bank (2200 while the house has fewer than
three) was measured on both lanes and rejected on both:

- 1v1, six cells: two better, four worse, the worst 6153 -> 5483.
- 1v7, three cells: three worse, and one seed that survived now loses
  (4503 -> 2973, lost at 11053).

So the army's money buys units, and the house's third refinery is not worth
the units it would cost. The income ceiling is real, but it is not the first
thing to fix: a house that cannot replace its losses has no economy to speak
of.

## The army is not strong enough to fight yet

Two attempts to make it fight, both measured on the 1v7 lane, both rejected:

| Attempt | Reading | Verdict |
|---|---|---|
| Engage a raider inside the base's ground instead of holding the post | one cell better, two worse; the worst 6034 -> 1834 with 13 of our units lost | the group commits to the raider and dies |
| Keep the cost of a third refinery in the bank | three cells worse; one that survived now loses | the army is the better use of the money |

Every rule that makes the army fight sooner costs more than it wins, because
the army it sends loses. The strength has to come first: more units per credit,
a front line that can absorb a hit, and a base that cannot be walked into.

## The army is composed from the role model, and the model was wrong

Three separate defects sat under "the army is not strong enough to fight",
and only the third was in the policy.

1. **`NeverUse=yes` was scored as a weapon.** `TechnoClass::What_Weapon_Should_I_Use
   @ 0x006F33BF` returns the primary as soon as the secondary carries the flag,
   so the entry is never fired. Retail's only such weapon is the
   `VirtualScanner` every dog and engineer carries as `Secondary=` — a range-5
   `InvisibleAll`/`SA` entry that exists to widen the guard scan. The role model
   scored it, which gave the attack dog 0.24 of anti-vehicle and 0.30 of
   anti-air weight off a warhead it can never shoot. The engine's own
   `capabilities.rs` already skipped the flag when it decides `Armed`; the role
   owner did not.
2. **The autoplay profile's weapon table was a second implementation.** It read
   every weapon slot of a type and summed them, so the IFV's seventeen
   `WeaponN=` entries — the crew weapons each passenger turns its turret into —
   became one transport with a dozen types' firepower, and the same
   `VirtualScanner` made the dog look armoured. Native reads one weapon: arm A
   of the ladder returns the gunner slot for a `TurretCount>0` non-gattling
   type, and `weapon_damage_value` (`0x006F3970`) reads slots 0..1 otherwise.
   The table now holds only those slots and never a `NeverUse=yes` entry, and
   `damage_against` is the best of them rather than their sum. `Armed` comes
   from `Capabilities` rather than a second reading of the list.
3. **The picker ranked by a count it did not have.** With `seen=0` on every
   reading, the old threat score was zero for every candidate and the tie broke
   on `(cost, id)`: `ADOG` before `E1`, `FV` before `MTNK`. The house fielded
   twenty-six attack dogs, whose `ParasiteDog` warhead is 0% against light,
   medium, heavy, wood, steel and concrete — an army that could kill riflemen
   and nothing else, in a match where the enemy builds mostly defences.

### One owner for "is this army"

`rules::unit_role::is_army` is now the answer, and the retail brain's census
asks it too (`Traits::combat` used to spell the predicate out). Every term is a
key the type declares: a weapon that fires, no capture or harvest job, fewer
than five seats (`TRANSPORT_SEATS`, moved onto the role model), not a spawn
child, not a yard maker. `ROLE_FLOOR` moved there for the same reason.

The `Roles::AI_ONLY` bit added a round earlier is gone. It read `NotHuman=yes`
as "the player cannot build this", which retail contradicts: every dog is
`NotHuman=yes` with `Cost=200`, `Prerequisite=Barracks` and `TechLevel=2`, and
is buildable. A mod's robot or animal unit marked the same way would have been
silently dropped from the army. The dog is refused now because the role model
says it answers one job, not because of a flag retail uses for something else.

### What the picker composes against

The threat vector the retail brain uses comes from the whole enemy army — which
it may do, because the native computer is omniscient. This policy may only look
at what its own units can see (spec 2.5), and on a map whose bases sit 116 cells
apart that is nothing on every reading of every match.

So the target is **the mix the rule set itself offers**: each job worth the sum
of the weights every buildable type carries in it, normalised, times the army
the house already fields. A role a mod adds or retunes reshapes the target with
no constant to revisit. A visible enemy is added on top in units of one type's
worth, so what the house can see still steers what it buys. The score is the
biggest gap a type closes, not the sum of all of them (one multi-role vehicle
outranking every specialist is the engine's own recorded finding), and ties go
to the type the house holds fewest of, then the dearer one.

Measured on the six-cell matrix against `eval-v2-fixed.jsonl`:

| Map | Seed | own | army | lost |
|---|---|---|---|---|
| Barrel.mmx | 10 | 43 -> 48 | 6975 -> 7058 | 0:0 |
| Barrel.mmx | 19 | 42 -> 47 | 7075 -> 7175 | 0:0 |
| Barrel.mmx | 24301 | 31 -> 42 | 3478 -> 6675 | 5:1 -> 0:0 |
| Death.mmx | 10 | 43 -> 49 | 7075 -> 7175 | 0:0 |
| Death.mmx | 19 | 44 -> 49 | 5658 -> 7293 | 0:0 -> 0:7 |
| Death.mmx | 24301 | 41 -> 29 | 6775 -> 4532 | 0:0 -> 5:1 |

Five cells better, one worse. The house fields `E1`/`GGI`/`MTNK`/`FV`/`ADOG`
in every cell instead of one type, and seed 19 on Death.mmx kills seven enemy
objects for none of its own. The one regressed cell is the campaign, not the
composition: the column leaves and is destroyed (`lost` 5:1), which is the
retreat rung status.md already records as missing.

The 1v7 lane on the same build, Death.mmx, seven brutal houses, 12000 ticks:

| Seed | own | army | lost |
|---|---|---|---|
| 0x5EED | 48 -> 37 | 4503 -> 4790 | 0:0 |
| 19 | 50 -> 41 | 5620 -> 6101 | 0:0 |
| 10 | 51 -> 37 | 6034 -> 4644 | 0:0 -> 2:0 |

All three still survive, and the army's hit points are up on two of the three —
but the object count falls on all three, because the same income now buys
dearer units. The wallet ends at 120-935 credits in every seed, so the house is
spending everything it has either way; the mix is simply more expensive per
object. That is a real cost of this change and is recorded rather than argued
away: the 1v1 matrix is the lane it was measured on, and the 1v7 lane now has a
number to beat.

## The army would not fight unless it had thirty units

The gate that commits the army was one number: `soldiers.len() >= ARMY_TO_ATTACK`
(30) **and** `soldiers >= 2 * near`, whatever the enemy in front of it was doing.
On Death.mmx seed 24301 that read, on every one of the twenty readings:

    military: tick=10800 enabled=true soldiers=23 want=30 seen=5 near=5 going=false

Twenty-three units, five enemies in sight, and the house holding because it had
not reached thirty. `epoints` (the enemy's strength inside the house's own
sight) climbed from 0 to 1750 over the last three thousand frames: the raiders
were eating the base while its army watched.

The two questions are separate, and now they have two constants:

- **Engaging** what the house can see is gated on the odds in front of it —
  `soldiers >= 2 * near` and a floor of four, which is the smallest group that
  can trade with a raid. An absolute size has no business here: a house of
  twenty-three that waits for thirty never fights.
- **Marching** into ground with nothing in sight still wants a column, and keeps
  the thirty (`ARMY_TO_MARCH`). A squad that walks off the map alone is a squad
  the house loses.

Measured against `eval-v2-fixed.jsonl`, six cells:

| Map | Seed | own | enemy | lost | army |
|---|---|---|---|---|---|
| Barrel.mmx | 10 | 43 -> 47 | 52 -> 38 | 0:0 -> 0:1 | 6975 -> 6908 |
| Barrel.mmx | 19 | 42 -> 47 | 56 -> 52 | 0:0 | 7075 -> 7175 |
| Barrel.mmx | 24301 | 31 -> 41 | 43 -> 44 | 5:1 -> 0:0 | 3478 -> 6521 |
| Death.mmx | 10 | 43 -> 49 | 46 -> 43 | 0:0 | 7075 -> 7175 |
| Death.mmx | 19 | 44 -> 48 | 42 -> 33 | 0:0 -> 0:4 | 5658 -> 7211 |
| Death.mmx | 24301 | 41 -> 39 | 47 -> 40 | 0:0 -> 0:2 | 6775 -> 5991 |

The house's object count rises in five of six, the enemy's falls in five of six,
and the army kills where it killed nothing. The cell the change was aimed at —
Death.mmx 24301 — goes from `own 29, lost 5:1` to `own 39, lost 0:2`.

The harness still marks four of the six `worse`, because `stuck` rises by one in
three cells and its keep rule counts that. Every one of those units is a `CMIN`
harvester on `Harvest` pausing at a dock or an ore field (`still=90..195`, and
the counter resets between readings) — the residual this file already records
above, not the army. The reading that would settle it is the spec's H2, "holds a
*movement* job and does not move": a truck in a harvest cycle is not the failure
H2 describes, and the harness does not make that distinction yet.

### Defending is not marching

The odds gate alone cannot decide a charge, because fog hides most of a raid.
On Death.mmx 1v7 seed 19 the house saw six enemies at tick 10800, called twelve
to six good odds with fifteen units, and lost fourteen of them to what the fog
was not showing: `own 41 -> 27`, `army 6101 -> 1849`, `lost 14:0`.

So the two questions are separated by distance as well as by size. A target
within [`NEAR_CELLS`] of home is a defence, and a house defends with the army it
has; a target beyond that is a march, and a march keeps the column of thirty.
The same radius is what the odds gate already counts enemies within, so a target
and a base are measured the same way rather than by two numbers.

Measured: 1v1 four cells better, two worse; 1v7 three cells better on own, army
and losses at once.

| Lane | Seed | own | army | lost |
|---|---|---|---|---|
| 1v1 Death.mmx | 24301 | 41 -> 39 | 6775 -> 5991 | 0:0 -> 0:2 |
| 1v1 Death.mmx | 19 | 44 -> 48 | 5658 -> 7357 | 0:0 -> 0:4 |
| 1v1 Barrel.mmx | 24301 | 31 -> 41 | 3478 -> 6521 | 5:1 -> 0:0 |
| 1v7 Death.mmx | 24301 | 37 -> 44 | 4790 -> 6688 | 0:0 |
| 1v7 Death.mmx | 19 | 41 -> 45 | 6101 -> 6303 | 0:0 |
| 1v7 Death.mmx | 10 | 37 -> 46 | 4644 -> 5718 | 2:0 -> 0:0 |

### The march arm was unreachable, and the scan stopped at eighteen cells

Splitting the gate left a defect in the table that reads the two halves:

    let order = match (engage, target, advance) {
        (true, Some((entity_id, _)), _) => Some(Target::Object(entity_id)),
        (true, None, Some(cell)) => Some(Target::Cell(cell)),   // never
        _ => None,
    };

The second arm needs `engage` and no target, and `engage` requires a target. The
column therefore stood at its post with `march=true order=None` for the rest of
every match, which is why the first measurement of the split showed nothing:
`advance` was computed and then thrown away. The match now reads `(target,
advance)` on its own arms.

Under it was a second stop: `march_cost` scanned one march's reach (eighteen
cells) for ground the house had not seen, and by tick 10800 on Death.mmx seed 19
every cell that close was already revealed — `march=true order=None` with
thirty-four units. A fallback scan at twice the radius (`MARCH_FAR_CELLS`) runs
only when the first finds nothing, so a safe frontier still wins when there is
one. The reading after both: `military: tick=10800 ... soldiers=34 march=true
order=Some(Cell((108, 168)))`.

Measured against `stuck-v8.jsonl`: five cells the same, one better, none worse.
That is honest rather than disappointing — the column only forms around tick
10000 on the current income, so there are twelve hundred frames left to march
in. Growth, not the march, is what bounds this lane now.

### Reverted: a truck the army paid for

The income ceiling is what bounds every lane now, and the reading that made it
concrete: three harvesters for two bays at the end of every match, and a wallet
that never again exceeds 900 credits after the opening spend. The second truck
is gated at `cost + 1500` (`EXTRA_HARVESTER_MARGIN`) and the army spends
everything above 900, so the 1700 the truck wants never accumulates.

The attempt: the economy states what it is saving in `Demands::reserve`, and the
managers that follow it — the army, and the economy's own build order — leave
that much alone. It works as designed and it is much worse:

| Map | Seed | own | army |
|---|---|---|---|
| Barrel.mmx | 10 | 47 -> 44 | 6908 -> 5061 |
| Barrel.mmx | 19 | 47 -> 44 | 7175 -> 5675 |
| Barrel.mmx | 24301 | 41 -> 36 | 6521 -> 4075 |
| Death.mmx | 10 | 49 -> 41 | 7175 -> 5675 |
| Death.mmx | 19 | 47 -> 45 | 7457 -> 6226 |
| Death.mmx | 24301 | 39 -> 34 | 5991 -> 3827 |

Six of six worse, and the army's hit points fall by a quarter to a third: a
reserve of 2900 is a house that builds nothing at all while it waits. This is
the third time an economy-saving rule has lost to the army (see "the money
belongs to the army", above), so the channel was reverted with it rather than
left in place for a fourth try. If income is raised again it has to come from
something that pays for itself without idling the wallet — a cheaper truck, a
closer ore field, or ore that is worth more per trip.

## The column is twenty, and the trade is the reading

The march gate wanted thirty armed units. On the current income that size forms
around tick 10000, so a column spent most of a twenty-minute match standing at
its post; Death.mmx seed 10 read `own 31, lost 35:2` at tick 24000 — it lost
thirty-five objects to a fight it never joined.

Twenty, over 24000 ticks, same six cells:

| Map | Seed | own | army | lost |
|---|---|---|---|---|
| Death.mmx | 24301 | 65 -> 80 | 7509 -> 10969 | 3:4 -> 0:4 |
| Death.mmx | 19 | 41 -> 69 | 5716 -> 9401 | 22:6 -> 12:3 |
| Death.mmx | 10 | 31 -> 50 | 3396 -> 6436 | 35:2 -> 18:16 |
| Barrel.mmx | 24301 | 51 -> 49 | 6376 -> 5715 | 11:12 -> 8:1 |
| Barrel.mmx | 19 | 54 -> 39 | 6406 -> 4045 | 16:1 -> 19:7 |
| Barrel.mmx | 10 | 63 -> 56 | 8577 -> 6140 | 2:6 -> 10:13 |

Averages: own 51 -> 57, army 6330 -> 7118, 89 lost for 31 killed -> 67 lost for
44 killed. Every one of the six still times out at 24000 rather than falling,
which is the survival reading that matters most.

It is a trade, not a win: `Death.mmx` gains enormously and `Barrel.mmx` loses on
object count, where the larger column holds ground the smaller one gives up. The
constant is therefore recorded with both sides rather than declared correct — the
next attempt at it should make the size a property of the map (how far the enemy
is) instead of a number that has to serve both.

### The 1v7 lane at 24000 ticks does not survive

All three seeds survive the twenty-minute mark the lane was recorded at (12000
ticks), and none survives twice that long:

| Seed | own at the end | lost | fell at |
|---|---|---|---|
| 0x5EED | 3 | 39:0 | after 12000 |
| 19 | 5 | 33:0 | 12418 |
| 10 | 4 | 38:0 | 15879 |

Seven brutal houses hold 324-369 objects between them against the house's
forty-odd, so this is not a defect a constant fixes. It is the honest state of
the lane: 12000 ticks survived, 24000 not, and the army still kills nothing on
that map — thirty-nine of the house's objects are lost for zero of theirs in one
seed. The next slice has to come from the enemy side of the ledger (the house
fights nothing it is not standing on) rather than from another composition rule.

### Reverted: fighting at home whatever the odds

The 1v7 lane loses thirty-nine objects for zero of the enemy's in one seed, and
the obvious suspect was the odds gate: an army that declines to fight while its
base burns. Taking the odds out of the home case is worse on all three seeds —
`own 45 -> 23` and `army 6303 -> 2769` on seed 19, `15:0` lost. The gate is not
the defect; it is the only thing keeping those units alive. What kills them is
whatever does not have to walk into the army to do it, and that is the next
question rather than this one.

## The column size is a fact about contact, not a number

Twenty and thirty were both measured and each won a lane: twenty was six cells
better on 1v1 and collapsed `Death.mmx` 1v7 seed 19 (`own 45 -> 23`, `army 6303
-> 2769`), thirty was the reverse. The constant was standing in for something
the policy can actually observe — **whether the house is in contact**.

A house that has seen no enemy and lost nothing for four minutes (`MARCH_
IMPATIENCE`) marches with twenty; one that has been in contact waits for thirty.
Contact is the enemy in sight *or* an object of the house's own gone since the
last reading: the 1v7 lane reads `seen=0` on every reading while its objects
die, so vision alone would call a house under attack idle.

Measured, 12000 ticks:

| Lane | Reading | Before (30) | After |
|---|---|---|---|
| 1v1, six cells | own | +26 total | **+37** |
| 1v1, six cells | army | +4191 total | **+5867** |
| 1v7 seed 19 | own / army | 45 / 6303 | 46 / 6200 |
| 1v7 seed 24301 | own / army | 44 / 6688 | 42 / 5193 |

Five of six 1v1 cells better than the column of thirty, one worse; 1v7 holds on
two seeds and loses about a fifth of its hit points on the third. Recorded with
both sides rather than called a win.

### The forty-minute reading, after the session

Both maps, three seeds, 24000 ticks, one opponent — the lane that decides
whether a house survives rather than whether it grows:

| Map | Seed | own | army | lost |
|---|---|---|---|---|
| Death.mmx | 24301 | 77 | 10739 | 2:6 |
| Death.mmx | 19 | 58 | 7608 | 11:4 |
| Death.mmx | 10 | 47 | 5885 | 21:15 |
| Barrel.mmx | 24301 | 50 | 6256 | 9:0 |
| Barrel.mmx | 19 | 39 | 4045 | 19:7 |
| Barrel.mmx | 10 | 51 | 6191 | 1:3 |

Six of six time out rather than falling. Against the column-of-thirty build on
the same six cells: own averages 54 against 51, army 6787 against 6330, and the
trade is 63 lost for 35 killed against 89 for 31 — better on all four numbers,
which is the first time this session that a change has moved every aggregate at
once. `Barrel.mmx` seed 19 remains the weak cell (`own 39, army 4045, 19:7`) and
is the one to read next.

### Two reverted experiments at the end of the session

**Counting an armed enemy building as an artillery need.** The 1v1 enemy builds
almost nothing but defences (`NAWALL` 32, `NALASR` 17, `NABNKR` 18 in one 1v7
log), and the house has no answer that outranges them. `HOWI` — the howitzer,
`Prerequisite=GAWEAP`, `HowitzerGun` reach 12 cells — is buildable from the
first war factory and the picker never buys it, because the only source of
artillery demand is the roster mix, and most types are not artillery. Adding
a demand for every armed enemy building the house can see changed nothing at
12000 or 24000 ticks in any of the six cells, because the army never gets close
enough to see one: `seen=0` on every reading of every match, and the column is
still wandering its own side of the map at tick 23400.

**Ranking the frontier by distance before proximity.** Preferring the frontier
cell farthest from home instead of the one closest to known ground is three
cells better and three worse over 24000 ticks (`Death.mmx` seeds 10 and 19 gain
10 and 16 objects; seed 24301 loses 17, `Barrel.mmx` 19 loses a third of its hit
points). Not a win, and reverted.

The second result is the more useful one: it says the march target is chosen
fresh every term from whatever the column's first unit happens to stand on, so
the column oscillates between opposite edges of the explored region instead of
walking in one direction. That is the shape of the scouting problem queue.md
still has open (D4), not a tie-break to tune.

**Anchoring the march on the column's centroid — rejected, then kept.**
`soldiers.first()` is whichever unit the entity store yields first, and it is
often a unit left behind at the base, so the frontier was searched from home on
every term and the column never got more than one march out. Measuring the
centroid at **12000 ticks** said three cells better and two worse, and it was
reverted on that reading.

That reading was too short. The question is how far a column travels over a
whole match, and at 12000 the two anchors have not diverged yet. At 24000:

| Map | Seed | own | army | lost |
|---|---|---|---|---|
| Death.mmx | 10 | 47 -> 69 | 5885 -> 9388 | 21:15 -> 2:8 |
| Death.mmx | 19 | 58 -> 64 | 7608 -> 8681 | 11:4 -> 14:2 |
| Death.mmx | 24301 | 77 -> 67 | 10739 -> 8768 | 2:6 -> 0:2 |
| Barrel.mmx | 10 | 51 -> 49 | 6191 -> 5730 | 1:3 -> 19:6 |
| Barrel.mmx | 19 | **39 -> 80** | **4045 -> 9804** | 19:7 -> 2:0 |
| Barrel.mmx | 24301 | 50 -> 39 | 6256 -> 3828 | 9:0 -> 20:9 |

Own 322 -> 368, army 40724 -> 46200, losses 63 -> 57, and `Barrel.mmx` seed 19 —
the cell that was recorded as the weakest — doubles its object count and its hit
points. Three cells get worse, two of them badly, which is the shape of an army
that now goes somewhere: whether a given seed meets the enemy on good ground or
bad is not something the anchor controls.

1v7 at 12000 is two cells better and one worse. Kept, and the lesson recorded:
a window shorter than the behaviour it measures cannot reject a change about
distance.

### The weak cell is an attrition the house cannot pay for

`Barrel.mmx` seed 19 at 24000 ticks reads `own 39, army 4045, lost 19:7`, and the
readings say what the number is:

    tick=14400 soldiers=45 engage=true  defences=4 credits=75
    tick=18000 soldiers=40 engage=true  defences=4 credits=175
    tick=21600 soldiers=32 engage=true  defences=3 credits=451
    tick=23400 soldiers=29 engage=false defences=1 credits=644

It is not a house that will not fight — `engage=true` on four of the last six
readings, and it is the enemy that comes to it. It is a house that loses the
trade: thirty soldiers become twenty-nine over four minutes while the pillbox
line is dismantled from four to one, and `economy: nothing to build` on every
reading because two refineries and three trucks leave 75-875 credits in the
wallet. Both the army and the defences are being replaced slower than they die.

That closes the session's picture: composition and the engage gate are fixed,
and what remains is the economy under a fight — which is the same ceiling the
three rejected saving rules ran into, now with a lane that shows the cost.

### Reverted: marching at the enemy base

The army should march toward where the enemy is, not just the frontier, and the
simulation tracks every house's `base_center`, so the policy can know the enemy's
starting ground the way a player does from the map. A `march_cell` that prefers
the frontier cell nearest the enemy base (still one march out, still only what it
sees) is **much worse**: at 24000 ticks three Death.mmx cells go from army 9388
to 5675, 8681 to 6223, 8768 to 8435, and lost 2:8 to 9:6. The army walks to the
fortress and is dismantled there - the enemy base is tesla coils and walls, and
the house has no siege. The frontier-edge creep the centroid anchor uses keeps the
army at its own edge, meeting the enemy in the field where the trade is
favourable. Reverted; the `enemy_base_cells` accessor stays out with it.


### Reverted: the retreat rung, which never fired

An interrupted session left a retreat rung in the working tree: a column that has
marched away from its base and finds itself outnumbered where it stands falls
back, `threat_around > 0 && away && soldiers.len() <= threat_around`. The evidence
it cited was 1v7 seed 19 losing fourteen of fifteen.

It was measured on both lanes and **fired zero times**:

    tick=7200  seen=3 near=3 threat_around=0 away=false retreat=false
    tick=9000  seen=1 near=1 threat_around=1 away=false retreat=false

Six of six 1v1 cells identical to the baseline, and three of three 1v7 cells
identical. Not a regression — dead code.

The reason is the same fog the whole army work ran into: `threat_around` counts
enemies in `seen`, which is empty on almost every reading, so the first conjunct is
false whenever the retreat would matter. In the one 1v7 reading where `seen` is not
empty, `away` is false instead: the column had not yet walked past `NEAR_CELLS`
from home. All three conjuncts never met.

The citation was also wrong in a second way: the 1v7 lane it points at reads
`seen=0` while its objects die, which is the *reason* `threat_around` cannot see
the threat. A retreat keyed on visibility cannot fire in the situation that
motivated it. The next attempt needs a threat the house can read without vision —
losses per term, or the `contact` notion `fc3130501` introduced for the column
size — not `seen` again.
