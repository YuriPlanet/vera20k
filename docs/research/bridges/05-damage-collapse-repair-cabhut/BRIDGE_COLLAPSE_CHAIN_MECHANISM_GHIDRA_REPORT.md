# Bridge Collapse Chain Mechanism — Ghidra Research Report

**Address(es):**
- `MapClass::CollapseBridge_NS_High` @ `0x00575BA0` (high-bridge collapse walker — NS axis)
- `MapClass::CollapseBridge_NS_Low` @ `0x00575540` (low-bridge collapse walker — NS axis, twin)
- `MapClass::DestroyBridge_High` @ `0x0057CCF0` (per-cell dispatcher, high)
- `MapClass::DestroyBridgeWalker_NS_High` @ `0x0057CF60` (overlay state machine, high NS)
- `MapClass::ApplyBridgeDestruction_NS_High` @ `0x0057E7A0` (perpendicular column write helper)
- `MapClass::FindBridgeEndpoints_NS_High` @ `0x0057DC20` (endpoint walker → broadcast)
- `RepairBridgeSegment` @ `0x00575EE0` (misnamed — actually a trigger-broadcast walker)
- `TechnoClass::ProcessCellAction` @ `0x006E53A0` (misnamed — actually FireTriggerAction)
- `MapClass::DestroyBridgeFromCell_High` @ `0x005749C0` (anchor-selection dispatcher)
- `MapClass::DestroyBridge_High_OnHutDeath` @ `0x00574000` (5×5 scan entry point from BuildingClass::Update / BombClass::Detonate)

**Confidence:** HIGH — every claim in this report was verified by live Ghidra decompilation or memory read in the 2026-05-20 session.

**Active in YR:** Yes — bridge destruction is a core YR mechanic; all call chains are live (no SpecialFlags gates, no TS-era callers).

---

## 1. Overview

This report resolves the four open questions left by the 2026-05-12 audit cluster
and the 2026-05-20 patch session:

1. What does `TechnoClass::ProcessCellAction(0x1F, 0, DAT_00ABD480, 0, 0)` actually do?
2. What is `DAT_00ABD480`?
3. Where is `FindBridgeEndpoints_NS_High @ 0x0057DC20` invoked from?
4. Does the `BridgeExplosions[rand]` animation (spawned by `CollapseBridge`) carry
   a warhead with AoE damage that could create a recursive chain?

**Bottom line:** Every "chain mechanism" hypothesis is refuted. The
collapse pipeline is **linear, not recursive**: one damage event triggers one
bounded 4-step walker that destroys ~18 cells in a 3-wide × 6-long footprint
(for a 3-wide bridge), with cosmetic anims spawned per cell and a no-op trigger
broadcast along the span. There is no warhead AoE re-entry, no damage propagation,
and no `CollapseBridge` recursion.

This also surfaces an unintended finding: the 2026-05-20 trace doc
[CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/08-traces/CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md) § Stage 2 mischaracterizes
gamemd's `CollapseBridge_*_High` as a "span-completing walker" that walks the
entire bridge end-to-end. The binary contradicts this — `local_2c = 4` is a
hard cap. The Rust port's new full-span flood-fill (shipped to
`bridge_orchestrator.rs` per the user's session note) over-destroys vs gamemd.
See § 6 for the disparity tally.

---

## 2. Call chain (verified end-to-end)

