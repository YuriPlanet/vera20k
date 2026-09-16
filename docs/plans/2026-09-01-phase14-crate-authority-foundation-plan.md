# Phase 14 Crate-Authority Foundation Implementation Plan

> **For Codex:** Execute this plan task-by-task. Each task is self-contained.

**Review-plan status:** READY after correcting the raw-ID `0xB2` slope
exception, signed human-count boundary, frame low-dword cast, and exact-length
Serde codec for the 256-slot table.

**Goal:** Replace the approximate scenario-start crate pass with one
simulation-owned, persisted, hashed, retail-ordered authority for ordinary
offline Skirmish and accepted playable random maps, while preserving the
fixed-map campaign and preview exclusions.

**Architecture:** Layered `[CrateRules]` processing owns the six startup
inputs. `sim::crates` owns the fixed slot table, native-x87 timer, placement
transaction, and visible/ghost result. Existing post-map loading invokes it in
retail order, and the app consumes live overlay state through the existing
`OverlayRenderIndex`; no presentation state feeds back into simulation.

**Design Doc:** `docs/plans/2026-09-01-phase14-crate-authority-foundation-design.md`

---

## Grounding Summary

- The active placement/runtime report verifies the 256 × 16-byte slot table,
  first-empty scan, 1,000-attempt random placer, exact hard-precheck boundary,
  accepted ghosts, identity-based Mark speed choice, timer formula, and no
  retail quick-checksum fold.
- The new caller/order correction report verifies that fixed-map
  `Full_Init` skips `Post_Map_Init` for raw mode 0 and nonzero control byte,
  while ordinary fresh Skirmish and successful generated-map paths reach it.
- Live Ghidra cold caller queries found exactly two parents for
  `Post_Map_Init`: `Full_Init` and `Read_Scenario`.
- Live assembly `0x004A184B..0x004A18C5` fixes accepted-write order:
  redraw, packed coordinate, timer RNG/math, start, aux, duration.
- Current `src/sim/crates.rs` already uses Scenario RNG and the shared FNPC,
  but owns only a local bool array, unsigned counts, the wrong rectangle, no
  timer state, direct overlay stamping, and no ghost model.
- Current `src/sim/scenario_post_map.rs` correctly gates crates on
  `skirmish_session: Some`, but grants AI credits before placement.
- Accepted random maps already enter the ordinary Skirmish load request through
  `LoadingRequest::with_accepted_random_map` and reach post-map with
  `Some(match_launch_descriptor)`; dialog preview does not construct a sim.
- `RulesLayerStack -> RulesPassProcessor -> ProcessedRulesLayers` is the
  existing native-pass projection. `allocate_late_global_references` already
  allocates the three crate overlay names before the new semantic accumulator
  must resolve their retained values.
- `Simulation` derives serde directly, so a non-skipped `CrateAuthority`
  field is the snapshot body. The current exact snapshot version is 113.
- `state_hash_with_schema` already carries version flags through v113; crate
  slots become the v114 field and probe.
- Retail `ini/rulesmd.ini [CrateRules]` supplies
  `CrateMinimum=1`, `CrateMaximum=255`, `CrateRegen=3`,
  `WoodCrateImg=CRATE`, `CrateImg=CRATE`,
  `WaterCrateImg=WCRATE`; `[MultiplayerDialogSettings] Crates=yes`.
- No active TS-only path is used. Network raw modes, pickup/effects,
  regeneration scans, removal, and specific-cell producers remain outside this
  PR.
- A fresh implementation critic invalidated the original “ordinary Mark only”
  scope hypothesis: custom startup image names can resolve to every active-YR
  special branch in `OverlayClass::Mark @ 0x005FC570`. The implementation must
  therefore reproduce high anchors, Railroad, walls, low endpoint tables/raw
  Scenario draws, Road tiberium germination, and `CellAnim`. Dense IDs
  `0x7E`/`0xA7` remain accepted ghosts because GSI-18.01 TS mutation is excluded.
- No consequential unknown remains after the special-branch correction.
  Test-only injection represents native allocation/Unlimbo failure without
  importing a native object graph.

## Key Technical Decisions

- **Keep the existing mode carrier:** `ScenarioPostMapOutput.crates` remains
  optional; `None` means fixed-map campaign, while option-off Skirmish returns
  `Some` with zero counts. — **Confidence: high**
  - **Source:** caller/order correction report; current
    `src/sim/scenario_post_map.rs:92-113`.
- **Accumulate only six startup fields:** constructor null image identities,
  signed min/max, exact regen bits, and per-pass key retention are semantic
  state; unrelated crate fields are not added. — **Confidence: high**
  - **Source:** active `ReadCrateRules @ 0x0066B900`; design scope; current
    `RulesPassProcessor` pattern.
- **Resolve Mark by allocated identity with water priority:** destination
  chooses water versus wood/common identity; selected identity equals current
  water first -> Float, otherwise current crate/wood -> Track. — **Confidence:
  high**
  - **Source:** active placement report `0x004A1944..0x004A1A78`.
- **Only two retryable hard rejections:** outside playfield or pre-existing
  overlay retries. Every subsequent failure, including null identity and any
  selected occupation bit `0x40`, creates a timed ghost. Other occupation bits
  remain admitted. Steep slopes are rejected by
  Mark except for exact raw overlay ID `0xB2`. — **Confidence: high**
  - **Source:** active placement report; `CrateSlot__ValidateCellAndCreateOverlay
    @ 0x004A18F0`.
