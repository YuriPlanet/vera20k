# Bounded native House power consumers

`house_power_consumers.py` preserves 44 controls from the original executable
research packet and adds two executions of the same radar provider scan and
four executions of the complete shared blackout setter. It
composes the existing `building_repair`, `factory_cadence`, `time_to_build` and
Construction fixture owners. Original PE instructions decide health, power,
Factory rate and cadence, blackout timers, radar availability and EVA advice;
no gameplay result is supplied by a hook.

The original is active-retail `gamemd.exe`.
The payload records every reached instruction boundary, original byte hashes,
entry order, complete Main/Scenario RNG buffers and unchanged executable
sections. The shared oracle owner refuses to publish
if an imported fixture owner changes during the run.
The 44 retained histories have identical native state and instruction receipts
to the source research packet. Only portable provenance and explicit startup
wording differ.

## Reproduction

Use the environment in [the shared native oracle guide](../native_oracle.md),
then run from the checkout:

```sh
python -m tools.spatial_oracle.house_power_consumers --check
```

`--write` deliberately records new native execution. Expected values must never
be generated from Rust. The ordinary check writes no files. The `.json` and
`.meta.json` pair carry the comparison and native identity without
depending on machine-specific evidence paths or copied retail assets.

## Inputs and original boundaries

The prior contains an admitted, marked Strength750/Power200 Building with actual
and sampled Health374, an admitted drain100 consumer, a zero-power Radar provider,
House/country/type vectors and a Cost600 pending Unit. Original vtables come from
the existing fixtures. The consumer rates use supplied BuildSpeed `.7` widened
from f32, MultipleFactory `.8` f32, Country/BuildTimeMultiplier1 f32 and low-power
factors1/`.5`/`.8` f32. RepairStep8, RepairPercent15%, RepairRate `.016` widened
from f32, SpeakDelay2 and MessageDelay `.6` widened from f32 are explicit scalar
bindings. These comparisons add no rules-reader/default claim. Constructors,
Unlimbo and arbitrary damage are outside the prior.

The two repair histories begin with an explicitly live Factory (`+70=0`),
rate8 and timer `{100,0,0}`. Executed `4C9EA0` returns false and preserves that
timer because the Factory is already running. Native Factory AI produces the
frame195 joined prior: stage12, rate8, timer start188/duration8, balance468,
credits4868 and spending132. This is **not** a comparison of Begin_Production
startup. The Rust joined tests adopt that exact recorded frame195 prior.

Whole global Factory loop `55B66A` enters Factory AI `4C9B20` before House
Update `4F8440`. The House execution stops at `4F8511`, after the power/radar/
SpySat prefix. Whole power `508C30` calls Factory rate rewrite `4CA6E0`, which
executes Time_To_Build `6F47A0`. Later advice `4F8B08..4F8DB1` executes separately,
with the interposed ordinary House state supplied. Building sampling is original
`440042..440072`; whole paid repair is `450630`.

## Executable results

| Original controls | Bounded finding |
| --- | --- |
| Two repair histories | With clean power, repair196 changes actual HP374 to382 while sampled HP374 and output99/drain100 remain. Sample197 invalidates power and radar; the House then publishes output101 and rate6. A supplied prior power-dirty byte exposes HP382 in House196 instead. |
| Both Factory histories | Global Factory AI advances stage13 at196 using rate8 and arms duration8. The subsequent rate6 rewrite preserves that timer. Stage remains13 through202, advances at204 with duration6 and again at210. |
| Nine rate controls | Full `4CA6E0` updates live, held and completed owned nonnull objects, gives owned null rate1 and retains foreign rate77. Only rate byte `+38` changes; timer bytes `+2C..+37` remain. Cost and power controls execute both clamps. |
| Six blackout controls | Remaining1 resets to `{196,0,0}`, invalidates and assesses even with prior dirty0. Remaining0 and2 with prior dirty0 preserve cached totals; with dirty1, remaining0 assesses and remaining2 forces output0. |
| Four blackout setter controls | Whole `50BC90` stores current frame196 and replaces the duration with30,4,80 or0, including shorter and zero replacements over a supplied running `{100,0,120}` timer. It sets power dirty1, preserves radar dirty and retained totals, and makes no RNG draw. |
| Sixteen radar controls | Dirty clears even for a nonlocal House, but only PlayerPtr changes the native local result. FreeRadar follows the separate radar-outage gate. Selling, offline, unmarked and limbo candidates are skipped. The first otherwise eligible EMP/warp candidate decides false. A first Selling candidate followed by a good provider gives true. |
| Thirteen advice controls | Local shortage with a counted type among the first three `Rules+0x8B0` entries (the `[AI] BuildConst=` items; the harness names them `POWER_TYPES`) sets the low-power guard and requests EVA once; restoration clears it. SpeakDelay2 produces native durations14400,7200,4800,3600,2880,2400,2057,1800 at stored speeds0..7. |

