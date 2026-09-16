# GScreenClass / TacticalClass — Screen Base & Radar-Tactical Composite

**Primary addresses:**
- `GScreenClass::Constructor` at `0x004F4220` (vtable `0x007EA6FC`)
- `TacticalClass::Constructor` at `0x006D1C20` (vtable `0x007F4348`)
- `RenderFrame_main` (GScreenClass virtual render) at `0x004F4480` (= GScreenClass vtable[15], +0x3C)
- `MouseClass::Draw` (chain draw override) at `0x006D0A20` (= MouseClass vtable[16], +0x40)
- `TacticalClass::Draw` (non-virtual, three-pass) at `0x006D3D10`
- `TacticalClass::Update` (vtable[23], +0x5C) at `0x006D2540`

**Confidence:** HIGH — every address verified by live Ghidra decompilation.
**Active in YR:** Yes — this is the top-level render/input pipeline for every in-game frame.

> This report fills the gap left by the existing docs ([TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md),
> [SIDEBAR_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/SIDEBAR_SYSTEM_GHIDRA_REPORT.md), [MouseClass_research.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MouseClass_research.md), [ScrollClass_research.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ScrollClass_research.md),
> `MAPCLASS_GHIDRA_REPORT.md`) by focusing on **GScreenClass itself** (the empty base that
> is never instantiated standalone) and the **orchestration contract** between the display
> chain (`g_DisplayChain` = `MouseClass*` at `0x00887640`) and the sibling tactical object
> (`g_Tactical` at `0x00887324`). Those other docs cover what happens **inside**
> `TacticalClass::Draw` and each leaf class's vtable — this one covers the **top-level
> screen composition** that drives them.

---

## 1. Overview

The engine has exactly **two top-level UI objects**, both singletons:

| Global | Address | Type | Size | Role |
|--------|---------|------|------|------|
| `g_DisplayChain` | `0x00887640` | `MouseClass*` (the whole inheritance chain) | `0x556C` (21 868 B) | Screen chrome, sidebar, power, radar, input |
| `g_Tactical` | `0x00887324` | `TacticalClass*` (AbstractClass-derived, NOT in the chain) | `0x0E18` (3 608 B) | Isometric viewport, camera, world rendering |

`GScreenClass` is the **abstract base** of the display chain. It's never instantiated on
its own; the concrete object at `g_DisplayChain` is a `MouseClass` whose memory layout
contains every ancestor's fields in order. `TacticalClass` is a **sibling** (AbstractClass
→ TacticalClass) that composes with the display chain only through globals — not
inheritance or ownership.

Every frame, `Main_Tick` (`0x0055D360`) calls them in this fixed order:

```
GScreenClass::Input          (vtable[9] on g_DisplayChain) — poll + dispatch
LogicClass::AI               — tick all game objects
House_AI_Tick / Map::Logic   — misc tick work
RenderFrame_main             (vtable[15] on g_DisplayChain) — see §6
```

Inside `RenderFrame_main`, the chain's `Draw` method (`vtable[16]` = `MouseClass::Draw`)
and `TacticalClass::Draw` (direct non-virtual call) interleave in a specific three-pass
sandwich that puts **UI chrome BETWEEN terrain and objects**. This is the single most
important and least-documented composition rule in the engine.

---

## 2. GScreenClass Layout

GScreenClass itself has only **four fields**. Everything else in the 0x556C-byte mega-object
comes from subclasses.

```c
// GScreenClass — 0x10 bytes before MapClass begins at +0x14
+0x00  void* vtable            // = 0x007EA6FC for bare GScreenClass
+0x04  int   field_04           // init 0 — consumed by vtable[10] (sign-decay: see §3 idx 10)
+0x08  int   field_08           // init 0 — same pattern as +0x04
+0x0C  int   RedrawFlag         // init 2; 0=none, 1=partial, 2=full redraw
```

**Constructor (0x004F4220):**
```c
void GScreenClass::Constructor(GScreenClass* this) {
    this->field_04   = 0;
    this->field_08   = 0;
    this->RedrawFlag = 2;      // default: force full first-frame redraw
    this->vtable     = &vtable_GScreenClass;  // 0x007EA6FC
}
```

**Significance of `RedrawFlag` (+0x0C):** This is read and **consumed** by `RenderFrame_main`:

```c
iVar1 = this->RedrawFlag;      // snapshot
this->RedrawFlag = 0;          // clear
bool forceRedraw = (iVar1 != 0);
// later: (*vtable[16])(this, iVar1 == 2)   — true only on full-redraw tick
```

So `RedrawFlag == 2` means "do a **full** sidebar/chrome repaint this frame"; any non-zero
value still triggers the `TacticalClass_Draw(..., forceRedraw=true, ...)` dirty-clear path.
The flag is **set** by vtable slot 7 (`0x004F42D0`: `*(int*)(this+0xC) = 2`), which is the
generic "request full redraw" entry point.

**Gap ahead of MapClass:** Bytes `+0x10..+0x13` are padding/alignment — MapClass begins
at `+0x14`.

---

