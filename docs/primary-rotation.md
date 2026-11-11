# Primary rotation

A VERA feature; the original game has no such order.

The original game has one primary factory of each kind, and every unit of
that kind comes out of it. Extra factories only make building faster. With
this feature several factories take turns being the primary, so their units
come out of each in turn.

Select two or more factories of one kind and press the **Rotate Primary** key
(I by default). A line says how many factories now take turns. Press the key
again with the same factories selected to stop.

- **Turns.** Each time you finish an object of that kind, the next factory of
  the group becomes primary, in the order you selected them. The unit after
  that comes out of it.
- **Kinds.** Barracks, War Factories, Naval Yards and Airfields each have a
  group of their own. Selecting factories of several kinds at once sets a
  group for each. Construction Yards are left alone: their primary decides
  where buildings are placed, not where anything comes out.
- **Rally points.** Each factory keeps its own, so a group can send every
  second unit somewhere else.
- **Losses.** A factory that is destroyed, sold or captured leaves its group.
  A group with fewer than two factories ends.
- **A new selection replaces the group** of that kind.

## What it is and is not

- The game only sends the order it already has for making a factory primary,
  the one a click on a factory sends. Nothing new goes over the network, and
  the one-primary rule itself is unchanged.
- The turn is taken when an object is finished, so the first object after the
  key is pressed may still come out of the old primary.
- Clicking a factory to make it primary yourself still works; the group
  carries on from that factory.
- War Factories and Naval Yards share one counter of finished vehicles, so a
  finished ship also turns the War Factory group, and a tank the Naval Yard
  group.
- A group belongs to the player's own game. It is not kept in a save game,
  and computer players do not use it.

## Keys

The key is **Rotate Primary** in the VERA category of Keyboard Options and can
be changed or removed there. Removing it turns the feature off. The original
keyboard file leaves I unused.

## Code

`src/app/input/primary_rotation.rs`.