- **Dispatch the complete reachable Mark body:** the constructor TerrainClass
  scan precedes Unlimbo and every Mark branch. The universal slope gate then
  precedes high bridge, TS legacy, Railroad, wall, low endpoint, and ordinary
  branches. High setters write raw `0`/`9` data before falling through and the
  four high IDs explicitly bypass ordinary passability. Low bodies use `3*L`
  raw Scenario draws and exact dense-ID tables; Road tiberium calls
  `SpreadCellGerminate(false)`. — **Confidence: high**
  - **Source:** `OverlayClass::Mark @ 0x005FC570`;
    `LOW_OVERLAY_MARK_FIXED_MAP_STAMP_RNG_TRANSACTION_GHIDRA_REPORT.md`;
    `CellClass::SpreadCellGerminate @ 0x004818E0`.
- **Use native x87 helper:** timer math uses `NativeF64Bits` and
  `X87Chop53`; no host `f64` arithmetic executes in `sim/`. — **Confidence:
  high**
  - **Source:** active placement report `0x004A1868..0x004A18C5`; repo
    `src/util/native_x87.rs`.
- **Persist/hash the raw table, not overlays:** ghost slots and timer words are
  future-affecting state even without a visible overlay. — **Confidence: high**
  - **Source:** active runtime report; current serde/world-hash architecture.
- **Reuse accepted-random loading:** add a production regression around the
  existing accepted map path; do not add an RMG-specific sim entry point. —
  **Confidence: high**
  - **Source:** `src/app/shell_skirmish.rs:225-267`;
    `src/app/loading/init.rs:1190-1264`.

## Open Questions

### Resolved During Planning

- **Does campaign run startup scatter?** No; `Full_Init` skips the helper for
  raw mode 0.
- **Is the upper random bound inclusive width or width minus one?** Coordinate
  is `left + RandomRanged(0, width - 1)`, with the analogous Y expression.
- **Does `none` retry?** No; the null pointer is an accepted timed ghost after
  hard prechecks.
- **What is accepted write order?** Redraw, coordinate at `0x004A1859`, timer
  RNG/math, then start/aux/duration at `0x004A18C0/2/5`.
- **Does accepted RMG need a new sim caller?** No; it already reaches the
  ordinary `skirmish_session: Some` post-map seam.

### Deferred to Implementation

- None. Compiler-driven borrow/layout adjustments may change private helper
  arrangement, but not interfaces or behavior in this plan.

## File Map

| Action | Path | Responsibility |
|---|---|---|
| Create | `src/rules/crate_rules.rs` | Six-field layered startup authority |
| Modify | `src/rules/mod.rs` | Export focused rules module |
| Modify | `src/rules/ini_parser.rs` | Per-pass accumulator and processed semantic output |
| Modify | `src/rules/ruleset.rs` | Install semantic crate state; private projected parser |
| Modify | `src/sim/crates.rs` | Public crate authority, placement transaction, and focused tests |
| Create | `src/sim/crates/state.rs` | Raw slots, timer fields, visibility queries |
| Modify | `src/sim/world/mod.rs` | Own and initialize `CrateAuthority` |
| Modify | `src/sim/overlay_grid.rs` | Crate-specific visible stamp primitive |
| Modify | `src/sim/scenario_post_map.rs` | Retail order and mode/option receipts |
| Modify | `src/sim/snapshot.rs` | Version 114 and raw-table round trip |
| Modify | `src/sim/world/world_hash.rs` | Version-gated ordered slot fold |
| Modify | `src/app/loading/init.rs` | First-frame occupied entry delivery and RMG regression |
| Modify | `src/app/frontend/skirmish.rs` | Preregister flagged and resolved crate overlay names |
| Modify | `src/render/overlay_atlas.rs` | Preload all resolved startup image frames |
| Modify | `src/app/presentation/overlay_index.rs` | Focused first-frame append/order regression if needed |

## Interface Changes

- `CrateRules` moves to `rules::crate_rules` and changes:
  `minimum/maximum: u32 -> i32`; three names become nullable retained overlay
  identities; `regen: NativeF64Bits` is added.
- `ProcessedRulesLayers` exposes `crate_rules() -> &CrateRules`.
- `Simulation` gains `pub(crate) crate_authority: CrateAuthority`.
- `CratePlacement` changes from `{requested: u32, placed: u32}` to
  `{requested: i32, accepted: u32, visible: u32}`.
- `human_player_count` changes from `u32` to `i32`, saturating only if the Rust
  collection length exceeds `i32::MAX`.
- `OverlayGrid` gains a crate-specific stamp that returns false out of bounds
  and otherwise writes identity/data while preserving unrelated cell fields.
- `ScenarioPostMapOutput` stays shape-compatible except for the nested receipt
  fields and remains `Option<CratePlacement>`.

All call sites are crate-private or internal. No public network, render, UI, or
audio API is introduced.

## Sim Checklist

- [ ] Timer math uses `NativeF64Bits`/`X87Chop53`; no host float in sim logic.
- [ ] `CrateAuthority` is serialized and included in v114 state hash.
- [ ] `sim::crates` imports no render/UI/sidebar/audio/network module.
- [ ] No per-tick change is introduced; startup runs before tick 0.
- [ ] House iteration is used only for a count, so `BTreeMap` order cannot
  alter results.
- [ ] Slot scans are fixed ascending array order.
- [ ] RNG draw order is X, Y per attempt and one timer draw per accepted result.
- [ ] The `u32` session frame is passed to timer storage with `as i32`,
  preserving the native low-dword bit pattern across wrap.

## Risk Areas

- The current parser's direct `RuleSet::from_ini` path can bypass per-pass
  allocation. Refactor it to wrap one `RulesLayerStack` and keep a private
  projected parser to avoid recursion.
- Native pointer aliasing must compare resolved numeric overlay IDs, not raw
  spelling.
