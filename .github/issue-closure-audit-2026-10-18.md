# Open issue closure audit — 2026-10-18

Six open issues have merged fixes and focused regression evidence supporting
manual closure: **#1303, #1629, #1342, #1278, #704 and #1391**.
Please review the evidence below and close the corresponding reports as completed.
Two further reports have relevant code fixes but retain acceptance gaps; they are
listed separately so that a related merge is not mistaken for full resolution.

## Snapshot and method

- Repository: `YuriPlanet/vera20k`; source inspected at
  [`fc8997b827f5c1e978082d685f5f2c1f1af956c3`](https://github.com/YuriPlanet/vera20k/commit/fc8997b827f5c1e978082d685f5f2c1f1af956c3).
- Final API snapshot before this audit PR: **220 open issues**, **1,461 PRs**: 1,429 merged,
  29 closed without merging, and three open. Issue #1902 appeared during the
  scan and was included; it describes a remaining wall-autofill lifetime gap.
- Retrieved every open issue's title/body and every PR's title/body, state,
  merge commit and closing-issue references. Cross-referenced issue numbers,
  native addresses, function names and subject terms; these matches were leads,
  not proof of a fix. The three open PRs were excluded as delivered fixes.
- Read candidate issues, their comments and fixing PRs; followed the current
  implementation and regression tests. All eight PRs in the next two sections
  have merge commits that are ancestors of the inspected source.
- All six closure candidates were still open at the final snapshot and had
  empty `closedByPullRequestsReferences`. A missing GitHub association does not
  mean a fix is absent, and a PR mentioning an issue may instead record a residual.
- No gameplay changes or issue-state changes are part of this audit. It is a
  review of reported defects, not a new whole-engine/native-parity certification.
  For the other **214** open issues, this scan establishes no closure recommendation;
  that does not assert that every old reproduction still fails today.

## Recommended manual closures

### #1303 — Captured Tech Secret Labs do not grant production

[Issue #1303](https://github.com/YuriPlanet/vera20k/issues/1303) is addressed by
[PR #1802](https://github.com/YuriPlanet/vera20k/pull/1802), merged October 16 UTC.
The [assignment owner](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/secret_lab.rs#L49)
selects the laboratory reward; [CanBuild](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/production/can_build.rs#L105)
admits an owned laboratory's grant before normal technology/prerequisite gates,
while preserving build limits. Capture and loss use the existing House membership.

The eight `secret_lab` checks passed on the inspected source, including
`captured_secret_lab_grants_before_normal_tech_admission_and_retracts_on_loss`
and the paid-queue/snapshot lifecycle regression. The existing original-executable
oracle was also replayed in this session: its assignment, getter, admission and
reader outputs matched the retained corpus after distinguishing executable identity.
PR #1802 records the retail capture/paid-production witness and comparison bounds.

### #1629 — Cloning Vats do not copy barracks infantry

[Issue #1629](https://github.com/YuriPlanet/vera20k/issues/1629) is addressed by
[PR #1618](https://github.com/YuriPlanet/vera20k/pull/1618), merged October 15 UTC.
The ordinary [production exit calls the cloning loop](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/production/production_queue.rs#L574).
[That loop](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/production/production_queue.rs#L637)
constructs one infantry copy per owned vat and reuses its normal ExitObject path.
Five `cloning` checks passed, covering retail NACLON, successful copies, refused
exits, the source dock prerequisite and exclusion of vehicle output.

Native evidence is the instruction-level `0x4449D9..0x444A53` infantry exit loop;
this audit does not claim an executed whole native cloning lifecycle. PR #1630's
earlier reference to #1629 is a historical residual, superseded by the later merge
of #1618, rather than evidence that cloning is still absent.

### #1342 — Temporal gunner handover skips rearm/burst work

[Issue #1342](https://github.com/YuriPlanet/vera20k/issues/1342) is addressed by
[PR #1900](https://github.com/YuriPlanet/vera20k/pull/1900), merged October 18 UTC.
The shared [ReceiveGunner/RemoveGunner callbacks and rearm helper](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/passenger.rs#L1310)
transfer the Temporal link, advance the receiving burst, call the existing GetROF
owner with the appropriate weapon slot and restart the receiving timer.

The 14 `sim::passenger::gunner_tests::` checks passed, including native history
comparisons, departure retry and
`retail_unload_and_snapshot_resume_preserve_the_gunners_rearm`.
The merged PR records 12 native histories/45 callbacks and ordinary production
captures through departure and the next admitted shot. Its whole-world and
rendered-native comparison exclusions do not negate this bounded handover fix.

### #1278 — Crate pickup skips Tag event 49 and actor-death return

[Issue #1278](https://github.com/YuriPlanet/vera20k/issues/1278) is addressed by
[PR #1755](https://github.com/YuriPlanet/vera20k/pull/1755), merged October 16 UTC.
The [shared pickup owner](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/crates/pickup.rs#L371)
synchronously dispatches event 49, rereads the collector's lifetime and returns
before removal/reward when the callback kills it. Surviving pickups notify Logic
through the existing Scenario flag; owner and crate data are reread after callbacks.

All eight `sim::trigger_runtime::crate_tests::` checks passed. These include
[`crate_tag_can_kill_collector_before_removal_and_notification`](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/trigger_runtime_crate_tests.rs#L144),
the native reentrant pickup controls and the retail ALL07 save/notification
continuation. Native callback sinks and supplied scene priors are explicitly
bounded in #1755; this is not a claim that every crate outcome or ALL07 is complete.

### #704 — Object-type lookup policies disagree for late/unbound IDs

[Issue #704](https://github.com/YuriPlanet/vera20k/issues/704) is addressed by
[PR #1708](https://github.com/YuriPlanet/vera20k/pull/1708), merged October 15 UTC.
[`StringInterner::object_type`](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/intern.rs#L77)
owns the shared lookup contract. The [derived table](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/type_handle_table.rs#L67)
uses handles only for its bound RuleSet/covered IDs and otherwise uses the same
name semantics, fixing the old late-ID `None` and wrong-RuleSet policies.
The PR migrated 324 interned-name lookups. Launch and restore bind through
`Simulation::resolve_type_handles`; the cache is neither saved nor hashed.

All three `sim::type_handle_table::tests::` checks passed.
`answers_as_the_name_path` compares bound, unbound, late-ID and other-RuleSet
answers; the other tests cover case folding and unknown IDs. The name fallback
is now an explicit cache policy with consistent answers, not a second decision.

### #1391 — Two bunker source pins break the Python test gate

[Issue #1391](https://github.com/YuriPlanet/vera20k/issues/1391) is superseded by
[PR #1453](https://github.com/YuriPlanet/vera20k/pull/1453), merged October 12 UTC.
It removed `test_checkout_scan_proves_every_recorded_pin`, the source-pin inventory
and the two sidecars' `source_normalized_lf_sha256` fields. The
[shared publisher](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/tools/native_oracle.py#L521)
no longer pins repository sources. This retires the obsolete check by policy,
rather than refreshing the two hashes once.

The two replacement checks
[`test_a_source_edit_without_native_change_passes_and_pins_nothing` and
`test_a_legacy_source_pin_from_another_branch_passes_and_write_drops_it`](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/tools/test_native_oracle.py#L509)
passed. This resolves the specific two-bunker test failure, not all older custom
producers' pins: #1137 and #1372 remain separate open work.

## Code fixes present, but do not close without resolving the acceptance gap

| Issue | Merged work and current evidence | Why no closure recommendation yet |
|---|---|---|
| [#1045](https://github.com/YuriPlanet/vera20k/issues/1045) | [#1469](https://github.com/YuriPlanet/vera20k/pull/1469) adds live mission/NavCom admission, same-cell boarding and `retail_replacement_move_cancels_pending_vehicle_boarding`; that regression passed in the 14-test gunner run. | The [issue comments](https://github.com/YuriPlanet/vera20k/issues/1045#issuecomment-6094750285) additionally require the GI/open-topped BFRT arrival, position/range and same-frame effects against a joined native history. The bounded #1469 evidence does not establish all of that. Narrow the issue to the remaining acceptance work, or confirm it separately before closure. |
| [#797](https://github.com/YuriPlanet/vera20k/issues/797) | [#1423](https://github.com/YuriPlanet/vera20k/pull/1423), implementation commit `11e14202bf`, adds the missing [sound-release/active-bit clear](https://github.com/YuriPlanet/vera20k/blob/fc8997b827f5c1e978082d685f5f2c1f1af956c3/src/sim/anim_class.rs#L2639). Three animation trigger checks passed. | The retained joined test exercises NoEffects across Next, sharing the release branch. A separate sounding-to-no-StartSound/no-Report Next history requested by the issue's comment was not established in this audit. Its comment also bounds the defect to modded data, not stock ART. |

## Related merges that do not justify closing the whole issue

| Open issue(s) | Merged PR(s) | Remaining boundary |
|---|---|---|
| [#1350](https://github.com/YuriPlanet/vera20k/issues/1350) Scatter | [#1523](https://github.com/YuriPlanet/vera20k/pull/1523) | Ordinary grounded GI input is delivered; the PR explicitly keeps broader planning/transport/falling-route work open. |
| [#1347](https://github.com/YuriPlanet/vera20k/issues/1347) Cheer | [#1872](https://github.com/YuriPlanet/vera20k/pull/1872) | Ordinary Conscript input works; app outgoing-buffer saturation remains an explicit issue residual. |
| [#1273](https://github.com/YuriPlanet/vera20k/issues/1273) Planning Mode | [#1684](https://github.com/YuriPlanet/vera20k/pull/1684), [#1824](https://github.com/YuriPlanet/vera20k/pull/1824) | Input/routes/overlay and local-defeat exit are implemented; exact patrol-conversion scheduling was explicitly unverified in #1684. Review that criterion before treating the whole report as resolved. |
| [#1351](https://github.com/YuriPlanet/vera20k/issues/1351) ALL03 Nuke | [#1780](https://github.com/YuriPlanet/vera20k/pull/1780) | Preferred-cell and recharge state are implemented and native-compared; the issue also requests the connected ALL03 campaign encounter. #1780 supplies Event13/AI cadence and enemy acquisition as boundaries. |
| [#1308](https://github.com/YuriPlanet/vera20k/issues/1308), [#1309](https://github.com/YuriPlanet/vera20k/issues/1309), [#1340](https://github.com/YuriPlanet/vera20k/issues/1340), [#1344](https://github.com/YuriPlanet/vera20k/issues/1344) | [#1423](https://github.com/YuriPlanet/vera20k/pull/1423), [#1573](https://github.com/YuriPlanet/vera20k/pull/1573), [#1663](https://github.com/YuriPlanet/vera20k/pull/1663) | Major opening/progression/effect work is delivered. The individual issues request additional named native/retail histories; the PRs explicitly bound their scene/mission claims. Reconcile those criteria before bulk closure. |
| [#1508](https://github.com/YuriPlanet/vera20k/issues/1508) trigger catalog | Many trigger PRs | Explicit umbrella with remaining event/action chains, including work in open PRs; retain it. |
| [#1326](https://github.com/YuriPlanet/vera20k/issues/1326) Flak-team Scatter | [#1403](https://github.com/YuriPlanet/vera20k/pull/1403) | Script21 dispatch and retail-reader regression are present; executed native team history and the requested naturally selected AI encounter were not recorded by that PR. |
| [#1730](https://github.com/YuriPlanet/vera20k/issues/1730) building shadows | [#1859](https://github.com/YuriPlanet/vera20k/pull/1859) | Demonstrates CAHSE05 rubble's own-image shadow. The report does not identify an affected intact building; this merge cannot certify every reported building shadow. |
| [#1709](https://github.com/YuriPlanet/vera20k/issues/1709) primary grouping | [#1820](https://github.com/YuriPlanet/vera20k/pull/1820) | Manual stock factory selection works; the PR explicitly leaves raw non-Type Factory RTTIs open. |
| [#1098](https://github.com/YuriPlanet/vera20k/issues/1098) projectile Trailer | [#1258](https://github.com/YuriPlanet/vera20k/pull/1258), [#1686](https://github.com/YuriPlanet/vera20k/pull/1686) | SUB and authored Scalable work do not establish the requested stock NUKEPUFF route and all remaining identity admission. |
| [#1276](https://github.com/YuriPlanet/vera20k/issues/1276) Drone firing frames | [#1723](https://github.com/YuriPlanet/vera20k/pull/1723) | Victim rocking/ground pose is delivered; that PR explicitly excludes the Drone body firing frames. |
| [#626](https://github.com/YuriPlanet/vera20k/issues/626) combat context | [#1682](https://github.com/YuriPlanet/vera20k/pull/1682) | Fixes the scan/retaliation wall omissions; broader shared fact/subject ownership remains open. |
| [#614](https://github.com/YuriPlanet/vera20k/issues/614) radar updates | [#1762](https://github.com/YuriPlanet/vera20k/pull/1762) | Cell source ownership was fixed; the issue now explicitly tracks linear dirty-cell lookups. |
| [#1137](https://github.com/YuriPlanet/vera20k/issues/1137), [#1372](https://github.com/YuriPlanet/vera20k/issues/1372) | [#1453](https://github.com/YuriPlanet/vera20k/pull/1453) | Shared pins were removed, while older custom producers/packet seals remain. Do not conflate these with the narrower #1391. |
| [#1392](https://github.com/YuriPlanet/vera20k/issues/1392) profile IDs | [#1393](https://github.com/YuriPlanet/vera20k/pull/1393) | Names superweapons; explicitly leaves the three numeric production profiles as the follow-up. |
| [#1406](https://github.com/YuriPlanet/vera20k/issues/1406) archive schema | [#1407](https://github.com/YuriPlanet/vera20k/pull/1407) | Deletes unread packets, but retains the affected unit-move archive and explicitly records its broken recheck. |
| [#597](https://github.com/YuriPlanet/vera20k/issues/597), [#620](https://github.com/YuriPlanet/vera20k/issues/620), [#653](https://github.com/YuriPlanet/vera20k/issues/653), [#674](https://github.com/YuriPlanet/vera20k/issues/674), [#847](https://github.com/YuriPlanet/vera20k/issues/847) | Earlier refactors | Their updated bodies identify remaining Zoom, disabled-button, RMG arithmetic, passive-name and theater-table work; relevant earlier removals are insufficient to close them. |

## Validation and reproducibility

This session ran **41 focused Rust checks** on the inspected source and **two
Python checks**, all passing with no ignored tests in those selections. The Rust
selections were `secret_lab` (8), `cloning` (5),
`sim::type_handle_table::tests::` (3), `sim::passenger::gunner_tests::` (14),
`sim::trigger_runtime::crate_tests::` (8) and
`sim::anim_class::trigger_tests::` (3), with `VERA20K_REQUIRE_RETAIL_INI=1`.
The first two were the preceding investigations of #1303/#1629 in this same
session; the audit reused the core library test executable built at the pinned
source for its further selections. It did not compile another game or rerun the
full workspace suite. The corresponding source tests can be selected with
`python -m tools.cargo_run -- test -p vera20k-core --lib <filter>`.

The Python selection was:

```sh
python -m unittest \
  tools.test_native_oracle.IdentityAndReferenceTests.test_a_source_edit_without_native_change_passes_and_pins_nothing \
  tools.test_native_oracle.IdentityAndReferenceTests.test_a_legacy_source_pin_from_another_branch_passes_and_write_drops_it
```

For a fresh inventory, repeat these read-only queries; the date/counts above are a
snapshot, not a live dashboard. Increase the limits if the repository grows past them.

```sh
gh issue list --repo YuriPlanet/vera20k --state open --limit 1000 \
  --json number,title,body,updatedAt,url,closedByPullRequestsReferences
gh pr list --repo YuriPlanet/vera20k --state all --limit 2000 \
  --json number,title,body,state,url,mergedAt,mergeCommit,closingIssuesReferences
```

Before closing an individual report, check for newer reproductions or comments
and cite its existing fixing PR. This audit intentionally leaves issue closure to
the maintainer and does not install automatic closing references.
