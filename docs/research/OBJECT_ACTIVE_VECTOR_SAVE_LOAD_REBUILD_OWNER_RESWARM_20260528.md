# Object Active Vector Save/Load Rebuild Owner - Reswarm 2026-05-28

**Address(es):** `FUN_0067D300`, `FUN_0067E730`, `FUN_0067E440`, `FUN_00551B20`, `FUN_00551B90`, `FUN_006CF240`, `FUN_006CF230`, `FUN_006CF350`, `FUN_0065AC40`, `AbstractClass::Save @ 0x00410320`, `AbstractClass::Load @ 0x00410380`, `ObjectClass::Load_IStream @ 0x005F5E80`, `ObjectClass__Save @ 0x005F6250`, `FUN_0055BAA0`, replay contrast `Main_Game @ 0x0052D9A0`, `Main_Tick @ 0x0055D360`, scenario contrast `ScenarioClass::Full_Init @ 0x00686B20`
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** who preserves or reconstructs the `LogicClass` active-object vector and `ObjectClass+0x98` membership across standard savegame save/load, replay startup/playback, and scenario load; what order source is used; and which Rust sorted-ID fallbacks are ruled out.
**Non-Scope:** ordinary reveal/register tail-append mechanics, non-`Reveal` `FUN_0055BAA0` callers, destructor/unregister compaction, final runtime watchpoint sampling of every Object-derived class's post-load `+0x98` byte, and exact `FUN_00551A30` active-order key semantics.
**Confidence:** High for vector owner/order, `ObjectClass__Save @ 0x005F6250` omission, IPersist raw-body distinction, and absence of a post-load re-registration owner in the standard wrapper; Medium for final per-class post-load `Object+0x98` byte value because runtime watchpoints were not taken.
**Active in YR:** Yes for standard savegame load/save, standard scenario load, and conditional replay playback (`DAT_00A8D5F8 & 2`).

## 0. Working Notes

Target question: who reconstructs or preserves `LogicClass` active object vector membership (`Object+0x98`) across save/load/replay/scenario load, and what order source is used.

Non-goals: do not re-prove reveal/spawn tail-append or conceal/destructor compaction unless a save/load contradiction appears.

Evidence needed to mark COMPLETE: `ObjectClass::Save/Load` serialization proof, post-load/replay/scenario owner proof, order-source proof, and Rust handoff/test implications.

Stop conditions: all open questions resolved/deferred, no mutable Ghidra actions, only the allowed report plus shared claims file edited.

## 1. Overview

Standard save/load does not rebuild the active object vector from sorted object storage. `FUN_0067D300` saves the `LogicClass` vector directly through `FUN_00551B20` with `ECX=0x87F778`, and `FUN_0067E730` reloads it through `FUN_00551B90` with the same singleton. `FUN_00551B90` appends saved entries in stream order and registers each vector slot for the generic swizzle pass.

There is no post-load owner that re-registers every loaded active object through `FUN_0055BAA0` or reconstructs `Object+0x98` from the vector. `FUN_0067E440` runs `FUN_0067E730`, then `FUN_006CF230 -> FUN_006CF350`, and that pass only patches queued pointer slots from old saved addresses to new object addresses.

Replay is separate from savegame load. Replay startup reads header/seed/scenario fields and starts normal scenario initialization; per-frame replay playback does not call the savegame vector loader or a replay-specific active-list repair. Scenario load order remains the previously verified loader/key order plus successful reveal/register timing.

## 2. Key Offsets / Containers

| Field / container | Meaning | Evidence | Active in YR |
|---|---|---|---|
| `LogicClass+0x04` | active object pointer array | `FUN_00551B20` writes slots; `FUN_00551B90` appends then swizzle-registers slots | Yes |
| `LogicClass+0x10` | active vector count | saved at `0x00551B38..0x00551B4B`, loaded at `0x00551BAA..0x00551BC3`, incremented at `0x00551C12..0x00551C22` | Yes |
| `ObjectClass+0x98` | object-local logic membership byte used by register/remove helpers | `FUN_0055BAA0`; helper report | Yes for ordinary lifecycle; no post-load reconcile owner found |
| `DAT_00B0C110+0x08/+0x14` | swizzle pointer-slot queue | `FUN_006CF240`, `FUN_006CF350` | Yes |
| `DAT_00B0C110+0x20/+0x2C` | old-object to new-object swap map | `AbstractClass::Load`, `FUN_006CF350` | Yes |

