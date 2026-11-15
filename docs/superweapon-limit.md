# Superweapon Limit

Each superweapon can be built up to the limit you choose, from 2 to 20, and each building charges
its own shot. With a limit of ten, ten Nuclear Missile Silos charge ten missiles, each on its own
timer. It is off by default. Move **Options → Game Options → Superweapon Limit** off its left end
to switch it on for the next game. A LAN or online room plays its host's setting, which the host
can change on the room's Game Options slider. A save keeps
the rules it was started with. Quick Match plays the original game, without it. Like Terrain
Deformation, the setting also reaches campaign missions.

The sidebar keeps one button for each superweapon. A launch uses the first charged shot, and the
button is ready again at once while another shot is charged. When more than one shot is charged,
the button shows how many in its top right corner. Each shot takes the full recharge time, and low
power holds all of them. Losing a building loses the charge that had the furthest to go, and a spy
in a superweapon building costs one charged shot, not all of them. Nukes fired in quick succession
leave from different silos, each missile flying to its own target.

## Rules keys

The setting switches on these keys for each building in `[BuildingTypes]` that has `SuperWeapon=`
or `SuperWeapon2=` and a `BuildLimit=` of at least 1 and below the limit; `BuildLimit=` becomes the
limit. `[VERA] SuperweaponLimit=` in VERA.INI keeps it, 0 for off. In the retail rules those are the six
superweapons: the Nuclear Missile Silo, Iron Curtain, Weather Control Device, Chronosphere,
Psychic Dominator and Genetic Mutator. Battle Labs, radars and other buildings that give a
superweapon have no limit, so they keep the house's one charge. A mod or a map can set the
keys as well, with any limit, and a map can switch them off for its own game:

```ini
[NAMISL]
BuildLimit=10
SuperWeaponPerBuilding=yes   ; default no
```

With `SuperWeaponPerBuilding=yes`, each of the house's buildings of this type charges its own shot
of the superweapons it gives. gamemd keeps one charge per superweapon however many buildings give
it, which is still what a building without the key gets. The superweapon's own timer charges the
first building's shot, and VERA keeps the others' beside it, counting them down each frame while the
superweapon is granted and not on hold. When a launch would restart the recharge, it takes the
charge that is closest to ready instead, and that building starts again from the full
`RechargeTime=`. A superweapon with `ManualControl=`, `UseChargeDrain=` or `PostClick=`, or one a
trigger or spy grants once, never has more than its one charge.

A launch from a `NukeSilo=` building with the key takes the first such silo that is not already
launching and gives that silo its own target, where gamemd's first silo reads the house's one nuke
target. A reset, such as a spy's, restarts one charge the way a launch does.

The extra charges and silo targets are saved with the game and are part of the simulation hash.
Without the key the game plays as gamemd: no extra charges, no extra state in the hash, and the
same random draws.

Not covered yet: a building switched off or under EMP keeps charging its own shot while another
building keeps the superweapon granted; with Ares, its own nuke launch still uses one silo; the
Phobos superweapon sidebar shows no count; and the computer players build their base from their
own lists, so they rarely build more than one of each.
