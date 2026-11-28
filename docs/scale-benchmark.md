# The scale benchmark

VERA20k is meant to run 20,000 units and 30 players. This is the one battle
that claim is measured on, how it is timed, and what the numbers are.

```sh
export RA2_DIR="/path/to/Yuri's Revenge"
python -m tools.scale_bench            # the simulation tick, then the client frame
python -m tools.scale_bench sim        # only the simulation tick
python -m tools.scale_bench client     # only the client frame
python -m tools.scale_bench hashes     # equal state hashes across thread counts and runs
python -m tools.scale_bench sim --pools 4   # only the four-thread tick
```

## The battle

The battle is a map, so the benchmark and the game play the same thing.
`python -m tools.scale_map target/scale/battle30.map --size 250 --starts 30 --battle`
writes it: the flat 500 by 500 cell match map with its 30 starts, ore and
trees, and in its own `[TaskForces]`, `[ScriptTypes]`, `[TeamTypes]`, `[Tags]`
and `[Triggers]` an army for whichever house sits at each start
([tools/scale_battle.py](../tools/scale_battle.py) says how, with the engine
facts it relies on). The game loads it as any skirmish map.

It is played as an offline skirmish of one local house and 29 computer houses,
with the game's default options and seed `0x5EED0001`:

- Every start's house fields 780 units, 23,400 in all, in 78 squads of ten:
  riflemen, other infantry, tanks and artillery, mixed by the start's place in
  the grid so that neighbours field different armies.
- A front is the line between two neighbouring starts, with eight waypoints
  both houses patrol. The first squads land a few cells short of their fronts
  between seconds 8 and 14, and each squad patrols between two of its front's
  waypoints for good. A patrolling team attacks what its leader's scan finds,
  and its members pick targets themselves while they move, so the two houses
  of a front keep meeting.
- A squad is a marker team of one unit and a body team of the other nine.
  When the marker dies, a trigger lands a new marker and body beside the
  house's start, and they patrol out to the same post. What is left of the old
  body fights on. The army so keeps its size on average: it dips to about
  seven eighths around frame 1,200 and climbs back.
- The map sets three rules for itself: `[General] AINearestEnemy=yes`
  ([what it does](ai-nearest-enemy.md)), so the computer houses fight each
  other and not only the human, and `Immune=yes` and `LegalTarget=no` on the
  construction yards and MCVs, so no house is defeated and the battle never
  ends.

In a 3,000-frame run the map holds between 20,900 and 23,900 mobile objects.
From frame 500 on about 15 of them die and 15 arrive each frame, some 1,000
shots are in the air, and the frames make about 330 objects each, shots and
animations included.

`sim-bench --scenario battle` checks that the battle is the one meant: every
100 frames it prints the mobile objects alive, lost and made, and the run fails
when a measured window loses nothing or the battle falls under 20,000 mobile
objects. A change that makes the armies stop fighting cannot pass as a faster
tick. `python -m tools.run_test_map battle` plays the same battle in small
(four starts, 60 units a house, `tests/maps/battle.map`).

## What is timed

The benchmark builds both programs with the `shipping` profile and the game with
`--no-default-features`. The release workflow also applies a gameplay profile
collected on each installer’s target to its final game binary.

- **The simulation tick.** `sim-bench --scenario battle` plays 3,000 frames at
  the client's 22 ms step and times each `SimRuntime` frame. The first 300
  frames, in which the armies arrive and meet, are left out. It reports the
  mean, median, 99th percentile and largest frame of the other 2,700, with
  every CPU thread in the worker pool and again with four (`--threads 4`), the
  cores an older PC has.
- **The client frame.** The game itself plays the same map from a launcher's
  start (`vera20k -SPAWN`, [External launches](spawn.md)) with its window
  hidden, and counts its main thread's CPU time from frame 300 to frame 3,000
  ([spawn test switches](../tools/spawn/README.md)): what a drawn picture
  costs, and what the match service of one simulation frame costs (the tick's
  tail and the frame and world copy the next tick leaves for drawing, without
  the tick itself). Their sum is the client frame: one picture of one
  simulation frame, whatever the picture and tick rates were. The picture is
  1024x768 at the default zoom: the hidden window opens at VERA's launch size
  (`VERA_LAUNCH_WINDOW_SIZE`), and a match draws at its window's size.

