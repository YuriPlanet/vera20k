# Status templates

VERA20K feature. A mod or a map can draw its own bars, pip rows, icons and
numbers at every object the local player sees: other health bars, an
energy bar from a module variable, an ammo row, an icon over a type. The
original's health strip, pips, marks and group numbers can be hidden.

Only this player's screen shows them: nothing goes over the network, and
saves and replays are not affected. A template never shows more than the
screen does: an object counts when it is drawn and is the player's own or in
its sight (current sight under `FogOfWar`). One disguised to the player shows
as what it wears: the worn type and house, its health share of the worn
`Strength=`, and none of its own amounts. A spectator reads only `Public` and
`InSight` variables.

## Where they come from

- The mod's `veraui.ini` and the map list theirs in `[VeraStatus]`:

  ```ini
  [VeraStatus]
  Templates=Energy,Ammo
  HideNative=HealthBars        ; HealthBars, Pips, Marks, Groups
  ```

  and give each a `[VeraStatus.<name>]` section.
- A client module (`ui.wasm`) calls `client_status` with the same keys,
  `Name=` first: Rust `StatusTemplates::set`, TypeScript
  `StatusTemplates.set`. A template of a name the module already has
  replaces it; one with no `ElementN=` removes it
  (`StatusTemplates::remove`). A module that stops loses its templates.

`HideNative=` (in `[VeraStatus]` or a template) hides those of the
original's families for the whole match. The mod's templates may; a map's
only when the mod sets `[VeraWasm] MapUI=full`, as for its nodes. A map's
templates are not read under `MapUI=no`.

## A template

```ini
[VeraStatus.Energy]
Category=Vehicle,Infantry          ; Vehicle, Infantry, Building, Aircraft; empty: all
Relation=Own,Ally                  ; Own, Ally, Enemy; empty: all
State=Selected                     ; Selected, Hovered, Damaged; empty: always
Types=MTNK,HTNK                    ; empty: every type
When1=AtLeast,Var:Energy,1         ; up to When16: Present, BelowMax, AtLeast, AtMost
Element1=Bar,-16,-30,32,3,Var:Energy/100,#000000,50:#00C0FF,0:#0040FF
Element2=Number,18,-34,Var:Energy,#FFFFFF
```

Up to `Element32=`, each `Kind,` then its values. Positions are pixels from
the object's anchor: where the original's health bar starts (a mobile's
centre lowered by `PixelSelectionBracketDelta=`, a building's first health
pip). They keep their pixel size under zoom.

| Kind | Values |
| --- | --- |
| `Bar` | x, y, width, height, amount, background (`#RRGGBB`, `#RRGGBBAA` or `none`), then bands `percent:#colour` (from that percent full upward) |
| `Segments` | x, y, width, height, step x, step y, count, amount, full picture, empty picture (optional) |
| `Icon` | x, y, width, height, picture (0 width or height: its own size) |
| `Number` | x, y, amount, colour |

A row as long as its amount (the original's own `Pips`) is not offered: it
may take a whole object's share of drawing commands. Use `Segments` with a
count; `count` pictures with the amount's share of them full, so a 10-step
level row is `Segments,...,10,Var:Level/10,star.shp,nostar.shp`.

Pictures are found by file name, extension included (`pips.shp`,
`mtnkicon.shp`, `mylogo.pcx`), as module nodes' `Art` is: any SHP or PCX the
game finds, drawn in the sidebar's palette (frame 0).

Amounts: `Health` (of `Strength=`), `Shield` (a Phobos shield), `Ammo` (of
`Ammo=`), `Passengers` (of `Passengers=`), `Rank` (0, 1, 2 of 2), and
`Var:<name>` an object-scope module variable with `/<max>` a number or
`Var:<name>` for its maximum. A variable shows only where this player may
read it: `Public` and `InSight` for any seen object, `Allies` for its own
objects and those of houses allied both ways, `Private` for its own. A variable name without `mod.`
or `map.` is the template source's own. An amount an object does not have
hides what reads it.

## Limits

256 templates in a match, of which the original's take what they need and
the others share three quarters; 32 elements and 16 conditions each; 384
drawing commands per object for all of them together; 16 distinct variable
amounts. A template past a limit is refused.

## Residuals

- The templates draw over the battlefield after its objects, not at each
  object's depth, so a building in front does not cover a unit's bar.
- `Storage` (ore and Tiberium loads) and production progress are not
  amounts yet; garrisoned buildings report no `Passengers`.
- `HideNative=HealthBars` hides a garrison's pips too: they share the
  strip's list.

## Code

`src/app/ui_modules/status.rs` (the templates, their text and the
amounts), `src/app/ui_modules/field.rs` (the layer drawn over the
battlefield), `ui_overlays::module_status_anchor` (which objects count and
their anchor), `presentation::frame::StatusFacts` (the amounts captured with
the frame), and `vera20k-gui`'s `status::World`.