## 3. Core Logic

### 3.1 Savegame Active Vector Owner

`FUN_0067D300` owns save stream serialization. It calls the vector saver with the live `LogicClass` singleton:

- assembly context `0x0067D435`: `MOV ECX,0x87f778`;
- `0x0067D43A`: `CALL 0x00551B20`.

`FUN_00551B20` writes `LogicClass+0x10` first, then writes entries from `LogicClass+0x04` in ascending vector index order. Assembly context:

- `0x00551B38..0x00551B4B`: loads `[EBX+0x10]` and stream-writes 4 bytes;
- `0x00551B5C..0x00551B78`: stream-writes `*(LogicClass+0x04 + index*4)` in a forward loop.

Material finding: savegame active order is the current vector order, not sorted `AbstractClass::ID`, object-array order, `EntityStore`-like key order, cell order, or type order. Active in YR: Yes.

### 3.2 Savegame Active Vector Load Owner

`FUN_0067E730` owns content stream loading. After state clear and earlier stream sections, it loads the same singleton:

- assembly context `0x0067E8CD`: `CALL 0x00581F50`;
- `0x0067E8D2`: `MOV ECX,0x87f778`;
- `0x0067E8D8`: `CALL 0x00551B90`.

`FUN_00551B90` reads count, then appends saved pointer tokens in stream order. It does not sort. Assembly/decompile evidence:

- `0x00551BAA..0x00551BC3`: reads a 4-byte saved count;
- `0x00551BE6..0x00551C10`: checks capacity/grow path;
- `0x00551C12..0x00551C22`: writes old-count index, increments `LogicClass+0x10`, stores the stream token;
- `0x00551C2E..0x00551C4C`: registers every vector slot with `FUN_006CF240`.

Material finding: load order source is the saved vector stream order. Active in YR: Yes.

### 3.3 Swizzle Fixup Is Not a Rebuild Owner

`FUN_006CF240` records a nonzero raw pointer token plus the address of the pointer slot, then clears that slot to zero (`0x006CF2A8`). `FUN_006CF350` later sorts the swizzle queues and writes `new_object_pointer` back to the queued slot address. Decompile and assembly `0x006CF350..0x006CF3FF` show pointer-slot writes and queue clears only.

Call evidence:

- `FUN_0067E440` callees include `FUN_0067E730`, `FUN_006CF230`, and post-load refresh helpers; they do not include `FUN_0055BAA0`.
- `FUN_006CF230` callers are `FUN_0067E440` and `ScenarioClass::Full_Init`; it is a thin wrapper over `FUN_006CF350`.
- known `FUN_0055BAA0` callers are `BuildingLightClass__Constructor`, `FUN_00437050`, `FUN_0075F8B0`, `ObjectClass::Reveal`, and `TechnoClass::SetInOpenTransport`; no save/load wrapper or post-load refresh helper is a caller.

Material finding: the standard post-load owner preserves vector membership by swizzling saved vector slots; it does not rebuild membership by walking objects and does not set `Object+0x98`. Active in YR: Yes for the wrapper; No for the imagined post-load re-registration pass.

### 3.4 ObjectClass Save/Load Has Two Surfaces

`ObjectClass__Save @ 0x005F6250` is the CRC/checksum-style save surface. Its decompile writes selected fields through `+0x90`, then `+0x9C/+0xA0/+0xA4`, with no `+0x98` access. Its callers are checksum/extras-style callers (`AnimClass__SaveExtras`, `BuildingLightClass__Save`, `MissionClass__Save`, `TerrainClass__Save`, etc.), not the IPersist ObjectClass stream wrapper. Active in YR: Yes, but not the savegame stream body.

The savegame stream surface is `FUN_0065AC40 -> AbstractClass::Save @ 0x00410320`. `AbstractClass::Save` writes the saved `this` pointer, then writes raw class-sized bytes from `this`, using virtual slot `+0x30` for the byte count. That raw-body write includes bytes in the Object layout, so it includes the saved `+0x98` byte before class load cleanup runs. Active in YR: Yes.

