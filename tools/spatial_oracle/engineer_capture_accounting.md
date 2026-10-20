# Engineer capture accounting comparison

This companion preserves the bounded accounting controls through the existing
[Engineer joined fixture](engineer_repair_joined.md). It imports
`EngineerJoinedFixture`, the existing native invocation owner and the shared
`native_oracle.finish_vectors` checker. It adds supplied boundary inputs and
read-only observation hooks; it contains no accounting or capture algorithm.
The primitive Engineer and Construction payloads, and both joined corpora,
are independent inputs and are not rewritten by this companion.

The active-retail executable is `gamemd.exe`, image base `00400000`.
The `.meta.json` records the Unicorn versions, native entries, supplied
boundaries, inherited fixture provenance and the canonical payload hash. The JSON records retail file hashes
and selected native reader outputs, not retail executable or asset bytes.

## Reproduction

Install the repository's native comparison dependencies and select the same
retail executable and extracted assets used by the Engineer joined owner:

```sh
export VERA20K_GAMEMD_EXE='/path/to/retail/gamemd.exe'
export VERA20K_ENGINEER_REPAIR_ASSETS='/path/to/engineer-repair/extract'
python -m tools.spatial_oracle.engineer_capture_accounting --check
```

The assets variable uses the same owner and fallback
`target/asset/engineer-repair/extract` as `engineer_repair_admission --joined`.
See that owner's document for extraction requirements and the selected rules,
mode, map, ART, sound and infantry assets. `--help` does not emulate or require
the executable. Default execution checks without writing; `--write` deliberately
regenerates only this companion and its provenance sidecar. `--output` changes
only the reference destination through the shared checker.

## Executed controls

The schema version is `1`, kind
`bounded-original-engineer-capture-accounting`. The three control lists are:

| List | Controls | Entry and retained result |
|---|---|---|
| `record_kill_controls` | Live NULL, repeated live NULL, DontScore, Insignificant, supplied sale suppression, initial loss `FFFFFFFF` | Whole `702D40` and original CostOf; old-owner loss/kill table, score, notification, owner and positive Health after every actual callback |
| `capture_controls` | Stock, DontScore, Insignificant, supplied maximum loss/kill counters and signed score | Original resolved command/event/Commence and whole Walk `75AEC0`/PerCell `519630`/Building ChangeOwner `448260`/Techno ChangeOwner `7014A0`; old/new accounting state before and after the externally supplied ownership race |
| `built_controls` | Initial item/total `0` and `FFFFFFFF` | Whole House Record_Last_Built `4FB6B0` and existing-capacity Counter `49FA00`; item `49FA47` and total `49FA50` after the callback |

Every row records complete Main, Scenario and MapGen states before and after
the tested calls, native requested draws and underlying raw advances, the
existing owner's call/lifecycle trace, observed accounting stores, original
instruction bytes at the visited accounting sites, instruction count and
ordered/unique PC hashes. The existing fixture checks every executable section
against the original PE file before execution; the companion requires those
sections unchanged after each control and no unfinished RNG call. Runtime
`AC13C8`/`AC13BC` values are observed after the inherited original Object CRT
startup. They are not supplied accounting answers.

## Native behavior and Rust connection

ChangeOwner calls NULL RecordTheKill at `7015A8` while the old owner is still
installed, after old live-quantity removal. RecordTheKill's DontScore gate
`702E54` precedes accounting mutations. Its Building Insignificant gate
`703045` suppresses its loss/kill branch. There is no first-credit gate:
repeated actual callbacks book immediately. The supplied sale control sets
Building `+53C=-1`, the state native sale stores at `44A1EF` before its NULL
callback `44A1F9`; that datum suppresses the loss increment, not a generic
Selling mission check.

ChangeOwner separately prices the victim for the old House, unconditionally
adds the price to the new House at `7015D0/7015D2`, then removes/adds tracking
at `7015DE/7015E6`. Its separate new-owner kill increment `70164D` is gated
by DontScore, without the RecordTheKill Insignificant gate. The owner pointer
store follows at `701735`. The executed capture controls retain this distinction
and the native wrapping outputs. Capture arrival's old-House `+244=1` write
at `519F4F` is observed. Constructor `4F577D` clears that retained notification;
TEvent `71F0E1`, mapped to event kind3 by `71F328/71F350`, is its known gameplay
reader. Live Tag/TEvent polling is outside these NULL-Tag controls.

The production accounting owner is `combat::record_kill::record_the_kill` with
the private `MatchStatistics` mutation APIs. Tests in
`src/sim/house_statistics_tests.rs` consume this companion directly:

- `native_null_record_kill_books_each_live_callback_immediately` compares five
  NULL controls against the real shared callback before owner change, checks
  retained positive Health and no Rust/native draws. It explicitly excludes the
  sale producer control, whose required `+53C` state is not represented yet.
- `native_change_owner_score_and_kill_counter_wrap` consumes the real capture's
  observed price and before/after table/score projection through the shared
  mutation APIs. It does not implement a second capture path in the test.
- `native_record_last_built_counter_wrap` compares both native Counter total
  outputs through `MatchStatistics::record_built`; it also checks native item
  and total agreement and no native draws.
- `house_capture_notification_is_retained_and_hashed` consumes the stock
  capture notification before/after, then exercises retained House serialization
  and hashing. Those Rust checks do not claim a live native Tag receiver.

Actual Cargo results belong to the chain owner's validation record. A checked
native companion or a total-only API test does not certify the full production
arrival, House loop or object lifecycle.

## Supplied state and limits

The inherited fixture executes native retail readers and original Object CRT
startup, while supplying an already admitted ordinary untagged GAPOWR with
actual Health374, estimated611 and sampled374, flat physical cells, native
vtables, registered UID and House/vector prior. Original final-head Walk and
PerCell execute; path search/payment and travel to that head are supplied.
Capture commands are delivered before an external ownership race. Flags and
maximum counters are explicit controls, not constructor-default or ordinary
stock claims. The old House's ArrayIndex1 is supplied prior; original ChangeOwner
chooses its kill slot. GAPOWR ArrayIndex0 comes from the original type-registration
tail in the existing fixture.

The valid capacity-one built Counter is supplied. Allocation/growth, House
`+246` trigger consumers and full PLACE/production admission are not certified.
The inherited platform/allocator/presentation boundaries remain explicit in
the sidecar. No native accounting, CostOf, capture, counter, RNG or lifecycle
decision is answered by this companion. Device progression, whole House/Logic
scheduling, complete lethal/sale/live Tag/TEvent behavior and radar redraw remain
outside this comparison.

The JSON retains raw per-victim-house kill tables. VERA's existing statistics
model projects these to totals; native per-type accounting, score-screen
aggregation, the harvested/object score split at arbitrary overflow and native
House checksum history `+548C` are separate residuals. `+548C` has native writers
and House ComputeCRC `50300D`, with no gameplay reader found in the prior
instruction census; this companion makes no native checksum equivalence claim.