- `none` can compare equal to another null configured pointer, but no object
  exists; it must still ghost after hard prechecks.
- Occupation tests apply the exact `OBJECT_OCCUPATION_BIT` (`0x40`) mask to the
  selected ground/deck byte; other raw bits remain admitted.
- Mark rejects `slope_type > 4` unless the selected raw overlay ID is exactly
  `0xB2`.
- Serde 1.0.229 implements direct fixed-array traits only through length 32;
  the 256-slot field needs an exact-length tuple codec rather than a derived
  array implementation or a variable-length `Vec` authority.
- Borrowing `overlay_grid`, `resolved_terrain`, occupation, and RNG from one
  `Simulation` may require a narrow transaction-input struct or take/restore;
  the authoritative owners and order must remain unchanged.
- Initial presentation must read the already-mutated live grid and upsert once;
  it must not stamp the overlay a second time.
- Adding a serde field changes bincode positional layout; version 114 and the
  exact-version rejection test are mandatory in the same buildable commit.

## Player-Experience Critical Items

| Task | Class | Item | Why it matters | Verification |
|---|---|---|---|---|
| 1 | COMPOUNDING | Layered signed rules and allocated image identities | Every startup count/image and later timer derives from it | Multi-pass parser tests + installed INI literal comparison |
| 2 | MILESTONE-BLOCKING | Persistent raw slots/timer and hash | Ghosts and future regen cannot be represented otherwise | slot bytes, timer goldens, snapshot/hash tests |
| 3 | MILESTONE-BLOCKING | Exact random/Mark/ghost transaction | Ordinary Crates-on Skirmish visibly diverges at frame zero | focused RNG, water/land, alias, null, occupation, slope tests |
| 4 | MILESTONE-BLOCKING | Crates-before-credits-before-alliances | Wrong initialization order changes RNG/state observation | post-map trace test |
| 4 | MILESTONE-BLOCKING | First-frame render-index delivery | Correct sim crates are otherwise invisible until another mutation | production-loading/index test |
| 4 | UNKNOWN-RISK CLOSED | Campaign and preview negative gates | A false positive would add crates to common campaign loads/previews | negative production tests |
| 5 | EXACTIFICATION-RESIDUAL | Native failed Overlay orphan object identity | Only malformed/failure paths; slot/timer/cell behavior is visible, orphan is not consumed | documented exclusion; ghost state test |

Representative production scenario: stock `rulesmd.ini`, a two-seat offline
Battle map, Crates checked, one land and one water-eligible region. The launch
must create the signed requested attempts before credits/alliances, show every
visible result in frame one, retain ghost timers, and restore/hash identically.

---

## Tasks

### Task 1: Layered six-field `CrateRules` authority

**Why:** Every later state and placement decision must consume the finalized
native-pass values and allocated identities.

**Files:**
- Create: `src/rules/crate_rules.rs`
- Modify: `src/rules/mod.rs`
- Modify: `src/rules/ini_parser.rs:571-602,656-797,1049-1148`
- Modify: `src/rules/ruleset.rs:1277-1337,2374-2375,2552-2575,2660`
- Modify: `src/rules/ini_parser_tests.rs`
- Modify: `src/rules/mission_data.rs` focused source-hash literal regression
- Modify: `src/sim/crates.rs` only to keep the existing approximate placement
  caller and its parser expectations compiling against nullable image names;
  Task 3 replaces that implementation.

**Pattern:** `RulesLayerStack` semantic sidecars and
`NativeF64Bits` readers such as `voxel_anim_type.rs`.

**Step 1: Define exact state and accumulator**

```rust
// src/rules/crate_rules.rs
use crate::rules::ini_parser::{IniFile, is_native_none_type_name};
use crate::util::native_x87::NativeF64Bits;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateRules {
    pub minimum: i32,
    pub maximum: i32,
    pub regen: NativeF64Bits,
    pub wood_crate_img: Option<String>,
    pub crate_img: Option<String>,
    pub water_crate_img: Option<String>,
}

impl Default for CrateRules {
    fn default() -> Self {
        Self {
            minimum: 1,
            maximum: 255,
            regen: NativeF64Bits::from_bits(10.0_f64.to_bits()),
            wood_crate_img: None,
            crate_img: None,
            water_crate_img: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CrateRulesAccumulator(CrateRules);

impl CrateRulesAccumulator {
    pub(crate) fn apply_pass(&mut self, ini: &IniFile) {
        let Some(section) = ini.section("CrateRules") else { return; };
        if let Some(value) = section.get_i32("CrateMinimum") {
            self.0.minimum = value;
        }
        if let Some(value) = section.get_i32("CrateMaximum") {
            self.0.maximum = value;
        }
        if section.get("CrateRegen").is_some() {
            self.0.regen = NativeF64Bits::from_bits(
                section.read_double(
                    "CrateRegen",
                    f64::from_bits(self.0.regen.bits()),
                ).to_bits(),
            );
        }
        for (key, target) in [
            ("WoodCrateImg", &mut self.0.wood_crate_img),
            ("CrateImg", &mut self.0.crate_img),
            ("WaterCrateImg", &mut self.0.water_crate_img),
        ] {
            if section.get(key).is_some() {
                let value = section.read_string(key, "", 0x80);
                *target = (!is_native_none_type_name(&value))
                    .then(|| value.to_ascii_uppercase());
            }
        }
    }

    pub(crate) fn finish(self) -> CrateRules { self.0 }
}
```

Use the exact `ReadString` capacity `0x80` for both semantic retention and late
OverlayType allocation, so 127-byte and 128-byte image-name boundaries resolve
to one identity.