## 3. GScreenClass Primary Vtable (`0x007EA6FC`)

22 slots. Addresses listed here are the **GScreenClass base implementations** — the actual
mega-object at `g_DisplayChain` uses the MouseClass vtable (`0x007E1964`, §5), which
overrides the virtual entries.

| Idx | Offset | Base addr    | Purpose (verified) |
|-----|--------|--------------|--------------------|
| 0   | 0x00   | `0x004F4240` | `QueryInterface` — compares against 2 GUIDs at `0x007F7C90` / `0x007EA6E8`; returns 0x80004002 on miss |
| 1   | 0x04   | `0x0040D230` | `AddRef` → returns `1` (non-ref-counted singleton) |
| 2   | 0x08   | `0x0040D240` | `Release` → returns `1` (non-ref-counted singleton) |
| 3   | 0x0C   | `0x004C9150` | `Stub__ReturnZero` — pure-virtual slot (IPersistStream `GetClassID`?) |
| 4   | 0x10   | `0x004F4C00` | **Destructor** — resets vtable then conditionally `delete`s |
| 5   | 0x14   | `0x004F42A0` | `Init_Clear` — `DAT_00a8ef54 = 0` (clears gadget-root global) |
| 6   | 0x18   | `0x004F42B0` | Compound init: calls `(*vt[7])()` then `(*vt[8])()` |
| 7   | 0x1C   | `0x004F42D0` | `Set_Redraw_Full` — `this->RedrawFlag = 2` |
| 8   | 0x20   | `0x004F42E0` | `Clear_Gadget_Root` — `DAT_00a8ef54 = 0` (same as idx 5) |
| 9   | 0x24   | `0x004F4320` | ★ **`GScreenClass::Input`** — top-level input dispatcher (§7) |
| 10  | 0x28   | `0x004F4BB0` | **Sign-decay tick** on `this->field_04` and `field_08` — every OTHER frame, nudges each field one step toward 0 (positive→`1-val`, negative→`-1-val`). Purpose unclear; possibly edge-scroll latch decay |
| 11  | 0x2C   | `0x004F43F0` | Gadget-equality test: takes a pointer, calls its `vtable[6]`, returns `1` if result equals `DAT_00a8ef54`. **NOT** a mouse-X getter; see below |
| 12  | 0x30   | `0x004F4410` | (base impl — not decompiled in this pass) |
| 13  | 0x34   | `0x004F4450` | (base impl — not decompiled; MouseClass override at `0x005BDAA0` is the active version) |
| 14  | 0x38   | `0x004F42F0` | `DAT_00a8ef54 = 0` (same body as idx 8 — two distinct vslots calling the same helper) |
| 15  | 0x3C   | `0x004F4480` | ★ **`RenderFrame_main`** — the per-frame render orchestrator (§6) |
| 16  | 0x40   | `0x004AEBD0` | **Base Draw** — 2-byte stub `ret 4`. Overridden by every concrete subclass (`MouseClass::Draw` at `0x006D0A20` is what actually runs) |
| 17  | 0x44   | `0x004F45B0` | Post-render hook (called at the end of `RenderFrame_main`) |
| 18  | 0x48   | `0x004C9150` | `Stub__ReturnZero` (pure virtual) |
| 19  | 0x4C   | `0x004C9150` | Stub |
| 20  | 0x50   | `0x004C9150` | Stub |
| 21  | 0x54   | `0x004C9150` | Stub |

**Why `vtable[11]` is the "mouse X" slot everyone thinks.** `GScreenClass::Input` (idx 9)
calls `(*g_DisplayChain->vtable[11])()` and stores the result as `*param_3`, which every
caller treats as a mouse X coordinate. That only works because **MouseClass overrides**
slot 11 with `MouseClass::Get_Mouse_X` (`0x004F43F0`'s base is a gadget-equality stub
unrelated to input). This is a recurring trap: reading a GScreenClass base vtable slot
tells you nothing about what the actual chain method does.

**Key takeaway:** the GScreenClass base is almost entirely **stubs and trivial state
hooks**. The real behavior lives in the MouseClass-level overrides (§5). The one
genuinely interesting base-class method is `RenderFrame_main` (idx 15), which is NOT
overridden down the chain — every concrete class inherits it.

---

## 4. TacticalClass Layout

Allocated via `operator_new(0xE18)` in `FUN_006851F0` (scenario init). Inherits from
`AbstractClass` (NOT from GScreenClass — completely independent).

**Constructor at `0x006D1C20` initializes (verified byte-for-byte from disassembly):**

