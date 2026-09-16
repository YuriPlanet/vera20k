# AnimClass Detached-Owner Marker +0x19B Consumers - Ghidra Research Report

**Address(es):** `AnimClass::Constructor @ 0x00421EA0`, load constructor `0x00422720`, `AnimClass::AI @ 0x00423AC0`, `AnimClass::Detach @ 0x00425150`, `AnimClass::ProcessCloakMode/Mark bridge @ 0x004238B0`, `AnimClass::SaveExtras @ 0x004254A0`, `AnimClass::DrawIt @ 0x00422CA0`
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** writers/readers of `AnimClass+0x19B` relevant to owner-expiry detach, plus whether the marker makes detached owner-attached anims continue, hide, or destroy.
**Non-Scope:** full `SetOwnerObject` lifecycle, temporal SQDG listener lifecycle, building 21-slot refresh, complete `AnimClass::AI` side-effect taxonomy, and exact audio mixer output.
**Confidence:** High for `+0x19B` writer/reader sites in `AnimClass`; Medium for proving absence outside `AnimClass` because byte-pattern hits outside the class include unrelated offsets/constants.
**Active in YR:** Yes. The `AnimClass::Detach` owner-expiry path is reached through active `ObjectClass::UnInit -> Detach_From_All_Lists`, and `AnimClass::AI` is the active per-tick path for revealed logic-enabled anims.

## Working Notes Gate

Target question: Verify what `AnimClass+0x19B` does after owner-expiry detach and whether detached owner-attached anims continue, hide, or are destroyed.
Non-goals: Do not redo `SetOwnerObject`, temporal SQDG, building active slots, or all `AnimClass::AI` semantics except where they consume `+0x19B`.
Evidence needed to mark COMPLETE: decompile plus disassembly/range proof for `AnimClass::Detach`, `AnimClass::AI`, constructor/init, `Next=` reset, save/read surface, and `DrawIt` non-consumption.
Stop conditions: stop after all observed `AnimClass+0x19B` reads/writes are classified and Rust handoff/test risks are concrete.

## 1. Overview

`AnimClass+0x19B` is best modeled as a native inactive/pending-destroy byte consumed by `AnimClass::AI`, not as the direct render hide flag. Owner-expiry detach sets it after clearing `AnimClass+0xCC`, removes the anim from the display layer, calls the owner cleanup callback, and marks the anim; on a later AI visit, the byte skips trailer spawning and routes the object to `AnimClass::Destroy`.

The detached anim does not become a normal ownerless free visual. It is removed from layer immediately, keeps its stored coordinate as-is rather than converting the owner-relative offset back to absolute, then is destroyed on its next AI pass after some top-of-AI maintenance code has already had a chance to run.

## 2. Key Offsets

| Offset | Type | Meaning in this slice | Active in YR | Evidence |
|---|---|---|---|---|
| `AnimClass+0x19B` | byte | inactive/pending-destroy marker tested by `AnimClass::AI`; set by owner-expiry detach and some AI validity checks; cleared by constructor and `Next=` | Yes / Conditional | `0x00421EA0`, `0x00423AC0`, `0x00425150` |
| `AnimClass+0xCC` | pointer | attached owner pointer; owner-expiry detach clears this before setting `+0x19B` | Conditional on owner-attached anims | `0x00425150` |
| `AnimClass+0x19C` | byte | first-AI guard; separate from `+0x19B` and serialized adjacent to it | Yes | `0x00423AC0`, `0x004254A0` |
| `AnimClass+0x19D` | byte | draw-hidden/visibility suppression consumed by `DrawIt`; separate from `+0x19B` | Conditional | `0x00423AC0`, `0x00422CA0` |

## 3. Core Findings

1. Constructor and load constructor clear `+0x19B`.
   Active in YR: Yes. Fresh runtime construction writes `0` to `AnimClass+0x19B` during `AnimClass::Constructor @ 0x00421EA0`; the load constructor at `0x00422720` also initializes it to `0` before saved state resolution. Evidence: decompile plus byte-pattern hits `0x00422005` and `0x00422830`; disassembly ranges `0x00421FE0..0x00422030`, `0x00422810..0x00422850`.

2. Owner-expiry detach sets `+0x19B=1`, but explicit detach through `SetOwnerObject(NULL)` does not.
   Active in YR: Conditional on an attached owner expiring. `AnimClass::Detach @ 0x00425150` removes the anim from display, calls old owner vtable `+0x60(this)`, clears `AnimClass+0xCC`, writes `+0x19B=1`, then calls vtable `+0x124(0)`, which resolves through `AnimClass::ProcessCloakMode @ 0x004238B0` to `ObjectClass::Mark(0)`. Evidence: decompile `0x00425150`, `0x004238B0`; disassembly range `0x00425170..0x004251B0`; byte-pattern hit `0x00425198`.

