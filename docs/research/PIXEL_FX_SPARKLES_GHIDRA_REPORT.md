# PixelFX Sparkles — Per-Frame Water & Ore Twinkle System

**Date:** 2026-05-17
**Primary addresses:**
- `DrawPixelFXSparkles @ 0x006D7840` — per-frame entry point, called from `TacticalClass_Draw`
- `PixelFXClass::Constructor @ 0x00631E10` and `0x00631E30` — vtable + Init wrappers
- `PixelFXClass::Init @ 0x00631D40` — randomise position/color/timer
- `PixelFXClass::Update_Color @ 0x00631E50` — advance lerp phase, recompute RGB
- `PixelFXClass::Tick_Timer @ 0x00631EE0` — countdown timer, return-true-on-expiry
- `CellClass::IsShrouded_AtMapCoord @ 0x00487950` — shroud check helper
- `TacticalClass_Draw @ 0x006D3D10` — calls `DrawPixelFXSparkles` at end of every render
- `OptionsClass::ApplyFromInGameDialog @ 0x004E1F03` — writes `g_ExtraAnimationsEnabled`
- `OptionsClass::SetDefaults @ 0x005FA370` — initialises `g_ExtraAnimationsEnabled`

**Globals (newly labelled in Ghidra this session):**
- `g_PixelFXParams_Water @ 0x008367C8` — 40-byte parameter struct for water sparkles
- `g_PixelFXParams_Ore @ 0x008367F0` — 40-byte parameter struct for ore sparkles
- `g_ExtraAnimationsEnabled @ 0x00A8EB78` — master enable (options-dialog toggle)

**Confidence:** HIGH — every function in the chain decompiled and read directly; constants tables read from memory; gate conditions verified line-by-line in `DrawPixelFXSparkles`.
**Active in YR:** Yes — runs every frame on every coastal/ore map in stock skirmish, conditional on the "Extra Animations" graphics option (default ON) and primary-surface classifier value `2`. The enrolled local DDrawCompat run maps that value to R5G6B5; this is not a universal claim about other installs or presentation stacks.

---

## 1. Overview — what this system actually does

Every render frame, the game iterates the cells visible in the viewport and draws **one or two animated single-pixel sparkles per water cell and per ore cell** directly to the native 16-bit primary surface (RGB565 in the enrolled local wrapper run). Each cell hosts a small `PixelFXClass` object that owns:
- a sub-pixel offset within its cell's diamond,
- a "base" (dim) RGB color and a "peak" (bright) RGB color,
- a phase counter that smoothly ping-pongs between base and peak,
- a timer for the next color step,
- a per-cell randomised lerp speed.

The sparkle's color and position are recomputed each render frame: phase advances, RGB is recomputed as a linear interpolation between base and peak. When phase completes a full ping-pong cycle, the sparkle re-initialises at a **new random sub-cell position with new color variation**, creating the perception that the twinkle has moved.

The effect is **invisible at full ambient brightness** (the peak color is dim relative to mid-day water and gets washed out) but **dominant at night ambient** because the contrast ratio between the bright peak pixels and the dimmed surrounding water shoots up. This is the "starlight reflecting in the water at night" effect that prompted the investigation.

**This is not animation. Not particles. Not palette cycling. Not TMP-baked pixels.** It is per-frame direct framebuffer pixel writes from a dedicated rendering pass, gated on cell terrain type. The mechanism is entirely separate from the AnimClass system, OreTwinkle/TWNK1 (which is also real but is a separate AnimClass-based per-map-load placement), and the deterministic per-cell tile variant picker.

---

## 2. Top-level call chain

```
Main_Tick @ 0x0055D360                       (per game tick)
  └─ Map__Logic()
  └─ RenderFrame_main @ 0x004F4480           (per render frame)
       └─ TacticalClass_Draw @ 0x006D3D10    (called 3× for layers 0/1/2)
            ├─ Tactical_ZBufferDirtyClear()
            ├─ Tactical_layer_shroud_edges()
            ├─ Tactical_layer_terrain_shadows()
            ├─ Tactical_layer_base_terrain()       (tile blit — the static water TMP pixels)
            ├─ Tactical_layer_smudges()
            ├─ Tactical_layer_building_overlays()
            ├─ Tactical_layer_overlays()
            ├─ Tactical_layer_animations()
            ├─ Tactical_ObjectRenderingLoop()       (units, buildings, etc.)
            ├─ DrawPixelFXSparkles @ 0x006D7840    ◀── THIS is the per-frame sparkle pass
            └─ ... (radar, super weapons, etc.)
```

`DrawPixelFXSparkles` runs **after** the unit/object render but **before** UI overlays — sparkles draw on top of water tiles but underneath cursor/HUD elements. They do NOT draw over units (because of the `cell.FirstObject == 0` gate; see §4).

---

## 3. PixelFXClass struct layout (40 bytes / 0x28)

Verified by reading `PixelFXClass::Init @ 0x00631D40` field-assignment-by-field:

| Offset | Size | Field | Set by | Purpose |
|---|---|---|---|---|
| `+0x00` | 4 | vtable_ptr | Constructor | `&vtable__PixelFXClass` |
| `+0x04` | 4 | CurrentR | Init / Update_Color | Live R component, written to screen each draw |
| `+0x08` | 4 | CurrentG | Init / Update_Color | Live G component |
| `+0x0C` | 4 | CurrentB | Init / Update_Color | Live B component |
| `+0x10` | 4 | Phase | Init / Update_Color | 0 .. 0x2000 lerp counter |
| `+0x14` | 4 | LerpSpeed | Init | Per-ms phase increment (random per cell, from params table) |
| `+0x18` | 4 | PeakR | Init | "Bright" endpoint of the lerp (with per-cell noise subtract) |
| `+0x1C` | 4 | PeakG | Init | |
| `+0x20` | 4 | PeakB | Init | |
| `+0x24` | 4 | BaseR | Init | "Dim" endpoint of the lerp |
| `+0x28` | 4 | BaseG | Init | |
| `+0x2C` | 4 | BaseB | Init | |
| `+0x30` | 4 | OffsetX | Init | Sub-pixel offset within cell, range `-31..32` |
| `+0x34` | 4 | OffsetY | Init | Sub-pixel offset within cell, range `-15..16` |
| `+0x38` | 4 | TimerRemaining | Init / Tick_Timer | Countdown in ms to next color step |

