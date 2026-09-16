# Save/Load LogicClass Active-Object Vector Reconstruction — Ghidra Research Report

**Address(es):** Save_Game `FUN_0067d300`; Load_Game `FUN_0067e730`; Clear_Scene `FUN_006851f0`;
`DynamicVectorClass::Save @ 0x00551B20`; `DynamicVectorClass::Load @ 0x00551B90`; LogicClass singleton
`0x0087F778`; swizzle-register helper `FUN_006cf240` (remap DB `DAT_00b0c110`); `ObjectClass::Save @ 0x005F6250`;
`ObjectClass::Load @ 0x005F5E80`; ObjectClass load/vtables-only constructor `0x005F3B50`; adder `FUN_0055BAA0`.
**Investigation Mode:** exhaustive-slice (rebuild-owner + restored-order question). One sub-question
(`+0x98` post-load state) is explicitly deferred.
**Claimed Scope:** what rebuilds the LogicClass active-object vector after a savegame load, by what
mechanism, and in what order. Closes OQ-AOOS-011 / OQ-LCPU-016 / synthesis claim 13.
**Non-Scope:** full save/load stream architecture, every `*.Save`/`*.Load` override, pointer-remap apply
internals, replay/network resync, class-specific load constructors beyond ObjectClass base.
**Confidence:** HIGH for the rebuild owner, mechanism, and restored order (live-decompiled this session).
**Active in YR:** Yes — standard savegame load path; `FUN_0067e730` calls `FUN_006851f0` then the heap/object
loads then `Logic.Load`.

## 1. Overview

The LogicClass active-object vector is **not** rebuilt by re-revealing objects after a savegame load. It
is **serialized directly as a `DynamicVectorClass`**: Save_Game writes the vector's count + each raw element
pointer; Load_Game first clears the vector (Clear_Scene), loads the object heaps, then reads the count + raw
pointers back and tail-appends them, registering each restored slot for pointer-swizzle remap. The restored
active order is therefore **byte-for-byte the saved live order** (the reveal-timing order at the instant of
save) — no re-derivation, no sort, no per-object reveal/register call.

## 2. Key Offsets / Addresses

| Symbol | Meaning | Evidence |
|---|---|---|
| LogicClass singleton `0x0087F778` | the active-object `DynamicVectorClass` (vtable `0x007E18FC`; items `+0x04`, capacity `+0x08`, count `+0x10`) | `read_memory 0x007E18FC`; xrefs to `0x0087F778` |
| `DynamicVectorClass::Save @ 0x00551B20` | writes `[count]` then each element pointer (4 bytes each, raw) | `decompile_function 0x00551B20` |
| `DynamicVectorClass::Load @ 0x00551B90` | reads `[count]`, tail-appends each raw pointer, then swizzle-registers every slot | `decompile_function 0x00551B90` |
| Logic vtable slot `+0x0C` (`0x0040CC70`) | vector reset/clear, called by Clear_Scene as `(*(LogicVtable+0xC))()` | `read_memory 0x007E18FC` slot 3; `FUN_006851f0` |
| Logic vtable slot `+0x1C` (`0x0055BAA0`) | the adder (registration) — confirms registration is a vector method | `read_memory 0x007E18FC` slot 7 |
| `FUN_006cf240` / `DAT_00b0c110` | pointer-remap (swizzle) registration DB | `decompile_function 0x00551B90`, `0x005F5E80` |
| `ObjectClass+0x98` | logic-membership byte — **not** saved/restored (see §3.4) | `decompile_function 0x005F6250`; `search_byte_patterns 88 86 98 00 00 00` |

## 3. Core Logic

### 3.1 Save (`FUN_0067d300`, `MOV ECX,0x87F778; CALL 0x00551B20`)

`DynamicVectorClass::Save(stream)` (`0x00551B20`):
- Writes the active count (`*(this+0x10)`) as 4 bytes.
- Loops `count` times, writing each element pointer `*(this+0x04) + i*4` as 4 raw bytes.
- No per-element vtable call — raw 32-bit object pointers are written verbatim (to be swizzled on load).
- Evidence: `get_assembly_context 0x0067d435` (`MOV ECX,0x87f778; PUSH ESI; CALL 0x00551b20`); `decompile_function 0x00551B20`.

