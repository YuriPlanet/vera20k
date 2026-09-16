# Radar Object Dot Priority / Visibility Gates -- Ghidra Research Report

**Address(es):** `0x00655C50` (`RadarClass::RenderCellPixel`), `0x00656150` (`RadarClass::RenderAllCells`), `0x00655560` (`RadarClass::AddObjectToTracker`), `0x00656750` (`RadarClass::GetObjectAtRadarPixel`)
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** live in-game `RadarClass` object-dot selection, tracker ordering, overlap winner, `RadarInvisible`, `RadarVisible`, shroud/fog, local/allied gates, and building/unit shared pixel selection.
**Non-Scope:** minimap surface sizing, click-to-camera transform, radar events, terrain dirtying outside object pixels, sidebar chrome, map-preview/lobby preview.
**Confidence:** High for the verified slice.
**Active in YR:** Yes. Evidence: `RadarClass::Update @ 0x00656EC0` calls `RadarClass::RenderCellPixel @ 0x00655C50` for dirty pixels, `TechnoClass::RegisterOnRadar @ 0x0070CC90` and `BuildingClass::RegisterOnRadar @ 0x00456580` populate the tracker, and `RadarClass::Update` blits the primary radar surface into the live sidebar path when radar mode/state are active.

## Working Notes Seed

- **Target question:** What exact object wins a live in-game minimap pixel, and which binary gates hide/show it?
- **Non-goals:** Do not re-open settled bridge dirty cells, minimap aperture, Soviet radar asset layout, or radar event diamond behavior.
- **Evidence needed to mark COMPLETE:** decompile plus assembly-context evidence for render iteration, tracker insertion/removal, click reverse search, parser offsets for `RadarInvisible`/`RadarVisible`, and Rust surface scan.
- **Stop conditions:** stop after every material gate in `RenderCellPixel`/`RenderAllCells` is resolved or explicitly deferred, and after stale prior-doc wording is identified.

## 1. Overview

The live minimap object-dot path is tracker-driven. Units and buildings insert one or more `{object, x, y}` entries into a 256-bucket tracker; dirty-pixel rendering scans the bucket from first to last and the first visible entry at the exact pixel wins. The no-shroud fast path overlays all tracked entries after a terrain blit and uses a visited bitfield so the first entry for each pixel wins there too.

The older broad radar docs were close on the tracker shape, but stale on one key gate: `type+0x232` in `RenderCellPixel` is not `RadarVisible` or `Cloakable`; it is `ObjectTypeClass.Insignificant`. `RadarVisible` is `TechnoTypeClass+0xC9B`.

## 2. Key Offsets / Fields

| Field | Meaning | Evidence | Active in YR |
|---|---|---|---|
| `RadarClass+0x1258` | radar object tracker: 256 bucket headers, 16-byte entries `{object,x,y,object}` | `0x00655560`, `0x00655C50`, `0x00656150` | Yes |
| `TechnoClass/Object+0x208/+0x20C` | current radar pixel X/Y used by unit registration | `0x0070CC90` | Yes |
| `TechnoClass/Object+0x423` | registered-on-radar flag, set after unit/building registration | `0x0070CC90`, `0x00456580` | Yes |
| `Object+0x21C` | owner house pointer | read in `0x00655DE5`, `0x00655F50`; House ally helper `0x004F9A90` | Yes |
| `ObjectType+0x22F` | `RadarInvisible=` | parser `0x005F946E..0x005F947F`, render read `0x00655DFF` | Yes |
| `ObjectType+0x232` | `Insignificant=`; stale docs mislabel this in radar section | parser `0x005F950A..0x005F951B` from prior audited docs; render read `0x00655E24` | Yes |
| `TechnoType+0xC9B` | `RadarVisible=` | parser context `0x00714AB1..0x00714ACC`, string `0x00843934`; render read `0x00655E3D` | Yes |
| `HouseType+0x1A6` | `MultiplayPassive=` | render read `0x00655E55..0x00655E60`; field identity from [COUNTRY_SIDE_TYPE_CLASSES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COUNTRY_SIDE_TYPE_CLASSES.md) and `rulesmd.ini:3343,3351` | Yes; true for stock Neutral/Special |
| `House+0x56F9..0x56FB` | raw RGB owner color for dirty-pixel path | `0x00655F7C..0x00655FE2` | Yes |
| `House+0x16054` | color scheme index for fast overlay path | `0x006561D2..0x00656232` | Yes |

