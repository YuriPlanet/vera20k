# Playing the v2 policy

This page is for the machine, not for a reader of the code: how to run the
build, what the house is expected to do, and what to write down when it does
not.

## Running it

    python -m tools.cargo_run -- build --profile fast
    the binary lands in target/owned-worktrees/ID/fast/vera20k.exe

The default policy is the retail one. To drive the house with the new policy:

    set VERA20K_BRAIN=v2
    vera20k.exe

Turn it off again by clearing VERA20K_BRAIN from the environment.

## The keys

The four auto play keys have no default binding: assign them in the hotkey
settings under the vera20k category, or they are still bound from an earlier
session.

- the master key: production, forces and abilities all on or all off;
- one key each for production (the base's own work), forces (the army) and
  abilities (the superweapons).

Auto play starts off in every match, and the host belongs to the match, not to
the account: a new scenario clears it.

## What the house should do

In the first ten minutes, with the map's own ore and the usual opening:

1. The maker (MCV) unpacks into a Construction Yard.
2. A power plant goes up next to it, and the house stops being in the dark.
3. A refinery goes up on the ore, and a harvester starts shuttling.
4. A barracks and a war factory follow.
5. Units queue from the factories, and the army forms at the base.

Then, once the column reaches `march_at` (twenty soldiers while the house has
seen nothing for four minutes, thirty once it has been in contact), the army
**leaves the base** and walks at the nearest edge of ground the house has not
revealed. It fights what it meets in the field, and a unit whose fight ends far
from home walks back.

The attack flag is on in this build. Measured on 1v7 easy over 24000 ticks the
house survives three of three seeds; on 1v7 normal it is destroyed at 17188. So
an easy match should hold and a normal one will not — that is the expected
behaviour, not a defect to report.

Two things it does not do, whatever the wish list says: it never attacks the
enemy base (the column meets the enemy in the field and turns back), and it
builds nothing after its fourth defence. `economy: nothing to build` in the log
is that ceiling, not an error.

## The switches, and how to see them

- production off: no new building is queued or placed. The house keeps its
  units and stops growing.
- forces off: the army stops taking orders. The base still builds.
- abilities off: the superweapons are not fired by the house. You can still
  fire them yourself.

The HUD line names the policy and the switches that are on.

## The log

Run with RUST_LOG=info to get the policy's own lines:

- start: map, country, credits, unit_count, tech, own, enemies, roster - the
  opening this match began from.
- policy v2: tick, proposals, intents, assigned, issued, unowned, refused,
  finished, stalled, ineffective, cooled, fallback, stuck - one line every
  ten seconds.
- policy v2 unit: id, type, job, target, standing - every unit, the job it
  holds, and how long it has stood where it stands.
- military: tick, soldiers, want, seen, near, going, advance - what the army
  decided, and why.

## What to write down when something is wrong

The unit lines are the report. For a unit that should be doing something and
is not, write down:

- the unit id and its type, from the unit line;
- the job it holds and its target;
- the standing frame count;
- what you expected instead (for example: it should have been attacking the
  tank at 100,120).

With the id, the match can be replayed to that frame and the decision read in
the log, instead of guessed at from the screen.
