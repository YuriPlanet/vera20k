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
| [Tiled flow fields](https://www.gameaipro.com/GameAIPro/GameAIPro_Chapter23_Crowd_Pathfinding_and_Steering_Using_Flow_Field_Tiles.pdf) | Share a reverse search among movers heading for the same goal or portal. | Best candidate for a separate opt-in mass-order mode. First measure portal/goal reuse with real formation offsets. Key static fields by topology revision, layer and movement profile; retain live cell admission and A* fallback for traffic and special targets. |
| [Weighted Jump Point Search](https://harabor.net/data/papers/cmhse-aaai23-jpsw.pdf) | Prune symmetric paths on a weighted grid. | Preserves an optimal path under its own movement and tie rules, not the stock route. Directed entry, tubes and changing costs require a new pruning proof. |
| [HPA*](https://webdocs.cs.ualberta.ca/~mmueller/ps/2004/hpastar.pdf) | Search a portal graph instead of the full grid. | The engine already narrows the search through zones and repairs them after terrain changes. Measure corridor retries before introducing another hierarchy. |
| [ALT landmarks](https://www.microsoft.com/en-us/research/publication/computing-the-shortest-path-a-search-meets-graph-theory/) | Guide A* around static obstacles using precomputed distance bounds. | Useful for a separate static graph. Even a valid bound changes node order, ties and the stock batch budget; the distance tables also cost memory per graph. |
| [D* Lite](https://dl.acm.org/doi/abs/10.5555/777092.777167) | Reuse work after a small set of edge changes with a stable goal. | Consider it for repeated long routes with sparse topology edits, not as a retained search per Foot in a moving jam. |

Every one of them changes which cell is generated first, so every one of them changes
the route, the ties and the batch's expansion budgets. That is why none of them can
be switched on behind a build feature: they would need their own opt-in, their own
compatibility rules and their own validation, and with retail data the game has to
play as `gamemd.exe` plays it.

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
| `PathfindWorkspace::with` | ~19% | The per-search workspace. The reset is inlined into the thread-local access: the two closed layers clear their word ranges before the search starts. |
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
the heap are gamemd's costs, ported. The workspace is VERA's own scratch: what a
search clears before it starts is VERA's choice, and it is the largest piece of
the search that no native instruction requires.

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
`an_integer_square_root_matches_the_x87_stored_root` and against the original
instruction results by the existing `astar_structural_height.json` node-cost rows,
and the memoized table is pinned against its own arithmetic by
`the_memoized_mantissa_table_is_the_computed_one`.

The profile above is why no further work on the A* kernel is queued: the frame is
bound by the entry asks and by the memory they touch, so the next lever is the
number of asks a search makes, not the price of an ask. The one place where VERA
pays for something no native instruction asks of it is the per-search workspace,
and that is where the next experiment starts.

To compare release builds from the same machine and retail install:

```sh
python -m tools.scale_map target/scale/flat500.map --size 250
python -m tools.cargo_run -- build --locked --release -p vera20k-core --bin sim-bench
bench=$(python -m tools.cargo_run --resolve sim-bench --profile release)
"$bench" --map target/scale/flat500.map --scenario march --seed 1 \
  --units 20000 --houses 30 --ticks 300 --frame-hashes target/scale/march.hashes
```

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