### 3.2 Clear (Load_Game → Clear_Scene `FUN_006851f0`)

Load_Game (`FUN_0067e730`) begins with `FUN_006851f0()`. Clear_Scene resets world state and, under the
`"Logic: Init"` heap-pool label, calls `(**(code**)(DAT_0087f778 + 0xc))()` — the Logic vector reset
(vtable slot `+0x0C`). It also deletes all `g_ObjectClass_Array` objects and clears `g_CurrentObjects`.
So the active vector is **empty** before the load stream is read. Evidence: `decompile_function 0x006851f0`.

### 3.3 Load (`FUN_0067e730`, `MOV ECX,0x87F778; CALL 0x00551B90`)

`DynamicVectorClass::Load(stream)` (`0x00551B90`):
1. Reads the saved count (4 bytes).
2. Loops `count` times: reads a 4-byte pointer into a local, then tail-appends it to the vector using the
   same grow-check the runtime uses (`this[4]` count, `this[1]` items, `this[2]` capacity, grow via `*this+8`).
   Order is preserved (forward append in saved order).
3. **Second loop:** for each restored element, calls `FUN_006cf240(&DAT_00b0c110, items + i*4)` — registers
   the slot in the pointer-remap DB so the raw saved pointer is fixed up to the new post-load address.

So the vector membership and order are restored wholesale and the element pointers are swizzled. Evidence:
`get_assembly_context 0x0067e8d2`; `decompile_function 0x00551B90`.

### 3.4 The `+0x98` membership byte is NOT round-tripped (tiny detail, load-bearing)

- `ObjectClass::Save @ 0x005F6250` serializes `+0x74`, `+0x80`/`+0x83` (only when `g_GameMode==0||5`),
  `+0x81` (InLimbo), `+0x84`, `+0x8C`, `+0x8D`, `+0x8F`, `+0x90` (alive), and coords `+0x9C/+0xA0/+0xA4`.
  **`+0x98` is absent.** (Re-verified live; confirms the prior negative claim.)
- `ObjectClass::Load @ 0x005F5E80` swizzle-registers `+0x30/+0x34/+0x38/+0x18/+0x88`, inits two VocHandles,
  clears `+0xA8`. It does **not** set `+0x98` and does **not** call Reveal/register.
- The ObjectClass load/vtables-only constructor `0x005F3B50` sets the 4 vtable pointers only.
- `DynamicVectorClass::Load` is a generic container — it restores pointers, not object-specific bytes.
- `search_byte_patterns` for the **EAX-base** immediate write `C6 80 98 00 00 00 01` → **no matches** —
  so there is no immediate-form writer of an **ObjectClass** `+0x98` membership byte. The **ESI-base**
  immediate form `C6 86 98 00 00 00 01` does match **twice** (`0x006396a5`, `0x0063977b`, both inside
  `FUN_00639740`), but those writes target a **different struct** whose base comes from `FUN_00705d20()`
  and set the `+0x90/+0x94/+0x98` triplet together as span/length fields
  (`+0x94 = iVar3`, `+0x98 = 1`, `+0x90 = *(base+0x14) - iVar3`) — **not** the ObjectClass logic-membership
  byte. The substantive conclusion stands: no path writes ObjectClass `+0x98` on load. The register-form
  `88 86 98 00 00 00` (`MOV [ESI+0x98],AL`) matches **only** the adder at `0x0055bac6`.
  (Corrected 2026-05-29: re-confirmed via `search_byte_patterns C6 80 98 00 00 00 01` → no matches,
  `search_byte_patterns C6 86 98 00 00 00 01` → `0x006396a5`/`0x0063977b`, `decompile_function 0x00639740`
  showing the `FUN_00705d20()`-based span triplet.)

