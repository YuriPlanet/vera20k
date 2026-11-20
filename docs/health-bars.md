# Health-strip display modes

`[VERA] HealthBars=` chooses which objects draw a health strip. gamemd draws one
for the selected object (with the PIPBRD bracket) and a bare strip for the object
under the cursor; every other object draws nothing. This is a VERA display mode:
`sim/` never reads it, so saves, replays, network sync and the AI are unaffected.

Set it in **Options → Interface → Health Bars**. It applies at once and is
saved as `[VERA] HealthBars=` in `VERA.INI`; the profile writer stores the key
again with the other `[VERA]` values, and editing the file directly works too.

| Value | Behavior |
|---|---|
| `0` (default) | gamemd's rule: selected draws the strip and bracket, the hovered object draws a bare strip. |
| `1` | Mode 0, plus a bare strip on every other object the local player can see. |
| `2` | Mode 0, plus a bare strip on every other object the local player can see that is below full health. |

A value this build does not know reads as `0`, so a hand-edited file never hides
the original strip.

## Which objects count as seen

The strip follows the shroud plane the bodies use: the object's cell must be
explored and not covered by a hostile gap, for every house alike, so an object
pre-placed on unrevealed ground gets no strip either. Under the `FogOfWar` game
option the cell must also be in current sight. `MultiplayPassive` houses (stock Neutral and Special) get no
automatic strip in modes 1 and 2; selecting or hovering one still draws gamemd's
own strip. gamemd's disguise rule still applies: an enemy Mirage worn as a tree
draws no strip until it is selected, in every mode.

## Full health

`Health::is_full(Strength)` decides it, so an object at exactly `Strength` counts
as full, and a building still under construction does not. All technos take part:
infantry, vehicles, aircraft and structures.

## What the mode does not widen

Only the health strip follows the mode. The PIPBRD bracket stays selected-only,
and these keep gamemd's selected-or-hovered admission: cargo, ammo and garrison
pips, the self-heal icon, control-group digits, and Phobos `DigitalDisplay`.
With `phobos` compiled in, a shield bar drawn inside the health strip follows the
strip because it shares the same block.

## Phobos type switches

With `phobos` compiled in, four TechnoType keys change when a bar and its pip
call draw. All are `Valueable<bool>` defaulting to `no`
(`Ext/TechnoType/Body.cpp:761-764`).

| key | effect |
|---|---|
| `HealthBar.Hide=` | skips the whole health-bar tail (`0x6F64A9`): background, strip, pip call, shield bar and `DigitalDisplay`. The rank insignia draws earlier (`0x6F534E`) and stays. |
| `HealthBar.HidePips=` | skips the class pip call (`0x6F6637` buildings, `0x6F683C` mobiles) while the strip still draws. Scale pips, the self-heal icon and control-group digits share that call. |
| `HealthBar.Permanent=` | takes the hovered arm while the object is unhovered and unselected (`0x6F5E37`); the shroud and disguise tests still apply. |
| `HealthBar.Permanent.PipScale=` | lifts the pip call's hover/selection test (`0x6F67E8`/`0x6F6A58`), so pips and group digits stay with a permanent bar. |

These are presentation switches: they change no simulation state, saves or
network sync.

## Residuals

- The visible-object count times the pip count (17 for mobiles, 8 for infantry)
  is the extra instance load in modes 1 and 2. No 20,000-unit frame-time
  measurement is recorded yet.
- `docs/ini-commander-personalities.md` and other product extensions keep their
  own presentation switches; this option is independent of them.
- `HealthBar.Hide=` also gates Phobos's select-box hooks (`0x6D9076`/`0x6D9134`)
  together with `HideSelectBox`. VERA's select box reads neither key yet.