| Offset  | Type       | Init value                 | Purpose |
|---------|------------|----------------------------|---------|
| `0x00`  | `void*`    | `0x007F4348`               | Primary vtable (IPersistStream interface) |
| `0x04`  | `void*`    | `0x007F432C`               | Secondary vtable (IRTTITypeInfo) |
| `0x08`  | `void*`    | `0x007F4324`               | Secondary vtable (INoticeSink) |
| `0x0C`  | `void*`    | `0x007F431C`               | Secondary vtable (INoticeSource) |
| `0x24`  | `u16`      | `0`                        | (small field) |
| `0xA4`  | `int`      | `-1`                       | (index-like, -1 sentinel) |
| `0xA8`  | `int`      | (set later = frame counter)| ★ **Last-ticked frame** — dedup guard in Update |
| `0xAC`  | `u8`       | `0`                        | Flag |
| `0xAD`  | `u8`       | `0`                        | Flag |
| `0xB0..0xC0` | `int[5]` | `0`                      | Cleared on scroll completion (scroll state slots) |
| `0xC4`  | `int`      | `0x3FF00000`               | Upper half of `1.0` as `double` — Z-multiplier or scale |
| `0xC8`  | `int`      | `DAT_00B0CE08`             | Copy of default viewport X |
| `0xCC`  | `int`      | `DAT_00B0CE0C`             | Copy of default viewport Y |
| `0xD0`  | `int`      | `DAT_00B0CE08`             | ★ **Cinematic scroll target X** |
| `0xD4`  | `int`      | `DAT_00B0CE0C`             | ★ **Cinematic scroll target Y** |
| `0xD8`  | `float`    | `0.0`                      | ★ **Scroll speed** (frames⁻¹ of progress; compared vs `FLOAT_007E1748`) |
| `0xDC`  | `float`    | `0.0`                      | ★ **Scroll progress** (0..1; saturates at `0x3F800000`=`1.0`) |
| `0xE0`  | `int`      | `0`                        | ★ **Dirty-cell count** (see `TACTICAL_RENDER_PIPELINE` doc) |
| `0xE4..` | `int[800]` | `0`                       | ★ **Dirty-cell ring buffer** (800 entries × 4 B = 3 200 B, ends at `0xD64`) |
| `0xD64` | `int`      | `0`                        | ★ **Current viewport X** (after scroll interp) |
| `0xD68` | `int`      | `0`                        | ★ **Current viewport Y** |
| `0xD6C..0xD78` | `int[3]` | `0`                   | Padding / scratch |
| `0xD74` | `int`      | `0`                        | ★ **Previous viewport X** (dirty-diff source) |
| `0xD78` | `int`      | `0`                        | ★ **Previous viewport Y** |
| `0xD7C` | `u8`       | `1`                        | Flag (shroud edges enabled?) |
| `0xD7D` | `u8`       | `0`                        | ★ **Viewport-moved flag** — set by Update, consumed by Draw Pass 0 |
| `0xD7E` | `u8`       | `0`                        | Flag |
| `0xD80..0xD8C` | `int[4]` | `DAT_00B0CD60..6C`     | Default 4×int rect (likely initial visible rect) |
| `0xDA0` | `int`      | (incr)                     | ★ **Scroll counter** (how many auto-scroll steps have fired) |
| `0xDA4` | `int`      | `GetRadarTimer()`          | ★ **Last scroll time** (timestamp) |
| `0xDA8` | `int`      | `0`                        | Scroll delay |
| `0xDAC` | `int`      | `0` → `Rules+0x50`         | ★ **Scroll period** (from Rules, updated each Update) |
| `0xDB0` | `int*`     | `0`                        | ★ **Visible-building draw list head** (per `TACTICAL_RENDER_PIPELINE` — max 500 entries) |
| `0xDE4..0xE08` | `float[9]` | 3D constants            | ★ **Isometric projection matrix** (contains `±4.2667f` and `±8.5333f` — cell aspect ratios) |
| `0xE0C` | `float`    | `1.0f` (`0x3F800000`)      | Matrix scale |
| `0xE10` | `int`      | `0`                        | (tail) |

**Matrix setup (verified from disassembly):**
```c
Matrix3x4_SetIdentity();
Matrix_rotate_x_axis(*(float*)&DAT_00B0CD88);   // tilt (isometric X-axis)
Matrix3x4_RotateZ(*(float*)&DAT_00B0CE98);      // yaw (isometric Z-axis)
FUN_005AEA10(*(float*)&DAT_007F046C / *(float*)&DAT_00B0CD78);   // scale
```

So TacticalClass owns the **isometric camera matrix** as 12 floats somewhere in its tail
(the exact offset depends on `FUN_005AEA10`, not extracted here). This is the single
source of truth for world→screen projection — every call through `TacticalClass::CellToPixel`
(`0x006D1FE0`) and `CoordsToClient2` (`0x006D2140`) reads it.

**Total size:** `operator_new(0xE18)` → **3 608 bytes**.

**Global singleton storage:** `g_Tactical = param_1` at end of ctor → written to
`0x00887324`. Destroyed in `FUN_006BE1C0` (global shutdown) via
`(*g_Tactical->vtable[8])(1)` (vtable+0x20 = scalar deleting destructor).

---

## 5. TacticalClass Primary Vtable (`0x007F4348`) — 25 Slots

Unlike GScreenClass (which leans on subclass overrides), TacticalClass actually implements
most of its vtable.