**Consequence / hazard:** after a savegame load, the object is present in the restored Logic vector, but its
`+0x98` cache is not written by any path traced here. If it ends up `0`, a later `Conceal`→remover
(`FUN_0055BAE0`) early-outs (it gates on `+0x98 != 0`) and leaves a stale pointer, and a later `Reveal`→adder
would double-append. Whether gamemd avoids this via a most-derived load constructor that sets `+0x98`, a
post-remap finalization pass, or zero-alloc + some other fixup is **not resolved here** (see §7, deferred).

## 4. INI Keys

None. No INI key controls savegame active-vector reconstruction.

## 5. Integration Points

- **Save_Game `FUN_0067d300`** → `Logic.Save` (and saves AnimTypes, Tubes via OleSaveToStream, plus the
  `0x00887324` object).
- **Load_Game `FUN_0067e730`** → `Clear_Scene` (`FUN_006851f0`) → heap/object loads (OleLoadFromStream) →
  `Map`-singleton load (`ECX=0x87F7E8; CALL 0x00581F50`) → `Logic.Load` → release of the `0x00887324` object.
- **Pointer-remap DB `DAT_00b0c110`** consumes the swizzle registrations from `Logic.Load` and `ObjectClass::Load`.
- This is the savegame path only; fresh scenario start uses `ScenarioClass::Full_Init` reveal-timing
  (see [ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md)).

## 6. Current Rust Implementation Status

| Rust surface | Status | Note |
|---|---|---|
| `src/sim/world/mod.rs::live_object_order_snapshot` (`:745`) | OK | Returns `self.logic.snapshot()` verbatim — the active order with no sort. The prior sorted-stable-ID fallback is gone (comment: "No sorted-ID fallback (was DRIFT)"). `for_each_live_object` (`:763`) implements the native count-reload + compacting-removal skip-successor contract. (Corrected 2026-05-29: re-confirmed via Read `src/sim/world/mod.rs:740-770`.) |
| Rust savegame serialization of `live_object_order` | unchecked | no Rust save/load of the active order found; must serialize the order vector directly, not re-derive |
| membership flag on load | unchecked | Rust must set membership consistent with restored vector presence (avoid the §3.4 hazard regardless of what gamemd does) |

## 7. Open Questions — Final State

- `[RESOLVED] OQ-SL-001 — Who rebuilds the Logic active vector after savegame load? → DynamicVectorClass::Load @ 0x00551B90, called by Load_Game FUN_0067e730 with ECX=0x87F778.` (evidence: `get_assembly_context 0x0067e8d2`; `decompile_function 0x00551B90`)
- `[RESOLVED] OQ-SL-002 — Is it rebuilt by re-revealing objects? → No; direct DynamicVector serialization, not per-object Reveal/register.` (evidence: adder callers list has no load path; `0x00551B90` appends raw pointers)
- `[RESOLVED] OQ-SL-003 — Is the vector cleared before load? → Yes; Clear_Scene FUN_006851f0 calls Logic vtable+0xC reset under the "Logic: Init" label.` (evidence: `decompile_function 0x006851f0`)
- `[RESOLVED] OQ-SL-004 — What restored order results? → The saved live order, preserved (forward append of saved pointers), then swizzled.` (evidence: `0x00551B20` writes items[0..count] in order; `0x00551B90` appends in read order)
- `[RESOLVED] OQ-SL-005 — How are saved raw pointers fixed up? → Each restored slot is registered in the remap DB DAT_00b0c110 via FUN_006cf240.` (evidence: `decompile_function 0x00551B90` second loop)
- `[RESOLVED] OQ-SL-006 — Does Save serialize +0x98? → No.` (evidence: `decompile_function 0x005F6250`; `search_byte_patterns`)
- `[DEFERRED] OQ-SL-007 — What sets ObjectClass+0x98 after load (if anything), so future Conceal/Reveal stay consistent?` (category: `bounded-cost-too-high`; reason: not in Save/Logic.Load/ObjectClass::Load/load-ctor; the register-form sweep across all base/src combos and the most-derived techno load constructors were not exhausted; next-step-if-pursued: decompile UnitClass/BuildingClass/FootClass/InfantryClass load constructors for a `+0x98=1` write, and scan the post-remap finalization pass; or runtime-observe `+0x98` of a known logic member immediately after load.)
- `[DEFERRED] OQ-SL-008 — What happens to a vector element whose object fails to load / swizzles to null?` (category: `requires-different-system-context`; reason: belongs to the remap-apply pass; next-step-if-pursued: trace the DAT_00b0c110 apply and any null-compaction.)

