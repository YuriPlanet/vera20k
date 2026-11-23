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
- The walk is not a corridor search. The hierarchy's zones are the native
  search's own filter over the cells it expands (`0x00429E7F..`), and the
  native drops them when its precheck fails and searches the whole map plain
  (`0x0042CB8B..0x0042CBA1`, `0x0042CB22..0x0042CB3F`). A field is shared by
  every mover heading for the goal, and the corridor is one search's own
  route from its own start, so the walk cannot hold to it for all of them.

What the mode does not change:

- A mover on a deck, or one heading for a deck, keeps the stock search: the
  field's plane is the ground one.
- An infantryman keeps the stock search: his cell gate is the Infantry one
  (`0x0051BF90`, against the vehicle's `0x0073F0A0`), and it lets him into a
  cell a terrain object stands in, so his plane is not the field's. The class
  decides this, not the locomotor: a vehicle that walks is still gated as a
  vehicle, and walks the field's plane.
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

Measured on the 20,000-unit march (the release build, the flat map, 30
houses, 300 ticks), with every unit of a house heading for one cell as a
player's mass order does:

| | the stock search | the shared field |
| --- | --- | --- |
| tanks and infantry: expansions | 96,104,932 | 92,031,902 |
| tanks and infantry: registrations left waiting | 2,726,612 | 2,699,497 |
| tanks only: expansions | 209,391,078 | 56,317,215 |
| tanks only: registrations left waiting | 4,457,527 | 3,190,317 |
| tanks only: fields built and used | 0 | 30 built, 145 used |

So the mode takes the search away from every registration the field serves,
and on a load of tanks it takes three quarters of the expansions and a quarter
of the waiting with them: a walk down the field is cheap enough that the batch
answers more of the queue a tick. The frame time follows the same way on that
load, where the field's side was the faster one in each of the three measured
rounds; how much faster is not pinned down, because the machine's own load
moved the frame time twofold between the rounds. The field's own work is a
visit of every cell of the map per build, and it is not small: on the mixed
load the 102 builds visited 12,724,500 cells, three times the 4,073,030
expansions the field took away, which is why that load gains so little.

The price is in the jam. The movers of a house share one route, so a funnel
that is a cell wide becomes a queue, and the re-paths around that jam grow:
on the mixed load they grew from 5,197,613 expansions to 5,946,136, and the
frame time itself did not move.

## Online play

The switch is data, so it travels with the rules or the map. Two players whose
rules or maps differ are refused by the rules compatibility check as usual. A
game from before this option reads no key and plays the stock search, and the
build check refuses a room that mixes it with a game that has the option, as
it does for any other change. The routes differ between a game with the option
and one without, so the two must not play the same match; that is what the
build check and the compatibility check are for.

The digest covers the section's text, not its effect. A map that carries
`[VERA] SharedGoalField=0` and the same map with no section at all play the
same game: on the 20,000-unit march they answered the same 31,957
registrations and left the same 2,721,382 waiting, and the field was built on
neither. They are still two different games to the room's check, because the
map's INI is digested as it reads (`IniFile::multiplayer_digest`, the sha256
of the file's text) and the section is part of that text.
