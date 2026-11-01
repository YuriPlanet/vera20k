# Optional Phobos harvester counter

Build with `--features phobos` to show the working and total harvester counts
beside the credits counter, Phobos's `HarvesterCounter` readout.

```ini
[Sidebar]                          ; uimd.ini
HarvesterCounter.Show=no           ; boolean
HarvesterCounter.Label=            ; CSF label, default ⛏ (U+26CF)
HarvesterCounter.ConditionYellow=0.99  ; double, `99%` is accepted
HarvesterCounter.ConditionRed=0.5      ; double, `50%` is accepted

[GDISide]                          ; rulesmd.ini, one section per side
Sidebar.HarvesterCounter.Offset=0,0     ; X,Y pixels
Sidebar.HarvesterCounter.HideMaxValue=no  ; show only the working count
Sidebar.HarvesterCounter.OnlyMaxValue=no  ; show only the total count
Sidebar.HarvesterCounter.ColorGreen=     ; unset: the sidebar tooltip colour
Sidebar.HarvesterCounter.ColorYellow=255,255,0
Sidebar.HarvesterCounter.ColorRed=255,0,0

[HarvesterType]                    ; rulesmd.ini
Harvester.Counted=                 ; boolean, defaults to the type's own flags

[Phobos]                           ; ra2md.ini, imported into the VERA.INI profile
ShowHarvesterCounter=yes           ; boolean, default yes
```

With both switches on and the player undefeated, the sidebar draws
`[label][working]/[total]`. `HideMaxValue` drops the `/total` half (it wins when
both format switches are set); `OnlyMaxValue` shows only the total. The colour
follows the working share: strictly above `ConditionYellow` green, strictly
above `ConditionRed` yellow, otherwise red — so at exactly 50% the counter is
already red, and a house with no counted harvester reads `1.0` and stays green.
`⛏` (U+26CF) is not in the retail font table; Phobos documents the same and
points at its enhanced font pack.

## Which TechnoTypes count

`[TechnoType] Harvester.Counted=` decides membership. Phobos leaves it unset,
then falls back in its own read order: the TechnoType pass sets it for any type
with `Enslaves=` (a slave miner), and the UnitType pass after it sets it for a
type with `Harvester=yes` (`Ext/TechnoType/Body.cpp:796-798`,
`Ext/UnitType/Body.cpp:96-97`). An explicit `no` therefore wins over both
fallbacks, and a building only counts when the key says so — which is how an
oil-derrick style `ProduceCash=` building joins the count.

The two numbers come from `HouseExt::TotalHarvesterCount`, which adds each
counted Techno that has left limbo once (`TechnoExt::HasBeenPlacedOnMap`, so
construction alone is not counted but a placed unit still counts from inside a
transport), and `HouseExt::ActiveHarvesterCount`, which adds those
`TechnoExt::IsHarvesting` answers for (`Ext/Techno/Body.cpp:78-129`): a live,
powered, non-limbo Techno whose slave manager is busy, a counted building that
is powered online, or a unit on Harvest, Unload, Enter or Guard with one of its
`Dock=` buildings present. A unit still radio-linked inside its factory, or one
flagged useless, is counted but not working.

## Native site

Phobos draws all three of its sidebar indicators from
`DEFINE_HOOK(0x4A25E0, CreditsClass_GraphicLogic_HarvesterCounter, 0x7)`
(`src/Misc/Hooks.UI.cpp:101`), inside the stock credits tick. It returns before
any indicator once `HouseClass::CurrentPlayer->Defeated`. The harvester branch
(`:111-135`) opens on `HarvesterCounter.Show` and `[Phobos] ShowHarvesterCounter`
only — unlike the power delta it asks nothing about owned buildings — and draws
at `DSurface::Sidebar->GetWidth() / 2 + 50`, `y = 2` plus the side's `Offset`,
with `TextPrintType::Center` hardcoded. That base column is identical to the
weeds counter's, which is why Phobos documents the two as overlapping.

## Where VERA20K owns it

- `rules/object_type.rs` - the `Harvester.Counted=` read and its two fallbacks.
- `rules/phobos_sidebar_indicators.rs` - the `uimd.ini [Sidebar]` keys and the
  per-side `Sidebar.HarvesterCounter.*` reads; `rules/ruleset.rs` keeps them in
  `[Sides]` order as `side_harvester_counter`.
- `sim/house_tracking/phobos.rs` - `harvester_counts`, the pair of
  `HouseExt::ActiveHarvesterCount`/`TotalHarvesterCount`, walking the entity
  store instead of keeping native's `OwnedCountedHarvesters` vector: spawn,
  death, capture and `TypeFrom=` conversion need no membership bookkeeping, and
  the same `+0x5500` per-type counts already answer `CountOwnedAndPresent`.
- `app/persistence/phobos_settings.rs` - `[Phobos] ShowHarvesterCounter`.
- `app/presentation/sidebar_render.rs` - `harvester_counter_indicator` picks the
  string, colour and anchor each frame, in the same pass as the power bar.

## Residuals

- VERA walks the store for the two counts on the local player's frame instead of
  maintaining Phobos' house vector. Same numbers, one pass per frame; a
  20,000-object match pays the scan rather than the bookkeeping.
- `TechnoExt::IsHarvesting`'s Enter arm reads this object's one radio link
  (`tether`), where native asks `GetNthLink(0)` from the link array. VERA models
  a single dock link today, so the two agree in every reachable case.
- The weeds counter (`WeedsCounter.*`) is the hook's third branch and needs the
  unported TS weeds mechanic for its data; see the issue.