**Storage:** the pointer to this struct lives at `CellClass + 0xFC`. Every water cell and every ore cell will lazily allocate one the first time it's rendered.

**Tiny detail — fields `+0x04..+0x0C` (current) and `+0x24..+0x2C` (base) are initially identical.** Init sets `current = base` so the sparkle starts at the dim endpoint, then Update_Color advances it toward peak.

---

## 4. The nine-condition gate (must all pass for one sparkle to render)

From `DrawPixelFXSparkles`, decoded literally:

```c
1. uVar3 <= DAT_00abcd44                                 // viewport bound (some clip check)
2. (*(int*)(g_PrimarySurface + 0x70))() == 2             // 16-bit RGB565 surface mode
3. g_ExtraAnimationsEnabled != 0                         // Options "Extra Animations" toggle ON
4. (*(int*)(g_PrimarySurface + 0x5C))(0, 0) != 0         // surface lock acquired

   // ... per cell within viewport iteration:

5. *(byte*)(cell + 0x12C) & 0x10 != 0                    // cell is in visible/dirty region
6. !CellClass__IsShrouded_AtMapCoord(cell)               // cell is explored (not under shroud)
7. cell.LandType (+0xEC) == 2                            // Water
   OR Get_Tiberium_Value(cell) != 0                      // ... OR cell has ore
8. cell.FirstObject (+0xE4) == 0                         // no occupant (no unit/building)
9. cell.Flags (+0x140) & 0x1000 == 0                     // some fog/temp flag NOT set
```

If any condition fails, the cell is skipped — no PixelFXClass is even allocated for it. So sparkles don't appear:
- on map editor preview screens (probably condition 1)
- if the player turned off "Extra Animations" in the in-game options
- on shrouded/unexplored cells
- on cells with units or buildings sitting on them (the unit visually occludes the sparkle anyway)
- in 8-bit palettised display mode (legacy, rarely used)

**Viewport iteration bounds** are computed from the camera rectangle:
```
rows = uStack_4[3] / 15 + 17
cols = uStack_4[2] / 60 + 4
```
where `uStack_4[2]` and `uStack_4[3]` are the viewport width and height in cell coords. The `/15` and `/60` reflect the isometric cell pixel dimensions (30 tall, 60 wide).

---

## 5. Parameter tables (read directly from binary at 0x008367C8)

The two parameter blocks are consecutive in memory: water at `0x008367C8`, ore at `0x008367F0`. Each block is exactly 40 bytes / 10 u32 fields. Init indexes them by `is_ore * 0x28`.

### 5.1 Verified raw memory dump

```
0x008367C8 (g_PixelFXParams_Water):
  9E 00 00 00  9E 00 00 00  E0 00 00 00  28 00 00 00
  28 00 00 00  50 00 00 00  05 00 00 00  FF 0F 00 00
  0C 00 00 00  03 00 00 00

0x008367F0 (g_PixelFXParams_Ore):
  FF 00 00 00  FF 00 00 00  F0 00 00 00  B0 00 00 00
  90 00 00 00  00 00 00 00  00 00 00 00  FF 0F 00 00
  1E 00 00 00  0F 00 00 00
```

### 5.2 Decoded fields

| Field offset | Field | Water | Ore |
|---|---|---|---|
| `+0x00` | PeakR  (multiplied by `lerp`) | **158** (0x9E) | **255** |
| `+0x04` | PeakG | **158** | **255** |
| `+0x08` | PeakB | **224** (0xE0) | **240** (0xF0) |
| `+0x0C` | BaseR (multiplied by `inv = 0x1000 - lerp`) | **40** (0x28) | **176** (0xB0) |
| `+0x10` | BaseG | **40** | **144** (0x90) |
| `+0x14` | BaseB | **80** (0x50) | **0** |
| `+0x18` | ColorNoiseShift | **5**  (mask 0x1F → subtract 0..31 per cell) | **0** (no noise) |
| `+0x1C` | TimerInitMask | **0xFFF** (random initial timer 0..4095 ms) | **0xFFF** |
| `+0x20` | LerpSpeedMax | **12** per ms | **30** per ms |
| `+0x24` | LerpSpeedMin | **3** per ms | **15** per ms |

### 5.3 Color interpretation

**Water sparkle:**
- Base (phase=0): RGB(40, 40, 80) — **dark indigo, almost-black blue**
- Peak (phase=0x1000): RGB(158, 158, 224) — **pale lavender-blue**, minus per-cell noise of 0..31 per channel
- LerpSpeed: random 3..12 per ms per cell. Full pulse cycle ≈ 0x2000 / (LerpSpeed × 33ms_per_frame) = **~21..83 frames ≈ 0.7..2.8 seconds** at 30 fps
- Initial timer: random 0..4095 ms (asynchronous start across cells)

**Ore sparkle:**
- Base: RGB(176, 144, 0) — **dark amber/brown**
- Peak: RGB(255, 255, 240) — **near-pure white** (no noise)
- LerpSpeed: random 15..30 per ms per cell. Full cycle ≈ **8..16 frames ≈ 0.3..0.5 seconds** — much faster than water
- Initial timer: random 0..4095 ms

**Subtle design note:** the "Base" color (the dim endpoint of the cycle) is what's drawn when `phase = 0` (sparkle just spawned) and again when `phase = 0x2000` (just before re-Init). The sparkle is at peak brightness only briefly, in the middle of its cycle. So at any given instant, most cells show *dim* sparkles (near base) and a small minority show *bright* (near peak). This is why the effect reads as "scattered random points of light" rather than a uniform glow.

---

## 6. The animation formulas (verified line-by-line)

### 6.1 Init (called once at sparkle creation and again on every cycle complete)

