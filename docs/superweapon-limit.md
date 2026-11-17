# Superweapon Limit

Each superweapon can be built up to the limit you choose, from 2 to 20, and each building charges
its own shot. With a limit of ten, ten Nuclear Missile Silos charge ten missiles, each on its own
timer. It is off by default. On the skirmish screen, press **Game Options** and move **Superweapon
Limit** off its left end; every later game plays it. A LAN or online room plays its host's setting,
which the host can change on the room's Game Options slider. A save keeps
the rules it was started with. Quick Match plays the original game, without it. Like Terrain
Deformation, the setting also reaches campaign missions.

The sidebar keeps one button for each superweapon. A launch uses the first charged shot, and the
button is ready again at once while another shot is charged. When more than one shot is charged,
the button shows how many in its top right corner. Each shot takes the full recharge time, and low
power holds all of them. Losing a building loses that building's own charge; if it was the one the
sidebar counts, the fullest charge left takes its place. A spy in a superweapon building costs one
charged shot, not all of them. Nukes fired in quick succession leave from different silos, each
missile flying to its own target. Each building's charge animations follow its own timer, so of
twenty silos only the ones near ready open, and only the one that fired closes.

**Super Weapon Building Timers** draws the same information above the buildings themselves: every
provider in sight shows the countdowns of the superweapons it gives, in the minutes and seconds the
timer list uses, so the next one to fire and the one that just fired are both visible at a glance.

It is a hotkey, not an Options box: **Ctrl+Alt+T** shows and hides it, and the key is rebindable on
Options' Keyboard page under VERA's own category. The choice is saved in `[VERA]
SuperweaponBuildingTimers=`, which also gives each scenario its starting state (off unless written).

Each line reads `<name> <time>`, where `<name>` is the superweapon's `UIName=` text. A `*` before the
name marks a superweapon each building charges on its own — the Superweapon Limit's per-building
shots — so twenty silos show twenty different countdowns, each over the silo that holds it. A
superweapon without the star is the house's one shared charge, as a Battle Lab's Force Shield or a
radar's paradrop, spy plane or psychic reveal is; every provider of it shows the same number, because
they all point at the same timer. A building that gives several superweapons stacks one line per
superweapon, in its type's own order (`SuperWeapon=`, `SuperWeapon2=`, then Ares' `SuperWeapons=`).

Every House is drawn, enemies included: an enemy silo counting down is what says an attack is
coming, and each line takes its building's own tint, so whose it is stays visible. An enemy draws
only the superweapons its timer list draws anyway — a grant with `SW.ShowTimer=no` (a radar's
paradrop, a Battle Lab's Force Shield) keeps no right-hand line, and a line above the building
would leak a capability the player cannot otherwise read — while the player's own and its allies'
providers draw everything. One under the
shroud draws none — a timer there would give away a silo scouting has not found. The labels are drawn
for every structure the frame retains, whether or not the building's body was planned that frame, and
the countdowns are captured with the frame rather than read from the simulation while drawing, so a
countdown neither blinks out nor disagrees with the timer list. It is a display setting only: it
changes nothing in the match, and it needs no agreement between players.

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
first building's shot, and VERA keeps the others' beside it, each with the building that charges
it, counting them down every frame while the superweapon is granted and not on hold. A launch
restarts the shot of the building that fired and hands the sidebar's timer to the charge closest
to ready, which keeps the frames it had left; every other building keeps counting. A superweapon
with `ManualControl=`, `UseChargeDrain=` or `PostClick=`, or one a trigger or spy grants once,
never has more than its one charge.

A launch from a `NukeSilo=` building with the key leaves from the silo whose charge the house's
ready shot is (the one whose `SuperAnim` opened), falling back to the most charged idle silo, and
gives that silo its own target; gamemd's first silo reads the house's one nuke target. A reset,
such as a spy's, restarts one charge the way a launch does.

The extra charges and silo targets are saved with the game and are part of the simulation hash.
Without the key the game plays as gamemd: no extra charges, no extra state in the hash, and the
same random draws.

Not covered yet: a building switched off or under EMP keeps charging its own shot while another
building keeps the superweapon granted; with Ares, its own nuke launch still uses one silo; the
Phobos superweapon sidebar shows no count; and the computer players build their base from their own
lists, so they rarely build more than one of each.
