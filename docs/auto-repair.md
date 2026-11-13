# Auto Repair

A VERA feature; in the original game a player's buildings wait for the wrench.

Tick **Auto Repair** on the Gameplay page of Options. From then on each of
your damaged buildings starts repairing by itself, as if you had clicked it
with the wrench. It is off by default, takes effect at once and is saved with
your other options (`[VERA] AutoRepair=` in the options file). The box is in
the main menu's Options; the Options menu inside a match is the original
game's and does not have it.

## What it does

- Several times a second (every 15 game frames) your game looks at your own
  buildings and sends the ordinary Repair order to each one that is damaged
  and not repairing.
- The repair itself is the game's own: the same speed, the same cost per
  step, the same wrench over the building and the same flash as a click.
- An automatic start is quiet on your game: no click and no "Repairing"
  announcement. Your own wrench clicks still make both. Other players' games
  cannot tell the order was automatic, so one whose view is near your
  building hears the click, as it would for a wrench click.
- A building is left alone while it is being built or sold, and when its
  type cannot be repaired by a click.
- Only as many repairs start as your money pays a step for, after the steps
  of the repairs already running. The game stops a repair it cannot pay for;
  Auto Repair starts it again once you can.
- After an order, a building waits until the order has surely arrived, at
  least 90 frames, before it can get another. The order is a switch, so a
  second one sent before the first had arrived would turn the repair off
  again. Online the order can take a while to arrive, for example while your
  game catches up.
- At most 16 buildings are ordered per look; the rest follow at the next.

## What it is and is not

- Each player chooses for themselves. The game only sends an order it already
  has, so nothing new goes over the network, other players' games need no
  knowledge of it, and it works in a match with players on other versions.
- Switching a building's repair off by hand does not last while the box is
  ticked: the building is ordered again after its wait. Untick the box to stop
  repairs.
- Computer players do not use it; they keep the original game's own repair
  rule (`[IQ] RepairSell=`).
- It is an option of your own game, not part of the match. A replay shows the
  repairs anyway, because it holds the orders that were sent.

## Code

`src/app/input/auto_repair.rs`; which buildings want the order is
`buildings_wanting_repair` in `src/sim/production/production_repair.rs`; the
skipped sounds are `QuietSounds` in `src/app/match_audio.rs`.
