# Level high-bridge decks

A VERA difference from the original game, on for every map. A high bridge
keeps one deck height along its whole span, so it can cross ground that dips
or slopes. On a map whose bridges stand over flat ground, which is every map
the original game ships, nothing changes.

## What the original does

The original has no deck height of its own. An object on a bridge stands at
the ground under its own cell plus four levels
(`ObjectClass::SetHeight`, `0x005F5FA0`). Over flat ground that gives a level
deck. Over uneven ground it does not:

- a vehicle crossing a dip sinks below the drawn bridge;
- a slope under the span tilts the vehicle;
- move rings and order lines land on the ground under the deck;
- a high-bridge piece on a sloped cell is refused when the map loads
  (the slope gate in Overlay Mark, `0x005FC5CD`), which leaves a hole in the
  bridge.

Mapmakers therefore keep the ground under a high bridge flat.

## What VERA does

When a map loads, each run of high-bridge pieces in a line, concrete or
wooden, is one span, and the span gets one deck level:

- the level of its bridgehead tiles, the lower one when both ends are found;
- the level of the one bridgehead that is found, when the other is not;
- otherwise the lower end of the span's own ground plus four levels.

The level is stored on the span's bridge cells. Pathfinding, movement, weapon
heights and ranges, the bridge's picture and shadow, clicks on the bridge,
order lines and move rings all read it. A vehicle on the span stays level.
Units under the span follow the ground as before.

A span may stand over a slope that the original would refuse.

The level is recorded when the bridge is placed and kept after that: if the
ground under a standing bridge changes later, the deck stays where it was. A
destroyed segment that is rebuilt comes back at the same level.

## For mapmakers

Lay the bridge as usual: a straight run of high-bridge pieces between two
bridgeheads. The ground under the run may dip or slope below the deck, and
units can pass under it on that ground.

What still cannot be done:

- two bridges crossing or stacked over each other: a cell holds one bridge
  piece, at one height;
- a low bridge passing under a high bridge, for the same reason. Build a
  strip of land under the high bridge instead.

A map that relies on a level deck over uneven ground plays wrong in the
original game and in VERA versions from before this change.

## Online play

The multiplayer rules digest carries a version, which this change raised to
`v2` (`ProcessAssets::multiplayer_rules_digest`). A game from before the
change and one with it do not agree on rules, so the rules compatibility
check keeps them from starting a match together.

## Checked against the original maps

Each retail map that has high bridges was loaded and every bridge cell
compared: all 12,324 cells on the 74 maps get the deck level the original
rule gives.