`ObjectClass::Load_IStream @ 0x005F5E80` calls `AbstractClass::Load @ 0x00410380`, then registers pointer slots `+0x30/+0x34/+0x38/+0x18/+0x88`, initializes two `VocHandle`s, and clears `+0xA8`. It does not call `ObjectClass::Reveal`, `FUN_0055BAA0`, or the remover. Active in YR: Yes.

Load-specific constructors can overwrite raw-loaded volatile bytes. Verified examples from prior save/load reports: `AircraftClass::Load @ 0x0041B430` and `BuildingClass::Load @ 0x00453E20` call constructor chains that reach `ObjectClass::Constructor @ 0x005F3900`, which initializes `Object+0x98` to zero. Active in YR: Yes for those loaded class paths.

Material finding: "ObjectClass save/load serializes membership" is surface-dependent. The CRC/checksum `ObjectClass__Save @ 0x005F6250` does not serialize `+0x98`; the IPersist raw-body stream path does initially carry it, but the standard load wrapper still has no later `+0x98` reconciliation pass, and representative derived constructors can reset it.

### 3.5 Replay Restore Contrast

[REPLAY_ACTIVE_VECTOR_RESTORE_CORNER_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/REPLAY_ACTIVE_VECTOR_RESTORE_CORNER_RESWARM_20260528.md) verified replay playback is controlled by `DAT_00A8D5F8 & 2`, not `g_GameMode == 5`. `Main_Game @ 0x0052D9A0` reads replay header fields and launches normal scenario initialization through `ScenarioClass::Read_Scenario @ 0x00684620` and `ScenarioClass::Full_Init @ 0x00686B20`.

Per-frame replay playback in `Main_Tick @ 0x0055D360` reads sync/selection/cursor records, then continues to ordinary late tick helpers and `LogicClass::PerTickUpdate`. It does not call `FUN_00551B90`, `FUN_0055BAA0`, or a replay-specific active-list repair. Active in YR: Conditional on replay playback.

Material finding: replay active-list order is normal scenario-load/reveal order plus runtime lifecycle mutations, not savegame vector restore. Active in YR: Yes, conditional.

### 3.6 Scenario Load Contrast

[ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md) remains valid for scenario load: `ScenarioClass::Full_Init @ 0x00686B20` processes object-bearing sections in fixed order and each reader walks INI keys upward before successful `Unlimbo`/`Reveal` reaches `FUN_0055BAA0`. Active in YR: Yes.

Material finding: scenario load does not use the savegame active-vector stream owner. Its order source is native loader section order, per-section INI key order, and successful reveal/register timing. Active in YR: Yes.

## 4. INI Keys

No INI key gates the standard savegame active-vector save/load or swizzle path. Scenario load uses map INI section/key order as data, but no rules key sorts the `LogicClass` vector.

| Data source | Effect | Evidence | Active in YR |
|---|---|---|---|
| Savegame `LogicClass` vector stream | preserves active vector order across save/load | `FUN_0067D300`, `FUN_00551B20`, `FUN_0067E730`, `FUN_00551B90` | Yes |
| Map INI section/key order | seeds scenario active order | `ScenarioClass::Full_Init`; active-order report | Yes |
| Replay header/scenario filename | starts normal scenario init for replay | replay report `Main_Game @ 0x0052D9A0` | Conditional |

## 5. Current Rust Implementation Status

