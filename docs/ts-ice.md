# Tiberian Sun ice

Firestorm's snow maps had ice that gives way. A heavy unit driving onto ice cracks it with
a sound; a heavier one, or a unit on ice that is already cracked, breaks it and sinks, and
so does every other vehicle standing there. A crack freezes over after a while, and open
water the map marks for ice grows back. Yuri's Revenge still reads the rules keys but has
no ice code. VERA runs Firestorm's ice on a Snow theater whose theater INI gives the three
ice sets their 64 tiles each and the ice shore set at least one tile. Retail `SNOWMD.INI`
names all four sets with no tiles in them, so stock games play as before: no extra random
draw, timer or save data.

```ini
; SNOWMD.INI (the theater)
[General]
Ice1Set=...       ; three tilesets of 64 ice tiles each, in Firestorm's order:
Ice2Set=...       ; 0 solid ice, 1-15 smoothed edges, 16 cracked,
Ice3Set=...       ; 17-62 ice/water edges, 63 open water
IceShoreSet=...   ; must hold tiles, or ice stays off
ShorePieces=...   ; ice beside these tiles takes an edge piece
WaterSet=...      ; open water, which a break may take in

; RULESMD.INI (or a map)
[General]
IceCrackingWeight=2.0   ; a unit at least this Weight= cracks the ice it drives onto
IceBreakingWeight=4.0   ; at least this heavy, it breaks it

[AudioVisual]
IceGrowthRate=1.0           ; minutes between growth passes; 0 turns growth off
IceSolidifyFrameTime=500    ; frames before a crack freezes over
IceCrackSounds=IceCrack1,IceCrack2   ; one plays at random per crack

; the map
[Basic]
IceGrowthEnabled=yes    ; growth and freezing over (default yes)
```

- A vehicle that arrives on a cell with `Weight=` at least `IceBreakingWeight=` breaks the
  ice there; one at least `IceCrackingWeight=` cracks it, and cracks it through if it is
  already cracked. Infantry and aircraft never crack ice.
- A crack takes one of the three families' cracked tile at random, plays one of
  `IceCrackSounds=` at full volume and freezes again `IceSolidifyFrameTime=` frames later.
  Its sides that are also cracked freeze with it.
- A break takes a 2x2 block from the cell, stepping along the breaker's facing, and only
  when all four cells hold ice or water. Each becomes broken ice (water to every unit), and
  the ice around it re-edges itself. Infantry and aircraft on those cells are removed with
  no kill; their trigger events 48 and 29 fire first. Other vehicles sink, except
  amphibious ones (`MovementZone=Amphibious`, `AmphibiousCrusher`,
  `AmphibiousDestroyer`), which stay afloat. The unit that broke the ice sinks whatever its
  movement zone. A sinking unit drifts three leptons a frame along its facing and nine
  down, then is gone once it is more than 400 leptons under the ground, counted as a loss
  with no attacker. A `Wake` anim marks each one.
- A `Wall=yes` or `Fire=yes` warhead cracks the ice it hits, or breaks cracked ice, unless
  it went off at a bridge deck over that cell.
- Every `IceGrowthRate=` minutes, with `IceGrowthEnabled=yes`, broken ice and open water of
  the first two families refreezes on the cells the map's IsoMapPack5 marks for ice.
- Navigation, the radar and the minimap follow each change, and saves keep the ice.

Firestorm 2.0 evidence: Read_Theater `0x4F3950` (the set keys) and its land patch
`0x4F54A5..0x4F5562` (edge and water tiles are Water); RandomIceSet `0x4F89B0`; Update_Ice
`0x51EB40`, Smooth_Ice `0x51EF90`, Edge_Ice `0x51FB30` with its table `0x6D3C04`;
Crack_Ice `0x520670`; Break_Ice `0x520870` with its footprint table `0x8058E0`; Grow_Ice
`0x520D20`; Solidify_Ice `0x520EB0`; Recalc_Ice `0x52C580`; the LogicClass::AI steps
`0x506EB7..0x506F6B` and `0x506F84`; UnitClass::Per_Cell_Process `0x651890..0x651975`;
Explosion_Damage `0x4601B4..0x46024F`; the UnitClass::AI sink `0x64DFA3..0x64E122` with
its drift table `0x80F818`; the Scenario constructor `0x5DAAF0..0x5DAAFB` (the growth
timer). gamemd reads the rules keys at ReadGeneral `0x66F192`/`0x66F1B8` and
ReadAudioVisual `0x66B523`, `0x66B543` and `0x66B550..0x66B596`, and keeps
`IceGrowthEnabled=` at Scenario+0x34A8. Code: `sim::world::ice`,
`map::resolved_terrain::ice`, `rules::ice_rules` and the ice arm of `sim::world::sinking`.

Residuals: Firestorm runs ice on every Snow map whatever its sets hold; VERA needs real
tiles, so a theater with partial sets has no ice. Firestorm empties the crack sound list
before each rules layer reads it; VERA keeps the last layer that names the key. A breaker
counts as on a bridge by its OnBridge state, not Firestorm's height test, and an explosion
by the cell's bridge head, not its bridge flag. `Weight=` keeps float precision, so a
weight within that of a threshold may compare differently. Ice hooks into gamemd's own
arrival and explosion code at the matching point, so an arrival gamemd ends early (docking,
boarding a transport, being absorbed) or an explosion it ends early cracks nothing. Kept as Firestorm has them: an
explosion's break takes the hit cell four times; a facing north-west drifts a sinking unit
straight down; a crack that broke stays in the freeze list for good; Edge_Ice redraws open
water even when it is unchanged; water counts as breakable, and with no `WaterSet=` tiles
0 to 12 count as water. VERA's other readers of a sinking unit treat one sinking through
ice as a sinking ship (no targeting, no orders). Each break rebuilds the navigation zones
in full.