```python
def init(self, is_ore: bool):
    params = g_PixelFXParams_Ore if is_ore else g_PixelFXParams_Water

    r = rand()
    self.OffsetX = (r & 0x3F) - 0x1F           # -31..32, sub-pixel position X
    self.OffsetY = ((r >> 5) & 0x1F) - 0x0F    # -15..16, sub-pixel position Y

    # Peak endpoint with per-cell noise subtract
    peak = [params.PeakR, params.PeakG, params.PeakB]
    if params.ColorNoiseShift > 0:
        mask = (1 << params.ColorNoiseShift) - 1
        r = rand()
        peak[0] -= (mask & r)
        r >>= params.ColorNoiseShift
        peak[1] -= (mask & r)
        r >>= params.ColorNoiseShift
        peak[2] -= (mask & r)
    self.PeakR, self.PeakG, self.PeakB = peak

    # Base endpoint (always pure from table, no noise)
    self.BaseR = params.BaseR
    self.BaseG = params.BaseG
    self.BaseB = params.BaseB
    self.CurrentR = self.BaseR   # initial color = base (dim)
    self.CurrentG = self.BaseG
    self.CurrentB = self.BaseB

    self.Phase = 0
    self.LerpSpeed = randint(params.LerpSpeedMin, params.LerpSpeedMax)  # inclusive
    self.TimerRemaining = rand() & params.TimerInitMask                  # random 0..mask ms
```

### 6.2 Tick_Timer

```python
def tick_timer(self, delta_ms: int) -> bool:
    self.TimerRemaining -= delta_ms
    if self.TimerRemaining < 1:
        self.TimerRemaining = 0
        return True   # caller should advance phase + redraw
    return False
```

The timer is **not** reset to a new random value on expiry — it just sits at 0, so once a sparkle "starts ticking", it updates every frame until the phase cycle completes and Init re-runs (which picks a new random TimerRemaining).

### 6.3 Update_Color (ping-pong lerp)

```python
def update_color(self, delta_ms: int):
    self.Phase += self.LerpSpeed * delta_ms
    if self.Phase > 0x2000:
        self.Phase = 0x2000

    lerp = self.Phase & 0xFFF
    if (self.Phase & 0x1000):
        lerp = 0x1000 - lerp           # ping-pong: second half mirrors back
    inv = 0x1000 - lerp

    self.CurrentR = (self.BaseR * inv + self.PeakR * lerp) >> 12
    self.CurrentG = (self.BaseG * inv + self.PeakG * lerp) >> 12
    self.CurrentB = (self.BaseB * inv + self.PeakB * lerp) >> 12
```

**Phase → output color map:**

| Phase | `lerp` | `inv`  | Output color | Description |
|---|---|---|---|---|
| `0x0000` | 0 | 0x1000 | Base | Sparkle just spawned (dim) |
| `0x0800` | 0x800 | 0x800 | (Base + Peak) / 2 | Mid-rising |
| `0x1000` | 0x1000 | 0 | Peak | Brightest point — visible "twinkle" |
| `0x1800` | 0x800 (flipped from 0x1000-0x800) | 0x800 | (Base + Peak) / 2 | Mid-falling |
| `0x1FFF` | 0xFFF (flipped to 1) | 0xFFF | ≈ Base | Just before re-Init |
| `0x2000+` | — | — | (re-Init triggers in caller) | Cycle complete |

So one sparkle's "lifetime" in the cell:
1. Spawn at random sub-pixel position with Base color (dim).
2. Slowly brighten to Peak as phase advances 0 → 0x1000.
3. Slowly fade back toward Base as phase advances 0x1000 → 0x1FFF.
4. Re-Init: new random sub-pixel position, new color noise, new lerp speed, new timer. Phase resets to 0. Visible result: the sparkle "moves" to a new spot within the same cell.

### 6.4 The render-loop wrapper (in `DrawPixelFXSparkles`)

```python
def draw_pixel_fx_sparkles(delta_ms):
    for cell in viewport_cells:
        if not passes_nine_condition_gate(cell):
            continue

        if cell.PixelFX is None:
            cell.PixelFX = PixelFXClass(is_ore=(get_tiberium_value(cell) != 0))
            # Constructor calls Init(is_ore) internally

        if cell.PixelFX.Phase > 0x1FFF:
            cell.PixelFX.init(is_ore=(get_tiberium_value(cell) != 0))   # restart cycle

        if cell.PixelFX.tick_timer(delta_ms):
            screen_pos = cell.screen_position() + (
                cell.PixelFX.OffsetX, cell.PixelFX.OffsetY
            )
            cell.PixelFX.update_color(delta_ms)

            # Direct runtime-format 16-bit framebuffer write
            r = cell.PixelFX.CurrentR
            g = cell.PixelFX.CurrentG
            b = cell.PixelFX.CurrentB
            pixel = (
                ((r >> g_DD_RLoss) << g_DD_RShift) |
                ((g >> g_DD_GLoss) << g_DD_GShift) |
                ((b >> g_DD_BLoss) << g_DD_BShift)
            )
            surface[screen_y * pitch + screen_x * 2] = pixel
```

`g_DD_RLoss/GLoss/BLoss` and `g_DD_RShift/GShift/BShift` are the runtime-derived DirectDraw channel loss/shift values loaded from the surface description. They describe RGB565 in the enrolled local run, but the general mechanism remains descriptor-derived.

---

## 7. CellClass field used by this system

Already known fields plus the new `PixelFX*` slot:

| Offset | Type | Field | Notes |
|---|---|---|---|
| `+0xE4` | `ObjectClass*` | FirstObject | NULL → cell has no occupant; sparkle gate condition |
| `+0xEC` | `i32` | LandType | `== 2` for Water (this is the canonical water test) |
| `+0xFC` | `PixelFXClass*` | **PixelFX** | per-cell sparkle pointer; lazy-init |
| `+0x12C` (300) | `u8` | flags_lo | bit 0x10 = visible/dirty (sparkle gate) |
| `+0x140` | `u32` | flags | bit 0x1000 NOT set (sparkle gate) |

