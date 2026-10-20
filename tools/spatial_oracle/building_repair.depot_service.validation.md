# Repair-depot service validation

An ordinary damaged HTNK sent to stock NADEPT uses the Building Repair mission
and Techno radio repair receiver. It retains its contact without funds, pays the
shared repair step cost, heals on the native cadence and leaves through the
Unit destination owner. The selected route is Russia/Battle/AnyTown;
MTNK/GADEPT provide additional native controls.

## Ownership and migration

The Building mission owns status, dispatch timer and private independent repair
progress. GameEntity owns private pending entry; radio contacts remain the slot
authority. Techno radio1C uses RuleSet type cost, `production::repair_step_cost`
and the existing House wallet owner. Estimated health, parasite removal,
EVA/radar, destination/power and command cleanup use their existing owners.
Building Art owns animation slots and their derived reverse index; Anim owns
the independent slot marker at +0x118 and normal completion at +0x179.

The Unit depot FSM, service timer, 30-tick unfunded grace eject, separate cost
formula, direct wallet debit and late global service sweep are removed.
Snapshot version 286 and rules-domain version 13 reject the superseded state.
This resolves [#675](https://github.com/YuriPlanet/vera20k/issues/675) and the
depot writer in [#661](https://github.com/YuriPlanet/vera20k/issues/661).

## Original executable evidence

[Depot comparisons](building_repair.depot_service.md) retain the original
readers, constructor defaults, layer order, input identities, bodies, callers,
numeric results, timers and all 250 Scenario RNG words.
[Expiry comparisons](building_slot_replacement.expiry.md) retain 64 expiry,
2 readiness and 5 observer controls using six physical retail SHPs and 16
AnimTypes. Both use the original executable,
Unicorn 2.1.4 and ambient x87 PC53/chop 0x0E7F. Original text and the actual
vtables remain unchanged. Native output supplies the goldens.

Depot arithmetic, radio replies, payment/heal, destination power, Guard/Repair
cadence, clear-map exit search, rally and contact cleanup execute within their
declared boundaries. The unfunded history retains 14,201 frames without payment,
heal or ejection. Lifecycle 30 rows establish pending-entry and radio ordering;
Rust consumes 28 ordinary seams. Two post-admission Deploy slices remain
native-only because stock HTNK cannot enter that command.

Twenty-two additive fallback controls execute both Repair power-recovery
branches and the no-contact readiness boundary. State0 admission failure calls
PowerOn; state1 NEED_MOVE rejection restores an unpowered contact. State0
without contact preserves the existing readiness byte before Queue(Guard),
while state1 writes ready1. Eighteen linked and four no-contact rows retain
complete state, actual calls/writes and all Scenario RNG words. Removing this
field reproduces the previous arrival-bearing payload byte-for-byte.

Seven arrival controls include four complete original terminal tails through
PerCell/DOCK_NOW/PowerOff/StopMoving/Foot gate/Unit idle and return, one prepared
post-Stop no-contact gate and two idle-argument controls. OriginalCursor executes
supplied budget 8 to terminal budget 1. Rust starts before payment and uses 8.
Before, after PerCell and final state are compared, including full RNG.
The armed stock arrival overwrites queued Sleep 0 with Guard 5; a retained target
preserves Sleep. A proposed Sleep-preservation change was rejected on this
executed evidence. Constructor TubeIndex=-1 makes the second idle flag inert
on this route.

Expiry distinguishes normal completion from cancellation, establishes successor
names/timers, scalar preclear and both detach visits before Display/Logic removal.
The existing decoder now reads inactive at +0x19B; +0x198 is independent sound
suppression. Historical payloads and sidecars are preserved. Depot, expiry and
both legacy corpus checks passed; their commands and limits are in the reports.

## Rust and retail validation

With retail `ini/` and `VERA20K_REQUIRE_RETAIL_INI=1`, the pre-review candidate
passed 9554 library tests, failed 0 and ignored 227 in 93.05s. Clippy exited 0 in
17.32s, with 725 warnings. All Cargo runs used `tools.cargo_run`; tests used
`--lib`. The focused arrival comparison passed all 7 controls. The full suite
also includes connected radio/service histories, animation expiry, command
cleanup, snapshot and production-reader regressions.

The sole fresh read-only critic found incorrect fallback power recovery and
premature readiness in state0 without a contact. Both new native-output
regressions failed on the unchanged implementation. The existing Repair owner
was corrected without adding state or another port. After these fixes, all 13
native service tests, 17 connected depot tests and the one ungated global replay
test passed. Those focused checks validate the later fixes in the already
tested modules. There was no second critic pass.

The normal ungated
`global_skirmish_replay_is_deterministic_and_baseline_stable` assertion passed
with pin `0xCDA1_0AEE_6EA0_AE46`.
The integrated replay package (`tools/spatial_oracle/depot_service_replay/main1018/README.md`, in git at 1b1a8b43e)
attributes the new Building progress hash over 601 complete recorded boundaries,
including three complete RNG streams and their exact caller order. Its only
projection is `tick_result.state_hash`; control asserts the incoming pin and
current was a self-measuring diagnostic. Its receipt checker passed. This is
Rust replay evidence, separate from the final ungated assertion and native
goldens. Both replay packages are in git at 1b1a8b43e.

The map-observer Python suite passed 80 tests in normal mode and 80 with `-O`.
The field ratchet passed 2537 versus 2538 at the merge base. `git diff --check`
passed.

The final release label is `depot-repair-service-reviewed-20261003`, executable
SHA-256 `0d08bda55cc1a6226b8b2ea89fb46c2952a6feb52049f8c9d52b5adb0f02ad73`,
source SHA-256 `7dea934557ef804d7fe4204b70d8cbf9cef15780db543c892fe6f4b5a9ac623b`.
It was built at dirty base `f1783f83d` after the review fixes. Earlier release
identities and validation are not substituted for this
candidate's focused checks or retail repeat.
The final [retail profile](../map_observation.depot-repair.example.json) uses
ordinary gameplay commands and the normal loader/exact-step Metal capture.
All four reviewed-release captures and offline validators passed. The tank
held at HP40 and cash0 for 607 frames, then paid 45 steps of 8 HP/2 credits
(total 90), reached HP400 and cleared both contacts at frame6856. It physically
departed at 6869 and reached the exit cell at 6899; final cash160, spent5990
and harvested0. All complete projected observations equal the prior release.
The corrected alternate
power/no-contact fallback branches have separate native/Rust controls; this
success scenario does not project them. The final Metal readback was inspected;
it is byte-identical to the earlier frame and carries no native pixel claim.

## Coverage and residuals

Native component/terminal execution and Rust retail integration establish their
stated coverage. They do not compare an entire native match, real native disk
serialization, native pixels, arbitrary mods or all scheduler/path branches.
Snapshot continuation uses the existing Rust seed 0 restore policy against an
explicitly supplied original RNG boundary. Expiry rows stop at deferred UnInit;
general storage retirement is outside those native rows.

Service bodies do not draw Scenario RNG in the selected controls; later Guard
draws remain in the histories. Mission timer writes and private progress are
compared; body/readiness is a separately executed prerequisite. Both animation
detach visits are represented. The production observer does not project direct
radio packets, locomotor power, pending entry or auxiliary navigation; their
native comparisons are separate evidence.

[#937](https://github.com/YuriPlanet/vera20k/issues/937) covers occupied-depot
foundation waiter/parking and its later admission risk. Hospital, airfield and
bunker service, retained saved-order/tube work and blocked exits are outside this
selected chain. Guard/miner order paths that can Restore cancelled suspended
state are recorded separately in
[#1023](https://github.com/YuriPlanet/vera20k/issues/1023). Their shared
pre-Queue radio/pending-entry cleanup is covered here; their broader post-Queue
migration is separate.