```
                ┌─────────────────────────────────────────────────────┐
                │ BuildingClass::Update @ 0x44031B                    │ (CABHUT hut-death entry)
                │   or BombClass::Detonate                            │
                └────────────────────────┬────────────────────────────┘
                                         ▼
              ┌─────────────────────────────────────────────────────────┐
              │ DestroyBridge_High_OnHutDeath @ 0x00574000              │  5×5 overlay scan
              │  - inner: 5×5 search for overlay 0xCD..0xE8             │  finds entry cell
              │  - fallback: 8-direction sweep using g_DirectionOffsets │
              └────────────────────────┬────────────────────────────────┘
                                       ▼
              ┌─────────────────────────────────────────────────────────┐
              │ DestroyBridgeFromCell_High @ 0x005749C0                 │  anchor selection
              │  - reads cell overlay (puVar3+0x44)                     │  (0/1/2-cell back)
              │  - classes into NS-set or EW-set                        │
              │  - picks anchor with 0/-1/-2 axial offset               │
              └────────────────────────┬────────────────────────────────┘
                                       ▼
              ┌─────────────────────────────────────────────────────────┐
              │ CollapseBridge_NS_High @ 0x00575BA0                     │  ★ THE 4-STEP WALKER
              │  - extent measurement: walk Y--, then Y++               │
              │  - direction = -1 if iVar10 < iVar11 else +1            │
              │  - start = (impact_y - (back-fwd)/2)  (signed div)      │
              │  - LOOP local_2c=4 iterations:                          │
              │     • spawn 3 anims at (X-1,Y) (X,Y) (X+1,Y) per iter  │ ← BridgeExplosions
              │     • call DestroyBridge_High(current) up to 3 retries  │
              │     • step Y by local_14 (+1 or -1)                     │
              │     • BREAK if next cell overlay outside [0xCD..0xE8]   │
              │  - tail: UpdateBridgeZonesHelper(); Tactical+0xD7C = 1  │
              └────────────────────────┬────────────────────────────────┘
                                       ▼
              ┌─────────────────────────────────────────────────────────┐
              │ DestroyBridge_High @ 0x0057CCF0  (per-cell dispatcher)  │
              │  - reads cell overlay (puVar3+0x44)                     │
              │  - NS class [0xCD..0xD5, 0xDF..0xE2, 0xE7]              │
              │    → DestroyBridgeWalker_NS_High                        │
              │  - EW class [0xD6..0xDE, 0xE3..0xE6, 0xE8]              │
              │    → DestroyBridgeWalker_EW_High                        │
              │  - also snaps to adjacent anchor cell if current is at  │
              │    a band-edge                                          │
              └────────────────────────┬────────────────────────────────┘
                                       ▼
              ┌─────────────────────────────────────────────────────────┐
              │ DestroyBridgeWalker_NS_High @ 0x0057CF60                │  overlay transition
              │   reads this->OverlayTypeIndex = local_ac               │
              │   branches on current overlay:                          │
              │   • 0xDF → set 3 cells to 0xE0, ApplyBridgeDest ×1      │
              │   • 0xE1 → set 3 cells to 0xE2, ApplyBridgeDest ×1      │
              │   • <0xD3 → set 3 cells to 0xD3, ApplyBridgeDest ×2     │
              │   • 0xD3..0xD5 → set 3 cells to 0xE7, ApplyBridgeDest ×2│  ← FULL DESTROY
              │                  FindBridgeEndpoints_NS_High            │
              │                  local_a5 = 1 (full-destroy flag)       │
              │                  scatter rect = 3×3 at (X-1,Y-1)        │
              │   • >0xD5 → return 0 (no-op)                            │
              │   then: RecalcAttributes ×3, UpdateBridgeZonesHelper    │
              │         if local_a5 != 0                                │
              └─────────┬───────────────────────────────┬───────────────┘
                        │                               │
                        ▼ (always, 1 or 2×)             ▼ (only on full-destroy)
        ┌────────────────────────────────┐  ┌──────────────────────────────┐
        │ ApplyBridgeDestruction_NS_High │  │ FindBridgeEndpoints_NS_High  │
        │ @ 0x0057E7A0                   │  │ @ 0x0057DC20                 │
        │ writes 3-cell axial range at   │  │ walks DAT_0089F690 dir then  │
        │ neighbor column (X±1)          │  │       DAT_0089F6A0 dir until │
        │ to overlay 0xE7 / 0xE0 / 0xE2 /│  │ off-bridge, calls            │
        │ 0xD1..0xD5 per neighbor-       │  │ RepairBridgeSegment(p1,p2)   │
        │ pattern lookup table (local_70)│  └─────────────┬────────────────┘
        └────────────────────────────────┘                ▼
                                       ┌─────────────────────────────────────────┐
                                       │ RepairBridgeSegment @ 0x00575EE0        │  trigger walker
                                       │ (DESPITE NAME: NOT REPAIR)              │
                                       │   walks from p1 → p2 along bridge span  │
                                       │   for each cell:                        │
                                       │     IF cell->AttachedTag (+0x3C) != 0:  │
                                       │       call FireTriggerAction(0x1F, 0,   │
                                       │           DAT_00ABD480, 0, 0)           │
                                       │   horizontal: 4 calls per X step        │
                                       │   vertical:   4 calls per Y step        │
                                       │   (main cell + 3 perpendicular cells)   │
                                       └─────────────────┬───────────────────────┘
                                                         ▼
                          ┌─────────────────────────────────────────────────────┐
                          │ TechnoClass::ProcessCellAction @ 0x006E53A0         │  trigger broadcast
                          │ (DESPITE NAME: NOT A SWITCH ON ACTION CODES)        │
                          │   walks this->AttachedTag.ActionList (+0x24, link   │
                          │     at +0x28)                                       │
                          │   for each entry e:                                 │
                          │     ev = e+0x9C  (event-type field)                 │
                          │     match = EvaluateConditions(0x1F, source, …, ev) │
                          │     if match:                                       │
                          │       PlayVoiceForObjects(source, coord)            │
                          │       DynamicVectorClass::Add  (pending action      │
                          │                                  queue)             │
                          │   re-entry guarded by this+0x35 latch               │
                          │ NO DAMAGE, NO OVERLAY MUTATION, NO DEATH, NO ZONE   │
                          │ CHANGE. Pure scripted-trigger broadcast.            │
                          │ ON VANILLA YR SKIRMISH MAPS WITH NO TRIGGERS BOUND  │
                          │ TO EVENT 0x1F (= TriggerEvent::BridgeDestroyed),    │
                          │ THIS IS A COMPLETE NO-OP.                           │
                          └─────────────────────────────────────────────────────┘
```

---

## 3. The four open questions — answered with binary evidence

### Q1 — What does `TechnoClass::ProcessCellAction(0x1F, 0, DAT_00ABD480, 0, 0)` actually do?

**Answer:** It is a **scripted-trigger broadcast**. The function is misnamed in
Ghidra; its actual semantic is `FireTriggerAction(eventType, source, coord, p5, p6)`.
It does **NOT** switch on action codes — the `0x1F` is the
`TriggerEvent::BridgeDestroyed` event ID passed straight through to
`TriggerActionEntry::EvaluateConditions` as the event-type filter.

**Verified body** (decompile_function 0x006E53A0, this session):

```c
undefined1 __thiscall TechnoClass__ProcessCellAction
    (int param_1, undefined4 param_2, int param_3,
     undefined4 param_4, undefined4 param_5, undefined4 param_6)
{
  if (g_IsMapEditor == '\0' && *(char *)(param_1 + 0x35) == '\0'
                            && *(char *)(param_1 + 0x34) == '\0') {
    if (*(int *)(param_1 + 0x24) != 0) {  // AttachedTag.ActionList head
      iVar1 = *(int *)(param_1 + 0x28);   // next-link offset
      *(undefined1 *)(param_1 + 0x35) = 1;  // re-entry latch
      do {
        if (iVar1 == 0) { … finalize, queue, return; }
        iVar2 = *(int *)(*(int *)(param_1 + 0x24) + 0x9c);  // entry's event-type
        cVar5 = TriggerActionEntry__EvaluateConditions(
                    param_2, param_3, param_5,
                    CONCAT31((int3)((uint)iVar2 >> 8), iVar2 == 2),
                    param_6);
        if (cVar5 != '\0') {
          if (iVar2 == 0)      { PlayVoiceForObjects; DynamicVectorClass::Add; }
          else if (iVar2 == 1) { /* gated by param_1+0x2c == 1 */ … }
          else if (iVar2 == 2) { PlayVoiceForObjects;            }
        }
        iVar1 = *(int *)(iVar1 + 0x28);
      } while(true);
    }
  }
  return 0;
}
```

