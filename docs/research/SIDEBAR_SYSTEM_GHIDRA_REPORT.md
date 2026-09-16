# Sidebar System in gamemd.exe — Complete Ghidra Research Report

Source: live decompilation of `gamemd.exe` via Ghidra MCP.
All constants verified by decompiling raw C code from each function.
Original source file: `D:\ra2mdpost\Sidebar.CPP`

---

## 1. Class Hierarchy (RTTI-confirmed)

```
GScreenClass         .?AVGScreenClass@@
  └─ MapClass        .?AVMapClass@@
      └─ DisplayClass .?AVDisplayClass@@
          └─ RadarClass .?AVRadarClass@@
              └─ PowerClass .?AVPowerClass@@
                  └─ SidebarClass .?AVSidebarClass@@
                      └─ TabClass .?AVTabClass@@
                          └─ ScrollClass .?AVScrollClass@@
```

Nested inner classes:
- `SidebarClass::SBGadgetClass` — RTTI at `0x0083f8d0` — sidebar button/gadget widget
- `SidebarClass::StripClass::SelectClass` — RTTI at `0x0083f900` — individual cameo click zone

Related command classes:
- `SidebarUpCommandClass` — RTTI at `0x008269a8`
- `SidebarDownCommandClass` — RTTI at `0x00826ef8`

### Total Object Size and Global Instance

The mega-class (GScreenClass through MouseClass) is **0x556C bytes (21,868 bytes)**.
Global singleton instance: **`0x0087F7E8`** (spans to ~`0x008D4D54`).
Initialized by static constructor at `0x0040D190`.

### Inheritance Chain with Field Ranges

```
GScreenClass     ctor: 0x004F4220  vtable: 0x7EA6FC  fields: +0x04..+0x0C
  MapClass       ctor: 0x00565090  vtable: 0x7ED404  fields: +0x14..+0x1170
    DisplayClass ctor: 0x004A8730  vtable: 0x7E6114  fields: +0x1174..+0x11E0
      RadarClass ctor: 0x00652960  vtable: 0x7F0344  fields: +0x11E8..+0x1508
        PowerClass ctor: 0x0063F6B0  vtable: 0x7EFF54  fields: +0x150C..+0x1540
          SidebarClass ctor: 0x006A4F20  vtable: 0x7F3058  fields: +0x1544..+0x5515
            TabClass ctor: 0x006CFE20  vtable: 0x7EDFB4  fields: +0x5518..+0x5546
              ScrollClass ctor: 0x00692290  vtable: 0x7F1094  fields: +0x5548..+0x555A
                MouseClass ctor: 0x005BDA40  vtable: 0x7E1964  fields: +0x555C..+0x5568
```

Secondary vtable at offset **+0x5518** (for TabClass base, offset 0x5518 = 21784 from COL).

### SidebarClass Primary Vtable (`0x007F3058`) — 55 Virtual Methods

| Idx | Address | Name |
|---|---|---|
| 0 | `0x004F4240` | QueryInterface |
| 4 | `0x006AC7F0` | Destructor |
| 5 | `0x006A5000` | One_Time (loads DARKEN.SHP) |
| 7 | `0x006A5030` | Init_Clear |
| 8 | `0x006A5310` | Init_IO (full sidebar init) |
| 10 | `0x006A7780` | AI (command processing; formerly mislabeled Action) |
| 16 | `0x006A6C30` | **Draw** |
| 30 | `0x006AC5D0` | Save |
| 31 | `0x006AC5E0` | Load |
| 33 | `0x006AC210` | Description (tooltip resolver) |
| 34 | `0x006ABD30` | One_Time_Load (SHP assets) |
| 50 | `0x006A5BF0` | Free (free SHP surfaces) |
| 53 | `0x006A5840` | Init_Mixfiles (palettes/SHPs) |
| 54 | `0x006A7D70` | Activate/ToggleSidebar |

---

## 2. Two Sidebar Layouts (Allied vs Soviet/Yuri)

The local side index `*(int*)(DAT_00a8b230 + 0x34B8)` distinguishes:
- **`== 0`**: Allied layout
- **`!= 0`**: Soviet/Yuri layout

Fresh writer tracing corrected the former `NewSidebar`/theater label:
`Read_Scenario` copies `HouseTypeClass+0xBC` at
`0x0068479D..0x006847C9`; `Full_Init` repeats the selection at
`0x00687794..0x00687833`. Stock side indices are Allied `0`, Soviet `1`,
and Yuri `2`. All layout values below show both branches where they differ.

---

## 3. Sidebar Positioning and Dimensions

### Core Globals (set in `FUN_006a5130` at `0x006a5130`)

| Global Address | Name | Value / Formula |
|---|---|---|
| `0x00886f94` | SidebarWidth | **0x9E = 158 pixels** (hardcoded) |
| `0x00886f90` | SidebarX | `view_left + view_width` (right edge of tactical area) |
| `0x00886f98` | SidebarTopClip | `DAT_007f5bf8 = 168` |
| `0x00886f9c` | SidebarBottomY | screen_height derived |

At 800x600: `SidebarX = 800 - 158 = 642`

### Sidebar Surface

Created in `FUN_00533fd0` at `0x00533fd0`. Surface pointer: `DAT_00887300`.
Surface dimensions: **168 x screen_height** pixels (10 pixels wider than sidebar_width for padding).
Debug string: `"SidebarSurface (%dx%d) %s\n"` at `0x00827bb8`.

### Tactical View Area

```
tactical_width  = screen_width - 158   (stored at DAT_00886fa8)
tactical_height = screen_height         (stored at DAT_00886fac)
tactical_left   = 0                     (stored at DAT_00886fa0)
tactical_top    = 0                     (stored at DAT_00886fa4)
```

Set in `FUN_004a8960` at `0x004a8960`.

---

## 4. Layout Constants

Set across `SidebarClass__InitLayoutConstants @ 0x006A5090` (six Y/spacing
globals) and `SidebarClass__InitSidebarRect @ 0x006A5130` (the remaining
position/size globals):

| Global | Description | Allied (side=0) | Soviet/Yuri (side≠0) |
|---|---|---|---|
| `0x00b0b4dc` | Repair/sell button X | SidebarX + 20 | SidebarX + 33 |
| `0x00b0b4e0` | Repair button Y | 158 + 8 = 166 | 158 + 7 = 165 |
| `0x00b0b4e4` | Sell button X delta from repair X | 64 | 52 |
| `0x00b0b4e8` | Tab buttons X start | SidebarX + 26 | SidebarX + 20 |
| `0x00b0b4ec` | Tab buttons Y | 158 + 39 = 197 | 158 + 39 = 197 |
| `0x00b0b4f0` | Tab button spacing | 29 | 32 |
| `0x00b0b4f4` | Cameo area X | SidebarX + 22 | SidebarX + 22 |
| `0x00b0b4f8` | Cameo area Y | 158 + 69 = 227 | 158 + 69 = 227 |
| `0x00b0b4fc` | Cameo column width | 63 | 64 |
| `0x00b0b500` | Cameo row height | **50** | **50** |
| `0x00b0b504` | Cameo total height | rows × 50 | rows × 50 |
| `0x00b0b508` | Scroll button X | SidebarX + 39 | SidebarX + 39 |
| `0x00b0b50c` | Scroll button Y | CameoY + 7 + total_height | CameoY + 7 + total_height |
| `0x00b0b510` | Scroll button width | 46 | 45 |
| `0x00b0b514` | Scroll speed (px/tick) | **50** | **50** |

### Visible Rows Formula (from `FUN_006ac430` at `0x006ac430`)

```c
int tab_overhead = (local_side_index != 0) ? 18 : 26;
int visible_rows = ((screen_bottom - cameo_y_start - tab_overhead - 7 + sidebar_width) / 50);
int visible_slots = visible_rows * 2;  // always 2 columns
```

---

## 5. Cameo Grid Layout

| Property | Value |
|---|---|
| Cameo icon size | **60 × 48 pixels** (0x3C × 0x30) |
| Columns per strip | **2** |
| Row height (with padding) | **50 pixels** (0x32) |
| Column spacing | 63 (Allied) or 64 (Soviet/Yuri) pixels |
| Max items per strip | **75** (0x4B) |
| Max visible slots (gadgets) | **60** per tab (0x3C) |

### Position formula (from `FUN_006a8220` at `0x006a8220` and `FUN_006abd30`)

```
cameo_x = CameoAreaX + column_width × column
cameo_y = CameoAreaY + 50 × row + 1
```

Where `CameoAreaX = SidebarX + 22` and `CameoAreaY = 158 + 69 = 227`.

---

## 6. SidebarClass Instance Fields