Each figure is taken three times, each run under the build lock
`tools.cargo_run` uses, so no cooperating Cargo build runs beside it. The
report gives the median and the spread of the runs, and the benchmark fails
when the runs of a figure differ by more than 5%. Every run of the battle must
end in the same state hash, whatever the thread count.

## Numbers

Measured on an Apple M4 Mac mini (4 performance and 6 efficiency cores, 16 GB).

The baseline is the engine before the work this benchmark was made for
(2026-11-24, `main` at c8f724622 with the benchmark itself). Three runs of each
figure; the table gives their median and, in brackets, the runs.

| Figure | Baseline | Goal |
|---|---|---|
| Mean tick, every CPU thread | 85.5 ms (87.0, 85.3, 85.5) | 15 ms |
| 99th-percentile tick, every CPU thread | 138.6 ms (138.6, 138.9, 137.0) | 69.3 ms |
| Mean tick, four threads | 88.3 ms (88.1, 88.3, 90.1) | 44.1 ms |
| Client frame | 15.8 ms (15.7, 15.8, 15.9) | 7.9 ms |

The median tick was 84.2 ms and the largest 222 ms; the four-thread runs' 99th
percentiles were 142, 153 and 164 ms. The client frame was 5.3 ms for a drawn
picture and 10.5 ms for the match service of a simulation frame. The process
used 2.2 cores with every thread in the pool and 1.6 with four: the tick was
one thread's work. Every run ended in state hash `2b60126ee960919b`.

`target/scale/runs.jsonl` keeps every timed run a checkout makes.

The goal: a mean tick of at most 15 ms, and the 99th-percentile tick, the
four-thread tick and the client frame each at most half their baseline.

## Sim-scale deviations

Where the engine differs from gamemd to reach these numbers, the difference is
recorded beside its code as a sim-scale deviation (AGENTS.md) and listed here.

- **Path searches in jams**
  ([path_batch.rs](../src/sim/movement/path_batch.rs)). When eight or more
  blocked units ask in one frame for a way round every moving friendly, a
  search stops at 4,096 expansions, where the original searches on, and its
  unit waits BlockagePathDelay frames again, as a newly blocked unit does,
  looking for a way through the traffic meanwhile. Blocked units' searches
  also get a smaller budget each frame: 16,384 expansions for the ways round
  and 32,768 for the ways through, instead of 65,527 each. In the battle's
  busiest part this halves a frame's search work, and jammed units find
  their way a little later.
- **Patrol scans spread over their period**
  ([actions.rs](../src/sim/team_script_vm/actions.rs),
  `team_action_patrol_waypoint`). A patrolling team's leader looks for a
  target once every `[AI] PatrolScan=` (14 frames with the retail rules), as
  in the original, but each team on its own frame of that period, its id
  modulo the period, where the original has every team look on the same
  frame. A team so turns on a threat up to 13 frames sooner or later, and,
  as it looks on its first own turn from that frame (below), up to 16 later.
  With a modded `PatrolScan=` of fewer than four frames a team looks on every
  own turn, once every four frames.
- **Patrol scans answered side by side**
  ([scan_batch.rs](../src/sim/world/techno_ai/scan_batch.rs)). The scans of
  the leaders due in a frame are made side by side as the team step begins,
  against the world as it stood then, and a leader's scan takes its answer
  while the leader and the answer's target stand as the scan saw them. What
  an earlier team's update in the same step changes, such as an order to its
  members, a leader's scan sees a period later.
- **Picture-only particle systems**
  ([particles/spawn.rs](../src/sim/particles/spawn.rs),
  `Simulation::spawn_effect_particle_system`). In a match, an electric bolt's
  sparks and a damaged object's smoke whose system and particle types change
  nothing but the picture (`RuleSet::particle_system_presentation_only`) run
  outside the lockstep state, as the picture-only effect anims do: each frame
  runs and draws them, but they take no Logic or Display place, unique id or
  Scenario draw, announce no particle's end, are neither hashed nor saved, and
  leave the object's damage-smoke slot (`TechnoClass+0x310`) empty. A smoke
  still stops when its owner dies, crashes, sinks or cloaks, as a shared one
  does. Later objects' unique ids and draws and the state hash so differ from
  gamemd's. A player sees the same sparks and smoke, except after a load: a
  hit makes smoke only as it makes the object yellow or red, so a damaged
  object then has none until a hit makes it red, or yellow again after a
  repair.