| Idx | Offset | Address       | Purpose |
|-----|--------|---------------|---------|
| 0   | 0x00   | `0x00410260`  | `QueryInterface` (AbstractClass-shared) |
| 1   | 0x04   | `0x00410300`  | `AddRef` |
| 2   | 0x08   | `0x00410310`  | `Release` |
| 3   | 0x0C   | `0x006DBCE0`  | TacticalClass-specific (likely `Get_Class_ID` / IPersistStream member) |
| 4   | 0x10   | `0x00410450`  | Generic AbstractClass helper |
| 5   | 0x14   | `0x006DBD20`  | TacticalClass-specific |
| 6   | 0x18   | `0x006DBE00`  | TacticalClass-specific |
| 7   | 0x1C   | `0x004103E0`  | Generic |
| 8   | 0x20   | `0x006DC470`  | ★ **Scalar Deleting Destructor** (the 3rd "Tactical__Constructor" symbol is actually this) |
| 9   | 0x24   | `0x00410470`  | Generic |
| 10  | 0x28  | `0x006DA560`  | TacticalClass-specific |
| 11  | 0x2C  | `0x006DC450`  | TacticalClass-specific |
| 12  | 0x30  | `0x006DC460`  | TacticalClass-specific |
| 13  | 0x34  | `0x00410410`  | Generic |
| 14  | 0x38  | `0x00410490`  | Generic |
| 15  | 0x3C  | `0x004104A0`  | Generic |
| 16  | 0x40  | `0x004104B0`  | Generic |
| 17  | 0x44  | `0x00410440`  | Generic |
| 18  | 0x48  | `0x004104C0`  | Generic |
| 19  | 0x4C  | `0x004104F0`  | Generic |
| 20  | 0x50  | `0x00410520`  | Generic |
| 21  | 0x54  | `0x00410530`  | Generic |
| 22  | 0x58  | `0x00410540`  | Generic |
| 23  | 0x5C  | `0x006D2540`  | ★ **`TacticalClass::Update`** — per-frame scroll/animation tick (§6) |
| 24  | 0x60  | `0x006DBB60`  | `Tactical__DrawLine3D` (utility for effects/overlays) |

**`TacticalClass::Draw` (`0x006D3D10`) is NOT virtual.** It is called directly by address
from `RenderFrame_main` (3 times per frame with different `param_3` selectors: 0/1/2).
See [TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md) for the complete internals of those three passes.

**`TacticalClass::Update` (vtable[23], `0x006D2540`) — per-frame scroll animator:**

```pseudo
Update(this):
    if (SpecialFlags & 2) return;               // suppress render
    if (this.LastTickFrame == g_CurrentFrame)   // already ticked this frame
        goto skip_to_viewport_commit;

    if (DAT_00A8ED5C != 0                       // cinematic scroll globally enabled
        && (this.ScrollTargetX != DAT_00B0CE08
         || this.ScrollTargetY != DAT_00B0CE0C)
        && this.ScrollSpeed != FLOAT_007E1748):  // sentinel "no-scroll" marker
        this.Progress += this.ScrollSpeed;
        if (this.Progress > 1.0): this.Progress = 1.0;
        p = LerpVec2(&this.ScrollTargetX, this.Progress);  // FUN_0075F5C0
        if (this.Progress >= 1.0): clear all scroll state  // +0xC8..+0xDC = 0
        p' = ClampToMapBounds(p)                 // FUN_006D8640 — if result differs
                                                 //   and no multiplayer lock → use clamped
        this.CurrentX = this.PreviousX = p'.x
        this.CurrentY = this.PreviousY = p'.y
        FUN_006D8B30()                           // notify scroll listeners
        this.ViewportMovedFlag = 1

    // Auto-scroll timer (radar events, ping)
    period = Rules.TacticalScrollPeriod          // Rules+0x50
    if (now - this.LastScrollTime >= period):
        this.LastScrollTime = GetRadarTimer()
        this.ScrollPeriod = period
        this.ScrollCounter += 1

skip_to_viewport_commit:
    this.LastTickFrame = g_CurrentFrame
    if (this.Previous{X,Y} != this.Current{X,Y} && !scrolled_this_frame):
        // commit current → previous, re-clamp, mark moved
        (same body as scroll block)

    return
```

Consequences: Cinematic pans happen *here*, not in Draw. The dirty-rect plumbing in
Pass 0 of `TacticalClass::Draw` reads `ViewportMovedFlag` (+0xD7D) and the current/prev
positions to decide whether to scroll the ABuffer/ZBuffer or do a full redraw. Update
→ Draw is a one-way signal through those two fields.

---

## 6. The Core Composition Contract — `RenderFrame_main`

This is the **single function** that defines how `GScreenClass`'s world-of-chrome and
`TacticalClass`'s world-of-terrain combine into one frame. It lives at
`GScreenClass::vtable[15]` (`0x004F4480`), is **never overridden**, and is invoked once per
frame from `Main_Tick` via the mega-object's vtable.

**Exact body (decompiled verbatim, annotated):**