## 3. Core Logic

### Tracker insertion and overlap priority

`RadarClass::AddObjectToTracker @ 0x00655560` hashes by `(x + y * -5) & 0xFF`, rejects exact duplicate `{object,x,y}`, and then inserts local-player objects at the front of the bucket while appending every non-local object at the back. Active in YR: Yes; direct callers are `TechnoClass::RegisterOnRadar @ 0x0070CC90` and `BuildingClass::RegisterOnRadar @ 0x00456580`.

This means the dirty-pixel draw winner is first matching visible entry in bucket order. Local-player entries inserted at the front are evaluated before enemy entries occupying the same minimap pixel, so local dots win visually when both pass gates. Active in YR: Yes; `RenderCellPixel @ 0x00655DC0..0x00655E78` scans forward and jumps to drawing at the first visible match.

The click path intentionally uses the reverse order: `GetObjectAtRadarPixel @ 0x00656750` starts at `bucket.count - 1` and walks backward. Active in YR: Yes. This makes click targeting prefer the last inserted entry among overlapping entries, while drawing prefers the first entry.

### Unit and building entries share the same pixel selection

Units call `AddObjectToTracker(this, object+0x208, object+0x20C)` once. Buildings call the same add helper once per foundation brush offset. Both produce identical 16-byte tracker entries and are consumed by the same `RenderCellPixel` and `RenderAllCells` loops. Active in YR: Yes (`0x0070CC90`, `0x00456580`, `0x00655C50`, `0x00656150`).

### Visibility gates in `RenderCellPixel`

For each tracker entry whose stored `(x,y)` equals the pixel:

1. If the pixel's underlying cell is shrouded/unexplored, the object is only eligible if `HouseClass::IsHumanPlayer(object.owner)` returns true. In multiplayer, that helper returns true only for `object.owner == g_PlayerPtr`; in single-player, it returns true for houses with the human/player-control bytes. Active in YR: Yes; assembly context `0x00655DDD..0x00655DF2`, helper `0x0050B6F0`.
2. If `type.RadarInvisible` at `+0x22F` is true, the object is eligible only when `HouseClass::Is_Ally_ByObject(g_PlayerPtr, object)` returns true. Active in YR: Yes; assembly `0x00655DFF..0x00655E17`, helper `0x004F9A90`.
3. If `type.Insignificant` at `+0x232` is false, the object is eligible. If `Insignificant` is true, then `RadarVisible` at `+0xC9B` can force eligibility; otherwise an owner with `HouseType.MultiplayPassive == false` can still be eligible. It skips only when `Insignificant=true`, `RadarVisible=false`, and either owner is null or owner country is `MultiplayPassive=true`. Active in YR: Yes; assembly `0x00655E24..0x00655E60`.
4. No current dynamic cloak state, cloak progress, `Invisible=` (`+0xC9A`), `CloakStop=` (`+0xC93`), sensor state, or gap-generator detection state is read in this object-dot gate. Active in YR: Yes as a negative finding; verified by the complete `RenderCellPixel` object gate pass and assembly context around `0x00655DC0..0x00655E78`.

If no entry passes, terrain fallback runs: fogged terrain is dimmed, shrouded terrain is black, and visible terrain is copied from the secondary surface. Active in YR: Yes; this part is covered by prior minimap reports and re-seen in `0x00655E7E..0x0065608F`.

### Color and flash