- **Teams update on their own turns**
  ([team_ai.rs](../src/sim/team_script_vm/team_ai.rs), `run_team_ai_pass`).
  The original updates every team every frame. Here a team updates one frame
  in four, its own turn (the frame plus its id is a multiple of four), and on
  the other frames only for what cannot wait, on the frame it happens as in
  the original: a team forming, recruiting or regrouping, a member joining or
  leaving, a script action finished or `Coordinate_Attack` restarting, the
  end of a suspension or of a guard action's time, and a moving or attacking
  team whose target is gone. A team that is moving or attacking (actions 0,
  1, 3, 4, 15, 16, 22, 46 to 48, 53, 54, 58 and 59), guarding (5), doing a
  mission (11) or nothing (2) waits for its turn; every other action runs
  every frame. A waiting team so gives its members their next orders, sees
  them arrive and finds its centre up to three frames late, and the members
  a `Carryall=` aircraft's Move or an aircraft's Patrol sends to Guard by
  setting the team's target (which neither the retail rules nor the retail AI
  do) wait as long for the team to send them on. Its patrol scan
  and `Coordinate_Attack`'s every-eighth-frame check of its leader run on its
  first own turn from their frame, so each still runs once a period (but a
  patrol scan of fewer than four frames, above), and no timer a script counts
  runs longer. In the battle this takes the team step from about 14% of the
  simulation thread to about 7%.
- **Each object's turn draws its own random numbers**
  ([turn_streams.rs](../src/sim/world/turn_streams.rs), with the generator in
  [rng.rs](../src/sim/rng.rs)). The original gives every object's turn its
  random numbers from the one Scenario stream, in the order of the object
  list (`LogicClass::AI`), so the numbers a turn gets depend on how many every
  turn before it took. Here the object pass takes one number from the
  Scenario stream as it begins, on every frame, whether or not any turn then
  draws, and each object's turn, with all it does to others, draws from a
  generator of its own (SplitMix64), started from that number, the frame and
  the object's id. Ranges are made from its numbers as from the Scenario
  stream's, so a mission delay, a targeting delay, ROF jitter, scatter or gas
  drift spreads as before, but takes other values than the original's for
  the same seed. The draws outside the turns stay on the Scenario stream:
  about 96% of them a computer player's choice of what to build next, the
  rest its strategy timer and ore growth. The pass's number comes before
  each of them every frame and the turns' numbers no longer do, so they take
  other values too. Nothing new is saved; a loaded match draws this way
  again. In the battle the turns draw about 5,500 numbers a frame, in one
  turn in ten, and the draws outside them 11; on Death.mmx, 47 and 2.4.
  `sim-bench --shared-stream` draws every turn from the Scenario stream. A
  turn that draws so no longer depends on the turns before it, as an object
  pass that runs turns side by side needs. The battle comes out about 0.2%
  bloodier: on 14 seeds (battle30-v1.map, 1,200 frames, each seed run with
  and without `--shared-stream` and the two compared), 38 more units lost of
  about 17,400 (paired t 2.6), the houses' own tallies 41 more (t 2.3), 34
  more kills (t 1.8), 48 more losses (2%, t 4.2) in frames 701-800, and the
  running total 31 to 36 higher from frame 1,100 on (t about 2.1). Death.mmx
  on 20 seeds and GoldSt.mmx on 10 build and form teams as before. No draw
  changes its range or how often it is made, and no number several objects
  shared is drawn apart now; no cause was found beyond the Scenario table's
  own structure: each number is the XOR of the ones 147 and 250 draws before
  it (x(n) = x(n-250) xor x(n-147), an R250 table), which every frame ties
  three objects' draws together at fixed distances in the object list, and
  the turns' own streams do not.