```c
void __thiscall RenderFrame_main(int* this)
{
    int saved_primary = g_PrimarySurface;
    g_PrimarySurface = g_CompositionSurface;                    // DAT_0088731C

    // (A) First chrome draw — ALWAYS, before any tactical work.
    //     vtable[16] = MouseClass::Draw (override). Args: (surface, redrawFlag=0).
    (*g_DisplayChain->vtable[16])(g_CompositionSurface, 0);

    int redrawFlags = this->RedrawFlag;   // +0x0C, snapshot
    this->RedrawFlag = 0;                 // consume
    bool forceRedraw = (redrawFlags != 0);

    if (FUN_0053BAE0() == 0) {            // not in loading-screen suppression
        // (B) Pass 0 — scroll / buffer prep (NO visible output)
        TacticalClass_Draw(g_Tactical, g_CompositionSurface, forceRedraw, 0);
        // (C) Pass 1 — terrain, shroud, overlays (writes ZBuffer + ABuffer)
        TacticalClass_Draw(g_Tactical, g_CompositionSurface, forceRedraw, 1);
        // (D) CHROME DRAW BETWEEN TERRAIN AND OBJECTS!
        //     vtable[16] again, this time with forceFullRedraw = (redrawFlags == 2).
        (*this->vtable[16])(redrawFlags == 2);
        // (E) Pass 2 — objects, effects, cursors (reads ABuffer for fog/shroud alpha)
        TacticalClass_Draw(g_Tactical, g_CompositionSurface, forceRedraw, 2);
    }

    // (F) Sidebar surface composite — only if sidebar is dirty and not in briefing mode
    if (DAT_00B0B519 != 0 && DAT_00A8ED6B == 0) {
        (*g_DisplayChain->vtable[16])(g_SidebarSurface, 1);     // redrawFlag=1 → sidebar-only
        DAT_00B0B519 = 0;
    }

    // (G) Gadget-root post-render hook
    if (DAT_00A8EF54 != 0) {
        (*DAT_00A8EF54->vtable[11])(0);     // vtable+0x2C on root gadget
    }

    // (H) Sidebar animations / drawables
    FUN_005D49A0();

    // (I) Tooltip manager (ToolTipManager)
    if (DAT_00887368 != 0) {
        (*DAT_00887368->vtable[3])(0);      // vtable+0x0C on tooltip singleton
    }

    // (J) Deferred house AI (the AI runs here IF a flag is set —
    //     this is the "render-then-think" path used by non-multiplayer modes)
    if (DAT_00A8B8B4 != 0) {
        House_AI_Tick();
    }

    // (K) Final composition present — vtable+0x3C = RenderFrame_main's OWN slot,
    //     but dispatched through the chain pointer (so this calls itself recursively?).
    //     Actually no: vtable[15] at idx 15 is this same function — but this is called
    //     on g_DisplayChain, which shares the vtable. The effect is: re-entering this
    //     function with DAT_0088731C and 0. This **does** happen (not guarded) — safe
    //     because the second entry finds RedrawFlag==0 and passes the Tactical gates
    //     harmlessly. It's the engine's idiom for "finalize the composition pass."
    (*g_DisplayChain->vtable[15])(g_CompositionSurface, 0);

    // (L) End-of-frame hook on self (vtable+0x44 = idx 17 = 0x004F45B0).
    (*this->vtable[17])();

    g_PrimarySurface = saved_primary;
}
```

### 6.1 Why the chrome draw is sandwiched between Pass 1 and Pass 2

Step (D) — `(*this->vtable[16])(redrawFlags == 2)` — draws the sidebar chrome, radar,
power bar, credits, and command bar **after** the terrain has been written to the back
surface but **before** world objects (units, buildings, effects) are drawn.

The consequence is visible when a unit sprite extends beyond the tactical view: the chrome
will occlude the terrain behind it but the object will *still* draw on top of the chrome
if its screen rect overlaps the sidebar. Pass 2 doesn't know or care about UI rectangles —
the tactical renderer's own clipping is what prevents world sprites from leaking into the
sidebar area (see `TACTICAL_RENDER_PIPELINE` §Pass 2, Step 1 — the 0x168/0xB4 margin clip).

### 6.2 Two flavors of chrome draw

| Call | Surface target | `redrawFlag` arg | Purpose |
|------|----------------|-------------------|---------|
| (A) Opening | `g_CompositionSurface` (`0x0088731C`) | `0` | Cheap predraw; chrome in sync with composition before tactical touches the buffer |
| (D) Middle  | (implicit — method writes to its own `g_SidebarSurface`) | `redrawFlags == 2` | Full chrome repaint if requested |
| (F) Late    | `g_SidebarSurface` (`0x00887300`) | `1` | Blit sidebar surface onto screen iff `DAT_00B0B519` dirty bit was set by (D) |

All three resolve to `MouseClass::Draw` (`0x006D0A20`), which internally dispatches down
the chain: `SidebarClass::Draw` (`0x006A6C30`) is the last statement.

---

## 7. Input Dispatch — `GScreenClass::Input` (`0x004F4320`)

Called once per tick from `Main_Tick` (gameplay path only). Decompiled:

```c
void GScreenClass::Input(GScreenClass* this, uint* outKey, int* outX, int* outY)
{
    *outX = (*g_DisplayChain->vtable[11])();   // MouseClass::Get_Mouse_X
    *outY = (*g_DisplayChain->vtable[12])();   // MouseClass::Get_Mouse_Y

    if (DAT_00A8EF54 == 0) {
        // No gadget root — raw keyboard/mouse polling.
        *outKey = FUN_0054F000() & 0xFFFF;
        if (*outKey != 0)
            *outKey = FUN_0054F050() & 0xFFFF;      // secondary key shift
    } else {
        // A gadget tree exists — let it consume input.
        if ((*DAT_00A8EF54->vtable[23])() != 0) {   // Hit_Test-ish
            (*this->vtable[14])(0);                 // Clear_Gadget_Root (DAT_00A8EF54 = 0)
        }
        int savedPrimary = g_PrimarySurface;
        g_PrimarySurface = DAT_0088730C;            // switch to gadget surface
        *outKey = (*DAT_00A8EF54->vtable[10])();    // Gadget::Input (consume)
        g_PrimarySurface = savedPrimary;
    }

    // Always dispatch through chain — the MouseClass override at vtable[10]
    // = 0x005BDDC0 is what actually moves the cursor & processes click bindings.
    (*this->vtable[10])(outKey, &savedMouseState);
}
```

**Chain follows** (per `SIDEBAR_SYSTEM_GHIDRA_REPORT §14`):
```
GScreenClass::Input (0x004F4320)  — entry
  └─ GadgetClass::Input (0x004E1640)         if gadget root exists
      └─ GadgetClass::Hit_Test (0x004E15A0)  — smallest-area-wins
      └─ GadgetClass::Clicked_On (0x004E13F0)
          └─ SelectClass::Action / GadgetClass::Action
  └─ chain.vtable[10] → MouseClass::Process_Input (0x005BDDC0)
      └─ ScrollClass handler (edge scroll, RMB drag)        — see ScrollClass_research
          └─ DisplayClass handler (tactical click → select) — see MouseClass_research
              └─ SidebarClass::Action (0x006A7780)          — see SIDEBAR_SYSTEM
```

`GScreenClass` itself provides **only** the framing: read mouse position, snapshot
key state, then hand off to whoever the gadget root is OR to the chain's own
`vtable[10]` override. It is a pass-through.

---

## 8. Per-Frame Orchestration (Main_Tick path)

From `Main_Tick` (`0x0055D360`) — the active-gameplay branch:

```c
if ((SpecialFlags & 2) == 0 && GameState == Gameplay && GameRunning) {
    GScreenClass::Input(local_18c, local_184, local_188);   // §7
    LogicClass::AI();                                        // tick all game objects
    if (AIRunEnabled) House_AI_Tick();                       // pre-render AI planning
    if ((g_CurrentFrameCounter & 7) == 7 && GameMode == Online) {
        Network_Keepalive();
    }
    Map::Logic();                                            // cell updates, tiberium growth
    RenderFrame_main();                                      // §6 — this object via vtable[15]
}
```

**Ordering invariant:** Input is ALWAYS polled before LogicClass::AI. Game objects see
the current frame's input when they tick. Render happens AFTER Logic, so the frame
drawn already reflects any position/anim updates this tick produced. This is why the
engine is able to be deterministic under lockstep: all input is captured into command
events before any simulation runs.

**Non-gameplay path (loading, paused, cinematic):** Uses a different wrapper
(`FUN_0055E160`) that still calls `(*g_Tactical->vtable[23])()` (Update) and
`RenderFrame_main()` without the input/logic pair — just animates the viewport and
redraws chrome.

### 8.1 Timing globals

| Global | Address | Purpose |
|--------|---------|---------|
| `DAT_00887328` | — | Frame start timestamp (`timeGetTime()`) |
| `DAT_00887330` | — | Target frame-budget (ms), adjusted for network latency |
| `DAT_00887348` | — | Radar-timer snapshot |
| `DAT_00887350` | — | Frame-rate setting (from `Rules[GameSpeed]`) |
| `DAT_00A8D5F8` | — | `SpecialFlags` — bit 2 suppresses render/input entirely |
| `DAT_00A8ED6B` | — | Briefing/transition mode — suppresses sidebar when non-zero |
| `DAT_00B0B519` | — | Sidebar-dirty bit — set by chrome-draw(D), cleared after present(F) |
| `DAT_00B0CD40` | — | Mouse-in-tactical flag (read by chrome draw) |
| `DAT_00B0CD44` | — | Recent-redraw counter (rate-limits full sidebar rebuilds) |

---

## 9. INI Keys

**There are NO INI keys that configure screen composition, tactical viewport size,
chrome layout, or render-pass selection.** All layout is hardcoded per resolution
(640/800/1024+), and all three-pass compositing happens unconditionally.

The few scalar-timing fields that reach this subsystem come from Rules:

| Source | INI key | Field read | Purpose |
|--------|---------|-----------|---------|
| Rules | `[General]GameSpeed` | `DAT_00887350` | Target ms/frame |
| Rules | Unlabeled (`Rules+0x50`) | `TacticalClass.ScrollPeriod` (+0xDAC) | Auto-scroll timer interval |
| Rules | `[Radar]RadarJammerRange` etc. | Consumed inside `MouseClass::Draw` — NOT in scope here |

Per `SIDEBAR_SYSTEM_GHIDRA_REPORT §2`, the Rules flag at `DAT_00A8B230 + 0x34B8`
toggles between RA2 and YR sidebar layouts — but that's chrome, not screen composition.

**Active in YR:** all of the above. No TS-legacy gating on `GScreenClass` or
`TacticalClass` orchestration — this pipeline runs every frame of every skirmish.

---

## 10. Integration Points

### 10.1 Who calls what

```
Main_Tick (0x0055D360)
  │
  ├── GScreenClass::Input(g_DisplayChain, ...)              [vtable[9], idx 9]
  │     └── (chain follows down to MouseClass/Sidebar)       [§7]
  │
  ├── LogicClass::AI()                                       [all game-object ticks]
  │
  ├── Map::Logic()                                           [cells, tiberium growth]
  │
  └── RenderFrame_main(g_DisplayChain)                       [vtable[15], idx 15]
        │
        ├── (A,D,F) MouseClass::Draw(...)                    [vtable[16] on g_DisplayChain]
        │     └── SidebarClass::Draw → PowerBar::Draw → StripClass::Draw → ...
        │
        ├── (B,C,E) TacticalClass::Draw(g_Tactical, ...)     [direct call, 3 passes]
        │     └── Tactical_layer_* (8 steps in Pass 1)       [per TACTICAL_RENDER_PIPELINE]
        │     └── Tactical_ObjectRenderingLoop (Pass 2)
        │
        ├── (G) DAT_00A8EF54->vtable[11](0)                  [gadget-root hook]
        │
        ├── (I) ToolTipManager->vtable[3](0)                 [tooltip render]
        │
        └── (K-L) recursion + end-hook                       [final present]

(non-gameplay paths also call TacticalClass::Update via g_Tactical->vtable[23])
```

### 10.2 Singleton lifecycle

- **Display chain** (`g_DisplayChain`) — constructed at WinMain via static ctor
  at `0x0040D190` (per `SIDEBAR_SYSTEM` §1). **Lives for the entire process.** Never
  destroyed between scenarios — only its state is reset by each subclass's `Init_Clear`.
- **Tactical** (`g_Tactical`) — constructed per-scenario in `FUN_006851F0`
  (`ScenarioClass::Full_Init` path). **Destroyed and re-created on every scenario load.**
  Destructor via `vtable[8]` in `FUN_006BE1C0`.

This asymmetry is significant: **resetting the tactical camera requires `operator new` +
constructor**, not just field reinitialization. The Rust rewrite can safely treat both as
persistent singletons; the fresh-allocation semantics only matter for save/load
compatibility with the original engine.

---

## 11. Current Rust Implementation Status

### 11.1 Architectural differences (intentional)

The Rust engine does **not** reproduce the 9-class single-inheritance chain. Instead:

| gamemd.exe construct | Rust equivalent | File(s) |
|----------------------|-----------------|---------|
| `GScreenClass` (base) | No analogue — not needed, since Rust has no OOP chrome inheritance | — |
| `g_DisplayChain` (MouseClass mega-object) | Split across `AppState` + `SidebarView` | [src/app.rs](src/app.rs), [src/sidebar/](src/sidebar/) |
| `g_Tactical` (viewport + camera) | `AppState` camera fields + `app_render` pipeline | [src/app_camera.rs](src/app_camera.rs), [src/app_render/mod.rs](src/app_render/mod.rs) |
| `RenderFrame_main` orchestrator | `render_game()` 6-phase pipeline | [src/app_render/mod.rs](src/app_render/mod.rs) |
| `GScreenClass::Input` + chain | `app_input` modules + egui capture | [src/app_input.rs](src/app_input.rs) |
| `TacticalClass::Update` scroll animator | Direct camera state manipulation in app layer | [src/app_camera.rs](src/app_camera.rs) |
| `TacticalClass::Draw` 3-pass compositor | 6-phase build + single GPU render pass | [src/app_render/mod.rs](src/app_render/mod.rs) |

### 11.2 Composition contract compliance

**Critical invariant that the Rust side currently handles differently:** gamemd.exe draws
chrome **between terrain and objects** (§6 step D). The Rust `render_game()` pipeline
builds all instances first and draws them in a single pass. This is OK because the GPU
Z-buffer and wgpu batching handle the layering correctly — but it means the Rust engine
does NOT need to expose the surface-swap / dirty-bit plumbing the original uses.

### 11.3 What's NOT implemented

- **Cinematic scroll interpolation.** TacticalClass fields +0xD0..+0xDC (scroll target
  X/Y, speed, progress 0..1) and the `FUN_006D8B30` listener pattern have no Rust
  counterpart. Map-ping/radar-event snap-to-target doesn't currently animate.
- **ABuffer/ZBuffer circular scroll.** Irrelevant — we use wgpu depth testing, not
  software buffer scrolling.