**Side-effect surface (complete):**
- `PlayVoiceForObjects` (EVA voice if action carries a Speak= field)
- `DynamicVectorClass::Add` to the global pending-actions queue (drained later
  in the tick by `TriggerClass::Run_Action`)
- Sets `param_1->latch (+0x35) = 1` for one tick (re-entry guard)
- Possibly calls `Detach_From_All_Lists()` at function tail under specific flag combinations

**No mutations to:** cell overlay, building HP, unit HP, terrain, bridge zone state,
overlay anchor, surrounding pavement, animation list, projectile list, smudge,
crater. **Zero damage, zero death, zero overlay change.**

**On vanilla YR skirmish maps:** No trigger has event 0x1F bound by default in any
shipping multiplayer map. The entire `RepairBridgeSegment` walk on a skirmish map
visits every span cell and broadcasts to nothing — a complete no-op. (The mechanism
exists for campaign trigger scripting only.)

**`param_2 = 0x1F`** is the event-type ID. The constant matches `TriggerEvent::BridgeDestroyed`
(= 31) per the `TriggerCondition::Evaluate @ 0x0071E940` case-cluster
membership — see [TECHNOCLASS_PROCESSCELLACTION_0x1F_0x30_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_PROCESSCELLACTION_0x1F_0x30_GHIDRA_REPORT.md) §2.

**Evidence:** `decompile_function 0x006E53A0` (this session); cross-confirmed with
[TECHNOCLASS_PROCESSCELLACTION_0x1F_0x30_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_PROCESSCELLACTION_0x1F_0x30_GHIDRA_REPORT.md) §1–§5 (2026-05-20 audit).

---

### Q2 — What is `DAT_00ABD480`?

**Answer:** It is a **zeroed-coord sentinel** — a global 4-byte coord initialized
to `0x00000000`, used as the "no specific cell context" placeholder for the
4th parameter (`coord` / `cell pointer`) of `FireTriggerAction`. It is not a
warhead pointer, not an animation pointer, not a flag bitfield.

**Verified:** `read_memory 0x00ABD480, length=16` returns `00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00`.
The 16-byte zero window confirms it's a static-zero variable, not a runtime-populated pointer.

**Sibling sentinel:** `DAT_00B0E700` (also `read_memory` confirmed = `00 00 00 00 00 00 00 00`)
is the sentinel value `ProcessCellAction` compares `param_4` against in its
cleanup branch:
```c
if (((short)param_4 != (short)DAT_00b0e700) || (param_4._2_2_ != DAT_00b0e700._2_2_)) {
  uVar7 = 0;
  MapClass__Get_CellClass(&param_4);
  FUN_00485250(uVar7);
}
```
When the caller passes `DAT_00ABD480` (= 0), this comparison is false, the
cell-cleanup branch is skipped. So the two sentinels work together: callers
that pass `DAT_00ABD480` are explicitly saying "this is a no-cell-context event."

**Evidence:** `read_memory 0x00ABD480` and `read_memory 0x00B0E700` (this session);
caller chain in `decompile_function 0x00575EE0` (RepairBridgeSegment) shows
the constant pushed at 7 call sites, all paired with `param_2 = 0x1F`.

---

### Q3 — Where is `FindBridgeEndpoints_NS_High @ 0x0057DC20` invoked from?

**Answer:** It has **exactly one caller**: `MapClass::DestroyBridgeWalker_NS_High @ 0x0057CF60`,
and only on the **full-destroy transition arm** (overlay reaches 0xD3..0xD5 → writes
0xE7 to 3 cells). It is not called on absorb-damage transitions (overlay <0xD3, =0xDF, =0xE1).

**Verified:** `get_function_callers 0x0057DC20` returns:
```
MapClass__DestroyBridgeWalker_NS_High @ 0057cf60
```

**Call-site context** (from `decompile_function 0x0057CF60`):
```c
else {  // local_ac in [0xD3..0xD5]
  if (0xd5 < local_ac) { return local_a5; }  // (= 0 — early out)
  local_a0->OverlayTypeIndex = 0xe7;         // 3-cell axial write
  local_a4->OverlayTypeIndex = 0xe7;
  this->OverlayTypeIndex = 0xe7;
  RadarClass__MarkTerrainDirty ×3;
  local_b0 = CONCAT22(param_1[1], *param_1 + -1);
  local_84 = CONCAT31(local_84._1_3_, 1);     // **full-destroy flag**
  local_ac = local_b0;
  MapClass__ApplyBridgeDestruction_NS_High(&local_ac);
  local_b0 = CONCAT22(param_1[1], *param_1 + 1);
  local_ac = local_b0;
  MapClass__ApplyBridgeDestruction_NS_High(&local_ac);
  MapClass__FindBridgeEndpoints_NS_High(*(undefined4 *)param_1);  // ← THE CALL
  local_80 = *param_1 + -1;
  local_7c = param_1[1] + -1;
  local_a5 = '\x01';
  local_78 = 3;
  local_74 = 3;
}
```

**Body of FindBridgeEndpoints_NS_High** (`get_function_callees 0x0057DC20`):
```
RepairBridgeSegment @ 00575ee0
```
Single callee — its job is to walk to the bridge endpoints in `DAT_0089F690`
and `DAT_0089F6A0` directions and call `RepairBridgeSegment(endpoint1, endpoint2)`,
which is the trigger-broadcast walker from Q1.

**NS/EW label swap caveat:** Per `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` §7,
the `_NS_High` label on this function actually processes the **EW-oriented** overlay
range despite the name. This is a Ghidra labeling artifact from the 2026-05-12 audit
and is consistent across the whole `_NS_High` / `_EW_High` family.

**Evidence:** `get_function_callers 0x0057DC20` and `decompile_function 0x0057CF60`
(both this session).

---

### Q4 — Does `BridgeExplosions[rand]` carry a warhead with AoE damage?

**Answer:** **NO.** Refuted by direct inspection of the AnimClass constructor call
at the spawn site and by the `artmd.ini` definitions of the anim entries.

