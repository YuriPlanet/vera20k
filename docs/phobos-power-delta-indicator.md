# Optional Phobos power-delta indicator

Build with `--features phobos` to draw the net power beside the credits
counter. An unmodified game shows only the power bar; this is Phobos's
`PowerDelta` readout.

```ini
[Sidebar]                        ; uimd.ini
PowerDelta.Show=no               ; boolean
PowerDelta.ConditionYellow=0.75  ; double, `75%` is accepted
PowerDelta.ConditionRed=1.0      ; double, `100%` is accepted

[ToolTips]                       ; uimd.ini
PowerLabel=                      ; CSF label, default ⚡ (U+26A1)
PowerBlackoutLabel=              ; CSF label, default ⚡❌ (U+26A1 U+274C)

[GDISide]                        ; rulesmd.ini, one section per side
Sidebar.PowerDelta.Offset=0,0    ; X,Y pixels
Sidebar.PowerDelta.ColorGreen=0,255,0
Sidebar.PowerDelta.ColorYellow=255,255,0
Sidebar.PowerDelta.ColorRed=255,0,0
Sidebar.PowerDelta.ColorGrey=128,128,128
Sidebar.PowerDelta.Align=left    ; left | center | centre | right

[Phobos]                         ; ra2md.ini, imported into the VERA.INI profile
ShowPowerDelta=yes               ; boolean, default yes
```

With both switches on and the house owning at least one building, the sidebar
draws the label followed by `PowerOutput - PowerDrain`, always with a sign
(`⚡+60`, `⚡-100`). The colour tracks the drain-to-output ratio rather than that
number: below `ConditionYellow` green, up to `ConditionRed` yellow, above it
red. No output with any drain reads red, and an empty base (0 and 0) reads
yellow. While the house is in a power blackout — an enemy spy in a power plant
or a force shield — the number is dropped and `PowerBlackoutLabel` is drawn in
`ColorGrey`.

## Native site

Phobos draws all three of its sidebar indicators from one hook,
`DEFINE_HOOK(0x4A25E0, CreditsClass_GraphicLogic_HarvesterCounter, 0x7)`
(`src/Misc/Hooks.UI.cpp:101`), inside the stock credits tick. It returns before
any indicator when `HouseClass::CurrentPlayer->Defeated`, then opens each branch
on its own conjuncts. The power-delta branch (`:137-173`) asks
`PowerDelta.Show`, `[Phobos] ShowPowerDelta` and a non-empty `House+68`
building vector; it reads the ratio as a `double`, compares
`percent < ConditionYellow` for green and Phobos' epsilon-tolerant
`LESS_EQUAL(percent, ConditionRed)` — `(x - y) <= 0.001` — for yellow, formats
`L"%ls%+d"`, and anchors at `DSurface::Sidebar->GetWidth() / 2 - 70`, `y = 2`
plus the side's `Offset`. The blackout test is `PowerBlackoutTimer` in progress,
the same timer `HouseClass::CreatePowerOutage @ 0x50BC90` starts for a spy.
`Sidebar.PowerDelta.Align` is a `TextAlign` whose values (`0x000/0x100/0x200`)
are exactly the `TextPrintType` left/centre/right bits, so Phobos ORs them
straight into the draw flags; the harvester and weeds branches hardcode centre.

## Where VERA20K owns it

- `rules/phobos_sidebar_indicators.rs` - the `uimd.ini [Sidebar]`/`[ToolTips]`
  reads and the per-side `Sidebar.PowerDelta.*` reads, with the constructor
  defaults, and `SidebarTextAlign`.
- `rules/ruleset.rs` - `side_power_delta`, the per-side list in `[Sides]` order,
  read the same way `Sidebar.GDIPositions` is.
- `app/persistence/phobos_settings.rs` - the `[Phobos] ShowPowerDelta` client
  switch from the imported profile.
- `app/loading/init_helpers.rs` - loads `uimd.ini` and resolves the two label
  keys through the loaded CSF table, which is when Phobos reads them.
- `app/presentation/sidebar_render.rs` - `refresh_sidebar_indicators` resolves
  the string, colour and anchor every frame, on the same per-frame pass as the
  power bar (`app/match_runtime/sim_tick.rs`), because the native hook runs in
  the credits tick rather than on a cameo transition.
- `render/sidebar_text.rs` - `build_indicator_instances` places the string on the
  sidebar surface with the same GAME.FNT path and depth as the credits counter,
  honouring the alignment bits.

## Residuals

- Trigger: the label glyph, or any digit, is a codepoint the loaded font table
  has no entry for. Effect: VERA draws its missing-glyph placeholder. Retail
  `GAME.FNT` has no `⚡` (U+26A1); Phobos documents the same and points at its
  enhanced font pack, so a real check of this indicator needs that pack and a
  running game.
- Trigger: `Sidebar.PowerDelta.Align=none`. Effect: VERA treats it as rejected
  text and keeps the previous alignment, where Phobos ORs `TextAlign::None`
  (`0xFFF`) into the flags, which also sets its font-size bits.
- Trigger: a label key the CSF table does not hold. Effect: VERA keeps the glyph
  default where native renders `MISSING:'<key>'`.
- The harvester counter (`[HarvesterCounter](phobos-harvester-counter.md)`) and
  the weeds counter (`WeedsCounter.*`) are the other two branches of `0x4A25E0`.
  The weeds branch still needs the unported TS weeds mechanic for its number.