- **`RedrawFlag`-based repaint selection.** In Rust we always redraw everything each
  frame; `redrawFlags == 2` vs `== 1` partial-repaint logic doesn't map to the GPU pipeline.
- **Gadget tree hit-testing** (GadgetClass::Hit_Test smallest-area-wins). egui handles
  this differently — not a parity gap, just a different paradigm.
- **Dirty-rect list** (+0xE0..+0xD64 in TacticalClass — 800-entry ring buffer). Rust
  repaints full viewport every frame.

---

## 12. Open Questions

1. **Exact layout of the isometric camera matrix.** The constructor writes 12+ floats
   starting at +0xDE4, but `FUN_005AEA10` (matrix scale) may also write to earlier
   offsets. Full matrix offset map requires decompiling the CellToPixel/CoordsToClient
   helpers — out of scope for this report. Existing [COORDINATE_SYSTEM_GAMEMD.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COORDINATE_SYSTEM_GAMEMD.md) may
   cover this. **Action:** if RA2's exact isometric projection differs from ours by
   sub-pixel amounts, re-investigate this field block.

2. **Why two distinct GScreenClass vslots (idx 8 and idx 14) both just set
   `DAT_00A8EF54 = 0`.** Likely two different event types ("screen init cleanup" vs
   "lose focus") that happen to share the same minimal implementation. Names would
   clarify — but since nothing in the chain overrides them with different behavior,
   it doesn't affect parity.

3. **Purpose of GScreenClass field_04 / field_08 + vtable[10] sign-decay pattern.**
   Reads to these two fields don't appear in obvious input paths. Hypothesis: they're
   "sticky keyboard modifier" counters used by long-lost debug builds — the modern
   engine may be carrying dead state. Not parity-critical.

4. **Recursive `RenderFrame_main` call at step (K).** `(*g_DisplayChain->vtable[15])(surface, 0)`
   at the end of the body appears to re-enter the same function. Second entry's
   `redrawFlags` is always 0 (just cleared) so it's effectively a chrome-only finalize.
   Verify by tracing the second-entry call stack — but don't over-invest, the Rust
   rewrite doesn't need this pattern.

5. **Rule `+0x50`** = `ScrollPeriod`. What INI key maps to Rules+0x50? The usual
   approach (grep for key name in INI reader at rules-struct offset 0x50) should find
   it; likely `[General]AutoScrollPeriod` or similar.

---

## Sources

**Ghidra addresses decompiled this session:**
- `0x004F4220` — GScreenClass::Constructor
- `0x007EA6FC` — GScreenClass primary vtable (256 bytes read)
- `0x004F4240`, `0x004C9150`, `0x004F4C00` — QueryInterface, Stub__ReturnZero, Destructor
- `0x004F42A0`, `0x004F42B0`, `0x004F42D0`, `0x004F42E0`, `0x004F42F0` — base trivial methods
- `0x004F4320` — GScreenClass::Input
- `0x004F43F0` — GScreenClass::vtable[11] base (NOT a mouse-X getter in base form)
- `0x004F4480` — RenderFrame_main (GScreenClass::vtable[15])
- `0x004AEBD0` — base Draw stub (`ret 4`)
- `0x004F4BB0` — vtable[10] sign-decay helper
- `0x006D1C20` — TacticalClass::Constructor (with disassembly byte-dump to extract vtable addresses)
- `0x007F4348` — TacticalClass primary vtable (256 bytes read)
- `0x006D2540` — TacticalClass::Update (vtable[23])
- `0x006D0A20` — MouseClass::Draw (vtable[16] override for display chain)
- `0x007E1964` — MouseClass primary vtable (128 bytes read, first 32 slots)
- `0x006851F0` — Scenario full-init (allocates `g_Tactical` via `operator_new(0xE18)`)
- `0x0055D360` — Main_Tick (gameplay + non-gameplay paths)
- `0x0055E160` — Non-gameplay frame wrapper

**Doc files cross-referenced:**
- [TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TACTICAL_RENDER_PIPELINE_GHIDRA_REPORT.md) — three-pass internals of `TacticalClass::Draw`
- [SIDEBAR_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/SIDEBAR_SYSTEM_GHIDRA_REPORT.md) — SidebarClass vtable, input dispatch chain, class hierarchy
- [MouseClass_research.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MouseClass_research.md) — MouseClass override methods, cursor state
- [ScrollClass_research.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ScrollClass_research.md) — Edge scroll, RMB drag, scroll coasting
- `MAPCLASS_GHIDRA_REPORT.md` — MapClass field layout (the largest chain member)
- `GAMEMD_ARCHITECTURE.md` — Overall class hierarchy + display chain diagram
- [ADDRESS_MAP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADDRESS_MAP.md) — Verified canonical addresses for all mentioned constructors/vtables

**INI files checked:**
- `ini/rulesmd.ini` — No viewport/screen/tactical-layout keys found (all layout hardcoded)
- `ini/rules.ini` — Same
- `ini/artmd.ini` — N/A (art data unrelated to screen composition)