`RenderCellPixel` uses the object's actual or disguise house, then packs raw owner RGB bytes from `House+0x56F9..0x56FB` through the DirectDraw shift/loss globals. If no house exists, it uses the default color scheme helper. Selected/combat flash can invert the packed color only when the object owner is the local player and the `(remaining - 1) / FlashFrameTime` interval is odd. Active in YR: Yes; `0x00655F48..0x0065604B`.

`RenderAllCells @ 0x00656150` is a no-shroud fast overlay path: it scans all buckets forward, uses a visited bitfield to draw a pixel only once, resolves actual/disguise house, and takes a pre-packed color from the house color scheme palette. It does not re-run `RadarInvisible`, shroud, or `Insignificant` gates. Active in YR: Conditional; used by the no-shroud/full-overlay path documented in `RADAR_MINIMAP_RENDERING.md` and decompiled at `0x00656150`.

## 4. INI Keys

| Key | Storage | Default / stock relevance | Effect in this slice | Active in YR |
|---|---|---|---|---|
| `RadarInvisible=` | `ObjectType+0x22F`; parser `0x005F946E..0x005F947F` | default false; stock Night Hawk/sub-like units use it | hides from non-allied local player dots | Yes |
| `RadarVisible=` | `TechnoType+0xC9B`; parser `0x00714AB1..0x00714ACC`; string `0x00843934` | default false in constructor evidence from `TIBTRE...` report; stock civilian invisible/building cases use it | overrides the `Insignificant`/passive-owner skip branch; does not override `RadarInvisible` | Yes |
| `Insignificant=` | `ObjectType+0x232`; parser `0x005F950A..0x005F951B` in audited docs | default false for ObjectType; many special scenery/terrain variants set it | unexpectedly participates in radar dot eligibility before `RadarVisible` | Yes |
| `MultiplayPassive=` | `HouseType+0x1A6`; `rulesmd.ini:3343,3351` true for Special/Neutral | false for playable countries | passive-owner objects can be skipped when `Insignificant=true` and `RadarVisible=false` | Yes |

## 5. Integration Points

`RadarClass::Update @ 0x00656EC0` is the live owner. It restores dirty background, calls `RenderCellPixel` over dirty rectangles and the pixel dirty list, draws radar events/spy-satellite overlays, then blits the primary surface into the sidebar when radar is active.

Registration is virtual/object-driven: `TechnoClass::RegisterOnRadar @ 0x0070CC90` adds one pixel; `BuildingClass::RegisterOnRadar @ 0x00456580` adds every brush/foundation pixel. `RemoveObjectFromTracker @ 0x00655740` removes by object in the hashed bucket and preserves the order of the remaining entries.

## 6. Current Rust Implementation Status

Rust surface scanned via Codegraph and file reads:

- `src/render/minimap.rs:213` `MinimapRenderer::update_unit_dots` iterates `EntityStore::values()` and writes dots directly into an RGBA texture.
- `src/render/minimap.rs:284..325` applies `RadarInvisible`, `RadarVisible`, friendliness, fog/gap checks, then writes building and unit dots.
- `src/rules/object_type.rs:324` and `src/rules/object_type.rs:936` parse `RadarVisible` as a generic object field defaulting false.

Observed deltas:

- Rust iteration order is `BTreeMap<u64, GameEntity>` stable-id order, not native radar tracker bucket insertion order with local-front/non-local-back priority.
- Rust `RadarVisible` currently bypasses fog before `RadarInvisible`, but native `RadarInvisible` is checked before the later `Insignificant/RadarVisible` branch. `RadarVisible` does not rescue a non-allied `RadarInvisible` object.
- Rust hides non-friendly objects in unrevealed or gap-covered cells, while native shrouded-cell object eligibility first asks whether the owner is human/local in MP; it does not use the same friendly/fog/gap logic in this gate.
- Rust treats buildings specially: khaki color plus multi-pixel dots based on foundation. Native building entries use the same object-dot renderer as units and draw 1x1 pixels per registered brush pixel, colored by owner/disguise house, not khaki.
- Rust lacks `Insignificant` and `HouseType.MultiplayPassive` in this minimap gate.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `RenderCellPixel` object scan | verified | decompile `0x00655C50`; assembly context `0x00655DC0..0x00655E78` | none |
| `RadarInvisible` gate | verified | parser `0x005F946E`; render `0x00655DFF..0x00655E17`; ally helper `0x004F9A90` | none |
| `RadarVisible` gate | verified | parser `0x00714AB1..0x00714ACC`; render read `0x00655E3D` | none |
| `Insignificant` correction | verified | parser audit docs `0x005F950A..0x005F951B`; render read `0x00655E24` | none |
| dynamic cloak state in dot gate | verified negative | no cloak-state field read in `0x00655DC0..0x00655E78`; no `+0x430`/cloak progress in object gate | none |
| tracker insertion priority | verified | `0x00655560` | none |
| building vs unit registration | verified | `0x0070CC90`, `0x00456580` | none |
| no-shroud `RenderAllCells` path | verified for selection and color; touched for caller selection | `0x00656150`; prior `RADAR_MINIMAP_RENDERING.md` | exact runtime condition frequency not re-measured |
| current Rust minimap status | verified by source scan | `src/render/minimap.rs:213`, `src/rules/object_type.rs:936` | implementation not changed in this slot |

## 8. Open Questions -- Final State

- `[RESOLVED] OQ1 -- Is this live in ordinary YR radar? -> Yes, `RadarClass::Update` calls `RenderCellPixel` on active radar dirty pixels and registration callers populate tracker entries.` (evidence: `0x00656EC0`, `0x0070CC90`, `0x00456580`)
- `[RESOLVED] OQ2 -- Which entry wins an overlapped dirty pixel? -> First visible matching entry in the bucket wins.` (evidence: `0x00655DC0..0x00655E78`)
- `[RESOLVED] OQ3 -- How does local ownership affect priority? -> Local-player entries insert at bucket front; non-local entries append.` (evidence: `0x00655560`)
- `[RESOLVED] OQ4 -- Does click selection use the same winner as drawing? -> No, click walks the bucket backward from `count-1`.` (evidence: `0x00656750`)
- `[RESOLVED] OQ5 -- Do buildings and units use the same pixel selection? -> Yes; both register through `AddObjectToTracker` and are consumed by the same renderer.` (evidence: `0x0070CC90`, `0x00456580`, `0x00655C50`)
- `[RESOLVED] OQ6 -- Is `RadarInvisible` overridden by alliance? -> Yes, allied objects pass; non-allied objects skip.` (evidence: `0x00655DFF..0x00655E17`, `0x004F9A90`)
- `[RESOLVED] OQ7 -- Is `RadarVisible` the byte at `type+0x232`? -> No; `RadarVisible` is `TechnoType+0xC9B`; `+0x232` is `Insignificant`.` (evidence: `0x00714AB1..0x00714ACC`, `0x005F950A..0x005F951B`)
- `[RESOLVED] OQ8 -- Does the dot path read current cloak state? -> No dynamic cloak-state read appears in the object eligibility block.` (evidence: `0x00655DC0..0x00655E78`)
- `[RESOLVED] OQ9 -- What does `HouseType+0x1A6` mean? -> `MultiplayPassive`, true for stock Special/Neutral.` (evidence: [COUNTRY_SIDE_TYPE_CLASSES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COUNTRY_SIDE_TYPE_CLASSES.md), `rulesmd.ini:3343,3351`)
- `[RESOLVED] OQ10 -- Is Rust currently using the native priority model? -> No; it iterates `EntityStore` order and overwrites pixels.` (evidence: `src/render/minimap.rs:284..325`)
- `[DEFERRED] OQ11 -- Exactly when does native choose `RenderAllCells` vs dirty `RenderCellPixel` in every radar mode?` (category: out-of-scope; reason: this slot only needed object winner/gates, and `RenderAllCells` behavior itself was decompiled; next-step-if-pursued: trace the no-shroud/full-refresh mode selector in `RadarClass::Update` and `RefreshRadar`)

## 9. Visual/UI Composition Ledger

