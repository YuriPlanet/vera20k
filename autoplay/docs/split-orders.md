# Splitting `assist::orders`

A mechanical split of `src/sim/strategy/assist/orders.rs` (4,252 lines, 49
top-level functions) into a parent module and two children: `common` for the
command-admission layer, `tests` for the test module it already held inline.

**Status:** applied. `orders.rs` is 2,399 lines, `orders/common.rs` 239,
`orders/tests.rs` 1,640. Section 7's question was answered before the move:
`Push` owns the army's target and gate, `issue` keeps admission, so all eight
functions moved.
**Precondition (met):** the `Push` design (army commitment) decides whether
this split is eight functions or four — see section 7.

---

## 1. Why, and why only this much

The file is under the repository's 5,000-line guard, so nothing is *required*.
What is wrong is that it holds two different kinds of contract:

- `issue` / `admits` / `reachable` answer **"may this command be sent, and
  how"** — `admits` alone has nine call sites across the assist layer, so
  changing it is a global audit.
- The other forty-one functions answer **"what should this unit do"** — leaf
  decisions, changing one affects itself.

They live together. A reader opening the file cannot tell which layer they are
in, and a constant like `PUSH_LOCK_FRAMES` was defined with a two-minute doc
comment and then never referenced because nobody could see where the army
decision belonged.

Readability comes from *each decision having one place that answers it*, not
from files being short. This split therefore does **not** try to make the file
small. It draws one seam — infrastructure versus decision — and moves the eight
functions on the infrastructure side into their own module.

### Rejected alternative

An eight-way split by topic (`capture` / `transport` / `supers` / `hold` /
`economy` / `errands` / ...) was considered and **rejected**:

- `errands.rs` was "everything left over" — a junk drawer with a better name.
- Eight files for one decision chain means more jumps than one file, not fewer.
- It is a mechanical size reduction dressed as architecture. Upstream's
  `820c82e49` was explicitly that and nothing more — "code moves verbatim".

Do not re-propose it without new evidence.

---

## 2. What is in scope

| In | Out |
|---|---|
| Moving eight functions into `orders/common.rs` | The `Push` behaviour fix (separate commit) |
| A `pub(super)` facade in `orders.rs` | Any change to a call site outside `orders` |
| Moving the test module to `orders/tests.rs` | Splitting tests by subject |
| Rewriting imports per child | Any logic edit, however small |
| One architecture-guard pin | The seven-way split |

**Zero call-site churn is a hard requirement.** `mod.rs` and the eight files
that call `orders::issue` / `orders::admits` must compile unchanged.

---

## 3. Target tree

Line numbers are from the commit this plan was written against and are for
orientation only. **Function names are authoritative** — line numbers drift as
functions move.

### `orders/common.rs` — the eight infrastructure functions

| Function | Line | Lines |
|---|---:|---:|
| `ordered_this_frame` | 1041 | 34 |
| `already_attacking` | 1075 | 13 |
| `admits` | 1088 | 18 |
| `issue` | 1106 | 103 |
| `reachable` | 1209 | 18 |
| `goal_cell` | 1227 | 22 |
| `on_map` | 1249 | 5 |
| `cell_in_bounds` | 1254 | 5 |

### `orders.rs` (parent) — everything else