| Rust surface | Current shape | Delta |
|---|---|---|
| `src/sim/world/mod.rs:289` | `live_object_order: Vec<u64>` is serialized by default. | Directionally matches native vector persistence for Rust snapshots, but lacks raw pointer swizzle semantics and a separate `Object+0x98` byte. |
| `src/sim/world/mod.rs:612` | `register_live_object` scans vector then appends ID. | Native ordinary register uses object-local `+0x98` as the duplicate gate, not a vector scan. |
| `src/sim/world/mod.rs:618` | `unregister_live_object` unconditionally retains/removes ID. | Native remover first checks `Object+0x98`; clear means no vector search/removal. |
| `src/sim/world/mod.rs:622` | `live_object_order_snapshot` appends sorted missing `EntityStore` IDs after registered order. | Drift risk: no native sorted repair after save/load/replay/scenario load was found. |
| `src/sim/snapshot.rs:84..107` | bincode serializes/deserializes full `Simulation`; callers rebuild skipped caches. | Rust snapshots should preserve `live_object_order` exactly and should not rebuild it from `EntityStore` sorted IDs. |
| `src/sim/world/world_hash.rs:33..54` | `state_hash` hashes entities and systems, but not `live_object_order` explicitly. | Determinism hash can miss active-order drift unless downstream entity state changes expose it. |
| `src/sim/world/world_spawn.rs:260,438,588` | map/runtime/limbo spawn call `register_live_object`; limbo creation still registers. | Save/load report does not change prior lifecycle finding: storage/existence must not imply active membership. |

## 6. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| save owner call to vector saver | verified | `0x0067D435..0x0067D43A`; `FUN_0067D300` decompile | none |
| vector save order | verified | `FUN_00551B20`; assembly `0x00551B38..0x00551B78` | none |
| load owner call to vector loader | verified | `0x0067E8D2..0x0067E8D8`; `FUN_0067E730` decompile | none |
| vector load order | verified | `FUN_00551B90`; assembly `0x00551BAA..0x00551C4C` | none |
| swizzle slot registration | verified | `FUN_006CF240`; assembly `0x006CF240..0x006CF2B3` | none |
| swizzle final patch | verified | `FUN_006CF350`; assembly `0x006CF350..0x006CF3FF`; callers | none |
| post-load `FUN_0055BAA0` re-registration owner | verified negative | `FUN_0067E440` callee list; `FUN_0055BAA0` caller list | none for standard wrapper |
| `ObjectClass__Save @ 0x005F6250` omission | verified | decompile; caller list | none |
| IPersist stream ObjectClass save/load | verified | `FUN_0065AC40`, `AbstractClass::Save`, `AbstractClass::Load`, `ObjectClass::Load_IStream` | actual byte-count puzzle belongs to broader save/load format docs |
| replay active-list restore | verified negative | replay report `Main_Game`, `Main_Tick`, disassembly ranges | runtime byte watchpoints optional |
| scenario active-order source | verified by prior report | active-order source report | none for this slice |
| final `Object+0x98` byte for every Object-derived class after load | deferred | representative constructors verified; no runtime watchpoint sweep | runtime debugger watchpoints |

## 7. Open Questions - Final State