| Order | Function / address | Condition / flag proof | Asset / frame | Rect / anchor | Palette / convert | Active for target? | Role |
|---|---|---|---|---|---|---|---|
| 1 | `RadarClass::Update @ 0x00656EC0` background restore | dirty rect/list or radar state changed | secondary radar terrain surface | primary radar surface dirty rects | DirectDraw 16-bit | yes | clean terrain restore |
| 2 | `RadarClass::RenderCellPixel @ 0x00655C50` | each dirty pixel | none | one primary-surface pixel | raw owner RGB packed through DD shifts | yes | object/terrain pixel |
| 3 | `RadarClass::RenderAllCells @ 0x00656150` | no-shroud/full overlay path | none | one primary-surface pixel per tracker entry | color-scheme palette entry | conditional | object overlay |
| 4 | `TickAndDrawRadarEvents` / spy satellite helpers | later in `Update` | radar event / spy-sat assets | primary/sidebar radar area | separate event/shape paths | yes, after object dots | overlay |
| 5 | `BSurface::Blit` in `Update` | active radar mode/state | primary radar surface | sidebar radar aperture | surface blit | yes | final minimap content |

Asset role matrix:

| Asset | Loaded | Drawn | Visible in target | Content/preview | Chrome/container | Overlay | Transition-only | Inactive | Evidence |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| generated primary radar surface | yes | yes | yes | content | no | no | no | no | `0x00656EC0` |
| object tracker entries | yes | yes | yes | content | no | no | no | no | `0x00655560`, `0x00655C50` |
| sidebar/radar SHPs | out-of-scope | out-of-scope | yes elsewhere | no | yes | no | yes elsewhere | no | settled sibling docs |

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Bucket order controls overlap: local-owner entries insert at front, non-local append; draw scans forward and first visible match wins | `0x00655560`, `0x00655C50`, `0x00656150` | mismatch: `EntityStore` iteration overwrites by stable id | `src/render/minimap.rs::update_unit_dots`; possibly a radar-dot staging helper | stage dot candidates by native radar pixel and resolve with native local-front/non-local-back order before writing | local Grizzly and enemy Rhino mapped to same minimap pixel; local owner's color wins on drawn pixel | Do not rely on BTreeMap/stable-id order or last-writer-wins |
| `RadarInvisible` gate precedes `RadarVisible`; non-allied `RadarInvisible=yes` skips even if later fields would otherwise show | `0x00655DFF..0x00655E17`, `0x004F9A90` | mismatch risk: Rust checks `radar_visible` first and lets it bypass fog/gates | `src/render/minimap.rs::update_unit_dots`; rules type fields | evaluate `RadarInvisible` before any `RadarVisible` override; allied objects still show | enemy Night Hawk with `RadarInvisible=yes RadarVisible=yes` remains hidden to local non-ally; allied one shows | Do not treat `RadarVisible` as a universal always-show flag |
| Buildings and units share object-dot renderer; buildings are not khaki multi-pixel foundation blobs in the object-dot path | `0x00456580`, `0x0070CC90`, `0x00655C50`, `0x00656150` | mismatch: Rust uses `COLOR_BUILDING` and variable dot size | `src/render/minimap.rs::update_unit_dots`; building foundation mapping | draw one owner-colored pixel for each native registered building brush pixel; use same visibility gates as units | overlapping player barracks/rifleman pixel uses owner color and native priority, not khaki/foundation-size blob | Do not reuse map-preview building color behavior for the live minimap |

Suggested Rust test names:

- `minimap_overlap_local_owner_front_entry_wins_draw_pixel`
- `minimap_radar_invisible_not_overridden_by_radar_visible_for_enemy`
- `minimap_building_dots_use_owner_color_single_tracker_pixels`

## Negative Facts / Do Not Do