The added provider cases execute the same complete `508DF0` body: independent
online1/warp1 followed by a good provider gives false; first Selling followed by
a good provider gives true. No producer reachability is inferred from those
supplied flags.

The setter controls supply numeric duration, frame, prior timer, cached totals
and dirty bytes. Original `50BC9C` sets House `+5778`; `50BCAA` stores the frame
and `50BCB7` stores the duration without consulting the prior timer. Its opaque
timer `+2A8` receives explicit entry-SP minus8 stack residue0 at `50BCB0/B4`;
no represented reader of that dword is established. Each row executes all13
original instructions through `RET4`, with unchanged Main/Scenario buffers and
PE sections. Caller conversion and ForceShield/Spy activation are outside these
setter controls; the native ForceShield caller `6CD18B` invokes this same owner.

## Rust comparisons and limits

[`engineer_power_consumer_tests.rs`](../../src/sim/world/engineer_power_consumer_tests.rs)
compares 106 joined state boundaries, all six timer/power controls, all four
replacement setter controls, nine local
radar controls with represented inputs and twelve local advice rows. Its extra
zero-power producer realizes the supplied factory counter1 for eligibility;
its admission is outside the comparison. The tests reuse Simulation's health
sample/invalidation/assessment owner, the production Factory sweep, the private
derived radar owner and the existing later EVA owner.

The nine multi-factory rate/status controls remain available to the Factory
owner's tests. The world comparison does not manufacture several same-class
House slots to simulate them.

Native radar availability is local Tactical `+14D8`, read `656DE0` and written
`656DF0`; it is not persisted House output. VERA's private per-owner cache is a
derived projection. Nonlocal native radar rows are therefore not equated with
that projection. EMP, a live Spy radar-outage timer, independent online/warp
states and their producer paths remain native-only controls. The offline
predicate has only Temporal's writer represented by VERA; independent raw
online0 and online1/warp1 priors are not asserted as Rust equivalents. The six
blackout tests compare timers, power and dirty bytes; their independently
supplied local radar prior is outside that comparison.

The native low-power timer `House+57BC` is written in advice, with no consumer
established. Rust compares the existing delay calculation and EVA requests
without adding unread timer state. Native process-global advice guard versus
VERA's per-human guard is the existing documented deterministic projection;
only local-player rows are compared here.

The repair fixture retains its synthetic N00/N02 damage-slot requests and
GetCurrentFrame/flash/slot-allocation/voice-device interfaces. Real Anim
allocation, IDs, lifecycle and its RNG footprint are excluded. Main and
Scenario remain unchanged **within these supplied boundaries**; that does not
certify full stock GAPOWR repair RNG. Debug logging, sample/device output,
localized text and MessageList dispatch are presentation interfaces. Ambient
x87 FPCW0E7F is supplied by the reused fixture, not an active-game invariant.

Full House/Logic AI, interposed teams/activation work, production delivery,
loaded scenarios, SW lifecycle, Spy/ForceShield/capture producers and rendered
or audible output remain outside this corpus.