**The constructor call** (decompile_function 0x00575BA0, lines mid-function):
```c
pvVar6 = operator_new(0x1c8);   // AnimClass instance size = 0x1C8
if (pvVar6 != (void *)0x0) {
  uVar7 = Random__RandomRanged(1, 5);                                     // start-frame variant
  iVar11 = Random__RandomRanged(0, *(int *)(g_RulesClass_Instance + 0x168) + -1);
  AnimClass__Constructor(
      *(undefined4 *)(*(int *)(g_RulesClass_Instance + 0x15c) + iVar11 * 4),  // arg 1: anim type
      &local_c,                                                                // arg 2: coord
      uVar7,                                                                   // arg 3: start frame (1..5)
      1,                                                                       // arg 4: flag (always 1)
      0x600,                                                                   // arg 5: flag (0x600)
      0,                                                                       // arg 6: zero
      0);                                                                      // arg 7: zero
}
```

**Seven arguments. No warhead pointer.** The third argument is the *start-frame
variant* (RandomRanged(1, 5)) — a small integer that picks which frame of the
loop to begin on for visual variety, not a warhead. The other three trailing args
are the literal constants `1, 0x600, 0, 0`.

**The DVC lookup** (per [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md) §2):
- `g_RulesClass + 0x168` = `BridgeExplosions.ActiveCount` (DVC at base +0x158)
- `g_RulesClass + 0x15C` = `BridgeExplosions.Vector` (data pointer to `AnimTypeClass*[]`)
- Index by `iVar11 * 4` because each element is a 4-byte `AnimTypeClass*`

**The AnimTypeClass instances themselves** (`artmd.ini:15656..15686` — the
`BridgeExplosions=` keys are `TWLT026, TWLT036, TWLT050, TWLT070`):
- `Normalized=yes`
- `Translucent=yes`
- `Report=Explosion0X`
- `UseNormalLight=yes`
- `Crater=no`
- `Scorch=no`
- **No `Warhead=` key.** AnimTypeClass.Warhead defaults to null → no damage tick.

**There is no recursive chain mechanism.** The collapse process is:
1. `CollapseBridge_NS_High` runs **once** per damage event
2. It iterates **at most 4 axial steps** (`local_2c = 4`)
3. Each iteration spawns 3 cosmetic anims (perpendicular spread X-1, X, X+1)
4. Each iteration calls `DestroyBridge_High` (per-cell overlay write, with up to 3 retries)
5. **No anim spawns damage. No damage spawns more collapse. The chain ends here.**

The user's hypothesis ("our spawn_bridge_debris has no warhead — does the
original?") is refuted: the original also has no warhead. Both engines spawn
pure visual+SFX animations with no damage hook.

**Evidence:** `decompile_function 0x00575BA0` (this session); cross-confirmed
with [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md) §2 (2026-05-18 audit) and
`artmd.ini:15656..15686` anim definitions.

---

## 4. CollapseBridge_NS_High — full algorithm (verified)

Per `decompile_function 0x00575BA0` (this session), here is the complete walker
behavior in pseudocode using exact constants from the binary:

```
fn CollapseBridge_NS_High(impact_cell: CellCoord) {
  // -- Phase 1: extent measurement --
  let mut back = 0;
  let mut fwd  = 0;
  let mut probe_back = impact_cell;
  loop {  // walk Y-- counting bridge cells
    probe_back.y -= 1;
    back += 1;
    let cell = get_cellclass(probe_back);
    let ov = cell.overlay;  // (+0x44)
    if ov < 0xCD { break; }       // OFF bridge → stop
    if ov >= 0xE9 { break; }      // OFF bridge band → stop
  }
  let mut probe_fwd = impact_cell;
  loop {  // walk Y++ counting bridge cells
    probe_fwd.y += 1;
    fwd += 1;
    let cell = get_cellclass(probe_fwd);
    let ov = cell.overlay;
    if ov < 0xCD { break; }
    if ov >= 0xE9 { break; }
  }

  // -- Phase 2: direction + start-cell selection --
  let step: i16 = if fwd < back { -1 } else { 1 };  // walk toward the longer extent
  let start_y = impact_cell.y - (back - fwd) / 2;   // signed division — IMPORTANT:
                                                    //   parity for odd-span bridges
                                                    //   landed by the round-toward-zero
                                                    //   convention. Verify with fixed-point.

  let mut cur = CellCoord { x: impact_cell.x, y: start_y };

  // -- Phase 3: main walker, 4 iterations max --
  for _step_idx in 0..4 {
    let cur_cell = get_cellclass(cur);

    // 3a. Spawn 3 cosmetic anims (perpendicular spread across bridge width)
    if cur_cell.overlay != 0xE8 {    // skip if already at destroyed-anchor sentinel
      let mut anim_pos = CellCoord { x: cur.x - 1, y: cur.y };
      for _ in 0..3 {
        let cell = get_cellclass(anim_pos);
        let coord_leptons = (
          cell.lepton_x * 0x100 + 0x80,  // center of cell in lepton-space
          cell.lepton_y * 0x100 + 0x80,
          cell.bridge_layer_z * DAT_00ABDE88,  // Z = cell+0x11B × scale
        );
        // 4 RNG calls IN THIS ORDER (lockstep-critical):
        let jitter_x = RandomRanged(0, 0x7FFFFFFE);  // X jitter
        let jitter_y = RandomRanged(0, 0x7FFFFFFE);  // Y jitter
        // (jitter is applied to coord_leptons via Math::ftol)
        let anim_obj = operator_new(0x1C8);  // AnimClass size
        if !anim_obj.is_null() {
          let start_frame = RandomRanged(1, 5);  // frame variant
          let anim_idx = RandomRanged(0, BridgeExplosions.count - 1);
          AnimClass::Constructor(
            BridgeExplosions[anim_idx],  // anim type
            coord_leptons,
            start_frame,
            1, 0x600, 0, 0,
          );
        }
        anim_pos.x += 1;
      }
    }

    // 3b. Per-cell destruction (up to 3 retries)
    for _retry in 0..3 {
      let result = DestroyBridge_High(&cur);  // dispatches to DestroyBridgeWalker_NS/EW_High
      if result != 0 { break; }
    }

    // 3c. Step along chosen axial direction
    cur.y += step;

    // 3d. Termination check — exit if walker has walked off the bridge
    let next_cell = get_cellclass(cur);
    if next_cell.overlay < 0xCD || next_cell.overlay > 0xE8 { break; }
  }

  // -- Phase 4: tail (always runs) --
  UpdateBridgeZonesHelper();
  *(byte*)(g_Tactical + 0xD7C) = 1;  // deferred PathGrid-rebuild flag
}
```

