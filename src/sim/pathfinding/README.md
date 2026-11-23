# Pathfinding at scale

Stock play uses cell-level `AStar_main_loop` (`0x00429A90`) with the three-level zone
ladder of `AStar_pathfind_search` (`0x0042C900`), already implemented in
[`zone_search`](zone_search.rs). Cell admission and cost depend
on direction, layer, the mover, occupancy, bridges and re-path urgency. The first
node generated for a cell and layer owns it, even if a cheaper route appears later.
The [path batch](../movement/path_batch.rs) also uses expansion counts to
choose which re-paths get an answer this frame. An equally short path is therefore
not sufficient evidence that a different search preserves stock play.

| Approach | Where it helps | Fit here |
| --- | --- | --- |
| [Tiled flow fields](https://www.gameaipro.com/GameAIPro/GameAIPro_Chapter23_Crowd_Pathfinding_and_Steering_Using_Flow_Field_Tiles.pdf) | Share a reverse search among movers heading for the same goal or portal. | Landed as an opt-in mass-order mode: `[VERA] SharedGoalField=1` gives the movers of a mass order one reverse search for their common goal. See "The mass order and the shared goal field" below. Key static fields by topology revision, layer and movement profile; retain live cell admission and A* fallback for traffic and special targets. |
| [Weighted Jump Point Search](https://harabor.net/data/papers/cmhse-aaai23-jpsw.pdf) | Prune symmetric paths on a weighted grid. | Preserves an optimal path under its own movement and tie rules, not the stock route. Directed entry, tubes and changing costs require a new pruning proof. |
| [HPA*](https://webdocs.cs.ualberta.ca/~mmueller/ps/2004/hpastar.pdf) | Search a portal graph instead of the full grid. | The engine already narrows the search through zones and repairs them after terrain changes. Measure corridor retries before introducing another hierarchy. |
| [ALT landmarks](https://www.microsoft.com/en-us/research/publication/computing-the-shortest-path-a-search-meets-graph-theory/) | Guide A* around static obstacles using precomputed distance bounds. | Useful for a separate static graph. Even a valid bound changes node order, ties and the stock batch budget; the distance tables also cost memory per graph. |
| [D* Lite](https://dl.acm.org/doi/abs/10.5555/777092.777167) | Reuse work after a small set of edge changes with a stable goal. | Consider it for repeated long routes with sparse topology edits, not as a retained search per Foot in a moving jam. |

Every one of them changes which cell is generated first, so every one of them changes
the route, the ties and the batch's expansion budgets. That is why none of them can
be switched on behind a build feature: they would need their own opt-in, their own
compatibility rules and their own validation, and with retail data the game has to
play as `gamemd.exe` plays it.

## Families that answer a different question

These change what the engine computes rather than how it searches, and the
original engine never asked the question they answer.

| Approach | What it answers | Fit here |
| --- | --- | --- |
| [Conflict-Based Search](https://dl.acm.org/doi/10.1016/j.artint.2014.11.006) (multi-agent path finding) | A set of paths in which no two agents occupy a cell at the same tick. | Not a fit. CBS needs every agent on one shared clock; gamemd gives each Foot its own path and settles the jam in the movement rules. Its low level is a pathfinder, so it could sit on the current one, but the shared clock and the collision-free guarantee are not what VERA simulates. |
| [ORCA](https://gamma.cs.unc.edu/ORCA/publications/ORCA.pdf) | A velocity that avoids the agents around it, without communication. | Not a fit. ORCA is continuous-space steering for robots; VERA's units move cell to cell, and the crowd is settled by the pathfinding and the movement rules. A velocity layer would be a different game. |
| [Symmetry-based search space reduction](https://ijcai.org/papers11/Papers/IJCAI11-480.pdf) (RSR) | Prune the empty rectangles of a map and connect their sides with macro edges. | A candidate for a static map only. The macro edges are a different graph, so the route and the ties change, and VERA's terrain, bridges and occupancy make the rectangles a poor fit. The zone ladder already prunes by topology. |
| [Clearance-based pathfinding and HAA*](https://users.cecs.anu.edu.au/~dharabor/pathfinding.html) | One abstract graph for agents of several sizes and terrain types. | A candidate for the same reason as RSR: a different graph, a different route. The zone ladder already narrows the search and repairs it after a terrain change. |
| Search-free pathfinding (Botea, "Ultra-Fast Optimal Pathfinding without Runtime Search", AIIDE 2011) | A route read from a precomputed portal table instead of searched. | A candidate for a mass-order mode, with the same precondition as flow fields: many movers have to share a goal or a portal. See the flow-field row above. |

## Where the frame time goes

A release profile of the 20,000-unit march (`sample` on the running benchmark, one
millisecond per sample, seven threads of 4,140 samples each) puts the pathfinding
chain at about half of the machine's samples. The parallel path batch takes 2,372
of the main thread's 4,140 samples, and every one of them is `pthread_cond_wait`:
the main thread waits for the workers. Each of the six rayon workers spends 2,271
to 2,372 of its 4,140 samples inside the batch job, and the workers sleep through
the rest (9,893 of 24,840 worker samples), so the machine is idle about 40% of
the time the search runs.

Inside the search, the workers' own samples with the idle and the kernel time
left out:

| Frame | Share of the search | What it is |
| --- | --- | --- |
| `classify_foot_entry` | ~26% | The native `Can_Enter_Cell` ask (`+0x1AC`) for every neighbour of every expanded cell. |
| the kept facts | ~23% | Occupancy, terrain and blocker reads behind the ask: `EntryFacts`, `CellOccupancy::get` and the cell object list. |
| `PathfindWorkspace::with` | ~19% | The per-search workspace accessor, with the part of the search the compiler inlined into it. |
| the open list heap | ~8% | `sift_down` and `push`. |
| `node_costs` | ~7% | g and f, the heuristic included. |
| the zone precheck | ~5% | `search_precheck_level_in`. |
| the terrain reads | ~5% | `native_cell_identity` and `native_cell_lookup`. |

The arithmetic of the heuristic is a small part of the frame, and the search is
bound by how many entry asks it makes and by the memory those asks touch, not by
how many floating-point instructions they use. That is also why the algorithmic
families above cannot pay off here by themselves: they change the number of
nodes, not the price of an ask.

One row is not native-mandated. The entry ask, the heuristic, the zone ladder and
the heap are gamemd's costs, ported. The workspace is VERA's own scratch, but the
measurement says its reset is not where the time goes: a search that cleared
nothing at all measured the same. What the row holds is what the compiler inlined
into the accessor, and the work that reaches the cells is native's shape.

The reset is cheap because of what the batch asks of it. Over the 300 ticks of the
benchmark 2,753,339 path requests wait for an answer, about 9,178 a tick, and the
batch answers 31,957 of them, about 106 a tick; the other 2,721,382, about 9,071
a tick, are left out unsearched. A search clears only the words the last search
wrote, so the clearing is bounded by the searches that ran and by what they
wrote: a few thousand words a pass next to the 2.6 million neighbour tests a
pass makes. A search that cleared nothing would save that much and no more.

The profile is of the flat 250x250 benchmark map, so its entry asks are the plain
path's: a search there floods mostly open ground, and every ask of a neighbour reads
the cell's kept facts. A map with bridges and cliffs asks more per node and reads
more per cell.

## What was tried, and what it measured

Each change below keeps the 300 frame hashes, the batch's answered and deferred
counts and the final state hash identical, and was measured as a release build
against the same machine, the same retail install and the same map, in alternating
pairs so that neither build systematically benefits from running first.

| Change | Result |
| --- | --- |
| Re-path budget checked every 256 expansions instead of 1,024 | Rejected. It removed 0.71% of expansions and did not lower mean frame time. |
| A 3x3 closed-bit stencil read in three row runs | Rejected. It cut expansions by 0.03% and raised p95 by 12.97%. |
| The heuristic's root computed from the integer square, the mantissa table still computed per root | Rejected. The square root left the A* loop, and the fastest of the runs got 2% to 6% slower. |
| The heuristic's root computed from the integer square, with the mantissa table computed once | Landed. Bit-identical output, one square root and one x87 store fewer per node, frame time unchanged. |
| The closed layers' reset replaced by a per-word generation, so a search clears nothing | Rejected. Two rounds of alternating release runs: the variant's fastest run was 2.25% and 1.77% slower than the baseline's. |

The frame time of this benchmark moves by up to 20% with the machine's load, so a
build measured against another build in one sitting proves nothing on its own. Every
number above comes from alternating the two builds and comparing the fastest run of
each, with the 300 frame hashes, the batch's answered and deferred counts and the
final state hash identical in every pair.

The landed change is arithmetic, not speed: `Sqrt_Approx` (`0x004CAC40`) stores its
argument as a binary32 that the chop control word truncates to 24 significant bits,
and the table index reads only the top 14 of them, so the dropped bits never reach
the result. The root of an integer square therefore comes out of the integer alone,
without the x87 load and store the A* used to pay for every node, and the table's
16,384 entries are computed once instead of once per node.
[`util::native_x87::sqrt_approx_square_bits`](../../util/native_x87.rs) is that path;
it is pinned against the x87 model by
`an_integer_square_root_matches_the_x87_stored_root` and
`the_integer_root_matches_the_x87_stored_root_over_its_range`, and against the
original instruction results by the existing `astar_structural_height.json`
node-cost rows. Those rows reach squares below 2²⁴, so they pin the range the
search actually asks of and not the chop above it; the chop is pinned against
the x87 model instead, and the store that decides it is `X87Chop53`'s, which the
new path goes through rather than repeating. The memoized table is pinned
against its own arithmetic by `the_memoized_mantissa_table_is_the_computed_one`,
and the retail table itself by `map::rmg`'s `sqrt_table_matches_retail_at_the_pinned_points`.

The profile above is why no further work on the A* kernel is queued: the frame is
bound by the entry asks and by the memory they touch, so the next lever is the
number of asks a search makes, not the price of an ask. The one place where VERA
pays for something no native instruction asks of it, the per-search workspace,
has now been measured and comes to nothing: clearing it per search is free. The
remaining lever is the shape of the search itself, and every algorithm that
changes the shape changes the route with it, so it belongs behind an opt-in of
its own rather than in the stock path.

## The mass order and the shared goal field

A click gives every selected unit the same destination cell: the client commits
one `Command::AttackMove` per selected object, each with the clicked cell
([`click_command`](../../app/input/context_order/click_command.rs)). Six hundred
tanks ordered onto one cell ask for six hundred paths to that cell.

The batch answers them as six hundred searches. Over the benchmark's 300 ticks
the registrations are three quarters of all the search work: 236,000 of the
323,000 expansions a tick are the registrations, the rest are the jam re-paths,
and the batch answers about 106 of them a tick under its cap of 256.

That is the mass order the survey's first row answers, and the mode is landed:
`[VERA] SharedGoalField=1` in the rules or the map turns it on
([`rules::vera_features`](../../rules/vera_features.rs)). The registrations that
head for the same goal cell share one reverse search over the map's ground
plane. The field gives every cell the step that shortens its way to the goal,
and each mover's path is a walk down that gradient instead of a search of its
own.

What the mode does, and where it stops:

- The field's plane is the path grid's ground walkability, which is VERA's own
  ([`PathGrid`](core.rs) has no native counterpart): it knows rock, water,
  overlays and terrain objects, but not the mover's SpeedType row and not the
  units standing in the cells. The field's route is therefore the shortest one
  in steps over that plane, and not always the route the mover would have
  searched for.
- The mover's own admission still decides every cell of the walk
  (`Foot+1AC`, `0x00429F54`), as it decides every cell of the search. A cell
  the mover refuses sends that search back to the stock one, so the field is a
  guide and never a route of its own.
- A mover on a deck, or one heading for a deck, keeps the stock search: the
  field's plane is the ground one, and a bridge is not on it.
- An infantryman keeps the stock search: his cell gate is the Infantry one
  (`0x0051BF90`, against the vehicle's `0x0073F0A0`), and it lets him into a
  cell a terrain object stands in, so his plane is not the field's. The class
  decides this, not the locomotor: a vehicle that walks is still gated as a
  vehicle, and walks the field's plane.
- The walk is not a corridor search. The hierarchy's zones are the native
  search's own filter over the cells it expands (`0x00429E7F..`), and the
  native drops them when its precheck fails and searches the whole map plain
  (`0x0042CB8B..0x0042CBA1`, `0x0042CB22..0x0042CB3F`). A field is shared by
  every mover heading for the goal, and the corridor is one search's own route
  from its own start, so the walk cannot hold to it for all of them.
- The field is kept between passes, so a mass order that names its goal every
  frame pays for the build once. The ground walkability plane's revision names
  its state, and a cell that opens or closes (a bridge lost, a wall built)
  drops the fields the batch holds.
- A group whose own paths would not repay the build keeps the stock search.
  The build visits every cell of the plane once, and the searches it replaces
  ask the Foot about the eight neighbours of every cell they expand, so the
  build pays for itself when the group's paths cover the map sixty-four times
  over ([`FIELD_BUILD_WORTH`](../movement/path_batch.rs)).

Because the routes change, a game that turns this on plays differently from a
game that does not, and its frame hashes do not hold.

The benchmark's march gives every unit its own formation-offset destination, so
its registrations share nothing; the reuse comes from the order a player gives,
not from the benchmark's. `sim-bench --shared-goal` gives the march the player's
order instead, so that the registrations of a house share a goal and the mode
has something to answer.

## What the shared goal field measured

The release build, the flat 500x500 march, 20,000 units, 30 houses, 300 ticks,
seed 1, in six configurations over three alternating rounds:

| | the load | the stock search | the shared field |
| --- | --- | --- | --- |
| C vs D | tanks and infantry | 96,104,932 expansions | 92,031,902 (−4.2%) |
| C vs D | the same | 2,726,612 left out | 2,699,497 |
| E vs F | tanks only | 209,391,078 expansions | 56,317,215 (−73%) |
| E vs F | the same | 4,457,527 left out | 3,190,317 (−28%) |
| E vs F | the same | 0 fields | 30 built, 145 used, 3,742,500 cells visited |

The counts are the mode's own, and they are the same in every round: the batch
answers a registration or leaves it waiting, the field is built and used, and
the expansions are the search's own. Only the expansions drift a little between
rounds, because they include the cancelled speculative work of a parallel
search, whose amount depends on how the workers got on.

What the numbers say:

- The mode removes the search of every registration the field serves, and with
  it most of the expansions: on the tanks-only load, three quarters of them.
- The wait is what a player feels: on the tanks-only load the registrations
  left waiting for an answer fell by 28%, because a walk down the field is
  cheap enough that the batch answers more of them a tick.
- The field's own work is a visit of every cell of the plane per build, and it
  is not small: on the mixed load the 102 builds visited 12,724,500 cells,
  three times the 4,073,030 expansions the field took away. That is why the
  mixed load's expansions fall by 4.2% and no more. On the tanks-only load the
  30 builds visited 3,742,500 cells against 153,073,863 expansions saved.
- The mixed load shows less than the tanks-only one because the infantry keep
  the stock search: their cell gate is the Infantry one (`0x0051BF90`), which
  lets them into a cell a terrain object stands in, so the ground plane is
  not their plane. Half of that load's units are infantry, and none of their
  searches are replaced.
- The frame time does not move on the mixed load, and falls on the tanks-only
  one. The frame times of the three rounds, each pair adjacent in time because
  the bench alternates its configurations within a round:

  | the mean frame time | round 1 | round 2 | round 3 |
  | --- | --- | --- | --- |
  | C vs D: tanks and infantry | 138.29 / 138.91 | 336.01 / 190.38 | 154.70 / 155.98 |
  | E vs F: tanks only | 211.64 / 202.79 | 266.42 / 250.25 | 235.63 / 149.05 |

  The machine's own load moved the frame time by twofold between rounds: the
  baseline configuration ran the same build on the same work three times and
  took 149 ms, 279 ms and 334 ms. So only the sign of a paired difference is
  worth reading, and the counts above, which do not depend on the load, are
  the measurement. On the mixed load the two quiet rounds agree within one
  percent, so the field does not move that load's frame. On the tanks-only
  load the field's load is the faster one in every round, by four percent to
  thirty-seven.

The mode's price is in the jam, and it is the price the docs name: the movers
of a house share one route, so a funnel that is a cell wide becomes a queue.
On the mixed load the jam re-paths grew from 5,197,613 expansions to
5,946,136 (+14%) and the live object count fell (19,309 to 19,220 at the end
of the run), because the shared route jams where the spread-out routes did not.

So the mode is not a frame-time win on a flat map with a mixed load. It is a
win on the load it was built for: many vehicles, one goal, a map where the
search is the cost.

To compare release builds from the same machine and retail install:

```sh
python -m tools.scale_map target/scale/flat500.map --size 250
python -m tools.scale_map target/scale/flat500-vera.map --vera-shared-goal-field
python -m tools.cargo_run -- build --locked --release -p vera20k-core --bin sim-bench
bench=$(python -m tools.cargo_run --resolve sim-bench --profile release)
"$bench" --map target/scale/flat500.map --scenario march --seed 1 \
  --units 20000 --houses 30 --ticks 300 --frame-hashes target/scale/march.hashes
```

The map carries the switch, so the two sides of the comparison are the same
binary on two maps that differ only in their `[VERA]` section. The map's path
has to be absolute: the loader resolves a relative one against the retail
root.

Set `RA2_DIR` to the retail install. Preserve each build's `march.hashes` before
running the other and compare every line as well as the batch's answer and deferral
counts. Actual expansions can differ: they include cancelled speculative work, whose
amount depends on worker progress. Compare those counts and frame times separately.
Run the two builds in alternating pairs, not one after the other, and compare the
fastest run of each: on a loaded machine the second build of a pair otherwise carries
the machine's drift, and the load moves the frame time by up to 20%.

For an opt-in algorithm, also measure same-goal and formation-offset loads separately,
exercise bridge layers and topology changes, and measure arrival and queue delays.
Changes to route selection need explicit opt-in and replay/online compatibility
validation; a single flat-map throughput result is not enough.