**Step 2: Attach the accumulator to native pass processing**

- Add `crate_rules: CrateRulesAccumulator` to `RulesPassProcessor`.
- Keep existing late reference allocation, including `UnitCrateType`, to
  avoid regressing registry order outside this mechanism.
- Immediately after `allocate_late_global_references(pass)`, call
  `self.crate_rules.apply_pass(pass)`.
- Return both the projected INI and `crate_rules.finish()` from
  `RulesLayerStack::process`.
- Add `ProcessedRulesLayers::crate_rules(&self) -> &CrateRules`.

**Step 3: Remove the old parser and make every entry use the pass stack**

- Delete the old unsigned/string `CrateRules` block from `ruleset.rs`.
- Import `crate::rules::crate_rules::CrateRules`.
- Rename the current large `RuleSet::from_ini` body to private
  `from_projected_ini`.
- Make public `from_ini` clone the one input into
  `RulesLayerStack::new(ini.clone())` and call `from_rules_layers`.
- Make `from_processed_rules` call `from_projected_ini(processed.ini())`,
  then replace `rules.crate_rules` with `processed.crate_rules().clone()`
  before setting `source_ini_hash`.
- In the temporary existing `sim::crates` implementation, resolve image names
  through `.as_deref().and_then(...)`, log `none` for null, and update only its
  parser expectations to `Some(...)`; do not alter placement semantics in this
  rules-only commit.
- Update the focused mission-data source-hash regression to pin both the raw
  `IniFile::content_hash` literal and the new one-layer
  `RulesLayerStack::content_hash` literal used by public `RuleSet::from_ini`.

**Step 4: Add focused tests**

Tests must prove:

- no `[CrateRules]` -> null images, 1/255, exact 10.0 bits;
- stock section -> CRATE/CRATE/WCRATE, 1/255, exact 3.0 bits;
- later absent section retains prior values;
- later section with one key changes only that key;
- negative and inverted signed min/max survive;
- `none` becomes null and unknown non-none image is allocated into the
  projected `[OverlayTypes]` registry;
- alias spellings resolve to one numeric ID in `OverlayTypeRegistry`;
- direct `RuleSet::from_ini` and one-layer `from_rules_layers` agree.

**Step 5: Verify and commit**

Before Cargo: `Get-Process cargo,rustc -ErrorAction SilentlyContinue`.

Run:
`cargo test -p vera20k --lib crate_rules -- --nocapture`

Also run:
`cargo test -p vera20k --lib rules_hash_and_enum_wire_values_survive_vocabulary_move -- --nocapture`

Expected: every crate-rules and pass-retention test passes.

Commit:
`git commit -m "feat(rules): preserve native startup crate authority"`

### Task 2: Persistent slot table, native timer, snapshot, and hash

**Why:** Placement cannot be correct or reviewable while its accepted result is
transient or derived from visible overlays.

**Files:**
- Modify: `src/sim/crates.rs`
- Create: `src/sim/crates/state.rs`
- Modify: `src/sim/world/mod.rs:632-840,2980-3018`
- Modify: `src/sim/snapshot.rs:300-331` and focused tests
- Modify: `src/sim/world/world_hash.rs:420-535` and schema fold

**Pattern:** direct serde-owned `Simulation` fields plus version probes used by
v110-v113.

**Step 1: Define raw state**

```rust
// src/sim/crates/state.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CrateSlot {
    pub start_frame: i32,
    pub aux: u32,
    pub duration: i32,
    pub cell_x: i16,
    pub cell_y: i16,
}

impl Default for CrateSlot {
    fn default() -> Self {
        Self { start_frame: -1, aux: 0, duration: 0, cell_x: 0, cell_y: 0 }
    }
}

impl CrateSlot {
    pub fn is_empty(self) -> bool { self.cell_x == 0 && self.cell_y == 0 }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CrateAuthority {
    #[serde(with = "crate_slot_array_serde")]
    slots: [CrateSlot; 256],
}

mod crate_slot_array_serde {
    use super::CrateSlot;
    use serde::de::{self, SeqAccess, Visitor};
    use serde::ser::SerializeTuple;
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S>(slots: &[CrateSlot; 256], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(256)?;
        for slot in slots {
            tuple.serialize_element(slot)?;
        }
        tuple.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[CrateSlot; 256], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SlotsVisitor;
        impl<'de> Visitor<'de> for SlotsVisitor {
            type Value = [CrateSlot; 256];

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("exactly 256 crate slots")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut slots = [CrateSlot::default(); 256];
                for (index, slot) in slots.iter_mut().enumerate() {
                    *slot = seq.next_element()?.ok_or_else(|| {
                        de::Error::invalid_length(index, &"exactly 256 crate slots")
                    })?;
                }
                Ok(slots)
            }
        }
        deserializer.deserialize_tuple(256, SlotsVisitor)
    }
}

impl Default for CrateAuthority {
    fn default() -> Self { Self { slots: [CrateSlot::default(); 256] } }
}

impl CrateAuthority {
    pub(crate) fn first_empty_index(&self) -> Option<usize> {
        self.slots.iter().position(|slot| slot.is_empty())
    }
    pub(crate) fn slots(&self) -> &[CrateSlot; 256] { &self.slots }
    pub(crate) fn slot_mut(&mut self, index: usize) -> &mut CrateSlot {
        &mut self.slots[index]
    }
}
```

Add `pub(crate) crate_authority: CrateAuthority` beside `overlay_grid` in
`Simulation` and initialize it with `Default`.

**Step 2: Implement the pure timer helper**

