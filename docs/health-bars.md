# Health-strip display modes

`[VERA] HealthBars=` chooses which objects draw a health strip. gamemd draws one
for the selected object (with the PIPBRD bracket) and a bare strip for the object
under the cursor; every other object draws nothing. This is a VERA display mode:
`sim/` never reads it, so saves, replays, network sync and the AI are unaffected.

It lives in `VERA.INI` for now; there is no Options control yet. The profile
writer stores the key again with the other `[VERA]` values.

| Value | Behavior |
|---|---|
| `0` (default) | gamemd's rule: selected draws the strip and bracket, the hovered object draws a bare strip. |
| `1` | Every object the local player can see draws a bare strip when it is not selected; selected objects keep the bracket. |
| `2` | Mode 1, except an unselected object at full health draws nothing. Hovering is not selecting, so a hovered full-health object stays bare. |

A value this build does not know reads as `0`, so a hand-edited file never hides
the original strip.

## Which objects count as seen

Own and allied objects always count. Another house's object needs its cell
explored — and, under the `FogOfWar` game option, currently visible — the same
reveal gate screen selection uses. The strip's PIPS fills use the 600 material,
which reads no A, so the shroud plane that blacks out an unexplored enemy's body
does not hide its pips; that gate is what keeps the strip out.

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

## Residuals

- The visible-object count times the pip count (17 for mobiles, 8 for infantry)
  is the extra instance load in modes 1 and 2. No 20,000-unit frame-time
  measurement is recorded yet.
- A hovered full-health object is hidden in mode 2 by design; treating hovering
  as selection would be a different mode.
- `docs/ini-commander-personalities.md` and other product extensions keep their
  own presentation switches; this option is independent of them.