- `[RESOLVED] OQ-OAVSL-001 - Who saves the active vector? -> FUN_0067D300 calls FUN_00551B20 with ECX=0x87F778.` (evidence: `0x0067D435..0x0067D43A`)
- `[RESOLVED] OQ-OAVSL-002 - What order does save write? -> count first, then active vector slots in ascending index order.` (evidence: `FUN_00551B20`, `0x00551B38..0x00551B78`)
- `[RESOLVED] OQ-OAVSL-003 - Who loads the active vector? -> FUN_0067E730 calls FUN_00551B90 with ECX=0x87F778.` (evidence: `0x0067E8D2..0x0067E8D8`)
- `[RESOLVED] OQ-OAVSL-004 - What order does load use? -> saved stream order; entries append at old count before pointer swizzle registration.` (evidence: `0x00551BAA..0x00551C4C`)
- `[RESOLVED] OQ-OAVSL-005 - Does post-load swizzle rebuild membership? -> No; it patches queued pointer slots and clears queues only.` (evidence: `FUN_006CF350`, `0x006CF350..0x006CF3FF`)
- `[RESOLVED] OQ-OAVSL-006 - Does the standard load wrapper call normal register helper? -> No; wrapper callee list excludes FUN_0055BAA0, and FUN_0055BAA0 callers exclude save/load/post-load helpers.` (evidence: Ghidra caller/callee lists)
- `[RESOLVED] OQ-OAVSL-007 - Does ObjectClass__Save @ 0x005F6250 serialize +0x98? -> No; it writes +0x90 then +0x9C/+0xA0/+0xA4.` (evidence: `ObjectClass__Save` decompile)
- `[RESOLVED] OQ-OAVSL-008 - Is 0x005F6250 the savegame stream body? -> No; savegame ObjectClass stream save is FUN_0065AC40 -> AbstractClass::Save.` (evidence: callers of `ObjectClass__Save` and `FUN_0065AC40`)
- `[RESOLVED] OQ-OAVSL-009 - Does IPersist raw-body save initially carry +0x98? -> Yes, AbstractClass::Save writes raw class-sized object bytes after the saved this pointer.` (evidence: `AbstractClass::Save @ 0x00410320`)
- `[RESOLVED] OQ-OAVSL-010 - Does ObjectClass::Load_IStream call reveal/register? -> No; it loads base state, registers pointer slots, initializes handles, clears +0xA8.` (evidence: `0x005F5E80`)
- `[RESOLVED] OQ-OAVSL-011 - Does replay use savegame vector restore? -> No; replay startup launches normal scenario init and per-frame playback has no active-list restore/re-register call.` (evidence: replay report `0x0052D9A0`, `0x0055D360`)
- `[RESOLVED] OQ-OAVSL-012 - Does scenario load use savegame vector restore? -> No; scenario load uses Full_Init section/key order plus reveal/register.` (evidence: active-order report `0x00686B20`)
- `[RESOLVED] OQ-OAVSL-013 - Is sorted stable-ID fallback parity-safe after load/replay/scenario? -> No evidence; native sources are saved vector stream order, replay normal scenario init, or scenario loader/reveal order.` (evidence: reports above; Rust `live_object_order_snapshot`)
- `[DEFERRED] OQ-OAVSL-014 - What is final Object+0x98 for every object class after load-specific constructor/reset paths?` (category: `needs-runtime-debugger`; reason: static evidence proves no wrapper-level reconcile owner and representative constructor resets, but exhaustive per-class final byte values need watchpoints; next-step-if-pursued: watch `Object+0x98` for active unit/building/aircraft/infantry/anim/light/wave through load completion.)
- `[DEFERRED] OQ-OAVSL-015 - What exact key does FUN_00551A30 use for its per-tick adjacent active-order pass?` (category: `requires-different-system-context`; reason: this report covers restore/rebuild owners, not per-tick order maintenance; next-step-if-pursued: investigate `FUN_00551A30` virtual `+0xB8` semantics.)

