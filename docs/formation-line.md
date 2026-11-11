# Formation line

A VERA feature; the original game has no such order.

Select two or more units, hold the **Formation Line** key (F by default) and
drag with the left mouse button. A row of green dots follows the drag, one per
selected unit. On release every unit moves to its own dot, so the group lines
up along whatever shape was drawn. A drag too short to be a line is an
ordinary click.

- Each unit gets an ordinary Move to its point, the same order a click there
  would give it alone. In attack-move mode the units attack-move instead.
  Nothing new goes over the network.
- Units are matched to points so that the group travels as little as it can.
  A group of more than 128 is cut into pieces together with the line, the
  units nearest each stretch of line taking that stretch, and each piece is
  matched the same way.
- The dots are in the colour of the green line a Move order draws. They lie
  on the ground: buildings, units and trees standing on them cover them.
- Buildings in the selection take no point.
- With fewer than two units selected the key does nothing and the drag is the
  usual selection box.

## Formation block

Hold **Shift** together with the Formation Line key and drag with the left
mouse button. The drag is now the block's front row, and the dots show the
selected units standing in rows behind it. On release every unit moves to its
dot.

- The drag's length sets how many units stand side by side. The rest fill
  rows behind, one unit in every cell.
- Behind is to the right of the drag: drag from left to right and the rows
  fill towards the bottom of the screen, drag from right to left and they
  fill towards the top.
- The front row runs the nearest of eight ways that stay on the map's cells:
  across the screen, down the screen, or along either side of the cells'
  diamonds. Across and down the screen the rows lie like bricks, every other
  one shifted half a cell.
- A short last row sits in the middle.
- A click without a drag gives a block across the screen, about as wide as it
  is deep, with its corner at the click.
- Shift counts when the mouse button goes down. The orders are the same
  ordinary Moves as for the line.

## Formation lock

Select two or more units and press **Formation Lock** (O by default). The
units are now one locked formation: each remembers where it stands relative to
the others, taking the cell it is heading for when it is still on the move. A
message says how many units were locked.

- A move or attack-move click on the ground sends each locked unit to its own
  place around the clicked cell, so the group arrives in its shape. That
  holds for a queued (Shift) and a forced (Alt) ground move too. The original
  game would send them all to that one cell and spread them.
- The lock belongs to the units, not to the selection. Put a locked formation
  in a team (Ctrl+1) and selecting the team moves it in shape; a second
  formation in a second team keeps its own shape.
- Several locked formations ordered together each keep their shape and also
  where they stand relative to each other.
- Part of a formation ordered alone keeps its part of the shape.
- If any selected unit is not locked, the click is the original one.
- Pressing the key on units that are one locked formation unlocks them.
  Pressing it on anything else locks the selection as a new formation.
- Only the destination keeps the shape. On the way each unit drives at its
  own speed along its own path.
- Other orders (attack, enter, deploy, Planning Mode) are the original ones.
- The lock is this player's own note about their units. It is not sent to
  other players and not saved: it ends with the match, and a loaded game
  starts with nothing locked.

## Keys

The keys are **Formation Line** and **Formation Lock** in the VERA category of
Keyboard Options and can be changed or removed there. Removing a key turns
that feature off; the block goes with the line's key. Shift with the Formation
Line key belongs to the block: no other command can be given that pair, and
one that already has it no longer runs.

F is the original key for **Follow**. While Formation Line has its default
key, Follow moves to V. Putting Follow back on F in Keyboard Options removes
Formation Line's key, and it stays removed until a key is set for it again.

## Code

`src/app/input/formation_line.rs` holds the line, the block and the matching;
`src/app/input/formation_lock.rs` holds the lock.
