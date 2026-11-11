# Jumpjets land beside a taken cell

A VERA option, off by default. With it off, Jumpjet units such as the
Nighthawk land as in the original game.

```ini
[JumpjetControls]
LandNearby=3   ; cells to search, default 0 (off), at most 8
```

A mod, a map or your [personal rules](personal-rules.md) (`VERARULES.INI`) can
set it. In an online game every player needs the same rules, which the rules
compatibility check compares.

## What the original does

A group ordered to one spot is given one cell each when the order is made. A
Jumpjet then descends on its cell, and if something stands there by the time
it arrives, the landing is refused and it stops and hovers over the cell
(`JumpjetLocomotionClass::State4_Descend`, `0x0054C550`). It does not look
for another cell. Units set down by the first transports, or anything else
that moved in, leave the later ones in the air.

## What the option changes

A Jumpjet whose landing cell is taken picks another: among the cells within
`LandNearby=` cells of the taken one, the cell nearest to itself that it may
enter outright, that is neither water nor beach, that no other flying object
holds and that no other airborne Jumpjet is heading for. Equally near cells
are taken by the lower row first, then the lower column; no random draw is
spent. With no such cell in range it hovers as in the original.

It looks in three places:

- **When the order is given.** A group's cells are handed out as before.
  Then a Jumpjet whose cell another airborne Jumpjet is already bound for,
  or an earlier Jumpjet of the same order was given, takes the nearest clear
  cell instead. A second group sent to the same spot gets cells beside the
  first's. An order to a single unit is not spread and relies on the
  approach check.

- **On the approach.** Every eighth frame within four cells of its
  destination, a Jumpjet on an ordinary move checks its landing cell. If the
  cell is taken, or an airborne Jumpjet with a lower id is heading for the
  same one, it picks its new cell then, so it loses no time.
- **On arrival.** If the cell is refused only when it descends, it picks one
  then and flies there.

Not changed: a landing over water or beach, which holds as before.

Code: `nearby_landing_cell` and `state4_descend` in
`src/sim/movement/jumpjet_flight.rs`.