## 8. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Save/load preserves the active vector directly and in saved vector order. Active in YR: Yes. | `FUN_0067D300`, `0x0067D435..0x0067D43A`; `FUN_00551B20`; `FUN_0067E730`, `0x0067E8D2..0x0067E8D8`; `FUN_00551B90` | `live_object_order` is serialized, but `live_object_order_snapshot` can append sorted missing IDs. | `src/sim/world/mod.rs:289`, `src/sim/world/mod.rs:622`, `src/sim/snapshot.rs:84` | Treat live order as first-class saved state; after load, preserve exact saved order and expose missing/stale members explicitly instead of sorted repair. | Save with live order `[30,10,20]` and entity storage keys `{10,20,30}`; load and assert the next active-order consumer sees `[30,10,20]`. Proposed test: `save_load_preserves_live_object_order_not_sorted_ids`. | Do not rebuild post-load active order by walking `EntityStore`/stable IDs. |
| The standard post-load wrapper does not call `FUN_0055BAA0` and swizzle only patches pointer slots. Active in YR: Yes. | `FUN_0067E440` callee list; `FUN_006CF230 -> FUN_006CF350`; `FUN_0055BAA0` caller list | Rust has no object-local membership byte; register/unregister infer from vector contents. | future active membership state in `Simulation` / `GameEntity`; `src/sim/world/mod.rs:612..619` | Keep vector membership/order separate from duplicate/removal gate state; do not model save load as "call register on every saved ID." | After load, concealing/despawning an active object should follow native membership-byte semantics once runtime byte values are sampled. Proposed test: `post_load_conceal_uses_native_logic_membership_gate`. | Do not assume vector membership and `Object+0x98` are automatically equivalent after load. |
| `ObjectClass__Save @ 0x005F6250` omits `+0x98`, but savegame stream persistence is raw-body `FUN_0065AC40 -> AbstractClass::Save`. Active in YR: Yes. | `ObjectClass__Save` decompile/callers; `FUN_0065AC40`; `AbstractClass::Save @ 0x00410320`; `ObjectClass::Load_IStream @ 0x005F5E80` | Rust bincode snapshots do not distinguish native CRC/checksum surface from IPersist raw-body save/load. | `src/sim/snapshot.rs`; future native `.SAV` importer | For Rust snapshots, preserve semantic state; for native `.SAV` import/export, model raw-body plus pointer swizzle/constructor reset instead of field-list assumptions from `0x005F6250`. | A native-save importer fixture with a field present in raw body but absent from `0x005F6250` must restore/reset it according to stream load semantics. Proposed test: `native_save_import_uses_ipersist_raw_body_not_crc_object_save`. | Do not cite `0x005F6250` as proof that savegames cannot contain `+0x98`. |
| Replay active order is not restored from a savegame vector; replay startup uses normal scenario initialization. Active in YR: Conditional on `DAT_00A8D5F8 & 2`. | replay report `Main_Game @ 0x0052D9A0`, `ScenarioClass::Read_Scenario @ 0x00684620`, `ScenarioClass::Full_Init @ 0x00686B20`, `Main_Tick @ 0x0055D360` | Rust replay runs over an existing `Simulation` and can be conflated with snapshot restore. | `src/sim/replay.rs`, snapshot/replay app surfaces | Native replay parity should initialize from replay header scenario data and normal map-load/reveal order, not deserialize a mid-match snapshot or invoke savegame vector loader. | Start replay for a map whose scenario active order differs from stable-ID order; first active-order consumer follows scenario load/reveal order. Proposed test: `native_replay_start_uses_scenario_init_not_snapshot_vector_load`. | Do not reuse savegame load or sorted repair as replay startup. |
| `state_hash` currently does not explicitly hash `live_object_order`. Active in YR: Rust-facing deterministic risk, not a binary behavior. | `src/sim/world/world_hash.rs:33..54`; `src/sim/world/mod.rs:289` | Reordered active vector can be invisible to hash until later behavior diverges. | `src/sim/world/world_hash.rs` | If active order is authoritative simulation state, include it in parity/desync checks or add targeted tests that compare order directly. | Mutate only `live_object_order` with same entities and assert a parity-mode hash or diagnostic detects the difference. Proposed test: `live_object_order_difference_affects_parity_hash`. | Do not rely on current `state_hash` to catch active-order drift. |

## 9. Negative Facts / Do Not Do

- Do not rebuild active order after load by sorting `EntityStore` or stable IDs. Active in YR: Yes; savegame load reads saved vector entries in stream order through `FUN_00551B90`.
- Do not implement post-load as "call `register_live_object`/`FUN_0055BAA0` for every saved active object." Active in YR: No for standard load wrapper; `FUN_0067E440` does not call `FUN_0055BAA0`.
- Do not use `ObjectClass__Save @ 0x005F6250` alone as savegame serialization proof. Active in YR: Yes but different surface; IPersist stream save is `FUN_0065AC40 -> AbstractClass::Save`.
- Do not treat replay restore as the same unresolved problem as save/load. Active in YR: replay playback is conditional on `DAT_00A8D5F8 & 2` and starts from normal scenario initialization, not savegame vector load.
- Do not treat `live_object_order_snapshot`'s sorted missing-ID append as a parity-safe repair path for save/load, replay, or scenario load. Active in YR: No native sorted repair found for these paths.

## 10. Stale Docs / Replacement Wording

