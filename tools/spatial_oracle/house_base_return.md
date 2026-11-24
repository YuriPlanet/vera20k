# Ordinary House return native corpus

The existing [House projection owner](house_base_projection.py) has an explicit
`--ordinary-return` mode. [house_base_return.json](house_base_return.json)
contains 12 additional `projection` rows and 28 `home_return` rows;
[metadata](house_base_return.meta.json) pins the original executable, Unicorn
versions, source owners and canonical payload hash. The original seven default
projection rows and their metadata remain byte-identical.

```sh
python -m tools.spatial_oracle.house_base_projection --check
python -m tools.spatial_oracle.house_base_projection --ordinary-return --check
```

Configure Python, Unicorn and the original executable as described in
[native_oracle.md](../native_oracle.md). Each selected run takes about three
seconds on the current machine. `--write` deliberately replaces the selected
corpus and its provenance; the default and `--check` write no files.

## Original execution and retained ownership

Both modes use one initialized physical House/Building/Map fixture in the
existing projection owner. Projection runs original `4FD150`, concrete
Building `+48` coordinates (`447AC0`), BuildingType `+84` cost (`45EDD0`) and
`56DC20`; it stops immediately before `4FD42F` sector aggregation or at the
existing `4FD4F8` epilogue boundary. The selected rows also record cost and
coordinate call order, signed weight totals, wrapped XY sums and every
primary/radius field write. The aggregate-sector mechanism is excluded.

Each home row runs original `500200` from entry through `RET8`. Original Unit
`7F5C70` or Infantry `7EB058` vtables select the ordinary concrete-class route.
Their `+2D4`, `+2D8` and `+2DC` methods point to original `41BF00`, `41BF10` and
`41BF20`; each body returns zero. Original `501AC0` therefore uses variant 0,
with no `RandomRanged(1,4)` draw. Original `50DEF0`, `49F420` with snap 1,
`578460`, conditional `586E50`, `56D230` and `56DC20` execute without callable
substitutions. The existing [standalone clamp owner](house_cell_clamp.py)
retains its independent 75-row `586E50` coverage.

The corpus records each function's caller, packed arguments, returned output,
ordered raw RNG draws and Scenario indices, full initial/final `0x3F4` generator
bytes, and ordered writes/final coordinates of shared Dummy `ABDC50+24`.
Original `65C6D0` initializes seed 31. The existing
[RNG owner](../rmg_oracle/gen_rng_vectors.py) independently executes original
`65C780` for the observed count; every draw and the entire final generator must
match. Recorded original code and concrete vtable spans must remain unchanged.
Observers only read state and record original writes.

## Executed facts

For both classes, radii 0, 512, 767 and 768 become 768; 769 and 2047 retain their
values; 2048 and 2049 become 2048. The corpus preserves the two raw draws for
these seed-31 rows below 2048 and six at 2048: five ranged draws, including four
rejections, then one direction draw. The final cell is `(7,9)` below 2048 and
`(12,12)` at 2048; the latter retains final Dummy `(17,17)` after original lookup
writes. Count, order and full stream state are established by execution, not
inferred from the final cell.

Home query `56D230` has two scalar inputs after the cell pointer: MovementZone
Normal 0 and bridgeAware 0. It has no SpeedType input. The following `56DC20`
call uses SpeedType Track 1 and the returned ground zone, MovementZone Normal 0,
bridgeAware 0, with packed scalar arguments
`[1,zone,0,0,1,1,0,0,0,1,0,0]`. Ordinary bridge-marked actors retain this ground
query. A supplied current ground zone 2 changes output to `(10,10)` for both
MTNK and bridge-marked E1 while the radial seed stays `(7,9)`. Raw zone FFFF is
returned as 65535; original `56DC43..56DC60` converts that value to -1 before
passability checks, and output stays `(7,9)`.

Alternate origin `(11,7)` changes the selected return to `(10,8)`. Original
`486840` gives an origin at cell level 2 coordinate Z 208, including when that
origin has the structural bridge flag; the ground-coordinate call adds no deck
height. With the supplied narrow LocalSize height 2, flat cells fail `578460`
and reach original `586E50`, producing `(6,8)`; level-4 cells pass the
height-aware predicate and retain `(7,9)` without correction.

Invalid primary `(0,0)` with no alternate produces original radial coordinate
`(-128,384,0)`; correction reaches seed/output `(4,5)`. Alternate `(8,8)` restores
`(7,9)`. Signed actor coordinate `(-257,-1,0)` supplies ground GetZone cell
`(-1,0)`, retaining native signed truncation before the packed cell conversion.

The signed Cost rows establish `Cost/1000+1` truncation: -999 contributes weight
1; -1000 and -1001 contribute 0; -2000 contributes -1 and is excluded from the
positive centroid weights. A zero or negative weight building still participates
in the second radius pass when another building supplies total positive weight
2, producing radius 1144. Reversing the same admitted Building array retains
center/radius while reversing native cost-call and weight-accumulation order.
Explicit extreme-coordinate controls retain wrapped centroid accumulation and
radius summation: the selected one-building radius is 379625056, and the
three-building wrapped sum produces 21711114. The corresponding ordinary
coordinate control produces 512.

## Supplied premises and comparison limits

The shared fixture supplies Size `(8,8)`, LocalSize `(0,0,8,8)`, allocated
17-by-17 flat cells without occupancy/overlay, uniform ground raw zone 1,
all 90 land speeds 1, frame 100, FPCW 0E7F and physical height constants 104.
Individual rows change bounds, cell level, structural bridge flag or raw zone
buffer explicitly. Zone overrides are premises, not flood-fill or topology
producer results. Negative Cost and extreme coordinates are arithmetic stress
inputs, not stock map placements. Projection supplies ordered Building records,
tracked count and cost factors independently; it does not establish their
lifecycle producers.

MTNK and E1 identify Unit/Infantry class routes with supplied postconstruction
coordinates and OnBridge state. No original object/type constructor, retail INI
reader or map loader executes here. The original methods consume the supplied
House geometry and map state; this corpus does not establish their production
origins. Mission_Rescue admission, mission scheduling, actual defender movement,
cleanup, connected retail runtime output and whole-bridge parity require their
own connected validation. These native rows are golden evidence for the bounded
arithmetic/query behavior, not closure of those mechanisms.
