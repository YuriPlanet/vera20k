# Ferry order

A VERA feature; the original game has no such order.

Select one or more transports that stand at the pick-up spot, press the
**Ferry** key (J by default) and left click the drop spot. Each transport then
carries units from where it stood to the clicked spot and comes back for more,
until it is told to do something else. The drop spot is the cell a Move click
there would send the transport to. A second press of the key, or a right
click, drops the armed click again.

After the key is pressed, a thin blue circle shows around each selected
transport (where it loads) and a thin yellow one around the cell under the
cursor (where it unloads). Where the click would give no transport a route
(too near the pick-up spot, or a cell it cannot move to) both circles are red
and the cursor is the refusal cursor.

While a ferrying transport is selected, the same blue and yellow circles mark
its route, and its waypoints show between them.

The circles lie on the ground: buildings, units and trees standing on them
cover them, as they cover the ground itself.

## Waypoints

After the Ferry key, hold the **Planning Mode** key (Z by default) and left
click to add a waypoint; up to 8. Let go of it and click to set the drop spot.
The transport passes the waypoints in order on the way out and in reverse on
the way back, so one detour round an enemy covers both directions.

The waypoints look as they do in Planning Mode: a square marker on each,
joined by the moving dashed line in your colour, from the pick-up spot through
the waypoints to the drop spot. While you are still clicking them they are
drawn in the brighter colour of waypoints not yet committed. Holding the key
gives Planning Mode's start and end sounds and the Move cursor; over a cell
none of the selected transports can move to, the cursor shows the refusal and
the click is not taken. A route without waypoints has no line, only its two
circles.

The ferry keeps these waypoints itself; they are not Planning Mode's own
nodes, which the game does not give to aircraft. So they cannot be clicked,
deleted or joined into a loop as those can: to change them, give the ferry
order again.

- **No stopping.** A ground transport is sent on to the next point within 2
  cells of a waypoint, an aircraft within 4, before it slows to land.
- **Several transports.** Sent to one waypoint together, they are each given a
  cell near it, as any group move is. Each is sent on from its own cell.
- **Blocked.** A transport that stops near a waypoint (within 8 cells) carries
  on to the next point. One that stops farther away was sent elsewhere, and
  its ferry ends. A waypoint a transport cannot be sent to at all is left out.

## The four legs

- **Loading.** The transport waits at the pick-up spot. Any of your infantry
  and vehicles that comes to a stop inside the circle (4 cells around the
  spot) is told to board, nearest first, while there is room and the transport
  would take it. Units only driving through are left alone. Units can also be
  clicked onto the transport as usual.
- **Leaving.** It leaves when it is full, or when it holds someone and nobody
  has been boarding for 75 game frames (5 seconds at 15 frames a second). An
  empty transport keeps waiting. A unit that is on its way but does not get
  aboard holds the transport for at most 300 frames.
- **Crossing.** It heads for the free cell nearest the drop spot inside the
  drop zone (3 cells around the spot): one that holds nothing, that no other
  ferrying transport is heading for, that it can move to and that has an
  empty cell beside it for the passengers. So several transports, or units
  already set down, do not block the drop. When every such cell of the zone
  is taken, it lands on the nearest one up to 4 cells outside the zone.
- **Unloading.** Where it stops it unloads as it does when clicked on
  itself. A transport on water first drives to the nearest land, as that
  order always does. If some passengers find no way out, it moves to
  another free cell with room for them and unloads the rest there.
- **Returning.** Empty, it moves back to the pick-up spot and loads again.

## What ends a ferry

- The **Stop** key with the transport selected.
- A new ferry order for the transport, which replaces the old route.
- The transport is destroyed, changes owner or boards another transport.
- The transport is found away from its route: more than 7 cells from the
  pick-up spot while loading, or stopped more than 3 cells from the end of a
  crossing. So ordering it somewhere else ends the ferry, and so does a drop
  spot it cannot reach.

A drop spot within 7 cells of the pick-up spot is refused, because units set
down inside the circle would board again.

## What it is and is not

- Any vehicle or aircraft of a type with `Passengers=` can ferry: the
  amphibious transports, land transports such as the Flak Track, and the
  Nighthawk.
- The game only sends orders it already has: Enter for the boarding units, an
  ordinary Move for each crossing and the transport's own Unload. Nothing new
  goes over the network, and other players' games need no knowledge of it.
- A route belongs to the player's own game. It is not kept in a save game,
  and computer players do not use it.
- Units standing idle inside the circle are picked up too, so keep units that
  should stay behind outside it.

## Keys

The key is **Ferry** in the VERA category of Keyboard Options and can be
changed or removed there. Removing it turns the feature off. The original
keyboard file leaves J unused.

## Code

`src/app/input/ferry.rs`.