- [OBJECT_TECHNO_LIFECYCLE_SHARED_STATE_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_TECHNO_LIFECYCLE_SHARED_STATE_SYSTEM_MODEL_SYNTHESIS.md): replace "Object save/load active-vector rebuild owner is known. | active-order report | unknown" with "Save/load active-vector order is known: `FUN_0067D300` saves and `FUN_0067E730` loads the `LogicClass` vector directly through `FUN_00551B20`/`FUN_00551B90`. The standard post-load wrapper does not re-register members through `FUN_0055BAA0`; final per-class `Object+0x98` byte sampling remains runtime-only follow-up."
- [ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md): replace "ObjectClass::Save @ 0x005F6250 does not serialize `ObjectClass+0x98`" with "`ObjectClass__Save @ 0x005F6250` is a CRC/checksum-style surface and omits `+0x98`; savegame IPersist persistence goes through `FUN_0065AC40 -> AbstractClass::Save`, which writes raw class-sized bytes before load-specific constructor/reset and swizzle paths run."
- [LOGIC_OBJECT_REGISTRATION_HELPER_FUN_0055BAA0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGIC_OBJECT_REGISTRATION_HELPER_FUN_0055BAA0_GHIDRA_REPORT.md): replace "Exact save/load and replay reconstruction of `Object+0x98` plus the LogicClass vector remains unresolved" with "Savegame vector order is saved/loaded directly and swizzled; replay startup uses normal scenario initialization. No standard post-load/replay `FUN_0055BAA0` re-registration owner was found. Runtime watchpoints are still needed for final per-class `Object+0x98` byte values after load."
- Any Rust handoff wording that says "stable IDs are deterministic, so sorted-ID fallback is safe" should be replaced with "Stable IDs are not an active-order source in gamemd. Use saved vector order for snapshots/savegames, scenario loader/key/reveal order for scenario load, and normal scenario initialization for replay."

## 11. Remaining Uncertainty

- Runtime watchpoints are still needed to assign final `Object+0x98` byte values for every Object-derived class after raw-body load plus load-specific constructor resets. This does not reopen the owner/order conclusion: no standard wrapper-level reconciliation owner was found.
- `FUN_00551A30` per-tick adjacent active-order maintenance key semantics are separate from save/load/replay/scenario restore ownership.

## Sources

- Ghidra read-only decompile/assembly/call evidence: `FUN_0067D300`, `FUN_0067E730`, `FUN_0067E440`, `FUN_00551B20`, `FUN_00551B90`, `FUN_006CF240`, `FUN_006CF230`, `FUN_006CF350`, `FUN_0065AC40`, `AbstractClass::Save @ 0x00410320`, `AbstractClass::Load @ 0x00410380`, `ObjectClass::Load_IStream @ 0x005F5E80`, `ObjectClass__Save @ 0x005F6250`, `FUN_0055BAA0`.
- Ghidra assembly ranges checked: `0x0067D435..0x0067D43A`, `0x0067E8D2..0x0067E8D8`, `0x00551B38..0x00551B78`, `0x00551BAA..0x00551C4C`, `0x006CF240..0x006CF2B3`, `0x006CF350..0x006CF3FF`.
- Prior research: [OBJECT_TECHNO_LIFECYCLE_SHARED_STATE_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_TECHNO_LIFECYCLE_SHARED_STATE_SYSTEM_MODEL_SYNTHESIS.md), [ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md), [LOGIC_OBJECT_REGISTRATION_HELPER_FUN_0055BAA0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGIC_OBJECT_REGISTRATION_HELPER_FUN_0055BAA0_GHIDRA_REPORT.md), [SAVE_LOAD_ACTIVE_VECTOR_RECONSTRUCTION_OWNER_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SAVE_LOAD_ACTIVE_VECTOR_RECONSTRUCTION_OWNER_RESWARM_20260528.md), [POST_LOAD_OBJECT_98_OWNER_RECONCILIATION_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/POST_LOAD_OBJECT_98_OWNER_RECONCILIATION_RESWARM_20260528.md), [REPLAY_ACTIVE_VECTOR_RESTORE_CORNER_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/REPLAY_ACTIVE_VECTOR_RESTORE_CORNER_RESWARM_20260528.md), [BUILDINGCLASS_SAVE_LOAD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_SAVE_LOAD_GHIDRA_REPORT.md).
- Rust static scan: `src/sim/world/mod.rs`, `src/sim/world/world_spawn.rs`, `src/sim/world/world_hash.rs`, `src/sim/snapshot.rs`, `src/sim/entity_store.rs`, `src/sim/replay.rs`.

**Status:** COMPLETE for active-vector owner/order and absence of standard post-load/replay re-registration owner; runtime-only per-class final `Object+0x98` byte sampling remains deferred.
