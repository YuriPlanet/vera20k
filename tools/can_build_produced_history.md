# Produced history and negative build limits

The existing `can_build_oracle.py` owner has an opt-in profile that executes
whole original `Record_Last_Built4FB6B0`, then original `CanBuild4F7870` and
`CheckBuildLimit50B370` on the same retained native Counter buffers. It is
consumed by the Rust production/history tests. The adjacent sidecar pins the
active-retail original image and declares supplied state and platform seams.

```sh
python -m tools.can_build_oracle --produced-history --check
# Explicitly regenerate original-executable observations for review:
python -m tools.can_build_oracle --produced-history --write
```

This selector leaves the default, Secret Lab and construction-option payloads
unchanged. It reuses `BuildEmu`/`Emu`, their loader, invocation, lists and
allocator. It does not introduce a VM, counter algorithm or admission policy.

The 14 histories contain 45 phases. Eight cover all four families with and
without `DontScore`, through successive native records under limit `-2`.
The other histories cover an Infantry `-1` limit, all-family zero/INT_MIN
limits after nonzero production, the selected Factory's held/FIFO contribution,
signed produced-plus-held overflow, later Type index growth, and separate Houses.
Limit/owned/factory changes are explicit component inputs. They are not native
execution of an INI pass, constructor, acquisition, queue admission, paid
delivery or whole match.

Whole native Record updates last-built slots before its DontScore test. A
scored record increments the matching produced item and family total; an
unscored record retains those counts. The comparison observes these actual
memory values and the subsequent native answers. It never derives a golden
from the number of records, a current-owned count or the Rust result.

The negative-1 history is a component control for the ordinary paid
map-authored E2 chain. Its supplied Type index10 does not claim
to be stock E2's registration index. Untouched stock Battle defaults remain
distinct from a map-authored negative limit.

The held/FIFO history supplies one delivered Infantry record under limit
`-3`, then a Factory holding the queried type and one queued copy. Native
CanBuild and CheckBuildLimit have separate decisions here: the former asks
produced history while the latter also counts the selected Factory. The next
phase replaces only the held type, preserving the queued queried copy. These
are declared query-composition inputs, not a full production scheduler.

The signed-sum history declares `produced_prior=2147483647`, preparing only
the selected Infantry item and family total before measurement. It then
supplies one matching held product under limit `-2`, followed by one original
Record. This isolates the signed overflow in original CheckBuildLimit's
produced-plus-CountTotal addition and the subsequent original item/total
increments without executing billions of records. The supplied prior is an
input; every query answer and post-Record count comes from native execution.

The late-index history retains the first item at10, then invokes native
Record for a supplied Type at65 with original Counter capacity64. Actual
resize `477C70` runs before any query of that new index, copies the old
storage and zeroes its added slots through the Counter caller. The observed
item64 is zero and the old item10 survives. This tests storage-independent
history results; Rust need not adopt native allocation capacity or growth.

The profile initializes Counters through original `49F9B0`, then supplies
valid external capacity64 arrays. It installs original Counter vtable
`7E5C54`, whose resize slot is `477C70`. Unexpected growth in the other
histories fails the run. Type and product layouts use these original vtables:

| Family | Type vtable | Product vtable | Product Type field |
| --- | --- | --- | --- |
| Building | 7E4570 | 7E3EBC | +520 |
| Unit | 7F6218 | 7F5C70 | +6C4 |
| Infantry | 7EB610 | 7EB058 | +6C0 |
| Aircraft | 7E2868 | 7E22A4 | +6C4 |

Original Type RTTI/ArrayIndex and product WhatAmI/GetTechnoType/getter bodies
execute. The older fixture's supplied RTTI/index/type hooks remain installed
but are asserted unreached. Valid supplied nonnegative Type indices are
explicit layout priors, not a claim of executing full native registration.
The original image remains unchanged. Allocation succeeds through the
existing platform transport; native allocation failure is outside coverage.

The JSON schema is `cases[].input` followed by `phases[].input/expected`.
Family names are the existing tool's `building/unit/infantry/aircraft`; Unit
corresponds to Rust's Vehicle category. Inputs specify the case's initial
family, index, signed limit, DontScore and any declared extra scope. Phase
operations are:

- `observe`: query retained native history without recording a product.
- `record`: invoke the whole native writer for the specified/default family,
  index and House.
- `set_limit`: supply the next signed Type limit input for all selected types.
- `factory`: supply the selected Factory's held index, FIFO and owned-count
  input, including its removal.

Expected values expose four-family `produced_items`, `produced_totals` and
`last_built`. Items/totals are unsigned dword bits; limits, indices and native
CanBuild answers are signed. The ownership history additionally exposes
`other_house` with the same fields. Each `queries` row gives the owner/family/
index/limit, three explicit CanBuild argument pairs with actual EAX answers,
and the CheckBuildLimit AL boolean. Factory backing slots/capacity and
uninitialized object payload bytes are not golden gameplay fields.

Existing active-retail `engineer_capture_accounting` zero/wrap controls pin
the Counter increment's UINT_MAX-to-zero wrapping bits and remain unchanged.
The additional signed-sum control above connects overflow to both limit readers.
Existing `naval_house_stats` controls cover native Counter streams. This
profile does not add a stream fixture. It does not execute native SAV,
starting-force reset or whole House initialization. The represented fresh
startup history is zero except for the declared boundary prior; no separate
runtime reset API is implied by this corpus.

Creation cues and all class fallbacks are explicitly `-1`; audio is not
executed. Whole Record still runs its last-type and tech/246 writes, but the
separate246 timer/raw-save difference is not reproduced by these controls.
All three native RNG buffers are seeded31 before measurement and checked
unchanged; selected RNG and detach entries are asserted unreached. Counter
allocation/free is platform storage work. These bounds do not certify paid
construction/exit, gameplay audio, arbitrary callback mutation, rendering,
native whole-world persistence or complete function/class equivalence.