```rust
pub(crate) fn crate_timer_words(
    regen: NativeF64Bits,
    draw: u32,
    current_frame: i32,
) -> (i32, u32, i32) {
    let regen = X87Chop53::load_f64(regen).expect("validated CrateRegen");
    let lower = X87Chop53::mul(regen, X87Chop53::load_i32(450));
    let upper = X87Chop53::mul(regen, X87Chop53::load_i32(1800));
    let fraction = X87Chop53::div(
        X87Chop53::load_i32(draw as i32),
        X87Chop53::load_i32(0x7fff_fffe),
    ).expect("nonzero timer divisor");
    let value = X87Chop53::add(
        lower,
        X87Chop53::mul(fraction, X87Chop53::sub(upper, lower)),
    );
    let stored_upper = X87Chop53::store_f64(upper).expect("finite CrateRegen upper");
    let duration = X87Chop53::ftol_i64(value).unwrap_or(i64::MIN) as i32;
    (current_frame, (stored_upper.bits() >> 32) as u32, duration)
}
```

If the existing x87 API requires a differently named conversion for unsigned
draw, preserve the exact nonnegative integer value before division; do not
replace the expression with host float.

The fallback is native masked `FISTP qword` integer-indefinite. The slot stores
only its low dword, which is zero for both positive and negative overflow.

**Step 3: Persist and hash**

- Bump `SNAPSHOT_VERSION` to 114 with a comment naming the raw 256-slot table.
- Add `include_crate_authority_v114` as the final
  `state_hash_with_schema` flag.
- When true, fold a domain tag, then every slot's five raw fields in ascending
  index order.
- Add `state_hash_without_crate_authority_v114` with all earlier flags true
  and v114 false.
- Do not modify `compute_retail_multiplayer_checksum`.

**Step 4: Add focused tests**

- every fresh slot is exactly `{-1,0,0,0,0}`;
- only both-zero coordinates mean empty;
- first-empty scan is ascending and a full table returns `None`;
- timer draw 0 -> duration 1350 at regen 3;
- draw `0x7fff_fffe` -> 5400 and aux `0x40B51800`;
- one interior draw pins forward interpolation;
- current frame is copied without increment;
- snapshot v114 round-trips visible-shaped and ghost-shaped slots;
- the tuple codec rejects a truncated slot sequence and never admits a
  variable-length crate authority;
- changing each raw slot field changes current hash;
- v114-off probe ignores all slot differences;
- retail quick checksum is identical when only crate slots differ;
- version test expects 114.

**Step 5: Verify and commit**

Check processes, then run:

- `cargo test -p vera20k --lib crate_authority -- --nocapture`
- `cargo test -p vera20k --lib crate_timer -- --nocapture`
- `cargo test -p vera20k --lib snapshot_version_is_current -- --nocapture`

Expected: all focused tests pass.

Commit:
`git commit -m "feat(sim): persist and hash native crate slots"`

### Task 3: Exact random placement, Mark, visible stamp, and ghosts

**Why:** This is the player-visible/deterministic core of the mechanism.

**Files:**
- Modify: `src/sim/crates.rs`
- Modify: `src/sim/overlay_grid.rs:298-365`

**Pattern:** existing shared `find_nearby_passable_cell`, raw occupation
authority, `ResolvedTerrainCell.bridge_facts`, and specialized overlay writers.

**Step 1: Define receipts and count**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CratePlacement {
    pub requested: i32,
    pub accepted: u32,
    pub visible: u32,
}

pub fn scenario_start_crate_count(rules: &CrateRules, human_count: i32) -> i32 {
    rules.maximum.min(rules.minimum.max(human_count))
}

