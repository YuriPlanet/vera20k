# One reverse search for a mass order

A rules key, off by default. With it off, every unit that walks to a cell
searches its own way there, as in the original game.

```ini
[VERA]
SharedGoalField=1   ; default off
```

A mod's `rulesmd.ini` or a map can set it; a map carries the key the way it
carries any other section. In an online game every player needs the same
rules, and the rules and map digests the room compares cover the key.

## What the original does

Every Foot that wants a path gets one of its own: `FootClass::Find_Path`
(`0x004D3920`) runs `AStar_pathfind_search` (`0x0042C900`), which asks the
Foot about every neighbour of every cell it expands (`Foot+1AC`,
`0x00429F54`). The search minimises the mover's own cost over the cells it
may enter, and the route it returns is the route the original game gives.

A player who selects six hundred tanks and clicks one cell gives all six
hundred the same destination, so all six hundred ask the same question and
each pays for a search of its own. VERA's path batch runs those searches side
by side, but it does not share them: the batch's cap answers about a hundred
a tick and the rest wait for the next.

## What the option changes

- The registrations that head for the same goal cell share one reverse search
  over the map's ground plane. The field gives every cell the step that
  shortens its way to the goal, and each mover's path is a walk down that
  gradient instead of a search of its own.
- The walk asks the mover about one cell per step. The search asked about the
  eight neighbours of every cell it expanded, and expanded every cell around
  the route as well.
- The mover's own admission decides every cell of the walk, as it decides
  every cell of the search. A cell the mover refuses sends that search back
  to the stock one, so the field is a guide and never a route of its own.
- The field is kept between frames, so a mass order that names its goal every
  frame pays for the build once. The ground walkability plane's revision names
  its state: a cell that opens or closes (a bridge lost, a wall built) drops
  the fields.
- A group whose own paths would not repay the build keeps the stock search.
  The build visits every cell of the plane once, and the searches it replaces
  ask about eight neighbours each, so the build pays for itself when the
  group's paths cover the map sixty-four times over.
- The movers of a mass order share one route, not one route each: the field
  sends them all through the same cells, where the search's own cost would
  have spread them around a narrow gap. A funnel that is a cell wide becomes
  a queue, and the queue is the mode's price for the search it saves.

What the mode does not change:

- A mover on a deck, or one heading for a deck, keeps the stock search: the
  field's plane is the ground one.
- An infantryman keeps the stock search: he walks where a terrain object
  stands, so his field would be another plane. The field serves vehicles.
- A jam re-path keeps the stock search: its urgency makes the search route
  around a jammed cell (the code-2 multiplier, `AStar_compute_edge_cost`,
  `0x00429830`), and a count of steps does not know that.

## What it costs, and what it saves

The routes are different: the field's route is the shortest one in steps over
the ground plane, and the search's is the shortest in the mover's own cost, so
a game that turns this on plays differently from a game that does not, and its
frame hashes do not hold. A cell the field cannot see (a crushable wall it
closes, a cell the mover may enter at a cost the field does not know) sends
that mover's search back to the stock one, so the mode costs the stock search
in those cases and never less than it.

## Online play

The switch is data, so it travels with the rules or the map. Two players whose
rules or maps differ are refused by the rules compatibility check as usual. A
game from before this option reads no key and plays the stock search, and the
build check refuses a room that mixes it with a game that has the option, as
it does for any other change. The routes differ between a game with the option
and one without, so the two must not play the same match; that is what the
build check and the compatibility check are for.