`Get_Tiberium_Value` is a separate method returning the cell's ore overlay value (0 if no ore).

---

## 8. The `g_ExtraAnimationsEnabled` toggle (`0x00A8EB78`)

Verified by xref dump:

**Written by:**
- `OptionsClass::ApplyFromInGameDialog` @ `0x004E1F10`
- `OptionsClass::ApplyFromLauncherDialog` @ `0x0055FAE0`
- `OptionsClass::SetDefaults` @ `0x005FA370` (initial default)

**Read by (every "fancy graphics" path):**
- `DrawPixelFXSparkles` @ `0x006D786C` ← this report's subject
- `LaserDrawClass::Draw` @ `0x00550611`
- `LaserDrawClass::DrawBeamSpecial` @ `0x00550BD7`
- `LineTrail::SetColorDecrement` @ `0x00556B50`
- `ParticleSystemClass::AI_Spark` @ `0x0062EBF2`
- `ParticleClass::Draw_It` @ `0x0062CEEA` and `0x0062CFBB`
- `AnimClass::DrawIt` @ `0x00423000` and `0x00423083`
- Several `FUN_*` paths in `OptionsClass` and related UI

This is the **"Extra Animations" / "Show Effects"** checkbox in YR's in-game Options dialog. It gates all fancy visual effects globally:
- water + ore sparkles (this system)
- laser beam visual effects
- particle systems (sparks, smoke variations)
- line trails (movement trails on units)
- some animation passes

When unchecked, you get a more austere visual presentation — no sparkles, simpler laser beams, no particle showers. Default is ON in stock YR. The control existed in 2000-2001 for low-spec hardware.

---

## 9. The `IsShrouded` helper (`0x00487950`, now labelled `CellClass__IsShrouded_AtMapCoord`)

Wraps the cell's MapCoord into a world-coordinate, calls `IsShrouded`, returns the result. Used by `DrawPixelFXSparkles` to skip sparkles on cells under shroud (unexplored areas show as black; rendering sparkles there would look out of place).

```c
void CellClass__IsShrouded_AtMapCoord(CellClass *cell)
{
    short coord_packed = cell + 0x24;      // MapCoord X/Y packed
    int world_x = (coord_packed_low * 0x100) + 0x80;
    int world_y = (coord_packed_high * 0x100) + 0x80;
    return IsShrouded(world_x, world_y);
}
```

**Note:** Ghidra's auto-decompile types this as void-return, but the assembly clearly forwards the IsShrouded return. The caller in `DrawPixelFXSparkles` does treat it as a char-returning predicate (`cVar1 = FUN_00487950(); if (cVar1 == '\0' ...)`). Ghidra's `void` typing is a quirk.

---

## 10. Active in YR — Yes / No / Conditional

| Subsystem | Active in YR? | Trigger frequency |
|---|---|---|
| `DrawPixelFXSparkles` per render frame | Yes | Every frame, ~30-60 fps |
| Water sparkles | Conditional | Every water cell visible, when `g_ExtraAnimationsEnabled != 0` AND 16-bit display mode AND not shrouded AND no occupant |
| Ore sparkles | Conditional | Every ore cell visible, same conditions |
| Non-classifier-2 surface mode disables it | Yes | If `g_PrimarySurface.mode != 2`, the whole pass returns early; the enrolled local runtime's classifier-2 format is RGB565 |
| `g_ExtraAnimationsEnabled = 0` disables it | Yes | Options dialog toggle |
| Shrouded cells | Suppressed | `IsShrouded` returns true → cell skipped |
| Occupied cells | Suppressed | `cell.FirstObject != 0` → skipped (sparkle would be occluded anyway) |

**No TS-legacy gating found.** This system is fully live in stock YR. Per the parity bar, this is observable output and must be reproduced for visual parity.

---

## 11. Current Rust implementation status

Mapped against `src/`:

| System | Rust file | Status |
|---|---|---|
| Per-frame water sparkle render | — | **Missing.** No equivalent of `DrawPixelFXSparkles`. |
| `PixelFXClass` per-cell state | — | **Missing.** No per-cell sparkle struct. |
| `g_ExtraAnimationsEnabled` toggle | — | **Missing.** No "Extra Animations" config option. |
| `g_PixelFXParams_Water/Ore` constants | — | **Missing.** Constants need to be ported. |
| Ping-pong lerp animation formula | — | **Missing.** |
| Sub-pixel offset within cell rendering | (partial) | We have sub-cell positioning for infantry but not for free-form pixel sparkles. |
| Shroud check on render | (varies) | The shroud system is implemented; need to wire it into the sparkle gate. |
| `cell.LandType` query | `src/sim/pathfinding/passability.rs` | **Present.** Our `LandType::Water` (Rust value 4, remapped from gamemd's value 2) is the equivalent. |
| `Get_Tiberium_Value` | (partial) | We have ore overlay handling but no "value" accessor for sparkle gating. |
| Runtime-format framebuffer writes | `src/render/tactical_compat.rs` (partial packed-format primitive only) | **Missing for PixelFX.** A wgpu sprite may be the final draw primitive, but exact local output still requires the runtime-derived packed value and guard-proven RGB565 presentation expansion before the write can be called equivalent. |

---

## 12. Suggested Rust implementation

**Module:** `src/render/pixel_fx_sparkles.rs`

```rust
//! Per-frame water/ore sparkle rendering — observable parity with
//! gamemd's DrawPixelFXSparkles. See ra2-rust-game-docs/PIXEL_FX_SPARKLES_GHIDRA_REPORT.md
//! for the full reverse-engineering of the source system.

const WATER_PARAMS: SparkleParams = SparkleParams {
    base_rgb: [40, 40, 80],
    peak_rgb: [158, 158, 224],
    color_noise_bits: 5,
    lerp_min: 3,
    lerp_max: 12,
    timer_init_mask: 0xFFF,
};

const ORE_PARAMS: SparkleParams = SparkleParams {
    base_rgb: [176, 144, 0],
    peak_rgb: [255, 255, 240],
    color_noise_bits: 0,
    lerp_min: 15,
    lerp_max: 30,
    timer_init_mask: 0xFFF,
};

pub struct CellSparkle {
    base_rgb: [u8; 3],
    peak_rgb: [u8; 3],
    current_rgb: [u8; 3],
    offset_x: i8,         // -31..32 sub-cell
    offset_y: i8,         // -15..16 sub-cell
    phase: u16,           // 0..0x2000
    lerp_speed: u8,       // per ms
    timer_ms: u16,
}

impl CellSparkle {
    fn init(is_ore: bool, rng: &mut impl Rng) -> Self { /* ... */ }
    fn tick_timer(&mut self, delta_ms: u16) -> bool { /* ... */ }
    fn update_color(&mut self, delta_ms: u16) { /* ping-pong lerp */ }
}
```

**Per-cell storage:** Add `Option<CellSparkle>` to `ResolvedTerrainCell` (or a parallel `Vec<Option<CellSparkle>>` indexed by cell). Lazy-init on first render with sparkle conditions met.

**Per-frame update + render pass:** in the existing terrain render path, after the tile blit and before unit pass:

```rust
fn render_pixel_fx_sparkles(
    cells: &mut [ResolvedTerrainCell],
    viewport: ViewportRect,
    delta_ms: u16,
    extra_animations_enabled: bool,
    sprite_batcher: &mut SpriteBatcher,
) {
    if !extra_animations_enabled {
        return;
    }
    for cell in viewport.visible_cells(cells) {
        if cell.is_shrouded() { continue; }
        if cell.first_object.is_some() { continue; }
        let is_water = cell.land_type == LandType::Water;
        let is_ore = cell.tiberium_value > 0;
        if !is_water && !is_ore { continue; }

        let sparkle = cell.sparkle.get_or_insert_with(|| CellSparkle::init(is_ore, rng));
        if sparkle.phase > 0x1FFF {
            *sparkle = CellSparkle::init(is_ore, rng);
        }
        if sparkle.tick_timer(delta_ms) {
            sparkle.update_color(delta_ms);
            let screen_pos = cell.screen_position()
                + Vec2::new(sparkle.offset_x as f32, sparkle.offset_y as f32);
            sprite_batcher.emit_point(screen_pos, sparkle.current_rgb);
        }
    }
}
```

**Sprite emission:** the cheapest way to render single-pixel sparkles in our wgpu pipeline is to add a small 1×1 point-sprite instance to an existing batch. Alternative: dedicated point pipeline (faster at scale but more plumbing). Either works for parity.

**Determinism for lockstep:** seed the per-cell RNG by `(cell_x, cell_y, sparkle_lifetime_counter)` so all clients see identical sparkles. The original engine doesn't sync sparkles (they're cosmetic), but our port should match across replays.

**Estimated work:** ~200 lines including struct, init/tick/lerp, render integration, and config. No shader changes required.

---

## 13. Visual parity expectations

After implementing this, the engine should show on any night-ambient water map:

1. **Tiny pale-blue dots** scattered across visible water, one per ~cell (sometimes none if conditions fail).
2. **Each dot pulses** from very dim (almost invisible) → pale blue → dim, over 0.7-2.8 seconds.
3. **Different dots are at different phases** — when one is bright, its neighbor might be mid-fade, etc.
4. **Sparkles "move"** between cycles — when a sparkle fades out, it reappears at a different sub-pixel position within the same cell.
5. **Ore patches** have similar but **brighter, more amber/white** sparkles cycling **faster**.
6. **Sparkles disappear** when units pass over the cell (occupant gate), and reappear when the unit moves off.
7. **No sparkles** if the player turns off "Extra Animations" in Options.

This should produce the visual that originally prompted the user's question: "stars reflecting in the water at night."

---

## 14. Open question resolutions (close-out 2026-05-17)

Three of the four open questions from the initial report were closed in that
session. The fourth (`g_PrimarySurface` vtable+0x70 == 2 → RGB565) was deferred
then and is now resolved for the enrolled runtime in section 14.4; PixelFX's
own production differential remains open.

Confidence is reported on three axes per `feedback_research_confidence_axes`:
- **Content** — does the cited behaviour actually match what the function does?
- **Identity** — is the cited address / field / global the right one?
- **Binding** — are the cited writers/callers actually wired to fire this code
  path in a normal YR skirmish?

### 14.1 `cell + 0x12C` bit 0x10 — RESOLVED

**Semantic:** "Cell is currently within at least one of the local player's
units' sight radius." Set when a reveal-circle helper touches the cell;
cleared when the corresponding conceal-circle helper runs and no unit's sight
covers the cell anymore. Ghidra's auto-derived label calls this "needs redraw"
— that name reflects what `Invalidate_Radius_For_Redraw` does, but the
operational behaviour (reveal-circle / conceal-circle pair) is more accurately
described as "currently in sight."

**Content: HIGH** — the gate site in `DrawPixelFXSparkles` is
`*(byte *)(iVar10 + 300) & 0x10` (decompile at `0x006D7840`); confirmed live
this session. Skipping cells where the bit is clear gives the observed in-game
behaviour: sparkles only animate inside the player's current sight area, never
on cached terrain outside any unit's sight.

**Identity: HIGH** — `+0x12C` is the 32-bit `ShroudFlags` field per
`CELLCLASS_STRUCT_GHIDRA_REPORT.md` row at +0x12C. The complete bit-map
investigation in [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) §E confirms only bits 3 and 4
are ever read or written across the full shroud pipeline (bits 0,1,2,5..31
unobserved). Bit 4 = `0x10` is unambiguously the gate bit.

**Binding: HIGH** — writers and clearers all decompiled live this session or
verified against the existing §E evidence matrix:

| Op | Site | Bits | Verified this session |
|---|---|---|---|
| `OR 0x10` then `OR 0x08` (reveal radius) | `MapClass::Invalidate_Radius_For_Redraw @ 0x00568140` | 4, 3 | Yes — full decomp read. Clears `+0x130`/`+0x134` (gap counters) then ORs both bits per cell in a circle. |
| `OR 0x18` (mass reveal) | `CellClass::RevealShroudFlags @ 0x004876F0` | 3, 4 | §E |
| `OR 0x18` (blackout) | `MapClass::BlackoutShroud @ 0x00577D90` | 3, 4 | §E |
| `AND ~0x08` then `AND ~0x10` (conceal radius) | `FUN_00567F70` | 3, 4 | Yes — full decomp read. Inverse of `Invalidate_Radius_For_Redraw`: only acts on cells whose `GapConcealmentCounter` is 0; clears both bits per cell in a circle. |
| `AND ~0x18` | `MapClass::ResetShroud @ 0x00577BB0`, `RecalcBridgeShroudFlags @ 0x00578100` | 3, 4 | §E |

**Rust mapping:** gate sparkles by the existing per-player sight bitmap or
"currently visible to local player" cell state. We re-rasterise the whole
viewport every frame, so there is no separate dirty-bit to mirror — the gate
semantic collapses to a sight-radius check.

### 14.2 `cell + 0x140` bit 0x1000 — RESOLVED (original "fog flag" guess was WRONG)

**Semantic:** "BridgeDeckCell" — marks the cells underneath a bridge platform
(anchor + 3 forward span cells). It is NOT a fog-of-war flag, despite my
initial guess.

**Content: HIGH** — the gate site in `DrawPixelFXSparkles` is
`(*(uint *)(iVar10 + 0x140) & 0x1000) == 0` (decompile, verified live).
Skipping cells where bit 12 is set means sparkles don't draw on cells under a
bridge deck — they would otherwise produce single bright pixels poking
through the bridge sprite.

**Identity: HIGH** — `+0x140` is the 32-bit `Flags` field per
`CELLCLASS_STRUCT_GHIDRA_REPORT.md` row at +0x140; bit 12 is listed there as
"BridgeDirectionBit" with MED confidence. This investigation upgrades the bit
to "BridgeDeckCell" with HIGH confidence based on direct decompile read.

**Binding: HIGH** — `CellClass::SetBridgeDirection_NESW @ 0x0047E040` (and
byte-identical `_NWSE @ 0x0047E470`) is the sole writer of bit 12 at +0x140.
Decompile read live this session shows:

- **Anchor + forward-1 + forward-2:** `Flags = Flags & wide_mask |
  (param_3 & 1) << 0xC | (other bridge bits)`. Sets bit 12 when constructing
  a bridge (`param_3 != 0`); clears when destroying (`param_3 == 0`).
- **Forward-3 (end of span):** `Flags = Flags & 0xFFFFEFFF | (param_3 & 1)
  << 0xC` — ONLY bit 12 is modified on this cell.
- **Opposite-direction cell (= far-side bridgehead landing):** mask clears
  bit 12 with NO compensating OR. Bridgehead cells do NOT carry bit 12.
- **Param_2 == 6 special case:** an additional cell receives a bit-16 update
  (NOT a bit-12 update). Does not affect bit 12 semantics.

**Caller chain verified** for both `_NESW` and `_NWSE`: `MapClass::Resize
@ 0x00565C10` (map load), `OverlayClass::Mark @ 0x005FC570`, the
`MapClass__UpdateRamp_{NS,EW}_Collapse{A,B}_{High,Low}` family, and
`ProcessBridgeDamageStateMachine_{High,Low}` (bridge damage / repair
pipeline). All bridge-related; no non-bridge writer of bit 12 exists in the
binary.

**Rust mapping:** skip sparkle on cells whose `bridge_state` marks them as
bridge-deck cells (already tracked for pathfinding and rendering). Bridgehead
landing cells are land terrain and already fail the water/ore test, so no
extra check needed for them.

### 14.3 OreTwinkle × PixelFX co-existence — RESOLVED

**They are two independent systems that both run on ore cells.** No conflict,
no shared state. Each contributes a distinct visual; the combination is what
gives ore patches their characteristic shimmer.

**Content: HIGH** — both systems' algorithms decompiled live this session.

**OreTwinkle (TWNK1 by INI convention):**

| Item | Address / value |
|---|---|
| `[General] OreTwinkleChance` INI key string | `0x0083A1CC` |
| Reader of that string | `RulesClass::ReadGeneral` xref at `0x0066D67B` |
| `[AudioVisual] OreTwinkle` INI key string | `0x0083CF4C` |
| Reader of that string | `RulesClass::ReadAudioVisual` xref at `0x0066B805` |
| `RulesClass + 0x186C` | `OreTwinkleChance` (int, 1-in-N probability) |
| `RulesClass + 0x1870` | `OreTwinkle` (AnimType pointer; default "TWNK1") |
| Spawner function | `FUN_00684C30` |
| Spawner callers (verified) | `ScenarioClass::Read_Scenario @ 0x00684620` (post map-load init), `CCINIClass::Constructor @ 0x00599650` |

Spawner behaviour: iterates every cell via `MapClass__CellIterator_Init/Next`;
for each cell with `CellClass::Get_Tiberium_Value() != 0`, rolls
`Random__RandomRanged(0, OreTwinkleChance - 1)` and if zero, calls
`AnimClass__Constructor(OreTwinkle_type_ptr, cell_coord, ...)`. Resulting
`AnimClass` instances persist and animate via the AnimClass per-tick update.
This is a **one-time scenario-load pass.**