| Offset | Size | Name | Description |
|---|---|---|---|
| +0x1544 | 0xF94×4 | Strips[4] | 4 StripClass instances (one per tab) |
| +0x5394 | 4 | AnimFrameCounter | Sidebar open/close anim frame AND tab-flash frame (corrected 2026-05-28: prior rows `+0x14E5 AnimFrameCounter` and `+0x5394 TabFlashFrame` were two names for the same field; `+0x14E5` is the int-array index used in `SidebarClass__AI`, byte offset = `0x14E5×4 = 0x5394`; both purposes are live — verified via `decompile_function 0x006a7780` and `decompile_function 0x006a6c30` (`SidebarClass__Draw` uses `*(int*)(this+0x5394)`) — ROOT_CAUSE: PARAM1_TYPE_MISREAD + STRUCT_FAMILY_CASCADE) |
| +0x5398 | 4 | AnimDirection | 1=opening, −1=closing, 0=idle; also used as tab-flash-active flag (corrected 2026-05-28: prior rows `+0x14E6 AnimDirection` and `+0x5398 TabFlashActive` name the same field; byte offset = `0x14E6×4 = 0x5398` — verified via `decompile_function 0x006a7780`) |
| +0x539C | 4 | ActiveTabIndex | Currently selected tab (0-3); also used as ActiveStripIndex — same field (corrected 2026-05-28: prior rows `+0x14E7 ActiveStripIndex` and `+0x539C ActiveTabIndex` are duplicates; byte offset = `0x14E7×4 = 0x539C` — verified via `decompile_function 0x006a7780`, used as strip multiplier `param_1[0x14e7] * 0x3e5`) |
| +0x53A5 | 1 | IsSidebarActive | Whether sidebar is visible |
| +0x53A6 | 1 | NeedsFullRedraw | Force complete redraw |
| +0x53A7 | 1 | NeedsStripRedraw | Force strip area redraw |
| +0x53A8 | 1 | NeedsTabRedraw | Blit pending flag |

---

## 7. StripClass Layout (each 0xF94 = 3988 bytes)

Stored at SidebarClass +0x1544, stride 0xF94, 4 instances.

| Offset | Size | Name | Description |
|---|---|---|---|
| +0x1C | 1 | IsActive | Whether this strip is active/visible |
| +0x20 | 4 | X | Strip X position |
| +0x24 | 4 | Y | Strip Y position |
| +0x38 | 4 | TabIndex | Which tab (0-3) this strip belongs to |
| +0x3C | 1 | NeedsRedraw | Dirty flag |
| +0x3D | 1 | AutoScrollActive | Auto-production scroll |
| +0x3E | 1 | ScrollDirection | 0=down, 1=up |
| +0x3F | 1 | IsScrolling | Currently scrolling |
| +0x40 | 4 | AnimState | Animation state |
| +0x44 | 4 | TopRowIndex | First visible row (scroll offset) |
| +0x48 | 4 | ScrollCounter | Scroll request delta (decrements to 0 per frame; NOT visible rows — those are computed dynamically) |
| +0x4C | 4 | ScrollPixelOffset | Smooth scroll pixel position |
| +0x50 | 4 | PrevScrollOffset | Previous scroll position |
| +0x54 | 4 | EntryCount | Number of build entries |
| +0x58 | 0x34×75 | Entries[] | Build entry array |

### Build Entry (CameoEntry) — 0x34 = 52 bytes each

| Offset | Size | Name | Description |
|---|---|---|---|
| +0x00 | 4 | ItemIndex | TechnoType/SuperWeapon index |
| +0x04 | 4 | ItemType | RTTI type code |
| +0x08 | 4 | SubType | Sub-type / variant |
| +0x0C | 4 | FactoryPtr | Pointer to associated FactoryClass (0 = no build) |
| +0x10 | 4 | BuildState | 0=idle, 1=building, 2=on-hold, 3=ready |
| +0x14 | 4 | ProgressValue | Sidebar progress-animation counter; incremented by `StepIncrement` and stopped after it exceeds 0x34 |
| +0x18 | 1 | IsProgressingThisTick | Set on a tick where `ProgressValue` advances |
| +0x19 | 3 | Padding | Alignment before the embedded timer |
| +0x1C | 4 | CameoTimer.StartTime | Frame at which the embedded cameo timer started |
| +0x20 | 4 | CameoTimer.pad | Reserved/uninitialised timer word |
| +0x24 | 4 | CameoTimer.Duration | Duration consumed by the embedded 12-byte `CDTimerClass` at +0x1C; remaining time is computed, not stored separately |
| +0x28 | 4 | ProgressRate | Per-step interval copied into `CameoTimer.Duration +0x24`; zero means the cameo progress timer is inert |
| +0x2C | 4 | StepIncrement | Added to `ProgressValue` on timer expiry |
| +0x30 | 4 | FlashEndFrame | Draw pulses DARKEN.SHP while `g_CurrentFrameCounter < FlashEndFrame` |

> **Corrected 2026-07-11.** The 2026-07-10 correction still extended the
> embedded timer one word too far. With `ESI = entry + 0x1C`, `StripClass::AI`
> calls `CDTimerClass::GetTimeRemaining` on `ESI`, tests the separate scalar at
> `ESI+0x0C` (`entry+0x28`), and copies it to the timer's third word at
> `ESI+0x08` (`entry+0x24`). It advances `ProgressValue +0x14` by
> `StepIncrement +0x2C`; `StripClass::Draw` compares the current frame against
> `FlashEndFrame +0x30`. The displayed queue count is computed from
> `FactoryClass::CountTotal`, not stored at `+0x30` (verified via
> `disassemble_bytes 0x006A8F50..0x006A9020`, `decompile_function 0x006A8710`,
> and `decompile_function 0x006A9540` — ROOT_CAUSE: STRUCT_FAMILY_CASCADE +
> OFFSET_RETYPED_WRONG).

---

## 8. SelectClass (Cameo Click Gadgets)

240 entries total (4 tabs × 60 slots) at global `0x00B07E80`, stride 0x38 (56 bytes).

| Offset | Size | Name | Description |
|---|---|---|---|
| +0x00 | 4 | VTable | VTable pointer |
| +0x04 | 4 | Next | Linked list next |
| +0x08 | 4 | Prev | Linked list prev |
| +0x0C | 4 | X | Left pixel coordinate |
| +0x10 | 4 | Y | Top pixel coordinate |
| +0x14 | 4 | Width | **60** (0x3C) |
| +0x18 | 4 | Height | **48** (0x30) |
| +0x1E | 1 | Disabled | Disabled flag |
| +0x20 | 1 | Flags | 0x10 bit = right-click enabled |
| +0x24 | 4 | CommandID | **0xCA** (202) for all cameo slots |
| +0x2C | 4 | OwnerStrip | Back-pointer to parent StripClass |
| +0x30 | 4 | VisualIndex | row×2 + col (position in visible grid) |

---

## 9. SBGadgetClass (Tab/Button Gadgets)

Each 0x60 (96) bytes. Tab array at `0x00B07C48` (4 entries).

> **2026-05-19 correction.** Position offsets (X, Y) and IsActive were wrong in
> the prior table. Verified via `disassemble_function 0x006a5310` (the Tab-button
> init loop): `[ESI-0x18] = X, [ESI-0x14] = Y` with `ESI = base+0x24`, giving
> X = base+0x0C and Y = base+0x10. IsActive at base+0x1D from the scroll-button
> init at `0x006a5496` (`MOV byte ptr [0x00b0b345],0x1` with base 0xb0b328).
> ID at +0x24 and SHP at +0x50 were already correct. Fields marked **UNVERIFIED**
> below were not covered by the 2026-05-19 audit — re-check before relying on them.
> See [SIDEBAR_INIT_GADGET_POSITIONING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_INIT_GADGET_POSITIONING_GHIDRA_REPORT.md) for full evidence.

| Offset | Size | Name | Description |
|---|---|---|---|
| +0x0C | 4 | X | X position (verified `006a5310` Tab loop) |
| +0x10 | 4 | Y | Y position (verified `006a5310` Tab loop) |
| +0x1D | 1 | IsActive | Active/visible (verified `006a5496` scroll-button init) |
| +0x1E | 1 | IsDisabled | Disabled — **UNVERIFIED** |
| +0x20 | 4 | (unknown) | Value 0x55 written to scroll buttons in Init (new in 2026-05-19; purpose unknown) |
| +0x24 | 4 | ID | Gadget ID (verified `006a5310`: Tab 0xCB..0xCE, Repair 0x65, Sell 0x66, Scroll 0xC8/0xC9) |
| +0x34 | 1 | IsToggled | Toggle state (for tab highlight) — **UNVERIFIED** |
| +0x38 | 4 | FlashPeriod | Tab flash timer period — **UNVERIFIED** |
| +0x3C | 4 | FlashCounter | Tab flash countdown — **UNVERIFIED** |
| +0x40 | 1 | IsMouseOver | Hover flag — **UNVERIFIED** |
| +0x44 | 4 | DrawOffsetX | X draw offset — **UNVERIFIED** |
| +0x48 | 4 | SurfaceOffsetY | Y offset on surface — **UNVERIFIED** |
| +0x4C | 1 | DrawToSidebar | Draw on sidebar surface flag — **UNVERIFIED** |
| +0x50 | 4 | SHP | SHP image pointer (verified `006a5310` scroll-button init) |
| +0x54 | 1 | NeedsDraw | Dirty flag — **UNVERIFIED** |

---

## 10. Tab System

### Tab Button IDs and Positions