3. `+0x19B=1` suppresses trailer spawning and then destroys the anim in `AnimClass::AI`.
   Active in YR: Yes for any active anim reaching `AI`. The trailer block requires `AnimClass+0x90 != 0` and `+0x19B == 0`; if `+0x19B != 0`, the later `if` jumps to `LAB_00424B38`, which calls vtable `+0xF8` (`AnimClass::Destroy`) and returns. Evidence: decompile `AnimClass::AI @ 0x00423AC0`; disassembly range `0x00424290..0x00424450` for trailer/read/destroy gate and range `0x00424B30..0x00424B40` for destroy tail; byte-pattern hits `0x004242B2`, `0x00424361`.

4. `+0x19B=1` is not a total "skip all AI" flag.
   Active in YR: Yes. The inactive check occurs after top-of-AI looping sound maintenance, bouncer processing, several visibility/validity updates, the `Type+0x34C` call, and a frame equality cleanup of `+0x11B`. Therefore a detached anim can still execute pre-check maintenance or special bouncer/visibility side effects once before destruction. Evidence: `AnimClass::AI @ 0x00423AC0` ordering before the `+0x19B` check; disassembly range `0x00423AC0..0x00424370`.

5. `+0x19B=1` is not directly consumed by `AnimClass::DrawIt`.
   Active in YR: Yes. `DrawIt @ 0x00422CA0` checks `+0x19D`, `+0x199`, translucency bytes, extra-animation limits, visibility, and type flags, but the decompiled body does not read `+0x19B`. Owner-expiry detach hides by `DisplayClass::RemoveFromLayer` before the next draw, not by a `DrawIt` branch on `+0x19B`. Evidence: decompile `0x00422CA0`; `+0x19D` read appears in `DrawIt`, while `+0x19B` byte-pattern hits are absent from the `0x00422CA0` body.

6. `Next=` clears `+0x19B`.
   Active in YR: Conditional on loop exhaustion with `AnimType.Next != null`. The same `AnimClass` object writes the new type pointer, recalculates playback fields, writes `+0x19B=0`, loads the new loop byte, resets timers/damage, calls `Middle()`, and returns. Evidence: `AnimClass::AI @ 0x004247F3..0x00424932`; disassembly range `0x00424840..0x00424890`; byte-pattern hit `0x0042486C`.

7. Other active `AnimClass::AI` validity checks can set `+0x19B=1`.
   Active in YR: Conditional. The `RulesClass+0x147C` special anim sets `+0x19B=1` if a building exists in its cell; the `AnimType+0x360` overlay-bound path sets `+0x19B=1` if the cell overlay is absent or no longer matches the anim type. These writes feed the same destroy tail as owner-expiry detach. Evidence: decompile `0x00423AC0`; byte-pattern hits `0x0042435A`, `0x00424429`.

8. `+0x19B` is saved.
   Active in YR: Conditional on save/load. `AnimClass::SaveExtras @ 0x004254A0` serializes byte `+0x19B` between `+0x197` and `+0x19C`. This means a saved inactive anim can preserve the pending-destroy marker rather than reinitializing to active. Evidence: decompile `0x004254A0`; disassembly range `0x004254C0..0x00425500`; byte-pattern hit `0x004254DF`.

## 4. Post-Detach Lifetime Semantics

Owner-expiry detach does not call `SetOwnerObject(NULL)`, so it does not convert the stored owner-relative coordinate back to absolute world coordinates. It clears `+0xCC` directly. If any pre-destroy logic calls `GetCoords` before `AnimClass::Destroy`, it now sees the stored relative offset as an ownerless coordinate. That matters most for top-of-AI looping sound maintenance and any special bouncer/visibility code reached before the inactive check.

The normal visual result is that the anim is removed from display immediately by `DisplayClass::RemoveFromLayer` in `AnimClass::Detach`, then destroyed on its next AI visit. It is not supposed to continue as a visible ownerless world animation after its owner expires.

## 5. Current Rust Status

Rust currently has no `+0x19B` analogue:

