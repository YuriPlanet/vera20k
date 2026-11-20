# Drop pods

A Foot riding a drop pod (an Ares `Type=DropPod` superweapon's unit) descends
on its own ballistic path, trailing smoke, with an optional landing animation,
puff and weapon. Phobos makes every value of that descent settable per
non-building TechnoType; the keys are read in
`Ext/TechnoType/Body.cpp:1430-1445` from `DroppodTypeClass::LoadFromINI`
(`src/New/Type/Affiliated/DroppodTypeClass.cpp:7-21`) and consumed by its two
locomotor hooks, `DroppodLocomotionClass_ILoco_MoveTo @ 0x4B607D` and
`_ILoco_Process @ 0x4B5B70`.

```ini
[UNIT]                               ; non-building TechnoType
DropPod.Angle=                       ; double radians, default [General] DropPodAngle
DropPod.AtmosphereEntry=             ; anim, default [AudioVisual] AtmosphereEntry
DropPod.GroundAnim=                  ; two anims: item 0 landed, item 1 out-of-map; default [General] DropPod
DropPod.AirImage=                    ; SHP file, default POD.SHP (presentation)
DropPod.Height=                      ; int, default [General] DropPodHeight
DropPod.Puff=                        ; anim, default [AudioVisual] DropPodPuff
DropPod.Speed=                       ; int, default [General] DropPodSpeed
DropPod.Trailer=                     ; anim, default [General] DropPodTrailer (Ares), then SMOKEY
DropPod.Trailer.SpawnDelay=          ; int frames, constructor default 6
DropPod.Trailer.Attached=            ; boolean, constructor default no
DropPod.Weapon=                      ; weapon, Read<true>, default [General] DropPodWeapon
DropPod.Weapon.HitLandOnly=          ; boolean, constructor default no
```

There is no `DropPod=` enable key in Phobos: `TechnoTypeExt::LoadFromINI`
creates the settings block for every TechnoType whose `WhatAmI()` is not
`BuildingType`, and resets it for buildings. The keys only shape a pod that
type rides; with every key unset the descent is the stock one. Each
`Nullable` key falls back to its `[General]`/`[AudioVisual]` value also when
its value is blank, `none`, `<none>` or `<default>`; `DropPod.Weapon` is read
with `Read<true>`, so a weapon named only here is constructed on the rules
pass (VERA: `native_processing::allocate_droppod_weapon_references`).

## Behavior

`MoveTo` (hook `0x4B607D`) places the Foot `DropPod.Height=` above the
destination's ground, run back along `DropPod.Angle=` toward -X (toward +X
when that point is outside the playfield, which also records OutOfMap), and
plays `DropPod.AtmosphereEntry=` after the Unlimbo.

`Process` (hook `0x4B5B70`) rewrites the descent for every pod:

- speed is `max(DropPod.Speed, current height / 10 + 2)`, so a small value
  decelerates exponentially;
- X moves by `cos(angle) * speed`, toward +X for an OutOfMap pod, Z falls by
  `sin(angle) * speed`;
- the trailer anim spawns when `(CurrentFrame - ParalysisTimer.StartTime) %
  DropPod.Trailer.SpawnDelay == 1`, with `DropPod.Trailer.Attached=` running
  `SetOwnerObject` on the pod; the trailer falls back to Ares'
  `[General] DropPodTrailer=`, then `SMOKEY`;
- with a weapon and no `HitLandOnly`, every `max(ROF, 3)` frames, unless the
  nearest object of the destination's cell is an ally, the weapon strafes a
  random point 0x55 leptons from the destination for twice its `Damage=`;
- on landing it plays `DropPod.Puff=` and `DropPod.GroundAnim=[OutOfMap]`,
  and, with `HitLandOnly`, detonates the weapon once at the landing point
  (`WeaponTypeExt::DetonateAt`, a targetless bullet with the pod as owner);
- every anim it creates takes the pod's house (`SetAnimOwnerHouseKind`,
  `ownedObject = true`), which the stock hooks do not set.

## Where VERA20K owns it

- `rules::phobos_droppod::PhobosDropPodRules` reads the twelve keys;
  `ObjectType::phobos_droppod` holds one block per non-building type.
- `sim/superweapon/drop_pod.rs` consumes them in
  `drop_pod_move_to` (Height/Angle/AtmosphereEntry) and
  `process_drop_pod_locomotor`/`land_drop_pod` (speed, trailer, weapon
  cadence, HitLandOnly, Puff, GroundAnim). The effective anim is the type's
  name only when that anim type exists (`AnimTypeClass::Find` would answer
  null otherwise, leaving the `Nullable` unset).
- The landing detonation goes through the shared
  `sim::kill_weapon::weapon_detonation_at_point` /
  `commit_attach_effect_detonations` owner, the same one `WarpInWeapon` uses.

Without the Phobos capability the stock cadence (`frame % 6` trailer,
`frame % 3` strafe, no landing weapon) and the global values are unchanged.
With it compiled in, every drop pod runs the Phobos Process, which is what
Phobos installs at the same address.

Draws and timers: no new RNG draws. The strafe keeps its Scenario direction
draw (`0x0049F420`) and its Main `Next` report pick; the landing detonation
draws what its bullet and warhead constructors draw. No timer is written; the
cadence reads the Foot's `ParalysisTimer.StartTime` (the Foot constructor's
creation frame) and the binary frame.

## Residuals

- `DropPod.AirImage=` is parsed but only an InfantryType draws it (Phobos
  swaps the infantry's SHP while it rides); VERA's presentation does not draw
  the pod image. Trigger: an infantry pod in flight; visual only.
- Phobos erases `DroppodOnly` laser trails at landing. VERA's trails are
  AttachEffect-owned, not a techno vector, so the erase is not ported; the
  draw-time suppression is also presentation. Trigger: a `DropPodOnly` trail
  on a pod rider.
- `DropPod.Trailer.SpawnDelay=0` divides by zero in native; VERA spawns no
  trailer for a non-positive delay. Trigger: a zero key; no retail data.
- The strafe's damage-explosion anim still has no owner house (the shared
  explosion effect owner); Phobos sets the pod's house on it. Trigger: any
  strafe or refused landing; color remap only.
- An anim name that names no registered type falls back to the global (the
  native `Nullable` stays unset). A `DropPod.GroundAnim` token that names
  nothing is dropped by the native vector read, which can make a second valid
  token the landing anim; VERA keeps the token, so that slot shows no anim.