> **2026-05-20 audit.** Content column corrected. Prior table had Tab 0=Structures
> and Tab 2=Infantry — both wrong per binary. Verified via
> `decompile_function 0x006A6300` (`SidebarClass::AddCameo`): RTTI dispatch maps
> BuildingType (0xF/0x10) → tab 2, Infantry/UnitType (1/2/3/0x28) → tab 3,
> Aircraft (6/7) → tab 0 (non-naval) or tab 1 (naval), SuperWeapon (0x1F/0x20/0x39) → tab 1.
> Tab IDs 0xCB..0xCE are assigned left-to-right by tab index in `SidebarClass::Init` at
> 0x006A5310 (`*piVar7 = iVar6 + 0xcb;`). Tab string-table mapping in
> [SIDEBAR_STRIPS_TABS_CAMEOS_GHIDRA.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_STRIPS_TABS_CAMEOS_GHIDRA.md) (TXT_DEFENSE_TAB_DESC at Tab 1,
> TXT_STRUCTURE_TAB_DESC at Tab 2, TXT_UNIT_TAB_DESC at Tab 3) corroborates.

| Tab | Command ID | Content (per binary) |
|---|---|---|
| Tab 0 | 0xCB (203) | Aircraft (non-naval) — receives RTTI 6/7 when SpeedType ≠ 5 |
| Tab 1 | 0xCC (204) | Defense — SuperWeapons (RTTI 0x1F/0x20/0x39) + naval Aircraft (RTTI 6/7 when SpeedType = 5) |
| Tab 2 | 0xCD (205) | Structures — BuildingType (RTTI 0xF/0x10) |
| Tab 3 | 0xCE (206) | Units / Infantry — InfantryType/Infantry/UnitType/Unit (RTTI 1/2/3/0x28) |

Tab position formula:
```
tab_x = TabBaseX + TabSpacing × tab_index
tab_y = TabBaseY  (= 158 + 39 = 197)
```

RA2: TabBaseX = SidebarX + 26, spacing = 29
YR:  TabBaseX = SidebarX + 20, spacing = 32

### Tab Category Mapping (from `FUN_006abc60` at `0x006abc60`)

| RTTI Type | Tab |
|---|---|
| 0x0F, 0x10 (BuildingType) | Tab 2 (Structure) |
| 0x01, 0x28, 0x02, 0x03 (Infantry/Vehicle) | Tab 3 (Unit) |
| 0x39, 0x20, 0x1F (SuperWeapon) | Tab 1 (Defense) |
| 0x06, 0x07 (Aircraft) | Tab 0 or 1 (based on naval flag) |

### Tab Switching (from `FUN_006a7590` at `0x006a7590`)

1. Lock surface
2. Hide all 60 cameo slots of old tab via `FUN_004f4450`
3. Set `ActiveStripIndex = new_tab`
4. Unlock surface
5. Show new tab's visible slots
6. Update scroll button visibility
7. Set repaint flag

---

## 11. Scroll System

| Button | ID | Width (Allied / Soviet-Yuri) |
|---|---|---|
| Scroll Down | 0xC9 (201) | 46 / 45 |
| Scroll Up | 0xC8 (200) | 46 / 45 |

IDs are assigned in `SidebarClass::Init` at `0x006a5310`. In `SidebarClass::AI`, scroll
commands are matched via globals `DAT_00b0b34c` (scroll down) and `DAT_00b0b42c` (scroll up)
with `| 0x8000` prefix, not by hardcoded literal IDs.

Position: Y = CameoAreaY + 7 + visible_cameo_height. Scroll down is at
`ScrollX = SidebarX + 39`; scroll up is at `ScrollX + ScrollWidth`.

### Scroll Logic (from `FUN_006a7780`)

```
visible_rows = computed from screen dimensions
// Scroll Down:
if ((scroll_offset + visible_rows) * 2 < total_items):
    scroll_target += visible_rows
// Scroll Up:
if (scroll_offset > 0):
    scroll_target -= visible_rows
```

### Smooth Scrolling (from `FUN_006a8b30` at `0x006a8b30`)

Scroll speed: **50 pixels per tick** (one full row height).
When pixel offset reaches row height (50), shifts the logical row index.

---

## 12. Drawing Pipeline

### Main Draw (`SidebarClass::Draw` at `0x006a6c30`)

1. Save current surface rect
2. Set drawing target to SidebarSurface (`DAT_00887300`)
3. **Background (full redraw path):**
   - Draw `SIDE1.SHP` (top) at Y = SidebarWidth
   - Loop: draw `SIDE2.SHP` (middle tile), repeating to fill height
   - Draw `SIDE3.SHP` (bottom cap)
   - Draw `ADDON.SHP` (extra area)
4. Draw gadget buttons (Repair, Sell, 4 tab buttons, scroll buttons) via `FUN_0069deb0`
5. If tab flash active (+0x5398): draw SIDE1/2/3.SHP animated frames
6. Call `StripClass::Draw` for active strip
7. Call `FUN_0063fb20` (PowerBar::Draw) — draws POWERP.SHP segments
8. Tooltip overlay — if tooltip singleton (`DAT_00887368`) exists, draw via vtable+0x0C
9. Dirty flag checks — iterate gadget dirty flags, set `DAT_00b0b518` if any need blit
10. Blit SidebarSurface to screen via `FUN_006a70e0` (smart dirty-region blit)

### Strip Draw (`StripClass::Draw` at `0x006a9540` — 4210 bytes, largest function)

For each visible cameo slot (2 columns × N rows):

1. **Cameo icon** — Draw cameo SHP at (x, y) with DrawSHP flags `0x400`
2. **Selection highlight** — If selected by another player, draw colored outline
3. **Darken overlay** — If unbuildable, draw `DARKEN.SHP` frame 0 with flags `0x401`
4. **Flash animation** — If recently completed, pulse every 16 frames using DARKEN.SHP with flags `0x404`
5. **Cameo name text** — At (x, y + 0x24), right-aligned within 60px width, via `FUN_006ac480`
6. **Build count overlay** — If queue > 1: AlphaBlendRect (alpha=**0xAF/175**) + count text at (x + 60, y + 1)
7. **Status text** — "Ready"/"On Hold": AlphaBlendRect (alpha=**0xAF**) + text at (x + 0x1E, y + 1) with flags `0x142` (centered). **Dual position**: when queue count is also shown, shifts to (x + 2, y_bottom + 1) with flags `0x42` (left-aligned).
8. **Clock/progress overlay** — when the linked factory is not complete,
   GCLOCK2.SHP frame `display_progress + 1` (ordinary valid range 1-54) with
   flags `0x404`

### Progress Bar (GCLOCK2.SHP)

- **55 stored frames**, indexed 0..54. Frame 0 is empty; frames 1..54 are the
  54 visible progress images.
- Ordinary linked-factory progress 0..53 is drawn as `display_progress + 1`,
  giving frame range **1..54**. A valid completed factory at progress 54
  branches around the GCLOCK draw, so frame argument 55 is not used on that
  path (`0x006A9E3E..0x006A9E44`).
- `FactoryClass::GetProgress` at `0x004ca120` returns 0..0x36 from StageClass::Value at offset +0x24
- `FactoryClass::IsComplete` at `0x004ca130` returns true when progress is
  `0x36` and either the produced object pointer is non-null or the special-item
  field is not `-1`.
- When the cameo's auxiliary progress exceeds factory progress, the displayed
  value is signed truncation of `(factory_progress + cameo_progress) / 2`;
  otherwise factory progress is used (`0x006A98C4..0x006A98CF`).

---

## 13. Art Assets

### Sidebar Frame SHPs (loaded in `FUN_006a5840` at `0x006a5840`)

| Global | Asset | Purpose |
|---|---|---|
| `0x00b0b468` | SIDE1.SHP | Top background piece |
| `0x00b0b46c` | SIDE2.SHP | Middle repeating tile |
| `0x00b0b470` | SIDE3.SHP | Bottom cap |
| `0x00b0b474` | ADDON.SHP | Expansion area |
| `0x00b0b478` | (tab anim 1) | Tab flash animation frame 1 |
| `0x00b0b47c` | (tab anim 2) | Tab flash animation frame 2 |
| `0x00b0b480` | (tab anim 3) | Tab flash animation frame 3 |
| `0x00b0b484` | GCLOCK2.SHP | Clock/progress overlay (55 stored frames; 54 ordinary drawable images) |
| `0x00b07bc0` | DARKEN.SHP | Semi-transparent overlay for unbuildable cameos |

### Tab and Button SHPs

| Asset | Purpose |
|---|---|
| TAB00.SHP - TAB03.SHP | Tab button icons (format: `TAB%02d.SHP`) |
| R-UP.SHP | Scroll up button |
| R-DN.SHP | Scroll down button |
| REPAIR.SHP | Repair button |
| SELL.SHP | Sell button |
| SIDEGDI1/2/3.SHP | Alternate sidebar panel art |
| Button%02d.SHP | Command bar buttons (25 buttons) |

### Observer/Faction Icon SHPs

