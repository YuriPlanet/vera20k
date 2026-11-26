# V49: Combat doctrine and regression repair

This candidate starts at the exact delivered V48 source, not the user's
unprovided local V48 checkout. No engine, rule, visibility or difficulty
changes. V50's unified spending plan and V51's observed exchange learning are
not included. All tactical thresholds below are provisional.

## Operation lifecycle

- Allocate scouts and necessary garrison first; optional raiding now draws
  only on surplus after reserving ten main-available actors. Keep up to four
  raiders and release optional raid allocation during a base raid. A small
  fist may take an actual in-range local mobile contact, not a remote base.
- A heavy live fixed battery (range >=7 or mind control) in the current
  front is an explicit barrier. Existing safe in-range artillery can fire;
  a verified native destination in a safe firing annulus is evidence of an
  ongoing approach, not evidence of arrival. A proposed Move or an available
  factory recipe is not an operating battery. Observe the actual tower HP;
  use the existing 1080-tick no-progress review and alternate-target ranking.
  A deferred gunline may be reconsidered for a legal armoured operation.
  A light turret still uses normal battle logic, with extra short infantry
  protection when the turret has a substantial range advantage.
- `advance/assault.rs` forecasts a concrete local cohort (4--24 healthy,
  >=75%-HP ground combat vehicles of nominal strength >=300, in a compact
  formation; exclude harvesters, transports, irreplaceable/suicide actors
  and native services). Ordinary Grizzly, Rhino and Lasher-size vehicles
  are eligible, no 600-HP single-vehicle gate or blanket standoff-purchase
  veto. The 300 threshold classifies eligible line vehicles, not group
  superiority. Use native attack admission, approachability, actual weapon
  damage, armour multiplier, burst, ROF, range, speed and current HP.
- Use 65% of friendly nominal sustained fire, full hostile DPS plus initial
  volley, splash and overlap allowances, travel and turning slack. Reject
  exposed susceptible vehicles under observed mind control and unsupported
  hostile effects in the evaluated path. Up to 32 relevant hostile fire
  sources; more or unsupported nonlinear weapons => no optimistic forecast.
  Maximum estimated engagement 540 ticks; estimated HP attrition <=38%;
  estimated replacement-equivalent loss <= twice target price plus 10% of
  cohort purchase value. During stockpile, an already local breach must
  meet tighter launch conditions (<=20% forecast loss, approach <=150 ticks).
  This is a credit-equivalent heuristic, not an actual casualty or cost
  prediction. No claim to simulate overkill, veterancy, projectile flight,
  target selection, charge-up, prism support, or actual movement routes.
- Keep the exact approved cohort together. Tactical/ordinary competing
  orders may not replace its primary attack; preserve real casualties,
  native service, manual control, and factory handoff protection. Review
  every 15 ticks (also check critical interrupts every frame). End at
  <4 remaining eligible members, <62% starting HP, unsafe changed forecast,
  150 ticks without target HP progress after the predicted approach window,
  strategic interrupt, target disappearance/offline/invulnerability, or
  the 240--660 tick bounded operation window. Visibility loss is NOT a kill.
  Clear old primary goals and veto stale attack continuation. Retry a failed
  target after 900 ticks or 25% more local cohort HP, still subject to a
  fresh forecast. Never authorize attacks purely because a timer expired.
- Keep ordinary short-range infantry outside the heavy gunline while the
  column works; preserve genuinely safe standoff fire and immediate local
  mobile-target firing. Actual detours keep safety memory; generic siege
  staging no longer grants a raider lease. Final ordinary front admission
  is not a new recovery state, and does not permanently hold a tank. Existing
  specialist, engineer, MCV, casualty and factory checks remain authoritative.
- The existing air, blackout and other target paths remain; this is not a
  unified multi-domain simulator or a proof of eventual breach. In particular,
  a fully blocked and unsafe front may still contain rather than attack.

## Immediate economic regression stopgap

Move discretionary local counter requests after existing research/heavy
savings and infantry limits. Do not use this exception during mobilization,
rest, or urgent specialist work. The fighter emergency shortcut is reserved
for current base threats. Local counter and siege inventory count surviving
owned eligible stock rather than only currently available/healthy troops;
manual-kept units are excluded. Native busy-line checks remain. This does
not yet account for all unpaid commitments, common reserves, repair costs,
all production paths or investment opportunity cost: those are V50 work.
The economy change is deliberately small and isolated in composition.rs
and siege_counter.rs (counter_held inventory only).

## Regression evidence and diagnostics

The historical development set is still 120 games nested in the fixed
1102-game V41 benchmark; original manifest bytes are unchanged. The V48
uploaded run contains 30 won, 65 lost, 25 unresolved vs V41 30/60/30 on this
set. Do not mix this deliberately balanced set with the full-set win rate.
Use V48 seeds 20004, 20069, 20145, 20222, 20638, 20988 as additional
inspection targets, not as hard-coded strategy cases. `v48-reference-summary.json`
contains a read-only reparse of the uploaded V48 receipts and logs.

New diagnostic fields: assault commits/ends and loss/progress interrupts,
front guard reports (including infantry), and readiness samples with at
least six available fighters but no available Attack member. Reports are
not accepted orders or actual kills. Old logs lack assault events; report
missing rather than zero. Compare rates as well as raw event counts because
match length, logging, and the number of fights differ.

## A/B usage (only when convenient; no obligation to run every version)

From a fresh V49 candidate checkout:

    python -m tools.group_tests check
    python -m unittest tools.tests.test_autoplay_benchmark tools.tests.test_autoplay_check -v
    python -m tools.cargo_run -- build --profile fast --bin autoplay-match
    python -m tools.cargo_run -- test -p vera20k-core --lib sim::strategy::assist

Use a separate V48 binary built from the same engine/source settings and
explicit candidate runner for both versions. The existing V48 uploaded
receipt runner hash predates the new diagnostic parser, so use its reference
summary as historical evidence; do not edit its run metadata or bypass
identity checks to claim controlled attribution.

    python -m tools.autoplay_benchmark run --suite all --stage smoke --run-dir target/autoplay-results/v48-control-v49-runner --label V48-control --binary "V48 binary" --ra2-dir "YR directory" --source-dir "V48 checkout" --jobs 16
    python -m tools.autoplay_benchmark run --suite all --stage smoke --run-dir target/autoplay-results/v49 --label V49 --binary "V49 binary" --ra2-dir "YR directory" --jobs 16
    python -m tools.autoplay_benchmark compare --suite smoke --before target/autoplay-results/v48-control-v49-runner --run-dir target/autoplay-results/v49 --output target/autoplay-results/v49/smoke-v48.csv

The source path is optional for the candidate and essential for the separate
control. Keep hardware, assets, jobs and game options constant. Both can be
extended to `--stage dev` using the same respective binaries and directories.
A historical V41 comparison omits `--before`. Keep original V48 binaries
and receipts. No need to rerun the 982 remaining V48 games. Later V50 can
be compared independently to V49 and the combined candidate to V48.

Suggested focused human checks: ordinary tanks vs single and overlapping
Prism/Tesla/Grand Cannon/Psychic Tower; immobilised artillery and terrain
blocking; infantry pursuing tower ahead of tanks; genuine raider detour;
new mobile threat at the ore field; loss interrupts during a committed wave;
repair/reload and native factory exit. Confirm actual damage and deaths,
not just fewer safety reports. No new strategy phase is declared passed
until real Rust tests and game tests are executed.