- `src/sim/components.rs` has `AnimClassSpawnDescriptor`, `AnimRuntime`, and `WorldEffect`, but no owner-expiry inactive byte, owner pointer, display layer membership, or pointer-expiry detach event.
- `src/app_chute_anim.rs` removes parachute visuals by polling entity/descent state. Native owner death would call `AnimClass::Detach`, set `+0x19B`, mark, remove from layer, then destroy on AI.
- `src/sim/movement/teleport_movement.rs` free `WarpOut` rows are not owner-attached and do not need this marker, but temporal/parachute/BEHIND-style attached anims do.

## 6. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Constructor initialization of `+0x19B` | verified | `0x00421EA0`, `0x00422720` | none |
| Owner-expiry writer | verified | `AnimClass::Detach @ 0x00425150` | none |
| Mark call after writer | verified | `0x00425150`, `0x004238B0` | internal `ObjectClass::Mark` dirtiness details out-of-scope |
| AI trailer gate | verified | `0x00423AC0`, `0x00424290..0x00424322` | none |
| AI destroy gate | verified | `0x00423AC0`, `0x00424361`, `0x00424B38` | exact scheduler same-pass ordering covered by separate docs |
| AI pre-check side effects | verified for ordering | `0x00423AC0..0x00424370` | exact audio mixer output deferred |
| DrawIt direct consumption | verified absent in scoped body | `0x00422CA0` | other render callers out-of-scope |
| Next reset | verified | `0x004247F3..0x00424932` | none |
| Save serialization | verified | `0x004254A0` | load deserialization internals inherited from object save system not exhausted |

## 7. Open Questions - Final State

- `[RESOLVED] OQ-01 - Who writes +0x19B after owner expiry? -> AnimClass::Detach writes it after clearing +0xCC and before Mark(0).` (evidence: `0x00425150`)
- `[RESOLVED] OQ-02 - Does SetOwnerObject(NULL) write +0x19B? -> No; explicit detach path is separate and owner-expiry detach is the writer.` (evidence: `0x00424B50` prior report, `0x00425150`)
- `[RESOLVED] OQ-03 - Does +0x19B directly hide DrawIt? -> No direct DrawIt read was found; RemoveFromLayer hides the owner-expired anim.` (evidence: `0x00422CA0`, `0x00425150`)
- `[RESOLVED] OQ-04 - Does +0x19B suppress trailers? -> Yes, trailer spawn requires +0x19B == 0.` (evidence: `0x00423AC0`, `0x004242B2`)
- `[RESOLVED] OQ-05 - Does +0x19B destroy the anim? -> Yes, AI branches to vtable +0xF8 destroy when +0x19B != 0.` (evidence: `0x00424361`, `0x00424B38`)
- `[RESOLVED] OQ-06 - Can any AI work happen before the destroy gate? -> Yes, top-of-AI sound/bouncer/visibility/special-type work occurs before the +0x19B gate.` (evidence: `0x00423AC0..0x00424370`)
- `[RESOLVED] OQ-07 - Does Next clear +0x19B? -> Yes, the in-place Next transition writes +0x19B=0 before Middle().` (evidence: `0x0042486C`)
- `[RESOLVED] OQ-08 - Is +0x19B serialized? -> Yes, SaveExtras writes it between +0x197 and +0x19C.` (evidence: `0x004254A0`)
- `[DEFERRED] OQ-09 - Exact audible output if a detached inactive anim has looping sound before destroy.` (category: requires-different-system-context; reason: requires sound-event/mixer runtime trace; next-step-if-pursued: audio-specific trace of `UpdateLoopingSound` for inactive detached anim)
- `[DEFERRED] OQ-10 - Full object save/load field reader that restores +0x19B.` (category: bounded-cost-too-high; reason: save serialization is proven, but generic object deserializer internals are outside this slice; next-step-if-pursued: save/load field order report)

