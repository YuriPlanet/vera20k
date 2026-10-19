# Temporal-held vehicle queue promotion (#1317)

This adds stock GAWEAP/MTNK/FV controls to the existing physical
[Factory input/VM owner](factory_loss.md). Prepare the same extracted cache plus
`VEHICLE_ASSETS` in `temporal_vehicle_promotion.py`, using the existing asset
extractor; `ABSENT_ASSETS` names the physically absent optional barrel/turret
files. Retail bytes are local inputs and are not distributed.

```sh
VERA20K_FACTORY_SIDEBAR_ASSETS=/path/to/extracted-cache \
  python -m tools.sidebar_oracle.temporal_vehicle_promotion --check
```

Original sparse type/country/ART/rules readers, Unit constructors, admission,
refund, cancellation, queue promotion and charging execute. The supplied base
has GACNST/GAWEAP, clear ground and credits10000. Original BeginProduction
(`4FA350`) starts MTNK and queues FV; FactoryAI (`4C9B20`) pays the first MTNK
step. A declared lone Temporal link enters original notice `71B0EA..71B14E`,
which calls Building pause `4521C0`. No factory-online answer, held-start flag,
progress or charge result is supplied.

Whole HouseAbandon (`4FAA10`) cancels MTNK and calls StartNext (`4CA5A0`),
which calls Begin(force1) at `4CA60A`. With only the warped GAWEAP, online
FindFactory fails; offline fallback `4FA45B` supplies Resume(`4C9EA0`, arg1).
That affordable start retains FV at stage0 with no step timer and sets the
recovery latch. Original LetGo (`71ABC0`) restores the building, but neither
release nor a later explicit UpdateFactoryQueue (`509140`) resumes that latch.
A same-type PRODUCE/Begin with arg0 resumes the existing FV. An independently
supplied second online GAWEAP exercises the running-promotion alternative;
its later same-FV PRODUCE queues another copy, because the head is running.

The Rust consumer is
`sim::production::factory::system_hold_tests::temporal_promotion_tests`.
Its actual command/frame regression covers vehicle, infantry, ship and aircraft
production, each with/without an online backup. It consumes native pause/latch,
stage/debt/timer and wallet observations for the offline vehicle promotion, and
the common admission/resume outcomes for the other stock categories. Aircraft
uses the shared wildcard active-head abandonment boundary: indexed cancellation
of the same ORCA type would cancel its queued copy first. The scenario's healthy
base, Temporal initiation and that wildcard call are explicit fixture inputs.

This protects the existing implementation from PR #1809 rather than adding a
second system-hold owner. Existing system-hold controls additionally cover user
versus system suspension, later-callback recovery, insufficient funds and the
constructor's RNG. The new controls are not a complete native CLEG firing or
whole-frame/pixel/audio comparison: linkage, admitted preplaced buildings,
selected FactoryAI sample frames and the later callback are declared boundaries.
Rust full-frame ticks need not align with those native sample frames. Full
VFS/theater precedence, native SAV and every modded type remain outside this
bounded comparison.
