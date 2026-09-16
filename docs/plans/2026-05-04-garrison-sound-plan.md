# Garrison Sound & EVA Implementation Plan

> **For Claude:** Execute this plan task-by-task. Each task is self-contained. Commit after every task.

**Goal:** Wire up the three player-audible cues that gamemd plays around garrison transitions: `EVA_StructureGarrisoned` (first occupant enters), `EVA_StructureAbandoned` (last occupant leaves), and the `BuildingGarrisonedSound` SFX from `[AudioVisual]`.

**Architecture:** Three new owner-bound `SimSoundEvent` variants emitted from `sim/passenger.rs` at the first-board / last-unload transitions. The app layer drains them, applies the local-human-player gate (matching the existing `BuildingComplete`/`UnitComplete` precedent), and dispatches to three new `GameSoundEvent` variants. One new `[AudioVisual]` INI field (`BuildingGarrisonedSound`).

**Design Doc:** [docs/plans/2026-05-04-garrison-sound-design.md](2026-05-04-garrison-sound-design.md)

---

## Grounding Summary

**Docs (R1):** [GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) §4 Step 5 documents the EVA + SFX emission flow inside `BuildingClass::AddGarrisonOccupant` (0x00522910). Audited via `/verify-doc` on 2026-05-04 — status YELLOW with no impact on this work (the four wrong labels are unrelated functions). [GARRISON_OCCUPANT_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_OCCUPANT_SYSTEM_GHIDRA_REPORT.md) confirms the BuildingClass+0x694 occupant count semantics. [GARRISON_IMPLEMENTATION_PLAN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_IMPLEMENTATION_PLAN.md) §10 lists these three cues as "Not implemented" — confirmed accurate.

