# Optional Ares Ivan bombs, abductors and weapon extras

Build with `--features ares`. Default builds ignore every key below and keep
the stock Crazy Ivan bomb, capture and FireAt behaviour.

```ini
[MyBomber]                       ; rulesmd.ini WeaponType, Warhead with IvanBomb=yes
IvanBomb.Delay=200               ; fuse frames, else [CombatDamage]IvanTimedDelay
IvanBomb.Damage=300              ; else IvanDamage
IvanBomb.Warhead=SuperWH         ; else IvanWarhead; none blasts nothing
IvanBomb.DeathBomb=yes           ; on an enemy: no fuse, goes off with its carrier
IvanBomb.DeathBombOnAllies=no    ; the same on an ally of the planter
IvanBomb.DestroysBridges=yes     ; a bombed bridge-repair hut drops its bridge
IvanBomb.Detachable=yes          ; no: Engineers cannot defuse it
IvanBomb.DetonateOnSell=yes      ; a sold carrier's bomb goes off
IvanBomb.TickingSound=MyTick     ; else [AudioVisual]BombTickingSound; none silences
IvanBomb.AttachSound=MyAttach    ; else BombAttachSound
IvanBomb.Image=MYCLOCK           ; MYCLOCK.shp in place of BOMBCURS.SHP
IvanBomb.FlickerRate=8           ; else IvanIconFlickerRate

[MyGrabber]                      ; WeaponType
Abductor=yes                     ; the target goes into the firer's hold
Abductor.ChangeOwner=no          ; yes: it joins the firer's house
Abductor.Temporal=no             ; yes: abduct when a warp erases it instead
Abductor.AbductBelowPercent=1.0  ; health ratio at or below which it is taken
Abductor.MaxHealth=0             ; positive: at most this much health
Abductor.Anim=WARPOUT            ; an AnimType played at the abductee

[MyGun]                          ; WeaponType
ProjectileRange=10               ; cells a Ranged= bullet may fly
ApplyDamage=                     ; yes/no: the shot carries Damage=; default
                                 ; unless IsSonic= or UseFireParticles=

[MyTank]                         ; TechnoType
ImmuneToAbduction=no
Passengers.BySize=yes            ; the hold counts Size= (abduction only here)
```

## Ivan bombs in Ares builds

`PlantBomb` (Ares.dll 10055AE0) remembers the planting weapon on its bomb,
and every bomb hook reads that weapon's keys over the Rules ones. Any Techno
plants (0x438E86), and a warhead with `CellSpread` of 0.5 or more plants on
every Techno within `(int)CellSpread` rings of its target's cell but the
planter (10056AF0). Ares also changes three stock rules for every bomb: a
building keeps its bomb when captured (0x448277 resumes past the defuse), a
deploying MCV or undeploying building hands its bomb to what it becomes
(10045A80), and a cloaking planter keeps its bombs' credit (0x725961).
Aircraft keep an illegal target on the sixteenth-frame check (0x6FA4C6).

## Abductors

`conductAbduction` (Ares.dll 10054A80) runs for a bullet snapped to a Foot
target (0x46920B), whose bullet is then neutered, or for a warp that erases
its target (0x71AAAC). It checks the type, the warhead's CanAffectHouse, the
Iron Curtain, health and the hold (CanHoldPassenger 100448E0), stops and
frees the target from its team, captor, captives, warp chain, spawns and
slaves, plays the anim, may change its owner, takes it off the map and adds
it to the hold.

## Where VERA20K owns it

- `rules/ares_weapon.rs`: `AresIvanBomb` and `AresAbductor` (reader
  10055520, constructor 100548B0); `rules/weapon_type.rs`: `ApplyDamage`
  and `ProjectileRange`; `rules/object_type.rs`: the two TechnoType keys.
  `IvanBomb.Warhead` and `Abductor.Anim` resolve in the ordered processor;
  the two cues bind with the other Rules sounds.
- `sim/bomb.rs`: the plant, fuse, blast, sale detonation, transfer and the
  clock frame (100568C0). `sim/combat/fire_error.rs` and the Engineer's
  cursor and click read `Simulation::bomb_detachable`.
- `sim/abduction.rs`: conductAbduction; `sim/passenger.rs`: its hold tail.
- `sim/projectile.rs`: the Range a FireAt bullet carries and spends.
- `render/selection_overlay.rs` and `app/presentation/ui_overlays.rs`: the
  `IvanBomb.Image` art (0x6F523C).

## Not covered

- The player's Detonate action for an own bomb (`IvanBomb.CanDetonate*`,
  [General]`CanDetonateTimeBomb`/`CanDetonateDeathBomb`, the What_Action and
  Berzerk click hooks): not read, no action.
- Abduction of a spawned or enslaved unit, a slave master's manager, the
  locomotor reset, a Building abductor's Absorbed flag and the Ares trigger
  events 0x47..0x4A (see `sim/abduction.rs`).
- `Cursor.Attack`/`Cursor.AttackOutOfRange`: there is no Ares
  `[MouseCursors]` table to name a cursor from.
- When a sale's bomb destroys the sold object, the sale stops there.
