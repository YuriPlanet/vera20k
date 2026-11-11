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

A refused Jumpjet flies to the nearest cell within `LandNearby=` cells of the
refused one that it may enter outright, that is neither water nor beach and
that no other flying object holds, and lands there. Equally near cells are
taken by the lower row first, then the lower column; no random draw is spent.
If that cell is taken too when it gets there, it searches again from it. With
no such cell in range it hovers as in the original.

Not changed: a landing over water or beach, which holds as before, and the
cells a group is given when the order is made.

Code: `nearby_landing_cell` and `state4_descend` in
`src/sim/movement/jumpjet_flight.rs`.