| Function | Line | Lines | Note |
|---|---:|---:|---|
| `captures` | 109 | 271 | |
| `suicide_errand` | 380 | 138 | no caller outside |
| `pack_captured_yard` | 518 | 68 | |
| `bridges` | 586 | 80 | |
| `garrison` | 666 | 93 | |
| `infiltration_value` | 759 | 36 | |
| `home_cells` | 795 | 6 | **shared** — bridges + garrison |
| `walks_into_buildings_flags` | 801 | 6 | |
| `within` | 807 | 46 | **shared** — bridges + garrison |
| `path_crosses` | 853 | 39 | also called by `advance.rs` |
| `engineer_preference` | 892 | 79 | |
| `capture_value` | 971 | 39 | |
| `built_capture_value` | 1010 | 21 | |
| `nearest_site` | 1031 | 10 | |
| `forces` | 1259 | 110 | |
| `deploy` | 1369 | 71 | |
| `pose_to_take` | 1440 | 12 | |
| `nearest_distance` | 1452 | 22 | |
| `holds_or_fights` | 1474 | 14 | **shared** — bridges, garrison, forces, crates, tests |
| `harvesters` | 1488 | 114 | |
| `sell_hard_to_hold` | 1602 | 117 | |
| `mind_controls` | 1719 | 14 | no caller outside |
| `spy_class_code` | 1733 | 13 | |
| `spy_class_open` | 1746 | 12 | |
| `spy_route_blocked` | 1758 | 36 | |
| `crates` | 1794 | 153 | |
| `repairs` | 1947 | 64 | |
| `walks_into_buildings` | 2011 | 20 | |
| `retreat_line` | 2031 | 47 | |
| `managed` | 2078 | 143 | |
| `carrying_step` | 2221 | 16 | no caller outside |
| `transports` | 2237 | 55 | |
| `wants_board` | 2292 | 13 | no caller outside |
| `board_idle` | 2305 | 76 | |
| `delivery_cell` | 2381 | 46 | |
| `supers` | 2428 | 72 | |
| `aim_of` | 2500 | 11 | |
| `landing_cell` | 2511 | 27 | |
| `spent_on_the_enemy` | 2538 | 44 | |
| `enemy_structure_cell` | 2582 | 24 | |
| `is_combat_unit` | 2606 | 3 | called by `composition.rs` |

**The parent keeps the shared vocabulary** — `home_cells`, `within`,
`holds_or_fights`, `is_combat_unit` — for the same reason upstream's
`world_commands.rs` keeps `CommandSubject`: both children and outside callers
reach them through the parent, and none of them is a decision on its own.

### `orders/tests.rs`

The current inline module (line 2610 to end, ~1,650 lines, 39 test functions)
moves verbatim. One file, not split by subject:

- It is under the 5,000-line guard.
- Splitting it would mean a group tag per file, and the group compiles them all
  together anyway — no build-time win.
- It has two shared fixtures, `snapshot` and `arena`, which a split would
  have to duplicate or hoist.

Tests reference `walks_into_buildings` and `holds_or_fights`; both stay
reachable from `super::`.

---

## 4. Visibility and the facade

The moved functions keep their visibility, which is `pub(super)` today. Inside
`orders/common.rs` that would mean "visible in `orders` and its descendants" —
too narrow, because **sibling** modules call them (`admits` alone has nine call
sites in `advance`, `avoidance`, `dodge`, `expand`, `kite`, `micro`,
`mod` and `walls`). As applied, the six public ones are
`pub(in crate::sim::strategy::assist)` — the same reach `pub(super)` gave them
from inside `orders` — and `goal_cell`/`cell_in_bounds` stay private.

The parent re-exports what other modules call, so no call site changes:

| Re-export | Called from |
|---|---|
| `captures`, `pack_captured_yard`, `bridges`, `garrison`, `forces`, `deploy`, `harvesters`, `sell_hard_to_hold`, `crates`, `repairs`, `managed`, `transports`, `supers` | `mod.rs` |
| `path_crosses` | `advance.rs` |
| `ordered_this_frame` | `avoidance.rs`, `dodge.rs`, `kite.rs`, `micro.rs` |
| `already_attacking` | `micro.rs` |
| `admits` | `advance.rs`, `avoidance.rs`, `dodge.rs`, `expand.rs`, `kite.rs`, `micro.rs`, `mod.rs`, `tracked.rs`, `walls.rs` |
| `issue` | `advance.rs`, `avoidance.rs`, `dodge.rs`, `expand.rs`, `kite.rs`, `micro.rs`, `walls.rs` |
| `on_map` | `dodge.rs` |
| `is_combat_unit` | `composition.rs` |

Nothing else is re-exported. `suicide_errand`, `mind_controls`,
`carrying_step` and `wants_board` are `pub(super)` today but have no caller
outside `orders`; they stay non-re-exported and may be narrowed to private
once the move is verified, in the same commit.

### Parent skeleton