pub fn human_player_count(sim: &Simulation) -> i32 {
    i32::try_from(
        sim.houses
            .values()
            .filter(|house| house.is_human && !house.multiplay_passive)
            .count(),
    )
    .unwrap_or(i32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OneCrateResult { HardRejected, AcceptedGhost, AcceptedVisible }
```

The outer loop is:

```rust
let requested = scenario_start_crate_count(&rules.crate_rules, human_count);
for _ in 0..requested.max(0) {
    match place_one_random_crate(/* authoritative inputs */) {
        OneCrateResult::HardRejected => {}
        OneCrateResult::AcceptedGhost => accepted += 1,
        OneCrateResult::AcceptedVisible => { accepted += 1; visible += 1; }
    }
}
```

It never tops up visible/accepted count.

**Step 2: Implement exact rectangle and FNPC**

- Require `playfield_bounds` and `playfield_size_height`; malformed headless
  fixtures return hard failure without inventing map-width coordinates.
- Derive active left/top/width/height with the same signed wrapping helpers used
  by current playfield normalization.
- Draw X then Y as
  `left + RandomRanged(0,width-1)` and
  `top + RandomRanged(0,height-1)`.
- Compare/swap the range endpoints as signed dwords, perform both draws even
  for zero/negative widths, use wrapping dword addition, and narrow each result
  through signed `i16` before FNPC.
- Preserve the existing zero-target FNPC tuple and
  `min(SizeW+SizeH,32)` cap.
- Origin water selects FNPC Float; every other origin selects Track.
- Retry at most 1,000 times.

**Step 3: Implement hard prechecks and complete reachable Mark dispatch**

After FNPC returns a snapped cell:

1. outside playfield -> retry;
2. existing overlay identity -> retry;
3. destination `yr_cell_land_type == Water` selects resolved
   `WaterCrateImg`, otherwise resolved `WoodCrateImg`;
4. apply the constructor TerrainClass gate, then the universal slope gate;
   dispatch high anchors, explicit `0x7E`/`0xA7` TS ghosts, Railroad, walls,
   and low endpoint trigger tables. High setters write anchor/F1/F2/opposite
   data `0` or `9`, stamp flags, and continue through later precedence;
5. only the ordinary branch compares selected numeric identity with resolved
   current water ID first -> Float; otherwise matching current crate or wood ID
   -> Track;
6. in the ordinary branch, a selected `None`, unresolved terrain cell,
   `slope_type > 4` when the selected raw overlay ID is not exact `0xB2`,
   selected occupation bit `0x40`, selected
   non-bridge speed zero, or injected allocation/Unlimbo/Mark failure ->
   accepted ghost;
7. `bridge_facts.raw_flags & 0x100 != 0` selects
   `raw_cell_occupation.deck_bits` and bypasses non-bridge speed-zero for a
   non-high ordinary overlay; otherwise select `ground_bits`. Exact high IDs
   bypass both occupation planes and speed checks explicitly;
8. ordinary success preserves native write order: identity/data zero (except
   high anchors, whose setter data is retained), Road data one and optional
   tiberium germination, `Crate=yes` override, `CellAnim`, then common Recalc.
   A constructed CellAnim receives Tiberium `Color=` palette authority and the
   CellClass ground Z-adjust only when the now-live cell reports tiberium.

Use the exact `OBJECT_OCCUPATION_BIT` mask; do not collapse it to whole-byte
nonzero admission. Do not use `place_overlay_native_runtime` wall/protection
semantics.

**Step 4: Add the specialized raw-field stamp and branch helpers**

```rust
pub(crate) fn write_crate_mark_fields(
    &mut self,
    resolved_terrain: &mut ResolvedTerrainGrid,
    registry: &OverlayTypeRegistry,
    rx: u16,
    ry: u16,
    overlay_id: u8,
    overlay_data: u8,
) -> bool {
    let Some(index) = index_of(self.width, self.height, rx, ry) else {
        return false;
    };
    let Some(name) = registry.name(overlay_id) else {
        return false;
    };
    self.cells[index].overlay_id = Some(overlay_id);
    self.cells[index].overlay_data = overlay_data;
    // Preserve wall_owner/unrelated fields rather than replacing OverlayCell.
    self.dirty_cells.push((rx, ry));
    resolved_terrain.set_runtime_overlay_bridge_identity(
        rx, ry, overlay_id, overlay_data, name,
    );
    true
}
```

`sim::crates` owns the exact high-anchor setters, wall connectivity, low bridge
fixed/search/body transaction and shared-dummy alias, Road tiberium density,
`CellAnim`, and the common Recalc tail. Do not fold those branches back into a
single `native_mark_overlay_data` approximation.

**Step 5: Install accepted slot/timer in native order**

- The logical redraw is a Rust-native no-pixel trace/no-op.
- Write packed coordinate to the retained first-empty slot.
- Consume timer RNG only after acceptance.
- Compute words with `crate_timer_words`.
- Pass `sim.session.binary_frame as i32` as `current_frame`; this preserves the
  native low-dword bit pattern when the unsigned Rust frame crosses `i32::MAX`.
- Store start, aux, duration in that order.
- Hard rejection leaves the slot byte-for-byte unchanged and consumes no timer
  draw.

**Step 6: Add focused tests**

Cover:

- signed/inverted/negative/>256 requested counts;
- exact nonzero-left/top inclusive rectangle, signed reversed endpoints, low-i16
  narrowing, and X/Y draw order;
- failed attempts spend X/Y; full table spends no RNG;
- origin water/land FNPC classification independent of destination image;
- destination water/land image;
- water-first alias priority;
- null image accepted ghost;
- terrain object, non-exempt slopes 5+, selected occupation bit `0x40`,
  speed-zero, and injected Mark failure accepted ghosts; other ground/deck
  occupation bits remain visible;
- exact raw overlay ID `0xB2` remains eligible on slopes 5+;
- structural bridge selects deck, checks bit `0x40`, and bypasses underlying
  speed zero;
- occupied overlay/outside playfield retry and do not install timer;
- visible stamp writes zero ordinarily, one for Road, and `0xFF` for
  `Crate=yes`, preserves wall owner, and dirties once;
- Railroad bypass/data zero; wall passability/connectivity/data ordering; high
  anchor raw setter data, fallthrough overrides, explicit passability bypass,
  and flag family; every low trigger table with exact
  `3*L` raw draws plus occupied-row no-op; Road tiberium neighbor density;
  visible and failed ordinary `CellAnim`, including conditional tiberium
  palette/Z post-writes; explicit TS legacy ghosts;
- ordinary pre-stamp ghosts preserve the cell; specialized branches retain any
  native writes that precede a later rejection;
- both visible and ghost slots store coordinate then timer words;
- 1,000 hard rejections return with empty slot.

**Step 7: Verify and commit**

Check processes, then run:

- `cargo test -p vera20k --lib scenario_start_crate -- --nocapture`
- `cargo test -p vera20k --lib crate_placement -- --nocapture`
- `cargo test -p vera20k --lib place_crate_overlay -- --nocapture`

Expected: all focused tests pass with literal RNG/slot assertions.

Commit:
`git commit -m "feat(sim): reproduce native startup crate placement"`

### Task 4: Retail post-map order and first-frame production delivery

**Why:** Correct internals are incomplete until the ordinary loading path and
first presentation consume them in the native lifecycle.

**Files:**
- Modify: `src/sim/scenario_post_map.rs:41-120` and tests
- Modify: `src/app/loading/init.rs:1190-1264,2335-2393,2485-2507`
- Modify: `src/app/frontend/skirmish.rs:2608-2635` and tests
- Modify: `src/app/presentation/overlay_index.rs` tests if needed

**Pattern:** existing post-map receipt, generated-map launch snapshot, and
`OverlayRenderIndex::upsert_occupied`.

**Step 1: Reorder the Skirmish branch**

```rust
let crates = if let Some(descriptor) = input.skirmish_session {
    let session = descriptor.session();
    let human_count = crate::sim::crates::human_player_count(self);
    let initial_path = self.path_grid_snapshot();
    let placement = crate::sim::crates::place_scenario_start_crates(
        self, input.rules, input.overlay_registry, initial_path.as_deref(), human_count,
    );
    crate::sim::scenario_bootstrap::apply_skirmish_ai_opening_credits(self);
    crate::sim::scenario_bootstrap::apply_skirmish_launch_alliances(
        self, input.house_roster, session,
    );
    Some(placement)
} else {
    self.house_alliances = input.house_roster.alliance_map();
    None
};
```

Add a test-only trace around these three calls and assert
`Crates -> AiCredits -> Alliances`. Do not change earlier ore/navigation order.

**Step 2: Preserve caller/option distinctions**

- Fixed-map campaign `None` returns `crates=None` and consumes no crate RNG.
- Skirmish with option off returns `Some({requested:0,accepted:0,visible:0})`.
- The generated launch snapshot already calls
  `finalize_constructed_scenario(..., Some(match_launch_descriptor))`; assert
  its receipt is `Some`.
- Preview-only tests assert no `Simulation` or post-map receipt is created.

**Step 3: Deliver visible startup cells to the initial index**

After `finalize_constructed_scenario` and before building `MapLoadResult`:

- inspect the `OverlayGrid` pending dirty-cell receipt without consuming it;
- de-duplicate coordinates in first-write order so low-bridge extensions and
  high-setter neighbor writes are included;
- materialize `OverlayEntry {rx,ry,overlay_id,frame:overlay_data}` only when
  identity is present;
- upsert those entries into the initial source vector with
  `OverlayRenderIndex::replace_from_source` then `upsert_occupied`, and move
  the resulting vector back into `overlays_connected`;
- never mutate `OverlayGrid` from this app step.

Extract a crate-neutral helper if necessary so initial loading and runtime use
the same index contract without constructing `AppState`.

**Step 4: Preregister crate art names**

Extend `preregister_runtime_overlay_names` to accept registry entries whose
flags have `crate_type` plus all three resolved `CrateRules` identities, in
addition to walls and low bridges. Preload the actual reachable Mark outputs:
frame zero for flagged crates, ordinary/Railroad data, wall frames, low
fixed/body IDs and states, and Road tiberium density frames. Route selected
high-anchor identities to `BridgeAtlas` body/shadow roots before startup
placement rather than generating `OverlayAtlas` keys. Add reachable crate
`CellAnim` names to the scheduler asset closure and live tiberium-remap keys to
the SHP atlas. Keep counters separate so crate additions are not misreported as
bridges.

**Step 5: Add focused production tests**

- campaign no-bootstrap/no-RNG;
- option-off Skirmish `Some` zero receipt;
- option-on Skirmish ordering trace;
- accepted generated map reaches `Some` and installs slots;
- preview lifecycle creates no sim crate state;
- every visible startup Mark dirty cell appends after source overlays in
  first-write order without consuming the sim/navigation receipt;
- startup bridge writes rebuild `BridgeRuntimeState` and republish initial
  navigation before AI credits, while leaving that dirty receipt pending;
- ghosts do not enter the index;
- existing-coordinate upsert retains its source slot;
- CRATE/WCRATE and a late-allocated configured `Crate=false` identity are
  preregistered/preloaded before scatter; high roots use BridgeAtlas and a
  crate CellAnim is bound before construction with its remap variant covered;
- renderer constant remains crate frame zero.

**Step 6: Verify and commit**

Check processes, then run:

- `cargo test -p vera20k --lib scenario_post_map -- --nocapture`
- `cargo test -p vera20k --lib random_map_launch -- --nocapture`
- `cargo test -p vera20k --lib runtime_overlay_names -- --nocapture`
- `cargo test -p vera20k --lib overlay_render_index -- --nocapture`

Expected: all focused production and presentation tests pass.

Commit:
`git commit -m "fix(app): deliver startup crates in retail post-map order"`

### Task 5: Literal retail, asset, and production validation

**Why:** Unit tests alone cannot prove the active executable, installed rules,
and installed theater assets used by production.

**Files:**
- Modify only focused ignored validation tests if an existing test cannot emit
  the required literals.
- Do not add capture bundles or unrelated certification matrices.

**Pattern:** existing installed-retail ignored tests and supported release
`asset.exe`.

**Step 1: Active binary**

- Compute SHA-256 for
  `C:\Users\enok\Documents\Command and Conquer Red Alert II\gamemd.exe`.
- Record exact hash in validation output.
- Re-read live Ghidra `Post_Map_Init`, `Full_Init`,
  `MapClass__PlaceCrateAtRandomCell`, and
  `CrateSlot__PlaceOverlayAndInitTimer` only where implementation comments
  cite them.

Expected literals: campaign gate, X/Y bounds, two hard prechecks, water-first
identity comparison, accepted ghost, coordinate-before-timer stores.

**Step 2: Installed rules**

Read installed active rules sources and print/compare:

`CrateMinimum=1`, `CrateMaximum=255`, `CrateRegen=3`,
`WoodCrateImg=CRATE`, `CrateImg=CRATE`,
`WaterCrateImg=WCRATE`, and launcher `Crates=yes`.

Run the production rules-layer loader and assert the resulting signed values,
exact regen bits, and resolved CRATE/WCRATE IDs match.

**Step 3: Installed assets**

Build only the supported release asset tool if its binary is not current, after
the required process check. Use it to show that `CRATE.TEM` and `WCRATE.TEM`
resolve from installed theater MIX data and are SHP(TS), 60×60, two frames.

**Step 4: Focused production proof**

Run the focused ignored production test proving:

```text
installed rules layers
-> resolved CRATE/WCRATE identities
-> accepted ordinary Skirmish post-map
-> persistent slot + live OverlayGrid
-> initial OverlayRenderIndex entry
-> registered crate art name/frame zero
```

Expected: literal PASS with IDs, cells, slot timer words, and asset metadata.

### Task 6: Fresh critic, fixes, final suite, and publication

**Why:** The goal requires independent read-only review of every diff and
literal validation before publication.

**Files:** no planned source file; only critic-confirmed corrections.

**Step 1: Prepare review evidence**

- Show `git diff origin/main...HEAD`.
- Show branch/HEAD and clean/dirty status.
- Attach focused Cargo outputs and literal active binary/INI/asset/production
  outputs.
- Confirm no Cargo/Rust process remains.

**Step 2: Fresh read-only critic**

Give one fresh critic the entire diff and literal evidence. Require it to check:

- design/plan scope and one-mechanism boundary;
- caller gates and post-map order;
- signed count/RNG/no-top-up behavior;
- identity aliasing and null/post-precheck ghosts;
- raw occupation/deck semantics;
- native x87 timer and slot write order;
- serde v114/hash probe/quick-checksum exclusion;
- first-frame presentation and accepted-RMG production path;
- no sim dependency on render/UI/sidebar/audio/network;
- focused tests and external evidence.

The critic must return PASS or actionable findings. Do not ask it to edit.

**Step 3: Repair and resubmit**

For every confirmed finding:

- patch only the implicated mechanism;
- rerun the smallest focused `--lib` filter;
- refresh literal evidence if behavior changed;
- give the new diff/evidence to a **fresh** read-only critic.

Repeat until a fresh critic returns PASS. Re-review any conflict resolution.

**Step 4: One final full library suite**

Only after critic PASS and PR readiness:

- check `Get-Process cargo,rustc`;
- run exactly once:
  `cargo test -p vera20k --lib`.

Expected: PASS with zero failing tests.

**Step 5: Publish and merge**

- Ensure each incremental commit is buildable and the worktree is clean.
- Push `feature/phase14-crate-authority-foundation`.
- Open one PR targeting `main`, containing only this mechanism.
- Merge only with green checks and critic PASS.
- Refresh/fetch `origin/main`, verify the merge commit, and rederive the Phase
  14 frontier before selecting the next mechanism.

## Sources & References

- **Design:** `docs/plans/2026-09-01-phase14-crate-authority-foundation-design.md`
- **Caller/order correction:** [docs/research/SCENARIO_START_CRATE_POST_MAP_CALLER_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SCENARIO_START_CRATE_POST_MAP_CALLER_GATE_GHIDRA_REPORT.md) — HIGH
- **Primary placement/runtime:** `docs/research/PHASE3_ACTIVE_RETAIL_CRATE_RUNTIME_GHIDRA_REPORT.md` — HIGH, with caller wording overridden by the correction report
- **Supporting system report:** `docs/research/CRATE_SYSTEM_GHIDRA_REPORT.md` — verified context; newer reports override conflicts
- **Ghidra:** `ScenarioClass__Read_Scenario @ 0x00684620`,
  `ScenarioClass__Read_Scenario_INI @ 0x00686730`,
  `ScenarioClass__Post_Map_Init @ 0x00686890`,
  `ScenarioClass__Full_Init @ 0x00686B20`,
  `MapClass__PlaceCrateAtRandomCell @ 0x0056BD40`,
  `CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0`,
  `CrateSlot__ValidateCellAndCreateOverlay @ 0x004A18F0`.
- **INI:** `ini/rulesmd.ini:782-796`,
  `ini/rulesmd.ini:3017-3034`.
- **Current Rust:** `src/rules/ini_parser.rs:571-797,1049-1148`;
  `src/rules/ruleset.rs:1277-1337,2552-2575`;
  `src/sim/crates.rs`; `src/sim/scenario_post_map.rs:41-120`;
  `src/sim/world/mod.rs:630-840`; `src/sim/snapshot.rs:300-360`;
  `src/sim/world/world_hash.rs:420-535`;
  `src/app/loading/init.rs:1190-1264,2335-2393`;
  `src/app/frontend/skirmish.rs:2608-2635`;
  `src/app/presentation/overlay_index.rs`.
- **Recent relevant commits:** `35a5fdbf` FNPC anchor gate,
  `5b3792fe` playfield contract, `f0ab8731` sim-owned post-map,
  `39996b98` accepted random-map launch regeneration,
  `db76065a` latest snapshot/hash schema.

## Post-Plan Self-Review

- [x] Every design requirement maps to Tasks 1-5.
- [x] No TBD/TODO/placeholder step remains.
- [x] Interfaces precede their consumers.
- [x] Sim code remains below render/UI/audio/network.
- [x] Signed counts, exact RNG, ghost semantics, persistence, and first-frame
  delivery have focused regressions.
- [x] Current git history and current files were re-read after design approval.
- [x] Research index, primary reports, live Ghidra, INI, assets, production
  paths, and Rust touchpoints are cited.
- [x] All technical decisions are high confidence.
- [x] No behavioral question is deferred.
- [x] Common player-visible, deterministic, residual, and negative-gate risks
  are classified.
- [x] Incremental commits and one final full `--lib` suite follow `AGENTS.md`.

**Autonomous approval:** The fresh skeptical `review-plan` gate returned READY.
The plan is dependency-coherent and implements one Phase 14 mechanism.