| Global | Asset | Purpose |
|---|---|---|
| `0x00b0b490` | OBSALLI.SHP | Observer Allied icon |
| `0x00b0b494` | OBSSOVI.SHP | Observer Soviet icon |
| `0x00b0b498` | OBSYURI.SHP | Observer Yuri icon |
| `0x00b0b49c` | RANI.SHP | Random icon |
| `0x00b0b4a0` | OBSI.SHP | Observer icon |
| `0x00b0b4a4`+ | USAI, JAPI, FRAI, GERI, GBRI, DJBI, ARBI, LATI, RUSI, YRII | Country icons |

### Additional Sidebar Art (table at `0x00844ce0`)

SIDEBTTN.SHP, SIDE2B.SHP, CREDITS.SHP, TOP.SHP, RADAR.SHP, RADARY.SHP,
BKGDLG/MD/SM.SHP (+ Y variants), RENDCAP.SHP, LENDCAP.SHP, BTTNBKGD.SHP,
LSPACER.SHP, SDBTM.SHP, SDTP.SHP, SDWRNTMP.SHP, SDMPBTN.SHP, SDBTNANM.SHP

### Palettes

| String Address | Palette | Purpose |
|---|---|---|
| `0x0084542c` | SIDEBAR.PAL | Tab buttons, background |
| `0x008204e0` | CAMEO.PAL | Cameo icons |
| `0x00830630` | SIDEFNT3.PAL | Sidebar font |

### MIX Archives

- `CAMEOMD.MIX` / `CAMEO.MIX` — cameo icon archives (loaded in game init)
- Side-specific MIX: `SIDECxxMD.MIX` / `SIDECxx.MIX` — per-faction sidebar art

---

## 14. Input Dispatch Chain

```
GScreenClass::Input (0x004f4320)
  └─ GadgetClass::Input (0x004e1640)
      └─ GadgetClass::Hit_Test (0x004e15a0)  — iterate gadgets, find smallest hit
      └─ GadgetClass::Clicked_On (0x004e13f0) — bounds check + action dispatch
          └─ For cameos: SelectClass::Action (0x006aad00)
          └─ For buttons: GadgetClass::Action (0x0048e5a0)
      └─ Chain to child handlers:
          └─ FUN_006922e0 (DisplayClass)
              └─ FUN_006d0680 (Command bar dispatch)
                  └─ FUN_006a7780 (SidebarClass::AI — tabs, scroll, sell, repair)
                      └─ FUN_0063fea0 (credits, scrollbar)
                          └─ FUN_00653850 (minimap, chat)
```

### Mouse Button Flag Mapping (in `GadgetClass::Input`)

| Raw Input | Flag | Meaning |
|---|---|---|
| 1 | 0x01 | Left press |
| 2 | 0x10 | Right press |
| 0x801 | 0x04 | Left release |
| 0x802 | 0x40 | Right release |
| held key1 | 0x02 / 0x08 | Left held / left up |
| held key2 | 0x20 / 0x80 | Right held / right up |

### Hit Testing (`GadgetClass::Hit_Test` at `0x004e15a0`)

- Iterates linked list of gadgets (head at `DAT_00a8ef54`)
- For each enabled gadget: tests `x ≤ mouseX < x+w` AND `y ≤ mouseY < y+h`
- If multiple overlapping: selects the **smallest area** gadget (w×h comparison)

### Sidebar Bounds Check (in `FUN_006d0680`)

```
mouseX >= sidebar_left AND mouseX < sidebar_left + sidebar_width
mouseY >= sidebar_top  AND mouseY < sidebar_top + screen_height - 2
```
When mouse is in sidebar region: calls `FUN_005bdc80(0,0)` to clear tactical cursor.

---

## 15. Cameo Click Handling

### Click → Slot Index (from `SelectClass::Action` at `0x006AAD00`)

> **2026-05-20 audit.** Entry address corrected from `0x006AB970` (a mid-body offset ~3185 bytes into the function) to `0x006AAD00`. Re-verified 2026-07-10: both `get_function_by_address 0x006AAD00` and `get_function_by_address 0x006AB970` resolve the containing function as `SelectClass__Action`, entry `0x006AAD00`, body `0x006AAD00..0x006AB986` — ROOT_CAUSE: GHIDRA_ADDRESS_SHIFT. Cross-confirmed against the patched [BUILD_QUEUE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILD_QUEUE_GHIDRA_REPORT.md) and [SIDEBAR_STRIPS_TABS_CAMEOS_GHIDRA.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_STRIPS_TABS_CAMEOS_GHIDRA.md).


```
visual_index = SelectClass.VisualIndex   // row*2 + col
scroll_offset = strip->TopRowIndex       // current scroll position
absolute_index = visual_index + scroll_offset * 2
item = &strip->Entries[absolute_index]   // at strip + 0x58 + absolute_index * 0x34
```

### Left-Click Behavior

**Super weapons (type 0x1F):**
1. Check if super weapon can be activated (`FUN_006cc360`)
2. If targeting required (offset 0xBC ≠ 0): set cursor mode for targeting
3. Otherwise: queue network command 0x12 (activate super weapon)

**Normal items:**
1. Resolve TechnoType from index
2. If the entry has a factory whose `ProductionRate +0x38` is zero or whose
   `IsSuspended +0x70` is set:
   - Incomplete production → send PRODUCE (0x0E) to restart/resume it
   - Complete building → begin building placement
   - Other complete objects → send PLACE (0x0B) or the type-specific deploy path
3. If there is no entry factory:
   - Check buildability
   - Send build-start command (0x0E)
   - Set state to 1 (building)
   - Calculate initial progress: `build_cost / 0x36` frames (clamped 1-255)

> **Corrected 2026-07-11.** The prior summary collapsed the stopped/suspended,
> completed, running, and absent-factory branches into “complete” versus “no
> factory” and incorrectly described an otherwise path as cancel+restart. The
> body explicitly gates the restart/place block on `factory+0x38 == 0 ||
> factory+0x70 != 0` (verified via `decompile_function 0x006AAD00` —
> ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT + OFFSET_RETYPED_WRONG).

### Right-Click Behavior

**Entry has an active factory:**
- If `ProductionRate +0x38 != 0` and `IsSuspended +0x70 == 0`, send SUSPEND
  (0x0F) and dirty the strip.
- Otherwise (rate zero or suspended), send ABANDON (0x10), or 0x2E when the
  queue-removal modifier is held.

**Entry has no active factory:**
- If its cameo state is building (1), change it to on-hold (2) and send SUSPEND
  (0x0F).
- Independently locate a factory queue containing the item; if found, send
  ABANDON (0x10), or 0x2E with the queue-removal modifier.

The taken branches play the click sound and corresponding EVA announcement.

> **Corrected 2026-07-11.** The prior active/no-active grouping reversed the
> running-factory pause branch and omitted the separate queued-item removal
> lookup. The event selection and both factory-field tests are explicit in the
> click-handler body (verified via `decompile_function 0x006AAD00` — ROOT_CAUSE:
> OPERATOR_OR_ORDER_DRIFT + OFFSET_RETYPED_WRONG).

### Network Event Types (engine names from event string table at `0x0082091c`)

| Code | Engine Name | Sidebar Action |
|---|---|---|
| 0x0B | PLACE | Place building/unit on map |
| 0x0E | PRODUCE | Start production (left-click cameo) |
| 0x0F | SUSPEND | Suspend/pause production (right-click while building) |
| 0x10 | ABANDON | Cancel completed item / remove from queue |
| 0x12 | SPECIAL_PLACE | Activate super weapon / target placement |
| 0x2E | (queued) | Remove queued copies under the right-click queue-removal modifier (corrected 2026-07-11 via `decompile_function 0x006AAD00` — ROOT_CAUSE: INFERENCE_HARDENED) |

---

## 16. Tooltip System

### Tooltip Resolution (`FUN_006ac210` at `0x006ac210`)

| Command ID | Tooltip |
|---|---|
| 200 (0xC8) | `TXT_SIDEBAR_UP` (scroll up) |
| 201 (0xC9) | `TXT_SIDEBAR_DOWN` (scroll down) |
| 203 (0xCB) | Tab 0 name (CSF string 0x13DB) |
| 204 (0xCC) | Tab 1 name (CSF string 0x13DD) |
| 205 (0xCD) | Tab 2 name (CSF string 0x13DF) |
| 206 (0xCE) | Tab 3 name (CSF string 0x13E1) |
| ≥ 1000 | Cameo tooltip (item cost/name) |

### Cameo Tooltip Text (`FUN_006a92e0` at `0x006a92e0`)

- Super weapons: returns display name from `DAT_00a8e334`
- Normal items: formats via `TXT_MONEY_FORMAT_1` (cost only) or `TXT_MONEY_FORMAT_2` (cost + name)
- Spaces (0x20) replaced with newlines (0x0A) for word wrapping
- Buffer at `DAT_00b07bc4`

### Low-Funds Sidebar Warning Timer (not tooltip timing)

- `HouseClass::Update` calls `FUN_006d0ec0` at `0x004F8BAA` on the local-player
  low-funds path immediately after the insufficient-funds EVA event.
