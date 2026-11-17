# Minimap beacons

A VERA feature; in the original game a beacon can only be placed on the
battlefield.

Tick **Minimap Beacons** on the Controls page of Options, in the main menu or
in the menu Escape opens during a match. Then press **B** (Place Beacon) and
left-click the minimap: the beacon goes on the cell you clicked, as if you had
scrolled there and clicked the battlefield. It is off by default, takes effect
at once and is saved with your other options (`[VERA] MinimapBeacons=` in the
options file).

## What it does

- With the box off, a minimap click in Place Beacon mode only moves the view,
  as in the original game.
- With the box on, the minimap shows the beacon cursor while Place Beacon mode
  is on and nothing is selected. A click places the beacon at that cell and
  ends the mode, exactly as a battlefield click does: the same three-beacon
  limit, the same announcement, sound and two help lines, and your allies see it on
  their battlefield and minimap.
- It works with Right-Click Orders too: the beacon click stays on the left
  button there, as on the battlefield.
- A minimap click that already means something keeps that meaning: with units
  selected it is their order, and with a superweapon ready to fire it is the
  superweapon's.

## What it is and is not

- Each player chooses for themselves. The beacon is sent as the ordinary
  beacon message, so allies on other versions, or with the box off, see it.
- Nothing changes in the match itself: beacons are not part of the game state,
  replays or saved games.

## Code

`with_minimap_beacon` in `src/app/input/radar.rs` answers the minimap click;
`band_box_left_up` in `src/app/input/dispatch.rs` places the beacon through
`place_at_cell` in `src/app/input/beacon.rs`.
