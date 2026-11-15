# House control

A VERA feature: who plays each House can change while a match runs, and a new AI House can join it.
The original game fixes both when the match starts.

Open **Diplomacy** in a skirmish or network match. Under each House's name a line says who plays
it:

- **Player**: a person. Clicking another House's line until it says Player makes it yours; your
  old House then stands without a player.
- **Easy AI**, **Medium AI**, **Brutal AI**: the game's own computer player at that level takes
  over the House and its base, as when a mission hands a base to the computer.
- **Nobody**: nobody gives orders; the House's units and defences still fight back.

Each click steps to the next choice. Below the last House, **+ Add AI House (enemy)** adds a Medium
AI House of a random country, at war with everybody, with an MCV on open ground far from every
base and the match's starting money; **+ Add AI House (ally)** adds one allied to you and your
allies.

## In a network match

Each change is an ordered command that every player's game runs at the same frame, like any
order, so replays and saves keep it. Any player may change any House. A player whose House went
to the AI or to nobody stays in the match: their game keeps time with the others, but their orders
are dropped until they take a House again. A player who leaves (or is lost) loses their House as in
the original game.

A game from before this feature does not know the two commands; it is another version and is
refused by the build check before the match starts ([Networking](Networking.md)).