- `FUN_006d0ec0` receives an `int *` and writes indices `[0x154E]`, `[0x154F]`,
  and `[0x1550]`; the actual byte offsets are therefore **+0x5538** (start frame),
  **+0x553C** (timer pad), and **+0x5540** (duration = 7).
- `CommandBar_Dispatch` reads `+0x5538/+0x5540`; when one frame remains it sets
  `+0x5545` and invalidates the sidebar for redraw.
- The tooltip singleton remains `DAT_00887368`, but this evidence does not
  establish the tooltip hover delay.

> **Corrected 2026-07-10.** The former subsection confused `int *` indices with
> byte offsets and inferred tooltip ownership from an unrelated timer. Verified
> via `decompile_function 0x006D0EC0`, `get_xrefs_to 0x006D0EC0`,
> `disassemble_function 0x004F8440` around callsite `0x004F8BAA`, and
> `decompile_function 0x006D0680` — ROOT_CAUSE: PARAM1_TYPE_MISREAD +
> INFERENCE_HARDENED.

---

## 17. Text Color System

`SetSidebarTextColor` at `0x0072f440`:
- Index 0: color from `DAT_00b0f9d8/DAT_00b0f9da` (default sidebar text)
- Index 1: color from `DAT_00b0fb04/DAT_00b0fb06` (alternate color)
- Index 2+: color from `DAT_00b0faa0/DAT_00b0faa2` (third color)

Text is 16-bit packed RGB, converted via shift masks at `DAT_008a0dd0-DAT_008a0de4`.
Sidebar font pointer: `DAT_00b0fc08` (returned by `FUN_0072f4d0`).

---

## 18. Repair and Sell Buttons

> **2026-05-20 audit.** Repair/Sell IDs were reversed in the previous version
> of this table AND in §37 line "Repair/Sell IDs swapped" — the "swap correction"
> note was itself a wrong correction. Verified via `decompile_function 0x006A5310`
> (`SidebarClass::Init`): the function writes `_DAT_00b0b3c4 = 0x65;` (assigning
> ID **0x65** to the gadget at `DAT_00b0b3ac`, which §26 of this same doc labels
> "Repair button gadget"), and writes `_DAT_00b07e1c = 0x66;` (assigning ID
> **0x66** to the gadget at `DAT_00b07e04` in the Sell/scroll block). Therefore
> **Repair = 0x65, Sell = 0x66**.

| Button | ID | X (Allied) | X (Soviet/Yuri) | Y (Allied / Soviet-Yuri) |
|---|---|---|---|---|
| Repair | 0x65 (101) | SidebarX + 20 | SidebarX + 33 | 166 / 165 |
| Sell | 0x66 (102) | SidebarX + 84 | SidebarX + 85 | 166 / 165 |

Note: 0x8065 calls `FUN_004ac8c0` (Repair handler), 0x8066 calls `FUN_004ac660` (Sell handler) — verified via `decompile_function 0x006A7780` (`SidebarClass::AI`) cases `uVar3 == 0x8065` and `uVar3 == 0x8066`.

---

## 19. Power Bar

Drawn in `FUN_0063fb20` at `0x0063fb20`:
- X offset: 0 (YR) or 5 (RA2)
- Y start: CameoAreaY (= 158 + 69)
- Uses `DAT_00ac4e74` SHP in 3-pixel vertical increments
- Fills the visible cameo area height ÷ 3

---

## 20. Observer Mode

When `DAT_00a83d4c == DAT_00ac1198` (observer/spectator):

- Only 1 row visible at a time
- Each row = one player
- Per player:
  1. Draw faction icon SHP (OBSALLI/OBSSOVI/OBSYURI based on side 0/1/2) at (x, y)
  2. Draw stat icon (from -3..9 switch → `DAT_00b0b49c..DAT_00b0b4c8`) offset by +70px X and Y
  3. Draw player name at (x + 8, y + 4)
  4. Draw stat lines (credits, kills, units, rank) each +17px vertically

---

## 21. Command Bar (Sidebar Buttons Panel)

### Constructor: `FUN_006cfe20` at `0x006cfe20` (1074 bytes)

- 25 command bar buttons, loaded as `Button%02d.SHP`
- Button SHP array: `DAT_00b0c148` (25 entries)
- Button-to-command mapping: `DAT_00b0cb78` (25 entries)
- Strip data array: `DAT_00b0c1c0` (25 entries, 0x60 bytes each)
- Command name table: `PTR_DAT_008427d0`

### Command Bar IDs

| ID | Purpose |
|---|---|
| 0x80D6-0x80EE | Command bar button presses |
| 0xC0D6-0xC0EE | Command bar right-click (team assignment) |
| 0x80F0 | Thumb close |
| 0x80F1 | Thumb open |

### Button Layout: `FUN_006d0fd0` at `0x006d0fd0`

Buttons are positioned in the command bar strip. Full relayout in `FUN_006d1200` (752 bytes, expanded mode).

---

## 22. Radar/Minimap Position (from `FUN_00652e90`)

| Property | RA2 | YR |
|---|---|---|
| Radar area X | minimap_x + 11 | minimap_x + 14 |
| Radar area Y | minimap_y + 4 | minimap_y + 5 |
| Bottom bar X | minimap_x + 83 | minimap_x + 86 |
| Radar width calc | (168-144)/2 + 4 = 16 | (168-145)/2 + 5 = 16 |

---

## 23. INI Options

| Section | Key | Effect |
|---|---|---|
| [Options] | SidebarCameoText | Boolean — show text on cameos (offset +0x1D in options) |
| [Video] | AllowVRAMSidebar | Boolean — use VRAM for sidebar surface (offset +0x36) |
| [Options] | Sidebar | Always 1 (RIGHT side) |
| [Video] | ScreenWidth/Height | Resolution, affects sidebar position |

---

## 24. Complete Layout Diagram (800×600, Soviet/Yuri Side Layout)

```
Screen: 800 × 600
Sidebar X: 642  (800 - 158)
Sidebar width: 158 pixels
Surface width: 168 pixels (10px padding)

┌──────────────────────────────────────────────────────┬─────────────────┐
│                                                      │  Y=0: Top       │
│                                                      │  ┌─Radar/Map──┐ │
│                                                      │  │            │ │
│            TACTICAL VIEW                             │  └────────────┘ │
│            (642 × 600)                               │  Y=165: Repair  │
│                                                      │  [Repair][Sell]  │
│                                                      │  Y=197: Tabs    │
│                                                      │  [T0][T1][T2][T3]│
│                                                      │  Y=227: Cameos  │
│                                                      │  ┌──┐┌──┐       │
│                                                      │  │60│|60│ row 0 │
│                                                      │  │48│|48│       │
│                                                      │  └──┘└──┘       │
│                                                      │  ┌──┐┌──┐ row 1 │
│                                                      │  │  ││  │(+50px)│
│                                                      │  └──┘└──┘       │
│                                                      │  ... more rows  │
│                                                      │  [▲Scroll][▼]   │
│                                                      │  Power bar area │
└──────────────────────────────────────────────────────┴─────────────────┘
  X=0                                                X=642           X=800
```

---

## 25. Complete Function Reference

