# Tiberian Sun webs

Firestorm's Cyborg Reaper fired web capsules that pinned infantry in place. Yuri's Revenge
dropped the web keys. VERA reads them as Firestorm 2.0 does. They stay off until a rules
file or a map sets `Webby=yes` on a warhead. No retail Yuri's Revenge INI does, so stock
games play as before.

```ini
; RULESMD.INI (or a map)
[WebMass]
Webby=yes                ; a hit webs infantry instead of hurting anything
WebDuration=600          ; frames a web holds a man (default 300)
WebDurationVariation=25  ; plus or minus this many frames, drawn per man (default 25)
WebRadius=2              ; cells around the impact that get webbed (default 2)
Particle=WebSys          ; the web drawn in each of those cells

[CYBORG]
IsWebImmune=yes          ; webs leave this infantry alone

[AudioVisual]
WebbedInfantry=WEBGUY    ; the animation whose shapes a webbed man is drawn from

[WebSys]
HoldsWhat=Web
BehavesLike=Web
Lifetime=30

[Web]
BehavesLike=Web
MaxEC=80
EndStateAI=10
StateAIAdvance=2
DeleteOnStateLimit=yes
```

```ini
; ARTMD.INI
[E1Sequence]
Struggle=0,6,0           ; the frames a webbed man plays, from WebbedInfantry's image
```

- A Webby warhead does no area damage. Every cell within `WebRadius` of the impact (a
  circle, `dx*dx + dy*dy <= WebRadius*WebRadius`) gets the warhead's `Particle=` system.
  Then each object standing there takes a zero-damage hit, before the next cell is
  visited. A cell more than three levels from the impact's height uses its bridge deck
  instead of the ground.
- An infantryman who is not `IsWebImmune` is webbed for `WebDuration` plus a random
  `-WebDurationVariation..WebDurationVariation`. A new web replaces his current one only
  when it is not shorter than the time left. He takes no damage. Vehicles, buildings and
  web-immune infantry are left alone.
- A webbed man plays `Struggle` from a random frame. He cannot move, fire, deploy or
  recloak, and the player cannot order him. If he was walking, he stops where he is.
  When the web runs out he lies down in `Prone` and carries on.
- Struggle is drawn from `WebbedInfantry`'s image when that key is set. Without it, the
  man's own image is used. A type without `Struggle=` cannot struggle: he is still held,
  in whatever he was doing.
- A unit with a Webby weapon and another weapon fires the web gun only at infantry it can
  web, and its other weapon at everything else: a webbed or web-immune man, a vehicle, a
  building, a bridge cell. A unit whose only weapon webs fires it at everything (see the
  residuals). The
  computer's target scan does not pick a man who is already webbed for more than a
  quarter of its own web gun's duration.
- A vehicle whose chosen weapon is its web gun, aimed at a man still webbed for more than
  a quarter of that gun's duration, drops him and goes back to Guard instead of firing.
  A vehicle whose only weapon is a web gun does this. One with two weapons does not
  choose the web gun against a webbed man, so it keeps firing its other weapon at him
  (see the residuals).
- `BehavesLike=Web` particles stay where they are and animate until `EndStateAI`. A web
  particle with a Webby `Warhead=` webs the men in its cell again each frame. Any other
  warhead does nothing there: its hit does zero damage, which Firestorm ignores. Retail
  Firestorm's `[Web]` has `Warhead=none`.

Firestorm 2.0 evidence: the warhead keys in `WarheadTypeClass::Read_INI` `0x0066EE30`
(defaults from the constructor `0x0066EB10`), `IsWebImmune` in `InfantryTypeClass::Read_INI`
`0x004DA8E0`, `WebbedInfantry` in the Rules AudioVisual reader (`0x005C8498`); the Webby
branch of `BulletClass::Detonate` `0x004465B0`; the web arm of `InfantryClass::Take_Damage`
`0x004D2380`; `InfantryClass::Doing_AI` `0x004D8A20` and `Get_Image_Data` `0x004D9030`;
`Is_Immobilized` (`0x004D5B10`) and its readers; `TechnoClass::What_Weapon_Should_I_Use`
`0x00629F20`, `Evaluate_Object` `0x0062D516` and `UnitClass::Firing_AI` `0x0064E7A3`; the
particle AIs `0x005A7170` (system) and `0x005A4310` (particle) and the BehavesLike name
tables `0x00707B20` and `0x00707D0C`. Code: `sim::combat::world_receiver::web_burst`,
`Simulation::infantry_take_web`, `sim::particles::web` and `rules::warhead_type::WarheadWeb`.

Residuals:
- Off the map, Firestorm still visits the cells a large `WebRadius` reaches and puts a web
  system on its dummy cell. VERA skips those cells.
- A webbed man's Struggle starts on a random frame. Firestorm draws it from a generator
  other than the scenario's (`0x0074BE20`, at `0x004D6643`). VERA's single port of the
  action call uses the scenario's, as Yuri's Revenge does. Only games with webs draw it.
- The web systems belong to no house, as in Firestorm. A null `Particle=` spawns nothing.
  Trees are not hit, since zero damage does nothing to them.
- Firestorm resets a man's web when a game is loaded. VERA keeps it.
- Firestorm raises trigger event 53 (Paralyzed) on a webbed man. Yuri's Revenge uses that
  number for another event, so VERA does not raise it.
- Firestorm's weapon choice first drops a weapon that cannot fire at all just then, such
  as one that is reloading. VERA keeps Yuri's Revenge's checks and puts the web rule after
  them. Firestorm has one reload timer for both weapons (`+0x128`, read in `Can_Fire`
  `0x0062FA12`), so after every shot both weapons read as reloading and the choice falls
  back to the primary. A unit whose primary is the web gun therefore picks it at a
  webbed man while it reloads, drops him and goes back to Guard. In VERA it fires its
  other weapon at him instead. With the web gun as secondary, neither game drops him.
- The computer's web filter runs after Yuri's Revenge's own target checks, just before
  the score, rather than where Firestorm's `Evaluate_Object` has it. Each of them only
  turns a target down, so the order changes nothing.
- Firestorm's weapon choice does not ask whether both weapons exist. A unit whose only
  weapon webs picks its empty slot for anything it cannot web, and holds its fire
  (`UnitClass::Firing_AI` `0x0064E7CD`). Yuri's Revenge's choice answers the primary
  first when a slot is empty (`0x006F3391`), and VERA keeps that, so such a unit fires
  its web gun at everything: at a vehicle, and at a webbed man once a quarter of his
  web is left, which webs him again before he lies down. Retail Firestorm's Reaper has
  two weapons.
- A deployed infantryman, such as a GI behind sandbags, struggles like any other and
  then lies down in `Prone`, no longer deployed. Firestorm has no deployed infantry and
  Yuri's Revenge has no webs, so this is VERA's choice.