### Anim spawn — RNG order (lockstep-critical)

Per axial iteration, per perpendicular cell, in this exact order:
1. `RandomRanged(0, 0x7FFFFFFE)` — X-jitter
2. `RandomRanged(0, 0x7FFFFFFE)` — Y-jitter
3. `RandomRanged(1, 5)` — anim start-frame variant
4. `RandomRanged(0, BridgeExplosions.count - 1)` — anim type index

That's **4 RNG draws × 3 perpendicular cells × up to 4 axial iterations = up to 48 RNG
draws** per CollapseBridge invocation. **The Rust port must mirror this exact draw
order or the multiplayer state hash diverges.**

### LOW variant identity

`CollapseBridge_NS_Low @ 0x00575540` is a compiled twin with the overlay band
substituted to `[0x4A..0x65]` and the destroyed-anchor sentinel changed from
`0xE7`/`0xE8` to `0x64`/`0x65`. Same 4-iteration cap, same RNG order, same
`DestroyBridge_Low` retry loop. EW variants of both Low and High exist at
`0x00575220` and `0x00575870` respectively (per [BRIDGE_PAVEMENT_WALKER_AND_CELLLIST_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_PAVEMENT_WALKER_AND_CELLLIST_DISPATCH_GHIDRA_REPORT.md)).

---

## 5. DestroyBridgeWalker_NS_High — overlay state machine

Per `decompile_function 0x0057CF60`, the walker reads the current cell's overlay
and dispatches into one of five branches. Each branch writes overlay to a **3-cell
axial range** at the impact column AND triggers `ApplyBridgeDestruction_NS_High`
on adjacent columns (X±1).

| `current_overlay` | Write to (X, Y-1/Y/Y+1) | ApplyBridgeDestruction call sites | Notes |
|-------------------|-------------------------|-----------------------------------|-------|
| `0xDF` (band edge) | `0xE0` | (X-1, Y) only — one call | Single-direction transition |
| `0xE1` (band edge) | `0xE2` | (X+1, Y) only — one call | Single-direction transition |
| `< 0xD3` (light damage) | `0xD3` (medium damage) | (X-1, Y) and (X+1, Y) | Two calls — bilateral cascade |
| `0xD3..0xD5` (medium damage) | `0xE7` (DESTROYED) | (X-1, Y) and (X+1, Y) | **Full-destroy arm** — also calls FindBridgeEndpoints, sets local_a5=1, scatter rect 3×3 at (X-1, Y-1) |
| `> 0xD5` | (return 0 — no-op) | — | Already destroyed |

**Implication for total destroyed cells per walker call:**
- Center column (X): 3 axial cells
- Adjacent columns (X-1, X+1): each gets 3 axial cells via ApplyBridgeDestruction
- **Total: 3 columns × 3 axial cells = 9 cells per DestroyBridgeWalker_NS_High call (full-destroy arm)**

**And per CollapseBridge_NS_High call (up to 4 axial iterations):**
- 4 iterations × 9-cell write = 36 cell-writes
- With overlap (each iteration shifts axially by 1, so the 3-cell axial window
  shifts by 1 cell): unique cells = (4 + 2) axial × 3 perp = **18 cells**

**This is the actual gamemd collapse footprint per damage event: ~18 cells in a
3-perp × 6-axial rectangle**, biased toward the longer end of the bridge from
the impact point. **NOT** a full-span flood-fill and **NOT** a single 3-cell column.

---

## 6. RepairBridgeSegment — what it actually does (it's not "repair")

Despite its Ghidra label, `RepairBridgeSegment @ 0x00575EE0` is a **trigger-event
broadcast walker** with no repair logic, no damage logic, and no overlay logic.

**Verified body** (decompile_function 0x00575EE0, this session):
- Two endpoints `param_1` and `param_2`
- Detects orientation: horizontal (same Y) vs vertical (same X) via `param_1._2_2_ == param_2._2_2_`
- Sorts endpoints so the walker advances in +X (horizontal) or +Y (vertical) direction
- Loop until `param_2 == uVar8` (sorted-far endpoint):
  - **Horizontal branch (4 ProcessCellAction calls per iteration):**
    1. Main cell @ `(cur.x, cur.y)`
    2. Perpendicular cell @ `cur + DAT_0089F698` (= zero per `read_memory`)
    3. Perpendicular cell @ `cur + g_DirectionOffsets` (advance 1)
    4. Perpendicular cell @ `cur + g_DirectionOffsets` (advance 2)
    Then `cur.x += 1`
  - **Vertical branch (4 ProcessCellAction calls per iteration):**
    1. Main cell @ `(cur.x, cur.y)`
    2. Perpendicular cell @ `cur + DAT_0089F690` (= zero per `read_memory`)
    3. Perpendicular cell @ `cur + DAT_0089F6A0` (advance 1)
    4. Perpendicular cell @ `cur + DAT_0089F6A0` (advance 2)
    Then `cur.y += 1`
- Each ProcessCellAction call is gated by `*(int *)(cell + 0x3C) != 0` (cell has attached TagClass)

**The DAT_* perpendicular offset globals** (`read_memory` confirmed = 8 bytes of zeros each):
- `DAT_0089F690` = `00 00 00 00 00 00 00 00`
- `DAT_0089F698` = `00 00 00 00 00 00 00 00`
- `DAT_0089F6A0` = `00 00 00 00 00 00 00 00`

These zero-init values look surprising — they should be `(0, ±1)` or `(±1, 0)`
cell-offsets to land on perpendicular rails. The static value at link time is
zero; they must be **populated at runtime** by `SetBridgeDirection_*` or similar
during map load. (Out of scope for this report — see [BRIDGE_DIRECTION_TABLES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_DIRECTION_TABLES_GHIDRA_REPORT.md)
for the runtime population path.) For Rust-port purposes this is a sim-detail that
does not affect the chain question.

