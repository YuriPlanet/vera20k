# Optional Ares firestorm walls

Build with `--features ares`. Ares 3.0 brings back Tiberian Sun's firestorm
wall as a superweapon (`Type=Firestorm`, SW_Firewall) that powers a House's
`Firestorm.Wall=` building sections while it drains. Default builds read none
of the keys below and keep the stock `FirestormWall=` paths.

```ini
[SuperWeaponTypes]
0=FirestormSpecial

[FirestormSpecial]
Type=Firestorm                  ; Action=None, UseChargeDrain=yes and
RechargeTime=2                  ;   SW.CreateRadarEvent=no are forced
SW.ChargeToDrainRatio=1.0       ; the common charge-drain keys apply

[GAFWLL]                        ; BuildingType
Firestorm.Wall=yes              ; a section of the firestorm wall
GuardRange=8                    ; how far a placed section links (cells)
IgnoresFirestorm=no             ; any ObjectType: the firestorm spares it

[AudioVisual]
FirestormActiveAnim=GAFSDF_A    ; defaults: Find of these four names on
FirestormIdleAnim=FSIDLE        ;   the rulesmd.ini pass
FirestormGroundAnim=FSGRND      ; a victim below 100 leptons
FirestormAirAnim=FSAIR          ; a higher one

[CombatDamage]
FirestormWarhead=               ; unset: C4Warhead=

[General]
DamageToFirestormDamageCoefficient=0.0 ; drain frames lost per point of damage
```

## What it does

A House's firestorm is up while its Firestorm Super drains, or after trigger
action 92 (Activate Firestorm) until action 93, the drain's end, its loss or
hold, a second click or low power (the House update drops it whenever the
House is short of power). Every change updates each of the House's sections:

- A section shows its four same-House neighbours (north, east, south, west)
  in its body frame, plus 32 while powered. A powered section that is not a
  straight piece shows `FirestormActiveAnim`, and flickers with
  `FirestormIdleAnim` (one Scenario draw per Update on frames not divisible
  by 8, 1 in 16 starts it).
- A powered section burns every Foot in its cell (its Health as
  `FirestormWarhead`, no attacker, ignoring defenses), and a Foot that
  stands in one burns at its next update; Feet in open-topped transports and
  `IgnoresFirestorm=yes` types are spared. A burn shows the ground or air
  anim.
- A shot whose path crosses a powered section of another House is
  retargeted at it, and its firer may drop its target at its next scan. A
  bullet that enters such a cell burns there without detonating; an
  `Inviso=` shot burns before it is placed (whatever its
  `IgnoresFirestorm=`), and its launch counts as failed.
- A powered section takes no damage: `F2I(damage *
  DamageToFirestormDamageCoefficient)` frames come off the charge timer of
  the House's first Firestorm Super instead.
- Infantry and vehicles cannot enter a powered section and walk through an
  unpowered one; a powered section cannot be sold and shows no health bar.
- Each change ends with `Update_Pathfinding_1` (RebuildZoneConnectivity
  `0x0056C510`, from the retained classes) and `Update_Pathfinding_2`
  (`0x00586990`) over the sections' cells, whose Recalc reaches
  RecalcZoneType's building arm: a section of a House whose firestorm is
  up makes its cell Impassable (`0x00483D9E`, Ares `10014B70`, hook
  `0x483D94`), so the zone hierarchy no longer connects through a powered
  wall; dropping it gives the cell back its terrain class. A restored game
  gives powered sections' cells the class again before its navigation
  rebuild.
- Placing a section (by the player or the computer) fills the gap to a
  same-House or allied section up to `GuardRange` cells away in each straight
  direction with new sections that build up; the placement cursor marks
  those cells for a section of a human House.

## Where VERA20K owns it