| Address | Name | Size | Purpose |
|---|---|---|---|
| `0x004a59e0` | ComputeTextRect | — | Compute bounding rect for text |
| `0x004a60e0` | DrawText | — | Text rendering on surfaces |
| `0x004aed70` | DrawSHP | — | General SHP rendering |
| `0x004ca120` | FactoryClass::GetProgress | — | Returns build progress (0..0x36) |
| `0x004ca130` | FactoryClass::IsComplete | — | Returns true when done (corrected 2026-05-28: was `HasCompleted`; binary label is `FactoryClass__IsComplete` via `get_function_by_address 0x004ca130` — ROOT_CAUSE: RTTI_LABEL_DRIFT) |
| `0x004e13f0` | GadgetClass::Clicked_On | — | Bounds check + action dispatch |
| `0x004e15a0` | GadgetClass::Hit_Test | — | Iterate gadgets, find smallest hit |
| `0x004e1640` | GadgetClass::Input | — | Main input loop |
| `0x004f4320` | GScreenClass::Input | — | Top-level input dispatch |
| `0x00533fd0` | CreateSurfaces | — | Creates SidebarSurface |
| `0x0063fb20` | PowerBar::Draw | — | Draws power bar |
| `0x00621b80` | AlphaBlendRect | — | Alpha-blended rect for tooltip bg |
| `0x00652e90` | Radar::Init | — | Positions minimap relative to sidebar |
| `0x006922e0` | DisplayClass::Input | — | Display-level input handler |
| `0x006A4F20` | SidebarClass::Constructor | — | Primary constructor (verified via `get_function_by_address 0x006A4F20 → SidebarClass__constructor`, 2026-05-20). NOTE: 0x006A4550 is `SideClass::Constructor` (parses [Sides] INI), NOT SidebarClass. §1's inheritance-chain table cites 0x006A4F20 correctly. |
| `0x006A4E60` | SidebarClass::constructor (variant) | — | Ghidra label is `SidebarClass__constructor` (likely copy / alternate-init ctor), NOT a destructor. The actual destructor is at vtable slot 4 = `0x006AC7F0` per §1's vtable table (verified via `get_function_by_address 0x006A4E60`, 2026-05-20). |
| `0x006a5000` | LoadDarken | — | Loads DARKEN.SHP |
| `0x006a5090` | InitLayoutConstants | — | Sets layout constants (Allied vs Soviet/Yuri side) |
| `0x006a5130` | InitSidebarRect | — | Sets sidebar_width=158, positions |
| `0x006a5310` | SidebarClass::Init | 1325 | Full initialization |
| `0x006a5840` | LoadSHPs | — | Loads all sidebar SHP assets |
| `0x006a5bf0` | FreeSHPs | — | Frees all SHP handles |
| `0x006a6300` | AddCameo | — | Adds build item to strip + EVA |
| `0x006a6610` | UpdateScrollButtons | — | Show/hide scroll buttons |
| `0x006a6a00` | ScrollStripPage | — | Page-based scroll |
| `0x006a6af0` | ScrollStripSmooth | — | Smooth/incremental scroll |
| `0x006a6c30` | SidebarClass::Draw | 1185 | Main sidebar draw |
| `0x006a70e0` | BlitToScreen | 952 | Blit sidebar surface to screen |
| `0x006a7590` | SwitchTab | 342 | Switch active tab |
| `0x006a7780` | SidebarClass::AI | 1428 | Per-tick sidebar command/input processing |
| `0x006a7d20` | PeriodicUpdate | — | Sidebar periodic update |
| `0x006a7d70` | ToggleSidebar | 783 | Toggle visibility |
| `0x006a8220` | InitSelectZones | — | Init cameo click zones per strip |
| `0x006a8330` | ActivateSelectZones | — | Activate visible cameo gadgets |
| `0x006a8420` | CompareItems | 748 | Sort comparison for buildables |
| `0x006a8710` | InsertEntry | — | Insert item sorted into strip |
| `0x006a8b30` | StripClass::AI | 1938 | Scroll anim + build progress |
| `0x006a92e0` | GetCameoTooltip | — | Tooltip text (cost/name) |
| `0x006a93f0` | StripClass::Activate | — | Activate strip buttons |
| `0x006a9540` | StripClass::Draw | 4210 | Draw all cameos (LARGEST) |
| `0x006aa600` | StripClass::Recalculate | 1711 | Remove completed/cancelled |
| `0x006aad00` | SelectClass::Action | — | Cameo click handler (re-verified via `get_function_by_address 0x006AAD00`, 2026-07-10 — ROOT_CAUSE: GHIDRA_ADDRESS_SHIFT) |
| `0x006abc60` | TypeToTab | — | Object type → tab mapping |
| `0x006abd30` | InitSurface | — | Setup sidebar surface + zones |
| `0x006ac210` | ResolveTooltip | — | Tooltip text for any widget |
| `0x006ac430` | GetVisibleSlotCount | — | Returns visible_rows × 2 |
| `0x006ac480` | DrawCameoText | — | Draw name text on cameo |
| `0x006cbee0` | Super::GetProgressFrame | — | Super weapon progress (0..0x36) |
| `0x006cc2b0` | Super::NameReadiness | — | Super weapon status text (corrected 2026-05-28: was `GetStatusText`; binary label `SuperClass__NameReadiness` — ROOT_CAUSE: RTTI_LABEL_DRIFT) |
| `0x006cfe20` | CommandBar::Constructor | 1074 | Command bar init |
| `0x006d02b0` | LoadButtonSHPs | — | Load Button%02d.SHP |
| `0x006d0680` | CommandBar::Dispatch | 822 | Command bar input handler |
| `0x006d0fd0` | CommandBar::Layout | — | Position command buttons |
| `0x006d1200` | CommandBar::FullRelayout | 752 | Expanded layout |
| `0x0072ddb0` | SidebarSurface::Init | — | 10 graphic + 3 DDraw surfaces |
| `0x0072ec70` | LayoutRectCalculator | — | Sidebar layout rect |
| `0x0072f440` | SetSidebarTextColor | — | Set text color from palette |
| `0x0072f4d0` | GetSidebarFont | — | Returns font at 0x00b0fc08 |

---

## 26. Global Data Address Map

| Address Range | Contents |
|---|---|
| `0x00886f90-0x00886fac` | Viewport/sidebar boundary rect |
| `0x00880d48+N×0xF94` | Per-strip state arrays (4 strips) |
| `0x00b07c48-0x00b07dc8` | 4 Tab button gadgets (stride 0x60) |
| `0x00b07df8-0x00b07e58` | Sell/scroll gadgets |
| `0x00b07e80-0x00b0b300` | 240 SelectClass cameo gadgets (4×60×0x38) |
| `0x00b0b3ac-0x00b0b3f0` | Repair button gadget |
| `0x00b0b468-0x00b0b484` | Sidebar SHP pointers (top/mid/bottom/cap/clock) |
| `0x00b0b490-0x00b0b4c8` | Faction icon SHP pointers (12+ entries) |
| `0x00b0b4dc-0x00b0b514` | Computed layout constants |
| `0x00b0c148` | Command bar button SHP array (25) |
| `0x00b0c1c0` | Command bar strip data (25×0x60) |
| `0x00b0cb78` | Button-to-command mapping (25) |
| `DAT_00887300` | SidebarSurface pointer |
| `DAT_00887368` | Tooltip singleton |
| `DAT_00a8b230+0x34B8` | Selected local side index (Allied 0, Soviet 1, Yuri 2) |
| `DAT_00a83d4c` | Player house pointer |
| `DAT_00ac1198` | Observer house pointer |

---

## 27. Build Queue Sort Order (from `FUN_006a8420` at `0x006a8420`)

Priority (from `FUN_006a8420`, verified by decompilation):
1. Null/empty entries sort last
2. Super weapons sort BEFORE ordinary entries (corrected 2026-07-10: `StripClass::InsertEntry` advances while `CompareItems(new, existing)` is false, and `CompareItems` returns true for a new superweapon against an ordinary entry, inserting it before that entry; verified via `decompile_function 0x006A8420` and `decompile_function 0x006A8710` — ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT)
3. **Super weapon vs super weapon**: by `SuperWeaponType::Type` (offset 0xB0), then UIName alphabetically
4. **Factory match**: items matching player's primary factory sort first
5. **Upgrade/naval status**: upgrades before non-upgrades, naval considerations
6. **Tech level** (offset 0x634): lower tech level sorts first
7. **Build cost**: `GetCost()` (vtable+0x84) — cheaper first
8. **Alphabetical**: UIName (offset 0x60, wchar_t*) via `wcscmp`

---

## 28. Surfaces Architecture

Five DirectDraw surfaces (from `FUN_00533fd0`):

| Global | Name | Memory |
|---|---|---|
| `DAT_0088730c` | HiddenSurface | System memory |
| `DAT_0088731c` | CompositeSurface | VRAM if 3D enabled |
| `DAT_008872fc` | TileSurface | VRAM if 3D enabled |
| `DAT_00887300` | **SidebarSurface** | VRAM or sysmem (based on `AllowVRAMSidebar`) |
| `DAT_00887310` | AlternateSurface | Always VRAM |

Sidebar surface init (`FUN_0072ddb0`) loads 10 SHP pieces from NTRLMD.MIX/NEUTRAL.MIX
and 3 palettes, then creates 8 layout region rects at `DAT_00b0fc10-0xb0fc2c`.

---

## 29. Power Bar System (NEW — from verification pass)

### PowerBar::Draw (`0x0063fb20`)

- **SHP**: `POWERP.SHP` at `DAT_00ac4e74` (loaded via `FUN_0063f7c0`)
- **Position**: X = 0 (YR) or 5 (RA2); Y = `DAT_00886f94 + 0x45` (sidebar top + 69)
- **Segment height**: 3 pixels each
- **Total segments**: `(DAT_00b0b504 + 3) / 3`
- **Draw order**: top-to-bottom — empty first, then green, yellow, red at bottom

### POWERP.SHP Frame Meanings

| Frame | Color | Meaning |
|---|---|---|
| 0 | Dark/empty | Unused power capacity |
| 1 | Green | Power supply meeting demand |
| 2 | Yellow | Low power warning |
| 3 | Red | Power drain exceeding supply |
| 4 | Partial | Transition segment (drawn when partial count is even) |

### PowerClass Fields (offsets from SidebarClass base)

| Offset | Field | Description |
|---|---|---|
| +0x150C | NeedsRedraw | bool — triggers redraw |
| +0x151C | PartialSegments | Animation step counter |
| +0x152C | GreenSegments | Powered segments count |
| +0x1530 | YellowSegments | Low power segments count |
| +0x1534 | RedSegments | Over-drain segments count |
| +0x1538 | IsAnimating | bool — segment transition active |
| +0x153C | CachedPowerDrain | Cached drain value |
| +0x1540 | CachedPowerOutput | Cached output value |

Animation: when power changes, `IsAnimating = true`, `PartialSegments = 10`.
Each tick decrements by 1 with 3-tick timer delay, gradually moving segments toward target.

---

## 30. Credits Display System (NEW)

### CreditsClass::Draw (`0x004a2370`)

