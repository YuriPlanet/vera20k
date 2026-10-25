# Optional Phobos DigitalDisplay

Build with `--features phobos` to register `[DigitalDisplayTypes]` and show
numeric unit information near the health bar. The feature follows Phobos
`DigitalDisplayTypeClass` (`src/New/Type/DigitalDisplayTypeClass.{h,cpp}`) and
`TechnoExt::ProcessDigitalDisplays`/`GetValuesForDisplay`
(`src/Ext/Techno/Body.Visuals.cpp`), which run inside
`TechnoClass::DrawHealthBar` at hooks `0x6F65D1` (buildings) and `0x6F683C`
(mobiles). The value mapping lives in `sim::digital_display`; positioning,
visibility and drawing live in `app::presentation::digital_display`.

Default builds keep the stock health bar and do not register the section.
A `phobos` build still needs the client switch on:

```ini
; RA2MD.INI [Phobos], imported once into VERA.INI by the profile loader
[Phobos]
DigitalDisplay.Enable=yes
```

The switch is read at startup from the player profile (`VERA.INI`, seeded from
`RA2MD.INI` on first run) and defaults to `no`. Changing it needs a restart,
as Phobos reads `RA2MD.INI` once per launch.

```ini
[DigitalDisplayTypes]
0=SOMEDIGITALDISPLAYTYPE

[AudioVisual]
Buildings.DefaultDigitalDisplayTypes=          ; List of DigitalDisplayTypes
Infantry.DefaultDigitalDisplayTypes=           ; List of DigitalDisplayTypes
Vehicles.DefaultDigitalDisplayTypes=           ; List of DigitalDisplayTypes
Aircraft.DefaultDigitalDisplayTypes=           ; List of DigitalDisplayTypes
DigitalDisplay.Health.FakeAtDisguise=true      ; boolean

[SOMEDIGITALDISPLAYTYPE]
InfoType=Health                 ; Health|Shield|Ammo|MindControl|Spawns|Passengers|
                                ; Tiberium|Experience|Occupants|GattlingStage|ROF|
                                ; Reload|SpawnTimer|GattlingTimer|ProduceCash|
                                ; PassengerKill|AutoDeath|SuperWeapon|IronCurtain|
                                ; TemporalLife|FactoryProcess
InfoIndex=0                     ; integer
Offset=0,0                      ; x,y
Offset.ShieldDelta=             ; x,y
Align=right                     ; left|center/centre|right
Anchor.Horizontal=right         ; left|center/centre|right
Anchor.Vertical=top             ; top|center/centre|bottom
Anchor.Building=top             ; top|lefttop|leftbottom|bottom|rightbottom|righttop
Percentage=false                ; boolean
HideMaxValue=                   ; boolean; unset means yes for Infantry
VisibleToHouses=all             ; none|owner/self|allies/ally|team|enemies/enemy|
                                ; neutral|all
VisibleToHouses.Observer=true   ; boolean
VisibleInSpecialState=true      ; boolean
ValueScaleDivisor=              ; integer; unset means 1, or 15 with ValueAsTimer
ValueAsTimer=false              ; boolean
ShowType=cursorhover,selected   ; cursorhover|selected|idle|all
Text.Color=0,255,0              ; r,g,b
Text.Color.ConditionYellow=255,255,0
Text.Color.ConditionRed=255,0,0
Text.Background=false           ; boolean
Shape=                          ; .shp filename; unset draws game-font text
Palette=palette.pal             ; .pal filename for Shape
Shape.Spacing=                  ; x,y; unset means 4,-2 for buildings, 4,0 otherwise
Shape.PercentageFrame=false     ; boolean

[SOMETECHNO]
DigitalDisplay.Disable=false    ; boolean
DigitalDisplayTypes=            ; List of DigitalDisplayTypes
DigitalDisplay.Health.FakeAtDisguise= ; boolean; unset inherits [AudioVisual] (true)
```

## Value source

`sim::digital_display::values_for_display` is the single owner of the mapping
from an info type to `(value, maximum)`; it only reads retained state. The
presentation skips a display when the value is negative or the maximum is not
positive, matching Phobos.

- Health reads the entity health and the type's `Strength`.
  `DigitalDisplay.Health.FakeAtDisguise` replaces the maximum with the
  disguised type's `Strength` and scales the current value when the viewer is
  not allied with the owner.
- Shield reads the Phobos shield points and type `Strength`; a broken,
  non-respawning shield draws nothing.
- Ammo, Passengers, Tiberium, Occupants and Experience read the existing ammo,
  cargo, storage, garrison and veterancy owners (`VeteranRatio * Cost`, with
  twice that as the maximum).
- MindControl reads the capture manager's count and `MaxControlNodes`.
- Spawns reads alive / docked / launching counts. Launching is alive minus
  docked, the Phobos `CountLaunchingSpawns` category. SpawnTimer scans the
  `Regenerating` slots and keeps the smallest nonzero remaining time.
- GattlingStage/GattlingTimer read the shared gattling state and the type's
  `WeaponStages` thresholds (elite stage table when elite).
