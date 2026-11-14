# Computer players pick the nearest enemy

A VERA option, off by default. With it off, computer players choose their
enemy as in the original game.

```ini
[General]
AINearestEnemy=yes   ; default no
```

A mod, a map or your [personal rules](personal-rules.md) (`VERARULES.INI`) can
set it. In an online game every player needs the same rules, which the rules
compatibility check compares.

## What the original does

A computer house without an enemy searches for one
(`HouseClass::AI_Building_Strategy`, `0x004FD538..0x004FD71E`). The search
means to take the nearest house, but reads every candidate's base centre from
the searching house itself (`0x004FD635..0x004FD657`), so all distances are
zero and the first house in the house list wins. Human players come first in
that list, so in a free-for-all every computer player starts with the human
as its enemy.

Anger then only grows on a house that deals damage, falls by one every 100
frames down to one, and equal anger again goes to the earlier house
(`HouseClass::UpdateAngerNodes`, `0x00504790`). The computer players rarely
turn on each other, and the human who shoots back keeps the lead.

Where the mode allows alliances (`AlliesAllowed=yes`, as in Battle) and no
computer player was given a team, there is more: each time a computer player
deploys its Construction Yard, and when one is defeated, every computer
player allies with every other and takes an anger point against each human
(`HouseClass::Computer_Paranoid`, `0x00501640`). The computer players then
fight the humans only.

## What the option changes

- **No banding together.** `Computer_Paranoid` does nothing, so the computer
  players stay enemies of each other and take no anger against the humans
  from it.

- **The search.** The house that is no ally, not defeated and not passive,
  and whose base centre is nearest to the searching house's, takes the anger
  point. Equally near houses are taken in house order. If every candidate is
  an ally the original's choice stands.
- **Equal anger.** When several houses hold the same highest anger, the one
  with the nearer base centre becomes the enemy instead of the earlier one.

Higher anger still wins: a computer player turns on whoever hurts it most,
and returns to its nearest enemy as that anger fades. A house without a base
centre counts as the farthest. No random draw is spent.