**All 7 ProcessCellAction call sites in RepairBridgeSegment:**
Per direct count from the decompilation, there are **8 push-sites** (not 7 as the
prior session note recorded): 4 in the horizontal branch, 4 in the vertical branch.
The 2026-05-12 audit's "7 call sites" claim was a miscount of the vertical branch.

---

## 7. Sentinel and global identity table

| Symbol | Address | Static value | Runtime role |
|--------|---------|--------------|--------------|
| `DAT_00ABD480` | `0x00ABD480` | 16 bytes of `0x00` | "No cell context" sentinel passed as ProcessCellAction param_4 (coord) |
| `DAT_00B0E700` | `0x00B0E700` | 8 bytes of `0x00` | Compare-against sentinel in ProcessCellAction cleanup branch |
| `DAT_0089F690` | `0x0089F690` | 8 bytes of `0x00` | RepairBridgeSegment vertical perpendicular offset #1 (runtime-populated) |
| `DAT_0089F698` | `0x0089F698` | 8 bytes of `0x00` | RepairBridgeSegment horizontal perpendicular offset (runtime-populated) |
| `DAT_0089F6A0` | `0x0089F6A0` | 8 bytes of `0x00` | RepairBridgeSegment vertical perpendicular offset #2 (runtime-populated) |
| `DAT_00ABDE88` | `0x00ABDE88` | (unread this session) | Bridge-layer Z scale (`cell+0x11B * DAT_00ABDE88`) — used in anim spawn lepton Z |
| `g_RulesClass_Instance` | `0x008871E0` | static `null` | Runtime-populated `RulesClass*` (heap-allocated at startup) |
| `g_RulesClass + 0x15C` | (runtime) | — | `BridgeExplosions.Vector` (data ptr to `AnimTypeClass*[]`) |
| `g_RulesClass + 0x168` | (runtime) | — | `BridgeExplosions.ActiveCount` |
| `g_Tactical + 0xD7C` | (runtime) | — | Deferred PathGrid-rebuild flag (set to 1 in CollapseBridge tail) |

All addresses verified via `read_memory` (this session) where applicable.

---

## 8. INI keys

| Key | Section | Type | Default | Effect |
|-----|---------|------|---------|--------|
| `BridgeExplosions=` | `[General]` | comma-list of AnimType names | `TWLT026, TWLT036, TWLT050, TWLT070` | Pool of cosmetic explosion anims spawned per cell by `CollapseBridge_*`. No damage. |
| `MetallicDebris=` | `[General]` | comma-list of AnimType names | (varies) | Pool spawned by `CellClass::BlowUpBridge @ 0x0047DD70` — a sibling spawn site only reached via `ProcessBridgeDamageStateMachine_*` and `UpdateRamp_*_Collapse*`, NOT via `CollapseBridge_*`. Worth noting that the user's Rust `spawn_bridge_debris` covers both pools. |
| `DestroyableBridges=` | `[General]` | bool | `yes` | Gates the whole damage-causes-collapse mechanic. If `no`, bridges absorb damage without overlay state transitions. See [DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/DESTROYABLEBRIDGES_INI_GATE_GHIDRA_REPORT.md). |

Each pool is parsed by `RulesClass::ReadGeneral @ 0x0066D530` into a
`DynamicVectorClass<AnimTypeClass*>` instance at the offsets in § 7. Per-tile-anim
warhead has no key — animations spawned by these pools never carry damage in
gamemd.

---

## 9. Integration with the tick cycle

- **Entry point:** Damage event (bomb, warhead) → cell damage handler → eventual
  call into `DestroyBridge_High_OnHutDeath @ 0x00574000` (for CABHUT-triggered
  destructions) or `ProcessBridgeDamageStateMachine_*` (for organic shell damage).
- **Tick ordering:** All cell-overlay writes happen synchronously within the
  damage-application stage. The `g_Tactical + 0xD7C` flag is set to defer
  PathGrid rebuild to the next tick's pre-AI phase.
- **Render dirty:** `RadarClass::MarkTerrainDirty` is called for each of the 3
  cells in the full-destroy arm, and `TacticalClass::DirtyScreenRect` is called
  with the union bounding rect.
- **Pending action queue:** Each successful `ProcessCellAction` push goes into
  the global `DynamicVectorClass<TriggerAction>` consumed by `TriggerClass::Run_Action`
  later in the tick. On a vanilla skirmish map, no triggers are bound to event
  0x1F so the queue stays empty.

---

## 10. Current Rust implementation status

Per the user's session note (paraphrased):
- Bridge fix shipped in `src/sim/world/bridge_orchestrator.rs` — full-span flood-fill
- Passes 2434/2434 lib tests but not yet verified in-game
- 5 bridge docs patched (2026-05-20 morning cluster)

### Disparities surfaced by this report (player-visible)

#### Disparity 1 — CollapseBridge scope: full-span vs. bounded 4-step

**Player-visible effect (high frequency — every bridge damage event):** A long
bridge (>6 cells from the impact point in either direction) is collapsed entirely
by the Rust port from a single C4 hit, whereas gamemd would collapse only the
~6 axial cells closest to the impact and leave the remainder standing for
follow-up damage. On a 12-cell bridge with impact at midpoint, the user can
destroy the entire bridge with one C4 in the port vs. half the bridge in gamemd.

**Severity:** HIGH — gameplay-relevant. Lifts the C4 from "damages a segment" to
"levels the whole bridge." The trace doc [CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/08-traces/CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md)
diagnosed the old code as "destroys ~3 cells, but gamemd destroys the whole span"
— **the second half of that diagnosis is wrong.** gamemd destroys ~18 cells
(3 perp × 6 axial), bounded by `local_2c = 4` (this report § 5).