- **Position**: Centered horizontally (`sidebarSurfaceWidth / 2`), Y = 2 (corrected 2026-05-28: was `screenWidth / 2`; binary reads sidebar surface width via `(*g_SidebarSurface + 0x7c)()` and divides by 2 — verified via `decompile_function 0x004a2370` — ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT)
- **Text color**: Green-tinted from `DAT_00b0fa1c`/`DAT_00b0fa1d`
- **Format**: Wide string `%ld` for money; `%d:%02d:%02d` for observer timer
- **Draw flags**: `0x4108` (alignment + shadow)
- **Background**: `CREDITS.SHP` (at `DAT_00b0fb08`) drawn via `FUN_006d0e60`

### Animated Counting Effect (`CreditsClass::AI` at `0x004a2600`)

- Step size = `|actual - displayed| / 8`, clamped to [1, 143]
- **Counting UP stores interval value 1; counting DOWN stores interval value 3.**
- `DisplayedValue` still advances in that same `CreditsClass::AI` call. The value
  at `+0x0C` is decremented when nonzero, then immediately overwritten with 1/3;
  it does **not** gate the current update or create frames between steps.
- _Corrected 2026-07-10: prior text interpreted the stored 1/3 value as a
  step-delay gate. Verified via `decompile_function 0x004A2600`: after computing
  `(((uVar3 < 1) - 1) & 0xfffffffe) + 3`, the function immediately adds the
  clamped signed step to `DisplayedValue` — ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT._
- Tick sound at 50% volume (`0x3f000000` = 0.5f) when `RulesClass->CreditTicks (offset 0x6DC) > 1`
- Observer mode: shows elapsed game time (HH:MM:SS) instead of money

### CreditsClass Fields (~16 bytes)

| Offset | Field | Description |
|---|---|---|
| +0x00 | CurrentValue | Actual current credits |
| +0x04 | DisplayValue | Currently displayed (animated) |
| +0x08 | NeedsRedraw | bool |
| +0x09 | IsCountingUp | bool |
| +0x0A | SoundFlag | bool — play tick sound |
| +0x0C | ComputedInterval | Stores 1 while counting up or 3 while counting down; not a step-update gate (corrected 2026-07-10 via `decompile_function 0x004A2600` — ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT) |

---

## 31. Sidebar Thumb (Collapse/Expand) (NEW)

### Command IDs

| ID | Action |
|---|---|
| 0x80F0 | Thumb CLOSE (collapse sidebar command panel) |
| 0x80F1 | Thumb OPEN (expand sidebar command panel) |

### Behavior (from `FUN_006d0680`)

**Close**: Removes all 25 strip gadgets from GadgetList, re-adds collapsed thumb gadget at `DAT_00b0ccb0`, invalidates all strip button IDs and `0xF0`, sets `IsOpen = false`, calls `FUN_006d09c0` (recalculate positions).

**Open**: Adds thumb button at `DAT_00b0cc40`, invalidates `0xF1`, sets `IsOpen = true`, calls `FUN_006d1200` (full sidebar open with all strip gadgets).

Tooltip strings: `"Tip:ThumbClosed"` (at `0x00842838`), `"Tip:ThumbOpen"` (at `0x00842848`).
IsOpen flag at SidebarClass offset `+0x5544` (`param_1[0x1551]`).

---

## 32. EVA Voice Events (NEW)

| EVA String | Address | Trigger |
|---|---|---|
| `EVA_NewConstructionOptions` | `0x0083fa64` | New buildable items appear in sidebar |
| `EVA_ConstructionComplete` | `0x0083fa80` | Build finished (ready to place/deploy) |
| `EVA_Building` | `0x0083fb38` | Construction started |
| `EVA_Training` | `0x0083fb48` | Infantry training started |
| `EVA_UnableToComply` | `0x0083fb58` | Cannot build (prerequisites/funds) |
| `EVA_OnHold` | `0x0083fb6c` | Build paused (right-click during production) |
| `EVA_SelectTarget` | `0x0083fb78` | Super weapon ready, select target |
| `EVA_Canceled` | `0x0083fb8c` | Build cancelled |
| `EVA_LowPower` | `0x0082473c` | Low power condition |
| `EVA_InsufficientFunds` | `0x00819044` | Not enough money |
| `EVA_UnitReady` | `0x008249a0` | Unit completed training |

Button click sound: `FUN_00750920(0x3f800000, 0)` = volume 1.0f (full).

---

## 33. FactoryClass Integration (NEW)

### FactoryClass Field Layout (verified)

| Offset | Field | Description |
|---|---|---|
| 0x24 | Production.Value | 0..54 (StageClass), returned by `GetProgress()` |
| 0x28 | Production.HasChanged | bool |
| 0x2C | Production.Timer.StartTime | Start frame of embedded `CDTimerClass` |
| 0x30 | Production.Timer.pad | Rewritten alongside timer starts; semantic purpose remains unknown |
| 0x34 | Production.Timer.Duration | Duration read by `CDTimerClass::GetTimeRemaining`; remaining time is computed from current frame minus `StartTime` |
| 0x38 | Production.Rate | Per-stage interval. AI tests it directly and copies it into `Timer.Duration +0x34` after each stage advance |
| 0x3C | Production.Step | Always 1 |
| 0x40-0x57 | QueuedObjects | DynamicVectorClass<TechnoTypeClass*> |
| 0x58 | Object | TechnoClass* currently being built |
| 0x5C | OnHold | bool |
| 0x60 | Balance | Credits still owed |
| 0x64 | OriginalBalance | Total cost |
| 0x68 | SpecialItem | int (-1 = none) |
| 0x6C | Owner | HouseClass* |
| 0x70 | IsSuspended | bool |
| 0x71 | IsManual | bool (player caused suspension) |

> **Corrected 2026-07-11.** The prior fresh correction incorrectly absorbed
> `+0x38` into the timer. `FactoryClass::AI` passes `this+0x2C` to
> `CDTimerClass::GetTimeRemaining`, then separately tests `this+0x38`; after a
> stage advance it copies `+0x38` into timer word `this+0x34`.
> `FactoryClass::SetRate` and `RecalcAllRates` independently compute/write
> `+0x38`, while the latter does not write the timer (verified via
> `disassemble_function 0x004C9B20`, `decompile_function 0x00426630`,
> `disassemble_function 0x004C9EA0`, and `disassemble_function 0x004CA6E0` —
> ROOT_CAUSE: STRUCT_FAMILY_CASCADE + OFFSET_RETYPED_WRONG).

### Key Functions

- `GetProgress` (`0x004ca120`): returns `*(this + 0x24)` — range 0..54
- `IsComplete` (`0x004ca130`): true when `Value == 0x36` AND (Object != null OR SpecialItem != -1) (corrected 2026-05-28: was `HasCompleted`; binary label `FactoryClass__IsComplete` — ROOT_CAUSE: RTTI_LABEL_DRIFT)
- `Suspend` (`0x004c9e60`): sets `IsManual +0x71` from its argument and `IsSuspended +0x70 = true`, zeros the separate `Production.Rate +0x38` and timer `Duration +0x34`, and writes the current frame to timer `StartTime +0x2C` (the timer pad is also rewritten). It does not clear `Production.Value +0x24`, so progress is preserved (corrected 2026-07-11 via `disassemble_function 0x004C9E60` — ROOT_CAUSE: STRUCT_FAMILY_CASCADE + OFFSET_RETYPED_WRONG).
- `SetRate` (`0x004c9ea0`): for eligible suspended production, clears `IsSuspended`, computes `clamp(GetBuildStepTime() / 54, 1, 255)`, writes that value to both `Production.Rate +0x38` and timer `Duration +0x34`, and starts the timer. It then checks the next installment against available credits; its boolean argument can request a manual re-suspend after that check (corrected 2026-07-11 via `disassemble_function 0x004C9EA0` — ROOT_CAUSE: INFERENCE_HARDENED + OFFSET_RETYPED_WRONG).
- `RecalcAllRates` (`0x004ca6e0`): recomputes and writes `Production.Rate +0x38` for factories owned by the specified house; it does not rewrite the active timer duration (corrected 2026-07-11 via `disassemble_function 0x004CA6E0` — ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT).
- `StartProduction` (`0x004c9c70`): either creates the current object after zeroing `Production.Rate +0x38`, timer `Duration +0x34`, and `Production.Value +0x24`, or appends the requested type to `QueuedObjects` when the current slot is busy (corrected 2026-07-11 via `disassemble_function 0x004C9C70` and `get_function_by_address 0x004C9C70` — ROOT_CAUSE: RTTI_LABEL_DRIFT + OFFSET_RETYPED_WRONG).
- `CountTotal` (`0x004ca670`): counts current + queued of a type (for display)

### Production Queue

- Max queue size: `MaximumQueuedObjects` from `rules.ini` (default **29**)
- Stored in `RulesClass + 0xF0`
- Queue is `DynamicVectorClass<TechnoTypeClass*>` with dynamic resize
- `StartNextQueued` (`0x004ca5a0`): when the queue is nonempty and `Object +0x58` is null, dequeues the first item only if `Production.Rate +0x38 == 0` or `IsSuspended +0x70 != 0`, then routes it through the house begin-production path (corrected 2026-07-11 via `disassemble_function 0x004CA5A0` — ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT; function identity remains verified by `get_function_by_address 0x004CA5A0`).