​```rust
//! The orders a player's hands would give one object.
//!
//! Parts: `common` — command admission and issuing, the layer every other
//! assist module calls. The rest of this file is the decisions themselves:
//! capture, garrisons, the holding posture, the economy, supers, transports,
//! and the one-off errands.
//!
//! Dependency rules: same as assist/.

mod common;
#[cfg(all(test, tests_sim_p_s))]
mod tests;

/// The admission half, re-exported so every caller keeps the path it had.
pub(in crate::sim::strategy::assist) use common::{
    admits, already_attacking, issue, on_map, ordered_this_frame, reachable,
};

// ... the forty-one decision functions, verbatim ...
// ... then the shared vocabulary: home_cells, within, holds_or_fights, is_combat_unit ...
​```

---

## 5. Execution

One commit. Steps are ordered so the tree compiles after each one; do not batch
them into a single unverified write.

| # | Action | Check |
|---|---|---|
| 0 | `git status` clean; record `git rev-parse HEAD` | — |
| 1 | Create `src/sim/strategy/assist/orders/` | — |
| 2 | Move the eight functions of section 3 into `orders/common.rs`, **verbatim**, with their doc comments. Write imports for what that file actually uses — do not paste the parent's import block | `cargo build` fails on unresolved names only |
| 3 | Delete the eight from `orders.rs`; add `mod common;` and the `pub(super) use common::{...}` facade | `build` |
| 4 | Move the test module to `orders/tests.rs`; replace the inline block with `#[cfg(all(test, tests_sim_p_s))] mod tests;`; dedent the module body one level and drop `mod tests {` | `build --tests` |
| 5 | Narrow `mind_controls`, `carrying_step`, `wants_board` to private — the compiler agrees; `suicide_errand` has **no caller at all** and stays `pub(super)` so it does not become dead code | `build` |
| 6 | Run the focused suite; expect 166 passed | see section 6 |
| 7 | Hand-edit the group tag if `group_tests check` names the new module | `group_tests check` silent |

**Never run `python -m tools.group_tests tag`.** It rewrites line endings and
splices text into source files; it has corrupted this repository before. If a
tag is needed, edit the `#[cfg(all(test, ...))]` attribute by hand and verify
with `check`.

**Move verbatim.** Do not fix a typo, do not reorder a match arm, do not
"improve" a comment while it is open. Anything noticed goes to a follow-up.

---

## 6. Verification

```bash
export RA2_DIR='I:/SteamLibrary/steamapps/common/Command & Conquer Red Alert II'
export VERA20K_REQUIRE_RETAIL_INI=1

# compiles, including the tests
python -m tools.cargo_run -- build --profile fast --bin vera20k

# the whole assist suite — 166 before, 166 after
python -m tools.cargo_run -- test -p vera20k-core --lib --profile fast -- sim::strategy::assist

# the autoplay crate, which drives the brain
python -m tools.cargo_run -- test -p vera20k-autoplay --lib --profile fast

# the group check must be silent
python -m tools.group_tests check
```

**Done means:** 166 passed, 0 failed; `group_tests check` silent; and
`git diff --stat` against the pre-split commit shows insertions and deletions
that balance apart from import blocks and the facade.

---

## 7. Precondition: the `Push` design

Write the army-commitment design **before** step 0. It decides whether
`common.rs` holds eight functions or four.

The question to answer on paper:

> Does `Push` absorb the "should I send this order" half of
> `orders::issue`?

- **No** — `Push` owns the army's target and gate, `issue` keeps admission.
  Proceed with all eight.
- **Yes** — some of `issue`'s callers move into `Push`, and `common.rs` is
  `issue` + `reachable` + `goal_cell` + `cell_in_bounds` only. Re-plan section 3
  before touching the tree.

Record the answer at the top of this file, under Status.

---

## 8. Rollback

One commit, nothing else depends on it:

```bash
git revert <sha>
```

If the split is wrong, revert it and keep the `Push` work. That independence
is the reason it is a separate commit.

---

## 9. Out of scope, permanently

- Splitting `orders.rs` by topic into seven children. Rejected in section 1.
- Splitting the test module by subject.
- Splitting `build.rs`. Its two halves (placement, roster queries) are a
  candidate, but it is 1,875 lines and no one has yet been confused by it in a
  way that a split would fix. Revisit only after the `Push` work lands.
- Any behaviour change. This commit moves code.