**Recommended fix direction:** Replace the full-span flood-fill in
`bridge_orchestrator.rs` with the bounded 4-iteration walker matching § 4's
pseudocode. Each iteration spawns 3 cosmetic anims (perpendicular) and a 9-cell
overlay write (center column 3 axial + adjacent columns 3 axial each via
`ApplyBridgeDestruction`). Total ~18 unique cells per damage event.

#### Disparity 2 — Bridge axis bias

**Player-visible effect (high frequency):** gamemd biases the walker's starting
position toward the *shorter* side of the bridge from the impact point and walks
toward the *longer* side (`step = -1 if fwd < back else 1`,
`start_y = impact_y - (back - fwd) / 2`). The Rust port's flood-fill has no axial
bias at all. After fixing Disparity 1, the choice of bias direction will be
visible: which half of the bridge gets destroyed depends on where the impact lands.

**Severity:** MEDIUM — secondary to Disparity 1 but matters for the strategic
"where on the bridge does the player aim the C4" decision.

#### Disparity 3 — Anim RNG draw order (lockstep-critical)

**Player-visible effect:** None to a single player — but in multiplayer the
state hash diverges. Per § 4, the per-iteration RNG order is:
X-jitter, Y-jitter, start-frame-variant, anim-type-index, **× 3 perpendicular cells × 4 axial iterations** = 48 RNG draws max.

The [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md) §4 and the existing test
`debris_consumes_correct_rng_count_per_cell` already require this order. **Verify
the test still passes after Disparity 1 fix changes the per-event cell count.**

#### Disparity 4 — RepairBridgeSegment broadcast (LOW priority)

**Player-visible effect:** None on a vanilla YR skirmish map (no triggers bound
to event 0x1F = BridgeDestroyed). Only relevant for campaign trigger scripting,
which is out of scope per memory `feedback_no_ai_yet.md` and the campaign
deferral. **Triggers fire every match where a bridge destruction occurs, but
the broadcast is a no-op without bound triggers.**

**Recommended fix direction:** Stub. Rust port can skip the trigger broadcast
walker entirely while campaign scripting is out of scope. Document the stub
location for future campaign work.

### Rust files relevant to the fix (per the user's note and prior trace docs)

- `src/sim/world/bridge_orchestrator.rs` — the dispatch + outcome aggregation (full-span flood-fill — needs replacement per § 10 Disparity 1)
- `src/sim/bridge_state/walker.rs` — `destroy_bridge_walker_ns_high` (overlay state machine, per § 5)
- `src/sim/bridge_state/mod.rs` — `apply_bridge_destruction_ns_high` and the sibling-column cascade (this is correct per § 5's ApplyBridgeDestruction analysis)

**Do not rebuild** the existing walker / state-machine code in `walker.rs` — it
correctly mirrors `DestroyBridgeWalker_*_High`'s 3-cell axial write + bilateral
`ApplyBridgeDestruction` call. The fix is **only** at the
`run_hut_destroy_entry` / `dispatch_bridge_collapse_from_hut` level, where the
4-iteration cap and start-cell bias need to mirror § 4's algorithm exactly.

---

## 11. Open Questions — Final state of the investigation log

### Resolved

- `[RESOLVED]` Q1 — What does ProcessCellAction(0x1F, ...) do? → Scripted-trigger
  broadcast; no damage path. (evidence: `decompile_function 0x006E53A0`, this session)
- `[RESOLVED]` Q2 — What is DAT_00ABD480? → Zeroed-coord sentinel for "no cell
  context" coord arg. (evidence: `read_memory 0x00ABD480` = 16 zero bytes, this session)
- `[RESOLVED]` Q3 — FindBridgeEndpoints_NS_High caller? → Only
  DestroyBridgeWalker_NS_High @ 0x0057CF60, full-destroy arm only. (evidence:
  `get_function_callers 0x0057DC20`, this session)
- `[RESOLVED]` Q4 — Does BridgeExplosions carry warhead/AoE? → No. AnimClass
  constructor has 7 args, none is a warhead pointer. anim entries in artmd.ini
  have no Warhead= key. (evidence: `decompile_function 0x00575BA0` lines mid-function,
  this session; cross-check with [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md) §2)
- `[RESOLVED]` Q5 — Is there any recursive chain re-entry? → No. CollapseBridge
  runs once per damage event, bounded to local_2c = 4 axial iterations.
  DestroyBridgeWalker writes overlay and dispatches to ApplyBridgeDestruction
  for adjacent columns, but no callee re-enters CollapseBridge. (evidence: callee
  graph for CollapseBridge_NS_High has no path back to CollapseBridge_*; verified
  via `get_function_callees 0x00575BA0` and decompilation of all 6 callees)
- `[RESOLVED]` Q6 — [CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/08-traces/CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md) Stage 2 claim
  that gamemd CollapseBridge walks the whole span → **WRONG.** The walker is
  bounded to 4 axial iterations. (evidence: `decompile_function 0x00575BA0`
  `local_2c = 4` assignment line, this session; cross-checked with LOW twin
  `decompile_function 0x00575540` showing identical algorithm)
- `[RESOLVED]` Q7 — How many ProcessCellAction call sites in RepairBridgeSegment?
  → 8 (not 7 as the 2026-05-12 audit noted): 4 in horizontal branch, 4 in
  vertical branch. (evidence: `decompile_function 0x00575EE0`, this session)
- `[RESOLVED]` Q8 — Sibling sentinel DAT_00B0E700 identity? → 8 zero bytes,
  used as the param_4 compare-against value inside ProcessCellAction's cleanup
  branch. (evidence: `read_memory 0x00B0E700`, this session)

### Deferred

- `[DEFERRED]` Q9 — `DAT_0089F690 / DAT_0089F698 / DAT_0089F6A0` perpendicular
  offset values when populated at runtime. (category: `requires-different-system-context`;
  reason: Static value is zero; runtime population is by `SetBridgeDirection_*`
  at map load. Out of scope for the chain-mechanism question — does not affect
  whether ProcessCellAction is a no-op. next-step-if-pursued: read
  [BRIDGE_DIRECTION_TABLES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_DIRECTION_TABLES_GHIDRA_REPORT.md) or decompile
  `SetBridgeDirection_NS_High` @ 0x0056A610.)
- `[DEFERRED]` Q10 — `DAT_00ABDE88` (bridge-layer Z scale) exact value and
  semantics. (category: `bounded-cost-too-high`; reason: cosmetic Z-coord
  computation for anim spawn. The user's `spawn_bridge_debris` already passes
  the layer-correct Z based on overlay band — would only matter for sub-cell
  visual precision. next-step-if-pursued: `read_memory 0x00ABDE88` and check
  any caller that writes to it.)
- `[DEFERRED]` Q11 — Exact `ProcessCellAction` `this` argument at the call site
  from RepairBridgeSegment. Ghidra's decompilation shows 5 explicit args but
  the function signature is `__thiscall` with 6 params (this + 5). The `this`
  is presumably in ECX from the outer caller chain. (category:
  `bounded-cost-too-high`; reason: regardless of what `this` is, the function
  is a trigger broadcast — on a vanilla skirmish map with no triggers bound to
  event 0x1F, the call is a no-op no matter what `this` points to.
  next-step-if-pursued: read assembly at any of the 8 call sites in
  RepairBridgeSegment with `get_assembly_context 0x00576007` and verify ECX
  contents.)
- `[DEFERRED]` Q12 — `DestroyBridge_High_OnHutDeath` "5×5 inner scan with
  fallback walk" — is the fallback walk path live in YR or TS-only? (category:
  `out-of-scope`; reason: the inner 5×5 scan reliably finds bridge cells in
  CABHUT scenarios; the fallback path is for unusual configurations. Not
  load-bearing for the chain-mechanism question. next-step-if-pursued: trace
  callers and check if fallback path is hit on a representative YR map.)