## 8. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected surface | Required effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Active vector is serialized directly (count + ordered pointers) and restored in saved order, not re-derived. | `0x00551B20`, `0x00551B90`, `0x0067e8d2` | Snapshot fixed: `live_object_order_snapshot` (`:745`) now returns `logic.snapshot()` verbatim (no sorted-ID fallback; corrected 2026-05-29). Remaining gap: no Rust **save/load** serialization of the active order found — direct order serialization is still unchecked. | `src/sim/world/mod.rs` (`live_object_order_snapshot`, save/load) | Serialize `live_object_order` as an ordered list; restore in the same order; never sort or re-derive from reveal on load. | Save with active order [B,A,C] (creation IDs differ from order); after load, order is exactly [B,A,C]. Test: `saveload_restores_live_object_order_verbatim`. | Do not rebuild active order by re-revealing or by sorting stable IDs. |
| Vector is cleared before the load stream is applied. | `0x006851f0` | unchecked | Rust load reset path | Clear the active order on load before repopulating. | Load over a populated session leaves no pre-load members. Test: `saveload_clears_active_order_before_restore`. | Do not append restored members onto a non-empty live order. |
| Membership cache `+0x98` is not serialized; vector presence is authoritative. | `0x005F6250`, `0x005F5E80` | unchecked | Rust membership flag, if any | Derive membership-true from restored vector presence so remove/re-add stay correct. | After load, dying a restored unit removes it from the active order exactly once (no stale entry, no double-add). Test: `saveload_restored_member_removes_cleanly`. | Do not assume a separate persisted membership flag; do not leave membership unset for restored members. |

### Stale Docs / Follow-up

- [ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md) OQ-AOOS-011 ("rebuild owner unknown") is now
  RESOLVED: the owner is `DynamicVectorClass::Load @ 0x00551B90` via Load_Game `FUN_0067e730`. The post-load
  `+0x98` setter remains the only open sub-question.
- [LOGICCLASS_OBJECT_LIFECYCLE_SPINE_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_OBJECT_LIFECYCLE_SPINE_SYSTEM_MODEL_SYNTHESIS.md) claim 13 should be upgraded from
  NEEDS_REINVESTIGATE to IMPLEMENTATION_SAFE for the rebuild mechanism and order; keep the `+0x98` post-load
  state as the lone deferred item.

## Sources

- Live Ghidra (this session): `decompile_function` `0x006851f0`, `0x0067d300` (preview), `0x0067e730` (preview),
  `0x00551B20`, `0x00551B90`, `0x005F5E80`, `0x005F6250`, `0x005F3B50`, `0x0055BAA0` (disasm);
  `get_assembly_context 0x0067d435,0x0067e8d2`; `read_memory 0x007E18FC`; `get_xrefs_to 0x0087F778`;
  `get_function_callers 0x0055BAA0`; `search_byte_patterns 88 86 98 00 00 00`,
  `C6 80 98 00 00 00 01` (no matches), `C6 86 98 00 00 00 01` (`0x006396a5`/`0x0063977b`, both in
  `FUN_00639740` — a different struct's span triplet, re-confirmed 2026-05-29).
- Prior docs: [ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md),
  [LOGICCLASS_OBJECT_LIFECYCLE_SPINE_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_OBJECT_LIFECYCLE_SPINE_SYSTEM_MODEL_SYNTHESIS.md), [LOGICCLASS_VS_MAPCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_VS_MAPCLASS_GHIDRA_REPORT.md).
- Rust: `src/sim/world/mod.rs`.