- Do not label `type+0x232` in `RenderCellPixel` as `RadarVisible` or `Cloakable`. It is `ObjectTypeClass.Insignificant`; `RadarVisible` is `TechnoType+0xC9B`. Evidence: parser/read contexts `0x005F950A..0x005F951B`, `0x00714AB1..0x00714ACC`, render reads `0x00655E24` and `0x00655E3D`. Active in YR: Yes.
- Do not implement live minimap buildings as map-preview khaki blobs. `CellClass::GetRadarColor` uses khaki for terrain/background building-cell color, but tracked live object dots use owner/disguise house colors. Evidence: `0x00655F48..0x0065604B`, `0x006561D2..0x00656240`. Active in YR: Yes.
- Do not make `RadarVisible` override `RadarInvisible`. The binary tests `RadarInvisible` first and skips non-allied entries before reaching the later branch. Evidence: `0x00655DFF..0x00655E17` before `0x00655E24..0x00655E60`. Active in YR: Yes.
- Do not use dynamic cloak state as a minimap-dot hide gate until separately proven elsewhere. This object-dot path does not read cloak state/progress or sensors. Evidence: complete render gate pass `0x00655DC0..0x00655E78`. Active in YR: Yes as a negative path fact.
- Do not use click target priority as draw priority. Draw scans forward; click scans backward. Evidence: `0x00655DC0..0x00655E78` vs `0x00656750`. Active in YR: Yes.

## Remaining Uncertainty

- The exact full-mode selector for when `RenderAllCells @ 0x00656150` is chosen instead of dirty-pixel `RenderCellPixel` was not exhaustively traced because this slot focused on object-dot selection and gates. The function's internal winner/color behavior is verified.

## Stale Docs / Replacement Wording

- `docs/research/RADAR_MINIMAP_RENDERING.md` section "Per-Cell Pixel Rendering: RenderCellPixel (0x00655C50)" currently describes the `type+0x232` branch as a "Cloaking check" / `Cloakable` and refers to `+0xC9B` as `CloakStop`. Replacement wording:
  - "After `RadarInvisible`, `RenderCellPixel` reads `ObjectTypeClass+0x232` (`Insignificant=`), not `RadarVisible` or `Cloakable`. If `Insignificant` is false, the object is eligible. If it is true, the code reads `TechnoTypeClass+0xC9B` (`RadarVisible=`); `RadarVisible=true` restores eligibility. Otherwise the object is eligible only when it has an owner whose `HouseTypeClass+0x1A6` (`MultiplayPassive=`) is false. No dynamic cloak state is read in this minimap-dot gate."
- [docs/research/RADAR_MINIMAP_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_MINIMAP_DEEP_DIVE.md) key insight says local objects draw on top and click searches backward for enemies. That remains directionally correct, but should be tightened:
  - "Local-player tracker entries insert at the front and draw scans forward, so local visible entries win the drawn pixel. Click lookup scans backward, so it can return a different overlapping object than the drawn pixel."

## Sources

- Ghidra decompile: `0x00655C50`, `0x00656150`, `0x00655560`, `0x00655740`, `0x006565A0`, `0x00656750`, `0x00656EC0`, `0x0070CC90`, `0x00456580`, `0x0050B6F0`, `0x004F9A90`.
- Ghidra assembly context: `0x00655DC0..0x00655E78`, `0x00714AB1..0x00714ACC`, `0x005F946E..0x005F947F`.
- Existing docs: `RADAR_MINIMAP_RENDERING.md`, [RADAR_MINIMAP_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADAR_MINIMAP_DEEP_DIVE.md), [COUNTRY_SIDE_TYPE_CLASSES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COUNTRY_SIDE_TYPE_CLASSES.md), [TIBTRE_BUILDING_EXCEPTION_BYTES_0XC9A_0X1701_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_BUILDING_EXCEPTION_BYTES_0XC9A_0X1701_GHIDRA_REPORT.md).
- INI: `ini/rulesmd.ini:3343`, `ini/rulesmd.ini:3351` (`MultiplayPassive=true` for Special/Neutral).
- Rust: `src/render/minimap.rs:213`, `src/render/minimap.rs:284..325`, `src/rules/object_type.rs:324`, `src/rules/object_type.rs:936`.