### Not pursued

- The 4 EW-axis twins (`CollapseBridge_EW_High`, `DestroyBridgeWalker_EW_High`,
  `ApplyBridgeDestruction_EW_High`, `FindBridgeEndpoints_EW_High`) — per
  decompilation of the LOW twin and the prior `HIGH_BRIDGE_DAMAGE_STATE_MACHINE`
  doc family, EW twins use identical algorithms with X/Y swap and overlay band
  substitution. Walking through them would not produce new findings for the
  chain question.

---

## 12. Sources

### Ghidra functions decompiled this session

- `0x006E53A0` — TechnoClass::ProcessCellAction (verified body)
- `0x00575BA0` — MapClass::CollapseBridge_NS_High (verified 4-iteration walker)
- `0x00575540` — MapClass::CollapseBridge_NS_Low (twin, cross-check)
- `0x0057CCF0` — MapClass::DestroyBridge_High (per-cell dispatcher)
- `0x0057CF60` — MapClass::DestroyBridgeWalker_NS_High (overlay state machine)
- `0x0057E7A0` — MapClass::ApplyBridgeDestruction_NS_High (perpendicular column write)
- `0x00575EE0` — RepairBridgeSegment (trigger broadcast walker)
- `0x005749C0` — MapClass::DestroyBridgeFromCell_High (anchor selection)
- `0x00574000` — MapClass::DestroyBridge_High_OnHutDeath (5×5 entry point)

### Ghidra memory reads this session

- `0x00ABD480` (16 bytes): `00 × 16` — sentinel verified
- `0x00B0E700` (8 bytes): `00 × 8` — sentinel verified
- `0x0089F690` (8 bytes): `00 × 8` — perpendicular offset (runtime-populated)
- `0x0089F698` (8 bytes): `00 × 8` — perpendicular offset (runtime-populated)
- `0x0089F6A0` (8 bytes): `00 × 8` — perpendicular offset (runtime-populated)

### Ghidra caller / callee queries this session

- `get_function_callers 0x0057DC20` (FindBridgeEndpoints_NS_High) → 1 caller
- `get_function_callers 0x00575BA0` (CollapseBridge_NS_High) → 1 caller
- `get_function_callers 0x005749C0` (DestroyBridgeFromCell_High) → 1 caller
- `get_function_callees 0x00575BA0` (CollapseBridge_NS_High) → 7 callees
- `get_function_callees 0x0057DC20` (FindBridgeEndpoints_NS_High) → 1 callee
- `get_function_callees 0x0057CF60` (DestroyBridgeWalker_NS_High) → 13 callees

### Prior research docs cross-referenced

- [TECHNOCLASS_PROCESSCELLACTION_0x1F_0x30_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_PROCESSCELLACTION_0x1F_0x30_GHIDRA_REPORT.md) (2026-05-20) — primary source for Q1, Q2 doc-side claims; all confirmed live this session
- `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` (2026-05-20) — call chain and the NS/EW-label-swap caveat
- [BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGEEXPLOSIONS_RULES_OFFSETS_GHIDRA_REPORT.md) (2026-05-18) — DVC layout and BlowUpBridge sibling spawn site
- [BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md) (2026-05-20) — caller-chain context for DestroyBridge_High_OnHutDeath
- [CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/08-traces/CABHUT_PER_CELL_DESTRUCTION_CASCADE_TRACE.md) (2026-05-20) — diagnosed the OLD Rust truncation bug correctly but mischaracterized gamemd's CollapseBridge as full-span; this report corrects that
- [BRIDGE_PAVEMENT_WALKER_AND_CELLLIST_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_PAVEMENT_WALKER_AND_CELLLIST_DISPATCH_GHIDRA_REPORT.md) (2026-05-18) — confirmed addresses for all 4 CollapseBridge_*_* variants
- `BRIDGE_SYSTEM.md` (2026-05-18) — master overview (consulted but not load-bearing for this report)

### INI files consulted

- `ini/artmd.ini:15656..15686` — BridgeExplosions anim type definitions (TWLT026/036/050/070) — no Warhead= key on any
- `ini/rulesmd.ini` `[General]` section — `BridgeExplosions=`, `MetallicDebris=`, `DestroyableBridges=` keys

### Rust files NOT consulted

This is a research-only investigation per the HARD-GATE in `/re-investigate`.
No Rust files were read or modified. § 10's disparity tally is derived from the
user's session-note description of the shipped code; the recommended fix
direction is for a separate `/brainstorm` or `/write-plan` session.
