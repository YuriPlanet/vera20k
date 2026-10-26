# Optional Phobos superweapon sidebar

Build with `--features phobos` and set `[Sidebar] SuperWeaponSidebar=yes` in
`uimd.ini` to put superweapon cameos in a left-edge column like C&C3. Default is
`no`.

```ini
[Sidebar]
SuperWeaponSidebar=false            ; boolean, default no
SuperWeaponSidebar.Pyramid=true     ; boolean, default yes
SuperWeaponSidebar.Interval=0       ; pixels between columns, default 0
SuperWeaponSidebar.LeftOffset=0     ; cameo offset from the column's left edge
SuperWeaponSidebar.CameoHeight=48   ; cameo pitch, clamped to at least 48
SuperWeaponSidebar.Max=0            ; first-column cap, 0 derives it from resolution
SuperWeaponSidebar.MaxColumns=      ; column cap, default unlimited
```

`Interval` adds to the fixed 60-pixel cameo width to form the column background
width. `CameoHeight - 48` is the gap between cameos. `Max` is clamped to
`(screen height - 96) / CameoHeight`, the source's 96-pixel vertical reserve.

In `rulesmd.ini`:

```ini
[GlobalControls]
ToggleSuperWeaponSidebarKeyEnabled=true ; boolean, default yes
SuperWeaponSidebarKeysEnabled=false     ; boolean, default no

[AudioVisual]
SuperWeaponSidebar.AllowByDefault=false ; boolean, default no

[SOMESIDE]                              ; Side
SuperWeaponSidebar.OnPCX=               ; expanded-state collapse button
SuperWeaponSidebar.OffPCX=              ; collapsed-state expand button
SuperWeaponSidebar.TopPCX=              ; column top frame
SuperWeaponSidebar.CenterPCX=           ; per-cameo background and side frames
SuperWeaponSidebar.BottomPCX=           ; column bottom frame

[SOMESW]                                ; SuperWeaponType
SuperWeaponSidebar.Allow=               ; unset falls back to AllowByDefault
SuperWeaponSidebar.PriorityHouses=      ; owners in the list sort first
SuperWeaponSidebar.RequiredHouses=      ; empty means every owner
SuperWeaponSidebar.Significance=0       ; integer, default 0
SW.ShowCameo=true                       ; boolean, left-sidebar membership gate
SW.UseAITargeting=false                 ; boolean, Ares constraint gate
SidebarPCX=                             ; Ares PCX cameo, wins over the SHP
SW.AITargeting=                         ; Ares mode, default from the Type's handler
SW.AITargeting.Constraints=             ; overrides the mode's default gate
```

In `RA2MD.INI` (VERA reads it through its imported client settings):

```ini
[Phobos]
SuperWeaponSidebar.RequiredSignificance=0 ; integer, default 0
```

Only superweapons with `Significance >= RequiredSignificance` are shown. With
`SuperWeaponSidebarKeysEnabled=yes`, the first column's first ten cameos get the
positional commands `FireTacticalSW1..10`; an assigned key replaces the ready
text on the cameo. Localize them with `TXT_FIRE_TACTICAL_SW_XX` and
`TXT_FIRE_TACTICAL_SW_XX_DESC` (`%d` receives 1..10), and the toggle with
`TXT_TOGGLE_SW_SIDEBAR` and `TXT_TOGGLE_SW_SIDEBAR_DESC`.

Superweapons the columns cannot hold keep their ordinary right-sidebar cameo.
Clicking a left cameo uses the same launch/selection path and sounds as the
right one; right-click clears the selected superweapon. Clicking the toggle
collapses or expands the columns and plays the GUI main-button sound, and the
collapsed state rides the local sidebar envelope in a save. Hovering a cameo
draws the tooltip-coloured one-pixel outline and the same name/cost tooltip as
the right cameo. With `--features ares`, a `SidebarPCX=` image replaces the SHP
cameo, and a superweapon with `SW.UseAITargeting=yes` whose
`SW.AITargeting.Constraints` fail darkens and refuses to launch, on the left
and, once it can fire, on the right.

The left sidebar lays out in native screen pixels, so the client's Sidebar size
does not scale or resize it. The keyboard settings page lists
`FireTacticalSW1..10` only while `SuperWeaponSidebarKeysEnabled=yes`, and the
toggle command only while `ToggleSuperWeaponSidebarKeyEnabled=yes`.

Source: `SWSidebarClass::AddButton`/`AddColumn`/`SortButtons`
(`src/Ext/Sidebar/SWSidebar/SWSidebarClass.cpp`),
`SWColumnClass::Draw`/`AddButton`/`SetHeight`
(`src/Ext/Sidebar/SWSidebar/SWColumnClass.cpp`),
`SWButtonClass::Draw`/`Action`/`LaunchSuper`
(`src/Ext/Sidebar/SWSidebar/SWButtonClass.cpp`),
`ToggleSWButtonClass::{Draw,Action,UpdatePosition,SwitchSidebar}`, the
`[Sidebar]` reads in `src/Phobos.INI.cpp`, `RulesExt`/`SWTypeExt` body reads,
and `src/Commands/FireTacticalSW.h`.

## Where VERA20K owns it

- `rules/phobos_sw_sidebar.rs` — `uimd.ini`, global switches and side artwork
  names.
- `rules/superweapon_type.rs` — per-type Allow, house lists, Significance and
  `SW.ShowCameo`.
- `ui/sw_sidebar.rs` — `AddColumn` capacity and `SortButtons` geometry.
- `app/presentation/sw_sidebar.rs` — membership, ordering, toggle state,
  per-frame draw state and hit testing.
- `app/presentation/sw_sidebar_build.rs` and `render/sw_sidebar_art.rs` — the
  PCX atlas and the five draw layers.
- `app/input/sw_sidebar.rs` and `app/input/dispatch.rs` — pointer input and the
  positional/toggle commands.

## Original project and authors

Phobos's "Exclusive SuperWeapon Sidebar" is credited to **NetsuNegi** and
**CrimRecya** in Phobos's `CREDITS.md`. VERA20K implements the behavior in Rust
and does not load or redistribute the Phobos DLL.

## Residuals

- An Ares constraint refusal plays the cameo click sound but shows no
  `Message.CannotFire` line; only the launch is withheld.
- A permitted click on a `SW.UseAITargeting=yes` type selects it for manual
  targeting, on either sidebar. Phobos's `LaunchSuper` and Ares's right-sidebar
  click hook (10034EB7..10034EE4) instead fire it at once at the cell Ares's AI
  targeting picks.
- Phobos darkens a cameo with `DARKEN.SHP` and the Darken blitter; the left
  sidebar halves the cameo's colour instead.
- VERA never writes the House's second defensive cell (+0x54F8), so
  `defensive_cell_set` always refuses and `defensive_cell_clear` never does.
- Builds with Phobos but without Ares do not read Phobos's own copies of
  `SidebarPCX`, `Money.Amount` and `SW.UseAITargeting`: the left sidebar draws
  the SHP cameo, never darkens for money, and launches `SW.UseAITargeting`
  types that Phobos without Ares refuses.
- Phobos reads the two `[GlobalControls]` switches once from `RULESMD.INI`;
  VERA reads them from each scenario's layered Rules.
- The left sidebar hovers over a cameo with no artwork still show a tooltip and
  outline, matching the native button's existence without art.