**Ghidra (R2):** Three xrefs verified live on 2026-05-04:
- `EVA_StructureGarrisoned` string at 0x008255b0 → xref from `BuildingClass::AddGarrisonOccupant` at 0x005229bc
- `EVA_StructureAbandoned` string at 0x0081926c → xref from `BuildingClass::CheckAutoSellOrCivilian` at 0x004582d3
- `BuildingGarrisonedSound` string at 0x0083a8fc → xref from `RulesClass::ReadAudioVisual` at 0x00669bf6 (confirms it's parsed from `[AudioVisual]`, not `[General]`)

**Repo pattern (R3):** Mirror the `BuildingComplete { owner }` flow — sim emits unconditionally with owner ID, app layer at [src/app_sim_tick.rs:315-333](../../src/app_sim_tick.rs#L315-L333) gates on local human, looks up faction-specific sound via `EvaRegistry::get(event_name, faction)`, returns `GameSoundEvent::BuildingReady { sound_id }`. Same shape adapted for our three cues. INI field follows `condition_red`/`condition_yellow` precedent in `GeneralRules::from_ini` (already reads from the `[AudioVisual]` section).

**INI keys (R4):**
- `ini/rulesmd.ini:614` — `[AudioVisual] BuildingGarrisonedSound=BuildingGarrisoned`
- `ini/evamd.ini:1417` — `[EVA_StructureGarrisoned]` Allied=`ceva107`, Russian=`csof107`, Yuri=`cyur107`
- `ini/evamd.ini:1425` — `[EVA_StructureAbandoned]` Allied=`ceva108`, Russian=`csof108`, Yuri=`cyur108`

**Still unknown:** Nothing material. The audio backend's per-event playback paths are already routed through the existing `GameSoundEvent::sound_id()` / `screen_pos()` accessors plus a small match in `app_building_anim.rs:421-449` — adding three variants requires extending those match arms but doesn't change the underlying audio engine.

## Key Technical Decisions

- **Three discrete `SimSoundEvent` variants** (vs a generic `EvaEvent { event_name }`) — **Confidence:** high. **Source:** repo pattern `BuildingComplete`/`UnitComplete` in [src/sim/world/mod.rs:104-107](../../src/sim/world/mod.rs#L104-L107). Approved in design doc.
- **Sim emits unconditionally; local-human gate at app layer** — **Confidence:** high. **Source:** repo pattern at [src/app_sim_tick.rs:316-322](../../src/app_sim_tick.rs#L316-L322). Required for replay/lockstep determinism.
- **`StructureAbandoned` uses pre-revert owner** — **Confidence:** high. **Source:** Ghidra `BuildingClass::CheckAutoSellOrCivilian` at 0x00458200 — `IsHumanPlayer` check is **before** `ChangeOwner` in the decompilation (verified 2026-05-04).
- **`building_garrisoned_sound: Option<String>` lives on `GeneralRules`** — **Confidence:** high. **Source:** `condition_yellow`/`condition_red` already live there despite parsing from `[AudioVisual]` ([src/rules/ruleset.rs:580-615](../../src/rules/ruleset.rs#L580-L615)). Following the same convention.
- **No refactor of `WeaponFired` → `PositionalSfx`** — **Confidence:** high (deferred). **Source:** design doc Out of Scope. Wait until 3+ positional SFX consumers exist.

## Open Questions

### Resolved During Planning

- **Where does `BuildingGarrisonedSound` come from in INI?** `[AudioVisual]` section, key `BuildingGarrisonedSound=BuildingGarrisoned`. Verified at `ini/rulesmd.ini:614` and Ghidra `RulesClass::ReadAudioVisual` xref.
- **What's the EVA fallback ID for each cue?** `ceva107` / `ceva108` (Allied default) — used as fallback in app dispatch when registry lookup misses, mirroring the `"ceva048"` precedent for `BuildingComplete`.
- **Does the audio backend need a new dispatch?** No new backend — existing `GameSoundEvent::sound_id()` / `screen_pos()` accessors plus the match in `app_building_anim.rs:430-451` cover playback. Need to extend the match with three new variants.

### Deferred to Implementation

- None. All design and INI questions are resolved.

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | [src/sim/world/mod.rs](../../src/sim/world/mod.rs) | Add 3 `SimSoundEvent` variants |
| Modify | [src/audio/events.rs](../../src/audio/events.rs) | Add 3 `GameSoundEvent` variants, extend `sound_id()` and `screen_pos()` accessors |
| Modify | [src/rules/ruleset.rs](../../src/rules/ruleset.rs) | Add `building_garrisoned_sound: Option<String>` to `GeneralRules`, parse from `[AudioVisual]`, default test |
| Modify | [src/sim/passenger.rs](../../src/sim/passenger.rs) | Emit garrison events at first-board / last-unload, plus 4 unit tests |
| Modify | [src/app_sim_tick.rs](../../src/app_sim_tick.rs) | 3 new dispatch arms in the SimSoundEvent → GameSoundEvent match |
| Modify | [src/app_building_anim.rs](../../src/app_building_anim.rs) | 3 new arms in the GameSoundEvent consumer match (for ducking / mute logic) |

No new files. No deletions.

## Interface Changes

- `SimSoundEvent` enum — adds three variants. Public-but-internal-to-crate; no external consumers. The `match` in `app_sim_tick.rs:282-364` will need three new arms (handled in Task 6).
- `GameSoundEvent` enum — adds three variants. The `match` in `app_building_anim.rs:430-451` will need new arms (handled in Task 7). The `sound_id()` and `screen_pos()` accessor methods on `GameSoundEvent` need updates (handled in Task 2).
- `GeneralRules` struct — adds one optional field. Existing serializers / clients see `None` for old saves. Non-breaking.

## Sim Checklist

- [x] All math uses `fixed`-point — N/A (no math added; only event emission)
- [x] New state included in deterministic state hash — `sound_events` is `#[serde(skip)]` and ephemeral per-tick (existing convention). New variants do not add persistent state.
- [x] No dependencies on render/ui/sidebar/audio/net — sim only references `SimSoundEvent` (data type). The `GameSoundEvent` extension lives in `audio/events.rs` which is data-only and intentionally importable from sim per its module doc comment ([src/audio/events.rs:16-18](../../src/audio/events.rs#L16-L18)).
- [x] Tick ordering impact noted — none. Events are emitted within `tick_passenger_system` which already runs in the existing slot.
- [x] BTreeMap iteration order considered — N/A. Emission happens inside the existing snapshot-then-mutate loop, which already iterates `entities.keys_sorted()`.

## Risk Areas

- **Owner capture timing in `tick_unloading`:** must capture `t.owner` *before* the mutable borrow that performs the revert, or borrow checker complains. Task 5 spells out the order explicitly.
- **Borrow split in `tick_boarding`:** pushing to `sim.sound_events` while holding `sim.entities.get(transport_id)` requires reading the owner/position into locals first. Task 4 spells this out.
- **Test setup overhead:** `tick_boarding` is a `Simulation`-level function; tests need `Simulation::new()` + a minimal `RuleSet` with a CanBeOccupied building type and an Occupier infantry type. Tasks 4 and 5 include test fixtures.

## Parity-Critical Items

| Task # | Item | Why it matters | Verification |
|--------|------|----------------|--------------|
| Task 4 | First-occupant emission only (count==1) | gamemd plays the cues only when count transitions 0→1 — every subsequent occupant is silent. Filling a 5-slot building should produce exactly one EVA + one SFX, not five of each. | Unit test `test_second_occupant_emits_no_event` — assert exactly one of each event after first board, none after second. |
| Task 4 | Owner used for emission == post-transfer owner | When a neutral civilian is garrisoned by Player A, ownership transfers to A and the EVA fires for A (so A hears it). gamemd does the IsHumanPlayer check after AddGarrisonOccupant has set the new owner. | Unit test sets passenger owner != neutral building owner, asserts emitted event carries the passenger's owner (== post-transfer building owner). |
| Task 5 | Owner used for `StructureAbandoned` == pre-revert owner | gamemd's `CheckAutoSellOrCivilian` checks `IsHumanPlayer` before `ChangeOwner`, so the abandoning player hears the cue, not the civilian house. | Unit test asserts `StructureAbandoned { owner }` carries the pre-revert owner ID, then verifies `t.owner` was reverted afterward. |
| Task 6 | Local-player gate at app layer (not sim) | Replays / spectator views must produce identical sim events regardless of who is watching. Local gating in sim would desync. | Inspection: dispatch arm contains `if !local_owner_name.eq_ignore_ascii_case(...) { continue; }` — same shape as `BuildingComplete` arm. |
| Task 8 | EVA cadence in-game matches gamemd | Player should hear "Structure garrisoned" exactly once per garrison, "Structure abandoned" exactly once per empty-out. Mis-firing on every occupant or never firing at all are both immediately noticeable. | Manual: garrison a 5-slot civilian building with 5 conscripts. Hear EVA + SFX once, when the first conscript enters. Sell the building. Hear EVA once when the last conscript leaves. |

---

## Tasks

### Task 1: Add three new `SimSoundEvent` variants

**Why:** Define the sim-side event types first. Everything downstream (emission, dispatch) depends on these existing.

**Files:**
- Modify: [src/sim/world/mod.rs:87-114](../../src/sim/world/mod.rs#L87-L114)

**Pattern:** Mirror existing `BuildingComplete { owner }` (owner-bound EVA) and `WeaponFired { ..., rx, ry }` (positional SFX) variants.

**Step 1: Add variants**

Add to the `SimSoundEvent` enum, after the existing `SuperWeaponStrike` variant (line 113):

```rust
    /// First occupant entered a CanBeOccupied building (cargo 0→1).
    /// Owner is the post-transfer building owner. App layer plays
    /// EVA_StructureGarrisoned if owner is local human.
    StructureGarrisoned { owner: InternedId },

    /// Last occupant left a garrisoned building (cargo 1→0).
    /// Owner is the **pre-revert** owner — the player whose garrison
    /// just emptied. Matches gamemd's CheckAutoSellOrCivilian which
    /// fires EVA before ChangeOwner. App layer plays EVA_StructureAbandoned
    /// if owner is local human.
    StructureAbandoned { owner: InternedId },

    /// First-occupant SFX from rulesmd [AudioVisual] BuildingGarrisonedSound.
    /// Positional cue gated on owner == local human.
    BuildingGarrisonedSfx { owner: InternedId, rx: u16, ry: u16 },
```

**Step 2: Verify compile**

Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS (compile may warn about non-exhaustive matches in app_sim_tick.rs — that's expected, fixed in Task 6).

If the `match sim_event { ... }` in [src/app_sim_tick.rs:282-362](../../src/app_sim_tick.rs#L282-L362) errors out as non-exhaustive, add a temporary catch-all `_ => continue,` arm at the bottom and remove it in Task 6. (Prefer fixing it at Task 6 in a single pass.)

**Step 3: Commit**

```
git add src/sim/world/mod.rs
git commit -m "garrison: add three new SimSoundEvent variants"
```

---

### Task 2: Add three new `GameSoundEvent` variants

**Why:** Define the app-layer event types and update the accessor methods. Tasks 6 and 7 depend on these existing.

**Files:**
- Modify: [src/audio/events.rs:22-100](../../src/audio/events.rs#L22-L100)

**Pattern:** Mirror existing `BuildingReady { sound_id }` (EVA-style) and `WeaponFired { sound_id, screen_pos }` (positional SFX).

**Step 1: Add variants**

Insert after the `UnitReady` variant (line 68):

```rust
    /// EVA cue: a friendly building was garrisoned (first occupant entered).
    StructureGarrisoned {
        /// sound.ini ID for the EVA announcement.
        sound_id: String,
    },

    /// EVA cue: a friendly garrison was abandoned (last occupant left).
    StructureAbandoned {
        /// sound.ini ID for the EVA announcement.
        sound_id: String,
    },

    /// Positional SFX from [AudioVisual] BuildingGarrisonedSound — plays at
    /// the building's screen position when the first occupant enters.
    BuildingGarrisonedSfx {
        /// sound.ini ID for the SFX (resolves "BuildingGarrisoned" → file).
        sound_id: String,
        /// Screen position for spatial audio.
        screen_pos: Option<(f32, f32)>,
    },
```

**Step 2: Update `sound_id()` accessor**

Replace the body of `pub fn sound_id(&self) -> &str` ([src/audio/events.rs:79-90](../../src/audio/events.rs#L79-L90)) so the `match` covers the new variants. Final form:

```rust
    pub fn sound_id(&self) -> &str {
        match self {
            Self::WeaponFired { sound_id, .. }
            | Self::UnitSelected { sound_id }
            | Self::UnitMoveOrder { sound_id }
            | Self::UnitAttackOrder { sound_id }
            | Self::EntityDestroyed { sound_id, .. }
            | Self::BuildingReady { sound_id }
            | Self::UnitReady { sound_id }
            | Self::UiSound { sound_id }
            | Self::StructureGarrisoned { sound_id }
            | Self::StructureAbandoned { sound_id }
            | Self::BuildingGarrisonedSfx { sound_id, .. } => sound_id,
        }
    }
```

**Step 3: Update `screen_pos()` accessor**

Replace the body of `pub fn screen_pos(&self) -> Option<(f32, f32)>` ([src/audio/events.rs:93-99](../../src/audio/events.rs#L93-L99)):

```rust
    pub fn screen_pos(&self) -> Option<(f32, f32)> {
        match self {
            Self::WeaponFired { screen_pos, .. } => *screen_pos,
            Self::EntityDestroyed { screen_pos, .. } => *screen_pos,
            Self::BuildingGarrisonedSfx { screen_pos, .. } => *screen_pos,
            _ => None,
        }
    }
```

**Step 4: Add unit tests**

Add inside the existing `#[cfg(test)] mod tests` at the bottom of the file (after `test_queue_drain` at line 145):

```rust
    #[test]
    fn test_structure_garrisoned_sound_id_accessor() {
        let evt: GameSoundEvent = GameSoundEvent::StructureGarrisoned {
            sound_id: "ceva107".to_string(),
        };
        assert_eq!(evt.sound_id(), "ceva107");
        assert_eq!(evt.screen_pos(), None);
    }

    #[test]
    fn test_building_garrisoned_sfx_screen_pos_accessor() {
        let evt: GameSoundEvent = GameSoundEvent::BuildingGarrisonedSfx {
            sound_id: "BuildingGarrisoned".to_string(),
            screen_pos: Some((100.0, 200.0)),
        };
        assert_eq!(evt.sound_id(), "BuildingGarrisoned");
        assert_eq!(evt.screen_pos(), Some((100.0, 200.0)));
    }
```

**Step 5: Verify**

Run: `cargo test -p ra2-rust-game --lib audio::events::tests -- --nocapture`
Expected: all events tests PASS, including the two new ones.

**Step 6: Commit**

```
git add src/audio/events.rs
git commit -m "audio: add three new GameSoundEvent variants for garrison cues"
```

---

### Task 3: Parse `BuildingGarrisonedSound` from `[AudioVisual]`

**Why:** The SFX dispatch in Task 6 needs `rules.general.building_garrisoned_sound`. Make the field exist and get parsed before any consumer references it.

**Files:**
- Modify: [src/rules/ruleset.rs](../../src/rules/ruleset.rs) — add field, default, parser, test

**Pattern:** Mirror existing `condition_yellow` / `condition_red` in `GeneralRules` (defined struct field, default value, parsed from `[AudioVisual]` section in `from_ini`).

**Step 1: Add the field to `GeneralRules`**

In [src/rules/ruleset.rs](../../src/rules/ruleset.rs), insert after `pub condition_red_x1000: i64,` (line 169):

```rust
    /// SFX played when the first occupant enters a CanBeOccupied building.
    /// Parsed from [AudioVisual] BuildingGarrisonedSound (typically "BuildingGarrisoned").
    /// None = no sound configured. Resolved at app layer to a sound.ini entry.
    pub building_garrisoned_sound: Option<String>,
```

**Step 2: Add the default**

In `impl Default for GeneralRules` (around line 401), add to the struct literal:

```rust
            building_garrisoned_sound: None,
```

(Insert next to `condition_red_x1000: 250,` to keep `[AudioVisual]` fields grouped.)

**Step 3: Parse the field in `GeneralRules::from_ini`**

In `GeneralRules::from_ini` ([src/rules/ruleset.rs:575-615](../../src/rules/ruleset.rs#L575-L615)), inside the returned `Self { ... }` literal, insert next to `condition_red_x1000: ...,`:

```rust
            building_garrisoned_sound: audio_visual
                .and_then(|s| s.get("BuildingGarrisonedSound"))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
```

**Step 4: Add unit tests**

Add the new tests to the existing `#[cfg(test)] mod tests` block at [src/rules/ruleset.rs:1389-1390](../../src/rules/ruleset.rs#L1389-L1390) (search for `mod tests` near the end of the file). The block already has `use super::*;` which gives access to `GeneralRules`. The test fixtures pattern `IniFile::from_str(...)` then `GeneralRules::from_ini(&ini)` is established by the existing tests there — mirror them.

Note: the file `src/rules/ini_parser_tests.rs` exists and contains `[AudioVisual]` tests, but those test the `IniFile`/`IniSection` parser primitives, not `GeneralRules::from_ini`. Don't put the new tests there — they belong with the other `GeneralRules` tests in `ruleset.rs`.

```rust
    #[test]
    fn test_building_garrisoned_sound_parsed() {
        let ini_str = "\
[General]
[AudioVisual]
BuildingGarrisonedSound=BuildingGarrisoned
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert_eq!(
            general.building_garrisoned_sound.as_deref(),
            Some("BuildingGarrisoned")
        );
    }

    #[test]
    fn test_building_garrisoned_sound_default_none() {
        let ini_str = "\
[General]
[AudioVisual]
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert!(general.building_garrisoned_sound.is_none());
    }

    #[test]
    fn test_building_garrisoned_sound_empty_treated_as_none() {
        let ini_str = "\
[General]
[AudioVisual]
BuildingGarrisonedSound=
";
        let ini = IniFile::from_str(ini_str);
        let general = GeneralRules::from_ini(&ini);
        assert!(general.building_garrisoned_sound.is_none());
    }
```

If the imports `IniFile` and `GeneralRules` are not already in scope, add `use super::*;` and `use crate::rules::ini_parser::IniFile;` at the top of the test module (look for the existing pattern in `condition_yellow` tests).

**Step 5: Verify**

Run: `cargo test -p ra2-rust-game --lib building_garrisoned_sound -- --nocapture`
Expected: 3 tests PASS.

Run: `cargo test -p ra2-rust-game --lib general_rules -- --nocapture`
Expected: existing `GeneralRules` tests still PASS (no regressions on `condition_yellow` / `condition_red`).

**Step 6: Commit**

```
git add src/rules/ruleset.rs
git commit -m "rules: parse BuildingGarrisonedSound from [AudioVisual]"
```

---

### Task 4: Emit garrison events at first-occupant boarding

**Why:** Hook the actual sim trigger. Without this, the SimSoundEvent variants from Task 1 are dead code.

**Files:**
- Modify: [src/sim/passenger.rs:280-396](../../src/sim/passenger.rs#L280-L396) — emit at first-board
- Modify: [src/sim/passenger.rs:589-678](../../src/sim/passenger.rs#L589-L678) — add tests

**Pattern:** Insert into the `if boarded { ... }` branch of `tick_boarding`, after the existing ownership-transfer block (line ~370) and before the `pax.passenger_role = PassengerRole::Inside { transport_id };` line. Read transport position/owner via `entities.get(transport_id)` to avoid borrow conflicts.

**Step 1: Import `SimSoundEvent` if not already in scope**

At the top of [src/sim/passenger.rs](../../src/sim/passenger.rs), check existing imports. The current import is:

```rust
use crate::sim::world::Simulation;
```

Add (or extend the existing `use crate::sim::world::...` line):

```rust
use crate::sim::world::{Simulation, SimSoundEvent};
```

**Step 2: Add the first-occupant emission block**

In `tick_boarding` ([src/sim/passenger.rs:280-396](../../src/sim/passenger.rs#L280-L396)), inside the `if boarded { ... }` branch, after the existing ownership-transfer block ends (after the closing `}` of the `if transport_can_be_occupied { ... }` block, around line 371) and **before** the `// Hide the passenger entity.` comment (line 372):

```rust
                // Garrison sound/EVA: emit on first occupant entry only.
                // gamemd AddGarrisonOccupant fires EVA + BuildingGarrisonedSound
                // when count transitions 0→1; subsequent occupants are silent.
                let first_occupant = sim
                    .entities
                    .get(transport_id)
                    .and_then(|t| t.passenger_role.cargo())
                    .map_or(false, |c| c.count() == 1);
                if first_occupant
                    && rules
                        .object(&transport_type_str)
                        .map_or(false, |o| o.can_be_occupied)
                {
                    if let Some(t) = sim.entities.get(transport_id) {
                        let owner = t.owner;
                        let rx = t.position.rx;
                        let ry = t.position.ry;
                        sim.sound_events
                            .push(SimSoundEvent::StructureGarrisoned { owner });
                        sim.sound_events.push(SimSoundEvent::BuildingGarrisonedSfx {
                            owner,
                            rx,
                            ry,
                        });
                    }
                }

```

**Step 3: Verify compile**

Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS.

**Step 4: Add tests**

Append to the bottom of the existing `#[cfg(test)] mod tests` block in [src/sim/passenger.rs](../../src/sim/passenger.rs). The tests need a fuller `Simulation` setup than the existing `PassengerCargo`-only tests. Use `Simulation::new()` plus a minimal RuleSet pattern from `src/sim/miner/miner_tests.rs:250` as a model.

Add at the top of the test module (or extend existing imports):

```rust
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::components::Position;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::{SimSoundEvent, Simulation};
    use crate::util::fixed_math::SimFixed;
```

(Adjust to match the file's existing import style. If `Position`, `Simulation`, etc. are already in scope from other tests in this module, skip those.)

Add a helper near the top of the test module:

```rust
    fn garrison_test_rules() -> RuleSet {
        let ini_str = "\
[InfantryTypes]
0=E1
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]
0=CAGAS01

[E1]
Name=Conscript
Cost=100
Strength=125
Armor=none
Speed=4
Occupier=yes

[CAGAS01]
Name=GasStation
Cost=0
Strength=400
Armor=wood
CanBeOccupied=yes
CanOccupyFire=yes
MaxNumberOccupants=5

[General]
[AudioVisual]
BuildingGarrisonedSound=BuildingGarrisoned
ConditionRed=25%
ConditionYellow=50%
";
        let ini = IniFile::from_str(ini_str);
        RuleSet::from_ini(&ini).expect("parse garrison test rules")
    }

    /// Spawn a CanBeOccupied building entity at (rx, ry) owned by `owner_str`.
    /// Returns the building's stable id. The PassengerRole::Transport cargo
    /// is initialized with capacity == MaxNumberOccupants from the rule.
    fn spawn_garrison_building(
        sim: &mut Simulation,
        rules: &RuleSet,
        type_ref: &str,
        owner_str: &str,
        rx: u16,
        ry: u16,
    ) -> u64 {
        let stable_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern(owner_str);
        let type_id = sim.interner.intern(type_ref);
        let mut ge = GameEntity::test_default(stable_id, type_ref, owner_str, rx, ry);
        ge.owner = owner_id;
        ge.type_ref = type_id;
        let obj = rules.object(type_ref).expect("type exists");
        ge.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(obj.max_number_occupants, 1),
        };
        sim.entities.insert(ge);
        stable_id
    }

    /// Spawn an Occupier infantry entity at (rx, ry) in `Boarding::Entering` state
    /// targeting `transport_id`. Returns the infantry's stable id.
    fn spawn_boarding_occupier(
        sim: &mut Simulation,
        type_ref: &str,
        owner_str: &str,
        transport_id: u64,
        rx: u16,
        ry: u16,
    ) -> u64 {
        let stable_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern(owner_str);
        let type_id = sim.interner.intern(type_ref);
        let mut ge = GameEntity::test_default(stable_id, type_ref, owner_str, rx, ry);
        ge.owner = owner_id;
        ge.type_ref = type_id;
        ge.passenger_role = PassengerRole::Boarding {
            target_transport_id: transport_id,
            phase: BoardingPhase::Entering,
        };
        sim.entities.insert(ge);
        stable_id
    }
```

Add the tests:

```rust
    #[test]
    fn test_first_occupant_emits_garrisoned_event() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let _pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        tick_boarding(&mut sim, &rules);

        // Find the garrisoned event and the SFX event.
        let mut found_eva = false;
        let mut found_sfx = false;
        for evt in &sim.sound_events {
            match evt {
                SimSoundEvent::StructureGarrisoned { owner } => {
                    assert_eq!(
                        sim.interner.resolve(*owner),
                        "Americans",
                        "EVA owner should be the garrisoning player"
                    );
                    found_eva = true;
                }
                SimSoundEvent::BuildingGarrisonedSfx { owner, rx, ry } => {
                    assert_eq!(sim.interner.resolve(*owner), "Americans");
                    assert_eq!((*rx, *ry), (10, 10));
                    found_sfx = true;
                }
                _ => {}
            }
        }
        assert!(found_eva, "expected StructureGarrisoned event");
        assert!(found_sfx, "expected BuildingGarrisonedSfx event");
    }

    #[test]
    fn test_second_occupant_emits_no_garrison_event() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);

        // Pre-populate with one occupant (simulating a previous successful board).
        if let Some(t) = sim.entities.get_mut(bldg) {
            if let Some(cargo) = t.passenger_role.cargo_mut() {
                cargo.board(9999, 1);
            }
        }
        let _pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg, 10, 11);

        tick_boarding(&mut sim, &rules);

        for evt in &sim.sound_events {
            match evt {
                SimSoundEvent::StructureGarrisoned { .. }
                | SimSoundEvent::BuildingGarrisonedSfx { .. } => {
                    panic!("garrison event should NOT emit on non-first occupant: {:?}", evt);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_non_garrison_transport_emits_no_garrison_events() {
        // Passengers=5 IFV-style transport (not CanBeOccupied) — no garrison events.
        let ini_str = "\
[InfantryTypes]
0=E1
[VehicleTypes]
0=IFV
[AircraftTypes]
[BuildingTypes]

[E1]
Name=Conscript
Cost=100
Strength=125
Armor=none
Speed=4
Occupier=yes

[IFV]
Name=IFV
Cost=600
Strength=200
Armor=light
Speed=8
Passengers=5

[General]
[AudioVisual]
ConditionRed=25%
ConditionYellow=50%
";
        let ini = IniFile::from_str(ini_str);
        let rules = RuleSet::from_ini(&ini).expect("parse");
        let mut sim = Simulation::new();
        // Spawn the IFV (Passengers=5, not CanBeOccupied).
        let bldg_id = sim.allocate_stable_id();
        let owner_id = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("IFV");
        let mut bldg = GameEntity::test_default(bldg_id, "IFV", "Americans", 10, 10);
        bldg.owner = owner_id;
        bldg.type_ref = type_id;
        bldg.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(5, 0),
        };
        sim.entities.insert(bldg);
        let _pax = spawn_boarding_occupier(&mut sim, "E1", "Americans", bldg_id, 10, 11);

        tick_boarding(&mut sim, &rules);

        for evt in &sim.sound_events {
            match evt {
                SimSoundEvent::StructureGarrisoned { .. }
                | SimSoundEvent::BuildingGarrisonedSfx { .. } => {
                    panic!("non-garrison transport should not emit garrison events: {:?}", evt);
                }
                _ => {}
            }
        }
    }
```

**Step 5: Verify tests**

Run: `cargo test -p ra2-rust-game --lib passenger::tests -- --nocapture`
Expected: all 3 new tests PASS, plus all existing `PassengerCargo` tests still PASS.

If a test fails because `Simulation::new()` is not callable from the test module (private), check that `Simulation::new()` is `pub` — it is (per `src/sim/world/mod.rs` exports). If `allocate_stable_id` is not public, you may need to use the public `spawn_*` API or expose the method `pub(crate)`. Inspect the actual signature and adjust the helpers.

**Step 6: Commit**

```
git add src/sim/passenger.rs
git commit -m "garrison: emit StructureGarrisoned + BuildingGarrisonedSfx on first board"
```

---

### Task 5: Emit `StructureAbandoned` at last-occupant unload

**Why:** Pair with Task 4 — without this the EVA cycle is one-sided. Captures the pre-revert owner correctly per gamemd.

**Files:**
- Modify: [src/sim/passenger.rs:560-587](../../src/sim/passenger.rs#L560-L587) — modify the `cargo_empty` branch
- Modify: [src/sim/passenger.rs](../../src/sim/passenger.rs) test module — add test

**Pattern:** Capture `t.owner` *before* the `&mut t` borrow performs the revert, then push the event.

**Step 1: Modify the cargo-empty branch in `tick_unloading`**

Find the existing `if cargo_empty { ... }` block (around line 565). Replace the inner contents to capture the pre-revert owner and emit:

```rust
        if cargo_empty {
            // Garrison ownership revert: when last occupant leaves a CanBeOccupied
            // building, revert ownership to the building's original (pre-garrison)
            // owner. Matches original engine's CheckAutoSellOrCivilian which
            // transfers back to the Civilian house identified by side index.
            let is_garrison_building = rules
                .object(&transport_type_str)
                .map(|obj| obj.can_be_occupied)
                .unwrap_or(false);
            // Pre-intern "Neutral" as fallback for garrison ownership revert.
            let neutral_id = sim.interner.intern("Neutral");
            // Capture pre-revert owner BEFORE the mut borrow — gamemd's
            // CheckAutoSellOrCivilian fires EVA_StructureAbandoned for the
            // player whose garrison just emptied, not the post-revert civilian.
            let abandoning_owner = if is_garrison_building {
                sim.entities.get(transport_id).map(|t| t.owner)
            } else {
                None
            };
            if let Some(t) = sim.entities.get_mut(transport_id) {
                t.order_intent = None;
                if is_garrison_building {
                    let revert_owner = t.garrison_original_owner.take().unwrap_or(neutral_id);
                    t.owner = revert_owner;
                    ownership_changed = true;
                }
            }
            if let Some(owner) = abandoning_owner {
                sim.sound_events
                    .push(SimSoundEvent::StructureAbandoned { owner });
            }
        }
```

**Step 2: Verify compile**

Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS.

**Step 3: Add test**

Append to the test module:

```rust
    #[test]
    fn test_last_occupant_emits_abandoned_event_with_pre_revert_owner() {
        let mut sim = Simulation::new();
        let rules = garrison_test_rules();
        // Spawn a CanBeOccupied building owned by Americans (post-garrison state),
        // with garrison_original_owner = Neutral (pre-garrison state).
        let bldg = spawn_garrison_building(&mut sim, &rules, "CAGAS01", "Americans", 10, 10);
        let neutral_id = sim.interner.intern("Neutral");
        // Set up the "1 occupant inside, original owner = Neutral" state.
        if let Some(t) = sim.entities.get_mut(bldg) {
            t.garrison_original_owner = Some(neutral_id);
            if let Some(cargo) = t.passenger_role.cargo_mut() {
                // Pretend a passenger entity 12345 was inside.
                cargo.board(12345, 1);
            }
            t.order_intent = Some(OrderIntent::Unloading);
        }
        // Spawn a placeholder passenger entity so unload_first finds it.
        let pax_owner = sim.interner.intern("Americans");
        let pax_type = sim.interner.intern("E1");
        let mut pax = GameEntity::test_default(12345, "E1", "Americans", 9, 10);
        pax.owner = pax_owner;
        pax.type_ref = pax_type;
        pax.passenger_role = PassengerRole::Inside { transport_id: bldg };
        sim.entities.insert(pax);

        // Tick unloading — should pop the one passenger and trigger empty branch.
        tick_unloading(&mut sim, &rules);

        // Assert StructureAbandoned was emitted with the PRE-revert owner (Americans).
        let mut found = false;
        for evt in &sim.sound_events {
            if let SimSoundEvent::StructureAbandoned { owner } = evt {
                assert_eq!(
                    sim.interner.resolve(*owner),
                    "Americans",
                    "StructureAbandoned should carry pre-revert owner, not post-revert civilian"
                );
                found = true;
            }
        }
        assert!(found, "expected StructureAbandoned event after last occupant left");

        // Confirm the revert actually happened (post-revert owner = Neutral).
        let bldg_owner_str = sim
            .entities
            .get(bldg)
            .map(|t| sim.interner.resolve(t.owner).to_string())
            .expect("building exists");
        assert_eq!(bldg_owner_str, "Neutral", "owner should have reverted to Neutral");
    }
```

If `OrderIntent` isn't already imported in the test module, add `use crate::sim::components::OrderIntent;`.

**Step 4: Verify**

Run: `cargo test -p ra2-rust-game --lib passenger::tests::test_last_occupant_emits_abandoned -- --nocapture`
Expected: PASS.

Run: `cargo test -p ra2-rust-game --lib passenger -- --nocapture`
Expected: full passenger test suite PASS.

**Step 5: Commit**

```
git add src/sim/passenger.rs
git commit -m "garrison: emit StructureAbandoned on last-occupant unload (pre-revert owner)"
```

---

### Task 6: App-side dispatch from `SimSoundEvent` to `GameSoundEvent`

**Why:** Without this the events emitted by Tasks 4 and 5 sit in the queue with nothing consuming them.

**Files:**
- Modify: [src/app_sim_tick.rs:282-364](../../src/app_sim_tick.rs#L282-L364)

**Pattern:** Mirror the existing `BuildingComplete { owner }` arm at lines 315-333.

**Step 1: Add three dispatch arms**

In the `match sim_event { ... }` block, after the `SimSoundEvent::UnitComplete { owner } => { ... }` arm (line 343-361), insert:

```rust
                    SimSoundEvent::StructureGarrisoned { owner } => {
                        // EVA cue: only play for the local human player.
                        let owner_str = sim.interner.resolve(owner);
                        if !local_owner_name
                            .as_deref()
                            .map_or(false, |l| l.eq_ignore_ascii_case(owner_str))
                        {
                            continue;
                        }
                        let faction = crate::app_building_anim::eva_faction_key(
                            owner_str,
                            &state.house_roster,
                        );
                        let sound_id = state
                            .eva_registry
                            .get("EVA_StructureGarrisoned", faction)
                            .unwrap_or("ceva107")
                            .to_string();
                        GameSoundEvent::StructureGarrisoned { sound_id }
                    }
                    SimSoundEvent::StructureAbandoned { owner } => {
                        let owner_str = sim.interner.resolve(owner);
                        if !local_owner_name
                            .as_deref()
                            .map_or(false, |l| l.eq_ignore_ascii_case(owner_str))
                        {
                            continue;
                        }
                        let faction = crate::app_building_anim::eva_faction_key(
                            owner_str,
                            &state.house_roster,
                        );
                        let sound_id = state
                            .eva_registry
                            .get("EVA_StructureAbandoned", faction)
                            .unwrap_or("ceva108")
                            .to_string();
                        GameSoundEvent::StructureAbandoned { sound_id }
                    }
                    SimSoundEvent::BuildingGarrisonedSfx { owner, rx, ry } => {
                        // Positional SFX: only audible to the local human player
                        // (matches gamemd VocClass::PlayAt with IsHumanPlayer gate).
                        let owner_str = sim.interner.resolve(owner);
                        if !local_owner_name
                            .as_deref()
                            .map_or(false, |l| l.eq_ignore_ascii_case(owner_str))
                        {
                            continue;
                        }
                        let sound_id = match rules
                            .general
                            .building_garrisoned_sound
                            .as_deref()
                        {
                            Some(s) if !s.is_empty() => s.to_string(),
                            _ => continue,
                        };
                        let (sx, sy) = crate::map::terrain::iso_to_screen(rx, ry, 0);
                        GameSoundEvent::BuildingGarrisonedSfx {
                            sound_id,
                            screen_pos: Some((sx, sy)),
                        }
                    }
```

The match must remain exhaustive — these three arms cover the new `SimSoundEvent` variants from Task 1.

**Step 2: Verify compile**

Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS — no non-exhaustive-match warnings on `SimSoundEvent`.

If Task 1 added a temporary `_ => continue,` arm, remove it now.

**Step 3: Verify**

Run: `cargo build -p ra2-rust-game`
Expected: clean build.

Run: `cargo test -p ra2-rust-game --lib`
Expected: all tests PASS (no app-layer tests touch these arms; the existing test suite should still be green).

**Step 4: Commit**

```
git add src/app_sim_tick.rs
git commit -m "app: dispatch garrison SimSoundEvent variants to GameSoundEvent"
```

---

### Task 7: Audio consumer match arms

**Why:** [src/app_building_anim.rs:430-451](../../src/app_building_anim.rs#L430-L451) holds a `match` over `GameSoundEvent` for non-spatial concerns (EVA ducking, mute logic, etc.). The new variants must be acknowledged here or the match becomes non-exhaustive.

**Files:**
- Modify: [src/app_building_anim.rs:421-455](../../src/app_building_anim.rs#L421-L455)

**Pattern:** Inspect the existing arms — `BuildingReady` and `UnitReady` are grouped (line 447) as "EVA-style cues that need no special routing." The new EVA cues fit there. The SFX cue is positional (like `WeaponFired` → no special handling).

**Step 1: Read the current match**

Open [src/app_building_anim.rs](../../src/app_building_anim.rs) and read the `match` body around line 430-451 carefully. Each existing arm performs some side-effect (EVA priority, voice line gating, etc.) or explicitly `{}` no-ops.

**Step 2: Add new arms**

The current consumer at [src/app_building_anim.rs:434-481](../../src/app_building_anim.rs#L434-L481) has four arms:

1. `UnitSelected | UnitMoveOrder | UnitAttackOrder => { play_voice_sound(...) }` (line 436)
2. `BuildingReady { .. } | UnitReady { .. } => {}` (line 447) — **intentionally empty**, comment above reads `// EVA events — temporarily disabled.`
3. `UiSound { .. } => { play_sound(...) }` (line 449)
4. `_ => { ... spatial volume + play_sound_with_volume(...) }` (line 459) — catch-all that handles all events with `screen_pos()` (currently `WeaponFired` and `EntityDestroyed`).

Extend the `BuildingReady`/`UnitReady` arm to cover the two new EVA variants. The current line reads:

```rust
            GameSoundEvent::BuildingReady { .. } | GameSoundEvent::UnitReady { .. } => {}
```

Replace with:

```rust
            GameSoundEvent::BuildingReady { .. }
            | GameSoundEvent::UnitReady { .. }
            | GameSoundEvent::StructureGarrisoned { .. }
            | GameSoundEvent::StructureAbandoned { .. } => {}
```

**Important:** the EVA group is intentionally empty (see the `// EVA events — temporarily disabled.` comment on the line above). Adding our new variants here means they will inherit the silencing — that's the correct grouping for consistency. When EVA dispatch is re-enabled engine-wide as a separate change, all four variants will play together. This is acknowledged in Task 8 Step 3 below.

For `BuildingGarrisonedSfx`, **do NOT add a specific arm**. The existing `_` catch-all at line 459 already handles all events with `screen_pos()` correctly: it computes spatial volume from the screen position and calls `play_sound_with_volume()`. `BuildingGarrisonedSfx` will fall through to this arm automatically. Adding a specific empty arm like `BuildingGarrisonedSfx { .. } => {}` would silence the SFX — verify by inspection that no such arm has been added.

**Step 3: Verify compile**

Run: `cargo check -p ra2-rust-game --lib`
Expected: PASS — no non-exhaustive-match warnings.

**Step 4: Verify tests**

Run: `cargo test -p ra2-rust-game --lib`
Expected: full suite PASS.

**Step 5: Commit**

```
git add src/app_building_anim.rs
git commit -m "audio: handle garrison sound events in consumer match"
```

---

### Task 8: Full regression + manual parity verification

**Why:** Confirm no regressions from cumulative changes and verify the user-visible behavior matches gamemd.

**Files:** None modified (verification only).

**Step 1: Full test suite**

Run: `cargo test -p ra2-rust-game`
Expected: all PASS, including the 8 new tests added in Tasks 2-5.

**Step 2: Build the game**

Run: `cargo build -p ra2-rust-game --release`
Expected: clean build, no warnings related to garrison code.

**Step 3: Manual in-game verification**

> **Important — EVA cues are engine-wide silenced today.** The audio backend at [src/app_building_anim.rs:446-447](../../src/app_building_anim.rs#L446-L447) has the existing `BuildingReady | UnitReady` group as an empty arm with the comment `// EVA events — temporarily disabled.`, and Task 7 extends that group with our two new EVA variants. So `EVA_StructureGarrisoned` and `EVA_StructureAbandoned` **will not be audible** in this Task even when this plan is correctly implemented. The SimSoundEvent → GameSoundEvent dispatch chain is verifiable from the unit tests (Tasks 4 and 5 already cover that). The audible verification of EVA cues is **deferred** until EVA dispatch is re-enabled engine-wide as a separate change (out of scope for this plan).
>
> What IS audibly verifiable in this Task: the `BuildingGarrisonedSound` SFX (which falls through to the spatial `_` catch-all in the consumer match) and the visual side-effects (pip overlay, ownership transfer).

Launch the engine on a YR map with civilian buildings (e.g., a stock multiplayer map with gas stations, hotels, restaurants).

Test cases:

1. **First-occupant cue:** Select a Conscript (or any Occupier infantry, e.g. GI for Allied), right-click on a neutral civilian gas station with `MaxNumberOccupants=5`. When the conscript enters:
   - Hear the `BuildingGarrisonedSound` SFX (a short structural creak/thud) — positional, near the building. **Audible.**
   - `EVA_StructureGarrisoned` voice cue — emitted but silenced by engine-wide EVA disable. **Inaudible until EVA is re-enabled.**
   - Pip overlay should show 1 of 5 filled. **Visible.**
   - Building owner changes to Conscript's house (color flip). **Visible.**

2. **Non-first-occupant silence:** Without selling/destroying, send 4 more conscripts into the same building one at a time. For each subsequent entry:
   - **No `BuildingGarrisonedSound` SFX** (audibly verifiable: silence after the first conscript).
   - No EVA cue (already silenced engine-wide; cannot distinguish from the muted state, but unit tests cover this — see `test_second_occupant_emits_no_garrison_event`).
   - Pip count increments. **Visible.**

3. **Last-occupant abandon cue:** Order the building to deploy (sell/unload). Each conscript walks out. For occupants 5, 4, 3, 2 leaving: silence. When the **last** conscript leaves:
   - `EVA_StructureAbandoned` voice cue — emitted but silenced engine-wide. **Inaudible until EVA is re-enabled.**
   - Building reverts to neutral (color/owner change). **Visible.**

4. **Owner gating (multi-player):** If running a skirmish vs AI, garrison a building with the AI's units. Confirm **no `BuildingGarrisonedSound` SFX** plays for the local player when an AI-owned building gets garrisoned (the local-player gate at the dispatch layer should suppress it). The visible side-effects (pip count, ownership) will still happen for the AI but are not audibly visible to the local player.

**Step 4: gamemd cross-check**

Run gamemd.exe on the same map. Garrison the same building with the same unit. Verify:
- Same EVA cadence (one cue on first entry, one on full empty-out).
- Same SFX (BuildingGarrisoned SFX file at building location).
- No drift in cue timing.

**Step 5: Sound IDs sanity check**

If the EVA cue plays a different voice than gamemd (wrong faction or wrong file), inspect `state.eva_registry`. The fallback `"ceva107"`/`"ceva108"` should only fire if the registry didn't load `evamd.ini`. Check the registry contents at runtime by adding a temporary `log::info!("eva get garrisoned/{}: {:?}", faction, ...)` to the dispatch arm if needed (remove before merging).

**Step 6: Commit verification notes**

If any divergence from gamemd is observed (e.g. wrong sound for a specific faction), document it as a follow-up issue — the plan's parity guard column above is the contract for what's covered. Don't fix mid-verification; capture and reassess.

If all checks pass:

```
git tag -a garrison-sound-implemented -m "Garrison sound + EVA cues match gamemd reference"
```

(Tag is optional; matches the project's existing convention of milestone tagging if used.)

---

## Sources & References

- **Design doc:** [docs/plans/2026-05-04-garrison-sound-design.md](2026-05-04-garrison-sound-design.md)
- **Disparity scan:** [docs/gap-scans/2026-05-04-disparity-scan-garrison.md](../gap-scans/2026-05-04-disparity-scan-garrison.md) (G2 = this work)
- **Verified Ghidra reports:**
  - [ra2-rust-game-docs/GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) (audited 2026-05-04 — YELLOW status; the four wrong labels do not affect this work)
  - [ra2-rust-game-docs/GARRISON_OCCUPANT_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_OCCUPANT_SYSTEM_GHIDRA_REPORT.md)
  - [ra2-rust-game-docs/GARRISON_IMPLEMENTATION_PLAN.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_IMPLEMENTATION_PLAN.md) §10 (status table is partly stale; the three cues listed there match this implementation)
- **gamemd.exe addresses (verified live 2026-05-04):**
  - `BuildingClass::AddGarrisonOccupant` at 0x00522910 — first-occupant EVA + SFX trigger
  - `BuildingClass::CheckAutoSellOrCivilian` at 0x00458200 — last-occupant EVA trigger (xref from string at 0x004582d3)
  - `RulesClass::ReadAudioVisual` at 0x00669bf6 — parses `BuildingGarrisonedSound` from `[AudioVisual]`
  - String addresses: `EVA_StructureGarrisoned` at 0x008255b0, `EVA_StructureAbandoned` at 0x0081926c, `BuildingGarrisonedSound` at 0x0083a8fc
- **INI keys:**
  - `ini/rulesmd.ini:614` — `[AudioVisual] BuildingGarrisonedSound=BuildingGarrisoned`
  - `ini/evamd.ini:1417-1431` — `[EVA_StructureGarrisoned]` and `[EVA_StructureAbandoned]` sections with per-faction defaults
- **Related code (patterns mirrored):**
  - [src/sim/world/mod.rs:104-107](../../src/sim/world/mod.rs#L104-L107) — `BuildingComplete { owner }` (sim event shape)
  - [src/app_sim_tick.rs:315-333](../../src/app_sim_tick.rs#L315-L333) — `BuildingComplete` dispatch (local-player gate + EVA registry lookup pattern)
  - [src/rules/ruleset.rs:580-615](../../src/rules/ruleset.rs#L580-L615) — `[AudioVisual]` parsing in `GeneralRules::from_ini`
  - [src/sim/miner/miner_tests.rs:248-280](../../src/sim/miner/miner_tests.rs#L248-L280) — Simulation-level test fixture pattern
