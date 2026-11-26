# Tiberian Sun Ion Cannon

Tiberian Sun's GDI Ion Cannon fired a beam from orbit at one cell. Yuri's Revenge kept the
blast, which its "Ion Cannon strike" trigger action still uses, but dropped the
superweapon, so gamemd reads `Type=IonCannon` as an unknown name. VERA adds it back, read
as Firestorm 2.0 reads it. Retail RULESMD.INI still holds Tiberian Sun's
`[IonCannonSpecial]` with `Type=IonCannon`, but does not list it under
`[SuperWeaponTypes]`, and no retail map does, so stock games play as before.

```ini
; A mod's RULESMD.INI, or a map
[SuperWeaponTypes]
; ...the retail entries 1 to 12, then:
13=IonCannonSpecial

[IonCannonSpecial] ; retail RULESMD.INI has these keys; a map repeats them
Type=IonCannon
Action=IonCannon   ; the player aims it at a cell
RechargeTime=8.5   ; minutes
IsPowered=true

[MYUPLINK]         ; the building that grants it
SuperWeapon=IonCannonSpecial

; The blast reads keys Yuri's Revenge already has:
[General]
IonBlast=RING1     ; the ring (the last SplashList= anim on water)
IonBeam=IONBEAM
[CombatDamage]
IonCannonDamage=751
IonCannonWarhead=IonCannonWH
```

- A type's keys are read only from the file that lists it and the files read after that
  one, in the order RULESMD, the mode INI, the map. A mod's RULESMD.INI that lists `IonCannonSpecial` gets
  the retail keys. A map that lists it must give `Type=IonCannon`, `Action=IonCannon` and
  the rest itself; without them the type has no `Type=`, and its cameo does nothing.
- The player clicks the cameo, then a cell. The blast lands on the centre of that cell, at
  the height of its level; a slope does not raise it. It is the same blast as the trigger
  action's: the ring and beam, `IonCannonDamage=` with `IonCannonWarhead=` (on a bridge
  deck first, then the ground), a flash, and the ripple that rocks nearby units for 79
  frames.
- Use `Action=IonCannon`. Without Ares, a click fires the first superweapon whose `Action=`
  matches, so an action another superweapon uses (such as `Nuke`) fires that one instead; an
  Ares game fires the armed type. Without a
  targeting action the cameo fires at once, at cell 0,0.
- It plays no sound and no EVA line when it fires, as in Firestorm.
- Computer players fire it too, at an enemy object. An object the blast can kill (its
  strength not above `IonCannonDamage=`) is worth its kind's `[General] AIIonCannon*Value=`
  entry, as for the nuke; any other is worth 1, or 3 for a building. A cloaked object gets
  a random value. Unlike the nuke's choice, it also aims at objects past the map's visible
  edge, ignores the trigger-set target cell, counts a war factory that builds ships, gives
  no tech-centre value, and counts an MCV only when it deploys into the first
  `[AI] BuildConst=` entry.
- Ares has no type of this name. In an Ares game it fires the same way, and Ares's
  targeting keys (`SW.AITargeting=` and the like) do not apply to it.

Firestorm 2.0 evidence: the name table `0x0070FED4` (IonCannon is its index 3) and
`SuperWeaponTypeClass::Read_INI`'s `Type=` read `0x0060D215`; its `Action=` read
`0x0060D1F6` through ReadAction `0x0044AB90` (action table `0x006CCC70`, IonCannon is
action 37); `SuperClass::Discharge` `0x0060B730` and `SuperClass::Place` `0x0060BDC0` case
3 (`0x0060BE21..0x0060BEC6`), which builds the IonBlast (`0x004EDB90`); the House AI's
Super loop `0x004C9E20` case 3 and its target picker `0x004C9F30`. Code:
`sim::superweapon::ion_cannon`, `ai_fire::ion_cannon_target` and the blast's owner,
`sim::world::ion_blast`.

Residuals: it aims with the Attack cursor, since Yuri's Revenge has no Ion Cannon cursor.
Like any superweapon past the retail twelve, it cannot be aimed on the radar. Firestorm's
superweapon voice keys (`RechargeVoice=` and the like) name Tiberian Sun speech and are
not read, so no line says it is ready. The blast is gamemd's; Firestorm's own blast was not
compared. The computer fires through gamemd's launch rather than Firestorm's direct discharge, so
computer players are also alerted when the type sets `AIDefendAgainst=`. Firestorm's sidebar refresh after the launch is not ported.