- `rules/ares_firestorm.rs`: RulesExt `+0xF0..+0x108` (`1002AAF0`,
  `1002A800`); `rules/ares_building_type.rs`: `Firestorm.Wall` (`+0x5D`)
  and `IsTrench` (`+0x84`);
  `rules/object_type.rs` and `rules/ares_bullet_type.rs`: the stock
  `IgnoresFirestorm=` (`0x005F92D0`); `rules/superweapon_type.rs`: the kind
  and SW_Firewall::LoadFromINI (`10071620`).
- `sim/ares_firestorm.rs`: the Houses whose firestorm is up
  (HouseClass+0x1FA, saved and hashed) and each section's frame
  (Building+0x61C, saved and hashed); SetFirestormState `10022590`,
  UpdateFirewall `10013280`, UpdateFirewallLinks `10013630`,
  IsActiveFirestormWall `10012770`, ImmolateVictim, FindFirstFirestorm
  (`10014DF0`, hook `0x5880A0`), buildLines `100113D0`, the placement
  shadow (`0x006D59D0` under hooks `0x6D5455`/`0x6D5A5C`) and
  SetFirestormState's zone batch over the sections' cells.
- `sim/superweapon/firewall.rs`: Activate `100715C0`, Deactivate
  `100715F0`, through the charge-drain owners in `superweapon/fire.rs` and
  `superweapon/mod.rs`.
- Hook sites: building Update `0x440378` (`techno_ai.rs`), Put `0x440D01`
  and Remove `0x445DF4` (`lifecycle.rs`), ReceiveDamage `0x4423E7`
  (`combat/world_receiver.rs`), CanSell `0x449518` (`production_sell.rs`),
  GetCurrentFrame `0x43EFB3` (`building_art.rs`), Foot update `0x4DA53E`
  and InfantryAI `0x51BD4C` (`object_turn.rs`), Can_Enter_Cell `0x51C4C8`
  and `0x73F7B0` (`object_entry.rs`), Fire `0x6FF008`/`0x6FF860` and the
  Inviso arm of `0x4688BD` (`combat/projectile_delivery.rs`), bullet Update
  `0x467B94` (`projectile.rs`, `world/projectile_collision.rs`),
  Place_Production `0x4FB257` and ExitObject `0x445355`
  (`production_placement.rs`, `ai_base_building.rs`), the low-power hook
  `0x4F8C97` (`house_eva.rs`, which also tests every `BuildConst=` entry),
  trigger actions 92/93 (`trigger_runtime_ares.rs`), and the client health
  bar `0x6F64CB` (`app/presentation/ui_overlays.rs`).

## Not included

- RecalcZoneType's building arm runs only in SetFirestormState's batch
  (`map::resolved_terrain::recalc_zone_type_with_buildings`, from
  `Simulation::ares_firestorm_update_pathfinding`). VERA's other Recalc
  writers (an overlay change on the cell, the playfield resize of trigger
  action 40) take the terrain inputs alone, and a section placed or
  removed while the firestorm is up keeps its cell's class until the next
  toggle, where native's placement Recalc applies the arm at once.
  Effect: the zones connect through that one section until the next
  toggle; the path grid blocks it either way. Frequency: building a
  section during a firestorm.
- Trench lines (`IsTrench`, the other half of `IsLinkable`) link through
  the same line building and placement shadow, and trench traversal
  moves occupants between neighbouring trenches
  ([ares-building-hooks.md](ares-building-hooks.md)).
- The `FirewallToggle` network event (`0x61`): Ares 3.0 handles it
  (Networking_RespondToEvent `1006A540`, a SetFirestormState toggle) but
  never raises it (the DLL holds no raiser; the restored
  RaiseFirewallToggle has no caller), so no player action produces it
  there either.
- The low-power drop and the tech-tree recheck of a click run, natively,
  for the local player's House only; VERA runs them for every
  human-controlled House.
- An explicit `FirestormWarhead=none` passes a null warhead natively; VERA
  drops that damage and still shows the burn anim.