### On-Hold Behavior

- **Progress is preserved** — `Production.Value` never reset by Suspend
- Manual pause dispatches SUSPEND (0x0F), whose house handler calls
  `FactoryClass::Suspend(true)` (`decompile_function 0x006AAD00` and
  `decompile_function 0x004FA910`).
- Automatic suspension is a separate availability/buildability path:
  `UpdateRadar` calls `Suspend(false)` when the current item no longer passes its
  production check, and calls `SetRate` when availability returns only if
  `IsManual == false` (`decompile_function 0x005091C0`).
- Insufficient funds do **not** automatically set `IsSuspended`. On a timer
  expiry, `FactoryClass::AI` sets `OnHold +0x5C`, reverses the just-added stage,
  and leaves the rate/timer cycle active; when the installment is affordable it
  clears `OnHold`, spends the credits, and keeps the stage (`disassemble_function
  0x004C9B20`).
- Visual: the "On Hold" text branch tests `FactoryPtr != null &&
  (Production.Rate +0x38 == 0 || IsSuspended +0x70 != 0)`, or cameo
  `BuildState == 2`; it does not read the low-funds `OnHold +0x5C` byte. The
  DARKEN.SHP buildability overlay is a separate draw condition
  (`disassemble_bytes 0x006A9E88..0x006A9EC8` and `decompile_function
  0x006A9540`).

> **Corrected 2026-07-11.** The prior text conflated the low-funds `OnHold`
> retry state with automatic `Suspend(false)` and treated `SetRate` as an
> “Unsuspend” helper specialized to low funds. The binary has distinct
> mechanisms (verified by the calls above — ROOT_CAUSE: INFERENCE_HARDENED +
> OPERATOR_OR_ORDER_DRIFT).

---

## 34. SuperClass Integration (NEW)

### SuperClass Field Layout (verified)

| Offset | Field | Description |
|---|---|---|
| 0x24 | CustomChargeTime | Overrides Type->RechargeTime if set |
| 0x28 | Type | SuperWeaponTypeClass* |
| 0x2C | Owner | HouseClass* |
| 0x30 | RechargeTimer | CDTimerClass (12 bytes) |
| 0x6D | IsPresent | bool — SW granted to player |
| 0x6F | IsReady | bool — fully charged |
| 0x70 | IsSuspended | bool — on hold |
| 0x7C | ChargeDrainState | 0=Idle, 1=Charging, 2=Draining |

### GetProgressBarFrame (`0x006cbee0`)

- Returns 0..54 (same range as FactoryClass)
- 0 = not started/not granted
- 54 = fully charged (non-ChargeDrain types only)
- 53 = max for in-progress (capped at 0x35)
- Normal SWs: `progress = (total - remaining) / total × 54.0`
- ChargeDrain SWs: separate calculation based on ChargeDrainState

### NameReadiness (`0x006cc2b0`) (corrected 2026-05-28: was `GetStatusText`; binary label is `SuperClass__NameReadiness` via `get_function_by_address 0x006cc2b0` — ROOT_CAUSE: RTTI_LABEL_DRIFT)

| Condition | CSF String |
|---|---|
| Suspended | `TXT_HOLD` (0x3B6) |
| Ready (normal SW) | `TXT_READY` (0x3B0) |
| ChargeDrain Idle | `TXT_READY` (0x397) |
| ChargeDrain Charging | `TXT_CHARGING` (0x39A) |
| ChargeDrain Draining | (0x39D) |
| Still recharging | NULL (no text) |

---

## 35. Palette System (NEW)

### Two Palettes for Sidebar SHPs

LoadSHPs (`0x006a5840`) creates two separate ConvertClass objects:

**Palette #1** (`DAT_00b0fbe4` → ConvertClass at `DAT_0087f6cc`):
Used for sidebar chrome: SIDE1/2/3, ADDON, GCLOCK2, TAB00-03, R-UP, R-DN, SELL, REPAIR

**Palette #2** (`DAT_00b0fbfc` → ConvertClass at `DAT_0087f6d0`):
Used for faction icons: OBSALLI, OBSSOVI, OBSYURI, RANI, OBSI, USAI, JAPI, FRAI, GERI, GBRI, DJBI, ARBI, LATI, RUSI, YRII

Both originate from SIDEBAR.PAL but processed differently via `FUN_0072ade0`.
A third palette is also loaded in `FUN_0072ddb0` for surface shell pieces.

### Side-Specific MIX Files

Format strings (at `0x00827dd4`+):
- `"SIDENC%02d.MIX"` — neutral/shared sidebar content
- `"SIDEC%02d.MIX"` — faction-specific sidebar content
- `"SIDEC%02dMD.MIX"` — YR faction-specific sidebar content

---

## 36. Additional INI Keys (NEW)

| Section | Key | Description |
|---|---|---|
| [Video] | AllowVRAMSidebar | Use VRAM for sidebar surface (offset +0x36 in options) |
| [Options] | SidebarCameoText | Show text on cameos (offset +0x1D in options, gates `FUN_006ac480`) |
| [Options] | Sidebar | Always 1 (RIGHT side) |
| [Sides] | SidebarImage | Per-side sidebar image override |
| [General] | FlashSidebarTabFrames | Tab flash animation frame count |
| [General] | MaximumQueuedObjects | Max production queue size (default 29) |
| — | SideBarSize | Sidebar dimensions (width/height override) |

---

## 37. Verification Summary

All constants verified by decompiling raw C code from each function. Verification pass
covered 20+ functions with complete decompiled output.

### Corrections Applied (from verification)

1. **StripClass +0x48**: was "VisibleRows" → corrected to `ScrollCounter` (scroll request delta, decrements to 0; visible rows are computed dynamically)
2. **Repair/Sell IDs** (2026-05-20 audit re-correction; the prior "swap" claim here was itself wrong): **0x65 = Repair** (gadget at `DAT_00b0b3ac`, handler `FUN_004ac8c0`); **0x66 = Sell** (gadget at `DAT_00b07e04`, handler `FUN_004ac660`). Verified via `decompile_function 0x006A5310` (`SidebarClass::Init`).
3. **GCLOCK2 frame range** (corrected 2026-07-25): the SHP stores frames
   0..54; ordinary linked, incomplete progress draws 1..54. Valid completion
   at progress 54 skips GCLOCK, so that path does not pass frame 55.
4. **Network event names**: 0x0F = "SUSPEND" (not "Cancel"), 0x10 = "ABANDON" (not "Place"), 0x0B = "PLACE"
5. **Draw order**: PowerBar::Draw was missing between StripClass::Draw and blit
6. **Scroll button IDs**: matched via globals `DAT_00b0b34c`/`DAT_00b0b42c`, not hardcoded literals
7. **Status text dual positioning**: when queue count shown, shifts from (x+0x1E, y+1) centered to (x+2, y_bottom+1) left-aligned
8. **Sort order**: corrected 2026-07-10 — super weapons sort BEFORE ordinary entries; `InsertEntry` inserts before the first existing entry for which `CompareItems` returns true (verified via `decompile_function 0x006A8420` and `decompile_function 0x006A8710` — ROOT_CAUSE: OPERATOR_OR_ORDER_DRIFT)
9. **+0x53A8**: only checked in `BlitToScreen` (`FUN_006a70e0`), not in `SidebarClass::Draw` itself

### Constants Confirmed Correct

All layout pixel values, offsets, cameo dimensions (60×48), row height (50), sidebar width (158),
tab spacings (29/32), column widths (63/64), scroll button dimensions (46/45), scroll speed (50),
visible rows formula, local-side field location, SelectClass stride (0x38), StripClass stride (0xF94),
CameoEntry stride (0x34), and all SidebarClass instance offsets (+0x1544, +0x539C, +0x53A5-A8)
were confirmed exactly matching the raw decompiled code.

---

## Unverified (YELLOW)

Items the 2026-05-20 audit did NOT re-verify against the binary. They
may be correct, partially correct, or wrong — treat as not load-bearing
until a follow-up audit confirms.

- **§13 Palette / SHP filename string addresses.** `0x0084542c = "SIDEBAR.PAL"`,
  `0x008204e0 = "CAMEO.PAL"`, `0x00830630 = "SIDEFNT3.PAL"`, plus all the SHP
  asset filenames in §13. Not verified per-address via `read_memory`. Follow-up
  needs a string-read per cited address.
- **§32 EVA voice event string addresses.** `EVA_NewConstructionOptions @ 0x0083fa64`
  through `EVA_UnitReady @ 0x008249a0`. Not verified per-address via `read_memory`.
- **§22 Radar/minimap pixel offset constants** (11/14 X, 4/5 Y, etc.). Function
  0x00652E90 not decompiled in this audit pass.
- **§21 Command bar internal layout** (`FUN_006D0FD0`, `FUN_006D1200`) beyond
  the gadget-ID range. Not decompiled in this audit pass.