- ROF and Reload read the rearm and ammo-reload timers. Reload falls back to
  `Reload=`/`EmptyReload=` when the timer has no duration.
- ProduceCash, SuperWeapon, IronCurtain and FactoryProcess are building-only.
  SuperWeapon selects `SuperWeapon`, `SuperWeapon2` or the Ares
  `SuperWeapons=` list by `InfoIndex`, then reads the owner's charge timer.
- TemporalLife reads `WarpRemaining` from the chain head's temporal link and
  uses `Strength * 10` as the maximum.
- `PassengerKill` and `AutoDeath` have no Phobos owner in VERA and never draw.

## Position and drawing

- Mobile positions use `GetFootSelectBracketPosition`: an 8-pip Infantry or
  17-pip mobile rectangle, then `Anchor.Horizontal`/`Anchor.Vertical`.
  Buildings use `GetBuildingSelectBracketPosition`: the projected Dimension2
  edge and hexagon vertex from `Anchor.Building`.
- `PixelSelectionBracketDelta` and the shield type's `BracketDelta` apply to
  both branches. `Offset`, then `Offset.ShieldDelta` (or the Shield info
  type's default `0,-10`), then the building `Top`/`LeftTop` adjustments
  follow.
- Text uses GAME.FNT, `FullShadow`, an optional black background and the
  green/yellow/red `Text.Color` triplet selected by health ratio. The Top
  anchor subtracts the native 12px text height plus 4px when a shield exists.
- `Shape=` digits use raw (non-centered) SHP draws. Digits are frames 0-9
  (green), 10-19 (yellow) and 20-29 (red); separators start at 30/32/34. Right
  alignment reverses the character order and spacing as Phobos does.
  `Shape.PercentageFrame=yes` instead draws one frame mapped from the ratio.
  A missing Shape or Palette file logs a warning and falls back to text.

## Where VERA20K owns it

- `rules/digital_display.rs` — the `[DigitalDisplayTypes]` catalog, enum
  spellings, defaults and `[AudioVisual]` globals.
- `rules/object_type.rs` — `DigitalDisplayTypes=`, `DigitalDisplay.Disable=`,
  and `DigitalDisplay.Health.FakeAtDisguise=`.
- `rules/native_processing.rs` / `rules/ruleset.rs` — ordered per-pass read,
  immutable projection, config hash and list resolution.
- `sim/digital_display.rs` — the read-only value projection.
- `app/presentation/digital_display.rs` — CanShow, anchors, formatting and
  text/SHP emission.
- `app/presentation/render/*` — status-run transport and the font/custom-SHP
  materials; `render/selection_overlay.rs` loads the custom SHPs once per match.
- `app/persistence/phobos_settings.rs` — the `[Phobos]` client switches.
- `app/presentation/ui_overlays.rs` — the building health-bar geometry and the
  foot select bracket, shared with the health and shield bars.

No simulation state is added: the feature is rules plus drawing, so saves keep
their existing layout apart from the new catalog in the process rules registry.
The catalog and every per-type binding are hashed under
`phobos-digital-display-v1`.

## Residuals

- `HealthBar.Permanent=` is not ported. Its `0x6F5E37` hook keeps the
  disguise checks, adds a shroud test and `HealthBar.Permanent.PipScale`; until
  that lands, only hovered or selected technos show displays and
  `ShowType=idle` never draws.
- `Shape=` gains `.shp` when the name lacks it, as Phobos's SHP reader does,
  but a later pass naming a file that fails to load replaces the earlier image
  instead of keeping it.
- `PassengerKill` and `AutoDeath` have no owners in VERA; those entries never
  draw even when authored.
- Ares-only `SuperWeapons=` names are available only when the `ares` feature is
  also built. `InfoIndex` 3+ draws nothing in a Phobos-only build.
- `FactoryProcess` shows the building's own factory. Phobos index 0 on a
  non-primary human building reads the house's primary factory, and index 2
  selects the separate defense primary; VERA has one primary per factory
  category, so those two arms draw nothing instead of another building's
  progress.
- Tiberium amounts are whole storage bales from VERA's `Storage` owner, not
  native float credit slots.
- Each text display draws its own background then its glyphs in list order.
  All SHP displays of one parent are submitted after that parent's text
  displays, regardless of their order in `DigitalDisplayTypes=`. Only
  overlapping text/SHP pairs could notice.
- The constructor’s `VisibleToHouses` default is `All`. The shipped Phobos
  documentation shows `owner` in its example, but the source constructor is
  authoritative here.
- Custom `Shape=` frame origins are applied as stored (non-centered), matching
  `BlitterFlags::None`; a shape authored for centered draws may sit differently.
- `ValueAsTimer` shape separators use the `%` sign frame: Phobos
  `ShapeTextPrinter::SignSequence` has no `:`, and the extra-frame base is
  selected by health colour, not by symbol. A shape that reserves a separate
  `:` band from the UI document will not draw it. Source is authoritative over
  the document for this quirk.