## 8. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Owner-expiry detach removes the anim from layer, clears owner, sets inactive `+0x19B=1`, then marks; next AI destroys it. | `0x00425150`, `0x004238B0`, `0x00423AC0` | Missing. Rust owner-bound visuals are polling/render bridges. | future generic anim runtime; `src/sim/components.rs`; `src/app_chute_anim.rs` | Add a detach-event marker distinct from normal explicit detach; remove from render membership immediately and route to destroy on next anim AI. | Owner dies with attached `PARACH`; chute is removed from draw membership and native-equivalent anim object self-destroys on next AI. Test: `attached_anim_owner_expiry_sets_inactive_and_destroys_next_ai`. | Do not let detached attached anims continue as visible ownerless world effects. |
| `+0x19B` does not skip all AI; pre-check maintenance may run before destruction. | `0x00423AC0..0x00424370` | Missing/unchecked; no generic AI ordering exists. | future anim scheduler/runtime | Preserve top-of-AI ordering if implementing sound/bouncer/visibility fields before inactive destruction. | Inactive detached anim with StartSound/Report field still executes the native pre-destroy sound-maintenance slot before cleanup. Test: `inactive_anim_runs_pre_destroy_ai_prefix_before_destroy`. | Do not place the inactive check at the very top of AI unless every pre-check side effect is proven irrelevant. |
| `+0x19B` gates trailer spawn and is cleared by in-place `Next=`. | `0x004242B2`, `0x004247F3..0x00424932` | Missing from descriptor/WorldEffect bridge. | generic AnimClass-like runtime; trailer/Next implementation | Track inactive separately from loop count so inactive anims do not spawn trailers, and `Next=` can reactivate the same object by clearing the byte. | An inactive anim with `TrailerAnim` spawns no trailer; an exhausted active anim transitioning to `Next` clears inactive and calls `Middle` on same object. Tests: `inactive_anim_does_not_spawn_trailer`, `anim_next_clears_inactive_marker_in_place`. | Do not model `Next=` as destroy/spawn or leave stale inactive state on the new type. |

## 9. Negative Facts / Do Not Do

- Do not call `+0x19B` a direct draw suppression flag; `DrawIt` does not read it in the scoped body.
- Do not make owner-expired attached anims continue as ordinary ownerless world animations.
- Do not implement inactive as "skip all AI from instruction zero"; native checks it after several top-of-AI operations.
- Do not merge `+0x19B` with `+0x19C` first-AI guard or `+0x19D` draw-hidden flag.
- Do not forget that `Next=` clears `+0x19B` on the same object.

## 10. Stale Docs / Replacement Wording

- [docs/research/ANIM_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIM_CLASS_GHIDRA_REPORT.md): replace "`+0x19B IsInactive: suppresses drawing and AI`" with "`+0x19B` is an AI-consumed inactive/pending-destroy byte. `AnimClass::AI` uses it to suppress trailer spawn and route the anim to `Destroy`; it is checked after top-of-AI maintenance, not at function entry. `AnimClass::DrawIt` does not directly test `+0x19B`; owner-expiry detach hides by `RemoveFromLayer` and then AI destroys the anim."
- [docs/research/ANIMCLASS_ATTACHEDOWNER_DETACH_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_ATTACHEDOWNER_DETACH_LIFECYCLE_GHIDRA_REPORT.md): replace "may let the anim continue ownerless depending on its state" with "sets `+0x19B=1`; the anim is removed from display immediately and `AnimClass::AI` destroys it on its next AI visit after native pre-check maintenance."
- [docs/research/ANIMCLASS_CHAINING_DAMAGE_OWNERSHIP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_CHAINING_DAMAGE_OWNERSHIP.md): replace any claim that trailers are the only visible effect of `+0x19B` with "`+0x19B` skips trailer creation and then takes the AI destroy tail; `Next=` clears it when reusing the same object."

## Sources

- Ghidra read-only decompile: `AnimClass::Constructor @ 0x00421EA0`, load constructor `0x00422720`, `AnimClass::AI @ 0x00423AC0`, `AnimClass::DrawIt @ 0x00422CA0`, `AnimClass::Detach @ 0x00425150`, `AnimClass::ProcessCloakMode @ 0x004238B0`, `AnimClass::SaveExtras @ 0x004254A0`, `AnimClass::Destroy @ 0x004255B0`.
- Ghidra byte-pattern/disassembly ranges: `0x00421FE0..0x00422030`, `0x00422810..0x00422850`, `0x00424290..0x00424450`, `0x00424840..0x00424890`, `0x00425170..0x004251B0`, `0x004254C0..0x00425500`.
- Research-index brief: `AnimClass 0x19B detached owner marker consumers`.
- Docs referenced: [ANIMCLASS_ATTACHEDOWNER_DETACH_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_ATTACHEDOWNER_DETACH_LIFECYCLE_GHIDRA_REPORT.md), [ANIM_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIM_CLASS_GHIDRA_REPORT.md), [ANIMCLASS_AI_TRAILER_NEXT_INTERACTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_TRAILER_NEXT_INTERACTION_GHIDRA_REPORT.md), [ANIMCLASS_CONSTRUCTOR_MIDDLE_SOUND_TIMING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_CONSTRUCTOR_MIDDLE_SOUND_TIMING_GHIDRA_REPORT.md).
- Rust scanned: `src/sim/components.rs`, `src/app_chute_anim.rs`, `src/sim/movement/teleport_movement.rs`.
