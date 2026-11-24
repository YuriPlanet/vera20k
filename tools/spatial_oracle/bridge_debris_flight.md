# Bridge debris: joined native primary-animation flight

`bridge_debris_flight.py` joins the original bridge producer to original per-frame
animation and Bounce code. The 32 saved cases were regenerated and compared with
`--check`. This is native execution evidence with the boundaries below; it is not
a complete game scheduler, rendered output or whole-bridge parity claim.

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. \
  python -m tools.spatial_oracle.bridge_debris_flight --check
```

Configure `VERA20K_GAMEMD_EXE` or `RA2_DIR` as described in
[the native oracle guide](../native_oracle.md).
The metadata records Unicorn identity, the payload hash, exact entry
points, assumptions, substitutions and the input-file hash.

## Inputs and execution

The harness imports the tracked `bridge_debris_producer.py`. Its original
`47DD70` producer selects from the native-read 15-name MetallicDebris list and
four BridgeExplosions entries. All selected constructors execute. The companion
`bridge_debris_flight.inputs.json` retains a production headless Hills export of
resolved retail ART fields, image frame counts, ordered Wake/SplashList,
HE.Wall and BridgeStrength from the release library at PR552. It extends the
producer's input with WAKE1, H2O_EXP1/2/3 and SMOKEY2. The export's missing `D`
configuration is retained as a recorded production defect, rather than treated
as native behavior: original AnimType constructor `427530` supplies `D` defaults.

For all 24 types the original AnimType constructor runs before the supplied ART
fields are written. Original image-metadata instructions `427C12..427C80` derive
Middle from the image frame count. Image pixels are absent. Stored RandomRate
values come from the production export; there is no second 900/x conversion.
This harness does not execute INI readers or a native image-file loader.

After `47DD70` returns, the harness invokes the selected metallic animation's
complete `AnimClass::AI @ 423AC0` once per incremented frame, until that animation
is no longer alive. It executes original Bounce Update `439B00`, result handling
`423930`, frame/expiry logic, immediate child constructors and Start, the landing
area-damage body `489280`, its bridge admission blocks and combat-light body
`48A620`. No bounce outcome, velocity, coordinate or RNG result is supplied.

Original Map lookups `565730` and `5657A0` use a supplied 32 by 30 cell table.
All cells have flat slope zero; the native slope matrices are initialized.
Original `47B2C0`, `439610`, `421E20` and `489100` initialize their 416-lepton
bridge offsets from supplied 104-lepton scalars. Geometry cases are:

| Case | Supplied geometry |
| --- | --- |
| ground4 / ground0 | Uniform level 4 / 0 |
| water | Level 4, Cell Land 2 |
| bridge | Level 4, raw structural flag 0x100 |
| waterbridge | Level 4 water with raw structural flag 0x100 |
| mesa | 3 by 3 level-4 plateau surrounded by level 0 |
| pit | 5 by 5 level-0 floor surrounded by level 4 |
| cliff | One level-4 source cell surrounded by level 12 |

Each geometry runs seeds 1, 10, 25 and 65, which select DBRIS2LG, DBRIS1LG, D and
DBRIS1SM respectively. The supplied source cell is (12,9). These are geometry
fixtures, not proof of a loaded map's bridge topology or the bridge driver.

## Native results

Seed 1 gives the following bounded primary-animation results:

| Geometry | Terminal AI visit | Result | Terminal coordinate |
| --- | ---: | --- | --- |
| ground4 | 62 | Stopped (2) | (4058,1450,416) |
| ground0 | 62 | Stopped (2) | (4058,1450,0) |
| water | 62 | Stopped (2) | (4058,1450,416) |
| bridge / waterbridge | 53 | Stopped (2) | (3932,1594,832) |
| mesa | 70 | Stopped (2) | (4170,1322,0) |
| pit | 53 | Stopped (2) | (3932,1594,416) |
| cliff | 9 | Bounced (1) | (3316,2298,1111) |

Dry landing constructs TWLT036, then calls area damage with exact XYZ, damage
20, HE, source/house zero and the area flag set. Water below its deck plane
constructs WAKE1 followed by H2O_EXP3 at Z+3, with no landing area call.
Water at the structural deck plane follows the dry branch.

For seed 1 on bridge and waterbridge, actual area bridge admission calls
`RandomRanged(1,1500)` at `489FF5` and obtains **348**, consuming **three raw
advances**, before the combat-light call. No bridge driver is reached because
348 exceeds damage20. The four following original raw values are
`1695085672,1901842442,1629675423,2338179239`. Other seed-1 geometries consume no
Scenario RNG after their producer and continue with
`1533642333,2057993852,571238747,1695085672`.

Stopped calls Anim Destroy `4255B0` from `423976` before constructing the expiry
animation; the outer landing branch calls Destroy again after area/light. The
native pending-delete buffer contains two entries for that pointer in these
fixtures. Bounced reaches the contact loop and then landing, with one outer
Destroy. The original drain `725C70` subsequently removes **all** matching
entries and reaches the scalar Anim destructor **once**, with an empty queue.
The scalar destructor itself is a returning boundary. Duplicate native queue
entries therefore do not mean duplicate physical deletion in this fixture.

Seed 10 constructs SMOKEY2 trailers on the native two-frame cadence, before its
terminal landing; their constructors and delay1 inputs are logged. Seed 25's D
has no Bounce body and expires on its second AI visit. Seed 65 exercises the
small debris's distinct launch and expiry type. Every visit saves the full body
bytes, truncated Location, alive state, full Scenario RNG, current frame, first-AI
guard, delay, remaining loops, frame step and timer fields. Timer start is native
+B4; stored duration is +BC; reload rate is +C0. Original `426630` computes
remaining time from the start, duration and current frame. The output also saves constructor
calls, one ordered call trace, producer/flight RNG states and raw advance counts,
and four native continuation draws. Child-construction events carry their AI
visit number. `drain` records the queue counts, scalar destructor call and
readable-pointer checks.

## Explicit boundaries

- **Only the primary metallic animation is AI-scheduled.** The sibling bridge
  explosion, trailers, expiry, wake and splash animations are constructed by
  original instructions but receive no later AI visits. Their later sound,
  middle-frame smudge, frame-timer and RNG behavior is outside this comparison.
  Do not compare these RNG states with an unrestricted whole-world tick.
- Sound indices remain constructor -1 even where the retained ART export names a
  Report. This declared input substitution suppresses audio Start effects.
- The cell object lists are empty. The cliff cases prove physical Bounced, not a
  hit on a centered stock tree. Conditional Terrain contact and its damage/
  removal are covered separately by `terrain_debris_receiver.py`.
- Concrete bridge admission uses supplied tile1020, base1000, middle20 and a
  shared anchor overlay18. If a bridge driver is called it returns supplied
  false and is logged; collapse physics is outside this corpus.
- Object Mark and Display Submit return1; Display Remove and the empty BombList
  invalidation boundary return0. Allocation uses a bump allocator; delete is a
  no-op. Native animation/Object Destroy, UnInit, Limbo and pointer-expiry bodies
  still execute. Other object/house/Techno observers and the common Logic vector
  are empty; the animation vector contains all constructed animations.
- Pending-vector initializer `725850..725886` executes before the supplied
  capacity/buffer. Terminal drain `725C70` executes unchanged, including native
  validity and RTTI/class checks. A supplied empty SEH chain at FS:[0] supports
  the CRT. The imported Windows IsBadReadPtr call at `7CAA5E` returns supplied
  false after the fixture confirms that its memory is mapped; scalar Anim
  destructor `426590` records one call with flag1 and returns. This proves the
  drain's compaction/call count, not the destructor or populated observers.
- The original combat-light body sees zero light-configuration fields; actual
  light creation and pixels are not established. All native executable
  instructions remain unchanged.

The Rust consumer should exercise the production producer and animation owner
under the same primary-only scheduling boundary, including exact map flags,
height, type configuration and child-construction ordering. Separate production
validation is required for the complete mixed animation scheduler and rendering.