**PixelFX (this report's subject):**
- Per-frame direct-pixel render pass at `0x006D7840`.
- Runs on every visible ore cell (no probability gate beyond the 9-condition
  visibility gate in §4).
- Stores per-cell state at `cell + 0xFC`, completely separate from any
  `AnimClass` instance the cell may host.
- Drawn via direct framebuffer pixel writes, not via the AnimClass render
  path.

**Identity: HIGH** — string xrefs traced to their `RulesClass` readers;
spawner caller chain traced to `Read_Scenario`; AnimClass constructor call
inside the spawner verified by direct decompile read of `FUN_00684C30`.

**Binding: HIGH** — independent code paths, independent per-cell storage
(AnimClass instance list vs `cell + 0xFC` PixelFXClass pointer), no shared
state observed during decompile. Ore cells that hosted a TWNK1 AnimClass at
load will also produce a per-frame PixelFX sparkle.

**Rust mapping:** implement both, independently:
- **OreTwinkle:** on scenario load (after ore overlays resolved), iterate ore
  cells; with probability `1 / OreTwinkleChance` spawn an animation entity
  using whatever AnimType the rules name (`OreTwinkle=TWNK1` by default).
  Plugs into our existing animation system; no shared logic with the PixelFX
  module.
- **PixelFX:** the per-frame render pass per §12 of this report.

### 14.4 Active local pixel format and Rust constraint — RESOLVED FOR ENROLLED RUNTIME

Current evidence resolves the practical local-runtime question. Live
decompilation shows that `DSurface__Constructor @ 0x004BA770` derives the
shift/loss globals from the DirectDraw descriptor and classifies R5G6B5 as
value `2` (`RShift=11`, `RLoss=3`, `GShift=5`, `GLoss=2`, `BShift=0`,
`BLoss=3`). The installed `DDrawCompat-gamemd.log:166-168` independently
records `D3DDDIFMT_R5G6B5` for plain, primary, and system-memory resources in
the enrolled `gamemd.exe` run.

RGB565 is therefore verified for this local DDrawCompat-backed presentation,
not universal gamemd behavior: the binary still derives the format at runtime
and separately supports RGB555. Rendering through true-colour wgpu is not an
exemption from the native-format constraint. Exact PixelFX and final-frame
parity must preserve the runtime-derived packed component values and prove
their expansion into capture bytes; a generic RGBA sprite write is not
automatically equivalent. Three sealed shell frames from the enrolled
AMD/DDrawCompat/DXGI guard currently corroborate the local presentation domain
with exactly 32 red/blue and 64 green values, but that codebook is
environment-scoped and PixelFX itself still needs its own production
differential.

---

## 15. Ghidra symbols updated this session

To make future investigation easier, these symbols have been renamed in the live Ghidra database (and the program was saved):

| Address | Old name | New name |
|---|---|---|
| `0x00487950` | `FUN_00487950` | `CellClass__IsShrouded_AtMapCoord` |
| `0x00A8EB78` | `DAT_00a8eb78` | `g_ExtraAnimationsEnabled` |
| `0x008367C8` | (unlabelled) | `g_PixelFXParams_Water` |
| `0x008367F0` | (unlabelled) | `g_PixelFXParams_Ore` |

Plate comments documenting the algorithm have been added to:
- `DrawPixelFXSparkles @ 0x006D7840`
- `PixelFXClass::Init @ 0x00631D40`
- `PixelFXClass::Update_Color @ 0x00631E50`
- `PixelFXClass::Tick_Timer @ 0x00631EE0`

`PixelFXClass::Constructor` (both at `0x00631E10` and `0x00631E30`) were already pre-labelled by Ghidra and confirmed trivial (vtable assignment + delegate to Init).

---

## 16. Sources

**Ghidra functions decompiled this session:**
- `0x004F4480` — `RenderFrame_main` (call chain)
- `0x006D3D10` — `TacticalClass_Draw` (call chain, found `DrawPixelFXSparkles` invocation)
- `0x006D7840` — `DrawPixelFXSparkles` (full body — the per-frame sparkle entry)
- `0x00487950` — `CellClass__IsShrouded_AtMapCoord` (shroud gate helper)
- `0x00631D40` — `PixelFXClass::Init` (full body — per-sparkle state init)
- `0x00631E10` — `PixelFXClass::Constructor` (0-arg variant)
- `0x00631E30` — `PixelFXClass::Constructor` (1-arg variant)
- `0x00631E50` — `PixelFXClass::Update_Color` (ping-pong lerp)
- `0x00631EE0` — `PixelFXClass::Tick_Timer` (countdown)
- `0x00543E50` — palette-clear function (confirmed NOT cycle code, just blackout for fades)
- `0x00543E30` — small initialiser (set 3 globals)
- `0x00543F10` — `BridgeShadowTable_StaticInit` (unrelated, found while ruling out vtable hypothesis)
- `0x00545000` — palette/LightConvert fixup post-load
- `0x00545150` — `Read_Theater_TileSets_INI` (palette load — only writes to `DAT_00abbed0` once)
- `0x0055D360` — `Main_Tick` (per-frame entry point)

**Memory tables read directly:**
- `0x008367C8` — water sparkle params (40 bytes verified)
- `0x008367F0` — ore sparkle params (40 bytes verified)
- `0x00839D68` — terrain section name pointer table (cross-referenced for LandType enum)

**XRef searches:**
- xrefs to `0x00abbed0` (iso palette static buffer): ONE write outside theater load — the blackout function. **No per-frame palette cycling exists** — verified.
- xrefs to `0x00A8EB78` (`g_ExtraAnimationsEnabled`): set by `OptionsClass` writers; read by every fancy-effect path. Confirms it's the in-game "Extra Animations" toggle.
- xrefs to `0x00A8ED84` (`g_CurrentFrameCounter`): used in many places, traced to `Main_Tick` as the source of truth.

**Companion docs cross-referenced:**
- [SEA_TILES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SEA_TILES_GHIDRA_REPORT.md) (same session, earlier) — for cell field offsets and water tile data flow
- `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md` — for the static tile pixel data (ruled out as the source of "twinkling" — pixels are static, not cycled)
- `CELLCLASS_STRUCT_GHIDRA_REPORT.md` — for cell+0x140 flag bit assignments

---

### 16.1 Close-out session (2026-05-17) — §14 question-closure work

**Additional Ghidra functions decompiled live this session:**
- `0x00568140` — `MapClass::Invalidate_Radius_For_Redraw` (the reveal-circle helper). Confirms `+0x12C` bit 4 = `0x10` is set per cell in a radius around the input point, along with `+0x12C` bit 3 = `0x08`; gap counters at `+0x130/+0x134` are zeroed first. Despite the Ghidra label, operationally this is "reveal in sight radius."
- `0x00567F70` — conceal-circle counterpart. For cells in a radius whose `GapConcealmentCounter` is 0, sets the counter to 1 then ANDs `+0x12C` with `~0x08` and `~0x10` — i.e., clears both bits 3 and 4 together. Only caller: `FUN_006E1A70` (shroud pipeline).
- `0x0047E040` — `CellClass::SetBridgeDirection_NESW`. Sole writer of `+0x140` bit 12 = `0x1000`. Anchor + forward-1/2/3 receive the bit when constructing (`param_3 != 0`), the third forward cell has only bit 12 modified, and the opposite-direction bridgehead has bit 12 explicitly cleared. Byte-identical twin at `0x0047E470` (`_NWSE`). Confirms bit 12 = "BridgeDeckCell," NOT "fog flag."
- `0x00684C30` — OreTwinkle spawner. Called from `ScenarioClass::Read_Scenario @ 0x00684620` and `CCINIClass::Constructor @ 0x00599650` (verified via `get_function_callers`). Iterates cells, rolls `Random__RandomRanged(0, OreTwinkleChance - 1)` per ore cell, spawns an `AnimClass(OreTwinkle, cell_coord, ...)` on a 1-in-N hit. One-time scenario-load pass.
- `0x004197C0` — `AircraftClass::Find_Approach_Cell`. Reader site for `+0x12C` bit 0x10 used in landing-cell selection: `g_GameMode != 0 || unit_flag || cell+0x12C & 0x10`. Confirms semantic = "currently visible to local player."
- `0x006F5090` — temporal/teleport callback (unnamed). Reader site for `+0x12C` bit 0x10: discovers the unit to the player when their current cell is currently in sight.
- `0x00440580` — `BuildingClass::Unlimbo`. Reader site for `+0x12C` bit 0x10 used to gate the "construction complete" discovery hook on the placement cell.

**Additional string xrefs:**
- "OreTwinkleChance" string @ `0x0083A1CC` → read by `RulesClass::ReadGeneral` at `0x0066D67B` (slot `RulesClass+0x186C`).
- "OreTwinkle" string @ `0x0083CF4C` → read by `RulesClass::ReadAudioVisual` at `0x0066B805` (slot `RulesClass+0x1870`).

**Byte-pattern searches that informed the conclusions:**
- `OR/AND` byte-imm writers to `+0x12C` byte: no direct simple-mask matches found — the bit is set via a 32-bit `OR` of multiple bits inside `Invalidate_Radius_For_Redraw` and `RevealShroudFlags`, not via a single-byte literal. Initial search misses confirmed the bit isn't written via a simple `OR byte [reg+0x12C], 0x10` anywhere.
- `OR/AND` byte-imm writers to `+0x140` low/high bytes for bit 12: no direct matches — bit 12 is also set via 32-bit compound `AND-OR` inside `SetBridgeDirection_NESW`, consistent with the function being the sole writer.
- `TEST byte [reg+0x12C], 0x10` (read pattern): 4 hits at `0x00419986`, `0x00440D44`, `0x006F5159`, `0x00749B3D` — first three are the AircraftClass/BuildingClass/teleport-callback readers above; the fourth address has no enclosing function in the current Ghidra DB (likely in an unanalyzed region).

**Companion docs cross-referenced this session:**
- `CELLCLASS_STRUCT_GHIDRA_REPORT.md` — `+0x12C` row (ShroudFlags, bits 3+4 only) and `+0x140` row (Flags bit 12 listed MED as "BridgeDirectionBit"; upgraded here).
- [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) §E — full bit-map evidence matrix for `+0x12C` bits 3 and 4.

**Symbols renamed this session in the live Ghidra DB (post-investigation update):**

| Address | Old name | New name | Confidence |
|---|---|---|---|
| `0x00568140` | `FUN_00568140` | `MapClass__Invalidate_Radius_For_Redraw` | HIGH — matches the name already used in [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) §E; body unambiguously matches. |
| `0x00567F70` | `FUN_00567F70` | `MapClass__Conceal_Radius` | HIGH — exact inverse of the above (same loop shape, opposite bit ops). |

**Plate comments added (no rename):**

| Address | Identified role | Why no rename |
|---|---|---|
| `0x00568140` | (see rename above; comment also added) | n/a |
| `0x00567F70` | (see rename above; comment also added) | n/a |
| `0x00684C30` | Post-map-load scenario init (includes OreTwinkle anim spawn pass) | Function does many things; partial-purpose name would mislead. Likely full name: `ScenarioClass::Finalize_Map_Init` or `MapClass::Post_Load_Setup`. |
| `0x006E1A70` | `TriggerAction::Execute` case 0x65 (action 101 decimal) handler — conceals shroud in a radius around a target waypoint. | Trigger action enum names for IDs ≥ 0x65 are not present as strings in the binary. Would need the trigger-action descriptor table to confirm the canonical action name. |
| `0x006F5090` | `FootClass::PerCellProcess` tail callback — runs cell-entry post-processing (temporal detach, ProcessCellAction(0x22), first-time playfield mark, first-time discover-to-player). | Confident on role but the canonical method name (e.g., `FootClass::On_Enter_Cell_Final`) is not derivable from binary strings. |

Program saved.

**Investigation history:**
- Started by hunting for palette cycling (classic Westwood trick). Ruled out — no per-frame writes to the iso palette.
- Then hunted for TMP-baked pixels. Found bright pixels in Water07/08/14, but those are STATIC — not the source of the perceived twinkle.
- Final pass found `DrawPixelFXSparkles` via the TacticalClass_Draw call chain after explicit per-frame-render-function tracing.

**Iteration that paid off:** searching for functions by descriptive name pattern (`Blitter`, `Tactical`, etc.) revealed many labelled functions that string-only searches missed. `DrawPixelFXSparkles` had an explicit Ghidra label that I'd never have found via keyword string search.

---

*End of report. The water/ore twinkle in stock YR is a per-frame, per-cell direct-framebuffer pixel renderer with its own struct, its own constants table, and its own gate conditions — independent of every other animation system in the engine. The mechanism is fully documented and ready to port.*
